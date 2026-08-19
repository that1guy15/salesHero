# 06 — Compliance and Privacy

Read before implementing M2. Two requirements here are legal rather than product decisions, and both are cheaper to build in than to retrofit.

**This document is engineering guidance, not legal advice.** Anyone deploying Riposte on live calls should have their own counsel review their recording disclosure and DNC process. Insurance telemarketing sits under both TCPA and state insurance regulation, and the specifics vary by state and line of business.

---

## 1. Recording consent

Riposte processes both sides of a live phone call. In roughly a dozen US states, recording a call without all parties' consent is a crime, not a policy violation. Several of those states are large insurance markets.

A defensible position is that Riposte does not *record* by default — audio lives in a ring buffer and is discarded — but transcription of the far party's speech is close enough to recording that treating it as such is the only sensible engineering posture.

**Requirements:**

- First-run disclosure screen with an explicit acknowledgment checkbox. Not skippable, not defaulted checked.
- Ship a disclosure script the agent can read: *"Just so you know, I use a tool that takes notes during our call — is that alright with you?"*
- A per-session banner in the console showing whether transcript retention is on.
- Settings text stating plainly what is processed, what is stored, and for how long.

**Do not** build automatic consent detection ("did the prospect say yes?"). It would be unreliable, and unreliable consent detection is worse than none because it manufactures a false record.

---

## 2. Data handling

| Data | Default | Notes |
|---|---|---|
| Raw audio | **Never written to disk** | Ring buffers only. Session recording is an explicit opt-in setting, default off, with its own warning. |
| Transcripts | Stored, 30-day TTL | TTL configurable 1–365 days or "never store". Purge job runs at app start. |
| Card text and feedback | Stored indefinitely | Needed for threshold learning. Contains prospect utterance snippets — covered by transcript TTL, which cascades. |
| API keys | Windows Credential Manager | Never in config files, never in playbook exports, never in logs. |
| Playbook exports | No transcripts, no feedback, no secrets | Objections and documents only. |
| Crash reports | Opt-in, scrubbed | No transcript text, ever. |

**Transcript TTL cascades to cards.** When an utterance is purged, `cards.window_text` for cards in that call is nulled. Otherwise the TTL is theater.

---

## 3. The `comply` rule

This is the most important requirement in the document.

When a prospect says "take me off your list," "stop calling me," or "put me on your do-not-call list," they have made a request with legal weight under the TCPA. A tool that responds by suggesting a rebuttal is actively coaching the user toward a violation, and doing so with an air of authority.

**Riposte must be structurally incapable of this.**

### Enforcement, four layers

**1. Type level.** `ObjectionKind::Comply` is a distinct variant. The refinement path takes `RebuttableObjection`, a newtype that cannot be constructed from a `Comply` objection:

```rust
pub struct RebuttableObjection(Objection);

impl RebuttableObjection {
    pub fn new(o: Objection) -> Option<Self> {
        match o.kind {
            ObjectionKind::Comply => None,
            _ => Some(Self(o)),
        }
    }
}

// refine() takes RebuttableObjection, not Objection.
// A comply objection cannot reach the LLM without changing this signature.
```

**2. Data level.** `allow_refine` is forced to 0 by SQL trigger on insert and update whenever `kind = 'comply'`. Playbook import applies the same override regardless of file contents — a hand-edited YAML cannot enable it.

**3. Test level.** Required test `comply_objections_never_reach_llm`: construct a playbook where every objection is `comply`, run the full pipeline over a fixture containing all their trigger phrases against a mock provider that panics on invocation. The test passes only if the mock is never called.

**4. UI level.** Comply cards take the full panel, hide the rail, carry no feedback buttons, and offer one action: log the removal and end the call. There is no affordance for continuing the conversation.

### Authoring constraints

The playbook editor must not allow a `comply` objection's response to contain a rebuttal. Enforcement is by warning rather than by blocking — an agency may have legitimate wording variations — but the warning is explicit:

> This is a compliance response. It should confirm the request and end the call. Do not include offers, alternatives, or reasons to stay on the line.

### Removal logging

"Log removal and end call" writes a row to a `removal_requests` table with call id, timestamp, and the triggering utterance, and offers a CSV export.

Riposte does **not** maintain a DNC list and does not suppress future dialing — it has no dialer integration. Say this plainly in the UI: the log exists so the agent can enter the removal in their actual dialer and CRM. Claiming more would be worse than claiming nothing, because it would invite reliance on a system that cannot deliver.

---

## 4. Redaction before egress

Insurance calls contain SSNs, dates of birth, driver's license numbers, and policy numbers, often in the first two minutes. Any text leaving the machine passes a redaction pass first.

```rust
pub fn redact(text: &str) -> String  // applied to window_text and chunks
                                     // in ChatRequest construction
```

| Pattern | Replacement |
|---|---|
| SSN (`\d{3}-?\d{2}-?\d{4}`) | `[SSN]` |
| Card numbers, Luhn-valid 13–19 digits | `[CARD]` |
| Digit runs ≥ 9 | `[NUMBER]` |
| Email addresses | `[EMAIL]` |
| Dates of birth in context (`born`, `DOB`, `date of birth` within 20 chars) | `[DOB]` |

Redaction applies **only to egress**, never to local storage or display. The agent needs to see what the prospect said. Skip it entirely in local-only mode, since nothing leaves.

Required test: `redaction_applied_before_egress` — assert that a `ChatRequest` built from a window containing each pattern carries none of them.

---

## 5. Local-only mode

A switch, prominently placed, that guarantees the process makes no outbound connections.

**Enforcement is at the provider layer, not by convention.** A guard wrapper checks `requires_network()` before dispatch and returns `LlmError::Blocked`. The same guard covers remote ASR engines and the OpenRouter model catalog fetch.

When on:
- Provider list shows only local options (Ollama, LM Studio, llama.cpp)
- Remote ASR engines are hidden, not merely disabled
- The OpenRouter catalog is unavailable with an explanation
- Redaction is skipped
- Footer shows `◐ local`

Required test: `local_only_mode_makes_no_network_calls` — enable it, run the pipeline over a fixture with a transport layer that fails the test on any socket open.

An agency handling health or life applications may need this to be true rather than merely claimed. Make it verifiable.

---

## 6. What Riposte is not

State these in the README and in the app, because users will assume otherwise:

- **Not a TCPA compliance system.** It helps an agent respond correctly to a removal request. It does not scrub lists, honor DNC registries, manage consent records, or prevent a single non-compliant dial.
- **Not a call recording system.** Recording is off by default and is not the product.
- **Not a system of record.** The dialer and CRM are. Riposte's logs are a convenience, not an authority.
- **Not a supervisor monitoring tool.** There is no manager view, no remote access to another agent's sessions, and no telemetry leaving the machine.

That last one is worth stating loudly. Agents are the ones who have to trust this software enough to leave it running, and "is my boss watching this" is the first question any of them will ask.
