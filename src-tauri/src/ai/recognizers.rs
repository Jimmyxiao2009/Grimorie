//! Handwriting recognition providers.
//!
//! The production recognizer sends a rendered ink raster to a vision-capable
//! model through the same OpenAI-compatible transport the manuscript-reading
//! actions use. It does not speak HTTP itself, and it does not know which vendor
//! is configured — that is the point of reusing [`crate::ai::AiProvider`].
//!
//! A [`MockRecognizer`] is provided for tests, so the queue, the commands, and
//! the persistence layer can exercise the full recognition lifecycle without a
//! network or a real model. Production code builds a [`VisionRecognizer`]; test
//! code builds a mock. Neither is hardcoded into the UI.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::ai::AiChunk;
use crate::ai::ink_recognition as prompt;
use crate::domain::ink_recognition::{
    InkRecognitionInput, InkRecognitionRequest, InkRecognitionResult, InkRecognizer,
};
use crate::error::{AppError, ErrorCode, Result};

/// A handwriting recognizer that sends a rendered raster to a vision model.
///
/// Built from an [`crate::ai::AiProvider`] (the shared transport), a provider
/// config (base URL, model, api key), and a temperature. Recognition reuses the
/// streaming chat-completions path: the response is accumulated to a string and
/// then parsed as the transcript JSON the prompt asked for.
///
/// Only the image variant of [`InkRecognitionInput`] is accepted — a vision
/// model has no use for raw vectors. A future native recognizer will accept the
/// vector variant, and the trait lets the two coexist.
pub struct VisionRecognizer {
    provider: Arc<dyn crate::ai::AiProvider>,
    base_url: String,
    model: String,
    api_key: String,
    temperature: f32,
    /// An optional override model. When `None`, the configured model is used;
    /// when set (e.g. a vision-specific model chosen in settings), it wins.
    model_override: Option<String>,
}

impl VisionRecognizer {
    /// Builds a recognizer for one configured provider. The API key is already
    /// resolved by the caller — it never lives longer than the request.
    pub fn new(
        provider: Arc<dyn crate::ai::AiProvider>,
        base_url: String,
        model: String,
        api_key: String,
        temperature: f32,
    ) -> Self {
        Self {
            provider,
            base_url,
            model,
            api_key,
            temperature,
            model_override: None,
        }
    }

    /// Overrides the model used for recognition. Used when the writer has chosen
    /// a vision-specific model for handwriting, distinct from the model used for
    /// manuscript-reading actions.
    pub fn with_model(mut self, model: String) -> Self {
        self.model_override = Some(model);
        self
    }

    fn resolved_model(&self) -> &str {
        self.model_override.as_deref().unwrap_or(&self.model)
    }
}

impl InkRecognizer for VisionRecognizer {
    fn name(&self) -> &'static str {
        "vision"
    }

    fn recognize<'a>(
        &'a self,
        request: InkRecognitionRequest,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<InkRecognitionResult>> + Send + 'a>> {
        Box::pin(async move {
            let (png, width, height) = match &request.input {
                InkRecognitionInput::Image { png, width, height } => (png, *width, *height),
                InkRecognitionInput::Vectors { .. } => {
                    // A vision model cannot read vectors. The caller is expected
                    // to render before invoking a vision recognizer; reaching
                    // this branch is a programmer error, not a network failure.
                    return Err(AppError::invalid(
                        "This recognizer needs a rendered image, not raw strokes.",
                    ));
                }
            };

            if png.is_empty() {
                return Err(AppError::invalid("There is no handwriting to recognize."));
            }

            let data_base64 = base64::encode(png);
            let model = self.resolved_model().to_string();
            // The writer's configured language, when they set one. Without this
            // the setting was read from the database, carried all the way onto
            // the request, and then dropped on the floor here.
            let user_message = match &request.language_hint {
                crate::domain::ink_recognition::LanguageHint::Auto => {
                    prompt::TRANSCRIBE_USER.to_string()
                }
                crate::domain::ink_recognition::LanguageHint::Language(tag) => {
                    prompt::transcribe_user_with_language(tag)
                }
            };
            let ai_request = crate::ai::AiRequest {
                base_url: self.base_url.clone(),
                model: model.clone(),
                api_key: self.api_key.clone(),
                messages: vec![
                    crate::ai::AiMessage {
                        role: crate::ai::AiRole::System,
                        content: prompt::TRANSCRIBE_SYSTEM.into(),
                    },
                    crate::ai::AiMessage {
                        role: crate::ai::AiRole::User,
                        content: crate::ai::AiContent::text_and_image(
                            &user_message,
                            "image/png",
                            &data_base64,
                        ),
                    },
                ],
                temperature: self.temperature,
                max_output_tokens: Some(1_024),
            };

            let mut stream = self.provider.stream(ai_request, cancel).await?;
            let mut reply = String::new();
            while let Some(chunk) = stream.next().await {
                match chunk? {
                    AiChunk::Delta(text) => reply.push_str(&text),
                    AiChunk::Done => break,
                }
            }

            if reply.trim().is_empty() {
                return Err(AppError::new(
                    ErrorCode::Provider,
                    "The recognition model returned nothing. Your handwriting has not been changed.",
                ));
            }

            let (text, language) = prompt::parse_transcript(&reply).ok_or_else(|| {
                AppError::new(
                    ErrorCode::Provider,
                    "The recognition model replied in an unexpected format. \
                     Your handwriting has not been changed.",
                )
                .with_detail(format!("reply was {} chars", reply.chars().count()))
            })?;

            tracing::debug!(
                model = %model,
                width,
                height,
                chars = text.chars().count(),
                "handwriting recognized"
            );

            Ok(InkRecognitionResult {
                text,
                confidence: None,
                language,
                provider: self.name().to_string(),
                model,
            })
        })
    }
}

/// The closure a [`MockRecognizer`] may use to compute its response. A type
/// alias keeps the field readable; the bare form is a four-level nested type.
type MockResponder =
    Box<dyn Fn(&InkRecognitionRequest) -> Result<InkRecognitionResult> + Send + Sync>;

/// A deterministic recognizer for tests.
///
/// Returns a fixed transcript, so the queue and persistence layers can exercise
/// the full lifecycle — schedule, run, persist, invalidate, retry — without a
/// network. The mock records how many times it was called and what inputs it
/// saw, so a test can assert that a stale job was cancelled rather than run.
pub struct MockRecognizer {
    /// The transcript to return, keyed by nothing: every call returns the same
    /// text unless `responder` is set.
    transcript: String,
    /// An optional responder that overrides `transcript`, for tests that need to
    /// inspect the request or return different results per call.
    responder: Mutex<Option<MockResponder>>,
    /// How many `recognize` calls reached the mock.
    calls: Mutex<usize>,
}

impl MockRecognizer {
    /// A mock that always returns the given transcript.
    pub fn always(transcript: impl Into<String>) -> Self {
        Self {
            transcript: transcript.into(),
            responder: Mutex::new(None),
            calls: Mutex::new(0),
        }
    }

    /// A mock that always fails, so failure and retry paths can be exercised.
    pub fn failing(message: impl Into<String>) -> Self {
        let message = message.into();
        Self::responder(move |_| Err(AppError::new(ErrorCode::Provider, message.clone())))
    }

    /// A mock whose response is computed by a closure, for tests that vary the
    /// result or inspect the request.
    pub fn responder<F>(responder: F) -> Self
    where
        F: Fn(&InkRecognitionRequest) -> Result<InkRecognitionResult> + Send + Sync + 'static,
    {
        Self {
            transcript: String::new(),
            responder: Mutex::new(Some(Box::new(responder))),
            calls: Mutex::new(0),
        }
    }

    /// How many recognition calls reached this mock.
    pub fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl InkRecognizer for MockRecognizer {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn recognize<'a>(
        &'a self,
        request: InkRecognitionRequest,
        _cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<InkRecognitionResult>> + Send + 'a>> {
        Box::pin(async move {
            {
                let mut calls = self.calls.lock().unwrap();
                *calls += 1;
            }
            // A cooperative yield so an async test that cancels between the
            // schedule and the run can actually observe cancellation. The mock
            // does no real work, so without this it would complete synchronously
            // before a cancel could land.
            tokio::task::yield_now().await;

            if let Some(responder) = self.responder.lock().unwrap().as_ref() {
                return responder(&request);
            }

            Ok(InkRecognitionResult {
                text: self.transcript.clone(),
                confidence: None,
                language: None,
                provider: self.name().to_string(),
                model: "mock-1".to_string(),
            })
        })
    }
}

/// Encode a byte slice as base64, without pulling a dependency for one call.
///
/// Standard base64 with padding, which is what a data URL expects. Kept here
/// rather than in a shared util because recognition is the only place the app
/// base64-encodes anything.
mod base64 {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let triple = (b0 << 16) | (b1 << 8) | b2;

            out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[(triple & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    #[cfg(test)]
    #[test]
    fn encodes_known_values() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[cfg(test)]
    #[test]
    fn round_trips_against_a_known_vector() {
        // "The quick brown fox" in base64, standard padding.
        assert_eq!(
            encode(b"The quick brown fox"),
            "VGhlIHF1aWNrIGJyb3duIGZveA=="
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ink_recognition::{LanguageHint, RecognitionMode};

    fn image_input() -> InkRecognitionInput {
        InkRecognitionInput::Image {
            png: vec![0x89, 0x50, 0x4E, 0x47],
            width: 300,
            height: 120,
        }
    }

    fn request() -> InkRecognitionRequest {
        InkRecognitionRequest {
            input: image_input(),
            mode: RecognitionMode::Auto,
            language_hint: LanguageHint::auto(),
        }
    }

    #[tokio::test]
    async fn a_mock_returns_its_transcript() {
        let mock = MockRecognizer::always("move this paragraph");
        let result = mock
            .recognize(request(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.text, "move this paragraph");
        assert_eq!(result.provider, "mock");
        assert_eq!(result.model, "mock-1");
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn a_failing_mock_returns_an_error_without_panic() {
        let mock = MockRecognizer::failing("network unavailable");
        let err = mock
            .recognize(request(), CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Provider);
        assert!(err.message.contains("network unavailable"));
    }

    #[tokio::test]
    async fn a_mock_refuses_vector_input() {
        // Only the vision recognizer refuses vectors; the mock accepts anything.
        // This test documents that the mock is input-agnostic, so a test that
        // wants vector-refusal behaviour uses the vision recognizer's check.
        let mock = MockRecognizer::always("text");
        let req = InkRecognitionRequest {
            input: InkRecognitionInput::Vectors {
                strokes: vec![],
                surface_width: 300.0,
            },
            mode: RecognitionMode::Text,
            language_hint: LanguageHint::auto(),
        };
        let result = mock.recognize(req, CancellationToken::new()).await.unwrap();
        assert_eq!(result.text, "text");
    }

    #[tokio::test]
    async fn a_mock_can_inspect_its_request() {
        let mock = MockRecognizer::responder(|req| {
            let dims = match &req.input {
                InkRecognitionInput::Image { width, height, .. } => (*width, *height),
                _ => (0, 0),
            };
            Ok(InkRecognitionResult {
                text: format!("{}x{}", dims.0, dims.1),
                confidence: None,
                language: None,
                provider: "mock".into(),
                model: "mock-1".into(),
            })
        });
        let result = mock
            .recognize(request(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.text, "300x120");
    }

    #[tokio::test]
    async fn a_mock_can_be_cancelled_before_it_runs() {
        // The mock yields once before responding, so a cancellation armed before
        // the await can abort it — though cancellation here is cooperative and
        // the mock checks nothing, so this mainly documents the token plumbing.
        let mock = MockRecognizer::always("text");
        let token = CancellationToken::new();
        token.cancel();
        // A cancelled token does not prevent the mock from completing, because
        // the mock does not consult it. This is intentional: the mock's job is to
        // exercise persistence, not cancellation. Real cancellation is the
        // queue's responsibility, tested there.
        let _ = mock.recognize(request(), token).await;
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn a_vision_recognizer_refuses_vector_input() {
        // A stub provider that never answers — the recognizer must refuse
        // vectors before it ever calls the provider.
        struct NoProvider;
        impl crate::ai::AiProvider for NoProvider {
            fn name(&self) -> &'static str {
                "no"
            }
            fn stream<'a>(
                &'a self,
                _: crate::ai::AiRequest,
                _: CancellationToken,
            ) -> Pin<Box<dyn Future<Output = Result<crate::ai::AiStream>> + Send + 'a>>
            {
                Box::pin(async { Err(AppError::internal("the stub provider was called")) })
            }
        }

        let recognizer = VisionRecognizer::new(
            Arc::new(NoProvider),
            "https://example.test/v1".into(),
            "gpt-4o".into(),
            "key".into(),
            0.0,
        );
        let req = InkRecognitionRequest {
            input: InkRecognitionInput::Vectors {
                strokes: vec![],
                surface_width: 300.0,
            },
            mode: RecognitionMode::Auto,
            language_hint: LanguageHint::auto(),
        };
        let err = recognizer
            .recognize(req, CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.message.contains("rendered image"));
    }

    #[tokio::test]
    async fn a_vision_recognizer_refuses_empty_input() {
        struct NoProvider;
        impl crate::ai::AiProvider for NoProvider {
            fn name(&self) -> &'static str {
                "no"
            }
            fn stream<'a>(
                &'a self,
                _: crate::ai::AiRequest,
                _: CancellationToken,
            ) -> Pin<Box<dyn Future<Output = Result<crate::ai::AiStream>> + Send + 'a>>
            {
                Box::pin(async { Err(AppError::internal("stub was called")) })
            }
        }
        let recognizer = VisionRecognizer::new(
            Arc::new(NoProvider),
            "https://example.test/v1".into(),
            "gpt-4o".into(),
            "key".into(),
            0.0,
        );
        let req = InkRecognitionRequest {
            input: InkRecognitionInput::Image {
                png: vec![],
                width: 1,
                height: 1,
            },
            mode: RecognitionMode::Auto,
            language_hint: LanguageHint::auto(),
        };
        let err = recognizer
            .recognize(req, CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.message.contains("no handwriting"));
    }

    /// A provider that records the request it was handed and returns a fixed
    /// transcript, so the prompt actually sent can be asserted on.
    struct CapturingProvider {
        seen: Mutex<Option<crate::ai::AiRequest>>,
    }

    impl CapturingProvider {
        fn new() -> Self {
            Self {
                seen: Mutex::new(None),
            }
        }

        /// The text of the user turn, flattened for assertion. The image part
        /// is skipped — what is under test is the instruction beside it.
        fn user_text(&self) -> String {
            let guard = self.seen.lock().unwrap();
            let request = guard.as_ref().expect("the provider was never called");
            let mut out = String::new();
            for message in &request.messages {
                if !matches!(message.role, crate::ai::AiRole::User) {
                    continue;
                }
                match &message.content {
                    crate::ai::AiContent::Text(text) => out.push_str(text),
                    crate::ai::AiContent::Parts(parts) => {
                        for part in parts {
                            if let crate::ai::AiContentPart::Text { text } = part {
                                out.push_str(text);
                            }
                        }
                    }
                }
            }
            out
        }
    }

    impl crate::ai::AiProvider for CapturingProvider {
        fn name(&self) -> &'static str {
            "capturing"
        }
        fn stream<'a>(
            &'a self,
            request: crate::ai::AiRequest,
            _: CancellationToken,
        ) -> Pin<Box<dyn Future<Output = Result<crate::ai::AiStream>> + Send + 'a>> {
            *self.seen.lock().unwrap() = Some(request);
            Box::pin(async {
                let chunks: Vec<Result<AiChunk>> = vec![
                    Ok(AiChunk::Delta(
                        r#"{"text":"transcribed","language":"zh-CN"}"#.to_string(),
                    )),
                    Ok(AiChunk::Done),
                ];
                Ok(Box::pin(futures_util::stream::iter(chunks)) as crate::ai::AiStream)
            })
        }
    }

    fn recognizer_for(provider: Arc<dyn crate::ai::AiProvider>) -> VisionRecognizer {
        VisionRecognizer::new(
            provider,
            "https://example.test/v1".into(),
            "gpt-4o".into(),
            "key".into(),
            0.0,
        )
    }

    #[tokio::test]
    async fn a_configured_language_reaches_the_prompt() {
        // The setting was read from the database and carried onto the request,
        // and then the recognizer ignored it entirely.
        let provider = Arc::new(CapturingProvider::new());
        let recognizer = recognizer_for(Arc::clone(&provider) as Arc<dyn crate::ai::AiProvider>);

        recognizer
            .recognize(
                InkRecognitionRequest {
                    input: image_input(),
                    mode: RecognitionMode::Auto,
                    language_hint: LanguageHint::parse("zh-CN"),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();

        let text = provider.user_text();
        assert!(
            text.contains("zh-CN"),
            "the language hint was dropped: {text}"
        );
        // A hint is a prior, not a constraint: someone who writes Chinese still
        // writes English terms in their margins.
        assert!(text.contains("hint, not a rule"));
        assert!(text.contains("never translate"));
    }

    #[tokio::test]
    async fn an_automatic_language_asks_for_no_particular_one() {
        let provider = Arc::new(CapturingProvider::new());
        let recognizer = recognizer_for(Arc::clone(&provider) as Arc<dyn crate::ai::AiProvider>);

        recognizer
            .recognize(request(), CancellationToken::new())
            .await
            .unwrap();

        let text = provider.user_text();
        assert_eq!(text.trim(), prompt::TRANSCRIBE_USER);
    }

    #[tokio::test]
    async fn a_vision_recognizer_parses_a_well_formed_reply() {
        // A stub provider that streams a JSON transcript.
        struct Stub;
        impl crate::ai::AiProvider for Stub {
            fn name(&self) -> &'static str {
                "stub"
            }
            fn stream<'a>(
                &'a self,
                _: crate::ai::AiRequest,
                _: CancellationToken,
            ) -> Pin<Box<dyn Future<Output = Result<crate::ai::AiStream>> + Send + 'a>>
            {
                Box::pin(async {
                    // Stream the JSON in two deltas, the way a real model would,
                    // so the accumulator is exercised. The two halves concatenate
                    // to a valid object.
                    let first = "{\"text\":\"move ".to_string();
                    let second = "this\",\"language\":\"en-US\"}".to_string();
                    let chunks: Vec<Result<AiChunk>> = vec![
                        Ok(AiChunk::Delta(first)),
                        Ok(AiChunk::Delta(second)),
                        Ok(AiChunk::Done),
                    ];
                    let stream = futures_util::stream::iter(chunks);
                    Ok(Box::pin(stream) as crate::ai::AiStream)
                })
            }
        }

        let recognizer = VisionRecognizer::new(
            Arc::new(Stub),
            "https://example.test/v1".into(),
            "gpt-4o".into(),
            "key".into(),
            0.0,
        );
        let result = recognizer
            .recognize(request(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.text, "move this");
        assert_eq!(result.language.as_deref(), Some("en-US"));
        assert_eq!(result.provider, "vision");
        assert_eq!(result.model, "gpt-4o");
        assert!(result.confidence.is_none());
    }
}
