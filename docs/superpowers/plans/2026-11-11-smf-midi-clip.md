# MIDI Files and the Interchange Base Implementation Plan (6a)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** VizDAW imports and exports Standard MIDI Files (types 0 and 1) and MIDI Clip Files (`.midi2`), safely against hostile files, with a report of everything that changed or was left out; format version 8 adds song info, region names and a project ID.

**Architecture:**
- A new platform-neutral crate, **`core-io`**, holds the byte-level readers and writers (SMF, UMP, Clip File), the mappings between them and the model (`midi_in`, `midi_out`), the hostile-input limits, guarded reading, partial-file writing and the `Report`.
- `core-ffi` runs every import and export as a job on the existing `Jobs`, applies an import as one history group ("Import MIDI File"), keeps the last report per document, stamps the project ID on save, and exposes song info and region names.
- The control channel gains `file.import_midi`, `file.export_midi`, `file.export_midi_clip` and `file.interchange_report`. Swift gains the MIDI import and export sheets, the report sheet, the Song info section of Project Settings, an editable region name, File › Open and Dock opening of `.mid`/`.midi2`, and MIDI drops on the arrangement.

**Tech Stack:** Rust 2024 (`core-model`, `core-io` new, `core-media`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6.1 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-11-11-import-export-design.md`: plan 6a's row of §13, with §2 goal 1, §4, §5, §6 (all of it), §7.1's SMF and Clip File columns, §7.2, §7.3, §9 (the MIDI items, the report, Project Settings › Song info), §10 (the four 6a commands), §11, §12 (SMF and Clip File parts) and §14's SMF type 2 and container rows. Parent: `2026-10-15-vizdaw-design.md` §5 and §8.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`. Swift only through `scripts/build-ffi.sh` (after any FFI change), `scripts/swift.sh build|test` and `scripts/make-app.sh`; never `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Branch `sp6-import` (at `561e4c2` when this plan was pre-flighted). Never switch branches, never push.
  - Commit at the end of every task by explicit paths (never `git add -A`). Never commit `.superpowers/`, `.DS_Store` or generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`).
  - `Cargo.lock` changes only in Task 2 (the new `core-io` package entry) and Task 7 (`core-ffi` gains the `core-io` dependency; F1). No new third-party crate: `core-io` uses only crates already in the lock (`im`, `thiserror`; `proptest`, `tempfile` for tests).
  - Every message ends with the executing session's own attribution lines (F25): its harness's `Co-Authored-By` line naming the model actually running, then the session line, e.g.
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Then check with `git log -1 --oneline && git show --stat HEAD` (a commit without `DEVELOPER_DIR` fails silently).
- **Clean room and names.** Never read `~/workspace/SVP/research`. No Opcode, Logic, Apple (except existing platform facts), Midas, Soundcraft, NI, Microsoft, Ableton, Steinberg/Cubase or ROLI names in UI or repo text; say "other DAWs". The MIDI Association, General MIDI and MPE are specification names and are allowed. Enforced for the app by `NameScanTests.noThirdPartyNamesInWhatTheAppShows` and `core-control/tests/names.rs` (whose directory list Task 2 extends with `core-io/src`; F15).
- **Track, not lane.** Text the app shows (labels, menus, sheets, report lines, alerts, README steps) says "track"; identifiers, command IDs, JSON keys and the file format keep `lane`. `core-ffi/tests/wording.rs` scans string literals of the crates it lists: Task 2 adds `core-io` to that list.
- **Never run `auval`**, in any form.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. The owner checks the views by eye from README steps 257–266.
  - Launch VizDAW only through the e2e (Task 10) with `VIZDAW_AUDIO=offline`; afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - Tests never open real audio or MIDI, never trigger the microphone prompt or TCC, and never touch the real `~/Library/Application Support/VizDAW`, preferences, Trash or devices: temp folders (`tempfile`), `Session::debug_set_trash_dir`, `MemoryDefaults()`, the e2e's throwaway home, fakes only. Every `core-ffi` test file calls `vizdaw_ffi::host::force_offline()` through its `serial` helper.
- **Programmatic calls open nothing.** Control-channel commands and programmatic calls never open dialogs, sheets, menus, panels or windows. The one exception already in the house: a command that makes a document (`file.new`, and here `file.import_midi` with `new_project: true`) shows that document's window, as `file.new` does. The report sheet opens only for jobs the user started from the UI.
- **Hostile input.** Every new command gets hostile-input tests at every layer (Task 8). Every parser obeys §6.5's limits (Task 2's constants) and is fuzzed (Tasks 3–4).
- **FFI safety.** No panic crosses the FFI: every new export returns `Result<_, VizError>` inside `ffi_guard` (`catch_unwind`). Job threads run parsing and mapping inside `std::panic::catch_unwind`; a caught panic fails the job with the message `internal error: …` and is a bug.
- **Swift 6.1.** Generated bindings compile in Swift 5 mode; app modules and tests in Swift 6 mode with no new warnings. No `@State` or `@FocusState` anywhere in this plan's code (view state lives in the `@Observable` models). Never capture `[weak self]` or a non-Sendable value inside `MainActor.assumeIsolated`; bind a strong `let` first, as `DocumentWindow.swift` does. Colours, spacing, radii and fonts come from the theme; every new control is accessibility-labelled.
- **Lints.** Every crate, `core-io` included, keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`: `.get()`, iterators, `checked_*`/`saturating_*`. Rust stable, edition 2024, GPL-3.0-only. `core-model`, `core-engine`, `core-media`, `core-io` and `core-control` stay free of OS APIs; everything Rust here runs on Linux.
- **Checked arithmetic.** Every computed tick, length, offset and size in `core-io` uses `checked_*`; an out-of-range item is left out and reported, never wrapped or saturated into a wrong value.
- **TDD.** Every new test is run and seen to fail before the code that makes it pass.
- **Real-time.** This plan adds no real-time code. Task 1 touches `EventBody::AudioRegion` (a new `Option<String>`), so it runs the rt-check gate: `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_input_fx --test rt_alloc_midi_strip --test rt_alloc_bounce --test rt_alloc_mpe`.
- **Media is write-once.** Nothing here writes media. Exports write a partial file (`.<name>.partial`) and rename it into place at the end; nothing is ever written inside a `.vizdaw` package by an export.
- **Format:** `core_model::file::VERSION` 7 → 8 (Task 1); `migrate_v7_to_v8` is the identity.
- **UniFFI changes** (Tasks 1 and 7), every new field `#[uniffi(default)]` or `#[uniffi(default = None)]`:
  - Task 1: `EventRow.region_name: Option<String>`; `AudioEdit::SetName { event: u64, name: Option<String> }`.
  - Task 7: records `ReportRow`, `ReportItemRow`, `SongInfoRow`, `MidiFileInfoRow`, `ChannelUseRow`, `ChannelMapRow`, `MidiImportRequestRow`, `SmfExportRequestRow`, `ClipExportRequestRow`; enums `ReportKindRow`, `MidiFileKindRow`; `JobKindRow::{ImportMidi, ExportMidi}`; fields `JobRow.report: Option<ReportRow>`, `JobRow.written: Vec<String>`; `ControlResult::Interchange`; the `Session` methods in Task 7. `FakeSession` gains every new method in Task 7.
- **Control channel.** The registry goes 166 → 170 in Task 8. Every command is in the registry and handled (`everyRegisteredMenuCommandHasAHandler`).
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the Linux clippy (`cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`), and `cargo test` for the crates the task touches (`cargo test -p core-io`, `cargo test -p core-model`, `cargo test -p core-ffi`, …); the whole workspace in Tasks 1, 6, 7 and 10.
  - Tasks 1 and 7–10 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 10 also runs the no-brand build (`VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <a fresh temp dir>`), `scripts/make-app.sh` and the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.
  - Known flaky: the `damaged_files` proptest, the MIDI listener tests (`platform-macos/tests/midi.rs`), `core-ffi` catalog tests under parallel load, the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline:** measure at Task 1 start (Rust passed/ignored, Swift, e2e 23, registry 166, README through 256, `VERSION` 7). Each task's last step gives the counts it adds; report the measured counts (a slip in the writer's arithmetic is not a failure).
- **How code is given.** New test files are written whole. New implementation files get their public items with doc lines, and the algorithm where the signature and tests leave a choice. Changes to existing files follow the existing pattern named in the step; if a named anchor no longer matches, make the same change to the current text and say so in the report.

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no ordinary test would exercise, most likely first. Each is pinned by named tests in its task.

1. **A damaged or hostile MIDI file.** Examples: a file cut off mid-event, a chunk length past the end of the file, a data byte with no status before it, a 1 GiB SysEx length, 100 000 empty `MTrk` chunks, a `.mid` that is really a FIFO, `/dev/zero` or a directory, a symlink loop, a `.midi2` of random UMP words.
   - Expected: refused with one plain sentence naming what and where ("This MIDI file can't be read: track 3 ends in the middle of an event."), or imported with the damaged items in the report. Never a panic, never `VizError::Internal`, never a hang; memory and time bounded by the §6.5 limits (each fuzz case, run with lowered `Limits`, under 2 s and 256 MiB; F5).
   - Pinned by `hostile_smf_is_refused_or_reported` and `smf_fuzz_never_panics` (Task 3), `hostile_clip_is_refused_or_reported` and `clip_fuzz_never_panics` (Task 4), `read_regular_refuses_non_files` (Task 2), `hostile_import_paths_are_refused` (Task 7).
2. **Positions that would wrap.** Examples: deltas of 0x0FFFFFFF at a division of 1 (each ×15 360), ten million of them; an import at tick 2⁴⁰ − 1; an SMPTE division of 0 ticks per frame.
   - Expected: what falls past `core_model::rules::MAX_TICKS` is left out and counted ("Events past the end of the timeline"); a zero division refuses the file; nothing wraps.
   - Pinned by `huge_deltas_are_left_out_not_wrapped` (Task 5) and `zero_division_is_refused` (Task 3).
3. **Importing into a project that already has material.** Examples: a song with its own tempo ramp; a file at 90 BPM dropped at bar 9; a file using channels 1, 2 and 10 into a project that already has a destination on channel 2.
   - Expected: the project's tempo and meter maps are untouched unless "Use the file's tempo map" is on; channel 2 reuses the existing destination, channels 1 and 10 get new built-in-synth destinations; one Undo ("Undo Import MIDI File") restores the document exactly.
   - Pinned by `importing_into_a_project_keeps_its_tempo_map`, `channels_reuse_existing_destinations` (Task 5) and `import_is_one_undo_step` (Task 7).
4. **Writing over things.** Examples: exporting to an existing file without `replace`; exporting into the open project's package; cancelling mid-export; exporting several clips into a folder where one name is taken.
   - Expected: refused before anything is written, or nothing left behind (no partial file, no half file); the taken clip name is refused without writing any of the batch.
   - Pinned by `exports_never_leave_half_files` and `hostile_export_paths_are_refused` (Task 7), `partial_files_are_removed_unless_committed` (Task 2).
5. **What a round trip keeps.** Examples: two overlapping notes on one pitch, a note-on of velocity 0, velocities that started as 7 bits, positions off the 960 grid, a muted note, per-note bend on a non-MPE track.
   - Expected: first-on first-off pairing; velocity 0 ends a note; 7-bit data round-trips exactly; off-grid positions round and the report says "Positions rounded to 960 ticks per quarter"; muted notes and per-note expression are listed as Left out with counts.
   - Pinned by `overlapping_notes_pair_first_on_first_off` (Task 5), `smf_round_trip_keeps_what_the_route_carries` and `export_lists_what_it_leaves_out` (Task 6).

## Plan Rulings

These are this plan's decisions where the spec leaves a choice. Each names its cost if wrong.

1. **`core-io` depends on `core-engine` too** (spec §5 names `core-model` and `core-media`). It reuses `core_engine::midi1::{translate, note_on_velocity, value7, bend14, Midi1Msg}`, `core_engine::input_fx::{up7, up32, LiveMsg, Expr, NoteExpr, CENTER}` and the 5g-2 `core_engine::mpe::{MpeDecoder, MpeEncoder, configuration}`, which §7.1/§7.2 require ("5g-2's encoder", "5g-2's decoder"). It still never depends on `core-ffi`. *Cost if wrong:* moving three small modules into a shared crate.
2. **6a adds only `JobKindRow::ImportMidi` and `ExportMidi`.** `ImportProject` and `ExportProject` arrive with the plans that use them (6b/6d), so no variant exists without a path. *Cost if wrong:* two enum lines.
3. **Damage inside a track chunk refuses the file** (a chunk past the end, a truncated event, a data byte without running status), as §11's example sentence does. Damage to one event's *values* (a tempo of 0, a time signature with denominator 2¹⁰⁰, an unmatched note-off, an unterminated SysEx) leaves that event out and reports it. *Cost if wrong:* a "keep what parsed so far" branch in the reader.
4. **Export ignores track mute and solo**: the chosen tracks are written as their events play (event mutes, play quantize, shift, own length and loop applied, active take only). A muted or unsoloed track is still written, with a Note. *Cost if wrong:* a filter in `midi_out`.
5. **Destinations on import.** `(group, channel)` with an existing destination on that group and channel (built-in synth or external; the first by ID) reuses it; otherwise a new built-in-synth destination is added, named `Built-in Synth (ch N)` with N 1-based. Events whose `(group, channel)` differs from their track's destination carry the `group_channel` override, and a `dest` override when their pair maps to another destination. *Cost if wrong:* the suggestion function.
6. **"As a new project"** is the app making a new untitled project (48 kHz at the default project depth, §6.1) and importing into it with `new_project: true`. The core then refuses a project that is not empty, uses the file's tempo, meter, title (as the sequence name) and copyright (into `SongInfo.copyright`), and applies **without an undo step** (the history is replaced; the document reads Edited). *Cost if wrong:* the import becomes an undo step instead.
7. **Into a project**, the file's title and copyright are left out (Note), its markers are added at their offset positions, and its tempo and meter maps only with "Use the file's tempo map". *Cost if wrong:* a line per field.
8. **Pickups.** Export writes a sequence's pickup (`start_offset > 0`) as a first bar whose time signature is the pickup length (`num = pickup ÷ beat`, the meter's denominator) when that is a whole number of beats, else as Changed ("Pickup written as a full bar"). Import turns a first time signature whose bar is shorter than the next one's into `start_offset` only as a new project or into an empty project with the file's tempo map. File position 0 is the pickup start. *Cost if wrong:* the pickup round trip (a fixed test).
9. **Text.** Meta and Flex Data text is decoded as UTF-8 with lossy replacement, control characters other than newline removed, and cut at `MAX_TEXT_BYTES` (Changed). Track names are trimmed and must pass `core_model::lane::valid_name`, else "Track N". One `core_io::text::sanitize(bytes: &[u8]) -> (String, bool /* cut */)` does this for SMF and Clip File alike (F9).
10. **Event order at one tick** on export: meta and Flex Data first; then channel messages sorted by `core_engine::midi1::Class` (NoteOff, Patch, Control, NoteOn; SysEx sorts as Control), then by one sub-key: within Control, SysEx after the channel messages, and otherwise the order `translate` produced (F22a). No second ordering table. Note-off is `8n` with the release velocity (never `9n 00`).
11. **SMF type 0 is split by channel by default; type 1 is not.** "Split by channel" in the sheet (and `split_channels`) overrides the default for either type. A track that is not split takes its main channel's destination (the channel with the most notes; ties go to the lowest).
12. **Only the import sheet's defaults apply to drops, File › Open and the Dock**: no sheet opens for them (a drop imports at the drop point and track; Open and the Dock import as a new project).
13. **A MIDI 2.0 note-on of velocity 0 is a note-on** (MIDI 2.0 semantics); only SMF and MIDI 1.0 packets treat velocity 0 as a note-off.
14. **The MIDI Clip File's text uses Flex Data as pinned in Task 4.** Marker and cue have no Flex Data status in the table this plan pins; they are written as Metadata Text "unknown" (status bank 0x01, status 0x00) prefixed `marker: ` and `cue: `, and read back by that prefix. Sequence markers use the `marker: ` form too; Task 6's text table says which goes where (F21). *Cost if wrong:* two statuses, once Task 4's check finds them.
15. **The report sheet's text** is built in Swift from `ReportRow` (summary line from the core, groups in the order Changed, Left out, Notes; a 6e plan adds Conflicts first). Items group by `(kind, what)` with a count and up to 50 details, then "and N more".
16. **The project ID is stamped on save by the core**, on every version in the history (present, undo and redo), so Undo never removes it. A save during an open edit (`in_transaction()`) skips the stamp; the next save stamps it (F23). It is a v4 UUID from `/dev/urandom` (a time-and-pid hash if that cannot be read), lowercase and hyphenated.
17. **Readers take `&Limits`** (`core_io::limits::Limits`, `Default` = the constants, public fields), so tests reach every limit at and one past it with small numbers; the 10 000 000-event case at the real constant is one `#[ignore]` test run by hand. Memory is bounded by the limits, not by 256 MiB at the real constants (F5): a hostile 64 MiB file may still allocate about 1 GB during import (backlog row).
18. **The export sheet comes before the Save panel** (spec §9 says after, but its "What won't transfer … before the user clicks Export" only works sheet-first; F16b). The dry-run check ignores `path` and `replace` (F2).

## Pre-flight amendments (2026-11-11)

From `.superpowers/sdd/2026-11-11-smf-midi-clip/preflight-report.md`; the controller accepted every proposed ruling. Each is applied in place in the task it touches; where a step and an older line disagree, the amendment wins.

- **F1** `Cargo.lock` also changes in Task 7 (core-ffi gains `core-io`); Global Constraints and Task 7's files say so.
- **F2** The export checks and `dry_run` ignore `path`/`replace`; `dry_run` is handled before the missing-path dialog (Tasks 7, 8; Ruling 18).
- **F3** Task 1 tests `ModelError::OutOfRange { field, .. }`; `info_field_name` gives `&'static str` names.
- **F4** The unreachable `u64::MAX` tick refusals and their tests are dropped (Tasks 3, 4); `checked_add` stays.
- **F5** Readers take `&Limits` (Ruling 17); tests use small limits; the 10 M-event case is one `#[ignore]` test; memory is "bounded by the limits".
- **F6** Two goldens are hand-specified before the code (Task 6 `small_goldens_match`); a missing golden fails unless `VIZDAW_BLESS=1`.
- **F7** The round-trip generator's constraints are stated, and the lossy cases are fixed tests asserting their reports (Task 6).
- **F8** The Clip header holds only tempo and meter; key goes in the sequence at tick 0; in-range changes are written; the title is the composition name (Tasks 4, 5, 6).
- **F9** `clip::read` assembles SysEx and text and applies limits once (`ClipEvent`); one `text::sanitize`; a clip at another `tpq` is tested (Tasks 2, 4, 5).
- **F10** `to_smf` counts what `flatten` hides: muted notes and events outside the sequence (Task 6).
- **F11** Meta-only SMF tracks make no track; the e2e counts MIDI tracks (Tasks 5, 10).
- **F12** The synchronous info and check calls run off the main actor with a generation counter (Tasks 7, 9); the double parse is a backlog row.
- **F13** `reportCommandNeverOpensTheSheet` moves to Task 9; Task 8 keeps a session-only check.
- **F14** `org.vizdaw.midi-clip` is declared under `UTExportedTypeDeclarations` (Task 10).
- **F15** `core-control/tests/names.rs` scans `core-io/src` (Task 2).
- **F16** Tempo 0, a denominator power of 100 and an unterminated SysEx get cases; velocity 0 is asserted; every §6.5 row is tested at the limit and one past (Tasks 2–5); the sheet-before-panel order is Ruling 18.
- **F17** Test-count estimates stay approximate; report measured counts.
- **F18** The branch SHA is updated; `Panels` lives in `AppCommands.swift`.
- **F19** Fixture paths are in each task's Files and commit; `smf::write` refuses out-of-order ticks (Tasks 3, 4, 6).
- **F20** Task 4 Step 0's fallback must name unchecked rows; a Clip File owner check and an interop backlog row are added (Task 10).
- **F21** One text and meta mapping table (Task 5) governs import and export.
- **F22** Ruling 10 derives from `midi1::Class`; one `check_info` serves the command and `validate`; the `passes.rs` mapping and `PendingFile` duplication are backlog rows.
- **F23** A stale `.partial` is refused naming it; a save during an open edit skips the project-ID stamp (Ruling 16; Tasks 2, 7).
- **F24** Negative drop ticks clamp to 0; a partial `channel_map` is filled from suggestions; `start`/`end` apply to every chosen track; drop-track `dest` overrides (Tasks 5, 7).
- **F25** Commit trailers use the executing session's own `Co-Authored-By` line (Global Constraints).

## File map

| File | What changes |
|---|---|
| `Cargo.toml` (workspace) | nothing (members are `crates/*`) |
| `crates/core-model/src/document.rs` | `SongInfo`, `Settings.info`, `Settings.project_id`, `Document::with_project_id` |
| `crates/core-model/src/event.rs` | `EventBody::AudioRegion.name` |
| `crates/core-model/src/command/settings.rs`, `command/audio.rs` | `SettingsCmd::SetInfo`, `AudioCmd::SetName` |
| `crates/core-model/src/rules.rs`, `file.rs`, `history.rs` | checks; `VERSION` 8; `History::stamp_project_id` |
| every `EventBody::AudioRegion { … }` site (67, listed by `grep -rn "AudioRegion {" crates`) | `name: None` in constructions, `..` in patterns |
| `crates/core-media/src/import.rs` | `rename_excl` becomes `pub` |
| `crates/core-io/` (new) | `limits`, `read`, `write`, `report`, `text`, `smf`, `ump`, `clip`, `midi_in`, `midi_out`, `gm`; tests and `tests/data/` (hand-written fixtures and goldens, each described in `tests/data/README.md`) |
| `crates/core-control/tests/names.rs` | scans `core-io/src` too |
| `crates/core-ffi/src/interchange.rs` (new), `jobs.rs`, `session.rs`, `events.rs`, `edit.rs`, `control.rs`, `bounce.rs`, `project_id.rs` (new), `lib.rs`; `Cargo.toml`, `Cargo.lock` | rows, jobs, session methods |
| `crates/core-control/src/registry.rs` | four commands |
| `app/Sources/VizDAWAppModel/Interchange.swift`, `InterchangeCommands.swift`, `ProjectSettingsModel.swift` (new); `ControlCommands.swift`, `ProjectMedia.swift`, `DocumentModel.swift`, `DocumentStore.swift`, `PaneContent.swift` | models and handlers |
| `app/Sources/VizDAW/MidiSheets.swift`, `ReportSheet.swift`, `ProjectSettingsSheet.swift` (new); `DocumentView.swift`, `AppCommands.swift`, `ControlChannel.swift`, `PaneViews.swift`, `VizDAWApp.swift`; `app/Sources/VizDAWEditors/AudioDrop.swift`, `ArrangementView.swift` | views, menus, drops |
| `scripts/make-app.sh` | document types for `.mid` and `.midi2` |
| `app/README.md`, `docs/feature-backlog.md` | checks 257–266; rows |
| `crates/vizctl/tests/e2e.rs` | `midi_files_round_trip_over_the_channel` |

---

### Task 1: Format version 8 (`core-model`)

**Files:**
- Modify: `crates/core-model/src/document.rs`, `event.rs`, `command/settings.rs`, `command/audio.rs`, `rules.rs`, `file.rs`, `history.rs`; every `EventBody::AudioRegion { … }` site in `crates/` (constructions get `name: None`; exhaustive patterns get `..`); `crates/core-ffi/src/events.rs`, `edit.rs` (the two UniFFI items); the five `assert_eq!(VERSION, 7)` sites (`tests/input_fx.rs:262`, `tests/mpe.rs:267`, `tests/file.rs:656,674`, `tests/learn.rs:211`) become 8; the doc line in `file.rs:3`
- Test: `crates/core-model/tests/song_info.rs` (new), `crates/core-model/tests/file.rs`, `crates/core-ffi/tests/regions.rs`

**Interfaces:**
- Produces:
  - `pub struct SongInfo { pub title, artist, album, original_artist, composer, songwriter, producer, arranger, year, genre, copyright, website, comment: Option<String> }` (13 fields, in that order), `Default`, `Clone`, `PartialEq`, serde with `#[serde(default, deny_unknown_fields)]` and each field `skip_serializing_if = "Option::is_none"`; `SongInfo::is_empty(&self) -> bool`.
  - `pub const MAX_INFO_BYTES: usize = 4096` (per field and for a region name).
  - `Settings.info: SongInfo` (`#[serde(default, skip_serializing_if = "SongInfo::is_empty")]`) and `Settings.project_id: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`).
  - `pub fn check_project_id(s: &str) -> bool`: 36 characters, lowercase hex in 8-4-4-4-12 groups.
  - `EventBody::AudioRegion.name: Option<String>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`; `AudioCmd::Split` gives both parts the name.
  - `SettingsCmd::SetInfo { info: SongInfo }` named "Set Song Info"; `AudioCmd::SetName { event: EventId, name: Option<String> }` named "Rename Region".
  - `Document::with_project_id(self, id: &str) -> Document` (no-op when one is set) and `History::stamp_project_id(&mut self, id: &str) -> Result<()>` (refused inside a transaction; applies `with_project_id` to the present and every undo and redo version).
  - `file::migrate_v7_to_v8` (identity), appended to `MIGRATIONS`; `VERSION = 8`.
  - FFI: `EventRow.region_name: Option<String>` (`#[uniffi(default = None)]`), `AudioEdit::SetName { event: u64, name: Option<String> }`.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it in the task report.
- [ ] **Step 2: Write the failing tests.**
  - `tests/song_info.rs`:
    - `set_info_round_trips_and_undoes`: `SetInfo` with title "Song", composer "A. Writer" and comment "line 1\nline 2" stores them; `History` undo restores `SongInfo::default()`; the name is "Set Song Info".
    - `set_info_checks_each_field`: a field of `MAX_INFO_BYTES + 1` bytes is `ModelError::OutOfRange { field: "info.title", .. }` (built with `ModelError::range`; F3), and exactly `MAX_INFO_BYTES` bytes is accepted; a field holding `\u{0007}` is refused naming its field; `"\n"` and `"\t"` are accepted; `Some("")` and `Some("  ")` store `None`; each refusal leaves the document equal.
    - `region_name_set_rename_and_split`: `SetName` with `Some(" Lead vox ")` stores `"Lead vox"`; `Some("")` stores `None`; a name failing `lane::valid_name` (a zero-width space alone) is refused as `OutOfRange { field: "region.name", .. }`; a name of `MAX_INFO_BYTES + 1` bytes is refused; `SetName` on a note is refused; `Split` of a named region gives both parts the name; undo restores.
    - `project_id_is_stamped_through_history`: after two edits, `stamp_project_id("0a1b2c3d-0000-4000-8000-00000000000a")` sets it; `undo` twice and `redo` once each keep it; a second stamp with another ID changes nothing; inside `begin` it is refused.
    - `an_empty_song_info_and_unnamed_region_write_nothing`: `to_json` of a document with defaults has no `"info"`, no `"project_id"` and no `"name"` key inside a region.
  - `tests/file.rs`:
    - `version_7_projects_migrate_to_version_8`, in the style of `version_6_projects_migrate_to_version_7`, with `assert_eq!(VERSION, 8)` and `migrate_v7_to_v8`.
    - `damaged_v8_fields_are_corrupt`: a `project_id` of `"xyz"`, an `info.title` with a control character and a region `name` of `""` each fail to load as `Corrupt`, naming `project_id`, `info.title` and `region.name`.
  - `crates/core-ffi/tests/regions.rs`: `set_name_reaches_event_rows`: `edit_audio(AudioEdit::SetName { event, name: Some("Take 3") })` then the lane's `EventRow.region_name == Some("Take 3")`; undo gives `None`.
- [ ] **Step 3: Run** `cargo test -p core-model --test song_info --test file` and `cargo test -p core-ffi --test regions`. Expected: fail to compile.
- [ ] **Step 4: Implement.**
  - One `pub fn check_info(info: &SongInfo) -> Result<()>` (with `fn info_field_name(i: usize) -> &'static str`, a `match` over the 13 names, since the error's field is `&'static str`; F3) serves both `SetInfo` and `rules::validate` (F22b). `SetInfo` normalises (trim; empty → `None`), then calls it: at most `MAX_INFO_BYTES` bytes and no control characters except `\n` and `\t`; the error field is `info.<field>`.
  - `SetName` follows `AudioCmd::SetGain` (`command/audio.rs`): `valid_name`, `MAX_INFO_BYTES`, regions only.
  - `rules::validate` checks `project_id` with `check_project_id`, `info` with `check_info` and every region `name` (the same rules), so a damaged file is Corrupt naming the field. `check_event_fields` checks a region name.
  - `migrate_v7_to_v8` with a doc line in `migrate_v6_to_v7`'s style: settings gain optional `info` and `project_id`, regions an optional `name`; the bump makes an older build say NewerFormat.
  - The UniFFI items map `region_name` from the region body and `SetName` to `AudioCmd::SetName` beside `SetGain` (`edit.rs:1262`).
- [ ] **Step 5: Run** `cargo test --workspace`, the rt-check gate, `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Expected: PASS (about +8 Rust tests; Swift unchanged apart from the defaulted field and the new enum case: add a `.setName` arm wherever a Swift `switch` over `AudioEdit` must be exhaustive).
- [ ] **Step 6: Commit** `feat(model): format version 8 with song info, region names and project ID (plan 6a Task 1)` with every file above.

### Task 2: The `core-io` crate: limits, guarded reading, partial writes, the report

**Files:**
- Create: `crates/core-io/Cargo.toml`, `src/lib.rs`, `src/limits.rs`, `src/read.rs`, `src/write.rs`, `src/report.rs`, `src/text.rs`
- Modify: `crates/core-media/src/import.rs` (`rename_excl` → `pub`, both cfg variants), `crates/core-ffi/tests/wording.rs` (scan `core-io`), `crates/core-control/tests/names.rs` (add `core-io/src` to its directory list beside `core-ffi/src` and `core-surface/src`; F15), `Cargo.lock`
- Test: `crates/core-io/tests/base.rs`

**Interfaces:**
- Produces:
  - `Cargo.toml`: package `core-io`, workspace edition/licence/rust-version; dependencies `core-model`, `core-media`, `core-engine` (paths), `im = { version = "15", features = ["serde"] }`, `thiserror = "2"`; dev `proptest = "1"`, `tempfile = "3"`.
  - `pub enum IoError { Refused(String), Io(String), Cancelled }` (`thiserror`; `Display` is the sentence alone; `From<std::io::Error>` → `Io`).
  - `limits.rs`, each `pub const` with a doc line citing §6.5:
    ```rust
    pub const MAX_MIDI_FILE_BYTES: u64 = 64 * 1024 * 1024;
    pub const MAX_VLQ_BYTES: usize = 4;
    pub const MAX_EVENTS: usize = 10_000_000;     // per file, all tracks
    pub const MAX_SYSEX_BYTES: usize = 1024 * 1024;
    pub const MAX_TRACKS: usize = 4096;            // MTrk chunks read; more is refused
    pub const MAX_TEXT_BYTES: usize = 64 * 1024;   // one meta or Flex Data text
    pub const MAX_DETAILS: usize = 50;             // report details per item
    ```
    and `pub struct Limits { pub max_events: usize, pub max_sysex_bytes: usize, pub max_tracks: usize, pub max_text_bytes: usize }` with `Default` = the constants (Ruling 17, F5). Every reader takes `&Limits`.
  - `text.rs`: `pub fn sanitize(bytes: &[u8], limits: &Limits) -> (String, bool)` (Ruling 9; the bool is "cut"), used by both the SMF and the Clip File paths (F9).
  - `read.rs`: `pub fn read_regular(path: &Path, max: u64) -> Result<Vec<u8>, IoError>`: `fs::metadata` (follows a symlink once; a loop is the OS error, refused), must be `is_file()` (FIFO, device and directory refused *before* opening), `len() <= max`, then reads with `take(max + 1)` and refuses if it got more.
  - `write.rs`:
    - `pub struct PendingFile` (private: partial path, destination, replace flag, committed);
    - `pub fn write_partial(dest: &Path, bytes: &[u8], replace: bool) -> Result<PendingFile, IoError>`: refuses an existing `dest` without `replace` ("<name> exists; choose another name or replace it", as `core_media::export` words it), writes `core_media::export::partial_path(dest)` with `create_new`; a partial left by a crashed run is refused with "<.name.partial> is left from an earlier export; remove it or choose another name" (F23);
    - `PendingFile::commit(self) -> Result<PathBuf, IoError>`: `rename` with `replace`, else `core_media::import::rename_excl`;
    - `Drop` removes an uncommitted partial.
  - `report.rs`:
    - `pub enum ItemKind { Changed, LeftOut, Note }`;
    - `pub struct Item { pub kind: ItemKind, pub what: String, pub count: u64, pub details: Vec<String> }`;
    - `pub struct Summary { pub tracks: u64, pub regions: u64, pub notes: u64, pub markers: u64, pub automation_points: u64, pub plugins: u64, pub tempo_map: bool }` (`Default`);
    - `pub struct Report { pub summary: Summary, pub items: Vec<Item> }` with `add(&mut self, kind: ItemKind, what: &str, detail: Option<String>)` (groups by `(kind, what)`, counts every call, keeps the first `MAX_DETAILS` details), `merge(&mut self, other: Report)`, `is_empty(&self) -> bool` (no items) and `summary_line(&self, verb: &str) -> String`.

- [ ] **Step 1: Write the failing tests** in `tests/base.rs`:
  - `read_regular_refuses_non_files`: a directory, a FIFO (`libc`-free: `std::process::Command::new("mkfifo")` in the temp dir, skipped with a printed note if `mkfifo` is missing), `/dev/zero`, a symlink loop (`a → b → a`), a missing path and a file of `max + 1` bytes are each `Err(Refused | Io)` and return within 1 s; a file of exactly `max` bytes and a symlink to a regular file read their bytes.
  - `partial_files_are_removed_unless_committed`: dropping a `PendingFile` leaves no `.x.mid.partial` and no `x.mid`; `commit` leaves only `x.mid`; an existing `x.mid` without `replace` is refused before anything is written; with `replace` it is overwritten; a stale `.x.mid.partial` is refused naming it.
  - `sanitize_cuts_and_cleans`: bytes `"A\u{0007}B\nC"` give `("AB\nC", false)`; invalid UTF-8 is replaced; with `max_text_bytes: 4`, exactly 4 bytes are kept uncut and 5 are cut to 4 (`true`), never inside a character.
  - `report_groups_and_caps_details`: 120 `add(LeftOut, "Muted notes", Some(format!("bar {k}")))` give one item with `count == 120` and 50 details; `summary_line("Imported")` for `tracks 12, notes 4210, markers 8, tempo_map` is exactly `"Imported 12 tracks, 4 210 notes, the tempo map and 8 markers"` (thousands grouped with a space; zero counts omitted; "1 track"/"1 note" singular; the order tracks, regions, notes, the tempo map, markers, automation points, plug-ins); an empty summary gives `"<verb> nothing"`.
- [ ] **Step 2: Run** `cargo test -p core-io --test base`. Expected: fail to compile.
- [ ] **Step 3: Implement** the items above, and add `"core-io"` to the crate list `wording.rs` scans (beside `core-ffi`, line 253 onward).
- [ ] **Step 4: Run** `cargo test -p core-io`, `cargo test -p core-media`, `cargo test -p core-ffi --test wording`, `cargo test -p core-control --test names`, both clippies. Expected: PASS (+4).
- [ ] **Step 5: Commit** `feat(io): core-io crate with limits, guarded reads, partial writes and the report (plan 6a Task 2)` with `crates/core-io/`, `crates/core-media/src/import.rs`, `crates/core-ffi/tests/wording.rs`, `crates/core-control/tests/names.rs`, `Cargo.lock`.

### Task 3: Standard MIDI File reader and writer (bytes)

**Files:**
- Create: `crates/core-io/src/smf.rs`
- Test: `crates/core-io/tests/smf_bytes.rs`, `crates/core-io/tests/smf_fuzz.rs`, `crates/core-io/tests/data/README.md` (how every fixture was made; all are ours, GPL-3.0), fixtures `crates/core-io/tests/data/golden_type1.mid`, `type0_running_status.mid`, `type1_three_tracks.mid` (F19)

**Interfaces:**
- Consumes: Task 2; `core_engine::midi1::Midi1Msg`.
- Produces:
  - `pub enum Division { Ppq(u16), Smpte { fps: u8, ticks_per_frame: u8 } }`;
  - `pub enum SmfEvent { Channel(Midi1Msg), SysEx(Vec<u8>) /* payload without F0/F7, reassembled */, Escape(Vec<u8>), Meta { kind: u8, data: Vec<u8> } }`;
  - `pub struct SmfTrack { pub events: Vec<(u64, SmfEvent)> }` (absolute ticks in file units, in file order);
  - `pub struct SmfFile { pub format: u16, pub division: Division, pub tracks: Vec<SmfTrack> }`;
  - `pub fn read(bytes: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<(SmfFile, Report), IoError>`;
  - `pub fn write(f: &SmfFile) -> Result<Vec<u8>, IoError>` (refuses a track longer than `u32::MAX` bytes, a delta over `0x0FFF_FFFF`, and events out of tick order; F19).

- [ ] **Step 1: Write the failing tests.**
  - `smf_bytes.rs`:
    - `reads_and_writes_the_golden`: `write` of type 1, `Ppq(960)`, conductor `[(0, Meta 0x03 "Song"), (0, Meta 0x51 [07 A1 20]), (0, Meta 0x58 [04 02 18 08]), (0, Meta 0x59 [00 00]), (0, Meta 0x2F [])]` and track `[(0, Meta 0x03 "Bass"), (0, 90 3C 64), (960, 80 3C 00), (960, Meta 0x2F [])]` equals the bytes
      `4D546864 00000006 0001 0002 03C0`, `4D54726B 00000021` + `00FF0304536F6E67 00FF510307A120 00FF580404021808 00FF59020000 00FF2F00`, `4D54726B 00000015` + `00FF030442617373 00903C64 8740803C00 00FF2F00`;
      `read` of those bytes gives the same `SmfFile` and an empty report.
    - `running_status_and_note_on_zero`: `90 3C 64 00 3E 64` reads as two note-ons; `00 3C 00` after them reads as `Channel(90 3C 00)` (the reader keeps it; Task 5 makes it a note-off; F16b); a meta or SysEx between cancels running status (a data byte after it refuses the file).
    - `sysex_packets_reassemble`: `F0 03 01 02 03` without `F7` continued by `F7 02 04 F7` gives one `SysEx([1,2,3,4])`; a lone `F7 02 90 3C` is `Escape`.
    - `unknown_chunks_are_skipped`: an `XFIH` chunk between tracks is skipped and reported as Note "Unknown file sections skipped".
    - `hostile_smf_is_refused_or_reported` (one case each, the message checked by substring):
      - header shorter than 14 bytes, wrong magic, header length < 6 → Refused "This isn't a MIDI file.";
      - type 2 → Refused "This is a type 2 MIDI file; VizDAW reads types 0 and 1.";
      - a chunk length past the end → Refused "…track 2 ends in the middle of an event." (the track number is 1-based);
      - an event cut off at the end of its chunk, a 5-byte VLQ, a data byte with no status → Refused naming the track; a 4-byte VLQ (`FF FF FF 7F`) reads;
      - a SysEx declaring 2³⁰ bytes → Refused (length past the chunk) with no allocation of that size (the test runs under 1 s);
      - with `Limits { max_sysex_bytes: 16, max_tracks: 8, max_events: 100, .. }` (F5, F16c): a SysEx of 16 bytes reads and one of 17 is Left out "SysEx messages over 1 MiB" (the text names the real limit); 8 `MTrk` chunks read and 9 are Refused "…more than 8 tracks."; 100 events read and 101 are Refused "…more than 100 events.";
      - an SysEx `F0` packet never terminated by an `F7` packet before the end of its track → Left out "Unfinished SysEx messages" (F16a);
      - `#[ignore]` `ten_million_events_are_refused_at_the_real_limit`: `MAX_EVENTS + 1` running-status note-ons with `Limits::default()` → Refused "…more than 10 000 000 events." (run by hand);
      - a type 0 file with two tracks → read, with Changed "Type 0 file with 2 tracks";
      - a declared track count larger than the chunks present → read, with Changed "The file declares 5 tracks and holds 2";
      - a missing End of Track → Note.
    - `zero_division_is_refused`: `Ppq(0)` and SMPTE with 0 ticks per frame or an fps not in {24, 25, 29, 30} → Refused "This MIDI file's timing can't be read.".
    - `cancel_stops_the_reader`: with `cancel` set, `read` returns `Err(Cancelled)`.
  - `smf_fuzz.rs` (proptest, cases from `VIZDAW_FUZZ_CASES` else 256, as `core-ffi/tests/damaged_files.rs`'s `cases`):
    - `smf_fuzz_never_panics` (every case with `Limits { max_events: 100_000, .. }`): byte damage (flip, insert, delete, truncate) on every fixture in `tests/data/*.mid` (the golden above and two more you write by hand: a type 0 with running status and SysEx, a type 1 with three tracks, tempo changes, a marker and lyrics; Task 6's goldens join the glob later), and a "chunk soup" generator (random `MThd`/`MTrk`/junk chunks with random lengths and random event bytes). Each `read` returns `Ok` or `Err(Refused)` within 2 s; `write` of every `Ok` result reads back equal.
- [ ] **Step 2: Run** `cargo test -p core-io --test smf_bytes --test smf_fuzz`. Expected: FAIL.
- [ ] **Step 3: Implement** `smf.rs`. The reader walks chunks by declared length (`checked_add` against `bytes.len()` before slicing), never reserves capacity from a declared length (grow by push; a SysEx `Vec` is filled only from bytes present), counts events against `limits.max_events`, accumulates ticks with `checked_add` (the lint requires it; the sum cannot reach `u64::MAX` within the limits, so there is no test for it; F4), and checks `cancel` every 65 536 events. Value-level damage (Ruling 3) is left to Task 5, except SysEx length and termination. The writer writes VLQs of at most 4 bytes (`write` refuses a delta over `0x0FFF_FFFF`; `midi_out` never produces one) and never uses running status.
- [ ] **Step 4: Run** the tests again and both clippies. Expected: PASS (about +8, one ignored).
- [ ] **Step 5: Commit** `feat(io): Standard MIDI File reader and writer with hostile-input limits (plan 6a Task 3)` with `crates/core-io/src/smf.rs`, `crates/core-io/src/lib.rs`, `crates/core-io/tests/smf_bytes.rs`, `crates/core-io/tests/smf_fuzz.rs`, `crates/core-io/tests/data/`.

### Task 4: UMP codec and the MIDI Clip File (bytes)

**Files:**
- Create: `crates/core-io/src/ump.rs`, `crates/core-io/src/clip.rs`
- Test: `crates/core-io/tests/clip_bytes.rs`, `crates/core-io/tests/clip_fuzz.rs`, fixtures `crates/core-io/tests/data/golden_minimal.midi2` and `every_kind.midi2`, with their entries in `tests/data/README.md` (F19)

**Interfaces:**
- Consumes: Task 2.
- Produces:
  - `pub struct Packet { words: [u32; 4], len: u8 }` with `Packet::new(words: &[u32]) -> Option<Packet>` (length must match the type), `words(&self) -> &[u32]`, `mt(&self) -> u8`, `group(&self) -> u8`; `pub fn packet_words(mt: u8) -> usize` (pinned table below).
  - `pub fn read_packets(bytes: &[u8]) -> Result<Vec<Packet>, IoError>` (big-endian words; a packet cut off at the end refuses) and `pub fn write_packets(p: &[Packet], out: &mut Vec<u8>)`.
  - Builders and decoders for the messages below, e.g. `ump::dcs(ticks: u32) -> Packet`, `ump::note_on(group, ch, pitch, vel16, attr: Option<NoteAttribute>) -> Packet`, `ump::set_tempo(ten_ns_per_quarter: u32) -> Packet`, `ump::text(bank, status, group, &str) -> Vec<Packet>`, and `pub enum Decoded { … }` from `ump::decode(&Packet) -> Decoded` (one variant per row of the table, plus `Unknown`).
  - `pub enum ClipEvent { Packet(Packet), SysEx7 { group: u8, data: Vec<u8> }, SysEx8 { group: u8, data: Vec<u8> }, Text { group: u8, bank: u8, status: u8, text: String } }`: `clip::read` assembles multi-packet SysEx and Flex Data text and applies the SysEx and text limits once (via `text::sanitize`); `midi_in` only maps (F9). `clip::write` splits them again.
  - `pub struct ClipFile { pub tpq: u16, pub header: Vec<ClipEvent>, pub events: Vec<(u64, ClipEvent)>, pub end: u64 }`: `header` holds the timed configuration messages after DCTPQ (at most one Set Tempo and one Set Time Signature, tempo first; F8), `events` the sequence data with ticks counted from Start of Clip (bar 1), Start and End of Clip excluded; `end` is the End of Clip tick.
  - `pub fn clip::read(bytes: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<(ClipFile, Report), IoError>` and `pub fn clip::write(c: &ClipFile) -> Vec<u8>`.

**The format, as this plan pins it.** The file layout is checked against *M2-116-U MIDI Clip File Specification v1.0* (11 May 2023): an 8-byte header `SMF2CLIP` (53 4D 46 32 43 4C 49 50); then the Clip Configuration Header: optional Set Profile On messages with no delta clockstamp, then `DCS 0` + `DCTPQ`, then optional timed messages each preceded by a DCS (at most one Set Tempo and one Set Time Signature, tempo first); then the Clip Sequence Data: `DCS` + Start of Clip (bar 1), timed messages, `DCS` + End of Clip, and nothing after it. All data big-endian. A DCS precedes every timed message (messages may share one DCS). DCS counts at most 1 048 575 ticks; a longer gap is written as `DCS 0xFFFFF` + NOOP, repeated. Extension `.midi2`. The UMP encodings come from *M2-104-UM UMP Format and MIDI 2.0 Protocol v1.1* and were **written from memory**:

| Message | Word 0 (bits) | Further words |
|---|---|---|
| Packet size by MT | 0x0,0x1,0x2,0x6,0x7: 1 word; 0x3,0x4,0x8,0x9,0xA: 2; 0xB,0xC: 3; 0x5,0xD,0xE,0xF: 4 | |
| NOOP | `0x0000_0000` | |
| DCTPQ | MT 0, status 0x3: `0x0030_0000 \| tpq` | |
| DCS | MT 0, status 0x4: `0x0040_0000 \| ticks` (20 bits) | |
| MIDI 1.0 channel voice | MT 0x2, group, status/ch, data1, data2 | |
| SysEx 7 | MT 0x3, group, status (0 complete, 1 start, 2 continue, 3 end) and byte count (0–6) | 6 data bytes across words 0–1 |
| MIDI 2.0 channel voice | MT 0x4, group, opcode/ch, index fields | word 1: data |
| — Note Off 0x8 / On 0x9 | note, attribute type | word 1: velocity (16) \| attribute data (16) |
| — Poly pressure 0xA, CC 0xB | note or index | word 1: 32-bit value |
| — Program 0xC | option flags (bit 0: bank valid) | word 1: program (bits 31–24), bank MSB (15–8), LSB (7–0) |
| — Channel pressure 0xD, pitch bend 0xE | | word 1: 32-bit value |
| — Per-note registered 0x0, assignable 0x1 | note, controller index | word 1: 32-bit value |
| — Per-note pitch bend 0x6 | note | word 1: 32-bit value |
| SysEx 8 | MT 0x5, group, status/count, stream ID | 13 bytes |
| Flex Data | MT 0xD, group, form (2), address (2: 0 channel, 1 group), channel, status bank, status | 12 bytes of payload |
| — Set Tempo | bank 0x00, status 0x00 | word 1: 10-ns units per quarter note |
| — Set Time Signature | bank 0x00, status 0x01 | word 1: numerator (8), denominator as a power of 2 (8), 1/32 notes per beat (8, 0 = unknown) |
| — Set Key Signature | bank 0x00, status 0x05 | word 1: sharps/flats (signed 4 bits) \| tonic (4 bits, A = 1 … G = 7) |
| — Metadata text | bank 0x01: 0x00 unknown, 0x01 project name, 0x02 composition name, 0x03 MIDI clip name, 0x04 copyright | text, UTF-8, form 0 complete / 1 start / 2 continue / 3 end |
| — Performance text | bank 0x02: 0x01 lyrics | as above |
| Start of Clip | MT 0xF, status 0x20: `0xF020_0000` | three zero words |
| End of Clip | MT 0xF, status 0x21: `0xF021_0000` | three zero words |

- [ ] **Step 0: Check the pinned encodings.** Fetch M2-104-UM v1.1 (the MIDI Association's site, or `https://amei-music.github.io/midi2.0-docs/`). If it is reachable, compare every row above, correct the constants in this plan's code where they differ, and list each correction in the task report. If it is not reachable (the fallback is allowed; F20), keep the table, write `// Pinned from memory by plan 6a; not checked against M2-104-UM` above the table's constants in `ump.rs`, and name in the report each row not checked (at least the time-signature denominator and the key-signature packing, the least certain). Key signature's minor mode: write the tonic of the relative minor when `minor` (A minor = 0 sharps, tonic A); on read, `minor` is true when the tonic is the relative minor's.
- [ ] **Step 1: Write the failing tests.**
  - `clip_bytes.rs`:
    - `writes_the_minimal_golden`: a clip with `tpq 15360`, header `[set_tempo(50_000_000), time_sig(4,2,8)]`, events `[(0, note_on(0,0,60,0x8000,None)), (15360, note_off(0,0,60,0,None))]`, `end 15360` writes exactly: `SMF2CLIP`, `00400000 00303C00`, `00400000` + tempo packet, `00400000` + time signature packet, `00400000 F0200000 00000000 00000000 00000000`, `00400000` + note-on, `00403C00` + note-off, `00400000 F0210000 00000000 00000000 00000000`; `read` gives it back with an empty report.
    - `long_gaps_restart_with_noop`: an event at tick 2 100 000 writes `DCS 0xFFFFF, NOOP, DCS 0xFFFFF, NOOP, DCS 0x00B22` (2 850, the remainder) before the event and reads back at 2 100 000.
    - `text_spans_packets`: a 40-byte clip name writes as start/continue/end packets and reads back as one `ClipEvent::Text`.
    - `header_holds_at_most_tempo_and_meter`: a second Set Time Signature in the header is moved to the sequence data at tick 0 with Changed (F8).
    - `hostile_clip_is_refused_or_reported`:
      - wrong magic → Refused "This isn't a MIDI Clip File.";
      - no DCTPQ, DCTPQ 0, no Start of Clip, a packet cut off at the end, a second Start of Clip → Refused naming what is missing;
      - data after End of Clip → Changed "Data after the end of the clip was ignored";
      - no End of Clip → Changed, the clip ends at its last event;
      - unknown message types (MT 0x6, 0x7, 0x8, 0x9, 0xA, 0xB, 0xC, 0xE) are skipped and counted: Left out "Unknown MIDI messages";
      - with `Limits { max_events: 100, max_sysex_bytes: 16, max_text_bytes: 8, .. }` (F5, F16c): 100 timed packets read and 101 are Refused; a SysEx 7 of 16 bytes reads and of 17 is Left out; a SysEx 7 start with no end is Left out "Unfinished SysEx messages"; a text of 8 bytes is kept and 9 is cut (Changed).
      (No `u64::MAX` tick test: DCS deltas within the limits cannot reach it; `checked_add` stays; F4.)
    - `ump_round_trips`: proptest over every builder in the table with random fields: `decode(build(x)) == x`.
  - `clip_fuzz.rs`: `clip_fuzz_never_panics` (with `Limits { max_events: 100_000, .. }`): byte damage on the goldens in `tests/data/*.midi2` (the minimal one and one you write with every message kind), and a "UMP soup" generator (header + random packets of random MTs, with valid and invalid sizes). `Ok` or `Err(Refused)` within 2 s; `write` of each `Ok` reads back equal.
- [ ] **Step 2: Run** `cargo test -p core-io --test clip_bytes --test clip_fuzz`. Expected: FAIL.
- [ ] **Step 3: Implement** `ump.rs` and `clip.rs`. Set Profile On messages (any packet before DCTPQ without a DCS) are skipped with a Note "Profile settings in the clip were not used".
- [ ] **Step 4: Run** the tests and both clippies. Expected: PASS (about +7).
- [ ] **Step 5: Commit** `feat(io): UMP codec and MIDI Clip File reader and writer (plan 6a Task 4)` with `crates/core-io/src/ump.rs`, `src/clip.rs`, `src/lib.rs`, `tests/clip_bytes.rs`, `tests/clip_fuzz.rs`, `tests/data/`.

### Task 5: MIDI import: file → tracks → the document

**Files:**
- Create: `crates/core-io/src/midi_in.rs`, `crates/core-io/src/gm.rs`
- Test: `crates/core-io/tests/midi_in.rs`

**Interfaces:**
- Consumes: Tasks 3–4; `core_engine::input_fx::{up7, up32}`, `core_engine::mpe::MpeDecoder`, `LiveMsg`; `core_model::history::History`, the commands named below.
- Produces:
  - `pub enum MidiFileKind { Smf0, Smf1, Clip }`;
  - `pub struct ImportTrack { pub name: String, pub main: Option<(u8, u8)>, pub events: Vec<EventSpec>, pub drum: bool, pub mpe: Option<MpeConfig> }`: MIDI events always carry `group_channel: Some((g, c))`; ticks at 15 360 per quarter from file position 0;
  - `pub struct MidiImport { pub kind: MidiFileKind, pub title: Option<String>, pub copyright: Option<String>, pub tempo: TempoMap, pub meter: MeterMap, pub has_tempo: bool, pub start_offset: Tick, pub markers: Vec<Marker>, pub tracks: Vec<ImportTrack>, pub report: Report }`;
  - `pub struct ImportOptions { pub split_channels: Option<bool>, pub mpe: bool }`;
  - `pub fn parse(bytes: &[u8], opts: &ImportOptions, limits: &Limits, cancel: &AtomicBool) -> Result<MidiImport, IoError>` (sniffs `MThd` → SMF, `SMF2CLIP` → Clip, else Refused "This isn't a MIDI file.");
  - `pub fn channels(imp: &MidiImport) -> BTreeMap<(u8, u8), u64>` (notes per used pair, pairs with no notes included at 0);
  - `pub enum DestChoice { Existing(DestId), NewBuiltin }` and `pub fn suggested_dest(doc: &Document, group: u8, channel: u8) -> DestChoice` (Ruling 5);
  - `pub struct Placement { pub seq: SeqId, pub lane: Option<LaneId>, pub tick: Tick, pub channel_map: BTreeMap<(u8, u8), DestChoice>, pub use_tempo: bool, pub new_project: bool }`;
  - `pub fn apply(h: &mut History, imp: &MidiImport, place: &Placement, report: &mut Report) -> Result<Vec<LaneId>, ModelError>`: runs the commands inside the caller's open transaction and fills `report.summary`. A negative `place.tick` is clamped to 0, as audio imports do; pairs missing from `channel_map` take `suggested_dest`; events placed on an existing drop track whose destination differs from their pair's carry the `dest` override (Ruling 5) (F24).
- **Text and meta mapping (F21)**, both directions (Task 6 writes by the same table):

  | Model | SMF meta | Clip File |
  |---|---|---|
  | sequence markers | `06` Marker (any track on import) | Metadata 0x00 `marker: …` |
  | `Text { kind: Text }` | `01` | Metadata 0x00 (no prefix) |
  | `Text { kind: Lyric }` | `05` | Performance 0x01 |
  | `Text { kind: Cue }` | `07` | Metadata 0x00 `cue: …` |
  | `Text { kind: Copyright }` | `02` on a track | Metadata 0x04 on a track |
  | `Text { kind: Marker }` | `06` on export (it plays as a marker elsewhere); import makes sequence markers only | Metadata 0x00 `marker: …` |
  | `SongInfo.copyright` | `02` on the conductor / first track | Metadata 0x04 at tick 0 before any note |
  | sequence name (title) | `03` on the conductor | Metadata 0x02 composition name (F8) |
  | track name | `03` on the track | Metadata 0x03 clip name |

  On import a `02` on the conductor (type 1 track 1, or type 0 at tick 0) goes to `MidiImport.copyright`; elsewhere it is a Copyright text event.
  - `gm::drum_name(note: u8) -> Option<&'static str>`: the General MIDI Level 1 percussion names for notes 35–81 ("Acoustic Bass Drum" … "Open Triangle").

- [ ] **Step 1: Write the failing tests** (fixtures built in the test with Task 3's and Task 4's writers; a `Document` with one sequence from `core_model`, as `tests/properties.rs`'s `seed` makes one):
  - `positions_scale_exactly_or_round`: PPQ 960 (×16) and 480 are exact with no report item; a Clip File at `tpq` 480 is exact and one at `tpq` 7 rounds with Changed "Positions rounded…" (F9); PPQ 1000 rounds to the nearest tick and reports Changed "Positions rounded to VizDAW's resolution" only when some position moved; SMPTE 25 fps × 40 ticks puts one second at tick 30 720 (two quarters at 120 BPM), Changed "Timecode-based file placed at 120 BPM".
  - `overlapping_notes_pair_first_on_first_off`: on, on, off, off on one pitch and channel give durations (first-on to first-off, second-on to second-off); `90 3C 00` ends a note; a note open at its track's end ends there (Changed "Notes with no end were ended at the end of their track"); an unmatched off is Left out "Note-offs with no note".
  - `values_scale_by_midi2_rules`: velocity 100 → `up7(100)`; CC 7 = 90 → `up32(90, 7)`; bend `E0 00 40` → `0x8000_0000`; channel and poly pressure likewise; `CC0 1, CC32 2, C0 05` at one tick → one `Program { program: 5, bank: Some((1, 2)) }`; RPN/NRPN CCs stay CCs.
  - `meta_events_map`: tempo `07 A1 20` → 120 BPM; a tempo above 999 or below 5 BPM is clamped with Changed; a tempo of `00 00 00` and a tempo meta of the wrong length are Left out "Unreadable tempo changes" (no division by zero; F16a); a time signature with denominator power 100 is Left out "Unreadable time signatures"; time and key signatures → meter entries (a change off a barline moves to the next barline, Changed); every row of the text table above maps as it says; track name → `ImportTrack.name`; port `FF 21 01 03` → group 3 for later events of that track; sequencer-specific and SMPTE offset → Left out.
  - `meta_only_tracks_make_no_track` (F11): a type 1 file whose first track holds only tempo, time signature, name and marker metas gives no `ImportTrack` for it (its name becomes `title`); a track of only lyrics becomes a track.
  - `type_0_splits_by_channel_and_type_1_does_not`: a type 0 on channels 1 and 10 gives two tracks, the channel-10 one `drum: true` named "Drums"; a type 1 track on channels 1 and 2 is one track with `main == Some((0, 0))` (more notes) unless `split_channels: Some(true)`.
  - `mpe_is_decoded`: a track that begins with the MCM (RPN 6 on channel 1, data entry 15) and member-channel notes with bend and CC 74 gives notes with `PerNoteKind::PitchBend` and `Registered(74)` entries, poly pressure events for member pressure, and `mpe == Some(lower zone, 15 members)`; the same bytes without an MCM decode only with `mpe: true`.
  - `clip_maps_everything_it_carries`: a clip with a 16-bit velocity, a note attribute, a 32-bit CC, per-note bend and registered controller 74 at offsets, a program with bank, a SysEx 8 and a clip name gives exactly those `EventSpec`s (per-note offsets from their note's start); a MIDI 1.0 packet note is translated up; a per-note message with no sounding note is Left out "Per-note messages with no note".
  - `huge_deltas_are_left_out_not_wrapped`: PPQ 1 with deltas of `0x0FFF_FFFF`: events whose tick passes `MAX_TICKS` (after placement too) are Left out "Events past the end of the timeline"; none wraps.
  - `channels_reuse_existing_destinations`: with a built-in synth destination on channel 2, `suggested_dest(doc, 0, 1)` is `Existing`, channel 1 and 10 are `NewBuiltin`; `apply` adds "Built-in Synth (ch 1)" and "Built-in Synth (ch 10)", sets each new track's `midi_dest`, and an event on a non-main channel carries the `dest` override.
  - `drum_tracks_get_gm_rows`: the channel-10 track is marked a drum lane and its rows for notes 36, 38 and 42 are named "Bass Drum 1", "Acoustic Snare" and "Closed Hi-Hat"; a note with no GM name keeps the row name `MarkDrumLane` gave it.
  - `importing_into_a_project_keeps_its_tempo_map`: into a sequence with a tempo ramp and events, at tick 8 bars: the maps are unchanged with `use_tempo: false` (Note "The file's tempo map was not used"); with `use_tempo: true` the file's maps replace them (Changed); markers are added at offset positions; title and copyright are Left out (Note).
  - `a_new_project_takes_the_file_whole`: `new_project: true` on an empty document sets the tempo, meter, `start_offset` (Ruling 8), the sequence name from the title and `SongInfo.copyright`; on a document with a lane it is `Err` ("A new-project import needs an empty project").
  - `a_single_track_lands_on_the_drop_track`: `place.lane = Some(l)` with a one-track import inserts into `l`; with two tracks they become new tracks after `l`; `place.tick = -500` places at 0; a `channel_map` naming only one of two pairs fills the other from `suggested_dest` (F24).
- [ ] **Step 2: Run** `cargo test -p core-io --test midi_in`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Tick scaling: `t × 15360 / ppq` with `checked_mul` and round-half-up division; SMPTE through seconds at 120 BPM.
  - `apply` uses, in order: `RoutingCmd::AddDestination` for each `NewBuiltin` (`DestKind::MidiEndpoint { endpoint_ref: BUILTIN_SYNTH_REF, group, channel }`); with `use_tempo` or `new_project`: `SequenceCmd::{SetTempo, SetMeter, SetStartOffset}`; `LaneCmd::Add` (named by the track), `LaneCmd::SetDest`, `EventCmd::Insert` (all events of the track in one command), `DrumCmd::MarkDrumLane` then `DrumCmd::RenameRow` per GM name, `LaneCmd::SetMpe`; `SequenceCmd::SetMarkers` with the merged list; with `new_project`: `SequenceCmd::Rename` and `SettingsCmd::SetInfo`.
  - MPE decoding feeds each channel message of the track through one `MpeDecoder` and maps `LiveMsg` to events as `core-ffi/src/passes.rs` does (bend → `PitchBend`, timbre → `Registered(74)`, pressure → `PolyPressure`), with per-note offsets from the note's start.
- [ ] **Step 4: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +13).
- [ ] **Step 5: Commit** `feat(io): MIDI file import mapping and apply (plan 6a Task 5)` with `crates/core-io/src/midi_in.rs`, `src/gm.rs`, `src/lib.rs`, `tests/midi_in.rs`.

### Task 6: MIDI export: the document → files; goldens and round trips

**Files:**
- Create: `crates/core-io/src/midi_out.rs`
- Test: `crates/core-io/tests/midi_out.rs`, `crates/core-io/tests/round_trip.rs`, goldens `crates/core-io/tests/data/export_small_type1_960.mid`, `export_small_clip.midi2` (hand-written, F6) and `export_type1_960.mid`, `export_type0_960.mid`, `export_type1_480.mid`, `export_type1_15360.mid`, `export_clip.midi2` (regression goldens), each described in `tests/data/README.md`

**Interfaces:**
- Consumes: Tasks 3–5; `core_model::query::flatten`; `core_engine::midi1::translate`; `core_engine::mpe::{MpeEncoder, configuration}`.
- Produces:
  - `pub struct SmfOptions { pub seq: SeqId, pub lanes: Vec<LaneId> /* empty: every track */, pub type0: bool, pub ppq: u16 /* 480, 960 or 15360 */, pub mpe: bool }`;
  - `pub fn to_smf(doc: &Document, o: &SmfOptions) -> Result<(SmfFile, Report), IoError>`;
  - `pub struct ClipOptions { pub seq: SeqId, pub lane: LaneId, pub range: Option<(Tick, Tick)> }`;
  - `pub fn to_clip(doc: &Document, o: &ClipOptions) -> Result<(ClipFile, Report), IoError>`;
  - both refuse an unknown sequence or lane, a lane of another sequence, a `ppq` outside the three, and an empty or reversed range, as `IoError::Refused`.

- [ ] **Step 1: Write the failing tests.**
  - `midi_out.rs`:
    - **Goldens (F6).** Every golden comparison fails when its file is missing; `VIZDAW_BLESS=1` (re)writes it and is never set in CI. Two goldens are written **by hand before the code**, from Ruling 10, the text table (Task 5) and the layouts pinned in Tasks 3–4, and committed with the failing test: `export_small_type1_960.mid` and `export_small_clip.midi2`, both of this document: one sequence "Song" at 120 BPM, 4/4, C major, no markers; one track "Bass" on a built-in synth destination on group 0 channel 1 with no patch and no mix; one note pitch 60, velocity `up7(100)`, release 0, tick 0, duration 15 360. The SMF golden is exactly Task 3's golden bytes (conductor "Song" with tempo, time and key signatures; track "Bass" with `00 90 3C 64`, `87 40 80 3C 00`). The clip golden is `SMF2CLIP`, `DCS 0` + DCTPQ 15360, `DCS 0` + Set Tempo 50 000 000, `DCS 0` + Set Time Signature 4/4, `DCS 0` + Start of Clip, `DCS 0` + composition name "Song", clip name "Bass", Set Key Signature (0, C), note on (60, velocity 0xC924 = `up7(100)`), then `DCS 0x3C00` + note off (60, 0), `DCS 0` + End of Clip; the writer puts its own `DCS` before every message (it never shares one), so each item above after Start of Clip has a `DCS 0` too. The larger goldens below are regression goldens: the implementer blesses them once, then hand-decodes `export_type1_960.mid` and `export_clip.midi2` against Ruling 10 and the pinned layouts and says so in the report.
    - `small_goldens_match`: `smf::write(to_smf(small doc, 960, type 1))` and `clip::write(to_clip(small doc, Bass, None))` equal the hand-written files byte for byte.
    - `golden_options`: a fixed document (a "Song" sequence at 120 BPM with a ramp to 140 over bar 2, 3/4 from bar 3, D major, two markers, `SongInfo.copyright`; a "Bass" track on a built-in synth destination on channel 1 with a default patch (bank 0/1, program 33) and MIDI mix volume 100; a "Drums" drum track on channel 10; a muted note; a SysEx; a lyric) exports byte-equal to `export_type1_960.mid`, `export_type0_960.mid`, `export_type1_480.mid` and `export_type1_15360.mid`.
    - `what_is_written_is_what_plays`: a reference used twice with transpose 2, a looped track of 1 bar under 3 bars of song, a play quantize of 1/8 and a shift of +10 ticks: the notes written equal `flatten`'s notes for that track; an inactive take writes nothing.
    - `the_conductor_carries_the_maps`: the ramp is written as tempo steps every 1/16 note (3 840 ticks at 15 360) with Changed "Tempo ramp at bar 2 written as 16 steps"; patch and mix at file position 0 (bank, program, CC 7) in Ruling 10's order; copyright as `FF 02`; the title (the sequence name) as the conductor's track name.
    - `groups_become_ports_and_tracks`: a track with events in groups 0 and 2 writes two SMF tracks, the second named "Bass (group 3)" starting with `FF 21 01 02`.
    - `mpe_tracks_use_the_encoder`: with `mpe: true` and the destination's `mpe` set, the track starts with `configuration(cfg)`'s messages and notes rotate over member channels with their bends; with `mpe: false` the per-note data is Left out "Per-note expression on 4 notes".
    - `export_lists_what_it_leaves_out`: the report has, with counts: Changed "Velocities rounded to 7 bits (MIDI file)", Changed "Positions rounded to 960 ticks per quarter" (only when a position moved), Left out "Muted notes" (counted by a second pass over the chosen lanes' active takes and nested references, since `flatten` drops them silently; F10), "Events outside the sequence" (before `-start_offset` or after its length; F10), "Note attributes", "8-bit SysEx", "Drum row names", "Song info other than title and copyright"; Note "Track 'X' is muted in the project" (Ruling 4).
    - `clip_export_is_exact`: `to_clip` of the Bass track writes `tpq 15360`; the tempo and meter in force at the range start as the header's one Set Tempo and one Set Time Signature; the key signature in force in the sequence data at tick 0 (not the header); every tempo and meter change inside the range as sequence-data Flex Data at its offset (F8); the composition name and clip name; 16-bit velocities, 32-bit values, per-note controllers at note start + offset, and a range `(bar 2, bar 4)` keeps events starting inside it, shifted to 0; byte-equal to `export_clip.midi2`.
  - `round_trip.rs` (proptest, `VIZDAW_FUZZ_CASES` else 64): a generator of documents with one sequence (tempo points with optional ramps, meter changes on barlines, markers, `start_offset` a whole number of beats) and 1–4 MIDI tracks on built-in destinations (channels 1–16, group 0) holding notes (7-bit-origin velocities `up7(v)`), CCs, bend, pressures, programs with banks, 7-bit SysEx and text. Its constraints (F7): every note has `duration >= 1`; no two notes of one pitch overlap on one track and channel, and no two tracks share a channel; no free CC 0 or CC 32 (banks come only with programs); text and names are printable, pass `valid_name` and fit `MAX_TEXT_BYTES`.
    - `smf_round_trip_keeps_what_the_route_carries`: `parse(write(to_smf(doc, ppq 15360)))` applied as a new project equals the projection: per track, notes `(tick, pitch, velocity, duration)`, controller values, programs, SysEx and text equal; the tempo map equals the original with ramps replaced by their steps; meters, markers and `start_offset` equal. At PPQ 960 every position is within 8 ticks (half the grid step) and the report lists Changed "Positions rounded…" exactly when one moved.
    - `clip_round_trip_is_exact`: for one track with 16-bit velocities, attributes and per-note controllers, `parse(write(to_clip(…)))` gives the same events.
    - Fixed cases: nested references, two takes, an MPE track, a drum track, a ramp, a pickup of 1 beat, 1 000 tracks of one note each (finishes under 10 s); and the lossy cases outside the generator, each asserting its report (F7): nested same-pitch notes (on, on, off, off) re-pair first-on first-off with no item; a zero-length note is Left out "Notes of zero length" on export; a free CC 0/32 before a program merges into its bank (Changed).
- [ ] **Step 2: Run** `cargo test -p core-io --test midi_out --test round_trip`. Expected: FAIL.
- [ ] **Step 3: Implement.** `to_smf` builds a document copy with every chosen lane's mute and solo cleared (`LaneCmd::SetFlags` through `core_model::command::apply`), flattens it with a `ViewFilter` whose `hidden_lanes` are the lanes not chosen (and counts what `flatten` hides in a second pass; F10), groups by `path[0].lane`, translates each event with `translate` on its resolved channel (notes give their `When::End` message at `tick + duration`), orders each tick per Ruling 10 using `Class`, writes text per Task 5's table, runs MPE tracks through one `MpeEncoder` per track, scales ticks by `ppq ÷ 15360` with round-half-up, and writes deltas (no VLQ over 4 bytes: a position past `0x0FFF_FFFF` from the last event is Left out "Events too far apart for a MIDI file").
- [ ] **Step 4: Run** `cargo test --workspace` and both clippies. Expected: PASS (about +10).
- [ ] **Step 5: Commit** `feat(io): MIDI file and clip export with goldens and round trips (plan 6a Task 6)` with `crates/core-io/src/midi_out.rs`, `src/lib.rs`, `tests/midi_out.rs`, `tests/round_trip.rs`, `tests/data/`.

### Task 7: The FFI: jobs, rows and session methods (`core-ffi`)

**Files:**
- Create: `crates/core-ffi/src/interchange.rs`, `crates/core-ffi/src/project_id.rs`
- Modify: `crates/core-ffi/Cargo.toml` (`core-io` path dependency), `Cargo.lock` (F1), `src/lib.rs`, `src/jobs.rs`, `src/session.rs` (`save_to`, `apply_jobs`, `discard`, the new methods), `src/bounce.rs` (`dest_path`, `outside`, `partial_fits` become `pub(crate)`), `src/control.rs` (`ControlResult::Interchange` and its JSON); `app/Tests/VizDAWAppModelTests/FakeSession.swift`; every Swift `switch` over `JobKindRow` (add the two cases)
- Test: `crates/core-ffi/tests/interchange.rs` (new), `crates/core-ffi/tests/persistence.rs`

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces (UniFFI):
  - `enum ReportKindRow { Changed, LeftOut, Note }`; `record ReportItemRow { kind, what: String, count: u64, details: Vec<String> }`; `record ReportRow { summary: String, items: Vec<ReportItemRow> }` (`summary` is `summary_line` with "Imported" or "Exported");
  - `JobKindRow::{ImportMidi, ExportMidi}`; `JobRow.report: Option<ReportRow>`; `JobRow.written: Vec<String>` (absolute paths an export put in place);
  - `record SongInfoRow` (the 13 fields, `Option<String>`);
  - `enum MidiFileKindRow { Smf0, Smf1, Clip }`; `record ChannelUseRow { group: u8, channel: u8, notes: u64, suggested: Option<u64> /* None: a new built-in synth */ }`; `record MidiFileInfoRow { kind, tracks: u32, channels: Vec<ChannelUseRow>, has_tempo: bool, mpe_found: bool }`;
  - `record ChannelMapRow { group: u8, channel: u8, dest: Option<u64> }`; `record MidiImportRequestRow { path: String, new_project: bool, seq: u64, lane: Option<u64>, tick: i64, channel_map: Vec<ChannelMapRow> /* empty: suggestions */, split_channels: Option<bool>, mpe: bool, use_tempo: bool }`;
  - `record SmfExportRequestRow { seq: u64, lanes: Vec<u64>, path: String, replace: bool, type0: bool, ppq: u16, mpe: bool }`; `record ClipExportRequestRow { seq: u64, lanes: Vec<u64>, path: String /* a file for one track, a folder for several */, replace: bool, start: Option<i64>, end: Option<i64> }`;
  - `Session` methods, each in `ffi_guard`: `midi_file_info(path: String) -> Result<MidiFileInfoRow>`; `import_midi(req) -> Result<u64>`; `export_midi(req) -> Result<u64>`; `export_midi_check(req) -> Result<ReportRow>`; `export_midi_clip(req) -> Result<u64>`; `export_midi_clip_check(req) -> Result<ReportRow>`; `interchange_report() -> Result<Option<ReportRow>>`; `song_info() -> Result<SongInfoRow>`; `set_song_info(info: SongInfoRow) -> Result<DocSnapshot>`;
  - `ControlResult::Interchange { command: String, doc: String, window: Option<i64>, undo_label: Option<String>, report: Option<ReportRow>, written: Vec<String> }`.
- Rules:
  - `export_midi_check` and `export_midi_clip_check` ignore `path` and `replace` and check only `seq`, `lanes`, `ppq` and the range (F2), so the sheet's dry run works before a path is chosen. They run synchronously (bounded by the limits); the Swift model calls them and `midi_file_info` off the main actor (F12).
  - `MidiFileInfoRow.mpe_found` is true when any `ImportTrack.mpe` is set.
  - Refusals before a job starts are `Model` (bad request, path text: `bounce::dest_path`; an export folder inside the project or any package: `bounce::outside`; `partial_fits`; an existing file without `replace`; a `ppq` outside 480/960/15360; a lane of another sequence; `channel_map` with a duplicate pair, a group or channel over 15, or an unknown destination; `tick` outside `±MAX_TICKS`). `Busy` while recording, while a bounce or another `ImportMidi`/`ExportMidi` job is unapplied, or inside an open edit.
  - `start`/`end` apply to every chosen track of a clip export (F24).
  - Several clip files: names `NN <track>.midi2` (NN from 01, the track name with `/` and `:` replaced by `-`); every name is checked before any is written; all partials are written, then committed in order; a commit failure fails the job naming the files already in place (bounce Ruling 12).
  - The import job reads with `read_regular(path, MAX_MIDI_FILE_BYTES)`, parses with `midi_in::parse` inside `catch_unwind`, and returns `Work::ImportMidi { result: Result<MidiImport, String>, place: Placement }`; `apply_jobs` opens `history.begin("Import MIDI File")`, runs `midi_in::apply`, commits (or aborts on error, changing nothing). `new_project` replaces the history with `History::new(doc)` instead (Ruling 6). Its report becomes the job row's and the session's last report.
  - The export job runs `to_smf`/`to_clip` on a document clone taken when the job starts, then `write_partial` and `commit`; `Work::ExportMidi { result: Result<(Vec<String>, Report), String> }`. A cancelled job removes its partials (the `PendingFile` drop).
  - `save_to` calls `history.stamp_project_id(&project_id::fresh())` first when the document has none and no edit is open; inside an open edit it skips the stamp (Ruling 16, F23).
  - `new_project` applies through a scratch `History::new(doc)` (in a transaction there), then replaces the session's history with a fresh `History::new` of the result; the document reads Edited because `dirty` compares with `clean`.

- [ ] **Step 1: Write the failing tests** in `tests/interchange.rs` (temp folders only; `serial` with `force_offline`; fixtures written with `core_io::smf::write`):
  - `midi_file_info_lists_channels_and_suggestions`: a type 1 file on channels 1, 2 and 10 into a session whose document has a built-in destination on channel 2: three rows, row 2's `suggested` is that destination, the others `None`; `has_tempo` true.
  - `import_is_one_undo_step`: `import_midi` at tick 4 bars into the demo document (`Session::new_demo`) → the job ends Done with a report; lanes +2; `undo_label() == "Import MIDI File"`; one `undo` makes the document equal to before (compare `core_model::file::to_json` text); `interchange_report()` returns the same report.
  - `new_project_import_has_nothing_to_undo`: on `Session::new_empty`, `new_project: true` → tempo from the file, `undo_label() == None`, `snapshot().dirty == true`; on the demo it is `Err(Model)`.
  - `exports_never_leave_half_files`: `export_midi` to `x.mid` writes it and lists it in `written`; again without `replace` is `Err(Model)` and the file is unchanged; with `debug_hold_jobs`, cancelling leaves no `x.mid` and no `.x.mid.partial`; a three-track clip export into a folder where `02 Keys.midi2` exists is `Err(Model)` and writes nothing.
  - `hostile_import_paths_are_refused`: empty, relative, 5 000 bytes, containing NUL, a directory, a FIFO, `/dev/zero`, a symlink loop and a missing file each end the job Failed (or `Err(Model)` at once) with a message naming the path, never `Internal`; the document is unchanged.
  - `hostile_export_paths_are_refused`: a path inside the open project's package, inside another `.vizdaw` folder, with `..`, and a folder that does not exist are `Err(Model)`.
  - `hostile_requests_are_refused`: `ppq` 0 and 70 000, `tick` ±2⁴¹, an unknown `seq`, a lane of another sequence, an empty `lanes` that names nothing for a clip, a `channel_map` with a duplicate, channel 16 or an unknown destination, `start >= end`: each `Err(Model)` naming the field, nothing changed.
  - `busy_states_refuse`: during a held `import_midi`, a second `import_midi` and an `export_midi` are `Err(Busy)`; while recording (offline, an armed track) `import_midi` is `Err(Busy)`.
  - `damaged_file_fails_the_job_with_its_sentence`: a truncated file ends Failed with "This MIDI file can't be read: track 2 ends in the middle of an event."
  - `song_info_round_trips`: `set_song_info` then `song_info()` equal; undo restores; a 5 000-byte title is `Err(Model)`.
  - `export_check_writes_nothing`: `export_midi_check` returns the report and no file exists; with an empty `path` it still succeeds (F2).
  - In `persistence.rs`: `save_stamps_a_project_id_and_save_as_keeps_it`: a new project saved has a valid ID; Save As to another path keeps it; reopening shows the same; a version-7 project gets one on its next save; a save while an edit is open succeeds without stamping, and the next save stamps (F23).
- [ ] **Step 2: Run** `cargo test -p core-ffi --test interchange --test persistence`. Expected: FAIL.
- [ ] **Step 3: Implement** in `interchange.rs` (row conversions, request checks, the job bodies) and the `Session` methods beside `import_audio`/`bounce` in `session.rs`; `discard` handles the two new `Work` variants (an import wrote nothing; an export's partials are already gone). `FakeSession` gains every new method, recording calls and returning settable values.
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Expected: PASS (about +13 Rust).
- [ ] **Step 5: Commit** `feat(ffi): MIDI import and export jobs, reports, song info, project ID (plan 6a Task 7)` with every file above, `Cargo.lock` included.

### Task 8: The control channel

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/tests/commands.rs` (count 170 and its sentence: "6a adds file.import_midi, file.export_midi, file.export_midi_clip and file.interchange_report"), `app/Sources/VizDAWAppModel/ControlCommands.swift` (`ControlJobs.answer` for the two kinds), `app/Sources/VizDAW/ControlChannel.swift` (`whenJobEnds` answers `.interchange` for them)
- Create: `app/Sources/VizDAWAppModel/InterchangeCommands.swift`
- Test: `crates/core-control/tests/commands.rs`, `app/Tests/VizDAWAppModelTests/InterchangeCommandsTests.swift` (new)

**Interfaces:**
- Consumes: Task 7.
- Produces, registry entries after `file.bounce` (menu "File"; every one `document: true`):
  - `file.import_midi`, "Import MIDI File…": `path` (`File`), `new_project` (`Flag`), `seq`, `lane` (`Int 0..=i64::MAX/2`), `tick` (`Int ±2⁴⁰`), `channel_map` (`Text`: JSON `[{"group":0,"channel":9,"dest":3}]`, `dest` null for a new built-in synth), `split_channels`, `mpe`, `use_tempo` (`Flag`); dialog "Open" answered by `path`.
  - `file.export_midi`, "MIDI File…": `path` (`Path("mid")`), `replace`, `seq`, `type` (`Choice ["0","1"]`), `ppq` (`Choice ["480","960","15360"]`), `lanes` (`Text`, comma-separated lane IDs), `mpe`, `dry_run`; dialog "Save" answered by `path` (`always: false`, see Step 1).
  - `file.export_midi_clip`, "MIDI Clip File…": `path` (`File`), `replace`, `seq`, `lanes`, `start`, `end` (`Int 0..=2⁴⁰`), `dry_run`; dialog "Save" (`always: false`).
  - `file.interchange_report`, "Last Import or Export Report…": no parameters; never opens the sheet when called.
  - `InterchangeCommands.ids` and `InterchangeCommands.run(_ id: String, args: [ControlArg], _ docID: UUID, _ m: DocumentModel, store: DocumentStore) -> ControlOutcome`:
    - defaults: `seq` = the shown sequence, `tick` = 0, `type` "1", `ppq` "960", `lanes` = every track, `split_channels` absent (the core's default);
    - `dry_run: true` is handled first, path or not → `.done(.interchange(…, report: check, written: []))` with nothing written (F2); then without `path` → `.dialog("Open"/"Save", "path")`;
    - `new_project: true` → `store.newProject(ProjectFormatRow(sampleRate: 48_000, depth: store.defaultFormat.depth))`, then the import on that model, answered as `.job(command:doc:job:)` with the new document;
    - otherwise `.job(…)`; `file.interchange_report` → `.done(.interchange(report: session.interchangeReport()))`.
  - `ControlJobs.answer` for `.importMidi`/`.exportMidi`: Done is `.ok(status: report?.summary)` whatever `created` holds; Failed/Cancelled as now. `whenJobEnds` replies `.interchange(command:doc:window:undoLabel:report:written:)` for those kinds.

- [ ] **Step 1: Write the failing tests.**
  - core-control `interchange_commands_check_their_params`: each command validates its minimal form; `{dry_run: true}` without `path` validates (no `DialogRequired`, F2); refuses `ppq: "0"`, `ppq: "70000"`, `type: "2"`, `tick: 2⁴¹`, a relative `path`, an export `path` not ending in `.mid` for `file.export_midi`, extra keys and a 10 000-character `channel_map` (`InvalidParams`); without `path` answers `DialogRequired` naming "Open" for `file.import_midi`. The two exports' "Save" dialogs are `always: false` with `when: "no path and no dry_run"`, so core-control lets `dry_run` through and the Swift handler answers the missing-path dialog (tested in `dryRunWritesNothing` and `exportWithoutPathAsksForSave`; F2). The existing `every_command_survives_random_params` (2048 cases) covers the four.
  - Swift `InterchangeCommandsTests` with `FakeSession`:
    - `importMidiBuildsTheRequest`: `{path, tick: 7680, lane: 2, channel_map: "[{\"group\":0,\"channel\":9,\"dest\":null}]", mpe: true}` gives the request with those values and the shown sequence.
    - `importMidiChecksItsArguments`: `channel_map` that is not JSON, not an array, has a duplicate pair, a channel of 16, a group of −1, or a non-integer dest; `lanes: "1,,2"`, `"a"`, a repeated ID; `start` without `end`; `start >= end` — each `invalid_params` naming the command and parameter, and no session call.
    - `dryRunWritesNothing`: `dry_run: true`, with and without `path`, calls only `exportMidiCheck` and answers `done`, never a dialog (F2).
    - `exportWithoutPathAsksForSave`: `file.export_midi` and `file.export_midi_clip` with neither `path` nor `dry_run` answer the "Save" dialog for `path`.
    - `newProjectMakesADocument`: `new_project: true` makes one new document through the store and starts the import on it.
    - `reportCommandCallsTheSessionOnce`: `file.interchange_report` calls `interchangeReport()` once, returns its report and opens nothing (the sheet check moves to Task 9; F13).
    - `everyRegisteredMenuCommandHasAHandler` passes with the four ids in `ControlCommands.handled`.
- [ ] **Step 2: Run** `cargo test -p core-control` and `scripts/swift.sh test --filter InterchangeCommands`. Expected: FAIL.
- [ ] **Step 3: Implement** in `InterchangeCommands.swift`, in `BounceCommands.request`'s style (parse, then a `Result<…, ControlFailure>`); the `channel_map` JSON is parsed with `JSONSerialization` and every element checked. Nothing opens a panel or sheet.
- [ ] **Step 4: Run** `cargo test -p core-control`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Expected: PASS (registry 170; about +1 Rust, +6 Swift).
- [ ] **Step 5: Commit** `feat(control): MIDI import and export commands and the report (plan 6a Task 8)`.

### Task 9: The app's models (`VizDAWAppModel`)

**Files:**
- Create: `app/Sources/VizDAWAppModel/Interchange.swift`, `app/Sources/VizDAWAppModel/ProjectSettingsModel.swift`
- Modify: `DocumentModel.swift` (accessors `interchange`, `projectSettings`, made once like `bounce`), `ProjectMedia.swift` (`pollJobs`: an ended `.importMidi`/`.exportMidi` job the UI started sets the report and shows the sheet), `DocumentStore.swift` (`open(path:)` routes MIDI files), `PaneContent.swift` (the region's name)
- Test: `app/Tests/VizDAWAppModelTests/InterchangeTests.swift`, `ProjectSettingsModelTests.swift` (new)

**Interfaces:**
- Consumes: Tasks 7–8.
- Produces:
  - `@Observable @MainActor public final class InterchangeModel` (in `BounceModel`'s style, weak document):
    - `report: ReportRow?`, `reportShown: Bool`; `showLastReport()`;
    - every synchronous core call these sheets make (`midiFileInfo`, `exportMidiCheck`, `exportMidiClipCheck`) runs in a `Task.detached` with a generation counter; the result is applied on the main actor only if it is the latest (F12);
    - import sheet: `importShown`, `importPath`, `info: MidiFileInfoRow?`, `newProject: Bool`, `choices: [ChannelChoice]` (`group`, `channel`, `dest: UInt64?`, `notes`), `splitChannels: Bool` (default: `info.kind == .smf0`), `mpe` (default `info.mpeFound`), `useTempo` (default false; forced on and disabled for a new project or an empty project); `beginImport(path:)` (reads `midiFileInfo`; an error is an alert "Import Failed"); `startImport()`;
    - `static func isMidi(_ path: String) -> Bool` (extensions `mid`, `midi`, `smf`, `midi2`, any case);
    - `importDropped(paths: [String], target: ImportTargetRow)`: every MIDI path imports with defaults at the drop target (Ruling 12), and the other paths go to `importAudio(paths:target:)` as today;
    - export sheet: `exportShown`, `exportKind: .smf/.clip`, `type0`, `ppq` (960), `lanes: Set<UInt64>` (default: the selected tracks, else all), `mpe`, `useSelection` (clip: the time selection), `wontTransfer: ReportRow?` refreshed from `exportMidiCheck`/`exportMidiClipCheck` on every change; `startExport(path:)`.
  - `public enum ReportText`:
    - `groups(_ r: ReportRow) -> [(title: String, items: [ReportItemRow])]` in the order "Changed", "Left out", "Notes", empty groups omitted;
    - `line(_ i: ReportItemRow) -> String` (`"<what> (<count>)"` when count > 1);
    - `details(_ i: ReportItemRow) -> [String]` (adds `"and N more"` when `count > details.count`);
    - `plain(_ r: ReportRow) -> String` for Copy and Save as Text: summary, then each group's title and lines with indented details; an empty report is `"<summary>\nEverything transferred."`.
  - `@Observable @MainActor public final class ProjectSettingsModel`: the format choice (rate, depth) and the 13 song-info texts; `load()`, `apply()` (one `setSongInfo` when any field changed, `changeFormat` when the format changed; nothing when nothing changed).
  - `DocumentStore.open(path:)`: `mid`, `midi`, `smf` and `midi2` (any case) make a new project (48 kHz at the default depth) and run its import with `new_project: true` (Ruling 12); other non-`.vizdaw` paths keep today's alert.
  - `DocumentModel.inspectorRows` passes `region.regionName ?? media name`; `public func renameRegion(_ event: UInt64, to name: String)` (empty clears) as one undo step.

- [ ] **Step 1: Write the failing tests.**
  - `InterchangeTests`:
    - `importSheetDefaults`: a type 0 info → `splitChannels` true; a type 1 → false; `mpeFound` → `mpe` true; a new project forces `useTempo`; suggestions fill `choices`.
    - `startImportSendsTheChoices`: changing row 2 to "New built-in synth" sends `ChannelMapRow(dest: nil)`; the target is the shown sequence at the playhead, with the selected track as `lane` when one is selected.
    - `exportOptionsRefreshTheDryRun`: each change of type, ppq, tracks or MPE calls the check once; the sheet's `wontTransfer` is its result.
    - `reportTextGroupsAndCaps`: a report with Changed (1), Left out (120 details given 50) and Note items gives the three groups in order, `"Muted notes (120)"`, 50 details and `"and 70 more"`; `plain` of an empty report ends with `"Everything transferred."`; no string says "lane".
    - `reportCommandNeverOpensTheSheet` (moved from Task 8; F13): running `file.interchange_report` through `InterchangeCommands.run` leaves `interchange.reportShown` false.
    - `checksRunOffTheMainActor` (F12): `beginImport` and the export dry run call the session from a detached task; with two option changes in a row only the second result lands (a generation counter), and the model stays usable while a check runs.
    - `aFinishedUiJobShowsTheReport`: a UI-started import that ends Done sets `report` and `reportShown`; a control-started one (not in `jobs`) does not.
    - `droppedMidiImportsWithDefaults`: two `.mid` paths and one `.wav` dropped: two `importMidi` calls at the target, one `importAudio` with the `.wav`.
    - `openRoutesMidiFiles`: `store.open(path: "/tmp/a.MID")` makes a new document and starts a `new_project` import; `"/tmp/a.txt"` still alerts.
  - `ProjectSettingsModelTests`:
    - `applySendsOnlyWhatChanged`: unchanged → no calls; a title change → one `setSongInfo`; a format change → one `changeFormat`; both → both.
    - `regionNameShowsInTheInspector`: a region with `regionName` "Take 3" shows "Take 3" in the Region row; without, the media name.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter Interchange` and `--filter ProjectSettings`. Expected: FAIL.
- [ ] **Step 3: Implement** to pass the tests.
- [ ] **Step 4: Run** `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +9 Swift).
- [ ] **Step 5: Commit** `feat(app): MIDI interchange, report and project settings models (plan 6a Task 9)`.

### Task 10: Views, menus, document types, README checks, e2e, backlog, gates

**Files:**
- Create: `app/Sources/VizDAW/MidiSheets.swift` (import and export sheets), `ReportSheet.swift`, `ProjectSettingsSheet.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift` (File menu; `Panels.chooseProject`'s message becomes "Choose a VizDAW project or a MIDI file" and it accepts `mid`, `midi`, `smf`, `midi2`), `DocumentView.swift` (attach the sheets beside `BounceSheet`, line 34), `PaneViews.swift` (editable region name in `InspectorPane`), `VizDAWApp.swift` (no change needed when `open(path:)` routes; check), `Panels` (choosers), `app/Sources/VizDAWEditors/ArrangementView.swift` and `RegionStripView.swift` (their `drop` calls `document.interchange.importDropped(paths:target:)` instead of `importAudio`), `RegionEditing.label` callers (the region's name); `scripts/make-app.sh` (document types); `app/README.md`; `docs/feature-backlog.md`; `crates/vizctl/tests/e2e.rs`
- Test: `scripts/swift.sh test` (name scans), the e2e

**Interfaces:**
- Consumes: Task 9.
- The File menu, after "Import Audio…": "Import MIDI File…" (enabled with a document; `Panels.chooseMidi()` allows `mid`, `midi`, `smf`, `midi2`); after "Bounce…": an "Export" submenu with "MIDI File…" and "MIDI Clip File…", each opening its sheet first and the Save panel from the sheet's Export button (the export sheet carries the spec §9 line under its title: MIDI File "Notes, controllers, tempo and markers as a Standard MIDI File"; MIDI Clip File "One track, lossless, as a MIDI 2.0 Clip File"); then "Last Import or Export Report…" (disabled with no report).
- "Project Settings…" opens `ProjectSettingsSheet` (Format: rate and depth pickers; Song info: 13 labelled fields, Comment multi-line) instead of `FormatPanel`; `file.project_settings` over the channel is unchanged.
- The report sheet (spec §9): summary line; disclosure groups with counts; Copy, Save as Text… (a save panel, `.txt`), and for exports Show in Finder; "Everything transferred." when empty.
- `make-app.sh`'s Info.plist: a `CFBundleDocumentTypes` entry, role Viewer, rank Alternate, for `public.midi-audio`; a `UTExportedTypeDeclarations` entry `org.vizdaw.midi-clip` beside `org.vizdaw.project` ("MIDI Clip File", conforms to `public.data`, extension `midi2`; F14) and a document type for it. `plutil -lint` passes (the script already runs it).

- [ ] **Step 1: Build the views and menus** as above (the export sheet before the Save panel, Ruling 18; say so in the task report): theme tokens only, no `@State`/`@FocusState`, accessibility labels on every control ("Destination for channel 10", "Song title", …), "track" in every string.
- [ ] **Step 2: Write the e2e** `midi_files_round_trip_over_the_channel` (`#[ignore = "needs a window server and VIZDAW_E2E_APP"]`, throwaway home, offline): `file.create_demo`; `file.export_midi` with `dry_run: true` returns an `interchange` result with a report and no file; `file.export_midi` to `<tmp>/demo.mid` writes it and its `written` lists it; `file.export_midi_clip` for one lane to `<tmp>/one.midi2`; `file.import_midi` of `demo.mid` at bar 9 adds as many tracks as the demo has MIDI tracks (the export's conductor track makes none, F11; via `state.get document`'s `lanes`), `undo_label` "Import MIDI File"; `file.interchange_report` returns that report; `file.import_midi` with `new_project: true` of `one.midi2` makes a second document; a path inside the demo's package for `file.export_midi` fails; `path` missing answers `dialog_required` naming "Save".
- [ ] **Step 3: Add README checks 257–266** after 256 under "### MIDI files (plan 6a)", with a line saying what tests cover:
  - 257: File › Import MIDI File… on a type 1 file shows the sheet: where (this project at the playhead, or a new project), the channel → destination table with suggestions, Split by channel, Treat as MPE, Use the file's tempo map (off). Import: new tracks at the playhead; one Undo ("Import MIDI File") removes them all.
  - 258: The report sheet opens after the import: the summary line, Changed / Left out / Notes with counts and disclosure rows naming tracks and bars; Copy and Save as Text… work; File › Last Import or Export Report… reopens it.
  - 259: A type 0 file splits into one track per channel; channel 10 becomes a drum track with General MIDI row names.
  - 260: Drop a `.mid` on the arrangement: it lands at the drop point (on the track under it for a one-track file). Drop a `.wav` and a `.mid` together: both import.
  - 261: File › Open… a `.mid` (and double-click one in Finder after `scripts/make-app.sh`, choosing VizDAW): a new untitled project with the file's tempo and title.
  - 262: File › Export › MIDI File…: the sheet's type, resolution, tracks and MPE options update "What won't transfer" as you change them; Export asks for a place; the file opens again in VizDAW with the same notes.
  - 263: File › Export › MIDI Clip File… with one track writes one `.midi2`; with three tracks, a folder of three; with a time selection, only that range.
  - 264: Exporting over an existing file asks first; choosing a place inside the project is refused with a message; Cancel during a long export leaves no file.
  - 265: File › Project Settings… has a Song info section: type a title and composer, OK; Undo ("Set Song Info") clears them; Save, close and reopen: they are there.
  - 266: Select a region: the inspector's Region name can be typed (one Undo, "Rename Region"); the arrangement shows the new name; clearing it shows the file's name again.
  - **Owner check (manual, only the owner can do):** export the demo song as a MIDI file, open it in the DAW you use that reads MIDI files, and check tracks, notes, tempo and markers; export a MIDI file from that DAW and import it into VizDAW. Also export one track as a MIDI Clip File and open it in any other software that reads `.midi2`, if you have one (F20).
- [ ] **Step 4: Add backlog rows** to `docs/feature-backlog.md` in its style: SMF type 2; the MIDI 2.0 container file once published; Flex Data marker/cue statuses if Task 4's check found none; check Clip File interop with a second reader (F20); a hostile 64 MiB MIDI file can allocate about 1 GB during import (F5); a MIDI file is parsed twice, for the sheet and the job (F12); move `passes.rs`'s `LiveMsg`-to-event mapping into one shared place with `midi_in` (F22c); `core_io::write::PendingFile` repeats `core_media`'s `PendingExport` pattern (F22d); the Clip File title as composition name versus project name, if the owner prefers the other (F8).
- [ ] **Step 5: Run all the gates:** `cargo fmt --all -- --check`; both clippies and `cargo clippy -p core-ffi --lib -- -D warnings`; `cargo test --workspace`; `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`; the no-brand build; `scripts/make-app.sh` and the e2e. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing. Expected: all PASS (e2e 24, registry 170, README through 266, `VERSION` 8).
- [ ] **Step 6: Commit** `feat(app): MIDI import and export sheets, report sheet, song info, README checks 257–266 (plan 6a Task 10)` with the files above.

## Self-Review

- **Spec coverage (6a row of §13):**
  - `core-io` crate, report, limits → Tasks 2–4; format version 8 with Project Settings › Song info → Tasks 1, 7, 9, 10;
  - SMF import and export (§7.2) → Tasks 3, 5, 6; MIDI Clip File (§7.3) → Tasks 4–6;
  - the MIDI sheets, the report sheet, drag-in, File › Open and the Dock → Tasks 9–10;
  - the four commands with hostile tests (§10) → Task 8, and the core-side refusals → Task 7;
  - jobs, staging (nothing to stage for MIDI), partial exports, busy states (§6.2) → Task 7;
  - goldens, round trips, fuzz, one test per §6.5 row that applies to 6a (regular file, 64 MiB, chunk lengths, VLQ, events, SysEx, running status, unknown chunks, positions) → Tasks 2–6;
  - README from 257, e2e, backlog rows (§14: SMF type 2, container) → Task 10.
- **Deliberately not here:** ZIP, XML and compound-file limits (6d, 6f); `ImportProject`/`ExportProject` job kinds (Ruling 2); Conflicts group (6e).
- **Names are consistent across tasks:** `SongInfo`, `SetInfo`, `SetName`, `stamp_project_id`, `IoError`, `read_regular`, `write_partial`/`PendingFile`, `Report`/`Item`/`ItemKind`/`Summary`, `SmfFile`/`SmfTrack`/`SmfEvent`/`Division`, `Packet`/`ClipFile`, `MidiImport`/`ImportTrack`/`ImportOptions`/`Placement`/`DestChoice`, `parse`/`apply`/`suggested_dest`, `SmfOptions`/`ClipOptions`/`to_smf`/`to_clip`, the UniFFI rows, `InterchangeModel`, `ReportText`, `ProjectSettingsModel`, `InterchangeCommands`.
- **Review Focus:** each line names its tests in the owning task.
