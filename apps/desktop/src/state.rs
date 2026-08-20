//! Application state: the store, the active playbook, and nothing else.
//!
//! Layer 0 has no engines to hold. That is the point — `AGENTS.md` invariant 3
//! requires the rail to render with ASR, embeddings and the LLM all
//! unavailable, and the simplest way to guarantee that is for the overlay to
//! have no way to reach them.

use crate::commands::{BelowFoldItem, EditedCard, Rail, RailItem, Stats};
use riposte_core::{CardOrigin, Objection};
use riposte_playbook::PlaybookFile;
use riposte_store::Store;
use std::sync::Mutex;

pub struct App {
    store: Mutex<Store>,
    playbook_id: Mutex<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Store(#[from] riposte_store::StoreError),
    #[error("{0}")]
    Playbook(#[from] riposte_playbook::PlaybookError),
    #[error("no objection '{0}' in the active playbook")]
    NoSuchObjection(String),
    #[error("{0}")]
    Other(String),
}

type Result<T> = std::result::Result<T, AppError>;

impl App {
    /// Opens the store and guarantees an active playbook exists.
    ///
    /// First run imports the bundled insurance playbook, so the app is useful
    /// with no setup at all — `01-PRD.md` F14. If that import fails the app
    /// still starts: an overlay with an empty rail is recoverable, a crash on
    /// launch is not.
    pub fn open(
        db_path: &std::path::Path,
        default_playbook: Option<&std::path::Path>,
    ) -> Result<Self> {
        let store = Store::open(db_path)?;

        if store.active_playbook_id()?.is_none() {
            if let Some(path) = default_playbook {
                match PlaybookFile::load(path) {
                    Ok(pb) => {
                        riposte_playbook::import(&store, &pb)?;
                    }
                    Err(e) => eprintln!("could not load the default playbook: {e}"),
                }
            }
        }

        let playbook_id = store.active_playbook_id()?.unwrap_or_default();
        Ok(App {
            store: Mutex::new(store),
            playbook_id: Mutex::new(playbook_id),
        })
    }

    fn with_store<T>(&self, f: impl FnOnce(&Store, &str) -> Result<T>) -> Result<T> {
        let store = self
            .store
            .lock()
            .map_err(|e| AppError::Other(e.to_string()))?;
        let id = self
            .playbook_id
            .lock()
            .map_err(|e| AppError::Other(e.to_string()))?
            .clone();
        f(&store, &id)
    }

    fn objections(store: &Store, playbook_id: &str) -> Result<Vec<Objection>> {
        Ok(store.load_objections(playbook_id)?)
    }

    fn to_item(o: &Objection) -> RailItem {
        RailItem {
            objection_id: o.id.clone(),
            slot: o.rail_slot.unwrap_or(0),
            label: o.label.clone(),
            kind: crate::commands::kind_of(o.kind),
            response: o.response.clone(),
            talk_track: o.talk_track.clone(),
        }
    }

    pub fn rail(&self) -> Result<Rail> {
        self.with_store(|store, id| {
            let objections = Self::objections(store, id)?;

            let mut items: Vec<RailItem> = objections
                .iter()
                .filter(|o| o.enabled && o.rail_slot.is_some())
                .map(Self::to_item)
                .collect();
            // Slot order, always. `05-UX-SPEC.md` §3: the rail never reorders
            // itself — an agent hits `3` from muscle memory without looking.
            items.sort_by_key(|i| i.slot);

            let below_fold = objections
                .iter()
                .filter(|o| o.enabled && o.rail_slot.is_none())
                .map(|o| BelowFoldItem {
                    objection_id: o.id.clone(),
                    label: o.label.clone(),
                    kind: crate::commands::kind_of(o.kind),
                })
                .collect();

            let playbook_name = store
                .list_playbooks()?
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| p.name)
                .unwrap_or_else(|| "No playbook".into());

            Ok(Rail {
                items,
                below_fold,
                playbook_name,
            })
        })
    }

    pub fn card(&self, objection_id: &str) -> Result<RailItem> {
        self.with_store(|store, id| {
            Self::objections(store, id)?
                .iter()
                .find(|o| o.id == objection_id)
                .map(Self::to_item)
                .ok_or_else(|| AppError::NoSuchObjection(objection_id.to_string()))
        })
    }

    pub fn trigger(&self, objection_id: &str, origin: CardOrigin) -> Result<RailItem> {
        self.with_store(|store, id| {
            let objections = Self::objections(store, id)?;
            let o = objections
                .iter()
                .find(|o| o.id == objection_id)
                .ok_or_else(|| AppError::NoSuchObjection(objection_id.to_string()))?;

            // No session and no call: hotkeys are live in the `No session` and
            // `Session, between calls` states (`05-UX-SPEC.md` §4), which is
            // exactly why both columns are nullable — `DECISIONS.md` D1.
            store.record_card(
                &riposte_store::new_card_id(),
                None,
                None,
                &o.id,
                origin,
                o.kind,
                0,
                &o.response,
                None,
                None,
                None,
            )?;
            Ok(Self::to_item(o))
        })
    }

    pub fn save_card(&self, edit: &EditedCard) -> Result<()> {
        self.with_store(|store, id| {
            let mut objections = Self::objections(store, id)?;
            let o = objections
                .iter_mut()
                .find(|o| o.id == edit.objection_id)
                .ok_or_else(|| AppError::NoSuchObjection(edit.objection_id.clone()))?;
            o.response = edit.response.clone();
            o.talk_track = edit.talk_track.clone();
            store.upsert_objection(id, o)?;
            Ok(())
        })
    }

    pub fn stats(&self) -> Result<Stats> {
        self.with_store(|store, _| {
            let s = store.usage_split()?;
            Ok(Stats {
                manual: s.manual,
                detected: s.detected,
            })
        })
    }

    pub fn log_removal(&self, objection_id: &str) -> Result<()> {
        self.with_store(|store, _| {
            store.log_removal_request(None, None, Some(objection_id))?;
            Ok(())
        })
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.with_store(|store, _| Ok(store.get_setting(key)?))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with_store(|store, _| {
            store.set_setting(key, value)?;
            Ok(())
        })
    }
}
