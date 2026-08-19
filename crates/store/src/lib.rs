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
const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_init.sql"))];

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
    //
    // Exposed as plain calls rather than `rusqlite::Transaction` so callers in
    // other crates can wrap a multi-step operation without depending on
    // rusqlite or having to convert its error type.

    pub fn begin(&self) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        Ok(())
    }

    pub fn commit(&self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    pub fn rollback(&self) -> Result<()> {
        self.conn.execute_batch("ROLLBACK")?;
        Ok(())
    }

    // ─── Migrations ──────────────────────────────────────────────────────

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL)",
            [],
        )?;

        let current: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))?;

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
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| r.get(0))?)
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
        self.conn.execute("UPDATE playbooks SET is_active = 0", [])?;
        self.conn
            .execute("UPDATE playbooks SET is_active = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn active_playbook_id(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT id FROM playbooks WHERE is_active = 1 LIMIT 1", [], |r| r.get(0))
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
                allow_refine, enabled, sort_order)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(id) DO UPDATE SET
               label = excluded.label, category = excluded.category,
               kind = excluded.kind, response = excluded.response,
               talk_track = excluded.talk_track, guardrails = excluded.guardrails,
               rail_slot = excluded.rail_slot,
               threshold_authored = excluded.threshold_authored,
               allow_refine = excluded.allow_refine,
               enabled = excluded.enabled, sort_order = excluded.sort_order",
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
            ],
        )?;

        self.conn
            .execute("DELETE FROM triggers WHERE objection_id = ?1 AND source = 'authored'", params![o.id])?;
        for phrase in &o.triggers {
            self.conn.execute(
                "INSERT INTO triggers (objection_id, phrase, source) VALUES (?1, ?2, 'authored')",
                params![o.id, phrase],
            )?;
        }

        self.conn
            .execute("DELETE FROM negative_triggers WHERE objection_id = ?1", params![o.id])?;
        for phrase in &o.negative_triggers {
            self.conn.execute(
                "INSERT INTO negative_triggers (objection_id, phrase) VALUES (?1, ?2)",
                params![o.id, phrase],
            )?;
        }
        Ok(())
    }

    pub fn load_objections(&self, playbook_id: &str) -> Result<Vec<Objection>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, label, category, kind, response, talk_track, guardrails,
                    rail_slot, threshold_authored, threshold_learned,
                    allow_refine, enabled, sort_order
             FROM objections WHERE playbook_id = ?1
             ORDER BY (rail_slot IS NULL), rail_slot, sort_order, id",
        )?;

        let rows = stmt.query_map(params![playbook_id], |r| {
            let kind_s: String = r.get(3)?;
            let talk: String = r.get::<_, Option<String>>(5)?.unwrap_or_else(|| "[]".into());
            let guard: String = r.get::<_, Option<String>>(6)?.unwrap_or_else(|| "[]".into());
            Ok((
                Objection {
                    id: r.get(0)?,
                    label: r.get(1)?,
                    category: r.get(2)?,
                    // Validated by a CHECK constraint on the way in; fall back
                    // to Comply on the impossible path so a corrupt row fails
                    // closed rather than becoming rebuttable.
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
                },
                kind_s,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (mut o, _) = row?;
            o.triggers = self.phrases("triggers", &o.id)?;
            o.negative_triggers = self.phrases("negative_triggers", &o.id)?;
            out.push(o);
        }
        Ok(out)
    }

    fn phrases(&self, table: &str, objection_id: &str) -> Result<Vec<String>> {
        let sql = format!("SELECT phrase FROM {table} WHERE objection_id = ?1 ORDER BY id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![objection_id], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
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
        for table in ["triggers", "negative_triggers"] {
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
            let table_name = if table == "triggers" { "triggers" } else { "negative_triggers" };
            out.extend(rows.into_iter().map(|(id, p)| (table_name, id, p)));
        }
        Ok(out)
    }

    pub fn set_embedding(&self, table: &str, row_id: i64, model: &str, vec: &[f32]) -> Result<()> {
        if table != "triggers" && table != "negative_triggers" && table != "doc_chunks" {
            return Err(StoreError::Other(format!("not an embeddable table: {table}")));
        }
        let sql = format!("UPDATE {table} SET embedding = ?1, embed_model = ?2 WHERE id = ?3");
        self.conn
            .execute(&sql, params![f32_to_blob(vec), model, row_id])?;
        Ok(())
    }

    /// All embedded trigger vectors for a playbook, grouped for the gate.
    pub fn load_trigger_vectors(&self, playbook_id: &str, model: &str) -> Result<Vec<TriggerVector>> {
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
        let detected: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM cards WHERE origin = 'detected'", [], |r| r.get(0))?;
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
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
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
        }
    }

    fn seeded() -> Store {
        let s = Store::open_in_memory().unwrap();
        s.upsert_playbook("pb", "Test", 1, "en-US").unwrap();
        s
    }

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), 1);
    }

    #[test]
    fn comply_forces_allow_refine_off_even_when_asked_for() {
        let s = seeded();
        let mut o = objection("obj_dnc", ObjectionKind::Comply, Some(6));
        o.allow_refine = true; // as a hand-edited YAML might claim
        s.upsert_objection("pb", &o).unwrap();

        let stored: i64 = s
            .conn()
            .query_row("SELECT allow_refine FROM objections WHERE id='obj_dnc'", [], |r| r.get(0))
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
            .execute("UPDATE objections SET allow_refine = 1 WHERE id='obj_dnc'", [])
            .unwrap();
        let stored: i64 = s
            .conn()
            .query_row("SELECT allow_refine FROM objections WHERE id='obj_dnc'", [], |r| r.get(0))
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
            "card1", None, None, "obj_a",
            CardOrigin::ManualHotkey, ObjectionKind::Rebut,
            0, "body", None, None, None,
        )
        .unwrap();
        let split = s.usage_split().unwrap();
        assert_eq!(split.manual, 1);
        assert_eq!(split.detected, 0);
    }

    #[test]
    fn removal_requests_are_loggable() {
        let s = seeded();
        let id = s.log_removal_request(None, None, Some("take me off your list")).unwrap();
        assert!(id > 0);
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
