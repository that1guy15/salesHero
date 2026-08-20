//! Required test for `AGENTS.md` invariant 3, and the M1.7 acceptance gate.
//!
//! > The Quick Response Rail renders without any model. If ASR, embeddings and
//! > the LLM are all unavailable, the rail must still display and its hotkeys
//! > must still work.
//!
//! The strongest available proof is structural: `riposte-desktop` has no
//! dependency through which a model could be reached, so "all engines
//! unavailable" is not a state to simulate — it is the only state that exists.
//! The rail is then driven end to end against a real store with no network, no
//! audio device, and no model on disk.

use riposte_core::CardOrigin;
use riposte_desktop::state::App;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "riposte-overlay-{tag}-{}-{}",
        std::process::id(),
        riposte_store::now_ms()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn playbook() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../playbooks/insurance-cold-call.yaml")
}

fn app(tag: &str) -> (App, std::path::PathBuf) {
    let dir = temp_dir(tag);
    let app = App::open(&dir.join("riposte.db"), Some(&playbook()))
        .expect("the overlay must open with no engines present");
    (app, dir)
}

/// Required test — `AGENTS.md` invariant 3.
#[test]
fn rail_renders_with_all_engines_disabled() {
    let (app, dir) = app("rail");

    let rail = app.rail().expect("the rail must render");
    assert_eq!(rail.items.len(), 6, "01-PRD.md §5 ships six rail slots");

    // Slot order, and it never changes: the agent hits 3 without looking.
    let slots: Vec<u8> = rail.items.iter().map(|i| i.slot).collect();
    assert_eq!(slots, vec![1, 2, 3, 4, 5, 6], "the rail reordered itself");

    // Every item is renderable on its own — a label to read and something to
    // say. A rail item with no body is a blank card mid-call.
    for item in &rail.items {
        assert!(!item.label.is_empty(), "slot {} has no label", item.slot);
        assert!(
            !item.response.trim().is_empty() || !item.talk_track.is_empty(),
            "slot {} has nothing to show",
            item.slot
        );
        assert!(
            item.label.chars().count() <= 28,
            "label too long for the rail: {:?}",
            item.label
        );
    }

    // The compliance entry is present and is not a rebuttal.
    let dnc = rail
        .items
        .iter()
        .find(|i| i.kind == "comply")
        .expect("no comply entry on the rail");
    assert_eq!(dnc.slot, 6);

    assert!(
        !rail.below_fold.is_empty(),
        "below-fold objections should load too"
    );
    std::fs::remove_dir_all(dir).ok();
}

/// The other half of invariant 3: the hotkeys must still *work*, not merely
/// render. A press records a card with no session and no call, which is what
/// makes `01-PRD.md` §7's headline metric measurable.
#[test]
fn hotkeys_work_with_all_engines_disabled() {
    let (app, dir) = app("hotkey");

    let before = app.stats().unwrap().manual;
    for slot in 1..=6u8 {
        let rail = app.rail().unwrap();
        let item = rail.items.iter().find(|i| i.slot == slot).unwrap();
        let card = app
            .trigger(&item.objection_id, CardOrigin::ManualHotkey)
            .unwrap_or_else(|e| panic!("slot {slot} did not fire: {e}"));
        assert_eq!(card.slot, slot);
    }
    let after = app.stats().unwrap();
    assert_eq!(after.manual - before, 6, "six presses must record six uses");
    assert_eq!(after.detected, 0, "Layer 0 records no detections");

    std::fs::remove_dir_all(dir).ok();
}

/// `01-PRD.md` F14: useful on first run with zero setup. An empty rail on a
/// fresh install is an empty product.
#[test]
fn first_run_loads_the_bundled_playbook() {
    let dir = temp_dir("firstrun");
    let db = dir.join("riposte.db");
    assert!(!db.exists());

    let app = App::open(&db, Some(&playbook())).unwrap();
    assert_eq!(app.rail().unwrap().items.len(), 6);

    std::fs::remove_dir_all(dir).ok();
}

/// The store is missing its playbook entirely — a corrupt install, or a user
/// who deleted it. The overlay must still come up: `05-UX-SPEC.md` §4 keeps the
/// rail visible even when capture has failed, and a launch crash is worse than
/// an empty rail.
#[test]
fn a_missing_playbook_does_not_prevent_startup() {
    let dir = temp_dir("noplaybook");
    let app = App::open(&dir.join("riposte.db"), None).expect("must still start");
    let rail = app.rail().expect("the rail must still render");
    assert!(rail.items.is_empty());
    std::fs::remove_dir_all(dir).ok();
}

/// Editing a card is the path `01-PRD.md` §2 says the real user will take.
#[test]
fn a_card_can_be_edited_in_place_and_persists() {
    let dir = temp_dir("edit");
    let db = dir.join("riposte.db");
    {
        let app = App::open(&db, Some(&playbook())).unwrap();
        let id = app.rail().unwrap().items[0].objection_id.clone();
        app.save_card(&riposte_desktop::commands::EditedCard {
            objection_id: id.clone(),
            response: "Say it my way.".into(),
            talk_track: vec!["In my own words.".into()],
        })
        .unwrap();
    }
    // Reopened: the edit is on disk, not in memory.
    let app = App::open(&db, Some(&playbook())).unwrap();
    let item = &app.rail().unwrap().items[0];
    assert_eq!(item.response, "Say it my way.");
    assert_eq!(item.talk_track, vec!["In my own words."]);

    std::fs::remove_dir_all(dir).ok();
}

/// `06-COMPLIANCE.md` §3: "log removal and end call" writes a row that outlives
/// the transcript TTL, because it is the one record an agent may need months
/// later.
#[test]
fn a_removal_request_is_logged() {
    let (app, dir) = app("removal");
    app.log_removal("obj_dnc").unwrap();
    std::fs::remove_dir_all(dir).ok();
}
