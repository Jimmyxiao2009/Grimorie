-- Page revision history.
--
-- Snapshots, not diffs. A manuscript Page is a few kilobytes and disk is
-- cheap; reconstructing text by replaying a chain of patches is how version
-- history turns into a source of data loss rather than a defence against it.
--
-- This is not version control and will not grow into it. History is bounded
-- per Page, and the only questions it answers are "what did this say before?"
-- and "put it back".

CREATE TABLE revisions (
    id              TEXT PRIMARY KEY NOT NULL,
    page_id         TEXT NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    -- The page's revision_number at the moment this snapshot was taken.
    revision_number INTEGER NOT NULL,
    document_json   TEXT NOT NULL,
    plain_text      TEXT NOT NULL,
    word_count      INTEGER NOT NULL,
    -- Why the snapshot exists: 'checkpoint', 'before-restore', 'before-ai'.
    -- Kept as text rather than an enum so a future reason cannot fail a load.
    reason          TEXT NOT NULL,
    created_at      TEXT NOT NULL
) STRICT;

CREATE INDEX revisions_page ON revisions (page_id, created_at DESC);
