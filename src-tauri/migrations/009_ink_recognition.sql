-- Ink intelligence — handwriting recognition metadata.
--
-- Recognition is derived from the ink strokes in `ink_strokes`, never a
-- replacement for them. This table holds the transcript, its status, and the
-- content hash of the strokes it was generated against, so a result can be
-- invalidated the moment the handwriting changes.
--
-- One row per ink annotation, at most. The annotation owns the strokes and
-- cascades to them; this row cascades from the annotation too, so deleting a
-- margin note — or the page beneath it — takes its recognition metadata with it
-- and leaves nothing behind. The handwritten strokes and this derived metadata
-- disappear together, because the metadata has no meaning without the ink it
-- describes.

CREATE TABLE ink_recognition (
    -- The ink annotation this recognition describes. Cascades from the
    -- annotation, so a deleted note's recognition cannot linger.
    annotation_id    TEXT PRIMARY KEY NOT NULL REFERENCES annotations (id) ON DELETE CASCADE,
    -- pending | recognizing | recognized | failed | stale | disabled.
    -- Text rather than a constraint, so a status added by a newer build cannot
    -- make a library unreadable by an older one — the same trade the
    -- annotations table makes for `kind`.
    status           TEXT NOT NULL DEFAULT 'pending',
    -- The transcript, recognised or hand-corrected. NULL until a recognizer or
    -- the writer supplies one. Not stored in a separate column from the status,
    -- because a transcript can outlive its own freshness (a stale result still
    -- shows its text until a refresh completes).
    recognized_text  TEXT,
    confidence       REAL,
    provider         TEXT,
    model            TEXT,
    language         TEXT,
    -- recognized | user-edited. Decides whether a later automatic run may
    -- overwrite the transcript; a hand-edited transcript wins.
    transcript_source TEXT NOT NULL DEFAULT 'recognized',
    -- The ink_content_hash of the strokes this result was generated from. NULL
    -- until the first recognition completes. When the ink changes, this no
    -- longer matches and the result becomes stale.
    content_hash     TEXT,
    -- A short, non-private description of a failure, for the retry affordance.
    error            TEXT,
    recognized_at    TEXT,
    updated_at       TEXT NOT NULL
) STRICT;

-- The Margin's query: every recognition row for a page, joined to its
-- annotation. Kept narrow because the page load reads them all at once.
CREATE INDEX ink_recognition_status ON ink_recognition (status);

-- Traceability for "convert handwriting to a text note": the typed annotation
-- created from an ink transcript remembers which ink it came from, so the
-- relationship survives a page reload and a future "archive the ink" action can
-- find it. Nullable, because most text notes are not converted from ink.
ALTER TABLE annotations ADD COLUMN source_ink_annotation_id TEXT REFERENCES annotations (id) ON DELETE SET NULL;
