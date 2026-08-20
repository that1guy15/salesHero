//! The three scoring arms under measurement.
//!
//! Lexical and BGE live in different vector spaces, so they cannot be combined
//! by averaging vectors — the hybrid combines *scores*, one gate per embedder.
//! That is deliberately a measurement-time construct: M1.5's job is to find out
//! whether combining helps before any of it is wired into the pipeline.

use riposte_core::{Objection, ObjectionKind};
use riposte_detect::{DetectionGate, Embedder, HashEmbedder, Score};

pub struct Arm {
    pub name: String,
    lexical: (DetectionGate, Box<dyn Embedder>),
    semantic: Option<(DetectionGate, Box<dyn Embedder>)>,
    /// Similarity floor of the semantic embedder, measured from the
    /// far-negative probes rather than assumed. BGE-small puts unrelated short
    /// sentences at 0.6–0.75 (`docs/PROJECT-STATUS.md` §3.1), which is inside
    /// the shipped threshold band — so the raw number is not comparable to a
    /// lexical one and must be rescaled before the two are combined.
    floor: f32,
}

impl Arm {
    pub fn lexical(objections: &[Objection]) -> Self {
        let e = HashEmbedder::new();
        let gate = DetectionGate::build(objections, &e);
        Arm {
            name: e.model_id().to_string(),
            lexical: (gate, Box::new(e)),
            semantic: None,
            floor: 0.0,
        }
    }

    #[cfg(feature = "onnx")]
    pub fn semantic(objections: &[Objection]) -> anyhow::Result<Self> {
        let e = riposte_detect::OnnxEmbedder::new()
            .map_err(|e| anyhow::anyhow!("loading BGE-small: {e}"))?;
        let gate = DetectionGate::build(objections, &e);
        let lex = HashEmbedder::new();
        Ok(Arm {
            name: e.model_id().to_string(),
            // A semantic arm still needs a lexical gate present so the comply
            // containment path is identical across arms; only `semantic` is
            // consulted for scores.
            lexical: (DetectionGate::build(objections, &lex), Box::new(lex)),
            semantic: Some((gate, Box::new(e))),
            floor: 0.0,
        })
    }

    #[cfg(feature = "onnx")]
    pub fn hybrid(objections: &[Objection]) -> anyhow::Result<Self> {
        let mut a = Self::semantic(objections)?;
        a.name = "hybrid(lexical + bge-small)".into();
        Ok(a)
    }

    pub fn is_hybrid(&self) -> bool {
        self.name.starts_with("hybrid")
    }

    /// Calibrates the semantic floor from utterances known to match nothing.
    pub fn set_floor_from(&mut self, far_negatives: &[String]) {
        let Some((gate, emb)) = &self.semantic else {
            return;
        };
        let mut all: Vec<f32> = Vec::new();
        for t in far_negatives {
            all.extend(gate.score(t, emb.as_ref()).iter().map(|s| s.positive));
        }
        // The 90th percentile of "unrelated" is the floor worth subtracting:
        // the mean understates it and the max is one outlier away from useless.
        let sorted = crate::stats::sorted(all);
        self.floor = crate::stats::percentile(&sorted, 90.0).unwrap_or(0.0);
    }

    pub fn floor(&self) -> f32 {
        self.floor
    }

    pub fn has_semantic(&self) -> bool {
        self.semantic.is_some()
    }

    /// Raw, un-rescaled scores from the semantic embedder.
    ///
    /// The shipped thresholds were authored for *these* numbers. Reporting them
    /// is what shows whether the thresholds sit inside the embedder's noise
    /// floor, which is the question `docs/PROJECT-STATUS.md` §3.1 raised.
    pub fn raw_semantic(&self, text: &str) -> Vec<Score> {
        match &self.semantic {
            Some((gate, emb)) => gate.score(text, emb.as_ref()),
            None => Vec::new(),
        }
    }

    fn rescale(&self, raw: f32) -> f32 {
        if self.floor <= 0.0 || self.floor >= 1.0 {
            return raw;
        }
        ((raw - self.floor) / (1.0 - self.floor)).clamp(0.0, 1.0)
    }

    /// Every objection's score for one utterance, best first.
    pub fn score(&self, text: &str) -> Vec<Score> {
        let (lex_gate, lex_emb) = &self.lexical;
        let lex = lex_gate.score(text, lex_emb.as_ref());

        let Some((sem_gate, sem_emb)) = &self.semantic else {
            return lex;
        };
        let mut sem = sem_gate.score(text, sem_emb.as_ref());

        for s in sem.iter_mut() {
            let rescaled = self.rescale(s.positive);
            let negative = self.rescale(s.negative);
            s.positive = rescaled;
            s.negative = negative;
            s.score = rescaled - 0.5 * negative;

            if self.is_hybrid() {
                // Take the stronger of the two signals. They fail in opposite
                // directions — lexical has excellent precision and near-zero
                // recall on paraphrase; BGE has recall and a floor that eats
                // precision — so a max recovers each one's strength without
                // letting either one's weakness drag the other down.
                //
                // A comply entry that matched lexically scores 1.0 by
                // containment (D11); max preserves that, which is the
                // behaviour that must not regress.
                if let Some(l) = lex.iter().find(|l| l.objection_id == s.objection_id) {
                    if l.score > s.score {
                        s.score = l.score;
                        s.positive = l.positive;
                        s.negative = l.negative;
                        s.best_trigger = l.best_trigger.clone();
                    }
                }
            }
        }
        sem.sort_by(|a, b| b.score.total_cmp(&a.score));
        sem
    }

    /// The hit this arm would act on, applying the same comply-first rule the
    /// pipeline uses. See `riposte_detect::select_hit`.
    pub fn hit(&self, text: &str) -> Option<Score> {
        let scores = self.score(text);
        riposte_detect::select_hit(&scores).cloned()
    }

    pub fn is_comply(objections: &[Objection], id: &str) -> bool {
        objections
            .iter()
            .any(|o| o.id == id && o.kind == ObjectionKind::Comply)
    }
}
