---
name: gate-calibration
description: Measure whether the detection gate actually separates the objections, and set thresholds from that measurement. Use for M1.5, before starting M2, after changing the embedder or trigger phrases, or whenever asked why detection fired wrongly or what a threshold should be.
---

# Gate calibration

**The shipped thresholds are not calibrated.** `docs/PROJECT-STATUS.md` §3.1
records a single observation that they may be badly wrong, and the PRD's
precision and recall targets are unevidenced until this measurement exists. This
is the blocker on M2 — do not build more pipeline on an unmeasured gate.

## What is already known

Two embedders, complementary failure modes:

| | Precision | Recall on paraphrase |
|---|---|---|
| `HashEmbedder` (lexical, default) | excellent — nothing out-of-domain scored above 0.27 against thresholds of 0.55–0.64 | **~zero** — "we're all set thanks" does not match "I'm all set" |
| BGE-small (`--features onnx`) | **poor** — a high similarity floor puts unrelated short sentences at 0.6–0.75, inside the shipped threshold band | good |

The BGE observation is one probe, not a characterisation: `"we're all set thanks"`
matched **Need to ask my spouse** at 0.687 via "we make these decisions together",
beating the entry it should have matched. Nothing systematic has been run.

`HashEmbedder` is a CI fixture and a pipeline harness (`DECISIONS.md` D6). It is
not shippable as the gate. The PRD's recall target of > 0.60 is a BGE number.

## The measurement

Build a score matrix and report distribution separation. Not a pass/fail — the
*shape* is what tells you whether thresholds can work at all.

**Positives:** every trigger phrase in the playbook, scored against its own
objection. These should all hit.

**Near-negatives:** every trigger phrase scored against every *other* objection.
This is M1's acceptance criterion "no default objection matches another's trigger
phrases above threshold", and it is the one at real risk.

**Far-negatives:** 30–50 utterances a prospect actually says that are not
objections — "yeah", "hold on", "let me grab a pen", "sure, go ahead", "what was
the name again", plus a voicemail greeting and a wrong-number exchange. None
should hit anything.

**Report, per embedder:**

- the 10th percentile of the positive distribution
- the 90th percentile of the best near-negative distribution
- the gap between them — **this is the number that matters**
- per objection, the same, since thresholds are per objection
- any pair of objections whose distributions overlap

## Reading the result

- **Clear gap** → set each threshold at the midpoint, clamped to [0.50, 0.85].
  Done.
- **Narrow gap** → try, in this order: per-objection score normalisation; BGE's
  `query:`/`passage:` instruction prefixes (currently not used — `DECISIONS.md`
  D7, and the reasoning there is worth re-reading before overturning); then a
  cross-encoder rerank on gate hits.
- **No gap** → do not paper over it with thresholds. Try the hybrid first (below).
  If that also fails, it is a product finding: say so plainly, and the rail-only
  v1 becomes the whole v1. That is a real option, not a failure.

## Try the hybrid before the expensive fixes

The two embedders fail in opposite directions, which is the useful part. Score
with both and combine — floor-correct BGE, then take a weighted max against the
lexical score, or require lexical corroboration only inside the band where BGE's
floor sits. `Embedder` is a trait precisely so this is a composition rather than
a rewrite, and the matrix above produces the weights as a by-product.

This costs one extra embedding per candidate span. Both are CPU-cheap. It is
strictly cheaper than a cross-encoder and should be measured first.

## Rules for this work

- **Never tune a threshold against a single example.** That is how the current
  numbers got here.
- **Comply entries are calibrated differently.** `obj_dnc` ships at 0.55 against
  0.60–0.66 deliberately: a false positive costs nothing, a false negative is a
  TCPA problem. Do not "correct" it upward to match the others. It also matches
  lexically now (`DECISIONS.md` D11), so its threshold is a backstop rather than
  the primary path.
- **Report what you did not test.** A silent sampling cap reads as full coverage.
- Record the outcome in `docs/DECISIONS.md` and update `PROJECT-STATUS.md` §3.

## Benchmark it too

M1's open acceptance criterion: gate evaluation < 10 ms for a 100-objection
playbook. There is no benchmark yet. Add one under `crates/detect/benches/` and
run `cargo bench -p riposte-detect`. Note that vectors now load from the store
(`DECISIONS.md` D12), so benchmark evaluation, not index construction.
