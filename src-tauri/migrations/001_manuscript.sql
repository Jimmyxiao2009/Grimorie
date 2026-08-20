-- The manuscript hierarchy and the settings store.
--
-- Timestamps are RFC3339 UTC strings. They sort correctly as text, survive
-- export to JSON unchanged, and are readable when someone opens the database
-- with an external tool — which matters for software whose promise is that the
-- user owns their data.
--
-- Identifiers are UUID text. Foreign keys cascade downward through the
-- hierarchy: deleting a Volume must not leave orphaned Chapters behind.

CREATE TABLE volumes (
    id             TEXT PRIMARY KEY NOT NULL,
    title          TEXT NOT NULL,
    subtitle       TEXT,
    description    TEXT,
    tint           TEXT NOT NULL DEFAULT 'ink',
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    last_opened_at TEXT,
    archived_at    TEXT
) STRICT;

-- The Library's two default views: most recently opened, and not archived.
CREATE INDEX volumes_last_opened ON volumes (last_opened_at DESC);
CREATE INDEX volumes_archived ON volumes (archived_at);

CREATE TABLE chapters (
    id         TEXT PRIMARY KEY NOT NULL,
    volume_id  TEXT NOT NULL REFERENCES volumes (id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    position   INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE INDEX chapters_volume ON chapters (volume_id, position);

CREATE TABLE pages (
    id              TEXT PRIMARY KEY NOT NULL,
    chapter_id      TEXT NOT NULL REFERENCES chapters (id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    -- The canonical representation of the document. Markdown is an
    -- import/export format and never the source of truth.
    document_json   TEXT NOT NULL,
    -- Derived from document_json in the same transaction, so the two cannot
    -- disagree. Exists for search, previews, statistics, and AI context.
    plain_text      TEXT NOT NULL DEFAULT '',
    position        INTEGER NOT NULL,
    -- Advances on every persisted change. Annotation anchors and AI
    -- suggestions record the revision they were computed against.
    revision_number INTEGER NOT NULL DEFAULT 1,
    word_count      INTEGER NOT NULL DEFAULT 0,
    character_count INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;

CREATE INDEX pages_chapter ON pages (chapter_id, position);
CREATE INDEX pages_updated ON pages (updated_at DESC);

CREATE TABLE settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
