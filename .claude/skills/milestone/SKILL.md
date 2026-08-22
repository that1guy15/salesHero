---
name: milestone
description: Drive one milestone of docs/07-BUILD-PLAN.md from start to merge. Use when starting, continuing, or closing out any milestone (M0-M9, M1.1, M1.5, M1.7), or when asked whether a milestone is done.
---

# Working a milestone

`AGENTS.md` sets the rules; `07-BUILD-PLAN.md` sets the work. This is how they
combine.

## Before writing any code

Read, in this order and in full:

1. `docs/01-PRD.md` — especially §3 (the scope fence) and §7 (what success is)
2. `docs/02-ARCHITECTURE.md`
3. The document your milestone names
4. `docs/PROJECT-STATUS.md` — what is actually built, and what is deviated
5. `docs/DECISIONS.md` — twelve entries; several contradict the specs on purpose

`PROJECT-STATUS.md` outranks the specs on questions of *what exists*.
`DECISIONS.md` outranks the specs on questions of *what we chose*.

## The rules that bite

**Every milestone ends with something runnable.** Not "the module compiles." The
build plan names the exact command. If you cannot run it and paste real output,
the milestone is not done. M0 and M1 were both reported complete with six
acceptance criteria open — that is the failure mode this rule exists to stop.

**Do not add features outside `01-PRD.md` §3.** Scope creep is the named failure
mode for this project. If something seems obviously missing, add it to §9
(Deferred) rather than building it. Bring it up; do not just build it.

**Record ambiguity resolutions in `docs/DECISIONS.md`** — one log for the whole
project. `AGENTS.md` says to use a section in the relevant doc; that predates
parallel work and produces contradictory decisions in different files.
`DECISIONS.md` is the single log. Format: what was specified, what we did
instead, why. Do not stall on ambiguity, and do not guess silently.

**Prefer the boring dependency.** `transcribe-rs` already multiplexes five ASR
engines. `fastembed` already handles ONNX embedding. Do not write a sixth
wrapper. This repo is already large for a solo maintainer.

**Platform code stays in `crates/audio`.** It is the only crate permitted
`#[cfg(windows)]`.

## Working loop

```bash
cargo check --workspace          # inner loop — the dependency graph is heavy
cargo test --workspace           # or: cargo nextest run --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
```

`RUSTFLAGS: -D warnings` is set in CI, so a warning is a build failure there.
Run `cargo fmt --all` before every commit — a formatting diff fails CI's first
step and has done so on `main` before.

## Closing a milestone

Do all of these, in order:

1. Run the milestone's **Runnable** command. Paste the actual output.
2. Walk the **Acceptance** checklist item by item. Each is `met`, `unmet`, or
   `restated in DECISIONS.md` with a reason. No item may be silently skipped.
3. Invoke the `invariant-audit` skill.
4. Invoke `compliance-review` if the diff touches detect, llm, store, the
   playbook format, or card rendering.
5. Update `docs/PROJECT-STATUS.md`: what is built, what is not, what you found.
   Write it for whoever picks this up next, not as a changelog.
6. **Invoke the `ship` skill** — commit signed, open the pull request, drive
   `required-ci` green, merge, delete the branch.

**A milestone is not closed until it is merged.** A green branch sitting unmerged
is an unfinished milestone, however complete the code looks. One milestone per
branch.

## Where the milestones actually stand

The build plan's original order was revised. Current sequence:

| Milestone | State |
|---|---|
| M0 storage, M1 gate | built; remediation in M1.1 |
| **M1.1 remediation** | in progress — audit findings F1–F21 |
| **M1.5 calibration** | not started — **gates M2**, see the `gate-calibration` skill |
| **M1.7 rail overlay v0** | not started — Layer 0 only, ships to real agents |
| M2 replay → M9 release | not started |

M1.5 exists because the shipped thresholds are not calibrated and the default
embedder is a CI fixture, not a gate (`DECISIONS.md` D6). Do not build M2 on an
unmeasured gate.

M1.7 exists because the PRD's headline metric — rail use without AI — needs no
audio, no ASR and no LLM, and the original plan spent the entire audio budget
before any agent touched the product.

M3 (LLM) is genuinely optional to a shippable v1. If anything slips, slip that.
