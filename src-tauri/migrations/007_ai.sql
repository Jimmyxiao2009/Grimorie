-- AI providers, profiles, and suggestions.
--
-- No API key is ever stored here. `credential_ref` names an entry in the
-- operating system's credential store; the secret lives there and nowhere else
-- in Grimoire — not in this file, not in a config file, not in a log.

CREATE TABLE ai_providers (
    id                TEXT PRIMARY KEY NOT NULL,
    -- What the user calls it: "OpenAI", "my server", "work gateway".
    name              TEXT NOT NULL,
    base_url          TEXT NOT NULL,
    model             TEXT NOT NULL,
    -- The key under which the secret sits in the OS credential store.
    credential_ref    TEXT NOT NULL,
    temperature       REAL NOT NULL DEFAULT 0.7,
    -- Null means "whatever the model defaults to".
    max_output_tokens INTEGER,
    -- Declared context window, used to budget how much manuscript may travel.
    context_length    INTEGER NOT NULL DEFAULT 8192,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
) STRICT;

-- The role the model is asked to play. Prompts are data rather than code so a
-- writer can adjust the wording without a rebuild — but every template ships
-- with a default, defined once in ai::prompts.
CREATE TABLE ai_profiles (
    id             TEXT PRIMARY KEY NOT NULL,
    name           TEXT NOT NULL,
    description    TEXT NOT NULL DEFAULT '',
    system_prompt  TEXT NOT NULL,
    -- Null uses whichever provider is configured.
    provider_id    TEXT REFERENCES ai_providers (id) ON DELETE SET NULL,
    -- Null inherits the provider's temperature.
    temperature    REAL,
    -- selection | page | chapter — how much manuscript the request may carry.
    context_policy TEXT NOT NULL DEFAULT 'selection',
    -- 1 for the profiles Grimoire ships. Editable, but restored if deleted.
    builtin        INTEGER NOT NULL DEFAULT 0,
    position       INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
) STRICT;

-- A proposed edit.
--
-- Everything needed to decide whether applying it is still safe is recorded at
-- the moment it was made: which revision it was computed against, the exact
-- text it expected to replace, and a hash of that text. Apply re-checks all of
-- it against the live Page. There is no path that patches text a suggestion was
-- not computed against.
CREATE TABLE ai_suggestions (
    id               TEXT PRIMARY KEY NOT NULL,
    page_id          TEXT NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    -- The Margin note this suggestion belongs to.
    annotation_id    TEXT REFERENCES annotations (id) ON DELETE CASCADE,
    base_revision    INTEGER NOT NULL,
    anchor_from      INTEGER NOT NULL,
    anchor_to        INTEGER NOT NULL,
    original_text    TEXT NOT NULL,
    replacement_text TEXT NOT NULL,
    context_hash     TEXT NOT NULL,
    -- pending | applied | dismissed | stale
    status           TEXT NOT NULL DEFAULT 'pending',
    created_at       TEXT NOT NULL,
    applied_at       TEXT
) STRICT;

CREATE INDEX ai_suggestions_page ON ai_suggestions (page_id, status);
CREATE INDEX ai_suggestions_annotation ON ai_suggestions (annotation_id);
