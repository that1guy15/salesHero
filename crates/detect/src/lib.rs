//! Detection: window construction, the embedding gate, and the branch on
//! `ObjectionKind` that keeps compliance responses away from any model.

pub mod embed;
pub mod gate;

pub use embed::{cosine, Embedder, HashEmbedder, DIM};
pub use gate::{
    build_window, window_text, DetectionGate, Dedupe, GateHit, Score, DEDUPE_WINDOW_MS,
    WINDOW_MS, WINDOW_TURNS,
};

#[cfg(feature = "onnx")]
pub use embed::OnnxEmbedder;

use riposte_core::{Objection, ObjectionKind, RebuttableObjection};

/// Stage 2 — branch on kind. See `docs/02-ARCHITECTURE.md` §7.
///
/// This is the only place the pipeline decides whether a model may be
/// involved. Returning [`RebuttableObjection`] rather than [`Objection`] means
/// the refinement path cannot be handed a `comply` entry by mistake.
pub enum Decision {
    /// Fixed authored card. No retrieval, no LLM, no tailoring.
    Comply { objection: Objection },
    /// Layer 1 immediately; Layer 2 refinement is permitted but optional.
    Promote { objection: RebuttableObjection },
}

pub fn decide(objection: Objection) -> Decision {
    match objection.kind {
        ObjectionKind::Comply => Decision::Comply { objection },
        ObjectionKind::Rebut | ObjectionKind::Disqualify => {
            match RebuttableObjection::new(objection.clone()) {
                Some(r) => Decision::Promote { objection: r },
                // Unreachable: only Comply returns None. Fail closed anyway.
                None => Decision::Comply { objection },
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

            let objection = objections.iter().find(|o| o.id == hit.objection_id).unwrap().clone();
            match decide(objection) {
                Decision::Comply { objection } => {
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

        assert!(fired > 0, "the gate matched none of the DNC triggers — test proves nothing");
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
