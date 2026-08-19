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
                return Err(PlaybookError::Invalid(format!("duplicate objection id '{}'", o.id)));
            }
            if let Some(slot) = o.rail_slot {
                if !(1..=6).contains(&slot) {
                    return Err(PlaybookError::Invalid(format!(
                        "objection '{}': rail_slot {slot} out of range 1-6",
                        o.id
                    )));
                }
                if let Some(prev) = seen_slots.insert(slot, &o.id) {
                    return Err(PlaybookError::Invalid(format!(
                        "rail_slot {slot} claimed by both '{prev}' and '{}'",
                        o.id
                    )));
                }
            }
            if o.triggers.is_empty() && o.enabled {
                return Err(PlaybookError::Invalid(format!(
                    "objection '{}' is enabled but has no trigger phrases",
                    o.id
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
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
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
}

/// Imports a playbook into the store and makes it active.
///
/// Atomic: a failure part-way leaves the database untouched. A partial import
/// leaves the user unable to reason about what they have.
pub fn import(store: &Store, pb: &PlaybookFile) -> Result<ImportReport> {
    let playbook_id = pb.playbook_id();

    store.begin()?;
    let result = (|| -> Result<ImportReport> {
        store.upsert_playbook(&playbook_id, &pb.name, pb.schema_version, &pb.locale)?;

        let mut comply = 0;
        let mut slots = 0;
        for (i, entry) in pb.objections.iter().enumerate() {
            let o = entry.to_objection(i as i64);
            if o.kind == ObjectionKind::Comply {
                comply += 1;
            }
            if o.rail_slot.is_some() {
                slots += 1;
            }
            store.upsert_objection(&playbook_id, &o)?;
        }

        Ok(ImportReport {
            playbook_id: playbook_id.clone(),
            name: pb.name.clone(),
            objections_imported: pb.objections.len(),
            rail_slots_filled: slots,
            comply_entries: comply,
        })
    })();

    match result {
        Ok(report) => {
            store.commit()?;
            store.set_active_playbook(&playbook_id)?;
            Ok(report)
        }
        Err(e) => {
            let _ = store.rollback();
            Err(e)
        }
    }
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

        let rail: Vec<_> = pb.objections.iter().filter(|o| o.rail_slot.is_some()).collect();
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
        assert_eq!(store.active_playbook_id().unwrap().as_deref(), Some(report.playbook_id.as_str()));

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
        assert!(!o.allow_refine, "file contents must not override invariant 4");

        let store = Store::open_in_memory().unwrap();
        import(&store, &pb).unwrap();
        let stored: i64 = store
            .conn()
            .query_row("SELECT allow_refine FROM objections WHERE id='obj_dnc'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, 0);
    }

    #[test]
    fn rejects_a_newer_schema_version() {
        let yaml = "schema_version: 99\nname: Future\nobjections: []\n";
        assert!(matches!(
            PlaybookFile::from_yaml(yaml),
            Err(PlaybookError::SchemaTooNew { found: 99, .. })
        ));
    }

    #[test]
    fn rejects_duplicate_rail_slots() {
        let yaml = r#"
schema_version: 1
name: "Clash"
objections:
  - { id: a, label: A, kind: rebut, rail_slot: 1, triggers: ["x"], response: "" }
  - { id: b, label: B, kind: rebut, rail_slot: 1, triggers: ["y"], response: "" }
"#;
        let err = PlaybookFile::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("rail_slot 1"), "got: {err}");
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

    #[test]
    fn import_is_atomic() {
        // Second entry collides on rail_slot at the SQL layer; nothing should land.
        let store = Store::open_in_memory().unwrap();
        let mut pb = PlaybookFile::from_yaml(DEFAULT_YAML).unwrap();
        pb.objections[1].rail_slot = Some(1); // now clashes with entry 0
        // Validation catches it before SQL does, which is the better error.
        assert!(pb.validate().is_err());

        // Force past validation to prove the transaction also holds.
        let report = import(&store, &pb);
        assert!(report.is_err());
        assert_eq!(store.list_playbooks().unwrap().len(), 0, "rollback must leave nothing behind");
    }
}
