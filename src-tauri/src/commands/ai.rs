//! AI commands.
//!
//! The one place where manuscript text can leave the machine, and it is
//! arranged so that cannot happen by accident:
//!
//! * [`ai_preview`] tells the writer exactly what a request would send, before
//!   anything is sent. The UI shows it and waits.
//! * [`ai_run`] is the only command that talks to a provider, and it needs an
//!   explicit action chosen by the writer.
//! * The API key is fetched from the OS credential store inside this module and
//!   never returned across IPC.
//! * Output becomes a Margin note. Nothing is written into a manuscript until
//!   the writer applies a suggestion and Grimoire has re-checked the text.

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::ai::context::{self, BuiltContext, ContextPolicy, ContextSources};
use crate::ai::{AiAction, AiChunk, AiMessage, AiRequest, AiRole, apply, prompts};
use crate::app_state::AppState;
use crate::credentials;
use crate::domain::ids::{AiProfileId, AiProviderId, SuggestionId};
use crate::error::{AppError, ErrorCode, Result};
use crate::repositories::ai::{AiProfile, AiProviderConfig, AiSuggestion, SuggestionStatus};
use crate::repositories::{self};

use super::parse_page_id;

// --- Configuration ----------------------------------------------------------

#[tauri::command]
pub async fn ai_providers(state: State<'_, AppState>) -> Result<Vec<AiProviderConfig>> {
    let mut providers = state.read(repositories::ai::list_providers).await?;
    // Whether a key exists is answered by the credential store, not the row.
    // The key itself is never part of this response.
    for provider in &mut providers {
        provider.has_key = credentials::exists(&provider.credential_ref);
    }
    Ok(providers)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ai_provider_save(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    base_url: String,
    model: String,
    temperature: f64,
    max_output_tokens: Option<i64>,
    context_length: i64,
    // Only sent when the writer typed a new one. Absent leaves the saved key
    // alone, so editing a model name does not require re-entering it.
    api_key: Option<String>,
) -> Result<AiProviderConfig> {
    let id = match id {
        Some(raw) => Some(AiProviderId::parse(&raw)?),
        None => None,
    };

    let mut saved = state
        .write(move |tx| {
            repositories::ai::upsert_provider(
                tx,
                id,
                &name,
                &base_url,
                &model,
                temperature,
                max_output_tokens,
                context_length,
            )
        })
        .await?;

    if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
        credentials::set(&saved.credential_ref, key.trim())?;
    }
    saved.has_key = credentials::exists(&saved.credential_ref);
    Ok(saved)
}

#[tauri::command]
pub async fn ai_provider_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    let id = AiProviderId::parse(&id)?;
    let credential_ref = state
        .write(move |tx| repositories::ai::delete_provider(tx, id))
        .await?;
    // The row and the secret go together. Leaving the key behind would orphan a
    // credential nothing refers to.
    credentials::delete(&credential_ref)?;
    Ok(())
}

#[tauri::command]
pub async fn ai_profiles(state: State<'_, AppState>) -> Result<Vec<AiProfile>> {
    state
        .write(|tx| {
            repositories::ai::ensure_builtin_profiles(tx)?;
            repositories::ai::list_profiles(tx)
        })
        .await
}

#[tauri::command]
pub async fn ai_profile_save(
    state: State<'_, AppState>,
    id: String,
    name: String,
    description: String,
    system_prompt: String,
    context_policy: String,
    temperature: Option<f64>,
) -> Result<AiProfile> {
    let id = AiProfileId::parse(&id)?;
    let policy = ContextPolicy::parse(&context_policy);
    state
        .write(move |tx| {
            repositories::ai::update_profile(
                tx,
                id,
                &name,
                &description,
                &system_prompt,
                policy,
                temperature,
            )
        })
        .await
}

#[tauri::command]
pub async fn ai_profile_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    let id = AiProfileId::parse(&id)?;
    state
        .write(move |tx| repositories::ai::delete_profile(tx, id))
        .await
}

// --- Running an action ------------------------------------------------------

/// Everything needed to run one request, assembled on the blocking thread.
struct Prepared {
    context: BuiltContext,
    request: AiRequest,
    action: AiAction,
    profile_name: String,
}

/// Gathers context and configuration for an action.
///
/// `with_key` is false for the preview, so a preview never touches the
/// credential store — the writer is being shown what *would* be sent, and that
/// question does not require a secret to answer.
fn prepare(
    conn: &rusqlite::Connection,
    page_id: crate::domain::PageId,
    profile_id: AiProfileId,
    action: AiAction,
    from: i64,
    to: i64,
    with_key: bool,
) -> Result<Prepared> {
    let settings = repositories::settings::load(conn)?;
    if !settings.ai_enabled {
        return Err(AppError::new(
            ErrorCode::Conflict,
            "AI is turned off. Turn it on in Settings if you want to use it.",
        ));
    }

    let profile = repositories::ai::get_profile(conn, profile_id)?;
    let provider = match profile.provider_id {
        Some(id) => repositories::ai::get_provider(conn, id)?,
        None => repositories::ai::active_provider(conn)?,
    };

    let page = repositories::pages::get(conn, page_id)?;
    let chapter = repositories::chapters::get(conn, page.chapter_id)?;
    let volume = repositories::volumes::get(conn, chapter.volume_id)?;

    // Neighbouring Pages, only for the policy that uses them — there is no
    // reason to read a Chapter's text to answer a question about one sentence.
    let neighbours: Vec<(String, String)> = if profile.context_policy == ContextPolicy::Chapter {
        repositories::pages::list_full(conn, page.chapter_id)?
            .into_iter()
            .filter(|other| other.id != page.id)
            .map(|other| (other.title, other.plain_text))
            .collect()
    } else {
        Vec::new()
    };

    let built = context::build(
        ContextSources {
            volume_title: &volume.title,
            chapter_title: &chapter.title,
            page_title: &page.title,
            page_text: &page.plain_text,
            from: from.max(0) as usize,
            to: to.max(0) as usize,
            neighbours: &neighbours,
        },
        profile.context_policy,
        settings.ai_context_budget_chars,
    );

    let api_key = if with_key {
        credentials::get(&provider.credential_ref)?
    } else {
        String::new()
    };

    let system = format!(
        "{}\n\n{}\n\n{}",
        prompts::BASE_SYSTEM,
        profile.system_prompt,
        prompts::action_instruction(action)
    );

    let request = AiRequest {
        base_url: provider.base_url,
        model: provider.model,
        api_key,
        messages: vec![
            AiMessage {
                role: AiRole::System,
                content: system,
            },
            AiMessage {
                role: AiRole::User,
                content: context::render(&built),
            },
        ],
        temperature: profile.temperature.unwrap_or(provider.temperature) as f32,
        max_output_tokens: provider.max_output_tokens,
    };

    Ok(Prepared {
        context: built,
        request,
        action,
        profile_name: profile.name,
    })
}

/// Describes what an action would send, without sending anything.
#[tauri::command]
pub async fn ai_preview(
    state: State<'_, AppState>,
    page_id: String,
    profile_id: String,
    action: String,
    from: i64,
    to: i64,
) -> Result<BuiltContext> {
    let page = parse_page_id(&page_id)?;
    let profile = AiProfileId::parse(&profile_id)?;
    let action = AiAction::parse(&action).ok_or_else(|| AppError::invalid("Unknown AI action."))?;

    state
        .read(move |conn| Ok(prepare(conn, page, profile, action, from, to, false)?.context))
        .await
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StreamEvent {
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoneEvent {
    request_id: String,
    /// The Margin note the reply became.
    annotation_id: String,
    /// Set when the reply proposed replacement text.
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion_id: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FailedEvent {
    request_id: String,
    message: String,
    code: crate::error::ErrorCode,
}

/// Runs an AI action over a selection and turns the reply into a Margin note.
///
/// Returns as soon as the request is under way; output arrives as `ai:delta`
/// events and finishes with `ai:done` or `ai:failed`. Nothing blocks the UI,
/// and the request can be stopped at any point with [`ai_cancel`].
#[tauri::command]
pub async fn ai_run(
    app: AppHandle,
    state: State<'_, AppState>,
    page_id: String,
    profile_id: String,
    action: String,
    from: i64,
    to: i64,
) -> Result<String> {
    let page = parse_page_id(&page_id)?;
    let profile = AiProfileId::parse(&profile_id)?;
    let action = AiAction::parse(&action).ok_or_else(|| AppError::invalid("Unknown AI action."))?;

    let prepared = state
        .read(move |conn| prepare(conn, page, profile, action, from, to, true))
        .await?;

    let request_id = Uuid::new_v4().to_string();
    let cancel = state.begin_request(&request_id);
    let provider = state.provider();
    let owned_state = state.inner().clone();
    let id_for_task = request_id.clone();

    tauri::async_runtime::spawn(async move {
        let outcome = stream_into_margin(
            &app,
            &owned_state,
            provider,
            prepared,
            page,
            from,
            to,
            &id_for_task,
            cancel,
        )
        .await;

        if let Err(error) = outcome {
            // Cancellation is the writer's choice, not a failure to report.
            if error.code != ErrorCode::Cancelled {
                tracing::warn!(code = ?error.code, detail = ?error.detail, "AI request failed");
                let _ = app.emit(
                    "ai:failed",
                    FailedEvent {
                        request_id: id_for_task.clone(),
                        message: error.message,
                        code: error.code,
                    },
                );
            }
        }

        owned_state.finish_request(&id_for_task);
    });

    Ok(request_id)
}

// Eight parameters, all of them distinct values this needs and none of them
// grouping naturally: bundling them into a struct would move the same list one
// line up without making anything clearer.
#[allow(clippy::too_many_arguments)]
async fn stream_into_margin(
    app: &AppHandle,
    state: &AppState,
    provider: std::sync::Arc<dyn crate::ai::AiProvider>,
    prepared: Prepared,
    page: crate::domain::PageId,
    from: i64,
    to: i64,
    request_id: &str,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<()> {
    let Prepared {
        request,
        action,
        profile_name,
        ..
    } = prepared;

    let mut stream = provider.stream(request, cancel.clone()).await?;
    let mut reply = String::new();

    while let Some(chunk) = stream.next().await {
        match chunk? {
            AiChunk::Delta(text) => {
                reply.push_str(&text);
                let _ = app.emit(
                    "ai:delta",
                    StreamEvent {
                        request_id: request_id.to_string(),
                        delta: Some(text),
                    },
                );
            }
            AiChunk::Done => break,
        }
    }

    if reply.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::Provider,
            "Your AI provider returned nothing. Nothing has been added to the Margin.",
        ));
    }

    let (replacement, note) = prompts::split_replacement(&reply);
    let body = if note.trim().is_empty() {
        reply.trim().to_string()
    } else {
        note
    };
    let label = format!("{} · {}", profile_name, prompts::action_label(action));

    // One transaction: the note and any proposal it carries appear together or
    // not at all.
    let (annotation_id, suggestion_id) = state
        .write(move |tx| {
            let mut annotation = repositories::annotations::create_anchored(
                tx,
                page,
                action.annotation_kind(),
                &body,
                from,
                to,
            )?;

            tx.execute(
                "UPDATE annotations SET author_profile = ?2 WHERE id = ?1",
                rusqlite::params![annotation.id, label],
            )?;
            annotation.author_profile = Some(label);

            let suggestion = match replacement {
                Some(text) if action.proposes_an_edit() => Some(
                    repositories::ai::create_suggestion(
                        tx,
                        page,
                        Some(annotation.id),
                        from,
                        to,
                        &text,
                    )?
                    .id,
                ),
                _ => None,
            };

            Ok((annotation.id, suggestion))
        })
        .await?;

    let _ = app.emit(
        "ai:done",
        DoneEvent {
            request_id: request_id.to_string(),
            annotation_id: annotation_id.to_string(),
            suggestion_id: suggestion_id.map(|id| id.to_string()),
        },
    );

    Ok(())
}

/// Stops a request. Safe to call for one that already finished.
#[tauri::command]
pub async fn ai_cancel(state: State<'_, AppState>, request_id: String) -> Result<bool> {
    Ok(state.cancel_request(&request_id))
}

// --- Suggestions ------------------------------------------------------------

#[tauri::command]
pub async fn ai_suggestions(
    state: State<'_, AppState>,
    page_id: String,
) -> Result<Vec<AiSuggestion>> {
    let page = parse_page_id(&page_id)?;
    state
        .read(move |conn| repositories::ai::suggestions_for_page(conn, page))
        .await
}

/// What would happen if this suggestion were applied right now.
///
/// This is "Re-evaluate": it re-checks the text without changing anything, and
/// records the answer, so a suggestion marked stale can come back if the writer
/// undoes whatever invalidated it.
#[tauri::command]
pub async fn ai_suggestion_reevaluate(
    state: State<'_, AppState>,
    id: String,
) -> Result<AiSuggestion> {
    let id = SuggestionId::parse(&id)?;
    state
        .write(move |tx| {
            let suggestion = repositories::ai::get_suggestion(tx, id)?;
            if matches!(
                suggestion.status,
                SuggestionStatus::Applied | SuggestionStatus::Dismissed
            ) {
                return Ok(suggestion);
            }

            let page = repositories::pages::get(tx, suggestion.page_id)?;
            let verdict = apply::validate(
                &page.plain_text,
                &suggestion.original_text,
                &suggestion.context_hash,
                suggestion.anchor_from,
                suggestion.anchor_to,
            );

            let status = match verdict {
                apply::Validation::Ready { .. } => SuggestionStatus::Pending,
                apply::Validation::Stale { .. } => SuggestionStatus::Stale,
            };
            repositories::ai::set_suggestion_status(tx, id, status)
        })
        .await
}

/// Applies a suggestion, after re-checking that it is still safe to.
///
/// The order matters and is the whole point:
///
/// 1. Re-read the Page and validate the suggestion against it. If the words
///    changed, mark it stale and change nothing.
/// 2. Snapshot the Page, so applying is undoable.
/// 3. Splice, refusing anything that cannot be expressed exactly.
/// 4. Save through the normal path, which re-derives text and re-anchors notes.
///
/// All four happen in one transaction. There is no state in which a revision
/// was taken but the edit did not land, or the reverse.
#[tauri::command]
pub async fn ai_suggestion_apply(
    state: State<'_, AppState>,
    id: String,
) -> Result<crate::domain::Page> {
    let id = SuggestionId::parse(&id)?;

    state
        .write(move |tx| {
            let suggestion = repositories::ai::get_suggestion(tx, id)?;
            if suggestion.status == SuggestionStatus::Applied {
                return Err(AppError::conflict(
                    "That suggestion has already been applied.",
                ));
            }

            let page = repositories::pages::get(tx, suggestion.page_id)?;

            let (from, to) = match apply::validate(
                &page.plain_text,
                &suggestion.original_text,
                &suggestion.context_hash,
                suggestion.anchor_from,
                suggestion.anchor_to,
            ) {
                apply::Validation::Ready { from, to } => (from, to),
                apply::Validation::Stale { reason } => {
                    repositories::ai::set_suggestion_status(tx, id, SuggestionStatus::Stale)?;
                    return Err(AppError::stale(reason));
                }
            };

            let document = apply::apply(&page.document, from, to, &suggestion.replacement_text)?;

            repositories::revisions::capture(
                tx,
                suggestion.page_id,
                repositories::revisions::RevisionReason::BeforeAi,
            )?;

            let saved = repositories::save_page(tx, suggestion.page_id, document)?;
            repositories::ai::set_suggestion_status(tx, id, SuggestionStatus::Applied)?;
            Ok(saved)
        })
        .await
}

#[tauri::command]
pub async fn ai_suggestion_dismiss(state: State<'_, AppState>, id: String) -> Result<AiSuggestion> {
    let id = SuggestionId::parse(&id)?;
    state
        .write(move |tx| {
            repositories::ai::set_suggestion_status(tx, id, SuggestionStatus::Dismissed)
        })
        .await
}

/// The actions the UI offers, so the list lives in one place.
#[tauri::command]
pub fn ai_actions() -> Vec<serde_json::Value> {
    AiAction::ALL
        .into_iter()
        .map(|action| {
            serde_json::json!({
                "id": action.as_str(),
                "label": prompts::action_label(action),
                "proposesAnEdit": action.proposes_an_edit(),
                "kind": action.annotation_kind().as_str(),
            })
        })
        .collect()
}
