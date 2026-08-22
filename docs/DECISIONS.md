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

## D11 — `comply` wins the gate, and is matched lexically as well as semantically

**Specified:** `04-CONTRACTS.md` §4 — `DetectionGate::evaluate` "takes the far-end
window and returns at most one hit," scored by
`max cosine over positives − 0.5 × max cosine over negatives`, hit if
`score ≥ objection.threshold`. Nothing orders two simultaneous hits, and nothing
treats `comply` differently at selection time.

**Changed:** two things, both in `crates/detect/src/gate.rs`.

1. `select_hit` — a qualifying `comply` entry wins outright, whatever outscored
   it. Shared by `evaluate` and the CLI so they cannot drift.
2. `comply_containment` — `comply` entries additionally match lexically. If a
   trigger's normalised tokens appear as a contiguous run anywhere in the span,
   it scores 1.0.

**Why:** measured on the shipped playbook, before the change:

| utterance | card served |
|---|---|
| "I'm not looking for anything, quit calling" | **Not interested — rebut**, 0.725 |
| "now's not a good time, quit calling" | **Not a good time — rebut**, 0.842 |
| "I already have insurance, quit calling" | **Already have coverage — rebut**, 0.797 |

In isolation `"quit calling"` scores **1.000** against `obj_dnc`. One clause in
front of it drags the span to **0.387** — under `obj_dnc`'s 0.55 threshold — so
the compliance entry did not even qualify, and the agent was coached to rebut a
withdrawal of consent.

Both halves were needed. Priority alone does not help when dilution keeps the
comply entry under threshold. Containment alone does not help when both entries
qualify and the rebuttal scores higher — which `"I have an agent, remove me from
your list"` already did (DNC 0.728, Already-covered 0.629), decided by score
ordering rather than by rule.

D9's suffix scoring cannot address this: it splits on *utterance* boundaries and
both clauses arrive in one ASR utterance.

Containment is exact rather than fuzzy — the *whole* trigger must appear
contiguously — so "don't call her at work, call me instead" does not match the
trigger "don't call here again". Erring toward firing is nevertheless correct
here: `obj_dnc` ships at 0.55 precisely because *"a false positive here costs
nothing"* (`03-DATA-MODEL.md` §3). It costs no embedding, is immune to dilution
by construction, and behaves identically under any `Embedder` — which matters
while the semantic gate is uncalibrated (`PROJECT-STATUS.md` §3.1).

This is a fifth enforcement layer for `06-COMPLIANCE.md` §3, and the first that
protects *which* objection is chosen. The other four all protect what happens
after the choice is already made.

Tests: `comply_wins_a_mixed_utterance` (required), `dnc_survives_clause_dilution`,
`containment_does_not_fire_on_a_partial_trigger`,
`priority_does_not_suppress_ordinary_rebuttals`.

## D8 — *(built)* Repeat denial escalates to the compliance card

Recorded above as *proposed, not built*, and deferred to M5. Built now instead:
it is the ethical core of the product and the remaining half of the compliance
gap D11 opened up.

**Two corrections to the original D8 note.** It claimed *"`Dedupe` already
tracks per-call repeats, so a second hit … should promote the `comply` card"*
(and `PROJECT-STATUS.md` §4.4 repeated it). `Dedupe` tracked only a last-seen
timestamp — there was no count. It also refreshed that timestamp on every
suppressed hit, turning the fixed 45 s window of `02-ARCHITECTURE.md` §7.1 into
a sliding one, under which a prospect repeating an objection every 40 s is
suppressed for the entire call.

**Built:**

- `Dedupe` measures its window from an objection's *first* fire, and carries a
  per-call `hits` count that survives window expiry.
  Tests: `dedupe_window_is_fixed_not_sliding`, `dedupe_counts_repeats_across_the_whole_call`.
- `objections.escalate_after_repeats` (migration `0002_escalation.sql`), exposed
  in the playbook YAML. `NULL`/absent means no escalation.
- `decide_with_repeats(objection, repeats, comply)` serves the playbook's
  compliance card once the count is reached, tagging it `escalated_from` so the
  transcript records what the prospect actually repeated.
- The shipped playbook arms it on `obj_no_form` and `obj_not_interested` at 2 —
  the two entries whose guardrails already said so in prose.

An escalated card is a `comply` card, so invariant 4 still holds over it:
`an_escalated_card_still_cannot_reach_the_llm`.

**Why a playbook field rather than hardcoded ids:** an agency authoring its own
playbook has the same obligation, and hardcoding `obj_no_form` would leave their
entries unprotected. The behaviour belongs to the objection, not to this file.

**Kept:** escalation with no `comply` entry in the playbook leaves the rebuttal
standing rather than dropping the card. Serving nothing would be defensible, but
silently swallowing a detection is worse than the rebuttal and harder to notice.
Test: `escalation_without_a_comply_entry_keeps_the_rebuttal`.

## D12 — Trigger embeddings are stored, and reconciled rather than recreated

**Specified:** `07-BUILD-PLAN.md` M1 — "trigger and negative-trigger embedding at
playbook save time". `03-DATA-MODEL.md` §2 gives `triggers` and
`negative_triggers` an embedding column (as a blob, per D5).

**Found:** the storage path existed and was never called.
`Store::phrases_needing_embedding`, `set_embedding` and `load_trigger_vectors`
had **zero references anywhere in the workspace, including tests**. Nothing was
ever embedded at save time. `DetectionGate::build` re-embedded every trigger
phrase on every invocation.

Harmless under `HashEmbedder` — 4 ms for the shipped playbook's 54 phrases — and
disqualifying under BGE-small, where it is seconds per detection against a
Layer 1 budget of 700–900 ms (`02-ARCHITECTURE.md` §10).

**Changed:**

- `detect::embed_playbook(store, playbook_id, embedder)` embeds what is missing
  for that model and stores it. Called by `riposte playbook import`.
- `DetectionGate::from_vectors(objections, vectors, model_id)` builds from
  stored vectors. Used by `riposte detect`.
- `DetectionGate::missing_vectors()` reports objections with no vectors for the
  active model; the CLI warns rather than silently re-embedding, because a
  partially embedded playbook is an import bug and hiding it hides the cost.
- `Store::upsert_objection` now **reconciles** trigger rows against the playbook
  instead of deleting and re-inserting them. Delete-and-recreate assigned new
  row ids, so every re-import discarded every vector.

`embed_model` is stored beside each vector, so switching embedder invalidates
exactly the affected rows and re-embeds only those. Measured: first import of
the shipped playbook embeds 54 phrases, a second embeds 0.

Reconciliation also fixed something the old code got right by accident and would
have lost: `learned` triggers (`03-DATA-MODEL.md` §4) are owned by feedback, not
by the playbook file, and must survive a re-import.
Test: `a_reimport_preserves_learned_triggers`.

## D13 — Documents can be declared before their content exists

**Specified:** `03-DATA-MODEL.md` §2 gives `documents` a `bytes BLOB NOT NULL`
("original, for re-export"), because §3 embeds document content in the
`.playbook` zip — *"a playbook is portable or it is useless"*.

**Found:** three things parsed and silently discarded.

- `doc_refs` was read off every objection and dropped. `objection_docs` had
  **no writer anywhere in the codebase**, and `load_objections` hardcoded
  `doc_refs: vec![]`. The shipped playbook declares four links; all were lost.
- The `documents:` block was parsed into `DocumentEntry` and dropped.
- `PlaybookSettings` — `gate_threshold_default`, `dedupe_window_ms`,
  `card_lifetime_ms`, `preroll_ms` — was parsed and dropped, while the gate used
  hardcoded constants. `02-ARCHITECTURE.md` §5 requires these be config-exposed
  because *"different dialers behave differently and this will need field
  tuning"*.

**Changed:**

- Migration `0003_declared_documents.sql` makes `documents.bytes` and
  `ingested_at` nullable, and adds `declared_at`. **NULL bytes means "declared
  by a playbook, content not yet ingested"** — which is the actual state of
  affairs, since the unit of import today is a bare `playbook.yaml` and the zip
  container is deferred to M7. `mime` is derived from the filename.
- Import declares documents first (`objection_docs` has a foreign key onto
  them), then writes the links, then the settings.
- `Store::get_playbook_setting` / `set_playbook_setting`, namespaced
  `playbook.<id>.<name>` so importing a second playbook does not silently
  retune the first.
- `GateConfig` replaces the hardcoded window and dedupe constants;
  `detect::gate_config(store, playbook_id)` reads it back. Unparseable values
  fall back to the shipped defaults — a bad setting should degrade tuning, not
  stop detection.
- Validation now rejects a `doc_refs` entry naming no declared document. That
  was previously invisible, because the field was discarded before anything
  could notice.

**On rebuilding three tables in a migration:** SQLite cannot drop a NOT NULL
constraint in place. This is safe *here specifically* without a data copy
because nothing had ever written to `documents`, `doc_chunks` or
`objection_docs` — that omission is the bug being fixed — so every database at
schema version 2 has all three empty. The migration says so. Do not copy the
pattern for a table that holds data.

**Not built:** actual ingestion — reading a PDF/DOCX/MD/TXT, chunking, embedding
into `doc_chunks`. That is M3, and `01-PRD.md` F9 owns it. What lands here is the
manifest and the references, so that nothing the user authored is thrown away in
the meantime.

## D14 — M0's export criteria, restated and implemented

`07-BUILD-PLAN.md` M0 lists four acceptance criteria. Three were unimplemented
and one was unachievable as written. All four are now closed.

### "Round-trip test: import → export → import produces byte-identical output"

**There was no export.** `PlaybookFile::to_yaml` existed, was never called, and
was never tested, so the round-trip test `AGENTS.md` requires could not exist.

The criterion is also not achievable against the *hand-authored* file, and
`PROJECT-STATUS.md` §4.2 was right to flag it — though for a smaller reason than
the `exported_at` manifest field it names, since a bare YAML has no manifest.
The real obstacle is that `playbooks/insurance-cold-call.yaml` carries comments,
deliberate key ordering and block scalars, none of which any serializer
reproduces.

**Restated:** *export → import → export is byte-identical.* That is the property
worth having — it is what makes export a dependable way to move a playbook
between machines — and it is testable. Plus a second test asserting every
authored field survives the trip, because byte-stability is worth little if
fields are being dropped consistently.

`PlaybookFile::from_store` reads back the **authored** threshold, never the
learned one: a learned threshold is this machine's feedback history and
`06-COMPLIANCE.md` §2 keeps feedback out of exports.
Test: `export_carries_the_authored_threshold_not_the_learned_one`.

### "Rail slot uniqueness enforced; collision on import warns and bumps"

Implemented as a **hard rejection of the whole import** — not what
`03-DATA-MODEL.md` §3 asks for ("later entry wins, earlier is bumped below fold,
warn the user"), and hostile to anyone hand-editing a file.

Now resolved during import: the last entry claiming a slot keeps it, earlier
claimants are bumped to below-the-fold, and `ImportReport::warnings` carries the
explanation. A slot *outside* 1–6 is still a hard error — that is a malformed
file, not a resolvable conflict.

### "Objection id collides with existing → replace, keep both, or skip"

Absent. Import silently upserted, and because `upsert_objection`'s conflict
clause did not touch `playbook_id`, an id held by another playbook stayed with
its original owner while the importing playbook believed it had taken it.

Added `OnCollision::{Replace, KeepBoth, Skip}` (`--on-collision` on the CLI,
matching `04-CONTRACTS.md` §6's `import_playbook { onCollision }`), and
`playbook_id` now transfers on `Replace`. Re-importing the *same* playbook is
not a collision — that is an update, and it stays silent.

### "Migration test passes from a v1 fixture"

`migrations_apply_and_are_idempotent` asserted only `schema_version() == 1`,
never re-ran the migrator, and had no fixture. Replaced with
`migrations_are_idempotent` (which reopens a real file) and
`migrates_from_a_v1_fixture_preserving_data`, which builds a v1 database from
migration 1 alone and asserts the data survives to current.

## D15 — Quality fixes from the M1.1 audit

Small individually; collected here because several were invisible rather than
merely untidy.

**Card ids could collide.** `format!("card_{}", now_ms())` against a `TEXT
PRIMARY KEY` — two rail presses inside one millisecond, and the second was lost.
That is precisely the event `01-PRD.md` §7's headline metric counts.
`new_card_id()` uses timestamp + process id + counter; machine-local uniqueness
is sufficient, since §1 of the data model is one SQLite file per install with no
hosted component to merge against.

**`riposte detect` recorded nothing,** so `riposte stats` reported
`detected = 0` forever while presenting itself as the manual-vs-detected split —
the PRD's primary metric, reading as "the AI is never used". It now records a
card with `origin: detected`. Added `--dry-run` for probing and calibration, so
experiments do not pollute the metric.

**A corrupt `kind` was silently downgraded to `Comply`.** Failing closed is
right — a corrupt row must never become rebuttable — but doing it quietly turns
an agent's rebuttal into a "do not respond" card with no indication. A canary
query now returns `StoreError::BadKind`, which was defined and never
constructed. A CHECK constraint makes this unreachable in practice; it exists to
catch a bad migration or a tampered schema.

**The tokenizer mangled irregular contractions.** A blanket `n't` → ` not` turned
`"can't"` into `"ca not"` and `"won't"` into `"wo not"`, discarding the verb
stem. "I can't afford that" and "I won't be switching" are different objections
and both stems carry signal. Handled explicitly, and `HashEmbedder`'s model id
bumped to `hash-lexical-v2` — which is what invalidates the stored vectors,
since re-embedding is keyed off `embed_model` (D12). Verified against a live
database: the stale vectors were detected, the CLI warned, and a re-import
re-embedded exactly those rows.

**`threshold_learned` survives a re-import** because `upsert_objection` omits it
from the conflict clause. Correct — the file carries the authored default, the
learned value is this machine's feedback history — but undocumented and
untested, so nothing stopped someone "fixing" it. Now tested.

**Also:** dropped a two-element tuple whose second element was discarded at every
call site, and a `table_name` binding that restated its own input to launder a
lifetime.

### Deliberately not changed

**The CLI defaults `--db` to `riposte.db` in the working directory,** not
`%APPDATA%\Riposte\riposte.db` as `03-DATA-MODEL.md` §1 specifies. `riposte` is
a development driver, and a dev tool that writes into the user's real
application data is worse than one that does not. The app proper owns that path
from M1.7.

**A stale embedding model leaves detection blind rather than re-embedding on
the fly.** The CLI warns loudly and the rail still works — Layer 0 has no
dependency on any of this, which is the architectural bet. The *application*
should embed at startup rather than warn; that belongs to whichever milestone
owns app startup, not to a dev CLI.

## D16 — M1.5 calibration: what the gate actually does

`PROJECT-STATUS.md` §3.1 raised, from a single probe, that the shipped
thresholds might be miscalibrated for BGE-small, and §5 made measuring it the
step before anything else. This is that measurement. **The answer is worse than
§3.1 suspected, and it changes what v1 should be.**

Harness: `evals/`, `riposte-evals calibrate|check`. 54 held-out probes —
paraphrases a prospect might actually say, deliberately sharing little wording
with any authored trigger, plus 17 far-negatives of ordinary call speech.

Scoring a trigger against its own objection measures nothing: a lexical embedder
returns 1.000 by construction. M1's acceptance criteria do exactly that, which
is why passing them never meant the gate worked.

### Finding 1 — the shipped thresholds sit inside BGE's noise floor

Raw BGE-small cosine against **unrelated** speech: min 0.611, p50 0.717,
p90 0.800, max 0.809. The authored thresholds are 0.55–0.66.

**16 of 16 far-negative utterances (100%) clear an authored threshold.** With
BGE wired up as shipped, "hello" fires an objection card. §3.1's single
observation was not an outlier; it was the normal case.

All figures below rescale the semantic arm to `(raw − floor)/(1 − floor)` with
the floor measured at 0.720 from the far-negatives. That is a measurement device
so the arms are comparable — not a fix.

### Finding 2 — no arm comes close to the recall target

Held-out recall against `01-PRD.md` §7's target of **> 0.60**:

| arm | recall | precision |
|---|---|---|
| lexical (`hash-lexical-v2`) | **0.16** | 1.00 |
| BGE-small, floor-corrected | **0.13** | 1.00 |
| hybrid (max of both) | **0.13** | 1.00 |

Precision is perfect on all three because nothing fires. Separation, p10 of
should-match minus p90 of should-not: lexical **−0.248**, BGE **−0.285**, hybrid
**−0.205**. All negative — the distributions overlap, so **no threshold exists**
that separates them. Per-objection, only `obj_dnc` separates, and only because
it matches lexically (D11) rather than through the embedding gate.

The hybrid helps — it is the best of the three on separation, and it lifts
individual probes ("that wasn't me…" 0.000 → 0.077, "we're all set thanks"
0.370 → 0.409) — but not nearly enough. **It is not the fix, and the cheap
options in §3.1 will not close a gap this size either.**

### What follows

The rail-first sequencing was already agreed; this is the evidence for it.
Layer 0 needs none of the above and is unaffected. What must not happen is M2
being built on this gate, or the PRD's detection targets being carried forward
as if they were achievable — on this evidence they are not, with either
embedder, at any threshold.

The realistic options, none cheap: a cross-encoder rerank on gate hits; a much
larger authored trigger vocabulary per objection; fine-tuning an embedder on
real call transcripts; or accepting that detection is a Layer-2 concern and
letting the LLM classify directly. That is a product decision, and it should be
made with these numbers in hand rather than after M2.

**Thresholds are therefore unchanged.** Proposing values computed from
overlapping distributions would dress up noise as calibration. The harness
refuses to propose when the gap is under 0.05, and refuses entirely for
compliance entries — `obj_dnc`'s 0.55 is policy, not a tuning parameter.

### Fixed along the way

**A negative trigger suppressed its objection's own authored trigger.**
`04-CONTRACTS.md` §4 specifies `positive − 0.5 × negative`. On the shipped
playbook, "I didn't request a quote" scores 1.000 against `obj_no_form` while
its near-twin negative "yes I did request a quote" scores 0.786, giving 0.607 —
below the 0.62 threshold. A positive and a negative differing only in polarity
share nearly every lexical feature.

Changed so a negative counts only when it *outscores* every positive, and then
at full weight. The margin is 0.214 in both directions; only its sign says which
reading is right, so a margin-proportional penalty cannot separate them. This
also makes the documented intent — "a strong positive still wins" — literally
true. Tests: `a_negative_does_not_suppress_a_stronger_positive`,
`a_negative_still_suppresses_when_it_outscores_the_positive`.

**3 of 6 held-out removal requests were missed.** "please don't contact me
anymore", "I want off whatever list I'm on", "lose my number" — all genuine DNC
requests, none caught. Given Finding 2, the semantic gate cannot be relied on
here at all.

`obj_dnc`'s triggers are now enumerated exhaustively (7 → 25). Unlike every
other entry this is the correct approach: a removal request is a closed, legally
recognised vocabulary of perhaps twenty formulations, a false positive costs
nothing, and a false negative is a TCPA violation. Now 6/6, with no new
cross-match violations. `riposte-evals check` runs in CI and fails the build on
a regression.

**M1's last open acceptance criterion is closed.** `crates/detect/benches/gate.rs`:
0.286 ms p50 at 100 objections against a 10 ms budget — 35× headroom. Evaluation
only; index construction moved to import time in D12.

## D17 — Platform code in the desktop app lives in one module

**Specified:** `AGENTS.md` — *"`crates/audio` is the only crate permitted to
contain `#[cfg(windows)]`. If you find yourself wanting a platform cfg
elsewhere, the abstraction is wrong — fix the abstraction."*

**Tension:** `05-UX-SPEC.md` §6 requires the overlay to call
`SetWindowDisplayAffinity`, by name, in the Tauri setup hook. A window manager
cannot be abstracted away the way an audio backend can.

**Changed:** the rule is honoured by its reason rather than its letter. Every
platform call in `apps/desktop` lives in `src/platform.rs`, behind functions
that compile to a no-op elsewhere. Nothing above that module knows which OS it
is on, and `get_capabilities` reports what the platform can actually do so the
UI shows a disabled toggle with an explanation instead of one that silently does
nothing.

**Why this is not a violation:** the invariant's stated purpose is that the
pipeline compiles and tests on Linux so CI stays cheap and a macOS port is one
crate rather than a rewrite. `apps/desktop` is an app, not a pipeline crate; it
still compiles and tests on every platform; and the containment is identical to
the one `crates/audio` provides. The `windows` dependency is
`[target.'cfg(windows)'.dependencies]`, so it is not even fetched elsewhere.

## D18 — Rail hotkeys are `Ctrl+Alt+1`–`6`, not bare digits

**Specified:** `05-UX-SPEC.md` §3 — *"Manual invocation via hotkey `1`–`6`"*,
and §3's behaviour table wants them live without the overlay having focus.

**Problem:** those two requirements are incompatible. A *global* registration of
bare digits captures them system-wide, so the agent could not type a `3` into
their dialer, a policy number into their CRM, or a phone extension.
`PROJECT-STATUS.md` §4.2 flagged this as needing resolution before any overlay
work; this is the resolution.

**Changed:** the global binding is `Ctrl+Alt+1`–`6`. Bare `1`–`6` still work as
a convenience while the overlay itself has focus, and are suppressed inside text
fields so editing a card does not fire cards.

**Why `Ctrl+Alt` and not `Ctrl+Shift`:** `Ctrl+Shift+<digit>` collides with
browser tab-group and dev-tools bindings in Chrome, which is where the dialer
runs. `Ctrl+Alt+<digit>` is unclaimed on Windows and reachable one-handed while
the other hand is on a mouse.

The hotkey badge in the UI still shows the bare numeral. `05-UX-SPEC.md` §2 is
right that it teaches the shortcut passively, and `Ctrl+Alt+3` in a 22px square
is unreadable. The modifier is taught by the first-run screen and by muscle
memory, and the numeral is what disambiguates the six.

**Not yet resolved:** whether `Ctrl+Alt+<digit>` collides with anything in
Ricochet360 specifically. That needs the Windows box and the real dialer — it is
on the M1.7 checklist in the `windows-verify` skill.

## D19 — Delivery goes through a pull request, and `main` is protected

**Specified:** `AGENTS.md` said *"One milestone per branch. Do not merge a
milestone whose runnable output criterion is unmet."* It never said what merging
involved.

**Found:** three branches pushed, zero pull requests, `main` unprotected and its
last CI run red. Work was being treated as finished when the code worked. The
documents described engineering standards in detail and said nothing about
delivery, so delivery is what got dropped.

**Changed:** nothing reaches `main` except through a pull request that passed
`required-ci` — including owner-authored work. `AGENTS.md` opens with an
authority hierarchy and a "Delivery is not optional" section; `CONTRIBUTING.md`
carries the loop and the pre-PR checklist; `GOVERNANCE.md` carries the
solo-maintainer rule; the `ship` skill carries the procedure.

**Why the solo-maintainer rule allows self-merge:** required human approval is
zero because self-approval is impossible on GitHub, and a rule that cannot be
satisfied gets bypassed — which is worse than one honestly scoped. Pull request,
green `required-ci`, and recorded review evidence all still apply. A second
maintainer turns required CODEOWNER approval on.

## D20 — CI is many leaves behind one aggregate gate

**Changed:** thirteen leaf jobs and a single `required-ci` that branch protection
points at.

**Why an aggregate:** adding a check would otherwise mean editing the protection
rule, which is a setting rather than a reviewed file, so it drifts. More
importantly, `required-ci` inspects each leaf result explicitly rather than
relying on `needs` alone — **GitHub treats a skipped required check as passing**,
so a job that silently stops running still shows green. The one deliberate skip
is `dco` outside a pull request, where there is nothing to inspect.

**Why these leaves:** each closes something that had already gone wrong or was
unverified — the MSRV was an unenforced promise, the ONNX gate had rotted to a
fatal warning because CI never compiled that feature, both M0 and M1 were
reported complete with six acceptance criteria open, and the compliance
selection defect (D11) is silent when it regresses. `no-call-audio` exists
because `.gitignore` should not be the only thing standing between this public
repository and a criminal exposure in a two-party-consent state.

## D21 — `unmaintained` advisories are scoped to direct dependencies

**Specified:** not addressed.

**Changed:** `deny.toml` sets `unmaintained = "workspace"`. Vulnerabilities and
yanked crates fail unconditionally and that is not configurable here.

**Why:** scoped to everything, the gate reports 16 findings. All are transitive,
none is a vulnerability, ten are Tauri's GTK3 Linux backend — which this product
does not ship, since it targets Windows, and for which no upgrade exists. A gate
that fires 16 times on things nobody can fix is a gate people learn to ignore,
and the next real advisory arrives into that habit. Scoping it to what we
directly control keeps it actionable.

`cpal` is banned outright: `02-ARCHITECTURE.md` §4 settles that decision, and a
reappearance in the graph means someone reopened it without saying so.
