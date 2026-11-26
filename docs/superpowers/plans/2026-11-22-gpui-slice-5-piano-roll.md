# 27.1.0 Slice 5: The Piano Roll Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app has the Piano Roll in the bottom detail pane (⌘1): a keyboard strip, pitch rows, grid, notes and playhead on its own time axis; selecting (click, ⇧-toggle, rubber band), drawing with the Pencil, erasing (click and swipe), moving, ⌥-copying, transposing and resizing notes with snap; Delete; auditioning notes and keys; the Tracks and Insert Into menus; and the arrangement's Pencil tool opening the roll — all driven over the control channel and matching the Swift app's strings, undo labels and `ui.json` layout.

**Architecture:** As in slices 2–4: behaviour lives in `vizdaw-app`. A `piano_roll` module holds `PianoRollSettings` (the `ui.json` `pianoRoll` key), `PianoRollModel` (shown lanes, edit lane, notes, selection), the pure geometry (`PitchAxis`, hit tests, `NoteDrag`, `Pencil`, `Eraser`, rubber band, a11y labels) and the pencil-opens-roll rule; `DocumentModel` gains `(seq, lane)` reads keyed by snapshot version and an audition model. `vizdaw-gpui` mounts a `PianoRoll` view in the detail pane and feeds `ToolMachine` events into the model, as the arrangement does since slice 4.

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin (zed rev `f4fe0be1`, `font-kit` on); `core-ffi`, `core-control`, `core-model`, `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md` (§4, §8, §10). Parity checklist: `docs/superpowers/specs/2026-11-22-parity-checklist.md`. Slice 4 plan and Outcome (rulings this slice revisits: double-click opens List while it is the only editor; the pane picker shows `last_kind`): `docs/superpowers/plans/2026-11-22-gpui-slice-4-block-editing-list.md`. Swift reference: `app/Sources/VizDAWEditors/{PianoRollView,PianoRollContainer,PianoRollGeometry,PianoTools,KeyboardView,EditorCanvas}.swift`, `app/Sources/VizDAW/PianoRollPane.swift`, `app/Sources/VizDAWAppModel/{PianoRollModel,EditorModel+Pencil,EditorModel+Audition,EditorLayout}.swift`. Research notes with file:line citations, exact strings, constants and the Swift test names: the slice 5 inventory (controller scratch; the relevant sections are copied into each brief).

## Global Constraints

- Song format unchanged (v11). `ui.json` stays compatible both ways: `pianoRoll` is written with Swift's keys as a struct merge; the fields this slice does not use (`expression`, `showRegions`, `showExpression`) and any unknown keys pass through a save unchanged; `detail.lanes` is written in Swift's shape.
- `vizdaw-app` never depends on GPUI.
- Command IDs, titles, keys and params come from `core_control::registry::COMMANDS`. Never re-typed.
- Exact Swift strings for every status, refusal, undo label ("Insert Note", "Erase Notes", "Move Notes", "Copy Notes", "Transpose Notes", "Resize Notes", core "Delete Events"), tree label and audition hint (quoted in the inventory sections each brief carries).
- Out of this slice (menu items disabled, channel `not_yet`): the region strip and the expression pane under the roll (their toggles absent, no space reserved), Scale Time (⌘ + end-zone drag), exact mode and the Modify panel, step entry, nudges, torn-off windows, MPE, the Strip Chart and Drum Grid.
- Real-time rule: nothing on the interface thread blocks on the engine; note reads are cached per snapshot version; the roll draws only the notes in view.
- Single-key shortcuts carry `!TextInput`. While the roll has the keyboard, Delete/Forward Delete delete the selected notes before the keymap (the registry binds Delete to `step.delete_last`).
- Every clickable control records role, label, bounds and `selected` in the registry while drawing. Labels shared with the arrangement tool bar (Tool, Snap, Zoom) are scoped by their container path, not renamed.
- Subagents: no GUI, no computer-use tools, never `dangerouslyDisableSandbox`. Visible checks and e2e runs are done by the controller (Mac) or over SSH on the owner's Linux box.
- Banned vendor/DAW names stay out of repo text. UI says "track", never "lane". Never commit `.DS_Store`, `.superpowers/`, generated FFI files or brand assets.
- CI: `cargo fmt --all -- --check`, clippy `-D warnings` (with and without `--features screenshot`), tests on macOS and Linux.
- Commit trailers: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.

## Decisions (controller, from the inventory §8)

- D1: `(seq, lane)` reads (`lane_rows`, `lane_events`) cached on `DocumentModel` keyed by snapshot version, for any sequence, not only the shown one.
- D2: Double-click a block opens `last_kind` (Swift; default Piano Roll) — the slice 4 "List while it is the only editor" ruling ends here; the slice 4 e2e test changes to expect the Piano Roll (and a second test path opens the List explicitly). The pane picker lists List and Piano Roll.
- D3: The "Tracks" menu (which tracks a sequence target shows) and "Insert Into: <name>" (the pencil's edit track) are in, writing `detail.lanes` and the edit lane as Swift does.
- D4: Region strip and expression pane: not drawn, no space reserved, toggles absent; their `ui.json` values kept.
- D5: ⌘ + end-zone drag (Scale Time) sends nothing this slice (the drag is ignored, no gesture opens); ledgered as a gap until the Modify slice.
- D6: `TimeAxis.origin` stays integer ticks; sub-tick differences from Swift's Double origin are accepted.
- D7: Tool/Snap/Zoom labels in the roll bar are the Swift labels; e2e and channel lookups scope by container.
- D8: Key, key-label and the 8 track swatch colours are fixed constants in the GPUI palette until themes land.
- Nudges stay out (they need Nudge settings from the Modify slice).

## Review Focus

1. A note drag that ends refused, Busy, or with its track deleted: no partial edit stays, at most one undo step with the right label, Escape and window loss cancel (also a Busy-pending final) and silence the audition — Tasks 2 and 5.
2. Transpose and resize at the edges: transposing past pitch 0 or 127 clamps the whole selection so no note leaves the range; a resize with zero horizontal motion sends nothing; a note never shrinks below one tick; pencil length never zero — Task 2.
3. Audition never sticks: every press that sounds a note is silenced on release, Escape, window loss, a refused gesture, closing the document; a Busy note-off is retried and after the retries the Swift hint shows once — Task 3.
4. `ui.json` `pianoRoll`: hostile values (`rowHeight` 999, `{"pianoRoll": 7}`, NaN zoom) clamp or fall back; unknown and out-of-slice fields survive a save — Task 1.
5. Many notes (a 100 000-event track): the roll draws and records only notes in view, and hit tests stay fast — Tasks 2 and 4.

---

### Task 1: core-app — piano roll settings, model and reads

**Files:**
- Create: `crates/vizdaw-app/src/piano_roll/{mod.rs,settings.rs,model.rs}`
- Modify: `crates/vizdaw-app/src/{lib.rs,document.rs,document/editing.rs,editor/layout.rs,editor/detail.rs,editor/mod.rs,session.rs,testing.rs,commands.rs}`
- Test: new `crates/vizdaw-app/tests/{piano_roll_model.rs,piano_roll_layout.rs,pencil_arrange.rs}`

**Interfaces:**
- Produces: `PianoRollSettings { show_keyboard, row_height, ticks_per_point, expression (opaque JSON), show_regions, show_expression }` with Swift defaults and clamps (inventory §2) in `EditorLayout` (tolerant decode, struct merge, pass-through); `detail.lanes` read/write helpers over `DetailState`; `DocumentModel::{lane_rows(seq), lane_events(seq, lane)}` cached per snapshot version for any sequence (D1); `PianoRollModel` (shown lanes for the target, edit lane, chosen lanes, colour index, notes in view order, `select(notes, adding)`, `toggle`, `delete_selected` ("Delete Events" via `edit`), `toggle_shown`, `set_edit_lane`); `pencil_opens_piano_roll(seq, lane, tick) -> bool` (false and no change for audio tracks and unknown lanes; switches the tool to Pencil, opens the roll on the block/lane, sets a reveal with a new serial); `editors.piano_roll` in `EDITOR_COMMANDS` (`show_detail(PianoRoll)`); D2's double-click rule.
- [ ] **Step 1: Write the failing tests**, ported from Swift: `PianoRollModelTests` (all but torn-window cases), `PencilArrangeTests` (headless), plus layout tests `piano_roll_settings_clamp_and_fall_back` (rowHeight 999 → 40, `{"pianoRoll":7}` → defaults, NaN zoom) and `piano_roll_keys_survive_a_save` (expression/showRegions/showExpression and an unknown key).
- [ ] **Step 2: Run** `cargo test -p vizdaw-app` → fail. **Step 3: Implement.** **Step 4: Run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy → pass.
- [ ] **Step 5: Commit**: `git commit -m "core-app: piano roll settings, model and reads"`.

### Task 2: core-app — piano roll geometry and note drags

**Files:**
- Create: `crates/vizdaw-app/src/piano_roll/{geometry.rs,drag.rs}`
- Test: new `crates/vizdaw-app/tests/{pitch_axis.rs,piano_roll_geometry.rs,note_drag.rs}`

**Interfaces:**
- Produces: `PitchAxis` (row height, vertical offset, `top(pitch)`, `pitch_at(y)`, keep-middle on row-height change, clamps), note rect and hit test (pitch row first, then rect; latest in time wins), zones (start/middle/end) with Swift's widths, `NoteDrag { events, label, time_delta, raw_semitones, semitones (clamped so the selection stays in 0..=127), duration_delta (zero motion → none; end ≥ tick + 1), command }`, `Pencil { step, length, command }` (velocity `100 << 9`), `Eraser::crossed(from, to, notes)`, rubber band `notes_in(rect)` (intersects), cursor kinds, `a11y_label` (inventory §1.8 exact), keyboard labels and black-key test; the drag glue driving `ToolMachine` + gestures for the roll (as `document/block_drag.rs` does for blocks), with Escape cancelling a Busy-pending gesture.
- [ ] **Step 1: Write the failing tests**: port `PitchAxisTests`, `PianoRollGeometryTests` verbatim (numbers in inventory §4) and the `PianoRollViewTests` gesture cases as model tests (select, ⇧-toggle, rubber band, pencil click and drag, eraser click and swipe, move/copy/transpose/resize with snap and without, edge clamps, zero-motion resize, Escape mid-drag, refused/Busy final, "Finish the drag first."), plus `hit_testing_a_long_track_is_quick`.
- [ ] **Step 2–4: Run → fail, implement, run** `cargo test -p vizdaw-app -p vizdaw-gpui` and clippy → pass.
- [ ] **Step 5: Commit**: `git commit -m "core-app: piano roll geometry and note drags"`.

### Task 3: core-app — audition

**Files:**
- Modify: `crates/vizdaw-app/src/{session.rs,testing.rs,document.rs,editor/gesture.rs (tick), piano_roll/model.rs}`; create `crates/vizdaw-app/src/audition.rs`
- Test: new `crates/vizdaw-app/tests/audition.rs`

**Interfaces:**
- Produces: `DocSession::audition(dest, pitch, velocity, on) -> Result<bool, VizError>` (+ `FakeSession` log `("on"|"off", pitch, dest)` and scripting), `Audition { on(note, velocity), off(), off_all(), tick() }` with the pending-off retry (120 ticks) and the two hint strings shown once ("Notes sound once the audio engine runs: press Play once.", "A note may still be sounding — press Stop."), `PianoRollModel::{sound(pitch, velocity, dest), silence()}`; `DocumentModel::idle` drives the retries; closing a document silences all.
- [ ] **Step 1: Write the failing tests** from `KeyboardTests` and the audition cases of `PianoRollModelTests` (`sounding_a_new_pitch_ends_the_old_one`, `a_lane_without_destination_is_silent`, hint once, Busy off retried then the hint, close silences).
- [ ] **Step 2–4.** **Step 5: Commit**: `git commit -m "core-app: audition"`.

### Task 4: GPUI — the piano roll view

**Files:**
- Create: `crates/vizdaw-gpui/src/piano_roll.rs`
- Modify: `crates/vizdaw-gpui/src/{detail_pane.rs,menus.rs,shell.rs,control.rs,arrangement.rs (palette)}`
- Test: new `crates/vizdaw-gpui/tests/piano_roll.rs`

**Interfaces:**
- Produces: the roll mounted for `DetailEditorKind::PianoRoll` (picker lists List and Piano Roll); the bar (Tool picker, Keyboard toggle, "Tracks" menu, "Insert Into: <name>" menu, "Shorter Rows", "Taller Rows", "Zoom Out", "Zoom In"); a 64 px side column (keyboard when shown, blank when hidden); 22 px ruler (reuse the arrangement's), pitch rows with black-key shading, grid, notes in track colours with the selected stroke, playhead from `Clock`; own time axis (96 ticks/pt default from settings) and vertical pitch scroll, wheel and ⌘-wheel/pinch zoom, ±1.5 zoom buttons, auto-centre on open and on target change, pencil reveal; registry: group "Piano Roll" with a `button` per visible note (label from `a11y_label`, `selected`), group "Keyboard". `editors.piano_roll` (⌘1) routed. Read-only this task (gestures in Task 5).
- [ ] **Step 1: Write the failing tests**: `editors_piano_roll_opens_the_roll`, `the_roll_lists_its_notes_in_the_tree`, `the_keyboard_toggle_keeps_the_column`, `rows_and_zoom_change_and_persist`, `the_roll_centres_on_its_notes`, `a_long_track_draws_only_the_notes_in_view`, `tracks_and_insert_into_menus_choose_lanes`.
- [ ] **Step 2–4** (plus clippy with and without `--features screenshot`). **Step 5: Commit**: `git commit -m "gpui: the piano roll view"`.

### Task 5: GPUI — piano roll editing and audition

**Files:**
- Modify: `crates/vizdaw-gpui/src/{piano_roll.rs,shell.rs,control.rs}`
- Test: `crates/vizdaw-gpui/tests/piano_roll.rs`, `crates/vizdaw-gpui/tests/control.rs`

**Interfaces:**
- Produces: mouse editing through Task 2's glue (click, ⇧-click, rubber band, pencil click/drag, eraser click/swipe, move/⌥-copy/transpose/resize, cursors per zone), Escape and window-loss cancel, Delete/Forward Delete while the roll has the keyboard (before the keymap; also over the channel like slice 4's list rule), audition on press and on the keyboard strip with silence on release/Escape/window loss/close, the gesture `tick` and audition retries from the poll; channel clicks on note elements select them.
- [ ] **Step 1: Write the failing tests**: `a_note_click_selects_and_shift_toggles`, `a_rubber_band_selects`, `the_pencil_draws_a_note` ("Insert Note"), `the_eraser_swipes` ("Erase Notes"), `dragging_moves_copies_transposes_and_resizes`, `escape_mid_drag_changes_nothing_and_silences`, `delete_deletes_the_selected_notes`, `a_key_press_sounds_and_release_silences`, `window_loss_silences`, `channel_delete_goes_to_the_roll`.
- [ ] **Step 2–4.** **Step 5: Commit**: `git commit -m "gpui: piano roll editing and audition"`.

### Task 6: GPUI — the Pencil in the arrangement and the e2e tests

**Files:**
- Modify: `crates/vizdaw-gpui/src/{arrangement.rs,detail_pane.rs}`, `crates/vizctl/tests/e2e_gpui.rs`
- Test: `crates/vizdaw-gpui/tests/arrangement.rs`

**Interfaces:**
- Produces: a Pencil click on a MIDI/instrument track in the arrangement opens the roll (`pencil_opens_piano_roll`), consuming the click only on success; double-click opens `last_kind` (D2); the block menu gains "Open in Piano Roll" before "Open in List". New e2e: `gpui_the_piano_roll_draws_selects_and_deletes` (⌘1 on the demo → `wait detail_kind piano_roll`; a note element click → `state.get selection` holds the note and its lane; Delete → undo "Delete Events"; `edit.undo`), `gpui_the_pencil_opens_the_roll` (tool Pencil via its picker element, a point click on the Chords row → `detail_kind piano_roll`, the roll's notes in the tree); update `gpui_blocks_select_and_open_in_the_list` for D2.
- [ ] **Step 1: Write the failing tests**: `a_pencil_click_opens_the_roll`, `a_pencil_click_on_an_audio_track_does_nothing`, `double_click_opens_the_last_used_editor`, `the_block_menu_opens_the_piano_roll`; then the e2e tests (`#[ignore]`).
- [ ] **Step 2–4.** **Step 5: Commit**: `git commit -m "gpui: the pencil opens the piano roll; piano roll e2e"`.
- [ ] **Step 6 (controller):** run the GPUI e2e suite on the Mac and Linux → all pass; look at the roll on the Mac; tick the checklist rows (`editors.piano_roll`, the Piano Roll window row's detail part, README piano-roll steps that pass).

## Outcome (2026-11-22)

All six tasks done (commits 23863821..61cea005). Evidence: workspace tests 4474 passed on macOS and 4343 on Linux; clippy `-D warnings` and fmt clean; GPUI e2e 13/13 on macOS and Linux (XWayland), including `gpui_the_piano_roll_draws_selects_and_deletes` and `gpui_the_pencil_opens_the_roll`; a visual check of the roll on the Mac.

Final review fixes (61cea005): the roll only claims a gesture its own press opened (a plain note click no longer lets leaving the roll abort an arrangement drop pending on Busy); a saved last editor without a view yet (Strip Chart, Drum Grid) opens the Piano Roll and keeps the saved value.

### Rulings

- Task 1: Ruling: shown-sequence lane_events renamed shown_lane_events (delegates) to free the name for lane_events(seq, lane) — none
- Task 1: Ruling: roll operations are DocumentModel::roll_* (List pattern); open_block_detail (D2) in the model, GPUI call site switched in Task 6 — none
- Task 2: Ruling: code drafted before tests; compensated with six mutation checks that each failed a test — reviewer to judge — weak tests could hide bugs
- Task 2: Ruling (D5): ⌘ + end-zone drag selects the note (Swift) but sends no gesture or edit until Scale Time lands — gap — a ⌘-drag silently does nothing
- Task 3: Ruling: roll_sound/roll_silence/roll_audition on DocumentModel (need the session); PianoRollModel.sounding is the one source of truth — none
- Task 3: Ruling: implementation before tests; substitute RED by removing the retry (4/16 failed) — reviewer judges — weak tests could hide bugs
- Task 4: Ruling: ruler/grid/playhead drawing extracted to a shared TimeLayer; list bar menu reused by the roll — avoids copies; arrangement and List tests unchanged — none
- Task 4: Ruling: roll page-follow respects layout.follow (Swift) — parity — none
- Task 5: Ruling: fast poll also bypasses the 50 ms idle throttle while wants_fast_poll — otherwise retries are no sooner — more idle calls while a note-off/final is pending
- Task 5: Ruling: eraser cursor is GPUI "not allowed" (no disappearing-item cursor) — platform — cosmetic
- Task 5: Ruling: closing/hiding the roll cancels only the roll's own pending gesture, not an arrangement one — each canvas owns its gesture — none
- Task 6: Ruling: a Pencil press-drag on a block does not drag it (Swift beginDrag guards tool == .arrow; inventory §1.9 was wrong) — parity — none
- Task 6: Ruling: control.rs and detail.rs tests updated to D2 (double-click → last kind) — follow-through — none
- Final: Ruling: declined — Escape on a focused roll cancels any open gesture incl. the arrangement's pending one (Swift contentCancel parity; Escape is explicit) — stands — a user pressing Escape in the roll also drops a pending block move
- Final: Ruling: declined — hiding the last shown track shows none until prune restores (Swift same) — stands — momentary empty roll
- Final: Ruling: declined — start-zone drag with no horizontal motion snaps an off-grid note (Swift same) — stands — a tiny unintended move
- Final: Ruling: declined — document close with a Busy-pending roll release goes through the shared store close path — stands, predates slice — see slice 4 ruling 23
- Final: Ruling: declined — eraser last point not re-mapped during auto-scroll (Swift same) — stands — cosmetic
- Final: Ruling: declined — runtime frame cost not measured — stands — performance follow-up

### Deferred minors

- Task 1: minor (deferred): read errors swallowed silently (Swift reportFromRead → status)
- Task 1: minor (deferred): track events keyed by global version, not per-track lane_revs (more re-reads after edits)
- Task 1: minor (deferred): NaN zoom clamp tested via sane() directly (JSON cannot carry NaN)
- Task 1: minor (deferred): refresh_lanes reports Busy as "Tracks Unavailable" alert
- Task 1: minor (deferred): while Busy persists the roll rebuilds on every roll_* call (one session call per missed track)
- Task 1: minor (deferred): a stored failure (empty) replaces the last good rows served if the next version is Busy
- Task 2: minor (deferred): roll_mouse_down computes the hit before applying a lost drag's Cancel (Swift same order)
- Task 2: minor (deferred): no note_drag tests for ⌥ middle-zone copy or a pencil click with no edit track (pure tests cover)
- Task 3: minor (deferred): a Busy note-off pending at document close is dropped with no retry/hint (Swift same; external MIDI could stay stuck)
- Task 3: minor (deferred): audition_on_at/audition_off_at lack doc comments
- Task 4: minor (deferred): the 8 track colours are placeholders (Swift reads them from design-token theme swatches) — themes slice (D8)
- Task 4: minor (deferred): no tests for roll playhead, page-follow, pinch
- Task 4: minor (deferred): first frame after mounting paints uncentred
- Task 4: minor (deferred): bar_menu/Entry shared but live in list_editor.rs with list-named ids
- Task 4: minor (deferred): allow(too_many_arguments) on detail_pane::render; keyboard label box 14 vs 11 pt
- Task 4: minor (deferred): saved_layout test writes a fixed shared temp path
- Task 5: minor (deferred): fast poll notifies/redraws at 60 Hz while a retry waits (notify only on state change)
- Task 5: minor (deferred): channel Delete rule unpinned until step.delete_last has a handler (step-entry slice adds the test)
- Task 5: minor (deferred): delete test does not press Delete while focus is elsewhere; eraser click test lacks the undo label assert
- Task 5: minor (deferred): cursor stale on tool change until pointer moves; key strip does not highlight the pressed key
- Task 5: minor (deferred): piano_roll.rs 1527 lines — split pointer/keyboard/lifecycle into piano_roll/editing.rs
- Task 5: minor (deferred): Escape with a held strip key and an open roll gesture cancels the gesture; key sounds until mouse-up
- Task 6: minor (deferred): roll e2e "note back" check waits only for the label to differ (later wait confirms)
- Task 6: minor (deferred): no test for a Pencil double-click on an audio track (falls through as Swift)
- Final: minor (deferred): hiding the roll after a new press does not abort the roll's own pending final (lands on tick; parity gap only)
- Final: minor (deferred): zoomed far out on a dense track every in-view note builds an a11y label + registry record per frame (pair with the per-version reads perf item)
- Final: minor (deferred): one very long note widens every candidates search
- Final: minor (deferred): centring can land on middle C after a Busy empty build
- Final: minor (deferred): selected HashSet rebuilt every frame

### Owner's manual checks

- Drag notes with the mouse: move, ⌥-copy, transpose (middle), resize (end), Escape mid-drag; check the undo names and that notes sound and stop.
- Pencil: click and drag in the roll; click a MIDI track in the arrangement with the Pencil (opens the roll scrolled to the bar).
- Eraser: click and swipe across notes.
- Hold a key on the keyboard strip, drag across keys, release outside the window (Mac and Linux): the note must stop.
- Play once before auditioning, then click notes: sound; with the engine stopped first: the hint shows once.
