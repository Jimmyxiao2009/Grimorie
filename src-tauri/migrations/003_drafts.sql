-- Crash-safe draft journal.
--
-- Autosave commits a Page every time typing pauses. This table shortens the
-- window before that: the editor journals its document on a much shorter
-- debounce into a single row, which is a cheap upsert rather than a full save
-- with its derived-field recomputation and revision bookkeeping.
--
-- Durability comes from SQLite's write-ahead log, so a journalled draft
-- survives the process being killed. On the next launch a draft that is newer
-- than its Page is offered back to the writer; one that is not is deleted
-- without a word, because recovery UI for nothing to recover is worse than no
-- recovery UI at all.
--
-- A draft is deleted the moment its Page saves successfully. A row here always
-- means "this was typed and not committed".

CREATE TABLE drafts (
    -- One live draft per Page; a new one replaces the old.
    page_id        TEXT PRIMARY KEY NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    document_json  TEXT NOT NULL,
    plain_text     TEXT NOT NULL,
    -- The page's revision_number when this draft was captured, so a draft from
    -- before a later successful save can be recognised as stale.
    base_revision  INTEGER NOT NULL,
    captured_at    TEXT NOT NULL
) STRICT;
