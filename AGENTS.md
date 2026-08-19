# Instructions for implementation agents

This file is read by Codex and Claude Code. Symlink `CLAUDE.md` → `AGENTS.md`.

---

## Read this first

Before writing any code, read `docs/01-PRD.md` and `docs/02-ARCHITECTURE.md` in full. Then read the specific document for the milestone you are on, per `docs/07-BUILD-PLAN.md`.

Do not begin at M4 (the GUI). The build order exists because the risky parts are the audio capture and the detection quality, and both are testable headlessly. A working overlay on top of detection that has never been measured is worse than no overlay.

---

## Invariants

These are not preferences. Breaking one is a bug even if tests pass.

1. **The realtime audio callback allocates nothing, locks nothing, and performs no I/O.** It converts samples and pushes to a ring buffer. That is all it ever does.

2. **Mic and far-end audio are never mixed.** They are separate streams end to end. Speaker attribution comes from which stream the audio arrived on, never from a model.

3. **The Quick Response Rail renders without any model.** If ASR, embeddings, and the LLM are all unavailable, the rail must still display and its hotkeys must still work. Test this explicitly — there is a required test named `rail_renders_with_all_engines_disabled`.

4. **Objections with `kind: comply` are never sent to an LLM for rebuttal generation.** See `docs/06-COMPLIANCE.md` §3. This is a legal constraint, not a product choice. There is a required test named `comply_objections_never_reach_llm`.

5. **LLM calls are hard-capped at 2500ms.** On timeout, keep whatever card is already displayed and log `latency_exceeded`. Never block the UI on a model.

6. **No audio is written to disk** unless the user has explicitly enabled session recording in settings. Default off. Ring buffers only.

7. **Local-only mode makes zero outbound connections.** Enforce at the provider layer, not by convention. There is a required test named `local_only_mode_makes_no_network_calls`.

---

## Working style

**Platform code stays in one place.** `crates/audio` is the only crate permitted to contain `#[cfg(windows)]`. Everything else compiles and tests on Linux so CI is cheap. If you find yourself wanting a platform cfg elsewhere, the abstraction is wrong — fix the abstraction.

**Every milestone ends with something runnable.** Not "the module compiles." A command someone can type that produces observable output. The build plan states what that command is for each milestone.

**Prefer the boring dependency.** This repo has a large surface area for a solo maintainer. `transcribe-rs` already multiplexes five ASR engines — do not write a sixth wrapper. `fastembed` already handles ONNX embedding — do not hand-roll it.

**When a spec is ambiguous, write down the interpretation you chose** in a `## Decisions` section at the bottom of the relevant doc, then proceed. Do not stall. Do not guess silently.

**Do not add features not in `docs/01-PRD.md` §3.** Scope creep is the main failure mode for this kind of project. If something seems obviously missing, add it to `docs/01-PRD.md` §9 (Deferred) rather than building it.

---

## Testing requirements

| Layer | Requirement |
|---|---|
| `crates/detect` | Unit tests over synthetic transcripts; precision/recall reported by `evals/` |
| `crates/playbook` | Round-trip test: import → export → import produces identical bytes |
| `crates/llm` | Mock transport; assert timeout, retry, and provider-shape behavior |
| `crates/store` | Migration tests forward from every prior `schema_version` |
| `crates/audio` | Manual verification checklist in `docs/07-BUILD-PLAN.md` M1 — cannot be CI-tested |
| End-to-end | `riposte-replay` over `evals/fixtures/`, asserting expected detections |

The four named invariant tests above are required and must be present by the milestone that introduces the relevant code.

---

## Commit conventions

Conventional commits, scoped to crate: `feat(detect): promote rail item on gate hit`.

One milestone per branch. Do not merge a milestone whose "runnable output" criterion is unmet.

---

## Things that will bite you

Collected so you do not rediscover them:

- `cpal` cannot do WASAPI loopback reliably and cannot do per-process capture at all. Use the `wasapi` crate. This is settled; do not revisit.
- WASAPI loopback works only in **shared mode**. Exclusive-mode streams are uncapturable.
- In per-process loopback mode, `get_device_period` returns nonsense and the period you pass to `initialize_client` is ignored.
- Chrome renders audio in a child process. Per-process capture of a browser dialer needs `include_tree = true`, which then captures every tab. This is why system-mix is the default.
- Tauri's `setAlwaysOnTop` does not survive `hide()` / `show()`. Re-assert it, and re-apply `SetWindowDisplayAffinity`, on every show.
- Whisper-family models hallucinate confidently on silence. VAD-gate before ASR so silence never reaches the model. Prefer Parakeet or Moonshine anyway.
- Ricochet360 connects the next lead about one second after the previous call ends. There is no idle gap to detect a call boundary in. See `docs/02-ARCHITECTURE.md` §5.
