//! Ink intelligence — handwriting recognition as derived metadata.
//!
//! Handwriting is authoritative; recognition is not. The vector strokes in
//! [`super::ink`] are the source of truth, and everything in this module is
//! *derived* from them: a transcript, a confidence, a status. If recognition
//! metadata is lost, the handwritten note still displays exactly as it was
//! written.
//!
//! # The contract this module enforces
//!
//! * **Recognition never mutates ink.** A recognizer receives a snapshot and
//!   returns metadata. There is no path from a recognizer back to the strokes.
//! * **Recognition corresponds to a specific ink state.** Every result records
//!   the [`content_hash`] of the strokes it was generated from, so a result
//!   computed against yesterday's handwriting cannot be applied to today's.
//! * **Providers are interchangeable.** A vision model, a native Windows
//!   recognizer, and a test mock all implement [`InkRecognizer`]. The domain
//!   knows nothing about HTTP, PNG, or any one vendor.
//!
//! # Layers
//!
//! Recognition is the first derived layer. An optional second layer — semantic
//! classification of a note ("this is a rewrite instruction", "this is a
//! question") — is sketched in [`InkSemanticKind`] but not built out in this
//! iteration. The hierarchy stays uncollapsed: a recognition error can never
//! reach the ink, and a semantic error can never reach the transcript.
//!
//! [`content_hash`]: ink_content_hash

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::ids::AnnotationId;
use super::ink::{InkPoint, InkStroke};
use super::manuscript::Timestamp;

// --- Status, sources, and modes --------------------------------------------

/// Where a transcript's lifecycle stands.
///
/// Recognition is a background job that can be interrupted by a crash, a page
/// switch, or the writer adding another stroke. The status records which of
/// those happened, so the UI can show the right thing and the queue can decide
/// what to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecognitionStatus {
    /// Scheduled but not yet started. The default for a note that has just been
    /// invalidated, or that was left mid-flight by an abnormal exit (see
    /// [`RecognitionStatus::Recognizing`], repaired on startup).
    Pending,
    /// A recognizer is working on it right now. Transient: only in flight while
    /// a job runs, and reset to `Pending` on startup if the app exited under it.
    Recognizing,
    /// A transcript is available.
    Recognized,
    /// The recognizer could not produce a transcript. The error is stored so the
    /// writer can see what went wrong and retry.
    Failed,
    /// The ink has changed since this result was produced, and the transcript no
    /// longer describes the handwriting on screen. A fresh recognition is due.
    Stale,
    /// Automatic recognition is turned off for this note — either by the global
    /// setting or because the note is below the recognition threshold. Manual
    /// recognition can still be requested.
    Disabled,
}

impl RecognitionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecognitionStatus::Pending => "pending",
            RecognitionStatus::Recognizing => "recognizing",
            RecognitionStatus::Recognized => "recognized",
            RecognitionStatus::Failed => "failed",
            RecognitionStatus::Stale => "stale",
            RecognitionStatus::Disabled => "disabled",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "recognizing" => RecognitionStatus::Recognizing,
            "recognized" => RecognitionStatus::Recognized,
            "failed" => RecognitionStatus::Failed,
            "stale" => RecognitionStatus::Stale,
            "disabled" => RecognitionStatus::Disabled,
            // An unrecognised status loads as the safest non-committal state
            // rather than failing the read: a row from a newer build must not
            // make the Margin unreadable.
            _ => RecognitionStatus::Pending,
        }
    }

    /// Whether a transcript is available for search and AI context, regardless
    /// of how it got there.
    pub fn has_transcript(self) -> bool {
        matches!(
            self,
            RecognitionStatus::Recognized | RecognitionStatus::Stale
        )
    }

    /// A status left in flight by a crash. Repaired to [`Pending`] on startup so
    /// the writer is never stuck behind a "Recognizing…" that can never finish.
    pub fn is_transient(self) -> bool {
        matches!(self, RecognitionStatus::Recognizing)
    }
}

/// Whether a transcript came from a recognizer or from the writer's own
/// correction.
///
/// The writer wins: once a transcript is hand-edited it is not overwritten by a
/// later automatic recognition, only by an explicit "recognize again".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptSource {
    /// Produced by a recognizer.
    Recognized,
    /// Typed or corrected by the writer. Takes priority over machine output.
    UserEdited,
}

impl TranscriptSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            TranscriptSource::Recognized => "recognized",
            TranscriptSource::UserEdited => "user-edited",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "user-edited" => TranscriptSource::UserEdited,
            _ => TranscriptSource::Recognized,
        }
    }
}

/// What kind of content a recognition is expected to transcribe.
///
/// Kept as an option on the request rather than a separate provider, so the
/// same recognizer can be asked for plain text or for LaTeX when a math-aware
/// backend exists. Math is not implemented in this iteration; the variant is
/// here so the API does not have to change later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecognitionMode {
    /// Ordinary handwriting, transcribed as plain text.
    Text,
    /// Mathematical notation, transcribed as LaTeX by a capable provider.
    Math,
    /// Let the provider decide. The default.
    Auto,
}

impl RecognitionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecognitionMode::Text => "text",
            RecognitionMode::Math => "math",
            RecognitionMode::Auto => "auto",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "text" => RecognitionMode::Text,
            "math" => RecognitionMode::Math,
            _ => RecognitionMode::Auto,
        }
    }
}

/// A hint to the recognizer about the handwriting's language.
///
/// `Auto` is the default and the common case — a writer should not have to pick
/// a language for every note. A specific tag (BCP-47, e.g. `zh-CN`, `en-US`)
/// may be supplied when the writer knows, and is passed through to providers
/// that can use it. The domain does not enumerate every tag; it only
/// distinguishes "auto" from "a specific language".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageHint {
    Auto,
    /// A specific language tag, validated only to be non-empty.
    Language(String),
}

impl LanguageHint {
    pub fn auto() -> Self {
        LanguageHint::Auto
    }

    /// Parses a stored or IPC value. `auto` (and the empty string) become
    /// [`LanguageHint::Auto`]; anything else is treated as a language tag.
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
            LanguageHint::Auto
        } else {
            LanguageHint::Language(trimmed.to_string())
        }
    }

    /// The string form stored and sent to providers: `auto` or the tag itself.
    pub fn as_str(&self) -> &str {
        match self {
            LanguageHint::Auto => "auto",
            LanguageHint::Language(tag) => tag,
        }
    }

    /// Whether a recognizer should be told a specific language.
    pub fn is_specific(&self) -> bool {
        matches!(self, LanguageHint::Language(_))
    }
}

impl Default for LanguageHint {
    fn default() -> Self {
        LanguageHint::Auto
    }
}

// --- The persisted record ---------------------------------------------------

/// One note's recognition state. At most one row per ink annotation.
///
/// Every field except `annotation_id` and `updated_at` is optional, because the
/// row exists from the moment a note is first recognised-as-candidate and
/// accumulates meaning as recognition proceeds. A note that has never been
/// recognised has no row at all; a note mid-flight has a row with
/// [`RecognitionStatus::Recognizing`] and no transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkRecognitionRecord {
    pub annotation_id: AnnotationId,
    pub status: RecognitionStatus,
    /// The transcript, whether recognised or hand-corrected. Absent until a
    /// recognizer or the writer supplies one.
    pub recognized_text: Option<String>,
    /// Recognizer-supplied, when the provider offers a real score. Never
    /// fabricated — `None` when unavailable, rather than a fake 97.4%.
    pub confidence: Option<f32>,
    pub provider: Option<String>,
    pub model: Option<String>,
    /// The language the recognizer reported, when it does.
    pub language: Option<String>,
    /// Whether the transcript was recognised or hand-corrected. Decides whether
    /// a later automatic run may overwrite it.
    pub transcript_source: TranscriptSource,
    /// The [`ink_content_hash`] of the strokes this result was generated from.
    /// When the ink changes, this no longer matches and the result is stale.
    pub content_hash: Option<String>,
    /// A short, non-private description of a failure, for the retry affordance.
    pub error: Option<String>,
    pub recognized_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

impl InkRecognitionRecord {
    /// The transcript the writer would want to see, search, and send to AI:
    /// the hand-corrected one when present, otherwise the recognised one.
    ///
    /// Stale results still carry their transcript — it is out of date, not gone
    /// — so the writer is not left looking at an empty note while a refresh is
    /// pending. Callers that must be current check `content_hash` themselves.
    pub fn transcript(&self) -> Option<&str> {
        self.recognized_text.as_deref().filter(|t| !t.is_empty())
    }

    /// Whether this record was produced against the given ink state. A record
    /// with no hash was never recognised; a record whose hash differs was
    /// recognised against different strokes.
    pub fn matches_hash(&self, strokes: &[InkStroke]) -> bool {
        match &self.content_hash {
            Some(hash) => *hash == ink_content_hash(strokes),
            None => false,
        }
    }
}

// --- Content hashing --------------------------------------------------------

/// Computes a stable hash of an ink note's strokes, so a recognition result can
/// be tied to the exact ink state it was generated from.
///
/// Only the *visual* content is hashed — tool, colour, width, and each point's
/// geometry and pressure. The stroke id, creation timestamp, and per-point
/// timing are excluded: they do not change what the handwriting looks like, and
/// including them would force a re-recognition whenever a note was reloaded with
/// fresh ids (for example, after an import) even though nothing on screen had
/// changed.
///
/// The hash is order-independent over strokes: two notes with the same strokes
/// drawn in a different order produce the same hash. Drawing order does not
/// change the recognition of the words, and order-independence keeps the cache
/// valid across the kinds of reordering an undo/redo can produce.
pub fn ink_content_hash(strokes: &[InkStroke]) -> String {
    // A canonical, sorted form: each stroke rendered to a deterministic string,
    // then sorted so the result is independent of drawing order.
    let mut rendered: Vec<String> = strokes
        .iter()
        .map(|stroke| {
            let mut buf = String::new();
            buf.push_str(stroke.tool.as_str());
            buf.push('|');
            buf.push_str(&stroke.color);
            buf.push('|');
            // Width to a fixed precision, so 2.0 and 2.000 do not differ.
            buf.push_str(&format!("{:.4}", stroke.width));
            buf.push('|');
            for point in &stroke.points {
                buf.push_str(&format_point(point));
                buf.push(';');
            }
            buf
        })
        .collect();
    rendered.sort_unstable();

    let mut canonical = String::new();
    for entry in &rendered {
        canonical.push_str(entry);
        canonical.push('\n');
    }

    fnv1a(&canonical)
}

fn format_point(point: &InkPoint) -> String {
    let pressure = match point.pressure {
        Some(p) => format!(":{:.4}", p),
        None => String::new(),
    };
    format!("{:.6},{:.6}{}", point.x, point.y, pressure)
}

/// FNV-1a, the same non-cryptographic hash the annotation module uses. It exists
/// to answer "is this the same ink?" cheaply; a dependency would be overkill.
fn fnv1a(value: &str) -> String {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{state:016x}")
}

// --- The request, the result, and the provider trait ------------------------

/// What a recognizer is asked to transcribe, in a form it can consume.
///
/// Designed to carry *either* the raw vectors *or* a rendered image, because
/// providers differ in what they accept: a future native Windows recognizer
/// takes vectors directly and never needs a raster, while a vision model needs
/// pixels. The domain does not assume PNG.
#[derive(Debug, Clone)]
pub enum InkRecognitionInput {
    /// Raw vector strokes, for a provider that accepts ink natively. Carries the
    /// surface width so the normalised `x` coordinates can be reconstructed in
    /// pixel space.
    Vectors {
        strokes: Vec<InkStroke>,
        surface_width: f32,
    },
    /// A rendered raster, for vision models. PNG bytes with the dimensions the
    /// raster was drawn at, so a provider that wants to know the aspect ratio
    /// can.
    Image {
        png: Vec<u8>,
        width: u32,
        height: u32,
    },
}

impl InkRecognitionInput {
    /// Whether the input carries any strokes worth recognising. A single
    /// accidental dot, or an empty note, is not — recognising it would waste a
    /// model call and return nothing useful.
    pub fn is_meaningful(&self) -> bool {
        match self {
            InkRecognitionInput::Vectors { strokes, .. } => strokes
                .iter()
                .any(|stroke| !stroke.is_empty() && meaningful_stroke(stroke)),
            InkRecognitionInput::Image { png, .. } => !png.is_empty(),
        }
    }
}

/// A stroke is meaningful for recognition when it is more than a single point
/// and not absurdly tiny. A tap that leaves a dot is not handwriting.
fn meaningful_stroke(stroke: &InkStroke) -> bool {
    if stroke.points.len() < 2 {
        return false;
    }
    // A stroke whose points all coincide is a dot drawn slowly, not a letter.
    let first = stroke.points[0];
    stroke
        .points
        .iter()
        .any(|p| (p.x - first.x).abs() > 0.002 || (p.y - first.y).abs() > 0.5)
}

/// A recognition request, already assembled and budgeted.
#[derive(Debug, Clone)]
pub struct InkRecognitionRequest {
    pub input: InkRecognitionInput,
    pub mode: RecognitionMode,
    pub language_hint: LanguageHint,
}

/// A normalized recognition result, with all provider-specific shape removed.
///
/// Whatever the provider returned — a chat completion, an OCR JSON envelope, a
/// native recognizer struct — it is reduced to these fields before it touches
/// the domain. Raw provider JSON never reaches the UI.
#[derive(Debug, Clone)]
pub struct InkRecognitionResult {
    /// The transcribed text, in the language it was written in.
    pub text: String,
    /// A real confidence score when the provider offers one; `None` otherwise.
    pub confidence: Option<f32>,
    /// The language the recognizer detected or was told, when known.
    pub language: Option<String>,
    /// Which provider produced this, for attribution and for staleness when the
    /// provider changes.
    pub provider: String,
    pub model: String,
}

/// A source of handwriting transcripts.
///
/// The trait mirrors [`crate::ai::AiProvider`]: it returns a boxed future so it
/// can be held as `dyn InkRecognizer`, and it takes a cancellation token so a
/// recognition that a newer stroke has superseded can be abandoned.
pub trait InkRecognizer: Send + Sync {
    /// Identifies the recognizer in logs and in the stored `provider` field.
    fn name(&self) -> &'static str;

    /// Transcribes the request. Cancellation is cooperative: the token lets a
    /// job be abandoned before it completes, and a stale result is the caller's
    /// to discard.
    fn recognize<'a>(
        &'a self,
        request: InkRecognitionRequest,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<InkRecognitionResult>> + Send + 'a>>;
}

// --- Semantic classification (designed, not built out) ---------------------

/// The kind of marginal annotation a handwritten note is, beyond its text.
///
/// This is the second derived layer: recognition turns strokes into text, and
/// classification turns text into intent. It is sketched here so the
/// architecture has a place for it, but no provider produces it yet and no
/// storage holds it. Plain text recognition is the mandatory MVP; semantic
/// classification is optional and must not block it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum InkSemanticKind {
    Comment,
    Question,
    Instruction,
    Warning,
    RewriteRequest,
    ArrowReference,
    CircleHighlight,
    Unknown,
}

impl InkSemanticKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            InkSemanticKind::Comment => "comment",
            InkSemanticKind::Question => "question",
            InkSemanticKind::Instruction => "instruction",
            InkSemanticKind::Warning => "warning",
            InkSemanticKind::RewriteRequest => "rewrite-request",
            InkSemanticKind::ArrowReference => "arrow-reference",
            InkSemanticKind::CircleHighlight => "circle-highlight",
            InkSemanticKind::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ink::{InkPoint, InkStroke, InkTool};

    fn stroke(tool: InkTool, width: f32, points: &[(f32, f32)]) -> InkStroke {
        InkStroke::new(
            tool,
            "ink-primary",
            width,
            points.iter().map(|(x, y)| InkPoint::new(*x, *y)).collect(),
        )
    }

    #[test]
    fn statuses_round_trip_through_text() {
        for status in [
            RecognitionStatus::Pending,
            RecognitionStatus::Recognizing,
            RecognitionStatus::Recognized,
            RecognitionStatus::Failed,
            RecognitionStatus::Stale,
            RecognitionStatus::Disabled,
        ] {
            assert_eq!(RecognitionStatus::parse(status.as_str()), status);
        }
        // An unknown status loads as pending, not as an error.
        assert_eq!(
            RecognitionStatus::parse("from-the-future"),
            RecognitionStatus::Pending
        );
    }

    #[test]
    fn transcript_sources_round_trip_and_default_to_recognized() {
        assert_eq!(
            TranscriptSource::parse(TranscriptSource::UserEdited.as_str()),
            TranscriptSource::UserEdited
        );
        assert_eq!(
            TranscriptSource::parse(TranscriptSource::Recognized.as_str()),
            TranscriptSource::Recognized
        );
        assert_eq!(
            TranscriptSource::parse("nonsense"),
            TranscriptSource::Recognized
        );
    }

    #[test]
    fn language_hint_distinguishes_auto_from_a_specific_tag() {
        assert_eq!(LanguageHint::parse("auto"), LanguageHint::Auto);
        assert_eq!(LanguageHint::parse(""), LanguageHint::Auto);
        assert_eq!(LanguageHint::parse("AUTO"), LanguageHint::Auto);
        assert_eq!(
            LanguageHint::parse("zh-CN"),
            LanguageHint::Language("zh-CN".into())
        );
        assert!(LanguageHint::Auto.is_specific() == false);
        assert!(LanguageHint::Language("en-US".into()).is_specific());
        assert_eq!(LanguageHint::Auto.as_str(), "auto");
        assert_eq!(LanguageHint::Language("en-US".into()).as_str(), "en-US");
    }

    #[test]
    fn the_content_hash_is_stable_for_the_same_ink() {
        let strokes = vec![stroke(InkTool::Pen, 2.0, &[(0.1, 5.0), (0.2, 6.0)])];
        assert_eq!(ink_content_hash(&strokes), ink_content_hash(&strokes));
    }

    #[test]
    fn adding_a_stroke_changes_the_hash() {
        let one = vec![stroke(InkTool::Pen, 2.0, &[(0.1, 5.0), (0.2, 6.0)])];
        let two = vec![
            stroke(InkTool::Pen, 2.0, &[(0.1, 5.0), (0.2, 6.0)]),
            stroke(InkTool::Pen, 2.0, &[(0.5, 20.0), (0.6, 21.0)]),
        ];
        assert_ne!(ink_content_hash(&one), ink_content_hash(&two));
    }

    #[test]
    fn erasing_a_stroke_changes_the_hash() {
        let s1 = stroke(InkTool::Pen, 2.0, &[(0.0, 0.0), (0.1, 0.0)]);
        let s2 = stroke(InkTool::Pen, 2.0, &[(0.0, 1.0), (0.1, 1.0)]);
        let both = vec![s1.clone(), s2.clone()];
        let one = vec![s1];
        assert_ne!(ink_content_hash(&both), ink_content_hash(&one));
    }

    #[test]
    fn the_hash_is_independent_of_drawing_order() {
        let s1 = stroke(InkTool::Pen, 2.0, &[(0.0, 0.0), (0.1, 0.0)]);
        let s2 = stroke(InkTool::Pen, 2.0, &[(0.0, 1.0), (0.1, 1.0)]);
        let first = vec![s1.clone(), s2.clone()];
        let second = vec![s2, s1];
        assert_eq!(ink_content_hash(&first), ink_content_hash(&second));
    }

    #[test]
    fn the_hash_ignores_ids_and_timestamps() {
        // Two strokes with identical geometry but fresh ids hash the same.
        let a = stroke(InkTool::Pen, 2.0, &[(0.1, 5.0), (0.2, 6.0)]);
        let b = stroke(InkTool::Pen, 2.0, &[(0.1, 5.0), (0.2, 6.0)]);
        assert_ne!(a.id, b.id);
        assert_eq!(ink_content_hash(&[a]), ink_content_hash(&[b]));
    }

    #[test]
    fn pressure_participates_in_the_hash() {
        let plain = InkStroke::new(
            InkTool::Pen,
            "ink-primary",
            2.0,
            vec![InkPoint::new(0.1, 5.0), InkPoint::new(0.2, 6.0)],
        );
        let pressed = InkStroke::new(
            InkTool::Pen,
            "ink-primary",
            2.0,
            vec![
                InkPoint {
                    x: 0.1,
                    y: 5.0,
                    pressure: Some(0.8),
                    timestamp: None,
                },
                InkPoint {
                    x: 0.2,
                    y: 6.0,
                    pressure: Some(0.8),
                    timestamp: None,
                },
            ],
        );
        assert_ne!(ink_content_hash(&[plain]), ink_content_hash(&[pressed]));
    }

    #[test]
    fn a_record_matches_its_own_hash_and_no_other() {
        let strokes = vec![stroke(InkTool::Pen, 2.0, &[(0.1, 5.0), (0.2, 6.0)])];
        let record = InkRecognitionRecord {
            annotation_id: AnnotationId::new(),
            status: RecognitionStatus::Recognized,
            recognized_text: Some("hello".into()),
            confidence: None,
            provider: Some("mock".into()),
            model: Some("mock-1".into()),
            language: None,
            transcript_source: TranscriptSource::Recognized,
            content_hash: Some(ink_content_hash(&strokes)),
            error: None,
            recognized_at: None,
            updated_at: crate::domain::manuscript::now(),
        };
        assert!(record.matches_hash(&strokes));

        let different = vec![stroke(InkTool::Pen, 2.0, &[(0.5, 50.0), (0.6, 51.0)])];
        assert!(!record.matches_hash(&different));
    }

    #[test]
    fn a_record_with_no_hash_matches_nothing() {
        let record = InkRecognitionRecord {
            annotation_id: AnnotationId::new(),
            status: RecognitionStatus::Pending,
            recognized_text: None,
            confidence: None,
            provider: None,
            model: None,
            language: None,
            transcript_source: TranscriptSource::Recognized,
            content_hash: None,
            error: None,
            recognized_at: None,
            updated_at: crate::domain::manuscript::now(),
        };
        assert!(!record.matches_hash(&[stroke(InkTool::Pen, 2.0, &[(0.1, 5.0)])]));
    }

    #[test]
    fn a_single_dot_is_not_meaningful_for_recognition() {
        let dot = InkRecognitionInput::Vectors {
            strokes: vec![stroke(InkTool::Pen, 2.0, &[(0.5, 5.0)])],
            surface_width: 300.0,
        };
        assert!(!dot.is_meaningful());
    }

    #[test]
    fn a_real_stroke_is_meaningful() {
        let writing = InkRecognitionInput::Vectors {
            strokes: vec![stroke(
                InkTool::Pen,
                2.0,
                &[(0.1, 5.0), (0.2, 6.0), (0.3, 5.0)],
            )],
            surface_width: 300.0,
        };
        assert!(writing.is_meaningful());
    }

    #[test]
    fn an_empty_note_is_not_meaningful() {
        let empty = InkRecognitionInput::Vectors {
            strokes: vec![],
            surface_width: 300.0,
        };
        assert!(!empty.is_meaningful());
    }

    #[test]
    fn only_recognized_and_stale_carry_a_transcript_for_search() {
        assert!(RecognitionStatus::Recognized.has_transcript());
        assert!(RecognitionStatus::Stale.has_transcript());
        assert!(!RecognitionStatus::Pending.has_transcript());
        assert!(!RecognitionStatus::Failed.has_transcript());
        assert!(!RecognitionStatus::Disabled.has_transcript());
    }

    #[test]
    fn recognizing_is_the_only_transient_status() {
        assert!(RecognitionStatus::Recognizing.is_transient());
        for status in [
            RecognitionStatus::Pending,
            RecognitionStatus::Recognized,
            RecognitionStatus::Failed,
            RecognitionStatus::Stale,
            RecognitionStatus::Disabled,
        ] {
            assert!(!status.is_transient());
        }
    }
}
