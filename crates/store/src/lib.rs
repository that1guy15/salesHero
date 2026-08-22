//! SQLite persistence. See `docs/03-DATA-MODEL.md`.
//!
//! Embeddings are stored as raw little-endian `f32` blobs on the `triggers`,
//! `negative_triggers`, and `doc_chunks` rows rather than in `sqlite-vec`
//! virtual tables. For a playbook of tens to low hundreds of entries a
//! brute-force scan beats any index — the architecture doc says as much
//! (§7.3) — and it keeps a C dependency out of the build until document
//! retrieval actually needs KNN. See `docs/DECISIONS.md` D5.

use riposte_core::{CardOrigin, Objection, ObjectionKind};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown objection kind: {0}")]
    BadKind(String),
    #[error("{0}")]
    Other(String),
}

/// Applied in order, recorded in `schema_migrations`. Forward-only.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_init.sql")),
    (2, include_str!("../migrations/0002_escalation.sql")),
    (3, include_str!("../migrations/0003_declared_documents.sql")),
];

/// Highest migration this build ships. Asserted by the migration tests so a new
/// migration cannot land without the v1-fixture test noticing.
pub const CURRENT_SCHEMA_VERSION: i64 = 3;

/// A card id that cannot collide with another taken in the same millisecond.
///
/// `format!("card_{}", now_ms())` collides on two rail presses inside one
/// millisecond, and `cards.id` is a primary key — an agent hammering a hotkey
/// would lose the second press, which is precisely the event the PRD's headline
/// metric counts.
///
/// Timestamp for sortability, process id so concurrent CLI invocations do not
/// collide, and a counter for repeats inside one process. Machine-local
/// uniqueness is enough: `03-DATA-MODEL.md` §1 is one SQLite file per install,
/// and there is no hosted component to merge ids across.
pub fn new_card_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        "card_{}_{}_{}",
        now_ms(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self> {
        // Writes happen mid-call and must never block the pipeline.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // D3: off by default in SQLite, which silently makes every ON DELETE
        // CASCADE in the schema a no-op — including the one the transcript TTL
        // in docs/06-COMPLIANCE.md §2 depends on.
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ─── Transactions ────────────────────────────────────────────────────

    /// Runs `f` inside a savepoint, committing on `Ok` and rolling back on
    /// `Err` or on panic.
    ///
    /// Replaces a `begin`/`commit`/`rollback` trio of bare `execute_batch`
    /// calls. Those had no RAII guard, so an early return between `begin` and
    /// `commit` left the transaction open on a shared connection, and a failing
    /// `commit` left it open with no way back. A savepoint rather than a plain
    /// transaction so a caller already inside one composes instead of erroring.
    ///
    /// Callers in other crates get a transaction boundary without depending on
    /// `rusqlite` or converting its error type, which is what the previous
    /// design was reaching for.
    pub fn with_tx<T, E>(
        &self,
        f: impl FnOnce() -> std::result::Result<T, E>,
    ) -> Result<std::result::Result<T, E>> {
        let name = "riposte_tx";
        self.conn.execute_batch(&format!("SAVEPOINT {name}"))?;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(Ok(v)) => {
                self.conn
                    .execute_batch(&format!("RELEASE {name}"))
                    .inspect_err(|_| {
                        let _ = self
                            .conn
                            .execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
                    })?;
                Ok(Ok(v))
            }
            Ok(Err(e)) => {
                self.conn
                    .execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"))?;
                Ok(Err(e))
            }
            Err(panic) => {
                let _ = self
                    .conn
                    .execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
                std::panic::resume_unwind(panic)
            }
        }
    }

    // ─── Migrations ──────────────────────────────────────────────────────

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL)",
            [],
        )?;

        let current: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )?;

        for (version, sql) in MIGRATIONS {
            if *version <= current {
                continue;
            }
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT OR REPLACE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![version, now_ms()],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )?)
    }

    // ─── Playbooks ───────────────────────────────────────────────────────

    pub fn upsert_playbook(
        &self,
        id: &str,
        name: &str,
        schema_version: i64,
        locale: &str,
    ) -> Result<()> {
        let ts = now_ms();
        self.conn.execute(
            "INSERT INTO playbooks (id, name, schema_version, locale, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               schema_version = excluded.schema_version,
               locale = excluded.locale,
               updated_at = excluded.updated_at",
            params![id, name, schema_version, locale, ts],
        )?;
        Ok(())
    }

    pub fn set_active_playbook(&self, id: &str) -> Result<()> {
        self.conn
            .execute("UPDATE playbooks SET is_active = 0", [])?;
        self.conn.execute(
            "UPDATE playbooks SET is_active = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn active_playbook_id(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM playbooks WHERE is_active = 1 LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn list_playbooks(&self) -> Result<Vec<PlaybookSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, p.schema_version, p.is_active,
                    (SELECT COUNT(*) FROM objections o WHERE o.playbook_id = p.id)
             FROM playbooks p ORDER BY p.created_at",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PlaybookSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    schema_version: r.get(2)?,
                    is_active: r.get::<_, i64>(3)? != 0,
                    objection_count: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ─── Objections ──────────────────────────────────────────────────────

    /// Replaces the objection and all of its triggers. Caller supplies a
    /// transaction boundary by calling this inside `with_tx` for atomic import.
    pub fn upsert_objection(&self, playbook_id: &str, o: &Objection) -> Result<()> {
        // Invariant 4, data level. The SQL trigger also enforces this; we set
        // it correctly on the way in so the trigger is a backstop, not the
        // mechanism.
        let allow_refine = o.kind.allows_refinement() && o.allow_refine;

        self.conn.execute(
            "INSERT INTO objections
               (id, playbook_id, label, category, kind, response, talk_track,
                guardrails, rail_slot, threshold_authored, threshold_learned,
                allow_refine, enabled, sort_order, escalate_after_repeats)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(id) DO UPDATE SET
               -- Ownership transfers on a Replace collision. Without this an
               -- objection stays with its original playbook while the importing
               -- one believes it took it.
               playbook_id = excluded.playbook_id,
               label = excluded.label, category = excluded.category,
               kind = excluded.kind, response = excluded.response,
               talk_track = excluded.talk_track, guardrails = excluded.guardrails,
               rail_slot = excluded.rail_slot,
               threshold_authored = excluded.threshold_authored,
               allow_refine = excluded.allow_refine,
               enabled = excluded.enabled, sort_order = excluded.sort_order,
               escalate_after_repeats = excluded.escalate_after_repeats",
            params![
                o.id,
                playbook_id,
                o.label,
                o.category,
                o.kind.as_str(),
                o.response,
                serde_json::to_string(&o.talk_track)?,
                serde_json::to_string(&o.guardrails)?,
                o.rail_slot,
                o.threshold_authored,
                o.threshold_learned,
                allow_refine as i64,
                o.enabled as i64,
                o.sort_order,
                o.escalate_after_repeats,
            ],
        )?;

        // Reconcile rather than delete-and-recreate: an unchanged phrase keeps
        // its row and therefore its embedding, so re-importing a playbook does
        // not force every vector to be recomputed. That is milliseconds under
        // HashEmbedder and seconds under BGE-small.
        self.sync_phrases("triggers", &o.id, &o.triggers)?;
        self.sync_phrases("negative_triggers", &o.id, &o.negative_triggers)?;
        Ok(())
    }

    /// Which playbook currently owns this objection id, if any.
    ///
    /// `objections.id` is a global primary key, so the same id in two playbooks
    /// is a collision rather than two rows. Import consults this to apply the
    /// user's `OnCollision` choice.
    pub fn objection_owner(&self, objection_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT playbook_id FROM objections WHERE id = ?1",
                params![objection_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn load_objections(&self, playbook_id: &str) -> Result<Vec<Objection>> {
        // Canary. The CHECK constraint on `objections.kind` makes this
        // impossible, so a hit means the schema was tampered with or a
        // migration went wrong. Loading such a row would fail closed to
        // `Comply` — safe, but it would silently turn an agent's rebuttal into
        // a compliance card, which is not something to discover on a live call.
        if let Some(id) = self
            .conn
            .query_row(
                "SELECT id FROM objections
                 WHERE playbook_id = ?1 AND kind NOT IN ('rebut','comply','disqualify')
                 LIMIT 1",
                params![playbook_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Err(StoreError::BadKind(id));
        }

        let mut stmt = self.conn.prepare(
            "SELECT id, label, category, kind, response, talk_track, guardrails,
                    rail_slot, threshold_authored, threshold_learned,
                    allow_refine, enabled, sort_order, escalate_after_repeats
             FROM objections WHERE playbook_id = ?1
             ORDER BY (rail_slot IS NULL), rail_slot, sort_order, id",
        )?;

        let rows = stmt.query_map(params![playbook_id], |r| {
            let kind_s: String = r.get(3)?;
            let talk: String = r
                .get::<_, Option<String>>(5)?
                .unwrap_or_else(|| "[]".into());
            let guard: String = r
                .get::<_, Option<String>>(6)?
                .unwrap_or_else(|| "[]".into());
            Ok(Objection {
                id: r.get(0)?,
                label: r.get(1)?,
                category: r.get(2)?,
                // Fail closed: a corrupt row must never become rebuttable.
                // Unreachable in practice — a CHECK constraint rejects any
                // other value, and the canary below turns it into a loud
                // `BadKind` rather than a silently mislabelled card.
                kind: ObjectionKind::parse(&kind_s).unwrap_or(ObjectionKind::Comply),
                response: r.get(4)?,
                talk_track: serde_json::from_str(&talk).unwrap_or_default(),
                guardrails: serde_json::from_str(&guard).unwrap_or_default(),
                rail_slot: r.get::<_, Option<i64>>(7)?.map(|v| v as u8),
                threshold_authored: r.get(8)?,
                threshold_learned: r.get(9)?,
                allow_refine: r.get::<_, i64>(10)? != 0,
                enabled: r.get::<_, i64>(11)? != 0,
                sort_order: r.get(12)?,
                triggers: vec![],
                negative_triggers: vec![],
                doc_refs: vec![],
                escalate_after_repeats: r.get::<_, Option<i64>>(13)?.map(|v| v as u32),
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            let mut o = row?;
            o.triggers = self.phrases("triggers", &o.id)?;
            o.negative_triggers = self.phrases("negative_triggers", &o.id)?;
            o.doc_refs = self.doc_refs(&o.id)?;
            out.push(o);
        }
        Ok(out)
    }

    /// Makes `table` hold exactly `wanted` for this objection, touching only
    /// the rows that actually changed. Authored phrases only — `learned`
    /// triggers (`docs/03-DATA-MODEL.md` §4) are owned by feedback, not by the
    /// playbook file, and must survive a re-import.
    fn sync_phrases(&self, table: &str, objection_id: &str, wanted: &[String]) -> Result<()> {
        let authored_only = if table == "triggers" {
            " AND source = 'authored'"
        } else {
            ""
        };

        let existing: Vec<(i64, String)> = {
            let sql =
                format!("SELECT id, phrase FROM {table} WHERE objection_id = ?1{authored_only}");
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params![objection_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };

        for (id, phrase) in &existing {
            if !wanted.contains(phrase) {
                let sql = format!("DELETE FROM {table} WHERE id = ?1");
                self.conn.execute(&sql, params![id])?;
            }
        }

        for phrase in wanted {
            if existing.iter().any(|(_, p)| p == phrase) {
                continue;
            }
            let sql = if table == "triggers" {
                format!("INSERT INTO {table} (objection_id, phrase, source) VALUES (?1, ?2, 'authored')")
            } else {
                format!("INSERT INTO {table} (objection_id, phrase) VALUES (?1, ?2)")
            };
            self.conn.execute(&sql, params![objection_id, phrase])?;
        }
        Ok(())
    }

    fn doc_refs(&self, objection_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT document_id FROM objection_docs WHERE objection_id = ?1
             ORDER BY document_id",
        )?;
        let rows = stmt
            .query_map(params![objection_id], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn phrases(&self, table: &str, objection_id: &str) -> Result<Vec<String>> {
        let sql = format!("SELECT phrase FROM {table} WHERE objection_id = ?1 ORDER BY id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![objection_id], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ─── Documents ───────────────────────────────────────────────────────

    /// Records a document declared by a playbook. Content may not exist yet —
    /// a bare `playbook.yaml` carries only the manifest. See D13.
    ///
    /// Re-declaring an already-ingested document keeps its bytes: the manifest
    /// describes it, it does not replace it.
    pub fn declare_document(
        &self,
        id: &str,
        playbook_id: &str,
        title: &str,
        filename: &str,
        sha256: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO documents
               (id, playbook_id, title, filename, mime, sha256, bytes,
                declared_at, ingested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL)
             ON CONFLICT(id) DO UPDATE SET
               playbook_id = excluded.playbook_id,
               title = excluded.title,
               filename = excluded.filename,
               mime = excluded.mime,
               sha256 = CASE WHEN excluded.sha256 = '' THEN documents.sha256
                             ELSE excluded.sha256 END",
            params![
                id,
                playbook_id,
                title,
                filename,
                mime_for(filename),
                sha256,
                now_ms()
            ],
        )?;
        Ok(())
    }

    /// Links an objection to a document it may cite during refinement.
    /// `03-DATA-MODEL.md` §3 `doc_refs`.
    pub fn link_objection_doc(&self, objection_id: &str, document_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO objection_docs (objection_id, document_id)
             VALUES (?1, ?2)",
            params![objection_id, document_id],
        )?;
        Ok(())
    }

    pub fn clear_objection_docs(&self, objection_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM objection_docs WHERE objection_id = ?1",
            params![objection_id],
        )?;
        Ok(())
    }

    pub fn list_documents(&self, playbook_id: &str) -> Result<Vec<DocumentSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, filename, mime, sha256, bytes IS NOT NULL
             FROM documents WHERE playbook_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![playbook_id], |r| {
                Ok(DocumentSummary {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    filename: r.get(2)?,
                    mime: r.get(3)?,
                    sha256: r.get(4)?,
                    ingested: r.get::<_, i64>(5)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ─── Settings ────────────────────────────────────────────────────────

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Settings are namespaced per playbook so importing a second playbook does
    /// not silently retune the first.
    pub fn playbook_setting_key(playbook_id: &str, name: &str) -> String {
        format!("playbook.{playbook_id}.{name}")
    }

    pub fn get_playbook_setting(&self, playbook_id: &str, name: &str) -> Result<Option<String>> {
        self.get_setting(&Self::playbook_setting_key(playbook_id, name))
    }

    pub fn set_playbook_setting(&self, playbook_id: &str, name: &str, value: &str) -> Result<()> {
        self.set_setting(&Self::playbook_setting_key(playbook_id, name), value)
    }

    // ─── Trigger embeddings ──────────────────────────────────────────────

    /// Every trigger phrase in the playbook that still needs an embedding for
    /// `model`, as `(table, row_id, phrase)`.
    pub fn phrases_needing_embedding(
        &self,
        playbook_id: &str,
        model: &str,
    ) -> Result<Vec<(&'static str, i64, String)>> {
        let mut out = Vec::new();
        for table in ["triggers", "negative_triggers"] as [&'static str; 2] {
            let sql = format!(
                "SELECT t.id, t.phrase FROM {table} t
                 JOIN objections o ON o.id = t.objection_id
                 WHERE o.playbook_id = ?1
                   AND (t.embedding IS NULL OR t.embed_model IS NOT ?2)
                 ORDER BY t.id"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params![playbook_id, model], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            out.extend(rows.into_iter().map(|(id, p)| (table, id, p)));
        }
        Ok(out)
    }

    pub fn set_embedding(&self, table: &str, row_id: i64, model: &str, vec: &[f32]) -> Result<()> {
        if table != "triggers" && table != "negative_triggers" && table != "doc_chunks" {
            return Err(StoreError::Other(format!(
                "not an embeddable table: {table}"
            )));
        }
        let sql = format!("UPDATE {table} SET embedding = ?1, embed_model = ?2 WHERE id = ?3");
        self.conn
            .execute(&sql, params![f32_to_blob(vec), model, row_id])?;
        Ok(())
    }

    /// All embedded trigger vectors for a playbook, grouped for the gate.
    pub fn load_trigger_vectors(
        &self,
        playbook_id: &str,
        model: &str,
    ) -> Result<Vec<TriggerVector>> {
        let mut out = Vec::new();
        for (table, positive) in [("triggers", true), ("negative_triggers", false)] {
            let sql = format!(
                "SELECT t.objection_id, t.phrase, t.embedding FROM {table} t
                 JOIN objections o ON o.id = t.objection_id
                 WHERE o.playbook_id = ?1 AND o.enabled = 1
                   AND t.embedding IS NOT NULL AND t.embed_model = ?2
                 ORDER BY t.id"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params![playbook_id, model], |r| {
                    Ok(TriggerVector {
                        objection_id: r.get(0)?,
                        phrase: r.get(1)?,
                        positive,
                        vector: blob_to_f32(&r.get::<_, Vec<u8>>(2)?),
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            out.extend(rows);
        }
        Ok(out)
    }

    // ─── Cards ───────────────────────────────────────────────────────────

    /// Records a card. `call_id` and `session_id` are optional so that manual
    /// rail use outside a call — the PRD's primary success metric — is
    /// recordable. See `docs/DECISIONS.md` D1.
    #[allow(clippy::too_many_arguments)]
    pub fn record_card(
        &self,
        id: &str,
        session_id: Option<&str>,
        call_id: Option<&str>,
        objection_id: &str,
        origin: CardOrigin,
        kind: ObjectionKind,
        layer_reached: u8,
        body_shown: &str,
        window_text: Option<&str>,
        similarity: Option<f32>,
        latency_gate_ms: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO cards
               (id, session_id, call_id, objection_id, origin, kind,
                layer_reached, body_shown, window_text, similarity,
                latency_gate_ms, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                id,
                session_id,
                call_id,
                objection_id,
                origin.as_str(),
                kind.as_str(),
                layer_reached as i64,
                body_shown,
                window_text,
                similarity,
                latency_gate_ms,
                now_ms(),
            ],
        )?;
        Ok(())
    }

    /// The PRD §7 headline split: rail use without AI vs. detected use.
    pub fn usage_split(&self) -> Result<UsageSplit> {
        let manual: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cards WHERE origin IN ('manual_hotkey','manual_click')",
            [],
            |r| r.get(0),
        )?;
        let detected: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cards WHERE origin = 'detected'",
            [],
            |r| r.get(0),
        )?;
        Ok(UsageSplit { manual, detected })
    }

    // ─── Compliance ──────────────────────────────────────────────────────

    pub fn log_removal_request(
        &self,
        call_id: Option<&str>,
        card_id: Option<&str>,
        triggering_utterance: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO removal_requests (call_id, card_id, requested_at, triggering_utterance)
             VALUES (?1, ?2, ?3, ?4)",
            params![call_id, card_id, now_ms(), triggering_utterance],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}

// ─── Value types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PlaybookSummary {
    pub id: String,
    pub name: String,
    pub schema_version: i64,
    pub is_active: bool,
    pub objection_count: i64,
}

#[derive(Debug, Clone)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub mime: String,
    pub sha256: String,
    /// `false` while only the playbook manifest is known. See D13.
    pub ingested: bool,
}

/// Best-effort MIME from a filename. `01-PRD.md` F9 scopes ingestion to PDF,
/// DOCX, MD and TXT; anything else is stored as octet-stream and will be
/// rejected by the ingester rather than here.
fn mime_for(filename: &str) -> &'static str {
    match filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, Clone)]
pub struct TriggerVector {
    pub objection_id: String,
    pub phrase: String,
    pub positive: bool,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct UsageSplit {
    pub manual: i64,
    pub detected: i64,
}

// ─── Blob helpers ────────────────────────────────────────────────────────

pub fn f32_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

pub fn blob_to_f32(b: &[u8]) -> Vec<f32> {
    // `as_chunks` rather than `chunks_exact`: it hands back fixed-size arrays,
    // so there is no per-element indexing for the compiler to bounds-check.
    // Stable since 1.88, which is this workspace's MSRV.
    b.as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(f32::from_le_bytes)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objection(id: &str, kind: ObjectionKind, slot: Option<u8>) -> Objection {
        Objection {
            id: id.into(),
            label: "L".into(),
            category: None,
            kind,
            response: "r".into(),
            talk_track: vec!["a".into()],
            guardrails: vec![],
            rail_slot: slot,
            threshold_authored: 0.62,
            threshold_learned: None,
            allow_refine: true,
            enabled: true,
            sort_order: 0,
            triggers: vec!["t one".into(), "t two".into()],
            negative_triggers: vec!["n one".into()],
            doc_refs: vec![],
            escalate_after_repeats: None,
        }
    }

    fn seeded() -> Store {
        let s = Store::open_in_memory().unwrap();
        s.upsert_playbook("pb", "Test", 1, "en-US").unwrap();
        s
    }

    #[test]
    fn migrations_apply_to_current() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    }

    /// The old test asserted only `schema_version() == 1` and never re-ran the
    /// migrator, so it did not test idempotency despite its name.
    #[test]
    fn migrations_are_idempotent() {
        let dir = std::env::temp_dir().join(format!("riposte-idem-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");

        let first = Store::open(&path).unwrap();
        first.upsert_playbook("pb", "T", 1, "en-US").unwrap();
        first
            .upsert_objection("pb", &objection("obj_a", ObjectionKind::Rebut, Some(1)))
            .unwrap();
        let v = first.schema_version().unwrap();
        drop(first);

        // Re-opening runs the migrator again over an already-migrated file.
        let second = Store::open(&path).unwrap();
        assert_eq!(second.schema_version().unwrap(), v, "migrator re-applied");
        assert_eq!(
            second.load_objections("pb").unwrap().len(),
            1,
            "data must survive a second migrator pass"
        );
        drop(second);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `docs/03-DATA-MODEL.md` §5: for every historical `schema_version`, a
    /// fixture migrates cleanly to current with all data preserved. The fixture
    /// is built by applying only migration 1, which is what a v1 install has on
    /// disk.
    #[test]
    fn migrates_from_a_v1_fixture_preserving_data() {
        let dir = std::env::temp_dir().join(format!("riposte-v1-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v1.db");

        // A v1 database: migration 1 only, then rows written through the v1
        // column set.
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_migrations (
                     version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL);",
            )
            .unwrap();
            conn.execute_batch(MIGRATIONS[0].1).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (1, 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO playbooks (id, name, schema_version, locale, is_active,
                                        created_at, updated_at)
                 VALUES ('pb', 'Legacy', 1, 'en-US', 1, 0, 0)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO objections (id, playbook_id, label, kind, response,
                                         rail_slot, threshold_authored)
                 VALUES ('obj_legacy', 'pb', 'Legacy', 'rebut', 'body', 1, 0.62)",
                [],
            )
            .unwrap();
        }

        let s = Store::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);

        let loaded = s.load_objections("pb").unwrap();
        assert_eq!(loaded.len(), 1, "v1 data must survive the migration");
        assert_eq!(loaded[0].id, "obj_legacy");
        assert_eq!(loaded[0].threshold_authored, 0.62);
        assert_eq!(
            loaded[0].escalate_after_repeats, None,
            "a column added in 0002 must read as None on a v1 row"
        );
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn comply_forces_allow_refine_off_even_when_asked_for() {
        let s = seeded();
        let mut o = objection("obj_dnc", ObjectionKind::Comply, Some(6));
        o.allow_refine = true; // as a hand-edited YAML might claim
        s.upsert_objection("pb", &o).unwrap();

        let stored: i64 = s
            .conn()
            .query_row(
                "SELECT allow_refine FROM objections WHERE id='obj_dnc'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 0, "comply objection must never allow refinement");
    }

    #[test]
    fn sql_trigger_backstops_a_direct_write() {
        let s = seeded();
        s.upsert_objection("pb", &objection("obj_dnc", ObjectionKind::Comply, None))
            .unwrap();
        // Bypass the Rust guard entirely.
        s.conn()
            .execute(
                "UPDATE objections SET allow_refine = 1 WHERE id='obj_dnc'",
                [],
            )
            .unwrap();
        let stored: i64 = s
            .conn()
            .query_row(
                "SELECT allow_refine FROM objections WHERE id='obj_dnc'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 0, "SQL trigger must revert a direct write");
    }

    #[test]
    fn round_trips_objections_with_triggers() {
        let s = seeded();
        s.upsert_objection("pb", &objection("obj_a", ObjectionKind::Rebut, Some(1)))
            .unwrap();
        let loaded = s.load_objections("pb").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].triggers, vec!["t one", "t two"]);
        assert_eq!(loaded[0].negative_triggers, vec!["n one"]);
        assert_eq!(loaded[0].talk_track, vec!["a"]);
    }

    #[test]
    fn rail_slot_is_unique_per_playbook() {
        let s = seeded();
        s.upsert_objection("pb", &objection("obj_a", ObjectionKind::Rebut, Some(1)))
            .unwrap();
        let err = s.upsert_objection("pb", &objection("obj_b", ObjectionKind::Rebut, Some(1)));
        assert!(err.is_err(), "two objections cannot share a rail slot");
    }

    #[test]
    fn manual_rail_use_is_recordable_without_a_call() {
        // The PRD's primary success metric. A NOT NULL call_id would make this
        // impossible — see docs/DECISIONS.md D1.
        let s = seeded();
        s.upsert_objection("pb", &objection("obj_a", ObjectionKind::Rebut, Some(1)))
            .unwrap();
        s.record_card(
            "card1",
            None,
            None,
            "obj_a",
            CardOrigin::ManualHotkey,
            ObjectionKind::Rebut,
            0,
            "body",
            None,
            None,
            None,
        )
        .unwrap();
        let split = s.usage_split().unwrap();
        assert_eq!(split.manual, 1);
        assert_eq!(split.detected, 0);
    }

    #[test]
    fn removal_requests_are_loggable() {
        let s = seeded();
        let id = s
            .log_removal_request(None, None, Some("take me off your list"))
            .unwrap();
        assert!(id > 0);
    }

    #[test]
    fn unchanged_phrases_keep_their_embeddings_across_a_reimport() {
        let s = seeded();
        let o = objection("obj_a", ObjectionKind::Rebut, Some(1));
        s.upsert_objection("pb", &o).unwrap();

        let pending = s.phrases_needing_embedding("pb", "m1").unwrap();
        assert_eq!(pending.len(), 3, "2 triggers + 1 negative");
        for (table, id, _) in &pending {
            s.set_embedding(table, *id, "m1", &[0.5, 0.5]).unwrap();
        }
        assert!(s.phrases_needing_embedding("pb", "m1").unwrap().is_empty());

        // Re-importing the same playbook must not invalidate the vectors.
        s.upsert_objection("pb", &o).unwrap();
        assert!(
            s.phrases_needing_embedding("pb", "m1").unwrap().is_empty(),
            "a re-import forced every vector to be recomputed"
        );
        assert_eq!(s.load_trigger_vectors("pb", "m1").unwrap().len(), 3);
    }

    #[test]
    fn edited_phrases_are_reconciled_not_duplicated() {
        let s = seeded();
        let mut o = objection("obj_a", ObjectionKind::Rebut, Some(1));
        s.upsert_objection("pb", &o).unwrap();

        o.triggers = vec!["t one".into(), "t three".into()];
        s.upsert_objection("pb", &o).unwrap();

        let loaded = s.load_objections("pb").unwrap();
        assert_eq!(loaded[0].triggers, vec!["t one", "t three"]);
    }

    /// Learned triggers come from feedback, not from the playbook file, so a
    /// re-import must not delete them. See `docs/03-DATA-MODEL.md` §4.
    #[test]
    fn a_reimport_preserves_learned_triggers() {
        let s = seeded();
        let o = objection("obj_a", ObjectionKind::Rebut, Some(1));
        s.upsert_objection("pb", &o).unwrap();
        s.conn()
            .execute(
                "INSERT INTO triggers (objection_id, phrase, source)
                 VALUES ('obj_a', 'learned from a thumbs up', 'learned')",
                [],
            )
            .unwrap();

        s.upsert_objection("pb", &o).unwrap();

        let learned: i64 = s
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM triggers WHERE objection_id='obj_a' AND source='learned'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(learned, 1, "re-import destroyed a learned trigger");
    }

    /// F13. `format!("card_{}", now_ms())` collided on presses inside one
    /// millisecond, and `cards.id` is a primary key — the lost press is exactly
    /// the event the PRD's headline metric counts.
    #[test]
    fn card_ids_do_not_collide_within_a_millisecond() {
        let ids: std::collections::HashSet<String> = (0..1000).map(|_| new_card_id()).collect();
        assert_eq!(ids.len(), 1000, "card ids collided");

        let s = seeded();
        s.upsert_objection("pb", &objection("obj_a", ObjectionKind::Rebut, Some(1)))
            .unwrap();
        for _ in 0..50 {
            s.record_card(
                &new_card_id(),
                None,
                None,
                "obj_a",
                CardOrigin::ManualHotkey,
                ObjectionKind::Rebut,
                0,
                "body",
                None,
                None,
                None,
            )
            .expect("a rapid repeat press must not be dropped");
        }
        assert_eq!(s.usage_split().unwrap().manual, 50);
    }

    /// F15. A `kind` the CHECK constraint could never admit must not load
    /// silently as a compliance card — it would turn an agent's rebuttal into a
    /// "do not respond" card with no indication anything was wrong.
    #[test]
    fn a_corrupt_kind_is_reported_rather_than_silently_downgraded() {
        let s = seeded();
        s.upsert_objection("pb", &objection("obj_a", ObjectionKind::Rebut, Some(1)))
            .unwrap();
        // Bypass the CHECK the only way possible: turn it off for this write.
        s.conn()
            .execute_batch("PRAGMA ignore_check_constraints = ON")
            .unwrap();
        s.conn()
            .execute(
                "UPDATE objections SET kind = 'nonsense' WHERE id = 'obj_a'",
                [],
            )
            .unwrap();
        s.conn()
            .execute_batch("PRAGMA ignore_check_constraints = OFF")
            .unwrap();

        match s.load_objections("pb") {
            Err(StoreError::BadKind(id)) => assert_eq!(id, "obj_a"),
            other => panic!("expected BadKind, got {other:?}"),
        }
    }

    /// F16. `upsert_objection` omits `threshold_learned` from its conflict
    /// clause, so a learned threshold survives a re-import. That is correct —
    /// the learned value is this machine's feedback history and the file only
    /// carries the authored default — but it was undocumented and untested, so
    /// nothing stopped someone "fixing" it.
    #[test]
    fn a_reimport_preserves_a_learned_threshold() {
        let s = seeded();
        let o = objection("obj_a", ObjectionKind::Rebut, Some(1));
        s.upsert_objection("pb", &o).unwrap();
        s.conn()
            .execute(
                "UPDATE objections SET threshold_learned = 0.77 WHERE id = 'obj_a'",
                [],
            )
            .unwrap();

        s.upsert_objection("pb", &o).unwrap();

        let loaded = s.load_objections("pb").unwrap();
        assert_eq!(
            loaded[0].threshold_learned,
            Some(0.77),
            "re-import erased learning"
        );
        assert_eq!(loaded[0].threshold_authored, 0.62);
        assert_eq!(loaded[0].effective_threshold(), 0.77);
    }

    #[test]
    fn embedding_blob_round_trips() {
        let v = vec![0.5f32, -0.25, 1.0];
        assert_eq!(blob_to_f32(&f32_to_blob(&v)), v);
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let s = seeded();
        let on: i64 = s
            .conn()
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(on, 1, "cascades are silent no-ops without this");
    }
}
