# VizDAW 4a — Editors Foundation and Arrangement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The document window shows an arrangement timeline: the sequence's lanes as rows with headers (name, mute/solo/arm, destination), their content grouped into blocks by a user-set rule, reference events as their own blocks labelled with the sequence name, blocks moved, ⌥-copied or merged into another lane by dragging, a smooth playhead, a ruler that selects time ranges, snap, a light drawing mode for big projects; a working sequence sidebar (name, keyboard slot, comment, use count, drag reorder, duplicate, new from template); the subsequence workflow (make a reference from the selection, open, private copy, flatten, re-point, set parameters); and a split detail pane plus tear-off editor windows restored with the project. The detail editors themselves arrive in 4b–4e; 4a shows a placeholder.

**Architecture:** `core-ffi` gains the editor read model — lane revisions computed by comparing `im` pointers (`revs.rs`), flat event rows (`events.rs`), arrangement blocks computed in Rust (`blocks.rs`), sequence info for rulers — and the editor write model: `edit(EditCommand, filter)` mirroring the core `Event`, `Reference`, `Lane`, `Sequence` and `Audio` commands one variant each (`edit.rs`), `edit_gesture` / `abort_edit` for drags, the document selection, `audition` (a new `EngineCmd::Audition` in `core-engine`), sequence templates and an opaque editor-layout blob stored in the package's `ui.json`. `TransportState` gains a host-time stamp. The Swift side adds `EditorModel` (revision-keyed caches, selection, edit and gesture dispatch with the Busy contract, layout) and `TransportClock` to `VizDAWAppModel`, a new `VizDAWEditors` target (pure geometry — `TimeAxis`, grid, tool state machine, auto-scroll, arrangement layout and hit-testing — plus the AppKit `EditorCanvas` and `ArrangementView`), and in `VizDAW` the new document layout, sidebar, subsequence menus and tear-off windows.

**Tech Stack:** Rust 2024 (`core-model`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `im` 15), Swift 6 / SwiftPM (SwiftUI shell, AppKit `NSView` + Core Animation layers + Core Graphics drawing, `NSView.displayLink`, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-16-editors-design.md` (§3 architecture, §4 phase 4a, §5 errors, §6 testing), parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` §9.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp4-editors` (already checked out, at `de68b6f` before Task 1). Commit at the end of every task, adding only the files the task lists; never push. End every commit message with the attribution lines your harness requires (the blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and the `Claude-Session:` line).
- Rust stable, edition 2024; crate manifests keep `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`. License GPL-3.0-only.
- `core-model` gains nothing in 4a (spec §3.1: "`core-model` gains no bookkeeping"); `core-engine` gains only the audition command (Task 6). Both stay free of OS-specific dependencies and build and test on Linux. `core-ffi` is platform-neutral except `#[cfg(target_os = "macos")]` code.
- `core-model`, `core-engine` and `core-ffi` keep `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. Tests may `unwrap` and index. No new `unsafe` anywhere.
- Everything reachable from the render callback stays `// RT`: no allocation, lock, I/O or logging (the audition command in Task 6 sends a fixed 3-byte message).
- No panic crosses the FFI: every exported function and method returns `Result<_, VizError>` and runs inside `ffi_guard`. No session `inner` lock is held across plugin code (3c re-entrancy contract); polling paths skip `VizError::Busy` silently; gestures follow spec §3.8 (Task 10).
- New UniFFI record and enum fields are appended at the END of existing records (`DocSnapshot`, `SeqRow`, `LaneRow`, `TransportState`) so the generated Swift memberwise initialisers only gain trailing arguments.
- Swift: the generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode. Never use SwiftUI's `@State` (the CLT SDK cannot expand it); keep view state in `@Observable` models. Models and pure logic are tested (Swift Testing, `@Test`, `#expect`); AppKit views are exercised by the manual checklist.
- Drawing (spec §2): custom `NSView`s with `wantsLayer`, Core Animation layers, Core Graphics drawing. No Metal.
- `FakeSession` implements the whole generated `SessionProtocol`: every task that adds a `Session` method adds the matching `FakeSession` member in the same task, so `scripts/swift.sh build` and `test` stay green after every task.
- Generated files are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Run `cargo fmt --all` first. Baseline before Task 1: 571 Rust tests, 93 Swift tests.
- Code in the steps is complete for new files (written whole with `cat > … <<'VIZDAW_EOF'`). Changes to existing files are given as exact **Find / Replace** excerpts of the tree the previous task left; apply them with an editor (the Edit tool). If an excerpt does not match because a fix landed meanwhile, make the same change by hand and keep going.
- Spec values used verbatim: gesture dead zone 3 points; a lane with 100 000 events returns from `lane_events` in under 50 ms in a release build; the playhead never runs more than one poll interval past the last sample; polling stays 33 ms playing / 250 ms stopped (Rust is called no more often than today); `NSView.displayLink` (macOS 14).

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no single requirement spells out, each pinned by a named test in its owning task:

1. **A referenced sequence changes while its parent is shown** — renaming Groove, or lengthening it, must relabel and resize the reference blocks in Song at once, although no lane of Song changed revision. Pinned: Task 1 `editing_a_referenced_sequence_keeps_the_parent_lane_revision` (documents the rule); Task 9 `referenceBlocksRefetchWhenAnySequenceRowChanges`.
2. **A plugin editor opens (Busy) in the middle of a block drag** — intermediate moves are dropped silently, the mouse-up result is retried until it lands, and Escape still cancels; the drag is one undo step. Pinned: Task 10 `busyIntermediateEditIsDroppedSilently`, `busyFinalEditIsRetriedOnTick`, `escapeWhileTheFinalEditWaitsAborts`; Task 4 `a_gesture_is_one_undo_step_with_the_last_target`.
3. **A drag the core refuses** — moving a reference block into a lane of the sequence it references (a cycle), or a target lane deleted meanwhile, ends the gesture cleanly: document unchanged, no open transaction left behind, a non-modal status message. Pinned: Task 4 `a_refused_gesture_target_leaves_the_start_state`; Task 10 `refusedFinalEditAbortsAndShowsStatus`, `aDragWhoseLaneVanishesIsCancelled`.
4. **Very large lanes and projects** — 100 000 events in a lane read fast, and thousands of blocks switch the arrangement to outlines automatically. Pinned: Task 2 `lane_events_of_100_000_events_is_fast`; Task 12 `lightModeFollowsTheThreshold`; Task 3 `silence_gap_blocks_of_100_000_notes`.
5. **Pickup bars and extreme scroll/zoom** — negative ticks before bar 1 snap, grid and group into bars correctly, and zooming or scrolling to the ends of the timeline never traps on an out-of-range `Int64` conversion. Pinned: Task 7 `pickupBarsSnapAndNumberBackwards`, `extremeOffsetsClampInsteadOfTrapping`; Task 3 `fixed_bars_put_the_pickup_in_bar_zero`.

## Plan Rulings

Decisions the spec leaves open, each implemented and tested as stated (format: Ruling: what — why — cost if wrong):

1. **Ruling:** Lane revisions live in `core-ffi` (`revs.rs`) and are refreshed lazily whenever a `DocSnapshot` is built: a lane keeps its revision when its scalar fields and drum layout are equal and every take has the same id, name and an `OrdMap::ptr_eq` event map; otherwise it gets the next value of a session-wide counter (undo gives a fresh value, never an old one) — why: spec §3.1; `im::Vector::ptr_eq` is always false for inline vectors and a lane usually has one take, so takes are compared element-wise — cost if wrong: a false "changed" refetches one lane.
2. **Ruling:** A lane's revision does not cover the sequences its references point to; `EditorModel` refetches the blocks of every cached lane that holds a reference block whenever any `SeqRow` (name, length) changes — why: reference blocks show the target's name and span — cost if wrong: one extra `blocks` call per such lane per sequence-level change.
3. **Ruling:** `EventRow` is the spec's flat record made lossless: besides the spec's fields it carries `group_channel`, `attribute`, `text_kind`, `data`/`eight_bit` (SysEx), `frames` (region length), `fade_in`/`fade_out`; `value`/`value2` meanings per kind are fixed in `events.rs`; reference loops are `ref_loops: u32` with 0 meaning Fill — why: ⌥-copy and paste insert rows back (`EventEdit::Insert` takes `EventRow`s), so a row must round-trip every `EventBody` — cost if wrong: none.
4. **Ruling:** `EventRow.duration` is the note length for notes, the reference's span (`reference_span`) for references and the region's length in ticks through the sequence tempo map for audio regions; `Insert` ignores it for references and regions — why: blocks and editors draw extents without re-deriving them in Swift — cost if wrong: none.
5. **Ruling:** Block rules: `SilenceGap { ticks }` starts a new block when the next event starts at least `ticks` (minimum 1) after the furthest end so far; `FixedBars { bars }` groups by bar numbers counted from bar 1 (`(bar - 1).div_euclid(bars)`, so the pickup bar 0 belongs to the group before bar 1) and block extents are those bar spans; `WholeLane` is one block of all non-reference events; references are always their own block and never split or merge other blocks; audio regions group with the other events; zero-length events extend nothing (Swift draws a 4-point minimum width) — why: spec §3.1 names the rules but not their edges — cost if wrong: a different grouping, no data change.
6. **Ruling:** The view filter travels as a second argument, `edit(cmd, filter: Option<ViewFilterRow>)`, instead of a field on every `EditCommand` variant — why: the same filter applies to every variant; one argument keeps the 46 variants identical to the core commands — cost if wrong: none (the FFI shape differs from spec §3.3's wording, not its meaning).
7. **Ruling:** `edit_gesture(cmd, filter)` replaces what the open edit did so far with `cmd` (core `History::abort` then `begin` with the same label, then the command), so every pointer move sends the full target (spec §3.8); `abort_edit()` is new (Escape); `begin_edit` remembers its label — why: move deltas are relative to the drag start; replaying from the start makes dropped intermediate edits harmless — cost if wrong: each pointer move recompiles from the start state (the same cost as today's mixer drags).
8. **Ruling:** Busy and gestures (Swift, `EditorModel`): intermediate `edit_gesture` calls that throw `Busy` are dropped silently; the final one is kept pending and retried by `EditorModel.tick()`, which the 60 Hz plugin timer calls through `DocumentModel.idle()`; a non-Busy failure of the final edit aborts the transaction and shows a status message; Escape aborts at any time; a gesture whose lanes vanish from a snapshot is aborted with a status message (spec §5) — cost if wrong: none.
9. **Ruling:** `audition(dest, pitch, velocity, on)` sends a MIDI 1.0 note on/off through a new `EngineCmd::Audition` to the targets the engine this session holds already has for that destination (destinations used by the loaded sequence) and returns whether anything was sent; without the engine it returns `false` and does nothing. Audition notes go through the active-note tracker, so Stop and Locate release them — why: the engine host is created on the first Play (2c) and starting an audio device for a preview click would be a surprise — cost if wrong: no audition before the first Play (4b's keyboard notes it in the UI).
10. **Ruling:** `TransportState.host_time_ns` is `platform_macos::midi::now_ns()` (CoreAudio host time in ns, the same clock as `CACurrentMediaTime` and `CADisplayLink.timestamp`) taken right after the position is read; off macOS it is ns since the first call in the process. `ticks_per_second` is `PPQ / seconds(tick → tick + PPQ)` through the sequence tempo map (30 720 at 120 BPM) — cost if wrong: a few ms of playhead offset.
11. **Ruling:** `TransportClock` extrapolates `tick + min(elapsed, pollInterval) × ticksPerSecond`; while playing it never steps backwards by less than 0.1 s worth of ticks (it holds instead), and snaps on larger backward jumps (locate, loop wrap), on stop, on start and on a change of sequence — why: spec §3.4 says "snaps back on a locate or stop"; poll jitter would otherwise make the playhead twitch — cost if wrong: a 0.1 s backward locate while playing is shown late by up to 0.1 s.
12. **Ruling:** The editor layout (detail pane content and last-used detail editor, torn-off windows with frames, sequence sidebar order, block rule, light mode, snap, zoom, follow) is a Swift `Codable` value stored opaquely by Rust in the package's existing window-state file `ui.json` (`editor_layout`, beside `open_plugin_windows`); it is written on Save/Save As only, is not an undo step and does not mark the document Edited — why: spec §3.7 "stored in the existing window-state file"; sequence order has no model field and adding one would change `project.json` — cost if wrong: reordering the sidebar alone does not prompt to save.
13. **Ruling:** "New from Template" offers three templates as one undo step "New Sequence from Template": Empty (one lane), Drum Kit (one drum lane "Drums" with GM rows Kick 36, Snare 38, Closed Hat 42, Open Hat 46, Crash 49), and Same Lanes As ‹sequence› (that sequence's lanes with names, destinations, outputs, drum rows, own length and loop flag, plus its tempo, meter and start offset, no events) — why: parent spec §9 names the feature but no templates — cost if wrong: a menu.
14. **Ruling:** Double-clicking a reference block opens the referenced sequence in the arrangement (the sidebar selection follows) with a "◀ ‹parent›" back button; double-clicking any other block opens exactly that block in the detail pane in the last-used detail editor (default piano roll) — why: spec §3.7 and parent §9 ("opening a reference opens its sequence") — cost if wrong: none.
15. **Ruling:** Detail editors get their shortcuts now: ⌘1 piano roll, ⌘2 list, ⌘3 Strip Chart, ⌘4 drum grid; ⌥⌘1–⌥⌘4 open that editor in its own window (spec §3.7 "⌥⌘ with the editor shortcut"); in 4a each shows a placeholder naming its target and event count — why: the spec has no editor shortcuts before 4f's registry — cost if wrong: 4f rebinds them.
16. **Ruling:** Snap defaults to a quarter note, enabled; the unmodified `G` key toggles it while an editor canvas has focus (4f moves it into the command registry); arrangement moves snap the dragged block's start; the ruler snaps both ends of a range — cost if wrong: none.
17. **Ruling:** Light drawing mode is Auto (outlines only when more than 1 500 blocks are visible), On or Off, chosen in the arrangement toolbar — why: spec "above a block count threshold" without a number — cost if wrong: the threshold is one constant.
18. **Ruling:** Canvas scrolling is done by the canvas itself (`TimeAxis.originTick` and a vertical offset), not `NSScrollView`; auto-scroll starts 24 points from an edge at 20 points/s per point of overshoot, at most 1 200 points/s; follow-playback is page mode (when the playhead leaves the view it jumps to 16 points from the left edge) and on by default — cost if wrong: constants.
19. **Ruling:** The selection lives in Rust (`History`); `EditorModel` keeps a copy, pushes every change at once with `set_selection` and refetches it after every edit, gesture end, undo and redo. A ruler drag selects the snapped range and every lane of the shown sequence; a click on a block selects its events and lane and sets the time range to the block; Shift adds; a click on empty space clears events — cost if wrong: none.
20. **Ruling:** `sequence_info` is cached per sequence and `DocSnapshot.version` (tempo and meter edits change no lane revision) — cost if wrong: one call per sequence per edit.
21. **Ruling:** The 100 000-event test is one test whose bound is 50 ms when built without debug assertions and 1 000 ms with them; CI gains a step running it in release on macOS — why: spec §3.1 "test-enforced with a generous bound in CI" — cost if wrong: none.
22. **Ruling:** `PitchAxis` and `ValueAxis` (spec §3.5) land with the editors that use them (4b, 4d); 4a ships `TimeAxis` and the arrangement's row geometry only — YAGNI — cost if wrong: none.
23. **Ruling:** ⌥-dragging a block inserts copies of its rows (`EventEdit::Insert`) in the target lane at the new time; a copied reference is another use of the same sequence (no new sequence). A plain drag moves the events (`EventEdit::Move`, with `to_lane` when the lane changes), which merges them into the target lane's content. The dragged set is the pressed block plus, when it is selected, every other selected block of the same lane; ⌥ is read when the drag begins (it fixes the undo label) — cost if wrong: pressing ⌥ mid-drag does not switch to copying.
24. **Ruling:** Lane headers in 4a edit name (double-click), mute/solo/arm (click) and destination (menu); a "+" row adds a lane. Reordering lanes by dragging headers waits for the visual design pass (sub-project 7) — why: spec §4a lists only those header fields — cost if wrong: none (the `LaneEdit::Reorder` command is already available).
25. **Ruling:** A new test target `VizDAWEditorsTests` tests `VizDAWEditors`' pure types; `Snap`, `MeterMath`, `SequenceOrder`, `EditorLayout`, `TransportClock` and `EditorModel` live in `VizDAWAppModel` (they are model state or persisted) and are tested in `VizDAWAppModelTests` — cost if wrong: none.

## Spec gaps and contradictions found while planning

- §4a / parent §9 "drag reorder" of sequences: the model keeps sequences in an `OrdMap` by ID with no order field (Ruling 12).
- Parent §9 "new-from-template": no template is defined (Ruling 13).
- §3.1 `EventRow` field list cannot round-trip SysEx, text kinds, fades, group/channel overrides or note attributes (Ruling 3).
- §3.2/§3.3 view filter "carried by EditCommand variants" versus one filter per call (Ruling 6).
- §3.2 audition has no engine before the first Play (Ruling 9).
- §3.1 lane revisions say nothing about references whose target changes (Ruling 2).
- §3.7 "⌥⌘ with the editor shortcut" while editor shortcuts are only defined in 4f (Ruling 15).
- §3.8 "the next pointer move resends the full target" needs a way to replace a transaction's content (Ruling 7); the FFI had no abort for Escape.

## File Structure

```
crates/core-engine/
  src/render.rs               EngineCmd::Audition, applied at block start (Task 6)
  src/engine.rs               Engine::audition (Task 6)
  tests/audition.rs           NEW (Task 6)
crates/core-ffi/
  src/revs.rs                 NEW LaneRev, Revisions, same_lane (Task 1)
  src/snapshot.rs             DocSnapshot.lane_revs, SeqRow.keyboard_slot, LaneRow.arm/midi_dest/audio_out,
                              KeySlotRow (Task 1); TransportState.host_time_ns/ticks_per_second (Task 6)
  src/events.rs               NEW EventRow and its parts, LaneEvents, SequenceInfo, row <-> spec (Task 2)
  src/blocks.rs               NEW BlockRule, BlockKind, BlockRow, blocks() (Task 3)
  src/edit.rs                 NEW EditCommand and its five groups, LaneRef, ViewFilterRow, to_command (Task 4)
  src/selection.rs            NEW SelectionRow, TickRange, conversions (Task 5)
  src/templates.rs            NEW SequenceTemplate and its command steps (Task 5)
  src/clock.rs                NEW now_ns, ticks_per_second (Task 6)
  src/session.rs              revisions, from_document (1); lane_events, sequence_info (2); blocks (3);
                              edit, edit_gesture, abort_edit (4); selection, templates, ui layout (5);
                              audition, transport stamp (6)
  src/lib.rs                  modules and re-exports (Tasks 1–6)
  tests/{revs,events,blocks,edit,selection,audition}.rs   NEW
app/
  Package.swift               VizDAWEditors target, VizDAWEditorsTests (Task 7)
  Sources/VizDAWAppModel/     MeterMath, Snap (7); TransportClock (8); EditorTypes, EditorLayout,
                              SequenceOrder, EditorModel (9); EditorModel+Gestures (10);
                              DocumentModel (8, 9, 13, 15)
  Sources/VizDAWEditors/      NEW TimeAxis, Grid (7); ToolMachine, AutoScroll, RulerSelection (11);
                              EditorCanvas, LayerPainter (11); ArrangementLayout, BlockDrag,
                              ArrangementA11y (12); ArrangementView, LaneHeaderView,
                              ArrangementContainer (12)
  Sources/VizDAW/             DocumentView (12, 15), SequenceSidebar (13), ReferenceMenus + sheets (14),
                              EditorWindows, DetailPane, AppCommands (15)
  Tests/VizDAWAppModelTests/  FakeSession grows with each FFI task; MeterMathTests, SnapTests (7);
                              TransportClockTests (8); EditorModelTests, SequenceOrderTests (9);
                              GestureTests (10); SidebarTests (13); ReferenceWorkflowTests (14);
                              LayoutTests (15)
  Tests/VizDAWEditorsTests/   NEW TimeAxisTests, GridTests (7); ToolMachineTests, AutoScrollTests (11);
                              ArrangementLayoutTests (12)
  README.md                   editors section and checklist steps 21–34 (Task 16)
.github/workflows/ci.yml      release run of the 100 000-event test (Task 16)
```

---

### Task 1: Lane revisions, richer sequence and lane rows, a test constructor

**Files:**
- Create: `crates/core-ffi/src/revs.rs`
- Modify: `crates/core-ffi/src/snapshot.rs`, `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/lib.rs`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `app/Tests/VizDAWAppModelTests/FormattingTests.swift`
- Test: `crates/core-ffi/tests/revs.rs` (new)

**Interfaces:**
- Consumes: `core_model::document::Document::sequences()`, `im::OrdMap::ptr_eq`, `Session::with` (private constructor).
- Produces: `vizdaw_ffi::LaneRev { seq: u64, lane: u64, rev: u64 }`; `revs::Revisions::{new(&Document), update(&mut self, &Document), rev(&self, SeqId, LaneId) -> Option<u64>, rows(&self) -> Vec<LaneRev>}`; `revs::same_lane(&Lane, &Lane) -> bool`; `DocSnapshot.lane_revs: Vec<LaneRev>` (last field); `KeySlotRow { key: String, modifiers: u8 }`; `SeqRow.keyboard_slot: Option<KeySlotRow>` (last field); `LaneRow.arm: bool, midi_dest: Option<u64>, audio_out: Option<u64>` (last fields); `Inner.revs: Revisions` and `Inner::snapshot(&mut self)`; test-hooks only: `Session::from_document(Document) -> Result<Arc<Session>, VizError>` (plain Rust, not exported). Swift: `DocSnapshot(…, laneRevs:)`, `SeqRow(…, keyboardSlot:)`, `LaneRow(…, arm:, midiDest:, audioOut:)`.

Ruling 1. Revisions are refreshed when a snapshot is built, so every path that returns a `DocSnapshot` (commands, undo, redo, idle, end_edit) carries current revisions without each path remembering to bump them.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/revs.rs <<'VIZDAW_EOF'
//! Lane revisions (spec §3.1): only lanes that changed get a new one.

use core_engine::fixtures::Builder;
use core_model::command::Command;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::sequence::KeySlot;
use vizdaw_ffi::{DocSnapshot, Session};

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

fn rev(s: &DocSnapshot, seq: u64, lane: u64) -> u64 {
    s.lane_revs
        .iter()
        .find(|r| r.seq == seq && r.lane == lane)
        .unwrap_or_else(|| panic!("no revision for {seq}/{lane}"))
        .rev
}

#[test]
fn every_lane_has_a_revision() {
    let s = Session::new_demo().unwrap();
    let snap = s.snapshot().unwrap();
    let lanes: u32 = snap.sequences.iter().map(|r| r.lane_count).sum();
    assert_eq!(snap.lane_revs.len(), lanes as usize);
    for row in &snap.sequences {
        for lane in s.lanes(row.id).unwrap() {
            rev(&snap, row.id, lane.id);
        }
    }
    let mut all: Vec<u64> = snap.lane_revs.iter().map(|r| r.rev).collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), snap.lane_revs.len(), "revisions are unique");
}

#[test]
fn only_the_changed_lane_gets_a_new_revision() {
    let s = Session::new_demo().unwrap();
    let before = s.snapshot().unwrap();
    let (song, beat) = (id_of(&before, "Song"), id_of(&before, "Beat"));
    let lanes = s.lanes(song).unwrap();
    let (groove, chords) = (lanes[0].id, lanes[1].id);
    let kit = s.lanes(beat).unwrap()[0].id;
    let after = s.set_lane_mute(song, chords, true).unwrap();
    assert_ne!(rev(&after, song, chords), rev(&before, song, chords));
    assert_eq!(rev(&after, song, groove), rev(&before, song, groove));
    assert_eq!(rev(&after, beat, kit), rev(&before, beat, kit));
}

#[test]
fn undo_and_redo_give_fresh_revisions() {
    let s = Session::new_demo().unwrap();
    let a = s.snapshot().unwrap();
    let song = id_of(&a, "Song");
    let lanes = s.lanes(song).unwrap();
    let (groove, chords) = (lanes[0].id, lanes[1].id);
    let b = s.set_lane_mute(song, chords, true).unwrap();
    let c = s.undo().unwrap();
    let d = s.redo().unwrap();
    let revs = [a, b.clone(), c.clone(), d.clone()].map(|x| rev(&x, song, chords));
    let mut unique = revs.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 4, "never an old number again: {revs:?}");
    assert_eq!(rev(&b, song, groove), rev(&d, song, groove));
    assert_eq!(rev(&c, song, groove), rev(&d, song, groove));
}

#[test]
fn mixer_and_sequence_level_edits_keep_every_revision() {
    let s = Session::new_demo().unwrap();
    let before = s.snapshot().unwrap();
    let after = s.add_bus("Verb".into()).unwrap();
    assert_eq!(after.lane_revs, before.lane_revs);
    let renamed = s
        .rename_sequence(id_of(&before, "Beat"), "Kit".into())
        .unwrap();
    assert_eq!(renamed.lane_revs, before.lane_revs);
}

#[test]
fn editing_a_referenced_sequence_keeps_the_parent_lane_revision() {
    // Plan Ruling 2: the parent's reference lane keeps its revision; the
    // app refetches reference blocks when a sequence row changes instead.
    let s = Session::new_demo().unwrap();
    let before = s.snapshot().unwrap();
    let (song, groove) = (id_of(&before, "Song"), id_of(&before, "Groove"));
    let song_groove = s.lanes(song).unwrap()[0].id;
    let bass = s.lanes(groove).unwrap()[1].id;
    let after = s.set_lane_mute(groove, bass, true).unwrap();
    assert_ne!(rev(&after, groove, bass), rev(&before, groove, bass));
    assert_eq!(rev(&after, song, song_groove), rev(&before, song, song_groove));
}

#[test]
fn rows_carry_arm_routing_and_keyboard_slots() {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("A", "Keys", Some(piano)).unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetKeyboardSlot {
        seq,
        slot: Some(KeySlot {
            key: "a".into(),
            modifiers: 4,
        }),
    }))
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let row = &s.lanes(seq.0).unwrap()[0];
    assert!(row.arm);
    assert_eq!(row.midi_dest, Some(piano.0));
    assert_eq!(row.audio_out, None);
    let snap = s.snapshot().unwrap();
    let slot = snap.sequences[0].keyboard_slot.clone().unwrap();
    assert_eq!((slot.key.as_str(), slot.modifiers), ("a", 4));
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p core-ffi --test revs`
Expected: compile errors — no field `lane_revs` on `DocSnapshot`, no field `arm`, no function `from_document`.

- [ ] **Step 3: Write `revs.rs`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/revs.rs <<'VIZDAW_EOF'
//! Lane revisions (spec §3.1). A lane's revision changes whenever anything
//! in it changes: the events of any take, the active take, its flags, its
//! routing, its drum layout. They are computed here by comparing the
//! document the revisions were last computed for with the current one:
//! `im` shares the event maps of unchanged lanes, so a pointer comparison
//! finds them, and `core-model` keeps no bookkeeping (Plan Ruling 1).

use core_model::document::Document;
use core_model::ids::{LaneId, SeqId};
use core_model::lane::Lane;
use std::collections::BTreeMap;

/// One lane's revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct LaneRev {
    /// Sequence ID.
    pub seq: u64,
    /// Lane ID.
    pub lane: u64,
    /// Changes whenever the lane changes; never reused within a session.
    pub rev: u64,
}

/// The revisions of every lane of one session's document.
#[derive(Debug, Clone)]
pub struct Revisions {
    /// The document `revs` describe.
    base: Document,
    revs: BTreeMap<(SeqId, LaneId), u64>,
    next: u64,
}

impl Revisions {
    /// Revisions 1, 2, … for every lane of `doc`.
    pub fn new(doc: &Document) -> Revisions {
        let mut r = Revisions {
            base: doc.clone(),
            revs: BTreeMap::new(),
            next: 1,
        };
        for s in doc.sequences().values() {
            for l in &s.lanes {
                let rev = r.bump();
                r.revs.insert((s.id, l.id), rev);
            }
        }
        r
    }

    fn bump(&mut self) -> u64 {
        let r = self.next;
        self.next = self.next.saturating_add(1);
        r
    }

    /// Brings the revisions up to `doc`: lanes that changed since the last
    /// update, and new lanes, get a fresh revision; lanes that are gone
    /// are dropped. O(1) when no sequence changed (a mixer edit).
    pub fn update(&mut self, doc: &Document) {
        if self.base.sequences().ptr_eq(doc.sequences()) {
            return;
        }
        let mut revs = BTreeMap::new();
        for s in doc.sequences().values() {
            let before = self.base.sequences().get(&s.id);
            for l in &s.lanes {
                let key = (s.id, l.id);
                let kept = before
                    .and_then(|b| b.lane(l.id))
                    .filter(|old| same_lane(old, l))
                    .and_then(|_| self.revs.get(&key).copied());
                let rev = kept.unwrap_or_else(|| self.bump());
                revs.insert(key, rev);
            }
        }
        self.revs = revs;
        self.base = doc.clone();
    }

    /// A lane's revision (`None` when the lane does not exist).
    pub fn rev(&self, seq: SeqId, lane: LaneId) -> Option<u64> {
        self.revs.get(&(seq, lane)).copied()
    }

    /// Every lane's revision, by sequence ID then lane ID.
    pub fn rows(&self) -> Vec<LaneRev> {
        self.revs
            .iter()
            .map(|((s, l), r)| LaneRev {
                seq: s.0,
                lane: l.0,
                rev: *r,
            })
            .collect()
    }
}

/// True when nothing an editor shows can differ between `a` and `b`.
/// Takes are compared one by one (`im::Vector::ptr_eq` is always false for
/// the inline vectors of one or two takes); event maps by pointer, so a
/// lane rebuilt with equal content counts as changed (harmless).
pub fn same_lane(a: &Lane, b: &Lane) -> bool {
    a.id == b.id
        && a.name == b.name
        && a.midi_dest == b.midi_dest
        && a.audio_out == b.audio_out
        && a.mute == b.mute
        && a.solo == b.solo
        && a.arm == b.arm
        && a.length == b.length
        && a.looped == b.looped
        && a.length_locked == b.length_locked
        && a.play_quantize == b.play_quantize
        && a.play_shift == b.play_shift
        && a.drum == b.drum
        && a.active_take == b.active_take
        && a.takes.len() == b.takes.len()
        && a
            .takes
            .iter()
            .zip(b.takes.iter())
            .all(|(x, y)| x.id == y.id && x.name == y.name && x.events.ptr_eq(&y.events))
}
VIZDAW_EOF
```

- [ ] **Step 4: Extend the snapshot records**

In `crates/core-ffi/src/snapshot.rs`:

Find:
```rust
    /// Problems worth showing (unresolved references).
    pub warnings: Vec<String>,
}
```
Replace:
```rust
    /// Problems worth showing (unresolved references).
    pub warnings: Vec<String>,
    /// Every lane's revision (spec §3.1): editors refetch a lane's events
    /// only when its revision changed.
    pub lane_revs: Vec<crate::revs::LaneRev>,
}

/// A keyboard slot: one key plus a modifier bitmask (1 = shift,
/// 2 = control, 4 = option, 8 = command).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct KeySlotRow {
    /// The key, e.g. "a" or "F5".
    pub key: String,
    /// Modifier bitmask.
    pub modifiers: u8,
}
```

Find:
```rust
    /// Number of lanes.
    pub lane_count: u32,
    /// Comment.
    pub comment: String,
}
```
Replace:
```rust
    /// Number of lanes.
    pub lane_count: u32,
    /// Comment.
    pub comment: String,
    /// The key chord that auditions or inserts it.
    pub keyboard_slot: Option<KeySlotRow>,
}
```

Find:
```rust
    /// Has a drum layout.
    pub is_drum: bool,
}
```
Replace:
```rust
    /// Has a drum layout.
    pub is_drum: bool,
    /// Record-armed.
    pub arm: bool,
    /// The lane's MIDI destination ID.
    pub midi_dest: Option<u64>,
    /// The lane's audio output path ID.
    pub audio_out: Option<u64>,
}
```

Find:
```rust
                lane_count: count(s.lanes.len()),
                comment: s.comment.clone(),
            })
```
Replace:
```rust
                lane_count: count(s.lanes.len()),
                comment: s.comment.clone(),
                keyboard_slot: s.keyboard_slot.as_ref().map(|k| KeySlotRow {
                    key: k.key.clone(),
                    modifiers: k.modifiers,
                }),
            })
```

Find:
```rust
                is_drum: l.drum.is_some(),
            }
```
Replace:
```rust
                is_drum: l.drum.is_some(),
                arm: l.arm,
                midi_dest: l.midi_dest.map(|d| d.0),
                audio_out: l.audio_out.map(|o| o.0),
            }
```

- [ ] **Step 5: Keep revisions in the session**

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// `close` ran: the marker is gone and must not come back.
    closed: bool,
}
```
Replace:
```rust
    /// `close` ran: the marker is gone and must not come back.
    closed: bool,
    /// Lane revisions (spec §3.1), refreshed whenever a snapshot is built.
    revs: crate::revs::Revisions,
}
```

Find:
```rust
        Inner {
            key,
            history: History::new(doc.clone()),
```
Replace:
```rust
        Inner {
            key,
            revs: crate::revs::Revisions::new(&doc),
            history: History::new(doc.clone()),
```

Find:
```rust
    fn snapshot(&self) -> Result<DocSnapshot, VizError> {
        let doc = self.doc();
        Ok(DocSnapshot {
```
Replace:
```rust
    fn snapshot(&mut self) -> Result<DocSnapshot, VizError> {
        self.revs.update(self.history.doc());
        let doc = self.doc();
        Ok(DocSnapshot {
            lane_revs: self.revs.rows(),
```

Then add the test constructor after the `impl std::fmt::Debug for Session` block:

Find:
```rust
/// Saving and autosaving one at a time (the package's blob pruning reads
```
Replace:
```rust
#[cfg(feature = "test-hooks")]
impl Session {
    /// An untitled session for any document (tests build documents with
    /// `core_engine::fixtures::Builder`). Not exported to Swift.
    pub fn from_document(doc: Document) -> Result<Arc<Session>, VizError> {
        ffi_guard(|| Session::with(doc, None, false))
    }
}

/// Saving and autosaving one at a time (the package's blob pruning reads
```

- [ ] **Step 6: Export the module**

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod recovery;
pub mod session;
```
Replace:
```rust
pub mod recovery;
pub mod revs;
pub mod session;
```

Find:
```rust
pub use recovery::RecoveryInfo;
```
Replace:
```rust
pub use recovery::RecoveryInfo;
pub use revs::LaneRev;
```

Find:
```rust
    DocSnapshot, IdleUpdate, LaneRow, ParamRow, ParamValue, PluginFormatTag, PluginRefRow, SeqRow,
    SlotRow, SlotStatus, TransportState,
```
Replace:
```rust
    DocSnapshot, IdleUpdate, KeySlotRow, LaneRow, ParamRow, ParamValue, PluginFormatTag,
    PluginRefRow, SeqRow, SlotRow, SlotStatus, TransportState,
```

- [ ] **Step 7: Run the Rust tests**

Run: `cargo test -p core-ffi --test revs && cargo test --workspace`
Expected: the 6 new tests pass; everything else still passes.

- [ ] **Step 8: Follow the new fields in Swift**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
        snap = DocSnapshot(
            version: version, title: "Untitled", dirty: false, undoLabel: nil,
            redoLabel: nil, sequences: sequences, warnings: [])
```
Replace:
```swift
        snap = DocSnapshot(
            version: version, title: "Untitled", dirty: false, undoLabel: nil,
            redoLabel: nil, sequences: sequences, warnings: [], laneRevs: [])
```

Find:
```swift
        id: id, name: name, useCount: uses, topLevel: topLevel, lengthTicks: 61_440,
        laneCount: 1, comment: "")
```
Replace:
```swift
        id: id, name: name, useCount: uses, topLevel: topLevel, lengthTicks: 61_440,
        laneCount: 1, comment: "", keyboardSlot: nil)
```

Find:
```swift
        referenceCount: 0, mute: mute, solo: false, looped: false, takeCount: 1,
        activeTake: "Take 1", isDrum: false)
```
Replace:
```swift
        referenceCount: 0, mute: mute, solo: false, looped: false, takeCount: 1,
        activeTake: "Take 1", isDrum: false, arm: false, midiDest: nil, audioOut: nil)
```

In `app/Tests/VizDAWAppModelTests/FormattingTests.swift`:

Find:
```swift
            version: 0, title: "demo", dirty: false, undoLabel: nil, redoLabel: nil,
            sequences: [], warnings: [])
```
Replace:
```swift
            version: 0, title: "demo", dirty: false, undoLabel: nil, redoLabel: nil,
            sequences: [], warnings: [], laneRevs: [])
```

- [ ] **Step 9: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add crates/core-ffi/src/revs.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/session.rs \
  crates/core-ffi/src/lib.rs crates/core-ffi/tests/revs.rs \
  app/Tests/VizDAWAppModelTests/FakeSession.swift app/Tests/VizDAWAppModelTests/FormattingTests.swift
git commit -m "feat(core-ffi): lane revisions and richer sequence and lane rows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 577 Rust tests, 93 Swift tests.

---

### Task 2: Event rows, `lane_events` and `sequence_info`

**Files:**
- Create: `crates/core-ffi/src/events.rs`
- Modify: `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/lib.rs`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/events.rs` (new)

**Interfaces:**
- Consumes: Task 1's `Inner.revs`, `Session::from_document`, `KeySlotRow`; `core_model::query::length::{reference_span, seq_length}`; `TempoMap::{tick_to_seconds, seconds_to_tick, points}`; `MeterMap::entries`.
- Produces (all `uniffi`): `EventKindRow { Note, Cc, PitchBend, ChannelPressure, PolyPressure, Program, SysEx, Text, Reference, AudioRegion }`; `TextKindRow { Text, Lyric, Cue, Marker, Copyright }`; `FadeShapeRow { Linear, EqualPower, Exponential, SCurve }`; `FadeRow { length: i64, shape: FadeShapeRow }`; `GroupChannel { group: u8, channel: u8 }`; `NoteAttributeRow { kind: u8, data: u16 }`; `PerNoteKindRow { Registered { index: u8 }, Assignable { index: u8 }, PitchBend }`; `PerNotePoint { kind, offset: i64, value: u32 }`; `EventRow` (fields below); `LaneEvents { seq, lane, rev, events: Vec<EventRow> }`; `TempoPointRow { tick: i64, bpm: f64, ramp: bool }`; `MeterRow { tick: i64, num: u8, den: u8, sharps: i8, minor: bool }`; `MarkerRow { tick: i64, name: String }`; `SequenceInfo { id, name, comment, keyboard_slot, start_offset, length_ticks, tempo, meter, markers, generated }`. Rust: `events::{row, spec, lane_events, sequence_info, loops, bad}`; `EventRow::blank(id, tick, kind)`. Session: `lane_events(seq, lane) -> LaneEvents`, `sequence_info(seq) -> SequenceInfo`. Swift: `session.laneEvents(seq:lane:)`, `session.sequenceInfo(seq:)`; `FakeSession.laneEventTable: [String: LaneEvents]` keyed `"seq/lane"`, `laneEventsCalls: [String]`, `readError`, `infoTable`, `infoCalls`.

Rulings 3 and 4. `EventRow` field order (Swift initialiser order): `id, tick, kind, mute, dest, group_channel, pitch, velocity, release_velocity, duration, value, value2, attribute, per_note, ref_seq, ref_length, ref_loops, ref_transpose, media, source_offset, frames, gain_db, fade_in, fade_out, text, text_kind, data, eight_bit`.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/events.rs <<'VIZDAW_EOF'
//! Event rows (spec §3.1, §6): every kind round-trips; big lanes are fast.

use core_engine::fixtures::{Builder, Q};
use core_model::command::Command;
use core_model::command::routing::RoutingCmd;
use core_model::document::Document;
use core_model::event::{
    EventBody, EventSpec, Fade, FadeShape, LoopCount, NoteAttribute, PerNoteCtl, PerNoteKind,
    TextKind,
};
use core_model::ids::{AnyId, LaneId, SeqId};
use core_model::time::Tick;
use std::sync::Arc;
use std::time::Instant;
use vizdaw_ffi::events::spec;
use vizdaw_ffi::{EventKindRow, Session, VizError};

const BAR: i64 = 4 * Q;

/// One lane holding every event kind, plus "Sub" (one bar) to reference.
fn every_kind() -> (Document, SeqId, LaneId) {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let other = b.dest("Other", "builtin:synth", 3).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
        }))
        .unwrap();
    let media = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap();
    let (sub, sub_lane) = b.sequence("Sub", "L", Some(piano)).unwrap();
    b.notes(sub, sub_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let (seq, lane) = b.sequence("All", "Everything", Some(piano)).unwrap();
    let mut note = EventSpec::new(
        Tick(0),
        EventBody::Note {
            pitch: 61,
            velocity: 0x9000,
            release_velocity: 0x1234,
            duration: Tick(Q),
            attribute: Some(NoteAttribute { kind: 3, data: 0x0102 }),
            per_note: [PerNoteCtl {
                kind: PerNoteKind::Registered(7),
                offset: Tick(0),
                value: 0x4000_0000,
            }]
            .into_iter()
            .collect(),
        },
    );
    note.dest = Some(other);
    note.group_channel = Some((0, 5));
    let mut cc = EventSpec::new(Tick(100), EventBody::Cc { index: 74, value: 0x8000_0001 });
    cc.mute = true;
    let specs = vec![
        note,
        cc,
        EventSpec::new(Tick(200), EventBody::PitchBend { value: 0x9000_0000 }),
        EventSpec::new(Tick(300), EventBody::ChannelPressure { value: 7 }),
        EventSpec::new(Tick(400), EventBody::PolyPressure { pitch: 64, value: 9 }),
        EventSpec::new(
            Tick(500),
            EventBody::Program {
                program: 33,
                bank: Some((0, 5)),
            },
        ),
        EventSpec::new(
            Tick(600),
            EventBody::SysEx {
                data: Arc::from(&[0x7E_u8, 0x80, 0x01][..]),
                eight_bit: true,
            },
        ),
        EventSpec::new(
            Tick(700),
            EventBody::Text {
                kind: TextKind::Cue,
                text: "verse".into(),
            },
        ),
        EventSpec {
            mute: true,
            ..EventSpec::new(
                Tick(BAR),
                EventBody::Reference {
                    seq: sub,
                    length: None,
                    loop_count: LoopCount::Count(3),
                    transpose: -2,
                },
            )
        },
        EventSpec::new(
            Tick(2 * BAR),
            EventBody::AudioRegion {
                media,
                source_offset: 10,
                length: 48_000,
                gain_db: -3.5,
                fade_in: Fade {
                    length: Tick(Q / 4),
                    shape: FadeShape::EqualPower,
                },
                fade_out: Fade {
                    length: Tick(Q / 8),
                    shape: FadeShape::SCurve,
                },
            },
        ),
    ];
    b.events(seq, lane, specs).unwrap();
    (b.finish(), seq, lane)
}

#[test]
fn every_kind_round_trips_through_a_row() {
    let (doc, seq, lane) = every_kind();
    let originals: Vec<EventSpec> = doc
        .lane(seq, lane)
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .map(|e| e.spec())
        .collect();
    let s = Session::from_document(doc).unwrap();
    let rows = s.lane_events(seq.0, lane.0).unwrap().events;
    assert_eq!(rows.len(), 10);
    let kinds: Vec<EventKindRow> = rows.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EventKindRow::Note,
            EventKindRow::Cc,
            EventKindRow::PitchBend,
            EventKindRow::ChannelPressure,
            EventKindRow::PolyPressure,
            EventKindRow::Program,
            EventKindRow::SysEx,
            EventKindRow::Text,
            EventKindRow::Reference,
            EventKindRow::AudioRegion,
        ]
    );
    for (row, original) in rows.iter().zip(&originals) {
        assert_eq!(&spec(row).unwrap(), original, "{:?}", row.kind);
    }
    let reference = &rows[8];
    assert_eq!(reference.duration, 3 * BAR, "span: Sub is one bar, three loops");
    assert_eq!(reference.ref_loops, 3);
    let region = &rows[9];
    assert_eq!(region.duration, 2 * Q, "one second at 120 BPM");
    assert_eq!(rows[5].value2, 5, "bank (0, 5) is 0 × 128 + 5");
}

#[test]
fn rows_are_in_time_order_and_carry_the_lane_revision() {
    let s = Session::new_demo().unwrap();
    let snap = s.snapshot().unwrap();
    let song = snap.sequences.iter().find(|r| r.name == "Song").unwrap().id;
    let chords = s.lanes(song).unwrap()[1].id;
    let lane = s.lane_events(song, chords).unwrap();
    assert!(lane.events.windows(2).all(|w| w[0].tick <= w[1].tick));
    let rev = snap
        .lane_revs
        .iter()
        .find(|r| r.seq == song && r.lane == chords)
        .unwrap()
        .rev;
    assert_eq!(lane.rev, rev);
    assert_eq!((lane.seq, lane.lane), (song, chords));
}

#[test]
fn unknown_lanes_are_model_errors() {
    let s = Session::new_demo().unwrap();
    let song = s.snapshot().unwrap().sequences[0].id;
    assert!(matches!(
        s.lane_events(song, 9_999),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.sequence_info(9_999),
        Err(VizError::Model { .. })
    ));
}

#[test]
fn out_of_range_row_values_are_refused() {
    let mut row = vizdaw_ffi::EventRow::blank(0, 0, EventKindRow::Cc);
    row.value2 = 300;
    assert!(matches!(spec(&row), Err(VizError::Model { .. })));
    let mut row = vizdaw_ffi::EventRow::blank(0, 0, EventKindRow::Reference);
    row.ref_seq = None;
    assert!(matches!(spec(&row), Err(VizError::Model { .. })));
}

#[test]
fn sequence_info_reads_tempo_meter_and_length() {
    let s = Session::new_demo().unwrap();
    let snap = s.snapshot().unwrap();
    let song = snap.sequences.iter().find(|r| r.name == "Song").unwrap();
    let info = s.sequence_info(song.id).unwrap();
    assert_eq!(info.name, "Song");
    assert_eq!(info.length_ticks, song.length_ticks);
    assert_eq!(info.length_ticks, 12 * BAR);
    assert_eq!(info.tempo.first().unwrap().bpm, 120.0);
    assert_eq!(info.tempo.last().unwrap().bpm, 90.0);
    assert_eq!(info.meter.len(), 1);
    assert_eq!((info.meter[0].num, info.meter[0].den), (4, 4));
    assert!(!info.generated);
}

#[test]
fn lane_events_of_100_000_events_is_fast() {
    // Spec §3.1: under 50 ms in a release build; a generous bound in debug
    // builds (Plan Ruling 21; CI also runs this test with --release).
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Big", "Notes", Some(piano)).unwrap();
    let specs: Vec<EventSpec> = (0..100_000_i64)
        .map(|i| {
            EventSpec::new(
                Tick(i * Q / 8),
                EventBody::note(36 + (i % 60) as u8, 0x8000, Tick(Q / 8)),
            )
        })
        .collect();
    b.events(seq, lane, specs).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let start = Instant::now();
    let lane = s.lane_events(seq.0, lane.0).unwrap();
    let took = start.elapsed();
    assert_eq!(lane.events.len(), 100_000);
    let bound_ms = if cfg!(debug_assertions) { 1_000 } else { 50 };
    assert!(
        took.as_millis() < bound_ms,
        "lane_events took {took:?} (bound {bound_ms} ms)"
    );
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p core-ffi --test events`
Expected: compile errors — unresolved `vizdaw_ffi::events`, no method `lane_events`.

- [ ] **Step 3: Write `events.rs`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/events.rs <<'VIZDAW_EOF'
//! Event rows (spec §3.1): a lane's active take as flat records the app
//! copies, and rows back into event specs for inserts (Plan Rulings 3, 4).
//!
//! `value` / `value2` by kind:
//! - `Cc`: `value` = 32-bit value, `value2` = controller number;
//! - `PitchBend`, `ChannelPressure`, `PolyPressure`: `value` = 32-bit value;
//! - `Program`: `value` = program, `value2` = bank as msb × 128 + lsb, or
//!   -1 for none;
//! - every other kind: 0.
//!
//! `duration` is the note length, a reference's span, or a region's length
//! in ticks through the sequence's tempo map; inserts ignore it for
//! references and regions (they use `ref_*` and `frames`).

use crate::error::VizError;
use crate::snapshot::KeySlotRow;
use core_model::document::Document;
use core_model::event::{
    Event, EventBody, EventSpec, Fade, FadeShape, LoopCount, NoteAttribute, PerNoteCtl,
    PerNoteKind, TextKind,
};
use core_model::ids::{DestId, LaneId, MediaId, SeqId};
use core_model::query::length::{reference_span, seq_length};
use core_model::sequence::SeqKind;
use core_model::tempo::TempoMap;
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Event kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum EventKindRow {
    /// A note.
    Note,
    /// A control change.
    Cc,
    /// Pitch bend.
    PitchBend,
    /// Channel pressure.
    ChannelPressure,
    /// Polyphonic pressure.
    PolyPressure,
    /// Program change.
    Program,
    /// System exclusive.
    SysEx,
    /// A text meta-event.
    Text,
    /// A reference to another sequence.
    Reference,
    /// An audio region.
    AudioRegion,
}

/// Text meta-event kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum TextKindRow {
    /// Generic text.
    Text,
    /// Lyric.
    Lyric,
    /// Cue point.
    Cue,
    /// Marker.
    Marker,
    /// Copyright notice.
    Copyright,
}

/// Fade curve shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FadeShapeRow {
    /// Linear gain.
    Linear,
    /// Equal power.
    EqualPower,
    /// Exponential.
    Exponential,
    /// S-curve.
    SCurve,
}

/// A fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct FadeRow {
    /// Length in ticks.
    pub length: i64,
    /// Shape.
    pub shape: FadeShapeRow,
}

/// A UMP group and channel override, each 0–15.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct GroupChannel {
    /// UMP group.
    pub group: u8,
    /// Channel.
    pub channel: u8,
}

/// A MIDI 2.0 note attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NoteAttributeRow {
    /// Attribute type.
    pub kind: u8,
    /// Attribute data.
    pub data: u16,
}

/// Per-note controller kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum PerNoteKindRow {
    /// Registered per-note controller.
    Registered {
        /// Controller number.
        index: u8,
    },
    /// Assignable per-note controller.
    Assignable {
        /// Controller number.
        index: u8,
    },
    /// Per-note pitch bend.
    PitchBend,
}

/// One per-note controller value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct PerNotePoint {
    /// Which controller.
    pub kind: PerNoteKindRow,
    /// Offset from the note start in ticks.
    pub offset: i64,
    /// 32-bit value.
    pub value: u32,
}

/// One event as a flat record; fields a kind does not use are zero,
/// empty or `None` (see the module documentation).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EventRow {
    /// Event ID (ignored by inserts).
    pub id: u64,
    /// Start tick.
    pub tick: i64,
    /// Kind.
    pub kind: EventKindRow,
    /// Muted.
    pub mute: bool,
    /// Destination override.
    pub dest: Option<u64>,
    /// Group/channel override.
    pub group_channel: Option<GroupChannel>,
    /// Note or poly-pressure pitch.
    pub pitch: u8,
    /// 16-bit note velocity.
    pub velocity: u16,
    /// 16-bit release velocity.
    pub release_velocity: u16,
    /// Length in ticks (note length, reference span, region length).
    pub duration: i64,
    /// Kind-specific value.
    pub value: i64,
    /// Second kind-specific value.
    pub value2: i64,
    /// Note attribute.
    pub attribute: Option<NoteAttributeRow>,
    /// Per-note controllers.
    pub per_note: Vec<PerNotePoint>,
    /// Referenced sequence.
    pub ref_seq: Option<u64>,
    /// Per-use length of a reference (`None`: the sequence's length).
    pub ref_length: Option<i64>,
    /// Reference repeats; 0 means Fill.
    pub ref_loops: u32,
    /// Reference transpose in semitones.
    pub ref_transpose: i8,
    /// Region media item.
    pub media: Option<u64>,
    /// Region start within the media, in frames.
    pub source_offset: u64,
    /// Region length in frames.
    pub frames: u64,
    /// Region gain in dB.
    pub gain_db: f32,
    /// Region fade in.
    pub fade_in: FadeRow,
    /// Region fade out.
    pub fade_out: FadeRow,
    /// Text of a text event.
    pub text: Option<String>,
    /// Kind of a text event.
    pub text_kind: Option<TextKindRow>,
    /// SysEx payload (without F0/F7).
    pub data: Vec<u8>,
    /// SysEx may hold 8-bit data.
    pub eight_bit: bool,
}

const NO_FADE: FadeRow = FadeRow {
    length: 0,
    shape: FadeShapeRow::Linear,
};

impl EventRow {
    /// A row of `kind` at `tick` with every other field zero, empty or
    /// `None`.
    pub fn blank(id: u64, tick: i64, kind: EventKindRow) -> EventRow {
        EventRow {
            id,
            tick,
            kind,
            mute: false,
            dest: None,
            group_channel: None,
            pitch: 0,
            velocity: 0,
            release_velocity: 0,
            duration: 0,
            value: 0,
            value2: 0,
            attribute: None,
            per_note: Vec::new(),
            ref_seq: None,
            ref_length: None,
            ref_loops: 0,
            ref_transpose: 0,
            media: None,
            source_offset: 0,
            frames: 0,
            gain_db: 0.0,
            fade_in: NO_FADE,
            fade_out: NO_FADE,
            text: None,
            text_kind: None,
            data: Vec::new(),
            eight_bit: false,
        }
    }
}

/// A lane's active take as rows, in time order.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LaneEvents {
    /// Sequence ID.
    pub seq: u64,
    /// Lane ID.
    pub lane: u64,
    /// The lane's revision these rows belong to.
    pub rev: u64,
    /// The rows.
    pub events: Vec<EventRow>,
}

/// One tempo point.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct TempoPointRow {
    /// Where it starts.
    pub tick: i64,
    /// Beats per minute.
    pub bpm: f64,
    /// Ramps linearly to the next point.
    pub ramp: bool,
}

/// One meter (and key) change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct MeterRow {
    /// Where it takes effect (a barline).
    pub tick: i64,
    /// Beats per bar.
    pub num: u8,
    /// Beat unit as a note value (4 = quarter).
    pub den: u8,
    /// Key: sharps (negative for flats).
    pub sharps: i8,
    /// Key: minor.
    pub minor: bool,
}

/// A named position.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct MarkerRow {
    /// Position.
    pub tick: i64,
    /// Name.
    pub name: String,
}

/// What rulers and headers need about a sequence.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SequenceInfo {
    /// Sequence ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// Comment.
    pub comment: String,
    /// Keyboard slot.
    pub keyboard_slot: Option<KeySlotRow>,
    /// Pickup length before bar 1.
    pub start_offset: i64,
    /// Playing length in ticks.
    pub length_ticks: i64,
    /// Tempo map.
    pub tempo: Vec<TempoPointRow>,
    /// Meter map (the first entry is at tick 0).
    pub meter: Vec<MeterRow>,
    /// Markers.
    pub markers: Vec<MarkerRow>,
    /// A generated sequence.
    pub generated: bool,
}

/// A `VizError::Model` with `message`.
pub fn bad(message: &str) -> VizError {
    VizError::Model {
        message: message.to_string(),
    }
}

/// Reference repeats from the FFI convention (0 = Fill).
pub fn loops(n: u32) -> LoopCount {
    if n == 0 {
        LoopCount::Fill
    } else {
        LoopCount::Count(n)
    }
}

fn narrow<T: TryFrom<i64>>(v: i64, what: &str) -> Result<T, VizError> {
    T::try_from(v).map_err(|_| bad(&format!("{what} {v} is out of range")))
}

fn kind_row(b: &EventBody) -> EventKindRow {
    match b {
        EventBody::Note { .. } => EventKindRow::Note,
        EventBody::Cc { .. } => EventKindRow::Cc,
        EventBody::PitchBend { .. } => EventKindRow::PitchBend,
        EventBody::ChannelPressure { .. } => EventKindRow::ChannelPressure,
        EventBody::PolyPressure { .. } => EventKindRow::PolyPressure,
        EventBody::Program { .. } => EventKindRow::Program,
        EventBody::SysEx { .. } => EventKindRow::SysEx,
        EventBody::Text { .. } => EventKindRow::Text,
        EventBody::Reference { .. } => EventKindRow::Reference,
        EventBody::AudioRegion { .. } => EventKindRow::AudioRegion,
    }
}

/// The core kind of an FFI kind.
pub fn kind_of(k: EventKindRow) -> core_model::event::EventKind {
    use core_model::event::EventKind as K;
    match k {
        EventKindRow::Note => K::Note,
        EventKindRow::Cc => K::Cc,
        EventKindRow::PitchBend => K::PitchBend,
        EventKindRow::ChannelPressure => K::ChannelPressure,
        EventKindRow::PolyPressure => K::PolyPressure,
        EventKindRow::Program => K::Program,
        EventKindRow::SysEx => K::SysEx,
        EventKindRow::Text => K::Text,
        EventKindRow::Reference => K::Reference,
        EventKindRow::AudioRegion => K::AudioRegion,
    }
}

fn text_kind_row(k: TextKind) -> TextKindRow {
    match k {
        TextKind::Text => TextKindRow::Text,
        TextKind::Lyric => TextKindRow::Lyric,
        TextKind::Cue => TextKindRow::Cue,
        TextKind::Marker => TextKindRow::Marker,
        TextKind::Copyright => TextKindRow::Copyright,
    }
}

fn text_kind_of(k: TextKindRow) -> TextKind {
    match k {
        TextKindRow::Text => TextKind::Text,
        TextKindRow::Lyric => TextKind::Lyric,
        TextKindRow::Cue => TextKind::Cue,
        TextKindRow::Marker => TextKind::Marker,
        TextKindRow::Copyright => TextKind::Copyright,
    }
}

/// A fade as a row.
pub fn fade_row(f: Fade) -> FadeRow {
    FadeRow {
        length: f.length.0,
        shape: match f.shape {
            FadeShape::Linear => FadeShapeRow::Linear,
            FadeShape::EqualPower => FadeShapeRow::EqualPower,
            FadeShape::Exponential => FadeShapeRow::Exponential,
            FadeShape::SCurve => FadeShapeRow::SCurve,
        },
    }
}

/// A fade from a row.
pub fn fade_of(f: &FadeRow) -> Fade {
    Fade {
        length: Tick(f.length),
        shape: match f.shape {
            FadeShapeRow::Linear => FadeShape::Linear,
            FadeShapeRow::EqualPower => FadeShape::EqualPower,
            FadeShapeRow::Exponential => FadeShape::Exponential,
            FadeShapeRow::SCurve => FadeShape::SCurve,
        },
    }
}

fn per_note_row(p: &PerNoteCtl) -> PerNotePoint {
    PerNotePoint {
        kind: match p.kind {
            PerNoteKind::Registered(index) => PerNoteKindRow::Registered { index },
            PerNoteKind::Assignable(index) => PerNoteKindRow::Assignable { index },
            PerNoteKind::PitchBend => PerNoteKindRow::PitchBend,
        },
        offset: p.offset.0,
        value: p.value,
    }
}

fn per_note_of(p: &PerNotePoint) -> PerNoteCtl {
    PerNoteCtl {
        kind: match p.kind {
            PerNoteKindRow::Registered { index } => PerNoteKind::Registered(index),
            PerNoteKindRow::Assignable { index } => PerNoteKind::Assignable(index),
            PerNoteKindRow::PitchBend => PerNoteKind::PitchBend,
        },
        offset: Tick(p.offset),
        value: p.value,
    }
}

/// Ticks covered by `frames` of `media` starting at `tick`.
fn region_ticks(doc: &Document, tempo: &TempoMap, tick: Tick, media: MediaId, frames: u64) -> i64 {
    let rate = doc
        .media()
        .get(&media)
        .map_or(48_000, |m| m.sample_rate.max(1));
    let start = tempo.tick_to_seconds(tick);
    let end = tempo.seconds_to_tick(start + frames as f64 / f64::from(rate));
    (end - tick).0.max(0)
}

/// One event as a row. `spans` memoises referenced sequences' lengths.
pub fn row(
    doc: &Document,
    tempo: &TempoMap,
    spans: &mut BTreeMap<SeqId, Tick>,
    ev: &Event,
) -> EventRow {
    let mut r = EventRow::blank(ev.id.0, ev.tick.0, kind_row(&ev.body));
    r.mute = ev.mute;
    r.dest = ev.dest.map(|d| d.0);
    r.group_channel = ev
        .group_channel
        .map(|(group, channel)| GroupChannel { group, channel });
    match &ev.body {
        EventBody::Note {
            pitch,
            velocity,
            release_velocity,
            duration,
            attribute,
            per_note,
        } => {
            r.pitch = *pitch;
            r.velocity = *velocity;
            r.release_velocity = *release_velocity;
            r.duration = duration.0;
            r.attribute = attribute.map(|a| NoteAttributeRow {
                kind: a.kind,
                data: a.data,
            });
            r.per_note = per_note.iter().map(per_note_row).collect();
        }
        EventBody::Cc { index, value } => {
            r.value = i64::from(*value);
            r.value2 = i64::from(*index);
        }
        EventBody::PitchBend { value } | EventBody::ChannelPressure { value } => {
            r.value = i64::from(*value);
        }
        EventBody::PolyPressure { pitch, value } => {
            r.pitch = *pitch;
            r.value = i64::from(*value);
        }
        EventBody::Program { program, bank } => {
            r.value = i64::from(*program);
            r.value2 = bank.map_or(-1, |(msb, lsb)| i64::from(msb) * 128 + i64::from(lsb));
        }
        EventBody::SysEx { data, eight_bit } => {
            r.data = data.to_vec();
            r.eight_bit = *eight_bit;
        }
        EventBody::Text { kind, text } => {
            r.text_kind = Some(text_kind_row(*kind));
            r.text = Some(text.clone());
        }
        EventBody::Reference {
            seq,
            length,
            loop_count,
            transpose,
        } => {
            r.ref_seq = Some(seq.0);
            r.ref_length = length.map(|t| t.0);
            r.ref_loops = match loop_count {
                LoopCount::Count(n) => (*n).max(1),
                LoopCount::Fill => 0,
            };
            r.ref_transpose = *transpose;
            let seq_len = *spans
                .entry(*seq)
                .or_insert_with(|| seq_length(doc, *seq).unwrap_or(Tick::ZERO));
            r.duration = reference_span(seq_len, *length, *loop_count).0;
        }
        EventBody::AudioRegion {
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => {
            r.media = Some(media.0);
            r.source_offset = *source_offset;
            r.frames = *length;
            r.gain_db = *gain_db;
            r.fade_in = fade_row(*fade_in);
            r.fade_out = fade_row(*fade_out);
            r.duration = region_ticks(doc, tempo, ev.tick, *media, *length);
        }
    }
    r
}

/// A row as an event spec, for inserts. The core validates the result
/// (ranges, destinations, reference cycles) when it is inserted.
pub fn spec(row: &EventRow) -> Result<EventSpec, VizError> {
    let body = match row.kind {
        EventKindRow::Note => EventBody::Note {
            pitch: row.pitch,
            velocity: row.velocity,
            release_velocity: row.release_velocity,
            duration: Tick(row.duration),
            attribute: row.attribute.map(|a| NoteAttribute {
                kind: a.kind,
                data: a.data,
            }),
            per_note: row.per_note.iter().map(per_note_of).collect(),
        },
        EventKindRow::Cc => EventBody::Cc {
            index: narrow(row.value2, "controller")?,
            value: narrow(row.value, "value")?,
        },
        EventKindRow::PitchBend => EventBody::PitchBend {
            value: narrow(row.value, "value")?,
        },
        EventKindRow::ChannelPressure => EventBody::ChannelPressure {
            value: narrow(row.value, "value")?,
        },
        EventKindRow::PolyPressure => EventBody::PolyPressure {
            pitch: row.pitch,
            value: narrow(row.value, "value")?,
        },
        EventKindRow::Program => EventBody::Program {
            program: narrow(row.value, "program")?,
            bank: if row.value2 < 0 {
                None
            } else {
                Some((
                    narrow(row.value2 / 128, "bank")?,
                    narrow(row.value2 % 128, "bank")?,
                ))
            },
        },
        EventKindRow::SysEx => EventBody::SysEx {
            data: Arc::from(row.data.as_slice()),
            eight_bit: row.eight_bit,
        },
        EventKindRow::Text => EventBody::Text {
            kind: text_kind_of(row.text_kind.unwrap_or(TextKindRow::Text)),
            text: row.text.clone().unwrap_or_default(),
        },
        EventKindRow::Reference => EventBody::Reference {
            seq: SeqId(
                row.ref_seq
                    .ok_or_else(|| bad("a reference row needs ref_seq"))?,
            ),
            length: row.ref_length.map(Tick),
            loop_count: loops(row.ref_loops),
            transpose: row.ref_transpose,
        },
        EventKindRow::AudioRegion => EventBody::AudioRegion {
            media: MediaId(row.media.ok_or_else(|| bad("a region row needs media"))?),
            source_offset: row.source_offset,
            length: row.frames,
            gain_db: row.gain_db,
            fade_in: fade_of(&row.fade_in),
            fade_out: fade_of(&row.fade_out),
        },
    };
    Ok(EventSpec {
        tick: Tick(row.tick),
        dest: row.dest.map(DestId),
        group_channel: row.group_channel.map(|g| (g.group, g.channel)),
        mute: row.mute,
        body,
    })
}

/// The active take of `lane` as rows, tagged with revision `rev`.
pub fn lane_events(
    doc: &Document,
    seq: SeqId,
    lane: LaneId,
    rev: u64,
) -> Result<LaneEvents, VizError> {
    let s = doc.sequence(seq)?;
    let l = doc.lane(seq, lane)?;
    let mut spans = BTreeMap::new();
    let events = l
        .active()
        .map(|t| {
            t.events
                .values()
                .map(|e| row(doc, &s.tempo, &mut spans, e))
                .collect()
        })
        .unwrap_or_default();
    Ok(LaneEvents {
        seq: seq.0,
        lane: lane.0,
        rev,
        events,
    })
}

/// What rulers need about `seq`.
pub fn sequence_info(doc: &Document, seq: SeqId) -> Result<SequenceInfo, VizError> {
    let s = doc.sequence(seq)?;
    Ok(SequenceInfo {
        id: s.id.0,
        name: s.name.clone(),
        comment: s.comment.clone(),
        keyboard_slot: s.keyboard_slot.as_ref().map(|k| KeySlotRow {
            key: k.key.clone(),
            modifiers: k.modifiers,
        }),
        start_offset: s.start_offset.0,
        length_ticks: seq_length(doc, seq)?.0,
        tempo: s
            .tempo
            .points()
            .iter()
            .map(|p| TempoPointRow {
                tick: p.tick.0,
                bpm: p.bpm.as_f64(),
                ramp: p.ramp,
            })
            .collect(),
        meter: s
            .meter
            .entries()
            .iter()
            .map(|e| MeterRow {
                tick: e.tick.0,
                num: e.meter.num,
                den: e.meter.den,
                sharps: e.key.sharps,
                minor: e.key.minor,
            })
            .collect(),
        markers: s
            .markers
            .iter()
            .map(|m| MarkerRow {
                tick: m.tick.0,
                name: m.name.clone(),
            })
            .collect(),
        generated: matches!(s.kind, SeqKind::Generated { .. }),
    })
}
VIZDAW_EOF
```

- [ ] **Step 4: Session reads**

In `crates/core-ffi/src/session.rs`, add a helper to `impl Inner` (after `fn doc`):

Find:
```rust
    fn doc(&self) -> &Document {
        self.history.doc()
    }
```
Replace:
```rust
    fn doc(&self) -> &Document {
        self.history.doc()
    }

    /// A lane's current revision (0 for a lane that does not exist).
    fn lane_rev(&mut self, seq: SeqId, lane: LaneId) -> u64 {
        self.revs.update(self.history.doc());
        self.revs.rev(seq, lane).unwrap_or(0)
    }
```

Append at the end of `session.rs`, before `impl Drop for Session` (find the line `impl Drop for Session {` and insert this block above it):

```rust
/// Editor reads (spec §3.1).
#[uniffi::export]
impl Session {
    /// The active take of lane `lane` of sequence `seq` as rows in time
    /// order, with the lane's revision.
    pub fn lane_events(&self, seq: u64, lane: u64) -> Result<crate::events::LaneEvents, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let rev = inner.lane_rev(SeqId(seq), LaneId(lane));
            crate::events::lane_events(inner.doc(), SeqId(seq), LaneId(lane), rev)
        })
    }

    /// Tempo, meter, markers, start offset and length of sequence `seq`.
    pub fn sequence_info(&self, seq: u64) -> Result<crate::events::SequenceInfo, VizError> {
        ffi_guard(|| crate::events::sequence_info(lock(&self.inner).doc(), SeqId(seq)))
    }
}
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod error;
pub mod guard;
```
Replace:
```rust
pub mod error;
pub mod events;
pub mod guard;
```

Find:
```rust
pub use error::VizError;
```
Replace:
```rust
pub use error::VizError;
pub use events::{
    EventKindRow, EventRow, FadeRow, FadeShapeRow, GroupChannel, LaneEvents, MarkerRow, MeterRow,
    NoteAttributeRow, PerNoteKindRow, PerNotePoint, SequenceInfo, TempoPointRow, TextKindRow,
};
```

- [ ] **Step 5: Run the Rust tests**

Run: `cargo test -p core-ffi --test events && cargo test --workspace`
Expected: the 6 new tests pass (the 100 000-event test takes a few seconds in debug, mostly building the document).

- [ ] **Step 6: `FakeSession` reads**

Append inside `final class FakeSession` in `app/Tests/VizDAWAppModelTests/FakeSession.swift`, before its closing brace (find `    func close() throws { closeCalls += 1 }` and insert after it):

```swift

    // MARK: Editor reads (plan 4a)

    /// Lane events by `"seq/lane"`.
    var laneEventTable: [String: LaneEvents] = [:]
    var laneEventsCalls: [String] = []
    /// Thrown by every editor read (`laneEvents`, `blocks`, `sequenceInfo`).
    var readError: VizError?
    func laneEvents(seq: UInt64, lane: UInt64) throws -> LaneEvents {
        laneEventsCalls.append("\(seq)/\(lane)")
        if let e = readError { throw e }
        guard let ev = laneEventTable["\(seq)/\(lane)"] else {
            throw VizError.Model(message: "unknown lane \(lane)")
        }
        return ev
    }

    var infoTable: [UInt64: SequenceInfo] = [:]
    var infoCalls = 0
    func sequenceInfo(seq: UInt64) throws -> SequenceInfo {
        infoCalls += 1
        if let e = readError { throw e }
        return infoTable[seq] ?? fourFour(seq)
    }
```

And append these free functions at the end of the file:

```swift
/// A 4/4, 120 BPM sequence info, 4 bars long.
func fourFour(_ seq: UInt64, name: String = "S") -> SequenceInfo {
    SequenceInfo(
        id: seq, name: name, comment: "", keyboardSlot: nil, startOffset: 0,
        lengthTicks: 4 * 61_440, tempo: [TempoPointRow(tick: 0, bpm: 120, ramp: false)],
        meter: [MeterRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)], markers: [],
        generated: false)
}

/// A note row.
func noteRow(_ id: UInt64, tick: Int64, pitch: UInt8 = 60, duration: Int64 = 15_360) -> EventRow {
    EventRow(
        id: id, tick: tick, kind: .note, mute: false, dest: nil, groupChannel: nil, pitch: pitch,
        velocity: 0x8000, releaseVelocity: 0, duration: duration, value: 0, value2: 0,
        attribute: nil, perNote: [], refSeq: nil, refLength: nil, refLoops: 0, refTranspose: 0,
        media: nil, sourceOffset: 0, frames: 0, gainDb: 0,
        fadeIn: FadeRow(length: 0, shape: .linear), fadeOut: FadeRow(length: 0, shape: .linear),
        text: nil, textKind: nil, data: [], eightBit: false)
}
```

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add crates/core-ffi/src/events.rs crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/events.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(core-ffi): lane events as rows and sequence info

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 583 Rust tests, 93 Swift tests.

---

### Task 3: Arrangement blocks computed in Rust

**Files:**
- Create: `crates/core-ffi/src/blocks.rs`
- Modify: `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/lib.rs`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/blocks.rs` (new)

**Interfaces:**
- Consumes: Task 2's `events::lane_events` (rows with spans), `EventKindRow`; `MeterMap::{tick_to_bbu, bbu_to_tick}`.
- Produces: `BlockRule { SilenceGap { ticks: i64 }, FixedBars { bars: u32 }, WholeLane }`; `BlockKind { Events, Reference }`; `BlockRow { start: i64, end: i64, kind: BlockKind, events: Vec<u64>, label: Option<String>, ref_seq: Option<u64>, muted: bool }`; `blocks::blocks(&Document, SeqId, LaneId, BlockRule) -> Result<Vec<BlockRow>, VizError>` (sorted by start, then end); `Session::blocks(seq, lane, rule)`. Swift: `session.blocks(seq:lane:rule:)`, `FakeSession.blocksTable: [String: [BlockRow]]`, `blocksCalls: [String]`, helper `blockRow(_:_:events:ref:label:)`.

Ruling 5. A reference block's `events` is `[reference event id]`, its `label` the referenced sequence's name ("Missing sequence" when unresolved); an events block is `muted` when every event in it is muted.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/blocks.rs <<'VIZDAW_EOF'
//! Arrangement blocks (spec §3.1, §6): each rule, reference isolation.

use core_engine::fixtures::{Builder, Q};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{EventId, LaneId, SeqId};
use core_model::time::Tick;
use std::time::Instant;
use vizdaw_ffi::{BlockKind, BlockRow, BlockRule, Session};

const BAR: i64 = 4 * Q;

/// Lane "L" of sequence "S" with `notes` `(tick, duration)`, plus a
/// sequence "Sub" (one bar) referenced at each tick of `refs`.
fn lane(notes: &[(i64, i64)], refs: &[i64]) -> (Document, SeqId, LaneId, Vec<EventId>) {
    let mut b = Builder::new();
    let d = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (sub, sub_lane) = b.sequence("Sub", "L", Some(d)).unwrap();
    b.notes(sub, sub_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(d)).unwrap();
    let list: Vec<(i64, u8, u16, i64)> = notes.iter().map(|(t, d)| (*t, 60, 0x8000, *d)).collect();
    let mut ids = b.notes(seq, lane, &list).unwrap();
    for t in refs {
        ids.push(b.reference(seq, lane, *t, sub, 1, 0).unwrap());
    }
    (b.finish(), seq, lane, ids)
}

fn run(doc: Document, seq: SeqId, lane: LaneId, rule: BlockRule) -> Vec<BlockRow> {
    Session::from_document(doc)
        .unwrap()
        .blocks(seq.0, lane.0, rule)
        .unwrap()
}

fn spans(blocks: &[BlockRow]) -> Vec<(i64, i64, usize)> {
    blocks
        .iter()
        .map(|b| (b.start, b.end, b.events.len()))
        .collect()
}

#[test]
fn silence_gap_splits_where_the_gap_reaches_the_size() {
    let notes = [(0, Q), (Q, Q), (4 * Q, Q)];
    let (doc, seq, l, _) = lane(&notes, &[]);
    let split = run(doc.clone(), seq, l, BlockRule::SilenceGap { ticks: 2 * Q });
    assert_eq!(spans(&split), vec![(0, 2 * Q, 2), (4 * Q, 5 * Q, 1)]);
    let joined = run(doc, seq, l, BlockRule::SilenceGap { ticks: 2 * Q + 1 });
    assert_eq!(spans(&joined), vec![(0, 5 * Q, 3)]);
    assert!(joined.iter().all(|b| b.kind == BlockKind::Events && b.label.is_none()));
}

#[test]
fn references_are_always_their_own_block() {
    let notes = [(0, Q), (2 * BAR, Q)];
    let (doc, seq, l, ids) = lane(&notes, &[BAR]);
    let blocks = run(doc, seq, l, BlockRule::WholeLane);
    assert_eq!(spans(&blocks), vec![(0, 2 * BAR + Q, 2), (BAR, 2 * BAR, 1)]);
    let r = &blocks[1];
    assert_eq!(r.kind, BlockKind::Reference);
    assert_eq!(r.events, vec![ids[2].0]);
    assert_eq!(r.label.as_deref(), Some("Sub"));
    assert!(r.ref_seq.is_some());
    // A reference between two close notes does not split their block.
    let notes = [(0, Q), (Q + 10, Q)];
    let (doc, seq, l, _) = lane(&notes, &[Q]);
    let blocks = run(doc, seq, l, BlockRule::SilenceGap { ticks: Q });
    assert_eq!(blocks.iter().filter(|b| b.kind == BlockKind::Events).count(), 1);
    assert_eq!(blocks.iter().filter(|b| b.kind == BlockKind::Reference).count(), 1);
}

#[test]
fn fixed_bars_group_by_bar_number() {
    let notes = [(0, Q), (BAR + 1, Q), (2 * BAR + 5, 3 * BAR)];
    let (doc, seq, l, _) = lane(&notes, &[]);
    let blocks = run(doc, seq, l, BlockRule::FixedBars { bars: 2 });
    assert_eq!(spans(&blocks), vec![(0, 2 * BAR, 2), (2 * BAR, 4 * BAR, 1)]);
}

#[test]
fn fixed_bars_put_the_pickup_in_bar_zero() {
    let notes = [(-Q, Q), (0, Q)];
    let (doc, seq, l, _) = lane(&notes, &[]);
    let blocks = run(doc, seq, l, BlockRule::FixedBars { bars: 1 });
    assert_eq!(spans(&blocks), vec![(-BAR, 0, 1), (0, BAR, 1)]);
}

#[test]
fn whole_lane_and_empty_lanes() {
    let (doc, seq, l, _) = lane(&[(Q, Q), (10 * BAR, Q)], &[]);
    let blocks = run(doc, seq, l, BlockRule::WholeLane);
    assert_eq!(spans(&blocks), vec![(Q, 10 * BAR + Q, 2)]);
    let (doc, seq, l, _) = lane(&[], &[]);
    assert!(run(doc, seq, l, BlockRule::WholeLane).is_empty());
}

#[test]
fn a_block_of_muted_events_is_muted() {
    let mut b = Builder::new();
    let d = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, l) = b.sequence("S", "L", Some(d)).unwrap();
    let mut a = EventSpec::new(Tick(0), EventBody::note(60, 0x8000, Tick(Q)));
    a.mute = true;
    let mut c = EventSpec::new(Tick(Q), EventBody::note(62, 0x8000, Tick(Q)));
    c.mute = true;
    b.events(seq, l, vec![a, c]).unwrap();
    let blocks = run(b.finish(), seq, l, BlockRule::WholeLane);
    assert!(blocks[0].muted);
}

#[test]
fn silence_gap_blocks_of_100_000_notes() {
    let mut b = Builder::new();
    let d = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, l) = b.sequence("Big", "L", Some(d)).unwrap();
    let specs: Vec<EventSpec> = (0..100_000_i64)
        .map(|i| EventSpec::new(Tick(i * Q / 8), EventBody::note(60, 0x8000, Tick(Q / 8))))
        .collect();
    b.events(seq, l, specs).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let start = Instant::now();
    let blocks = s.blocks(seq.0, l.0, BlockRule::SilenceGap { ticks: Q }).unwrap();
    let took = start.elapsed();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].events.len(), 100_000);
    let bound_ms = if cfg!(debug_assertions) { 2_000 } else { 100 };
    assert!(took.as_millis() < bound_ms, "blocks took {took:?}");
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p core-ffi --test blocks`
Expected: compile errors — unresolved `BlockKind`, `BlockRow`, `BlockRule`, no method `blocks`.

- [ ] **Step 3: Write `blocks.rs`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/blocks.rs <<'VIZDAW_EOF'
//! Arrangement blocks (spec §3.1, Plan Ruling 5): a lane's active take
//! grouped for the arrangement. A reference event is always its own block;
//! other events (audio regions included) group by the rule.

use crate::error::VizError;
use crate::events::{EventKindRow, EventRow, lane_events};
use core_model::document::Document;
use core_model::ids::{LaneId, SeqId};
use core_model::meter::MeterMap;
use core_model::time::{BarBeatUnit, Tick};

/// How a lane's events group into blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum BlockRule {
    /// A new block starts after at least `ticks` (minimum 1) of silence.
    SilenceGap {
        /// Gap size in ticks.
        ticks: i64,
    },
    /// Blocks of `bars` bars, counted from bar 1.
    FixedBars {
        /// Bars per block (minimum 1).
        bars: u32,
    },
    /// One block for all events.
    WholeLane,
}

/// What a block holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum BlockKind {
    /// Ordinary events (and audio regions).
    Events,
    /// One reference event.
    Reference,
}

/// One arrangement block.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct BlockRow {
    /// First tick.
    pub start: i64,
    /// End tick (exclusive; equal to `start` for zero-length content).
    pub end: i64,
    /// Kind.
    pub kind: BlockKind,
    /// The events in the block, in time order.
    pub events: Vec<u64>,
    /// The referenced sequence's name, for reference blocks.
    pub label: Option<String>,
    /// The referenced sequence, for reference blocks.
    pub ref_seq: Option<u64>,
    /// Every event in the block is muted.
    pub muted: bool,
}

struct Group {
    start: i64,
    end: i64,
    events: Vec<u64>,
    muted: bool,
    /// The bar span `[start, end)` of a fixed-bars group.
    bars: Option<(i64, i64)>,
}

impl Group {
    fn finish(self) -> BlockRow {
        let (start, end) = self.bars.unwrap_or((self.start, self.end));
        BlockRow {
            start,
            end,
            kind: BlockKind::Events,
            events: self.events,
            label: None,
            ref_seq: None,
            muted: self.muted,
        }
    }
}

/// The span `[start, end)` of the `bars`-bar group holding `tick`.
fn bar_span(meter: &MeterMap, tick: i64, bars: u32) -> (i64, i64) {
    let n = i32::try_from(bars.max(1)).unwrap_or(i32::MAX);
    let bar = meter.tick_to_bbu(Tick(tick), 1).bar;
    let first = (bar - 1).div_euclid(n).saturating_mul(n).saturating_add(1);
    let at = |bar: i32| {
        meter
            .bbu_to_tick(
                BarBeatUnit {
                    bar,
                    beat: 1,
                    unit: 0,
                },
                1,
            )
            .0
    };
    (at(first), at(first.saturating_add(n)))
}

fn reference_block(doc: &Document, r: &EventRow) -> BlockRow {
    let label = r
        .ref_seq
        .and_then(|s| doc.sequence(SeqId(s)).ok())
        .map_or_else(|| "Missing sequence".to_string(), |s| s.name.clone());
    BlockRow {
        start: r.tick,
        end: r.tick.saturating_add(r.duration.max(0)),
        kind: BlockKind::Reference,
        events: vec![r.id],
        label: Some(label),
        ref_seq: r.ref_seq,
        muted: r.mute,
    }
}

/// The blocks of `lane` under `rule`, sorted by start, then end.
pub fn blocks(
    doc: &Document,
    seq: SeqId,
    lane: LaneId,
    rule: BlockRule,
) -> Result<Vec<BlockRow>, VizError> {
    let meter = &doc.sequence(seq)?.meter;
    let rows = lane_events(doc, seq, lane, 0)?.events;
    let mut out = Vec::new();
    let mut group: Option<Group> = None;
    for r in &rows {
        if r.kind == EventKindRow::Reference {
            out.push(reference_block(doc, r));
            continue;
        }
        let end = r.tick.saturating_add(r.duration.max(0));
        let bars = match rule {
            BlockRule::FixedBars { bars } => Some(bar_span(meter, r.tick, bars)),
            _ => None,
        };
        let joins = match (&group, rule) {
            (None, _) => false,
            (Some(g), BlockRule::SilenceGap { ticks }) => {
                r.tick.saturating_sub(g.end) < ticks.max(1)
            }
            (Some(g), BlockRule::FixedBars { .. }) => g.bars == bars,
            (Some(_), BlockRule::WholeLane) => true,
        };
        match group.as_mut() {
            Some(g) if joins => {
                g.end = g.end.max(end);
                g.events.push(r.id);
                g.muted &= r.mute;
            }
            _ => {
                if let Some(g) = group.take() {
                    out.push(g.finish());
                }
                group = Some(Group {
                    start: r.tick,
                    end,
                    events: vec![r.id],
                    muted: r.mute,
                    bars,
                });
            }
        }
    }
    if let Some(g) = group {
        out.push(g.finish());
    }
    out.sort_by_key(|b| (b.start, b.end));
    Ok(out)
}
VIZDAW_EOF
```

- [ ] **Step 4: Session method and exports**

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// Tempo, meter, markers, start offset and length of sequence `seq`.
    pub fn sequence_info(&self, seq: u64) -> Result<crate::events::SequenceInfo, VizError> {
        ffi_guard(|| crate::events::sequence_info(lock(&self.inner).doc(), SeqId(seq)))
    }
```
Replace:
```rust
    /// Tempo, meter, markers, start offset and length of sequence `seq`.
    pub fn sequence_info(&self, seq: u64) -> Result<crate::events::SequenceInfo, VizError> {
        ffi_guard(|| crate::events::sequence_info(lock(&self.inner).doc(), SeqId(seq)))
    }

    /// The arrangement blocks of lane `lane` under `rule` (spec §3.1).
    pub fn blocks(
        &self,
        seq: u64,
        lane: u64,
        rule: crate::blocks::BlockRule,
    ) -> Result<Vec<crate::blocks::BlockRow>, VizError> {
        ffi_guard(|| {
            crate::blocks::blocks(lock(&self.inner).doc(), SeqId(seq), LaneId(lane), rule)
        })
    }
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
uniffi::setup_scaffolding!();

pub mod catalog;
```
Replace:
```rust
uniffi::setup_scaffolding!();

pub mod blocks;
pub mod catalog;
```

Find:
```rust
pub use catalog::{
```
Replace:
```rust
pub use blocks::{BlockKind, BlockRow, BlockRule};
pub use catalog::{
```

- [ ] **Step 5: Run the Rust tests**

Run: `cargo test -p core-ffi --test blocks && cargo test --workspace`
Expected: the 7 new tests pass.

- [ ] **Step 6: `FakeSession` blocks**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`, after the `sequenceInfo(seq:)` member added in Task 2, add:

```swift

    /// Blocks by `"seq/lane"`.
    var blocksTable: [String: [BlockRow]] = [:]
    var blocksCalls: [String] = []
    var lastRule: BlockRule?
    func blocks(seq: UInt64, lane: UInt64, rule: BlockRule) throws -> [BlockRow] {
        blocksCalls.append("\(seq)/\(lane)")
        lastRule = rule
        if let e = readError { throw e }
        return blocksTable["\(seq)/\(lane)"] ?? []
    }
```

Append at the end of the file:

```swift
/// An events block, or a reference block when `ref` is set.
func blockRow(
    _ start: Int64, _ end: Int64, events: [UInt64], ref: UInt64? = nil, label: String? = nil
) -> BlockRow {
    BlockRow(
        start: start, end: end, kind: ref == nil ? .events : .reference, events: events,
        label: label, refSeq: ref, muted: false)
}
```

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add crates/core-ffi/src/blocks.rs crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/blocks.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(core-ffi): arrangement blocks by silence gap, fixed bars or whole lane

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 590 Rust tests, 93 Swift tests.

---

### Task 4: `edit(EditCommand)`, gesture replacement and abort

**Files:**
- Create: `crates/core-ffi/src/edit.rs`
- Modify: `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/lib.rs`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/edit.rs` (new)

**Interfaces:**
- Consumes: Task 2's `events::{spec, loops, bad, fade_of, kind_of}`, `EventRow`, `FadeRow`, `GroupChannel`, `TempoPointRow`, `MeterRow`, `MarkerRow`; Task 1's `KeySlotRow`; core `EventCmd`, `ReferenceCmd`, `LaneCmd`, `SequenceCmd`, `AudioCmd`, `History::{begin, abort, commit}`.
- Produces: `LaneRef { seq: u64, lane: u64 }`; `QuantizeRow { grid: i64, strength: u8 }`; `GeneratorRow { generator: String, params: HashMap<String, String> }`; `ViewFilterRow { hidden_kinds: Vec<EventKindRow>, hidden_lanes: Vec<u64> }`; `EditCommand { Event { edit: EventEdit }, Reference { edit: ReferenceEdit }, Lane { edit: LaneEdit }, Sequence { edit: SequenceEdit }, Audio { edit: AudioEdit } }` with the variants below; `edit::to_command(&EditCommand) -> Result<Command, VizError>`; `edit::view_filter(Option<&ViewFilterRow>) -> ViewFilter`. Session: `edit(cmd, filter) -> DocSnapshot`, `edit_gesture(cmd, filter) -> DocSnapshot`, `abort_edit() -> DocSnapshot`; private `execute_with(Command, &ViewFilter)`; `Inner.edit_label: Option<String>`. Swift: `session.edit(cmd:filter:)`, `session.editGesture(cmd:filter:)`, `session.abortEdit()`; `EditCommand.event(edit: .move(events:delta:toLane:))` and so on; `FakeSession.edits`, `editError`, `gestureEdits`, `gestureErrors: [VizError?]`, `calls` gains `"edit"`, `"gesture"`, `"gesture refused"`, `"abort"`.

Rulings 6 and 7. Variants (Swift case names in brackets are the UniFFI camel-case forms):
- `EventEdit`: `Insert { seq, lane, events: Vec<EventRow> }`, `Delete { events: Vec<u64> }`, `Move { events, delta: i64, to_lane: Option<LaneRef> }`, `SetPitch { events, pitch: u8 }`, `SetVelocity { events, velocity: u16 }`, `SetDuration { events, duration: i64 }`, `SetValue { events, value: u32 }`, `SetDest { events, dest: Option<u64> }`, `SetGroupChannel { events, group_channel: Option<GroupChannel> }`, `SetMute { events, mute: bool }`, `Nudge { events, ticks: i64, semitones: i8 }`.
- `ReferenceEdit`: `Insert { seq, lane, tick: i64, target: u64, length: Option<i64>, loop_count: u32, transpose: i8 }`, `FromSelection { seq, per_lane: bool, round_to_bars: bool, loop_count: u32 }`, `RePoint { event, target }`, `MakePrivateCopy { event }`, `Flatten { event }`, `SetParams { event, length: Option<i64>, loop_count: u32, transpose: i8 }` (`loop_count` 0 = Fill).
- `LaneEdit`: `Add { seq, name, index: Option<u32> }`, `Remove { seq, lane }`, `Reorder { seq, lane, to: u32 }`, `Rename { seq, lane, name }`, `SetDest { seq, lane, dest: Option<u64> }`, `SetOutput { seq, lane, output: Option<u64> }`, `SetFlags { seq, lane, mute: Option<bool>, solo: Option<bool>, arm: Option<bool> }`, `SetLength { seq, lane, length: Option<i64>, looped: bool, locked: bool }`, `SetPlayQuantize { seq, lane, quantize: Option<QuantizeRow> }`, `SetPlayShift { seq, lane, shift: i64 }`, `AddTake { seq, lane, name }`, `SetActiveTake { seq, lane, take: u64 }`.
- `SequenceEdit`: `New { name }`, `Duplicate { seq }`, `Rename { seq, name }`, `Delete { seq, delete_references: bool }`, `SetKind { seq, generator: Option<GeneratorRow> }` (None = normal), `SetKeyboardSlot { seq, slot: Option<KeySlotRow> }`, `SetTempo { seq, tempo: Vec<TempoPointRow> }`, `SetMeter { seq, meter: Vec<MeterRow> }`, `SetMarkers { seq, markers: Vec<MarkerRow> }`, `SetStartOffset { seq, offset: i64 }`, `SetLength { seq, locked: Option<i64> }` (None = derived), `SetComment { seq, comment }`.
- `AudioEdit`: `Insert { seq, lane, tick, media: u64, source_offset: u64, length: u64, gain_db: f32, fade_in: FadeRow, fade_out: FadeRow }`, `Trim { event, front_frames: i64, back_frames: i64 }`, `Split { event, at: i64 }`, `SetGain { event, gain_db: f32 }`, `SetFades { event, fade_in, fade_out }`.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/edit.rs <<'VIZDAW_EOF'
//! Editor commands (spec §3.2, §6): every variant applies and undoes;
//! gestures replace their content; a refused target leaves the start.

use core_engine::fixtures::{Builder, Q};
use core_model::command::Command;
use core_model::command::lane::LaneCmd;
use core_model::command::routing::RoutingCmd;
use core_model::document::Document;
use core_model::event::{EventBody, Fade};
use core_model::ids::{AnyId, DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
use std::collections::HashMap;
use vizdaw_ffi::edit::to_command;
use vizdaw_ffi::{
    AudioEdit, EditCommand, EventEdit, EventKindRow, EventRow, FadeRow, FadeShapeRow,
    GeneratorRow, GroupChannel, KeySlotRow, LaneEdit, LaneRef, MarkerRow, MeterRow, QuantizeRow,
    ReferenceEdit, SequenceEdit, Session, TempoPointRow, VizError,
};

const BAR: i64 = 4 * Q;

struct Fixture {
    doc: Document,
    s: SeqId,
    l1: LaneId,
    l2: LaneId,
    sub2: SeqId,
    sub: SeqId,
    note: EventId,
    cc: EventId,
    region: EventId,
    reference: EventId,
    dest2: DestId,
    out: OutputPathId,
    media: MediaId,
    take1: TakeId,
}

fn created<T>(r: &core_model::command::Report, pick: impl Fn(&AnyId) -> Option<T>) -> T {
    r.created.iter().find_map(pick).unwrap()
}

fn fixture() -> Fixture {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let dest2 = b.dest("Bass", "builtin:synth", 1).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Out 3-4".into(),
            reference: "hw:3".into(),
        }))
        .unwrap();
    let out = created(&r, |id| match id {
        AnyId::Output(o) => Some(*o),
        _ => None,
    });
    let r = b
        .run(Command::Routing(RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 480_000,
            content_hash: None,
        }))
        .unwrap();
    let media = created(&r, |id| match id {
        AnyId::Media(m) => Some(*m),
        _ => None,
    });
    let (sub, sub_lane) = b.sequence("Sub", "L", Some(piano)).unwrap();
    b.notes(sub, sub_lane, &[(0, 60, 0x8000, Q)]).unwrap();
    let (sub2, sub2_lane) = b.sequence("Sub2", "L", Some(piano)).unwrap();
    b.notes(sub2, sub2_lane, &[(0, 67, 0x8000, Q)]).unwrap();
    let (s, l1) = b.sequence("S", "One", Some(piano)).unwrap();
    let l2 = b.lane(s, "Two", Some(piano)).unwrap();
    let note = b.notes(s, l1, &[(0, 60, 0x8000, Q)]).unwrap()[0];
    let cc = b
        .event(s, l1, Q, EventBody::Cc { index: 7, value: 1 << 30 })
        .unwrap();
    let reference = b.reference(s, l1, BAR, sub, 1, 0).unwrap();
    let region = b
        .event(
            s,
            l1,
            2 * BAR,
            EventBody::AudioRegion {
                media,
                source_offset: 0,
                length: 48_000,
                gain_db: 0.0,
                fade_in: Fade::default(),
                fade_out: Fade::default(),
            },
        )
        .unwrap();
    let take1 = b.doc().lane(s, l2).unwrap().active_take;
    b.run(Command::Lane(LaneCmd::AddTake {
        seq: s,
        lane: l2,
        name: "Take 2".into(),
    }))
    .unwrap();
    Fixture {
        doc: b.finish(),
        s,
        l1,
        l2,
        sub2,
        sub,
        note,
        cc,
        region,
        reference,
        dest2,
        out,
        media,
        take1,
    }
}

/// Everything the app can read, without revisions.
fn state(s: &Session) -> String {
    let snap = s.snapshot().unwrap();
    let mut out = format!("{:?}\n", snap.sequences);
    for row in &snap.sequences {
        out += &format!("{:?}\n", s.sequence_info(row.id).unwrap());
        for l in s.lanes(row.id).unwrap() {
            out += &format!("{l:?}\n{:?}\n", s.lane_events(row.id, l.id).unwrap().events);
        }
    }
    out
}

fn fade(length: i64) -> FadeRow {
    FadeRow {
        length,
        shape: FadeShapeRow::EqualPower,
    }
}

fn ev(e: EventEdit) -> EditCommand {
    EditCommand::Event { edit: e }
}

fn commands(f: &Fixture) -> Vec<EditCommand> {
    let (s, l1, l2) = (f.s.0, f.l1.0, f.l2.0);
    let one = |id: EventId| vec![id.0];
    let mut row = EventRow::blank(0, 3 * Q, EventKindRow::Note);
    row.pitch = 64;
    row.velocity = 0x8000;
    row.duration = Q;
    vec![
        ev(EventEdit::Insert { seq: s, lane: l2, events: vec![row] }),
        ev(EventEdit::Delete { events: one(f.note) }),
        ev(EventEdit::Move { events: one(f.note), delta: Q, to_lane: None }),
        ev(EventEdit::Move {
            events: one(f.note),
            delta: 0,
            to_lane: Some(LaneRef { seq: s, lane: l2 }),
        }),
        ev(EventEdit::SetPitch { events: one(f.note), pitch: 61 }),
        ev(EventEdit::SetVelocity { events: one(f.note), velocity: 0x1000 }),
        ev(EventEdit::SetDuration { events: one(f.note), duration: 2 * Q }),
        ev(EventEdit::SetValue { events: one(f.cc), value: 5 }),
        ev(EventEdit::SetDest { events: one(f.note), dest: Some(f.dest2.0) }),
        ev(EventEdit::SetGroupChannel {
            events: one(f.note),
            group_channel: Some(GroupChannel { group: 0, channel: 2 }),
        }),
        ev(EventEdit::SetMute { events: one(f.note), mute: true }),
        ev(EventEdit::Nudge { events: one(f.note), ticks: 10, semitones: 2 }),
        EditCommand::Reference {
            edit: ReferenceEdit::Insert {
                seq: s,
                lane: l2,
                tick: 0,
                target: f.sub.0,
                length: None,
                loop_count: 1,
                transpose: 0,
            },
        },
        EditCommand::Reference {
            edit: ReferenceEdit::RePoint { event: f.reference.0, target: f.sub2.0 },
        },
        EditCommand::Reference {
            edit: ReferenceEdit::MakePrivateCopy { event: f.reference.0 },
        },
        EditCommand::Reference { edit: ReferenceEdit::Flatten { event: f.reference.0 } },
        EditCommand::Reference {
            edit: ReferenceEdit::SetParams {
                event: f.reference.0,
                length: Some(2 * BAR),
                loop_count: 2,
                transpose: 3,
            },
        },
        EditCommand::Lane { edit: LaneEdit::Add { seq: s, name: "Three".into(), index: Some(0) } },
        EditCommand::Lane { edit: LaneEdit::Remove { seq: s, lane: l2 } },
        EditCommand::Lane { edit: LaneEdit::Reorder { seq: s, lane: l2, to: 0 } },
        EditCommand::Lane { edit: LaneEdit::Rename { seq: s, lane: l1, name: "Uno".into() } },
        EditCommand::Lane { edit: LaneEdit::SetDest { seq: s, lane: l1, dest: None } },
        EditCommand::Lane {
            edit: LaneEdit::SetOutput { seq: s, lane: l1, output: Some(f.out.0) },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: s,
                lane: l1,
                mute: Some(true),
                solo: None,
                arm: Some(true),
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetLength {
                seq: s,
                lane: l1,
                length: Some(BAR),
                looped: true,
                locked: false,
            },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetPlayQuantize {
                seq: s,
                lane: l1,
                quantize: Some(QuantizeRow { grid: Q / 4, strength: 50 }),
            },
        },
        EditCommand::Lane { edit: LaneEdit::SetPlayShift { seq: s, lane: l1, shift: -20 } },
        EditCommand::Lane {
            edit: LaneEdit::AddTake { seq: s, lane: l1, name: "Alt".into() },
        },
        EditCommand::Lane {
            edit: LaneEdit::SetActiveTake { seq: s, lane: l2, take: f.take1.0 },
        },
        EditCommand::Sequence { edit: SequenceEdit::New { name: "Fresh".into() } },
        EditCommand::Sequence { edit: SequenceEdit::Duplicate { seq: s } },
        EditCommand::Sequence { edit: SequenceEdit::Rename { seq: s, name: "Song".into() } },
        EditCommand::Sequence {
            edit: SequenceEdit::Delete { seq: f.sub.0, delete_references: true },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetKind {
                seq: s,
                generator: Some(GeneratorRow {
                    generator: "arp".into(),
                    params: HashMap::from([("rate".to_string(), "8".to_string())]),
                }),
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetKeyboardSlot {
                seq: s,
                slot: Some(KeySlotRow { key: "F5".into(), modifiers: 8 }),
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetTempo {
                seq: s,
                tempo: vec![
                    TempoPointRow { tick: 0, bpm: 100.0, ramp: true },
                    TempoPointRow { tick: BAR, bpm: 140.5, ramp: false },
                ],
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetMeter {
                seq: s,
                meter: vec![MeterRow { tick: 0, num: 3, den: 4, sharps: -2, minor: true }],
            },
        },
        EditCommand::Sequence {
            edit: SequenceEdit::SetMarkers {
                seq: s,
                markers: vec![MarkerRow { tick: BAR, name: "B".into() }],
            },
        },
        EditCommand::Sequence { edit: SequenceEdit::SetStartOffset { seq: s, offset: Q } },
        EditCommand::Sequence { edit: SequenceEdit::SetLength { seq: s, locked: Some(8 * BAR) } },
        EditCommand::Sequence {
            edit: SequenceEdit::SetComment { seq: s, comment: "intro".into() },
        },
        EditCommand::Audio {
            edit: AudioEdit::Insert {
                seq: s,
                lane: l2,
                tick: 0,
                media: f.media.0,
                source_offset: 100,
                length: 4_800,
                gain_db: -6.0,
                fade_in: fade(10),
                fade_out: fade(20),
            },
        },
        EditCommand::Audio {
            edit: AudioEdit::Trim { event: f.region.0, front_frames: 100, back_frames: 100 },
        },
        EditCommand::Audio { edit: AudioEdit::Split { event: f.region.0, at: 2 * BAR + Q } },
        EditCommand::Audio { edit: AudioEdit::SetGain { event: f.region.0, gain_db: 3.0 } },
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event: f.region.0,
                fade_in: fade(Q / 8),
                fade_out: fade(Q / 4),
            },
        },
    ]
}

#[test]
fn every_variant_applies_and_undoes() {
    let f = fixture();
    let all = commands(&f);
    assert_eq!(all.len(), 46, "every 4a variant (Move twice) but FromSelection (Task 5)");
    for cmd in all {
        let s = Session::from_document(f.doc.clone()).unwrap();
        let (before, revs) = (state(&s), s.snapshot().unwrap().lane_revs);
        let label = to_command(&cmd).unwrap().name();
        let snap = s
            .edit(cmd.clone(), None)
            .unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
        assert_eq!(snap.undo_label.as_deref(), Some(label), "{cmd:?}");
        assert!(
            state(&s) != before || snap.lane_revs != revs,
            "{cmd:?} changed nothing"
        );
        s.undo().unwrap();
        assert_eq!(state(&s), before, "{cmd:?} did not undo");
    }
}

#[test]
fn a_new_lane_gets_a_revision_and_a_removed_one_loses_it() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let before = s.snapshot().unwrap().lane_revs.len();
    let snap = s
        .edit(
            EditCommand::Lane {
                edit: LaneEdit::Add { seq: f.s.0, name: "New".into(), index: None },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.lane_revs.len(), before + 1);
    let snap = s
        .edit(EditCommand::Lane { edit: LaneEdit::Remove { seq: f.s.0, lane: f.l2.0 } }, None)
        .unwrap();
    assert_eq!(snap.lane_revs.len(), before);
    assert!(!snap.lane_revs.iter().any(|r| r.lane == f.l2.0));
}

fn move_note(f: &Fixture, delta: i64, to: Option<LaneRef>) -> EditCommand {
    ev(EventEdit::Move { events: vec![f.note.0], delta, to_lane: to })
}

fn note_tick(s: &Session, f: &Fixture) -> i64 {
    s.lane_events(f.s.0, f.l1.0)
        .unwrap()
        .events
        .iter()
        .find(|r| r.id == f.note.0)
        .unwrap()
        .tick
}

#[test]
fn a_gesture_is_one_undo_step_with_the_last_target() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    s.begin_edit("Move Block".into()).unwrap();
    s.edit_gesture(move_note(&f, Q, None), None).unwrap();
    assert_eq!(note_tick(&s, &f), Q);
    s.edit_gesture(move_note(&f, 2 * Q, None), None).unwrap();
    assert_eq!(note_tick(&s, &f), 2 * Q, "replaced, not added to");
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Move Block"));
    s.undo().unwrap();
    assert_eq!(note_tick(&s, &f), 0);
}

#[test]
fn a_refused_gesture_target_leaves_the_start_state() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let start = state(&s);
    // Moving the reference to Sub into Sub's own lane would be a cycle.
    let sub_lane = s.lanes(f.sub.0).unwrap()[0].id;
    s.begin_edit("Move Block".into()).unwrap();
    s.edit_gesture(move_note(&f, Q, None), None).unwrap();
    let cycle = ev(EventEdit::Move {
        events: vec![f.reference.0],
        delta: 0,
        to_lane: Some(LaneRef { seq: f.sub.0, lane: sub_lane }),
    });
    assert!(matches!(s.edit_gesture(cycle, None), Err(VizError::Model { .. })));
    assert_eq!(state(&s), start, "the earlier target is gone too");
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label, None, "no undo step");
}

#[test]
fn abort_edit_restores_and_closes_the_edit() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let start = state(&s);
    s.begin_edit("Move Block".into()).unwrap();
    s.edit_gesture(move_note(&f, Q, None), None).unwrap();
    s.abort_edit().unwrap();
    assert_eq!(state(&s), start);
    assert!(matches!(
        s.edit_gesture(move_note(&f, Q, None), None),
        Err(VizError::Model { .. })
    ));
    // Nothing open: abort is a no-op.
    s.abort_edit().unwrap();
}

#[test]
fn bad_rows_and_values_are_model_errors_and_change_nothing() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let start = state(&s);
    let mut row = EventRow::blank(0, 0, EventKindRow::Cc);
    row.value2 = 999;
    let bad = ev(EventEdit::Insert { seq: f.s.0, lane: f.l1.0, events: vec![row] });
    assert!(matches!(s.edit(bad, None), Err(VizError::Model { .. })));
    let tempo = EditCommand::Sequence {
        edit: SequenceEdit::SetTempo {
            seq: f.s.0,
            tempo: vec![TempoPointRow { tick: 0, bpm: f64::NAN, ramp: false }],
        },
    };
    assert!(matches!(s.edit(tempo, None), Err(VizError::Model { .. })));
    assert_eq!(state(&s), start);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p core-ffi --test edit`
Expected: compile errors — unresolved `vizdaw_ffi::edit`, `EditCommand`, no method `edit_gesture`.

- [ ] **Step 3: Write `edit.rs`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/edit.rs <<'VIZDAW_EOF'
//! Editor commands (spec §3.2): one variant per core command of the
//! groups 4a needs (Event, Reference, Lane, Sequence, Audio); field types
//! are FFI records. The view filter travels beside the command
//! (Plan Ruling 6).

use crate::error::VizError;
use crate::events::{
    EventKindRow, EventRow, FadeRow, GroupChannel, MarkerRow, MeterRow, TempoPointRow, bad,
    fade_of, kind_of, loops, spec,
};
use crate::snapshot::KeySlotRow;
use core_model::command::{AudioCmd, Command, EventCmd, LaneCmd, ReferenceCmd, SequenceCmd};
use core_model::ids::{DestId, EventId, LaneId, MediaId, OutputPathId, SeqId, TakeId};
use core_model::lane::QuantizeSpec;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::selection::ViewFilter;
use core_model::sequence::{GeneratorParams, KeySlot, Marker, SeqKind, SeqLength};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::Tick;
use std::collections::HashMap;

/// A lane of a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct LaneRef {
    /// Sequence ID.
    pub seq: u64,
    /// Lane ID.
    pub lane: u64,
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
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GeneratorRow {
    /// Generator name.
    pub generator: String,
    /// Parameters.
    pub params: HashMap<String, String>,
}

/// What a view hides from commands that act on the selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ViewFilterRow {
    /// Hidden event kinds.
    pub hidden_kinds: Vec<EventKindRow>,
    /// Hidden lanes.
    pub hidden_lanes: Vec<u64>,
}

/// Every edit an editor sends (spec §3.2).
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum EditCommand {
    /// Event edits.
    Event {
        /// The edit.
        edit: EventEdit,
    },
    /// Reference edits.
    Reference {
        /// The edit.
        edit: ReferenceEdit,
    },
    /// Lane and take edits.
    Lane {
        /// The edit.
        edit: LaneEdit,
    },
    /// Sequence edits.
    Sequence {
        /// The edit.
        edit: SequenceEdit,
    },
    /// Audio-region edits.
    Audio {
        /// The edit.
        edit: AudioEdit,
    },
}

/// Mirrors `core_model::command::EventCmd`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum EventEdit {
    /// Insert rows (IDs ignored) into a lane's active take.
    Insert {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// The events.
        events: Vec<EventRow>,
    },
    /// Delete events.
    Delete {
        /// The events.
        events: Vec<u64>,
    },
    /// Move in time and optionally to another lane.
    Move {
        /// The events.
        events: Vec<u64>,
        /// Time offset.
        delta: i64,
        /// Destination lane.
        to_lane: Option<LaneRef>,
    },
    /// Set pitch.
    SetPitch {
        /// The events.
        events: Vec<u64>,
        /// Pitch 0–127.
        pitch: u8,
    },
    /// Set velocity.
    SetVelocity {
        /// The events.
        events: Vec<u64>,
        /// 16-bit velocity.
        velocity: u16,
    },
    /// Set note duration.
    SetDuration {
        /// The events.
        events: Vec<u64>,
        /// Ticks (>= 0).
        duration: i64,
    },
    /// Set the 32-bit value.
    SetValue {
        /// The events.
        events: Vec<u64>,
        /// Value.
        value: u32,
    },
    /// Set or clear the destination override.
    SetDest {
        /// The events.
        events: Vec<u64>,
        /// Destination.
        dest: Option<u64>,
    },
    /// Set or clear the group/channel override.
    SetGroupChannel {
        /// The events.
        events: Vec<u64>,
        /// Group and channel.
        group_channel: Option<GroupChannel>,
    },
    /// Mute or unmute.
    SetMute {
        /// The events.
        events: Vec<u64>,
        /// Mute flag.
        mute: bool,
    },
    /// Nudge in time and pitch.
    Nudge {
        /// The events.
        events: Vec<u64>,
        /// Time offset.
        ticks: i64,
        /// Pitch offset.
        semitones: i8,
    },
}

/// Mirrors `core_model::command::ReferenceCmd` (`loop_count` 0 = Fill).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ReferenceEdit {
    /// Insert a reference.
    Insert {
        /// Owning sequence.
        seq: u64,
        /// Owning lane.
        lane: u64,
        /// Position.
        tick: i64,
        /// Referenced sequence.
        target: u64,
        /// Per-use length.
        length: Option<i64>,
        /// Repeats (0 = Fill).
        loop_count: u32,
        /// Transpose.
        transpose: i8,
    },
    /// Move the selection into new sequence(s) and reference them.
    FromSelection {
        /// The sequence the selection is in.
        seq: u64,
        /// One reference per lane.
        per_lane: bool,
        /// Widen the range to barlines.
        round_to_bars: bool,
        /// Repeats of the new reference(s) (0 = Fill).
        loop_count: u32,
    },
    /// Point a reference at another sequence.
    RePoint {
        /// The reference event.
        event: u64,
        /// New target.
        target: u64,
    },
    /// Clone the referenced sequence for this use.
    MakePrivateCopy {
        /// The reference event.
        event: u64,
    },
    /// Replace the reference with its events.
    Flatten {
        /// The reference event.
        event: u64,
    },
    /// Set per-use length, repeats and transpose.
    SetParams {
        /// The reference event.
        event: u64,
        /// Per-use length.
        length: Option<i64>,
        /// Repeats (0 = Fill).
        loop_count: u32,
        /// Transpose.
        transpose: i8,
    },
}

/// Mirrors `core_model::command::LaneCmd`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LaneEdit {
    /// Add a lane at `index` or at the end.
    Add {
        /// Sequence.
        seq: u64,
        /// Name.
        name: String,
        /// Position.
        index: Option<u32>,
    },
    /// Remove a lane.
    Remove {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
    },
    /// Move a lane to position `to`.
    Reorder {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// New index.
        to: u32,
    },
    /// Rename a lane.
    Rename {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// New name.
        name: String,
    },
    /// Set the MIDI destination.
    SetDest {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Destination.
        dest: Option<u64>,
    },
    /// Set the audio output path.
    SetOutput {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Output.
        output: Option<u64>,
    },
    /// Set mute/solo/arm; `None` leaves a flag unchanged.
    SetFlags {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Mute.
        mute: Option<bool>,
        /// Solo.
        solo: Option<bool>,
        /// Record arm.
        arm: Option<bool>,
    },
    /// Set the lane's own length, loop flag and lock.
    SetLength {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Own length.
        length: Option<i64>,
        /// Loop.
        looped: bool,
        /// Lock against recording.
        locked: bool,
    },
    /// Set play quantize.
    SetPlayQuantize {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Quantize.
        quantize: Option<QuantizeRow>,
    },
    /// Set play shift.
    SetPlayShift {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Shift in ticks.
        shift: i64,
    },
    /// Add an empty take and make it active.
    AddTake {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Take name.
        name: String,
    },
    /// Choose the playing take.
    SetActiveTake {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Take.
        take: u64,
    },
}

/// Mirrors `core_model::command::SequenceCmd`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum SequenceEdit {
    /// A new sequence with one empty lane.
    New {
        /// Name.
        name: String,
    },
    /// A deep copy.
    Duplicate {
        /// Source.
        seq: u64,
    },
    /// Rename.
    Rename {
        /// Target.
        seq: u64,
        /// New name.
        name: String,
    },
    /// Delete (a used sequence needs `delete_references`).
    Delete {
        /// Target.
        seq: u64,
        /// Also remove every reference to it.
        delete_references: bool,
    },
    /// Normal (`None`) or generated.
    SetKind {
        /// Target.
        seq: u64,
        /// Generator parameters.
        generator: Option<GeneratorRow>,
    },
    /// Assign or clear the keyboard slot.
    SetKeyboardSlot {
        /// Target.
        seq: u64,
        /// Slot.
        slot: Option<KeySlotRow>,
    },
    /// Replace the tempo map.
    SetTempo {
        /// Target.
        seq: u64,
        /// Points; the first at tick 0.
        tempo: Vec<TempoPointRow>,
    },
    /// Replace the meter map.
    SetMeter {
        /// Target.
        seq: u64,
        /// Entries; the first at tick 0, all on barlines.
        meter: Vec<MeterRow>,
    },
    /// Replace the markers.
    SetMarkers {
        /// Target.
        seq: u64,
        /// Markers.
        markers: Vec<MarkerRow>,
    },
    /// Set the pickup length.
    SetStartOffset {
        /// Target.
        seq: u64,
        /// Offset (>= 0).
        offset: i64,
    },
    /// Lock the length (`Some`) or derive it (`None`).
    SetLength {
        /// Target.
        seq: u64,
        /// Locked length.
        locked: Option<i64>,
    },
    /// Set the comment.
    SetComment {
        /// Target.
        seq: u64,
        /// Comment.
        comment: String,
    },
}

/// Mirrors `core_model::command::AudioCmd`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum AudioEdit {
    /// Insert a region.
    Insert {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// Start tick.
        tick: i64,
        /// Media item.
        media: u64,
        /// Start within the media (frames).
        source_offset: u64,
        /// Length (frames).
        length: u64,
        /// Gain in dB.
        gain_db: f32,
        /// Fade in.
        fade_in: FadeRow,
        /// Fade out.
        fade_out: FadeRow,
    },
    /// Trim frames from the front and back.
    Trim {
        /// The region.
        event: u64,
        /// Frames removed from the start.
        front_frames: i64,
        /// Frames removed from the end.
        back_frames: i64,
    },
    /// Split in two at `at`.
    Split {
        /// The region.
        event: u64,
        /// Split position.
        at: i64,
    },
    /// Set gain.
    SetGain {
        /// The region.
        event: u64,
        /// Gain in dB.
        gain_db: f32,
    },
    /// Set fades.
    SetFades {
        /// The region.
        event: u64,
        /// Fade in.
        fade_in: FadeRow,
        /// Fade out.
        fade_out: FadeRow,
    },
}

fn ids(v: &[u64]) -> Vec<EventId> {
    v.iter().copied().map(EventId).collect()
}

fn lane_ref(l: &LaneRef) -> (SeqId, LaneId) {
    (SeqId(l.seq), LaneId(l.lane))
}

fn index(i: u32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

/// The core command for an editor command.
pub fn to_command(c: &EditCommand) -> Result<Command, VizError> {
    Ok(match c {
        EditCommand::Event { edit } => Command::Event(event_cmd(edit)?),
        EditCommand::Reference { edit } => Command::Reference(reference_cmd(edit)),
        EditCommand::Lane { edit } => Command::Lane(lane_cmd(edit)),
        EditCommand::Sequence { edit } => Command::Sequence(sequence_cmd(edit)?),
        EditCommand::Audio { edit } => Command::Audio(audio_cmd(edit)),
    })
}

/// The core view filter (`None`: hide nothing).
pub fn view_filter(f: Option<&ViewFilterRow>) -> ViewFilter {
    f.map_or_else(ViewFilter::default, |f| ViewFilter {
        hidden_kinds: f.hidden_kinds.iter().map(|k| kind_of(*k)).collect(),
        hidden_lanes: f.hidden_lanes.iter().copied().map(LaneId).collect(),
    })
}

fn event_cmd(e: &EventEdit) -> Result<EventCmd, VizError> {
    Ok(match e {
        EventEdit::Insert { seq, lane, events } => EventCmd::Insert {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            events: events.iter().map(spec).collect::<Result<_, _>>()?,
        },
        EventEdit::Delete { events } => EventCmd::Delete {
            events: ids(events),
        },
        EventEdit::Move {
            events,
            delta,
            to_lane,
        } => EventCmd::Move {
            events: ids(events),
            delta: Tick(*delta),
            to_lane: to_lane.as_ref().map(lane_ref),
        },
        EventEdit::SetPitch { events, pitch } => EventCmd::SetPitch {
            events: ids(events),
            pitch: *pitch,
        },
        EventEdit::SetVelocity { events, velocity } => EventCmd::SetVelocity {
            events: ids(events),
            velocity: *velocity,
        },
        EventEdit::SetDuration { events, duration } => EventCmd::SetDuration {
            events: ids(events),
            duration: Tick(*duration),
        },
        EventEdit::SetValue { events, value } => EventCmd::SetValue {
            events: ids(events),
            value: *value,
        },
        EventEdit::SetDest { events, dest } => EventCmd::SetDest {
            events: ids(events),
            dest: dest.map(DestId),
        },
        EventEdit::SetGroupChannel {
            events,
            group_channel,
        } => EventCmd::SetGroupChannel {
            events: ids(events),
            group_channel: group_channel.map(|g| (g.group, g.channel)),
        },
        EventEdit::SetMute { events, mute } => EventCmd::SetMute {
            events: ids(events),
            mute: *mute,
        },
        EventEdit::Nudge {
            events,
            ticks,
            semitones,
        } => EventCmd::Nudge {
            events: ids(events),
            ticks: Tick(*ticks),
            semitones: *semitones,
        },
    })
}

fn reference_cmd(e: &ReferenceEdit) -> ReferenceCmd {
    match e {
        ReferenceEdit::Insert {
            seq,
            lane,
            tick,
            target,
            length,
            loop_count,
            transpose,
        } => ReferenceCmd::Insert {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            tick: Tick(*tick),
            target: SeqId(*target),
            length: length.map(Tick),
            loop_count: loops(*loop_count),
            transpose: *transpose,
        },
        ReferenceEdit::FromSelection {
            seq,
            per_lane,
            round_to_bars,
            loop_count,
        } => ReferenceCmd::FromSelection {
            seq: SeqId(*seq),
            per_lane: *per_lane,
            round_to_bars: *round_to_bars,
            loop_count: loops(*loop_count),
        },
        ReferenceEdit::RePoint { event, target } => ReferenceCmd::RePoint {
            event: EventId(*event),
            target: SeqId(*target),
        },
        ReferenceEdit::MakePrivateCopy { event } => ReferenceCmd::MakePrivateCopy {
            event: EventId(*event),
        },
        ReferenceEdit::Flatten { event } => ReferenceCmd::Flatten {
            event: EventId(*event),
        },
        ReferenceEdit::SetParams {
            event,
            length,
            loop_count,
            transpose,
        } => ReferenceCmd::SetParams {
            event: EventId(*event),
            length: length.map(Tick),
            loop_count: loops(*loop_count),
            transpose: *transpose,
        },
    }
}

fn lane_cmd(e: &LaneEdit) -> LaneCmd {
    match e {
        LaneEdit::Add { seq, name, index: i } => LaneCmd::Add {
            seq: SeqId(*seq),
            name: name.clone(),
            index: i.map(index),
        },
        LaneEdit::Remove { seq, lane } => LaneCmd::Remove {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
        },
        LaneEdit::Reorder { seq, lane, to } => LaneCmd::Reorder {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            to: index(*to),
        },
        LaneEdit::Rename { seq, lane, name } => LaneCmd::Rename {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            name: name.clone(),
        },
        LaneEdit::SetDest { seq, lane, dest } => LaneCmd::SetDest {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            dest: dest.map(DestId),
        },
        LaneEdit::SetOutput { seq, lane, output } => LaneCmd::SetOutput {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            output: output.map(OutputPathId),
        },
        LaneEdit::SetFlags {
            seq,
            lane,
            mute,
            solo,
            arm,
        } => LaneCmd::SetFlags {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            mute: *mute,
            solo: *solo,
            arm: *arm,
        },
        LaneEdit::SetLength {
            seq,
            lane,
            length,
            looped,
            locked,
        } => LaneCmd::SetLength {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            length: length.map(Tick),
            looped: *looped,
            locked: *locked,
        },
        LaneEdit::SetPlayQuantize {
            seq,
            lane,
            quantize,
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
            lane: LaneId(*lane),
            shift: Tick(*shift),
        },
        LaneEdit::AddTake { seq, lane, name } => LaneCmd::AddTake {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            name: name.clone(),
        },
        LaneEdit::SetActiveTake { seq, lane, take } => LaneCmd::SetActiveTake {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            take: TakeId(*take),
        },
    }
}

fn bpm(v: f64) -> Result<Bpm, VizError> {
    if !v.is_finite() || v <= 0.0 {
        return Err(bad(&format!("tempo {v} BPM is out of range")));
    }
    let milli = (v * 1000.0).round();
    if milli > f64::from(u32::MAX) {
        return Err(bad(&format!("tempo {v} BPM is out of range")));
    }
    Ok(Bpm(milli as u32))
}

fn sequence_cmd(e: &SequenceEdit) -> Result<SequenceCmd, VizError> {
    Ok(match e {
        SequenceEdit::New { name } => SequenceCmd::New { name: name.clone() },
        SequenceEdit::Duplicate { seq } => SequenceCmd::Duplicate { seq: SeqId(*seq) },
        SequenceEdit::Rename { seq, name } => SequenceCmd::Rename {
            seq: SeqId(*seq),
            name: name.clone(),
        },
        SequenceEdit::Delete {
            seq,
            delete_references,
        } => SequenceCmd::Delete {
            seq: SeqId(*seq),
            delete_references: *delete_references,
        },
        SequenceEdit::SetKind { seq, generator } => SequenceCmd::SetKind {
            seq: SeqId(*seq),
            kind: match generator {
                None => SeqKind::Normal,
                Some(g) => SeqKind::Generated {
                    generator: GeneratorParams {
                        generator: g.generator.clone(),
                        params: g
                            .params
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    },
                },
            },
        },
        SequenceEdit::SetKeyboardSlot { seq, slot } => SequenceCmd::SetKeyboardSlot {
            seq: SeqId(*seq),
            slot: slot.as_ref().map(|k| KeySlot {
                key: k.key.clone(),
                modifiers: k.modifiers,
            }),
        },
        SequenceEdit::SetTempo { seq, tempo } => SequenceCmd::SetTempo {
            seq: SeqId(*seq),
            tempo: TempoMap::new(
                tempo
                    .iter()
                    .map(|p| {
                        Ok(TempoPoint {
                            tick: Tick(p.tick),
                            bpm: bpm(p.bpm)?,
                            ramp: p.ramp,
                        })
                    })
                    .collect::<Result<Vec<_>, VizError>>()?,
            )?,
        },
        SequenceEdit::SetMeter { seq, meter } => SequenceCmd::SetMeter {
            seq: SeqId(*seq),
            meter: MeterMap::new(
                meter
                    .iter()
                    .map(|m| MeterEntry {
                        tick: Tick(m.tick),
                        meter: Meter {
                            num: m.num,
                            den: m.den,
                        },
                        key: KeySig {
                            sharps: m.sharps,
                            minor: m.minor,
                        },
                    })
                    .collect(),
            )?,
        },
        SequenceEdit::SetMarkers { seq, markers } => SequenceCmd::SetMarkers {
            seq: SeqId(*seq),
            markers: markers
                .iter()
                .map(|m| Marker {
                    tick: Tick(m.tick),
                    name: m.name.clone(),
                })
                .collect(),
        },
        SequenceEdit::SetStartOffset { seq, offset } => SequenceCmd::SetStartOffset {
            seq: SeqId(*seq),
            offset: Tick(*offset),
        },
        SequenceEdit::SetLength { seq, locked } => SequenceCmd::SetLength {
            seq: SeqId(*seq),
            length: locked.map_or(SeqLength::Derived, |t| SeqLength::Locked(Tick(t))),
        },
        SequenceEdit::SetComment { seq, comment } => SequenceCmd::SetComment {
            seq: SeqId(*seq),
            comment: comment.clone(),
        },
    })
}

fn audio_cmd(e: &AudioEdit) -> AudioCmd {
    match e {
        AudioEdit::Insert {
            seq,
            lane,
            tick,
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => AudioCmd::Insert {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            tick: Tick(*tick),
            media: MediaId(*media),
            source_offset: *source_offset,
            length: *length,
            gain_db: *gain_db,
            fade_in: fade_of(fade_in),
            fade_out: fade_of(fade_out),
        },
        AudioEdit::Trim {
            event,
            front_frames,
            back_frames,
        } => AudioCmd::Trim {
            event: EventId(*event),
            front_frames: *front_frames,
            back_frames: *back_frames,
        },
        AudioEdit::Split { event, at } => AudioCmd::Split {
            event: EventId(*event),
            at: Tick(*at),
        },
        AudioEdit::SetGain { event, gain_db } => AudioCmd::SetGain {
            event: EventId(*event),
            gain_db: *gain_db,
        },
        AudioEdit::SetFades {
            event,
            fade_in,
            fade_out,
        } => AudioCmd::SetFades {
            event: EventId(*event),
            fade_in: fade_of(fade_in),
            fade_out: fade_of(fade_out),
        },
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Session edits**

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// Lane revisions (spec §3.1), refreshed whenever a snapshot is built.
    revs: crate::revs::Revisions,
}
```
Replace:
```rust
    /// Lane revisions (spec §3.1), refreshed whenever a snapshot is built.
    revs: crate::revs::Revisions,
    /// The label of the edit `begin_edit` opened (`edit_gesture` reopens
    /// it under the same label).
    edit_label: Option<String>,
}
```

Find:
```rust
            revs: crate::revs::Revisions::new(&doc),
```
Replace:
```rust
            revs: crate::revs::Revisions::new(&doc),
            edit_label: None,
```

Find:
```rust
    fn execute(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
        let next = core_model::command::apply(lock(&self.inner).doc(), &cmd)
            .ok()
            .map(|n| n.doc);
        if let Some(next) = next {
            self.capture_removed(&next)?;
        }
        let mut inner = lock(&self.inner);
        inner.history.execute(&cmd, &ViewFilter::default())?;
```
Replace:
```rust
    fn execute(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
        self.execute_with(cmd, &ViewFilter::default())
    }

    /// [`Session::execute`] with a view filter (editor commands that act
    /// on the selection).
    fn execute_with(&self, cmd: Command, filter: &ViewFilter) -> Result<DocSnapshot, VizError> {
        let next = core_model::command::apply(lock(&self.inner).doc(), &cmd)
            .ok()
            .map(|n| n.doc);
        if let Some(next) = next {
            self.capture_removed(&next)?;
        }
        let mut inner = lock(&self.inner);
        inner.history.execute(&cmd, filter)?;
```

Find:
```rust
    pub fn begin_edit(&self, label: String) -> Result<(), VizError> {
        ffi_guard(|| Ok(lock(&self.inner).history.begin(label)?))
    }
```
Replace:
```rust
    pub fn begin_edit(&self, label: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.history.begin(label.clone())?;
            inner.edit_label = Some(label);
            Ok(())
        })
    }
```

Find:
```rust
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                inner.history.commit()?;
                inner.version += 1;
            }
            self.commit_held(&mut inner);
            inner.snapshot()
```
Replace:
```rust
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                inner.history.commit()?;
                inner.version += 1;
            }
            inner.edit_label = None;
            self.commit_held(&mut inner);
            inner.snapshot()
```

Insert above `impl Drop for Session {`:

```rust
/// Editor edits (spec §3.2, §3.8).
#[uniffi::export]
impl Session {
    /// Applies an editor command: one undo step, or part of the open edit.
    /// `filter` hides kinds and lanes from commands that act on the
    /// selection.
    pub fn edit(
        &self,
        cmd: crate::edit::EditCommand,
        filter: Option<crate::edit::ViewFilterRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let c = crate::edit::to_command(&cmd)?;
            self.execute_with(c, &crate::edit::view_filter(filter.as_ref()))
        })
    }

    /// Inside the edit `begin_edit` opened (a drag): replaces everything the
    /// edit did so far with `cmd` (Plan Ruling 7), so each pointer move
    /// sends the full target. A refused `cmd` leaves the document as it was
    /// when the edit began, with the edit still open. `Model` error when no
    /// edit is open.
    pub fn edit_gesture(
        &self,
        cmd: crate::edit::EditCommand,
        filter: Option<crate::edit::ViewFilterRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let c = crate::edit::to_command(&cmd)?;
            {
                let mut inner = lock(&self.inner);
                let label = match (&inner.edit_label, inner.history.in_transaction()) {
                    (Some(l), true) => l.clone(),
                    _ => return Err(crate::events::bad("no edit is open")),
                };
                inner.history.abort()?;
                inner.history.begin(label)?;
                inner.version += 1;
            }
            match self.execute_with(c, &crate::edit::view_filter(filter.as_ref())) {
                Ok(s) => Ok(s),
                Err(e) => {
                    // The rewind changed the document: the engine follows.
                    lock(&self.inner).reload_host(self.id);
                    Err(e)
                }
            }
        })
    }

    /// Closes the open edit and restores the document from before it
    /// (Escape during a drag). Nothing happens when no edit is open.
    pub fn abort_edit(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                inner.history.abort()?;
                inner.version += 1;
                inner.reload_host(self.id);
            }
            inner.edit_label = None;
            self.commit_held(&mut inner);
            inner.snapshot()
        })
    }
}
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod editor;
pub mod error;
```
Replace:
```rust
pub mod edit;
pub mod editor;
pub mod error;
```

Find:
```rust
pub use editor::EditorSize;
```
Replace:
```rust
pub use edit::{
    AudioEdit, EditCommand, EventEdit, GeneratorRow, LaneEdit, LaneRef, QuantizeRow,
    ReferenceEdit, SequenceEdit, ViewFilterRow,
};
pub use editor::EditorSize;
```

- [ ] **Step 5: Run the Rust tests**

Run: `cargo test -p core-ffi --test edit && cargo test --workspace`
Expected: the 6 new tests pass. If a table entry in `every_variant_applies_and_undoes` fails because the core refuses the fixture's values, the panic names the command: fix the fixture value (not the mapping) unless the mapping is wrong.

- [ ] **Step 6: `FakeSession` edits**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`, after the `blocks(seq:lane:rule:)` member, add:

```swift

    // MARK: Editor edits (plan 4a)

    var edits: [EditCommand] = []
    var editError: VizError?
    func edit(cmd: EditCommand, filter: ViewFilterRow?) throws -> DocSnapshot {
        if let e = editError { throw e }
        edits.append(cmd)
        calls.append("edit")
        return commandResult()
    }

    var gestureEdits: [EditCommand] = []
    /// What the next `editGesture` calls throw, oldest first (nil succeeds).
    var gestureErrors: [VizError?] = []
    func editGesture(cmd: EditCommand, filter: ViewFilterRow?) throws -> DocSnapshot {
        let e = gestureErrors.isEmpty ? nil : gestureErrors.removeFirst()
        if let e {
            calls.append("gesture refused")
            throw e
        }
        gestureEdits.append(cmd)
        calls.append("gesture")
        return commandResult()
    }

    func abortEdit() throws -> DocSnapshot {
        calls.append("abort")
        return commandResult()
    }
```

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add crates/core-ffi/src/edit.rs crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/edit.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(core-ffi): editor commands, gesture replacement and abort

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 596 Rust tests, 93 Swift tests.

---

### Task 5: Selection, sequence templates and the editor layout in `ui.json`

**Files:**
- Create: `crates/core-ffi/src/selection.rs`, `crates/core-ffi/src/templates.rs`
- Modify: `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/lib.rs`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/selection.rs` (new)

**Interfaces:**
- Consumes: Task 4's `LaneRef`, `EditCommand`, `ViewFilterRow`, `ReferenceEdit::FromSelection`; `History::{selection, set_selection, begin, commit, abort, execute}`; `Selection::retain_existing`; `ui.json` helpers `read_windows` / `write_windows` / `UiState` in `session.rs`.
- Produces: `TickRange { start: i64, end: i64 }`; `SelectionRow { time_range: Option<TickRange>, lanes: Vec<LaneRef>, events: Vec<u64> }`; `selection::{row(&Selection) -> SelectionRow, selection(&SelectionRow) -> Selection}`; `SequenceTemplate { Empty, DrumKit, SameLanesAs { seq: u64 } }`; `templates::{DRUM_KIT, build(&mut History, &str, &SequenceTemplate) -> Result<SeqId, VizError>}`. Session: `selection() -> SelectionRow`, `set_selection(selection: SelectionRow)`, `new_sequence_from_template(name, template) -> DocSnapshot`, `ui_layout() -> Option<String>`, `set_ui_layout(json: String)`; `Inner.editor_layout: Option<serde_json::Value>`; `UiState.editor_layout`. Swift: `session.selection()`, `session.setSelection(selection:)`, `session.newSequenceFromTemplate(name:template:)`, `session.uiLayout()`, `session.setUiLayout(json:)`; `FakeSession.sel`, `selectionSets`, `templates`, `layout`.

Rulings 12, 13 and 19. `set_selection` drops lanes and events that do not exist and a range whose end is not after its start; it is never an undo step and does not bump `DocSnapshot.version`.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/selection.rs <<'VIZDAW_EOF'
//! Selection round trip, reference-from-selection with a view filter,
//! sequence templates and the editor layout in ui.json.

use vizdaw_ffi::{
    DocSnapshot, EditCommand, EventKindRow, LaneRef, ReferenceEdit, SelectionRow,
    SequenceTemplate, Session, TickRange, ViewFilterRow, VizError,
};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

fn id_of(s: &DocSnapshot, name: &str) -> u64 {
    s.sequences.iter().find(|r| r.name == name).unwrap().id
}

fn song() -> (std::sync::Arc<Session>, u64, u64) {
    let s = Session::new_demo().unwrap();
    let song = id_of(&s.snapshot().unwrap(), "Song");
    let chords = s.lanes(song).unwrap()[1].id;
    (s, song, chords)
}

#[test]
fn selection_round_trips_and_drops_unknown_ids() {
    let (s, song, chords) = song();
    let first = s.lane_events(song, chords).unwrap().events[0].id;
    s.set_selection(SelectionRow {
        time_range: Some(TickRange { start: 0, end: BAR }),
        lanes: vec![
            LaneRef { seq: song, lane: chords },
            LaneRef { seq: song, lane: 9_999 },
        ],
        events: vec![first, 9_999],
    })
    .unwrap();
    let sel = s.selection().unwrap();
    assert_eq!(sel.time_range, Some(TickRange { start: 0, end: BAR }));
    assert_eq!(sel.lanes, vec![LaneRef { seq: song, lane: chords }]);
    assert_eq!(sel.events, vec![first]);
}

#[test]
fn an_empty_or_inverted_range_is_dropped() {
    let (s, _, _) = song();
    for (start, end) in [(BAR, BAR), (BAR, 0)] {
        s.set_selection(SelectionRow {
            time_range: Some(TickRange { start, end }),
            lanes: vec![],
            events: vec![],
        })
        .unwrap();
        assert_eq!(s.selection().unwrap().time_range, None);
    }
}

#[test]
fn selecting_is_not_an_undo_step() {
    let (s, song, chords) = song();
    let before = s.snapshot().unwrap();
    s.set_selection(SelectionRow {
        time_range: Some(TickRange { start: 0, end: BAR }),
        lanes: vec![LaneRef { seq: song, lane: chords }],
        events: vec![],
    })
    .unwrap();
    let after = s.snapshot().unwrap();
    assert_eq!(after.version, before.version);
    assert_eq!(after.undo_label, None);
}

fn from_selection(song: u64) -> EditCommand {
    EditCommand::Reference {
        edit: ReferenceEdit::FromSelection {
            seq: song,
            per_lane: false,
            round_to_bars: true,
            loop_count: 1,
        },
    }
}

#[test]
fn a_reference_is_made_from_the_selection() {
    let (s, song, chords) = song();
    let sequences = s.snapshot().unwrap().sequences.len();
    s.set_selection(SelectionRow {
        time_range: Some(TickRange { start: 10, end: 4 * BAR - 10 }),
        lanes: vec![LaneRef { seq: song, lane: chords }],
        events: vec![],
    })
    .unwrap();
    let snap = s.edit(from_selection(song), None).unwrap();
    assert_eq!(snap.sequences.len(), sequences + 1);
    let rows = s.lane_events(song, chords).unwrap().events;
    let r = rows.iter().find(|r| r.kind == EventKindRow::Reference).unwrap();
    assert_eq!((r.tick, r.duration), (0, 4 * BAR), "rounded to bars");
    assert!(
        !rows
            .iter()
            .any(|r| r.kind == EventKindRow::Note && r.tick < 4 * BAR)
    );
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().sequences.len(), sequences);
}

#[test]
fn the_view_filter_keeps_hidden_kinds_in_place() {
    let (s, song, chords) = song();
    s.set_selection(SelectionRow {
        time_range: Some(TickRange { start: 0, end: 4 * BAR }),
        lanes: vec![LaneRef { seq: song, lane: chords }],
        events: vec![],
    })
    .unwrap();
    let filter = ViewFilterRow {
        hidden_kinds: vec![EventKindRow::Note],
        hidden_lanes: vec![],
    };
    s.edit(from_selection(song), Some(filter)).unwrap();
    let rows = s.lane_events(song, chords).unwrap().events;
    assert!(
        rows.iter()
            .any(|r| r.kind == EventKindRow::Note && r.tick < 4 * BAR),
        "hidden notes stay in the lane"
    );
}

#[test]
fn templates_make_one_undo_step() {
    let (s, song, _) = song();
    let n = s.snapshot().unwrap().sequences.len();
    let snap = s
        .new_sequence_from_template("Plain".into(), SequenceTemplate::Empty)
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("New Sequence from Template"));
    let plain = id_of(&snap, "Plain");
    assert_eq!(s.lanes(plain).unwrap().len(), 1);

    let snap = s
        .new_sequence_from_template("Kit".into(), SequenceTemplate::DrumKit)
        .unwrap();
    let kit = s.lanes(id_of(&snap, "Kit")).unwrap();
    assert_eq!(kit.len(), 1);
    assert!(kit[0].is_drum);
    assert_eq!(kit[0].name, "Drums");

    let snap = s
        .new_sequence_from_template("Shape".into(), SequenceTemplate::SameLanesAs { seq: song })
        .unwrap();
    let shape = id_of(&snap, "Shape");
    let (a, b) = (s.lanes(song).unwrap(), s.lanes(shape).unwrap());
    let names = |v: &[vizdaw_ffi::LaneRow]| v.iter().map(|l| l.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&a), names(&b));
    assert_eq!(
        a.iter().map(|l| l.midi_dest).collect::<Vec<_>>(),
        b.iter().map(|l| l.midi_dest).collect::<Vec<_>>()
    );
    assert!(b.iter().all(|l| l.midi_event_count == 0 && l.reference_count == 0));
    assert_eq!(
        s.sequence_info(song).unwrap().tempo,
        s.sequence_info(shape).unwrap().tempo
    );
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().sequences.len(), n + 2);
    assert!(matches!(
        s.new_sequence_from_template("X".into(), SequenceTemplate::SameLanesAs { seq: 9_999 }),
        Err(VizError::Model { .. })
    ));
    assert_eq!(s.snapshot().unwrap().sequences.len(), n + 2, "a failed template leaves nothing");
}

#[test]
fn the_editor_layout_is_saved_in_ui_json() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_demo().unwrap();
    assert_eq!(s.ui_layout().unwrap(), None);
    s.set_ui_layout(r#"{"version":1,"sequenceOrder":[3,1,2]}"#.into())
        .unwrap();
    assert!(matches!(
        s.set_ui_layout("not json".into()),
        Err(VizError::Model { .. })
    ));
    let before = s.snapshot().unwrap();
    assert!(!before.dirty, "the layout does not mark the document edited");
    let path = dir.path().join("layout.vizdaw");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    let text = std::fs::read_to_string(path.join("ui.json")).unwrap();
    assert!(text.contains("open_plugin_windows"), "{text}");
    let reopened = Session::open(path.to_string_lossy().into_owned()).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&reopened.ui_layout().unwrap().unwrap()).unwrap();
    assert_eq!(json["sequenceOrder"], serde_json::json!([3, 1, 2]));
}
VIZDAW_EOF
```

The tests use `serde_json`, already a normal dependency of `core-ffi` (integration tests see it), so the manifest does not change.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p core-ffi --test selection`
Expected: compile errors — unresolved `SelectionRow`, `SequenceTemplate`, `TickRange`, no method `set_selection`.

- [ ] **Step 3: Write `selection.rs` and `templates.rs`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/selection.rs <<'VIZDAW_EOF'
//! The document-wide selection as the app sees it (spec §3.3).

use crate::edit::LaneRef;
use core_model::ids::{EventId, LaneId, SeqId};
use core_model::selection::Selection;
use core_model::time::Tick;

/// A `[start, end)` tick range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct TickRange {
    /// First tick.
    pub start: i64,
    /// End tick (exclusive).
    pub end: i64,
}

/// The selection: an edit range, lanes and events.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct SelectionRow {
    /// The edit-in/edit-out range.
    pub time_range: Option<TickRange>,
    /// Selected lanes.
    pub lanes: Vec<LaneRef>,
    /// Selected events.
    pub events: Vec<u64>,
}

/// The selection as a row.
pub fn row(s: &Selection) -> SelectionRow {
    SelectionRow {
        time_range: s.time_range.map(|(a, b)| TickRange {
            start: a.0,
            end: b.0,
        }),
        lanes: s
            .lanes
            .iter()
            .map(|(seq, lane)| LaneRef {
                seq: seq.0,
                lane: lane.0,
            })
            .collect(),
        events: s.events.iter().map(|e| e.0).collect(),
    }
}

/// A row as a selection; a range whose end is not after its start is
/// dropped. Unknown IDs are kept here (the caller drops them against the
/// document).
pub fn selection(r: &SelectionRow) -> Selection {
    Selection {
        time_range: r
            .time_range
            .filter(|t| t.end > t.start)
            .map(|t| (Tick(t.start), Tick(t.end))),
        lanes: r
            .lanes
            .iter()
            .map(|l| (SeqId(l.seq), LaneId(l.lane)))
            .collect(),
        events: r.events.iter().copied().map(EventId).collect(),
    }
}
VIZDAW_EOF
cat > crates/core-ffi/src/templates.rs <<'VIZDAW_EOF'
//! "New from Template" (parent spec §9, Plan Ruling 13): a new sequence
//! built from core commands inside the caller's transaction.

use crate::error::VizError;
use crate::events::bad;
use core_model::command::drum::DrumCmd;
use core_model::command::lane::LaneCmd;
use core_model::command::sequence::SequenceCmd;
use core_model::command::{Command, Report};
use core_model::history::History;
use core_model::ids::{AnyId, LaneId, SeqId};
use core_model::selection::ViewFilter;

/// What a new sequence starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SequenceTemplate {
    /// One empty lane.
    Empty,
    /// One drum lane "Drums" with General MIDI rows.
    DrumKit,
    /// The lanes (names, routing, drum rows, own length), tempo, meter and
    /// start offset of sequence `seq`, without events.
    SameLanesAs {
        /// The sequence to copy the shape of.
        seq: u64,
    },
}

/// The drum kit template's rows: name and General MIDI note.
pub const DRUM_KIT: [(&str, u8); 5] = [
    ("Kick", 36),
    ("Snare", 38),
    ("Closed Hat", 42),
    ("Open Hat", 46),
    ("Crash", 49),
];

fn lane_of(r: &Report) -> Result<LaneId, VizError> {
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .ok_or_else(|| bad("no lane was created"))
}

/// Builds sequence `name` from `template` in `history` (the caller holds an
/// open transaction and aborts it on error). Returns the new sequence.
pub fn build(
    history: &mut History,
    name: &str,
    template: &SequenceTemplate,
) -> Result<SeqId, VizError> {
    let source = match template {
        SequenceTemplate::SameLanesAs { seq } => Some(history.doc().sequence(SeqId(*seq))?.clone()),
        _ => None,
    };
    let filter = ViewFilter::default();
    let mut run = |cmd: Command| history.execute(&cmd, &filter).map_err(VizError::from);
    let r = run(Command::Sequence(SequenceCmd::New {
        name: name.to_string(),
    }))?;
    let seq = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .ok_or_else(|| bad("no sequence was created"))?;
    let first = lane_of(&r)?;
    match (template, source) {
        (SequenceTemplate::DrumKit, _) => {
            run(Command::Lane(LaneCmd::Rename {
                seq,
                lane: first,
                name: "Drums".into(),
            }))?;
            run(Command::Drum(DrumCmd::MarkDrumLane {
                seq,
                lane: first,
                drum: true,
            }))?;
            for (row, note) in DRUM_KIT {
                run(Command::Drum(DrumCmd::AddRow {
                    seq,
                    lane: first,
                    name: row.into(),
                    note,
                    dest: None,
                }))?;
            }
        }
        (SequenceTemplate::SameLanesAs { .. }, Some(src)) => {
            run(Command::Sequence(SequenceCmd::SetTempo {
                seq,
                tempo: src.tempo.clone(),
            }))?;
            run(Command::Sequence(SequenceCmd::SetMeter {
                seq,
                meter: src.meter.clone(),
            }))?;
            run(Command::Sequence(SequenceCmd::SetStartOffset {
                seq,
                offset: src.start_offset,
            }))?;
            for (i, l) in src.lanes.iter().enumerate() {
                let lane = if i == 0 {
                    first
                } else {
                    lane_of(&run(Command::Lane(LaneCmd::Add {
                        seq,
                        name: l.name.clone(),
                        index: None,
                    }))?)?
                };
                run(Command::Lane(LaneCmd::Rename {
                    seq,
                    lane,
                    name: l.name.clone(),
                }))?;
                run(Command::Lane(LaneCmd::SetDest {
                    seq,
                    lane,
                    dest: l.midi_dest,
                }))?;
                run(Command::Lane(LaneCmd::SetOutput {
                    seq,
                    lane,
                    output: l.audio_out,
                }))?;
                if let Some(length) = l.length {
                    run(Command::Lane(LaneCmd::SetLength {
                        seq,
                        lane,
                        length: Some(length),
                        looped: l.looped,
                        locked: l.length_locked,
                    }))?;
                }
                if let Some(drum) = &l.drum {
                    run(Command::Drum(DrumCmd::MarkDrumLane {
                        seq,
                        lane,
                        drum: true,
                    }))?;
                    for row in &drum.rows {
                        run(Command::Drum(DrumCmd::AddRow {
                            seq,
                            lane,
                            name: row.name.clone(),
                            note: row.note,
                            dest: row.dest,
                        }))?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(seq)
}
VIZDAW_EOF
```

`MarkDrumLane` may already create rows for the notes a lane holds; the new lanes hold no notes, so every row comes from `AddRow`. If `AddRow` refuses a duplicate row (same note and destination), the source lane cannot have had one either.

- [ ] **Step 4: Session: selection, templates, layout**

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// The label of the edit `begin_edit` opened (`edit_gesture` reopens
    /// it under the same label).
    edit_label: Option<String>,
}
```
Replace:
```rust
    /// The label of the edit `begin_edit` opened (`edit_gesture` reopens
    /// it under the same label).
    edit_label: Option<String>,
    /// The editors' layout, opaque JSON from the app, saved in `ui.json`
    /// (plan 4a Ruling 12).
    editor_layout: Option<serde_json::Value>,
}
```

Find:
```rust
            edit_label: None,
```
Replace:
```rust
            edit_label: None,
            editor_layout: None,
```

Find:
```rust
        file::save(inner.doc(), path)?;
        write_windows(path, &inner.windows);
```
Replace:
```rust
        file::save(inner.doc(), path)?;
        write_windows(path, &inner.windows, inner.editor_layout.as_ref());
```

Find:
```rust
            let windows = read_windows(&p);
            let s = Session::with(doc, Some(p), options.plugins_off)?;
            {
                let mut inner = lock(&s.inner);
                inner.windows = windows;
```
Replace:
```rust
            let windows = read_windows(&p);
            let layout = read_layout(&p);
            let s = Session::with(doc, Some(p), options.plugins_off)?;
            {
                let mut inner = lock(&s.inner);
                inner.windows = windows;
                inner.editor_layout = layout;
```

Find:
```rust
fn read_windows(package: &Path) -> std::collections::BTreeSet<u64> {
    std::fs::read_to_string(package.join(UI_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<UiState>(&t).ok())
        .map(|u| u.open_plugin_windows.into_iter().collect())
        .unwrap_or_default()
}

/// Best-effort: a project whose window list cannot be written still saved.
fn write_windows(package: &Path, windows: &std::collections::BTreeSet<u64>) {
    let ui = UiState {
        open_plugin_windows: windows.iter().copied().collect(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&ui) {
        let _ = file::write_atomic(&package.join(UI_FILE), text.as_bytes());
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UiState {
    open_plugin_windows: Vec<u64>,
}
```
Replace:
```rust
fn read_ui(package: &Path) -> Option<UiState> {
    std::fs::read_to_string(package.join(UI_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<UiState>(&t).ok())
}

fn read_windows(package: &Path) -> std::collections::BTreeSet<u64> {
    read_ui(package)
        .map(|u| u.open_plugin_windows.into_iter().collect())
        .unwrap_or_default()
}

fn read_layout(package: &Path) -> Option<serde_json::Value> {
    read_ui(package).and_then(|u| u.editor_layout)
}

/// Best-effort: a project whose window list cannot be written still saved.
fn write_windows(
    package: &Path,
    windows: &std::collections::BTreeSet<u64>,
    layout: Option<&serde_json::Value>,
) {
    let ui = UiState {
        open_plugin_windows: windows.iter().copied().collect(),
        editor_layout: layout.cloned(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&ui) {
        let _ = file::write_atomic(&package.join(UI_FILE), text.as_bytes());
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UiState {
    open_plugin_windows: Vec<u64>,
    /// The editors' layout (plan 4a); older files have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editor_layout: Option<serde_json::Value>,
}
```

Insert above `impl Drop for Session {`:

```rust
/// The selection (spec §3.3), sequence templates and the editor layout.
#[uniffi::export]
impl Session {
    /// The document-wide selection.
    pub fn selection(&self) -> Result<crate::selection::SelectionRow, VizError> {
        ffi_guard(|| Ok(crate::selection::row(lock(&self.inner).history.selection())))
    }

    /// Replaces the selection (not an undo step); unknown lanes and events
    /// are dropped.
    pub fn set_selection(&self, selection: crate::selection::SelectionRow) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let sel = crate::selection::selection(&selection).retain_existing(inner.doc());
            inner.history.set_selection(sel);
            Ok(())
        })
    }

    /// A new sequence from `template` named `name`, as one undo step.
    pub fn new_sequence_from_template(
        &self,
        name: String,
        template: crate::templates::SequenceTemplate,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            inner.history.begin("New Sequence from Template")?;
            match crate::templates::build(&mut inner.history, &name, &template) {
                Ok(_) => inner.history.commit()?,
                Err(e) => {
                    inner.history.abort()?;
                    return Err(e);
                }
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.snapshot()
        })
    }

    /// The editor layout saved with the project (JSON), if any.
    pub fn ui_layout(&self) -> Result<Option<String>, VizError> {
        ffi_guard(|| {
            Ok(lock(&self.inner)
                .editor_layout
                .as_ref()
                .map(|v| v.to_string()))
        })
    }

    /// Replaces the editor layout (JSON; written by the next save, never an
    /// undo step).
    pub fn set_ui_layout(&self, json: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let v: serde_json::Value = serde_json::from_str(&json)
                .map_err(|e| crate::events::bad(&format!("the layout is not JSON: {e}")))?;
            lock(&self.inner).editor_layout = Some(v);
            Ok(())
        })
    }
}
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod revs;
pub mod session;
```
Replace:
```rust
pub mod revs;
pub mod selection;
pub mod session;
```

Find:
```rust
pub mod snapshot;
```
Replace:
```rust
pub mod snapshot;
pub mod templates;
```

Find:
```rust
pub use revs::LaneRev;
```
Replace:
```rust
pub use revs::LaneRev;
pub use selection::{SelectionRow, TickRange};
pub use templates::SequenceTemplate;
```

- [ ] **Step 5: Run the Rust tests**

Run: `cargo test -p core-ffi --test selection && cargo test --workspace`
Expected: the 7 new tests pass.

- [ ] **Step 6: `FakeSession` selection, templates, layout**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`, after `abortEdit()` (Task 4), add:

```swift

    // MARK: Selection, templates, layout (plan 4a)

    var sel = SelectionRow(timeRange: nil, lanes: [], events: [])
    var selectionSets: [SelectionRow] = []
    func selection() throws -> SelectionRow { sel }
    func setSelection(selection: SelectionRow) throws {
        selectionSets.append(selection)
        sel = selection
    }

    var templates: [(name: String, template: SequenceTemplate)] = []
    func newSequenceFromTemplate(name: String, template: SequenceTemplate) throws -> DocSnapshot {
        templates.append((name, template))
        return commandResult()
    }

    var layout: String?
    func uiLayout() throws -> String? { layout }
    func setUiLayout(json: String) throws { layout = json }
```

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add crates/core-ffi/src/selection.rs crates/core-ffi/src/templates.rs \
  crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs crates/core-ffi/tests/selection.rs \
  app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(core-ffi): selection, sequence templates and the editor layout

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 603 Rust tests, 93 Swift tests.

---

### Task 6: Audition and the transport's host-time stamp

**Files:**
- Modify: `crates/core-engine/src/render.rs`, `crates/core-engine/src/engine.rs`
- Create: `crates/core-ffi/src/clock.rs`
- Modify: `crates/core-ffi/src/host.rs`, `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/snapshot.rs`, `crates/core-ffi/src/lib.rs`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `app/Tests/VizDAWAppModelTests/FormattingTests.swift`
- Test: `crates/core-engine/tests/audition.rs` (new), `crates/core-ffi/tests/audition.rs` (new)

**Interfaces:**
- Consumes: `core_engine::midi1::{Midi1Msg, note_on_velocity, release_velocity}`, `schedule::targets::kinds_for`, `TargetTable::find`, `Out::send`, `ActiveNotes::observe`; `platform_macos::midi::now_ns`; `TempoMap::tick_to_seconds`.
- Produces: `EngineCmd::Audition(TargetIdx, Midi1Msg)`; `Engine::audition(&mut self, &Document, DestId, pitch: u8, velocity: u16, on: bool) -> Result<bool, EngineError>`; `vizdaw_ffi::host::audition(owner, &Document, DestId, u8, u16, bool) -> Result<bool, VizError>`; `clock::{now_ns() -> u64, ticks_per_second(&Document, Option<SeqId>, Tick) -> f64}`; `TransportState.host_time_ns: u64`, `TransportState.ticks_per_second: f64` (last fields); `Session::audition(dest, pitch, velocity, on) -> bool`. Swift: `session.audition(dest:pitch:velocity:on:) -> Bool`; `TransportState(…, hostTimeNs:, ticksPerSecond:)`; `FakeSession.auditions`.

Rulings 9 and 10. The audition message is applied at the start of the next block at offset 0 and observed by the active-note tracker, so Stop, Locate and a loop wrap release a held audition note like any other.

- [ ] **Step 1: Write the failing engine test**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/audition.rs <<'VIZDAW_EOF'
//! Editor audition (spec §3.2): a note straight to a destination's target.

use core_engine::BackendError;
use core_engine::backend::{BlockTime, InstrumentNode, MidiPortInfo, StereoBuf};
use core_engine::engine::{Engine, EngineConfig, SinkFactory};
use core_engine::fixtures::{Builder, Q};
use core_engine::render::{Renderer, Sink};
use core_engine::schedule::{TargetIdx, TargetKind};
use core_model::document::Document;
use core_model::ids::{DestId, SeqId};
use std::sync::{Arc, Mutex};

type Seen = Arc<Mutex<Vec<(u64, Vec<u8>)>>>;

struct Node {
    seen: Seen,
    rendered: u64,
}

impl InstrumentNode for Node {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        self.seen
            .lock()
            .unwrap()
            .push((self.rendered + u64::from(offset), msg.to_vec()));
    }
    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        out.left[..frames as usize].fill(0.0);
        out.right[..frames as usize].fill(0.0);
        self.rendered += u64::from(frames);
    }
    fn reset(&mut self) {}
}

#[derive(Default, Clone)]
struct Factory {
    synth: Seen,
}

impl SinkFactory for Factory {
    fn ports(&self) -> Vec<MidiPortInfo> {
        vec![]
    }
    fn make(
        &mut self,
        _target: TargetIdx,
        _kind: &TargetKind,
        _sr: f64,
        _max: u32,
    ) -> Result<Sink, BackendError> {
        Ok(Sink::Instrument(Box::new(Node {
            seen: self.synth.clone(),
            rendered: 0,
        })))
    }
}

/// "Synth" (channel index 2) is used by the sequence; "Virtual" plays on
/// the virtual source, a target this sequence never loads.
fn doc() -> (Document, SeqId, DestId, DestId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 2).unwrap();
    let unused = b.dest("Virtual", "virtual:out", 5).unwrap();
    let (seq, lane) = b.sequence("S", "L", Some(synth)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q)]).unwrap();
    (b.finish(), seq, synth, unused)
}

fn blocks(r: &mut Renderer, n: usize, ns: &mut u64) {
    let mut l = vec![0.0f32; 256];
    let mut rr = vec![0.0f32; 256];
    for _ in 0..n {
        let mut chans: [&mut [f32]; 2] = [&mut l, &mut rr];
        r.process(
            &mut chans,
            256,
            BlockTime {
                host_time_ns: *ns,
                sample_rate: 48_000.0,
            },
        );
        *ns += 256 * 1_000_000_000 / 48_000;
    }
}

#[test]
fn an_audition_note_sounds_and_stop_releases_it() {
    let (doc, seq, synth, _) = doc();
    let f = Factory::default();
    let (mut engine, mut r) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone())).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut ns = 0;
    blocks(&mut r, 1, &mut ns);
    assert!(engine.audition(&doc, synth, 64, 0x8000, true).unwrap());
    blocks(&mut r, 1, &mut ns);
    engine.stop().unwrap();
    blocks(&mut r, 1, &mut ns);
    let seen = f.synth.lock().unwrap().clone();
    assert!(seen.contains(&(256, vec![0x92, 64, 64])), "{seen:?}");
    assert!(seen.contains(&(512, vec![0x82, 64, 0])), "stop releases: {seen:?}");
}

#[test]
fn an_audition_note_off_ends_the_note() {
    let (doc, seq, synth, _) = doc();
    let f = Factory::default();
    let (mut engine, mut r) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone())).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut ns = 0;
    engine.audition(&doc, synth, 64, 0x8000, true).unwrap();
    blocks(&mut r, 1, &mut ns);
    engine.audition(&doc, synth, 64, 0x2000, false).unwrap();
    blocks(&mut r, 1, &mut ns);
    engine.stop().unwrap();
    blocks(&mut r, 1, &mut ns);
    let seen = f.synth.lock().unwrap().clone();
    let offs = seen
        .iter()
        .filter(|(_, b)| b.len() == 3 && b[0] == 0x82 && b[1] == 64)
        .count();
    assert_eq!(offs, 1, "released once, not again at stop: {seen:?}");
}

#[test]
fn destinations_without_a_loaded_target_are_not_auditioned() {
    let (doc, seq, _, unused) = doc();
    let (mut engine, _r) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(Factory::default())).unwrap();
    engine.load(&doc, seq).unwrap();
    assert!(!engine.audition(&doc, unused, 64, 0x8000, true).unwrap());
    assert!(!engine.audition(&doc, DestId(9_999), 64, 0x8000, true).unwrap());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p core-engine --test audition`
Expected: compile error — no method `audition` on `Engine`.

- [ ] **Step 3: The audition command**

In `crates/core-engine/src/render.rs`, add `use crate::midi1::Midi1Msg;` below `use crate::backend::{BlockInfo, BlockTime, MidiOut, StereoBuf};`.

Find:
```rust
    /// Remove a released target's sink; it goes back through the retire
    /// ring (a plugin instrument is deactivated off the audio thread).
    RemoveSink(TargetIdx),
}
```
Replace:
```rust
    /// Remove a released target's sink; it goes back through the retire
    /// ring (a plugin instrument is deactivated off the audio thread).
    RemoveSink(TargetIdx),
    /// Send a note on or off to a target now (editor audition), tracked
    /// like any played note so Stop and Locate release it.
    Audition(TargetIdx, Midi1Msg),
}
```

Find:
```rust
            EngineCmd::AddSink(idx, sink) => {
```
Replace:
```rust
            EngineCmd::Audition(target, msg) => {
                let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
                let mut o = Out {
                    sinks: &mut self.sinks,
                    shared: &self.shared,
                    block_ns: self.block_ns,
                    ns_per_frame: 0.0,
                    late_ns,
                };
                o.send(target, 0, msg.as_slice());
                self.notes.observe(target, msg.as_slice());
            }
            EngineCmd::AddSink(idx, sink) => {
```

In `crates/core-engine/src/engine.rs`, extend the imports:

Find:
```rust
use core_model::ids::{PluginInstanceId, SeqId, StripId};
```
Replace:
```rust
use crate::midi1::{Midi1Msg, note_on_velocity, release_velocity};
use crate::schedule::targets::kinds_for;
use core_model::ids::{DestId, PluginInstanceId, SeqId, StripId};
```

Find:
```rust
    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
```
Replace:
```rust
    /// Sends a note on (`on`) or off for `pitch` straight to the targets
    /// `dest` plays on, without touching the document (editor audition,
    /// spec §3.2). Returns false when this engine has none of them (the
    /// loaded sequence does not use that target) or `dest` is unknown.
    pub fn audition(
        &mut self,
        doc: &Document,
        dest: DestId,
        pitch: u8,
        velocity: u16,
        on: bool,
    ) -> Result<bool, EngineError> {
        let Some(d) = doc.instruments().get(&dest) else {
            return Ok(false);
        };
        let channel = match &d.kind {
            DestKind::MidiEndpoint { channel, .. } => channel & 0x0F,
            DestKind::PluginInstrument { .. } => 0,
        };
        let Ok(kinds) = kinds_for(&d.kind, self.cfg.route, &self.factory.ports()) else {
            return Ok(false);
        };
        let pitch = pitch & 0x7F;
        let msg = if on {
            Midi1Msg::three(0x90 | channel, pitch, note_on_velocity(velocity))
        } else {
            Midi1Msg::three(0x80 | channel, pitch, release_velocity(velocity))
        };
        let mut sent = false;
        for k in &kinds {
            if let Some(t) = self.targets.find(k) {
                self.push(EngineCmd::Audition(t, msg))?;
                sent = true;
            }
        }
        Ok(sent)
    }

    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
```

- [ ] **Step 4: Run the engine tests**

Run: `cargo test -p core-engine --test audition && cargo test -p core-engine`
Expected: PASS. The RT allocation tests (`rt_alloc*`) still pass: the new arm allocates nothing.

- [ ] **Step 5: Write the failing FFI test**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/audition.rs <<'VIZDAW_EOF'
//! Audition through the session and the transport's host-time stamp
//! (spec §3.2, §3.4). The engine host is process-wide: one test at a time.

use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::time::Duration;
use vizdaw_ffi::Session;
use vizdaw_ffi::host::{drain_offline_log, force_offline, reset_host};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

fn demo() -> (std::sync::Arc<Session>, u64, u64) {
    let s = Session::new_demo().unwrap();
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    let piano = s
        .destinations()
        .unwrap()
        .iter()
        .find(|d| d.name == "Built-in Piano")
        .unwrap()
        .id;
    (s, song, piano)
}

#[test]
fn audition_needs_the_engine_and_then_sounds() {
    let _g = serial();
    let (s, song, piano) = demo();
    assert!(
        !s.audition(piano, 72, 0x8000, true).unwrap(),
        "no engine before the first Play (Plan Ruling 9)"
    );
    s.play(song).unwrap();
    sleep(Duration::from_millis(50));
    drain_offline_log();
    assert!(s.audition(piano, 72, 0x8000, true).unwrap());
    sleep(Duration::from_millis(100));
    assert!(s.audition(piano, 72, 0, false).unwrap());
    sleep(Duration::from_millis(100));
    s.stop().unwrap();
    let log = drain_offline_log();
    assert!(log.iter().any(|(_, b)| b == &vec![0x90, 72, 64]), "{log:?}");
    assert!(log.iter().any(|(_, b)| b == &vec![0x80, 72, 0]), "{log:?}");
    reset_host();
}

#[test]
fn the_transport_carries_host_time_and_tempo() {
    let _g = serial();
    let (s, _, _) = demo();
    let a = s.transport().unwrap();
    sleep(Duration::from_millis(2));
    let b = s.transport().unwrap();
    assert!(b.host_time_ns > a.host_time_ns);
    assert_eq!(a.ticks_per_second, 30_720.0, "120 BPM");
    // Song ramps from 120 to 90 BPM over its last two bars.
    s.locate(12 * BAR - Q).unwrap();
    let t = s.transport().unwrap();
    assert!(
        t.ticks_per_second < 30_720.0 && t.ticks_per_second > 23_000.0,
        "{}",
        t.ticks_per_second
    );
}
VIZDAW_EOF
```

Run: `cargo test -p core-ffi --test audition`
Expected: compile errors — no method `audition`, no field `host_time_ns`.

- [ ] **Step 6: Clock, host and session**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/clock.rs <<'VIZDAW_EOF'
//! Host time and tempo for the playhead (spec §3.4, Plan Ruling 10).

use core_model::document::Document;
use core_model::ids::SeqId;
use core_model::time::{PPQ, Tick};

/// Now on the host clock in nanoseconds. On macOS this is CoreAudio host
/// time, the clock of `CACurrentMediaTime` and `CADisplayLink.timestamp`;
/// elsewhere nanoseconds since the first call.
pub fn now_ns() -> u64 {
    #[cfg(target_os = "macos")]
    {
        platform_macos::midi::now_ns()
    }
    #[cfg(not(target_os = "macos"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        u64::try_from(START.get_or_init(Instant::now).elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

/// Ticks per second at `tick` of `seq` through its tempo map, measured
/// over the next quarter note (30 720 at 120 BPM, also when `seq` is
/// unknown).
pub fn ticks_per_second(doc: &Document, seq: Option<SeqId>, tick: Tick) -> f64 {
    let default = 2.0 * PPQ as f64;
    let Some(s) = seq.and_then(|s| doc.sequence(s).ok()) else {
        return default;
    };
    let d = s.tempo.tick_to_seconds(tick + Tick(PPQ)) - s.tempo.tick_to_seconds(tick);
    if d.is_finite() && d > 0.0 {
        PPQ as f64 / d
    } else {
        default
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/src/snapshot.rs`:

Find:
```rust
    /// Messages of the last compile report.
    pub report: Vec<String>,
}
```
Replace:
```rust
    /// Messages of the last compile report.
    pub report: Vec<String>,
    /// Host time (ns; `CACurrentMediaTime`'s clock on macOS) when `tick`
    /// was read.
    pub host_time_ns: u64,
    /// Tempo at `tick` in ticks per second.
    pub ticks_per_second: f64,
}
```

In `crates/core-ffi/src/session.rs`, in `transport()`:

Find:
```rust
                    report: h.report,
                },
```
Replace:
```rust
                    report: h.report,
                    host_time_ns: crate::clock::now_ns(),
                    ticks_per_second: crate::clock::ticks_per_second(doc, Some(h.seq), h.tick),
                },
```

Find:
```rust
                        faults: inner.reload_fault.iter().cloned().collect(),
                        report: Vec::new(),
                    }
```
Replace:
```rust
                        faults: inner.reload_fault.iter().cloned().collect(),
                        report: Vec::new(),
                        host_time_ns: crate::clock::now_ns(),
                        ticks_per_second: crate::clock::ticks_per_second(doc, seq, tick),
                    }
```

In `crates/core-ffi/src/host.rs`, add `use core_model::ids::DestId;` beside `use core_model::ids::SeqId;` (merge into `use core_model::ids::{DestId, SeqId};`), and after `pub fn set_loop(…) { … }` add:

```rust
/// Sends an audition note through `owner`'s engine (Plan Ruling 9); false
/// when `owner` does not hold the engine or it has no target for `dest`.
pub fn audition(
    owner: u64,
    doc: &Document,
    dest: DestId,
    pitch: u8,
    velocity: u16,
    on: bool,
) -> Result<bool, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => Ok(h.engine.audition(doc, dest, pitch, velocity, on)?),
        _ => Ok(false),
    }
}
```

In `crates/core-ffi/src/session.rs`, inside the exported `impl Session` that holds `transport()`, after `transport()`:

Find:
```rust
    /// Where the transport stands.
    pub fn transport(&self) -> Result<TransportState, VizError> {
```
Replace:
```rust
    /// Sounds (`on`) or releases `pitch` on destination `dest` without
    /// touching the document (spec §3.2). True when it was sent; false
    /// when this session does not hold the engine or the loaded sequence
    /// does not play on `dest`'s target (Plan Ruling 9).
    pub fn audition(&self, dest: u64, pitch: u8, velocity: u16, on: bool) -> Result<bool, VizError> {
        self.transport_call(|| {
            let inner = lock(&self.inner);
            host::audition(self.id, inner.doc(), DestId(dest), pitch, velocity, on)
        })
    }

    /// Where the transport stands.
    pub fn transport(&self) -> Result<TransportState, VizError> {
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod catalog;
```
Replace:
```rust
pub mod catalog;
pub mod clock;
```

- [ ] **Step 7: Run the Rust tests**

Run: `cargo test -p core-ffi --test audition && cargo test --workspace`
Expected: PASS (5 new tests in all).

- [ ] **Step 8: Follow `TransportState` in Swift**

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
        playing: false, seq: nil, tick: 0, positionBbu: "1.1.0", seconds: 0,
        sampleRate: 0, device: "", faults: [], report: [])
```
Replace:
```swift
        playing: false, seq: nil, tick: 0, positionBbu: "1.1.0", seconds: 0,
        sampleRate: 0, device: "", faults: [], report: [], hostTimeNs: 0,
        ticksPerSecond: 30_720)
```

Make the same replacement in `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the `var state = TransportState(` initialiser, same two lines) and in `app/Tests/VizDAWAppModelTests/FormattingTests.swift` `deviceBeforeFirstPlay`. In `FormattingTests.positionCombinesBbuAndTime`:

Find:
```swift
            sampleRate: 48_000, device: "Offline", faults: [], report: [])
```
Replace:
```swift
            sampleRate: 48_000, device: "Offline", faults: [], report: [], hostTimeNs: 0,
            ticksPerSecond: 30_720)
```

In `FakeSession.swift`, after `setUiLayout(json:)` (Task 5), add:

```swift

    var auditions: [(dest: UInt64, pitch: UInt8, on: Bool)] = []
    func audition(dest: UInt64, pitch: UInt8, velocity: UInt16, on: Bool) throws -> Bool {
        auditions.append((dest, pitch, on))
        return true
    }
```

- [ ] **Step 9: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add crates/core-engine/src/render.rs crates/core-engine/src/engine.rs \
  crates/core-engine/tests/audition.rs crates/core-ffi/src/clock.rs crates/core-ffi/src/host.rs \
  crates/core-ffi/src/session.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/audition.rs app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Tests/VizDAWAppModelTests/FakeSession.swift app/Tests/VizDAWAppModelTests/FormattingTests.swift
git commit -m "feat(engine, core-ffi): audition and the transport host-time stamp

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 93 Swift tests.

---

### Task 7: The `VizDAWEditors` target, `TimeAxis`, grid, meter maths and snap

**Files:**
- Modify: `app/Package.swift`
- Create: `app/Sources/VizDAWAppModel/MeterMath.swift`, `app/Sources/VizDAWAppModel/Snap.swift`
- Create: `app/Sources/VizDAWEditors/TimeAxis.swift`, `app/Sources/VizDAWEditors/Grid.swift`
- Test: `app/Tests/VizDAWAppModelTests/MeterMathTests.swift`, `app/Tests/VizDAWAppModelTests/SnapTests.swift`, `app/Tests/VizDAWEditorsTests/TimeAxisTests.swift`, `app/Tests/VizDAWEditorsTests/GridTests.swift` (all new)

**Interfaces:**
- Consumes: Task 2's `MeterRow` (generated Swift: `MeterRow(tick: Int64, num: UInt8, den: UInt8, sharps: Int8, minor: Bool)`).
- Produces: `MeterMath.{ppq, beatTicks(_:), barTicks(_:), bar(at:_:) -> MeterMath.Bar, barStart(_:_:) -> Int64}`, `MeterMath.Bar { number: Int64, start: Int64, length: Int64, meter: MeterRow }`; `Snap: Codable, Equatable, Sendable { division: Snap.Division, triplet, dotted, enabled; step(in: MeterRow) -> Int64; apply(_ tick: Int64, meter: [MeterRow]) -> Int64 }`, `Snap.Division { bar, half, quarter, eighth, sixteenth, thirtySecond }` with `title`; `TimeAxis { ticksPerPoint, originTick; init(ticksPerPoint:originTick:); x(_:) -> CGFloat; tick(atX:) -> Int64; width(ofTicks:) -> CGFloat; visibleRange(width:) -> ClosedRange<Int64>; zoomed(by:aroundX:); scrolled(byPoints:); scrolled(toTick:); static clampTick(_:) -> Int64; static limit, minTicksPerPoint, maxTicksPerPoint }`; `GridLine { tick: Int64, kind: GridLine.Kind (.bar(Int64) | .beat) }`; `Grid.lines(meter:from:to:ticksPerPoint:minSpacing:maxLines:) -> [GridLine]`, `Grid.labelEvery(barTicks:ticksPerPoint:minSpacing:) -> Int64`. Package: targets `VizDAWEditors` (depends on `VizDAWAppModel`, `VizDAWCore`) and `VizDAWEditorsTests`; `VizDAW` depends on `VizDAWEditors`.

Rulings 16, 22, 25. All tick arithmetic stays within `TimeAxis.limit` (2⁵⁰ ticks, about two years at 120 BPM), so no Swift integer operation can trap.

- [ ] **Step 1: Add the targets**

In `app/Package.swift`:

Find:
```swift
        .executableTarget(
            name: "VizDAW",
            dependencies: ["VizDAWAppModel", "VizDAWCore"],
            path: "Sources/VizDAW"
        ),
```
Replace:
```swift
        // Editors (plan 4a): pure geometry plus the AppKit canvas views.
        .target(
            name: "VizDAWEditors",
            dependencies: ["VizDAWAppModel", "VizDAWCore"],
            path: "Sources/VizDAWEditors"
        ),
        .executableTarget(
            name: "VizDAW",
            dependencies: ["VizDAWAppModel", "VizDAWCore", "VizDAWEditors"],
            path: "Sources/VizDAW"
        ),
```

Find:
```swift
        .testTarget(
            name: "VizDAWAppModelTests",
            dependencies: ["VizDAWAppModel", "VizDAWCore"],
            path: "Tests/VizDAWAppModelTests"
        ),
```
Replace:
```swift
        .testTarget(
            name: "VizDAWAppModelTests",
            dependencies: ["VizDAWAppModel", "VizDAWCore"],
            path: "Tests/VizDAWAppModelTests"
        ),
        .testTarget(
            name: "VizDAWEditorsTests",
            dependencies: ["VizDAWEditors", "VizDAWAppModel", "VizDAWCore"],
            path: "Tests/VizDAWEditorsTests"
        ),
```

- [ ] **Step 2: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWEditors app/Tests/VizDAWEditorsTests
cat > app/Tests/VizDAWAppModelTests/MeterMathTests.swift <<'VIZDAW_EOF'
import Testing
import VizDAWCore
@testable import VizDAWAppModel

let Q: Int64 = 15_360
let BAR: Int64 = 4 * Q

func meter(_ tick: Int64, _ num: UInt8, _ den: UInt8) -> MeterRow {
    MeterRow(tick: tick, num: num, den: den, sharps: 0, minor: false)
}

@Suite struct MeterMathTests {
    @Test func barsCountFromOneAtTickZero() {
        let m = [meter(0, 4, 4)]
        #expect(MeterMath.bar(at: 0, m) == MeterMath.Bar(number: 1, start: 0, length: BAR, meter: m[0]))
        #expect(MeterMath.bar(at: BAR - 1, m).number == 1)
        #expect(MeterMath.bar(at: BAR, m).number == 2)
        #expect(MeterMath.barStart(3, m) == 2 * BAR)
    }

    @Test func pickupBarsNumberBackwards() {
        let m = [meter(0, 4, 4)]
        #expect(MeterMath.bar(at: -1, m).number == 0)
        #expect(MeterMath.bar(at: -1, m).start == -BAR)
        #expect(MeterMath.bar(at: -BAR - 1, m).number == -1)
        #expect(MeterMath.barStart(0, m) == -BAR)
    }

    @Test func meterChangesContinueTheBarNumbers() {
        let m = [meter(0, 4, 4), meter(2 * BAR, 3, 4)]
        let third = MeterMath.bar(at: 2 * BAR, m)
        #expect(third.number == 3)
        #expect(third.length == 3 * Q)
        #expect(MeterMath.bar(at: 2 * BAR + 3 * Q, m).number == 4)
        #expect(MeterMath.barStart(4, m) == 2 * BAR + 3 * Q)
        #expect(MeterMath.beatTicks(meter(0, 6, 8)) == Q / 2)
    }

    @Test func anEmptyMeterIsFourFour() {
        #expect(MeterMath.bar(at: BAR, []).number == 2)
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWAppModelTests/SnapTests.swift <<'VIZDAW_EOF'
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@Suite struct SnapTests {
    let fourFour = [meter(0, 4, 4)]

    @Test func quarterSnapsToTheNearestBeat() {
        let s = Snap()
        #expect(s.apply(Q * 4 / 10, meter: fourFour) == 0)
        #expect(s.apply(Q * 6 / 10, meter: fourFour) == Q)
        #expect(s.apply(Q / 2, meter: fourFour) == Q, "ties go up")
    }

    @Test func offLeavesTicksAlone() {
        var s = Snap()
        s.enabled = false
        #expect(s.apply(123, meter: fourFour) == 123)
    }

    @Test func tripletsAndDots() {
        var s = Snap()
        s.triplet = true
        #expect(s.step(in: fourFour[0]) == Q * 2 / 3)
        s = Snap()
        s.division = .eighth
        s.dotted = true
        #expect(s.step(in: fourFour[0]) == Q * 3 / 4)
    }

    @Test func barSnapFollowsTheMeter() {
        var s = Snap()
        s.division = .bar
        let threeFour = [meter(0, 3, 4)]
        #expect(s.apply(30_000, meter: threeFour) == 3 * Q)
        #expect(s.apply(20_000, meter: threeFour) == 0)
    }

    @Test func aGridThatDoesNotDivideTheBarSnapsToTheBarLine() {
        var s = Snap()
        s.division = .half
        s.triplet = true  // 20 480 ticks: three per 4/4 bar, fits exactly
        #expect(s.apply(BAR - 100, meter: fourFour) == BAR)
        let fiveFour = [meter(0, 5, 4)]
        s = Snap()
        s.division = .half  // 30 720: 5/4 bar ends 15 360 after the second half
        #expect(s.apply(5 * Q - 1_000, meter: fiveFour) == 5 * Q)
    }

    @Test func pickupBarsSnapAndNumberBackwards() {
        let s = Snap()
        #expect(s.apply(-Q * 4 / 10, meter: fourFour) == 0)
        #expect(s.apply(-BAR + 100, meter: fourFour) == -BAR)
    }

    @Test func snapRoundTripsThroughJSON() throws {
        var s = Snap()
        s.division = .sixteenth
        s.triplet = true
        let data = try JSONEncoder().encode(s)
        #expect(try JSONDecoder().decode(Snap.self, from: data) == s)
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWEditorsTests/TimeAxisTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Testing
@testable import VizDAWEditors

@Suite struct TimeAxisTests {
    @Test func ticksAndPointsRoundTrip() {
        let a = TimeAxis(ticksPerPoint: 100, originTick: 1_000)
        #expect(a.x(1_000) == 0)
        #expect(a.x(2_000) == 10)
        #expect(a.tick(atX: 10) == 2_000)
        #expect(a.width(ofTicks: 15_360) == 153.6)
        #expect(a.visibleRange(width: 100) == 1_000...11_000)
    }

    @Test func zoomKeepsTheTickUnderThePointer() {
        let a = TimeAxis(ticksPerPoint: 100, originTick: 1_000)
        let z = a.zoomed(by: 2, aroundX: 50)
        #expect(z.ticksPerPoint == 50)
        #expect(z.tick(atX: 50) == a.tick(atX: 50))
    }

    @Test func zoomClampsToItsLimits() {
        let a = TimeAxis()
        #expect(a.zoomed(by: 1e9, aroundX: 0).ticksPerPoint == TimeAxis.minTicksPerPoint)
        #expect(a.zoomed(by: 1e-9, aroundX: 0).ticksPerPoint == TimeAxis.maxTicksPerPoint)
    }

    @Test func extremeOffsetsClampInsteadOfTrapping() {
        let far = TimeAxis().scrolled(byPoints: 1e30)
        #expect(far.originTick == Double(TimeAxis.limit))
        #expect(far.tick(atX: .greatestFiniteMagnitude) == TimeAxis.limit)
        let back = TimeAxis().scrolled(byPoints: -1e30)
        #expect(back.tick(atX: 0) == -TimeAxis.limit)
        #expect(TimeAxis.clampTick(.nan) == 0)
        #expect(TimeAxis().x(Int64.max).isFinite)
        #expect(TimeAxis().scrolled(toTick: .infinity).originTick == Double(TimeAxis.limit))
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWEditorsTests/GridTests.swift <<'VIZDAW_EOF'
import Testing
import VizDAWCore
@testable import VizDAWEditors

private let Q: Int64 = 15_360
private let BAR: Int64 = 4 * Q

private func meter(_ tick: Int64, _ num: UInt8, _ den: UInt8) -> MeterRow {
    MeterRow(tick: tick, num: num, den: den, sharps: 0, minor: false)
}

@Suite struct GridTests {
    @Test func barsAndBeatsAtNormalZoom() {
        let lines = Grid.lines(meter: [meter(0, 4, 4)], from: 0, to: 2 * BAR, ticksPerPoint: 128)
        #expect(lines.filter { $0.kind == .beat }.count == 6)
        #expect(lines.compactMap { if case .bar(let n) = $0.kind { n } else { nil } } == [1, 2, 3])
        #expect(lines.first?.tick == 0)
    }

    @Test func beatsDropOutWhenTheyCrowd() {
        let lines = Grid.lines(meter: [meter(0, 4, 4)], from: 0, to: 4 * BAR, ticksPerPoint: Double(Q) / 4)
        #expect(lines.allSatisfy { $0.kind != .beat })
        #expect(lines.count == 5)
    }

    @Test func barsThinOutFarOut() {
        // One point per bar: every eighth bar keeps 6 points apart.
        let lines = Grid.lines(meter: [meter(0, 4, 4)], from: 0, to: 20 * BAR, ticksPerPoint: Double(BAR))
        #expect(lines.compactMap { if case .bar(let n) = $0.kind { n } else { nil } } == [1, 9, 17])
    }

    @Test func pickupBarsAreNumberedBackwards() {
        let lines = Grid.lines(meter: [meter(0, 4, 4)], from: -2 * BAR, to: 0, ticksPerPoint: 512)
        let bars = lines.compactMap { l -> (Int64, Int64)? in
            if case .bar(let n) = l.kind { (n, l.tick) } else { nil }
        }
        #expect(bars.map(\.0) == [-1, 0, 1])
        #expect(bars.map(\.1) == [-2 * BAR, -BAR, 0])
    }

    @Test func meterChangesMoveTheBeats() {
        let m = [meter(0, 4, 4), meter(BAR, 3, 4)]
        let lines = Grid.lines(meter: m, from: BAR, to: BAR + 3 * Q, ticksPerPoint: 128)
        #expect(lines.map(\.tick) == [BAR, BAR + Q, BAR + 2 * Q, BAR + 3 * Q])
        #expect(lines.last?.kind == .bar(3))
    }

    @Test func labelsSpreadOut() {
        #expect(Grid.labelEvery(barTicks: BAR, ticksPerPoint: 128) == 1)
        #expect(Grid.labelEvery(barTicks: BAR, ticksPerPoint: Double(BAR) / 10) == 4)
    }
}
VIZDAW_EOF
```

- [ ] **Step 3: Run them to see them fail**

Run: `scripts/build-ffi.sh && scripts/swift.sh test`
Expected: compile errors — cannot find `MeterMath`, `Snap`, `TimeAxis`, `Grid`.

- [ ] **Step 4: Write the model types**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/MeterMath.swift <<'VIZDAW_EOF'
import VizDAWCore

/// Bars and beats over a meter map (`SequenceInfo.meter`), as the core's
/// `MeterMap` counts them: the first entry is at tick 0 and also governs
/// the pickup bars before it; bar 1 starts at tick 0; entries fall on
/// barlines. Ticks stay within ±2⁵⁰ (`TimeAxis.limit`), so nothing traps.
public enum MeterMath {
    /// Ticks per quarter note.
    public static let ppq: Int64 = 15_360

    static let fourFour = MeterRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)

    /// One bar.
    public struct Bar: Equatable, Sendable {
        public let number: Int64
        public let start: Int64
        public let length: Int64
        public let meter: MeterRow

        public init(number: Int64, start: Int64, length: Int64, meter: MeterRow) {
            self.number = number
            self.start = start
            self.length = length
            self.meter = meter
        }
    }

    public static func beatTicks(_ m: MeterRow) -> Int64 { ppq * 4 / Int64(max(m.den, 1)) }

    public static func barTicks(_ m: MeterRow) -> Int64 { beatTicks(m) * Int64(max(m.num, 1)) }

    /// Each entry with the number of the bar it starts.
    static func withBars(_ meter: [MeterRow]) -> [(entry: MeterRow, bar: Int64)] {
        let entries = meter.isEmpty ? [fourFour] : meter
        var out: [(entry: MeterRow, bar: Int64)] = []
        for e in entries {
            if let prev = out.last {
                out.append((e, prev.bar + (e.tick - prev.entry.tick) / barTicks(prev.entry)))
            } else {
                out.append((e, 1))
            }
        }
        return out
    }

    static func floorDiv(_ a: Int64, _ b: Int64) -> Int64 {
        let q = a / b
        return (a % b != 0 && (a < 0) != (b < 0)) ? q - 1 : q
    }

    /// The bar containing `tick`.
    public static func bar(at tick: Int64, _ meter: [MeterRow]) -> Bar {
        let bars = withBars(meter)
        let (e, first) = bars.last(where: { $0.entry.tick <= tick }) ?? bars[0]
        let length = barTicks(e)
        let k = floorDiv(tick - e.tick, length)
        return Bar(number: first + k, start: e.tick + k * length, length: length, meter: e)
    }

    /// The first tick of bar `number`.
    public static func barStart(_ number: Int64, _ meter: [MeterRow]) -> Int64 {
        let bars = withBars(meter)
        let (e, first) = bars.last(where: { $0.bar <= number }) ?? bars[0]
        return e.tick + (number - first) * barTicks(e)
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWAppModel/Snap.swift <<'VIZDAW_EOF'
import VizDAWCore

/// The shared edit grid (spec §3.5): one setting for every editor,
/// toggled by one key (Plan Ruling 16). The grid is counted from the start
/// of the bar a tick is in, so a bar line always snaps.
public struct Snap: Codable, Equatable, Sendable {
    public enum Division: String, Codable, CaseIterable, Sendable {
        case bar, half, quarter, eighth, sixteenth, thirtySecond

        public var title: String {
            switch self {
            case .bar: "Bar"
            case .half: "1/2"
            case .quarter: "1/4"
            case .eighth: "1/8"
            case .sixteenth: "1/16"
            case .thirtySecond: "1/32"
            }
        }
    }

    public var division: Division = .quarter
    public var triplet = false
    public var dotted = false
    public var enabled = true

    public init() {}

    /// The grid step in ticks inside a bar of meter `m`.
    public func step(in m: MeterRow) -> Int64 {
        let q = MeterMath.ppq
        var s: Int64
        switch division {
        case .bar: return MeterMath.barTicks(m)
        case .half: s = 2 * q
        case .quarter: s = q
        case .eighth: s = q / 2
        case .sixteenth: s = q / 4
        case .thirtySecond: s = q / 8
        }
        if triplet { s = s * 2 / 3 }
        if dotted { s = s * 3 / 2 }
        return max(s, 1)
    }

    /// `tick` on the nearest grid line of its bar (ties go later); the bar's
    /// end counts as a grid line. Unchanged when snap is off.
    public func apply(_ tick: Int64, meter: [MeterRow]) -> Int64 {
        guard enabled else { return tick }
        let bar = MeterMath.bar(at: tick, meter)
        let s = step(in: bar.meter)
        let down = bar.start + ((tick - bar.start) / s) * s
        let up = min(down + s, bar.start + bar.length)
        return tick - down < up - tick ? down : up
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/TimeAxis.swift <<'VIZDAW_EOF'
import CoreGraphics

/// Tick ↔ x for time-based editors (spec §3.5): `ticksPerPoint` is the
/// zoom, `originTick` the tick at x = 0 of the canvas. Pure value type;
/// every conversion clamps to ±`limit` instead of trapping.
public struct TimeAxis: Equatable, Sendable {
    public static let minTicksPerPoint: Double = 2
    /// One 4/4 bar per point.
    public static let maxTicksPerPoint: Double = 61_440
    /// The ticks the axis reaches (2⁵⁰, about two years at 120 BPM).
    public static let limit: Int64 = 1 << 50

    public private(set) var ticksPerPoint: Double
    public private(set) var originTick: Double

    public init(ticksPerPoint: Double = 384, originTick: Double = 0) {
        self.ticksPerPoint = Self.clampZoom(ticksPerPoint)
        self.originTick = Self.clampOrigin(originTick)
    }

    static func clampZoom(_ v: Double) -> Double {
        guard v.isFinite else { return v > 0 ? maxTicksPerPoint : minTicksPerPoint }
        return min(max(v, minTicksPerPoint), maxTicksPerPoint)
    }

    static func clampOrigin(_ v: Double) -> Double {
        if v.isNaN { return 0 }
        return min(max(v, -Double(limit)), Double(limit))
    }

    /// A tick from a double: NaN is 0; values beyond ±`limit` clamp.
    public static func clampTick(_ d: Double) -> Int64 {
        if d.isNaN { return 0 }
        if d >= Double(limit) { return limit }
        if d <= -Double(limit) { return -limit }
        return Int64(d.rounded(.down))
    }

    public func x(_ tick: Int64) -> CGFloat {
        let t = min(max(tick, -Self.limit), Self.limit)
        return CGFloat((Double(t) - originTick) / ticksPerPoint)
    }

    public func tick(atX x: CGFloat) -> Int64 {
        Self.clampTick(originTick + Double(x) * ticksPerPoint)
    }

    public func width(ofTicks t: Int64) -> CGFloat {
        CGFloat(Double(min(max(t, -Self.limit), Self.limit)) / ticksPerPoint)
    }

    public func visibleRange(width: CGFloat) -> ClosedRange<Int64> {
        tick(atX: 0)...tick(atX: max(width, 0))
    }

    /// Zoomed in by `factor` (> 1 zooms in) keeping the tick under `x`.
    public func zoomed(by factor: Double, aroundX x: CGFloat) -> TimeAxis {
        guard factor.isFinite, factor > 0 else { return self }
        let anchor = originTick + Double(x) * ticksPerPoint
        let tpp = Self.clampZoom(ticksPerPoint / factor)
        return TimeAxis(ticksPerPoint: tpp, originTick: anchor - Double(x) * tpp)
    }

    public func scrolled(byPoints dx: CGFloat) -> TimeAxis {
        TimeAxis(ticksPerPoint: ticksPerPoint, originTick: originTick + Double(dx) * ticksPerPoint)
    }

    public func scrolled(toTick t: Double) -> TimeAxis {
        TimeAxis(ticksPerPoint: ticksPerPoint, originTick: t)
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/Grid.swift <<'VIZDAW_EOF'
import VizDAWAppModel
import VizDAWCore

/// One line of the editor grid.
public struct GridLine: Equatable, Sendable {
    public enum Kind: Equatable, Sendable {
        /// A bar line with its bar number.
        case bar(Int64)
        case beat
    }

    public let tick: Int64
    public let kind: Kind
}

/// Bar and beat lines for the grid layer and the ruler (spec §3.5).
public enum Grid {
    /// Lines from the bar containing `from` up to `to`. Beat lines only
    /// when beats are at least `minSpacing` points apart; bar lines thinned
    /// to every 2ⁿ-th bar (counted from bar 1) when bars are closer than
    /// that. At most `maxLines` lines.
    public static func lines(
        meter: [MeterRow], from: Int64, to: Int64, ticksPerPoint: Double,
        minSpacing: Double = 6, maxLines: Int = 4_000
    ) -> [GridLine] {
        var out: [GridLine] = []
        var bar = MeterMath.bar(at: from, meter)
        var visits = 0
        while bar.start <= to && out.count < maxLines && visits < 200_000 {
            visits += 1
            let barPoints = Double(bar.length) / ticksPerPoint
            var every: Int64 = 1
            while Double(every) * barPoints < minSpacing && every < (1 << 20) { every *= 2 }
            if MeterMath.floorDiv(bar.number - 1, every) * every == bar.number - 1 {
                out.append(GridLine(tick: bar.start, kind: .bar(bar.number)))
            }
            let beat = MeterMath.beatTicks(bar.meter)
            if Double(beat) / ticksPerPoint >= minSpacing {
                var t = bar.start + beat
                while t < bar.start + bar.length && out.count < maxLines {
                    if t >= from && t <= to { out.append(GridLine(tick: t, kind: .beat)) }
                    t += beat
                }
            }
            bar = MeterMath.bar(at: bar.start + bar.length, meter)
        }
        return out
    }

    /// Label every n-th bar so labels are at least `minSpacing` points
    /// apart (n a power of two).
    public static func labelEvery(barTicks: Int64, ticksPerPoint: Double, minSpacing: Double = 28) -> Int64 {
        var every: Int64 = 1
        while Double(every) * Double(barTicks) / ticksPerPoint < minSpacing && every < (1 << 20) {
            every *= 2
        }
        return every
    }
}
VIZDAW_EOF
```

`MeterMath.floorDiv` is `internal`; make it `public` so `Grid` (another module) can use it:

In `app/Sources/VizDAWAppModel/MeterMath.swift`:

Find:
```swift
    static func floorDiv(_ a: Int64, _ b: Int64) -> Int64 {
```
Replace:
```swift
    public static func floorDiv(_ a: Int64, _ b: Int64) -> Int64 {
```

- [ ] **Step 5: Run the tests**

Run: `scripts/swift.sh build && scripts/swift.sh test`
Expected: PASS — 4 + 7 + 4 + 6 = 21 new tests (114 Swift tests).

In `GridTests.barsThinOutFarOut`, bars are 1 point apart: `every` doubles to 8 (8 points ≥ 6), so bars 1, 9, 17 remain in bars 1–21. In `labelsSpreadOut`, a bar of 10 points needs every 4th bar (40 ≥ 28).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Package.swift app/Sources/VizDAWAppModel/MeterMath.swift app/Sources/VizDAWAppModel/Snap.swift \
  app/Sources/VizDAWEditors/TimeAxis.swift app/Sources/VizDAWEditors/Grid.swift \
  app/Tests/VizDAWAppModelTests/MeterMathTests.swift app/Tests/VizDAWAppModelTests/SnapTests.swift \
  app/Tests/VizDAWEditorsTests/TimeAxisTests.swift app/Tests/VizDAWEditorsTests/GridTests.swift
git commit -m "feat(app): editors target, time axis, grid, meter maths and snap

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 114 Swift tests.

---

### Task 8: `TransportClock`

**Files:**
- Create: `app/Sources/VizDAWAppModel/TransportClock.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Test: `app/Tests/VizDAWAppModelTests/TransportClockTests.swift` (new)

**Interfaces:**
- Consumes: Task 6's `TransportState.hostTimeNs`, `ticksPerSecond`.
- Produces: `@MainActor final class TransportClock { sample: TransportClock.Sample?; shown: Double; pollInterval: Double; isPlaying: Bool; static holdSeconds; update(_ t: TransportState, pollInterval: Double); tick(atHostTimeNs: UInt64) -> Double }`; `TransportClock.Sample { tick: Int64, hostTimeNs: UInt64, ticksPerSecond: Double, playing: Bool, seq: UInt64? }`; `DocumentModel.clock: TransportClock` (`@ObservationIgnored`), `DocumentModel.pollSeconds: Double`.

Ruling 11. The canvas (Task 11) calls `tick(atHostTimeNs:)` from its display link with `CADisplayLink.timestamp × 10⁹`; Rust is not called more often than the existing poll.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/TransportClockTests.swift <<'VIZDAW_EOF'
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let t0: UInt64 = 1_000_000_000

private func state(
    tick: Int64, at ns: UInt64, playing: Bool = true, seq: UInt64? = 1, tps: Double = 30_720
) -> TransportState {
    TransportState(
        playing: playing, seq: seq, tick: tick, positionBbu: "", seconds: 0, sampleRate: 48_000,
        device: "", faults: [], report: [], hostTimeNs: ns, ticksPerSecond: tps)
}

@MainActor
@Suite struct TransportClockTests {
    @Test func noSampleIsTickZero() {
        #expect(TransportClock().tick(atHostTimeNs: t0) == 0)
    }

    @Test func extrapolatesBetweenPolls() {
        let c = TransportClock()
        c.update(state(tick: 1_000, at: t0), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 10_000_000) == 1_000 + 0.01 * 30_720)
    }

    @Test func neverRunsMoreThanOnePollPastTheSample() {
        let c = TransportClock()
        c.update(state(tick: 1_000, at: t0), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 1_000_000_000) == 1_000 + 0.033 * 30_720)
    }

    @Test func holdsInsteadOfTwitchingBack() {
        let c = TransportClock()
        c.update(state(tick: 1_000, at: t0), pollInterval: 0.033)
        let ahead = c.tick(atHostTimeNs: t0 + 30_000_000)
        // The next poll reads slightly less than was drawn: hold.
        c.update(state(tick: 1_900, at: t0 + 30_000_000), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 30_000_000) == ahead)
        #expect(c.tick(atHostTimeNs: t0 + 40_000_000) == 1_900 + 0.01 * 30_720)
    }

    @Test func snapsBackOnALocateOrALoopWrap() {
        let c = TransportClock()
        c.update(state(tick: 100_000, at: t0), pollInterval: 0.033)
        _ = c.tick(atHostTimeNs: t0 + 20_000_000)
        c.update(state(tick: 0, at: t0 + 33_000_000), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 33_000_000) == 0)
    }

    @Test func stopAndSequenceChangesSnap() {
        let c = TransportClock()
        c.update(state(tick: 9_000, at: t0), pollInterval: 0.033)
        _ = c.tick(atHostTimeNs: t0 + 30_000_000)
        c.update(state(tick: 8_000, at: t0 + 33_000_000, playing: false), pollInterval: 0.25)
        #expect(c.tick(atHostTimeNs: t0 + 900_000_000) == 8_000)
        c.update(state(tick: 9_000, at: t0, seq: 1), pollInterval: 0.033)
        _ = c.tick(atHostTimeNs: t0 + 30_000_000)
        c.update(state(tick: 8_990, at: t0 + 33_000_000, seq: 2), pollInterval: 0.033)
        #expect(c.tick(atHostTimeNs: t0 + 33_000_000) == 8_990)
    }

    @Test func theDocumentFeedsTheClockOnEveryPoll() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        f.state.playing = true
        f.state.tick = 5_000
        f.state.hostTimeNs = 77
        m.poll()
        #expect(m.clock.sample?.tick == 5_000)
        #expect(m.clock.sample?.hostTimeNs == 77)
        #expect(m.clock.pollInterval == 0.033)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/swift.sh test`
Expected: compile error — cannot find `TransportClock`.

- [ ] **Step 3: Write `TransportClock` and feed it**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/TransportClock.swift <<'VIZDAW_EOF'
import VizDAWCore

/// Extrapolates the playhead between transport polls (spec §3.4, Plan
/// Ruling 11): `tick + min(elapsed, pollInterval) × ticksPerSecond`. While
/// playing it never steps back by less than `holdSeconds` (poll jitter);
/// it snaps on bigger backward jumps (locate, loop wrap), on start, stop
/// and a change of sequence. Owned by `DocumentModel` outside observation:
/// the canvas reads it on every display frame.
@MainActor
public final class TransportClock {
    public struct Sample: Equatable, Sendable {
        public let tick: Int64
        public let hostTimeNs: UInt64
        public let ticksPerSecond: Double
        public let playing: Bool
        public let seq: UInt64?
    }

    public static let holdSeconds = 0.1

    public private(set) var sample: Sample?
    /// The tick drawn last.
    public private(set) var shown: Double = 0
    /// How far (seconds) the extrapolation may run past the last sample.
    public private(set) var pollInterval: Double = 0.033

    public init() {}

    public var isPlaying: Bool { sample?.playing ?? false }

    /// A new transport reading.
    public func update(_ t: TransportState, pollInterval: Double) {
        let new = Sample(
            tick: t.tick, hostTimeNs: t.hostTimeNs, ticksPerSecond: max(t.ticksPerSecond, 0),
            playing: t.playing, seq: t.seq)
        self.pollInterval = pollInterval
        let old = sample
        sample = new
        let continuing = (old?.playing ?? false) && new.playing && old?.seq == new.seq
        let jumpedBack = Double(new.tick) < shown - Self.holdSeconds * new.ticksPerSecond
        if !continuing || jumpedBack {
            shown = Double(new.tick)
        }
    }

    /// The tick to draw at host time `now` (ns, `CACurrentMediaTime`'s clock).
    public func tick(atHostTimeNs now: UInt64) -> Double {
        guard let s = sample else { return 0 }
        guard s.playing else {
            shown = Double(s.tick)
            return shown
        }
        let elapsed = now > s.hostTimeNs ? Double(now - s.hostTimeNs) / 1e9 : 0
        let t = Double(s.tick) + min(elapsed, pollInterval) * s.ticksPerSecond
        shown = max(shown, t)
        return shown
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
    /// Slots whose parameters changed outside a gesture (their windows
    /// refetch), cleared by whoever handles them.
    public var paramsChanged: Set<UInt64> = []
```
Replace:
```swift
    /// Slots whose parameters changed outside a gesture (their windows
    /// refetch), cleared by whoever handles them.
    public var paramsChanged: Set<UInt64> = []
    /// The playhead between polls (spec §3.4); fed by `poll()`.
    @ObservationIgnored public let clock = TransportClock()
```

Find:
```swift
    /// 33 ms while playing, 250 ms when stopped (to catch the end).
    public var pollInterval: Duration { isPlaying ? .milliseconds(33) : .milliseconds(250) }
```
Replace:
```swift
    /// 33 ms while playing, 250 ms when stopped (to catch the end).
    public var pollInterval: Duration { isPlaying ? .milliseconds(33) : .milliseconds(250) }
    /// `pollInterval` in seconds.
    public var pollSeconds: Double { isPlaying ? 0.033 : 0.25 }
```

Find:
```swift
    public func poll() {
        do {
            transport = try session.transport()
        } catch VizError.Busy {
```
Replace:
```swift
    public func poll() {
        do {
            transport = try session.transport()
            clock.update(transport, pollInterval: pollSeconds)
        } catch VizError.Busy {
```

Find:
```swift
        do {
            transport = try session.transport()
        } catch VizError.Busy {
            transport = Self.stopped
        }
        apply(snapshot)
```
Replace:
```swift
        do {
            transport = try session.transport()
        } catch VizError.Busy {
            transport = Self.stopped
        }
        clock.update(transport, pollInterval: transport.playing ? 0.033 : 0.25)
        apply(snapshot)
```

- [ ] **Step 4: Run the tests**

Run: `scripts/swift.sh test`
Expected: PASS (7 new tests, 121 Swift tests).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Sources/VizDAWAppModel/TransportClock.swift app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Tests/VizDAWAppModelTests/TransportClockTests.swift
git commit -m "feat(app): transport clock extrapolates the playhead between polls

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 121 Swift tests.

---

### Task 9: `EditorModel` — revision-keyed caches, selection, edits, layout

**Files:**
- Create: `app/Sources/VizDAWAppModel/EditorTypes.swift`, `app/Sources/VizDAWAppModel/EditorLayout.swift`, `app/Sources/VizDAWAppModel/SequenceOrder.swift`, `app/Sources/VizDAWAppModel/EditorModel.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Test: `app/Tests/VizDAWAppModelTests/EditorModelTests.swift`, `app/Tests/VizDAWAppModelTests/SequenceOrderTests.swift` (new)

**Interfaces:**
- Consumes: the session methods of Tasks 1–6 (`laneEvents`, `blocks`, `sequenceInfo`, `edit`, `selection`, `setSelection`, `uiLayout`, `setUiLayout`); `DocSnapshot.laneRevs`; Task 7's `Snap`, `MeterMath`; FakeSession members of Tasks 2–5 and helpers `noteRow`, `blockRow`, `fourFour`.
- Produces:
  - `LaneKey { seq, lane }` (`Hashable, Codable, Sendable`, `init(_:_:)`); `Tool { arrow, pencil, eraser }`; `LightMode { auto, on, off }`; `BlockRuleSetting { silenceGap(beats: Int), fixedBars(Int), wholeLane }` with `rule: BlockRule`, `title`, `static choices`; `DetailEditorKind { pianoRoll, list, stripChart, drumGrid }` with `title`, `key: String` ("1"–"4"), `plan: String`; `EditorTarget { block(seq:lane:start:end:), lane(seq:lane:), sequence(UInt64) }` with `seq`, `lane: UInt64?`; `DetailState { kind, target }`; `TornWindow { id: UUID, detail: DetailState, frame: [Double]? }`.
  - `EditorLayout` (`Codable, Equatable, Sendable`; tolerant decoding): `version, detail: DetailState?, lastKind, windows: [TornWindow], sequenceOrder: [UInt64], blockRule, lightMode, snap, laneHeight: Double, ticksPerPoint: Double, follow: Bool`; `static decode(_ json: String?) -> EditorLayout`; `encoded() -> String`.
  - `SequenceOrder.sorted(_:by:) -> [SeqRow]`, `SequenceOrder.moved(all:order:visible:from:to:) -> [UInt64]`.
  - `@Observable @MainActor final class EditorModel`: `session`, `applySnapshot` (set by `DocumentModel`), `revs: [LaneKey: UInt64]`, `snapshotVersion`, `drawVersion: Int`, `selection: SelectionRow`, `layout: EditorLayout`, `tool: Tool`, `status: String?`, `navigation: [UInt64]`; `laneEvents(_:) -> LaneEvents?`, `blocks(_:) -> [BlockRow]`, `info(_:) -> SequenceInfo?`, `edit(_:filter:) -> Bool`, `setSelection(_:)`, `refreshSelection()`, `clearSelection()`, `selectBlock(_:in:adding:)`, `selectRange(_:lanes:)`, `isSelected(_:) -> Bool`, `isSelected(event:) -> Bool`, `updateLayout(_:)`, `storeLayout()`, `report(_:)`, `clearStatus()`, `push(_:)`, `pop() -> UInt64?`, `resetNavigation()`, internal `sync(_:)`, test counters `eventFetches`, `blockFetches`, `infoFetches`, `cachedLanes`.
  - `DocumentModel.editor: EditorModel`; `DocumentModel.choose(_:)` (sidebar selection: resets navigation); `topLevel`/`referenced` follow `editor.layout.sequenceOrder`; `save()`/`saveAs(path:)` store the layout first.

Rulings 2, 12, 19, 20. Caches are `@ObservationIgnored` so a fetch made while drawing never invalidates the drawing; views observe `drawVersion`, `selection`, `layout`, `status` and `navigation`.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/SequenceOrderTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@Suite struct SequenceOrderTests {
    let rows = [
        seq(1, "Beat", topLevel: false), seq(2, "Groove", topLevel: false),
        seq(3, "Song", topLevel: true),
    ]

    @Test func listedFirstThenTheRestByID() {
        #expect(SequenceOrder.sorted(rows, by: [3]).map(\.id) == [3, 1, 2])
        #expect(SequenceOrder.sorted(rows, by: []).map(\.id) == [1, 2, 3])
        #expect(SequenceOrder.sorted(rows, by: [99, 2]).map(\.id) == [2, 1, 3], "stale IDs are ignored")
    }

    @Test func movingWithinASectionKeepsTheOthersInPlace() {
        let order = SequenceOrder.moved(
            all: rows, order: [], visible: [1, 2], from: IndexSet(integer: 1), to: 0)
        #expect(order == [2, 1, 3])
        let back = SequenceOrder.moved(
            all: rows, order: order, visible: [2, 1], from: IndexSet(integer: 0), to: 2)
        #expect(back == [1, 2, 3])
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWAppModelTests/EditorModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// demoFake with revisions for Song's lanes (30 Groove, 31 Chords).
func editorFake() -> FakeSession {
    let f = demoFake()
    f.snap.laneRevs = [
        LaneRev(seq: 3, lane: 30, rev: 1), LaneRev(seq: 3, lane: 31, rev: 2),
    ]
    f.laneEventTable["3/31"] = LaneEvents(seq: 3, lane: 31, rev: 2, events: [noteRow(1, tick: 0)])
    f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [])
    f.blocksTable["3/30"] = [blockRow(0, 4 * 61_440, events: [7], ref: 2, label: "Groove")]
    f.blocksTable["3/31"] = [blockRow(0, 61_440, events: [1])]
    return f
}

/// The next command returns `f.snap` changed by `change`, one version on.
func nextSnapshot(_ f: FakeSession, _ change: (inout DocSnapshot) -> Void) {
    var s = f.snap
    s.version += 1
    change(&s)
    f.bump(s)
}

let chords = LaneKey(3, 31)
let grooveLane = LaneKey(3, 30)

@MainActor
@Suite struct EditorModelTests {
    @Test func eventsAreFetchedOnceAndAgainOnlyWhenTheirRevisionMoves() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let e = m.editor
        #expect(e.laneEvents(chords)?.events.count == 1)
        #expect(e.laneEvents(chords) != nil)
        #expect(e.laneEvents(grooveLane) != nil)
        #expect(e.eventFetches == 2)
        f.laneEventTable["3/31"] = LaneEvents(seq: 3, lane: 31, rev: 5, events: [])
        nextSnapshot(f) { $0.laneRevs[1].rev = 5 }
        m.rename(1, to: "Kit")
        #expect(e.laneEvents(chords)?.events.isEmpty == true)
        #expect(e.laneEvents(grooveLane) != nil)
        #expect(e.eventFetches == 3, "only Chords was refetched")
    }

    @Test func lanesThatAreGoneLeaveTheCache() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        _ = m.editor.laneEvents(chords)
        _ = m.editor.blocks(chords)
        nextSnapshot(f) { $0.laneRevs.removeLast() }
        m.rename(1, to: "Kit")
        #expect(!m.editor.cachedLanes.contains(chords))
    }

    @Test func blocksFollowTheRevisionAndTheRule() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let e = m.editor
        #expect(e.blocks(chords).count == 1)
        _ = e.blocks(chords)
        #expect(e.blockFetches == 1)
        e.updateLayout { $0.blockRule = .fixedBars(2) }
        _ = e.blocks(chords)
        #expect(e.blockFetches == 2)
        #expect(f.lastRule == .fixedBars(bars: 2))
    }

    @Test func referenceBlocksRefetchWhenAnySequenceRowChanges() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let e = m.editor
        _ = e.blocks(grooveLane)
        _ = e.blocks(chords)
        // Groove is renamed: no lane of Song changed revision.
        nextSnapshot(f) { $0.sequences[1].name = "Groove 2" }
        m.rename(2, to: "Groove 2")
        _ = e.blocks(grooveLane)
        _ = e.blocks(chords)
        #expect(f.blocksCalls == ["3/30", "3/31", "3/30"])
    }

    @Test func sequenceInfoIsCachedPerVersion() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        _ = m.editor.info(3)
        _ = m.editor.info(3)
        #expect(m.editor.infoFetches == 1)
        m.rename(1, to: "Kit")
        _ = m.editor.info(3)
        #expect(m.editor.infoFetches == 2)
    }

    @Test func busyReadsKeepTheStaleRowsSilently() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        _ = m.editor.laneEvents(chords)
        nextSnapshot(f) { $0.laneRevs[1].rev = 9 }
        m.rename(1, to: "Kit")
        f.readError = .Busy(message: "busy")
        #expect(m.editor.laneEvents(chords)?.events.count == 1)
        #expect(m.editor.status == nil)
        f.readError = .Model(message: "gone")
        #expect(m.editor.laneEvents(chords) == nil)
        #expect(m.editor.status == "gone")
    }

    @Test func anEditAppliesItsSnapshotAndRefreshesTheSelection() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let before = m.snapshot.version
        f.sel = SelectionRow(timeRange: nil, lanes: [], events: [1])
        let ok = m.editor.edit(.event(edit: .delete(events: [1])))
        #expect(ok)
        #expect(m.snapshot.version == before + 1)
        #expect(m.editor.selection.events == [1])
        #expect(f.edits == [.event(edit: .delete(events: [1]))])
    }

    @Test func aRefusedEditShowsAStatus() throws {
        let f = editorFake()
        f.editError = .Model(message: "would make a cycle")
        let m = try DocumentModel(session: f)
        #expect(!m.editor.edit(.event(edit: .delete(events: [1]))))
        #expect(m.editor.status == "would make a cycle")
        m.editor.clearStatus()
        #expect(m.editor.status == nil)
    }

    @Test func selectingABlockSelectsItsEventsLaneAndRange() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let e = m.editor
        e.selectBlock(blockRow(0, 100, events: [1, 2]), in: chords, adding: false)
        #expect(f.sel.events == [1, 2])
        #expect(f.sel.lanes == [LaneRef(seq: 3, lane: 31)])
        #expect(f.sel.timeRange == TickRange(start: 0, end: 100))
        e.selectBlock(blockRow(200, 300, events: [3]), in: grooveLane, adding: true)
        #expect(Set(f.sel.events) == [1, 2, 3])
        #expect(f.sel.timeRange == TickRange(start: 0, end: 300))
        #expect(e.isSelected(blockRow(0, 100, events: [1, 2])))
        e.clearSelection()
        #expect(f.sel.events.isEmpty && f.sel.timeRange == nil)
    }

    @Test func aRulerRangeSelectsEveryShownLane() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.selectRange(TickRange(start: 0, end: 61_440), lanes: [grooveLane, chords])
        #expect(f.sel.lanes == [LaneRef(seq: 3, lane: 30), LaneRef(seq: 3, lane: 31)])
        #expect(f.sel.events.isEmpty)
    }

    @Test func theLayoutIsStoredOnSaveAndReadAtOpen() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.updateLayout { $0.sequenceOrder = [2, 1] }
        m.saveAs(path: "/tmp/x/a.vizdaw")
        #expect(EditorLayout.decode(f.layout).sequenceOrder == [2, 1])
        let reopened = try DocumentModel(session: f)
        #expect(reopened.referenced.map(\.id) == [2, 1])
    }

    @Test func layoutDecodingIsTolerant() {
        #expect(EditorLayout.decode(nil) == EditorLayout())
        #expect(EditorLayout.decode("garbage") == EditorLayout())
        let partial = EditorLayout.decode(#"{"version":1,"lightMode":"on","future":42}"#)
        #expect(partial.lightMode == .on)
        #expect(partial.snap == Snap())
        var l = EditorLayout()
        l.detail = DetailState(kind: .list, target: .block(seq: 3, lane: 31, start: 0, end: 10))
        l.windows = [TornWindow(id: UUID(), detail: DetailState(kind: .pianoRoll, target: .lane(seq: 3, lane: 30)), frame: [1, 2, 3, 4])]
        #expect(EditorLayout.decode(l.encoded()) == l)
    }

    @Test func blockRuleSettingsMapToRules() {
        #expect(BlockRuleSetting.silenceGap(beats: 2).rule == .silenceGap(ticks: 30_720))
        #expect(BlockRuleSetting.fixedBars(4).rule == .fixedBars(bars: 4))
        #expect(BlockRuleSetting.silenceGap(beats: 0).rule == .silenceGap(ticks: 15_360))
        #expect(BlockRuleSetting.wholeLane.rule == .wholeLane)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/swift.sh test`
Expected: compile errors — cannot find `LaneKey`, `EditorLayout`, `SequenceOrder`, no member `editor`.

- [ ] **Step 3: Editor types, layout and order**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/EditorTypes.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// A lane of a sequence, as cache key and in saved layouts.
public struct LaneKey: Hashable, Codable, Sendable {
    public let seq: UInt64
    public let lane: UInt64

    public init(_ seq: UInt64, _ lane: UInt64) {
        self.seq = seq
        self.lane = lane
    }

    public var ref: LaneRef { LaneRef(seq: seq, lane: lane) }
}

/// The editing tool (spec §3.5); editor-specific tools join in 4b–4e.
public enum Tool: String, Codable, CaseIterable, Sendable {
    case arrow, pencil, eraser

    public var title: String {
        switch self {
        case .arrow: "Arrow"
        case .pencil: "Pencil"
        case .eraser: "Eraser"
        }
    }
}

/// Light drawing mode (Plan Ruling 17).
public enum LightMode: String, Codable, CaseIterable, Sendable {
    case auto, on, off

    public var title: String {
        switch self {
        case .auto: "Automatic"
        case .on: "On"
        case .off: "Off"
        }
    }
}

/// The arrangement's block rule with its user-set size.
public enum BlockRuleSetting: Codable, Hashable, Sendable {
    case silenceGap(beats: Int)
    case fixedBars(Int)
    case wholeLane

    public static let choices: [BlockRuleSetting] = [
        .silenceGap(beats: 1), .silenceGap(beats: 2), .silenceGap(beats: 4),
        .fixedBars(1), .fixedBars(2), .fixedBars(4), .fixedBars(8), .wholeLane,
    ]

    public var rule: BlockRule {
        switch self {
        case .silenceGap(let beats): .silenceGap(ticks: Int64(max(beats, 1)) * MeterMath.ppq)
        case .fixedBars(let n): .fixedBars(bars: UInt32(clamping: max(n, 1)))
        case .wholeLane: .wholeLane
        }
    }

    public var title: String {
        switch self {
        case .silenceGap(let beats): "Silence of \(beats) Beat\(beats == 1 ? "" : "s")"
        case .fixedBars(let n): "\(n) Bar\(n == 1 ? "" : "s")"
        case .wholeLane: "Whole Lane"
        }
    }
}

/// The detail editors (spec §3.7, Plan Ruling 15); 4a shows placeholders.
public enum DetailEditorKind: String, Codable, CaseIterable, Sendable {
    case pianoRoll, list, stripChart, drumGrid

    public var title: String {
        switch self {
        case .pianoRoll: "Piano Roll"
        case .list: "List"
        case .stripChart: "Strip Chart"
        case .drumGrid: "Drum Grid"
        }
    }

    /// The digit of its ⌘ shortcut (⌥⌘ opens it in a window).
    public var key: String {
        switch self {
        case .pianoRoll: "1"
        case .list: "2"
        case .stripChart: "3"
        case .drumGrid: "4"
        }
    }

    /// The plan that builds it.
    public var plan: String {
        switch self {
        case .pianoRoll: "4b"
        case .list: "4c"
        case .stripChart: "4d"
        case .drumGrid: "4e"
        }
    }
}

/// What a detail editor shows.
public enum EditorTarget: Codable, Hashable, Sendable {
    case block(seq: UInt64, lane: UInt64, start: Int64, end: Int64)
    case lane(seq: UInt64, lane: UInt64)
    case sequence(UInt64)

    public var seq: UInt64 {
        switch self {
        case .block(let s, _, _, _), .lane(let s, _), .sequence(let s): s
        }
    }

    public var lane: UInt64? {
        switch self {
        case .block(_, let l, _, _), .lane(_, let l): l
        case .sequence: nil
        }
    }
}

/// A detail editor and its target.
public struct DetailState: Codable, Hashable, Sendable {
    public var kind: DetailEditorKind
    public var target: EditorTarget

    public init(kind: DetailEditorKind, target: EditorTarget) {
        self.kind = kind
        self.target = target
    }
}

/// A torn-off editor window (spec §3.7).
public struct TornWindow: Codable, Hashable, Sendable, Identifiable {
    public let id: UUID
    public var detail: DetailState
    /// `[x, y, width, height]` in screen points, once known.
    public var frame: [Double]?

    public init(id: UUID, detail: DetailState, frame: [Double]?) {
        self.id = id
        self.detail = detail
        self.frame = frame
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWAppModel/EditorLayout.swift <<'VIZDAW_EOF'
import Foundation

/// Everything about the editors that is saved with the project but is not
/// document state (Plan Ruling 12): stored as JSON by the session in the
/// package's `ui.json`, written on Save, never an undo step. Decoding is
/// tolerant: a missing or malformed field keeps its default.
public struct EditorLayout: Codable, Equatable, Sendable {
    public var version = 1
    /// The split detail pane's content (`nil`: empty).
    public var detail: DetailState?
    /// The editor a double-clicked block opens in.
    public var lastKind: DetailEditorKind = .pianoRoll
    public var windows: [TornWindow] = []
    /// The sequence sidebar's order (Plan Ruling 12).
    public var sequenceOrder: [UInt64] = []
    public var blockRule: BlockRuleSetting = .silenceGap(beats: 4)
    public var lightMode: LightMode = .auto
    public var snap = Snap()
    public var laneHeight: Double = 48
    public var ticksPerPoint: Double = 384
    /// Follow playback (page mode, Plan Ruling 18).
    public var follow = true

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func field<T: Decodable>(_ key: CodingKeys, _ fallback: T) -> T {
            ((try? c.decodeIfPresent(T.self, forKey: key)) ?? nil) ?? fallback
        }
        let d = EditorLayout()
        version = field(.version, d.version)
        detail = field(.detail, d.detail)
        lastKind = field(.lastKind, d.lastKind)
        windows = field(.windows, d.windows)
        sequenceOrder = field(.sequenceOrder, d.sequenceOrder)
        blockRule = field(.blockRule, d.blockRule)
        lightMode = field(.lightMode, d.lightMode)
        snap = field(.snap, d.snap)
        laneHeight = field(.laneHeight, d.laneHeight)
        ticksPerPoint = field(.ticksPerPoint, d.ticksPerPoint)
        follow = field(.follow, d.follow)
    }

    /// The layout in `json`, or the default when it is missing or unreadable.
    public static func decode(_ json: String?) -> EditorLayout {
        guard let data = json?.data(using: .utf8),
            let l = try? JSONDecoder().decode(EditorLayout.self, from: data)
        else { return EditorLayout() }
        return l
    }

    public func encoded() -> String {
        let e = JSONEncoder()
        e.outputFormatting = [.sortedKeys]
        guard let data = try? e.encode(self) else { return "{}" }
        return String(decoding: data, as: UTF8.self)
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWAppModel/SequenceOrder.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// The sequence sidebar's user order (Plan Ruling 12).
public enum SequenceOrder {
    /// `rows` in the user's order: IDs listed in `order` first, in that
    /// order; the rest after them by ID. Stale IDs are ignored.
    public static func sorted(_ rows: [SeqRow], by order: [UInt64]) -> [SeqRow] {
        var rank: [UInt64: Int] = [:]
        for (i, id) in order.enumerated() where rank[id] == nil { rank[id] = i }
        return rows.sorted { a, b in
            switch (rank[a.id], rank[b.id]) {
            case let (x?, y?): x < y
            case (.some, nil): true
            case (nil, .some): false
            case (nil, nil): a.id < b.id
            }
        }
    }

    /// The full order after a sidebar section showing `visible` (IDs in
    /// display order) moved the rows at `source` to `destination` (the
    /// `onMove` convention); rows outside the section keep their places.
    public static func moved(
        all: [SeqRow], order: [UInt64], visible: [UInt64], from source: IndexSet, to destination: Int
    ) -> [UInt64] {
        let moving = source.sorted().filter { visible.indices.contains($0) }.map { visible[$0] }
        var rest = visible.enumerated().filter { !source.contains($0.offset) }.map(\.element)
        let at = destination - source.filter { $0 < destination }.count
        rest.insert(contentsOf: moving, at: min(max(at, 0), rest.count))
        let members = Set(visible)
        var next = rest.makeIterator()
        return sorted(all, by: order).map { row in
            members.contains(row.id) ? (next.next() ?? row.id) : row.id
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: `EditorModel`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/EditorModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// The editors of one document (spec §3.6): lane caches keyed by revision,
/// the shared selection, edit dispatch, and the Swift-side editor state
/// that is not document state (tool, layout, navigation). Owned by
/// `DocumentModel`. Caches are outside observation: a fetch made while a
/// view draws never invalidates that view; views observe `drawVersion`.
@Observable
@MainActor
public final class EditorModel {
    public let session: any SessionProtocol
    /// Hands a command's snapshot to the document (set by `DocumentModel`,
    /// which calls `sync` back).
    @ObservationIgnored var applySnapshot: (DocSnapshot) -> Void = { _ in }

    public private(set) var revs: [LaneKey: UInt64] = [:]
    public private(set) var snapshotVersion: UInt64 = 0
    /// Changes whenever something drawn may have changed.
    public private(set) var drawVersion = 0
    public private(set) var selection = SelectionRow(timeRange: nil, lanes: [], events: [])
    public private(set) var layout = EditorLayout()
    public var tool: Tool = .arrow
    /// The last failure of a user action (spec §5: non-modal).
    public var status: String?
    /// Sequences left by opening a reference; Back returns to the last.
    public private(set) var navigation: [UInt64] = []

    private struct BlockEntry {
        let rev: UInt64
        let rule: BlockRule
        let blocks: [BlockRow]
        /// A sequence row changed since: reference labels and spans may be old.
        var stale = false
    }

    @ObservationIgnored private var events: [LaneKey: LaneEvents] = [:]
    @ObservationIgnored private var blockCache: [LaneKey: BlockEntry] = [:]
    @ObservationIgnored private var infoCache: [UInt64: (version: UInt64, info: SequenceInfo)] = [:]
    @ObservationIgnored private var sequenceRows: [SeqRow] = []
    @ObservationIgnored private var selectedEvents: Set<UInt64> = []

    @ObservationIgnored public private(set) var eventFetches = 0
    @ObservationIgnored public private(set) var blockFetches = 0
    @ObservationIgnored public private(set) var infoFetches = 0

    public init(session: any SessionProtocol, snapshot: DocSnapshot) {
        self.session = session
        layout = EditorLayout.decode((try? session.uiLayout()) ?? nil)
        sync(snapshot)
        refreshSelection()
    }

    /// Lanes with cached events or blocks (tests).
    var cachedLanes: Set<LaneKey> { Set(events.keys).union(blockCache.keys) }

    // MARK: Snapshots

    /// A new snapshot (spec §3.6): lanes that are gone leave the caches;
    /// lanes whose revision moved are refetched on their next read (their
    /// old rows stay as the fallback while the session is busy); blocks
    /// holding a reference go stale when any sequence row changed (Plan
    /// Ruling 2).
    func sync(_ s: DocSnapshot) {
        var next: [LaneKey: UInt64] = [:]
        for r in s.laneRevs { next[LaneKey(r.seq, r.lane)] = r.rev }
        for key in Array(events.keys) where next[key] == nil { events[key] = nil }
        for key in Array(blockCache.keys) where next[key] == nil { blockCache[key] = nil }
        if s.sequences != sequenceRows {
            for (key, entry) in blockCache where entry.blocks.contains(where: { $0.kind == .reference }) {
                blockCache[key]?.stale = true
            }
            sequenceRows = s.sequences
        }
        revs = next
        snapshotVersion = s.version
        drawVersion += 1
        afterSync(s)
    }

    /// Hook for gesture checks (Task 10).
    func afterSync(_ s: DocSnapshot) {}

    // MARK: Reads

    /// A lane's events at its current revision (fetched when it moved).
    /// `Busy` keeps the stale rows silently; other failures go to `status`.
    public func laneEvents(_ key: LaneKey) -> LaneEvents? {
        if let cached = events[key], cached.rev == revs[key] { return cached }
        do {
            eventFetches += 1
            let fresh = try session.laneEvents(seq: key.seq, lane: key.lane)
            events[key] = fresh
            return fresh
        } catch VizError.Busy {
            return events[key]
        } catch {
            report(error)
            return nil
        }
    }

    /// A lane's blocks under the layout's rule.
    public func blocks(_ key: LaneKey) -> [BlockRow] {
        let rule = layout.blockRule.rule
        if let c = blockCache[key], c.rev == revs[key], c.rule == rule, !c.stale { return c.blocks }
        do {
            blockFetches += 1
            let b = try session.blocks(seq: key.seq, lane: key.lane, rule: rule)
            blockCache[key] = BlockEntry(rev: revs[key] ?? 0, rule: rule, blocks: b)
            return b
        } catch VizError.Busy {
            return blockCache[key]?.blocks ?? []
        } catch {
            report(error)
            return []
        }
    }

    /// Tempo, meter and markers of `seq`, cached per snapshot version.
    public func info(_ seq: UInt64) -> SequenceInfo? {
        if let c = infoCache[seq], c.version == snapshotVersion { return c.info }
        do {
            infoFetches += 1
            let i = try session.sequenceInfo(seq: seq)
            infoCache[seq] = (snapshotVersion, i)
            return i
        } catch VizError.Busy {
            return infoCache[seq]?.info
        } catch {
            report(error)
            return nil
        }
    }

    // MARK: Edits

    /// Sends one editor command (one undo step). Failures show in
    /// `status`; returns whether it applied.
    @discardableResult
    public func edit(_ cmd: EditCommand, filter: ViewFilterRow? = nil) -> Bool {
        do {
            applySnapshot(try session.edit(cmd: cmd, filter: filter))
            refreshSelection()
            return true
        } catch {
            report(error)
            return false
        }
    }

    public func report(_ error: any Error) {
        if let e = error as? VizError {
            status = e.message
        } else {
            status = error.localizedDescription
        }
    }

    public func clearStatus() { status = nil }

    // MARK: Selection (Plan Ruling 19)

    public func setSelection(_ s: SelectionRow) {
        selection = s
        selectedEvents = Set(s.events)
        drawVersion += 1
        do {
            try session.setSelection(selection: s)
        } catch {
            report(error)
        }
    }

    /// Reads the selection back (after edits, undo and redo).
    public func refreshSelection() {
        guard let s = try? session.selection(), s != selection else { return }
        selection = s
        selectedEvents = Set(s.events)
        drawVersion += 1
    }

    public func clearSelection() {
        setSelection(SelectionRow(timeRange: nil, lanes: [], events: []))
    }

    public func isSelected(event id: UInt64) -> Bool { selectedEvents.contains(id) }

    /// Every event of `b` is selected.
    public func isSelected(_ b: BlockRow) -> Bool {
        !b.events.isEmpty && b.events.allSatisfy(selectedEvents.contains)
    }

    /// Selects a block's events, its lane and its time range; `adding`
    /// unites them with the current selection.
    public func selectBlock(_ b: BlockRow, in key: LaneKey, adding: Bool) {
        var s = adding ? selection : SelectionRow(timeRange: nil, lanes: [], events: [])
        for id in b.events where !s.events.contains(id) { s.events.append(id) }
        if !s.lanes.contains(key.ref) { s.lanes.append(key.ref) }
        let range = TickRange(start: b.start, end: max(b.end, b.start + 1))
        if let r = s.timeRange, adding {
            s.timeRange = TickRange(start: min(r.start, range.start), end: max(r.end, range.end))
        } else {
            s.timeRange = range
        }
        setSelection(s)
    }

    /// A time range over `lanes` (a ruler drag), no events.
    public func selectRange(_ range: TickRange, lanes: [LaneKey]) {
        setSelection(SelectionRow(timeRange: range, lanes: lanes.map(\.ref), events: []))
    }

    // MARK: Layout and navigation

    public func updateLayout(_ change: (inout EditorLayout) -> Void) {
        change(&layout)
        drawVersion += 1
    }

    /// Hands the layout to the session; the next save writes it.
    public func storeLayout() {
        try? session.setUiLayout(json: layout.encoded())
    }

    public func push(_ parent: UInt64) { navigation.append(parent) }

    public func pop() -> UInt64? { navigation.popLast() }

    public func resetNavigation() { navigation = [] }
}
VIZDAW_EOF
```

- [ ] **Step 5: Wire it into `DocumentModel`**

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
public final class DocumentModel {
    public let session: any SessionProtocol
    public private(set) var snapshot: DocSnapshot
```
Replace:
```swift
public final class DocumentModel {
    public let session: any SessionProtocol
    public private(set) var snapshot: DocSnapshot
    /// The editors (plan 4a).
    public let editor: EditorModel
```

Find:
```swift
        self.session = session
        snapshot = try session.snapshot()
```
Replace:
```swift
        self.session = session
        snapshot = try session.snapshot()
        editor = EditorModel(session: session, snapshot: snapshot)
```

Find:
```swift
        clock.update(transport, pollInterval: transport.playing ? 0.033 : 0.25)
        apply(snapshot)
    }
```
Replace:
```swift
        clock.update(transport, pollInterval: transport.playing ? 0.033 : 0.25)
        apply(snapshot)
        editor.applySnapshot = { [weak self] in self?.apply($0) }
    }
```

Find:
```swift
    public var topLevel: [SeqRow] { snapshot.sequences.filter(\.topLevel) }
    public var referenced: [SeqRow] { snapshot.sequences.filter { !$0.topLevel } }
```
Replace:
```swift
    public var topLevel: [SeqRow] {
        SequenceOrder.sorted(snapshot.sequences.filter(\.topLevel), by: editor.layout.sequenceOrder)
    }
    public var referenced: [SeqRow] {
        SequenceOrder.sorted(snapshot.sequences.filter { !$0.topLevel }, by: editor.layout.sequenceOrder)
    }
```

Find:
```swift
    public func select(_ seq: UInt64?) {
        guard seq != selectedSeq else { return }
        selectedSeq = seq
        refreshLanes()
    }
```
Replace:
```swift
    public func select(_ seq: UInt64?) {
        guard seq != selectedSeq else { return }
        selectedSeq = seq
        refreshLanes()
    }

    /// The sidebar chose a sequence: reference navigation starts over.
    public func choose(_ seq: UInt64?) {
        editor.resetNavigation()
        select(seq)
    }
```

Find:
```swift
    public func save() {
        perform("Save Failed") { try $0.save() }
    }

    public func saveAs(path: String) {
        perform("Save Failed") { try $0.saveAs(path: path) }
    }
```
Replace:
```swift
    public func save() {
        editor.storeLayout()
        perform("Save Failed") { try $0.save() }
    }

    public func saveAs(path: String) {
        editor.storeLayout()
        perform("Save Failed") { try $0.saveAs(path: path) }
    }
```

Find:
```swift
    func apply(_ s: DocSnapshot) {
        snapshot = s
```
Replace:
```swift
    func apply(_ s: DocSnapshot) {
        snapshot = s
        editor.sync(s)
```

Also refresh the shared selection after Undo and Redo:

Find:
```swift
    public func undo() { command("Undo Failed", quietWhenBusy: true) { try $0.undo() } }

    public func redo() { command("Redo Failed", quietWhenBusy: true) { try $0.redo() } }
```
Replace:
```swift
    public func undo() {
        command("Undo Failed", quietWhenBusy: true) { try $0.undo() }
        editor.refreshSelection()
    }

    public func redo() {
        command("Redo Failed", quietWhenBusy: true) { try $0.redo() }
        editor.refreshSelection()
    }
```

- [ ] **Step 6: Run the tests**

Run: `scripts/swift.sh test`
Expected: PASS — 2 + 13 new tests (136 Swift tests); the existing `DocumentModelTests` still pass (the sidebar order is ID order when no layout is saved).

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Sources/VizDAWAppModel/EditorTypes.swift app/Sources/VizDAWAppModel/EditorLayout.swift \
  app/Sources/VizDAWAppModel/SequenceOrder.swift app/Sources/VizDAWAppModel/EditorModel.swift \
  app/Sources/VizDAWAppModel/DocumentModel.swift app/Tests/VizDAWAppModelTests/EditorModelTests.swift \
  app/Tests/VizDAWAppModelTests/SequenceOrderTests.swift
git commit -m "feat(app): editor model with revision-keyed caches, selection and layout

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 136 Swift tests.

---

### Task 10: Gestures and the Busy contract

**Files:**
- Create: `app/Sources/VizDAWAppModel/EditorModel+Gestures.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Test: `app/Tests/VizDAWAppModelTests/GestureTests.swift` (new)

**Interfaces:**
- Consumes: Task 4's `beginEdit(label:)`, `editGesture(cmd:filter:)`, `endEdit()`, `abortEdit()`; Task 9's `EditorModel` (`applySnapshot`, `report`, `refreshSelection`, `afterSync`), FakeSession `gestureErrors`, `calls` (`"begin <label>"`, `"gesture"`, `"gesture refused"`, `"end"`, `"abort"`), `beginError`, `mixerError` (thrown by `endEdit`).
- Produces: `Gesture { label: String, lanes: Set<LaneKey>, applied: EditCommand?, pendingFinal: EditCommand?, finishing: Bool }`; `EditorModel.gesture: Gesture?`; `EditorModel.beginGesture(_ label: String, lanes: Set<LaneKey>) -> Bool`, `updateGesture(_ target: EditCommand?)` (`nil` = back at the start), `endGesture(_ final: EditCommand?)`, `cancelGesture()`, `tick()`; `DocumentModel.idle()` calls `editor.tick()` first.

Spec §3.8 and §5, Rulings 7 and 8. Every target is the full change from the drag's start; `nil` means "no change". A refused intermediate target is silent (Rust shows the start state; the snapshot is re-read); a refused final target aborts and shows the reason; Busy anywhere is retried (final) or dropped (intermediate).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/GestureTests.swift <<'VIZDAW_EOF'
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let move1: EditCommand = .event(edit: .move(events: [1], delta: 100, toLane: nil))
private let move2: EditCommand = .event(edit: .move(events: [1], delta: 200, toLane: nil))

@MainActor
@Suite struct GestureTests {
    func open() throws -> (FakeSession, DocumentModel, EditorModel) {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        f.calls = []
        #expect(m.editor.beginGesture("Move Block", lanes: [chords]))
        return (f, m, m.editor)
    }

    @Test func aDragIsOneEditEndingWithTheLastTarget() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        e.updateGesture(move2)
        e.endGesture(move2)
        #expect(f.calls == ["begin Move Block", "gesture", "gesture", "end"])
        #expect(f.gestureEdits == [move1, move2])
        #expect(e.gesture == nil)
    }

    @Test func busyIntermediateEditIsDroppedSilently() throws {
        let (f, _, e) = try open()
        f.gestureErrors = [.Busy(message: "busy")]
        e.updateGesture(move1)
        #expect(e.status == nil)
        #expect(e.gesture != nil)
        e.updateGesture(move2)
        e.endGesture(move2)
        #expect(f.calls == ["begin Move Block", "gesture refused", "gesture", "end"])
    }

    @Test func busyFinalEditIsRetriedOnTick() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        f.gestureErrors = [.Busy(message: "busy")]
        e.endGesture(move2)
        #expect(e.gesture?.finishing == true)
        #expect(!f.calls.contains("end"))
        e.tick()
        #expect(f.gestureEdits == [move1, move2])
        #expect(f.calls.last == "end")
        #expect(e.gesture == nil)
    }

    @Test func thePluginTimerRetriesThePendingFinal() throws {
        let (f, m, e) = try open()
        f.gestureErrors = [.Busy(message: "busy")]
        e.endGesture(move1)
        m.idle()
        #expect(f.calls.last == "end")
        #expect(e.gesture == nil)
    }

    @Test func aBusyEndEditIsRetriedToo() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        f.mixerError = .Busy(message: "busy")
        e.endGesture(move1)
        #expect(e.gesture != nil)
        f.mixerError = nil
        e.tick()
        #expect(e.gesture == nil)
        #expect(f.gestureEdits == [move1], "the applied target is not sent again")
    }

    @Test func escapeWhileTheFinalEditWaitsAborts() throws {
        let (f, _, e) = try open()
        f.gestureErrors = [.Busy(message: "busy")]
        e.endGesture(move1)
        e.cancelGesture()
        #expect(f.calls.last == "abort")
        e.tick()
        #expect(f.calls.last == "abort")
        #expect(e.gesture == nil)
    }

    @Test func refusedFinalEditAbortsAndShowsStatus() throws {
        let (f, _, e) = try open()
        f.gestureErrors = [.Model(message: "would make a cycle")]
        e.endGesture(move1)
        #expect(f.calls == ["begin Move Block", "gesture refused", "abort"])
        #expect(e.status == "would make a cycle")
        #expect(e.gesture == nil)
    }

    @Test func aRefusedIntermediateTargetIsSilent() throws {
        let (f, _, e) = try open()
        f.gestureErrors = [.Model(message: "would make a cycle")]
        e.updateGesture(move1)
        #expect(e.status == nil)
        #expect(e.gesture?.applied == nil)
    }

    @Test func aDragWhoseLaneVanishesIsCancelled() throws {
        let (f, m, e) = try open()
        e.updateGesture(move1)
        var s = f.snap
        s.version += 1
        s.laneRevs.removeAll { $0.lane == 31 }
        f.idleUpdates = [IdleUpdate(snapshot: s, slotsChanged: false, paramsChanged: [], problems: [])]
        m.idle()
        #expect(f.calls.last == "abort")
        #expect(e.gesture == nil)
        #expect(e.status?.contains("cancelled") == true)
    }

    @Test func endingWithoutAMoveClosesAnEmptyEdit() throws {
        let (f, _, e) = try open()
        e.endGesture(nil)
        #expect(f.calls == ["begin Move Block", "end"])
    }

    @Test func returningToTheStartRewinds() throws {
        let (f, _, e) = try open()
        e.updateGesture(move1)
        e.updateGesture(nil)
        #expect(f.calls == ["begin Move Block", "gesture", "abort", "begin Move Block"])
        e.endGesture(nil)
        #expect(f.calls.last == "end")
    }

    @Test func onlyOneGestureAtATime() throws {
        let (_, _, e) = try open()
        #expect(!e.beginGesture("Copy Block", lanes: []))
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/swift.sh test`
Expected: compile errors — no member `beginGesture`, `gesture`.

- [ ] **Step 3: Implement gestures**

In `app/Sources/VizDAWAppModel/EditorModel.swift`:

Find:
```swift
    /// Sequences left by opening a reference; Back returns to the last.
    public private(set) var navigation: [UInt64] = []
```
Replace:
```swift
    /// Sequences left by opening a reference; Back returns to the last.
    public private(set) var navigation: [UInt64] = []
    /// The drag in progress (spec §3.8).
    public internal(set) var gesture: Gesture?
```

Find:
```swift
    /// Hook for gesture checks (Task 10).
    func afterSync(_ s: DocSnapshot) {}
```
Replace:
```swift
    /// A gesture whose lanes vanished (undo in another window, a plugin
    /// timer step) is aborted (spec §5).
    func afterSync(_ s: DocSnapshot) {
        guard let g = gesture else { return }
        let alive = Set(s.laneRevs.map { LaneKey($0.seq, $0.lane) })
        guard !g.lanes.isSubset(of: alive) else { return }
        cancelGesture()
        status = "The lane was removed; the drag was cancelled."
    }
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/EditorModel+Gestures.swift <<'VIZDAW_EOF'
import VizDAWCore

/// A drag in progress: one undo step named `label` (spec §3.2, §3.8).
public struct Gesture: Equatable, Sendable {
    public let label: String
    /// The lanes it edits; it is cancelled when one disappears (spec §5).
    public let lanes: Set<LaneKey>
    /// The target the document shows now (`nil`: the start state).
    public var applied: EditCommand?
    /// The mouse-up target, until it has been applied and the edit closed.
    public var pendingFinal: EditCommand?
    /// Mouse-up came; `tick()` retries what is left.
    public var finishing = false
}

extension EditorModel {
    /// Opens a gesture once the pointer left the dead zone. False when
    /// one is already open or the session refused.
    @discardableResult
    public func beginGesture(_ label: String, lanes: Set<LaneKey>) -> Bool {
        guard gesture == nil else { return false }
        do {
            try session.beginEdit(label: label)
            gesture = Gesture(label: label, lanes: lanes)
            return true
        } catch {
            report(error)
            return false
        }
    }

    /// The pointer moved; `target` is the full change from the drag's start
    /// (`nil`: none). Busy is dropped (the next move resends); a refused
    /// target leaves the start state on screen, silently.
    public func updateGesture(_ target: EditCommand?) {
        guard var g = gesture, !g.finishing, target != g.applied else { return }
        guard let target else {
            rewind(&g)
            gesture = g
            return
        }
        do {
            applySnapshot(try session.editGesture(cmd: target, filter: nil))
            g.applied = target
        } catch VizError.Busy {
            // Dropped: the next pointer move sends the full target again.
        } catch {
            // Refused: the session rewound to the start state.
            g.applied = nil
            if let s = try? session.snapshot() { applySnapshot(s) }
        }
        gesture = g
    }

    /// Mouse-up: `final` becomes the gesture's content and the edit closes
    /// as one undo step; Busy leaves it pending for `tick()`.
    public func endGesture(_ final: EditCommand?) {
        guard var g = gesture else { return }
        g.finishing = true
        g.pendingFinal = final
        gesture = g
        finish()
    }

    /// Escape, or the gesture's lanes vanished: the start state comes back
    /// and the edit closes without an undo step.
    public func cancelGesture() {
        guard gesture != nil else { return }
        gesture = nil
        do {
            applySnapshot(try session.abortEdit())
        } catch {
            report(error)
        }
        refreshSelection()
    }

    /// Retries a mouse-up that met Busy (the 60 Hz plugin timer calls it).
    public func tick() {
        if gesture?.finishing == true { finish() }
    }

    private func rewind(_ g: inout Gesture) {
        guard g.applied != nil else { return }
        do {
            applySnapshot(try session.abortEdit())
            try session.beginEdit(label: g.label)
            g.applied = nil
        } catch {
            report(error)
        }
    }

    private func finish() {
        guard var g = gesture else { return }
        if g.pendingFinal == nil, g.applied != nil {
            cancelGesture()
            return
        }
        if let cmd = g.pendingFinal, cmd != g.applied {
            do {
                applySnapshot(try session.editGesture(cmd: cmd, filter: nil))
                g.applied = cmd
                gesture = g
            } catch VizError.Busy {
                return
            } catch {
                cancelGesture()
                report(error)
                return
            }
        }
        do {
            let s = try session.endEdit()
            gesture = nil
            applySnapshot(s)
            refreshSelection()
        } catch VizError.Busy {
            return
        } catch {
            gesture = nil
            report(error)
            if let s = try? session.abortEdit() { applySnapshot(s) }
        }
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
    public func idle() {
        guard let u = try? session.idle() else { return }
```
Replace:
```swift
    public func idle() {
        // A drag's mouse-up that met Busy is retried here (spec §3.8).
        editor.tick()
        guard let u = try? session.idle() else { return }
```

- [ ] **Step 4: Run the tests**

Run: `scripts/swift.sh test`
Expected: PASS — 12 new tests (148 Swift tests).

`Gesture` gets a memberwise initialiser with defaults for `applied`, `pendingFinal` and `finishing`, so `Gesture(label:lanes:)` compiles; if Swift complains that the synthesized initialiser is internal, that is fine (only this module calls it).

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Sources/VizDAWAppModel/EditorModel+Gestures.swift app/Sources/VizDAWAppModel/EditorModel.swift \
  app/Sources/VizDAWAppModel/DocumentModel.swift app/Tests/VizDAWAppModelTests/GestureTests.swift
git commit -m "feat(app): drags as one edit with the Busy contract

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 148 Swift tests.

---

### Task 11: Tool state machine, auto-scroll, ruler selection and `EditorCanvas`

**Files:**
- Create: `app/Sources/VizDAWEditors/ToolMachine.swift`, `app/Sources/VizDAWEditors/Scrolling.swift`, `app/Sources/VizDAWEditors/EditorCanvas.swift`
- Test: `app/Tests/VizDAWEditorsTests/ToolMachineTests.swift`, `app/Tests/VizDAWEditorsTests/ScrollingTests.swift`, `app/Tests/VizDAWEditorsTests/EditorCanvasTests.swift` (new)

**Interfaces:**
- Consumes: Task 7's `TimeAxis`, `Grid`, `MeterMath`, `Snap`; Task 5's `TickRange`; Task 8's `TransportClock` (through a closure).
- Produces:
  - `Modifiers: OptionSet { shift, control, option, command }`, `Modifiers(event: NSEvent)`.
  - `ToolMachine<Hit: Equatable & Sendable>` with `phase: Phase` (`idle`, `pressed(hit:at:clicks:modifiers:)`, `dragging(hit:from:modifiers:)`), `static deadZone` (3), `down(_:at:clicks:modifiers:) -> [Effect]`, `drag(to:modifiers:) -> [Effect]`, `up(at:modifiers:) -> [Effect]`, `cancel() -> [Effect]`; `Effect` = `click(hit:at:clicks:modifiers:)`, `begin(hit:from:modifiers:)`, `move(hit:from:to:modifiers:)`, `end(hit:from:to:modifiers:)`, `cancel(hit:)`.
  - `AutoScroll.speed(x:width:) -> CGFloat` (`edge` 24, 20 pt/s per point, `maxSpeed` 1 200); `PageFollow.origin(forPlayhead:axis:width:) -> Double?` (`margin` 16); `RulerSelection.range(fromTick:toX:axis:snap:meter:) -> TickRange?`.
  - `open class EditorCanvas: NSView` with subviews `grid`, `content`, `overlay`, `ruler` (`CanvasLayerView`, layer-backed, flipped, transparent to hits) and `playheadLayer`; properties `axis`, `verticalOffset`, `follow`, `rulerPreview: TickRange?`; overridable `meter`, `snap`, `selectedRange`, `contentHeight`, `accessibilityName`, `drawContent(in:)`, `drawOverlay(in:)`, `contentMouseDown(_:clicks:modifiers:)`, `contentMouseDragged(_:modifiers:)`, `contentMouseUp(_:modifiers:)`, `contentCancel()`, `accessibilityItems() -> [CanvasAccessibilityItem]`; callbacks `playheadSource: ((UInt64) -> Double?)?`, `onRulerRange: ((TickRange) -> Void)?`, `onRulerClick: ((Int64) -> Void)?`, `onToggleSnap: (() -> Void)?`, `onAxisChange: ((TimeAxis) -> Void)?`, `onVerticalScroll: ((CGFloat) -> Void)?`; `setPlaying(_:)`, `reload()`, `static hostNow() -> UInt64`, `static rulerHeight` (22).
  - `CanvasAccessibilityItem { label: String, frame: CGRect, selected: Bool, press: (@MainActor () -> Void)? }`.

Spec §3.5, Rulings 16 and 18. Content points handed to subclasses are in the `content` view's coordinates (y = 0 is the top of the content area, below the ruler). A drag in the ruler selects a time range whatever the tool; a click in the ruler locates. `G` toggles snap; Escape cancels the gesture; ⌘-scroll and pinch zoom around the pointer.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/ToolMachineTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Testing
@testable import VizDAWEditors

private typealias M = ToolMachine<String>

@Suite struct ToolMachineTests {
    @Test func aClickWithoutMovementIsAClick() {
        var m = M()
        #expect(m.down("a", at: CGPoint(x: 10, y: 10), clicks: 2, modifiers: [.shift]).isEmpty)
        #expect(m.drag(to: CGPoint(x: 12, y: 11), modifiers: []).isEmpty, "inside the dead zone")
        #expect(
            m.up(at: CGPoint(x: 12, y: 11), modifiers: [])
                == [.click(hit: "a", at: CGPoint(x: 10, y: 10), clicks: 2, modifiers: [.shift])])
        #expect(m.phase == .idle)
    }

    @Test func movingPastThreePointsBeginsADrag() {
        var m = M()
        _ = m.down("a", at: .zero, clicks: 1, modifiers: [])
        #expect(m.drag(to: CGPoint(x: 3, y: 0), modifiers: []).isEmpty, "exactly 3 is still inside")
        let fx = m.drag(to: CGPoint(x: 4, y: 0), modifiers: [.option])
        #expect(
            fx == [
                .begin(hit: "a", from: .zero, modifiers: [.option]),
                .move(hit: "a", from: .zero, to: CGPoint(x: 4, y: 0), modifiers: [.option]),
            ])
        #expect(
            m.drag(to: CGPoint(x: 9, y: 2), modifiers: [])
                == [.move(hit: "a", from: .zero, to: CGPoint(x: 9, y: 2), modifiers: [])])
        #expect(
            m.up(at: CGPoint(x: 9, y: 2), modifiers: [])
                == [.end(hit: "a", from: .zero, to: CGPoint(x: 9, y: 2), modifiers: [])])
    }

    @Test func escapeCancelsADragAndIgnoresTheRest() {
        var m = M()
        _ = m.down("a", at: .zero, clicks: 1, modifiers: [])
        _ = m.drag(to: CGPoint(x: 10, y: 0), modifiers: [])
        #expect(m.cancel() == [.cancel(hit: "a")])
        #expect(m.drag(to: CGPoint(x: 20, y: 0), modifiers: []).isEmpty)
        #expect(m.up(at: CGPoint(x: 20, y: 0), modifiers: []).isEmpty)
    }

    @Test func escapeBeforeTheDeadZoneDoesNothing() {
        var m = M()
        _ = m.down("a", at: .zero, clicks: 1, modifiers: [])
        #expect(m.cancel().isEmpty)
        #expect(m.up(at: .zero, modifiers: []).isEmpty)
    }

    @Test func aDownDuringADragCancelsTheLostOne() {
        var m = M()
        _ = m.down("a", at: .zero, clicks: 1, modifiers: [])
        _ = m.drag(to: CGPoint(x: 10, y: 0), modifiers: [])
        #expect(m.down("b", at: .zero, clicks: 1, modifiers: []) == [.cancel(hit: "a")])
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWEditorsTests/ScrollingTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

@Suite struct ScrollingTests {
    @Test func autoScrollStartsNearTheEdges() {
        #expect(AutoScroll.speed(x: 100, width: 400) == 0)
        #expect(AutoScroll.speed(x: 24, width: 400) == 0)
        #expect(AutoScroll.speed(x: 14, width: 400) == -200)
        #expect(AutoScroll.speed(x: 390, width: 400) == 280)
        #expect(AutoScroll.speed(x: -500, width: 400) == -AutoScroll.maxSpeed)
    }

    @Test func pageFollowJumpsWhenThePlayheadLeaves() {
        let a = TimeAxis(ticksPerPoint: 100, originTick: 0)
        #expect(PageFollow.origin(forPlayhead: 5_000, axis: a, width: 400) == nil)
        #expect(PageFollow.origin(forPlayhead: 41_000, axis: a, width: 400) == 41_000 - 1_600)
        #expect(PageFollow.origin(forPlayhead: -100, axis: a, width: 400) == -100 - 1_600)
    }

    @Test func rulerRangesSnapAndIgnoreDirection() {
        let a = TimeAxis(ticksPerPoint: 128, originTick: 0)
        let m = [MeterRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        let r = RulerSelection.range(fromTick: a.tick(atX: 250), toX: 10, axis: a, snap: Snap(), meter: m)
        #expect(r == TickRange(start: 0, end: 30_720))
        #expect(RulerSelection.range(fromTick: 100, toX: 1, axis: a, snap: Snap(), meter: m) == nil)
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWEditorsTests/EditorCanvasTests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
@testable import VizDAWEditors

@MainActor
final class ItemsCanvas: EditorCanvas {
    var pressed = 0
    override func accessibilityItems() -> [CanvasAccessibilityItem] {
        [
            CanvasAccessibilityItem(
                label: "Block Chords, bars 1 to 2", frame: CGRect(x: 0, y: 0, width: 50, height: 20),
                selected: true, press: { [unowned self] in self.pressed += 1 }),
            CanvasAccessibilityItem(
                label: "Block Bass, bar 3", frame: CGRect(x: 60, y: 0, width: 50, height: 20),
                selected: false, press: nil),
        ]
    }
}

@MainActor
@Suite struct EditorCanvasTests {
    @Test func theRulerSitsAboveTheContentLayers() {
        let c = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        c.layout()
        #expect(c.ruler.frame == NSRect(x: 0, y: 0, width: 400, height: EditorCanvas.rulerHeight))
        #expect(c.content.frame == NSRect(x: 0, y: 22, width: 400, height: 278))
        #expect(c.grid.frame == c.content.frame && c.overlay.frame == c.content.frame)
        #expect(c.content.hitTest(NSPoint(x: 10, y: 10)) == nil, "events go to the canvas")
    }

    @Test func itIsAnAccessibilityContainerOfItsItems() {
        let c = ItemsCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        c.layout()
        #expect(c.accessibilityRole() == .group)
        let kids = (c.accessibilityChildren() ?? []).compactMap { $0 as? NSAccessibilityElement }
        #expect(kids.map { $0.accessibilityLabel() ?? "" } == ["Block Chords, bars 1 to 2", "Block Bass, bar 3"])
        #expect(kids[0].isAccessibilitySelected())
        #expect(kids[0].accessibilityPerformPress())
        #expect(c.pressed == 1)
        #expect(!kids[1].accessibilityPerformPress())
    }

    @Test func thePlayheadFollowsTheSourceWithoutAnimation() {
        let c = EditorCanvas(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
        c.layout()
        c.axis = TimeAxis(ticksPerPoint: 100, originTick: 0)
        c.playheadSource = { _ in 10_000 }
        c.setPlaying(false)
        #expect(!c.playheadLayer.isHidden)
        #expect(c.playheadLayer.frame.minX == 100)
        c.playheadSource = { _ in nil }
        c.setPlaying(false)
        #expect(c.playheadLayer.isHidden)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/swift.sh test`
Expected: compile errors — cannot find `ToolMachine`, `AutoScroll`, `EditorCanvas`.

- [ ] **Step 3: Pure interaction types**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/ToolMachine.swift <<'VIZDAW_EOF'
import AppKit

/// Keyboard modifiers of a pointer event.
public struct Modifiers: OptionSet, Hashable, Sendable {
    public let rawValue: Int
    public init(rawValue: Int) { self.rawValue = rawValue }

    public static let shift = Modifiers(rawValue: 1)
    public static let control = Modifiers(rawValue: 2)
    public static let option = Modifiers(rawValue: 4)
    public static let command = Modifiers(rawValue: 8)

    @MainActor
    public init(event: NSEvent) {
        let f = event.modifierFlags
        var m: Modifiers = []
        if f.contains(.shift) { m.insert(.shift) }
        if f.contains(.control) { m.insert(.control) }
        if f.contains(.option) { m.insert(.option) }
        if f.contains(.command) { m.insert(.command) }
        self = m
    }
}

/// Pointer gestures (spec §3.5): a press becomes a drag once the pointer
/// moved more than `deadZone` points (the transaction opens then); release
/// ends it; Escape cancels it. A press released inside the dead zone is a
/// click. `Hit` is what the press landed on (editor-specific).
public struct ToolMachine<Hit: Equatable & Sendable>: Sendable {
    public static var deadZone: CGFloat { 3 }

    public enum Phase: Equatable, Sendable {
        case idle
        case pressed(hit: Hit, at: CGPoint, clicks: Int, modifiers: Modifiers)
        case dragging(hit: Hit, from: CGPoint, modifiers: Modifiers)
    }

    public enum Effect: Equatable, Sendable {
        case click(hit: Hit, at: CGPoint, clicks: Int, modifiers: Modifiers)
        case begin(hit: Hit, from: CGPoint, modifiers: Modifiers)
        case move(hit: Hit, from: CGPoint, to: CGPoint, modifiers: Modifiers)
        case end(hit: Hit, from: CGPoint, to: CGPoint, modifiers: Modifiers)
        case cancel(hit: Hit)
    }

    public private(set) var phase: Phase = .idle

    public init() {}

    /// A press. A drag still open (its release was lost) is cancelled.
    public mutating func down(_ hit: Hit, at p: CGPoint, clicks: Int, modifiers: Modifiers) -> [Effect] {
        var out: [Effect] = []
        if case .dragging(let h, _, _) = phase { out.append(.cancel(hit: h)) }
        phase = .pressed(hit: hit, at: p, clicks: clicks, modifiers: modifiers)
        return out
    }

    public mutating func drag(to p: CGPoint, modifiers: Modifiers) -> [Effect] {
        switch phase {
        case .idle:
            return []
        case let .pressed(hit, start, _, _):
            guard hypot(p.x - start.x, p.y - start.y) > Self.deadZone else { return [] }
            phase = .dragging(hit: hit, from: start, modifiers: modifiers)
            return [
                .begin(hit: hit, from: start, modifiers: modifiers),
                .move(hit: hit, from: start, to: p, modifiers: modifiers),
            ]
        case let .dragging(hit, start, _):
            phase = .dragging(hit: hit, from: start, modifiers: modifiers)
            return [.move(hit: hit, from: start, to: p, modifiers: modifiers)]
        }
    }

    public mutating func up(at p: CGPoint, modifiers: Modifiers) -> [Effect] {
        defer { phase = .idle }
        switch phase {
        case .idle:
            return []
        case let .pressed(hit, start, clicks, downModifiers):
            return [.click(hit: hit, at: start, clicks: clicks, modifiers: downModifiers)]
        case let .dragging(hit, start, _):
            return [.end(hit: hit, from: start, to: p, modifiers: modifiers)]
        }
    }

    /// Escape.
    public mutating func cancel() -> [Effect] {
        defer { phase = .idle }
        if case .dragging(let hit, _, _) = phase { return [.cancel(hit: hit)] }
        return []
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/Scrolling.swift <<'VIZDAW_EOF'
import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// Scrolling while a drag passes an edge (Plan Ruling 18).
public enum AutoScroll {
    /// Distance from an edge where scrolling starts.
    public static let edge: CGFloat = 24
    /// Points per second for each point of overshoot.
    public static let rate: CGFloat = 20
    public static let maxSpeed: CGFloat = 1_200

    /// Horizontal scroll speed in points per second (negative: towards
    /// earlier time) for a pointer at `x` in a view `width` wide.
    public static func speed(x: CGFloat, width: CGFloat) -> CGFloat {
        if x < edge { return -min(maxSpeed, (edge - x) * rate) }
        if x > width - edge { return min(maxSpeed, (x - (width - edge)) * rate) }
        return 0
    }
}

/// Follow playback in page mode (Plan Ruling 18).
public enum PageFollow {
    public static let margin: CGFloat = 16

    /// The new origin tick when the playhead at `tick` is outside the
    /// visible width, else nil.
    public static func origin(forPlayhead tick: Double, axis: TimeAxis, width: CGFloat) -> Double? {
        let x = (tick - axis.originTick) / axis.ticksPerPoint
        guard x < 0 || x > Double(width) else { return nil }
        return tick - Double(margin) * axis.ticksPerPoint
    }
}

/// A ruler drag's time range (spec §3.5, parent §9).
public enum RulerSelection {
    /// The snapped range between the press (`anchor`, a tick, so it
    /// survives auto-scroll) and the pointer at `x`; nil when empty.
    public static func range(
        fromTick anchor: Int64, toX x: CGFloat, axis: TimeAxis, snap: Snap, meter: [MeterRow]
    ) -> TickRange? {
        let other = axis.tick(atX: x)
        let a = snap.apply(min(anchor, other), meter: meter)
        let b = snap.apply(max(anchor, other), meter: meter)
        return b > a ? TickRange(start: a, end: b) : nil
    }
}
VIZDAW_EOF
```

In `rulerRangesSnapAndIgnoreDirection`: the anchor is x = 250 (tick 32 000, snaps to 30 720) and the pointer x = 10 (tick 1 280, snaps to 0), so the range is 0–30 720; from tick 100 to x = 1 (tick 128) both snap to 0: nil.

- [ ] **Step 4: `EditorCanvas`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/EditorCanvas.swift <<'VIZDAW_EOF'
import AppKit
import QuartzCore
import VizDAWAppModel
import VizDAWCore

/// A layer-backed, flipped view that draws through a closure and lets
/// every event through to its canvas.
public final class CanvasLayerView: NSView {
    var paint: (@MainActor (CGRect) -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
    }

    required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var isOpaque: Bool { false }
    public override func hitTest(_ point: NSPoint) -> NSView? { nil }
    public override func draw(_ dirtyRect: NSRect) { paint?(dirtyRect) }
}

/// One element an editor exposes to VoiceOver (spec §3.5).
public struct CanvasAccessibilityItem {
    public let label: String
    /// In `content` coordinates.
    public let frame: CGRect
    public let selected: Bool
    public let press: (@MainActor () -> Void)?

    public init(label: String, frame: CGRect, selected: Bool, press: (@MainActor () -> Void)?) {
        self.label = label
        self.frame = frame
        self.selected = selected
        self.press = press
    }
}

@MainActor
final class CanvasElement: NSAccessibilityElement {
    private let press: (@MainActor () -> Void)?

    init(_ item: CanvasAccessibilityItem, parent: NSView, frame: CGRect) {
        press = item.press
        super.init()
        setAccessibilityRole(.button)
        setAccessibilityLabel(item.label)
        setAccessibilityParent(parent)
        setAccessibilityFrameInParentSpace(frame)
        setAccessibilitySelected(item.selected)
    }

    override func accessibilityPerformPress() -> Bool {
        guard let press else { return false }
        press()
        return true
    }
}

/// The shared editor surface (spec §3.5): a ruler on top and, below it,
/// grid, content and overlay layers (each a layer-backed view, redrawn
/// only for its dirty rect; the grid only when the axis or size changes)
/// plus a playhead layer moved — not redrawn — on every display frame.
/// Subclasses draw content and handle content gestures; the canvas owns
/// scrolling, zoom, the ruler, auto-scroll, snap toggling, the playhead
/// and the accessibility container.
@MainActor
open class EditorCanvas: NSView {
    public static let rulerHeight: CGFloat = 22

    public let grid = CanvasLayerView()
    public let content = CanvasLayerView()
    public let overlay = CanvasLayerView()
    public let ruler = CanvasLayerView()
    public let playheadLayer = CALayer()

    public var axis = TimeAxis() {
        didSet {
            guard axis != oldValue else { return }
            reload()
            onAxisChange?(axis)
            movePlayhead(now: Self.hostNow())
        }
    }

    public var verticalOffset: CGFloat = 0 {
        didSet {
            guard verticalOffset != oldValue else { return }
            content.needsDisplay = true
            overlay.needsDisplay = true
            onVerticalScroll?(verticalOffset)
        }
    }

    /// Follow playback (page mode).
    public var follow = true
    /// The range a ruler drag is selecting.
    public private(set) var rulerPreview: TickRange?

    public var playheadSource: (@MainActor (UInt64) -> Double?)?
    public var onRulerRange: ((TickRange) -> Void)?
    public var onRulerClick: ((Int64) -> Void)?
    public var onToggleSnap: (() -> Void)?
    public var onAxisChange: ((TimeAxis) -> Void)?
    public var onVerticalScroll: ((CGFloat) -> Void)?

    private var link: CADisplayLink?
    private var playing = false
    private var rulerAnchor: Int64?
    private var rulerMoved = false
    private var dragPoint: CGPoint?
    private var dragModifiers: Modifiers = []
    private var scrollTimer: Timer?

    public override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        setUp()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    private func setUp() {
        wantsLayer = true
        layer?.backgroundColor = NSColor.textBackgroundColor.cgColor
        for v in [grid, content, overlay, ruler] { addSubview(v) }
        grid.paint = { [unowned self] r in self.drawGrid(r) }
        content.paint = { [unowned self] r in self.drawContent(in: r) }
        overlay.paint = { [unowned self] r in self.drawOverlayBase(r) }
        ruler.paint = { [unowned self] r in self.drawRuler(r) }
        playheadLayer.backgroundColor = NSColor.systemRed.cgColor
        playheadLayer.isHidden = true
        overlay.layer?.addSublayer(playheadLayer)
    }

    // MARK: Subclass surface

    /// The meter map of what is shown (grid, ruler, snap).
    open var meter: [MeterRow] { [] }
    open var snap: Snap { Snap() }
    /// The selection's time range, shaded in the ruler and the content.
    open var selectedRange: TickRange? { nil }
    /// Height of everything below the ruler (vertical scroll limit).
    open var contentHeight: CGFloat { content.bounds.height }
    open var accessibilityName: String { "Editor" }

    open func drawContent(in dirty: CGRect) {}
    open func drawOverlay(in dirty: CGRect) {}
    open func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {}
    open func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {}
    open func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {}
    open func contentCancel() {}
    open func accessibilityItems() -> [CanvasAccessibilityItem] { [] }

    /// Everything redraws (data changed).
    public func reload() {
        for v in [grid, content, overlay, ruler] { v.needsDisplay = true }
    }

    public static func hostNow() -> UInt64 { UInt64(max(CACurrentMediaTime(), 0) * 1e9) }

    // MARK: Layout

    open override func layout() {
        super.layout()
        let below = NSRect(
            x: 0, y: Self.rulerHeight, width: bounds.width,
            height: max(bounds.height - Self.rulerHeight, 0))
        grid.frame = below
        content.frame = below
        overlay.frame = below
        ruler.frame = NSRect(x: 0, y: 0, width: bounds.width, height: Self.rulerHeight)
        verticalOffset = clampVertical(verticalOffset)
        movePlayhead(now: Self.hostNow())
    }

    func clampVertical(_ y: CGFloat) -> CGFloat {
        min(max(y, 0), max(contentHeight - content.bounds.height, 0))
    }

    // MARK: Drawing

    private func drawGrid(_ dirty: CGRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        let lines = Grid.lines(
            meter: meter, from: axis.tick(atX: dirty.minX - 1), to: axis.tick(atX: dirty.maxX + 1),
            ticksPerPoint: axis.ticksPerPoint)
        for line in lines {
            let x = axis.x(line.tick).rounded() + 0.5
            switch line.kind {
            case .bar: ctx.setStrokeColor(NSColor.separatorColor.cgColor)
            case .beat: ctx.setStrokeColor(NSColor.separatorColor.withAlphaComponent(0.35).cgColor)
            }
            ctx.setLineWidth(1)
            ctx.move(to: CGPoint(x: x, y: dirty.minY))
            ctx.addLine(to: CGPoint(x: x, y: dirty.maxY))
            ctx.strokePath()
        }
    }

    private func shade(_ range: TickRange?, in rect: CGRect, alpha: CGFloat) {
        guard let range else { return }
        let x0 = axis.x(range.start)
        let x1 = axis.x(range.end)
        NSColor.controlAccentColor.withAlphaComponent(alpha).setFill()
        CGRect(x: x0, y: rect.minY, width: max(x1 - x0, 1), height: rect.height).fill()
    }

    private func drawOverlayBase(_ dirty: CGRect) {
        shade(rulerPreview ?? selectedRange, in: overlay.bounds, alpha: 0.10)
        drawOverlay(in: dirty)
    }

    private func drawRuler(_ dirty: CGRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext else { return }
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        shade(rulerPreview ?? selectedRange, in: ruler.bounds, alpha: 0.35)
        let first = meter.first ?? MeterRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)
        let every = Grid.labelEvery(
            barTicks: MeterMath.barTicks(first), ticksPerPoint: axis.ticksPerPoint)
        let lines = Grid.lines(
            meter: meter, from: axis.tick(atX: dirty.minX - 60), to: axis.tick(atX: dirty.maxX),
            ticksPerPoint: axis.ticksPerPoint)
        let h = ruler.bounds.height
        let font = NSFont.monospacedDigitSystemFont(ofSize: 10, weight: .regular)
        ctx.setStrokeColor(NSColor.secondaryLabelColor.cgColor)
        ctx.setLineWidth(1)
        for line in lines {
            let x = axis.x(line.tick).rounded() + 0.5
            switch line.kind {
            case .bar(let n):
                ctx.move(to: CGPoint(x: x, y: h * 0.35))
                ctx.addLine(to: CGPoint(x: x, y: h))
                ctx.strokePath()
                if MeterMath.floorDiv(n - 1, every) * every == n - 1 {
                    NSString(string: "\(n)").draw(
                        at: CGPoint(x: x + 3, y: 2),
                        withAttributes: [.font: font, .foregroundColor: NSColor.labelColor])
                }
            case .beat:
                ctx.move(to: CGPoint(x: x, y: h * 0.7))
                ctx.addLine(to: CGPoint(x: x, y: h))
                ctx.strokePath()
            }
        }
        NSColor.separatorColor.setFill()
        CGRect(x: dirty.minX, y: h - 1, width: dirty.width, height: 1).fill()
    }

    // MARK: Playhead (spec §3.4)

    /// Starts or stops the display link; also moves the playhead once.
    public func setPlaying(_ on: Bool) {
        playing = on
        if on, link == nil {
            let l = displayLink(target: self, selector: #selector(frameTick(_:)))
            l.add(to: .main, forMode: .common)
            link = l
        } else if !on {
            link?.invalidate()
            link = nil
        }
        movePlayhead(now: Self.hostNow())
    }

    @objc private func frameTick(_ l: CADisplayLink) {
        movePlayhead(now: UInt64(max(l.timestamp, 0) * 1e9))
    }

    func movePlayhead(now: UInt64) {
        guard let tick = playheadSource?(now) else {
            playheadLayer.isHidden = true
            return
        }
        if playing, follow,
            let origin = PageFollow.origin(forPlayhead: tick, axis: axis, width: content.bounds.width)
        {
            axis = axis.scrolled(toTick: origin)
        }
        let x = CGFloat((tick - axis.originTick) / axis.ticksPerPoint).rounded()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        playheadLayer.isHidden = x < -1 || x > overlay.bounds.width + 1
        playheadLayer.frame = CGRect(x: x, y: 0, width: 1, height: overlay.bounds.height)
        CATransaction.commit()
    }

    // MARK: Events

    open override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        let p = convert(e.locationInWindow, from: nil)
        dragModifiers = Modifiers(event: e)
        if p.y < Self.rulerHeight {
            rulerAnchor = axis.tick(atX: p.x)
            rulerMoved = false
            return
        }
        contentMouseDown(
            content.convert(e.locationInWindow, from: nil), clicks: e.clickCount,
            modifiers: dragModifiers)
    }

    open override func mouseDragged(with e: NSEvent) {
        dragPoint = convert(e.locationInWindow, from: nil)
        dragModifiers = Modifiers(event: e)
        startAutoScroll()
        dragStep()
    }

    open override func mouseUp(with e: NSEvent) {
        stopAutoScroll()
        dragPoint = nil
        if let anchor = rulerAnchor {
            rulerAnchor = nil
            let range = rulerPreview
            rulerPreview = nil
            reload()
            if rulerMoved, let range {
                onRulerRange?(range)
            } else if !rulerMoved {
                onRulerClick?(snap.apply(anchor, meter: meter))
            }
            return
        }
        contentMouseUp(content.convert(e.locationInWindow, from: nil), modifiers: Modifiers(event: e))
    }

    /// One drag step at the last pointer position (also run by auto-scroll,
    /// so content under a still pointer follows the scrolled axis).
    private func dragStep() {
        guard let p = dragPoint else { return }
        if let anchor = rulerAnchor {
            rulerMoved = true
            rulerPreview = RulerSelection.range(
                fromTick: anchor, toX: p.x, axis: axis, snap: snap, meter: meter)
            ruler.needsDisplay = true
            overlay.needsDisplay = true
            return
        }
        contentMouseDragged(convert(p, to: content), modifiers: dragModifiers)
    }

    private func startAutoScroll() {
        guard scrollTimer == nil else { return }
        scrollTimer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.autoScrollStep() }
        }
    }

    private func stopAutoScroll() {
        scrollTimer?.invalidate()
        scrollTimer = nil
    }

    private func autoScrollStep() {
        guard let p = dragPoint else { return }
        let v = AutoScroll.speed(x: p.x, width: bounds.width)
        guard v != 0 else { return }
        axis = axis.scrolled(byPoints: v / 60)
        dragStep()
    }

    open override func keyDown(with e: NSEvent) {
        let plain = e.modifierFlags.intersection(.deviceIndependentFlagsMask)
            .subtracting([.capsLock, .numericPad, .function]).isEmpty
        if e.keyCode == 53 {  // Escape
            cancelGestures()
        } else if plain, e.charactersIgnoringModifiers?.lowercased() == "g" {
            onToggleSnap?()
        } else {
            super.keyDown(with: e)
        }
    }

    open override func cancelOperation(_ sender: Any?) { cancelGestures() }

    private func cancelGestures() {
        stopAutoScroll()
        if rulerAnchor != nil {
            rulerAnchor = nil
            rulerPreview = nil
            reload()
        }
        contentCancel()
    }

    open override func scrollWheel(with e: NSEvent) {
        let x = convert(e.locationInWindow, from: nil).x
        if e.modifierFlags.contains(.command) {
            axis = axis.zoomed(by: pow(1.01, Double(e.scrollingDeltaY)), aroundX: x)
            return
        }
        axis = axis.scrolled(byPoints: -e.scrollingDeltaX)
        verticalOffset = clampVertical(verticalOffset - e.scrollingDeltaY)
    }

    open override func magnify(with e: NSEvent) {
        axis = axis.zoomed(by: 1 + Double(e.magnification), aroundX: convert(e.locationInWindow, from: nil).x)
    }

    // MARK: Accessibility (spec §3.5)

    open override func isAccessibilityElement() -> Bool { true }
    open override func accessibilityRole() -> NSAccessibility.Role? { .group }
    open override func accessibilityLabel() -> String? { accessibilityName }

    open override func accessibilityChildren() -> [Any]? {
        accessibilityItems().map { item in
            CanvasElement(item, parent: self, frame: content.convert(item.frame, to: self))
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 5: Run the tests**

Run: `scripts/swift.sh build && scripts/swift.sh test`
Expected: PASS — 5 + 3 + 3 new tests (159 Swift tests). `thePlayheadFollowsTheSourceWithoutAnimation`: tick 10 000 at 100 ticks per point is x = 100; `setPlaying(false)` moves the playhead once without a display link.

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Sources/VizDAWEditors/ToolMachine.swift app/Sources/VizDAWEditors/Scrolling.swift \
  app/Sources/VizDAWEditors/EditorCanvas.swift app/Tests/VizDAWEditorsTests/ToolMachineTests.swift \
  app/Tests/VizDAWEditorsTests/ScrollingTests.swift app/Tests/VizDAWEditorsTests/EditorCanvasTests.swift
git commit -m "feat(editors): tool state machine, ruler selection, auto-scroll and the editor canvas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 159 Swift tests.

---

### Task 12: The arrangement — layout, hit-testing, drags, lane headers, light mode

**Files:**
- Create: `app/Sources/VizDAWEditors/ArrangementLayout.swift`, `app/Sources/VizDAWEditors/ArrangementView.swift`, `app/Sources/VizDAWEditors/LaneHeaderView.swift`, `app/Sources/VizDAWEditors/ArrangementContainer.swift`, `app/Sources/VizDAW/ArrangementPane.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Sources/VizDAW/DocumentView.swift`
- Test: `app/Tests/VizDAWEditorsTests/ArrangementLayoutTests.swift`, `app/Tests/VizDAWAppModelTests/NavigationTests.swift` (new)

**Interfaces:**
- Consumes: Tasks 7–11 (`TimeAxis`, `Snap`, `MeterMath`, `ToolMachine`, `EditorCanvas`, `EditorModel` reads, selection, gestures); `DocumentModel.lanes`, `selectedSeq`, `transport`, `clock`; `SessionProtocol.destinations()` (3c `DestRow { id, name, plugin }`).
- Produces:
  - Pure (`VizDAWEditors`): `ArrangementGeometry { laneHeight, verticalOffset; rowTop(_:), row(atY:count:), blockRect(_:row:axis:), contentHeight(rows:); static blockInset (2), minBlockWidth (4) }`; `BlockHit { lane: LaneKey, index: Int, block: BlockRow }`; `ArrangementHit { block(BlockHit), empty(lane: LaneKey?, tick: Int64) }`; `ArrangementHitTest.hit(_:lanes:blocks:geometry:axis:) -> ArrangementHit`; `LightDrawing.{threshold (1 500), isLight(_:visibleBlocks:)}`; `BlockDrag.{delta(rawTicks:blockStart:snap:meter:), events(pressed:laneBlocks:selected:), command(events:rows:from:to:delta:copy:) -> EditCommand?, label(copy:)}`; `ArrangementA11y.{label(_:laneName:meter:), header(_:)}`; `HeaderControl { name, mute, solo, arm, destination }`; `LaneHeaderLayout.{width (200), controls(rowTop:), control(at:rowTop:)}`.
  - Views: `ArrangementView(document:)` (`EditorCanvas`), `LaneHeaderView(document:)`, `ArrangementContainer(document:)`, SwiftUI `ArrangementPane(model:)` with `ArrangementBar` and `StatusLine`.
  - Model: `EditorModel.openDetail(_:kind:)`, `openReference(_:)`, `back()`, `selectSequence`/`currentSequence` hooks; `DocumentModel.locate(_:)`; `DocumentModel.playKey: PlayKey { playing, seq }` (changes only when they change).

Rulings 14, 17, 23, 24. The ⌥ state is read when the drag begins (the transaction's label, "Move Block" or "Copy Block", is fixed then). The playhead shows only while the transport plays (or sits on) the sequence the arrangement shows.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/ArrangementLayoutTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let Q: Int64 = 15_360
private let BAR: Int64 = 4 * Q
private let fourFour = [MeterRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

private func block(_ s: Int64, _ e: Int64, _ ids: [UInt64], ref: UInt64? = nil, muted: Bool = false) -> BlockRow {
    BlockRow(
        start: s, end: e, kind: ref == nil ? .events : .reference, events: ids,
        label: ref == nil ? nil : "Groove", refSeq: ref, muted: muted)
}

private func row(_ id: UInt64, tick: Int64) -> EventRow {
    EventRow(
        id: id, tick: tick, kind: .note, mute: false, dest: nil, groupChannel: nil, pitch: 60,
        velocity: 0x8000, releaseVelocity: 0, duration: Q, value: 0, value2: 0, attribute: nil,
        perNote: [], refSeq: nil, refLength: nil, refLoops: 0, refTranspose: 0, media: nil,
        sourceOffset: 0, frames: 0, gainDb: 0, fadeIn: FadeRow(length: 0, shape: .linear),
        fadeOut: FadeRow(length: 0, shape: .linear), text: nil, textKind: nil, data: [],
        eightBit: false)
}

private let a = LaneKey(3, 30)
private let b = LaneKey(3, 31)

@Suite struct ArrangementLayoutTests {
    let axis = TimeAxis(ticksPerPoint: 384, originTick: 0)

    @Test func blocksSitInsideTheirRows() {
        let g = ArrangementGeometry()
        #expect(g.blockRect(block(0, BAR, [1]), row: 1, axis: axis) == CGRect(x: 0, y: 50, width: 160, height: 44))
        #expect(g.blockRect(block(BAR, BAR, [1]), row: 0, axis: axis).width == 4, "zero length stays clickable")
        #expect(g.row(atY: 47, count: 2) == 0)
        #expect(g.row(atY: 48, count: 2) == 1)
        #expect(g.row(atY: 96, count: 2) == nil)
        var scrolled = g
        scrolled.verticalOffset = 48
        #expect(scrolled.row(atY: 0, count: 2) == 1)
        #expect(g.contentHeight(rows: 2) == 144, "one spare row for adding lanes")
    }

    @Test func hitTestingPrefersTheTopmostBlock() {
        let blocks = [a: [block(0, 2 * BAR, [1, 2]), block(BAR, 2 * BAR, [9], ref: 2)]]
        let hit = ArrangementHitTest.hit(
            CGPoint(x: 200, y: 10), lanes: [a, b], blocks: blocks, geometry: ArrangementGeometry(), axis: axis)
        #expect(hit == .block(BlockHit(lane: a, index: 1, block: blocks[a]![1])))
        let empty = ArrangementHitTest.hit(
            CGPoint(x: 10, y: 60), lanes: [a, b], blocks: blocks, geometry: ArrangementGeometry(), axis: axis)
        #expect(empty == .empty(lane: b, tick: 3_840))
        let below = ArrangementHitTest.hit(
            CGPoint(x: 10, y: 500), lanes: [a, b], blocks: blocks, geometry: ArrangementGeometry(), axis: axis)
        #expect(below == .empty(lane: nil, tick: 3_840))
    }

    @Test func dragDeltaSnapsTheBlockStart() {
        #expect(BlockDrag.delta(rawTicks: 15_000, blockStart: 1_000, snap: Snap(), meter: fourFour) == 14_360)
        var off = Snap()
        off.enabled = false
        #expect(BlockDrag.delta(rawTicks: 15_000, blockStart: 1_000, snap: off, meter: fourFour) == 15_000)
    }

    @Test func movesStayInTheirLaneOrMergeIntoAnother() {
        #expect(BlockDrag.command(events: [1], rows: [], from: a, to: a, delta: 0, copy: false) == nil)
        #expect(
            BlockDrag.command(events: [1], rows: [], from: a, to: a, delta: Q, copy: false)
                == .event(edit: .move(events: [1], delta: Q, toLane: nil)))
        #expect(
            BlockDrag.command(events: [1], rows: [], from: a, to: b, delta: 0, copy: false)
                == .event(edit: .move(events: [1], delta: 0, toLane: LaneRef(seq: 3, lane: 31))))
    }

    @Test func optionCopiesShiftedRowsIntoTheTargetLane() {
        let rows = [row(1, tick: 0), row(2, tick: Q), row(3, tick: 5 * BAR)]
        let cmd = BlockDrag.command(events: [1, 2], rows: rows, from: a, to: b, delta: BAR, copy: true)
        guard case .event(.insert(let seq, let lane, let events))? = cmd else {
            Issue.record("expected an insert, got \(String(describing: cmd))")
            return
        }
        #expect((seq, lane) == (3, 31))
        #expect(events.map(\.tick) == [BAR, BAR + Q])
        #expect(BlockDrag.label(copy: true) == "Copy Block")
        #expect(BlockDrag.label(copy: false) == "Move Block")
    }

    @Test func draggingASelectedBlockCarriesTheOtherSelectedBlocksOfItsLane() {
        let lane = [block(0, BAR, [1, 2]), block(2 * BAR, 3 * BAR, [3]), block(4 * BAR, 5 * BAR, [4])]
        #expect(BlockDrag.events(pressed: lane[0], laneBlocks: lane, selected: [1, 2, 4]) == [1, 2, 4])
        #expect(BlockDrag.events(pressed: lane[1], laneBlocks: lane, selected: [1, 2, 4]) == [3])
    }

    @Test func lightModeFollowsTheThreshold() {
        #expect(!LightDrawing.isLight(.auto, visibleBlocks: 1_500))
        #expect(LightDrawing.isLight(.auto, visibleBlocks: 1_501))
        #expect(LightDrawing.isLight(.on, visibleBlocks: 1))
        #expect(!LightDrawing.isLight(.off, visibleBlocks: 100_000))
    }

    @Test func accessibilityLabelsNameKindLaneAndBars() {
        #expect(
            ArrangementA11y.label(block(0, 2 * BAR, [1, 2]), laneName: "Chords", meter: fourFour)
                == "Block on Chords, bars 1 to 2, 2 events")
        #expect(
            ArrangementA11y.label(block(BAR, 2 * BAR, [9], ref: 2, muted: true), laneName: "Groove", meter: fourFour)
                == "Reference to Groove on Groove, bar 2, muted")
    }

    @Test func headerControlsAreFoundByPosition() {
        let controls = LaneHeaderLayout.controls(rowTop: 48)
        let mute = controls.first { $0.0 == .mute }!.1
        #expect(LaneHeaderLayout.control(at: CGPoint(x: mute.midX, y: mute.midY), rowTop: 48) == .mute)
        #expect(LaneHeaderLayout.control(at: CGPoint(x: 100, y: 48 + 30), rowTop: 48) == .destination)
        #expect(LaneHeaderLayout.control(at: CGPoint(x: 199, y: 48 + 46), rowTop: 48) == nil)
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWAppModelTests/NavigationTests.swift <<'VIZDAW_EOF'
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct NavigationTests {
    @Test func aBlockOpensInTheLastUsedDetailEditor() throws {
        let m = try DocumentModel(session: editorFake())
        m.editor.openDetail(.block(seq: 3, lane: 31, start: 0, end: 10))
        #expect(m.editor.layout.detail == DetailState(kind: .pianoRoll, target: .block(seq: 3, lane: 31, start: 0, end: 10)))
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .list)
        m.editor.openDetail(.lane(seq: 3, lane: 30))
        #expect(m.editor.layout.detail?.kind == .list, "the last used editor")
    }

    @Test func openingAReferenceShowsItsSequenceAndBackReturns() throws {
        let m = try DocumentModel(session: editorFake())
        #expect(m.selectedSeq == 3)
        m.editor.openReference(blockRow(0, 10, events: [7], ref: 2, label: "Groove"))
        #expect(m.selectedSeq == 2)
        #expect(m.editor.navigation == [3])
        m.editor.back()
        #expect(m.selectedSeq == 3)
        #expect(m.editor.navigation.isEmpty)
    }

    @Test func choosingFromTheSidebarForgetsTheTrail() throws {
        let m = try DocumentModel(session: editorFake())
        m.editor.openReference(blockRow(0, 10, events: [7], ref: 2, label: "Groove"))
        m.choose(1)
        #expect(m.editor.navigation.isEmpty)
        #expect(m.selectedSeq == 1)
    }

    @Test func playKeyChangesOnlyWithPlayStateOrSequence() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        #expect(m.playKey == PlayKey(playing: false, seq: nil))
        m.playStop()
        #expect(m.playKey == PlayKey(playing: true, seq: 3))
        f.state.tick = 99
        m.poll()
        #expect(m.playKey == PlayKey(playing: true, seq: 3))
    }

    @Test func aRulerClickLocates() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.locate(61_440)
        #expect(f.state.tick == 61_440)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/swift.sh test`
Expected: compile errors — cannot find `ArrangementGeometry`, `BlockDrag`, no member `openDetail`, `locate`.

- [ ] **Step 3: Pure arrangement types**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/ArrangementLayout.swift <<'VIZDAW_EOF'
import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// Rows and block rectangles of the arrangement, in `content` coordinates
/// (y = 0 is the top of the first lane before scrolling).
public struct ArrangementGeometry: Equatable, Sendable {
    public static let blockInset: CGFloat = 2
    public static let minBlockWidth: CGFloat = 4

    public var laneHeight: CGFloat = 48
    public var verticalOffset: CGFloat = 0

    public init() {}

    public func rowTop(_ i: Int) -> CGFloat { CGFloat(i) * laneHeight - verticalOffset }

    public func row(atY y: CGFloat, count: Int) -> Int? {
        guard laneHeight > 0 else { return nil }
        let i = Int(((y + verticalOffset) / laneHeight).rounded(.down))
        return (0..<count).contains(i) ? i : nil
    }

    public func blockRect(_ b: BlockRow, row: Int, axis: TimeAxis) -> CGRect {
        let x0 = axis.x(b.start)
        let x1 = axis.x(b.end)
        return CGRect(
            x: x0, y: rowTop(row) + Self.blockInset, width: max(x1 - x0, Self.minBlockWidth),
            height: laneHeight - 2 * Self.blockInset)
    }

    /// The lanes plus one spare row (the "Add Lane" row of the headers).
    public func contentHeight(rows: Int) -> CGFloat { CGFloat(rows + 1) * laneHeight }
}

public struct BlockHit: Equatable, Sendable {
    public let lane: LaneKey
    public let index: Int
    public let block: BlockRow

    public init(lane: LaneKey, index: Int, block: BlockRow) {
        self.lane = lane
        self.index = index
        self.block = block
    }
}

public enum ArrangementHit: Equatable, Sendable {
    case block(BlockHit)
    case empty(lane: LaneKey?, tick: Int64)
}

public enum ArrangementHitTest {
    /// What `p` lands on; among overlapping blocks the last drawn wins.
    public static func hit(
        _ p: CGPoint, lanes: [LaneKey], blocks: [LaneKey: [BlockRow]],
        geometry: ArrangementGeometry, axis: TimeAxis
    ) -> ArrangementHit {
        let tick = axis.tick(atX: p.x)
        guard let row = geometry.row(atY: p.y, count: lanes.count) else {
            return .empty(lane: nil, tick: tick)
        }
        let key = lanes[row]
        let list = blocks[key] ?? []
        for (i, b) in list.enumerated().reversed()
        where geometry.blockRect(b, row: row, axis: axis).contains(p) {
            return .block(BlockHit(lane: key, index: i, block: b))
        }
        return .empty(lane: key, tick: tick)
    }
}

/// Light drawing mode (Plan Ruling 17).
public enum LightDrawing {
    public static let threshold = 1_500

    public static func isLight(_ mode: LightMode, visibleBlocks: Int) -> Bool {
        switch mode {
        case .on: true
        case .off: false
        case .auto: visibleBlocks > threshold
        }
    }
}

/// Moving, copying and merging blocks (parent spec §9, Plan Ruling 23).
public enum BlockDrag {
    /// The dragged distance, snapped so the pressed block's start lands
    /// on the grid.
    public static func delta(rawTicks: Int64, blockStart: Int64, snap: Snap, meter: [MeterRow]) -> Int64 {
        guard snap.enabled else { return rawTicks }
        return snap.apply(blockStart + rawTicks, meter: meter) - blockStart
    }

    /// The events a drag carries: the pressed block's, plus those of every
    /// other fully selected block of the lane when the pressed one is
    /// selected.
    public static func events(pressed: BlockRow, laneBlocks: [BlockRow], selected: Set<UInt64>) -> [UInt64] {
        let isSelected = { (b: BlockRow) in !b.events.isEmpty && b.events.allSatisfy(selected.contains) }
        guard isSelected(pressed) else { return pressed.events }
        return laneBlocks.filter(isSelected).flatMap(\.events)
    }

    /// The full target of a drag of `events` from lane `from` by `delta`
    /// into lane `to`: a move (merging into `to`), or with `copy` an insert
    /// of the shifted `rows`. Nil when nothing would change.
    public static func command(
        events: [UInt64], rows: [EventRow], from: LaneKey, to: LaneKey, delta: Int64, copy: Bool
    ) -> EditCommand? {
        if delta == 0 && from == to { return nil }
        if copy {
            let ids = Set(events)
            let copies = rows.filter { ids.contains($0.id) }.map { r -> EventRow in
                var c = r
                c.tick += delta
                return c
            }
            guard !copies.isEmpty else { return nil }
            return .event(edit: .insert(seq: to.seq, lane: to.lane, events: copies))
        }
        return .event(edit: .move(events: events, delta: delta, toLane: from == to ? nil : to.ref))
    }

    public static func label(copy: Bool) -> String { copy ? "Copy Block" : "Move Block" }
}

/// VoiceOver labels (spec §3.5).
public enum ArrangementA11y {
    public static func label(_ b: BlockRow, laneName: String, meter: [MeterRow]) -> String {
        let first = MeterMath.bar(at: b.start, meter).number
        let last = MeterMath.bar(at: max(b.end - 1, b.start), meter).number
        let bars = first == last ? "bar \(first)" : "bars \(first) to \(last)"
        let muted = b.muted ? ", muted" : ""
        switch b.kind {
        case .reference:
            return "Reference to \(b.label ?? "a sequence") on \(laneName), \(bars)\(muted)"
        case .events:
            let n = b.events.count
            return "Block on \(laneName), \(bars), \(n) event\(n == 1 ? "" : "s")\(muted)"
        }
    }

    public static func header(_ lane: LaneRow) -> String {
        var parts = [lane.name, lane.destination ?? "no destination"]
        if lane.mute { parts.append("muted") }
        if lane.solo { parts.append("soloed") }
        if lane.arm { parts.append("armed") }
        return parts.joined(separator: ", ")
    }
}

/// The clickable parts of a lane header (Plan Ruling 24).
public enum HeaderControl: Equatable, Sendable {
    case name, mute, solo, arm, destination
}

public enum LaneHeaderLayout {
    public static let width: CGFloat = 200

    public static func controls(rowTop y: CGFloat) -> [(HeaderControl, CGRect)] {
        [
            (.name, CGRect(x: 8, y: y + 4, width: 122, height: 18)),
            (.mute, CGRect(x: 134, y: y + 4, width: 18, height: 18)),
            (.solo, CGRect(x: 155, y: y + 4, width: 18, height: 18)),
            (.arm, CGRect(x: 176, y: y + 4, width: 18, height: 18)),
            (.destination, CGRect(x: 8, y: y + 25, width: 186, height: 17)),
        ]
    }

    public static func control(at p: CGPoint, rowTop: CGFloat) -> HeaderControl? {
        controls(rowTop: rowTop).first { $0.1.contains(p) }?.0
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Navigation in the model**

In `app/Sources/VizDAWAppModel/EditorModel.swift`:

Find:
```swift
    public func push(_ parent: UInt64) { navigation.append(parent) }
```
Replace:
```swift
    /// Selects a sequence in the document / reads the shown one (set by
    /// `DocumentModel`).
    @ObservationIgnored var selectSequence: (UInt64) -> Void = { _ in }
    @ObservationIgnored var currentSequence: () -> UInt64? = { nil }

    /// Shows `target` in the detail pane, in `kind` or the last-used
    /// editor (spec §3.7).
    public func openDetail(_ target: EditorTarget, kind: DetailEditorKind? = nil) {
        updateLayout { l in
            let k = kind ?? l.lastKind
            l.lastKind = k
            l.detail = DetailState(kind: k, target: target)
        }
    }

    /// Opens the sequence a reference block plays (Plan Ruling 14).
    public func openReference(_ b: BlockRow) {
        guard let target = b.refSeq, let current = currentSequence() else { return }
        push(current)
        selectSequence(target)
    }

    /// Back to the sequence the last opened reference sat in.
    public func back() {
        guard let parent = pop() else { return }
        selectSequence(parent)
    }

    public func push(_ parent: UInt64) { navigation.append(parent) }
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
        editor.applySnapshot = { [weak self] in self?.apply($0) }
    }
```
Replace:
```swift
        editor.applySnapshot = { [weak self] in self?.apply($0) }
        editor.selectSequence = { [weak self] in self?.select($0) }
        editor.currentSequence = { [weak self] in self?.selectedSeq }
    }
```

Find:
```swift
    public private(set) var transport: TransportState
```
Replace:
```swift
    public private(set) var transport: TransportState
    /// Playing, and which sequence; set only when they change, so views
    /// that must not redraw on every poll observe this, not `transport`.
    public private(set) var playKey = PlayKey(playing: false, seq: nil)
```

Find:
```swift
            transport = try session.transport()
            clock.update(transport, pollInterval: pollSeconds)
```
Replace:
```swift
            transport = try session.transport()
            clock.update(transport, pollInterval: pollSeconds)
            updatePlayKey()
```

Find:
```swift
        clock.update(transport, pollInterval: transport.playing ? 0.033 : 0.25)
        apply(snapshot)
```
Replace:
```swift
        clock.update(transport, pollInterval: transport.playing ? 0.033 : 0.25)
        playKey = PlayKey(playing: transport.playing, seq: transport.seq)
        apply(snapshot)
```

Append to `DocumentModel.swift` (outside the class):

```swift
/// Whether the transport plays, and which sequence.
public struct PlayKey: Equatable, Sendable {
    public let playing: Bool
    public let seq: UInt64?
}
```

Find:
```swift
    public func returnToStart() {
```
Replace:
```swift
    private func updatePlayKey() {
        let k = PlayKey(playing: transport.playing, seq: transport.seq)
        if k != playKey { playKey = k }
    }

    /// Moves the transport to `tick` (a click in a ruler).
    public func locate(_ tick: Int64) {
        do {
            try session.locate(tick: tick)
        } catch {
            alert = AlertItem(error, title: "Locate Failed")
        }
        poll()
    }

    public func returnToStart() {
```

- [ ] **Step 5: Run the pure and model tests**

Run: `scripts/swift.sh test`
Expected: PASS — 9 + 5 new tests (173 Swift tests).

- [ ] **Step 6: The arrangement views**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/ArrangementView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The arrangement timeline (spec §4a, parent §9): the shown sequence's
/// lanes as rows, their blocks, reference blocks labelled with their
/// sequence, block drags (move, ⌥ copy, merge into another lane),
/// double-click to open, light drawing for big projects.
@MainActor
public final class ArrangementView: EditorCanvas {
    public let document: DocumentModel
    var editor: EditorModel { document.editor }

    private var machine = ToolMachine<ArrangementHit>()
    private(set) var geometry = ArrangementGeometry()
    /// The last drawing was in light mode.
    public private(set) var light = false

    private struct Drag {
        let hit: BlockHit
        let events: [UInt64]
        let rows: [EventRow]
        let copy: Bool
        let startTick: Int64
    }

    private var drag: Drag?

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        axis = TimeAxis(ticksPerPoint: document.editor.layout.ticksPerPoint)
        playheadSource = { [unowned self] now in
            let t = self.document.transport
            guard let seq = self.seq, t.seq == seq else { return nil }
            return self.document.clock.tick(atHostTimeNs: now)
        }
        onRulerRange = { [unowned self] r in self.editor.selectRange(r, lanes: self.lanes) }
        onRulerClick = { [unowned self] t in self.document.locate(t) }
        onToggleSnap = { [unowned self] in self.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [unowned self] a in
            if a.ticksPerPoint != self.editor.layout.ticksPerPoint {
                self.editor.updateLayout { $0.ticksPerPoint = a.ticksPerPoint }
            }
        }
        refresh()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    var seq: UInt64? { document.selectedSeq }

    var lanes: [LaneKey] {
        guard let s = seq else { return [] }
        return document.lanes.map { LaneKey(s, $0.id) }
    }

    public override var meter: [MeterRow] { seq.flatMap { editor.info($0)?.meter } ?? [] }
    public override var snap: Snap { editor.layout.snap }
    public override var selectedRange: TickRange? { editor.selection.timeRange }
    public override var contentHeight: CGFloat { geometry.contentHeight(rows: lanes.count) }
    public override var accessibilityName: String { "Arrangement" }

    /// Redraws whenever drawn document or editor state changes.
    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = editor.selection
            _ = document.lanes
            _ = document.selectedSeq
            // Not `transport`: it changes on every poll; the playhead moves
            // without redrawing.
            _ = document.playKey
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.refresh()
                self?.observe()
            }
        }
    }

    func refresh() {
        geometry.laneHeight = CGFloat(editor.layout.laneHeight)
        if editor.layout.ticksPerPoint != axis.ticksPerPoint {
            axis = TimeAxis(ticksPerPoint: editor.layout.ticksPerPoint, originTick: axis.originTick)
        }
        follow = editor.layout.follow
        setPlaying(document.playKey.playing && document.playKey.seq == seq)
        reload()
    }

    public override var verticalOffset: CGFloat {
        didSet { geometry.verticalOffset = verticalOffset }
    }

    // MARK: Drawing

    public override func drawContent(in dirty: CGRect) {
        let keys = lanes
        var rows: [(Int, LaneKey, [BlockRow])] = []
        var visible = 0
        for (i, key) in keys.enumerated() {
            let top = geometry.rowTop(i)
            guard top + geometry.laneHeight >= dirty.minY, top <= dirty.maxY else { continue }
            let shown = editor.blocks(key).filter {
                geometry.blockRect($0, row: i, axis: axis).intersects(dirty)
            }
            visible += shown.count
            rows.append((i, key, shown))
        }
        light = LightDrawing.isLight(editor.layout.lightMode, visibleBlocks: visible)
        for (i, key, blocks) in rows {
            NSColor.separatorColor.setFill()
            CGRect(x: dirty.minX, y: geometry.rowTop(i + 1) - 0.5, width: dirty.width, height: 1).fill()
            for b in blocks { draw(b, row: i, key: key) }
        }
    }

    private func draw(_ b: BlockRow, row: Int, key: LaneKey) {
        let r = geometry.blockRect(b, row: row, axis: axis)
        let selected = editor.isSelected(b)
        let color: NSColor = b.kind == .reference ? .systemPurple : .systemBlue
        let shape = NSBezierPath(roundedRect: r.insetBy(dx: 0.5, dy: 0.5), xRadius: 3, yRadius: 3)
        if !light {
            color.withAlphaComponent(b.muted ? 0.12 : 0.30).setFill()
            shape.fill()
        }
        (selected ? NSColor.controlAccentColor : color).setStroke()
        shape.lineWidth = selected ? 2 : 1
        if b.muted { shape.setLineDash([3, 2], count: 2, phase: 0) }
        shape.stroke()
        guard !light, r.width > 24 else { return }
        if b.kind == .reference {
            let text = NSString(string: "↻ " + (b.label ?? ""))
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(rect: r.insetBy(dx: 3, dy: 1)).addClip()
            text.draw(
                at: CGPoint(x: r.minX + 4, y: r.minY + 3),
                withAttributes: [.font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.labelColor])
            NSGraphicsContext.restoreGraphicsState()
        } else {
            sketch(b, in: r, key: key)
        }
    }

    /// Mini notes inside an events block (normal drawing only).
    private func sketch(_ b: BlockRow, in r: CGRect, key: LaneKey) {
        guard let rows = editor.laneEvents(key)?.events else { return }
        var lo = 0
        var hi = rows.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if rows[mid].tick < b.start { lo = mid + 1 } else { hi = mid }
        }
        let notes = rows[lo...].prefix(while: { $0.tick <= b.end }).filter { $0.kind == .note }
        guard let minP = notes.map(\.pitch).min(), let maxP = notes.map(\.pitch).max() else { return }
        let span = CGFloat(max(Int(maxP) - Int(minP), 1))
        NSColor.labelColor.withAlphaComponent(0.55).setFill()
        for n in notes {
            let y = r.maxY - 5 - CGFloat(Int(n.pitch) - Int(minP)) / span * (r.height - 10)
            let x = axis.x(n.tick)
            CGRect(x: x, y: y - 1, width: max(axis.width(ofTicks: n.duration), 1), height: 2)
                .intersection(r).fill()
        }
    }

    // MARK: Gestures

    private func hit(_ p: CGPoint) -> ArrangementHit {
        let keys = lanes
        var map: [LaneKey: [BlockRow]] = [:]
        if let row = geometry.row(atY: p.y, count: keys.count) { map[keys[row]] = editor.blocks(keys[row]) }
        return ArrangementHitTest.hit(p, lanes: keys, blocks: map, geometry: geometry, axis: axis)
    }

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        handle(machine.down(hit(p), at: p, clicks: clicks, modifiers: modifiers))
    }

    public override func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.drag(to: p, modifiers: modifiers))
    }

    public override func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.up(at: p, modifiers: modifiers))
    }

    public override func contentCancel() {
        handle(machine.cancel())
    }

    private func handle(_ effects: [ToolMachine<ArrangementHit>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(hit, _, clicks, modifiers):
                click(hit, clicks: clicks, modifiers: modifiers)
            case let .begin(.block(b), from, modifiers):
                beginDrag(b, from: from, copy: modifiers.contains(.option))
            case let .move(_, _, to, _):
                if let d = drag { editor.updateGesture(target(d, to: to)) }
            case let .end(_, _, to, _):
                if let d = drag {
                    drag = nil
                    editor.endGesture(target(d, to: to))
                }
            case .cancel:
                if drag != nil {
                    drag = nil
                    editor.cancelGesture()
                }
            case .begin(.empty, _, _):
                break
            }
        }
    }

    private func click(_ hit: ArrangementHit, clicks: Int, modifiers: Modifiers) {
        switch hit {
        case .block(let b) where editor.tool == .eraser:
            editor.edit(.event(edit: .delete(events: b.block.events)))
        case .block(let b) where clicks >= 2:
            if b.block.kind == .reference {
                editor.openReference(b.block)
            } else {
                editor.openDetail(.block(seq: b.lane.seq, lane: b.lane.lane, start: b.block.start, end: b.block.end))
            }
        case .block(let b):
            editor.selectBlock(b.block, in: b.lane, adding: modifiers.contains(.shift))
        case .empty:
            if !modifiers.contains(.shift) { editor.clearSelection() }
        }
    }

    private func beginDrag(_ b: BlockHit, from: CGPoint, copy: Bool) {
        guard editor.tool == .arrow else { return }
        let selected = Set(editor.selection.events)
        let events = BlockDrag.events(pressed: b.block, laneBlocks: editor.blocks(b.lane), selected: selected)
        let rows = copy ? (editor.laneEvents(b.lane)?.events ?? []) : []
        guard editor.beginGesture(BlockDrag.label(copy: copy), lanes: [b.lane]) else { return }
        drag = Drag(hit: b, events: events, rows: rows, copy: copy, startTick: axis.tick(atX: from.x))
    }

    private func target(_ d: Drag, to: CGPoint) -> EditCommand? {
        let raw = axis.tick(atX: to.x) - d.startTick
        let delta = BlockDrag.delta(rawTicks: raw, blockStart: d.hit.block.start, snap: snap, meter: meter)
        let keys = lanes
        let toLane = geometry.row(atY: to.y, count: keys.count).map { keys[$0] } ?? d.hit.lane
        return BlockDrag.command(
            events: d.events, rows: d.rows, from: d.hit.lane, to: toLane, delta: delta, copy: d.copy)
    }

    // MARK: Accessibility

    public override func accessibilityItems() -> [CanvasAccessibilityItem] {
        var out: [CanvasAccessibilityItem] = []
        let rows = document.lanes
        for (i, key) in lanes.enumerated() where i < rows.count {
            for b in editor.blocks(key) {
                let r = geometry.blockRect(b, row: i, axis: axis)
                guard r.intersects(content.bounds) else { continue }
                out.append(
                    CanvasAccessibilityItem(
                        label: ArrangementA11y.label(b, laneName: rows[i].name, meter: meter), frame: r,
                        selected: editor.isSelected(b),
                        press: { [unowned self] in self.editor.selectBlock(b, in: key, adding: false) }))
            }
        }
        return out
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/LaneHeaderView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// Lane headers beside the arrangement (Plan Ruling 24): name (double-click
/// renames), mute/solo/arm, destination menu, and an "Add Lane" row.
@MainActor
public final class LaneHeaderView: NSView, NSTextFieldDelegate {
    public let document: DocumentModel
    var editor: EditorModel { document.editor }
    public var verticalOffset: CGFloat = 0 { didSet { needsDisplay = true } }
    private var renaming: (lane: UInt64, field: NSTextField)?

    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        wantsLayer = true
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    var laneHeight: CGFloat { CGFloat(editor.layout.laneHeight) }

    private func observe() {
        withObservationTracking {
            _ = document.lanes
            _ = editor.layout
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.observe()
            }
        }
    }

    /// Top of row `i` in this view (below a ruler-high spacer).
    func rowTop(_ i: Int) -> CGFloat {
        EditorCanvas.rulerHeight + CGFloat(i) * laneHeight - verticalOffset
    }

    public override func draw(_ dirty: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        let small = NSFont.systemFont(ofSize: 11)
        for (i, lane) in document.lanes.enumerated() {
            let top = rowTop(i)
            guard top + laneHeight >= dirty.minY, top <= dirty.maxY else { continue }
            for (control, r) in LaneHeaderLayout.controls(rowTop: top) {
                switch control {
                case .name:
                    NSString(string: lane.name).draw(
                        in: r, withAttributes: [.font: NSFont.systemFont(ofSize: 12, weight: .medium)])
                case .mute: toggle("M", on: lane.mute, color: .systemYellow, in: r)
                case .solo: toggle("S", on: lane.solo, color: .systemGreen, in: r)
                case .arm: toggle("R", on: lane.arm, color: .systemRed, in: r)
                case .destination:
                    NSString(string: "▾ " + (lane.destination ?? "No Destination")).draw(
                        in: r, withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
                }
            }
            NSColor.separatorColor.setFill()
            NSRect(x: 0, y: top + laneHeight - 0.5, width: bounds.width, height: 1).fill()
        }
        let add = rowTop(document.lanes.count)
        NSString(string: "+ Add Lane").draw(
            at: CGPoint(x: 8, y: add + 6),
            withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
        NSColor.windowBackgroundColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: EditorCanvas.rulerHeight).fill()
        NSColor.separatorColor.setFill()
        NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill()
    }

    private func toggle(_ letter: String, on: Bool, color: NSColor, in r: CGRect) {
        let box = NSBezierPath(roundedRect: r, xRadius: 3, yRadius: 3)
        (on ? color : NSColor.quaternaryLabelColor).setFill()
        box.fill()
        let s = NSString(string: letter)
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 10, weight: .bold),
            .foregroundColor: on ? NSColor.black : NSColor.secondaryLabelColor,
        ]
        let size = s.size(withAttributes: attrs)
        s.draw(at: CGPoint(x: r.midX - size.width / 2, y: r.midY - size.height / 2), withAttributes: attrs)
    }

    private func row(at y: CGFloat) -> Int? {
        let i = Int(((y - EditorCanvas.rulerHeight + verticalOffset) / laneHeight).rounded(.down))
        return i >= 0 ? i : nil
    }

    public override func mouseDown(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        guard let seq = document.selectedSeq, let i = row(at: p.y) else { return }
        let lanes = document.lanes
        guard i < lanes.count else {
            if i == lanes.count {
                editor.edit(.lane(edit: .add(seq: seq, name: "Lane \(lanes.count + 1)", index: nil)))
            }
            return
        }
        let lane = lanes[i]
        switch LaneHeaderLayout.control(at: p, rowTop: rowTop(i)) {
        case .mute: flags(seq, lane.id, mute: !lane.mute)
        case .solo: flags(seq, lane.id, solo: !lane.solo)
        case .arm: flags(seq, lane.id, arm: !lane.arm)
        case .destination: destinationMenu(seq: seq, lane: lane, at: p)
        case .name where e.clickCount >= 2: beginRename(lane, row: i)
        case .name, nil: break
        }
    }

    private func flags(_ seq: UInt64, _ lane: UInt64, mute: Bool? = nil, solo: Bool? = nil, arm: Bool? = nil) {
        editor.edit(.lane(edit: .setFlags(seq: seq, lane: lane, mute: mute, solo: solo, arm: arm)))
    }

    private func destinationMenu(seq: UInt64, lane: LaneRow, at p: CGPoint) {
        let menu = NSMenu()
        let none = MenuAction(title: "No Destination") { [unowned self] in
            self.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: nil)))
        }
        menu.addItem(none.item(checked: lane.midiDest == nil))
        let dests = (try? document.session.destinations()) ?? []
        for d in dests {
            let a = MenuAction(title: d.name) { [unowned self] in
                self.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: d.id)))
            }
            menu.addItem(a.item(checked: lane.midiDest == d.id))
        }
        menu.popUp(positioning: nil, at: p, in: self)
    }

    private func beginRename(_ lane: LaneRow, row i: Int) {
        renaming?.field.removeFromSuperview()
        guard let r = LaneHeaderLayout.controls(rowTop: rowTop(i)).first(where: { $0.0 == .name })?.1 else { return }
        let field = NSTextField(string: lane.name)
        field.frame = r.insetBy(dx: -2, dy: -2)
        field.delegate = self
        addSubview(field)
        window?.makeFirstResponder(field)
        renaming = (lane.id, field)
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        guard let r = renaming, let seq = document.selectedSeq else { return }
        let (lane, field) = (r.lane, r.field)
        renaming = nil
        let name = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        field.removeFromSuperview()
        let current = document.lanes.first { $0.id == lane }?.name
        if !name.isEmpty, name != current {
            editor.edit(.lane(edit: .rename(seq: seq, lane: lane, name: name)))
        }
    }

    public override func scrollWheel(with e: NSEvent) {
        nextResponder?.scrollWheel(with: e)
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Lane Headers" }

    public override func accessibilityChildren() -> [Any]? {
        guard let seq = document.selectedSeq else { return [] }
        return document.lanes.enumerated().map { i, lane in
            let r = NSRect(x: 0, y: rowTop(i), width: bounds.width, height: laneHeight)
            let item = CanvasAccessibilityItem(
                label: ArrangementA11y.header(lane), frame: r, selected: false,
                press: { [unowned self] in self.flags(seq, lane.id, mute: !lane.mute) })
            return CanvasElement(item, parent: self, frame: r)
        }
    }
}

/// A menu item that runs a closure.
@MainActor
final class MenuAction: NSObject {
    let title: String
    let run: @MainActor () -> Void
    private static var alive: [MenuAction] = []

    init(title: String, run: @escaping @MainActor () -> Void) {
        self.title = title
        self.run = run
    }

    func item(checked: Bool) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: #selector(fire), keyEquivalent: "")
        i.target = self
        i.state = checked ? .on : .off
        Self.alive.append(self)
        if Self.alive.count > 512 { Self.alive.removeFirst(256) }
        return i
    }

    @objc func fire() { run() }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/ArrangementContainer.swift <<'VIZDAW_EOF'
import AppKit
import VizDAWAppModel

/// Lane headers on the left, the arrangement canvas on the right; they
/// scroll vertically together.
@MainActor
public final class ArrangementContainer: NSView {
    public let headers: LaneHeaderView
    public let canvas: ArrangementView

    public init(document: DocumentModel) {
        headers = LaneHeaderView(document: document)
        canvas = ArrangementView(document: document)
        super.init(frame: .zero)
        addSubview(headers)
        addSubview(canvas)
        canvas.onVerticalScroll = { [unowned self] y in self.headers.verticalOffset = y }
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    public override func layout() {
        super.layout()
        let w = LaneHeaderLayout.width
        headers.frame = NSRect(x: 0, y: 0, width: w, height: bounds.height)
        canvas.frame = NSRect(x: w, y: 0, width: max(bounds.width - w, 0), height: bounds.height)
    }

    public override func scrollWheel(with e: NSEvent) {
        canvas.scrollWheel(with: e)
    }
}
VIZDAW_EOF
```

`MenuAction` keeps itself alive in a small static list because `NSMenuItem.target` is weak; the list is trimmed so it never grows without bound.

- [ ] **Step 7: Show the arrangement in the window**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/ArrangementPane.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The arrangement with its tool bar and status line.
struct ArrangementPane: View {
    let model: DocumentModel

    var body: some View {
        VStack(spacing: 0) {
            ArrangementBar(model: model, editor: model.editor)
            Divider()
            if model.selectedSeq == nil {
                ContentUnavailableView(
                    "No Sequence", systemImage: "music.note.list",
                    description: Text("Choose File > Create Demo Project to try VizDAW."))
            } else {
                ArrangementHost(document: model)
            }
            StatusLine(editor: model.editor)
        }
    }
}

struct ArrangementHost: NSViewRepresentable {
    let document: DocumentModel

    func makeNSView(context: Context) -> ArrangementContainer {
        ArrangementContainer(document: document)
    }

    func updateNSView(_ view: ArrangementContainer, context: Context) {}
}

struct ArrangementBar: View {
    let model: DocumentModel
    @Bindable var editor: EditorModel

    var body: some View {
        HStack(spacing: 10) {
            if let parent = editor.navigation.last,
                let name = model.snapshot.sequences.first(where: { $0.id == parent })?.name
            {
                Button("◀ \(name)") { editor.back() }
                    .help("Back to the sequence this reference sits in")
            }
            Picker("Tool", selection: $editor.tool) {
                ForEach(Tool.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 210)
            Toggle(
                "Snap",
                isOn: Binding(
                    get: { editor.layout.snap.enabled },
                    set: { on in editor.updateLayout { $0.snap.enabled = on } })
            )
            .help("Snap to the edit grid (G)")
            Menu(editor.layout.snap.division.title) {
                ForEach(Snap.Division.allCases, id: \.self) { d in
                    Button(d.title) { editor.updateLayout { $0.snap.division = d } }
                }
                Divider()
                Toggle(
                    "Triplet",
                    isOn: Binding(
                        get: { editor.layout.snap.triplet },
                        set: { on in editor.updateLayout { $0.snap.triplet = on } }))
                Toggle(
                    "Dotted",
                    isOn: Binding(
                        get: { editor.layout.snap.dotted },
                        set: { on in editor.updateLayout { $0.snap.dotted = on } }))
            }
            .frame(width: 70)
            Menu("Blocks: \(editor.layout.blockRule.title)") {
                ForEach(BlockRuleSetting.choices, id: \.self) { r in
                    Button(r.title) { editor.updateLayout { $0.blockRule = r } }
                }
            }
            .frame(width: 210)
            Menu("Light: \(editor.layout.lightMode.title)") {
                ForEach(LightMode.allCases, id: \.self) { m in
                    Button(m.title) { editor.updateLayout { $0.lightMode = m } }
                }
            }
            .frame(width: 140)
            Toggle(
                "Follow",
                isOn: Binding(
                    get: { editor.layout.follow },
                    set: { on in editor.updateLayout { $0.follow = on } }))
            Spacer()
            Button {
                editor.updateLayout { $0.ticksPerPoint = min($0.ticksPerPoint * 1.5, 61_440) }
            } label: {
                Image(systemName: "minus.magnifyingglass")
            }
            .help("Zoom Out")
            Button {
                editor.updateLayout { $0.ticksPerPoint = max($0.ticksPerPoint / 1.5, 2) }
            } label: {
                Image(systemName: "plus.magnifyingglass")
            }
            .help("Zoom In")
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}

/// The last failure of an editor action (spec §5: non-modal).
struct StatusLine: View {
    let editor: EditorModel

    var body: some View {
        if let s = editor.status {
            HStack {
                Image(systemName: "exclamationmark.triangle").foregroundStyle(.orange)
                Text(s).lineLimit(1)
                Spacer()
                Button("Dismiss") { editor.clearStatus() }.buttonStyle(.borderless)
            }
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(.bar)
            .task(id: s) {
                try? await Task.sleep(for: .seconds(6))
                if editor.status == s { editor.clearStatus() }
            }
        }
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/DocumentView.swift`, the detail column shows the arrangement instead of the lane table (Task 15 adds the split detail pane):

Find:
```swift
        } detail: {
            LaneTable(model: model)
        }
```
Replace:
```swift
        } detail: {
            ArrangementPane(model: model)
        }
```

Delete the now unused `struct LaneTable: View { … }` from `DocumentView.swift` (the whole struct, from `struct LaneTable: View {` to its closing brace before `struct TransportToolbar`).

- [ ] **Step 8: Build, test, look**

Run: `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test`
Expected: PASS (173 Swift tests).

Then `scripts/swift.sh run VizDAW --demo` (with the `VIZDAW_AUDIO=offline` environment variable if no audio device should play) and check by eye: Song shows lanes Groove and Chords; Groove holds two purple "↻ Groove" blocks, Chords one blue block with note sketches; Space plays and the red playhead moves smoothly; a ruler drag shades a range; dragging the second Groove block one bar right moves it (Edit > Undo "Move Block" moves it back); ⌥-dragging copies; dragging a Chords block onto the Groove row merges it there. Quit with ⌘Q.

- [ ] **Step 9: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Sources/VizDAWEditors/ArrangementLayout.swift app/Sources/VizDAWEditors/ArrangementView.swift \
  app/Sources/VizDAWEditors/LaneHeaderView.swift app/Sources/VizDAWEditors/ArrangementContainer.swift \
  app/Sources/VizDAW/ArrangementPane.swift app/Sources/VizDAW/DocumentView.swift \
  app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Tests/VizDAWEditorsTests/ArrangementLayoutTests.swift app/Tests/VizDAWAppModelTests/NavigationTests.swift
git commit -m "feat(editors): the arrangement with lane headers, block drags and light drawing

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 173 Swift tests.

---

### Task 13: The sequence sidebar as a working surface

**Files:**
- Create: `app/Sources/VizDAWAppModel/DocumentModel+Sequences.swift`, `app/Sources/VizDAW/SequenceSidebar.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Sources/VizDAW/DocumentView.swift`
- Test: `app/Tests/VizDAWAppModelTests/SidebarTests.swift` (new)

**Interfaces:**
- Consumes: Task 5's `newSequenceFromTemplate(name:template:)`, `SequenceTemplate`; Task 4's `SequenceEdit` (`duplicate`, `setComment`, `setKeyboardSlot`, `delete`); Task 9's `SequenceOrder`, `EditorModel.edit`, `updateLayout`; `SeqRow.keyboardSlot`, `useCount`, `comment`.
- Produces: `KeySlotFormat.text(_ slot: KeySlotRow?) -> String`; `CommentDraft { seq, text }`, `KeySlotDraft { seq, key, shift, control, option, command }` (both `Identifiable` by `seq`); `DocumentModel.commentDraft: CommentDraft?`, `slotDraft: KeySlotDraft?`, `pendingDelete: UInt64?`; `DocumentModel.moveSequences(section:from:to:)`, `duplicate(_:)`, `newSequence(_ template: SequenceTemplate)`, `uniqueName(_ base: String) -> String`, `beginComment(_:)`, `commitComment()`, `beginSlot(_:)`, `commitSlot()`, `requestDelete(_:)`, `confirmDelete()`; SwiftUI `SequenceSidebar` (replaces the one in `DocumentView.swift`), `SequenceRow`, `CommentSheet`, `KeySlotSheet`.

Parent spec §9 "Sequence sidebar", Rulings 12 and 13. Deleting a sequence that references use asks first and then removes those references too (the core's `delete_references`).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/SidebarTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct SidebarTests {
    @Test func keySlotsUseAppleModifierOrder() {
        #expect(KeySlotFormat.text(KeySlotRow(key: "a", modifiers: 15)) == "⌃⌥⇧⌘A")
        #expect(KeySlotFormat.text(KeySlotRow(key: "F5", modifiers: 0)) == "F5")
        #expect(KeySlotFormat.text(nil) == "")
    }

    @Test func reorderingASectionStoresTheOrder() throws {
        let m = try DocumentModel(session: editorFake())
        m.moveSequences(section: m.referenced, from: IndexSet(integer: 1), to: 0)
        #expect(m.referenced.map(\.id) == [2, 1])
        #expect(m.editor.layout.sequenceOrder == [2, 1, 3])
        #expect(m.topLevel.map(\.id) == [3])
    }

    @Test func duplicateIsACommand() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.duplicate(1)
        #expect(f.edits == [.sequence(edit: .duplicate(seq: 1))])
    }

    @Test func aNewSequenceFromATemplateIsSelected() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        nextSnapshot(f) { $0.sequences.append(seq(4, "Sequence 1", topLevel: true)) }
        m.newSequence(.drumKit)
        #expect(f.templates.map(\.name) == ["Sequence 1"])
        #expect(f.templates.map(\.template) == [.drumKit])
        #expect(m.selectedSeq == 4)
    }

    @Test func newNamesSkipTakenOnes() throws {
        let f = editorFake()
        f.snap.sequences.append(seq(4, "Sequence 1", topLevel: true))
        let m = try DocumentModel(session: f)
        #expect(m.uniqueName("Sequence") == "Sequence 2")
    }

    @Test func commentsAndSlotsAreCommands() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.beginComment(3)
        m.commentDraft?.text = "verse idea"
        m.commitComment()
        m.beginSlot(3)
        m.slotDraft?.key = "a"
        m.slotDraft?.option = true
        m.commitSlot()
        m.beginSlot(3)
        m.slotDraft?.key = "  "
        m.commitSlot()
        #expect(
            f.edits == [
                .sequence(edit: .setComment(seq: 3, comment: "verse idea")),
                .sequence(edit: .setKeyboardSlot(seq: 3, slot: KeySlotRow(key: "a", modifiers: 4))),
                .sequence(edit: .setKeyboardSlot(seq: 3, slot: nil)),
            ])
        #expect(m.commentDraft == nil && m.slotDraft == nil)
    }

    @Test func deletingAUsedSequenceAsksFirst() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.requestDelete(1)
        #expect(m.pendingDelete == 1)
        #expect(f.edits.isEmpty)
        m.confirmDelete()
        #expect(f.edits == [.sequence(edit: .delete(seq: 1, deleteReferences: true))])
        m.requestDelete(3)
        #expect(f.edits.last == .sequence(edit: .delete(seq: 3, deleteReferences: false)))
        #expect(m.pendingDelete == nil)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/swift.sh test`
Expected: compile errors — cannot find `KeySlotFormat`, no member `moveSequences`.

- [ ] **Step 3: Sequence intents**

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
    /// The fault popover is open.
    public var showsFaults = false
```
Replace:
```swift
    /// The fault popover is open.
    public var showsFaults = false
    /// Sidebar sheets and the delete confirmation (plan 4a).
    public var commentDraft: CommentDraft?
    public var slotDraft: KeySlotDraft?
    public var pendingDelete: UInt64?
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/DocumentModel+Sequences.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// A keyboard slot as menus show it ("⌃⌥⇧⌘A").
public enum KeySlotFormat {
    public static func text(_ slot: KeySlotRow?) -> String {
        guard let slot else { return "" }
        var s = ""
        if slot.modifiers & 2 != 0 { s += "⌃" }
        if slot.modifiers & 4 != 0 { s += "⌥" }
        if slot.modifiers & 1 != 0 { s += "⇧" }
        if slot.modifiers & 8 != 0 { s += "⌘" }
        return s + (slot.key.count == 1 ? slot.key.uppercased() : slot.key)
    }
}

/// The comment being edited.
public struct CommentDraft: Identifiable, Equatable, Sendable {
    public let seq: UInt64
    public var text: String
    public var id: UInt64 { seq }
}

/// The keyboard slot being edited.
public struct KeySlotDraft: Identifiable, Equatable, Sendable {
    public let seq: UInt64
    public var key: String
    public var shift = false
    public var control = false
    public var option = false
    public var command = false
    public var id: UInt64 { seq }

    var slot: KeySlotRow? {
        let k = key.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !k.isEmpty else { return nil }
        let m = (shift ? 1 : 0) | (control ? 2 : 0) | (option ? 4 : 0) | (command ? 8 : 0)
        return KeySlotRow(key: k, modifiers: UInt8(m))
    }
}

/// The sequence sidebar's intents (parent spec §9).
extension DocumentModel {
    /// Rows `source` of a sidebar section moved to `destination`.
    public func moveSequences(section: [SeqRow], from source: IndexSet, to destination: Int) {
        let order = SequenceOrder.moved(
            all: snapshot.sequences, order: editor.layout.sequenceOrder,
            visible: section.map(\.id), from: source, to: destination)
        editor.updateLayout { $0.sequenceOrder = order }
    }

    public func duplicate(_ seq: UInt64) {
        editor.edit(.sequence(edit: .duplicate(seq: seq)))
    }

    /// `base` or "`base` n" with the smallest n not taken.
    public func uniqueName(_ base: String) -> String {
        let taken = Set(snapshot.sequences.map(\.name))
        var n = 1
        while taken.contains("\(base) \(n)") { n += 1 }
        return "\(base) \(n)"
    }

    /// A new sequence from `template` (one undo step), then selected.
    public func newSequence(_ template: SequenceTemplate) {
        let before = Set(snapshot.sequences.map(\.id))
        let name = uniqueName("Sequence")
        do {
            apply(try session.newSequenceFromTemplate(name: name, template: template))
        } catch {
            editor.report(error)
            return
        }
        if let made = snapshot.sequences.first(where: { !before.contains($0.id) }) {
            choose(made.id)
        }
    }

    public func beginComment(_ seq: UInt64) {
        commentDraft = CommentDraft(
            seq: seq, text: snapshot.sequences.first { $0.id == seq }?.comment ?? "")
    }

    public func commitComment() {
        guard let d = commentDraft else { return }
        commentDraft = nil
        editor.edit(.sequence(edit: .setComment(seq: d.seq, comment: d.text)))
    }

    public func beginSlot(_ seq: UInt64) {
        let slot = snapshot.sequences.first { $0.id == seq }?.keyboardSlot
        var d = KeySlotDraft(seq: seq, key: slot?.key ?? "")
        let m = slot?.modifiers ?? 0
        d.shift = m & 1 != 0
        d.control = m & 2 != 0
        d.option = m & 4 != 0
        d.command = m & 8 != 0
        slotDraft = d
    }

    public func commitSlot() {
        guard let d = slotDraft else { return }
        slotDraft = nil
        editor.edit(.sequence(edit: .setKeyboardSlot(seq: d.seq, slot: d.slot)))
    }

    /// Deletes an unused sequence at once; a used one waits for
    /// `confirmDelete` (its references go too).
    public func requestDelete(_ seq: UInt64) {
        let uses = snapshot.sequences.first { $0.id == seq }?.useCount ?? 0
        if uses > 0 {
            pendingDelete = seq
        } else {
            editor.edit(.sequence(edit: .delete(seq: seq, deleteReferences: false)))
        }
    }

    public func confirmDelete() {
        guard let seq = pendingDelete else { return }
        pendingDelete = nil
        editor.edit(.sequence(edit: .delete(seq: seq, deleteReferences: true)))
    }
}
VIZDAW_EOF
```

`DocumentModel.apply(_:)` is `internal`; the extension lives in the same module, so it can call it.

- [ ] **Step 4: Run the tests**

Run: `scripts/swift.sh test`
Expected: PASS — 7 new tests (180 Swift tests).

- [ ] **Step 5: The sidebar view**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/SequenceSidebar.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The sequence sidebar (parent spec §9): name, keyboard slot, comment and
/// use count per sequence; drag to reorder; duplicate, delete, comment,
/// slot and new-from-template from the context menu and the "+" menu.
struct SequenceSidebar: View {
    @Bindable var model: DocumentModel

    var body: some View {
        List(selection: Binding(get: { model.selectedSeq }, set: { model.choose($0) })) {
            Section("Top-level") {
                ForEach(model.topLevel) { row in SequenceRow(model: model, row: row) }
                    .onMove { model.moveSequences(section: model.topLevel, from: $0, to: $1) }
            }
            Section("Used as reference") {
                ForEach(model.referenced) { row in SequenceRow(model: model, row: row) }
                    .onMove { model.moveSequences(section: model.referenced, from: $0, to: $1) }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            HStack {
                Menu {
                    Button("New Sequence") { model.newSequence(.empty) }
                    TemplateItems(model: model)
                } label: {
                    Image(systemName: "plus")
                }
                .menuStyle(.borderlessButton)
                .frame(width: 32)
                .help("New Sequence")
                Spacer()
            }
            .padding(6)
        }
        .sheet(item: $model.commentDraft) { _ in CommentSheet(model: model) }
        .sheet(item: $model.slotDraft) { _ in KeySlotSheet(model: model) }
        .confirmationDialog(
            "Delete this sequence and every reference to it?",
            isPresented: Binding(
                get: { model.pendingDelete != nil }, set: { if !$0 { model.pendingDelete = nil } })
        ) {
            Button("Delete", role: .destructive) { model.confirmDelete() }
            Button("Cancel", role: .cancel) { model.pendingDelete = nil }
        } message: {
            Text("Other sequences use it; their references are removed too. Undo brings both back.")
        }
    }
}

/// The "New from Template" items (Plan Ruling 13).
struct TemplateItems: View {
    let model: DocumentModel

    var body: some View {
        Menu("New from Template") {
            Button("Empty") { model.newSequence(.empty) }
            Button("Drum Kit") { model.newSequence(.drumKit) }
            Divider()
            ForEach(model.snapshot.sequences) { s in
                Button("Same Lanes as \(s.name)") { model.newSequence(.sameLanesAs(seq: s.id)) }
            }
        }
    }
}

struct SequenceRow: View {
    @Bindable var model: DocumentModel
    let row: SeqRow
    @FocusState private var focused: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 1) {
                if model.renamingSeq == row.id {
                    TextField("Name", text: $model.renameDraft)
                        .focused($focused)
                        .onSubmit { model.commitRename(model.renameDraft) }
                        .onExitCommand { model.cancelRename() }
                        .onAppear { focused = true }
                        .onChange(of: focused) { _, now in
                            if !now { model.commitRename(model.renameDraft) }
                        }
                } else {
                    Text(row.name)
                        .onTapGesture(count: 2) { model.beginRename(row.id) }
                }
                if !row.comment.isEmpty {
                    Text(row.comment)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer()
            if row.keyboardSlot != nil {
                Text(KeySlotFormat.text(row.keyboardSlot))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .help("Keyboard slot")
            }
            if row.useCount > 0 {
                Text("\(row.useCount)")
                    .font(.caption.monospacedDigit())
                    .padding(.horizontal, 6)
                    .background(Capsule().fill(.quaternary))
                    .help("Used by \(row.useCount) reference(s)")
            }
        }
        .tag(row.id)
        .accessibilityElement(children: .combine)
        .contextMenu {
            Button("Rename") { model.beginRename(row.id) }
            Button("Duplicate") { model.duplicate(row.id) }
            Button("Comment…") { model.beginComment(row.id) }
            Button("Keyboard Slot…") { model.beginSlot(row.id) }
            TemplateItems(model: model)
            Divider()
            Button("Delete…", role: .destructive) { model.requestDelete(row.id) }
        }
    }
}

struct CommentSheet: View {
    @Bindable var model: DocumentModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Comment").font(.headline)
            TextEditor(
                text: Binding(
                    get: { model.commentDraft?.text ?? "" },
                    set: { model.commentDraft?.text = $0 })
            )
            .frame(width: 320, height: 120)
            HStack {
                Spacer()
                Button("Cancel") { model.commentDraft = nil }.keyboardShortcut(.cancelAction)
                Button("Save") { model.commitComment() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding()
    }
}

struct KeySlotSheet: View {
    @Bindable var model: DocumentModel

    private func flag(_ title: String, _ path: WritableKeyPath<KeySlotDraft, Bool>) -> some View {
        Toggle(
            title,
            isOn: Binding(
                get: { model.slotDraft?[keyPath: path] ?? false },
                set: { model.slotDraft?[keyPath: path] = $0 }))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Keyboard Slot").font(.headline)
            TextField(
                "Key (empty clears the slot)",
                text: Binding(get: { model.slotDraft?.key ?? "" }, set: { model.slotDraft?.key = $0 }))
            HStack {
                flag("⌃", \.control)
                flag("⌥", \.option)
                flag("⇧", \.shift)
                flag("⌘", \.command)
            }
            HStack {
                Spacer()
                Button("Cancel") { model.slotDraft = nil }.keyboardShortcut(.cancelAction)
                Button("Save") { model.commitSlot() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding()
        .frame(width: 320)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/DocumentView.swift`, delete the old `struct SequenceSidebar: View { … }` and `struct SequenceRow: View { … }` (both now live in `SequenceSidebar.swift`).

- [ ] **Step 6: Build and test**

Run: `scripts/swift.sh build && scripts/swift.sh test`
Expected: PASS (180 Swift tests). With `scripts/swift.sh run VizDAW --demo`: the sidebar shows use counts (Beat 1, Groove 2); dragging Groove above Beat reorders them; the context menu's Comment… and Keyboard Slot… show under the name and at the right; "+" > New from Template > Drum Kit adds and selects "Sequence 1" with a drum lane.

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Sources/VizDAWAppModel/DocumentModel+Sequences.swift app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Sources/VizDAW/SequenceSidebar.swift app/Sources/VizDAW/DocumentView.swift \
  app/Tests/VizDAWAppModelTests/SidebarTests.swift
git commit -m "feat(app): sequence sidebar with slots, comments, reorder, duplicate and templates

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 180 Swift tests.

---

### Task 14: The subsequence workflow

**Files:**
- Create: `app/Sources/VizDAWAppModel/EditorModel+References.swift`, `app/Sources/VizDAW/ReferenceSheet.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`, `app/Sources/VizDAWEditors/ArrangementView.swift`, `app/Sources/VizDAW/DocumentView.swift`, `app/Sources/VizDAW/AppCommands.swift`
- Test: `app/Tests/VizDAWAppModelTests/ReferenceWorkflowTests.swift` (new)

**Interfaces:**
- Consumes: Task 4's `ReferenceEdit` (`fromSelection`, `makePrivateCopy`, `flatten`, `rePoint`, `setParams`); Task 9's selection and `edit`; Task 12's `openReference`, `MenuAction`, `ArrangementView` hit-testing.
- Produces: `ReferenceDraft { event: UInt64, useSequenceLength: Bool, lengthBeats: Int, fill: Bool, loops: Int, transpose: Int }` (`Identifiable` by `event`) with `init(row: EventRow)` and `command: EditCommand`; `EditorModel.referenceDraft: ReferenceDraft?`; `EditorModel.makeReferenceFromSelection(seq:) -> Bool`, `makeReference(from:in:) -> Bool`, `makePrivateCopy(_:)`, `flatten(_:)`, `rePoint(_:to:)`, `editReference(_:in:)`, `commitReference()`, `rePointTargets(excluding:among:) -> [SeqRow]`; `ArrangementView.menu(for:)` block context menus; SwiftUI `ReferenceSheet`; menu Sequence > "Make Reference from Selection" (⌥⌘R).

Spec §4a "Subsequence workflow": make a reference from the selection (round to bars, one reference for the selected lanes), open the referenced sequence (double-click, Task 12), make a private copy, flatten, re-point, set params. Every one is one undo step done by the core; a refusal (a cycle) shows in the status line.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/ReferenceWorkflowTests.swift <<'VIZDAW_EOF'
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// A reference row to sequence 2 (event 7).
func referenceRow(length: Int64? = nil, loops: UInt32 = 1, transpose: Int8 = 0) -> EventRow {
    var r = noteRow(7, tick: 0)
    r.kind = .reference
    r.refSeq = 2
    r.refLength = length
    r.refLoops = loops
    r.refTranspose = transpose
    r.duration = 4 * 61_440
    return r
}

@MainActor
@Suite struct ReferenceWorkflowTests {
    @Test func makingAReferenceNeedsARange() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        #expect(!m.editor.makeReferenceFromSelection(seq: 3))
        #expect(m.editor.status?.contains("time range") == true)
        #expect(f.edits.isEmpty)
    }

    @Test func aReferenceIsMadeFromTheSelection() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.selectRange(TickRange(start: 0, end: 61_440), lanes: [chords])
        #expect(m.editor.makeReferenceFromSelection(seq: 3))
        #expect(
            f.edits == [
                .reference(edit: .fromSelection(seq: 3, perLane: false, roundToBars: true, loopCount: 1))
            ])
    }

    @Test func aBlockBecomesAReferenceBySelectingItFirst() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        #expect(m.editor.makeReference(from: blockRow(0, 61_440, events: [1]), in: chords))
        #expect(f.selectionSets.last?.timeRange == TickRange(start: 0, end: 61_440))
        #expect(f.selectionSets.last?.lanes == [LaneRef(seq: 3, lane: 31)])
        #expect(f.edits.count == 1)
    }

    @Test func copyFlattenAndRePointAreCommands() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.makePrivateCopy(7)
        m.editor.flatten(7)
        m.editor.rePoint(7, to: 1)
        #expect(
            f.edits == [
                .reference(edit: .makePrivateCopy(event: 7)),
                .reference(edit: .flatten(event: 7)),
                .reference(edit: .rePoint(event: 7, target: 1)),
            ])
    }

    @Test func aRefusedRePointShowsWhy() throws {
        let f = editorFake()
        f.editError = .Model(message: "reference cycle: Groove uses Song")
        let m = try DocumentModel(session: f)
        m.editor.rePoint(7, to: 3)
        #expect(m.editor.status == "reference cycle: Groove uses Song")
    }

    @Test func theParametersDraftReadsAndWritesTheReference() throws {
        var d = ReferenceDraft(row: referenceRow(length: nil, loops: 0, transpose: -2))
        #expect(d.useSequenceLength && d.fill && d.transpose == -2)
        d.useSequenceLength = false
        d.lengthBeats = 8
        d.fill = false
        d.loops = 3
        d.transpose = 5
        #expect(
            d.command
                == .reference(edit: .setParams(event: 7, length: 8 * 15_360, loopCount: 3, transpose: 5)))
        let e = ReferenceDraft(row: referenceRow(length: 2 * 15_360, loops: 2))
        #expect(!e.useSequenceLength && e.lengthBeats == 2 && e.loops == 2 && !e.fill)
    }

    @Test func editingAReferenceOpensItsDraftAndCommitSends() throws {
        let f = editorFake()
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [referenceRow(loops: 2)])
        let m = try DocumentModel(session: f)
        m.editor.editReference(blockRow(0, 10, events: [7], ref: 2, label: "Groove"), in: grooveLane)
        #expect(m.editor.referenceDraft?.loops == 2)
        m.editor.referenceDraft?.transpose = 7
        m.editor.commitReference()
        #expect(m.editor.referenceDraft == nil)
        #expect(f.edits == [.reference(edit: .setParams(event: 7, length: nil, loopCount: 2, transpose: 7))])
    }

    @Test func rePointTargetsSkipTheCurrentOne() {
        let rows = [seq(1, "Beat", topLevel: false), seq(2, "Groove", topLevel: false), seq(3, "Song", topLevel: true)]
        #expect(EditorModel.rePointTargets(excluding: 2, among: rows).map(\.id) == [1, 3])
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/swift.sh test`
Expected: compile errors — cannot find `ReferenceDraft`, no member `makeReferenceFromSelection`.

- [ ] **Step 3: Reference intents**

In `app/Sources/VizDAWAppModel/EditorModel.swift`:

Find:
```swift
    /// The drag in progress (spec §3.8).
    public internal(set) var gesture: Gesture?
```
Replace:
```swift
    /// The drag in progress (spec §3.8).
    public internal(set) var gesture: Gesture?
    /// The reference whose parameters are being edited.
    public var referenceDraft: ReferenceDraft?
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/EditorModel+References.swift <<'VIZDAW_EOF'
import VizDAWCore

/// A reference's per-use parameters as the sheet edits them.
public struct ReferenceDraft: Identifiable, Equatable, Sendable {
    public let event: UInt64
    /// Use the referenced sequence's own length.
    public var useSequenceLength: Bool
    /// Per-use length in quarter notes (when not the sequence's length).
    public var lengthBeats: Int
    /// Repeat the sequence until the per-use length is filled.
    public var fill: Bool
    public var loops: Int
    public var transpose: Int

    public var id: UInt64 { event }

    public init(row: EventRow) {
        event = row.id
        useSequenceLength = row.refLength == nil
        lengthBeats = Int(max((row.refLength ?? row.duration) / MeterMath.ppq, 1))
        fill = row.refLoops == 0
        loops = Int(max(row.refLoops, 1))
        transpose = Int(row.refTranspose)
    }

    public var command: EditCommand {
        .reference(
            edit: .setParams(
                event: event,
                length: useSequenceLength ? nil : Int64(max(lengthBeats, 1)) * MeterMath.ppq,
                loopCount: fill ? 0 : UInt32(clamping: max(loops, 1)),
                transpose: Int8(clamping: transpose)))
    }
}

/// The subsequence workflow (spec §4a).
extension EditorModel {
    /// Moves the selected lanes' events in the selection's range into a new
    /// sequence and puts one reference in their place (rounded to bars).
    @discardableResult
    public func makeReferenceFromSelection(seq: UInt64) -> Bool {
        guard selection.timeRange != nil, selection.lanes.contains(where: { $0.seq == seq }) else {
            status = "Select a time range in the ruler (or a block) first."
            return false
        }
        return edit(.reference(edit: .fromSelection(seq: seq, perLane: false, roundToBars: true, loopCount: 1)))
    }

    /// A block's content becomes a reference.
    @discardableResult
    public func makeReference(from block: BlockRow, in lane: LaneKey) -> Bool {
        selectRange(TickRange(start: block.start, end: max(block.end, block.start + 1)), lanes: [lane])
        return makeReferenceFromSelection(seq: lane.seq)
    }

    public func makePrivateCopy(_ event: UInt64) {
        edit(.reference(edit: .makePrivateCopy(event: event)))
    }

    public func flatten(_ event: UInt64) {
        edit(.reference(edit: .flatten(event: event)))
    }

    public func rePoint(_ event: UInt64, to target: UInt64) {
        edit(.reference(edit: .rePoint(event: event, target: target)))
    }

    /// Opens the parameters sheet for a reference block.
    public func editReference(_ block: BlockRow, in lane: LaneKey) {
        guard let id = block.events.first,
            let row = laneEvents(lane)?.events.first(where: { $0.id == id && $0.kind == .reference })
        else { return }
        referenceDraft = ReferenceDraft(row: row)
    }

    public func commitReference() {
        guard let d = referenceDraft else { return }
        referenceDraft = nil
        edit(d.command)
    }

    /// Sequences a reference may point to instead of `current`.
    public static func rePointTargets(excluding current: UInt64?, among rows: [SeqRow]) -> [SeqRow] {
        rows.filter { $0.id != current }
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests**

Run: `scripts/swift.sh test`
Expected: PASS — 8 new tests (188 Swift tests).

- [ ] **Step 5: Block context menus, the sheet, the menu command**

In `app/Sources/VizDAWEditors/ArrangementView.swift`, add before `// MARK: Accessibility`:

```swift
    // MARK: Context menus (spec §4a subsequence workflow)

    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = content.convert(event.locationInWindow, from: nil)
        guard case .block(let b) = hit(p) else { return nil }
        if !editor.isSelected(b.block) { editor.selectBlock(b.block, in: b.lane, adding: false) }
        let menu = NSMenu()
        func add(_ title: String, _ run: @escaping @MainActor () -> Void) {
            menu.addItem(MenuAction(title: title, run: run).item(checked: false))
        }
        let block = b.block
        let key = b.lane
        if block.kind == .reference, let event = block.events.first {
            add("Open Sequence") { [unowned self] in self.editor.openReference(block) }
            add("Parameters…") { [unowned self] in self.editor.editReference(block, in: key) }
            add("Make Private Copy") { [unowned self] in self.editor.makePrivateCopy(event) }
            add("Flatten") { [unowned self] in self.editor.flatten(event) }
            let sub = NSMenu()
            for s in EditorModel.rePointTargets(excluding: block.refSeq, among: document.snapshot.sequences) {
                sub.addItem(
                    MenuAction(title: s.name) { [unowned self] in self.editor.rePoint(event, to: s.id) }
                        .item(checked: false))
            }
            let rePoint = NSMenuItem(title: "Re-point To", action: nil, keyEquivalent: "")
            rePoint.submenu = sub
            menu.addItem(rePoint)
        } else {
            for kind in DetailEditorKind.allCases {
                add("Open in \(kind.title)") { [unowned self] in
                    self.editor.openDetail(
                        .block(seq: key.seq, lane: key.lane, start: block.start, end: block.end), kind: kind)
                }
            }
            add("Make Reference") { [unowned self] in self.editor.makeReference(from: block, in: key) }
        }
        menu.addItem(.separator())
        add("Delete") { [unowned self] in self.editor.edit(.event(edit: .delete(events: block.events))) }
        return menu
    }

```

`hit(_:)` and `editor` are `private`/internal members of the same class, so the override can use them.

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/ReferenceSheet.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel

/// A reference's per-use length, repeats and transpose (spec §4a "set params").
struct ReferenceSheet: View {
    @Bindable var editor: EditorModel

    private func binding<T>(_ path: WritableKeyPath<ReferenceDraft, T>, _ fallback: T) -> Binding<T> {
        Binding(
            get: { editor.referenceDraft?[keyPath: path] ?? fallback },
            set: { editor.referenceDraft?[keyPath: path] = $0 })
    }

    var body: some View {
        Form {
            Toggle("Use the sequence's length", isOn: binding(\.useSequenceLength, true))
            Stepper(
                "Length: \(editor.referenceDraft?.lengthBeats ?? 1) beats",
                value: binding(\.lengthBeats, 1), in: 1...9_999
            )
            .disabled(editor.referenceDraft?.useSequenceLength ?? true)
            Toggle("Fill the length with repeats", isOn: binding(\.fill, false))
            Stepper("Repeats: \(editor.referenceDraft?.loops ?? 1)", value: binding(\.loops, 1), in: 1...999)
                .disabled(editor.referenceDraft?.fill ?? false)
            Stepper(
                "Transpose: \(editor.referenceDraft?.transpose ?? 0) semitones",
                value: binding(\.transpose, 0), in: -48...48)
            HStack {
                Spacer()
                Button("Cancel") { editor.referenceDraft = nil }.keyboardShortcut(.cancelAction)
                Button("Apply") { editor.commitReference() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding()
        .frame(width: 340)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/DocumentView.swift`:

Find:
```swift
        .navigationTitle(model.windowTitle)
        .toolbar { TransportToolbar(model: model) }
```
Replace:
```swift
        .navigationTitle(model.windowTitle)
        .toolbar { TransportToolbar(model: model) }
        .sheet(
            item: Binding(
                get: { model.editor.referenceDraft }, set: { model.editor.referenceDraft = $0 })
        ) { _ in
            ReferenceSheet(editor: model.editor)
        }
```

In `app/Sources/VizDAW/AppCommands.swift`, add a Sequence menu after the Transport menu:

Find:
```swift
        CommandMenu("Transport") {
```
Replace:
```swift
        CommandMenu("Sequence") {
            Button("Make Reference from Selection") {
                if let doc, let seq = doc.selectedSeq { doc.editor.makeReferenceFromSelection(seq: seq) }
            }
            .keyboardShortcut("r", modifiers: [.command, .option])
            .disabled(doc?.selectedSeq == nil)
            Button("Back to Parent Sequence") { doc?.editor.back() }
                .keyboardShortcut("[", modifiers: [.command])
                .disabled(doc?.editor.navigation.isEmpty ?? true)
        }
        CommandMenu("Transport") {
```

- [ ] **Step 6: Build and test**

Run: `scripts/swift.sh build && scripts/swift.sh test`
Expected: PASS (188 Swift tests). By eye with the demo: right-click a Groove block in Song > Make Private Copy: a new sequence "Groove copy" appears in the sidebar and the block's label changes; Flatten turns a reference into ordinary events; Re-point To > Beat relabels the block; Parameters… > Repeats 2 doubles its length; a ruler drag over bars 1–4 of Chords then ⌥⌘R moves those chords into a new sequence referenced from Chords; Edit > Undo reverses each.

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Sources/VizDAWAppModel/EditorModel+References.swift app/Sources/VizDAWAppModel/EditorModel.swift \
  app/Sources/VizDAWEditors/ArrangementView.swift app/Sources/VizDAW/ReferenceSheet.swift \
  app/Sources/VizDAW/DocumentView.swift app/Sources/VizDAW/AppCommands.swift \
  app/Tests/VizDAWAppModelTests/ReferenceWorkflowTests.swift
git commit -m "feat(app): subsequence workflow — make, open, copy, flatten, re-point, parameters

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 188 Swift tests.

---

### Task 15: Window layout — split detail pane, tear-off windows, editor commands

**Files:**
- Create: `app/Sources/VizDAWAppModel/EditorModel+Windows.swift`, `app/Sources/VizDAW/DetailPane.swift`, `app/Sources/VizDAW/EditorWindows.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`, `app/Sources/VizDAW/DocumentView.swift`, `app/Sources/VizDAW/DocumentWindow.swift`, `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/VizDAWApp.swift`
- Test: `app/Tests/VizDAWAppModelTests/LayoutTests.swift` (new)

**Interfaces:**
- Consumes: Task 9's `EditorLayout`, `DetailState`, `TornWindow`, `EditorTarget`, `updateLayout`, `storeLayout`; Task 12's `openDetail`; `SessionProtocol.lanes(seq:)`; `DocumentStore.model(_:)`, `appStore`, `PluginWindows` (pattern).
- Produces: `EditorModel.detailTarget(shown:) -> EditorTarget?`, `showDetail(_:shown:)`, `closeDetail()`, `tearOff(_:shown:) -> TornWindow?`, `closeWindow(_:)`, `setWindowFrame(_:_:)`, `setWindowKind(_:_:)`, `eventCount(_:) -> Int?`, `title(_:sequences:) -> String`, internal `pruneLayout(_:)` (called from `sync`); SwiftUI `DetailPane(model:state:onKind:onTearOff:onClose:)`, `DetailArea(model:docID:)`, `EditorSplit(model:docID:)`; `EditorWindows.{show(_:doc:window:), tearOff(_:doc:kind:), reopen(_:doc:), closeDocument(_:)}`; `DocumentView(model:docID:)`; menu Editors (⌘1–⌘4, ⌥⌘1–⌥⌘4, Close Detail Editor).

Spec §3.7, Rulings 12, 14, 15. A torn-off window shows the same target as the detail pane (or, with an empty pane, the selection's lane and range, else the shown sequence); several may be open. Closing the window removes it from the layout; closing the document keeps the list so the windows reopen with the project (after Save).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/LayoutTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct LayoutTests {
    @Test func detailEditorsOpenOnTheSelectionOrTheSequence() throws {
        let m = try DocumentModel(session: editorFake())
        let e = m.editor
        #expect(e.detailTarget(shown: 3) == .sequence(3))
        e.selectRange(TickRange(start: 0, end: 61_440), lanes: [chords])
        #expect(e.detailTarget(shown: 3) == .block(seq: 3, lane: 31, start: 0, end: 61_440))
        e.showDetail(.list, shown: 3)
        #expect(e.layout.detail == DetailState(kind: .list, target: .block(seq: 3, lane: 31, start: 0, end: 61_440)))
        e.clearSelection()
        e.showDetail(.drumGrid, shown: 3)
        #expect(e.layout.detail?.target == .block(seq: 3, lane: 31, start: 0, end: 61_440), "keeps its target")
        #expect(e.layout.detail?.kind == .drumGrid)
        e.closeDetail()
        #expect(e.layout.detail == nil)
    }

    @Test func tearingOffAddsAWindowAndClosingRemovesIt() throws {
        let m = try DocumentModel(session: editorFake())
        let e = m.editor
        let w = try #require(e.tearOff(.pianoRoll, shown: 3))
        let w2 = try #require(e.tearOff(.list, shown: 3))
        #expect(w.id != w2.id)
        #expect(e.layout.windows.map(\.detail.kind) == [.pianoRoll, .list])
        e.setWindowFrame(w.id, [10, 20, 700, 400])
        e.setWindowKind(w.id, .stripChart)
        #expect(e.layout.windows[0].frame == [10, 20, 700, 400])
        #expect(e.layout.windows[0].detail.kind == .stripChart)
        e.closeWindow(w.id)
        #expect(e.layout.windows.map(\.id) == [w2.id])
    }

    @Test func targetsOfDeletedSequencesAreDropped() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 2, lane: 20))
        _ = m.editor.tearOff(.list, shown: 2)
        nextSnapshot(f) { $0.sequences.removeAll { $0.id == 2 } }
        m.rename(1, to: "Kit")
        #expect(m.editor.layout.detail == nil)
        #expect(m.editor.layout.windows.isEmpty)
    }

    @Test func eventCountsCoverTheTarget() throws {
        let f = editorFake()
        f.laneEventTable["3/31"] = LaneEvents(
            seq: 3, lane: 31, rev: 2,
            events: [noteRow(1, tick: 0), noteRow(2, tick: 15_360), noteRow(3, tick: 5 * 61_440)])
        let m = try DocumentModel(session: f)
        #expect(m.editor.eventCount(.block(seq: 3, lane: 31, start: 0, end: 61_440)) == 2)
        #expect(m.editor.eventCount(.lane(seq: 3, lane: 31)) == 3)
        #expect(m.editor.eventCount(.sequence(3)) == nil)
    }

    @Test func titlesNameSequenceLaneAndBars() throws {
        let m = try DocumentModel(session: editorFake())
        let rows = m.snapshot.sequences
        #expect(m.editor.title(.block(seq: 3, lane: 31, start: 0, end: 2 * 61_440), sequences: rows) == "Song › Chords, bars 1–2")
        #expect(m.editor.title(.lane(seq: 3, lane: 30), sequences: rows) == "Song › Groove")
        #expect(m.editor.title(.sequence(1), sequences: rows) == "Beat")
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/swift.sh test`
Expected: compile errors — no member `detailTarget`, `tearOff`.

- [ ] **Step 3: Window state in the model**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/EditorModel+Windows.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// The detail pane and torn-off editor windows (spec §3.7).
extension EditorModel {
    /// What a detail editor opens on: the selection's single lane (with its
    /// range when there is one), else the shown sequence.
    public func detailTarget(shown seq: UInt64?) -> EditorTarget? {
        if selection.lanes.count == 1, let l = selection.lanes.first {
            if let r = selection.timeRange {
                return .block(seq: l.seq, lane: l.lane, start: r.start, end: r.end)
            }
            return .lane(seq: l.seq, lane: l.lane)
        }
        return seq.map { .sequence($0) }
    }

    /// ⌘1–⌘4: the detail pane shows `kind`, on its current target or on
    /// the selection.
    public func showDetail(_ kind: DetailEditorKind, shown seq: UInt64?) {
        if let d = layout.detail {
            openDetail(d.target, kind: kind)
        } else if let t = detailTarget(shown: seq) {
            openDetail(t, kind: kind)
        }
    }

    public func closeDetail() {
        updateLayout { $0.detail = nil }
    }

    /// A new torn-off window with `kind` on the detail pane's target (or
    /// the selection's); nil when there is nothing to show.
    @discardableResult
    public func tearOff(_ kind: DetailEditorKind, shown seq: UInt64?) -> TornWindow? {
        guard let target = layout.detail?.target ?? detailTarget(shown: seq) else { return nil }
        let w = TornWindow(id: UUID(), detail: DetailState(kind: kind, target: target), frame: nil)
        updateLayout {
            $0.windows.append(w)
            $0.lastKind = kind
        }
        return w
    }

    public func closeWindow(_ id: UUID) {
        updateLayout { $0.windows.removeAll { $0.id == id } }
    }

    public func setWindowFrame(_ id: UUID, _ frame: [Double]) {
        guard let i = layout.windows.firstIndex(where: { $0.id == id }), layout.windows[i].frame != frame
        else { return }
        updateLayout { $0.windows[i].frame = frame }
    }

    public func setWindowKind(_ id: UUID, _ kind: DetailEditorKind) {
        guard let i = layout.windows.firstIndex(where: { $0.id == id }) else { return }
        updateLayout {
            $0.windows[i].detail.kind = kind
            $0.lastKind = kind
        }
    }

    /// Drops the detail target and windows of sequences that are gone.
    func pruneLayout(_ s: DocSnapshot) {
        let alive = Set(s.sequences.map(\.id))
        let detailGone = layout.detail.map { !alive.contains($0.target.seq) } ?? false
        let windowsGone = layout.windows.contains { !alive.contains($0.detail.target.seq) }
        guard detailGone || windowsGone else { return }
        updateLayout { l in
            if detailGone { l.detail = nil }
            l.windows.removeAll { !alive.contains($0.detail.target.seq) }
        }
    }

    /// How many events a target covers (the 4a placeholder's summary).
    public func eventCount(_ t: EditorTarget) -> Int? {
        switch t {
        case let .block(seq, lane, start, end):
            let stop = max(end, start + 1)
            return laneEvents(LaneKey(seq, lane))?.events.filter { $0.tick >= start && $0.tick < stop }.count
        case let .lane(seq, lane):
            return laneEvents(LaneKey(seq, lane))?.events.count
        case .sequence:
            return nil
        }
    }

    /// "Song › Chords, bars 1–2".
    public func title(_ t: EditorTarget, sequences: [SeqRow]) -> String {
        let seqName = sequences.first { $0.id == t.seq }?.name ?? "Sequence"
        guard let laneID = t.lane else { return seqName }
        let laneName = ((try? session.lanes(seq: t.seq)) ?? []).first { $0.id == laneID }?.name ?? "Lane"
        guard case let .block(_, _, start, end) = t else { return "\(seqName) › \(laneName)" }
        let meter = info(t.seq)?.meter ?? []
        let first = MeterMath.bar(at: start, meter).number
        let last = MeterMath.bar(at: max(end - 1, start), meter).number
        let bars = first == last ? "bar \(first)" : "bars \(first)–\(last)"
        return "\(seqName) › \(laneName), \(bars)"
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/EditorModel.swift`:

Find:
```swift
        revs = next
        snapshotVersion = s.version
        drawVersion += 1
        afterSync(s)
```
Replace:
```swift
        revs = next
        snapshotVersion = s.version
        drawVersion += 1
        afterSync(s)
        pruneLayout(s)
```

- [ ] **Step 4: Run the tests**

Run: `scripts/swift.sh test`
Expected: PASS — 5 new tests (193 Swift tests).

- [ ] **Step 5: The detail pane, the split and torn-off windows**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/DetailPane.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel

/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and — in 4a — a placeholder where the editor will draw (4b–4e).
struct DetailPane: View {
    let model: DocumentModel
    let state: DetailState
    let onKind: (DetailEditorKind) -> Void
    let onTearOff: (() -> Void)?
    let onClose: (() -> Void)?

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Picker("Editor", selection: Binding(get: { state.kind }, set: onKind)) {
                    ForEach(DetailEditorKind.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 380)
                Text(model.editor.title(state.target, sequences: model.snapshot.sequences))
                    .font(.headline)
                    .lineLimit(1)
                Spacer()
                if let onTearOff {
                    Button("Open in Window", action: onTearOff)
                        .help("⌥⌘\(state.kind.key)")
                }
                if let onClose {
                    Button(action: onClose) { Image(systemName: "xmark") }
                        .buttonStyle(.borderless)
                        .help("Close Detail Editor")
                }
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            Divider()
            ContentUnavailableView(
                "\(state.kind.title) arrives in plan \(state.kind.plan)",
                systemImage: "rectangle.and.pencil.and.ellipsis",
                description: Text(summary))
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
struct DetailArea: View {
    let model: DocumentModel
    let docID: UUID

    var body: some View {
        if let d = model.editor.layout.detail {
            DetailPane(
                model: model, state: d,
                onKind: { k in model.editor.openDetail(d.target, kind: k) },
                onTearOff: {
                    EditorWindows.tearOff(appStore, doc: docID, kind: d.kind)
                    model.editor.closeDetail()
                },
                onClose: { model.editor.closeDetail() })
        } else {
            ContentUnavailableView(
                "No Editor Open", systemImage: "rectangle.split.1x2",
                description: Text("Double-click a block, or press ⌘1 to ⌘4."))
        }
    }
}

/// Arrangement over the split detail pane.
struct EditorSplit: View {
    let model: DocumentModel
    let docID: UUID

    var body: some View {
        VSplitView {
            ArrangementPane(model: model)
                .frame(minHeight: 160, idealHeight: 380)
            DetailArea(model: model, docID: docID)
                .frame(minHeight: 120, idealHeight: 220)
        }
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAW/EditorWindows.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel

/// Torn-off editor windows (spec §3.7), by document and window ID.
@MainActor
enum EditorWindows {
    private struct Key: Hashable {
        let doc: UUID
        let id: UUID
    }

    private static var open: [Key: EditorWindowController] = [:]

    static func show(_ store: DocumentStore, doc: UUID, window: TornWindow) {
        let key = Key(doc: doc, id: window.id)
        if let c = open[key] {
            c.showWindow(nil)
            return
        }
        guard let model = store.model(doc) else { return }
        let c = EditorWindowController(model: model, window: window) { byUser in
            open[key] = nil
            if byUser { model.editor.closeWindow(window.id) }
        }
        open[key] = c
        c.showWindow(nil)
    }

    /// ⌥⌘1–⌥⌘4 and "Open in Window".
    static func tearOff(_ store: DocumentStore, doc: UUID, kind: DetailEditorKind) {
        guard let model = store.model(doc), let w = model.editor.tearOff(kind, shown: model.selectedSeq)
        else { return }
        show(store, doc: doc, window: w)
    }

    /// The windows saved with the project.
    static func reopen(_ store: DocumentStore, doc: UUID) {
        guard let model = store.model(doc) else { return }
        for w in model.editor.layout.windows { show(store, doc: doc, window: w) }
    }

    /// The document is closing: its windows close but stay in its layout.
    static func closeDocument(_ doc: UUID) {
        for (key, c) in open where key.doc == doc { c.closeForDocument() }
    }
}

@MainActor
final class EditorWindowController: NSWindowController, NSWindowDelegate {
    private let model: DocumentModel
    private let id: UUID
    private let onClose: (Bool) -> Void
    private var forDocument = false

    init(model: DocumentModel, window w: TornWindow, onClose: @escaping (Bool) -> Void) {
        self.model = model
        id = w.id
        self.onClose = onClose
        let saved = w.frame.flatMap { f in
            f.count == 4 ? NSRect(x: f[0], y: f[1], width: f[2], height: f[3]) : nil
        }
        let win = NSWindow(
            contentRect: saved ?? NSRect(x: 0, y: 0, width: 720, height: 420),
            styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        win.isReleasedWhenClosed = false
        win.title = "\(w.detail.kind.title) — \(model.snapshot.title)"
        win.contentView = NSHostingView(rootView: TornEditorView(model: model, id: w.id))
        super.init(window: win)
        win.delegate = self
        if let saved {
            win.setFrame(saved, display: false)
        } else {
            win.center()
        }
    }

    required init?(coder: NSCoder) { nil }

    func closeForDocument() {
        forDocument = true
        close()
    }

    func windowWillClose(_ notification: Notification) { onClose(!forDocument) }
    func windowDidMove(_ notification: Notification) { saveFrame() }
    func windowDidEndLiveResize(_ notification: Notification) { saveFrame() }

    private func saveFrame() {
        guard let f = window?.frame else { return }
        model.editor.setWindowFrame(id, [f.minX, f.minY, f.width, f.height])
    }
}

struct TornEditorView: View {
    let model: DocumentModel
    let id: UUID

    var body: some View {
        if let w = model.editor.layout.windows.first(where: { $0.id == id }) {
            DetailPane(
                model: model, state: w.detail, onKind: { model.editor.setWindowKind(id, $0) },
                onTearOff: nil, onClose: nil)
        } else {
            ContentUnavailableView("Editor Closed", systemImage: "xmark.rectangle")
        }
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/DocumentView.swift`:

Find:
```swift
struct DocumentView: View {
    @Bindable var model: DocumentModel
```
Replace:
```swift
struct DocumentView: View {
    @Bindable var model: DocumentModel
    let docID: UUID
```

Find:
```swift
        } detail: {
            ArrangementPane(model: model)
        }
```
Replace:
```swift
        } detail: {
            EditorSplit(model: model, docID: docID)
        }
```

In `app/Sources/VizDAW/DocumentWindow.swift`:

Find:
```swift
                DocumentView(model: model)
```
Replace:
```swift
                DocumentView(model: model, docID: id)
```

Find:
```swift
                        PluginWindows.reopen(store, doc: id)
```
Replace:
```swift
                        PluginWindows.reopen(store, doc: id)
                        EditorWindows.reopen(store, doc: id)
```

In `app/Sources/VizDAW/VizDAWApp.swift`:

Find:
```swift
        appStore.willClose = { PluginWindows.closeDocument($0) }
```
Replace:
```swift
        appStore.willClose = {
            PluginWindows.closeDocument($0)
            EditorWindows.closeDocument($0)
        }
```

In `app/Sources/VizDAW/AppCommands.swift`:

Find:
```swift
        CommandMenu("Sequence") {
```
Replace:
```swift
        CommandMenu("Editors") {
            ForEach(DetailEditorKind.allCases, id: \.self) { kind in
                Button(kind.title) { doc?.editor.showDetail(kind, shown: doc?.selectedSeq) }
                    .keyboardShortcut(KeyEquivalent(Character(kind.key)), modifiers: .command)
                    .disabled(doc == nil)
            }
            Divider()
            ForEach(DetailEditorKind.allCases, id: \.self) { kind in
                Button("Open \(kind.title) in Window") {
                    if let docID { EditorWindows.tearOff(store, doc: docID, kind: kind) }
                }
                .keyboardShortcut(KeyEquivalent(Character(kind.key)), modifiers: [.command, .option])
                .disabled(docID == nil)
            }
            Divider()
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
        }
        CommandMenu("Sequence") {
```

- [ ] **Step 6: Build and test**

Run: `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test`
Expected: PASS (193 Swift tests). By eye: double-clicking a Chords block shows "Piano Roll arrives in plan 4b — 3 events in this selection" under the arrangement; ⌘2 switches it to List; ⌥⌘1 opens a "Piano Roll — demo" window; Save As, close the document, reopen it: the window comes back where it was.

- [ ] **Step 7: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
git add app/Sources/VizDAWAppModel/EditorModel+Windows.swift app/Sources/VizDAWAppModel/EditorModel.swift \
  app/Sources/VizDAW/DetailPane.swift app/Sources/VizDAW/EditorWindows.swift \
  app/Sources/VizDAW/DocumentView.swift app/Sources/VizDAW/DocumentWindow.swift \
  app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/VizDAWApp.swift \
  app/Tests/VizDAWAppModelTests/LayoutTests.swift
git commit -m "feat(app): split detail pane, tear-off editor windows restored with the project

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```
Expected: all green; 608 Rust tests, 193 Swift tests.

---

### Task 16: Typing never plays, the 4a checklist, CI, final gate

**Files:**
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Sources/VizDAWEditors/LaneHeaderView.swift`
- Modify: `app/README.md`, `.github/workflows/ci.yml`
- Test: `app/Tests/VizDAWAppModelTests/DocumentModelTests.swift` (one test appended)

**Interfaces:**
- Consumes: everything above.
- Produces: `EditorModel.textEditing: Bool`; `DocumentModel.transportKeysEnabled` is false while any name, comment, slot or reference field is being edited; README "Editors (plan 4a)" section and checklist steps 21–34; CI release step for the 100 000-event test.

Space and Return are unmodified menu shortcuts (Transport), and menu key equivalents reach the menu before a text field: every text entry the 4a UI adds must switch them off, as the sidebar rename already does.

- [ ] **Step 1: Write the failing test**

Append inside `struct DocumentModelTests` in `app/Tests/VizDAWAppModelTests/DocumentModelTests.swift` (before its final closing brace):

```swift

    @Test func transportKeysAreOffWhileAnyTextIsEdited() throws {
        let m = try DocumentModel(session: demoFake())
        #expect(m.transportKeysEnabled)
        m.editor.textEditing = true
        #expect(!m.transportKeysEnabled)
        m.editor.textEditing = false
        m.beginComment(3)
        #expect(!m.transportKeysEnabled)
        m.commentDraft = nil
        m.beginSlot(3)
        #expect(!m.transportKeysEnabled)
        m.slotDraft = nil
        m.editor.referenceDraft = ReferenceDraft(row: referenceRow())
        #expect(!m.transportKeysEnabled)
        m.editor.referenceDraft = nil
        #expect(m.transportKeysEnabled)
    }
```

Run: `scripts/swift.sh test`
Expected: compile error — no member `textEditing`.

- [ ] **Step 2: Switch the transport keys off while typing**

In `app/Sources/VizDAWAppModel/EditorModel.swift`:

Find:
```swift
    /// The reference whose parameters are being edited.
    public var referenceDraft: ReferenceDraft?
```
Replace:
```swift
    /// The reference whose parameters are being edited.
    public var referenceDraft: ReferenceDraft?
    /// An editor text field (a lane name) has the keyboard.
    public var textEditing = false
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
    public var transportKeysEnabled: Bool { renamingSeq == nil }
```
Replace:
```swift
    public var transportKeysEnabled: Bool {
        renamingSeq == nil && commentDraft == nil && slotDraft == nil
            && editor.referenceDraft == nil && !editor.textEditing
    }
```

In `app/Sources/VizDAWEditors/LaneHeaderView.swift`:

Find:
```swift
        addSubview(field)
        window?.makeFirstResponder(field)
        renaming = (lane.id, field)
```
Replace:
```swift
        addSubview(field)
        window?.makeFirstResponder(field)
        renaming = (lane.id, field)
        editor.textEditing = true
```

Find:
```swift
        let (lane, field) = (r.lane, r.field)
        renaming = nil
```
Replace:
```swift
        let (lane, field) = (r.lane, r.field)
        renaming = nil
        editor.textEditing = false
```

Run: `scripts/swift.sh test`
Expected: PASS (194 Swift tests).

- [ ] **Step 3: README**

In `app/README.md`:

Find:
```markdown
1. `scripts/build-ffi.sh && scripts/swift.sh run VizDAW`. A window appears
   in front with a menu bar (File, Edit, Transport).
```
Replace:
```markdown
1. `scripts/build-ffi.sh && scripts/swift.sh run VizDAW`. A window appears
   in front with a menu bar (File, Edit, Editors, Sequence, Transport).
```

Find:
```markdown
3. Song is selected; the table lists the lanes Groove and Chords. Press
```
Replace:
```markdown
3. Song is selected; the arrangement shows the lanes Groove and Chords. Press
```

Find:
```markdown
4. Press Space again and, while it plays, switch Mute on for Chords: the
```
Replace:
```markdown
4. Press Space again and, while it plays, click M in the Chords lane header: the
```

Find:
```markdown
`VIZDAW_AUDIO=offline` renders through the offline backend in real time
```
Replace:
```markdown
## Editors (plan 4a)

- The document window shows the arrangement (the selected sequence's
  lanes and their blocks) over a detail pane. Detail editors arrive in
  plans 4b–4e; until then the pane names its target and event count.
  ⌘1–⌘4 choose piano roll, list, Strip Chart or drum grid; ⌥⌘1–⌥⌘4 open
  one in its own window.
- Blocks follow the toolbar's rule (silence gap, fixed bars, whole lane);
  a reference is always its own purple block named after its sequence.
  Drag moves, ⌥-drag copies, dragging onto another lane merges; G toggles
  snap; Escape cancels a drag; a drag in the ruler selects a time range,
  a click there locates.
- The editor layout (detail pane, editor windows and their frames, the
  sidebar order, block rule, light mode, snap, zoom) is saved in the
  project's `ui.json` when you save; it is not an undo step.

`VIZDAW_AUDIO=offline` renders through the offline backend in real time
```

Append at the end of the file:

```markdown

### Editors (plan 4a)

Use the demo project (File > Create Demo Project) unless a step says
otherwise.

21. Song shows two lane rows. Groove holds two purple "↻ Groove" blocks
    (bars 1–4 and 5–8); Chords one blue block with small note dashes. The
    ruler numbers bars 1, 2, 3…; with ⌘-scroll or a pinch the view zooms
    around the pointer and the bar labels thin out when zoomed far out.
22. Playhead: press Space. The red playhead glides smoothly (no visible
    steps at 30 per second), slows over the last two bars, and when it
    reaches the right edge the view jumps one page. Turn Follow off: the
    view stays put. Stop: the playhead stays where playback stopped.
23. Ruler: drag in the ruler from bar 2 to bar 4 with any tool chosen: the
    range is shaded (snapped to beats) in the ruler and across both lanes.
    Click in the ruler: the position display jumps there.
24. Drag feel: press on the second Groove block and move 2 points: nothing
    moves. Drag it one bar right: it follows the pointer (and, while
    playing, the music follows within a moment). Press Escape before
    releasing: it snaps back and Edit has no new undo step. Drag again and
    release: Edit > Undo Move Block puts it back in one step. With Snap off
    (G) it moves freely.
25. ⌥-drag the Chords block one bar right: a copy appears and the original
    stays. Drag the Chords block down onto the Groove row: its events join
    that lane (Undo restores). Drag a block to the right edge and hold:
    the view scrolls and the block keeps following.
26. Lane headers: M, S and R toggle mute, solo and record arm (mute is
    audible while playing); the destination menu lists the instruments;
    double-click a lane name, type, Return renames it (typing a space does
    not start playback); "+ Add Lane" adds "Lane 3".
27. Blocks menu > 1 Bar: Chords splits into twelve blocks; Whole Lane joins
    them. Light > On draws outlines only; Automatic draws filled blocks for
    the demo (fewer than 1 500 visible).
28. Sidebar: Groove shows 2 and Beat 1 (uses). Drag Groove above Beat: the
    order sticks after Save and reopen. Context menu > Comment… shows the
    comment under the name; Keyboard Slot… with ⌥ and "a" shows ⌥A. Duplicate
    adds "Groove copy". "+" > New from Template > Drum Kit adds and selects
    "Sequence 1" with a drum lane "Drums"; Same Lanes as Song copies Song's
    lanes and tempo without events. Delete… on Beat asks first and removes
    Groove's references to it; Undo brings both back.
29. Make a reference: drag in the ruler over bars 1–4, Sequence > Make
    Reference from Selection (⌥⌘R): Song gains a new sequence in the
    sidebar and the selected range of every lane becomes one purple block.
    Undo restores the events.
30. Open a reference: double-click a Groove block: the arrangement shows
    Groove, the sidebar selects it, and "◀ Song" goes back (⌘[ too).
31. Reference context menu (right-click a Groove block): Make Private Copy
    makes "Groove copy" and relabels that block only; Flatten replaces the
    block with ordinary events (the drums and bass inline); Re-point To >
    Beat relabels it; Re-point To > Song shows "reference cycle…" in the
    status line and changes nothing; Parameters… with Repeats 2 doubles
    the block, Transpose +5 is audible. Each is one Undo step.
32. Detail pane: double-click the Chords block: the pane below says "Piano
    Roll arrives in plan 4b" with the block's event count; ⌘2 switches to
    List; ⌥⌘1 opens a "Piano Roll — demo" window. Move and resize it, Save
    As `~/Desktop/editors.vizdaw`, close the project and reopen it: the
    window comes back at the same place; the arrangement keeps its block
    rule, snap and zoom.
33. Busy: put an AU v3 instrument with a slow editor on a strip (plan 3c
    steps), open its window and, while it is opening, drag a block and
    release: no alert appears; the block lands (perhaps a moment later) and
    Undo reverts it in one step.
34. VoiceOver (⌘F5): the arrangement is a group "Arrangement"; moving
    through it reads blocks such as "Block on Chords, bars 1 to 12, 37
    events" and "Reference to Groove on Groove, bars 1 to 4"; VO-Space on a
    block selects it. The lane headers read "Chords, Built-in Piano"; the
    toolbar controls (tool, Snap, Blocks, Light, Follow, zoom) are labelled.
```

- [ ] **Step 4: CI runs the big-lane test in release**

In `.github/workflows/ci.yml`:

Find:
```yaml
      - name: Test
        run: cargo test --workspace
```
Replace:
```yaml
      - name: Test
        run: cargo test --workspace
      - name: Big lane read speed (release, spec 4a §3.1)
        run: cargo test --release -p core-ffi --test events lane_events_of_100_000_events_is_fast
```

- [ ] **Step 5: Final gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace
cargo test --release -p core-ffi --test events lane_events_of_100_000_events_is_fast
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --smoke; echo "smoke exit $?"
scripts/make-app.sh
```
Expected: all green; 608 Rust tests, 194 Swift tests; the release run of the 100 000-event test passes under 50 ms; `--smoke` prints the transport, mixer and editor size and exits 0 (4a does not change it); `dist/VizDAW.app` is built. Then walk checklist steps 21–34 by hand (the user's step; note anything that fails in the hand-off).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift \
  app/Sources/VizDAWEditors/LaneHeaderView.swift app/Tests/VizDAWAppModelTests/DocumentModelTests.swift \
  app/README.md .github/workflows/ci.yml
git commit -m "docs(app): editors checklist; typing never plays; CI times the big lane in release

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---

## Self-Review

**1. Spec coverage (phase 4a of `2026-10-16-editors-design.md`, with §3, §5, §6):**

| Spec | Task |
|---|---|
| §3.1 `DocSnapshot.lane_revs`, pointer comparison in core-ffi, core-model unchanged | 1 (Ruling 1) |
| §3.1 `lane_events` → `LaneEvents { rev, events: Vec<EventRow> }`, every field, every `EventBody` kind | 2 (Rulings 3, 4) |
| §3.1 `sequence_info` (tempo, meter, markers, start offset, length) | 2 |
| §3.1 `blocks(seq, lane, BlockRule)`: SilenceGap, FixedBars, WholeLane; reference always its own block | 3 (Ruling 5) |
| §3.1 100 000 events under 50 ms in release, generous bound in CI | 2, 16 (Ruling 21) |
| §3.2 `edit(EditCommand)` for Event, Reference, Lane, Sequence, Audio — one variant per core command; errors as `VizError` | 4 (46 variants tested), 5 (`FromSelection`) |
| §3.2 gestures via `begin_edit`/`end_edit`, one drag one undo step, intermediate edits follow the pointer | 4 (`edit_gesture`, `abort_edit`, Ruling 7), 10, 12 |
| §3.2 `audition(dest, pitch, velocity, on)` | 6 (Ruling 9) |
| §3.3 `selection()` / `set_selection`, not an undo step, shared by all editors; view filters per editor passed with commands | 5, 9 (Rulings 6, 19) |
| §3.4 `host_time_ns`, `ticks_per_second`; `TransportClock` with display link, clamped to one poll, snaps on locate/stop | 6, 8, 11 (Rulings 10, 11) |
| §3.5 `TimeAxis` (pure, tested); layers grid/content/overlay/playhead; dirty-rect content; Retina | 7, 11 (Ruling 22 defers Pitch/Value axes) |
| §3.5 ruler drag selects a range across all lanes whatever the tool | 11, 12 |
| §3.5 snap (division, triplet, dotted, enabled) toggled by one key | 7, 11 (Ruling 16) |
| §3.5 tool state machine, 3-point dead zone, Escape aborts | 11, 10, 12 |
| §3.5 auto-scroll; follow playback (page mode) | 11 (Ruling 18) |
| §3.5 accessibility container with labelled elements and actions | 11, 12 |
| §3.5 hit-testing and geometry as pure functions | 12 (`ArrangementGeometry`, `ArrangementHitTest`, `BlockDrag`) |
| §3.6 `EditorModel`: per document, `@Observable`, owned by `DocumentModel`, rev-keyed cache, dispatch, Swift-side editor state, FakeSession tests | 9, 10 |
| §3.7 window: sidebar \| arrangement over split detail; double-click opens the block in the last-used editor; tear-off windows restored with the project from the window-state file | 12, 15 (Rulings 12, 14, 15) |
| §3.8 Busy: intermediate dropped, final retried each tick until success or cancel; polling silent | 9, 10 (Ruling 8) |
| §4a arrangement: rows with header (name, mute/solo/arm, destination); blocks per rule with user-set size; reference labels; move / ⌥ copy / merge; light mode automatic above a threshold and manual | 12 (Rulings 17, 23, 24) |
| §4a sequence sidebar: name, slot, comment, use count; drag reorder; duplicate; new from template | 13 (Rulings 12, 13) |
| §4a subsequence workflow: make reference from selection, open, private copy, flatten, re-point, set params | 12 (open), 14 |
| §4a split detail pane placeholder until 4b; tear-off windows | 15 |
| §5 non-modal status for user-action failures, silent polling Busy; a drag whose target vanishes aborts; no Rust objects across the run loop | 9 (`status`, `StatusLine`), 10 (`afterSync`), all Swift reads are copied records |
| §6 tests: event rows per kind, lane revisions, blocks per rule and reference isolation, each edit variant applies and undoes, selection round trip; Swift axis maths, hit-testing, block layout, tool state machine, EditorModel cache invalidation; manual checklist | 1–5, 7, 9–12, 16 |

Out of scope here and left to later plans: detail editors and their tools (4b–4e), `PitchAxis`/`ValueAxis` (Ruling 22), Drum and SelectAndModify `EditCommand` groups (4c, 4e), key binding registry (4f, which also takes over G and ⌘1–⌘4), lane reordering by drag (Ruling 24), audition before the first Play (Ruling 9).

**2. Placeholder scan:** every step carries its code or exact Find/Replace text and its command; no "TBD", "similar to Task N" or untested names. The detail pane's "arrives in plan 4b" text is the specified 4a placeholder, not a plan gap.

**3. Type consistency:** Rust records and their Swift forms are used with UniFFI's camel-case names throughout (`laneRevs`, `refSeq`, `toLane`, `keyboardSlot`, `hostTimeNs`, `ticksPerSecond`, `EditCommand.event(edit: .move(events:delta:toLane:))`, `ReferenceEdit.fromSelection(seq:perLane:roundToBars:loopCount:)`, `BlockRule.silenceGap(ticks:)`, `SequenceTemplate.sameLanesAs(seq:)`). New record fields are appended last, matching the trailing initialiser arguments added to `FakeSession`, `FormattingTests` and `DocumentModel.stopped`. `FakeSession` gains every new `SessionProtocol` method in the task that adds it (Tasks 2–6). Names shared across tasks: `LaneKey(_:_:)`, `EditorModel.{laneEvents, blocks, info, edit, selectBlock, selectRange, beginGesture, updateGesture, endGesture, cancelGesture, tick, openDetail, openReference, back, showDetail, tearOff}`, `DocumentModel.{editor, clock, playKey, choose, locate}`, `BlockDrag.command(events:rows:from:to:delta:copy:)`, `ArrangementGeometry.row(atY:count:)` — each defined once (Tasks 9–15) and used with the same signature.

**4. Review Focus:** each of the five lines has its tests in its owning task (Tasks 1, 2, 3, 4, 7, 9, 10, 12). Other input classes checked and covered: non-finite tempo and out-of-range row values (Task 4 `bad_rows_and_values_are_model_errors_and_change_nothing`, Task 2), unknown lanes and sequences (Task 2), stale and inverted selections (Task 5), a corrupt or partial layout JSON (Task 9 `layoutDecodingIsTolerant`), destinations without an engine target (Task 6), a drag ended without movement or returned to its start (Task 10), a deleted sequence still shown in the detail pane or a torn-off window (Task 15), typing a space in any new text field (Task 16).

**Execution note:** the Rust tasks (1–6) were checked against the tree at `de68b6f` (signatures of `History`, `Session`, the command enums, `Builder`, `Engine`, `Renderer::apply`); the Swift tasks were written against the 3c app sources. The plan was not rehearsed end to end on a scratch clone; an executor meeting a compile error in a Find/Replace excerpt fixes it in the task's own files and keeps the interfaces above.
