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

#[test]
fn applying_a_suggestion_is_safe_undoable_and_refuses_when_the_text_moved_on() {
    use crate::ai::apply;
    use crate::repositories::ai::SuggestionStatus;

    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();
    let conn = db.get().unwrap();

    let volume = repositories::volumes::create(&conn, "The Salt Road", None, None).unwrap();
    let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
    let page = repositories::pages::create(&conn, chapter.id, "The Crows").unwrap();
    repositories::save_page(&conn, page.id, document("The road had been salt once.")).unwrap();

    let text = repositories::pages::get(&conn, page.id).unwrap().plain_text;
    let from = text.find("salt").unwrap() as i64;
    let to = from + 4;

    // --- A suggestion applied against unchanged text lands exactly. ---
    let suggestion =
        repositories::ai::create_suggestion(&conn, page.id, None, from, to, "brine").unwrap();

    let verdict = apply::validate(
        &text,
        &suggestion.original_text,
        &suggestion.context_hash,
        suggestion.anchor_from,
        suggestion.anchor_to,
    );
    let apply::Validation::Ready { from, to } = verdict else {
        panic!("expected the suggestion to be applicable, got {verdict:?}");
    };

    let before = repositories::pages::get(&conn, page.id).unwrap();
    repositories::revisions::capture(
        &conn,
        page.id,
        repositories::revisions::RevisionReason::BeforeAi,
    )
    .unwrap();
    let spliced = apply::apply(&before.document, from, to, "brine").unwrap();
    let saved = repositories::save_page(&conn, page.id, spliced).unwrap();

    assert_eq!(saved.plain_text, "The road had been brine once.");

    // Applying is undoable: the replaced text is in history.
    let history = repositories::revisions::list(&conn, page.id).unwrap();
    assert!(
        history
            .iter()
            .any(|r| r.reason == repositories::revisions::RevisionReason::BeforeAi),
        "no before-AI snapshot was taken"
    );
    assert!(history.iter().any(|r| r.preview.contains("salt once")));

    // --- A second suggestion, made stale by an edit, is refused. ---
    let stale =
        repositories::ai::create_suggestion(&conn, page.id, None, 18, 23, "seawater").unwrap();
    assert_eq!(stale.original_text, "brine");

    repositories::save_page(&conn, page.id, document("Something else entirely now.")).unwrap();
    let current = repositories::pages::get(&conn, page.id).unwrap();

    let verdict = apply::validate(
        &current.plain_text,
        &stale.original_text,
        &stale.context_hash,
        stale.anchor_from,
        stale.anchor_to,
    );
    assert!(
        matches!(verdict, apply::Validation::Stale { .. }),
        "a suggestion whose text is gone must be refused, got {verdict:?}"
    );

    repositories::ai::set_suggestion_status(&conn, stale.id, SuggestionStatus::Stale).unwrap();
    assert_eq!(
        repositories::ai::get_suggestion(&conn, stale.id)
            .unwrap()
            .status,
        SuggestionStatus::Stale
    );
    // And the manuscript is untouched by the refusal.
    assert_eq!(
        repositories::pages::get(&conn, page.id).unwrap().plain_text,
        "Something else entirely now."
    );
}

#[test]
fn an_ai_note_and_its_suggestion_are_created_together_or_not_at_all() {
    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();

    let (page_id, _) = {
        let conn = db.get().unwrap();
        let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
        let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
        let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();
        repositories::save_page(&conn, page.id, document("The road had been salt once.")).unwrap();
        (page.id, volume.id)
    };

    // A transaction that fails after creating the note must leave neither.
    let outcome = db.transaction(|tx| {
        repositories::annotations::create_anchored(
            tx,
            page_id,
            crate::domain::annotation::AnnotationKind::AiSuggestion,
            "Tightened.",
            18,
            22,
        )?;
        // An impossible suggestion: no text in the range.
        repositories::ai::create_suggestion(tx, page_id, None, 5, 5, "x")
    });
    assert!(outcome.is_err());

    let conn = db.get().unwrap();
    assert!(
        repositories::annotations::list(&conn, page_id)
            .unwrap()
            .is_empty(),
        "the note should have rolled back with the suggestion"
    );
    assert!(
        repositories::ai::suggestions_for_page(&conn, page_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn handwriting_survives_closing_and_reopening_the_application() {
    use crate::domain::ink::{InkPoint, InkStroke, InkTool};

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("grimoire.db");

    let page_id;
    let annotation_id;
    let first_stroke_id;

    // --- First session: write a handwritten note. ---
    {
        let db = Database::open(&path).unwrap();
        let conn = db.get().unwrap();

        let volume = repositories::volumes::create(&conn, "Notes", None, None).unwrap();
        let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
        let page = repositories::pages::create(&conn, chapter.id, "Margin").unwrap();
        page_id = page.id;

        let annotation = repositories::ink::create_ink_annotation(&conn, page_id).unwrap();
        annotation_id = annotation.id;

        let stroke = InkStroke::new(
            InkTool::Pen,
            "ink-primary",
            2.0,
            vec![
                InkPoint::new(0.1, 10.0),
                InkPoint::new(0.5, 14.0),
                InkPoint::new(0.9, 10.0),
            ],
        );
        first_stroke_id = stroke.id;
        repositories::ink::add_strokes(&conn, annotation.id, &[stroke]).unwrap();
    }
    // Pool closed, WAL checkpointed — the app is "shut down".

    // --- Second session: the handwriting is exactly where it was. ---
    {
        let db = Database::open(&path).unwrap();
        let conn = db.get().unwrap();

        let notes = repositories::ink::strokes_for_page(&conn, page_id).unwrap();
        assert_eq!(notes.len(), 1, "the ink note survived the restart");
        let (read_annotation, strokes) = &notes[0];
        assert_eq!(*read_annotation, annotation_id);
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].id, first_stroke_id);
        // Coordinates are byte-for-byte what was written — no drift.
        assert_eq!(strokes[0].points[0], InkPoint::new(0.1, 10.0));
        assert_eq!(strokes[0].points[2], InkPoint::new(0.9, 10.0));
        assert_eq!(strokes[0].tool, InkTool::Pen);
    }
}

#[test]
fn ink_and_text_annotations_coexist_and_ink_survives_page_navigation() {
    use crate::domain::annotation::AnnotationKind;
    use crate::domain::ink::{InkPoint, InkStroke, InkTool};

    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();
    let conn = db.get().unwrap();

    let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
    let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
    let page_a = repositories::pages::create(&conn, chapter.id, "A").unwrap();
    let page_b = repositories::pages::create(&conn, chapter.id, "B").unwrap();

    // Page A: a text note and an ink note with three strokes.
    repositories::annotations::create_for_page(&conn, page_a.id, AnnotationKind::Note, "A thought")
        .unwrap();
    let ink = repositories::ink::create_ink_annotation(&conn, page_a.id).unwrap();
    let strokes: Vec<InkStroke> = (0..3)
        .map(|i| {
            InkStroke::new(
                InkTool::Pen,
                "ink-primary",
                2.0,
                vec![
                    InkPoint::new(0.1, i as f32 * 10.0),
                    InkPoint::new(0.5, i as f32 * 10.0 + 4.0),
                ],
            )
        })
        .collect();
    repositories::ink::add_strokes(&conn, ink.id, &strokes).unwrap();

    // Both kinds are on the page together.
    let all = repositories::annotations::list(&conn, page_a.id).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|a| a.kind == AnnotationKind::Note));
    assert!(all.iter().any(|a| a.kind == AnnotationKind::Ink));

    // Page B has no ink.
    assert!(
        repositories::ink::strokes_for_page(&conn, page_b.id)
            .unwrap()
            .is_empty()
    );

    // "Navigate back" to Page A: the three strokes are all still there, intact.
    let notes = repositories::ink::strokes_for_page(&conn, page_a.id).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].1.len(), 3);
    // Drawing order is preserved across the round trip.
    assert_eq!(notes[0].1[0].points[0].y, 0.0);
    assert_eq!(notes[0].1[2].points[0].y, 20.0);
}

#[test]
fn erasing_a_stroke_persists_and_leaves_the_rest() {
    use crate::domain::ink::{InkPoint, InkStroke, InkTool};

    let dir = TempDir::new().unwrap();
    let db = Database::open(dir.path().join("grimoire.db")).unwrap();
    let conn = db.get().unwrap();

    let volume = repositories::volumes::create(&conn, "A", None, None).unwrap();
    let chapter = repositories::chapters::create(&conn, volume.id, "One").unwrap();
    let page = repositories::pages::create(&conn, chapter.id, "First").unwrap();

    let ink = repositories::ink::create_ink_annotation(&conn, page.id).unwrap();
    let s1 = InkStroke::new(
        InkTool::Pen,
        "ink-primary",
        2.0,
        vec![InkPoint::new(0.1, 0.0)],
    );
    let s2 = InkStroke::new(
        InkTool::Pen,
        "ink-primary",
        2.0,
        vec![InkPoint::new(0.5, 0.0)],
    );
    let s3 = InkStroke::new(
        InkTool::Pen,
        "ink-primary",
        2.0,
        vec![InkPoint::new(0.9, 0.0)],
    );
    repositories::ink::add_strokes(&conn, ink.id, &[s1, s2.clone(), s3]).unwrap();

    // Erase the middle stroke.
    repositories::ink::delete_stroke(&conn, s2.id).unwrap();

    let notes = repositories::ink::strokes_for_page(&conn, page.id).unwrap();
    assert_eq!(notes[0].1.len(), 2, "two strokes remain after erasing one");
    assert!(
        notes[0].1.iter().all(|s| s.id != s2.id),
        "the erased stroke is gone"
    );

    // And the erasure survives a reopen.
    drop(conn);
    let conn = db.get().unwrap();
    let notes = repositories::ink::strokes_for_page(&conn, page.id).unwrap();
    assert_eq!(notes[0].1.len(), 2);
}

// ---------------------------------------------------------------------------
// Ink intelligence — recognition lifecycle, end to end.
//
// These mirror the flow scenarios in the ink-intelligence spec: recognise a
// note and reload, invalidate on ink change, reject a stale response, prefer a
// manual correction, and find a handwritten transcript through search. They run
// against a real database file with the mock recognizer, so the persistence and
// staleness guarantees are verified, not just the in-memory queue.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod ink_recognition_flows {
    use super::*;
    use crate::ai::recognition_queue::{InkSnapshot, RecognitionBookkeeping};
    use crate::ai::recognizers::MockRecognizer;
    use crate::app_state::AppState;
    use crate::domain::ink::{InkPoint, InkStroke, InkTool};
    use crate::domain::ink_recognition::{
        InkRecognitionResult, InkRecognizer, RecognitionStatus, TranscriptSource,
    };
    use std::sync::Arc;
    use std::time::Duration;

    fn stroke(points: &[(f32, f32)]) -> InkStroke {
        InkStroke::new(
            InkTool::Pen,
            "ink-primary",
            2.0,
            points.iter().map(|(x, y)| InkPoint::new(*x, *y)).collect(),
        )
    }

    /// A page with text, so an anchored ink note has something to point at.
    fn page_with_text(conn: &rusqlite::Connection, text: &str) -> crate::domain::PageId {
        let volume = repositories::volumes::create(conn, "A", None, None).unwrap();
        let chapter = repositories::chapters::create(conn, volume.id, "One").unwrap();
        let page = repositories::pages::create(conn, chapter.id, "First").unwrap();
        repositories::pages::save_document(conn, page.id, document(text)).unwrap();
        page.id
    }

    fn ink_note(
        conn: &rusqlite::Connection,
        page_id: crate::domain::PageId,
    ) -> crate::domain::AnnotationId {
        repositories::ink::create_ink_annotation(conn, page_id)
            .unwrap()
            .id
    }

    fn snapshot(strokes: Vec<InkStroke>) -> InkSnapshot {
        InkSnapshot {
            strokes,
            surface_width: 300.0,
            png: None,
            width: 0,
            height: 0,
        }
    }

    async fn recognize(
        state: &Arc<AppState>,
        bookkeeping: &Arc<RecognitionBookkeeping>,
        annotation: crate::domain::AnnotationId,
        strokes: Vec<InkStroke>,
        transcript: &str,
    ) -> crate::error::Result<InkRecognitionResult> {
        let recognizer: Arc<dyn InkRecognizer> = Arc::new(MockRecognizer::always(transcript));
        bookkeeping
            .recognize_now(Arc::clone(state), recognizer, annotation, snapshot(strokes))
            .await
    }

    #[tokio::test]
    async fn flow1_recognition_persists_across_reload() {
        // create Ink Note → add strokes → run mock recognizer → persist transcript
        // → reload Page → transcript remains.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grimoire.db");
        let db = Database::open(&path).unwrap();
        let state = Arc::new(AppState::new(db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());

        let annotation = {
            let conn = db.get().unwrap();
            let page_id = page_with_text(&conn, "The road had been salt once.");
            let annotation = ink_note(&conn, page_id);
            let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0), (0.3, 5.0)])];
            repositories::ink::add_strokes(&conn, annotation, &strokes).unwrap();
            annotation
        };

        let result = recognize(
            &state,
            &bookkeeping,
            annotation,
            vec![stroke(&[(0.1, 5.0), (0.2, 6.0), (0.3, 5.0)])],
            "recognised transcript",
        )
        .await
        .unwrap();
        assert_eq!(result.text, "recognised transcript");

        // A fresh connection (a "reopen") sees the same transcript.
        drop(state);
        let conn = db.get().unwrap();
        let row = repositories::ink_recognition::get(&conn, annotation)
            .unwrap()
            .unwrap();
        assert_eq!(row.status, RecognitionStatus::Recognized);
        assert_eq!(
            row.recognized_text.as_deref(),
            Some("recognised transcript")
        );
    }

    #[tokio::test]
    async fn flow2_ink_changes_invalidate_and_reschedule() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grimoire.db");
        let db = Database::open(&path).unwrap();
        let state = Arc::new(AppState::new(db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());

        let annotation = {
            let conn = db.get().unwrap();
            let page_id = page_with_text(&conn, "Some text.");
            let annotation = ink_note(&conn, page_id);
            let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])];
            repositories::ink::add_strokes(&conn, annotation, &strokes).unwrap();
            annotation
        };

        // Recognise once.
        recognize(
            &state,
            &bookkeeping,
            annotation,
            vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])],
            "first",
        )
        .await
        .unwrap();
        {
            let conn = db.get().unwrap();
            assert_eq!(
                repositories::ink_recognition::get(&conn, annotation)
                    .unwrap()
                    .unwrap()
                    .status,
                RecognitionStatus::Recognized
            );
        }

        // Add a stroke: the ink changes, and the old recognition becomes stale.
        {
            let conn = db.get().unwrap();
            repositories::ink::add_strokes(
                &conn,
                annotation,
                &[stroke(&[(0.5, 50.0), (0.6, 51.0)])],
            )
            .unwrap();
            repositories::ink_recognition::invalidate(&conn, annotation).unwrap();
        }
        let row = {
            let conn = db.get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert_eq!(row.status, RecognitionStatus::Stale);
        // The old transcript is kept while a refresh is due.
        assert_eq!(row.recognized_text.as_deref(), Some("first"));
    }

    #[tokio::test]
    async fn recognised_ink_reports_itself_current_so_a_reschedule_costs_nothing() {
        // The contract `ink_recognize` relies on to skip needless model calls:
        // once a result is committed, the note reports itself current for
        // exactly the ink it was recognised from — and not for any other ink.
        // If this ever stopped holding, every schedule would become a paid
        // vision request whether or not the handwriting had changed.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grimoire.db");
        let db = Database::open(&path).unwrap();
        let state = Arc::new(AppState::new(db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());

        let strokes = vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])];
        let annotation = {
            let conn = db.get().unwrap();
            let page_id = page_with_text(&conn, "Some text.");
            let annotation = ink_note(&conn, page_id);
            repositories::ink::add_strokes(&conn, annotation, &strokes).unwrap();
            annotation
        };

        recognize(
            &state,
            &bookkeeping,
            annotation,
            strokes.clone(),
            "a transcript",
        )
        .await
        .unwrap();

        let conn = db.get().unwrap();
        let current_hash = crate::domain::ink_recognition::ink_content_hash(&strokes);
        assert!(
            repositories::ink_recognition::is_current_for(&conn, annotation, &current_hash)
                .unwrap(),
            "a freshly recognised note should report itself current"
        );

        // Different ink is not current, so a real change still schedules.
        let changed = vec![
            stroke(&[(0.1, 5.0), (0.2, 6.0)]),
            stroke(&[(0.7, 70.0), (0.8, 71.0)]),
        ];
        let changed_hash = crate::domain::ink_recognition::ink_content_hash(&changed);
        assert!(
            !repositories::ink_recognition::is_current_for(&conn, annotation, &changed_hash)
                .unwrap()
        );
    }

    #[tokio::test]
    async fn flow3_stale_response_does_not_overwrite_newer_ink() {
        // recognition request A starts → ink changes → request A returns →
        // result A must not overwrite the current state.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grimoire.db");
        let db = Database::open(&path).unwrap();
        let state = Arc::new(AppState::new(db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());

        let annotation = {
            let conn = db.get().unwrap();
            let page_id = page_with_text(&conn, "Some text.");
            let annotation = ink_note(&conn, page_id);
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
            annotation
        };

        // A slow recognizer, so the test can mutate the ink while it runs.
        let slow: Arc<dyn InkRecognizer> = Arc::new(MockRecognizer::responder(|_| {
            std::thread::sleep(Duration::from_millis(40));
            Ok(InkRecognitionResult {
                text: "stale answer".into(),
                confidence: None,
                language: None,
                provider: "mock".into(),
                model: "mock-1".into(),
            })
        }));

        let state_for_job = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            bookkeeping
                .recognize_now(
                    state_for_job,
                    slow,
                    annotation,
                    snapshot(vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])]),
                )
                .await
        });

        // Mutate the ink while recognition is in flight.
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        {
            let conn = db.get().unwrap();
            repositories::ink::add_strokes(
                &conn,
                annotation,
                &[stroke(&[(0.9, 90.0), (0.95, 91.0)])],
            )
            .unwrap();
        }

        let outcome = handle.await.unwrap();
        assert!(outcome.is_err(), "a stale result must be rejected");

        // No stale transcript was committed.
        let row = {
            let conn = db.get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert!(row.recognized_text.is_none());
        assert_eq!(row.status, RecognitionStatus::Stale);
    }

    #[tokio::test]
    async fn flow4_manual_correction_survives_reload_and_blocks_automatic() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grimoire.db");
        let db = Database::open(&path).unwrap();
        let state = Arc::new(AppState::new(db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());

        let annotation = {
            let conn = db.get().unwrap();
            let page_id = page_with_text(&conn, "Some text.");
            let annotation = ink_note(&conn, page_id);
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
            annotation
        };

        // Recognise, then the writer corrects by hand.
        recognize(
            &state,
            &bookkeeping,
            annotation,
            vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])],
            "machine",
        )
        .await
        .unwrap();
        {
            let conn = db.get().unwrap();
            repositories::ink_recognition::set_user_transcript(
                &conn,
                annotation,
                "writer's correction",
            )
            .unwrap();
        }

        // A later automatic recognition lands: it must not clobber the writer.
        recognize(
            &state,
            &bookkeeping,
            annotation,
            vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])],
            "machine again",
        )
        .await
        .unwrap();

        // The writer's version remains authoritative.
        let row = {
            let conn = db.get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert_eq!(row.recognized_text.as_deref(), Some("writer's correction"));
        assert_eq!(row.transcript_source, TranscriptSource::UserEdited);
    }

    #[tokio::test]
    async fn flow5_handwritten_transcript_is_searchable() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grimoire.db");
        let db = Database::open(&path).unwrap();
        let state = Arc::new(AppState::new(db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());

        let annotation = {
            let conn = db.get().unwrap();
            let page_id = page_with_text(&conn, "Some text.");
            let annotation = ink_note(&conn, page_id);
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
            annotation
        };

        // Recognise with a CJK transcript, then search for it.
        recognize(
            &state,
            &bookkeeping,
            annotation,
            vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])],
            "这里的转折太突然了",
        )
        .await
        .unwrap();

        let hits = {
            let conn = db.get().unwrap();
            crate::search::search(&conn, "转折", None, 10).unwrap()
        };
        assert!(
            hits.iter().any(|h| h.kind == crate::search::EntityKind::Ink
                && h.entity_id == annotation.to_string()),
            "handwritten transcript should be found in search"
        );
    }

    #[tokio::test]
    async fn convert_to_text_keeps_the_ink_and_creates_a_note() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grimoire.db");
        let db = Database::open(&path).unwrap();
        let state = Arc::new(AppState::new(db.clone()).unwrap());
        let bookkeeping = Arc::new(RecognitionBookkeeping::default());

        let (page_id, annotation) = {
            let conn = db.get().unwrap();
            let page_id = page_with_text(&conn, "Some text.");
            let annotation = ink_note(&conn, page_id);
            repositories::ink::add_strokes(&conn, annotation, &[stroke(&[(0.1, 5.0), (0.2, 6.0)])])
                .unwrap();
            (page_id, annotation)
        };

        recognize(
            &state,
            &bookkeeping,
            annotation,
            vec![stroke(&[(0.1, 5.0), (0.2, 6.0)])],
            "recognised transcript",
        )
        .await
        .unwrap();

        // Convert: keep the ink, create a text note, record lineage.
        let note_id = {
            let conn = db.get().unwrap();
            let transcript = repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .and_then(|r| r.transcript().map(str::to_string))
                .unwrap();
            let note = repositories::annotations::create_for_page(
                &conn,
                page_id,
                crate::domain::annotation::AnnotationKind::Note,
                &transcript,
            )
            .unwrap();
            conn.execute(
                "UPDATE annotations SET source_ink_annotation_id = ?2 WHERE id = ?1",
                rusqlite::params![note.id, annotation],
            )
            .unwrap();
            note.id
        };

        // The ink note still exists, with its strokes intact.
        let ink_strokes = {
            let conn = db.get().unwrap();
            repositories::ink::strokes_for_annotation(&conn, annotation).unwrap()
        };
        assert!(
            !ink_strokes.is_empty(),
            "the ink was not deleted by conversion"
        );

        // The text note carries the transcript and its lineage.
        let conn = db.get().unwrap();
        let note = repositories::annotations::get(&conn, note_id).unwrap();
        assert_eq!(note.body, "recognised transcript");
        let source: Option<String> = conn
            .query_row(
                "SELECT source_ink_annotation_id FROM annotations WHERE id = ?1",
                [note_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source, Some(annotation.to_string()));
    }

    #[tokio::test]
    async fn startup_repair_recovers_a_stuck_recognizing_row() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("grimoire.db");
        let db = Database::open(&path).unwrap();

        let annotation = {
            let conn = db.get().unwrap();
            let page_id = page_with_text(&conn, "Some text.");
            ink_note(&conn, page_id)
        };

        // Simulate an abnormal exit mid-job: a row stuck in 'recognizing'.
        {
            let conn = db.get().unwrap();
            repositories::ink_recognition::ensure_pending(&conn, annotation).unwrap();
            repositories::ink_recognition::mark_recognizing(&conn, annotation).unwrap();
        }

        // On the next launch, the startup repair moves it to pending.
        let repaired = {
            let conn = db.get().unwrap();
            repositories::ink_recognition::repair_transient(&conn).unwrap()
        };
        assert_eq!(repaired, 1);
        let row = {
            let conn = db.get().unwrap();
            repositories::ink_recognition::get(&conn, annotation)
                .unwrap()
                .unwrap()
        };
        assert_eq!(row.status, RecognitionStatus::Pending);
    }
}
