# Contributing

Riposte runs on a live phone call, in front of a prospect, for an agent who
cannot look away. Correctness, compliance and latency outrank feature speed
every time. This document is the workflow; `GOVERNANCE.md` is who decides.

## The one rule that is not negotiable

**Nothing reaches `main` except through a pull request that passed
`required-ci`.** Not a hotfix, not a one-line typo, not a documentation change,
not work by the owner. Direct pushes to `main` are blocked by branch protection
and that protection is not to be lifted "just this once".

The reason is specific to this product. `docs/06-COMPLIANCE.md` §3 contains two
requirements that are legal rather than product decisions, enforced five
independent ways. Every one of those five was added *after* something was found
to be broken. The gate is what stops the sixth.

## Setup

`docs/DEV-SETUP.md` covers what a machine needs. Then:

```bash
make doctor     # what this machine is missing
make gate       # everything CI enforces
```

`make gate` and CI run the same targets. "It passed locally" and "it passed in
CI" should never diverge; if they do, that is a bug in the Makefile.

## The loop

```text
pick the milestone item  ->  branch  ->  build with tests  ->  make gate
  ->  invariant-audit skill  ->  compliance-review skill if it applies
  ->  record deviations in docs/DECISIONS.md  ->  commit -s  ->  push  ->  PR
  ->  required-ci green  ->  merge  ->  update docs/PROJECT-STATUS.md
```

One milestone per branch. Branch names: `mN.N-short-slug` for milestone work,
`fix/slug`, `chore/slug`, `docs/slug` for everything else.

## Commits

Conventional commits, scoped to crate:

```bash
git commit -s -m "feat(detect): promote rail item on gate hit"
```

The `-s` is required. Every commit carries a `Signed-off-by` trailer certifying
the [Developer Certificate of Origin 1.1](https://developercertificate.org/).
CI rejects a pull request containing an unsigned commit.

Write the message for someone reading it in six months trying to work out why.
State what changed, then why, then what you deliberately did not do.

## Before opening a pull request

- [ ] `make gate` passes
- [ ] `make audit` passes, and any dependency change has a reviewed lock diff
- [ ] New behaviour has a test, and that test **fails without the change**
- [ ] The milestone's runnable-output command produces its specified output, and
      that output is pasted into the PR
- [ ] Every acceptance criterion is met, or restated in `docs/DECISIONS.md` with
      a reason. None silently skipped.
- [ ] Any deviation from the nine planning documents is recorded in
      `docs/DECISIONS.md`: what was specified, what was done, why
- [ ] `docs/PROJECT-STATUS.md` updated if what-is-built changed
- [ ] No real call audio, transcripts, prospect names, or phone numbers
- [ ] No credentials, and no model weights

## Reviewing

Use the `spec-auditor` and `rust-reviewer` subagents, and the `invariant-audit`
skill. A review is not a rubber stamp on a green build — CI checks what we
already knew to check.

Findings are ranked:

| | Meaning | Merge? |
|---|---|---|
| **P0** | Compliance, data loss, or the rail does not render | Never |
| **P1** | A documented behaviour is wrong | Never |
| **P2** | Degraded but usable | With a filed issue |
| **P3** | Cosmetic or cleanup | With a filed issue |

**No P0 or P1 merges.** Not with a follow-up issue, not with a TODO.

## Things that are treated as bugs even when tests pass

The seven invariants in `AGENTS.md`. Read them before touching `crates/detect`,
`crates/store`, `crates/audio`, or the card path. Breaking one is a bug even if
every test is green, because the tests are downstream of the invariants and not
the other way round.

## What never goes in a public issue

Real call content, a real removal request, a credential, or anything
identifying a prospect. Use a [private advisory](SECURITY.md). Recording law is
not a formality and this repository is public.
