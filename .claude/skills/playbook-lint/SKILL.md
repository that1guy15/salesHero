---
name: playbook-lint
description: Validate and review a playbook YAML — schema, compliance rules, and whether the card copy is any good. Use when authoring, editing or reviewing playbooks/*.yaml, adding objections or triggers, or before shipping playbook content to agents.
---

# Playbook lint

Two passes. The first is mechanical and the parser does most of it. The second
is the one that decides whether an agent keeps the app running.

## Pass 1 — structure

```bash
cargo run -q -p riposte-cli -- playbook import <path> && \
cargo run -q -p riposte-cli -- rail
```

Import refuses the whole file on any of these, naming the failing entry:

- `schema_version` newer than the build supports
- duplicate objection `id`
- `rail_slot` outside 1–6, or claimed twice
- unknown `kind` (must be `rebut`, `comply`, or `disqualify`)
- an `enabled` objection with no trigger phrases

Then check by eye:

- **`label` ≤ 28 characters.** It is read in peripheral vision by someone
  mid-sentence. Longer labels wrap and stop being recognisable by shape.
- **`id` values are stable.** They are referenced by feedback and card history.
  Editing a label must never change an id.
- **`threshold`** in 0.50–0.85. Below 0.50 fires on everything; above 0.85
  effectively disables the entry.
- **`doc_refs`** resolve to an id in the `documents:` block.

## Pass 2 — compliance

**Every `comply` entry:**

- Its `response` and `talk_track` contain **no rebuttal, no alternative, no
  "before you go", no reason to stay on the line**. Confirm the request, end the
  call. The editor warns rather than blocks — an agency may have legitimate
  wording — but the warning is explicit and you should raise it here.
- `allow_refine` is irrelevant. It is forced off on import whatever the file
  says. Do not "fix" a file by setting it.
- Its `threshold` should sit *below* the rebuttable entries. `obj_dnc` ships at
  0.55 against 0.60–0.66 because a false positive here costs nothing and a false
  negative is a TCPA problem.

**Any `rebut` entry whose guardrails contain a repeat obligation** — "if they
say it twice", "one attempt only", "treat a second X as a removal request" —
**must carry `escalate_after_repeats`.** A guardrail expressed only as prose is
a request an LLM may ignore. `docs/DECISIONS.md` D8. The shipped playbook arms
`obj_no_form` and `obj_not_interested` at 2.

**Trigger phrases must not cross entries.** If a phrase would reasonably match
two objections, the gate will pick one on score and it will be arbitrary. Run
the `gate-calibration` skill to see the actual separation before shipping.

## Pass 3 — is the copy any good

This is the part that determines whether agents keep it running. `01-PRD.md` §8:
*"Not a script reader. Cards are bullets and one next question, never a paragraph
to recite. An agent reading verbatim sounds worse than an agent improvising."*

- **`response` is direction, not dialogue.** "Don't argue about the form. Concede
  immediately, then ask one question" — what to *do*. Not words to say.
- **`talk_track` is words to say, and each line stands alone.** An agent reads
  one, not all three. Lines that only work in sequence do not work at all.
- **Three talk-track lines maximum.** More cannot be scanned mid-sentence.
- **It must sound like a person.** Read it aloud. "I'm not asking you to switch
  today. I'm asking to be the second quote" survives that test; anything with
  "leverage", "solutions", or "reach out" does not.
- **Concede before redirecting.** Every good entry in the shipped playbook opens
  by agreeing with the prospect. Cards that argue lose calls.
- **No promises the agent cannot keep.** No rate claims before quoting, no
  disparaging a named carrier, no invented prior relationship.

## Domain check

For content questions — whether an objection is real, how agents actually handle
it, what a bought lead is — use the `insurance-domain` skill. Do not invent
objection copy from general knowledge of sales; this is a specific market with
specific screen-outs.
