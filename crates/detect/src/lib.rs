//! Detection: window construction, the embedding gate, and the branch on
//! `ObjectionKind` that keeps compliance responses away from any model.

pub mod embed;
pub mod gate;

pub use embed::{cosine, Embedder, HashEmbedder, DIM};
pub use gate::{
    build_window, build_window_with, select_hit, window_text, Dedupe, DetectionGate, GateConfig,
    GateHit, Score, DEDUPE_WINDOW_MS, WINDOW_MS, WINDOW_TURNS,
};

#[cfg(feature = "onnx")]
pub use embed::OnnxEmbedder;

use riposte_core::{Objection, ObjectionKind, RebuttableObjection};
use riposte_store::{Store, StoreError};

/// Loads the active playbook's tuning knobs, falling back to the shipped
/// defaults for anything absent or unparseable.
///
/// The values are written by playbook import (`docs/DECISIONS.md` D13). A
/// malformed stored value falls back rather than failing: a bad setting should
/// degrade tuning, not stop detection.
pub fn gate_config(store: &Store, playbook_id: &str) -> Result<GateConfig, StoreError> {
    let mut cfg = GateConfig::default();
    if let Some(v) = store
        .get_playbook_setting(playbook_id, "dedupe_window_ms")?
        .and_then(|v| v.parse::<i64>().ok())
    {
        cfg.dedupe_window_ms = v;
    }
    if let Some(v) = store
        .get_playbook_setting(playbook_id, "card_lifetime_ms")?
        .and_then(|v| v.parse::<i64>().ok())
    {
        cfg.card_lifetime_ms = v;
    }
    Ok(cfg)
}

/// Embeds every trigger phrase in a playbook that does not yet have a vector
/// for this embedder, and stores it.
///
/// Called at playbook import. `M1` of `docs/07-BUILD-PLAN.md` specifies
/// embedding "at playbook save time"; before this the whole index was rebuilt
/// on every evaluation, which is seconds per detection under BGE-small.
///
/// Returns how many phrases were embedded. Idempotent: a second call over an
/// unchanged playbook embeds nothing.
pub fn embed_playbook(
    store: &Store,
    playbook_id: &str,
    embedder: &dyn Embedder,
) -> Result<usize, StoreError> {
    let model = embedder.model_id();
    let pending = store.phrases_needing_embedding(playbook_id, model)?;
    for (table, row_id, phrase) in &pending {
        store.set_embedding(table, *row_id, model, &embedder.embed(phrase))?;
    }
    Ok(pending.len())
}

/// Stage 2 — branch on kind. See `docs/02-ARCHITECTURE.md` §7.
///
/// This is the only place the pipeline decides whether a model may be
/// involved. Returning [`RebuttableObjection`] rather than [`Objection`] means
/// the refinement path cannot be handed a `comply` entry by mistake.
pub enum Decision {
    /// Fixed authored card. No retrieval, no LLM, no tailoring.
    Comply {
        objection: Objection,
        /// Set when this card was reached by repeat escalation rather than by
        /// matching a `comply` trigger directly — the id of the objection the
        /// prospect actually repeated. See [`decide_with_repeats`].
        escalated_from: Option<String>,
    },
    /// Layer 1 immediately; Layer 2 refinement is permitted but optional.
    Promote { objection: RebuttableObjection },
}

pub fn decide(objection: Objection) -> Decision {
    decide_with_repeats(objection, 1, None)
}

/// Stage 2, with the call's repeat count — `docs/DECISIONS.md` D8.
///
/// `insurance-cold-call.yaml` carried two obligations as prose guardrails:
/// *"If they deny it a second time, treat it as a removal request"*
/// (`obj_no_form`) and *"One attempt only. A second 'not interested' ends the
/// call"* (`obj_not_interested`). Every other compliance guarantee in this
/// project is structural — type level, SQL trigger, required test. These two
/// were text an LLM was asked politely to respect.
///
/// `repeats` comes from [`Dedupe::hits`]; `comply` is the playbook's compliance
/// entry. When an objection carrying `escalate_after_repeats` reaches that
/// count within one call, the compliance card is served instead of the
/// rebuttal. Coaching an agent to keep pushing after a second refusal is the
/// exact failure `docs/06-COMPLIANCE.md` §3 exists to prevent.
///
/// Escalation is skipped when the playbook has no `comply` entry — serving no
/// card is correct there, but silently dropping the objection would be worse
/// than the rebuttal, so the rebuttal stands and the caller can log it.
pub fn decide_with_repeats(
    objection: Objection,
    repeats: u32,
    comply: Option<&Objection>,
) -> Decision {
    match objection.kind {
        ObjectionKind::Comply => Decision::Comply {
            objection,
            escalated_from: None,
        },
        ObjectionKind::Rebut | ObjectionKind::Disqualify => {
            if let (Some(after), Some(target)) = (objection.escalate_after_repeats, comply) {
                if repeats >= after && target.kind == ObjectionKind::Comply {
                    return Decision::Comply {
                        objection: target.clone(),
                        escalated_from: Some(objection.id),
                    };
                }
            }
            match RebuttableObjection::new(objection.clone()) {
                Some(r) => Decision::Promote { objection: r },
                // Unreachable: only Comply returns None. Fail closed anyway.
                None => Decision::Comply {
                    objection,
                    escalated_from: None,
                },
            }
        }
    }
}

#[cfg(test)]
mod invariant_tests {
    use super::*;
    use crate::embed::HashEmbedder;
    use riposte_core::Utterance;

    /// A stand-in for `crates/llm`, which lands at M3. Any call is a failure.
    struct PanickingProvider;

    impl PanickingProvider {
        fn complete(&self, _prompt: &str) -> String {
            panic!("a comply objection reached the LLM — docs/06-COMPLIANCE.md §3");
        }
    }

    fn comply_objection(id: &str, triggers: &[&str]) -> Objection {
        Objection {
            id: id.into(),
            label: "Take me off your list".into(),
            category: Some("compliance".into()),
            kind: ObjectionKind::Comply,
            response: "Confirm removal and end the call.".into(),
            talk_track: vec!["I'm removing you right now.".into()],
            guardrails: vec![],
            rail_slot: Some(6),
            threshold_authored: 0.55,
            threshold_learned: None,
            allow_refine: true, // as a hostile playbook would claim
            enabled: true,
            sort_order: 0,
            triggers: triggers.iter().map(|s| s.to_string()).collect(),
            negative_triggers: vec![],
            doc_refs: vec![],
            escalate_after_repeats: None,
        }
    }

    /// Required test — `AGENTS.md` invariant 4.
    ///
    /// Every objection in the playbook is `comply`. Every one of their trigger
    /// phrases is fed through the full gate. The provider panics if invoked.
    /// The test passes only if it never is.
    #[test]
    fn comply_objections_never_reach_llm() {
        let embedder = HashEmbedder::new();
        let provider = PanickingProvider;

        let triggers = [
            "take me off your list",
            "put me on your do not call list",
            "stop calling me",
            "don't call here again",
            "remove me from your list",
            "I'm on the do not call registry",
            "quit calling",
        ];
        let objections = vec![comply_objection("obj_dnc", &triggers)];
        let gate = DetectionGate::build(&objections, &embedder);

        let mut fired = 0;
        for phrase in triggers {
            let window = vec![Utterance::prospect(phrase)];
            let Some(hit) = gate.evaluate(&window, &embedder) else {
                continue;
            };
            fired += 1;

            let objection = objections
                .iter()
                .find(|o| o.id == hit.objection_id)
                .unwrap()
                .clone();
            match decide(objection) {
                Decision::Comply { objection, .. } => {
                    // The fixed authored card, rendered as written.
                    assert_eq!(objection.response, "Confirm removal and end the call.");
                }
                Decision::Promote { objection } => {
                    // Only reachable if the branch is broken. Prove it by
                    // doing what the refinement path would do.
                    provider.complete(objection.get().response.as_str());
                }
            }
        }

        assert!(
            fired > 0,
            "the gate matched none of the DNC triggers — test proves nothing"
        );
    }

    // ─── D8: repeat-denial escalation ────────────────────────────────────

    fn rebuttal_with_escalation(id: &str, after: u32) -> Objection {
        let mut o = comply_objection(id, &["not interested"]);
        o.kind = ObjectionKind::Rebut;
        o.escalate_after_repeats = Some(after);
        o
    }

    /// The ethical core of `docs/DECISIONS.md` D8. A second refusal inside one
    /// call serves the compliance card, not another rebuttal.
    #[test]
    fn second_denial_escalates_to_the_comply_card() {
        let dnc = comply_objection("obj_dnc", &["stop calling me"]);
        let rebut = rebuttal_with_escalation("obj_not_interested", 2);

        // First refusal: coach normally.
        assert!(matches!(
            decide_with_repeats(rebut.clone(), 1, Some(&dnc)),
            Decision::Promote { .. }
        ));

        // Second: stop.
        match decide_with_repeats(rebut, 2, Some(&dnc)) {
            Decision::Comply {
                objection,
                escalated_from,
            } => {
                assert_eq!(objection.id, "obj_dnc");
                assert_eq!(escalated_from.as_deref(), Some("obj_not_interested"));
            }
            Decision::Promote { .. } => {
                panic!("a second refusal was met with another rebuttal — DECISIONS.md D8")
            }
        }
    }

    /// An escalated card is still a comply card, so invariant 4 still holds:
    /// it cannot be turned into a `RebuttableObjection` and reach a model.
    #[test]
    fn an_escalated_card_still_cannot_reach_the_llm() {
        let dnc = comply_objection("obj_dnc", &["stop calling me"]);
        let rebut = rebuttal_with_escalation("obj_no_form", 2);
        let Decision::Comply { objection, .. } = decide_with_repeats(rebut, 3, Some(&dnc)) else {
            panic!("expected escalation");
        };
        assert!(RebuttableObjection::new(objection).is_none());
    }

    /// Objections without the field are unaffected however often they repeat.
    #[test]
    fn objections_without_escalation_never_escalate() {
        let dnc = comply_objection("obj_dnc", &["stop calling me"]);
        let mut plain = comply_objection("obj_price", &["how much is it"]);
        plain.kind = ObjectionKind::Rebut;
        plain.escalate_after_repeats = None;
        assert!(matches!(
            decide_with_repeats(plain, 9, Some(&dnc)),
            Decision::Promote { .. }
        ));
    }

    /// With no compliance entry in the playbook there is nothing to escalate
    /// to. The rebuttal stands rather than the objection being dropped.
    #[test]
    fn escalation_without_a_comply_entry_keeps_the_rebuttal() {
        let rebut = rebuttal_with_escalation("obj_not_interested", 2);
        assert!(matches!(
            decide_with_repeats(rebut, 5, None),
            Decision::Promote { .. }
        ));
    }

    /// End to end through the real counter: the gate fires, `Dedupe` counts,
    /// and the second fire escalates.
    #[test]
    fn dedupe_counter_drives_the_escalation() {
        let e = HashEmbedder::new();
        let dnc = comply_objection("obj_dnc", &["stop calling me"]);
        let mut rebut = rebuttal_with_escalation("obj_not_interested", 2);
        rebut.threshold_authored = 0.62;
        let gate = DetectionGate::build(&[rebut.clone()], &e);
        let mut dedupe = Dedupe::with_default_window();

        let mut escalated = false;
        for t in [0i64, 60_000] {
            let hit = gate
                .evaluate(&[Utterance::prospect("not interested")], &e)
                .expect("trigger should match");
            dedupe.should_suppress(&hit.objection_id, t);
            let decision =
                decide_with_repeats(rebut.clone(), dedupe.hits(&hit.objection_id), Some(&dnc));
            if matches!(decision, Decision::Comply { .. }) {
                escalated = true;
            }
        }
        assert!(escalated, "two fires in one call did not escalate");
    }

    #[test]
    fn rebut_objections_do_reach_the_refinement_path() {
        // The mirror of the above: if nothing is ever promoted, the invariant
        // test above would pass vacuously.
        let mut o = comply_objection("obj_x", &["not interested"]);
        o.kind = ObjectionKind::Rebut;
        assert!(matches!(decide(o), Decision::Promote { .. }));
    }

    #[test]
    fn disqualify_is_rebuttable_but_comply_is_not() {
        let mut o = comply_objection("obj_x", &["x"]);
        o.kind = ObjectionKind::Disqualify;
        assert!(matches!(decide(o.clone()), Decision::Promote { .. }));
        o.kind = ObjectionKind::Comply;
        assert!(matches!(decide(o), Decision::Comply { .. }));
    }
}
