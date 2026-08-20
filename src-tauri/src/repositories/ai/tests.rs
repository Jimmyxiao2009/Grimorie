use super::*;
use crate::database::testing::TempDatabase;
use crate::repositories;
use serde_json::json;

fn document(text: &str) -> serde_json::Value {
    json!({
        "type": "doc",
        "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]
    })
}

fn page_with(conn: &Connection, text: &str) -> PageId {
    let volume = repositories::volumes::create(conn, "A", None, None).unwrap();
    let chapter = repositories::chapters::create(conn, volume.id, "One").unwrap();
    let page = repositories::pages::create(conn, chapter.id, "First").unwrap();
    repositories::pages::save_document(conn, page.id, document(text)).unwrap();
    page.id
}

// --- Providers --------------------------------------------------------------

#[test]
fn a_provider_round_trips_without_ever_holding_a_key() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();

    let provider = upsert_provider(
        &conn,
        None,
        "My server",
        "https://api.example.test/v1",
        "a-model",
        0.7,
        None,
        16_000,
    )
    .unwrap();

    let loaded = get_provider(&conn, provider.id).unwrap();
    assert_eq!(loaded.name, "My server");
    assert_eq!(loaded.model, "a-model");
    // Only a reference is stored. Nothing here is or contains a secret.
    assert!(loaded.credential_ref.starts_with("provider."));
    assert!(
        !loaded.has_key,
        "has_key is decided by the credential store, not the row"
    );
}

#[test]
fn a_provider_address_must_be_a_url() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();

    let error =
        upsert_provider(&conn, None, "x", "api.example.test", "m", 0.7, None, 8000).unwrap_err();
    assert!(error.message.contains("http://"), "{}", error.message);

    assert!(upsert_provider(&conn, None, "x", "   ", "m", 0.7, None, 8000).is_err());
    assert!(
        upsert_provider(
            &conn,
            None,
            "x",
            "https://ok.test/v1",
            "  ",
            0.7,
            None,
            8000
        )
        .is_err()
    );
}

#[test]
fn provider_settings_are_clamped_to_usable_ranges() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();

    let provider =
        upsert_provider(&conn, None, "x", "https://ok.test/v1", "m", 99.0, None, 10).unwrap();

    assert!(provider.temperature <= 2.0);
    assert!(provider.context_length >= 1_000);
}

#[test]
fn updating_a_provider_keeps_its_credential_reference() {
    // Changing the model must not orphan the saved key.
    let db = TempDatabase::open();
    let conn = db.get().unwrap();

    let created = upsert_provider(
        &conn,
        None,
        "x",
        "https://ok.test/v1",
        "old",
        0.7,
        None,
        8000,
    )
    .unwrap();
    let updated = upsert_provider(
        &conn,
        Some(created.id),
        "x",
        "https://ok.test/v1",
        "new",
        0.7,
        None,
        8000,
    )
    .unwrap();

    assert_eq!(updated.credential_ref, created.credential_ref);
    assert_eq!(updated.model, "new");
}

#[test]
fn deleting_a_provider_reports_the_credential_to_remove() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let provider =
        upsert_provider(&conn, None, "x", "https://ok.test/v1", "m", 0.7, None, 8000).unwrap();

    let reference = delete_provider(&conn, provider.id).unwrap();
    assert_eq!(reference, provider.credential_ref);
    assert!(get_provider(&conn, provider.id).is_err());
}

#[test]
fn asking_for_a_provider_before_one_exists_says_where_to_add_it() {
    let db = TempDatabase::open();
    let error = active_provider(&db.get().unwrap()).unwrap_err();
    assert!(error.message.contains("Settings"), "{}", error.message);
}

// --- Profiles ---------------------------------------------------------------

#[test]
fn the_shipped_profiles_are_created_on_first_run() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();

    let created = ensure_builtin_profiles(&conn).unwrap();
    assert!(created >= 6);

    let profiles = list_profiles(&conn).unwrap();
    assert!(profiles.iter().any(|p| p.name == "Editor"));
    assert!(profiles.iter().any(|p| p.name == "Continuity Reviewer"));
    assert!(profiles.iter().all(|p| p.builtin));
}

#[test]
fn a_profile_the_writer_deleted_does_not_come_back() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();

    ensure_builtin_profiles(&conn).unwrap();
    let editor = list_profiles(&conn)
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Editor")
        .unwrap();
    delete_profile(&conn, editor.id).unwrap();

    // Startup runs this again; it must not resurrect what was removed.
    assert_eq!(ensure_builtin_profiles(&conn).unwrap(), 0);
    assert!(
        !list_profiles(&conn)
            .unwrap()
            .iter()
            .any(|p| p.name == "Editor")
    );
}

#[test]
fn a_builtin_profile_can_be_rewritten() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    ensure_builtin_profiles(&conn).unwrap();

    let editor = list_profiles(&conn).unwrap().into_iter().next().unwrap();
    let updated = update_profile(
        &conn,
        editor.id,
        "My Editor",
        "Mine.",
        "Read as I would.",
        ContextPolicy::Page,
        Some(0.3),
    )
    .unwrap();

    assert_eq!(updated.name, "My Editor");
    assert_eq!(updated.context_policy, ContextPolicy::Page);
    assert_eq!(updated.temperature, Some(0.3));
}

#[test]
fn a_profile_needs_a_name_and_instructions() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    ensure_builtin_profiles(&conn).unwrap();
    let profile = list_profiles(&conn).unwrap().into_iter().next().unwrap();

    assert!(
        update_profile(
            &conn,
            profile.id,
            "  ",
            "",
            "prompt",
            ContextPolicy::Selection,
            None
        )
        .is_err()
    );
    assert!(
        update_profile(
            &conn,
            profile.id,
            "Name",
            "",
            "   ",
            ContextPolicy::Selection,
            None
        )
        .is_err()
    );
}

// --- Suggestions ------------------------------------------------------------

#[test]
fn a_suggestion_records_what_it_expected_to_replace() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let page_id = page_with(&conn, "The road had been salt once.");

    let suggestion = create_suggestion(&conn, page_id, None, 18, 22, "brine").unwrap();

    assert_eq!(suggestion.original_text, "salt");
    assert_eq!(suggestion.replacement_text, "brine");
    assert_eq!(suggestion.context_hash, hash("salt"));
    assert_eq!(suggestion.status, SuggestionStatus::Pending);
    // The revision it was computed against, so staleness can be judged later.
    assert_eq!(
        suggestion.base_revision,
        repositories::pages::get(&conn, page_id)
            .unwrap()
            .revision_number
    );
}

#[test]
fn a_suggestion_over_no_text_is_refused() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let page_id = page_with(&conn, "Some text.");

    assert!(create_suggestion(&conn, page_id, None, 3, 3, "x").is_err());
}

#[test]
fn suggestion_offsets_are_clamped_to_the_page() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let page_id = page_with(&conn, "Short.");

    let suggestion = create_suggestion(&conn, page_id, None, 0, 9_999, "x").unwrap();
    assert_eq!(suggestion.original_text, "Short.");
}

#[test]
fn suggestion_status_transitions_are_recorded() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let page_id = page_with(&conn, "The road had been salt once.");
    let suggestion = create_suggestion(&conn, page_id, None, 18, 22, "brine").unwrap();

    let applied = set_suggestion_status(&conn, suggestion.id, SuggestionStatus::Applied).unwrap();
    assert_eq!(applied.status, SuggestionStatus::Applied);
    assert!(applied.applied_at.is_some());

    let dismissed =
        set_suggestion_status(&conn, suggestion.id, SuggestionStatus::Dismissed).unwrap();
    assert!(
        dismissed.applied_at.is_none(),
        "only applying records a time"
    );
}

#[test]
fn deleting_a_page_takes_its_suggestions_with_it() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let page_id = page_with(&conn, "The road had been salt once.");
    create_suggestion(&conn, page_id, None, 18, 22, "brine").unwrap();

    repositories::pages::delete(&conn, page_id).unwrap();

    let left: i64 = conn
        .query_row("SELECT count(*) FROM ai_suggestions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn a_suggestion_belongs_to_the_margin_note_that_produced_it() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let page_id = page_with(&conn, "The road had been salt once.");

    let note = repositories::annotations::create_for_page(
        &conn,
        page_id,
        crate::domain::annotation::AnnotationKind::AiSuggestion,
        "Tightened.",
    )
    .unwrap();
    let suggestion = create_suggestion(&conn, page_id, Some(note.id), 18, 22, "brine").unwrap();

    assert_eq!(suggestion.annotation_id, Some(note.id));

    // Removing the note removes the proposal with it: a suggestion with no
    // note is unreachable in the UI.
    repositories::annotations::delete(&conn, note.id).unwrap();
    assert!(get_suggestion(&conn, suggestion.id).is_err());
}
