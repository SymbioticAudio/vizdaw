# 27.1.0 Slice 10d: The Drum Grid (the editor, its bar, row headers and the Drum menu) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GPUI app has the Swift app's Drum Grid.
- Editors › Drum Grid (⌘4, Ctrl+4 off macOS) shows it in the bottom pane. The bottom pane's picker offers it, and a song saved with it as the last editor opens it.
- It shows the drum tracks of the target's sequence. Each track has a header band, then one band per row (a taller one when the row's velocity lane shows), then a band to drop a row on to give it a track of its own.
- Opening a MIDI or Instrument track that has no drum layout gives it one (one undo step). A sequence with no drum track shows "Choose a Track" with a button per MIDI or Instrument track.
- It has a bar: Paint/Erase, View, Step, Spacing, Length, Vel, Add Row, and the current track's menu with its play quantize and nudge.
- The canvas is a step matrix or a timeline. A click toggles a hit. A drag paints or erases a run along its row, and is one undo step. Escape leaves none. A shown velocity lane sets each new hit's velocity from the pointer's height.
- The row headers show each row's name, note and destination. A click selects a row, a double-click renames it, and a drag reorders it, moves it to another drum track or splits it into a new track. The header menu has Rename…, Show Velocity, Split Row to Own Track and Remove Row.
- The Drum menu (Add Drum Row, Show Velocity, Split Row to Own Track, Remove Drum Row, Timeline View) acts on the grid's selected row, from the menu, keys and the control channel.
- The bar's settings are saved with the project.

**Architecture:**
- **The core is complete** (plan 4e).
  - `core_ffi::drum` has the records: `DrumLaneRow`, `DrumRowRow`, `DrumHitRow`, `DrumEdit` (with `Paint`), `DrumGridSettingsRow`, `DrumGridLayoutRow`, `DrumBandRow`, `DrumSpotRow`, `DrumCellRow`, `DrumStrokeRow`, `QuantizeChoiceRow` and `DrumNewRow`.
  - It also has the grid's logic, as the plain functions Swift called (Ruling 1): `drum_grid_settings_sane`, `drum_grid_layout`, `drum_grid_spot`, `drum_row_velocity`, `drum_grid_cells`, `drum_stroke_begin`, `drum_stroke_extend`, `drum_stroke_command`, `drum_row_drop`, `drum_quantize_choices`, `drum_quantize_title`, `drum_nudged` and `drum_next_row`.
  - `Session::drum_lanes(seq)` is at `core-ffi/src/session.rs:2559`. `LaneEdit::SetPlayQuantize` and `LaneEdit::SetPlayShift` are at `core-ffi/src/edit.rs:415,424`.
  - The registry has `editors.drum_grid` and the five `drum.*` commands (`core-control/src/registry.rs:2838,3014-3039`).
  - `DETAIL_KINDS` already has "drum_grid".
- **App (`vizdaw-app`).** Everything Swift kept in `DrumGridModel`, `DrumGridSettings` and `DrumCommands` becomes pure code with unit tests, in a new `drum_grid` module:
  - `settings.rs`: the bar's settings and length choices.
  - `text.rs`: the tree labels, header text and messages.
  - `model.rs`: the lanes read, the layout, selection and opening a track.
  - `stroke.rs`: strokes as gestures.
  - `rows.rs`: row and track edits.
  - `commands.rs`: the Drum menu.
- **The layout** gains `drumGrid` (`editor/layout.rs`).
- **The session seam** gains one call, `drum_lanes`.
- **GPUI.** A new `drum_grid/` module:
  - `mod.rs`: the view state, the bar, the empty state and `render`.
  - `canvas.rs`: the canvas, its pointer and its tree.
  - `headers.rs`: the row headers, drag, rename and menu.

  There is one instance on `DocumentWindow`, the bottom pane's. The menus gain Editors › Drum Grid and a Drum menu. Views draw the model and send intents to it; they hold no rules beyond layout.
- **No Modify, Exact Mode or Strip Chart tie-in.** Swift's grid opens its gestures with `origin: .other` (`M/DrumGridModel.swift:263`). No Modify panel, Exact Mode or Strip Chart file mentions the drum grid (Ruling 4).

**Tech Stack:** Rust 2024; GPUI at the slice 1 pin; `core-ffi` (`drum`, `edit`, `session`), `core-control`, `core-engine` fixtures (e2e), `vizctl`.

**Spec:** `docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md`.
- Drum grid design: `docs/superpowers/specs/2026-10-16-editors-design.md` §4e (lines 113-117), and `docs/superpowers/plans/2026-10-18-editors-drum-grid.md` (Plan 4e Rulings 1-18).
- Parity rows in `docs/superpowers/specs/2026-11-22-parity-checklist.md`:
  - `editors.drum_grid` (:75)
  - `drum.add_row`, `drum.show_velocity`, `drum.split_row`, `drum.remove_row`, `drum.timeline` (:130-134)
  - README "Drum grid (plan 4e)" (:507)
  - e2e `drum_grid_paints_and_erases_by_element` (:545)
- Swift sources, all at tag `swift-parity-baseline`. Read them with `git show "swift-parity-baseline:<path>"`, quoted (zsh reads `:a` as a history modifier).
  - `S/` = `app/Sources/VizDAW/`, `M/` = `app/Sources/VizDAWAppModel/`, `E/` = `app/Sources/VizDAWEditors/`, `T/` = `app/Tests/VizDAWAppModelTests/`.
  - The files: `M/DrumGrid.swift`, `M/DrumGridModel.swift`, `M/DrumCommands.swift`, `M/DocumentModel.swift:393-428`, `M/EditorModel.swift:413`, `S/DrumGridPane.swift`, `S/AppCommands.swift:311-318`, `E/DrumGridView.swift` and `E/DrumRowHeaderView.swift`.
  - The tests: `T/DrumGridModelTests.swift` and `T/DrumGridViewTests.swift`. The Swift e2e is `crates/vizctl/tests/e2e.rs:343-408`.
  - The README's manual steps 87-98 (`app/README.md:799-877`). Step 93 is stale (Ruling 3).
  - Every string below is copied from those files; cite the file:line next to each constant.

## Global Constraints

- **Rules from slices 7a–10c that still apply:**
  - The Swift app is retired: match its behaviour, but don't keep compatibility with it.
  - Use Swift's exact strings, citing file:line. This includes the "·" in "Drums · Kit" and the "…" in "Rename…".
  - The UI says "track", never "lane". Swift's UI strings already say "track" ("Split Row to Own Track", "Drop a row here to give it a track of its own"), except one help text, which Ruling 16 rewords.
  - No vendor or DAW brand names in repo text (Opcode, Logic, Apple beyond platform facts, Midas, Soundcraft, NI/Native Instruments, Microsoft, Ableton, Steinberg/Cubase, ROLI). Swift's doc comment "A Pulse-style drum grid" is not copied. Licence notices are exempt.
  - GPUI tests press `secondary-` keys.
  - Linux CI runs the GPUI tests and the e2e on the offline backend, and every 10d test must pass there.
- **Keys off macOS** follow the codebase's existing mapping (10b Outcome 3):
  - ⌘4 is Ctrl+4. No ⌃4 chord exists, so nothing yields.
  - The Drum menu has no default keys (Plan 4e Ruling 13).
- **Before every commit:** `export PATH="$HOME/.cargo/bin:$PATH"`, then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the tests the task names. CI stops at Format.
- **Running the e2e on the Mac:** `cargo build -p vizdaw-gpui -p vizdaw-scan`, then `VIZDAW_E2E_APP=target/debug/vizdaw-gpui VIZDAW_AUDIO=offline cargo test -p vizctl --test e2e_gpui -- --ignored --exact <name>`.
- **Safety:**
  - Never launch the app against the real home directory: tests use `temp_home()`.
  - No GUI or computer-use tools.
  - Never kill a process you did not start.
  - Never commit `.DS_Store`, `.superpowers/`, generated FFI files, `target/` or brand assets. This slice adds no FFI.
- **Commit trailers:**
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
  ```
- **Seams:** no app or GPUI unit test opens a real session. They use `FakeSession`, with `drum_fake()` (Task 1) as Swift's `drumFake()` (`T/DrumGridModelTests.swift:18-41`). The stroke's extend gets one test seam, `set_drum_extend_for_test` (Task 3).
- **No wall-clock timing in tests.** A CI flake came from one (`automation::a_landed_cycle_is_heard_soon_after_the_wrap`, backlog row "Flaky dls test…").
  - No test asserts on elapsed time, sleeps, or relies on a frame arriving within a deadline.
  - GPUI tests advance with `run_until_parked`, and app tests with explicit calls.
  - The e2e polls with its existing `wait_in_tree`/`wait_for_*` helpers up to `WAIT_MS`. It never asserts a duration.
- **The look:** theme tokens only (`crate::arrangement::colors`, `transport_bar::colors`, `strip_palette::tokens()`). `no_source_file_keeps_a_colour_literal` stays green.
- **Nothing per frame touches the session** beyond what the piano roll already reads:
  - The drum lanes are read once per snapshot version and sequence (`M/DrumGridModel.swift:88-108`).
  - The layout is computed once per row shape and settings (`:134-141`).
  - Cells and tree items are built only for the view (at most 2 048 in the tree, `E/DrumGridView.swift:27`).
- **Out of scope:**
  - Torn-off editor windows: `editors.drum_grid_window` (⌥⌘4), the rows "Torn-off editor: Drum Grid" (:386) and "Open Drum Grid in Window" (:80), Swift's `activeDrumGrid` slot choice (`M/DocumentModel.swift:397,424-428`), and the Swift tests `tornWindowsAndTheKindSwitchPrepareTheLaneToo`, `aWindowThatNeverDrewSnapshotsItsRowsAndCells` and the torn-window half of `showsTheDrumLanesOfTheShownSequence`. These come in a later slice, and `editors.drum_grid_window` keeps answering `not_yet` until then.
  - The Audio Editor (`editors.audio_editor`): a later slice.
  - README steps 94-95's engine behaviour (swing and clean starts). It is the core's and was tested in plan 4e Task 2. This slice only sets play quantize and nudge from the bar.
- **Review pace:** fix crashes, data loss and wrong overwrites. Log other edge cases to the backlog.

## Review Focus

1. **A stroke that never ends cleanly.** Each of these leaves no open session edit and no undo step:
   - Escape mid-drag
   - a mouse-up that never arrives, so that a new press cancels the old stroke
   - the detail switched to another editor, or closed, mid-stroke
   - the window losing focus, and the document closing
   - a failed extend on the release

   Two more cases open nothing at all: another drag still finishing, which refuses the stroke with "Finish the drag first.", and an eraser click on an empty cell.

   Tests: `escape_and_refusals_leave_no_undo_step`, `a_new_press_cancels_a_stroke_whose_release_was_lost`, `a_grid_that_stops_showing_drops_its_stroke` and `a_failed_extend_on_mouse_up_aborts_the_gesture` (Task 3); `leaving_the_pane_mid_stroke_leaves_no_undo_step` (Task 6); `escape_during_a_row_header_drag_drops_nothing` (Task 7).
2. **Edits aimed at a row that moved (wrong overwrite).** The lanes can change under the grid: another edit, undo, or a Busy read followed by a fresh one.
   - A stroke paints the row it pressed, by track and row ID, never the row now at that index.
   - Indexes that no longer exist are refused, never a trap.
   - A failed read forgets the rows read before it.

   Tests: `a_stroke_paints_the_row_it_began_on_after_the_rows_reorder` (Task 3); `a_busy_read_then_a_fresh_one_does_not_trap` and `a_failure_forgets_the_rows_read_before_it` (Task 2).
3. **Opening the grid changes the document.** Opening a MIDI or Instrument track without a drum layout marks it (one undo step, `M/DrumGridModel.swift:393-399`). It must never do so for:
   - an audio track
   - a track that already has a layout
   - a sequence target
   - another editor kind

   It must also not mark the track again when the grid reopens on it.

   Tests: `opening_a_midi_lane_gives_it_a_default_layout` and `a_lane_with_a_layout_keeps_it_and_an_audio_lane_is_not_offered` (Task 2); `a_saved_drum_grid_last_editor_opens_the_grid` (Task 5).
4. **A hostile or old `ui.json`.** These read field by field and never panic: `{"drumGrid":{"view":"timeline","step":-5,"velocity":200,"rowHeight":1e9,"spacing":"x"}}`, a step of 1 or 10^12, a negative spacing, a zero duration and an unknown view. The grid draws with the lengths Rust paints with.

   Tests: `settings_decode_tolerantly` and `the_settings_survive_a_save` (Task 1).
5. **A busy session, per-frame reads, and extreme zoom.**
   - Busy keeps the lanes on screen. A failure shows once in the status line and is not retried at the same version.
   - A 20-move stroke reads the lanes at most once more.
   - At the widest zoom the tree stays at 2 048 items.
   - The velocity stepper stops at 1 and 127, and nudge stops at ±3 840 ticks.

   Tests: `busy_keeps_the_lanes_and_a_failure_shows_once` (Task 2); `a_stroke_reads_the_drum_lanes_once` and `the_tree_lists_at_most_2048_cells` (Task 6); `the_velocity_stepper_stops_at_its_ends` (Task 5); `lane_quantize_and_nudge` (Task 4).

---

### Task 1: The bar's settings, the tree labels, the saved layout, and the session seam

**Files:**
- Create: `crates/vizdaw-app/src/drum_grid/{mod.rs,settings.rs,text.rs}`
- Modify:
  - `crates/vizdaw-app/src/lib.rs`
  - `crates/vizdaw-app/src/editor/layout.rs`: `EditorLayout` 485-508, `Default` 527, `decode` 591, `entries` 633, `merge_key` 687.
  - `crates/vizdaw-app/src/session.rs`: the trait near 488, the live impl near 1041.
  - `crates/vizdaw-app/src/testing.rs`: `FakeSession`, `drum_fake()`.
- Test: `crates/vizdaw-app/tests/drum_grid.rs` (new)

**Interfaces:**
- Produces (ports `M/DrumGrid.swift`, `E/DrumGridView.swift:13-28`, `E/DrumRowHeaderView.swift:88,140-142,311-312`, `S/DrumGridPane.swift:139-159`):
  - **View:** `pub enum DrumView { Matrix, Timeline }`, serde lowercase, with `ALL`, `title()` ("Step Matrix", "Timeline", `:7-12`) and `row() -> DrumViewRow`.
  - **Settings:** `pub struct DrumGridSettings { pub view: DrumView, pub step: i64, pub spacing: i64, pub duration: i64, pub velocity: u8, pub row_height: f64, pub velocity_height: f64, pub ticks_per_point: f64 }`.
    - Its defaults: Matrix, PPQ/4, PPQ/4, PPQ/8, 100, 20, 28 and 96 (`:27-39`).
    - `ROW_HEIGHT_RANGE: RangeInclusive<f64> = 12.0..=64.` and `VELOCITY_HEIGHT_RANGE = 16.0..=96.`
    - `pub fn decode(&Value) -> Self` (`:47-69`):
      - Each missing or malformed field keeps its default.
      - The velocity is clamped to 1–127.
      - The heights go through `EditorLayout::sane` with their ranges, and the zoom with `TICKS_PER_POINT_RANGE`.
      - Step, spacing and duration then go through `core_ffi::drum::drum_grid_settings_sane`. On an `Err`, each is at least 1.
      - The JSON keys are Swift's: `view`, `step`, `spacing`, `duration`, `velocity`, `rowHeight`, `velocityHeight`, `ticksPerPoint`.
    - `row(&self) -> DrumGridSettingsRow`.
    - `pub const LENGTHS: [(&str, i64); 9]`: "1 Bar" 4q, "1/2", "1/4", "1/8", "1/8 Triplet" q/3, "1/16", "1/16 Triplet" q/6, "1/32", "1/64" q/16 (`:79-83`).
    - `lengths_including(current: i64) -> Vec<(String, i64)>`: adds `("‹n› ticks", n)` when `current` is not in `LENGTHS`.
    - `pub fn stepped_velocity(v: u8, up: bool) -> u8`: up is `min(v + 4, 127)`, down is `max(v, 5) − 4` (`S/DrumGridPane.swift:117-118`).
  - **Text** (`text.rs`):
    - `pub const MAX_TREE_ITEMS: usize = 2_048` (`E/DrumGridView.swift:27`).
    - `pub fn cell_label(row: &str, tick: i64, hit: bool, velocity: u16, meter: &[MeterEntryRow]) -> String` (`:15-23`):
      - "‹row›, bar ‹b› beat ‹beat›", with " tick ‹t›" when off the beat.
      - Then ", on, velocity ‹velocity >> 9›" or ", off".
    - `pub fn row_label(lane: &DrumLaneRow, row: &DrumRowRow) -> String`: "‹name›, ‹PitchName›, ‹track›, ‹destination›", with "no destination" when there is none (`E/DrumRowHeaderView.swift:311-312`).
    - `pub fn header_text(lane: &DrumLaneRow) -> String`: "‹track› · ‹destination›", with "No Destination" when there is none (`:88`).
    - `pub fn caption(row: &DrumRowRow) -> (String, Option<String>, bool)`: name, destination, and inherited when `row.dest` is `None` (`:140-142`).
    - `pub fn nudge_title(shift: i64) -> String`: "Nudge: +360 ticks", "Nudge: -240 ticks" or "Nudge: 0 ticks" (`S/DrumGridPane.swift:151`).
    - The strings, each with its Swift file:line:
      - `PAINT_LABEL` "Paint Drum Hits" and `ERASE_LABEL` "Erase Drum Hits"
      - `NO_ROW` "Select a drum row first." (`M/DrumGridModel.swift:47-49`)
      - `NO_GRID` "Open a drum grid first." (`M/DrumCommands.swift:26`)
      - `NO_DRUM_TRACK` "The drum grid shows no drum track." (`:57`)
      - `NO_TRACK_TO_ADD` "There is no drum track to add a row to." (`M/DrumGridModel.swift:281`)
      - `fn every_note_taken(track: &str) -> String`: "Every note already has a row in ‹track›." (`:286`)
      - `REFUSED` "The drum command was refused." (`M/DrumCommands.swift:71`)
      - `NEW_TRACK_BAND` "Drop a row here to give it a track of its own" (`E/DrumGridView.swift:173`)
      - The empty states: "No MIDI or Instrument Tracks" with "Add a MIDI or Instrument track to edit it as drums.", and "Choose a Track" with "The Drum Grid edits a MIDI or Instrument track as drum rows." (`S/DrumGridPane.swift:37-43`)
  - **Layout:** `EditorLayout.drum_grid: DrumGridSettings` under "drumGrid", merged as a struct (`merge_key`'s struct list).
  - **The seam:**
    - `fn drum_lanes(&self, seq: u64) -> Result<Vec<DrumLaneRow>, VizError>` on `DocSession`, with a one-line live impl.
    - `FakeSession` gains `set_drum_lanes(seq, Vec<DrumLaneRow>)` and `drum_lanes_reads() -> usize`. The call is recorded as `"drum_lanes"`, and it honours `set_call_error` and `fail_next("drum_lanes", …)`.
    - The fake's `edit` sets the lane row's `is_drum` for `DrumEdit::MarkLane`, as it already renames tracks.
  - **Fixture:** `pub fn drum_fake() -> FakeSession` (`T/DrumGridModelTests.swift:18-41`):
    - It starts from `editor_fake()`.
    - Sequence 3's lanes are Groove 30 (MIDI), Drums 40 and Perc 41 (both `is_drum`) and Vocals 42 (`TrackKindRow::Audio`), with `set_info(3, four_four(3))`.
    - Drums (40) holds Kick 1 (note 36), with hits 11 at 0 and 12 at q, and Snare 2 (note 38), with hit 13 at q. Perc (41) holds Clap 3 (note 39).
    - Every hit is a sixteenth long with velocity `100 << 9`. Rows have `dest: None` and destination "Kit"; the lanes have destination "Kit".
- [ ] **Step 1: Write the failing tests** in `tests/drum_grid.rs`, with `q = 15_360`:
  - `settings_decode_tolerantly` (`T/DrumGridModelTests.swift:286-300`):
    - The first JSON gives view Timeline, step 3 840, velocity 127, row height 64 and the default spacing.
    - `{"step":1,"spacing":-3,"duration":0}` gives 480, 480 and q/8.
    - `{"step":1000000000000,"spacing":1000000000000000}` gives 61 440 and 16q.
    - `{}` gives the default settings.
    - `lengths_including(999).last() == ("999 ticks", 999)`, and `lengths_including(q)` has 9 entries.
  - `the_settings_survive_a_save`: `merge_into` keeps an unknown key inside "drumGrid", and a non-default view and velocity round-trip.
  - `cells_and_rows_are_labelled`:
    - `cell_label("Kick", 0, true, 100 << 9, meter) == "Kick, bar 1 beat 1, on, velocity 100"`.
    - `cell_label("Snare", 7_680, false, 0, meter) == "Snare, bar 1 beat 1 tick 7680, off"`.
    - `row_label` of Clap is "Clap, D#2, Perc, Kit".
    - `header_text` with no destination is "Drums · No Destination".
    - `nudge_title(360) == "Nudge: +360 ticks"` and `nudge_title(-240) == "Nudge: -240 ticks"`.
  - `every_row_header_names_its_destination_and_dims_an_inherited_one` (`T/DrumGridViewTests.swift:187-199`): a row with `dest: Some(7)` and destination "Other" is `("Kick", Some("Other"), false)`, and Kick as it is is `(…, Some("Kit"), true)`.
  - `the_velocity_stepper_steps_by_four` (`stepped_velocity`): 100 up is 104, 125 up is 127, 3 down is 1 and 100 down is 96.
  - `the_fake_reads_drum_lanes`: `drum_fake().drum_lanes(3)` has Drums and Perc, and the read count is 1. A scripted Busy answers Busy.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test drum_grid`. Expected: FAIL (missing module).
- [ ] **Step 3: Implement.** Reuse `layout.rs`'s `field` helper (make it `pub(crate)`) rather than copying it (10c leftover).
- [ ] **Step 4: Run** the test, `cargo test -p vizdaw-app --test detail`, and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `drum_grid: the bar's settings, labels, the saved layout and the drum_lanes seam`.

### Task 2: What the grid shows: lanes, layout, selection and opening a track

**Files:**
- Create: `crates/vizdaw-app/src/drum_grid/model.rs`
- Modify:
  - `crates/vizdaw-app/src/document.rs`: a `drum_grid: DrumGridModel` field, and `idle` 387.
  - `crates/vizdaw-app/src/document/editing.rs`: `open_detail_in` 193, `close_detail` 236.
  - `crates/vizdaw-app/src/strip_chart/model.rs`: `detail_changed` 543 becomes `strip_detail_changed`.
- Test: `crates/vizdaw-app/tests/drum_grid_model.rs` (new); move `exact_mode.rs`'s `drag_calls` into `tests/support/mod.rs` as `edit_calls` and use it from both files.

**Interfaces:**
- Consumes: Task 1.
- Produces (ports `M/DrumGridModel.swift:5-180,373-399`, `M/DocumentModel.swift:393-428`):
  - `pub struct DrumRowKey { pub lane: u64, pub row: u64 }`.
  - `pub struct DrumGridModel`: the pane's grid. It holds the selected row, the stroke (Task 3), and the `Mutex` caches of the lanes and layout, as `piano_roll/model.rs:88,177-200` does. There is no window slot (Ruling 2).
  - **On `DocumentModel`:**
    - `drum_showing() -> bool`: the detail's kind is `DrumGrid`.
    - `drum_seq() -> Option<u64>` and `drum_focus_range() -> Option<TickRange>`: a block target's `start..max(end, start + 1)`.
    - `drum_lanes() -> Arc<[DrumLaneRow]>` (`:88-108`):
      - It is cached on `(seq, snapshot().version)`.
      - Busy returns the cache when it is the same sequence, else empty.
      - Any other error caches the failure at that version (no retry), clears the cache, reads as empty and queues its message.
    - `pub(crate) fn take_drum_failure(&mut self) -> bool`, called from `idle`: the queued message becomes `status`, once (Ruling 11).
    - `drum_layout() -> Arc<DrumGridLayoutRow>`: `drum_grid_layout(shapes, settings.row())`, cached on the shapes and settings. An error is an empty layout.
    - `drum_spot(y: f64) -> DrumSpotRow` (an error is `Nothing`) and `drum_row(lane: usize, row: usize) -> Option<DrumRowRow>`.
    - `drum_selected_row() -> Option<DrumRowKey>`, `drum_selected() -> Option<(DrumLaneRow, DrumRowRow)>` (only while the row still exists) and `drum_select(lane: usize, row: usize)` (out-of-range indexes do nothing).
    - `drum_current_lane() -> Option<DrumLaneRow>`: the selected row's track, else the first.
    - `drum_settings() -> &DrumGridSettings` and `update_drum_settings(impl FnOnce(&mut DrumGridSettings))`.
    - `drum_select_range(range: TickRange)`: the time range over the shown drum tracks, with no events (`E/DrumGridView.swift:55-58`).
    - `pub fn drum_grid_offers(row: &LaneRow) -> bool`: anything but `TrackKindRow::Audio` (`:382`).
    - `drum_tracks() -> Vec<LaneRow>`: the target sequence's offered tracks (`:112-115`).
    - `drum_open_lane(lane: u64)`: `open_detail(Lane{seq, lane}, Some(DrumGrid))` (`:118-125`).
    - `mark_drum_lane(seq, lane, on: bool) -> bool`: `edit(Drum{MarkLane})` (`:377-379`).
  - **Opening:** `open_detail_in` calls a private `prepare_drum_grid(target)` first, when `kind.unwrap_or(last_kind) == DrumGrid` (`M/EditorModel.swift:413`). It marks the target's track when the track exists, is offered and is not `is_drum` (`:393-399`).
  - **Detail changes:** `DocumentModel::detail_changed(before)` moves to `document/editing.rs`. It calls `strip_detail_changed(before.clone())` and `drum_detail_changed(before)`:
    - When the detail stops being a Drum Grid, the selected row is cleared (`pruneDrumGrids`, `M/DocumentModel.swift:418-420`).
    - When its kind or target changes, the stroke is cancelled (Task 3 fills it).
- [ ] **Step 1: Write the failing tests.** Port `T/DrumGridModelTests.swift` by name. `make()` opens `Lane{3,40}` in the Drum Grid and clears the calls.
  - `opening_a_midi_lane_gives_it_a_default_layout`: opening `Lane{3,30}` gives `edits() == [Drum{MarkLane{3,30,true}}]`, and the detail target is `Lane{3,30}`. Opening it again sends nothing more.
  - `a_lane_with_a_layout_keeps_it_and_an_audio_lane_is_not_offered`: `Lane{3,40}`, `Block{3,41,0,100}`, `Lane{3,42}` and `Sequence{3}` send no edit. The ids of `drum_tracks()` are `[30, 40, 41]` and their names "Groove", "Drums", "Perc". The List opened on `Lane{3,30}` sends nothing.
  - `shows_the_drum_lanes_of_the_shown_sequence` (pane half):
    - The names are `["Drums", "Perc"]`, and there are 6 bands.
    - `drum_spot(30) == Row{0,0,None}`, `drum_spot(10) == Header{0}` and `drum_spot(1000) == NewLane`.
    - The current lane is 40.
    - After more `drum_lanes()` and `drum_layout()` calls, `drum_lanes_reads() == 1`.
  - `busy_keeps_the_lanes_and_a_failure_shows_once` (`:219-236`): Review Focus 5. Use `set_call_error("drum_lanes", …)` and an edit to bump the version. The message "gone" reaches `status()` after one `idle()`, and a second `idle()` after clearing the status leaves it `None`.
  - `a_busy_read_then_a_fresh_one_does_not_trap` (`:249-270`): Review Focus 2. Script `fail_next("drum_lanes", busy)`, then `set_drum_lanes(3, [Drums])`. `drum_select(1, 0)` selects `DrumRowKey{41, 3}`, and `drum_selected()` is then `None`.
  - `a_failure_forgets_the_rows_read_before_it` (`:272-284`).

  New: `leaving_the_drum_grid_clears_the_selected_row`. Select Snare, then `show_detail(List)`: `drum_selected_row()` is `None`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test drum_grid_model`. Expected: FAIL.
- [ ] **Step 3: Implement.** Call `core_ffi::drum::*` for the layout and spot. Do not port them (Ruling 1).
- [ ] **Step 4: Run** the test, `cargo test -p vizdaw-app --test strip_chart_model --test exact_mode`, and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `drum_grid: the drum tracks shown, the layout, the selected row and opening a track`.

### Task 3: Strokes as one undo step

**Files:**
- Create: `crates/vizdaw-app/src/drum_grid/stroke.rs`
- Modify: `crates/vizdaw-app/src/drum_grid/model.rs` (`drum_detail_changed`)
- Test: `crates/vizdaw-app/tests/drum_grid_model.rs`

**Interfaces:**
- Consumes: Task 2.
- Produces (ports `M/DrumGridModel.swift:182-270`):
  - `pub type DrumExtend = fn(DrumStrokeRow, Vec<DrumHitRow>, i64, i64, Option<u8>, DrumGridSettingsRow) -> Result<DrumStrokeRow, VizError>`. Its default is `core_ffi::drum::drum_stroke_extend`.
  - **On `DocumentModel`:**
    - `drum_stroke() -> Option<&DrumStrokeRow>`.
    - `drum_stroke_began(lane: usize, row: usize, at: i64, snapped: i64, grab: i64, velocity: Option<u8>, erase: bool)`:
      - A stroke still held is cancelled first.
      - Out-of-range indexes do nothing.
      - Otherwise it selects the row, calls `drum_stroke_begin` and keeps the target `(seq, lane id, row id, the row's hits now)`. Nothing opens yet.
    - `drum_stroke_moved(at: i64, grab: i64, velocity: Option<u8>)`: the first move opens the gesture, and each move then sends `update_gesture(command)`.
    - `drum_stroke_ended(at, grab, velocity)`:
      - A failed extend cancels.
      - A click opens and closes in one go, and only when it has a command.
      - `end_gesture(Some(cmd))`, or `cancel_gesture()` when the final command is `None`.
    - `drum_stroke_cancelled()`: cancels the stroke's own gesture only.
    - `set_drum_extend_for_test(f: DrumExtend)`.
  - **Opening** (`:258-270`):
    - It calls `begin_gesture_from(ERASE_LABEL or PAINT_LABEL, vec![(seq, lane)], GestureOrigin::Other)`.
    - A refusal while a gesture was already open sets the status `GESTURE_OPEN`.
    - The stroke keeps the gesture's serial and touches only that gesture (10c Outcome 5).
  - **The command** is `drum_stroke_command(stroke, seq, lane id, row id, settings.row())`, built from the stored target, never from the current indexes (Review Focus 2).
  - **Dropping:** `drum_detail_changed` cancels the stroke when the detail's kind or target changes, or it closes.
- [ ] **Step 1: Write the failing tests.** Port `T/DrumGridModelTests.swift` by name, with `s16 = q / 4`. Calls go through `edit_calls(&f)`, where Swift's "begin X", "gesture", "end" and "abort" are `begin_edit(X)`, `edit_gesture`, `end_edit` and `abort_edit`.
  - `a_click_on_an_empty_cell_paints_one_hit` (`:107-121`):
    - `began(0, 1, 2*s16 + 5, 2*s16, 1, None, false)`, then `ended(2*s16 + 5, 1, None)`.
    - The calls are `["begin_edit(Paint Drum Hits)", "edit_gesture", "end_edit"]`.
    - The last gesture edit is `Drum{Paint{seq 3, lane 40, row 2, hits [DrumPaintHitRow{2*s16, 100 << 9}], window s16, duration q/8}}`.
    - The selected row is `{40, 2}`, and there is no stroke.
  - `a_click_on_a_hit_erases_it` (`:123-130`): `begin_edit(Erase Drum Hits)` and `Event{Delete{[12]}}`.
  - `a_drag_paints_a_run_and_the_velocity_lane_sets_each_hit` (`:132-153`):
    - With spacing q/2, the hits are at 0 (velocity 20 << 9), then q/2, q, 3q/2 and 2q (90 << 9).
    - The release at q/2 + 3 sends the first two, and the last call is `end_edit`.
  - `escape_and_refusals_leave_no_undo_step` (`:155-175`): Review Focus 1.
  - `a_failed_extend_on_mouse_up_aborts_the_gesture` (`:238-247`): uses `set_drum_extend_for_test` with an `Err(Model{"bad"})`.

  New:
  - `a_new_press_cancels_a_stroke_whose_release_was_lost`: began and moved on Snare, then began on Kick. The last call is `abort_edit`, and the stroke is on Kick.
  - `a_grid_that_stops_showing_drops_its_stroke`: mid-stroke, `show_detail(List)` makes the last call `abort_edit`, with no gesture and no stroke. Do the same with `close_detail`.
  - `a_stroke_paints_the_row_it_began_on_after_the_rows_reorder` (Review Focus 2):
    - Begin on Snare (lane 0, row 1).
    - `set_drum_lanes` swaps Kick and Snare, and an edit bumps the version.
    - Move and release: the paint's `row` is 2 (Snare's id).
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test drum_grid_model`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `drum_grid: paint and erase strokes as one undo step`.

### Task 4: Rows, play quantize and nudge, the Drum menu, and `editors.drum_grid`

**Files:**
- Create: `crates/vizdaw-app/src/drum_grid/{rows.rs,commands.rs}`
- Modify:
  - `crates/vizdaw-app/src/commands.rs`: `EDITOR_COMMANDS` 43, `enabled` ~400, `is_on`, `perform_from_menu` ~440, `run` ~590, `run_editor` 773.
  - `crates/vizdaw-app/src/editor/detail.rs`: `is_available` 32.
  - `crates/vizdaw-app/src/editor/mod.rs`: the fallback arm at 152-154 goes (Ruling 13).
- Test: `crates/vizdaw-app/tests/drum_grid_model.rs`, `crates/vizdaw-app/tests/drum_commands.rs` (new); update `tests/detail.rs:296,327`.

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces (ports `M/DrumGridModel.swift:272-370`, `M/DrumCommands.swift`):
  - **Rows**, each `-> bool`:
    - `drum_add_row()`:
      - With no current lane, `NO_TRACK_TO_ADD`.
      - Otherwise it asks `drum_next_row(notes)`; `None` is `every_note_taken(name)`.
      - It sends `AddRow{seq, lane, name, note, dest: None}`, then selects the new row read back by note with no dest.
    - `drum_remove_selected_row()`, `drum_split_selected_row()`, and `drum_show_velocity(on: Option<bool>)`: `None` toggles, and asking for the current state sends nothing but is true. Each sets `NO_ROW` without a selection.
    - `drum_rename(key: DrumRowKey, name: &str)`: the name is trimmed. An empty name or the current one sends nothing and is false.
    - `drum_drop(lane: usize, row: usize, y: f64)`: `drum_row_drop(...)`, then `edit`. `None` and stale indexes are false.
  - **Tracks:**
    - `pub fn drum_quantize_choices() -> &'static [QuantizeChoiceRow]`, built once (`OnceLock`).
    - `drum_quantize_title(lane: &DrumLaneRow) -> String`: an error reads "Custom".
    - `drum_set_quantize(lane: u64, q: Option<QuantizeRow>) -> bool`: `Lane{SetPlayQuantize}`.
    - `drum_nudge(lane: u64, steps: i64) -> bool`: through `drum_nudged`. An unchanged shift sends nothing and is true; otherwise `Lane{SetPlayShift}`.
  - **The Drum menu** (`commands.rs`):
    - `pub const DRUM_COMMANDS: [&str; 5] = ["drum.add_row", "drum.show_velocity", "drum.split_row", "drum.remove_row", "drum.timeline"]`.
    - `pub fn drum_enabled(id, &DocumentModel) -> bool`:
      - `add_row` needs a current lane.
      - `show_velocity`, `split_row` and `remove_row` need a selected row.
      - `timeline` is always enabled.
      - Everything except `timeline` needs `drum_showing()` (Ruling 2).
    - `pub fn drum_is_on(id, &DocumentModel) -> bool`: the selected row's `show_velocity`, or the view is `Timeline`.
    - `pub fn run_drum(id, args, &mut DocumentModel) -> Result<Ran, ControlError>`:
      - `timeline` takes `on` (absent toggles) and sets the view.
      - Without a grid, `unavailable(NO_GRID)`. Without a lane, `unavailable(NO_DRUM_TRACK)`. Without a row, `unavailable(NO_ROW)`.
      - A refused edit is `failed` with the status, or `REFUSED`.
      - `show_velocity` takes `on`.
    - `commands::{enabled, is_on, run, perform_from_menu}` route `DRUM_COMMANDS` there.
  - **`editors.drum_grid`** joins `EDITOR_COMMANDS` and runs `show_detail(DrumGrid)`.
  - **`is_available`** is true for every kind.
- [ ] **Step 1: Write the failing tests.**

  Port `T/DrumGridModelTests.swift` by name:
  - `row_commands_act_on_the_selected_row` (`:177-203`):
    - Remove with no selection fails with `NO_ROW`.
    - Add sends `AddRow{3, 40, "Closed Hat", 42, None}`.
    - With Snare selected:
      - Show Velocity sends `ShowVelocity{3,40,2,true}`.
      - Split sends `SplitRow{3,40,2}`.
      - Remove sends `RemoveRow{3,40,2}`.
    - `"  Bass Drum "` renames to "Bass Drum", and "Kick" sends nothing.
    - Drops at y 90, 500 and 30 give `MoveRowToLane{3,40,1,41}`, `SplitRow{3,40,1}` and nothing.
  - `lane_quantize_and_nudge` (`:205-217`), with:
    - "1/16 Swing 62%" sent through `SetPlayQuantize`.
    - The title of Drums is "Off".
    - `drum_nudge(40, -2)` sends `SetPlayShift{3,40,-240}`.
    - `mark_drum_lane(3,30,true)` sends `MarkLane`.

    New (Review Focus 5): with `play_shift: 3_840` in the fake, `drum_nudge(40, 1)` sends no edit and is true.

  In `tests/drum_commands.rs`:
  - `the_drum_menu_follows_the_grid`:
    - Before ⌘4, every id but `timeline` is disabled, and `run_drum("drum.split_row")` is `Unavailable` with "Open a drum grid first.".
    - On `Lane{3,40}`, `add_row` is enabled and `remove_row` is disabled ("Select a drum row first.").
    - After `drum_select(0,1)` all five are enabled.
  - `the_drum_menu_runs_its_commands`:
    - `drum.timeline` with `on: true` sets `Timeline`, and `is_on` is true. Without `on` it toggles back.
    - `drum.show_velocity` with `on: true` sends `ShowVelocity{…, true}`.
    - `drum.add_row`, `drum.split_row` and `drum.remove_row` send their edits.
    - A refused edit (`set_edit_error`) answers `Failed` with the status text.
  - `editors_drum_grid_shows_the_grid`: `run("editors.drum_grid")` gives `detail_kind_name() == "drum_grid"` on the open target.

  Update `tests/detail.rs`: `the_strip_chart_is_available` becomes `every_editor_is_available`. `an_unported_last_editor_opens_the_piano_roll` becomes `a_saved_drum_grid_last_editor_opens_the_drum_grid`: kind "drum_grid", with the saved value kept.
- [ ] **Step 2: Run** `cargo test -p vizdaw-app --test drum_grid_model --test drum_commands --test detail`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-app` and `cargo test -p core-control`. Expected: PASS.
- [ ] **Step 5: Commit** `drum_grid: rows, play quantize and nudge, the Drum menu and editors.drum_grid`.

### Task 5: The grid in the bottom pane, its bar, the empty state, ⌘4 and the Drum menu

**Files:**
- Create: `crates/vizdaw-gpui/src/drum_grid/mod.rs`: the view state, the bar, the empty state and `render`. Tasks 6 and 7 add `canvas.rs` and `headers.rs`.
- Modify:
  - `crates/vizdaw-gpui/src/detail_pane.rs`: the `editor` match at 117-150.
  - `crates/vizdaw-gpui/src/panes.rs`: `KINDS` at 41.
  - `crates/vizdaw-gpui/src/shell.rs`: a `drum_grid: DrumGridView` field, constructed and prepared like `strip_chart`.
  - `crates/vizdaw-gpui/src/menus.rs`:
    - `ROUTED_EDITOR_COMMANDS` 186, and `routed` adds `DRUM_COMMANDS`.
    - The Editors menu at 699: `editors.drum_grid` after `editors.strip_chart`.
    - A "Drum" menu after "Track" (Ruling 7): Add Drum Row, Show Velocity, Split Row to Own Track, Remove Drum Row, a separator, then Timeline View (`S/AppCommands.swift:311-318`).
    - The `checked` list in `command` gains `drum.show_velocity` and `drum.timeline`.
  - `crates/vizdaw-gpui/src/list_editor.rs`: `Entry::Label` takes a `String` (the "Nudge: …" heading).
  - `crates/vizdaw-gpui/src/{control.rs,lib.rs}`.
- Test: `crates/vizdaw-gpui/tests/drum_grid.rs` (new); update `tests/piano_roll.rs:1345` and `tests/menus.rs`

**Interfaces:**
- Consumes: Tasks 1–4.
- Produces (ports `S/DrumGridPane.swift`, widths and a11y verbatim):
  - **The view:** `pub struct DrumGridView { focus: FocusHandle, axis: TimeAxis, offset: f32, layout: Rc<Cell<Bounds<Pixels>>>, mounted: bool, shown_target: Option<EditorTarget>, menu: Option<DrumMenu>, detach_pending: bool, … }`.
    - `DrumGridView::new(cx)`.
    - `prepare(&mut self, d: &DocumentModel) -> bool`: whether it shows. It takes the saved zoom. At the first layout for a block target, it scrolls to `start − 16 × ticks_per_point` (`E/DrumGridView.swift:106-114`).
    - `pub struct DrumGridState` with `DrumGridState::of(doc, transport: Option<&TransportModel>, now) -> Option<Self>`.
    - `pub(crate) fn render(view, state, scope, window, cx) -> AnyElement`.
  - **The bar** (`S/DrumGridPane.swift:95-136`), in one row:
    - The buttons "Paint" and "Erase" (100 in all, the current one selected). They `set_tool(Pencil)` or `set_tool(Eraser)` (Ruling 6).
    - "View" (100): `DrumView::ALL`.
    - "Step", "Spacing" and "Length" (70 each), from `lengths_including`. Their help: "Step: the width of a step matrix cell", "Spacing: the distance between the hits a drag paints" and "Length: the length of a painted hit".
    - The text "Vel ‹n›" with the buttons "Decrease Velocity" and "Increase Velocity", which call `stepped_velocity`. Their help: "The velocity of painted hits in rows whose velocity is hidden" (Rulings 5 and 16).
    - The button "Add Row" (help "Add Drum Row"): `drum_add_row`.
    - A spacer, then, when there is a current lane, the menu "Track" (160).
      - Its face is "‹track›: ‹quantize title›", and its help "The track's play quantize (grid or swing) and nudge, applied as it plays".
      - It lists the heading "Play Quantize", a checked item per choice, the heading `nudge_title(shift)`, and "Nudge Earlier" and "Nudge Later" (`:139-159`).
    - Pickers are `menu` in the tree, their value the current title. They open `bar_menu` lists. Every change goes through `update_drum_settings`, `set_tool` or the Task 4 calls.
  - **The empty state** (`:27-53`), when `drum_lanes()` is empty:
    - With no offered tracks, the text "No MIDI or Instrument Tracks" and its description.
    - Otherwise, "Choose a Track", its description, and a `button` per track (its name) that calls `drum_open_lane`.
  - **The pane:** `detail_pane::render` hosts the grid for `DetailEditorKind::DrumGrid`, and the bottom pane's picker offers "Drum Grid". `control.rs` no longer answers `not_yet` for `editors.drum_grid` or the `drum.*` ids, and `editors.drum_grid_window` still does.
  - **Constants:** `GROUP = "Drum Grid"`, `ROWS_GROUP = "Drum Rows"`, `TOOL_PAINT = "Paint"`, `TOOL_ERASE = "Erase"`, `VIEW = "View"`, `STEP = "Step"`, `SPACING = "Spacing"`, `LENGTH = "Length"`, `ADD_ROW = "Add Row"` and `TRACK = "Track"`.
- [ ] **Step 1: Write the failing tests.** GPUI tests in `tests/drum_grid.rs`, with `drum_fake()` and a 360-point bottom pane, set up as `tests/strip_chart.rs:48-82`:
  - `editors_drum_grid_opens_the_grid`: on `Lane{3,40}`, after `secondary-4`:
    - `detail_kind_name()` is "drum_grid".
    - The tree has the `group` "Drum Grid" and the `group` "Drum Rows".
    - `menu` "View" has the value "Step Matrix", "Step" "1/16", "Spacing" "1/16" and "Length" "1/32".
    - The text is "Vel 100", `button` "Paint" is selected, and `menu` "Track" has the value "Drums: Off".
  - `bar_changes_are_saved_with_the_project`: after View › Timeline, `saved_layout(..)["drumGrid"]["view"] == "timeline"`. `button` "Erase" makes `tool() == Eraser`.
  - `the_velocity_stepper_stops_at_its_ends` (Review Focus 5): 6 presses of "Increase Velocity" from 100 read "Vel 124", and a seventh reads "Vel 127". With velocity 3, "Decrease Velocity" reads "Vel 1".
  - `the_track_menu_sets_quantize_and_nudges`: picking "1/8 Swing 66%" sends `SetPlayQuantize`, and "Nudge Later" sends `SetPlayShift{3,40,120}`.
  - `add_row_adds_the_next_kit_row`: "Add Row" sends `AddRow{3,40,"Closed Hat",42,None}`.
  - `a_sequence_without_drum_tracks_offers_its_tracks`:
    - With `set_drum_lanes(3, vec![])` on `Sequence{3}`, the text "Choose a Track" shows with the buttons "Groove", "Drums" and "Perc", and no "Vocals".
    - Clicking "Groove" sends `MarkLane{3,30,true}` and targets `Lane{3,30}`.
    - With the lanes all audio, the text is "No MIDI or Instrument Tracks".
  - `the_drum_menu_is_in_the_menu_bar`: the menu model has "Drum" after "Track", with the five titles. "Timeline View" and "Show Velocity" have check marks.
  - `a_saved_drum_grid_last_editor_opens_the_grid` (replaces `an_unported_last_editor_double_click_opens_the_roll`, `tests/piano_roll.rs:1345`):
    - Set up `roll_fake()` with `{"lastKind":"drumGrid"}`, then double-click the Chords block.
    - The result is "drum_grid", and `edits()` ends with `MarkLane{3,31,true}` (Review Focus 3).
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test drum_grid`. Expected: FAIL.
- [ ] **Step 3: Implement.** Copy the bar and pane plumbing from `strip_chart.rs:1484-1791`.
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui` and `cargo test -p vizdaw-app`. Expected: PASS.
- [ ] **Step 5: Commit** `drum_grid: the grid in the bottom pane, its bar, ⌘4 and the Drum menu`.

### Task 6: The canvas: drawing, strokes, the ruler and the tree

**Files:**
- Create: `crates/vizdaw-gpui/src/drum_grid/canvas.rs`
- Modify: `crates/vizdaw-gpui/src/drum_grid/mod.rs`, and `crates/vizdaw-gpui/src/shell.rs` (detach and focus, as `chart_detach` and `chart_hidden` do)
- Test: `crates/vizdaw-gpui/tests/drum_grid.rs`

**Interfaces:**
- Consumes: Tasks 2–5.
- Produces (ports `E/DrumGridView.swift`):
  - **The layout:**
    - The headers (150 wide) sit at the left, and the canvas fills the rest.
    - A 22-point ruler (`RULER_HEIGHT`) sits over both.
    - Content y is `band.top − offset`.
    - A ruler click locates. A ruler drag past `DEAD_ZONE` calls `drum_select_range`.
    - The wheel scrolls in time and vertically, clamped to `drum_layout().height`.
    - The secondary-wheel and a pinch zoom about the pointer and save `drum_grid.ticks_per_point`.
    - While the target's sequence plays and the layout follows, the view pages with the playhead, as `PianoRoll::follow` does.
  - **Drawing** (`:152-231`), per visible band:
    - A header band is filled with `colors::ruler()`.
    - A row band:
      - The selection tint (`colors::selection()` at 0.08) on the selected row.
      - In the matrix, cells from `drum_grid_cells(hits, view's first tick, last tick + 1, step)`, inset 1 by 2. An empty cell is the separator at 0.15. A hit cell is `region_fill` at `0.35 + 0.65 × velocity / 65_535`, outlined with the region line, or the selected outline (2) when one of its hits is selected.
      - In the timeline, each hit at its start, at least 4 points wide. A muted hit is drawn at 0.3, others at 0.85.
      - With a velocity lane (`height > cells`), its background at 0.04, then a 3-wide bar per hit at `velocity / 65_535` of the lane.
    - The new-track band shows `NEW_TRACK_BAND` at (8, top + 7) in the disabled text colour.
    - Each band has a separator at its bottom.
  - **The pointer** (`:233-305`):
    - `grab = ceil(4 × ticks_per_point)`.
    - A press over a row calls `drum_stroke_began(lane, row, tick, snap.floor(tick), grab, drum_row_velocity(..), tool == Eraser)`.
    - Moves past `DEAD_ZONE` call `drum_stroke_moved`, and the release calls `drum_stroke_ended`. A release before the dead zone is a click: began, then ended at the same point.
    - Escape calls `drum_stroke_cancelled`, as do the window losing the keyboard, the grid leaving its pane, and the document closing.
  - **The tree:** the canvas is the `group` "Drum Grid". It holds a `button` per visible cell (matrix) or hit (timeline), at most `MAX_TREE_ITEMS`, labelled with `cell_label` and bounded by its rect. `ui.click` on one is a real click at its centre, which toggles it (Ruling 8).
- [ ] **Step 1: Write the failing GPUI tests.** Port `T/DrumGridViewTests.swift`.
  - Points are canvas content points: x from the canvas's left edge, y below the ruler.
  - The zoom is the default 96 ticks a point, so a sixteenth is 40 points.
  - The bands are: header 0–22, Kick 22–42, Snare 42–62 (to 90 with its velocity lane), Perc's header 62–84, Clap 84–104 and the new-track band.
  - Port by name:
    - `a_click_on_an_empty_cell_paints_it`: (85, 50) gives `paint([2*s16])` and calls `begin_edit(Paint Drum Hits)`, `edit_gesture`, `end_edit`. The selected row is `{40, 2}`.
    - `a_drag_paints_a_run_and_escape_takes_it_back`: (5, 50) to (125, 70) gives `paint([0, s16, 2*s16, 3*s16])` (the stroke stays on its row). Escape makes the last call `abort_edit`, and the release after it sends nothing.
    - `the_eraser_erases_the_hits_it_crosses`: with `Tool::Eraser`, (5, 30) to (165, 30) gives `begin_edit(Erase Drum Hits)` and `Event{Delete{[11, 12]}}`.
    - `a_shown_velocity_lane_sets_the_velocity_from_the_height`: with Snare's lane shown, a click at (85, 42) paints velocity `127 << 9`, and the content height is 160.
    - `cells_and_hits_are_in_the_tree_and_a_click_toggles`:
      - The first `button` under "Drum Grid" is "Kick, bar 1 beat 1, on, velocity 100".
      - A click on "Snare, bar 1 beat 1 tick 7680, off" gives `paint([2*s16])`.
      - In the timeline, the buttons are exactly "Kick, bar 1 beat 1, on, velocity 100", "Kick, bar 1 beat 2, on, velocity 100" and "Snare, bar 1 beat 2, on, velocity 100".

  New tests:
  - `leaving_the_pane_mid_stroke_leaves_no_undo_step` (Review Focus 1): `show_detail(List)` mid-drag makes the last call `abort_edit`. So does losing the window's focus mid-drag.
  - `a_stroke_reads_the_drum_lanes_once` (Review Focus 5): across a 20-move drag with `run_until_parked` after each move, `drum_lanes_reads()` grows by at most 1.
  - `the_tree_lists_at_most_2048_cells` (Review Focus 5): with `ticks_per_point` 61 440 and step 480, the group holds exactly 2 048 `button`s.
  - `a_ruler_drag_selects_the_range_over_the_drum_tracks`: the selection's lanes are `{(3,40), (3,41)}`, with no events.
  - `the_wheel_zoom_is_saved`: a secondary-wheel changes `drum_settings().ticks_per_point`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test drum_grid`. Expected: FAIL.
- [ ] **Step 3: Implement.** Copy the canvas, ruler, wheel and detach plumbing from `strip_chart.rs` and `piano_roll.rs:794-1200`. All the geometry is the model's (`drum_layout`, `drum_spot`) and the core's.
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `drum_grid: the canvas, paint strokes, the ruler and the tree`.

### Task 7: The row headers: select, drag, rename and the row menu

**Files:**
- Create: `crates/vizdaw-gpui/src/drum_grid/headers.rs`
- Modify: `crates/vizdaw-gpui/src/drum_grid/mod.rs`, `crates/vizdaw-gpui/src/shell.rs` (the rename field, as `begin_rename` at 1286-1331)
- Test: `crates/vizdaw-gpui/tests/drum_grid.rs`

**Interfaces:**
- Consumes: Tasks 2, 4 and 6.
- Produces (ports `E/DrumRowHeaderView.swift`):
  - **Drawing** (`:76-136`), at a width of 150, over the ruler-coloured background:
    - A header band shows `header_text` in body semibold.
    - A row band shows the selection tint at 0.2, then the caption. The name is in the label colour. Then " · ‹destination›", in the disabled colour when inherited, else the secondary one. The `PitchName` sits at the right, 38 in, in the digits font.
    - While a drag has moved, a 2-point line in the selected outline colour sits at the pointer.
  - **The pointer:**
    - A press on a row selects it and takes the keyboard.
    - A press with a click count of 2 opens the rename.
    - A drag past `DEAD_ZONE` vertically, then the release, calls `drum_drop(lane, row, content y)`. A press with no drag only selects.
    - Escape during a drag ends it, and the release drops nothing. Escape with no drag calls `drum_stroke_cancelled` (`:201-212`).
    - A secondary-click on a row selects it and opens the row menu.
  - **The row menu** (`:222-236`): "Rename…", "Show Velocity" (checked when shown), a separator, "Split Row to Own Track" and "Remove Row". They call the rename, `drum_show_velocity(Some(!shown))`, `drum_split_selected_row` and `drum_remove_selected_row`.
  - **Rename:**
    - A `TextInput` over the row's name (x 8, the band's width − 50) holds the row's name.
    - Return or blur commits through `drum_rename`. Escape cancels and sends nothing.
    - The owner is "drumRow", through `begin_text_editing` and `end_text_editing`.
    - The grid leaving the pane ends it, without renaming (Ruling 9).
  - **The tree:** the headers are the `group` "Drum Rows". Each visible row is a `button` labelled `row_label` and selected when it is the selected row. Pressing one selects it.
- [ ] **Step 1: Write the failing GPUI tests.** Port `T/DrumGridViewTests.swift` (header y is ruler + content y):
  - `a_row_header_drag_moves_the_row_and_a_double_click_renames`:
    - A Kick drag from y 30 to 95 sends `MoveRowToLane{3,40,1,41}`.
    - A press at 50 without a drag sends nothing and selects `{40, 2}`.
    - A double-click at 50 opens the field, and `transport_keys_enabled()` is false. Typing "Rim" and Return sends `RenameRow{3,40,2,"Rim"}`, and the keys are enabled again.
    - The row menu's titles are `["Rename…", "Show Velocity", "Split Row to Own Track", "Remove Row"]`, with the separator after the second.
    - The "Drum Rows" buttons are `["Kick, C2, Drums, Kit", "Snare, D2, Drums, Kit", "Clap, D#2, Perc, Kit"]`.
  - `escape_in_the_rename_field_cancels_the_rename`: after "Rim" and Escape, no edit is sent, the field is gone and Snare keeps its name.
  - `escape_during_a_row_header_drag_drops_nothing` (Review Focus 1): no edit is sent. Then, with the keyboard still on the headers, a canvas stroke and Escape in the headers make the last call `abort_edit`.
  - New: `the_row_menu_shows_velocity_and_splits`: secondary-click Snare, then "Show Velocity" sends `ShowVelocity{3,40,2,true}`, and "Split Row to Own Track" sends `SplitRow{3,40,2}`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-gpui --test drum_grid`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p vizdaw-gpui`. Expected: PASS.
- [ ] **Step 5: Commit** `drum_grid: row headers, drags, rename and the row menu`.

### Task 8: The e2e and the docs

**Files:**
- Modify:
  - `crates/vizctl/tests/e2e_gpui.rs`. Reuse:
    - `temp_home` 134, `wait_in_tree` 169, `wait_for_editor` 515 and `wait_for_undo` 919
    - `find_role` 4427, `click_role` 6103, `wait_for_role_value` 6110 and `run_in` 1676
    - `App::call_err` 90, and `core_engine::fixtures::Builder` as in `gpui_midi_mix_reaches_state_get` 2129
  - `docs/superpowers/specs/2026-11-22-parity-checklist.md`
  - `docs/testing/manual-checks.md`
  - `docs/feature-backlog.md`

- [ ] **Step 1: Write the e2e test.** `gpui_drum_grid_paints_and_erases_by_element` is the Swift `drum_grid_paints_and_erases_by_element` (`crates/vizctl/tests/e2e.rs:343-408`) in the docked pane (Ruling 8). It is `#[ignore = "needs a window server and VIZDAW_E2E_APP"]`, with a temp home.
  1. Build "Beat" with the track "Drums" on a "Kit" `builtin:synth` destination. It has rows Kick 36 and Snare 38, and one Kick note `(0, 36, 0x8000, Q/4)`. Save it into the home and `app.open` it.
  2. `command.run drum.split_row` answers `ErrorCode::Unavailable` (no grid yet, README 98).
  3. Click the Drums block, then `command.run editors.drum_grid`. `state.get editor` gives `detail.kind == "drum_grid"`.
  4. Wait for "Kick, bar 1 beat 1, on, velocity 64". Click "Snare, bar 1 beat 1, off", then `wait_for_undo("Paint Drum Hits")`.
  5. Click "Snare, bar 1 beat 1, on, velocity 100", then `wait_for_undo("Erase Drum Hits")`.
  6. `command.run drum.add_row` gives `wait_for_undo("Add Drum Row")` and the label "Closed Hat, bar 1 beat 1, off".
  7. `command.run drum.timeline {"on": true}`: "Kick, bar 1 beat 1, on, velocity 64" stays, "Closed Hat, bar 1 beat 1, off" goes, and `menu` "View" reads "Timeline".
  8. Save As, close and reopen: `menu` "View" still reads "Timeline".

  Use the core's undo labels if they differ from "Add Drum Row", and say so in a comment.
- [ ] **Step 2: Run** it on the Mac (the Global Constraints command). Expected: PASS. When a step fails for an app reason, fix the app with a test that fails first, not the e2e.
- [ ] **Step 3: Docs.**
  - **Checklist** (Mac cells only; the Linux cells are the controller's post-CI tick):
    - Row 75, `editors.drum_grid`: "S10d (`gpui_drum_grid_paints_and_erases_by_element`)".
    - Rows 130-134, the `drum.*` commands: "S10d". Write "S10d (e2e)" on `drum.add_row` and `drum.timeline`.
    - Row 507, "Drum grid (plan 4e)": "S10d (Drum Grid, bar, row headers, Drum menu; torn-off windows: later slice; on screen: M29)".
    - Row 545, `drum_grid_paints_and_erases_by_element`: "S10d (`gpui_drum_grid_paints_and_erases_by_element`, docked pane)".
    - Leave rows 80 (`editors.drum_grid_window`) and 386 blank.
  - **Manual checks:** the next free numbers are M29 and L35.
    - M29 (Mac): README 87-97, without the windows half of 96, and with 97's labels read from `ui.tree`, not VoiceOver. That covers:
      - ⌘4 on Beat's block, and the bar fitting on one line at the default window width
      - a click, a run, a run dragged back, and Escape
      - Spacing 1/8, Length 1/16, Step 1/8 and the Timeline
      - the Erase tool and an empty-cell click
      - Show Velocity and painting with height
      - Add Row ("Open Hat" A#2), rename, reorder, a drop onto Perc, a drop below the last track, Split Row to Own Track and Remove Row
      - opening a plain MIDI track in the grid, and the "Choose a Track" state
      - play quantize 1/8 Swing 66% heard as a shuffle, and Nudge Later read as "Nudge: +360 ticks" after three presses
      - the Drum menu's five items and their checks
      - save and reopen
    - L35 (Linux, XWayland):
      - The grid and headers draw with the system font.
      - Ctrl+4 opens the grid.
      - A secondary-click on a row header opens its menu at the pointer.
      - A double-click renames with the Linux text field, and Escape cancels it.
      - A header drag below the last track splits the row.
  - **Backlog:**
    - Change row 362 ("The Strip Chart in torn-off editor windows") to cover the Drum Grid too. That means `editors.drum_grid_window`, Swift's `activeDrumGrid` (the Drum menu acts on the grid last pressed in), and the Swift tests `tornWindowsAndTheKindSwitchPrepareTheLaneToo` and `aWindowThatNeverDrewSnapshotsItsRowsAndCells`.
    - New row: README step 93 is stale (Ruling 3). Swift's header-menu "Drum Track" was replaced by marking a track when the grid opens it. Decide whether 27.1 wants a way to turn a drum track back into a plain one.
    - Add a "GPUI slice 10d: review leftovers" row for the minors found in review.
- [ ] **Step 4: Commit** the e2e with `e2e: the Drum Grid paints and erases by element`. Commit the docs with `docs: slice 10d checklist, manual checks and backlog`.

## Rulings made while planning

1. **The grid's logic is the core's `core_ffi::drum::drum_*` functions, called as Swift called them.** They are pure functions of records, already exported and tested in plan 4e (Plan 4e Ruling 1). Calling them keeps one copy and adds no dependency (`vizdaw-app` already depends on `core-ffi`). Cost if wrong: none; `core-editor` could be called directly later.
2. **One grid, the bottom pane's.** There are no torn-off windows yet, so Swift's `activeDrumGrid` (`M/DocumentModel.swift:397,424-428`) reduces to this: the Drum menu acts on the pane's grid while the detail shows a Drum Grid. Cost if wrong: the torn-off slice adds a slot choice, as 10c's Ruling 12 left `ChartSlot::Window` for later.
3. **README step 93 is stale; the code wins.** At the baseline no track header menu offers "Drum Track", and `drum.mark_lane` is not in the registry. Instead `openDetail` marks a MIDI or Instrument track when the grid opens it (`M/EditorModel.swift:413`, `M/DrumGridModel.swift:393-399`), and the empty state offers "Choose a Track" (`S/DrumGridPane.swift:29-53`). This slice follows the code. Cost if wrong: one header-menu item and a command, if the owner wants the README behaviour back (backlog row).
4. **No Modify, Exact Mode or Strip Chart tie-in.** Swift's drum gestures open with `origin: .other` (`M/DrumGridModel.swift:263`), so Exact Mode never takes them. No Modify panel, Exact Mode or Strip Chart source mentions the drum grid. Cost if wrong: none found.
5. **The Vel stepper is the text "Vel ‹n›" with "Decrease Velocity" and "Increase Velocity" buttons in the tree.** Swift's `Stepper` was one control labelled "Velocity". GPUI has no stepper, and 10c used the same shape for Rows. Cost if wrong: two labels.
6. **Paint and Erase are two buttons that set the shared tool (Pencil or Eraser), as Swift's segmented picker did** (`S/DrumGridPane.swift:91-105`). The Arrow tool paints too (Plan 4e Ruling 5). Cost if wrong: none.
7. **The Drum menu sits after Track.** Swift's order is Track, Region, Drum, Transport (`S/AppCommands.swift:265-319`). GPUI has no Region menu yet, so Drum follows Track; Region goes before it when it lands. Cost if wrong: a menu's position.
8. **The e2e uses the docked pane and clicks cells by element.** Swift tore the grid off because its docked pane was too short (Plan 4e Ruling 15). Torn-off windows are out of scope, and two rows fit the GPUI pane. `ui.click` on a cell's `button` is a real click at its centre, which toggles the cell, as VoiceOver's press did. Cost if wrong: an e2e that needs `ui.pane` to make the pane taller.
9. **A rename commits on Return and on blur, and Escape cancels** (`E/DrumRowHeaderView.swift:255-280`; Swift's end-editing renamed unless the movement was cancel). The grid leaving its pane ends the rename without renaming, as Swift's teardown did (`:283-290`). Cost if wrong: one rename.
10. **The row menu opens on a secondary-click**, as Swift's `menu(for:)` did. Its items are `menuitem`s in the tree. Cost if wrong: none.
11. **A failed lanes read shows in the status line on the next `idle`.** Reads are `&self`, so they cannot set `status`. Swift posted the message on the next main-actor turn (`M/DrumGridModel.swift:105`). Cost if wrong: the message is one idle tick late.
12. **The headers' tree items are `button`s that select their row, and header drags have no e2e.** There is no `ui.drag` (10b Ruling 9). Drags are covered by the GPUI tests and M29. Cost if wrong: none.
13. **`is_available` is true for every kind, and the unported-editor fallback goes.** With the Drum Grid in, every `DetailEditorKind` has its editor (the Audio Editor is a pane view, not a detail kind). The two tests of the fallback become tests that a saved `drumGrid` opens the grid. Cost if wrong: none.
14. **`detail_changed` becomes one hook for both editors.** It moves from `strip_chart/model.rs` to `document/editing.rs` and calls the Strip Chart's and the Drum Grid's halves. Cost if wrong: none.
15. **The stroke's extend gets a test seam (`set_drum_extend_for_test`).** It is Swift's `extendStroke` (`M/DrumGridModel.swift:38-42`). The core call fails only when a panic is caught, so without the seam the abort path has no test. Cost if wrong: one function pointer.
16. **One help text loses "lane".** Swift's Vel help was "The velocity of painted hits in rows whose velocity lane is hidden" (`S/DrumGridPane.swift:120`). The UI never says "lane", so it reads "…in rows whose velocity is hidden". The menu item stays "Show Velocity". Cost if wrong: one string.

## Controller decisions

- Confirm Ruling 3. Opening a plain MIDI or Instrument track in the Drum Grid marks it a drum track (an undoable edit), and the README's header-menu "Drum Track" is not ported.
- Confirm Ruling 8 (the e2e uses the docked pane), and Ruling 7 (the Drum menu after Track until Region exists).

## Outcome (2026-11-28)

Eight tasks, 11 commits (beb0b737..4c2036a9), each task reviewed; final whole-branch review: ready to merge, no Critical or Important findings. Controller rulings:

1. Ruling 3 confirmed: opening a non-drum track in the grid marks it a drum track, one undoable edit, as the code at the tag does; no header "Drum Track" item. The mark keys off the editor that really opens (`EditorModel::opening_kind`) and only off a fresh track read, so a Busy core cannot mark twice.
2. Rulings 7 and 8 confirmed: the Drum menu sits after Track; the e2e paints by clicking cell elements in the docked pane.
3. Ruling 13 checked: the Audio Editor is a pane view, not a detail kind, so removing the unported-editor fallback leaves nothing unported.
4. A press anywhere on the grid (ruler, header band, new-track band, secondary click) first cancels a stroke whose release was lost. The task review graded the header case Important, because the next held move could commit the old stroke; fixed in 539ad3f9 and verified by the controller in place of a re-review.
5. The digits font is a platform monospace (no theme token exists); duplicated helpers, live-settings strokes and the other edge cases are in "GPUI slice 10d: review leftovers".
6. The slice 10b and 10c Linux ticks had left a stray empty cell in 18 checklist rows; fixed in 4c2036a9.

