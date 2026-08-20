---
name: windows-verify
description: Run the manual Windows verification checklists for audio capture, the overlay, and the installer. Use on the Windows 11 machine when closing M1.7, M4, M6 or M9 — none of these can be verified from macOS or in CI.
---

# Windows verification

CI compiles on `windows-latest`. It does not verify audio behaviour, window
management, or install. Those need the physical Windows 11 box, and every item
below is manual.

**An agent reporting M4 or M6 done from a Mac has verified compilation and
nothing else.** Say so explicitly if that is the situation.

## Before you start

- Windows 10 2004+ or Windows 11 (needed for `WDA_EXCLUDEFROMCAPTURE` and
  per-process loopback)
- A real headset, not laptop speakers — acoustic bleed invalidates the
  attribution checks
- Chrome, since the dialer is browser-based
- MSVC toolchain. The `x86_64-pc-windows-gnu` std installed on the Mac is not
  what Tauri and WASAPI build against.

## M1.7 — rail overlay

- [ ] `cargo tauri dev` — the overlay appears, borderless, always on top
- [ ] `Ctrl+Alt+3` expands "Already have coverage" **while Chrome has focus**
- [ ] typing `3` into a Chrome text field does **not** trigger the overlay —
      this is the whole reason the hotkey is not a bare digit
- [ ] edit a card in place, restart the app, the edit persisted
- [ ] the overlay is absent from a Zoom screen share, an OBS capture, and the
      Snipping Tool
- [ ] always-on-top survives `hide()` then `show()` — Tauri does not preserve
      it, so it must be re-asserted, and so must the display affinity
- [ ] drag the overlay, restart, position restored
- [ ] `riposte stats` counts the manual uses
- [ ] **kill the network and delete every model — the rail still works.** This
      is the product.

## M4 — audio capture

- [ ] mic and system audio arrive on **separate** streams with correct
      attribution; speak into the mic and play audio, confirm no crossover
- [ ] 60 minutes continuous with no dropouts and no memory growth — watch RSS
- [ ] per-process capture on Chrome with `include_tree = true` captures the
      dialer (Chrome renders audio in a child process, so without the tree flag
      you get silence — and with it you get every tab, which is why system-mix
      is the default)
- [ ] realtime callback verified allocation-free under a tracking allocator
- [ ] pre-roll returns 10 s of prior audio on demand
- [ ] mute stops delivery **without tearing down the stream**
- [ ] unplug the headset mid-session — no crash

Known constraints, do not rediscover: loopback works only in **shared mode**;
in per-process mode `get_device_period` returns nonsense and the period passed
to `initialize_client` is ignored.

## M5 — call boundaries

Needs a **recorded multi-call Ricochet360 session from the test agent**. This is
the long-lead item that blocks the riskiest milestone; if it has not been
requested yet, that is the finding.

- [ ] voicemail greetings never reach ACTIVE
- [ ] calls separated by ~1 s are detected as two calls, not one
- [ ] pre-roll drain places pre-detection utterances in the transcript with
      negative timestamps
- [ ] a fixture with 12 known calls segments within ±1

## M9 — release

- [ ] clean install on a fresh Windows 11 machine with no dev tools
- [ ] works with **no API key configured**
- [ ] uninstall removes everything including `%APPDATA%`
- [ ] 4-hour dialing session: no crash, no memory growth, no audio dropout
- [ ] all seven `AGENTS.md` invariants hold in the shipped build

Code signing has a multi-day to multi-week procurement lead time. If it has not
started, that is the finding — not something to discover at M9.

## Reporting

For each box: `pass`, `fail` with what happened, or `not run` with why. Do not
mark a box passed because the code looks correct. The entire point of this list
is that these behaviours cannot be reasoned about from source.
