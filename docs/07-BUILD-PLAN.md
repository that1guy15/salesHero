# 07 — Build Plan

Nine milestones. Each ends with a command someone can run and observe. Do not merge a milestone whose runnable output criterion is unmet.

M0–M4 have no GUI. This is deliberate: the risky parts are audio capture and detection quality, both testable headlessly, and projects of this shape usually die fighting window management before anyone knows whether the detection works.

---

## M0 — Workspace and storage

**Goal:** the skeleton exists and the database round-trips.

- Cargo workspace with all seven crates plus two apps
- `crates/store`: rusqlite + sqlite-vec statically linked, full schema from `03-DATA-MODEL.md`, migration runner
- `crates/playbook`: YAML parse and serialize, the `.playbook` zip format, import with collision handling
- Default insurance playbook loads on first run
- CI: build + test on Linux and Windows

**Runnable:** `cargo run -p riposte-cli -- playbook import playbooks/insurance-cold-call.yaml && cargo run -p riposte-cli -- playbook list` prints six rail objections with their kinds.

**Acceptance**
- [ ] Round-trip test: import → export → import produces byte-identical output
- [ ] `kind: comply` forces `allow_refine = 0`, verified through both the SQL trigger and a hand-edited YAML with `allow_refine: true`
- [ ] Migration test passes from a v1 fixture
- [ ] Rail slot uniqueness enforced; collision on import warns and bumps

---

## M1 — Embeddings and the gate

**Goal:** detection works on text, with no audio involved at all.

- `crates/detect`: `fastembed` with BGE-small-en-v1.5, trigger and negative-trigger embedding at playbook save time
- Brute-force cosine scoring per `04-CONTRACTS.md` §4
- Rolling window construction (3 turns / 30 s)
- Dedupe by objection id within 45 s per call

**Runnable:** `cargo run -p riposte-cli -- detect --text "yeah I never filled out any form"` prints the matched objection, similarity, and kind.

**Acceptance**
- [ ] All six default objections match at least 3 of their own trigger phrases at ≥ 0.62
- [ ] No default objection matches another's trigger phrases above threshold
- [ ] Negative triggers measurably reduce score
- [ ] Gate evaluation is < 10 ms for a 100-objection playbook — benchmark it
- [ ] Multi-turn window catches an objection split across two utterances

---

## M2 — Replay harness

**Goal:** the full non-audio pipeline runs over recorded calls in CI.

- `apps/replay` per `04-CONTRACTS.md` §7
- Stereo WAV split into agent/prospect channels
- Wire VAD → segmentation → ASR → gate, all offline
- JSONL output with per-stage timings
- `--expect` mode for regression testing
- `evals/fixtures/` with 8–12 labeled synthetic calls covering all six defaults, plus two negatives (voicemail greeting, wrong number with no objection)

**Runnable:** `cargo run -p riposte-replay -- --audio evals/fixtures/call-01.wav --playbook playbooks/insurance-cold-call.yaml --no-llm --format pretty`

**Acceptance**
- [ ] `--no-llm` output is deterministic across runs
- [ ] `evals/run.rs` reports precision, recall, and p50/p95 latency
- [ ] Voicemail fixture produces zero detections
- [ ] CI runs the eval suite and fails on precision regression > 5 points
- [ ] **Required test:** `comply_objections_never_reach_llm`

> Fixture audio: use TTS-generated synthetic calls in the public repo. Real call audio needs consent from both parties before it can be committed. Keep real fixtures local and gitignored.

---

## M3 — LLM providers

**Goal:** Layer 2 refinement works, and cannot break Layer 1.

- `crates/llm`: `ChatProvider` trait, `OpenAiCompatProvider`, `AnthropicProvider`
- OpenRouter model catalog fetch
- Prompt construction per `04-CONTRACTS.md` §5, versioned
- Hard 2500 ms timeout, no retry
- `keyring` integration
- Local-only guard wrapper
- Redaction per `06-COMPLIANCE.md` §4
- Document ingestion: PDF/DOCX/MD/TXT → chunk → embed → `vec_chunks`
- Retrieval: top-4 KNN filtered by `doc_refs`

**Runnable:** the same replay command without `--no-llm` produces refined card bodies with citations.

**Acceptance**
- [ ] Timeout at 2500 ms preserves the Layer 1 card and logs `latency_exceeded`
- [ ] Malformed JSON from the model degrades to Layer 1 rather than erroring
- [ ] `is_objection: false` emits `card:demoted`
- [ ] Ollama and OpenRouter both work through the same code path
- [ ] **Required test:** `local_only_mode_makes_no_network_calls`
- [ ] **Required test:** `redaction_applied_before_egress`
- [ ] Anthropic requests carry a cache breakpoint on the system block

---

## M4 — Windows audio capture

**Goal:** live dual-stream capture into the existing pipeline.

- `CaptureBackend` trait and `CaptureSource` enum per `02-ARCHITECTURE.md` §3
- `WasapiBackend`: mic capture, system-mix loopback, per-process loopback
- `CoreAudioTapBackend` stub returning `UnsupportedOnPlatform`
- SPSC ring buffers, realtime-safe `FrameSink`
- 10-second `PrerollBuffer` on the far-end stream
- `rubato` resampling to 16 kHz mono on the consumer side
- Mute without teardown

**Runnable:** `cargo run -p riposte-cli -- listen --source system-mix` prints a live speaker-tagged transcript to the terminal.

**Acceptance** — largely manual; this cannot be CI-tested
- [ ] Mic and system audio arrive on separate streams with correct attribution
- [ ] Runs 60 minutes with no dropouts and no memory growth
- [ ] Per-process capture on Chrome with `include_tree = true` captures the dialer
- [ ] Realtime callback verified allocation-free — check with a tracking allocator under test
- [ ] Pre-roll returns 10 s of prior audio on demand
- [ ] Mute stops delivery without tearing down the stream
- [ ] Device change mid-session is handled without a crash

---

## M5 — Call boundary detection

**Goal:** individual calls are correctly separated inside a continuous session.

- State machine per `02-ARCHITECTURE.md` §5
- Pre-roll drain on `CallStarted`
- Detection suppressed in IDLE and RINGING
- Dedupe state resets per call
- Thresholds exposed in config

**Runnable:** `cargo run -p riposte-replay -- --audio evals/fixtures/session-multi-call.wav` correctly segments a 20-minute recording into its constituent calls.

**Acceptance**
- [ ] Voicemail greetings do not reach ACTIVE
- [ ] Calls separated by ~1 s are detected as two calls, not one
- [ ] Pre-roll drain places pre-detection utterances in the transcript with negative timestamps
- [ ] `outcome` classified as connected / voicemail / no_answer
- [ ] Fixture with 12 known calls segments within ±1

> This is the milestone most likely to need field tuning. Ship the thresholds as config and expect to adjust them after the first week of real use.

---

## M6 — Overlay

**Goal:** the rail exists on screen and works.

- Tauri v2 + Svelte
- Overlay window: borderless, always-on-top, no taskbar, draggable, position persisted
- Rail rendering, hotkeys 1–6, click, `Esc`, `Ctrl+Shift+M`
- Layer transitions per `05-UX-SPEC.md` §3
- Comply cards take the full panel
- Status bar and engine status footer
- `SetWindowDisplayAffinity` with a settings toggle
- Re-apply always-on-top and display affinity on every `show()`

**Runnable:** `cargo tauri dev` — the overlay appears, hotkeys expand cards, live calls promote them.

**Acceptance**
- [ ] **Required test:** `rail_renders_with_all_engines_disabled`
- [ ] Overlay absent from Zoom share, OBS capture, and Snipping Tool
- [ ] Always-on-top survives hide/show
- [ ] Hotkeys work when the overlay does not have focus
- [ ] Comply card hides the rail and offers only the log-and-end action
- [ ] `prefers-reduced-motion` disables all transitions
- [ ] Rail slots never reorder during a call

---

## M7 — Console

**Goal:** users can bring their own content.

- Playbook tab: edit objections, drag rail slots, kind selector, threshold slider with learned vs authored
- Documents tab: import, list, link to objections
- Calls tab: session/call tree, transcript with inline cards
- Settings tab: capture, ASR, provider, OpenRouter catalog with pricing, local-only toggle, screen-share toggle, TTL
- Export and import `.playbook`
- First-run flow per `05-UX-SPEC.md` §7

**Runnable:** author an objection in the UI, export, wipe the database, re-import, and it works on a live call.

**Acceptance**
- [ ] Comply authoring shows the warning from `06-COMPLIANCE.md` §3
- [ ] OpenRouter picker is searchable and shows price and context length
- [ ] First-run consent screen cannot be skipped
- [ ] Audio setup step requires both level meters to register signal
- [ ] Transcript TTL purge nulls `cards.window_text`

---

## M8 — Feedback and learning

**Goal:** the app improves with use, visibly.

- 👍/👎 on cards, four-way reason picker on 👎
- Reason-code side effects per `03-DATA-MODEL.md` §4
- Nightly threshold recomputation
- Learned triggers from near-threshold 👍, capped at 20
- Top-3 exemplar injection into refinement prompts
- Insights tab

**Runnable:** after 20 feedback events, the console shows adjusted thresholds and the diff against authored defaults.

**Acceptance**
- [ ] Each reason code produces its documented side effect
- [ ] Thresholds clamp to [0.50, 0.85]
- [ ] Reset restores the authored default
- [ ] Rail-usage vs detected-usage split is visible — the PRD's primary metric
- [ ] Comply cards accept no feedback

---

## M9 — Test-user release

**Goal:** ten agents run it on live calls for two weeks.

- MSI installer, code signed
- Bundled Moonshine tiny — no first-run download
- Crash reporting, opt-in, transcript-free
- Telemetry: only the `01-PRD.md` §7 metrics, opt-in, aggregate only
- In-app feedback channel
- Onboarding doc for agents (not for developers)

**Acceptance**
- [ ] Clean install on a fresh Windows 11 machine, no dev tools
- [ ] Works with no API key configured
- [ ] Uninstall removes everything including `%APPDATA%`
- [ ] 4-hour dialing session with no crash, no memory growth, no audio dropout
- [ ] All seven `AGENTS.md` invariants hold in the shipped build

---

## Sequencing notes

**Parallelizable:** M1 and M4 are independent. M6 and M7 are independent once M5 lands.

**Critical path:** M0 → M1 → M2 → M5 → M6 → M9. M3 (LLM) is genuinely optional to a shippable v1 — if it slips, ship Layer 0 and Layer 1 only. That is the whole point of the layered design, and it should be treated as a real option rather than a fallback nobody would take.

**The riskiest milestone is M5**, not M4. Audio capture is well-documented and either works or doesn't. Call boundary detection is a heuristic tuned against a dialer's specific timing behavior, and it will need real Ricochet360 sessions to get right. Budget accordingly, and get a recorded multi-call session from the test agent before starting it.
