-- Bookmarks and tags.
--
-- Both are deliberately small. A bookmark is a Page you meant to come back to;
-- a tag is a word you want to gather things under. Neither is a knowledge
-- graph, and nothing here should grow into one.

CREATE TABLE bookmarks (
    id         TEXT PRIMARY KEY NOT NULL,
    page_id    TEXT NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    -- Optional note about why this was marked. Usually empty.
    label      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
) STRICT;

-- One bookmark per Page. Marking a Page twice is not a thing a person means to
-- do, and a list with the same Page in it three times is just untidy.
CREATE UNIQUE INDEX bookmarks_page ON bookmarks (page_id);

CREATE TABLE tags (
    id         TEXT PRIMARY KEY NOT NULL,
    -- Display form, as the user typed it.
    name       TEXT NOT NULL,
    -- Case-folded and trimmed, so "Salt" and "salt" are the same tag.
    slug       TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE UNIQUE INDEX tags_slug ON tags (slug);

-- Tags attach to Volumes, Chapters, and Pages. A single table with a kind
-- column rather than three join tables: the alternative triples the schema to
-- express the same relationship three times.
--
-- Deliberately no foreign key on entity_id, because it points at one of three
-- tables. Rows are cleaned up explicitly when their subject is deleted, which
-- the repository is responsible for and its tests cover.
CREATE TABLE entity_tags (
    tag_id      TEXT NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    entity_kind TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (tag_id, entity_id)
) STRICT;

CREATE INDEX entity_tags_entity ON entity_tags (entity_id);
