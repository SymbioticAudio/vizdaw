# DAWproject Merge Implementation Plan (6e)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A DAWproject file that went from VizDAW to another DAW and back can be merged onto the project it came from: the other DAW's edits arrive, everything only VizDAW knows about stays, VizDAW's own edits win every conflict, and the whole merge is one undo step, "Undo Merge DAWproject".

**Architecture:**
- **The three sides meet as projections** (spec §12's form, which 6d's `record::Projection` already is):
  - *base* is the snapshot 6d keeps in the project (`Interchange/<export id>.json`);
  - *theirs* is the returning file, imported by 6d's mapping into a scratch project at our format, then projected;
  - *ours* is the projection of the project as it is now.

  6d's round-trip tests prove that an unchanged file projects to the base within stated tolerances. Task 1 turns those tolerances into production code (`as_returned`), so "unchanged" has one definition.
- **`core-io`** gains `dawproject::merge`:
  - `theirs`: the scratch import, with the tags and trusted XML IDs it found (`Origins`);
  - `matching`: the three sides' tracks, strips, markers, regions and notes paired (§7.8 "Matching");
  - `decide`: the rule table applied per unit, giving a `MergePlan` with its preview and report;
  - `apply`: the plan as model commands inside the caller's transaction.

  `record` gains the snapshot's format 2 (identities and the region-gain option), a bounded reader and the normalisers. `import` keeps the tags it used to strip and drop. `report` gains the `Conflict` kind.
- **`core-model`** gains one undoable command, `MixerCmd::SetPluginState`. `core-host`'s `sync` already loads a slot's state when the document's blob changes, so undo reloads the old settings.
- **`core-ffi`** gains a `MergeDawproject` job:
  - it opens the file, finds and checks the snapshot, converts the audio as 6d's import does, and maps;
  - it builds theirs and the plan, then applies the plan to a clone of the project's history. All of this happens off the session lock.
  - The session swaps the merged history in only if the project has not changed since. Three steps (`Preview`, `Hold`, `Apply`) serve the control channel's `preview`, the sheet's Merge button and a plain channel merge.
- **The control channel's** `file.import_dawproject` gains `mode: merge`, `project`, `snapshot` and `preview`, and answers `dialog_required` "Return" and "Open" as spec §10 says.
- **Swift** gains:
  - the store-level return prompt (an `NSAlert`, like `StoreAlerts`): "New Project", "Merge With Existing Project…" or Cancel;
  - `MergeModel` and `MergeSheet`: the preview page on the target project's window, with the snapshot list when the file lost its export ID;
  - the report's "Conflicts" group.

**Tech Stack:** Rust 2024 (`core-io`, `core-model`, `core-host` tests only, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6.1 / SwiftPM (Swift Testing), macOS 14+. No new crates.

**Spec:** `docs/superpowers/specs/2026-11-11-import-export-design.md`. Plan 6e's row of §13 covers:
- §2 goal 5a;
- §7.8's read side: the prompt, finding the project, matching, the three-way merge, the conflicts take, the preview and the Conflicts group;
- §6.1's merge row ("Undo Merge DAWproject"), and §6.2 and §6.3 as they apply;
- §9's return prompt and its preview page, and the report's "Conflicts" group;
- §10's `mode`/`project`/`snapshot`/`preview` on `file.import_dawproject`, `dialog_required` "Return" and "Open", and the merge hostile tests;
- §11;
- §12's "Merge (6e)" block, the DAWproject part of the owner check, and the README checks.

Build on what 6d built. Read it; do not re-plan it. Names changed during 6d's execution, so the code wins over 6d's plan:
- `core-io::dawproject`:
  - `record` (`Snapshot`, `Projection`, `PTrack`, `PNote`, `PRegion`, `PStrip`, `ObjectRef`, `RecordHead`, `projection`, `project`, `SNAPSHOT_DIR`, `KEEP_SNAPSHOTS`, `FORMAT`);
  - `package` (`open`, `DpPackage`, `ReturnInfo::from_vizdaw`, `DpInfo`);
  - `import` (`map`, `apply`, `DpImport`, `ImportEnv`, `MediaSource`, `SlotStates`, `NOT_NEW`, `FROM_VIZDAW`);
  - `import::timeline` (`Arranged`, `Item`, `RegionPlan`);
  - `units` (`TagKind`, `tag`, `strip_tags`, `EXPORT_LINE`);
  - `export` (`export`, `ExportOptions`, `RegionGain`, `DpExport.snapshot_json`, `Layout`, `Part`).
- `core-io`'s tests: `common::package` (`write_package`, `package_bytes`, `TestPlist`), `dawproject_round_trip.rs` (`same`, `copy_hash`, `without_holds`, `from_tick`, the proptest generators), `schema::{read_project, write_project, write_metadata}`.
- `core-ffi`:
  - `dawproject.rs` (`info`, `installed`, `ImportJob`, `import_body`, `import_job`, `keep_snapshot`, `keep_snapshots`, `snapshot_name`, `snapshots_in`, `DAWPROJECT_FILE`, `plist()`);
  - `session.rs` (`import_dawproject`, `apply_dawproject_work`, `apply_dawproject_import`, `interchange_idle`, `media_jobs_idle`, `remove_unkept`, `kept`, `reserved_names`, `root`, the snapshot capture in `export_dawproject`);
  - `interchange.rs` (`file_path`, `io_error`, `report_row`, `ReportRow`, `ReportKindRow`);
  - `jobs.rs` (`JobKindRow`, `JobRow`, `Work`).
- `core-model`: `History::{begin, commit, abort, execute, present_serial, capture_state_keeping, undo}`, `LaneCmd::{AddTake, SetActiveTake, Remove, Rename}`, `ReferenceCmd::MakePrivateCopy`, `EventCmd::{Insert, Delete}`, `AudioCmd::*`, `MixerCmd::*`, `SequenceCmd::{SetTempo, SetMeter, SetMarkers}`, `SettingsCmd::SetInfo`, `state::blob_id`.
- Swift:
  - `DocumentStore.openDawproject`, `dawprojectInfo`, `closeIfUntouched`, `documentID(forPath:)` and `StoreAlerts`;
  - `InterchangeCommands` (`importDawproject`, `importDawprojectIntoNewProject`, `madeProjects`);
  - `DawprojectModel` (its detached-task and generation pattern), `ReportText`, `ReportSheet`, `ProjectPanels` and `FakeSession`.

## Global Constraints

Every task's requirements include these.

- **Shell.**
  - Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
  - Swift goes only through `scripts/build-ffi.sh` (after any FFI change), `scripts/swift.sh build|test` and `scripts/make-app.sh`. Never run `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Branch `sp6-import`, at `ed475c1` with plans 6a–6d finished, then this plan's commit. Never switch branches, never push.
  - Commit at the end of every task by explicit paths (never `git add -A`). Never commit `.superpowers/`, `.DS_Store` or generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`).
  - `Cargo.lock` does not change: this plan adds no dependency.
  - Every message ends with the executing session's own attribution lines: its harness's `Co-Authored-By` line naming the model actually running, then the session line, e.g.
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Then check with `git log -1 --oneline && git show --stat HEAD` (a commit without `DEVELOPER_DIR` fails silently).
- **Clean room and names.**
  - Never read `~/workspace/SVP/research`.
  - No Opcode, Logic, Apple (beyond platform facts already in the specs), Midas, Soundcraft, NI, Microsoft, Ableton, Steinberg/Cubase or ROLI names in UI or repo text, and none of the DAW names the DAWproject README lists. Say "other DAWs" or "the other DAW".
  - **Ardour** ideas may be borrowed (the alternate-take-on-conflict idea is one). Ardour is never named in code, comments, tests, UI text, report text, registry help or README steps. Only docs and the backlog may name it.
  - The DAWproject format's origin appears only in its reference URL and the vendored `LICENSE`, as in 6d. Test fixtures' `Application@name` is "Fixture" or "VizDAW", and "the other DAW" in a test is a hand-edited file, never a named product.
  - The scans `NameScanTests.noThirdPartyNamesInWhatTheAppShows` and `core-control/tests/names.rs` cover the new text.
- **Track, not lane.** Everything the app shows says "track": labels, menus, the prompt, the sheet, report lines, take names, alerts and README steps. Identifiers, command IDs, JSON keys and the file format keep `lane`. `core-ffi/tests/wording.rs` already scans `core-io`.
- **Never run `auval`**, in any form.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. The owner checks the views by eye from README steps 291–299.
  - Launch VizDAW only through the e2e (Task 11) with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - Tests never open real audio or MIDI and never trigger the microphone prompt or TCC. They never touch `~/Library` (the real `~/Library/Application Support/VizDAW` included), preferences, the Trash or devices.
  - Use temp folders (`tempfile`), `Session::debug_set_trash_dir`, `MemoryDefaults()`, the e2e's throwaway home and fakes only.
  - Every `core-ffi` test file calls `vizdaw_ffi::host::force_offline()` through its `serial` helper. Every Swift suite that touches the real library sets `TestSupportDir` first, as 6d's suites do, and `SupportDirScanTests` passes.
- **Programmatic calls open nothing.**
  - Control-channel commands and programmatic calls never open dialogs, sheets, alerts, menus, panels or windows.
  - The return prompt, the Open panel for a project and `MergeSheet` open only for files the user opened from the UI (File › Open, Import Project ▸ DAWproject…, Finder, the Dock).
  - A channel merge into a `project` path opens that project as a document, as `file.open` does under the channel, and as 6d's channel import makes a document. That is the command's result, not a prompt.
- **Hostile input.**
  - The returning file, its record, its tags and the project's snapshots are all hostile (§6.5).
  - Every new command and parameter gets hostile-input tests at every layer (Tasks 8–10).
  - Every computed tick, frame and length uses `checked_*`/`saturating_*` or `i128`. Every float is finite before it is compared or applied.
- **FFI safety.**
  - No panic crosses the FFI: every new export returns `Result<_, VizError>` inside `ffi_guard`.
  - The merge job runs every parse, mapping, plan and apply inside `std::panic::catch_unwind`. A caught panic fails the job with `internal error: …` and is a bug.
- **One undo step.**
  - Every edit a merge makes is inside one history transaction labelled `merge::LABEL` ("Merge DAWproject"), so Edit shows "Undo Merge DAWproject".
  - The live plug-in capture before it is the existing step-free capture, as Save does.
  - A merge that changes nothing adds no step.
- **Nothing dropped silently.** Every item a merge does not take from theirs gets a report line and a test that names the line's exact `what`: conflicts, kept deletions, VST replacements, tags ignored, and loops unrolled.
- **Swift 6.1.**
  - Generated bindings compile in Swift 5 mode. App modules and tests compile in Swift 6 mode with no new warnings.
  - No `@State` or `@FocusState` anywhere in this plan's code (view state lives in the `@Observable` models).
  - Never capture `[weak self]` or a non-Sendable value inside `MainActor.assumeIsolated`; bind a strong `let` first, as `DocumentWindow.swift` does.
  - Colours, spacing, radii and fonts come from the theme. Every new control is accessibility-labelled.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. Rust stable, edition 2024, GPL-3.0-only. `core-model`, `core-engine`, `core-media`, `core-io` and `core-control` stay free of OS APIs; everything Rust here runs on Linux.
- **TDD.** Every new test is run and seen to fail before the code that makes it pass. Hand-written expected values (report texts, counts, take names) are typed from this plan before the code exists.
- **Real-time.** No real-time code changes. `MixerCmd::SetPluginState` changes the document only; the host's existing `sync` loads state off the audio thread.
- **Format.** `core_model::file::VERSION` stays 9. Snapshot format 2 is a package file, not part of `project.json`; formats 1 and 2 are both read.
- **Lessons from plans 6a–6d (binding):**
  - **The core owns file naming and every sentence.** Swift never builds a file name, a take name, a refusal or a report line.
  - **Never replace anything without explicit confirmation.** The merge writes only into the project's `Media/` (staged in `.incoming/`, `create_new`) and changes the document only through its transaction.
  - **No file I/O or parsing on the main thread.**
    - `dawproject_info` and `dawproject_snapshots` are called from detached tasks.
    - `merge_dawproject` checks only text and the model before it spawns. It has the same two carve-outs as `export_dawproject`: `file_path`'s lstat, and the live plug-in capture. `has_snapshot` is one lstat. Say so in Task 8's report.
  - **Cancel is tested** at every stage of the merge job (Task 8) and in the sheet (Task 10).
  - **One interchange job at a time.**
    - `MergeDawproject` joins `interchange_idle`'s pending kinds and `ForDawModel.blocking`.
    - A held merge (between preview and Merge) counts as running.
  - **Imports insert a crossfaded pair in one command** (backlog, 6c F22). The merge's added and replaced regions follow the rule.
- **UniFFI changes** (Task 8 only). Every new field on an existing record is `#[uniffi(default)]` or `#[uniffi(default = None)]`:
  - `JobKindRow::MergeDawproject`, `ReportKindRow::Conflict` and `MergeStepRow { Preview, Hold, Apply }`;
  - records `DawprojectMergeRequestRow`, `MergePreviewRow` and `SnapshotRow`;
  - `DawprojectInfoRow.project_id`, `JobRow.merge`;
  - `Session::merge_dawproject`, `confirm_merge`, `cancel_merge`, `dawproject_snapshots`, `project_id` and `has_snapshot`.

  `FakeSession` gains each method, with logs and settable answers.

  Every Swift `switch` over `JobKindRow` or `ReportKindRow`, and every `==`/`contains` chain on job kinds, gains the new case:
  - `ProjectMedia.swift` `jobText` ("Merging DAWproject…") and `failureTitle` ("DAWproject Merge Failed");
  - `ControlChannel.swift` (~167);
  - `ControlCommands.swift` (~77, ~87: a finished merge answers `.ok(status: report.summary)`);
  - `Interchange.swift` (`jobFailed`, `ReportText.groups`).
- **Control channel.** The registry stays at 175 commands (6e adds parameters, not commands), and `everyRegisteredMenuCommandHasAHandler` passes.
- **Each task ends green.**
  - Always:
    - `cargo fmt --all` then `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches. Run the whole workspace in Tasks 1, 7, 8 and 11.
  - Tasks 8–11 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 11 also runs:
    - the no-brand build `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <a fresh temp dir>`;
    - `scripts/make-app.sh`;
    - the e2e `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.
  - Known flaky: the `damaged_files` proptest, the MIDI listener tests, `core-ffi` catalog tests under parallel load and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline:** measure at the start of Task 1: Rust passed/ignored, Swift, e2e 27, registry 175, README through 290, `VERSION` 9. Each task's last step gives the counts it adds. Report the measured counts; a slip in the writer's arithmetic is not a failure.
- **How code is given.**
  - New test files are written whole.
  - New implementation files get their public items with doc lines, plus the algorithm where the signature and tests leave a choice.
  - Changes to existing files follow the existing pattern named in the step. If a named anchor no longer matches, make the same change to the current text and say so in the report.

## Review Focus

These are the five failure modes a person is most likely to hit, most likely first. The spec implies them, but no ordinary test would exercise them. Each is pinned by named tests in its task.

1. **The other DAW rewrote the file its own way.**
   - Examples: tracks renamed and reordered, its own audio files under new names and at another rate, comments (and so the `vz:` tags) dropped, no `vizdaw/record.json`, levels rounded to 0.01 dB, automation re-sampled at the same values.
   - Expected:
     - tracks match by name, then order, with no duplicated tracks;
     - regions match by position within a frame;
     - nothing counts as changed because of rounding;
     - the report says the tracks were matched by name and order.
   - Pinned by:
     - `renamed_and_reordered_tracks_match_by_order`, `audio_at_another_rate_matches_within_a_frame` and `rounded_levels_are_unchanged` (Task 4);
     - `levels_within_tolerance_are_unchanged` (Task 5);
     - the stripped variants of every Task 6 case (Task 7).
2. **Merging twice, or merging a file nobody edited.**
   - Examples: Merge clicked on a file straight from Export; the same returned file merged a second time.
   - Expected: the first changes nothing and adds no undo step ("Nothing to merge: the file matches the project."). The second merge decides nothing new: no duplicate notes, markers or takes.
   - Pinned by `nothing_to_merge_adds_no_step` and `merging_the_same_file_twice_changes_nothing_the_second_time` (Task 6), and the e2e (Task 11).
3. **The project changes between the preview and Merge.**
   - Examples: an edit over the control channel, a recording started, an undo while the sheet shows.
   - Expected:
     - Merge is refused with "The project changed after the preview; preview the merge again.", and nothing is applied;
     - recording refuses with `busy`;
     - the held media is removed on Cancel or on close.
   - Pinned by `confirm_after_an_edit_is_refused`, `confirm_while_recording_is_busy` and `closing_drops_a_held_merge` (Task 8), and `aRefusedConfirmShowsTheSentence` (Task 10).
4. **Edits on both sides near each other.**
   - Examples:
     - ours deleted a track that theirs edited;
     - theirs deleted a track that holds input effects and an inactive take;
     - ours and theirs both edited different notes of one track;
     - theirs edited a shared clip on one of its two uses;
     - theirs edited one pass of a looped use.
   - Expected:
     - VizDAW wins every one, and each is listed under Conflicts or Changed;
     - VizDAW-only state is byte-equal afterwards;
     - one undo restores the project exactly.
   - Pinned by `ours_deleted_theirs_changed_stays_deleted` (Task 5), and by `deleted_track_with_vizdaw_only_state_is_kept_empty`, `both_changed_notes_become_an_alternate_take`, `shared_uses_edited_differently_become_private_copies` and `looped_use_edited_on_one_pass_becomes_notes` (Task 6).
5. **A hostile or damaged return, or a damaged snapshot.**
   - Examples:
     - `vz:t999` naming no track, `vz:s3` on a `Track`, one tag on two tracks, 10 000 tags;
     - a record whose `export_id` names no snapshot;
     - a snapshot that is truncated, has format 7, or holds identity lists that do not line up with its projection;
     - a snapshot over 256 MiB.
   - Expected: refused with one sentence, or merged with the bad items treated as new or unmatched and listed. Never a panic, never `Internal`, each case under 2 s and 256 MiB.
   - Pinned by `hostile_tags_never_match_twice` (Task 4), `read_snapshot_refuses_damage` and `snapshot_limits` (Task 1), and `merge_fuzz.rs` and `hostile_records` (Task 7).

## Plan Rulings

These are this plan's decisions where the spec leaves a choice. Each names its cost if wrong.

1. **The three sides are projections.**
   - Base is the snapshot's `Projection`. Theirs is `projection` of the file imported into a scratch project. Ours is `projection` of the live document. All three are for the snapshot's `seq`.
   - Before any comparison, base goes through `record::as_returned` and theirs through `record::returned`:
     - `as_returned` gives what an unchanged file brings back: ticks shifted by `-origin`, the maps from tick 0 with the last ramp flat, region gain 0, and no media hash for a region with gain under `Apply` (its audio comes back as a new copy, Ruling 11);
     - `returned` drops the import's hold points and sorts each note's expression.
   - Task 1 moves this logic out of the round-trip test's `same`. That test then asserts `as_returned(projection(doc)) ≈ returned(projection(import(export(doc))))`, so 6d's whole proptest proves the normalisers.
   - *Cost if wrong:* one normaliser; every decision rests on it.
2. **The record contributes only its head** (`RecordHead`, as 6d reads it).
   - The snapshot is the base and holds the ID map. `record.document` is never read: ours is the authority for VizDAW-only state (§7.8 "always keeps from ours").
   - XML IDs in `project.xml` (`t<lane>`, `c<strip>`, …) are trusted as identities only when the record's head names the snapshot's export. Without it, an ID like `t5` may be another app's own.
   - *Cost if wrong:* a reader of `record.document` for one case nobody has found.
3. **Snapshot format 2** (`record::SNAPSHOT_FORMAT = 2`; the record keeps `FORMAT = 1`).
   - Format 2 adds `region_gain` (`"apply"`/`"leave_out"`) and `identity`, an `Identity` parallel to the projection. It holds each exported track's lane and the event and top-level event behind each note, region, MIDI event and automation point. It also holds the reference uses with their spans, and each channel's strip and device slots.
   - **Format 1** (6d's, still read, as spec §7.4 promises):
     - ours is matched to base by name and type, then order, like a stripped return;
     - the region gain is assumed `Apply`;
     - shared sequences are not refined per use (a use counts as its notes);
     - a Note says so (`text::OLD_EXPORT`).
   - *Cost if wrong:* format 1 merges are less exact; only exports made between 6d and 6e have one.
4. **Matching (§7.8).**
   - **Tracks:**
     - by trusted XML ID, else by `vz:t` tag (`vz:t<lane>` plus the part's `audio` flag for a split track);
     - else by name and content type among the unmatched, else by order among the unmatched of the same type.
     - Base↔ours uses the lane ID (format 2).
   - **Strips:** with their track, else buses by ID, tag or name.
   - **Markers:** by `vz:m` index, else name and tick (±1 tick).
   - **Regions:** by `vz:e` tag, else by media (content hash or file stem) plus tick (±1 tick) and length (±1 frame).
   - **Notes:** within a matched track part, by pitch, channel, start and duration, each within `NOTE_TICKS` (1 tick at 15 360 PPQ).
   - A tag naming nothing, a tag of the wrong kind, or a second element with a tag already used is ignored and counted (`import::TAGS_IGNORED`, Note).
   - **Tolerances,** constants in `merge`:
     - `NOTE_TICKS = 1`;
     - `FRAMES = 1`;
     - `LEVEL_DB = 0.05` (levels and send levels; −∞ equals anything below −120 dB);
     - `PAN = 0.005`;
     - `VALUE = 1e-4` (normalised automation values; points within `NOTE_TICKS`).
   - *Cost if wrong:* constants.
5. **Units: what is compared and replaced as one thing.**

   | Unit | Compared | Theirs applied as |
   |---|---|---|
   | A track part's notes (with expression and MIDI events) | as a set | note-level edits (Ruling 8 for uses) |
   | A track part's regions (with fades, crossfades, names, mutes) | as a set | the changed regions deleted and re-inserted with ours' media and gain |
   | A track's name | value | `LaneCmd::Rename` |
   | A strip's level, pan, mute, solo, colour, name, output | each field | `MixerCmd::Set*`, `RenameStrip`, `SetColour`, `SetOutput` |
   | A strip's sends | each send by target | `AddSend`/`EditSend`/`RemoveSend` |
   | A strip's device list (formats, IDs, order) | as a list | `RemoveInsert`/`AddInsert`/`SetEq` with `SetPluginState` |
   | One device's bypass; one device's state (blob ID) | each | `SetBypass`; `SetPluginState` |
   | One automation curve (target) | whole curve | the curve's events deleted and theirs inserted |
   | One marker | name and tick | the merged list in one `SetMarkers` |
   | Tempo map; meter map | whole map | `SetTempo`; `SetMeter` |
   | One song-info field | value | the merged `SongInfo` in one `SetInfo` |
   | A track or bus | exists | added (Ruling 9) or removed (Ruling 7) |

   The rule table of §7.8 is applied per unit. "Changed the same way" means ours and theirs are within tolerance of each other.

   **Ours wins** (owner, §15):
   - a note or region set changed on both sides becomes the conflict take (Ruling 6);
   - any other unit changed on both sides keeps ours, and theirs' value is listed.

   *Cost if wrong:* a finer unit (single notes) is a later refinement inside `decide`.
6. **The conflict take.**
   - `LaneCmd::AddTake` named `"Returned <date>"`, then theirs' version of each conflicting part, flattened as it plays: notes, MIDI events, and regions with their crossfade pairs in one `EventCmd::Insert`. Then `LaneCmd::SetActiveTake` back to ours' take. Ours stays the active take.
   - `<date>` is the request's `date` (`YYYY-MM-DD`, the app's local date; Task 8 checks the form). A second take of that name on the track gets " (2)", " (3)", ….
   - The take holds only the parts in conflict. If ours' notes and theirs' notes conflict on a split track, the take holds theirs' notes only; a Note says so (`text::TAKE_PARTS`).
   - *Cost if wrong:* what the take holds.
7. **Deletions.**
   - A base track theirs deleted, ours unchanged, is removed (`LaneCmd::Remove`) unless it holds something the file cannot carry. That means:
     - more than one take;
     - a drum layout, an input, a monitor mode other than the default, input effects or an MPE config;
     - play quantize or shift;
     - its own length or loop;
     - a reference use of a sequence used outside the exported sequence.
   - Such a track is emptied of its carried content and kept, under Conflicts (`text::KEPT_EMPTY`).
   - Theirs deleted and ours changed: kept, under Conflicts (`text::DELETED_THERE`). Ours deleted and theirs changed: stays deleted, under Conflicts (`text::DELETED_HERE`). Buses follow the same table with `RemoveBus`.
   - *Cost if wrong:* the spec's table says "Removed"; the exception protects §7.8's keep list (owner question 1).
8. **Shared sequences.**
   - Theirs' notes are attributed to a use when they lie in its span: the span of theirs' clip tagged `vz:e<use>` when there is one, else the base use's span. Ticks are made relative to the use's start, with its transpose taken off.
   - **If every exported use of the sequence has the same relative edit,** the edit goes into the sequence, once. Uses of it outside the exported sequence (other sequences, keyboard slots) play the edit too, with a Note naming the sequence (`text::SHARED_ELSEWHERE`).
   - **Otherwise,** each use whose edit differs from the sequence's gets `ReferenceCmd::MakePrivateCopy`, then its edit (`text::PRIVATE_COPIES`, Changed).
   - **A looped use whose passes were edited differently** is replaced by theirs' notes on the track (`text::LOOPS_UNROLLED`, Changed).
   - *Cost if wrong:* where the edit lands outside the export (owner question 2).
9. **New things from theirs** are added as 6d's import adds them. That covers a new track with its destination or plug-in instrument, its output path, strip settings and devices; new buses; new markers; and new regions, whose media are converted into ours' `Media/`.
   - New media reuse an existing item of ours with the same content hash, else `MediaCmd::Add`.
   - Task 6 factors 6d's `import::apply` into `apply_tracks`, which both use.
   - *Cost if wrong:* none to 6d's import (its tests run unchanged).
10. **Plug-ins.**
    - Before the job spawns, the live state of every slot is captured (step-free, as `export_dawproject` does), so ours' blob IDs are current and undo returns to the live settings.
    - **Theirs' state for a matched device** is applied with the new undoable `MixerCmd::SetPluginState`. Its blob ID is `state::blob_id(bytes)`, comparable to the snapshot's.
    - **A device theirs replaced with a VST plug-in,** or one the file names but this Mac has not installed, stays ours, listed (`text::VST_KEPT`, Left out).
    - *Cost if wrong:* an AU whose state round-trips to different bytes shows as changed. That is harmless when ours is unchanged, and a listed conflict when it is not.
11. **Region gain and media of matched regions stay ours.**
    - The file has no clip gain: 6d writes it into the audio or leaves it out, and its import sets 0. So theirs' gain is never a change.
    - A matched region keeps ours' media item. Theirs' converted copy is used only for regions theirs added, and unused copies are removed after the merge.
    - *Cost if wrong:* a region whose audio the other DAW really replaced keeps ours (§7.8 lists "audio clips", not their content).
12. **The job and its steps.**
    - `MergeStepRow::Preview` computes the plan and report, keeps nothing and changes nothing (`preview: true`).
    - `Hold` keeps the merged history and its media until `confirm_merge(job)` or `cancel_merge(job)` (the sheet).
    - `Apply` swaps at once (the channel without `preview`).
    - The plan and the merge run on the job thread against a clone of the history taken at spawn. The swap happens only if `History::present_serial()` is unchanged, else "The project changed after the preview; preview the merge again." So the swap is O(1) on the main thread. This also absorbs backlog row "the import applies on the main thread" (M8) for merges.
    - *Cost if wrong:* a refused confirm after an edit; the user previews again.
13. **Which project, and which snapshot.**
    - **The open match:** a document whose `project_id` equals the record's. Else, when the file has an export ID (record or metadata line), one that `has_snapshot(export_id)`. When several match, the most recently active.
    - **Without an open match:** the prompt's "Merge With Existing Project…" shows an Open panel (UI), or the channel needs `project`.
    - **Refusals:**
      - a project whose ID differs from the record's, or that has no snapshot of the file's export, is refused: "This file was not exported from that project.";
      - a file with no export ID needs a snapshot chosen. The UI lists the project's snapshots (`SnapshotRow`: date, track count, newest first). The channel's refusal names them: "This file lost its export ID; give `snapshot`, one of: <id> (<yyyy-mm-dd hh:mm UTC>, <n> tracks), …" (at most 10);
      - `snapshot` is refused when the file names a different export.
    - *Cost if wrong:* refusal wording.
14. **The prompt and its second page (§9).**
    - The prompt is a store-level `NSAlert`:
      - message: "This DAWproject came from a VizDAW project.";
      - informative text: with an open match, "It matches “<project>”, which is open.", else "Merge it into the project it came from, or open it as a new project.";
      - buttons: "New Project", "Merge With Existing Project…", "Cancel".
    - The second page is `MergeSheet` on the target project's window, so the user sees the project it changes:
      - the snapshot list when needed;
      - the counts line;
      - the conflict list;
      - Merge and Cancel.
    - *Cost if wrong:* UI only.
15. **The report.**
    - `ItemKind::Conflict` and `ReportKindRow::Conflict` are new. The sheet's groups are "Conflicts", "Changed", "Left out", "Notes".
    - The summary is `text::summary(&MergePreview)`: "Merged: <a> applied, <k> kept, <n> added, <r> removed, <c> in conflict". With nothing to do, it is "Nothing to merge: the file matches the project."
    - The import's own lines (Changed/Left out from the mapping) are merged into the report as 6d's.
    - *Cost if wrong:* wording.
16. **When the prompt appears** stays 6d's `ReturnInfo::from_vizdaw`: a record, an export line, or `vz:t` tags on at least half the tracks. The spec says "most"; at exactly half, 6d prompts.
    - Over the channel, a file from VizDAW without `mode` now answers `dialog_required` "Return" (spec §10). 6d's control tests and e2e, which import their own export with no `mode`, gain `mode: "new"` (Tasks 9, 11).
    - *Cost if wrong:* one comparison.
17. **Limits** (constants in `limits.rs`):
    - `MAX_SNAPSHOT_BYTES` = 256 MiB, read through the same bounded `read` as the record;
    - a snapshot's or record's `ids` at most `MAX_XML_NODES` entries;
    - `identity` lists equal in length to the projection's, else the snapshot is damaged;
    - at most `KEEP_SNAPSHOTS` snapshots listed.

    "The project's copy of that export is damaged, so the file can't be merged." refuses a damaged snapshot.
    *Cost if wrong:* constants.

---

### Task 1: `core-io`: the Conflict kind, snapshot format 2, the bounded snapshot reader and the normalisers

**Files:**
- Modify:
  - `crates/core-io/src/report.rs` (`ItemKind::Conflict`, its count and order: Conflict first);
  - `crates/core-io/src/limits.rs` (`MAX_SNAPSHOT_BYTES`);
  - `crates/core-io/src/dawproject/record.rs`;
  - `crates/core-io/src/dawproject/export.rs` (the snapshot it builds, ~line 186);
  - `crates/core-io/tests/dawproject_round_trip.rs` (`same` uses the normalisers);
  - every `match` over `ItemKind` in the workspace (`core-ffi/src/interchange.rs`'s `report_row` maps it to a placeholder until Task 8; say so).
- Create: `crates/core-io/tests/common/merge.rs` (`fixture() -> (Document, SeqId)`; Task 4 adds the harness), and add it to `tests/common/mod.rs`
- Test: `crates/core-io/tests/dawproject_merge_base.rs`

**Interfaces:**
- Consumes: 6d's `record` and `export`.
- Produces:
  ```rust
  // record.rs
  pub const SNAPSHOT_FORMAT: u32 = 2;                    // written; 1 and 2 read
  pub struct Snapshot { /* 6d's fields */
      #[serde(default, skip_serializing_if = "Option::is_none")] pub region_gain: Option<String>, // "apply" | "leave_out"
      #[serde(default, skip_serializing_if = "Option::is_none")] pub identity: Option<Identity> }
  /// Who each projection row is in the project it was taken from, parallel to the projection.
  pub struct Identity { pub tracks: Vec<TrackIds>, pub strips: Vec<StripIds> }
  pub struct TrackIds { pub lane: u64, pub notes: Vec<EventAt>, pub midi: Vec<EventAt>, pub regions: Vec<EventAt>,
      pub automation: Vec<Vec<EventAt>>, pub uses: Vec<UseAt> }
  /// An event, and the top-level event it plays through (itself, or its reference use).
  pub struct EventAt { pub event: u64, pub top: u64 }
  pub struct UseAt { pub event: u64, pub seq: u64, pub tick: i64, pub end: i64, pub transpose: i8, pub passes: u32 }
  pub struct StripIds { pub strip: Option<u64>, pub devices: Vec<u64> }   // devices: slot IDs, in PStrip.devices order
  pub fn projection_with_identity(doc: &Document, seq: SeqId) -> Result<(Projection, Identity), IoError>;
  /// Reads a snapshot: size, JSON, format 1|2, UUID IDs, ids count, identity lengths (Ruling 17).
  pub fn read_snapshot(bytes: &[u8]) -> Result<Snapshot, IoError>;
  /// Base as an unchanged file brings it back (Ruling 1).
  pub fn as_returned(base: &Projection, region_gain: RegionGain) -> Projection;
  /// A returning file's projection without what the import adds (hold points; expression order).
  pub fn returned(theirs: &Projection) -> Projection;
  pub const SNAPSHOT_DAMAGED: &str = "The project's copy of that export is damaged, so the file can't be merged.";
  ```
  - `as_returned` sets `media_hash = None` on a region with gain under `Apply`, and `returned` leaves theirs' hashes as they are. Matching then pairs such a region by tag or position (Task 4), and its media is never compared (Ruling 11). The round-trip test keeps its own `copy_hash` assertion on the side for those regions.
  - `export` writes `SNAPSHOT_FORMAT`, `region_gain` and `identity` (from the same layout as the projection).

- [ ] **Step 1: Write the failing tests** in `dawproject_merge_base.rs`:
  - `snapshot_round_trips_with_identity`: a `core_engine::fixtures::Builder` document with two tracks, one shared sequence used twice, a crossfaded pair, a send and a plug-in insert (Tasks 2 and 4–6 reuse it as `common::merge::fixture()`, added here) → export → `read_snapshot(snapshot_json)`:
    - `format == 2` and `region_gain == Some("apply")`;
    - `identity.tracks[i].lane` is each part's lane;
    - each `notes[j].top` is the use event for notes inside a use;
    - `uses` names both uses with their `seq`, spans and `passes`;
    - `strips[k].devices` is the insert's slot ID.
  - `format_one_still_reads`: 6d's snapshot JSON with `format: 1` and no new fields → `identity == None`, `region_gain == None`.
  - `read_snapshot_refuses_damage`, each → `Err` whose message is `SNAPSHOT_DAMAGED`:
    - truncated JSON;
    - `format: 7`;
    - `project_id: "x"`;
    - `identity.tracks` one shorter than `projection.tracks`;
    - a `TrackIds.notes` one longer than its track's `notes_played`.
  - `snapshot_limits`: at `MAX_SNAPSHOT_BYTES` (built with padding in `ids` under `common::measure`) reads; one byte over is refused before parsing (allocation under 1 MiB). `ids` at `MAX_XML_NODES` reads; one more is refused.
  - `as_returned_shifts_the_pickup_and_flattens_the_last_ramp`: a hand `Projection` with `origin = -3840`, a tempo ramp on its last point and a region with `gain_db = -6` → hand-typed expected values for `Apply` and `LeaveOut`.
  - `returned_drops_hold_points`: automation `[(0, 0.2), (99, 0.2), (100, 0.8)]` from a `hold` import → `[(0, 0.2), (100, 0.8)]`.
  - `ItemKind::Conflict` sorts first in `Report::items` grouping: a report with one of each kind lists Conflict, Changed, Left out, Note.
  - In `dawproject_round_trip.rs`, `same(original, imported, gain)` becomes `assert_projection_eq(&as_returned(original, gain), &returned(imported))`, the existing comparisons on two normalised projections. Every existing round-trip test and the proptest still pass.
- [ ] **Step 2: Run** `cargo test -p core-io --test dawproject_merge_base`. Expected: FAIL (`SNAPSHOT_FORMAT`, `read_snapshot`, `as_returned` not defined).
- [ ] **Step 3: Implement** the items above. `projection_with_identity` walks the layout the way `project` does, pushing an ID at each push of a projection row, so the two stay parallel. `project` is unchanged.
- [ ] **Step 4: Run** `cargo test --workspace` and both clippies. Expected: PASS (about +7 Rust).
- [ ] **Step 5: Commit** `feat(io): DAWproject snapshot format 2 with identities, its bounded reader and the return normalisers (plan 6e Task 1)`.

### Task 2: `core-io`: the import keeps what its tags and trusted IDs name

**Files:**
- Modify:
  - `crates/core-io/src/dawproject/import.rs` (`ImportEnv.ids`, `Origins`, the tags on `TrackPlan`/`BusPlan`, `TAGS_IGNORED`);
  - `import/timeline/mod.rs` (`Item.tag`, markers' tags);
  - `import/apply.rs` (`apply_tagged`; `apply` calls it and drops the origins);
  - every `ImportEnv { .. }` literal (add `ids: None`).
- Test: `crates/core-io/tests/dawproject_import.rs` (new cases)

**Interfaces:**
- Consumes: Task 1; 6d's `strip_tags`, `TagKind`.
- Produces:
  ```rust
  pub struct ImportEnv<'a> { /* 6d's */ pub ids: Option<&'a BTreeMap<String, ObjectRef>> } // trusted XML IDs (Ruling 2)
  /// What each new object was in the project the file came from.
  #[derive(Debug, Clone, Default, PartialEq)]
  pub struct Origins {
      pub lanes: BTreeMap<LaneId, (u64, bool)>,      // new lane → (base lane, audio part)
      pub strips: BTreeMap<StripId, u64>,
      pub events: BTreeMap<EventId, u64>,           // new top-level event → its clip's vz:e (base event)
      pub markers: Vec<Option<u64>>,                // per new marker, in order: its vz:m index
      pub slots: BTreeMap<PluginInstanceId, u64>,   // new slot → vz:d / vz:q instance
  }
  pub fn apply_tagged(h: &mut History, imp: &DpImport, seq: SeqId, report: &mut Report)
      -> Result<(SlotStates, Origins), ModelError>;
  /// Note: tags or trusted IDs that named nothing, the wrong kind, or a part already named (a count).
  pub const TAGS_IGNORED: &str = "Tags in the file that named nothing or named one part twice (ignored)";
  ```
  - **Rules:**
    - A trusted XML ID wins over a tag. A `Track`'s ID resolves only to `kind: "track"`, a `Channel`'s to `strip`/`channel`, a device's to `device`; any other kind is ignored and counted.
    - Within each map, the first element in file order to name an object keeps it, and later ones are counted.
    - A clip tag goes to every top-level event its clip became: the reference use, or each note and region placed straight on the track.
    - Tags are still stripped from every comment, as 6d does.

- [ ] **Step 1: Write the failing tests:**
  - `origins_follow_the_tags`: our export of Task 1's fixture → `map` → `apply_tagged` into a new project. `origins.lanes` maps each new lane to its source lane (the split part's flag true), `strips` each strip, `events` the use and each region, `markers` `[Some(0), Some(1)]`, and `slots` the insert.
  - `trusted_ids_win_over_tags`: the same file with its `Track` comments swapped between two tracks, with the record's ids passed in `env.ids` → origins follow the IDs. Without `env.ids` → they follow the (swapped) tags.
  - `hostile_tags_are_ignored_and_counted`: a hand file with `vz:t999` (no such lane is fine here: it is kept as named, and matching decides), `vz:s3` on a `Track`, the same `vz:t1` on two tracks, an ID `t1` mapped to `kind: "strip"` in `env.ids`, and 10 000 tagged empty clips:
    - exactly one lane maps to 1;
    - the wrong kinds are absent;
    - the report has `TAGS_IGNORED` with the count 3 plus the clips past the first per object;
    - the case runs under 2 s.
  - `apply_still_matches_6d`: `apply` and `apply_tagged` give equal documents for every fixture in `dawproject_import.rs`.
- [ ] **Step 2: Run** `cargo test -p core-io --test dawproject_import`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above. The mapping records each element's tags beside its plan entry (`TrackPlan.tag`, `BusPlan.tag`, `Item.tag`, the marker list, `SlotPlan.tag`); `apply_tagged` fills `Origins` from the IDs each command reports.
- [ ] **Step 4: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +4 Rust).
- [ ] **Step 5: Commit** `feat(io): DAWproject import keeps the origins its tags and trusted IDs name (plan 6e Task 2)`.

### Task 3: `core-model`: `MixerCmd::SetPluginState`, and the host reloads it on undo

**Files:**
- Modify:
  - `crates/core-model/src/command/mixer.rs` (the variant, its label "Set Plug-in Settings", its apply);
  - `crates/core-model/src/state.rs` (share `capture_keeping`'s blob store);
  - `crates/core-model/tests/mixer_commands.rs` (`arb_cmd` gains it).
- Test: `crates/core-model/tests/plugin_state.rs` (new cases), `crates/core-host/tests/host.rs` (one case)

**Interfaces:**
- Produces:
  ```rust
  /// Set a plug-in instance's saved settings, with undo (plan 6e Ruling 10): the blob
  /// `state::blob_id(bytes)` is stored, the slot points at it, and its parameter overlay is
  /// cleared. Refused for an unknown slot or empty bytes; placeholder slots are allowed.
  SetPluginState { slot: PluginInstanceId, bytes: Arc<[u8]> },
  ```

- [ ] **Step 1: Write the failing tests:**
  - `plugin_state.rs`:
    - `set_plugin_state_is_one_undoable_step`: a strip with an insert whose state is blob A and whose overlay is `{3: 0.5}`. `SetPluginState(B)` → the slot's state is `blob_id(B)`, the overlay is empty, `doc.state_blob` returns B. Undo → blob A and `{3: 0.5}`. Redo → B.
    - `set_plugin_state_refuses`: an unknown slot and empty bytes give `ModelError`, and the document is unchanged.
    - It works on an instrument destination's slot and on an EQ slot.
  - `mixer_commands.rs`: `arb_cmd` includes `SetPluginState` with 0–64 random bytes; the existing undo property passes.
  - `host.rs`: `sync_loads_a_changed_state_and_the_undone_one`, with the existing fake format: after `sync(doc_with_B)` the fake instance's `load_state` saw B; after `sync(doc_with_A)` it saw A.
- [ ] **Step 2: Run** `cargo test -p core-model --test plugin_state` and `cargo test -p core-host --test host sync_loads`. Expected: FAIL.
- [ ] **Step 3: Implement** the variant; its apply reuses `state::capture_keeping`'s store with an empty keep map, as a command.
- [ ] **Step 4: Run** `cargo test -p core-model -p core-host` and both clippies. Expected: PASS (about +4 Rust).
- [ ] **Step 5: Commit** `feat(model): an undoable SetPluginState that the host reloads (plan 6e Task 3)`.

### Task 4: `core-io::dawproject::merge`: theirs, and matching the three sides

**Files:**
- Create:
  - `crates/core-io/src/dawproject/merge/mod.rs` (module doc: the three sides, the tolerances);
  - `merge/theirs.rs`;
  - `merge/matching.rs`;
  - `crates/core-io/tests/dawproject_merge_match.rs`.
- Modify: `crates/core-io/src/dawproject/mod.rs` (`pub mod merge;`), `crates/core-io/tests/common/merge.rs` (the harness below, used by Tasks 4–7).

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces:
  ```rust
  // merge/mod.rs
  pub const NOTE_TICKS: i64 = 1;  pub const FRAMES: u64 = 1;  pub const LEVEL_DB: f64 = 0.05;
  pub const PAN: f64 = 0.005;     pub const VALUE: f64 = 1e-4;
  // merge/theirs.rs
  /// The returning file as a project of its own.
  pub struct Theirs { pub doc: Document, pub seq: SeqId, pub projection: Projection, pub identity: Identity,
                      pub origins: Origins, pub states: SlotStates, pub report: Report }
  /// Applies `imp` (with `apply_tagged`) to a new empty project at `format`, then projects it
  /// (`projection_with_identity`, then `returned`).
  pub fn theirs(imp: &DpImport, format: AudioFormat, cancel: &AtomicBool) -> Result<Theirs, IoError>;
  // merge/matching.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum MatchedBy { Id, Tag, Name, Order, Lane }
  /// Indices into the three projections' rows (`None`: not on that side).
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Matched { pub base: Option<usize>, pub theirs: Option<usize>, pub ours: Option<usize>, pub by: MatchedBy }
  pub struct Matches { pub tracks: Vec<Matched>, pub strips: Vec<Matched>, pub markers: Vec<Matched>,
                       /// Some track was matched by name or order (`text::BY_NAME`).
                       pub heuristic: bool }
  pub fn match_all(base: &Snapshot, theirs: &Theirs, ours: &(Projection, Identity), cancel: &AtomicBool)
      -> Result<Matches, IoError>;
  /// Within matched rows: pairs of (base or ours, theirs) indices.
  pub fn notes(a: &PTrack, b: &PTrack) -> Vec<(Option<usize>, Option<usize>)>;
  pub fn regions(a: (&PTrack, &TrackIds), b: (&PTrack, &TrackIds, &Origins)) -> Vec<(Option<usize>, Option<usize>)>;
  ```
  - Base↔ours is `Lane` by `identity` (format 2), else Ruling 4's name and order rules. Theirs↔base follows Ruling 4. Each row appears in exactly one `Matched`.
  - `notes` pairs greedily in (tick, pitch) order: each base note takes the first unpaired theirs note within the tolerances. It is O(n log n) through a sort, never O(n²) on a 50 000-note part.
- **Test harness** (`tests/common/merge.rs`, written whole):
  ```rust
  pub struct Trip { pub base_doc: Document, pub seq: SeqId, pub export: DpExport, pub snapshot: Snapshot, pub dir: TempDir }
  pub fn trip(doc: &Document, seq: SeqId) -> Trip;                       // export with TestPlist and fixed IDs
  /// The returned file: `export`'s project.xml and metadata.xml read with `read_project`/`read_metadata`,
  /// edited as the other DAW would, written back, packaged; `strip` removes the record, the tags, or both.
  pub fn returned(t: &Trip, strip: Strip, edit: impl FnOnce(&mut DpProject, &mut DpMetaData)) -> PathBuf;
  #[derive(Clone, Copy)] pub enum Strip { Nothing, Record, Tags, Both }
  /// open → map (with the snapshot's ids when the record matches) → `theirs`.
  pub fn theirs_of(t: &Trip, path: &Path) -> Theirs;
  ```

- [ ] **Step 1: Write the failing tests** in `dawproject_merge_match.rs`:
  - `unchanged_file_matches_everything`: `returned(.., Strip::Nothing, |_, _| {})` → every track, strip and marker matched on all three sides `by: Id`. Every `notes` and `regions` pair is complete. `heuristic == false`.
  - `tracks_match_by_tag_then_name_then_order`: `Strip::Record` → `by: Tag`. `Strip::Both` → `by: Name`. `Strip::Both` with every track renamed → `by: Order`.
  - `renamed_and_reordered_tracks_match_by_order` (Review Focus 1): `Strip::Both`, two audio tracks swapped and renamed "Audio 1"/"Audio 2" → each matched to its base by order of type, with no unmatched rows. `heuristic` is true.
  - `ids_are_ignored_without_the_record`: a hand file (application "Fixture") whose `Track`s have IDs `t1`, `t2` naming other lanes → matched by name, not ID.
  - `split_tracks_match_their_parts`: a track with notes and audio comes back as two parts → each part matched to its own base row.
  - `strips_follow_their_tracks_and_buses_match_by_tag_then_name`.
  - `markers_match_by_tag_then_name_and_tick`: with tags stripped, a marker moved by 1 tick still matches, one moved by 2 is new plus deleted.
  - `notes_match_within_a_tick`: a note moved by 1 tick pairs; by 2 ticks it is unpaired on both sides. A 50 000-note part pairs in under 1 s.
  - `regions_match_by_tag_then_media_and_position`, and `audio_at_another_rate_matches_within_a_frame` (Review Focus 1): the returned file's audio rewritten at 44 100 Hz for a 48 000 Hz project, stripped of tags → the regions pair, with lengths within `FRAMES`.
  - `rounded_levels_are_unchanged`: volumes rewritten through dB rounded to 0.01 → strips matched, and `(a.gain_db - b.gain_db).abs() <= LEVEL_DB`.
  - `format_one_snapshots_match_ours_by_name_and_order`: the snapshot rewritten with `format: 1` and no identity → ours matched by `Name`.
  - `hostile_tags_never_match_twice` (Review Focus 5): Task 2's hostile file → each base row in at most one `Matched`, never a panic.
- [ ] **Step 2: Run** `cargo test -p core-io --test dawproject_merge_match`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +12 Rust).
- [ ] **Step 5: Commit** `feat(io): DAWproject merge: theirs as a project, and matching the three sides (plan 6e Task 4)`.

### Task 5: `merge::decide`: the rule table, the plan, its preview and its report lines

**Files:**
- Create: `crates/core-io/src/dawproject/merge/decide.rs`, `merge/text.rs`, `crates/core-io/tests/dawproject_merge_decide.rs`

**Interfaces:**
- Consumes: Task 4.
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub enum Unit { Track, TrackName, Notes, Regions, Strip(StripField), Send(usize /*base target strip row*/),
                  Devices, Bypass(usize), State(usize), Automation(String /*target, by matched strip*/),
                  Marker, Tempo, Meter, Info(usize /*field*/), Bus }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum StripField { Level, Pan, Mute, Solo, Colour, Name, Output }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Outcome { Theirs, Ours, Same, Conflict, Removed, KeptEmpty, DeletedThere, DeletedHere, Added }
  #[derive(Debug, Clone, PartialEq)]
  pub struct Decision { pub unit: Unit, pub row: usize /*index in Matches' list for the unit*/, pub outcome: Outcome,
                        /// "Track 'Bass': notes", "Strip 'Bass': level", "Tempo map", "Song info: title".
                        pub what: String, pub ours: Option<String>, pub theirs: Option<String> }
  #[derive(Debug, Clone, Default, PartialEq, Eq)]
  pub struct MergePreview { pub applied: u32, pub kept: u32, pub added: u32, pub removed: u32, pub conflicts: u32,
                            pub conflict_lines: Vec<String> }
  pub struct MergePlan { pub base: Snapshot, pub theirs: Theirs, pub matches: Matches,
                         pub decisions: Vec<Decision>, pub preview: MergePreview, pub report: Report }
  /// Decides every unit (Rulings 5, 7, 11). Units unchanged on both sides make no decision.
  pub fn plan(base: Snapshot, theirs: Theirs, ours: &Document, cancel: &AtomicBool) -> Result<MergePlan, IoError>;
  ```
  - **Counting:**
    - `applied` counts `Theirs` and `Same`; `kept` counts `Ours`; `added` counts `Added`; `removed` counts `Removed`;
    - `conflicts` counts `Conflict`, `KeptEmpty`, `DeletedThere` and `DeletedHere`;
    - `conflict_lines` are the Conflicts group's detail lines, in decision order.
  - **`text.rs`** holds the exact report text, written from this list before the code:
    - **Conflicts:**
      - `CONFLICT_TAKE` "Tracks changed here and in the other DAW (the other DAW's version is an alternate take)", detail "<track>: take “Returned <date>”";
      - `CONFLICT_SETTING` "Settings changed here and in the other DAW (kept as they are here)", detail "<what>: here <ours>, there <theirs>";
      - `KEPT_EMPTY` "Tracks the other DAW deleted that hold what the file cannot carry (kept, emptied)";
      - `DELETED_THERE` "Parts the other DAW deleted that were changed here (kept)";
      - `DELETED_HERE` "Parts deleted here that the other DAW changed (left deleted)".
    - **Changed:**
      - `PRIVATE_COPIES` "Uses of a shared sequence edited differently became private copies", detail the sequence;
      - `LOOPS_UNROLLED` "Looped uses edited on some passes became notes".
    - **Left out:** `VST_KEPT` "Plug-ins the other DAW replaced with ones VizDAW cannot host (kept as they are here)", detail the plug-in name.
    - **Notes:**
      - `MERGED` "This file was merged into the project it came from (the export of <yyyy-mm-dd>)";
      - `BY_NAME` "The file lost VizDAW's track tags; tracks were matched by name and order";
      - `SNAPSHOT_CHOSEN` "The file lost its export ID; it was compared with the export you chose";
      - `OLD_EXPORT` "This export was made before merging existed; tracks were matched by name and order";
      - `SHARED_ELSEWHERE` "Edits to shared sequences also play where they are used outside the merged sequence", detail the sequence;
      - `TAKE_PARTS` "An alternate take holds only the other DAW's changed part of a track".
    - The summaries: `NOTHING` "Nothing to merge: the file matches the project.", and `pub fn summary(p: &MergePreview) -> String` → "Merged: <a> applied, <k> kept, <n> added, <r> removed, <c> in conflict".
    - Values in `ours`/`theirs` text are formatted as the app shows them: "−6.0 dB", "C", "on"/"off", "120 BPM at bar 1; 140 BPM at bar 17", or for a curve "12 points".

- [ ] **Step 1: Write the failing tests** in `dawproject_merge_decide.rs`. Each builds a `Trip`, edits theirs through `returned(..)` and ours through `History` commands on a copy of `base_doc`, then calls `plan`. One test per rule-table row, each for a strip level, a track's notes and a marker:
  - `theirs_changed_ours_unchanged_is_theirs`;
  - `ours_changed_theirs_unchanged_is_ours`;
  - `both_changed_the_same_way_is_applied_once` (`Same`, counted in `applied`);
  - `both_changed_differently_is_a_conflict`: the notes give `Conflict` with `what` "Track 'Bass': notes"; the level gives `Conflict` with ours "−6.0 dB" and theirs "−3.0 dB";
  - `deleted_there_unchanged_here_is_removed`;
  - `deleted_there_changed_here_is_kept` (`DeletedThere`);
  - `ours_deleted_theirs_changed_stays_deleted` (Review Focus 4, `DeletedHere`);
  - `new_there_is_added`.

  Further tests:
  - `nothing_changed_decides_nothing`: `decisions` is empty and `report.summary == NOTHING`.
  - `levels_within_tolerance_are_unchanged`: theirs' level is +0.04 dB → no decision; +0.06 dB → `Theirs`.
  - `region_gain_is_never_theirs` (Ruling 11): base gain −6 dB under `Apply` and `LeaveOut` → no region decision.
  - `vst_replacement_is_kept_and_listed`: theirs swaps the insert for a `Vst3Plugin` → `Ours` on `Devices`, and `VST_KEPT` with the plug-in's name.
  - `song_info_merges_field_by_field`: theirs changes the title, ours the artist → two decisions, both applied or kept, no conflict.
  - `tempo_and_meter_are_whole_maps`: both change different tempo points → one `Conflict` on `Tempo`.
  - `preview_counts`: a mixed case with every outcome → the hand-typed `MergePreview` (applied 3, kept 2, added 1, removed 1, conflicts 4) and `conflict_lines` in order. `summary` gives "Merged: 3 applied, 2 kept, 1 added, 1 removed, 4 in conflict".
  - `report_lines_are_listed`: every `text` constant above appears in some test's report, with its kind (Conflicts, Changed, Left out, Note).
- [ ] **Step 2: Run** `cargo test -p core-io --test dawproject_merge_decide`. Expected: FAIL.
- [ ] **Step 3: Implement** `plan`: normalise (Ruling 1), match (Task 4), then for each unit compare base→theirs and base→ours within the tolerances and pick the table's outcome. The deletion keep rule (Ruling 7) reads the lane in `ours`.
- [ ] **Step 4: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +16 Rust).
- [ ] **Step 5: Commit** `feat(io): DAWproject merge decisions, preview and report lines (plan 6e Task 5)`.

### Task 6: `merge::apply`: the plan as commands, in one transaction

**Files:**
- Create: `crates/core-io/src/dawproject/merge/apply.rs`, `crates/core-io/tests/dawproject_merge.rs`
- Modify: `crates/core-io/src/dawproject/import/apply.rs` (factor `apply_tracks` out of `apply`, Ruling 9; `apply`'s behaviour is unchanged)

**Interfaces:**
- Consumes: Tasks 3–5.
- Produces:
  ```rust
  pub const LABEL: &str = "Merge DAWproject";
  /// Inside the caller's open transaction on the project `plan` was made against: carries out
  /// each decision (Rulings 5–11), then merges the plan's report into `report` and sets its
  /// summary. `date` names conflict takes ("Returned <date>").
  pub fn apply_merge(h: &mut History, plan: &MergePlan, date: &str, report: &mut Report) -> Result<(), ModelError>;
  // import/apply.rs
  /// Adds plan tracks `which` (with their destinations, paths, strips and devices) and the
  /// buses they need to a project that may already have tracks; `apply` calls it for all.
  pub(crate) fn apply_tracks(h: &mut History, imp: &DpImport, seq: SeqId, which: &[usize],
                             buses: &mut BTreeMap<usize, StripId>, report: &mut Report) -> Result<SlotStates, ModelError>;
  ```
  - **Order of commands:**
    1. buses added;
    2. tracks added (`apply_tracks`);
    3. per matched track: name, notes, regions, automation;
    4. strips: fields, sends, devices, bypass, state;
    5. markers, tempo, meter, song info;
    6. deletions last.
  - Theirs' new slots get their state through `SetPluginState` inside the transaction, not `capture_state`.
  - A merge with no decisions runs no command.
  - Converted media no region uses are left for the caller to remove.

- [ ] **Step 1: Write the failing tests** in `dawproject_merge.rs`. Each applies the plan inside `begin(LABEL)`…`commit()` on a `History` of ours, then checks the result:
  - `theirs_only_edits_are_applied`: the owner's check, done in the file: a note moved 1 beat, the "Bass" strip's level changed from −6 to −3 dB, and a marker "Bridge" added at bar 9. All three are in ours; nothing else changed; `undo_label() == Some("Merge DAWproject")`.
  - `ours_only_edits_are_kept`: ours renamed a track and added a note after the export → both are still there.
  - `both_changed_notes_become_an_alternate_take`:
    - the lane has a new take "Returned 2026-11-13" holding theirs' notes;
    - ours' take is still active and unchanged;
    - the report's Conflicts holds `CONFLICT_TAKE` with "Bass: take “Returned 2026-11-13”".
  - `take_names_count_up_on_the_same_day`: the same conflict merged twice on one date → "Returned 2026-11-13 (2)".
  - `both_changed_settings_keep_ours_and_list_theirs`.
  - `deletions_follow_the_table`: a theirs-deleted untouched track is gone. A theirs-deleted track ours edited stays. An ours-deleted track theirs edited is still deleted.
  - `deleted_track_with_vizdaw_only_state_is_kept_empty` (Review Focus 4): input effects and an inactive take on a theirs-deleted track → the lane stays, its active take is empty, its inactive take and input effects are intact, and `KEPT_EMPTY` names it.
  - `vizdaw_only_state_is_byte_equal`: `common::merge::vizdaw_only(&doc)` serialises §7.8's keep list (other sequences, keyboard slots, inactive takes, input effects, inputs and monitoring, `return_of`, MPE input settings, drum rows, automation modes, MIDI learn, surfaces, the audio format, templates) to JSON. It is byte-equal before and after a merge that changes every carried thing.
  - `one_undo_restores_the_project_exactly`: after the merge in `theirs_only_edits_are_applied` and the conflict cases, one `undo` → `file::to_json(doc)` equals the pre-merge JSON.
  - `shared_sequence_edited_the_same_way_edits_the_sequence`: theirs moves the same note in both uses → one edit in the shared sequence, no new sequence, plus `SHARED_ELSEWHERE` when a keyboard slot uses it.
  - `shared_uses_edited_differently_become_private_copies`: only the second use edited → it now points at a new sequence named by `MakePrivateCopy`; the first use and the original sequence are unchanged; `PRIVATE_COPIES` names the sequence.
  - `looped_use_edited_on_one_pass_becomes_notes`: a 4-pass use with a note moved in pass 3 → the use is gone, theirs' notes are on the track, `LOOPS_UNROLLED` is listed, and the notes play exactly as theirs' projection.
  - `new_tracks_and_buses_are_added_as_an_import_would`: theirs adds a note track on a CLAP instrument and a bus with a send to it → the projection of ours' new track equals the 6d import of the same parts.
  - `crossfaded_pair_added_in_one_insert`: theirs adds two overlapping clips with covering fades → one crossfade in ours (6c F22).
  - `plugin_state_from_theirs_is_one_step`: theirs' insert state bytes differ → the slot's blob is theirs', and undo brings ours back.
  - `nothing_to_merge_adds_no_step` (Review Focus 2): an unchanged file → no command; `undo_label()` is what it was; the summary is `NOTHING`.
  - `merging_the_same_file_twice_changes_nothing_the_second_time` (Review Focus 2): merge, then `plan` again against the result → only `Same` and `Ours` decisions; applying adds no step, no note, no marker and no take.
- [ ] **Step 2: Run** `cargo test -p core-io --test dawproject_merge`. Expected: FAIL.
- [ ] **Step 3: Implement** `apply_merge` and the `apply_tracks` factoring. Run `cargo test -p core-io --test dawproject_import --test dawproject_round_trip` to see 6d unchanged.
- [ ] **Step 4: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +17 Rust).
- [ ] **Step 5: Commit** `feat(io): DAWproject merge applied as one transaction (plan 6e Task 6)`.

### Task 7: Merge tests without the record or tags, edit-script properties, hostile records and fuzz

**Files:**
- Create: `crates/core-io/tests/dawproject_merge_fuzz.rs`
- Modify: `crates/core-io/tests/dawproject_merge.rs` (the stripped variants), `crates/core-io/tests/common/merge.rs`

**Interfaces:**
- Consumes: Tasks 1–6.

- [ ] **Step 1: Write the failing tests:**
  - **Stripped variants** (§12): every Task 6 case runs for each `Strip` value. `Strip::Both` also drops the metadata export line and passes the snapshot explicitly (the user chose it), so the report holds `SNAPSHOT_CHOSEN` and `BY_NAME`. The expected document is the same for all four except where the case relies on a tag; those exceptions are named per case:
    - a renamed track becomes new plus deleted;
    - a marker moved by more than a tick becomes new plus deleted.
  - `edit_scripts_never_panic_validate_and_undo_once` (proptest, `common::cases(64)`):
    - random documents from 6d's round-trip generators;
    - a random script of 0–8 edits applied to theirs (on the `DpProject`: move, add or delete notes and clips, change volume, pan or mute, rename, add or move markers, change tempo, delete tracks);
    - another applied to ours (as commands);
    - a random `Strip`.

    Check that:
    - `plan` and `apply_merge` never panic and never return `Internal`;
    - the result validates (`core_model::rules::validate` or the crate's equivalent);
    - the preview counts equal the decisions;
    - one undo gives back ours exactly;
    - `vizdaw_only` is byte-equal.
  - `hostile_records` (§12): each case merges or is refused with one sentence:
    - a malformed `record.json` (6d's `RECORD_UNREADABLE` line, then tags);
    - snapshot `ids` pointing at a lane that no longer exists in ours (it is treated as deleted here);
    - ids pointing at the wrong kind (ignored, `TAGS_IGNORED`);
    - two ids naming one lane (the first wins);
    - a 4 000 000-entry ids map (reads, under 2 s);
    - one entry more (`SNAPSHOT_DAMAGED`).
  - `merge_fuzz.rs`: byte damage (flip, insert, delete, truncate) on three returned packages from Task 6's cases, and on their snapshot JSON (`VIZDAW_FUZZ_CASES`, default 256). Each case is refused or merges with a report: never a panic, never `Internal`, under 2 s and 256 MiB (`common::measure`).
- [ ] **Step 2: Run** `cargo test -p core-io --test dawproject_merge --test dawproject_merge_fuzz`. Expected: FAIL where a variant exposes a gap (fix in Tasks 4–6's code; say which in the report). If every new test passes at once, say so and show one mutation that makes `edit_scripts…` fail.
- [ ] **Step 3: Fix** what the runs find.
- [ ] **Step 4: Run** `cargo test --workspace` and both clippies. Expected: PASS (about +60 Rust, counting variants).
- [ ] **Step 5: Commit** `test(io): DAWproject merge without record or tags, edit-script properties, hostile records and fuzz (plan 6e Task 7)`.

### Task 8: `core-ffi`: the merge job, its steps, snapshots and the Conflict row

**Files:**
- Modify:
  - `crates/core-ffi/src/dawproject.rs` (requests, rows, `merge_body`, `merge_job`, `snapshot_rows`, sentences; factor 6d's `import_body` stages 5–7 into `read_and_convert`, used by both);
  - `src/jobs.rs` (`JobKindRow::MergeDawproject`, `Work::MergeDawproject`, `JobRow.merge`);
  - `src/interchange.rs` (`ReportKindRow::Conflict`, `report_row`);
  - `src/session.rs` (the methods, `held_merge`, `apply_jobs`' arm, `interchange_idle`'s kinds, close);
  - `src/lib.rs` (exports);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift`;
  - the Swift `switch`es named in Global Constraints.
- Test: `crates/core-ffi/tests/dawproject_merge.rs`

**Interfaces:**
- Consumes: Tasks 1–7; 6d's `info`, `installed`, `plist()`, the live capture in `export_dawproject`.
- Produces:
  ```rust
  #[derive(uniffi::Enum)] pub enum MergeStepRow { Preview, Hold, Apply }
  #[derive(uniffi::Record)] pub struct DawprojectMergeRequestRow {
      pub path: String, pub snapshot: Option<String>, pub step: MergeStepRow, pub date: String }  // date "YYYY-MM-DD"
  #[derive(uniffi::Record)] pub struct MergePreviewRow {
      pub applied: u32, pub kept: u32, pub added: u32, pub removed: u32, pub conflicts: u32,
      pub conflict_lines: Vec<String>, pub summary: String, pub export_id: String, pub exported_at: u64 }
  #[derive(uniffi::Record)] pub struct SnapshotRow { pub export_id: String, pub exported_at: u64, pub tracks: u32 }
  // DawprojectInfoRow gains: #[uniffi(default = None)] pub project_id: Option<String>  (the record head's)
  // JobRow gains:            #[uniffi(default = None)] pub merge: Option<MergePreviewRow>
  // ReportKindRow gains Conflict; report_row maps ItemKind::Conflict to it.
  impl Session {
      pub fn merge_dawproject(&self, req: DawprojectMergeRequestRow) -> Result<u64, VizError>;
      pub fn confirm_merge(&self, job: u64) -> Result<ReportRow, VizError>;   // swaps the held history (Ruling 12)
      pub fn cancel_merge(&self, job: u64) -> Result<(), VizError>;           // drops it and its media
      pub fn dawproject_snapshots(&self) -> Result<Vec<SnapshotRow>, VizError>; // newest first, at most 10; reads files: call off the main thread
      pub fn project_id(&self) -> Result<Option<String>, VizError>;
      pub fn has_snapshot(&self, export_id: String) -> Result<bool, VizError>; // one lstat; false for a malformed ID
  }
  pub const NOT_FROM_HERE: &str = "This file was not exported from that project.";
  pub const NOT_FROM_VIZDAW: &str = "This file did not come from VizDAW, so it can't be merged; open it as a new project.";
  pub const CHANGED_SINCE: &str = "The project changed after the preview; preview the merge again.";
  pub fn needs_snapshot(rows: &[SnapshotRow]) -> String; // Ruling 13's sentence; with none: NOT_FROM_HERE
  ```
  - **`merge_dawproject`:**
    - checks `interchange_idle` (a held merge counts), `media_jobs_idle`, not recording and no open edit;
    - `file_path(.., DAWPROJECT_FILE)`;
    - `date` matches `^\d{4}-\d{2}-\d{2}$` (else `Model` "the merge date must be YYYY-MM-DD");
    - `snapshot` is `None` or a project ID (`check_project_id`), else `Model` naming `snapshot`;
    - captures every slot's live state (the export's carve-out);
    - spawns with `history.clone()`, `present_serial()`, the root, the format, `reserved_names()`, `installed()`, the project ID, the request, and the 6d test hooks (`dawproject_cancel_at`, `dawproject_free`).
  - **The job** (inside `catch_unwind`; stages, for `debug_dawproject_cancel_at`):
    1. `open` (stage 5 after);
    2. **the export:** the record head's export ID, else the metadata line's, else `req.snapshot`. With none → `needs_snapshot(snapshot rows)`. A record or line naming a different export than a given `req.snapshot` → `NOT_FROM_HERE`. A record head whose `project_id` differs from the project's → `NOT_FROM_HERE`. A file that is not `from_vizdaw` → `NOT_FROM_VIZDAW`;
    3. `<root>/Interchange/<export id>.json` (lstat, a regular file) → `read(MAX_SNAPSHOT_BYTES)` → `read_snapshot`. Missing → `NOT_FROM_HERE`; damaged → `SNAPSHOT_DAMAGED`; its `project_id` differs from the project's → `NOT_FROM_HERE`;
    4. `read_and_convert` (6d's free-space check, extract and convert; stages 6–7);
    5. `map` with `env.ids = Some(&snapshot.ids)` only when the record head names this export (stage 8 after);
    6. `merge::theirs`, then `merge::plan` against the cloned history's document (stage 9 after);
    7. for `Hold` and `Apply`: `begin(LABEL)`, `apply_merge` (in `catch_unwind`), `commit` (stage 10 after).

    Returns `Work::MergeDawproject { files, result: Result<Box<Merged>, String> }`, where `Merged` holds the preview, the report, the step and, for `Hold`/`Apply`, the merged `History` and the serial.
  - **`apply_jobs`:**
    - `Preview` → Done, `merge` and `report` set, files removed;
    - `Apply` → the swap (below), Done with the report;
    - `Hold` → Done with `merge` set, kept in `inner.held_merge`.

    The **swap** happens only when `present_serial()` equals the job's; otherwise the job is Failed with `CHANGED_SINCE`. Then `inner.history = merged`, `last_step` and `surfaces.tempo_step` cleared, the host synced, `interchange_report` set with verb "Merged", and the files no history state references removed (`remove_unkept`). This absorbs backlog row "DAWproject import clean-up": an empty `.incoming` is removed after every merge, and on a panic too.
  - **`confirm_merge(job)`:** `Model` "no merge is waiting" for any other job. `Busy` while recording or in an open edit (the hold stays). The swap as above: on `CHANGED_SINCE` the hold and its files are dropped.
  - **`cancel_merge`, closing the session and a newer merge** drop the hold and remove its files.

- [ ] **Step 1: Write the failing tests** in `core-ffi/tests/dawproject_merge.rs`. The fixture is 6d's `project()` saved as a package, exported; the returned file is edited with `core-io`'s schema reader and writer, as Task 4's harness does:
  - `merge_applies_theirs_in_one_step`: `Apply` → Done, report summary "Merged: 3 applied, 0 kept, 0 added, 0 removed, 0 in conflict", the three edits in `debug_document_json`, `snapshot().undo_label == Some("Merge DAWproject")`; `undo` → the pre-merge JSON.
  - `preview_changes_nothing`: `Preview` → `JobRow.merge` with the counts. The document JSON, the undo label and `Media/` listings are unchanged; there is no hold.
  - `hold_then_confirm`: `Hold` → Done with `merge` set. Nothing has changed yet; `confirm_merge(job)` → the report, and the document changed as `Apply` would.
  - `hold_then_cancel_removes_its_media`: the returned file adds a region with new audio. After `cancel_merge` the `Media/` listing is as before.
  - `confirm_after_an_edit_is_refused` (Review Focus 3): `Hold`, then a rename → `confirm_merge` is `Model` `CHANGED_SINCE`, nothing applied, the hold gone.
  - `confirm_while_recording_is_busy`: recording (6d's fake) → `Busy`; after stopping, `confirm_merge` works.
  - `closing_drops_a_held_merge`: `Hold`, then `close` → the `Media/.incoming` and new media are gone.
  - `refusals_name_their_reason`, each Failed with the sentence:
    - another project's file (`NOT_FROM_HERE`);
    - the snapshot file deleted (`NOT_FROM_HERE`);
    - the snapshot truncated (`SNAPSHOT_DAMAGED`);
    - a hand "Fixture" file (`NOT_FROM_VIZDAW`);
    - a file with record and metadata line stripped and no `snapshot` → `needs_snapshot` naming the project's one snapshot with its date and track count;
    - a `snapshot` naming another export than the record's (`NOT_FROM_HERE`).
  - `chosen_snapshot_merges_a_stripped_file`: as above with `snapshot` → Done, with `SNAPSHOT_CHOSEN` and `BY_NAME` in the report.
  - `untitled_project_merges_from_its_temp_root`: export from an unsaved project, merge back into it → Done.
  - `snapshots_list_newest_first`: three exports → three `SnapshotRow`s, newest first, with track counts. `has_snapshot` is true for each, false for an unknown UUID and for `"../x"`.
  - `project_id_is_read`: saved project → `Some(id)`; `info(path)` of its export → `project_id` equal.
  - `cancel_at_every_stage_leaves_media_unchanged`: a cancel before the run, and at stages 5–10, each Cancelled. `Media/` and `Media/.incoming/` listings are unchanged, and so is the document JSON (each clears `debug_free_bytes` first).
  - `busy_states_refuse`: recording, an open edit, a running export, a Convert job, and a held merge each make `merge_dawproject` `Busy`.
  - `hostile_requests`: 6d's path list (empty, relative, a directory, a FIFO, `/dev/zero`, a symlink loop, over 4 KiB, NUL, non-UTF-8), `date` "13/11/2026" and "", and `snapshot` "x" and 4 KiB of `a`. Each is `Model` or `Io`, never `Internal`. `confirm_merge(999)` and `cancel_merge(999)` give `Model`.
  - `conflict_rows_reach_swift`: a merge with a conflict → `report.items` holds a `ReportKindRow::Conflict` row with `CONFLICT_SETTING`.
  - `merge_state_reaches_the_plugin` (CLAP test plug-in, 6d's `clap_state_round_trips` setup): theirs sets parameter 0 to 0.75 → after the merge `debug_plugin_param(slot, 0) == 0.75`; after undo it reads the pre-merge value.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test dawproject_merge`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above, then `scripts/build-ffi.sh`. Add the Swift `switch` arms (`.mergeDawproject`; `.conflict` grouped as "Conflicts"). `FakeSession` gains the six methods with settable answers and logs.
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo clippy -p core-ffi --lib -- -D warnings`, both clippies, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +18 Rust).
- [ ] **Step 5: Commit** `feat(ffi): DAWproject merge job with preview, hold and apply; snapshots and conflict rows (plan 6e Task 8)`.

### Task 9: The control channel: `mode`, `project`, `snapshot` and `preview` on `file.import_dawproject`

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (`IMPORT_DAWPROJECT_PARAMS`; the command's dialogs gain "Return" and an "Open" answered by `project`);
  - `crates/core-control/tests/commands.rs` (6d's `dawproject_commands_check_their_params`: `mode: "merge"` now validates, `mode: "fuse"` does not);
  - `app/Sources/VizDAWAppModel/InterchangeCommands.swift`;
  - `app/Sources/VizDAWAppModel/DocumentStore.swift` (`documentID(forProjectID:)`, `documentHolding(exportID:)`);
  - `app/Tests/VizDAWAppModelTests/InterchangeCommandsTests.swift`.

**Interfaces:**
- Consumes: Task 8.
- Produces:
  - **Registry params:**
    - `path`, as 6d;
    - `mode`: `Choice(&["new", "merge"])`, "new: open the file as a new project; merge: merge it into the project it came from. Needed when the file came from VizDAW.";
    - `project`: `Path("vizdaw")`, "For merge: the project the file came from, when it is not open (answers the Open dialog).";
    - `snapshot`: `Text`, "For merge: the export the file came from (an export ID), when the file lost its own.";
    - `preview`: `Flag`, "For merge: return the merge preview and change nothing.".
  - **Dialogs:**
    - "Open" (always; answered by `path`);
    - "Return" ("the file came from VizDAW and no mode"; answered by `mode`);
    - "Open" ("merge with no matching open project"; answered by `project`).
  - **Swift, the `run` case:**
    1. no `path` → `.dialog("Open", "path")`;
    2. otherwise `.pending` reads `dawprojectInfo` off the main actor, then:
       - `from_vizdaw` with no `mode` → `.dialog("Return", "mode")`;
       - `mode: merge` on a file not from VizDAW → `.failed(.of(.invalidParams, "file.import_dawproject: `mode` merge needs a file that came from VizDAW."))`;
       - `new` → 6d's `importDawprojectIntoNewProject`.
    3. **merge:**
       - a `snapshot` that is not 36 lowercase hex-and-hyphen characters → `invalid_params` naming `snapshot`;
       - the target is `project` when given. If its canonical path is an open document → `invalid_params` "file.import_dawproject: `project` is already open; leave out project to merge into it.". Otherwise it is opened through `store.open(path:)`; an alert there is `.failed(.alert)`.
       - without `project`: `store.documentID(forProjectID: info.projectId)` ?? `store.documentHolding(exportID: info.exportId)` ?? `.dialog("Open", "project")`;
       - then `.job(command:, doc: target, job: session.mergeDawproject(req: … step: preview ? .preview : .apply, date: MergeModel.today()))`.
    4. A project the command opened is closed again if the merge is refused before or by its job and it is untouched (`madeProjects`, as 6d's new projects).
  - ```swift
    // DocumentStore
    /// The most recently active open document whose project ID is `id` (nil for nil).
    public func documentID(forProjectID id: String?) -> UUID?
    /// The most recently active open document holding snapshot `exportID` (nil for nil).
    public func documentHolding(exportID: String?) -> UUID?
    ```

- [ ] **Step 1: Write the failing tests.**
  - `core-control`:
    - `merge_params_validate`: `{mode: "merge", path, project: "/x/a.vizdaw", snapshot: "<uuid>", preview: true}` validates. `mode: "fuse"`, `project: "a.vizdaw"` (relative), `project: "/x/a.dawproject"`, `preview: "yes"` and an extra key are each `InvalidParams` naming the parameter.
    - `every_command_survives_random_params` covers the new params.
    - `help_names_the_return_dialog`: the command's help JSON lists the "Return" dialog answered by `mode`.
  - Swift `InterchangeCommandsTests`:
    - `fromVizdawWithoutModeAsksForReturn`: info `fromVizdaw: true` → `dialog_required` "Return" naming `mode`; no `newProject`, no session call.
    - `modeNewStillMakesAProject`: as 6d's test with `mode: "new"`.
    - `mergeWithoutAMatchAsksForOpen`: no open document matches → `dialog_required` "Open" naming `project`.
    - `mergeFindsTheOpenProjectByIDThenBySnapshot`: two fakes, one answering `projectId` and one `hasSnapshot` → the merge goes to the ID match. With no record ID → the snapshot holder. The request carries `step: .apply`, the path and today's date.
    - `previewSendsPreviewAndChangesNothing`: `preview: true` → `step: .preview`.
    - `mergeIntoAProjectPathOpensIt`: `project` names a closed package → one `open(path:)`, then the merge on it. A refused job closes it again.
    - `mergeRefusesHostileArguments` (calling `InterchangeCommands.run` directly, as 6d's S7), each `invalid_params` naming the command and the parameter, with no session call:
      - `mode: "merge"` on a non-VizDAW file;
      - `project` already open (the same path through a symlink);
      - `project` that is not a package (a plain file named `x.vizdaw`);
      - `snapshot` "x".
    - `mergeWhileTheTargetRecordsIsBusy`: the fake's `mergeDawproject` throws `Busy` → `busy`.
    - `everyRegisteredMenuCommandHasAHandler` passes.
- [ ] **Step 2: Run** `cargo test -p core-control` and `scripts/swift.sh test --filter InterchangeCommands`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run** `cargo test -p core-control`, both clippies, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (registry 175; about +3 Rust, +9 Swift).
- [ ] **Step 5: Commit** `feat(control): merge parameters and the Return dialog on file.import_dawproject (plan 6e Task 9)`.

### Task 10: The app's model: the return prompt's state, `MergeModel` and the Conflicts group

**Files:**
- Create: `app/Sources/VizDAWAppModel/MergeModel.swift`, `app/Tests/VizDAWAppModelTests/MergeModelTests.swift`
- Modify:
  - `app/Sources/VizDAWAppModel/DocumentStore.swift` (`returnPrompt`, `answerReturn`, `chooseMergeProject`; `dawprojectRead` routes a from-VizDAW file to the prompt);
  - `DocumentModel.swift` (lazy `merge`, as `dawproject`);
  - `ForDawModel.swift` (`blocking` gains `.mergeDawproject`);
  - `Interchange.swift` (`ReportText.groups`: "Conflicts" first);
  - `app/Tests/VizDAWAppModelTests/DocumentStoreTests.swift`, `InterchangeTests.swift`.

**Interfaces:**
- Consumes: Tasks 8–9.
- Produces:
  ```swift
  public struct ReturnPrompt: Equatable, Sendable {
      public let path: String; public let info: DawprojectInfoRow
      public let match: UUID?; public let matchName: String?          // the open project, named in the prompt
  }
  public enum ReturnChoice: Sendable { case newProject, merge, cancel }
  // DocumentStore
  public private(set) var returnPrompt: ReturnPrompt?   // set only by UI opens (openDawproject); the app shows it
  /// New Project: 6d's path. Merge: into `match`, else `chooseMergeProject()` (an Open panel the app sets;
  /// nil → nothing), opened with `open(path:)` or found open, then `merge.begin`. Cancel: nothing.
  public func answerReturn(_ choice: ReturnChoice)
  public var chooseMergeProject: (() -> String?)?
  @MainActor @Observable public final class MergeModel {
      public init(document: DocumentModel)
      public var shown: Bool
      public private(set) var path: String
      public private(set) var snapshots: [SnapshotRow]         // when the file lost its export ID
      public var chosenSnapshot: String?                       // starts the preview when set
      public private(set) var preview: MergePreviewRow?
      public private(set) var error: String?                   // the core's sentence
      public private(set) var job: UInt64?
      public var running: Bool { get }
      /// Shows the sheet; without an export ID, lists snapshots off the main actor; else starts the Hold job.
      public func begin(path: String, info: DawprojectInfoRow)
      public func confirm()            // confirmMerge(job:) → InterchangeModel.report shows the report; closes
      public func cancel()             // cancels a running job or cancelMerge(job:); closes
      public func settle() async
      public static func today(_ now: Date = Date()) -> String   // "yyyy-MM-dd", POSIX locale, current time zone
  }
  ```
  - `MergeModel` follows `DawprojectModel`: detached reads with a generation counter, and its job watched through the document's job polling. A Done job with `merge` sets `preview`; a Failed one sets `error`.

- [ ] **Step 1: Write the failing tests.**
  - `DocumentStoreTests`:
    - `aFileFromVizdawPromptsInsteadOfOpening`: info `fromVizdaw: true` → after `settle()`, `returnPrompt` is set and no `newProject` was made. A plain file still makes its project at once (6d's test passes).
    - `thePromptNamesTheOpenMatch`: an open fake answering `projectId` = the info's → `match` is its ID and `matchName` its display name.
    - `newProjectAnswerOpensAsSixD`: `.newProject` → one `newProject` at the info's rate and one `importDawproject`.
    - `mergeAnswerGoesToTheMatch`: `.merge` → the match's `merge.shown` is true and one `mergeDawproject(step: .hold)` was sent.
    - `mergeWithoutAMatchAsksForAProject`: `chooseMergeProject` returns a path → one open of it, then `begin` on it; returning nil → nothing.
    - `cancelAnswerDoesNothing`.
  - `MergeModelTests`:
    - `previewArrivesFromTheHeldJob`: the fake's job ends Done with a `MergePreviewRow` → `preview` set, `running` false.
    - `aLostExportIDListsSnapshotsOffTheMainActor`: info `exportId: nil` → `snapshots` filled after `settle()`, no job sent. Setting `chosenSnapshot` → one `mergeDawproject` with that snapshot.
    - `noSnapshotsShowsTheSentence`: an empty list → `error` is the core's `NOT_FROM_HERE` sentence (from the fake's settable answer), and Merge is unavailable.
    - `confirmShowsTheReport`: → one `confirmMerge(job:)`; `document.interchange.report` is the returned row; `shown` false.
    - `aRefusedConfirmShowsTheSentence` (Review Focus 3): `confirmMerge` throws "The project changed after the preview; preview the merge again." → `error` is that text and the sheet stays.
    - `cancelSendsCancelMerge`: with a held job → one `cancelMerge(job:)`. With a running job → one cancel of it.
    - `todayIsLocalISO`: a fixed `Date` and time zone → "2026-11-13".
  - `InterchangeTests`: `conflictsComeFirst`: a report with every kind → groups "Conflicts", "Changed", "Left out", "Notes"; `plain` lists them in that order.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter Merge`, `--filter DocumentStore` and `--filter Interchange`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run** `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +17 Swift).
- [ ] **Step 5: Commit** `feat(app): return prompt state, merge model and the Conflicts group (plan 6e Task 10)`.

### Task 11: The prompt, the merge sheet, README checks, the merge demo, e2e, backlog and gates

**Files:**
- Create: `app/Sources/VizDAW/MergeSheet.swift`
- Modify:
  - `app/Sources/VizDAW/AppCommands.swift` (`ReturnPrompts.watch`, beside `StoreAlerts`);
  - `app/Sources/VizDAW/ProjectPanels.swift` (`chooseProjectToMerge()`, an Open panel for `.vizdaw` packages through 6b's panel helper; set as `store.chooseMergeProject` at launch);
  - `app/Sources/VizDAW/DocumentView.swift` (the sheet's presentation);
  - `app/Sources/VizDAW/ReportSheet.swift` (the "Conflicts" group's look);
  - `crates/core-ffi/tests/dawproject_merge.rs` (`write_merge_demo`, ignored);
  - `crates/vizctl/tests/e2e.rs` (6d's `dawproject_round_trip_over_the_channel` passes `mode: "new"` in step 5 and uses `mode: "fuse"` in step 2, Ruling 16; a new test);
  - `app/README.md`, `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 8–10.
- Produces:
  - **`ReturnPrompts`** (`@MainActor enum`):
    - watches `store.returnPrompt` as `StoreAlerts` watches `store.alert`;
    - shows an `NSAlert` with Ruling 14's texts and the buttons in that order, then calls `store.answerReturn`;
    - under `ControlChannel.active` it shows nothing, calls `ControlChannel.note` with "A DAWproject from VizDAW was opened; it was not merged" and answers `.cancel`.
  - **`MergeSheet`:**
    - title "Merge DAWproject";
    - while running, "Reading the file…" with a progress bar and Cancel;
    - with `snapshots`, "Which export did this file come from?" and a list (date and time, "<n> tracks"), then Preview;
    - with `preview`: its `summary` line, then "Conflicts (VizDAW's version is kept)" and `conflictLines`, or "No conflicts.";
    - `error` in the theme's warning style;
    - Cancel and Merge (default button; disabled without a preview).

    Theme tokens; every control accessibility-labelled.

- [ ] **Step 1: Write the e2e** `dawproject_merge_over_the_channel` (`#[ignore = "needs a window server and VIZDAW_E2E_APP"]`, throwaway home, offline):
  1. `app.new_demo`; `file.save_as {path: <home>/demo.vizdaw}`; `file.export_dawproject {path: <home>/demo.dawproject}` → Done.
  2. `file.import_dawproject {path}` → `dialog_required` "Return". With `mode: "merge", snapshot: "x"` → `InvalidParams` naming `snapshot`. With `mode: "merge", project: <home>/demo.vizdaw` → `InvalidParams` (already open). `window_numbers` is unchanged after them.
  3. `{path, mode: "merge", preview: true}` → the job's report summary is "Nothing to merge: the file matches the project.". `state.get document`'s undo label is unchanged.
  4. `{path, mode: "merge"}` → Done, the same summary, no new undo step, no new window.
  5. `file.interchange_report` returns that report.
  6. `{path, mode: "new"}` → a new window with the demo's track count (6d's path still works).
- [ ] **Step 2: Add the merge demo writer** `write_merge_demo` (`#[ignore]`, runs only with `VIZDAW_MERGE_DEMO=<dir>`, which must be an empty existing folder outside `~/Library`). It writes `<dir>/Merge Demo.vizdaw` (Task 8's fixture with a keyboard slot, an input effect and a second sequence) and `<dir>/Returned.dawproject`:
  - a note moved;
  - the "Bass" level changed;
  - a marker "Bridge" added;
  - a level conflict on "Drums" and a notes conflict on "Keys" (the project changes them after the export).

  It also writes `<dir>/Other.dawproject`, 6d's hand fixture `hand_other` packaged, and prints the three paths. Run it once to see it work.
- [ ] **Step 3: Add README checks 291–299** after 290, under "### DAWproject merge (plan 6e)". Start with a line naming the tests that cover the rest:
  - `core-io`'s `dawproject_merge*.rs`;
  - `core-ffi/tests/dawproject_merge.rs`;
  - `MergeModelTests`, `DocumentStoreTests`, `InterchangeCommandsTests`;
  - the e2e `dawproject_merge_over_the_channel`.

  The steps:
  - 291: Make the demo files (`VIZDAW_MERGE_DEMO=<an empty folder> cargo test -p core-ffi --test dawproject_merge write_merge_demo -- --ignored`) and open `Merge Demo.vizdaw`.
  - 292: File › Import Project › DAWproject… with `Returned.dawproject`. The prompt says "This DAWproject came from a VizDAW project.", names the open project, and offers New Project, Merge With Existing Project… and Cancel. Cancel does nothing.
  - 293: Again, and choose New Project: a new untitled window opens, as in 286.
  - 294: Again, and choose Merge With Existing Project…: the project's window shows the Merge DAWproject sheet, with "Reading the file…" and then the counts line and two conflicts ("Drums" level, "Keys" notes). Cancel leaves the project and Edit › Undo's label unchanged.
  - 295: Merge: the note moved, the "Bass" level changed and the "Bridge" marker arrived. "Drums" keeps its level. "Keys" has a take "Returned <today>" that is not active. The report opens with "Conflicts" first. The keyboard slot, the input effect and the second sequence are still there.
  - 296: Edit › Undo Merge DAWproject restores everything from 295 in one step; Redo brings it back.
  - 297: Close the project. Drop `Returned.dawproject` on the Dock icon: the prompt says no project is open. Merge With Existing Project… shows an Open panel; choose `Merge Demo.vizdaw`; it opens and the sheet follows.
  - 298: Merge the same file a second time: the sheet's counts show nothing applied again, and no new take appears.
  - 299: Open `Other.dawproject` from the demo folder (a file from another source): no prompt; it opens as a new project.
  - **Owner check (manual, only the owner can do; spec §12, 6e's part):**
    - export the demo song;
    - open it in the DAW you use that reads DAWproject; move a note, change a level and add a marker there; export a DAWproject back;
    - merge it into the original. Check that the three edits arrived and that keyboard slots, input effects and other sequences are still there;
    - undo the merge.
- [ ] **Step 4: Update the backlog**, in its style:
  - **New rows:**
    - "DAWproject merge: finer conflicts" (Ruling 5: a track's notes conflict as a whole; single-note three-way is a refinement);
    - "DAWproject merge: replaced audio" (Ruling 11: a matched region keeps our media);
    - "DAWproject merge: shared sequences used outside the export" (Ruling 8, owner question 2);
    - "DAWproject merge: format 1 snapshots" (Ruling 3: drop the heuristic base path once no format-1 snapshots remain).
  - **Absorbed 6d rows:**
    - in "DAWproject: the import applies on the main thread", append "Plan 6e: merges build their history on the job thread and swap it in (Ruling 12); the new-project import still applies on the main thread";
    - in "DAWproject import clean-up", append "Plan 6e: the shared read-and-convert stage removes an empty `.incoming` after every job and after a panic".
- [ ] **Step 5: Run all the gates:**
  - `cargo fmt --all -- --check`;
  - both clippies and `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `cargo test --workspace`;
  - `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test` (name scans included);
  - the no-brand build;
  - `scripts/make-app.sh` and the e2e.

  Afterwards `pgrep -lf MacOS/VizDAW` prints nothing. Expected: all PASS (e2e 28, registry 175, README through 299, `VERSION` 9).
- [ ] **Step 6: Commit** `feat(app): return prompt, merge sheet; README checks 291–299, merge demo, e2e and backlog (plan 6e Task 11)`.

## Self-Review

- **Spec coverage (6e's row of §13):**
  - **The prompt** (§7.8, §9; owner) → Ruling 14, Tasks 10–11. **When it appears** → Ruling 16, 6d's `from_vizdaw`.
  - **Finding the project:** the open match, the Open panel, refusals and the snapshot list → Ruling 13 and Tasks 8–10.
  - **Matching:** tracks and strips by tag, record ID, name and order; notes within a tick; clips by tag or media plus position; markers; automation; maps; song info; shared sequences → Rulings 4 and 8, Tasks 2, 4 and 6.
  - **The merge rules table,** every row → Ruling 5 and Task 5; applied → Task 6. **Owner: VizDAW wins** → Rulings 5–6.
  - **What a merge takes and always keeps** → Rulings 5, 10 and 11, Task 6's `vizdaw_only_state_is_byte_equal`.
  - **One undo step, one batch, staged media** → Ruling 12, Global Constraints, Tasks 6 and 8.
  - **The preview page** → Rulings 12 and 14, Tasks 8, 10 and 11. **The Conflicts group** → Ruling 15, Tasks 1, 8 and 10.
  - **AAF does not merge:** nothing to build here.
  - **§6.1 merge row** → Task 8. **§6.2** jobs and staging → Task 8. **§6.3** → Ruling 15. **§6.5** → Ruling 17, Tasks 1 and 7.
  - **§10:** `mode`, `project`, `snapshot`, `preview`, `dialog_required` "Return" and "Open", every listed hostile case → Task 9 (with Task 8's core refusals).
  - **§11** → each refusal sentence. **§12 Merge (6e):** three-way tests, stripped variants, proptest edit scripts, hostile records → Tasks 5–7; the e2e, README checks and owner check → Task 11.
- **Deliberately not here:** the AAF and OMF routes (6f, 6h), Multi-Q and its `Equalizer` mapping (6g), single-note conflicts, and merging replaced audio (backlog rows, Task 11).
- **Names are consistent across tasks:** every name used in a later task is one an earlier task's Interfaces block produces (checked: `Identity`, `TrackIds`, `EventAt`, `UseAt`, `StripIds`, `Origins`, `apply_tagged`, `Theirs`, `Matched`, `Matches`, `MatchedBy`, `MergePlan`, `MergePreview`, `Decision`, `Outcome`, `Unit`, `apply_merge`, `LABEL`, `MergeStepRow`, `DawprojectMergeRequestRow`, `MergePreviewRow`, `SnapshotRow`, `ReturnPrompt`, `ReturnChoice`, `MergeModel`).
- **Review Focus:** each line names its tests in the owning task.
- **Spec conflicts and questions for the owner** (each settled by a Ruling until answered):
  1. Ruling 7: a track the other DAW deleted is kept, emptied, when it holds what the file cannot carry. The spec's table says "Removed", but its keep list says inactive takes and input effects always stay.
  2. Ruling 8: an edit to a shared sequence also plays where the sequence is used outside the merged sequence (other sequences, keyboard slots), with a Note. The alternative is private copies for every exported use.
  3. Ruling 11: region gain never comes back. The file has no clip gain, and 6d's import sets 0, though §7.8 lists "region names and gain" as taken from theirs.
  4. Ruling 5: a track's notes conflict as a whole. Edits to different notes of one track on both sides become an alternate take, as the spec's table says, rather than a note-level merge.
  5. Ruling 16: the prompt appears at "at least half" of the tracks tagged (6d), where the spec says "most".
