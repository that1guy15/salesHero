# 04 — Contracts

Interfaces between crates, and between the Rust core and the UI. Implement these signatures as written; changing one is a cross-crate decision.

---

## 1. Core types (`crates/core`)

```rust
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speaker { Agent, Prospect }

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectionKind {
    /// Full pipeline: promote, retrieve, refine.
    Rebut,
    /// Fixed authored card. Never sent to a model. See docs/06.
    Comply,
    /// Ends the call cleanly; no rebuttal, but not a legal obligation.
    Disqualify,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallState { Idle, Ringing, Active, Ending }

#[derive(Clone, Serialize, Deserialize)]
pub struct Utterance {
    pub speaker: Speaker,
    pub text: String,
    pub t_start_ms: i64,      // relative to call start; negative = pre-roll
    pub t_end_ms: i64,
    pub from_preroll: bool,
}

/// What the UI renders. One type for all three layers.
#[derive(Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub objection_id: String,
    pub kind: ObjectionKind,
    pub origin: CardOrigin,
    pub layer: u8,                    // 0 rail, 1 promoted, 2 refined
    pub label: String,
    pub headline: Option<String>,     // layer 2 only
    pub body: String,
    pub talk_track: Vec<String>,
    pub citations: Vec<Citation>,
    pub similarity: Option<f32>,
    pub created_at_ms: i64,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum CardOrigin { Detected, ManualHotkey, ManualClick }
```

---

## 2. Capture (`crates/audio`)

See `docs/02-ARCHITECTURE.md` §3 for `CaptureBackend`, `CaptureSource`, `CaptureError`, `FrameSink`.

```rust
pub struct CaptureHandle {
    pub source: CaptureSource,
    /// Suspends delivery without tearing down the stream. Mute hotkey.
    pub fn set_muted(&self, muted: bool);
    pub fn stop(self);
}

/// Far-end ring buffer with retained history.
pub struct PrerollBuffer {
    pub fn new(capacity_ms: u32, sample_rate: u32) -> Self;
    pub fn push(&self, frames: &[f32]);              // realtime-safe
    /// Copy the last `ms` of audio out. Called on CallStarted.
    pub fn drain_back(&self, ms: u32) -> Vec<f32>;
}
```

`PrerollBuffer::push` is called from the realtime callback: no allocation, no locks, no I/O. `drain_back` is called from a worker thread and may allocate.

---

## 3. ASR (`crates/asr`)

```rust
pub trait AsrEngine: Send {
    fn transcribe(&mut self, pcm: &[f32], opts: &TranscribeOptions)
        -> Result<Transcript, AsrError>;
    fn capabilities(&self) -> AsrCapabilities;
    fn warm_up(&mut self) -> Result<(), AsrError>;
}

pub struct TranscribeOptions {
    pub language: Option<String>,
    pub is_partial: bool,
}

pub struct Transcript {
    pub text: String,
    pub confidence: Option<f32>,
    pub duration_ms: u32,
}

pub struct AsrCapabilities {
    pub streaming: bool,
    pub languages: Vec<String>,
    pub requires_network: bool,      // gates availability in local-only mode
}
```

Call `warm_up()` at app start, not on first utterance. Cold-start on the first call of the day would blow the entire latency budget.

---

## 4. Detection (`crates/detect`)

```rust
pub trait CallBoundaryDetector: Send {
    fn on_vad(&mut self, speaker: Speaker, event: VadEvent) -> Option<CallEvent>;
    fn state(&self) -> CallState;
}

pub enum CallEvent {
    Started { call_id: String, preroll_ms: u32 },
    Ended   { call_id: String, duration_ms: u32, outcome: CallOutcome },
}

pub struct DetectionGate {
    pub fn evaluate(&self, window: &[Utterance]) -> Option<GateHit>;
}

pub struct GateHit {
    pub objection_id: String,
    pub kind: ObjectionKind,
    pub similarity: f32,
    pub window_text: String,
}
```

`DetectionGate::evaluate` is pure and synchronous. It takes the far-end window and returns at most one hit. Scoring:

```
score(objection) = max cosine over positive triggers
                 − 0.5 × (max cosine over negative triggers)
hit if score ≥ objection.threshold
```

Subtracting negatives rather than hard-excluding lets a strong positive still win, which matters because negative triggers are learned from user error and are noisier than authored ones.

---

## 5. LLM (`crates/llm`)

```rust
#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse, LlmError>;
    fn supports_prompt_cache(&self) -> bool;
    fn id(&self) -> ProviderId;
    fn requires_network(&self) -> bool;
}

pub struct ChatRequest {
    pub system: String,
    pub system_is_cacheable: bool,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub timeout: Duration,           // caller sets 2500 ms; provider must honor
    pub response_format: ResponseFormat,   // JsonSchema when supported
}

pub enum LlmError {
    Timeout,                         // → keep layer 1 card, log latency_exceeded
    RateLimited { retry_after: Option<Duration> },
    Auth,                            // → surface in UI, do not retry
    Network,
    BadResponse(String),             // schema violation
    Blocked,                         // local-only mode refused an outbound call
}
```

**Retry policy:** none inside a call. A retry lands after the moment has passed and costs money for nothing. Log and move on.

**`LlmError::Blocked`** is returned by a guard wrapper when local-only mode is on and a provider reports `requires_network()`. This is the enforcement point for invariant 7 and where `local_only_mode_makes_no_network_calls` asserts.

### Refinement prompt

System block — static for the whole session, marked cacheable:

```
You help an insurance agent handle objections during live outbound calls.

The agent has already been shown their own authored response. Your job is to
tailor it to what the prospect actually said, using the supporting excerpts
provided. You are not writing a script.

Rules:
- Output ONLY valid JSON matching the schema. No prose, no code fences.
- `response`: at most 3 short bullets. Never a paragraph.
- `next_question`: exactly one question the agent should ask next.
- Never contradict the guardrails.
- Never invent facts. If the excerpts do not support a claim, omit it.
- If this is not actually an objection, set is_objection false and stop.

Objections in this playbook: {labels}
Global guardrails: {guardrails}

Schema:
{"is_objection":bool,"objection_id":str,"confidence":float,
 "headline":str,"response":str,"next_question":str,"citations":[str]}
```

User message — only what varies:

```
TRANSCRIPT (last 3 prospect turns, agent turns interleaved):
{window}

MATCHED: {objection_id} — {label}
AGENT'S AUTHORED RESPONSE:
{response}
GUARDRAILS: {objection_guardrails}

SUPPORTING EXCERPTS:
{chunks}

HIGHLY-RATED PAST RESPONSES FOR THIS OBJECTION:
{top_3_exemplars}
```

Version this string. Store `prompt_version` on every card so quality changes are attributable.

---

## 6. Tauri IPC

### Commands (UI → core)

```typescript
// Session
invoke('start_session', { captureMode, micDeviceId, processId? }): Promise<SessionId>
invoke('stop_session'): Promise<void>
invoke('set_muted', { muted: boolean }): Promise<void>

// Rail & cards
invoke('get_rail'): Promise<RailItem[]>                 // slots 1-6, from active playbook
invoke('get_all_objections'): Promise<Objection[]>       // below-fold list
invoke('trigger_card', { objectionId, origin }): Promise<Card>   // manual hotkey/click
invoke('dismiss_card', { cardId }): Promise<void>

// Feedback
invoke('submit_feedback', { cardId, verdict, reason?, note? }): Promise<void>

// Playbook
invoke('list_playbooks'): Promise<PlaybookSummary[]>
invoke('set_active_playbook', { playbookId }): Promise<void>
invoke('save_objection', { objection: Objection }): Promise<void>
invoke('delete_objection', { objectionId }): Promise<void>
invoke('set_rail_slot', { objectionId, slot: number | null }): Promise<void>
invoke('import_document', { path }): Promise<DocumentSummary>
invoke('export_playbook', { playbookId, destPath }): Promise<void>
invoke('import_playbook', { path, onCollision }): Promise<ImportReport>

// Settings
invoke('list_capture_sources'): Promise<CaptureSource[]>
invoke('list_providers'): Promise<ProviderInfo[]>
invoke('list_openrouter_models'): Promise<ModelInfo[]>   // fetched live
invoke('set_provider', { providerId, model, apiKey? }): Promise<void>
invoke('set_local_only', { enabled: boolean }): Promise<void>
invoke('get_model_latency_stats'): Promise<LatencyStats[]>
```

### Events (core → UI)

```typescript
'call:state'      { state: CallState, callId?: string }
'call:started'    { callId: string }
'call:ended'      { callId: string, durationMs: number, outcome: string }

'transcript:partial'  { speaker: Speaker, text: string }
'transcript:final'    { utterance: Utterance }

'card:promoted'   { card: Card }        // layer 1
'card:refined'    { card: Card }        // layer 2 — replaces by card.id
'card:demoted'    { cardId: string }    // model returned is_objection: false

'capture:error'   { error: CaptureError, recoverable: boolean }
'llm:error'       { error: LlmError, provider: string }
'engine:status'   { asr: EngineStatus, embed: EngineStatus, llm: EngineStatus }
```

`engine:status` drives the degradation indicator in the overlay. The UI must render correctly when all three report `Unavailable` — that is Layer 0, and it is the normal state during startup and in local-only mode without a local LLM.

---

## 7. Replay CLI

```
riposte-replay --audio <path> --playbook <path> [OPTIONS]

  --audio <PATH>          Stereo WAV: L=agent, R=prospect
                          Or pass --audio-agent and --audio-prospect separately
  --playbook <PATH>       .playbook or playbook.yaml
  --format <FMT>          jsonl | pretty | csv          [default: jsonl]
  --no-llm                Layer 1 only. Deterministic, CI-safe.
  --provider <ID>         Override configured provider
  --expect <PATH>         Expected detections; exit 1 on mismatch
  --realtime              Play at wall-clock speed rather than as fast as possible
```

JSONL output — one object per event, same shape as the Tauri events above plus timing:

```json
{"t_ms":8420,"event":"card:promoted","objection_id":"obj_no_form","similarity":0.71,"latency_gate_ms":780}
```

`--no-llm` must be deterministic given the same audio and playbook. CI depends on it.
