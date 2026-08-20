//! AI provider, profile, and suggestion persistence.

use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};

use crate::ai::context::ContextPolicy;
use crate::ai::prompts;
use crate::domain::annotation::hash;
use crate::domain::ids::{AiProfileId, AiProviderId, AnnotationId, PageId, SuggestionId};
use crate::domain::manuscript::{Timestamp, now};
use crate::error::{AppError, Result};

// --- Providers --------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConfig {
    pub id: AiProviderId,
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// Names the entry in the OS credential store. Never the key itself.
    pub credential_ref: String,
    pub temperature: f64,
    pub max_output_tokens: Option<i64>,
    pub context_length: i64,
    /// Whether a key is actually saved, so Settings can say so without ever
    /// receiving the key.
    #[serde(default)]
    pub has_key: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn map_provider(row: &Row<'_>) -> rusqlite::Result<AiProviderConfig> {
    Ok(AiProviderConfig {
        id: row.get("id")?,
        name: row.get("name")?,
        base_url: row.get("base_url")?,
        model: row.get("model")?,
        credential_ref: row.get("credential_ref")?,
        temperature: row.get("temperature")?,
        max_output_tokens: row.get("max_output_tokens")?,
        context_length: row.get("context_length")?,
        has_key: false,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn upsert_provider(
    conn: &Connection,
    id: Option<AiProviderId>,
    name: &str,
    base_url: &str,
    model: &str,
    temperature: f64,
    max_output_tokens: Option<i64>,
    context_length: i64,
) -> Result<AiProviderConfig> {
    let base_url = base_url.trim();
    let model = model.trim();
    if base_url.is_empty() {
        return Err(AppError::invalid("A provider needs an address."));
    }
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err(AppError::invalid(
            "A provider address must start with http:// or https://.",
        ));
    }
    if model.is_empty() {
        return Err(AppError::invalid("A provider needs a model name."));
    }

    let temperature = temperature.clamp(0.0, 2.0);
    let context_length = context_length.clamp(1_000, 2_000_000);
    let at = now();

    match id {
        Some(id) => {
            let changed = conn.execute(
                "UPDATE ai_providers
                    SET name = ?2, base_url = ?3, model = ?4, temperature = ?5,
                        max_output_tokens = ?6, context_length = ?7, updated_at = ?8
                  WHERE id = ?1",
                params![
                    id,
                    name.trim(),
                    base_url,
                    model,
                    temperature,
                    max_output_tokens,
                    context_length,
                    at
                ],
            )?;
            if changed == 0 {
                return Err(AppError::not_found("AI provider"));
            }
            get_provider(conn, id)
        }
        None => {
            let id = AiProviderId::new();
            // The credential reference is derived from the id, so it is stable
            // for the provider's life and cannot collide with another's.
            let credential_ref = format!("provider.{id}");
            conn.execute(
                "INSERT INTO ai_providers (id, name, base_url, model, credential_ref,
                                           temperature, max_output_tokens, context_length,
                                           created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                params![
                    id,
                    name.trim(),
                    base_url,
                    model,
                    credential_ref,
                    temperature,
                    max_output_tokens,
                    context_length,
                    at
                ],
            )?;
            get_provider(conn, id)
        }
    }
}

pub fn get_provider(conn: &Connection, id: AiProviderId) -> Result<AiProviderConfig> {
    conn.query_row(
        "SELECT * FROM ai_providers WHERE id = ?1",
        params![id],
        map_provider,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => AppError::not_found("AI provider"),
        other => other.into(),
    })
}

pub fn list_providers(conn: &Connection) -> Result<Vec<AiProviderConfig>> {
    let mut statement = conn.prepare("SELECT * FROM ai_providers ORDER BY created_at")?;
    let rows = statement
        .query_map([], map_provider)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The provider a request should use.
///
/// Grimoire supports one configured provider at a time in this iteration; the
/// table allows several so that adding a picker later is a UI change rather
/// than a migration.
pub fn active_provider(conn: &Connection) -> Result<AiProviderConfig> {
    list_providers(conn)?.into_iter().next().ok_or_else(|| {
        AppError::new(
            crate::error::ErrorCode::Conflict,
            "No AI provider is configured yet. Add one in Settings.",
        )
    })
}

pub fn delete_provider(conn: &Connection, id: AiProviderId) -> Result<String> {
    let provider = get_provider(conn, id)?;
    conn.execute("DELETE FROM ai_providers WHERE id = ?1", params![id])?;
    // Returned so the caller can remove the secret too. Deleting the row
    // without deleting the key would leave a credential nothing refers to.
    Ok(provider.credential_ref)
}

// --- Profiles ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProfile {
    pub id: AiProfileId,
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub provider_id: Option<AiProviderId>,
    pub temperature: Option<f64>,
    pub context_policy: ContextPolicy,
    pub builtin: bool,
    pub position: i64,
}

fn map_profile(row: &Row<'_>) -> rusqlite::Result<AiProfile> {
    Ok(AiProfile {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        system_prompt: row.get("system_prompt")?,
        provider_id: row.get("provider_id")?,
        temperature: row.get("temperature")?,
        context_policy: ContextPolicy::parse(&row.get::<_, String>("context_policy")?),
        builtin: row.get::<_, i64>("builtin")? != 0,
        position: row.get("position")?,
    })
}

/// Creates the shipped profiles if none exist.
///
/// Only when the table is *empty*. A writer who deleted a profile they did not
/// want should not find it back the next morning.
pub fn ensure_builtin_profiles(conn: &Connection) -> Result<usize> {
    let existing: i64 = conn.query_row("SELECT count(*) FROM ai_profiles", [], |row| row.get(0))?;
    if existing > 0 {
        return Ok(0);
    }

    let at = now();
    for (position, profile) in prompts::BUILTIN_PROFILES.iter().enumerate() {
        conn.execute(
            "INSERT INTO ai_profiles (id, name, description, system_prompt, provider_id,
                                      temperature, context_policy, builtin, position,
                                      created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, 1, ?6, ?7, ?7)",
            params![
                AiProfileId::new(),
                profile.name,
                profile.description,
                profile.system_prompt,
                profile.context_policy,
                position as i64,
                at
            ],
        )?;
    }

    Ok(prompts::BUILTIN_PROFILES.len())
}

pub fn list_profiles(conn: &Connection) -> Result<Vec<AiProfile>> {
    let mut statement = conn.prepare("SELECT * FROM ai_profiles ORDER BY position, name")?;
    let rows = statement
        .query_map([], map_profile)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get_profile(conn: &Connection, id: AiProfileId) -> Result<AiProfile> {
    conn.query_row(
        "SELECT * FROM ai_profiles WHERE id = ?1",
        params![id],
        map_profile,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => AppError::not_found("AI profile"),
        other => other.into(),
    })
}

pub fn update_profile(
    conn: &Connection,
    id: AiProfileId,
    name: &str,
    description: &str,
    system_prompt: &str,
    context_policy: ContextPolicy,
    temperature: Option<f64>,
) -> Result<AiProfile> {
    if name.trim().is_empty() {
        return Err(AppError::invalid("A profile needs a name."));
    }
    if system_prompt.trim().is_empty() {
        return Err(AppError::invalid("A profile needs instructions."));
    }

    let changed = conn.execute(
        "UPDATE ai_profiles
            SET name = ?2, description = ?3, system_prompt = ?4, context_policy = ?5,
                temperature = ?6, updated_at = ?7
          WHERE id = ?1",
        params![
            id,
            name.trim(),
            description.trim(),
            system_prompt.trim(),
            context_policy.as_str(),
            temperature.map(|t| t.clamp(0.0, 2.0)),
            now()
        ],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("AI profile"));
    }
    get_profile(conn, id)
}

pub fn delete_profile(conn: &Connection, id: AiProfileId) -> Result<()> {
    let changed = conn.execute("DELETE FROM ai_profiles WHERE id = ?1", params![id])?;
    if changed == 0 {
        return Err(AppError::not_found("AI profile"));
    }
    Ok(())
}

// --- Suggestions ------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuggestionStatus {
    Pending,
    Applied,
    Dismissed,
    /// The text it was computed against has changed.
    Stale,
}

impl SuggestionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SuggestionStatus::Pending => "pending",
            SuggestionStatus::Applied => "applied",
            SuggestionStatus::Dismissed => "dismissed",
            SuggestionStatus::Stale => "stale",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "applied" => SuggestionStatus::Applied,
            "dismissed" => SuggestionStatus::Dismissed,
            "stale" => SuggestionStatus::Stale,
            _ => SuggestionStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSuggestion {
    pub id: SuggestionId,
    pub page_id: PageId,
    pub annotation_id: Option<AnnotationId>,
    pub base_revision: i64,
    pub anchor_from: i64,
    pub anchor_to: i64,
    pub original_text: String,
    pub replacement_text: String,
    pub context_hash: String,
    pub status: SuggestionStatus,
    pub created_at: Timestamp,
    pub applied_at: Option<Timestamp>,
}

fn map_suggestion(row: &Row<'_>) -> rusqlite::Result<AiSuggestion> {
    Ok(AiSuggestion {
        id: row.get("id")?,
        page_id: row.get("page_id")?,
        annotation_id: row.get("annotation_id")?,
        base_revision: row.get("base_revision")?,
        anchor_from: row.get("anchor_from")?,
        anchor_to: row.get("anchor_to")?,
        original_text: row.get("original_text")?,
        replacement_text: row.get("replacement_text")?,
        context_hash: row.get("context_hash")?,
        status: SuggestionStatus::parse(&row.get::<_, String>("status")?),
        created_at: row.get("created_at")?,
        applied_at: row.get("applied_at")?,
    })
}

/// Records a proposed edit, together with everything needed to decide later
/// whether applying it is still safe.
pub fn create_suggestion(
    conn: &Connection,
    page_id: PageId,
    annotation_id: Option<AnnotationId>,
    from: i64,
    to: i64,
    replacement: &str,
) -> Result<AiSuggestion> {
    let page = super::pages::get(conn, page_id)?;
    let chars: Vec<char> = page.plain_text.chars().collect();
    let start = from.clamp(0, chars.len() as i64) as usize;
    let end = to.clamp(start as i64, chars.len() as i64) as usize;

    let original: String = chars[start..end].iter().collect();
    if original.is_empty() {
        return Err(AppError::invalid("There is no text there to replace."));
    }

    let id = SuggestionId::new();
    conn.execute(
        "INSERT INTO ai_suggestions (id, page_id, annotation_id, base_revision,
                                     anchor_from, anchor_to, original_text,
                                     replacement_text, context_hash, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10)",
        params![
            id,
            page_id,
            annotation_id,
            page.revision_number,
            start as i64,
            end as i64,
            original,
            replacement,
            hash(&original),
            now()
        ],
    )?;

    get_suggestion(conn, id)
}

pub fn get_suggestion(conn: &Connection, id: SuggestionId) -> Result<AiSuggestion> {
    conn.query_row(
        "SELECT * FROM ai_suggestions WHERE id = ?1",
        params![id],
        map_suggestion,
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => AppError::not_found("suggestion"),
        other => other.into(),
    })
}

pub fn suggestions_for_page(conn: &Connection, page_id: PageId) -> Result<Vec<AiSuggestion>> {
    let mut statement =
        conn.prepare("SELECT * FROM ai_suggestions WHERE page_id = ?1 ORDER BY created_at DESC")?;
    let rows = statement
        .query_map(params![page_id], map_suggestion)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn set_suggestion_status(
    conn: &Connection,
    id: SuggestionId,
    status: SuggestionStatus,
) -> Result<AiSuggestion> {
    let applied_at = (status == SuggestionStatus::Applied).then(now);
    let changed = conn.execute(
        "UPDATE ai_suggestions SET status = ?2, applied_at = ?3 WHERE id = ?1",
        params![id, status.as_str(), applied_at],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("suggestion"));
    }
    get_suggestion(conn, id)
}

#[cfg(test)]
mod tests;
