# Governance

Riposte is a maintainer-led pre-release project. The accountable owner and sole
GitHub maintainer is `@that1guy15`.

## Roles

- **Owner / lead maintainer** — product scope, release authorisation, security
  coordination, and appointment of reviewers.
- **Contributors** — DCO-signed changes with tests, documentation, and synthetic
  fixtures.
- **Review agents** — `spec-auditor` and `rust-reviewer` supply recorded
  engineering review while there is one human maintainer. Their findings are
  evidence, not approval.

### Solo-maintainer rule

With one maintainer, required human approval is zero, because self-approval is
impossible and a rule that cannot be satisfied gets bypassed. Everything else
still applies: owner-authored work goes through a pull request, `required-ci`
must be green, and recorded review evidence is attached. The owner may merge
their own pull request once checks pass.

**When a second maintainer is appointed, required CODEOWNER approval is turned
on and this paragraph is deleted.**

## Authority

Read and obey, in order:

1. `docs/01-PRD.md` — what ships and what does not. §3 is the scope fence.
2. `docs/02-ARCHITECTURE.md` — the shape everything else follows from.
3. `docs/06-COMPLIANCE.md` — legal obligations. Outranks product preference.
4. `docs/DECISIONS.md` — where implementation deviates from the specs, and why.
   On any question of *what we chose*, this outranks the specs.
5. `docs/PROJECT-STATUS.md` — on any question of *what exists*, this outranks
   the specs.
6. `AGENTS.md` — engineering workflow and safety boundary.
7. Project skills under `.claude/skills/` — task procedure.

A skill never overrides a specification. A specification never overrides a
compliance obligation.

## Changing scope

`docs/01-PRD.md` §3 is the fence and §9 is where additions wait. Adding a
capability to §3 is an owner decision recorded in `docs/DECISIONS.md`, not a
side effect of a pull request that seemed like a good idea.

Scope creep is the named failure mode for a project of this shape. A proposal
with no field evidence goes to §9. That is not a rejection.

## Compliance changes

Any change touching the five `comply` enforcement layers, the redaction path,
local-only enforcement, or retention requires:

- the `compliance-review` skill run against the diff, with findings recorded;
- `riposte-evals check` green, including the removal-request floor;
- a `docs/DECISIONS.md` entry when behaviour changes at all.

**A compliance regression is a P0 and blocks release regardless of schedule.**

## Releases

Engineering completion, release-candidate readiness, and distribution to agents
are three separate decisions.

- Tags matching `v*.*.*` build installers and open a **draft** release. A draft
  is not a release.
- Publishing requires the `docs/WINDOWS-TESTING.md` checklist completed on real
  hardware, and a signed installer.
- Distribution to live agents additionally requires the consent flow reviewed by
  counsel, per `docs/06-COMPLIANCE.md`.

The owner authorises each step explicitly. No workflow publishes on its own.

## Security

Never open a public issue containing a vulnerability, a credential, or real call
content. See [SECURITY.md](SECURITY.md).
