# Project status — handoff

**As of 2026-08-19.** Written for whoever picks this up next.

The nine planning documents in this repo were reviewed in full, the repo was
scaffolded, and milestones **M0** and **M1** of `07-BUILD-PLAN.md` are built,
tested, and runnable. Two findings from that work change what the next
milestone should be. They are in §3 and they matter more than the code.

---

## 0. Update — product review and M1.1 remediation

A full product and code review was run over this repo after the M0/M1 handoff.
It produced 21 findings; the compliance ones are fixed and recorded, the rest
are tracked. **Read this section before §1–§4, which predate it.**

### The finding that mattered

**A removal request stated alongside another objection was served a rebuttal.**
Measured on the shipped playbook:

| utterance | card served, before the fix |
|---|---|
| "I'm not looking for anything, quit calling" | Not interested — **rebut**, 0.725 |
| "now's not a good time, quit calling" | Not a good time — **rebut**, 0.842 |
| "I already have insurance, quit calling" | Already have coverage — **rebut**, 0.797 |

`"quit calling"` alone scores **1.000** against `obj_dnc`. One clause in front of
it dragged the span to 0.387 — under the 0.55 threshold — so the compliance entry
never qualified. The four enforcement layers in `06-COMPLIANCE.md` §3 all protect
what happens *after* an objection is chosen. Nothing protected the choice.

Fixed two ways, both in `crates/detect/src/gate.rs` — `select_hit` (comply wins
outright) and `comply_containment` (comply also matches lexically, so dilution
cannot bury it). Recorded as **D11**. Required test
`comply_wins_a_mixed_utterance`, asserted end to end in CI.

### Also fixed

- **D8 built, not deferred.** The two prose guardrails in the playbook
  ("if they deny it a second time…", "one attempt only") are now enforced via
  `objections.escalate_after_repeats` and `decide_with_repeats`. §4.4 below
  claimed `Dedupe` already tracked per-call repeats — **it did not**, it stored
  only a last-seen timestamp, and it refreshed that timestamp on every
  suppressed hit, turning the fixed 45 s window into a sliding one. Both fixed.
- **Embeddings are stored at import** (**D12**). `phrases_needing_embedding`,
  `set_embedding` and `load_trigger_vectors` existed with **zero callers**; the
  gate re-embedded every trigger on every evaluation. Seconds per detection
  under BGE-small, against a Layer 1 budget of 700–900 ms.
- **Import is genuinely atomic.** `Store::with_tx` replaces bare
  `BEGIN`/`COMMIT`, and activation moved inside the boundary — it used to run
  after the commit, so a failure there left an imported-but-inactive playbook.
- **Migration tests are real.** `migrations_apply_and_are_idempotent` asserted
  only `schema_version() == 1` and never re-ran the migrator. Replaced with a
  genuine idempotency test and the v1-fixture test `03-DATA-MODEL.md` §5 asks
  for. Schema is now at version 2.
- **CI was red on `main`.** `cargo fmt --all -- --check` failed across all six
  source files. CI also never compiled `--features onnx`, so the *real* gate had
  rotted to an `unused_mut` warning — a hard failure under `-D warnings`. Both
  fixed, and the ONNX check added to CI so it cannot recur.

### M1.1 is complete

All 21 findings are closed. Beyond the compliance work above:

- **Nothing the user authored is discarded any more** (**D13**). `doc_refs`,
  the `documents:` block and `PlaybookSettings` were all parsed and dropped —
  `objection_docs` had no writer at all, and the gate used hardcoded constants
  while the playbook's own tuning knobs did nothing. Migration `0003` lets a
  document be *declared* before its content exists, which is the actual state
  of a bare-YAML import.
- **M0's four acceptance criteria are closed** (**D14**). Export now exists, so
  the required round-trip test could finally be written; the byte-identical
  criterion is restated as *export → import → export*, which is achievable and
  is the property actually worth having. Rail-slot collisions bump-and-warn as
  `03-DATA-MODEL.md` §3 specifies rather than rejecting the import.
  `OnCollision::{Replace, KeepBoth, Skip}` implements the id-collision policy —
  and `playbook_id` now transfers on Replace, which it did not, so an objection
  stayed with its old playbook while the importing one believed it had taken it.
- **Quality batch** (**D15**): card ids could collide inside a millisecond and
  drop a rail press — the exact event the headline metric counts; `detect`
  recorded nothing so `stats` read "the AI is never used" by construction; a
  corrupt `kind` downgraded silently to a compliance card; the tokenizer turned
  "can't" into "ca not".

**Verified green:** 72 tests, `cargo fmt --check` clean, clippy clean under
`-D warnings` on both the default and `onnx` feature sets, and every M0/M1
runnable criterion produces its specified output.

### Next

**M1.5 — gate calibration.** §3.1 below is unchanged and still the blocker on
M2. Nothing in M1.1 touched the thresholds; they remain uncalibrated, and the
default embedder remains a CI fixture rather than a shippable gate.

### Sequencing changed

Two milestones inserted, agreed with the product owner:

- **M1.5 — gate calibration.** §3.1 below, unchanged, and it **gates M2**.
- **M1.7 — rail overlay v0, Layer 0 only, to real agents.** The PRD's headline
  metric — rail use without AI — needs no audio, no ASR and no LLM. The original
  order spent the entire audio budget before any agent touched the product.
  `AGENTS.md`'s "do not begin at M4" forbids an overlay on *unmeasured
  detection*; a Layer-0 overlay has no detection to be unmeasured.

A **physical Windows 11 machine is available**, which retires the platform risk
described in §4.1 below. Read §4.1 as history.

See `docs/DECISIONS.md` D8, D11, D12 and `.claude/skills/` for the review
workflows (`invariant-audit`, `compliance-review`, `gate-calibration`,
`playbook-lint`, `insurance-domain`, `milestone`, `windows-verify`).

---

## 1. What is built

Cargo workspace, five crates, **56 tests passing**, no warnings, `cargo fmt`
clean. (Was 37 at the original handoff — see §0.)

| Crate | State | What it does |
|---|---|---|
| `crates/core` | done for M0/M1 | Domain types per `04-CONTRACTS.md` §1. `RebuttableObjection` newtype enforces invariant 4 at the type level. |
| `crates/store` | done for M0/M1 | `rusqlite` + bundled SQLite. Full schema, forward-only migration runner, WAL. |
| `crates/playbook` | done for M0 | YAML parse, validate, atomic import. `.playbook` zip container deferred to M7. |
| `crates/detect` | done for M1 | `Embedder` trait, gate scoring, window construction, dedupe, kind branch. |
| `apps/cli` | done for M0/M1 | `riposte` binary — the runnable output for both milestones. |

### Verified runnable

Both milestones' acceptance commands produce the specified output:

```bash
cargo run -p riposte-cli -- playbook import playbooks/insurance-cold-call.yaml
cargo run -p riposte-cli -- playbook list
cargo run -p riposte-cli -- rail
cargo run -p riposte-cli -- detect --text "yeah I never filled out any form" --all
```

`rail` prints the six rail objections with their kinds, DNC rendered in the
compliance colour. `detect` prints the matched objection, similarity, threshold,
the trigger that matched, and the authored response. All six default rail
objections detect correctly against their own trigger phrases.

Two extra commands exist to make the PRD's headline metric observable now
rather than at M8: `riposte hotkey <1-6>` records a manual rail use, and
`riposte stats` prints the manual-vs-detected split.

### Compliance invariant

`comply_objections_never_reach_llm` is **implemented and passing**
(`crates/detect/src/lib.rs`). Every DNC trigger phrase is driven through the
full gate against a provider that panics on invocation. It is guarded four ways
as `06-COMPLIANCE.md` §3 requires: the `RebuttableObjection` newtype, the
`allow_refine` override on import, a SQL trigger that reverts even a direct
`UPDATE`, and the test. Three of the four have their own tests.

The other three named invariant tests (`rail_renders_with_all_engines_disabled`,
`local_only_mode_makes_no_network_calls`, `redaction_applied_before_egress`)
belong to milestones not yet started.

### Not built

Everything from M2 onward: replay harness, LLM providers, audio capture, call
boundary detection, overlay, console, feedback learning. No Tauri, no audio, no
network calls anywhere in the codebase.

---

## 2. Deviations from the specs

Ten, all recorded with reasoning in **`docs/DECISIONS.md`**. Read that file
before changing the schema. The four that alter the data model:

- **D1** — `cards.call_id` made nullable and `session_id` added. As specified,
  `NOT NULL` made the PRD's *primary* success metric physically unrecordable,
  because manual rail use happens between calls and with no session.
- **D2** — `removal_requests` table added. Required by `06-COMPLIANCE.md` §3,
  absent from `03-DATA-MODEL.md` §2.
- **D3** — `PRAGMA foreign_keys = ON`. Off by default in SQLite, which silently
  made all ten `ON DELETE CASCADE` clauses no-ops, including the one the
  transcript TTL depends on.
- **D4** — `threshold` split into authored and learned columns. Three documents
  describe a console showing both with a reset button; one column cannot.

D5 defers `sqlite-vec` (embeddings are `f32` blobs; the gate brute-forces, which
the architecture doc already argues for). D9 changes the gate to score window
suffixes — found by a failing test, described below.

---

## 3. The two findings that should drive the next milestone

### 3.1 The default thresholds appear miscalibrated for BGE-small

This is the important one.

The gate was run with the real embedder (`--features onnx`, BGE-small-en-v1.5).
On the probe `"we're all set thanks"` — which should match **Not interested**,
whose authored trigger is *"I'm all set"* — it instead matched **Need to ask my
spouse** at 0.687 against that entry's threshold of 0.66, via the trigger
*"we make these decisions together"*.

That is a false positive on a phrase with no semantic relationship to the
matched entry. It is consistent with a well-known property of BGE models: they
produce a **high similarity floor**, where unrelated short sentences routinely
score 0.6–0.75. The playbook's thresholds (0.55–0.66) sit inside that floor, so
they are not discriminating.

**This is one observation, not a characterisation.** No systematic sweep was
run. What it establishes is that the shipped thresholds cannot be assumed
correct, and that `07-BUILD-PLAN.md` M1's acceptance criterion *"No default
objection matches another's trigger phrases above threshold"* is at real risk of
failing. It should be run as a measurement before any more pipeline is built on
top of the gate.

**Recommended next step — calibration before M2.** Build the score matrix:
every default trigger phrase against every objection, plus a set of
known-negative utterances. Report the separation between the true-match
distribution and the best false-match distribution. Then either raise the
thresholds to where the distributions separate, or — if they do not separate —
change the approach. Options in rough order of cost: normalise scores per
objection, use BGE's instruction prefixes (see D7), or add a cross-encoder
rerank on gate hits.

Until that measurement exists, the PRD §7 targets of precision > 0.75 and
recall > 0.60 are unevidenced.

### 3.2 The default embedder is a harness, not a gate

`HashEmbedder` (lexical: hashed character trigrams and word n-grams) is the
default so that CI is hermetic, replay determinism is achievable, and the gate
runs on any machine with no model download. It is honest about what it is:

- **Precision, 4 out-of-domain probes:** nothing scored above 0.27 against
  thresholds of 0.55–0.64. No false positives.
- **Recall, 5 paraphrase probes: zero.** *"we're all set thanks"*, *"I've got a
  guy who handles that already"*, *"catch me another time"* and
  *"I don't recall ever submitting anything online"* all miss entirely.

It matches near-verbatim triggers and nothing else. It is a CI fixture and a
pipeline harness. **It is not shippable as the gate**, and the recall target in
the PRD is a BGE number, not a `HashEmbedder` number. The trait exists precisely
so this swap is a one-line change.

### 3.3 A design change already made

**D9 — the gate scores window suffixes, not just the concatenated window.**
Found by a failing test: `"hmm / yeah, I don't know / we're already covered"`
scored under threshold because the filler diluted the cosine. Real call audio is
mostly filler, so this would have suppressed genuine objections in the field.
The gate now scores the whole window, the last two turns, and the last turn, and
keeps the best. `cards.window_text` stores the span that actually matched, which
is also the correct text to learn a negative trigger from.

---

## 4. Engineering concerns

Raised as the engineering manager on this build. Ordered by how much they should
change the plan.

### 4.1 Platform — the dominant risk

**The specs target Windows only. Development is happening on macOS.**

`crates/audio` (WASAPI, per-process loopback, `SetWindowDisplayAffinity`) cannot
be compiled meaningfully, run, or verified on this machine. That is **M4**, and
its entire acceptance checklist is manual and Windows-only: 60-minute dropout
test, allocation-free callback verification, Chrome per-process capture, device
change mid-session. M6's overlay acceptance criteria are also Windows behaviours
(capture exclusion, always-on-top surviving hide/show). M9 needs a code-signed
MSI and a clean Windows 11 install.

Consequences to plan around:

- Locally buildable and verifiable today: **M0, M1, M2, M3, M5 (partially), M7,
  M8**. That is most of the product, which is a credit to the layered design.
- **M4, M6, M9 need real Windows.** CI is already configured for
  `windows-latest`, which covers compilation but not audio behaviour. A physical
  Windows box or VM with audio hardware is required before M4 starts.
- `07-BUILD-PLAN.md` names **M5** as the riskiest milestone and it is right —
  call boundary detection is a heuristic tuned to Ricochet360's specific timing.
  **Get a recorded multi-call Ricochet360 session from the test agent now.**
  It is the long-lead item and nothing else unblocks it.
- Code signing certificates have a multi-day to multi-week lead time. If the
  ten-agent live test in M9 is real, **start that procurement now**, not at M9.

### 4.2 Specification defects

A systematic cross-check of the nine documents surfaced a substantial defect
list. Beyond the four already fixed (§2), these were independently confirmed by
reading and remain open:

- **`ObjectionKind::Disqualify` is defined, coloured in the UX spec, and never
  used.** No playbook entry uses it and `02-ARCHITECTURE.md` §7 routes it
  identically to `Rebut`. Give it a distinct behaviour or cut it.
- **The latency budget has no slack.** `02-ARCHITECTURE.md` §10 sums to
  670–880 ms against a Layer 1 target of 700–900 ms — with nothing allocated for
  thread handoff, resampling, or ASR queue contention. The 500 ms VAD trailing
  silence will probably need to drop to ~300 ms, at the cost of more
  fragmentation.
- **Pre-roll drain conflicts with the ASR back-pressure policy.**
  §4 says drain 10 s of pre-roll through ASR on `CallStarted`; §9 says the ASR
  pool drops the *oldest* item under pressure. So the pre-roll — the highest
  value audio in the call — is exactly what gets dropped, at exactly the moment
  it arrives. Needs a priority lane or a separate drain worker.
- **Local-only mode has a hole.** Enforcement checks `requires_network()` on the
  *provider type*. An Ollama provider pointed at a remote base URL defeats it
  silently. Enforce on the resolved URL, not the provider id.
- **Bare hotkeys `1`–`6` cannot be global.** `05-UX-SPEC.md` §3 wants them to
  work without focus; a global registration of bare digits would hijack those
  keys system-wide and the agent could not type "1" into their dialer. Needs a
  modifier, or focus-follows behaviour. Resolve before M6.
- **Two acceptance criteria are unachievable as written.** M0's "byte-identical"
  export round-trip is impossible while the manifest carries `exported_at`;
  M2's `--no-llm` determinism is not achievable with multi-threaded ONNX
  inference unless it is pinned single-threaded. Restate both.
- **Redaction gives false assurance.** The regexes in `06-COMPLIANCE.md` §4
  operate on ASR output, which renders spoken digits as words roughly half the
  time. A spoken SSN passes through untouched. Describe it as partial
  mitigation, not a guarantee.
- **"Recompute thresholds nightly"** does not work on a desktop app that is
  closed at night. Make it "on start if >24 h since last".

### 4.3 Public repository

The repo is public. Three things follow:

- **`.gitignore` is load-bearing, not hygiene.** Real call audio in a public
  repo is a criminal exposure in two-party-consent states. `**/*.wav` and
  friends are ignored with a narrow allowance for synthetic fixtures. Keep it
  that way, and turn on GitHub push protection plus a secret-scanning pre-commit
  hook before more contributors arrive.
- **Model weights must never be committed.** `models/`, `*.onnx`, `*.gguf` are
  ignored. Parakeet TDT alone is 631–680 MB, and these carry their own licences.
- **Lead with the compliance posture.** A public "cold call objection busting
  tool" invites a reading that the `comply` design specifically refutes. That
  design is the strongest thing in these documents and it is currently buried in
  document six.

### 4.4 Ethics, stated plainly

The `comply` guarantee is genuinely well designed — four independent
enforcement layers for a legal obligation is more than most commercial products
do. Two adjacent obligations are **not** enforced the same way, and should be:

`insurance-cold-call.yaml` carries *"If they deny it a second time, treat it as a
removal request"* (`obj_no_form`) and *"One attempt only. A second 'not
interested' ends the call"* (`obj_not_interested`) as **prose guardrails** — text
an LLM is asked politely to respect. Everything else compliance-critical in this
project is structural. `Dedupe` already tracks per-call repeats, so escalating a
second hit to the compliance card is a small change. Logged as **D8**, deferred
to M5 which owns card lifecycle.

This matters beyond ethics: it is the difference between a tool that coaches an
agent to push past a withdrawal of consent and one that does not.

> **Superseded — see §0.** Both are now enforced structurally, via
> `objections.escalate_after_repeats` and `decide_with_repeats`. D8 is built,
> not deferred. One correction to the paragraph above: `Dedupe` did **not**
> track per-call repeats. It stored a last-seen timestamp only, and refreshed it
> on every suppressed hit, which also turned the fixed 45 s window into a
> sliding one. Both fixed.

### 4.5 Working with a team of agents on this codebase

- **Rust build times dominate.** The dependency graph (`ort`, `fastembed`,
  `tauri`, bundled SQLite) is heavy; a full `target/` runs to gigabytes. Parallel agents
  in separate worktrees each get their own target directory. Use a shared
  `CARGO_TARGET_DIR` or `sccache`, and prefer `cargo check` in the inner loop.
- **One decision log, not per-document sections.** `AGENTS.md` says to record
  ambiguity resolutions in the relevant doc; with parallel agents that produces
  contradictory decisions in different files. `docs/DECISIONS.md` is the single
  log.
- **Scope creep is the named failure mode** and agents are prone to it.
  `01-PRD.md` §3 is the fence; §9 is where additions go.
- **CI is the gate for anything platform-specific.** An agent reporting M4
  "done" from a Mac has verified compilation and nothing else.

---

## 5. Suggested next steps

1. **Calibrate the gate** (§3.1). Build the score matrix, measure separation,
   set thresholds from data. This is a day of work and everything downstream
   depends on it. Do not build M2 on an unmeasured gate.
2. **Decide the `Disqualify` question** — behaviour or deletion.
3. **Request a recorded multi-call Ricochet360 session** from the test agent.
   Long lead time, blocks M5, and M5 is the riskiest milestone.
4. **Then M2** — the replay harness, with `evals/` reporting precision, recall,
   and latency, so the calibration from step 1 becomes a regression gate.
5. Resolve the hotkey collision (§4.2) before any M6 work starts.

M3 (LLM providers) remains genuinely optional to a shippable v1, and
`07-BUILD-PLAN.md` is right to say so. If anything slips, slip that.
