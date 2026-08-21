//! The domain: entities, their invariants, and the pure logic that derives
//! facts from them.
//!
//! Nothing here performs I/O. No SQL, no filesystem, no HTTP, no clock beyond
//! `now()`. That constraint is what makes the parts of Grimoire that can lose a
//! user's work — text derivation, anchor relocation, settings validation —
//! testable exhaustively and quickly.

pub mod annotation;
pub mod ids;
pub mod ink;
pub mod ink_recognition;
pub mod manuscript;
pub mod settings;
pub mod text;

pub use ids::{
    AnnotationId, AttachmentId, BookmarkId, ChapterId, PageId, RevisionId, SessionId, StrokeId,
    TagId, VolumeId,
};
pub use ink::{InkPoint, InkStroke, InkTool};
pub use manuscript::{
    Chapter, ChapterOutline, CoverTint, Outline, Page, PageSummary, Timestamp, Volume,
    VolumeSummary, empty_document, now,
};
