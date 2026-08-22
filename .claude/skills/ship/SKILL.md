---
name: ship
description: Take finished work from a working tree to merged through a pull request. Use whenever code is written and ready — branching, committing, opening the PR, driving required-ci to green, and merging. Invoke it instead of running raw git or gh commands by hand.
---

# Shipping

Work is done when it is **merged through a pull request that passed
`required-ci`**. A pushed branch is an unfinished task. If you wrote the code,
you open the pull request — nobody has to ask.

Never push to `main`. Branch protection blocks it and that is deliberate.

## 1. Branch

```bash
git status --short          # must be clean before you start
git checkout main && git pull --ff-only
git checkout -b <name>
```

| Kind of work | Name |
|---|---|
| A milestone | `m1.7-rail-overlay` |
| A defect | `fix/comply-loses-the-gate` |
| Process, CI, deps | `chore/engineering-workflow` |
| Documentation only | `docs/windows-testing` |

One milestone per branch. If the branch depends on another unmerged branch,
branch from it and target the PR at it.

**Merging a stack:** merge bottom-up, and do **not** pass `--delete-branch`
until the whole stack is in. Deleting a base branch *closes* every pull request
targeting it, and a closed PR cannot be reopened or retargeted — it has to be
raised again from scratch. Rebase-merging also rewrites SHAs, so every branch
still in the stack needs `git rebase origin/main` before its checks mean
anything.

```bash
gh pr merge <bottom> --rebase          # no --delete-branch yet
git checkout <next> && git rebase origin/main && make push
gh pr merge <next> --rebase
# delete the branches once the stack is empty
```

## 2. Before you commit

```bash
make gate     # fmt, clippy on both feature sets, tests, detection gate
make audit    # advisories, licences, banned crates, sources
```

Then the review skills. These catch what CI cannot, because CI only checks what
we already knew to check:

- `invariant-audit` — always
- `compliance-review` — if the diff touches `crates/detect`, `crates/store`,
  `crates/llm`, the playbook format, or card rendering
- `spec-auditor` subagent — at the end of a milestone
- `rust-reviewer` subagent — for anything in `crates/`

**No P0 or P1 merges.** Not with a follow-up issue attached. Fix it or do not
open the PR.

Record every deviation from the nine planning documents in `docs/DECISIONS.md`
before committing — what was specified, what you did, why. A deviation found in
review that is not written down is a finding.

## 3. Commit

```bash
git commit -s -m "feat(detect): promote rail item on gate hit"
```

`-s` is mandatory. CI rejects a PR with an unsigned commit and fixing it means
rewriting history, which is worse than remembering.

Conventional commits scoped to crate: `feat`, `fix`, `chore`, `docs`, `refactor`,
`test`, `perf`. Scope is the crate or app: `detect`, `store`, `playbook`,
`desktop`, `evals`, `cli`, `ci`.

Body: what changed, then **why**, then what you deliberately did not do. Write
for someone reading it in six months trying to understand a decision. Include
the measured numbers when the change was driven by measurement.

Every commit needs the trailers the harness requires, in addition to the
sign-off.

## 4. Open the pull request

```bash
make push          # runs the gate first and refuses to push if it is red
gh pr create --fill --base <base>
```

Use `make push`, not a bare `git push`. Running the gate and then pushing as two
separate commands means a red gate does not stop the push — that has already
happened once. `make push` also refuses outright on `main`.

Then fill the template properly. It is not ceremony:

- **Runnable output** — paste the actual terminal output of the milestone's
  command. Not "tests pass". `07-BUILD-PLAN.md` exists because milestones get
  declared done on "it compiles".
- **Acceptance criteria** — every box `met`, `unmet`, or `restated in
  DECISIONS.md`. M0 and M1 were both reported complete with six criteria open;
  that is the failure this section prevents.
- **Invariants** — tick the ones the diff could touch, having actually checked.
- **Not included** — what a reader would reasonably expect and will not find.

## 5. Drive it green

```bash
gh pr checks --watch
```

When something fails, read the log before changing anything:

```bash
gh run view --log-failed
```

Fix the cause. Do not retry hoping for a different result, do not disable the
check, and do not add an exception to `deny.toml` to make a finding go away —
an exception is a decision that needs a reason written next to it.

`required-ci` is the aggregate. It fails if any leaf failed, and it treats an
unexpected skip as suspicious, because GitHub counts a skipped required check
as passing.

## 6. Merge

```bash
gh pr merge --squash --delete-branch
```

Squash: one milestone, one commit on `main`, and the PR keeps the detail. The
squash message is the PR body, so it needs to be worth reading.

With one maintainer, required human approval is zero — self-approval is
impossible and a rule that cannot be satisfied gets bypassed. `required-ci`
still has to be green, and review evidence still has to be attached. See
`GOVERNANCE.md`.

## 7. After merge

```bash
git checkout main && git pull --ff-only
```

- Update `docs/PROJECT-STATUS.md` — what is built, what is not, what you found.
  Write it for whoever picks this up next, not as a changelog.
- Close the issue, if there was one.
- Confirm the branch is deleted locally and remotely.

## Releases

Tags are a separate authority from merging. A tag matching `v*.*.*` builds
installers and opens a **draft** release; a draft is not a release. Publishing
needs `docs/WINDOWS-TESTING.md` completed on real hardware and a signed
installer. Distribution to live agents needs the consent flow reviewed by
counsel. Never tag or publish without explicit authorisation — see
`GOVERNANCE.md`.

## When you are blocked

Say so, in the PR, with the specific blocker. A pull request parked with an
honest "blocked on X" is useful. One quietly left open is not.
