//! Typed identifiers.
//!
//! Every entity gets its own ID type rather than sharing a bare `Uuid`. The
//! cost is one macro; the benefit is that passing a `ChapterId` where a
//! `PageId` belongs stops compiling. In a codebase where a mistake like that
//! would silently attach an annotation to the wrong Page, that is worth having.
//!
//! IDs serialise as plain strings, so the frontend sees ordinary opaque
//! identifiers and never needs to know they are UUIDs.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, Result};

macro_rules! typed_id {
    ($name:ident, $label:literal) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Mints a new identifier.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Parses an identifier that arrived from the frontend.
            pub fn parse(raw: &str) -> Result<Self> {
                Uuid::parse_str(raw).map(Self).map_err(|_| {
                    AppError::invalid(format!("That is not a valid {} identifier.", $label))
                })
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                // Hyphenated lower-case, which is what SQLite stores and what
                // the frontend round-trips.
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = AppError;
            fn from_str(raw: &str) -> Result<Self> {
                Self::parse(raw)
            }
        }

        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(rusqlite::types::ToSqlOutput::from(self.0.to_string()))
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(
                value: rusqlite::types::ValueRef<'_>,
            ) -> rusqlite::types::FromSqlResult<Self> {
                let text = value.as_str()?;
                Uuid::parse_str(text)
                    .map(Self)
                    .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }
    };
}

typed_id!(VolumeId, "Volume");
typed_id!(ChapterId, "Chapter");
typed_id!(PageId, "Page");
typed_id!(AnnotationId, "annotation");
typed_id!(BookmarkId, "bookmark");
typed_id!(TagId, "tag");
typed_id!(RevisionId, "revision");
typed_id!(AttachmentId, "attachment");
typed_id!(SessionId, "writing session");
typed_id!(AiProfileId, "AI profile");
typed_id!(AiProviderId, "AI provider");
typed_id!(SuggestionId, "AI suggestion");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        assert_ne!(PageId::new(), PageId::new());
    }

    #[test]
    fn ids_round_trip_through_text() {
        let id = ChapterId::new();
        let parsed = ChapterId::parse(&id.to_string()).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn parsing_rubbish_names_the_entity() {
        let error = PageId::parse("not-a-uuid").unwrap_err();
        assert!(error.message.contains("Page"), "{}", error.message);
    }

    #[test]
    fn ids_serialise_as_bare_strings() {
        let id = VolumeId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
    }
}
