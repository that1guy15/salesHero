//! Tauri IPC. A Layer 0 subset of `docs/04-CONTRACTS.md` §6.
//!
//! Only what the rail needs: read the playbook, record a use, edit a card,
//! toggle capture exclusion. No session, no capture, no providers — those
//! commands arrive with the milestones that implement them, and stubbing them
//! now would make the app look more finished than it is.

use crate::platform;
use crate::state::App;
use riposte_core::{CardOrigin, ObjectionKind};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Serialize)]
pub struct RailItem {
    pub objection_id: String,
    pub slot: u8,
    pub label: String,
    /// `rebut` | `comply` | `disqualify` — drives the colour, and comply drives
    /// a different layout entirely.
    pub kind: String,
    pub response: String,
    pub talk_track: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BelowFoldItem {
    pub objection_id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct Rail {
    pub items: Vec<RailItem>,
    pub below_fold: Vec<BelowFoldItem>,
    pub playbook_name: String,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub manual: i64,
    pub detected: i64,
}

#[derive(Debug, Serialize)]
pub struct Capability {
    /// False on any platform that cannot hide a window from capture. The UI
    /// shows the toggle disabled with an explanation rather than pretending.
    pub capture_exclusion: bool,
    pub platform: String,
}

#[derive(Debug, Deserialize)]
pub struct EditedCard {
    pub objection_id: String,
    pub response: String,
    pub talk_track: Vec<String>,
}

type Result<T> = std::result::Result<T, String>;

fn kind_str(k: ObjectionKind) -> String {
    k.as_str().to_string()
}

/// The rail. This is the whole product at Layer 0 — it must succeed with no
/// model, no network, and no audio device. See `AGENTS.md` invariant 3.
#[tauri::command]
pub fn get_rail(app: State<'_, App>) -> Result<Rail> {
    app.rail().map_err(|e| e.to_string())
}

/// Records a rail use and returns the card to expand.
///
/// `origin` distinguishes a hotkey from a click. Both are `layer_reached = 0`
/// and both are countable by the same query as a detected card, which is what
/// makes `01-PRD.md` §7's headline metric measurable at all.
#[tauri::command]
pub fn trigger_card(app: State<'_, App>, objection_id: String, origin: String) -> Result<RailItem> {
    let origin = match origin.as_str() {
        "manual_hotkey" => CardOrigin::ManualHotkey,
        "manual_click" => CardOrigin::ManualClick,
        other => return Err(format!("not a manual origin: {other}")),
    };
    app.trigger(&objection_id, origin)
        .map_err(|e| e.to_string())
}

/// Practice mode presses are deliberately *not* recorded.
///
/// The headline metric is rail use on live calls. Counting drills would inflate
/// it and destroy the only number that tells us whether the product works.
#[tauri::command]
pub fn practice_card(app: State<'_, App>, objection_id: String) -> Result<RailItem> {
    app.card(&objection_id).map_err(|e| e.to_string())
}

/// Edits a card in place. `01-PRD.md` §2 is explicit that the target user will
/// not author YAML but will happily retype a response in their own words — so
/// this is reachable from the card itself, not only from a console.
#[tauri::command]
pub fn save_card(app: State<'_, App>, edit: EditedCard) -> Result<()> {
    app.save_card(&edit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_stats(app: State<'_, App>) -> Result<Stats> {
    app.stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_capabilities() -> Capability {
    Capability {
        capture_exclusion: platform::supports_capture_exclusion(),
        platform: std::env::consts::OS.to_string(),
    }
}

/// Logs a removal request. `06-COMPLIANCE.md` §3 — the log exists so the agent
/// can enter it in their real dialer. Riposte does not suppress dialing and is
/// not a system of record; the UI says so.
#[tauri::command]
pub fn log_removal(app: State<'_, App>, objection_id: String) -> Result<()> {
    app.log_removal(&objection_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_capture_exclusion(
    window: tauri::WebviewWindow,
    app: State<'_, App>,
    enabled: bool,
) -> Result<bool> {
    let applied = platform::set_excluded_from_capture(&window, enabled);
    app.set_setting(
        "overlay.exclude_from_capture",
        if enabled { "1" } else { "0" },
    )
    .map_err(|e| e.to_string())?;
    Ok(applied)
}

#[tauri::command]
pub fn get_setting(app: State<'_, App>, key: String) -> Result<Option<String>> {
    app.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(app: State<'_, App>, key: String, value: String) -> Result<()> {
    app.set_setting(&key, &value).map_err(|e| e.to_string())
}

pub fn kind_of(k: ObjectionKind) -> String {
    kind_str(k)
}
