---
name: compliance-review
description: Review a change against docs/06-COMPLIANCE.md before it merges. Use for any diff touching crates/detect, crates/llm, crates/store, the playbook format, or card rendering — and whenever asked about TCPA, DNC, consent, redaction, retention, or local-only mode.
---

# Compliance review

`docs/06-COMPLIANCE.md` contains two requirements that are legal rather than
product decisions. This is the gate they pass through. It is engineering
guidance, not legal advice — say so if asked to opine on the law.

Work through every section that the diff touches. Report `pass` / `fail` /
`not touched` with evidence.

## 1. The `comply` rule — the one that matters

A prospect saying "take me off your list" has made a request with legal weight.
A tool that answers with a rebuttal is coaching a violation, with authority.
Riposte must be **structurally incapable** of that.

Five layers. A diff may not weaken any of them.

| # | Layer | Where |
|---|---|---|
| 1 | `RebuttableObjection` cannot be constructed from a `Comply` objection | `crates/core/src/lib.rs` |
| 2 | `allow_refine` forced to 0 by SQL trigger on insert **and** update | `crates/store/migrations/0001_init.sql` |
| 3 | Import overrides the file's own `allow_refine` claim | `crates/playbook/src/lib.rs` |
| 4 | Required test `comply_objections_never_reach_llm` | `crates/detect/src/lib.rs` |
| 5 | A qualifying `comply` entry wins the gate outright, and matches lexically so it cannot be diluted | `crates/detect/src/gate.rs` — `select_hit`, `comply_containment` |

Layer 5 is `docs/DECISIONS.md` D11 and is the only one protecting *which*
objection is chosen. Layers 1–4 all protect what happens after the choice.
Before it existed, `"I already have insurance, quit calling"` served a rebuttal.

**Check specifically:**

- Does any new selection, ranking, or tie-break path bypass `select_hit`?
- Does `refine()` still take `RebuttableObjection` rather than `Objection`?
- Can a new authoring or import path set `kind` after `allow_refine`?
- Does a new card type render a comply objection with alternatives beside it?
  Comply cards take the whole panel and offer one action. *"The correct number
  of alternatives to offer at this moment is zero."*

**Repeat escalation** (`DECISIONS.md` D8) is part of this rule, not an extra.
`escalate_after_repeats` must survive import, and an escalated card is still a
comply card. Check `an_escalated_card_still_cannot_reach_the_llm` still passes.

## 2. Redaction before egress

Any text leaving the machine passes `redact()` first, applied when the
`ChatRequest` is built — never to local storage or display, since the agent
needs to see what was actually said. Skipped entirely in local-only mode.

Required test: `redaction_applied_before_egress`.

**State the limitation honestly wherever it is described.** The regexes run on
ASR output, which renders spoken digits as words roughly half the time. A spoken
SSN passes through untouched. `PROJECT-STATUS.md` §4.2: describe it as partial
mitigation, never as a guarantee. Flag any UI copy or doc that overclaims.

## 3. Local-only mode

A guard wrapper returns `LlmError::Blocked` before dispatch. It covers remote ASR
and the OpenRouter catalog fetch too, not just chat.

**Enforce on the resolved base URL, not the provider id.** An Ollama provider
pointed at a remote host defeats a `requires_network()` check on the provider
type. This hole is recorded in `PROJECT-STATUS.md` §4.2 — confirm it is closed
before M3 merges.

Required test: `local_only_mode_makes_no_network_calls`.

## 4. Retention

- Raw audio: never written to disk unless session recording is explicitly on.
- Transcripts: 30-day TTL by default. The purge must **cascade to
  `cards.window_text`** or the TTL is theatre. This depends on
  `PRAGMA foreign_keys = ON` (`DECISIONS.md` D3) — verify it is still set.
- `removal_requests` deliberately outlives the TTL: `call_id` and `card_id` are
  nullable with `ON DELETE SET NULL`. A removal request is the one record an
  agent may need months later.
- API keys: OS credential store only. Never in config, exports, or logs.
- Playbook exports carry no transcripts, no feedback, no secrets.

## 5. Claims

These must stay true in the README, the UI, and any new copy:

- **Not a TCPA compliance system.** No list scrubbing, no DNC registry, no
  suppression of a single dial.
- **Not a call recorder.** Off by default, not the product.
- **Not a system of record.** The dialer and CRM are.
- **Not a supervisor tool.** No manager view, no remote access, no telemetry
  leaving the machine. *"Is my boss watching this" is the first question any
  agent asks.*

Riposte does not maintain a DNC list. The removal log exists so the agent can
enter it in their real dialer. Claiming more invites reliance the system cannot
support, which is worse than claiming nothing.

## 6. Line of business

The shipped playbook targets **P&C auto/home**. If a change targets **Medicare
Advantage or Part D**, stop and escalate: CMS marketing rules require recording
the entire sales call and retaining it, which is the direct opposite of this
product's ring-buffer stance. That is a market gate needing a compliance-recording
mode, not a feature request.
