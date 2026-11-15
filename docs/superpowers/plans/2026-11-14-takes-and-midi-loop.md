# Choosing Takes, and MIDI Loop Recording That Layers Implementation Plan (5c-2 takes)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the owner's bug of 2026-11-14. Every track gets a Takes menu to choose, rename and delete takes, with control-channel commands. Loop-recording MIDI layers each pass into the playing take by default, while audio passes still become takes. The Replace/Overdub mode shows on the transport bar beside Record.

**Architecture:**
- **`core-model`.** `LaneCmd` gains `RenameTake` and `RemoveTake`, and `Document` gains `remove_take`, which takes the take's events out of the event index. `SetActiveTake` already exists. Each is one History step. The file format does not change.
- **`core-ffi`.** `LaneEdit` gains the two matching variants, which run through the existing `edit` export. `LaneRow` gains `takes: Vec<TakeRow>` and `active_take_id`. `state.get lanes` gains `take_list`. The pass planner (`passes.rs`) does not change: with `passes_as_takes` off it already merges MIDI and still makes audio takes. Only its docs change.
- **Swift.**
  - The track header's context menu and the arrangement's region menu get a shared "Takes" submenu (`TakesMenu`, in `VizDAWEditors`).
  - A new `TakeCommands` adds `track.choose_take`, `track.rename_take` and `track.delete_take`.
  - The record-options switch becomes "MIDI passes as takes", default off, under a new preferences key.
  - New documents start in Overdub.
  - `RecordControls` shows a Replace/Overdub switch beside Record.

**Tech Stack:** Rust 2024 (`core-model`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`). Swift 6.1 / SwiftPM with Swift Testing, macOS 14+. No new crate, no new dependency.

**Spec:** the owner's approval of 2026-11-14 below is the binding spec. It amends `docs/superpowers/specs/2026-10-18-recording-audio-design.md` §6.4 and §6.9 (Ruling 1 lists the lines). Executors read both. The name "5c-2" is shared with the earlier Track Synth plan (`2026-10-26-synth-tracking.md`), so this plan's code comments and commits say "plan 5c-2 takes".

> **The owner's bug (2026-11-14):** "when I pencil draw notes so there's something in arrange, if I cycle record it overwrites (erases) the previous recording/penciled notes are destroyed. There's no way to select a previous take, and there's no overdub?"
>
> **What the owner approved (2026-11-14). These are the binding requirements:**
> 1. **A take picker.** Each track gets a Takes menu (on the track, and in the region's context menu) to choose which take plays, rename a take, and delete a take. Deleting a take is a normal undo step. Add a control-channel command (or commands) for choosing, renaming and deleting, with hostile-input tests, and raise the registry count accordingly.
> 2. **MIDI loop recording that layers by default.** Loop-recording MIDI over existing notes merges each pass into the active take by default, so the parts build up, as in overdub. Audio loop passes still become takes, because they cannot merge. Keep a way to get MIDI passes as takes. Decide whether that is the existing "Passes as takes" switch, re-scoped, or a new "MIDI passes as takes" switch, and state your call. Say exactly which spec lines change. Replace still removes only the events in the recorded range, as specced.
> 3. **(Controller's call, owner told.)** Show the Replace/Overdub mode on the transport bar beside Record, as a small visible switch. The popover keeps its copy.

**What the code does today** (`cfe30a1`):
- "Passes as takes" is on by default. A loop pass becomes a new take ("Take n"), the last one is made active, and so the pencilled notes are hidden in Take 1.
- The model has `SetActiveTake` and `AddTake`, but no command, menu or channel call reaches them.
- With the switch off, `passes::plan` already merges MIDI passes into the active take, and audio loop passes still become takes.
- `TransportMarks.overdub` defaults to `false`, so new documents start in Replace. Replace clears the recorded range of the active take once, before the passes go in. With Replace and merged passes, the pencilled notes in the loop range still go.

**Not in this plan:**
- Comping, or showing takes as stacked tracks.
- Choosing, renaming or deleting a take that is not playing from the menu. Choose it first. The commands take any take.
- A Settings default for Replace/Overdub.
- Edits to the e2e `records_audio_loop_takes_and_midi`, which another agent is fixing. It sets `passes_as_takes` and `replace` itself, so this plan does not change what it does.

## Plan Rulings

These are this plan's decisions where the approval leaves a choice. Each names its cost if wrong.

1. **Item 2's switch: the existing "Passes as takes", re-scoped and renamed "MIDI passes as takes", default off.** The code already applies the switch to MIDI only, because audio loop passes become takes either way. A second switch would therefore have nothing to do.
   - **Kept names.** The wire names stay: `RecordSpecRow.passes_as_takes`, `record.set`'s `passes_as_takes` param and `state.get record`'s field. Scripts and the e2e keep working, and their docs say "MIDI only".
   - **Renamed in Swift.** The property is renamed `midiPassesAsTakes` in `AudioSettings`, `RecordDefaults` and `RecordModel`. `AudioSettings` stores it under the new key `midiPassesAsTakes`. The old `passesAsTakes` key is not read, so a `true` saved by an earlier build does not carry over and the new default holds.
   - **Labels.** The popover and Settings › Recording › New Documents both read "MIDI passes as takes". The help is "Loop recording: each MIDI pass becomes a new take. Off: passes layer into the playing take. Audio passes always become takes."
   - **Spec lines that change** in `2026-10-18-recording-audio-design.md`:
     - **273:** "`passes_as_takes: bool` (default true)." becomes "`passes_as_takes: bool` (default false; MIDI only, since audio loop passes always become takes; the UI calls it "MIDI passes as takes"; plan 5c-2 takes)."
     - **289:** "With `passes_as_takes`, each kept pass, …" becomes "*MIDI* with `passes_as_takes`: each kept pass, plus the last pass if not discarded, becomes a new take named "Take n", and the last becomes the active take."
     - **290:** "Without it, MIDI passes merge …" becomes "*MIDI* without it (the default): every kept pass merges into the active take, so the parts build up. Replace clears the recorded range once, before the passes go in, so it removes what was there before the run, never an earlier pass of the same run. *Audio:* each kept pass becomes a take, since audio cannot merge."
     - **341:** "A loop pass makes matching MIDI and audio takes." becomes "A loop pass makes an audio take; the MIDI lane's pass is a take too with `passes_as_takes`, else it merges."
     - **407:** the Record popover lists "MIDI passes as takes". Add a bullet after it: "a Replace/Overdub switch beside Record, the same mode as the popover's (plan 5c-2 takes)."
     - **428:** "passes as takes;" becomes "MIDI passes as takes (default off);".
     - **704:** "a loop pass makes MIDI and audio takes together." becomes "a loop pass makes MIDI and audio takes together when `passes_as_takes` is on."
     - **640** does not change, because the param name stays.
   - **Cost if wrong:** someone who wants MIDI takes turns the switch on once in Settings › Recording. It is still in the popover too.
2. **New documents start in Overdub.** `TransportMarks.overdub` now defaults to `true`. Item 2's "over existing notes merges … by default" is only true if the default mode keeps the existing notes, because Replace removes them. A document whose `ui.json` saved `overdub` keeps its mode, and the transport switch (item 3) shows it.
   - **Cost if wrong:** in new documents, a MIDI pass that is not looped merges instead of replacing the range. Audio is unchanged, because audio overdub already replaces what it overlaps (spec line 284). Reverting is one default.
3. **Replace in a looped MIDI run does not change.** The clear covers the windows of the kept passes, and it runs once before any pass is inserted. Every pass of the run stays, and notes outside the windows stay. A test pins this (Task 3).
4. **The take model commands.**
   - **`RenameTake { seq, lane, take, name }`** uses `lane::valid_name`. Its refusal is "a take name cannot be empty or hold control or zero-width characters". The name is stored trimmed. Its undo label is "Rename Take".
   - **`RemoveTake { seq, lane, take }`**:
     - an unknown take is `UnknownId(Take)`;
     - the track's only take is refused with "a track keeps at least one take";
     - removing the active take makes the take before it active, or the next one when it was first;
     - the report lists every event of the take and the take itself under `removed`;
     - media stays in the pool, so Undo brings back its regions playing;
     - its undo label is "Delete Take".
   - **`SetActiveTake`** keeps its name and label, "Set Active Take".
5. **The Takes menu** (`TakesMenu.item(document:seq:lane:) -> NSMenuItem`, title "Takes"):
   - one item per take, in order, titled with its name, with the playing take checked;
   - a separator;
   - "Rename “<playing take>”…";
   - "Delete “<playing take>”", disabled when the track has one take.

   The menu goes in the header's `laneMenu`, after "New MIDI Destination…" and before the Colour separator. It also goes in `ArrangementView.menu(for:)`'s block menu, for the block's track, just above the separator before "Delete".
   - **Rename** asks through a new hook, `DocumentModel.askTakeName: (@MainActor (_ current: String) -> String?)?`. `DocumentView` installs it with `TextPanel.run(title: "Rename Take", message: "A new name for “<name>”:", initial: <name>, ok: "Rename", label: "Take name")`. With no hook installed, the status line says "Show the document window to rename a take." Only the menu calls the hook, never the channel.
   - **The header** shows the playing take when a track has more than one: `LaneHeaderLayout.nameText(_ lane: LaneRow) -> String` gives "<name> · <take>", else "<name>". The header draws it and uses it as the name's accessibility label.
   - **While recording**, choosing, renaming and deleting a take are refused with `TrackCommands.recordingBusy`, as Rename and Delete Track are.
6. **The control commands.** They bring the registry from 183 to **186**. Check the count at the start, because plan 8a's executor shares the branch, and add 3 to whatever it is.
   - **`track.choose_take`** takes `seq` and `lane` (`opt_id_param`, defaulting as `track.rename` does) and `take` (`id_param`, required). Class `Change`.
   - **`track.rename_take`** takes the same plus `name` (Text, required). Class `Change`.
   - **`track.delete_take`** takes `seq`, `lane` and `take`. Class `Remove`.

   All three are menu "Track", `document: true`, with no dialogs. They answer `status` with the take's name: the chosen take's, the new name, or the deleted take's.
   - **Refusals:**
     - a NUL, or a blank `name`, is `invalid_params`;
     - a take that the track does not have is `unknown_id` "There is no take <n> on this track.";
     - the only take is `failed` "A track keeps at least one take.";
     - during recording the answer is `busy`;
     - unknown sequences and tracks are refused in `track.rename`'s words.
7. **The transport switch.** `RecordControls` shows, between Record and the Record Options chevron, `AppKitButton(record.modeTitle, isOn: !record.replaces, enabled: !record.isRecording, help: RecordModel.modeHelp) { record.toggleMode() }`.
   - `modeTitle` is "Replace" or "Overdub".
   - The accessibility label is `record.modeLabel`, "Record mode: Replace" or "Record mode: Overdub".
   - `modeHelp` is "Replace removes the notes in the recorded range; Overdub keeps them (transport.overdub)".
   - `toggleMode()` runs `TransportCommands.run("transport.overdub", m)`, so a refusal (a text field holding the keyboard) shows as it does for the key.
   - The bar is shared, so the mixer window's bar shows the switch too.
   - The chevron's help becomes "Replace or overdub, punch, loop, count-in, pre-roll and MIDI takes".

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift.**
  - Build only through `scripts/build-ffi.sh` (after any FFI change) and `scripts/swift.sh build|test`, plus `scripts/make-app.sh` in Task 4. Never run `swift`, `xcodebuild` or `xcrun` directly.
  - Swift 6.1:
    - no `[weak self]`, and no non-Sendable value captured inside `MainActor.assumeIsolated`;
    - no new `@State` or `@FocusState`: view state lives in the `@Observable` models.
  - Every new control has an accessibility label. Colours, fonts and spacing come from theme tokens (`theme.text…`, `theme.type…`, `theme.spacing…`).
- **Tests never touch the machine.**
  - They never open real audio or MIDI devices, and never trigger the microphone or TCC prompt.
  - They never touch the real `~/Library`. Use temp folders, `MemoryDefaults()`, the e2e's throwaway home and fakes.
  - Every `core-ffi` test file calls `force_offline()` through its `serial` helper.
  - Swift tests that make a real `Session` set `TestSupportDir.shared`.
  - Linux CI has no AU and no CoreFoundation, so such tests are `#[cfg(target_os = "macos")]`.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`, and never read `~/workspace/SVP/research`.
- **Programmatic calls open nothing.** Control-channel commands and programmatic calls never open dialogs, sheets, alerts, panels, menus or windows. Only the Takes menu's Rename item calls `askTakeName`.
- **Hostile input.**
  - New FFI goes through the existing `edit` export, inside `ffi_guard` (and so `catch_unwind`).
  - Each new control command gets hostile-input tests: wrong types, unknown params, missing required params, a 70 000-character string, a NUL, and IDs out of range or unknown.
  - The registry count goes up by exactly 3 (Ruling 6), both in `crates/core-control/tests/commands.rs` and in `crates/core-control/tests/co_producer.rs`.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list. That list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`). Read them, keep new text clear of the list, and never copy it.
  - Text the app shows says "track", never "lane". Identifiers and JSON keys keep `lane`.
- **Commits.**
  - Stay on branch `sp6-import` (at `cfe30a1` when this plan was written). Never switch branches, and never push. Other agents commit on this branch, so retry if `.git/index.lock` is held.
  - Commit at the end of every task by explicit paths, never `git add -A`. Never commit `.superpowers/`, `.DS_Store`, `target/`, generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`) or `dist/`.
  - Every message ends with exactly these two lines:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
  - Check each commit with `git log -1 --oneline && git show --stat HEAD`.
  - `Cargo.lock` does not change.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and the `clippy::{unwrap_used, expect_used, indexing_slicing, panic}` denials outside tests. `core-model` stays free of OS APIs.
- **Format.** `core_model::file::VERSION` stays 9, because takes are already in the file.
- **TDD.** Every new or changed test is run, and seen to fail, before the code that makes it pass.
- **Each task ends green.**
  - Always:
    - `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy, `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches.
  - Tasks 1–3 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Known flaky: the `damaged_files` and `dawproject_fuzz` proptests, `platform-macos/tests/midi.rs`, `core-ffi` catalog tests under load, and the e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline.** Measure at the start of Task 1: the registry (183 when written), the last README step (325), the ignored e2e count (31) and `VERSION` (9). Each task reports the counts it adds.

## Review Focus

These are the five failure modes a user is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **Pencilled notes vanish again.**
   - Examples:
     - a new project, with notes pencilled into a MIDI track, then three loop passes with default settings;
     - the same after Undo;
     - an old project whose `ui.json` says Replace.
   - Expected:
     - with the defaults, the pencilled notes and every pass's notes play from one take, and the take count is unchanged;
     - one Undo removes only the passes' notes;
     - an old project's Replace is visible on the bar before Record.
   - Pinned by Task 3's `pencilled_notes_survive_a_merged_loop_run` and `aNewDocumentOverdubsAndLayersMidiPasses`.
2. **Deleting a take loses or breaks something.**
   - Examples:
     - deleting the playing take, or the first take;
     - trying to delete the only take;
     - deleting a take holding audio regions, then Undo;
     - a stale take ID after Undo.
   - Expected:
     - the take before it plays, or the next one when it was first;
     - the only take is refused, and nothing changes;
     - Undo brings back the take, its events (same IDs, findable by `locate`), its place in the order and which take was playing;
     - the region's media is still in the pool;
     - no panic, and the event index stays exact.
   - Pinned by Task 1's `remove_take_unindexes_and_moves_the_active_take`, `remove_take_refuses_the_last_and_unknown` and `deleting_a_take_keeps_its_media`.
3. **An old preference brings the old behaviour back.**
   - Examples:
     - preferences saved by an earlier build with `"passesAsTakes": true`;
     - a damaged value under the new key.
   - Expected:
     - MIDI passes layer, and the switch reads off;
     - a damaged value reads as off.
   - Pinned by Task 3's `midiPassesAsTakesDefaultsOffAndIgnoresTheOldKey`.
4. **Hostile take commands.**
   - Examples:
     - `take` as a string, negative, `1e30` or past `i64::MAX`;
     - a take on another track;
     - `name` with a NUL, blank, a zero-width space, or 70 000 characters;
     - an unknown param;
     - any take command while recording.
   - Expected: `invalid_params`, `unknown_id`, `failed` or `busy` per Ruling 6. Nothing changes, no undo step is made, and no dialog opens.
   - Pinned by Task 2's `take_commands_check_their_params` and `takeCommandsRefuseHostileInput`.
5. **The two Replace/Overdub controls disagree.**
   - Examples:
     - switching on the bar, then opening the popover;
     - `transport.overdub` over the channel;
     - the mixer window's bar;
     - a click while recording.
   - Expected:
     - the bar, the popover and `state.get record`'s spec show the same mode;
     - while recording, the switch is disabled and the mode does not change.
   - Pinned by Task 3's `theBarSwitchIsTheTransportsMode`.

---

### Task 1: Rename and delete takes in the model, and takes in the snapshot

**Files:**
- Modify:
  - `crates/core-model/src/command/lane.rs` (the two variants, their names, `apply`);
  - `crates/core-model/src/document.rs` (`remove_take`);
  - `crates/core-model/tests/cmd_lane.rs`;
  - `crates/core-ffi/src/edit.rs` (the two `LaneEdit` variants and their mapping);
  - `crates/core-ffi/src/snapshot.rs` (`TakeRow`, `LaneRow.takes`, `LaneRow.active_take_id`);
  - `crates/core-ffi/src/control.rs` (`take_list` in `state.get` lanes);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift`, if its `LaneRow`s need the new fields.
- Create: `crates/core-ffi/tests/takes.rs`.

**Interfaces:**
- Produces (`core-model`):
  ```rust
  // LaneCmd gains:
  /// Rename a take (plan 5c-2 takes Ruling 4).
  RenameTake { seq: SeqId, lane: LaneId, take: TakeId, name: String },   // "Rename Take"
  /// Delete a take; the track keeps at least one (Ruling 4).
  RemoveTake { seq: SeqId, lane: LaneId, take: TakeId },                 // "Delete Take"
  impl Document {
      /// Removes a take and unindexes its events; moves `active_take` per Ruling 4.
      pub(crate) fn remove_take(&mut self, seq: SeqId, lane: LaneId, take: TakeId) -> Result<Take>;
  }
  ```
- Produces (`core-ffi`):
  ```rust
  // LaneEdit gains (u64 IDs, like SetActiveTake):
  RenameTake { seq: u64, lane: u64, take: u64, name: String },
  RemoveTake { seq: u64, lane: u64, take: u64 },
  /// One take of a lane, in order.
  #[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
  pub struct TakeRow { pub id: u64, pub name: String, pub midi_event_count: u32, pub region_count: u32 }
  // LaneRow gains, both #[uniffi(default)]:
  pub takes: Vec<TakeRow>,
  pub active_take_id: u64,
  ```
  `state.get` lanes gains `"take_list": [{"id", "name", "active"}]`. `takes` (the count) and `active_take` (the name) keep their meaning.
  In Swift these are `LaneEdit.renameTake(seq:lane:take:name:)`, `.removeTake(seq:lane:take:)`, `TakeRow` and `LaneRow.takes`/`activeTakeId`.

- [ ] **Step 1: Write the failing model tests** in `cmd_lane.rs`. Use the existing fixture style: a sequence, a lane "Keys" with two notes in Take 1, then `AddTake "Take 2"` with one note, then `AddTake "Take 3"`, which is empty and active.
  - `rename_take_checks_its_name`:
    - `RenameTake { take: t1, name: "  Verse " }` stores "Verse";
    - each of `""`, `"   "`, `"a\u{0}b"` and `"a\u{200B}b"` is `Err(Invalid)` with "a take name cannot be empty or hold control or zero-width characters", and the document is unchanged;
    - an unknown take is `UnknownId(Take)`.
  - `remove_take_unindexes_and_moves_the_active_take`:
    - removing Take 3, which is active, makes Take 2 active;
    - removing Take 1 while Take 2 is active leaves Take 2 active, with `takes == [Take 2]`;
    - with Take 1 active, removing it makes the next take active;
    - each removed take's events fail `d.locate(id)`;
    - the report's `removed` holds each of the take's events and `AnyId::Take(id)`;
    - `rules::check` passes after each removal.
  - `remove_take_refuses_the_last_and_unknown`:
    - on a lane with one take, removing it is `Err(Invalid)` "a track keeps at least one take";
    - an unknown take is `UnknownId(Take)`;
    - the document is unchanged after each.
  - `take_edits_undo_as_one_step_each`, through `History`:
    - `RemoveTake` of Take 2 is one step labelled "Delete Take";
    - `undo` brings back Take 2 in its place, with its note's ID, which `locate` finds, and with the take that was active before;
    - `RenameTake` is labelled "Rename Take".
- [ ] **Step 2: Run** `cargo test -p core-model --test cmd_lane`. Expected: FAIL (no variants).
- [ ] **Step 3: Implement** the variants, their `name()` arms, their `apply` arms and `Document::remove_take`, following `remove_lane`/`unindex_lane`. Ruling 4 gives the rules.
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS. Fix any exhaustive `match` on `LaneCmd` elsewhere in the workspace, such as the DAWproject or co-producer code paths, so that `cargo build --workspace` compiles.
- [ ] **Step 5: Write the failing `core-ffi` tests** in `takes.rs`, with a `serial` helper (offline). Use `Session::from_document` over a document built as in Step 1.
  - `lane_rows_list_takes`:
    - `lanes(seq)` gives "Keys" `takes == [TakeRow{t1,"Take 1",2,0}, TakeRow{t2,"Take 2",1,0}, TakeRow{t3,"Take 3",0,0}]` and `active_take_id == t3`, with `take_count == 3` and `active_take == "Take 3"` unchanged;
    - the `state.get` lanes JSON (through the function `control.rs` uses) has `take_list[2] == {"id": t3, "name": "Take 3", "active": true}`.
  - `choosing_renaming_and_deleting_takes_through_edit`:
    - `edit(Lane(SetActiveTake{take: t1}))` makes `midi_event_count == 2`, with undo label "Set Active Take" and the document dirty;
    - `RenameTake{t1, "Verse"}` gives label "Rename Take" and the row name "Verse";
    - `RemoveTake{t2}` gives label "Delete Take";
    - `undo` brings t2 back;
    - `RemoveTake` with `u64::MAX` is `Err` with the undo label unchanged.
  - `deleting_a_take_keeps_its_media`:
    - a document whose Take 2 holds one audio region over a pool item (as the existing region tests build one);
    - `RemoveTake{t2}`: the media pool still lists the item;
    - `undo`: the take's `region_count == 1` again.
- [ ] **Step 6: Run** `cargo test -p core-ffi --test takes`. Expected: FAIL.
- [ ] **Step 7: Implement** the `LaneEdit` variants and mapping, `TakeRow`, the two `LaneRow` fields filled in `snapshot.rs`, and `take_list`.
- [ ] **Step 8: Run** the Step 6 test, then `cargo test -p core-ffi`. Expected: PASS.
- [ ] **Step 9: End-of-task checks**, including `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. **Commit** `feat(model): rename and delete takes, and takes in the lane snapshot (plan 5c-2 takes Task 1)`.

### Task 2: The Takes menu and the three take commands

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs`;
  - `crates/core-control/tests/commands.rs`;
  - `crates/core-control/tests/co_producer.rs` (add the ids to `CHANGE` and `REMOVE`, and bump the count);
  - `app/Sources/VizDAWAppModel/TrackCommands.swift` (factor out its seq/lane resolution for reuse);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (dispatch and `handled`);
  - `app/Sources/VizDAWAppModel/DocumentModel.swift` (`askTakeName`);
  - `app/Sources/VizDAWEditors/LaneHeaderView.swift` (`laneMenu`, and the name drawn through `nameText`);
  - `app/Sources/VizDAWEditors/ArrangementLayout.swift` (`LaneHeaderLayout.nameText`);
  - `app/Sources/VizDAWEditors/ArrangementView.swift` (the block menu);
  - `app/Sources/VizDAW/DocumentView.swift` (installs `askTakeName`);
  - `app/Tests/VizDAWAppModelTests/ArrangementViewTests.swift` (`theLaneMenuHasNoDrumTrackItem`'s titles gain "Takes" before "Colour").
- Create:
  - `app/Sources/VizDAWAppModel/TakeCommands.swift`;
  - `app/Sources/VizDAWEditors/TakesMenu.swift`;
  - `app/Tests/VizDAWAppModelTests/TakeCommandsTests.swift`.

**Interfaces:**
- Consumes: Task 1's `LaneEdit.setActiveTake`, `.renameTake`, `.removeTake`, `LaneRow.takes` and `activeTakeId`.
- Produces (Swift):
  ```swift
  @MainActor public enum TakeCommands {
      public static let choose = "track.choose_take"
      public static let rename = "track.rename_take"
      public static let delete = "track.delete_take"
      public static let ids: Set<String>
      public static let onlyTake = "A track keeps at least one take."
      public static let noAsk = "Show the document window to rename a take."
      /// The menu's entries; a refusal shows in the status line.
      public static func choose(seq: UInt64, lane: UInt64, take: UInt64, _ m: DocumentModel) -> KeyOutcome
      public static func rename(seq: UInt64, lane: UInt64, take: UInt64, to name: String, _ m: DocumentModel) -> KeyOutcome
      public static func delete(seq: UInt64, lane: UInt64, take: UInt64, _ m: DocumentModel) -> KeyOutcome
      /// The menu's Rename…: asks through m.askTakeName, then rename.
      public static func renameAsking(seq: UInt64, lane: UInt64, take: UInt64, _ m: DocumentModel)
      public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome
  }
  // DocumentModel
  @ObservationIgnored public var askTakeName: (@MainActor (_ current: String) -> String?)?
  // VizDAWEditors
  @MainActor public enum TakesMenu {
      public static func item(document: DocumentModel, seq: UInt64, lane: LaneRow) -> NSMenuItem
  }
  extension LaneHeaderLayout { public static func nameText(_ lane: LaneRow) -> String }
  ```
- **The registry entries.** Ruling 6 gives them. The params are `TAKE_PARAMS` (`seq`, `lane`, and `id_param("take", "The take (see state.get lanes take_list).")`) and `TAKE_RENAME_PARAMS` (the same plus a required Text `name`, with help "The take's new name.").

- [ ] **Step 1: Write the failing command tests** in `commands.rs` and `co_producer.rs`.
  - Both counts go up by 3. In `commands.rs`, append "; 5c-2 takes adds track.choose_take, track.rename_take and track.delete_take" to the message.
  - `take_commands_check_their_params`:
    - `{take: 3}` parses for choose and delete, and `{take: 3, name: "Verse"}` parses for rename;
    - each of these is `invalid_params`:
      - `take` missing;
      - `take: "3"`, `-1`, `1e30` or `true`;
      - rename without `name`;
      - `name: 5`;
      - a 70 000-character `name`;
      - an unknown param.
- [ ] **Step 2: Write the failing Swift tests.**
  - **`TakeCommandsTests.swift`.** Use the `editorFake()` DocumentModel that `TrackCommandsTests` uses. Its track 30 gets takes `[(300, "Take 1"), (301, "Take 2")]`, with 301 active.
    - `chooseRenameDeleteRunOnTheTrack`. The fake records each edit:
      - `run("track.choose_take", {lane: 30, take: 300})` sends `setActiveTake(seq: 3, lane: 30, take: 300)` and answers status "Take 1";
      - `rename_take {lane: 30, take: 300, name: "Verse"}` sends `renameTake(…, "Verse")` and answers "Verse";
      - `delete_take {lane: 30, take: 301}` sends `removeTake` and answers "Take 2".
    - `takeCommandsRefuseHostileInput`:
      - `name: "a\u{0}b"` and `name: "  "` are `.invalidParams`;
      - `take: 999` is `.unknownId` "There is no take 999 on this track.";
      - delete on a one-take track is `.failed` with `onlyTake`;
      - `lane: 99` is refused in `track.rename`'s words;
      - the fake records no edit for any of these, `askTakeName` is never called, and no alert is raised.
    - `takeCommandsWaitWhileRecording`: with the fake recording, each of the three is `.busy(TrackCommands.recordingBusy)`, with no edit sent.
    - `renameAskingUsesTheHook`:
      - with `askTakeName = { _ in "Chorus" }`, `renameAsking` sends `renameTake(…, "Chorus")`;
      - with the hook returning nil, nothing is sent;
      - with no hook, `editor.status == TakeCommands.noAsk`.
  - **`ArrangementViewTests.swift`.**
    - `theTakesMenuListsAndActs`:
      - `TakesMenu.item(document: m, seq: 3, lane: <track 30>)` has the title "Takes";
      - its submenu's titles are `["Take 1", "Take 2", "Rename “Take 2”…", "Delete “Take 2”"]`, separators aside, with only "Take 2" checked;
      - performing "Take 1"'s action sends `setActiveTake(…, 300)`;
      - on a one-take track, "Delete “Take 1”" is disabled.
    - `theHeaderNamesThePlayingTake`: `nameText` is "Keys · Take 2" for two takes, and "Keys" for one.
    - `theRegionMenuHasTakes`: `ArrangementView.menu(for:)` on a block of track 30 holds a "Takes" item. Build the event as the existing block-menu tests do. If none exist, test a `static func blockMenu(…)` that `menu(for:)` calls instead.
- [ ] **Step 3: Run** `cargo test -p core-control --test commands --test co_producer` and `scripts/build-ffi.sh && scripts/swift.sh test --filter 'TakeCommandsTests|ArrangementViewTests'`. Expected: FAIL.
- [ ] **Step 4: Implement:**
  - the registry entries;
  - `TakeCommands`, which reuses `TrackCommands`' seq/lane resolution, does its refusals in Swift before any edit (Ruling 6), and edits through `m.editor.edit(.lane(edit: …))`;
  - the dispatch case beside `TrackCommands`', plus `.union(TakeCommands.ids)` in `handled`;
  - `askTakeName`, and its install in `DocumentView` next to where `beginTrackRename`'s owner is set up;
  - `TakesMenu`;
  - the two menus' new items (Ruling 5);
  - `nameText`, used for the drawn name and the name's accessibility label.
- [ ] **Step 5: Run** the Step 3 commands. Expected: PASS. `NameScanTests` and `core-control/tests/names.rs` pass with the new text.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(arrange): a Takes menu on every track and region, and the take commands (plan 5c-2 takes Task 2)`.

### Task 3: MIDI loop passes layer by default, new documents overdub, and the bar's mode switch

**Files:**
- Modify:
  - `app/Sources/VizDAWAppModel/AudioPreferences.swift` (`midiPassesAsTakes`, key `midiPassesAsTakes`, default false; `setRecordDefaults(countIn:preRoll:midiPassesAsTakes:)`);
  - `app/Sources/VizDAWAppModel/RecordModel.swift` (`midiPassesAsTakes = false`; `spec()` passes it as `passesAsTakes`);
  - `app/Sources/VizDAWAppModel/RecordOptions.swift` (`modeTitle`, `modeLabel`, `modeHelp`, `toggleMode()`, `midiTakesHelp`);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (`record.set`'s `passes_as_takes` sets `midiPassesAsTakes`);
  - `app/Sources/VizDAWAppModel/TransportMarks.swift` (`overdub = true`);
  - `app/Sources/VizDAW/RecordControls.swift` (the switch, the popover toggle's label and help, the chevron's help);
  - `app/Sources/VizDAW/AudioSettingsView.swift` (the toggle's label);
  - `crates/core-ffi/src/record.rs` and `crates/core-ffi/src/passes.rs` (docs only: `passes_as_takes` is MIDI only, default off in the app);
  - `crates/core-control/src/registry.rs` (the `passes_as_takes` help, which says "MIDI loop passes become takes (audio always does)");
  - `crates/core-ffi/tests/passes.rs`;
  - `docs/superpowers/specs/2026-10-18-recording-audio-design.md` (Ruling 1's lines);
  - Swift tests that pin the old defaults: `AudioPreferencesTests`, `RecordModelTests` (line 49's `passesAsTakes:`), `TransportCommandsTests.bareKeyCommandsWaitForTextFields` (its `!overdub` becomes `overdub`), `RecordOptionsTests`.

**Interfaces:**
- Consumes: nothing from Tasks 1–2.
- Produces (Swift):
  ```swift
  // AudioSettings, RecordDefaults, RecordModel: passesAsTakes → midiPassesAsTakes (default false)
  // RecordModel (RecordOptions.swift)
  public var modeTitle: String          // "Replace" | "Overdub"
  public var modeLabel: String          // "Record mode: Replace" | "Record mode: Overdub"
  public static let modeHelp: String    // Ruling 7
  public static let midiTakesHelp: String // Ruling 1's help
  public func toggleMode() -> KeyOutcome   // TransportCommands.run("transport.overdub", m)
  ```

- [ ] **Step 1: Write the failing Rust test** `pencilled_notes_survive_a_merged_loop_run` in `crates/core-ffi/tests/passes.rs`, using its `Case` builder.
  - **The fixture.** The doc's MIDI lane holds two pencilled notes: one inside the loop window, at tick `Q`, and one outside it. A loop run of three passes plays one note each, with `takes: false`.
  - **With `replace: false`:**
    - `plan` gives a `LaneRecord` with empty `takes` and `clear`, and the three notes in `active`;
    - applying it as `Command::Record` leaves one take, holding all five notes;
    - one `undo` leaves the two pencilled notes.
  - **With `replace: true`:**
    - `clear` covers the passes' windows;
    - after applying, the note outside the window and the three pass notes remain, and the pencilled note at `Q` is gone (Ruling 3).
  - **With `takes: true`:** three new takes, the last active, and Take 1 still holds both pencilled notes.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test passes pencilled`. Expected: PASS at once, because the planner does not change. Confirm the test fails when the `active.events` assertion is inverted, then restore it. That is this step's red check, and the report says so.
- [ ] **Step 3: Write the failing Swift tests.**
  - `AudioPreferencesTests.midiPassesAsTakesDefaultsOffAndIgnoresTheOldKey`:
    - a new `AudioSettings` has `midiPassesAsTakes == false`;
    - decoding `{"passesAsTakes": true}` gives false;
    - `{"midiPassesAsTakes": true}` gives true;
    - `{"midiPassesAsTakes": "yes"}` gives false;
    - `setRecordDefaults(countIn: 0, preRoll: 0, midiPassesAsTakes: true)` stores it and sets `RecordModel.defaults`.
  - `RecordModelTests.aNewDocumentOverdubsAndLayersMidiPasses`:
    - a new `DocumentModel` gives `record.spec().replace == false` and `passesAsTakes == false`;
    - `TransportMarks()` decoded from `{}` has `overdub == true`, and from `{"overdub": false}` has false.
  - `RecordOptionsTests.theBarSwitchIsTheTransportsMode`:
    - `modeTitle == "Overdub"` and `modeLabel == "Record mode: Overdub"`;
    - `toggleMode()` gives `.done`, then `"Replace"`, `replaces == true`, and `spec().replace == true`;
    - `record.set {replace: false}` brings back "Overdub";
    - with a text field holding the keyboard (`m.beginRename(3)`), `toggleMode()` is `.unavailable` and the mode is unchanged.
  - Flip the existing assertions that pinned the old defaults (the files above), and name each in the report.
- [ ] **Step 4: Run** `scripts/swift.sh test --filter 'AudioPreferencesTests|RecordModelTests|RecordOptionsTests|TransportCommandsTests'`. Expected: FAIL.
- [ ] **Step 5: Implement** the rename and key, the two defaults, the model members and the view.
  - The switch goes between Record and the chevron, per Ruling 7. `AppKitButton` takes `modeTitle` as its title, and `modeLabel` is set as its accessibility label.
  - The popover's and Settings' toggles become `Toggle("MIDI passes as takes", …)` with `.help(RecordModel.midiTakesHelp)`.
  - The Rust docs change, and so do the spec lines in Ruling 1, word for word.
- [ ] **Step 6: Run** the Step 4 command, then `cargo test -p core-ffi --test passes` and `cargo test -p core-control`. Expected: PASS.
- [ ] **Step 7: End-of-task checks**, including the Swift build and test. **Commit** `feat(record): MIDI loop passes layer by default, new documents overdub, and Replace/Overdub on the transport bar (plan 5c-2 takes Task 3)`.

### Task 4: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (step 134; new steps after the last one);
  - `crates/vizctl/tests/e2e.rs` (a new test only);
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–3. No production code changes, unless a step finds a bug. Fix such a bug in the module that owns it, with a regression test.

- [ ] **Step 1: Add the e2e** `takes_and_layered_midi_loop_over_the_channel`, `#[ignore]`. Put it after the last e2e. Use the same fixture shape as `records_audio_loop_takes_and_midi` (four bars at 120 BPM, `SeqLength::Locked(16 * Q)`, a "Keys" MIDI lane with input port "Keys" on the built-in synth), but do not edit that test or its helpers' behaviour.
  - **The fixture.** "Keys" holds two notes in Take 1 (one at tick 0, one at `8 * Q`), and the file has no `ui.json`.
  - **The defaults:**
    - `state.get record`'s spec has `replace == false` and `passes_as_takes == false`;
    - `ui.tree` holds an `AXButton` labelled "Record mode: Overdub", found by a helper written like `play_button`;
    - `ui.click` on it turns the spec's `replace` to true, and a second click turns it back.
  - **Layering.** Arm Keys and run `record.set {loop: true}`. Record, inject one note in pass 1 and one in pass 2, and wait with the existing frame-based helpers, as that test does. Then Stop and `wait_for_undo "Record"`:
    - Keys has `takes == 1` and `events == 4`;
    - after `edit.undo`, `events == 2`.
  - **Takes.** Run `record.set {passes_as_takes: true}`, then a two-pass loop with one injected note each, then Stop:
    - `takes == 3` and `active_take == "Take 3"`;
    - from `take_list`, `track.choose_take {take: <Take 1 id>}` answers "Take 1", gives `events == 2` and leaves undo "Set Active Take";
    - `track.rename_take {take: <Take 1 id>, name: "Verse"}` answers "Verse";
    - `track.delete_take {take: <Take 3 id>}` gives `takes == 2`;
    - `edit.undo` gives `takes == 3`;
    - `track.delete_take {take: 999999}` is `unknown_id`.
- [ ] **Step 2: Write the README steps** in `app/README.md`.
  - **Amend step 134** to begin "Loop passes: arm an audio track (a MIDI track makes takes only with MIDI passes as takes on: step <N+3>), turn Loop on …". The rest stays.
  - **Add the steps.** After the last step (`N` = 325 when written), add a heading "Plan 5c-2 takes, choosing takes and MIDI loop recording that layers:". Each step states what to do and what is seen, and says "track":
    - **N+1.** File › New: the transport bar shows "Overdub" beside Record. Pencil four notes into a MIDI track. Turn Loop on and record three passes, playing different notes each time. Everything plays together from one take, the pencilled notes are still there, and the header shows no take name. One Undo removes only what you played.
    - **N+2.** Click "Overdub" on the bar: it reads "Replace", and the Record ⌄ popover's Mode says Replace too. Loop-record over the same range: the notes in the loop range are replaced by what you played across the passes, and notes outside the loop stay. Click back to "Overdub".
    - **N+3.** Settings › Recording › New Documents: turn on "MIDI passes as takes", then make a new project, or turn it on in the Record ⌄ popover. Loop-record two MIDI passes. Right-click the track's header › Takes: "Take 1" to "Take 3", with the last checked, and the header reads "<track> · Take 3". Choose Take 1: the pencilled notes play again.
    - **N+4.** Takes › Rename "Take 1"…: type "Verse". The header shows it. Undo puts the old name back. Right-click a region on the track: the same Takes menu.
    - **N+5.** Takes › Delete "Verse": the take goes, and the one before it plays, or the next when it was first. Undo brings it back with its notes. On a track with one take, Delete is greyed out.
    - **N+6.** Arm an audio track and loop-record two passes with "MIDI passes as takes" off: you get two takes anyway. Choose between them in Takes, and each plays its pass.
    - **N+7.** Open a project saved before this build that recorded in Replace: the bar shows "Replace" before you press Record.
    - **N+8.** Look at the transport bar at its narrowest, in the document window and in the mixer window: the mode switch sits between Record and ⌄, and nothing is clipped or overlaps.
- [ ] **Step 3: Update the backlog** (`docs/feature-backlog.md`):
  - On the "Customize the transport bar" row, append: "Plan 5c-2 takes put a Replace/Overdub switch beside Record; make it one of the bar's items."
  - **New rows:**
    - "Comping: takes shown as stacked tracks, with a part of each chosen";
    - "Rename or delete a take that is not playing, from the Takes menu";
    - "A Settings default for Replace/Overdub in new documents";
    - any narrow edge cases the task reports listed, at the owner's review pace.
- [ ] **Step 4: Run the gates:**
  - `cargo test --workspace`;
  - both clippies;
  - `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `scripts/build-ffi.sh`;
  - `scripts/swift.sh build`;
  - `scripts/swift.sh test`;
  - the no-brand build, `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <fresh temp dir>`;
  - `scripts/make-app.sh`;
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: every ignored test runs, one more than the baseline, and all pass.

  Then `pgrep -lf MacOS/VizDAW` prints nothing. Report against the baseline: registry +3, README +8, e2e +1, `VERSION` 9.
- [ ] **Step 5: Commit** `docs(app): takes and layered MIDI loop README checks, e2e and backlog (plan 5c-2 takes Task 4)`.

## Self-Review

1. **Spec coverage.**
   - **Item 1:**
     - the Takes menu on the track and in the region's menu: Ruling 5 and Task 2;
     - choose, rename and delete: Task 1 (model and FFI) and Task 2 (UI and commands);
     - delete as a normal undo step: `take_edits_undo_as_one_step_each`;
     - the commands, their hostile-input tests and the count: Ruling 6 and Task 2.
   - **Item 2:**
     - MIDI layering by default: Rulings 1–2 and Task 3;
     - audio passes still become takes: the unchanged planner, `pencilled_notes_survive_a_merged_loop_run` and README N+6;
     - a way to keep MIDI takes: the re-scoped switch (Ruling 1);
     - the spec lines: Ruling 1 lists 273, 289, 290, 341, 407, 428 and 704;
     - Replace only removes events in the range: Ruling 3, pinned in Task 3.
   - **Item 3:** the bar switch, with the popover keeping its picker: Ruling 7 and Task 3.
2. **Steps.** Every test step names its tests and assertions with the plan's values: IDs, names, sentences and counts. Code steps give signatures and point to the ruling that fixes the behaviour.
3. **Type consistency.** These names are spelled the same in every task:
   - `RenameTake`, `RemoveTake`, `remove_take`, `TakeRow`, `LaneRow.takes`, `active_take_id`/`activeTakeId`, `take_list`;
   - `TakeCommands.choose`, `.rename`, `.delete`, `renameAsking`, `askTakeName`, `TakesMenu.item`, `LaneHeaderLayout.nameText`;
   - `midiPassesAsTakes`, `modeTitle`, `modeLabel`, `modeHelp`, `toggleMode`, `midiTakesHelp`;
   - `track.choose_take`, `track.rename_take`, `track.delete_take`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 1–3.
5. **Proportion.** Four tasks at the owner's lighter pace. Code appears only as signatures. The plan reuses `SetActiveTake`, the existing merge path in `passes::plan`, the `edit` export, `TrackCommands`' resolution, `TextPanel` and `AppKitButton`.
6. **Owner decisions.** None are open. Ruling 1 (the switch re-scoped, with its new key) and Ruling 2 (new documents overdub) are stated for the owner's review.
