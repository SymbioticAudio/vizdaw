# VizDAW 4e — Drum Grid Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Pulse-style drum grid detail editor (⌘4, ⌥⌘4 in a window) that shows only drum lanes as stacked rows (name, note, destination), as a step matrix of evenly spaced cells or a timeline of true starts and lengths; a paint tool that toggles a hit on click and paints or erases a run on drag with configurable spacing, length and velocity, a row's shown velocity lane setting each new hit's velocity from the pointer's height; rows added, renamed, reordered, dragged to another drum lane or below the last lane into a lane of their own, split out and removed; per drum lane play quantize (grid or swing groove) and nudge; the engine rule that a drum lane only starts notes that begin after playback, unmute or solo began, pinned by tests; a Drum menu in the control registry, and accessibility elements so `vizctl tree/click/drag` drive the grid end to end.

**Architecture:** Rust first, so a later port mostly redraws (the user's guideline). `core-model` gains `DrumCmd::Paint` (add hits to a row, skipping cells already hit) and play-quantize grooves (`QuantizeSpec.groove`, `QuantizeSpec::swing`). A new platform-neutral crate `core-editor` holds the drum grid's logic as pure functions of plain data: the vertical layout of lanes and rows, what a height is over, the velocity a height means, matrix cells, paint/erase strokes, where a dragged row lands, the play-quantize menu, nudge steps and the next row's note and General MIDI name. `core-ffi` exposes `Session.drum_lanes(seq)` (drum lanes with rows and hits), the `EditCommand::Drum` group the editors spec §3.2 lists, and `core-editor` as `drum_*` functions over records. Swift keeps state and draws: `DrumGridSettings` and `DrumGridModel` (`VizDAWAppModel`), `DrumGridView` (an `EditorCanvas`) and `DrumRowHeaderView` (`VizDAWEditors`), the pane with its bar and the Drum menu (`VizDAW`); one `DrumCommands` implementation serves the menu and the control channel.

**Tech Stack:** Rust 2024 (`core-model`, `core-editor` (new), `core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`, `im` 15, `serde`, `proptest`), Swift 6 / SwiftPM (SwiftUI shell, AppKit `NSView` canvases with Core Graphics, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-16-editors-design.md` (§4e — binding; §3.2 the `Drum` edit group; §3.5 canvas, tools, accessibility; §5 errors; §6 testing), parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` §9 ("Drum grid" paragraph), `docs/superpowers/specs/2026-10-15-core-model-design.md` (drum layouts, `DrumCmd`, play quantize and shift), `docs/superpowers/specs/2026-10-18-control-channel-design.md` §6 (registry), §5.5 (element clicks). Previous plans for conventions: `docs/superpowers/plans/2026-10-17-editors-strip-chart.md` (closest model), `2026-10-18-control-channel.md`.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools; a `git commit` without `DEVELOPER_DIR` fails silently — check every commit with `git log -1`). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp4-editors` (code head `f58f61e` before Task 1, plus this plan's commit). Commit at the end of every task, adding only the files the task lists (and `Cargo.lock` when a manifest changed); never push. End every commit message with the attribution lines your harness requires (the blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and the `Claude-Session:` line).
- Clean room: never read or copy anything from `~/workspace/SVP/research` or the Studio Vision binary. General MIDI percussion names are public knowledge.
- Never use GUI or computer-use tools. Launch VizDAW only through `vizctl launch` or the e2e test (a throwaway home under `/tmp`), quit it when done (`vizctl quit`; `pkill -f "VizDAW --control"` if a test left one), and check `pgrep -lf MacOS/VizDAW` prints nothing at the end of a task that launched it.
- Rust stable, edition 2024; manifests keep `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`, `publish = false`. License GPL-3.0-only. New crate: `core-editor` only (depends on `core-model`; dev-dependency `proptest`). New dependencies only between workspace crates (`core-ffi` → `core-editor`; `vizctl` dev → `core-engine`, `core-model`).
- `core-editor`: `#![forbid(unsafe_code)]`, `#![deny(missing_docs)]`, `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`, platform-neutral. `core-model`, `core-engine`, `core-ffi` keep their lints. Tests may `unwrap` and index. No new `unsafe`.
- File compatibility: `core_model::file::VERSION` stays 2. `QuantizeSpec.groove` is skipped when empty and defaults when absent, so projects without a groove read and write exactly as before (the insta snapshots do not change); a build before 4e refuses a project with a groove at the unknown field. `DrumCmd::Paint` is a command, never file data.
- No panic crosses the FFI: every new exported function returns `Result<_, VizError>` and runs inside `ffi_guard`. No session `inner` lock is held across plugin code.
- UniFFI: `EditCommand::Drum` is appended at the END of `EditCommand`; the new records and enums are new. The one existing record that gains a field is `QuantizeRow` (`groove: Vec<i64>`, last, `#[uniffi(default)]`, so Swift's memberwise initialiser keeps compiling; it loses `Copy`). A `Vec<u8>` argument crosses as Swift `Data`.
- `FakeSession` implements the whole generated `SessionProtocol`: it gains `drumLanes(seq:)` in the same task as `Session::drum_lanes` (Task 4).
- Swift: the generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings (the `MemoryDefaults` Sendable warning is older). Never use SwiftUI's `@State` or `@FocusState` in new views; keep view state in `@Observable` models. Every Swift `switch` over a generated enum that gains a case is updated in the same task as the Rust change. Models and pure logic are tested (Swift Testing, `@Test`, `#expect`); AppKit views get behaviour tests through internal entry points. Closures stored by views capture `[weak self]`.
- Drawing (editors spec §2): custom `NSView`s (an `EditorCanvas` subclass) with Core Graphics; no Metal.
- Control channel (control spec §6, §7): programmatic calls never open dialogs; every new menu command is in `core_control::registry::COMMANDS` (38 today; the count test moves to 44) and handled by `ControlCommands.run` (the test `everyRegisteredMenuCommandHasAHandler` compares the two).
- Generated files are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command after a Rust change.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Run `cargo fmt --all` first. Tasks 7, 9, 10 and 11 also run `scripts/make-app.sh`; Tasks 9–11 the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1`). Baseline before Task 1: 827 Rust tests listed by `cargo test --workspace -- --list` (823 pass, 4 ignored), 581 Swift `@Test` functions.
- Code in the steps is complete for new files (written whole with `cat > … <<'VIZDAW_EOF'`). Changes to existing files are exact **Find / Replace** excerpts of the tree the previous task left, in order; apply them with an editor (the Edit tool). If an excerpt does not match because a fix landed meanwhile, make the same change by hand and keep going.
- Spec values used verbatim: 15 360 ticks per quarter note; "shows drum lanes only"; "rows with note, name and destination"; "step matrix and timeline views"; "paint tool toggles on click and paints/erases a run on drag with configurable spacing, duration and velocity"; "a shown velocity lane sets new hits' velocity from the vertical pointer position"; "rows: add, reorder, drag to another drum lane or below the last to create one, split out into its own lane (existing `Drum` commands)"; "per drum lane play quantize (grid or groove) and nudge"; "a drum lane plays a note only if it starts after playback/unmute/solo began"; one drag is one undo step; editors never hold Rust objects across the run loop.

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no requirement spells out, each pinned by a named test in its owning task:

1. **A stroke that leaves its row or the screen** — dragged above or below the row, far left of its start, or out to an extreme tick, with a spacing larger than the step. Expected: it stays on its own row, runs from its first cell towards the pointer (shrinking when the pointer comes back), never holds more than 4 096 hits or a tick beyond ±2^40, and its hits sit on its stride. Pinned: Task 3 `strokes_stay_bounded`, `a_press_on_an_empty_cell_paints_and_a_drag_paints_a_run`; Task 6 `aDragPaintsARunAndEscapeTakesItBack` ("the stroke stays on its row").
2. **A stroke that ends badly** — Escape mid-drag, a release that never comes (the window closes, a new press arrives), another drag still finishing, an eraser click on an empty cell. Expected: no undo step, no edit left open, a status line message when refused. Pinned: Task 5 `escapeAndRefusalsLeaveNoUndoStep`; Task 6 `aDragPaintsARunAndEscapeTakesItBack`.
3. **Painting where hits already are** — a run over cells that already hold hits, the same cell twice in one paint, a click on a lit cell, two grids on one lane. Expected: no stacked duplicates (a cell holds what it held plus at most one new hit), a click on a hit erases it, and undo restores exactly. Pinned: Task 1 `paint_adds_hits_where_the_row_has_none_within_a_window`, `hostile_paints_apply_cleanly_or_change_nothing`; Task 4 `a_click_on_an_empty_cell_paints_and_one_on_a_hit_erases`.
4. **Hostile values from outside** — a hand-edited `ui.json` (negative step, velocity 200, a string for spacing), a damaged project's groove, a control-channel call with stray parameters, FFI records Swift built from stale state (an unknown lane index, NaN heights). Expected: clamped, refused with an error, or ignored; never a panic, `Internal`, or a changed document. Pinned: Task 2 `check_refuses_bad_grooves`, `hostile_quantize_never_panics_and_stays_near`; Task 4 `grid_functions_answer_any_input`, `hostile_drum_edits_apply_or_change_nothing`; Task 5 `settingsDecodeTolerantly`; Task 8 `the_drum_menu_commands_are_registered`.
5. **The document changing under the grid** — a busy session while a plugin editor spins the run loop, a failed read, another window adding or splitting rows. Expected: Busy keeps the lanes on screen, a failure shows once in the status line, the grid follows every edit and undo. Pinned: Task 5 `busyKeepsTheLanesAndAFailureShowsOnce`; Task 4 `every_drum_edit_applies_and_undoes`.

## Plan Rulings

Decisions the spec leaves open, each implemented and tested as stated (format: Ruling: what — why — cost if wrong):

1. **Ruling:** The grid's logic lives in a new crate `core-editor` (`core_editor::drum`): layout, spots, velocity from height, cells, strokes, row drops, the play-quantize menu, nudge steps, the next row; `core-ffi` wraps it as `drum_*` functions over records that Swift calls with copies of its rows (no Rust object is held across the run loop). The shared `TimeAxis`, `Snap` and `ToolMachine` stay Swift (every editor uses them): a stroke is handed a tick already snapped — why: the user's guideline (logic in Rust where cheap, so a port only redraws) — cost if wrong: one FFI call per pointer move and per drawn row (measured cheap; cells are capped at 4 096).
2. **Ruling:** The drum grid shows every drum lane of its target's sequence, in lane order, each as a header band and one band per row, then a "Drop a row here to give it a lane of its own" band; a block target scrolls to the block's start. One `DrumGridModel` per slot (the pane, each torn window), like the Strip Chart; the bar's settings are shared (`EditorLayout.drumGrid`) — cost if wrong: none (a lane filter can come later).
3. **Ruling:** A lane becomes a drum lane (or a normal lane again, keeping its notes) from its header's context menu "Drum Lane" or Drum > Drum Lane (the selection's first lane in the shown sequence) — why: no UI could mark one before — cost if wrong: none.
4. **Ruling:** Step matrix: cells of `step` ticks counted from tick 0 (bar 1 beat 1); a hit fills the cell its start falls in; a cell's brightness follows its loudest hit. Timeline: hits at their true start and length, at least 4 points wide — cost if wrong: grids for odd meters need a bar-relative origin later.
5. **Ruling:** The paint tool: the shared tool's arrow and pencil paint, the eraser erases. A press on a hit (in the matrix, a lit cell; in the timeline, a hit under the pointer) makes the stroke erase, anywhere else paint; the stroke stays on its row; its run goes from its first position (a cell start in the matrix, the press snapped to the shared grid in the timeline) towards the pointer every stride — the spacing, rounded to whole cells (at least one) in the matrix — and shrinks when the pointer comes back; at most 4 096 hits (`MAX_PAINT_HITS`) — cost if wrong: a different feel for runs.
6. **Ruling:** A row whose velocity lane is shown grows by `velocityHeight`; over it the pointer's height in the whole row (cells and lane) sets each new hit's velocity, bottom 1, top 127, clamped while a stroke on it is dragged elsewhere; each hit keeps the velocity it got when the run first reached it. Rows without the lane use the bar's velocity. 7-bit velocities become 16-bit as `v << 9` (the pencil's rule) — why: "the vertical pointer position" has no range in the spec — cost if wrong: a re-tuned mapping.
7. **Ruling:** Painting is a core command `DrumCmd::Paint { seq, lane, row, hits, window, duration }` that adds a note of the row's pitch carrying the row's destination at each hit unless the row has a note starting within `[tick, tick + window)` (a cell in the matrix, one tick in the timeline); erasing is `EventEdit::Delete` of the hits the run covers (matrix: hits in the run's cells; timeline: hits touching the swept range). A stroke is one gesture and one undo step labelled "Paint Drum Hits" or "Erase Drum Hits"; a click is a gesture too; an erase that covers nothing opens none — cost if wrong: none.
8. **Ruling:** Rows are dragged by their header; release height decides (`core_editor::drum::drop_row`): over its own lane → `ReorderRow` to that index; over another drum lane → `MoveRowToLane` (appended; the core keeps the sound by pinning the destination); over the new-lane band or anything below → `SplitRowToLane` (a lane after the source, named after the row). Rows stay within their sequence (the core command's rule) — cost if wrong: none.
9. **Ruling:** "A drum lane plays a note only if it starts after playback/unmute/solo began" already holds: the engine never starts a note mid-way in any lane (Play and Locate fire events at or after the position; a schedule swap — every mute, solo or edit — cuts sounding notes and fires only later events, the 2b ruling). Task 2 pins it for drum lanes (play mid-note, unmute mid-note, solo mid-note) instead of adding a flag; a future note chase must skip drum lanes — cost if wrong: a later chase forgets the exception (the tests catch it).
10. **Ruling:** Play quantize groove: `QuantizeSpec` gains `groove: Vec<Tick>` (grid line `k` moves by `groove[k % len]` before the strength applies, like `Quantize::Groove`; at most 64 offsets, each shorter than the grid); `QuantizeSpec::swing(grid, percent)` makes `[0, late]` with `late = 2·grid·(percent − 50)/100` (50–75 %). The lane menu offers Off, 1/4, 1/8, 1/8 Triplet, 1/16, 1/16 Triplet, 1/32 and 1/8 and 1/16 swing at 54, 58, 62 and 66 %, all at full strength; any other value reads "Custom". Nudge is the lane's play shift in steps of a 128th note (120 ticks, about 3.9 ms at 120 BPM), within a sixteenth either way — why: the model had a grid-only play quantize and no groove; "nudge" has no unit in the spec — cost if wrong: more grooves later (the type takes any).
11. **Ruling:** `DrumGridSettings` (`EditorLayout.drumGrid`, tolerant decoding): view, step (default 1/16), spacing (1/16), length (1/32), velocity (100), row height (20 pt), velocity lane height (28 pt), the grid's own zoom (96 ticks per point); Rust clamps whatever it is given (step a 128th note to a whole note, else a sixteenth; spacing and length 1 tick to 16 quarters; velocity 1–127; heights 12–64 and 16–96 pt, else the defaults) — cost if wrong: none.
12. **Ruling:** Accessibility: the grid lists the visible cells (matrix) or hits (timeline), at most 2 048, labelled "Kick, bar 1 beat 2 tick 3840, on, velocity 96" or "…, off"; pressing one toggles it like a click. The row headers list "Kick, C2, Drums, Kit" (name, note, lane, destination); pressing one selects it. These are the elements `vizctl find/click` uses — cost if wrong: none.
13. **Ruling:** A Drum menu with six registry commands (count 38 → 44): `drum.mark_lane` (`on`), `drum.add_row`, `drum.show_velocity` (`on`), `drum.split_row`, `drum.remove_row`, `drum.timeline` (`on`); no default shortcuts (plan 4f binds keys). Row commands act on the selected row of the drum grid last pressed in (else the pane's when it shows one); without one they are `unavailable`. One implementation (`DrumCommands`) serves the menu and `command.run`; none opens a dialog: Add Row takes the next free note of a kit order (Kick, Snare, Closed Hat, Open Hat, Clap, Crash, Ride, toms…) with its General MIDI name; renaming is inline — cost if wrong: none.
14. **Ruling:** Channel clicks: `NSApp.postEvent` renumbers events, so `ControlUI.isChannel` also recognises the timestamps of the mouse events the channel posted (the newest 256) — found while planning: every single click the channel sent to a canvas of a background VizDAW was spent on activating the window (the control plan's e2e only clicked a toolbar button) — cost if wrong: none (a fix).
15. **Ruling:** The e2e builds its own project (one top-level sequence "Beat" with a drum lane), opens it with `app.open` and tears the drum grid off (a default window leaves the docked pane too short to show rows) — cost if wrong: none.
16. **Ruling:** Not in 4e: notes sounding while painted (the spec does not ask; the piano roll's audition can be reused later), moving hits with the arrow tool (the piano roll and list do it), drum rows across sequences — cost if wrong: small follow-ups.
17. **Ruling:** A press in the grid or on a row header selects that row (`DrumGridModel.selectedRow`); painting does not change the document-wide selection — cost if wrong: none.
18. **Ruling:** New Swift tests go to `VizDAWAppModelTests` (they need `FakeSession`), as in 4c Ruling 26 and 4d Ruling 27.

## Carried and parked items

Picked up by this plan:
- 4a ledger, Task 5 minor: "no core-model cmd_drum test for SetRowShowVelocity (add in 4e)" → Task 1 `showing_a_rows_velocity_is_one_undo_step`.
- Found while planning (control channel): channel single clicks lost on canvases of a background VizDAW → Task 9.

Left untouched (they belong to other plans or stay parked as their reviews decided):
- 4d final fix report: scale time can still act on selected notes of lanes the piano roll does not show (needs a lane condition or modify-by-IDs; Minor 13); bridged AU v3 automation echo unverified (README step 76).
- Control final review, stay-parked list: Task 6 #3 stale `status` after a silent refusal and Task 11 M1 element click under the toolbar (plan 4f); F18 `thirtySecond` on the wire (decide in 4f); e2e temp homes left in `/tmp` (Task 11 M3; this plan's e2e keeps the existing `tempdir().keep()` pattern); the other Task 1–11 minors as triaged there; `app.quit` while a channel-opened menu tracks (one manual check).
- 4b/4c/4d minors already closed or parked by their final fix waves.

## Spec gaps and contradictions found while planning

- "Shows drum lanes only" says nothing about which lanes for a block, lane or sequence target (Ruling 2).
- "Configurable spacing" in a step matrix whose spacing is not a whole number of cells (Ruling 5).
- "Vertical pointer position" gives no range or which part of the row (Ruling 6).
- "Play quantize (grid or groove)": the model's play quantize was grid-only and there was no groove for lanes (Ruling 10); "nudge" has no unit (Ruling 10).
- "Drag to another drum lane": the core moves rows only within a sequence (Ruling 8).
- The engine rule already holds for every lane (Ruling 9).
- Editors spec §3.2 lists a `Drum` edit group that 4a did not build (Task 4).
- Parent §9: "each row naming its note and destination" — rows show both; a row without its own destination names the lane's (Ruling 12).

## File Structure

```
crates/core-model/
  src/command/drum.rs          DrumHit, MAX_PAINT_HITS, DrumCmd::Paint (Task 1)
  src/lane.rs                  QuantizeSpec.groove, grid(), swing(), check(), apply(&self), MAX_GROOVE (2)
  src/command/{lane,reference}.rs, src/query/flatten.rs, src/rules.rs  groove checks and clones (2)
  tests/cmd_drum.rs            paint and show-velocity tests, hostile paints (1)
  tests/play_groove.rs         swing, checks, flatten, hostile quantize (2)
  tests/{model_types,cmd_lane,cmd_reference}.rs  QuantizeSpec::grid (2)
crates/core-engine/tests/drum_lanes.rs   clean starts, swung playback (2)
crates/core-editor/            new crate: src/{lib,drum}.rs, tests/drum.rs (3)
crates/core-ffi/
  src/drum.rs                  drum lanes, DrumEdit, grid records and drum_* functions (4)
  src/edit.rs, src/lib.rs, src/session.rs, Cargo.toml  EditCommand::Drum, QuantizeRow.groove, drum_lanes (4)
  tests/drum.rs, tests/edit.rs (4)
crates/core-control/src/registry.rs, tests/commands.rs   the Drum menu (8)
crates/vizctl/Cargo.toml, tests/e2e.rs                     the drum grid end to end (10)
app/
  Sources/VizDAWAppModel/      DrumGrid, DrumGridModel, EditorLayout, DocumentModel (5); DrumCommands, ControlCommands (8)
  Sources/VizDAWEditors/       DrumGridView, DrumRowHeaderView (6); LaneHeaderView (7); ControlUI (9)
  Sources/VizDAW/              DrumGridPane, DetailPane, EditorWindows (7); AppCommands (8)
  Tests/VizDAWAppModelTests/   FakeSession (4); DrumGridModelTests (5); DrumGridViewTests (6);
                               ArrangementViewTests (7); ControlCommandsTests (8)
  Tests/VizDAWEditorsTests/ControlUITests (9)
  README.md                    drum grid checklist steps 87–98 (11)
```

---

### Task 1: Paint drum hits into a row — `DrumCmd::Paint`; the show-velocity command's carried test

**Files:**
- Modify: `crates/core-model/src/command/drum.rs`
- Test: `crates/core-model/tests/cmd_drum.rs`

**Interfaces:**
- Consumes: `core_model::command::event::insert_specs(d, seq, lane, &[EventSpec], r)` (inserts into the active take, validates, auto-creates drum rows), `rules::{check_positive, check_ticks}`, `DrumRow::matches(pitch, dest)`.
- Produces: `core_model::command::drum::{DrumHit { tick: Tick, velocity: u16 }, MAX_PAINT_HITS = 4096}`; `DrumCmd::Paint { seq: SeqId, lane: LaneId, row: DrumRowId, hits: Vec<DrumHit>, window: Tick, duration: Tick }` named "Paint Drum Hits".

Ruling 7. Everything is checked before anything changes (hit count, window and duration > 0 and within ±2^40, every tick within ±2^40, velocities > 0, the row exists), so a refused paint changes nothing; a paint whose every hit is already there changes nothing and `History` records no undo step. The 4a ledger parked a missing core test for `SetRowShowVelocity`; it comes first.

- [ ] **Step 1: Write the failing tests**

In `crates/core-model/tests/cmd_drum.rs` (the imports, then new tests at the end of the file):

Find:

```rust
use core_model::ModelError;
use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{
    AnyId, Counters, DestId, DrumRowId, EventId, LaneId, PluginInstanceId, SeqId,
};
use core_model::mixer::{PluginRef, PluginSlot};
use core_model::rules::validate;
use core_model::time::{PPQ, Tick};

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
```

Replace with:

```rust
use core_model::ModelError;
use core_model::command::drum::{DrumHit, MAX_PAINT_HITS};
use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Destination, Document, DocumentRepr, Settings};
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{
    AnyId, Counters, DestId, DrumRowId, EventId, LaneId, PluginInstanceId, SeqId,
};
use core_model::mixer::{PluginRef, PluginSlot};
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
```

Find:

```rust
        vec![AnyId::Event(EventId(0)), AnyId::Event(EventId(2))]
    );
}
```

Replace with:

```rust
        vec![AnyId::Event(EventId(0)), AnyId::Event(EventId(2))]
    );
}

/// The row's notes in lane 0's active take: `(tick, pitch, velocity, dest)`.
fn row_notes(d: &Document, pitch: u8) -> Vec<(i64, u8, u16, Option<DestId>)> {
    d.lane(SeqId(0), LaneId(0))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note {
                pitch: p, velocity, ..
            } if p == pitch => Some((e.tick.0, p, velocity, e.dest)),
            _ => None,
        })
        .collect()
}

fn paint(row: u64, hits: &[(i64, u16)], window: i64) -> Command {
    drum(DrumCmd::Paint {
        seq: SeqId(0),
        lane: LaneId(0),
        row: DrumRowId(row),
        hits: hits
            .iter()
            .map(|&(t, v)| DrumHit {
                tick: Tick(t),
                velocity: v,
            })
            .collect(),
        window: Tick(window),
        duration: Tick(PPQ / 8),
    })
}

#[test]
fn showing_a_rows_velocity_is_one_undo_step() {
    // Carried from plan 4a (Task 5): the command had no core test.
    let mut h = History::new(kit());
    let show = |on| {
        drum(DrumCmd::SetRowShowVelocity {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(1),
            show: on,
        })
    };
    h.execute(&show(true), &ViewFilter::default()).unwrap();
    let shown = |d: &Document| {
        d.lane(SeqId(0), LaneId(0))
            .unwrap()
            .drum
            .as_ref()
            .unwrap()
            .rows[1]
            .show_velocity
    };
    assert!(shown(h.doc()));
    assert_eq!(h.undo_label(), Some("Show Drum Row Velocity"));
    h.undo().unwrap();
    assert!(!shown(h.doc()));
    assert!(
        run(
            &kit(),
            drum(DrumCmd::SetRowShowVelocity {
                seq: SeqId(0),
                lane: LaneId(0),
                row: DrumRowId(9),
                show: true,
            })
        )
        .is_err(),
        "an unknown row is refused"
    );
}

#[test]
fn paint_adds_hits_where_the_row_has_none_within_a_window() {
    let q = PPQ / 4;
    // Row 0 (C2) has notes at 0 and 2 beats. A window of a 16th skips the
    // hit at 0, the one a little before beat 2, and the repeated one.
    let a = apply(
        &kit(),
        &paint(
            0,
            &[
                (0, 512),
                (q, 40_000),
                (2 * q, 512),
                (2 * PPQ - 100, 512),
                (q, 512),
            ],
            q,
        ),
    )
    .unwrap();
    assert_eq!(
        row_notes(&a.doc, 36),
        vec![
            (0, 36, 100, None),
            (q, 36, 40_000, None),
            (2 * q, 36, 512, None),
            (2 * PPQ, 36, 100, None),
        ]
    );
    assert_eq!(a.report.created.len(), 2, "{:?}", a.report);
    assert!(validate(&a.doc).is_empty());
    // A row that names a destination gives its notes that destination.
    let d = run(
        &kit(),
        drum(DrumCmd::AddRow {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Clap".into(),
            note: 39,
            dest: Some(DestId(1)),
        }),
    )
    .unwrap();
    let d = run(&d, paint(2, &[(0, 900)], q)).unwrap();
    assert_eq!(row_notes(&d, 39), vec![(0, 39, 900, Some(DestId(1)))]);
    // The drum view lists it in its own row (no new row was made).
    assert_eq!(rows(&d, 0).len(), 3);
}

#[test]
fn paint_refuses_bad_input_and_changes_nothing() {
    let max = 1i64 << 40;
    let bad = [
        paint(0, &[(0, 0)], 10),
        paint(0, &[(max + 1, 1)], 10),
        paint(0, &[(0, 1)], 0),
        paint(0, &[(0, 1)], max + 1),
        paint(9, &[(0, 1)], 10),
        drum(DrumCmd::Paint {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            hits: vec![],
            window: Tick(10),
            duration: Tick(0),
        }),
        drum(DrumCmd::Paint {
            seq: SeqId(0),
            lane: LaneId(1),
            row: DrumRowId(0),
            hits: vec![],
            window: Tick(10),
            duration: Tick(10),
        }),
        drum(DrumCmd::Paint {
            seq: SeqId(0),
            lane: LaneId(0),
            row: DrumRowId(0),
            hits: vec![
                DrumHit {
                    tick: Tick(0),
                    velocity: 1
                };
                MAX_PAINT_HITS + 1
            ],
            window: Tick(10),
            duration: Tick(10),
        }),
    ];
    for c in bad {
        let mut h = History::new(kit());
        assert!(h.execute(&c, &ViewFilter::default()).is_err(), "{c:?}");
        assert_eq!(h.doc(), &kit());
        assert_eq!(h.undo_depth(), 0);
    }
    // A paint whose every hit is already there adds no undo step.
    let mut h = History::new(kit());
    h.execute(
        &paint(0, &[(0, 1), (2 * PPQ, 1)], 10),
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(h.undo_depth(), 0);
}

fn arb_hits() -> impl Strategy<Value = Vec<(i64, u16)>> {
    let tick = prop_oneof![
        -(4 * PPQ)..(8 * PPQ),
        Just(i64::MIN),
        Just(i64::MAX),
        Just((1 << 40) - 1),
        Just(-(1 << 40)),
    ];
    prop::collection::vec((tick, any::<u16>()), 0..40)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Hostile paints (any ticks, velocities, windows, durations, rows,
    /// lanes) never panic; one that applies leaves a valid document whose
    /// new notes are the row's, at distinct ticks, none with an older note
    /// of the row starting within its window, and undo brings the
    /// original back.
    #[test]
    fn hostile_paints_apply_cleanly_or_change_nothing(
        hits in arb_hits(),
        window in prop_oneof![-5i64..PPQ, Just(i64::MAX), Just(1i64 << 40)],
        duration in prop_oneof![-5i64..PPQ, Just(i64::MAX)],
        row in 0u64..4,
        lane in 0u64..3,
    ) {
        let mut h = History::new(kit());
        let cmd = drum(DrumCmd::Paint {
            seq: SeqId(0),
            lane: LaneId(lane),
            row: DrumRowId(row),
            hits: hits.iter().map(|&(t, v)| DrumHit { tick: Tick(t), velocity: v }).collect(),
            window: Tick(window),
            duration: Tick(duration),
        });
        match h.execute(&cmd, &ViewFilter::default()) {
            Err(_) => prop_assert_eq!(h.doc(), &kit()),
            Ok(report) => {
                prop_assert!(validate(h.doc()).is_empty(), "{:?}", validate(h.doc()));
                let pitch = if row == 0 { 36 } else { 38 };
                let before: Vec<i64> = row_notes(&kit(), pitch).iter().map(|n| n.0).collect();
                let after: Vec<i64> = row_notes(h.doc(), pitch).iter().map(|n| n.0).collect();
                prop_assert_eq!(after.len(), before.len() + report.created.len());
                let added: Vec<i64> = after.iter().copied().filter(|t| !before.contains(t)).collect();
                prop_assert_eq!(added.len(), report.created.len(), "distinct new ticks");
                for a in &added {
                    prop_assert!(
                        !before.iter().any(|b| b >= a && *b < a.saturating_add(window)),
                        "{a} has an older note within {window}"
                    );
                }
                let text = core_model::file::to_json(h.doc()).unwrap();
                prop_assert_eq!(&core_model::file::from_json(&text).unwrap(), h.doc());
                while h.undo_depth() > 0 {
                    h.undo().unwrap();
                }
                prop_assert_eq!(h.doc(), &kit());
            }
        }
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_drum 2>&1 | grep -E "^error" | head -5
```

Expected: FAIL to compile: `unresolved imports core_model::command::drum::DrumHit, core_model::command::drum::MAX_PAINT_HITS` and `no variant named Paint`.

- [ ] **Step 3: Implement**

In `crates/core-model/src/command/drum.rs`:

Find:

```rust
//! `(pitch, dest override)`; membership is derived, never stored.

use super::Report;
use super::event::note_name;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::ids::{AnyId, DestId, DrumRowId, EventId, LaneId, SeqId};
use crate::lane::{DrumLayout, DrumRow, Lane};
use std::collections::BTreeSet;

/// Drum-lane edits.
#[derive(Debug, Clone, PartialEq)]
pub enum DrumCmd {
```

Replace with:

```rust
//! `(pitch, dest override)`; membership is derived, never stored.

use super::Report;
use super::event::{insert_specs, note_name};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec};
use crate::ids::{AnyId, DestId, DrumRowId, EventId, LaneId, SeqId};
use crate::lane::{DrumLayout, DrumRow, Lane};
use crate::rules::{check_positive, check_ticks};
use crate::time::Tick;
use std::collections::BTreeSet;

/// The most hits one [`DrumCmd::Paint`] may carry (a drum grid stroke
/// stops growing there).
pub const MAX_PAINT_HITS: usize = 4096;

/// One hit a paint stroke adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrumHit {
    /// Start.
    pub tick: Tick,
    /// 16-bit attack velocity (> 0).
    pub velocity: u16,
}

/// Drum-lane edits.
#[derive(Debug, Clone, PartialEq)]
pub enum DrumCmd {
```

Find:

```rust
        /// Row.
        row: DrumRowId,
    },
}

impl DrumCmd {
```

Replace with:

```rust
        /// Row.
        row: DrumRowId,
    },
    /// Paint hits into a row (the drum grid's paint tool): a note of the
    /// row's pitch, carrying the row's destination, at each hit's tick in
    /// the active take, unless the row already has a note starting in
    /// `[tick, tick + window)` (a hit painted earlier in the list counts).
    Paint {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Row.
        row: DrumRowId,
        /// The hits, at most [`MAX_PAINT_HITS`].
        hits: Vec<DrumHit>,
        /// A cell: a hit is skipped when a note starts this close after it (> 0).
        window: Tick,
        /// Length of each new note (> 0).
        duration: Tick,
    },
}

impl DrumCmd {
```

Find:

```rust
            DrumCmd::RemoveRow { .. } => "Remove Drum Row",
            DrumCmd::MoveRowToLane { .. } => "Move Drum Row to Lane",
            DrumCmd::SplitRowToLane { .. } => "Split Drum Row to Lane",
        }
    }
}
```

Replace with:

```rust
            DrumCmd::RemoveRow { .. } => "Remove Drum Row",
            DrumCmd::MoveRowToLane { .. } => "Move Drum Row to Lane",
            DrumCmd::SplitRowToLane { .. } => "Split Drum Row to Lane",
            DrumCmd::Paint { .. } => "Paint Drum Hits",
        }
    }
}
```

Find:

```rust
            r.created.extend([AnyId::Lane(new_lane), AnyId::Take(take)]);
            move_row(d, *seq, *lane, *row, new_lane, r)?;
        }
    }
    Ok(())
}
```

Replace with:

```rust
            r.created.extend([AnyId::Lane(new_lane), AnyId::Take(take)]);
            move_row(d, *seq, *lane, *row, new_lane, r)?;
        }
        DrumCmd::Paint {
            seq,
            lane,
            row,
            hits,
            window,
            duration,
        } => paint(d, *seq, *lane, *row, hits, *window, *duration, r)?,
    }
    Ok(())
}

/// [`DrumCmd::Paint`]: everything is checked before anything changes.
#[allow(clippy::too_many_arguments)]
fn paint(
    d: &mut Document,
    seq: SeqId,
    lane: LaneId,
    row: DrumRowId,
    hits: &[DrumHit],
    window: Tick,
    duration: Tick,
    r: &mut Report,
) -> Result<()> {
    if hits.len() > MAX_PAINT_HITS {
        return Err(ModelError::range("drum_paint.hits", hits.len()));
    }
    check_positive("drum_paint.window", window)?;
    check_ticks("drum_paint.window", window.0)?;
    check_positive("drum_paint.duration", duration)?;
    check_ticks("drum_paint.duration", duration.0)?;
    for h in hits {
        check_ticks("drum_paint.tick", h.tick.0)?;
        if h.velocity == 0 {
            return Err(ModelError::range("drum_paint.velocity", h.velocity));
        }
    }
    let x = find_row(d, seq, lane, row)?;
    let l = d.lane(seq, lane)?;
    let mut starts: BTreeSet<i64> = l
        .active()
        .map(|t| {
            t.events
                .values()
                .filter(
                    |e| matches!(e.body, EventBody::Note { pitch, .. } if x.matches(pitch, e.dest)),
                )
                .map(|e| e.tick.0)
                .collect()
        })
        .unwrap_or_default();
    for h in hits {
        let end = h.tick.0.saturating_add(window.0);
        if starts.range(h.tick.0..end).next().is_some() {
            continue;
        }
        let mut spec = EventSpec::new(h.tick, EventBody::note(x.note, h.velocity, duration));
        spec.dest = x.dest;
        insert_specs(d, seq, lane, &[spec], r)?;
        starts.insert(h.tick.0);
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_drum 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: `test result: ok. 13 passed`.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
git add crates/core-model/src/command/drum.rs crates/core-model/tests/cmd_drum.rs
git commit -m "feat(model): paint drum hits into a row; a test for showing a row's velocity

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: no failure; 831 Rust tests listed (827 + 4); the commit shows in `git log -1`. Swift is unchanged (no FFI change).

---

### Task 2: Play quantize grooves (swing) for lanes; drum lanes start notes cleanly in playback

**Files:**
- Modify: `crates/core-model/src/lane.rs`, `crates/core-model/src/command/lane.rs`, `crates/core-model/src/command/reference.rs`, `crates/core-model/src/query/flatten.rs`, `crates/core-model/src/rules.rs`, `crates/core-ffi/src/edit.rs`
- Modify (tests that build a `QuantizeSpec`): `crates/core-model/tests/model_types.rs`, `crates/core-model/tests/cmd_lane.rs`, `crates/core-model/tests/cmd_reference.rs`
- Create: `crates/core-model/tests/play_groove.rs`, `crates/core-engine/tests/drum_lanes.rs`

**Interfaces:**
- Consumes: `core_engine::fixtures::Builder::{new, dest, sequence, lane, notes, drum_rows, run, from_doc, finish}`, `OfflineBackend`, `OfflineSinks::log()`, `Engine::{new, load, play, locate_tick}`, `EventLog::drain`, `LogRecord::line` ("frame t0 90 pp vv").
- Produces: `core_model::lane::{QuantizeSpec { grid: Tick, strength: u8, groove: Vec<Tick> }, MAX_GROOVE = 64}` with `QuantizeSpec::grid(grid, strength)`, `QuantizeSpec::swing(grid, percent)`, `check(&self) -> Result<()>`, `apply(&self, Tick) -> Tick`; `QuantizeSpec` is `Clone` (no longer `Copy`).

Rulings 9 and 10. The groove follows `Quantize::Groove`'s rule (grid line `k`, counted from tick 0, moves by `groove[k % len]`, then the strength scales the move). `#[serde(default, skip_serializing_if = "Vec::is_empty")]` keeps files without a groove byte-identical (the insta snapshots must not change). `SetPlayQuantize` and project validation both use `check()`. The engine needs no change: its tests pin the drum-lane rule (Ruling 9) and show a swung lane playing late. `core-ffi`'s conversion becomes `QuantizeSpec::grid` here; Task 4 carries the groove through.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-model/tests/play_groove.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-model/tests
cat > crates/core-model/tests/play_groove.rs <<'VIZDAW_EOF'
//! Play quantize with a groove (editors spec §4e: per drum lane play
//! quantize, grid or groove).

use core_model::command::{Command, DrumCmd, EventCmd, LaneCmd, RoutingCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{LaneId, SeqId};
use core_model::lane::{MAX_GROOVE, QuantizeSpec};
use core_model::query::flatten;
use core_model::rules::validate;
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

const E: i64 = PPQ / 2;

#[test]
fn swing_moves_every_second_line_late() {
    let s = QuantizeSpec::swing(Tick(E), 66);
    // 66 % of a quarter for the first eighth: the second starts 16 % of a
    // quarter (0.32 of an eighth) late.
    assert_eq!(s.groove, vec![Tick(0), Tick(2_457)]);
    assert_eq!(s.apply(Tick(10)), Tick(0));
    assert_eq!(s.apply(Tick(E + 10)), Tick(E + 2_457));
    assert_eq!(s.apply(Tick(2 * E - 30)), Tick(2 * E));
    assert_eq!(
        s.apply(Tick(-E)),
        Tick(-E + 2_457),
        "lines count from tick 0"
    );
    assert_eq!(
        QuantizeSpec::swing(Tick(E), 50).apply(Tick(E + 10)),
        Tick(E)
    );
    assert_eq!(
        QuantizeSpec::swing(Tick(E), 99).groove[1],
        Tick(E / 2),
        "clamped to 75 %"
    );
    let mut half = s.clone();
    half.strength = 50;
    assert_eq!(half.apply(Tick(E)), Tick(E + 1_228));
    assert!(s.check().is_ok());
}

#[test]
fn check_refuses_bad_grooves() {
    let bad = [
        QuantizeSpec::grid(Tick(0), 100),
        QuantizeSpec::grid(Tick((1 << 40) + 1), 100),
        QuantizeSpec::grid(Tick(E), 101),
        QuantizeSpec {
            groove: vec![Tick(E)],
            ..QuantizeSpec::grid(Tick(E), 100)
        },
        QuantizeSpec {
            groove: vec![Tick(-E)],
            ..QuantizeSpec::grid(Tick(E), 100)
        },
        QuantizeSpec {
            groove: vec![Tick(0); MAX_GROOVE + 1],
            ..QuantizeSpec::grid(Tick(E), 100)
        },
    ];
    for q in bad {
        assert!(q.check().is_err(), "{q:?}");
    }
    assert!(
        QuantizeSpec {
            groove: vec![Tick(E - 1), Tick(1 - E)],
            ..QuantizeSpec::grid(Tick(E), 100)
        }
        .check()
        .is_ok()
    );
}

/// One sequence with a drum lane: notes at 0, an eighth and a quarter
/// plus an eighth (off the grid by 20 ticks each).
fn drums() -> Document {
    let mut d = Document::new();
    for c in [
        Command::Routing(RoutingCmd::AddDestination {
            name: "Kit".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "e".into(),
                group: 0,
                channel: 9,
            },
            default_patch: None,
        }),
        Command::Sequence(SequenceCmd::New {
            name: "Beat".into(),
        }),
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(core_model::ids::DestId(0)),
        }),
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: [20, E + 20, PPQ + E - 20]
                .iter()
                .map(|&t| EventSpec::new(Tick(t), EventBody::note(42, 100, Tick(E / 2))))
                .collect(),
        }),
        Command::Drum(DrumCmd::MarkDrumLane {
            seq: SeqId(0),
            lane: LaneId(0),
            drum: true,
        }),
    ] {
        d = apply(&d, &c).unwrap().doc;
    }
    d
}

fn starts(d: &Document) -> Vec<i64> {
    flatten(d, SeqId(0), None, &ViewFilter::default())
        .unwrap()
        .map(|r| r.tick.0)
        .collect()
}

#[test]
fn a_groove_plays_through_flatten_and_survives_save() {
    let set = |q: Option<QuantizeSpec>| {
        Command::Lane(LaneCmd::SetPlayQuantize {
            seq: SeqId(0),
            lane: LaneId(0),
            quantize: q,
        })
    };
    let d = apply(&drums(), &set(Some(QuantizeSpec::swing(Tick(E), 66))))
        .unwrap()
        .doc;
    assert_eq!(starts(&d), vec![0, E + 2_457, PPQ + E + 2_457]);
    assert!(validate(&d).is_empty());
    let text = core_model::file::to_json(&d).unwrap();
    assert!(text.contains("\"groove\""), "the groove is saved");
    assert_eq!(core_model::file::from_json(&text).unwrap(), d);
    // A plain grid writes no groove key: older builds read the file.
    let d = apply(&d, &set(Some(QuantizeSpec::grid(Tick(E), 100))))
        .unwrap()
        .doc;
    assert!(!core_model::file::to_json(&d).unwrap().contains("groove"));
    assert_eq!(starts(&d), vec![0, E, PPQ + E]);
    // A bad groove is refused and changes nothing.
    let bad = QuantizeSpec {
        groove: vec![Tick(E)],
        ..QuantizeSpec::grid(Tick(E), 100)
    };
    assert!(apply(&d, &set(Some(bad))).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any grid, strength, groove and tick: applying never panics, and a
    /// spec that passes `check` moves a start by less than two grids.
    #[test]
    fn hostile_quantize_never_panics_and_stays_near(
        grid in prop_oneof![-10i64..20_000, Just(i64::MAX), Just(i64::MIN), Just(1i64 << 40)],
        strength in any::<u8>(),
        groove in prop::collection::vec(prop_oneof![-20_000i64..20_000, Just(i64::MAX), Just(i64::MIN)], 0..8),
        tick in prop_oneof![any::<i64>(), -(1i64 << 40)..(1i64 << 40)],
    ) {
        let q = QuantizeSpec { grid: Tick(grid), strength, groove: groove.into_iter().map(Tick).collect() };
        let out = q.apply(Tick(tick));
        if q.check().is_ok() && tick.unsigned_abs() <= 1 << 40 {
            prop_assert!(i128::from(out.0 - tick).abs() < 2 * i128::from(grid), "{tick} -> {} by {q:?}", out.0);
        }
    }
}
VIZDAW_EOF
```

Create `crates/core-engine/tests/drum_lanes.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-engine/tests
cat > crates/core-engine/tests/drum_lanes.rs <<'VIZDAW_EOF'
//! Drum lanes in playback (editors spec §4e): a drum lane plays a note
//! only if the note starts after playback, unmute or solo began (Plan 4e
//! Ruling 9: no lane ever starts a note mid-way, so this pins the rule
//! for drum lanes), and a lane's groove play quantize moves its notes.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{EventLog, LogRecord, OfflineBackend, OfflineSinks};
use core_model::command::{Command, LaneCmd};
use core_model::document::Document;
use core_model::ids::{LaneId, SeqId};
use core_model::lane::QuantizeSpec;
use core_model::time::Tick;

const SR: f64 = 48_000.0;
/// Frames per quarter at the default 120 BPM.
const FQ: u64 = 24_000;

struct Run {
    engine: Engine,
    backend: OfflineBackend,
    log: EventLog,
}

fn start(doc: &Document, seq: SeqId) -> Run {
    let sinks = OfflineSinks::new(10_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(doc, seq).unwrap();
    let mut backend = OfflineBackend::new(SR, 512);
    backend.start(renderer).unwrap();
    Run {
        engine,
        backend,
        log,
    }
}

/// Note-ons only: `frame pitch`.
fn note_ons(log: &EventLog) -> Vec<(u64, u8)> {
    log.drain()
        .iter()
        .map(LogRecord::line)
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            match f.as_slice() {
                [frame, _, "90", pitch, vel] if *vel != "00" => {
                    Some((frame.parse().ok()?, u8::from_str_radix(pitch, 16).ok()?))
                }
                _ => None,
            }
        })
        .collect()
}

/// Sequence "Beat": drum lane "Drums" (kick held for two beats at 0,
/// snare at beat 2, hat at beat 3) and a plain lane "Keys" (a note at
/// beat 4); `flags` sets the drum lane's mute and the keys' solo first.
fn beat(drum_mute: bool, keys_solo: bool) -> (Document, SeqId, LaneId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, drums) = b.sequence("Beat", "Drums", Some(synth)).unwrap();
    b.drum_rows(seq, drums, &[("Kick", 36), ("Snare", 38), ("Hat", 42)])
        .unwrap();
    b.notes(
        seq,
        drums,
        &[
            (0, 36, 0x8000, 2 * Q),
            (Q, 38, 0x8000, Q / 4),
            (2 * Q, 42, 0x8000, Q / 4),
        ],
    )
    .unwrap();
    let keys = b.lane(seq, "Keys", Some(synth)).unwrap();
    b.notes(seq, keys, &[(3 * Q, 60, 0x8000, Q / 4)]).unwrap();
    for (lane, mute, solo) in [(drums, drum_mute, false), (keys, false, keys_solo)] {
        b.run(Command::Lane(LaneCmd::SetFlags {
            seq,
            lane,
            mute: Some(mute),
            solo: Some(solo),
            arm: None,
        }))
        .unwrap();
    }
    (b.finish(), seq, drums)
}

#[test]
fn a_drum_note_that_started_before_play_never_sounds() {
    let (doc, seq, _) = beat(false, false);
    let mut run = start(&doc, seq);
    // Half a beat into the held kick.
    run.engine.locate_tick(Tick(Q / 2)).unwrap();
    run.engine.play().unwrap();
    run.backend.run_frames(512 * 200);
    assert_eq!(
        note_ons(&run.log),
        vec![(FQ / 2, 38), (3 * FQ / 2, 42), (5 * FQ / 2, 60)],
        "the kick that began before the play position is not started"
    );
}

#[test]
fn unmuting_or_soloing_mid_note_waits_for_the_next_note() {
    // Muted drum lane, then soloed keys: either way the drum lane is
    // silent until a third of a beat in, when it is let through.
    for (muted, soloed) in [(true, false), (false, true)] {
        let (silent, seq, _) = beat(muted, soloed);
        let mut run = start(&silent, seq);
        run.engine.play().unwrap();
        run.backend.run_frames(512 * 16);
        let (heard, seq2, _) = if soloed {
            // Soloing the drum lane too lets it play beside the keys.
            let (d, s, drums) = beat(false, true);
            let mut b = Builder::from_doc(d);
            b.run(Command::Lane(LaneCmd::SetFlags {
                seq: s,
                lane: drums,
                mute: None,
                solo: Some(true),
                arm: None,
            }))
            .unwrap();
            (b.finish(), s, drums)
        } else {
            beat(false, false)
        };
        run.engine.load(&heard, seq2).unwrap();
        run.backend.run_frames(512 * 200);
        assert_eq!(
            note_ons(&run.log),
            vec![(FQ, 38), (2 * FQ, 42), (3 * FQ, 60)],
            "muted {muted} soloed {soloed}: the held kick does not start mid-way"
        );
    }
}

#[test]
fn a_swung_drum_lane_plays_its_offbeats_late() {
    let (doc, seq, drums) = beat(false, false);
    let mut b = Builder::from_doc(doc);
    b.notes(seq, drums, &[(Q / 2 + 30, 42, 0x8000, Q / 8)])
        .unwrap();
    b.run(Command::Lane(LaneCmd::SetPlayQuantize {
        seq,
        lane: drums,
        quantize: Some(QuantizeSpec::swing(Tick(Q / 2), 66)),
    }))
    .unwrap();
    let mut run = start(&b.finish(), seq);
    run.engine.play().unwrap();
    run.backend.run_frames(512 * 200);
    // The offbeat eighth plays at 66 % of the beat: tick 7680 + 2457,
    // frame 15 839 (rounded down) at 120 BPM.
    let ons = note_ons(&run.log);
    assert_eq!(ons.first(), Some(&(0, 36)));
    assert_eq!(ons.get(1), Some(&(15_839, 42)), "{ons:?}");
    assert_eq!(&ons[2..], &[(FQ, 38), (2 * FQ, 42), (3 * FQ, 60)]);
}
VIZDAW_EOF
```

The three older tests build `QuantizeSpec` literals; they now use `QuantizeSpec::grid`. In `crates/core-model/tests/model_types.rs`:

Find:

```rust

#[test]
fn quantize_spec_applies_strength() {
    let q = QuantizeSpec {
        grid: Tick(100),
        strength: 50,
    };
    assert_eq!(q.apply(Tick(140)), Tick(120));
    assert_eq!(q.apply(Tick(160)), Tick(180));
    let zero = QuantizeSpec {
        grid: Tick(0),
        strength: 100,
    };
    assert_eq!(zero.apply(Tick(7)), Tick(7));
}

```

Replace with:

```rust

#[test]
fn quantize_spec_applies_strength() {
    let q = QuantizeSpec::grid(Tick(100), 50);
    assert_eq!(q.apply(Tick(140)), Tick(120));
    assert_eq!(q.apply(Tick(160)), Tick(180));
    let zero = QuantizeSpec::grid(Tick(0), 100);
    assert_eq!(zero.apply(Tick(7)), Tick(7));
}

```

In `crates/core-model/tests/cmd_lane.rs`:

Find:

```rust
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec {
                    grid: Tick(0),
                    strength: 100
                })
            }
        )
        .is_err()
```

Replace with:

```rust
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec::grid(Tick(0), 100))
            }
        )
        .is_err()
```

Find:

```rust
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec {
                    grid: Tick(960),
                    strength: 101
                })
            }
        )
        .is_err()
```

Replace with:

```rust
            LaneCmd::SetPlayQuantize {
                seq: id.0,
                lane: id.1,
                quantize: Some(QuantizeSpec::grid(Tick(960), 101))
            }
        )
        .is_err()
```

In `crates/core-model/tests/cmd_reference.rs`:

Find:

```rust
        Command::Lane(LaneCmd::SetPlayQuantize {
            seq: SeqId(0),
            lane: LaneId(0),
            quantize: Some(QuantizeSpec {
                grid: Tick(Q),
                strength: 100,
            }),
        }),
    )
    .unwrap();
```

Replace with:

```rust
        Command::Lane(LaneCmd::SetPlayQuantize {
            seq: SeqId(0),
            lane: LaneId(0),
            quantize: Some(QuantizeSpec::grid(Tick(Q), 100)),
        }),
    )
    .unwrap();
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test play_groove 2>&1 | grep -E "^error" | head -3
cargo test -p core-engine --test drum_lanes 2>&1 | grep -E "^error" | head -3
```

Expected: FAIL to compile: `no function or associated item named swing found for struct QuantizeSpec` (and `grid`, `MAX_GROOVE`).

- [ ] **Step 3: Implement**

In `crates/core-model/src/lane.rs` (the imports, then `QuantizeSpec`):

Find:

```rust
//! Lanes (untyped tracks), takes and drum layouts.

use crate::event::Event;
use crate::ids::{DestId, DrumRowId, EventId, LaneId, OutputPathId, TakeId};
use crate::time::Tick;
```

Replace with:

```rust
//! Lanes (untyped tracks), takes and drum layouts.

use crate::error::ModelError;
use crate::event::Event;
use crate::ids::{DestId, DrumRowId, EventId, LaneId, OutputPathId, TakeId};
use crate::time::Tick;
```

Find:

```rust
    }
}

/// Non-destructive quantize: snap note starts toward a grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizeSpec {
    /// Grid size in ticks (> 0).
    pub grid: Tick,
    /// Strength 0–100 percent.
    pub strength: u8,
}

impl QuantizeSpec {
    /// Applies the quantize to a tick.
    pub fn apply(self, t: Tick) -> Tick {
        if self.grid.0 <= 0 {
            return t;
        }
        let g = self.grid.0;
        let snapped = (t.0.saturating_add(g / 2)).div_euclid(g).saturating_mul(g);
        let delta = i128::from(snapped - t.0) * i128::from(self.strength.min(100)) / 100;
        Tick(t.0.saturating_add(i64::try_from(delta).unwrap_or(0)))
    }
}
```

Replace with:

```rust
    }
}

/// The most offsets a play-quantize groove may have.
pub const MAX_GROOVE: usize = 64;

/// Non-destructive quantize: snap note starts toward a grid, or a groove
/// (a grid whose lines are moved by a repeating list of offsets).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizeSpec {
    /// Grid size in ticks (> 0).
    pub grid: Tick,
    /// Strength 0–100 percent.
    pub strength: u8,
    /// Groove: grid line `k` (counted from tick 0) moves by
    /// `groove[k % len]` before the strength applies; each offset is
    /// shorter than the grid. Empty: a plain grid (and absent in files,
    /// so projects without a groove read and write as before).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groove: Vec<Tick>,
}

impl QuantizeSpec {
    /// A plain grid.
    pub fn grid(grid: Tick, strength: u8) -> QuantizeSpec {
        QuantizeSpec {
            grid,
            strength,
            groove: Vec::new(),
        }
    }

    /// Swing: every second line of `grid` moves late so that the pair
    /// splits `percent : 100 − percent` (50 is straight, 66 a triplet
    /// feel); `percent` is clamped to 50–75; full strength.
    pub fn swing(grid: Tick, percent: u8) -> QuantizeSpec {
        let p = i64::from(percent.clamp(50, 75));
        let late = grid.0.saturating_mul(2).saturating_mul(p - 50) / 100;
        QuantizeSpec {
            grid,
            strength: 100,
            groove: vec![Tick::ZERO, Tick(late)],
        }
    }

    /// Checks the grid (> 0, within ±2^40 ticks), the strength (≤ 100)
    /// and the groove (at most [`MAX_GROOVE`] offsets, each shorter than
    /// the grid).
    pub fn check(&self) -> crate::error::Result<()> {
        crate::rules::check_positive("play_quantize.grid", self.grid)?;
        crate::rules::check_ticks("play_quantize.grid", self.grid.0)?;
        if self.strength > 100 {
            return Err(ModelError::range("play_quantize.strength", self.strength));
        }
        if self.groove.len() > MAX_GROOVE {
            return Err(ModelError::range("play_quantize.groove", self.groove.len()));
        }
        if let Some(o) = self
            .groove
            .iter()
            .find(|o| o.0.unsigned_abs() >= self.grid.0.unsigned_abs())
        {
            return Err(ModelError::range("play_quantize.groove", o.0));
        }
        Ok(())
    }

    /// Applies the quantize to a tick.
    pub fn apply(&self, t: Tick) -> Tick {
        if self.grid.0 <= 0 {
            return t;
        }
        let g = self.grid.0;
        let k = t.0.saturating_add(g / 2).div_euclid(g);
        let mut target = k.saturating_mul(g);
        if !self.groove.is_empty() {
            let len = i64::try_from(self.groove.len()).unwrap_or(1);
            let i = usize::try_from(k.rem_euclid(len)).unwrap_or(0);
            target = target.saturating_add(self.groove.get(i).map_or(0, |o| o.0));
        }
        let delta =
            (i128::from(target) - i128::from(t.0)) * i128::from(self.strength.min(100)) / 100;
        Tick(t.0.saturating_add(i64::try_from(delta).unwrap_or(0)))
    }
}
```

In `crates/core-model/src/command/lane.rs`:

Find:

```rust
            quantize,
        } => {
            if let Some(q) = quantize {
                check_positive("play_quantize.grid", q.grid)?;
                if q.strength > 100 {
                    return Err(ModelError::range("play_quantize.strength", q.strength));
                }
            }
            d.lane_mut(*seq, *lane)?.play_quantize = *quantize;
        }
        LaneCmd::SetPlayShift { seq, lane, shift } => {
            d.lane_mut(*seq, *lane)?.play_shift = *shift;
```

Replace with:

```rust
            quantize,
        } => {
            if let Some(q) = quantize {
                q.check()?;
            }
            d.lane_mut(*seq, *lane)?.play_quantize = quantize.clone();
        }
        LaneCmd::SetPlayShift { seq, lane, shift } => {
            d.lane_mut(*seq, *lane)?.play_shift = *shift;
```

In `crates/core-model/src/rules.rs`:

Find:

```rust
                push(check_positive("lane.length", len), &lctx);
                push(check_ticks("lane.length", len.0), &lctx);
            }
            if let Some(q) = l.play_quantize {
                push(
                    need(q.grid.0 > 0 && q.strength <= 100, "play_quantize", q.grid.0),
                    &lctx,
                );
            }
            if l.take(l.active_take).is_none() {
                push(
```

Replace with:

```rust
                push(check_positive("lane.length", len), &lctx);
                push(check_ticks("lane.length", len.0), &lctx);
            }
            if let Some(q) = &l.play_quantize {
                push(q.check(), &lctx);
            }
            if l.take(l.active_take).is_none() {
                push(
```

In `crates/core-model/src/command/reference.rs`:

Find:

```rust
            nl.audio_out = l.audio_out;
            nl.mute = l.mute;
            nl.solo = l.solo;
            nl.play_quantize = l.play_quantize;
            nl.play_shift = l.play_shift;
            nl.drum = l.drum.as_ref().map(|dl| DrumLayout {
                rows: dl
```

Replace with:

```rust
            nl.audio_out = l.audio_out;
            nl.mute = l.mute;
            nl.solo = l.solo;
            nl.play_quantize = l.play_quantize.clone();
            nl.play_shift = l.play_shift;
            nl.drum = l.drum.as_ref().map(|dl| DrumLayout {
                rows: dl
```

In `crates/core-model/src/query/flatten.rs`:

Find:

```rust
                    continue;
                }
                let mut lt = e.tick + base;
                if let (Some(q), EventBody::Note { .. }) = (lane.play_quantize, &e.body) {
                    lt = q.apply(lt);
                }
                lt = lt + lane.play_shift;
```

Replace with:

```rust
                    continue;
                }
                let mut lt = e.tick + base;
                if let (Some(q), EventBody::Note { .. }) = (&lane.play_quantize, &e.body) {
                    lt = q.apply(lt);
                }
                lt = lt + lane.play_shift;
```

In `crates/core-ffi/src/edit.rs`:

Find:

```rust
        } => LaneCmd::SetPlayQuantize {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            quantize: quantize.map(|q| QuantizeSpec {
                grid: Tick(q.grid),
                strength: q.strength,
            }),
        },
        LaneEdit::SetPlayShift { seq, lane, shift } => LaneCmd::SetPlayShift {
            seq: SeqId(*seq),
```

Replace with:

```rust
        } => LaneCmd::SetPlayQuantize {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            quantize: quantize.map(|q| QuantizeSpec::grid(Tick(q.grid), q.strength)),
        },
        LaneEdit::SetPlayShift { seq, lane, shift } => LaneCmd::SetPlayShift {
            seq: SeqId(*seq),
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test play_groove --test model_types --test cmd_lane --test cmd_reference --test file 2>&1 | grep -E "^test result|FAILED|panicked"
cargo test -p core-engine --test drum_lanes 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: all pass (`play_groove`: 4; `drum_lanes`: 3; `file`'s snapshots unchanged). The swung offbeat plays at frame 15 839: tick 7 680 + 2 457 at 120 BPM (24 000 frames per quarter), rounded down.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add crates/core-model/src/lane.rs crates/core-model/src/command/lane.rs crates/core-model/src/command/reference.rs \
  crates/core-model/src/query/flatten.rs crates/core-model/src/rules.rs crates/core-ffi/src/edit.rs \
  crates/core-model/tests/play_groove.rs crates/core-model/tests/model_types.rs crates/core-model/tests/cmd_lane.rs \
  crates/core-model/tests/cmd_reference.rs crates/core-engine/tests/drum_lanes.rs
git commit -m "feat(model): play quantize grooves (swing); tests pin clean drum-lane starts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 838 Rust tests listed (831 + 7); Swift 581 (the FFI surface is unchanged).

---

### Task 3: `core-editor` — the drum grid's layout, spots, cells, strokes and row drops

**Files:**
- Create: `crates/core-editor/Cargo.toml`, `crates/core-editor/src/lib.rs`, `crates/core-editor/src/drum.rs`
- Test: `crates/core-editor/tests/drum.rs`
- Modify: `Cargo.lock` (cargo adds the crate)

**Interfaces:**
- Consumes: `core_model::command::drum::MAX_PAINT_HITS` (Task 1), `core_model::lane::QuantizeSpec::{grid, swing, check}` (Task 2), `core_model::rules::MAX_TICKS`, `core_model::time::{PPQ, Tick}`.
- Produces (`core_editor::drum`): `View {Matrix, Timeline}`, `Tool {Paint, Erase}`, `Settings { view, step, spacing, duration, velocity: u8, row_height: f64, velocity_height: f64 }` with `Default`, `sane()`, `stride()`; `LaneShape { rows: Vec<bool> }`; `BandKind {Header{lane}, Row{lane,row}, NewLane}`; `Band { kind, top, height, cells }` with `shows_velocity()`; `Layout { bands, height }`; `layout(&[LaneShape], &Settings) -> Layout`; `Spot {Header{lane}, Row{lane,row,velocity: Option<u8>}, NewLane, Nothing}`; `band_at`, `spot(&Layout, y) -> Spot`, `velocity_at(&Band, y) -> u8`, `row_band`, `row_velocity(&Layout, lane, row, y) -> Option<u8>`; `Hit { id, tick, duration, velocity: u16 }`; `cell_start(tick, step)`, `Cell { tick, hits: Vec<u64>, velocity: u16 }`, `cells(&[Hit], from, to, step) -> Vec<Cell>` (≤ `MAX_CELLS` 4 096), `hits_at(&[Hit], tick, grab) -> Vec<u64>`; `Stroke { erase, anchor, start, to, hits: Vec<(i64, u16)>, erased: Vec<u64> }`, `begin(&[Hit], at, snapped, grab, velocity: Option<u8>, Tool, &Settings) -> Stroke`, `extend(&Stroke, &[Hit], at, grab, velocity, &Settings) -> Stroke`, `velocity16(u8) -> u16`, `paint_window(&Settings) -> i64`; `Drop {Reorder{to}, ToLane{lane}, NewLane, Nothing}`, `drop_row(&Layout, lane, row, y) -> Drop`; `nudged(shift, steps) -> i64` (`NUDGE_STEP` 120, `MAX_NUDGE` 3 840); `quantize_choices() -> Vec<(String, Option<QuantizeSpec>)>`, `quantize_title(Option<&QuantizeSpec>) -> String`; `row_name(note) -> String`, `next_row(&[u8]) -> Option<(u8, String)>`; constants `HEADER_HEIGHT` 22, `NEW_LANE_HEIGHT` 28.

Rulings 1, 4–8, 10, 13. Pure functions of plain data: no document, no FFI, no drawing. Every input may be hostile (it comes from Swift through the FFI): non-finite heights read as "nothing" or the defaults, ticks are clamped to ±2^40, steps to 1–2^40, strokes stop at 4 096 hits. Band and row indexes are `usize`; the FFI turns them into `u32`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-editor/tests/drum.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-editor/tests
cat > crates/core-editor/tests/drum.rs <<'VIZDAW_EOF'
//! The drum grid's logic (Plan 4e Task 3).

use core_editor::drum::{
    Band, BandKind, Drop, HEADER_HEIGHT, Hit, LaneShape, Layout, MAX_CELLS, MAX_NUDGE,
    NEW_LANE_HEIGHT, Settings, Spot, Tool, View, begin, cell_start, cells, drop_row, extend,
    hits_at, layout, next_row, nudged, paint_window, quantize_choices, quantize_title, row_name,
    row_velocity, spot, velocity_at, velocity16,
};
use core_model::command::drum::MAX_PAINT_HITS;
use core_model::lane::QuantizeSpec;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

const S16: i64 = PPQ / 4;

fn matrix() -> Settings {
    Settings::default()
}

fn timeline() -> Settings {
    Settings {
        view: View::Timeline,
        ..Settings::default()
    }
}

/// Two lanes: lane 0 with rows 0 and 1 (row 1 shows velocity), lane 1
/// with one row.
fn two_lanes() -> Layout {
    layout(
        &[
            LaneShape {
                rows: vec![false, true],
            },
            LaneShape { rows: vec![false] },
        ],
        &matrix(),
    )
}

fn hit(id: u64, tick: i64) -> Hit {
    Hit {
        id,
        tick,
        duration: S16 / 2,
        velocity: 100 << 9,
    }
}

#[test]
fn lanes_stack_headers_rows_and_the_new_lane_band() {
    let l = two_lanes();
    let kinds: Vec<(BandKind, f64, f64, f64)> = l
        .bands
        .iter()
        .map(|b| (b.kind, b.top, b.height, b.cells))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (BandKind::Header { lane: 0 }, 0.0, 22.0, 22.0),
            (BandKind::Row { lane: 0, row: 0 }, 22.0, 20.0, 20.0),
            (BandKind::Row { lane: 0, row: 1 }, 42.0, 48.0, 20.0),
            (BandKind::Header { lane: 1 }, 90.0, 22.0, 22.0),
            (BandKind::Row { lane: 1, row: 0 }, 112.0, 20.0, 20.0),
            (BandKind::NewLane, 132.0, NEW_LANE_HEIGHT, NEW_LANE_HEIGHT),
        ]
    );
    assert_eq!(l.height, 160.0);
    assert_eq!(HEADER_HEIGHT, 22.0);
}

#[test]
fn a_height_names_its_band_and_a_shown_velocity_lane_its_velocity() {
    let l = two_lanes();
    assert_eq!(spot(&l, 5.0), Spot::Header { lane: 0 });
    assert_eq!(
        spot(&l, 30.0),
        Spot::Row {
            lane: 0,
            row: 0,
            velocity: None
        }
    );
    // Row 1 spans 42–90: its top is 127, its bottom 1.
    assert_eq!(
        spot(&l, 42.0),
        Spot::Row {
            lane: 0,
            row: 1,
            velocity: Some(127)
        }
    );
    assert_eq!(
        spot(&l, 89.99),
        Spot::Row {
            lane: 0,
            row: 1,
            velocity: Some(1)
        }
    );
    assert_eq!(
        spot(&l, 66.0),
        Spot::Row {
            lane: 0,
            row: 1,
            velocity: Some(64)
        }
    );
    // A stroke on row 1 dragged above or below it is clamped to the row;
    // row 0 shows no velocity lane.
    assert_eq!(row_velocity(&l, 0, 1, 0.0), Some(127));
    assert_eq!(row_velocity(&l, 0, 1, 500.0), Some(1));
    assert_eq!(row_velocity(&l, 0, 0, 30.0), None);
    assert_eq!(row_velocity(&l, 3, 0, 30.0), None);
    assert_eq!(spot(&l, 140.0), Spot::NewLane);
    assert_eq!(
        spot(&l, 10_000.0),
        Spot::NewLane,
        "anywhere below the last lane"
    );
    assert_eq!(spot(&l, -1.0), Spot::Nothing);
    assert_eq!(spot(&l, f64::NAN), Spot::Nothing);
    assert_eq!(spot(&Layout::default(), 3.0), Spot::Nothing);
    let b = Band {
        kind: BandKind::Row { lane: 0, row: 0 },
        top: 0.0,
        height: 0.0,
        cells: 0.0,
    };
    assert_eq!(
        velocity_at(&b, 0.0),
        64,
        "a zero-height band has a middle velocity"
    );
}

#[test]
fn settings_are_clamped_and_matrix_strides_are_whole_cells() {
    let wild = Settings {
        view: View::Matrix,
        step: 0,
        spacing: -4,
        duration: i64::MIN,
        velocity: 0,
        row_height: f64::NAN,
        velocity_height: 1e9,
    };
    let s = wild.sane();
    assert_eq!(
        (s.step, s.spacing, s.duration, s.velocity),
        (S16, S16, PPQ / 8, 1)
    );
    assert_eq!((s.row_height, s.velocity_height), (20.0, 96.0));
    assert_eq!(
        Settings {
            step: i64::MAX,
            ..matrix()
        }
        .sane()
        .step,
        4 * PPQ
    );
    // An eighth's spacing over sixteenth cells is two cells; a dotted
    // sixteenth rounds to two; a 32nd rounds up to one.
    for (spacing, stride) in [(PPQ / 2, PPQ / 2), (S16 * 3 / 2, PPQ / 2), (PPQ / 8, S16)] {
        assert_eq!(
            Settings {
                spacing,
                ..matrix()
            }
            .stride(),
            stride,
            "{spacing}"
        );
    }
    assert_eq!(
        Settings {
            spacing: PPQ / 8,
            ..timeline()
        }
        .stride(),
        PPQ / 8
    );
    assert_eq!(paint_window(&matrix()), S16);
    assert_eq!(paint_window(&timeline()), 1);
}

#[test]
fn cells_hold_the_hits_that_start_in_them() {
    let hits = [
        hit(7, S16 + 10),
        hit(3, 0),
        hit(9, S16 + 20),
        hit(4, 4 * S16),
    ];
    let c = cells(&hits, -10, 3 * S16, S16);
    let summary: Vec<(i64, Vec<u64>)> = c.iter().map(|c| (c.tick, c.hits.clone())).collect();
    assert_eq!(
        summary,
        vec![
            (-S16, vec![]),
            (0, vec![3]),
            (S16, vec![7, 9]),
            (2 * S16, vec![]),
        ]
    );
    assert_eq!(c[1].velocity, 100 << 9);
    assert_eq!(c[0].velocity, 0);
    assert_eq!(cells(&hits, 0, i64::MAX, 1).len(), MAX_CELLS);
    assert_eq!(cell_start(-1, S16), -S16);
    assert_eq!(cell_start(5, 0), 5, "a step below 1 counts as 1");
    assert_eq!(
        cell_start(-1, i64::MAX),
        -(1 << 40),
        "a step above 2^40 counts as 2^40"
    );
    assert_eq!(hits_at(&hits, S16 + 15, 1), vec![7]);
    assert_eq!(hits_at(&hits, S16 + 25, 1), vec![7, 9]);
    assert_eq!(hits_at(&hits, 3 * S16, 1), Vec::<u64>::new());
}

#[test]
fn a_press_on_an_empty_cell_paints_and_a_drag_paints_a_run() {
    let hits = [hit(1, 2 * S16)];
    let s = Settings {
        spacing: PPQ / 2,
        ..matrix()
    };
    let st = begin(&hits, 10, 0, 1, None, Tool::Paint, &s);
    assert!(!st.erase);
    assert_eq!(st.start, 0);
    assert_eq!(st.hits, vec![(0, 100 << 9)]);
    // Dragged to the fifth cell with the velocity lane at 20: every
    // second cell, the first keeping its velocity.
    let st = extend(&st, &hits, 4 * S16 + 5, 1, Some(20), &s);
    assert_eq!(
        st.hits,
        vec![(0, 100 << 9), (2 * S16, 20 << 9), (4 * S16, 20 << 9)]
    );
    // Back left of the start: the run turns around and shrinks.
    let st = extend(&st, &hits, -S16 * 3, 1, Some(90), &s);
    assert_eq!(st.hits, vec![(0, 100 << 9), (-2 * S16, 90 << 9)]);
    assert!(st.erased.is_empty());
    assert_eq!(st.to, -S16 * 3);
}

#[test]
fn a_press_on_a_hit_erases_and_the_erase_tool_always_does() {
    let hits = [hit(1, S16 + 3), hit(2, 2 * S16), hit(3, 3 * S16)];
    let st = begin(&hits, S16 + 100, 0, 1, None, Tool::Paint, &matrix());
    assert!(st.erase);
    assert_eq!(st.erased, vec![1]);
    let st = extend(&st, &hits, 3 * S16 + 1, 1, None, &matrix());
    assert_eq!(st.erased, vec![1, 2, 3]);
    assert!(st.hits.is_empty());
    let st = begin(&hits, 0, 0, 1, None, Tool::Erase, &matrix());
    assert!(st.erase && st.erased.is_empty());
    // The timeline erases what the swept range touches; a press inside a
    // long hit counts.
    let long = [Hit {
        id: 5,
        tick: 0,
        duration: PPQ,
        velocity: 1,
    }];
    let st = begin(&long, PPQ / 2, PPQ / 2, 1, None, Tool::Paint, &timeline());
    assert!(st.erase);
    assert_eq!(st.erased, vec![5]);
    // The timeline paints from the snapped press every spacing.
    let st = begin(&[], 1_000, 0, 1, Some(127), Tool::Paint, &timeline());
    let st = extend(&st, &[], 2 * S16 + 7, 1, Some(1), &timeline());
    assert_eq!(
        st.hits,
        vec![(0, 127 << 9), (S16, 1 << 9), (2 * S16, 1 << 9)]
    );
}

#[test]
fn strokes_stop_at_the_paint_limit_and_the_tick_limit() {
    let s = Settings {
        step: PPQ / 32,
        spacing: 1,
        view: View::Timeline,
        ..matrix()
    };
    let st = begin(&[], 0, 0, 1, None, Tool::Paint, &s);
    let st = extend(&st, &[], i64::MAX, 1, None, &s);
    assert_eq!(st.hits.len(), MAX_PAINT_HITS);
    let st = begin(&[], i64::MIN, i64::MIN, 1, None, Tool::Paint, &s);
    assert_eq!(st.start, -(1 << 40));
    assert_eq!(velocity16(0), 1 << 9);
    assert_eq!(velocity16(200), 127 << 9);
}

#[test]
fn a_dragged_row_lands_by_height() {
    let l = two_lanes();
    assert_eq!(drop_row(&l, 0, 0, 60.0), Drop::Reorder { to: 1 });
    assert_eq!(drop_row(&l, 0, 1, 5.0), Drop::Reorder { to: 0 });
    assert_eq!(drop_row(&l, 0, 0, 5.0), Drop::Nothing);
    assert_eq!(drop_row(&l, 0, 0, 30.0), Drop::Nothing);
    assert_eq!(drop_row(&l, 0, 1, 115.0), Drop::ToLane { lane: 1 });
    assert_eq!(drop_row(&l, 1, 0, 95.0), Drop::Nothing);
    assert_eq!(drop_row(&l, 1, 0, 30.0), Drop::ToLane { lane: 0 });
    assert_eq!(drop_row(&l, 1, 0, 500.0), Drop::NewLane);
    assert_eq!(drop_row(&l, 1, 0, f64::INFINITY), Drop::Nothing);
}

#[test]
fn nudge_quantize_menu_and_new_rows() {
    assert_eq!(nudged(0, 2), 240);
    assert_eq!(nudged(0, i64::MAX), MAX_NUDGE);
    assert_eq!(nudged(i64::MIN, -1), -MAX_NUDGE);
    let choices = quantize_choices();
    assert_eq!(choices.len(), 15);
    assert_eq!(choices[0], ("Off".to_string(), None));
    assert_eq!(quantize_title(None), "Off");
    let swing = QuantizeSpec::swing(Tick(PPQ / 4), 58);
    assert_eq!(quantize_title(Some(&swing)), "1/16 Swing 58%");
    assert_eq!(
        quantize_title(Some(&QuantizeSpec::grid(Tick(PPQ / 4), 50))),
        "Custom"
    );
    assert!(
        choices
            .iter()
            .all(|(_, q)| q.as_ref().is_none_or(|q| q.check().is_ok()))
    );
    assert_eq!(next_row(&[]), Some((36, "Kick".to_string())));
    assert_eq!(next_row(&[36, 38]), Some((42, "Closed Hat".to_string())));
    let all: Vec<u8> = (0..=127).collect();
    assert_eq!(next_row(&all), None);
    let gm: Vec<u8> = (35..=81).collect();
    assert_eq!(next_row(&gm), Some((0, "C-1".to_string())));
    assert_eq!(row_name(81), "Open Triangle");
    assert_eq!(row_name(60), "Hi Bongo");
    assert_eq!(row_name(84), "C6");
}

fn arb_settings() -> impl Strategy<Value = Settings> {
    (
        any::<bool>(),
        prop_oneof![any::<i64>(), 1i64..PPQ],
        prop_oneof![any::<i64>(), 1i64..PPQ],
        any::<i64>(),
        any::<u8>(),
        prop_oneof![any::<f64>(), 0.0f64..100.0],
        prop_oneof![any::<f64>(), 0.0f64..100.0],
    )
        .prop_map(|(t, step, spacing, duration, velocity, rh, vh)| Settings {
            view: if t { View::Timeline } else { View::Matrix },
            step,
            spacing,
            duration,
            velocity,
            row_height: rh,
            velocity_height: vh,
        })
}

fn arb_hits() -> impl Strategy<Value = Vec<Hit>> {
    prop::collection::vec(
        (
            0u64..20,
            prop_oneof![any::<i64>(), -PPQ..(8 * PPQ)],
            prop_oneof![any::<i64>(), 0i64..PPQ],
            any::<u16>(),
        )
            .prop_map(|(id, tick, duration, velocity)| Hit {
                id,
                tick,
                duration,
                velocity,
            }),
        0..12,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any shapes and settings: bands tile the height without gaps, and
    /// every band's middle is that band's spot.
    #[test]
    fn layouts_tile_and_spots_agree(
        rows in prop::collection::vec(prop::collection::vec(any::<bool>(), 0..5), 0..4),
        s in arb_settings(),
    ) {
        let lanes: Vec<LaneShape> = rows.into_iter().map(|rows| LaneShape { rows }).collect();
        let l = layout(&lanes, &s);
        let mut y = 0.0;
        for b in &l.bands {
            prop_assert_eq!(b.top, y);
            prop_assert!(b.height > 0.0 && b.cells > 0.0 && b.cells <= b.height);
            y += b.height;
            let mid = spot(&l, b.top + b.height / 2.0);
            let ok = match (b.kind, mid) {
                (BandKind::Header { lane }, Spot::Header { lane: l2 }) => lane == l2,
                (BandKind::Row { lane, row }, Spot::Row { lane: l2, row: r2, velocity }) => {
                    lane == l2 && row == r2 && velocity.is_some() == b.shows_velocity()
                }
                (BandKind::NewLane, Spot::NewLane) => true,
                _ => false,
            };
            prop_assert!(ok, "{:?} vs {:?}", b, mid);
        }
        prop_assert_eq!(l.height, y);
    }

    /// Any hits, window and step: cells are consecutive and at most
    /// MAX_CELLS; within ±2^40 ticks every hit starting inside the cells
    /// shows in exactly one.
    #[test]
    fn cells_partition_the_hits(
        hits in arb_hits(),
        from in prop_oneof![any::<i64>(), -(1i64 << 40)..(1i64 << 40)],
        len in 0i64..(64 * PPQ),
        step in prop_oneof![any::<i64>(), 1i64..PPQ],
    ) {
        let to = from.saturating_add(len);
        let c = cells(&hits, from, to, step);
        prop_assert!(c.len() <= MAX_CELLS);
        for w in c.windows(2) {
            prop_assert!(w[1].tick > w[0].tick);
        }
        let sane = |t: i64| t.unsigned_abs() <= 1 << 40;
        if let (Some(first), Some(last)) = (c.first(), c.last()) {
            let end = last.tick.saturating_add(step.clamp(1, 1 << 40));
            for h in hits.iter().filter(|h| sane(h.tick) && sane(from)) {
                let n = c.iter().filter(|x| x.hits.contains(&h.id) && cell_start(h.tick, step) == x.tick).count();
                if h.tick >= first.tick && h.tick < end {
                    prop_assert_eq!(n, 1, "{:?}", h);
                }
            }
        }
    }

    /// Any press and drags: a stroke holds at most MAX_PAINT_HITS distinct
    /// hits within ±2^40 ticks, on its stride from its start; it erases
    /// only hits of the row, and paints nothing while erasing.
    #[test]
    fn strokes_stay_bounded(
        hits in arb_hits(),
        at in any::<i64>(),
        snapped in any::<i64>(),
        moves in prop::collection::vec((any::<i64>(), prop::option::of(any::<u8>())), 0..4),
        erase in any::<bool>(),
        grab in any::<i64>(),
        s in arb_settings(),
    ) {
        let tool = if erase { Tool::Erase } else { Tool::Paint };
        let mut st = begin(&hits, at, snapped, grab, None, tool, &s);
        for (to, v) in moves {
            st = extend(&st, &hits, to, grab, v, &s);
        }
        prop_assert!(st.hits.len() <= MAX_PAINT_HITS);
        let stride = s.stride();
        let mut seen = std::collections::BTreeSet::new();
        for (t, v) in &st.hits {
            prop_assert!(t.unsigned_abs() <= 1 << 40);
            prop_assert!(seen.insert(*t), "distinct");
            prop_assert!((i128::from(*t) - i128::from(st.start)) % i128::from(stride) == 0);
            prop_assert!(*v >= 1 << 9);
        }
        if st.erase {
            prop_assert!(st.hits.is_empty());
        }
        prop_assert!(st.erased.iter().all(|id| hits.iter().any(|h| h.id == *id)));
    }

    /// Dropping a row anywhere never panics, and a drop into its own lane
    /// names one of that lane's indexes.
    #[test]
    fn drops_never_panic(rows in prop::collection::vec(1usize..5, 1..4), lane in 0usize..4, row in 0usize..5, y in any::<f64>()) {
        let lanes: Vec<LaneShape> = rows.iter().map(|n| LaneShape { rows: vec![false; *n] }).collect();
        let l = layout(&lanes, &Settings::default());
        if let Drop::Reorder { to } = drop_row(&l, lane, row, y) {
            prop_assert!(to < rows.get(lane).copied().unwrap_or(0));
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-editor 2>&1 | tail -2
```

Expected: FAIL: `error: package ID specification core-editor did not match any packages`.

- [ ] **Step 3: Implement the crate**

Create `crates/core-editor/Cargo.toml` (the workspace's `members = ["crates/*"]` picks it up):

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-editor
cat > crates/core-editor/Cargo.toml <<'VIZDAW_EOF'
[package]
name = "core-editor"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
core-model = { path = "../core-model" }

[dev-dependencies]
proptest = "1"
VIZDAW_EOF
```

Create `crates/core-editor/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-editor/src
cat > crates/core-editor/src/lib.rs <<'VIZDAW_EOF'
//! `core-editor`: editor logic that is neither document state nor
//! drawing — what a point is over, cells, strokes, where a dragged row
//! lands — as pure functions of plain data, so a front end only draws
//! (editors spec §3.5: hit-testing and geometry are pure functions; Plan
//! 4e Ruling 1). Platform-neutral; it depends only on `core-model`.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod drum;
VIZDAW_EOF
```

Create `crates/core-editor/src/drum.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-editor/src
cat > crates/core-editor/src/drum.rs <<'VIZDAW_EOF'
//! The drum grid (editors spec §4e, parent spec §9 "Drum grid"): drum
//! lanes stacked as bands (a header, then one band per row, a row with
//! its velocity lane shown taller), a step matrix of evenly spaced cells
//! or a timeline of true starts and lengths, a paint tool that toggles a
//! hit on click and paints or erases a run on drag, and rows dragged to a
//! new place, another drum lane or a new lane.

use core_model::command::drum::MAX_PAINT_HITS;
use core_model::lane::QuantizeSpec;
use core_model::rules::MAX_TICKS;
use core_model::time::{PPQ, Tick};

/// Height of a lane's header band (its name, play quantize and nudge).
pub const HEADER_HEIGHT: f64 = 22.0;
/// Height of the band under the last lane that a row is dropped on to
/// split it into a lane of its own.
pub const NEW_LANE_HEIGHT: f64 = 28.0;
/// The most cells [`cells`] returns.
pub const MAX_CELLS: usize = 4096;
/// One nudge step (a 128th of a quarter: about 3.9 ms at 120 BPM).
pub const NUDGE_STEP: i64 = PPQ / 128;
/// A lane's nudge stays within a sixteenth either way.
pub const MAX_NUDGE: i64 = PPQ / 4;

/// How hits are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    /// Evenly spaced cells; a hit fills the cell its start falls in.
    Matrix,
    /// True start times and durations.
    Timeline,
}

/// What the paint tool is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// Toggles: a press on a hit erases, anywhere else paints.
    Paint,
    /// Always erases.
    Erase,
}

/// The grid's settings (saved with the project by the front end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Matrix or timeline.
    pub view: View,
    /// A matrix cell in ticks.
    pub step: i64,
    /// Distance between a stroke's hits (in the matrix, whole cells).
    pub spacing: i64,
    /// Length of a painted hit.
    pub duration: i64,
    /// Velocity (1–127) of a painted hit when its row's velocity lane is
    /// hidden.
    pub velocity: u8,
    /// Height of a row's cells.
    pub row_height: f64,
    /// Height of a shown velocity lane under its row's cells.
    pub velocity_height: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            view: View::Matrix,
            step: PPQ / 4,
            spacing: PPQ / 4,
            duration: PPQ / 8,
            velocity: 100,
            row_height: 20.0,
            velocity_height: 28.0,
        }
    }
}

fn clamp_f(v: f64, lo: f64, hi: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

impl Settings {
    /// Every field within its range: a step of a 128th to a whole note
    /// (else a sixteenth), spacing 1 tick to four bars (else the step),
    /// duration 1 tick to four bars (else a 32nd), velocity 1–127, row
    /// heights 12–64 and 16–96 points (else the defaults).
    pub fn sane(&self) -> Settings {
        let d = Settings::default();
        let step = if self.step > 0 {
            self.step.clamp(PPQ / 32, PPQ * 4)
        } else {
            d.step
        };
        Settings {
            view: self.view,
            step,
            spacing: if self.spacing > 0 {
                self.spacing.min(PPQ * 16)
            } else {
                step
            },
            duration: if self.duration > 0 {
                self.duration.min(PPQ * 16)
            } else {
                d.duration
            },
            velocity: self.velocity.clamp(1, 127),
            row_height: clamp_f(self.row_height, 12.0, 64.0, d.row_height),
            velocity_height: clamp_f(self.velocity_height, 16.0, 96.0, d.velocity_height),
        }
    }

    /// Distance between a stroke's hits: the spacing, in the matrix
    /// rounded to whole cells (at least one).
    pub fn stride(&self) -> i64 {
        let s = self.sane();
        match s.view {
            View::Timeline => s.spacing,
            View::Matrix => {
                let cells = (s.spacing + s.step / 2) / s.step;
                cells.max(1) * s.step
            }
        }
    }
}

/// A drum lane's rows for layout: whether each shows its velocity lane.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaneShape {
    /// One entry per row, in display order.
    pub rows: Vec<bool>,
}

/// What a band is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BandKind {
    /// Lane `lane`'s header.
    Header {
        /// Lane index.
        lane: usize,
    },
    /// Row `row` of lane `lane`.
    Row {
        /// Lane index.
        lane: usize,
        /// Row index in the lane.
        row: usize,
    },
    /// The drop target for a new lane, under the last lane.
    NewLane,
}

/// One horizontal band of the grid, in content coordinates (y down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    /// What it is.
    pub kind: BandKind,
    /// Top edge.
    pub top: f64,
    /// Height.
    pub height: f64,
    /// Height of its cells (less than `height` when a row shows its
    /// velocity lane underneath).
    pub cells: f64,
}

impl Band {
    /// A row with its velocity lane shown.
    pub fn shows_velocity(&self) -> bool {
        matches!(self.kind, BandKind::Row { .. }) && self.height > self.cells
    }
}

/// The grid's bands, top to bottom.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    /// Headers, rows and the new-lane band.
    pub bands: Vec<Band>,
    /// Total height.
    pub height: f64,
}

/// Lanes stacked top to bottom: each a header and its rows, then the
/// new-lane band.
pub fn layout(lanes: &[LaneShape], settings: &Settings) -> Layout {
    let s = settings.sane();
    let mut bands = Vec::new();
    let mut y = 0.0;
    let mut push = |kind, height: f64, cells: f64, y: &mut f64| {
        bands.push(Band {
            kind,
            top: *y,
            height,
            cells,
        });
        *y += height;
    };
    for (lane, shape) in lanes.iter().enumerate() {
        push(
            BandKind::Header { lane },
            HEADER_HEIGHT,
            HEADER_HEIGHT,
            &mut y,
        );
        for (row, &velocity) in shape.rows.iter().enumerate() {
            let h = s.row_height + if velocity { s.velocity_height } else { 0.0 };
            push(BandKind::Row { lane, row }, h, s.row_height, &mut y);
        }
    }
    push(BandKind::NewLane, NEW_LANE_HEIGHT, NEW_LANE_HEIGHT, &mut y);
    Layout { bands, height: y }
}

/// What a height in the grid is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Spot {
    /// A lane's header.
    Header {
        /// Lane index.
        lane: usize,
    },
    /// A row; `velocity` (1–127, from the height) when its velocity lane
    /// is shown.
    Row {
        /// Lane index.
        lane: usize,
        /// Row index.
        row: usize,
        /// The velocity the height means.
        velocity: Option<u8>,
    },
    /// The new-lane band, or anywhere below it.
    NewLane,
    /// Above the grid, or not a number.
    Nothing,
}

/// The band `y` is in.
pub fn band_at(l: &Layout, y: f64) -> Option<&Band> {
    if !y.is_finite() || y < 0.0 {
        return None;
    }
    let i = l.bands.partition_point(|b| b.top + b.height <= y);
    l.bands.get(i).or_else(|| l.bands.last())
}

/// The velocity (1–127) a height in a row means: its bottom edge 1, its
/// top edge 127, linear in between (the whole row, cells and velocity
/// lane, so a stroke can sweep it; Plan 4e Ruling 6).
pub fn velocity_at(band: &Band, y: f64) -> u8 {
    if !y.is_finite() || band.height <= 0.0 || !band.height.is_finite() {
        return 64;
    }
    let f = ((band.top + band.height - y) / band.height).clamp(0.0, 1.0);
    // In 1..=127 by the clamp, so the cast is exact.
    (1.0 + f * 126.0).round().clamp(1.0, 127.0) as u8
}

/// What `y` is over.
pub fn spot(l: &Layout, y: f64) -> Spot {
    let Some(b) = band_at(l, y) else {
        return Spot::Nothing;
    };
    match b.kind {
        BandKind::Header { lane } => Spot::Header { lane },
        BandKind::Row { lane, row } => Spot::Row {
            lane,
            row,
            velocity: b.shows_velocity().then(|| velocity_at(b, y)),
        },
        BandKind::NewLane => Spot::NewLane,
    }
}

/// The band of row `row` of lane `lane`.
pub fn row_band(l: &Layout, lane: usize, row: usize) -> Option<&Band> {
    l.bands
        .iter()
        .find(|b| b.kind == BandKind::Row { lane, row })
}

/// The velocity height `y` means for row `row` of lane `lane` while a
/// stroke on it is dragged anywhere (clamped to the row), when the row
/// shows its velocity lane.
pub fn row_velocity(l: &Layout, lane: usize, row: usize, y: f64) -> Option<u8> {
    row_band(l, lane, row)
        .filter(|b| b.shows_velocity())
        .map(|b| velocity_at(b, y))
}

/// One note of a row, as the grid needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hit {
    /// Event ID.
    pub id: u64,
    /// Start.
    pub tick: i64,
    /// Length.
    pub duration: i64,
    /// 16-bit velocity.
    pub velocity: u16,
}

/// Start of the cell `tick` falls in (cells of `step` ticks from tick 0;
/// the step is taken within 1–2^40).
pub fn cell_start(tick: i64, step: i64) -> i64 {
    let step = step.clamp(1, MAX_TICKS);
    tick.div_euclid(step).saturating_mul(step)
}

/// One matrix cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cell {
    /// Its start.
    pub tick: i64,
    /// The row's hits that start in it, by start.
    pub hits: Vec<u64>,
    /// The loudest of their velocities (0 when empty).
    pub velocity: u16,
}

/// The cells of `step` ticks that overlap `[from, to)`, at most
/// [`MAX_CELLS`], each with the hits that start in it.
pub fn cells(hits: &[Hit], from: i64, to: i64, step: i64) -> Vec<Cell> {
    let step = step.clamp(1, MAX_TICKS);
    let mut sorted: Vec<&Hit> = hits.iter().collect();
    sorted.sort_by_key(|h| (h.tick, h.id));
    let mut out = Vec::new();
    let mut c = cell_start(from, step);
    while c < to && out.len() < MAX_CELLS {
        let end = c.saturating_add(step);
        let lo = sorted.partition_point(|h| h.tick < c);
        let hi = sorted.partition_point(|h| h.tick < end);
        let inside = sorted.get(lo..hi).unwrap_or(&[]);
        out.push(Cell {
            tick: c,
            hits: inside.iter().map(|h| h.id).collect(),
            velocity: inside.iter().map(|h| h.velocity).max().unwrap_or(0),
        });
        if end == c {
            break;
        }
        c = end;
    }
    out
}

/// The hits under `tick` in the timeline: those whose
/// `[start, start + max(duration, grab))` holds it (`grab`: the ticks a
/// hit is wide at least on screen).
pub fn hits_at(hits: &[Hit], tick: i64, grab: i64) -> Vec<u64> {
    hits.iter()
        .filter(|h| h.tick <= tick && tick < h.tick.saturating_add(h.duration.max(grab).max(1)))
        .map(|h| h.id)
        .collect()
}

/// A paint or erase stroke on one row (Plan 4e Rulings 5–7).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stroke {
    /// It erases (the tool, or a paint press on a hit).
    pub erase: bool,
    /// The pointer's tick at the press.
    pub anchor: i64,
    /// The first hit's tick (a cell start in the matrix, the snapped
    /// press in the timeline).
    pub start: i64,
    /// The pointer's tick now.
    pub to: i64,
    /// Painting: `(tick, 16-bit velocity)` of each hit, from `start`
    /// towards the pointer every stride.
    pub hits: Vec<(i64, u16)>,
    /// Erasing: the IDs of the row's hits the run covers.
    pub erased: Vec<u64>,
}

fn clamp_tick(t: i64) -> i64 {
    t.clamp(-MAX_TICKS, MAX_TICKS)
}

/// A 7-bit velocity as the model's 16-bit one.
pub fn velocity16(v: u8) -> u16 {
    u16::from(v.clamp(1, 127)) << 9
}

/// A press at tick `at` on a row with `hits` (`snapped`: `at` on the
/// shared edit grid, used by the timeline; `grab`: see [`hits_at`];
/// `velocity`: the row's velocity lane under the pointer, if shown).
pub fn begin(
    hits: &[Hit],
    at: i64,
    snapped: i64,
    grab: i64,
    velocity: Option<u8>,
    tool: Tool,
    settings: &Settings,
) -> Stroke {
    let s = settings.sane();
    let at = clamp_tick(at);
    let (start, occupied) = match s.view {
        View::Matrix => {
            let c = cell_start(at, s.step);
            (c, hits.iter().any(|h| cell_start(h.tick, s.step) == c))
        }
        View::Timeline => (clamp_tick(snapped), !hits_at(hits, at, grab).is_empty()),
    };
    let first = Stroke {
        erase: tool == Tool::Erase || occupied,
        anchor: at,
        start,
        to: at,
        hits: Vec::new(),
        erased: Vec::new(),
    };
    extend(&first, hits, at, grab, velocity, &s)
}

/// The stroke with the pointer at tick `at`: the run from its start to
/// the pointer (shrinking when the pointer goes back), each painted hit
/// keeping the velocity it got when the run first reached it.
pub fn extend(
    stroke: &Stroke,
    hits: &[Hit],
    at: i64,
    grab: i64,
    velocity: Option<u8>,
    settings: &Settings,
) -> Stroke {
    let s = settings.sane();
    let stride = s.stride();
    let at = clamp_tick(at);
    let end = match s.view {
        View::Matrix => cell_start(at, s.step),
        View::Timeline => at,
    };
    let span = end.saturating_sub(stroke.start);
    let n = usize::try_from((span / stride).unsigned_abs()).unwrap_or(usize::MAX);
    let dir = if span < 0 { -stride } else { stride };
    let positions: Vec<i64> = (0..=n.min(MAX_PAINT_HITS - 1))
        .map(|k| {
            let k = i64::try_from(k).unwrap_or(0);
            stroke.start.saturating_add(dir.saturating_mul(k))
        })
        .filter(|t| (-MAX_TICKS..=MAX_TICKS).contains(t))
        .collect();
    let mut out = Stroke {
        to: at,
        hits: Vec::new(),
        erased: Vec::new(),
        ..stroke.clone()
    };
    if stroke.erase {
        let mut ids: Vec<u64> = match s.view {
            View::Matrix => hits
                .iter()
                .filter(|h| positions.contains(&cell_start(h.tick, s.step)))
                .map(|h| h.id)
                .collect(),
            View::Timeline => {
                let (lo, hi) = (stroke.anchor.min(at), stroke.anchor.max(at));
                hits.iter()
                    .filter(|h| {
                        h.tick <= hi && h.tick.saturating_add(h.duration.max(grab).max(1)) > lo
                    })
                    .map(|h| h.id)
                    .collect()
            }
        };
        ids.sort_unstable();
        ids.dedup();
        out.erased = ids;
    } else {
        let now = velocity16(velocity.unwrap_or(s.velocity));
        out.hits = positions
            .iter()
            .map(|&t| {
                let kept = stroke.hits.iter().find(|(p, _)| *p == t).map(|(_, v)| *v);
                (t, kept.unwrap_or(now))
            })
            .collect();
    }
    out
}

/// The window a painted hit checks for an existing one: a cell in the
/// matrix, one tick (the same start) in the timeline.
pub fn paint_window(settings: &Settings) -> i64 {
    let s = settings.sane();
    match s.view {
        View::Matrix => s.step,
        View::Timeline => 1,
    }
}

/// Where a row dragged from `(lane, row)` lands when released at `y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Drop {
    /// To index `to` in its own lane.
    Reorder {
        /// New index.
        to: usize,
    },
    /// Into another drum lane (appended).
    ToLane {
        /// Lane index.
        lane: usize,
    },
    /// Into a new lane of its own.
    NewLane,
    /// Nowhere (its own place, or above the grid).
    Nothing,
}

/// [`Drop`] for a row of lane `lane` at index `row` released at `y`.
pub fn drop_row(l: &Layout, lane: usize, row: usize, y: f64) -> Drop {
    match spot(l, y) {
        Spot::Row {
            lane: to, row: r, ..
        } if to == lane => {
            if r == row {
                Drop::Nothing
            } else {
                Drop::Reorder { to: r }
            }
        }
        Spot::Header { lane: to } if to == lane => {
            if row == 0 {
                Drop::Nothing
            } else {
                Drop::Reorder { to: 0 }
            }
        }
        Spot::Row { lane: to, .. } | Spot::Header { lane: to } => Drop::ToLane { lane: to },
        Spot::NewLane => Drop::NewLane,
        Spot::Nothing => Drop::Nothing,
    }
}

/// A lane's play shift moved by `steps` nudge steps, within
/// ±[`MAX_NUDGE`].
pub fn nudged(shift: i64, steps: i64) -> i64 {
    shift
        .saturating_add(steps.saturating_mul(NUDGE_STEP))
        .clamp(-MAX_NUDGE, MAX_NUDGE)
}

/// The play quantize menu (Plan 4e Ruling 10): Off, straight grids, and
/// swing on eighths and sixteenths.
pub fn quantize_choices() -> Vec<(String, Option<QuantizeSpec>)> {
    let mut out = vec![("Off".to_string(), None)];
    for (title, grid) in [
        ("1/4", PPQ),
        ("1/8", PPQ / 2),
        ("1/8 Triplet", PPQ / 3),
        ("1/16", PPQ / 4),
        ("1/16 Triplet", PPQ / 6),
        ("1/32", PPQ / 8),
    ] {
        out.push((title.to_string(), Some(QuantizeSpec::grid(Tick(grid), 100))));
    }
    for (name, grid) in [("1/8", PPQ / 2), ("1/16", PPQ / 4)] {
        for p in [54u8, 58, 62, 66] {
            out.push((
                format!("{name} Swing {p}%"),
                Some(QuantizeSpec::swing(Tick(grid), p)),
            ));
        }
    }
    out
}

/// The menu title of a lane's play quantize: its choice's, else
/// "Custom".
pub fn quantize_title(q: Option<&QuantizeSpec>) -> String {
    quantize_choices()
        .into_iter()
        .find(|(_, c)| c.as_ref() == q)
        .map_or_else(|| "Custom".to_string(), |(t, _)| t)
}

/// General MIDI percussion names, notes 35–81.
const GM: [&str; 47] = [
    "Acoustic Bass Drum",
    "Kick",
    "Side Stick",
    "Snare",
    "Hand Clap",
    "Electric Snare",
    "Low Floor Tom",
    "Closed Hat",
    "High Floor Tom",
    "Pedal Hat",
    "Low Tom",
    "Open Hat",
    "Low-Mid Tom",
    "Hi-Mid Tom",
    "Crash",
    "High Tom",
    "Ride",
    "Chinese Cymbal",
    "Ride Bell",
    "Tambourine",
    "Splash",
    "Cowbell",
    "Crash 2",
    "Vibraslap",
    "Ride 2",
    "Hi Bongo",
    "Low Bongo",
    "Mute Hi Conga",
    "Open Hi Conga",
    "Low Conga",
    "High Timbale",
    "Low Timbale",
    "High Agogo",
    "Low Agogo",
    "Cabasa",
    "Maracas",
    "Short Whistle",
    "Long Whistle",
    "Short Guiro",
    "Long Guiro",
    "Claves",
    "Hi Wood Block",
    "Low Wood Block",
    "Mute Cuica",
    "Open Cuica",
    "Mute Triangle",
    "Open Triangle",
];

/// The name a new row for `note` gets: its General MIDI percussion name,
/// else the note's name ("C4" is 60).
pub fn row_name(note: u8) -> String {
    match note.checked_sub(35).and_then(|i| GM.get(usize::from(i))) {
        Some(n) => (*n).to_string(),
        None => {
            const NAMES: [&str; 12] = [
                "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
            ];
            let name = NAMES.get(usize::from(note % 12)).copied().unwrap_or("?");
            format!("{name}{}", i32::from(note / 12) - 1)
        }
    }
}

/// The order Add Row tries notes in: the usual kit first, then the rest
/// of General MIDI percussion, then every other note.
const PREFERRED: [u8; 12] = [36, 38, 42, 46, 39, 49, 51, 45, 47, 50, 37, 44];

/// The note and name of the row Add Row makes in a lane whose rows use
/// `used` notes (rows with their own destination count too); `None` when
/// all 128 are taken.
pub fn next_row(used: &[u8]) -> Option<(u8, String)> {
    PREFERRED
        .iter()
        .copied()
        .chain(35..=81)
        .chain(0..=127)
        .find(|n| !used.contains(n))
        .map(|n| (n, row_name(n)))
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-editor 2>&1 | grep -E "^test result|FAILED|panicked"
```

Expected: `test result: ok. 13 passed` (plus the empty unit and doc-test harnesses). In `a_height_names_its_band…`, row 1 spans 42–90 (20 pt of cells and a 28 pt velocity lane): y 42 is 127, y 66 is 64, y 89.99 is 1.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
git add Cargo.lock crates/core-editor/Cargo.toml crates/core-editor/src/lib.rs crates/core-editor/src/drum.rs crates/core-editor/tests/drum.rs
git commit -m "feat(editor): core-editor with the drum grid's layout, cells, strokes and row drops

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 851 Rust tests listed (838 + 13). If a proptest ever fails, keep the `.proptest-regressions` file it writes out of the commit and fix the code.

---

### Task 4: FFI — drum lanes, the `Drum` edit group, grooves, and the grid's logic as functions

**Files:**
- Create: `crates/core-ffi/src/drum.rs`
- Modify: `crates/core-ffi/Cargo.toml`, `crates/core-ffi/src/edit.rs`, `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/session.rs`, `Cargo.lock`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the new `Session` method)
- Test: `crates/core-ffi/tests/drum.rs`, `crates/core-ffi/tests/edit.rs`

**Interfaces:**
- Consumes: Tasks 1–3; `crate::snapshot::seq_id`, `crate::guard::ffi_guard`, `Session::{edit, begin_edit, edit_gesture, end_edit, undo, lanes}`.
- Produces (`vizdaw_ffi`): `Session::drum_lanes(seq) -> Vec<DrumLaneRow>`; records `DrumHitRow { id, tick, duration, velocity: u16, mute }`, `DrumRowRow { id, name, note, dest: Option<u64>, destination: Option<String>, show_velocity, hits }`, `DrumLaneRow { lane, name, destination, mute, solo, play_quantize: Option<QuantizeRow>, play_shift, rows }`, `DrumPaintHitRow { tick, velocity }`, `DrumGridSettingsRow { view: DrumViewRow, step, spacing, duration, velocity: u8, row_height, velocity_height }`, `DrumBandRow { kind: DrumBandKindRow, top, height, cells }`, `DrumGridLayoutRow { bands, height }`, `DrumCellRow { tick, hits, velocity }`, `DrumStrokeRow { lane: u32, row: u32, erase, anchor, start, to, hits, erased }`, `QuantizeChoiceRow { title, quantize }`, `DrumNewRow { note, name }`; enums `DrumViewRow`, `DrumBandKindRow {Header{lane}, Row{lane,row}, NewLane}`, `DrumSpotRow {Header{lane}, Row{lane,row,velocity}, NewLane, Nothing}`, `DrumEdit {MarkLane, AddRow, RenameRow, ShowVelocity, ReorderRow{to: u32}, RemoveRow, MoveRowToLane, SplitRow, Paint}`; `EditCommand::Drum { edit: DrumEdit }` (last); `QuantizeRow.groove: Vec<i64>` and `edit::{quantize_of, quantize_row}`; functions `drum_grid_layout(shapes: Vec<Vec<bool>>, settings)`, `drum_grid_spot(layout, y)`, `drum_row_velocity(layout, lane, row, y)`, `drum_grid_cells(hits, from, to, step)`, `drum_grid_hits_at(hits, tick, grab)`, `drum_stroke_begin(lane, row, hits, at, snapped, grab, velocity, erase_tool, settings)`, `drum_stroke_extend(stroke, hits, at, grab, velocity, settings)`, `drum_stroke_command(stroke, seq, lane, row, settings) -> Option<EditCommand>`, `drum_row_drop(layout, lanes: Vec<u64>, seq, lane_index, row, row_index, y) -> Option<EditCommand>`, `drum_quantize_choices()`, `drum_quantize_title(quantize)`, `drum_nudged(shift, steps)`, `drum_next_row(used: Vec<u8>)` (Swift: `Data`). Swift names are the camel-cased forms (`drumGridLayout(shapes:settings:)`, `session.drumLanes(seq:)`, `.drum(edit: .paint(...))`).
- `FakeSession`: `drumTable: [UInt64: [DrumLaneRow]]`, `drumLanesCalls`, `drumLanes(seq:)` (throws `readError`).

Editors spec §3.2 lists the `Drum` group; 4a never built it. `drum_lanes` groups the active take's notes into rows with `DrumRow::matches` (the core's membership rule) and names each row's destination (its own, else the lane's). A stroke's command is a `DrumEdit::Paint` (window: a cell in the matrix, one tick in the timeline; duration from the settings) or an `EventEdit::Delete`; it is `None` when the stroke covers nothing. A row drop names the lane IDs in layout order so indexes resolve to IDs in Rust.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/drum.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/drum.rs <<'VIZDAW_EOF'
//! The drum grid through the FFI (Plan 4e Task 4): drum lanes, the Drum
//! edit group, strokes and row drops as commands, play quantize grooves.

use core_engine::fixtures::{Builder, Q};
use core_model::document::Document;
use core_model::ids::{LaneId, SeqId};
use proptest::prelude::*;
use std::sync::Arc;
use vizdaw_ffi::drum::{
    drum_grid_cells, drum_grid_hits_at, drum_grid_layout, drum_grid_spot, drum_next_row,
    drum_nudged, drum_quantize_choices, drum_quantize_title, drum_row_drop, drum_row_velocity,
    drum_stroke_begin, drum_stroke_command, drum_stroke_extend,
};
use vizdaw_ffi::{
    DrumBandKindRow, DrumBandRow, DrumEdit, DrumGridLayoutRow, DrumGridSettingsRow, DrumHitRow,
    DrumLaneRow, DrumPaintHitRow, DrumSpotRow, DrumStrokeRow, DrumViewRow, EditCommand, LaneEdit,
    QuantizeRow, Session, VizError,
};

const S16: i64 = Q / 4;

/// "Beat": drum lane "Drums" (Kick 36: hits at 0 and a beat; Snare 38:
/// a hit at a beat), drum lane "Perc" (Clap 39), plain lane "Keys".
fn beat() -> (Document, SeqId, LaneId, LaneId, LaneId) {
    let mut b = Builder::new();
    let kit = b.dest("Kit", "builtin:synth", 9).unwrap();
    let (seq, drums) = b.sequence("Beat", "Drums", Some(kit)).unwrap();
    b.drum_rows(seq, drums, &[("Kick", 36), ("Snare", 38)])
        .unwrap();
    b.notes(
        seq,
        drums,
        &[
            (0, 36, 0x8000, S16),
            (Q, 36, 0x7000, S16),
            (Q, 38, 0x6000, S16),
        ],
    )
    .unwrap();
    let perc = b.lane(seq, "Perc", Some(kit)).unwrap();
    b.drum_rows(seq, perc, &[("Clap", 39)]).unwrap();
    let keys = b.lane(seq, "Keys", Some(kit)).unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q)]).unwrap();
    (b.finish(), seq, drums, perc, keys)
}

fn session() -> (Arc<Session>, u64, u64, u64, u64) {
    let (d, s, drums, perc, keys) = beat();
    (
        Session::from_document(d).unwrap(),
        s.0,
        drums.0,
        perc.0,
        keys.0,
    )
}

fn matrix() -> DrumGridSettingsRow {
    DrumGridSettingsRow {
        view: DrumViewRow::Matrix,
        step: S16,
        spacing: S16,
        duration: S16 / 2,
        velocity: 100,
        row_height: 20.0,
        velocity_height: 28.0,
    }
}

fn shapes(lanes: &[DrumLaneRow]) -> Vec<Vec<bool>> {
    lanes
        .iter()
        .map(|l| l.rows.iter().map(|r| r.show_velocity).collect())
        .collect()
}

fn drum(edit: DrumEdit) -> EditCommand {
    EditCommand::Drum { edit }
}

#[test]
fn drum_lanes_list_only_drum_lanes_with_their_rows_and_hits() {
    let (s, seq, drums, perc, _) = session();
    let lanes = s.drum_lanes(seq).unwrap();
    assert_eq!(
        lanes
            .iter()
            .map(|l| (l.lane, l.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(drums, "Drums"), (perc, "Perc")]
    );
    let d = &lanes[0];
    assert_eq!(d.destination.as_deref(), Some("Kit"));
    assert_eq!(
        (d.play_quantize.clone(), d.play_shift, d.mute, d.solo),
        (None, 0, false, false)
    );
    type Row<'a> = (&'a str, u8, Vec<(i64, u16)>);
    let rows: Vec<Row> = d
        .rows
        .iter()
        .map(|r| {
            (
                r.name.as_str(),
                r.note,
                r.hits.iter().map(|h| (h.tick, h.velocity)).collect(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("Kick", 36, vec![(0, 0x8000), (Q, 0x7000)]),
            ("Snare", 38, vec![(Q, 0x6000)]),
        ]
    );
    assert_eq!(d.rows[0].destination.as_deref(), Some("Kit"));
    assert!(lanes[1].rows[0].hits.is_empty());
    assert!(matches!(s.drum_lanes(999), Err(VizError::Model { .. })));
}

#[test]
fn every_drum_edit_applies_and_undoes() {
    let (s0, seq, drums, perc, keys) = session();
    let lanes = s0.drum_lanes(seq).unwrap();
    let (kick, snare) = (lanes[0].rows[0].id, lanes[0].rows[1].id);
    let all = [
        DrumEdit::MarkLane {
            seq,
            lane: keys,
            drum: true,
        },
        DrumEdit::MarkLane {
            seq,
            lane: perc,
            drum: false,
        },
        DrumEdit::AddRow {
            seq,
            lane: drums,
            name: "Hat".into(),
            note: 42,
            dest: None,
        },
        DrumEdit::RenameRow {
            seq,
            lane: drums,
            row: kick,
            name: "Bass Drum".into(),
        },
        DrumEdit::ShowVelocity {
            seq,
            lane: drums,
            row: snare,
            show: true,
        },
        DrumEdit::ReorderRow {
            seq,
            lane: drums,
            row: snare,
            to: 0,
        },
        DrumEdit::RemoveRow {
            seq,
            lane: drums,
            row: kick,
        },
        DrumEdit::MoveRowToLane {
            seq,
            lane: drums,
            row: snare,
            to_lane: perc,
        },
        DrumEdit::SplitRow {
            seq,
            lane: drums,
            row: kick,
        },
        DrumEdit::Paint {
            seq,
            lane: drums,
            row: snare,
            hits: vec![DrumPaintHitRow {
                tick: 0,
                velocity: 0x5000,
            }],
            window: S16,
            duration: S16,
        },
    ];
    let labels = [
        "Mark as Drum Lane",
        "Mark as Drum Lane",
        "Add Drum Row",
        "Rename Drum Row",
        "Show Drum Row Velocity",
        "Reorder Drum Row",
        "Remove Drum Row",
        "Move Drum Row to Lane",
        "Split Drum Row to Lane",
        "Paint Drum Hits",
    ];
    for (edit, label) in all.into_iter().zip(labels) {
        let (s, ..) = session();
        let before = (s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap());
        let snap = s
            .edit(drum(edit.clone()), None)
            .unwrap_or_else(|e| panic!("{edit:?}: {e}"));
        assert_eq!(snap.undo_label.as_deref(), Some(label), "{edit:?}");
        assert_ne!(
            (s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap()),
            before,
            "{edit:?}"
        );
        s.undo().unwrap();
        assert_eq!(
            (s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap()),
            before,
            "{edit:?}"
        );
    }
}

#[test]
fn a_click_on_an_empty_cell_paints_and_one_on_a_hit_erases() {
    let (s, seq, drums, ..) = session();
    let lanes = s.drum_lanes(seq).unwrap();
    let layout = drum_grid_layout(shapes(&lanes), matrix()).unwrap();
    // Snare row of lane 0: band 2 (header, kick, snare).
    let DrumSpotRow::Row {
        lane,
        row,
        velocity,
    } = drum_grid_spot(layout.clone(), 22.0 + 20.0 + 5.0).unwrap()
    else {
        panic!("not a row")
    };
    assert_eq!((lane, row, velocity), (0, 1, None));
    assert_eq!(
        drum_row_velocity(layout.clone(), 0, 1, 0.0).unwrap(),
        None,
        "no velocity lane shown"
    );
    let snare = &lanes[0].rows[1];
    // A press in the third cell, dragged to the sixth: four hits.
    let st = drum_stroke_begin(
        lane,
        row,
        snare.hits.clone(),
        2 * S16 + 9,
        0,
        1,
        None,
        false,
        matrix(),
    )
    .unwrap();
    let st = drum_stroke_extend(st, snare.hits.clone(), 5 * S16 + 1, 1, None, matrix()).unwrap();
    assert!(!st.erase);
    assert_eq!(
        st.hits.iter().map(|h| h.tick).collect::<Vec<_>>(),
        vec![2 * S16, 3 * S16, 4 * S16, 5 * S16]
    );
    let cmd = drum_stroke_command(st, seq, drums, snare.id, matrix())
        .unwrap()
        .unwrap();
    s.begin_edit("Paint Drum Hits".into()).unwrap();
    s.edit_gesture(cmd, None).unwrap();
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Paint Drum Hits"));
    let snare = s.drum_lanes(seq).unwrap()[0].rows[1].clone();
    // The hit at a beat (cell 4) was there: it stays, the others come in.
    assert_eq!(
        snare
            .hits
            .iter()
            .map(|h| (h.tick, h.duration, h.velocity))
            .collect::<Vec<_>>(),
        vec![
            (2 * S16, S16 / 2, 100 << 9),
            (3 * S16, S16 / 2, 100 << 9),
            (Q, S16, 0x6000),
            (5 * S16, S16 / 2, 100 << 9)
        ]
    );
    let cells = drum_grid_cells(snare.hits.clone(), 0, 6 * S16, S16).unwrap();
    assert_eq!(
        cells.iter().map(|c| !c.hits.is_empty()).collect::<Vec<_>>(),
        vec![false, false, true, true, true, true]
    );
    // A press on the hit at a beat erases it (a click: no drag).
    let st =
        drum_stroke_begin(0, 1, snare.hits.clone(), Q + 7, 0, 1, None, false, matrix()).unwrap();
    assert!(st.erase);
    let cmd = drum_stroke_command(st, seq, drums, snare.id, matrix())
        .unwrap()
        .unwrap();
    s.edit(cmd, None).unwrap();
    assert_eq!(s.drum_lanes(seq).unwrap()[0].rows[1].hits.len(), 3);
    // A stroke that covers nothing makes no command.
    let st = drum_stroke_begin(0, 1, vec![], 0, 0, 1, None, true, matrix()).unwrap();
    assert_eq!(
        drum_stroke_command(st, seq, drums, snare.id, matrix()).unwrap(),
        None
    );
    assert_eq!(
        drum_grid_hits_at(snare.hits, 2 * S16 + 5, 1).unwrap().len(),
        1
    );
}

#[test]
fn row_drops_reorder_move_and_split() {
    let (s, seq, drums, perc, _) = session();
    let lanes = s.drum_lanes(seq).unwrap();
    let ids: Vec<u64> = lanes.iter().map(|l| l.lane).collect();
    let layout = drum_grid_layout(shapes(&lanes), matrix()).unwrap();
    let kick = lanes[0].rows[0].id;
    // Bands: header 0–22, kick 22–42, snare 42–62, header 62–84, clap
    // 84–104, new lane 104–132.
    let at = |y: f64| drum_row_drop(layout.clone(), ids.clone(), seq, 0, kick, 0, y).unwrap();
    assert_eq!(
        at(50.0),
        Some(drum(DrumEdit::ReorderRow {
            seq,
            lane: drums,
            row: kick,
            to: 1
        }))
    );
    assert_eq!(
        at(90.0),
        Some(drum(DrumEdit::MoveRowToLane {
            seq,
            lane: drums,
            row: kick,
            to_lane: perc
        }))
    );
    assert_eq!(
        at(400.0),
        Some(drum(DrumEdit::SplitRow {
            seq,
            lane: drums,
            row: kick
        }))
    );
    assert_eq!(at(30.0), None);
    assert_eq!(
        drum_row_drop(layout.clone(), ids.clone(), seq, 9, kick, 0, 50.0).unwrap(),
        None,
        "an unknown lane index"
    );
    s.edit(at(400.0).unwrap(), None).unwrap();
    let after = s.drum_lanes(seq).unwrap();
    assert_eq!(
        after
            .iter()
            .map(|l| (l.name.as_str(), l.rows.len()))
            .collect::<Vec<_>>(),
        vec![("Drums", 1), ("Kick", 1), ("Perc", 1)]
    );
    assert_eq!(
        after[1].rows[0].hits.len(),
        2,
        "the kick's notes went along"
    );
}

#[test]
fn play_quantize_grooves_and_nudges_round_trip() {
    let (s, seq, drums, ..) = session();
    let choices = drum_quantize_choices().unwrap();
    assert_eq!(choices.len(), 15);
    let swing = choices
        .iter()
        .find(|c| c.title == "1/8 Swing 66%")
        .unwrap()
        .quantize
        .clone();
    assert_eq!(
        swing.as_ref().map(|q| q.groove.clone()),
        Some(vec![0, 2_457])
    );
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetPlayQuantize {
                seq,
                lane: drums,
                quantize: swing.clone(),
            },
        },
        None,
    )
    .unwrap();
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetPlayShift {
                seq,
                lane: drums,
                shift: drum_nudged(0, -3).unwrap(),
            },
        },
        None,
    )
    .unwrap();
    let lane = &s.drum_lanes(seq).unwrap()[0];
    assert_eq!(lane.play_quantize, swing);
    assert_eq!(lane.play_shift, -360);
    assert_eq!(
        drum_quantize_title(lane.play_quantize.clone()).unwrap(),
        "1/8 Swing 66%"
    );
    assert_eq!(drum_quantize_title(None).unwrap(), "Off");
    let bad = QuantizeRow {
        grid: Q / 2,
        strength: 100,
        groove: vec![Q],
    };
    assert_eq!(drum_quantize_title(Some(bad.clone())).unwrap(), "Custom");
    assert!(matches!(
        s.edit(
            EditCommand::Lane {
                edit: LaneEdit::SetPlayQuantize {
                    seq,
                    lane: drums,
                    quantize: Some(bad)
                },
            },
            None
        ),
        Err(VizError::Model { .. })
    ));
    assert_eq!(
        drum_next_row(vec![36, 38])
            .unwrap()
            .map(|r| (r.note, r.name)),
        Some((42, "Closed Hat".to_string()))
    );
}

fn arb_layout() -> impl Strategy<Value = DrumGridLayoutRow> {
    (
        prop::collection::vec(
            (
                0u32..4,
                0u32..4,
                any::<u8>(),
                any::<f64>(),
                any::<f64>(),
                any::<f64>(),
            ),
            0..8,
        ),
        any::<f64>(),
    )
        .prop_map(|(bands, height)| DrumGridLayoutRow {
            bands: bands
                .into_iter()
                .map(|(lane, row, k, top, h, c)| DrumBandRow {
                    kind: match k % 3 {
                        0 => DrumBandKindRow::Header { lane },
                        1 => DrumBandKindRow::Row { lane, row },
                        _ => DrumBandKindRow::NewLane,
                    },
                    top,
                    height: h,
                    cells: c,
                })
                .collect(),
            height,
        })
}

fn arb_settings() -> impl Strategy<Value = DrumGridSettingsRow> {
    (
        any::<bool>(),
        any::<i64>(),
        any::<i64>(),
        any::<i64>(),
        any::<u8>(),
        any::<f64>(),
        any::<f64>(),
    )
        .prop_map(
            |(t, step, spacing, duration, velocity, row_height, velocity_height)| {
                DrumGridSettingsRow {
                    view: if t {
                        DrumViewRow::Timeline
                    } else {
                        DrumViewRow::Matrix
                    },
                    step,
                    spacing,
                    duration,
                    velocity,
                    row_height,
                    velocity_height,
                }
            },
        )
}

fn arb_hits() -> impl Strategy<Value = Vec<DrumHitRow>> {
    prop::collection::vec(
        (
            any::<u64>(),
            any::<i64>(),
            any::<i64>(),
            any::<u16>(),
            any::<bool>(),
        )
            .prop_map(|(id, tick, duration, velocity, mute)| DrumHitRow {
                id,
                tick,
                duration,
                velocity,
                mute,
            }),
        0..8,
    )
}

fn arb_edit() -> impl Strategy<Value = DrumEdit> {
    let id = || prop_oneof![0u64..12, any::<u64>()];
    prop_oneof![
        (id(), id(), any::<bool>()).prop_map(|(seq, lane, drum)| DrumEdit::MarkLane {
            seq,
            lane,
            drum
        }),
        (id(), id(), any::<u8>(), prop::option::of(id())).prop_map(|(seq, lane, note, dest)| {
            DrumEdit::AddRow {
                seq,
                lane,
                name: "x".into(),
                note,
                dest,
            }
        }),
        (id(), id(), id(), any::<u32>()).prop_map(|(seq, lane, row, to)| DrumEdit::ReorderRow {
            seq,
            lane,
            row,
            to
        }),
        (id(), id(), id(), id()).prop_map(|(seq, lane, row, to_lane)| DrumEdit::MoveRowToLane {
            seq,
            lane,
            row,
            to_lane
        }),
        (id(), id(), id()).prop_map(|(seq, lane, row)| DrumEdit::SplitRow { seq, lane, row }),
        (id(), id(), id()).prop_map(|(seq, lane, row)| DrumEdit::RemoveRow { seq, lane, row }),
        (
            id(),
            id(),
            id(),
            prop::collection::vec((any::<i64>(), any::<u16>()), 0..6),
            any::<i64>(),
            any::<i64>()
        )
            .prop_map(|(seq, lane, row, hits, window, duration)| DrumEdit::Paint {
                seq,
                lane,
                row,
                hits: hits
                    .into_iter()
                    .map(|(tick, velocity)| DrumPaintHitRow { tick, velocity })
                    .collect(),
                window,
                duration,
            }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Hostile layouts, settings, hits and strokes: every grid function
    /// answers (no `Internal`, which a caught panic would be).
    #[test]
    fn grid_functions_answer_any_input(
        layout in arb_layout(),
        y in any::<f64>(),
        hits in arb_hits(),
        at in any::<i64>(),
        to in any::<i64>(),
        grab in any::<i64>(),
        v in prop::option::of(any::<u8>()),
        erase in any::<bool>(),
        s in arb_settings(),
        shapes in prop::collection::vec(prop::collection::vec(any::<bool>(), 0..4), 0..4),
    ) {
        prop_assert!(drum_grid_spot(layout.clone(), y).is_ok());
        prop_assert!(drum_row_velocity(layout.clone(), 1, 0, y).is_ok());
        prop_assert!(drum_grid_layout(shapes, s).is_ok());
        prop_assert!(drum_grid_cells(hits.clone(), at, to, grab).is_ok());
        prop_assert!(drum_grid_hits_at(hits.clone(), at, grab).is_ok());
        let st = drum_stroke_begin(0, 0, hits.clone(), at, to, grab, v, erase, s).unwrap();
        let st: DrumStrokeRow = drum_stroke_extend(st, hits.clone(), to, grab, v, s).unwrap();
        prop_assert!(drum_stroke_command(st, 1, 2, 3, s).is_ok());
        prop_assert!(drum_row_drop(layout, vec![1, 2], 1, 0, 3, 0, y).is_ok());
        prop_assert!(drum_nudged(at, to).is_ok());
    }

    /// Hostile drum edits on the fixture: applied, or refused with a model
    /// error that leaves the document as it was; never a panic.
    #[test]
    fn hostile_drum_edits_apply_or_change_nothing(edit in arb_edit()) {
        let (s, seq, ..) = session();
        let before = (s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap());
        match s.edit(drum(edit), None) {
            Ok(_) => {}
            Err(VizError::Model { .. }) => {
                prop_assert_eq!((s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap()), before);
            }
            Err(e) => prop_assert!(false, "unexpected {e:?}"),
        }
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/tests/edit.rs` (its `QuantizeRow` literal gains the field):

Find:

```rust
                quantize: Some(QuantizeRow {
                    grid: Q / 4,
                    strength: 50,
                }),
            },
        },
```

Replace with:

```rust
                quantize: Some(QuantizeRow {
                    grid: Q / 4,
                    strength: 50,
                    groove: vec![],
                }),
            },
        },
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test drum 2>&1 | grep -E "^error" | head -3
```

Expected: FAIL to compile: `unresolved import vizdaw_ffi::drum`.

- [ ] **Step 3: Implement**

In `crates/core-ffi/Cargo.toml`:

Find:

```toml

[dependencies]
core-control = { path = "../core-control" }
core-model = { path = "../core-model" }
core-engine = { path = "../core-engine", features = ["fixtures"] }
core-host = { path = "../core-host" }
```

Replace with:

```toml

[dependencies]
core-control = { path = "../core-control" }
core-editor = { path = "../core-editor" }
core-model = { path = "../core-model" }
core-engine = { path = "../core-engine", features = ["fixtures"] }
core-host = { path = "../core-host" }
```

Create `crates/core-ffi/src/drum.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/src
cat > crates/core-ffi/src/drum.rs <<'VIZDAW_EOF'
//! The drum grid's FFI (editors spec §4e): drum lanes with their rows and
//! hits, the `Drum` edit group, and `core-editor`'s grid logic as pure
//! functions of records, so Swift only draws (Plan 4e Ruling 1).

use crate::edit::{EditCommand, EventEdit, QuantizeRow, quantize_of, quantize_row};
use crate::error::VizError;
use crate::guard::ffi_guard;
use core_editor::drum as grid;
use core_model::command::DrumCmd;
use core_model::command::drum::DrumHit;
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{DestId, DrumRowId, LaneId, SeqId};
use core_model::time::Tick;

/// One note of a drum row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumHitRow {
    /// Event ID.
    pub id: u64,
    /// Start.
    pub tick: i64,
    /// Length.
    pub duration: i64,
    /// 16-bit velocity.
    pub velocity: u16,
    /// Muted.
    pub mute: bool,
}

/// One row of a drum lane.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumRowRow {
    /// Row ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Note 0–127.
    pub note: u8,
    /// The row's destination override.
    pub dest: Option<u64>,
    /// Name of the destination its notes play on (the row's, else the
    /// lane's).
    pub destination: Option<String>,
    /// The grid shows its velocity lane.
    pub show_velocity: bool,
    /// Its notes in the active take, by start.
    pub hits: Vec<DrumHitRow>,
}

/// One drum lane of a sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumLaneRow {
    /// Lane ID.
    pub lane: u64,
    /// Name.
    pub name: String,
    /// Name of the lane's MIDI destination.
    pub destination: Option<String>,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Play quantize.
    pub play_quantize: Option<QuantizeRow>,
    /// Play shift (nudge) in ticks.
    pub play_shift: i64,
    /// Rows in display order.
    pub rows: Vec<DrumRowRow>,
}

/// The drum lanes of sequence `seq`, in lane order.
pub fn drum_lanes(doc: &Document, seq: u64) -> Result<Vec<DrumLaneRow>, VizError> {
    let s = doc.sequence(crate::snapshot::seq_id(doc, seq)?)?;
    let name =
        |d: Option<DestId>| d.and_then(|d| doc.instruments().get(&d).map(|x| x.name.clone()));
    Ok(s.lanes
        .iter()
        .filter_map(|l| {
            let layout = l.drum.as_ref()?;
            let notes: Vec<_> = l
                .active()
                .map(|t| t.events.values().collect())
                .unwrap_or_default();
            let rows = layout
                .rows
                .iter()
                .map(|r| DrumRowRow {
                    id: r.id.0,
                    name: r.name.clone(),
                    note: r.note,
                    dest: r.dest.map(|d| d.0),
                    destination: name(r.dest.or(l.midi_dest)),
                    show_velocity: r.show_velocity,
                    hits: notes
                        .iter()
                        .filter_map(|e| match e.body {
                            EventBody::Note {
                                pitch,
                                velocity,
                                duration,
                                ..
                            } if r.matches(pitch, e.dest) => Some(DrumHitRow {
                                id: e.id.0,
                                tick: e.tick.0,
                                duration: duration.0,
                                velocity,
                                mute: e.mute,
                            }),
                            _ => None,
                        })
                        .collect(),
                })
                .collect();
            Some(DrumLaneRow {
                lane: l.id.0,
                name: l.name.clone(),
                destination: name(l.midi_dest),
                mute: l.mute,
                solo: l.solo,
                play_quantize: l.play_quantize.as_ref().map(quantize_row),
                play_shift: l.play_shift.0,
                rows,
            })
        })
        .collect())
}

/// One hit a paint adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumPaintHitRow {
    /// Start.
    pub tick: i64,
    /// 16-bit velocity (> 0).
    pub velocity: u16,
}

/// Mirrors `core_model::command::DrumCmd`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DrumEdit {
    /// Make a lane a drum lane (rows for its notes) or a normal lane.
    MarkLane {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Drum lane or not.
        drum: bool,
    },
    /// Add a row; `(note, dest)` must be new in the lane.
    AddRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Name.
        name: String,
        /// Note 0–127.
        note: u8,
        /// Destination override.
        dest: Option<u64>,
    },
    /// Rename a row.
    RenameRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
        /// New name.
        name: String,
    },
    /// Show or hide a row's velocity lane.
    ShowVelocity {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
        /// Shown.
        show: bool,
    },
    /// Move a row to index `to` of its lane.
    ReorderRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
        /// New index (clamped).
        to: u32,
    },
    /// Remove a row and its notes.
    RemoveRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
    },
    /// Move a row and its notes to another lane of the sequence.
    MoveRowToLane {
        /// Sequence.
        seq: u64,
        /// Source lane.
        lane: u64,
        /// Row.
        row: u64,
        /// Target lane.
        to_lane: u64,
    },
    /// Move a row to a new lane of its own after its lane.
    SplitRow {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
    },
    /// Paint hits into a row (`DrumCmd::Paint`).
    Paint {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Row.
        row: u64,
        /// The hits.
        hits: Vec<DrumPaintHitRow>,
        /// A hit is skipped when the row has a note starting this close
        /// after it (> 0).
        window: i64,
        /// Length of each new note (> 0).
        duration: i64,
    },
}

/// The core command of a drum edit.
pub fn drum_cmd(e: &DrumEdit) -> DrumCmd {
    let s = |seq: &u64| SeqId(*seq);
    match e {
        DrumEdit::MarkLane { seq, lane, drum } => DrumCmd::MarkDrumLane {
            seq: s(seq),
            lane: LaneId(*lane),
            drum: *drum,
        },
        DrumEdit::AddRow {
            seq,
            lane,
            name,
            note,
            dest,
        } => DrumCmd::AddRow {
            seq: s(seq),
            lane: LaneId(*lane),
            name: name.clone(),
            note: *note,
            dest: dest.map(DestId),
        },
        DrumEdit::RenameRow {
            seq,
            lane,
            row,
            name,
        } => DrumCmd::RenameRow {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            name: name.clone(),
        },
        DrumEdit::ShowVelocity {
            seq,
            lane,
            row,
            show,
        } => DrumCmd::SetRowShowVelocity {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            show: *show,
        },
        DrumEdit::ReorderRow { seq, lane, row, to } => DrumCmd::ReorderRow {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            to: usize::try_from(*to).unwrap_or(usize::MAX),
        },
        DrumEdit::RemoveRow { seq, lane, row } => DrumCmd::RemoveRow {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
        },
        DrumEdit::MoveRowToLane {
            seq,
            lane,
            row,
            to_lane,
        } => DrumCmd::MoveRowToLane {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            to_lane: LaneId(*to_lane),
        },
        DrumEdit::SplitRow { seq, lane, row } => DrumCmd::SplitRowToLane {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
        },
        DrumEdit::Paint {
            seq,
            lane,
            row,
            hits,
            window,
            duration,
        } => DrumCmd::Paint {
            seq: s(seq),
            lane: LaneId(*lane),
            row: DrumRowId(*row),
            hits: hits
                .iter()
                .map(|h| DrumHit {
                    tick: Tick(h.tick),
                    velocity: h.velocity,
                })
                .collect(),
            window: Tick(*window),
            duration: Tick(*duration),
        },
    }
}

/// How the grid shows hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DrumViewRow {
    /// Evenly spaced cells.
    Matrix,
    /// True starts and lengths.
    Timeline,
}

/// The grid's settings (`core_editor::drum::Settings`).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct DrumGridSettingsRow {
    /// Matrix or timeline.
    pub view: DrumViewRow,
    /// A matrix cell in ticks.
    pub step: i64,
    /// Distance between a stroke's hits.
    pub spacing: i64,
    /// Length of a painted hit.
    pub duration: i64,
    /// Velocity (1–127) of a painted hit while its row's velocity lane is
    /// hidden.
    pub velocity: u8,
    /// Height of a row's cells.
    pub row_height: f64,
    /// Height of a shown velocity lane.
    pub velocity_height: f64,
}

fn settings(s: &DrumGridSettingsRow) -> grid::Settings {
    grid::Settings {
        view: match s.view {
            DrumViewRow::Matrix => grid::View::Matrix,
            DrumViewRow::Timeline => grid::View::Timeline,
        },
        step: s.step,
        spacing: s.spacing,
        duration: s.duration,
        velocity: s.velocity,
        row_height: s.row_height,
        velocity_height: s.velocity_height,
    }
}

/// What a band of the grid is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DrumBandKindRow {
    /// A lane's header.
    Header {
        /// Lane index.
        lane: u32,
    },
    /// A row.
    Row {
        /// Lane index.
        lane: u32,
        /// Row index.
        row: u32,
    },
    /// The new-lane band.
    NewLane,
}

/// One band (content coordinates, y down).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct DrumBandRow {
    /// What it is.
    pub kind: DrumBandKindRow,
    /// Top.
    pub top: f64,
    /// Height.
    pub height: f64,
    /// Height of its cells.
    pub cells: f64,
}

/// The grid's bands.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DrumGridLayoutRow {
    /// Bands, top to bottom.
    pub bands: Vec<DrumBandRow>,
    /// Total height.
    pub height: f64,
}

/// What a height is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum DrumSpotRow {
    /// A lane's header.
    Header {
        /// Lane index.
        lane: u32,
    },
    /// A row (with the velocity the height means when its velocity lane
    /// is shown).
    Row {
        /// Lane index.
        lane: u32,
        /// Row index.
        row: u32,
        /// 1–127.
        velocity: Option<u8>,
    },
    /// The new-lane band or below.
    NewLane,
    /// Nothing.
    Nothing,
}

/// One matrix cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumCellRow {
    /// Start.
    pub tick: i64,
    /// The hits starting in it.
    pub hits: Vec<u64>,
    /// The loudest velocity (0 when empty).
    pub velocity: u16,
}

/// A paint or erase stroke (`core_editor::drum::Stroke`) on row `row` of
/// lane `lane` (indexes in the layout).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumStrokeRow {
    /// Lane index.
    pub lane: u32,
    /// Row index.
    pub row: u32,
    /// It erases.
    pub erase: bool,
    /// The pointer's tick at the press.
    pub anchor: i64,
    /// The first hit's tick.
    pub start: i64,
    /// The pointer's tick now.
    pub to: i64,
    /// Painting: the hits.
    pub hits: Vec<DrumPaintHitRow>,
    /// Erasing: the IDs of the hits it covers.
    pub erased: Vec<u64>,
}

/// One entry of the play quantize menu.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct QuantizeChoiceRow {
    /// Menu title.
    pub title: String,
    /// The quantize (`None`: off).
    pub quantize: Option<QuantizeRow>,
}

/// The row Add Row makes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct DrumNewRow {
    /// Note.
    pub note: u8,
    /// Name.
    pub name: String,
}

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

fn hits_of(h: &[DrumHitRow]) -> Vec<grid::Hit> {
    h.iter()
        .map(|h| grid::Hit {
            id: h.id,
            tick: h.tick,
            duration: h.duration,
            velocity: h.velocity,
        })
        .collect()
}

fn layout_of(l: &DrumGridLayoutRow) -> grid::Layout {
    grid::Layout {
        bands: l
            .bands
            .iter()
            .map(|b| grid::Band {
                kind: match b.kind {
                    DrumBandKindRow::Header { lane } => grid::BandKind::Header {
                        lane: usize::try_from(lane).unwrap_or(usize::MAX),
                    },
                    DrumBandKindRow::Row { lane, row } => grid::BandKind::Row {
                        lane: usize::try_from(lane).unwrap_or(usize::MAX),
                        row: usize::try_from(row).unwrap_or(usize::MAX),
                    },
                    DrumBandKindRow::NewLane => grid::BandKind::NewLane,
                },
                top: b.top,
                height: b.height,
                cells: b.cells,
            })
            .collect(),
        height: l.height,
    }
}

fn stroke_of(s: &DrumStrokeRow) -> grid::Stroke {
    grid::Stroke {
        erase: s.erase,
        anchor: s.anchor,
        start: s.start,
        to: s.to,
        hits: s.hits.iter().map(|h| (h.tick, h.velocity)).collect(),
        erased: s.erased.clone(),
    }
}

fn stroke_row(lane: u32, row: u32, s: grid::Stroke) -> DrumStrokeRow {
    DrumStrokeRow {
        lane,
        row,
        erase: s.erase,
        anchor: s.anchor,
        start: s.start,
        to: s.to,
        hits: s
            .hits
            .into_iter()
            .map(|(tick, velocity)| DrumPaintHitRow { tick, velocity })
            .collect(),
        erased: s.erased,
    }
}

/// The bands of drum lanes whose rows show their velocity lanes as in
/// `shapes` (one list per lane, one flag per row).
#[uniffi::export]
pub fn drum_grid_layout(
    shapes: Vec<Vec<bool>>,
    settings: DrumGridSettingsRow,
) -> Result<DrumGridLayoutRow, VizError> {
    ffi_guard(|| {
        let lanes: Vec<grid::LaneShape> = shapes
            .into_iter()
            .map(|rows| grid::LaneShape { rows })
            .collect();
        let l = grid::layout(&lanes, &self::settings(&settings));
        Ok(DrumGridLayoutRow {
            bands: l
                .bands
                .iter()
                .map(|b| DrumBandRow {
                    kind: match b.kind {
                        grid::BandKind::Header { lane } => {
                            DrumBandKindRow::Header { lane: index(lane) }
                        }
                        grid::BandKind::Row { lane, row } => DrumBandKindRow::Row {
                            lane: index(lane),
                            row: index(row),
                        },
                        grid::BandKind::NewLane => DrumBandKindRow::NewLane,
                    },
                    top: b.top,
                    height: b.height,
                    cells: b.cells,
                })
                .collect(),
            height: l.height,
        })
    })
}

/// What height `y` of `layout` is over.
#[uniffi::export]
pub fn drum_grid_spot(layout: DrumGridLayoutRow, y: f64) -> Result<DrumSpotRow, VizError> {
    ffi_guard(|| {
        Ok(match grid::spot(&layout_of(&layout), y) {
            grid::Spot::Header { lane } => DrumSpotRow::Header { lane: index(lane) },
            grid::Spot::Row {
                lane,
                row,
                velocity,
            } => DrumSpotRow::Row {
                lane: index(lane),
                row: index(row),
                velocity,
            },
            grid::Spot::NewLane => DrumSpotRow::NewLane,
            grid::Spot::Nothing => DrumSpotRow::Nothing,
        })
    })
}

/// The velocity height `y` means for row `row` of lane `lane` (layout
/// indexes) while a stroke on it is dragged (clamped to the row); `None`
/// when the row hides its velocity lane.
#[uniffi::export]
pub fn drum_row_velocity(
    layout: DrumGridLayoutRow,
    lane: u32,
    row: u32,
    y: f64,
) -> Result<Option<u8>, VizError> {
    ffi_guard(|| {
        Ok(grid::row_velocity(
            &layout_of(&layout),
            usize::try_from(lane).unwrap_or(usize::MAX),
            usize::try_from(row).unwrap_or(usize::MAX),
            y,
        ))
    })
}

/// The matrix cells of `step` ticks overlapping `[from, to)` with the
/// hits that start in each (at most 4 096).
#[uniffi::export]
pub fn drum_grid_cells(
    hits: Vec<DrumHitRow>,
    from: i64,
    to: i64,
    step: i64,
) -> Result<Vec<DrumCellRow>, VizError> {
    ffi_guard(|| {
        Ok(grid::cells(&hits_of(&hits), from, to, step)
            .into_iter()
            .map(|c| DrumCellRow {
                tick: c.tick,
                hits: c.hits,
                velocity: c.velocity,
            })
            .collect())
    })
}

/// The hits under `tick` in the timeline (`grab`: a hit's least width in
/// ticks).
#[uniffi::export]
pub fn drum_grid_hits_at(
    hits: Vec<DrumHitRow>,
    tick: i64,
    grab: i64,
) -> Result<Vec<u64>, VizError> {
    ffi_guard(|| Ok(grid::hits_at(&hits_of(&hits), tick, grab)))
}

/// A press on row `row` of lane `lane` (layout indexes) whose hits are
/// `hits`, at tick `at` (`snapped`: on the shared edit grid); `velocity`
/// from a shown velocity lane; `erase_tool`: the eraser is chosen.
#[allow(clippy::too_many_arguments)]
#[uniffi::export]
pub fn drum_stroke_begin(
    lane: u32,
    row: u32,
    hits: Vec<DrumHitRow>,
    at: i64,
    snapped: i64,
    grab: i64,
    velocity: Option<u8>,
    erase_tool: bool,
    settings: DrumGridSettingsRow,
) -> Result<DrumStrokeRow, VizError> {
    ffi_guard(|| {
        let tool = if erase_tool {
            grid::Tool::Erase
        } else {
            grid::Tool::Paint
        };
        let s = grid::begin(
            &hits_of(&hits),
            at,
            snapped,
            grab,
            velocity,
            tool,
            &self::settings(&settings),
        );
        Ok(stroke_row(lane, row, s))
    })
}

/// `stroke` with the pointer at tick `at`.
#[uniffi::export]
pub fn drum_stroke_extend(
    stroke: DrumStrokeRow,
    hits: Vec<DrumHitRow>,
    at: i64,
    grab: i64,
    velocity: Option<u8>,
    settings: DrumGridSettingsRow,
) -> Result<DrumStrokeRow, VizError> {
    ffi_guard(|| {
        let s = grid::extend(
            &stroke_of(&stroke),
            &hits_of(&hits),
            at,
            grab,
            velocity,
            &self::settings(&settings),
        );
        Ok(stroke_row(stroke.lane, stroke.row, s))
    })
}

/// The edit a stroke on row `row` of lane `lane` of sequence `seq` makes:
/// a paint, a delete, or nothing when it covers nothing.
#[uniffi::export]
pub fn drum_stroke_command(
    stroke: DrumStrokeRow,
    seq: u64,
    lane: u64,
    row: u64,
    settings: DrumGridSettingsRow,
) -> Result<Option<EditCommand>, VizError> {
    ffi_guard(|| {
        let s = self::settings(&settings).sane();
        Ok(if stroke.erase {
            (!stroke.erased.is_empty()).then(|| EditCommand::Event {
                edit: EventEdit::Delete {
                    events: stroke.erased.clone(),
                },
            })
        } else {
            (!stroke.hits.is_empty()).then(|| EditCommand::Drum {
                edit: DrumEdit::Paint {
                    seq,
                    lane,
                    row,
                    hits: stroke.hits.clone(),
                    window: grid::paint_window(&s),
                    duration: s.duration,
                },
            })
        })
    })
}

/// The edit that drops row `row` (ID) at index `row_index` of lane
/// `lane_index` at height `y`; `lanes` are the shown lanes' IDs in
/// layout order. `None`: it lands where it is, or nowhere.
#[allow(clippy::too_many_arguments)]
#[uniffi::export]
pub fn drum_row_drop(
    layout: DrumGridLayoutRow,
    lanes: Vec<u64>,
    seq: u64,
    lane_index: u32,
    row: u64,
    row_index: u32,
    y: f64,
) -> Result<Option<EditCommand>, VizError> {
    ffi_guard(|| {
        let li = usize::try_from(lane_index).unwrap_or(usize::MAX);
        let Some(&lane) = lanes.get(li) else {
            return Ok(None);
        };
        let ri = usize::try_from(row_index).unwrap_or(usize::MAX);
        let edit = match grid::drop_row(&layout_of(&layout), li, ri, y) {
            grid::Drop::Reorder { to } => DrumEdit::ReorderRow {
                seq,
                lane,
                row,
                to: index(to),
            },
            grid::Drop::ToLane { lane: to } => match lanes.get(to) {
                Some(&to_lane) => DrumEdit::MoveRowToLane {
                    seq,
                    lane,
                    row,
                    to_lane,
                },
                None => return Ok(None),
            },
            grid::Drop::NewLane => DrumEdit::SplitRow { seq, lane, row },
            grid::Drop::Nothing => return Ok(None),
        };
        Ok(Some(EditCommand::Drum { edit }))
    })
}

/// The play quantize menu: Off, grids and swings.
#[uniffi::export]
pub fn drum_quantize_choices() -> Result<Vec<QuantizeChoiceRow>, VizError> {
    ffi_guard(|| {
        Ok(grid::quantize_choices()
            .into_iter()
            .map(|(title, q)| QuantizeChoiceRow {
                title,
                quantize: q.as_ref().map(quantize_row),
            })
            .collect())
    })
}

/// The menu title of a play quantize ("Custom" when it is no choice).
#[uniffi::export]
pub fn drum_quantize_title(quantize: Option<QuantizeRow>) -> Result<String, VizError> {
    ffi_guard(|| {
        Ok(grid::quantize_title(
            quantize.as_ref().map(quantize_of).as_ref(),
        ))
    })
}

/// A play shift moved by `steps` nudge steps (within a sixteenth).
#[uniffi::export]
pub fn drum_nudged(shift: i64, steps: i64) -> Result<i64, VizError> {
    ffi_guard(|| Ok(grid::nudged(shift, steps)))
}

/// The row Add Row makes in a lane whose rows use `used` notes.
#[uniffi::export]
pub fn drum_next_row(used: Vec<u8>) -> Result<Option<DrumNewRow>, VizError> {
    ffi_guard(|| Ok(grid::next_row(&used).map(|(note, name)| DrumNewRow { note, name })))
}
VIZDAW_EOF
```

In `crates/core-ffi/src/edit.rs`:

Find:

```rust
//! Editor commands (spec §3.2): one variant per core command of the
//! groups 4a needs (Event, Reference, Lane, Sequence, Audio); field types
//! are FFI records. The view filter travels beside the command
//! (Plan Ruling 6).

```

Replace with:

```rust
//! Editor commands (spec §3.2): one variant per core command of the
//! groups 4a needs (Event, Reference, Lane, Sequence, Audio) and of the
//! Drum group (4e, `crate::drum`); field types
//! are FFI records. The view filter travels beside the command
//! (Plan Ruling 6).

```

Find:

```rust
}

/// Non-destructive play quantize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct QuantizeRow {
    /// Grid in ticks (> 0).
    pub grid: i64,
    /// Strength 0–100.
    pub strength: u8,
}

/// Generator parameters of a generated sequence.
```

Replace with:

```rust
}

/// Non-destructive play quantize.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct QuantizeRow {
    /// Grid in ticks (> 0).
    pub grid: i64,
    /// Strength 0–100.
    pub strength: u8,
    /// Groove offsets per grid line (empty: a plain grid; Plan 4e
    /// Ruling 10).
    #[uniffi(default)]
    pub groove: Vec<i64>,
}

/// The core quantize of a row.
pub fn quantize_of(q: &QuantizeRow) -> QuantizeSpec {
    QuantizeSpec {
        grid: Tick(q.grid),
        strength: q.strength,
        groove: q.groove.iter().copied().map(Tick).collect(),
    }
}

/// A core quantize as a row.
pub fn quantize_row(q: &QuantizeSpec) -> QuantizeRow {
    QuantizeRow {
        grid: q.grid.0,
        strength: q.strength,
        groove: q.groove.iter().map(|t| t.0).collect(),
    }
}

/// Generator parameters of a generated sequence.
```

Find:

```rust
        /// The edit.
        edit: crate::modify::SettingsEdit,
    },
}

/// Mirrors `core_model::command::EventCmd`.
```

Replace with:

```rust
        /// The edit.
        edit: crate::modify::SettingsEdit,
    },
    /// Drum-lane edits (4e).
    Drum {
        /// The edit.
        edit: crate::drum::DrumEdit,
    },
}

/// Mirrors `core_model::command::EventCmd`.
```

Find:

```rust
            ));
        }
        EditCommand::Settings { edit } => Command::Settings(crate::modify::settings_cmd(edit)?),
    })
}

```

Replace with:

```rust
            ));
        }
        EditCommand::Settings { edit } => Command::Settings(crate::modify::settings_cmd(edit)?),
        EditCommand::Drum { edit } => Command::Drum(crate::drum::drum_cmd(edit)),
    })
}

```

Find:

```rust
        } => LaneCmd::SetPlayQuantize {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            quantize: quantize.map(|q| QuantizeSpec::grid(Tick(q.grid), q.strength)),
        },
        LaneEdit::SetPlayShift { seq, lane, shift } => LaneCmd::SetPlayShift {
            seq: SeqId(*seq),
```

Replace with:

```rust
        } => LaneCmd::SetPlayQuantize {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            quantize: quantize.as_ref().map(quantize_of),
        },
        LaneEdit::SetPlayShift { seq, lane, shift } => LaneCmd::SetPlayShift {
            seq: SeqId(*seq),
```

In `crates/core-ffi/src/lib.rs`:

Find:

```rust
pub mod catalog;
pub mod clock;
pub mod control;
pub mod edit;
pub mod editor;
pub mod error;
```

Replace with:

```rust
pub mod catalog;
pub mod clock;
pub mod control;
pub mod drum;
pub mod edit;
pub mod editor;
pub mod error;
```

Find:

```rust
    WindowKindRow, WindowRefRow, WindowRow, control_command_ids, control_element_check,
    control_host_busy, control_wait_met,
};
pub use edit::{
    AudioEdit, EditCommand, EventEdit, GeneratorRow, LaneEdit, LaneRef, QuantizeRow, ReferenceEdit,
    SequenceEdit, ViewFilterRow,
```

Replace with:

```rust
    WindowKindRow, WindowRefRow, WindowRow, control_command_ids, control_element_check,
    control_host_busy, control_wait_met,
};
pub use drum::{
    DrumBandKindRow, DrumBandRow, DrumCellRow, DrumEdit, DrumGridLayoutRow, DrumGridSettingsRow,
    DrumHitRow, DrumLaneRow, DrumNewRow, DrumPaintHitRow, DrumRowRow, DrumSpotRow, DrumStrokeRow,
    DrumViewRow, QuantizeChoiceRow,
};
pub use edit::{
    AudioEdit, EditCommand, EventEdit, GeneratorRow, LaneEdit, LaneRef, QuantizeRow, ReferenceEdit,
    SequenceEdit, ViewFilterRow,
```

In `crates/core-ffi/src/session.rs`:

Find:

```rust
        ffi_guard(|| lane_rows(lock(&self.inner).doc(), seq))
    }

    /// Renames sequence `seq` (one undo step).
    pub fn rename_sequence(&self, seq: u64, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
```

Replace with:

```rust
        ffi_guard(|| lane_rows(lock(&self.inner).doc(), seq))
    }

    /// The drum lanes of sequence `seq` with their rows and each row's
    /// notes in the active take (the drum grid, 4e).
    pub fn drum_lanes(&self, seq: u64) -> Result<Vec<crate::drum::DrumLaneRow>, VizError> {
        ffi_guard(|| crate::drum::drum_lanes(lock(&self.inner).doc(), seq))
    }

    /// Renames sequence `seq` (one undo step).
    pub fn rename_sequence(&self, seq: u64, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
```

`FakeSession` must implement the new `SessionProtocol` method. In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:

```swift
        return ev
    }

    var infoTable: [UInt64: SequenceInfo] = [:]
    var infoCalls = 0
    func secondsAt(seq: UInt64, ticks: [Int64]) throws -> [Double] { ticks.map { Double($0) / 30_720 } }
```

Replace with:

```swift
        return ev
    }

    /// Drum lanes by sequence (plan 4e); a sequence not listed has none.
    var drumTable: [UInt64: [DrumLaneRow]] = [:]
    var drumLanesCalls = 0
    func drumLanes(seq: UInt64) throws -> [DrumLaneRow] {
        drumLanesCalls += 1
        if let e = readError { throw e }
        return drumTable[seq] ?? []
    }

    var infoTable: [UInt64: SequenceInfo] = [:]
    var infoCalls = 0
    func secondsAt(seq: UInt64, ticks: [Int64]) throws -> [Double] { ticks.map { Double($0) / 30_720 } }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test drum --test edit 2>&1 | grep -E "^test result|FAILED|panicked"
scripts/build-ffi.sh && grep -n "groove: \[Int64\] = \[\]" app/Sources/VizDAWCore/vizdaw_ffi.swift | head -1
scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
```

Expected: `drum` 7 passed, `edit` unchanged; the generated `QuantizeRow` initialiser defaults `groove` to `[]`; Swift builds (no Swift `switch` covers `EditCommand` exhaustively) and its 581 tests pass.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
git add Cargo.lock crates/core-ffi/Cargo.toml crates/core-ffi/src/drum.rs crates/core-ffi/src/edit.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/src/session.rs crates/core-ffi/tests/drum.rs crates/core-ffi/tests/edit.rs \
  app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(ffi): drum lanes, the Drum edit group, grooves, and the drum grid's logic

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 858 Rust tests listed (851 + 7); Swift 581.

---

### Task 5: Swift — drum grid settings and `DrumGridModel`: lanes, strokes, rows, quantize, nudge

**Files:**
- Create: `app/Sources/VizDAWAppModel/DrumGrid.swift`, `app/Sources/VizDAWAppModel/DrumGridModel.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorLayout.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Test: `app/Tests/VizDAWAppModelTests/DrumGridModelTests.swift`

**Interfaces:**
- Consumes: Task 4's `drum*` functions, records and `.drum(edit:)`; `EditorModel.{session, snapshotVersion, layout, updateLayout, edit, beginGesture, updateGesture, endGesture, cancelGesture, gesture, status}`, `EditorModel.gestureOpenMessage`, `KeyedDecodingContainer.field`, `EditorLayout.sane`, `DetailSlot`, `LaneKey`, `MeterMath.ppq`; test helpers `editorFake()`, `lane(_:_:)`, `nextSnapshot`, `settle()`.
- Produces: `DrumView {matrix, timeline}` (`title`, `row`); `DrumGridSettings` (`view, step, spacing, duration, velocity, rowHeight, velocityHeight, ticksPerPoint`, `row: DrumGridSettingsRow`, `lengths`, `lengths(including:)`); `EditorLayout.drumGrid`; `DrumRowKey { lane, row }`; `DrumGridModel` (`init(document:slot:read:)`, `pane(_:)`, `window(_:of:)`, `slot`, `becameActive()`, `state`, `target`, `seq`, `focusRange`, `lanes`, `settings`, `updateSettings`, `layout`, `spot(_:)`, `row(_:_:)`, `indexes(_:)`, `selected`, `currentLane`, `select(lane:row:)`, `selectedRow`, `stroke`, `strokeBegan(lane:row:at:snapped:grab:velocity:erase:)`, `strokeMoved(at:grab:velocity:)`, `strokeEnded(at:grab:velocity:)`, `strokeCancelled()`, `addRow()`, `removeSelectedRow()`, `splitSelectedRow()`, `showVelocity(_:)`, `rename(_:to:)`, `drop(lane:row:atY:)`, `quantizeChoices`, `quantizeTitle(_:)`, `setQuantize(lane:_:)`, `nudge(lane:steps:)`, labels `paintLabel`, `eraseLabel`, `noRowMessage`); `EditorModel.markDrumLane(seq:lane:on:)`; `DocumentModel.{drumGrid(_:), dropDrumGrid(_:), activeDrumGrid, drumGridForCommands(), selectedLane}`; test helpers `drumFake()`, `drumHit(_:_:velocity:)`, `drumRow(_:_:_:hits:velocity:)`.

Rulings 2, 3, 5–7, 10, 11, 17. The model reads `drumLanes` once per snapshot version (Busy keeps the last rows; another failure shows once in the status line, set on the next main-actor turn because reads happen while views draw). A stroke remembers the row's hits as they were when it began: its live gesture edits change the document under it, and every command is measured from the start state (spec §3.2). It opens its gesture on the first move (a click opens it at mouse-up), labelled for paint or erase; a refused gesture says "Finish the drag first." when another drag is still finishing; an erase that covers nothing opens nothing. `drumGridForCommands()` is the grid last pressed in while it still shows a drum grid, else the pane's when the pane shows one.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/DrumGridModelTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/DrumGridModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
private let s16 = q / 4

func drumHit(_ id: UInt64, _ tick: Int64, velocity: UInt16 = 100 << 9) -> DrumHitRow {
    DrumHitRow(id: id, tick: tick, duration: s16, velocity: velocity, mute: false)
}

func drumRow(_ id: UInt64, _ name: String, _ note: UInt8, hits: [DrumHitRow] = [], velocity: Bool = false) -> DrumRowRow {
    DrumRowRow(
        id: id, name: name, note: note, dest: nil, destination: "Kit", showVelocity: velocity, hits: hits)
}

/// Sequence 3 of `editorFake()` with drum lanes "Drums" (40: Kick 1 with
/// hits 11 at 0 and 12 at a beat, Snare 2 with hit 13 at a beat) and
/// "Perc" (41: Clap 3).
func drumFake() -> FakeSession {
    let f = editorFake()
    f.laneTable[3] = [lane(30, "Groove"), lane(40, "Drums"), lane(41, "Perc")]
    f.snap.laneRevs += [LaneRev(seq: 3, lane: 40, rev: 4), LaneRev(seq: 3, lane: 41, rev: 5)]
    f.drumTable[3] = [
        DrumLaneRow(
            lane: 40, name: "Drums", destination: "Kit", mute: false, solo: false, playQuantize: nil, playShift: 0,
            rows: [
                drumRow(1, "Kick", 36, hits: [drumHit(11, 0), drumHit(12, q)]),
                drumRow(2, "Snare", 38, hits: [drumHit(13, q)]),
            ]),
        DrumLaneRow(
            lane: 41, name: "Perc", destination: "Kit", mute: false, solo: false, playQuantize: nil, playShift: 0,
            rows: [drumRow(3, "Clap", 39)]),
    ]
    return f
}

@MainActor
@Suite struct DrumGridModelTests {
    func make(_ target: EditorTarget = .lane(seq: 3, lane: 40)) throws -> (FakeSession, DocumentModel, DrumGridModel) {
        let f = drumFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(target, kind: .drumGrid)
        f.calls = []
        return (f, m, m.drumGrid(.pane))
    }

    @Test func showsTheDrumLanesOfTheShownSequence() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(g.lanes.map(\.name) == ["Drums", "Perc"])
        // Header, Kick, Snare, header, Clap, the new-lane band.
        #expect(g.layout.bands.count == 6)
        #expect(g.spot(30) == .row(lane: 0, row: 0, velocity: nil))
        #expect(g.spot(10) == .header(lane: 0))
        #expect(g.spot(1_000) == .newLane)
        #expect(g.currentLane?.lane == 40)
        _ = g.lanes
        _ = g.layout
        #expect(f.drumLanesCalls == 1, "read once per snapshot")
        #expect(m.drumGrid(.pane) === g, "made once per slot")
        #expect(m.drumGridForCommands() === g)
        // A torn window's grid pressed in is the one the menu acts on.
        let id = try #require(m.editor.tearOff(.drumGrid, shown: 3)?.id)
        let w = m.drumGrid(.window(id))
        w.becameActive()
        #expect(m.drumGridForCommands() === w)
        m.editor.openDetail(.lane(seq: 3, lane: 40), kind: .list)
        m.editor.closeWindow(id)
        #expect(m.drumGridForCommands() == nil, "only while a slot shows a drum grid")
    }

    @Test func aClickOnAnEmptyCellPaintsOneHit() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.strokeBegan(lane: 0, row: 1, at: 2 * s16 + 5, snapped: 2 * s16, grab: 1, velocity: nil, erase: false)
        g.strokeEnded(at: 2 * s16 + 5, grab: 1, velocity: nil)
        #expect(f.calls == ["begin Paint Drum Hits", "gesture", "end"])
        #expect(
            f.gestureEdits.last
                == .drum(
                    edit: .paint(
                        seq: 3, lane: 40, row: 2, hits: [DrumPaintHitRow(tick: 2 * s16, velocity: 100 << 9)],
                        window: s16, duration: q / 8)))
        #expect(g.selectedRow == DrumRowKey(lane: 40, row: 2))
        #expect(g.stroke == nil)
    }

    @Test func aClickOnAHitErasesIt() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.strokeBegan(lane: 0, row: 0, at: q + 9, snapped: q, grab: 1, velocity: nil, erase: false)
        g.strokeEnded(at: q + 9, grab: 1, velocity: nil)
        #expect(f.calls.first == "begin Erase Drum Hits")
        #expect(f.gestureEdits.last == .event(edit: .delete(events: [12])))
    }

    @Test func aDragPaintsARunAndTheVelocityLaneSetsEachHit() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.updateSettings { $0.spacing = q / 2 }
        g.strokeBegan(lane: 0, row: 1, at: 0, snapped: 0, grab: 1, velocity: 20, erase: false)
        g.strokeMoved(at: 2 * q + 1, grab: 1, velocity: 90)
        let hits: [DrumPaintHitRow] = [
            DrumPaintHitRow(tick: 0, velocity: 20 << 9), DrumPaintHitRow(tick: q / 2, velocity: 90 << 9),
            DrumPaintHitRow(tick: q, velocity: 90 << 9), DrumPaintHitRow(tick: 3 * q / 2, velocity: 90 << 9),
            DrumPaintHitRow(tick: 2 * q, velocity: 90 << 9),
        ]
        #expect(
            f.gestureEdits.last
                == .drum(edit: .paint(seq: 3, lane: 40, row: 2, hits: hits, window: s16, duration: q / 8)))
        g.strokeEnded(at: q / 2 + 3, grab: 1, velocity: 127)
        #expect(
            f.gestureEdits.last
                == .drum(edit: .paint(seq: 3, lane: 40, row: 2, hits: Array(hits.prefix(2)), window: s16, duration: q / 8)),
            "back towards the start: the run shrinks, each hit keeps its velocity")
        #expect(f.calls.last == "end")
        #expect(m.editor.gesture == nil)
    }

    @Test func escapeAndRefusalsLeaveNoUndoStep() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        g.strokeBegan(lane: 0, row: 1, at: 0, snapped: 0, grab: 1, velocity: nil, erase: false)
        g.strokeMoved(at: q, grab: 1, velocity: nil)
        g.strokeCancelled()
        #expect(f.calls == ["begin Paint Drum Hits", "gesture", "abort"])
        #expect(g.stroke == nil && m.editor.gesture == nil)
        // The eraser on an empty cell erases nothing: no gesture at all.
        f.calls = []
        g.strokeBegan(lane: 1, row: 0, at: 0, snapped: 0, grab: 1, velocity: nil, erase: true)
        g.strokeEnded(at: 0, grab: 1, velocity: nil)
        #expect(f.calls.isEmpty)
        // Another drag still finishing: the stroke is refused and says so.
        let sent = f.gestureEdits.count
        #expect(m.editor.beginGesture("Move", lanes: [LaneKey(3, 40)], origin: .other))
        g.strokeBegan(lane: 0, row: 1, at: 0, snapped: 0, grab: 1, velocity: nil, erase: false)
        g.strokeEnded(at: 0, grab: 1, velocity: nil)
        #expect(m.editor.status == EditorModel.gestureOpenMessage)
        #expect(f.gestureEdits.count == sent)
    }

    @Test func rowCommandsActOnTheSelectedRow() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(!g.removeSelectedRow())
        #expect(m.editor.status == DrumGridModel.noRowMessage)
        #expect(g.addRow())
        #expect(f.edits.last == .drum(edit: .addRow(seq: 3, lane: 40, name: "Closed Hat", note: 42, dest: nil)))
        g.select(lane: 0, row: 1)
        #expect(g.selected?.row.name == "Snare")
        #expect(g.showVelocity(nil))
        #expect(f.edits.last == .drum(edit: .showVelocity(seq: 3, lane: 40, row: 2, show: true)))
        #expect(g.splitSelectedRow())
        #expect(f.edits.last == .drum(edit: .splitRow(seq: 3, lane: 40, row: 2)))
        #expect(g.removeSelectedRow())
        #expect(f.edits.last == .drum(edit: .removeRow(seq: 3, lane: 40, row: 2)))
        #expect(g.rename(DrumRowKey(lane: 40, row: 1), to: "  Bass Drum "))
        #expect(f.edits.last == .drum(edit: .renameRow(seq: 3, lane: 40, row: 1, name: "Bass Drum")))
        #expect(!g.rename(DrumRowKey(lane: 40, row: 1), to: "Kick"), "unchanged: no undo step")
        // Kick dropped on Clap's band moves it into Perc; far below splits it.
        #expect(g.drop(lane: 0, row: 0, atY: 90))
        #expect(f.edits.last == .drum(edit: .moveRowToLane(seq: 3, lane: 40, row: 1, toLane: 41)))
        #expect(g.drop(lane: 0, row: 0, atY: 500))
        #expect(f.edits.last == .drum(edit: .splitRow(seq: 3, lane: 40, row: 1)))
        let n = f.edits.count
        #expect(!g.drop(lane: 0, row: 0, atY: 30), "its own place")
        #expect(f.edits.count == n)
    }

    @Test func laneQuantizeNudgeAndDrumLaneMarking() throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        let swing = DrumGridModel.quantizeChoices.first { $0.title == "1/16 Swing 62%" }
        #expect(swing != nil)
        #expect(g.setQuantize(lane: 40, swing?.quantize))
        #expect(f.edits.last == .lane(edit: .setPlayQuantize(seq: 3, lane: 40, quantize: swing?.quantize)))
        #expect(g.quantizeTitle(g.lanes[0]) == "Off")
        #expect(g.nudge(lane: 40, steps: -2))
        #expect(f.edits.last == .lane(edit: .setPlayShift(seq: 3, lane: 40, shift: -240)))
        #expect(m.editor.markDrumLane(seq: 3, lane: 30, on: true))
        #expect(f.edits.last == .drum(edit: .markLane(seq: 3, lane: 30, drum: true)))
    }

    @Test func busyKeepsTheLanesAndAFailureShowsOnce() async throws {
        let (f, m, g) = try make()
        defer { withExtendedLifetime(m) {} }
        #expect(g.lanes.count == 2)
        f.readError = .Busy(message: "busy")
        nextSnapshot(f) { _ in }
        m.editor.edit(.drum(edit: .markLane(seq: 3, lane: 30, drum: true)))
        #expect(g.lanes.count == 2, "Busy keeps what was read")
        f.readError = .Model(message: "gone")
        nextSnapshot(f) { _ in }
        m.editor.edit(.drum(edit: .markLane(seq: 3, lane: 30, drum: false)))
        #expect(g.lanes.isEmpty)
        let calls = f.drumLanesCalls
        _ = g.lanes
        #expect(f.drumLanesCalls == calls, "not retried at the same version")
        await settle()
        #expect(m.editor.status == "gone")
    }

    @Test func settingsDecodeTolerantly() throws {
        let json = #"{"drumGrid":{"view":"timeline","step":-5,"velocity":200,"rowHeight":1e9,"spacing":"x"}}"#
        let l = EditorLayout.decode(json)
        #expect(l.drumGrid.view == .timeline)
        #expect(l.drumGrid.step == 1 && l.drumGrid.velocity == 127 && l.drumGrid.rowHeight == 64)
        #expect(l.drumGrid.spacing == DrumGridSettings().spacing)
        #expect(EditorLayout.decode("{}").drumGrid == DrumGridSettings())
        #expect(DrumGridSettings.lengths(including: 999).last?.1 == 999)
        #expect(DrumGridSettings.lengths(including: q).count == DrumGridSettings.lengths.count)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build --build-tests 2>&1 | grep -E "error:" | head -3
```

Expected: FAIL to compile; the errors name what this task adds (`DrumRowKey`, `DrumGridModel`, `drumGrid`, `DrumGridSettings`).

- [ ] **Step 3: Implement**

Create `app/Sources/VizDAWAppModel/DrumGrid.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/DrumGrid.swift <<'VIZDAW_EOF'
import VizDAWCore

/// How the drum grid shows hits (spec §4e).
public enum DrumView: String, Codable, CaseIterable, Sendable {
    case matrix, timeline

    public var title: String {
        switch self {
        case .matrix: "Step Matrix"
        case .timeline: "Timeline"
        }
    }

    public var row: DrumViewRow {
        switch self {
        case .matrix: .matrix
        case .timeline: .timeline
        }
    }
}

/// The drum grid's bar, shared by every drum grid and saved with the
/// project (Plan 4e Ruling 11). Rust clamps every value it is given;
/// decoding is tolerant like `EditorLayout`'s.
public struct DrumGridSettings: Codable, Equatable, Sendable {
    public var view: DrumView = .matrix
    /// A matrix cell.
    public var step: Int64 = MeterMath.ppq / 4
    /// Distance between a stroke's hits.
    public var spacing: Int64 = MeterMath.ppq / 4
    /// Length of a painted hit.
    public var duration: Int64 = MeterMath.ppq / 8
    /// Velocity (1–127) of a painted hit while its row's velocity lane is hidden.
    public var velocity: UInt8 = 100
    public var rowHeight: Double = 20
    public var velocityHeight: Double = 28
    /// The drum grid's own zoom (a 4/4 bar is 640 pt at 96).
    public var ticksPerPoint: Double = 96

    public init() {}

    enum CodingKeys: String, CodingKey {
        case view, step, spacing, duration, velocity, rowHeight, velocityHeight, ticksPerPoint
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = DrumGridSettings()
        view = c.field(.view, d.view)
        step = max(c.field(.step, d.step), 1)
        spacing = max(c.field(.spacing, d.spacing), 1)
        duration = max(c.field(.duration, d.duration), 1)
        velocity = min(max(c.field(.velocity, d.velocity), 1), 127)
        rowHeight = EditorLayout.sane(c.field(.rowHeight, d.rowHeight), 12...64, d.rowHeight)
        velocityHeight = EditorLayout.sane(c.field(.velocityHeight, d.velocityHeight), 16...96, d.velocityHeight)
        ticksPerPoint = EditorLayout.sane(
            c.field(.ticksPerPoint, d.ticksPerPoint), EditorLayout.ticksPerPointRange, d.ticksPerPoint)
    }

    /// The settings as Rust takes them.
    public var row: DrumGridSettingsRow {
        DrumGridSettingsRow(
            view: view.row, step: step, spacing: spacing, duration: duration, velocity: velocity,
            rowHeight: rowHeight, velocityHeight: velocityHeight)
    }

    /// Step, spacing and duration choices: title and ticks.
    public static let lengths: [(String, Int64)] = [
        ("1 Bar", 4 * MeterMath.ppq), ("1/2", 2 * MeterMath.ppq), ("1/4", MeterMath.ppq),
        ("1/8", MeterMath.ppq / 2), ("1/8 Triplet", MeterMath.ppq / 3), ("1/16", MeterMath.ppq / 4),
        ("1/16 Triplet", MeterMath.ppq / 6), ("1/32", MeterMath.ppq / 8), ("1/64", MeterMath.ppq / 16),
    ]

    /// `lengths`, plus `current` when it is none of them (a hand-edited
    /// `ui.json`), so a picker never shows blank.
    public static func lengths(including current: Int64) -> [(String, Int64)] {
        lengths.contains { $0.1 == current } ? lengths : lengths + [("\(current) ticks", current)]
    }
}
VIZDAW_EOF
```

Create `app/Sources/VizDAWAppModel/DrumGridModel.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/DrumGridModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// A drum row by lane and row ID.
public struct DrumRowKey: Hashable, Sendable {
    public let lane: UInt64
    public let row: UInt64

    public init(lane: UInt64, row: UInt64) {
        self.lane = lane
        self.row = row
    }
}

/// One drum grid (spec §4e, Plan 4e Rulings 2–8): the drum lanes of the
/// shown sequence, the selected row, the stroke in progress and the
/// commands it sends. The grid's logic (layout, spots, cells, strokes,
/// drops) is Rust's (`drum_*` functions); this model keeps the state and
/// talks to the session. The detail pane and every torn-off window have
/// their own; the bar's settings are shared (`EditorLayout.drumGrid`).
@Observable
@MainActor
public final class DrumGridModel {
    /// Weak: closing a document while its grid is on screen must not trap.
    public weak var document: DocumentModel?
    public let editor: EditorModel
    /// Where it is shown (the Drum menu acts on the last one pressed in).
    public let slot: DetailSlot
    @ObservationIgnored private let read: @MainActor () -> DetailState?
    /// The row the row commands act on.
    public var selectedRow: DrumRowKey?
    public private(set) var stroke: DrumStrokeRow?
    /// The stroke's row and the row's hits when it began (the start state
    /// its live edits are measured from).
    @ObservationIgnored private var strokeTarget: (seq: UInt64, lane: UInt64, row: UInt64, hits: [DrumHitRow])?
    @ObservationIgnored private var strokeOpen = false
    @ObservationIgnored private var lanesCache: (seq: UInt64, version: UInt64, lanes: [DrumLaneRow])?
    @ObservationIgnored private var lanesFailure: (seq: UInt64, version: UInt64)?
    @ObservationIgnored private var layoutCache: (shapes: [[Bool]], settings: DrumGridSettingsRow, layout: DrumGridLayoutRow)?

    public static let paintLabel = "Paint Drum Hits"
    public static let eraseLabel = "Erase Drum Hits"
    public static let noRowMessage = "Select a drum row first."

    public init(document: DocumentModel, slot: DetailSlot, read: @escaping @MainActor () -> DetailState?) {
        self.document = document
        self.editor = document.editor
        self.slot = slot
        self.read = read
    }

    /// The drum grid of the detail pane.
    public static func pane(_ document: DocumentModel) -> DrumGridModel {
        DrumGridModel(document: document, slot: .pane, read: { [weak document] in document?.editor.layout.detail })
    }

    /// The drum grid of torn-off window `id`.
    public static func window(_ id: UUID, of document: DocumentModel) -> DrumGridModel {
        DrumGridModel(
            document: document, slot: .window(id),
            read: { [weak document] in document?.editor.layout.windows.first { $0.id == id }?.detail })
    }

    /// A press in this grid: the Drum menu acts on it from now on.
    public func becameActive() { document?.activeDrumGrid = slot }

    // MARK: What is shown

    public var state: DetailState? { read() }
    public var target: EditorTarget? { state?.target }
    public var seq: UInt64? { target?.seq }

    /// A block target's range, scrolled to when shown.
    public var focusRange: TickRange? {
        guard case let .block(_, _, start, end)? = target else { return nil }
        return TickRange(start: start, end: max(end, start + 1))
    }

    /// The drum lanes of the shown sequence, read once per snapshot. Busy
    /// keeps what was read before; another failure shows once in the
    /// status line and reads as none.
    public var lanes: [DrumLaneRow] {
        guard let seq else { return [] }
        let v = editor.snapshotVersion
        if let c = lanesCache, c.seq == seq, c.version == v { return c.lanes }
        if let f = lanesFailure, f.seq == seq, f.version == v { return [] }
        do {
            let l = try editor.session.drumLanes(seq: seq)
            lanesCache = (seq, v, l)
            return l
        } catch VizError.Busy {
            return lanesCache?.seq == seq ? lanesCache?.lanes ?? [] : []
        } catch {
            lanesFailure = (seq, v)
            let message = (error as? VizError)?.message ?? error.localizedDescription
            // Not while a view draws: on the next main-actor turn.
            Task { @MainActor [weak editor] in editor?.status = message }
            return []
        }
    }

    public var settings: DrumGridSettings { editor.layout.drumGrid }

    public func updateSettings(_ change: (inout DrumGridSettings) -> Void) {
        editor.updateLayout { change(&$0.drumGrid) }
    }

    /// The bands of the shown lanes (Rust's layout, cached).
    public var layout: DrumGridLayoutRow {
        let shapes = lanes.map { $0.rows.map(\.showVelocity) }
        let s = settings.row
        if let c = layoutCache, c.shapes == shapes, c.settings == s { return c.layout }
        let l = (try? drumGridLayout(shapes: shapes, settings: s)) ?? DrumGridLayoutRow(bands: [], height: 0)
        layoutCache = (shapes, s, l)
        return l
    }

    /// What height `y` (content coordinates) is over.
    public func spot(_ y: Double) -> DrumSpotRow {
        (try? drumGridSpot(layout: layout, y: y)) ?? .nothing
    }

    public func row(_ lane: Int, _ row: Int) -> DrumRowRow? {
        guard lanes.indices.contains(lane), lanes[lane].rows.indices.contains(row) else { return nil }
        return lanes[lane].rows[row]
    }

    /// Lane and row indexes of `key`.
    public func indexes(_ key: DrumRowKey) -> (lane: Int, row: Int)? {
        guard let l = lanes.firstIndex(where: { $0.lane == key.lane }),
            let r = lanes[l].rows.firstIndex(where: { $0.id == key.row })
        else { return nil }
        return (l, r)
    }

    /// The selected row while it still exists.
    public var selected: (lane: DrumLaneRow, row: DrumRowRow)? {
        guard let key = selectedRow, let i = indexes(key) else { return nil }
        return (lanes[i.lane], lanes[i.lane].rows[i.row])
    }

    /// The lane the lane controls act on: the selected row's, else the first.
    public var currentLane: DrumLaneRow? { selected?.lane ?? lanes.first }

    public func select(lane: Int, row: Int) {
        guard let r = self.row(lane, row) else { return }
        selectedRow = DrumRowKey(lane: lanes[lane].lane, row: r.id)
    }

    // MARK: Strokes (Plan 4e Rulings 5–7)

    /// A press on row `row` of lane `lane` at tick `at` (`snapped`: on the
    /// shared edit grid; `grab`: a hit's least width in ticks; `velocity`:
    /// the row's velocity lane under the pointer; `erase`: the eraser).
    public func strokeBegan(
        lane: Int, row: Int, at: Int64, snapped: Int64, grab: Int64, velocity: UInt8?, erase: Bool
    ) {
        // A stroke whose mouse-up was lost must not hold its gesture open.
        if stroke != nil { strokeCancelled() }
        guard let seq, let r = self.row(lane, row) else { return }
        let laneID = lanes[lane].lane
        selectedRow = DrumRowKey(lane: laneID, row: r.id)
        guard
            let s = try? drumStrokeBegin(
                lane: UInt32(lane), row: UInt32(row), hits: r.hits, at: at, snapped: snapped, grab: grab,
                velocity: velocity, eraseTool: erase, settings: settings.row)
        else { return }
        stroke = s
        strokeTarget = (seq, laneID, r.id, r.hits)
        strokeOpen = false
    }

    /// The pointer moved past the view's dead zone.
    public func strokeMoved(at: Int64, grab: Int64, velocity: UInt8?) {
        guard let next = extended(at: at, grab: grab, velocity: velocity) else { return }
        stroke = next
        if !strokeOpen {
            guard open(next) else {
                stroke = nil
                return
            }
        }
        editor.updateGesture(command(next))
    }

    /// The pointer went up: a click toggles one hit, a drag ends its run.
    public func strokeEnded(at: Int64, grab: Int64, velocity: UInt8?) {
        guard let s = extended(at: at, grab: grab, velocity: velocity) else { return }
        stroke = nil
        let final = command(s)
        if !strokeOpen {
            guard final != nil, open(s) else { return }
        }
        strokeOpen = false
        if let final {
            editor.endGesture(final)
        } else {
            editor.cancelGesture()
        }
    }

    /// Escape, or the view went away: no undo step.
    public func strokeCancelled() {
        if stroke != nil, strokeOpen { editor.cancelGesture() }
        stroke = nil
        strokeOpen = false
    }

    private func extended(at: Int64, grab: Int64, velocity: UInt8?) -> DrumStrokeRow? {
        guard let s = stroke, let t = strokeTarget else { return nil }
        return try? drumStrokeExtend(
            stroke: s, hits: t.hits, at: at, grab: grab, velocity: velocity, settings: settings.row)
    }

    func command(_ s: DrumStrokeRow) -> EditCommand? {
        guard let t = strokeTarget else { return nil }
        return (try? drumStrokeCommand(stroke: s, seq: t.seq, lane: t.lane, row: t.row, settings: settings.row))
            ?? nil
    }

    private func open(_ s: DrumStrokeRow) -> Bool {
        guard let t = strokeTarget else { return false }
        let pending = editor.gesture != nil
        guard
            editor.beginGesture(
                s.erase ? Self.eraseLabel : Self.paintLabel, lanes: [LaneKey(t.seq, t.lane)], origin: .other)
        else {
            if pending { editor.status = EditorModel.gestureOpenMessage }
            return false
        }
        strokeOpen = true
        return true
    }

    // MARK: Rows (spec §4e)

    private func drum(_ e: DrumEdit) -> Bool { editor.edit(.drum(edit: e)) }

    /// A row for the next free kit note in the selected row's lane (else
    /// the first drum lane); it becomes the selected row.
    @discardableResult
    public func addRow() -> Bool {
        guard let seq, let lane = currentLane else {
            editor.status = "There is no drum lane to add a row to."
            return false
        }
        // A `Vec<u8>` crosses the FFI as `Data`.
        guard let new = (try? drumNextRow(used: Data(lane.rows.map(\.note)))) ?? nil else {
            editor.status = "Every note already has a row in \(lane.name)."
            return false
        }
        guard drum(.addRow(seq: seq, lane: lane.lane, name: new.name, note: new.note, dest: nil)) else { return false }
        let rows = lanes.first { $0.lane == lane.lane }?.rows ?? []
        if let r = rows.first(where: { r in r.note == new.note && r.dest == nil }) {
            selectedRow = DrumRowKey(lane: lane.lane, row: r.id)
        }
        return true
    }

    @discardableResult
    public func removeSelectedRow() -> Bool {
        guard let seq, let s = selected else {
            editor.status = Self.noRowMessage
            return false
        }
        return drum(.removeRow(seq: seq, lane: s.lane.lane, row: s.row.id))
    }

    @discardableResult
    public func splitSelectedRow() -> Bool {
        guard let seq, let s = selected else {
            editor.status = Self.noRowMessage
            return false
        }
        return drum(.splitRow(seq: seq, lane: s.lane.lane, row: s.row.id))
    }

    /// Shows or hides the selected row's velocity lane (`nil` toggles).
    @discardableResult
    public func showVelocity(_ on: Bool?) -> Bool {
        guard let seq, let s = selected else {
            editor.status = Self.noRowMessage
            return false
        }
        let show = on ?? !s.row.showVelocity
        guard show != s.row.showVelocity else { return true }
        return drum(.showVelocity(seq: seq, lane: s.lane.lane, row: s.row.id, show: show))
    }

    @discardableResult
    public func rename(_ key: DrumRowKey, to name: String) -> Bool {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let seq, let i = indexes(key), !name.isEmpty, name != lanes[i.lane].rows[i.row].name else { return false }
        return drum(.renameRow(seq: seq, lane: key.lane, row: key.row, name: name))
    }

    /// Row `row` of lane `lane` dragged by its header and released at
    /// height `y` (content coordinates): reordered, moved to another drum
    /// lane or split into a new lane (Rust decides which).
    @discardableResult
    public func drop(lane: Int, row: Int, atY y: Double) -> Bool {
        guard let seq, let r = self.row(lane, row),
            let cmd = (try? drumRowDrop(
                layout: layout, lanes: lanes.map(\.lane), seq: seq, laneIndex: UInt32(lane), row: r.id,
                rowIndex: UInt32(row), y: y)) ?? nil
        else { return false }
        return editor.edit(cmd)
    }

    // MARK: Lanes (play quantize and nudge)

    public static let quantizeChoices: [QuantizeChoiceRow] = (try? drumQuantizeChoices()) ?? []

    public func quantizeTitle(_ lane: DrumLaneRow) -> String {
        (try? drumQuantizeTitle(quantize: lane.playQuantize)) ?? "Custom"
    }

    @discardableResult
    public func setQuantize(lane: UInt64, _ q: QuantizeRow?) -> Bool {
        guard let seq else { return false }
        return editor.edit(.lane(edit: .setPlayQuantize(seq: seq, lane: lane, quantize: q)))
    }

    /// Moves a lane's nudge (play shift) by `steps` steps of a 128th note.
    @discardableResult
    public func nudge(lane: UInt64, steps: Int64) -> Bool {
        guard let seq, let l = lanes.first(where: { $0.lane == lane }) else { return false }
        let shift = (try? drumNudged(shift: l.playShift, steps: steps)) ?? l.playShift
        guard shift != l.playShift else { return true }
        return editor.edit(.lane(edit: .setPlayShift(seq: seq, lane: lane, shift: shift)))
    }
}

extension EditorModel {
    /// Makes lane `lane` of `seq` a drum lane (rows for its notes) or a
    /// normal lane again (Drum > Drum Lane, the lane header menu).
    @discardableResult
    public func markDrumLane(seq: UInt64, lane: UInt64, on: Bool) -> Bool {
        edit(.drum(edit: .markLane(seq: seq, lane: lane, drum: on)))
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/EditorLayout.swift`:

Find:

```swift
    public var pianoRoll = PianoRollSettings()
    /// Every Strip Chart's bar (Plan 4d Ruling 21).
    public var stripChart = StripChartSettings()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow, pianoRoll, stripChart
    }

    public init(from decoder: Decoder) throws {
```

Replace with:

```swift
    public var pianoRoll = PianoRollSettings()
    /// Every Strip Chart's bar (Plan 4d Ruling 21).
    public var stripChart = StripChartSettings()
    /// Every drum grid's bar (Plan 4e Ruling 11).
    public var drumGrid = DrumGridSettings()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow, pianoRoll, stripChart, drumGrid
    }

    public init(from decoder: Decoder) throws {
```

Find:

```swift
        follow = c.field(.follow, d.follow)
        pianoRoll = c.field(.pianoRoll, d.pianoRoll)
        stripChart = c.field(.stripChart, d.stripChart)
    }

    /// Lane heights a hand-edited or corrupt `ui.json` is clamped to.
```

Replace with:

```swift
        follow = c.field(.follow, d.follow)
        pianoRoll = c.field(.pianoRoll, d.pianoRoll)
        stripChart = c.field(.stripChart, d.stripChart)
        drumGrid = c.field(.drumGrid, d.drumGrid)
    }

    /// Lane heights a hand-edited or corrupt `ui.json` is clamped to.
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:

```swift
        for (slot, c) in stripCharts where c.state?.kind != .stripChart { dropStripChart(slot) }
    }

    /// The list default filter and columns (set by `DocumentStore`).
    @ObservationIgnored public var listPreferences = ListPreferences()
    /// One list editor per slot, while that slot shows a list.
```

Replace with:

```swift
        for (slot, c) in stripCharts where c.state?.kind != .stripChart { dropStripChart(slot) }
    }

    /// One drum grid per slot (Plan 4e Ruling 2).
    @ObservationIgnored private var drumGrids: [DetailSlot: DrumGridModel] = [:]
    /// The drum grid last pressed in, which the Drum menu acts on.
    public var activeDrumGrid: DetailSlot?

    /// The drum grid of the detail pane or a torn-off window, made once.
    public func drumGrid(_ slot: DetailSlot) -> DrumGridModel {
        if let m = drumGrids[slot] { return m }
        let m: DrumGridModel
        switch slot {
        case .pane: m = .pane(self)
        case .window(let id): m = .window(id, of: self)
        }
        drumGrids[slot] = m
        return m
    }

    /// A torn-off window closed: its drum grid goes (and its stroke).
    public func dropDrumGrid(_ slot: DetailSlot) {
        drumGrids.removeValue(forKey: slot)?.strokeCancelled()
        if activeDrumGrid == slot { activeDrumGrid = nil }
    }

    /// Drum grids whose slot no longer shows one go.
    func pruneDrumGrids() {
        for (slot, g) in drumGrids where g.state?.kind != .drumGrid { dropDrumGrid(slot) }
    }

    /// The drum grid the Drum menu acts on: the last one pressed in while
    /// it still shows a drum grid, else the detail pane's when it shows one.
    public func drumGridForCommands() -> DrumGridModel? {
        if let slot = activeDrumGrid, let g = drumGrids[slot], g.state?.kind == .drumGrid { return g }
        guard editor.layout.detail?.kind == .drumGrid else { return nil }
        return drumGrid(.pane)
    }

    /// The lane Drum > Drum Lane acts on: the selection's first lane of the
    /// shown sequence.
    public var selectedLane: LaneRow? {
        guard let seq = selectedSeq,
            let ref = editor.selection.lanes.first(where: { $0.seq == seq })
        else { return nil }
        return lanes.first { $0.id == ref.lane }
    }

    /// The list default filter and columns (set by `DocumentStore`).
    @ObservationIgnored public var listPreferences = ListPreferences()
    /// One list editor per slot, while that slot shows a list.
```

Find:

```swift
        editor.layoutChanged = { [weak self] in
            self?.pruneLists()
            self?.pruneStripCharts()
        }
    }

```

Replace with:

```swift
        editor.layoutChanged = { [weak self] in
            self?.pruneLists()
            self?.pruneStripCharts()
            self?.pruneDrumGrids()
        }
    }

```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter DrumGridModelTests 2>&1 | tail -1
```

Expected: `Test run with 9 tests in 1 suite passed`. In `drumFake()` the layout is header 0–22, Kick 22–42, Snare 42–62, Perc's header 62–84, Clap 84–104, the new-lane band 104–132: a Kick drop at height 90 lands on Clap (into Perc), at 500 below everything (a new lane).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAWAppModel/DrumGrid.swift app/Sources/VizDAWAppModel/DrumGridModel.swift \
  app/Sources/VizDAWAppModel/EditorLayout.swift app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Tests/VizDAWAppModelTests/DrumGridModelTests.swift
git commit -m "feat(app-model): DrumGridModel, drum grid settings and the Drum Lane edit

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: Swift 590 (581 + 9); Rust unchanged (858).

---

### Task 6: Swift — `DrumGridView` and the row headers: drawing, the paint tool, row drags, VoiceOver

**Files:**
- Create: `app/Sources/VizDAWEditors/DrumGridView.swift`, `app/Sources/VizDAWEditors/DrumRowHeaderView.swift`
- Test: `app/Tests/VizDAWAppModelTests/DrumGridViewTests.swift`

**Interfaces:**
- Consumes: Task 5's `DrumGridModel`; `EditorCanvas` (subclass surface: `axis`, `verticalOffset`, `meter`, `snap`, `selectedRange`, `contentHeight`, `accessibilityName`, `drawContent(in:)`, `contentMouseDown/Dragged/Up`, `contentCancel`, `accessibilityItems`, `playheadSource`, `onRulerRange`, `onRulerClick`, `onToggleSnap`, `onAxisChange`, `onVerticalScroll`, `reload`, `clampVertical`, `setPlaying`, `follow`), `CanvasAccessibilityItem`, `CanvasElement.reuse`, `ToolMachine`, `Modifiers`, `ControlUI.isChannel`, `MenuAction`, `PitchName.name`, `MeterMath`, `TimeAxis`.
- Produces: `DrumPress { lane, row }`; `DrumA11y.label(row:tick:hit:velocity:meter:)`, `DrumA11y.maxItems` (2 048); `DrumGridView: EditorCanvas` (`init(model:)`; internal: `cellRect`, `hitRect`, `grab`, `toggle(_:tick:)`); `DrumRowHeaderView` (`init(model:)`, `width` 150, `verticalOffset`; internal for tests: `mouseDown(at:clicks:)`, `mouseDragged(to:)`, `mouseUp(at:)`, `rowMenu(_:)`, `renaming`, `beginRename(_:)`); `DrumGridContainer` (`init(model:)`, `canvas`, `headers`).

Rulings 4–8, 12. The view asks the model (and through it Rust) what a height is over and which cells a row has in the dirty rect; it draws them and forwards the pointer through a `ToolMachine<DrumPress?>`: a click toggles (a stroke began and ended at once), a drag paints or erases a run; Escape cancels. The press's row is fixed for the stroke; the velocity of a shown velocity lane comes from `drumRowVelocity` for that row at the pointer's height, clamped to the row. The header column (150 pt) shares the canvas's vertical scroll; a header press selects its row, a double-click renames it in place (the keyboard is a text owner meanwhile, so transport keys stay off), a drag past the dead zone drops the row where it is released, and the context menu holds Rename, Show Velocity, Split Row to Own Lane and Remove Row. The lane header band names the lane and its destination; a row with its own destination names it after the row's name. Both views accept the control channel's first clicks.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/DrumGridViewTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/DrumGridViewTests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

private let q: Int64 = 15_360
private let s16 = q / 4

/// `drumFake()` in a drum grid in the detail pane, 96 ticks per point (a
/// sixteenth is 40 pt). Content bands: header 0–22, Kick 22–42, Snare
/// 42–62 (90 with its velocity lane shown), Perc's header, Clap, the
/// new-lane band.
@MainActor
private func grid(snareVelocity: Bool = false) throws -> (FakeSession, DocumentModel, DrumGridContainer) {
    let f = drumFake()
    if snareVelocity { f.drumTable[3]?[0].rows[1].showVelocity = true }
    let m = try DocumentModel(session: f)
    m.editor.openDetail(.lane(seq: 3, lane: 40), kind: .drumGrid)
    let c = DrumGridContainer(model: m.drumGrid(.pane))
    c.frame = NSRect(x: 0, y: 0, width: 950, height: 300)
    c.layout()
    c.canvas.layout()
    f.calls = []
    return (f, m, c)
}

private func paint(_ ticks: [Int64], velocity: UInt16 = 100 << 9, row: UInt64 = 2) -> EditCommand {
    .drum(
        edit: .paint(
            seq: 3, lane: 40, row: row, hits: ticks.map { DrumPaintHitRow(tick: $0, velocity: velocity) },
            window: s16, duration: q / 8))
}

@MainActor
@Suite struct DrumGridViewTests {
    @Test func aClickOnAnEmptyCellPaintsIt() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        c.canvas.contentMouseDown(CGPoint(x: 85, y: 50), clicks: 1, modifiers: [])
        c.canvas.contentMouseUp(CGPoint(x: 85, y: 50), modifiers: [])
        #expect(f.gestureEdits.last == paint([2 * s16]))
        #expect(f.calls == ["begin Paint Drum Hits", "gesture", "end"])
        #expect(m.activeDrumGrid == .pane)
        #expect(m.drumGrid(.pane).selectedRow == DrumRowKey(lane: 40, row: 2))
    }

    @Test func aDragPaintsARunAndEscapeTakesItBack() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        c.canvas.contentMouseDown(CGPoint(x: 5, y: 50), clicks: 1, modifiers: [])
        c.canvas.contentMouseDragged(CGPoint(x: 125, y: 70), modifiers: [])
        #expect(f.gestureEdits.last == paint([0, s16, 2 * s16, 3 * s16]), "the stroke stays on its row")
        c.canvas.contentCancel()
        #expect(f.calls.last == "abort")
        #expect(m.editor.gesture == nil)
        c.canvas.contentMouseUp(CGPoint(x: 125, y: 70), modifiers: [])
        #expect(f.calls.last == "abort", "the release after Escape does nothing")
    }

    @Test func theEraserErasesTheHitsItCrosses() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        m.editor.tool = .eraser
        c.canvas.contentMouseDown(CGPoint(x: 5, y: 30), clicks: 1, modifiers: [])
        c.canvas.contentMouseDragged(CGPoint(x: 165, y: 30), modifiers: [])
        c.canvas.contentMouseUp(CGPoint(x: 165, y: 30), modifiers: [])
        #expect(f.calls.first == "begin Erase Drum Hits")
        #expect(f.gestureEdits.last == .event(edit: .delete(events: [11, 12])))
    }

    @Test func aShownVelocityLaneSetsTheVelocityFromTheHeight() throws {
        let (f, m, c) = try grid(snareVelocity: true)
        defer { withExtendedLifetime(m) {} }
        // The Snare band is 42–90: its top is 127.
        c.canvas.contentMouseDown(CGPoint(x: 85, y: 42), clicks: 1, modifiers: [])
        c.canvas.contentMouseUp(CGPoint(x: 85, y: 42), modifiers: [])
        #expect(f.gestureEdits.last == paint([2 * s16], velocity: 127 << 9))
        // Header, Kick, Snare with its lane, header, Clap, new-lane band.
        let height: CGFloat = 160
        #expect(c.canvas.contentHeight == height)
    }

    @Test func cellsAndHitsAreAccessibleAndPressToggles() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        let items = c.canvas.accessibilityItems()
        #expect(items.first?.label == "Kick, bar 1 beat 1, on, velocity 100")
        let off = try #require(items.first { $0.label == "Snare, bar 1 beat 1 tick 7680, off" })
        off.press?()
        #expect(f.gestureEdits.last == paint([2 * s16]))
        m.drumGrid(.pane).updateSettings { $0.view = .timeline }
        let hits = c.canvas.accessibilityItems().map(\.label)
        #expect(
            hits == [
                "Kick, bar 1 beat 1, on, velocity 100", "Kick, bar 1 beat 2, on, velocity 100",
                "Snare, bar 1 beat 2, on, velocity 100",
            ])
    }

    @Test func aRowHeaderDragMovesTheRowAndADoubleClickRenames() throws {
        let (f, m, c) = try grid()
        defer { withExtendedLifetime(m) {} }
        let h = c.headers
        let top = EditorCanvas.rulerHeight
        // Kick dragged onto Clap (content 84–104): into Perc.
        h.mouseDown(at: CGPoint(x: 20, y: top + 30), clicks: 1)
        h.mouseDragged(to: CGPoint(x: 20, y: top + 95))
        h.mouseUp(at: CGPoint(x: 20, y: top + 95))
        #expect(f.edits.last == .drum(edit: .moveRowToLane(seq: 3, lane: 40, row: 1, toLane: 41)))
        // A press without a drag only selects.
        let n = f.edits.count
        h.mouseDown(at: CGPoint(x: 20, y: top + 50), clicks: 1)
        h.mouseUp(at: CGPoint(x: 20, y: top + 50))
        #expect(f.edits.count == n)
        #expect(m.drumGrid(.pane).selectedRow == DrumRowKey(lane: 40, row: 2))
        h.mouseDown(at: CGPoint(x: 20, y: top + 50), clicks: 2)
        let field = try #require(h.renaming?.field)
        #expect(m.editor.textEditing)
        field.stringValue = "Rim"
        h.controlTextDidEndEditing(Notification(name: NSControl.textDidEndEditingNotification))
        #expect(f.edits.last == .drum(edit: .renameRow(seq: 3, lane: 40, row: 2, name: "Rim")))
        #expect(!m.editor.textEditing)
        #expect(
            h.rowMenu(DrumPress(lane: 0, row: 1)).items.map(\.title)
                == ["Rename…", "Show Velocity", "", "Split Row to Own Lane", "Remove Row"])
        let rows = (h.accessibilityChildren() as? [NSAccessibilityElement])?.compactMap { $0.accessibilityLabel() }
        #expect(rows == ["Kick, C2, Drums, Kit", "Snare, D2, Drums, Kit", "Clap, D#2, Perc, Kit"])
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build --build-tests 2>&1 | grep -E "error:" | head -3
```

Expected: FAIL to compile; the errors name `DrumGridContainer` and `DrumPress`.

- [ ] **Step 3: Implement**

Create `app/Sources/VizDAWEditors/DrumGridView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWEditors
cat > app/Sources/VizDAWEditors/DrumGridView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// What a press in the drum grid landed on: a row, by layout indexes.
public struct DrumPress: Equatable, Sendable {
    public let lane: Int
    public let row: Int
}

/// Accessibility labels of drum cells and hits (Plan 4e Ruling 12).
public enum DrumA11y {
    /// "Kick, bar 2 beat 3 tick 1920, on, velocity 100".
    public static func label(row: String, tick: Int64, hit: Bool, velocity: UInt16, meter: [MeterEntryRow]) -> String {
        let bar = MeterMath.bar(at: tick, meter)
        let beatTicks = MeterMath.beatTicks(bar.meter)
        let into = tick - bar.start
        var s = "\(row), bar \(bar.number) beat \(into / beatTicks + 1)"
        if into % beatTicks != 0 { s += " tick \(into % beatTicks)" }
        s += hit ? ", on, velocity \(velocity >> 9)" : ", off"
        return s
    }

    /// The most cells and hits the grid lists to VoiceOver and the control
    /// channel (the visible ones).
    public static let maxItems = 2_048
}

/// The drum grid's hits (spec §4e): drum lanes as bands of rows over a
/// shared time axis, a step matrix of cells or a timeline of hits, the
/// paint tool and velocity lanes. What a point is over, the cells, the
/// strokes and their commands come from Rust through `DrumGridModel`;
/// this view draws and forwards the pointer.
@MainActor
public final class DrumGridView: EditorCanvas {
    public let model: DrumGridModel
    var editor: EditorModel { model.editor }
    var document: DocumentModel? { model.document }

    private var machine = ToolMachine<DrumPress?>()
    /// The target the view last scrolled to.
    private var scrolledTo: EditorTarget??

    public init(model: DrumGridModel) {
        self.model = model
        super.init(frame: .zero)
        axis = TimeAxis(ticksPerPoint: model.settings.ticksPerPoint)
        playheadSource = { [weak self] now in
            guard let self, let doc = self.document, let seq = self.model.seq, doc.transport.seq == seq else {
                return nil
            }
            return doc.clock.tick(atHostTimeNs: now)
        }
        onRulerRange = { [weak self] r in
            guard let self, let seq = self.model.seq else { return }
            self.editor.selectRange(r, lanes: self.model.lanes.map { LaneKey(seq, $0.lane) })
        }
        onRulerClick = { [weak self] t in self?.document?.locate(t) }
        onToggleSnap = { [weak self] in self?.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [weak self] a in
            guard let self, a.ticksPerPoint != self.editor.layout.drumGrid.ticksPerPoint else { return }
            self.editor.updateLayout { $0.drumGrid.ticksPerPoint = a.ticksPerPoint }
        }
        refresh()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var meter: [MeterEntryRow] { model.seq.flatMap { editor.info($0)?.meter } ?? [] }
    public override var snap: Snap { editor.layout.snap }
    public override var selectedRange: TickRange? { editor.selection.timeRange ?? model.focusRange }
    public override var contentHeight: CGFloat { CGFloat(model.layout.height) }
    public override var accessibilityName: String { "Drum Grid" }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = editor.selection
            _ = document?.playKey
            _ = model.selectedRow
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.refresh()
                self?.observe()
            }
        }
    }

    func refresh() {
        let s = model.settings
        if s.ticksPerPoint != axis.ticksPerPoint {
            axis = TimeAxis(ticksPerPoint: s.ticksPerPoint, originTick: axis.originTick)
        }
        follow = editor.layout.follow
        let key = document?.playKey
        setPlaying(key?.playing == true && key?.seq == model.seq)
        if let t = scrolledTo, t != model.target { needsLayout = true }
        verticalOffset = clampVertical(verticalOffset)
        reload()
    }

    /// The first layout, and the first after the target changed, scrolls
    /// to a block target.
    public override func layout() {
        super.layout()
        let target = model.target
        guard scrolledTo != .some(target), content.bounds.width > 0 else { return }
        scrolledTo = .some(target)
        if let r = model.focusRange {
            axis = axis.scrolled(toTick: Double(r.start) - 16 * axis.ticksPerPoint)
        }
    }

    // MARK: Geometry

    /// A band's top in content coordinates.
    func top(_ b: DrumBandRow) -> CGFloat { CGFloat(b.top) - verticalOffset }

    /// A hit is at least this wide on screen, in ticks.
    var grab: Int64 { Int64((4 * axis.ticksPerPoint).rounded(.up)) }

    /// The rect of matrix cell `tick` in a band.
    func cellRect(_ tick: Int64, _ b: DrumBandRow) -> CGRect {
        let x0 = axis.x(tick)
        let x1 = axis.x(tick + max(model.settings.step, 1))
        return CGRect(x: x0, y: top(b), width: max(x1 - x0, 1), height: CGFloat(b.cells))
    }

    /// The rect of a hit in the timeline.
    func hitRect(_ h: DrumHitRow, _ b: DrumBandRow) -> CGRect {
        let x0 = axis.x(h.tick)
        return CGRect(x: x0, y: top(b), width: max(axis.width(ofTicks: h.duration), 4), height: CGFloat(b.cells))
    }

    private func visibleBands(_ dirty: CGRect) -> [DrumBandRow] {
        model.layout.bands.filter { b in
            let t = top(b)
            return t + CGFloat(b.height) >= dirty.minY && t <= dirty.maxY
        }
    }

    private func cells(_ r: DrumRowRow, _ dirty: CGRect) -> [DrumCellRow] {
        (try? drumGridCells(
            hits: r.hits, from: axis.tick(atX: dirty.minX), to: axis.tick(atX: dirty.maxX) + 1,
            step: model.settings.step)) ?? []
    }

    // MARK: Drawing

    public override func drawContent(in dirty: CGRect) {
        let matrix = model.settings.view == .matrix
        for b in visibleBands(dirty) {
            let y = top(b)
            switch b.kind {
            case .header:
                NSColor.windowBackgroundColor.setFill()
                CGRect(x: dirty.minX, y: y, width: dirty.width, height: CGFloat(b.height)).fill()
            case let .row(lane, row):
                guard let r = model.row(Int(lane), Int(row)) else { continue }
                if model.selectedRow == DrumRowKey(lane: model.lanes[Int(lane)].lane, row: r.id) {
                    NSColor.controlAccentColor.withAlphaComponent(0.08).setFill()
                    CGRect(x: dirty.minX, y: y, width: dirty.width, height: CGFloat(b.height)).fill()
                }
                if matrix {
                    drawCells(r, b, dirty)
                } else {
                    drawHits(r, b, dirty)
                }
                if b.height > b.cells { drawVelocities(r, b, dirty) }
            case .newLane:
                NSString(string: "Drop a row here to give it a lane of its own").draw(
                    at: CGPoint(x: max(dirty.minX, 0) + 8, y: y + 7),
                    withAttributes: [
                        .font: NSFont.systemFont(ofSize: 11), .foregroundColor: NSColor.tertiaryLabelColor,
                    ])
            }
            NSColor.separatorColor.setFill()
            CGRect(x: dirty.minX, y: y + CGFloat(b.height) - 0.5, width: dirty.width, height: 1).fill()
        }
    }

    private func drawCells(_ r: DrumRowRow, _ b: DrumBandRow, _ dirty: CGRect) {
        for c in cells(r, dirty) {
            let rect = cellRect(c.tick, b).insetBy(dx: 1, dy: 2)
            let path = NSBezierPath(roundedRect: rect, xRadius: 2, yRadius: 2)
            if c.hits.isEmpty {
                NSColor.quaternaryLabelColor.withAlphaComponent(0.15).setFill()
                path.fill()
            } else {
                let strength = 0.35 + 0.65 * CGFloat(c.velocity) / 65_535
                NSColor.systemOrange.withAlphaComponent(strength).setFill()
                path.fill()
                if c.hits.contains(where: editor.isSelected(event:)) {
                    NSColor.controlAccentColor.setStroke()
                    path.lineWidth = 2
                    path.stroke()
                }
            }
        }
    }

    private func drawHits(_ r: DrumRowRow, _ b: DrumBandRow, _ dirty: CGRect) {
        for h in r.hits {
            let rect = hitRect(h, b).insetBy(dx: 0, dy: 2)
            guard rect.maxX >= dirty.minX, rect.minX <= dirty.maxX else { continue }
            let path = NSBezierPath(roundedRect: rect, xRadius: 2, yRadius: 2)
            NSColor.systemOrange.withAlphaComponent(h.mute ? 0.3 : 0.85).setFill()
            path.fill()
            if editor.isSelected(event: h.id) {
                NSColor.controlAccentColor.setStroke()
                path.lineWidth = 2
                path.stroke()
            }
        }
    }

    private func drawVelocities(_ r: DrumRowRow, _ b: DrumBandRow, _ dirty: CGRect) {
        let laneTop = top(b) + CGFloat(b.cells)
        let h = CGFloat(b.height - b.cells) - 4
        NSColor.labelColor.withAlphaComponent(0.04).setFill()
        CGRect(x: dirty.minX, y: laneTop, width: dirty.width, height: h + 4).fill()
        NSColor.systemOrange.setFill()
        for hit in r.hits {
            let x = axis.x(hit.tick)
            guard x >= dirty.minX - 4, x <= dirty.maxX else { continue }
            let bar = max(h * CGFloat(hit.velocity) / 65_535, 1)
            CGRect(x: x, y: laneTop + 2 + h - bar, width: 3, height: bar).fill()
        }
    }

    // MARK: Gestures

    /// The tick under `x`, and on the shared edit grid.
    private func ticks(_ x: CGFloat) -> (at: Int64, snapped: Int64) {
        let t = axis.tick(atX: x)
        return (t, snap.floor(t, meter: meter))
    }

    private func velocity(_ press: DrumPress, y: CGFloat) -> UInt8? {
        (try? drumRowVelocity(
            layout: model.layout, lane: UInt32(press.lane), row: UInt32(press.row), y: Double(y + verticalOffset)))
            ?? nil
    }

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        model.becameActive()
        var press: DrumPress?
        if case let .row(lane, row, _) = model.spot(Double(p.y + verticalOffset)) {
            press = DrumPress(lane: Int(lane), row: Int(row))
            model.select(lane: Int(lane), row: Int(row))
        }
        handle(machine.down(press, at: p, clicks: clicks, modifiers: modifiers))
    }

    public override func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.drag(to: p, modifiers: modifiers))
    }

    public override func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.up(at: p, modifiers: modifiers))
    }

    /// Escape; also aborts a gesture whose final edit still waits on Busy.
    public override func contentCancel() {
        handle(machine.cancel())
        model.strokeCancelled()
        if editor.gesture != nil { editor.cancelGesture() }
    }

    private func begin(_ press: DrumPress, at p: CGPoint) {
        let t = ticks(p.x)
        model.strokeBegan(
            lane: press.lane, row: press.row, at: t.at, snapped: t.snapped, grab: grab,
            velocity: velocity(press, y: p.y), erase: editor.tool == .eraser)
    }

    private func handle(_ effects: [ToolMachine<DrumPress?>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(press?, at, _, _):
                begin(press, at: at)
                model.strokeEnded(at: ticks(at.x).at, grab: grab, velocity: velocity(press, y: at.y))
            case let .begin(press?, from, _):
                begin(press, at: from)
            case let .move(press?, _, to, _):
                model.strokeMoved(at: ticks(to.x).at, grab: grab, velocity: velocity(press, y: to.y))
            case let .end(press?, _, to, _):
                model.strokeEnded(at: ticks(to.x).at, grab: grab, velocity: velocity(press, y: to.y))
            case .cancel:
                model.strokeCancelled()
            case .click(nil, _, _, _), .begin(nil, _, _), .move(nil, _, _, _), .end(nil, _, _, _):
                break
            }
        }
    }

    /// Leaving the window ends the press: its release will not come here.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        _ = machine.cancel()
        model.strokeCancelled()
    }

    // MARK: Accessibility (Plan 4e Ruling 12)

    /// The visible cells (matrix) or hits (timeline) of every visible row;
    /// pressing one toggles it like a click.
    public override func accessibilityItems() -> [CanvasAccessibilityItem] {
        let visible = content.bounds
        let matrix = model.settings.view == .matrix
        var out: [CanvasAccessibilityItem] = []
        for b in visibleBands(visible) {
            guard case let .row(lane, row) = b.kind, let r = model.row(Int(lane), Int(row)) else { continue }
            let press = DrumPress(lane: Int(lane), row: Int(row))
            let items: [(tick: Int64, hit: Bool, velocity: UInt16, frame: CGRect)] =
                matrix
                ? cells(r, visible).map { c in (c.tick, !c.hits.isEmpty, c.velocity, cellRect(c.tick, b)) }
                : r.hits.map { h in (h.tick, true, h.velocity, hitRect(h, b)) }.filter { $0.frame.intersects(visible) }
            for i in items {
                guard out.count < DrumA11y.maxItems else { return out }
                let frame = i.frame.intersection(visible)
                guard !frame.isEmpty else { continue }
                let tick = i.tick
                out.append(
                    CanvasAccessibilityItem(
                        label: DrumA11y.label(row: r.name, tick: tick, hit: i.hit, velocity: i.velocity, meter: meter),
                        frame: frame, selected: false,
                        press: { [weak self] in self?.toggle(press, tick: tick) }))
            }
        }
        return out
    }

    /// A click at `tick` on a row (VoiceOver's press).
    func toggle(_ press: DrumPress, tick: Int64) {
        model.becameActive()
        model.strokeBegan(
            lane: press.lane, row: press.row, at: tick, snapped: tick, grab: 1, velocity: nil,
            erase: editor.tool == .eraser)
        model.strokeEnded(at: tick, grab: 1, velocity: nil)
    }
}
VIZDAW_EOF
```

Create `app/Sources/VizDAWEditors/DrumRowHeaderView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWEditors
cat > app/Sources/VizDAWEditors/DrumRowHeaderView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The drum grid's row headers (spec §4e): each lane's name and
/// destination, and per row its name, note and (when it has one of its
/// own) destination. A click selects a row, a double-click
/// renames it, a drag moves it (to another place, another drum lane, or
/// below the last lane for a lane of its own), the context menu holds the
/// row commands.
@MainActor
public final class DrumRowHeaderView: NSView, NSTextFieldDelegate {
    public static let width: CGFloat = 150

    public let model: DrumGridModel
    var editor: EditorModel { model.editor }
    public var verticalOffset: CGFloat = 0 { didSet { needsDisplay = true } }
    /// The row being dragged and the pointer's height (view coordinates).
    private(set) var dragging: (press: DrumPress, y: CGFloat, moved: Bool)?
    /// The row being renamed.
    private(set) var renaming: (key: DrumRowKey, field: NSTextField)?
    private var a11yElements: [CanvasElement] = []

    public init(model: DrumGridModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// A click the control channel sends is never a window-activating first
    /// click (control spec §5.5).
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
        ControlUI.isChannel(event) || super.acceptsFirstMouse(for: event)
    }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = model.selectedRow
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.observe()
            }
        }
    }

    /// Content height `y` (the canvas's coordinates below its ruler) of a
    /// point in this view.
    func contentY(_ viewY: CGFloat) -> Double { Double(viewY - EditorCanvas.rulerHeight + verticalOffset) }

    /// A band's rect in this view.
    func rect(_ b: DrumBandRow) -> CGRect {
        CGRect(
            x: 0, y: EditorCanvas.rulerHeight + CGFloat(b.top) - verticalOffset, width: bounds.width,
            height: CGFloat(b.height))
    }

    public override func draw(_ dirty: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        let lanes = model.lanes
        for b in model.layout.bands {
            let r = rect(b)
            guard r.maxY >= dirty.minY, r.minY <= dirty.maxY else { continue }
            switch b.kind {
            case let .header(lane):
                guard lanes.indices.contains(Int(lane)) else { continue }
                let l = lanes[Int(lane)]
                NSString(string: "\(l.name) · \(l.destination ?? "No Destination")").draw(
                    in: r.insetBy(dx: 6, dy: 3),
                    withAttributes: [.font: NSFont.systemFont(ofSize: 12, weight: .semibold)])
            case let .row(lane, row):
                guard let d = model.row(Int(lane), Int(row)) else { continue }
                if model.selectedRow == DrumRowKey(lane: lanes[Int(lane)].lane, row: d.id) {
                    NSColor.controlAccentColor.withAlphaComponent(0.2).setFill()
                    r.fill()
                }
                // A row with a destination of its own names it (Ruling 12).
                let name = d.dest == nil ? d.name : "\(d.name) · \(d.destination ?? "?")"
                NSString(string: name).draw(
                    in: CGRect(x: 10, y: r.minY + 3, width: r.width - 50, height: 16),
                    withAttributes: [.font: NSFont.systemFont(ofSize: 11)])
                NSString(string: PitchName.name(d.note)).draw(
                    in: CGRect(x: r.width - 38, y: r.minY + 3, width: 34, height: 16),
                    withAttributes: [
                        .font: NSFont.monospacedDigitSystemFont(ofSize: 10, weight: .regular),
                        .foregroundColor: NSColor.secondaryLabelColor,
                    ])
            case .newLane:
                continue
            }
            NSColor.separatorColor.setFill()
            CGRect(x: 0, y: r.maxY - 0.5, width: bounds.width, height: 1).fill()
        }
        if let d = dragging, d.moved {
            NSColor.controlAccentColor.setFill()
            CGRect(x: 0, y: d.y - 1, width: bounds.width, height: 2).fill()
        }
        NSColor.windowBackgroundColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: EditorCanvas.rulerHeight).fill()
        NSColor.separatorColor.setFill()
        NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill()
    }

    // MARK: Mouse

    private func press(at viewY: CGFloat) -> DrumPress? {
        guard case let .row(lane, row, _) = model.spot(contentY(viewY)) else { return nil }
        return DrumPress(lane: Int(lane), row: Int(row))
    }

    public override func mouseDown(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        mouseDown(at: p, clicks: e.clickCount)
    }

    /// A press at `p` (view coordinates); tests call it directly.
    func mouseDown(at p: CGPoint, clicks: Int) {
        model.becameActive()
        guard let press = press(at: p.y) else { return }
        model.select(lane: press.lane, row: press.row)
        if clicks >= 2 {
            beginRename(press)
        } else {
            dragging = (press, p.y, false)
        }
    }

    public override func mouseDragged(with e: NSEvent) {
        mouseDragged(to: convert(e.locationInWindow, from: nil))
    }

    func mouseDragged(to p: CGPoint) {
        guard var d = dragging else { return }
        d.moved = d.moved || abs(p.y - d.y) > ToolMachine<Int>.deadZone
        d.y = p.y
        dragging = d
        needsDisplay = true
    }

    public override func mouseUp(with e: NSEvent) {
        mouseUp(at: convert(e.locationInWindow, from: nil))
    }

    func mouseUp(at p: CGPoint) {
        guard let d = dragging else { return }
        dragging = nil
        needsDisplay = true
        guard d.moved else { return }
        model.drop(lane: d.press.lane, row: d.press.row, atY: contentY(p.y))
    }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53, dragging != nil {  // Escape
            dragging = nil
            needsDisplay = true
        } else {
            super.keyDown(with: e)
        }
    }

    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = convert(event.locationInWindow, from: nil)
        guard let press = press(at: p.y) else { return nil }
        model.select(lane: press.lane, row: press.row)
        return rowMenu(press)
    }

    /// Rename, Show Velocity, Split to Own Lane, Remove Row.
    func rowMenu(_ press: DrumPress) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let shown = model.row(press.lane, press.row)?.showVelocity ?? false
        menu.addItem(MenuAction(title: "Rename…") { [weak self] in self?.beginRename(press) }.item(checked: false))
        menu.addItem(
            MenuAction(title: "Show Velocity") { [weak self] in self?.model.showVelocity(!shown) }.item(checked: shown))
        menu.addItem(.separator())
        menu.addItem(
            MenuAction(title: "Split Row to Own Lane") { [weak self] in self?.model.splitSelectedRow() }.item(
                checked: false))
        menu.addItem(
            MenuAction(title: "Remove Row") { [weak self] in self?.model.removeSelectedRow() }.item(checked: false))
        return menu
    }

    // MARK: Renaming

    func beginRename(_ press: DrumPress) {
        renaming?.field.removeFromSuperview()
        guard let r = model.row(press.lane, press.row),
            let b = model.layout.bands.first(where: { $0.kind == .row(lane: UInt32(press.lane), row: UInt32(press.row)) })
        else { return }
        let field = NSTextField(string: r.name)
        let band = rect(b)
        field.frame = CGRect(x: 8, y: band.minY + 1, width: band.width - 50, height: 20)
        field.delegate = self
        addSubview(field)
        window?.makeFirstResponder(field)
        renaming = (DrumRowKey(lane: model.lanes[press.lane].lane, row: r.id), field)
        editor.beginTextEditing("drumRow")
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let r = renaming else { return }
        renaming = nil
        editor.endTextEditing("drumRow")
        r.field.removeFromSuperview()
        model.rename(r.key, to: r.field.stringValue)
    }

    /// Torn down mid-rename: give the keyboard back to the transport keys.
    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window == nil, renaming != nil {
            renaming = nil
            editor.endTextEditing("drumRow")
        }
    }

    public override func scrollWheel(with e: NSEvent) {
        nextResponder?.scrollWheel(with: e)
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Drum Rows" }

    public override func accessibilityChildren() -> [Any]? {
        let lanes = model.lanes
        var items: [(item: CanvasAccessibilityItem, frame: CGRect)] = []
        for b in model.layout.bands {
            guard case let .row(lane, row) = b.kind, let d = model.row(Int(lane), Int(row)) else { continue }
            let r = rect(b)
            guard r.maxY > EditorCanvas.rulerHeight, r.minY < bounds.height else { continue }
            let press = DrumPress(lane: Int(lane), row: Int(row))
            let selected = model.selectedRow == DrumRowKey(lane: lanes[Int(lane)].lane, row: d.id)
            let label =
                "\(d.name), \(PitchName.name(d.note)), \(lanes[Int(lane)].name), \(d.destination ?? "no destination")"
            items.append(
                (
                    CanvasAccessibilityItem(
                        label: label, frame: r, selected: selected,
                        press: { [weak self] in self?.model.select(lane: press.lane, row: press.row) }), r
                ))
        }
        a11yElements = CanvasElement.reuse(a11yElements, for: items, parent: self)
        return a11yElements
    }
}

/// The drum grid's views: the row headers beside the hits, sharing one
/// vertical scroll.
@MainActor
public final class DrumGridContainer: NSView {
    public let model: DrumGridModel
    public let canvas: DrumGridView
    public let headers: DrumRowHeaderView

    public init(model: DrumGridModel) {
        self.model = model
        canvas = DrumGridView(model: model)
        headers = DrumRowHeaderView(model: model)
        super.init(frame: .zero)
        addSubview(headers)
        addSubview(canvas)
        canvas.onVerticalScroll = { [weak self] y in self?.headers.verticalOffset = y }
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    public override func layout() {
        super.layout()
        let w = DrumRowHeaderView.width
        headers.frame = NSRect(x: 0, y: 0, width: w, height: bounds.height)
        canvas.frame = NSRect(x: w, y: 0, width: max(bounds.width - w, 0), height: bounds.height)
    }

    public override func scrollWheel(with e: NSEvent) { canvas.scrollWheel(with: e) }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter DrumGridViewTests 2>&1 | tail -1
```

Expected: `Test run with 6 tests in 1 suite passed`. At 96 ticks per point a sixteenth is 40 pt: x 85 is tick 8 160, in the third cell (7 680); content y 50 is the Snare row (42–62).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAWEditors/DrumGridView.swift app/Sources/VizDAWEditors/DrumRowHeaderView.swift \
  app/Tests/VizDAWAppModelTests/DrumGridViewTests.swift
git commit -m "feat(editors): DrumGridView and the drum row headers, with VoiceOver elements

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: Swift 596 (590 + 6).

---

### Task 7: App — the drum grid in the detail pane and torn windows, its bar, and the lane menu's Drum Lane

**Files:**
- Create: `app/Sources/VizDAW/DrumGridPane.swift`
- Modify: `app/Sources/VizDAW/DetailPane.swift`, `app/Sources/VizDAW/EditorWindows.swift`, `app/Sources/VizDAWEditors/LaneHeaderView.swift`
- Test: `app/Tests/VizDAWAppModelTests/ArrangementViewTests.swift`

**Interfaces:**
- Consumes: Tasks 5–6; `DetailPane`'s `slot`, `EditorWindows.show`'s close closure, `LaneHeaderView.{row(at:), document, editor}`, `MenuAction`.
- Produces: `DrumGridEditor(model:)`, `DrumGridHost`, `DrumGridBar` (SwiftUI, `VizDAW` target); `LaneHeaderView.laneMenu(seq:lane:) -> NSMenu` and its `menu(for:)`.

Rulings 2, 3, 10, 11. The bar is one line that fits the detail pane of a default 980-pt window beside the sidebar (a wider bar made SwiftUI grow the window): Paint/Erase (the shared tool: Erase is the eraser, Paint the pencil), the view, Step, Spacing and Length pickers (labels in tooltips and accessibility), "Vel n", an add-row button, and a lane menu titled "Drums: 1/8 Swing 66%" holding the play-quantize choices (the current one checked) and Nudge Earlier/Later under a "Nudge: +240 ticks" heading. A sequence without drum lanes shows "No Drum Lanes" and how to make one. The pane's old "arrives in plan 4e" placeholder goes. A torn window's close drops its drum grid like its other editors.

- [ ] **Step 1: Write the failing test**

In `app/Tests/VizDAWAppModelTests/ArrangementViewTests.swift`:

Find:

```swift
        #expect(m.editor.status == "no destinations")
    }

    @Test func aRenameEndsOnTheLaneItBeganOn() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
```

Replace with:

```swift
        #expect(m.editor.status == "no destinations")
    }

    @Test func theLaneMenuMarksAndUnmarksADrumLane() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let h = LaneHeaderView(document: m)
        var lane = m.lanes[1]
        let item = try #require(h.laneMenu(seq: 3, lane: lane).items.first)
        #expect(item.title == "Drum Lane" && item.state == .off)
        _ = item.target?.perform(item.action)
        #expect(f.edits.last == .drum(edit: .markLane(seq: 3, lane: 31, drum: true)))
        lane.isDrum = true
        let on = try #require(h.laneMenu(seq: 3, lane: lane).items.first)
        #expect(on.state == .on)
        _ = on.target?.perform(on.action)
        #expect(f.edits.last == .drum(edit: .markLane(seq: 3, lane: 31, drum: false)))
    }

    @Test func aRenameEndsOnTheLaneItBeganOn() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
```

- [ ] **Step 2: Run it to see it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build --build-tests 2>&1 | grep -E "error:" | head -3
```

Expected: FAIL to compile; the error names `laneMenu`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWEditors/LaneHeaderView.swift`:

Find:

```swift
        }
    }

    private func flags(_ seq: UInt64, _ lane: UInt64, mute: Bool? = nil, solo: Bool? = nil, arm: Bool? = nil) {
        editor.edit(.lane(edit: .setFlags(seq: seq, lane: lane, mute: mute, solo: solo, arm: arm)))
    }
```

Replace with:

```swift
        }
    }

    /// Right-click or Control-click on a lane: its menu.
    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = convert(event.locationInWindow, from: nil)
        guard let seq = document.selectedSeq, let i = row(at: p.y), document.lanes.indices.contains(i) else {
            return nil
        }
        return laneMenu(seq: seq, lane: document.lanes[i])
    }

    /// Drum Lane: marks the lane a drum lane (rows for its notes) or a
    /// normal lane again (Plan 4e Ruling 3).
    func laneMenu(seq: UInt64, lane: LaneRow) -> NSMenu {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let drum = MenuAction(title: "Drum Lane") { [weak self] in
            self?.editor.markDrumLane(seq: seq, lane: lane.id, on: !lane.isDrum)
        }
        menu.addItem(drum.item(checked: lane.isDrum))
        return menu
    }

    private func flags(_ seq: UInt64, _ lane: UInt64, mute: Bool? = nil, solo: Bool? = nil, arm: Bool? = nil) {
        editor.edit(.lane(edit: .setFlags(seq: seq, lane: lane, mute: mute, solo: solo, arm: arm)))
    }
```

Create `app/Sources/VizDAW/DrumGridPane.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAW
cat > app/Sources/VizDAW/DrumGridPane.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The drum grid (spec §4e) with its bar; a sequence without drum lanes
/// says how to make one.
struct DrumGridEditor: View {
    let model: DrumGridModel

    var body: some View {
        // Observed, so the empty state follows edits and undo.
        let _ = model.editor.drawVersion
        VStack(spacing: 0) {
            DrumGridBar(model: model, editor: model.editor)
            Divider()
            if model.lanes.isEmpty {
                ContentUnavailableView(
                    "No Drum Lanes", systemImage: "square.grid.3x3",
                    description: Text(
                        "Make a lane a drum lane with Drum > Drum Lane or its header's menu, or add a Drum Kit sequence."))
            } else {
                DrumGridHost(model: model)
            }
        }
    }
}

struct DrumGridHost: NSViewRepresentable {
    let model: DrumGridModel

    func makeNSView(context: Context) -> DrumGridContainer { DrumGridContainer(model: model) }

    func updateNSView(_ view: DrumGridContainer, context: Context) {}
}

/// Tool, view, step, spacing, length, velocity, Add Row, and a menu with
/// the current lane's play quantize and nudge (Plan 4e Rulings 10, 11):
/// one line that fits the detail pane of a default-sized window.
struct DrumGridBar: View {
    let model: DrumGridModel
    @Bindable var editor: EditorModel

    private func binding<T>(_ key: WritableKeyPath<DrumGridSettings, T>) -> Binding<T> {
        Binding(
            get: { editor.layout.drumGrid[keyPath: key] },
            set: { v in editor.updateLayout { $0.drumGrid[keyPath: key] = v } })
    }

    private func lengths(_ title: String, _ key: WritableKeyPath<DrumGridSettings, Int64>, help: String) -> some View {
        Picker(title, selection: binding(key)) {
            ForEach(DrumGridSettings.lengths(including: editor.layout.drumGrid[keyPath: key]), id: \.1) {
                Text($0.0).tag($0.1)
            }
        }
        .labelsHidden()
        .frame(width: 70)
        .help(help)
        .accessibilityLabel(title)
    }

    private var erasing: Binding<Bool> {
        Binding(get: { editor.tool == .eraser }, set: { editor.tool = $0 ? .eraser : .pencil })
    }

    var body: some View {
        let s = editor.layout.drumGrid
        HStack(spacing: 6) {
            Picker("Tool", selection: erasing) {
                Text("Paint").tag(false)
                Text("Erase").tag(true)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 100)
            .accessibilityLabel("Tool")
            Picker("View", selection: binding(\.view)) {
                ForEach(DrumView.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .frame(width: 100)
            .accessibilityLabel("View")
            lengths("Step", \.step, help: "Step: the width of a step matrix cell")
            lengths("Spacing", \.spacing, help: "Spacing: the distance between the hits a drag paints")
            lengths("Length", \.duration, help: "Length: the length of a painted hit")
            Stepper(
                "Vel \(s.velocity)",
                onIncrement: { editor.updateLayout { $0.drumGrid.velocity = min($0.drumGrid.velocity + 4, 127) } },
                onDecrement: { editor.updateLayout { $0.drumGrid.velocity = max($0.drumGrid.velocity, 5) - 4 } }
            )
            .help("The velocity of painted hits in rows whose velocity lane is hidden")
            .accessibilityLabel("Velocity")
            Button {
                model.addRow()
            } label: {
                Image(systemName: "plus")
            }
            .help("Add Drum Row")
            .accessibilityLabel("Add Row")
            Spacer(minLength: 0)
            if let lane = model.currentLane {
                laneMenu(lane)
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }

    /// Play quantize choices (the current one checked) and nudge.
    private func laneMenu(_ lane: DrumLaneRow) -> some View {
        let title = model.quantizeTitle(lane)
        let shift = lane.playShift
        return Menu("\(lane.name): \(title)") {
            Section("Play Quantize") {
                ForEach(DrumGridModel.quantizeChoices, id: \.title) { c in
                    Toggle(
                        c.title,
                        isOn: Binding(
                            get: { c.title == title }, set: { _ in model.setQuantize(lane: lane.lane, c.quantize) }))
                }
            }
            Section("Nudge: \(shift > 0 ? "+" : "")\(shift) ticks") {
                Button("Nudge Earlier") { model.nudge(lane: lane.lane, steps: -1) }
                Button("Nudge Later") { model.nudge(lane: lane.lane, steps: 1) }
            }
        }
        .frame(width: 160)
        .help("The lane's play quantize (grid or swing) and nudge, applied as it plays")
        .accessibilityLabel("Lane")
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/DetailPane.swift`:

Find:

```swift

/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and the editor: the piano roll (4b), the list (4c), the Strip Chart
/// (4d), else a placeholder.
struct DetailPane: View {
    let model: DocumentModel
    let state: DetailState
```

Replace with:

```swift

/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and the editor: the piano roll (4b), the list (4c), the Strip Chart
/// (4d) or the drum grid (4e).
struct DetailPane: View {
    let model: DocumentModel
    let state: DetailState
```

Find:

```swift
            } else if state.kind == .stripChart {
                StripChartEditor(model: model.stripChart(slot))
            } else {
                ContentUnavailableView(
                    "\(state.kind.title) arrives in plan \(state.kind.plan)",
                    systemImage: "rectangle.and.pencil.and.ellipsis",
                    description: Text(summary))
            }
        }
    }

    private var summary: String {
        guard let n = model.editor.eventCount(state.target) else {
            return "Showing the whole sequence."
        }
        return "\(n) event\(n == 1 ? "" : "s") in this \(state.target.lane == nil ? "sequence" : "selection")."
    }
}

/// The lower half of the document window (spec §3.7).
```

Replace with:

```swift
            } else if state.kind == .stripChart {
                StripChartEditor(model: model.stripChart(slot))
            } else {
                DrumGridEditor(model: model.drumGrid(slot))
            }
        }
    }
}

/// The lower half of the document window (spec §3.7).
```

In `app/Sources/VizDAW/EditorWindows.swift`:

Find:

```swift
            if byUser { model.editor.closeWindow(window.id) }
            model.dropPianoRoll(.window(window.id))
            model.dropStripChart(.window(window.id))
            // Every close (by the user, pruned, with the document) drops its list.
            model.dropList(.window(window.id))
            // Called from windowWillClose: dropping the last strong reference to the
```

Replace with:

```swift
            if byUser { model.editor.closeWindow(window.id) }
            model.dropPianoRoll(.window(window.id))
            model.dropStripChart(.window(window.id))
            model.dropDrumGrid(.window(window.id))
            // Every close (by the user, pruned, with the document) drops its list.
            model.dropList(.window(window.id))
            // Called from windowWillClose: dropping the last strong reference to the
```

- [ ] **Step 4: Run the tests and build the app**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter ArrangementViewTests 2>&1 | tail -1
scripts/make-app.sh 2>&1 | tail -1
```

Expected: the arrangement tests pass; `built dist/VizDAW.app`.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAW/DrumGridPane.swift app/Sources/VizDAW/DetailPane.swift app/Sources/VizDAW/EditorWindows.swift \
  app/Sources/VizDAWEditors/LaneHeaderView.swift app/Tests/VizDAWAppModelTests/ArrangementViewTests.swift
git commit -m "feat(app): the drum grid in the detail pane and torn windows; Drum Lane in the lane menu

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: Swift 597 (596 + 1). The app-target views have no unit tests (the executable target cannot be imported); README steps 87–96 check them by eye and Task 10's e2e drives a torn grid.

---

### Task 8: The Drum menu — six registry commands, one implementation for the menu and the channel

**Files:**
- Create: `app/Sources/VizDAWAppModel/DrumCommands.swift`
- Modify: `crates/core-control/src/registry.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAW/AppCommands.swift`
- Test: `crates/core-control/tests/commands.rs`, `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`

**Interfaces:**
- Consumes: Task 5's `DocumentModel.{drumGridForCommands(), selectedLane, selectedSeq}`, `DrumGridModel.{currentLane, selected, addRow, showVelocity, splitSelectedRow, removeSelectedRow}`, `EditorModel.markDrumLane`; `registry::{cmd, CommandSpec, ON}`; `ControlCommands.{handled, run, unavailable}`; `controlCommandIds()`.
- Produces: registry IDs `drum.mark_lane` (param `on`), `drum.add_row`, `drum.show_velocity` (`on`), `drum.split_row`, `drum.remove_row`, `drum.timeline` (`on`), menu "Drum", no shortcuts, no dialogs; `DrumOutcome {done, unavailable(String), refused(String)}`; `DrumCommands.{markLane, addRow, showVelocity, splitRow, removeRow, timeline, ids, enabled(_:_:), isOn(_:_:), run(_:on:_:)}`.

Ruling 13. `ControlCommands.handled` gains `DrumCommands.ids`, and `run` maps `unavailable` to the channel's `unavailable` and a core refusal to `failed` with the status line's text. The app's `CommandMenu("Drum")` sits between Sequence and Transport: toggles for Drum Lane, Show Velocity and Timeline View, items for Add Drum Row, Split Row to Own Lane and Remove Drum Row, each disabled when `DrumCommands.enabled` says so.

- [ ] **Step 1: Write the failing tests**

In `crates/core-control/tests/commands.rs`:

Find:

```rust
    );
}

#[test]
fn the_registry_has_unique_ids_and_answers_dialogs_by_parameter() {
    let ids: std::collections::BTreeSet<_> = COMMANDS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(COMMANDS.len(), 38);
    for c in COMMANDS {
        assert!(
            c.id.contains('.')
```

Replace with:

```rust
    );
}

#[test]
fn the_drum_menu_commands_are_registered() {
    let drum: Vec<(&str, &str, bool)> = COMMANDS
        .iter()
        .filter(|c| c.menu == "Drum")
        .map(|c| (c.id, c.title, !c.params.is_empty()))
        .collect();
    assert_eq!(
        drum,
        vec![
            ("drum.mark_lane", "Drum Lane", true),
            ("drum.add_row", "Add Drum Row", false),
            ("drum.show_velocity", "Show Velocity", true),
            ("drum.split_row", "Split Row to Own Lane", false),
            ("drum.remove_row", "Remove Drum Row", false),
            ("drum.timeline", "Timeline View", true),
        ]
    );
    assert!(
        COMMANDS
            .iter()
            .filter(|c| c.menu == "Drum")
            .all(|c| c.document && c.dialogs.is_empty())
    );
    assert_eq!(
        ok(
            "command.run",
            json!({"id": "drum.timeline", "params": {"on": true}})
        ),
        Command::CommandRun {
            id: "drum.timeline".into(),
            args: vec![("on".into(), ArgValue::Flag(true))],
            window: None,
        }
    );
    assert_eq!(
        err(
            "command.run",
            json!({"id": "drum.add_row", "params": {"note": 36}})
        )
        .0,
        ErrorCode::InvalidParams,
        "Add Drum Row takes no parameters"
    );
}

#[test]
fn the_registry_has_unique_ids_and_answers_dialogs_by_parameter() {
    let ids: std::collections::BTreeSet<_> = COMMANDS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        44,
        "38 before plan 4e, plus the Drum menu's six"
    );
    for c in COMMANDS {
        assert!(
            c.id.contains('.')
```

In `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`:

Find:

```swift
        #expect(Set(controlCommandIds()) == ControlCommands.handled)
    }

    @Test func playStopPlaysAndStopsTheSelectedSequence() throws {
        let (s, id, m, f) = try demo()
        #expect(m.selectedSeq == 3)
```

Replace with:

```swift
        #expect(Set(controlCommandIds()) == ControlCommands.handled)
    }

    @Test func theDrumMenuRunsOnTheSelectedLaneAndTheDrumGrid() throws {
        let f = drumFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        // No selected lane, no drum grid: unavailable, nothing sent.
        #expect(failure(run(DrumCommands.markLane, s, id))?.code == .unavailable)
        #expect(failure(run(DrumCommands.addRow, s, id))?.message == "Open a drum grid first.")
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 30)], events: []))
        _ = run(DrumCommands.markLane, [ControlArg(name: "on", value: .flag(value: true))], s, id)
        #expect(f.edits.last == .drum(edit: .markLane(seq: 3, lane: 30, drum: true)))
        m.editor.openDetail(.lane(seq: 3, lane: 40), kind: .drumGrid)
        _ = run(DrumCommands.addRow, s, id)
        #expect(f.edits.last == .drum(edit: .addRow(seq: 3, lane: 40, name: "Closed Hat", note: 42, dest: nil)))
        // Row commands need a selected row.
        m.drumGrid(.pane).selectedRow = nil
        #expect(failure(run(DrumCommands.splitRow, s, id))?.message == DrumGridModel.noRowMessage)
        m.drumGrid(.pane).select(lane: 0, row: 1)
        _ = run(DrumCommands.showVelocity, s, id)
        #expect(f.edits.last == .drum(edit: .showVelocity(seq: 3, lane: 40, row: 2, show: true)))
        _ = run(DrumCommands.splitRow, s, id)
        #expect(f.edits.last == .drum(edit: .splitRow(seq: 3, lane: 40, row: 2)))
        _ = run(DrumCommands.removeRow, s, id)
        #expect(f.edits.last == .drum(edit: .removeRow(seq: 3, lane: 40, row: 2)))
        // Timeline View is a setting: on, then toggled back.
        _ = run(DrumCommands.timeline, [ControlArg(name: "on", value: .flag(value: true))], s, id)
        #expect(m.editor.layout.drumGrid.view == .timeline)
        #expect(DrumCommands.isOn(DrumCommands.timeline, m))
        _ = run(DrumCommands.timeline, s, id)
        #expect(m.editor.layout.drumGrid.view == .matrix)
        // A refusal is the core's message.
        f.editError = .Model(message: "no")
        #expect(failure(run(DrumCommands.removeRow, s, id)) == .of(.failed, "no"))
    }

    @Test func playStopPlaysAndStopsTheSelectedSequence() throws {
        let (s, id, m, f) = try demo()
        #expect(m.selectedSeq == 3)
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | grep -E "panicked|test result" | head -3
```

Expected: FAIL: `the_drum_menu_commands_are_registered` (an empty list) and the count (38, expected 44).

- [ ] **Step 3: Implement**

In `crates/core-control/src/registry.rs` (before `transport.play_stop`, so `COMMANDS` stays in menu order):

Find:

```rust
        Some("⌘["),
        true,
    ),
    cmd(
        "transport.play_stop",
        "Transport",
```

Replace with:

```rust
        Some("⌘["),
        true,
    ),
    CommandSpec {
        params: ON,
        ..cmd("drum.mark_lane", "Drum", "Drum Lane", None, true)
    },
    cmd("drum.add_row", "Drum", "Add Drum Row", None, true),
    CommandSpec {
        params: ON,
        ..cmd("drum.show_velocity", "Drum", "Show Velocity", None, true)
    },
    cmd(
        "drum.split_row",
        "Drum",
        "Split Row to Own Lane",
        None,
        true,
    ),
    cmd("drum.remove_row", "Drum", "Remove Drum Row", None, true),
    CommandSpec {
        params: ON,
        ..cmd("drum.timeline", "Drum", "Timeline View", None, true)
    },
    cmd(
        "transport.play_stop",
        "Transport",
```

Create `app/Sources/VizDAWAppModel/DrumCommands.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/DrumCommands.swift <<'VIZDAW_EOF'
import VizDAWCore

/// What a Drum menu command came to.
public enum DrumOutcome: Equatable, Sendable {
    case done
    /// The menu item is disabled: why.
    case unavailable(String)
    /// The core refused it: the status line's message.
    case refused(String)
}

/// The Drum menu (Plan 4e Ruling 13): one implementation for the menu
/// items and the control channel's `command.run`.
@MainActor
public enum DrumCommands {
    public static let markLane = "drum.mark_lane"
    public static let addRow = "drum.add_row"
    public static let showVelocity = "drum.show_velocity"
    public static let splitRow = "drum.split_row"
    public static let removeRow = "drum.remove_row"
    public static let timeline = "drum.timeline"

    public static let ids: Set<String> = [markLane, addRow, showVelocity, splitRow, removeRow, timeline]

    static let noLane = "Select a lane in the arrangement first."
    static let noGrid = "Open a drum grid first."

    /// Whether `id` can run on `m` now (its menu item is enabled).
    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        switch id {
        case markLane: m.selectedLane != nil
        case addRow: m.drumGridForCommands()?.currentLane != nil
        case showVelocity, splitRow, removeRow: m.drumGridForCommands()?.selected != nil
        case timeline: true
        default: false
        }
    }

    /// A toggle's state (its menu item's check mark).
    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool {
        switch id {
        case markLane: m.selectedLane?.isDrum ?? false
        case showVelocity: m.drumGridForCommands()?.selected?.row.showVelocity ?? false
        case timeline: m.editor.layout.drumGrid.view == .timeline
        default: false
        }
    }

    /// Runs `id` on `m`; `on` sets a toggle (nil flips it).
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> DrumOutcome {
        if id == timeline {
            let want = on ?? !isOn(timeline, m)
            m.editor.updateLayout { $0.drumGrid.view = want ? .timeline : .matrix }
            return .done
        }
        if id == markLane {
            guard let seq = m.selectedSeq, let lane = m.selectedLane else { return .unavailable(noLane) }
            let want = on ?? !lane.isDrum
            guard want != lane.isDrum else { return .done }
            return m.editor.markDrumLane(seq: seq, lane: lane.id, on: want) ? .done : refused(m)
        }
        guard let g = m.drumGridForCommands() else { return .unavailable(noGrid) }
        if id == addRow {
            guard g.currentLane != nil else { return .unavailable("The drum grid shows no drum lane.") }
            return g.addRow() ? .done : refused(m)
        }
        guard g.selected != nil else { return .unavailable(DrumGridModel.noRowMessage) }
        let ok: Bool
        switch id {
        case showVelocity: ok = g.showVelocity(on)
        case splitRow: ok = g.splitSelectedRow()
        case removeRow: ok = g.removeSelectedRow()
        default: return .unavailable("VizDAW has no handler for \(id).")
        }
        return ok ? .done : refused(m)
    }

    private static func refused(_ m: DocumentModel) -> DrumOutcome {
        .refused(m.editor.status ?? "The drum command was refused.")
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:

```swift

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
```

Replace with:

```swift

    /// Every registry ID `run` handles (a test checks it is all of them).
    public static var handled: Set<String> {
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
    }

    /// Runs command `id` with `args` on `doc` (the target window's document,
```

Find:

```swift
        case "transport.return_to_start":
            guard m.returnKeyEnabled else { return unavailable("A text field or list has the keyboard.") }
            m.returnToStart()
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
```

Replace with:

```swift
        case "transport.return_to_start":
            guard m.returnKeyEnabled else { return unavailable("A text field or list has the keyboard.") }
            m.returnToStart()
        case _ where DrumCommands.ids.contains(id):
            switch DrumCommands.run(id, on: args.flag("on"), m) {
            case .done: break
            case .unavailable(let why): return unavailable(why)
            case .refused(let why): return .failed(.of(.failed, why))
            }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
```

In `app/Sources/VizDAW/AppCommands.swift`:

Find:

```swift
                .keyboardShortcut("[", modifiers: [.command])
                .disabled(doc?.editor.navigation.isEmpty ?? true)
        }
        CommandMenu("Transport") {
            Button(doc?.isPlaying == true ? "Stop" : "Play") { doc?.playStop() }
                .keyboardShortcut(.space, modifiers: [])
```

Replace with:

```swift
                .keyboardShortcut("[", modifiers: [.command])
                .disabled(doc?.editor.navigation.isEmpty ?? true)
        }
        CommandMenu("Drum") {
            drumToggle("Drum Lane", DrumCommands.markLane)
            Divider()
            drumItem("Add Drum Row", DrumCommands.addRow)
            drumToggle("Show Velocity", DrumCommands.showVelocity)
            drumItem("Split Row to Own Lane", DrumCommands.splitRow)
            drumItem("Remove Drum Row", DrumCommands.removeRow)
            Divider()
            drumToggle("Timeline View", DrumCommands.timeline)
        }
        CommandMenu("Transport") {
            Button(doc?.isPlaying == true ? "Stop" : "Play") { doc?.playStop() }
                .keyboardShortcut(.space, modifiers: [])
```

Find:

```swift
                doc == nil || !(doc?.transportKeysEnabled ?? false) || (doc?.editor.selection.events.isEmpty ?? true))
    }

    /// A Modify menu command: the panel set to its task (Plan 4c Ruling 10).
    private func panelItem(_ task: PanelTask, _ key: Character) -> some View {
        Button("\(task.title)…") { doc?.selectModify.open(task) }
```

Replace with:

```swift
                doc == nil || !(doc?.transportKeysEnabled ?? false) || (doc?.editor.selection.events.isEmpty ?? true))
    }

    /// A Drum menu command (Plan 4e Ruling 13); a refusal shows in the
    /// status line like any editor command.
    private func drumItem(_ title: String, _ id: String) -> some View {
        Button(title) { if let doc { DrumCommands.run(id, doc) } }
            .disabled(doc.map { !DrumCommands.enabled(id, $0) } ?? true)
    }

    private func drumToggle(_ title: String, _ id: String) -> some View {
        Toggle(
            title,
            isOn: Binding(
                get: { doc.map { DrumCommands.isOn(id, $0) } ?? false },
                set: { on in if let doc { DrumCommands.run(id, on: on, doc) } })
        )
        .disabled(doc.map { !DrumCommands.enabled(id, $0) } ?? true)
    }

    /// A Modify menu command: the panel set to its task (Plan 4c Ruling 10).
    private func panelItem(_ task: PanelTask, _ key: Character) -> some View {
        Button("\(task.title)…") { doc?.selectModify.open(task) }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | grep -E "^test result|FAILED|panicked"
scripts/build-ffi.sh && scripts/swift.sh test --filter ControlCommandsTests 2>&1 | tail -1
```

Expected: `commands` 13 passed; the Swift suite passes, including `everyRegisteredMenuCommandHasAHandler` (Rust's 44 IDs equal `handled`).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs app/Sources/VizDAWAppModel/DrumCommands.swift \
  app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAW/AppCommands.swift \
  app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift
git commit -m "feat(control): the Drum menu in the registry, the app and the channel

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 859 Rust tests listed (858 + 1); Swift 598 (597 + 1).

---

### Task 9: Channel clicks reach the canvases of a background VizDAW

**Files:**
- Modify: `app/Sources/VizDAWEditors/ControlUI.swift`
- Test: `app/Tests/VizDAWEditorsTests/ControlUITests.swift`

**Interfaces:**
- Consumes: `ControlUI.{clickEvents, post, isChannel, channelEventBase}`.
- Produces: `ControlUI.remember(_:)` (internal), `ControlUI.postedLimit` (256); `isChannel` true for an event whose timestamp is one the channel posted.

Ruling 14, found while planning: `NSApp.postEvent` hands AppKit a renumbered copy (the channel's number 1 073 741 825 came back as 1), so `isChannel` was false for every posted click, `acceptsFirstMouse` said no, and AppKit spent each single click on a canvas of the (never key) background window on activating it; double-clicks got through. The event's timestamp survives the queue, so `post` remembers the newest 256 and `isChannel` matches them within a microsecond (a user's click never shares one). Toolbar buttons were not affected (they take first clicks themselves), which is why the control plan's e2e passed.

- [ ] **Step 1: Write the failing test**

In `app/Tests/VizDAWEditorsTests/ControlUITests.swift`:

Find:

```swift
        #expect(!c.acceptsFirstMouse(for: nil))
    }

    @Test func aClickPointNeedsAVisibleFiniteElement() throws {
        let (w, _, _) = window()
        #expect(ControlUI.point(.point(x: .nan, y: 3), in: w) == nil)
```

Replace with:

```swift
        #expect(!c.acceptsFirstMouse(for: nil))
    }

    @Test func aPostedEventRenumberedByAppKitIsStillTheChannels() throws {
        let (w, c, _) = window()
        let channel = ControlUI.clickEvents(w, at: CGPoint(x: 5, y: 5), button: .left, count: 1, modifiers: none)
        ControlUI.remember(channel)
        // What AppKit dispatches after `postEvent`: same timestamp, a small number.
        let dispatched = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: NSPoint(x: 5, y: 5), modifierFlags: [],
                timestamp: channel[0].timestamp, windowNumber: w.windowNumber, context: nil, eventNumber: 1,
                clickCount: 1, pressure: 1))
        #expect(ControlUI.isChannel(dispatched))
        #expect(c.acceptsFirstMouse(for: dispatched))
        let user = try #require(
            NSEvent.mouseEvent(
                with: .leftMouseDown, location: NSPoint(x: 5, y: 5), modifierFlags: [],
                timestamp: channel[0].timestamp + 0.5, windowNumber: w.windowNumber, context: nil, eventNumber: 2,
                clickCount: 1, pressure: 1))
        #expect(!ControlUI.isChannel(user), "a user's click at another time is not")
        // Only the newest are kept.
        for _ in 0..<(ControlUI.postedLimit / 2 + 1) {
            ControlUI.remember(ControlUI.clickEvents(w, at: .zero, button: .left, count: 1, modifiers: none))
        }
        #expect(!ControlUI.isChannel(dispatched))
    }

    @Test func aClickPointNeedsAVisibleFiniteElement() throws {
        let (w, _, _) = window()
        #expect(ControlUI.point(.point(x: .nan, y: 3), in: w) == nil)
```

- [ ] **Step 2: Run it to see it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build --build-tests 2>&1 | grep -E "error:" | head -3
```

Expected: FAIL to compile; the errors name `remember` and `postedLimit`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWEditors/ControlUI.swift`:

Find:

```swift
    /// small counters. That is how the editors tell them apart.
    static let channelEventBase = 1 << 30
    private static var eventNumber = channelEventBase

    /// Whether `event` was built by the channel.
    public static func isChannel(_ event: NSEvent?) -> Bool {
        guard let e = event else { return false }
        switch e.type {
        case .leftMouseDown, .leftMouseUp, .leftMouseDragged, .rightMouseDown, .rightMouseUp:
            return e.eventNumber >= channelEventBase
        default: return false
        }
    }
```

Replace with:

```swift
    /// small counters. That is how the editors tell them apart.
    static let channelEventBase = 1 << 30
    private static var eventNumber = channelEventBase
    /// Timestamps of the mouse events the channel posted, newest last (at
    /// most `postedLimit`). `NSApp.postEvent` renumbers an event, so the
    /// event AppKit dispatches no longer has the channel's number; its
    /// timestamp survives (plan 4e Task 9: without this, a click on a
    /// canvas of a background VizDAW was spent on activating the window).
    private static var posted: [TimeInterval] = []
    static let postedLimit = 256

    /// Notes `events` as the channel's (`post` does; tests call it).
    static func remember(_ events: [NSEvent]) {
        posted += events.map(\.timestamp)
        if posted.count > postedLimit { posted.removeFirst(posted.count - postedLimit) }
    }

    /// Whether `event` was built by the channel: its number (an event
    /// handed over directly), or its timestamp (one that went through the
    /// queue).
    public static func isChannel(_ event: NSEvent?) -> Bool {
        guard let e = event else { return false }
        switch e.type {
        case .leftMouseDown, .leftMouseUp, .leftMouseDragged, .rightMouseDown, .rightMouseUp:
            return e.eventNumber >= channelEventBase || posted.contains { abs($0 - e.timestamp) < 1e-6 }
        default: return false
        }
    }
```

Find:

```swift
    /// Orders `window` to the front (without stealing focus from the
    /// active app) and queues `events` behind anything already queued.
    public static func post(_ events: [NSEvent], to window: NSWindow) {
        window.orderFront(nil)
        for e in events { NSApp.postEvent(e, atStart: false) }
    }
```

Replace with:

```swift
    /// Orders `window` to the front (without stealing focus from the
    /// active app) and queues `events` behind anything already queued.
    public static func post(_ events: [NSEvent], to window: NSWindow) {
        remember(events)
        window.orderFront(nil)
        for e in events { NSApp.postEvent(e, atStart: false) }
    }
```

- [ ] **Step 4: Run the tests, then the existing end-to-end tests against a fresh app**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test --filter ControlUITests 2>&1 | tail -1
scripts/make-app.sh 2>&1 | tail -1
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 2>&1 | grep -E "^test |test result"
pgrep -lf MacOS/VizDAW || echo "no VizDAW left"
```

Expected: the suite passes; both existing e2e tests pass; no VizDAW left.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
git add app/Sources/VizDAWEditors/ControlUI.swift app/Tests/VizDAWEditorsTests/ControlUITests.swift
git commit -m "fix(control): channel clicks reach the canvases of a background VizDAW

AppKit renumbers posted events, so the channel's own clicks were not
recognised as first clicks and were spent on activating the window.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: Swift 599 (598 + 1).

---

### Task 10: The drum grid end to end through the control channel

**Files:**
- Modify: `crates/vizctl/Cargo.toml` (dev-dependencies `core-engine` with `fixtures`, `core-model`), `crates/vizctl/tests/e2e.rs`, `Cargo.lock`

**Interfaces:**
- Consumes: Tasks 1–9; the e2e's `App::{start, call, wait, quit}`, `WAIT_MS`; `core_engine::fixtures::{Builder, Q}`; `core_model::file::save`; the channel's `app.open`, `command.run`, `state.get windows/document`, `ui.tree`, `ui.click`.
- Produces: e2e test `drum_grid_paints_and_erases_by_element`; helpers `labelled(nodes, label)`, `wait_for_label(app, window, label)`, `wait_for_undo(app, window, label)`.

Ruling 15. The test writes `Beat.vizdaw` into the throwaway home (one top-level sequence "Beat" whose lane "Drums" has rows Kick and Snare and a kick at bar 1, velocity `0x8000` = 64), opens it, runs `editors.drum_grid_window`, finds the torn window in `state.get windows`, waits for the cells' elements, clicks "Snare, bar 1 beat 1, off" (undo becomes "Paint Drum Hits"), clicks the now lit cell (undo "Erase Drum Hits"), runs `drum.add_row` (a "Closed Hat" row appears) and `drum.timeline` (the timeline lists hits only). Every wait polls with a deadline; there is no fixed sleep beyond 200 ms poll intervals.

- [ ] **Step 1: Write the test**

In `crates/vizctl/Cargo.toml`:

Find:

```toml
serde_json = "1"

[dev-dependencies]
tempfile = "3"
```

Replace with:

```toml
serde_json = "1"

[dev-dependencies]
core-engine = { path = "../core-engine", features = ["fixtures"] }
core-model = { path = "../core-model" }
tempfile = "3"
```

In `crates/vizctl/tests/e2e.rs` (new helpers and the test, before `play_button`):

Find:

```rust
    app.quit();
}

/// The ID of the tree's button labelled (or titled) exactly "Play".
fn play_button(nodes: &Value) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
```

Replace with:

```rust
    app.quit();
}

/// The ID of the first tree node labelled exactly `label`.
fn labelled(nodes: &Value, label: &str) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        if n["label"].as_str() == Some(label) {
            Some(n["id"].clone())
        } else {
            labelled(&n["children"], label)
        }
    })
}

/// Polls the window's tree until a node is labelled exactly `label`.
fn wait_for_label(app: &mut App, window: &Value, label: &str) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(id) = labelled(&tree["nodes"], label) {
            return id;
        }
        assert!(
            Instant::now() < deadline,
            "no {label:?} within {WAIT_MS} ms in {tree}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Polls `state.get document` until its undo label is `label`.
fn wait_for_undo(app: &mut App, window: &Value, label: &str) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let doc = app.call("state.get", json!({"target": "document", "window": window}));
        if doc["undo"].as_str() == Some(label) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "undo is not {label:?} within {WAIT_MS} ms: {doc}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// README steps 87–88 through the channel (plan 4e): a project whose only
/// sequence has a drum lane opens; Open Drum Grid in Window shows its
/// cells as elements; a click on an empty cell paints a hit, a click on
/// it again erases it; Drum > Add Drum Row and Timeline View run by ID.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn drum_grid_paints_and_erases_by_element() {
    use core_engine::fixtures::{Builder, Q};
    let mut app = App::start();
    let mut b = Builder::new();
    let kit = b.dest("Kit", "builtin:synth", 9).unwrap();
    let (seq, drums) = b.sequence("Beat", "Drums", Some(kit)).unwrap();
    b.drum_rows(seq, drums, &[("Kick", 36), ("Snare", 38)])
        .unwrap();
    b.notes(seq, drums, &[(0, 36, 0x8000, Q / 4)]).unwrap();
    let path = app.launched.home.join("Beat.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();

    let window = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(
        &window,
        json!({"condition": "documents_at_least", "count": 1}),
    );
    // In a window of its own, where the rows have room.
    app.call(
        "command.run",
        json!({"id": "editors.drum_grid_window", "window": window}),
    );
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    let window = loop {
        let w = app.call("state.get", json!({"target": "windows"}));
        let torn = w["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["kind"] == "editor")
            .map(|w| w["number"].clone());
        if let Some(n) = torn {
            break n;
        }
        assert!(Instant::now() < deadline, "no drum grid window in {w}");
        std::thread::sleep(Duration::from_millis(200));
    };

    // The Kick's hit at bar 1 is on; the Snare's first cell is off.
    wait_for_label(&mut app, &window, "Kick, bar 1 beat 1, on, velocity 64");
    let cell = wait_for_label(&mut app, &window, "Snare, bar 1 beat 1, off");
    app.call("ui.click", json!({"window": window, "element": cell}));
    wait_for_undo(&mut app, &window, "Paint Drum Hits");
    let hit = wait_for_label(&mut app, &window, "Snare, bar 1 beat 1, on, velocity 100");
    app.call("ui.click", json!({"window": window, "element": hit}));
    wait_for_undo(&mut app, &window, "Erase Drum Hits");

    // The Drum menu by ID: a row for the next kit note, then the timeline.
    app.call(
        "command.run",
        json!({"id": "drum.add_row", "window": window}),
    );
    wait_for_undo(&mut app, &window, "Add Drum Row");
    wait_for_label(&mut app, &window, "Closed Hat, bar 1 beat 1, off");
    app.call(
        "command.run",
        json!({"id": "drum.timeline", "params": {"on": true}, "window": window}),
    );
    wait_for_label(&mut app, &window, "Kick, bar 1 beat 1, on, velocity 64");
    let tree = app.call("ui.tree", json!({"window": window}));
    assert!(
        labelled(&tree["nodes"], "Closed Hat, bar 1 beat 1, off").is_none(),
        "the timeline lists hits only"
    );
    app.quit();
}

/// The ID of the tree's button labelled (or titled) exactly "Play".
fn play_button(nodes: &Value) -> Option<Value> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
```

- [ ] **Step 2: Build the app and run the end-to-end tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/make-app.sh 2>&1 | tail -1
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 2>&1 | grep -E "^test |test result"
pgrep -lf MacOS/VizDAW || echo "no VizDAW left"
```

Expected: 3 passed (`click_play_and_press_space`, `demo_play_screenshot_stop`, `drum_grid_paints_and_erases_by_element`); no VizDAW left. Without Task 9 the new test fails at "Paint Drum Hits" (the click is spent on activation).

- [ ] **Step 3: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
git add Cargo.lock crates/vizctl/Cargo.toml crates/vizctl/tests/e2e.rs
git commit -m "test(vizctl): drive the drum grid end to end

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

Expected: 860 Rust tests listed (859 + 1 ignored e2e test).

---

### Task 11: Manual checklist for the drum grid; the whole-branch gate

**Files:**
- Modify: `app/README.md`

**Interfaces:**
- Consumes: everything above.
- Produces: checklist steps 87–98 under "### Drum grid (plan 4e)".

Spec §6: a manual checklist per phase (drag feel, playhead, tear-off windows, VoiceOver).

- [ ] **Step 1: Append the checklist**

Append to `app/README.md` (after step 86):

```markdown

### Drum grid (plan 4e)

Use the demo project. To reach Beat, double-click Song's first Groove
block, then Groove's Beat block; its lane "Drums" is a drum lane with
rows Kick, Snare and Closed Hat. Snap stays on (quarters) unless a step
says otherwise.

87. Open: click Beat's block and press Cmd-4. The detail pane shows
    "Drum Grid" with a bar (Paint/Erase, Step Matrix, 1/16, 1/16, 1/32,
    Vel 100, +, "Drums: Off") and row headers Kick C2, Snare D2 and
    Closed Hat F#2 under a "Drums" header. Kick lights the cells at beats
    1 and 3, Snare at 2 and 4, Closed Hat every eighth, brighter on the
    beats (louder hits). The ruler, zoom (Cmd-scroll) and playhead work
    as in the other editors.
88. Paint: click an empty Kick cell: it lights; Edit > Undo "Paint Drum
    Hits" takes it back. Click a lit cell: it goes dark ("Erase Drum
    Hits"). Drag along the Snare row: every cell the drag passes lights
    while dragging, and the whole run is one undo step; dragging back
    towards the start shrinks the run; Escape during the drag leaves
    nothing. A drag that wanders into other rows stays on its own row.
89. Settings: Spacing 1/8 makes a drag light every second cell; Length
    1/16 makes new hits longer; Step 1/8 halves the number of cells, and a
    cell lights when any hit starts in it. Timeline shows the hits at
    their true starts and lengths: a click on a hit erases it, a click
    elsewhere paints one at the grid line before the pointer (G toggles
    snap; with snap off, exactly where you click).
90. Erase tool: choose Erase; a drag erases every hit it crosses; a click
    on an empty cell does nothing and adds no undo step.
91. Velocity lane: right-click Snare's header > Show Velocity: the row
    grows a lane with a bar per hit. Painting in that row sets each new
    hit's velocity from the pointer's height (top 127, bottom 1); a run
    dragged up and down gives each hit the velocity where the pointer
    passed it, and the bars and the cells' brightness show it. Vel in the
    bar sets new hits in rows whose velocity lane is hidden.
92. Rows: + adds "Open Hat" (A#2) and selects it. Double-click a row name
    to rename it. Drag a row header up or down to reorder; onto another
    drum lane to move it there; below the last lane ("Drop a row here to
    give it a lane of its own") to give it a new lane named after it,
    its notes going along. The header menu's Split Row to Own Lane does
    the same; Remove Row deletes the row and its notes. Each is one undo
    step, and the arrangement shows the new lane.
93. Drum lanes: in Song, right-click the Chords lane header > Drum Lane:
    Chords becomes a drum lane with a row per pitch, and Cmd-4 on Song
    shows it; Drum Lane again makes it a normal lane (its notes stay).
    Drum > Drum Lane does the same for the selected lane (click its
    block first). A sequence without drum lanes shows "No Drum Lanes".
94. Play quantize and nudge: play Beat (Space) and choose "Drums: Off" >
    1/8 Swing 66%: the hats' offbeats play late (a shuffle) while the
    list editor still shows them on the grid. Nudge Later a few times:
    the drums play slightly behind Groove's bass (play Groove); the menu
    reads "Nudge: +360 ticks". Off and Nudge Earlier undo it; both are
    saved with the project.
95. Clean starts: play Song from the middle of a beat (click in the
    ruler): the first drum hits heard are the ones after the position,
    never the rest of a hit that began earlier. Mute Groove's Drums lane
    while playing and unmute it mid-bar: the drums come back at their
    next hit; the same with solo.
96. Windows and the Drum menu: Option-Cmd-4 opens the grid in its own
    window; Drum > Add Drum Row, Show Velocity, Split Row to Own Lane and
    Remove Drum Row act on the grid you last clicked in (its selected
    row), Drum > Timeline View switches every grid. Save, close and
    reopen: the bar's settings and the window come back.
97. VoiceOver (Cmd-F5): the grid is a group "Drum Grid" whose cells read
    "Kick, bar 1 beat 1, on, velocity 96" or "Snare, bar 1 beat 1 tick
    3840, off"; VO-Space on a cell toggles it; the headers are a group
    "Drum Rows" reading "Kick, C2, Drums, Built-in Drums". The "Drums"
    header band reads "Drums · Built-in Drums"; a row with a destination
    of its own shows it after its name.
98. Channel: with `vizctl` (steps 81–86) and VizDAW in the background,
    `$V run editors.drum_grid_window` on Beat (selected in the sidebar),
    then `$V find "Snare, bar 1 beat 1 tick 3840, off"` gives a cell ID;
    `$V click <ID>` paints it (`$V state document` shows undo "Paint Drum
    Hits") without VizDAW coming to the front. `$V run drum.add_row` adds
    and selects a row, `$V run drum.remove_row` removes it again, and
    `$V run drum.timeline '{"on": true}'` switches the view; in a fresh
    launch, `$V run drum.split_row` before any row was clicked answers
    `unavailable`.
```

- [ ] **Step 2: Run everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -60
cargo test --workspace -- --list 2>/dev/null | grep -c ': test$'
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -1 && scripts/swift.sh test 2>&1 | tail -1
scripts/make-app.sh 2>&1 | tail -1
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --test-threads 1 2>&1 | grep -E "^test |test result"
pgrep -lf MacOS/VizDAW || echo "no VizDAW left"
git status --short
```

Expected: every suite passes (855 Rust tests pass, 5 ignored, 860 listed); Swift 599; the app builds; e2e 3/3; no VizDAW left; `git status` shows only `app/README.md` (generated files are ignored).

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md
git commit -m "docs(app): manual checklist for the drum grid

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --format='%h %s'
```

---

## Self-Review

1. **Spec coverage (§4e, parent §9 drum grid, §3.2):**
   - Shows drum lanes only; rows with note, name and destination: Task 4 (`drum_lanes`), Task 6 (headers: name, note; lane band and a row's own destination), Ruling 2.
   - Step matrix and timeline views: Task 3 (`cells`, `hits_at`, timeline strokes), Task 6 (drawing, accessibility), Task 7 (the bar's view picker), Task 8 (`drum.timeline`).
   - Paint tool toggles on click, paints/erases a run on drag with configurable spacing, duration and velocity: Tasks 1 (`DrumCmd::Paint`), 3 (`begin`/`extend`, `stride`, `paint_window`), 4 (`drum_stroke_command`), 5 (gestures), 6 (`ToolMachine`), 7 (bar).
   - A shown velocity lane sets new hits' velocity from the vertical pointer position: Task 3 (`velocity_at`, `row_velocity`), Task 5 (`aDragPaintsARunAndTheVelocityLaneSetsEachHit`), Task 6 (`aShownVelocityLaneSetsTheVelocityFromTheHeight`); the lane is shown with Show Velocity (header menu, Drum menu).
   - Rows: add, reorder, drag to another drum lane or below the last to create one, split out into its own lane: Tasks 3 (`drop_row`, `next_row`), 4 (`drum_row_drop`, `DrumEdit`), 5 (row commands), 6 (header drags, menu), 8 (Drum menu).
   - Per drum lane play quantize (grid or groove) and nudge: Tasks 2 (grooves), 3 (menu, nudge steps), 4 (`QuantizeRow.groove`), 5 and 7 (lane menu).
   - The engine rule: Task 2 (`drum_lanes.rs`, Ruling 9).
   - §3.2 `Drum` edit group: Task 4. §3.5 accessibility and tools: Tasks 6, 9. §5 errors (status line, aborted gestures, Busy): Task 5. §6 tests: Rust tests per variant (Task 4 `every_drum_edit_applies_and_undoes`), hit-testing and tool transitions (Tasks 3, 5, 6), manual checklist (Task 11).
   - The user's guidelines: logic in Rust (Ruling 1, Tasks 3–4); no dialogs and the registry (Task 8); accessibility elements and `vizctl` (Tasks 6, 9, 10); property tests for hostile input (Tasks 1, 2, 3, 4); README steps from 87 (Task 11).
2. **Placeholder scan:** every code step holds complete code or exact Find/Replace excerpts; no "TBD", "similar to", or unnamed handling.
3. **Type consistency:** `DrumHit`/`MAX_PAINT_HITS` (Task 1) are used by `core-editor` (Task 3) and `drum_cmd` (Task 4); `QuantizeSpec::{grid, swing, check}` (Task 2) by `quantize_choices` (Task 3) and `quantize_of/quantize_row` (Task 4); `core_editor::drum::{Settings, Layout, Spot, Stroke, Drop}` (Task 3) mirror `DrumGridSettingsRow`, `DrumGridLayoutRow`, `DrumSpotRow`, `DrumStrokeRow` and `drum_row_drop` (Task 4); Swift calls `drumGridLayout(shapes:settings:)`, `drumGridSpot(layout:y:)`, `drumRowVelocity(layout:lane:row:y:)`, `drumGridCells(hits:from:to:step:)`, `drumStrokeBegin(lane:row:hits:at:snapped:grab:velocity:eraseTool:settings:)`, `drumStrokeExtend(stroke:hits:at:grab:velocity:settings:)`, `drumStrokeCommand(stroke:seq:lane:row:settings:)`, `drumRowDrop(layout:lanes:seq:laneIndex:row:rowIndex:y:)`, `drumQuantizeChoices()`, `drumQuantizeTitle(quantize:)`, `drumNudged(shift:steps:)`, `drumNextRow(used:)` (Tasks 5–6) exactly as UniFFI generates them; `DrumGridModel`'s API (Task 5) is what Tasks 6–8 call; `DrumCommands` IDs (Task 8) equal the registry's.
4. **Review Focus:** each of the five lines has its named test in its owning task (Tasks 1–6, 8).

Running counts: Rust tests listed 827 → 831 (T1) → 838 (T2) → 851 (T3) → 858 (T4) → 859 (T8) → 860 (T10, an ignored e2e); Swift `@Test` functions 581 → 590 (T5) → 596 (T6) → 597 (T7) → 598 (T8) → 599 (T9).
