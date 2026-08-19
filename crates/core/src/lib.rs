//! Core domain types shared by every crate.
//!
//! Mirrors `docs/04-CONTRACTS.md` §1. Changing a type here is a cross-crate
//! decision — update the contract doc in the same commit.

use serde::{Deserialize, Serialize};

// ─── Speaker ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    Agent,
    Prospect,
}

impl Speaker {
    pub fn as_str(&self) -> &'static str {
        match self {
            Speaker::Agent => "agent",
            Speaker::Prospect => "prospect",
        }
    }
}

// ─── ObjectionKind ───────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectionKind {
    /// Full pipeline: promote, retrieve, refine.
    Rebut,
    /// Fixed authored card. Never sent to a model. See `docs/06-COMPLIANCE.md`.
    Comply,
    /// Ends the call cleanly; no rebuttal, but not a legal obligation.
    Disqualify,
}

impl ObjectionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ObjectionKind::Rebut => "rebut",
            ObjectionKind::Comply => "comply",
            ObjectionKind::Disqualify => "disqualify",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rebut" => Some(ObjectionKind::Rebut),
            "comply" => Some(ObjectionKind::Comply),
            "disqualify" => Some(ObjectionKind::Disqualify),
            _ => None,
        }
    }

    /// Invariant 4 (`AGENTS.md`): a `comply` objection is never refined by a
    /// model. This is the single predicate the whole guarantee rests on.
    pub fn allows_refinement(&self) -> bool {
        !matches!(self, ObjectionKind::Comply)
    }
}

// ─── Objection ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Objection {
    pub id: String,
    pub label: String,
    pub category: Option<String>,
    pub kind: ObjectionKind,
    pub response: String,
    pub talk_track: Vec<String>,
    pub guardrails: Vec<String>,
    pub rail_slot: Option<u8>,
    /// Authored default, as written in the playbook.
    pub threshold_authored: f32,
    /// Learned from feedback; `None` until enough events exist.
    /// Kept separate so the console can show both and offer a reset.
    pub threshold_learned: Option<f32>,
    pub allow_refine: bool,
    pub enabled: bool,
    pub sort_order: i64,
    pub triggers: Vec<String>,
    pub negative_triggers: Vec<String>,
    pub doc_refs: Vec<String>,
}

impl Objection {
    /// The threshold actually used by the gate.
    pub fn effective_threshold(&self) -> f32 {
        self.threshold_learned.unwrap_or(self.threshold_authored)
    }
}

/// A `rebut`/`disqualify` objection, proven at the type level.
///
/// The refinement path takes this, never a bare [`Objection`]. A `comply`
/// objection cannot be constructed into one, so it cannot reach an LLM
/// without changing this signature. See `docs/06-COMPLIANCE.md` §3.
#[derive(Clone, Debug)]
pub struct RebuttableObjection(Objection);

impl RebuttableObjection {
    pub fn new(o: Objection) -> Option<Self> {
        match o.kind {
            ObjectionKind::Comply => None,
            _ => Some(Self(o)),
        }
    }

    pub fn get(&self) -> &Objection {
        &self.0
    }
}

// ─── Utterance ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Utterance {
    pub speaker: Speaker,
    pub text: String,
    /// Relative to call start; negative means it came from the pre-roll buffer.
    pub t_start_ms: i64,
    pub t_end_ms: i64,
    pub from_preroll: bool,
}

impl Utterance {
    /// Convenience for tests and the `detect --text` path.
    pub fn prospect(text: impl Into<String>) -> Self {
        Utterance {
            speaker: Speaker::Prospect,
            text: text.into(),
            t_start_ms: 0,
            t_end_ms: 0,
            from_preroll: false,
        }
    }
}

// ─── Card ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardOrigin {
    Detected,
    ManualHotkey,
    ManualClick,
}

impl CardOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            CardOrigin::Detected => "detected",
            CardOrigin::ManualHotkey => "manual_hotkey",
            CardOrigin::ManualClick => "manual_click",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Citation {
    pub chunk_ref: String,
    pub document_title: String,
}

/// What the UI renders. One type for all three layers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub objection_id: String,
    pub kind: ObjectionKind,
    pub origin: CardOrigin,
    /// 0 = rail, 1 = promoted, 2 = refined.
    pub layer: u8,
    pub label: String,
    /// Layer 2 only.
    pub headline: Option<String>,
    pub body: String,
    pub talk_track: Vec<String>,
    pub citations: Vec<Citation>,
    pub similarity: Option<f32>,
    pub created_at_ms: i64,
}

// ─── Call lifecycle ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallState {
    Idle,
    Ringing,
    Active,
    Ending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallOutcome {
    Connected,
    Voicemail,
    NoAnswer,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(kind: ObjectionKind) -> Objection {
        Objection {
            id: "obj_test".into(),
            label: "Test".into(),
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
            triggers: vec![],
            negative_triggers: vec![],
            doc_refs: vec![],
        }
    }

    #[test]
    fn comply_cannot_become_rebuttable() {
        assert!(RebuttableObjection::new(obj(ObjectionKind::Comply)).is_none());
        assert!(RebuttableObjection::new(obj(ObjectionKind::Rebut)).is_some());
        assert!(RebuttableObjection::new(obj(ObjectionKind::Disqualify)).is_some());
    }

    #[test]
    fn learned_threshold_overrides_authored() {
        let mut o = obj(ObjectionKind::Rebut);
        assert_eq!(o.effective_threshold(), 0.62);
        o.threshold_learned = Some(0.71);
        assert_eq!(o.effective_threshold(), 0.71);
    }
}
