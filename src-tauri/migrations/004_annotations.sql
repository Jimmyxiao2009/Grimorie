-- Margin annotations.
--
-- The anchor is stored inline rather than in its own table: an annotation has
-- exactly zero or one anchor, and a join for a one-to-one relationship buys
-- nothing but a chance to end up with an orphan.
--
-- Offsets are character positions into the Page's plain_text, not ProseMirror
-- positions. That is what lets relocation run in Rust, where it can be tested
-- without a DOM — see domain/annotation.rs.

CREATE TABLE annotations (
    id             TEXT PRIMARY KEY NOT NULL,
    page_id        TEXT NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    -- note | question | suggestion | warning | reference | ai-review |
    -- ai-suggestion. Text rather than a constraint so a kind added by a newer
    -- build cannot make a library unreadable by an older one.
    kind           TEXT NOT NULL,
    -- active | resolved | stale
    status         TEXT NOT NULL DEFAULT 'active',
    body           TEXT NOT NULL,

    -- 'page' for a whole-Page note, 'range' for an anchored one. A whole-Page
    -- annotation has no anchor and therefore can never go stale.
    target_kind    TEXT NOT NULL,
    anchor_from    INTEGER,
    anchor_to      INTEGER,
    anchor_text    TEXT,
    context_before TEXT,
    context_after  TEXT,
    -- The page revision the offsets were computed against.
    base_revision  INTEGER,
    text_hash      TEXT,

    -- Names the AI profile when the annotation was written by one.
    author_profile TEXT,

    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
) STRICT;

-- The Margin's query: everything on this Page, active first.
CREATE INDEX annotations_page ON annotations (page_id, status);
