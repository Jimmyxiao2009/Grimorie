//! The IPC surface.
//!
//! Every command here is a thin adapter: parse identifiers, call one repository
//! function, return. Rules that decide what is *allowed* live in the domain;
//! rules about *how it is stored* live in repositories. If a command body grows
//! past a few lines of glue, the logic belongs one layer down.
//!
//! Commands are organised by concern rather than piled into one module, so this
//! stays a designed surface instead of an RPC dump.

pub mod ai;
pub mod annotations;
pub mod bookmarks;
pub mod history;
pub mod ink;
pub mod ink_recognition;
pub mod manuscript;
pub mod search;
pub mod settings;
pub mod volumes;

use crate::domain::{ChapterId, PageId, VolumeId};
use crate::error::Result;

/// Identifiers arrive from the frontend as opaque strings and are validated
/// here, at the boundary, so nothing below this layer handles an untyped id.
pub(crate) fn parse_volume_id(raw: &str) -> Result<VolumeId> {
    VolumeId::parse(raw)
}

pub(crate) fn parse_chapter_id(raw: &str) -> Result<ChapterId> {
    ChapterId::parse(raw)
}

pub(crate) fn parse_page_id(raw: &str) -> Result<PageId> {
    PageId::parse(raw)
}

pub(crate) fn parse_page_ids(raw: &[String]) -> Result<Vec<PageId>> {
    raw.iter().map(|id| PageId::parse(id)).collect()
}

pub(crate) fn parse_chapter_ids(raw: &[String]) -> Result<Vec<ChapterId>> {
    raw.iter().map(|id| ChapterId::parse(id)).collect()
}
