# 27.1.0 Host Track H2: Plug-in Editor Windows Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app opens plug-in editor windows with Swift's behaviour and strings: AU on macOS, CLAP and VST3 on macOS and Linux (X11, XWayland), with the header (Bypass, Learn shown disabled, status), the parameter-list fallback, the Built-in Synth sound window, and the slot gestures that open them (Open, double-click, the EQ slot's click). Core gains VST3 `IPlugView` hosting, an asynchronous AU v3 view request, and every H0 "Carried to H2" item.

**Architecture:** core-host adds `Vst3Editor: PluginEditor` with a host frame (`IPlugFrame`, plus `Linux::IRunLoop` on the existing run-loop table) and makes the AU v3 view request asynchronous (`open` reports "opening", a later `open` finishes). core-ffi folds the scale into the exported `open_editor`. vizdaw-app ports `PluginWindowModel` and the Built-in Synth window model. vizdaw-gpui gets one `Normal` GPUI window per (document, slot): a GPUI header above a container we own (an `NSView` hosted through `gpui::NativeView` on macOS; an X child window with the root visual on Linux) that the plug-in is parented into, so a Busy close can outlive the GPUI window. One app-wide main-thread loop pumps the plug-in run loop.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin (`f4fe0be1f8`, `gpui::NativeView`); `vst3 = "=0.3.0"`; `objc2-app-kit` (macOS), `x11rb` 0.13 (Linux); the `test-plugins` crate and the VST3 SDK samples built in CI.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§2 goals 4 and 5). Research notes with citations: the H2 inventory (§1 core, §2 native hosting, §3 carries, §4 slot UI, §5 Swift behaviour and strings, §6 task split, §7 risks). H0 and H1 plans and Outcomes (`docs/superpowers/plans/2026-11-23-host-h0-clap-linux.md`, `2026-11-23-host-h1-vst3.md`).

## Global Constraints

- Worktree `~/workspace/vizdaw-host`, branch `host`. H2 is the host track's UI slice: it edits `crates/vizdaw-app` and `crates/vizdaw-gpui` (`mixer/slots.rs`, `mixer_view.rs`, `app_state.rs`, `lib.rs`, `control.rs`); rebase on `next` before Task 3 and again before Task 5 so the mixer track's edits are not fought.
- **Ruling D1 (owner):** macOS editor windows are free standalone windows as in Swift (no `addChildWindow`; they can go behind the document window). Linux editor windows are `WM_TRANSIENT_FOR` their **document** window (set by us with `x11rb`, never GPUI's focused-window guess), also when opened from the Mixer window or reopened at project load. GPUI window kind `Normal` everywhere (not `Floating`, not `Dialog`).
- **Ruling D2 (owner):** on Linux the app runs as X11 (XWayland in a Wayland session) when `DISPLAY` is set, so every editor embeds; native-Wayland editors are a later 27.x item. The design doc's goal 5 is amended in Task 4.
- **Ruling D3 (controller):** the AU v3 editor open becomes asynchronous in core (open reports "opening"; the window retries on the tick). No nested run loop may run under GPUI. The generic view for AU v3 is only the stopgap if the async path slips, and needs a recorded ruling.
- **Ruling D4 (controller):** the Built-in Synth sound window is in H2.
- **Ruling D5 (controller):** `open_editor_scaled` is folded into the exported `open_editor(slot, parent, scale)`; no Swift compatibility.
- **Carries:** every H0 "Carried to H2" item is a step below: the run-loop pump from the instance thread in every Linux binary that loads CLAP (Task 2), POLLNVAL back-off (Task 2), drop resizes for closed slots (Tasks 2 and 3), cap fixed sizes to the screen (Task 4), lift the macOS timer/fd and resize gates (Task 2, landing with the app pump), `WM_TRANSIENT_FOR` (Task 4). Also the timer cap (256 per instance), `VIZDAW_REQUIRE_DISPLAY`, and view-before-controller-terminate.
- Sizes: core speaks points. X11 container px = points × scale. Header 37 pt including the divider (Swift `headerHeight`). The screen cap applies to the window, not to what the plug-in is told.
- Close order everywhere: editor, then container, then GPUI window. Document close and quit close every editor **before** the session drops.
- Stays out (the plug-in catalogue slice): the instrument menu, "+", "EQ ＋", "Replace…", the plug-in browser, the Plug-in Manager, Plug-in settings, MIDI Learn (Learn shows disabled).
- Swift is retired (no compatibility); exact Swift strings (inventory §5); UI says "track", never "lane"; no banned vendor/DAW product names in repo text — the `vst3` crate's vendor-named binding module, re-exported once in `core-host/src/vst3/ffi.rs` (Rust use paths only), is the only allowed use, licence notices exempt.
- GPUI tests press `secondary-` keys. On-screen checks become rows in `docs/testing/manual-checks.md`.
- Subagents: no GUI, no computer-use, never `dangerouslyDisableSandbox`. Linux tests run over ssh on 192.168.1.192 (`~/vizdaw-host`, Xvfb for X11); the controller runs them. CI: fmt, clippy `-D warnings`, tests on macOS and Linux. Commit trailers as before.

## Review Focus

1. Lifetimes and close order: no plug-in ever draws into a destroyed parent (the X11 BadWindow exit); a Busy close keeps the container alive (macOS `Retained<NSView>`, Linux reparented to the root and unmapped) and is retried until it succeeds; document close and quit close editors before the session drops.
2. Re-entrancy: no nested run loop or synchronous callback into `App` from `open_editor`/`close_editor`/`resize_editor`; opens run from `cx.defer` after the window is registered; the AU v3 open never spins the main run loop.
3. VST3 COM: `removed` → `setFrame(null)` → view release, before controller `terminate`; every `IRunLoop` registration a view made is gone when it closes or drops; `resizeView` inside `attached` is held and reported at the next idle.
4. Sizes: points vs pixels on every path (open, user resize snap without re-entry, `editor_resize` follow, scale change reopen); the screen cap; resizes for closed slots dropped.
5. Run loop: the macOS gates lift only with the app pump in place; POLLNVAL delivered once; timers capped; the pump runs on the thread that creates instances.
6. Model parity: one window per (document, slot), front on reopen, refused while that slot's close is pending, closed when the slot goes (undo included); a parameter drag is one undo step "Change Plugin Parameter"; a double-click on an insert arms no drag.

---

### Task 1: core-host — the VST3 editor and the test VST3 `view` kind

**Files:** `crates/core-host/src/vst3/{editor.rs,instance.rs,ffi.rs}`, `crates/core-host/src/run_loop.rs`, `crates/test-plugins/src/{vst3_module.rs,vst3_view.rs}`; tests `crates/core-host/tests/{vst3_editor.rs,vst3_gui_linux.rs}`.
**Interfaces:** `Vst3Editor: PluginEditor` (`set_scale`, `open(ParentView) -> (w, h)` in points, `resize`, `can_resize`, `close`); a host frame `ComWrapper` implementing `IPlugFrame` and, on Linux, `Linux::IRunLoop` (fds through the posix-fd path, timers through the timer path); `Vst3Instance::editor()` returns it when the controller has a view; `InstanceEvents.editor_resize` from `resizeView`.
- [ ] Tests first: headless on both OS (sizes in points, scale sent before `attached` on Linux and not on macOS, resize both ways through `checkSizeConstraint`/`onSize`, a `resizeView` during `attached` reported at the next idle, `removed` and release on close and on drop, run-loop leftovers gone); Linux under Xvfb with `VIZDAW_REQUIRE_DISPLAY` (the test view's X child and pixels; the SDK `again` and `host-checker` editors open into an `x11rb` parent and a child appears; their timers fire through the pump).
- [ ] Implement the inventory §1.3 order: lazy `createView("editor")` with a cached "has a view" (no view kept alive between opens, so `has_editor` does not create one per snapshot), `isPlatformTypeSupported` (`NSView` / `X11EmbedWindowID`, else no editor), `setFrame`, `setContentScaleFactor` on Linux, `attached` then `getSize` (Linux px → points with the CLAP `to_points`/`from_points` rules), `canResize`, accept `resizeView` from fixed-size views and answer with `onSize`. Release the view before controller `terminate` (H1 Review Focus 1). Forward no keys; ignore `requestOpenEditor`.
- [ ] Test VST3 `view` kind: a headless `IPlugView` recording `attached`/`removed`/platform type/scale/`onSize`, able to call `resizeView` through the frame, registering an `IRunLoop` timer and fd on Linux; an X11 drawing variant reusing `TP/gui.rs`.
- [ ] Controller runs the Linux tests; commit `core-host: VST3 editors`.

### Task 2: core-host and core-ffi — run loop, gates, async AU v3, the scaled `open_editor`

**Files:** `crates/core-host/src/{run_loop.rs,clap/handlers.rs,instance.rs}`, `crates/platform-macos/src/au/{mod.rs,ffi.rs}`, `crates/core-ffi/src/{editor.rs,session.rs}`, `crates/vizdaw-gpui/src/lib.rs` (the app-wide pump), `crates/vizdaw-play/src/live.rs`, `.github/workflows/ci.yml`; tests `crates/core-host/tests/{run_loop.rs,clap_editor.rs}`, `crates/core-ffi/tests/editors_main.rs`.
**Interfaces:** `Session::open_editor(slot, parent: u64, scale: f64) -> Result<EditorSize, _>` (D5; `open_editor_scaled` removed, callers and the fake session updated); a new `HostError::Opening` mapped to `Busy` with a distinct "editor opening" message; `run_loop_fds()` omitting an fd after its one POLLNVAL `ERROR` until it is modified or re-registered; a per-instance cap of 256 timers.
- [ ] Tests first: flip `on_macos_resize_requests_are_refused_and_nothing_changes` (`clap_editor.rs:255-270`) to accept; the `timer` kind ticks on macOS through `run_loop_pump`; register an fd, close it, pump N times → one callback (carry 2); the 257th timer is refused; `close_editor` clears that slot's pending `editor_resize` (carry 3, core half); an "opening" open on a fake AU v3 editor is `Busy` and a later open succeeds.
- [ ] Lift the macOS gates (carry 5): `HostTimer` everywhere, `HostPosixFd` on `unix`, `request_resize` allowed on macOS; update the `run_loop.rs` docs.
- [ ] Async AU v3 (D3): `requestViewController` fills a slot from its completion; `open` returns `Opening` until it is filled, then attaches; no `CFRunLoopRunInMode` in the open path. The Cocoa and generic paths stay synchronous. The AU v3 part is a controller check (an AU v3 cannot load in CI).
- [ ] Run-loop pump (carry 1), in the same commit as the gate lift: vizdaw-gpui runs one app-wide `cx.spawn` loop on the main thread every 10 ms calling `vizdaw_ffi::editor::run_loop_pump()` (not inside the 50 ms document idle); vizdaw-play pumps in its live loop (100 ms resolution, documented). Document that vizctl, vizdaw-scan and vizdaw-mcp need none.
- [ ] CI: `VIZDAW_REQUIRE_DISPLAY=1` in the X11 and e2e steps. Commit `core: editor run loop on macOS, async AU v3 views, scaled open_editor`.

### Task 3: vizdaw-app — the plug-in window and Built-in Synth window models

**Files:** `crates/vizdaw-app/src/{plugin_window.rs,builtin_synth_window.rs,document.rs,testing.rs,lib.rs}`; tests in those modules.
**Interfaces:** `PluginWindowModel` (title, `refresh`, open/owed retry, close → pending → retry, user resize ↔ `resize_editor`, `editor_resize` follow, parameter rows with drag/click/refused touch, touched follow for Learn, bypass); `BuiltinSynthWindowModel` (sounds, set, refusal); `DocumentModel` keeps `params_changed` and `editor_resize` from idle instead of dropping them; the fake session scripts `open_editor` (incl. Busy/opening), `close_editor` (Busy), `resize_editor`, `params`, `set_window_open`, `open_windows`.
- [ ] Port the 20 `PluginWindowModelTests`, the 2 `PluginIdleTests` if `document.rs` lacks them, and `soundsRoundTripThroughTheSession`, plus a test that a resize for a slot whose editor is closed is dropped (carry 3, app half).
- [ ] Implement with Swift's strings: title "‹strip› — ‹plug-in›" (the plug-in name alone with no strip name, "Plugin" with no row), "Loading…", the placeholder reason, the error message, "No parameters.", "Bypass Failed", "Parameter Failed", undo "Change Plugin Parameter", value text (whole number when stepped, else "%.2f"), normalised value clamped (0 for an empty range or NaN), hidden parameters left out; Built-in Synth "Sound", Piano/Bass/Pad/Lead/Drums, "Changes every track that plays this Built-in Synth.", "Set Built-in Synth Sound", "Only a Built-in Synth has these sounds.". Reuse the strip-control drag model (`mixer/model.rs`) rather than copying it.
- [ ] Commit `app: plug-in window and Built-in Synth window models`.

### Task 4: vizdaw-gpui — plug-in windows (platform layer)

**Files:** `crates/vizdaw-gpui/src/plugin_windows/{mod.rs,macos.rs,linux.rs}`, `crates/vizdaw-gpui/src/{app_state.rs,lib.rs,control.rs}`, `crates/vizdaw-gpui/Cargo.toml`, `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`; tests `crates/vizdaw-gpui/tests/plugin_windows.rs`.
**Interfaces:** `PluginWindows` registry keyed by (document, slot); a per-platform `EditorHost` (create in a GPUI window at header offset and size, `set_frame`, `park` for a pending close, `destroy`); `WindowEntry` holds an `AnyWindowHandle` with a kind, so `state.get windows` lists `plugin` (title as above) and `other` (Built-in Synth), and `ui.tree`/`ui.screenshot` reach them.
- [ ] Tests first under the GPUI test platform (native attach skipped there): registered before open via `cx.defer`; a second open brings it to the front; refused while the slot's close is pending or the slot is gone; closes on slot gone, on document close (before `close_document` drops the session) and at quit (before `close_all`, also in `on_app_quit`); saved windows reopen with the project; window list kind and title.
- [ ] Window: `Normal`, titled, closable, minimizable, centred, initial 420×360; resizable only while a resizable editor shows (style mask via the raw handle on macOS, `WM_NORMAL_HINTS` on X11), min content 120 × 97; content = editor + 37, or the parameter list (width 400, 360 when empty; height min(420, 30n + 30), 120 when empty) plus the header.
- [ ] macOS: a plain container `NSView` in a `gpui::NativeView` element sized to the editor's points; the plug-in attaches inside the container. Busy close keeps a `Retained<NSView>` in the pending list, drops the `NativeView`, closes the window, retries on the tick. Reserve no actions (plain keys to the plug-in, ⌘-keys to the menu, README :70-72).
- [ ] Linux: pin GPUI to X11 when `DISPLAY` is set (D2, `linux_platform(WindowingModes::X11)`); one process-wide `x11rb` connection on the main thread; the container X window with the root depth and visual, the default colormap and a border pixel, at (0, 37 × scale); `WM_TRANSIENT_FOR` on the GPUI window → the document window (carry 6); on resize or scale change move/resize the container (a new scale reopens the editor). Close: `close_editor` → destroy container → GPUI window; on Busy reparent the container to the root and unmap it, retry, destroy on success.
- [ ] Sizes: open size and every `editor_resize` capped to `display.visible_bounds()` minus a margin (carry 4; the editor is clipped, no scrolling); a user resize sends `resize_editor(content − header)` and snaps to the answer without re-entering the handler; `editor_resize` applies only to an open window whose editor is open.
- [ ] Amend the design doc goal 5 to: "Linux audio and MIDI through PipeWire; plug-in windows on X11, and in Wayland sessions through XWayland (the app runs as X11 when `DISPLAY` is set); native-Wayland plug-in windows are a 27.x item." with "(owner, 2026-11-24, D2)", and list it as a Linux gap with its answer in §2a.
- [ ] Commit `gpui: plug-in windows`.

### Task 5: vizdaw-gpui — header, parameter list, Built-in Synth window, slot gestures

**Files:** `crates/vizdaw-gpui/src/plugin_windows/view.rs`, `crates/vizdaw-gpui/src/builtin_synth_window.rs`, `crates/vizdaw-gpui/src/mixer_view.rs` (`slot_holder`, `builtin_synth_slot`, `eq_slot`, `slot_entries`, module doc), `crates/vizdaw-app/src/mixer/slots.rs` (`SlotMenuItem::enabled`, module doc); tests `crates/vizdaw-gpui/tests/{mixer_slots.rs,plugin_window_view.rs}`.
**Interfaces:** the header ("Bypass" toggle; "Learn" disabled with help "Touch a parameter, then learn a MIDI control for it" / "MIDI Learn: ‹name›" / "parameter ‹id›"; the status or placeholder text); parameter rows (name 110 wide, slider labelled by the parameter name with value text and ", read-only", value 48 wide monospaced) as registry elements; the Built-in Synth window (titled "Built-in Synth", 300×200, the "Sound" radio group and caption, follows every snapshot, one per (document, destination)).
- [ ] Tests first: `Open` enabled for insert, instrument and EQ slots; a double-click on an insert or instrument slot opens its window and leaves no press-to-drag armed; the EQ slot opens on a single click; a Built-in Synth slot double-click opens the Built-in Synth window (button recorded enabled; its single click, the instrument menu, stays disabled); tree labels; a parameter drag is one undo step; Bypass toggles the slot.
- [ ] Implement; update the "does nothing yet" module docs. Commit `gpui: plug-in window header, parameters, Built-in Synth window and slot gestures`.

### Task 6: e2e, CI and checks

**Files:** `crates/vizctl/tests/e2e_gpui.rs`, `.github/workflows/ci.yml`, `docs/testing/manual-checks.md`, `docs/superpowers/specs/2026-11-22-parity-checklist.md` (rows "Plug-in editor window", "Built-in Synth window").
- [ ] e2e: `gpui_plugin_window_opens_bypasses_and_reopens_with_the_project` (CLAP `timer` kind; `ui.click` count 2 → windows list kind `plugin`, title "‹strip› — ‹plug-in›", content 300×237; header Bypass toggles the slot; save, close, reopen → window back; remove the slot → window gone; README 11, 12, 18); `gpui_plugin_window_without_an_editor_lists_parameters` (gain kind). Linux CI only: `gpui_x11_editor_embeds_under_the_plugin_window` (gui kind: GPUI window → container → plug-in child by `query_tree`; `WM_TRANSIENT_FOR` = the document window; the gui kind's resize request grows the window; close leaves no plug-in window) and the same for the VST3 `view` kind and SDK `again`.
- [ ] CI: the new X11 test steps with `VIZDAW_REQUIRE_DISPLAY`. Controller runs the Linux e2e over ssh.
- [ ] `manual-checks.md` rows — controller (macOS): an AU Cocoa editor, an AU v3 editor ("Loading…" then the editor, no hang or crash), the bundled Multi-Q CLAP, an SDK VST3 `again`; knob drag = one undo step; Bypass from the header; the keys rule; a resizable editor follows a window drag; a self-resizing CLAP editor grows its window; a move between a 2× and a 1× display; close during another editor's open (Busy); quit with two windows open. Owner (Linux): XWayland in the KDE session and a pure X11 session; the gui test plug-in, a DPF plug-in (timer required) and a heavy open-source instrument as CLAP and VST3; move/minimize with the document window; typing into an editor's text field; 2× scale; close and quit with editors open (no BadWindow exit); transparency or garbage in the editor area.
- [ ] Tick the parity rows; write the Outcome (rulings, carries, deferred minors); commit `host H2: e2e, CI and checks`.

## Outcome (2026-11-24)

All six tasks done (commits on `host` from 4f267133 to the Task 6 commit `host H2: e2e, CI and checks`). The GPUI app opens plug-in editor windows (CLAP, VST3 and AU on macOS; CLAP and VST3 embedded on X11 and XWayland), with the header, the parameter-list fallback, the Built-in Synth sound window and the slot gestures, on Swift's strings; core hosts VST3 `IPlugView`s, pumps the plug-in run loop from the app, and opens AU v3 views asynchronously. Evidence: vizdaw-app and vizdaw-gpui 1609/1609 on the Mac at the end, app+gpui 1609/1609 on Linux after Task 5; GPUI e2e 34/35 on the Mac (`gpui_demo_play_screenshot_stop` needs the `screenshot` build) (with `gpui_plugin_window_opens_bypasses_and_reopens_with_the_project`, `gpui_plugin_window_without_an_editor_lists_parameters` and `gpui_builtin_synth_window_picks_a_sound`); the Linux X11 e2e (`gpui_x11_*`) runs in CI only (the test box has no X server).

**Not yet seen on screen or on a real device:** every real editor (AU Cocoa, AU v3, Multi-Q, the SDK `again`, Linux plug-ins), a real AU v3's asynchronous open, the keys rule, ⌘W with a focused native editor, 2× scale, and the Linux X11 tree outside Xvfb. All are rows in `docs/testing/manual-checks.md` ("Plug-in editor windows (host track H2)").

### Plan amendments

- Task 1: Ruling R1 (implementer, accepted; plan amendment): the module's host context also offers Linux::IRunLoop — VSTGUI's X11 frame registers through the factory-wide run loop set from setHostContext (x11frame.cpp:646, x11platform.cpp:131), else a null call — cost: host-context registrations live until the plug-in unregisters them or the module exits; Review Focus 3 "gone at close" holds for frame registrations only
- Task 5 file list: `crates/vizdaw-gpui/src/builtin_synth_window.rs` was not created; the Built-in Synth window's drawing is in `crates/vizdaw-gpui/src/plugin_windows/view.rs` (Task 5 ruling below). Task 4 also added `plugin_windows/x11.rs` and `plugin_windows/other.rs`.
- Task 6 file list: also `crates/vizdaw-app/src/session.rs` and `crates/vizdaw-gpui/src/lib.rs` (the catalog at launch), `crates/vizdaw-gpui/src/control.rs` (`content`, ⌘W over the channel), `crates/test-plugins/src/vst3_view.rs` (`VIZDAW_TEST_VST3_VIEW_FLAGS`) and `crates/vizctl/Cargo.toml` (`test-plugins`, Linux `x11rb`).

### Rulings

- Task 2: Rulings (implementer, accepted): pump in core-host (re-exported by core-ffi; vizdaw-play lacks core-ffi); only POLLNVAL silenced; 256-timer cap per table; debug_editor_opening test hook; GPUI pump starts in run() not install() — costs in task-2-report.md
- Task 2: Ruling: reviewer Minor 1 (AU v3 completion block released after the request) stays Minor — the block ABI requires an async callee to copy a block it keeps; the old code relied on the same — cost: a crash with a unit that breaks that rule; cheap fix (keep RcBlock in ViewRequest) if seen
- Pre-Task 3: merged origin/next (670b547c) into host as f543f07f (no force-push; plan said rebase); pre-Task 5 merged origin/next again (IdleNews kept both sides' fields; plugin_news folded into take_idle_news)
- Task 3: Rulings (implementer, accepted): drags reuse MixerModel gestures (Touch(None) resolved by core, session.rs:9191); end_plugin_gestures dropped (core close_editor ends them); plug-in news kept only for slots with a window
- Task 3: Ruling: idle problems (plug-in load failures, autosave failed) still dropped — pre-existing; owner slice 8b (Faults bar item) — backlog row on next (since reached the Faults popover on next, 35a627e1)
- Task 4: Rulings (implementer, accepted): close button and resize snap on a later main-loop turn (cx.spawn) — no plug-in call inside the platform callback; extra file plugin_windows/x11.rs; parent 0 on the test platform; SCREEN_MARGIN 40 pt; scale-change reopen Linux only; a Busy close then document close keeps the container until exit (no BadWindow) — costs in task-4-report.md
- Task 5: Rulings (implementer, accepted): Built-in Synth drawing in plugin_windows/view.rs (no builtin_synth_window.rs in gpui); Bypass a checkbox in ui.tree; double-click opens on the second mouse-down; Built-in Synth slot opens the focused track's synth, else the first (reviewer: no wrong-track write — changes go to the shown destination)
- Task 6: Ruling (implementer): the GPUI app opens its plug-in catalog (`plugins.json` in the support folder) at launch, before any document, with no scan (Swift opened it at launch) — without it no project's plug-in loaded in the GPUI app at all — cost: a damaged catalog file is only printed to stderr until the Plugin Manager slice shows it
- Task 6: Ruling (implementer): `state.get windows` rows carry `content` (the drawn size, no title bar) beside `frame` — the brief's 300×237 is a content size
- Task 6: Ruling (implementer): over the channel, `ui.key` ⌘W in a plug-in or Built-in Synth window is delivered to that window (its own ⌘W closes it) instead of running `file.close` on its document, which closed the whole project — Swift `performClose` parity
- Task 6: Ruling (implementer): the e2e seeds the throwaway home's catalog with the test bundles and gives a saved copy of the demo an insert by editing its `project.json` (the GPUI channel has no `mixer.add_insert`; the catalogue slice adds the picking) — cost: the file format's insert shape is written by `core_model::mixer::PluginSlot`'s serde, not by the app
- Task 6: Ruling (implementer): the test VST3 view reads extra flags from `VIZDAW_TEST_VST3_VIEW_FLAGS` so an app the test launches draws the X11 variant
- Task 6: Ruling (implementer): the parity rows are ticked on the Mac only (H2); the Linux cells stay blank until CI's `gpui_x11_*` and the Linux GPUI e2e pass on `host`

### Carried

- Task 1: Linux gate: box lacks Xvfb/ninja — vst3_gui_linux runs in CI only (added to ci.yml); box ran core-host/test-plugins/editors_main 258/258
- Task 2: Linux 2033/2034 — scan_fills_the_catalog_and_plugins_lists_it fails on the box on next too (systemd-coredump core_pattern makes the crasher outlive the 2 s scan timeout → 'timed out'); environmental, not H2; backlog row on next
- Task 3: carry to Task 4: registry enforces one window per (document, slot); tick calls refresh/open_editor/take_editor_resize/follow_core_touches/retry_close, opened() on create — done in Task 4
- Task 4: fix round 1 4f6f370f: park and destroy checked (server round-trip before GPUI destroys its window); CI runs vizdaw-gpui --lib plugin_windows::x11 under Xvfb
- Task 4: carry to Task 5: ⌘W closes a plug-in window (Swift did) — done in Task 5 (keyboard) and Task 6 (the channel)
- Task 6: carry to the controller: run the Linux GPUI e2e (`gpui_x11_*` included) over ssh where an X server is available, else read CI's run; then tick the Linux parity cells

### On-screen checks (rows in `docs/testing/manual-checks.md`)

- Task 2: controller check (on screen, Task 5+): real AU v3 opens Busy then attaches; close while opening; generic view after 3 s
- Task 5: on-screen checks: a native editor holding focus may swallow ⌘W/Ctrl-W (X11); window focuses its root on open
- Task 4 and Task 5 reports' on-screen lists (the header at 37 pt, the parameter list, the Built-in Synth window, real double-clicks, the style mask and snap, the Busy park, the 24-bit container)

### Deferred minors

- Task 1: minor (deferred): 6 Minor items in task-1-review.md
- Task 2: minor (deferred): Minors 2-5 in task-2-review.md
- Task 3: minor (deferred): Busy destinations() → wrong set_sound refusal; Busy first-move read drops a drag; toggle_bypass not deduped; end_open_drag releases plug-in touch; no test that closing stops params_changed
- Task 4: minor (deferred): our WM_NORMAL_HINTS replace GPUI's; macOS host drop assumes GPUI releases its last-frame NativeView clone; X11 test pins the contract, not the original race; other Minors in task-4-review.md
- Task 5: minor (deferred): no focused-track Built-in Synth test; header test doesn't pin 37 pt; two Built-in Synth windows share a title (Swift same); other Minors in task-5-review.md
- Task 6: minor (deferred): over the channel, other bound keys in a plug-in window still run their commands (Space plays) where the keyboard gives plain keys to the plug-in; a parameter-list window opened while its plug-in loads shows 360 wide until the parameters arrive, then grows to 400; the GPUI app ignores `VIZDAW_SUPPORT_DIR` (the e2e seeds `$HOME`'s support folder)
