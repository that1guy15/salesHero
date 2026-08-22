## Summary

What changed, and why. Lead with the reason — a reviewer who knows why can
check whether the change achieves it.

## Milestone

Milestone: <!-- M1.1 / M1.5 / M1.7 / M2 … or "none — maintenance" -->
Closes #

## Runnable output

`docs/07-BUILD-PLAN.md`: *"Do not merge a milestone whose runnable output
criterion is unmet."* Paste the command and its **actual** output.

```text

```

## Acceptance criteria

Every box in the milestone's checklist is `met`, `unmet`, or `restated in
docs/DECISIONS.md` with a reason. No item may be silently skipped.

<!-- - [x] … -->

## Invariants

`AGENTS.md` lists seven. Confirm the ones this diff could touch.

- [ ] Realtime audio callback still allocates nothing, locks nothing, does no I/O
- [ ] Mic and far-end streams are still never mixed
- [ ] The rail still renders with every engine unavailable
- [ ] `comply` objections still cannot reach an LLM — **and still win the gate**
- [ ] LLM calls are still hard-capped at 2500 ms
- [ ] No audio is written to disk without explicit opt-in
- [ ] Local-only mode still makes zero outbound connections
- [ ] `#[cfg(windows)]` appears only in `crates/audio` and `apps/desktop/src/platform.rs`

## Compliance

Required when the diff touches `crates/detect`, `crates/store`, `crates/llm`,
the playbook format, or card rendering. Run the `compliance-review` skill.

- [ ] Not applicable
- [ ] Reviewed — no weakening of the five `comply` enforcement layers
- [ ] `riposte-evals check` passes: every removal request in the probe set is caught

## Deviations

Anything that departs from the nine planning documents is recorded in
`docs/DECISIONS.md` with what was specified, what was done, and why.

- [ ] No deviation
- [ ] Recorded as D__

## Evidence

- [ ] `make gate` passes locally
- [ ] Tests added for new behaviour, and they fail without the change
- [ ] `docs/PROJECT-STATUS.md` updated if what-is-built changed
- [ ] Platform-specific behaviour verified on Windows, or explicitly listed as unverified

## Not included

What a reader might reasonably expect here and will not find, and why.
