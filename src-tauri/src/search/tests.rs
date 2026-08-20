use super::query::{build_match, snippet_around};
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

/// A small library: one Volume, one Chapter, and the given pages.
fn library(conn: &Connection, pages: &[(&str, &str)]) -> VolumeId {
    let volume =
        repositories::volumes::create(conn, "The Salt Road", Some("A novel"), None).unwrap();
    let chapter = repositories::chapters::create(conn, volume.id, "Chapter I — Leaving").unwrap();
    for (title, body) in pages {
        let page = repositories::pages::create(conn, chapter.id, title).unwrap();
        repositories::save_page(conn, page.id, document(body)).unwrap();
    }
    volume.id
}

// --- Query construction -----------------------------------------------------

#[test]
fn an_empty_query_matches_nothing_rather_than_everything() {
    assert_eq!(build_match(""), None);
    assert_eq!(build_match("   "), None);
    assert_eq!(build_match("!!! ..."), None);
}

#[test]
fn fts_syntax_in_a_query_is_treated_as_words() {
    // A writer searching for these wants the words, not boolean operators.
    let expression = build_match("AND OR NEAR").unwrap();
    assert!(expression.contains("\"AND\""), "{expression}");
    assert!(expression.contains("\"OR\""), "{expression}");

    // Quotes and colons must not be able to break out of the term.
    let tricky = build_match("say \"hello\" now").unwrap();
    assert!(tricky.contains("\"\"hello\"\""), "{tricky}");
}

#[test]
fn the_last_word_gets_a_prefix_wildcard_so_results_appear_while_typing() {
    let expression = build_match("salt roa").unwrap();
    assert!(expression.ends_with("\"roa\"*"), "{expression}");
    assert!(expression.contains("\"salt\" AND"), "{expression}");
}

#[test]
fn a_single_cjk_character_is_not_given_a_wildcard() {
    // It is already a whole word; a prefix match would hit a large fraction of
    // the manuscript.
    let expression = build_match("手").unwrap();
    assert_eq!(expression, "\"手\"");
}

// --- Searching --------------------------------------------------------------

#[test]
fn finds_a_page_by_its_body_text() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(
        &conn,
        &[(
            "The Crows",
            "The road had been salt once, or so the carters said.",
        )],
    );

    let hits = search(&conn, "carters", None, 20).unwrap();
    let page = hits
        .iter()
        .find(|h| h.kind == EntityKind::Page)
        .expect("page hit");
    assert_eq!(page.title, "The Crows");
    assert!(page.snippet.contains("carters"));
    assert!(!page.highlights.is_empty());
    assert_eq!(page.path, vec!["The Salt Road", "Chapter I — Leaving"]);
}

#[test]
fn finds_a_page_by_its_title() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(&conn, &[("The Ledger", "Names, mostly.")]);

    let hits = search(&conn, "ledger", None, 20).unwrap();
    assert!(hits.iter().any(|h| h.title == "The Ledger"));
}

#[test]
fn a_title_match_outranks_a_body_match() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(
        &conn,
        &[
            (
                "Somewhere else",
                "The crows were mentioned here in passing only.",
            ),
            ("The Crows", "Nothing relevant in this body at all."),
        ],
    );

    let hits = search(&conn, "crows", None, 20).unwrap();
    let pages: Vec<&SearchHit> = hits.iter().filter(|h| h.kind == EntityKind::Page).collect();
    assert_eq!(pages.first().map(|h| h.title.as_str()), Some("The Crows"));
}

#[test]
fn finds_cjk_text_by_a_substring_of_it() {
    // The reason the index stores a segmented form at all.
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(
        &conn,
        &[("开端", "手稿属于用户。The manuscript belongs to the user.")],
    );

    let hits = search(&conn, "属于", None, 20).unwrap();
    assert!(!hits.is_empty(), "a two-character CJK query found nothing");

    let page = hits
        .iter()
        .find(|h| h.kind == EntityKind::Page)
        .expect("page hit");
    // The snippet reads as written, without spaces between the characters.
    assert!(
        page.snippet.contains("手稿属于用户"),
        "got {:?}",
        page.snippet
    );
}

#[test]
fn finds_latin_text_that_sits_beside_cjk() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(
        &conn,
        &[("开端", "手稿属于用户。The manuscript belongs to the user.")],
    );

    let hits = search(&conn, "manuscript", None, 20).unwrap();
    assert!(hits.iter().any(|h| h.kind == EntityKind::Page));
}

#[test]
fn all_terms_must_match() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(&conn, &[("One", "salt and wind"), ("Two", "salt and ash")]);

    let hits = search(&conn, "salt ash", None, 20).unwrap();
    let titles: Vec<&str> = hits
        .iter()
        .filter(|h| h.kind == EntityKind::Page)
        .map(|h| h.title.as_str())
        .collect();
    assert_eq!(titles, ["Two"]);
}

#[test]
fn a_search_can_be_confined_to_one_volume() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let first = library(&conn, &[("A", "a distinctive phrase")]);
    library(&conn, &[("B", "a distinctive phrase")]);

    let everywhere = search(&conn, "distinctive", None, 20).unwrap();
    assert_eq!(
        everywhere
            .iter()
            .filter(|h| h.kind == EntityKind::Page)
            .count(),
        2
    );

    let confined = search(&conn, "distinctive", Some(&first.to_string()), 20).unwrap();
    assert_eq!(
        confined
            .iter()
            .filter(|h| h.kind == EntityKind::Page)
            .count(),
        1
    );
}

#[test]
fn annotations_are_searchable_and_name_the_page_to_open() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(&conn, &[("The Crows", "The road had been salt once.")]);

    let page_id = {
        let volume = repositories::volumes::list(&conn, repositories::volumes::Shelf::All, None)
            .unwrap()[0]
            .volume
            .id;
        repositories::pages::summaries_for_volume(&conn, volume).unwrap()[0].id
    };
    repositories::annotations::create_for_page(
        &conn,
        page_id,
        crate::domain::annotation::AnnotationKind::Note,
        "Reconsider the opening cadence.",
    )
    .unwrap();

    let hits = search(&conn, "cadence", None, 20).unwrap();
    let note = hits
        .iter()
        .find(|h| h.kind == EntityKind::Annotation)
        .expect("annotation hit");
    assert_eq!(note.page_id.as_deref(), Some(page_id.to_string().as_str()));
    assert!(note.snippet.contains("cadence"));
}

// --- Index maintenance ------------------------------------------------------

#[test]
fn editing_a_page_updates_what_it_matches() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(&conn, &[("One", "the original wording")]);

    let page_id = {
        let volume = repositories::volumes::list(&conn, repositories::volumes::Shelf::All, None)
            .unwrap()[0]
            .volume
            .id;
        repositories::pages::summaries_for_volume(&conn, volume).unwrap()[0].id
    };
    repositories::save_page(&conn, page_id, document("a completely different sentence")).unwrap();

    assert!(search(&conn, "original", None, 20).unwrap().is_empty());
    assert!(!search(&conn, "different", None, 20).unwrap().is_empty());
}

#[test]
fn re_indexing_a_page_does_not_leave_duplicate_entries() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(&conn, &[("One", "persistent phrase")]);

    let page_id = {
        let volume = repositories::volumes::list(&conn, repositories::volumes::Shelf::All, None)
            .unwrap()[0]
            .volume
            .id;
        repositories::pages::summaries_for_volume(&conn, volume).unwrap()[0].id
    };
    for _ in 0..5 {
        repositories::save_page(&conn, page_id, document("persistent phrase")).unwrap();
    }

    let hits = search(&conn, "persistent", None, 20).unwrap();
    assert_eq!(
        hits.iter().filter(|h| h.kind == EntityKind::Page).count(),
        1
    );
}

#[test]
fn a_deleted_page_stops_appearing_in_results() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(&conn, &[("Doomed", "a phrase worth finding")]);

    let page_id = {
        let volume = repositories::volumes::list(&conn, repositories::volumes::Shelf::All, None)
            .unwrap()[0]
            .volume
            .id;
        repositories::pages::summaries_for_volume(&conn, volume).unwrap()[0].id
    };
    repositories::pages::delete(&conn, page_id).unwrap();

    let hits = search(&conn, "finding", None, 20).unwrap();
    assert!(hits.iter().all(|h| h.kind != EntityKind::Page), "{hits:?}");
}

#[test]
fn deleting_a_volume_clears_everything_it_contributed() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    let volume_id = library(
        &conn,
        &[("One", "vanishing text"), ("Two", "vanishing text")],
    );

    repositories::volumes::delete(&conn, volume_id).unwrap();

    assert!(search(&conn, "vanishing", None, 20).unwrap().is_empty());
    let orphans: i64 = conn
        .query_row("SELECT count(*) FROM search_rows", [], |row| row.get(0))
        .unwrap();
    assert_eq!(orphans, 0);
}

#[test]
fn rebuilding_reproduces_the_same_results() {
    let db = TempDatabase::open();
    let conn = db.get().unwrap();
    library(&conn, &[("The Crows", "The road had been salt once.")]);

    let before = search(&conn, "salt", None, 20).unwrap().len();
    rebuild(&conn).unwrap();
    let after = search(&conn, "salt", None, 20).unwrap().len();

    assert_eq!(before, after);
    assert!(after > 0);
}

// --- Snippets ---------------------------------------------------------------

#[test]
fn a_snippet_centres_on_the_match_and_marks_it() {
    let text = "A ".repeat(200) + "needle" + &" B".repeat(200);
    let terms = vec!["needle".to_string()];
    let (snippet, highlights) = snippet_around(&text, &terms, 80);

    assert!(snippet.contains("needle"));
    assert!(
        snippet.chars().count() <= 80 + 50,
        "snippet was {}",
        snippet.chars().count()
    );
    assert_eq!(highlights.len(), 1);

    let (from, to) = highlights[0];
    let marked: String = snippet
        .chars()
        .skip(from as usize)
        .take((to - from) as usize)
        .collect();
    assert_eq!(marked, "needle");
}

#[test]
fn snippet_highlights_land_on_the_right_characters_with_cjk() {
    let text = "第三章。手稿属于用户。风与灰烬。";
    let terms = vec!["属于".to_string()];
    let (snippet, highlights) = snippet_around(text, &terms, 60);

    assert_eq!(highlights.len(), 1);
    let (from, to) = highlights[0];
    let marked: String = snippet
        .chars()
        .skip(from as usize)
        .take((to - from) as usize)
        .collect();
    assert_eq!(marked, "属于");
}

#[test]
fn short_text_is_returned_whole_without_ellipses() {
    let (snippet, _) = snippet_around("A short line.", &["short".to_string()], 180);
    assert_eq!(snippet, "A short line.");
}

#[test]
fn overlapping_matches_produce_one_mark_each() {
    // "salt" and "salted" both match the same word; the ranges must not nest.
    let text = "the salted road";
    let terms = vec!["salt".to_string(), "salted".to_string()];
    let (_, highlights) = snippet_around(text, &terms, 180);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0], (4, 10));
}

#[test]
fn a_snippet_of_text_with_no_match_still_reads_from_the_start() {
    let (snippet, highlights) = snippet_around("Some text without the term.", &[], 180);
    assert!(snippet.starts_with("Some text"));
    assert!(highlights.is_empty());
}

#[test]
fn empty_text_produces_an_empty_snippet() {
    let (snippet, highlights) = snippet_around("", &["x".to_string()], 180);
    assert!(snippet.is_empty());
    assert!(highlights.is_empty());
}

#[test]
fn a_library_that_predates_search_gets_an_index_on_next_open() {
    // The upgrade path: adding the table does not fill it, and without a
    // rebuild a writer would upgrade and find their own manuscript
    // unsearchable until every Page had been re-saved.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grimoire.db");

    {
        let db = crate::database::Database::open(&path).unwrap();
        let conn = db.get().unwrap();
        library(&conn, &[("The Crows", "The road had been salt once.")]);

        // Simulate the state an older library arrives in: content present,
        // index empty.
        conn.execute("DELETE FROM search_index", []).unwrap();
        conn.execute("DELETE FROM search_rows", []).unwrap();
        assert!(search(&conn, "salt", None, 20).unwrap().is_empty());
    }

    let reopened = crate::database::Database::open(&path).unwrap();
    let conn = reopened.get().unwrap();
    assert!(
        !search(&conn, "salt", None, 20).unwrap().is_empty(),
        "reopening should have rebuilt the index"
    );
}

#[test]
fn an_empty_library_does_not_trigger_a_rebuild_on_every_launch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grimoire.db");
    // Opening twice with nothing in it must be uneventful.
    drop(crate::database::Database::open(&path).unwrap());
    let db = crate::database::Database::open(&path).unwrap();
    let conn = db.get().unwrap();
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM search_rows", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
