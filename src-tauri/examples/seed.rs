//! Builds a scratch library with sample content, for development.
//!
//! This exists so the workspace can be looked at with a real manuscript in it
//! without hand-typing one every time. It writes through the same repositories
//! the application uses, so what it produces cannot differ from what the app
//! would have produced.
//!
//! It refuses to touch an existing file — a development convenience must never
//! be one typo away from overwriting someone's manuscripts.
//!
//!   cargo run --example seed -- ../.grimoire-dev/scratch.db
//!   GRIMOIRE_LIBRARY=../.grimoire-dev/scratch.db pnpm tauri dev

use std::path::PathBuf;

use grimoire_lib::database::Database;
use grimoire_lib::domain::annotation::AnnotationKind;
use grimoire_lib::repositories;
use serde_json::json;

fn paragraph(text: &str) -> serde_json::Value {
    json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
}

fn document(blocks: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "type": "doc", "content": blocks })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../.grimoire-dev/scratch.db".to_string())
        .into();

    if path.exists() {
        eprintln!(
            "{} already exists. Delete it first, or name another file.",
            path.display()
        );
        std::process::exit(1);
    }

    let db = Database::open(&path)?;
    let conn = db.get()?;

    // --- A novel in progress. ---
    let salt = repositories::volumes::create(
        &conn,
        "The Salt Road",
        Some("A novel"),
        Some("Ilse walks east, and the road remembers what it used to carry."),
    )?;

    let one = repositories::chapters::create(&conn, salt.id, "Chapter I — Leaving")?;
    let two = repositories::chapters::create(&conn, salt.id, "Chapter II — The Carters")?;
    let three = repositories::chapters::create(&conn, salt.id, "Chapter III — Wind and Ash")?;

    let crows = repositories::pages::create(&conn, one.id, "The Crows")?;
    repositories::pages::save_document(
        &conn,
        crows.id,
        document(vec![
            paragraph(
                "The road had been salt once, or so the carters said, and the wind still carried \
                 the taste of it in dry months. Ilse walked it at dusk because dusk was when the \
                 crows left, and she had never been able to write with crows watching.",
            ),
            paragraph(
                "She had taken nothing but the ledger, which was heavier than it looked, and the \
                 knife her mother had used for bread and for other things.",
            ),
            json!({
                "type": "blockquote",
                "content": [paragraph("What the road carries, the road keeps.")]
            }),
            paragraph(
                "It was not true. She had watched the road give things back for three winters, \
                 and none of them had come back the way they went.",
            ),
        ]),
    )?;

    let ledger = repositories::pages::create(&conn, one.id, "The Ledger")?;
    repositories::pages::save_document(
        &conn,
        ledger.id,
        document(vec![
            json!({
                "type": "heading",
                "attrs": { "level": 2 },
                "content": [{ "type": "text", "text": "What was written in it" }]
            }),
            paragraph(
                "Names, mostly. A column of them, and beside each a weight, and beside the weight \
                 a place. Her mother's hand for the first two hundred pages and then a hand she \
                 did not know.",
            ),
        ]),
    )?;

    let carters = repositories::pages::create(&conn, two.id, "A Fire, and Four Men")?;
    repositories::pages::save_document(
        &conn,
        carters.id,
        document(vec![paragraph(
            "They let her sit at the edge of the fire without asking anything, which told her more \
             about them than an hour of questions would have.",
        )]),
    )?;

    repositories::pages::create(&conn, two.id, "The Offer")?;
    repositories::pages::create(&conn, three.id, "Untitled Page")?;

    // --- A second Volume, so the shelf has more than one thing on it. ---
    let field = repositories::volumes::create(
        &conn,
        "Field Notes",
        Some("Research"),
        Some("Salt, roads, and the people who moved one along the other."),
    )?;
    let sources = repositories::chapters::create(&conn, field.id, "Sources")?;
    let salt_trade = repositories::pages::create(&conn, sources.id, "The salt trade")?;
    repositories::pages::save_document(
        &conn,
        salt_trade.id,
        document(vec![paragraph(
            "Salt roads are real, and duller than the book needs them to be. Keep the geography, \
             lose the accountancy.",
        )]),
    )?;

    // --- A CJK Volume, so mixed scripts are visible in the tree and editor. ---
    let manuscript = repositories::volumes::create(&conn, "手稿", Some("笔记"), None)?;
    let chapter = repositories::chapters::create(&conn, manuscript.id, "第三章 — 风与灰烬")?;
    let opening = repositories::pages::create(&conn, chapter.id, "开端")?;
    repositories::pages::save_document(
        &conn,
        opening.id,
        document(vec![paragraph(
            "手稿属于用户。The manuscript belongs to the user — 一句她抄在扉页上的话，却从未真正相信过。",
        )]),
    )?;

    // A few margin notes, including one anchored to text that exists and one
    // whose text has since changed — so the stale path is visible too.
    let crows_text = repositories::pages::get(&conn, crows.id)?.plain_text;
    let mut anchor_on =
        |phrase: &str, kind, body: &str| -> Result<(), Box<dyn std::error::Error>> {
            if let Some(byte) = crows_text.find(phrase) {
                let from = crows_text[..byte].chars().count() as i64;
                let to = from + phrase.chars().count() as i64;
                repositories::annotations::create_anchored(&conn, crows.id, kind, body, from, to)?;
            }
            Ok(())
        };

    anchor_on(
        "carters",
        AnnotationKind::Question,
        "Whose voice is this? If Ilse has never met them, she cannot know what they said.",
    )?;
    anchor_on(
        "the knife her mother had used for bread",
        AnnotationKind::Suggestion,
        "Too neat. Let the knife be ordinary here and terrible later.",
    )?;
    repositories::annotations::create_for_page(
        &conn,
        crows.id,
        AnnotationKind::Note,
        "Opening works. The second paragraph is doing two jobs.",
    )?;

    repositories::volumes::touch_opened(&conn, salt.id)?;

    println!("seeded {}", path.display());
    println!("  The Salt Road  {}", salt.id);
    println!("  Field Notes    {}", field.id);
    println!("  手稿            {}", manuscript.id);
    println!(
        "run with:  GRIMOIRE_LIBRARY={} pnpm tauri dev",
        path.display()
    );
    Ok(())
}
