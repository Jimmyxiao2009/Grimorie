//! Transport tests for the OpenAI-compatible provider.
//!
//! These run against a real socket rather than a mocked client. The parts most
//! likely to break — SSE framing across arbitrary packet boundaries,
//! cancellation mid-response, and turning an HTTP failure into something a
//! writer can act on — only exist at the socket level, and a mocked client
//! would test the mock instead.
//!
//! The server here is a few lines of `tokio` writing literal bytes. That is the
//! point: it can produce the awkward shapes a real provider produces, including
//! a delta split down the middle of a word.

#![cfg(test)]

use std::time::Duration;

use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::openai::OpenAiCompatible;
use super::{AiChunk, AiMessage, AiProvider, AiRequest, AiRole};
use crate::error::ErrorCode;

/// Serves one request, writing `chunks` with a pause between them.
///
/// Returns the base URL to point the provider at.
async fn serve(status_line: &'static str, chunks: Vec<&'static str>, pause: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();

    tokio::spawn(async move {
        let Ok((mut socket, _)) = listener.accept().await else {
            return;
        };

        // Read the request headers, so the client is not left waiting on a
        // half-open connection.
        let mut scratch = vec![0u8; 8192];
        let _ = socket.read(&mut scratch).await;

        let header = format!(
            "{status_line}\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\
             Connection: close\r\n\r\n"
        );
        if socket.write_all(header.as_bytes()).await.is_err() {
            return;
        }

        for chunk in chunks {
            if socket.write_all(chunk.as_bytes()).await.is_err() {
                return;
            }
            let _ = socket.flush().await;
            tokio::time::sleep(pause).await;
        }
        let _ = socket.shutdown().await;
    });

    format!("http://127.0.0.1:{port}/v1")
}

fn request(base_url: String) -> AiRequest {
    AiRequest {
        base_url,
        model: "a-model".into(),
        api_key: "sk-test".into(),
        messages: vec![AiMessage {
            role: AiRole::User,
            content: "hello".into(),
        }],
        temperature: 0.7,
        max_output_tokens: None,
    }
}

async fn collect(base_url: String, cancel: CancellationToken) -> Vec<AiChunk> {
    let provider = OpenAiCompatible::new().expect("client");
    let Ok(mut stream) = provider.stream(request(base_url), cancel).await else {
        panic!("the provider refused to start streaming");
    };

    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(chunk) => {
                let done = chunk == AiChunk::Done;
                chunks.push(chunk);
                if done {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    chunks
}

fn text_of(chunks: &[AiChunk]) -> String {
    chunks
        .iter()
        .filter_map(|chunk| match chunk {
            AiChunk::Delta(text) => Some(text.as_str()),
            AiChunk::Done => None,
        })
        .collect()
}

#[tokio::test]
async fn a_streamed_reply_arrives_in_order() {
    let base = serve(
        "HTTP/1.1 200 OK",
        vec![
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"The road \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"had been salt.\"}}]}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(5),
    )
    .await;

    let chunks = collect(base, CancellationToken::new()).await;
    assert_eq!(text_of(&chunks), "The road had been salt.");
    assert_eq!(chunks.last(), Some(&AiChunk::Done));
}

#[tokio::test]
async fn an_event_split_across_packets_is_reassembled() {
    // The failure this guards against truncates words mid-token, which reads
    // as the model producing nonsense rather than as a framing bug.
    let base = serve(
        "HTTP/1.1 200 OK",
        vec![
            "data: {\"choices\":[{\"delta\":{\"cont",
            "ent\":\"salt\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" road\"",
            "}}]}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(5),
    )
    .await;

    let chunks = collect(base, CancellationToken::new()).await;
    assert_eq!(text_of(&chunks), "salt road");
}

#[tokio::test]
async fn several_events_in_one_packet_all_arrive() {
    let base = serve(
        "HTTP/1.1 200 OK",
        vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"c\"}}]}\n\n\
             data: [DONE]\n\n",
        ],
        Duration::from_millis(1),
    )
    .await;

    assert_eq!(
        text_of(&collect(base, CancellationToken::new()).await),
        "abc"
    );
}

#[tokio::test]
async fn a_malformed_event_does_not_abandon_the_rest() {
    let base = serve(
        "HTTP/1.1 200 OK",
        vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"before \"}}]}\n\n",
            "data: {this is not json}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"after\"}}]}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(5),
    )
    .await;

    assert_eq!(
        text_of(&collect(base, CancellationToken::new()).await),
        "before after"
    );
}

#[tokio::test]
async fn comments_and_blank_lines_are_ignored() {
    // Some servers send SSE keep-alive comments during a long generation.
    let base = serve(
        "HTTP/1.1 200 OK",
        vec![
            ": keep-alive\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"text\"}}]}\n\n",
            "\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(5),
    )
    .await;

    assert_eq!(
        text_of(&collect(base, CancellationToken::new()).await),
        "text"
    );
}

#[tokio::test]
async fn a_server_that_closes_without_done_still_finishes_cleanly() {
    let base = serve(
        "HTTP/1.1 200 OK",
        vec!["data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n"],
        Duration::from_millis(5),
    )
    .await;

    let chunks = collect(base, CancellationToken::new()).await;
    assert_eq!(text_of(&chunks), "partial");
    assert_eq!(
        chunks.last(),
        Some(&AiChunk::Done),
        "closing the body is a normal finish"
    );
}

#[tokio::test]
async fn cancelling_mid_response_stops_the_stream() {
    let base = serve(
        "HTTP/1.1 200 OK",
        vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"one \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"two \"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"three\"}}]}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(120),
    )
    .await;

    let cancel = CancellationToken::new();
    let provider = OpenAiCompatible::new().expect("client");
    let Ok(mut stream) = provider.stream(request(base), cancel.clone()).await else {
        panic!("the provider refused to start streaming");
    };

    let first = stream
        .next()
        .await
        .expect("a first chunk")
        .expect("no error");
    assert!(matches!(first, AiChunk::Delta(_)));

    cancel.cancel();

    // The next poll reports cancellation rather than continuing to read.
    match stream.next().await {
        Some(Err(error)) => assert_eq!(error.code, ErrorCode::Cancelled),
        Some(Ok(chunk)) => panic!("kept streaming after cancellation: {chunk:?}"),
        None => {} // Ended immediately, which is also a stop.
    }
}

#[tokio::test]
async fn a_rejected_key_is_reported_as_something_to_fix_in_settings() {
    let base = serve(
        "HTTP/1.1 401 Unauthorized",
        vec!["{\"error\":{\"message\":\"Incorrect API key provided\"}}"],
        Duration::from_millis(1),
    )
    .await;

    let provider = OpenAiCompatible::new().expect("client");
    let Err(error) = provider
        .stream(request(base), CancellationToken::new())
        .await
    else {
        panic!("a 401 must fail");
    };

    assert_eq!(error.code, ErrorCode::Provider);
    assert!(error.message.contains("API key"), "{}", error.message);
    assert!(error.message.contains("Settings"), "{}", error.message);
    // The provider's own words are kept for the details disclosure.
    assert!(error.detail.unwrap().contains("Incorrect API key"));
}

#[tokio::test]
async fn an_unreachable_provider_says_to_check_the_address() {
    // Nothing is listening on this port.
    let provider = OpenAiCompatible::new().expect("client");
    let Err(error) = provider
        .stream(
            request("http://127.0.0.1:1/v1".to_string()),
            CancellationToken::new(),
        )
        .await
    else {
        panic!("an unreachable host must fail");
    };

    assert_eq!(error.code, ErrorCode::Network);
    assert!(
        error.message.contains("couldn't reach"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn cancelling_before_the_response_arrives_reports_cancellation() {
    let cancel = CancellationToken::new();
    cancel.cancel();

    let provider = OpenAiCompatible::new().expect("client");
    let Err(error) = provider
        .stream(request("http://127.0.0.1:1/v1".to_string()), cancel)
        .await
    else {
        panic!("an already-cancelled request must not proceed");
    };

    assert_eq!(error.code, ErrorCode::Cancelled);
}
