//! Playbook file format: parse, validate, import.
//! See `docs/03-DATA-MODEL.md` §3.
//!
//! The `.playbook` zip container lands in M7 alongside the console. For now
//! the unit of import is a bare `playbook.yaml`, which is what the default
//! insurance playbook ships as.

use riposte_core::{Objection, ObjectionKind};
use riposte_store::Store;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Highest `schema_version` this build understands.
pub const SUPPORTED_SCHEMA_VERSION: i64 = 1;

pub type Result<T> = std::result::Result<T, PlaybookError>;

#[derive(Debug, thiserror::Error)]
pub enum PlaybookError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("malformed playbook: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("store: {0}")]
    Store(#[from] riposte_store::StoreError),
    #[error(
        "playbook schema_version {found} is newer than this build supports \
         ({supported}) — update Riposte"
    )]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("invalid playbook: {0}")]
    Invalid(String),
}

// ─── File shape ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookFile {
    pub schema_version: i64,
    pub name: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default)]
    pub objections: Vec<ObjectionEntry>,
    #[serde(default)]
    pub documents: Vec<DocumentEntry>,
    #[serde(default)]
    pub settings: PlaybookSettings,
}

fn default_locale() -> String {
    "en-US".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectionEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub category: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub rail_slot: Option<u8>,
    #[serde(default = "default_threshold")]
    pub threshold: f32,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub negative_triggers: Vec<String>,
    #[serde(default)]
    pub response: String,
    #[serde(default)]
    pub talk_track: Vec<String>,
    #[serde(default)]
    pub guardrails: Vec<String>,
    #[serde(default)]
    pub doc_refs: Vec<String>,
    /// After this many fires within one call, serve the playbook's `comply`
    /// card instead of this objection's rebuttal. See `docs/DECISIONS.md` D8.
    #[serde(default)]
    pub escalate_after_repeats: Option<u32>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Present only so a hand-edited file can be shown to have no effect on a
    /// `comply` entry. Never trusted — see `docs/06-COMPLIANCE.md` §3.
    #[serde(default = "default_true")]
    pub allow_refine: bool,
}

fn default_threshold() -> f32 {
    0.62
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentEntry {
    pub id: String,
    pub title: String,
    pub filename: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookSettings {
    #[serde(default = "default_threshold")]
    pub gate_threshold_default: f32,
    #[serde(default = "default_dedupe")]
    pub dedupe_window_ms: u64,
    #[serde(default = "default_lifetime")]
    pub card_lifetime_ms: u64,
    #[serde(default = "default_preroll")]
    pub preroll_ms: u64,
}

fn default_dedupe() -> u64 {
    45_000
}
fn default_lifetime() -> u64 {
    90_000
}
fn default_preroll() -> u64 {
    10_000
}

impl PlaybookSettings {
    /// The knobs as storable `(name, value)` pairs. Kept next to the struct so
    /// adding a field and forgetting to persist it is a compile error rather
    /// than a silently ignored setting.
    pub fn as_pairs(&self) -> [(&'static str, String); 4] {
        [
            (
                "gate_threshold_default",
                self.gate_threshold_default.to_string(),
            ),
            ("dedupe_window_ms", self.dedupe_window_ms.to_string()),
            ("card_lifetime_ms", self.card_lifetime_ms.to_string()),
            ("preroll_ms", self.preroll_ms.to_string()),
        ]
    }
}

impl Default for PlaybookSettings {
    fn default() -> Self {
        PlaybookSettings {
            gate_threshold_default: default_threshold(),
            dedupe_window_ms: default_dedupe(),
            card_lifetime_ms: default_lifetime(),
            preroll_ms: default_preroll(),
        }
    }
}

// ─── Parse and validate ──────────────────────────────────────────────────

impl PlaybookFile {
    pub fn from_yaml(src: &str) -> Result<Self> {
        let pb: PlaybookFile = serde_yaml::from_str(src)?;
        pb.validate()?;
        Ok(pb)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let src = std::fs::read_to_string(path).map_err(|source| PlaybookError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_yaml(&src)
    }

    pub fn to_yaml(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version > SUPPORTED_SCHEMA_VERSION {
            return Err(PlaybookError::SchemaTooNew {
                found: self.schema_version,
                supported: SUPPORTED_SCHEMA_VERSION,
            });
        }

        let mut seen_ids = std::collections::HashSet::new();
        let mut seen_slots = std::collections::HashMap::new();

        for o in &self.objections {
            if ObjectionKind::parse(&o.kind).is_none() {
                return Err(PlaybookError::Invalid(format!(
                    "objection '{}': unknown kind '{}' (expected rebut, comply, or disqualify)",
                    o.id, o.kind
                )));
            }
            if !seen_ids.insert(&o.id) {
                return Err(PlaybookError::Invalid(format!(
                    "duplicate objection id '{}'",
                    o.id
                )));
            }
            if let Some(slot) = o.rail_slot {
                if !(1..=6).contains(&slot) {
                    return Err(PlaybookError::Invalid(format!(
                        "objection '{}': rail_slot {slot} out of range 1-6",
                        o.id
                    )));
                }
                // A clash is resolved on import, not rejected here:
                // `03-DATA-MODEL.md` §3 says the later entry wins and the
                // earlier is bumped below the fold with a warning.
                seen_slots.insert(slot, &o.id);
            }
            if o.triggers.is_empty() && o.enabled {
                return Err(PlaybookError::Invalid(format!(
                    "objection '{}' is enabled but has no trigger phrases",
                    o.id
                )));
            }
        }

        // A doc_ref that names nothing is a typo, and it used to be invisible
        // because doc_refs were parsed and discarded entirely.
        let declared: std::collections::HashSet<&String> =
            self.documents.iter().map(|d| &d.id).collect();
        for o in &self.objections {
            for r in &o.doc_refs {
                if !declared.contains(r) {
                    return Err(PlaybookError::Invalid(format!(
                        "objection '{}' references document '{r}', which is not \
                         declared in the documents block",
                        o.id
                    )));
                }
            }
        }

        let mut seen_docs = std::collections::HashSet::new();
        for d in &self.documents {
            if !seen_docs.insert(&d.id) {
                return Err(PlaybookError::Invalid(format!(
                    "duplicate document id '{}'",
                    d.id
                )));
            }
        }
        Ok(())
    }

    /// A stable id derived from the name, so re-importing the same file
    /// updates in place rather than accumulating duplicates.
    pub fn playbook_id(&self) -> String {
        let slug: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        format!("pb_{}", slug.trim_matches('_'))
    }
}

impl ObjectionEntry {
    pub fn to_objection(&self, sort_order: i64) -> Objection {
        let kind = ObjectionKind::parse(&self.kind).unwrap_or(ObjectionKind::Comply);
        Objection {
            id: self.id.clone(),
            label: self.label.clone(),
            category: self.category.clone(),
            kind,
            response: self.response.clone(),
            talk_track: self.talk_track.clone(),
            guardrails: self.guardrails.clone(),
            rail_slot: self.rail_slot,
            threshold_authored: self.threshold,
            threshold_learned: None,
            // Invariant 4: the file's own claim is overridden, not consulted.
            allow_refine: kind.allows_refinement() && self.allow_refine,
            enabled: self.enabled,
            sort_order,
            triggers: self.triggers.clone(),
            negative_triggers: self.negative_triggers.clone(),
            doc_refs: self.doc_refs.clone(),
            escalate_after_repeats: self.escalate_after_repeats,
        }
    }
}

// ─── Export ──────────────────────────────────────────────────────────────

impl PlaybookFile {
    /// Rebuilds a playbook file from the store.
    ///
    /// `07-BUILD-PLAN.md` M0 asks for a byte-identical import/export round
    /// trip. Against the *hand-authored* YAML that is not achievable and never
    /// was: the source carries comments, key ordering and block scalars that no
    /// serializer reproduces. See D14 for the restatement that is both
    /// achievable and worth having.
    ///
    /// Exports carry no transcripts, no feedback and no secrets — objections
    /// and documents only (`06-COMPLIANCE.md` §2). Nothing here reads those
    /// tables.
    pub fn from_store(store: &Store, playbook_id: &str) -> Result<Self> {
        let summary = store
            .list_playbooks()?
            .into_iter()
            .find(|p| p.id == playbook_id)
            .ok_or_else(|| PlaybookError::Invalid(format!("no playbook '{playbook_id}'")))?;

        let objections = store
            .load_objections(playbook_id)?
            .into_iter()
            .map(ObjectionEntry::from_objection)
            .collect();

        let documents = store
            .list_documents(playbook_id)?
            .into_iter()
            .map(|d| DocumentEntry {
                id: d.id,
                title: d.title,
                filename: d.filename,
                sha256: d.sha256,
            })
            .collect();

        let mut settings = PlaybookSettings::default();
        let get = |name: &str| -> Result<Option<String>> {
            Ok(store.get_playbook_setting(playbook_id, name)?)
        };
        if let Some(v) = get("gate_threshold_default")?.and_then(|v| v.parse().ok()) {
            settings.gate_threshold_default = v;
        }
        if let Some(v) = get("dedupe_window_ms")?.and_then(|v| v.parse().ok()) {
            settings.dedupe_window_ms = v;
        }
        if let Some(v) = get("card_lifetime_ms")?.and_then(|v| v.parse().ok()) {
            settings.card_lifetime_ms = v;
        }
        if let Some(v) = get("preroll_ms")?.and_then(|v| v.parse().ok()) {
            settings.preroll_ms = v;
        }

        Ok(PlaybookFile {
            schema_version: summary.schema_version,
            name: summary.name,
            locale: store
                .get_playbook_setting(playbook_id, "locale")?
                .unwrap_or_else(default_locale),
            objections,
            documents,
            settings,
        })
    }

    /// Writes the playbook to `path` as YAML.
    pub fn export(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        std::fs::write(path, self.to_yaml()?).map_err(|source| PlaybookError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}

impl ObjectionEntry {
    fn from_objection(o: Objection) -> Self {
        ObjectionEntry {
            id: o.id,
            label: o.label,
            category: o.category,
            kind: o.kind.as_str().to_string(),
            rail_slot: o.rail_slot,
            // The authored value, never the learned one. A learned threshold is
            // this machine's feedback history and does not belong in a file
            // shared with a team.
            threshold: o.threshold_authored,
            triggers: o.triggers,
            negative_triggers: o.negative_triggers,
            response: o.response,
            talk_track: o.talk_track,
            guardrails: o.guardrails,
            doc_refs: o.doc_refs,
            escalate_after_repeats: o.escalate_after_repeats,
            enabled: o.enabled,
            allow_refine: o.allow_refine,
        }
    }
}

// ─── Import ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ImportReport {
    pub playbook_id: String,
    pub name: String,
    pub objections_imported: usize,
    pub rail_slots_filled: usize,
    pub comply_entries: usize,
    pub documents_declared: usize,
    pub doc_links: usize,
    /// Non-fatal things the user should see: bumped rail slots, renamed or
    /// skipped objections. An import that silently resolves a conflict leaves
    /// the user unable to reason about what they have.
    pub warnings: Vec<String>,
}

/// What to do when an objection id already exists under a *different*
/// playbook. `03-DATA-MODEL.md` §3 and `04-CONTRACTS.md` §6 both call for the
/// user to choose.
///
/// Re-importing the same playbook is not a collision — that is an update, and
/// it is the normal path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnCollision {
    /// Take the id over. The previous owner loses the objection.
    #[default]
    Replace,
    /// Import under a suffixed id, leaving the original alone.
    KeepBoth,
    /// Leave the existing objection; do not import this entry.
    Skip,
}

/// Imports a playbook into the store and makes it active.
///
/// Atomic: a failure part-way leaves the database untouched. A partial import
/// leaves the user unable to reason about what they have.
pub fn import(store: &Store, pb: &PlaybookFile) -> Result<ImportReport> {
    import_with(store, pb, OnCollision::default())
}

pub fn import_with(
    store: &Store,
    pb: &PlaybookFile,
    on_collision: OnCollision,
) -> Result<ImportReport> {
    let playbook_id = pb.playbook_id();

    // Everything, including activation, inside one boundary. Activating after
    // the commit meant a failure there left an imported-but-inactive playbook —
    // an observable partial import, which is exactly what this promises cannot
    // happen.
    let mut warnings: Vec<String> = Vec::new();

    store.with_tx(|| -> Result<ImportReport> {
        let mut warnings = std::mem::take(&mut warnings);
        store.upsert_playbook(&playbook_id, &pb.name, pb.schema_version, &pb.locale)?;

        // Declared before the objections that reference them: objection_docs
        // has a foreign key onto documents.
        for d in &pb.documents {
            store.declare_document(&d.id, &playbook_id, &d.title, &d.filename, &d.sha256)?;
        }

        let mut comply = 0;
        let mut slots = 0;
        let mut doc_links = 0;
        let mut imported = 0;

        // Rail slots, resolved before anything is written. A later entry wins
        // the slot and the earlier one is bumped below the fold, per
        // `03-DATA-MODEL.md` §3. Rejecting the whole import over a slot clash
        // — the previous behaviour — is not what the spec asks for and is
        // hostile to someone hand-editing a file.
        let mut slot_owner: std::collections::HashMap<u8, usize> = Default::default();
        for (i, e) in pb.objections.iter().enumerate() {
            if let Some(slot) = e.rail_slot {
                if let Some(prev) = slot_owner.insert(slot, i) {
                    warnings.push(format!(
                        "rail slot {slot}: '{}' takes it, '{}' moved below the fold",
                        e.id, pb.objections[prev].id
                    ));
                }
            }
        }

        for (i, entry) in pb.objections.iter().enumerate() {
            let mut o = entry.to_objection(i as i64);

            if let Some(slot) = o.rail_slot {
                if slot_owner.get(&slot) != Some(&i) {
                    o.rail_slot = None;
                }
            }

            // An id already held by a different playbook.
            if let Some(owner) = store.objection_owner(&o.id)? {
                if owner != playbook_id {
                    match on_collision {
                        OnCollision::Skip => {
                            warnings.push(format!(
                                "objection '{}' already belongs to playbook '{owner}' — skipped",
                                o.id
                            ));
                            continue;
                        }
                        OnCollision::KeepBoth => {
                            let renamed = format!("{}__{}", o.id, playbook_id);
                            warnings.push(format!(
                                "objection '{}' already belongs to playbook '{owner}' — \
                                 imported as '{renamed}'",
                                o.id
                            ));
                            o.id = renamed;
                        }
                        OnCollision::Replace => {
                            warnings.push(format!(
                                "objection '{}' taken over from playbook '{owner}'",
                                o.id
                            ));
                        }
                    }
                }
            }

            if o.kind == ObjectionKind::Comply {
                comply += 1;
            }
            if o.rail_slot.is_some() {
                slots += 1;
            }
            imported += 1;
            store.upsert_objection(&playbook_id, &o)?;

            store.clear_objection_docs(&o.id)?;
            for r in &o.doc_refs {
                store.link_objection_doc(&o.id, r)?;
                doc_links += 1;
            }
        }

        // The playbook's tuning knobs, which were parsed and discarded.
        // Namespaced per playbook so a second import does not retune the first.
        for (name, value) in pb.settings.as_pairs() {
            store.set_playbook_setting(&playbook_id, name, &value)?;
        }

        store.set_active_playbook(&playbook_id)?;

        Ok(ImportReport {
            playbook_id: playbook_id.clone(),
            name: pb.name.clone(),
            objections_imported: imported,
            rail_slots_filled: slots,
            comply_entries: comply,
            documents_declared: pb.documents.len(),
            doc_links,
            warnings,
        })
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_YAML: &str = include_str!("../../../playbooks/insurance-cold-call.yaml");

    #[test]
    fn parses_the_shipped_default_playbook() {
        let pb = PlaybookFile::from_yaml(DEFAULT_YAML).expect("default playbook must parse");
        assert_eq!(pb.schema_version, 1);
        assert_eq!(pb.objections.len(), 10);

        let rail: Vec<_> = pb
            .objections
            .iter()
            .filter(|o| o.rail_slot.is_some())
            .collect();
        assert_eq!(rail.len(), 6, "six rail slots per docs/01-PRD.md §5");
    }

    #[test]
    fn dnc_entry_is_comply_and_sits_on_slot_six() {
        let pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        let dnc = pb.objections.iter().find(|o| o.id == "obj_dnc").unwrap();
        assert_eq!(dnc.kind, "comply");
        assert_eq!(dnc.rail_slot, Some(6));
    }

    #[test]
    fn imports_the_default_playbook_into_a_store() {
        let store = Store::open_in_memory().unwrap();
        let pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        let report = import(&store, &pb).unwrap();

        assert_eq!(report.objections_imported, 10);
        assert_eq!(report.rail_slots_filled, 6);
        assert_eq!(report.comply_entries, 1);
        assert_eq!(
            store.active_playbook_id().unwrap().as_deref(),
            Some(report.playbook_id.as_str())
        );

        let loaded = store.load_objections(&report.playbook_id).unwrap();
        assert_eq!(loaded.len(), 10);
        // Rail items sort first, in slot order.
        assert_eq!(loaded[0].rail_slot, Some(1));
        assert_eq!(loaded[5].rail_slot, Some(6));
        assert!(loaded[6].rail_slot.is_none());
    }

    #[test]
    fn hand_edited_allow_refine_cannot_enable_a_comply_objection() {
        let yaml = r#"
schema_version: 1
name: "Attack"
objections:
  - id: obj_dnc
    label: "Take me off your list"
    kind: comply
    allow_refine: true
    triggers: ["stop calling me"]
    response: "Confirm removal and end the call."
"#;
        let pb = PlaybookFile::from_yaml(yaml).unwrap();
        let o = pb.objections[0].to_objection(0);
        assert!(
            !o.allow_refine,
            "file contents must not override invariant 4"
        );

        let store = Store::open_in_memory().unwrap();
        import(&store, &pb).unwrap();
        let stored: i64 = store
            .conn()
            .query_row(
                "SELECT allow_refine FROM objections WHERE id='obj_dnc'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 0);
    }

    /// D8: the two prose guardrails in the shipped playbook are backed by a
    /// machine-readable field, and it survives a round trip through the store.
    #[test]
    fn shipped_playbook_arms_repeat_escalation() {
        let pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        for id in ["obj_no_form", "obj_not_interested"] {
            let o = pb.objections.iter().find(|o| o.id == id).unwrap();
            assert_eq!(
                o.escalate_after_repeats,
                Some(2),
                "{id} carries a repeat guardrail in prose but does not enforce it"
            );
        }

        let store = Store::open_in_memory().unwrap();
        let report = import(&store, &pb).unwrap();
        let loaded = store.load_objections(&report.playbook_id).unwrap();
        let no_form = loaded.iter().find(|o| o.id == "obj_no_form").unwrap();
        assert_eq!(no_form.escalate_after_repeats, Some(2));
        let price = loaded.iter().find(|o| o.id == "obj_price_first").unwrap();
        assert_eq!(price.escalate_after_repeats, None, "default must stay off");
    }

    /// F10/F12: `doc_refs` and the `documents:` block were parsed and then
    /// silently dropped — `objection_docs` had no writer at all, and
    /// `load_objections` hardcoded an empty `doc_refs`.
    #[test]
    fn doc_refs_and_documents_survive_an_import() {
        let store = Store::open_in_memory().unwrap();
        let pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        let report = import(&store, &pb).unwrap();

        assert_eq!(report.documents_declared, 2);
        assert_eq!(
            report.doc_links, 4,
            "3 objections reference docs, one twice over"
        );

        let docs = store.list_documents(&report.playbook_id).unwrap();
        assert_eq!(docs.len(), 2);
        let lead = docs.iter().find(|d| d.id == "doc_lead_sources").unwrap();
        assert_eq!(lead.title, "Where our leads come from");
        assert_eq!(lead.mime, "text/markdown", "mime derived from the filename");
        assert!(
            !lead.ingested,
            "a bare YAML declares documents, it does not ingest them"
        );

        let loaded = store.load_objections(&report.playbook_id).unwrap();
        let no_form = loaded.iter().find(|o| o.id == "obj_no_form").unwrap();
        assert_eq!(no_form.doc_refs, vec!["doc_lead_sources"]);
        let expensive = loaded.iter().find(|o| o.id == "obj_too_expensive").unwrap();
        assert_eq!(expensive.doc_refs, vec!["doc_carrier_comparison"]);
        let bad_time = loaded.iter().find(|o| o.id == "obj_bad_time").unwrap();
        assert!(bad_time.doc_refs.is_empty());
    }

    /// F11: the `settings:` block was parsed and discarded while the gate used
    /// hardcoded constants.
    #[test]
    fn playbook_settings_are_persisted_and_readable() {
        let store = Store::open_in_memory().unwrap();
        let mut pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        pb.settings.dedupe_window_ms = 20_000;
        let report = import(&store, &pb).unwrap();

        assert_eq!(
            store
                .get_playbook_setting(&report.playbook_id, "dedupe_window_ms")
                .unwrap()
                .as_deref(),
            Some("20000")
        );
        assert_eq!(
            store
                .get_playbook_setting(&report.playbook_id, "preroll_ms")
                .unwrap()
                .as_deref(),
            Some("10000")
        );
    }

    /// Settings are namespaced per playbook, so importing a second one does not
    /// silently retune the first.
    #[test]
    fn a_second_playbook_does_not_retune_the_first() {
        let store = Store::open_in_memory().unwrap();
        let mut a = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        a.settings.dedupe_window_ms = 11_000;
        let ra = import(&store, &a).unwrap();

        let mut b = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        b.name = "Second".into();
        b.settings.dedupe_window_ms = 22_000;
        let rb = import(&store, &b).unwrap();

        assert_eq!(
            store
                .get_playbook_setting(&ra.playbook_id, "dedupe_window_ms")
                .unwrap()
                .as_deref(),
            Some("11000")
        );
        assert_eq!(
            store
                .get_playbook_setting(&rb.playbook_id, "dedupe_window_ms")
                .unwrap()
                .as_deref(),
            Some("22000")
        );
    }

    #[test]
    fn rejects_a_doc_ref_that_names_no_document() {
        let yaml = r#"
schema_version: 1
name: "Typo"
objections:
  - id: a
    label: A
    kind: rebut
    triggers: ["x"]
    response: ""
    doc_refs: [doc_typo]
documents:
  - { id: doc_real, title: T, filename: t.md }
"#;
        let err = PlaybookFile::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("doc_typo"), "got: {err}");
    }

    // ─── Round trip ──────────────────────────────────────────────────────

    /// The M0 acceptance criterion, restated per D14. A hand-authored file
    /// cannot survive a byte-identical round trip — comments and block scalars
    /// do not serialize back — but an *exported* one must, or export is not a
    /// dependable way to move a playbook between machines.
    #[test]
    fn export_import_export_is_byte_identical() {
        let store = Store::open_in_memory().unwrap();
        let pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        let id = import(&store, &pb).unwrap().playbook_id;

        let first = PlaybookFile::from_store(&store, &id)
            .unwrap()
            .to_yaml()
            .unwrap();

        let store2 = Store::open_in_memory().unwrap();
        let reparsed = PlaybookFile::from_yaml(&first).unwrap();
        let id2 = import(&store2, &reparsed).unwrap().playbook_id;
        let second = PlaybookFile::from_store(&store2, &id2)
            .unwrap()
            .to_yaml()
            .unwrap();

        assert_eq!(first, second, "export is not stable across a round trip");
    }

    /// Byte-stability is worth little if fields are being dropped on the way
    /// through. This asserts the content actually survives.
    #[test]
    fn a_round_trip_preserves_every_authored_field() {
        let store = Store::open_in_memory().unwrap();
        let original = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        let id = import(&store, &original).unwrap().playbook_id;
        let exported = PlaybookFile::from_store(&store, &id).unwrap();

        assert_eq!(exported.schema_version, original.schema_version);
        assert_eq!(exported.name, original.name);
        assert_eq!(exported.objections.len(), original.objections.len());
        assert_eq!(exported.documents.len(), original.documents.len());
        assert_eq!(
            exported.settings.dedupe_window_ms,
            original.settings.dedupe_window_ms
        );
        assert_eq!(exported.settings.preroll_ms, original.settings.preroll_ms);

        for want in &original.objections {
            let got = exported
                .objections
                .iter()
                .find(|o| o.id == want.id)
                .unwrap_or_else(|| panic!("objection '{}' was lost", want.id));
            assert_eq!(got.label, want.label, "{}", want.id);
            assert_eq!(got.kind, want.kind, "{}", want.id);
            assert_eq!(got.rail_slot, want.rail_slot, "{}", want.id);
            assert_eq!(got.threshold, want.threshold, "{}", want.id);
            assert_eq!(got.triggers, want.triggers, "{}", want.id);
            assert_eq!(got.negative_triggers, want.negative_triggers, "{}", want.id);
            assert_eq!(got.talk_track, want.talk_track, "{}", want.id);
            assert_eq!(got.guardrails, want.guardrails, "{}", want.id);
            assert_eq!(got.doc_refs, want.doc_refs, "{}", want.id);
            assert_eq!(got.category, want.category, "{}", want.id);
            assert_eq!(
                got.escalate_after_repeats, want.escalate_after_repeats,
                "{}",
                want.id
            );
            assert_eq!(got.response.trim(), want.response.trim(), "{}", want.id);
        }
    }

    /// A learned threshold is this machine's feedback history. It must not
    /// travel in a file shared with a team — `06-COMPLIANCE.md` §2, "exports
    /// contain no secrets, no transcripts, no feedback".
    #[test]
    fn export_carries_the_authored_threshold_not_the_learned_one() {
        let store = Store::open_in_memory().unwrap();
        let pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        let id = import(&store, &pb).unwrap().playbook_id;
        store
            .conn()
            .execute(
                "UPDATE objections SET threshold_learned = 0.81 WHERE id = 'obj_no_form'",
                [],
            )
            .unwrap();

        let exported = PlaybookFile::from_store(&store, &id).unwrap();
        let o = exported
            .objections
            .iter()
            .find(|o| o.id == "obj_no_form")
            .unwrap();
        assert_eq!(
            o.threshold, 0.62,
            "the learned threshold leaked into the export"
        );
    }

    #[test]
    fn rejects_a_newer_schema_version() {
        let yaml = "schema_version: 99\nname: Future\nobjections: []\n";
        assert!(matches!(
            PlaybookFile::from_yaml(yaml),
            Err(PlaybookError::SchemaTooNew { found: 99, .. })
        ));
    }

    /// `03-DATA-MODEL.md` §3: "later entry wins, earlier is bumped below fold,
    /// warn the user". This used to reject the whole import instead, which is
    /// hostile to anyone hand-editing a file and is not what the spec says.
    #[test]
    fn a_duplicate_rail_slot_bumps_the_earlier_entry_and_warns() {
        let yaml = r#"
schema_version: 1
name: "Clash"
objections:
  - { id: a, label: A, kind: rebut, rail_slot: 1, triggers: ["x"], response: "" }
  - { id: b, label: B, kind: rebut, rail_slot: 1, triggers: ["y"], response: "" }
"#;
        let pb = PlaybookFile::from_yaml(yaml).expect("a slot clash is resolved, not rejected");
        let store = Store::open_in_memory().unwrap();
        let report = import(&store, &pb).unwrap();

        assert_eq!(report.rail_slots_filled, 1);
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains("rail slot 1"),
            "{:?}",
            report.warnings
        );

        let loaded = store.load_objections(&report.playbook_id).unwrap();
        let a = loaded.iter().find(|o| o.id == "a").unwrap();
        let b = loaded.iter().find(|o| o.id == "b").unwrap();
        assert_eq!(b.rail_slot, Some(1), "the later entry wins the slot");
        assert_eq!(
            a.rail_slot, None,
            "the earlier entry is bumped below the fold"
        );
    }

    /// A rail slot outside 1-6 is still a hard error — that is a malformed
    /// file, not a resolvable conflict.
    #[test]
    fn rejects_a_rail_slot_out_of_range() {
        let yaml = r#"
schema_version: 1
name: "Bad"
objections:
  - { id: a, label: A, kind: rebut, rail_slot: 9, triggers: ["x"], response: "" }
"#;
        let err = PlaybookFile::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("out of range"), "got: {err}");
    }

    // ─── Objection id collisions ─────────────────────────────────────────

    fn two_playbooks_sharing_an_id() -> (PlaybookFile, PlaybookFile) {
        let mk = |name: &str| {
            PlaybookFile::from_yaml(&format!(
                r#"
schema_version: 1
name: "{name}"
objections:
  - {{ id: obj_shared, label: "{name}", kind: rebut, triggers: ["x"], response: "" }}
"#
            ))
            .unwrap()
        };
        (mk("First"), mk("Second"))
    }

    #[test]
    fn collision_replace_takes_the_id_over() {
        let store = Store::open_in_memory().unwrap();
        let (a, b) = two_playbooks_sharing_an_id();
        let ra = import_with(&store, &a, OnCollision::Replace).unwrap();
        let rb = import_with(&store, &b, OnCollision::Replace).unwrap();

        assert!(
            rb.warnings.iter().any(|w| w.contains("taken over")),
            "{:?}",
            rb.warnings
        );
        assert_eq!(
            store.objection_owner("obj_shared").unwrap().as_deref(),
            Some(rb.playbook_id.as_str())
        );
        assert!(store.load_objections(&ra.playbook_id).unwrap().is_empty());
    }

    #[test]
    fn collision_keep_both_suffixes_the_id() {
        let store = Store::open_in_memory().unwrap();
        let (a, b) = two_playbooks_sharing_an_id();
        let ra = import_with(&store, &a, OnCollision::Replace).unwrap();
        let rb = import_with(&store, &b, OnCollision::KeepBoth).unwrap();

        assert_eq!(
            store.load_objections(&ra.playbook_id).unwrap().len(),
            1,
            "original untouched"
        );
        let second = store.load_objections(&rb.playbook_id).unwrap();
        assert_eq!(second.len(), 1);
        assert_ne!(second[0].id, "obj_shared");
        assert!(
            second[0].id.starts_with("obj_shared__"),
            "got {}",
            second[0].id
        );
    }

    #[test]
    fn collision_skip_leaves_the_existing_objection() {
        let store = Store::open_in_memory().unwrap();
        let (a, b) = two_playbooks_sharing_an_id();
        let ra = import_with(&store, &a, OnCollision::Replace).unwrap();
        let rb = import_with(&store, &b, OnCollision::Skip).unwrap();

        assert_eq!(rb.objections_imported, 0);
        assert!(
            rb.warnings.iter().any(|w| w.contains("skipped")),
            "{:?}",
            rb.warnings
        );
        let kept = store.load_objections(&ra.playbook_id).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].label, "First",
            "the existing objection was overwritten"
        );
    }

    /// Re-importing the same playbook is an update, not a collision. This is
    /// the normal path and must stay silent.
    #[test]
    fn reimporting_the_same_playbook_is_not_a_collision() {
        let store = Store::open_in_memory().unwrap();
        let pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        import(&store, &pb).unwrap();
        let second = import(&store, &pb).unwrap();
        assert!(second.warnings.is_empty(), "{:?}", second.warnings);
        assert_eq!(second.objections_imported, 10);
    }

    #[test]
    fn rejects_an_unknown_kind() {
        let yaml = r#"
schema_version: 1
name: "Bad"
objections:
  - { id: a, label: A, kind: destroy, triggers: ["x"], response: "" }
"#;
        let err = PlaybookFile::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown kind"), "got: {err}");
    }

    /// The test above reaches SQL only because validation is bypassed by hand.
    /// This one fails *part way through* a valid import — entry 4 of 10 hits a
    /// constraint — so the rollback has real work to undo.
    #[test]
    fn a_failure_part_way_through_leaves_nothing_behind() {
        let store = Store::open_in_memory().unwrap();
        let mut pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        // `validate` runs in `from_yaml`, so mutating afterwards reaches SQL.
        // Entry 4 of 10 takes a rail_slot outside the CHECK's 1-6 range, so the
        // playbook row and entries 0-3 are already written when it fails.
        pb.objections[4].rail_slot = Some(9);
        assert!(
            import(&store, &pb).is_err(),
            "SQL CHECK should reject rail_slot 9"
        );

        assert_eq!(
            store.list_playbooks().unwrap().len(),
            0,
            "the playbook row written before the failure was not rolled back"
        );
        assert!(
            store.active_playbook_id().unwrap().is_none(),
            "a failed import must not leave an active playbook"
        );
    }

    /// Activation is inside the transaction boundary, so a successful import
    /// activates and a failed one leaves the previous active playbook alone.
    #[test]
    fn a_failed_import_does_not_disturb_the_active_playbook() {
        let store = Store::open_in_memory().unwrap();
        let good = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        let first = import(&store, &good).unwrap();
        assert_eq!(
            store.active_playbook_id().unwrap().as_deref(),
            Some(first.playbook_id.as_str())
        );

        let mut bad = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        bad.name = "Other".into();
        bad.objections[4].rail_slot = Some(9);
        assert!(import(&store, &bad).is_err());

        assert_eq!(
            store.active_playbook_id().unwrap().as_deref(),
            Some(first.playbook_id.as_str()),
            "a failed import stole the active flag"
        );
    }
}
