# Riposte

Real-time objection coaching for outbound sales agents.

Riposte listens to a live call, keeps the agent's most common objections one keystroke away, and — when it recognizes what the prospect just said — promotes the right response and tailors it to the actual words used.

Built for insurance agents running bought-lead dialers, where calls arrive roughly one second after the last one ended and the first fifteen seconds decide everything.

> **Working name.** "Riposte" appears in the workspace name, crate prefixes, and bundle identifier. Renaming is a find-and-replace across `Cargo.toml`, `tauri.conf.json`, and this file. Decide before M4.

---

## The core idea

Most "AI sales copilot" products put a language model on the critical path, which means the agent stares at a spinner during the exact three seconds that matter. Riposte inverts that.

**The Quick Response Rail is always on screen.** The agent's top 5–6 objections sit in a fixed panel, bound to hotkeys, visible from the moment the app opens. No model, no transcription, no latency. It is useful with the network off and the AI disabled.

**Detection promotes, it does not gate.** When the pipeline recognizes an objection, the matching rail item illuminates and expands. When the model finishes ~1.5s later, the body is replaced with a version tailored to what the prospect actually said. If any of that fails, the rail is still sitting there, still correct, still one keystroke away.

Everything downstream of the rail is an enhancement. That is the whole architectural bet.

---

## What v1 does

- Captures the agent's mic and the far-end audio as two separate streams — perfect speaker attribution with no diarization
- Detects call start and end from audio alone, with no dialer integration
- Transcribes locally (Moonshine / Parakeet) or via a remote ASR
- Matches prospect speech against a user-authored objection library
- Surfaces responses instantly, refines them with an LLM when one is configured
- Distinguishes objections you **rebut** from ones you must **comply** with — "take me off your list" is never met with a rebuttal
- Collects structured thumbs up/down and learns thresholds from it
- Runs fully local, or against OpenRouter, Anthropic, OpenAI, Ollama, or LM Studio
- Ships a default insurance cold-call playbook so it works before anyone authors anything

## What v1 does not do

- CRM or dialer integration of any kind
- Post-call summaries, deal scoring, forecasting
- macOS (abstracted for, not implemented — see `docs/02-ARCHITECTURE.md` §3)
- Any hosted or multi-tenant component
- Audio persistence (transcripts only, opt-in)

---

## Documents

Read in order. Each assumes the prior ones.

| # | Document | What it settles |
|---|---|---|
| 1 | [`docs/01-PRD.md`](docs/01-PRD.md) | Who this is for, what ships, acceptance criteria |
| 2 | [`docs/02-ARCHITECTURE.md`](docs/02-ARCHITECTURE.md) | Components, platform abstraction, concurrency, latency |
| 3 | [`docs/03-DATA-MODEL.md`](docs/03-DATA-MODEL.md) | SQLite schema, playbook format, import/export |
| 4 | [`docs/04-CONTRACTS.md`](docs/04-CONTRACTS.md) | Rust traits, Tauri IPC commands and events |
| 5 | [`docs/05-UX-SPEC.md`](docs/05-UX-SPEC.md) | Overlay layout, states, hotkeys, visual system |
| 6 | [`docs/06-COMPLIANCE.md`](docs/06-COMPLIANCE.md) | Consent, DNC handling, redaction — read before M2 |
| 7 | [`docs/07-BUILD-PLAN.md`](docs/07-BUILD-PLAN.md) | Milestones as verifiable tasks |
| — | [`docs/WINDOWS-TESTING.md`](docs/WINDOWS-TESTING.md) | Setting up and testing the overlay on Windows |
| — | [`docs/DEV-SETUP.md`](docs/DEV-SETUP.md) | What a development machine needs, and why |
| — | [`docs/PROJECT-STATUS.md`](docs/PROJECT-STATUS.md) | **Start here.** What is built, what is not, open concerns |
| — | [`docs/DECISIONS.md`](docs/DECISIONS.md) | Where the implementation deviates from the specs, and why |

Implementation agents: start at [`AGENTS.md`](AGENTS.md).

---

## Stack

Rust workspace, Tauri v2 shell, everything on-device by default.

| Concern | Choice |
|---|---|
| Audio capture | `wasapi` (Windows), behind a `CaptureBackend` trait |
| VAD | Silero VAD via `ort` |
| ASR | `transcribe-rs` → Moonshine v2 / Parakeet TDT |
| Embeddings | `fastembed` → BGE-small-en-v1.5, 384-dim |
| Storage | `rusqlite` + `sqlite-vec`, statically linked |
| LLM | `ChatProvider` trait — OpenAI-compatible + Anthropic native |
| Secrets | `keyring` → Windows Credential Manager |
| UI | Tauri v2 + Svelte |

## Status

**Early. M0 and M1 of `docs/07-BUILD-PLAN.md` are built and runnable; nothing after that is started.** No audio capture, no LLM, no UI. Read [`docs/PROJECT-STATUS.md`](docs/PROJECT-STATUS.md) before picking this up — it carries what works, what is deviated from spec, and two findings about gate calibration that should drive the next milestone.

## Quick start

Nothing below needs audio hardware, a network connection, an API key, or a model download.

```bash
cargo build --workspace
cargo test --workspace
```

```bash
# Load the default insurance playbook
cargo run -p riposte-cli -- playbook import playbooks/insurance-cold-call.yaml

# Layer 0 — the Quick Response Rail. No model, no network, 0 ms.
cargo run -p riposte-cli -- rail

# Score a line of prospect speech against the playbook
cargo run -p riposte-cli -- detect --text "yeah I never filled out any form" --all

# Press a rail hotkey; recorded as manual use, the PRD's headline metric
cargo run -p riposte-cli -- hotkey 3
cargo run -p riposte-cli -- stats
```

The default embedder is lexical and deterministic, so this runs anywhere and CI is hermetic. The real semantic gate — BGE-small-en-v1.5 — is behind a feature flag and downloads weights on first use:

```bash
cargo run -p riposte-cli --features onnx -- --onnx detect --text "we're all set thanks" --all
```

See `docs/PROJECT-STATUS.md` §3 before trusting either one's output. The shipped thresholds are not yet calibrated against measured data.

## What this is not

Stated here because users will otherwise assume otherwise, and one of these is the reason the project is designed the way it is.

- **Not a TCPA compliance system.** It helps an agent respond correctly to a removal request. It does not scrub lists, honor DNC registries, or prevent a single non-compliant dial.
- **Not a call recorder.** Recording is off by default and is not the product.
- **Not a supervisor monitoring tool.** No manager view, no remote access, no telemetry leaving the machine.

A "take me off your list" is never met with a rebuttal. That is enforced at the type level, by a SQL trigger, on playbook import, and by a required test — not by asking a model nicely. See [`docs/06-COMPLIANCE.md`](docs/06-COMPLIANCE.md) §3.

## License

MIT. See [`docs/06-COMPLIANCE.md`](docs/06-COMPLIANCE.md) before deploying to live calls — recording law is not a formality.
