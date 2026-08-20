//! Annotations and text anchoring.
//!
//! # Coordinate space
//!
//! Anchors are stored as **character offsets into the Page's plain text**, not
//! as ProseMirror positions. That choice is what lets relocation live here, in
//! pure Rust, where it can be tested exhaustively without a DOM — and anchoring
//! is the one place in Grimoire where a subtle bug silently corrupts meaning
//! rather than loudly failing.
//!
//! The frontend converts between plain-text offsets and editor positions using
//! the same document walk as [`super::text::plain_text_from_document`], and a
//! shared fixture test keeps the two implementations honest.
//!
//! # The rule
//!
//! An annotation that cannot be placed confidently is never placed. Pointing a
//! comment at the wrong sentence is worse than admitting the target is gone, so
//! ambiguity resolves to [`Relocation::Lost`] and the annotation is marked
//! stale for the user to deal with.

use serde::{Deserialize, Serialize};

use super::ids::{AnnotationId, PageId};
use super::manuscript::{Timestamp, now};

/// How much surrounding text an anchor remembers. Long enough to disambiguate
/// a repeated phrase, short enough that ordinary editing nearby does not
/// destroy the match.
pub const CONTEXT_CHARS: usize = 48;

/// Below this, a multi-candidate match is treated as ambiguous.
const CONTEXT_CONFIDENCE_FLOOR: f32 = 0.55;

/// Confidence granted to a unique textual match whose context has changed.
const UNIQUE_MATCH_CONFIDENCE: f32 = 0.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationKind {
    Note,
    Question,
    Suggestion,
    Warning,
    Reference,
    AiReview,
    AiSuggestion,
}

impl AnnotationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AnnotationKind::Note => "note",
            AnnotationKind::Question => "question",
            AnnotationKind::Suggestion => "suggestion",
            AnnotationKind::Warning => "warning",
            AnnotationKind::Reference => "reference",
            AnnotationKind::AiReview => "ai-review",
            AnnotationKind::AiSuggestion => "ai-suggestion",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "note" => AnnotationKind::Note,
            "question" => AnnotationKind::Question,
            "suggestion" => AnnotationKind::Suggestion,
            "warning" => AnnotationKind::Warning,
            "reference" => AnnotationKind::Reference,
            "ai-review" => AnnotationKind::AiReview,
            "ai-suggestion" => AnnotationKind::AiSuggestion,
            _ => return None,
        })
    }

    pub fn is_ai(&self) -> bool {
        matches!(
            self,
            AnnotationKind::AiReview | AnnotationKind::AiSuggestion
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationStatus {
    Active,
    Resolved,
    /// The anchored text could no longer be found, or was found ambiguously.
    Stale,
}

impl AnnotationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AnnotationStatus::Active => "active",
            AnnotationStatus::Resolved => "resolved",
            AnnotationStatus::Stale => "stale",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "active" => AnnotationStatus::Active,
            "resolved" => AnnotationStatus::Resolved,
            "stale" => AnnotationStatus::Stale,
            _ => return None,
        })
    }
}

/// Where an annotation points.
///
/// A whole-Page annotation has no anchor and therefore can never go stale,
/// which is why the distinction is in the type rather than a nullable field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AnnotationTarget {
    Page,
    Range(AnnotationAnchor),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationAnchor {
    /// Character offset into the Page's plain text, inclusive.
    pub from: i64,
    /// Character offset into the Page's plain text, exclusive.
    pub to: i64,
    pub selected_text: String,
    pub context_before: String,
    pub context_after: String,
    /// The `revision_number` the offsets were computed against.
    pub base_revision: i64,
    /// Of `selected_text`, so an unchanged anchor is confirmed without a search.
    pub text_hash: String,
}

impl AnnotationAnchor {
    /// Builds an anchor from a selection in the given plain text.
    ///
    /// The range is clamped to the text, so a selection that arrives slightly
    /// out of date produces a valid anchor rather than a panic.
    pub fn capture(text: &str, from: i64, to: i64, base_revision: i64) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len() as i64;
        let start = from.clamp(0, len);
        let end = to.clamp(start, len);

        let selected: String = slice(&chars, start, end);
        let before_start = (start - CONTEXT_CHARS as i64).max(0);
        let after_end = (end + CONTEXT_CHARS as i64).min(len);

        Self {
            from: start,
            to: end,
            text_hash: hash(&selected),
            selected_text: selected,
            context_before: slice(&chars, before_start, start),
            context_after: slice(&chars, end, after_end),
            base_revision,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.to <= self.from
    }
}

/// The outcome of trying to place an anchor in a changed document.
#[derive(Debug, Clone, PartialEq)]
pub enum Relocation {
    /// The text at the stored offsets is still the anchored text.
    Unchanged,
    /// Found elsewhere, with the confidence of the match.
    Moved { from: i64, to: i64, confidence: f32 },
    /// Gone, or ambiguous enough that guessing would be wrong.
    Lost,
}

/// Attempts to place `anchor` in `text`.
///
/// Strategy, in order:
///
/// 1. The stored offsets still hold the anchored text — nothing moved.
/// 2. The anchored text with its original surroundings appears exactly once.
/// 3. The anchored text appears somewhere; candidates are scored on how much of
///    their surrounding context survives, with distance from the original
///    position breaking ties.
///
/// A single textual match is accepted even when its context has changed, since
/// editing *around* an unchanged phrase is the common case. Multiple matches
/// with weak context are refused.
pub fn relocate(anchor: &AnnotationAnchor, text: &str) -> Relocation {
    if anchor.is_empty() {
        return Relocation::Lost;
    }

    let haystack: Vec<char> = text.chars().collect();
    let needle: Vec<char> = anchor.selected_text.chars().collect();
    if needle.is_empty() || needle.len() > haystack.len() {
        return Relocation::Lost;
    }

    // 1. Still where it was.
    let from = anchor.from.max(0) as usize;
    let to = anchor.to.max(0) as usize;
    if to <= haystack.len() && to - from == needle.len() && haystack[from..to] == needle[..] {
        return Relocation::Unchanged;
    }

    let candidates = find_all(&haystack, &needle);
    if candidates.is_empty() {
        return Relocation::Lost;
    }

    // 2/3. Score every candidate on surviving context.
    let before: Vec<char> = anchor.context_before.chars().collect();
    let after: Vec<char> = anchor.context_after.chars().collect();

    let mut scored: Vec<(usize, f32)> = candidates
        .iter()
        .map(|&at| {
            (
                at,
                context_score(&haystack, at, needle.len(), &before, &after),
            )
        })
        .collect();

    // Best score wins; the nearest to the original offset breaks a tie, because
    // text rarely travels far between edits.
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| distance(a.0, from).cmp(&distance(b.0, from)))
    });

    let (best_at, best_score) = scored[0];

    if best_score >= CONTEXT_CONFIDENCE_FLOOR {
        return moved(best_at, needle.len(), best_score);
    }

    if candidates.len() == 1 {
        // Unique in the document. The context changed, the phrase did not.
        return moved(best_at, needle.len(), UNIQUE_MATCH_CONFIDENCE);
    }

    // Several identical phrases and nothing to tell them apart. Refuse.
    Relocation::Lost
}

fn moved(at: usize, len: usize, confidence: f32) -> Relocation {
    Relocation::Moved {
        from: at as i64,
        to: (at + len) as i64,
        confidence,
    }
}

fn distance(a: usize, b: usize) -> usize {
    a.abs_diff(b)
}

fn find_all(haystack: &[char], needle: &[char]) -> Vec<usize> {
    let mut found = Vec::new();
    if needle.is_empty() || needle.len() > haystack.len() {
        return found;
    }
    for start in 0..=(haystack.len() - needle.len()) {
        if haystack[start..start + needle.len()] == needle[..] {
            found.push(start);
        }
    }
    found
}

/// How much of the remembered context still surrounds a candidate, from 0 to 1.
///
/// Sides are weighted by how much context was recorded, so an anchor at the very
/// start of a Page — with nothing before it — is judged entirely on what follows
/// rather than being penalised for the missing side.
fn context_score(
    haystack: &[char],
    at: usize,
    needle_len: usize,
    before: &[char],
    after: &[char],
) -> f32 {
    let before_len = before.len();
    let after_len = after.len();
    if before_len == 0 && after_len == 0 {
        return 0.0;
    }

    let matched_before = {
        let available = &haystack[..at];
        common_suffix(available, before)
    };
    let matched_after = {
        let end = at + needle_len;
        let available = if end <= haystack.len() {
            &haystack[end..]
        } else {
            &[][..]
        };
        common_prefix(available, after)
    };

    (matched_before + matched_after) as f32 / (before_len + after_len) as f32
}

fn common_prefix(a: &[char], b: &[char]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn common_suffix(a: &[char], b: &[char]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

fn slice(chars: &[char], from: i64, to: i64) -> String {
    let start = from.max(0) as usize;
    let end = (to.max(0) as usize).min(chars.len());
    if start >= end {
        String::new()
    } else {
        chars[start..end].iter().collect()
    }
}

/// FNV-1a. Not cryptographic and not meant to be — it exists to answer "is this
/// the same string?" cheaply, and a dependency would be overkill for that.
pub fn hash(value: &str) -> String {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{state:016x}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub id: AnnotationId,
    pub page_id: PageId,
    pub kind: AnnotationKind,
    pub status: AnnotationStatus,
    pub body: String,
    pub target: AnnotationTarget,
    /// Set when the annotation was produced by an AI profile, for attribution.
    pub author_profile: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Annotation {
    pub fn create(
        page_id: PageId,
        kind: AnnotationKind,
        body: &str,
        target: AnnotationTarget,
    ) -> Self {
        let at = now();
        Self {
            id: AnnotationId::new(),
            page_id,
            kind,
            status: AnnotationStatus::Active,
            body: body.trim().to_string(),
            target,
            author_profile: None,
            created_at: at,
            updated_at: at,
        }
    }

    pub fn anchor(&self) -> Option<&AnnotationAnchor> {
        match &self.target {
            AnnotationTarget::Range(anchor) => Some(anchor),
            AnnotationTarget::Page => None,
        }
    }

    /// Re-places this annotation against the Page's current text.
    ///
    /// Returns true when something changed and the record needs writing back.
    /// A whole-Page annotation is never touched; a resolved one is left alone
    /// so reviewing history does not resurrect settled comments.
    pub fn reanchor(&mut self, text: &str, revision: i64) -> bool {
        if self.status == AnnotationStatus::Resolved {
            return false;
        }
        let Some(anchor) = self.anchor().cloned() else {
            return false;
        };

        match relocate(&anchor, text) {
            Relocation::Unchanged => {
                let refreshed = anchor.base_revision != revision;
                let revived = self.status == AnnotationStatus::Stale;
                if refreshed || revived {
                    let mut updated = anchor;
                    updated.base_revision = revision;
                    self.target = AnnotationTarget::Range(updated);
                    self.status = AnnotationStatus::Active;
                    self.updated_at = now();
                    return true;
                }
                false
            }
            Relocation::Moved { from, to, .. } => {
                // Re-capture so the context travels with the new position;
                // otherwise the anchor drifts further on every subsequent edit.
                let recaptured = AnnotationAnchor::capture(text, from, to, revision);
                self.target = AnnotationTarget::Range(recaptured);
                self.status = AnnotationStatus::Active;
                self.updated_at = now();
                true
            }
            Relocation::Lost => {
                if self.status == AnnotationStatus::Stale {
                    return false;
                }
                self.status = AnnotationStatus::Stale;
                self.updated_at = now();
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "The road had been salt once, or so the carters said, and the wind still carried the taste of it in dry months.";

    fn anchor_on(text: &str, phrase: &str) -> AnnotationAnchor {
        let from = text.chars().collect::<Vec<_>>();
        let start = text
            .find(phrase)
            .map(|byte| text[..byte].chars().count())
            .expect("phrase");
        let _ = from;
        AnnotationAnchor::capture(
            text,
            start as i64,
            (start + phrase.chars().count()) as i64,
            1,
        )
    }

    #[test]
    fn capture_records_the_selection_and_its_surroundings() {
        let anchor = anchor_on(TEXT, "carters");
        assert_eq!(anchor.selected_text, "carters");
        assert!(anchor.context_before.ends_with("or so the "));
        assert!(anchor.context_after.starts_with(" said"));
    }

    #[test]
    fn capture_clamps_a_range_that_runs_past_the_end() {
        let anchor = AnnotationAnchor::capture("short", 2, 900, 1);
        assert_eq!(anchor.selected_text, "ort");
        assert_eq!(anchor.to, 5);
    }

    #[test]
    fn capture_is_char_accurate_on_cjk() {
        let text = "手稿属于用户";
        let anchor = AnnotationAnchor::capture(text, 2, 4, 1);
        assert_eq!(anchor.selected_text, "属于");
    }

    #[test]
    fn unchanged_text_needs_no_relocation() {
        let anchor = anchor_on(TEXT, "carters");
        assert_eq!(relocate(&anchor, TEXT), Relocation::Unchanged);
    }

    #[test]
    fn an_insertion_before_the_anchor_moves_it() {
        let anchor = anchor_on(TEXT, "carters");
        let edited = format!("Once upon a time. {TEXT}");
        match relocate(&anchor, &edited) {
            Relocation::Moved {
                from,
                to,
                confidence,
            } => {
                let found: String = edited
                    .chars()
                    .skip(from as usize)
                    .take((to - from) as usize)
                    .collect();
                assert_eq!(found, "carters");
                assert!(confidence > 0.9, "confidence was {confidence}");
            }
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn deleting_the_anchored_text_loses_it_rather_than_guessing() {
        let anchor = anchor_on(TEXT, "carters");
        let edited = TEXT.replace("carters", "drovers");
        assert_eq!(relocate(&anchor, &edited), Relocation::Lost);
    }

    #[test]
    fn a_repeated_phrase_is_disambiguated_by_context() {
        let text = "She said yes. He said yes. They said yes.";
        //          Anchor the middle "yes", which is identical to two others.
        let start = text.find("He said yes").unwrap() + "He said ".len();
        let anchor = AnnotationAnchor::capture(
            text,
            text[..start].chars().count() as i64,
            text[..start + 3].chars().count() as i64,
            1,
        );
        assert_eq!(anchor.selected_text, "yes");

        // Edit the first sentence only. The middle "yes" must stay put.
        let edited = "She replied yes. He said yes. They said yes.";
        match relocate(&anchor, edited) {
            Relocation::Moved { from, to, .. } => {
                let found: String = edited
                    .chars()
                    .skip(from as usize)
                    .take((to - from) as usize)
                    .collect();
                assert_eq!(found, "yes");
                let prefix: String = edited.chars().take(from as usize).collect();
                assert!(prefix.ends_with("He said "), "landed after {prefix:?}");
            }
            Relocation::Unchanged => {}
            other => panic!("expected placement, got {other:?}"),
        }
    }

    #[test]
    fn identical_phrases_with_destroyed_context_are_refused() {
        // Every occurrence looks the same and the remembered surroundings are
        // gone. Guessing here would attach the note to the wrong sentence.
        let anchor = AnnotationAnchor::capture("alpha beta gamma delta", 6, 10, 1);
        assert_eq!(anchor.selected_text, "beta");
        let edited = "beta beta beta beta";
        assert_eq!(relocate(&anchor, edited), Relocation::Lost);
    }

    #[test]
    fn a_unique_match_survives_heavy_editing_around_it() {
        let anchor = anchor_on(TEXT, "carried the taste");
        let edited = "Completely different opening. carried the taste of nothing at all.";
        match relocate(&anchor, edited) {
            Relocation::Moved { confidence, .. } => {
                assert!(confidence >= UNIQUE_MATCH_CONFIDENCE);
            }
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_anchor_is_never_placed() {
        let anchor = AnnotationAnchor::capture(TEXT, 5, 5, 1);
        assert_eq!(relocate(&anchor, TEXT), Relocation::Lost);
    }

    #[test]
    fn relocation_never_returns_a_range_outside_the_text() {
        let anchor = anchor_on(TEXT, "months");
        let edited = format!("{TEXT} {TEXT}");
        if let Relocation::Moved { from, to, .. } = relocate(&anchor, &edited) {
            assert!(from >= 0);
            assert!(to as usize <= edited.chars().count());
            assert!(to > from);
        }
    }

    #[test]
    fn reanchoring_marks_a_lost_annotation_stale_instead_of_moving_it() {
        let anchor = anchor_on(TEXT, "carters");
        let mut annotation = Annotation::create(
            PageId::new(),
            AnnotationKind::Note,
            "Whose voice is this?",
            AnnotationTarget::Range(anchor),
        );

        let edited = TEXT.replace("carters", "drovers");
        assert!(annotation.reanchor(&edited, 2));
        assert_eq!(annotation.status, AnnotationStatus::Stale);
    }

    #[test]
    fn a_stale_annotation_revives_when_its_text_comes_back() {
        let anchor = anchor_on(TEXT, "carters");
        let mut annotation = Annotation::create(
            PageId::new(),
            AnnotationKind::Note,
            "note",
            AnnotationTarget::Range(anchor),
        );

        annotation.reanchor(&TEXT.replace("carters", "drovers"), 2);
        assert_eq!(annotation.status, AnnotationStatus::Stale);

        // Undo restores the text; the note should come back with it.
        assert!(annotation.reanchor(TEXT, 3));
        assert_eq!(annotation.status, AnnotationStatus::Active);
    }

    #[test]
    fn reanchoring_reports_no_change_when_there_is_none() {
        let anchor = anchor_on(TEXT, "carters");
        let base = anchor.base_revision;
        let mut annotation = Annotation::create(
            PageId::new(),
            AnnotationKind::Note,
            "note",
            AnnotationTarget::Range(anchor),
        );
        // Same text, same revision — nothing to write back.
        assert!(!annotation.reanchor(TEXT, base));
    }

    #[test]
    fn whole_page_annotations_never_go_stale() {
        let mut annotation = Annotation::create(
            PageId::new(),
            AnnotationKind::Note,
            "On the whole",
            AnnotationTarget::Page,
        );
        assert!(!annotation.reanchor("completely different text", 9));
        assert_eq!(annotation.status, AnnotationStatus::Active);
    }

    #[test]
    fn resolved_annotations_are_left_alone() {
        let anchor = anchor_on(TEXT, "carters");
        let mut annotation = Annotation::create(
            PageId::new(),
            AnnotationKind::Note,
            "note",
            AnnotationTarget::Range(anchor),
        );
        annotation.status = AnnotationStatus::Resolved;
        assert!(!annotation.reanchor("nothing like the original", 4));
        assert_eq!(annotation.status, AnnotationStatus::Resolved);
    }

    #[test]
    fn a_moved_anchor_recaptures_its_context() {
        // Without re-capture the anchor keeps stale surroundings and drifts
        // further on each subsequent edit.
        let anchor = anchor_on(TEXT, "carters");
        let mut annotation = Annotation::create(
            PageId::new(),
            AnnotationKind::Note,
            "note",
            AnnotationTarget::Range(anchor),
        );

        let edited = format!("New opening sentence. {TEXT}");
        assert!(annotation.reanchor(&edited, 2));

        let updated = annotation.anchor().unwrap();
        assert_eq!(updated.base_revision, 2);
        let found: String = edited
            .chars()
            .skip(updated.from as usize)
            .take((updated.to - updated.from) as usize)
            .collect();
        assert_eq!(found, "carters");
        // And the second edit still finds it.
        assert_eq!(relocate(updated, &edited), Relocation::Unchanged);
    }

    #[test]
    fn kinds_and_statuses_round_trip_through_text() {
        for kind in [
            AnnotationKind::Note,
            AnnotationKind::Question,
            AnnotationKind::Suggestion,
            AnnotationKind::Warning,
            AnnotationKind::Reference,
            AnnotationKind::AiReview,
            AnnotationKind::AiSuggestion,
        ] {
            assert_eq!(AnnotationKind::parse(kind.as_str()), Some(kind));
        }
        for status in [
            AnnotationStatus::Active,
            AnnotationStatus::Resolved,
            AnnotationStatus::Stale,
        ] {
            assert_eq!(AnnotationStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(AnnotationKind::parse("nonsense"), None);
    }

    #[test]
    fn hashing_distinguishes_different_text() {
        assert_eq!(hash("carters"), hash("carters"));
        assert_ne!(hash("carters"), hash("drovers"));
        assert_eq!(hash("").len(), 16);
    }
}
