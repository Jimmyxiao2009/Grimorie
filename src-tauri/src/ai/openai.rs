//! An OpenAI-compatible chat-completions provider.
//!
//! "Compatible" is the point: the same wire format is spoken by OpenAI, by
//! self-hosted servers, and by most gateways, so one implementation covers the
//! cases a writer is likely to have. The base URL is configurable precisely so
//! that nothing here assumes a particular company's host.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, ErrorCode, Result};

use super::{AiChunk, AiContent, AiContentPart, AiProvider, AiRequest, AiStream};

/// How long to wait for the first byte. Generous, because a large context and a
/// cold model can take a while, but not unbounded.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct OpenAiCompatible {
    client: reqwest::Client,
}

impl OpenAiCompatible {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            // No overall timeout: a long generation is not a failure, and
            // cancellation is the writer's to decide.
            .user_agent(concat!("Grimoire/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|err| {
                AppError::new(
                    ErrorCode::Network,
                    "Grimoire couldn't prepare a network client.",
                )
                .with_detail(err.to_string())
            })?;
        Ok(Self { client })
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<i64>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: ChatContent<'a>,
}

/// The wire form of [`AiContent`]: a bare string for text, or an array of typed
/// parts for multimodal. Serialised with `#[serde(untagged)]` so a text message
/// goes out as `"content": "..."` — the shape every server expects — and a
/// multimodal one as `"content": [{"type":"text",...},{"type":"image_url",...}]`.
#[derive(Serialize)]
#[serde(untagged)]
enum ChatContent<'a> {
    Text(&'a str),
    Parts(&'a [AiContentPart]),
}

/// Joins a base URL to the completions path, tolerating either form of base.
///
/// People paste both "https://host/v1" and "https://host/v1/" and some paste
/// the full endpoint. Guessing wrong produces a 404 that looks like a server
/// problem rather than a typo, so all three are accepted.
fn completions_url(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

/// Extracts the text delta from one streamed chunk.
///
/// Tolerant on purpose: compatible servers differ in which optional fields they
/// send, and a missing `content` on a role-only first chunk is normal rather
/// than an error.
fn delta_of(value: &Value) -> Option<String> {
    let choice = value.get("choices")?.as_array()?.first()?;
    let delta = choice.get("delta").or_else(|| choice.get("message"))?;
    let content = delta.get("content")?;
    match content {
        Value::String(text) => Some(text.clone()),
        // Some servers send content as an array of parts.
        Value::Array(parts) => {
            let joined: String = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect();
            (!joined.is_empty()).then_some(joined)
        }
        _ => None,
    }
}

/// Turns a failed HTTP response into a message a writer can act on.
async fn describe_failure(status: reqwest::StatusCode, body: String) -> AppError {
    // Providers put their explanation in different places; take whichever is
    // present rather than showing raw JSON.
    let detail = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|e| e.get("message"))
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(300).collect());

    let message = match status.as_u16() {
        401 | 403 => {
            "Your AI provider rejected the API key. Check it in Settings — your manuscript \
             has not been changed."
        }
        404 => "Your AI provider didn't recognise that model or address. Check them in Settings.",
        429 => "Your AI provider is rate-limiting requests. Try again in a moment.",
        500..=599 => "Your AI provider had a problem at their end. Try again in a moment.",
        _ => "Your AI provider refused the request.",
    };

    AppError::new(ErrorCode::Provider, message).with_detail(format!("{status}: {detail}"))
}

impl AiProvider for OpenAiCompatible {
    fn name(&self) -> &'static str {
        "openai-compatible"
    }

    fn stream<'a>(
        &'a self,
        request: AiRequest,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<AiStream>> + Send + 'a>> {
        Box::pin(async move {
            let body = ChatRequest {
                model: &request.model,
                messages: request
                    .messages
                    .iter()
                    .map(|message| ChatMessage {
                        role: message.role.as_str(),
                        content: match &message.content {
                            AiContent::Text(text) => ChatContent::Text(text),
                            AiContent::Parts(parts) => ChatContent::Parts(parts),
                        },
                    })
                    .collect(),
                temperature: request.temperature,
                stream: true,
                max_tokens: request.max_output_tokens,
            };

            let send = self
                .client
                .post(completions_url(&request.base_url))
                .bearer_auth(&request.api_key)
                .json(&body)
                .send();

            let response = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(cancelled()),
                result = send => result.map_err(|err| {
                    // The key is in the request but must never be in the log.
                    tracing::warn!(
                        provider = "openai-compatible",
                        request = %request.describe(),
                        "AI request failed to send"
                    );
                    AppError::new(
                        ErrorCode::Network,
                        "Grimoire couldn't reach your AI provider. Check your connection \
                         and the address in Settings.",
                    )
                    .with_detail(err.without_url().to_string())
                })?,
            };

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(describe_failure(status, text).await);
            }

            Ok(Box::pin(sse_stream(response, cancel)) as AiStream)
        })
    }
}

fn cancelled() -> AppError {
    AppError::new(ErrorCode::Cancelled, "The AI request was stopped.")
}

/// Parses a server-sent-event body into chunks.
///
/// The framing is line-based over a byte stream that can split anywhere, so
/// partial lines are carried across reads. Getting this wrong truncates words
/// mid-token, which looks like the model producing nonsense.
fn sse_stream(
    response: reqwest::Response,
    cancel: CancellationToken,
) -> impl futures_util::Stream<Item = Result<AiChunk>> {
    let mut bytes = response.bytes_stream();
    let mut buffer = String::new();
    let mut finished = false;

    futures_util::stream::poll_fn(move |context| {
        use std::task::Poll;

        loop {
            if finished {
                return Poll::Ready(None);
            }

            if cancel.is_cancelled() {
                finished = true;
                return Poll::Ready(Some(Err(cancelled())));
            }

            // Emit whatever complete events the buffer already holds before
            // asking the socket for more.
            if let Some(index) = buffer.find('\n') {
                let line = buffer[..index].trim_end_matches('\r').to_string();
                buffer.drain(..index + 1);

                let Some(payload) = line.strip_prefix("data:") else {
                    // Comments, event names, and blank separators.
                    continue;
                };
                let payload = payload.trim();

                if payload == "[DONE]" {
                    finished = true;
                    return Poll::Ready(Some(Ok(AiChunk::Done)));
                }
                if payload.is_empty() {
                    continue;
                }

                match serde_json::from_str::<Value>(payload) {
                    Ok(value) => {
                        if let Some(text) = delta_of(&value) {
                            if !text.is_empty() {
                                return Poll::Ready(Some(Ok(AiChunk::Delta(text))));
                            }
                        }
                        // A chunk with no content — a role marker, or a usage
                        // report at the end. Nothing to show.
                        continue;
                    }
                    Err(error) => {
                        // One malformed event should not abandon a response
                        // that is otherwise arriving fine.
                        tracing::debug!(%error, "skipping malformed AI stream event");
                        continue;
                    }
                }
            }

            match bytes.poll_next_unpin(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    finished = true;
                    // Some servers close without sending [DONE]. Reaching the
                    // end of the body is a normal finish, not a failure.
                    return Poll::Ready(Some(Ok(AiChunk::Done)));
                }
                Poll::Ready(Some(Err(error))) => {
                    finished = true;
                    return Poll::Ready(Some(Err(AppError::new(
                        ErrorCode::Network,
                        "The connection to your AI provider was interrupted.",
                    )
                    .with_detail(error.without_url().to_string()))));
                }
                Poll::Ready(Some(Ok(part))) => {
                    // Invalid UTF-8 can only be a split multi-byte character,
                    // so the lossy conversion is on a boundary the next read
                    // completes.
                    buffer.push_str(&String::from_utf8_lossy(&part));
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_completions_path_is_appended_once() {
        assert_eq!(
            completions_url("https://api.example.test/v1"),
            "https://api.example.test/v1/chat/completions"
        );
        assert_eq!(
            completions_url("https://api.example.test/v1/"),
            "https://api.example.test/v1/chat/completions"
        );
        // Someone pasting the whole endpoint gets what they meant.
        assert_eq!(
            completions_url("https://api.example.test/v1/chat/completions"),
            "https://api.example.test/v1/chat/completions"
        );
        assert_eq!(
            completions_url("  http://localhost:1234/v1  "),
            "http://localhost:1234/v1/chat/completions"
        );
    }

    #[test]
    fn a_text_delta_is_extracted() {
        let value = json!({ "choices": [{ "delta": { "content": "salt" } }] });
        assert_eq!(delta_of(&value).as_deref(), Some("salt"));
    }

    #[test]
    fn a_role_only_chunk_yields_nothing_rather_than_failing() {
        let value = json!({ "choices": [{ "delta": { "role": "assistant" } }] });
        assert_eq!(delta_of(&value), None);
    }

    #[test]
    fn an_empty_choices_array_yields_nothing() {
        assert_eq!(delta_of(&json!({ "choices": [] })), None);
        assert_eq!(delta_of(&json!({ "usage": { "total_tokens": 12 } })), None);
    }

    #[test]
    fn content_sent_as_parts_is_joined() {
        // Some compatible servers send structured content.
        let value = json!({
            "choices": [{ "delta": { "content": [
                { "type": "text", "text": "the " },
                { "type": "text", "text": "road" }
            ] } }]
        });
        assert_eq!(delta_of(&value).as_deref(), Some("the road"));
    }

    #[test]
    fn a_non_streaming_message_shape_is_also_understood() {
        // Servers that ignore `stream` and answer in one piece.
        let value = json!({ "choices": [{ "message": { "content": "whole answer" } }] });
        assert_eq!(delta_of(&value).as_deref(), Some("whole answer"));
    }

    #[tokio::test]
    async fn provider_failures_are_explained_in_terms_of_what_to_do() {
        let unauthorized = describe_failure(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"Invalid API key"}}"#.into(),
        )
        .await;
        assert_eq!(unauthorized.code, ErrorCode::Provider);
        assert!(unauthorized.message.contains("API key"));
        assert!(unauthorized.message.contains("has not been changed"));
        assert!(unauthorized.detail.unwrap().contains("Invalid API key"));

        let limited = describe_failure(reqwest::StatusCode::TOO_MANY_REQUESTS, String::new()).await;
        assert!(limited.message.contains("rate-limiting"));

        let broken =
            describe_failure(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "boom".into()).await;
        assert!(broken.message.contains("their end"));
    }

    #[test]
    fn a_cancelled_request_is_reported_as_cancelled_not_as_an_error() {
        assert_eq!(cancelled().code, ErrorCode::Cancelled);
    }
}
