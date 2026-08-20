//! Volume, Chapter, and Page — the manuscript hierarchy.
//!
//! Constructors enforce the invariants that must hold everywhere, so a
//! repository never has to remember to trim a title or recompute a word count.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::ids::{ChapterId, PageId, VolumeId};
use super::text;

pub type Timestamp = DateTime<Utc>;

pub fn now() -> Timestamp {
    Utc::now()
}

pub const UNTITLED_VOLUME: &str = "Untitled Volume";
pub const UNTITLED_CHAPTER: &str = "Untitled Chapter";
pub const UNTITLED_PAGE: &str = "Untitled Page";

/// An empty editor document: one empty paragraph, which is what ProseMirror
/// requires and what puts a cursor on the page.
pub fn empty_document() -> Value {
    json!({ "type": "doc", "content": [{ "type": "paragraph" }] })
}

/// The palette a Volume's spine is drawn in.
///
/// Grimoire has no cover-image feature, so rather than show eight identical
/// grey rectangles on the shelf, each Volume gets a tint. It is a real, small
/// piece of identity instead of a placeholder for a feature that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverTint {
    Ink,
    Clay,
    Moss,
    Slate,
    Wine,
    Ochre,
    Plum,
    Sea,
}

impl CoverTint {
    pub const ALL: [CoverTint; 8] = [
        CoverTint::Ink,
        CoverTint::Clay,
        CoverTint::Moss,
        CoverTint::Slate,
        CoverTint::Wine,
        CoverTint::Ochre,
        CoverTint::Plum,
        CoverTint::Sea,
    ];

    /// Picks a tint from the identifier, so two Volumes created in a row look
    /// different without the user being asked to choose.
    pub fn derive(id: &VolumeId) -> Self {
        let bytes = id.as_uuid().as_bytes();
        let index = usize::from(bytes[15]) % Self::ALL.len();
        Self::ALL[index]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CoverTint::Ink => "ink",
            CoverTint::Clay => "clay",
            CoverTint::Moss => "moss",
            CoverTint::Slate => "slate",
            CoverTint::Wine => "wine",
            CoverTint::Ochre => "ochre",
            CoverTint::Plum => "plum",
            CoverTint::Sea => "sea",
        }
    }

    pub fn parse(raw: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|tint| tint.as_str() == raw)
            .unwrap_or(CoverTint::Ink)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub id: VolumeId,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub tint: CoverTint,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub last_opened_at: Option<Timestamp>,
    pub archived_at: Option<Timestamp>,
}

impl Volume {
    pub fn create(title: &str, subtitle: Option<&str>, description: Option<&str>) -> Self {
        let id = VolumeId::new();
        let at = now();
        Self {
            tint: CoverTint::derive(&id),
            id,
            title: title_or(title, UNTITLED_VOLUME),
            subtitle: optional_text(subtitle),
            description: optional_text(description),
            created_at: at,
            updated_at: at,
            last_opened_at: None,
            archived_at: None,
        }
    }

    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

/// A Volume as the Library shelf needs it: the record plus the counts that
/// would otherwise force the frontend to fetch the whole manuscript to render
/// one row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeSummary {
    #[serde(flatten)]
    pub volume: Volume,
    pub chapter_count: i64,
    pub page_count: i64,
    pub word_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: ChapterId,
    pub volume_id: VolumeId,
    pub title: String,
    pub position: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Chapter {
    pub fn create(volume_id: VolumeId, title: &str, position: i64) -> Self {
        let at = now();
        Self {
            id: ChapterId::new(),
            volume_id,
            title: title_or(title, UNTITLED_CHAPTER),
            position: position.max(0),
            created_at: at,
            updated_at: at,
        }
    }
}

/// A Page with its document. Loaded when a Page is opened, not when a tree is
/// drawn — see [`PageSummary`] for the latter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: PageId,
    pub chapter_id: ChapterId,
    pub title: String,
    /// The canonical representation. Markdown is import/export only.
    pub document: Value,
    /// Derived from `document` in the same transaction that writes it.
    pub plain_text: String,
    pub position: i64,
    /// Increments on every persisted change. Anchors and AI suggestions record
    /// the revision they were computed against so staleness can be detected.
    pub revision_number: i64,
    pub word_count: i64,
    pub character_count: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Page {
    pub fn create(chapter_id: ChapterId, title: &str, position: i64) -> Self {
        let at = now();
        let document = empty_document();
        Self {
            id: PageId::new(),
            chapter_id,
            title: title_or(title, UNTITLED_PAGE),
            plain_text: String::new(),
            document,
            position: position.max(0),
            revision_number: 1,
            word_count: 0,
            character_count: 0,
            created_at: at,
            updated_at: at,
        }
    }

    /// Applies a new editor document, recomputing every derived field together
    /// so they cannot drift apart.
    pub fn apply_document(&mut self, document: Value) {
        self.plain_text = text::plain_text_from_document(&document);
        self.word_count = text::count_words(&self.plain_text);
        self.character_count = text::count_characters(&self.plain_text);
        self.document = document;
        self.revision_number += 1;
        self.updated_at = now();
    }

    pub fn summary(&self) -> PageSummary {
        PageSummary {
            id: self.id,
            chapter_id: self.chapter_id,
            title: self.title.clone(),
            position: self.position,
            word_count: self.word_count,
            preview: text::preview(&self.plain_text, 90),
            updated_at: self.updated_at,
        }
    }
}

/// What the manuscript tree needs. Deliberately excludes the document: a Volume
/// with two hundred Pages must not ship two hundred documents to draw a list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSummary {
    pub id: PageId,
    pub chapter_id: ChapterId,
    pub title: String,
    pub position: i64,
    pub word_count: i64,
    pub preview: String,
    pub updated_at: Timestamp,
}

/// A Volume's structure in one round trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outline {
    pub volume: Volume,
    pub chapters: Vec<ChapterOutline>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterOutline {
    #[serde(flatten)]
    pub chapter: Chapter,
    pub pages: Vec<PageSummary>,
}

fn title_or(raw: &str, fallback: &str) -> String {
    let normalised = text::normalise_title(raw);
    if normalised.is_empty() {
        fallback.to_string()
    } else {
        normalised
    }
}

fn optional_text(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_volume_without_a_title_gets_a_usable_one() {
        assert_eq!(Volume::create("", None, None).title, UNTITLED_VOLUME);
        assert_eq!(Volume::create("   ", None, None).title, UNTITLED_VOLUME);
    }

    #[test]
    fn volume_titles_are_normalised() {
        let volume = Volume::create("  The   Salt Road  ", None, None);
        assert_eq!(volume.title, "The Salt Road");
    }

    #[test]
    fn blank_subtitles_become_absent_rather_than_empty() {
        let volume = Volume::create("A", Some("   "), Some(""));
        assert!(volume.subtitle.is_none());
        assert!(volume.description.is_none());
    }

    #[test]
    fn a_new_volume_is_not_archived() {
        assert!(!Volume::create("A", None, None).is_archived());
    }

    #[test]
    fn tints_are_stable_for_an_id_and_survive_a_round_trip() {
        let id = VolumeId::new();
        assert_eq!(CoverTint::derive(&id), CoverTint::derive(&id));
        for tint in CoverTint::ALL {
            assert_eq!(CoverTint::parse(tint.as_str()), tint);
        }
    }

    #[test]
    fn an_unknown_tint_falls_back_rather_than_failing_to_load_a_volume() {
        assert_eq!(CoverTint::parse("chartreuse"), CoverTint::Ink);
    }

    #[test]
    fn a_new_page_starts_empty_but_editable() {
        let page = Page::create(ChapterId::new(), "One", 0);
        assert_eq!(page.word_count, 0);
        assert_eq!(page.character_count, 0);
        assert_eq!(page.revision_number, 1);
        // An empty paragraph, so the editor has somewhere to put the cursor.
        assert_eq!(page.document["content"][0]["type"], "paragraph");
    }

    #[test]
    fn applying_a_document_recomputes_every_derived_field_together() {
        let mut page = Page::create(ChapterId::new(), "One", 0);
        page.apply_document(json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{ "type": "text", "text": "The road had been salt once" }]
            }]
        }));

        assert_eq!(page.plain_text, "The road had been salt once");
        assert_eq!(page.word_count, 6);
        assert_eq!(page.character_count, 27);
        assert_eq!(page.revision_number, 2);
    }

    #[test]
    fn every_save_advances_the_revision_number() {
        let mut page = Page::create(ChapterId::new(), "One", 0);
        let start = page.revision_number;
        page.apply_document(empty_document());
        page.apply_document(empty_document());
        assert_eq!(page.revision_number, start + 2);
    }

    #[test]
    fn negative_positions_are_clamped() {
        assert_eq!(Chapter::create(VolumeId::new(), "A", -5).position, 0);
        assert_eq!(Page::create(ChapterId::new(), "A", -1).position, 0);
    }

    #[test]
    fn a_page_summary_carries_a_preview_but_not_the_document() {
        let mut page = Page::create(ChapterId::new(), "One", 0);
        page.apply_document(json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{ "type": "text", "text": "The road had been salt once, or so they said" }]
            }]
        }));
        let summary = page.summary();
        assert!(summary.preview.starts_with("The road"));
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json.get("document").is_none());
    }

    #[test]
    fn volumes_serialise_with_camel_case_keys_for_the_frontend() {
        let json = serde_json::to_value(Volume::create("A", None, None)).unwrap();
        assert!(json.get("createdAt").is_some());
        assert!(json.get("created_at").is_none());
    }
}
