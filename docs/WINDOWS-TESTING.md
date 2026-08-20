# Testing the rail overlay on Windows

M1.7 ships **Layer 0 only** — the Quick Response Rail. No audio, no
transcription, no AI, no network. That is deliberate: `01-PRD.md` §7's headline
metric is *rail use without AI assist*, and it needs none of the pipeline.

Everything below is manual. CI compiles on `windows-latest`; it does not verify
window management, global hotkeys, or capture exclusion. **An agent reporting
M1.7 done from a Mac has verified compilation and nothing else.**

---

## 1. Machine

| Requirement | Why |
|---|---|
| Windows 10 version 2004+ or Windows 11 | `WDA_EXCLUDEFROMCAPTURE` was introduced in 2004 |
| Chrome installed | The dialer is browser-based; the hotkey conflict test needs it |

Check your build: `Win+R` → `winver`. You need **2004 / build 19041** or newer.

---

## 2. Prerequisites

Run each block in **PowerShell**. Reopen the terminal after installing Rust so
`PATH` picks it up.

### 2.1 Visual Studio C++ build tools

Tauri and `rusqlite`'s bundled SQLite compile C. The **MSVC** toolchain is
required — `x86_64-pc-windows-gnu` is not what these build against.

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e
```

In the Visual Studio Installer that opens, select **Desktop development with
C++** and install. This is a large download; start it first.

### 2.2 WebView2

Tauri renders the UI in WebView2. Windows 11 ships it. On Windows 10:

```powershell
winget install --id Microsoft.EdgeWebView2Runtime -e
```

### 2.3 Rust

```powershell
winget install --id Rustlang.Rustup -e
rustup default stable
rustup component add rustfmt clippy rust-analyzer
```

### 2.4 Node

```powershell
winget install --id OpenJS.NodeJS.LTS -e
```

### 2.5 Verify

```powershell
rustc --version   # 1.82 or newer
cargo --version
node --version    # 18 or newer
npm --version
```

---

## 3. Build and run

```powershell
git clone https://github.com/that1guy15/salesHero.git
cd salesHero
git checkout m1.7-rail-overlay

# Headless checks first — these should pass before you look at a window.
cargo test --workspace

# Frontend dependencies (once).
npm --prefix apps/desktop/ui install

# Run the overlay.
cargo run -p riposte-desktop
```

First build pulls the Tauri dependency graph and takes **10–20 minutes**.
Subsequent builds are seconds.

The overlay appears as a 380px borderless panel, always on top, with no taskbar
entry. On first launch it shows the consent screen, then imports the bundled
insurance playbook — **no setup, no API key, no model download**.

For frontend hot-reload while iterating on the UI:

```powershell
npm --prefix apps/desktop/ui install -g @tauri-apps/cli   # or: npx tauri dev
npx --prefix apps/desktop tauri dev
```

### Where its data lives

```
%APPDATA%\com.riposte.overlay\riposte.db
```

Delete that file to get a clean first-run. Nothing else is written anywhere.

---

## 4. The test checklist

Mark each **pass**, **fail** with what happened, or **not run** with why. Do not
mark a box passed because the code looks correct — that is the entire reason
this list is manual.

### 4.1 It starts and it works

- [ ] Consent screen appears on first launch and **Continue is disabled** until
      the checkbox is ticked
- [ ] After accepting, six rail items appear in slots 1–6, in order
- [ ] Slot 6 (“Take me off your list”) is **orange**, every other slot is blue
- [ ] Clicking a rail item expands it in place; the items below shift down
- [ ] Clicking a second item collapses the first — only one is ever open
- [ ] `Esc` collapses the expanded item
- [ ] Restart the app: the consent screen does **not** appear again

### 4.2 Hotkeys — the important one

The global binding is **`Ctrl+Alt+1` … `Ctrl+Alt+6`**, not bare digits. Bare
digits registered globally would capture them system-wide and the agent could
not type a `3` into their dialer. See `docs/DECISIONS.md` D18.

- [ ] `Ctrl+Alt+3` expands “Already have coverage” **while Chrome has focus** —
      click into a Chrome address bar first, then press it without touching the
      overlay
- [ ] Typing `3` into a Chrome text field types a `3` and does **not** trigger
      the overlay
- [ ] All six of `Ctrl+Alt+1` … `Ctrl+Alt+6` fire the matching slot
- [ ] **With Ricochet360 open and a call connected**, `Ctrl+Alt+<n>` does not
      collide with anything in the dialer. *This is the one item that cannot be
      checked anywhere but here.* If it collides, say which key — the binding is
      a one-line change.

### 4.3 Screen-share exclusion

The footer chip reads `hidden from share` when active. Click it to toggle.

- [ ] Start a Zoom or Teams meeting, share your **entire screen**, and look at
      the self-preview: the overlay is **absent**
- [ ] Repeat with OBS “Display Capture”: absent
- [ ] Repeat with `Win+Shift+S` (Snipping Tool): absent
- [ ] Toggle the chip off — the overlay now **appears** in all three
- [ ] Toggle back on, then minimise and restore the window: still absent
      *(Tauri drops both always-on-top and display affinity on a hide/show cycle,
      so they are re-asserted on every show — this checks that it works)*

> This is not a security feature and Microsoft says so. It will not stop someone
> photographing the screen. Do not describe it to agents as more than it is.

### 4.4 Window behaviour

- [ ] Drag the status bar to move the window
- [ ] The overlay stays on top of Chrome, Ricochet360, and Zoom
- [ ] It has **no taskbar entry** and does not appear in `Alt+Tab`
- [ ] Resize to 340px wide: nothing overlaps or clips
- [ ] Windows display scaling at 150%: still readable, still no clipping

### 4.5 Editing — the thing agents will actually do

- [ ] Expand a card, click **“this isn’t how I say it — edit”**
- [ ] Change the wording, click **Save**
- [ ] Close the app completely and reopen: the edit is still there
- [ ] While a textarea has focus, pressing `3` types a `3` and does not fire a card

### 4.6 Practice mode

- [ ] Click **practice** in the status bar — the indicator turns blue
- [ ] Drill `Ctrl+Alt+1` through `6`; the cards expand normally
- [ ] The usage counter in the footer does **not** increase
      *(practice presses are deliberately not counted — counting drills would
      destroy the only number that tells us whether the product works)*
- [ ] Turn practice off; a real press **does** increase the counter

### 4.7 The compliance card

- [ ] Press `Ctrl+Alt+6`. The card takes the **whole panel** and the rail is
      hidden — there are no other options visible
- [ ] It says **REMOVAL REQUESTED** in words, not just in colour
- [ ] There are **no thumbs up/down buttons** — this card is not a suggestion
- [ ] The fine print states that Riposte does not maintain a DNC list
- [ ] Click **Log removal and end call**, then Dismiss

### 4.8 The architectural bet

This is the one that matters most. Layer 0 must survive everything.

- [ ] **Disconnect the network entirely.** Restart the app. The rail works.
- [ ] **Unplug the headset / disable all audio devices.** Restart. The rail works.
- [ ] There is no model anywhere on disk and none is downloaded

If any of those three fails, the core product claim is wrong and that outranks
every other item on this list.

### 4.9 A real dialing session

- [ ] Run it for a **4-hour dialing session**
- [ ] No crash
- [ ] Memory does not grow — check `riposte-desktop.exe` in Task Manager at the
      start and the end
- [ ] The overlay never steals focus mid-call
- [ ] Afterwards, the footer counter reflects roughly how often you actually
      used it

---

## 5. What to report back

For the agents running it, three questions matter more than the checklist:

1. **How many times a day did you press a hotkey?** The footer counter is the
   number. `01-PRD.md` §7 wants **> 15 per agent per day**. It is the metric the
   whole product is judged on.
2. **Did you edit the cards?** If not, why — were the defaults fine, or was
   editing too awkward to bother with?
3. **Did you keep it running?** If you closed it, at what point in the day, and
   what made you close it?

If agents use the rail and never ask for the AI, that is a **successful v1**.

Known limitations to state up front, so nobody reports them as bugs:

- It does not listen to calls. Detection is not built yet, and measurement
  (`DECISIONS.md` D16) says the current approach does not work — that is the
  next product decision, not a bug.
- There is no console, no settings window, no session history.
- The name is a working title.
