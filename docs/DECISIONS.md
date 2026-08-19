# Decisions

Interpretations chosen where the specs were ambiguous, silent, or wrong, per
`AGENTS.md` — *"write down the interpretation you chose, then proceed."*

One log for the whole project rather than a `## Decisions` section per
document: with several people (or agents) working in parallel, per-document
sections drift into contradicting each other, which is the failure this rule
exists to prevent.

Format: what was specified, what we did instead, why.

---

## D1 — `cards.call_id` and `cards.session_id` are nullable

**Specified:** `03-DATA-MODEL.md` §2 declares `call_id TEXT NOT NULL REFERENCES calls(id)`, and there is no `session_id` column.

**Changed:** both columns are nullable, and `session_id` was added.

**Why:** the PRD's *primary* success metric is rail use without AI assist
(`01-PRD.md` §7, first row — "the first row matters most"). `05-UX-SPEC.md` §4
keeps hotkeys live in the `No session` and `Session, between calls` states, and
`03-DATA-MODEL.md` §2 explicitly requires manual uses to be recorded as cards
with `layer_reached = 0` so they are countable by the same query as detected
cards. With a `NOT NULL` `call_id` there is no row to point at, so the headline
metric could not be recorded at all. Test: `manual_rail_use_is_recordable_without_a_call`.

## D2 — `removal_requests` table added

**Specified:** `06-COMPLIANCE.md` §3 requires that "log removal and end call"
writes a row with call id, timestamp, and triggering utterance, plus a CSV
export. No such table exists in `03-DATA-MODEL.md` §2.

**Changed:** added `removal_requests`, with `call_id` and `card_id` nullable and
`ON DELETE SET NULL`.

**Why:** straight omission. Nullable and `SET NULL` because the removal log must
outlive the transcript TTL purge — a removal request is the one record the agent
may need months later, and it is not transcript data.

## D3 — `PRAGMA foreign_keys = ON`

**Specified:** `03-DATA-MODEL.md` §1 sets `journal_mode` and `synchronous` only.

**Changed:** also enable `foreign_keys`.

**Why:** SQLite defaults this **off**, which silently makes all ten
`ON DELETE CASCADE` clauses in the schema no-ops — including the one
`06-COMPLIANCE.md` §2 relies on for the transcript TTL. A TTL that does not
cascade is theatre. Test: `foreign_keys_are_enforced`.

## D4 — `threshold` split into `threshold_authored` and `threshold_learned`

**Specified:** one `threshold REAL NOT NULL DEFAULT 0.62` column.

**Changed:** two columns; `effective_threshold()` prefers the learned value.

**Why:** `03-DATA-MODEL.md` §4 and `05-UX-SPEC.md` §5 both require the console to
show the learned value *next to* the authored default with a reset button. One
column cannot represent both, and overwriting the authored value makes the reset
button unimplementable.

## D5 — Embeddings stored as `BLOB`, not in `sqlite-vec`

**Specified:** `03-DATA-MODEL.md` §2 declares `vec_triggers`, `vec_negatives`,
and `vec_chunks` as `vec0` virtual tables.

**Changed:** an `embedding BLOB` column (raw little-endian `f32`) on `triggers`,
`negative_triggers`, and `doc_chunks`. No `sqlite-vec` dependency yet.

**Why:** `02-ARCHITECTURE.md` §7.3 already argues that for a playbook of tens to
low hundreds of entries, "brute-force SIMD beats any index and is simpler than
both" — so the gate never queries these tables by KNN anyway. Deferring
`sqlite-vec` removes a C build dependency from every milestone before M3.
Revisit at M3, when document-chunk retrieval genuinely needs KNN over a larger
corpus; the blob layout is dimension-identical, so migrating is a backfill.

## D6 — The embedder is a trait with a no-model default

**Specified:** `02-ARCHITECTURE.md` §7 names `fastembed` / BGE-small-en-v1.5 as
*the* embedder.

**Changed:** `Embedder` is a trait. `HashEmbedder` (hashed character trigrams and
word n-grams, deterministic, no model, no network) is the default;
`OnnxEmbedder` (BGE-small via `fastembed`) is behind the `onnx` feature.

**Why:** three reasons. CI stays hermetic and fast with no model download.
`--no-llm` replay determinism (`07-BUILD-PLAN.md` M2) is achievable, which it is
not with multi-threaded ONNX inference. And the gate is testable on any machine
before the ONNX toolchain is settled.

**Caveat, and it is a real one:** `HashEmbedder` is *lexical*. Measured on the
shipped playbook it has excellent precision (nothing out-of-domain scores above
0.27 against thresholds of 0.55–0.64) and **near-zero recall on paraphrases** —
"we're all set thanks" does not match the authored trigger "I'm all set".
It is a pipeline harness and a CI fixture, **not** a shippable gate. The
`01-PRD.md` §7 recall target of > 0.60 is a BGE number.

## D7 — No BGE instruction prefix

**Specified:** not addressed.

**Changed:** both sides are embedded with no `query:` / `passage:` prefix.

**Why:** BGE's asymmetric prefixes are for retrieval, where a short query is
matched against a long document. Here both sides are short conversational
utterances, so the symmetric treatment is the better fit. Consistency matters
more than the choice; revisit if measured precision disappoints.

## D8 — Repeat denial escalates to the compliance card *(proposed, not built)*

**Specified:** `insurance-cold-call.yaml` carries these as prose guardrails:
`obj_no_form` — "If they deny it a second time, treat it as a removal request";
`obj_not_interested` — "One attempt only. A second 'not interested' ends the call."

**Proposed:** enforce both in code. `Dedupe` already tracks per-call repeats, so
a second hit on `obj_no_form` within a call should promote the `comply` card
rather than the rebuttal.

**Why:** every other compliance guarantee in this project is structural — type
level, SQL trigger, required test. These two are the same obligation expressed
only as text that an LLM is asked politely to respect. Coaching an agent to keep
pushing after a second denial of consent is the exact failure `06-COMPLIANCE.md`
§3 exists to prevent. Deferred to the milestone that owns card lifecycle (M5),
not skipped.

## D9 — The gate scores window suffixes, not just the whole window

**Specified:** `02-ARCHITECTURE.md` §7.1 — build a window of the last 3 far-end
turns, embed it, cosine against triggers.

**Changed:** score every suffix of the window (last 1, last 2, all 3) and keep
each objection's best.

**Why:** found by a failing test. Scoring only the concatenation loses real
objections to dilution — "hmm / yeah, I don't know / we're already covered" is
mostly filler, and averaging it in drags the cosine under threshold. Suffix
scoring lets a clean single turn match at full strength while the longer span
still catches a genuinely split objection. Costs at most 3 embeddings per
evaluation. `cards.window_text` now stores the span that actually matched, which
is also the correct text to learn a negative trigger from.

## D10 — Repo layout matches the documents

**Specified:** every cross-reference in all nine documents points at `docs/…` and
`playbooks/…`.

**Changed:** moved the flat files into `docs/` and `playbooks/` accordingly.

**Why:** every link in the repo was broken, including the four commands in
`07-BUILD-PLAN.md`.
