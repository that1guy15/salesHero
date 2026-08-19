# 01 — Product Requirements

## 1. Problem

An insurance agent working bought internet leads makes 60–120 dials a day. Connect rates are low, and the calls that do connect are decided in the first fifteen seconds — the prospect says something like "I never filled out any form" or "I'm already covered," and the agent either has a good response ready or the call is over.

Agencies write these responses down. They live in a Google Doc, a laminated card, or a training session six months ago. Under time pressure, none of those are reachable. The agent has one second between the prospect's objection and the moment their hesitation becomes obvious.

Existing tools address this from the wrong end. Enterprise agent-assist platforms (Balto, Cresta, Level AI) require telephony integration, enterprise contracts, and a data science team to tune. Dialer-native coaching (Kixie AI Coach, Dialpad Sell) is locked to that vendor's phone system. A five-person agency on Ricochet360 can get neither.

## 2. Who this is for

**Primary: the producing agent.** Works a dialer in Chrome. Headset on. Ricochet360, Kixie, JustCall, or similar. Makes their own calls, often owns or co-owns the agency. Not technical. Will not configure a YAML file — but will happily click "edit" on a card and retype the response in their own words.

**Secondary: the agency owner with 3–15 producers.** Wants everyone using the same rebuttals. Cares that the new hire in week two sounds like the veteran. Will author the playbook once and export it to the team.

**Explicitly not for v1:** enterprise contact centers, teams needing supervisor dashboards, anyone requiring CRM writeback.

## 3. Scope

### 3.1 Ships in v1

| # | Capability | Notes |
|---|---|---|
| F1 | Quick Response Rail — 5–6 objections always visible, hotkey-bound | The product's core. Works with everything else disabled. |
| F2 | Dual-stream audio capture (mic + far end) | Windows/WASAPI |
| F3 | Automatic call start/end detection from audio | No dialer integration |
| F4 | Local streaming transcription | Moonshine v2 default, Parakeet option |
| F5 | Objection detection via local embedding match | Promotes a rail item |
| F6 | LLM refinement of the promoted response | Optional; app fully functional without |
| F7 | `rebut` / `comply` / `disqualify` objection kinds | Compliance-critical — see doc 06 |
| F8 | Playbook editor — author, edit, reorder, enable/disable | |
| F9 | Supporting document import and retrieval | PDF, DOCX, MD, TXT |
| F10 | Playbook export / import as a portable file | Sharing across a team |
| F11 | Structured feedback: 👍 / 👎 with reason code | |
| F12 | Provider selection: local, OpenRouter, Anthropic, OpenAI, Ollama, LM Studio | |
| F13 | Local-only mode with enforced network isolation | |
| F14 | Default insurance cold-call playbook, preloaded | Works on first run, zero setup |
| F15 | Session history with transcripts and card outcomes | |
| F16 | Consent notice and recording disclosure helper | |

### 3.2 Explicitly out of scope for v1

CRM or dialer API integration · post-call summaries · deal scoring · supervisor/manager views · team sync or shared cloud playbooks · macOS build · mobile · call recording as a feature · multi-language · TTS or any voice output.

## 4. The three-layer response model

This is the central product concept and everything in the UI derives from it.

```
Layer 0   RAIL          Always visible. No model. 0 ms.
                        Top 5–6 objections, hotkeys 1–6.
                        Agent can act before the system knows anything.

Layer 1   PROMOTION     Embedding gate fires. ~700–900 ms.
                        The matching rail item illuminates and expands
                        to show the authored response and talk track.

Layer 2   REFINEMENT    LLM returns. ~1.5–2.5 s.
                        Body is replaced with a version tailored to the
                        prospect's actual words, plus a supporting fact.
                        Optional. Silently skipped if unavailable.
```

Each layer degrades to the one below it. Layer 0 has no dependencies at all. That property is what makes this shippable to live test users rather than a demo.

## 5. The default playbook

Ships preloaded. Derived from bought-internet-lead insurance calling, where the dominant screen-outs are distinct from generic B2B cold calling.

| Rail slot | Objection | Kind | Why it's here |
|---|---|---|---|
| 1 | "I never filled out a form" | rebut | The signature objection of bought leads. Aged leads especially. |
| 2 | "Not interested" | rebut | Universal, and usually reflexive rather than considered |
| 3 | "I already have coverage / an agent" | rebut | Highest-frequency real objection |
| 4 | "Now's not a good time" | rebut | Callback capture, not persuasion |
| 5 | "Who is this / where'd you get my number" | rebut | Trust repair; mishandled constantly |
| 6 | "Take me off your list" | **comply** | **Never rebutted.** Compliance action. |

Slots 7+ live below the fold: price objections, "just email me something," "I need to talk to my spouse," "how much is it" (price-first).

Full content in `playbooks/insurance-cold-call.yaml`.

## 6. Environment assumptions

Derived from the target user's actual setup.

| Assumption | Consequence for the build |
|---|---|
| Dialer runs in Chrome (Ricochet360 is browser-based) | System-mix capture by default; per-process is an advanced option |
| Next lead connects ~1 second after the last call ends | No idle gap to detect boundaries in — see architecture §5 |
| Progressive dialing, calls back-to-back for hours | A session contains dozens of calls; call is the unit, not session |
| Agent wears a headset | Mic and far-end are cleanly separated; no acoustic bleed to cancel |
| Windows 10 2004+ / Windows 11 | `WDA_EXCLUDEFROMCAPTURE` available; per-process loopback available |
| Consumer laptop, no discrete GPU | ASR and embeddings must be CPU-viable. Non-negotiable. |
| Agent may screen-share with a manager | Overlay must be excludable from capture |

## 7. Success criteria for the v1 test

Riposte v1 succeeds if, across ten agents over two weeks of live calling:

| Metric | Target | Measured by |
|---|---|---|
| Rail used without AI assist | > 15 uses/agent/day | Hotkey and click telemetry |
| Detection precision | > 0.75 | Feedback: `not_an_objection` rate |
| Detection recall on the six defaults | > 0.60 | Eval fixtures + manual review |
| Time to Layer 1 (p50) | < 1000 ms | `cards.latency_gate_ms` |
| Time to Layer 2 (p50) | < 2000 ms | `cards.latency_llm_ms` |
| Agent keeps it running voluntarily | > 70% of sessions | Session start telemetry |
| Crashes or audio dropouts | 0 | Crash reports |

The first row matters most. If agents use the rail and ignore the AI, that is a **successful v1** — it means the product is useful and the intelligence layer needs tuning. If agents disable it entirely, nothing else matters.

## 8. Non-goals worth stating out loud

- **Not a script reader.** Cards are bullets and one next question, never a paragraph to recite. An agent reading verbatim sounds worse than an agent improvising.
- **Not a compliance product.** It helps with DNC handling but it is not a TCPA compliance system and must not be marketed as one.
- **Not a recorder.** Recording is off by default and stays that way.
- **Not a coach for the agent's own speech.** v1 detects on the prospect stream only.

## 9. Deferred

Parked deliberately. Add here rather than building.

- macOS implementation (abstraction exists from day one)
- Rep-stream analysis: talk ratio, filler words, monologue detection
- Multi-objection stacking ("it's expensive *and* the timing is bad")
- Team playbook sync
- Dialer webhooks for authoritative call boundaries
- Post-call summary and disposition suggestion
- Sentiment and buying-signal detection
- Automatic trigger-phrase mining from past transcripts
