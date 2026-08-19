//! Stage 1 — the cheap local gate. See `docs/02-ARCHITECTURE.md` §7 and
//! `docs/04-CONTRACTS.md` §4.
//!
//! Pure and synchronous: takes the far-end window, returns at most one hit.
//! Rejecting here costs nothing; only a hit goes on to spend an LLM call.

use crate::embed::{cosine, Embedder};
use riposte_core::{Objection, ObjectionKind, Speaker, Utterance};

/// Weight applied to the best negative-trigger match. Subtracting rather than
/// hard-excluding lets a strong positive still win, which matters because
/// negative triggers are learned from user error and are noisier than
/// authored ones.
const NEGATIVE_WEIGHT: f32 = 0.5;

#[derive(Debug, Clone)]
pub struct GateHit {
    pub objection_id: String,
    pub label: String,
    pub kind: ObjectionKind,
    pub similarity: f32,
    pub window_text: String,
}

/// Every objection's score for one window. Ordered best-first. Used by the
/// CLI and by the calibration path — seeing the runners-up is what tells you
/// whether a threshold is wrong.
#[derive(Debug, Clone)]
pub struct Score {
    pub objection_id: String,
    pub label: String,
    pub kind: ObjectionKind,
    pub positive: f32,
    pub negative: f32,
    pub score: f32,
    pub threshold: f32,
    pub best_trigger: String,
    /// Which candidate span actually produced this score. Usually a suffix of
    /// the window rather than the whole thing — see [`DetectionGate::evaluate`].
    pub matched_text: String,
}

impl Score {
    pub fn is_hit(&self) -> bool {
        self.score >= self.threshold
    }
}

struct Entry {
    objection_id: String,
    label: String,
    kind: ObjectionKind,
    threshold: f32,
    positives: Vec<(String, Vec<f32>)>,
    negatives: Vec<(String, Vec<f32>)>,
}

pub struct DetectionGate {
    entries: Vec<Entry>,
    model_id: String,
}

impl DetectionGate {
    /// Embeds every trigger phrase up front. Called at playbook load, not per
    /// utterance.
    pub fn build(objections: &[Objection], embedder: &dyn Embedder) -> Self {
        let entries = objections
            .iter()
            .filter(|o| o.enabled)
            .map(|o| Entry {
                objection_id: o.id.clone(),
                label: o.label.clone(),
                kind: o.kind,
                threshold: o.effective_threshold(),
                positives: o
                    .triggers
                    .iter()
                    .map(|p| (p.clone(), embedder.embed(p)))
                    .collect(),
                negatives: o
                    .negative_triggers
                    .iter()
                    .map(|p| (p.clone(), embedder.embed(p)))
                    .collect(),
            })
            .collect();

        DetectionGate { entries, model_id: embedder.model_id().to_string() }
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn objection_count(&self) -> usize {
        self.entries.len()
    }

    fn score_one(&self, e: &Entry, vec: &[f32], text: &str) -> Score {
        let (best_trigger, positive) = e
            .positives
            .iter()
            .map(|(p, v)| (p.as_str(), cosine(vec, v)))
            .fold(("", f32::MIN), |acc, x| if x.1 > acc.1 { x } else { acc });

        let negative = e
            .negatives
            .iter()
            .map(|(_, v)| cosine(vec, v))
            .fold(0.0f32, f32::max);

        let positive = if positive == f32::MIN { 0.0 } else { positive };

        Score {
            objection_id: e.objection_id.clone(),
            label: e.label.clone(),
            kind: e.kind,
            positive,
            negative,
            score: positive - NEGATIVE_WEIGHT * negative,
            threshold: e.threshold,
            best_trigger: best_trigger.to_string(),
            matched_text: text.to_string(),
        }
    }

    /// Scores every objection against each candidate span, keeping each
    /// objection's best.
    ///
    /// Scoring only the whole concatenated window loses real objections to
    /// dilution: "hmm — yeah, I don't know — we're already covered" is mostly
    /// filler, and averaging the filler in drags the cosine under threshold.
    /// Scoring suffixes as well means a clean single turn still matches at
    /// full strength while a genuinely split objection is still caught by the
    /// longer span. See `docs/DECISIONS.md` D9.
    pub fn score_candidates(&self, candidates: &[String], embedder: &dyn Embedder) -> Vec<Score> {
        let embedded: Vec<(String, Vec<f32>)> = candidates
            .iter()
            .filter(|c| !c.trim().is_empty())
            .map(|c| (c.clone(), embedder.embed(c)))
            .collect();

        let mut scores: Vec<Score> = self
            .entries
            .iter()
            .filter_map(|e| {
                embedded
                    .iter()
                    .map(|(text, vec)| self.score_one(e, vec, text))
                    .max_by(|a, b| a.score.total_cmp(&b.score))
            })
            .collect();

        scores.sort_by(|a, b| b.score.total_cmp(&a.score));
        scores
    }

    /// Scores a single span. Used by `riposte detect --text`.
    pub fn score(&self, text: &str, embedder: &dyn Embedder) -> Vec<Score> {
        self.score_candidates(std::slice::from_ref(&text.to_string()), embedder)
    }

    /// Every suffix of the window, longest first: the whole window, then the
    /// last two turns, then the last turn alone.
    fn candidates(window: &[Utterance]) -> Vec<String> {
        (0..window.len())
            .map(|start| window_text(&window[start..]))
            .collect()
    }

    /// At most one hit — the highest scorer that clears its own threshold.
    pub fn evaluate(&self, window: &[Utterance], embedder: &dyn Embedder) -> Option<GateHit> {
        if window.is_empty() {
            return None;
        }
        let candidates = Self::candidates(window);
        let scores = self.score_candidates(&candidates, embedder);
        scores.into_iter().find(Score::is_hit).map(|s| GateHit {
            objection_id: s.objection_id,
            label: s.label,
            kind: s.kind,
            similarity: s.score,
            // The span that actually matched, not the whole window — this is
            // what gets stored as `cards.window_text` and what negative
            // triggers are learned from on a `not_an_objection` verdict.
            window_text: s.matched_text,
        })
    }

    pub fn evaluate_text(&self, text: &str, embedder: &dyn Embedder) -> Option<GateHit> {
        self.evaluate(&[Utterance::prospect(text)], embedder)
    }
}

// ─── Window construction ─────────────────────────────────────────────────

/// Objections build across turns — "Hmm." / "Yeah, I don't know." / "We're
/// already covered." Matching a single utterance misses these constantly.
pub const WINDOW_TURNS: usize = 3;
pub const WINDOW_MS: i64 = 30_000;

/// The last [`WINDOW_TURNS`] prospect turns, or [`WINDOW_MS`], whichever is
/// shorter. Agent turns are excluded: v1 detects on the prospect stream only
/// (`docs/01-PRD.md` §8).
pub fn build_window(utterances: &[Utterance]) -> Vec<Utterance> {
    let prospect: Vec<&Utterance> = utterances
        .iter()
        .filter(|u| u.speaker == Speaker::Prospect)
        .collect();

    let Some(last) = prospect.last() else {
        return vec![];
    };
    let cutoff = last.t_end_ms - WINDOW_MS;

    prospect
        .iter()
        .rev()
        .take(WINDOW_TURNS)
        .filter(|u| u.t_end_ms >= cutoff)
        .rev()
        .map(|u| (*u).clone())
        .collect()
}

pub fn window_text(window: &[Utterance]) -> String {
    window
        .iter()
        .map(|u| u.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// ─── Dedupe ──────────────────────────────────────────────────────────────

pub const DEDUPE_WINDOW_MS: i64 = 45_000;

/// Suppresses a repeat of the same objection inside one call. Reset on
/// `CallStarted` — a new prospect is a new chance to say the same thing.
#[derive(Debug, Default)]
pub struct Dedupe {
    seen: Vec<(String, i64)>,
    window_ms: i64,
}

impl Dedupe {
    pub fn new(window_ms: i64) -> Self {
        Dedupe { seen: Vec::new(), window_ms }
    }

    pub fn with_default_window() -> Self {
        Self::new(DEDUPE_WINDOW_MS)
    }

    /// `true` if this hit should be suppressed. Records it either way.
    pub fn should_suppress(&mut self, objection_id: &str, now_ms: i64) -> bool {
        self.seen.retain(|(_, t)| now_ms - t < self.window_ms);
        if let Some((_, t)) = self.seen.iter_mut().find(|(id, _)| id == objection_id) {
            *t = now_ms;
            return true;
        }
        self.seen.push((objection_id.to_string(), now_ms));
        false
    }

    /// How many times this objection has fired since the last reset. Feeds the
    /// repeat-denial escalation in `docs/DECISIONS.md` D8.
    pub fn reset(&mut self) {
        self.seen.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::HashEmbedder;

    fn objection(id: &str, label: &str, kind: ObjectionKind, triggers: &[&str], negatives: &[&str]) -> Objection {
        Objection {
            id: id.into(),
            label: label.into(),
            category: None,
            kind,
            response: String::new(),
            talk_track: vec![],
            guardrails: vec![],
            rail_slot: None,
            threshold_authored: 0.62,
            threshold_learned: None,
            allow_refine: true,
            enabled: true,
            sort_order: 0,
            triggers: triggers.iter().map(|s| s.to_string()).collect(),
            negative_triggers: negatives.iter().map(|s| s.to_string()).collect(),
            doc_refs: vec![],
        }
    }

    fn utt(text: &str, t: i64) -> Utterance {
        Utterance { speaker: Speaker::Prospect, text: text.into(), t_start_ms: t, t_end_ms: t + 1000, from_preroll: false }
    }

    #[test]
    fn negative_triggers_reduce_the_score() {
        let e = HashEmbedder::new();
        let without = DetectionGate::build(
            &[objection("a", "A", ObjectionKind::Rebut, &["I never filled out any form"], &[])],
            &e,
        );
        let with = DetectionGate::build(
            &[objection("a", "A", ObjectionKind::Rebut, &["I never filled out any form"], &["I filled out the form yesterday"])],
            &e,
        );
        let text = "I filled out the form yesterday";
        let s1 = without.score(text, &e)[0].score;
        let s2 = with.score(text, &e)[0].score;
        assert!(s2 < s1, "negative trigger did not reduce score: {s1} -> {s2}");
    }

    #[test]
    fn window_keeps_only_prospect_turns() {
        let mut u = vec![utt("hello", 0)];
        u.push(Utterance { speaker: Speaker::Agent, text: "hi there".into(), t_start_ms: 1000, t_end_ms: 2000, from_preroll: false });
        u.push(utt("not interested", 2000));
        let w = build_window(&u);
        assert_eq!(w.len(), 2);
        assert!(w.iter().all(|x| x.speaker == Speaker::Prospect));
        assert_eq!(window_text(&w), "hello not interested");
    }

    #[test]
    fn window_caps_at_three_turns() {
        let u: Vec<_> = (0..6).map(|i| utt(&format!("turn{i}"), i * 1000)).collect();
        let w = build_window(&u);
        assert_eq!(w.len(), 3);
        assert_eq!(window_text(&w), "turn3 turn4 turn5");
    }

    #[test]
    fn window_drops_turns_older_than_thirty_seconds() {
        let u = vec![utt("ancient", 0), utt("recent", 40_000), utt("now", 41_000)];
        let w = build_window(&u);
        assert_eq!(window_text(&w), "recent now");
    }

    #[test]
    fn objection_split_across_turns_is_caught() {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(
            &[objection("a", "Already covered", ObjectionKind::Rebut, &["we are already covered"], &[])],
            &e,
        );
        // Neither fragment alone is the trigger; together they are.
        let u = vec![utt("hmm", 0), utt("yeah I don't know", 2000), utt("we are already covered", 4000)];
        let w = build_window(&u);
        assert!(gate.evaluate(&w, &e).is_some(), "multi-turn window should catch this");
    }

    #[test]
    fn dedupe_suppresses_a_repeat_inside_the_window() {
        let mut d = Dedupe::with_default_window();
        assert!(!d.should_suppress("obj_a", 0));
        assert!(d.should_suppress("obj_a", 10_000));
        assert!(!d.should_suppress("obj_b", 10_000), "different objection is not suppressed");
    }

    #[test]
    fn dedupe_expires_after_the_window() {
        let mut d = Dedupe::new(45_000);
        assert!(!d.should_suppress("obj_a", 0));
        assert!(!d.should_suppress("obj_a", 46_000));
    }

    #[test]
    fn dedupe_resets_on_a_new_call() {
        let mut d = Dedupe::with_default_window();
        assert!(!d.should_suppress("obj_a", 0));
        d.reset();
        assert!(!d.should_suppress("obj_a", 1000), "a new call is a new chance to object");
    }

    #[test]
    fn empty_window_never_hits() {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(&[objection("a", "A", ObjectionKind::Rebut, &["stop calling"], &[])], &e);
        assert!(gate.evaluate(&[], &e).is_none());
    }
}
