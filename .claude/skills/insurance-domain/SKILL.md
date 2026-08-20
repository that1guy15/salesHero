---
name: insurance-domain
description: Ground product and engineering decisions in how P&C insurance cold calling actually works — bought leads, the objection taxonomy, TCPA and DNC obligations, and what makes a card useful to a producing agent. Use when writing or reviewing playbook content, judging whether a feature matches agent reality, or answering questions about the end customer.
---

# The customer, and the call

Riposte's user is a producing insurance agent working bought internet leads on a
progressive dialer. Everything in the product derives from what that job is
actually like. This is the reference for decisions that need that context.

**Line of business: P&C — auto and home.** Final expense and Medicare differ
materially; see the last section before assuming anything transfers.

## The job

60–120 dials a day. Most do not connect. The ones that do are decided in the
first fifteen seconds — the prospect says something, and the agent either has a
response ready or the call is over.

The dialer (Ricochet360, Kixie, JustCall) runs in Chrome and connects the next
lead about **one second** after the last call ends. There is no gap to think in,
no gap to look something up in, and no idle period for software to detect a call
boundary in. The agent wears a headset and is talking almost continuously for
hours.

This single fact — one second between calls — is why the rail is always visible
and hotkey-bound rather than something you open, and why the far-end buffer keeps
ten seconds of pre-roll.

## What a bought internet lead is

The prospect filled in a rate-comparison form on a third-party site — often not
the agency's own, often months ago, often without registering that they were
requesting calls. The lead was sold, sometimes several times, sometimes to
several agencies at once. An **aged lead** is one resold weeks or months later.

Three consequences the product is built around:

1. **"I never filled out any form" is usually sincere.** They did fill something
   in; they do not remember it, and it did not carry this agency's name. An
   agent who argues the point loses. The correct move is to concede immediately,
   give a concrete reason they might not remember, and ask one question.
2. **The prospect may have taken six of these calls today.** Their opener is
   reflex, not a considered position. "Not interested" arrives before they know
   what is being sold.
3. **Trust is the scarce resource.** "Where did you get my number" is not
   hostility, it is a reasonable question, and hedging confirms the worst
   reading. Answer it directly and completely, including the lead source.

## The objection taxonomy

The six that earn a rail slot, and why each is there:

| Objection | Category | What it really is |
|---|---|---|
| "Never filled out a form" | screening | The signature objection of bought and aged leads |
| "Not interested" | screening | Reflex, arriving before the pitch |
| "Already have coverage" | coverage | The highest-frequency *real* objection — and a buying signal |
| "Not a good time" | timing | A callback to capture, not a mind to change |
| "Who is this / where'd you get my number" | trust | Trust repair; mishandled constantly |
| **"Take me off your list"** | **compliance** | **Not an objection. A legal instruction.** |

Below the fold: price-first ("how much is it"), "just email me something",
"I need to talk to my spouse", "that's too expensive".

### How experienced agents actually handle them

- **Already covered is good news.** They are a proven buyer who has already
  decided they need the product. Never attack the incumbent carrier or agent by
  name — it insults the prospect's past judgement. Find the renewal date and ask
  when anyone last actually shopped it. Ask to be the second quote, not the
  switch.
- **Not a good time: believe them.** Do not pitch through it. Get a *specific*
  time — "sometime next week" never happens, "Thursday at 2" sometimes does.
- **Price-first is engagement, not an objection.** But a number without rating
  information is a guess that has to be walked back later, which costs more than
  the delay. Trade information for information.
- **"Just email me something" is usually a polite exit.** Agree to send, but
  attach a commitment to it, or the email is the end of the call.
- **Spouse objections are legitimate.** Do not close around an absent partner.
  Get both on the next call.

## Compliance, in the terms an agent meets it

Not legal advice. Engineering context. Anyone deploying on live calls needs their
own counsel — insurance telemarketing sits under TCPA *and* state insurance
regulation, and the specifics vary by state and by line.

**A removal request is not an objection.** "Take me off your list", "stop calling
me", "put me on your do-not-call list" carry legal weight under the TCPA.
Suggesting a rebuttal is coaching a violation, with the authority of software.
This is why `kind: comply` exists, why it is enforced five ways, and why the DNC
entry's threshold ships *lower* than everything else — a false positive costs
nothing, a false negative is a violation.

**A second refusal is a withdrawal of consent.** An agent pushing past "no" twice
is the behaviour the whole compliance design exists to prevent. Enforced in code
via `escalate_after_repeats`, not asked for in prose.

**Recording law is not a formality.** Roughly a dozen states require all-party
consent, several of them large insurance markets, and violation is criminal
rather than a policy matter. Riposte does not record by default — ring buffers,
discarded — but transcribing the far party is close enough that treating it as
recording is the only defensible posture.

**Riposte is not a DNC system.** It does not scrub lists, honour registries, or
prevent a single dial. The removal log exists so the agent can enter it in their
real dialer. Never let the product imply otherwise; inviting reliance on a system
that cannot deliver is worse than offering nothing.

## What makes a card useful

The interface is read in peripheral vision by someone talking to a stranger who
can hang up. That constraint decides everything.

- **Direction, not dialogue.** Tell the agent what to *do*. An agent reading a
  script verbatim sounds worse than an agent improvising badly, and prospects
  hear it instantly.
- **Bullets and one next question.** Never a paragraph.
- **Each talk-track line stands alone.** The agent reads one, not the set.
- **Concede first.** Every card that works opens by agreeing with the prospect.
- **The agent will rewrite these, and should.** Responses in someone else's
  voice are worse than no card. The product's job is to make editing trivial,
  not to be right out of the box.

## Where this does not transfer

- **Final expense / life.** Aged lists, elderly prospects, and "he passed away"
  is a frequent and high-stakes screen-out with no entry in the current playbook.
  Vulnerable-person handling becomes a product requirement, not a nicety.
- **Medicare Advantage / Part D.** CMS marketing rules require recording the
  *entire* sales call and retaining it for years — the direct opposite of this
  product's ring-buffer stance. Selling into that market needs a
  compliance-recording mode and a separate legal review. Treat it as a gate, not
  a feature request.
- **Commercial / B2B.** Different gatekeeper dynamics entirely. The screen-outs
  in this playbook are consumer screen-outs.
