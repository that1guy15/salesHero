# 03 — Data Model

## 1. Storage

One SQLite file at `%APPDATA%\Riposte\riposte.db`. `rusqlite` with the `bundled` feature; `sqlite-vec` statically linked.

The `sqlite-vec` crate embeds the C source and compiles it at build time via `cc`, exposing `sqlite3_vec_init` for registration through `rusqlite`'s `sqlite3_auto_extension()`. No DLL to ship, no runtime extension loading, no installer complexity.

```rust
use sqlite_vec::sqlite3_vec_init;
use rusqlite::{Connection, ffi::sqlite3_auto_extension};

unsafe {
    sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
}
let db = Connection::open(app_data_path()?)?;
```

Set `PRAGMA journal_mode=WAL` and `PRAGMA synchronous=NORMAL`. Writes happen mid-call and must never block the pipeline.

---

## 2. Schema

```sql
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
    id              TEXT PRIMARY KEY,
    playbook_id     TEXT NOT NULL REFERENCES playbooks(id) ON DELETE CASCADE,
    label           TEXT NOT NULL,          -- rail display text, ≤ 28 chars
    category        TEXT,                   -- screening | coverage | price | timing | trust | compliance
    kind            TEXT NOT NULL,          -- rebut | comply | disqualify
    response        TEXT NOT NULL,          -- authored body, shown at Layer 1
    talk_track      TEXT,                   -- JSON array of strings
    guardrails      TEXT,                   -- JSON array; injected into prompt
    rail_slot       INTEGER,                -- 1-6 = on rail; NULL = below fold
    threshold       REAL NOT NULL DEFAULT 0.62,
    allow_refine    INTEGER NOT NULL DEFAULT 1,   -- forced 0 when kind='comply'
    enabled         INTEGER NOT NULL DEFAULT 1,
    sort_order      INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX idx_rail_slot ON objections(playbook_id, rail_slot)
    WHERE rail_slot IS NOT NULL;

-- Enforced in SQL as well as in Rust. Belt and braces on a legal constraint.
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
    source          TEXT NOT NULL DEFAULT 'authored'   -- authored | learned
);

-- Phrases that must NOT match this objection. Populated from
-- 'wrong_objection' feedback. Subtracted during scoring.
CREATE TABLE negative_triggers (
    id              INTEGER PRIMARY KEY,
    objection_id    TEXT NOT NULL REFERENCES objections(id) ON DELETE CASCADE,
    phrase          TEXT NOT NULL
);

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
    token_estimate  INTEGER NOT NULL
);

CREATE TABLE objection_docs (
    objection_id    TEXT NOT NULL REFERENCES objections(id) ON DELETE CASCADE,
    document_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    PRIMARY KEY (objection_id, document_id)
);

-- ─── Vectors ─────────────────────────────────────────────────────────────

CREATE VIRTUAL TABLE vec_triggers USING vec0(
    trigger_id      INTEGER PRIMARY KEY,
    embedding       float[384]
);

CREATE VIRTUAL TABLE vec_negatives USING vec0(
    negative_id     INTEGER PRIMARY KEY,
    embedding       float[384]
);

CREATE VIRTUAL TABLE vec_chunks USING vec0(
    chunk_id        INTEGER PRIMARY KEY,
    embedding       float[384]
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
    outcome         TEXT                    -- connected | voicemail | no_answer | unknown
);

CREATE TABLE utterances (
    id              INTEGER PRIMARY KEY,
    call_id         TEXT NOT NULL REFERENCES calls(id) ON DELETE CASCADE,
    speaker         TEXT NOT NULL,          -- agent | prospect
    t_start_ms      INTEGER NOT NULL,       -- relative to call start; may be negative (pre-roll)
    t_end_ms        INTEGER NOT NULL,
    text            TEXT NOT NULL,
    from_preroll    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE cards (
    id              TEXT PRIMARY KEY,
    call_id         TEXT NOT NULL REFERENCES calls(id) ON DELETE CASCADE,
    objection_id    TEXT NOT NULL,
    origin          TEXT NOT NULL,          -- detected | manual_hotkey | manual_click
    kind            TEXT NOT NULL,
    window_text     TEXT,                   -- NULL for manual
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
    demoted         INTEGER NOT NULL DEFAULT 0,   -- model said is_objection: false
    created_at      INTEGER NOT NULL
);

CREATE TABLE feedback (
    card_id         TEXT PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    verdict         TEXT NOT NULL,          -- up | down
    reason          TEXT,                   -- see §4
    note            TEXT,
    created_at      INTEGER NOT NULL
);

CREATE TABLE settings (
    key             TEXT PRIMARY KEY,
    value           TEXT NOT NULL
);

CREATE TABLE schema_migrations (
    version         INTEGER PRIMARY KEY,
    applied_at      INTEGER NOT NULL
);
```

**Note on `cards.origin`.** Manual rail uses — hotkey or click — are recorded as cards with `layer_reached = 0`. This is deliberate: it is the primary success metric in `01-PRD.md` §7 and it must be measurable with the same query as detected cards.

---

## 3. Playbook file format

The unit of sharing. A zip with extension `.playbook`, so it double-click-associates but remains inspectable and diffable when unpacked.

```
insurance-cold-call.playbook
├── manifest.json          { schema_version, name, exported_at, app_version }
├── playbook.yaml          objections, triggers, settings
└── docs/
    ├── lead-sources.pdf
    └── carrier-comparison.docx
```

### `playbook.yaml`

```yaml
schema_version: 1
name: "Insurance Cold Call — Default"
locale: en-US

objections:
  - id: obj_no_form
    label: "Never filled out a form"      # rail text, ≤ 28 chars
    category: screening
    kind: rebut
    rail_slot: 1
    threshold: 0.62
    triggers:
      - "I never filled out any form"
      - "I didn't request a quote"
      - "I don't know what you're talking about"
      - "I never signed up for anything"
    negative_triggers:
      - "I filled out the form yesterday"    # affirms — do not fire
    response: |
      Don't argue about the form. Concede immediately, then give a
      concrete reason they might not remember, and ask one question.
    talk_track:
      - "Totally possible — these come to us from comparison sites, so it may not have been our name on it."
      - "Were you looking at rates online in the last few months?"
      - "If it wasn't you, I'll take you out right now — no problem either way."
    guardrails:
      - "Never insist they did fill out a form."
      - "If they repeat the denial, treat it as a removal request."
    doc_refs: [doc_lead_sources]
    enabled: true

  - id: obj_dnc
    label: "Take me off your list"
    category: compliance
    kind: comply                            # forces allow_refine = false
    rail_slot: 6
    threshold: 0.55                          # deliberately lower — false positives are cheap here
    triggers:
      - "take me off your list"
      - "put me on your do not call list"
      - "stop calling me"
      - "don't call here again"
    response: |
      STOP. Do not rebut. Do not offer anything.
      Confirm removal, thank them, end the call.
    talk_track:
      - "Absolutely — I'm removing you right now. You won't hear from us again."
      - "Sorry to have bothered you. Have a good day."
    guardrails:
      - "No rebuttal. No alternative offer. No 'before you go'."
    enabled: true

documents:
  - id: doc_lead_sources
    title: "Where our leads come from"
    filename: lead-sources.pdf
    sha256: "…"

settings:
  gate_threshold_default: 0.62
  dedupe_window_ms: 45000
  card_lifetime_ms: 90000
```

### Rules

- `schema_version` present from v1. Write the migration path before it is needed.
- `id` values are stable and referenced by feedback data. Editing a label never changes an id.
- `kind: comply` forces `allow_refine: false` on import regardless of file contents. Enforced in code and by SQL trigger.
- `rail_slot` must be unique within a playbook, 1–6. Slot collisions on import: later entry wins, earlier is bumped below fold, warn the user.
- Exports contain **no secrets, no transcripts, no feedback**. Objections and documents only.
- Documents are embedded in the zip, not referenced by path. A playbook is portable or it is useless.

### Import behavior

| Situation | Behavior |
|---|---|
| `schema_version` > app supports | Refuse, tell the user to update |
| `schema_version` < current | Run migrations, note in UI |
| Objection id collides with existing | Prompt: replace, keep both (suffix id), or skip |
| Document sha256 already present | Reuse the existing row, do not re-embed |
| Malformed yaml | Refuse whole import, name the failing key and line |

Import is atomic. Partial imports leave the user unable to reason about what they have.

---

## 4. Feedback reason codes

On 👎, ask exactly one question — four options, one tap. These are separable signals with different remedies, which a single undifferentiated thumbs-down cannot give you.

| Code | Meaning | Automated response |
|---|---|---|
| `not_an_objection` | Gate false positive | Raise this objection's threshold by 0.02 (floor 0.50); add window text to `negative_triggers` |
| `wrong_objection` | Matched, but the wrong entry | Add window text to that entry's `negative_triggers`; prompt user to pick the right one and add it as a positive trigger |
| `bad_advice` | Right objection, bad card | No automated change. Surface in the console for the user to edit the authored response. |
| `too_slow` | Arrived after the moment had passed | Record against provider+model; surface p50 in the model picker |

On 👍: add the window text as a `learned` trigger if similarity was between threshold and threshold + 0.08 — i.e. it barely made it, so reinforce it. Cap learned triggers at 20 per objection, evicting oldest.

### Threshold learning

Recompute nightly, per objection, once ≥ 10 feedback events exist:

```
positives = confirmed hits (👍, or 👎 with reason ≠ not_an_objection)
negatives = 👎 with reason = not_an_objection

threshold = midpoint between the 10th percentile of positive similarities
            and the 90th percentile of negative similarities
clamped to [0.50, 0.85]
```

Show the learned value in the console next to the authored default, with a reset button. Users need to be able to see and undo what the system decided.

---

## 5. Migrations

Numbered SQL files in `crates/store/migrations/NNNN_name.sql`, applied in order, recorded in `schema_migrations`. Forward-only.

Required test: for every historical `schema_version`, a fixture database migrates cleanly to current and all data is preserved. This test exists from v1 with a single fixture. It is much easier to add now than to retrofit after the first user has data worth losing.
