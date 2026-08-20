//! Riposte overlay — M1.7. **Layer 0 only.**
//!
//! No audio, no ASR, no embeddings, no LLM, no detection. The rail renders from
//! the playbook and nothing else, which is the whole architectural bet
//! (`02-ARCHITECTURE.md` §1) and the thing `AGENTS.md` invariant 3 requires.
//!
//! `01-PRD.md` §7's first row — rail use without AI assist — is measurable the
//! moment this ships, and it needs none of M2 through M5.

pub mod commands;
pub mod platform;
pub mod state;

use state::App;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

/// Hotkeys are `Ctrl+Alt+1`–`6`, not bare digits.
///
/// `05-UX-SPEC.md` §3 asks for `1`–`6` working without focus. Registered
/// globally, bare digits are hijacked system-wide — the agent could not type a
/// phone extension, a policy number, or a `3` into their dialer.
/// `PROJECT-STATUS.md` §4.2 flagged this as needing resolution before any M6
/// work; this is the resolution. See `docs/DECISIONS.md` D18.
const HOTKEY_MODIFIERS: Modifiers = Modifiers::CONTROL.union(Modifiers::ALT);

const SLOT_CODES: [(u8, Code); 6] = [
    (1, Code::Digit1),
    (2, Code::Digit2),
    (3, Code::Digit3),
    (4, Code::Digit4),
    (5, Code::Digit5),
    (6, Code::Digit6),
];

fn data_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// The bundled playbook, so first run needs no setup at all (`01-PRD.md` F14).
fn bundled_playbook(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    if let Ok(p) = app.path().resolve(
        "playbooks/insurance-cold-call.yaml",
        tauri::path::BaseDirectory::Resource,
    ) {
        if p.exists() {
            return Some(p);
        }
    }
    // Running from the workspace during development.
    let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../playbooks/insurance-cold-call.yaml");
    dev.exists().then_some(dev)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_rail,
            commands::trigger_card,
            commands::practice_card,
            commands::save_card,
            commands::get_stats,
            commands::get_capabilities,
            commands::log_removal,
            commands::set_capture_exclusion,
            commands::get_setting,
            commands::set_setting,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            let dir = data_dir(&handle);
            std::fs::create_dir_all(&dir).ok();
            let state = App::open(
                &dir.join("riposte.db"),
                bundled_playbook(&handle).as_deref(),
            )
            .map_err(|e| format!("opening the store: {e}"))?;
            app.manage(state);

            if let Some(window) = app.get_webview_window("overlay") {
                // Hide from screen capture by default. `05-UX-SPEC.md` §6 ships
                // it on, as a visible toggle — an agent screen-sharing a quote
                // should not be broadcasting their objection cards.
                let on = app
                    .state::<App>()
                    .get_setting("overlay.exclude_from_capture")
                    .ok()
                    .flatten()
                    .map(|v| v == "1")
                    .unwrap_or(true);
                platform::reassert_window_state(&window, on);
            }

            register_hotkeys(&handle)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the overlay");
}

fn register_hotkeys(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let shortcuts: Vec<(u8, Shortcut)> = SLOT_CODES
        .iter()
        .map(|(slot, code)| (*slot, Shortcut::new(Some(HOTKEY_MODIFIERS), *code)))
        .collect();

    let handle = app.clone();
    app.global_shortcut().on_shortcuts(
        shortcuts.iter().map(|(_, s)| *s).collect::<Vec<_>>(),
        move |_app, shortcut, event| {
            // Fire on press, not release: the agent is mid-sentence and the
            // card needs to be there before they finish it.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let Some((slot, _)) = SLOT_CODES
                .iter()
                .map(|(slot, code)| (*slot, Shortcut::new(Some(HOTKEY_MODIFIERS), *code)))
                .find(|(_, s)| s == shortcut)
            else {
                return;
            };
            // The window owns what a slot means; it already has the rail.
            let _ = handle.emit("rail:hotkey", slot);
        },
    )?;
    Ok(())
}
