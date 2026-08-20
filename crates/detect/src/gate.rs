//! Stage 1 — the cheap local gate. See `docs/02-ARCHITECTURE.md` §7 and
//! `docs/04-CONTRACTS.md` §4.
//!
//! Pure and synchronous: takes the far-end window, returns at most one hit.
//! Rejecting here costs nothing; only a hit goes on to spend an LLM call.

use crate::embed::{cosine, normalize_tokens, Embedder};
use riposte_core::{Objection, ObjectionKind, Speaker, Utterance};

/// Weight applied to a negative trigger that out-competes every positive one.
///
/// Subtracting rather than hard-excluding lets a strong positive still win,
/// which matters because negative triggers are learned from user error and are
/// noisier than authored ones.
const NEGATIVE_WEIGHT: f32 = 0.5;

/// How much a negative trigger suppresses this objection.
///
/// `04-CONTRACTS.md` §4 specifies `positive − 0.5 × negative` on the raw
/// negative score. Measured on the shipped playbook, that suppresses an
/// objection's *own authored trigger*: "I didn't request a quote" scores 1.000
/// against `obj_no_form`, but its near-identical negative trigger "yes I did
/// request a quote" scores 0.786, giving 1.000 − 0.393 = 0.607 — under the 0.62
/// threshold. A positive and a negative that differ only in polarity share
/// almost every lexical feature, so the negative fires whenever the positive
/// does.
///
/// A negative therefore only counts when it explains the utterance *better*
/// than any positive one does — and when it does, it counts at full weight.
/// The two cases the shipped playbook actually contains:
///
/// | utterance | positive | negative | verdict |
/// |---|---|---|---|
/// | "I didn't request a quote" (a trigger) | 1.000 | 0.786 | fires — positive wins |
/// | "yes I did request a quote" (a negative trigger) | 0.786 | 1.000 | suppressed |
///
/// A margin-proportional penalty cannot separate these: the margin is 0.214
/// either way, and only its *sign* says which reading is right.
///
/// This also makes the documented intent literally true — "subtracting rather
/// than hard-excluding lets a strong positive still win" — where before, a
/// negative trigger suppressed in proportion to its absolute score even when
/// the positive beat it outright. See `docs/DECISIONS.md` D16.
fn penalty(positive: f32, negative: f32) -> f32 {
    if negative > positive {
        NEGATIVE_WEIGHT * negative
    } else {
        0.0
    }
}

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

/// The one hit to act on, from an already-sorted score list.
///
/// A qualifying `comply` entry wins outright, whatever outscored it. Selecting
/// purely by score means a prospect saying "I already have insurance, quit
/// calling" is served a rebuttal for a removal request — the exact failure
/// `docs/06-COMPLIANCE.md` §3 exists to prevent. Every other enforcement layer
/// protects what happens *after* the objection is chosen; this protects the
/// choice itself.
///
/// Shared by [`DetectionGate::evaluate`] and the CLI so the two cannot drift.
pub fn select_hit(scores: &[Score]) -> Option<&Score> {
    scores
        .iter()
        .find(|s| s.kind == ObjectionKind::Comply && s.is_hit())
        .or_else(|| scores.iter().find(|s| s.is_hit()))
}

/// A trigger phrase with everything the gate needs to match it: the embedding
/// for the semantic path, and the normalised tokens for the lexical comply
/// scan described on [`comply_containment`].
struct Trigger {
    phrase: String,
    vector: Vec<f32>,
    tokens: Vec<String>,
}

struct Entry {
    objection_id: String,
    label: String,
    kind: ObjectionKind,
    threshold: f32,
    positives: Vec<Trigger>,
    negatives: Vec<Trigger>,
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
                positives: o.triggers.iter().map(|p| trigger(p, embedder)).collect(),
                negatives: o
                    .negative_triggers
                    .iter()
                    .map(|p| trigger(p, embedder))
                    .collect(),
            })
            .collect();

        DetectionGate {
            entries,
            model_id: embedder.model_id().to_string(),
        }
    }

    /// Builds the gate from embeddings already in the store, so a playbook is
    /// embedded once at import rather than on every evaluation.
    ///
    /// `DetectionGate::build` re-embeds every trigger phrase each time it is
    /// called. That is free with [`HashEmbedder`] but costs seconds with
    /// BGE-small, which made the live pipeline impossible — `M1` of
    /// `docs/07-BUILD-PLAN.md` specifies embedding "at playbook save time".
    ///
    /// Phrases with no stored vector for `model_id` are skipped rather than
    /// embedded on the fly: a partially embedded playbook is a bug in the
    /// import path, and silently papering over it would hide the seconds it
    /// costs. Callers run [`missing_vectors`] first.
    pub fn from_vectors(
        objections: &[Objection],
        vectors: &[riposte_store::TriggerVector],
        model_id: &str,
    ) -> Self {
        let entries = objections
            .iter()
            .filter(|o| o.enabled)
            .map(|o| {
                let mine = |positive: bool| -> Vec<Trigger> {
                    vectors
                        .iter()
                        .filter(|v| v.objection_id == o.id && v.positive == positive)
                        .map(|v| Trigger {
                            phrase: v.phrase.clone(),
                            vector: v.vector.clone(),
                            tokens: normalize_tokens(&v.phrase),
                        })
                        .collect()
                };
                Entry {
                    objection_id: o.id.clone(),
                    label: o.label.clone(),
                    kind: o.kind,
                    threshold: o.effective_threshold(),
                    positives: mine(true),
                    negatives: mine(false),
                }
            })
            .collect();

        DetectionGate {
            entries,
            model_id: model_id.to_string(),
        }
    }

    /// Enabled objections that have no stored positive vectors — i.e. the
    /// playbook was never embedded for this model, or was edited since.
    pub fn missing_vectors(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.positives.is_empty())
            .map(|e| e.objection_id.as_str())
            .collect()
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn objection_count(&self) -> usize {
        self.entries.len()
    }

    fn score_one(&self, e: &Entry, vec: &[f32], text: &str) -> Score {
        let (mut best_trigger, positive) = e
            .positives
            .iter()
            .map(|t| (t.phrase.as_str(), cosine(vec, &t.vector)))
            .fold(("", f32::MIN), |acc, x| if x.1 > acc.1 { x } else { acc });

        let negative = e
            .negatives
            .iter()
            .map(|t| cosine(vec, &t.vector))
            .fold(0.0f32, f32::max);

        let mut positive = if positive == f32::MIN { 0.0 } else { positive };

        // Compliance: a removal request stated inside a longer utterance must
        // not be diluted below threshold by the words around it. See
        // [`comply_containment`].
        if e.kind == ObjectionKind::Comply {
            if let Some(matched) = comply_containment(&e.positives, text) {
                if positive < 1.0 {
                    positive = 1.0;
                    best_trigger = matched;
                }
            }
        }

        Score {
            objection_id: e.objection_id.clone(),
            label: e.label.clone(),
            kind: e.kind,
            positive,
            negative,
            score: positive - penalty(positive, negative),
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

    /// At most one hit, chosen by [`select_hit`].
    pub fn evaluate(&self, window: &[Utterance], embedder: &dyn Embedder) -> Option<GateHit> {
        if window.is_empty() {
            return None;
        }
        let candidates = Self::candidates(window);
        let scores = self.score_candidates(&candidates, embedder);
        select_hit(&scores).cloned().map(|s| GateHit {
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

// ─── Compliance: lexical containment ─────────────────────────────────────

fn trigger(phrase: &str, embedder: &dyn Embedder) -> Trigger {
    Trigger {
        phrase: phrase.to_string(),
        vector: embedder.embed(phrase),
        tokens: normalize_tokens(phrase),
    }
}

/// Does this span contain a `comply` trigger verbatim, whatever else it says?
///
/// The embedding gate scores a whole span, so a removal request stated
/// alongside anything else is diluted by the surrounding words. Measured on the
/// shipped playbook: "quit calling" alone scores 1.000 against `obj_dnc`, but
/// "I'm not looking for anything, quit calling" scores 0.387 — under `obj_dnc`'s
/// 0.55 threshold — while "Not interested" fires at 0.725. The agent is then
/// coached to rebut a withdrawal of consent.
///
/// D9's suffix scoring cannot help: it splits on *utterance* boundaries, and
/// both clauses arrive in one ASR utterance.
///
/// So `comply` entries also get a lexical pass. If a trigger's tokens appear as
/// a contiguous run anywhere in the span, it is a hit at full strength. This is
/// exact rather than fuzzy — requiring the *whole* trigger contiguously means
/// "don't call her at work, call me instead" does not match "don't call here
/// again". It costs no embedding, it is immune to dilution by construction, and
/// it behaves identically under any [`Embedder`], which matters because the
/// semantic gate is not yet calibrated (`docs/PROJECT-STATUS.md` §3.1).
///
/// Erring toward firing is correct here: `obj_dnc` ships at threshold 0.55
/// precisely because "a false positive here costs nothing".
fn comply_containment<'a>(triggers: &'a [Trigger], span: &str) -> Option<&'a str> {
    let haystack = normalize_tokens(span);
    triggers
        .iter()
        .filter(|t| !t.tokens.is_empty() && t.tokens.len() <= haystack.len())
        .find(|t| {
            haystack
                .windows(t.tokens.len())
                .any(|w| w == t.tokens.as_slice())
        })
        .map(|t| t.phrase.as_str())
}

// ─── Window construction ─────────────────────────────────────────────────

/// Objections build across turns — "Hmm." / "Yeah, I don't know." / "We're
/// already covered." Matching a single utterance misses these constantly.
pub const WINDOW_TURNS: usize = 3;
pub const WINDOW_MS: i64 = 30_000;

/// Runtime tuning for window construction and dedupe.
///
/// `02-ARCHITECTURE.md` §5 requires these be config-exposed — "different
/// dialers behave differently and this will need field tuning". The playbook
/// carries them in its `settings:` block; before this they were parsed,
/// discarded, and the gate used hardcoded constants regardless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateConfig {
    pub window_turns: usize,
    pub window_ms: i64,
    pub dedupe_window_ms: i64,
    pub card_lifetime_ms: i64,
}

impl Default for GateConfig {
    fn default() -> Self {
        GateConfig {
            window_turns: WINDOW_TURNS,
            window_ms: WINDOW_MS,
            dedupe_window_ms: DEDUPE_WINDOW_MS,
            card_lifetime_ms: 90_000,
        }
    }
}

/// The last [`GateConfig::window_turns`] prospect turns, or
/// [`GateConfig::window_ms`], whichever is shorter. Agent turns are excluded:
/// v1 detects on the prospect stream only (`docs/01-PRD.md` §8).
pub fn build_window_with(utterances: &[Utterance], cfg: &GateConfig) -> Vec<Utterance> {
    let prospect: Vec<&Utterance> = utterances
        .iter()
        .filter(|u| u.speaker == Speaker::Prospect)
        .collect();

    let Some(last) = prospect.last() else {
        return vec![];
    };
    let cutoff = last.t_end_ms - cfg.window_ms;

    prospect
        .iter()
        .rev()
        .take(cfg.window_turns)
        .filter(|u| u.t_end_ms >= cutoff)
        .rev()
        .map(|u| (*u).clone())
        .collect()
}

/// [`build_window_with`] using the shipped defaults.
pub fn build_window(utterances: &[Utterance]) -> Vec<Utterance> {
    build_window_with(utterances, &GateConfig::default())
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

#[derive(Debug, Clone)]
struct Seen {
    objection_id: String,
    /// When this objection *first* fired inside the current suppression
    /// window. Deliberately not refreshed on a repeat — see
    /// [`Dedupe::should_suppress`].
    first_ms: i64,
    /// Times it has fired since the last [`Dedupe::reset`], i.e. within this
    /// call. Not reset when the suppression window expires: the count is a
    /// property of the call, the window is a property of the card.
    hits: u32,
}

/// Suppresses a repeat of the same objection inside one call, and counts the
/// repeats so the call lifecycle can act on them.
///
/// Reset on `CallStarted` — a new prospect is a new chance to say the same
/// thing.
#[derive(Debug, Default)]
pub struct Dedupe {
    seen: Vec<Seen>,
    window_ms: i64,
}

impl Dedupe {
    pub fn new(window_ms: i64) -> Self {
        Dedupe {
            seen: Vec::new(),
            window_ms,
        }
    }

    pub fn with_default_window() -> Self {
        Self::new(DEDUPE_WINDOW_MS)
    }

    /// `true` if this hit should be suppressed. Records it either way.
    ///
    /// The window is measured from the objection's *first* fire, not its most
    /// recent one. Refreshing the timestamp on every suppressed hit turns the
    /// fixed 45 s window of `02-ARCHITECTURE.md` §7.1 into a sliding one, under
    /// which a prospect repeating an objection every 40 s is suppressed for the
    /// entire call and never gets a second card.
    pub fn should_suppress(&mut self, objection_id: &str, now_ms: i64) -> bool {
        let window = self.window_ms;
        // Expire the suppression window without forgetting the call-level
        // count, which D8 depends on.
        for s in self.seen.iter_mut() {
            if now_ms - s.first_ms >= window {
                s.first_ms = i64::MIN;
            }
        }
        match self
            .seen
            .iter_mut()
            .find(|s| s.objection_id == objection_id)
        {
            Some(s) => {
                s.hits += 1;
                if s.first_ms == i64::MIN {
                    s.first_ms = now_ms;
                    false
                } else {
                    true
                }
            }
            None => {
                self.seen.push(Seen {
                    objection_id: objection_id.to_string(),
                    first_ms: now_ms,
                    hits: 1,
                });
                false
            }
        }
    }

    /// How many times this objection has fired since the last [`Dedupe::reset`]
    /// — i.e. within the current call, suppressed hits included. Feeds the
    /// repeat-denial escalation in `docs/DECISIONS.md` D8.
    pub fn hits(&self, objection_id: &str) -> u32 {
        self.seen
            .iter()
            .find(|s| s.objection_id == objection_id)
            .map_or(0, |s| s.hits)
    }

    pub fn reset(&mut self) {
        self.seen.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::HashEmbedder;

    fn objection(
        id: &str,
        label: &str,
        kind: ObjectionKind,
        triggers: &[&str],
        negatives: &[&str],
    ) -> Objection {
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
            escalate_after_repeats: None,
        }
    }

    fn utt(text: &str, t: i64) -> Utterance {
        Utterance {
            speaker: Speaker::Prospect,
            text: text.into(),
            t_start_ms: t,
            t_end_ms: t + 1000,
            from_preroll: false,
        }
    }

    #[test]
    fn negative_triggers_reduce_the_score() {
        let e = HashEmbedder::new();
        let without = DetectionGate::build(
            &[objection(
                "a",
                "A",
                ObjectionKind::Rebut,
                &["I never filled out any form"],
                &[],
            )],
            &e,
        );
        let with = DetectionGate::build(
            &[objection(
                "a",
                "A",
                ObjectionKind::Rebut,
                &["I never filled out any form"],
                &["I filled out the form yesterday"],
            )],
            &e,
        );
        let text = "I filled out the form yesterday";
        let s1 = without.score(text, &e)[0].score;
        let s2 = with.score(text, &e)[0].score;
        assert!(
            s2 < s1,
            "negative trigger did not reduce score: {s1} -> {s2}"
        );
    }

    /// Found by the M1.5 calibration harness: `obj_no_form`'s own authored
    /// trigger "I didn't request a quote" scored 1.000, and its near-identical
    /// negative "yes I did request a quote" scored 0.786. Under
    /// `positive - 0.5 * negative` that is 0.607, below the objection's own
    /// 0.62 threshold — an objection suppressed by its own trigger.
    #[test]
    fn a_negative_does_not_suppress_a_stronger_positive() {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(
            &[objection(
                "obj_no_form",
                "Never filled out a form",
                ObjectionKind::Rebut,
                &["I didn't request a quote", "I never filled out any form"],
                &["yes I did request a quote", "I filled that out yesterday"],
            )],
            &e,
        );
        for phrase in ["I didn't request a quote", "I never filled out any form"] {
            let s = &gate.score(phrase, &e)[0];
            assert!(
                s.is_hit(),
                "{phrase:?} is an authored trigger and scored {:.3} < {:.2} \
                 (positive {:.3}, negative {:.3})",
                s.score,
                s.threshold,
                s.positive,
                s.negative
            );
        }
    }

    /// The other side of the same boundary: when the negative genuinely is the
    /// better reading, it must still suppress.
    #[test]
    fn a_negative_still_suppresses_when_it_outscores_the_positive() {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(
            &[objection(
                "obj_no_form",
                "Never filled out a form",
                ObjectionKind::Rebut,
                &["I didn't request a quote", "I never filled out any form"],
                &["yes I did request a quote", "I filled that out yesterday"],
            )],
            &e,
        );
        for phrase in ["yes I did request a quote", "I filled that out yesterday"] {
            let s = &gate.score(phrase, &e)[0];
            assert!(
                !s.is_hit(),
                "{phrase:?} affirms the opposite and scored {:.3} >= {:.2}",
                s.score,
                s.threshold
            );
        }
    }

    #[test]
    fn window_keeps_only_prospect_turns() {
        let mut u = vec![utt("hello", 0)];
        u.push(Utterance {
            speaker: Speaker::Agent,
            text: "hi there".into(),
            t_start_ms: 1000,
            t_end_ms: 2000,
            from_preroll: false,
        });
        u.push(utt("not interested", 2000));
        let w = build_window(&u);
        assert_eq!(w.len(), 2);
        assert!(w.iter().all(|x| x.speaker == Speaker::Prospect));
        assert_eq!(window_text(&w), "hello not interested");
    }

    #[test]
    fn window_config_actually_changes_the_window() {
        let u: Vec<_> = (0..6).map(|i| utt(&format!("turn{i}"), i * 1000)).collect();
        let cfg = GateConfig {
            window_turns: 2,
            ..GateConfig::default()
        };
        assert_eq!(window_text(&build_window_with(&u, &cfg)), "turn4 turn5");

        // Turns end at t+1000, so the last ends at 6000. A 500 ms window puts
        // the cutoff at 5500, past turn4's end of 5000.
        let tight = GateConfig {
            window_ms: 500,
            ..GateConfig::default()
        };
        assert_eq!(window_text(&build_window_with(&u, &tight)), "turn5");
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
            &[objection(
                "a",
                "Already covered",
                ObjectionKind::Rebut,
                &["we are already covered"],
                &[],
            )],
            &e,
        );
        // Neither fragment alone is the trigger; together they are.
        let u = vec![
            utt("hmm", 0),
            utt("yeah I don't know", 2000),
            utt("we are already covered", 4000),
        ];
        let w = build_window(&u);
        assert!(
            gate.evaluate(&w, &e).is_some(),
            "multi-turn window should catch this"
        );
    }

    #[test]
    fn dedupe_suppresses_a_repeat_inside_the_window() {
        let mut d = Dedupe::with_default_window();
        assert!(!d.should_suppress("obj_a", 0));
        assert!(d.should_suppress("obj_a", 10_000));
        assert!(
            !d.should_suppress("obj_b", 10_000),
            "different objection is not suppressed"
        );
    }

    #[test]
    fn dedupe_expires_after_the_window() {
        let mut d = Dedupe::new(45_000);
        assert!(!d.should_suppress("obj_a", 0));
        assert!(!d.should_suppress("obj_a", 46_000));
    }

    /// The window runs from the objection's *first* fire. Refreshing it on
    /// every suppressed hit makes it slide, and a prospect repeating an
    /// objection just inside the window is then suppressed for the whole call.
    #[test]
    fn dedupe_window_is_fixed_not_sliding() {
        let mut d = Dedupe::new(45_000);
        assert!(!d.should_suppress("obj_a", 0));
        // Repeats inside the window are suppressed and must not extend it.
        assert!(d.should_suppress("obj_a", 40_000));
        assert!(d.should_suppress("obj_a", 44_000));
        assert!(
            !d.should_suppress("obj_a", 46_000),
            "window slid: 46s is past a window opened at 0"
        );
    }

    /// D8 needs a count, and `PROJECT-STATUS.md` §4.4 claimed one already
    /// existed. It did not — only a last-seen timestamp was stored.
    #[test]
    fn dedupe_counts_repeats_across_the_whole_call() {
        let mut d = Dedupe::with_default_window();
        assert_eq!(d.hits("obj_a"), 0);
        d.should_suppress("obj_a", 0);
        assert_eq!(d.hits("obj_a"), 1);
        d.should_suppress("obj_a", 10_000); // suppressed, still a repeat
        assert_eq!(d.hits("obj_a"), 2);
        d.should_suppress("obj_a", 90_000); // new window, still the same call
        assert_eq!(d.hits("obj_a"), 3, "count is per call, not per window");
        d.reset();
        assert_eq!(d.hits("obj_a"), 0, "a new call starts the count over");
    }

    #[test]
    fn dedupe_resets_on_a_new_call() {
        let mut d = Dedupe::with_default_window();
        assert!(!d.should_suppress("obj_a", 0));
        d.reset();
        assert!(
            !d.should_suppress("obj_a", 1000),
            "a new call is a new chance to object"
        );
    }

    // ─── Compliance: the gate's own selection ────────────────────────────
    //
    // These guard the failure demonstrated on the shipped playbook: a removal
    // request stated in the same breath as another objection was served a
    // rebuttal. See `comply_containment` and `select_hit`.

    fn dnc_and_rebuttal() -> Vec<Objection> {
        let mut dnc = objection(
            "obj_dnc",
            "Take me off your list",
            ObjectionKind::Comply,
            &["stop calling me", "quit calling", "take me off your list"],
            &[],
        );
        dnc.threshold_authored = 0.55;
        let mut rebut = objection(
            "obj_not_interested",
            "Not interested",
            ObjectionKind::Rebut,
            &["not interested", "I'm not looking for anything"],
            &[],
        );
        rebut.threshold_authored = 0.64;
        vec![rebut, dnc]
    }

    /// Required test. A withdrawal of consent inside a longer utterance is
    /// never met with a rebuttal, however strongly the rebuttal scored.
    #[test]
    fn comply_wins_a_mixed_utterance() {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(&dnc_and_rebuttal(), &e);

        for phrase in [
            "I'm not looking for anything, quit calling",
            "not interested, stop calling me",
            "I'm not looking for anything and I want you to take me off your list",
        ] {
            let hit = gate
                .evaluate(&[Utterance::prospect(phrase)], &e)
                .unwrap_or_else(|| panic!("no hit at all for {phrase:?}"));
            assert_eq!(
                hit.kind,
                ObjectionKind::Comply,
                "{phrase:?} was served a {:?} card ({}) — docs/06-COMPLIANCE.md §3",
                hit.kind,
                hit.objection_id
            );
        }
    }

    /// The dilution half of the same defect. "quit calling" alone scores 1.000
    /// against `obj_dnc`; one clause in front of it dropped the whole span to
    /// 0.387, under the 0.55 threshold, so the comply entry never qualified and
    /// could not have won on priority alone.
    #[test]
    fn dnc_survives_clause_dilution() {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(&dnc_and_rebuttal(), &e);
        let diluted = gate.score("I'm not looking for anything, quit calling", &e);
        let dnc = diluted
            .iter()
            .find(|s| s.objection_id == "obj_dnc")
            .unwrap();
        assert!(
            dnc.is_hit(),
            "DNC diluted to {:.3} against threshold {:.2}",
            dnc.score,
            dnc.threshold
        );
        assert_eq!(dnc.best_trigger, "quit calling");
    }

    /// Containment is exact, not fuzzy: a stray "don't call" in ordinary speech
    /// must not be read as a removal request.
    #[test]
    fn containment_does_not_fire_on_a_partial_trigger() {
        let e = HashEmbedder::new();
        let mut dnc = objection(
            "obj_dnc",
            "Take me off your list",
            ObjectionKind::Comply,
            &["don't call here again"],
            &[],
        );
        dnc.threshold_authored = 0.55;
        let gate = DetectionGate::build(&[dnc], &e);
        let hit = gate.evaluate(
            &[Utterance::prospect(
                "don't call her at work, call me on my cell instead",
            )],
            &e,
        );
        assert!(hit.is_none(), "partial trigger match fired: {hit:?}");
    }

    /// A rebuttable objection with no comply in play is unaffected.
    #[test]
    fn priority_does_not_suppress_ordinary_rebuttals() {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(&dnc_and_rebuttal(), &e);
        let hit = gate
            .evaluate(&[Utterance::prospect("not interested")], &e)
            .expect("plain objection should still fire");
        assert_eq!(hit.objection_id, "obj_not_interested");
    }

    #[test]
    fn empty_window_never_hits() {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(
            &[objection(
                "a",
                "A",
                ObjectionKind::Rebut,
                &["stop calling"],
                &[],
            )],
            &e,
        );
        assert!(gate.evaluate(&[], &e).is_none());
    }
}
