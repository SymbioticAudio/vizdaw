# 27.1.0 Slice 6a: Mixer Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app gets a usable mixer: the Mix view (⌃2) and Arrange view (⌃1) with the window's pane host (left, right and bottom panes from `layout.views`, dividers, pane pickers), the bottom pane's Mixer view and a separate Mixer window, channel strips with fader, pan, level box, mute/solo/record, automation mode and output routing, master and buses (Add Bus), live meters, strip selection and rename — driven over the control channel (`state.get mixer|layout|automation`, the `ui.*` view and pane commands, `window.mixer`, `automation.*`, `mixer.new_bus`) with the Swift strings, labels and undo steps.

**Architecture:** As before: behaviour in `vizdaw-app`. `DocSession` grows the core's mixer, levels, gesture, automation and `run_mixer` calls; a `mixer` module ports `MixerModel` (strip columns, touch/move/release/click gestures that the core owns as undo steps, typed values, heard values while playing, selection, rename, add bus, output) and the meter ballistics; one mixer model per document, shared by every placement. `vizdaw-gpui` gains a pane host, reusable controls (fader, knob, number box, toggle, menu face, meter canvas) and a `MixerView` used by the Mix view, the bottom pane and the Mixer window.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin (`font-kit` on); `core-ffi`, `core-control`, `core-model` (`automation::{fader_db, fader_norm, pan_of, pan_norm}`), `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§4, §7 check 4 meters, §8, §10). Parity checklist: `docs/superpowers/specs/2026-11-22-parity-checklist.md` (Mixer, View, Window rows). Slice 5 plan Outcome (patterns: model-owned canvas glue, focus flags set directly, flat `ui.tree` scoped by id path, fresh-read caches): `docs/superpowers/plans/2026-11-22-gpui-slice-5-piano-roll.md`. Swift reference: `app/Sources/VizDAW/{MixerView,ChannelStrip,StripGrid,Fader,Knob,NumberBox,LevelMeter,PaneContent,DocumentShell,MixerWindow}.swift` and `app/Sources/VizDAWAppModel/{MixerModel,MixerMath,StripEntry,ControlGestureModel,ControlDrag,WindowLayout,PaneGeometry,ViewCommands,AutomationCommands,MixerCommands}.swift` (exact files and lines in the inventory). Research notes with citations, exact strings, constants, Swift tests and e2e steps: the slice 6 inventory (controller scratch; relevant sections copied into each brief).

## Global Constraints

- Song format unchanged (v11). `ui.json` unchanged by the mixer (the `views` layout already round-trips with unknown keys kept).
- `vizdaw-app` never depends on GPUI. Command IDs, titles, keys and params come from `core_control::registry`. `state.get mixer` JSON is produced by `vizdaw_ffi::control::result_json(ControlResult::Mixer…)`, never a second serializer.
- Faders, pan and levels are core gestures (`gesture` touch / value / release), not `EditCommand`s: the core names the undo step ("Set Fader", "Move Group <name>", …); the app never synthesises labels. Every opened touch is released — on mouse-up, on window deactivation, when the strip vanishes, when the last placement closes — and a bare press sends nothing.
- Exact Swift strings, labels and values for every strip control, status and refusal (inventory §1.3, §3); master has no Solo/Output/Send/Pan rows; send indexes are 1-based in the registry and 0-based in the FFI.
- Meters: read through `Session::levels()` at 30 Hz with Swift's ballistics (`max(peak, old × 0.85)` per 30 Hz tick, peaks sanitised to 2.0, scaled by elapsed time if drawn per frame); a strip missing from a take drops to silence; no meter read blocks the audio thread. No clip latch or peak-hold line (Swift has none).
- Out of this slice (menu items disabled, channel `not_yet`, rows left blank so faders line up): sends, groups, VCAs, insert/EQ/instrument slots, the left-pane Channel Strip's real content, MIDI destination strips, plug-in editor windows and the plug-in catalogue, MIDI Learn and surfaces, Co-Producer, themes, recording inputs, automation lanes, the MIDI FX section.
- Colours: named tokens in a `strip_palette.rs` (classic-light values) so the themes slice swaps values.
- Single-key shortcuts carry `!TextInput`; number boxes are `TextInput`s (Return commits, Escape cancels, refusal → status).
- Every control records role, label, value and bounds in the registry while drawing (fader/knob role `slider`, number box `textfield`, toggles `button` with value `on`/`off`, strip menus `menu`).
- Subagents: no GUI, no computer-use tools, never `dangerouslyDisableSandbox`. Visible checks and e2e runs are done by the controller (Mac) or over SSH on the owner's Linux box.
- Banned vendor/DAW names out of repo text. UI says "track". Never commit `.DS_Store`, `.superpowers/`, generated FFI files or brand assets.
- CI: `cargo fmt --all -- --check`, clippy `-D warnings` (with and without `--features screenshot`), tests on macOS and Linux.
- Commit trailers: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.

## Decisions (controller, from the inventory §7.2)

- M1 Meters: Option A (`levels()` polled at 30 Hz, two short mutexes, audio thread untouched); measure 64 strips at 60 fps on the Mac and record the numbers in the Outcome; a lock-free feed (Option B) only if the measurement misses.
- M2 Ballistics: Swift's; no clip latch or peak hold.
- M3 Fader/knob drags live in the model as `touch/move/release/click` with an explicit drag tracker (port `ControlDrag.Tracker` and `ControlGestureModel`, incl. the 0.25 s double-tap rule, ⌥ = fine while dragging and "alone" at press for the fader). No Escape cancel (Swift has none). Not the slice 4/5 `gesture.rs` (the core owns these steps).
- M4 Pane host: the full host (left, centre, right, bottom, dividers with size drag in the Swift ranges, pane pickers); panes whose content comes in later slices show a short note ("<Pane> comes in a later version." is a NEW string: use the Swift note text where the Swift app has one, else this).
- M5 Mixer window: in this slice (second window kind `mixer`, title `Mixer — <document title>`, default 760×560, transport bar without the document-only items).
- M6 Plug-in slot rows: not shown this slice (6b shows them read-only).
- M7 Palette tokens in `strip_palette.rs` (classic-light values from the Swift theme where the repo holds them; placeholders noted otherwise).
- M8 Strip row heights: explicit heights; port `StripLayout::split` and the share-rows rule.
- M9 `heard` values polled while playing (≤ 4096 targets, 30 Hz) so faders follow automation.
- M10 One mixer model per document; placements (Mix view, bottom pane, Mixer window) as a set; drags released only when the last placement goes.

## Review Focus

1. A fader or pan drag never leaves a touch open: mouse-up, window deactivation, the strip vanishing, the view closing, and a Busy release (retried) all release it; a bare press or a double-click leaves no touch; the core's Busy refusal "Release the touched control first." never shows after the user lets go — Tasks 1, 4 and 5.
2. Typed values: the strict grammar (ASCII digits, 32-scalar cap, U+2212 minus, `-inf` yes, bare `inf` no, values rounding to the floor print `−∞ dB`), refusals to the status line, Escape restores — Tasks 1 and 4.
3. Meters under load: 64 strips at 60 fps with no dropped frames from the meter read; a second document (not holding the engine) shows silent meters, not an error; a strip that leaves the take decays to silence — Task 6.
4. Pane geometry at the edges: a window too small for the panes hides them in Swift's order for the frame only (layout keeps them); divider drags clamp to the Swift ranges; the layout survives a save — Task 3.
5. Two placements of one mixer (Mix view and Mixer window): a drag in one is seen in the other; closing one does not release the other's drag; renames are placement-scoped — Tasks 1, 5 and 6.

---

### Task 1: core-app — mixer session calls and the mixer model

**Files:** create `crates/vizdaw-app/src/mixer/{mod.rs,math.rs,model.rs,gesture.rs,meters.rs}`; modify `crates/vizdaw-app/src/{lib.rs,session.rs,testing.rs,store.rs,document.rs}`; tests `crates/vizdaw-app/tests/{mixer_model.rs,mixer_gesture.rs,strip_entry.rs,meters.rs}`.

**Interfaces — produces:** `DocSession` calls for mixer, levels, gesture (touch/value/release, gesture-as), automation state and mode, heard values, `run_mixer`, add/new-from/remove bus, rename strip, set output, strip colour, release channel touches, surface selection (exact names per core-ffi; FakeSession scripts and records each); `MixerModel` per document (columns in Swift order with ids `s<id>`/`d<id>`/`v<id>`, `StripEntry`, `MixerMath` (fader/pan/level formatting and parsing with the strict grammar), refetch tolerant of Busy, `touch/move/release/click` with the tracker and ⌥ rules, typed values, live/heard/core values, selection with `set_surface_selection`, rename, add bus, set output, placements set with last-placement release); meter state and ballistics fed by a `tick(now)`.
- [ ] **Step 1: failing tests** ported from Swift `MixerModelTests`, `MixerGestureTests`, `StripEntryTests`, `MixerMathTests` and the meter ballistics; plus `a_bare_press_sends_nothing`, `a_busy_release_is_retried`, `the_last_placement_releases_open_drags`, `a_second_document_has_silent_meters`.
- [ ] **Step 2–4:** fail → implement → `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy pass. **Step 5: commit** `core-app: mixer session calls and the mixer model`.

### Task 2: core-app + control — view, pane, window, automation and mixer commands

**Files:** modify `crates/vizdaw-app/src/{commands.rs,editor/layout.rs (helpers only)}`, `crates/vizdaw-gpui/src/{control.rs,menus.rs}`; tests `crates/vizdaw-app/tests/view_commands.rs`, `crates/vizdaw-gpui/tests/control.rs`.

**Interfaces — produces:** `ui.view_arrange` (⌃1), `ui.view_mix` (⌃2), `ui.toggle_left`, `ui.toggle_right` (`ui.toggle_bottom` exists), `ui.pane {pane, view, shown, size, main}` with the Swift validation and status ("right markers 300 shown"-style), `window.mixer`, `automation.set_mode`, `automation.gesture`, `automation.release_all`, `mixer.new_bus`, with their Swift refusals; `state.get mixer` (via `result_json`), `state.get layout`, `state.get automation`; View and Window menu items; `state.get windows` reports kind `mixer` for Mixer windows (Task 6 makes them).
- [ ] **Step 1: failing tests** from Swift `MixerCommandsTests`, `AutomationCommandsTests`, `ViewCommandsTests`, and the GPUI control tests for e2e `window_views_and_panes_over_the_channel` hostile `ui.pane` cases and `touch_writes_a_fader_over_the_channel`'s command part.
- [ ] **Step 2–5:** as Task 1; commit `core-app: view, pane, automation and mixer commands`.

### Task 3: GPUI — the pane host and the Mix view

**Files:** create `crates/vizdaw-gpui/src/panes.rs`; modify `crates/vizdaw-gpui/src/{shell.rs,detail_pane.rs,transport_bar.rs}`; tests `crates/vizdaw-gpui/tests/panes.rs`.

**Interfaces — produces:** the window laid out from `layout.views` for the current main view: left, centre (arrangement or the mixer in the Mix view), right and bottom panes per `PaneGeometry` (divider 1 px, centre minimum 240×160, Swift size ranges, hide-to-fit order for the frame only), dividers that drag-resize (size applied at drag end), each pane's picker (labels per Swift), the bottom pane's existing editors plus a "Mixer" view, notes for panes not built yet; the transport bar's `main_view` and `panes` items (labels "Arrange"/"Mix", "Left Pane", "Bottom Pane", "Right Pane"). The existing bottom detail pane becomes the host's bottom pane.
- [ ] **Step 1: failing tests:** `the_mix_view_hides_the_arrangement`, `panes_follow_the_layout`, `a_small_window_hides_panes_for_the_frame_only`, `a_divider_drag_clamps_and_persists`, `pane_pickers_choose_views`, `the_transport_bar_switches_views_and_panes`, and the existing detail-pane tests unchanged.
- [ ] **Step 2–5;** commit `gpui: the pane host and the Mix view`.

### Task 4: GPUI — mixer controls

**Files:** create `crates/vizdaw-gpui/src/controls/{mod.rs,fader.rs,knob.rs,number_box.rs,toggle.rs,menu_face.rs,meter.rs}`, `crates/vizdaw-gpui/src/strip_palette.rs`; tests `crates/vizdaw-gpui/tests/controls.rs`.

**Interfaces — produces:** reusable controls driven by the Task 1 model: fader (vertical, scale marks, drag with ⌥ fine and ⌥-at-press alone, double-click reset, keys per Swift), knob (pan; drag, ⌥ fine, double-click centre), number box (TextInput; strict grammar via `MixerMath`; Return commits, Escape restores, refusal → status), toggle (M/S/R/I with state colours), menu face, meter canvas (one canvas per strip, bands per Swift); each records role/label/value per the Global Constraints. Window deactivation and unmount release an open drag.
- [ ] **Step 1: failing tests:** `a_fader_drag_is_one_gesture`, `a_bare_press_sends_nothing`, `option_drags_fine`, `a_double_click_resets_without_a_touch_left_open`, `deactivation_releases_the_drag`, `the_number_box_takes_typed_values_and_refuses_bad_ones`, `the_knob_centres_on_double_click`, `controls_are_in_the_tree_with_their_values`.
- [ ] **Step 2–5;** visual check by the controller; commit `gpui: mixer controls`.

### Task 5: GPUI — the channel strips and the mixer view

**Files:** create `crates/vizdaw-gpui/src/mixer_view.rs`; modify `crates/vizdaw-gpui/src/{panes.rs,shell.rs,menus.rs}`; tests `crates/vizdaw-gpui/tests/mixer.rs`.

**Interfaces — produces:** `MixerView` (used by the Mix view and the bottom pane): strip columns in model order, each with the header (colour bar, name, kind label, click selects, ⇧/⌘ extend, double-click rename), pan knob (not on master), fader with scale and meter, level number box, M S R I toggles (master: no Solo, spacer kept), automation mode menu, output menu (Master and buses; not on master), solo-dim at 50 %, selected edge; rows 6b fills left blank so faders line up (`StripLayout::split`, explicit heights); the bar with "Add Bus" and the DSP load text; master after the divider; horizontal scrolling with only visible strips recorded.
- [ ] **Step 1: failing tests:** `the_mix_view_shows_a_strip_per_track_and_the_master`, `strip_labels_and_values_match_swift`, `a_fader_drag_moves_the_strip`, `mute_and_solo_toggle`, `solo_dims_the_others`, `the_output_menu_routes_to_a_bus`, `add_bus_adds_a_strip`, `rename_by_double_click`, `selection_reaches_the_core`, `a_strip_that_vanishes_releases_its_drag`, `many_strips_record_only_the_visible_ones`.
- [ ] **Step 2–5;** commit `gpui: channel strips and the mixer view`.

### Task 6: GPUI — meters, heard values, the Mixer window and e2e

**Files:** modify `crates/vizdaw-gpui/src/{mixer_view.rs,shell.rs,app_state.rs,lib.rs,control.rs}`, `crates/vizctl/tests/e2e_gpui.rs`; tests `crates/vizdaw-gpui/tests/{mixer.rs,window.rs}`.

**Interfaces — produces:** the 30 Hz meter and heard-value poll (only while a placement is visible; per-frame drawing decays by elapsed time), the Mixer window (second window kind per M5, own focus, transport bar without document-only items, closing releases only its own placement's drags), `state.get windows` kind `mixer`; a frame-time measurement hook used by the controller for M1. New e2e (GPUI ports): the mixer parts of `touch_writes_a_fader_over_the_channel`, `window_views_and_panes_over_the_channel` (views and panes; the markers/mixer-window steps that need later slices dropped and noted), `groups_vcas_and_transport_bar_over_the_channel`'s transport-bar and strip parts that need no groups/VCAs, and `midi_mix_reaches_state_get` only if it needs no MIDI strips (else noted for 6b).
- [ ] **Step 1: failing tests:** `meters_decay_and_fall_silent`, `heard_values_follow_while_playing`, `the_mixer_window_opens_and_reports_its_kind`, `closing_the_mixer_window_keeps_the_mix_views_drag`; then the e2e tests (`#[ignore]`).
- [ ] **Step 2–5;** commit `gpui: meters, the mixer window and mixer e2e`.
- [ ] **Step 6 (controller):** run e2e on the Mac and Linux; look at the Mix view and the Mixer window on the Mac; measure 64 strips at 60 fps (M1) and record the numbers; tick the checklist rows.
