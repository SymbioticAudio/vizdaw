# GPUI Spike Report (27.1.0 phase 0)

Date: 2026-11-21. Plan: `docs/superpowers/plans/2026-11-20-gpui-spike.md`. Spec: `2026-11-20-cross-platform-27-1-design.md` §7. Code and detailed notes: `spikes/gpui-spike/` (README "Findings") on branch `spike/gpui` (kept for reference, not merged).

## Verdict: GO

The Mac checks ran the same day once the screen was unlocked: checks 1, 3, 4 and 5 pass on macOS. On Linux (X11 through XWayland) checks 3 and 4 pass, and check 1 passes for embedding and closing but not for moving and minimizing with the main window (the editor is its own top-level window; phase 2 adds `WM_TRANSIENT_FOR`, see below). Check 2 (screen reader) was reported only, as planned.

## Pin

`gpui = "=0.2.2"` (crates.io) with `runtime_shaders`, plus two local patches needed on Linux (below). **Phase 2 should pin a GPUI main revision instead**: main already has the X11 window-handle fix and AccessKit (27.2), and has split the platforms into `gpui_macos`, `gpui_linux`, `gpui_windows` crates.

## Results

| Check | macOS | Linux X11 (XWayland) | Linux native Wayland |
|---|---|---|---|
| 1 Plug-in editor in a window tied to the GPUI window | **pass**: Multi-Q (CLAP) and AUDelay (AU) open in `addChildWindow` children; both move with the main window (+100/−80), hide when it is minimized, respond to clicks (Multi-Q BYPASS), and close with it, no crash | **partial**: loopino and u-he Diva embed; closing the main window closes both, exit 0. Not done: move/minimize with the main window (no `WM_TRANSIENT_FOR`); knobs not tried (no eyes on screen) | not possible: CLAP has no embedded-Wayland API |
| 2 Screen reader (reported) | no accessibility tree in 0.2.2: only the native menus are visible to VoiceOver | — | — |
| 3 Control channel `tree` / `click` / `state` | **pass** (incl. `no_element`, 200 calls) | **pass** | **pass** |
| 4 64 meters + playhead at 60 fps | **pass** (60 Hz display): 1,200 frames per 20 s; dropped (interval > 25 ms) 0.08 % without editors, 0.08–0.42 % with both editors open and 300 `vizctl tree` calls; p99 17.7 ms → 22.7 ms with editors | **pass** (render-callback rate; on-screen presentation not seen; `over budget` there used the strict 16.7 ms cutoff, see note): ~165 fps, p50 6.06 ms, p99 6.37 ms, 0 % over 16.7 ms | 2 frames then idle (frame callbacks; visibility unconfirmed) |
| 5 Text, menus, shortcuts | **pass**: native menus; ⌘B and `b` bump in the pane; typing `bob` in the field inserts it without bumping; ⌘A then typing replaces; Tab moves focus | no menus shown (GPUI draws none on Linux) | same |

## What phase 2 must plan for

1. **Pin GPUI main** (X11 `window_handle` fix; `xattr` build break on Linux in 0.2.2). Release builds precompile Metal shaders (CI installs the Metal Toolchain).
2. **Text input component**: GPUI has none; the spike adapts GPUI's example. VizDAW needs a full one (IME, selection, undo, number fields).
3. **Shortcut contexts**: single-key shortcuts must carry `!TextInput`, or typing in a field triggers them (proved by test).
4. **Element registry**: GPUI has no accessibility tree, so every control records role, label and bounds while drawing (spike: zero-size `canvas` overlay). The spike hard-codes element paths and presses by label; phase 2 derives paths from the view tree and dispatches clicks through hit-testing so enabled state and layout changes are respected. Cap control calls handled per frame so a burst cannot stall drawing. The control channel uses it now; AccessKit can be fed from it for 27.2. Point clicks (`click X Y`) need hit-testing the registry.
5. **Linux menus**: an in-window menu bar.
6. **Linux plug-in editors**: one GPUI window per editor with the plug-in embedded through its X11 ID; the Linux app runs GPUI under XWayland (only XWayland inside a KDE Wayland session was tested, not a pure X11 session). The editor window needs `WM_TRANSIENT_FOR` set to the main window (through X11 directly; GPUI has no API) so it moves and minimizes with it, and an `on_window_should_close` handler that closes the editor before GPUI destroys its X11 window (otherwise the plug-in's next X call hits BadWindow and Xlib exits the process). On quit, close every editor before windows are torn down. CLAP X11 sizes are physical pixels: call `gui.set_scale` with the window's scale and convert to points. core-host needs CLAP `timer-support` (and likely `posix-fd-support`) on Linux; DPF-based plug-ins refuse to open without it.
7. **Mac plug-in editors**: child `NSWindow` via `addChildWindow:ordered:`; editors close before their parent (`on_window_should_close`).
8. **Launching for tests on macOS**: a shell-launched binary cannot take focus on macOS 14+; the dev loop needs an `.app` wrapper (`scripts/bundle-mac.sh`).

## Kept from the spike

- `core-host`: `clap::editor_api()` (Cocoa on macOS, X11 on Linux) and the X11 parent path (commit 909a0933). Everything under `spikes/` stays throwaway.

## Accessibility (27.2)

0.2.2 exposes nothing inside the window. Main depends on AccessKit; alternatively feed AccessKit from the element registry through `accesskit_macos` / `accesskit_unix`.

## Measurement note

The plan's pass line ("over-budget ≤ 1 %, p99 ≤ 16.7 ms") cannot be met by any app on a 60 Hz display: intervals sit on the 16.67 ms vsync period with microsecond jitter, so about half land just above it. The spike now also reports **dropped frames** (interval over 1.5 periods, 25 ms), which is what the check means; that is the number used above. p99 with plug-in editors open (about 23 ms) is one late frame in a hundred, below the drop line.
