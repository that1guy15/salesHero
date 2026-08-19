-- 0001_init.sql — initial schema.
-- Mirrors docs/03-DATA-MODEL.md §2, with four corrections recorded in
-- docs/DECISIONS.md (D1–D4). Forward-only; never edit an applied migration.

-- ─── Playbook ────────────────────────────────────────────────────────────

CREATE TABLE playbooks (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    schema_version  INTEGER NOT NULL,
    locale          TEXT NOT NULL DEFAULT 'en-US',
    is_active       INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE TABLE objections (
    id                  TEXT PRIMARY KEY,
    playbook_id         TEXT NOT NULL REFERENCES playbooks(id) ON DELETE CASCADE,
    label               TEXT NOT NULL,      -- rail display text, <= 28 chars
    category            TEXT,               -- screening|coverage|price|timing|trust|compliance
    kind                TEXT NOT NULL,      -- rebut | comply | disqualify
    response            TEXT NOT NULL,      -- authored body, shown at Layer 1
    talk_track          TEXT,               -- JSON array of strings
    guardrails          TEXT,               -- JSON array; injected into prompt
    rail_slot           INTEGER,            -- 1-6 = on rail; NULL = below fold
    -- D4: split so the console can show authored vs learned and offer a reset.
    threshold_authored  REAL NOT NULL DEFAULT 0.62,
    threshold_learned   REAL,               -- NULL until feedback justifies one
    allow_refine        INTEGER NOT NULL DEFAULT 1,  -- forced 0 when kind='comply'
    enabled             INTEGER NOT NULL DEFAULT 1,
    sort_order          INTEGER NOT NULL DEFAULT 0,
    CHECK (kind IN ('rebut','comply','disqualify')),
    CHECK (rail_slot IS NULL OR (rail_slot BETWEEN 1 AND 6))
);

CREATE UNIQUE INDEX idx_rail_slot ON objections(playbook_id, rail_slot)
    WHERE rail_slot IS NOT NULL;

-- Belt and braces on a legal constraint: enforced in Rust (RebuttableObjection)
-- AND here, so a hand-edited YAML or a direct SQL write cannot enable it.
CREATE TRIGGER trg_comply_no_refine
AFTER INSERT ON objections
WHEN NEW.kind = 'comply' AND NEW.allow_refine = 1
BEGIN
    UPDATE objections SET allow_refine = 0 WHERE id = NEW.id;
END;

CREATE TRIGGER trg_comply_no_refine_upd
AFTER UPDATE ON objections
WHEN NEW.kind = 'comply' AND NEW.allow_refine = 1
BEGIN
    UPDATE objections SET allow_refine = 0 WHERE id = NEW.id;
END;

CREATE TABLE triggers (
    id              INTEGER PRIMARY KEY,
    objection_id    TEXT NOT NULL REFERENCES objections(id) ON DELETE CASCADE,
    phrase          TEXT NOT NULL,
    source          TEXT NOT NULL DEFAULT 'authored',  -- authored | learned
    -- D5: embeddings live here as a raw f32 LE blob rather than in sqlite-vec.
    -- A playbook is tens to low hundreds of entries; brute-force SIMD beats an
    -- index (docs/02-ARCHITECTURE.md §7.3) and drops a C dependency for now.
    embedding       BLOB,
    embed_model     TEXT,
    CHECK (source IN ('authored','learned'))
);

CREATE INDEX idx_triggers_objection ON triggers(objection_id);

-- Phrases that must NOT match this objection. Populated from
-- 'wrong_objection' feedback. Subtracted during scoring.
CREATE TABLE negative_triggers (
    id              INTEGER PRIMARY KEY,
    objection_id    TEXT NOT NULL REFERENCES objections(id) ON DELETE CASCADE,
    phrase          TEXT NOT NULL,
    embedding       BLOB,
    embed_model     TEXT
);

CREATE INDEX idx_negatives_objection ON negative_triggers(objection_id);

-- ─── Documents ───────────────────────────────────────────────────────────

CREATE TABLE documents (
    id              TEXT PRIMARY KEY,
    playbook_id     TEXT NOT NULL REFERENCES playbooks(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    filename        TEXT NOT NULL,
    mime            TEXT NOT NULL,
    sha256          TEXT NOT NULL,
    bytes           BLOB NOT NULL,          -- original, for re-export
    ingested_at     INTEGER NOT NULL
);

CREATE TABLE doc_chunks (
    id              INTEGER PRIMARY KEY,
    document_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    ord             INTEGER NOT NULL,
    text            TEXT NOT NULL,
    token_estimate  INTEGER NOT NULL,
    embedding       BLOB,
    embed_model     TEXT
);

CREATE TABLE objection_docs (
    objection_id    TEXT NOT NULL REFERENCES objections(id) ON DELETE CASCADE,
    document_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    PRIMARY KEY (objection_id, document_id)
);

-- ─── Runtime ─────────────────────────────────────────────────────────────

CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    playbook_id     TEXT NOT NULL,
    started_at      INTEGER NOT NULL,
    ended_at        INTEGER,
    capture_mode    TEXT NOT NULL,          -- system_mix | process
    asr_engine      TEXT NOT NULL,
    provider        TEXT,                   -- NULL in local-only mode
    model           TEXT
);

CREATE TABLE calls (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    started_at      INTEGER NOT NULL,
    ended_at        INTEGER,
    duration_ms     INTEGER,
    outcome         TEXT                    -- connected|voicemail|no_answer|unknown
);

CREATE TABLE utterances (
    id              INTEGER PRIMARY KEY,
    call_id         TEXT NOT NULL REFERENCES calls(id) ON DELETE CASCADE,
    speaker         TEXT NOT NULL,          -- agent | prospect
    t_start_ms      INTEGER NOT NULL,       -- relative to call start; may be negative
    t_end_ms        INTEGER NOT NULL,
    text            TEXT NOT NULL,
    from_preroll    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_utterances_call ON utterances(call_id);

CREATE TABLE cards (
    id              TEXT PRIMARY KEY,
    -- D1: both nullable. The PRD's primary success metric is manual rail use,
    -- and the UX spec keeps hotkeys live with no session and between calls.
    -- A NOT NULL call_id made the headline metric unrecordable.
    session_id      TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    call_id         TEXT REFERENCES calls(id) ON DELETE CASCADE,
    objection_id    TEXT NOT NULL,
    origin          TEXT NOT NULL,          -- detected|manual_hotkey|manual_click
    kind            TEXT NOT NULL,
    window_text     TEXT,                   -- NULL for manual; nulled by TTL purge
    similarity      REAL,
    layer_reached   INTEGER NOT NULL,       -- 0 | 1 | 2
    body_shown      TEXT NOT NULL,
    citations       TEXT,                   -- JSON array of chunk refs
    provider        TEXT,
    model           TEXT,
    prompt_version  TEXT,
    tokens_in       INTEGER,
    tokens_out      INTEGER,
    latency_gate_ms INTEGER,
    latency_llm_ms  INTEGER,
    demoted         INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    CHECK (origin IN ('detected','manual_hotkey','manual_click')),
    CHECK (layer_reached BETWEEN 0 AND 2)
);

CREATE INDEX idx_cards_call ON cards(call_id);
CREATE INDEX idx_cards_origin ON cards(origin);

CREATE TABLE feedback (
    card_id         TEXT PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    verdict         TEXT NOT NULL,          -- up | down
    reason          TEXT,                   -- see docs/03-DATA-MODEL.md §4
    note            TEXT,
    created_at      INTEGER NOT NULL,
    CHECK (verdict IN ('up','down')),
    CHECK (reason IS NULL OR reason IN
        ('not_an_objection','wrong_objection','bad_advice','too_slow'))
);

-- D2: required by docs/06-COMPLIANCE.md §3 but absent from the data model.
-- This is the log the agent uses to enter the removal in their real dialer.
-- Riposte does not suppress dialing and is not a system of record.
CREATE TABLE removal_requests (
    id                  INTEGER PRIMARY KEY,
    call_id             TEXT REFERENCES calls(id) ON DELETE SET NULL,
    card_id             TEXT REFERENCES cards(id) ON DELETE SET NULL,
    requested_at        INTEGER NOT NULL,
    triggering_utterance TEXT,
    exported_at         INTEGER
);

CREATE TABLE settings (
    key             TEXT PRIMARY KEY,
    value           TEXT NOT NULL
);

-- `schema_migrations` is created by the migration runner itself before any
-- migration is applied, so it is deliberately not declared here.
