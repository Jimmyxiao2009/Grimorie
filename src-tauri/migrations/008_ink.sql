-- Handwritten ink strokes.
--
-- Ink is an annotation kind: a handwritten margin note is an `annotations` row
-- whose kind is 'ink', and its vector content lives here. Linking to
-- `annotations` rather than `pages` directly gives ink the anchoring, staleness,
-- and cascade behaviour the Margin already implements — including page delete,
-- which cascades annotations, which cascade these strokes.
--
-- Points are stored as one JSON blob per stroke rather than one row per point.
-- A stroke is read and written as a unit, never edited point by point, so the
-- row granularity matches the access pattern. The JSON is a versioned envelope
-- (see domain::ink) so a future point schema can migrate forward without
-- splitting the table.

CREATE TABLE ink_strokes (
    id             TEXT PRIMARY KEY NOT NULL,
    -- The margin note this stroke belongs to. Cascades from the annotation, so
    -- deleting the note — or the page beneath it — takes the handwriting with
    -- it and leaves nothing behind.
    annotation_id  TEXT NOT NULL REFERENCES annotations (id) ON DELETE CASCADE,
    -- pen | highlighter.
    tool           TEXT NOT NULL,
    -- A semantic colour id resolved per theme, or a raw CSS colour.
    color          TEXT NOT NULL,
    width          REAL NOT NULL,
    -- The versioned points envelope from domain::ink.
    points_json    TEXT NOT NULL,
    position       INTEGER NOT NULL,
    created_at     TEXT NOT NULL
) STRICT;

-- The Margin's query: every stroke on a Page, in drawing order.
CREATE INDEX ink_strokes_annotation ON ink_strokes (annotation_id, position);
