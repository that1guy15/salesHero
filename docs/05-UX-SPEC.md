# 05 — UX Specification

## 1. The design constraint

This interface is read in peripheral vision by someone who is talking to a stranger and cannot afford to look away. That single constraint decides everything: type size, contrast, motion, information density, and how much the interface is allowed to change while the agent is mid-sentence.

Three rules follow.

**Recognition, not reading.** Rail items are identified by position and color, not by parsing text. An agent who has used the app for a week should hit `3` for "already covered" without looking. Position is therefore stable — the rail never reorders itself during a call.

**Movement means something.** The only things that animate are things the agent needs to notice. No hover flourishes, no ambient motion, no transitions for their own sake. When something moves, it is because a card was promoted. Everything else is still.

**Degradation is visible but quiet.** When ASR or the LLM is unavailable, the agent should be able to tell at a glance — but it must not look like an error state, because Layer 0 is a perfectly good place to be.

---

## 2. Visual system

The reference point is instrumentation — an aircraft glass cockpit, a mixing desk — not a SaaS dashboard. Dark, high contrast, color used strictly to encode state.

### Palette

| Token | Hex | Use |
|---|---|---|
| `--surface` | `#12151A` | Overlay background |
| `--surface-raised` | `#1B2028` | Card body, rail item |
| `--edge` | `#2A313C` | Hairlines, dividers |
| `--text` | `#E8EDF4` | Primary |
| `--text-dim` | `#8A94A3` | Labels, metadata |
| `--rebut` | `#4DA3FF` | Rebut objections — calm blue |
| `--comply` | `#FF6B4D` | Comply objections — arresting, unmissable |
| `--disqualify` | `#B98CFF` | Disqualify |
| `--live` | `#3ED598` | Call active indicator |

`--comply` is the only warm color in the system, and it is used for exactly one thing: an objection the agent must not argue with. That exclusivity is deliberate — orange on this screen means stop.

### Type

| Role | Face | Size | Weight |
|---|---|---|---|
| Rail label | Inter Tight | 15px | 600 |
| Card headline | Inter Tight | 17px | 600 |
| Card body | Inter | 15px / 1.45 | 400 |
| Talk track | Inter | 15px / 1.5 | 400, italic |
| Hotkey badge | JetBrains Mono | 12px | 500 |
| Metadata | Inter | 11px | 500, `--text-dim` |

Body text does not go below 15px. This is read at arm's length, off-axis, by people who are not all twenty-five.

### The signature element

**The hotkey badge.** A monospace numeral in a hairline square, sitting at the left edge of every rail item. It is the one piece of persistent chrome, and it is doing real work: it teaches the keyboard shortcut passively over the first few days of use, until the agent stops reading the labels entirely. When an item is promoted, the badge fills solid with the kind color — the number stays legible, so muscle memory holds even as the item changes appearance.

---

## 3. Overlay layout

380px wide, docked right by default, draggable, position remembered. Borderless, no taskbar entry, always on top.

```
┌────────────────────────────────────────┐
│ ● LIVE      02:14        ⋯   ─   ✕     │  32px  status bar
├────────────────────────────────────────┤
│                                        │
│  ┌──────────────────────────────────┐  │
│  │ ▌1▐  Never filled out a form     │  │  ← promoted: expanded
│  │                                  │  │
│  │  Denies submitting a form —      │  │  headline (layer 2)
│  │  likely an aged lead             │  │
│  │                                  │  │
│  │  • Concede immediately           │  │  body
│  │  • Name the comparison site      │  │
│  │  • Offer removal, mean it        │  │
│  │                                  │  │
│  │  "Were you looking at rates      │  │  next question
│  │   online a few months back?"     │  │
│  │                                  │  │
│  │  👍  👎          lead-sources.pdf │  │  feedback + citation
│  └──────────────────────────────────┘  │
│                                        │
│   2   Not interested                   │  ← neutral rail items
│   3   Already have coverage            │
│   4   Not a good time                  │
│   5   Who is this?                     │
│   6   Take me off your list            │  ← --comply color always
│                                        │
├────────────────────────────────────────┤
│  ⌄ more (7)          ◐ local  ▪ ASR    │  28px  footer
└────────────────────────────────────────┘
```

### Behavior

**Rail items never reorder.** Slot 1 is always slot 1. Promotion expands an item in place; the ones below shift down. This is the only layout change permitted during a call.

**One expanded item at a time.** A second detection collapses the first. Two open cards is more than can be read while talking.

**Manual invocation via hotkey `1`–`6`** expands that item exactly as detection would, recorded as `origin: manual_hotkey`. Clicking does the same with `manual_click`. This path has no dependency on anything beyond the playbook.

**`Esc` collapses** the expanded item back to neutral.

**`Ctrl+Shift+M`** toggles capture mute globally. Status bar dot goes hollow.

**Auto-collapse after 90 seconds** or on `call:ended`, whichever first. Configurable.

### Layer transitions

| From | To | Treatment |
|---|---|---|
| Neutral → promoted | Layer 0 → 1 | Height animates 140ms ease-out. Badge fills. No fade — fades read as sluggish at this size. |
| Promoted → refined | Layer 1 → 2 | Body crossfades 100ms. Headline slides in from 4px. Height animates if needed. **The talk track does not move** — the agent may be reading it. |
| Promoted → demoted | model said no | Collapse over 200ms. No error state, no explanation. It quietly stops being there. |

The rule about the talk track is the important one. Replacing text under someone's eyes mid-sentence is worse than showing them a slightly stale response.

### Comply cards are different

When a `comply` objection fires, it does not expand like the others. It **takes the whole panel**:

```
┌────────────────────────────────────────┐
│ ● LIVE      02:14        ⋯   ─   ✕     │
├────────────────────────────────────────┤
│                                        │
│   ▌6▐   REMOVAL REQUESTED              │  --comply, 20px
│                                        │
│   Do not rebut. Do not offer           │
│   anything else.                       │
│                                        │
│   "Absolutely — I'm removing you       │
│    right now. You won't hear from      │
│    us again."                          │
│                                        │
│   ┌──────────────────────────────┐     │
│   │   Log removal and end call   │     │  primary action
│   └──────────────────────────────┘     │
│                                        │
└────────────────────────────────────────┘
```

No feedback buttons — this card is not a suggestion to be rated. No citations. No refinement. It stays until dismissed explicitly or the call ends. The rail is hidden underneath it, because the correct number of alternatives to offer at this moment is zero.

---

## 4. States

| State | Status bar | Rail |
|---|---|---|
| No session | `○ IDLE` grey | Visible, hotkeys live |
| Session, between calls | `○ WAITING` grey | Visible, hotkeys live |
| Ringing | `◐ RINGING` amber | Visible, detection suppressed |
| Call active | `● LIVE 02:14` green | Visible, detection armed |
| Muted | `◌ MUTED` grey, hollow | Visible, hotkeys live |
| Capture failed | `⚠ NO AUDIO` red | Visible, hotkeys live |

Note the last row. Audio capture failing does not disable the product — it drops it to Layer 0. The error is stated plainly in the status bar and the agent keeps working.

### Engine status footer

Three dots: ASR, embeddings, LLM. Filled = ready, hollow = loading, absent = unavailable/disabled. `◐ local` appears when local-only mode is on. Hovering names the engine and its measured p50.

This is the honest-degradation surface. An agent should be able to tell that the LLM is off without it feeling like something is broken.

---

## 5. Console window

Separate normal window, opened from the `⋯` menu. Not visible during calls.

| Tab | Contents |
|---|---|
| **Playbook** | Objection list. Drag to assign rail slots 1–6. Inline edit of label, response, talk track, triggers. Kind selector with an explanation of `comply`. Threshold slider showing both authored and learned values, with reset. |
| **Documents** | Import, list, per-document chunk count, link to objections. |
| **Calls** | Session → call tree. Transcript with cards inline at their timestamps. Feedback editable after the fact. |
| **Insights** | Detections per objection, precision from feedback, rail usage vs detected usage, p50 latency by model. |
| **Settings** | Capture source and mic. ASR engine. Provider and model, with the OpenRouter catalog searchable and showing price and context length. Local-only toggle. Screen-share exclusion toggle. Consent notice text. |

The Insights tab exists because the PRD's primary success metric is rail usage. The user should be able to see it too — it is the number that tells an agency owner whether their playbook is any good, independent of whether the AI is working.

---

## 6. Screen capture exclusion

The overlay calls `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` in the Tauri setup hook.

`WDA_EXCLUDEFROMCAPTURE` (0x00000011) was introduced in Windows 10 version 2004. The window renders to the physical monitor and does not appear anywhere else. The exclusion happens in the Desktop Window Manager's compositor rather than in the capturing application, so the window is absent from Zoom, Teams, and Meet screen shares, from OBS, and from the Snipping Tool.

Microsoft is explicit that this is not a security or DRM feature and offers no guarantee against, for example, someone photographing the screen. Do not overclaim it in the UI.

**Ship it as a visible toggle, defaulted on**, labeled "Hide from screen sharing," with a one-line explanation. The legitimate use is obvious — an agent screen-sharing a quote with a prospect or a manager should not be broadcasting their objection cards — but a capability that hides a window from monitoring deserves to be stated plainly rather than buried.

Re-apply the affinity on every `show()`. Tauri's `setAlwaysOnTop` does not survive `hide()`/`show()` cycles, and neither reliably does the display affinity.

---

## 7. First run

Four screens, skippable after the first.

1. **What this does** — one paragraph, one screenshot of the rail. States plainly that it listens to call audio.
2. **Recording disclosure** — explains two-party consent, offers a disclosure script the agent can use, links to `docs/06-COMPLIANCE.md`. Requires an explicit acknowledgment checkbox. Not skippable.
3. **Audio setup** — pick mic, pick far-end source, live level meters for both. The agent must see both meters move before continuing; this catches the most common setup failure before it becomes a support ticket.
4. **Your rail** — shows the six preloaded insurance objections, invites editing but does not require it. "You can change these any time — try it on a few calls first."

The app is usable at the end of screen 4 with no model configured, no API key, and no documents imported. That is the point.

---

## 8. Accessibility floor

Not negotiable, and cheap to do now.

- All rail actions reachable by keyboard; visible focus ring in `--rebut`
- `prefers-reduced-motion` disables all transitions; layers swap instantly
- Contrast ≥ 7:1 for body text, ≥ 4.5:1 for dim text
- Overlay scales 100–150% without reflow breakage
- Color is never the only signal: `comply` cards also carry the word "REMOVAL REQUESTED" and a distinct layout
