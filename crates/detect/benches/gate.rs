//! `07-BUILD-PLAN.md` M1: "Gate evaluation is < 10 ms for a 100-objection
//! playbook — benchmark it." This is that benchmark, and it was the last M1
//! acceptance criterion with nothing behind it.
//!
//! Measures *evaluation*, not index construction. Since `docs/DECISIONS.md`
//! D12 the index is built once at playbook import and loaded from the store, so
//! construction is not on the per-utterance path and benchmarking it would
//! flatter the result.
//!
//!     cargo bench -p riposte-detect

use riposte_core::{Objection, ObjectionKind, Speaker, Utterance};
use riposte_detect::{DetectionGate, HashEmbedder};
use std::time::Instant;

fn synthetic(n: usize) -> Vec<Objection> {
    (0..n)
        .map(|i| Objection {
            id: format!("obj_{i}"),
            label: format!("Objection {i}"),
            category: None,
            kind: if i == 0 {
                ObjectionKind::Comply
            } else {
                ObjectionKind::Rebut
            },
            response: String::new(),
            talk_track: vec![],
            guardrails: vec![],
            rail_slot: None,
            threshold_authored: 0.62,
            threshold_learned: None,
            allow_refine: true,
            enabled: true,
            sort_order: i as i64,
            // Six triggers each, matching the shipped playbook's density.
            triggers: (0..6)
                .map(|t| format!("this is trigger phrase number {t} for objection {i}"))
                .collect(),
            negative_triggers: vec![format!("not objection {i} at all")],
            doc_refs: vec![],
            escalate_after_repeats: None,
        })
        .collect()
}

fn turn(text: &str, t: i64) -> Utterance {
    Utterance {
        speaker: Speaker::Prospect,
        text: text.into(),
        t_start_ms: t,
        t_end_ms: t + 1000,
        from_preroll: false,
    }
}

fn main() {
    let embedder = HashEmbedder::new();
    println!("gate evaluation — target < 10 ms at 100 objections\n");
    println!(
        "  {:>10}  {:>10}  {:>10}  {:>10}",
        "objections", "p50", "p95", "max"
    );

    for n in [10usize, 50, 100, 250] {
        let gate = DetectionGate::build(&synthetic(n), &embedder);

        // A full three-turn window, which is what the gate actually sees: D9
        // scores every suffix, so this is three spans per evaluation.
        let window = vec![
            turn("hmm", 0),
            turn("yeah I don't know about that", 2000),
            turn("this is trigger phrase number 3 for objection 7", 4000),
        ];

        for _ in 0..50 {
            std::hint::black_box(gate.evaluate(&window, &embedder));
        }

        let mut samples: Vec<u128> = (0..500)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(gate.evaluate(&window, &embedder));
                t.elapsed().as_micros()
            })
            .collect();
        samples.sort_unstable();

        let us = |p: f64| samples[((samples.len() as f64 * p) as usize).min(samples.len() - 1)];
        let over = us(0.95) as f64 / 1000.0 > 10.0;
        println!(
            "  {n:>10}  {:>8.3}ms {:>8.3}ms {:>8.3}ms{}",
            us(0.50) as f64 / 1000.0,
            us(0.95) as f64 / 1000.0,
            *samples.last().unwrap() as f64 / 1000.0,
            if over { "   OVER BUDGET" } else { "" }
        );
    }
    println!(
        "\n  Index construction is excluded: since DECISIONS.md D12 it happens once at\n  \
         playbook import, not per utterance."
    );
}
