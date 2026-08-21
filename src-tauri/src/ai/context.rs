//! Context assembly and budgeting.
//!
//! This decides how much of a manuscript leaves the machine, and it is written
//! to send as little as the action needs rather than as much as the model would
//! accept.
//!
//! Three policies:
//!
//! * `selection` — the selected passage, with a little text either side so the
//!   model can hear the rhythm it sits in.
//! * `page`      — the whole Page the selection is on.
//! * `chapter`   — the Page plus its neighbours, for continuity checking.
//!
//! Whatever the policy, the result is trimmed to a character budget and reports
//! what it left out, so the writer is told when a request was narrowed rather
//! than finding out from a strange answer.

use serde::{Deserialize, Serialize};

use crate::domain::text::preview;

/// How much text surrounds a selection under the `selection` policy.
const SURROUND_CHARS: usize = 700;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextPolicy {
    Selection,
    Page,
    Chapter,
}

impl ContextPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextPolicy::Selection => "selection",
            ContextPolicy::Page => "page",
            ContextPolicy::Chapter => "chapter",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "page" => ContextPolicy::Page,
            "chapter" => ContextPolicy::Chapter,
            // Anything unrecognised falls to the *narrowest* policy. If a
            // stored value cannot be read, the safe failure is sending less.
            _ => ContextPolicy::Selection,
        }
    }
}

/// What the request will carry, and what it will not.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltContext {
    /// The passage the writer selected. Always present, never truncated away.
    pub selection: String,
    /// Manuscript around the selection, already budgeted.
    pub surrounding: String,
    /// Volume, Chapter, and Page titles, for orientation.
    pub where_from: String,
    /// Total characters of manuscript the request will send.
    pub chars_sent: usize,
    /// True when the budget forced text to be left behind.
    pub trimmed: bool,
    /// A sentence the UI shows before anything is sent.
    pub summary: String,
    /// Recognised handwritten margin notes on the Page, included verbatim as
    /// `[Handwritten note]` blocks. Short, and the writer's own marginalia, so
    /// they are not subject to the surrounding-text budget.
    #[serde(default)]
    pub ink_notes: Vec<InkNoteContextOwned>,
}

/// An owned form of [`InkNoteContext`], for the serialisable [`BuiltContext`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InkNoteContextOwned {
    pub transcript: String,
    pub anchored_text: String,
}

/// The pieces a context is assembled from.
pub struct ContextSources<'a> {
    pub volume_title: &'a str,
    pub chapter_title: &'a str,
    pub page_title: &'a str,
    pub page_text: &'a str,
    /// Selection offsets into `page_text`, in characters.
    pub from: usize,
    pub to: usize,
    /// Neighbouring Pages in the Chapter, in manuscript order. Only used by the
    /// `chapter` policy.
    pub neighbours: &'a [(String, String)],
    /// Recognised handwritten margin notes on this Page. Each carries its
    /// transcript and, when the note is anchored, the prose it points at — so a
    /// model can hear the writer's own marginalia alongside the manuscript
    /// without ever receiving raw stroke coordinates. The transcript is the
    /// writer-corrected one when present.
    pub ink_notes: &'a [InkNoteContextOwned],
}

/// Takes characters from the end of a string.
fn tail(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

/// Takes characters from the start of a string.
fn head(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// Assembles the context for a request.
///
/// The selection is never sacrificed to the budget — a request that dropped the
/// passage being asked about would be worse than one that failed. Everything
/// else is trimmed to fit around it.
pub fn build(
    sources: ContextSources<'_>,
    policy: ContextPolicy,
    budget_chars: i64,
) -> BuiltContext {
    let budget = budget_chars.max(200) as usize;

    let chars: Vec<char> = sources.page_text.chars().collect();
    let from = sources.from.min(chars.len());
    let to = sources.to.clamp(from, chars.len());
    let selection: String = chars[from..to].iter().collect();

    let where_from = format!(
        "{} › {} › {}",
        sources.volume_title, sources.chapter_title, sources.page_title
    );

    // What is left for surrounding text once the selection and the orientation
    // line are accounted for.
    let reserved = selection.chars().count() + where_from.chars().count();
    let remaining = budget.saturating_sub(reserved);

    let (surrounding, trimmed) = match policy {
        ContextPolicy::Selection => {
            let want = SURROUND_CHARS.min(remaining / 2);
            let before: String = tail(&chars[..from].iter().collect::<String>(), want);
            let after: String = head(&chars[to..].iter().collect::<String>(), want);
            let trimmed = before.chars().count() < from
                || after.chars().count() < chars.len().saturating_sub(to);
            (format!("{before}[…]{after}"), trimmed)
        }

        ContextPolicy::Page => {
            let whole = sources.page_text;
            if whole.chars().count() <= remaining {
                (whole.to_string(), false)
            } else {
                (head(whole, remaining), true)
            }
        }

        ContextPolicy::Chapter => {
            let mut assembled = String::new();
            let mut trimmed = false;
            let mut left = remaining;

            // The Page itself comes first and gets the larger share; a
            // continuity check that dropped the passage's own Page to fit its
            // neighbours would be answering the wrong question.
            let own = head(sources.page_text, left);
            trimmed |= own.chars().count() < sources.page_text.chars().count();
            left = left.saturating_sub(own.chars().count());
            assembled.push_str(&own);

            for (title, text) in sources.neighbours {
                if left < 200 {
                    trimmed = true;
                    break;
                }
                let heading = format!("\n\n--- {title} ---\n");
                let share = left.saturating_sub(heading.chars().count());
                let body = head(text, share.min(left / 2).max(200));
                trimmed |= body.chars().count() < text.chars().count();
                left = left.saturating_sub(body.chars().count() + heading.chars().count());
                assembled.push_str(&heading);
                assembled.push_str(&body);
            }

            (assembled, trimmed)
        }
    };

    let chars_sent = selection.chars().count() + surrounding.chars().count();

    let summary = if selection.is_empty() {
        format!("Sends this Page ({chars_sent} characters) to your AI provider.")
    } else {
        let shown = preview(&selection, 40);
        match policy {
            ContextPolicy::Selection => format!(
                "Sends “{shown}” and the text around it — {chars_sent} characters in total."
            ),
            ContextPolicy::Page => {
                format!("Sends “{shown}” and the whole Page — {chars_sent} characters in total.")
            }
            ContextPolicy::Chapter => format!(
                "Sends “{shown}”, this Page, and its neighbours — {chars_sent} characters \
                 in total."
            ),
        }
    };

    let summary = if trimmed {
        format!("{summary} Some text was left out to stay within the context budget.")
    } else {
        summary
    };

    let ink_notes = sources.ink_notes.to_vec();

    BuiltContext {
        selection,
        surrounding,
        where_from,
        chars_sent,
        trimmed,
        summary,
        ink_notes,
    }
}

/// Renders the assembled context as the user message for a request.
pub fn render(context: &BuiltContext) -> String {
    let mut message = String::new();
    message.push_str("From: ");
    message.push_str(&context.where_from);
    message.push_str("\n\nSurrounding text:\n");
    message.push_str(&context.surrounding);
    if !context.selection.is_empty() {
        message.push_str("\n\nThe selected passage:\n");
        message.push_str(&context.selection);
    }
    if !context.ink_notes.is_empty() {
        message.push_str("\n\nHandwritten margin notes on this Page:");
        for note in &context.ink_notes {
            message.push_str("\n[Handwritten note");
            if !note.anchored_text.is_empty() {
                message.push_str(" anchored to: ");
                message.push_str(&note.anchored_text);
            }
            message.push_str("]\n");
            message.push_str(&note.transcript);
        }
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "Alpha beta gamma. The road had been salt once, or so the carters said. \
                        Delta epsilon zeta eta theta.";

    fn sources<'a>(
        from: usize,
        to: usize,
        neighbours: &'a [(String, String)],
    ) -> ContextSources<'a> {
        ContextSources {
            volume_title: "The Salt Road",
            chapter_title: "Chapter I",
            page_title: "The Crows",
            page_text: PAGE,
            from,
            to,
            neighbours,
            ink_notes: &[],
        }
    }

    fn offsets(phrase: &str) -> (usize, usize) {
        let start = PAGE
            .find(phrase)
            .map(|b| PAGE[..b].chars().count())
            .unwrap();
        (start, start + phrase.chars().count())
    }

    #[test]
    fn the_selection_is_carried_exactly() {
        let (from, to) = offsets("the carters said");
        let built = build(sources(from, to, &[]), ContextPolicy::Selection, 8000);
        assert_eq!(built.selection, "the carters said");
    }

    #[test]
    fn selection_policy_carries_text_from_both_sides() {
        let (from, to) = offsets("the carters said");
        let built = build(sources(from, to, &[]), ContextPolicy::Selection, 8000);
        assert!(built.surrounding.contains("Alpha beta"));
        assert!(built.surrounding.contains("Delta epsilon"));
        assert!(built.surrounding.contains("[…]"));
    }

    #[test]
    fn page_policy_carries_the_whole_page() {
        let (from, to) = offsets("the carters said");
        let built = build(sources(from, to, &[]), ContextPolicy::Page, 8000);
        assert_eq!(built.surrounding, PAGE);
        assert!(!built.trimmed);
    }

    #[test]
    fn chapter_policy_carries_neighbouring_pages() {
        let neighbours = vec![(
            "The Ledger".to_string(),
            "Names, mostly, and a weight beside each.".to_string(),
        )];
        let (from, to) = offsets("the carters said");
        let built = build(sources(from, to, &neighbours), ContextPolicy::Chapter, 8000);
        assert!(built.surrounding.contains("The Ledger"));
        assert!(built.surrounding.contains("Names, mostly"));
    }

    #[test]
    fn a_tight_budget_never_sacrifices_the_selection() {
        // Asking about a passage and then not sending it would be worse than
        // failing outright.
        let (from, to) = offsets("the carters said");
        let built = build(sources(from, to, &[]), ContextPolicy::Selection, 200);
        assert_eq!(built.selection, "the carters said");
    }

    #[test]
    fn exceeding_the_budget_is_reported_rather_than_hidden() {
        // A Page comfortably larger than the budget, so trimming is forced.
        let long_page = "Words enough to overflow. ".repeat(60);
        let built = build(
            ContextSources {
                volume_title: "The Salt Road",
                chapter_title: "Chapter I",
                page_title: "The Crows",
                page_text: &long_page,
                from: 0,
                to: 5,
                neighbours: &[],
                ink_notes: &[],
            },
            ContextPolicy::Page,
            400,
        );

        assert!(
            built.trimmed,
            "a page of {} chars fitted in 400",
            long_page.len()
        );
        assert!(built.summary.contains("left out"), "{}", built.summary);
        assert!(built.chars_sent < long_page.chars().count());
    }

    #[test]
    fn the_summary_says_what_will_be_sent_before_it_is_sent() {
        let (from, to) = offsets("the carters said");
        let built = build(sources(from, to, &[]), ContextPolicy::Selection, 8000);
        assert!(built.summary.contains("carters"));
        assert!(built.summary.contains("characters"));
        assert!(!built.summary.contains("left out"));
    }

    #[test]
    fn an_unreadable_policy_falls_back_to_sending_the_least() {
        assert_eq!(ContextPolicy::parse("everything"), ContextPolicy::Selection);
        assert_eq!(ContextPolicy::parse("chapter"), ContextPolicy::Chapter);
    }

    #[test]
    fn the_rendered_message_contains_the_selection_and_where_it_came_from() {
        let (from, to) = offsets("the carters said");
        let built = build(sources(from, to, &[]), ContextPolicy::Selection, 8000);
        let rendered = render(&built);
        assert!(rendered.contains("The Salt Road › Chapter I › The Crows"));
        assert!(rendered.contains("the carters said"));
    }

    #[test]
    fn offsets_beyond_the_text_are_clamped_rather_than_panicking() {
        let built = build(sources(9_000, 9_999, &[]), ContextPolicy::Selection, 8000);
        assert!(built.selection.is_empty());
    }

    #[test]
    fn cjk_selections_are_measured_in_characters() {
        let text = "手稿属于用户。The manuscript belongs to the user.";
        let built = build(
            ContextSources {
                volume_title: "手稿",
                chapter_title: "第三章",
                page_title: "开端",
                page_text: text,
                from: 2,
                to: 4,
                neighbours: &[],
                ink_notes: &[],
            },
            ContextPolicy::Selection,
            8000,
        );
        assert_eq!(built.selection, "属于");
    }

    #[test]
    fn ink_notes_are_carried_into_context() {
        let (from, to) = offsets("the carters said");
        let ink = [
            InkNoteContextOwned {
                transcript: "这里的转折太突然了".into(),
                anchored_text: "the carters said".into(),
            },
            InkNoteContextOwned {
                transcript: "move this later".into(),
                anchored_text: String::new(),
            },
        ];
        let built = build(
            ContextSources {
                volume_title: "The Salt Road",
                chapter_title: "Chapter I",
                page_title: "The Crows",
                page_text: PAGE,
                from,
                to,
                neighbours: &[],
                ink_notes: &ink,
            },
            ContextPolicy::Selection,
            8000,
        );
        assert_eq!(built.ink_notes.len(), 2);
        assert_eq!(built.ink_notes[0].transcript, "这里的转折太突然了");
    }

    #[test]
    fn ink_notes_render_as_handwritten_note_blocks() {
        let (from, to) = offsets("the carters said");
        let ink = [InkNoteContextOwned {
            transcript: "too abrupt here".into(),
            anchored_text: "the carters said".into(),
        }];
        let built = build(
            ContextSources {
                volume_title: "The Salt Road",
                chapter_title: "Chapter I",
                page_title: "The Crows",
                page_text: PAGE,
                from,
                to,
                neighbours: &[],
                ink_notes: &ink,
            },
            ContextPolicy::Selection,
            8000,
        );
        let rendered = render(&built);
        assert!(rendered.contains("Handwritten margin notes"));
        assert!(rendered.contains("[Handwritten note anchored to: the carters said]"));
        assert!(rendered.contains("too abrupt here"));
        // Stroke coordinates never appear in context.
        assert!(!rendered.contains("points"));
    }

    #[test]
    fn an_unanchored_ink_note_renders_without_an_anchor() {
        let (from, to) = offsets("the carters said");
        let ink = [InkNoteContextOwned {
            transcript: "whole page note".into(),
            anchored_text: String::new(),
        }];
        let built = build(
            ContextSources {
                volume_title: "The Salt Road",
                chapter_title: "Chapter I",
                page_title: "The Crows",
                page_text: PAGE,
                from,
                to,
                neighbours: &[],
                ink_notes: &ink,
            },
            ContextPolicy::Selection,
            8000,
        );
        let rendered = render(&built);
        assert!(rendered.contains("[Handwritten note]\nwhole page note"));
    }
}
