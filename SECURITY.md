# Security and privacy reporting

## Report privately

Open a [private security advisory](https://github.com/that1guy15/salesHero/security/advisories/new).
Do not open a public issue, and do not attach real material to one.

**Never include in any public channel:** call audio, transcripts, prospect names
or phone numbers, a real removal request, API keys, or a database file. This
repository is public, and in roughly a dozen US states an unconsented recording
is a criminal matter rather than a policy violation.

## What is in scope

Riposte runs entirely on the agent's machine. There is no server, no
multi-tenant component, and no telemetry leaving the device. The interesting
surface is therefore local:

- Anything that causes a **removal request to receive a rebuttal**. This is the
  highest-severity class of defect in the project, ranked above memory safety —
  it coaches a user toward a TCPA violation with the authority of software. See
  `docs/06-COMPLIANCE.md` §3.
- **Text escaping the machine that should not.** Redaction bypass, local-only
  mode making an outbound connection, a transcript in a crash report, a secret
  in a playbook export.
- **Audio written to disk** without explicit opt-in.
- **Credentials** anywhere other than the OS credential store.
- Retention failures: transcripts surviving their TTL, or a purge that does not
  cascade to `cards.window_text`.
- Memory safety, injection, or privilege issues in the desktop app.

## What is out of scope

- That the overlay can be photographed. `SetWindowDisplayAffinity` is not a
  security feature and Microsoft says so; we do not claim otherwise.
- That Riposte does not scrub DNC lists. It never claimed to — see
  `docs/06-COMPLIANCE.md` §6.
- Findings that require an attacker who already has code execution as the user.

## Response

Acknowledged within 5 working days. A confirmed compliance defect blocks the
next release regardless of schedule.
