# VizDAW 4c — List Editor, Select-and-Modify, Exact Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The detail pane and torn-off windows gain a keyboard-first list editor (every event kind, references and audio regions, plus the sequence's tempo, meter/key and marker entries as rows; bar-number typing, single sub-field editing, Enter commits and the list follows the event, notes played on a MIDI source enter pitch and velocity, insert repeats the last kind, optional End/Length/Release/SMPTE columns, kind and destination filters applied to all lists or saved as default, opening a reference opens its sequence); a select-and-modify panel (select half: rules on kind and attributes with equal/not equal/inside/outside, replacing, adding to or refining the selection; modify half: transpose chromatic, diatonic, inverted, to a scale or by a note map, changes of velocity, release velocity, duration and value by set, scale, add, limit, compress or legato with a constant, shaped or oscillating amount, quantize, density, move, scale time, trim start, set destination, substitute, delete, in place or to a copy), named templates in a menu, Modify Again, menu commands that open the panel set to their task, and exact mode (a drag opens the panel pre-filled instead of applying).

**Architecture:** Rust first. `core-model` gains the rule conditions and modifications the panel needs (`rule.rs`, `modify.rs`), templates that remember "to a copy", and `EventCmd::Replace` (one undo step per list cell edit). `platform-macos` gains a `NoteListener` (CoreMIDI input from every source) and `core-ffi` a `NoteInput` object that queues note-ons, flat select-and-modify records (`modify.rs`: a rule is a list of conditions, UniFFI enums cannot hold themselves), `EditCommand::{SelectAndModify, ModifyAgain, Settings}`, `EventEdit::Replace`, and the reads `modify_templates`, `last_modify`, `seconds_at`, `ticks_at`. Swift then adds, in `VizDAWAppModel`, pure list maths and text (`ListFormat`, `ListRows`, `ListEdit`), the per-slot `ListEditorModel` with `ListPreferences`, `NoteInputHub`, the pure panel configuration (`SelectModify`, `PanelFields`, `ExactMode`), the per-document `SelectModifyModel`, text-editing owners and panel focus; in `VizDAWEditors` the `ListEditorView` (an `NSTableView`, so VoiceOver reads a table) and the rubber band's exact-mode hook; in `VizDAW` the list pane, the floating select-and-modify panel, the Modify menu and the Insert Event and Exact Mode commands.

**Tech Stack:** Rust 2024 (`core-model`, `platform-macos` with `objc2-core-midi`, `core-ffi` with UniFFI 0.32.2, `im` 15, `serde`), Swift 6 / SwiftPM (SwiftUI shell, AppKit `NSTableView`/`NSPanel`, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-16-editors-design.md` (§3 architecture, §4 phase 4c, §5 errors, §6 testing), parent `docs/superpowers/specs/2026-10-15-vizdaw-design.md` §5 (time, events) and §9 (List editor paragraph; "Shared editor behaviours": select-and-modify panel, templates, Modify Again, menu commands, exact mode). Previous plans for conventions: `docs/superpowers/plans/2026-10-16-editors-foundation.md`, `docs/superpowers/plans/2026-10-17-editors-piano-roll.md`; ledgers `.superpowers/4a-ledger.md`, `.superpowers/4b-ledger.md` list the carried-over minors (Ruling 25).

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp4-editors` (checked out; code head `42776c9` before Task 1, plus this plan's commit). Commit at the end of every task, adding only the files the task lists; never push. End every commit message with the attribution lines your harness requires (the blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and the `Claude-Session:` line).
- Rust stable, edition 2024; crate manifests keep `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`. License GPL-3.0-only.
- `core-model` gains only the rule variants (Task 1), the modify additions (Tasks 2–3), `ModifyTemplate.to_copy` (Task 1) and `EventCmd::Replace` (Task 3); existing `Rule`, `Modify`, `Transpose` variants keep their names and serde shapes (saved projects and templates must load). `core-model` stays free of OS-specific dependencies and builds and tests on Linux. `core-ffi` is platform-neutral except `#[cfg(target_os = "macos")]` code. `platform-macos` gains only `NoteListener` (Task 5).
- `core-model`, `core-engine`, `core-ffi` and `platform-macos` keep their `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. Tests may `unwrap` and index. New `unsafe` only in `platform-macos::midi::NoteListener`, each block with a `// SAFETY:` comment like its neighbours.
- No panic crosses the FFI: every exported function and method returns `Result<_, VizError>` (constructors that cannot fail return `Arc<Self>`) and runs inside `ffi_guard`. No session `inner` lock is held across plugin code. Polling paths skip `VizError::Busy` silently; gestures follow spec §3.8 through `EditorModel+Gestures.swift`.
- New UniFFI enum variants are appended at the END of existing enums (`EditCommand`, `EventEdit`); new record and enum types are new. No existing record gains a field.
- Swift: the generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings. Never use SwiftUI's `@State` or `@FocusState` (the CLT SDK cannot expand them); keep view state, including text-field drafts, in `@Observable` models. Models and pure logic are tested (Swift Testing, `@Test`, `#expect`); AppKit views get behaviour tests through internal entry points and the manual checklist.
- Drawing (spec §2): custom `NSView`s or standard AppKit controls with Core Graphics; no Metal. Closures stored by views capture `[weak self]`.
- `FakeSession` implements the whole generated `SessionProtocol`; Task 6 adds the four new `Session` methods to it.
- Generated files are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command after a Rust change.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Run `cargo fmt --all` first. Baseline before Task 1: 628 Rust tests, 341 Swift tests.
- Code in the steps is complete for new files (written whole with `cat > … <<'VIZDAW_EOF'`). Changes to existing files are given as exact **Find / Replace** excerpts of the tree the previous task left; apply them with an editor (the Edit tool). If an excerpt does not match because a fix landed meanwhile, make the same change by hand and keep going.
- Spec values used verbatim: 15 360 ticks per quarter note; positions shown and typed as bar·beat·unit with 960 units per beat (`UNITS_PER_BEAT`, as the transport shows them); MIDI note numbers with C-1 = 0 and middle C 60 = "C4"; one drag is one undo step; a gesture's intermediate edits that hit Busy are dropped and the final edit is retried each tick; editors never hold Rust objects across the run loop; text fields turn the transport keys off while they have the keyboard.

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no requirement spells out, each pinned by a named test in its owning task:

1. **Garbage or out-of-range text typed into a list cell** ("abc", bar 0 beat 0, velocity 300, a unit of 99 999 999 999, an end before the start) — nothing changes, the status line says why, the cell shows its old value again, and huge numbers never trap. Pinned: Task 7 `hugeAndBadPositionsAreRefused`; Task 8 `badTextIsRefusedWithAMessage`; Task 9 `aRefusedCommitLeavesTheDocumentAndSaysWhy`.
2. **Space and Return typed into list cells and panel fields** — a space or newline goes into the field and playback never starts; Return on the list (not editing) edits the cursor row instead of returning the transport to the start; Space on the list still plays. Pinned: Task 6 `transportKeysAreOffWhileThePanelIsKey`, `returnIsOffWhileAListHasFocusButSpaceIsNot`, `laneNameAndListCellsDoNotClearEachOther`; Task 10 `editingACellTurnsTheTransportKeysOffUntilItEnds`.
3. **Select-and-modify with nothing to act on** — no lanes selected, an empty selection, a rule matching nothing: no undo step, no crash, and a status line saying what to select. Pinned: Task 3 `a_modify_that_changes_nothing_adds_no_undo_step`; Task 12 `applyWithoutLanesSaysWhatToSelect`, `applyWithNothingTurnedOnSaysSo`.
4. **Exact mode with Escape and with drags it does not map** — the dragged notes go back where they were with no undo step, the panel opens pre-filled once, Escape opens nothing, and drags exact mode does not map (a pencil click) still apply. Pinned: Task 14 `anExactDragLeavesNoUndoStepAndOpensThePanel`, `escapeInExactModeOpensNothing`, `aPencilClickInExactModeStillInserts`.
5. **The edited event moves far away or vanishes** — typing a bar number moves the event out of view: the list's cursor and the document selection follow it and the table scrolls to it; a row deleted meanwhile (undo in another window) makes the commit do nothing quietly. Pinned: Task 9 `theCursorFollowsAMovedEvent`, `committingARowThatVanishedDoesNothing`.

## Plan Rulings

Decisions the spec leaves open, each implemented and tested as stated (format: Ruling: what — why — cost if wrong):

1. **Ruling:** `core-model` gains `Rule::{ReleaseVelocityRange, ValueRange, ControllerRange, Bracketed}`, `Transpose::{Diatonic, Invert}`, `Modify::{NoteMap, Change, TrimStart, Substitute, Density, Delete}`, `ModifyTemplate.to_copy` and `EventCmd::Replace` — why: parent §9's select half names release velocity, value and bracketing conditions and its modify half names operations the core lacks; the list needs one undo step per cell edit; spec §3 lists no core additions before 4d — cost if wrong: core surface to fold elsewhere.
2. **Ruling:** The old `Modify::{Velocity, Duration, ControllerValue, Legato}` stay untouched (projects and templates that used them load); the panel produces only `Modify::Change`, and the FFI reads the old ones back as `ModifyRow::Change` rows — cost if wrong: four legacy variants.
3. **Ruling:** Amounts: `Constant`; `Line { from, to, curve }` running from the first to the last changed event's tick (curves straight, ease in, ease out, S-curve); `Oscillator { wave, low, high, period, phase }` (sine, square, triangle, saw; period in ticks, phase measured from the first changed event); `Random { low, high, seed }`, the same value for the same seed and event, with a new seed drawn at each Apply (Modify Again repeats the seed). Clock units and meter-following periods belong to the Strip Chart (4d) — cost if wrong: an amount kind more in 4d.
4. **Ruling:** The FFI rule is flat: `RuleRow { conditions, match_all }`, each `ConditionRow { test, negate }`; "not equal" and "outside" are a negated equal or inside condition; bracketing is "starts inside a note of this pitch range in the same lane" (`ConditionTest::BracketedBy`) — why: UniFFI enums cannot hold themselves (4b Ruling 3), and a panel row is one condition — cost if wrong: nested rules need a tree record.
5. **Ruling:** Density acts on controller series (CC per number, pitch bend, channel pressure, poly pressure per pitch) per lane and take: Thin removes events closer than `spacing` to the last kept one; Thicken inserts straight-line values every `spacing` ticks between neighbours (at most 100 000 per command). Substitute turns CC, pitch bend and channel pressure into CC n, pitch bend or channel pressure keeping the 32-bit value, or sets notes and poly pressure to one pitch. Trim Start moves note starts by `delta` keeping their ends (at least 1 tick; zero-length notes stay). Delete deletes — why: parent §9 names them without defining them — cost if wrong: semantics to retune.
6. **Ruling:** Compress maps the changed events' lowest value to `low` and highest to `high`, the rest in proportion (Strip Chart Compress semantics); Limit clamps into `min..=max`; Legato applies to durations only — cost if wrong: none.
7. **Ruling:** Select and Add search the selection's lanes (core `select::apply`), Refine searches the selected events; with Select or Add and no selected lane the panel refuses with "Select lanes to search first: drag in a ruler or select a block." The selection's time range is the default of a new Time condition, never applied implicitly — cost if wrong: one more implicit condition.
8. **Ruling:** Modify Again is `EditCommand::ModifyAgain`, which `Session.edit` runs through `History::modify_again` (not a core `Command`; `to_command` refuses it); the last modification lives in memory only, like the core's `last_modify` — cost if wrong: none.
9. **Ruling:** Templates are document data (core `Settings.templates`, saved in the project; saving or deleting is the undo step "Set Modify Template"); Modify > Templates runs a template at once on the selection ("one-step recall"); the panel's Templates menu loads one for editing or deletes it — cost if wrong: a menu change.
10. **Ruling:** A new "Modify" menu: Select by Rule… ⌃⌘S, Transpose… ⌃⌘T, Quantize… ⌃⌘U, Change Values… ⌃⌘M, Move… ⌃⌘V, Modify Again ⌃⌘R ("Modify Again: Transpose" names the step), Templates ▸, Exact Mode ⌃⌘E (a checked toggle). 4f's registry rebinds them — why: plain ⌘ letters are taken by macOS conventions — cost if wrong: different default keys.
11. **Ruling:** MIDI input: `platform_macos::midi::NoteListener` (its own CoreMIDI client; listens to every source present when started) feeds `core_ffi::input::NoteInput`, a queue of note-ons (the newest 256 kept; velocity 0 is a note-off and dropped; MIDI 1.0 velocity upscaled to 16 bits by the MIDI 2.0 min-center-max rule). The app's 60 Hz timer drains it only while some list has MIDI In on and hands the notes to every listening document's active list. Recording proper is sub-project 5 — cost if wrong: none (new sources need MIDI In toggled to be picked up).
12. **Ruling:** A played note goes into the cell being edited when that cell is a pitch or velocity cell of a note (its name or 0–127 value replaces the text; Enter commits); otherwise, with MIDI In on, the cursor row's note takes the played pitch and velocity as one undo step ("Edit Event") — cost if wrong: none.
13. **Ruling:** Rows are every event of the target lanes' active takes (a block target: the events whose start lies in the block), plus, for a sequence target only, its tempo points, meter/key entries and markers as rows; sorted by tick, then tempo, meter, marker, events, then lane order, then event ID. Map rows are edited by replacing the whole map (`SequenceEdit::SetTempo/SetMeter/SetMarkers`) — why: parent §5 calls tempo, meter, keys and markers "event data, editable in the list editor" while the model keeps them as sequence maps — cost if wrong: none.
14. **Ruling:** Columns: Position, End*, Length, Kind, Lane (a sequence target), Destination, Data 1, Data 2, Release*, SMPTE*, Mute; * off by default; End, Length, Release and SMPTE are the parent spec's optional columns. SMPTE is `hh:mm:ss:ff` at 24, 25 or 30 fps (default 30) from the sequence's tick 0 through its own tempo map (`seconds_at`); a typed SMPTE position is converted back (`ticks_at`) and rounded to the nearest display unit (1/960 beat) — cost if wrong: none.
15. **Ruling:** Position typing: "5" moves to bar 5's downbeat, "5.3" to beat 3 of bar 5, "5.3.480" to that unit; a leading or inner empty part keeps that sub-field: ".3" changes only the beat, "..480" only the unit, "5.." only the bar. Bars are −1 000 000…1 000 000, beats 1…999, units 0…959; anything else is refused. Lengths are typed and shown as beats.units of the beat at the event's start ("1.480"; ".240" keeps the beats) — cost if wrong: none.
16. **Ruling:** Data columns by kind: note pitch name / velocity 0–127; CC number / value 0–127; pitch bend — / −8192…8191; channel pressure — / 0–127; poly pressure pitch / 0–127; program number / bank "msb:lsb" (empty: none); SysEx byte count / hex preview (read-only); text kind / text; reference sequence name / "×loops transpose" ("×3 +2", "fill 0"); audio region "Audio" / gain dB; tempo BPM / "ramp" or "jump"; meter "n/d" / key ("D major", "F# minor"); marker "Marker" / name. Typed 0–127 values use the MIDI 2.0 upscale; a value left alone keeps its full resolution. Kind, Lane, SysEx data and region media are read-only — cost if wrong: none.
17. **Ruling:** A list edit of an event is `EventCmd::Replace` ("Edit Event", the kind cannot change) except Position and SMPTE (`Move`, "Move Events"), Destination (`SetDest`) and Mute (`SetMute`) — cost if wrong: undo labels.
18. **Ruling:** Insert: Editors > Insert Event ⌘I and the list bar's Insert button insert the last inserted kind (Note at first); the button's menu picks another kind, which becomes the last. Position: the cursor row's tick, else the stopped transport's tick when it lies in the target, else the target's start. Lane: the target's lane, else the cursor row's, else the sequence's first lane. Defaults: note C4 velocity 100 one quarter; CC 7 = 64; pitch bend centre; pressures 0; program 0, no bank; SysEx `7E 7F 09 01`; text "Text"; tempo the tempo in effect (no ramp); meter/key the one in effect at the next barline; marker "Marker n". References and regions are not insertable here (the arrangement makes them) — cost if wrong: none.
19. **Ruling:** Filters: each list has its own (`DetailState.listFilter`, saved with the project; nil = the default); "Apply to All Lists" copies it to the detail pane's and every torn window's list state; "Save as Default" stores it, with the column choices, as an app preference (`UserDefaults` key `VizDAW.list`). The destination filter hides MIDI events whose own destination, else their lane's, is not shown; references, regions, text and map rows ignore it. The hidden kinds also travel as the `ViewFilterRow` when the panel is opened from that list — cost if wrong: none.
20. **Ruling:** Keyboard: ↑/↓ move the cursor (selected rows become the document selection); Return or Enter edits the cursor row's Position cell; Tab and Shift-Tab move between editable cells; Return commits and the cursor follows the event; Escape cancels; Delete deletes the selected rows; ⌘↓, or a double-click on a reference row's Kind or Data 1 cell, opens the referenced sequence. Transport Return is off while a list has the keyboard (`DocumentModel.returnKeyEnabled`); Space still plays — cost if wrong: none.
21. **Ruling:** `EditorModel.textEditing` becomes derived from a set of owners (`beginTextEditing(_:)` / `endTextEditing(_:)`) so a lane name field and a list cell no longer clear each other (4a ledger T16 "shared textEditing Bool") — cost if wrong: none.
22. **Ruling:** The select-and-modify panel is one floating `NSPanel` per document. While it is key it is the menu commands' target (`DocumentStore.panelFocus`, after a key torn window) and the transport keys are off (`EditorModel.panelKey`); Apply acts on that document's current selection. Apply refused with Busy shows the status line (a user action, not retried) — cost if wrong: none.
23. **Ruling:** Exact mode is a per-document toggle (not saved), Modify > Exact Mode ⌃⌘E and a piano roll bar toggle. With it on a drag still previews; at mouse-up the gesture is cancelled (no undo step) and the panel opens on the dragged events: a move → Move; an ⌥-copy → Move or Transpose to a copy; a middle-zone drag → Transpose; an end-zone drag → Change Duration by Add; a rubber band → Select by Rule with Kind, Time and Pitch conditions; an arrangement block move within its lane → Move. Cross-lane moves, block copies, the pencil, the eraser, region moves and expression strokes apply as usual. Scale-time drags and Strip Chart lines join in 4d — cost if wrong: more drags to map.
24. **Ruling:** Opening a reference from the list retargets that list to the referenced sequence and pushes the arrangement's navigation (Back ⌘[ returns the arrangement; the list keeps its target) — cost if wrong: none.
25. **Ruling:** 4a ledger T2 round-trip gaps (LoopCount::Fill, a reference length, a program without bank, assignable and pitch-bend per-note values, the other text kinds, the other fade shapes, 7-bit SysEx) land in Task 5; 4b ledger T3 "no negative FFI-level test for new variants" lands in Task 4 — cost if wrong: none.
26. **Ruling:** New pure tests go to `VizDAWAppModelTests` (the list and panel maths live in `VizDAWAppModel`) and view behaviour tests there too with `FakeSession` (4b Ruling 23) — cost if wrong: none.

## Spec gaps and contradictions found while planning

- §3 lists no `core-model` additions before 4d, but the panel's conditions and operations and one-step list edits have no core command (Ruling 1).
- §4c "pitch/velocity by playing a note (MIDI input)": the app has no MIDI input; MIDI recording is sub-project 5 (Ruling 11).
- Parent §5 calls tempo, meter, keys and markers event data editable in the list, but the model stores them as sequence maps (Ruling 13).
- Parent §9 "density", "substitute", "trim start", "compress" and "bracketing events" are named, not defined (Rulings 4–6).
- Parent §9 exact mode lists scale-time and Strip Chart line drags, which do not exist before 4d (Ruling 23).
- §3.2 lists `ModifyAgain` as an `EditCommand` variant, while the core has it as a `History` method, not a `Command` (Ruling 8).
- Parent §9 "oscillating amount" gives no units (Ruling 3); "saved as default" does not say per project or per user (Ruling 19).

## File Structure

```
crates/core-model/
  src/rule.rs                 ReleaseVelocityRange, ValueRange, ControllerRange, Bracketed (Task 1)
  src/modify.rs               to_copy (1); Diatonic, Invert, NoteMap, Change, amounts (2);
                              TrimStart, Substitute, Density, Delete (3)
  src/command/select.rs       apply_modify returns the new targets (Task 2)
  src/command/event.rs        EventCmd::Replace (Task 3)
  tests/select_modify.rs      tests (Tasks 1–3)
  tests/cmd_event.rs          Replace test (Task 3)
crates/platform-macos/
  src/midi.rs                 NoteListener (Task 5)
  tests/midi.rs               listener test (Task 5)
crates/core-ffi/
  src/modify.rs               rule/modify/template records and conversions (Task 4)
  src/edit.rs                 EditCommand::{SelectAndModify, ModifyAgain, Settings}, EventEdit::Replace (4)
  src/session.rs              ModifyAgain path, modify_templates, last_modify (4); seconds_at, ticks_at (5)
  src/input.rs                NoteInput (Task 5)
  src/lib.rs                  modules and re-exports (Tasks 4, 5)
  tests/{modify,edit}.rs      tests (Task 4); tests/{events,input}.rs (Task 5)
app/
  Sources/VizDAWAppModel/     EditorModel, EditorModel+Modify, DocumentStore, DocumentModel (6);
                              ListFormat (7); ListRows, ListEdit (8); ListPreferences, ListEditorModel,
                              NoteInputHub, EditorTypes, DocumentModel lists (9); SelectModify,
                              PanelFields, ExactMode (11); SelectModifyModel (12);
                              EditorModel+Gestures exact mode (14)
  Sources/VizDAWEditors/      LaneHeaderView owners (6); ListEditorView (10); PianoRollView band (14)
  Sources/VizDAW/             AppCommands (6, 10, 13, 14); ListEditorPane, DetailPane, EditorWindows,
                              VizDAWApp (10); ModifyPanel, DocumentWindow (13); PianoRollPane (14)
  Tests/VizDAWAppModelTests/  FakeSession, ModifyReadTests, DocumentModelTests (6); ListFormatTests (7);
                              ListRowsTests, ListEditTests (8); ListEditorModelTests (9);
                              ListEditorViewTests (10); SelectModifyTests, ExactModeTests (11);
                              SelectModifyModelTests (12); ExactGestureTests (14)
  README.md                   list and select-and-modify checklist steps 51–66 (Task 15)
```

---
### Task 1: Release-velocity, value, controller and bracketing conditions; templates remember "to a copy"

**Files:**
- Modify: `crates/core-model/src/rule.rs`
- Modify: `crates/core-model/src/modify.rs`
- Test: `crates/core-model/tests/select_modify.rs`

**Interfaces:**
- Consumes: `rule::matches`, `Document::{locate, lane}`, `Lane::take`, `Take.events` (an `OrdMap<(Tick, EventId), Event>`).
- Produces: `Rule::ReleaseVelocityRange { low: u16, high: u16 }`, `Rule::ValueRange { low: u32, high: u32 }` (CC, pitch-bend and pressure values, program numbers), `Rule::ControllerRange { low: u8, high: u8 }`, `Rule::Bracketed { by: Box<Rule> }` (starts inside a note of the same lane and take that matches `by`, the event itself excluded); `ModifyTemplate.to_copy: bool` (`#[serde(default)]`).

Ruling 1. Ranges are inclusive like the existing ones. `Bracketed` looks only at notes (other kinds have no span) whose span `tick..tick + duration` contains the event's tick.

- [ ] **Step 1: Write the failing tests**

In `crates/core-model/tests/select_modify.rs`, Find:

```rust
    let t = ModifyTemplate {
        rule: Some(Rule::Kind {
            kinds: vec![EventKind::Note],
        }),
        mode: SelectMode::Select,
        modify: Some(Modify::Quantize(Quantize::Grid {
            grid: Tick(Q / 4),
            strength: 80,
        })),
    };
```

Replace:

```rust
    let t = ModifyTemplate {
        rule: Some(Rule::Kind {
            kinds: vec![EventKind::Note],
        }),
        mode: SelectMode::Select,
        modify: Some(Modify::Quantize(Quantize::Grid {
            grid: Tick(Q / 4),
            strength: 80,
        })),
        to_copy: true,
    };
```

Append to the same file:

```rust
#[test]
fn value_controller_release_and_bracketing_rules() {
    let d = doc();
    // ev3 is CC7 = 100; the notes have release velocity 0.
    assert_eq!(matching(&d, Rule::ValueRange { low: 50, high: 150 }), vec![3]);
    assert_eq!(matching(&d, Rule::ControllerRange { low: 7, high: 7 }), vec![3]);
    assert!(matching(&d, Rule::ControllerRange { low: 8, high: 127 }).is_empty());
    assert_eq!(
        matching(&d, Rule::ReleaseVelocityRange { low: 0, high: 0 }),
        vec![0, 1, 2, 4]
    );
    // ev5: a CC at 5Q, inside ev4 (72, 4Q..6Q).
    let d = apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(
                Tick(5 * Q),
                EventBody::Cc { index: 1, value: 5 },
            )],
        }),
    )
    .unwrap()
    .doc;
    let under_72 = Rule::Bracketed {
        by: Box::new(Rule::PitchRange { low: 72, high: 72 }),
    };
    assert_eq!(matching(&d, under_72), vec![5]);
    // No note starts inside another note; ev3 at 3Q is at ev2's end (excluded).
    let under_any_note = Rule::Bracketed {
        by: Box::new(Rule::Kind {
            kinds: vec![EventKind::Note],
        }),
    };
    assert_eq!(matching(&d, under_any_note), vec![5]);
    // "Outside" is a negated range.
    let outside = Rule::All(vec![
        Rule::Kind {
            kinds: vec![EventKind::Note],
        },
        Rule::Not(Box::new(Rule::PitchRange { low: 60, high: 64 })),
    ]);
    assert_eq!(matching(&d, outside), vec![2, 4]);
}

#[test]
fn program_numbers_match_value_ranges() {
    let d = apply(
        &doc(),
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(
                Tick(0),
                EventBody::Program {
                    program: 9,
                    bank: None,
                },
            )],
        }),
    )
    .unwrap()
    .doc;
    assert_eq!(matching(&d, Rule::ValueRange { low: 9, high: 9 }), vec![5]);
}

#[test]
fn a_template_saved_before_to_copy_existed_still_loads() {
    let t: ModifyTemplate =
        serde_json::from_str(r#"{"rule":null,"mode":"select","modify":"legato"}"#).unwrap();
    assert!(!t.to_copy);
    assert_eq!(t.modify, Some(Modify::Legato));
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify 2>&1 | tail -15
```

Expected: compile errors (`no variant named ValueRange`, `struct ModifyTemplate has no field named to_copy`).

- [ ] **Step 3: Implement the rules**

In `crates/core-model/src/rule.rs`, Find:

```rust
    /// The sub-rule does not match.
    Not(Box<Rule>),
}
```

Replace:

```rust
    /// The sub-rule does not match.
    Not(Box<Rule>),
    /// Note release velocity in range.
    ReleaseVelocityRange {
        /// Lowest.
        low: u16,
        /// Highest.
        high: u16,
    },
    /// CC, pitch-bend and pressure value, or program number, in range.
    ValueRange {
        /// Lowest.
        low: u32,
        /// Highest.
        high: u32,
    },
    /// Control-change controller number in range.
    ControllerRange {
        /// Lowest.
        low: u8,
        /// Highest.
        high: u8,
    },
    /// Starts inside a note of the same lane and take (`note.tick <= tick <
    /// note.tick + duration`, the event itself excluded) that matches `by`.
    Bracketed {
        /// What the bracketing note must match.
        by: Box<Rule>,
    },
}
```

Find:

```rust
        Rule::Not(r) => !matches(doc, r, lane_dest, ev, index),
    }
}
```

Replace:

```rust
        Rule::Not(r) => !matches(doc, r, lane_dest, ev, index),
        Rule::ReleaseVelocityRange { low, high } => matches!(
            ev.body,
            EventBody::Note { release_velocity, .. } if (*low..=*high).contains(&release_velocity)
        ),
        Rule::ValueRange { low, high } => match ev.body {
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. } => (*low..=*high).contains(&value),
            EventBody::Program { program, .. } => (*low..=*high).contains(&u32::from(program)),
            _ => false,
        },
        Rule::ControllerRange { low, high } => {
            matches!(ev.body, EventBody::Cc { index, .. } if (*low..=*high).contains(&index))
        }
        Rule::Bracketed { by } => bracketed(doc, by, ev),
    }
}

/// Whether a note of `ev`'s own lane and take that matches `by` sounds at
/// `ev`'s start.
fn bracketed(doc: &Document, by: &Rule, ev: &Event) -> bool {
    let Ok(loc) = doc.locate(ev.id) else {
        return false;
    };
    let Ok(lane) = doc.lane(loc.seq, loc.lane) else {
        return false;
    };
    let Some(take) = lane.take(loc.take) else {
        return false;
    };
    take.events
        .range(..=(ev.tick, EventId(u64::MAX)))
        .any(|(_, b)| {
            b.id != ev.id
                && matches!(b.body, EventBody::Note { duration, .. } if ev.tick < b.tick + duration)
                && matches(doc, by, lane.midi_dest, b, 0)
        })
}
```

In `crates/core-model/src/modify.rs`, Find:

```rust
    /// Modification.
    pub modify: Option<Modify>,
}
```

Replace:

```rust
    /// Modification.
    pub modify: Option<Modify>,
    /// Modify copies inserted beside the originals (a harmonizing copy).
    #[serde(default)]
    pub to_copy: bool,
}
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify 2>&1 | tail -5
```

Expected: PASS (all `select_modify` tests). If `take.events.range` does not accept `..=(Tick, EventId)`, use `take.events.iter().take_while(|(k, _)| k.0 <= ev.tick)`; the result is the same.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -40
git add crates/core-model/src/rule.rs crates/core-model/src/modify.rs crates/core-model/tests/select_modify.rs
git commit -m "feat(core): release-velocity, value, controller and bracketing rules; templates keep to-copy

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 631 Rust tests pass (628 + 3).

---
### Task 2: Diatonic, inverting and mapped transposes; value changes with shaped and oscillating amounts

**Files:**
- Modify: `crates/core-model/src/modify.rs`
- Modify: `crates/core-model/src/command/select.rs`
- Test: `crates/core-model/tests/select_modify.rs`

**Interfaces:**
- Consumes: `scale_step`, `shift_pitch`, `edit_events`, Task 1's types.
- Produces: `Transpose::Diatonic { steps: i8, root: u8, mask: u16 }`, `Transpose::Invert { axis: u8 }`; `Modify::NoteMap { map: Vec<(u8, u8)> }`; `Modify::Change { target: ValueTarget, op: ChangeOp, amount: Amount }`; `pub enum ValueTarget { Velocity, ReleaseVelocity, Duration, Value }`; `pub enum ChangeOp { Set, Scale, Add, Limit { min: i64, max: i64 }, Compress { low: i64, high: i64 }, Legato }`; `pub enum Curve { Straight, EaseIn, EaseOut, SCurve }`; `pub enum Wave { Sine, Square, Triangle, Saw }`; `pub enum Amount { Constant { value: i64 }, Line { from: i64, to: i64, curve: Curve }, Oscillator { wave: Wave, low: i64, high: i64, period: Tick, phase: Tick }, Random { low: i64, high: i64, seed: u64 } }`; `pub fn Amount::at(&self, t: Tick, first: Tick, last: Tick, id: EventId) -> i64`; `pub(crate) fn apply_modify(...) -> Result<Vec<EventId>>` (the events the selection holds afterwards).

Rulings 2, 3, 6. Names: `Change` is "Change Velocity", "Change Release Velocity", "Change Duration" or "Change Values" by target; `NoteMap` and the new transposes are "Transpose". `Scale` amounts are percent. Values are clamped like the old variants (velocities 0–65 535, durations ≥ 0, values 0–2³²−1). Diatonic validates the scale like `TransposeInScale` (root ≤ 11, non-empty mask, else `OutOfRange`); Invert's axis and NoteMap pitches must be ≤ 127; an oscillator period ≤ 0 and a limit with `min > max` are `OutOfRange`; Legato on another target is `Invalid`.

- [ ] **Step 1: Write the failing tests**

In `crates/core-model/tests/select_modify.rs`, Find:

```rust
use core_model::modify::{DurationOp, Modify, ModifyTemplate, Quantize, Transpose, ValueOp};
```

Replace:

```rust
use core_model::modify::{
    Amount, ChangeOp, Curve, DurationOp, Modify, ModifyTemplate, Quantize, Transpose, ValueOp,
    ValueTarget, Wave,
};
```

Append:

```rust
const C_MAJOR: u16 = 0b1010_1011_0101;
/// The notes of `doc()` in time order: 60, 64, 67, 72; velocities 10–40.
const NOTES: [u64; 4] = [0, 1, 2, 4];

fn try_sm(d: &Document, sel: &Selection, modify: Modify) -> core_model::Result<Applied> {
    apply_in(
        d,
        Context {
            selection: sel,
            filter: &ViewFilter::default(),
        },
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(modify),
            to_copy: false,
        },
    )
}

fn notes_after(modify: Modify) -> Document {
    try_sm(&doc(), &lane_sel(&NOTES), modify).unwrap().doc
}

fn pitches(d: &Document) -> Vec<u8> {
    NOTES
        .iter()
        .map(|i| match body(d, *i) {
            EventBody::Note { pitch, .. } => pitch,
            b => panic!("{b:?}"),
        })
        .collect()
}

fn velocities(d: &Document) -> Vec<u16> {
    NOTES
        .iter()
        .map(|i| match body(d, *i) {
            EventBody::Note { velocity, .. } => velocity,
            b => panic!("{b:?}"),
        })
        .collect()
}

fn change(target: ValueTarget, op: ChangeOp, amount: Amount) -> Modify {
    Modify::Change { target, op, amount }
}

fn velocity_change(op: ChangeOp, amount: Amount) -> Vec<u16> {
    velocities(&notes_after(change(ValueTarget::Velocity, op, amount)))
}

const fn constant(value: i64) -> Amount {
    Amount::Constant { value }
}

#[test]
fn diatonic_inversion_and_note_map() {
    let d = notes_after(Modify::Transpose(Transpose::Diatonic {
        steps: 2,
        root: 0,
        mask: C_MAJOR,
    }));
    assert_eq!(pitches(&d), vec![64, 67, 71, 76], "two degrees up in C major");
    let d = notes_after(Modify::Transpose(Transpose::Invert { axis: 64 }));
    assert_eq!(pitches(&d), vec![68, 64, 61, 56], "mirrored around E4");
    let d = notes_after(Modify::NoteMap {
        map: vec![(60, 36), (67, 38)],
    });
    assert_eq!(pitches(&d), vec![36, 64, 38, 72]);
    let sel = lane_sel(&NOTES);
    for bad in [
        Modify::Transpose(Transpose::Diatonic {
            steps: 1,
            root: 12,
            mask: C_MAJOR,
        }),
        Modify::Transpose(Transpose::Diatonic {
            steps: 1,
            root: 0,
            mask: 0,
        }),
        Modify::Transpose(Transpose::Invert { axis: 128 }),
        Modify::NoteMap {
            map: vec![(60, 200)],
        },
    ] {
        assert!(try_sm(&doc(), &sel, bad.clone()).is_err(), "{bad:?}");
    }
    assert_eq!(Modify::NoteMap { map: vec![] }.name(), "Transpose");
}

#[test]
fn constant_value_changes() {
    assert_eq!(velocity_change(ChangeOp::Add, constant(5)), vec![15, 25, 35, 45]);
    assert_eq!(velocity_change(ChangeOp::Scale, constant(50)), vec![5, 10, 15, 20]);
    assert_eq!(velocity_change(ChangeOp::Set, constant(70_000)), vec![65_535; 4]);
    assert_eq!(velocity_change(ChangeOp::Add, constant(-100)), vec![0; 4]);
    assert_eq!(
        velocity_change(ChangeOp::Limit { min: 15, max: 35 }, constant(0)),
        vec![15, 20, 30, 35]
    );
    assert_eq!(
        velocity_change(ChangeOp::Compress { low: 100, high: 400 }, constant(0)),
        vec![100, 200, 300, 400]
    );
}

#[test]
fn shaped_and_oscillating_amounts_follow_the_event_times() {
    // The notes start at 0, Q+10, 2Q and 4Q.
    let line = |curve| {
        velocity_change(
            ChangeOp::Set,
            Amount::Line {
                from: 0,
                to: 400,
                curve,
            },
        )
    };
    assert_eq!(line(Curve::Straight), vec![0, 100, 200, 400]);
    assert_eq!(line(Curve::EaseIn), vec![0, 25, 100, 400]);
    let osc = |wave, period| {
        velocity_change(
            ChangeOp::Set,
            Amount::Oscillator {
                wave,
                low: 10,
                high: 90,
                period: Tick(period),
                phase: Tick(0),
            },
        )
    };
    assert_eq!(osc(Wave::Square, 2 * Q), vec![90, 10, 90, 90]);
    assert_eq!(osc(Wave::Saw, 4 * Q), vec![10, 30, 50, 10]);
    let random = || {
        velocity_change(
            ChangeOp::Set,
            Amount::Random {
                low: 10,
                high: 20,
                seed: 7,
            },
        )
    };
    let a = random();
    assert_eq!(a, random(), "the same seed gives the same values");
    assert!(a.iter().all(|v| (10..=20).contains(v)), "{a:?}");
}

#[test]
fn release_velocity_duration_and_value_targets() {
    let d = notes_after(change(ValueTarget::ReleaseVelocity, ChangeOp::Set, constant(7)));
    for i in NOTES {
        assert!(matches!(body(&d, i), EventBody::Note { release_velocity: 7, .. }));
    }
    let d = notes_after(change(ValueTarget::Duration, ChangeOp::Add, constant(-Q / 4)));
    let durations: Vec<i64> = NOTES
        .iter()
        .map(|i| match body(&d, *i) {
            EventBody::Note { duration, .. } => duration.0,
            b => panic!("{b:?}"),
        })
        .collect();
    assert_eq!(durations, vec![Q / 4, Q / 4, 3 * Q / 4, 7 * Q / 4]);
    let d = notes_after(change(ValueTarget::Duration, ChangeOp::Legato, constant(0)));
    assert!(matches!(body(&d, 0), EventBody::Note { duration: Tick(t), .. } if t == Q + 10));
    // The CC (ev3) is the only value target.
    let d = try_sm(
        &doc(),
        &lane_sel(&[0, 3]),
        change(ValueTarget::Value, ChangeOp::Add, constant(10)),
    )
    .unwrap()
    .doc;
    assert_eq!(body(&d, 3), EventBody::Cc { index: 7, value: 110 });
    assert!(matches!(body(&d, 0), EventBody::Note { velocity: 10, .. }));
}

#[test]
fn bad_amounts_and_targets_are_refused() {
    let sel = lane_sel(&NOTES);
    for bad in [
        change(ValueTarget::Velocity, ChangeOp::Legato, constant(0)),
        change(
            ValueTarget::Velocity,
            ChangeOp::Limit { min: 5, max: 4 },
            constant(0),
        ),
        change(
            ValueTarget::Velocity,
            ChangeOp::Set,
            Amount::Oscillator {
                wave: Wave::Sine,
                low: 0,
                high: 1,
                period: Tick(0),
                phase: Tick(0),
            },
        ),
    ] {
        assert!(try_sm(&doc(), &sel, bad.clone()).is_err(), "{bad:?}");
    }
    assert_eq!(
        change(ValueTarget::Velocity, ChangeOp::Set, constant(0)).name(),
        "Change Velocity"
    );
    assert_eq!(
        change(ValueTarget::Value, ChangeOp::Set, constant(0)).name(),
        "Change Values"
    );
}
```

Check of the expected numbers: Line x for Q+10 is (Q+10)/4Q = 0.2502 → 100.07 → 100 (ease in 0.0626 → 25); Square at Q+10 is past half of 2Q → low; Saw over 4Q at Q+10 is 0.2502 of the way → 10 + 80 × 0.2502 = 30.0, at 2Q 50, at 4Q back to 10.

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify 2>&1 | tail -15
```

Expected: compile errors (`no variant named Diatonic`, `cannot find type ValueTarget`).

- [ ] **Step 3: Implement**

In `crates/core-model/src/modify.rs`, Find:

```rust
    /// Change CC, pitch-bend and pressure values (clamped to u32).
    ControllerValue(ValueOp),
}
```

Replace:

```rust
    /// Change CC, pitch-bend and pressure values (clamped to u32).
    ControllerValue(ValueOp),
    /// Map pitches: a note or poly pressure whose pitch is a `from` takes
    /// its `to`.
    NoteMap {
        /// `(from, to)` pairs, each 0–127.
        map: Vec<(u8, u8)>,
    },
    /// Change one value of each fitting event by `op` with `amount`.
    Change {
        /// Which value.
        target: ValueTarget,
        /// How the amount combines with it.
        op: ChangeOp,
        /// The amount (percent for `Scale`).
        amount: Amount,
    },
}

/// Which value [`Modify::Change`] edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueTarget {
    /// Note velocity (16-bit).
    Velocity,
    /// Note release velocity (16-bit).
    ReleaseVelocity,
    /// Note length in ticks.
    Duration,
    /// CC, pitch-bend and pressure values (32-bit).
    Value,
}

/// How [`Modify::Change`] combines its amount with each value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeOp {
    /// The amount.
    Set,
    /// The value times the amount, in percent.
    Scale,
    /// The value plus the amount.
    Add,
    /// The value kept inside `min..=max`.
    Limit {
        /// Lowest kept.
        min: i64,
        /// Highest kept.
        max: i64,
    },
    /// The lowest changed value becomes `low`, the highest `high`, the rest
    /// in proportion.
    Compress {
        /// New lowest.
        low: i64,
        /// New highest.
        high: i64,
    },
    /// Notes end where the next changed note of their lane starts
    /// (durations only).
    Legato,
}

/// The shape of a [`Amount::Line`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    /// Straight.
    Straight,
    /// Slow, then fast.
    EaseIn,
    /// Fast, then slow.
    EaseOut,
    /// Slow at both ends.
    SCurve,
}

/// The wave of an [`Amount::Oscillator`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wave {
    /// Sine, starting in the middle and rising.
    Sine,
    /// High for the first half period, low for the second.
    Square,
    /// From low up to high at half a period, back down.
    Triangle,
    /// From low rising to high, then jumping back.
    Saw,
}

/// An amount that may change with each event's position (Plan 4c Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Amount {
    /// The same for every event.
    Constant {
        /// The amount.
        value: i64,
    },
    /// From `from` at the first changed event to `to` at the last.
    Line {
        /// At the first event.
        from: i64,
        /// At the last event.
        to: i64,
        /// Shape.
        curve: Curve,
    },
    /// A wave between `low` and `high`, measured from the first event.
    Oscillator {
        /// Wave.
        wave: Wave,
        /// Lowest.
        low: i64,
        /// Highest.
        high: i64,
        /// Period (> 0).
        period: Tick,
        /// Shift of the wave.
        phase: Tick,
    },
    /// A value in `low..=high` per event, fixed by `seed` and the event.
    Random {
        /// Lowest.
        low: i64,
        /// Highest.
        high: i64,
        /// Seed.
        seed: u64,
    },
}

fn lerp(a: i64, b: i64, y: f64) -> i64 {
    (a as f64 + (b as f64 - a as f64) * y).round() as i64
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Amount {
    /// The amount for event `id` at `t` when the changed events start
    /// between `first` and `last`.
    pub fn at(&self, t: Tick, first: Tick, last: Tick, id: EventId) -> i64 {
        match *self {
            Amount::Constant { value } => value,
            Amount::Line { from, to, curve } => {
                let span = (last - first).0;
                let x = if span > 0 {
                    (t - first).0 as f64 / span as f64
                } else {
                    0.0
                };
                let y = match curve {
                    Curve::Straight => x,
                    Curve::EaseIn => x * x,
                    Curve::EaseOut => 1.0 - (1.0 - x) * (1.0 - x),
                    Curve::SCurve => x * x * (3.0 - 2.0 * x),
                };
                lerp(from, to, y)
            }
            Amount::Oscillator {
                wave,
                low,
                high,
                period,
                phase,
            } => {
                let p = period.0.max(1);
                let ph = (t - first).0.saturating_add(phase.0).rem_euclid(p) as f64 / p as f64;
                let w = match wave {
                    Wave::Sine => (std::f64::consts::TAU * ph).sin(),
                    Wave::Square => {
                        if ph < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    Wave::Triangle => 1.0 - 4.0 * (ph - 0.5).abs(),
                    Wave::Saw => 2.0 * ph - 1.0,
                };
                lerp(low, high, (w + 1.0) / 2.0)
            }
            Amount::Random { low, high, seed } => {
                let (lo, hi) = if low <= high { (low, high) } else { (high, low) };
                let span = i128::from(hi) - i128::from(lo) + 1;
                let h = i128::from(splitmix(seed ^ splitmix(id.0)));
                i64::try_from(i128::from(lo) + h % span).unwrap_or(hi)
            }
        }
    }
}
```

Note: the Saw test expects 10 at 4Q (phase wraps to 0 → w = −1 → low). The Square test expects `high` at phase 0: `ph < 0.5` → 1.0 → high.

Find:

```rust
/// Pitch changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Transpose {
```

Replace (keep the existing two variants; add two after them):

```rust
/// Pitch changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Transpose {
    /// By `steps` degrees of a scale ([`scale_step`]).
    Diatonic {
        /// Degrees (negative: down).
        steps: i8,
        /// Root pitch class 0–11.
        root: u8,
        /// 12-bit scale mask.
        mask: u16,
    },
    /// Mirrored around `axis` (clamped at 0 and 127).
    Invert {
        /// The pitch that stays.
        axis: u8,
    },
```

Then, still in `Transpose`, the existing `Interval` and `ToScale` variants follow unchanged (serde's externally tagged names do not depend on order).

Find:

```rust
            Modify::ControllerValue(_) => "Change Controller Values",
        }
    }
}
```

Replace:

```rust
            Modify::ControllerValue(_) => "Change Controller Values",
            Modify::NoteMap { .. } => "Transpose",
            Modify::Change { target, .. } => match target {
                ValueTarget::Velocity => "Change Velocity",
                ValueTarget::ReleaseVelocity => "Change Release Velocity",
                ValueTarget::Duration => "Change Duration",
                ValueTarget::Value => "Change Values",
            },
        }
    }
}
```

Find:

```rust
        Modify::Transpose(t) => match &mut e.body {
            EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                *pitch = match t {
                    Transpose::Interval { semitones } => shift_pitch(*pitch, i32::from(*semitones)),
                    Transpose::ToScale { root, mask } => snap_to_scale(*pitch, *root, *mask),
                }
            }
            _ => {}
        },
```

Replace:

```rust
        Modify::Transpose(t) => match &mut e.body {
            EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } => {
                *pitch = match t {
                    Transpose::Interval { semitones } => shift_pitch(*pitch, i32::from(*semitones)),
                    Transpose::ToScale { root, mask } => snap_to_scale(*pitch, *root, *mask),
                    Transpose::Diatonic { steps, root, mask } => {
                        if *root > 11 || mask & 0x0fff == 0 {
                            return Err(ModelError::range(
                                "transpose.scale",
                                format!("{root}/{mask:#x}"),
                            ));
                        }
                        scale_step(*pitch, i32::from(*steps), *root, *mask)
                    }
                    Transpose::Invert { axis } => {
                        if *axis > 127 {
                            return Err(ModelError::range("transpose.axis", axis));
                        }
                        shift_pitch(0, 2 * i32::from(*axis) - i32::from(*pitch))
                    }
                }
            }
            _ => {}
        },
        Modify::NoteMap { map } => {
            if map.iter().any(|(a, b)| *a > 127 || *b > 127) {
                return Err(ModelError::range("note_map", "a pitch above 127"));
            }
            if let EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } =
                &mut e.body
            {
                if let Some((_, to)) = map.iter().find(|(from, _)| *from == *pitch) {
                    *pitch = *to;
                }
            }
        }
        // Applied to the whole set in `apply_modify`.
        Modify::Change { .. } => {}
```

Find the whole function `pub(crate) fn apply_modify(` to the end of the file and Replace it with:

```rust
/// Notes end where the next of `ids` in their lane starts.
fn legato(d: &mut Document, ids: &[EventId], r: &mut Report) -> Result<()> {
    let mut by_lane: BTreeMap<_, Vec<(Tick, EventId)>> = BTreeMap::new();
    for id in ids {
        let (ev, loc) = d.event(*id)?;
        if let EventBody::Note { .. } = ev.body {
            by_lane
                .entry((loc.seq, loc.lane))
                .or_default()
                .push((ev.tick, *id));
        }
    }
    for notes in by_lane.values_mut() {
        notes.sort();
        for pair in notes.windows(2) {
            if let [(t0, id), (t1, _)] = pair {
                if t1 > t0 {
                    let len = *t1 - *t0;
                    edit_events(d, &[*id], r, |e| {
                        if let EventBody::Note { duration, .. } = &mut e.body {
                            *duration = len;
                        }
                        Ok(())
                    })?;
                }
            }
        }
    }
    Ok(())
}

fn read_value(e: &Event, t: ValueTarget) -> Option<i64> {
    match (t, &e.body) {
        (ValueTarget::Velocity, EventBody::Note { velocity, .. }) => Some(i64::from(*velocity)),
        (ValueTarget::ReleaseVelocity, EventBody::Note {
            release_velocity, ..
        }) => Some(i64::from(*release_velocity)),
        (ValueTarget::Duration, EventBody::Note { duration, .. }) => Some(duration.0),
        (
            ValueTarget::Value,
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. },
        ) => Some(i64::from(*value)),
        _ => None,
    }
}

fn write_value(e: &mut Event, t: ValueTarget, v: i64) {
    let v16 = u16::try_from(v.clamp(0, i64::from(u16::MAX))).unwrap_or(u16::MAX);
    let v32 = u32::try_from(v.clamp(0, i64::from(u32::MAX))).unwrap_or(u32::MAX);
    match (t, &mut e.body) {
        (ValueTarget::Velocity, EventBody::Note { velocity, .. }) => *velocity = v16,
        (ValueTarget::ReleaseVelocity, EventBody::Note {
            release_velocity, ..
        }) => *release_velocity = v16,
        (ValueTarget::Duration, EventBody::Note { duration, .. }) => *duration = Tick(v.max(0)),
        (
            ValueTarget::Value,
            EventBody::Cc { value, .. }
            | EventBody::PitchBend { value }
            | EventBody::ChannelPressure { value }
            | EventBody::PolyPressure { value, .. },
        ) => *value = v32,
        _ => {}
    }
}

fn narrow(v: i128) -> i64 {
    i64::try_from(v).unwrap_or(if v < 0 { i64::MIN } else { i64::MAX })
}

fn apply_change(
    d: &mut Document,
    ids: &[EventId],
    target: ValueTarget,
    op: ChangeOp,
    amount: &Amount,
    r: &mut Report,
) -> Result<()> {
    if let ChangeOp::Legato = op {
        if target != ValueTarget::Duration {
            return Err(ModelError::invalid("legato changes note lengths only"));
        }
        return legato(d, ids, r);
    }
    if let Amount::Oscillator { period, .. } = amount {
        if period.0 <= 0 {
            return Err(ModelError::range("amount.period", period.0));
        }
    }
    if let ChangeOp::Limit { min, max } = op {
        if min > max {
            return Err(ModelError::range("limit", format!("{min}..{max}")));
        }
    }
    let mut found: Vec<(EventId, Tick, i64)> = Vec::new();
    for id in ids.iter().copied().collect::<std::collections::BTreeSet<_>>() {
        let (ev, _) = d.event(id)?;
        if let Some(v) = read_value(ev, target) {
            found.push((id, ev.tick, v));
        }
    }
    let (Some(first), Some(last)) = (
        found.iter().map(|f| f.1).min(),
        found.iter().map(|f| f.1).max(),
    ) else {
        return Ok(());
    };
    let lo = found.iter().map(|f| f.2).min().unwrap_or(0);
    let hi = found.iter().map(|f| f.2).max().unwrap_or(0);
    for (id, tick, v) in found {
        let a = amount.at(tick, first, last, id);
        let next = match op {
            ChangeOp::Set => a,
            ChangeOp::Scale => narrow(i128::from(v) * i128::from(a) / 100),
            ChangeOp::Add => v.saturating_add(a),
            ChangeOp::Limit { min, max } => v.clamp(min, max),
            ChangeOp::Compress { low, high } => {
                if hi == lo {
                    low
                } else {
                    narrow(
                        i128::from(low)
                            + (i128::from(v) - i128::from(lo)) * (i128::from(high) - i128::from(low))
                                / (i128::from(hi) - i128::from(lo)),
                    )
                }
            }
            ChangeOp::Legato => v,
        };
        edit_events(d, &[id], r, |e| {
            write_value(e, target, next);
            Ok(())
        })?;
    }
    Ok(())
}

/// Applies `m` to the events `ids`; returns the events the selection holds
/// afterwards.
pub(crate) fn apply_modify(
    d: &mut Document,
    ids: &[EventId],
    m: &Modify,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    match m {
        Modify::Legato => legato(d, ids, r)?,
        Modify::Change {
            target,
            op,
            amount,
        } => apply_change(d, ids, *target, *op, amount, r)?,
        _ => edit_events(d, ids, r, |e| modify_one(e, m))?,
    }
    Ok(ids.to_vec())
}
```

`modify.rs` already imports `EventId` (used by `Amount::at`); in `modify_one`, the existing `Modify::Legato => {}` arm stays.

In `crates/core-model/src/command/select.rs`, Find:

```rust
        apply_modify(d, &targets, m, r)?;
    }
```

Replace:

```rust
        targets = apply_modify(d, &targets, m, r)?;
    }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -40
git add crates/core-model/src/modify.rs crates/core-model/src/command/select.rs crates/core-model/tests/select_modify.rs
git commit -m "feat(core): diatonic, inverted and mapped transposes; value changes with shaped amounts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 636 Rust tests pass (631 + 5).

---
### Task 3: Density, trim start, substitute and delete; `EventCmd::Replace`

**Files:**
- Modify: `crates/core-model/src/modify.rs`
- Modify: `crates/core-model/src/command/event.rs`
- Test: `crates/core-model/tests/select_modify.rs`
- Test: `crates/core-model/tests/cmd_event.rs`

**Interfaces:**
- Consumes: Task 2's `apply_modify -> Result<Vec<EventId>>`; `Document::{remove_event, insert_event, replace_event, counters_mut}`; `check_event`, `ensure_drum_row`.
- Produces: `Modify::TrimStart { delta: Tick }` ("Trim Start"), `Modify::Substitute(Substitute)` ("Substitute"), `Modify::Density(Density)` ("Thin Out" / "Thicken"), `Modify::Delete` ("Delete Events"); `pub enum Substitute { Controller { index: u8 }, PitchBend, ChannelPressure, Pitch { pitch: u8 } }`; `pub enum Density { Thin { spacing: Tick }, Thicken { spacing: Tick } }`; `EventCmd::Replace { event: EventId, spec: EventSpec }` ("Edit Event").

Ruling 5, 17. The selection after the command holds the targets that still exist plus the events Thicken created. `Replace` keeps the event's ID and lane; it refuses a kind change (`Invalid`) and validates like an insert (`check_event`).

- [ ] **Step 1: Write the failing tests**

In `crates/core-model/tests/select_modify.rs`, Find:

```rust
use core_model::modify::{
    Amount, ChangeOp, Curve, DurationOp, Modify, ModifyTemplate, Quantize, Transpose, ValueOp,
    ValueTarget, Wave,
};
```

Replace:

```rust
use core_model::history::History;
use core_model::modify::{
    Amount, ChangeOp, Curve, Density, DurationOp, Modify, ModifyTemplate, Quantize, Substitute,
    Transpose, ValueOp, ValueTarget, Wave,
};
```

Append:

```rust
/// seq0/lane0 with CC7 at ticks `at` (value = tick) and nothing else.
fn controllers(at: &[i64]) -> Document {
    let d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let events = at
        .iter()
        .map(|t| {
            EventSpec::new(
                Tick(*t),
                EventBody::Cc {
                    index: 7,
                    value: *t as u32,
                },
            )
        })
        .collect();
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events,
        }),
    )
    .unwrap()
    .doc
}

fn cc_ticks(d: &Document) -> Vec<(i64, u32)> {
    d.lane(SeqId(0), LaneId(0))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Cc { value, .. } => Some((e.tick.0, value)),
            _ => None,
        })
        .collect()
}

#[test]
fn thin_out_keeps_events_at_least_the_spacing_apart() {
    let d = controllers(&[0, 10, 20, 100, 200]);
    let a = try_sm(
        &d,
        &lane_sel(&[0, 1, 2, 3, 4]),
        Modify::Density(Density::Thin { spacing: Tick(50) }),
    )
    .unwrap();
    assert_eq!(cc_ticks(&a.doc), vec![(0, 0), (100, 100), (200, 200)]);
    assert_eq!(ids(a.selection.as_ref().unwrap()), vec![0, 3, 4]);
}

#[test]
fn thicken_fills_gaps_on_a_straight_line() {
    let d = controllers(&[0, 100]);
    let a = try_sm(
        &d,
        &lane_sel(&[0, 1]),
        Modify::Density(Density::Thicken { spacing: Tick(30) }),
    )
    .unwrap();
    assert_eq!(
        cc_ticks(&a.doc),
        vec![(0, 0), (30, 30), (60, 60), (90, 90), (100, 100)]
    );
    assert_eq!(a.selection.unwrap().events.len(), 5);
    assert!(
        try_sm(
            &d,
            &lane_sel(&[0, 1]),
            Modify::Density(Density::Thin { spacing: Tick(0) })
        )
        .is_err()
    );
}

#[test]
fn trim_start_keeps_the_end() {
    // ev2: 67 at 2Q, one quarter long.
    let d = try_sm(&doc(), &lane_sel(&[2]), Modify::TrimStart { delta: Tick(Q / 4) })
        .unwrap()
        .doc;
    let (ev, _) = d.event(EventId(2)).unwrap();
    assert_eq!(ev.tick, Tick(2 * Q + Q / 4));
    assert!(matches!(ev.body, EventBody::Note { duration: Tick(t), .. } if t == 3 * Q / 4));
    let d = try_sm(&doc(), &lane_sel(&[2]), Modify::TrimStart { delta: Tick(2 * Q) })
        .unwrap()
        .doc;
    let (ev, _) = d.event(EventId(2)).unwrap();
    assert_eq!(ev.tick, Tick(3 * Q - 1), "one tick is left");
}

#[test]
fn substitute_changes_kind_or_pitch() {
    let sub = |s: Substitute, ids: &[u64]| try_sm(&doc(), &lane_sel(ids), Modify::Substitute(s));
    let d = sub(Substitute::Controller { index: 11 }, &[3]).unwrap().doc;
    assert_eq!(body(&d, 3), EventBody::Cc { index: 11, value: 100 });
    let d = sub(Substitute::PitchBend, &[3]).unwrap().doc;
    assert_eq!(body(&d, 3), EventBody::PitchBend { value: 100 });
    let d = sub(Substitute::Pitch { pitch: 36 }, &NOTES).unwrap().doc;
    assert_eq!(pitches(&d), vec![36; 4]);
    assert!(sub(Substitute::Controller { index: 200 }, &[3]).is_err());
}

#[test]
fn delete_removes_the_selection_and_undoes() {
    let mut h = History::new(doc());
    h.set_selection(lane_sel(&[0, 1]));
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::Delete),
            to_copy: false,
        },
        &ViewFilter::default(),
    )
    .unwrap();
    assert!(h.doc().event(EventId(0)).is_err());
    assert!(h.selection().events.is_empty());
    assert_eq!(h.undo_label(), Some("Delete Events"));
    h.undo().unwrap();
    assert!(h.doc().event(EventId(0)).is_ok());
}

#[test]
fn a_modify_that_changes_nothing_adds_no_undo_step() {
    let mut h = History::new(doc());
    // No events selected, no lanes: nothing to change.
    h.execute(
        &Command::SelectAndModify {
            rule: None,
            mode: SelectMode::Select,
            modify: Some(Modify::Delete),
            to_copy: false,
        },
        &ViewFilter::default(),
    )
    .unwrap();
    assert_eq!(h.undo_label(), None);
    assert_eq!(Modify::Density(Density::Thin { spacing: Tick(1) }).name(), "Thin Out");
    assert_eq!(Modify::TrimStart { delta: Tick(1) }.name(), "Trim Start");
}
```

Append to `crates/core-model/tests/cmd_event.rs`:

```rust
#[test]
fn replace_swaps_an_event_keeping_its_id_and_kind() {
    let d = insert(&two_seqs(), 0, 0, vec![note(0, 60)]).unwrap();
    let mut spec = EventSpec::new(Tick(PPQ), EventBody::note(62, 7, Tick(10)));
    spec.mute = true;
    let d2 = run(
        &d,
        ev(EventCmd::Replace {
            event: EventId(0),
            spec: spec.clone(),
        }),
    )
    .unwrap();
    assert_eq!(d2.event(EventId(0)).unwrap().0.spec(), spec);
    assert_eq!(
        ev(EventCmd::Replace {
            event: EventId(0),
            spec: spec.clone()
        })
        .name(),
        "Edit Event"
    );
    let cc = EventSpec::new(Tick(0), EventBody::Cc { index: 1, value: 2 });
    assert!(matches!(
        run(
            &d,
            ev(EventCmd::Replace {
                event: EventId(0),
                spec: cc
            })
        ),
        Err(ModelError::Invalid { .. })
    ));
    let bad = EventSpec::new(Tick(0), EventBody::note(200, 7, Tick(10)));
    assert!(matches!(
        run(
            &d,
            ev(EventCmd::Replace {
                event: EventId(0),
                spec: bad
            })
        ),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(
        run(
            &d,
            ev(EventCmd::Replace {
                event: EventId(9),
                spec
            })
        )
        .is_err()
    );
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test select_modify --test cmd_event 2>&1 | tail -15
```

Expected: compile errors (`no variant named Density`, `no variant named Replace`).

- [ ] **Step 3: Implement the modifications**

In `crates/core-model/src/modify.rs`, Find:

```rust
    /// Change one value of each fitting event by `op` with `amount`.
    Change {
```

Replace:

```rust
    /// Move note starts by `delta`, keeping their ends (at least 1 tick).
    TrimStart {
        /// Offset of the start.
        delta: Tick,
    },
    /// Turn controller kinds into another, or notes into one pitch.
    Substitute(Substitute),
    /// Thin out or thicken controller series.
    Density(Density),
    /// Delete the events.
    Delete,
    /// Change one value of each fitting event by `op` with `amount`.
    Change {
```

Find:

```rust
/// Which value [`Modify::Change`] edits.
```

Replace:

```rust
/// What [`Modify::Substitute`] turns events into (Plan 4c Ruling 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Substitute {
    /// CC, pitch bend and channel pressure become CC `index` (value kept).
    Controller {
        /// Controller number 0–127.
        index: u8,
    },
    /// CC, pitch bend and channel pressure become pitch bend.
    PitchBend,
    /// CC, pitch bend and channel pressure become channel pressure.
    ChannelPressure,
    /// Notes and poly pressure take `pitch`.
    Pitch {
        /// Pitch 0–127.
        pitch: u8,
    },
}

/// Controller-series density changes (Plan 4c Ruling 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Density {
    /// Remove events closer than `spacing` to the last kept one.
    Thin {
        /// Minimum distance (> 0).
        spacing: Tick,
    },
    /// Add straight-line values every `spacing` between neighbours.
    Thicken {
        /// Distance between added events (> 0).
        spacing: Tick,
    },
}

/// The most events one Thicken may add.
const MAX_THICKEN: usize = 100_000;

/// Which value [`Modify::Change`] edits.
```

In `Modify::name`, Find:

```rust
            Modify::NoteMap { .. } => "Transpose",
```

Replace:

```rust
            Modify::NoteMap { .. } => "Transpose",
            Modify::TrimStart { .. } => "Trim Start",
            Modify::Substitute(_) => "Substitute",
            Modify::Density(Density::Thin { .. }) => "Thin Out",
            Modify::Density(Density::Thicken { .. }) => "Thicken",
            Modify::Delete => "Delete Events",
```

In `modify_one`, Find:

```rust
        // Applied to the whole set in `apply_modify`.
        Modify::Change { .. } => {}
```

Replace:

```rust
        Modify::TrimStart { delta } => {
            if let EventBody::Note { duration, .. } = &mut e.body {
                if duration.0 > 0 {
                    let end = e.tick + *duration;
                    let start = (e.tick + *delta).min(end - Tick(1));
                    *duration = end - start;
                    e.tick = start;
                }
            }
        }
        Modify::Substitute(s) => {
            let value = match e.body {
                EventBody::Cc { value, .. }
                | EventBody::PitchBend { value }
                | EventBody::ChannelPressure { value } => Some(value),
                _ => None,
            };
            match (s, value) {
                (Substitute::Controller { index }, Some(value)) => {
                    e.body = EventBody::Cc {
                        index: *index,
                        value,
                    }
                }
                (Substitute::PitchBend, Some(value)) => e.body = EventBody::PitchBend { value },
                (Substitute::ChannelPressure, Some(value)) => {
                    e.body = EventBody::ChannelPressure { value }
                }
                (Substitute::Pitch { pitch: to }, _) => {
                    if let EventBody::Note { pitch, .. } | EventBody::PolyPressure { pitch, .. } =
                        &mut e.body
                    {
                        *pitch = *to;
                    }
                }
                _ => {}
            }
        }
        // Applied to the whole set in `apply_modify`.
        Modify::Change { .. } | Modify::Density(_) | Modify::Delete => {}
```

(`edit_events` re-validates each event, so a controller number or pitch above 127 is refused as `OutOfRange`.)

Find:

```rust
/// Applies `m` to the events `ids`; returns the events the selection holds
/// afterwards.
```

Replace:

```rust
/// The series a controller event belongs to inside its lane and take.
fn series(e: &Event) -> Option<(u8, u8)> {
    match e.body {
        EventBody::Cc { index, .. } => Some((0, index)),
        EventBody::PitchBend { .. } => Some((1, 0)),
        EventBody::ChannelPressure { .. } => Some((2, 0)),
        EventBody::PolyPressure { pitch, .. } => Some((3, pitch)),
        _ => None,
    }
}

fn series_value(b: &EventBody) -> u32 {
    match b {
        EventBody::Cc { value, .. }
        | EventBody::PitchBend { value }
        | EventBody::ChannelPressure { value }
        | EventBody::PolyPressure { value, .. } => *value,
        _ => 0,
    }
}

fn with_value(b: &mut EventBody, v: u32) {
    if let EventBody::Cc { value, .. }
    | EventBody::PitchBend { value }
    | EventBody::ChannelPressure { value }
    | EventBody::PolyPressure { value, .. } = b
    {
        *value = v;
    }
}

fn density(
    d: &mut Document,
    ids: &[EventId],
    den: Density,
    r: &mut Report,
) -> Result<Vec<EventId>> {
    let spacing = match den {
        Density::Thin { spacing } | Density::Thicken { spacing } => spacing,
    };
    if spacing.0 <= 0 {
        return Err(ModelError::range("density.spacing", spacing.0));
    }
    let mut groups: BTreeMap<_, Vec<(Tick, EventId)>> = BTreeMap::new();
    let mut out = Vec::new();
    for id in ids.iter().copied().collect::<std::collections::BTreeSet<_>>() {
        let (ev, loc) = d.event(id)?;
        match series(ev) {
            Some((a, b)) => groups
                .entry((loc.seq, loc.lane, loc.take, a, b))
                .or_default()
                .push((ev.tick, id)),
            None => out.push(id),
        }
    }
    let mut added = 0usize;
    for ((seq, lane, take, _, _), mut evs) in groups {
        evs.sort();
        match den {
            Density::Thin { .. } => {
                let mut last: Option<Tick> = None;
                for (t, id) in evs {
                    if last.is_some_and(|l| (t - l).0 < spacing.0) {
                        d.remove_event(id)?;
                        r.removed.push(AnyId::Event(id));
                    } else {
                        last = Some(t);
                        out.push(id);
                    }
                }
            }
            Density::Thicken { .. } => {
                for pair in evs.windows(2) {
                    let [(t0, a), (t1, b)] = pair else { continue };
                    let first = d.event(*a)?.0.clone();
                    let va = i128::from(series_value(&first.body));
                    let vb = i128::from(series_value(&d.event(*b)?.0.body));
                    let gap = i128::from((*t1 - *t0).0);
                    let mut t = t0.0.saturating_add(spacing.0);
                    while t < t1.0 {
                        added += 1;
                        if added > MAX_THICKEN {
                            return Err(ModelError::range("density.spacing", spacing.0));
                        }
                        let v = va + (vb - va) * i128::from(t - t0.0) / gap;
                        let mut spec = first.spec();
                        spec.tick = Tick(t);
                        with_value(&mut spec.body, u32::try_from(v).unwrap_or(0));
                        let id = d.counters_mut().alloc_event();
                        d.insert_event(seq, lane, Some(take), spec.with_id(id))?;
                        r.created.push(AnyId::Event(id));
                        out.push(id);
                        t = t.saturating_add(spacing.0);
                    }
                }
                out.extend(evs.iter().map(|(_, id)| *id));
            }
        }
    }
    Ok(out)
}

/// Applies `m` to the events `ids`; returns the events the selection holds
/// afterwards.
```

Find (in `apply_modify`):

```rust
        } => apply_change(d, ids, *target, *op, amount, r)?,
        _ => edit_events(d, ids, r, |e| modify_one(e, m))?,
    }
    Ok(ids.to_vec())
```

Replace:

```rust
        } => apply_change(d, ids, *target, *op, amount, r)?,
        Modify::Density(den) => return density(d, ids, *den, r),
        Modify::Delete => {
            for id in ids.iter().copied().collect::<std::collections::BTreeSet<_>>() {
                d.remove_event(id)?;
                r.removed.push(AnyId::Event(id));
            }
            return Ok(Vec::new());
        }
        _ => edit_events(d, ids, r, |e| modify_one(e, m))?,
    }
    Ok(ids.to_vec())
```

At the top of `modify.rs`, Find:

```rust
use crate::command::Report;
```

Replace:

```rust
use crate::command::Report;
use crate::ids::AnyId;
```

- [ ] **Step 4: Implement `Replace`**

In `crates/core-model/src/command/event.rs`, Find:

```rust
        /// `(tick, value)` pairs.
        points: Vec<(Tick, u32)>,
    },
}
```

Replace:

```rust
        /// `(tick, value)` pairs.
        points: Vec<(Tick, u32)>,
    },
    /// Replace an event's position, routing, mute and body, keeping its ID
    /// and lane; the kind cannot change (plan 4c: one list edit).
    Replace {
        /// The event.
        event: EventId,
        /// What it becomes.
        spec: EventSpec,
    },
}
```

Find:

```rust
            EventCmd::SetPerNote { .. } => "Set Per-Note Values",
```

Replace:

```rust
            EventCmd::SetPerNote { .. } => "Set Per-Note Values",
            EventCmd::Replace { .. } => "Edit Event",
```

In `apply`, add a new arm before the closing of the `match cmd` (next to the `SetPitch` arm is fine). Find:

```rust
        EventCmd::SetPitch { events, pitch } => edit_events(d, events, r, |e| {
```

Replace:

```rust
        EventCmd::Replace { event, spec } => {
            let (kind, loc) = {
                let (e, l) = d.event(*event)?;
                (e.body.kind(), l)
            };
            if kind != spec.body.kind() {
                return Err(ModelError::invalid("an edit cannot change an event's kind"));
            }
            check_event(d, loc.seq, spec)?;
            d.replace_event(spec.clone().with_id(*event))?;
            ensure_drum_row(d, *event, r)?;
        }
        EventCmd::SetPitch { events, pitch } => edit_events(d, events, r, |e| {
```

If `EventSpec`, `ModelError` or `check_event` are not yet imported in `event.rs`, add them to its `use` lines (`use crate::event::EventSpec;`, `use crate::error::ModelError;`, `use crate::rules::check_event;`).

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model 2>&1 | grep -E "^test result|FAILED|panicked" | tail -30
```

Expected: PASS everywhere (the proptest property included: it builds its own commands).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -40
git add crates/core-model/src/modify.rs crates/core-model/src/command/event.rs crates/core-model/tests/select_modify.rs crates/core-model/tests/cmd_event.rs
git commit -m "feat(core): density, trim start, substitute and delete modifications; Replace event command

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 643 Rust tests pass (636 + 7).

---
### Task 4: FFI select-and-modify records, Modify Again, templates and `EventEdit::Replace`

**Files:**
- Create: `crates/core-ffi/src/modify.rs`
- Modify: `crates/core-ffi/src/edit.rs`
- Modify: `crates/core-ffi/src/session.rs`
- Modify: `crates/core-ffi/src/lib.rs`
- Test: `crates/core-ffi/tests/modify.rs` (new)
- Test: `crates/core-ffi/tests/edit.rs`

**Interfaces:**
- Consumes: Tasks 1–3's core types; `events::{bad, kind_of, spec}`; `History::{modify_again, last_modify}`; `Session::execute_with`.
- Produces (UniFFI, re-exported from the crate root):
  - `enum ConditionTest { TimeRange { start: i64, end: i64 }, Kinds { kinds: Vec<EventKindRow> }, Pitch { low: u8, high: u8 }, Velocity { low: u16, high: u16 }, ReleaseVelocity { low: u16, high: u16 }, Duration { min: i64, max: i64 }, Value { low: u32, high: u32 }, Controller { low: u8, high: u8 }, Dest { dest: Option<u64> }, Metric { grid: i64, offset: i64, tolerance: i64 }, EveryNth { n: u32, phase: u32 }, BracketedBy { low: u8, high: u8 } }`
  - `record ConditionRow { test: ConditionTest, negate: bool }`, `record RuleRow { conditions: Vec<ConditionRow>, match_all: bool }`, `enum SelectModeRow { Select, Add, Refine }`
  - `enum ValueTargetRow { Velocity, ReleaseVelocity, Duration, Value }`, `enum CurveRow { Straight, EaseIn, EaseOut, SCurve }`, `enum WaveRow { Sine, Square, Triangle, Saw }`
  - `enum AmountRow { Constant { value: i64 }, Line { from: i64, to: i64, curve: CurveRow }, Oscillator { wave: WaveRow, low: i64, high: i64, period: i64, phase: i64 }, Random { low: i64, high: i64, seed: u64 } }`
  - `enum ChangeOpRow { Set, Scale, Add, Limit { min: i64, max: i64 }, Compress { low: i64, high: i64 }, Legato }`
  - `record NoteMapEntry { from: u8, to: u8 }`, `enum SubstituteRow { Controller { index: u8 }, PitchBend, ChannelPressure, Pitch { pitch: u8 } }`
  - `enum ModifyRow { TransposeInterval { semitones: i8 }, TransposeDiatonic { steps: i8, root: u8, mask: u16 }, TransposeToScale { root: u8, mask: u16 }, TransposeInvert { axis: u8 }, NoteMap { map: Vec<NoteMapEntry> }, Quantize { grid: i64, strength: u8, offsets: Vec<i64> }, Change { target: ValueTargetRow, op: ChangeOpRow, amount: AmountRow }, Move { delta: i64 }, ScaleTime { num: u32, den: u32, anchor: i64 }, TrimStart { delta: i64 }, SetDest { dest: Option<u64> }, Substitute { to: SubstituteRow }, Thin { spacing: i64 }, Thicken { spacing: i64 }, Delete }`
  - `record ModifyTemplateRow { name: String, rule: Option<RuleRow>, mode: SelectModeRow, modify: Option<ModifyRow>, to_copy: bool }`, `enum SettingsEdit { SaveTemplate { template: ModifyTemplateRow }, DeleteTemplate { name: String } }`
  - `EditCommand::SelectAndModify { rule: Option<RuleRow>, mode: SelectModeRow, modify: Option<ModifyRow>, to_copy: bool }`, `EditCommand::ModifyAgain`, `EditCommand::Settings { edit: SettingsEdit }` (appended); `EventEdit::Replace { event: u64, row: EventRow }` (appended).
  - `Session.modify_templates() -> Vec<ModifyTemplateRow>` (by name), `Session.last_modify() -> Option<ModifyRow>`.
  - Rust helpers `vizdaw_ffi::modify::{rule, rule_row, modify, modify_row, template, template_row, mode_of}`.

Rulings 2, 4, 8, 9, 25. An empty template name is a `Model` error. Quantize with no offsets is a grid quantize, with offsets a groove. Templates whose rule is not a flat list (none can be made through the FFI) are left out of `modify_templates`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/modify.rs`:

```rust
//! Select-and-modify through the FFI (plan 4c): rules and modifications
//! round-trip, run as one undo step, repeat with Modify Again and save as
//! templates.

use core_engine::fixtures::{Builder, Q};
use core_model::modify::{DurationOp, Modify, ValueOp};
use std::sync::Arc;
use vizdaw_ffi::modify::{modify, modify_row, rule, rule_row};
use vizdaw_ffi::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, CurveRow, EditCommand, EventKindRow,
    LaneRef, ModifyRow, ModifyTemplateRow, NoteMapEntry, RuleRow, SelectModeRow, SelectionRow,
    Session, SettingsEdit, SubstituteRow, ValueTargetRow, VizError, WaveRow,
};

const C_MAJOR: u16 = 0b1010_1011_0101;

/// "S"/"L" with notes 60, 64, 67 a quarter apart (velocities 0x8000,
/// 0x4000, 0x2000).
fn session() -> (Arc<Session>, u64, u64, Vec<u64>) {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (s, l) = b.sequence("S", "L", Some(piano)).unwrap();
    let ids = b
        .notes(
            s,
            l,
            &[(0, 60, 0x8000, Q), (Q, 64, 0x4000, Q), (2 * Q, 67, 0x2000, Q)],
        )
        .unwrap();
    let s2 = Session::from_document(b.finish()).unwrap();
    (s2, s.0, l.0, ids.iter().map(|e| e.0).collect())
}

fn pitches(s: &Session, seq: u64, lane: u64) -> Vec<u8> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .map(|r| r.pitch)
        .collect()
}

fn select(s: &Session, seq: u64, lane: u64, events: Vec<u64>) {
    s.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq, lane }],
        events,
    })
    .unwrap();
}

fn cond(test: ConditionTest, negate: bool) -> ConditionRow {
    ConditionRow { test, negate }
}

fn sm(rule: Option<RuleRow>, modify: Option<ModifyRow>) -> EditCommand {
    EditCommand::SelectAndModify {
        rule,
        mode: SelectModeRow::Select,
        modify,
        to_copy: false,
    }
}

fn velocity_set(value: i64) -> ModifyRow {
    ModifyRow::Change {
        target: ValueTargetRow::Velocity,
        op: ChangeOpRow::Set,
        amount: AmountRow::Constant { value },
    }
}

#[test]
fn select_by_rule_then_transpose_is_one_undo_step() {
    let (s, seq, lane, ids) = session();
    select(&s, seq, lane, vec![]);
    let rule = RuleRow {
        conditions: vec![cond(ConditionTest::Pitch { low: 60, high: 64 }, false)],
        match_all: true,
    };
    let snap = s
        .edit(
            sm(Some(rule), Some(ModifyRow::TransposeInterval { semitones: 12 })),
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Transpose"));
    assert_eq!(pitches(&s, seq, lane), vec![72, 76, 67]);
    assert_eq!(s.selection().unwrap().events, ids[..2].to_vec());
    s.undo().unwrap();
    assert_eq!(pitches(&s, seq, lane), vec![60, 64, 67]);
}

#[test]
fn a_select_only_command_changes_the_selection_without_an_undo_step() {
    let (s, seq, lane, ids) = session();
    select(&s, seq, lane, vec![]);
    let not_67 = RuleRow {
        conditions: vec![cond(ConditionTest::Pitch { low: 67, high: 67 }, true)],
        match_all: true,
    };
    let snap = s.edit(sm(Some(not_67), None), None).unwrap();
    assert_eq!(snap.undo_label, None);
    assert_eq!(s.selection().unwrap().events, ids[..2].to_vec());
}

#[test]
fn modify_again_repeats_on_the_current_selection() {
    let (s, seq, lane, ids) = session();
    assert!(matches!(
        s.edit(EditCommand::ModifyAgain, None),
        Err(VizError::Model { .. })
    ));
    assert_eq!(s.last_modify().unwrap(), None);
    select(&s, seq, lane, vec![ids[0]]);
    s.edit(sm(None, Some(velocity_set(100))), None).unwrap();
    assert_eq!(s.last_modify().unwrap(), Some(velocity_set(100)));
    select(&s, seq, lane, vec![ids[2]]);
    let snap = s.edit(EditCommand::ModifyAgain, None).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Change Velocity"));
    let rows = s.lane_events(seq, lane).unwrap().events;
    assert_eq!(rows[2].velocity, 100);
    assert_eq!(rows[1].velocity, 0x4000, "not selected");
}

#[test]
fn templates_save_list_and_delete() {
    let (s, _, _, _) = session();
    let t = ModifyTemplateRow {
        name: "Accent".into(),
        rule: Some(RuleRow {
            conditions: vec![cond(
                ConditionTest::Kinds {
                    kinds: vec![EventKindRow::Note],
                },
                false,
            )],
            match_all: true,
        }),
        mode: SelectModeRow::Refine,
        modify: Some(ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Add,
            amount: AmountRow::Line {
                from: 0,
                to: 4096,
                curve: CurveRow::EaseOut,
            },
        }),
        to_copy: true,
    };
    let snap = s
        .edit(
            EditCommand::Settings {
                edit: SettingsEdit::SaveTemplate {
                    template: t.clone(),
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Set Modify Template"));
    assert_eq!(s.modify_templates().unwrap(), vec![t]);
    s.edit(
        EditCommand::Settings {
            edit: SettingsEdit::DeleteTemplate {
                name: "Accent".into(),
            },
        },
        None,
    )
    .unwrap();
    assert!(s.modify_templates().unwrap().is_empty());
    let blank = ModifyTemplateRow {
        name: "  ".into(),
        rule: None,
        mode: SelectModeRow::Select,
        modify: None,
        to_copy: false,
    };
    assert!(matches!(
        s.edit(
            EditCommand::Settings {
                edit: SettingsEdit::SaveTemplate { template: blank }
            },
            None
        ),
        Err(VizError::Model { .. })
    ));
}

#[test]
fn every_modify_row_round_trips() {
    let osc = AmountRow::Oscillator {
        wave: WaveRow::Triangle,
        low: 0,
        high: 10,
        period: Q,
        phase: 0,
    };
    let all = vec![
        ModifyRow::TransposeInterval { semitones: -3 },
        ModifyRow::TransposeDiatonic {
            steps: 2,
            root: 0,
            mask: C_MAJOR,
        },
        ModifyRow::TransposeToScale {
            root: 2,
            mask: C_MAJOR,
        },
        ModifyRow::TransposeInvert { axis: 64 },
        ModifyRow::NoteMap {
            map: vec![NoteMapEntry { from: 36, to: 38 }],
        },
        ModifyRow::Quantize {
            grid: Q / 4,
            strength: 80,
            offsets: vec![],
        },
        ModifyRow::Quantize {
            grid: Q / 2,
            strength: 100,
            offsets: vec![0, 120],
        },
        velocity_set(5),
        ModifyRow::Change {
            target: ValueTargetRow::ReleaseVelocity,
            op: ChangeOpRow::Scale,
            amount: AmountRow::Line {
                from: 50,
                to: 150,
                curve: CurveRow::SCurve,
            },
        },
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Legato,
            amount: AmountRow::Constant { value: 0 },
        },
        ModifyRow::Change {
            target: ValueTargetRow::Value,
            op: ChangeOpRow::Limit { min: 1, max: 2 },
            amount: osc,
        },
        ModifyRow::Change {
            target: ValueTargetRow::Value,
            op: ChangeOpRow::Compress { low: 0, high: 9 },
            amount: AmountRow::Random {
                low: 1,
                high: 2,
                seed: 7,
            },
        },
        ModifyRow::Move { delta: -Q },
        ModifyRow::ScaleTime {
            num: 3,
            den: 2,
            anchor: Q,
        },
        ModifyRow::TrimStart { delta: 10 },
        ModifyRow::SetDest { dest: Some(3) },
        ModifyRow::SetDest { dest: None },
        ModifyRow::Substitute {
            to: SubstituteRow::Controller { index: 11 },
        },
        ModifyRow::Substitute {
            to: SubstituteRow::PitchBend,
        },
        ModifyRow::Substitute {
            to: SubstituteRow::ChannelPressure,
        },
        ModifyRow::Substitute {
            to: SubstituteRow::Pitch { pitch: 36 },
        },
        ModifyRow::Thin { spacing: Q / 8 },
        ModifyRow::Thicken { spacing: Q / 16 },
        ModifyRow::Delete,
    ];
    for m in all {
        assert_eq!(modify_row(&modify(&m).unwrap()), m);
    }
}

#[test]
fn every_condition_round_trips() {
    let tests = vec![
        ConditionTest::TimeRange { start: 0, end: Q },
        ConditionTest::Kinds {
            kinds: vec![EventKindRow::Note, EventKindRow::Cc],
        },
        ConditionTest::Pitch { low: 1, high: 2 },
        ConditionTest::Velocity { low: 3, high: 4 },
        ConditionTest::ReleaseVelocity { low: 5, high: 6 },
        ConditionTest::Duration { min: 7, max: 8 },
        ConditionTest::Value { low: 9, high: 10 },
        ConditionTest::Controller { low: 11, high: 12 },
        ConditionTest::Dest { dest: Some(4) },
        ConditionTest::Dest { dest: None },
        ConditionTest::Metric {
            grid: Q,
            offset: 0,
            tolerance: 10,
        },
        ConditionTest::EveryNth { n: 2, phase: 1 },
        ConditionTest::BracketedBy { low: 36, high: 36 },
    ];
    for match_all in [true, false] {
        let r = RuleRow {
            conditions: tests
                .iter()
                .enumerate()
                .map(|(i, t)| cond(t.clone(), i % 2 == 1))
                .collect(),
            match_all,
        };
        assert_eq!(rule_row(&rule(&r).unwrap()), Some(r));
    }
}

#[test]
fn legacy_modifications_read_as_change_rows() {
    assert_eq!(
        modify_row(&Modify::Velocity(ValueOp::Add(-5))),
        ModifyRow::Change {
            target: ValueTargetRow::Velocity,
            op: ChangeOpRow::Add,
            amount: AmountRow::Constant { value: -5 },
        }
    );
    assert_eq!(
        modify_row(&Modify::Duration(DurationOp::ScalePercent(50))),
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Scale,
            amount: AmountRow::Constant { value: 50 },
        }
    );
    assert_eq!(
        modify_row(&Modify::Legato),
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Legato,
            amount: AmountRow::Constant { value: 0 },
        }
    );
    assert_eq!(
        modify_row(&Modify::ControllerValue(ValueOp::Set(9))),
        ModifyRow::Change {
            target: ValueTargetRow::Value,
            op: ChangeOpRow::Set,
            amount: AmountRow::Constant { value: 9 },
        }
    );
}

#[test]
fn bad_rules_and_modifications_are_refused_and_change_nothing() {
    let (s, seq, lane, ids) = session();
    select(&s, seq, lane, ids.clone());
    let one = |test| RuleRow {
        conditions: vec![cond(test, false)],
        match_all: true,
    };
    let bad = vec![
        sm(
            Some(one(ConditionTest::Metric {
                grid: 0,
                offset: 0,
                tolerance: 0,
            })),
            None,
        ),
        sm(Some(one(ConditionTest::EveryNth { n: 0, phase: 0 })), None),
        sm(Some(one(ConditionTest::TimeRange { start: 5, end: 5 })), None),
        sm(
            None,
            Some(ModifyRow::TransposeDiatonic {
                steps: 1,
                root: 12,
                mask: C_MAJOR,
            }),
        ),
        sm(
            None,
            Some(ModifyRow::Change {
                target: ValueTargetRow::Velocity,
                op: ChangeOpRow::Set,
                amount: AmountRow::Oscillator {
                    wave: WaveRow::Sine,
                    low: 0,
                    high: 1,
                    period: 0,
                    phase: 0,
                },
            }),
        ),
        sm(None, Some(ModifyRow::Thin { spacing: 0 })),
    ];
    for cmd in bad {
        assert!(
            matches!(s.edit(cmd.clone(), None), Err(VizError::Model { .. })),
            "{cmd:?}"
        );
    }
    assert_eq!(pitches(&s, seq, lane), vec![60, 64, 67]);
    assert_eq!(s.snapshot().unwrap().undo_label, None);
}

#[test]
fn modify_again_is_not_a_gesture_command() {
    assert!(vizdaw_ffi::edit::to_command(&EditCommand::ModifyAgain).is_err());
}
```

In `crates/core-ffi/tests/edit.rs`, Find:

```rust
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event: f.region.0,
                fade_in: fade(Q / 8),
                fade_out: fade(Q / 4),
            },
        },
    ]
}
```

Replace:

```rust
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event: f.region.0,
                fade_in: fade(Q / 8),
                fade_out: fade(Q / 4),
            },
        },
        ev(EventEdit::Replace {
            event: f.note.0,
            row: replaced_note(),
        }),
    ]
}

/// The fixture's note as a list edit leaves it: later, higher, shorter.
fn replaced_note() -> EventRow {
    let mut r = EventRow::blank(0, Q / 2, EventKindRow::Note);
    r.pitch = 70;
    r.velocity = 0x7000;
    r.duration = Q / 2;
    r
}
```

Find:

```rust
    assert_eq!(
        all.len(),
        50,
        "every variant (Move twice) but FromSelection, with 4b's four"
    );
```

Replace:

```rust
    assert_eq!(
        all.len(),
        51,
        "every variant (Move twice) but FromSelection, with 4b's four and 4c's Replace"
    );
```

Append to `crates/core-ffi/tests/edit.rs`:

```rust
/// 4b ledger T3: the 4b variants (and 4c's Replace) refuse bad input at the
/// FFI and change nothing.
#[test]
fn the_newer_event_variants_refuse_bad_input_and_change_nothing() {
    let f = fixture();
    let s = Session::from_document(f.doc.clone()).unwrap();
    let start = state(&s);
    let bad = vec![
        ev(EventEdit::TransposeInScale {
            events: vec![f.note.0],
            steps: 1,
            root: 12,
            mask: 0b1010_1011_0101,
        }),
        ev(EventEdit::TransposeInScale {
            events: vec![f.note.0],
            steps: 1,
            root: 0,
            mask: 0,
        }),
        ev(EventEdit::SetPerNote {
            events: vec![f.note.0],
            kind: PerNoteKindRow::PitchBend,
            from: 10,
            to: 5,
            points: vec![],
        }),
        ev(EventEdit::Resize {
            events: vec![9_999],
            delta: 10,
        }),
        ev(EventEdit::Duplicate {
            events: vec![9_999],
            ticks: 10,
            semitones: 0,
        }),
        ev(EventEdit::Replace {
            event: f.note.0,
            row: EventRow::blank(0, 0, EventKindRow::Cc),
        }),
        ev(EventEdit::Replace {
            event: 9_999,
            row: replaced_note(),
        }),
    ];
    for cmd in bad {
        assert!(
            matches!(s.edit(cmd.clone(), None), Err(VizError::Model { .. })),
            "{cmd:?}"
        );
    }
    assert_eq!(state(&s), start);
    assert_eq!(s.snapshot().unwrap().undo_label, None);
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test modify --test edit 2>&1 | tail -15
```

Expected: compile errors (`unresolved import vizdaw_ffi::modify`, `no variant named Replace`).

- [ ] **Step 3: Write `crates/core-ffi/src/modify.rs`**

```bash
cat > crates/core-ffi/src/modify.rs <<'VIZDAW_EOF'
//! Select-and-modify (parent spec §9, plan 4c): rules, modifications and
//! templates as flat records. A rule is a list of conditions joined by
//! "all" or "any", each optionally negated (UniFFI enums cannot hold
//! themselves, Plan Ruling 4).

use crate::error::VizError;
use crate::events::{EventKindRow, bad, kind_of};
use core_model::command::SettingsCmd;
use core_model::document::Document;
use core_model::event::EventKind;
use core_model::ids::DestId;
use core_model::modify::{
    Amount, ChangeOp, Curve, Density, DurationOp, Modify, ModifyTemplate, Quantize, Substitute,
    Transpose, ValueOp, ValueTarget, Wave,
};
use core_model::rule::{Rule, SelectMode};
use core_model::time::Tick;

/// One test of a selection rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ConditionTest {
    /// Start in `[start, end)`.
    TimeRange {
        /// First tick.
        start: i64,
        /// End (exclusive, after `start`).
        end: i64,
    },
    /// Kind is one of these.
    Kinds {
        /// Accepted kinds.
        kinds: Vec<EventKindRow>,
    },
    /// Note or poly-pressure pitch in range.
    Pitch {
        /// Lowest.
        low: u8,
        /// Highest.
        high: u8,
    },
    /// Note velocity (16-bit) in range.
    Velocity {
        /// Lowest.
        low: u16,
        /// Highest.
        high: u16,
    },
    /// Note release velocity (16-bit) in range.
    ReleaseVelocity {
        /// Lowest.
        low: u16,
        /// Highest.
        high: u16,
    },
    /// Note length in range.
    Duration {
        /// Shortest.
        min: i64,
        /// Longest.
        max: i64,
    },
    /// CC, pitch-bend and pressure value, or program number, in range.
    Value {
        /// Lowest.
        low: u32,
        /// Highest.
        high: u32,
    },
    /// CC controller number in range.
    Controller {
        /// Lowest.
        low: u8,
        /// Highest.
        high: u8,
    },
    /// Resolved destination (None: unresolved).
    Dest {
        /// Destination.
        dest: Option<u64>,
    },
    /// Within `tolerance` of a grid line.
    Metric {
        /// Grid (> 0).
        grid: i64,
        /// Grid offset.
        offset: i64,
        /// Allowed distance.
        tolerance: i64,
    },
    /// Every `n`th candidate from `phase`.
    EveryNth {
        /// Period (>= 1).
        n: u32,
        /// First match.
        phase: u32,
    },
    /// Starts inside a note of this pitch range in its lane.
    BracketedBy {
        /// Lowest pitch.
        low: u8,
        /// Highest pitch.
        high: u8,
    },
}

/// A condition, optionally negated ("not equal", "outside").
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ConditionRow {
    /// The test.
    pub test: ConditionTest,
    /// Match when the test fails.
    pub negate: bool,
}

/// A selection rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct RuleRow {
    /// The conditions.
    pub conditions: Vec<ConditionRow>,
    /// All must match (else any).
    pub match_all: bool,
}

/// How a rule's matches combine with the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SelectModeRow {
    /// Replace the selected events.
    Select,
    /// Add to them.
    Add,
    /// Keep the selected ones that match.
    Refine,
}

/// Which value a change edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ValueTargetRow {
    /// Note velocity.
    Velocity,
    /// Note release velocity.
    ReleaseVelocity,
    /// Note length.
    Duration,
    /// CC, pitch-bend and pressure values.
    Value,
}

/// Line shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum CurveRow {
    /// Straight.
    Straight,
    /// Slow, then fast.
    EaseIn,
    /// Fast, then slow.
    EaseOut,
    /// Slow at both ends.
    SCurve,
}

/// Oscillator waves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum WaveRow {
    /// Sine.
    Sine,
    /// Square.
    Square,
    /// Triangle.
    Triangle,
    /// Saw.
    Saw,
}

/// An amount (Plan Ruling 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum AmountRow {
    /// The same for every event.
    Constant {
        /// Amount.
        value: i64,
    },
    /// Along a line from the first to the last event.
    Line {
        /// At the first.
        from: i64,
        /// At the last.
        to: i64,
        /// Shape.
        curve: CurveRow,
    },
    /// A wave.
    Oscillator {
        /// Wave.
        wave: WaveRow,
        /// Lowest.
        low: i64,
        /// Highest.
        high: i64,
        /// Period in ticks (> 0).
        period: i64,
        /// Shift in ticks.
        phase: i64,
    },
    /// Random per event, fixed by the seed.
    Random {
        /// Lowest.
        low: i64,
        /// Highest.
        high: i64,
        /// Seed.
        seed: u64,
    },
}

/// How a change combines its amount with each value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ChangeOpRow {
    /// The amount.
    Set,
    /// Times the amount in percent.
    Scale,
    /// Plus the amount.
    Add,
    /// Kept inside `min..=max`.
    Limit {
        /// Lowest.
        min: i64,
        /// Highest.
        max: i64,
    },
    /// The range mapped onto `low..=high`.
    Compress {
        /// New lowest.
        low: i64,
        /// New highest.
        high: i64,
    },
    /// To the next note (lengths only).
    Legato,
}

/// One pitch of a note map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NoteMapEntry {
    /// Pitch replaced.
    pub from: u8,
    /// Its replacement.
    pub to: u8,
}

/// What Substitute turns events into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SubstituteRow {
    /// CC `index`.
    Controller {
        /// Controller number.
        index: u8,
    },
    /// Pitch bend.
    PitchBend,
    /// Channel pressure.
    ChannelPressure,
    /// One pitch for notes.
    Pitch {
        /// The pitch.
        pitch: u8,
    },
}

/// A modification (parent spec §9 modify half).
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ModifyRow {
    /// By semitones.
    TransposeInterval {
        /// Semitones.
        semitones: i8,
    },
    /// By scale degrees.
    TransposeDiatonic {
        /// Degrees.
        steps: i8,
        /// Root 0–11.
        root: u8,
        /// Scale mask.
        mask: u16,
    },
    /// Snapped to a scale.
    TransposeToScale {
        /// Root 0–11.
        root: u8,
        /// Scale mask.
        mask: u16,
    },
    /// Mirrored around a pitch.
    TransposeInvert {
        /// The pitch that stays.
        axis: u8,
    },
    /// By a note map.
    NoteMap {
        /// The map.
        map: Vec<NoteMapEntry>,
    },
    /// Quantize to a grid (no offsets) or a groove.
    Quantize {
        /// Grid in ticks.
        grid: i64,
        /// Strength 0–100.
        strength: u8,
        /// Groove offsets per grid line.
        offsets: Vec<i64>,
    },
    /// Change a value.
    Change {
        /// Which value.
        target: ValueTargetRow,
        /// How.
        op: ChangeOpRow,
        /// Amount.
        amount: AmountRow,
    },
    /// Move in time.
    Move {
        /// Offset.
        delta: i64,
    },
    /// Scale time around an anchor.
    ScaleTime {
        /// Numerator.
        num: u32,
        /// Denominator.
        den: u32,
        /// Fixed point.
        anchor: i64,
    },
    /// Move note starts, keeping ends.
    TrimStart {
        /// Offset.
        delta: i64,
    },
    /// Set the destination override.
    SetDest {
        /// Destination (None: the lane's).
        dest: Option<u64>,
    },
    /// Substitute kinds or pitches.
    Substitute {
        /// Into what.
        to: SubstituteRow,
    },
    /// Thin out controllers.
    Thin {
        /// Minimum distance.
        spacing: i64,
    },
    /// Thicken controllers.
    Thicken {
        /// Distance between added events.
        spacing: i64,
    },
    /// Delete.
    Delete,
}

/// A named configuration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct ModifyTemplateRow {
    /// Name (not blank).
    pub name: String,
    /// Selection rule.
    pub rule: Option<RuleRow>,
    /// Selection mode.
    pub mode: SelectModeRow,
    /// Modification.
    pub modify: Option<ModifyRow>,
    /// Modify copies.
    pub to_copy: bool,
}

/// Document-settings edits (templates).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SettingsEdit {
    /// Store a template under its name (replacing one of that name).
    SaveTemplate {
        /// The template.
        template: ModifyTemplateRow,
    },
    /// Remove a template.
    DeleteTemplate {
        /// Its name.
        name: String,
    },
}

fn kind_row(k: EventKind) -> EventKindRow {
    match k {
        EventKind::Note => EventKindRow::Note,
        EventKind::Cc => EventKindRow::Cc,
        EventKind::PitchBend => EventKindRow::PitchBend,
        EventKind::ChannelPressure => EventKindRow::ChannelPressure,
        EventKind::PolyPressure => EventKindRow::PolyPressure,
        EventKind::Program => EventKindRow::Program,
        EventKind::SysEx => EventKindRow::SysEx,
        EventKind::Text => EventKindRow::Text,
        EventKind::Reference => EventKindRow::Reference,
        EventKind::AudioRegion => EventKindRow::AudioRegion,
    }
}

/// The core mode.
pub fn mode_of(m: SelectModeRow) -> SelectMode {
    match m {
        SelectModeRow::Select => SelectMode::Select,
        SelectModeRow::Add => SelectMode::Add,
        SelectModeRow::Refine => SelectMode::Refine,
    }
}

fn mode_row(m: SelectMode) -> SelectModeRow {
    match m {
        SelectMode::Select => SelectModeRow::Select,
        SelectMode::Add => SelectModeRow::Add,
        SelectMode::Refine => SelectModeRow::Refine,
    }
}

fn leaf(t: &ConditionTest) -> Result<Rule, VizError> {
    Ok(match t {
        ConditionTest::TimeRange { start, end } => {
            if end <= start {
                return Err(bad("the time range is empty"));
            }
            Rule::TimeRange {
                start: Tick(*start),
                end: Tick(*end),
            }
        }
        ConditionTest::Kinds { kinds } => Rule::Kind {
            kinds: kinds.iter().map(|k| kind_of(*k)).collect(),
        },
        ConditionTest::Pitch { low, high } => Rule::PitchRange {
            low: *low,
            high: *high,
        },
        ConditionTest::Velocity { low, high } => Rule::VelocityRange {
            low: *low,
            high: *high,
        },
        ConditionTest::ReleaseVelocity { low, high } => Rule::ReleaseVelocityRange {
            low: *low,
            high: *high,
        },
        ConditionTest::Duration { min, max } => Rule::DurationRange {
            min: Tick(*min),
            max: Tick(*max),
        },
        ConditionTest::Value { low, high } => Rule::ValueRange {
            low: *low,
            high: *high,
        },
        ConditionTest::Controller { low, high } => Rule::ControllerRange {
            low: *low,
            high: *high,
        },
        ConditionTest::Dest { dest } => Rule::Dest {
            dest: dest.map(DestId),
        },
        ConditionTest::Metric {
            grid,
            offset,
            tolerance,
        } => {
            if *grid <= 0 {
                return Err(bad("the metric grid must be longer than 0 ticks"));
            }
            Rule::MetricPosition {
                grid: Tick(*grid),
                offset: Tick(*offset),
                tolerance: Tick(*tolerance),
            }
        }
        ConditionTest::EveryNth { n, phase } => {
            if *n == 0 {
                return Err(bad("every nth needs n of 1 or more"));
            }
            Rule::EveryNth {
                n: *n,
                phase: *phase,
            }
        }
        ConditionTest::BracketedBy { low, high } => Rule::Bracketed {
            by: Box::new(Rule::PitchRange {
                low: *low,
                high: *high,
            }),
        },
    })
}

/// The core rule of a row.
pub fn rule(r: &RuleRow) -> Result<Rule, VizError> {
    let mut parts = Vec::with_capacity(r.conditions.len());
    for c in &r.conditions {
        let l = leaf(&c.test)?;
        parts.push(if c.negate { Rule::Not(Box::new(l)) } else { l });
    }
    Ok(if r.match_all {
        Rule::All(parts)
    } else {
        Rule::Any(parts)
    })
}

fn test_of(r: &Rule) -> Option<ConditionTest> {
    Some(match r {
        Rule::TimeRange { start, end } => ConditionTest::TimeRange {
            start: start.0,
            end: end.0,
        },
        Rule::Kind { kinds } => ConditionTest::Kinds {
            kinds: kinds.iter().map(|k| kind_row(*k)).collect(),
        },
        Rule::PitchRange { low, high } => ConditionTest::Pitch {
            low: *low,
            high: *high,
        },
        Rule::VelocityRange { low, high } => ConditionTest::Velocity {
            low: *low,
            high: *high,
        },
        Rule::ReleaseVelocityRange { low, high } => ConditionTest::ReleaseVelocity {
            low: *low,
            high: *high,
        },
        Rule::DurationRange { min, max } => ConditionTest::Duration {
            min: min.0,
            max: max.0,
        },
        Rule::ValueRange { low, high } => ConditionTest::Value {
            low: *low,
            high: *high,
        },
        Rule::ControllerRange { low, high } => ConditionTest::Controller {
            low: *low,
            high: *high,
        },
        Rule::Dest { dest } => ConditionTest::Dest {
            dest: dest.map(|d| d.0),
        },
        Rule::MetricPosition {
            grid,
            offset,
            tolerance,
        } => ConditionTest::Metric {
            grid: grid.0,
            offset: offset.0,
            tolerance: tolerance.0,
        },
        Rule::EveryNth { n, phase } => ConditionTest::EveryNth {
            n: *n,
            phase: *phase,
        },
        Rule::Bracketed { by } => match by.as_ref() {
            Rule::PitchRange { low, high } => ConditionTest::BracketedBy {
                low: *low,
                high: *high,
            },
            _ => return None,
        },
        Rule::All(_) | Rule::Any(_) | Rule::Not(_) => return None,
    })
}

fn condition_of(r: &Rule) -> Option<ConditionRow> {
    match r {
        Rule::Not(inner) => test_of(inner).map(|test| ConditionRow { test, negate: true }),
        other => test_of(other).map(|test| ConditionRow {
            test,
            negate: false,
        }),
    }
}

/// The row of a core rule; None when it is not a flat list of conditions.
pub fn rule_row(r: &Rule) -> Option<RuleRow> {
    let (parts, match_all) = match r {
        Rule::All(p) => (p.as_slice(), true),
        Rule::Any(p) => (p.as_slice(), false),
        other => (std::slice::from_ref(other), true),
    };
    let conditions = parts.iter().map(condition_of).collect::<Option<Vec<_>>>()?;
    Some(RuleRow {
        conditions,
        match_all,
    })
}

fn target_of(t: ValueTargetRow) -> ValueTarget {
    match t {
        ValueTargetRow::Velocity => ValueTarget::Velocity,
        ValueTargetRow::ReleaseVelocity => ValueTarget::ReleaseVelocity,
        ValueTargetRow::Duration => ValueTarget::Duration,
        ValueTargetRow::Value => ValueTarget::Value,
    }
}

fn target_row(t: ValueTarget) -> ValueTargetRow {
    match t {
        ValueTarget::Velocity => ValueTargetRow::Velocity,
        ValueTarget::ReleaseVelocity => ValueTargetRow::ReleaseVelocity,
        ValueTarget::Duration => ValueTargetRow::Duration,
        ValueTarget::Value => ValueTargetRow::Value,
    }
}

fn op_of(o: ChangeOpRow) -> ChangeOp {
    match o {
        ChangeOpRow::Set => ChangeOp::Set,
        ChangeOpRow::Scale => ChangeOp::Scale,
        ChangeOpRow::Add => ChangeOp::Add,
        ChangeOpRow::Limit { min, max } => ChangeOp::Limit { min, max },
        ChangeOpRow::Compress { low, high } => ChangeOp::Compress { low, high },
        ChangeOpRow::Legato => ChangeOp::Legato,
    }
}

fn op_row(o: ChangeOp) -> ChangeOpRow {
    match o {
        ChangeOp::Set => ChangeOpRow::Set,
        ChangeOp::Scale => ChangeOpRow::Scale,
        ChangeOp::Add => ChangeOpRow::Add,
        ChangeOp::Limit { min, max } => ChangeOpRow::Limit { min, max },
        ChangeOp::Compress { low, high } => ChangeOpRow::Compress { low, high },
        ChangeOp::Legato => ChangeOpRow::Legato,
    }
}

fn curve_of(c: CurveRow) -> Curve {
    match c {
        CurveRow::Straight => Curve::Straight,
        CurveRow::EaseIn => Curve::EaseIn,
        CurveRow::EaseOut => Curve::EaseOut,
        CurveRow::SCurve => Curve::SCurve,
    }
}

fn curve_row(c: Curve) -> CurveRow {
    match c {
        Curve::Straight => CurveRow::Straight,
        Curve::EaseIn => CurveRow::EaseIn,
        Curve::EaseOut => CurveRow::EaseOut,
        Curve::SCurve => CurveRow::SCurve,
    }
}

fn wave_of(w: WaveRow) -> Wave {
    match w {
        WaveRow::Sine => Wave::Sine,
        WaveRow::Square => Wave::Square,
        WaveRow::Triangle => Wave::Triangle,
        WaveRow::Saw => Wave::Saw,
    }
}

fn wave_row(w: Wave) -> WaveRow {
    match w {
        Wave::Sine => WaveRow::Sine,
        Wave::Square => WaveRow::Square,
        Wave::Triangle => WaveRow::Triangle,
        Wave::Saw => WaveRow::Saw,
    }
}

fn amount_of(a: AmountRow) -> Amount {
    match a {
        AmountRow::Constant { value } => Amount::Constant { value },
        AmountRow::Line { from, to, curve } => Amount::Line {
            from,
            to,
            curve: curve_of(curve),
        },
        AmountRow::Oscillator {
            wave,
            low,
            high,
            period,
            phase,
        } => Amount::Oscillator {
            wave: wave_of(wave),
            low,
            high,
            period: Tick(period),
            phase: Tick(phase),
        },
        AmountRow::Random { low, high, seed } => Amount::Random { low, high, seed },
    }
}

fn amount_row(a: Amount) -> AmountRow {
    match a {
        Amount::Constant { value } => AmountRow::Constant { value },
        Amount::Line { from, to, curve } => AmountRow::Line {
            from,
            to,
            curve: curve_row(curve),
        },
        Amount::Oscillator {
            wave,
            low,
            high,
            period,
            phase,
        } => AmountRow::Oscillator {
            wave: wave_row(wave),
            low,
            high,
            period: period.0,
            phase: phase.0,
        },
        Amount::Random { low, high, seed } => AmountRow::Random { low, high, seed },
    }
}

fn subst_of(s: SubstituteRow) -> Substitute {
    match s {
        SubstituteRow::Controller { index } => Substitute::Controller { index },
        SubstituteRow::PitchBend => Substitute::PitchBend,
        SubstituteRow::ChannelPressure => Substitute::ChannelPressure,
        SubstituteRow::Pitch { pitch } => Substitute::Pitch { pitch },
    }
}

fn subst_row(s: Substitute) -> SubstituteRow {
    match s {
        Substitute::Controller { index } => SubstituteRow::Controller { index },
        Substitute::PitchBend => SubstituteRow::PitchBend,
        Substitute::ChannelPressure => SubstituteRow::ChannelPressure,
        Substitute::Pitch { pitch } => SubstituteRow::Pitch { pitch },
    }
}

/// The core modification of a row (values are checked when it runs).
pub fn modify(m: &ModifyRow) -> Result<Modify, VizError> {
    Ok(match m {
        ModifyRow::TransposeInterval { semitones } => Modify::Transpose(Transpose::Interval {
            semitones: *semitones,
        }),
        ModifyRow::TransposeDiatonic { steps, root, mask } => {
            Modify::Transpose(Transpose::Diatonic {
                steps: *steps,
                root: *root,
                mask: *mask,
            })
        }
        ModifyRow::TransposeToScale { root, mask } => Modify::Transpose(Transpose::ToScale {
            root: *root,
            mask: *mask,
        }),
        ModifyRow::TransposeInvert { axis } => {
            Modify::Transpose(Transpose::Invert { axis: *axis })
        }
        ModifyRow::NoteMap { map } => Modify::NoteMap {
            map: map.iter().map(|e| (e.from, e.to)).collect(),
        },
        ModifyRow::Quantize {
            grid,
            strength,
            offsets,
        } => Modify::Quantize(if offsets.is_empty() {
            Quantize::Grid {
                grid: Tick(*grid),
                strength: *strength,
            }
        } else {
            Quantize::Groove {
                grid: Tick(*grid),
                offsets: offsets.iter().copied().map(Tick).collect(),
                strength: *strength,
            }
        }),
        ModifyRow::Change { target, op, amount } => Modify::Change {
            target: target_of(*target),
            op: op_of(*op),
            amount: amount_of(*amount),
        },
        ModifyRow::Move { delta } => Modify::Move {
            delta: Tick(*delta),
        },
        ModifyRow::ScaleTime { num, den, anchor } => Modify::ScaleTime {
            num: *num,
            den: *den,
            anchor: Tick(*anchor),
        },
        ModifyRow::TrimStart { delta } => Modify::TrimStart {
            delta: Tick(*delta),
        },
        ModifyRow::SetDest { dest } => Modify::ReassignDest {
            dest: dest.map(DestId),
        },
        ModifyRow::Substitute { to } => Modify::Substitute(subst_of(*to)),
        ModifyRow::Thin { spacing } => Modify::Density(Density::Thin {
            spacing: Tick(*spacing),
        }),
        ModifyRow::Thicken { spacing } => Modify::Density(Density::Thicken {
            spacing: Tick(*spacing),
        }),
        ModifyRow::Delete => Modify::Delete,
    })
}

fn constant(target: ValueTargetRow, op: ChangeOpRow, value: i64) -> ModifyRow {
    ModifyRow::Change {
        target,
        op,
        amount: AmountRow::Constant { value },
    }
}

fn legacy(target: ValueTargetRow, op: ValueOp) -> ModifyRow {
    match op {
        ValueOp::Set(v) => constant(target, ChangeOpRow::Set, i64::from(v)),
        ValueOp::ScalePercent(p) => constant(target, ChangeOpRow::Scale, i64::from(p)),
        ValueOp::Add(a) => constant(target, ChangeOpRow::Add, a),
    }
}

/// The row of a core modification; the legacy value variants read as
/// `Change` rows (Plan Ruling 2).
pub fn modify_row(m: &Modify) -> ModifyRow {
    match m {
        Modify::Transpose(Transpose::Interval { semitones }) => ModifyRow::TransposeInterval {
            semitones: *semitones,
        },
        Modify::Transpose(Transpose::ToScale { root, mask }) => ModifyRow::TransposeToScale {
            root: *root,
            mask: *mask,
        },
        Modify::Transpose(Transpose::Diatonic { steps, root, mask }) => {
            ModifyRow::TransposeDiatonic {
                steps: *steps,
                root: *root,
                mask: *mask,
            }
        }
        Modify::Transpose(Transpose::Invert { axis }) => {
            ModifyRow::TransposeInvert { axis: *axis }
        }
        Modify::NoteMap { map } => ModifyRow::NoteMap {
            map: map
                .iter()
                .map(|(from, to)| NoteMapEntry {
                    from: *from,
                    to: *to,
                })
                .collect(),
        },
        Modify::Quantize(Quantize::Grid { grid, strength }) => ModifyRow::Quantize {
            grid: grid.0,
            strength: *strength,
            offsets: Vec::new(),
        },
        Modify::Quantize(Quantize::Groove {
            grid,
            offsets,
            strength,
        }) => ModifyRow::Quantize {
            grid: grid.0,
            strength: *strength,
            offsets: offsets.iter().map(|o| o.0).collect(),
        },
        Modify::Velocity(op) => legacy(ValueTargetRow::Velocity, *op),
        Modify::ControllerValue(op) => legacy(ValueTargetRow::Value, *op),
        Modify::Duration(DurationOp::Set(t)) => {
            constant(ValueTargetRow::Duration, ChangeOpRow::Set, t.0)
        }
        Modify::Duration(DurationOp::ScalePercent(p)) => {
            constant(ValueTargetRow::Duration, ChangeOpRow::Scale, i64::from(*p))
        }
        Modify::Legato => constant(ValueTargetRow::Duration, ChangeOpRow::Legato, 0),
        Modify::Change {
            target,
            op,
            amount,
        } => ModifyRow::Change {
            target: target_row(*target),
            op: op_row(*op),
            amount: amount_row(*amount),
        },
        Modify::ReassignDest { dest } => ModifyRow::SetDest {
            dest: dest.map(|d| d.0),
        },
        Modify::ScaleTime { num, den, anchor } => ModifyRow::ScaleTime {
            num: *num,
            den: *den,
            anchor: anchor.0,
        },
        Modify::Move { delta } => ModifyRow::Move { delta: delta.0 },
        Modify::TrimStart { delta } => ModifyRow::TrimStart { delta: delta.0 },
        Modify::Substitute(s) => ModifyRow::Substitute { to: subst_row(*s) },
        Modify::Density(Density::Thin { spacing }) => ModifyRow::Thin { spacing: spacing.0 },
        Modify::Density(Density::Thicken { spacing }) => ModifyRow::Thicken {
            spacing: spacing.0,
        },
        Modify::Delete => ModifyRow::Delete,
    }
}

/// The core template of a row; a blank name is refused.
pub fn template(t: &ModifyTemplateRow) -> Result<ModifyTemplate, VizError> {
    if t.name.trim().is_empty() {
        return Err(bad("a template needs a name"));
    }
    Ok(ModifyTemplate {
        rule: t.rule.as_ref().map(rule).transpose()?,
        mode: mode_of(t.mode),
        modify: t.modify.as_ref().map(modify).transpose()?,
        to_copy: t.to_copy,
    })
}

/// The row of a stored template; None when its rule is not flat.
pub fn template_row(name: &str, t: &ModifyTemplate) -> Option<ModifyTemplateRow> {
    let rule = match &t.rule {
        Some(r) => Some(rule_row(r)?),
        None => None,
    };
    Some(ModifyTemplateRow {
        name: name.to_string(),
        rule,
        mode: mode_row(t.mode),
        modify: t.modify.as_ref().map(modify_row),
        to_copy: t.to_copy,
    })
}

/// Every template of `doc` that has a row, by name.
pub fn template_rows(doc: &Document) -> Vec<ModifyTemplateRow> {
    doc.settings()
        .templates
        .iter()
        .filter_map(|(n, t)| template_row(n, t))
        .collect()
}

/// The core settings command of a settings edit.
pub fn settings_cmd(e: &SettingsEdit) -> Result<SettingsCmd, VizError> {
    Ok(match e {
        SettingsEdit::SaveTemplate { template: t } => SettingsCmd::SetTemplate {
            name: t.name.trim().to_string(),
            template: Some(template(t)?),
        },
        SettingsEdit::DeleteTemplate { name } => SettingsCmd::SetTemplate {
            name: name.clone(),
            template: None,
        },
    })
}
VIZDAW_EOF
```

- [ ] **Step 4: Wire the commands**

In `crates/core-ffi/src/edit.rs`, Find:

```rust
    /// Audio-region edits.
    Audio {
        /// The edit.
        edit: AudioEdit,
    },
}
```

Replace:

```rust
    /// Audio-region edits.
    Audio {
        /// The edit.
        edit: AudioEdit,
    },
    /// Select by rule, then optionally modify the selection or copies (4c).
    SelectAndModify {
        /// Rule (None: the selection as it is).
        rule: Option<crate::modify::RuleRow>,
        /// How the matches combine with the selection.
        mode: crate::modify::SelectModeRow,
        /// Modification (None: select only, no undo step).
        modify: Option<crate::modify::ModifyRow>,
        /// Modify copies beside the originals.
        to_copy: bool,
    },
    /// Repeat the last modification on the current selection (4c, Plan
    /// Ruling 8; `Session.edit` only).
    ModifyAgain,
    /// Document settings: modify templates (4c).
    Settings {
        /// The edit.
        edit: crate::modify::SettingsEdit,
    },
}
```

Find:

```rust
        /// The new values (absolute ticks).
        points: Vec<PerNoteValueRow>,
    },
}
```

Replace:

```rust
        /// The new values (absolute ticks).
        points: Vec<PerNoteValueRow>,
    },
    /// Replace an event with a row of the same kind (a list edit, 4c).
    Replace {
        /// The event.
        event: u64,
        /// What it becomes (its ID is ignored).
        row: EventRow,
    },
}
```

Find:

```rust
        EditCommand::Audio { edit } => Command::Audio(audio_cmd(edit)),
    })
}
```

Replace:

```rust
        EditCommand::Audio { edit } => Command::Audio(audio_cmd(edit)),
        EditCommand::SelectAndModify {
            rule,
            mode,
            modify,
            to_copy,
        } => Command::SelectAndModify {
            rule: rule.as_ref().map(crate::modify::rule).transpose()?,
            mode: crate::modify::mode_of(*mode),
            modify: modify.as_ref().map(crate::modify::modify).transpose()?,
            to_copy: *to_copy,
        },
        EditCommand::ModifyAgain => {
            return Err(bad("Modify Again repeats a modification; it is not a command"));
        }
        EditCommand::Settings { edit } => Command::Settings(crate::modify::settings_cmd(edit)?),
    })
}
```

Find:

```rust
            points: points.iter().map(|p| (Tick(p.tick), p.value)).collect(),
        },
    })
}
```

Replace:

```rust
            points: points.iter().map(|p| (Tick(p.tick), p.value)).collect(),
        },
        EventEdit::Replace { event, row } => EventCmd::Replace {
            event: EventId(*event),
            spec: spec(row)?,
        },
    })
}
```

In `crates/core-ffi/src/session.rs`, Find (inside `pub fn edit`):

```rust
            if lock(&self.inner).edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            let c = crate::edit::to_command(&cmd)?;
            self.execute_with(c, &crate::edit::view_filter(filter.as_ref()))
```

Replace:

```rust
            if lock(&self.inner).edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            let filter = crate::edit::view_filter(filter.as_ref());
            if matches!(cmd, crate::edit::EditCommand::ModifyAgain) {
                return self.modify_again_with(&filter);
            }
            let c = crate::edit::to_command(&cmd)?;
            self.execute_with(c, &filter)
```

Find:

```rust
    fn execute_with(&self, cmd: Command, filter: &ViewFilter) -> Result<DocSnapshot, VizError> {
```

Replace:

```rust
    /// Modify Again (Plan 4c Ruling 8): the last modification on the
    /// current selection, one undo step named after it. It removes no
    /// plugin, so nothing is captured first.
    fn modify_again_with(&self, filter: &ViewFilter) -> Result<DocSnapshot, VizError> {
        let mut inner = lock(&self.inner);
        inner.history.modify_again(filter)?;
        inner.version += 1;
        inner.reload_host(self.id);
        self.mark(&mut inner);
        inner.snapshot()
    }

    fn execute_with(&self, cmd: Command, filter: &ViewFilter) -> Result<DocSnapshot, VizError> {
```

Find (the "Editor reads" export block):

```rust
    /// Tempo, meter, markers, start offset and length of sequence `seq`.
    pub fn sequence_info(&self, seq: u64) -> Result<crate::events::SequenceInfo, VizError> {
```

Replace:

```rust
    /// The select-and-modify templates, by name (4c).
    pub fn modify_templates(&self) -> Result<Vec<crate::modify::ModifyTemplateRow>, VizError> {
        ffi_guard(|| Ok(crate::modify::template_rows(lock(&self.inner).doc())))
    }

    /// The modification Modify Again repeats, if any (4c).
    pub fn last_modify(&self) -> Result<Option<crate::modify::ModifyRow>, VizError> {
        ffi_guard(|| {
            Ok(lock(&self.inner)
                .history
                .last_modify()
                .map(crate::modify::modify_row))
        })
    }

    /// Tempo, meter, markers, start offset and length of sequence `seq`.
    pub fn sequence_info(&self, seq: u64) -> Result<crate::events::SequenceInfo, VizError> {
```

In `crates/core-ffi/src/lib.rs`, Find:

```rust
pub mod mixer;
pub mod plugins;
```

Replace:

```rust
pub mod mixer;
pub mod modify;
pub mod plugins;
```

Find:

```rust
pub use guard::ffi_guard;
```

Replace:

```rust
pub use guard::ffi_guard;
pub use modify::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, CurveRow, ModifyRow, ModifyTemplateRow,
    NoteMapEntry, RuleRow, SelectModeRow, SettingsEdit, SubstituteRow, ValueTargetRow, WaveRow,
};
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test modify --test edit 2>&1 | tail -8
```

Expected: PASS (modify: 9 tests; edit: the new test plus the updated count).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -40
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/core-ffi/src/modify.rs crates/core-ffi/src/edit.rs crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs crates/core-ffi/tests/modify.rs crates/core-ffi/tests/edit.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(ffi): select-and-modify records, Modify Again, templates and the Replace edit

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 653 Rust tests pass (643 + 10). `SessionProtocol` now has two more methods, so `FakeSession` no longer conforms and the Swift test target stops compiling. Keep the gate green by adding these stubs to `app/Tests/VizDAWAppModelTests/FakeSession.swift` (Task 6 replaces them with scriptable versions) before running `scripts/swift.sh test`, and add that file to this commit:

```swift
    func modifyTemplates() throws -> [ModifyTemplateRow] { [] }
    func lastModify() throws -> ModifyRow? { nil }
```

---
### Task 5: MIDI note input; seconds and ticks for SMPTE; the remaining row round trips

**Files:**
- Modify: `crates/platform-macos/src/midi.rs`
- Test: `crates/platform-macos/tests/midi.rs`
- Create: `crates/core-ffi/src/input.rs`
- Modify: `crates/core-ffi/src/session.rs`
- Modify: `crates/core-ffi/src/lib.rs`
- Test: `crates/core-ffi/tests/input.rs` (new)
- Test: `crates/core-ffi/tests/events.rs`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift` (two stubs, replaced in Task 6)

**Interfaces:**
- Consumes: `platform_macos::midi::{decode, os}` (private helpers in the same file), `TempoMap::{tick_to_seconds, seconds_to_tick}`.
- Produces: `pub struct platform_macos::midi::NoteListener` with `pub fn connect_all(name: &str, sink: MessageSink) -> Result<NoteListener, BackendError>` and `pub fn sources(&self) -> u32`, `pub type MessageSink = Box<dyn Fn(&[u8]) + Send + Sync>`; UniFFI `record NoteInRow { pitch: u8, velocity: u16, channel: u8 }`, `object NoteInput` with constructor `new()`, `start() -> u32`, `stop()`, `take() -> Vec<NoteInRow>`, test-hook `inject(bytes: Vec<u8>)`; `pub fn vizdaw_ffi::input::{upscale_velocity(v: u8) -> u16, note_on(bytes: &[u8]) -> Option<NoteInRow>}`, `pub const CAPACITY: usize = 256`; `Session.seconds_at(seq: u64, ticks: Vec<i64>) -> Vec<f64>`, `Session.ticks_at(seq: u64, seconds: Vec<f64>) -> Vec<i64>`.

Rulings 11, 14, 25. `seconds_at` uses the sequence's own tempo map from its tick 0 (negative ticks are negative seconds); `ticks_at` rounds to the nearest tick. An unknown sequence is a `Model` error.

- [ ] **Step 1: Write the failing tests**

Append to `crates/platform-macos/tests/midi.rs`:

```rust
#[test]
fn a_note_listener_hears_every_source() {
    use platform_macos::midi::NoteListener;
    use std::sync::{Arc, Mutex};
    let mut sys = MidiSystem::new("VizDAW Keys Test").unwrap();
    let mut out = sys.open_virtual_source("VizDAW Keys Test Out").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let got = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let sink = Arc::clone(&got);
    let listener = NoteListener::connect_all(
        "VizDAW Keys Test In",
        Box::new(move |b: &[u8]| sink.lock().unwrap().push(b.to_vec())),
    )
    .unwrap();
    assert!(listener.sources() >= 1, "the virtual source counts");
    std::thread::sleep(Duration::from_millis(50));
    assert!(out.send(now_ns(), &[0x92, 61, 100]));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !got.lock().unwrap().contains(&vec![0x92, 61, 100]) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(got.lock().unwrap().contains(&vec![0x92, 61, 100]));
}
```

Create `crates/core-ffi/tests/input.rs`:

```rust
//! MIDI note input (plan 4c, Plan Ruling 11): note-ons queue with MIDI 2.0
//! velocities; note-offs and other messages do not.

use vizdaw_ffi::input::{CAPACITY, upscale_velocity};
use vizdaw_ffi::{NoteInRow, NoteInput};

#[test]
fn note_ons_queue_in_order_and_nothing_else_does() {
    let n = NoteInput::new();
    n.inject(vec![0x90, 60, 127]).unwrap();
    n.inject(vec![0x80, 60, 0]).unwrap();
    n.inject(vec![0x91, 62, 0]).unwrap();
    n.inject(vec![0xB0, 7, 100]).unwrap();
    n.inject(vec![0x9F, 64, 1]).unwrap();
    assert_eq!(
        n.take().unwrap(),
        vec![
            NoteInRow {
                pitch: 60,
                velocity: 0xFFFF,
                channel: 0
            },
            NoteInRow {
                pitch: 64,
                velocity: 1 << 9,
                channel: 15
            },
        ]
    );
    assert!(n.take().unwrap().is_empty());
}

#[test]
fn the_queue_keeps_the_newest_notes() {
    let n = NoteInput::new();
    for i in 0..300u32 {
        n.inject(vec![0x90, (i % 128) as u8, 100]).unwrap();
    }
    let got = n.take().unwrap();
    assert_eq!(got.len(), CAPACITY);
    assert_eq!(got[0].pitch, 44, "the oldest 44 were dropped");
}

#[test]
fn velocities_upscale_by_the_midi_2_rule() {
    assert_eq!(upscale_velocity(0), 0);
    assert_eq!(upscale_velocity(64), 0x8000);
    assert_eq!(upscale_velocity(100), 0xC924);
    assert_eq!(upscale_velocity(127), 0xFFFF);
    assert_eq!(upscale_velocity(100) >> 9, 100, "the 7-bit value reads back");
}

#[test]
fn starting_again_and_stopping_never_fail() {
    let n = NoteInput::new();
    n.start().unwrap();
    n.start().unwrap();
    n.stop().unwrap();
    n.stop().unwrap();
}
```

Append to `crates/core-ffi/tests/events.rs`:

```rust
#[test]
fn seconds_and_ticks_follow_the_sequence_tempo() {
    let mut b = Builder::new();
    let (seq, _) = b.sequence("S", "L", None).unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    let bpm = s.sequence_info(seq.0).unwrap().tempo[0].bpm;
    let spq = 60.0 / bpm;
    let secs = s.seconds_at(seq.0, vec![0, Q, -Q]).unwrap();
    assert_eq!(secs[0], 0.0);
    assert!((secs[1] - spq).abs() < 1e-9, "{secs:?}");
    assert!((secs[2] + spq).abs() < 1e-9, "{secs:?}");
    assert_eq!(s.ticks_at(seq.0, vec![spq, 2.0 * spq]).unwrap(), vec![Q, 2 * Q]);
    assert!(matches!(
        s.seconds_at(9_999, vec![0]),
        Err(VizError::Model { .. })
    ));
}

/// 4a ledger T2: the variants `every_kind` does not cover.
#[test]
fn the_other_variants_round_trip_too() {
    let mut b = Builder::new();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddMedia {
            path: "Media/b.wav".into(),
            sample_rate: 48_000,
            channels: 1,
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
    let (seq, lane) = b.sequence("Other", "Variants", Some(piano)).unwrap();
    let mut specs = vec![
        EventSpec::new(
            Tick(0),
            EventBody::Note {
                pitch: 50,
                velocity: 1,
                release_velocity: 2,
                duration: Tick(Q),
                attribute: None,
                per_note: [
                    PerNoteCtl {
                        kind: PerNoteKind::Assignable(3),
                        offset: Tick(5),
                        value: 9,
                    },
                    PerNoteCtl {
                        kind: PerNoteKind::PitchBend,
                        offset: Tick(6),
                        value: 0x8000_0000,
                    },
                ]
                .into_iter()
                .collect(),
            },
        ),
        EventSpec::new(
            Tick(10),
            EventBody::Program {
                program: 5,
                bank: None,
            },
        ),
        EventSpec::new(
            Tick(20),
            EventBody::SysEx {
                data: Arc::from(&[0x7E_u8, 0x7F][..]),
                eight_bit: false,
            },
        ),
    ];
    for (i, kind) in [
        TextKind::Text,
        TextKind::Lyric,
        TextKind::Marker,
        TextKind::Copyright,
    ]
    .into_iter()
    .enumerate()
    {
        specs.push(EventSpec::new(
            Tick(30 + i as i64),
            EventBody::Text {
                kind,
                text: format!("t{i}"),
            },
        ));
    }
    specs.push(EventSpec::new(
        Tick(BAR),
        EventBody::Reference {
            seq: sub,
            length: Some(Tick(Q)),
            loop_count: LoopCount::Fill,
            transpose: 0,
        },
    ));
    specs.push(EventSpec::new(
        Tick(2 * BAR),
        EventBody::AudioRegion {
            media,
            source_offset: 0,
            length: 48_000,
            gain_db: 0.0,
            fade_in: Fade {
                length: Tick(Q / 2),
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length: Tick(Q / 2),
                shape: FadeShape::Exponential,
            },
        },
    ));
    b.events(seq, lane, specs).unwrap();
    let doc = b.finish();
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
    assert_eq!(rows.len(), originals.len());
    for (row, original) in rows.iter().zip(&originals) {
        assert_eq!(&spec(row).unwrap(), original, "{:?}", row.kind);
    }
    assert_eq!(rows[1].value2, -1, "no bank");
    let reference = rows.iter().find(|r| r.kind == EventKindRow::Reference).unwrap();
    assert_eq!((reference.ref_loops, reference.ref_length), (0, Some(Q)));
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test midi 2>&1 | tail -5; cargo test -p core-ffi --test input --test events 2>&1 | tail -10
```

Expected: compile errors (`NoteListener` not found; `unresolved import vizdaw_ffi::input`; no method `seconds_at`). `the_other_variants_round_trip_too` may already pass once it compiles; that is fine (it pins coverage).

- [ ] **Step 3: Implement the listener**

In `crates/platform-macos/src/midi.rs`, Find:

```rust
    MIDIEventList, MIDIEventPacket, MIDIGetDestination, MIDIGetNumberOfDestinations,
```

Replace:

```rust
    MIDIEventList, MIDIEventPacket, MIDIGetDestination, MIDIGetNumberOfDestinations,
    MIDIGetNumberOfSources, MIDIGetSource,
```

Find:

```rust
/// Nanoseconds to host-clock ticks (the timestamps CoreMIDI carries).
```

Replace:

```rust
/// What a [`NoteListener`] hands each received MIDI 1.0 message to (on
/// CoreMIDI's thread).
pub type MessageSink = Box<dyn Fn(&[u8]) + Send + Sync>;

/// Listens to every MIDI source present when it connects (plan 4c: notes
/// played into the list editor). It has its own client, so output is never
/// disturbed; sources added later are picked up by connecting again.
pub struct NoteListener {
    client: MIDIClientRef,
    port: MIDIPortRef,
    sources: u32,
    _block: RcBlock<dyn Fn(NonNull<MIDIEventList>, *mut c_void)>,
}

// SAFETY: the client and port are plain CoreMIDI object references, usable
// from any thread; the block is only called by CoreMIDI (its captures are
// `Send + Sync`) and is dropped with the listener after the port is
// disposed.
unsafe impl Send for NoteListener {}

impl NoteListener {
    /// Creates client `name` and connects it to every source present now.
    pub fn connect_all(name: &str, sink: MessageSink) -> Result<NoteListener, BackendError> {
        let mut client: MIDIClientRef = 0;
        let cf = CFString::from_str(name);
        // SAFETY: `client` is a valid out-pointer; no notification callback.
        os("MIDIClientCreate", unsafe {
            MIDIClientCreate(&cf, None, ptr::null_mut(), NonNull::from(&mut client))
        })?;
        let pending = Mutex::new(Vec::new());
        let block = RcBlock::new(move |list: NonNull<MIDIEventList>, _src: *mut c_void| {
            let mut out = Vec::new();
            if let Ok(mut sx) = pending.lock() {
                decode(list, &mut out, &mut sx);
            }
            for r in out {
                sink(&r.bytes);
            }
        });
        let mut port: MIDIPortRef = 0;
        let pcf = CFString::from_str("VizDAW Notes In");
        // SAFETY: valid client, out-pointer and block; CoreMIDI copies the
        // block, and ours lives as long as the port anyway.
        let made = os("MIDIInputPortCreateWithProtocol", unsafe {
            MIDIInputPortCreateWithProtocol(
                client,
                &pcf,
                MIDIProtocolID::Protocol_1_0,
                NonNull::from(&mut port),
                (&*block as *const block2::DynBlock<_>).cast_mut(),
            )
        });
        if let Err(e) = made {
            // SAFETY: the client was created above and is not used again.
            unsafe {
                MIDIClientDispose(client);
            }
            return Err(e);
        }
        // SAFETY: a pure query.
        let n = unsafe { MIDIGetNumberOfSources() };
        let mut sources = 0u32;
        for i in 0..n {
            // SAFETY: `i` is below the count just read.
            let src = unsafe { MIDIGetSource(i) };
            if src == 0 {
                continue;
            }
            // SAFETY: both objects are live; the port belongs to our client.
            let ok = os("MIDIPortConnectSource", unsafe {
                MIDIPortConnectSource(port, src, ptr::null_mut())
            });
            if ok.is_ok() {
                sources = sources.saturating_add(1);
            }
        }
        Ok(NoteListener {
            client,
            port,
            sources,
            _block: block,
        })
    }

    /// How many sources it listens to.
    pub fn sources(&self) -> u32 {
        self.sources
    }
}

impl Drop for NoteListener {
    fn drop(&mut self) {
        // SAFETY: the port and client belong to this listener and are not
        // used again.
        unsafe {
            MIDIPortDispose(self.port);
            MIDIClientDispose(self.client);
        }
    }
}

/// Nanoseconds to host-clock ticks (the timestamps CoreMIDI carries).
```

- [ ] **Step 4: Implement `NoteInput` and the time reads**

```bash
cat > crates/core-ffi/src/input.rs <<'VIZDAW_EOF'
//! MIDI note input (plan 4c, Plan Ruling 11): notes played on any MIDI
//! source, for the list editor's "play a note to enter it". Recording
//! proper is sub-project 5.

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A note-on played on a MIDI source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NoteInRow {
    /// Note number 0–127.
    pub pitch: u8,
    /// 16-bit velocity (MIDI 2.0 upscale of the 7-bit one).
    pub velocity: u16,
    /// MIDI channel 0–15.
    pub channel: u8,
}

/// How many note-ons are kept until taken (the newest win).
pub const CAPACITY: usize = 256;

type Queue = Arc<Mutex<VecDeque<NoteInRow>>>;

/// A MIDI 1.0 velocity as 16 bits by the MIDI 2.0 min-center-max rule.
pub fn upscale_velocity(v: u8) -> u16 {
    let v = u16::from(v & 0x7F);
    let shifted = v << 9;
    if v <= 64 {
        return shifted;
    }
    let rep = v & 0x3F;
    shifted | (rep << 3) | (rep >> 3)
}

/// The note-on in a MIDI 1.0 message (velocity 0 is a note-off).
pub fn note_on(bytes: &[u8]) -> Option<NoteInRow> {
    match bytes {
        [s, p, v] if s & 0xF0 == 0x90 && *v > 0 => Some(NoteInRow {
            pitch: p & 0x7F,
            velocity: upscale_velocity(*v),
            channel: s & 0x0F,
        }),
        _ => None,
    }
}

fn push(q: &Queue, bytes: &[u8]) {
    if let Some(n) = note_on(bytes) {
        let mut q = lock(q);
        if q.len() >= CAPACITY {
            q.pop_front();
        }
        q.push_back(n);
    }
}

/// Notes played on MIDI sources (the app makes one).
#[derive(uniffi::Object)]
pub struct NoteInput {
    queue: Queue,
    #[cfg(target_os = "macos")]
    listener: Mutex<Option<platform_macos::midi::NoteListener>>,
}

#[cfg(target_os = "macos")]
fn listen(input: &NoteInput) -> Result<u32, VizError> {
    // Drop the old connection first: one client at a time.
    *lock(&input.listener) = None;
    let q = Arc::clone(&input.queue);
    let l = platform_macos::midi::NoteListener::connect_all(
        "VizDAW Note Input",
        Box::new(move |b: &[u8]| push(&q, b)),
    )?;
    let n = l.sources();
    *lock(&input.listener) = Some(l);
    Ok(n)
}

#[cfg(not(target_os = "macos"))]
fn listen(_: &NoteInput) -> Result<u32, VizError> {
    Ok(0)
}

#[cfg(target_os = "macos")]
fn unlisten(input: &NoteInput) {
    *lock(&input.listener) = None;
}

#[cfg(not(target_os = "macos"))]
fn unlisten(_: &NoteInput) {}

#[uniffi::export]
impl NoteInput {
    /// Not listening yet.
    #[uniffi::constructor]
    pub fn new() -> Arc<NoteInput> {
        Arc::new(NoteInput {
            queue: Arc::default(),
            #[cfg(target_os = "macos")]
            listener: Mutex::new(None),
        })
    }

    /// Listens to every MIDI source present now (again: picks up new
    /// ones); returns how many (0 off macOS).
    pub fn start(&self) -> Result<u32, VizError> {
        ffi_guard(|| listen(self))
    }

    /// Stops listening; queued notes stay until taken.
    pub fn stop(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            unlisten(self);
            Ok(())
        })
    }

    /// The note-ons since the last call, oldest first.
    pub fn take(&self) -> Result<Vec<NoteInRow>, VizError> {
        ffi_guard(|| Ok(lock(&self.queue).drain(..).collect()))
    }
}

/// Test hooks.
#[cfg(feature = "test-hooks")]
#[uniffi::export]
impl NoteInput {
    /// Handles `bytes` as if a source sent them.
    pub fn inject(&self, bytes: Vec<u8>) -> Result<(), VizError> {
        ffi_guard(|| {
            push(&self.queue, &bytes);
            Ok(())
        })
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/src/session.rs`, Find:

```rust
    /// The select-and-modify templates, by name (4c).
```

Replace:

```rust
    /// Seconds from tick 0 of sequence `seq` at each tick, through its own
    /// tempo map (the list's SMPTE column, 4c).
    pub fn seconds_at(&self, seq: u64, ticks: Vec<i64>) -> Result<Vec<f64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let tempo = &inner.doc().sequence(SeqId(seq))?.tempo;
            Ok(ticks
                .into_iter()
                .map(|t| tempo.tick_to_seconds(core_model::time::Tick(t)))
                .collect())
        })
    }

    /// The nearest tick at each time in seconds (typed SMPTE positions, 4c).
    pub fn ticks_at(&self, seq: u64, seconds: Vec<f64>) -> Result<Vec<i64>, VizError> {
        ffi_guard(|| {
            let inner = lock(&self.inner);
            let tempo = &inner.doc().sequence(SeqId(seq))?.tempo;
            Ok(seconds
                .into_iter()
                .map(|s| tempo.seconds_to_tick(s).0)
                .collect())
        })
    }

    /// The select-and-modify templates, by name (4c).
```

In `crates/core-ffi/src/lib.rs`, Find:

```rust
pub mod host;
```

Replace:

```rust
pub mod host;
pub mod input;
```

Find:

```rust
pub use guard::ffi_guard;
```

Replace:

```rust
pub use guard::ffi_guard;
pub use input::{NoteInRow, NoteInput};
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`, next to Task 4's stubs, add (Task 6 replaces them):

```swift
    func secondsAt(seq: UInt64, ticks: [Int64]) throws -> [Double] { ticks.map { Double($0) / 30_720 } }
    func ticksAt(seq: UInt64, seconds: [Double]) throws -> [Int64] { seconds.map { Int64(($0 * 30_720).rounded()) } }
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test midi 2>&1 | tail -4; cargo test -p core-ffi --test input --test events 2>&1 | tail -6
```

Expected: PASS. If `inner.doc()` is not reachable through the guard type, read the document the way `sequence_info` does (`crate::events::sequence_info(lock(&self.inner).doc(), …)`).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | tail -40
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
git add crates/platform-macos/src/midi.rs crates/platform-macos/tests/midi.rs crates/core-ffi/src/input.rs crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs crates/core-ffi/tests/input.rs crates/core-ffi/tests/events.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "feat(ffi): MIDI note input, seconds and ticks for SMPTE; cover the remaining row variants

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

Expected: 660 Rust tests pass (653 + 7); 341 Swift tests pass.

---
### Task 6: App plumbing — fake session, text-editing owners, panel focus, Return key, modify reads

**Files:**
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`
- Create: `app/Sources/VizDAWAppModel/EditorModel+Modify.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentStore.swift`
- Modify: `app/Sources/VizDAWEditors/LaneHeaderView.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`
- Test: `app/Tests/VizDAWAppModelTests/ModifyReadTests.swift` (new)
- Test: `app/Tests/VizDAWAppModelTests/DocumentModelTests.swift`
- Test: `app/Tests/VizDAWAppModelTests/TornFocusTests.swift`

**Interfaces:**
- Consumes: Task 4's `Session.modifyTemplates()` / `lastModify()`, Task 5's `secondsAt(seq:ticks:)` / `ticksAt(seq:seconds:)`.
- Produces: `EditorModel.textEditors: Set<String>` (read-only), `EditorModel.textEditing: Bool` (derived), `beginTextEditing(_ owner: String)`, `endTextEditing(_ owner: String)`; `EditorModel.panelKey: Bool`, `EditorModel.listFocused: Bool`; `EditorModel.lastModify() -> ModifyRow?`, `EditorModel.modifyTemplates() -> [ModifyTemplateRow]` (read once per snapshot version; failures keep the last values); `DocumentModel.returnKeyEnabled: Bool`; `DocumentStore.panelFocus: UUID?`, `focusPanel(_ doc: UUID)`, `blurPanel(_ doc: UUID)`; `commandTarget(focused:)` prefers a key torn window, then a key panel, then the focused window. `FakeSession.{lastModifyRow, modifyTemplateRows, modifyReadError, modifyReads}`.

Rulings 20, 21, 22. Lane name editing uses the owner `"laneName"`; the list uses `"list"` (Task 10).

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/ModifyReadTests.swift`:

```swift
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct ModifyReadTests {
    @Test func modifyReadsHappenOncePerSnapshotVersion() throws {
        let f = editorFake()
        f.lastModifyRow = .move(delta: 5)
        let m = try DocumentModel(session: f)
        #expect(m.editor.lastModify() == .move(delta: 5))
        #expect(m.editor.lastModify() == .move(delta: 5))
        #expect(f.modifyReads == 1)
        f.lastModifyRow = .delete
        m.editor.edit(.event(edit: .delete(events: [1])))
        #expect(m.editor.lastModify() == .delete)
        #expect(f.modifyReads == 2)
    }

    @Test func aFailedReadKeepsTheLastValues() throws {
        let f = editorFake()
        f.modifyTemplateRows = [
            ModifyTemplateRow(name: "Up", rule: nil, mode: .select, modify: .transposeInterval(semitones: 12), toCopy: false)
        ]
        let m = try DocumentModel(session: f)
        #expect(m.editor.modifyTemplates().map(\.name) == ["Up"])
        f.modifyReadError = .Busy(message: "busy")
        m.editor.edit(.event(edit: .delete(events: [1])))
        #expect(m.editor.modifyTemplates().map(\.name) == ["Up"])
        #expect(m.editor.status == nil, "reads stay silent")
    }

    @Test func laneNameAndListCellsDoNotClearEachOther() throws {
        let m = try DocumentModel(session: demoFake())
        m.editor.beginTextEditing("laneName")
        m.editor.beginTextEditing("list")
        m.editor.endTextEditing("laneName")
        #expect(m.editor.textEditing, "the list cell still has the keyboard")
        #expect(!m.transportKeysEnabled)
        m.editor.endTextEditing("list")
        #expect(!m.editor.textEditing)
        #expect(m.transportKeysEnabled)
    }

    @Test func transportKeysAreOffWhileThePanelIsKey() throws {
        let m = try DocumentModel(session: demoFake())
        m.editor.panelKey = true
        #expect(!m.transportKeysEnabled)
        #expect(!m.returnKeyEnabled)
        m.editor.panelKey = false
        #expect(m.transportKeysEnabled)
    }

    @Test func returnIsOffWhileAListHasFocusButSpaceIsNot() throws {
        let m = try DocumentModel(session: demoFake())
        #expect(m.returnKeyEnabled)
        m.editor.listFocused = true
        #expect(!m.returnKeyEnabled)
        #expect(m.transportKeysEnabled, "Space still plays")
    }
}
```

In `app/Tests/VizDAWAppModelTests/DocumentModelTests.swift`, Find:

```swift
        m.editor.textEditing = true
        #expect(!m.transportKeysEnabled)
        m.editor.textEditing = false
```

Replace:

```swift
        m.editor.beginTextEditing("test")
        #expect(!m.transportKeysEnabled)
        m.editor.endTextEditing("test")
```

Append to `TornFocusTests` (before its closing brace) in `app/Tests/VizDAWAppModelTests/TornFocusTests.swift`:

```swift
    @Test func aKeyPanelIsTheCommandTargetAfterATornWindow() throws {
        let s = store()
        let a = try #require(s.newEmpty())
        let b = try #require(s.newEmpty())
        let c = try #require(s.newEmpty())
        s.focusPanel(a)
        #expect(s.commandTarget(focused: b) == a, "a's panel is key")
        let w = UUID()
        s.focusTorn(TornFocus(doc: c, window: w))
        #expect(s.commandTarget(focused: b) == c, "a torn window wins")
        s.blurTorn(window: w)
        s.blurPanel(b)
        #expect(s.commandTarget(focused: b) == a, "blurring another panel keeps it")
        s.blurPanel(a)
        #expect(s.commandTarget(focused: b) == b)
        s.focusPanel(a)
        s.close(a)
        #expect(s.panelFocus == nil)
    }
```

- [ ] **Step 2: Replace the fake session's stubs**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`, Find (the stubs Tasks 4 and 5 added):

```swift
    func modifyTemplates() throws -> [ModifyTemplateRow] { [] }
    func lastModify() throws -> ModifyRow? { nil }
```

Replace:

```swift
    // MARK: Select-and-modify and the list (plan 4c)

    var lastModifyRow: ModifyRow?
    var modifyTemplateRows: [ModifyTemplateRow] = []
    /// Thrown by `lastModify()` and `modifyTemplates()`.
    var modifyReadError: VizError?
    var modifyReads = 0
    func lastModify() throws -> ModifyRow? {
        modifyReads += 1
        if let e = modifyReadError { throw e }
        return lastModifyRow
    }
    func modifyTemplates() throws -> [ModifyTemplateRow] {
        if let e = modifyReadError { throw e }
        return modifyTemplateRows
    }
```

(The `secondsAt` / `ticksAt` lines from Task 5 stay: 120 BPM, 30 720 ticks a second.)

- [ ] **Step 3: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:|failed|passed" | head -20
```

Expected: compile errors (`value of type 'EditorModel' has no member 'beginTextEditing'`, `'panelKey'`, `'focusPanel'`).

- [ ] **Step 4: Implement**

In `app/Sources/VizDAWAppModel/EditorModel.swift`, Find:

```swift
    /// An editor text field (a lane name) has the keyboard.
    public var textEditing = false
```

Replace:

```swift
    /// The editor text fields that have the keyboard, by owner ("laneName",
    /// "list"); transport keys are off while any has it (Plan 4c Ruling 21).
    public private(set) var textEditors: Set<String> = []
    public var textEditing: Bool { !textEditors.isEmpty }
    /// This document's select-and-modify panel is the key window (Plan 4c
    /// Ruling 22).
    public var panelKey = false
    /// A list editor's table has the keyboard: Return edits instead of
    /// returning to the start (Plan 4c Ruling 20).
    public var listFocused = false

    public func beginTextEditing(_ owner: String) { textEditors.insert(owner) }

    public func endTextEditing(_ owner: String) { textEditors.remove(owner) }
```

Find:

```swift
    @ObservationIgnored private var selectedEvents: Set<UInt64> = []
```

Replace:

```swift
    @ObservationIgnored private var selectedEvents: Set<UInt64> = []
    /// Templates and the last modification at a snapshot version (plan 4c).
    @ObservationIgnored var modifyCache: ModifyReads?
```

Create `app/Sources/VizDAWAppModel/EditorModel+Modify.swift`:

```bash
cat > app/Sources/VizDAWAppModel/EditorModel+Modify.swift <<'VIZDAW_EOF'
import VizDAWCore

/// What Modify Again repeats and the saved templates, at one snapshot
/// version.
struct ModifyReads {
    let version: UInt64
    let last: ModifyRow?
    let templates: [ModifyTemplateRow]
}

/// Templates and Modify Again (plan 4c), read once per snapshot version for
/// the menus; a failed read (Busy) keeps the last values, silently.
extension EditorModel {
    /// The modification Modify Again repeats, if any.
    public func lastModify() -> ModifyRow? { modifyReads().last }

    /// The saved select-and-modify templates, by name.
    public func modifyTemplates() -> [ModifyTemplateRow] { modifyReads().templates }

    func modifyReads() -> ModifyReads {
        if let c = modifyCache, c.version == snapshotVersion { return c }
        do {
            let c = ModifyReads(
                version: snapshotVersion, last: try session.lastModify(), templates: try session.modifyTemplates())
            modifyCache = c
            return c
        } catch {
            return modifyCache ?? ModifyReads(version: snapshotVersion, last: nil, templates: [])
        }
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`, Find:

```swift
    public var transportKeysEnabled: Bool {
        renamingSeq == nil && commentDraft == nil && slotDraft == nil
            && editor.referenceDraft == nil && !editor.textEditing
    }
```

Replace:

```swift
    public var transportKeysEnabled: Bool {
        renamingSeq == nil && commentDraft == nil && slotDraft == nil
            && editor.referenceDraft == nil && !editor.textEditing && !editor.panelKey
    }
    /// Return (Return to Start) is also off while a list editor has the
    /// keyboard: there it edits the cursor row (Plan 4c Ruling 20).
    public var returnKeyEnabled: Bool { transportKeysEnabled && !editor.listFocused }
```

In `app/Sources/VizDAWAppModel/DocumentStore.swift`, Find:

```swift
    /// The document menu commands act on: a key torn-off window's, else the
    /// focused document window's.
    public func commandTarget(focused: UUID?) -> UUID? {
        tornFocus.map(\.doc) ?? focused
    }
```

Replace:

```swift
    /// The document whose select-and-modify panel is key (Plan 4c Ruling 22).
    public private(set) var panelFocus: UUID?

    public func focusPanel(_ doc: UUID) { panelFocus = doc }

    /// Document `doc`'s panel resigned key or closed.
    public func blurPanel(_ doc: UUID) {
        if panelFocus == doc { panelFocus = nil }
    }

    /// The document menu commands act on: a key torn-off window's, else a
    /// key select-and-modify panel's, else the focused document window's.
    public func commandTarget(focused: UUID?) -> UUID? {
        tornFocus.map(\.doc) ?? panelFocus ?? focused
    }
```

Find:

```swift
        if tornFocus?.doc == id { tornFocus = nil }
```

Replace:

```swift
        if tornFocus?.doc == id { tornFocus = nil }
        if panelFocus == id { panelFocus = nil }
```

In `app/Sources/VizDAWEditors/LaneHeaderView.swift`, Find:

```swift
        renaming = (seq, lane.id, lane.name, field)
        editor.textEditing = true
```

Replace:

```swift
        renaming = (seq, lane.id, lane.name, field)
        editor.beginTextEditing("laneName")
```

Find:

```swift
        renaming = nil
        editor.textEditing = false
        let name = r.field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
```

Replace:

```swift
        renaming = nil
        editor.endTextEditing("laneName")
        let name = r.field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
```

Find:

```swift
        if window == nil, renaming != nil {
            renaming = nil
            editor.textEditing = false
        }
```

Replace:

```swift
        if window == nil, renaming != nil {
            renaming = nil
            editor.endTextEditing("laneName")
        }
```

In `app/Sources/VizDAW/AppCommands.swift`, Find:

```swift
    /// A key torn-off window's document first (Plan 4b Ruling 18).
    private var doc: DocumentModel? { store.tornFocus.flatMap { store.model($0.doc) } ?? focusedDoc }
```

Replace:

```swift
    /// A key torn-off window's document first (Plan 4b Ruling 18), then a key
    /// select-and-modify panel's (Plan 4c Ruling 22).
    private var doc: DocumentModel? {
        store.tornFocus.flatMap { store.model($0.doc) } ?? store.panelFocus.flatMap { store.model($0) }
            ?? focusedDoc
    }
```

Find:

```swift
            Button("Return to Start") { doc?.returnToStart() }
                .keyboardShortcut(.return, modifiers: [])
                .disabled(!(doc?.transportKeysEnabled ?? false))
```

Replace:

```swift
            Button("Return to Start") { doc?.returnToStart() }
                .keyboardShortcut(.return, modifiers: [])
                .disabled(!(doc?.returnKeyEnabled ?? false))
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```

Expected: build with no new warnings; "Test run with 347 tests … passed" (341 + 6).

- [ ] **Step 6: Gate and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "FAILED|panicked" ; scripts/swift.sh test 2>&1 | tail -2
git add app/Tests/VizDAWAppModelTests/FakeSession.swift app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/EditorModel+Modify.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/DocumentStore.swift app/Sources/VizDAWEditors/LaneHeaderView.swift app/Sources/VizDAW/AppCommands.swift app/Tests/VizDAWAppModelTests/ModifyReadTests.swift app/Tests/VizDAWAppModelTests/DocumentModelTests.swift app/Tests/VizDAWAppModelTests/TornFocusTests.swift
git commit -m "feat(app): text-editing owners, panel focus, Return key rule and modify reads

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 7: Positions, lengths, SMPTE, velocities and key names as text

**Files:**
- Create: `app/Sources/VizDAWAppModel/ListFormat.swift`
- Test: `app/Tests/VizDAWAppModelTests/ListFormatTests.swift` (new)

**Interfaces:**
- Consumes: `MeterMath.{bar(at:_:), barStart(_:_:), beatTicks(_:), ppq}`, `PitchName`.
- Produces: `public struct Bbu: Equatable, Sendable { bar, beat, unit: Int64; static unitsPerBeat = 960; static bars, beats, units: ClosedRange<Int64>; static func of(_ tick: Int64, meter: [MeterEntryRow]) -> Bbu; func tick(meter:) -> Int64; var text: String }`; `enum PositionInput { static func parse(_ text: String, current: Bbu) -> Bbu? }`; `enum DurationText { static func text(_ ticks: Int64, beat: Int64) -> String; static func parse(_ text: String, current: Int64, beat: Int64) -> Int64? }`; `enum Smpte { static let rates = [24, 25, 30]; static func text(seconds: Double, fps: Int) -> String; static func parse(_ text: String, fps: Int) -> Double? }`; `enum Velocity7 { static func of(_ v: UInt16) -> Int; static func up(_ v: Int) -> UInt16 }`; `enum Value7 { static func of(_ v: Int64) -> Int; static func up(_ v: Int) -> UInt32 }`; `enum Bend14 { static func of(_ v: Int64) -> Int; static func up(_ signed: Int) -> UInt32 }`; `extension PitchName { static func parse(_ text: String) -> UInt8? }`; `enum KeyName { static func name(sharps: Int8, minor: Bool) -> String; static func parse(_ text: String) -> (sharps: Int8, minor: Bool)? }`.

Rulings 14, 15, 16. `Bbu` matches the core's `tick_to_bbu` / `bbu_to_tick` with 960 units (the transport's `positionBbu`). The 7-, 14- and 32-bit upscales are the MIDI 2.0 min-center-max rule (`Value7.up(127) == 0xFFFF_FFFF`, `Bend14.up(0) == 0x8000_0000`).

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/ListFormatTests.swift`:

```swift
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
private let fourFour = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

@Suite struct ListFormatTests {
    @Test func positionsReadLikeTheTransport() {
        #expect(Bbu.of(0, meter: fourFour).text == "1.1.0")
        #expect(Bbu.of(q, meter: fourFour).text == "1.2.0")
        #expect(Bbu.of(4 * q + q / 2, meter: fourFour).text == "2.1.480")
        #expect(Bbu.of(-q, meter: fourFour).text == "0.4.0", "a pickup")
        let sixEight = fourFour + [MeterEntryRow(tick: 4 * q, num: 6, den: 8, sharps: 0, minor: false)]
        #expect(Bbu.of(4 * q + 3 * (q / 2), meter: sixEight).text == "2.4.0")
        #expect(Bbu(bar: 2, beat: 1, unit: 480).tick(meter: fourFour) == 4 * q + q / 2)
        for t: Int64 in [0, 1, q - 1, 7 * q + 123, -5 * q] {
            let b = Bbu.of(t, meter: fourFour)
            let back = b.tick(meter: fourFour)
            #expect(back <= t && Bbu.of(back, meter: fourFour) == b, "\(t)")
        }
    }

    @Test func typedPositionsKeepTheSubFieldsLeftEmpty() {
        let cur = Bbu(bar: 3, beat: 2, unit: 240)
        #expect(PositionInput.parse("5", current: cur) == Bbu(bar: 5, beat: 1, unit: 0), "bar number: downbeat")
        #expect(PositionInput.parse("5.3", current: cur) == Bbu(bar: 5, beat: 3, unit: 0))
        #expect(PositionInput.parse("5.3.480", current: cur) == Bbu(bar: 5, beat: 3, unit: 480))
        #expect(PositionInput.parse(".4", current: cur) == Bbu(bar: 3, beat: 4, unit: 240), "beat only")
        #expect(PositionInput.parse("..480", current: cur) == Bbu(bar: 3, beat: 2, unit: 480), "unit only")
        #expect(PositionInput.parse("7..", current: cur) == Bbu(bar: 7, beat: 2, unit: 240), "bar only")
        #expect(PositionInput.parse(" 6 ", current: cur) == Bbu(bar: 6, beat: 1, unit: 0))
        #expect(PositionInput.parse("-1", current: cur) == Bbu(bar: -1, beat: 1, unit: 0))
    }

    @Test func hugeAndBadPositionsAreRefused() {
        let cur = Bbu(bar: 1, beat: 1, unit: 0)
        for bad in ["", "x", "5.0", "5.1.960", "1.2.3.4", "99999999999999999999", "1.1.99999999999", "1000001", "5.1000"] {
            #expect(PositionInput.parse(bad, current: cur) == nil, "\(bad)")
        }
        let far = Bbu(bar: Bbu.bars.upperBound, beat: Bbu.beats.upperBound, unit: Bbu.units.upperBound)
        #expect(far.tick(meter: fourFour) > 0, "the largest typed position does not trap")
        #expect(DurationText.parse("99999999999", current: 0, beat: q) == nil)
    }

    @Test func lengthsAreBeatsAndUnits() {
        #expect(DurationText.text(q, beat: q) == "1.0")
        #expect(DurationText.text(q + q / 4, beat: q) == "1.240")
        #expect(DurationText.text(0, beat: q) == "0.0")
        #expect(DurationText.parse("2", current: q, beat: q) == 2 * q)
        #expect(DurationText.parse(".480", current: q, beat: q) == q + q / 2, "units only")
        #expect(DurationText.parse("1.5", current: 0, beat: q) == q + 80)
        #expect(DurationText.parse("a", current: 0, beat: q) == nil)
        #expect(DurationText.parse("-1", current: 0, beat: q) == nil)
    }

    @Test func smpteRoundTrips() {
        #expect(Smpte.text(seconds: 3661.5, fps: 25) == "01:01:01:12")
        #expect(Smpte.text(seconds: -0.5, fps: 25) == "-00:00:00:12")
        #expect(Smpte.text(seconds: .nan, fps: 30) == "--:--:--:--")
        #expect(abs(Smpte.parse("01:01:01:12", fps: 25)! - 3661.48) < 1e-9)
        #expect(abs(Smpte.parse("1:12", fps: 25)! - 1.48) < 1e-9, "fields count from the right")
        #expect(Smpte.parse("-0:12", fps: 24) == -0.5)
        #expect(Smpte.parse("00:00:00:25", fps: 25) == nil, "frame out of range")
        #expect(Smpte.parse("1:60:00:00", fps: 25) == nil)
        #expect(Smpte.parse("x", fps: 25) == nil)
    }

    @Test func sevenBitValuesUpscaleByTheMidi2Rule() {
        #expect(Velocity7.up(100) == 0xC924)
        #expect(Velocity7.up(127) == 0xFFFF)
        #expect(Velocity7.up(64) == 0x8000)
        #expect(Velocity7.up(200) == 0xFFFF && Velocity7.up(-5) == 0)
        #expect(Velocity7.of(0xC924) == 100)
        #expect(Value7.up(127) == 0xFFFF_FFFF && Value7.up(64) == 0x8000_0000)
        #expect(Value7.of(Int64(Value7.up(100))) == 100)
        #expect(Bend14.up(0) == 0x8000_0000 && Bend14.up(8191) == 0xFFFF_FFFF && Bend14.up(-8192) == 0)
        #expect(Bend14.of(Int64(Bend14.up(-100))) == -100)
    }

    @Test func pitchAndKeyNamesParse() {
        #expect(PitchName.parse("C4") == 60)
        #expect(PitchName.parse("c#4") == 61)
        #expect(PitchName.parse("Db4") == 61)
        #expect(PitchName.parse("bb3") == 58)
        #expect(PitchName.parse("C-1") == 0)
        #expect(PitchName.parse("G9") == 127)
        #expect(PitchName.parse("G#9") == nil)
        #expect(PitchName.parse("64") == 64)
        #expect(PitchName.parse("H2") == nil && PitchName.parse("C") == nil && PitchName.parse("200") == nil)
        #expect(KeyName.name(sharps: 2, minor: false) == "D major")
        #expect(KeyName.name(sharps: -3, minor: true) == "C minor")
        #expect(KeyName.parse("D major")! == (2, false))
        #expect(KeyName.parse("c minor")! == (-3, true))
        #expect(KeyName.parse("F# minor")! == (3, true))
        #expect(KeyName.parse("Gb major")! == (-6, false))
        #expect(KeyName.parse("H major") == nil && KeyName.parse("C") == nil)
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:" | head -5
```

Expected: `cannot find 'Bbu' in scope`.

- [ ] **Step 3: Implement**

```bash
cat > app/Sources/VizDAWAppModel/ListFormat.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// A position as bar·beat·unit, 960 units per beat as the transport shows it;
/// pickup bars count down from 0 (Plan 4c Ruling 15).
public struct Bbu: Equatable, Sendable {
    public var bar: Int64
    public var beat: Int64
    public var unit: Int64

    public static let unitsPerBeat: Int64 = 960
    /// Typed values outside these are refused.
    public static let bars: ClosedRange<Int64> = -1_000_000...1_000_000
    public static let beats: ClosedRange<Int64> = 1...999
    public static let units: ClosedRange<Int64> = 0...959

    public init(bar: Int64, beat: Int64, unit: Int64) {
        self.bar = bar
        self.beat = beat
        self.unit = unit
    }

    /// The position of `tick` (the core's `tick_to_bbu`).
    public static func of(_ tick: Int64, meter: [MeterEntryRow]) -> Bbu {
        let b = MeterMath.bar(at: tick, meter)
        let beatT = MeterMath.beatTicks(b.meter)
        let inBar = tick - b.start
        return Bbu(bar: b.number, beat: inBar / beatT + 1, unit: (inBar % beatT) * unitsPerBeat / beatT)
    }

    /// The first tick of this position (the core's `bbu_to_tick`).
    public func tick(meter: [MeterEntryRow]) -> Int64 {
        let start = MeterMath.barStart(bar, meter)
        let beatT = MeterMath.beatTicks(MeterMath.bar(at: start, meter).meter)
        let units = (unit * beatT + Self.unitsPerBeat - 1) / Self.unitsPerBeat
        return start + (max(beat, 1) - 1) * beatT + units
    }

    public var text: String { "\(bar).\(beat).\(unit)" }
}

/// Typed positions (Plan 4c Ruling 15): "5" is bar 5's downbeat, "5.3" and
/// "5.3.480" name more; an empty part keeps that sub-field of the current
/// position (".3" the beat only, "..480" the unit only, "5.." the bar only).
public enum PositionInput {
    public static func parse(_ text: String, current: Bbu) -> Bbu? {
        let s = text.trimmingCharacters(in: .whitespaces)
        guard !s.isEmpty else { return nil }
        let parts = s.split(separator: ".", omittingEmptySubsequences: false).map(String.init)
        guard parts.count <= 3 else { return nil }
        let subField = parts[0].isEmpty
        var out = subField ? current : Bbu(bar: current.bar, beat: 1, unit: 0)
        for (i, p) in parts.enumerated() {
            if p.isEmpty {
                if !subField, i == 1 { out.beat = current.beat }
                if !subField, i == 2 { out.unit = current.unit }
                continue
            }
            guard let v = Int64(p) else { return nil }
            switch i {
            case 0: out.bar = v
            case 1: out.beat = v
            default: out.unit = v
            }
        }
        guard Bbu.bars.contains(out.bar), Bbu.beats.contains(out.beat), Bbu.units.contains(out.unit) else {
            return nil
        }
        return out
    }
}

/// Lengths as beats.units of a beat of `beat` ticks (Plan 4c Ruling 15).
public enum DurationText {
    public static let maxBeats: Int64 = 1_000_000

    public static func text(_ ticks: Int64, beat: Int64) -> String {
        let b = max(beat, 1)
        return "\(ticks / b).\((ticks % b) * Bbu.unitsPerBeat / b)"
    }

    /// The ticks `text` means; an empty part keeps `current`'s; nil unless a
    /// length of 0 or more.
    public static func parse(_ text: String, current: Int64, beat: Int64) -> Int64? {
        let b = max(beat, 1)
        let s = text.trimmingCharacters(in: .whitespaces)
        guard !s.isEmpty else { return nil }
        let parts = s.split(separator: ".", omittingEmptySubsequences: false).map(String.init)
        guard parts.count <= 2 else { return nil }
        var beats = max(current, 0) / b
        var units = (max(current, 0) % b) * Bbu.unitsPerBeat / b
        if !parts[0].isEmpty {
            guard let v = Int64(parts[0]), (0...maxBeats).contains(v) else { return nil }
            beats = v
            if parts.count == 1 { units = 0 }
        }
        if parts.count == 2, !parts[1].isEmpty {
            guard let u = Int64(parts[1]), Bbu.units.contains(u) else { return nil }
            units = u
        }
        return beats * b + (units * b + Bbu.unitsPerBeat - 1) / Bbu.unitsPerBeat
    }
}

/// `hh:mm:ss:ff` (Plan 4c Ruling 14).
public enum Smpte {
    public static let rates = [24, 25, 30]

    public static func text(seconds: Double, fps: Int) -> String {
        guard seconds.isFinite else { return "--:--:--:--" }
        let f = Int64(max(fps, 1))
        let frames = Int64(min((abs(seconds) * Double(f)).rounded(.down), 9e15))
        let ff = frames % f
        let total = frames / f
        let sign = seconds < 0 && frames > 0 ? "-" : ""
        return sign + String(format: "%02lld:%02lld:%02lld:%02lld", total / 3600, (total / 60) % 60, total % 60, ff)
    }

    /// Seconds of a typed SMPTE time; fewer fields count from the right
    /// ("12" is 12 frames, "1:12" one second and 12 frames).
    public static func parse(_ text: String, fps: Int) -> Double? {
        var s = Substring(text.trimmingCharacters(in: .whitespaces))
        let negative = s.hasPrefix("-")
        if negative { s = s.dropFirst() }
        let parts = s.split(separator: ":", omittingEmptySubsequences: false)
        guard (1...4).contains(parts.count) else { return nil }
        var values: [Int64] = []
        for p in parts {
            guard let v = Int64(p), (0...1_000_000).contains(v) else { return nil }
            values.append(v)
        }
        let f = Int64(max(fps, 1))
        let v = Array(repeating: Int64(0), count: 4 - values.count) + values
        guard v[3] < f, values.count < 3 || v[2] < 60, values.count < 4 || v[1] < 60 else { return nil }
        let secs = Double(v[0] * 3600 + v[1] * 60 + v[2]) + Double(v[3]) / Double(f)
        return negative ? -secs : secs
    }
}

/// 16-bit velocities shown and typed as MIDI 1.0 values (Plan 4c Ruling 16).
public enum Velocity7 {
    public static func of(_ v: UInt16) -> Int { Int(v >> 9) }

    public static func up(_ v: Int) -> UInt16 {
        let c = UInt16(min(max(v, 0), 127))
        let shifted = c << 9
        guard c > 64 else { return shifted }
        let rep = c & 0x3F
        return shifted | (rep << 3) | (rep >> 3)
    }
}

/// 32-bit controller and pressure values shown and typed as 0–127.
public enum Value7 {
    public static func of(_ v: Int64) -> Int { Int(UInt32(clamping: v) >> 25) }

    public static func up(_ v: Int) -> UInt32 {
        let c = UInt32(min(max(v, 0), 127))
        let shifted = c << 25
        guard c > 64 else { return shifted }
        let rep = c & 0x3F
        return shifted | (rep << 19) | (rep << 13) | (rep << 7) | (rep << 1) | (rep >> 5)
    }
}

/// 32-bit pitch bend shown and typed as −8192…8191.
public enum Bend14 {
    public static func of(_ v: Int64) -> Int { Int(UInt32(clamping: v) >> 18) - 8192 }

    public static func up(_ signed: Int) -> UInt32 {
        let u = UInt32(min(max(signed + 8192, 0), 16_383))
        let shifted = u << 18
        guard u > 8192 else { return shifted }
        let rep = u & 0x1FFF
        return shifted | (rep << 5) | (rep >> 8)
    }
}

extension PitchName {
    /// "C4", "c#4", "Db4", "C-1", or a number 0–127.
    public static func parse(_ text: String) -> UInt8? {
        let s = text.trimmingCharacters(in: .whitespaces)
        if let n = Int(s) { return (0...127).contains(n) ? UInt8(n) : nil }
        let bases: [String: Int] = ["C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11]
        guard let first = s.first, let base = bases[String(first).uppercased()] else { return nil }
        var rest = s.dropFirst()
        var pc = base
        if let a = rest.first, a == "#" || a == "♯" {
            pc += 1
            rest = rest.dropFirst()
        } else if let a = rest.first, a == "b" || a == "♭" {
            pc -= 1
            rest = rest.dropFirst()
        }
        guard let octave = Int(rest) else { return nil }
        let n = (octave + 1) * 12 + pc
        return (0...127).contains(n) ? UInt8(n) : nil
    }
}

/// Key signatures by name ("D major", "F# minor").
public enum KeyName {
    /// By sharps −7…7.
    static let majors = ["Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#"]
    static let minors = ["Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#"]

    public static func name(sharps: Int8, minor: Bool) -> String {
        let i = Int(sharps) + 7
        guard (0..<15).contains(i) else { return "\(sharps) sharps" }
        return minor ? "\(minors[i]) minor" : "\(majors[i]) major"
    }

    public static func parse(_ text: String) -> (sharps: Int8, minor: Bool)? {
        let words = text.trimmingCharacters(in: .whitespaces).split(separator: " ")
        guard words.count == 2, let f = words[0].first else { return nil }
        let isMinor: Bool
        switch words[1].lowercased() {
        case "major": isMinor = false
        case "minor": isMinor = true
        default: return nil
        }
        let root = String(f).uppercased() + words[0].dropFirst().lowercased()
        guard let i = (isMinor ? minors : majors).firstIndex(of: root) else { return nil }
        return (Int8(i - 7), isMinor)
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | tail -3
```

Expected: 354 tests pass (347 + 7).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -c "warning:" ; git add app/Sources/VizDAWAppModel/ListFormat.swift app/Tests/VizDAWAppModelTests/ListFormatTests.swift
git commit -m "feat(app): list text for positions, lengths, SMPTE, values and key names

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 8: List rows, cell text and cell edits (pure)

**Files:**
- Create: `app/Sources/VizDAWAppModel/ListRows.swift`
- Create: `app/Sources/VizDAWAppModel/ListEdit.swift`
- Test: `app/Tests/VizDAWAppModelTests/ListRowsTests.swift` (new)
- Test: `app/Tests/VizDAWAppModelTests/ListEditTests.swift` (new)

**Interfaces:**
- Consumes: Task 7 (`Bbu`, `PositionInput`, `DurationText`, `Smpte`, `Velocity7`, `Value7`, `Bend14`, `PitchName.parse`, `KeyName`), `MeterMath`, `LaneKey`.
- Produces:
  - `enum ListKind: String, Codable, CaseIterable, Hashable, Sendable` (`note, cc, pitchBend, channelPressure, polyPressure, program, sysEx, text, reference, audioRegion, tempo, meter, marker`) with `init(_ k: EventKindRow)`, `eventKind: EventKindRow?`, `isMidi`, `title`, `static insertable: [ListKind]`.
  - `struct ListFilter: Codable, Hashable, Sendable { hiddenKinds: Set<ListKind>; shownDests: Set<UInt64>?; func shows(_ kind: ListKind, dest: UInt64?) -> Bool; var viewFilter: ViewFilterRow? }`.
  - `enum ListColumn: String, CaseIterable, Sendable` (`position, end, duration, kind, lane, dest, data1, data2, release, smpte, mute`) with `title`; `struct ListColumns: Codable, Hashable, Sendable { end = false, duration = true, release = false, smpte = false, fps = 30; func visible(lanes: Int) -> [ListColumn] }`.
  - `struct ListRow: Equatable, Identifiable, Sendable` with `enum Source { event(lane: LaneKey, row: EventRow), tempo(index: Int, point: TempoPointRow), meter(index: Int, entry: MeterEntryRow), marker(index: Int, marker: MarkerRow) }`, `id: String` ("e<id>", "t<i>", "m<i>", "k<i>"), `tick`, `kind`, `event: EventRow?`, `lane: LaneKey?`.
  - `struct ListLane { key: LaneKey; events: [EventRow]; dest: UInt64? }`, `struct ListInput { lanes: [ListLane]; range: Range<Int64>?; info: SequenceInfo? }`, `enum ListRows { static func build(_ input: ListInput, filter: ListFilter) -> [ListRow] }`.
  - `struct ListContext { seq, meter, info, laneNames, destNames, seqNames, seconds: [String: Double], fps, tickAtSeconds: (Double) -> Int64? }`.
  - `enum ListText { static func cell(_ r: ListRow, _ col: ListColumn, _ c: ListContext) -> String; static func editable(_ r: ListRow, _ col: ListColumn) -> Bool; static func summary(_ r: ListRow, _ c: ListContext) -> String }`.
  - `struct ListEditError: Error, Equatable { message: String }`, `enum ListChange: Equatable { unchanged; edit(EditCommand, tick: Int64) }`, `enum ListEdit { static func commit(_ text: String, column: ListColumn, row: ListRow, context: ListContext) throws -> ListChange }`.
  - `enum ListInsert { static func command(_ kind: ListKind, tick: Int64, lane: LaneKey?, seq: UInt64, info: SequenceInfo?) -> EditCommand?; static func row(_ kind: ListKind, tick: Int64) -> EventRow?; static func blankRow(kind: EventKindRow, tick: Int64) -> EventRow }`, `enum ListDelete { static func commands(_ rows: [ListRow], seq: UInt64, info: SequenceInfo?) -> [EditCommand] }`.

Rulings 13–19. Errors carry the status-line text. A text equal to what the cell shows changes nothing (`.unchanged`, no undo step).

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/ListRowsTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
let listLaneA = LaneKey(3, 31)
let listLaneB = LaneKey(3, 32)

/// An event row of `kind` with `id` at `tick` (other fields blank).
func listEvent(_ id: UInt64, _ tick: Int64, _ kind: EventKindRow) -> EventRow {
    var r = ListInsert.blankRow(kind: kind, tick: tick)
    r.id = id
    return r
}

func listContext() -> ListContext {
    var info = fourFour(3)
    info.tempo.append(TempoPointRow(tick: 16 * q, bpm: 90, ramp: true))
    info.markers = [MarkerRow(tick: q, name: "Verse")]
    return ListContext(
        seq: 3, meter: info.meter, info: info, laneNames: [listLaneA: "Chords", listLaneB: "Bass"],
        destNames: [5: "Piano", 6: "Bass Synth"], seqNames: [2: "Groove"])
}

@Suite struct ListRowsTests {
    @Test func rowsAreSortedWithMapRowsFirst() {
        let a = ListLane(key: listLaneA, events: [listEvent(6, 0, .cc), listEvent(5, q, .note)], dest: 5)
        let b = ListLane(key: listLaneB, events: [listEvent(7, 0, .note)], dest: 6)
        let rows = ListRows.build(ListInput(lanes: [a, b], range: nil, info: listContext().info), filter: ListFilter())
        #expect(rows.map(\.id) == ["t0", "m0", "e6", "e7", "k0", "e5", "t1"])
        #expect(rows[2].lane == listLaneA && rows[3].lane == listLaneB)
    }

    @Test func blockRangesAndFiltersLeaveRowsOut() {
        var text = listEvent(8, 0, .text)
        text.text = "hi"
        text.textKind = .lyric
        let a = ListLane(key: listLaneA, events: [listEvent(6, 0, .cc), listEvent(5, q, .note), text], dest: 5)
        let b = ListLane(key: listLaneB, events: [listEvent(7, 0, .note)], dest: 6)
        let inBlock = ListRows.build(ListInput(lanes: [a], range: 0..<q, info: nil), filter: ListFilter())
        #expect(inBlock.map(\.id) == ["e6", "e8"], "the note at q starts after the block")
        let noCC = ListRows.build(ListInput(lanes: [a, b], range: nil, info: nil), filter: ListFilter(hiddenKinds: [.cc]))
        #expect(!noCC.map(\.id).contains("e6"))
        let piano = ListRows.build(ListInput(lanes: [a, b], range: nil, info: nil), filter: ListFilter(shownDests: [5]))
        #expect(piano.map(\.id) == ["e6", "e8", "e5"], "Bass's note is hidden; text ignores destinations")
        var routed = listEvent(9, 0, .note)
        routed.dest = 6
        let own = ListRows.build(
            ListInput(lanes: [ListLane(key: listLaneA, events: [routed], dest: 5)], range: nil, info: nil),
            filter: ListFilter(shownDests: [5]))
        #expect(own.isEmpty, "an event's own destination wins over its lane's")
        #expect(ListFilter(hiddenKinds: [.cc, .tempo]).viewFilter == ViewFilterRow(hiddenKinds: [.cc], hiddenLanes: []))
        #expect(ListFilter().viewFilter == nil)
    }

    @Test func optionalColumnsAndTheLaneColumn() {
        #expect(ListColumns().visible(lanes: 1) == [.position, .duration, .kind, .dest, .data1, .data2, .mute])
        var all = ListColumns()
        all.end = true
        all.release = true
        all.smpte = true
        #expect(
            all.visible(lanes: 2)
                == [.position, .end, .duration, .kind, .lane, .dest, .data1, .data2, .release, .smpte, .mute])
    }

    @Test func cellsShowEveryKind() {
        let c = listContext()
        func cells(_ e: EventRow) -> [String] {
            let r = ListRow(.event(lane: listLaneA, row: e))
            return [ListText.cell(r, .kind, c), ListText.cell(r, .data1, c), ListText.cell(r, .data2, c)]
        }
        var note = listEvent(1, q, .note)
        note.pitch = 60
        note.velocity = Velocity7.up(100)
        note.duration = q + q / 4
        let nr = ListRow(.event(lane: listLaneA, row: note))
        #expect(cells(note) == ["Note", "C4", "100"])
        #expect(ListText.cell(nr, .position, c) == "1.2.0")
        #expect(ListText.cell(nr, .duration, c) == "1.240")
        #expect(ListText.cell(nr, .end, c) == "1.3.240")
        #expect(ListText.cell(nr, .dest, c) == "Lane")
        #expect(ListText.cell(nr, .lane, c) == "Chords")
        #expect(ListText.summary(nr, c) == "Note C4, bar 1 beat 2, velocity 100")
        var cc = listEvent(2, 0, .cc)
        cc.value2 = 7
        cc.value = Int64(Value7.up(64))
        cc.dest = 6
        #expect(cells(cc) == ["Controller", "7", "64"])
        #expect(ListText.cell(ListRow(.event(lane: listLaneA, row: cc)), .dest, c) == "Bass Synth")
        var pb = listEvent(3, 0, .pitchBend)
        pb.value = Int64(Bend14.up(-100))
        #expect(cells(pb) == ["Pitch Bend", "", "-100"])
        var program = listEvent(4, 0, .program)
        program.value = 9
        program.value2 = 0 * 128 + 5
        #expect(cells(program) == ["Program", "9", "0:5"])
        var sysex = listEvent(5, 0, .sysEx)
        sysex.data = Data([0x7E, 0x7F, 0x09, 0x01])
        #expect(cells(sysex) == ["SysEx", "4 bytes", "7E 7F 09 01"])
        var text = listEvent(6, 0, .text)
        text.text = "verse"
        text.textKind = .lyric
        #expect(cells(text) == ["Lyric", "Lyric", "verse"])
        var reference = listEvent(7, 0, .reference)
        reference.refSeq = 2
        reference.refLoops = 3
        reference.refTranspose = 2
        #expect(cells(reference) == ["Reference", "Groove", "×3 +2"])
        reference.refLoops = 0
        reference.refTranspose = -1
        #expect(cells(reference)[2] == "fill -1")
        var region = listEvent(8, 0, .audioRegion)
        region.gainDb = -3.5
        #expect(cells(region) == ["Audio Region", "Audio", "-3.5 dB"])
        let info = c.info!
        let tempo = ListRow(.tempo(index: 1, point: info.tempo[1]))
        #expect([ListText.cell(tempo, .data1, c), ListText.cell(tempo, .data2, c)] == ["90.00", "ramp"])
        let meter = ListRow(.meter(index: 0, entry: MeterEntryRow(tick: 0, num: 6, den: 8, sharps: 2, minor: false)))
        #expect([ListText.cell(meter, .data1, c), ListText.cell(meter, .data2, c)] == ["6/8", "D major"])
        #expect(ListText.cell(ListRow(.marker(index: 0, marker: info.markers[0])), .data2, c) == "Verse")
        #expect(!ListText.editable(tempo, .kind) && ListText.editable(tempo, .position))
        #expect(!ListText.editable(ListRow(.tempo(index: 0, point: info.tempo[0])), .position), "the first tempo stays at 0")
        #expect(!ListText.editable(ListRow(.event(lane: listLaneA, row: sysex)), .data2))
    }

    @Test func buildingRowsForAHundredThousandEventsIsQuick() {
        let events = (0..<100_000).map { i in listEvent(UInt64(i), Int64(i) * 240, .note) }
        let start = Date()
        let rows = ListRows.build(
            ListInput(lanes: [ListLane(key: listLaneA, events: events, dest: 5)], range: nil, info: nil),
            filter: ListFilter())
        #expect(rows.count == 100_000)
        #expect(Date().timeIntervalSince(start) < 3, "debug build bound")
    }
}
```

Create `app/Tests/VizDAWAppModelTests/ListEditTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
private let bar: Int64 = 4 * q

private func noteRowAt(_ tick: Int64) -> ListRow {
    var n = listEvent(1, tick, .note)
    n.pitch = 60
    n.velocity = Velocity7.up(64)
    n.duration = q
    return ListRow(.event(lane: listLaneA, row: n))
}

private func commit(_ text: String, _ col: ListColumn, _ r: ListRow, _ c: ListContext = listContext()) throws
    -> ListChange
{
    try ListEdit.commit(text, column: col, row: r, context: c)
}

private func replaced(_ r: ListRow, _ change: (inout EventRow) -> Void) -> ListChange {
    var n = r.event!
    change(&n)
    return .edit(.event(edit: .replace(event: n.id, row: n)), tick: n.tick)
}

private func message(_ body: () throws -> ListChange) -> String? {
    do {
        _ = try body()
        return nil
    } catch let e as ListEditError {
        return e.message
    } catch {
        return "\(error)"
    }
}

@Suite struct ListEditTests {
    @Test func typingABarNumberMovesToItsDownbeat() throws {
        let r = noteRowAt(q + 100)
        #expect(
            try commit("5", .position, r)
                == .edit(.event(edit: .move(events: [1], delta: 4 * bar - (q + 100), toLane: nil)), tick: 4 * bar))
        // q + 100 is 1.2.6; ".3" changes the beat only: 1.3.6 = 2q + 96.
        #expect(
            try commit(".3", .position, r)
                == .edit(.event(edit: .move(events: [1], delta: q - 4, toLane: nil)), tick: 2 * q + 96))
        #expect(try commit("1.2.6", .position, r) == .unchanged)
    }

    @Test func noteCellsReplaceTheEvent() throws {
        let r = noteRowAt(0)
        #expect(try commit("D4", .data1, r) == replaced(r) { $0.pitch = 62 })
        #expect(try commit("100", .data2, r) == replaced(r) { $0.velocity = Velocity7.up(100) })
        #expect(try commit("64", .data2, r) == .unchanged, "the shown value keeps its 16 bits")
        #expect(try commit("2.0", .duration, r) == replaced(r) { $0.duration = 2 * q })
        #expect(try commit("1.3", .end, r) == replaced(r) { $0.duration = 2 * q })
        #expect(try commit("90", .release, r) == replaced(r) { $0.releaseVelocity = Velocity7.up(90) })
        #expect(try commit("M", .mute, r) == .edit(.event(edit: .setMute(events: [1], mute: true)), tick: 0))
        #expect(try commit("bass synth", .dest, r) == .edit(.event(edit: .setDest(events: [1], dest: 6)), tick: 0))
        #expect(try commit("", .dest, r) == .unchanged, "already the lane's")
    }

    @Test func otherKindsEditTheirFields() throws {
        let cc = ListRow(.event(lane: listLaneA, row: listEvent(2, 0, .cc)))
        #expect(try commit("11", .data1, cc) == replaced(cc) { $0.value2 = 11 })
        #expect(try commit("127", .data2, cc) == replaced(cc) { $0.value = Int64(Value7.up(127)) })
        let pb = ListRow(.event(lane: listLaneA, row: listEvent(3, 0, .pitchBend)))
        #expect(try commit("-8192", .data2, pb) == .unchanged, "0 is the lowest bend")
        #expect(try commit("0", .data2, pb) == replaced(pb) { $0.value = Int64(Bend14.up(0)) })
        var p = listEvent(4, 0, .program)
        p.value2 = -1
        let program = ListRow(.event(lane: listLaneA, row: p))
        #expect(try commit("1:2", .data2, program) == replaced(program) { $0.value2 = 130 })
        var t = listEvent(5, 0, .text)
        t.text = "a"
        t.textKind = .text
        let text = ListRow(.event(lane: listLaneA, row: t))
        #expect(try commit("Cue", .data1, text) == replaced(text) { $0.textKind = .cue })
        #expect(try commit("b", .data2, text) == replaced(text) { $0.text = "b" })
        var rf = listEvent(6, 0, .reference)
        rf.refLoops = 1
        let reference = ListRow(.event(lane: listLaneA, row: rf))
        #expect(
            try commit("×3 +2", .data2, reference)
                == replaced(reference) {
                    $0.refLoops = 3
                    $0.refTranspose = 2
                })
        #expect(try commit("fill", .data2, reference) == replaced(reference) { $0.refLoops = 0 })
        let region = ListRow(.event(lane: listLaneA, row: listEvent(7, 0, .audioRegion)))
        #expect(try commit("-6 dB", .data2, region) == replaced(region) { $0.gainDb = -6 })
    }

    @Test func badTextIsRefusedWithAMessage() {
        let r = noteRowAt(0)
        #expect(message { try commit("abc", .position, r) }?.contains("not a position") == true)
        #expect(message { try commit("300", .data2, r) } == "Velocity must be 0 to 127.")
        #expect(message { try commit("H9", .data1, r) }?.contains("not a pitch") == true)
        #expect(message { try commit("0.4.0", .end, r) } == "The end is before the start.")
        #expect(message { try commit("Organ", .dest, r) } == "No destination is named \"Organ\".")
        #expect(message { try commit("maybe", .mute, r) } == "Type M to mute, or clear the cell.")
        let c = listContext()
        let tempo = ListRow(.tempo(index: 1, point: c.info!.tempo[1]))
        #expect(message { try commit("0", .data1, tempo) } == "Tempo must be 1 to 999 BPM.")
        let region = ListRow(.event(lane: listLaneA, row: listEvent(7, 0, .audioRegion)))
        #expect(message { try commit("+30 dB", .data2, region) } == "Gain must be -144 to 24 dB.")
    }

    @Test func mapRowsReplaceTheirWholeMap() throws {
        let c = listContext()
        let info = c.info!
        let tempo = ListRow(.tempo(index: 1, point: info.tempo[1]))
        var t = info.tempo
        t[1].bpm = 100
        #expect(try commit("100", .data1, tempo) == .edit(.sequence(edit: .setTempo(seq: 3, tempo: t)), tick: 16 * q))
        let meter = ListRow(.meter(index: 0, entry: info.meter[0]))
        var m = info.meter
        m[0].num = 6
        m[0].den = 8
        #expect(try commit("6/8", .data1, meter) == .edit(.sequence(edit: .setMeter(seq: 3, meter: m)), tick: 0))
        m = info.meter
        m[0].sharps = 3
        m[0].minor = true
        #expect(try commit("F# minor", .data2, meter) == .edit(.sequence(edit: .setMeter(seq: 3, meter: m)), tick: 0))
        let marker = ListRow(.marker(index: 0, marker: info.markers[0]))
        #expect(
            try commit("3", .position, marker)
                == .edit(.sequence(edit: .setMarkers(seq: 3, markers: [MarkerRow(tick: 2 * bar, name: "Verse")])), tick: 2 * bar))
        let first = ListRow(.tempo(index: 0, point: info.tempo[0]))
        #expect(try commit("2", .position, first) == .unchanged, "the first tempo stays at 0")
    }

    @Test func typedSmpteRoundsToADisplayUnit() throws {
        var c = listContext()
        c.fps = 25
        // 1.48 s at 120 BPM is 45 465.6 ticks; a unit is 16 ticks: 45 472 is 2 841.75 units → 45 472.
        c.tickAtSeconds = { s in Int64((s * 30_720).rounded()) }
        let r = noteRowAt(0)
        #expect(
            try commit("00:00:01:12", .smpte, r)
                == .edit(.event(edit: .move(events: [1], delta: 45_472, toLane: nil)), tick: 45_472))
    }

    @Test func insertDefaultsAndDeleteCommands() {
        let c = listContext()
        guard case .event(.insert(_, _, let rows))? = ListInsert.command(.note, tick: q, lane: listLaneA, seq: 3, info: c.info)
        else { Issue.record("no insert"); return }
        #expect(rows.count == 1 && rows[0].pitch == 60 && rows[0].velocity == Velocity7.up(100) && rows[0].duration == q)
        #expect(ListInsert.command(.reference, tick: 0, lane: listLaneA, seq: 3, info: c.info) == nil)
        #expect(ListInsert.command(.note, tick: 0, lane: nil, seq: 3, info: c.info) == nil)
        guard case .sequence(.setMeter(_, let meter))? = ListInsert.command(.meter, tick: q, lane: nil, seq: 3, info: c.info)
        else { Issue.record("no meter"); return }
        #expect(meter.map(\.tick) == [0, bar], "the next barline")
        guard case .sequence(.setMarkers(_, let markers))? = ListInsert.command(.marker, tick: 0, lane: nil, seq: 3, info: c.info)
        else { Issue.record("no marker"); return }
        #expect(markers.map(\.name) == ["Marker 2", "Verse"])
        let rows = [
            noteRowAt(0), ListRow(.tempo(index: 0, point: c.info!.tempo[0])),
            ListRow(.tempo(index: 1, point: c.info!.tempo[1])), ListRow(.marker(index: 0, marker: c.info!.markers[0])),
        ]
        #expect(
            ListDelete.commands(rows, seq: 3, info: c.info) == [
                .event(edit: .delete(events: [1])),
                .sequence(edit: .setTempo(seq: 3, tempo: [c.info!.tempo[0]])),
                .sequence(edit: .setMarkers(seq: 3, markers: [])),
            ])
    }
}
```

Check of the SMPTE number: 1.48 s × 30 720 = 45 465.6 → 45 466 ticks; one unit is 15 360 / 960 = 16 ticks; 45 466 / 16 = 2 841.6 → nearest unit 2 842 → 45 472.

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:" | head -5
```

Expected: `cannot find 'ListInsert' in scope`.

- [ ] **Step 3: Write `ListRows.swift`**

```bash
cat > app/Sources/VizDAWAppModel/ListRows.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// What a list row is: an event kind or a sequence map entry (Plan 4c
/// Ruling 13).
public enum ListKind: String, Codable, CaseIterable, Hashable, Sendable {
    case note, cc, pitchBend, channelPressure, polyPressure, program, sysEx, text, reference, audioRegion
    case tempo, meter, marker

    public init(_ k: EventKindRow) {
        switch k {
        case .note: self = .note
        case .cc: self = .cc
        case .pitchBend: self = .pitchBend
        case .channelPressure: self = .channelPressure
        case .polyPressure: self = .polyPressure
        case .program: self = .program
        case .sysEx: self = .sysEx
        case .text: self = .text
        case .reference: self = .reference
        case .audioRegion: self = .audioRegion
        }
    }

    public var eventKind: EventKindRow? {
        switch self {
        case .note: .note
        case .cc: .cc
        case .pitchBend: .pitchBend
        case .channelPressure: .channelPressure
        case .polyPressure: .polyPressure
        case .program: .program
        case .sysEx: .sysEx
        case .text: .text
        case .reference: .reference
        case .audioRegion: .audioRegion
        case .tempo, .meter, .marker: nil
        }
    }

    /// Routed to a MIDI destination (the destination filter applies).
    public var isMidi: Bool {
        switch self {
        case .note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .sysEx: true
        default: false
        }
    }

    public var title: String {
        switch self {
        case .note: "Note"
        case .cc: "Controller"
        case .pitchBend: "Pitch Bend"
        case .channelPressure: "Channel Pressure"
        case .polyPressure: "Poly Pressure"
        case .program: "Program"
        case .sysEx: "SysEx"
        case .text: "Text"
        case .reference: "Reference"
        case .audioRegion: "Audio Region"
        case .tempo: "Tempo"
        case .meter: "Meter and Key"
        case .marker: "Marker"
        }
    }

    /// What Insert offers (Plan 4c Ruling 18).
    public static let insertable: [ListKind] = [
        .note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .sysEx, .text, .tempo, .meter, .marker,
    ]
}

/// What a list hides (Plan 4c Ruling 19).
public struct ListFilter: Codable, Hashable, Sendable {
    public var hiddenKinds: Set<ListKind>
    /// Destinations shown (nil: all).
    public var shownDests: Set<UInt64>?

    public init(hiddenKinds: Set<ListKind> = [], shownDests: Set<UInt64>? = nil) {
        self.hiddenKinds = hiddenKinds
        self.shownDests = shownDests
    }

    /// Whether a row of `kind` routed to `dest` (its own, else its lane's)
    /// shows.
    public func shows(_ kind: ListKind, dest: UInt64?) -> Bool {
        if hiddenKinds.contains(kind) { return false }
        guard let shownDests, kind.isMidi else { return true }
        return dest.map(shownDests.contains) ?? false
    }

    /// The hidden event kinds as the core's view filter (spec §3.3).
    public var viewFilter: ViewFilterRow? {
        let kinds = ListKind.allCases.filter(hiddenKinds.contains).compactMap(\.eventKind)
        return kinds.isEmpty ? nil : ViewFilterRow(hiddenKinds: kinds, hiddenLanes: [])
    }
}

/// List columns (Plan 4c Ruling 14).
public enum ListColumn: String, CaseIterable, Sendable {
    case position, end, duration, kind, lane, dest, data1, data2, release, smpte, mute

    public var title: String {
        switch self {
        case .position: "Position"
        case .end: "End"
        case .duration: "Length"
        case .kind: "Kind"
        case .lane: "Lane"
        case .dest: "Destination"
        case .data1: "Data 1"
        case .data2: "Data 2"
        case .release: "Release"
        case .smpte: "SMPTE"
        case .mute: "Mute"
        }
    }
}

/// The optional columns shown (an app preference, Plan 4c Ruling 19).
public struct ListColumns: Codable, Hashable, Sendable {
    public var end = false
    public var duration = true
    public var release = false
    public var smpte = false
    /// SMPTE frames per second: 24, 25 or 30.
    public var fps = 30

    public init() {}

    public func visible(lanes: Int) -> [ListColumn] {
        var c: [ListColumn] = [.position]
        if end { c.append(.end) }
        if duration { c.append(.duration) }
        c.append(.kind)
        if lanes > 1 { c.append(.lane) }
        c += [.dest, .data1, .data2]
        if release { c.append(.release) }
        if smpte { c.append(.smpte) }
        c.append(.mute)
        return c
    }
}

/// One row of a list editor.
public struct ListRow: Equatable, Identifiable, Sendable {
    public enum Source: Equatable, Sendable {
        case event(lane: LaneKey, row: EventRow)
        case tempo(index: Int, point: TempoPointRow)
        case meter(index: Int, entry: MeterEntryRow)
        case marker(index: Int, marker: MarkerRow)
    }

    public let source: Source

    public init(_ source: Source) { self.source = source }

    /// Events keep their ID across edits; map rows are numbered.
    public var id: String {
        switch source {
        case .event(_, let e): "e\(e.id)"
        case .tempo(let i, _): "t\(i)"
        case .meter(let i, _): "m\(i)"
        case .marker(let i, _): "k\(i)"
        }
    }

    public var tick: Int64 {
        switch source {
        case .event(_, let e): e.tick
        case .tempo(_, let p): p.tick
        case .meter(_, let m): m.tick
        case .marker(_, let m): m.tick
        }
    }

    public var kind: ListKind {
        switch source {
        case .event(_, let e): ListKind(e.kind)
        case .tempo: .tempo
        case .meter: .meter
        case .marker: .marker
        }
    }

    public var event: EventRow? {
        if case .event(_, let e) = source { return e }
        return nil
    }

    public var lane: LaneKey? {
        if case .event(let l, _) = source { return l }
        return nil
    }
}

/// A lane's events for a list.
public struct ListLane: Sendable {
    public var key: LaneKey
    public var events: [EventRow]
    /// The lane's MIDI destination.
    public var dest: UInt64?

    public init(key: LaneKey, events: [EventRow], dest: UInt64?) {
        self.key = key
        self.events = events
        self.dest = dest
    }
}

/// What a list shows.
public struct ListInput: Sendable {
    public var lanes: [ListLane]
    /// A block's range (events starting inside it).
    public var range: Range<Int64>?
    /// The sequence's maps: tempo, meter and marker rows (sequence targets).
    public var info: SequenceInfo?

    public init(lanes: [ListLane], range: Range<Int64>?, info: SequenceInfo?) {
        self.lanes = lanes
        self.range = range
        self.info = info
    }
}

public enum ListRows {
    /// Rows by tick, then tempo, meter, marker, events, then lane order, then
    /// ID (Plan 4c Ruling 13).
    public static func build(_ input: ListInput, filter: ListFilter) -> [ListRow] {
        typealias Keyed = (tick: Int64, group: Int, lane: Int, id: UInt64, row: ListRow)
        var keyed: [Keyed] = []
        func inRange(_ t: Int64) -> Bool { input.range.map { $0.contains(t) } ?? true }
        if let info = input.info {
            if filter.shows(.tempo, dest: nil) {
                for (i, p) in info.tempo.enumerated() where inRange(p.tick) {
                    keyed.append((p.tick, 0, 0, UInt64(i), ListRow(.tempo(index: i, point: p))))
                }
            }
            if filter.shows(.meter, dest: nil) {
                for (i, m) in info.meter.enumerated() where inRange(m.tick) {
                    keyed.append((m.tick, 1, 0, UInt64(i), ListRow(.meter(index: i, entry: m))))
                }
            }
            if filter.shows(.marker, dest: nil) {
                for (i, m) in info.markers.enumerated() where inRange(m.tick) {
                    keyed.append((m.tick, 2, 0, UInt64(i), ListRow(.marker(index: i, marker: m))))
                }
            }
        }
        for (li, lane) in input.lanes.enumerated() {
            for e in lane.events where inRange(e.tick) {
                guard filter.shows(ListKind(e.kind), dest: e.dest ?? lane.dest) else { continue }
                keyed.append((e.tick, 3, li, e.id, ListRow(.event(lane: lane.key, row: e))))
            }
        }
        keyed.sort { ($0.tick, $0.group, $0.lane, $0.id) < ($1.tick, $1.group, $1.lane, $1.id) }
        return keyed.map(\.row)
    }
}

/// What cell text needs besides the row.
public struct ListContext {
    public var seq: UInt64
    public var meter: [MeterEntryRow]
    public var info: SequenceInfo?
    public var laneNames: [LaneKey: String]
    public var destNames: [UInt64: String]
    public var seqNames: [UInt64: String]
    /// Seconds by row ID (the SMPTE column).
    public var seconds: [String: Double]
    public var fps: Int
    /// The tick at a time in seconds (typed SMPTE).
    public var tickAtSeconds: (Double) -> Int64?

    public init(
        seq: UInt64, meter: [MeterEntryRow] = [], info: SequenceInfo? = nil, laneNames: [LaneKey: String] = [:],
        destNames: [UInt64: String] = [:], seqNames: [UInt64: String] = [:], seconds: [String: Double] = [:],
        fps: Int = 30, tickAtSeconds: @escaping (Double) -> Int64? = { _ in nil }
    ) {
        self.seq = seq
        self.meter = meter
        self.info = info
        self.laneNames = laneNames
        self.destNames = destNames
        self.seqNames = seqNames
        self.seconds = seconds
        self.fps = fps
        self.tickAtSeconds = tickAtSeconds
    }

    /// The beat length at `tick`.
    public func beat(at tick: Int64) -> Int64 { MeterMath.beatTicks(MeterMath.bar(at: tick, meter).meter) }
}

/// Cell text (Plan 4c Ruling 16).
public enum ListText {
    static let textKinds: [TextKindRow] = [.text, .lyric, .cue, .marker, .copyright]

    static func title(_ k: TextKindRow?) -> String {
        switch k {
        case .text, nil: "Text"
        case .lyric: "Lyric"
        case .cue: "Cue"
        case .marker: "Marker Text"
        case .copyright: "Copyright"
        }
    }

    static func kindTitle(_ r: ListRow) -> String {
        if let e = r.event, e.kind == .text { return title(e.textKind) }
        return r.kind.title
    }

    /// A note's length, a reference's span or a region's length.
    static func span(_ r: ListRow) -> Int64? {
        guard let e = r.event, [.note, .reference, .audioRegion].contains(e.kind) else { return nil }
        return e.duration
    }

    public static func cell(_ r: ListRow, _ col: ListColumn, _ c: ListContext) -> String {
        switch col {
        case .position:
            return Bbu.of(r.tick, meter: c.meter).text
        case .end:
            guard let len = span(r) else { return "" }
            return Bbu.of(r.tick + len, meter: c.meter).text
        case .duration:
            guard let len = span(r) else { return "" }
            return DurationText.text(len, beat: c.beat(at: r.tick))
        case .kind:
            return kindTitle(r)
        case .lane:
            return r.lane.flatMap { c.laneNames[$0] } ?? ""
        case .dest:
            guard let e = r.event, r.kind.isMidi else { return "" }
            return e.dest.map { c.destNames[$0] ?? "Destination \($0)" } ?? "Lane"
        case .data1:
            return data1(r, c)
        case .data2:
            return data2(r, c)
        case .release:
            guard let e = r.event, e.kind == .note else { return "" }
            return "\(Velocity7.of(e.releaseVelocity))"
        case .smpte:
            return c.seconds[r.id].map { Smpte.text(seconds: $0, fps: c.fps) } ?? ""
        case .mute:
            return r.event?.mute == true ? "M" : ""
        }
    }

    static func data1(_ r: ListRow, _ c: ListContext) -> String {
        switch r.source {
        case .tempo(_, let p): return String(format: "%.2f", p.bpm)
        case .meter(_, let m): return "\(m.num)/\(m.den)"
        case .marker: return ""
        case .event(_, let e):
            switch e.kind {
            case .note, .polyPressure: return PitchName.name(e.pitch)
            case .cc: return "\(e.value2)"
            case .program: return "\(e.value)"
            case .sysEx: return "\(e.data.count) bytes"
            case .text: return title(e.textKind)
            case .reference: return e.refSeq.flatMap { c.seqNames[$0] } ?? "Sequence"
            case .audioRegion: return "Audio"
            case .pitchBend, .channelPressure: return ""
            }
        }
    }

    static func data2(_ r: ListRow, _ c: ListContext) -> String {
        switch r.source {
        case .tempo(_, let p): return p.ramp ? "ramp" : "jump"
        case .meter(_, let m): return KeyName.name(sharps: m.sharps, minor: m.minor)
        case .marker(_, let m): return m.name
        case .event(_, let e):
            switch e.kind {
            case .note: return "\(Velocity7.of(e.velocity))"
            case .cc, .channelPressure, .polyPressure: return "\(Value7.of(e.value))"
            case .pitchBend: return "\(Bend14.of(e.value))"
            case .program: return e.value2 < 0 ? "" : "\(e.value2 / 128):\(e.value2 % 128)"
            case .sysEx:
                let head = e.data.prefix(8).map { String(format: "%02X", $0) }.joined(separator: " ")
                return e.data.count > 8 ? head + " …" : head
            case .text: return e.text ?? ""
            case .reference:
                let loops = e.refLoops == 0 ? "fill" : "×\(e.refLoops)"
                return "\(loops) \(e.refTranspose > 0 ? "+" : "")\(e.refTranspose)"
            case .audioRegion: return String(format: "%.1f dB", e.gainDb)
            }
        }
    }

    public static func editable(_ r: ListRow, _ col: ListColumn) -> Bool {
        switch r.source {
        case .tempo(let i, _), .meter(let i, _):
            return col == .data1 || col == .data2 || (col == .position && i > 0)
        case .marker:
            return col == .position || col == .data2
        case .event(_, let e):
            switch col {
            case .position, .smpte, .mute: return true
            case .end, .duration, .release: return e.kind == .note
            case .dest: return ListKind(e.kind).isMidi
            case .kind, .lane: return false
            case .data1: return [.note, .polyPressure, .cc, .program, .text].contains(e.kind)
            case .data2:
                return [.note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .text, .reference, .audioRegion]
                    .contains(e.kind)
            }
        }
    }

    /// The row as VoiceOver reads it ("Note C4, bar 3 beat 1, velocity 100").
    public static func summary(_ r: ListRow, _ c: ListContext) -> String {
        let b = Bbu.of(r.tick, meter: c.meter)
        let at = "bar \(b.bar) beat \(b.beat)"
        if r.kind == .note { return "Note \(data1(r, c)), \(at), velocity \(data2(r, c))" }
        let d = [data1(r, c), data2(r, c)].filter { !$0.isEmpty }.joined(separator: ", ")
        return "\(kindTitle(r)), \(at)" + (d.isEmpty ? "" : ", \(d)")
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Write `ListEdit.swift`**

```bash
cat > app/Sources/VizDAWAppModel/ListEdit.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// Why typed text was refused; `message` goes to the status line.
public struct ListEditError: Error, Equatable {
    public let message: String

    public init(_ message: String) { self.message = message }
}

/// What a committed cell does.
public enum ListChange: Equatable {
    case unchanged
    /// Send the command; the row then starts at `tick` (the list follows it).
    case edit(EditCommand, tick: Int64)
}

/// Typed cell text to commands (Plan 4c Rulings 15–17).
public enum ListEdit {
    public static func commit(_ text: String, column: ListColumn, row r: ListRow, context c: ListContext) throws
        -> ListChange
    {
        let t = text.trimmingCharacters(in: .whitespaces)
        switch r.source {
        case .event(_, let e): return try event(t, column, e, c)
        case .tempo(let i, let p): return try tempo(t, column, i, p, c)
        case .meter(let i, let m): return try meter(t, column, i, m, c)
        case .marker(let i, let m): return try marker(t, column, i, m, c)
        }
    }

    static func position(_ t: String, from tick: Int64, _ c: ListContext) throws -> Int64 {
        guard let b = PositionInput.parse(t, current: Bbu.of(tick, meter: c.meter)) else {
            throw ListEditError("\"\(t)\" is not a position: type a bar, bar.beat or bar.beat.unit.")
        }
        return b.tick(meter: c.meter)
    }

    /// A typed SMPTE time as a tick, rounded to the nearest display unit.
    static func smpte(_ t: String, _ c: ListContext) throws -> Int64 {
        guard let s = Smpte.parse(t, fps: c.fps), let tick = c.tickAtSeconds(s) else {
            throw ListEditError("\"\(t)\" is not an SMPTE time (hh:mm:ss:ff).")
        }
        let unit = max(c.beat(at: tick) / Bbu.unitsPerBeat, 1)
        let start = MeterMath.bar(at: tick, c.meter).start
        return start + ((tick - start + unit / 2) / unit) * unit
    }

    static func number(_ t: String, _ range: ClosedRange<Int>, _ what: String) throws -> Int {
        guard let n = Int(t), range.contains(n) else {
            throw ListEditError("\(what) must be \(range.lowerBound) to \(range.upperBound).")
        }
        return n
    }

    static func event(_ t: String, _ col: ListColumn, _ e: EventRow, _ c: ListContext) throws -> ListChange {
        func replace(_ change: (inout EventRow) -> Void) -> ListChange {
            var n = e
            change(&n)
            return n == e ? .unchanged : .edit(.event(edit: .replace(event: e.id, row: n)), tick: n.tick)
        }
        switch col {
        case .position, .smpte:
            let tick = try (col == .position ? position(t, from: e.tick, c) : smpte(t, c))
            return tick == e.tick
                ? .unchanged : .edit(.event(edit: .move(events: [e.id], delta: tick - e.tick, toLane: nil)), tick: tick)
        case .end:
            let end = try position(t, from: e.tick + e.duration, c)
            guard end >= e.tick else { throw ListEditError("The end is before the start.") }
            return replace { $0.duration = end - e.tick }
        case .duration:
            guard let d = DurationText.parse(t, current: e.duration, beat: c.beat(at: e.tick)) else {
                throw ListEditError("\"\(t)\" is not a length: type beats.units, such as 1.480.")
            }
            return replace { $0.duration = d }
        case .release:
            let v = try number(t, 0...127, "Release velocity")
            return Velocity7.of(e.releaseVelocity) == v ? .unchanged : replace { $0.releaseVelocity = Velocity7.up(v) }
        case .mute:
            let on: Bool
            switch t.lowercased() {
            case "m", "y", "yes", "1", "x", "on": on = true
            case "", "n", "no", "0", "off": on = false
            default: throw ListEditError("Type M to mute, or clear the cell.")
            }
            return on == e.mute ? .unchanged : .edit(.event(edit: .setMute(events: [e.id], mute: on)), tick: e.tick)
        case .dest:
            let dest: UInt64?
            if t.isEmpty || t == "-" || t == "—" || t.lowercased() == "lane" {
                dest = nil
            } else if let id = c.destNames.first(where: { $0.value.caseInsensitiveCompare(t) == .orderedSame })?.key {
                dest = id
            } else {
                throw ListEditError("No destination is named \"\(t)\".")
            }
            return dest == e.dest ? .unchanged : .edit(.event(edit: .setDest(events: [e.id], dest: dest)), tick: e.tick)
        case .data1:
            return try data1(t, e, replace)
        case .data2:
            return try data2(t, e, replace)
        case .kind, .lane:
            return .unchanged
        }
    }

    static func data1(_ t: String, _ e: EventRow, _ replace: ((inout EventRow) -> Void) -> ListChange) throws
        -> ListChange
    {
        switch e.kind {
        case .note, .polyPressure:
            guard let p = PitchName.parse(t) else {
                throw ListEditError("\"\(t)\" is not a pitch: type a name such as C4 or a number 0 to 127.")
            }
            return replace { $0.pitch = p }
        case .cc:
            let n = try number(t, 0...127, "The controller number")
            return replace { $0.value2 = Int64(n) }
        case .program:
            let n = try number(t, 0...127, "The program")
            return replace { $0.value = Int64(n) }
        case .text:
            guard let k = ListText.textKinds.first(where: { ListText.title($0).caseInsensitiveCompare(t) == .orderedSame })
            else { throw ListEditError("Type Text, Lyric, Cue, Marker Text or Copyright.") }
            return replace { $0.textKind = k }
        default:
            return .unchanged
        }
    }

    static func data2(_ t: String, _ e: EventRow, _ replace: ((inout EventRow) -> Void) -> ListChange) throws
        -> ListChange
    {
        switch e.kind {
        case .note:
            let v = try number(t, 0...127, "Velocity")
            return Velocity7.of(e.velocity) == v ? .unchanged : replace { $0.velocity = Velocity7.up(v) }
        case .cc, .channelPressure, .polyPressure:
            let v = try number(t, 0...127, "The value")
            return Value7.of(e.value) == v ? .unchanged : replace { $0.value = Int64(Value7.up(v)) }
        case .pitchBend:
            let v = try number(t, -8192...8191, "Pitch bend")
            return Bend14.of(e.value) == v ? .unchanged : replace { $0.value = Int64(Bend14.up(v)) }
        case .program:
            if t.isEmpty || t == "-" { return replace { $0.value2 = -1 } }
            let parts = t.split(separator: ":").map { Int($0.trimmingCharacters(in: .whitespaces)) }
            guard parts.count == 2, let m = parts[0], let l = parts[1], (0...127).contains(m), (0...127).contains(l)
            else { throw ListEditError("Type the bank as msb:lsb, such as 0:5, or clear it.") }
            return replace { $0.value2 = Int64(m * 128 + l) }
        case .text:
            return replace { $0.text = t }
        case .reference:
            let refusal = ListEditError("Type repeats and transpose, such as ×3 +2 or fill 0.")
            let words = t.split(separator: " ").map(String.init)
            guard (1...2).contains(words.count) else { throw refusal }
            let first = words[0].lowercased()
            let loops: UInt32
            if first == "fill" {
                loops = 0
            } else if let n = UInt32(first.replacingOccurrences(of: "×", with: "").replacingOccurrences(of: "x", with: "")),
                (1...10_000).contains(n)
            {
                loops = n
            } else {
                throw refusal
            }
            var transpose = e.refTranspose
            if words.count == 2 {
                guard let tr = Int(words[1].replacingOccurrences(of: "+", with: "")), (-127...127).contains(tr) else {
                    throw refusal
                }
                transpose = Int8(tr)
            }
            return replace {
                $0.refLoops = loops
                $0.refTranspose = transpose
            }
        case .audioRegion:
            let s = t.lowercased().replacingOccurrences(of: "db", with: "").trimmingCharacters(in: .whitespaces)
            guard let g = Float(s), g.isFinite, (-144...24).contains(g) else {
                throw ListEditError("Gain must be -144 to 24 dB.")
            }
            return replace { $0.gainDb = g }
        default:
            return .unchanged
        }
    }

    static func tempo(_ t: String, _ col: ListColumn, _ i: Int, _ p: TempoPointRow, _ c: ListContext) throws
        -> ListChange
    {
        guard var points = c.info?.tempo, points.indices.contains(i) else { return .unchanged }
        var n = p
        switch col {
        case .position:
            guard i > 0 else { return .unchanged }
            n.tick = try position(t, from: p.tick, c)
        case .data1:
            let s = t.lowercased().replacingOccurrences(of: "bpm", with: "").trimmingCharacters(in: .whitespaces)
            guard let bpm = Double(s), bpm.isFinite, (1...999).contains(bpm) else {
                throw ListEditError("Tempo must be 1 to 999 BPM.")
            }
            n.bpm = bpm
        case .data2:
            switch t.lowercased() {
            case "ramp": n.ramp = true
            case "jump", "": n.ramp = false
            default: throw ListEditError("Type ramp or jump.")
            }
        default:
            return .unchanged
        }
        guard n != p else { return .unchanged }
        points[i] = n
        points.sort { $0.tick < $1.tick }
        return .edit(.sequence(edit: .setTempo(seq: c.seq, tempo: points)), tick: n.tick)
    }

    static func meter(_ t: String, _ col: ListColumn, _ i: Int, _ m: MeterEntryRow, _ c: ListContext) throws
        -> ListChange
    {
        guard var entries = c.info?.meter, entries.indices.contains(i) else { return .unchanged }
        var n = m
        switch col {
        case .position:
            guard i > 0 else { return .unchanged }
            n.tick = try position(t, from: m.tick, c)
        case .data1:
            let parts = t.split(separator: "/").compactMap { UInt8($0.trimmingCharacters(in: .whitespaces)) }
            guard parts.count == 2, (1...64).contains(parts[0]), [1, 2, 4, 8, 16, 32, 64].contains(parts[1]) else {
                throw ListEditError("Type the meter as beats/unit, such as 6/8.")
            }
            n.num = parts[0]
            n.den = parts[1]
        case .data2:
            guard let k = KeyName.parse(t) else { throw ListEditError("Type a key such as D major or F# minor.") }
            n.sharps = k.sharps
            n.minor = k.minor
        default:
            return .unchanged
        }
        guard n != m else { return .unchanged }
        entries[i] = n
        entries.sort { $0.tick < $1.tick }
        return .edit(.sequence(edit: .setMeter(seq: c.seq, meter: entries)), tick: n.tick)
    }

    static func marker(_ t: String, _ col: ListColumn, _ i: Int, _ m: MarkerRow, _ c: ListContext) throws
        -> ListChange
    {
        guard var markers = c.info?.markers, markers.indices.contains(i) else { return .unchanged }
        var n = m
        switch col {
        case .position:
            n.tick = try position(t, from: m.tick, c)
        case .data2:
            guard !t.isEmpty else { throw ListEditError("A marker needs a name.") }
            n.name = t
        default:
            return .unchanged
        }
        guard n != m else { return .unchanged }
        markers[i] = n
        markers.sort { $0.tick < $1.tick }
        return .edit(.sequence(edit: .setMarkers(seq: c.seq, markers: markers)), tick: n.tick)
    }
}

/// New rows (Plan 4c Ruling 18).
public enum ListInsert {
    public static func blankRow(kind: EventKindRow, tick: Int64) -> EventRow {
        EventRow(
            id: 0, tick: tick, kind: kind, mute: false, dest: nil, groupChannel: nil, pitch: 0, velocity: 0,
            releaseVelocity: 0, duration: 0, value: 0, value2: 0, attribute: nil, perNote: [], refSeq: nil,
            refLength: nil, refLoops: 0, refTranspose: 0, media: nil, sourceOffset: 0, frames: 0, gainDb: 0,
            fadeIn: FadeRow(length: 0, shape: .linear), fadeOut: FadeRow(length: 0, shape: .linear), text: nil,
            textKind: nil, data: Data(), eightBit: false)
    }

    /// A new event row of `kind` with its defaults; nil for references,
    /// regions and map kinds.
    public static func row(_ kind: ListKind, tick: Int64) -> EventRow? {
        guard let k = kind.eventKind else { return nil }
        var r = blankRow(kind: k, tick: tick)
        switch kind {
        case .note:
            r.pitch = 60
            r.velocity = Velocity7.up(100)
            r.duration = MeterMath.ppq
        case .cc:
            r.value2 = 7
            r.value = Int64(Value7.up(64))
        case .pitchBend:
            r.value = Int64(Bend14.up(0))
        case .channelPressure:
            r.value = 0
        case .polyPressure:
            r.pitch = 60
        case .program:
            r.value2 = -1
        case .sysEx:
            r.data = Data([0x7E, 0x7F, 0x09, 0x01])
        case .text:
            r.text = "Text"
            r.textKind = .text
        default:
            return nil
        }
        return r
    }

    /// The command inserting `kind` at `tick`: into `lane` for events, into
    /// sequence `seq`'s maps otherwise; nil when it cannot be inserted here.
    public static func command(_ kind: ListKind, tick: Int64, lane: LaneKey?, seq: UInt64, info: SequenceInfo?)
        -> EditCommand?
    {
        switch kind {
        case .reference, .audioRegion:
            return nil
        case .tempo:
            guard var points = info?.tempo else { return nil }
            let bpm = points.last(where: { $0.tick <= tick })?.bpm ?? 120
            points.removeAll { $0.tick == tick }
            points.append(TempoPointRow(tick: tick, bpm: bpm, ramp: false))
            points.sort { $0.tick < $1.tick }
            return .sequence(edit: .setTempo(seq: seq, tempo: points))
        case .meter:
            guard var entries = info?.meter else { return nil }
            let bar = MeterMath.bar(at: tick, entries)
            let at = tick == bar.start ? tick : bar.start + bar.length
            guard !entries.contains(where: { $0.tick == at }) else { return nil }
            let m = bar.meter
            entries.append(MeterEntryRow(tick: at, num: m.num, den: m.den, sharps: m.sharps, minor: m.minor))
            entries.sort { $0.tick < $1.tick }
            return .sequence(edit: .setMeter(seq: seq, meter: entries))
        case .marker:
            var markers = info?.markers ?? []
            markers.append(MarkerRow(tick: tick, name: "Marker \(markers.count + 1)"))
            markers.sort { $0.tick < $1.tick }
            return .sequence(edit: .setMarkers(seq: seq, markers: markers))
        default:
            guard let lane, let r = row(kind, tick: tick) else { return nil }
            return .event(edit: .insert(seq: lane.seq, lane: lane.lane, events: [r]))
        }
    }
}

/// Deleting rows: one command per kind of row (events, tempo, meter,
/// markers); the first tempo point and meter entry stay.
public enum ListDelete {
    public static func commands(_ rows: [ListRow], seq: UInt64, info: SequenceInfo?) -> [EditCommand] {
        var out: [EditCommand] = []
        let events = rows.compactMap(\.event).map(\.id)
        if !events.isEmpty { out.append(.event(edit: .delete(events: events))) }
        guard let info else { return out }
        var tempo = Set<Int>()
        var meter = Set<Int>()
        var markers = Set<Int>()
        for r in rows {
            switch r.source {
            case .tempo(let i, _) where i > 0: tempo.insert(i)
            case .meter(let i, _) where i > 0: meter.insert(i)
            case .marker(let i, _): markers.insert(i)
            default: break
            }
        }
        if !tempo.isEmpty {
            let kept = info.tempo.enumerated().filter { !tempo.contains($0.offset) }.map(\.element)
            out.append(.sequence(edit: .setTempo(seq: seq, tempo: kept)))
        }
        if !meter.isEmpty {
            let kept = info.meter.enumerated().filter { !meter.contains($0.offset) }.map(\.element)
            out.append(.sequence(edit: .setMeter(seq: seq, meter: kept)))
        }
        if !markers.isEmpty {
            let kept = info.markers.enumerated().filter { !markers.contains($0.offset) }.map(\.element)
            out.append(.sequence(edit: .setMarkers(seq: seq, markers: kept)))
        }
        return out
    }
}
VIZDAW_EOF
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | tail -3
```

Expected: 366 tests pass (354 + 5 + 7). If a test expectation and the code disagree on a computed number, recompute it by hand from the rules in this task (they are the specification) and fix whichever is wrong.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/ListRows.swift app/Sources/VizDAWAppModel/ListEdit.swift app/Tests/VizDAWAppModelTests/ListRowsTests.swift app/Tests/VizDAWAppModelTests/ListEditTests.swift
git commit -m "feat(app): list rows, cell text and cell edits for every kind

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 9: `ListEditorModel`, list preferences, per-list filters and played notes

**Files:**
- Create: `app/Sources/VizDAWAppModel/ListPreferences.swift`
- Create: `app/Sources/VizDAWAppModel/ListEditorModel.swift`
- Create: `app/Sources/VizDAWAppModel/NoteInputHub.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorTypes.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentStore.swift`
- Test: `app/Tests/VizDAWAppModelTests/ListEditorModelTests.swift` (new)

**Interfaces:**
- Consumes: Task 8 (`ListRows`, `ListEdit`, `ListInsert`, `ListDelete`, `ListContext`, `ListFilter`, `ListColumns`), `EditorModel.{laneEvents, laneRows, info, edit, setSelection, revs, snapshotVersion, push, selectSequence, currentSequence}`, `Session.{destinations, secondsAt, ticksAt}`, `NoteInputProtocol`.
- Produces:
  - `final class ListPreferences` (`@Observable @MainActor`): `init(defaults: UserDefaults? = nil)`, `defaultFilter: ListFilter`, `columns: ListColumns` (saved under `VizDAW.list` when `defaults` is set).
  - `struct ListCell: Equatable, Sendable { row: String; column: ListColumn }`.
  - `final class ListEditorModel` (`@Observable @MainActor`): `static func pane(_:)`, `static func window(_:of:)`, `document`, `editor`, `slot: DetailSlot`, `state`, `target`, `filter`, `columnSettings`, `lanes: [LaneKey]`, `columns: [ListColumn]`, `rows: [ListRow]`, `context: ListContext`, `destinations: [DestRow]`, `cursor: String?`, `cursorRow`, `editing: ListCell?`, `fieldInput: String?`, `lastInsertKind`, `insertKinds: [ListKind]`, `midiIn: Bool`, `row(_:)`, `index(of:)`, `commit(_:row:column:) -> Bool`, `insert(_ kind: ListKind? = nil)`, `delete(_ ids: [String])`, `select(_ ids: [String])`, `openReference(_ id: String) -> Bool`, `receive(_ notes: [NoteInRow])`, `setFilter(_:)`, `toggle(_ kind:)`, `shows(dest:)`, `toggle(dest:)`, `applyFilterToAll()`, `saveFilterAsDefault()`, `setColumns(_:)`.
  - `DetailState.listFilter: ListFilter?` (with an `init` parameter defaulting to nil); `EditorModel.openSequence(_ seq: UInt64)`; `DocumentModel.{listPreferences, list(_:), dropList(_:), activeList, listForCommands(), listensForNotes, receiveNotes(_:)}`; `DocumentStore.listPreferences` (given to every document it makes; `live()` uses `UserDefaults.standard`).
  - `final class NoteInputHub` (`@MainActor`): `init(input: (any NoteInputProtocol)?)`, `poll(_ docs: [DocumentModel])`.

Rulings 11, 12, 18, 19, 24. A list keeps its filter when its pane opens another target.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/ListEditorModelTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

final class FakeNoteInput: NoteInputProtocol, @unchecked Sendable {
    var queued: [NoteInRow] = []
    var starts = 0
    func start() throws -> UInt32 {
        starts += 1
        return 1
    }
    func stop() throws {}
    func take() throws -> [NoteInRow] {
        defer { queued = [] }
        return queued
    }
}

private let d4Loud = NoteInRow(pitch: 62, velocity: 0xC924, channel: 0)

@MainActor
private func listModel(_ target: EditorTarget = .lane(seq: 3, lane: 31)) throws
    -> (FakeSession, DocumentModel, ListEditorModel)
{
    let f = rollFake()
    f.dests = [DestRow(id: 5, name: "Piano", plugin: nil), DestRow(id: 6, name: "Bass Synth", plugin: nil)]
    let m = try DocumentModel(session: f)
    m.editor.openDetail(target, kind: .list)
    return (f, m, m.list(.pane))
}

@MainActor
@Suite struct ListEditorModelTests {
    @Test func rowsFollowTheTarget() throws {
        let (_, m, l) = try listModel()
        #expect(l.rows.map(\.id) == ["e1", "e2"])
        #expect(l.columns == [.position, .duration, .kind, .dest, .data1, .data2, .mute])
        m.editor.openDetail(.sequence(3), kind: .list)
        #expect(l.rows.map(\.id) == ["t0", "m0", "e1", "e3", "e2"], "map rows, then lanes in order")
        #expect(l.columns.contains(.lane))
        m.editor.openDetail(.block(seq: 3, lane: 31, start: 0, end: 15_360), kind: .list)
        #expect(l.rows.map(\.id) == ["e1"])
        #expect(l.context.destNames[5] == "Piano")
    }

    @Test func theCursorFollowsAMovedEvent() throws {
        let (f, _, l) = try listModel()
        #expect(l.commit("3", row: "e1", column: .position))
        #expect(f.edits.last == .event(edit: .move(events: [1], delta: 2 * 61_440, toLane: nil)))
        #expect(l.cursor == "e1")
        #expect(f.sel.events == [1] && f.sel.lanes == [LaneRef(seq: 3, lane: 31)], "the document selection follows")
    }

    @Test func aRefusedCommitLeavesTheDocumentAndSaysWhy() throws {
        let (f, m, l) = try listModel()
        #expect(!l.commit("abc", row: "e1", column: .position))
        #expect(f.edits.isEmpty)
        #expect(m.editor.status?.contains("not a position") == true)
        #expect(l.commit("1.1.0", row: "e1", column: .position), "the same position changes nothing")
        #expect(f.edits.isEmpty)
    }

    @Test func committingARowThatVanishedDoesNothing() throws {
        let (f, m, l) = try listModel()
        #expect(!l.commit("3", row: "e99", column: .position))
        #expect(f.edits.isEmpty && m.editor.status == nil)
    }

    @Test func insertRepeatsTheLastKind() throws {
        let (f, m, l) = try listModel()
        #expect(l.lastInsertKind == .note)
        l.insert(.cc)
        guard case .event(.insert(3, 31, let rows))? = f.edits.last else {
            Issue.record("no insert")
            return
        }
        #expect(rows.first?.kind == .cc && rows.first?.tick == 0)
        #expect(l.lastInsertKind == .cc)
        l.insert()
        #expect(f.edits.count == 2)
        guard case .event(.insert(_, _, let again))? = f.edits.last else { return }
        #expect(again.first?.kind == .cc, "insert repeats the last inserted kind")
        l.insert(.reference)
        #expect(f.edits.count == 2 && m.editor.status == "References and regions are made in the arrangement.")
        l.insert(.tempo)
        #expect(f.edits.count == 2, "a lane target lists no tempo rows")
        #expect(!l.insertKinds.contains(.tempo))
        l.cursor = "e2"
        l.insert(.note)
        guard case .event(.insert(_, _, let at))? = f.edits.last else { return }
        #expect(at.first?.tick == 15_360, "at the cursor row")
    }

    @Test func filtersApplyToAllListsOrBecomeTheDefault() throws {
        let (_, m, l) = try listModel()
        l.toggle(.note)
        #expect(l.rows.isEmpty)
        let w = try #require(m.editor.tearOff(.list, shown: 3))
        let other = m.list(.window(w.id))
        #expect(other.rows.count == 2, "a new list starts from the default")
        l.applyFilterToAll()
        #expect(other.rows.isEmpty)
        l.saveFilterAsDefault()
        #expect(m.listPreferences.defaultFilter.hiddenKinds == [.note])
        l.setFilter(ListFilter())
        #expect(l.rows.count == 2)
        l.toggle(dest: 5)
        #expect(l.filter.shownDests == [6] && !l.shows(dest: 5))
        #expect(l.rows.isEmpty, "Chords plays on destination 5")
        m.editor.openDetail(.lane(seq: 3, lane: 32), kind: .list)
        #expect(l.filter.shownDests == [6], "the filter stays with the list")
    }

    @Test func aPlayedNoteGoesIntoTheEditedPitchOrVelocityCell() throws {
        let (f, _, l) = try listModel()
        l.receive([d4Loud])
        #expect(l.fieldInput == nil, "MIDI In is off")
        l.midiIn = true
        l.editing = ListCell(row: "e1", column: .data1)
        l.receive([d4Loud])
        #expect(l.fieldInput == "D4")
        l.editing = ListCell(row: "e1", column: .data2)
        l.receive([d4Loud])
        #expect(l.fieldInput == "100")
        #expect(f.edits.isEmpty)
    }

    @Test func aPlayedNoteSetsTheCursorNote() throws {
        let (f, _, l) = try listModel()
        l.midiIn = true
        l.cursor = "e1"
        l.receive([d4Loud])
        guard case .event(.replace(1, let row))? = f.edits.last else {
            Issue.record("no replace")
            return
        }
        #expect(row.pitch == 62 && row.velocity == 0xC924)
    }

    @Test func openingAReferenceOpensItsSequence() throws {
        let (f, m, l) = try listModel(.lane(seq: 3, lane: 30))
        var r = ListInsert.blankRow(kind: .reference, tick: 0)
        r.id = 7
        r.refSeq = 2
        f.laneEventTable["3/30"] = LaneEvents(seq: 3, lane: 30, rev: 1, events: [r])
        #expect(l.openReference("e7"))
        #expect(l.target == .sequence(2))
        #expect(m.selectedSeq == 2 && m.editor.navigation == [3])
        #expect(!l.openReference("e1"), "not a reference")
    }

    @Test func noteInputGoesOnlyToListeningDocuments() throws {
        let (f, m, l) = try listModel()
        let input = FakeNoteInput()
        let hub = NoteInputHub(input: input)
        input.queued = [d4Loud]
        hub.poll([m])
        #expect(input.starts == 0 && input.queued.count == 1, "nobody listens")
        l.midiIn = true
        l.cursor = "e1"
        hub.poll([m])
        #expect(input.starts == 1)
        #expect(f.edits.count == 1)
        hub.poll([m])
        #expect(input.starts == 1, "started once")
    }

    @Test func columnsAndSmpteComeFromThePreferences() throws {
        let (_, m, l) = try listModel()
        var c = ListColumns()
        c.smpte = true
        c.fps = 25
        l.setColumns(c)
        #expect(l.columns.contains(.smpte))
        let row = try #require(l.row("e2"))
        #expect(ListText.cell(row, .smpte, l.context) == "00:00:00:12", "a quarter at 120 BPM is half a second")
        #expect(m.listPreferences.columns.fps == 25)
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:" | head -5
```

Expected: `value of type 'DocumentModel' has no member 'list'`.

- [ ] **Step 3: Implement**

```bash
cat > app/Sources/VizDAWAppModel/ListPreferences.swift <<'VIZDAW_EOF'
import Foundation
import Observation

/// The default list filter and the optional columns: app preferences
/// (Plan 4c Ruling 19). Without `defaults` they live in memory only.
@Observable
@MainActor
public final class ListPreferences {
    public static let key = "VizDAW.list"
    @ObservationIgnored private let defaults: UserDefaults?

    private struct Stored: Codable {
        var defaultFilter: ListFilter
        var columns: ListColumns
    }

    public var defaultFilter: ListFilter { didSet { save() } }
    public var columns: ListColumns { didSet { save() } }

    public init(defaults: UserDefaults? = nil) {
        self.defaults = defaults
        let s = defaults?.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(Stored.self, from: $0) }
        defaultFilter = s?.defaultFilter ?? ListFilter()
        columns = s?.columns ?? ListColumns()
    }

    private func save() {
        guard let defaults,
            let data = try? JSONEncoder().encode(Stored(defaultFilter: defaultFilter, columns: columns))
        else { return }
        defaults.set(data, forKey: Self.key)
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWAppModel/NoteInputHub.swift <<'VIZDAW_EOF'
import VizDAWCore

/// Notes played on MIDI sources, handed to the documents whose list editors
/// listen (Plan 4c Ruling 11). The app's 60 Hz timer polls it; nothing is
/// started or read while no list listens.
@MainActor
public final class NoteInputHub {
    private let input: (any NoteInputProtocol)?
    private var started = false

    public init(input: (any NoteInputProtocol)?) {
        self.input = input
    }

    public func poll(_ docs: [DocumentModel]) {
        let listening = docs.filter(\.listensForNotes)
        guard let input, !listening.isEmpty else { return }
        if !started { started = (try? input.start()) != nil }
        guard let notes = try? input.take(), !notes.isEmpty else { return }
        for d in listening { d.receiveNotes(notes) }
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWAppModel/ListEditorModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// A cell of a list editor.
public struct ListCell: Equatable, Sendable {
    public var row: String
    public var column: ListColumn

    public init(row: String, column: ListColumn) {
        self.row = row
        self.column = column
    }
}

/// One list editor (spec §4c): its rows, cursor, filter, inserts and edits.
/// The detail pane and every torn-off window have their own; the document,
/// selection and preferences are shared. Owned by its `DocumentModel`.
@Observable
@MainActor
public final class ListEditorModel {
    public weak var document: DocumentModel?
    public let editor: EditorModel
    public let slot: DetailSlot
    @ObservationIgnored private let read: @MainActor () -> DetailState?
    @ObservationIgnored let write: @MainActor (DetailState) -> Void
    /// The row the keyboard cursor is on.
    public var cursor: String?
    /// The cell being edited (set by the view).
    public var editing: ListCell?
    /// Text a played note puts into the edited cell (the view takes it).
    public var fieldInput: String?
    public private(set) var lastInsertKind: ListKind = .note
    /// Notes played on a MIDI source enter pitch and velocity.
    public var midiIn = false

    private struct RowsKey: Equatable {
        let target: EditorTarget
        let filter: ListFilter
        let version: UInt64
        let revs: [UInt64?]
        let smpte: Bool
    }

    @ObservationIgnored private var cache: (key: RowsKey, rows: [ListRow], seconds: [String: Double])?
    @ObservationIgnored private var destCache: (version: UInt64, rows: [DestRow])?

    public init(
        document: DocumentModel, slot: DetailSlot, read: @escaping @MainActor () -> DetailState?,
        write: @escaping @MainActor (DetailState) -> Void
    ) {
        self.document = document
        self.editor = document.editor
        self.slot = slot
        self.read = read
        self.write = write
    }

    public static func pane(_ document: DocumentModel) -> ListEditorModel {
        ListEditorModel(
            document: document, slot: .pane,
            read: { [weak document] in document?.editor.layout.detail },
            write: { [weak document] d in document?.editor.updateLayout { $0.detail = d } })
    }

    public static func window(_ id: UUID, of document: DocumentModel) -> ListEditorModel {
        ListEditorModel(
            document: document, slot: .window(id),
            read: { [weak document] in document?.editor.layout.windows.first { $0.id == id }?.detail },
            write: { [weak document] d in
                document?.editor.updateLayout { l in
                    if let i = l.windows.firstIndex(where: { $0.id == id }) { l.windows[i].detail = d }
                }
            })
    }

    // MARK: What is shown

    public var state: DetailState? { read() }
    public var target: EditorTarget? { state?.target }
    public var preferences: ListPreferences? { document?.listPreferences }
    public var filter: ListFilter { state?.listFilter ?? preferences?.defaultFilter ?? ListFilter() }
    public var columnSettings: ListColumns { preferences?.columns ?? ListColumns() }

    /// The target's lane, or every lane of a sequence target.
    public var lanes: [LaneKey] {
        guard let t = target else { return [] }
        if let l = t.lane { return [LaneKey(t.seq, l)] }
        return editor.laneRows(t.seq).map { LaneKey(t.seq, $0.id) }
    }

    public var columns: [ListColumn] { columnSettings.visible(lanes: lanes.count) }

    public var rows: [ListRow] { built().rows }

    public func row(_ id: String) -> ListRow? { rows.first { $0.id == id } }

    public func index(of id: String) -> Int? { rows.firstIndex { $0.id == id } }

    public var cursorRow: ListRow? { cursor.flatMap(row) }

    private var range: Range<Int64>? {
        if case let .block(_, _, s, e) = target { return s..<max(e, s + 1) }
        return nil
    }

    private func built() -> (rows: [ListRow], seconds: [String: Double]) {
        guard let t = target else { return ([], [:]) }
        let keys = lanes
        let key = RowsKey(
            target: t, filter: filter, version: editor.snapshotVersion, revs: keys.map { editor.revs[$0] },
            smpte: columnSettings.smpte)
        if let c = cache, c.key == key { return (c.rows, c.seconds) }
        let laneRows = editor.laneRows(t.seq)
        let input = ListInput(
            lanes: keys.map { k in
                ListLane(
                    key: k, events: editor.laneEvents(k)?.events ?? [],
                    dest: laneRows.first { $0.id == k.lane }?.midiDest)
            },
            range: range, info: t.lane == nil ? editor.info(t.seq) : nil)
        let rows = ListRows.build(input, filter: filter)
        var seconds: [String: Double] = [:]
        if columnSettings.smpte, !rows.isEmpty,
            let secs = try? editor.session.secondsAt(seq: t.seq, ticks: rows.map(\.tick)), secs.count == rows.count
        {
            for (r, s) in zip(rows, secs) { seconds[r.id] = s }
        }
        cache = (key, rows, seconds)
        return (rows, seconds)
    }

    /// The instrument list (Destination column and filter), per snapshot.
    public var destinations: [DestRow] {
        if let c = destCache, c.version == editor.snapshotVersion { return c.rows }
        let rows = (try? editor.session.destinations()) ?? destCache?.rows ?? []
        destCache = (editor.snapshotVersion, rows)
        return rows
    }

    public var context: ListContext {
        guard let t = target else { return ListContext(seq: 0) }
        let info = editor.info(t.seq)
        let laneNames = Dictionary(
            editor.laneRows(t.seq).map { (LaneKey(t.seq, $0.id), $0.name) }, uniquingKeysWith: { a, _ in a })
        let dests = Dictionary(destinations.map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
        let seqs = Dictionary(
            (document?.snapshot.sequences ?? []).map { ($0.id, $0.name) }, uniquingKeysWith: { a, _ in a })
        let session = editor.session
        return ListContext(
            seq: t.seq, meter: info?.meter ?? [], info: info, laneNames: laneNames, destNames: dests,
            seqNames: seqs, seconds: built().seconds, fps: columnSettings.fps,
            tickAtSeconds: { s in (try? session.ticksAt(seq: t.seq, seconds: [s]))?.first })
    }

    // MARK: Edits

    /// Commits typed text (Enter or Tab); false when refused (the status
    /// line says why) or the row is gone.
    @discardableResult
    public func commit(_ text: String, row id: String, column: ListColumn) -> Bool {
        guard let r = row(id) else { return false }
        let change: ListChange
        do {
            change = try ListEdit.commit(text, column: column, row: r, context: context)
        } catch let e as ListEditError {
            editor.status = e.message
            return false
        } catch {
            editor.report(error)
            return false
        }
        guard case let .edit(cmd, tick) = change else { return true }
        guard editor.edit(cmd) else { return false }
        follow(r, to: tick)
        return true
    }

    /// The cursor stays on the edited row: an event keeps its ID (and
    /// becomes the selection); a map row is found again by kind and tick.
    func follow(_ r: ListRow, to tick: Int64) {
        if r.event != nil {
            cursor = r.id
            select([r.id])
        } else {
            cursor = rows.first { $0.kind == r.kind && $0.tick == tick }?.id ?? cursor
        }
    }

    /// What Insert offers here: map kinds only for a whole sequence.
    public var insertKinds: [ListKind] {
        target?.lane == nil ? ListKind.insertable : ListKind.insertable.filter { $0.eventKind != nil }
    }

    /// Inserts `kind`, or the last inserted kind (Plan 4c Ruling 18).
    public func insert(_ kind: ListKind? = nil) {
        guard let t = target else { return }
        let k = kind ?? lastInsertKind
        if k == .reference || k == .audioRegion {
            editor.status = "References and regions are made in the arrangement."
            return
        }
        guard insertKinds.contains(k) else {
            editor.status = "Open the whole sequence to add tempo, meter or marker rows."
            return
        }
        let tick = insertTick()
        let lane = t.lane.map { LaneKey(t.seq, $0) } ?? cursorRow?.lane ?? lanes.first
        guard let cmd = ListInsert.command(k, tick: tick, lane: lane, seq: t.seq, info: editor.info(t.seq)) else {
            editor.status = k == .meter ? "There is already a meter change at the next barline." : "There is no lane to insert into."
            return
        }
        let before = Set(rows.compactMap(\.event).map(\.id))
        guard editor.edit(cmd) else { return }
        lastInsertKind = k
        let made =
            k.eventKind == nil
            ? rows.first { $0.kind == k && $0.tick >= tick }
            : rows.first { r in r.event.map { !before.contains($0.id) } ?? false }
        if let made {
            cursor = made.id
            if made.event != nil { select([made.id]) }
        }
    }

    /// The cursor row's tick, else the stopped transport's when it lies in
    /// the target, else the target's start.
    func insertTick() -> Int64 {
        if let r = cursorRow { return r.tick }
        if let d = document, !d.isPlaying, range?.contains(d.transport.tick) ?? true { return d.transport.tick }
        return range?.lowerBound ?? 0
    }

    public func delete(_ ids: [String]) {
        guard let t = target else { return }
        let set = Set(ids)
        let chosen = rows.filter { set.contains($0.id) }
        for cmd in ListDelete.commands(chosen, seq: t.seq, info: editor.info(t.seq)) {
            guard editor.edit(cmd) else { return }
        }
        cursor = nil
    }

    /// Selected rows become the document selection (their events and lanes).
    public func select(_ ids: [String]) {
        let set = Set(ids)
        let chosen = rows.filter { set.contains($0.id) }
        var lanes: [LaneRef] = []
        for r in chosen {
            if let l = r.lane, !lanes.contains(l.ref) { lanes.append(l.ref) }
        }
        editor.setSelection(SelectionRow(timeRange: nil, lanes: lanes, events: chosen.compactMap(\.event).map(\.id)))
    }

    /// A reference row opens its sequence in this list (Plan 4c Ruling 24).
    @discardableResult
    public func openReference(_ id: String) -> Bool {
        guard let e = row(id)?.event, e.kind == .reference, let seq = e.refSeq, var s = state else { return false }
        editor.openSequence(seq)
        s.target = .sequence(seq)
        s.lanes = nil
        write(s)
        cursor = nil
        return true
    }

    /// Notes played on a MIDI source (Plan 4c Ruling 12).
    public func receive(_ notes: [NoteInRow]) {
        guard midiIn, let n = notes.last else { return }
        if let cell = editing {
            guard let r = row(cell.row) else { return }
            if cell.column == .data1, r.kind == .note || r.kind == .polyPressure {
                fieldInput = PitchName.name(n.pitch)
            } else if cell.column == .data2, r.kind == .note {
                fieldInput = "\(Velocity7.of(n.velocity))"
            }
            return
        }
        guard let r = cursorRow, var e = r.event, e.kind == .note else { return }
        e.pitch = n.pitch
        e.velocity = n.velocity
        if editor.edit(.event(edit: .replace(event: e.id, row: e))) { follow(r, to: e.tick) }
    }

    // MARK: Filters and columns (Plan 4c Ruling 19)

    public func setFilter(_ f: ListFilter) {
        guard var s = state else { return }
        s.listFilter = f
        write(s)
    }

    public func toggle(_ kind: ListKind) {
        var f = filter
        if f.hiddenKinds.contains(kind) { f.hiddenKinds.remove(kind) } else { f.hiddenKinds.insert(kind) }
        setFilter(f)
    }

    public func shows(dest: UInt64) -> Bool { filter.shownDests?.contains(dest) ?? true }

    public func toggle(dest: UInt64) {
        var f = filter
        let all = Set(destinations.map(\.id))
        var shown = f.shownDests ?? all
        if shown.contains(dest) { shown.remove(dest) } else { shown.insert(dest) }
        f.shownDests = shown == all ? nil : shown
        setFilter(f)
    }

    /// Every list of this document (the pane's and each window's) takes
    /// this list's filter.
    public func applyFilterToAll() {
        let f = filter
        editor.updateLayout { l in
            if l.detail?.kind == .list { l.detail?.listFilter = f }
            for i in l.windows.indices where l.windows[i].detail.kind == .list { l.windows[i].detail.listFilter = f }
        }
    }

    public func saveFilterAsDefault() { preferences?.defaultFilter = filter }

    public func setColumns(_ c: ListColumns) { preferences?.columns = c }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/EditorTypes.swift`, Find:

```swift
    public var lanes: [UInt64]?

    public init(kind: DetailEditorKind, target: EditorTarget, lanes: [UInt64]? = nil) {
        self.kind = kind
        self.target = target
        self.lanes = lanes
    }
```

Replace:

```swift
    public var lanes: [UInt64]?
    /// List editor: its kind and destination filter (nil: the default).
    /// Optional, so earlier layouts decode (Plan 4c Ruling 19).
    public var listFilter: ListFilter?

    public init(kind: DetailEditorKind, target: EditorTarget, lanes: [UInt64]? = nil, listFilter: ListFilter? = nil) {
        self.kind = kind
        self.target = target
        self.lanes = lanes
        self.listFilter = listFilter
    }
```

In `app/Sources/VizDAWAppModel/EditorModel.swift`, Find:

```swift
            let lanes = l.detail?.target == target ? l.detail?.lanes : nil
            l.detail = DetailState(kind: k, target: target, lanes: lanes)
```

Replace:

```swift
            let lanes = l.detail?.target == target ? l.detail?.lanes : nil
            l.detail = DetailState(kind: k, target: target, lanes: lanes, listFilter: l.detail?.listFilter)
```

Find:

```swift
    public func push(_ parent: UInt64) { navigation.append(parent) }
```

Replace:

```swift
    /// Shows sequence `seq`, remembering the current one for Back (a
    /// reference opened from a list, Plan 4c Ruling 24).
    public func openSequence(_ seq: UInt64) {
        if let current = currentSequence() { push(current) }
        selectSequence(seq)
    }

    public func push(_ parent: UInt64) { navigation.append(parent) }
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`, Find:

```swift
    /// A torn-off window closed: its piano roll goes, silencing its note.
    public func dropPianoRoll(_ slot: DetailSlot) {
        pianoRolls.removeValue(forKey: slot)?.silence()
    }
```

Replace:

```swift
    /// A torn-off window closed: its piano roll goes, silencing its note.
    public func dropPianoRoll(_ slot: DetailSlot) {
        pianoRolls.removeValue(forKey: slot)?.silence()
    }

    /// The list default filter and columns (set by `DocumentStore`).
    @ObservationIgnored public var listPreferences = ListPreferences()
    /// One list editor per slot.
    @ObservationIgnored private var lists: [DetailSlot: ListEditorModel] = [:]
    /// The list the keyboard was last in (Insert Event, played notes).
    @ObservationIgnored public var activeList: DetailSlot?

    /// The list editor of the detail pane or a torn-off window, made once.
    public func list(_ slot: DetailSlot) -> ListEditorModel {
        if let l = lists[slot] { return l }
        let l: ListEditorModel
        switch slot {
        case .pane: l = .pane(self)
        case .window(let id): l = .window(id, of: self)
        }
        lists[slot] = l
        return l
    }

    public func dropList(_ slot: DetailSlot) {
        lists.removeValue(forKey: slot)
        if activeList == slot { activeList = nil }
    }

    /// The active list while its slot shows a list.
    public func listForCommands() -> ListEditorModel? {
        guard let slot = activeList, let l = lists[slot], l.state?.kind == .list else { return nil }
        return l
    }

    public var listensForNotes: Bool { lists.values.contains { $0.midiIn } }

    /// Played notes go to the active list when it listens, else to a
    /// listening one.
    public func receiveNotes(_ notes: [NoteInRow]) {
        let active = listForCommands().flatMap { $0.midiIn ? $0 : nil }
        (active ?? lists.values.first { $0.midiIn })?.receive(notes)
    }
```

In `app/Sources/VizDAWAppModel/DocumentStore.swift`, Find:

```swift
    /// Makes the demo with an audio lane (Plan 4b Ruling 19); set by `live()`.
    public var makeDemoAudio: Make?
```

Replace:

```swift
    /// Makes the demo with an audio lane (Plan 4b Ruling 19); set by `live()`.
    public var makeDemoAudio: Make?

    /// List defaults and columns given to every document (Plan 4c Ruling 19).
    public var listPreferences = ListPreferences()
```

Find:

```swift
        s.makeDemoAudio = { try Session.newDemoAudio() }
        return s
```

Replace:

```swift
        s.makeDemoAudio = { try Session.newDemoAudio() }
        s.listPreferences = ListPreferences(defaults: .standard)
        return s
```

Find:

```swift
            let model = try DocumentModel(session: make())
            let id = UUID()
```

Replace:

```swift
            let model = try DocumentModel(session: make())
            model.listPreferences = listPreferences
            let id = UUID()
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | head; scripts/swift.sh test 2>&1 | tail -3
```

Expected: no new warnings; 377 tests pass (366 + 11).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/ListPreferences.swift app/Sources/VizDAWAppModel/ListEditorModel.swift app/Sources/VizDAWAppModel/NoteInputHub.swift app/Sources/VizDAWAppModel/EditorTypes.swift app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/DocumentStore.swift app/Tests/VizDAWAppModelTests/ListEditorModelTests.swift
git commit -m "feat(app): list editor model with filters, inserts, follow and played notes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 10: `ListEditorView`, the list pane, Insert Event and the MIDI ticker

**Files:**
- Create: `app/Sources/VizDAWEditors/ListEditorView.swift`
- Create: `app/Sources/VizDAW/ListEditorPane.swift`
- Modify: `app/Sources/VizDAW/DetailPane.swift`
- Modify: `app/Sources/VizDAW/EditorWindows.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Test: `app/Tests/VizDAWAppModelTests/ListEditorViewTests.swift` (new)

**Interfaces:**
- Consumes: Task 9's `ListEditorModel`, `DocumentModel.{list, dropList, activeList, listForCommands}`, `NoteInputHub`; Task 6's `beginTextEditing`/`endTextEditing`, `listFocused`; generated `NoteInput`.
- Produces: `public final class ListEditorView: NSView` with `static let textOwner = "list"`, `init(model: ListEditorModel)`, and internal test entry points `table: ListTable`, `shown: [ListRow]`, `shownColumns: [ListColumn]`, `reload()`, `began(row:column:field:)`, `ended(movement:cancelled:)`, `applyInput()`, `key(_:modifiers:) -> Bool`, `focusChanged(_:)`; SwiftUI `ListEditorPane(model:)`; Editors > Insert Event ⌘I; `noteInputHub` polled by the 60 Hz ticker; `DocumentModel.activeList` becomes observed (menus enable on focus).

Rulings 18, 20, 24. The table is a standard `NSTableView` (VoiceOver reads a table with column headers; each row's label is `ListText.summary`). Editing a cell turns the transport keys off (`"list"` owner) until the edit ends, however it ends (commit, Escape, the view leaving its window). The table does not reload while a cell is edited; it reloads when the edit ends.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/ListEditorViewTests.swift`:

```swift
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

@MainActor
private func listView(_ target: EditorTarget = .lane(seq: 3, lane: 31)) throws
    -> (FakeSession, DocumentModel, ListEditorView)
{
    let f = rollFake()
    let m = try DocumentModel(session: f)
    m.editor.openDetail(target, kind: .list)
    let v = ListEditorView(model: m.list(.pane))
    v.frame = NSRect(x: 0, y: 0, width: 900, height: 300)
    v.layout()
    return (f, m, v)
}

@MainActor
private func cell(_ v: ListEditorView, row: Int, column: Int) -> String? {
    (v.tableView(v.table, viewFor: v.table.tableColumns[column], row: row) as? NSTextField)?.stringValue
}

@MainActor
@Suite struct ListEditorViewTests {
    @Test func rowsAndColumnsShow() throws {
        let (_, _, v) = try listView()
        #expect(v.shown.map(\.id) == ["e1", "e2"])
        #expect(v.table.tableColumns.map(\.title) == ["Position", "Length", "Kind", "Destination", "Data 1", "Data 2", "Mute"])
        #expect(v.numberOfRows(in: v.table) == 2)
        #expect(cell(v, row: 1, column: 0) == "1.2.0")
        #expect(cell(v, row: 1, column: 4) == "E4")
    }

    @Test func editingACellTurnsTheTransportKeysOffUntilItEnds() throws {
        let (f, m, v) = try listView()
        let field = NSTextField(string: "1.1.0")
        v.began(row: "e1", column: .position, field: field)
        #expect(!m.transportKeysEnabled)
        #expect(m.list(.pane).editing == ListCell(row: "e1", column: .position))
        field.stringValue = "2"
        v.ended(movement: .return)
        #expect(m.transportKeysEnabled)
        #expect(f.edits.last == .event(edit: .move(events: [1], delta: 61_440, toLane: nil)))
        #expect(m.list(.pane).cursor == "e1", "the list follows the moved event")
        #expect(m.list(.pane).editing == nil)
    }

    @Test func escapeLeavesTheEventAlone() throws {
        let (f, m, v) = try listView()
        let field = NSTextField(string: "1.1.0")
        v.began(row: "e1", column: .position, field: field)
        field.stringValue = "9"
        v.ended(movement: .cancel, cancelled: true)
        #expect(f.edits.isEmpty)
        #expect(!m.editor.textEditing)
    }

    @Test func leavingTheWindowEndsTheEdit() throws {
        let (f, m, v) = try listView()
        v.began(row: "e1", column: .position, field: NSTextField(string: "1.1.0"))
        v.viewWillMove(toWindow: nil)
        #expect(!m.editor.textEditing && f.edits.isEmpty)
    }

    @Test func aPlayedNoteFillsTheEditedCell() throws {
        let (_, m, v) = try listView()
        let l = m.list(.pane)
        l.midiIn = true
        let field = NSTextField(string: "C4")
        v.began(row: "e1", column: .data1, field: field)
        l.receive([NoteInRow(pitch: 62, velocity: 0xC924, channel: 0)])
        v.applyInput()
        #expect(field.stringValue == "D4")
        #expect(l.fieldInput == nil)
    }

    @Test func keysDeleteAndOpenReferencesAndFocusPicksTheList() throws {
        let (f, m, v) = try listView()
        v.table.selectRowIndexes(IndexSet(integer: 0), byExtendingSelection: false)
        #expect(m.list(.pane).cursor == "e1")
        #expect(!v.key(125, modifiers: .command), "⌘↓ on a note opens nothing")
        #expect(v.key(51, modifiers: []))
        #expect(f.edits.last == .event(edit: .delete(events: [1])))
        v.focusChanged(true)
        #expect(m.activeList == .pane && m.listForCommands() === m.list(.pane))
        #expect(!m.returnKeyEnabled && m.transportKeysEnabled)
        v.focusChanged(false)
        #expect(m.returnKeyEnabled)
    }

    @Test func theTableIsAnAccessibleTable() throws {
        let (_, _, v) = try listView()
        #expect(v.table.accessibilityLabel() == "Event List")
        #expect(v.tableView(v.table, rowViewForRow: 0)?.accessibilityLabel() == "Note C4, bar 1 beat 1, velocity 64")
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:" | head -5
```

Expected: `cannot find 'ListEditorView' in scope`.

- [ ] **Step 3: Write the view**

```bash
cat > app/Sources/VizDAWEditors/ListEditorView.swift <<'VIZDAW_EOF'
import AppKit
import Observation
import VizDAWAppModel
import VizDAWCore

/// The list's table: hands keys and focus changes to its view first.
public final class ListTable: NSTableView {
    weak var owner: ListEditorView?

    public override func keyDown(with e: NSEvent) {
        if owner?.key(e.keyCode, modifiers: e.modifierFlags) != true { super.keyDown(with: e) }
    }

    public override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { owner?.focusChanged(true) }
        return ok
    }

    public override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { owner?.focusChanged(false) }
        return ok
    }
}

/// The list editor (spec §4c): one row per event (and tempo, meter and
/// marker entry), keyboard-first, edited in place. An `NSTableView`, so
/// VoiceOver reads it as a table (Plan 4c Ruling 20).
@MainActor
public final class ListEditorView: NSView, NSTableViewDataSource, NSTableViewDelegate, NSTextFieldDelegate {
    /// The text-editing owner while a cell is edited (Plan 4c Ruling 21).
    public static let textOwner = "list"
    public let model: ListEditorModel
    let scroll = NSScrollView()
    let table = ListTable()
    private(set) var shown: [ListRow] = []
    private(set) var shownColumns: [ListColumn] = []
    private var context: ListContext
    /// The cell being edited: row, column, field and the text it started with.
    private(set) var editingCell: (row: String, column: ListColumn, field: NSTextField, original: String)?
    /// Selection changes made here, not by the user.
    private var syncing = false

    public init(model: ListEditorModel) {
        self.model = model
        context = model.context
        super.init(frame: .zero)
        table.owner = self
        table.dataSource = self
        table.delegate = self
        table.allowsMultipleSelection = true
        table.usesAlternatingRowBackgroundColors = true
        table.rowHeight = 18
        table.style = .plain
        table.target = self
        table.doubleAction = #selector(doubleClicked)
        table.setAccessibilityLabel("Event List")
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        addSubview(scroll)
        observeRows()
        observeCursor()
        reload()
    }

    public required init?(coder: NSCoder) { nil }

    public override func layout() {
        super.layout()
        scroll.frame = bounds
    }

    // MARK: Observation

    private func observeRows() {
        withObservationTracking {
            _ = model.rows
            _ = model.columns
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.observeRows()
                self?.reload()
            }
        }
    }

    private func observeCursor() {
        withObservationTracking {
            _ = model.cursor
            _ = model.fieldInput
        } onChange: { [weak self] in
            Task { @MainActor in
                self?.observeCursor()
                self?.applyInput()
                self?.syncCursor()
            }
        }
    }

    // MARK: Rows

    /// Re-reads rows and columns; waits while a cell is edited.
    func reload() {
        guard editingCell == nil else { return }
        let cols = model.columns
        if cols != shownColumns { rebuildColumns(cols) }
        shown = model.rows
        context = model.context
        syncing = true
        table.reloadData()
        syncing = false
        syncCursor()
    }

    private func rebuildColumns(_ cols: [ListColumn]) {
        for c in table.tableColumns { table.removeTableColumn(c) }
        for c in cols {
            let tc = NSTableColumn(identifier: NSUserInterfaceItemIdentifier(c.rawValue))
            tc.title = c.title
            tc.width = Self.width(c)
            table.addTableColumn(tc)
        }
        shownColumns = cols
    }

    static func width(_ c: ListColumn) -> CGFloat {
        switch c {
        case .position, .end: 90
        case .duration: 70
        case .kind: 110
        case .lane: 90
        case .dest: 110
        case .data1: 90
        case .data2: 140
        case .release: 60
        case .smpte: 100
        case .mute: 44
        }
    }

    /// The table's selection and scroll follow the model's cursor.
    func syncCursor() {
        guard let c = model.cursor, let i = shown.firstIndex(where: { $0.id == c }) else { return }
        if !table.selectedRowIndexes.contains(i) {
            syncing = true
            table.selectRowIndexes(IndexSet(integer: i), byExtendingSelection: false)
            syncing = false
        }
        table.scrollRowToVisible(i)
    }

    /// A played note's text goes into the edited cell (Plan 4c Ruling 12).
    func applyInput() {
        guard let text = model.fieldInput else { return }
        model.fieldInput = nil
        guard let e = editingCell else { return }
        if let editor = e.field.currentEditor() {
            editor.string = text
            editor.selectAll(nil)
        } else {
            e.field.stringValue = text
        }
    }

    // MARK: Table

    public func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

    public func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        guard let tableColumn, let column = ListColumn(rawValue: tableColumn.identifier.rawValue),
            shown.indices.contains(row)
        else { return nil }
        let r = shown[row]
        let field =
            (tableView.makeView(withIdentifier: tableColumn.identifier, owner: self) as? NSTextField)
            ?? makeField(tableColumn.identifier)
        field.stringValue = ListText.cell(r, column, context)
        field.isEditable = ListText.editable(r, column)
        field.textColor = field.isEditable ? .labelColor : .secondaryLabelColor
        return field
    }

    private func makeField(_ id: NSUserInterfaceItemIdentifier) -> NSTextField {
        let f = NSTextField()
        f.identifier = id
        f.isBordered = false
        f.drawsBackground = false
        f.font = .monospacedDigitSystemFont(ofSize: 12, weight: .regular)
        f.lineBreakMode = .byTruncatingTail
        f.delegate = self
        return f
    }

    public func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        let v = NSTableRowView()
        if shown.indices.contains(row) { v.setAccessibilityLabel(ListText.summary(shown[row], context)) }
        return v
    }

    public func tableViewSelectionDidChange(_ notification: Notification) {
        guard !syncing else { return }
        let ids = table.selectedRowIndexes.compactMap { shown.indices.contains($0) ? shown[$0].id : nil }
        let r = table.selectedRow
        model.cursor = shown.indices.contains(r) ? shown[r].id : nil
        model.select(ids)
    }

    // MARK: Editing

    /// Starts editing a cell (Return, Tab, double-click).
    func beginEditing(row index: Int, column: ListColumn) {
        guard shown.indices.contains(index), let c = shownColumns.firstIndex(of: column),
            ListText.editable(shown[index], column)
        else { return }
        table.editColumn(c, row: index, with: nil, select: true)
    }

    public func controlTextDidBeginEditing(_ obj: Notification) {
        guard let field = obj.object as? NSTextField else { return }
        let r = table.row(for: field)
        let c = table.column(for: field)
        guard shown.indices.contains(r), shownColumns.indices.contains(c) else { return }
        began(row: shown[r].id, column: shownColumns[c], field: field)
    }

    func began(row: String, column: ListColumn, field: NSTextField) {
        editingCell = (row, column, field, field.stringValue)
        model.editing = ListCell(row: row, column: column)
        model.editor.beginTextEditing(Self.textOwner)
    }

    public func controlTextDidEndEditing(_ obj: Notification) {
        let raw = (obj.userInfo?["NSTextMovement"] as? Int) ?? NSTextMovement.other.rawValue
        ended(movement: NSTextMovement(rawValue: raw) ?? .other)
    }

    /// Editing ended: commit unless cancelled, then move on (Tab and
    /// Shift-Tab to the next editable cell, else back to the table).
    func ended(movement: NSTextMovement, cancelled: Bool = false) {
        guard let e = editingCell else { return }
        editingCell = nil
        model.editing = nil
        model.editor.endTextEditing(Self.textOwner)
        let text = e.field.stringValue
        if !cancelled, text != e.original { model.commit(text, row: e.row, column: e.column) }
        reload()
        switch movement {
        case .tab, .backtab: editNext(after: e.row, column: e.column, forward: movement == .tab)
        default: window?.makeFirstResponder(table)
        }
    }

    private func editNext(after row: String, column: ListColumn, forward: Bool) {
        guard let i = shown.firstIndex(where: { $0.id == row }), let c = shownColumns.firstIndex(of: column) else {
            return
        }
        let order = forward ? Array(shownColumns[(c + 1)...]) : Array(shownColumns[..<c].reversed())
        guard let next = order.first(where: { ListText.editable(shown[i], $0) }) else {
            window?.makeFirstResponder(table)
            return
        }
        beginEditing(row: i, column: next)
    }

    public func control(_ control: NSControl, textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        guard selector == #selector(NSResponder.cancelOperation(_:)), let e = editingCell else { return false }
        e.field.stringValue = e.original
        ended(movement: .cancel, cancelled: true)
        return true
    }

    // MARK: Keys and focus (Plan 4c Ruling 20)

    /// True when the key was handled here.
    func key(_ code: UInt16, modifiers: NSEvent.ModifierFlags) -> Bool {
        let row = table.selectedRow
        switch code {
        case 36, 76:  // Return, Enter
            guard shown.indices.contains(row) else { return false }
            beginEditing(row: row, column: .position)
            return true
        case 51, 117:  // Delete, Forward Delete
            let ids = table.selectedRowIndexes.compactMap { shown.indices.contains($0) ? shown[$0].id : nil }
            guard !ids.isEmpty else { return false }
            model.delete(ids)
            return true
        case 125 where modifiers.contains(.command):  // ⌘↓
            guard shown.indices.contains(row) else { return false }
            return model.openReference(shown[row].id)
        default:
            return false
        }
    }

    func focusChanged(_ on: Bool) {
        model.editor.listFocused = on
        if on { model.document?.activeList = model.slot }
    }

    @objc func doubleClicked() {
        let r = table.clickedRow
        let c = table.clickedColumn
        guard shown.indices.contains(r), shownColumns.indices.contains(c) else { return }
        let column = shownColumns[c]
        if shown[r].kind == .reference, column == .kind || column == .data1 {
            model.openReference(shown[r].id)
        } else {
            beginEditing(row: r, column: column)
        }
    }

    /// Leaving the window ends an edit unsaved and gives Return back.
    public override func viewWillMove(toWindow newWindow: NSWindow?) {
        super.viewWillMove(toWindow: newWindow)
        guard newWindow == nil else { return }
        if editingCell != nil { ended(movement: .cancel, cancelled: true) }
        model.editor.listFocused = false
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: The pane, the menu item and the ticker**

```bash
cat > app/Sources/VizDAW/ListEditorPane.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The list editor (spec §4c) with its bar.
struct ListEditorPane: View {
    let model: ListEditorModel

    var body: some View {
        VStack(spacing: 0) {
            ListBar(model: model)
            Divider()
            ListHost(model: model)
        }
    }
}

struct ListHost: NSViewRepresentable {
    let model: ListEditorModel

    func makeNSView(context: Context) -> ListEditorView { ListEditorView(model: model) }

    func updateNSView(_ view: ListEditorView, context: Context) {}
}

/// Insert, columns, what is shown, and MIDI In (Plan 4c Rulings 14, 18, 19).
struct ListBar: View {
    @Bindable var model: ListEditorModel

    private func column(_ key: WritableKeyPath<ListColumns, Bool>) -> Binding<Bool> {
        Binding(
            get: { model.columnSettings[keyPath: key] },
            set: { on in
                var c = model.columnSettings
                c[keyPath: key] = on
                model.setColumns(c)
            })
    }

    private var fps: Binding<Int> {
        Binding(
            get: { model.columnSettings.fps },
            set: { f in
                var c = model.columnSettings
                c.fps = f
                model.setColumns(c)
            })
    }

    var body: some View {
        HStack(spacing: 10) {
            Menu {
                ForEach(model.insertKinds, id: \.self) { k in
                    Button(k.title) { model.insert(k) }
                }
            } label: {
                Text("Insert \(model.lastInsertKind.title)")
            } primaryAction: {
                model.insert()
            }
            .frame(width: 190)
            .help("Insert the last inserted kind (⌘I); the menu chooses another")
            Menu("Columns") {
                Toggle("End", isOn: column(\.end))
                Toggle("Length", isOn: column(\.duration))
                Toggle("Release Velocity", isOn: column(\.release))
                Toggle("SMPTE", isOn: column(\.smpte))
                Picker("SMPTE Frame Rate", selection: fps) {
                    ForEach(Smpte.rates, id: \.self) { Text("\($0) fps").tag($0) }
                }
            }
            .frame(width: 100)
            Menu("Show") {
                ForEach(ListKind.allCases, id: \.self) { k in
                    Toggle(
                        k.title,
                        isOn: Binding(get: { !model.filter.hiddenKinds.contains(k) }, set: { _ in model.toggle(k) }))
                }
                Divider()
                ForEach(model.destinations, id: \.id) { d in
                    Toggle(
                        d.name,
                        isOn: Binding(get: { model.shows(dest: d.id) }, set: { _ in model.toggle(dest: d.id) }))
                }
                Divider()
                Button("Show Everything") { model.setFilter(ListFilter()) }
                Button("Apply to All Lists") { model.applyFilterToAll() }
                Button("Save as Default") { model.saveFilterAsDefault() }
            }
            .frame(width: 80)
            .help("The kinds and destinations this list shows")
            Toggle("MIDI In", isOn: $model.midiIn)
                .help("Play a note to enter its pitch and velocity")
            Spacer()
            Text("\(model.rows.count) rows").foregroundStyle(.secondary).font(.caption)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/DetailPane.swift`, Find:

```swift
    /// This frame's piano roll (the pane's or the torn window's).
    let roll: PianoRollModel
```

Replace:

```swift
    /// This frame's piano roll (the pane's or the torn window's).
    let roll: PianoRollModel
    /// This frame's list editor.
    let list: ListEditorModel
```

Find:

```swift
            if state.kind == .pianoRoll {
                PianoRollEditor(model: roll)
            } else {
```

Replace:

```swift
            if state.kind == .pianoRoll {
                PianoRollEditor(model: roll)
            } else if state.kind == .list {
                ListEditorPane(model: list)
            } else {
```

Find:

```swift
                model: model, state: d, roll: model.pianoRoll(.pane),
```

Replace:

```swift
                model: model, state: d, roll: model.pianoRoll(.pane), list: model.list(.pane),
```

In `app/Sources/VizDAW/EditorWindows.swift`, Find:

```swift
                    model: model, state: w.detail, roll: model.pianoRoll(.window(id)),
```

Replace:

```swift
                    model: model, state: w.detail, roll: model.pianoRoll(.window(id)),
                    list: model.list(.window(id)),
```

Find:

```swift
            model.dropPianoRoll(.window(window.id))
```

Replace:

```swift
            model.dropPianoRoll(.window(window.id))
            model.dropList(.window(window.id))
```

In `app/Sources/VizDAW/AppCommands.swift`, Find:

```swift
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
```

Replace:

```swift
            Button("Close Detail Editor") { doc?.editor.closeDetail() }
                .disabled(doc?.editor.layout.detail == nil)
            Button("Insert Event") { doc?.listForCommands()?.insert() }
                .keyboardShortcut("i")
                .disabled(doc?.listForCommands() == nil || doc?.editor.textEditing == true)
```

In `app/Sources/VizDAW/VizDAWApp.swift`, Find:

```swift
/// Editing preferences: the nudge steps (Plan 4b Ruling 16).
@MainActor let editingPreferences = EditingPreferences()
```

Replace:

```swift
/// Editing preferences: the nudge steps (Plan 4b Ruling 16).
@MainActor let editingPreferences = EditingPreferences()

/// Notes played on MIDI sources, for list editors with MIDI In on (Plan 4c
/// Ruling 11).
@MainActor let noteInputHub = NoteInputHub(input: NoteInput())
```

Find:

```swift
                for model in store.documents.values { model.idle() }
```

Replace:

```swift
                for model in store.documents.values { model.idle() }
                noteInputHub.poll(Array(store.documents.values))
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`, Find:

```swift
    /// The list the keyboard was last in (Insert Event, played notes).
    @ObservationIgnored public var activeList: DetailSlot?
```

Replace:

```swift
    /// The list the keyboard was last in (Insert Event, played notes);
    /// observed, so Editors > Insert Event enables when a list gets focus.
    public var activeList: DetailSlot?
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | head; scripts/swift.sh test 2>&1 | tail -3
```

Expected: no new warnings; 384 tests pass (377 + 7). Then run the app once (`VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --demo`), double-click the Chords block and press ⌘2: the list shows the chords' notes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWEditors/ListEditorView.swift app/Sources/VizDAW/ListEditorPane.swift app/Sources/VizDAW/DetailPane.swift app/Sources/VizDAW/EditorWindows.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/VizDAWApp.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Tests/VizDAWAppModelTests/ListEditorViewTests.swift
git commit -m "feat(app): list editor view and pane, Insert Event, MIDI note input ticker

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 11: The panel's configuration, fields and exact-mode mapping (pure)

**Files:**
- Create: `app/Sources/VizDAWAppModel/SelectModify.swift`
- Create: `app/Sources/VizDAWAppModel/PanelFields.swift`
- Create: `app/Sources/VizDAWAppModel/ExactMode.swift`
- Test: `app/Tests/VizDAWAppModelTests/SelectModifyTests.swift` (new)
- Test: `app/Tests/VizDAWAppModelTests/ExactModeTests.swift` (new)

**Interfaces:**
- Consumes: Task 4's generated records (`RuleRow`, `ConditionRow`, `ConditionTest`, `SelectModeRow`, `ModifyRow`, `ChangeOpRow`, `AmountRow`, `CurveRow`, `WaveRow`, `ValueTargetRow`, `SubstituteRow`, `NoteMapEntry`, `ModifyTemplateRow`, `EditCommand.selectAndModify`); Task 7 (`Bbu`, `PositionInput`, `DurationText`, `Velocity7`, `Value7`, `PitchName.parse`); `Scale`.
- Produces:
  - `enum PanelTask: String, CaseIterable, Sendable { selectByRule, transpose, quantize, change, move }` with `title`.
  - `struct SelectHalf { enabled, matchAll, mode: SelectModeRow, conditions: [ConditionRow] }`, `struct ModifyHalf { enabled, op: ModifyRow, toCopy }`, `struct SelectModifyConfig { select, modify; static func preset(_:range:) ; var rule: RuleRow?; var modification: ModifyRow?; func command() -> EditCommand?; init(template:); func template(named:) -> ModifyTemplateRow }`.
  - `enum ConditionKind` (12 cases) with `title`, `init(_ t: ConditionTest)`, `func test(range: TickRange?) -> ConditionTest`; `enum Comparison { equal, notEqual, inside, outside }` with `title`, `static func choices(for:)`, `static func of(_:)`, `func applied(to:)`.
  - `enum ModifyKind` (15 cases) with `title`, `init(_ r: ModifyRow)`, `row` (default), `static func title(_ r: ModifyRow) -> String` (the undo label's wording); `enum ChangeOpKind` (`set, scale, add, limit, compress, legato`) with `init(_:)`, `func row(for: ValueTargetRow) -> ChangeOpRow`; `enum AmountKind` (`constant, line, oscillator, random`) with `init(_:)`, `func row(seed: UInt64) -> AmountRow`; `extension Scale { static func of(mask: UInt16) -> Scale? }`.
  - `enum Edge { low, high, delta, exact }`, `enum FieldKind { count, percent, pitch, velocity(Edge), value(Edge), length, position }`, `struct Field { label, kind }`, `enum FieldText { text(_:_:meter:), parse(_:_:meter:) }`; `ConditionTest.{fields, numbers, with(_:)}`, `ModifyRow.{fields, numbers, with(_:)}`; `enum NoteMapText`, `enum OffsetsText` (`text`/`parse`).
  - `struct ModifyRequest: Equatable, Sendable { task, config, events: [UInt64]?, lanes: [LaneKey], range: TickRange? }`; `enum ExactMode { static func request(for: EditCommand, lanes: Set<LaneKey>) -> ModifyRequest?; static func selectRequest(range:pitches:lanes:adding:) -> ModifyRequest }`.

Rulings 3, 4, 7, 23. Velocity and value fields show 0–127: range lows and highs cover the whole 7-bit step (`v << 9` … `v << 9 | 511`), exact values use the MIDI 2.0 upscale, deltas are steps of 512 (or 2²⁵). Lengths are beats.units of a quarter note, signed for moves.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/SelectModifyTests.swift`:

```swift
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let q: Int64 = 15_360
private let meter44 = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

@Suite struct SelectModifyTests {
    @Test func presetsMakeTheirTasksCommand() {
        #expect(
            SelectModifyConfig.preset(.transpose).command()
                == .selectAndModify(rule: nil, mode: .select, modify: .transposeInterval(semitones: 0), toCopy: false))
        #expect(SelectModifyConfig.preset(.quantize).modify.op == .quantize(grid: q / 4, strength: 100, offsets: []))
        #expect(SelectModifyConfig.preset(.move).modify.op == .move(delta: 0))
        let rule = SelectModifyConfig.preset(.selectByRule, range: TickRange(start: 0, end: 4 * q))
        #expect(
            rule.command()
                == .selectAndModify(
                    rule: RuleRow(
                        conditions: [
                            ConditionRow(test: .kinds(kinds: [.note]), negate: false),
                            ConditionRow(test: .timeRange(start: 0, end: 4 * q), negate: false),
                        ], matchAll: true),
                    mode: .select, modify: nil, toCopy: false))
    }

    @Test func nothingTurnedOnIsNoCommand() {
        var c = SelectModifyConfig.preset(.change)
        c.modify.enabled = false
        #expect(c.command() == nil)
        c.select.enabled = true
        #expect(c.command() == nil, "a select half without conditions does nothing")
    }

    @Test func templatesRoundTrip() {
        var c = SelectModifyConfig.preset(.selectByRule, range: TickRange(start: 0, end: q))
        c.select.mode = .refine
        c.select.matchAll = false
        c.modify.enabled = true
        c.modify.op = .change(target: .velocity, op: .add, amount: .line(from: 0, to: 2048, curve: .easeIn))
        c.modify.toCopy = true
        let t = c.template(named: "Swell")
        #expect(t.name == "Swell" && t.toCopy && t.mode == .refine)
        #expect(SelectModifyConfig(template: t) == c)
    }

    @Test func comparisonsReadAndSetRanges() {
        let hundred = ConditionRow(test: .velocity(low: 100 << 9, high: (100 << 9) | 511), negate: false)
        #expect(Comparison.of(hundred) == .equal)
        #expect(Comparison.of(Comparison.outside.applied(to: hundred)) == .notEqual, "still one step")
        let wide = ConditionRow(test: .velocity(low: 50 << 9, high: 0xFFFF), negate: false)
        #expect(Comparison.of(wide) == .inside)
        #expect(Comparison.equal.applied(to: wide).test == .velocity(low: 50 << 9, high: (50 << 9) | 511))
        #expect(Comparison.outside.applied(to: wide).negate)
        #expect(Comparison.choices(for: .timeRange(start: 0, end: 1)) == [.inside, .outside])
        #expect(Comparison.choices(for: .kinds(kinds: [.note])) == [.equal, .notEqual])
        #expect(Comparison.of(ConditionRow(test: .dest(dest: 5), negate: true)) == .notEqual)
    }

    @Test func kindsRoundTripThroughTheirDefaults() {
        for k in ConditionKind.allCases { #expect(ConditionKind(k.test(range: nil)) == k, "\(k)") }
        for k in ModifyKind.allCases { #expect(ModifyKind(k.row) == k, "\(k)") }
        #expect(ModifyKind.title(.change(target: .duration, op: .add, amount: .constant(value: 1))) == "Change Duration")
        #expect(ModifyKind.title(.transposeDiatonic(steps: 1, root: 0, mask: 1)) == "Transpose")
        #expect(ModifyKind.title(.setDest(dest: nil)) == "Reassign Destination")
        for k in ChangeOpKind.allCases { #expect(ChangeOpKind(k.row(for: .velocity)) == k) }
        for k in AmountKind.allCases { #expect(AmountKind(k.row(seed: 1)) == k) }
        #expect(Scale.of(mask: Scale.dorian.mask) == .dorian && Scale.of(mask: 1) == nil)
    }

    @Test func fieldsAndNumbersAgree() {
        var tests = ConditionKind.allCases.map { $0.test(range: nil) }
        tests.append(.metric(grid: q, offset: 5, tolerance: 7))
        for t in tests {
            #expect(t.fields.count == t.numbers.count, "\(t)")
            #expect(t.with(t.numbers) == t, "\(t)")
        }
        var rows = ModifyKind.allCases.map(\.row)
        for op in ChangeOpKind.allCases {
            for a in AmountKind.allCases {
                rows.append(.change(target: .value, op: op.row(for: .value), amount: a.row(seed: 3)))
            }
        }
        for r in rows {
            #expect(r.fields.count == r.numbers.count, "\(r)")
            #expect(r.with(r.numbers) == r, "\(r)")
        }
        #expect(
            ModifyRow.change(target: .velocity, op: .scale, amount: .constant(value: 100)).fields
                == [Field("amount", .percent)])
        #expect(
            ModifyRow.change(target: .velocity, op: .add, amount: .constant(value: 0)).fields
                == [Field("amount", .velocity(.delta))])
    }

    @Test func fieldTextRoundTrips() {
        func parse(_ s: String, _ k: FieldKind) -> Int64? { FieldText.parse(s, k, meter: meter44) }
        func text(_ n: Int64, _ k: FieldKind) -> String { FieldText.text(n, k, meter: meter44) }
        #expect(parse("100", .velocity(.exact)) == 0xC924 && text(0xC924, .velocity(.exact)) == "100")
        #expect(parse("100", .velocity(.high)) == 51_711)
        #expect(parse("-10", .velocity(.delta)) == -5120 && text(-5120, .velocity(.delta)) == "-10")
        #expect(parse("127", .value(.exact)) == 0xFFFF_FFFF)
        #expect(parse("-1.480", .length) == -(q + q / 2) && text(-(q + q / 2), .length) == "-1.480")
        #expect(parse("2", .position) == 4 * q && text(4 * q, .position) == "2.1.0")
        #expect(parse("80%", .percent) == 80 && text(80, .percent) == "80%")
        #expect(parse("C4", .pitch) == 60 && text(60, .pitch) == "C4")
        #expect(parse("abc", .count) == nil && parse("200", .velocity(.exact)) == nil)
    }

    @Test func noteMapAndGrooveText() {
        let m = NoteMapText.parse("C2>D2, E2>F#2")
        #expect(m == [NoteMapEntry(from: 36, to: 38), NoteMapEntry(from: 40, to: 42)])
        #expect(NoteMapText.text(m ?? []) == "C2>D2, E2>F#2")
        #expect(NoteMapText.parse("C2") == nil)
        #expect(NoteMapText.parse("") == [])
        #expect(OffsetsText.parse("0 120 -60") == [0, 120, -60] && OffsetsText.text([0, 120, -60]) == "0 120 -60")
        #expect(OffsetsText.parse("0 x") == nil)
    }
}
```

Create `app/Tests/VizDAWAppModelTests/ExactModeTests.swift`:

```swift
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@Suite struct ExactModeTests {
    @Test func aMoveOpensMoveOnTheDraggedEvents() throws {
        let r = try #require(
            ExactMode.request(for: .event(edit: .move(events: [1, 2], delta: 100, toLane: nil)), lanes: [chords]))
        #expect(r.task == .move && r.config.modify.op == .move(delta: 100) && !r.config.modify.toCopy)
        #expect(r.events == [1, 2] && r.lanes == [chords] && !r.config.select.enabled)
    }

    @Test func copiesAndTransposes() throws {
        let copy = try #require(
            ExactMode.request(for: .event(edit: .duplicate(events: [1], ticks: 50, semitones: 0)), lanes: [chords]))
        #expect(copy.config.modify.op == .move(delta: 50) && copy.config.modify.toCopy)
        let harmony = try #require(
            ExactMode.request(for: .event(edit: .duplicate(events: [1], ticks: 0, semitones: 3)), lanes: [chords]))
        #expect(harmony.task == .transpose && harmony.config.modify.op == .transposeInterval(semitones: 3))
        #expect(harmony.config.modify.toCopy)
        #expect(ExactMode.request(for: .event(edit: .duplicate(events: [1], ticks: 5, semitones: 3)), lanes: []) == nil)
        let up = try #require(
            ExactMode.request(for: .event(edit: .nudge(events: [1], ticks: 0, semitones: -2)), lanes: [chords]))
        #expect(up.task == .transpose && up.config.modify.op == .transposeInterval(semitones: -2))
        let longer = try #require(ExactMode.request(for: .event(edit: .resize(events: [1], delta: 960)), lanes: [chords]))
        #expect(longer.task == .change)
        #expect(longer.config.modify.op == .change(target: .duration, op: .add, amount: .constant(value: 960)))
    }

    @Test func otherDragsApplyAsUsual() {
        let to = LaneRef(seq: 3, lane: 32)
        #expect(ExactMode.request(for: .event(edit: .move(events: [1], delta: 5, toLane: to)), lanes: [chords]) == nil)
        #expect(ExactMode.request(for: .event(edit: .delete(events: [1])), lanes: [chords]) == nil)
        #expect(ExactMode.request(for: .event(edit: .insert(seq: 3, lane: 31, events: [])), lanes: [chords]) == nil)
        #expect(ExactMode.request(for: .lane(edit: .rename(seq: 3, lane: 31, name: "x")), lanes: [chords]) == nil)
    }

    @Test func aRubberBandBecomesASelectRule() {
        let r = ExactMode.selectRequest(
            range: TickRange(start: 0, end: 15_360), pitches: 60...64, lanes: [chords], adding: true)
        #expect(r.task == .selectByRule && r.events == nil && r.range == TickRange(start: 0, end: 15_360))
        #expect(r.config.select.enabled && r.config.select.mode == .add && !r.config.modify.enabled)
        #expect(
            r.config.select.conditions == [
                ConditionRow(test: .kinds(kinds: [.note]), negate: false),
                ConditionRow(test: .timeRange(start: 0, end: 15_360), negate: false),
                ConditionRow(test: .pitch(low: 60, high: 64), negate: false),
            ])
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:" | head -5
```

Expected: `cannot find 'SelectModifyConfig' in scope`.

- [ ] **Step 3: Write `PanelFields.swift`**

```bash
cat > app/Sources/VizDAWAppModel/PanelFields.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// Which end of a 7-bit step a typed value stands for.
public enum Edge: Equatable, Sendable {
    /// A range's low end: the step's first 16- or 32-bit value.
    case low
    /// A range's high end: the step's last value.
    case high
    /// A signed change: steps of 512 (or 2²⁵).
    case delta
    /// One value: the MIDI 2.0 upscale.
    case exact
}

/// How a panel field shows and reads its number.
public enum FieldKind: Equatable, Sendable {
    case count
    case percent
    case pitch
    /// A 16-bit velocity as 0–127.
    case velocity(Edge)
    /// A 32-bit value as 0–127.
    case value(Edge)
    /// Ticks as signed beats.units of a quarter note.
    case length
    /// A tick as bar.beat.unit.
    case position
}

/// One numeric field of a condition or modification.
public struct Field: Equatable, Sendable {
    public let label: String
    public let kind: FieldKind

    public init(_ label: String, _ kind: FieldKind) {
        self.label = label
        self.kind = kind
    }
}

public enum FieldText {
    public static func text(_ n: Int64, _ k: FieldKind, meter: [MeterEntryRow]) -> String {
        switch k {
        case .count: return "\(n)"
        case .percent: return "\(n)%"
        case .pitch: return PitchName.name(UInt8(min(max(n, 0), 127)))
        case .velocity(let e): return e == .delta ? "\(n / 512)" : "\(n >> 9)"
        case .value(let e): return e == .delta ? "\(n / 33_554_432)" : "\(n >> 25)"
        case .length:
            let m = n == .min ? Int64.max : abs(n)
            return (n < 0 ? "-" : "") + DurationText.text(m, beat: MeterMath.ppq)
        case .position: return Bbu.of(n, meter: meter).text
        }
    }

    public static func parse(_ s: String, _ k: FieldKind, meter: [MeterEntryRow]) -> Int64? {
        let t = s.trimmingCharacters(in: .whitespaces)
        switch k {
        case .count:
            guard let v = Int64(t), abs(v) <= 1_000_000 else { return nil }
            return v
        case .percent:
            let p = t.replacingOccurrences(of: "%", with: "").trimmingCharacters(in: .whitespaces)
            guard let v = Int64(p), (0...10_000).contains(v) else { return nil }
            return v
        case .pitch:
            return PitchName.parse(t).map(Int64.init)
        case .velocity(let e):
            guard let v = Int64(t) else { return nil }
            switch e {
            case .delta: return (-127...127).contains(v) ? v * 512 : nil
            case .low: return (0...127).contains(v) ? v << 9 : nil
            case .high: return (0...127).contains(v) ? (v << 9) | 0x1FF : nil
            case .exact: return (0...127).contains(v) ? Int64(Velocity7.up(Int(v))) : nil
            }
        case .value(let e):
            guard let v = Int64(t) else { return nil }
            switch e {
            case .delta: return (-127...127).contains(v) ? v << 25 : nil
            case .low: return (0...127).contains(v) ? v << 25 : nil
            case .high: return (0...127).contains(v) ? (v << 25) | 0x1FF_FFFF : nil
            case .exact: return (0...127).contains(v) ? Int64(Value7.up(Int(v))) : nil
            }
        case .length:
            let negative = t.hasPrefix("-")
            guard let d = DurationText.parse(negative ? String(t.dropFirst()) : t, current: 0, beat: MeterMath.ppq)
            else { return nil }
            return negative ? -d : d
        case .position:
            return PositionInput.parse(t, current: Bbu(bar: 1, beat: 1, unit: 0)).map { $0.tick(meter: meter) }
        }
    }
}

extension ConditionTest {
    public var fields: [Field] {
        switch self {
        case .timeRange: [Field("from", .position), Field("to", .position)]
        case .pitch, .bracketedBy: [Field("from", .pitch), Field("to", .pitch)]
        case .velocity, .releaseVelocity: [Field("from", .velocity(.low)), Field("to", .velocity(.high))]
        case .duration: [Field("from", .length), Field("to", .length)]
        case .value: [Field("from", .value(.low)), Field("to", .value(.high))]
        case .controller: [Field("from", .count), Field("to", .count)]
        case .metric: [Field("grid", .length), Field("offset", .length), Field("within", .length)]
        case .everyNth: [Field("every", .count), Field("starting at", .count)]
        case .kinds, .dest: []
        }
    }

    public var numbers: [Int64] {
        switch self {
        case let .timeRange(s, e): [s, e]
        case let .pitch(l, h), let .bracketedBy(l, h): [Int64(l), Int64(h)]
        case let .velocity(l, h), let .releaseVelocity(l, h): [Int64(l), Int64(h)]
        case let .duration(a, b): [a, b]
        case let .value(l, h): [Int64(l), Int64(h)]
        case let .controller(l, h): [Int64(l), Int64(h)]
        case let .metric(g, o, t): [g, o, t]
        case let .everyNth(n, p): [Int64(n), Int64(p)]
        case .kinds, .dest: []
        }
    }

    /// The test with its numbers replaced (clamped to their types).
    public func with(_ n: [Int64]) -> ConditionTest {
        let old = numbers
        func at(_ i: Int) -> Int64 { n.indices.contains(i) ? n[i] : (old.indices.contains(i) ? old[i] : 0) }
        func pitch(_ v: Int64) -> UInt8 { UInt8(min(max(v, 0), 127)) }
        switch self {
        case .timeRange: return .timeRange(start: at(0), end: at(1))
        case .pitch: return .pitch(low: pitch(at(0)), high: pitch(at(1)))
        case .bracketedBy: return .bracketedBy(low: pitch(at(0)), high: pitch(at(1)))
        case .velocity: return .velocity(low: UInt16(clamping: at(0)), high: UInt16(clamping: at(1)))
        case .releaseVelocity: return .releaseVelocity(low: UInt16(clamping: at(0)), high: UInt16(clamping: at(1)))
        case .duration: return .duration(min: at(0), max: at(1))
        case .value: return .value(low: UInt32(clamping: at(0)), high: UInt32(clamping: at(1)))
        case .controller: return .controller(low: pitch(at(0)), high: pitch(at(1)))
        case .metric: return .metric(grid: max(at(0), 1), offset: at(1), tolerance: max(at(2), 0))
        case .everyNth: return .everyNth(n: UInt32(clamping: max(at(0), 1)), phase: UInt32(clamping: at(1)))
        case .kinds, .dest: return self
        }
    }
}

extension ModifyRow {
    static func unit(_ t: ValueTargetRow, _ e: Edge) -> FieldKind {
        switch t {
        case .velocity, .releaseVelocity: .velocity(e)
        case .duration: .length
        case .value: .value(e)
        }
    }

    static func changeFields(_ t: ValueTargetRow, _ op: ChangeOpRow, _ a: AmountRow) -> [Field] {
        switch op {
        case .limit: return [Field("lowest", unit(t, .low)), Field("highest", unit(t, .high))]
        case .compress: return [Field("lowest", unit(t, .exact)), Field("highest", unit(t, .exact))]
        case .legato: return []
        case .set, .scale, .add:
            let k: FieldKind = op == .scale ? .percent : unit(t, op == .add ? .delta : .exact)
            switch a {
            case .constant: return [Field("amount", k)]
            case .line: return [Field("from", k), Field("to", k)]
            case .oscillator: return [Field("low", k), Field("high", k), Field("period", .length), Field("phase", .length)]
            case .random: return [Field("low", k), Field("high", k)]
            }
        }
    }

    public var fields: [Field] {
        switch self {
        case .transposeInterval: [Field("semitones", .count)]
        case .transposeDiatonic: [Field("steps", .count)]
        case .transposeInvert: [Field("around", .pitch)]
        case .quantize: [Field("grid", .length), Field("strength", .percent)]
        case let .change(t, op, a): Self.changeFields(t, op, a)
        case .move: [Field("by", .length)]
        case .scaleTime: [Field("times", .count), Field("divided by", .count), Field("around", .position)]
        case .trimStart: [Field("by", .length)]
        case .substitute(.controller): [Field("controller", .count)]
        case .substitute(.pitch): [Field("pitch", .pitch)]
        case .thin, .thicken: [Field("spacing", .length)]
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: []
        }
    }

    public var numbers: [Int64] {
        switch self {
        case .transposeInterval(let s): return [Int64(s)]
        case .transposeDiatonic(let s, _, _): return [Int64(s)]
        case .transposeInvert(let a): return [Int64(a)]
        case let .quantize(g, s, _): return [g, Int64(s)]
        case let .change(_, op, amount):
            switch op {
            case let .limit(a, b), let .compress(a, b): return [a, b]
            case .legato: return []
            case .set, .scale, .add:
                switch amount {
                case .constant(let v): return [v]
                case let .line(f, t, _): return [f, t]
                case let .oscillator(_, l, h, p, ph): return [l, h, p, ph]
                case let .random(l, h, _): return [l, h]
                }
            }
        case .move(let d), .trimStart(let d): return [d]
        case let .scaleTime(n, d, a): return [Int64(n), Int64(d), a]
        case .substitute(.controller(let i)): return [Int64(i)]
        case .substitute(.pitch(let p)): return [Int64(p)]
        case .thin(let s), .thicken(let s): return [s]
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: return []
        }
    }

    /// The modification with its numbers replaced (clamped to their types).
    public func with(_ n: [Int64]) -> ModifyRow {
        let old = numbers
        func at(_ i: Int) -> Int64 { n.indices.contains(i) ? n[i] : (old.indices.contains(i) ? old[i] : 0) }
        func pitch(_ v: Int64) -> UInt8 { UInt8(min(max(v, 0), 127)) }
        switch self {
        case .transposeInterval:
            return .transposeInterval(semitones: Int8(clamping: at(0)))
        case let .transposeDiatonic(_, r, m):
            return .transposeDiatonic(steps: Int8(clamping: at(0)), root: r, mask: m)
        case .transposeInvert:
            return .transposeInvert(axis: pitch(at(0)))
        case let .quantize(_, _, o):
            return .quantize(grid: max(at(0), 1), strength: UInt8(min(max(at(1), 0), 100)), offsets: o)
        case let .change(t, op, amount):
            switch op {
            case .limit: return .change(target: t, op: .limit(min: at(0), max: at(1)), amount: amount)
            case .compress: return .change(target: t, op: .compress(low: at(0), high: at(1)), amount: amount)
            case .legato: return self
            case .set, .scale, .add:
                let a: AmountRow
                switch amount {
                case .constant: a = .constant(value: at(0))
                case let .line(_, _, c): a = .line(from: at(0), to: at(1), curve: c)
                case let .oscillator(w, _, _, _, _):
                    a = .oscillator(wave: w, low: at(0), high: at(1), period: max(at(2), 1), phase: at(3))
                case let .random(_, _, s): a = .random(low: at(0), high: at(1), seed: s)
                }
                return .change(target: t, op: op, amount: a)
            }
        case .move: return .move(delta: at(0))
        case .trimStart: return .trimStart(delta: at(0))
        case .scaleTime:
            return .scaleTime(
                num: UInt32(clamping: max(at(0), 1)), den: UInt32(clamping: max(at(1), 1)), anchor: at(2))
        case .substitute(.controller): return .substitute(to: .controller(index: pitch(at(0))))
        case .substitute(.pitch): return .substitute(to: .pitch(pitch: pitch(at(0))))
        case .thin: return .thin(spacing: max(at(0), 1))
        case .thicken: return .thicken(spacing: max(at(0), 1))
        case .transposeToScale, .noteMap, .setDest, .substitute, .delete: return self
        }
    }
}

/// A note map as "C2>D2, E2>F#2".
public enum NoteMapText {
    public static func text(_ m: [NoteMapEntry]) -> String {
        m.map { "\(PitchName.name($0.from))>\(PitchName.name($0.to))" }.joined(separator: ", ")
    }

    public static func parse(_ s: String) -> [NoteMapEntry]? {
        var out: [NoteMapEntry] = []
        for item in s.split(separator: ",") {
            let p = item.split(separator: ">")
            guard p.count == 2, let a = PitchName.parse(String(p[0])), let b = PitchName.parse(String(p[1])) else {
                return nil
            }
            out.append(NoteMapEntry(from: a, to: b))
        }
        return out
    }
}

/// Groove offsets in ticks as "0 120 -60".
public enum OffsetsText {
    public static func text(_ o: [Int64]) -> String { o.map(String.init).joined(separator: " ") }

    public static func parse(_ s: String) -> [Int64]? {
        var out: [Int64] = []
        for w in s.split(separator: " ") {
            guard let v = Int64(w), abs(v) <= 1_000_000 else { return nil }
            out.append(v)
        }
        return out
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Write `SelectModify.swift` and `ExactMode.swift`**

```bash
cat > app/Sources/VizDAWAppModel/SelectModify.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// What a Modify menu command opens the panel for (parent spec §9).
public enum PanelTask: String, CaseIterable, Sendable {
    case selectByRule, transpose, quantize, change, move

    public var title: String {
        switch self {
        case .selectByRule: "Select by Rule"
        case .transpose: "Transpose"
        case .quantize: "Quantize"
        case .change: "Change Values"
        case .move: "Move"
        }
    }
}

/// The select half (Plan 4c Ruling 7).
public struct SelectHalf: Equatable, Sendable {
    public var enabled: Bool
    public var matchAll: Bool
    public var mode: SelectModeRow
    public var conditions: [ConditionRow]

    public init(enabled: Bool = false, matchAll: Bool = true, mode: SelectModeRow = .select, conditions: [ConditionRow] = []) {
        self.enabled = enabled
        self.matchAll = matchAll
        self.mode = mode
        self.conditions = conditions
    }
}

/// The modify half.
public struct ModifyHalf: Equatable, Sendable {
    public var enabled: Bool
    public var op: ModifyRow
    /// To a copy (a harmonizing copy for transposes).
    public var toCopy: Bool

    public init(enabled: Bool = true, op: ModifyRow = .transposeInterval(semitones: 0), toCopy: Bool = false) {
        self.enabled = enabled
        self.op = op
        self.toCopy = toCopy
    }
}

/// Everything the panel holds; a template stores exactly this.
public struct SelectModifyConfig: Equatable, Sendable {
    public var select = SelectHalf()
    public var modify = ModifyHalf()

    public init() {}

    /// The panel set to `task`; Select by Rule starts with Kind is Note and,
    /// when the selection has a range, a Position condition over it.
    public static func preset(_ task: PanelTask, range: TickRange? = nil) -> SelectModifyConfig {
        var c = SelectModifyConfig()
        switch task {
        case .selectByRule:
            c.select.enabled = true
            c.select.conditions = [ConditionRow(test: .kinds(kinds: [.note]), negate: false)]
            if let r = range {
                c.select.conditions.append(ConditionRow(test: .timeRange(start: r.start, end: r.end), negate: false))
            }
            c.modify.enabled = false
        case .transpose:
            c.modify.op = .transposeInterval(semitones: 0)
        case .quantize:
            c.modify.op = .quantize(grid: MeterMath.ppq / 4, strength: 100, offsets: [])
        case .change:
            c.modify.op = .change(target: .velocity, op: .add, amount: .constant(value: 0))
        case .move:
            c.modify.op = .move(delta: 0)
        }
        return c
    }

    public var rule: RuleRow? {
        select.enabled && !select.conditions.isEmpty
            ? RuleRow(conditions: select.conditions, matchAll: select.matchAll) : nil
    }

    public var modification: ModifyRow? { modify.enabled ? modify.op : nil }

    /// What Apply sends; nil when neither half has anything to do.
    public func command() -> EditCommand? {
        let r = rule
        let m = modification
        guard r != nil || m != nil else { return nil }
        return .selectAndModify(rule: r, mode: select.mode, modify: m, toCopy: m != nil && modify.toCopy)
    }

    public init(template t: ModifyTemplateRow) {
        select = SelectHalf(
            enabled: t.rule != nil, matchAll: t.rule?.matchAll ?? true, mode: t.mode, conditions: t.rule?.conditions ?? [])
        modify = ModifyHalf(enabled: t.modify != nil, op: t.modify ?? .transposeInterval(semitones: 0), toCopy: t.toCopy)
    }

    public func template(named name: String) -> ModifyTemplateRow {
        ModifyTemplateRow(name: name, rule: rule, mode: select.mode, modify: modification, toCopy: modify.toCopy)
    }
}

/// The attributes a condition tests (parent spec §9 select half).
public enum ConditionKind: String, CaseIterable, Sendable {
    case time, kind, pitch, velocity, releaseVelocity, duration, value, controller, destination, metric, everyNth
    case bracketed

    public var title: String {
        switch self {
        case .time: "Position"
        case .kind: "Kind"
        case .pitch: "Pitch"
        case .velocity: "Velocity"
        case .releaseVelocity: "Release Velocity"
        case .duration: "Length"
        case .value: "Value"
        case .controller: "Controller"
        case .destination: "Destination"
        case .metric: "Metric Position"
        case .everyNth: "Every Nth"
        case .bracketed: "Inside a Note"
        }
    }

    public init(_ t: ConditionTest) {
        switch t {
        case .timeRange: self = .time
        case .kinds: self = .kind
        case .pitch: self = .pitch
        case .velocity: self = .velocity
        case .releaseVelocity: self = .releaseVelocity
        case .duration: self = .duration
        case .value: self = .value
        case .controller: self = .controller
        case .dest: self = .destination
        case .metric: self = .metric
        case .everyNth: self = .everyNth
        case .bracketedBy: self = .bracketed
        }
    }

    /// A new condition of this kind; Position covers `range` (else the first
    /// four 4/4 bars).
    public func test(range: TickRange?) -> ConditionTest {
        let q = MeterMath.ppq
        switch self {
        case .time:
            let r = range ?? TickRange(start: 0, end: 16 * q)
            return .timeRange(start: r.start, end: r.end)
        case .kind: return .kinds(kinds: [.note])
        case .pitch: return .pitch(low: 60, high: 72)
        case .velocity: return .velocity(low: 0, high: 0xFFFF)
        case .releaseVelocity: return .releaseVelocity(low: 0, high: 0xFFFF)
        case .duration: return .duration(min: 0, max: q)
        case .value: return .value(low: 0, high: UInt32.max)
        case .controller: return .controller(low: 7, high: 7)
        case .destination: return .dest(dest: nil)
        case .metric: return .metric(grid: q, offset: 0, tolerance: q / 16)
        case .everyNth: return .everyNth(n: 2, phase: 0)
        case .bracketed: return .bracketedBy(low: 0, high: 127)
        }
    }
}

/// How a condition compares (parent spec §9: equal, not equal, inside or
/// outside a range); "not" is the condition's `negate`.
public enum Comparison: String, CaseIterable, Sendable {
    case equal, notEqual, inside, outside

    public var title: String {
        switch self {
        case .equal: "is"
        case .notEqual: "is not"
        case .inside: "inside"
        case .outside: "outside"
        }
    }

    public var negates: Bool { self == .notEqual || self == .outside }

    public static func choices(for t: ConditionTest) -> [Comparison] {
        switch ConditionKind(t) {
        case .time: [.inside, .outside]
        case .pitch, .velocity, .releaseVelocity, .duration, .value, .controller, .bracketed: allCases
        default: [.equal, .notEqual]
        }
    }

    /// A range reads as "is" when both ends show the same text.
    public static func of(_ c: ConditionRow) -> Comparison {
        let choices = choices(for: c.test)
        guard choices.contains(.inside) else { return c.negate ? .notEqual : .equal }
        let f = c.test.fields
        let n = c.test.numbers
        if choices.contains(.equal), f.count == 2, n.count == 2,
            FieldText.text(n[0], f[0].kind, meter: []) == FieldText.text(n[1], f[1].kind, meter: [])
        {
            return c.negate ? .notEqual : .equal
        }
        return c.negate ? .outside : .inside
    }

    /// The condition compared this way; "is" and "is not" on a range make
    /// its high end the same step as its low end.
    public func applied(to c: ConditionRow) -> ConditionRow {
        var out = c
        out.negate = negates
        let f = c.test.fields
        let n = c.test.numbers
        if self == .equal || self == .notEqual, Comparison.choices(for: c.test).contains(.inside), f.count == 2,
            n.count == 2
        {
            let low = FieldText.text(n[0], f[0].kind, meter: [])
            out.test = c.test.with([n[0], FieldText.parse(low, f[1].kind, meter: []) ?? n[0]])
        }
        return out
    }
}

/// The panel's modify choices (parent spec §9 modify half).
public enum ModifyKind: String, CaseIterable, Sendable {
    case transpose, diatonic, toScale, invert, noteMap, quantize, change, move, scaleTime, trimStart, setDest
    case substitute, thin, thicken, delete

    public var title: String {
        switch self {
        case .transpose: "Transpose"
        case .diatonic: "Transpose in Scale"
        case .toScale: "Snap to Scale"
        case .invert: "Invert"
        case .noteMap: "Note Map"
        case .quantize: "Quantize"
        case .change: "Change Values"
        case .move: "Move"
        case .scaleTime: "Scale Time"
        case .trimStart: "Trim Start"
        case .setDest: "Set Destination"
        case .substitute: "Substitute"
        case .thin: "Thin Out"
        case .thicken: "Thicken"
        case .delete: "Delete"
        }
    }

    public init(_ r: ModifyRow) {
        switch r {
        case .transposeInterval: self = .transpose
        case .transposeDiatonic: self = .diatonic
        case .transposeToScale: self = .toScale
        case .transposeInvert: self = .invert
        case .noteMap: self = .noteMap
        case .quantize: self = .quantize
        case .change: self = .change
        case .move: self = .move
        case .scaleTime: self = .scaleTime
        case .trimStart: self = .trimStart
        case .setDest: self = .setDest
        case .substitute: self = .substitute
        case .thin: self = .thin
        case .thicken: self = .thicken
        case .delete: self = .delete
        }
    }

    /// A new modification of this kind.
    public var row: ModifyRow {
        let q = MeterMath.ppq
        switch self {
        case .transpose: return .transposeInterval(semitones: 0)
        case .diatonic: return .transposeDiatonic(steps: 1, root: 0, mask: Scale.major.mask)
        case .toScale: return .transposeToScale(root: 0, mask: Scale.major.mask)
        case .invert: return .transposeInvert(axis: 60)
        case .noteMap: return .noteMap(map: [])
        case .quantize: return .quantize(grid: q / 4, strength: 100, offsets: [])
        case .change: return .change(target: .velocity, op: .add, amount: .constant(value: 0))
        case .move: return .move(delta: 0)
        case .scaleTime: return .scaleTime(num: 1, den: 1, anchor: 0)
        case .trimStart: return .trimStart(delta: 0)
        case .setDest: return .setDest(dest: nil)
        case .substitute: return .substitute(to: .controller(index: 1))
        case .thin: return .thin(spacing: q / 16)
        case .thicken: return .thicken(spacing: q / 16)
        case .delete: return .delete
        }
    }

    /// The step's name as the undo menu shows it ("Modify Again: …").
    public static func title(_ r: ModifyRow) -> String {
        switch r {
        case .transposeInterval, .transposeDiatonic, .transposeToScale, .transposeInvert, .noteMap: "Transpose"
        case .quantize: "Quantize"
        case .change(let t, _, _):
            switch t {
            case .velocity: "Change Velocity"
            case .releaseVelocity: "Change Release Velocity"
            case .duration: "Change Duration"
            case .value: "Change Values"
            }
        case .move: "Move"
        case .scaleTime: "Scale Time"
        case .trimStart: "Trim Start"
        case .setDest: "Reassign Destination"
        case .substitute: "Substitute"
        case .thin: "Thin Out"
        case .thicken: "Thicken"
        case .delete: "Delete Events"
        }
    }
}

/// The change operations, for a picker.
public enum ChangeOpKind: String, CaseIterable, Sendable {
    case set, scale, add, limit, compress, legato

    public var title: String {
        switch self {
        case .set: "Set to"
        case .scale: "Scale by"
        case .add: "Add"
        case .limit: "Limit to"
        case .compress: "Compress to"
        case .legato: "Legato"
        }
    }

    public init(_ op: ChangeOpRow) {
        switch op {
        case .set: self = .set
        case .scale: self = .scale
        case .add: self = .add
        case .limit: self = .limit
        case .compress: self = .compress
        case .legato: self = .legato
        }
    }

    /// The operation with the target's full range for limits.
    public func row(for t: ValueTargetRow) -> ChangeOpRow {
        let top: Int64 =
            switch t {
            case .velocity, .releaseVelocity: 0xFFFF
            case .duration: 4 * MeterMath.ppq
            case .value: Int64(UInt32.max)
            }
        switch self {
        case .set: return .set
        case .scale: return .scale
        case .add: return .add
        case .limit: return .limit(min: 0, max: top)
        case .compress: return .compress(low: 0, high: top)
        case .legato: return .legato
        }
    }
}

/// The amount shapes, for a picker (Plan 4c Ruling 3).
public enum AmountKind: String, CaseIterable, Sendable {
    case constant, line, oscillator, random

    public var title: String {
        switch self {
        case .constant: "Constant"
        case .line: "Along a Line"
        case .oscillator: "Oscillating"
        case .random: "Random"
        }
    }

    public init(_ a: AmountRow) {
        switch a {
        case .constant: self = .constant
        case .line: self = .line
        case .oscillator: self = .oscillator
        case .random: self = .random
        }
    }

    public func row(seed: UInt64) -> AmountRow {
        switch self {
        case .constant: .constant(value: 0)
        case .line: .line(from: 0, to: 0, curve: .straight)
        case .oscillator: .oscillator(wave: .sine, low: 0, high: 0, period: MeterMath.ppq, phase: 0)
        case .random: .random(low: 0, high: 0, seed: seed)
        }
    }
}

extension Scale {
    /// The named scale with this mask, if any.
    public static func of(mask: UInt16) -> Scale? { allCases.first { $0.mask == mask } }
}
VIZDAW_EOF
cat > app/Sources/VizDAWAppModel/ExactMode.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// The panel opened by exact mode or a menu command, with what it acts on.
public struct ModifyRequest: Equatable, Sendable {
    public var task: PanelTask
    public var config: SelectModifyConfig
    /// The dragged events (nil: keep the selection's events).
    public var events: [UInt64]?
    public var lanes: [LaneKey]
    public var range: TickRange?

    public init(task: PanelTask, config: SelectModifyConfig, events: [UInt64]?, lanes: [LaneKey], range: TickRange?) {
        self.task = task
        self.config = config
        self.events = events
        self.lanes = lanes
        self.range = range
    }
}

/// Exact mode (parent spec §9, Plan 4c Ruling 23): a drag's final command
/// as the panel pre-filled with its amounts.
public enum ExactMode {
    /// The panel for a drag that ended with `cmd`; nil when that drag
    /// applies as usual.
    public static func request(for cmd: EditCommand, lanes: Set<LaneKey>) -> ModifyRequest? {
        guard case .event(let e) = cmd else { return nil }
        let sorted = lanes.sorted { ($0.seq, $0.lane) < ($1.seq, $1.lane) }
        func make(_ task: PanelTask, _ events: [UInt64], _ op: ModifyRow, copy: Bool) -> ModifyRequest {
            var c = SelectModifyConfig.preset(task)
            c.modify.op = op
            c.modify.toCopy = copy
            return ModifyRequest(task: task, config: c, events: events, lanes: sorted, range: nil)
        }
        switch e {
        case .move(let events, let delta, nil):
            return make(.move, events, .move(delta: delta), copy: false)
        case let .duplicate(events, ticks, semitones):
            if semitones == 0 { return make(.move, events, .move(delta: ticks), copy: true) }
            if ticks == 0 { return make(.transpose, events, .transposeInterval(semitones: semitones), copy: true) }
            return nil
        case let .nudge(events, ticks, semitones):
            if ticks == 0 { return make(.transpose, events, .transposeInterval(semitones: semitones), copy: false) }
            if semitones == 0 { return make(.move, events, .move(delta: ticks), copy: false) }
            return nil
        case let .resize(events, delta):
            return make(
                .change, events, .change(target: .duration, op: .add, amount: .constant(value: delta)), copy: false)
        default:
            return nil
        }
    }

    /// A rubber band as Select by Rule: notes starting in its range and
    /// pitches, in the shown lanes.
    public static func selectRequest(
        range: TickRange, pitches: ClosedRange<UInt8>, lanes: [LaneKey], adding: Bool
    ) -> ModifyRequest {
        var c = SelectModifyConfig.preset(.selectByRule)
        c.select.mode = adding ? .add : .select
        c.select.conditions = [
            ConditionRow(test: .kinds(kinds: [.note]), negate: false),
            ConditionRow(test: .timeRange(start: range.start, end: range.end), negate: false),
            ConditionRow(test: .pitch(low: pitches.lowerBound, high: pitches.upperBound), negate: false),
        ]
        return ModifyRequest(task: .selectByRule, config: c, events: nil, lanes: lanes, range: range)
    }
}
VIZDAW_EOF
```

- [ ] **Step 5: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | head; scripts/swift.sh test 2>&1 | tail -3
```

Expected: 396 tests pass (384 + 8 + 4).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/SelectModify.swift app/Sources/VizDAWAppModel/PanelFields.swift app/Sources/VizDAWAppModel/ExactMode.swift app/Tests/VizDAWAppModelTests/SelectModifyTests.swift app/Tests/VizDAWAppModelTests/ExactModeTests.swift
git commit -m "feat(app): select-and-modify configuration, panel fields and exact-mode mapping

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 12: `SelectModifyModel` — open, apply, templates, Modify Again

**Files:**
- Create: `app/Sources/VizDAWAppModel/SelectModifyModel.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `app/Tests/VizDAWAppModelTests/SelectModifyModelTests.swift` (new)

**Interfaces:**
- Consumes: Task 11 (`SelectModifyConfig`, `PanelTask`, `ConditionKind`, `Comparison`, `ModifyKind`, `ChangeOpKind`, `AmountKind`, `FieldText`, `NoteMapText`, `OffsetsText`, `ModifyRequest`, `Scale.of`), Task 6 (`EditorModel.{panelKey, lastModify, modifyTemplates}`), `EditorModel.{edit, selection, info, status}`.
- Produces: `final class SelectModifyModel` (`@Observable @MainActor`): `document`, `config`, `task`, `isOpen`, `isKey` (mirrors `EditorModel.panelKey`), `templateName`, `filter: ViewFilterRow?`, `drafts`, `invalid: Set<String>`, `onPresent: (() -> Void)?`, `nextSeed: () -> UInt64`, `meter`, `templates`, `destinations`; `open(_ task: PanelTask, filter: ViewFilterRow? = nil)`, `open(_ request: ModifyRequest)`, `close()`, `apply() -> Bool`, `saveTemplate() -> Bool`, `deleteTemplate(_:)`, `load(_:)`, `run(_:) -> Bool`, `modifyAgain() -> Bool`; editing: `addCondition(_:)`, `removeCondition(at:)`, `setComparison(_:at:)`, `setKinds(_:at:)`, `setConditionDest(_:at:)`, `conditionText(_:_:)`, `setConditionText(_:_:_:)`, `setModifyKind(_:)`, `setTarget(_:)`, `setOp(_:)`, `setAmount(_:)`, `setCurve(_:)`, `setWave(_:)`, `setScale(root:scale:)`, `setModifyDest(_:)`, `setSubstitute(_:)`, `modifyText(_:)`, `setModifyText(_:_:)`, `noteMapText`, `setNoteMapText(_:)`, `offsetsText`, `setOffsetsText(_:)`; static messages `nothingMessage`, `noLanesMessage`, `invalidMessage`, `noNameMessage`. `DocumentModel.selectModify: SelectModifyModel`. `FakeSession.editFilters: [ViewFilterRow?]`.

Rulings 3, 7, 9, 22. Field text is kept as typed (`drafts`, keyed "c<i>.<j>", "m<j>", "map", "offsets") so a half-typed value is not overwritten; a draft that does not parse is listed in `invalid` (drawn red) and Apply refuses until it is fixed. Replacing the configuration (open, load, another kind) clears the drafts.

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`, Find:

```swift
    func edit(cmd: EditCommand, filter: ViewFilterRow?) throws -> DocSnapshot {
        if let e = editError { throw e }
        edits.append(cmd)
```

Replace:

```swift
    var editFilters: [ViewFilterRow?] = []
    func edit(cmd: EditCommand, filter: ViewFilterRow?) throws -> DocSnapshot {
        if let e = editError { throw e }
        edits.append(cmd)
        editFilters.append(filter)
```

Create `app/Tests/VizDAWAppModelTests/SelectModifyModelTests.swift`:

```swift
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
private func panel() throws -> (FakeSession, DocumentModel, SelectModifyModel) {
    let f = rollFake()
    let m = try DocumentModel(session: f)
    return (f, m, m.selectModify)
}

@MainActor
private func selectChords(_ m: DocumentModel, range: TickRange? = nil) {
    m.editor.setSelection(SelectionRow(timeRange: range, lanes: [LaneRef(seq: 3, lane: 31)], events: [1]))
}

@MainActor
@Suite struct SelectModifyModelTests {
    @Test func openingATaskPresetsAndPresentsThePanel() throws {
        let (_, m, s) = try panel()
        var presented = 0
        s.onPresent = { presented += 1 }
        s.open(.quantize)
        #expect(s.isOpen && s.task == .quantize && presented == 1)
        #expect(s.config == SelectModifyConfig.preset(.quantize))
        selectChords(m, range: TickRange(start: 0, end: 61_440))
        s.open(.selectByRule)
        #expect(s.config.select.conditions.last?.test == .timeRange(start: 0, end: 61_440))
        s.close()
        #expect(!s.isOpen && !s.isKey)
    }

    @Test func applySendsOneCommandWithTheOpeningListsFilter() throws {
        let (f, m, s) = try panel()
        selectChords(m)
        let hidden = ViewFilterRow(hiddenKinds: [.cc], hiddenLanes: [])
        s.open(.transpose, filter: hidden)
        s.setModifyText("7", 0)
        #expect(s.apply())
        #expect(f.edits.last == .selectAndModify(rule: nil, mode: .select, modify: .transposeInterval(semitones: 7), toCopy: false))
        #expect(f.editFilters.last == hidden)
    }

    @Test func applyWithoutLanesSaysWhatToSelect() throws {
        let (f, m, s) = try panel()
        s.open(.selectByRule)
        #expect(!s.apply())
        #expect(m.editor.status == SelectModifyModel.noLanesMessage && f.edits.isEmpty)
        selectChords(m)
        #expect(s.apply())
        #expect(f.edits.count == 1)
    }

    @Test func applyWithNothingTurnedOnSaysSo() throws {
        let (f, m, s) = try panel()
        s.open(.change)
        s.config.modify.enabled = false
        #expect(!s.apply())
        #expect(m.editor.status == SelectModifyModel.nothingMessage && f.edits.isEmpty)
    }

    @Test func typedFieldsKeepDraftsAndRefuseBadText() throws {
        let (f, m, s) = try panel()
        selectChords(m)
        s.open(.change)
        s.setModifyText("abc", 0)
        #expect(s.modifyText(0) == "abc" && s.invalid == ["m0"])
        #expect(s.config.modify.op == .change(target: .velocity, op: .add, amount: .constant(value: 0)))
        #expect(!s.apply())
        #expect(m.editor.status == SelectModifyModel.invalidMessage && f.edits.isEmpty)
        s.setModifyText("10", 0)
        #expect(s.invalid.isEmpty)
        #expect(s.config.modify.op == .change(target: .velocity, op: .add, amount: .constant(value: 5120)))
        s.setModifyKind(.move)
        #expect(s.drafts.isEmpty && s.modifyText(0) == "0.0")
    }

    @Test func conditionsAndChangeOperationsEdit() throws {
        let (_, _, s) = try panel()
        s.open(.change)
        s.addCondition(.velocity)
        #expect(s.config.select.enabled && s.config.select.conditions.count == 1)
        s.setConditionText("100", 0, 0)
        s.setComparison(.equal, at: 0)
        #expect(s.config.select.conditions[0].test == .velocity(low: 100 << 9, high: (100 << 9) | 511))
        #expect(s.conditionText(0, 1) == "100")
        s.setTarget(.duration)
        s.setOp(.scale)
        #expect(s.config.modify.op == .change(target: .duration, op: .scale, amount: .constant(value: 100)))
        s.setAmount(.line)
        s.setCurve(.sCurve)
        #expect(s.config.modify.op == .change(target: .duration, op: .scale, amount: .line(from: 0, to: 0, curve: .sCurve)))
        s.setModifyKind(.diatonic)
        s.setScale(root: 2, scale: .dorian)
        #expect(s.config.modify.op == .transposeDiatonic(steps: 1, root: 2, mask: Scale.dorian.mask))
        s.setModifyKind(.noteMap)
        s.setNoteMapText("C2>D2")
        #expect(s.config.modify.op == .noteMap(map: [NoteMapEntry(from: 36, to: 38)]))
        s.removeCondition(at: 0)
        #expect(s.config.select.conditions.isEmpty)
    }

    @Test func templatesSaveLoadRunAndDelete() throws {
        let (f, m, s) = try panel()
        selectChords(m)
        s.open(.transpose)
        s.setModifyText("12", 0)
        s.templateName = "  "
        #expect(!s.saveTemplate() && m.editor.status == SelectModifyModel.noNameMessage)
        s.templateName = "Up"
        #expect(s.saveTemplate())
        let t = ModifyTemplateRow(name: "Up", rule: nil, mode: .select, modify: .transposeInterval(semitones: 12), toCopy: false)
        #expect(f.edits.last == .settings(edit: .saveTemplate(template: t)))
        s.open(.move)
        #expect(s.run(t))
        #expect(f.edits.last == .selectAndModify(rule: nil, mode: .select, modify: .transposeInterval(semitones: 12), toCopy: false))
        s.deleteTemplate("Up")
        #expect(f.edits.last == .settings(edit: .deleteTemplate(name: "Up")))
        s.load(t)
        #expect(s.config == SelectModifyConfig(template: t) && s.templateName == "Up")
    }

    @Test func modifyAgainSendsModifyAgain() throws {
        let (f, _, s) = try panel()
        #expect(s.modifyAgain())
        #expect(f.edits.last == .modifyAgain)
    }

    @Test func randomAmountsGetANewSeedAtEachApply() throws {
        let (f, m, s) = try panel()
        selectChords(m)
        var seeds: [UInt64] = [11, 22]
        s.nextSeed = { seeds.removeFirst() }
        s.open(.change)
        s.config.modify.op = .change(target: .velocity, op: .set, amount: .random(low: 0, high: 10, seed: 0))
        s.apply()
        s.apply()
        let used = f.edits.compactMap { e -> UInt64? in
            guard case .selectAndModify(_, _, .change(_, _, .random(_, _, let seed))?, _) = e else { return nil }
            return seed
        }
        #expect(used == [11, 22])
    }

    @Test func aKeyPanelTurnsTheTransportKeysOff() throws {
        let (_, m, s) = try panel()
        s.isKey = true
        #expect(!m.transportKeysEnabled && m.editor.panelKey)
        s.isKey = false
        #expect(m.transportKeysEnabled)
    }
}
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:" | head -5
```

Expected: `value of type 'DocumentModel' has no member 'selectModify'`.

- [ ] **Step 3: Implement**

```bash
cat > app/Sources/VizDAWAppModel/SelectModifyModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// The select-and-modify panel of one document (parent spec §9): what it is
/// set to, the text typed into it, and what Apply, templates and Modify
/// Again send. The panel window itself is the app's (Plan 4c Ruling 22).
@Observable
@MainActor
public final class SelectModifyModel {
    public weak var document: DocumentModel?
    public var config = SelectModifyConfig.preset(.change)
    public private(set) var task: PanelTask = .change
    public private(set) var isOpen = false
    public var templateName = ""
    /// The opening list's hidden kinds (spec §3.3), else nil.
    public var filter: ViewFilterRow?
    /// Field text as typed, by key ("c<i>.<j>", "m<j>", "map", "offsets").
    public private(set) var drafts: [String: String] = [:]
    /// Drafts that do not parse (drawn red; Apply waits for them).
    public private(set) var invalid: Set<String> = []
    /// Shows the panel window (set by the app).
    @ObservationIgnored public var onPresent: (() -> Void)?
    /// A new random seed per Apply (Plan 4c Ruling 3; tests replace it).
    @ObservationIgnored public var nextSeed: () -> UInt64 = { UInt64.random(in: 0...UInt64.max) }
    @ObservationIgnored private var destCache: (version: UInt64, rows: [DestRow])?

    public static let nothingMessage = "Turn on Select or Modify first."
    public static let noLanesMessage = "Select lanes to search first: drag in a ruler or select a block."
    public static let invalidMessage = "Fix the fields marked in red first."
    public static let noNameMessage = "Name the template first."

    public init() {}

    var editor: EditorModel? { document?.editor }

    /// The panel window is key: the transport keys are off.
    public var isKey: Bool {
        get { editor?.panelKey ?? false }
        set { editor?.panelKey = newValue }
    }

    /// The meter positions are typed in: the shown sequence's.
    public var meter: [MeterEntryRow] {
        guard let d = document, let seq = d.selectedSeq else { return [] }
        return d.editor.info(seq)?.meter ?? []
    }

    public var templates: [ModifyTemplateRow] { editor?.modifyTemplates() ?? [] }

    public var destinations: [DestRow] {
        guard let editor else { return [] }
        if let c = destCache, c.version == editor.snapshotVersion { return c.rows }
        let rows = (try? editor.session.destinations()) ?? destCache?.rows ?? []
        destCache = (editor.snapshotVersion, rows)
        return rows
    }

    // MARK: Opening

    /// A Modify menu command: the panel set to `task`.
    public func open(_ task: PanelTask, filter: ViewFilterRow? = nil) {
        self.task = task
        config = .preset(task, range: editor?.selection.timeRange)
        self.filter = filter
        present()
    }

    /// Exact mode: the panel pre-filled with a drag's amounts.
    public func open(_ request: ModifyRequest) {
        task = request.task
        config = request.config
        filter = nil
        present()
    }

    public func close() {
        isOpen = false
        isKey = false
    }

    private func present() {
        drafts = [:]
        invalid = []
        isOpen = true
        onPresent?()
    }

    // MARK: Running

    /// Runs the configuration on the selection: one undo step when it
    /// modifies, a selection change otherwise.
    @discardableResult
    public func apply() -> Bool {
        guard let editor else { return false }
        guard invalid.isEmpty else {
            editor.status = Self.invalidMessage
            return false
        }
        guard let cmd = config.command() else {
            editor.status = Self.nothingMessage
            return false
        }
        if config.rule != nil, config.select.mode != .refine, editor.selection.lanes.isEmpty {
            editor.status = Self.noLanesMessage
            return false
        }
        return editor.edit(reseeded(cmd), filter: filter)
    }

    func reseeded(_ cmd: EditCommand) -> EditCommand {
        guard case let .selectAndModify(rule, mode, .change(t, op, .random(lo, hi, _))?, copy) = cmd else { return cmd }
        return .selectAndModify(
            rule: rule, mode: mode, modify: .change(target: t, op: op, amount: .random(low: lo, high: hi, seed: nextSeed())),
            toCopy: copy)
    }

    @discardableResult
    public func saveTemplate() -> Bool {
        guard let editor else { return false }
        let name = templateName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else {
            editor.status = Self.noNameMessage
            return false
        }
        return editor.edit(.settings(edit: .saveTemplate(template: config.template(named: name))))
    }

    public func deleteTemplate(_ name: String) {
        editor?.edit(.settings(edit: .deleteTemplate(name: name)))
    }

    /// The panel shows template `t`.
    public func load(_ t: ModifyTemplateRow) {
        config = SelectModifyConfig(template: t)
        templateName = t.name
        drafts = [:]
        invalid = []
    }

    /// Modify > Templates: runs `t` at once on the selection.
    @discardableResult
    public func run(_ t: ModifyTemplateRow) -> Bool {
        load(t)
        return apply()
    }

    @discardableResult
    public func modifyAgain() -> Bool { editor?.edit(.modifyAgain) ?? false }

    // MARK: Editing the select half

    public func addCondition(_ k: ConditionKind) {
        config.select.conditions.append(ConditionRow(test: k.test(range: editor?.selection.timeRange), negate: false))
        config.select.enabled = true
    }

    public func removeCondition(at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions.remove(at: i)
        drafts = [:]
        invalid = []
    }

    public func setComparison(_ c: Comparison, at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions[i] = c.applied(to: config.select.conditions[i])
        drafts = drafts.filter { !$0.key.hasPrefix("c\(i).") }
    }

    public func setKinds(_ kinds: [EventKindRow], at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions[i].test = .kinds(kinds: kinds)
    }

    public func setConditionDest(_ dest: UInt64?, at i: Int) {
        guard config.select.conditions.indices.contains(i) else { return }
        config.select.conditions[i].test = .dest(dest: dest)
    }

    public func conditionText(_ i: Int, _ j: Int) -> String {
        if let d = drafts["c\(i).\(j)"] { return d }
        guard config.select.conditions.indices.contains(i) else { return "" }
        let t = config.select.conditions[i].test
        let f = t.fields
        let n = t.numbers
        guard f.indices.contains(j), n.indices.contains(j) else { return "" }
        return FieldText.text(n[j], f[j].kind, meter: meter)
    }

    public func setConditionText(_ s: String, _ i: Int, _ j: Int) {
        let key = "c\(i).\(j)"
        drafts[key] = s
        guard config.select.conditions.indices.contains(i) else { return }
        let t = config.select.conditions[i].test
        let f = t.fields
        guard f.indices.contains(j), let v = FieldText.parse(s, f[j].kind, meter: meter) else {
            invalid.insert(key)
            return
        }
        invalid.remove(key)
        var n = t.numbers
        n[j] = v
        config.select.conditions[i].test = t.with(n)
    }

    // MARK: Editing the modify half

    public func setModifyKind(_ k: ModifyKind) {
        config.modify.op = k.row
        config.modify.enabled = true
        drafts = [:]
        invalid = []
    }

    public func setTarget(_ t: ValueTargetRow) {
        guard case let .change(_, op, a) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: ChangeOpKind(op).row(for: t), amount: a)
        drafts = [:]
    }

    public func setOp(_ k: ChangeOpKind) {
        guard case let .change(t, _, a) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: k.row(for: t), amount: k == .scale ? .constant(value: 100) : a)
        drafts = [:]
    }

    public func setAmount(_ k: AmountKind) {
        guard case let .change(t, op, _) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: op, amount: k.row(seed: nextSeed()))
        drafts = [:]
    }

    public func setCurve(_ c: CurveRow) {
        guard case let .change(t, op, .line(f, to, _)) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: op, amount: .line(from: f, to: to, curve: c))
    }

    public func setWave(_ w: WaveRow) {
        guard case let .change(t, op, .oscillator(_, lo, hi, p, ph)) = config.modify.op else { return }
        config.modify.op = .change(target: t, op: op, amount: .oscillator(wave: w, low: lo, high: hi, period: p, phase: ph))
    }

    public func setScale(root: UInt8, scale: Scale) {
        switch config.modify.op {
        case .transposeDiatonic(let steps, _, _):
            config.modify.op = .transposeDiatonic(steps: steps, root: root, mask: scale.mask)
        case .transposeToScale:
            config.modify.op = .transposeToScale(root: root, mask: scale.mask)
        default:
            break
        }
    }

    public func setModifyDest(_ d: UInt64?) { config.modify.op = .setDest(dest: d) }

    public func setSubstitute(_ s: SubstituteRow) {
        config.modify.op = .substitute(to: s)
        drafts = [:]
    }

    public func modifyText(_ j: Int) -> String {
        if let d = drafts["m\(j)"] { return d }
        let f = config.modify.op.fields
        let n = config.modify.op.numbers
        guard f.indices.contains(j), n.indices.contains(j) else { return "" }
        return FieldText.text(n[j], f[j].kind, meter: meter)
    }

    public func setModifyText(_ s: String, _ j: Int) {
        let key = "m\(j)"
        drafts[key] = s
        let op = config.modify.op
        let f = op.fields
        guard f.indices.contains(j), let v = FieldText.parse(s, f[j].kind, meter: meter) else {
            invalid.insert(key)
            return
        }
        invalid.remove(key)
        var n = op.numbers
        n[j] = v
        config.modify.op = op.with(n)
    }

    public var noteMapText: String {
        if let d = drafts["map"] { return d }
        if case .noteMap(let m) = config.modify.op { return NoteMapText.text(m) }
        return ""
    }

    public func setNoteMapText(_ s: String) {
        drafts["map"] = s
        guard let m = NoteMapText.parse(s) else {
            invalid.insert("map")
            return
        }
        invalid.remove("map")
        config.modify.op = .noteMap(map: m)
    }

    public var offsetsText: String {
        if let d = drafts["offsets"] { return d }
        if case .quantize(_, _, let o) = config.modify.op { return OffsetsText.text(o) }
        return ""
    }

    public func setOffsetsText(_ s: String) {
        drafts["offsets"] = s
        guard let o = OffsetsText.parse(s), case let .quantize(g, st, _) = config.modify.op else {
            invalid.insert("offsets")
            return
        }
        invalid.remove("offsets")
        config.modify.op = .quantize(grid: g, strength: st, offsets: o)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`, Find:

```swift
    /// The editors (plan 4a).
    public let editor: EditorModel
```

Replace:

```swift
    /// The editors (plan 4a).
    public let editor: EditorModel
    /// The select-and-modify panel (plan 4c).
    public let selectModify = SelectModifyModel()
```

Find:

```swift
        editor.currentSequence = { [weak self] in self?.selectedSeq }
    }
```

Replace:

```swift
        editor.currentSequence = { [weak self] in self?.selectedSeq }
        selectModify.document = self
    }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | head; scripts/swift.sh test 2>&1 | tail -3
```

Expected: 406 tests pass (396 + 10).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/SelectModifyModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Tests/VizDAWAppModelTests/FakeSession.swift app/Tests/VizDAWAppModelTests/SelectModifyModelTests.swift
git commit -m "feat(app): select-and-modify model with drafts, templates and Modify Again

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 13: The floating panel, the Modify menu and the list's Modify… button

**Files:**
- Create: `app/Sources/VizDAW/ModifyPanel.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`
- Modify: `app/Sources/VizDAW/DocumentWindow.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift`
- Modify: `app/Sources/VizDAW/ListEditorPane.swift`

**Interfaces:**
- Consumes: Task 12's `SelectModifyModel` (every editing method), Task 11's kinds and `ModifyKind.title`, Task 6's `DocumentStore.{focusPanel, blurPanel}` and `EditorModel.{lastModify, modifyTemplates}`, `StatusLine`, `ScaleMath.rootNames`, `Scale`.
- Produces: `enum ModifyPanels { show(_ store:doc:), close(_ doc:) }` (one `NSPanel` per document); SwiftUI `SelectModifyPanelView`; the "Modify" menu (Rulings 9, 10): Select by Rule… ⌃⌘S, Transpose… ⌃⌘T, Quantize… ⌃⌘U, Change Values… ⌃⌘M, Move… ⌃⌘V, Modify Again ⌃⌘R, Templates ▸; `SelectModifyModel.onPresent` wired per document window; the list bar's "Modify…" opens the panel with the list's kind filter.

Ruling 22. The views hold no state: every field reads and writes `SelectModifyModel` (`conditionText`/`setConditionText`, `modifyText`/`setModifyText`), whose drafts keep half-typed text. This task is UI wiring over Task 12's tested model; its checks are a launch and the README steps of Task 15 (61–64, 66).

- [ ] **Step 1: Write the panel**

```bash
cat > app/Sources/VizDAW/ModifyPanel.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The floating select-and-modify panels, one per document (Plan 4c
/// Ruling 22). A closed panel is kept and shown again.
@MainActor
enum ModifyPanels {
    private static var open: [UUID: ModifyPanelController] = [:]

    static func show(_ store: DocumentStore, doc: UUID) {
        guard let model = store.model(doc) else { return }
        let c = open[doc] ?? ModifyPanelController(model: model, doc: doc)
        open[doc] = c
        c.showWindow(nil)
        c.window?.makeKeyAndOrderFront(nil)
    }

    /// The document is closing.
    static func close(_ doc: UUID) {
        open.removeValue(forKey: doc)?.close()
    }
}

@MainActor
final class ModifyPanelController: NSWindowController, NSWindowDelegate {
    private let model: DocumentModel
    private let doc: UUID

    init(model: DocumentModel, doc: UUID) {
        self.model = model
        self.doc = doc
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 620, height: 460),
            styleMask: [.titled, .closable, .resizable, .utilityWindow], backing: .buffered, defer: false)
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = true
        panel.becomesKeyOnlyIfNeeded = false
        panel.isReleasedWhenClosed = false
        panel.title = "Select and Modify — \(model.snapshot.title)"
        super.init(window: panel)
        panel.contentView = NSHostingView(
            rootView: SelectModifyPanelView(model: model.selectModify, editor: model.editor))
        panel.delegate = self
        panel.center()
    }

    required init?(coder: NSCoder) { nil }

    func windowDidBecomeKey(_ notification: Notification) {
        appStore.focusPanel(doc)
        model.selectModify.isKey = true
    }

    func windowDidResignKey(_ notification: Notification) {
        appStore.blurPanel(doc)
        model.selectModify.isKey = false
    }

    func windowWillClose(_ notification: Notification) {
        appStore.blurPanel(doc)
        model.selectModify.close()
    }
}

struct SelectModifyPanelView: View {
    @Bindable var model: SelectModifyModel
    let editor: EditorModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Picker("Task", selection: Binding(get: { model.task }, set: { model.open($0, filter: model.filter) })) {
                    ForEach(PanelTask.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .frame(width: 260)
                Spacer()
                Menu("Templates") {
                    if model.templates.isEmpty { Text("No Templates") }
                    ForEach(model.templates, id: \.name) { t in
                        Button("Load “\(t.name)”") { model.load(t) }
                    }
                    if !model.templates.isEmpty {
                        Divider()
                        ForEach(model.templates, id: \.name) { t in
                            Button("Delete “\(t.name)”") { model.deleteTemplate(t.name) }
                        }
                    }
                }
                .frame(width: 120)
            }
            Toggle("Select", isOn: $model.config.select.enabled).font(.headline)
            if model.config.select.enabled { SelectHalfView(model: model) }
            Divider()
            Toggle("Modify", isOn: $model.config.modify.enabled).font(.headline)
            if model.config.modify.enabled { ModifyHalfView(model: model) }
            Divider()
            HStack {
                TextField("Template name", text: $model.templateName).frame(width: 180)
                Button("Save as Template") { model.saveTemplate() }
                Spacer()
                Button("Apply") { model.apply() }.keyboardShortcut(.defaultAction)
            }
            StatusLine(editor: editor)
        }
        .padding(12)
        .frame(minWidth: 580, minHeight: 320, alignment: .topLeading)
    }
}

struct SelectHalfView: View {
    @Bindable var model: SelectModifyModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Picker("Result", selection: $model.config.select.mode) {
                    Text("Replace the Selection").tag(SelectModeRow.select)
                    Text("Add to the Selection").tag(SelectModeRow.add)
                    Text("Refine the Selection").tag(SelectModeRow.refine)
                }
                .frame(width: 280)
                Picker("Match", selection: $model.config.select.matchAll) {
                    Text("All Conditions").tag(true)
                    Text("Any Condition").tag(false)
                }
                .frame(width: 220)
            }
            ForEach(Array(model.config.select.conditions.indices), id: \.self) { i in
                ConditionRowView(model: model, index: i)
            }
            Menu("Add Condition") {
                ForEach(ConditionKind.allCases, id: \.self) { k in
                    Button(k.title) { model.addCondition(k) }
                }
            }
            .frame(width: 160)
        }
    }
}

struct ConditionRowView: View {
    let model: SelectModifyModel
    let index: Int

    var body: some View {
        if model.config.select.conditions.indices.contains(index) {
            let c = model.config.select.conditions[index]
            HStack(spacing: 6) {
                Text(ConditionKind(c.test).title).frame(width: 120, alignment: .leading)
                Picker("", selection: Binding(get: { Comparison.of(c) }, set: { model.setComparison($0, at: index) })) {
                    ForEach(Comparison.choices(for: c.test), id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .frame(width: 90)
                .accessibilityLabel("Comparison")
                switch c.test {
                case .kinds(let kinds):
                    KindsMenu(model: model, index: index, kinds: kinds)
                case .dest(let dest):
                    DestPicker(destinations: model.destinations, selection: dest) { model.setConditionDest($0, at: index) }
                default:
                    ForEach(Array(c.test.fields.enumerated()), id: \.offset) { j, f in
                        TextField(
                            f.label,
                            text: Binding(get: { model.conditionText(index, j) }, set: { model.setConditionText($0, index, j) })
                        )
                        .frame(width: 84)
                        .foregroundStyle(model.invalid.contains("c\(index).\(j)") ? Color.red : Color.primary)
                        .accessibilityLabel("\(ConditionKind(c.test).title) \(f.label)")
                    }
                }
                Spacer()
                Button {
                    model.removeCondition(at: index)
                } label: {
                    Image(systemName: "minus.circle")
                }
                .buttonStyle(.borderless)
                .help("Remove Condition")
                .accessibilityLabel("Remove Condition")
            }
        }
    }
}

struct KindsMenu: View {
    let model: SelectModifyModel
    let index: Int
    let kinds: [EventKindRow]
    static let all: [EventKindRow] = [
        .note, .cc, .pitchBend, .channelPressure, .polyPressure, .program, .sysEx, .text, .reference, .audioRegion,
    ]

    var body: some View {
        Menu(kinds.isEmpty ? "No Kind" : kinds.map { ListKind($0).title }.joined(separator: ", ")) {
            ForEach(Self.all, id: \.self) { k in
                Toggle(
                    ListKind(k).title,
                    isOn: Binding(
                        get: { kinds.contains(k) },
                        set: { on in model.setKinds(on ? kinds + [k] : kinds.filter { $0 != k }, at: index) }))
            }
        }
        .frame(width: 220)
    }
}

struct DestPicker: View {
    let destinations: [DestRow]
    let selection: UInt64?
    let set: (UInt64?) -> Void

    var body: some View {
        Picker("", selection: Binding(get: { selection }, set: set)) {
            Text("No Destination").tag(UInt64?.none)
            ForEach(destinations, id: \.id) { d in Text(d.name).tag(Optional(d.id)) }
        }
        .labelsHidden()
        .frame(width: 180)
        .accessibilityLabel("Destination")
    }
}

/// Substitute's choices (Plan 4c Ruling 5).
enum SubstituteChoice: String, CaseIterable {
    case controller, pitchBend, channelPressure, pitch

    init(_ s: SubstituteRow) {
        switch s {
        case .controller: self = .controller
        case .pitchBend: self = .pitchBend
        case .channelPressure: self = .channelPressure
        case .pitch: self = .pitch
        }
    }

    var title: String {
        switch self {
        case .controller: "Controller"
        case .pitchBend: "Pitch Bend"
        case .channelPressure: "Channel Pressure"
        case .pitch: "One Pitch"
        }
    }

    var row: SubstituteRow {
        switch self {
        case .controller: .controller(index: 1)
        case .pitchBend: .pitchBend
        case .channelPressure: .channelPressure
        case .pitch: .pitch(pitch: 36)
        }
    }
}

struct ModifyHalfView: View {
    @Bindable var model: SelectModifyModel

    private var op: ModifyRow { model.config.modify.op }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Picker("Operation", selection: Binding(get: { ModifyKind(op) }, set: { model.setModifyKind($0) })) {
                    ForEach(ModifyKind.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .frame(width: 260)
                Picker("Apply", selection: $model.config.modify.toCopy) {
                    Text("In Place").tag(false)
                    Text("To a Copy").tag(true)
                }
                .pickerStyle(.segmented)
                .frame(width: 200)
            }
            HStack(spacing: 6) {
                specials
                ForEach(Array(op.fields.enumerated()), id: \.offset) { j, f in
                    TextField(f.label, text: Binding(get: { model.modifyText(j) }, set: { model.setModifyText($0, j) }))
                        .frame(width: 84)
                        .foregroundStyle(model.invalid.contains("m\(j)") ? Color.red : Color.primary)
                        .accessibilityLabel(f.label)
                }
                Spacer()
            }
        }
    }

    @ViewBuilder private var specials: some View {
        switch op {
        case let .transposeDiatonic(_, root, mask), let .transposeToScale(root, mask):
            Picker("Root", selection: Binding(get: { root }, set: { model.setScale(root: $0, scale: Scale.of(mask: mask) ?? .major) })) {
                ForEach(0..<12, id: \.self) { i in Text(ScaleMath.rootNames[i]).tag(UInt8(i)) }
            }
            .frame(width: 110)
            Picker("Scale", selection: Binding(get: { Scale.of(mask: mask) ?? .major }, set: { model.setScale(root: root, scale: $0) })) {
                ForEach(Scale.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .frame(width: 190)
        case .noteMap:
            TextField("C2>D2, E2>F#2", text: Binding(get: { model.noteMapText }, set: { model.setNoteMapText($0) }))
                .frame(width: 260)
                .foregroundStyle(model.invalid.contains("map") ? Color.red : Color.primary)
                .accessibilityLabel("Note Map")
        case .quantize:
            TextField("Groove offsets", text: Binding(get: { model.offsetsText }, set: { model.setOffsetsText($0) }))
                .frame(width: 140)
                .foregroundStyle(model.invalid.contains("offsets") ? Color.red : Color.primary)
                .help("Groove offsets in ticks, one per grid line; empty for a plain grid")
        case let .change(target, changeOp, amount):
            Picker("", selection: Binding(get: { target }, set: { model.setTarget($0) })) {
                Text("Velocity").tag(ValueTargetRow.velocity)
                Text("Release Velocity").tag(ValueTargetRow.releaseVelocity)
                Text("Length").tag(ValueTargetRow.duration)
                Text("Value").tag(ValueTargetRow.value)
            }
            .labelsHidden()
            .frame(width: 140)
            .accessibilityLabel("Value to Change")
            Picker("", selection: Binding(get: { ChangeOpKind(changeOp) }, set: { model.setOp($0) })) {
                ForEach(ChangeOpKind.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .frame(width: 120)
            .accessibilityLabel("Operation")
            switch changeOp {
            case .set, .scale, .add:
                Picker("", selection: Binding(get: { AmountKind(amount) }, set: { model.setAmount($0) })) {
                    ForEach(AmountKind.allCases, id: \.self) { Text($0.title).tag($0) }
                }
                .labelsHidden()
                .frame(width: 130)
                .accessibilityLabel("Amount")
            default:
                EmptyView()
            }
            switch amount {
            case .line(_, _, let curve):
                Picker("", selection: Binding(get: { curve }, set: { model.setCurve($0) })) {
                    Text("Straight").tag(CurveRow.straight)
                    Text("Ease In").tag(CurveRow.easeIn)
                    Text("Ease Out").tag(CurveRow.easeOut)
                    Text("S-Curve").tag(CurveRow.sCurve)
                }
                .labelsHidden()
                .frame(width: 110)
                .accessibilityLabel("Curve")
            case .oscillator(let wave, _, _, _, _):
                Picker("", selection: Binding(get: { wave }, set: { model.setWave($0) })) {
                    Text("Sine").tag(WaveRow.sine)
                    Text("Square").tag(WaveRow.square)
                    Text("Triangle").tag(WaveRow.triangle)
                    Text("Saw").tag(WaveRow.saw)
                }
                .labelsHidden()
                .frame(width: 100)
                .accessibilityLabel("Wave")
            default:
                EmptyView()
            }
        case .setDest(let dest):
            DestPicker(destinations: model.destinations, selection: dest) { model.setModifyDest($0) }
        case .substitute(let to):
            Picker("", selection: Binding(get: { SubstituteChoice(to) }, set: { model.setSubstitute($0.row) })) {
                ForEach(SubstituteChoice.allCases, id: \.self) { Text($0.title).tag($0) }
            }
            .labelsHidden()
            .frame(width: 150)
            .accessibilityLabel("Substitute With")
        default:
            EmptyView()
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: The Modify menu, the window hook, closing and the list button**

In `app/Sources/VizDAW/AppCommands.swift`, Find:

```swift
        CommandMenu("Sequence") {
```

Replace:

```swift
        CommandMenu("Modify") {
            panelItem(.selectByRule, "s")
            panelItem(.transpose, "t")
            panelItem(.quantize, "u")
            panelItem(.change, "m")
            panelItem(.move, "v")
            Divider()
            Button(doc?.editor.lastModify().map { "Modify Again: \(ModifyKind.title($0))" } ?? "Modify Again") {
                doc?.selectModify.modifyAgain()
            }
            .keyboardShortcut("r", modifiers: [.command, .control])
            .disabled(doc?.editor.lastModify() == nil || doc?.editor.gesture != nil)
            Menu("Templates") {
                ForEach(doc?.editor.modifyTemplates() ?? [], id: \.name) { t in
                    Button(t.name) { doc?.selectModify.run(t) }
                }
            }
            .disabled((doc?.editor.modifyTemplates() ?? []).isEmpty)
        }
        CommandMenu("Sequence") {
```

Find:

```swift
    private func show(_ id: UUID?) {
```

Replace:

```swift
    /// A Modify menu command: the panel set to its task (Plan 4c Ruling 10).
    private func panelItem(_ task: PanelTask, _ key: Character) -> some View {
        Button("\(task.title)…") { doc?.selectModify.open(task) }
            .keyboardShortcut(KeyEquivalent(key), modifiers: [.command, .control])
            .disabled(doc == nil)
    }

    private func show(_ id: UUID?) {
```

In `app/Sources/VizDAW/DocumentWindow.swift`, Find:

```swift
                    .onAppear {
                        SmokeRun.start(model) { openWindow(id: "mixer", value: id) }
```

Replace:

```swift
                    .onAppear {
                        model.selectModify.onPresent = { ModifyPanels.show(store, doc: id) }
                        SmokeRun.start(model) { openWindow(id: "mixer", value: id) }
```

In `app/Sources/VizDAW/VizDAWApp.swift`, Find:

```swift
            PluginWindows.closeDocument($0)
            EditorWindows.closeDocument($0)
```

Replace:

```swift
            PluginWindows.closeDocument($0)
            EditorWindows.closeDocument($0)
            ModifyPanels.close($0)
```

In `app/Sources/VizDAW/ListEditorPane.swift`, Find:

```swift
            Toggle("MIDI In", isOn: $model.midiIn)
                .help("Play a note to enter its pitch and velocity")
```

Replace:

```swift
            Toggle("MIDI In", isOn: $model.midiIn)
                .help("Play a note to enter its pitch and velocity")
            Button("Modify…") { model.document?.selectModify.open(.change, filter: model.filter.viewFilter) }
                .help("Select and Modify, hiding the kinds this list hides")
```

- [ ] **Step 3: Build, test and launch**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | head; scripts/swift.sh test 2>&1 | tail -3
VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --smoke 2>&1 | grep "smoke:"
```

Expected: no new warnings; 406 tests pass; "smoke: ok". If the SwiftUI type-checker reports "unable to type-check this expression in reasonable time" on `specials`, split each `case` body into its own small `View` struct (same content); behaviour is unchanged. Then launch the demo (`VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --demo`), select the Chords block and press ⌃⌘T: the panel opens set to Transpose; Apply with 12 moves the chords up an octave.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAW/ModifyPanel.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/DocumentWindow.swift app/Sources/VizDAW/VizDAWApp.swift app/Sources/VizDAW/ListEditorPane.swift
git commit -m "feat(app): select-and-modify panel and the Modify menu

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 14: Exact mode

**Files:**
- Modify: `app/Sources/VizDAWAppModel/EditorModel.swift`
- Modify: `app/Sources/VizDAWAppModel/EditorModel+Gestures.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`
- Modify: `app/Sources/VizDAWEditors/PianoRollView.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`
- Modify: `app/Sources/VizDAW/PianoRollPane.swift`
- Test: `app/Tests/VizDAWAppModelTests/ExactGestureTests.swift` (new)
- Test: `app/Tests/VizDAWAppModelTests/PianoRollViewTests.swift`

**Interfaces:**
- Consumes: Task 11's `ExactMode`, `ModifyRequest`; Task 12's `SelectModifyModel.open(_ request:)`; the gesture API (`endGesture`, `cancelGesture`).
- Produces: `EditorModel.exactMode: Bool`, `EditorModel.onExact: ((ModifyRequest) -> Void)?` (set by `DocumentModel` to open its panel), `EditorModel.requestExact(_:)`; Modify > Exact Mode ⌃⌘E; the piano roll bar's "Exact" toggle.

Ruling 23. The drag previews as usual (intermediate edits, sound); at mouse-up `endGesture` asks `ExactMode.request(for:lanes:)`: a request cancels the gesture (`abortEdit`: the document is back as before, no undo step), makes the dragged events and their lanes the selection, and opens the panel. A drag with no request (pencil, eraser, cross-lane move, region move, expression stroke) ends as before. Escape cancels as before and opens nothing. The rubber band (which has no gesture) asks `ExactMode.selectRequest` instead of selecting.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/ExactGestureTests.swift`:

```swift
import Testing
import VizDAWCore
@testable import VizDAWAppModel

private let move200: EditCommand = .event(edit: .move(events: [1], delta: 200, toLane: nil))

@MainActor
@Suite struct ExactGestureTests {
    func open(exact: Bool) throws -> (FakeSession, DocumentModel, Counter) {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let c = Counter()
        m.selectModify.onPresent = { c.n += 1 }
        m.editor.exactMode = exact
        f.calls = []
        return (f, m, c)
    }

    final class Counter { var n = 0 }

    @Test func anExactDragLeavesNoUndoStepAndOpensThePanel() throws {
        let (f, m, c) = try open(exact: true)
        #expect(m.editor.beginGesture("Move Notes", lanes: [chords]))
        m.editor.updateGesture(move200)
        m.editor.endGesture(move200)
        #expect(f.calls == ["begin Move Notes", "gesture", "abort"], "previewed, then put back")
        #expect(m.editor.gesture == nil)
        #expect(c.n == 1 && m.selectModify.task == .move)
        #expect(m.selectModify.config.modify.op == .move(delta: 200))
        #expect(f.sel.events == [1] && f.sel.lanes == [LaneRef(seq: 3, lane: 31)], "it acts on the dragged notes")
    }

    @Test func escapeInExactModeOpensNothing() throws {
        let (f, m, c) = try open(exact: true)
        #expect(m.editor.beginGesture("Move Notes", lanes: [chords]))
        m.editor.updateGesture(move200)
        m.editor.cancelGesture()
        #expect(c.n == 0 && !m.selectModify.isOpen)
        #expect(f.calls.last == "abort")
    }

    @Test func aPencilClickInExactModeStillInserts() throws {
        let (f, m, c) = try open(exact: true)
        let insert: EditCommand = .event(edit: .insert(seq: 3, lane: 31, events: []))
        #expect(m.editor.beginGesture("Insert Note", lanes: [chords]))
        m.editor.endGesture(insert)
        #expect(f.calls.last == "end" && c.n == 0)
    }

    @Test func withExactModeOffADragApplies() throws {
        let (f, m, c) = try open(exact: false)
        #expect(m.editor.beginGesture("Move Notes", lanes: [chords]))
        m.editor.updateGesture(move200)
        m.editor.endGesture(move200)
        #expect(f.calls.last == "end" && c.n == 0)
    }
}
```

In `app/Tests/VizDAWAppModelTests/PianoRollViewTests.swift`, Find:

```swift
        #expect(Set(m.editor.selection.events) == [1, 2])
        #expect(m.editor.gesture == nil, "selecting is not an edit")
    }
```

Replace:

```swift
        #expect(Set(m.editor.selection.events) == [1, 2])
        #expect(m.editor.gesture == nil, "selecting is not an edit")
    }

    @Test func aRubberBandInExactModeOpensSelectByRule() throws {
        let (_, m, v) = try roll()
        m.editor.exactMode = true
        let a = point(v, 1, .start)
        let b = point(v, 2, .end)
        let from = CGPoint(x: a.x - 1, y: b.y - 15)
        let to = CGPoint(x: b.x, y: a.y + 15)
        v.contentMouseDown(from, clicks: 1, modifiers: [])
        v.contentMouseDragged(to, modifiers: [])
        v.contentMouseUp(to, modifiers: [])
        let s = m.selectModify
        #expect(s.isOpen && s.task == .selectByRule && !s.config.modify.enabled)
        guard case let .pitch(low, high)? = s.config.select.conditions.last?.test else {
            Issue.record("no pitch condition")
            return
        }
        #expect(low <= 60 && high >= 64, "the band's rows: C4 to E4 and a little more")
        #expect(m.editor.selection.events.isEmpty && m.editor.selection.timeRange != nil)
    }
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | grep -E "error:" | head -5
```

Expected: `value of type 'EditorModel' has no member 'exactMode'`.

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWAppModel/EditorModel.swift`, Find:

```swift
    public func beginTextEditing(_ owner: String) { textEditors.insert(owner) }
```

Replace:

```swift
    /// Exact mode (parent spec §9, Plan 4c Ruling 23): a drag opens the
    /// select-and-modify panel with its amounts instead of applying.
    public var exactMode = false
    /// Opens the panel for a request (set by `DocumentModel`).
    @ObservationIgnored var onExact: ((ModifyRequest) -> Void)?

    /// The request's events and lanes (or its range and lanes) become the
    /// selection, then the panel opens.
    public func requestExact(_ r: ModifyRequest) {
        setSelection(SelectionRow(timeRange: r.range, lanes: r.lanes.map(\.ref), events: r.events ?? []))
        onExact?(r)
    }

    public func beginTextEditing(_ owner: String) { textEditors.insert(owner) }
```

In `app/Sources/VizDAWAppModel/EditorModel+Gestures.swift`, Find:

```swift
    public func endGesture(_ final: EditCommand?) {
        guard var g = gesture else { return }
        g.finishing = true
```

Replace:

```swift
    public func endGesture(_ final: EditCommand?) {
        guard var g = gesture else { return }
        // Exact mode: the drag was a preview; put it back and ask instead.
        if exactMode, !g.finishing, let final, let r = ExactMode.request(for: final, lanes: g.lanes) {
            cancelGesture()
            requestExact(r)
            return
        }
        g.finishing = true
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`, Find:

```swift
        selectModify.document = self
```

Replace:

```swift
        selectModify.document = self
        editor.onExact = { [weak self] r in self?.selectModify.open(r) }
```

In `app/Sources/VizDAWEditors/PianoRollView.swift`, Find:

```swift
        if case let .band(from, adding) = d {
            clearBand()
            let found = PianoRollGeometry.notes(
                in: RubberBand.rect(from: from, to: p), lanes: laneNotes(), time: axis, pitch: pitchAxis)
```

Replace:

```swift
        if case let .band(from, adding) = d {
            clearBand()
            let rect = RubberBand.rect(from: from, to: p)
            if editor.exactMode {
                let start = axis.tick(atX: rect.minX)
                let end = max(axis.tick(atX: rect.maxX), start + 1)
                let a = pitchAxis.pitch(atY: rect.minY)
                let b = pitchAxis.pitch(atY: rect.maxY)
                editor.requestExact(
                    ExactMode.selectRequest(
                        range: TickRange(start: start, end: end), pitches: min(a, b)...max(a, b),
                        lanes: model.shownLanes, adding: adding))
                return
            }
            let found = PianoRollGeometry.notes(
                in: rect, lanes: laneNotes(), time: axis, pitch: pitchAxis)
```

In `app/Sources/VizDAW/AppCommands.swift`, Find:

```swift
            .disabled((doc?.editor.modifyTemplates() ?? []).isEmpty)
        }
```

Replace:

```swift
            .disabled((doc?.editor.modifyTemplates() ?? []).isEmpty)
            Divider()
            Toggle(
                "Exact Mode",
                isOn: Binding(get: { doc?.editor.exactMode ?? false }, set: { doc?.editor.exactMode = $0 })
            )
            .keyboardShortcut("e", modifiers: [.command, .control])
            .disabled(doc == nil)
        }
```

In `app/Sources/VizDAW/PianoRollPane.swift`, Find:

```swift
            Toggle("Audio", isOn: setting(\.showRegions))
```

Replace:

```swift
            Toggle("Exact", isOn: $editor.exactMode)
                .help("Exact mode: a drag opens Select and Modify with its amounts (⌃⌘E)")
            Toggle("Audio", isOn: setting(\.showRegions))
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build 2>&1 | grep -E "warning:|error:" | head; scripts/swift.sh test 2>&1 | tail -3
```

Expected: 411 tests pass (406 + 5); the existing gesture tests still pass (exact mode is off by default).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/EditorModel.swift app/Sources/VizDAWAppModel/EditorModel+Gestures.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWEditors/PianoRollView.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/PianoRollPane.swift app/Tests/VizDAWAppModelTests/ExactGestureTests.swift app/Tests/VizDAWAppModelTests/PianoRollViewTests.swift
git commit -m "feat(app): exact mode opens select-and-modify with the dragged amounts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---
### Task 15: The 4c checklist and the final gate

**Files:**
- Modify: `app/README.md`

**Interfaces:**
- Consumes: everything above.
- Produces: README steps 51–66 (spec §6: a manual checklist per phase, including drag feel, torn windows and VoiceOver).

- [ ] **Step 1: Append the checklist**

In `app/README.md`, Find:

```markdown
    typed and playback does not start; Space in the piano roll itself still
    plays and stops.
```

Replace:

```markdown
    typed and playback does not start; Space in the piano roll itself still
    plays and stops.

### List editor and Select and Modify (plan 4c)

Use the demo project (File > Create Demo Project) unless a step says
otherwise.

51. List: double-click the Chords block and press Cmd-2: the detail pane
    shows the list with Position, Length, Kind, Destination, Data 1, Data 2
    and Mute columns and one row per chord note ("Note", its pitch name and
    velocity 0–127). The
    arrow keys move the cursor; the selected rows are the document's
    selection (Cmd-1 shows those notes selected in the piano roll).
52. Bar typing: on the first row press Return: its Position cell edits.
    Type 3 and press Return: the note moves to bar 3's downbeat, the list
    scrolls to it and keeps it selected, and Edit shows "Undo Move Events".
    In another row type ".3": only its beat changes. Type "abc": the status
    line says it is not a position and the cell shows the old value.
53. Cells: press Return on a row, then Tab: the cursor moves to Length,
    Data 1 (type D4), Data 2 (type 100); each commit is one undo step "Edit
    Event"; Shift-Tab goes back; Escape leaves the cell unchanged.
54. Typing never plays: while a cell is edited, Space types a space and the
    transport does not start; with the list focused but not editing, Return
    edits the cursor row (it does not return to the start) and Space plays.
55. Insert: click "Insert Note": a C4 quarter note appears at the cursor
    row's position and is selected. Insert ▾ > Controller adds CC 7 = 64 and
    the button reads "Insert Controller"; Cmd-I (Editors > Insert Event)
    adds another controller. Insert ▾ offers no Reference or Audio Region.
56. Columns ▾: turn on End, Release Velocity and SMPTE, frame rate 25 fps:
    the columns appear. Type 00:00:02:00 in a note's SMPTE cell: it moves to
    bar 2 (two seconds at 120 BPM). Quit and relaunch: the column choices
    are kept.
57. Show ▾: turn off Controller: the controller rows disappear; turn off
    the Chords lane's destination: the Chords notes disappear. Option-Cmd-2 tears off
    another list: it shows everything. In the first list choose Apply to All
    Lists: the torn list hides the same. Save as Default, then File > New
    and Create Demo Project: its list starts with that filter. Show
    Everything restores the rows.
58. Whole sequence: click Song in the sidebar, click empty arrangement
    space (nothing selected) and press Cmd-2: the list shows a Tempo row
    (120.00, jump) and a Meter and Key row (4/4, C major) at 1.1.0, then
    every lane's rows with a Lane column. Insert ▾ > Marker adds "Marker 1";
    type 100 in the Tempo row's Data 1: the tempo changes (Undo Set Tempo).
59. References: Song's list shows "Reference", "Groove", "×1 0" rows. Type
    "×2 +12" in one's Data 2: that use repeats twice an octave up. Press
    Cmd-Down on it (or double-click its Kind cell): the list and the
    arrangement show Groove; Sequence > Back to Parent Sequence returns the
    arrangement to Song.
60. MIDI In (needs a MIDI keyboard connected before turning it on): turn on
    MIDI In, select a note row and play a key: the row takes the played
    pitch and velocity (Undo Edit Event). Edit a Data 1 cell and play: the
    note name appears in the cell; Return commits it.
61. Modify menu: select the Chords block, Modify > Transpose… (Ctrl-Cmd-T):
    the floating "Select and Modify — Untitled" panel opens set to
    Transpose. Type 12 and press Apply: the chords move up an octave, Edit
    shows "Undo Transpose". Choose "To a Copy" and type 4, Apply: a
    harmonizing copy a third higher appears; the originals stay.
62. Select half: Modify > Select by Rule… (Ctrl-Cmd-S) with the Chords block
    selected: Kind is Note and Position covers the block. Add Condition >
    Pitch, "inside", C4 to E4, Apply: only those notes are selected and no
    undo step is added. Add Condition > Velocity "is" 100 with Result
    "Refine the Selection". Click an empty arrangement spot (nothing
    selected) and Apply: the status says "Select lanes to search first…".
63. Shapes: Modify > Change Values… (Ctrl-Cmd-M): Velocity, Set to, Along a
    Line, from 20 to 120, Apply: the list shows velocities rising across the
    selection. Oscillating, Square, low 30, high 110, period 0.480: they
    alternate. Type "abc" in a field: it turns red and Apply says "Fix the
    fields marked in red first."
64. Templates and Modify Again: type "Octave Up" in Template name and Save
    as Template: Modify > Templates lists it, and after Save, close and
    reopen it is still there. Select other notes: Modify > Templates >
    Octave Up runs it at once. Modify > Modify Again: Transpose
    (Ctrl-Cmd-R) repeats the last modification on the current selection.
65. Exact mode: Modify > Exact Mode (Ctrl-Cmd-E; the piano roll bar's Exact
    toggle shows it). Drag a chord note's left edge one beat right: it
    follows the pointer and, on release, goes back while the panel opens on
    Move with "1.0"; Apply moves it as one undo step. A middle-zone drag up
    three rows opens Transpose 3; an end-zone drag opens Change Values,
    Length, Add; a rubber band opens Select by Rule with Position and Pitch
    conditions. Escape during a drag opens nothing; a pencil click still
    inserts. Turn Exact Mode off: drags apply again.
66. Panel and VoiceOver: with the panel in front, Cmd-Z undoes in its
    document and Space does not play; close it and reopen it with Ctrl-Cmd-M:
    it comes back where it was. With VoiceOver (Cmd-F5) the list reads as a
    table "Event List" with the column names as headers and rows such as
    "Note C4, bar 1 beat 1, velocity 100" (with the row's own values); the
    panel's pickers and fields
    read their labels (Comparison, Amount, Curve, "Pitch from"…).
```

- [ ] **Step 2: Final gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "^test result" | awk '{s+=$4} END {print "rust passed:", s}'
cargo test -p core-engine --features rt-check --test rt_alloc 2>&1 | tail -2
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -c "warning:" ; scripts/swift.sh test 2>&1 | tail -2
VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --smoke 2>&1 | grep "smoke:"
scripts/make-app.sh 2>&1 | tail -1
```

Expected: fmt and clippy clean; 660 Rust tests pass; rt_alloc passes; no new Swift warnings; 411 Swift tests pass; "smoke: ok"; "built dist/VizDAW.app".

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/README.md
git commit -m "docs(app): list editor and select-and-modify checklist (plan 4c)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
```

---

## Self-Review

**1. Spec coverage** (spec §4c and parent §9, item by item):

| Requirement | Task |
|---|---|
| List editor shows every event kind; references and regions are rows | 8 (`ListRows`, `ListText`), 9, 10 |
| Typing a bar number moves to that downbeat | 7 (`PositionInput`), 8, 9 (`theCursorFollowsAMovedEvent`) |
| A single sub-field edited alone | 7 (".3", "..480", "5.."), 8 |
| Enter commits and the list follows the moved event | 9 (`follow`), 10 (`ended`, `syncCursor`) |
| Pitch and velocity by playing a note (MIDI input) | 5 (`NoteListener`, `NoteInput`), 9 (`receive`, `NoteInputHub`), 10 (ticker, `applyInput`) |
| Insert repeats the last inserted kind | 8 (`ListInsert`), 9 (`insertRepeatsTheLastKind`), 10 (⌘I, bar button) |
| Optional columns End, Length, Release velocity, SMPTE | 5 (`seconds_at`, `ticks_at`), 7 (`Smpte`), 8 (`ListColumns`), 9, 10 |
| Type/destination filters applied to all lists or saved as default | 8 (`ListFilter`), 9 (`applyFilterToAll`, `saveFilterAsDefault`, `ListPreferences`), 10 (Show ▾) |
| Opening a reference opens its sequence | 9 (`openReference`), 10 (⌘↓, double-click) |
| Tempo, meter, keys, markers editable in the list (parent §5) | 8 (map rows), 9 |
| Keyboard-first navigation; transport keys off while typing | 6 (owners, Return rule), 10 |
| Select half: kinds and attributes (time, pitch, velocity, duration, value, destination, metric position, bracketing, every Nth) with equal/not equal/inside/outside; replace/add/refine | 1 (rules), 4 (flat records), 11 (`ConditionKind`, `Comparison`), 12, 13 |
| Modify half: transpose chromatic/diatonic/inversion/to scale/note map, harmonizing copy | 2, 4, 11, 12, 13 ("To a Copy") |
| Velocity, release velocity, duration, value by set/scale/add/limit/compress/legato, constant/shaped/oscillating amounts | 2, 4, 11, 12, 13 |
| Quantize, density, move, scale time, trim start, set destination, substitute, delete; in place or to a copy | 2, 3, 4, 11, 12, 13 |
| Named templates in a menu, one-step recall | 1 (`to_copy`), 4 (`SettingsEdit`, `modify_templates`), 12, 13 |
| Modify Again | 4 (`EditCommand::ModifyAgain`), 6 (`lastModify`), 12, 13 |
| Transpose/quantize/modify/move/select-by-rule menu commands open the panel set to their task | 11 (`PanelTask` presets), 12 (`open`), 13 (Modify menu) |
| Exact mode | 11 (`ExactMode`), 14 |
| Torn windows as command targets; panel as command target | 6 (`panelFocus`), 10 (lists in torn windows), 13 |
| Accessibility (the list is a VoiceOver table) | 10 (`theTableIsAnAccessibleTable`), 13 (labels), 15 (step 66) |
| Busy contract | 14 (exact mode goes through `cancelGesture`/`abortEdit`; final edits still retried when exact mode does not apply); Ruling 22 for Apply |
| README checklist after step 50 | 15 |
| Ledger carry-overs (4a T2 round trips, 4b T3 negative FFI tests) | 5, 4 |

No requirement is without a task. Gaps the spec leaves open are ruled (Rulings 1–26) and listed under "Spec gaps".

**2. Placeholder scan:** no "TBD", "TODO", "implement later" or "similar to Task N"; every code step has its code. Task 13 has no new automated tests by design (SwiftUI views in the executable target; the model it drives is Task 12's, tested); its checks are the launch in Step 3 and README steps 61–66.

**3. Type consistency** (names used across tasks): `ListRow.id` "e<id>"/"t<i>"/"m<i>"/"k<i>" (8 → 9, 10); `ListChange.edit(_, tick:)` (8 → 9); `ListCell(row:column:)` (9 → 10); `ListEditorView.textOwner == "list"` and `"laneName"` (6, 10); `EditorModel.{panelKey, listFocused, textEditing, beginTextEditing, endTextEditing}` (6 → 10, 12, 13); `EditorModel.{lastModify(), modifyTemplates()}` (6 → 12, 13); `DocumentModel.{list(_:), dropList(_:), activeList, listForCommands(), listensForNotes, receiveNotes(_:), listPreferences, selectModify, returnKeyEnabled}` (6, 9, 10, 12 → 13, 14); `SelectModifyModel.open(_:filter:)` / `open(_ request:)` (12 → 13, 14); `ModifyRequest` / `ExactMode.request(for:lanes:)` / `selectRequest(range:pitches:lanes:adding:)` (11 → 14); FFI `EditCommand.{selectAndModify(rule:mode:modify:toCopy:), modifyAgain, settings(edit:)}`, `EventEdit.replace(event:row:)`, `SettingsEdit.{saveTemplate(template:), deleteTemplate(name:)}` (4 → 8, 9, 11, 12); `Session.{modifyTemplates(), lastModify(), secondsAt(seq:ticks:), ticksAt(seq:seconds:)}` (4, 5 → 6, 9); `NoteInputProtocol.{start(), stop(), take()}` (5 → 9). Core: `apply_modify -> Result<Vec<EventId>>` (2 → 3), `Amount::at` (2), `Density`, `Substitute` (3 → 4).

**4. Review Focus:** each of the five lines has its named tests in its owning task (Tasks 3, 6, 7, 8, 9, 10, 12, 14). Other input classes checked and covered elsewhere: 100 000-event lists (`buildingRowsForAHundredThousandEventsIsQuick`), unknown event IDs at the FFI (`the_newer_event_variants_refuse_bad_input_and_change_nothing`), a gesture still finishing when exact mode would apply (`endGesture` checks `!g.finishing`), templates saved by older builds (`a_template_saved_before_to_copy_existed_still_loads`).
