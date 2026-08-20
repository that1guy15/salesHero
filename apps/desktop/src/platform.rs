//! Everything the overlay needs that only one operating system provides.
//!
//! `AGENTS.md` names `crates/audio` as the only *crate* permitted `#[cfg(windows)]`,
//! and the reason it gives is that the pipeline must compile and test on Linux
//! so CI stays cheap and a macOS port is one crate rather than a rewrite. A
//! window manager cannot be abstracted away the same manner — `05-UX-SPEC.md` §6
//! calls `SetWindowDisplayAffinity` by name.
//!
//! So the rule is honoured in the way it is meant: every platform call in the
//! desktop app lives in this file, behind functions that compile to a no-op
//! elsewhere. Nothing above this module knows which OS it is on. See
//! `docs/DECISIONS.md` D17.

/// Hides the window from screen capture — Zoom, Teams, OBS, Snipping Tool.
///
/// `WDA_EXCLUDEFROMCAPTURE` (0x11), Windows 10 2004+. The exclusion happens in
/// the compositor rather than in the capturing application, so it holds against
/// anything going through DWM.
///
/// Microsoft is explicit that this is not a security or DRM feature and offers
/// no guarantee against, for example, photographing the screen.
/// `05-UX-SPEC.md` §6: do not overclaim it in the UI.
///
/// Returns whether the platform did anything, so the UI can report honestly
/// rather than showing a toggle that silently does nothing.
pub fn set_excluded_from_capture(window: &tauri::WebviewWindow, exclude: bool) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
        };
        let Ok(handle) = window.hwnd() else {
            return false;
        };
        let affinity = if exclude {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };
        // SAFETY: `hwnd()` hands back a live window handle owned by Tauri, and
        // the call does not outlive this function.
        unsafe { SetWindowDisplayAffinity(HWND(handle.0 as _), affinity).is_ok() }
    }
    #[cfg(not(windows))]
    {
        let _ = (window, exclude);
        false
    }
}

/// Whether this build can hide the overlay from screen capture at all.
pub const fn supports_capture_exclusion() -> bool {
    cfg!(windows)
}

/// Re-asserts window state that Tauri does not preserve across `hide()`/`show()`.
///
/// Both always-on-top and the display affinity are dropped on a hide/show
/// cycle — `AGENTS.md` lists this under "things that will bite you". Call this
/// on every show, not just at startup.
pub fn reassert_window_state(window: &tauri::WebviewWindow, exclude_from_capture: bool) {
    let _ = window.set_always_on_top(true);
    if exclude_from_capture {
        set_excluded_from_capture(window, true);
    }
}
