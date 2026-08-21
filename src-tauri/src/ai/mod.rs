//! The AI layer.
//!
//! Everything a model touches passes through here, and the boundary is drawn
//! deliberately:
//!
//! * **The key never leaves Rust.** It is read from the OS credential store at
//!   the moment a request is built and dropped when it completes. The WebView
//!   never sees it, and neither does the database.
//! * **Nothing is sent without being asked for.** There is no background
//!   analysis, no telemetry, no warming request. A model sees manuscript text
//!   only when the writer invokes an action that needs it.
//! * **Only the context the action requires travels.** See [`context`], which
//!   budgets what goes out and reports what it left behind.
//! * **The model never edits the manuscript.** Output becomes a Margin
//!   annotation, and a proposed edit becomes a suggestion the writer applies —
//!   after Grimoire re-checks that the text is still what the model saw.

pub mod apply;
pub mod context;
pub mod openai;
pub mod prompts;

#[cfg(test)]
mod transport_tests;

use std::future::Future;
use std::pin::Pin;

use futures_util::Stream;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::Result;

/// One message in a conversation with a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: AiRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiRole {
    System,
    User,
    Assistant,
}

impl AiRole {
    fn as_str(&self) -> &'static str {
        match self {
            AiRole::System => "system",
            AiRole::User => "user",
            AiRole::Assistant => "assistant",
        }
    }
}

/// A request as the provider layer sees it — already assembled, already
/// budgeted, with the credential resolved.
#[derive(Debug, Clone)]
pub struct AiRequest {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub messages: Vec<AiMessage>,
    pub temperature: f32,
    pub max_output_tokens: Option<i64>,
}

impl AiRequest {
    /// Renders the request for a log line, with the key and the manuscript
    /// removed. Used when a request fails and the failure needs describing.
    pub fn describe(&self) -> String {
        format!(
            "model={} messages={} chars={}",
            self.model,
            self.messages.len(),
            self.messages.iter().map(|m| m.content.len()).sum::<usize>()
        )
    }
}

/// A piece of a streaming response.
#[derive(Debug, Clone, PartialEq)]
pub enum AiChunk {
    /// Text to append to what has arrived so far.
    Delta(String),
    /// The model finished normally.
    Done,
}

pub type AiStream = Pin<Box<dyn Stream<Item = Result<AiChunk>> + Send>>;

/// A source of model output.
///
/// The trait exists so a second provider can be added without touching the
/// editor, the Margin, or the suggestion logic. It returns a boxed future
/// rather than using `async fn`, because it has to be usable as
/// `dyn AiProvider` — and that is worth more here than avoiding the noise.
pub trait AiProvider: Send + Sync {
    /// Identifies the provider in logs and errors.
    fn name(&self) -> &'static str;

    /// Starts a streaming completion.
    ///
    /// Cancellation is cooperative: dropping the stream stops reading, and the
    /// token lets a request be abandoned before the first byte arrives.
    fn stream<'a>(
        &'a self,
        request: AiRequest,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<AiStream>> + Send + 'a>>;
}

/// What an AI action was asked to do.
///
/// A closed set rather than free-form text: each maps to a reviewed prompt in
/// [`prompts`], so a request cannot be assembled from a string that arrived
/// over IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiAction {
    Review,
    Critique,
    Tighten,
    Continuity,
    Questions,
    Alternatives,
}

impl AiAction {
    pub const ALL: [AiAction; 6] = [
        AiAction::Review,
        AiAction::Critique,
        AiAction::Tighten,
        AiAction::Continuity,
        AiAction::Questions,
        AiAction::Alternatives,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            AiAction::Review => "review",
            AiAction::Critique => "critique",
            AiAction::Tighten => "tighten",
            AiAction::Continuity => "continuity",
            AiAction::Questions => "questions",
            AiAction::Alternatives => "alternatives",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.as_str() == raw)
    }

    /// What the resulting Margin note is labelled as.
    pub fn annotation_kind(&self) -> crate::domain::annotation::AnnotationKind {
        use crate::domain::annotation::AnnotationKind;
        match self {
            // These propose specific replacement text, so they become
            // suggestions the writer can apply.
            AiAction::Tighten | AiAction::Alternatives => AnnotationKind::AiSuggestion,
            _ => AnnotationKind::AiReview,
        }
    }

    /// Whether this action is expected to return replacement text.
    pub fn proposes_an_edit(&self) -> bool {
        matches!(self, AiAction::Tighten | AiAction::Alternatives)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::annotation::AnnotationKind;

    #[test]
    fn actions_round_trip_through_text() {
        for action in AiAction::ALL {
            assert_eq!(AiAction::parse(action.as_str()), Some(action));
        }
        assert_eq!(AiAction::parse("delete everything"), None);
    }

    #[test]
    fn only_editing_actions_produce_suggestions() {
        assert!(AiAction::Tighten.proposes_an_edit());
        assert!(AiAction::Alternatives.proposes_an_edit());
        assert!(!AiAction::Review.proposes_an_edit());
        assert!(!AiAction::Questions.proposes_an_edit());

        assert_eq!(
            AiAction::Tighten.annotation_kind(),
            AnnotationKind::AiSuggestion
        );
        assert_eq!(AiAction::Review.annotation_kind(), AnnotationKind::AiReview);
    }

    #[test]
    fn a_request_description_carries_no_key_and_no_manuscript() {
        let request = AiRequest {
            base_url: "https://example.test/v1".into(),
            model: "a-model".into(),
            api_key: "sk-secret-value".into(),
            messages: vec![AiMessage {
                role: AiRole::User,
                content: "The road had been salt once.".into(),
            }],
            temperature: 0.7,
            max_output_tokens: None,
        };

        let described = request.describe();
        assert!(!described.contains("sk-secret-value"));
        assert!(!described.contains("salt"));
        assert!(described.contains("a-model"));
    }
}
