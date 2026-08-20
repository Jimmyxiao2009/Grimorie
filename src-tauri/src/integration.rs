//! Whole-flow tests.
//!
//! The per-module tests check one behaviour at a time. These follow a
//! manuscript through the sequence a writer actually performs — create, write,
//! close, reopen — against a real database file, because "close and reopen
//! without data loss" is a promise that cannot be verified one unit at a time.
//!
//! They live inside the crate rather than in `tests/` so they can reach the
//! repositories directly without widening the crate's public surface.

#![cfg(test)]

use serde_json::json;
use tempfile::TempDir;

use crate::database::Database;
use crate::domain::settings::AppSettings;
use crate::repositories::{self, volumes::Shelf};

fn document(text: &str) -> serde_json::Value {
    json!({
        "type": "doc",
        "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]
    })
}

#[test]
fn a_manuscript_survives_closing_and_reopening_the_application() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("grimoire.db");

    let volume_id;
    let page_id;

    // --- First session: write something. ---
    {
        let db = Database::open(&path).unwrap();
        let conn = db.get().unwrap();

        let volume =
            repositories::volumes::create(&conn, "The Salt Road", Some("A novel"), None).unwrap();
        let chapter = repositories::chapters::create(&conn, volume.id, "Chapter I").unwrap();
        let page = repositories::pages::create(&conn, chapter.id, "The Crows").unwrap();

        repositories::pages::save_document(
            &conn,
            page.id,
            document("The road had been salt once, or so the carters said."),
        )
        .unwrap();

        volume_id = volume.id;
        page_id = page.id;
    }
    // Everything is dropped here: pool closed, WAL checkpointed on close.

    // --- Second session: it is all still there. ---
    {
        let db = Database::open(&path).unwrap();
        let conn = db.get().unwrap();

        let outline = repositories::outline(&conn, volume_id).unwrap();
        assert_eq!(outline.volume.title, "The Salt Road");
        assert_eq!(outline.chapters.len(), 1);
        assert_eq!(outline.chapters[0].pages.len(), 1);

        let page = repositories::pages::get(&conn, page_id).unwrap();
        assert_eq!(
            page.plain_text,
            "The road had been salt once, or so the carters said."
        );
        // The road had been salt once or so the carters said — eleven words.
        assert_eq!(page.word_count, 11);
        assert_eq!(
            page.document["content"][0]["content"][0]["text"],
            json!(page.plain_text)
        );
    }
}

#[test]
fn a_full_manuscript_keeps_its_order_across_every_structural_change() {
    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();
    let conn = db.get().unwrap();

    let volume = repositories::volumes::create(&conn, "Worldbook", None, None).unwrap();
    let one = repositories::chapters::create(&conn, volume.id, "One").unwrap();
    let two = repositories::chapters::create(&conn, volume.id, "Two").unwrap();

    let a = repositories::pages::create(&conn, one.id, "A").unwrap();
    let b = repositories::pages::create(&conn, one.id, "B").unwrap();
    let c = repositories::pages::create(&conn, two.id, "C").unwrap();

    // Reorder chapters, move a page across, then delete one.
    repositories::chapters::reorder(&conn, volume.id, &[two.id, one.id]).unwrap();
    repositories::pages::move_to_chapter(&conn, a.id, two.id, Some(0)).unwrap();
    repositories::pages::delete(&conn, b.id).unwrap();

    let outline = repositories::outline(&conn, volume.id).unwrap();
    assert_eq!(
        outline
            .chapters
            .iter()
            .map(|c| c.chapter.title.as_str())
            .collect::<Vec<_>>(),
        ["Two", "One"]
    );
    assert_eq!(
        outline.chapters[0]
            .pages
            .iter()
            .map(|p| p.id)
            .collect::<Vec<_>>(),
        [a.id, c.id]
    );
    assert!(outline.chapters[1].pages.is_empty());

    // Positions are dense everywhere, which is what reordering depends on.
    for chapter in &outline.chapters {
        for (seat, page) in chapter.pages.iter().enumerate() {
            assert_eq!(page.position, seat as i64);
        }
    }
}

#[test]
fn deleting_a_volume_removes_every_trace_of_it_and_nothing_else() {
    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();
    let conn = db.get().unwrap();

    let doomed = repositories::volumes::create(&conn, "Doomed", None, None).unwrap();
    let kept = repositories::volumes::create(&conn, "Kept", None, None).unwrap();

    for volume in [doomed.id, kept.id] {
        let chapter = repositories::chapters::create(&conn, volume, "One").unwrap();
        let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();
        repositories::pages::save_document(&conn, page.id, document("words here")).unwrap();
    }

    repositories::volumes::delete(&conn, doomed.id).unwrap();

    let remaining = repositories::volumes::list(&conn, Shelf::All, None).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].volume.id, kept.id);
    // The surviving Volume still has its manuscript.
    assert_eq!(remaining[0].page_count, 1);
    assert_eq!(remaining[0].word_count, 2);

    let pages: i64 = conn
        .query_row("SELECT count(*) FROM pages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pages, 1, "the deleted Volume left pages behind");
}

#[test]
fn settings_and_manuscripts_share_a_file_without_disturbing_each_other() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("grimoire.db");

    {
        let db = Database::open(&path).unwrap();
        let conn = db.get().unwrap();
        repositories::volumes::create(&conn, "A", None, None).unwrap();
        repositories::settings::save(
            &conn,
            &AppSettings {
                theme: "night".into(),
                ..Default::default()
            },
        )
        .unwrap();
    }

    let db = Database::open(&path).unwrap();
    let conn = db.get().unwrap();
    assert_eq!(repositories::settings::load(&conn).unwrap().theme, "night");
    assert_eq!(
        repositories::volumes::list(&conn, Shelf::All, None)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_second_connection_sees_a_write_made_on_the_first() {
    // The pool hands out several connections; a save made through one must be
    // visible to the next read, which may well come from a different one.
    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();

    let volume = {
        let conn = db.get().unwrap();
        repositories::volumes::create(&conn, "Shared", None, None).unwrap()
    };
    let chapter = {
        let conn = db.get().unwrap();
        repositories::chapters::create(&conn, volume.id, "One").unwrap()
    };
    let page = {
        let conn = db.get().unwrap();
        repositories::pages::create(&conn, chapter.id, "First").unwrap()
    };
    {
        let conn = db.get().unwrap();
        repositories::pages::save_document(&conn, page.id, document("visible")).unwrap();
    }

    let conn = db.get().unwrap();
    assert_eq!(
        repositories::pages::get(&conn, page.id).unwrap().plain_text,
        "visible"
    );
}

#[test]
fn a_failed_transaction_leaves_the_manuscript_exactly_as_it_was() {
    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();

    let (volume, chapter) = {
        let conn = db.get().unwrap();
        let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
        let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
        repositories::pages::create(&conn, chapter.id, "Keep me").unwrap();
        (volume.id, chapter.id)
    };

    // A multi-step change that fails partway must not half-apply.
    let outcome = db.transaction(|tx| {
        repositories::pages::create(tx, chapter, "Doomed")?;
        repositories::chapters::rename(tx, chapter, "Renamed")?;
        // An empty title is refused by the domain rule.
        repositories::pages::rename(tx, repositories::pages::summaries(tx, chapter)?[0].id, "  ")
    });
    assert!(outcome.is_err());

    let conn = db.get().unwrap();
    let outline = repositories::outline(&conn, volume).unwrap();
    assert_eq!(
        outline.chapters[0].chapter.title, "One",
        "the rename should have rolled back"
    );
    assert_eq!(
        outline.chapters[0].pages.len(),
        1,
        "the added page should have rolled back"
    );
    assert_eq!(outline.chapters[0].pages[0].title, "Keep me");
}

#[test]
fn cjk_and_mixed_script_manuscripts_round_trip_intact() {
    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();
    let conn = db.get().unwrap();

    let volume = repositories::volumes::create(&conn, "手稿", None, None).unwrap();
    let chapter = repositories::chapters::create(&conn, volume.id, "第三章 — 风与灰烬").unwrap();
    let page = repositories::pages::create(&conn, chapter.id, "开端").unwrap();

    let text = "手稿属于用户。The manuscript belongs to the user.";
    let saved = repositories::pages::save_document(&conn, page.id, document(text)).unwrap();

    assert_eq!(saved.plain_text, text);
    // 手稿属于用户 (6) + 6 English words = 12.
    assert_eq!(saved.word_count, 12);
    assert_eq!(saved.character_count, text.chars().count() as i64);

    let reloaded = repositories::outline(&conn, volume.id).unwrap();
    assert_eq!(reloaded.chapters[0].chapter.title, "第三章 — 风与灰烬");
}
