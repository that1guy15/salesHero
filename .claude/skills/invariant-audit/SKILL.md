---
name: invariant-audit
description: Audit the repo against the seven AGENTS.md invariants and the four required named tests. Use before merging any milestone, after any change to crates/detect, crates/store, crates/llm or crates/audio, and whenever asked to check compliance, invariants, or whether a milestone is really done.
---

# Invariant audit

`AGENTS.md` states seven invariants and calls them *"not preferences — breaking
one is a bug even if tests pass."* Four have named tests that CI checks for by
name. This skill verifies both, and checks that the tests are not vacuous.

Report findings as a table: invariant, status (`ok` / `gap` / `n/a — milestone
not started`), and the evidence. Never report `ok` without evidence you actually
gathered in this run.

## 1. The seven invariants

Run from the repo root.

### I1 — realtime callback allocates nothing, locks nothing, does no I/O

`n/a` until `crates/audio` exists (M4). Once it does:

```bash
rg -n 'fn on_frames' crates/audio/src
```
Inside that function and anything it calls, flag: `Vec::`, `String::`, `format!`,
`Box::`, `.to_vec()`, `.clone()` on owned data, `Mutex`, `RwLock`, `lock()`,
`println!`, `log::`, `tracing::`, any `std::fs` or `std::io`.

### I2 — mic and far-end are never mixed

```bash
rg -n 'Speaker::(Agent|Prospect)' crates/ apps/
```
Attribution must come from which stream a frame arrived on. Flag any diarization,
speaker-embedding, or clustering. Detection runs on the prospect stream only —
`build_window` in `crates/detect/src/gate.rs` filters to `Speaker::Prospect`;
confirm that filter still exists.

### I3 — the rail renders with no model

Required test `rail_renders_with_all_engines_disabled`. Lands with the overlay.

```bash
cargo test --workspace -- --list 2>/dev/null | rg rail_renders_with_all_engines_disabled
```

### I4 — comply objections never reach an LLM

Required test `comply_objections_never_reach_llm`. Five enforcement layers must
all be present — the fifth is `docs/DECISIONS.md` D11 and is the only one that
protects *which* objection is selected:

```bash
rg -n 'RebuttableObjection' crates/core/src/lib.rs          # 1. type level
rg -n 'trg_comply_no_refine' crates/store/migrations/       # 2. SQL trigger
rg -n 'allows_refinement' crates/playbook/src/lib.rs        # 3. import override
rg -n 'fn select_hit|comply_containment' crates/detect/src/gate.rs  # 5. selection
cargo test --workspace -- --list 2>/dev/null | rg comply_objections_never_reach_llm  # 4. test
```

Then confirm `refine()` — once `crates/llm` exists — takes `RebuttableObjection`
and not `Objection`. If it takes `Objection`, layer 1 is decorative.

### I5 — LLM calls hard-capped at 2500 ms

`n/a` until M3. Then: the timeout is set by the caller and honoured by the
provider, there is no retry inside a call, and a timeout logs `latency_exceeded`
while keeping the Layer 1 card.

### I6 — no audio written to disk unless session recording is on

```bash
rg -n 'File::create|fs::write|WavWriter|hound' crates/ apps/
```
Every hit must be behind an explicit settings check. Default off.

### I7 — local-only mode makes zero outbound connections

Required test `local_only_mode_makes_no_network_calls`. `n/a` until M3.

**Known hole to check for** (`PROJECT-STATUS.md` §4.2): enforcement must be on
the **resolved base URL**, not on the provider id. An Ollama provider pointed at
a remote host defeats a `requires_network()` check on the provider type.

## 2. Required tests exist and are not vacuous

```bash
for t in comply_objections_never_reach_llm rail_renders_with_all_engines_disabled \
         local_only_mode_makes_no_network_calls redaction_applied_before_egress; do
  printf '%-46s ' "$t"
  cargo test --workspace -- --list 2>/dev/null | rg -q "$t" && echo present || echo MISSING
done
```

A test that asserts a mock was never called passes trivially if nothing ever ran.
`comply_objections_never_reach_llm` guards against this with
`assert!(fired > 0, ...)`. **Every invariant test of that shape needs the same
guard.** Read each present test and confirm it has one. A missing positive-path
assertion is a finding, not a nitpick.

## 3. Platform containment

```bash
rg -n '#\[cfg\(windows\)\]|#\[cfg\(target_os' crates/ apps/ | rg -v '^crates/audio/'
```
Any hit outside `crates/audio` is a finding. `AGENTS.md`: *"if you find yourself
wanting a platform cfg elsewhere, the abstraction is wrong — fix the abstraction."*

## 4. Milestone honesty

`07-BUILD-PLAN.md`: *"do not merge a milestone whose runnable output criterion is
unmet."* For the milestone under review, run its **Runnable** command and paste
the real output. Then walk its **Acceptance** checklist item by item and mark
each `met` / `unmet` / `restated in DECISIONS.md`.

M0 and M1 were both reported complete with six criteria open. Assume nothing.
