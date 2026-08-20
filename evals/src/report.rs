//! The calibration report.
//!
//! Four sections, in the order the decisions depend on each other: do the
//! authored triggers work at all, do they collide with each other, what happens
//! on speech nobody wrote down, and — given all that — where should the
//! thresholds actually sit.

use crate::probes::Probe;
use crate::scoring::Arm;
use crate::stats::{self, fmt};
use riposte_core::{Objection, ObjectionKind};

const DIM: &str = "\x1b[38;2;138;148;163m";
const OK: &str = "\x1b[38;2;62;213;152m";
const BAD: &str = "\x1b[38;2;255;107;77m";
const BOLD: &str = "\x1b[1m";
const R: &str = "\x1b[0m";

fn head(title: &str) {
    println!("\n{BOLD}{title}{R}");
}

pub fn calibrate(arm: &Arm, objections: &[Objection], probes: &[Probe]) {
    println!(
        "\n{BOLD}GATE CALIBRATION{R}  {DIM}{} · {} objections · {} probes{R}",
        arm.name,
        objections.len(),
        probes.len()
    );
    if arm.floor() > 0.0 {
        println!(
            "{DIM}semantic floor measured at {:.3} from far-negative probes; \
             scores rescaled to (raw − floor)/(1 − floor){R}",
            arm.floor()
        );
    }

    raw_floor_check(arm, probes);
    self_match(arm, objections);
    cross_match(arm, objections);
    let (pos, neg) = held_out(arm, objections, probes);
    separation(&pos, &neg);
    propose(arm, objections, probes);
}

// ─── 0. Do the shipped thresholds sit inside the embedder's noise floor? ─

/// The question `docs/PROJECT-STATUS.md` §3.1 raised from one probe, answered
/// over the whole far-negative set.
///
/// BGE-family models have a high similarity floor: unrelated short sentences do
/// not score near zero, they score 0.6–0.75. The shipped thresholds are
/// 0.55–0.66. If the floor is above them, the thresholds are not discriminating
/// anything — every utterance in a call clears them, including silence between
/// objections.
fn raw_floor_check(arm: &Arm, probes: &[Probe]) {
    if !arm.has_semantic() {
        return;
    }
    head("NOISE FLOOR — the shipped thresholds against raw, un-rescaled scores");

    let far: Vec<&Probe> = probes.iter().filter(|p| p.expected().is_none()).collect();
    let mut raws: Vec<f32> = Vec::new();
    let mut would_fire = 0usize;

    for p in &far {
        let scores = arm.raw_semantic(&p.text);
        if let Some(best) = scores.iter().max_by(|a, b| a.score.total_cmp(&b.score)) {
            raws.push(best.score);
            if best.score >= best.threshold {
                would_fire += 1;
            }
        }
    }
    let sorted = stats::sorted(raws);
    println!(
        "  {DIM}unrelated speech, raw cosine: min {} p50 {} p90 {} max {}{R}",
        fmt(stats::min(&sorted)),
        fmt(stats::percentile(&sorted, 50.0)),
        fmt(stats::percentile(&sorted, 90.0)),
        fmt(stats::max(&sorted))
    );
    let pct = 100.0 * would_fire as f32 / far.len().max(1) as f32;
    let colour = if would_fire == 0 { OK } else { BAD };
    println!(
        "  {colour}{would_fire}/{} ({pct:.0}%) of non-objection utterances clear an authored threshold{R}",
        far.len()
    );
    if would_fire > 0 {
        println!("  {BAD}The thresholds are inside this embedder's noise floor. They are not{R}");
        println!("  {BAD}discriminating — they are letting nearly everything through.{R}");
        println!("  {DIM}Everything below rescales to (raw - floor)/(1 - floor) so the arms{R}");
        println!("  {DIM}are comparable at all. That is a measurement device, not a fix.{R}");
    }
}

// ─── 1. Do the authored triggers match their own objection? ──────────────

fn self_match(arm: &Arm, objections: &[Objection]) {
    head("SELF-MATCH — each objection against its own trigger phrases");
    println!("{DIM}  M1 acceptance: at least 3 of its own triggers at or above threshold{R}");
    println!(
        "{DIM}  {:<24} {:>9} {:>8} {:>8}{R}",
        "objection", "at/above", "min", "thresh"
    );

    for o in objections {
        let scores: Vec<f32> = o
            .triggers
            .iter()
            .map(|t| {
                arm.score(t)
                    .into_iter()
                    .find(|s| s.objection_id == o.id)
                    .map_or(0.0, |s| s.score)
            })
            .collect();
        let th = o.effective_threshold();
        let n = scores.iter().filter(|s| **s >= th).count();
        let sorted = stats::sorted(scores.clone());
        let pass = n >= 3.min(o.triggers.len());
        println!(
            "{}  {:<24} {:>4}/{:<4} {:>8} {:>8.2}{R}",
            if pass { OK } else { BAD },
            o.label,
            n,
            o.triggers.len(),
            fmt(stats::min(&sorted)),
            th
        );
    }
}

// ─── 2. Do they collide with each other? ─────────────────────────────────

fn cross_match(arm: &Arm, objections: &[Objection]) {
    head("CROSS-MATCH — each objection against every *other* objection's triggers");
    println!(
        "{DIM}  M1 acceptance: no objection matches another's trigger phrases above threshold{R}"
    );

    let mut violations = 0;
    for owner in objections {
        for trigger in &owner.triggers {
            for s in arm.score(trigger) {
                if s.objection_id == owner.id || !s.is_hit() {
                    continue;
                }
                // A comply entry firing on someone else's trigger is by design:
                // it ships at a deliberately low threshold because a false
                // positive costs nothing and a false negative is a TCPA
                // problem. Reported, not counted.
                let comply = s.kind == ObjectionKind::Comply;
                violations += usize::from(!comply);
                println!(
                    "{}  {} {DIM}'{}'{R}{} → {} at {:.3} ≥ {:.2}{R}",
                    if comply { DIM } else { BAD },
                    owner.label,
                    trigger,
                    if comply { DIM } else { BAD },
                    s.label,
                    s.score,
                    s.threshold
                );
            }
        }
    }
    if violations == 0 {
        println!("{OK}  no violations{R}");
    } else {
        println!("{BAD}  {violations} violation(s) — thresholds are not discriminating{R}");
    }
}

// ─── 3. Speech nobody wrote down ─────────────────────────────────────────

/// Returns `(scores of correct matches, best wrong-or-spurious scores)`.
fn held_out(arm: &Arm, objections: &[Objection], probes: &[Probe]) -> (Vec<f32>, Vec<f32>) {
    head("HELD-OUT PARAPHRASES — the measurement that actually matters");
    println!(
        "{DIM}  Scoring a trigger against its own objection is circular. These are\n  \
         paraphrases sharing little wording with any authored trigger.{R}"
    );

    let (mut pos, mut neg) = (Vec::new(), Vec::new());
    let (mut tp, mut fp, mut fn_, mut wrong) = (0, 0, 0, 0);
    let mut misses: Vec<(&str, &str, f32, f32)> = Vec::new();
    let mut spurious: Vec<(&str, String, f32)> = Vec::new();

    for p in probes {
        let hit = arm.hit(&p.text);
        match (p.expected(), &hit) {
            (Some(want), Some(got)) if got.objection_id == want => {
                tp += 1;
                pos.push(got.score);
            }
            (Some(want), Some(got)) => {
                wrong += 1;
                fp += 1;
                neg.push(got.score);
                spurious.push((&p.text, got.label.clone(), got.score));
                let _ = want;
            }
            (Some(want), None) => {
                fn_ += 1;
                let best = arm
                    .score(&p.text)
                    .into_iter()
                    .find(|s| s.objection_id == want);
                if let Some(b) = best {
                    misses.push((&p.text, want, b.score, b.threshold));
                    pos.push(b.score);
                }
            }
            (None, Some(got)) => {
                fp += 1;
                neg.push(got.score);
                spurious.push((&p.text, got.label.clone(), got.score));
            }
            (None, None) => {
                // Correct silence. Record the best score anyway — how close the
                // gate came is what sets the ceiling on a threshold.
                if let Some(b) = arm.score(&p.text).into_iter().next() {
                    neg.push(b.score);
                }
            }
        }
    }

    let labelled = probes.iter().filter(|p| p.expected().is_some()).count();
    let recall = if labelled > 0 {
        tp as f32 / labelled as f32
    } else {
        0.0
    };
    let precision = if tp + fp > 0 {
        tp as f32 / (tp + fp) as f32
    } else {
        0.0
    };

    println!(
        "\n  recall     {}{:>4}/{:<4}  {:.2}{R}  {DIM}(01-PRD.md §7 target > 0.60){R}",
        if recall >= 0.60 { OK } else { BAD },
        tp,
        labelled,
        recall
    );
    println!(
        "  precision  {}{:>4}/{:<4}  {:.2}{R}  {DIM}(target > 0.75){R}",
        if precision >= 0.75 { OK } else { BAD },
        tp,
        tp + fp,
        precision
    );
    println!(
        "  {DIM}missed {fn_} · wrong objection {wrong} · fired on nothing {}{R}",
        fp - wrong
    );

    if !misses.is_empty() {
        println!("\n{DIM}  missed — scored below the objection's own threshold:{R}");
        for (t, want, got, th) in misses.iter().take(12) {
            println!("    {BAD}·{R} {DIM}\"{t}\"{R} → {want} {got:.3} < {th:.2}");
        }
        if misses.len() > 12 {
            println!("    {DIM}… and {} more{R}", misses.len() - 12);
        }
    }
    if !spurious.is_empty() {
        println!("\n{DIM}  fired when it should not have:{R}");
        for (t, label, sc) in spurious.iter().take(12) {
            println!("    {BAD}·{R} {DIM}\"{t}\"{R} → {label} {sc:.3}");
        }
        if spurious.len() > 12 {
            println!("    {DIM}… and {} more{R}", spurious.len() - 12);
        }
    }

    // DNC recall is reported separately because a miss here is not a quality
    // problem, it is a compliance one.
    let dnc: Vec<&Probe> = probes
        .iter()
        .filter(|p| p.expected().is_some_and(|e| Arm::is_comply(objections, e)))
        .collect();
    if !dnc.is_empty() {
        let caught = dnc
            .iter()
            .filter(|p| {
                arm.hit(&p.text)
                    .is_some_and(|h| h.kind == ObjectionKind::Comply)
            })
            .count();
        println!(
            "\n  {}removal requests caught  {caught}/{}{R}  {DIM}(a miss here is a TCPA problem, \
             not a quality one){R}",
            if caught == dnc.len() { OK } else { BAD },
            dnc.len()
        );
        for p in &dnc {
            if !arm
                .hit(&p.text)
                .is_some_and(|h| h.kind == ObjectionKind::Comply)
            {
                println!("    {BAD}MISSED{R} {DIM}\"{}\"{R}", p.text);
            }
        }
    }

    (stats::sorted(pos), stats::sorted(neg))
}

// ─── 4. Is there a gap to put a threshold in? ────────────────────────────

fn separation(pos: &[f32], neg: &[f32]) {
    head("DISTRIBUTION SEPARATION — the number that decides everything");

    let p10 = stats::percentile(pos, 10.0);
    let n90 = stats::percentile(neg, 90.0);
    println!(
        "  {DIM}should-match   n={:<3} min {} p10 {} mean {} max {}{R}",
        pos.len(),
        fmt(stats::min(pos)),
        fmt(p10),
        fmt(stats::mean(pos)),
        fmt(stats::max(pos))
    );
    println!(
        "  {DIM}should-not     n={:<3} min {} p90 {} mean {} max {}{R}",
        neg.len(),
        fmt(stats::min(neg)),
        fmt(n90),
        fmt(stats::mean(neg)),
        fmt(stats::max(neg))
    );

    match (p10, n90) {
        (Some(p), Some(n)) => {
            let gap = p - n;
            let (colour, verdict) = if gap > 0.10 {
                (OK, "wide — set thresholds at the midpoint and move on")
            } else if gap > 0.0 {
                (
                    OK,
                    "narrow — usable, but per-objection thresholds are doing real work",
                )
            } else {
                (
                    BAD,
                    "none — the distributions overlap. Thresholds cannot fix this; \
                     change the approach (see the gate-calibration skill)",
                )
            };
            println!("\n  {colour}gap  p10(match) − p90(no-match) = {gap:+.3}{R}");
            println!("  {colour}{verdict}{R}");
        }
        _ => println!("  {DIM}not enough data on one side to compute a gap{R}"),
    }
}

// ─── 5. Thresholds from the data ─────────────────────────────────────────

fn propose(arm: &Arm, objections: &[Objection], probes: &[Probe]) {
    head("PROPOSED THRESHOLDS");
    println!(
        "{DIM}  Midpoint between what this objection scores on speech that should match it\n  \
         and the highest it scores on speech that should not. Clamped to [0.50, 0.85]\n  \
         per 03-DATA-MODEL.md §4.{R}"
    );
    println!(
        "{DIM}  {:<24} {:>8} {:>9} {:>8} {:>8}{R}",
        "objection", "authored", "proposed", "match", "no-match"
    );

    for o in objections {
        let mut yes: Vec<f32> = Vec::new();
        let mut no: Vec<f32> = Vec::new();
        for p in probes {
            let s = arm
                .score(&p.text)
                .into_iter()
                .find(|s| s.objection_id == o.id);
            let Some(s) = s else { continue };
            if p.expected() == Some(o.id.as_str()) {
                yes.push(s.score);
            } else {
                no.push(s.score);
            }
        }
        let yes = stats::sorted(yes);
        let no = stats::sorted(no);
        let lo = stats::percentile(&yes, 10.0);
        let hi = stats::percentile(&no, 90.0);

        // A gap smaller than this is noise, and a threshold placed inside it
        // would be worse than the authored default — it would look
        // data-driven while firing on whatever happened to be in the probe set.
        const MEANINGFUL_GAP: f32 = 0.05;

        let (proposed, note) = if o.kind == ObjectionKind::Comply {
            // Never propose for a compliance entry. `obj_dnc` ships at 0.55
            // against 0.60-0.66 deliberately: a false positive costs nothing
            // and a false negative is a TCPA violation. Since D11 it also
            // matches lexically, so the threshold is a backstop rather than the
            // primary path. "Correcting" it upward to match the others is the
            // specific mistake to avoid here.
            (
                None,
                format!("{DIM}  compliance entry — threshold is policy{R}"),
            )
        } else {
            match (lo, hi) {
                (Some(l), Some(h)) if l - h >= MEANINGFUL_GAP => {
                    (Some(((l + h) / 2.0).clamp(0.50, 0.85)), String::new())
                }
                (Some(l), Some(h)) if l > h => (
                    None,
                    format!("{DIM}  gap {:.3} is noise — leave authored{R}", l - h),
                ),
                _ => (None, format!("{DIM}  no separation — leave authored{R}")),
            }
        };
        println!(
            "  {:<24} {:>8.2} {:>9} {:>8} {:>8}{note}",
            o.label,
            o.threshold_authored,
            proposed.map_or_else(|| "  —  ".into(), |p| format!("{p:.2}")),
            fmt(lo),
            fmt(hi)
        );
    }

    println!(
        "\n{DIM}  Nothing here is applied automatically. Read the separation section first —\n  \
         a proposal computed from overlapping distributions is a number, not a threshold.{R}\n"
    );
}

// ─── check: the CI gate ──────────────────────────────────────────────────

pub fn check(
    arm: &Arm,
    objections: &[Objection],
    probes: &[Probe],
    min_recall: Option<f32>,
) -> usize {
    let mut failures = 0;

    // M1: no objection matches another's triggers above threshold.
    for owner in objections {
        for trigger in &owner.triggers {
            for s in arm.score(trigger) {
                if s.objection_id == owner.id || !s.is_hit() || s.kind == ObjectionKind::Comply {
                    continue;
                }
                println!(
                    "{BAD}FAIL{R} cross-match: {}'s trigger {DIM}'{}'{R} fired {} at {:.3} ≥ {:.2}",
                    owner.label, trigger, s.label, s.score, s.threshold
                );
                failures += 1;
            }
        }
    }

    // M1: each objection matches at least 3 of its own triggers.
    for o in objections {
        let want = 3.min(o.triggers.len());
        let n = o
            .triggers
            .iter()
            .filter(|t| {
                arm.score(t)
                    .into_iter()
                    .find(|s| s.objection_id == o.id)
                    .is_some_and(|s| s.score >= o.effective_threshold())
            })
            .count();
        if n < want {
            println!(
                "{BAD}FAIL{R} self-match: {} hit {n}/{} of its own triggers, need {want}",
                o.label,
                o.triggers.len()
            );
            failures += 1;
        }
    }

    // Compliance: every removal request in the probe set is caught. This is not
    // a quality bar, it is docs/06-COMPLIANCE.md §3.
    for p in probes {
        let Some(want) = p.expected() else { continue };
        if !Arm::is_comply(objections, want) {
            continue;
        }
        if !arm
            .hit(&p.text)
            .is_some_and(|h| h.kind == ObjectionKind::Comply)
        {
            println!(
                "{BAD}FAIL{R} compliance: removal request not caught — {DIM}\"{}\"{R}",
                p.text
            );
            failures += 1;
        }
    }

    if let Some(floor) = min_recall {
        let labelled = probes.iter().filter(|p| p.expected().is_some()).count();
        let tp = probes
            .iter()
            .filter(|p| {
                p.expected()
                    .is_some_and(|w| arm.hit(&p.text).is_some_and(|h| h.objection_id == w))
            })
            .count();
        let recall = tp as f32 / labelled.max(1) as f32;
        if recall < floor {
            println!("{BAD}FAIL{R} recall {recall:.2} below required {floor:.2}");
            failures += 1;
        }
    }

    if failures == 0 {
        println!(
            "{OK}PASS{R} {DIM}{} — all M1 acceptance criteria and the compliance gate{R}",
            arm.name
        );
    } else {
        println!("\n{BAD}{failures} failure(s){R}");
    }
    failures
}
