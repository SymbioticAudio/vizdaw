# VizDAW 4b — Piano Roll Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The detail pane and torn-off editor windows show a working piano roll: notes over a pitch axis, several lanes at once with a colour per lane, the lanes' audio regions in their own strip (one row per audio output path, with mute and solo), an optional keyboard that auditions on click, cursor zones over a note (start moves, middle transposes, end resizes, ⌥ copies), a pencil that inserts at the grid length or paints a length, an eraser, notes that sound while pressed and dragged, a per-note expression pane for MIDI 2.0 per-note data, and nudge in time and pitch (pitch can follow a key and scale) with its steps on one Settings page. It also closes the items 4a's final review carried over: torn windows become command targets, audition notes can no longer stick, a media-pool revision test, weak menu closures and a Swift 6 warning.

**Architecture:** Rust first. `core-engine` keeps audition notes in their own active-note tracker so recompiles and loop wraps (every intermediate edit of a drag recompiles) no longer cut a dragged note while Stop, Locate and an engine handover still release it. `core-model` gains four event commands the piano roll needs as single undo steps and single gesture targets (`Duplicate`, `Resize`, `TransposeInScale`, `SetPerNote`); `core-ffi` mirrors them in `EventEdit` and adds a demo-with-audio session constructor. Swift then adds, in `VizDAWAppModel`, audition note bookkeeping with note-off retries, torn-window command focus, pitch and scale maths, the per-editor `PianoRollModel`, nudge settings and preferences; in `VizDAWEditors`, pure geometry (`PitchAxis`, `ValueAxis`, note rects, cursor zones, hit-testing, drag/pencil/eraser targets, region rows, expression curves) split from the AppKit views (`PianoRollView` on `EditorCanvas`, keyboard, region strip, expression pane, container); and in `VizDAW` the detail pane and torn windows host the piano roll, a Settings scene holds the nudge page, and the Editors menu gains nudge commands.

**Tech Stack:** Rust 2024 (`core-model`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `im` 15), Swift 6 / SwiftPM (SwiftUI shell, AppKit `NSView` + Core Animation layers + Core Graphics, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-16-editors-design.md` (§3 architecture, §4 phase 4b, §5 errors, §6 testing), parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` §9 (piano roll paragraph, nudge, shared editor behaviours). Previous plan for conventions: `docs/superpowers/plans/2026-10-16-editors-foundation.md`; its ledger `.superpowers/4a-ledger.md` lists the carried-over minors.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp4-editors` (checked out; code head `5c21fc0` before Task 1, plus this plan's commit). Commit at the end of every task, adding only the files the task lists; never push. End every commit message with the attribution lines your harness requires (the blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and the `Claude-Session:` line).
- Rust stable, edition 2024; crate manifests keep `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`. License GPL-3.0-only.
- `core-model` gains only the four `EventCmd` variants and `modify::scale_step` of Task 2; `core-engine` gains only the separate audition tracker (Task 1) and the `demo_audio_document` fixture (Task 3). Both stay free of OS-specific dependencies and build and test on Linux. `core-ffi` is platform-neutral except `#[cfg(target_os = "macos")]` code.
- `core-model`, `core-engine` and `core-ffi` keep `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. Tests may `unwrap` and index. No new `unsafe` anywhere.
- Everything reachable from the render callback stays `// RT`: no allocation, lock, I/O or logging. The second `ActiveNotes` is a fixed-size array built off the audio thread.
- No panic crosses the FFI: every exported function and method returns `Result<_, VizError>` and runs inside `ffi_guard`. No session `inner` lock is held across plugin code. Polling paths skip `VizError::Busy` silently; gestures follow spec §3.8 through `EditorModel+Gestures.swift` (`beginGesture` / `updateGesture` / `endGesture` / `cancelGesture`, final edit retried by `tick()`).
- New UniFFI enum variants are appended at the END of existing enums (`EventEdit`); new record types are new. No existing record gains a field in 4b.
- Swift: the generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings. Never use SwiftUI's `@State` (the CLT SDK cannot expand it); keep view state in `@Observable` models. Models and pure logic are tested (Swift Testing, `@Test`, `#expect`); AppKit views get behaviour tests through their internal entry points (`contentMouseDown(_:clicks:modifiers:)` etc., as `ArrangementViewTests` do) and the manual checklist.
- Drawing (spec §2): custom `NSView`s with `wantsLayer`, Core Animation layers, Core Graphics drawing. No Metal. Closures stored by views capture `[weak self]` (4a final review); `[unowned self]` stays only where 4a uses it for a view's own sublayer paint closures.
- `FakeSession` implements the whole generated `SessionProtocol`. 4b adds no `Session` method (only a constructor, which is not in the protocol), so `FakeSession` changes only to script audition results (Task 4).
- Generated files are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command after a Rust change.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Run `cargo fmt --all` first. Baseline before Task 1: 614 Rust tests, 227 Swift tests.
- Code in the steps is complete for new files (written whole with `cat > … <<'VIZDAW_EOF'`). Changes to existing files are given as exact **Find / Replace** excerpts of the tree the previous task left; apply them with an editor (the Edit tool). If an excerpt does not match because a fix landed meanwhile, make the same change by hand and keep going.
- Spec values used verbatim: gesture dead zone 3 points (`ToolMachine.deadZone`); one drag is one undo step; a gesture's intermediate edits that hit Busy are dropped and the final edit is retried each tick; editors never hold Rust objects across the run loop; MIDI note numbers with C-1 = 0 and middle C 60 = "C4" (core `note_name`).

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no requirement spells out, each pinned by a named test in its owning task:

1. **A plugin editor opens (Busy) while a note is dragged** — the note-off must still reach the instrument (retried until it lands) and the final edit must land too; nothing sticks. Pinned: Task 4 `aBusyNoteOffIsRetriedOnTick`, `aNoteOnWaitsForItsPendingNoteOff`; Task 9 `aNoteDragEndsItsSoundEvenWhenTheFinalEditWaits`.
2. **Dragging while the song plays** — every intermediate edit recompiles the schedule and the loop wraps: the dragged note keeps sounding until release, while Stop, Locate and another document taking the engine still silence it. Pinned: Task 1 `an_audition_note_survives_a_recompile_and_a_loop_wrap`, `locate_releases_an_audition_note`, `another_session_taking_the_engine_releases_an_audition_note`.
3. **Transposing at the edges of the pitch range** — dragging a chord up past G9 or down past C-1 stops the whole group at the edge instead of squashing it, and a scale nudge stops at the last in-range scale pitch. Pinned: Task 7 `transposeClampsTheGroupAtTheEdges`; Task 2 `scale_step_stops_at_the_range_edges`.
4. **Lanes with no destination, and no engine yet** — pressing notes or keys of a lane without a MIDI destination is silent with no error, and before the first Play the user gets one hint, not one per click. Pinned: Task 4 `noEngineShowsOneHint`; Task 8 `aLaneWithoutDestinationIsSilent`.
5. **A shown lane is deleted (undo in another window)** — an extra lane a piano roll shows disappears from it silently; the edit lane falls back to a shown one; a torn window whose target lane goes still closes (4a). Pinned: Task 8 `aShownLaneThatVanishesIsDropped`, `theEditLaneFallsBackWhenItsLaneIsHidden`.

## Plan Rulings

Decisions the spec leaves open, each implemented and tested as stated (format: Ruling: what — why — cost if wrong):

1. **Ruling:** Audition notes live in a second `ActiveNotes` in the renderer. Stop, Locate and `RemoveSink` release them; a schedule swap (`SetSchedule`, i.e. every recompile), a loop wrap and the end of the sequence do not. `host::play` stops the engine before it drops a host another document's plugins played in, as it already does for device rebuilds — why: every intermediate edit of a drag recompiles; with one tracker the dragged note was cut on the first move. The 4a ledger asked for "Locate/loop-wrap release tests": Locate releases (tested), a wrap deliberately does not (tested) because the editor owns the note-off — cost if wrong: a held audition note rings through a loop wrap until mouse-up.
2. **Ruling:** Editor-side audition bookkeeping (`EditorModel+Audition.swift`): note-ons that fail are dropped (Busy silently, others to the status line); a note-off that fails for any reason is retried by `EditorModel.tick()` (the 60 Hz plugin timer) for up to 120 ticks, about 2 s, then dropped (Stop releases anything left); a note-on for a note whose note-off is still pending is not sent. `audition` returning false (no engine before the first Play, 4a Ruling 9) shows "Notes sound once the audio engine runs: press Play once." once per document — cost if wrong: none.
3. **Ruling:** `core-model` gains `EventCmd::Duplicate { events, ticks, semitones }`, `Resize { events, delta }` (relative; a note keeps at least 1 tick), `TransposeInScale { events, steps, root, mask }` and `SetPerNote { events, kind, from, to, points }` (absolute ticks; replaces the note's values of `kind` inside `from..=to`; points outside the note are dropped), plus `modify::scale_step` — why: ⌥-copy across several lanes, resizing several notes, scale-step nudge and drawing per-note values across a chord must each be one undo step and one gesture target, and the FFI cannot batch commands (UniFFI enums cannot nest themselves); spec §3's table mentions only 4d's core addition — cost if wrong: four small commands to fold into select-and-modify in 4c.
4. **Ruling:** Cursor zones: a note's edge zones are `min(8 pt, width / 3)` wide. Start zone drags move in time only (the pressed note's start snaps to the grid); middle zone drags transpose only (whole rows); end zone drags change the duration (the pressed note's end snaps; at least 1 tick). ⌥ held when the drag begins copies (start and middle zones, `Duplicate`); ⌥ on the end zone is ignored. Notes are drawn at least 3 pt wide — why: spec "start moves, middle transposes, end resizes" without sizes; one dimension per zone keeps each drag one core command — cost if wrong: a moved note cannot change pitch in the same drag.
5. **Ruling:** The dragged set is the pressed note plus every selected note of the shown lanes when the pressed note is selected; otherwise the pressed note alone, which becomes the selection when the drag begins. A transpose is clamped so the group's lowest and highest notes stay within 0–127 — cost if wrong: none.
6. **Ruling:** Notes sound while pressed: a note pressed with the arrow or pencil sounds (its own velocity) until release; a middle-zone drag re-sounds it at each new pitch; the pencil sounds the new note's pitch on press; the keyboard sounds each key it is dragged across. The destination is the event's own destination, else its lane's MIDI destination; no destination means silence. One note sounds per piano roll at a time — cost if wrong: none.
7. **Ruling:** Pencil: a click on empty space inserts a note at the grid line at or before the pointer (`Snap.floor`), at the pointer's pitch, one grid step long (a quarter note with snap off), velocity `100 << 9` (MIDI 1.0 velocity 100), into the edit lane; dragging right paints the length to the snapped pointer (at least one grid step; with snap off at least a 32nd note). It is one undo step "Insert Note". The pencil over an existing note behaves like the arrow — cost if wrong: none.
8. **Ruling:** Eraser: a click on a note deletes it; a drag deletes every note the pointer crosses, as one undo step "Erase Notes". Arrow on empty space: a click clears the selection (Shift keeps it), a drag draws a rubber band selecting the notes of every shown lane it touches (Shift adds). Delete/Backspace in the piano roll deletes the selected notes of the shown lanes — cost if wrong: none.
9. **Ruling:** Shown lanes: a lane or block target shows its lane plus the extra lanes listed in `DetailState.lanes`; a sequence target shows, when `DetailState.lanes` is nil, every lane with a MIDI destination, MIDI events or audio regions, else exactly the listed lanes. Lanes are drawn in sequence order with a colour from an 8-colour palette by the lane's index in its sequence. The edit lane (pencil target) is the lane of the last pressed or selected note when shown, else the target's lane, else the first shown lane; other lanes are drawn at 40% opacity under it and lose hit tests to it — why: spec "several lanes at once with per-lane colour" without a selection rule — cost if wrong: a menu.
10. **Ruling:** A block target shades the block's range (when no ruler selection exists) and opens scrolled to it; notes outside the block are shown and editable. The first layout centres the shown notes vertically (C4 when there are none) — cost if wrong: none.
11. **Ruling:** Piano roll view settings are one `PianoRollSettings` in `EditorLayout` shared by every piano roll (keyboard shown, row height 10 pt in 4–40, its own zoom 96 ticks per point, expression kind, strip and pane shown); which lanes an editor shows is per editor in `DetailState.lanes` (optional, so 4a layouts decode). Both are saved with the project in `ui.json` (4a Ruling 12) — cost if wrong: none.
12. **Ruling:** A 64-pt column always sits left of the notes, the region strip and the expression pane: it holds the keyboard (or stays blank when the keyboard is hidden), the strip's row headers and the pane's kind menu, so all three share one time axis and x origin. Only C keys are labelled — cost if wrong: 64 pt of width.
13. **Ruling:** The region strip has one 22-pt row per audio output path of the shown lanes that hold regions, in lane order, plus "No Output" last for regions of lanes without one; it hides itself when there are no regions. A row header shows the output path's mixer strip name with M and S buttons that set that strip's mute and solo (`set_strip_mute` / `set_strip_solo`; refused with the usual status while a drag is open). A region click selects it; a drag moves it in time (`EventEdit.Move`, snapped, "Move Region") — why: spec "mute/solo per audio output path"; audio-path strips are silent until sub-project 5, so M/S show state but make no sound yet — cost if wrong: none.
14. **Ruling:** The expression pane (96 pt) shows one per-note kind at a time, chosen from per-note pitch bend, registered 1, 2, 3, 7, 10, 74 and assignable 1–4. It draws the edit lane's notes' curves as steps (a note's value before its first point is neutral: centre for pitch bend, 0 otherwise). Any tool but the eraser draws: a stroke writes one point per 4 pt of x into the notes it crosses (the selected ones among them when any is selected), replacing their values in the stroke's range; the eraser removes them. One stroke is one undo step ("Draw Expression" / "Erase Expression") through `SetPerNote` — why: spec names the lanes, not the editing; line shapes are 4d's — cost if wrong: none.
15. **Ruling:** Per-note data plays as the engine plays it today: MIDI 1.0 targets drop it and the compile report says so. 4b edits and saves it — why: no MIDI 2.0 output path exists yet — cost if wrong: none (spec gap below).
16. **Ruling:** Nudge settings are app preferences (UserDefaults key `VizDAW.nudge`, JSON), not project data, on one Settings page "Editing": time step = the edit grid (default), a fixed note value (bar … 1/32, optionally triplet) or a tick count; pitch step = n semitones (default 1) or n scale steps; scale steps follow the sequence's key signature (major or natural minor, default) or a fixed root and scale (major, natural minor, harmonic minor, Dorian, Mixolydian, major and minor pentatonic). Editors menu commands Nudge Earlier / Later / Up / Down on ⌥← ⌥→ ⌥↑ ⌥↓ act on the selected events (4f rebinds) — why: parent §9 "Both steps are set on one Settings page"; arrows are the parent spec's "modified arrows nudge" — cost if wrong: none.
17. **Ruling:** A scale-step nudge reads the key at the selection's range start, else at the earliest selected event of the cached selection lanes, else at tick 0; a key signature with `sharps` s has root `(7·s) mod 12` (+9 for minor). A note off the scale moves to the next scale pitch in the nudge's direction, which counts as one step — cost if wrong: none.
18. **Ruling:** Torn windows become command targets through `DocumentStore.tornFocus` (set when a torn window becomes key, cleared when it resigns or closes or its document closes), preferred by `AppCommands` over SwiftUI's focused scene values (an `NSHostingView` in an AppKit `NSWindow` publishes none). With a torn window key, ⌘1–⌘4 switch that window's editor and ⌥⌘1–⌥⌘4 tear another window off its target; Space, ⌘Z and the other document commands act on its document — cost if wrong: none.
19. **Ruling:** File > Create Demo Project with Audio opens the demo plus output path "Audio 1-2", media item `Media/loop.wav` (48 kHz stereo, 8 s; the file does not exist) and lane "Loop" in Song routed to it with 2-bar regions at bars 1 and 5 — why: no UI can add audio before sub-project 6, and the strip needs something to show; regions are model data and stay silent until sub-project 5 — cost if wrong: a menu item to remove later.
20. **Ruling:** Mouse cursors follow the zones: start zone open hand, middle zone up-down arrows, end zone right resize, pencil crosshair, eraser "disappearing item" — cost if wrong: none.
21. **Ruling:** Accessibility: every visible note is an element labelled "Note C4, bar 3 beat 1, velocity 100" (spec §3.5; "tick n" is added off the beat, the lane name when several lanes show, "muted" when muted; velocity is the 16-bit value shifted to 7 bits); VO-Space selects it. The keyboard is a group "Keyboard" — cost if wrong: none.
22. **Ruling:** One `PianoRollModel` per slot (the detail pane, each torn window), cached by `DocumentModel.pianoRoll(_:)` and dropped (silencing its note) when its torn window closes; the model holds its document `unowned` (the document owns it) — cost if wrong: none.
23. **Ruling:** New pure tests go to `VizDAWEditorsTests` (geometry) and `VizDAWAppModelTests` (models, maths, view behaviour with `FakeSession`, as in 4a Ruling 25) — cost if wrong: none.

## Spec gaps and contradictions found while planning

- §3 lists no `core-model` additions before 4d, but relative resize, multi-lane copy, scale-step transpose and per-note writes have no core command (Ruling 3).
- §4b "per-note expression lanes" — the engine drops per-note data on MIDI 1.0 targets, the only targets that exist (Ruling 15).
- §4b "mute/solo per audio output path" — audio-path strips are silent until sub-project 5, and no UI adds audio before sub-project 6 (Rulings 13, 19).
- Parent §9 "start moves, middle transposes" does not say whether a start-zone drag may also change pitch (Ruling 4).
- Parent §9 nudge keys are 4f's registry; 4b ships ⌥-arrows (Ruling 16).
- The 4a ledger's "loop-wrap release" expectation conflicts with notes sounding while dragged during playback (Ruling 1).
- §3.7 torn windows are AppKit windows; SwiftUI focused values do not reach them (Ruling 18).

## File Structure

```
crates/core-engine/
  src/render.rs               second ActiveNotes for auditions (Task 1)
  src/fixtures.rs             demo_audio_document (Task 3)
  tests/audition.rs           recompile/wrap/locate tests (Task 1)
crates/core-model/
  src/command/event.rs        Duplicate, Resize, TransposeInScale, SetPerNote (Task 2)
  src/modify.rs               scale_step (Task 2)
  tests/cmd_event.rs          tests (Task 2)
crates/core-ffi/
  src/host.rs                 stop before dropping another document's host (Task 1)
  src/events.rs               PerNoteValueRow, per_note_kind (Task 3)
  src/edit.rs                 four EventEdit variants (Task 3)
  src/session.rs              Session::new_demo_audio (Task 3)
  src/lib.rs                  re-export (Task 3)
  tests/{audition,revs,edit,events}.rs   tests (Tasks 1, 3)
app/
  Sources/VizDAWAppModel/     EditorModel+Audition (4); DocumentStore, EditorModel+Windows (5);
                              PitchMath (6); Snap.floor (7); PianoRollSettings, PianoRollModel,
                              EditorLayout, EditorTypes, EditorModel.laneRows (8); DocumentModel
                              pianoRoll slots (10); PianoRollModel outputs (11); Nudge (13)
  Sources/VizDAWEditors/      EditorCanvas, ArrangementView, LaneHeaderView fixes (4);
                              PitchAxis, ValueAxis (6); PianoRollGeometry, NoteDrag, PianoTools (7);
                              PianoRollView (9); PianoKeyboardView, PianoRollContainer (10);
                              RegionStrip, RegionStripView (11); Expression, ExpressionPaneView (12)
  Sources/VizDAW/             AppCommands, EditorWindows (5); PianoRollPane, DetailPane (10);
                              AppCommands demo-audio item (11); EditingSettingsView, VizDAWApp,
                              AppCommands nudge (13)
  Tests/VizDAWAppModelTests/  FakeSession, AuditionTests (4); TornFocusTests (5); PitchMathTests (6);
                              PianoRollModelTests (8); PianoRollViewTests (9); KeyboardTests (10);
                              RegionOutputTests (11); NudgeTests (13)
  Tests/VizDAWEditorsTests/   PitchAxisTests (6); PianoRollGeometryTests (7); RegionStripTests (11);
                              ExpressionTests (12)
  README.md                   piano roll checklist steps 36–49 (Task 14)
```

---
### Task 1: Audition notes survive recompiles; Stop, Locate and handover release them; media revisions test

**Files:**
- Modify: `crates/core-engine/src/render.rs`
- Modify: `crates/core-ffi/src/host.rs`
- Test: `crates/core-engine/tests/audition.rs`, `crates/core-ffi/tests/audition.rs`, `crates/core-ffi/tests/revs.rs`

**Interfaces:**
- Consumes: `core_engine::engine::Engine::{audition, load, play, stop, locate_tick, set_loop_range}`, `vizdaw_ffi::revs::Revisions::{new, update, rev}`, `vizdaw_ffi::host::{drain_offline_log, force_offline, reset_host}`.
- Produces: no new API. Behaviour: `EngineCmd::Audition` notes are tracked apart from played notes; `Renderer::release_now(auditions: bool)`; `host::play` stops a host before dropping it.

Ruling 1. The 4a ledger items carried here: "Locate/loop-wrap release tests", "engine handover without host::release must not leave an audition note held", "no media-change revs test".

- [ ] **Step 1: Write the failing engine tests**

Append to `crates/core-engine/tests/audition.rs`:

```rust
fn note_off(seen: &[(u64, Vec<u8>)], pitch: u8) -> bool {
    seen.iter().any(|(_, b)| b == &vec![0x82, pitch, 0])
}

#[test]
fn an_audition_note_survives_a_recompile_and_a_loop_wrap() {
    let (doc, seq, synth, _) = doc();
    let f = Factory::default();
    let (mut engine, mut r) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone())).unwrap();
    engine.load(&doc, seq).unwrap();
    engine.set_loop_range(Some((0, 1_024))).unwrap();
    engine.play().unwrap();
    let mut ns = 0;
    blocks(&mut r, 1, &mut ns);
    assert!(engine.audition(&doc, synth, 70, 0x8000, true).unwrap());
    // An edit during a drag recompiles: the schedule is swapped.
    engine.load(&doc, seq).unwrap();
    // 12 blocks of 256 frames: three wraps of the 1 024-frame loop.
    blocks(&mut r, 12, &mut ns);
    let seen = f.synth.lock().unwrap().clone();
    assert!(seen.iter().any(|(_, b)| b == &vec![0x92, 70, 64]), "{seen:?}");
    assert!(note_off(&seen, 60), "played notes are still released at the wrap: {seen:?}");
    assert!(!note_off(&seen, 70), "the dragged note keeps sounding: {seen:?}");
    engine.stop().unwrap();
    blocks(&mut r, 1, &mut ns);
    assert!(note_off(&f.synth.lock().unwrap(), 70), "stop releases it");
}

#[test]
fn locate_releases_an_audition_note() {
    let (doc, seq, synth, _) = doc();
    let f = Factory::default();
    let (mut engine, mut r) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(f.clone())).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut ns = 0;
    blocks(&mut r, 1, &mut ns);
    assert!(engine.audition(&doc, synth, 70, 0x8000, true).unwrap());
    blocks(&mut r, 1, &mut ns);
    engine.locate_tick(core_model::time::Tick(0)).unwrap();
    blocks(&mut r, 1, &mut ns);
    assert!(note_off(&f.synth.lock().unwrap(), 70));
}
```

- [ ] **Step 2: Write the failing FFI tests**

Append to `crates/core-ffi/tests/audition.rs`:

```rust
#[test]
fn another_session_taking_the_engine_releases_an_audition_note() {
    let _g = serial();
    let (a, song, piano) = demo();
    a.play(song).unwrap();
    sleep(Duration::from_millis(50));
    a.stop().unwrap();
    sleep(Duration::from_millis(50));
    drain_offline_log();
    assert!(a.audition(piano, 75, 0x8000, true).unwrap());
    sleep(Duration::from_millis(50));
    // Another document plays without `a` releasing the engine first.
    let (b, song_b, _) = demo();
    b.play(song_b).unwrap();
    sleep(Duration::from_millis(100));
    b.stop().unwrap();
    let log = drain_offline_log();
    assert!(log.iter().any(|(_, m)| m == &vec![0x90, 75, 64]), "{log:?}");
    assert!(log.iter().any(|(_, m)| m == &vec![0x80, 75, 0]), "{log:?}");
    assert!(
        !a.audition(piano, 75, 0, false).unwrap(),
        "a no longer holds the engine"
    );
    reset_host();
}
```

Append to `crates/core-ffi/tests/revs.rs`:

```rust
#[test]
fn a_media_pool_change_bumps_only_lanes_holding_regions() {
    use core_model::command::routing::RoutingCmd;
    use core_model::event::{EventBody, Fade};
    use core_model::ids::AnyId;
    use vizdaw_ffi::revs::Revisions;

    let mut b = Builder::new();
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
    let (seq, audio) = b.sequence("S", "Audio", None).unwrap();
    let notes = b.lane(seq, "Notes", None).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 960)]).unwrap();
    b.event(
        seq,
        audio,
        0,
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
    let mut revs = Revisions::new(b.doc());
    let (audio_rev, notes_rev) = (revs.rev(seq, audio).unwrap(), revs.rev(seq, notes).unwrap());
    b.run(Command::Routing(RoutingCmd::AddMedia {
        path: "Media/b.wav".into(),
        sample_rate: 44_100,
        channels: 1,
        length_frames: 10,
        content_hash: None,
    }))
    .unwrap();
    revs.update(b.doc());
    assert_ne!(
        revs.rev(seq, audio).unwrap(),
        audio_rev,
        "region durations are computed through the media pool"
    );
    assert_eq!(revs.rev(seq, notes).unwrap(), notes_rev);
}
```

- [ ] **Step 3: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test audition 2>&1 | tail -15
cargo test -p core-ffi --test audition --test revs 2>&1 | tail -15
```

Expected: `an_audition_note_survives_a_recompile_and_a_loop_wrap` FAILS ("the dragged note keeps sounding"); `locate_releases_an_audition_note`, `another_session_taking_the_engine_releases_an_audition_note` and `a_media_pool_change_bumps_only_lanes_holding_regions` already PASS (they pin behaviour that must not regress).

- [ ] **Step 4: Track audition notes apart**

In `crates/core-engine/src/render.rs`:

Find:
```rust
    /// Send a note on or off to a target now (editor audition), tracked
    /// like any played note so Stop and Locate release it.
    Audition(TargetIdx, Midi1Msg),
```
Replace:
```rust
    /// Send a note on or off to a target now (editor audition). Tracked
    /// apart from played notes: Stop, Locate and removing the target
    /// release it; a schedule swap (every edit recompiles), a loop wrap and
    /// the end do not, so a note dragged in an editor keeps sounding until
    /// the editor ends it (4b Plan Ruling 1).
    Audition(TargetIdx, Midi1Msg),
```

Find:
```rust
    transport: Transport,
    notes: ActiveNotes,
    sinks: Box<SinkTable>,
```
Replace:
```rust
    transport: Transport,
    notes: ActiveNotes,
    /// Notes sounding because an editor auditions them.
    audition: ActiveNotes,
    sinks: Box<SinkTable>,
```

Find:
```rust
            notes: ActiveNotes::default(),
            sinks: Box::default(),
```
Replace:
```rust
            notes: ActiveNotes::default(),
            audition: ActiveNotes::default(),
            sinks: Box::default(),
```

Find:
```rust
    /// Note-offs, then a controller reset on every channel of the current
    /// schedule, at block offset 0; the next played frames chase the
    /// controller state (ruling O23). // RT
    fn release_now(&mut self) {
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: self.block_ns,
            ns_per_frame: 0.0,
            late_ns,
        };
        release_all(&mut self.notes, &mut o, 0);
```
Replace:
```rust
    /// Note-offs (of audition notes too when `auditions`), then a
    /// controller reset on every channel of the current schedule, at block
    /// offset 0; the next played frames chase the controller state (ruling
    /// O23). // RT
    fn release_now(&mut self, auditions: bool) {
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: self.block_ns,
            ns_per_frame: 0.0,
            late_ns,
        };
        release_all(&mut self.notes, &mut o, 0);
        if auditions {
            release_all(&mut self.audition, &mut o, 0);
        }
```

Find:
```rust
            EngineCmd::SetSchedule(new) => {
                self.release_now();
```
Replace:
```rust
            EngineCmd::SetSchedule(new) => {
                self.release_now(false);
```

Find:
```rust
            EngineCmd::Stop => {
                self.release_now();
                self.transport.stop();
            }
            EngineCmd::Locate(frame) => {
                self.release_now();
```
Replace:
```rust
            EngineCmd::Stop => {
                self.release_now(true);
                self.transport.stop();
            }
            EngineCmd::Locate(frame) => {
                self.release_now(true);
```

Find:
```rust
                // Forget its sounding notes: nothing will play them now.
                self.notes.release(idx, |_| {});
```
Replace:
```rust
                // Forget its sounding notes: nothing will play them now.
                self.notes.release(idx, |_| {});
                self.audition.release(idx, |_| {});
```

Find:
```rust
                o.send(target, 0, msg.as_slice());
                self.notes.observe(target, msg.as_slice());
```
Replace:
```rust
                o.send(target, 0, msg.as_slice());
                self.audition.observe(target, msg.as_slice());
```

- [ ] **Step 5: Stop a host before dropping it for another document**

In `crates/core-ffi/src/host.rs`:

Find:
```rust
    if slot
        .as_ref()
        .is_some_and(|h| h.factory_owner != owner && h.plugins_in_engine)
    {
        *slot = None;
    }
```
Replace:
```rust
    if slot
        .as_ref()
        .is_some_and(|h| h.factory_owner != owner && h.plugins_in_engine)
    {
        if let Some(h) = slot.as_mut() {
            // Its played and auditioned notes end before the engine goes
            // (4b Plan Ruling 1).
            let _ = h.engine.stop();
            std::thread::sleep(DRAIN);
        }
        *slot = None;
    }
```

- [ ] **Step 6: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test audition 2>&1 | tail -8
cargo test -p core-ffi --test audition --test revs 2>&1 | tail -8
```

Expected: all PASS, including the 4a tests `an_audition_note_sounds_and_stop_releases_it` and `an_audition_note_off_ends_the_note`.

- [ ] **Step 7: Full check and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep "test result" | awk '{s+=$4} END {print s}'
```

Expected: clippy clean; 618 tests (614 + 4).

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/render.rs crates/core-ffi/src/host.rs crates/core-engine/tests/audition.rs crates/core-ffi/tests/audition.rs crates/core-ffi/tests/revs.rs
git commit -m "fix: audition notes survive recompiles and loop wraps; stop/locate/handover release them

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---

### Task 2: Core event commands for the piano roll

**Files:**
- Modify: `crates/core-model/src/command/event.rs`
- Modify: `crates/core-model/src/modify.rs`
- Test: `crates/core-model/tests/cmd_event.rs`

**Interfaces:**
- Consumes: `edit_events`, `insert_specs`-style insertion (`Document::{event, insert_event}`, `counters_mut().alloc_event()`), `check_event`, `ensure_drum_row`, `shift_pitch`.
- Produces: `EventCmd::Duplicate { events: Vec<EventId>, ticks: Tick, semitones: i8 }` ("Duplicate Events"); `EventCmd::Resize { events: Vec<EventId>, delta: Tick }` ("Resize Notes"); `EventCmd::TransposeInScale { events: Vec<EventId>, steps: i8, root: u8, mask: u16 }` ("Transpose in Scale"); `EventCmd::SetPerNote { events: Vec<EventId>, kind: PerNoteKind, from: Tick, to: Tick, points: Vec<(Tick, u32)> }` ("Set Per-Note Values"); `pub fn core_model::modify::scale_step(p: u8, steps: i32, root: u8, mask: u16) -> u8`.

Ruling 3. Semantics:
- `Duplicate` copies each event (any kind) into its own lane's take at `tick + ticks`; notes and poly pressure are transposed by `semitones`, clamped at 0 and 127. Copies are reported in `created`; drum rows are created as for inserts.
- `Resize` adds `delta` to note durations; the result is at least 1 tick; `delta == 0` changes nothing. Other kinds are untouched.
- `TransposeInScale` moves note and poly-pressure pitches by `steps` degrees of the scale (`root` 0–11, `mask` bit i = pitch class root + i); an off-scale pitch reaching the next scale pitch counts as one step; it stops at the last scale pitch inside 0–127. Root > 11 or an empty mask is an `OutOfRange` error.
- `SetPerNote` works on notes: values of `kind` whose absolute tick (`note.tick + offset`) lies in `from..=to` are removed, then each `(tick, value)` of `points` inside both `from..=to` and the note's span `tick..=tick + duration` is added at `offset = tick - note.tick`; the note's values are kept sorted by offset (stable). `from > to` is an `OutOfRange` error.

- [ ] **Step 1: Write the failing tests**

Append to `crates/core-model/tests/cmd_event.rs`:

```rust
use core_model::event::{PerNoteCtl, PerNoteKind};
use core_model::modify::scale_step;

const C_MAJOR: u16 = 0b1010_1011_0101;

fn pitch_of(d: &Document, id: u64) -> u8 {
    match body(d, id) {
        EventBody::Note { pitch, .. } => pitch,
        b => panic!("not a note: {b:?}"),
    }
}

#[test]
fn duplicate_copies_in_place_with_offsets() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 120), note(PPQ, 60)]).unwrap();
    let a = core_model::command::apply(
        &d,
        &ev(EventCmd::Duplicate {
            events: vec![EventId(0), EventId(1)],
            ticks: Tick(10),
            semitones: 12,
        }),
    )
    .unwrap();
    assert_eq!(a.report.created.len(), 2);
    let d = a.doc;
    assert_eq!(pitch_of(&d, 0), 120, "originals stay");
    assert_eq!(pitch_of(&d, 2), 127, "clamped at 127");
    assert_eq!(d.event(EventId(2)).unwrap().0.tick, Tick(10));
    assert_eq!(pitch_of(&d, 3), 72);
    assert_eq!(d.event(EventId(3)).unwrap().1.lane, LaneId(0));
}

#[test]
fn resize_is_relative_and_keeps_one_tick() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60), note(0, 62)]).unwrap();
    let d = run(
        &d,
        ev(EventCmd::Resize {
            events: vec![EventId(0), EventId(1)],
            delta: Tick(-PPQ / 2),
        }),
    )
    .unwrap();
    assert_eq!(body(&d, 0), EventBody::note(60, 100, Tick(PPQ / 2)));
    let d = run(
        &d,
        ev(EventCmd::Resize {
            events: vec![EventId(0)],
            delta: Tick(-10 * PPQ),
        }),
    )
    .unwrap();
    assert_eq!(body(&d, 0), EventBody::note(60, 100, Tick(1)));
    assert_eq!(body(&d, 1), EventBody::note(62, 100, Tick(PPQ / 2)));
}

#[test]
fn transpose_in_scale_moves_by_degrees() {
    // E4 F4 C#4 B4 C4
    let d = insert(
        &two_seqs(),
        0,
        0,
        vec![note(0, 64), note(0, 65), note(0, 61), note(0, 71), note(0, 60)],
    )
    .unwrap();
    let up = |steps: i8| {
        run(
            &d,
            ev(EventCmd::TransposeInScale {
                events: (0..5).map(EventId).collect(),
                steps,
                root: 0,
                mask: C_MAJOR,
            }),
        )
        .unwrap()
    };
    let d1 = up(1);
    let got: Vec<u8> = (0..5).map(|i| pitch_of(&d1, i)).collect();
    assert_eq!(got, vec![65, 67, 62, 72, 62]);
    let d2 = up(-1);
    let got: Vec<u8> = (0..5).map(|i| pitch_of(&d2, i)).collect();
    assert_eq!(got, vec![62, 64, 60, 69, 59]);
    for bad in [
        EventCmd::TransposeInScale {
            events: vec![EventId(0)],
            steps: 1,
            root: 12,
            mask: C_MAJOR,
        },
        EventCmd::TransposeInScale {
            events: vec![EventId(0)],
            steps: 1,
            root: 0,
            mask: 0,
        },
    ] {
        assert!(matches!(
            run(&d, ev(bad)),
            Err(ModelError::OutOfRange { .. })
        ));
    }
}

#[test]
fn scale_step_stops_at_the_range_edges() {
    assert_eq!(scale_step(127, 1, 0, C_MAJOR), 127, "G9 is the top");
    assert_eq!(scale_step(126, 1, 0, C_MAJOR), 127);
    assert_eq!(scale_step(0, -1, 0, C_MAJOR), 0);
    assert_eq!(scale_step(60, 7, 0, C_MAJOR), 72, "an octave");
    assert_eq!(scale_step(60, 0, 0, C_MAJOR), 60);
    assert_eq!(scale_step(61, 0, 0, C_MAJOR), 61, "zero steps keep an off-scale pitch");
}

#[test]
fn set_per_note_replaces_values_in_the_range() {
    let d = insert(&two_seqs(), 0, 0, vec![note(PPQ, 60)]).unwrap();
    let pb = PerNoteKind::PitchBend;
    let set = |d: &Document, from: i64, to: i64, points: Vec<(i64, u32)>| {
        run(
            d,
            ev(EventCmd::SetPerNote {
                events: vec![EventId(0)],
                kind: pb,
                from: Tick(from),
                to: Tick(to),
                points: points.into_iter().map(|(t, v)| (Tick(t), v)).collect(),
            }),
        )
        .unwrap()
    };
    // Absolute ticks; the point before the note and the one after it are dropped.
    let d = set(
        &d,
        0,
        3 * PPQ,
        vec![(0, 1), (PPQ, 2), (PPQ + 100, 3), (2 * PPQ, 4), (2 * PPQ + 1, 5)],
    );
    let values = |d: &Document| match body(d, 0) {
        EventBody::Note { per_note, .. } => per_note
            .iter()
            .map(|p: &PerNoteCtl| (p.offset.0, p.value))
            .collect::<Vec<_>>(),
        b => panic!("{b:?}"),
    };
    assert_eq!(values(&d), vec![(0, 2), (100, 3), (PPQ, 4)]);
    // Erase the middle one only.
    let d = set(&d, PPQ + 50, PPQ + 150, vec![]);
    assert_eq!(values(&d), vec![(0, 2), (PPQ, 4)]);
    // Another kind is untouched by a pitch-bend write.
    let d = run(
        &d,
        ev(EventCmd::SetPerNote {
            events: vec![EventId(0)],
            kind: PerNoteKind::Registered(1),
            from: Tick(0),
            to: Tick(10 * PPQ),
            points: vec![(Tick(PPQ + 10), 9)],
        }),
    )
    .unwrap();
    assert_eq!(values(&d), vec![(0, 2), (10, 9), (PPQ, 4)]);
    assert!(matches!(
        run(
            &d,
            ev(EventCmd::SetPerNote {
                events: vec![EventId(0)],
                kind: pb,
                from: Tick(5),
                to: Tick(4),
                points: vec![],
            })
        ),
        Err(ModelError::OutOfRange { .. })
    ));
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_event 2>&1 | tail -15
```

Expected: compile errors (`no variant named Duplicate`, `scale_step` not found).

- [ ] **Step 3: Add `scale_step`**

In `crates/core-model/src/modify.rs`:

Find:
```rust
fn quantize_tick(t: Tick, q: &Quantize) -> Result<Tick> {
```
Replace:
```rust
/// `p` moved `steps` degrees of a scale (bit `i` of `mask` enables pitch
/// class `root + i`): an off-scale pitch reaching the next scale pitch in
/// the step's direction counts as one step; it stops at the last scale
/// pitch inside 0–127. An empty mask or zero steps leave `p` unchanged.
pub fn scale_step(p: u8, steps: i32, root: u8, mask: u16) -> u8 {
    if mask & 0x0fff == 0 || steps == 0 {
        return p;
    }
    let in_scale = |q: i32| {
        let pc = (q - i32::from(root % 12)).rem_euclid(12);
        mask & (1 << pc) != 0
    };
    let dir = steps.signum();
    let mut q = i32::from(p);
    for _ in 0..steps.unsigned_abs() {
        match (1..=12).map(|d| q + dir * d).find(|c| in_scale(*c)) {
            Some(c) if (0..=127).contains(&c) => q = c,
            _ => break,
        }
    }
    shift_pitch(0, q)
}

fn quantize_tick(t: Tick, q: &Quantize) -> Result<Tick> {
```

- [ ] **Step 4: Add the four commands**

In `crates/core-model/src/command/event.rs`:

Find:
```rust
use crate::event::{Event, EventBody, EventSpec};
```
Replace:
```rust
use crate::error::ModelError;
use crate::event::{Event, EventBody, EventSpec, PerNoteCtl, PerNoteKind};
use crate::modify::scale_step;
```

Find:
```rust
    /// Nudge in time and pitch (pitch clamps at 0 and 127).
    Nudge {
        /// The events.
        events: Vec<EventId>,
        /// Time offset.
        ticks: Tick,
        /// Pitch offset.
        semitones: i8,
    },
}
```
Replace:
```rust
    /// Nudge in time and pitch (pitch clamps at 0 and 127).
    Nudge {
        /// The events.
        events: Vec<EventId>,
        /// Time offset.
        ticks: Tick,
        /// Pitch offset.
        semitones: i8,
    },
    /// Copy events into their own lanes, moved by `ticks`; notes and poly
    /// pressure transposed by `semitones` (clamped at 0 and 127).
    Duplicate {
        /// The events.
        events: Vec<EventId>,
        /// Time offset of the copies.
        ticks: Tick,
        /// Pitch offset of the copies.
        semitones: i8,
    },
    /// Lengthen or shorten notes by `delta`; a note keeps at least 1 tick.
    Resize {
        /// The events.
        events: Vec<EventId>,
        /// Duration change.
        delta: Tick,
    },
    /// Move note and poly-pressure pitches `steps` degrees of a scale.
    TransposeInScale {
        /// The events.
        events: Vec<EventId>,
        /// Scale degrees (negative: down).
        steps: i8,
        /// Root pitch class 0–11.
        root: u8,
        /// 12-bit scale mask (bit `i`: pitch class `root + i`).
        mask: u16,
    },
    /// Replace the per-note values of `kind` whose absolute tick lies in
    /// `from..=to` with the `points` (absolute ticks) inside the note.
    SetPerNote {
        /// The notes.
        events: Vec<EventId>,
        /// Which per-note controller.
        kind: PerNoteKind,
        /// First tick replaced.
        from: Tick,
        /// Last tick replaced.
        to: Tick,
        /// `(tick, value)` pairs.
        points: Vec<(Tick, u32)>,
    },
}
```

Find:
```rust
            EventCmd::Nudge { .. } => "Nudge Events",
        }
```
Replace:
```rust
            EventCmd::Nudge { .. } => "Nudge Events",
            EventCmd::Duplicate { .. } => "Duplicate Events",
            EventCmd::Resize { .. } => "Resize Notes",
            EventCmd::TransposeInScale { .. } => "Transpose in Scale",
            EventCmd::SetPerNote { .. } => "Set Per-Note Values",
        }
```

Find (the end of `apply`):
```rust
                EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                    *pitch = shift_pitch(*pitch, i32::from(*semitones))
                }
                _ => {}
            }
            Ok(())
        })?,
    }
    Ok(())
}
```
Replace:
```rust
                EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                    *pitch = shift_pitch(*pitch, i32::from(*semitones))
                }
                _ => {}
            }
            Ok(())
        })?,
        EventCmd::Duplicate {
            events,
            ticks,
            semitones,
        } => {
            let unique: BTreeSet<EventId> = events.iter().copied().collect();
            for id in unique {
                let (ev, loc) = d.event(id)?;
                let mut spec = ev.spec();
                spec.tick = spec.tick + *ticks;
                match &mut spec.body {
                    EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                        *pitch = shift_pitch(*pitch, i32::from(*semitones))
                    }
                    _ => {}
                }
                check_event(d, loc.seq, &spec)?;
                let new_id = d.counters_mut().alloc_event();
                d.insert_event(loc.seq, loc.lane, Some(loc.take), spec.with_id(new_id))?;
                r.created.push(AnyId::Event(new_id));
                ensure_drum_row(d, new_id, r)?;
            }
        }
        EventCmd::Resize { events, delta } => edit_events(d, events, r, |e| {
            if let EventBody::Note { duration, .. } = &mut e.body {
                if delta.0 != 0 {
                    *duration = Tick(duration.0.saturating_add(delta.0).max(1));
                }
            }
            Ok(())
        })?,
        EventCmd::TransposeInScale {
            events,
            steps,
            root,
            mask,
        } => {
            if *root > 11 {
                return Err(ModelError::range("scale.root", root));
            }
            if mask & 0x0fff == 0 {
                return Err(ModelError::range("scale.mask", mask));
            }
            edit_events(d, events, r, |e| {
                match &mut e.body {
                    EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                        *pitch = scale_step(*pitch, i32::from(*steps), *root, *mask)
                    }
                    _ => {}
                }
                Ok(())
            })?
        }
        EventCmd::SetPerNote {
            events,
            kind,
            from,
            to,
            points,
        } => {
            if from > to {
                return Err(ModelError::range(
                    "per_note.range",
                    format!("{}..={}", from.0, to.0),
                ));
            }
            edit_events(d, events, r, |e| {
                let start = e.tick;
                if let EventBody::Note {
                    duration, per_note, ..
                } = &mut e.body
                {
                    let end = start + *duration;
                    let mut kept: Vec<PerNoteCtl> = per_note
                        .iter()
                        .filter(|p| {
                            let at = start + p.offset;
                            p.kind != *kind || at < *from || at > *to
                        })
                        .copied()
                        .collect();
                    kept.extend(
                        points
                            .iter()
                            .filter(|(t, _)| *t >= *from && *t <= *to && *t >= start && *t <= end)
                            .map(|(t, v)| PerNoteCtl {
                                kind: *kind,
                                offset: *t - start,
                                value: *v,
                            }),
                    );
                    kept.sort_by_key(|p| p.offset);
                    *per_note = kept.into_iter().collect();
                }
                Ok(())
            })?
        }
    }
    Ok(())
}
```

`modify.rs` already imports `shift_pitch` from this module; the reverse import of `scale_step` is fine (sibling modules of one crate).

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_event 2>&1 | tail -8
```

Expected: PASS (5 new tests). If `core-ffi` fails to build because `edit.rs` matches `EventCmd` exhaustively somewhere, it does not: `event_cmd` builds `EventCmd` values and never matches on them.

- [ ] **Step 6: Full check and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep "test result" | awk '{s+=$4} END {print s}'
git add crates/core-model/src/command/event.rs crates/core-model/src/modify.rs crates/core-model/tests/cmd_event.rs
git commit -m "feat(core): duplicate, resize, scale-step transpose and per-note event commands

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 623 tests.

---

### Task 3: FFI mirrors and a demo with audio

**Files:**
- Modify: `crates/core-ffi/src/events.rs`, `crates/core-ffi/src/edit.rs`, `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/lib.rs`
- Modify: `crates/core-engine/src/fixtures.rs`
- Test: `crates/core-ffi/tests/edit.rs`, `crates/core-ffi/tests/events.rs`

**Interfaces:**
- Consumes: Task 2's `EventCmd` variants.
- Produces: `vizdaw_ffi::PerNoteValueRow { tick: i64, value: u32 }`; `events::per_note_kind(PerNoteKindRow) -> PerNoteKind` (crate); `EventEdit::Duplicate { events: Vec<u64>, ticks: i64, semitones: i8 }`, `EventEdit::Resize { events: Vec<u64>, delta: i64 }`, `EventEdit::TransposeInScale { events: Vec<u64>, steps: i8, root: u8, mask: u16 }`, `EventEdit::SetPerNote { events: Vec<u64>, kind: PerNoteKindRow, from: i64, to: i64, points: Vec<PerNoteValueRow> }` (appended); `core_engine::fixtures::demo_audio_document() -> Result<Document>`; `Session::new_demo_audio()` (UniFFI constructor). Swift: `EventEdit.duplicate(events:ticks:semitones:)`, `.resize(events:delta:)`, `.transposeInScale(events:steps:root:mask:)`, `.setPerNote(events:kind:from:to:points:)`, `PerNoteValueRow(tick:value:)`, `Session.newDemoAudio()`.

Rulings 3, 19.

- [ ] **Step 1: Write the failing tests**

In `crates/core-ffi/tests/edit.rs`:

Find:
```rust
    AudioEdit, EditCommand, EventEdit, EventKindRow, EventRow, FadeRow, FadeShapeRow, GeneratorRow,
    GroupChannel, KeySlotRow, LaneEdit, LaneRef, MarkerRow, MeterEntryRow, QuantizeRow,
    ReferenceEdit, SequenceEdit, Session, TempoPointRow, VizError,
```
Replace:
```rust
    AudioEdit, EditCommand, EventEdit, EventKindRow, EventRow, FadeRow, FadeShapeRow, GeneratorRow,
    GroupChannel, KeySlotRow, LaneEdit, LaneRef, MarkerRow, MeterEntryRow, PerNoteKindRow,
    PerNotePoint, PerNoteValueRow, QuantizeRow, ReferenceEdit, SequenceEdit, Session,
    TempoPointRow, VizError,
```

Find:
```rust
        ev(EventEdit::Nudge {
            events: one(f.note),
            ticks: 10,
            semitones: 2,
        }),
```
Replace:
```rust
        ev(EventEdit::Nudge {
            events: one(f.note),
            ticks: 10,
            semitones: 2,
        }),
        ev(EventEdit::Duplicate {
            events: one(f.note),
            ticks: Q,
            semitones: 3,
        }),
        ev(EventEdit::Resize {
            events: one(f.note),
            delta: Q / 2,
        }),
        ev(EventEdit::TransposeInScale {
            events: one(f.note),
            steps: 2,
            root: 0,
            mask: 0b1010_1011_0101,
        }),
        ev(EventEdit::SetPerNote {
            events: one(f.note),
            kind: PerNoteKindRow::PitchBend,
            from: 0,
            to: Q,
            points: vec![PerNoteValueRow {
                tick: Q / 2,
                value: 7,
            }],
        }),
```

Find:
```rust
    assert_eq!(
        all.len(),
        46,
        "every 4a variant (Move twice) but FromSelection (Task 5)"
    );
```
Replace:
```rust
    assert_eq!(
        all.len(),
        50,
        "every variant (Move twice) but FromSelection, with 4b's four"
    );
```

Append to `crates/core-ffi/tests/edit.rs`:

```rust
#[test]
fn per_note_values_read_back_as_offsets() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    s.edit(
        ev(EventEdit::SetPerNote {
            events: vec![f.note.0],
            kind: PerNoteKindRow::Registered { index: 74 },
            from: 0,
            to: Q,
            points: vec![PerNoteValueRow {
                tick: Q / 4,
                value: 1 << 31,
            }],
        }),
        None,
    )
    .unwrap();
    let rows = s.lane_events(f.s.0, f.l1.0).unwrap().events;
    let note = rows.iter().find(|r| r.id == f.note.0).unwrap();
    assert_eq!(
        note.per_note,
        vec![PerNotePoint {
            kind: PerNoteKindRow::Registered { index: 74 },
            offset: Q / 4,
            value: 1 << 31,
        }]
    );
}
```

Append to `crates/core-ffi/tests/events.rs` (add `use vizdaw_ffi::{EventKindRow, StripKindRow};` at the top if those names are not imported yet):

```rust
#[test]
fn the_demo_with_audio_has_a_loop_lane_with_two_regions() {
    const BAR: i64 = 4 * 15_360;
    let s = vizdaw_ffi::Session::new_demo_audio().unwrap();
    let snap = s.snapshot().unwrap();
    let song = snap.sequences.iter().find(|r| r.name == "Song").unwrap().id;
    let lane = s
        .lanes(song)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Loop")
        .unwrap();
    let output = lane.audio_out.expect("routed to an output path");
    let regions: Vec<(i64, i64)> = s
        .lane_events(song, lane.id)
        .unwrap()
        .events
        .iter()
        .filter(|r| r.kind == EventKindRow::AudioRegion)
        .map(|r| (r.tick, r.duration))
        .collect();
    assert_eq!(regions, vec![(0, 2 * BAR), (4 * BAR, 2 * BAR)]);
    let strips = s.mixer().unwrap().strips;
    assert!(strips.iter().any(
        |st| matches!(st.kind, StripKindRow::AudioPath { path } if path == output)
            && st.name == "Audio 1-2"
    ));
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test edit --test events 2>&1 | tail -15
```

Expected: compile errors (`PerNoteValueRow`, `EventEdit::Duplicate`, `new_demo_audio` unknown).

- [ ] **Step 3: The fixture**

In `crates/core-engine/src/fixtures.rs`:

Find:
```rust
use core_model::event::{EventBody, EventSpec, LoopCount};
```
Replace:
```rust
use core_model::event::{EventBody, EventSpec, Fade, LoopCount};
```

Append at the end of the file:

```rust
/// The demo plus an audio lane (4b Plan Ruling 19): output path "Audio
/// 1-2", media item `Media/loop.wav` (48 kHz stereo, 8 s; the file does not
/// exist) and lane "Loop" in Song routed to it, with 2-bar regions at bars
/// 1 and 5 (both inside Song's 120 BPM part).
pub fn demo_audio_document() -> Result<Document> {
    let mut b = Builder::from_doc(demo_document()?);
    let r = b.run(Command::Routing(RoutingCmd::AddOutput {
        name: "Audio 1-2".into(),
        reference: "hw:1-2".into(),
    }))?;
    let output = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .ok_or_else(|| ModelError::invalid("no output created"))?;
    let r = b.run(Command::Routing(RoutingCmd::AddMedia {
        path: "Media/loop.wav".into(),
        sample_rate: 48_000,
        channels: 2,
        length_frames: 384_000,
        content_hash: None,
    }))?;
    let media = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .ok_or_else(|| ModelError::invalid("no media created"))?;
    let song = b
        .doc()
        .sequences()
        .values()
        .find(|s| s.name == "Song")
        .map(|s| s.id)
        .ok_or_else(|| ModelError::invalid("no Song"))?;
    let lane = b.lane(song, "Loop", None)?;
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq: song,
        lane,
        output: Some(output),
    }))?;
    for bar in [0, 4] {
        b.event(
            song,
            lane,
            bar * BAR,
            EventBody::AudioRegion {
                media,
                source_offset: 0,
                // 4 s at 48 kHz: two bars at 120 BPM.
                length: 192_000,
                gain_db: 0.0,
                fade_in: Fade::default(),
                fade_out: Fade::default(),
            },
        )?;
    }
    Ok(b.finish())
}
```

- [ ] **Step 4: Rows and the edit mapping**

In `crates/core-ffi/src/events.rs`:

Find:
```rust
/// One event as a flat record; fields a kind does not use are zero,
```
Replace:
```rust
/// A per-note value at an absolute tick, as editors write them
/// (`EventEdit::SetPerNote`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct PerNoteValueRow {
    /// Absolute tick.
    pub tick: i64,
    /// 32-bit value.
    pub value: u32,
}

/// One event as a flat record; fields a kind does not use are zero,
```

Find:
```rust
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
```
Replace:
```rust
/// The core per-note kind of a row kind.
pub(crate) fn per_note_kind(k: PerNoteKindRow) -> PerNoteKind {
    match k {
        PerNoteKindRow::Registered { index } => PerNoteKind::Registered(index),
        PerNoteKindRow::Assignable { index } => PerNoteKind::Assignable(index),
        PerNoteKindRow::PitchBend => PerNoteKind::PitchBend,
    }
}

fn per_note_of(p: &PerNotePoint) -> PerNoteCtl {
    PerNoteCtl {
        kind: per_note_kind(p.kind),
        offset: Tick(p.offset),
        value: p.value,
    }
}
```

In `crates/core-ffi/src/edit.rs`:

Find:
```rust
use crate::events::{
    EventKindRow, EventRow, FadeRow, GroupChannel, MarkerRow, MeterEntryRow, TempoPointRow, bad,
    fade_of, kind_of, loops, spec,
};
```
Replace:
```rust
use crate::events::{
    EventKindRow, EventRow, FadeRow, GroupChannel, MarkerRow, MeterEntryRow, PerNoteKindRow,
    PerNoteValueRow, TempoPointRow, bad, fade_of, kind_of, loops, per_note_kind, spec,
};
```

Find:
```rust
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
```
Replace:
```rust
    /// Nudge in time and pitch.
    Nudge {
        /// The events.
        events: Vec<u64>,
        /// Time offset.
        ticks: i64,
        /// Pitch offset.
        semitones: i8,
    },
    /// Copy events in their lanes, moved and transposed (4b).
    Duplicate {
        /// The events.
        events: Vec<u64>,
        /// Time offset of the copies.
        ticks: i64,
        /// Pitch offset of the copies.
        semitones: i8,
    },
    /// Lengthen or shorten notes; each keeps at least 1 tick (4b).
    Resize {
        /// The events.
        events: Vec<u64>,
        /// Duration change.
        delta: i64,
    },
    /// Move pitches by scale degrees (4b).
    TransposeInScale {
        /// The events.
        events: Vec<u64>,
        /// Degrees (negative: down).
        steps: i8,
        /// Root pitch class 0–11.
        root: u8,
        /// 12-bit scale mask.
        mask: u16,
    },
    /// Replace one per-note kind's values in `from..=to` (4b).
    SetPerNote {
        /// The notes.
        events: Vec<u64>,
        /// Which per-note controller.
        kind: PerNoteKindRow,
        /// First tick replaced.
        from: i64,
        /// Last tick replaced.
        to: i64,
        /// The new values (absolute ticks).
        points: Vec<PerNoteValueRow>,
    },
}
```

Find:
```rust
        } => EventCmd::Nudge {
            events: ids(events),
            ticks: Tick(*ticks),
            semitones: *semitones,
        },
    })
}
```
Replace:
```rust
        } => EventCmd::Nudge {
            events: ids(events),
            ticks: Tick(*ticks),
            semitones: *semitones,
        },
        EventEdit::Duplicate {
            events,
            ticks,
            semitones,
        } => EventCmd::Duplicate {
            events: ids(events),
            ticks: Tick(*ticks),
            semitones: *semitones,
        },
        EventEdit::Resize { events, delta } => EventCmd::Resize {
            events: ids(events),
            delta: Tick(*delta),
        },
        EventEdit::TransposeInScale {
            events,
            steps,
            root,
            mask,
        } => EventCmd::TransposeInScale {
            events: ids(events),
            steps: *steps,
            root: *root,
            mask: *mask,
        },
        EventEdit::SetPerNote {
            events,
            kind,
            from,
            to,
            points,
        } => EventCmd::SetPerNote {
            events: ids(events),
            kind: per_note_kind(*kind),
            from: Tick(*from),
            to: Tick(*to),
            points: points.iter().map(|p| (Tick(p.tick), p.value)).collect(),
        },
    })
}
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
    MeterEntryRow, NoteAttributeRow, PerNoteKindRow, PerNotePoint, SequenceInfo, TempoPointRow,
    TextKindRow,
```
Replace:
```rust
    MeterEntryRow, NoteAttributeRow, PerNoteKindRow, PerNotePoint, PerNoteValueRow, SequenceInfo,
    TempoPointRow, TextKindRow,
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// An untitled copy of the demo project.
    #[uniffi::constructor]
    pub fn new_demo() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let doc = core_engine::fixtures::demo_document()?;
            Session::with(doc, None, false)
        })
    }
```
Replace:
```rust
    /// An untitled copy of the demo project.
    #[uniffi::constructor]
    pub fn new_demo() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let doc = core_engine::fixtures::demo_document()?;
            Session::with(doc, None, false)
        })
    }

    /// An untitled copy of the demo project with an audio lane (4b Plan
    /// Ruling 19).
    #[uniffi::constructor]
    pub fn new_demo_audio() -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let doc = core_engine::fixtures::demo_audio_document()?;
            Session::with(doc, None, false)
        })
    }
```

- [ ] **Step 5: Run the tests, regenerate the bindings**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test edit --test events 2>&1 | tail -8
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error|warning: .*VizDAW(AppModel|Editors|/)" | head
```

Expected: PASS; the Swift build is clean (no Swift code switches over `EventEdit`).

- [ ] **Step 6: Full check and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep "test result" | awk '{s+=$4} END {print s}'
scripts/swift.sh test 2>&1 | tail -2
git add crates/core-ffi/src/events.rs crates/core-ffi/src/edit.rs crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs crates/core-engine/src/fixtures.rs crates/core-ffi/tests/edit.rs crates/core-ffi/tests/events.rs
git commit -m "feat(ffi): piano-roll event edits and a demo project with audio

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 625 Rust tests; 227 Swift tests.

---
### Task 4: Audition bookkeeping with note-off retries; weak menu closures; the Swift 6 warning

**Files:**
- Create: `app/Sources/VizDAWAppModel/EditorModel+Audition.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`, `app/Sources/VizDAWAppModel/EditorModel+Gestures.swift`
- Modify: `app/Sources/VizDAWEditors/EditorCanvas.swift`, `app/Sources/VizDAWEditors/ArrangementView.swift`, `app/Sources/VizDAWEditors/LaneHeaderView.swift`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `app/Tests/VizDAWAppModelTests/AuditionTests.swift` (new)

**Interfaces:**
- Consumes: `SessionProtocol.audition(dest:pitch:velocity:on:) throws -> Bool`, `EditorModel.{status, report(_:), tick()}`.
- Produces: `public struct AuditionNote: Hashable, Sendable { dest: UInt64; pitch: UInt8; init(dest:pitch:) }`; `EditorModel.auditionOn(_ note: AuditionNote, velocity: UInt16)`, `auditionOff(_ note: AuditionNote)`, `auditionOffAll()`, `hasPendingNoteOffs: Bool`, `static noteOffRetries = 120`, `static auditionHint: String`; `EditorModel.tick()` also retries note-offs. FakeSession: `auditionErrors: [VizError?]`, `auditionSent: Bool`.

Ruling 2. The 4a ledger item "Busy note-off leaves audition note held (editor should retry note-off on Busy)" lands here; the MenuAction closures and the `CanvasElement` warning (EditorCanvas.swift:60-61, "main actor-isolated property 'press' can not be referenced from a nonisolated context") are the 4a final review's.

- [ ] **Step 1: Script audition in FakeSession**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
    var auditions: [(dest: UInt64, pitch: UInt8, on: Bool)] = []
    func audition(dest: UInt64, pitch: UInt8, velocity: UInt16, on: Bool) throws -> Bool {
        auditions.append((dest, pitch, on))
        return true
    }
```
Replace:
```swift
    var auditions: [(dest: UInt64, pitch: UInt8, on: Bool)] = []
    /// What the next `audition` calls throw, oldest first (nil succeeds).
    var auditionErrors: [VizError?] = []
    /// What a successful `audition` returns (false: no engine yet).
    var auditionSent = true
    func audition(dest: UInt64, pitch: UInt8, velocity: UInt16, on: Bool) throws -> Bool {
        let e = auditionErrors.isEmpty ? nil : auditionErrors.removeFirst()
        if let e { throw e }
        auditions.append((dest, pitch, on))
        return auditionSent
    }
```

- [ ] **Step 2: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/AuditionTests.swift <<'VIZDAW_EOF'
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let c4 = AuditionNote(dest: 5, pitch: 60)
private let d4 = AuditionNote(dest: 5, pitch: 62)

@MainActor
@Suite struct AuditionTests {
    func make() throws -> (FakeSession, EditorModel) {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        return (f, m.editor)
    }

    func log(_ f: FakeSession) -> [String] {
        f.auditions.map { "\($0.on ? "on" : "off") \($0.pitch)" }
    }

    @Test func aNoteSoundsAndEnds() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        e.auditionOff(c4)
        e.auditionOff(c4)
        #expect(log(f) == ["on 60", "off 60"], "a second off for the same note sends nothing")
        #expect(e.status == nil)
    }

    @Test func aBusyNoteOffIsRetriedOnTick() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = [.Busy(message: "busy"), .Busy(message: "busy")]
        e.auditionOff(c4)
        #expect(e.hasPendingNoteOffs)
        e.tick()
        #expect(e.hasPendingNoteOffs, "still busy")
        e.tick()
        #expect(!e.hasPendingNoteOffs)
        #expect(log(f) == ["on 60", "off 60"])
        #expect(e.status == nil, "Busy is silent")
    }

    @Test func thePluginTimerRetriesNoteOffs() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = [.Busy(message: "busy")]
        m.editor.auditionOff(c4)
        m.idle()
        #expect(log(f) == ["on 60", "off 60"])
    }

    @Test func aNoteOffThatKeepsFailingIsDroppedAfterTheRetries() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = Array(repeating: .Engine(message: "queue full"), count: EditorModel.noteOffRetries + 5)
        e.auditionOff(c4)
        for _ in 0..<EditorModel.noteOffRetries { e.tick() }
        #expect(!e.hasPendingNoteOffs)
        #expect(log(f) == ["on 60"])
    }

    @Test func aNoteOnWaitsForItsPendingNoteOff() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        f.auditionErrors = [.Busy(message: "busy"), .Busy(message: "busy")]
        e.auditionOff(c4)
        // The retry inside auditionOn meets Busy again: the new note-on for
        // the same key is not sent (it would be ended by the late note-off).
        e.auditionOn(c4, velocity: 0x8000)
        #expect(log(f) == ["on 60"])
        e.auditionOn(c4, velocity: 0x8000)
        #expect(log(f) == ["on 60", "off 60", "on 60"])
    }

    @Test func busyNoteOnIsDroppedSilently() throws {
        let (f, e) = try make()
        f.auditionErrors = [.Busy(message: "busy")]
        e.auditionOn(c4, velocity: 0x8000)
        e.auditionOff(c4)
        #expect(f.auditions.isEmpty, "never started, so never ended")
        #expect(e.status == nil)
    }

    @Test func noEngineShowsOneHint() throws {
        let (f, e) = try make()
        f.auditionSent = false
        e.auditionOn(c4, velocity: 0x8000)
        #expect(e.status == EditorModel.auditionHint)
        e.clearStatus()
        e.auditionOn(d4, velocity: 0x8000)
        #expect(e.status == nil, "once per document")
        e.auditionOff(d4)
        #expect(log(f) == ["on 60", "on 62"], "nothing held, nothing to end")
    }

    @Test func offAllEndsEverySoundingNote() throws {
        let (f, e) = try make()
        e.auditionOn(c4, velocity: 0x8000)
        e.auditionOn(d4, velocity: 0x8000)
        e.auditionOffAll()
        #expect(Set(log(f).suffix(2)) == ["off 60", "off 62"])
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter AuditionTests 2>&1 | tail -5
```

Expected: compile errors (`AuditionNote` unknown).

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWAppModel/EditorModel.swift`:

Find:
```swift
    @ObservationIgnored public private(set) var eventFetches = 0
```
Replace:
```swift
    /// Audition notes sounding now, note-offs waiting for a retry with the
    /// attempts left, and whether the no-engine hint was shown (Plan 4b
    /// Ruling 2).
    @ObservationIgnored var heldAuditions: Set<AuditionNote> = []
    @ObservationIgnored var pendingOffs: [AuditionNote: Int] = [:]
    @ObservationIgnored var auditionHintShown = false

    @ObservationIgnored public private(set) var eventFetches = 0
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/EditorModel+Audition.swift <<'VIZDAW_EOF'
import VizDAWCore

/// A note an editor sends straight to a destination (spec §3.2).
public struct AuditionNote: Hashable, Sendable {
    public let dest: UInt64
    public let pitch: UInt8

    public init(dest: UInt64, pitch: UInt8) {
        self.dest = dest
        self.pitch = pitch
    }
}

/// Audition notes (Plan 4b Ruling 2): a failed note-on is dropped; a
/// failed note-off is retried by `tick()` so no note sticks.
extension EditorModel {
    /// Ticks of the 60 Hz plugin timer (about 2 s) a note-off is retried for.
    public static let noteOffRetries = 120
    public static let auditionHint = "Notes sound once the audio engine runs: press Play once."

    /// Sounds `note`. Note-offs still pending are retried first; while the
    /// one for this very note cannot be sent, the note is not started.
    public func auditionOn(_ note: AuditionNote, velocity: UInt16) {
        retryNoteOffs()
        guard pendingOffs[note] == nil else { return }
        do {
            let sent = try session.audition(dest: note.dest, pitch: note.pitch, velocity: velocity, on: true)
            if sent {
                heldAuditions.insert(note)
            } else if !auditionHintShown {
                auditionHintShown = true
                status = Self.auditionHint
            }
        } catch VizError.Busy {
            return
        } catch {
            report(error)
        }
    }

    /// Ends `note` if it sounds; a failed note-off is retried by `tick()`.
    public func auditionOff(_ note: AuditionNote) {
        guard heldAuditions.remove(note) != nil else { return }
        sendOff(note, attemptsLeft: Self.noteOffRetries)
    }

    /// Ends every audition note this document sounds.
    public func auditionOffAll() {
        for note in heldAuditions { auditionOff(note) }
    }

    public var hasPendingNoteOffs: Bool { !pendingOffs.isEmpty }

    /// One retry of every pending note-off.
    func retryNoteOffs() {
        for (note, left) in pendingOffs {
            pendingOffs[note] = nil
            sendOff(note, attemptsLeft: left - 1)
        }
    }

    private func sendOff(_ note: AuditionNote, attemptsLeft: Int) {
        do {
            _ = try session.audition(dest: note.dest, pitch: note.pitch, velocity: 0, on: false)
        } catch {
            if attemptsLeft > 0 { pendingOffs[note] = attemptsLeft }
        }
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/EditorModel+Gestures.swift`:

Find:
```swift
    /// Retries a mouse-up that met Busy (the 60 Hz plugin timer calls it).
    public func tick() {
        if gesture?.finishing == true { finish() }
    }
```
Replace:
```swift
    /// Retries a mouse-up that met Busy and audition note-offs that failed
    /// (the 60 Hz plugin timer calls it).
    public func tick() {
        if hasPendingNoteOffs { retryNoteOffs() }
        if gesture?.finishing == true { finish() }
    }
```

- [ ] **Step 4: The Swift 6 warning and weak menu closures**

In `app/Sources/VizDAWEditors/EditorCanvas.swift`:

Find:
```swift
    override func accessibilityPerformPress() -> Bool {
        guard let press else { return false }
        press()
        return true
    }
```
Replace:
```swift
    // AppKit declares this nonisolated; VoiceOver calls it on the main thread.
    nonisolated override func accessibilityPerformPress() -> Bool {
        MainActor.assumeIsolated {
            guard let press else { return false }
            press()
            return true
        }
    }
```

In `app/Sources/VizDAWEditors/ArrangementView.swift`, replace every `[unowned self] in self.` inside `menu(for:)` with `[weak self] in self?.`. The block after the change:

```swift
        if block.kind == .reference, let event = block.events.first {
            add("Open Sequence") { [weak self] in self?.editor.openReference(block) }
            add("Parameters…") { [weak self] in self?.editor.editReference(block, in: key) }
            add("Make Private Copy") { [weak self] in self?.editor.makePrivateCopy(event) }
            add("Flatten") { [weak self] in self?.editor.flatten(event) }
            let sub = NSMenu()
            for s in EditorModel.rePointTargets(excluding: block.refSeq, among: document.snapshot.sequences) {
                sub.addItem(
                    MenuAction(title: s.name) { [weak self] in self?.editor.rePoint(event, to: s.id) }
                        .item(checked: false))
            }
            let rePoint = NSMenuItem(title: "Re-point To", action: nil, keyEquivalent: "")
            rePoint.submenu = sub
            menu.addItem(rePoint)
        } else {
            for kind in DetailEditorKind.allCases {
                add("Open in \(kind.title)") { [weak self] in
                    self?.editor.openDetail(
                        .block(seq: key.seq, lane: key.lane, start: block.start, end: block.end), kind: kind)
                }
            }
            add("Make Reference") { [weak self] in self?.editor.makeReference(from: block, in: key) }
        }
        menu.addItem(.separator())
        add("Delete") { [weak self] in self?.editor.edit(.event(edit: .delete(events: block.events))) }
        return menu
```

In `app/Sources/VizDAWEditors/LaneHeaderView.swift`:

Find:
```swift
        let none = MenuAction(title: "No Destination") { [unowned self] in
            self.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: nil)))
        }
```
Replace:
```swift
        let none = MenuAction(title: "No Destination") { [weak self] in
            self?.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: nil)))
        }
```

Find:
```swift
            let a = MenuAction(title: d.name) { [unowned self] in
                self.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: d.id)))
            }
```
Replace:
```swift
            let a = MenuAction(title: d.name) { [weak self] in
                self?.editor.edit(.lane(edit: .setDest(seq: seq, lane: lane.id, dest: d.id)))
            }
```

(If a single-expression closure returning the discardable `Bool?` of `edit` does not type-check as `() -> Void`, write `_ = self?.editor.edit(...)`.)

- [ ] **Step 5: Run the tests and check the warning is gone**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
touch app/Sources/VizDAWEditors/EditorCanvas.swift
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
```

Expected: no warnings or errors; 235 tests pass (227 + 8).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/EditorModel+Audition.swift app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/EditorModel+Gestures.swift app/Sources/VizDAWEditors/EditorCanvas.swift app/Sources/VizDAWEditors/ArrangementView.swift app/Sources/VizDAWEditors/LaneHeaderView.swift app/Tests/VizDAWAppModelTests/FakeSession.swift app/Tests/VizDAWAppModelTests/AuditionTests.swift
git commit -m "feat(app): audition notes with retried note-offs; weak menu closures; fix isolation warning

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---

### Task 5: Torn-off editor windows are command targets

**Files:**
- Modify: `app/Sources/VizDAWAppModel/DocumentStore.swift`, `app/Sources/VizDAWAppModel/EditorModel+Windows.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/EditorWindows.swift`
- Test: `app/Tests/VizDAWAppModelTests/TornFocusTests.swift` (new)

**Interfaces:**
- Consumes: `EditorModel.{setWindowKind(_:_:), layout.windows, updateLayout}`.
- Produces: `public struct TornFocus: Equatable, Sendable { doc: UUID; window: UUID; init(doc:window:) }`; `DocumentStore.tornFocus: TornFocus?` (read-only), `focusTorn(_:)`, `blurTorn(window:)`, `commandTarget(focused: UUID?) -> UUID?`; `EditorModel.tearOff(_ kind: DetailEditorKind, from window: UUID) -> TornWindow?`; `EditorWindows.tearOff(_:doc:kind:from:)` (new `from: UUID? = nil`); `EditorWindowController.init(model:doc:window:onClose:)`.

Ruling 18. This is 4a's deferred "I5": a torn window gave no focused document, so Space, ⌘Z and ⌘1–4 did nothing while it was key.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/TornFocusTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct TornFocusTests {
    func store() -> DocumentStore {
        DocumentStore(makeEmpty: { editorFake() }, makeDemo: { editorFake() }, open: { _ in editorFake() })
    }

    @Test func aKeyTornWindowIsTheCommandTarget() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let b = try #require(s.newEmpty())
        let w = UUID()
        #expect(s.commandTarget(focused: b) == b)
        s.focusTorn(TornFocus(doc: a, window: w))
        #expect(s.commandTarget(focused: b) == a, "the torn window of a is key")
        s.blurTorn(window: w)
        #expect(s.commandTarget(focused: b) == b)
    }

    @Test func blurringAnotherWindowKeepsTheFocus() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let w = UUID()
        s.focusTorn(TornFocus(doc: a, window: w))
        s.blurTorn(window: UUID())
        #expect(s.tornFocus == TornFocus(doc: a, window: w))
    }

    @Test func closingTheDocumentClearsTheFocus() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        s.focusTorn(TornFocus(doc: a, window: UUID()))
        s.close(a)
        #expect(s.tornFocus == nil)
        #expect(s.commandTarget(focused: nil) == nil)
    }

    @Test func tearingOffFromAWindowCopiesItsTarget() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31))
        let w = try #require(m.editor.tearOff(.pianoRoll, shown: 3))
        m.editor.closeDetail()
        let w2 = try #require(m.editor.tearOff(.list, from: w.id))
        #expect(w2.detail == DetailState(kind: .list, target: .lane(seq: 3, lane: 31)))
        #expect(m.editor.layout.windows.map(\.id) == [w.id, w2.id])
        #expect(m.editor.tearOff(.list, from: UUID()) == nil)
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter TornFocusTests 2>&1 | tail -5
```

Expected: compile errors (`TornFocus` unknown).

- [ ] **Step 2: The store's torn focus**

In `app/Sources/VizDAWAppModel/DocumentStore.swift`:

Find:
```swift
/// The open documents, one per window, keyed by the window's value.
@Observable
@MainActor
public final class DocumentStore {
```
Replace:
```swift
/// A torn-off editor window that is key (Plan 4b Ruling 18).
public struct TornFocus: Equatable, Sendable {
    public let doc: UUID
    public let window: UUID

    public init(doc: UUID, window: UUID) {
        self.doc = doc
        self.window = window
    }
}

/// The open documents, one per window, keyed by the window's value.
@Observable
@MainActor
public final class DocumentStore {
```

Find:
```swift
    public func model(_ id: UUID) -> DocumentModel? { documents[id] }
```
Replace:
```swift
    public func model(_ id: UUID) -> DocumentModel? { documents[id] }

    /// The torn-off editor window that is key, if any. AppKit windows
    /// publish no SwiftUI focused values, so menu commands ask this first.
    public private(set) var tornFocus: TornFocus?

    public func focusTorn(_ f: TornFocus) { tornFocus = f }

    /// Window `window` resigned key or closed.
    public func blurTorn(window: UUID) {
        if tornFocus?.window == window { tornFocus = nil }
    }

    /// The document menu commands act on: a key torn-off window's, else the
    /// focused document window's.
    public func commandTarget(focused: UUID?) -> UUID? {
        tornFocus.map(\.doc) ?? focused
    }
```

Find:
```swift
    public func close(_ id: UUID) {
        mixers.removeValue(forKey: id)?.endOpenDrag()
```
Replace:
```swift
    public func close(_ id: UUID) {
        if tornFocus?.doc == id { tornFocus = nil }
        mixers.removeValue(forKey: id)?.endOpenDrag()
```

- [ ] **Step 3: Tear off from a window**

In `app/Sources/VizDAWAppModel/EditorModel+Windows.swift`:

Find:
```swift
    public func closeWindow(_ id: UUID) {
```
Replace:
```swift
    /// ⌥⌘1–⌥⌘4 while torn-off window `id` is key: a new window with `kind`
    /// on that window's target (Plan 4b Ruling 18).
    @discardableResult
    public func tearOff(_ kind: DetailEditorKind, from id: UUID) -> TornWindow? {
        guard var detail = layout.windows.first(where: { $0.id == id })?.detail else { return nil }
        detail.kind = kind
        let w = TornWindow(id: UUID(), detail: detail, frame: nil)
        updateLayout {
            $0.windows.append(w)
            $0.lastKind = kind
        }
        return w
    }

    public func closeWindow(_ id: UUID) {
```

- [ ] **Step 4: Windows report focus; commands use it**

In `app/Sources/VizDAW/EditorWindows.swift`:

Find:
```swift
        weak var made: EditorWindowController?
        let c = EditorWindowController(model: model, window: window) { byUser in
```
Replace:
```swift
        weak var made: EditorWindowController?
        let c = EditorWindowController(model: model, doc: doc, window: window) { byUser in
```

Find:
```swift
    /// ⌥⌘1–⌥⌘4 and "Open in Window".
    static func tearOff(_ store: DocumentStore, doc: UUID, kind: DetailEditorKind) {
        guard let model = store.model(doc), let w = model.editor.tearOff(kind, shown: model.selectedSeq)
        else { return }
        show(store, doc: doc, window: w)
    }
```
Replace:
```swift
    /// ⌥⌘1–⌥⌘4 and "Open in Window"; `from` a key torn window copies its
    /// target (Plan 4b Ruling 18).
    static func tearOff(_ store: DocumentStore, doc: UUID, kind: DetailEditorKind, from window: UUID? = nil) {
        guard let model = store.model(doc) else { return }
        let made = window.map { model.editor.tearOff(kind, from: $0) } ?? model.editor.tearOff(kind, shown: model.selectedSeq)
        guard let w = made else { return }
        show(store, doc: doc, window: w)
    }
```

Find:
```swift
    private let model: DocumentModel
    private let id: UUID
    private let onClose: (Bool) -> Void
    private var forDocument = false
    private(set) var closed = false

    init(model: DocumentModel, window w: TornWindow, onClose: @escaping (Bool) -> Void) {
        self.model = model
        id = w.id
```
Replace:
```swift
    private let model: DocumentModel
    private let doc: UUID
    private let id: UUID
    private let onClose: (Bool) -> Void
    private var forDocument = false
    private(set) var closed = false

    init(model: DocumentModel, doc: UUID, window w: TornWindow, onClose: @escaping (Bool) -> Void) {
        self.model = model
        self.doc = doc
        id = w.id
```

Find:
```swift
    func windowWillClose(_ notification: Notification) {
        guard !closed else { return }
        closed = true
        onClose(!forDocument)
    }
```
Replace:
```swift
    func windowWillClose(_ notification: Notification) {
        guard !closed else { return }
        closed = true
        appStore.blurTorn(window: id)
        onClose(!forDocument)
    }
    func windowDidBecomeKey(_ notification: Notification) {
        appStore.focusTorn(TornFocus(doc: doc, window: id))
    }
    func windowDidResignKey(_ notification: Notification) { appStore.blurTorn(window: id) }
```

In `app/Sources/VizDAW/AppCommands.swift`:

Find:
```swift
    let store: DocumentStore
    @FocusedValue(\.documentModel) private var doc
    @FocusedValue(\.documentID) private var docID
    @Environment(\.openWindow) private var openWindow
```
Replace:
```swift
    let store: DocumentStore
    @FocusedValue(\.documentModel) private var focusedDoc
    @FocusedValue(\.documentID) private var focusedID
    @Environment(\.openWindow) private var openWindow

    /// A key torn-off window's document first (Plan 4b Ruling 18).
    private var doc: DocumentModel? { store.tornFocus.flatMap { store.model($0.doc) } ?? focusedDoc }
    private var docID: UUID? { store.commandTarget(focused: focusedID) }
    /// The key torn-off window, if any.
    private var tornWindow: UUID? { store.tornFocus?.window }
```

Find:
```swift
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
```
Replace:
```swift
            ForEach(DetailEditorKind.allCases, id: \.self) { kind in
                Button(kind.title) {
                    if let w = tornWindow {
                        doc?.editor.setWindowKind(w, kind)
                    } else {
                        doc?.editor.showDetail(kind, shown: doc?.selectedSeq)
                    }
                }
                .keyboardShortcut(KeyEquivalent(Character(kind.key)), modifiers: .command)
                .disabled(doc == nil)
            }
            Divider()
            ForEach(DetailEditorKind.allCases, id: \.self) { kind in
                Button("Open \(kind.title) in Window") {
                    if let docID { EditorWindows.tearOff(store, doc: docID, kind: kind, from: tornWindow) }
                }
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
```

Expected: clean build; 239 tests.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/DocumentStore.swift app/Sources/VizDAWAppModel/EditorModel+Windows.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/EditorWindows.swift app/Tests/VizDAWAppModelTests/TornFocusTests.swift
git commit -m "feat(app): torn-off editor windows are menu command targets

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 6: Pitch names, scales, `PitchAxis` and `ValueAxis`

**Files:**
- Create: `app/Sources/VizDAWAppModel/PitchMath.swift`
- Create: `app/Sources/VizDAWEditors/PitchAxis.swift`
- Test: `app/Tests/VizDAWAppModelTests/PitchMathTests.swift`, `app/Tests/VizDAWEditorsTests/PitchAxisTests.swift` (new)

**Interfaces:**
- Consumes: nothing new.
- Produces (VizDAWAppModel): `enum PitchName { static func name(_ p: UInt8) -> String; static func isBlack(_ p: UInt8) -> Bool }`; `enum Scale: String, Codable, CaseIterable, Sendable { major, naturalMinor, harmonicMinor, dorian, mixolydian, majorPentatonic, minorPentatonic; var mask: UInt16; var title: String }`; `enum ScaleMath { static func keyRoot(sharps: Int8, minor: Bool) -> UInt8; static func inScale(_ p: Int, root: UInt8, mask: UInt16) -> Bool; static func step(_ p: UInt8, by steps: Int, root: UInt8, mask: UInt16) -> UInt8; static let rootNames: [String] }`.
- Produces (VizDAWEditors): `struct PitchAxis: Equatable, Sendable { rowHeight: CGFloat (get); offset: CGFloat; init(rowHeight:offset:); contentHeight; top(_:); center(_:); pitch(atY:) -> UInt8; row(atY:) -> Int; semitones(fromY:toY:) -> Int; withRowHeight(_:keepingY:) -> PitchAxis; centering(_:height:) -> PitchAxis; clamped(height:) -> PitchAxis; visible(height:) -> ClosedRange<UInt8> }`; `struct ValueAxis: Equatable, Sendable { height; inset; init(height:inset:); y(_ v: UInt32) -> CGFloat; value(atY:) -> UInt32 }`.

Spec §3.5 ("a matching `PitchAxis`/`ValueAxis` for vertical mapping. Pure value types, unit-tested"); 4a Ruling 22 deferred them here. Ruling 17 (key roots).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/PitchMathTests.swift <<'VIZDAW_EOF'
import Testing
@testable import VizDAWAppModel

@Suite struct PitchMathTests {
    @Test func namesMatchTheCore() {
        #expect(PitchName.name(60) == "C4")
        #expect(PitchName.name(0) == "C-1")
        #expect(PitchName.name(61) == "C#4")
        #expect(PitchName.name(127) == "G9")
    }

    @Test func blackKeys() {
        #expect((60...71).filter(PitchName.isBlack) == [61, 63, 66, 68, 70])
    }

    @Test func scaleMasksHoldTheirPitchClasses() {
        func classes(_ s: Scale) -> [Int] { (0..<12).filter { s.mask & (1 << $0) != 0 } }
        #expect(classes(.major) == [0, 2, 4, 5, 7, 9, 11])
        #expect(classes(.naturalMinor) == [0, 2, 3, 5, 7, 8, 10])
        #expect(classes(.harmonicMinor) == [0, 2, 3, 5, 7, 8, 11])
        #expect(classes(.dorian) == [0, 2, 3, 5, 7, 9, 10])
        #expect(classes(.mixolydian) == [0, 2, 4, 5, 7, 9, 10])
        #expect(classes(.majorPentatonic) == [0, 2, 4, 7, 9])
        #expect(classes(.minorPentatonic) == [0, 3, 5, 7, 10])
    }

    @Test func keySignatureRoots() {
        #expect(ScaleMath.keyRoot(sharps: 0, minor: false) == 0)
        #expect(ScaleMath.keyRoot(sharps: 1, minor: false) == 7, "G")
        #expect(ScaleMath.keyRoot(sharps: 2, minor: false) == 2, "D")
        #expect(ScaleMath.keyRoot(sharps: -1, minor: false) == 5, "F")
        #expect(ScaleMath.keyRoot(sharps: -3, minor: false) == 3, "E flat")
        #expect(ScaleMath.keyRoot(sharps: 0, minor: true) == 9, "A minor")
        #expect(ScaleMath.keyRoot(sharps: 1, minor: true) == 4, "E minor")
    }

    @Test func stepsMirrorTheCore() {
        let m = Scale.major.mask
        #expect(ScaleMath.step(64, by: 1, root: 0, mask: m) == 65)
        #expect(ScaleMath.step(61, by: 1, root: 0, mask: m) == 62, "off-scale: one step to the next")
        #expect(ScaleMath.step(60, by: -1, root: 0, mask: m) == 59)
        #expect(ScaleMath.step(127, by: 1, root: 0, mask: m) == 127)
        #expect(ScaleMath.step(60, by: 7, root: 0, mask: m) == 72)
        // G major: F# is in, F is not.
        #expect(ScaleMath.step(64, by: 1, root: 7, mask: m) == 66)
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWEditorsTests/PitchAxisTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Testing
@testable import VizDAWEditors

@Suite struct PitchAxisTests {
    @Test func rowsRunFromTheTop() {
        let a = PitchAxis(rowHeight: 10, offset: 0)
        #expect(a.top(127) == 0)
        #expect(a.top(0) == 1_270)
        #expect(a.center(60) == 675)
        #expect(a.contentHeight == 1_280)
    }

    @Test func pitchAtYClampsAndFollowsTheOffset() {
        let a = PitchAxis(rowHeight: 10, offset: 600)
        #expect(a.pitch(atY: 75) == 60)
        #expect(a.pitch(atY: -1_000) == 127)
        #expect(a.pitch(atY: 9_999) == 0)
        #expect(a.pitch(atY: .nan) == 60)
    }

    @Test func semitonesAreWholeRowsUpPositive() {
        let a = PitchAxis(rowHeight: 10, offset: 3)
        #expect(a.semitones(fromY: 100, toY: 81) == 2)
        #expect(a.semitones(fromY: 100, toY: 104) == 0)
        #expect(a.semitones(fromY: 100, toY: 135) == -3)
        #expect(a.semitones(fromY: 0, toY: -5_000) > 127, "unclamped: the drag clamps the group")
    }

    @Test func zoomKeepsThePitchUnderTheAnchor() {
        let a = PitchAxis(rowHeight: 10, offset: 600)
        let z = a.withRowHeight(20, keepingY: 75)
        #expect(z.rowHeight == 20)
        #expect(z.pitch(atY: 75) == 60)
        #expect(PitchAxis(rowHeight: 999).rowHeight == PitchAxis.maxRowHeight)
        #expect(PitchAxis(rowHeight: .nan).rowHeight == 10)
    }

    @Test func centeringClampsToTheContent() {
        let a = PitchAxis(rowHeight: 10)
        let c = a.centering(60, height: 200)
        #expect(abs(c.center(60) - 100) < 0.001)
        #expect(a.centering(127, height: 200).offset == 0)
        #expect(a.centering(0, height: 200).offset == 1_080)
        #expect(a.clamped(height: 2_000).offset == 0, "taller than the content")
    }

    @Test func visiblePitches() {
        let a = PitchAxis(rowHeight: 10, offset: 600)
        #expect(a.visible(height: 100) == 58...67)
    }

    @Test func valueAxisRoundTripsAndClamps() {
        let v = ValueAxis(height: 96, inset: 3)
        #expect(v.y(UInt32.max) == 3)
        #expect(v.y(0) == 93)
        #expect(v.value(atY: 3) == UInt32.max)
        #expect(v.value(atY: 93) == 0)
        #expect(v.value(atY: -50) == UInt32.max)
        #expect(v.value(atY: 500) == 0)
        let mid = v.value(atY: v.y(0x8000_0000))
        #expect(mid > 0x7F00_0000 && mid < 0x8100_0000)
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter "PitchMathTests|PitchAxisTests" 2>&1 | tail -5
```

Expected: compile errors (types unknown).

- [ ] **Step 2: Implement the maths**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/PitchMath.swift <<'VIZDAW_EOF'
/// Note names as the core prints them: C-1 is 0, middle C (60) is "C4".
public enum PitchName {
    static let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]

    public static func name(_ p: UInt8) -> String {
        "\(names[Int(p % 12)])\(Int(p / 12) - 1)"
    }

    public static func isBlack(_ p: UInt8) -> Bool {
        [1, 3, 6, 8, 10].contains(Int(p % 12))
    }
}

/// Scales nudge can follow (Plan 4b Ruling 16).
public enum Scale: String, Codable, CaseIterable, Sendable {
    case major, naturalMinor, harmonicMinor, dorian, mixolydian, majorPentatonic, minorPentatonic

    /// Bit `i`: pitch class `root + i` (the core's mask convention).
    public var mask: UInt16 {
        switch self {
        case .major: 0b1010_1011_0101
        case .naturalMinor: 0b0101_1010_1101
        case .harmonicMinor: 0b1001_1010_1101
        case .dorian: 0b0110_1010_1101
        case .mixolydian: 0b0110_1011_0101
        case .majorPentatonic: 0b0010_1001_0101
        case .minorPentatonic: 0b0100_1010_1001
        }
    }

    public var title: String {
        switch self {
        case .major: "Major"
        case .naturalMinor: "Natural Minor"
        case .harmonicMinor: "Harmonic Minor"
        case .dorian: "Dorian"
        case .mixolydian: "Mixolydian"
        case .majorPentatonic: "Major Pentatonic"
        case .minorPentatonic: "Minor Pentatonic"
        }
    }
}

/// Keys and scale steps (Plan 4b Rulings 16, 17).
public enum ScaleMath {
    public static let rootNames = PitchName.names

    /// The root pitch class of a key signature: (7 × sharps) mod 12, plus 9
    /// for a minor key.
    public static func keyRoot(sharps: Int8, minor: Bool) -> UInt8 {
        let major = ((7 * Int(sharps)) % 12 + 12) % 12
        return UInt8((major + (minor ? 9 : 0)) % 12)
    }

    public static func inScale(_ p: Int, root: UInt8, mask: UInt16) -> Bool {
        let pc = ((p - Int(root % 12)) % 12 + 12) % 12
        return mask & (1 << pc) != 0
    }

    /// `p` moved `steps` scale degrees, as the core's `scale_step` does.
    public static func step(_ p: UInt8, by steps: Int, root: UInt8, mask: UInt16) -> UInt8 {
        guard mask & 0x0FFF != 0, steps != 0 else { return p }
        let dir = steps > 0 ? 1 : -1
        var q = Int(p)
        for _ in 0..<abs(steps) {
            guard let next = (1...12).map({ q + dir * $0 }).first(where: { inScale($0, root: root, mask: mask) }),
                (0...127).contains(next)
            else { break }
            q = next
        }
        return UInt8(q)
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/PitchAxis.swift <<'VIZDAW_EOF'
import CoreGraphics

/// Pitch ↔ y for the piano roll (spec §3.5): pitch 127 on top, one row of
/// `rowHeight` points per pitch; `offset` is the vertical scroll (points
/// from the top of pitch 127). Pure value type; nothing traps.
public struct PitchAxis: Equatable, Sendable {
    public static let minRowHeight: CGFloat = 4
    public static let maxRowHeight: CGFloat = 40
    public static let pitches = 128

    public private(set) var rowHeight: CGFloat
    public var offset: CGFloat

    public init(rowHeight: CGFloat = 10, offset: CGFloat = 0) {
        self.rowHeight = Self.clampRow(rowHeight)
        self.offset = offset.isFinite ? offset : 0
    }

    static func clampRow(_ h: CGFloat) -> CGFloat {
        guard h.isFinite else { return 10 }
        return min(max(h, minRowHeight), maxRowHeight)
    }

    public var contentHeight: CGFloat { CGFloat(Self.pitches) * rowHeight }

    public func top(_ pitch: UInt8) -> CGFloat {
        CGFloat(127 - Int(min(pitch, 127))) * rowHeight - offset
    }

    public func center(_ pitch: UInt8) -> CGFloat { top(pitch) + rowHeight / 2 }

    /// The unclamped row under `y` (0 is pitch 127's row).
    public func row(atY y: CGFloat) -> Int {
        guard y.isFinite else { return 67 }
        let r = ((y + offset) / rowHeight).rounded(.down)
        return Int(min(max(r, -1_000_000), 1_000_000))
    }

    public func pitch(atY y: CGFloat) -> UInt8 {
        UInt8(127 - min(max(row(atY: y), 0), 127))
    }

    /// Whole rows from `a` to `b`, up positive, unclamped.
    public func semitones(fromY a: CGFloat, toY b: CGFloat) -> Int {
        row(atY: a) - row(atY: b)
    }

    /// Another row height, keeping the pitch position under `y`.
    public func withRowHeight(_ h: CGFloat, keepingY y: CGFloat) -> PitchAxis {
        let new = Self.clampRow(h)
        let anchor = (y + offset) / rowHeight
        return PitchAxis(rowHeight: new, offset: anchor * new - y)
    }

    /// `pitch` centred in a view `height` tall, clamped to the content.
    public func centering(_ pitch: UInt8, height: CGFloat) -> PitchAxis {
        var a = self
        a.offset = CGFloat(127 - Int(min(pitch, 127))) * rowHeight + rowHeight / 2 - height / 2
        return a.clamped(height: height)
    }

    public func clamped(height: CGFloat) -> PitchAxis {
        var a = self
        a.offset = min(max(offset, 0), max(contentHeight - height, 0))
        return a
    }

    public func visible(height: CGFloat) -> ClosedRange<UInt8> {
        let h = height.isFinite ? max(height, 0) : 0
        return pitch(atY: max(h - 0.001, 0))...pitch(atY: 0)
    }
}

/// A 32-bit value ↔ y inside a pane `height` tall: the maximum at the top,
/// `inset` points kept free above and below.
public struct ValueAxis: Equatable, Sendable {
    public var height: CGFloat
    public var inset: CGFloat

    public init(height: CGFloat, inset: CGFloat = 3) {
        self.height = height
        self.inset = inset
    }

    var span: CGFloat { max(height - 2 * inset, 1) }

    public func y(_ v: UInt32) -> CGFloat {
        inset + span * (1 - CGFloat(Double(v) / Double(UInt32.max)))
    }

    public func value(atY y: CGFloat) -> UInt32 {
        guard y.isFinite else { return 0 }
        let f = min(max(1 - Double((y - inset) / span), 0), 1)
        return UInt32((f * Double(UInt32.max)).rounded())
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter "PitchMathTests|PitchAxisTests" 2>&1 | tail -5
```

Expected: PASS (12 tests). Check `visible(height: 100)` by hand if it fails: with offset 600 and rows of 10, y 0 is row 60 (pitch 67) and y 99.999 is row 69 (pitch 58).

- [ ] **Step 3: Full check and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
git add app/Sources/VizDAWAppModel/PitchMath.swift app/Sources/VizDAWEditors/PitchAxis.swift app/Tests/VizDAWAppModelTests/PitchMathTests.swift app/Tests/VizDAWEditorsTests/PitchAxisTests.swift
git commit -m "feat(editors): pitch names, scales, PitchAxis and ValueAxis

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 251 tests.

---

### Task 7: Note geometry, cursor zones, hit-testing and the tools' targets

**Files:**
- Create: `app/Sources/VizDAWEditors/PianoRollGeometry.swift`, `app/Sources/VizDAWEditors/PianoTools.swift`
- Modify: `app/Sources/VizDAWAppModel/Snap.swift`
- Test: `app/Tests/VizDAWEditorsTests/PianoRollGeometryTests.swift` (new), `app/Tests/VizDAWAppModelTests/SnapTests.swift`

**Interfaces:**
- Consumes: `TimeAxis`, `PitchAxis` (Task 6), `Snap`, `MeterMath`, `PitchName`, `LaneKey`, `EditCommand`/`EventEdit` with Task 3's variants.
- Produces:
  - `Snap.floor(_ tick: Int64, meter: [MeterEntryRow]) -> Int64`.
  - `enum NoteZone { start, middle, end }`; `struct NoteHit: Equatable, Sendable { lane: LaneKey; row: EventRow; zone: NoteZone }`; `enum PianoRollHit: Equatable, Sendable { note(NoteHit); empty(tick: Int64, pitch: UInt8) }`; `struct LaneNotes: Equatable, Sendable { lane: LaneKey; notes: [EventRow]; init(lane:notes:) }`.
  - `enum PianoRollGeometry { minNoteWidth = 3; maxEdge = 8; rect(_:time:pitch:) -> CGRect; zone(atX:in:) -> NoteZone; hit(_:lanes:active:time:pitch:) -> PianoRollHit; notes(in:lanes:time:pitch:) -> [(lane: LaneKey, id: UInt64)] }`.
  - `struct NoteDrag: Equatable, Sendable { zone; pressed: EventRow; lane: LaneKey; events: [UInt64]; lowest: UInt8; highest: UInt8; copy: Bool; init(...); label: String; static events(pressed:selected:lanes:) -> [UInt64]; static timeDelta(rawTicks:start:snap:meter:) -> Int64; static rawSemitones(dy:rowHeight:) -> Int; semitones(raw:) -> Int; durationDelta(rawTicks:snap:meter:) -> Int64; command(rawTicks:dy:rowHeight:snap:meter:) -> EditCommand?; soundingPitch(dy:rowHeight:) -> UInt8 }`.
  - `enum Pencil { velocity: UInt16 = 100 << 9; step(at:snap:meter:) -> Int64; start(_:snap:meter:) -> Int64; length(start:to:snap:meter:) -> Int64; note(tick:pitch:length:) -> EventRow; command(lane:tick:pitch:length:) -> EditCommand }`; `enum Eraser { adding(_:to:) -> [UInt64]; command(_:) -> EditCommand? }`; `enum RubberBand { rect(from:to:) -> CGRect }`; `enum CursorKind { arrow, openHand, upDown, resizeRight, crosshair, eraser }` and `enum PianoRollCursor { kind(for:tool:) -> CursorKind }`; `enum PianoRollA11y { label(_:laneName:meter:) -> String }`.

Rulings 4, 5, 7, 8, 20, 21.

- [ ] **Step 1: Write the failing tests**

Append to `app/Tests/VizDAWAppModelTests/SnapTests.swift` inside the existing suite (before its closing brace):

```swift
    @Test func floorTakesTheGridLineAtOrBefore() {
        let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        var s = Snap()
        s.division = .quarter
        #expect(s.floor(15_359, meter: meter) == 0)
        #expect(s.floor(15_360, meter: meter) == 15_360)
        #expect(s.floor(-1, meter: meter) == -15_360, "pickup bars")
        s.enabled = false
        #expect(s.floor(15_359, meter: meter) == 15_359)
    }
```

(If `SnapTests.swift` has no `import VizDAWCore`, add it.)

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/PianoRollGeometryTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let q: Int64 = 15_360
private let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
private let laneA = LaneKey(3, 31)
private let laneB = LaneKey(3, 30)
/// 96 ticks per point: a quarter is 160 pt. Rows of 10 pt, pitch 127 at y 0.
private let time = TimeAxis(ticksPerPoint: 96)
private let pitch = PitchAxis(rowHeight: 10)

private func note(_ id: UInt64, _ tick: Int64, _ p: UInt8, _ length: Int64 = q) -> EventRow {
    var r = Pencil.note(tick: tick, pitch: p, length: length)
    r.id = id
    r.velocity = 0x8000
    return r
}

private func y(_ p: UInt8) -> CGFloat { pitch.center(p) }

@Suite struct PianoRollGeometryTests {
    @Test func rectsHaveAMinimumWidth() {
        #expect(PianoRollGeometry.rect(note(1, q, 60), time: time, pitch: pitch) == CGRect(x: 160, y: 670, width: 160, height: 10))
        #expect(PianoRollGeometry.rect(note(1, 0, 60, 0), time: time, pitch: pitch).width == 3)
    }

    @Test func zonesOnWideAndNarrowNotes() {
        let wide = CGRect(x: 0, y: 0, width: 160, height: 10)
        #expect(PianoRollGeometry.zone(atX: 7, in: wide) == .start)
        #expect(PianoRollGeometry.zone(atX: 8, in: wide) == .middle)
        #expect(PianoRollGeometry.zone(atX: 151, in: wide) == .middle)
        #expect(PianoRollGeometry.zone(atX: 152, in: wide) == .end)
        let narrow = CGRect(x: 0, y: 0, width: 12, height: 10)
        #expect(PianoRollGeometry.zone(atX: 3, in: narrow) == .start)
        #expect(PianoRollGeometry.zone(atX: 6, in: narrow) == .middle)
        #expect(PianoRollGeometry.zone(atX: 9, in: narrow) == .end)
    }

    @Test func theActiveLaneWinsAndTheLatestNoteWins() {
        let lanes = [
            LaneNotes(lane: laneB, notes: [note(7, 0, 60, 4 * q)]),
            LaneNotes(lane: laneA, notes: [note(1, 0, 60, 2 * q), note(2, q, 60)]),
        ]
        let p = CGPoint(x: 200, y: y(60))
        guard case .note(let h) = PianoRollGeometry.hit(p, lanes: lanes, active: laneA, time: time, pitch: pitch)
        else { Issue.record("no hit"); return }
        #expect(h.lane == laneA && h.row.id == 2 && h.zone == .middle)
        guard case .note(let hb) = PianoRollGeometry.hit(p, lanes: lanes, active: laneB, time: time, pitch: pitch)
        else { Issue.record("no hit"); return }
        #expect(hb.row.id == 7)
        #expect(
            PianoRollGeometry.hit(CGPoint(x: 200, y: y(61)), lanes: lanes, active: laneA, time: time, pitch: pitch)
                == .empty(tick: 19_200, pitch: 61))
    }

    @Test func rubberBandFindsNotesOfEveryLane() {
        let lanes = [
            LaneNotes(lane: laneB, notes: [note(7, 0, 64)]),
            LaneNotes(lane: laneA, notes: [note(1, 0, 60), note(2, 4 * q, 60)]),
        ]
        let r = RubberBand.rect(from: CGPoint(x: 100, y: y(65)), to: CGPoint(x: 10, y: y(59)))
        let found = PianoRollGeometry.notes(in: r, lanes: lanes, time: time, pitch: pitch)
        #expect(found.map { $0.id } == [7, 1])
        #expect(found.map { $0.lane } == [laneB, laneA])
    }

    @Test func theDraggedSetIsTheSelectionWhenThePressedNoteIsSelected() {
        let lanes = [
            LaneNotes(lane: laneB, notes: [note(7, 0, 64)]),
            LaneNotes(lane: laneA, notes: [note(1, 0, 60), note(2, q, 62)]),
        ]
        #expect(NoteDrag.events(pressed: 1, selected: [1, 7], lanes: lanes) == [1, 7])
        #expect(NoteDrag.events(pressed: 2, selected: [1, 7], lanes: lanes) == [2])
        #expect(NoteDrag.events(pressed: 1, selected: [1, 99], lanes: lanes) == [1], "hidden lanes are not dragged")
    }

    func drag(_ zone: NoteZone, copy: Bool = false, lowest: UInt8 = 60, highest: UInt8 = 64) -> NoteDrag {
        NoteDrag(
            zone: zone, pressed: note(1, q, 60), lane: laneA, events: [1, 2], lowest: lowest, highest: highest,
            copy: copy)
    }

    @Test func startZoneMovesInTimeSnappingThePressedStart() {
        var snap = Snap()
        snap.division = .quarter
        let d = drag(.start)
        #expect(d.label == "Move Notes")
        #expect(d.command(rawTicks: 9_000, dy: -40, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .move(events: [1, 2], delta: q, toLane: nil)))
        #expect(d.command(rawTicks: 100, dy: 0, rowHeight: 10, snap: snap, meter: meter) == nil)
        #expect(drag(.start, copy: true).command(rawTicks: -q, dy: 0, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .duplicate(events: [1, 2], ticks: -q, semitones: 0)))
        #expect(drag(.start, copy: true).label == "Copy Notes")
    }

    @Test func middleZoneTransposes() {
        let snap = Snap()
        #expect(drag(.middle).label == "Transpose Notes")
        #expect(drag(.middle).command(rawTicks: 50_000, dy: -21, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .nudge(events: [1, 2], ticks: 0, semitones: 2)))
        #expect(drag(.middle, copy: true).command(rawTicks: 0, dy: 30, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .duplicate(events: [1, 2], ticks: 0, semitones: -3)))
        #expect(drag(.middle).soundingPitch(dy: -21, rowHeight: 10) == 62)
        #expect(drag(.start).soundingPitch(dy: -21, rowHeight: 10) == 60)
    }

    @Test func transposeClampsTheGroupAtTheEdges() {
        let d = drag(.middle, lowest: 60, highest: 125)
        #expect(d.semitones(raw: 10) == 2)
        #expect(d.semitones(raw: -100) == -60)
        #expect(d.command(rawTicks: 0, dy: -1_000, rowHeight: 10, snap: Snap(), meter: meter)
            == .event(edit: .nudge(events: [1, 2], ticks: 0, semitones: 2)))
        let top = drag(.middle, lowest: 127, highest: 127)
        #expect(top.command(rawTicks: 0, dy: -50, rowHeight: 10, snap: Snap(), meter: meter) == nil)
    }

    @Test func endZoneResizesSnappingTheEndAndKeepsOneTick() {
        var snap = Snap()
        snap.division = .sixteenth
        let d = drag(.end, copy: true)
        #expect(d.label == "Resize Notes", "⌥ is ignored at the end")
        #expect(d.command(rawTicks: 3_000, dy: 0, rowHeight: 10, snap: snap, meter: meter)
            == .event(edit: .resize(events: [1, 2], delta: 3_840)))
        #expect(d.durationDelta(rawTicks: -10 * q, snap: snap, meter: meter) == 1 - q)
        snap.enabled = false
        #expect(d.durationDelta(rawTicks: 7, snap: snap, meter: meter) == 7)
    }

    @Test func pencilStartsOnTheGridAndPaintsALength() {
        var snap = Snap()
        snap.division = .eighth
        #expect(Pencil.start(10_000, snap: snap, meter: meter) == 7_680)
        #expect(Pencil.step(at: 0, snap: snap, meter: meter) == 7_680)
        #expect(Pencil.length(start: 7_680, to: 7_000, snap: snap, meter: meter) == 7_680, "at least one step")
        #expect(Pencil.length(start: 7_680, to: 30_000, snap: snap, meter: meter) == 23_040)
        snap.enabled = false
        #expect(Pencil.step(at: 0, snap: snap, meter: meter) == q)
        #expect(Pencil.length(start: 0, to: 100, snap: snap, meter: meter) == q / 8)
        #expect(Pencil.length(start: 0, to: 5_000, snap: snap, meter: meter) == 5_000)
        let row = Pencil.note(tick: 7_680, pitch: 64, length: 7_680)
        #expect(row.kind == .note && row.velocity == 51_200 && row.duration == 7_680 && row.pitch == 64)
        #expect(Pencil.command(lane: laneA, tick: 7_680, pitch: 64, length: 7_680)
            == .event(edit: .insert(seq: 3, lane: 31, events: [row])))
    }

    @Test func theEraserCollectsEachNoteOnce() {
        let hit = PianoRollHit.note(NoteHit(lane: laneA, row: note(2, 0, 60), zone: .middle))
        let once = Eraser.adding(hit, to: [1])
        #expect(once == [1, 2])
        #expect(Eraser.adding(hit, to: once) == [1, 2])
        #expect(Eraser.adding(.empty(tick: 0, pitch: 1), to: once) == [1, 2])
        #expect(Eraser.command([]) == nil)
        #expect(Eraser.command([1, 2]) == .event(edit: .delete(events: [1, 2])))
    }

    @Test func cursorsFollowZonesAndTools() {
        let n = { (z: NoteZone) in PianoRollHit.note(NoteHit(lane: laneA, row: note(1, 0, 60), zone: z)) }
        #expect(PianoRollCursor.kind(for: n(.start), tool: .arrow) == .openHand)
        #expect(PianoRollCursor.kind(for: n(.middle), tool: .pencil) == .upDown)
        #expect(PianoRollCursor.kind(for: n(.end), tool: .arrow) == .resizeRight)
        #expect(PianoRollCursor.kind(for: n(.end), tool: .eraser) == .eraser)
        #expect(PianoRollCursor.kind(for: .empty(tick: 0, pitch: 0), tool: .pencil) == .crosshair)
        #expect(PianoRollCursor.kind(for: .empty(tick: 0, pitch: 0), tool: .arrow) == .arrow)
    }

    @Test func accessibilityLabels() {
        var n = note(1, 2 * 4 * q, 60)
        n.velocity = 100 << 9
        #expect(PianoRollA11y.label(n, laneName: nil, meter: meter) == "Note C4, bar 3 beat 1, velocity 100")
        n.tick += q + 240
        n.mute = true
        #expect(PianoRollA11y.label(n, laneName: "Chords", meter: meter)
            == "Note C4, bar 3 beat 2 tick 240, velocity 100, Chords, muted")
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter "PianoRollGeometryTests|SnapTests" 2>&1 | tail -5
```

Expected: compile errors (`Pencil`, `NoteDrag` … unknown; `Snap.floor` missing).

- [ ] **Step 2: `Snap.floor`**

In `app/Sources/VizDAWAppModel/Snap.swift`:

Find:
```swift
    /// `tick` on the nearest grid line of its bar (ties go later); the bar's
```
Replace:
```swift
    /// `tick` on the grid line at or before it, within its bar. Unchanged
    /// when snap is off (the pencil's start, Plan 4b Ruling 7).
    public func floor(_ tick: Int64, meter: [MeterEntryRow]) -> Int64 {
        guard enabled else { return tick }
        let bar = MeterMath.bar(at: tick, meter)
        let s = step(in: bar.meter)
        return bar.start + ((tick - bar.start) / s) * s
    }

    /// `tick` on the nearest grid line of its bar (ties go later); the bar's
```

- [ ] **Step 3: Geometry and drags**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/PianoRollGeometry.swift <<'VIZDAW_EOF'
import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// Where over a note the pointer is (Plan 4b Ruling 4).
public enum NoteZone: Equatable, Sendable {
    case start, middle, end
}

public struct NoteHit: Equatable, Sendable {
    public let lane: LaneKey
    public let row: EventRow
    public let zone: NoteZone

    public init(lane: LaneKey, row: EventRow, zone: NoteZone) {
        self.lane = lane
        self.row = row
        self.zone = zone
    }
}

public enum PianoRollHit: Equatable, Sendable {
    case note(NoteHit)
    case empty(tick: Int64, pitch: UInt8)
}

/// One shown lane's notes (kind `.note` only), in time order.
public struct LaneNotes: Equatable, Sendable {
    public let lane: LaneKey
    public let notes: [EventRow]

    public init(lane: LaneKey, notes: [EventRow]) {
        self.lane = lane
        self.notes = notes
    }
}

/// Note rectangles, zones and hit-testing as pure functions of rows and
/// axes (spec §3.5).
public enum PianoRollGeometry {
    public static let minNoteWidth: CGFloat = 3
    /// The widest a start or end zone gets.
    public static let maxEdge: CGFloat = 8

    public static func rect(_ n: EventRow, time: TimeAxis, pitch: PitchAxis) -> CGRect {
        CGRect(
            x: time.x(n.tick), y: pitch.top(n.pitch),
            width: max(time.width(ofTicks: n.duration), minNoteWidth), height: pitch.rowHeight)
    }

    /// Edge zones are `min(maxEdge, width / 3)` wide.
    public static func zone(atX x: CGFloat, in r: CGRect) -> NoteZone {
        let edge = min(maxEdge, r.width / 3)
        if x < r.minX + edge { return .start }
        if x >= r.maxX - edge { return .end }
        return .middle
    }

    /// The note under `p`: the active lane's first, then the others in
    /// order; within a lane the latest-starting one (Plan 4b Ruling 9).
    public static func hit(
        _ p: CGPoint, lanes: [LaneNotes], active: LaneKey?, time: TimeAxis, pitch: PitchAxis
    ) -> PianoRollHit {
        let target = pitch.pitch(atY: p.y)
        let ordered = lanes.filter { $0.lane == active } + lanes.filter { $0.lane != active }
        for l in ordered {
            let under = l.notes.last { n in
                n.pitch == target && rect(n, time: time, pitch: pitch).contains(p)
            }
            if let n = under {
                let r = rect(n, time: time, pitch: pitch)
                return .note(NoteHit(lane: l.lane, row: n, zone: zone(atX: p.x, in: r)))
            }
        }
        return .empty(tick: time.tick(atX: p.x), pitch: target)
    }

    /// The notes a rubber band `r` touches, lane by lane.
    public static func notes(
        in r: CGRect, lanes: [LaneNotes], time: TimeAxis, pitch: PitchAxis
    ) -> [(lane: LaneKey, id: UInt64)] {
        lanes.flatMap { l in
            l.notes.filter { rect($0, time: time, pitch: pitch).intersects(r) }.map { (l.lane, $0.id) }
        }
    }
}

/// A note drag (Plan 4b Rulings 4, 5): what it carries and its full
/// target for a pointer moved by `rawTicks` and `dy` from the press.
public struct NoteDrag: Equatable, Sendable {
    public let zone: NoteZone
    public let pressed: EventRow
    public let lane: LaneKey
    /// The dragged events.
    public let events: [UInt64]
    /// The lowest and highest pitch among them (transpose limits).
    public let lowest: UInt8
    public let highest: UInt8
    /// ⌥ when the drag began (start and middle zones only).
    public let copy: Bool

    public init(
        zone: NoteZone, pressed: EventRow, lane: LaneKey, events: [UInt64], lowest: UInt8, highest: UInt8,
        copy: Bool
    ) {
        self.zone = zone
        self.pressed = pressed
        self.lane = lane
        self.events = events
        self.lowest = lowest
        self.highest = highest
        self.copy = copy && zone != .end
    }

    public var label: String {
        switch zone {
        case .start: copy ? "Copy Notes" : "Move Notes"
        case .middle: copy ? "Copy Notes" : "Transpose Notes"
        case .end: "Resize Notes"
        }
    }

    /// Every selected note of the shown lanes when the pressed one is
    /// selected, the pressed one first; else the pressed one alone.
    public static func events(pressed: UInt64, selected: Set<UInt64>, lanes: [LaneNotes]) -> [UInt64] {
        guard selected.contains(pressed) else { return [pressed] }
        let others = lanes.flatMap(\.notes).map(\.id).filter { $0 != pressed && selected.contains($0) }
        return [pressed] + others
    }

    /// The time offset that puts the pressed note's start on the grid.
    public static func timeDelta(rawTicks: Int64, start: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        guard snap.enabled else { return rawTicks }
        return snap.apply(start + rawTicks, meter: meter) - start
    }

    /// Whole rows for a vertical move of `dy` points (up positive).
    public static func rawSemitones(dy: CGFloat, rowHeight: CGFloat) -> Int {
        guard dy.isFinite, rowHeight > 0 else { return 0 }
        return Int(min(max((-dy / rowHeight).rounded(), -1_000), 1_000))
    }

    /// `raw` clamped so every dragged note stays inside 0–127.
    public func semitones(raw: Int) -> Int {
        min(max(raw, -Int(lowest)), 127 - Int(highest))
    }

    /// The duration change that puts the pressed note's end on the grid; the
    /// note keeps at least one tick.
    public func durationDelta(rawTicks: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        let end = pressed.tick + pressed.duration
        var newEnd = end + rawTicks
        if snap.enabled { newEnd = snap.apply(newEnd, meter: meter) }
        newEnd = max(newEnd, pressed.tick + 1)
        return newEnd - end
    }

    /// The full target (spec §3.8: every move sends it whole); nil when
    /// nothing would change.
    public func command(
        rawTicks: Int64, dy: CGFloat, rowHeight: CGFloat, snap: Snap, meter: [MeterEntryRow]
    ) -> EditCommand? {
        switch zone {
        case .start:
            let d = Self.timeDelta(rawTicks: rawTicks, start: pressed.tick, snap: snap, meter: meter)
            guard d != 0 else { return nil }
            return copy
                ? .event(edit: .duplicate(events: events, ticks: d, semitones: 0))
                : .event(edit: .move(events: events, delta: d, toLane: nil))
        case .middle:
            let s = semitones(raw: Self.rawSemitones(dy: dy, rowHeight: rowHeight))
            guard s != 0 else { return nil }
            return copy
                ? .event(edit: .duplicate(events: events, ticks: 0, semitones: Int8(s)))
                : .event(edit: .nudge(events: events, ticks: 0, semitones: Int8(s)))
        case .end:
            let d = durationDelta(rawTicks: rawTicks, snap: snap, meter: meter)
            guard d != 0 else { return nil }
            return .event(edit: .resize(events: events, delta: d))
        }
    }

    /// The pitch the pressed note sounds at (Plan 4b Ruling 6).
    public func soundingPitch(dy: CGFloat, rowHeight: CGFloat) -> UInt8 {
        guard zone == .middle else { return pressed.pitch }
        let s = semitones(raw: Self.rawSemitones(dy: dy, rowHeight: rowHeight))
        return UInt8(min(max(Int(pressed.pitch) + s, 0), 127))
    }
}

/// VoiceOver labels (spec §3.5, Plan 4b Ruling 21).
public enum PianoRollA11y {
    public static func label(_ n: EventRow, laneName: String?, meter: [MeterEntryRow]) -> String {
        let bar = MeterMath.bar(at: n.tick, meter)
        let beatTicks = MeterMath.beatTicks(bar.meter)
        let into = n.tick - bar.start
        var s = "Note \(PitchName.name(n.pitch)), bar \(bar.number) beat \(into / beatTicks + 1)"
        if into % beatTicks != 0 { s += " tick \(into % beatTicks)" }
        s += ", velocity \(n.velocity >> 9)"
        if let laneName { s += ", \(laneName)" }
        if n.mute { s += ", muted" }
        return s
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/PianoTools.swift <<'VIZDAW_EOF'
import CoreGraphics
import Foundation
import VizDAWAppModel
import VizDAWCore

/// The pencil (Plan 4b Ruling 7).
public enum Pencil {
    /// MIDI 1.0 velocity 100 as a 16-bit velocity.
    public static let velocity: UInt16 = 100 << 9

    /// One grid step at `tick`; a quarter note with snap off.
    public static func step(at tick: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        guard snap.enabled else { return MeterMath.ppq }
        return snap.step(in: MeterMath.bar(at: tick, meter).meter)
    }

    /// A new note's start: the grid line at or before `tick`.
    public static func start(_ tick: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        snap.floor(tick, meter: meter)
    }

    /// The length painted from `start` to the pointer at `tick`: snapped and
    /// at least one step; with snap off the raw length, at least a 32nd.
    public static func length(start: Int64, to tick: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        let step = step(at: start, snap: snap, meter: meter)
        guard snap.enabled else { return tick > start ? max(tick - start, MeterMath.ppq / 8) : MeterMath.ppq / 8 }
        guard tick > start else { return step }
        return max(snap.apply(tick, meter: meter) - start, step)
    }

    public static func note(tick: Int64, pitch: UInt8, length: Int64) -> EventRow {
        EventRow(
            id: 0, tick: tick, kind: .note, mute: false, dest: nil, groupChannel: nil, pitch: pitch,
            velocity: velocity, releaseVelocity: 0, duration: length, value: 0, value2: 0,
            attribute: nil, perNote: [], refSeq: nil, refLength: nil, refLoops: 0, refTranspose: 0,
            media: nil, sourceOffset: 0, frames: 0, gainDb: 0,
            fadeIn: FadeRow(length: 0, shape: .linear), fadeOut: FadeRow(length: 0, shape: .linear),
            text: nil, textKind: nil, data: Data(), eightBit: false)
    }

    public static func command(lane: LaneKey, tick: Int64, pitch: UInt8, length: Int64) -> EditCommand {
        .event(edit: .insert(seq: lane.seq, lane: lane.lane, events: [note(tick: tick, pitch: pitch, length: length)]))
    }
}

/// The eraser (Plan 4b Ruling 8).
public enum Eraser {
    /// `erased` plus the note `hit` landed on, each once, in order.
    public static func adding(_ hit: PianoRollHit, to erased: [UInt64]) -> [UInt64] {
        guard case .note(let n) = hit, !erased.contains(n.row.id) else { return erased }
        return erased + [n.row.id]
    }

    public static func command(_ erased: [UInt64]) -> EditCommand? {
        erased.isEmpty ? nil : .event(edit: .delete(events: erased))
    }
}

public enum RubberBand {
    public static func rect(from a: CGPoint, to b: CGPoint) -> CGRect {
        CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(b.x - a.x), height: abs(b.y - a.y))
    }
}

/// The pointer's shape (Plan 4b Ruling 20); the view maps it to `NSCursor`.
public enum CursorKind: Equatable, Sendable {
    case arrow, openHand, upDown, resizeRight, crosshair, eraser
}

public enum PianoRollCursor {
    public static func kind(for hit: PianoRollHit, tool: Tool) -> CursorKind {
        if tool == .eraser { return .eraser }
        switch hit {
        case .note(let n):
            switch n.zone {
            case .start: return .openHand
            case .middle: return .upDown
            case .end: return .resizeRight
            }
        case .empty:
            return tool == .pencil ? .crosshair : .arrow
        }
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter "PianoRollGeometryTests|SnapTests" 2>&1 | tail -5
```

Expected: PASS. Hand checks if one fails: in `startZoneMovesInTimeSnappingThePressedStart` the pressed note starts at a quarter (15 360); +9 000 is 24 360, nearest quarter 30 720, so the delta is 15 360. In `endZoneResizes…` the end is 30 720; +3 000 = 33 720, nearest sixteenth (3 840 grid) is 34 560, delta 3 840. In `theActiveLaneWins…` x 200 is tick 19 200 (inside note 2, which starts at 15 360 → x 160, width 160: x 200 is 40 pt in, middle zone).

- [ ] **Step 4: Full check and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
git add app/Sources/VizDAWEditors/PianoRollGeometry.swift app/Sources/VizDAWEditors/PianoTools.swift app/Sources/VizDAWAppModel/Snap.swift app/Tests/VizDAWEditorsTests/PianoRollGeometryTests.swift app/Tests/VizDAWAppModelTests/SnapTests.swift
git commit -m "feat(editors): note geometry, cursor zones and piano-roll tool targets

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 265 tests.

---
### Task 8: Piano roll settings, shown lanes and `PianoRollModel`

**Files:**
- Create: `app/Sources/VizDAWAppModel/PianoRollSettings.swift`, `app/Sources/VizDAWAppModel/PianoRollModel.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorTypes.swift`, `app/Sources/VizDAWAppModel/EditorLayout.swift`, `app/Sources/VizDAWAppModel/EditorModel.swift`, `app/Sources/VizDAWAppModel/EditorModel+Windows.swift`
- Test: `app/Tests/VizDAWAppModelTests/PianoRollModelTests.swift` (new)

**Interfaces:**
- Consumes: `EditorModel.{laneEvents, setSelection, selection, edit, auditionOn, auditionOff, updateLayout, layout, snapshotVersion}`, `AuditionNote` (Task 4), `EditorLayout.sane`.
- Produces:
  - `enum ExpressionKind: Codable, Hashable, Sendable { pitchBend, registered(UInt8), assignable(UInt8); static choices; init(_ row: PerNoteKindRow); row: PerNoteKindRow; title: String; neutral: UInt32 }`.
  - `struct PianoRollSettings: Codable, Equatable, Sendable { showKeyboard = true; rowHeight: Double = 10; ticksPerPoint: Double = 96; expression = .pitchBend; showRegions = true; showExpression = true }`; `EditorLayout.pianoRoll: PianoRollSettings`.
  - `DetailState.lanes: [UInt64]?` and `DetailState.init(kind:target:lanes: = nil)`.
  - `EditorModel.laneRows(_ seq: UInt64) -> [LaneRow]` (cached per snapshot version).
  - `enum DetailSlot: Hashable, Sendable { pane, window(UUID) }`.
  - `@Observable @MainActor final class PianoRollModel { unowned let document; editor; chosenLane: LaneKey?; sounding: AuditionNote? (read-only); init(document:read:write:); static pane(_:); static window(_:of:); state; target; seq; sequenceLanes: [LaneRow]; shownLanes: [LaneKey]; isShown(_:); toggleShown(_:); editLane: LaneKey?; colorIndex(_:) -> Int; row(_:) -> LaneRow?; notes(_:) -> [EventRow]; regions(_:) -> [EventRow]; focusRange: TickRange?; dest(of:in:) -> UInt64?; sound(_:velocity:dest:); silence(); select(_:adding:); toggle(_:in:); deleteSelected() }`.

Rulings 6, 9, 10, 11, 22.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/PianoRollModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Song (3): Groove (30, no destination, no MIDI), Chords (31, destination
/// 5, notes 1 = C4 at 0 and 2 = E4 at one quarter) and Bass (32,
/// destination 6, note 3 = E2 at 0).
func rollFake() -> FakeSession {
    let f = editorFake()
    var chords = lane(31, "Chords")
    chords.midiDest = 5
    chords.midiEventCount = 2
    var bass = lane(32, "Bass")
    bass.midiDest = 6
    bass.midiEventCount = 1
    f.laneTable[3] = [lane(30, "Groove"), chords, bass]
    f.snap.laneRevs.append(LaneRev(seq: 3, lane: 32, rev: 3))
    f.laneEventTable["3/31"] = LaneEvents(
        seq: 3, lane: 31, rev: 2, events: [noteRow(1, tick: 0, pitch: 60), noteRow(2, tick: 15_360, pitch: 64)])
    f.laneEventTable["3/32"] = LaneEvents(seq: 3, lane: 32, rev: 3, events: [noteRow(3, tick: 0, pitch: 40)])
    return f
}

@MainActor
@Suite struct PianoRollModelTests {
    func make(_ target: EditorTarget, lanes: [UInt64]? = nil) throws -> (FakeSession, DocumentModel, PianoRollModel) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(target, kind: .pianoRoll)
        if let lanes { m.editor.updateLayout { $0.detail?.lanes = lanes } }
        return (f, m, PianoRollModel.pane(m))
    }

    func log(_ f: FakeSession) -> [String] {
        f.auditions.map { "\($0.on ? "on" : "off") \($0.pitch)" }
    }

    @Test func aLaneTargetShowsItsLanePlusExtras() throws {
        let (_, _, r) = try make(.lane(seq: 3, lane: 31))
        #expect(r.shownLanes == [LaneKey(3, 31)])
        r.toggleShown(32)
        #expect(r.shownLanes == [LaneKey(3, 31), LaneKey(3, 32)])
        r.toggleShown(31)
        #expect(r.isShown(31), "the target lane always shows")
        r.toggleShown(32)
        #expect(r.shownLanes == [LaneKey(3, 31)])
    }

    @Test func aSequenceTargetShowsLanesWithNotesOrADestination() throws {
        let (_, m, r) = try make(.sequence(3))
        #expect(r.shownLanes == [LaneKey(3, 31), LaneKey(3, 32)])
        r.toggleShown(31)
        #expect(r.shownLanes == [LaneKey(3, 32)])
        #expect(m.editor.layout.detail?.lanes == [32])
        r.toggleShown(30)
        #expect(r.shownLanes == [LaneKey(3, 30), LaneKey(3, 32)], "sequence order")
    }

    @Test func coloursFollowTheLaneIndexInItsSequence() throws {
        let (_, _, r) = try make(.sequence(3))
        #expect(r.colorIndex(LaneKey(3, 30)) == 0)
        #expect(r.colorIndex(LaneKey(3, 32)) == 2)
    }

    @Test func theEditLaneFollowsTheLastChosenShownLane() throws {
        let (_, _, r) = try make(.lane(seq: 3, lane: 31), lanes: [32])
        #expect(r.editLane == LaneKey(3, 31))
        r.select([(LaneKey(3, 32), 3)], adding: false)
        #expect(r.editLane == LaneKey(3, 32))
    }

    @Test func theEditLaneFallsBackWhenItsLaneIsHidden() throws {
        let (_, _, r) = try make(.lane(seq: 3, lane: 31), lanes: [32])
        r.chosenLane = LaneKey(3, 32)
        r.toggleShown(32)
        #expect(r.editLane == LaneKey(3, 31))
    }

    @Test func selectingTogglingAndDeletingNotes() throws {
        let (f, m, r) = try make(.lane(seq: 3, lane: 31), lanes: [32])
        r.select([(LaneKey(3, 31), 1), (LaneKey(3, 32), 3)], adding: false)
        #expect(m.editor.selection.events == [1, 3])
        #expect(m.editor.selection.lanes == [LaneRef(seq: 3, lane: 31), LaneRef(seq: 3, lane: 32)])
        r.toggle(1, in: LaneKey(3, 31))
        #expect(m.editor.selection.events == [3])
        r.deleteSelected()
        #expect(f.edits.last == .event(edit: .delete(events: [3])))
    }

    @Test func soundingANewPitchEndsTheOldOne() throws {
        let (f, _, r) = try make(.lane(seq: 3, lane: 31))
        r.sound(60, velocity: 0x8000, dest: 5)
        r.sound(60, velocity: 0x8000, dest: 5)
        r.sound(62, velocity: 0x8000, dest: 5)
        r.silence()
        #expect(log(f) == ["on 60", "off 60", "on 62", "off 62"])
        #expect(r.sounding == nil)
    }

    @Test func aLaneWithoutDestinationIsSilent() throws {
        let (f, m, r) = try make(.lane(seq: 3, lane: 30))
        let groove = LaneKey(3, 30)
        var row = noteRow(9, tick: 0)
        #expect(r.dest(of: row, in: groove) == nil)
        r.sound(60, velocity: 0x8000, dest: r.dest(of: row, in: groove))
        r.silence()
        #expect(f.auditions.isEmpty)
        #expect(m.editor.status == nil)
        row.dest = 9
        #expect(r.dest(of: row, in: groove) == 9, "the event's own destination wins")
    }

    @Test func aShownLaneThatVanishesIsDropped() throws {
        let (f, m, r) = try make(.lane(seq: 3, lane: 31), lanes: [32])
        nextSnapshot(f) { $0.laneRevs.removeAll { $0.lane == 32 } }
        f.laneTable[3]?.removeAll { $0.id == 32 }
        m.rename(1, to: "Kit")
        #expect(m.editor.layout.detail?.lanes == nil)
        #expect(m.editor.layout.detail != nil, "the target lane is still there")
        #expect(r.shownLanes == [LaneKey(3, 31)])
    }

    @Test func aBlockTargetHasAFocusRange() throws {
        let (_, _, r) = try make(.block(seq: 3, lane: 31, start: 61_440, end: 122_880))
        #expect(r.focusRange == TickRange(start: 61_440, end: 122_880))
        #expect(r.shownLanes == [LaneKey(3, 31)])
    }

    @Test func laneRowsAreCachedPerSnapshotVersion() throws {
        let (f, m, _) = try make(.lane(seq: 3, lane: 31))
        let before = f.lanesCalls
        _ = m.editor.laneRows(2)
        _ = m.editor.laneRows(2)
        #expect(f.lanesCalls == before + 1)
        m.rename(1, to: "Kit")
        _ = m.editor.laneRows(2)
        #expect(f.lanesCalls == before + 2)
    }

    @Test func settingsAndShownLanesDecodeTolerantly() throws {
        let l = EditorLayout.decode(#"{"pianoRoll":{"rowHeight":999,"showKeyboard":false}}"#)
        #expect(l.pianoRoll.rowHeight == 40)
        #expect(!l.pianoRoll.showKeyboard)
        #expect(l.pianoRoll.ticksPerPoint == 96)
        #expect(EditorLayout.decode(#"{"pianoRoll":7}"#).pianoRoll == PianoRollSettings())
        let d = DetailState(kind: .pianoRoll, target: .sequence(3), lanes: [31])
        #expect(try JSONDecoder().decode(DetailState.self, from: JSONEncoder().encode(d)) == d)
        let old = #"{"kind":"pianoRoll","target":{"sequence":{"_0":3}}}"#
        #expect(try JSONDecoder().decode(DetailState.self, from: Data(old.utf8)).lanes == nil, "4a layouts")
        for k in ExpressionKind.choices { #expect(ExpressionKind(k.row) == k) }
        #expect(ExpressionKind.pitchBend.neutral == 0x8000_0000)
        #expect(ExpressionKind.registered(74).title == "Registered 74 (Timbre)")
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter PianoRollModelTests 2>&1 | tail -5
```

Expected: compile errors (`PianoRollModel`, `lanes:` unknown). If the 4a encoding of `EditorTarget.sequence` differs from `{"sequence":{"_0":3}}`, print `String(decoding: JSONEncoder().encode(EditorTarget.sequence(3)), as: UTF8.self)` once and use that shape in `old`.

- [ ] **Step 2: Settings types**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/PianoRollSettings.swift <<'VIZDAW_EOF'
import VizDAWCore

/// A MIDI 2.0 per-note controller the expression pane shows (Plan 4b
/// Ruling 14).
public enum ExpressionKind: Codable, Hashable, Sendable {
    case pitchBend
    case registered(UInt8)
    case assignable(UInt8)

    public static let choices: [ExpressionKind] = [
        .pitchBend, .registered(1), .registered(2), .registered(3), .registered(7), .registered(10),
        .registered(74), .assignable(1), .assignable(2), .assignable(3), .assignable(4),
    ]

    static let registeredNames: [UInt8: String] = [
        1: "Modulation", 2: "Breath", 3: "Pitch 7.25", 7: "Volume", 10: "Pan", 74: "Timbre",
    ]

    public init(_ row: PerNoteKindRow) {
        switch row {
        case .pitchBend: self = .pitchBend
        case .registered(let i): self = .registered(i)
        case .assignable(let i): self = .assignable(i)
        }
    }

    public var row: PerNoteKindRow {
        switch self {
        case .pitchBend: .pitchBend
        case .registered(let i): .registered(index: i)
        case .assignable(let i): .assignable(index: i)
        }
    }

    public var title: String {
        switch self {
        case .pitchBend: "Per-Note Pitch Bend"
        case .registered(let i): "Registered \(i)" + (Self.registeredNames[i].map { " (\($0))" } ?? "")
        case .assignable(let i): "Assignable \(i)"
        }
    }

    /// A note's value before its first point: centre for pitch bend.
    public var neutral: UInt32 { self == .pitchBend ? 0x8000_0000 : 0 }
}

/// View settings every piano roll shares, saved with the project (Plan 4b
/// Ruling 11). Decoding is tolerant like `EditorLayout`'s.
public struct PianoRollSettings: Codable, Equatable, Sendable {
    public var showKeyboard = true
    public var rowHeight: Double = 10
    /// The piano roll's own zoom (a 4/4 bar is 640 pt at 96).
    public var ticksPerPoint: Double = 96
    public var expression: ExpressionKind = .pitchBend
    public var showRegions = true
    public var showExpression = true

    public static let rowHeightRange: ClosedRange<Double> = 4...40

    public init() {}

    enum CodingKeys: String, CodingKey {
        case showKeyboard, rowHeight, ticksPerPoint, expression, showRegions, showExpression
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func field<T: Decodable>(_ key: CodingKeys, _ fallback: T) -> T {
            ((try? c.decodeIfPresent(T.self, forKey: key)) ?? nil) ?? fallback
        }
        let d = PianoRollSettings()
        showKeyboard = field(.showKeyboard, d.showKeyboard)
        rowHeight = EditorLayout.sane(field(.rowHeight, d.rowHeight), Self.rowHeightRange, d.rowHeight)
        ticksPerPoint = EditorLayout.sane(
            field(.ticksPerPoint, d.ticksPerPoint), EditorLayout.ticksPerPointRange, d.ticksPerPoint)
        expression = field(.expression, d.expression)
        showRegions = field(.showRegions, d.showRegions)
        showExpression = field(.showExpression, d.showExpression)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/EditorTypes.swift`:

Find:
```swift
/// A detail editor and its target.
public struct DetailState: Codable, Hashable, Sendable {
    public var kind: DetailEditorKind
    public var target: EditorTarget

    public init(kind: DetailEditorKind, target: EditorTarget) {
        self.kind = kind
        self.target = target
    }
}
```
Replace:
```swift
/// A detail editor and its target.
public struct DetailState: Codable, Hashable, Sendable {
    public var kind: DetailEditorKind
    public var target: EditorTarget
    /// Piano roll: lanes shown besides the target's lane; for a sequence
    /// target exactly the lanes shown (nil: the default set). Optional, so
    /// 4a layouts decode (Plan 4b Rulings 9, 11).
    public var lanes: [UInt64]?

    public init(kind: DetailEditorKind, target: EditorTarget, lanes: [UInt64]? = nil) {
        self.kind = kind
        self.target = target
        self.lanes = lanes
    }
}
```

In `app/Sources/VizDAWAppModel/EditorLayout.swift`:

Find:
```swift
    /// Follow playback (page mode, Plan Ruling 18).
    public var follow = true

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow
    }
```
Replace:
```swift
    /// Follow playback (page mode, Plan Ruling 18).
    public var follow = true
    /// Every piano roll's view settings (Plan 4b Ruling 11).
    public var pianoRoll = PianoRollSettings()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, detail, lastKind, windows, sequenceOrder, blockRule
        case lightMode, snap, laneHeight, ticksPerPoint, follow, pianoRoll
    }
```

Find:
```swift
        follow = field(.follow, d.follow)
    }
```
Replace:
```swift
        follow = field(.follow, d.follow)
        pianoRoll = field(.pianoRoll, d.pianoRoll)
    }
```

- [ ] **Step 3: Lane rows of any sequence; pruning extra lanes**

In `app/Sources/VizDAWAppModel/EditorModel.swift`:

Find:
```swift
    @ObservationIgnored private var sequenceRows: [SeqRow] = []
```
Replace:
```swift
    @ObservationIgnored private var sequenceRows: [SeqRow] = []
    @ObservationIgnored private var laneRowCache: [UInt64: (version: UInt64, rows: [LaneRow])] = [:]
```

Find:
```swift
    // MARK: Edits

    /// Shown when an edit, undo or redo arrives while a drag is open (it
```
Replace:
```swift
    /// The lanes of any sequence (a piano roll may show one the sidebar
    /// does not), cached per snapshot version; Busy keeps the old rows.
    public func laneRows(_ seq: UInt64) -> [LaneRow] {
        if let c = laneRowCache[seq], c.version == snapshotVersion { return c.rows }
        do {
            let rows = try session.lanes(seq: seq)
            laneRowCache[seq] = (snapshotVersion, rows)
            return rows
        } catch VizError.Busy {
            return laneRowCache[seq]?.rows ?? []
        } catch {
            reportFromRead(error)
            return []
        }
    }

    // MARK: Edits

    /// Shown when an edit, undo or redo arrives while a drag is open (it
```

In `app/Sources/VizDAWAppModel/EditorModel+Windows.swift`:

Find:
```swift
        let detailGone = layout.detail.map { gone($0.target) } ?? false
        let windowsGone = layout.windows.contains { gone($0.detail.target) }
        guard detailGone || windowsGone else { return }
        updateLayout { l in
            if detailGone { l.detail = nil }
            l.windows.removeAll { gone($0.detail.target) }
        }
```
Replace:
```swift
        /// Extra lanes that are gone leave a piano roll silently (Plan 4b
        /// Review Focus 5).
        func cleaned(_ d: DetailState) -> DetailState {
            guard let extra = d.lanes else { return d }
            var c = d
            let kept = extra.filter { lanes.contains(LaneKey(d.target.seq, $0)) }
            c.lanes = kept.isEmpty ? nil : kept
            return c
        }
        let detailGone = layout.detail.map { gone($0.target) } ?? false
        let windowsGone = layout.windows.contains { gone($0.detail.target) }
        let lanesGone =
            (layout.detail.map { cleaned($0) != $0 } ?? false)
            || layout.windows.contains { cleaned($0.detail) != $0.detail }
        guard detailGone || windowsGone || lanesGone else { return }
        updateLayout { l in
            if detailGone { l.detail = nil }
            l.windows.removeAll { gone($0.detail.target) }
            l.detail = l.detail.map(cleaned)
            for i in l.windows.indices { l.windows[i].detail = cleaned(l.windows[i].detail) }
        }
```

- [ ] **Step 4: `PianoRollModel`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/PianoRollModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// Where a piano roll lives (Plan 4b Ruling 22).
public enum DetailSlot: Hashable, Sendable {
    case pane
    case window(UUID)
}

/// One piano roll (spec §4b): which lanes it shows, which lane new notes go
/// to, lane colours and the note it sounds. The detail pane and every
/// torn-off window have their own; document, selection and settings are
/// shared. Owned by its `DocumentModel` (Plan 4b Ruling 22).
@Observable
@MainActor
public final class PianoRollModel {
    public unowned let document: DocumentModel
    public var editor: EditorModel { document.editor }
    @ObservationIgnored private let read: @MainActor () -> DetailState?
    @ObservationIgnored private let write: @MainActor (DetailState) -> Void
    /// The lane of the last pressed or selected note (Plan 4b Ruling 9).
    public var chosenLane: LaneKey?
    /// The note sounding while a note or key is pressed.
    public private(set) var sounding: AuditionNote?

    public init(
        document: DocumentModel, read: @escaping @MainActor () -> DetailState?,
        write: @escaping @MainActor (DetailState) -> Void
    ) {
        self.document = document
        self.read = read
        self.write = write
    }

    /// The piano roll of the detail pane.
    public static func pane(_ document: DocumentModel) -> PianoRollModel {
        PianoRollModel(
            document: document,
            read: { [unowned document] in document.editor.layout.detail },
            write: { [unowned document] d in document.editor.updateLayout { $0.detail = d } })
    }

    /// The piano roll of torn-off window `id`.
    public static func window(_ id: UUID, of document: DocumentModel) -> PianoRollModel {
        PianoRollModel(
            document: document,
            read: { [unowned document] in document.editor.layout.windows.first { $0.id == id }?.detail },
            write: { [unowned document] d in
                document.editor.updateLayout { l in
                    if let i = l.windows.firstIndex(where: { $0.id == id }) { l.windows[i].detail = d }
                }
            })
    }

    // MARK: What is shown

    public var state: DetailState? { read() }
    public var target: EditorTarget? { state?.target }
    public var seq: UInt64? { target?.seq }

    /// Every lane of the target's sequence.
    public var sequenceLanes: [LaneRow] { seq.map { editor.laneRows($0) } ?? [] }

    /// The lanes shown, in sequence order (Plan 4b Ruling 9).
    public var shownLanes: [LaneKey] {
        guard let t = target else { return [] }
        let rows = editor.laneRows(t.seq)
        let wanted: Set<UInt64>
        if let lane = t.lane {
            wanted = Set([lane] + (state?.lanes ?? []))
        } else if let exact = state?.lanes {
            wanted = Set(exact)
        } else {
            wanted = Set(rows.filter { $0.midiDest != nil || $0.midiEventCount > 0 || $0.regionCount > 0 }.map(\.id))
        }
        return rows.filter { wanted.contains($0.id) }.map { LaneKey(t.seq, $0.id) }
    }

    public func isShown(_ lane: UInt64) -> Bool { shownLanes.contains { $0.lane == lane } }

    /// Shows or hides `lane`; the target's own lane always shows.
    public func toggleShown(_ lane: UInt64) {
        guard var s = state, s.target.lane != lane else { return }
        var set = Set(s.target.lane == nil ? shownLanes.map(\.lane) : (s.lanes ?? []))
        if set.contains(lane) { set.remove(lane) } else { set.insert(lane) }
        let ordered = sequenceLanes.map(\.id).filter(set.contains)
        s.lanes = ordered.isEmpty && s.target.lane != nil ? nil : ordered
        write(s)
    }

    /// The lane the pencil inserts into.
    public var editLane: LaneKey? {
        let shown = shownLanes
        if let c = chosenLane, shown.contains(c) { return c }
        if let t = target, let l = t.lane { return LaneKey(t.seq, l) }
        return shown.first
    }

    /// The lane's palette index: its position in its sequence.
    public func colorIndex(_ key: LaneKey) -> Int {
        editor.laneRows(key.seq).firstIndex { $0.id == key.lane } ?? 0
    }

    public func row(_ key: LaneKey) -> LaneRow? {
        editor.laneRows(key.seq).first { $0.id == key.lane }
    }

    public func notes(_ key: LaneKey) -> [EventRow] {
        editor.laneEvents(key)?.events.filter { $0.kind == .note } ?? []
    }

    public func regions(_ key: LaneKey) -> [EventRow] {
        editor.laneEvents(key)?.events.filter { $0.kind == .audioRegion } ?? []
    }

    /// A block target's range (Plan 4b Ruling 10).
    public var focusRange: TickRange? {
        guard case let .block(_, _, start, end)? = target else { return nil }
        return TickRange(start: start, end: max(end, start + 1))
    }

    /// Where a note sounds: its own destination, else its lane's.
    public func dest(of n: EventRow, in key: LaneKey) -> UInt64? {
        n.dest ?? row(key)?.midiDest
    }

    // MARK: Sound (Plan 4b Ruling 6)

    /// Sounds `pitch` (ending the previous note); nil `dest` is silence.
    public func sound(_ pitch: UInt8, velocity: UInt16, dest: UInt64?) {
        let next = dest.map { AuditionNote(dest: $0, pitch: pitch) }
        guard next != sounding else { return }
        silence()
        guard let next else { return }
        editor.auditionOn(next, velocity: velocity)
        sounding = next
    }

    public func silence() {
        guard let s = sounding else { return }
        sounding = nil
        editor.auditionOff(s)
    }

    // MARK: Selection

    /// Selects `notes` (with their lanes); `adding` keeps the selection.
    public func select(_ notes: [(lane: LaneKey, id: UInt64)], adding: Bool) {
        var s = adding ? editor.selection : SelectionRow(timeRange: nil, lanes: [], events: [])
        for n in notes {
            if !s.events.contains(n.id) { s.events.append(n.id) }
            if !s.lanes.contains(n.lane.ref) { s.lanes.append(n.lane.ref) }
        }
        editor.setSelection(s)
        if let last = notes.last { chosenLane = last.lane }
    }

    /// Shift-click: adds or removes one note.
    public func toggle(_ id: UInt64, in lane: LaneKey) {
        var s = editor.selection
        if let i = s.events.firstIndex(of: id) {
            s.events.remove(at: i)
        } else {
            s.events.append(id)
            if !s.lanes.contains(lane.ref) { s.lanes.append(lane.ref) }
            chosenLane = lane
        }
        editor.setSelection(s)
    }

    /// Delete/Backspace: the selected notes of the shown lanes.
    public func deleteSelected() {
        let shown = Set(shownLanes.flatMap { notes($0) }.map(\.id))
        let ids = editor.selection.events.filter(shown.contains)
        guard !ids.isEmpty else { return }
        editor.edit(.event(edit: .delete(events: ids)))
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter PianoRollModelTests 2>&1 | tail -5
```

Expected: PASS (12 tests).

- [ ] **Step 5: Full check and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
git add app/Sources/VizDAWAppModel/PianoRollSettings.swift app/Sources/VizDAWAppModel/PianoRollModel.swift app/Sources/VizDAWAppModel/EditorTypes.swift app/Sources/VizDAWAppModel/EditorLayout.swift app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/EditorModel+Windows.swift app/Tests/VizDAWAppModelTests/PianoRollModelTests.swift
git commit -m "feat(app): piano roll model, settings and shown lanes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 277 tests.

---

### Task 9: `PianoRollView` — notes, lanes, tools, sound, accessibility

**Files:**
- Create: `app/Sources/VizDAWEditors/PianoRollView.swift`
- Test: `app/Tests/VizDAWAppModelTests/PianoRollViewTests.swift` (new)

**Interfaces:**
- Consumes: `EditorCanvas` (subclass surface: `meter`, `snap`, `selectedRange`, `contentHeight`, `accessibilityName`, `drawContent`, `drawOverlay`, `contentMouseDown/Dragged/Up`, `contentCancel`, `accessibilityItems`, `verticalOffset`, `clampVertical`, `axis`, `onAxisChange`, `playheadSource`, `onRulerRange`, `onRulerClick`, `onToggleSnap`, `setPlaying`, `follow`, `reload`), `ToolMachine`, Task 7's geometry and tools, Task 8's `PianoRollModel`.
- Produces: `@MainActor public final class PianoRollView: EditorCanvas { init(model:); model; pitchAxis: PitchAxis (read-only); onPitchScroll: ((PitchAxis) -> Void)?; onTimeAxis: ((TimeAxis) -> Void)?; laneNotes() -> [LaneNotes]; static cursor(_:) -> NSCursor }`; `@MainActor public enum PianoRollPalette { static func color(_ index: Int) -> NSColor }`.

Rulings 4–10, 20, 21. The drag follows the 4a pattern (`ArrangementView`): a `ToolMachine` turns pointer events into begin/move/end; each move sends the full target through `updateGesture`; mouse-up sends it through `endGesture`; Escape (`contentCancel`) aborts. The note sounds from mouse-down to mouse-up, independently of the gesture (so a Busy final edit never holds it).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/PianoRollViewTests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// Chords (31) of `rollFake()` in the detail pane: notes 1 (C4 at 0) and 2
/// (E4 at one quarter), 160 pt per quarter at the default zoom.
@MainActor
private func roll() throws -> (FakeSession, DocumentModel, PianoRollView) {
    let f = rollFake()
    let m = try DocumentModel(session: f)
    m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
    let v = PianoRollView(model: PianoRollModel.pane(m))
    v.frame = NSRect(x: 0, y: 0, width: 800, height: 400)
    v.layout()
    return (f, m, v)
}

/// A point inside a zone of note `id`, in content coordinates.
@MainActor
private func point(_ v: PianoRollView, _ id: UInt64, _ zone: NoteZone) -> CGPoint {
    let n = v.laneNotes().flatMap(\.notes).first { $0.id == id }!
    let r = PianoRollGeometry.rect(n, time: v.axis, pitch: v.pitchAxis)
    switch zone {
    case .start: return CGPoint(x: r.minX + 2, y: r.midY)
    case .middle: return CGPoint(x: r.midX, y: r.midY)
    case .end: return CGPoint(x: r.maxX - 2, y: r.midY)
    }
}

private func log(_ f: FakeSession) -> [String] {
    f.auditions.map { "\($0.on ? "on" : "off") \($0.pitch)" }
}

@MainActor
@Suite struct PianoRollViewTests {
    @Test func pressingANoteSelectsAndSoundsItUntilRelease() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        #expect(log(f) == ["on 60"])
        v.contentMouseUp(p, modifiers: [])
        #expect(log(f) == ["on 60", "off 60"])
        #expect(m.editor.selection.events == [1])
    }

    @Test func aMiddleDragTransposesAndResoundsAtTheNewPitch() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        let up2 = CGPoint(x: p.x, y: p.y - 2 * v.pitchAxis.rowHeight)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(up2, modifiers: [])
        #expect(m.editor.gesture?.label == "Transpose Notes")
        #expect(f.gestureEdits.last == .event(edit: .nudge(events: [1], ticks: 0, semitones: 2)))
        v.contentMouseUp(up2, modifiers: [])
        #expect(m.editor.gesture == nil)
        #expect(log(f) == ["on 60", "off 60", "on 62", "off 62"])
    }

    @Test func anOptionStartDragCopiesInTime() throws {
        let (f, _, v) = try roll()
        let p = point(v, 2, .start)
        let right = CGPoint(x: p.x + 160, y: p.y)
        v.contentMouseDown(p, clicks: 1, modifiers: [.option])
        v.contentMouseDragged(right, modifiers: [.option])
        v.contentMouseUp(right, modifiers: [.option])
        #expect(f.gestureEdits.last == .event(edit: .duplicate(events: [2], ticks: 15_360, semitones: 0)))
    }

    @Test func anEndDragResizes() throws {
        let (f, _, v) = try roll()
        let p = point(v, 1, .end)
        // +100 pt is 9 600 ticks: the end (15 360) goes to 24 960, nearest quarter 30 720.
        let right = CGPoint(x: p.x + 100, y: p.y)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(right, modifiers: [])
        v.contentMouseUp(right, modifiers: [])
        #expect(f.gestureEdits.last == .event(edit: .resize(events: [1], delta: 15_360)))
    }

    @Test func pencilClickInsertsAGridNoteInTheEditLane() throws {
        let (f, m, v) = try roll()
        m.editor.tool = .pencil
        // x 400 is tick 38 400: the note starts on the quarter at 30 720.
        let p = CGPoint(x: 400, y: v.pitchAxis.center(67))
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseUp(p, modifiers: [])
        #expect(f.edits.last == Pencil.command(lane: LaneKey(3, 31), tick: 30_720, pitch: 67, length: 15_360))
        #expect(log(f) == ["on 67", "off 67"], "the new pitch sounds while pressed")
    }

    @Test func pencilDragPaintsALength() throws {
        let (f, m, v) = try roll()
        m.editor.tool = .pencil
        let y = v.pitchAxis.center(67)
        v.contentMouseDown(CGPoint(x: 400, y: y), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 700, y: y), modifiers: [])
        v.contentMouseUp(CGPoint(x: 700, y: y), modifiers: [])
        // x 700 is tick 67 200; nearest quarter 61 440: 30 720 long.
        #expect(f.gestureEdits.last == Pencil.command(lane: LaneKey(3, 31), tick: 30_720, pitch: 67, length: 30_720))
        #expect(m.editor.gesture == nil)
    }

    @Test func theEraserDeletesEveryNoteItCrosses() throws {
        let (f, m, v) = try roll()
        m.editor.tool = .eraser
        let a = point(v, 1, .middle)
        let b = point(v, 2, .middle)
        v.contentMouseDown(a, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: a.x + 10, y: a.y), modifiers: [])
        v.contentMouseDragged(b, modifiers: [])
        v.contentMouseUp(b, modifiers: [])
        #expect(f.gestureEdits.last == .event(edit: .delete(events: [1, 2])))
        #expect(f.auditions.isEmpty, "the eraser is silent")
    }

    @Test func aRubberBandSelectsNotes() throws {
        let (_, m, v) = try roll()
        let a = point(v, 1, .start)
        let b = point(v, 2, .end)
        // From empty space above E4 to below C4.
        let from = CGPoint(x: a.x - 1, y: b.y - 15)
        let to = CGPoint(x: b.x, y: a.y + 15)
        v.contentMouseDown(from, clicks: 1, modifiers: [])
        v.contentMouseDragged(to, modifiers: [])
        v.contentMouseUp(to, modifiers: [])
        #expect(Set(m.editor.selection.events) == [1, 2])
        #expect(m.editor.gesture == nil, "selecting is not an edit")
    }

    @Test func aNoteDragEndsItsSoundEvenWhenTheFinalEditWaits() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        let row = v.pitchAxis.rowHeight
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: p.x, y: p.y - row), modifiers: [])
        f.gestureErrors = [.Busy(message: "busy")]
        f.auditionErrors = [.Busy(message: "busy")]
        v.contentMouseUp(CGPoint(x: p.x, y: p.y - 2 * row), modifiers: [])
        #expect(m.editor.gesture?.finishing == true)
        #expect(m.editor.hasPendingNoteOffs)
        m.idle()
        #expect(m.editor.gesture == nil)
        #expect(!m.editor.hasPendingNoteOffs)
        #expect(log(f) == ["on 60", "off 60", "on 61", "off 61"])
    }

    @Test func escapeCancelsTheDragAndSilences() throws {
        let (f, m, v) = try roll()
        let p = point(v, 1, .middle)
        v.contentMouseDown(p, clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: p.x, y: p.y - 30), modifiers: [])
        v.contentCancel()
        #expect(m.editor.gesture == nil)
        #expect(f.calls.last == "abort")
        #expect(log(f).last?.hasPrefix("off") == true)
    }

    @Test func accessibilityListsVisibleNotes() throws {
        let (_, _, v) = try roll()
        #expect(v.accessibilityItems().map(\.label) == [
            "Note C4, bar 1 beat 1, velocity 64", "Note E4, bar 1 beat 2, velocity 64",
        ])
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter PianoRollViewTests 2>&1 | tail -5
```

Expected: compile errors (`PianoRollView` unknown).

- [ ] **Step 2: Implement the view**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/PianoRollView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// Lane colours (Plan 4b Ruling 9).
@MainActor
public enum PianoRollPalette {
    static let colors: [NSColor] = [
        .systemBlue, .systemOrange, .systemGreen, .systemPink, .systemPurple, .systemTeal, .systemYellow,
        .systemBrown,
    ]

    public static func color(_ index: Int) -> NSColor {
        colors[((index % colors.count) + colors.count) % colors.count]
    }
}

/// The piano roll's note area (spec §4b, parent §9): notes of the shown
/// lanes over a pitch axis, cursor zones, pencil, eraser, rubber band,
/// notes that sound while pressed and dragged.
@MainActor
public final class PianoRollView: EditorCanvas {
    public let model: PianoRollModel
    var editor: EditorModel { model.editor }
    var document: DocumentModel { model.document }

    public private(set) var pitchAxis = PitchAxis()
    /// The keyboard follows the vertical scroll and row height.
    public var onPitchScroll: ((PitchAxis) -> Void)?
    /// The region strip and expression pane follow the time axis.
    public var onTimeAxis: ((TimeAxis) -> Void)?

    private var machine = ToolMachine<PianoRollHit>()
    private var centered = false
    private var tracking: NSTrackingArea?

    private enum Drag {
        case notes(NoteDrag, startTick: Int64, startY: CGFloat)
        case pencil(lane: LaneKey, start: Int64, pitch: UInt8)
        case erase([UInt64])
        case band(from: CGPoint, adding: Bool)
    }

    private var drag: Drag?
    /// The rubber band being drawn, in content coordinates.
    private(set) var band: CGRect?

    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        let settings = model.editor.layout.pianoRoll
        pitchAxis = PitchAxis(rowHeight: CGFloat(settings.rowHeight))
        axis = TimeAxis(ticksPerPoint: settings.ticksPerPoint)
        playheadSource = { [weak self] now in
            guard let self, let seq = self.model.seq, self.document.transport.seq == seq else { return nil }
            return self.document.clock.tick(atHostTimeNs: now)
        }
        onRulerRange = { [weak self] r in
            guard let self else { return }
            self.editor.selectRange(r, lanes: self.model.shownLanes)
        }
        onRulerClick = { [weak self] t in self?.document.locate(t) }
        onToggleSnap = { [weak self] in self?.editor.updateLayout { $0.snap.enabled.toggle() } }
        onAxisChange = { [weak self] a in
            guard let self else { return }
            self.onTimeAxis?(a)
            if a.ticksPerPoint != self.editor.layout.pianoRoll.ticksPerPoint {
                self.editor.updateLayout { $0.pianoRoll.ticksPerPoint = a.ticksPerPoint }
            }
        }
        refresh()
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var meter: [MeterEntryRow] { model.seq.flatMap { editor.info($0)?.meter } ?? [] }
    public override var snap: Snap { editor.layout.snap }
    public override var selectedRange: TickRange? { editor.selection.timeRange ?? model.focusRange }
    public override var contentHeight: CGFloat { pitchAxis.contentHeight }
    public override var accessibilityName: String { "Piano Roll" }

    public override var verticalOffset: CGFloat {
        didSet {
            pitchAxis.offset = verticalOffset
            onPitchScroll?(pitchAxis)
        }
    }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.layout
            _ = editor.selection
            _ = editor.tool
            _ = document.playKey
            _ = model.chosenLane
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.refresh()
                self?.observe()
            }
        }
    }

    func refresh() {
        let s = editor.layout.pianoRoll
        if CGFloat(s.rowHeight) != pitchAxis.rowHeight {
            pitchAxis = pitchAxis.withRowHeight(CGFloat(s.rowHeight), keepingY: content.bounds.height / 2)
            verticalOffset = clampVertical(pitchAxis.offset)
            onPitchScroll?(pitchAxis)
        }
        if s.ticksPerPoint != axis.ticksPerPoint {
            axis = TimeAxis(ticksPerPoint: s.ticksPerPoint, originTick: axis.originTick)
        }
        follow = editor.layout.follow
        setPlaying(document.playKey.playing && document.playKey.seq == model.seq)
        reload()
    }

    /// The first layout centres the shown notes and scrolls to a block
    /// target (Plan 4b Ruling 10).
    public override func layout() {
        super.layout()
        guard !centered, content.bounds.height > 0 else { return }
        centered = true
        let pitches = model.shownLanes.flatMap { model.notes($0) }.map(\.pitch)
        var middle: UInt8 = 60
        if let lo = pitches.min(), let hi = pitches.max() { middle = UInt8((Int(lo) + Int(hi)) / 2) }
        verticalOffset = clampVertical(pitchAxis.centering(middle, height: content.bounds.height).offset)
        if let r = model.focusRange {
            axis = axis.scrolled(toTick: Double(r.start) - 16 * axis.ticksPerPoint)
        }
        onTimeAxis?(axis)
    }

    func laneNotes() -> [LaneNotes] {
        model.shownLanes.map { LaneNotes(lane: $0, notes: model.notes($0)) }
    }

    // MARK: Drawing

    public override func drawContent(in dirty: CGRect) {
        drawRows(dirty)
        let lanes = laneNotes()
        let active = model.editLane
        for l in lanes where l.lane != active { draw(l, active: false, dirty) }
        for l in lanes where l.lane == active { draw(l, active: true, dirty) }
    }

    /// Black-key rows shaded, a line under every C.
    private func drawRows(_ dirty: CGRect) {
        for p in pitchAxis.visible(height: content.bounds.height) {
            let y = pitchAxis.top(p)
            if PitchName.isBlack(p) {
                NSColor.labelColor.withAlphaComponent(0.05).setFill()
                CGRect(x: dirty.minX, y: y, width: dirty.width, height: pitchAxis.rowHeight).fill()
            }
            if p % 12 == 0 {
                NSColor.separatorColor.setFill()
                CGRect(x: dirty.minX, y: y + pitchAxis.rowHeight - 0.5, width: dirty.width, height: 1).fill()
            }
        }
    }

    private func draw(_ l: LaneNotes, active: Bool, _ dirty: CGRect) {
        let color = PianoRollPalette.color(model.colorIndex(l.lane))
        let from = axis.tick(atX: dirty.minX - PianoRollGeometry.minNoteWidth)
        let to = axis.tick(atX: dirty.maxX)
        for n in l.notes where n.tick <= to && n.tick + n.duration >= from {
            let r = PianoRollGeometry.rect(n, time: axis, pitch: pitchAxis)
            guard r.intersects(dirty) else { continue }
            let path = NSBezierPath(roundedRect: r.insetBy(dx: 0.5, dy: 0.5), xRadius: 2, yRadius: 2)
            color.withAlphaComponent((active ? 0.85 : 0.4) * (n.mute ? 0.4 : 1)).setFill()
            path.fill()
            let selected = editor.isSelected(event: n.id)
            (selected ? NSColor.controlAccentColor : (color.shadow(withLevel: 0.4) ?? color)).setStroke()
            path.lineWidth = selected ? 2 : 1
            path.stroke()
        }
    }

    public override func drawOverlay(in dirty: CGRect) {
        guard let band else { return }
        NSColor.controlAccentColor.withAlphaComponent(0.12).setFill()
        band.fill()
        NSColor.controlAccentColor.setStroke()
        NSBezierPath(rect: band.insetBy(dx: 0.5, dy: 0.5)).stroke()
    }

    // MARK: Gestures

    private func hit(_ p: CGPoint) -> PianoRollHit {
        PianoRollGeometry.hit(p, lanes: laneNotes(), active: model.editLane, time: axis, pitch: pitchAxis)
    }

    public override func contentMouseDown(_ p: CGPoint, clicks: Int, modifiers: Modifiers) {
        let h = hit(p)
        handle(machine.down(h, at: p, clicks: clicks, modifiers: modifiers))
        pressDown(h)
    }

    public override func contentMouseDragged(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.drag(to: p, modifiers: modifiers))
    }

    public override func contentMouseUp(_ p: CGPoint, modifiers: Modifiers) {
        handle(machine.up(at: p, modifiers: modifiers))
        model.silence()
    }

    /// Escape; also aborts a gesture whose final edit still waits on Busy.
    public override func contentCancel() {
        handle(machine.cancel())
        drag = nil
        clearBand()
        model.silence()
        if editor.gesture != nil { editor.cancelGesture() }
    }

    /// A pressed note sounds until release; the pencil sounds its new note
    /// (Plan 4b Ruling 6).
    private func pressDown(_ h: PianoRollHit) {
        switch (editor.tool, h) {
        case (.eraser, _):
            break
        case (_, .note(let n)):
            model.chosenLane = n.lane
            model.sound(n.row.pitch, velocity: n.row.velocity, dest: model.dest(of: n.row, in: n.lane))
        case (.pencil, .empty(_, let pitch)):
            if let lane = model.editLane {
                model.sound(pitch, velocity: Pencil.velocity, dest: model.row(lane)?.midiDest)
            }
        case (_, .empty):
            break
        }
    }

    private func handle(_ effects: [ToolMachine<PianoRollHit>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(hit, _, _, modifiers):
                click(hit, modifiers: modifiers)
            case let .begin(hit, from, modifiers):
                begin(hit, from: from, modifiers: modifiers)
            case let .move(_, _, to, _):
                move(to: to)
            case let .end(_, _, to, _):
                end(at: to)
            case .cancel:
                if drag != nil {
                    drag = nil
                    clearBand()
                    editor.cancelGesture()
                }
            }
        }
    }

    private func click(_ hit: PianoRollHit, modifiers: Modifiers) {
        switch (editor.tool, hit) {
        case (.eraser, .note(let n)):
            editor.edit(.event(edit: .delete(events: [n.row.id])))
        case (_, .note(let n)):
            if modifiers.contains(.shift) {
                model.toggle(n.row.id, in: n.lane)
            } else {
                model.select([(n.lane, n.row.id)], adding: false)
            }
        case (.pencil, .empty(let tick, let pitch)):
            guard let lane = model.editLane else { return }
            let start = Pencil.start(tick, snap: snap, meter: meter)
            editor.edit(
                Pencil.command(
                    lane: lane, tick: start, pitch: pitch, length: Pencil.step(at: start, snap: snap, meter: meter)))
        case (_, .empty):
            if !modifiers.contains(.shift) { editor.clearSelection() }
        }
    }

    private func begin(_ hit: PianoRollHit, from: CGPoint, modifiers: Modifiers) {
        switch (editor.tool, hit) {
        case (.eraser, _):
            let erased = Eraser.adding(hit, to: [])
            guard editor.beginGesture("Erase Notes", lanes: Set(model.shownLanes)) else { return }
            drag = .erase(erased)
            editor.updateGesture(Eraser.command(erased))
        case (_, .note(let n)):
            if !editor.isSelected(event: n.row.id) { model.select([(n.lane, n.row.id)], adding: false) }
            let lanes = laneNotes()
            let ids = NoteDrag.events(pressed: n.row.id, selected: Set(editor.selection.events), lanes: lanes)
            let idSet = Set(ids)
            let pitches = lanes.flatMap(\.notes).filter { idSet.contains($0.id) }.map(\.pitch)
            let d = NoteDrag(
                zone: n.zone, pressed: n.row, lane: n.lane, events: ids, lowest: pitches.min() ?? n.row.pitch,
                highest: pitches.max() ?? n.row.pitch, copy: modifiers.contains(.option))
            guard editor.beginGesture(d.label, lanes: Set(model.shownLanes)) else { return }
            drag = .notes(d, startTick: axis.tick(atX: from.x), startY: from.y)
        case (.pencil, .empty(let tick, let pitch)):
            guard let lane = model.editLane, editor.beginGesture("Insert Note", lanes: [lane]) else { return }
            drag = .pencil(lane: lane, start: Pencil.start(tick, snap: snap, meter: meter), pitch: pitch)
        case (_, .empty):
            drag = .band(from: from, adding: modifiers.contains(.shift))
        }
    }

    private func target(_ d: Drag, at p: CGPoint) -> EditCommand? {
        switch d {
        case let .notes(nd, startTick, startY):
            return nd.command(
                rawTicks: axis.tick(atX: p.x) - startTick, dy: p.y - startY, rowHeight: pitchAxis.rowHeight,
                snap: snap, meter: meter)
        case let .pencil(lane, start, pitch):
            let length = Pencil.length(start: start, to: axis.tick(atX: p.x), snap: snap, meter: meter)
            return Pencil.command(lane: lane, tick: start, pitch: pitch, length: length)
        case .erase(let erased):
            return Eraser.command(Eraser.adding(hit(p), to: erased))
        case .band:
            return nil
        }
    }

    private func move(to p: CGPoint) {
        guard let d = drag else { return }
        switch d {
        case let .notes(nd, _, startY):
            editor.updateGesture(target(d, at: p))
            model.sound(
                nd.soundingPitch(dy: p.y - startY, rowHeight: pitchAxis.rowHeight), velocity: nd.pressed.velocity,
                dest: model.dest(of: nd.pressed, in: nd.lane))
        case .pencil:
            editor.updateGesture(target(d, at: p))
        case .erase(let erased):
            let next = Eraser.adding(hit(p), to: erased)
            guard next != erased else { return }
            drag = .erase(next)
            editor.updateGesture(Eraser.command(next))
        case let .band(from, _):
            band = RubberBand.rect(from: from, to: p)
            overlay.needsDisplay = true
        }
    }

    private func end(at p: CGPoint) {
        guard let d = drag else { return }
        drag = nil
        if case let .band(from, adding) = d {
            clearBand()
            let found = PianoRollGeometry.notes(
                in: RubberBand.rect(from: from, to: p), lanes: laneNotes(), time: axis, pitch: pitchAxis)
            model.select(found, adding: adding)
            return
        }
        editor.endGesture(target(d, at: p))
    }

    private func clearBand() {
        guard band != nil else { return }
        band = nil
        overlay.needsDisplay = true
    }

    // MARK: Keys and cursors

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 51 || e.keyCode == 117 {  // Delete, Forward Delete
            model.deleteSelected()
        } else {
            super.keyDown(with: e)
        }
    }

    public override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let t = tracking { removeTrackingArea(t) }
        let t = NSTrackingArea(
            rect: .zero, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(t)
        tracking = t
    }

    public override func mouseMoved(with e: NSEvent) {
        let p = content.convert(e.locationInWindow, from: nil)
        guard p.y >= 0 else {
            NSCursor.arrow.set()
            return
        }
        Self.cursor(PianoRollCursor.kind(for: hit(p), tool: editor.tool)).set()
    }

    public static func cursor(_ k: CursorKind) -> NSCursor {
        switch k {
        case .arrow: .arrow
        case .openHand: .openHand
        case .upDown: .resizeUpDown
        case .resizeRight: .resizeRight
        case .crosshair: .crosshair
        case .eraser: .disappearingItem
        }
    }

    // MARK: Accessibility (Plan 4b Ruling 21)

    public override func accessibilityItems() -> [CanvasAccessibilityItem] {
        let several = model.shownLanes.count > 1
        var out: [CanvasAccessibilityItem] = []
        for l in laneNotes() {
            let name = several ? model.row(l.lane)?.name : nil
            for n in l.notes {
                let r = PianoRollGeometry.rect(n, time: axis, pitch: pitchAxis)
                guard r.intersects(content.bounds) else { continue }
                let key = l.lane
                let id = n.id
                out.append(
                    CanvasAccessibilityItem(
                        label: PianoRollA11y.label(n, laneName: name, meter: meter), frame: r,
                        selected: editor.isSelected(event: id),
                        press: { [weak self] in self?.model.select([(key, id)], adding: false) }))
            }
        }
        return out
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter PianoRollViewTests 2>&1 | tail -5
```

Expected: PASS (11 tests). If `model.select(found, adding:)` does not type-check because the labels differ, map: `found.map { (lane: $0.lane, id: $0.id) }`.

- [ ] **Step 3: Full check and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
git add app/Sources/VizDAWEditors/PianoRollView.swift app/Tests/VizDAWAppModelTests/PianoRollViewTests.swift
git commit -m "feat(editors): piano roll note view with zones, pencil, eraser and audition

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 288 tests.

---
### Task 10: Keyboard, container, and the piano roll in the detail pane and torn windows

**Files:**
- Create: `app/Sources/VizDAWEditors/PianoKeyboardView.swift`, `app/Sources/VizDAWEditors/PianoRollContainer.swift`, `app/Sources/VizDAW/PianoRollPane.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Modify: `app/Sources/VizDAW/DetailPane.swift`, `app/Sources/VizDAW/EditorWindows.swift`
- Test: `app/Tests/VizDAWAppModelTests/KeyboardTests.swift` (new)

**Interfaces:**
- Consumes: `PianoRollView` (Task 9), `PianoRollModel`, `DetailSlot` (Task 8), `PitchAxis`, `Pencil.velocity`, `PitchName`, `EditorCanvas.rulerHeight`, `StatusLine` (VizDAW, 4a).
- Produces: `DocumentModel.pianoRoll(_ slot: DetailSlot) -> PianoRollModel`, `dropPianoRoll(_:)`; `@MainActor protocol TimeAxisFollower: AnyObject { var timeAxis: TimeAxis { get set } }`; `PianoRollContainer { static sideWidth = 64; model; canvas; keyboard; init(model:); addLowerPane(_ view: NSView & TimeAxisFollower, height: @escaping @MainActor () -> CGFloat) }`; `PianoKeyboardView { init(model:); pitchAxis; press(atY:); dragTo(y:); release() }`; `enum KeyboardLabels { static func label(_:) -> String? }`; SwiftUI `PianoRollEditor(model:)`, `PianoRollBar`, `PianoRollHost`; `DetailPane(model:state:roll:onKind:onTearOff:onClose:)`.

Rulings 6, 12, 22. Spec §4b "the piano roll replaces the detail pane placeholder"; §3.7 torn windows.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/KeyboardTests.swift <<'VIZDAW_EOF'
import AppKit
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

@MainActor
@Suite struct KeyboardTests {
    func board() throws -> (FakeSession, DocumentModel, PianoKeyboardView) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let k = PianoKeyboardView(model: m.pianoRoll(.pane))
        k.frame = NSRect(x: 0, y: 0, width: 64, height: 400)
        k.pitchAxis = PitchAxis(rowHeight: 10, offset: 600)
        return (f, m, k)
    }

    func y(_ k: PianoKeyboardView, _ p: UInt8) -> CGFloat { EditorCanvas.rulerHeight + k.pitchAxis.center(p) }

    func log(_ f: FakeSession) -> [String] {
        f.auditions.map { "\($0.on ? "on" : "off") \($0.pitch)" }
    }

    @Test func onlyCsAreLabelled() {
        #expect(KeyboardLabels.label(60) == "C4")
        #expect(KeyboardLabels.label(0) == "C-1")
        #expect(KeyboardLabels.label(61) == nil)
    }

    @Test func aKeyClickAuditionsThroughTheEditLane() throws {
        let (f, _, k) = try board()
        k.press(atY: y(k, 60))
        k.release()
        #expect(log(f) == ["on 60", "off 60"])
        #expect(f.auditions.allSatisfy { $0.dest == 5 }, "Chords' destination")
    }

    @Test func draggingAcrossKeysPlaysEachInTurn() throws {
        let (f, _, k) = try board()
        k.press(atY: y(k, 60))
        k.dragTo(y: y(k, 60) + 2)
        k.dragTo(y: y(k, 62))
        k.release()
        #expect(log(f) == ["on 60", "off 60", "on 62", "off 62"])
    }

    @Test func theRulerStripIsNotAKey() throws {
        let (f, _, k) = try board()
        k.press(atY: 5)
        k.release()
        #expect(f.auditions.isEmpty)
    }

    @Test func pianoRollModelsAreKeptPerSlot() throws {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        let a = m.pianoRoll(.pane)
        #expect(m.pianoRoll(.pane) === a)
        let id = UUID()
        let w = m.pianoRoll(.window(id))
        #expect(w !== a)
        w.sound(60, velocity: 0x8000, dest: 5)
        m.dropPianoRoll(.window(id))
        #expect(f.auditions.last?.on == false, "a closing window silences its note")
        #expect(m.pianoRoll(.window(id)) !== w)
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter KeyboardTests 2>&1 | tail -5
```

Expected: compile errors (`PianoKeyboardView`, `pianoRoll(_:)` unknown).

- [ ] **Step 2: Piano roll models per slot**

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
    /// The playhead between polls (spec §3.4); fed by `poll()`.
    @ObservationIgnored public let clock = TransportClock()
```
Replace:
```swift
    /// The playhead between polls (spec §3.4); fed by `poll()`.
    @ObservationIgnored public let clock = TransportClock()
    /// One piano roll per slot (Plan 4b Ruling 22).
    @ObservationIgnored private var pianoRolls: [DetailSlot: PianoRollModel] = [:]

    /// The piano roll of the detail pane or a torn-off window, made once.
    public func pianoRoll(_ slot: DetailSlot) -> PianoRollModel {
        if let m = pianoRolls[slot] { return m }
        let m: PianoRollModel
        switch slot {
        case .pane: m = .pane(self)
        case .window(let id): m = .window(id, of: self)
        }
        pianoRolls[slot] = m
        return m
    }

    /// A torn-off window closed: its piano roll goes, silencing its note.
    public func dropPianoRoll(_ slot: DetailSlot) {
        pianoRolls.removeValue(forKey: slot)?.silence()
    }
```

- [ ] **Step 3: Keyboard and container**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/PianoKeyboardView.swift <<'VIZDAW_EOF'
import AppKit
import VizDAWAppModel
import VizDAWCore

public enum KeyboardLabels {
    /// Only C keys are labelled (Plan 4b Ruling 12).
    public static func label(_ p: UInt8) -> String? { p % 12 == 0 ? PitchName.name(p) : nil }
}

/// The optional keyboard beside the notes (spec §4b): one row per pitch,
/// aligned with the note rows; a click auditions through the edit lane's
/// destination, a drag plays each key it crosses (Plan 4b Ruling 6).
@MainActor
public final class PianoKeyboardView: NSView {
    public let model: PianoRollModel
    public var pitchAxis = PitchAxis() { didSet { needsDisplay = true } }
    private(set) var pressed: UInt8?

    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// The key under `y`; the ruler-high strip on top is not a key.
    func pitch(atY y: CGFloat) -> UInt8? {
        y < EditorCanvas.rulerHeight ? nil : pitchAxis.pitch(atY: y - EditorCanvas.rulerHeight)
    }

    func press(atY y: CGFloat) {
        guard let p = pitch(atY: y) else { return }
        pressed = p
        play(p)
    }

    func dragTo(y: CGFloat) {
        guard pressed != nil, let p = pitch(atY: y), p != pressed else { return }
        pressed = p
        play(p)
    }

    func release() {
        pressed = nil
        model.silence()
    }

    private func play(_ p: UInt8) {
        model.sound(p, velocity: Pencil.velocity, dest: model.editLane.flatMap { model.row($0)?.midiDest })
    }

    public override func mouseDown(with e: NSEvent) { press(atY: convert(e.locationInWindow, from: nil).y) }
    public override func mouseDragged(with e: NSEvent) { dragTo(y: convert(e.locationInWindow, from: nil).y) }
    public override func mouseUp(with e: NSEvent) { release() }

    public override func draw(_ dirty: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        let top = EditorCanvas.rulerHeight
        let font = NSFont.systemFont(ofSize: 9)
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: NSRect(x: 0, y: top, width: bounds.width, height: max(bounds.height - top, 0))).addClip()
        for p in pitchAxis.visible(height: max(bounds.height - top, 0)) {
            let y = top + pitchAxis.top(p)
            let row = NSRect(x: 0, y: y, width: bounds.width, height: pitchAxis.rowHeight)
            if PitchName.isBlack(p) {
                NSColor.textBackgroundColor.setFill()
                row.fill()
                NSColor.labelColor.withAlphaComponent(p == pressed ? 0.5 : 0.85).setFill()
                NSRect(x: 0, y: y + 1, width: bounds.width * 0.62, height: pitchAxis.rowHeight - 2).fill()
            } else {
                (p == pressed ? NSColor.controlAccentColor.withAlphaComponent(0.4) : NSColor.textBackgroundColor)
                    .setFill()
                row.fill()
            }
            NSColor.separatorColor.setFill()
            NSRect(x: 0, y: y + pitchAxis.rowHeight - 0.5, width: bounds.width, height: 0.5).fill()
            if let label = KeyboardLabels.label(p), pitchAxis.rowHeight >= 8 {
                NSString(string: label).draw(
                    at: CGPoint(x: bounds.width - 26, y: y + (pitchAxis.rowHeight - 11) / 2),
                    withAttributes: [.font: font, .foregroundColor: NSColor.secondaryLabelColor])
            }
        }
        NSGraphicsContext.restoreGraphicsState()
        NSColor.separatorColor.setFill()
        NSRect(x: bounds.width - 1, y: 0, width: 1, height: bounds.height).fill()
    }

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Keyboard" }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/PianoRollContainer.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel

/// A view under the notes that shares their time axis.
@MainActor
public protocol TimeAxisFollower: AnyObject {
    var timeAxis: TimeAxis { get set }
}

/// The piano roll's views (Plan 4b Ruling 12): a 64-pt side column (the
/// keyboard, or blank) beside the notes, and below them the lower panes
/// (region strip, expression pane), whose own side columns hold their
/// headers, so every pane shares one time axis and x origin.
@MainActor
public final class PianoRollContainer: NSView {
    public static let sideWidth: CGFloat = 64

    public let model: PianoRollModel
    public let canvas: PianoRollView
    public let keyboard: PianoKeyboardView
    private var lower: [(view: NSView & TimeAxisFollower, height: @MainActor () -> CGFloat)] = []

    public init(model: PianoRollModel) {
        self.model = model
        canvas = PianoRollView(model: model)
        keyboard = PianoKeyboardView(model: model)
        super.init(frame: .zero)
        addSubview(keyboard)
        addSubview(canvas)
        keyboard.pitchAxis = canvas.pitchAxis
        canvas.onPitchScroll = { [weak self] a in self?.keyboard.pitchAxis = a }
        canvas.onTimeAxis = { [weak self] a in
            self?.lower.forEach { $0.view.timeAxis = a }
        }
        // Lower panes (plan 4b Tasks 11 and 12) are added here.
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }

    /// Adds a pane under the notes; `height` is asked on every layout (0
    /// hides it).
    public func addLowerPane(_ view: NSView & TimeAxisFollower, height: @escaping @MainActor () -> CGFloat) {
        lower.append((view, height))
        addSubview(view)
        view.timeAxis = canvas.axis
        needsLayout = true
    }

    private func observe() {
        withObservationTracking {
            _ = model.editor.layout.pianoRoll
            _ = model.editor.drawVersion
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsLayout = true
                self?.observe()
            }
        }
    }

    public override func layout() {
        super.layout()
        var bottom = bounds.height
        for pane in lower.reversed() {
            let h = min(max(pane.height(), 0), max(bottom - 80, 0))
            bottom -= h
            pane.view.isHidden = h == 0
            pane.view.frame = NSRect(x: 0, y: bottom, width: bounds.width, height: h)
        }
        let side = Self.sideWidth
        keyboard.isHidden = !model.editor.layout.pianoRoll.showKeyboard
        keyboard.frame = NSRect(x: 0, y: 0, width: side, height: max(bottom, 0))
        canvas.frame = NSRect(x: side, y: 0, width: max(bounds.width - side, 0), height: max(bottom, 0))
    }

    public override func scrollWheel(with e: NSEvent) { canvas.scrollWheel(with: e) }
}
VIZDAW_EOF
```

- [ ] **Step 4: SwiftUI host and bar; detail pane and torn windows**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/PianoRollPane.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The piano roll (spec §4b) with its bar.
struct PianoRollEditor: View {
    let model: PianoRollModel

    var body: some View {
        VStack(spacing: 0) {
            PianoRollBar(model: model, editor: model.editor)
            Divider()
            PianoRollHost(model: model)
        }
    }
}

struct PianoRollHost: NSViewRepresentable {
    let model: PianoRollModel

    func makeNSView(context: Context) -> PianoRollContainer { PianoRollContainer(model: model) }

    func updateNSView(_ view: PianoRollContainer, context: Context) {}
}

/// Tool, keyboard, shown lanes, edit lane, panes, row height and zoom.
struct PianoRollBar: View {
    let model: PianoRollModel
    @Bindable var editor: EditorModel

    private func setting(_ key: WritableKeyPath<PianoRollSettings, Bool>) -> Binding<Bool> {
        Binding(
            get: { editor.layout.pianoRoll[keyPath: key] },
            set: { on in editor.updateLayout { $0.pianoRoll[keyPath: key] = on } })
    }

    private var editLaneName: String { model.editLane.flatMap { model.row($0)?.name } ?? "—" }

    var body: some View {
        HStack(spacing: 10) {
            Picker("Tool", selection: $editor.tool) {
                ForEach(Tool.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 210)
            Toggle("Keyboard", isOn: setting(\.showKeyboard))
            Menu("Lanes") {
                ForEach(model.sequenceLanes, id: \.id) { l in
                    Toggle(l.name, isOn: Binding(get: { model.isShown(l.id) }, set: { _ in model.toggleShown(l.id) }))
                        .disabled(l.id == model.target?.lane)
                }
            }
            .frame(width: 80)
            .help("Lanes shown in this piano roll")
            Menu("Insert Into: \(editLaneName)") {
                ForEach(model.shownLanes, id: \.self) { key in
                    Button(model.row(key)?.name ?? "Lane") { model.chosenLane = key }
                }
            }
            .frame(width: 190)
            .help("The lane the pencil adds notes to")
            Toggle("Audio", isOn: setting(\.showRegions))
            Toggle("Expression", isOn: setting(\.showExpression))
            Spacer()
            Button {
                editor.updateLayout { $0.pianoRoll.rowHeight = max($0.pianoRoll.rowHeight - 2, 4) }
            } label: {
                Image(systemName: "arrow.down.right.and.arrow.up.left")
            }
            .help("Shorter Rows")
            Button {
                editor.updateLayout { $0.pianoRoll.rowHeight = min($0.pianoRoll.rowHeight + 2, 40) }
            } label: {
                Image(systemName: "arrow.up.left.and.arrow.down.right")
            }
            .help("Taller Rows")
            Button {
                editor.updateLayout { $0.pianoRoll.ticksPerPoint = min($0.pianoRoll.ticksPerPoint * 1.5, 61_440) }
            } label: {
                Image(systemName: "minus.magnifyingglass")
            }
            .help("Zoom Out")
            Button {
                editor.updateLayout { $0.pianoRoll.ticksPerPoint = max($0.pianoRoll.ticksPerPoint / 1.5, 2) }
            } label: {
                Image(systemName: "plus.magnifyingglass")
            }
            .help("Zoom In")
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/DetailPane.swift`:

Find:
```swift
/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and — in 4a — a placeholder where the editor will draw (4b–4e).
struct DetailPane: View {
    let model: DocumentModel
    let state: DetailState
```
Replace:
```swift
/// A detail editor's frame: the editor picker, its target, Open in Window,
/// and the editor: the piano roll (4b), else a placeholder (4c–4e).
struct DetailPane: View {
    let model: DocumentModel
    let state: DetailState
    /// This frame's piano roll (the pane's or the torn window's).
    let roll: PianoRollModel
```

Find:
```swift
            Divider()
            ContentUnavailableView(
                "\(state.kind.title) arrives in plan \(state.kind.plan)",
                systemImage: "rectangle.and.pencil.and.ellipsis",
                description: Text(summary))
        }
    }
```
Replace:
```swift
            Divider()
            if state.kind == .pianoRoll {
                PianoRollEditor(model: roll)
            } else {
                ContentUnavailableView(
                    "\(state.kind.title) arrives in plan \(state.kind.plan)",
                    systemImage: "rectangle.and.pencil.and.ellipsis",
                    description: Text(summary))
            }
        }
    }
```

Find:
```swift
            DetailPane(
                model: model, state: d,
                onKind: { k in model.editor.openDetail(d.target, kind: k) },
```
Replace:
```swift
            DetailPane(
                model: model, state: d, roll: model.pianoRoll(.pane),
                onKind: { k in model.editor.openDetail(d.target, kind: k) },
```

In `app/Sources/VizDAW/EditorWindows.swift`:

Find:
```swift
        let c = EditorWindowController(model: model, doc: doc, window: window) { byUser in
            if byUser { model.editor.closeWindow(window.id) }
```
Replace:
```swift
        let c = EditorWindowController(model: model, doc: doc, window: window) { byUser in
            if byUser { model.editor.closeWindow(window.id) }
            model.dropPianoRoll(.window(window.id))
```

Find:
```swift
        if let w = model.editor.layout.windows.first(where: { $0.id == id }) {
            DetailPane(
                model: model, state: w.detail, onKind: { model.editor.setWindowKind(id, $0) },
                onTearOff: nil, onClose: nil)
        } else {
```
Replace:
```swift
        if let w = model.editor.layout.windows.first(where: { $0.id == id }) {
            VStack(spacing: 0) {
                DetailPane(
                    model: model, state: w.detail, roll: model.pianoRoll(.window(id)),
                    onKind: { model.editor.setWindowKind(id, $0) }, onTearOff: nil, onClose: nil)
                StatusLine(editor: model.editor)
            }
        } else {
```

- [ ] **Step 5: Run the tests and build**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
```

Expected: clean build; 293 tests. By eye (optional now, required in Task 14): `scripts/swift.sh run`, File > Create Demo Project, double-click the Chords block: the pane shows the keyboard, the ruler and the chords' notes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWEditors/PianoKeyboardView.swift app/Sources/VizDAWEditors/PianoRollContainer.swift app/Sources/VizDAW/PianoRollPane.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAW/DetailPane.swift app/Sources/VizDAW/EditorWindows.swift app/Tests/VizDAWAppModelTests/KeyboardTests.swift
git commit -m "feat(app): piano roll in the detail pane and torn windows, with keyboard

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 11: Audio-region strip with mute and solo per output path; demo with audio

**Files:**
- Create: `app/Sources/VizDAWEditors/RegionStrip.swift`, `app/Sources/VizDAWEditors/RegionStripView.swift`
- Modify: `app/Sources/VizDAWAppModel/PianoRollModel.swift`, `app/Sources/VizDAWAppModel/DocumentStore.swift`
- Modify: `app/Sources/VizDAWEditors/PianoRollContainer.swift`, `app/Sources/VizDAW/AppCommands.swift`
- Test: `app/Tests/VizDAWEditorsTests/RegionStripTests.swift`, `app/Tests/VizDAWAppModelTests/RegionOutputTests.swift` (new)

**Interfaces:**
- Consumes: `SessionProtocol.{mixer(), setStripMute(strip:mute:), setStripSolo(strip:solo:)}`, `StripKindRow.audioPath(path:)`, `LaneRow.audioOut`, `PianoRollModel.{shownLanes, regions, row, select}`, `NoteDrag.timeDelta`, `TimeAxisFollower`, `Session.newDemoAudio()` (Task 3).
- Produces:
  - Pure: `struct RegionItem: Equatable, Sendable { lane: LaneKey; row: EventRow }`; `struct RegionLine: Equatable, Sendable { output: UInt64?; title: String; items: [RegionItem] }`; `enum RegionHit: Equatable, Sendable { mute(UInt64), solo(UInt64), region(RegionItem), none }`; `enum RegionStripLayout { rowHeight = 22; lines(_:names:) -> [RegionLine]; height(lines:) -> CGFloat; rect(_:line:time:side:) -> CGRect; muteRect(line:), soloRect(line:) -> CGRect; hit(_:lines:time:side:) -> RegionHit; moveCommand(_:rawTicks:snap:meter:) -> EditCommand? }`.
  - `PianoRollModel.outputStrips() -> [UInt64: StripRow]`, `regionLanes() -> [(lane: LaneKey, output: UInt64?, regions: [EventRow])]`, `setOutputMute(_:_:)`, `setOutputSolo(_:_:)`.
  - `RegionStripView: NSView, TimeAxisFollower { init(model:); wantedHeight() -> CGFloat }`.
  - `DocumentStore.makeDemoAudio: Make?`, `newDemoAudio() -> UUID?`.

Rulings 13, 19.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/RegionStripTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let q: Int64 = 15_360
private let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
private let time = TimeAxis(ticksPerPoint: 96)

private func region(_ id: UInt64, _ tick: Int64, _ length: Int64 = 2 * q) -> EventRow {
    var r = Pencil.note(tick: tick, pitch: 0, length: length)
    r.id = id
    r.kind = .audioRegion
    return r
}

@Suite struct RegionStripTests {
    let a = LaneKey(3, 40)
    let b = LaneKey(3, 41)
    let c = LaneKey(3, 42)

    @Test func oneLinePerOutputPathInLaneOrderNoOutputLast() {
        let lines = RegionStripLayout.lines(
            [
                (lane: c, output: nil, regions: [region(9, 0)]),
                (lane: a, output: 7, regions: [region(2, q), region(1, 0)]),
                (lane: LaneKey(3, 43), output: 8, regions: []),
                (lane: b, output: 7, regions: [region(3, 4 * q)]),
            ], names: [7: "Audio 1-2"])
        #expect(lines.map(\.title) == ["Audio 1-2", "No Output"])
        #expect(lines[0].output == 7)
        #expect(lines[0].items.map(\.row.id) == [1, 2, 3], "time order across lanes")
        #expect(lines[1].items.map(\.lane) == [c])
        #expect(RegionStripLayout.lines([(lane: a, output: 5, regions: [region(1, 0)])], names: [:])[0].title == "Output 5")
        #expect(RegionStripLayout.height(lines: 0) == 0)
        #expect(RegionStripLayout.height(lines: 2) == 45)
    }

    @Test func hitsFindButtonsAndRegions() {
        let lines = RegionStripLayout.lines(
            [(lane: a, output: 7, regions: [region(1, q)]), (lane: c, output: nil, regions: [region(9, 0)])],
            names: [:])
        let side: CGFloat = 64
        #expect(RegionStripLayout.hit(CGPoint(x: 10, y: 11), lines: lines, time: time, side: side) == .mute(7))
        #expect(RegionStripLayout.hit(CGPoint(x: 30, y: 11), lines: lines, time: time, side: side) == .solo(7))
        #expect(RegionStripLayout.hit(CGPoint(x: 10, y: 33), lines: lines, time: time, side: side) == .none, "No Output has no buttons")
        #expect(
            RegionStripLayout.hit(CGPoint(x: side + 200, y: 11), lines: lines, time: time, side: side)
                == .region(RegionItem(lane: a, row: region(1, q))))
        #expect(RegionStripLayout.hit(CGPoint(x: side + 50, y: 11), lines: lines, time: time, side: side) == .none)
        #expect(RegionStripLayout.hit(CGPoint(x: 10, y: 99), lines: lines, time: time, side: side) == .none)
    }

    @Test func aRegionDragMovesItSnappingItsStart() {
        var snap = Snap()
        snap.division = .bar
        let item = RegionItem(lane: a, row: region(1, q))
        #expect(RegionStripLayout.moveCommand(item, rawTicks: 3 * q, snap: snap, meter: meter)
            == .event(edit: .move(events: [1], delta: 3 * q, toLane: nil)))
        #expect(RegionStripLayout.moveCommand(item, rawTicks: 100, snap: Snap(), meter: meter) == nil, "back on its quarter")
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWAppModelTests/RegionOutputTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// `rollFake()` plus lane Loop (33) routed to output path 7, whose mixer
/// strip is 40 "Audio 1-2".
func audioFake() -> FakeSession {
    let f = rollFake()
    var loop = lane(33, "Loop")
    loop.audioOut = 7
    loop.regionCount = 1
    f.laneTable[3]?.append(loop)
    f.snap.laneRevs.append(LaneRev(seq: 3, lane: 33, rev: 4))
    var r = noteRow(8, tick: 0)
    r.kind = .audioRegion
    r.duration = 61_440
    f.laneEventTable["3/33"] = LaneEvents(seq: 3, lane: 33, rev: 4, events: [r])
    f.mixerSnap = MixerSnapshot(
        version: 0, strips: [stripRow(40, "Audio 1-2", kind: .audioPath(path: 7)), stripRow(0, "Master", kind: .master)],
        buses: [])
    return f
}

@MainActor
@Suite struct RegionOutputTests {
    @Test func aSequenceTargetShowsRegionLanesAndTheirOutput() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let r = m.pianoRoll(.pane)
        #expect(r.shownLanes.contains(LaneKey(3, 33)))
        let loop = try #require(r.regionLanes().first { $0.lane == LaneKey(3, 33) })
        #expect(loop.output == 7)
        #expect(loop.regions.map(\.id) == [8])
        #expect(r.outputStrips()[7]?.id == 40)
    }

    @Test func muteAndSoloSetTheOutputPathsStrip() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let r = m.pianoRoll(.pane)
        r.setOutputMute(7, true)
        r.setOutputSolo(7, true)
        r.setOutputMute(99, true)
        #expect(f.calls.filter { $0.hasPrefix("mute") || $0.hasPrefix("solo") } == ["mute 40 true", "solo 40 true"])
    }

    @Test func outputButtonsWaitForAnOpenDrag() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        #expect(m.editor.beginGesture("Move Notes", lanes: [LaneKey(3, 31)]))
        m.pianoRoll(.pane).setOutputMute(7, true)
        #expect(!f.calls.contains("mute 40 true"))
        #expect(m.editor.status == EditorModel.gestureOpenMessage)
    }

    @Test func theStoreMakesTheDemoWithAudio() throws {
        let s = DocumentStore(makeEmpty: { editorFake() }, makeDemo: { editorFake() }, open: { _ in editorFake() })
        #expect(s.newDemoAudio() == nil, "no maker")
        s.makeDemoAudio = { audioFake() }
        #expect(s.newDemoAudio() != nil)
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter "RegionStripTests|RegionOutputTests" 2>&1 | tail -5
```

Expected: compile errors.

- [ ] **Step 2: Pure strip layout**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/RegionStrip.swift <<'VIZDAW_EOF'
import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// One audio region and its lane.
public struct RegionItem: Equatable, Sendable {
    public let lane: LaneKey
    public let row: EventRow

    public init(lane: LaneKey, row: EventRow) {
        self.lane = lane
        self.row = row
    }
}

/// One row of the strip: an audio output path (nil: lanes without one).
public struct RegionLine: Equatable, Sendable {
    public let output: UInt64?
    public let title: String
    public let items: [RegionItem]
}

public enum RegionHit: Equatable, Sendable {
    case mute(UInt64)
    case solo(UInt64)
    case region(RegionItem)
    case none
}

/// The region strip between the notes and the expression pane (spec §4b,
/// Plan 4b Ruling 13). Pure: lines, rectangles, hits, the move target.
public enum RegionStripLayout {
    public static let rowHeight: CGFloat = 22

    /// One line per output path of the lanes that hold regions, in lane
    /// order, then "No Output"; each line's regions in time order.
    public static func lines(
        _ lanes: [(lane: LaneKey, output: UInt64?, regions: [EventRow])], names: [UInt64: String]
    ) -> [RegionLine] {
        var order: [UInt64] = []
        var byOutput: [UInt64: [RegionItem]] = [:]
        var loose: [RegionItem] = []
        for l in lanes where !l.regions.isEmpty {
            let items = l.regions.map { RegionItem(lane: l.lane, row: $0) }
            guard let o = l.output else {
                loose += items
                continue
            }
            if byOutput[o] == nil { order.append(o) }
            byOutput[o, default: []] += items
        }
        let sorted = { (items: [RegionItem]) in items.sorted { $0.row.tick < $1.row.tick } }
        var out = order.map { o in
            RegionLine(output: o, title: names[o] ?? "Output \(o)", items: sorted(byOutput[o] ?? []))
        }
        if !loose.isEmpty { out.append(RegionLine(output: nil, title: "No Output", items: sorted(loose))) }
        return out
    }

    public static func height(lines: Int) -> CGFloat { lines == 0 ? 0 : CGFloat(lines) * rowHeight + 1 }

    static func top(_ line: Int) -> CGFloat { CGFloat(line) * rowHeight }

    public static func rect(_ r: EventRow, line: Int, time: TimeAxis, side: CGFloat) -> CGRect {
        CGRect(
            x: side + time.x(r.tick), y: top(line) + 2, width: max(time.width(ofTicks: r.duration), 3),
            height: rowHeight - 4)
    }

    public static func muteRect(line: Int) -> CGRect { CGRect(x: 4, y: top(line) + 3, width: 18, height: 16) }
    public static func soloRect(line: Int) -> CGRect { CGRect(x: 24, y: top(line) + 3, width: 18, height: 16) }

    public static func hit(_ p: CGPoint, lines: [RegionLine], time: TimeAxis, side: CGFloat) -> RegionHit {
        guard p.y >= 0, p.y.isFinite else { return .none }
        let i = Int((p.y / rowHeight).rounded(.down))
        guard i < lines.count else { return .none }
        let line = lines[i]
        if p.x < side {
            guard let o = line.output else { return .none }
            if muteRect(line: i).contains(p) { return .mute(o) }
            if soloRect(line: i).contains(p) { return .solo(o) }
            return .none
        }
        if let item = line.items.last(where: { rect($0.row, line: i, time: time, side: side).contains(p) }) {
            return .region(item)
        }
        return .none
    }

    /// A region drag's full target: its start snapped (nil: no change).
    public static func moveCommand(
        _ item: RegionItem, rawTicks: Int64, snap: Snap, meter: [MeterEntryRow]
    ) -> EditCommand? {
        let d = NoteDrag.timeDelta(rawTicks: rawTicks, start: item.row.tick, snap: snap, meter: meter)
        return d == 0 ? nil : .event(edit: .move(events: [item.row.id], delta: d, toLane: nil))
    }
}
VIZDAW_EOF
```

- [ ] **Step 3: Output strips in the model; the demo with audio in the store**

In `app/Sources/VizDAWAppModel/PianoRollModel.swift`:

Find:
```swift
    /// The note sounding while a note or key is pressed.
    public private(set) var sounding: AuditionNote?
```
Replace:
```swift
    /// The note sounding while a note or key is pressed.
    public private(set) var sounding: AuditionNote?
    @ObservationIgnored private var stripCache: (version: UInt64, strips: [UInt64: StripRow])?
```

Find:
```swift
    // MARK: Sound (Plan 4b Ruling 6)
```
Replace:
```swift
    // MARK: Audio regions (Plan 4b Ruling 13)

    /// The shown lanes with their output path and regions.
    public func regionLanes() -> [(lane: LaneKey, output: UInt64?, regions: [EventRow])] {
        shownLanes.map { (lane: $0, output: row($0)?.audioOut, regions: regions($0)) }
    }

    /// Mixer strips of audio output paths, by path, cached per snapshot
    /// version; a failed read keeps the last ones.
    public func outputStrips() -> [UInt64: StripRow] {
        if let c = stripCache, c.version == editor.snapshotVersion { return c.strips }
        guard let m = try? document.session.mixer() else { return stripCache?.strips ?? [:] }
        var out: [UInt64: StripRow] = [:]
        for s in m.strips {
            if case let .audioPath(path) = s.kind { out[path] = s }
        }
        stripCache = (editor.snapshotVersion, out)
        return out
    }

    public func setOutputMute(_ output: UInt64, _ on: Bool) {
        mixerEdit(output) { s, strip in try s.setStripMute(strip: strip, mute: on) }
    }

    public func setOutputSolo(_ output: UInt64, _ on: Bool) {
        mixerEdit(output) { s, strip in try s.setStripSolo(strip: strip, solo: on) }
    }

    private func mixerEdit(_ output: UInt64, _ body: (any SessionProtocol, UInt64) throws -> DocSnapshot) {
        guard editor.gesture == nil else {
            editor.status = EditorModel.gestureOpenMessage
            return
        }
        guard let strip = outputStrips()[output] else { return }
        do {
            editor.applySnapshot(try body(document.session, strip.id))
        } catch {
            editor.report(error)
        }
    }

    // MARK: Sound (Plan 4b Ruling 6)
```

In `app/Sources/VizDAWAppModel/DocumentStore.swift`:

Find:
```swift
    public func newDemo() -> UUID? { add("Demo Project Failed", makeDemo) }
```
Replace:
```swift
    public func newDemo() -> UUID? { add("Demo Project Failed", makeDemo) }

    /// Makes the demo with an audio lane (Plan 4b Ruling 19); set by `live()`.
    public var makeDemoAudio: Make?

    public func newDemoAudio() -> UUID? {
        guard let make = makeDemoAudio else { return nil }
        return add("Demo Project Failed", make)
    }
```

Find:
```swift
    public static func live() -> DocumentStore {
        DocumentStore(
            makeEmpty: { try Session.newEmpty() },
            makeDemo: { try Session.newDemo() },
            open: { try Session.open(path: $0) }
        )
    }
```
Replace:
```swift
    public static func live() -> DocumentStore {
        let s = DocumentStore(
            makeEmpty: { try Session.newEmpty() },
            makeDemo: { try Session.newDemo() },
            open: { try Session.open(path: $0) }
        )
        s.makeDemoAudio = { try Session.newDemoAudio() }
        return s
    }
```

In `app/Sources/VizDAW/AppCommands.swift`:

Find:
```swift
            Button("Create Demo Project") { show(store.newDemo()) }
```
Replace:
```swift
            Button("Create Demo Project") { show(store.newDemo()) }
            Button("Create Demo Project with Audio") { show(store.newDemoAudio()) }
```

- [ ] **Step 4: The strip view, added under the notes**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/RegionStripView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The audio regions of the shown lanes, one line per output path, with
/// mute and solo per path (spec §4b, Plan 4b Ruling 13).
@MainActor
public final class RegionStripView: NSView, TimeAxisFollower {
    public let model: PianoRollModel
    var editor: EditorModel { model.editor }
    public var timeAxis = TimeAxis() { didSet { needsDisplay = true } }
    private var machine = ToolMachine<RegionHit>()
    private var moving: (item: RegionItem, startTick: Int64)?
    private var a11yElements: [CanvasElement] = []

    var side: CGFloat { PianoRollContainer.sideWidth }
    var meter: [MeterEntryRow] { model.seq.flatMap { editor.info($0)?.meter } ?? [] }

    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.selection
            _ = editor.layout.pianoRoll
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.observe()
            }
        }
    }

    func lines() -> [RegionLine] {
        RegionStripLayout.lines(model.regionLanes(), names: model.outputStrips().mapValues(\.name))
    }

    /// The height the container gives it (0 hides it).
    public func wantedHeight() -> CGFloat {
        editor.layout.pianoRoll.showRegions ? RegionStripLayout.height(lines: lines().count) : 0
    }

    // MARK: Drawing

    public override func draw(_ dirty: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirty.fill()
        NSColor.separatorColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill()
        let strips = model.outputStrips()
        let small = NSFont.systemFont(ofSize: 10)
        for (i, line) in lines().enumerated() {
            if let o = line.output {
                let s = strips[o]
                button("M", RegionStripLayout.muteRect(line: i), on: s?.mute ?? false, color: .systemOrange)
                button("S", RegionStripLayout.soloRect(line: i), on: s?.solo ?? false, color: .systemYellow)
            }
            NSGraphicsContext.saveGraphicsState()
            NSBezierPath(rect: NSRect(x: side, y: 0, width: max(bounds.width - side, 0), height: bounds.height))
                .addClip()
            for item in line.items {
                let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
                guard r.intersects(dirty) else { continue }
                let path = NSBezierPath(roundedRect: r, xRadius: 3, yRadius: 3)
                NSColor.systemTeal.withAlphaComponent(item.row.mute ? 0.15 : 0.35).setFill()
                path.fill()
                (editor.isSelected(event: item.row.id) ? NSColor.controlAccentColor : NSColor.systemTeal).setStroke()
                path.lineWidth = editor.isSelected(event: item.row.id) ? 2 : 1
                path.stroke()
                NSString(string: model.row(item.lane)?.name ?? "").draw(
                    at: CGPoint(x: r.minX + 4, y: r.minY + 2),
                    withAttributes: [.font: small, .foregroundColor: NSColor.labelColor])
            }
            NSString(string: line.title).draw(
                at: CGPoint(x: side + 4, y: RegionStripLayout.rowHeight * CGFloat(i) + 4),
                withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
            NSGraphicsContext.restoreGraphicsState()
        }
    }

    private func button(_ title: String, _ r: CGRect, on: Bool, color: NSColor) {
        (on ? color : NSColor.controlBackgroundColor).setFill()
        NSBezierPath(roundedRect: r, xRadius: 3, yRadius: 3).fill()
        NSColor.separatorColor.setStroke()
        NSBezierPath(roundedRect: r.insetBy(dx: 0.5, dy: 0.5), xRadius: 3, yRadius: 3).stroke()
        NSString(string: title).draw(
            at: CGPoint(x: r.minX + 5, y: r.minY + 1),
            withAttributes: [.font: NSFont.boldSystemFont(ofSize: 11), .foregroundColor: NSColor.labelColor])
    }

    // MARK: Events

    private func point(_ e: NSEvent) -> CGPoint { convert(e.locationInWindow, from: nil) }

    public override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        let p = point(e)
        handle(
            machine.down(
                RegionStripLayout.hit(p, lines: lines(), time: timeAxis, side: side), at: p, clicks: e.clickCount,
                modifiers: Modifiers(event: e)))
    }

    public override func mouseDragged(with e: NSEvent) {
        handle(machine.drag(to: point(e), modifiers: Modifiers(event: e)))
    }

    public override func mouseUp(with e: NSEvent) {
        handle(machine.up(at: point(e), modifiers: Modifiers(event: e)))
    }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53 {  // Escape
            handle(machine.cancel())
            moving = nil
            if editor.gesture != nil { editor.cancelGesture() }
        } else {
            super.keyDown(with: e)
        }
    }

    private func handle(_ effects: [ToolMachine<RegionHit>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(hit, _, _, modifiers):
                click(hit, adding: modifiers.contains(.shift))
            case let .begin(.region(item), from, _):
                guard editor.beginGesture("Move Region", lanes: [item.lane]) else { continue }
                moving = (item, timeAxis.tick(atX: from.x - side))
            case .begin:
                break
            case let .move(_, _, to, _):
                if let m = moving { editor.updateGesture(target(m, to)) }
            case let .end(_, _, to, _):
                if let m = moving {
                    moving = nil
                    editor.endGesture(target(m, to))
                }
            case .cancel:
                if moving != nil {
                    moving = nil
                    editor.cancelGesture()
                }
            }
        }
    }

    private func target(_ m: (item: RegionItem, startTick: Int64), _ p: CGPoint) -> EditCommand? {
        RegionStripLayout.moveCommand(
            m.item, rawTicks: timeAxis.tick(atX: p.x - side) - m.startTick, snap: editor.layout.snap, meter: meter)
    }

    private func click(_ hit: RegionHit, adding: Bool) {
        let strips = model.outputStrips()
        switch hit {
        case .mute(let o): model.setOutputMute(o, !(strips[o]?.mute ?? false))
        case .solo(let o): model.setOutputSolo(o, !(strips[o]?.solo ?? false))
        case .region(let item): model.select([(item.lane, item.row.id)], adding: adding)
        case .none: break
        }
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Audio Regions" }

    public override func accessibilityChildren() -> [Any]? {
        let strips = model.outputStrips()
        var items: [(item: CanvasAccessibilityItem, frame: CGRect)] = []
        for (i, line) in lines().enumerated() {
            if let o = line.output {
                let muted = strips[o]?.mute ?? false
                let soloed = strips[o]?.solo ?? false
                items.append(
                    (CanvasAccessibilityItem(
                        label: "Mute \(line.title)", frame: RegionStripLayout.muteRect(line: i), selected: muted,
                        press: { [weak self] in self?.model.setOutputMute(o, !muted) }),
                    RegionStripLayout.muteRect(line: i)))
                items.append(
                    (CanvasAccessibilityItem(
                        label: "Solo \(line.title)", frame: RegionStripLayout.soloRect(line: i), selected: soloed,
                        press: { [weak self] in self?.model.setOutputSolo(o, !soloed) }),
                    RegionStripLayout.soloRect(line: i)))
            }
            for item in line.items {
                let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
                guard r.intersects(bounds) else { continue }
                let label = "Region on \(model.row(item.lane)?.name ?? "a lane"), \(line.title)"
                items.append(
                    (CanvasAccessibilityItem(
                        label: label, frame: r, selected: editor.isSelected(event: item.row.id),
                        press: { [weak self] in self?.model.select([(item.lane, item.row.id)], adding: false) }), r))
            }
        }
        a11yElements = CanvasElement.reuse(a11yElements, for: items, parent: self)
        return a11yElements
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWEditors/PianoRollContainer.swift`:

Find:
```swift
        // Lower panes (plan 4b Tasks 11 and 12) are added here.
```
Replace:
```swift
        // Lower panes (plan 4b Tasks 11 and 12) are added here.
        let strip = RegionStripView(model: model)
        addLowerPane(strip) { [weak strip] in strip?.wantedHeight() ?? 0 }
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
```

Expected: clean; 300 tests (293 + 3 + 4). Hand check for `hitsFindButtonsAndRegions`: region 1 starts at a quarter (x 160 in the time area, 224 in the strip) and is two quarters long, so x 264 (side + 200) is inside and x 114 (side + 50) is not.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWEditors/RegionStrip.swift app/Sources/VizDAWEditors/RegionStripView.swift app/Sources/VizDAWAppModel/PianoRollModel.swift app/Sources/VizDAWAppModel/DocumentStore.swift app/Sources/VizDAWEditors/PianoRollContainer.swift app/Sources/VizDAW/AppCommands.swift app/Tests/VizDAWEditorsTests/RegionStripTests.swift app/Tests/VizDAWAppModelTests/RegionOutputTests.swift
git commit -m "feat(editors): audio region strip with mute and solo per output path

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 12: Per-note expression pane

**Files:**
- Create: `app/Sources/VizDAWEditors/Expression.swift`, `app/Sources/VizDAWEditors/ExpressionPaneView.swift`
- Modify: `app/Sources/VizDAWEditors/PianoRollContainer.swift`
- Test: `app/Tests/VizDAWEditorsTests/ExpressionTests.swift`, `app/Tests/VizDAWAppModelTests/ExpressionPaneTests.swift` (new)

**Interfaces:**
- Consumes: `ValueAxis` (Task 6), `ExpressionKind`, `PianoRollSettings.{expression, showExpression}` (Task 8), `EventEdit.setPerNote` and `PerNoteValueRow` (Task 3), `TimeAxisFollower`, `PianoRollModel.{editLane, notes, colorIndex}`, `MenuAction` (4a, internal to VizDAWEditors), `PianoRollPalette`.
- Produces: `enum ExpressionGeometry { paneHeight = 96; pointSpacing = 4; steps(_:kind:neutral:) -> [(from: Int64, to: Int64, value: UInt32)]; targets(_:from:to:selected:) -> [UInt64]; points(_:time:values:) -> [PerNoteValueRow]; command(stroke:erase:kind:notes:selected:time:values:) -> EditCommand?; shortTitle(_:) -> String }`; `ExpressionPaneView: NSView, TimeAxisFollower { init(model:); wantedHeight(); strokeBegan(at:); strokeMoved(to:); strokeEnded(); strokeCancelled() }`.

Ruling 14. Stroke points arrive in the view's coordinates; the time area starts after the 64-pt side column.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/ExpressionTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let time = TimeAxis(ticksPerPoint: 96)
private let values = ValueAxis(height: 96, inset: 3)

private func note(_ id: UInt64, _ tick: Int64, _ length: Int64, _ points: [PerNotePoint] = []) -> EventRow {
    var r = Pencil.note(tick: tick, pitch: 60, length: length)
    r.id = id
    r.perNote = points
    return r
}

@Suite struct ExpressionTests {
    @Test func stepsHoldEachValueUntilTheNext() {
        let pb = PerNoteKindRow.pitchBend
        let n = note(1, 1_000, 100, [
            PerNotePoint(kind: pb, offset: 0, value: 5),
            PerNotePoint(kind: .registered(index: 1), offset: 10, value: 99),
            PerNotePoint(kind: pb, offset: 50, value: 7),
        ])
        let s = ExpressionGeometry.steps(n, kind: pb, neutral: 0x8000_0000)
        #expect(s.map { $0.from } == [1_000, 1_050])
        #expect(s.map { $0.to } == [1_050, 1_100])
        #expect(s.map { $0.value } == [5, 7])
        let empty = ExpressionGeometry.steps(note(2, 0, 100), kind: pb, neutral: 0x8000_0000)
        #expect(empty.map { $0.value } == [0x8000_0000])
        let late = ExpressionGeometry.steps(note(3, 0, 100, [PerNotePoint(kind: pb, offset: 40, value: 1)]), kind: pb, neutral: 9)
        #expect(late.map { $0.value } == [9, 1], "neutral before the first point")
    }

    @Test func targetsPreferTheSelectedNotes() {
        let notes = [note(1, 0, 1_000), note(2, 0, 1_000), note(3, 5_000, 1_000)]
        #expect(ExpressionGeometry.targets(notes, from: 100, to: 200, selected: []) == [1, 2])
        #expect(ExpressionGeometry.targets(notes, from: 100, to: 200, selected: [2, 3]) == [2])
        #expect(ExpressionGeometry.targets(notes, from: 100, to: 6_000, selected: [3]) == [3])
        #expect(ExpressionGeometry.targets(notes, from: 2_000, to: 3_000, selected: []) == [])
    }

    @Test func strokesAreThinnedToOnePointPerSpacing() {
        // One point per 4 pt = 384 ticks; later stroke points win on one tick.
        let stroke = [
            CGPoint(x: 0, y: 93), CGPoint(x: 1, y: 3), CGPoint(x: 4, y: 48), CGPoint(x: 4, y: 3), CGPoint(x: 10, y: 3),
        ]
        let pts = ExpressionGeometry.points(stroke, time: time, values: values)
        #expect(pts.map(\.tick) == [0, 384, 960])
        #expect(pts[0].value == 0)
        #expect(pts[1].value == UInt32.max)
    }

    @Test func aStrokeIsOneSetPerNoteOverItsRange() {
        let notes = [note(1, 0, 3_000)]
        let stroke = [CGPoint(x: 0, y: 3), CGPoint(x: 10, y: 93)]
        let cmd = ExpressionGeometry.command(
            stroke: stroke, erase: false, kind: .pitchBend, notes: notes, selected: [], time: time, values: values)
        #expect(cmd == .event(edit: .setPerNote(
            events: [1], kind: .pitchBend, from: 0, to: 960,
            points: [PerNoteValueRow(tick: 0, value: UInt32.max), PerNoteValueRow(tick: 960, value: 0)])))
        let erase = ExpressionGeometry.command(
            stroke: stroke, erase: true, kind: .pitchBend, notes: notes, selected: [], time: time, values: values)
        #expect(erase == .event(edit: .setPerNote(events: [1], kind: .pitchBend, from: 0, to: 960, points: [])))
        #expect(ExpressionGeometry.command(
            stroke: [CGPoint(x: 500, y: 3)], erase: false, kind: .pitchBend, notes: notes, selected: [], time: time,
            values: values) == nil, "no note under the stroke")
        #expect(ExpressionGeometry.command(
            stroke: [], erase: false, kind: .pitchBend, notes: notes, selected: [], time: time, values: values) == nil)
    }

    @Test func shortTitles() {
        #expect(ExpressionGeometry.shortTitle(.pitchBend) == "Bend")
        #expect(ExpressionGeometry.shortTitle(.registered(74)) == "Reg 74")
        #expect(ExpressionGeometry.shortTitle(.assignable(2)) == "Asg 2")
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWAppModelTests/ExpressionPaneTests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

@MainActor
@Suite struct ExpressionPaneTests {
    func pane() throws -> (FakeSession, DocumentModel, ExpressionPaneView) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.lane(seq: 3, lane: 31), kind: .pianoRoll)
        let v = ExpressionPaneView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 600, height: 96)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        return (f, m, v)
    }

    @Test func aStrokeWritesPerNoteValuesAsOneStep() throws {
        let (f, m, v) = try pane()
        // Over note 1 (C4, 0 to 15 360 = x 0 to 160 in the time area).
        v.strokeBegan(at: CGPoint(x: 64 + 10, y: 3))
        v.strokeMoved(to: CGPoint(x: 64 + 20, y: 48))
        #expect(m.editor.gesture?.label == "Draw Expression")
        v.strokeEnded()
        #expect(m.editor.gesture == nil)
        guard case let .event(edit: .setPerNote(events, kind, from, to, points))? = f.gestureEdits.last else {
            Issue.record("no per-note edit: \(f.gestureEdits)")
            return
        }
        #expect(events == [1] && kind == .pitchBend && from == 960 && to == 1_920)
        #expect(points.map(\.tick) == [960, 1_920])
        #expect(points.first?.value == UInt32.max)
    }

    @Test func theEraserErasesAndAClickWritesOnePoint() throws {
        let (f, m, v) = try pane()
        m.editor.tool = .eraser
        v.strokeBegan(at: CGPoint(x: 64 + 10, y: 3))
        v.strokeMoved(to: CGPoint(x: 64 + 20, y: 3))
        v.strokeEnded()
        #expect(f.gestureEdits.last == .event(edit: .setPerNote(events: [1], kind: .pitchBend, from: 960, to: 1_920, points: [])))
        m.editor.tool = .pencil
        v.strokeBegan(at: CGPoint(x: 64 + 10, y: 93))
        v.strokeEnded()
        #expect(f.edits.last == .event(edit: .setPerNote(
            events: [1], kind: .pitchBend, from: 960, to: 960, points: [PerNoteValueRow(tick: 960, value: 0)])))
    }

    @Test func theSideColumnIsNotATimeArea() throws {
        let (f, m, v) = try pane()
        v.strokeBegan(at: CGPoint(x: 10, y: 3))
        v.strokeMoved(to: CGPoint(x: 30, y: 3))
        v.strokeEnded()
        #expect(f.gestureEdits.isEmpty && f.edits.isEmpty)
        #expect(m.editor.gesture == nil)
    }

    @Test func escapeCancelsTheStroke() throws {
        let (f, m, v) = try pane()
        v.strokeBegan(at: CGPoint(x: 64 + 10, y: 3))
        v.strokeMoved(to: CGPoint(x: 64 + 20, y: 48))
        v.strokeCancelled()
        #expect(m.editor.gesture == nil)
        #expect(f.calls.last == "abort")
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter "ExpressionTests|ExpressionPaneTests" 2>&1 | tail -5
```

Expected: compile errors.

- [ ] **Step 2: Pure expression geometry**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/Expression.swift <<'VIZDAW_EOF'
import CoreGraphics
import VizDAWAppModel
import VizDAWCore

/// Per-note expression curves and strokes (spec §4b, Plan 4b Ruling 14).
public enum ExpressionGeometry {
    public static let paneHeight: CGFloat = 96
    /// One stroke point is kept per this many points of x.
    public static let pointSpacing: CGFloat = 4

    /// A note's values of `kind` as steps over its span: `neutral` until the
    /// first point, then each value until the next point or the note's end.
    public static func steps(
        _ n: EventRow, kind: PerNoteKindRow, neutral: UInt32
    ) -> [(from: Int64, to: Int64, value: UInt32)] {
        let end = n.tick + max(n.duration, 0)
        var out: [(from: Int64, to: Int64, value: UInt32)] = []
        var at = n.tick
        var value = neutral
        for p in n.perNote where p.kind == kind {
            let t = min(max(n.tick + p.offset, n.tick), end)
            if t > at { out.append((at, t, value)) }
            at = max(at, t)
            value = p.value
        }
        out.append((at, end, value))
        return out
    }

    /// The notes a stroke over `from...to` edits: the selected ones among
    /// those it overlaps, else all it overlaps.
    public static func targets(_ notes: [EventRow], from: Int64, to: Int64, selected: Set<UInt64>) -> [UInt64] {
        let under = notes.filter { $0.tick <= to && $0.tick + $0.duration >= from }
        let chosen = under.filter { selected.contains($0.id) }
        return (chosen.isEmpty ? under : chosen).map(\.id)
    }

    /// Stroke points (time-area coordinates) as values, ascending, one per
    /// `pointSpacing` points; on one tick the later stroke point wins.
    public static func points(_ stroke: [CGPoint], time: TimeAxis, values: ValueAxis) -> [PerNoteValueRow] {
        let spacing = max(TimeAxis.clampTick(Double(pointSpacing) * time.ticksPerPoint), 1)
        var byTick: [Int64: UInt32] = [:]
        for p in stroke { byTick[time.tick(atX: p.x)] = values.value(atY: p.y) }
        var out: [PerNoteValueRow] = []
        for (tick, value) in byTick.sorted(by: { $0.key < $1.key }) {
            if let last = out.last, tick - last.tick < spacing { continue }
            out.append(PerNoteValueRow(tick: tick, value: value))
        }
        return out
    }

    /// A stroke's full target: one `SetPerNote` over its time range.
    public static func command(
        stroke: [CGPoint], erase: Bool, kind: PerNoteKindRow, notes: [EventRow], selected: Set<UInt64>,
        time: TimeAxis, values: ValueAxis
    ) -> EditCommand? {
        guard let lo = stroke.map(\.x).min(), let hi = stroke.map(\.x).max() else { return nil }
        let from = time.tick(atX: lo)
        let to = time.tick(atX: hi)
        let ids = targets(notes, from: from, to: to, selected: selected)
        guard !ids.isEmpty else { return nil }
        let pts = erase ? [] : points(stroke, time: time, values: values)
        return .event(edit: .setPerNote(events: ids, kind: kind, from: from, to: to, points: pts))
    }

    /// The pane header's label.
    public static func shortTitle(_ k: ExpressionKind) -> String {
        switch k {
        case .pitchBend: "Bend"
        case .registered(let i): "Reg \(i)"
        case .assignable(let i): "Asg \(i)"
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 3: The pane view**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/ExpressionPaneView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The per-note expression pane under the region strip (spec §4b, Plan 4b
/// Ruling 14): the edit lane's notes' values of one kind; any tool but the
/// eraser draws them, the eraser removes them; a stroke is one undo step.
@MainActor
public final class ExpressionPaneView: NSView, TimeAxisFollower {
    public let model: PianoRollModel
    var editor: EditorModel { model.editor }
    public var timeAxis = TimeAxis() { didSet { needsDisplay = true } }

    /// The stroke so far, in time-area coordinates.
    private var stroke: [CGPoint] = []
    private var erasing = false
    private var start: CGPoint?
    private var open = false

    var side: CGFloat { PianoRollContainer.sideWidth }
    var kind: ExpressionKind { editor.layout.pianoRoll.expression }
    var values: ValueAxis { ValueAxis(height: bounds.height) }

    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
        observe()
    }

    public required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    private func observe() {
        withObservationTracking {
            _ = editor.drawVersion
            _ = editor.selection
            _ = editor.layout.pianoRoll
            _ = model.chosenLane
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.needsDisplay = true
                self?.observe()
            }
        }
    }

    public func wantedHeight() -> CGFloat {
        editor.layout.pianoRoll.showExpression ? ExpressionGeometry.paneHeight : 0
    }

    func editNotes() -> [EventRow] { model.editLane.map { model.notes($0) } ?? [] }

    // MARK: Drawing

    public override func draw(_ dirty: NSRect) {
        NSColor.textBackgroundColor.setFill()
        dirty.fill()
        NSColor.windowBackgroundColor.setFill()
        NSRect(x: 0, y: 0, width: side, height: bounds.height).fill()
        NSColor.separatorColor.setFill()
        NSRect(x: 0, y: 0, width: bounds.width, height: 1).fill()
        NSString(string: ExpressionGeometry.shortTitle(kind) + " ▾").draw(
            at: CGPoint(x: 6, y: 6),
            withAttributes: [.font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: NSColor.labelColor])
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: NSRect(x: side, y: 0, width: max(bounds.width - side, 0), height: bounds.height)).addClip()
        let v = values
        if kind == .pitchBend {
            NSColor.separatorColor.setFill()
            NSRect(x: side, y: v.y(0x8000_0000), width: bounds.width, height: 0.5).fill()
        }
        guard let lane = model.editLane else {
            NSGraphicsContext.restoreGraphicsState()
            return
        }
        let color = PianoRollPalette.color(model.colorIndex(lane))
        let from = timeAxis.tick(atX: dirty.minX - side)
        let to = timeAxis.tick(atX: dirty.maxX - side)
        for n in editNotes() where n.tick <= to && n.tick + n.duration >= from {
            let selected = editor.isSelected(event: n.id)
            let x0 = side + timeAxis.x(n.tick)
            let x1 = side + timeAxis.x(n.tick + n.duration)
            color.withAlphaComponent(0.08).setFill()
            NSRect(x: x0, y: 0, width: max(x1 - x0, 1), height: bounds.height).fill()
            let path = NSBezierPath()
            for (i, s) in ExpressionGeometry.steps(n, kind: kind.row, neutral: kind.neutral).enumerated() {
                let a = CGPoint(x: side + timeAxis.x(s.from), y: v.y(s.value))
                let b = CGPoint(x: side + timeAxis.x(s.to), y: v.y(s.value))
                if i == 0 { path.move(to: a) } else { path.line(to: a) }
                path.line(to: b)
            }
            (selected ? NSColor.controlAccentColor : color).setStroke()
            path.lineWidth = selected ? 2 : 1.5
            path.stroke()
        }
        NSGraphicsContext.restoreGraphicsState()
    }

    // MARK: Strokes (view coordinates)

    func strokeBegan(at p: CGPoint) {
        stroke = []
        open = false
        start = nil
        guard p.x >= side else { return }
        erasing = editor.tool == .eraser
        start = p
        stroke = [CGPoint(x: p.x - side, y: p.y)]
    }

    func strokeMoved(to p: CGPoint) {
        guard let s = start else { return }
        stroke.append(CGPoint(x: max(p.x, side) - side, y: p.y))
        if !open {
            guard hypot(p.x - s.x, p.y - s.y) > ToolMachine<Int>.deadZone, let lane = model.editLane else { return }
            open = editor.beginGesture(erasing ? "Erase Expression" : "Draw Expression", lanes: [lane])
        }
        if open { editor.updateGesture(command()) }
    }

    func strokeEnded() {
        defer {
            stroke = []
            start = nil
            open = false
        }
        guard start != nil else { return }
        if open {
            editor.endGesture(command())
        } else if let c = command() {
            editor.edit(c)
        }
    }

    func strokeCancelled() {
        if open { editor.cancelGesture() }
        stroke = []
        start = nil
        open = false
    }

    private func command() -> EditCommand? {
        ExpressionGeometry.command(
            stroke: stroke, erase: erasing, kind: kind.row, notes: editNotes(),
            selected: Set(editor.selection.events), time: timeAxis, values: values)
    }

    // MARK: Events

    public override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        let p = convert(e.locationInWindow, from: nil)
        if p.x < side {
            kindMenu().popUp(positioning: nil, at: p, in: self)
            return
        }
        strokeBegan(at: p)
    }

    public override func mouseDragged(with e: NSEvent) { strokeMoved(to: convert(e.locationInWindow, from: nil)) }
    public override func mouseUp(with e: NSEvent) { strokeEnded() }

    public override func keyDown(with e: NSEvent) {
        if e.keyCode == 53 { strokeCancelled() } else { super.keyDown(with: e) }
    }

    func kindMenu() -> NSMenu {
        let menu = NSMenu()
        for k in ExpressionKind.choices {
            menu.addItem(
                MenuAction(title: k.title) { [weak self] in
                    self?.editor.updateLayout { $0.pianoRoll.expression = k }
                }.item(checked: k == kind))
        }
        return menu
    }

    // MARK: Accessibility

    public override func isAccessibilityElement() -> Bool { true }
    public override func accessibilityRole() -> NSAccessibility.Role? { .group }
    public override func accessibilityLabel() -> String? { "Expression: \(kind.title)" }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWEditors/PianoRollContainer.swift`:

Find:
```swift
        let strip = RegionStripView(model: model)
        addLowerPane(strip) { [weak strip] in strip?.wantedHeight() ?? 0 }
```
Replace:
```swift
        let strip = RegionStripView(model: model)
        addLowerPane(strip) { [weak strip] in strip?.wantedHeight() ?? 0 }
        let expression = ExpressionPaneView(model: model)
        addLowerPane(expression) { [weak expression] in expression?.wantedHeight() ?? 0 }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
```

Expected: clean; 309 tests (300 + 5 + 4). Hand check for `aStrokeWritesPerNoteValuesAsOneStep`: x 10 and 20 of the time area are ticks 960 and 1 920, 960 ticks apart (more than the 384-tick spacing), so both stroke points are kept; note 2 starts at 15 360 and is outside the stroke.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWEditors/Expression.swift app/Sources/VizDAWEditors/ExpressionPaneView.swift app/Sources/VizDAWEditors/PianoRollContainer.swift app/Tests/VizDAWEditorsTests/ExpressionTests.swift app/Tests/VizDAWAppModelTests/ExpressionPaneTests.swift
git commit -m "feat(editors): per-note expression pane for MIDI 2.0 per-note data

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 13: Nudge in time and pitch, with one Settings page

**Files:**
- Create: `app/Sources/VizDAWAppModel/Nudge.swift`, `app/Sources/VizDAW/EditingSettingsView.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift`, `app/Sources/VizDAW/AppCommands.swift`
- Test: `app/Tests/VizDAWAppModelTests/NudgeTests.swift` (new)

**Interfaces:**
- Consumes: `Snap`, `MeterMath`, `Scale`, `ScaleMath` (Task 6), `EditorModel.{selection, edit, info, laneEvents, isSelected(event:), layout}`, `EventEdit.{nudge, transposeInScale}`.
- Produces: `enum NudgeTimeStep: Codable, Hashable, Sendable { grid, division(Snap.Division, triplet: Bool), ticks(Int64) }`; `enum NudgePitchStep: Codable, Hashable, Sendable { semitones(Int), scaleSteps(Int) }`; `enum NudgeDirection: Sendable { earlier, later, up, down }`; `struct NudgeSettings: Codable, Equatable, Sendable { time = .grid; pitch = .semitones(1); followKeySignature = true; root: UInt8 = 0; scale: Scale = .major; ticks(at:meter:snap:) -> Int64; key(at:meter:) -> (root: UInt8, mask: UInt16) }`; `@Observable @MainActor final class EditingPreferences { static key = "VizDAW.nudge"; init(defaults:); nudge: NudgeSettings }`; `EditorModel.nudgeAnchor() -> (seq: UInt64, tick: Int64)?`, `EditorModel.nudge(_:settings:) -> Bool`; app global `editingPreferences`; Settings scene; Editors menu Nudge Earlier/Later/Up/Down on ⌥←/⌥→/⌥↑/⌥↓.

Rulings 16, 17. Parent §9: "Nudge moves the selection by a set time or pitch step; pitch nudge can follow a key and scale. Both steps are set on one Settings page."

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/NudgeTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let fourFour = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

@MainActor
@Suite struct NudgeTests {
    @Test func timeSteps() {
        var snap = Snap()
        snap.division = .eighth
        snap.enabled = false
        var s = NudgeSettings()
        #expect(s.ticks(at: 0, meter: fourFour, snap: snap) == 7_680, "the grid's step even with snap off")
        s.time = .division(.quarter, triplet: true)
        #expect(s.ticks(at: 0, meter: fourFour, snap: snap) == 10_240)
        s.time = .division(.bar, triplet: false)
        #expect(s.ticks(at: 0, meter: [MeterEntryRow(tick: 0, num: 3, den: 4, sharps: 0, minor: false)], snap: snap) == 46_080)
        s.time = .ticks(0)
        #expect(s.ticks(at: 0, meter: fourFour, snap: snap) == 1)
        s.time = .ticks(240)
        #expect(s.ticks(at: 0, meter: fourFour, snap: snap) == 240)
    }

    @Test func keysFollowTheSignatureOrAreFixed() {
        var s = NudgeSettings()
        let gMajor = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 1, minor: false)]
        #expect(s.key(at: 0, meter: gMajor) == (7, Scale.major.mask))
        let aMinor = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: true)]
        #expect(s.key(at: 0, meter: aMinor) == (9, Scale.naturalMinor.mask))
        s.followKeySignature = false
        s.root = 14
        s.scale = .dorian
        #expect(s.key(at: 0, meter: gMajor) == (2, Scale.dorian.mask))
    }

    func selected(_ events: [UInt64]) throws -> (FakeSession, EditorModel) {
        let f = rollFake()
        let m = try DocumentModel(session: f)
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [LaneRef(seq: 3, lane: 31)], events: events))
        return (f, m.editor)
    }

    @Test func nudgeSendsTimeAndPitchCommands() throws {
        let (f, e) = try selected([1, 2])
        var s = NudgeSettings()
        #expect(e.nudge(.later, settings: s))
        #expect(f.edits.last == .event(edit: .nudge(events: [1, 2], ticks: 15_360, semitones: 0)))
        e.nudge(.earlier, settings: s)
        #expect(f.edits.last == .event(edit: .nudge(events: [1, 2], ticks: -15_360, semitones: 0)))
        s.pitch = .semitones(3)
        e.nudge(.up, settings: s)
        #expect(f.edits.last == .event(edit: .nudge(events: [1, 2], ticks: 0, semitones: 3)))
        s.pitch = .scaleSteps(2)
        e.nudge(.down, settings: s)
        #expect(f.edits.last == .event(edit: .transposeInScale(events: [1, 2], steps: -2, root: 0, mask: Scale.major.mask)))
    }

    @Test func nothingSelectedNothingSent() throws {
        let (f, e) = try selected([])
        #expect(!e.nudge(.up, settings: NudgeSettings()))
        #expect(f.edits.isEmpty)
    }

    @Test func refusedWhileADragIsOpen() throws {
        let (f, e) = try selected([1])
        #expect(e.beginGesture("Move Notes", lanes: [LaneKey(3, 31)]))
        #expect(!e.nudge(.up, settings: NudgeSettings()))
        #expect(f.edits.isEmpty)
        #expect(e.status == EditorModel.gestureOpenMessage)
    }

    @Test func theAnchorIsTheRangeStartElseTheEarliestSelectedEvent() throws {
        let (_, e) = try selected([2])
        #expect(e.nudgeAnchor()?.tick == 15_360)
        #expect(e.nudgeAnchor()?.seq == 3)
        e.setSelection(
            SelectionRow(timeRange: TickRange(start: 61_440, end: 122_880), lanes: [LaneRef(seq: 3, lane: 31)], events: [2]))
        #expect(e.nudgeAnchor()?.tick == 61_440)
    }

    @Test func preferencesPersistAndDecodeTolerantly() throws {
        let name = "VizDAWTests.nudge.\(UUID().uuidString)"
        let d = try #require(UserDefaults(suiteName: name))
        defer { d.removePersistentDomain(forName: name) }
        let p = EditingPreferences(defaults: d)
        #expect(p.nudge == NudgeSettings())
        p.nudge.pitch = .scaleSteps(2)
        p.nudge.time = .ticks(120)
        let q = EditingPreferences(defaults: d)
        #expect(q.nudge.pitch == .scaleSteps(2))
        #expect(q.nudge.time == .ticks(120))
        d.set(Data(#"{"scale":"nope","followKeySignature":false}"#.utf8), forKey: EditingPreferences.key)
        let r = EditingPreferences(defaults: d)
        #expect(r.nudge.scale == .major)
        #expect(!r.nudge.followKeySignature)
        d.set(Data("garbage".utf8), forKey: EditingPreferences.key)
        #expect(EditingPreferences(defaults: d).nudge == NudgeSettings())
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter NudgeTests 2>&1 | tail -5
```

Expected: compile errors (`NudgeSettings` unknown). (The tuple comparisons `== (7, mask)` use Swift's tuple `==`; if the compiler wants matching labels, compare `.root` and `.mask` separately.)

- [ ] **Step 2: Implement nudge**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/Nudge.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// The time step of a nudge (Plan 4b Ruling 16).
public enum NudgeTimeStep: Codable, Hashable, Sendable {
    /// The edit grid's step (also with snap off).
    case grid
    case division(Snap.Division, triplet: Bool)
    case ticks(Int64)
}

/// The pitch step of a nudge.
public enum NudgePitchStep: Codable, Hashable, Sendable {
    case semitones(Int)
    case scaleSteps(Int)
}

public enum NudgeDirection: Sendable {
    case earlier, later, up, down
}

/// The Settings page "Editing" (parent spec §9): both nudge steps.
public struct NudgeSettings: Codable, Equatable, Sendable {
    public var time: NudgeTimeStep = .grid
    public var pitch: NudgePitchStep = .semitones(1)
    /// Scale steps follow the sequence's key signature (major or natural
    /// minor); off: `root` and `scale`.
    public var followKeySignature = true
    public var root: UInt8 = 0
    public var scale: Scale = .major

    public init() {}

    enum CodingKeys: String, CodingKey {
        case time, pitch, followKeySignature, root, scale
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func field<T: Decodable>(_ key: CodingKeys, _ fallback: T) -> T {
            ((try? c.decodeIfPresent(T.self, forKey: key)) ?? nil) ?? fallback
        }
        let d = NudgeSettings()
        time = field(.time, d.time)
        pitch = field(.pitch, d.pitch)
        followKeySignature = field(.followKeySignature, d.followKeySignature)
        root = field(.root, d.root) % 12
        scale = field(.scale, d.scale)
    }

    /// The time step at `tick` (at least 1).
    public func ticks(at tick: Int64, meter: [MeterEntryRow], snap: Snap) -> Int64 {
        let m = MeterMath.bar(at: tick, meter).meter
        switch time {
        case .grid:
            var s = snap
            s.enabled = true
            return s.step(in: m)
        case let .division(d, triplet):
            var s = Snap()
            s.division = d
            s.triplet = triplet
            return s.step(in: m)
        case .ticks(let n):
            return max(n, 1)
        }
    }

    /// The key scale steps follow at `tick` (Plan 4b Ruling 17).
    public func key(at tick: Int64, meter: [MeterEntryRow]) -> (root: UInt8, mask: UInt16) {
        guard followKeySignature else { return (root % 12, scale.mask) }
        let e = MeterMath.bar(at: tick, meter).meter
        return (ScaleMath.keyRoot(sharps: e.sharps, minor: e.minor), e.minor ? Scale.naturalMinor.mask : Scale.major.mask)
    }
}

/// App-wide editing preferences, kept in UserDefaults (Plan 4b Ruling 16).
@Observable
@MainActor
public final class EditingPreferences {
    public static let key = "VizDAW.nudge"
    @ObservationIgnored private let defaults: UserDefaults

    public var nudge: NudgeSettings {
        didSet {
            if let data = try? JSONEncoder().encode(nudge) { defaults.set(data, forKey: Self.key) }
        }
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        nudge =
            defaults.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(NudgeSettings.self, from: $0) }
            ?? NudgeSettings()
    }
}

extension EditorModel {
    /// Where a nudge reads its grid and key: the selection's range start,
    /// else its earliest selected event among the cached selection lanes,
    /// else tick 0 (Plan 4b Ruling 17); nil without selected lanes.
    public func nudgeAnchor() -> (seq: UInt64, tick: Int64)? {
        guard let first = selection.lanes.first else { return nil }
        if let r = selection.timeRange { return (first.seq, r.start) }
        let ticks = selection.lanes.flatMap { l in
            (laneEvents(LaneKey(l.seq, l.lane))?.events ?? []).filter { isSelected(event: $0.id) }.map(\.tick)
        }
        return (first.seq, ticks.min() ?? 0)
    }

    /// Nudges the selected events one step (parent spec §9); one undo step.
    /// False when nothing is selected or the edit was refused.
    @discardableResult
    public func nudge(_ dir: NudgeDirection, settings: NudgeSettings) -> Bool {
        let events = selection.events
        guard !events.isEmpty else { return false }
        let anchor = nudgeAnchor()
        let meter = anchor.flatMap { info($0.seq)?.meter } ?? []
        let tick = anchor?.tick ?? 0
        switch dir {
        case .earlier, .later:
            let t = settings.ticks(at: tick, meter: meter, snap: layout.snap)
            return edit(.event(edit: .nudge(events: events, ticks: dir == .later ? t : -t, semitones: 0)))
        case .up, .down:
            let sign = dir == .up ? 1 : -1
            switch settings.pitch {
            case .semitones(let n):
                return edit(
                    .event(edit: .nudge(events: events, ticks: 0, semitones: Int8(clamping: sign * max(n, 1)))))
            case .scaleSteps(let n):
                let k = settings.key(at: tick, meter: meter)
                return edit(
                    .event(
                        edit: .transposeInScale(
                            events: events, steps: Int8(clamping: sign * max(n, 1)), root: k.root, mask: k.mask)))
            }
        }
    }
}
VIZDAW_EOF
scripts/swift.sh test --filter NudgeTests 2>&1 | tail -5
```

Expected: PASS (7 tests).

- [ ] **Step 3: The Settings page and the menu commands**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/EditingSettingsView.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel

/// Settings > Editing: both nudge steps on one page (parent spec §9, Plan
/// 4b Ruling 16).
struct EditingSettingsView: View {
    @Bindable var prefs: EditingPreferences

    private enum TimeChoice: Hashable { case grid, division, ticks }

    private var timeChoice: Binding<TimeChoice> {
        Binding(
            get: {
                switch prefs.nudge.time {
                case .grid: .grid
                case .division: .division
                case .ticks: .ticks
                }
            },
            set: { choice in
                switch choice {
                case .grid: prefs.nudge.time = .grid
                case .division: prefs.nudge.time = .division(.sixteenth, triplet: false)
                case .ticks: prefs.nudge.time = .ticks(240)
                }
            })
    }

    private var scaleSteps: Binding<Bool> {
        Binding(
            get: { if case .scaleSteps = prefs.nudge.pitch { true } else { false } },
            set: { on in prefs.nudge.pitch = on ? .scaleSteps(amount) : .semitones(amount) })
    }

    private var amount: Int {
        switch prefs.nudge.pitch {
        case .semitones(let n), .scaleSteps(let n): n
        }
    }

    private var amountBinding: Binding<Int> {
        Binding(
            get: { amount },
            set: { n in
                if case .scaleSteps = prefs.nudge.pitch {
                    prefs.nudge.pitch = .scaleSteps(n)
                } else {
                    prefs.nudge.pitch = .semitones(n)
                }
            })
    }

    var body: some View {
        Form {
            Section("Nudge in Time") {
                Picker("Step", selection: timeChoice) {
                    Text("Edit Grid").tag(TimeChoice.grid)
                    Text("Note Value").tag(TimeChoice.division)
                    Text("Ticks").tag(TimeChoice.ticks)
                }
                if case let .division(d, triplet) = prefs.nudge.time {
                    Picker(
                        "Note Value",
                        selection: Binding(get: { d }, set: { prefs.nudge.time = .division($0, triplet: triplet) })
                    ) {
                        ForEach(Snap.Division.allCases, id: \.self) { Text($0.title).tag($0) }
                    }
                    Toggle(
                        "Triplet", isOn: Binding(get: { triplet }, set: { prefs.nudge.time = .division(d, triplet: $0) }))
                }
                if case let .ticks(n) = prefs.nudge.time {
                    Stepper(
                        "\(n) ticks (\(MeterMath.ppq) per quarter note)",
                        value: Binding(get: { Int(n) }, set: { prefs.nudge.time = .ticks(Int64(max($0, 1))) }),
                        in: 1...61_440, step: 60)
                }
            }
            Section("Nudge in Pitch") {
                Picker("Step", selection: scaleSteps) {
                    Text("Semitones").tag(false)
                    Text("Scale Steps").tag(true)
                }
                Stepper("Amount: \(amount)", value: amountBinding, in: 1...12)
                Toggle("Follow the sequence's key signature", isOn: $prefs.nudge.followKeySignature)
                    .disabled(!scaleSteps.wrappedValue)
                Picker("Root", selection: $prefs.nudge.root) {
                    ForEach(0..<12, id: \.self) { Text(ScaleMath.rootNames[$0]).tag(UInt8($0)) }
                }
                .disabled(!scaleSteps.wrappedValue || prefs.nudge.followKeySignature)
                Picker("Scale", selection: $prefs.nudge.scale) {
                    ForEach(Scale.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .disabled(!scaleSteps.wrappedValue || prefs.nudge.followKeySignature)
            }
        }
        .formStyle(.grouped)
        .frame(width: 460)
        .padding()
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/VizDAWApp.swift`:

Find:
```swift
/// The Plugin Manager window's model.
@MainActor let pluginManager = PluginManagerModel(plugins: appPlugins)
```
Replace:
```swift
/// The Plugin Manager window's model.
@MainActor let pluginManager = PluginManagerModel(plugins: appPlugins)

/// Editing preferences: the nudge steps (Plan 4b Ruling 16).
@MainActor let editingPreferences = EditingPreferences()
```

Find:
```swift
        Window("Plugin Manager", id: "plugins") {
            PluginManagerView(model: pluginManager)
        }
```
Replace:
```swift
        Window("Plugin Manager", id: "plugins") {
            PluginManagerView(model: pluginManager)
        }
        Settings {
            EditingSettingsView(prefs: editingPreferences)
        }
```

In `app/Sources/VizDAW/AppCommands.swift`:

Find:
```swift
            Divider()
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
        }
```
Replace:
```swift
            Divider()
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
            Divider()
            nudge("Nudge Earlier", .earlier, .leftArrow)
            nudge("Nudge Later", .later, .rightArrow)
            nudge("Nudge Up", .up, .upArrow)
            nudge("Nudge Down", .down, .downArrow)
        }
```

Find:
```swift
    private func show(_ id: UUID?) {
```
Replace:
```swift
    /// ⌥-arrows nudge the selection (Plan 4b Ruling 16); off while a text
    /// field has the keyboard, so ⌥-arrows there still move the cursor.
    private func nudge(_ title: String, _ dir: NudgeDirection, _ key: KeyEquivalent) -> some View {
        Button(title) { doc?.editor.nudge(dir, settings: editingPreferences.nudge) }
            .keyboardShortcut(key, modifiers: .option)
            .disabled(
                doc == nil || !(doc?.transportKeysEnabled ?? false) || (doc?.editor.selection.events.isEmpty ?? true))
    }

    private func show(_ id: UUID?) {
```

- [ ] **Step 4: Run the tests and build**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | grep -v "build-system native" | head
scripts/swift.sh test 2>&1 | tail -2
```

Expected: clean; 316 tests.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/Nudge.swift app/Sources/VizDAW/EditingSettingsView.swift app/Sources/VizDAW/VizDAWApp.swift app/Sources/VizDAW/AppCommands.swift app/Tests/VizDAWAppModelTests/NudgeTests.swift
git commit -m "feat(app): nudge in time and pitch (key and scale aware) with a Settings page

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---

### Task 14: The 4b checklist and the final gate

**Files:**
- Modify: `app/README.md`

**Interfaces:**
- Consumes: everything above.
- Produces: README section "Piano roll (plan 4b)", checklist steps 36–49.

Spec §6 "Manual checklist in `app/README.md` for each phase (drag feel, playhead smoothness, audition, tear-off windows, accessibility with VoiceOver)".

- [ ] **Step 1: README**

In `app/README.md`, after the last line of step 35 (`toolbar controls (tool, Snap, Blocks, Light, Follow, zoom) are labelled.`), append:

```markdown

### Piano roll (plan 4b)

Use the demo project (File > Create Demo Project) unless a step says
otherwise. The audio engine starts on the first Play: until then pressing a
note shows "Notes sound once the audio engine runs: press Play once." once,
so press Space twice before the sound steps.

36. Double-click the Chords block: the detail pane shows the piano roll: a
    keyboard on the left with C3, C4 and C5 labelled, the ruler, and one
    three-note chord per bar in blue with shaded black-key rows. Keyboard
    off hides the keys (the column stays). Taller Rows / Shorter Rows and
    the zoom buttons change the rows and the zoom; Cmd-scroll zooms around
    the pointer; scrolling moves both ways and the keyboard follows.
37. Sound: click a note: it is outlined (selected) and sounds while the
    button is held. Click a key: it sounds; drag down the keyboard: each key
    plays in turn; release: silence.
38. Cursor zones: over a note's left edge the pointer is an open hand, over
    the middle up-down arrows, over the right edge a resize arrow. Drag the
    middle of a note up two rows: it follows in semitones and sounds at each
    new pitch; release, then Edit > Undo Transpose Notes restores it in one
    step. Drag the left edge one quarter right: it moves on the grid (Undo
    Move Notes). Drag the right edge: the length changes on the grid (Undo
    Resize Notes). Option-drag the left edge: a copy appears (Undo Copy
    Notes). Press Escape during any drag: everything snaps back, the sound
    stops and Edit has no new undo step.
39. Rubber band: with the arrow, drag over empty space around a chord: its
    three notes are selected; drag one of them up: all three move together;
    drag far up: the chord stops when its top note reaches G9 (no squashing).
40. Pencil: choose Pencil and click an empty row: a one-grid-step note
    appears and sounds while held (Undo Insert Note). Drag right from empty
    space: the new note's length follows the pointer in grid steps. Press G
    (snap off): the pencil paints any length. Eraser: a click deletes a
    note; dragging across notes deletes each one crossed, one undo step
    "Erase Notes". Select notes and press Delete: they go.
41. Several lanes: double-click a Groove block (it opens Groove), then
    double-click its Bass block; Lanes > Drums adds the drum lane in a
    second colour, drawn fainter. Press a drum note: "Insert Into" switches
    to Drums and the pencil now adds notes there. Lanes > Drums again hides
    it.
42. Playing: Transport > Loop on, Space; drag a chord note up and hold it
    while the loop restarts: the dragged note keeps sounding until you
    release; the music follows the edit; Stop silences everything.
43. Torn window: Option-Cmd-1 opens "Piano Roll — Untitled" on the same
    target. With it in front: Space plays and stops, Cmd-Z undoes, Cmd-2
    switches that window to List, Option-Cmd-1 opens another window on its
    target. Press and hold a keyboard key in it and close the window with
    Cmd-W: no note keeps sounding.
44. Audio: File > Create Demo Project with Audio; press Cmd-1 with nothing
    selected: the piano roll shows Song with Chords and Loop. Under the
    notes the strip "Audio 1-2" holds two teal regions labelled Loop at
    bars 1 and 5 (silent until sub-project 5). M and S toggle mute and solo
    of the "Audio 1-2" mixer strip (Window > Mixer shows the same); Undo
    reverts. Drag a region one bar right: it moves on the grid (Undo Move
    Region). The Audio toggle hides the strip.
45. Expression: the pane under the strip shows "Bend ▾" and a centre line.
    Select a chord note and draw across it in the pane: a stepped line
    follows the pointer inside that note only (Undo Draw Expression). "Bend
    ▾" > Registered 74 (Timbre) shows that kind; the eraser removes values
    (Undo Erase Expression). Save As, close, reopen: the values and the
    chosen kind are still there. (The built-in instruments are MIDI 1.0 and
    ignore per-note data; the compile report says so.)
46. Nudge: VizDAW > Settings… (Cmd-,) shows "Nudge in Time" (Edit Grid) and
    "Nudge in Pitch" (Semitones, Amount 1). Select a chord: Option-Right
    moves it one grid step later, Option-Left back, Option-Up a semitone up.
    Choose Scale Steps with "Follow the sequence's key signature" on:
    Option-Up turns C-E-G into D-F-A (C major). Turn following off, Root D,
    Scale Dorian: steps follow D Dorian. Quit and relaunch: the settings are
    kept. In a lane name field Option-arrows move the text cursor, not notes.
47. Busy: with an AU v3 instrument whose editor opens slowly (plan 3c
    steps), start opening it and at once drag a piano-roll note and
    release: no note keeps sounding, and the drag lands (perhaps a moment
    later) as one undo step.
48. VoiceOver (Cmd-F5): the piano roll is a group "Piano Roll" whose notes
    read like "Note C4, bar 1 beat 1, velocity 56"; VO-Space selects one.
    The keyboard reads "Keyboard", the strip "Audio Regions" with "Mute
    Audio 1-2" and "Solo Audio 1-2", the pane "Expression: Per-Note Pitch
    Bend".
49. Layout: change Keyboard, row height and zoom, show an extra lane, Save,
    close and reopen: the piano roll and its torn windows come back the
    same.
```

- [ ] **Step 2: Final gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace
cargo test -p core-engine --features rt-check --test rt_alloc
cargo test --release -p core-ffi --test events lane_events_of_100_000_events_is_fast
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --smoke; echo "smoke exit $?"
scripts/make-app.sh
```

Expected: all green; 625 Rust tests, 316 Swift tests; the render path still allocates nothing with the second tracker; `--smoke` exits 0; `dist/VizDAW.app` is built. Then walk checklist steps 36–49 by hand (the user's step; note anything that fails in the hand-off).

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md
git commit -m "docs(app): piano roll checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---

## Self-Review

**1. Spec coverage (phase 4b of `2026-10-16-editors-design.md`, with §3, §5, §6 and parent §9):**

| Spec | Task |
|---|---|
| §3.5 `PitchAxis` / `ValueAxis`, pure and unit-tested | 6 |
| §4b note drawing over a pitch axis | 6, 7, 9 |
| §4b several lanes at once with per-lane colour | 8 (Ruling 9), 9 (palette, fainter inactive lanes) |
| §4b audio regions in their own strip between notes and controller pane, mute/solo per audio output path | 11 (Rulings 13, 19); the pane under it is the per-note expression pane (12) |
| §4b optional keyboard beside it; click auditions | 10 (Rulings 6, 12) |
| §4b / parent §9 cursor zones: start moves, middle transposes, end resizes; ⌥ copies | 2 (`Duplicate`, `Resize`), 3, 7, 9 (Rulings 3, 4, 5, 20) |
| §4b pencil inserts at the default length or paints a length; eraser deletes | 7, 9 (Rulings 7, 8) |
| §4b notes sound while dragged (audition) | 1 (engine keeps them through recompiles), 4 (retries), 8, 9, 10 (Rulings 1, 2, 6) |
| §4b per-note expression lanes for MIDI 2.0 per-note data | 2 (`SetPerNote`), 3, 12 (Rulings 14, 15) |
| §4b / parent §9 nudge time and pitch; pitch can follow a key and scale; steps on one Settings page | 2 (`TransposeInScale`), 6 (`Scale`, `ScaleMath`), 13 (Rulings 16, 17) |
| §3.7 the piano roll in the detail pane and in tear-off windows | 5 (focus), 10 (Rulings 18, 22) |
| §3.2 one drag one undo step, intermediate edits follow the pointer; §3.8 Busy contract | 9 (through `EditorModel+Gestures`), 11 (region drags), 12 (strokes) |
| §3.5 accessibility: elements with role, label "Note C4, bar 3 beat 1, velocity 100", actions | 7 (`PianoRollA11y`), 9, 10, 11, 12 (Ruling 21) |
| §5 status for user-action failures, silent Busy, a drag whose lanes vanish aborts | 4, 8 (vanished lanes), 9 (inherits `afterSync`) |
| §6 tests: `PitchAxis` maths, hit-testing and cursor zones, edit mapping of every new variant; manual checklist with audition, tear-off, VoiceOver | 3 (`every_variant_applies_and_undoes`, 50), 6, 7, 14 |
| 4a carry-overs: torn-window focus; Busy note-off retry; Locate/wrap and handover tests; media revs test; weak MenuAction closures; `CanvasElement` Swift 6 warning | 5; 4; 1; 1; 4; 4 |

Out of scope here and left to later plans: velocity and controller lanes in the controller pane (4d's Strip Chart), exact mode for piano-roll drags (4c), the key-binding registry (4f rebinds ⌥-arrows, G, Delete and ⌘1–⌘4), audio-region trim and split in the strip (sub-project 5), MIDI 2.0 output of per-note data (Ruling 15).

**2. Placeholder scan:** every step carries its code or exact Find/Replace text and its command. The remaining editors' "arrives in plan 4c/4d/4e" text is the specified placeholder for kinds other than the piano roll.

**3. Type consistency:** Rust names and their Swift forms: `EventEdit::{Duplicate, Resize, TransposeInScale, SetPerNote}` ↔ `.duplicate(events:ticks:semitones:)`, `.resize(events:delta:)`, `.transposeInScale(events:steps:root:mask:)`, `.setPerNote(events:kind:from:to:points:)`; `PerNoteValueRow { tick, value }` ↔ `PerNoteValueRow(tick:value:)`; `PerNoteKindRow::Registered { index }` ↔ `.registered(index:)`; `StripKindRow::AudioPath { path }` ↔ `.audioPath(path:)`; `Session::new_demo_audio` ↔ `Session.newDemoAudio()`. Swift names shared across tasks and defined once: `AuditionNote(dest:pitch:)` and `EditorModel.{auditionOn(_:velocity:), auditionOff(_:), hasPendingNoteOffs, noteOffRetries, auditionHint}` (4); `TornFocus`, `DocumentStore.{tornFocus, focusTorn, blurTorn, commandTarget(focused:)}`, `EditorModel.tearOff(_:from:)` (5); `PitchName`, `Scale`, `ScaleMath`, `PitchAxis`, `ValueAxis` (6); `Snap.floor`, `NoteZone`, `NoteHit`, `PianoRollHit`, `LaneNotes`, `PianoRollGeometry`, `NoteDrag`, `Pencil`, `Eraser`, `RubberBand`, `CursorKind`, `PianoRollCursor`, `PianoRollA11y` (7); `ExpressionKind`, `PianoRollSettings`, `DetailState.lanes`, `EditorModel.laneRows(_:)`, `DetailSlot`, `PianoRollModel` (8); `PianoRollView`, `PianoRollPalette` (9); `DocumentModel.{pianoRoll(_:), dropPianoRoll(_:)}`, `TimeAxisFollower`, `PianoRollContainer.{sideWidth, addLowerPane}`, `PianoKeyboardView`, `KeyboardLabels` (10); `RegionItem`, `RegionLine`, `RegionHit`, `RegionStripLayout`, `PianoRollModel.{outputStrips, regionLanes, setOutputMute, setOutputSolo}`, `DocumentStore.{makeDemoAudio, newDemoAudio}` (11); `ExpressionGeometry`, `ExpressionPaneView` (12); `NudgeSettings`, `NudgeTimeStep`, `NudgePitchStep`, `NudgeDirection`, `EditingPreferences`, `EditorModel.{nudgeAnchor, nudge(_:settings:)}` (13). Test helpers `rollFake()` (8) and `audioFake()` (11) are reused by later tasks' tests with the same lanes and IDs.

**4. Review Focus:** each of the five lines has its tests in its owning task (Tasks 1, 2, 4, 7, 8, 9). Other input classes checked and covered: zero-length notes (Task 7 minimum width), off-scale pitches under scale steps (Tasks 2, 6), a stroke outside every note or in the side column (Task 12), mute/solo with no strip for the path or during a drag (Task 11), malformed preferences and layouts (Tasks 8, 13), a drag ended back at its start (nil targets, Tasks 7, 11), notes beyond the pitch range in a transpose (Task 7), a closing torn window while a note sounds (Task 10).

**Execution note:** the Rust tasks were checked against the tree at `5c21fc0` (signatures of `EventCmd`, `edit_events`, `Document::{event, insert_event}`, `Builder`, `Engine`, `Renderer::apply`, `host::play`, `edit.rs::event_cmd`, `events.rs::per_note_of`); the Swift tasks against the 4a app sources (`EditorCanvas`, `ToolMachine`, `EditorModel+Gestures`, `ArrangementView`, `DetailPane`, `EditorWindows`, `AppCommands`, `FakeSession`). The plan was not rehearsed end to end on a scratch clone; an executor meeting a compile error in a Find/Replace excerpt fixes it in the task's own files and keeps the interfaces above.
