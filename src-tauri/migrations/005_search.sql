-- Full-text search.
--
-- The indexed columns hold a *segmented* form of the text, not the text itself.
-- SQLite's unicode61 tokenizer classifies CJK characters as letters, so an
-- unbroken run of them becomes one enormous token and searching for a two
-- character word inside it finds nothing. This was measured before the schema
-- was chosen; the trigram tokenizer fails the same query, because two
-- characters cannot form a trigram.
--
-- Segmentation happens in Rust (domain::text::segment_for_index) and the
-- identical transform is applied to queries, so the two always agree. That also
-- rules out maintaining this index with SQL triggers — SQLite cannot segment —
-- which is why the repositories write to it explicitly.
--
-- Snippets are built in Rust from the *original* text rather than by FTS5's
-- snippet(), which would return the segmented form with spaces between every
-- CJK character.

CREATE VIRTUAL TABLE search_index USING fts5(
    title,
    body,
    entity_kind UNINDEXED,
    entity_id   UNINDEXED,
    volume_id   UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Maps an entity to its FTS rowid.
--
-- Without this, updating a Page's index entry would mean DELETE ... WHERE
-- entity_id = ?, which FTS5 answers with a full scan. A Page is re-indexed on
-- every save, so that scan would sit in the autosave path.
CREATE TABLE search_rows (
    entity_id TEXT PRIMARY KEY NOT NULL,
    row_id    INTEGER NOT NULL,
    volume_id TEXT
) STRICT;

CREATE INDEX search_rows_volume ON search_rows (volume_id);
