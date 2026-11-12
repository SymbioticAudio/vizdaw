# DAWproject Implementation Plan (6d)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** DAWproject import and export, the full-fidelity route. Export writes one `.dawproject` ZIP holding `project.xml`, `metadata.xml`, the audio as WAV, plug-in state files and VizDAW's private return record. It carries:
- tracks, strips, buses, sends, plug-ins with state, track names and colours;
- note clips with per-note expression, MIDI controller points, audio clips with fades and crossfades;
- the tempo and meter maps, markers, automation and song info.

It also keeps a snapshot of each export in the project for plan 6e's merge. Import opens any DAWproject file as a new project and reports everything it changed or left out. Hostile files are refused or partly imported, with bounded memory and time and never a panic.

**Architecture:**
- **`core-io`** gains:
  - `zip`: a bounded reader and a writer on `rawzip`, with `flate2` for DEFLATE;
  - `xml`: a bounded tree reader on `quick-xml` and an escaping writer;
  - `dawproject`: a typed model of the schema (`schema`), the export mapping, the import mapping, the record and snapshot (`record`), units and tags (`units`) and plug-in state (`plugins`).

  Everything there is platform-neutral. The AU property-list conversion sits behind a `PlistCodec` trait.
- **`core-ffi`** gains `dawproject.rs`:
  - an `ExportDawproject` job (its own kind; 6b's `ExportProject` is "Export for another DAW", pre-flight L1): WAV copies staged beside the destination, the ZIP written as a partial, then renamed into place, then the snapshot written into the package's `Interchange/`;
  - an `ImportProject` job: open, extract and convert the audio through `MediaImporter`, map;
  - the session methods. An import applies as a new project, as 6a's new-project MIDI import does.
- `platform-macos` gains `plist` (CoreFoundation conversion).
- The control channel gains `file.import_dawproject` and `file.export_dawproject`.
- Swift gains:
  - `DawprojectModel` and `DawprojectSheet`;
  - File › Import Project ▸ DAWproject… and Export ▸ DAWproject…;
  - File › Open and Dock drops of `.dawproject`.

  The report sheet and job queueing are 6a's and 6b's.

**Tech Stack:** Rust 2024 (`core-io`, `core-model` read-only, `core-media`, `core-ffi` with UniFFI 0.32.2, `core-control`, `platform-macos`, `vizctl`), Swift 6.1 / SwiftPM (Swift Testing), macOS 14+.
- New crates (checked on crates.io on 2026-11-12):

  | Crate | Version, date | Licence | Why |
  |---|---|---|---|
  | `rawzip` | 0.5.2, 2026-10-04 | MIT | ZIP read and write. Pure Rust, no dependencies, no `unsafe`. It walks the central directory without allocating per entry, so our 10 000-entry limit applies before a hostile directory costs memory. Rust 1.85. |
  | `flate2` | 1.1.11, 2026-11-06, `default-features = false, features = ["rust_backend"]` | MIT OR Apache-2.0 | DEFLATE for `rawzip`, through `miniz_oxide` 0.9.1 (MIT OR Zlib OR Apache-2.0) and `adler2` 2.0.1 (0BSD OR MIT OR Apache-2.0). Pure Rust. |
  | `quick-xml` | 0.42.0, 2026-08-22, `default-features = false` | MIT | A streaming XML reader. It reports `DocType` and entity references as events, so we refuse them. Rust 1.86. Pulls `memchr` 2.8.3 (Unlicense OR MIT), already in the lockfile. |
  | `serde`, `serde_json` | already in the lockfile | MIT OR Apache-2.0 | `record.json` and the snapshot. |
- Not used:
  - `zip` 9.5.1 (MIT, 2026-11-10), the spec's pick (§5). It needs Rust 1.88 and pulls `nix`, `getrandom`, `typed-path` and `zeroize`, and it materialises the whole central directory before we can count the entries.
  - `roxmltree` 0.21.2. It builds a DOM, has no depth limit we can set below its own, and would have to hold the whole 256 MiB text.

**Spec:** `docs/superpowers/specs/2026-11-11-import-export-design.md`. Plan 6d's row of §13 covers:
- §2 goal 2;
- §7.1's DAWproject column and §7.4 in full;
- §7.0's DAWproject use;
- §7.8's write side: the record, the tags and the snapshots;
- §6.1–§6.5 as they apply;
- §9: Import Project ▸ DAWproject…, Export ▸ DAWproject… and its sheet, "What won't transfer", the report;
- §10: `file.import_dawproject` (New Project only) and `file.export_dawproject`, their hostile tests and `dry_run`;
- §11;
- §12: goldens, round trips, XSD checks, plug-in state, hostile files, limits, jobs, the DAWproject part of the e2e, README checks and the 6d owner check.

Plan 6e owns the merge: the "New Project" / "Merge With Existing Project" prompt, matching, the three-way merge, the preview and the Conflicts group.

Build on what 6a–6c built; read it, do not re-plan it:
- `core-io`: `limits`, `read`, `write`, `report`, `text::sanitize`, `names::safe_part`, `midi_in`, and `midi_out`'s `source`, `file_start`, `start_meters` and `start_bpm`;
- `core-ffi`: `interchange.rs` (`file_path`, `file_dest`, `io_error`, `report_row`), `jobs.rs`, `for_daw.rs`, `Session::interchange_idle`, `apply_midi_import`'s new-project path and `apply_import_as`'s cleanup;
- `core-model`: `crossfade`, `automation`'s laws (`fader_db`, `fader_norm`, `pan_norm`, `pan_of`), `state` and `History::capture_state`;
- `core-media`: `import::import_reserved`, `export::PcmWriter` and `record::free_bytes`;
- Swift: `InterchangeCommands`, `InterchangeModel`, `ReportText`, `ReportSheet`, `ForDawModel`'s queueing, and `DocumentStore.openMidi`.

**The DAWproject format as checked on 2026-11-12** (https://github.com/bitwig/dawproject, MIT; README: "The format is version 1.0 and is stable"):
- **What we pin:** `main` at commit `ee4dcdde75940f30e14e55401a26955a58b8322b` (2025-07-12). `Project.xsd` there last changed in `2c45c639` (2024-11-21). SHA-256: `Project.xsd` `58e2fd9864772850aac3eab1f3de8693857dc5384df6d29fbc320ae1de2347cc`, `MetaData.xsd` `fb3ba378271770dddbcced8990aba537de3d36ff2d58573523460a699221c99f`. We also read `Reference.html` at that commit.
- **The tag:** `v1.0.0` (2023-09-26, `1651bf06`) is the only one. Its `Project.xsd` lacks `Send/Enable` and `Clip@enable`; `main` has both.
- **Element names the plan uses, with the XSD's sequence order, which the writer must keep:**
  - **`Project@version`:** `Application@name,version`, `Transport` (`Tempo`: realParameter with `unit="bpm"`; `TimeSignature@numerator,denominator`), `Structure` (`Track` | `Channel`)*, `Arrangement`, `Scenes`.
  - **`Track`:** `@contentType`, a list of `audio automation notes video markers tracks`; `@loaded`; then `Channel?` and `Track*`.
  - **`Channel`:**
    - attributes `@role` (`regular master effect submix vca`), `@audioChannels`, `@destination` (IDREF), `@solo`;
    - children in order: `Devices`, `Mute` (boolParameter), `Pan` (realParameter), `Sends/Send*`, `Volume` (realParameter).
  - **`Send`:** `@destination`, `@type` (`pre post`), with `Enable`, `Pan`, `Volume`.
  - **Devices:**
    - `AuPlugin`, `ClapPlugin`, `Vst2Plugin`, `Vst3Plugin`, `BuiltinDevice`, `Equalizer`, `Compressor`, `NoiseGate`, `Limiter` and `Device`;
    - attributes `@deviceID`, `@deviceName` (required), `@deviceRole` (`instrument noteFX audioFX analyzer`), `@deviceVendor`, `@loaded`, and `@pluginVersion` on plug-ins;
    - children in order: `Parameters`, `Enabled`, `State@path,external`.
  - **Parameters:** `RealParameter@parameterID,unit,value,min,max` and `BoolParameter@value`. Every referenceable element has `@id`, `@name`, `@color` (`#rrggbb`) and `@comment`.
  - **`Arrangement`:** `Lanes`, `Markers`, `TempoAutomation` and `TimeSignatureAutomation` (both `Points`).
  - **Timelines** (`@timeUnit` `beats`|`seconds`, inherited, default `beats`; `@track`):
    - `Lanes`, `Clips`, `Notes`, `Points`, `Audio`, `Warps`, `Markers` (at least one `Marker`), `ClipSlot` and `Video`;
    - `Clip@time,duration,contentTimeUnit,playStart,playStop,loopStart,loopEnd,fadeTimeUnit,fadeInTime,fadeOutTime,enable,reference`, holding one content timeline;
    - `Note@time,duration,channel,key,vel,rel` (vel and rel normalised), holding at most one child timeline for per-note expression;
    - `Points@unit`, holding `Target@parameter,expression,channel,key,controller`, then `RealPoint@time,value,interpolation` (`hold linear`), `BoolPoint`, `IntegerPoint`, `EnumPoint` or `TimeSignaturePoint`;
    - `Audio@channels,sampleRate,duration` (seconds), holding a `File`;
    - `Warps@contentTimeUnit`, holding one timeline, then `Warp@time,contentTime`;
    - `Marker@time`.
  - **`expression`:** `gain pan transpose timbre formant pressure channelController channelPressure polyPressure pitchBend programChange`. The spec's "controller" target is `expression="channelController"` with `@controller`.
  - **`MetaData`:** `Title Artist Album OriginalArtist Composer Songwriter Producer Arranger Year Genre Copyright Website Comment`, in that order.
- **Reference facts used:**
  - a negative `fadeInTime` "will make this Clip start at t = time − abs(fadeInTime)" (a crossfade form);
  - `File@external="true"` paths are relative to the `.dawproject` file, or absolute;
  - `RealParameter` values may be `inf`/`-inf`;
  - the reference library strips a UTF-8 BOM.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`. Run Swift only through `scripts/build-ffi.sh` (after any FFI change), `scripts/swift.sh build|test` and `scripts/make-app.sh`. Never run `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Branch `sp6-import` (at `0466205` when this plan was written, `4e933b4` with it; plans 6a, 6b and 6c are finished on it). Never switch branches, never push.
  - Commit at the end of every task by explicit paths (never `git add -A`). Never commit `.superpowers/`, `.DS_Store` or generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`).
  - `Cargo.lock` changes only in Task 1, by the crates in Tech Stack and what they pull in (`miniz_oxide`, `adler2`, `crc32fast`, `simd-adler32`; Task 1 Step 5 copies `cargo tree -p core-io -e normal`'s new crates and licences into Tech Stack and Task 13's notices row, S1); no later task adds a dependency (Task 1 also adds `core-io`'s path dev-dependency on `core-engine` with `fixtures`, L3). The workspace `rust-version` goes 1.85 → 1.86 in Task 1, which `quick-xml` 0.42 needs.
  - Every message ends with the executing session's own attribution lines: its harness's `Co-Authored-By` line naming the model actually running, then the session line, e.g.
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Then check with `git log -1 --oneline && git show --stat HEAD` (a commit without `DEVELOPER_DIR` fails silently).
- **Clean room and names.**
  - Never read `~/workspace/SVP/research`.
  - No Opcode, Logic, Apple (except existing platform facts), Midas, Soundcraft, NI, Microsoft, Ableton, Steinberg/Cubase or ROLI names in UI or repo text; say "other DAWs". The MIDI Association, General MIDI and MPE are specification names and are allowed.
  - **Ardour** may be named only in docs, the backlog and third-party licence notices, whose notices are kept. Never name it in code, comments, tests, UI text or README steps.
  - **DAWproject's origin** (the company in its repository path and its MIT notice) appears only in the reference URL `https://github.com/bitwig/dawproject` (doc comments, the test-data README) and in the vendored `LICENSE` notice, kept verbatim. It never appears in UI text, report text, registry help, README steps or test fixtures' `Application@name`. Our fixtures name their application "Fixture" or "VizDAW". No other DAW maker or product is named anywhere, not even the ones the format's README lists.
  - The scans enforce this: `NameScanTests.noThirdPartyNamesInWhatTheAppShows` and `core-control/tests/names.rs`, which gain the format's origin and the other readers' names in Task 13.
- **Track, not lane.** Text the app shows (labels, menus, sheets, report lines, alerts, README steps) says "track". Identifiers, command IDs, JSON keys and the file format keep `lane`. `core-ffi/tests/wording.rs` already scans `core-io`.
- **Never run `auval`**, in any form.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. The owner checks the views by eye from README steps 282–290.
  - Launch VizDAW only through the e2e (Task 13) with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - Tests never open real audio or MIDI, never trigger the microphone prompt or TCC, and never touch the real `~/Library/Application Support/VizDAW`, preferences, Trash or devices. Use temp folders (`tempfile`), `Session::debug_set_trash_dir`, `MemoryDefaults()`, the e2e's throwaway home and fakes only. Every `core-ffi` test file calls `vizdaw_ffi::host::force_offline()` through its `serial` helper.
- **Programmatic calls open nothing.** Control-channel commands and programmatic calls never open dialogs, sheets, menus, panels or windows. The DAWproject sheet and the report sheet open only for jobs the user started from the UI. `file.import_dawproject` makes a window, as `file.import_midi` with `new_project` does.
- **Hostile input.**
  - Every new command gets hostile-input tests at every layer (Tasks 9–12). Every file is treated as hostile (§6.5).
  - Every computed tick, frame and length uses `checked_*`/`saturating_*` or `i128`; nothing wraps.
  - Every float from a file is finite and in range before it becomes ticks, frames or a value.
  - Every name we make from user text passes `names::safe_part`. Every text we take from a file passes `text::sanitize`.
- **FFI safety.** No panic crosses the FFI: every new export returns `Result<_, VizError>` inside `ffi_guard` (`catch_unwind`). The job threads run every parse, mapping and write inside `std::panic::catch_unwind`; a caught panic fails the job with `internal error: …` and is a bug.
- **Swift 6.1.**
  - Generated bindings compile in Swift 5 mode; app modules and tests compile in Swift 6 mode with no new warnings.
  - No `@State` or `@FocusState` anywhere in this plan's code (view state lives in the `@Observable` models).
  - Never capture `[weak self]` or a non-Sendable value inside `MainActor.assumeIsolated`; bind a strong `let` first, as `DocumentWindow.swift` does.
  - Colours, spacing, radii and fonts come from the theme; every new control is accessibility-labelled.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`: use `.get()`, iterators and `checked_*`/`saturating_*`. Rust stable, edition 2024, GPL-3.0-only. `core-model`, `core-engine`, `core-media`, `core-io` and `core-control` stay free of OS APIs; everything Rust here runs on Linux.
- **TDD.** Every new test is run and seen to fail before the code that makes it pass.
- **Real-time.** This plan adds no real-time code and does not touch `core-engine`. If a task finds it must, it runs 6c's rt-check gate.
- **Format.** `core_model::file::VERSION` stays 9; the document format does not change. The snapshots are package files (`Interchange/<export id>.json`), not part of `project.json`.
- **Lessons from plans 6a–6c (binding):**
  - **The core owns file naming.**
    - Every name the app shows or checks comes from the core: `Session::dawproject_name`, the ZIP's entry names and the snapshot's name.
    - Swift never builds a file name.
  - **Never replace anything without explicit confirmation.**
    - An export over an existing file needs `replace` (the Save panel's confirmation, or the channel's `replace`), and the old file stays until the new one is complete.
    - Existence is always `symlink_metadata` (lstat): a dangling link counts as taken.
    - Snapshots, extracted audio and staged WAVs are written with `create_new` and never replace anything.
  - **No file I/O or parsing on the main thread.**
    - `dawproject_info` and `export_dawproject_check` are called from detached tasks with a generation counter.
    - `export_dawproject` and `import_dawproject` check only text and the model before they spawn. They have two carve-outs, as `export_midi` and `bounce` do: `file_dest`'s lstat, and the plug-in capture with `refresh_media`. Say so in Task 9's report.
  - **Every dropped item gets a report line.** Each task's "listed" test names the item kinds it drops, with their exact `what` text.
  - **Goldens are hand-written before the code and can fail.**
    - The XML goldens and the hand-made import fixtures are typed by hand from this plan's rules before the code exists.
    - They are read with a copy of 6a's never-blessed `hand_golden` (`core-io/tests/midi_out.rs`). `VIZDAW_BLESS` never writes them, and a missing one fails.
  - **Cancel is tested** at every stage of both jobs (Tasks 9, 10, 12).
  - **One interchange job at a time.**
    - `ImportProject` and `ExportDawproject` join `interchange_idle`'s pending kinds and `ForDawModel.blocking` (L1).
    - The export sheet queues behind running jobs as 6b's sheet does. An import always runs in its own new document, so it never waits.
  - **Every user action is one undo step.**
    - An import as a new project leaves nothing to undo (spec §6.1).
    - An export adds no step. Its project-ID stamp is not one (`History::stamp_project_id`).
  - **Edit notes and the report are surfaced.** The report sheet opens when a UI-started job ends. Last Import or Export Report… reopens it.
- **UniFFI changes** (Tasks 9–10 only; every new field on an existing record is `#[uniffi(default)]` or `#[uniffi(default = None)]`):
  - `JobKindRow::ImportProject` and `JobKindRow::ExportDawproject` (L1); the enum `RegionGainRow { Apply, LeaveOut }`;
  - records `DawprojectExportRequestRow`, `DawprojectImportRequestRow` and `DawprojectInfoRow`;
  - `Session::export_dawproject`, `export_dawproject_check`, `dawproject_name`, `dawproject_info` and `import_dawproject`;
  - the `test-hooks` methods `debug_dawproject_cancel_at`, `debug_free_bytes` and `debug_plugin_param(slot, param) -> Result<f64, VizError>` (reads a loaded slot's current parameter value from the host; L4);
  - the free function `dawproject_info(path: String) -> Result<DawprojectInfoRow, VizError>` (`#[uniffi::export]` in `lib.rs`, beside `shutdown`), so the store reads a file with no document open (L8).

  `FakeSession` gains each method, with logs and settable answers, in the task that adds it. Every Swift `switch` over `JobKindRow` and every `==`/`contains` chain on job kinds gains both kinds: `ProjectMedia.swift` `jobText` ("Exporting DAWproject…", "Importing DAWproject…") and `failureTitle` ("DAWproject Export Failed", "DAWproject Import Failed"), `ControlChannel.swift` (~167), `ControlCommands.swift` (~77, ~87: a finished job of either kind answers `.ok(status: report.summary)`), `Interchange.swift` (~112, ~119: `jobFailed` reopens no MIDI sheet for them). `.exportDawproject` lands in Task 9, `.importProject` in Task 10 (L1).
- **Control channel.** The registry goes 173 → 175 in Task 11. Every command is in the registry and handled (`everyRegisteredMenuCommandHasAHandler`).
- **Each task ends green.**
  - Always:
    - `cargo fmt --all` then `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches. Run the whole workspace in Tasks 1, 8, 10 and 13.
  - Tasks 9–13 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 13 also runs:
    - the no-brand build: `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <a fresh temp dir>`;
    - `scripts/make-app.sh`;
    - the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.
  - Known flaky:
    - the `damaged_files` proptest;
    - the MIDI listener tests (`platform-macos/tests/midi.rs`);
    - `core-ffi` catalog tests under parallel load;
    - the latency e2e timing.

    Re-run once before investigating, and say so in the report.
  - **Baseline:** measure at the start of Task 1: Rust passed/ignored, Swift, e2e 26, registry 173, README through 281, `VERSION` 9. Each task's last step gives the counts it adds. Report the measured counts; a slip in the writer's arithmetic is not a failure.
- **How code is given.**
  - New test files are written whole.
  - New implementation files get their public items with doc lines, plus the algorithm where the signature and tests leave a choice.
  - Changes to existing files follow the existing pattern named in the step. If a named anchor no longer matches, make the same change to the current text and say so in the report.

## Review Focus

These are the five failure modes a user is most likely to hit, most likely first. The spec implies them, but no ordinary test would exercise them. Each is pinned by named tests in its task.

1. **A file from another DAW whose structure VizDAW does not have.**
   - Examples:
     - folder tracks;
     - an audio clip holding a `Clips` of audio events (two levels);
     - `Warps` that only map seconds to beats;
     - `timeUnit="seconds"` on an outer `Lanes` with beats inside;
     - a looped clip whose `playStart` is not its `loopStart`;
     - alias clips (`reference`) on two tracks with different instruments;
     - automation `Points` at the arrangement level with no `track`;
     - a negative `fadeInTime`.
   - Expected: every clip plays where it played there, within a tick or a frame. What VizDAW cannot hold is listed.
   - Pinned by:
     - `nested_clips_compose_their_offsets`, `identity_warps_are_not_reported`, `seconds_timelines_map_through_the_tempo_map`, `looped_clip_becomes_a_looped_use`, `alias_clips_share_one_sequence` and `negative_fade_in_starts_the_clip_earlier` (Task 7);
     - `reads_the_hand_other_file` (Task 6).
2. **A hostile or broken package.**
   - Examples:
     - entry names `../x`, `/x`, `C:x`, `a\..\b`;
     - duplicate names, symlink entries, encrypted entries;
     - a 50 MiB XML that deflates from 10 KiB;
     - an entry that inflates past its declared size;
     - a central directory claiming 65 535 entries;
     - a `DOCTYPE` with entity expansion, 300 levels of nesting;
     - times `1e300`, `NaN`, `-0`, `inf`;
     - IDREF cycles through `reference`;
     - an audio entry bigger than the free disk.
   - Expected: refused with one sentence, or imported with the bad items left out and listed. Never a panic; each case under 2 s and 256 MiB.
   - Pinned by Task 1's and Task 2's hostile tests, `dawproject_fuzz.rs` and `dawproject_limits.rs` (Task 8), and `not_enough_space_refuses` (Task 10).
3. **Plug-ins that cannot travel as they are.**
   - Examples:
     - an instrument that is not installed here;
     - a VST3 instrument on a note track;
     - an AU state that is not a property list;
     - a `State` path missing from the ZIP;
     - one plug-in instrument played by three tracks;
     - a placeholder slot with saved state at export;
     - a slot with no state at all.
   - Expected:
     - a not-installed plug-in becomes a placeholder that keeps its settings;
     - VST plug-ins are Left out by name, and the track plays the built-in synth (a Note);
     - unreadable state is Left out by plug-in name;
     - a shared instrument is copied to each track with its state (Changed);
     - a placeholder's saved state is written (a Note).
   - Pinned by `plugins_map_or_wait` (Task 6), `shared_strips` and `plugin_states` (Task 4), `clap_state_round_trips` (Task 10) and `au_state_survives_the_aupreset_conversion` (Task 9).
4. **VizDAW-specific shapes on the way out and back.**
   - Examples:
     - a crossfade;
     - a pickup before bar 1;
     - a tempo ramp;
     - a muted reference use;
     - one sequence used three times, once transposed;
     - a region with gain;
     - a track holding notes and audio;
     - overlapping regions with no crossfade.
   - Expected: after export and import, the projection is equal (exact rows) or within the stated tolerance, and the report lists exactly the expected kinds.
   - Pinned by `round_trips_preserve_the_projection` and the fixed cases in `dawproject_round_trip.rs` (Task 8).
5. **Size and odd text.**
   - Examples:
     - 1 000 tracks, a 50 000-note clip;
     - names with `<>&"'`, tabs, newlines, U+0001, bidirectional overrides and emoji;
     - an empty track name;
     - a colour `#zzz`;
     - a 70 000-byte song title.
   - Expected: the export escapes everything and stays schema-valid. The import sanitises and cuts with a Changed line. A 1 000-track project exports and imports in under 10 s in debug.
   - Pinned by `names_are_escaped` (Task 4), `writer_escapes_and_round_trips` (Task 2), `hostile_text_is_sanitised` (Task 6) and `a_thousand_tracks` (Task 8).

## Plan Rulings

These are this plan's decisions where the spec leaves a choice. Each names its cost if wrong.

1. **Crates (spec §5 table).** `rawzip` + `flate2` replace the spec's `zip`, for the reasons in Tech Stack. `quick-xml` is the spec's pick. *Cost if wrong:* swapping one crate behind `core-io::zip`'s six functions.
2. **The schema version we write and check.** We write and validate against `main`'s `Project.xsd` (at `ee4dcdd`). It is the only one with `Send/Enable` and `Clip@enable`, which the spec's muted clips and send enables need.
   - Import reads files from either version.
   - `Project@version` "1.x" is read. Any other major is refused: "This DAWproject file is version <v>; VizDAW reads version 1."
   - *Cost if wrong:* a reader that validates strictly against the `v1.0.0` tag would refuse files with a muted clip or a send enable. Dropping both is one option flag.
3. **File time 0 is `midi_out::file_start(s)`, the pickup's start** (6a Ruling 8), so no time in the file is negative.
   - The record keeps the origin tick.
   - A song with a pickup gets the Note "The song starts before bar 1; other DAWs show it from the pickup's start."
   - Import without the record puts file time 0 at tick 0, with no start offset.
   - *Cost if wrong:* one offset.
4. **Times.**
   - The arrangement's `Lanes` is `timeUnit="beats"`. Beats are `ticks / 15 360` written as the shortest round-trip decimal (`xml::fmt_double`), so ticks come back exactly.
   - **Audio clips:**
     - `contentTimeUnit="seconds"`, with `playStart = source_offset / rate` and `playStop = (source_offset + length) / rate`, so frames come back exactly (`round(s × rate)`);
     - `duration` is in beats, from the tempo map;
     - `fadeTimeUnit="seconds"`.
   - Import prefers `playStop − playStart` over `duration` for an audio clip's length when both are there.
   - *Cost if wrong:* a reader that ignores `playStop` sees a length within one frame.
5. **IDs.**
   - XML IDs are derived from VizDAW objects, not from position:
     - a track `t<lane>` (its audio part `t<lane>a`);
     - a channel `c<strip>`, or `c<strip>t<lane>` for a shared strip's copy, or `k<lane>` for a track with no strip of its own (external MIDI, or a track whose audio path became a submix);
     - a bus track `b<strip>`; the master track `master`;
     - on a channel `<ch>`: `<ch>v`, `<ch>p` and `<ch>m` for volume, pan and mute; `<ch>s<i>`, `<ch>s<i>e` and `<ch>s<i>v` for a send, its enable and its volume; `<ch>d<instance>` and `<ch>d<instance>e` for a device and its enable; `<ch>d<instance>q<param>` for a plug-in parameter;
     - `tempo` and `timesig` in the transport.
   - Timelines, clips, notes and markers get no `id` (nothing references them).
   - Deterministic, collision-free, and forward references need no second pass.
   - *Cost if wrong:* a renumbering pass.
6. **Tracks and strips (spec §7.1, §7.4).**
   - Each VizDAW track becomes a `Track`. A track with both MIDI and audio events splits into "<name>" (notes, `t<lane>`) and "<name> audio" (`t<lane>a`), Changed.
   - **A track's channel is its strip:**
     - the `Instrument`/`BuiltinSynth` strip of its destination for notes;
     - the `AudioPath` strip of its `audio_out` for audio.
   - **A strip several tracks share:**
     - *An instrument strip* (plug-in or built-in synth) is copied to each track's channel, devices and state included. Changed: "Instrument '<strip>' shared by <n> tracks was copied to each".
     - *An audio path* becomes one `Channel role="submix"` named after the strip, wrapped in a `Track contentType="audio"` with ID `b<strip>` (the channel's ID is `c<strip>`). The tracks' own channels are plain (unity, centre, no devices) with `destination` = the submix. Changed: "Audio paths shared by several tracks became submix channels".
   - A track on an external MIDI destination gets a plain channel `k<lane>` to the master. Note: "Track '<name>' plays an external MIDI destination; the file has no instrument for it".
   - The built-in synth gets the same, with "plays the built-in synth here".
   - Buses become `Track contentType="audio"` + `Channel`: role `submix` when a strip outputs to them, else `effect`. The master is `Track id="master" contentType="audio notes"` + `Channel role="master"`. Bare `Channel`s are read on import too.
   - Order in `Structure`: tracks in sequence order, then buses in mixer order, then the master.
   - *Cost if wrong:* the shared-audio-path case is one function.
7. **Devices.**
   - Order on a channel: the instrument (role `instrument`), the EQ slot, then the inserts (role `audioFX`), as VizDAW processes them.
   - AU is `AuPlugin`, CLAP is `ClapPlugin`:
     - `deviceID` is `PluginRef.id` as VizDAW stores it (AU `type/subtype/manufacturer` four-char codes, CLAP's plug-in ID);
     - `deviceName`, `deviceVendor` (omitted when empty) and `pluginVersion` come from `PluginRef`;
     - `loaded="true"`; `Enabled` is `!bypass`.
   - `Parameters` lists only the parameters automation targets, as `RealParameter unit="normalized" min="0" max="1"` with `parameterID` = the parameter's 32 bits as an `xs:int` (two's complement). A `ParamId` over `u32::MAX` is Left out with its automation.
   - **Tags:** the EQ slot's device comment is `vz:q<instance>`; an insert's is `vz:d<instance>`. Import puts a `vz:q`-tagged device back in the EQ slot, and every other audio effect in the inserts.
   - Sidechains are Left out. Instrument extra outputs (`InstrumentOutput` strips) are Left out by strip.
   - *Cost if wrong:* the import's EQ rule is one branch.
8. **Plug-in state files.**
   - Entries are `plugins/NNN.aupreset` and `plugins/NNN.clap-preset`, numbered from 001 in write order.
   - **CLAP:** the raw state stream.
   - **AU:** `PlistCodec::to_xml` of the stored binary property list (spec §7.4). Import runs every AU state through `to_binary`; a failure is Left out "Settings of plug-in '<name>'".
   - **No state:**
     - a slot with no stored state and none captured: no `State`, with the Note "Plug-in '<name>' has no saved settings";
     - a placeholder slot (not loaded) with stored state: that state is written, with the Note "Plug-in '<name>' is not loaded here; its saved settings were written".
   - `NoPlist` (Linux, dry runs and tests) refuses both ways.
   - *Cost if wrong:* a file-format guess about `.clap-preset`: the Reference names no container, so other hosts may expect their own.
9. **Audio entries.**
   - Entries are `audio/NNN.wav`, numbered from 001; the human name is `Audio@name`. Numbered ASCII names cannot clash or traverse, and need no UTF-8 name flag.
   - One WAV per (media item, applied gain) pair: with `region_gain: apply`, a region with gain gets its own copy; regions at 0 dB share their media's entry.
   - **Sample format:**
     - the media item's rate, the project depth (32-bit float WAV for float projects), no dither;
     - stored uncompressed, with every XML and JSON entry deflated (spec §7.4).
   - *Cost if wrong:* other DAWs' media pools show "001.wav"; readable names are one function (an owner question).
10. **Crossfades (spec §7.0).**
    - **Export:** a crossfading region B over A writes overlapping clips: B's `fadeInTime` is its crossfade length, and A's `fadeOutTime` is `max(A.fade_out, overlap)`. Changed: "Crossfades were written as overlapping clips; the earlier clip's fade-out covers the overlap". Other DAWs hear A fade over the whole overlap even when B's fade-in is shorter (S4).
    - **Import:** where clip B starts inside clip A on one track, B's fade-in is at least 1 frame and at most the overlap + 1 frame, and A's fade-out is at least the overlap − 1 frame, B becomes a crossfade:
      - `fade_in` = `min(B's fade-in, room)`, Linear;
      - A's fade-out is cleared when it was within one frame of the overlap.

      The pair is inserted as two specs in one `EventCmd::Insert` (6c F22).
    - A negative `fadeInTime` is read as the Reference says: the clip starts `|f|` earlier with a fade-in of `|f|`.
    - *Cost if wrong:* the owner may prefer the negative-`fadeInTime` form on export (an owner question).
11. **Note clips (spec §7.4 "along the arrangement's blocks").**
    - The export takes a `ClipRule`, which `core-ffi` maps from the window's `BlockRule`. The rule is applied to each track's *played* top-level MIDI events: `SilenceGap(t)` starts a clip after a gap of at least `t`; `FixedBars(n)` makes n-bar windows from bar 1; `WholeTrack` makes one clip.
    - Each top-level reference use is its own clip named after its sequence. It loops (`loopStart` 0, `loopEnd` the pass length, `duration` the use's length) when every pass is the same; otherwise it is unrolled.
    - A muted use is `enable="false"`.
    - A transposed use is written as it plays, with the Note "Transposed uses of a sequence were written as played".
    - *Cost if wrong:* clip boundaries only; the notes are the same.
12. **Per-note expression (spec §7.1).** A note's expressions are its one child timeline: a single `Points`, or a `Lanes` of `Points` in the order transpose, timbre, pressure, gain, pan.
    - **Point times:** relative to the note's start, in beats. The Reference does not say; this is how VizDAW stores `PerNoteCtl.offset`.
    - **Point values:** `RealPoint`s with `interpolation="hold"` (VizDAW's per-note values are steps).
    - **Mappings:**
      - per-note pitch bend → `transpose` (`unit="semitones"`, `(v − 2³¹) × 48 / 2³¹`);
      - `Registered(74)` → `timbre`, `Registered(7)` → `gain`, `Registered(10)` → `pan` (normalised `v / (2³² − 1)`);
      - `PolyPressure` events on the note's pitch and channel inside its span → `pressure` points;
      - other per-note kinds and note attributes are Left out.
    - *Cost if wrong:* a reader that takes absolute times places expression late; one offset.
13. **MIDI controller points.**
    - Inside a clip, controller events become `Points` per (expression, channel, number), with `RealPoint unit="normalized" interpolation="hold"`:
      - `Cc` → `channelController`;
      - `PitchBend` → `pitchBend` (0.5 is the centre);
      - `ChannelPressure` → `channelPressure`;
      - `PolyPressure` outside a note → `polyPressure` with `key`.
    - `Program` is `IntegerPoint`s under `programChange`, its bank as `channelController` 0 and 32 points at the same time.
    - A clip with notes and points holds a `Lanes` of `Notes` and `Points`.
    - A destination's default patch and MIDI mix are written as track-level `Points` at time 0 (program, CC 0/32, CC 7, CC 10).
    - SysEx, texts, lyrics and cues are Left out with counts.
    - *Cost if wrong:* none to the notes.
14. **Automation.**
    - Each played `Automation` event goes into a `Points` in its top-level track's `Lanes`, targeting the parameter's ID:
      - fader and send level: `unit="linear"`, `linear_gain(fader_db(v))`, `interpolation="linear"`;
      - pan: `unit="normalized"`, the value as is;
      - mute: `BoolPoint`;
      - plug-in parameter: `unit="normalized"`.
    - On a shared instrument strip copied per track, `Points@parameter` names the copy of the track that holds the automation event.
    - Solo and velocity-scale automation are Left out (the format has no solo parameter).
    - **Import:**
      - `hold` adds a point one tick before the next, with the held value. Changed: "Held automation steps were written as one-tick ramps".
      - Other units scale by the parameter's finite `min`/`max`, else are Left out.
      - Points with no track go on the track that owns their parameter's channel, else on one new track "Automation" with no destination or audio path (a Note).
    - *Cost if wrong:* exact at points; the curve between may differ slightly (spec §7.1).
15. **Units, colours and tags.**
    - **Levels:** volume is `linear` (`10^(dB/20)`, max `10^(12/20)`); pan is `normalized` (`(p + 1) / 2`).
    - **Colours:** a swatch is written from a fixed table, independent of the theme: Red `#e5484d`, Orange `#f76b15`, Yellow `#ffc53d`, Green `#46a758`, Teal `#12a594`, Blue `#0090ff`, Purple `#8e4ec6`, Pink `#d6409f`. Import takes the nearest swatch by RGB distance, Changed unless exact. A malformed colour is Left out.
    - **Tags (spec §7.8):** `vz:t<lane>` on `Track` (also the audio part), `vz:s<strip>` on `Channel` (copies too), `vz:e<event>` on `Clip`, `vz:m<index>` on `Marker`, and `vz:d`/`vz:q` on devices. Import strips every `vz:<letter><digits>` token before it uses a comment.
    - The metadata `Comment` gets `VizDAW export <export id>` as its last line (the whole text when the user's comment is empty), and import strips it.
    - *Cost if wrong:* tag letters only.
16. **The record and the snapshot (spec §7.8, write side).**
    - **`vizdaw/record.json`:** `{format: 1, project_id, export_id, exported_at (Unix seconds), app_version, seq, origin, ids: {<xml id>: {kind, id}}, document: <project.json as JSON>}`. The whole document is a superset of "the VizDAW-only state", which is simplest and lossless for 6e. Its plug-in blobs are the `plugins/` entries.
    - **The snapshot:** `Interchange/<export id>.json` in the package is `{format: 1, project_id, export_id, exported_at, seq, origin, ids, projection}`. `projection` is Task 4's `Projection` of what was exported.
    - **Export IDs:** `project_id::fresh()`. A project with no `project_id` gets one stamped at export (`History::stamp_project_id`, no undo step; the title shows Edited).
    - **Save:** `save_to` makes the package's `Interchange/` when needed (`create_dir_all`, a link refused; L6), copies the root's snapshots into a new package (regular files named `<uuid>.json`, lstat, links skipped), then prunes the package's `Interchange/` to the 10 newest by modification time (ties by name). A failure is a news problem, never a failed save.
    - **Import (6d):** reads only `RecordHead {format, project_id, export_id}`. A VizDAW file gets the Note "This file came from a VizDAW project; it was opened as a new project".
    - *Cost if wrong:* 6e may want a smaller record; one projection call.
17. **Import as a new project only (spec §6.1, owner scope for 6d).**
    - `import_dawproject` refuses a document with any track: "A DAWproject opens as a new project." The app always makes a fresh project first, at `DawprojectInfoRow.sample_rate`: the majority rate of the file's `Audio` elements among `AudioFormat::RATES` (ties: the higher count, then 48 000), else 48 000 with the Note "The file's audio rate isn't a project rate; the project is 48 kHz".
    - `mode` takes only `"new"` until 6e.
    - *Cost if wrong:* 6e extends the request with `#[uniffi(default)]` fields.
18. **Package limits (spec §6.5), constants in `limits.rs`:**
    - 10 000 entries;
    - entry names of at most 1 024 bytes;
    - `project.xml` at most 256 MiB; `metadata.xml` at most 1 MiB; `vizdaw/record.json` at most 256 MiB; a state file at most 64 MiB;
    - XML at most 200:1, by declared and by actual inflated size;
    - XML depth 256; 4 000 000 nodes and 128 MiB of tree (arena and nodes, counted as they grow; past either, "it has too many parts"); 64 attributes per element. So a parse holds at most the XML text (256 MiB) plus 128 MiB, and the hostile-case budget is "under 2 s and 256 MiB" for fuzz cases, whose XML is under 64 MiB (L7);
    - attribute and text values at most 64 KiB (`MAX_TEXT_BYTES`);
    - numbers at most 64 characters;
    - timeline nesting (clips in clips, warps, note children, reference chains) at most 16.

    **Refusals of the whole file:**
    - names (empty component, `.`, `..`, leading `/`, a `\`, a `:` in the first component, NUL, too long);
    - exact duplicates, symlink entries, more entries than the limit;
    - `project.xml` missing, encrypted, too big or too tightly packed;
    - any DOCTYPE or unknown entity, the depth or node limit;
    - duplicate IDs, reference cycles, and a major version other than 1.

    **Items left out and listed:**
    - an encrypted or non-DEFLATE audio or state entry;
    - a dangling or wrong-kind IDREF;
    - a bad number or a missing required attribute;
    - deeper nesting;
    - `metadata.xml` that fails its own limits.

    Before extracting, `needed_bytes` must fit `free_bytes` of the package's volume (L10): every referenced entry's declared size (the extracted copy) plus one converted CAF per audio source (entries and regular external files, sized from `metadata` in `open`), each CAF sized as frames × channels × the project depth's bytes from the `Audio` element (with its file size × 4 as the floor when the element lies), else "Not enough free space to import this file (it needs <n> MB)."
    *Cost if wrong:* constants.
19. **What `core-io` returns to `core-ffi`.** `core-io` never decodes audio.
    - The import job extracts each referenced entry to `Media/.incoming/<fresh>.<ext>` (`create_new`), converts it with `import_reserved`, and removes the extracted copy.
    - External files may be anywhere a relative `..` or absolute path points (the format allows it); they are read only when the path is not a URL (`scheme://`) and `metadata` (one symlink hop) says it is a regular file. They are never written.
    - Failures become `MediaSource::Offline { why }`: an offline media item (path `Media/<safe name>.caf`, the file's rate, channels and length), relinkable, and listed: "Audio files that could not be read (they are offline; relink them)".
    - A source wider than stereo becomes several mono items. The first stays on the clip's track; each other channel's region goes on a new track "<track> ch <n>" (Changed).
    - *Cost if wrong:* the multichannel rule only.

## Pre-flight amendments (2026-11-12)

These come from `.superpowers/sdd/2026-11-12-dawproject/preflight-report.md`, and the controller accepted every proposed ruling. Each is applied in place in the task it touches; where a step and an older line disagree, the amendment wins.

- **L1** DAWproject export gets its own `JobKindRow::ExportDawproject` (6b's `ExportProject` stays "Export for another DAW"); both new kinds join every Swift `switch` and `==` chain, `interchange_idle` and `ForDawModel.blocking` (Global Constraints, Tasks 9, 10, 12).
- **L2** `FixedBars(1)` gives 2 note clips; the Riff uses are separate (Task 5).
- **L3** `core-io` gains a path dev-dependency on `core-engine` with `fixtures` (Task 1).
- **L4** `debug_plugin_param(slot, param)` reads a parameter for `clap_state_round_trips` (Task 10).
- **L5** A looped clip whose `playStart` is not its `loopStart` is unrolled, with Changed; the sequence's length is `loopEnd − loopStart` (Task 7).
- **L6** `Interchange/` is created by the snapshot writer and by `keep_snapshots`; tested on a package without it (Ruling 16, Task 9).
- **L7** The tree is capped at 4 000 000 nodes and 128 MiB; a limits test parses at the cap under the allocator (Ruling 18, Tasks 1, 8).
- **L8** `dawproject_info` is a free UniFFI function; `openDawproject` returns at once and works with no document open; `open(path:)` returns `nil` for it (Tasks 10–12).
- **L9** Cancel is tested inside `Tree::parse`, `map` and `export` (Tasks 2, 4, 6).
- **L10** `needed_bytes` counts entries, external files and one CAF per source at the project depth (Ruling 18, Tasks 6, 8, 10).
- **L11** Track mute and solo get their report Notes on export (Task 4).
- **S1** The lockfile and notice lists come from `cargo tree` (Tasks 1, 13).
- **S2** The root README says Rust 1.86 (Task 1).
- **S3** Schema types are prefixed `Dp` (Task 3).
- **S4** The crossfade Changed line says the earlier fade covers the overlap (Ruling 10, Task 5).
- **S5** `file_path` takes the noun for its refusal (Task 10).
- **S6** `export_dawproject` checks the staging folder's name length (Task 9).
- **S7** The e2e expects `InvalidParams` for a relative path; the Swift hostile tests bypass the registry (Tasks 11, 13).
- **S8** Allocation counts are per thread (Tasks 1, 8).
- **S9** A WAV copy past 4 GiB is Left out before staging (Task 9).
- **S10** Cancel tests clear `debug_free_bytes` (Task 10).
- **S11** No `id` is written on non-referenceable elements; tested (Task 3).
- **S12** `ZipReader::read` returns `TOO_LARGE` past `max` (Task 1).
- **S13** Task 5's fixture states ticks, frames and the crossfade command (Task 5).
- **S14** `DpInfo.tracks` counts note and audio tracks only (Task 6).
- **S15** The plan's base commit is noted (Global Constraints).
- **Also from the report:** a shared strip's copy owns its automation target (Ruling 14); external `..` paths are allowed by the format and read only as regular audio (Ruling 19); "Colours and comments on markers and tracks" is listed on export (Task 4); `hand_timeline`'s content is defined by Task 7's tests; Task 6 keeps the tempo map for Task 7.

---

### Task 1: Dependencies, package limits and `core-io::zip`

**Files:**
- Modify:
  - `Cargo.toml` (`rust-version = "1.86"`);
  - `crates/core-io/Cargo.toml`:
    - `rawzip = "0.5.2"`;
    - `flate2 = { version = "1.1.11", default-features = false, features = ["rust_backend"] }`;
    - `quick-xml = { version = "0.42.0", default-features = false }`;
    - `serde = { version = "1", features = ["derive"] }`;
    - `serde_json = "1"`;
    - `[dev-dependencies] core-engine = { path = "../core-engine", features = ["fixtures"] }` (Tasks 4–8 build fixtures with `core_engine::fixtures::Builder`; L3);
  - `README.md` ("Rust 1.86 or newer"; S2);
  - `Cargo.lock`; `crates/core-io/src/lib.rs` (`pub mod zip;`, `IoError` unchanged); `src/limits.rs`; `src/read.rs`.
- Create: `crates/core-io/src/zip.rs`, `crates/core-io/tests/common/mod.rs`, `crates/core-io/tests/common/rawzip_builder.rs`, `crates/core-io/tests/zip.rs`.

**Interfaces:**
- Produces:
  - ```rust
    // limits.rs: one `pub const MAX_…` per value of Ruling 18 (MAX_ZIP_ENTRIES, MAX_ENTRY_NAME_BYTES, MAX_XML_RATIO,
    // MAX_PROJECT_XML_BYTES, MAX_METADATA_XML_BYTES, MAX_RECORD_BYTES, MAX_STATE_BYTES, MAX_XML_DEPTH, MAX_XML_NODES,
    // MAX_XML_TREE_BYTES, MAX_XML_ATTRS, MAX_NUMBER_CHARS, MAX_TIMELINE_NESTING)
    /// The limits a package reader applies; tests lower them.
    pub struct PackageLimits { pub max_entries: usize, pub max_project_xml: u64, pub max_metadata_xml: u64,
        pub max_record: u64, pub max_state: u64, pub max_ratio: u64, pub max_depth: usize, pub max_nodes: usize, pub max_tree_bytes: usize }
    impl Default for PackageLimits { /* the constants */ }
    // read.rs
    /// `path` opened as a regular file (one symlink hop; FIFO, device, directory refused), checked again on the handle.
    pub fn open_regular(path: &Path) -> Result<std::fs::File, IoError>;   // read_regular is rebuilt on it
    // zip.rs
    pub enum EntryMethod { Stored, Deflated }
    pub struct EntryInfo { pub name: String, pub method: Option<EntryMethod>, pub encrypted: bool, pub size: u64, pub compressed: u64 }
    pub struct ZipReader { /* rawzip archive over the file, the checked entries */ }
    impl ZipReader {
        pub fn open(path: &Path, limits: &PackageLimits) -> Result<ZipReader, IoError>;
        pub fn entries(&self) -> &[EntryInfo];
        pub fn find(&self, name: &str) -> Option<&EntryInfo>;
        /// Inflates `e` (CRC and size verified). Refused with `TOO_LARGE` past `max` bytes (S12), and,
        /// with `ratio`, when declared or actual size / compressed > ratio.
        pub fn read(&self, e: &EntryInfo, max: u64, ratio: Option<u64>, cancel: &AtomicBool) -> Result<Vec<u8>, IoError>;
        /// Streams `e` into a new file `to` (`create_new`); `to` is removed on any error or cancel.
        pub fn extract(&self, e: &EntryInfo, to: &Path, cancel: &AtomicBool) -> Result<u64, IoError>;
    }
    pub struct ZipWriter<W: Write> { /* rawzip ZipArchiveWriter */ }
    impl<W: Write> ZipWriter<W> {
        pub fn new(out: W) -> Self;
        pub fn add(&mut self, name: &str, method: EntryMethod, bytes: &[u8]) -> Result<(), IoError>;
        pub fn add_from(&mut self, name: &str, method: EntryMethod, src: &mut dyn Read, cancel: &AtomicBool) -> Result<u64, IoError>;
        pub fn finish(self) -> Result<W, IoError>;
    }
    /// Ruling 18's name rules; `Err` is the reason fragment.
    pub fn check_name(name: &str) -> Result<(), &'static str>;
    pub const NOT_DAWPROJECT: &str = "This isn't a DAWproject file.";
    pub const TOO_LARGE: &str = "This DAWproject file is too large to open.";
    ```
  - **Refusal text.** Damage is "This DAWproject file is damaged: <why>.", where `<why>` is:
    - "an entry name leaves the package";
    - "two entries are named '<name>'";
    - "it holds a link";
    - "it holds more than 10 000 files";
    - "<name> unpacks to more than 200 times its size";
    - "<name> is larger than it says".

    Unreadable entries (`read` and `extract` only; `open` succeeds) are "<name> is encrypted" and "<name> uses a compression VizDAW can't read".
  - **`open`:**
    - refuses before iterating when `entries_hint()` is over the limit;
    - counts while iterating, and stops past the limit;
    - skips directory entries (a name ending in `/`) after checking them;
    - treats Unix mode `0o120000` in the external attributes (with a Unix "version made by") as a link.

    `add`/`add_from` write no modification time (`rawzip`'s default), so output bytes are deterministic.
  - `tests/common/rawzip_builder.rs` (test-only): `fn raw_zip(entries: &[RawEntry]) -> Vec<u8>` writes local headers, the central directory and the EOCD byte by byte. Each `RawEntry { name: Vec<u8>, data: Vec<u8>, method: u16, flags: u16, declared_size: u32, crc: Option<u32>, unix_mode: Option<u32>, zip64_size: Option<u64> }` lets a test lie about any field.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it in the task report.
- [ ] **Step 2: Write the failing tests** in `tests/zip.rs` (with `mod common;`):
  - `writer_output_reads_back`: three entries (Stored "a.txt" "hi"; Deflated "dir/b.xml" of 100 KiB of text; Stored "audio/001.wav" of 3 bytes) read back byte-equal, with `EntryInfo` sizes and methods right. Two writes of the same entries give equal bytes.
  - `names_that_leave_the_package_are_refused`, each `open` → `Refused` containing "an entry name leaves the package": `../x`, `/abs`, `C:x`, `a/../../b`, `a\..\b`, `x\0y`, `a//b`, `./x`, and a 1 025-byte name. Then `check_name("audio/001.wav")` and `check_name("plugins/001.aupreset")` are `Ok`.
  - `duplicate_names_are_refused`: two "a.txt" → "two entries are named 'a.txt'".
  - `symlink_entries_are_refused`: Unix mode `0o120777` → "it holds a link".
  - `entry_count_limits`:
    - 10 000 entries open;
    - 10 001 are refused;
    - an EOCD claiming 65 535 entries over a 2-entry directory is refused before iterating, and a thread-local counting allocator sees under 1 MiB allocated by `open` on the test's own thread (S8).
  - `xml_bombs_are_refused`:
    - a Deflated entry of 50 MiB of zeros, read with `ratio: Some(200)` → "unpacks to more than 200 times";
    - the same entry with a declared size of 1 KiB → "is larger than it says" after at most 1 KiB + one buffer is held;
    - with `ratio: None` and `max` 64 MiB, the 50 MiB entry reads.
  - `encrypted_and_unknown_methods_are_unreadable`: flags bit 0, and method 12; `open` is `Ok`, and `read`/`extract` give the two sentences.
  - `damaged_bytes_never_panic`: every prefix of a valid 3-entry ZIP, every single-byte flip of it, and 4 KiB of random bytes each give `Ok` or `Refused`, never a panic.
  - `zip64_edge_sizes_never_panic`: sizes `0xFFFF_FFFF` with a ZIP64 extra claiming 2⁶³, and a declared size of 0 with 1 MiB of data. Each is `Refused` or an `Err` from `read`, allocating under 2 MiB.
  - `extract_is_bounded_and_cleans_up`:
    - a wrong CRC → `Err`, with `to` gone;
    - a cancel set from a second thread after the first chunk → `Cancelled`, with `to` gone;
    - an existing `to` → refused, untouched.
  - `open_refuses_what_is_not_a_regular_file`: a directory, a FIFO (`mkfifo` through `libc`-free `std::process::Command("mkfifo")`) and a symlink loop.
  - `random_bytes_never_panic`: proptest with `cases(256)` (copy 6a's `cases` reading `VIZDAW_FUZZ_CASES`).
- [ ] **Step 3: Run** `cargo test -p core-io --test zip`. Expected: fail to compile.
- [ ] **Step 4: Implement** the items above. `read` caps its `Vec` growth at `min(max, declared) + 64 KiB` and counts every inflated byte. `extract` copies in 64 KiB chunks through `verifying_reader`.
- [ ] **Step 5: Run** `cargo test --workspace`, both clippies (the Linux one proves `rawzip` and `flate2` build there). Expected: PASS (about +12 Rust). `git diff --stat Cargo.lock` lists only the new crates; `cargo tree -p core-io -e normal` gives their licences for this plan's lists (S1).
- [ ] **Step 6: Commit** `feat(io): bounded ZIP reader and writer, package limits (plan 6d Task 1)` with every file above.

### Task 2: `core-io::xml`: the bounded tree reader, strict numbers and the escaping writer

**Files:**
- Create: `crates/core-io/src/xml.rs`, `crates/core-io/tests/xml.rs`
- Modify: `crates/core-io/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 (`PackageLimits`).
- Produces:
  - ```rust
    #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct NodeId(u32);
    /// An element tree: names and attribute values in one string arena; text kept
    /// only up to MAX_TEXT_BYTES per element (MetaData's fields).
    pub struct Tree { /* arena */ }
    impl Tree {
        pub fn parse(text: &[u8], limits: &PackageLimits, cancel: &AtomicBool) -> Result<Tree, IoError>;
        pub fn root(&self) -> NodeId;
        pub fn name(&self, n: NodeId) -> &str;
        pub fn attr(&self, n: NodeId, name: &str) -> Option<&str>;
        pub fn children(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_;
        pub fn text(&self, n: NodeId) -> &str;
        pub fn len(&self) -> usize;
    }
    pub fn double(s: &str) -> Option<f64>;        // XSD lexical, finite, ≤ MAX_NUMBER_CHARS, surrounding whitespace trimmed
    pub fn double_or_inf(s: &str) -> Option<f64>; // also `inf`, `-inf`, `INF`, `-INF`
    pub fn int(s: &str) -> Option<i32>;           // `[+-]?[0-9]+`, in range
    pub fn boolean(s: &str) -> Option<bool>;      // `true` `false` `1` `0`
    pub fn fmt_double(v: f64) -> String;          // shortest round-trip (Rust `{}`), "-0" as "0"; never NaN or inf
    pub enum Attr<'a> { Str(&'a str), F64(f64), I64(i64), Bool(bool) }
    pub struct XmlWriter { /* String, open-element stack */ }
    impl XmlWriter {
        pub fn new() -> Self; // `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` + "\n"
        pub fn open(&mut self, name: &str, attrs: &[(&str, Attr)]);
        pub fn empty(&mut self, name: &str, attrs: &[(&str, Attr)]);
        pub fn text_element(&mut self, name: &str, text: &str);
        pub fn close(&mut self);
        pub fn finish(self) -> String;   // closes nothing; a still-open element is a bug (debug_assert)
    }
    ```
  - **The reader's refusals.** Each is "This DAWproject file is damaged: <why>.", where `<why>` is:
    - "it declares a document type" (any `DocType`);
    - "it uses an unknown entity" (any entity reference other than `amp lt gt quot apos` and character references);
    - "it is not UTF-8" (an `encoding` other than UTF-8 in any case, a UTF-16 BOM or invalid UTF-8);
    - "it is nested too deeply";
    - "it has too many parts";
    - "an element has too many attributes";
    - "an element repeats an attribute";
    - "an attribute is longer than 64 KiB";
    - "its XML is broken" (mismatched or unclosed tags, two roots, text after the root).

    A UTF-8 BOM is skipped. Comments, processing instructions and CDATA are accepted (CDATA as text). The cancel flag is looked at every 65 536 events.
  - **Writer layout:**
    - two spaces of indent per level, one element per line, `/>` for empty elements;
    - attributes in the order given;
    - escaping:
      - `& < > "` in attributes and `& < >` in text;
      - tab, newline and CR in attributes as `&#9;` `&#10;` `&#13;`;
      - other C0 controls, U+FFFE and U+FFFF dropped.

- [ ] **Step 1: Write the failing tests** in `tests/xml.rs`:
  - `parses_a_small_project`: a hand-written 20-line text with nested elements, attributes, a comment, a `<?pi?>` and `<Title>A &amp; B</Title>` gives the expected names, attributes, children order and text "A & B".
  - `doctype_and_entities_are_refused`: the billion-laughs text (a DOCTYPE with nested entities), `<a>&foo;</a>` with no DOCTYPE, and `<a x="&foo;"/>` are each refused with their sentence. `&#x41;&amp;` reads "A&".
  - `nesting_and_size_limits`: depth 256 parses and 257 is refused. With `max_nodes: 100`, 100 elements parse and 101 are refused. 64 attributes parse and 65 are refused.
  - `encodings`: a UTF-8 BOM and `encoding="utf-8"` are accepted. `encoding="UTF-16"`, a UTF-16 BOM and byte `0xFF` inside text are each refused.
  - `numbers_are_strict`:
    - `double`: "1.5e3" is 1500.0, ".5", "5." and " 2 " parse, "-0" is −0.0; "NaN", "INF", "1e400", "0x10", "1_0", "1,5", "" and 65 digits are `None`;
    - `double_or_inf("-inf")` is −∞;
    - `int`: "2147483647" and "-2147483648" parse, "2147483648" and "1.0" are `None`;
    - `boolean`: the four forms parse, "yes" and "TRUE" are `None`.
  - `structure_errors_are_refused`: `<a></b>`, `<a>`, `<a/><b/>`, `<a/>x` and `<a x="1" x="2"/>`.
  - `writer_escapes_and_round_trips`: a name of `<>&"'`, tab, newline, U+0001, U+202E and "🎹" written as an attribute and as text parses back equal, minus U+0001.
  - `fmt_double` with `double` round-trips exactly for 1/3, 0.1, 1e-300, 2⁵³, `15_360.0 / 15_360.0` (written "1") and `65_535.0 / 65_535.0`, and for every `n / 65_535.0` with n in 0..=65 535.
  - `parse_stops_when_cancelled` (L9): a pre-set flag returns `Cancelled` before the first element; a flag set from a second thread while a 300 000-element text parses returns `Cancelled` within one check interval (65 536 events).
  - `random_xml_never_panics`: proptest, `cases(512)`. A tag-soup generator mixes known DAWproject names, random attributes with `&`, `<`, entity references, DOCTYPE fragments and depth up to 300. Each case is `Ok` or `Refused` in under 2 s.
- [ ] **Step 2: Run** `cargo test -p core-io --test xml`. Expected: fail to compile.
- [ ] **Step 3: Implement.** A depth stack of `NodeId`s; names and values pushed into the arena as they arrive. The node count is checked before each push.
- [ ] **Step 4: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +10).
- [ ] **Step 5: Commit** `feat(io): bounded XML tree reader and escaping writer (plan 6d Task 2)`.

### Task 3: `dawproject::schema`: the typed model, reading and writing it, IDREFs, the vendored XSDs

**Files:**
- Create:
  - `crates/core-io/src/dawproject/mod.rs` (modules, re-exports);
  - `crates/core-io/src/dawproject/schema.rs`;
  - `crates/core-io/tests/dawproject_schema.rs`;
  - `crates/core-io/tests/data/dawproject/Project.xsd` and `MetaData.xsd`, copied byte for byte from commit `ee4dcdd` (the SHA-256s in this plan's format section, checked in the test);
  - `crates/core-io/tests/data/dawproject/LICENSE` (the MIT notice, verbatim);
  - `crates/core-io/tests/data/dawproject/hand_minimal.project.xml` and `hand_minimal.metadata.xml`, hand-written;
  - `crates/core-io/tests/common/xmllint.rs`: `fn schema_check(xsd: &str, xml: &str)`. It runs `xmllint --noout --schema <xsd> <tmpfile>` and asserts success. It is required on macOS; elsewhere it runs when `xmllint` is on `PATH` and prints "skipped: no xmllint" otherwise.
- Modify:
  - `crates/core-io/src/lib.rs`;
  - `crates/core-io/tests/data/README.md`: a "DAWproject (plan 6d)" section with:
    - the reference URL, the commit and the SHA-256s;
    - the licence;
    - how each hand file was written: by hand from this plan, element by element.

**Interfaces:**
- Consumes: Task 2.
- Produces the typed model. Every type derives `Debug, Clone, PartialEq`; optional XML attributes are `Option`. **Every schema type is prefixed `Dp`** (`DpNamed`, `DpTrack`, `DpChannel`, `DpSend`, `DpDevice`, `DpClip`, `DpNote`, `DpPoint`, `DpUnit`, …) so a glob import never shadows `Send` or a `core_model` name (S3). The blocks below omit the prefix for brevity, except `DpProject` and `DpMetaData`.
  - **Shared and parameter types:**
    ```rust
    pub struct Named { pub id: Option<String>, pub name: Option<String>, pub color: Option<String>, pub comment: Option<String> }
    pub enum Unit { Linear, Normalized, Percent, Decibel, Hertz, Semitones, Seconds, Beats, Bpm }
    pub enum TimeUnit { Beats, Seconds }
    pub struct RealParam { pub named: Named, pub parameter_id: Option<i32>, pub unit: Unit, pub value: Option<f64>, pub min: Option<f64>, pub max: Option<f64> }
    pub struct BoolParam { pub named: Named, pub parameter_id: Option<i32>, pub value: Option<bool> }
    pub struct TimeSigParam { pub named: Named, pub numerator: i32, pub denominator: i32 }
    ```
  - **The project, its structure and devices:**
    ```rust
    pub struct DpProject { pub version: String, pub application: Option<(String, String)>, pub tempo: Option<RealParam>,
        pub time_signature: Option<TimeSigParam>, pub structure: Vec<StructureItem>, pub arrangement: Option<Arrangement>, pub scenes: Vec<Scene> }
    pub enum StructureItem { Track(Track), Channel(Channel) }
    pub enum ContentType { Audio, Automation, Notes, Video, Markers, Tracks }
    pub struct Track { pub named: Named, pub content: Vec<ContentType>, pub loaded: Option<bool>, pub channel: Option<Channel>, pub children: Vec<Track> }
    pub enum MixerRole { Regular, Master, Effect, Submix, Vca }
    pub struct Channel { pub named: Named, pub role: Option<MixerRole>, pub audio_channels: Option<i32>, pub destination: Option<String>,
        pub solo: Option<bool>, pub devices: Vec<Device>, pub mute: Option<BoolParam>, pub pan: Option<RealParam>, pub sends: Vec<Send>, pub volume: Option<RealParam> }
    pub enum DeviceKind { Au, Clap, Vst2, Vst3, Builtin, Equalizer, Compressor, NoiseGate, Limiter, Other }
    pub enum DeviceRole { Instrument, NoteFx, AudioFx, Analyzer }
    pub struct FileRef { pub path: String, pub external: bool }
    pub struct Device { pub kind: DeviceKind, pub named: Named, pub device_id: Option<String>, pub device_name: String, pub role: DeviceRole,
        pub vendor: Option<String>, pub version: Option<String>, pub loaded: Option<bool>, pub parameters: Vec<RealParam>,
        pub enabled: Option<BoolParam>, pub state: Option<FileRef>, pub settings: Vec<String> /* built-in devices' bands etc., as text for the report */ }
    pub enum SendType { Pre, Post }
    pub struct Send { pub named: Named, pub destination: Option<String>, pub kind: Option<SendType>, pub enable: Option<BoolParam>, pub pan: Option<RealParam>, pub volume: RealParam }
    ```
  - **The arrangement and timelines:**
    ```rust
    pub struct Arrangement { pub named: Named, pub lanes: Option<Lanes>, pub markers: Vec<Marker>, pub tempo_automation: Option<Points>, pub time_signature_automation: Option<Points> }
    pub struct Head { pub named: Named, pub time_unit: Option<TimeUnit>, pub track: Option<String> }
    pub enum Timeline { Lanes(Lanes), Clips(Clips), Notes(Notes), Points(Points), Audio(Audio), Warps(Warps), Markers(Head, Vec<Marker>), ClipSlot(Head, Option<Box<Clip>>), Video(Head), Unknown(String) }
    pub struct Lanes { pub head: Head, pub items: Vec<Timeline> }
    pub struct Clips { pub head: Head, pub clips: Vec<Clip> }
    pub struct Clip { pub named: Named, pub time: f64, pub duration: Option<f64>, pub content_time_unit: Option<TimeUnit>, pub play_start: Option<f64>,
        pub play_stop: Option<f64>, pub loop_start: Option<f64>, pub loop_end: Option<f64>, pub fade_time_unit: Option<TimeUnit>,
        pub fade_in: Option<f64>, pub fade_out: Option<f64>, pub enable: Option<bool>, pub reference: Option<String>, pub content: Option<Box<Timeline>> }
    pub struct Notes { pub head: Head, pub notes: Vec<Note> }
    pub struct Note { pub time: f64, pub duration: f64, pub channel: i32, pub key: i32, pub vel: Option<f64>, pub rel: Option<f64>, pub content: Option<Box<Timeline>> }
    pub enum Expression { Gain, Pan, Transpose, Timbre, Formant, Pressure, ChannelController, ChannelPressure, PolyPressure, PitchBend, ProgramChange }
    pub struct Target { pub parameter: Option<String>, pub expression: Option<Expression>, pub channel: Option<i32>, pub key: Option<i32>, pub controller: Option<i32> }
    pub enum Interpolation { Hold, Linear }
    pub enum PointValue { Real(f64, Option<Interpolation>), Bool(bool), Integer(i32), Enum(i32), TimeSig(i32, i32) }
    pub struct Point { pub time: f64, pub value: PointValue }
    pub struct Points { pub head: Head, pub unit: Option<Unit>, pub target: Target, pub points: Vec<Point> }
    pub struct Audio { pub head: Head, pub duration: f64, pub channels: i32, pub sample_rate: i32, pub algorithm: Option<String>, pub file: FileRef }
    pub struct Warps { pub head: Head, pub content_time_unit: TimeUnit, pub content: Box<Timeline>, pub warps: Vec<(f64, f64)> }
    pub struct Marker { pub named: Named, pub time: f64 }
    pub struct Scene { pub named: Named, pub content: Option<Timeline> }
    pub struct DpMetaData { pub fields: [Option<String>; 13] }   // MetaData's order, which is SongInfo::fields' order
    ```
  - **Functions:**
    ```rust
    pub fn read_project(t: &Tree, report: &mut Report) -> Result<DpProject, IoError>;
    pub fn read_metadata(t: &Tree, report: &mut Report) -> DpMetaData;
    pub fn write_project(p: &DpProject) -> String;
    pub fn write_metadata(m: &DpMetaData) -> String;
    /// Every `id` once (else refused); each IDREF's target kind; reference cycles refused.
    pub fn check_ids(p: &DpProject, report: &mut Report) -> Result<Ids, IoError>;
    pub struct Ids { /* id -> what it names: Track, Channel, Parameter, Timeline, Device, Send */ }
    impl Ids { pub fn kind(&self, id: &str) -> Option<IdKind>; }
    pub enum IdKind { Track, Channel, Parameter, Timeline, Device, Send, Other }
    ```
  - **Reading rules:**
    - an element or attribute this model lacks is skipped and counted (Note, "Parts of the file VizDAW doesn't know");
    - a required attribute missing or unreadable leaves its element out (Left out, "Parts of the file that could not be read", with the element name);
    - the 17th level of timeline nesting is left out the same way;
    - a duplicate `id` → "This DAWproject file is damaged: two parts share the ID '<id>'.";
    - a cycle of clip `reference`s (including a clip whose reference holds itself) → "This DAWproject file is damaged: clips refer to each other in a loop.";
    - a dangling or wrong-kind IDREF stays in the model, and `check_ids` reports it (Left out, "References to parts that are not in the file"). The mappers then treat it as absent.
  - **Writing rules:**
    - XSD order; `id` is written only on referenceable types, never on `Clip`, `Marker`, `Note`, `Point` or `Scene` (their `named.id` is ignored; S11);
    - attributes in the order `id`, `name`, `color`, `comment`, then the element's own in the struct's field order;
    - `None` attributes omitted; `Markers` omitted when empty;
    - `Note@channel` always written.

- [ ] **Step 1: Write the hand fixtures** (before any code):
  - `hand_minimal.project.xml`:
    - `Application name="Fixture" version="1"`, a transport at 120 bpm (id `tempo`) and 4/4 (id `timesig`);
    - one notes `Track` "Keys" with a `Channel` (Mute, Pan, Volume, one Send), one CLAP device with `State path="plugins/001.clap-preset"`;
    - a master `Track`;
    - an `Arrangement` with one `Clips` holding a note clip with two notes, one note with a `Points` transpose child;
    - one `Markers` with "Verse".
  - `hand_minimal.metadata.xml`: Title, Artist and Comment.
- [ ] **Step 2: Write the failing tests** in `tests/dawproject_schema.rs`:
  - `xsds_are_the_pinned_ones`: the SHA-256 of both vendored files, from `shasum -a 256` (macOS) or `sha256sum` through `Command`, equals the values in this plan's format section. With neither tool on `PATH` it prints "skipped: no SHA-256 tool" (macOS always has `shasum`).
  - `reads_the_hand_minimal_project`: asserts the typed fields (names, units, values, IDREFs, the note expression's target and points).
  - `writes_the_hand_minimal_project_byte_for_byte`: `write_project(read_project(x)) == x`, and the same for metadata (`hand_golden`).
  - `written_projects_pass_the_vendored_schema`: `schema_check` on both hand files, and on a `DpProject` built in code with every element kind and an `id` set on every `named`, clips and markers included (S11).
  - `ids_are_checked`:
    - a duplicate `id` is refused;
    - a self-referencing clip and a two-clip loop are refused;
    - a dangling `Channel@destination`, a `Lanes@track` naming a `Channel` and a `Points` target naming a `Track` each give one "References to parts that are not in the file" detail.
  - `unknown_parts_are_noted`: a `<Future x="1"><Deep/></Future>` inside `Track` and an unknown attribute on `Clip` give the Note with count 2.
  - `unreadable_items_are_left_out`: a `Clip` without `time`, a `Note` with `key="x"`, a `RealPoint` with `value="NaN"` and nesting 17 deep each leave only their element out.
  - `versions`: "1.0" and "1.1" read; "2.0" → "This DAWproject file is version 2.0; VizDAW reads version 1."; a root that is not `Project` → `NOT_DAWPROJECT`.
- [ ] **Step 3: Run** `cargo test -p core-io --test dawproject_schema`. Expected: fail to compile.
- [ ] **Step 4: Implement** `schema.rs`.
- [ ] **Step 5: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +8).
- [ ] **Step 6: Commit** `feat(io): the DAWproject typed model, its reader, writer and ID checks; vendored XSDs (plan 6d Task 3)`.

### Task 4: Export: structure, mixer, plug-ins, metadata, tempo, meter, markers, tags, the record and the projection

**Files:**
- Create:
  - `crates/core-io/src/dawproject/units.rs`, `plugins.rs`, `record.rs`, `export.rs` (this task: everything but the arrangement's track content);
  - `crates/core-io/tests/dawproject_export.rs`;
  - `crates/core-io/tests/data/dawproject/export_structure.project.xml` and `export_structure.metadata.xml`, hand-written.
- Modify: `crates/core-io/src/midi_out.rs` (`source` and its `Source` become `pub(crate)`, with no other change).

**Interfaces:**
- Consumes: Tasks 2–3; `midi_out::{source, file_start, start_meters, start_bpm}`; `core_model::automation` laws; `core_model::file::to_json`.
- Produces:
  - **Options, environment and the export result:**
    ```rust
    pub enum RegionGain { Apply, LeaveOut }
    pub enum ClipRule { SilenceGap(Tick), FixedBars(u32), WholeTrack }
    pub struct ExportOptions { pub seq: SeqId, pub region_gain: RegionGain, pub clips: ClipRule }
    pub struct ExportEnv<'a> { pub plist: &'a dyn PlistCodec, pub app_version: &'a str, pub project_id: &'a str, pub export_id: &'a str, pub exported_at: u64 }
    pub struct AudioFile { pub entry: String, pub media: MediaId, pub gain_db: f32, pub name: String }
    pub struct DpExport { pub project_xml: String, pub metadata_xml: String, pub record_json: String, pub snapshot_json: String,
        pub audio: Vec<AudioFile>, pub states: Vec<(String, Vec<u8>)>, pub report: Report }
    pub fn export(doc: &Document, o: &ExportOptions, env: &ExportEnv, cancel: &AtomicBool) -> Result<DpExport, IoError>;
    /// The report `export` gives, with `NoPlist` and no record (the dry run).
    pub fn check(doc: &Document, o: &ExportOptions, cancel: &AtomicBool) -> Result<Report, IoError>;
    /// "<Song>.dawproject": `safe_part` of the title, else the sequence's name, reserving 11 bytes.
    pub fn file_name(doc: &Document, seq: SeqId) -> Result<String, IoError>;
    ```
  - **`units.rs`:**
    ```rust
    pub fn beats(t: Tick) -> f64;                         // t / 15 360
    pub fn ticks(beats: f64) -> Option<(Tick, bool)>;     // round half away; moved = off the grid by > 1e-6 tick; None past ±MAX_TICKS
    pub fn linear_gain(db: f64) -> f64;  pub fn db_of_linear(l: f64) -> f64;   // ≤ 0 → GAIN_FLOOR_DB
    pub fn pan_normalized(p: f64) -> f64; pub fn pan_of_normalized(n: f64) -> f64;
    pub fn swatch_hex(s: Swatch) -> &'static str; pub fn nearest_swatch(hex: &str) -> Option<(Swatch, bool)>;
    pub enum TagKind { Track, Strip, Event, Marker, Device, Eq }   // t s e m d q
    pub fn tag(kind: TagKind, id: u64) -> String;
    pub fn strip_tags(comment: &str) -> (String, Vec<(TagKind, u64)>);
    pub const EXPORT_LINE: &str = "VizDAW export ";
    ```
  - **`plugins.rs`:**
    ```rust
    pub trait PlistCodec: Send + Sync { fn to_xml(&self, binary: &[u8]) -> Result<Vec<u8>, String>; fn to_binary(&self, plist: &[u8]) -> Result<Vec<u8>, String>; }
    pub struct NoPlist; // both refuse: "converting Audio Unit settings needs macOS"
    pub fn state_entry(n: usize, format: PluginFormat) -> String; // "plugins/001.aupreset" | "plugins/001.clap-preset"
    ```
  - **`record.rs`:**
    ```rust
    pub const RECORD_ENTRY: &str = "vizdaw/record.json"; pub const SNAPSHOT_DIR: &str = "Interchange"; pub const KEEP_SNAPSHOTS: usize = 10;
    #[derive(Serialize, Deserialize)] pub struct ObjectRef { pub kind: String /* "track" "strip" "device" "parameter" */, pub id: u64 }
    #[derive(Serialize, Deserialize)] pub struct Record { pub format: u32, pub project_id: String, pub export_id: String, pub exported_at: u64,
        pub app_version: String, pub seq: u64, pub origin: i64, pub ids: BTreeMap<String, ObjectRef>, pub document: serde_json::Value }
    #[derive(Deserialize)] pub struct RecordHead { pub format: u32, pub project_id: String, pub export_id: String }
    #[derive(Serialize, Deserialize)] pub struct Snapshot { pub format: u32, pub project_id: String, pub export_id: String, pub exported_at: u64,
        pub seq: u64, pub origin: i64, pub ids: BTreeMap<String, ObjectRef>, pub projection: Projection }
    /// What the route carries (§7.1), ID-free: tracks in order, strips by index.
    #[derive(Serialize, Deserialize, PartialEq, Debug, Clone)] pub struct Projection { pub tracks: Vec<PTrack>, pub strips: Vec<PStrip>,
        pub tempo: Vec<(i64, u32, bool)>, pub meter: Vec<(i64, u8, u8)>, pub markers: Vec<(i64, String)>, pub info: Vec<Option<String>>, pub origin: i64 }
    // PTrack, PNote, PRegion and PStrip derive what Projection derives.
    pub struct PTrack { pub name: String, pub notes: bool, pub audio: bool, pub strip: Option<usize>, pub notes_played: Vec<PNote>,
        pub midi: Vec<(i64, String)>, pub regions: Vec<PRegion>, pub automation: Vec<(String, Vec<(i64, f64)>)> }
    pub struct PNote { pub tick: i64, pub pitch: u8, pub channel: u8, pub velocity: u16, pub release: u16, pub duration: i64, pub expression: Vec<(String, i64, u32)> }
    pub struct PRegion { pub tick: i64, pub media_hash: Option<String>, pub offset: u64, pub length: u64, pub fade_in: u64, pub fade_out: u64,
        pub muted: bool, pub name: Option<String>, pub crossfade: bool, pub gain_db: f32 }
    pub struct PStrip { pub name: String, pub role: String, pub colour: Option<String>, pub gain_db: f64, pub pan: f64, pub mute: bool, pub solo: bool,
        pub output: Option<usize>, pub sends: Vec<(usize, f64, bool, bool)>, pub devices: Vec<(String, String, bool, Option<u64>)> /* format, id, bypass, state hash */ }
    pub fn projection(doc: &Document, seq: SeqId) -> Result<Projection, IoError>;
    ```
    The projection is built from what plays (`midi_out::source` with every track chosen). `PTrack.midi` is each non-note MIDI event as `(tick, "<kind> <ch> <n> <value>")`. A shared instrument strip is projected once per track that plays it (as exported).
  - **The mapping (Rulings 3, 5–8, 15, 16):**
    - **Application and transport:** `Application name="VizDAW" version=<app_version>`. The transport's `Tempo` (id `tempo`, `unit="bpm"`, `min="5"` `max="999"`) is `start_bpm`; `TimeSignature` (id `timesig`) is the first of `start_meters`.
    - **Tempo and meter automation:**
      - `TempoAutomation` (target `tempo`, `unit="bpm"`) is written only when the map has more than one point: every point from the origin on, as `RealPoint`s, `linear` when the point ramps, else `hold`;
      - `TimeSignatureAutomation` (target `timesig`) is written when there is more than one meter entry or a pickup.
    - **Markers:** `Marker time name comment=vz:m<index>`.
    - **Mixer values:** `Volume` (`unit="linear"`, min 0, max `linear_gain(12)`), `Pan` (`normalized`, 0–1), `Mute`, and the `solo` attribute from the strip. `destination` is the output channel's ID. Sends are `type` `pre`/`post`, `Enable`, `Volume`.
    - **Metadata:** the 13 `SongInfo` fields plus the export line (Ruling 15). Empty fields are omitted.
    - **What goes in the report.** Each is the exact `what` the listed test asserts:
      - Left out:
        - "Inactive takes"
        - "Other sequences (not exported)" (with a count)
        - "Keyboard slots"
        - "Input effects"
        - "Track inputs and monitoring"
        - "MPE input settings"
        - "Drum rows (the notes are kept)"
        - "Automation modes"
        - "MIDI learn"
        - "Sidechains"
        - "Instrument extra outputs"
        - "Key signatures"
        - "Settings of plug-in '<name>'"
        - "Plug-in parameters past 32 bits"
        - "Colours and comments on markers and tracks" (spec §6.4)
      - Changed:
        - the two shared-strip lines (Ruling 6)
        - "Tracks with notes and audio were split in two"
      - Note:
        - the external-MIDI and built-in-synth lines (Ruling 6)
        - the two plug-in-state lines (Ruling 8)
        - the pickup line (Ruling 3)
        - "Track '<name>' is muted in the project" and "Track '<name>' is silenced by a solo in the project" (from `midi_out::source`; the track is written unmuted, its channel's `Mute` is the strip's; L11)

      Only lines whose state the document has appear. Each `what` is a constant in `export.rs`.

- [ ] **Step 1: Write the hand goldens** for `structure_fixture()`, typed from this task's rules. The fixture is defined at the top of the test file with `core_engine::fixtures::Builder` and `apply`:
  - sequence "Song" at 120 BPM ramping to 140 at bar 5, 4/4 → 3/4 at bar 9;
  - markers "Verse" (bar 1) and "Chorus" (bar 9);
  - track "Keys" on a CLAP instrument `org.example.synth` with state `b"abc"` and an AU insert `aufx/dely/xmpl` with a 4-byte binary state;
  - track "Drums" on audio path "Drums" with colour Blue and a pre-fader send at −6 dB to bus "Verb";
  - the master at −1.5 dB;
  - song info title "T", artist "A", comment "c".

  `FakePlist::to_xml` wraps the bytes as `<plist><data>HEX</data></plist>`. The arrangement's lanes are empty in this golden (no events); Task 5 fills them. `export_structure.project.xml` holds the whole project text, `export_structure.metadata.xml` the metadata.
- [ ] **Step 2: Write the failing tests** in `tests/dawproject_export.rs`:
  - `structure_matches_the_hand_golden`: both XMLs byte-exact (`hand_golden`). `states` is `[("plugins/001.clap-preset", b"abc"), ("plugins/002.aupreset", <FakePlist xml>)]`.
  - `exports_pass_the_vendored_schema`.
  - `shared_strips`:
    - two tracks on one plug-in instrument → two `Track`s, each with a `ClapPlugin` (IDs `c<s>t<l1>d<i>`, `c<s>t<l2>d<i>`), two state entries with equal bytes, and the Changed line with count 1 and detail "2 tracks";
    - two tracks on one audio path → a submix `Track` `b<s>` holding channel `c<s>`, the tracks' channels plain `k<lane>` with `destination="c<s>"`, and the Changed line.
  - `mixed_track_splits`: a track with a note and a region → `t<l>` (`notes`) and `t<l>a` (`audio`) named "Keys" and "Keys audio", and the Changed line.
  - `vizdaw_only_state_is_listed` (with a muted and a soloed track, L11): a fixture with each listed state gives each Left out or Note `what` above exactly once. A fixture without any gives none of them.
  - `tags_and_record`:
    - every `Track@comment` is `vz:t<lane>`, every `Channel@comment` is `vz:s<strip>`;
    - the metadata comment is "c\nVizDAW export E1" (with `export_id` "E1"), and only "VizDAW export E1" when the comment is empty;
    - `Record.ids` maps `t<lane>` → `{"track", lane}`, `c<strip>` → `{"strip", strip}` and `c<s>d<i>` → `{"device", i}`;
    - `Record.document` equals `serde_json::from_str(&file::to_json(doc))`;
    - the snapshot's `projection == projection(doc, seq)`.
  - `names_are_escaped`: track "A<&>\"B\tC" and a title with a newline give a schema-valid file that `read_project` reads back with the same names.
  - `plugin_states`:
    - `NoPlist` → the AU device has no `State` and "Settings of plug-in 'Delay'" is Left out;
    - a slot with `state: None` → the Note "Plug-in 'Synth' has no saved settings";
    - a `ParamId(1 << 33)` automation target → "Plug-in parameters past 32 bits".
  - `pickup_moves_file_time_zero`: a start offset of one beat → the "Verse" marker at `time="1"`, `Record.origin == -15360`, and the pickup Note.
  - `export_stops_when_cancelled` (L9): a pre-set flag returns `Cancelled`; a flag set from a second thread during the export of a 300 000-note song returns `Cancelled` within one 65 536-item interval (`export` and `check` look at the flag every 65 536 events).
  - `projection_is_id_free`: the same song built twice with different creation orders gives equal projections.
- [ ] **Step 3: Run** `cargo test -p core-io --test dawproject_export`. Expected: fail to compile.
- [ ] **Step 4: Implement** the items above.
- [ ] **Step 5: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +10).
- [ ] **Step 6: Commit** `feat(io): DAWproject export of structure, mixer, plug-ins, maps, metadata, tags and record (plan 6d Task 4)`.

### Task 5: Export: the arrangement (note clips, expression, controller points, audio clips, fades, crossfades, automation, reference uses)

**Files:**
- Modify: `crates/core-io/src/dawproject/export.rs` (or a sibling `export_timeline.rs` it calls)
- Create: `crates/core-io/tests/dawproject_timeline_out.rs`, `crates/core-io/tests/data/dawproject/export_timeline.project.xml` (hand-written)

**Interfaces:**
- Consumes: Task 4.
- Produces:
  - The `Arrangement`'s root `Lanes timeUnit="beats"` holds one `Lanes track="t<lane>"` per exported track, in track order. Each holds one `Clips` (when the track has clips) and then its track-level `Points`.
  - The rest follows Rulings 4 and 9–14.
  - `DpExport.audio` lists one `AudioFile` per (media, applied gain), numbered in first-use order.
  - **Clip attributes:**
    - note clip: `time duration playStart="0"`;
    - looped use: also `loopStart="0" loopEnd`;
    - audio clip: `time duration contentTimeUnit="seconds" playStart playStop fadeTimeUnit="seconds" fadeInTime fadeOutTime` (fades omitted when 0), then `enable="false"` when muted, and `name` and `comment=vz:e<event>`.
  - `Audio@name` is the media item's name, with `timeUnit="seconds"`, `duration` = media frames / rate, `channels` and `sampleRate`.
  - **The report's `what`s:**
    - Left out:
      - "Muted notes"
      - "SysEx messages"
      - "Texts, lyrics and cues"
      - "Note attributes"
      - "Per-note controllers other than pitch, timbre, volume and pan"
      - "Solo automation"
      - "Velocity-scale automation"
      - "Region gain" (with `leave_out`; region names)
    - Changed:
      - "Fade shapes (the file keeps fade lengths only)"
      - the crossfade line (Ruling 10)
      - "MIDI groups other than group 1 were written as group 1"
      - "Audio inside nested sequences was written as plain clips"
    - Note:
      - "Overlapping regions: in VizDAW the later one takes over; other DAWs may play both"
      - "Transposed uses of a sequence were written as played"

- [ ] **Step 1: Write the hand golden** `export_timeline.project.xml` for `timeline_fixture()`, at 120 BPM, `ClipRule::SilenceGap(4 beats)`:
  - track "Keys" on the built-in synth:
    - notes at beats 0 and 1 (C4, velocity 0x8000), and a note at beat 12 with per-note pitch bend +2 semitones at offset 0 and `Registered(74)` 0.5 at offset 1 beat;
    - CC 1 = 0.25 at beat 0.5; a `Program { 5, bank (0, 1) }` at beat 12;
    - a use of sequence "Riff" (one bar) looped 2× at bar 5; a muted use of "Riff" at bar 9;
  - track "Drums":
    - region A at tick 0, 48 000 frames, a Linear fade-in of 480 frames;
    - region B at tick 15 360 (frame 24 000 at 120 BPM and 48 kHz), 48 000 frames, named "Hit", gain −6 dB (with `RegionGain::Apply`); then `AudioCmd::Crossfade { event: B, length: Frames(4_800), shape: Linear }` (S13);
  - fader automation on "Drums": points at 0, 1 and 2 beats (0.0, 0.631, 1.0); mute automation on at beat 3.
- [ ] **Step 2: Write the failing tests** in `tests/dawproject_timeline_out.rs`:
  - `timeline_matches_the_hand_golden`; `exports_pass_the_vendored_schema`.
  - `clip_rules_group_as_the_arrangement_shows`: the "Keys" notes give 2 clips under `SilenceGap(4 beats)`, 2 under `FixedBars(1)` (bar 1, beats 0–4, and bar 4, beats 12–16, which holds the program too), and 1 under `WholeTrack`. The two "Riff" uses are clips of their own under every rule and are not in these counts (L2).
  - `looped_and_unrolled_uses`: a use looped 3× with identical passes → one clip with `loopEnd` = the pass and `duration` 3 passes. A use whose last pass is cut by the sequence's end → an unlooped clip holding every played note.
  - `numbers_come_back_exactly`:
    - for v in [0, 1, 32 767, 65 535], `xml::double(vel) × 65 535` rounds to v;
    - per-note pitch-bend values [0, 2³¹, 2³¹ + 2³¹/48, u32::MAX] come back from their semitones exactly;
    - region offsets of 0, 1 and 2⁴⁰ − 1 frames at 44 100 come back from `playStart` exactly.
  - `what_is_left_out_on_the_timeline`: a fixture with a SysEx, a lyric, a note attribute, a muted note, `Registered(1)`, velocity-scale and solo automation, an EqualPower fade and `leave_out` gives each `what` above once.
  - `region_gain_entries`: `Apply` gives `audio/001.wav` (A, 0 dB) and `audio/002.wav` (B's media, −6 dB); `LeaveOut` gives one entry and the Left out line naming "Hit".
  - `crossfade_writes_matching_fades`: A's `fadeOutTime` is 0.5 (24 000 frames of overlap), B's `fadeInTime` is 0.1, and the crossfade Changed line appears (its S4 wording).
  - `default_patch_and_midi_mix_at_time_zero`: a destination with `Patch { 10, None }` and `MidiMix { volume: Some(100), pan: Some(64) }` gives track-level `Points` at time 0: `programChange` 10, `channelController` 7 = 100/127 and 10 = 64/127.
- [ ] **Step 3: Run** `cargo test -p core-io --test dawproject_timeline_out`. Expected: FAIL.
- [ ] **Step 4: Implement.**
  - The played events come from `midi_out::source` over a copy of the document. In that copy, the mute flag of every top-level reference use and audio region is cleared, and the IDs that were muted are kept aside.
  - Events group by `path[0]`: `path.len() == 1` events go through the `ClipRule`; longer paths go by their use (`path[0].event`).
  - Audio clip durations in beats are `ticks(tempo.seconds_to_tick(start_s + length / rate)) − tick`.
- [ ] **Step 5: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +8). Task 4's golden still passes: its fixture has no events.
- [ ] **Step 6: Commit** `feat(io): DAWproject export of clips, notes with expression, audio, crossfades and automation (plan 6d Task 5)`.

### Task 6: Import: the package, structure, mixer, plug-ins, tempo, meter, markers and song info

**Files:**
- Create:
  - `crates/core-io/src/dawproject/package.rs`, `crates/core-io/src/dawproject/import.rs` (this task: everything but track content);
  - `crates/core-io/tests/dawproject_import.rs`;
  - `crates/core-io/tests/data/dawproject/hand_other.project.xml` and `hand_other.metadata.xml`, hand-written to look like another DAW's file, with no VizDAW tags. The test zips them with `ZipWriter`.

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces:
  - **The package and what the app asks before importing:**
    ```rust
    pub struct DpPackage { pub project: DpProject, pub metadata: DpMetaData, pub ids: Ids, pub ret: ReturnInfo, pub report: Report /* zip, base folder: private */ }
    pub fn open(path: &Path, limits: &PackageLimits, cancel: &AtomicBool) -> Result<DpPackage, IoError>;
    pub struct ReturnInfo { pub record: Option<RecordHead>, pub export_id: Option<String>, pub tagged_tracks: u32, pub tracks: u32 }
    impl ReturnInfo { /// Record, export line, or `vz:t` tags on at least half the tracks (§7.8).
        pub fn from_vizdaw(&self) -> bool; }
    pub struct DpInfo { pub sample_rate: u32, pub tracks: u32, pub audio_files: u32, pub ret: ReturnInfo }
    pub enum AudioSource { Entry(String), External(PathBuf) }
    pub struct AudioRef { pub key: usize, pub source: AudioSource, pub channels: u16, pub sample_rate: u32, pub frames: u64, pub name: String }
    impl DpPackage {
        pub fn info(&self) -> DpInfo;               // Ruling 17's rate; `tracks` counts note and audio tracks, not folders, buses or the master (S14)
        pub fn audio(&self) -> &[AudioRef];          // one per distinct File path, in document order
        pub fn needed_bytes(&self, depth_bytes: u32) -> u64;   // Ruling 18 (L10)
        pub fn extract(&self, a: &AudioRef, to: &Path, cancel: &AtomicBool) -> Result<u64, IoError>;
        pub fn state(&self, path: &str) -> Result<Vec<u8>, IoError>;   // ≤ max_state
    }
    ```
  - **Mapping and applying:**
    ```rust
    pub enum MediaSource { Online(Vec<core_media::import::Imported>), Offline { why: String } }
    pub struct ImportEnv<'a> { pub installed: &'a dyn Fn(PluginFormat, &str) -> bool, pub plist: &'a dyn PlistCodec, pub rate: u32 }
    pub struct DpImport { pub report: Report /* the plan: private */ }
    pub fn map(pkg: &DpPackage, media: &BTreeMap<usize, MediaSource>, env: &ImportEnv, cancel: &AtomicBool) -> Result<DpImport, IoError>;
    /// Inside the caller's open transaction on a document with no tracks;
    /// returns each new plugin slot's state to capture after the commit.
    pub fn apply(h: &mut History, imp: &DpImport, seq: SeqId, report: &mut Report) -> Result<Vec<(PluginInstanceId, Arc<[u8]>)>, ModelError>;
    ```
  - **`open`:**
    - `project.xml` is read with `max_project_xml` and the ratio;
    - `metadata.xml` is read with `max_metadata_xml` and the ratio; a refusal there leaves the song info out ("Song info that could not be read");
    - `vizdaw/record.json` is read with `max_record` into `RecordHead`; a refusal there sets `record: None` and adds a Note.
  - **Structure mapping (Rulings 6, 7, 15, spec §7.4):**
    - **Track kinds:**
      - a `Track` with `notes`, or with an instrument device → a track with a MIDI destination;
      - `audio` → a track with a new output path and its `AudioPath` strip; a track with both gets both;
      - `tracks`, or a track with children → a bus that the children's channels output to, with Changed "Folder tracks became buses";
    - **Channel roles:**
      - `master` → the existing master strip's values;
      - `effect`/`submix` → buses;
      - `vca` → Left out "VCA channels";
    - **Channel values:**
      - the volume `db_of_linear`;
      - pan `pan_of_normalized` (other units scaled by `min`/`max`);
      - mute and solo;
      - sends: dB from linear, pre/post, `Enable`; send pan Left out "Send pan";
      - a `destination` that is the master or a bus is kept; any other is the master, with Changed "Outputs to tracks went to the master";
    - **Instruments:**
      - the first `instrument` device: CLAP or AU → a `PluginInstrument` destination with the file's `PluginRef`; its state (`state(path)`; AU through `to_binary`) is returned for capture;
      - `installed(format, id)` false → the Note "Plug-in '<name>' is not installed; it keeps its settings until it is";
      - VST2 or VST3, or none → a new built-in-synth destination ("Built-in Synth (ch 1)"), with Left out "Plug-in '<name>' (VST)" or the Note "Track '<name>' plays the built-in synth";
    - **Effects:**
      - `audioFX` devices → inserts, or the EQ slot for a `vz:q` tag; `Enabled false` → bypass;
      - VST → Left out by name;
      - `Equalizer`/`Compressor`/`NoiseGate`/`Limiter`/`BuiltinDevice`/`Device` → Left out "Built-in effect '<kind>' on track '<name>'" with its `settings` lines as details;
      - `noteFX`/`analyzer` → Left out by name;
    - **Names and colours:** names are `sanitize`d ("Track <n>" when empty); colours go to the strip by `nearest_swatch`;
    - **Comments:** tags stripped; any other comment text → Left out "Comments on tracks, clips and markers";
    - **Maps:** transport tempo and time signature, then `TempoAutomation` and `TimeSignatureAutomation`. `seconds` points convert in order through the map built so far. A meter off a barline moves to the next one, with Changed "Meter changes moved to the next barline". A tempo outside 5–999 is clamped, with Changed. Then markers;
    - **Song info:** `sanitize`d, cut to `MAX_INFO_BYTES` (Changed "Song info cut to 4 096 bytes"), and the export line stripped from `Comment`. The sequence's name is the title, else the file's stem;
    - **VizDAW files:** the Note of Ruling 16 when `from_vizdaw()`.

- [ ] **Step 1: Write `hand_other`:**
  - a master `Track` holding its channel;
  - a folder track "Band" holding "Bass" (notes, a `Vst3Plugin` instrument "Bass VST") and "Gtr" (audio, an `AuPlugin` audioFX `aufx/dely/xmpl` with `State path="plugins/d.aupreset"`, and an `Equalizer` with bands bell 1 kHz +3 dB Q 0.71 and highPass 80 Hz);
  - "Lead" (notes, `ClapPlugin` `org.example.lead`, `State path="plugins/l.clap-preset"`, colour `#a2eabf`);
  - an effect channel "Room" fed by a post send from "Lead" (volume 0.5 linear, `Enable` false);
  - a bare `Channel role="vca"`;
  - tempo 149 with `TempoAutomation` ramping to 160 at beat 32;
  - time signatures 4/4, then 7/8 at beat 30 (off the 4/4 barline at 32);
  - markers "Intro" at 0 and "Out" at 64;
  - every metadata field, with Comment "made here".
- [ ] **Step 2: Write the failing tests** in `tests/dawproject_import.rs`, applied to a new document built like a new project (one empty sequence and the master):
  - `reads_the_hand_other_file`:
    - tracks "Bass", "Gtr", "Lead" in that order, and buses "Band" and "Room";
    - "Bass" and "Gtr" output to "Band";
    - the Left out lines for the VST3, the Equalizer (its two bands as details) and the VCA;
    - "Lead"'s strip colour is the swatch nearest `#a2eabf`, Changed;
    - the send is −6.02 dB ±0.01, post, disabled;
    - the tempo map is 149 ramping to 160 at tick 32 × 15 360; the 7/8 entry sits at tick 32 × 15 360, with Changed;
    - the markers are imported;
    - all 13 song info fields; the summary counts 3 tracks, 2 markers and the tempo map.
  - `plugins_map_or_wait`:
    - with `installed` true for `org.example.lead`, the returned states hold the CLAP bytes, with no Note;
    - with false, the Note appears and the state is still returned;
    - the AU insert's state is `FakePlist::to_binary` of the entry;
    - a `State` path missing from the ZIP → Left out "Settings of plug-in 'Delay'".
  - `tags_are_stripped_and_counted`:
    - `comment="vz:t3 hello"` on one of 4 tracks gives `tagged_tracks == 1`, `from_vizdaw()` false, and the Left out comments line;
    - 2 of 4 tagged gives true;
    - a metadata comment ending "VizDAW export E9" gives `export_id == Some("E9")`, true, and a song info comment without that line;
    - a `vizdaw/record.json` with `{format: 1, project_id, export_id, …}` gives `record`;
    - a record of 2 bytes of garbage gives `None` and a Note.
  - `channel_destinations`: an output to the master, to a bus, and to a regular track's channel (→ master, with Changed).
  - `refused_packages`: a non-ZIP, a ZIP without `project.xml` (`NOT_DAWPROJECT`), `project.xml` over a lowered `max_project_xml` (`TOO_LARGE`), and a version-2 file.
  - `info_majority_rate`:
    - 3 `Audio` at 44 100 and 1 at 48 000 → 44 100;
    - only 22 050 → 48 000 with the Note;
    - none → 48 000;
    - `audio_files` counts distinct paths.
  - `hostile_text_is_sanitised`: a track name with U+0001, U+202E and 70 000 bytes, and a title of 70 000 bytes, import cut and clean, with the Changed lines.
  - `map_stops_when_cancelled` (L9): `open` and `map` with a pre-set flag return `Cancelled` before any work; a flag set from a second thread during `map` of a 300 000-note file returns `Cancelled` within one 65 536-item interval.
  - `apply_validates_and_is_one_step`: `begin` → `apply` → `commit` gives `rules::validate` empty, and `undo` restores the empty project.
- [ ] **Step 3: Run** `cargo test -p core-io --test dawproject_import`. Expected: fail to compile.
- [ ] **Step 4: Implement** the items above. `map` checks the cancel flag every 65 536 items.
- [ ] **Step 5: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +9).
- [ ] **Step 6: Commit** `feat(io): DAWproject import of the package, structure, mixer, plug-ins, maps and song info (plan 6d Task 6)`.

### Task 7: Import: the arrangement (notes, expression, controller points, re-sharing, audio, fades, crossfades, automation, seconds, warps, scenes)

**Files:**
- Modify: `crates/core-io/src/dawproject/import.rs` (or a sibling `import_timeline.rs`)
- Create: `crates/core-io/tests/dawproject_timeline_in.rs`, `crates/core-io/tests/data/dawproject/hand_timeline.project.xml` (hand-written)

**Interfaces:**
- Consumes: Task 6.
- Produces the timeline mapping inside `map`/`apply`:
  - **Time:**
    - Task 6's `map` builds the tempo map first and keeps it in the mapping state; every seconds conversion uses it;
    - `text::sanitize` takes `Limits::default()` beside the `PackageLimits`;
    - time units are inherited from the parent timeline, defaulting to beats;
    - a clip's content timeline uses `contentTimeUnit`, else the parent's unit;
    - seconds → ticks through the imported tempo map;
    - every result goes through `units::ticks` or checked frame arithmetic, and an item out of range is left out ("Parts placed past the end of the timeline").
  - **Note clips:**
    - notes in `[playStart, playStop or playStart + duration)` of the content are placed at `time + (t − playStart)`;
    - a note crossing the clip's end is cut, with Changed "Notes cut at clip ends";
    - `loopStart`/`loopEnd` repeat the loop span to fill `duration`;
    - `vel`/`rel` are clamped to 0–1, with Changed when clamped, and scaled ×65 535 rounded;
    - `key` outside 0–127 or `channel` outside 0–15 → the note is left out ("Notes that could not be read");
    - a note's channel is kept as `group_channel` `(0, c)` only when it differs from the track's main channel (the one with the most notes).
  - **Re-sharing (spec §7.4):**
    - clips with a `reference` to one content, clips with a loop, and clips that share a name and have identical content each become uses of one new sequence, on the same track or on tracks with the same destination;
    - the sequence is named after the clip (else "<track> clip <n>"), with one lane on that destination;
    - the use is a `Reference` with `loop_count` `Count(n)` when the duration is a whole number of passes, else `Fill` with `length`. The sequence's length is `loopEnd − loopStart`. A looped clip whose `playStart` is not its `loopStart`, or whose pass cannot start at the sequence's start, is unrolled onto the track with Changed "Looped clips with a different start were unrolled" (L5);
    - other note clips go straight onto the track;
    - uses across tracks with different destinations get one sequence each, with the Note "Shared clips on tracks with different instruments became separate sequences".
  - **Expression (Ruling 12, the inverse):**
    - transpose over ±48 is clamped, with Changed;
    - `formant` → Left out "Formant expression".
  - **Controller points (Ruling 13, the inverse):**
    - every point becomes one event;
    - `linear` interpolation between MIDI points → Changed "MIDI ramps were written as their points";
    - a `programChange` with `channelController` 0 and 32 points at the same time → `Program { bank }`.
  - **Audio clips (Rulings 4, 10, 19):**
    - **Region values:**
      - the region's tick is `time` (a negative `fadeInTime` starts it earlier);
      - `source_offset` = `playStart` in frames at the media rate;
      - the length is `playStop − playStart`, else `duration` through the map;
      - fades → frames, Linear;
      - `enable="false"` → `mute`; `name` → the region's name;
    - **Nested content:**
      - a `Clips` inside an audio clip is flattened, composing offsets and cutting to the outer window;
      - `Warps` → the inner audio, placed unstretched. It is Left out "Time-stretching (clips placed unstretched)" unless every warp maps `time` to the same seconds within 1 ms;
    - **Media:**
      - an `Online` media item is added once per source; `Offline` → an offline item and its region, with Ruling 19's line;
      - several mono items → Ruling 19;
    - **Crossfades:** after placing a track's regions, Ruling 10's crossfade rule is applied; each crossfaded pair goes into one `EventCmd::Insert`.
  - **Automation (Ruling 14, the inverse):** a `Points` whose target parameter is a channel's `Volume`, `Pan`, `Mute`, a send's `Volume` or a device's parameter → automation events on the owning track.
  - **Clip launcher and video:** `Scenes`' `ClipSlot`s → unplaced sequences "<scene> – <track>", with Changed "Clip launcher slots became unplaced sequences"; `Video` → Left out "Video".

- [ ] **Step 1: Write `hand_timeline`** before any code: one track per Step 2 test, named after the test, holding exactly the clips, points and values that test states (the positions and values in Step 2 are the fixture's). The test-data README lists the tracks.
- [ ] **Step 2: Write the failing tests** in `tests/dawproject_timeline_in.rs`:
  - `notes_land_where_the_clips_play`: a clip at time 4 with `playStart` 1 and `playStop` 3 holding notes at 0.5, 1, 2.5 (duration 1) and 3 → notes at ticks of beats 4 and 5.5 (the 2.5 note cut to 0.5 beats, with Changed); the 0.5 and 3 notes are out.
  - `looped_clip_becomes_a_looped_use`:
    - `loopStart 0 loopEnd 2 duration 6` → a sequence named after the clip and a use with `Count(3)`;
    - `duration 5` → `Fill` with `length` 5 beats;
    - the new sequence's length is `loopEnd − loopStart`;
    - `playStart 1` with `loopStart 0` (a use cannot start inside its sequence) → the notes go straight on the track, unrolled from beat 1 of the content, with Changed "Looped clips with a different start were unrolled" (L5).
  - `alias_clips_share_one_sequence`: two clips referencing one `Notes` on one track → one sequence, two uses. On two tracks with different instruments → two sequences and the Note.
  - `same_name_same_content_is_reshared`: two clips "A" with the same notes → one sequence; "A" with different notes, and "A"/"B" with the same notes → separate.
  - `note_expressions_map_to_per_note_expression`:
    - transpose +2 at 0 → `PitchBend` 2³¹ + 2 × 2³¹/48;
    - timbre 0.5 at 0.25 beat → `Registered(74)` `to_u32(0.5)` at offset 3 840;
    - pressure 1.0 → a `PolyPressure` event at note tick + offset;
    - gain → `Registered(7)`; pan → `Registered(10)`;
    - formant → Left out; transpose 60 → 48, with Changed.
  - `midi_points_become_events`: `channelController 1` 0.5 → `Cc { 1, to_u32(0.5) }`; `pitchBend` 0.5 → 0x8000_0000; `programChange` 5 with CC 0 = 0 and CC 32 = 1 at one time → `Program { 5, Some((0, 1)) }`; linear points → the Changed line.
  - `audio_clips_map_to_regions`: a beats clip at time 2, `contentTimeUnit="seconds" playStart="0.5" playStop="1.5"`, `fadeInTime 0.01` in seconds, `fadeOutTime 0.25` in beats, at 48 kHz and 120 BPM → tick 30 720, offset 24 000, length 48 000, fade-in 480, fade-out 6 000, Linear.
  - `negative_fade_in_starts_the_clip_earlier`: time 4, `fadeInTime -0.5` in beats → tick of beat 3.5, with a fade-in of 0.25 s in frames.
  - `nested_clips_compose_their_offsets`: an outer clip at time 8 holding `Clips` of two inner clips at content times 0 and 2 → regions at beats 8 and 10, the second cut at the outer `duration` 3.
  - `identity_warps_are_not_reported`: warps (0,0) and (8, 4.0 s) at 120 BPM are not reported; (8, 2.0 s) gives the time-stretching line and the region unstretched.
  - `overlapping_clips_with_covering_fades_become_crossfades`:
    - A at 0 for 1 s with fade-out 0.5 s, and B at 0.5 s with fade-in 0.5 s → B is a crossfade, `fade_in` 24 000, Linear; A's fade-out is 0; `rules::validate` is empty;
    - with B's fade-in at 0.1 s and A's fade-out 0 → plain overlap, no crossfade.
  - `automation_points_become_events`:
    - `Volume` linear points 0.0, 1.0 and 2.0 → `StripFader` values `fader_norm(db_of_linear(·))`;
    - pan normalised 0.25 → `StripPan` 0.25;
    - `BoolPoint`s → `StripMute` 0/1;
    - a send `Volume` → `SendLevel`;
    - a device parameter with `min -12 max 12` and `unit="decibel"`, value 0 → 0.5;
    - `hold` adds the one-tick point, with Changed;
    - arrangement-level `Points` with no track go to the owning track. A target on the master goes to the new "Automation" track, with its Note.
  - `seconds_timelines_map_through_the_tempo_map`: `Lanes timeUnit="seconds"` at 120 BPM with a clip at 0.5 s → beat 1. Under a ramp 120→240 over 4 beats, a clip at 1.5 s → the tick `seconds_to_tick` gives. A nested beats timeline inside a seconds one is in beats.
  - `scenes_become_unplaced_sequences`: two scenes × one track → sequences "Scene 1 – Bass" and "Scene 2 – Bass", unplaced, with Changed. Video is Left out.
  - `hostile_numbers_leave_items_out`: time 1e300, duration −1, key 200, channel 99, vel 2.0 (clamped, Changed), `sampleRate="0"` and `value="NaN"` each leave their item out (or clamp it), and the rest of the track imports.
- [ ] **Step 3: Run** `cargo test -p core-io --test dawproject_timeline_in`. Expected: FAIL.
- [ ] **Step 4: Implement** the items above.
- [ ] **Step 5: Run** `cargo test -p core-io` and both clippies. Expected: PASS (about +15).
- [ ] **Step 6: Commit** `feat(io): DAWproject import of clips, notes with expression, audio, crossfades, automation and scenes (plan 6d Task 7)`.

### Task 8: Round trips, schema checks on every export, hostile files and the limits table

**Files:**
- Create: `crates/core-io/tests/dawproject_round_trip.rs`, `crates/core-io/tests/dawproject_fuzz.rs`, `crates/core-io/tests/dawproject_limits.rs`
- Modify: `crates/core-io/tests/data/README.md` (the fuzz and round-trip harness notes)

**Interfaces:**
- Consumes: Tasks 1–7.
- Produces test harnesses only:
  - `round_trip(doc, opts) -> (Document, Report, Report)`:
    1. `export` with `FakePlist`;
    2. writes the package with `ZipWriter`, each `AudioFile` as a 44-byte stub WAV;
    3. runs `open` and then `map`, with `media` mapping each entry back to `MediaSource::Online(vec![Imported { path, content_hash, … of the original item }])` (the copy's hash for gain copies);
    4. runs `apply` on a new project at the same rate.
  - **The tolerances**, compared on `projection(original)` and `projection(imported)`:
    - exact: names, notes and their expressions, MIDI events, region ticks, offsets and lengths, fade lengths, mutes, crossfade flags, the tempo and meter, markers, song info, sends' targets and flags, devices' format, ID, bypass and state hash;
    - within 1e-9: levels and pan;
    - automation values within 1e-9 at every point; the hold points the import adds are dropped from the comparison;
    - `gain_db` equal with `LeaveOut` and 0 after `Apply`;
    - colours exact, since every swatch's hex is in the table.

- [ ] **Step 1: Write the failing tests.**
  - `dawproject_round_trip.rs`:
    - `round_trips_preserve_the_projection`: proptest, `cases(64)`, with a `Spec` generator in the style of `tests/round_trip.rs`'s `spec()`/`build()` extended with:
      - regions (with fades, names, gain, crossfades);
      - strips, buses, sends and colours;
      - CLAP plug-ins with states;
      - automation of every carried kind;
      - markers, tempo ramps, meter changes and a pickup;
      - song info.

      Projections are equal within the tolerances above, and the export's report holds only the expected kinds for the spec.
    - Fixed cases, one test each:
      - `nested_references_reshare`: a sequence used 3× (once looped 2×) → one sequence with 3 uses after the trip;
      - `transposed_uses_stay_separate`;
      - `takes_and_vizdaw_only_state_are_listed`;
      - `mpe_expression_survives`;
      - `drum_track_keeps_its_notes`;
      - `ramps_and_a_pickup`: the tempo map and markers equal, and the origin applied;
      - `shared_instrument_is_copied`: two tracks with their own copies, states equal;
      - `stereo_and_mono_media`;
      - `crossfades_survive`: flag and length exact; shape Linear;
      - `a_thousand_tracks`: 1 000 tracks × 10 notes, export + import under 10 s in debug, timed with `Instant`.
    - `every_export_passes_the_schema`: 16 generated specs through `schema_check` (XSD), both files.
  - `dawproject_fuzz.rs`, with a `#[global_allocator]` that counts per thread (S8), asserting the case thread's peak under 256 MiB, and each case under 2 s:
    - `damaged_packages_never_panic`: byte flips, inserts, deletes and truncations (`cases(256)`) of the zipped `hand_minimal`, `hand_other`, `hand_timeline` and one `export` output. Each `open` + `map` is `Ok` or `Refused`, never `Io` for in-memory faults and never a panic.
    - `hostile_zips_never_panic`: generator over `raw_zip` with traversal names, duplicates, link modes, flags, method 99, declared sizes 0 / `u32::MAX` / ZIP64, and a 50 MiB-of-zeros `project.xml`.
    - `hostile_xml_never_panics`: generator of `project.xml` texts with DOCTYPE, depth up to 300, 10⁵ elements, numbers from {`1e400`, `NaN`, `-0`, `inf`, 70 digits}, dangling and cyclic IDREFs, unknown elements, and `contentType` lists of 1 000 words.
  - `dawproject_limits.rs`: one test per §6.5 row this plan owns, each at the limit (accepted) and one past it (refused or left out with its line), using lowered `PackageLimits` for the size rows:
    - `zip_entries`, `zip_names` (each forbidden form), `zip_links`, `zip_encrypted`, `zip_ratio` (200:1 accepted, 201:1 refused);
    - `free_space_estimate` (`needed_bytes` = entry sizes + one CAF per source, external files included, at 2, 3 and 4 bytes per sample; L10);
    - `xml_size`, `xml_doctype`, `xml_depth` (256/257), `xml_nodes`;
    - `xml_tree_bytes`: a ratio-legal ZIP of `<a/>` repeated to the node cap parses (and one more is refused) with the counting allocator's peak under `MAX_PROJECT_XML_BYTES`-sized text + 128 MiB + 16 MiB (L7);
    - `numbers` (`MAX_NUMBER_CHARS` 64/65; finite only);
    - `idrefs_resolve`, `reference_cycles`, `nesting` (16/17);
    - `positions` (beats at `MAX_TICKS / 15 360` accepted, one beat past it left out);
    - `external_media` (a FIFO, a directory, a symlink loop and `http://x/a.wav` → never read; `AudioRef` kept for the job, which makes them offline);
    - `defaults_are_the_spec_values` (every constant equals Ruling 18).
- [ ] **Step 2: Run** `cargo test -p core-io --test dawproject_round_trip --test dawproject_fuzz --test dawproject_limits`. Expected: the tests compile and run against Tasks 1–7. Any that pass at once are kept, and this step says which; each failure is a bug fixed in Step 3 with a regression case added.
- [ ] **Step 3: Fix** what Step 2 found in `core-io`, each fix with its own regression case in the owning task's test file.
- [ ] **Step 4: Run** `cargo test --workspace` and both clippies. Expected: PASS (about +30). Run once more with `VIZDAW_FUZZ_CASES=4096` locally, and give its time in the report.
- [ ] **Step 5: Commit** `test(io): DAWproject round trips, schema checks, hostile files and limits (plan 6d Task 8)`.

### Task 9: `core-ffi`: the export job, its rows and checks, the snapshots on Save, and the AU property-list codec

**Files:**
- Create:
  - `crates/core-ffi/src/dawproject.rs`;
  - `crates/platform-macos/src/plist.rs`;
  - `crates/core-ffi/tests/dawproject.rs` (export cases; `serial` with `force_offline`);
  - `crates/platform-macos/tests/plist.rs`.
- Modify:
  - `crates/core-ffi/src/lib.rs`, `src/session.rs`:
    - the methods below;
    - `save_to` calls `dawproject::keep_snapshots`;
    - `capture_all` gains a sibling, `capture_for_export`;
  - `src/write.rs` in `core-io`: `create_partial`; `write_partial` rebuilt on it;
  - `crates/platform-macos/src/lib.rs`, `crates/core-ffi/src/jobs.rs` (`JobKindRow::ExportDawproject`);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` and the Swift job-kind sites named in Global Constraints (L1), with a test that a failed `.exportDawproject` row is titled "DAWproject Export Failed" and opens no MIDI sheet.

**Interfaces:**
- Consumes: Tasks 1–8; `interchange::{file_dest, report_row, io_error}`; `Session::{interchange_idle, export_roots}`.
- Produces:
  - ```rust
    // core-io write.rs
    /// The partial beside `dest`, open for streaming; refused like `write_partial`.
    pub fn create_partial(dest: &Path, replace: bool) -> Result<(PendingFile, std::fs::File), IoError>;
    // platform-macos plist.rs (CFPropertyListCreateWithData, CFPropertyListCreateData)
    pub fn to_xml(plist: &[u8]) -> Result<Vec<u8>, String>;      // any format in, XMLFormat_v1_0 out
    pub fn to_binary(plist: &[u8]) -> Result<Vec<u8>, String>;   // any format in, BinaryFormat_v1_0 out
    // core-ffi dawproject.rs
    #[derive(uniffi::Enum)] pub enum RegionGainRow { Apply, LeaveOut }
    #[derive(uniffi::Record)] pub struct DawprojectExportRequestRow { pub seq: u64, pub path: String, pub replace: bool,
        pub region_gain: RegionGainRow, pub blocks: crate::blocks::BlockRule }
    pub(crate) struct MacPlist; // PlistCodec through platform_macos::plist (macOS); NoPlist elsewhere
    pub(crate) fn plist() -> &'static dyn core_io::dawproject::PlistCodec;
    pub(crate) fn keep_snapshots(from: &Path, to: &Path) -> Result<(), String>;   // Ruling 16
    // session.rs (#[uniffi::export])
    pub fn export_dawproject(&self, req: DawprojectExportRequestRow) -> Result<u64, VizError>;
    pub fn export_dawproject_check(&self, req: DawprojectExportRequestRow) -> Result<ReportRow, VizError>;
    pub fn dawproject_name(&self, seq: u64) -> Result<String, VizError>;
    #[cfg(feature = "test-hooks")] pub fn debug_dawproject_cancel_at(&self, stage: Option<u32>) -> Result<(), VizError>;
    #[cfg(feature = "test-hooks")] pub fn debug_free_bytes(&self, bytes: Option<u64>) -> Result<(), VizError>;
    ```
  - **`export_dawproject`** (on the lock):
    - `interchange_idle`;
    - the sequence must exist;
    - `file_dest(&req.path, req.replace, &roots)`, and the extension must be `dawproject` ("Choose a name ending in .dawproject."); `name_fits` also checks the longest name the job makes, `.<name>.audio.partial` (S6);
    - `refresh_media` and `capture_for_export`, which captures every loaded slot and returns the slots it could not capture;
    - the project ID stamped when missing (Ruling 16);
    - `export_id = project_id::fresh()`, `exported_at` = now (Unix seconds), `app_version = env!("CARGO_PKG_VERSION")`;
    - then the doc is cloned and an `ExportDawproject` job spawned.
  - **The job** (inside `catch_unwind`; stage numbers for `debug_dawproject_cancel_at`):
    1. `core_io::dawproject::export` (stage 1 after);
    2. the free-space check: the WAVs' total bytes at the project depth plus the XML, × 2, against `free_bytes` of the destination's folder (or `debug_free_bytes`) → "Not enough free space to export (it needs <n> MB).";
    3. a media copy whose WAV would pass 4 GiB is not staged: Left out "Audio too long for a WAV file" with its names, its clips written without audio (S9). Then `stage_folder(<dest>.audio)` (a hidden `.<name>.dawproject.audio.partial`, always removed). Each `AudioFile` is written there from the media item's CAF with `PcmWriter` (WAV, project depth, media rate, `dither: false`), with gain applied as `10^(g/20)` per sample (stage 2 after the first);
    4. `create_partial(dest, replace)` → `ZipWriter` over a `BufWriter`, in this order:
       - `metadata.xml`, `project.xml` and `vizdaw/record.json` (Deflated; stage 3 after `project.xml`);
       - `audio/*` (Stored, `add_from` with cancel);
       - `plugins/*` (Deflated);

       then `finish`, `sync_all` and a last cancel look (stage 4);
    5. `commit`;
    6. the snapshot: `Interchange/` is made with `create_dir_all` when missing (lstat; a link or a file there is refused), then `write_partial(<root>/Interchange/<export id>.json, replace: false)` + commit (L6). A failure adds the Note "The merge record could not be kept in the project: <why>"; the export is still Done.

    The job returns `Work::ExportMidi { result, written: [dest] }` under the kind `JobKindRow::ExportDawproject` (L1): the payload's apply is the generic report-and-files path; its doc line becomes "A MIDI or DAWproject export's files"; the row's kind is what the app titles.
  - The slots `capture_for_export` could not capture use their stored state. With none stored, the Left out line is "Settings of plug-in '<name>'" (spec §11).
  - **`export_dawproject_check`** checks as `export_dawproject` does, minus Busy and the path, then calls `core_io::dawproject::check` off the lock.
  - **`dawproject_name`** is `file_name`.
  - `ClipRule` from `BlockRule`: `SilenceGap { ticks }` → `SilenceGap(Tick(ticks.max(1)))`, `FixedBars { bars }` → `FixedBars(bars.max(1))`, `WholeLane` → `WholeTrack`.

- [ ] **Step 1: Write the failing tests.**
  - `platform-macos/tests/plist.rs`:
    - `xml_and_binary_round_trip`: a dictionary with data, numbers and strings built through `to_binary(xml)` → `to_xml` → `to_binary` gives equal bytes;
    - garbage → `Err`;
    - `au_state_survives_the_aupreset_conversion`: the `ClassInfo` of the effect `platform-macos`'s AU tests already instantiate goes through `to_xml` then `to_binary` and is accepted back by that unit's `set_state`.
  - `core-ffi/tests/dawproject.rs`, with a temp package holding generated mono and stereo CAFs and the test gain CLAP from `test-plugins`:
    - `export_writes_the_package`:
      - the job is Done;
      - the entries are exactly `metadata.xml`, `project.xml`, `vizdaw/record.json`, `audio/001.wav`, `audio/002.wav`, `plugins/001.clap-preset`, with methods Deflated ×3, Stored ×2, Deflated;
      - each WAV's `fmt ` chunk has the project's depth and the media's rate, and its `data` length is frames × channels × bytes;
      - the first 16 frames equal the CAF's samples;
      - `JobRow.written` is the file and `report.summary` starts "Exported".
    - `region_gain_apply_and_leave_out`: a −6 dB region's copy has samples × 0.501 187 ±1 LSB; `LeaveOut` writes one copy and the Left out line.
    - `snapshots_are_kept_and_save_prunes_to_ten`:
      - the first export of a project with no `Interchange/` makes it (L6); 12 exports of an untitled project → 12 snapshots in the scratch root's `Interchange/`;
      - Save As → the new package has the 10 newest;
      - a symlink placed in `Interchange/` is not followed or copied;
      - each snapshot's `export_id` matches the record in its ZIP.
    - `project_id_is_stamped_once`: an untitled export stamps an ID equal to the record's `project_id`. A second export reuses it. `snapshot().undo_label` is unchanged.
    - `cancel_at_every_stage_leaves_nothing`: a cancel before the run, and `debug_dawproject_cancel_at` 1–4, each end Cancelled, with no destination, no `.x.dawproject.partial`, no `.x.dawproject.audio.partial` and no new snapshot.
    - `replacing_needs_replace`:
      - an existing file and a dangling symlink at the destination are each refused at once (Model) without `replace`;
      - with `replace`, the old bytes stay until the job commits; at stage 3 a cancel leaves the old file byte-equal.
    - `not_enough_space_refuses`: `debug_free_bytes(Some(10))` → Failed with the sentence, nothing left.
    - `busy_states_refuse`: recording, an open edit, a running bounce, another export and a running MIDI import each give `Busy`.
    - `placeholder_and_missing_state_are_listed`: a slot for an uninstalled plug-in with a stored blob → its blob is in `plugins/`, with the Note. A slot with neither → the Left out line.
    - `check_matches_the_job`: `export_dawproject_check`'s items equal the job's report items, minus the plug-in state lines `NoPlist` adds.
    - `hostile_requests`: `path` empty, relative, a directory, a FIFO, `/dev/zero`, a symlink loop, over 4 KiB, with NUL, inside the open package, or ending `.zip`, and `seq` unknown, each give `Model` (never `Internal`), with nothing written.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test dawproject` and `cargo test -p platform-macos --test plist`. Expected: fail to compile.
- [ ] **Step 3: Implement** the items above, then `scripts/build-ffi.sh`. `FakeSession` gains the three methods: logs of requests, a settable check answer and job IDs.
- [ ] **Step 4: Run** `cargo test -p core-ffi -p platform-macos -p core-io`, `cargo clippy -p core-ffi --lib -- -D warnings`, both clippies, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +14 Rust; Swift unchanged).
- [ ] **Step 5: Commit** `feat(ffi): DAWproject export job, checks, snapshots on save and the AU codec (plan 6d Task 9)`.

### Task 10: `core-ffi`: the import job, the info read and applying as a new project

**Files:**
- Modify:
  - `crates/core-ffi/src/dawproject.rs`, `src/jobs.rs` (`JobKindRow::ImportProject`, `Work::ImportProject`), `src/session.rs` (`apply_jobs` arm, `apply_dawproject_import`, `interchange_idle`'s kinds);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift`;
  - every Swift `switch` and `==` chain over `JobKindRow` named in Global Constraints (L1).
- Test: `crates/core-ffi/tests/dawproject.rs` (import cases)

**Interfaces:**
- Consumes: Task 9; `core_media::import::import_reserved`; `crate::media::importer`.
- Produces:
  - ```rust
    #[derive(uniffi::Record)] pub struct DawprojectInfoRow { pub sample_rate: u32, pub tracks: u32, pub audio_files: u32,
        pub from_vizdaw: bool, pub export_id: Option<String> }
    #[derive(uniffi::Record)] pub struct DawprojectImportRequestRow { pub path: String }
    #[uniffi::export] pub fn dawproject_info(path: String) -> Result<DawprojectInfoRow, VizError>;   // a free function (L8): open + info, no session
    pub fn import_dawproject(&self, req: DawprojectImportRequestRow) -> Result<u64, VizError>;
    // jobs.rs
    /// A DAWproject import: the media it made (removed unless applied) and the plan.
    Work::ImportProject { files: Vec<(String, Result<Vec<Imported>, String>)>, result: Result<Box<core_io::dawproject::DpImport>, String> }
    ```
  - **`import_dawproject`:**
    - `interchange_idle`, `media_jobs_idle`, `file_path` (which gains a noun argument, "a MIDI file" / "a DAWproject file", for its refusal; S5);
    - refused when the document has a track (Ruling 17);
    - spawns `ImportProject` with the root, the project format, `referenced_media_anywhere()` keys as reserved names, a snapshot of the installed `(format, id)` pairs (enabled catalog entries), `plist()` and the importer.
  - **The job** (inside `catch_unwind`; stages 5–8):
    1. `open` (stage 5 after);
    2. `needed_bytes(project depth)` against `free_bytes(root)` (or `debug_free_bytes`) → "Not enough free space to import this file (it needs <n> MB).";
    3. per `AudioRef`, with progress:
       - `Entry` → `extract` to `<root>/Media/.incoming/<fresh>.<ext>` (stage 6 after the first), `import_reserved` with stem = the ref's name (stage 7 after the first), then the extracted copy removed;
       - `External` → Ruling 19's checks, then `import_reserved` from it;
       - failures → `Offline`;
    4. `map` (stage 8 after).

    A cancel or failure at any stage returns `Work::ImportProject { files, result: Err(..) }` (`"Cancelled"` for a cancel), so `apply_jobs` removes the files through `remove_unkept` (6b F1).
  - **`apply_dawproject_import`**, as `apply_midi_import`'s new-project path:
    - a scratch `History` from the document;
    - `begin("Import DAWproject")`, `apply` inside `catch_unwind`, `commit`;
    - `capture_state` for each returned slot;
    - `inner.history = History::new(..)`, with `last_step` and `surfaces.tempo_step` cleared;
    - `interchange_report = report_row(&report, "Imported")`.

    On any `Err`, the files are removed and the row is Failed with the message (Cancelled for `"Cancelled"`).

- [ ] **Step 1: Write the failing tests** in `core-ffi/tests/dawproject.rs`:
  - `info_reads_rate_tracks_and_return`: our own export → `from_vizdaw` true and `export_id` equal to the snapshot's. A hand-zipped `hand_other` → false, rate 48 000, tracks 3.
  - `import_as_new_project`: export Task 9's fixture, then import it into a fresh session made as a new project:
    - the track names and count, notes and their expression;
    - the regions online with `content_hash` equal to the source's;
    - B's crossfade flag; the tempo ramp, markers, song info, strip gains and the send;
    - `snapshot().undo_label == None`;
    - the report summary "Imported 2 tracks, …" and the from-VizDAW Note.
  - `clap_state_round_trips`: the test gain plug-in with parameter 0 at 0.25 → export → import → after loading, the new `debug_plugin_param(slot, 0)` reads 0.25 (spec §12; L4).
  - `cancel_at_every_stage_leaves_media_unchanged` (each case clears `debug_free_bytes` first; S10): a cancel before the run, and stages 5–8, each end Cancelled. The listings of `Media/` and `Media/.incoming/` equal those taken before; `debug_document_json` is unchanged.
  - `missing_and_undecodable_audio_become_offline`: a referenced entry that is absent, an entry of 1 KiB of garbage, an external FIFO and `http://x/a.wav` → the job is Done with 4 offline regions and the Left out line with 4 details.
  - `not_enough_space_refuses`; `refused_files` (non-ZIP, version 2, traversal names: Failed with the sentence, nothing in `Media/`); `into_a_project_with_tracks_is_refused` (Model).
  - `busy_states_refuse` (recording, open edit, a running export, a Convert job).
  - `hostile_requests` as Task 9's list, for `dawproject_info` and `import_dawproject`; each `Model` or `Io`, never `Internal`.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test dawproject`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above, then `scripts/build-ffi.sh`. Add the Swift `switch` arms (`.importProject`, titled "Import Project" where kinds are named). `FakeSession` gains `dawprojectInfo` and `importDawproject`, with settable answers and logs.
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo clippy -p core-ffi --lib -- -D warnings`, both clippies, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +10 Rust).
- [ ] **Step 5: Commit** `feat(ffi): DAWproject import job, info read and new-project apply (plan 6d Task 10)`.

### Task 11: The control channel: `file.import_dawproject` and `file.export_dawproject`

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs`:
    - `IMPORT_DAWPROJECT_PARAMS` and `EXPORT_DAWPROJECT_PARAMS`;
    - two entries after `file.export_for_daw`;
  - `crates/core-control/tests/commands.rs`: the count is 175, and the sentence gains "; 6d adds file.import_dawproject and file.export_dawproject";
  - `app/Sources/VizDAWAppModel/InterchangeCommands.swift`;
  - `app/Tests/VizDAWAppModelTests/InterchangeCommandsTests.swift` (or the file 6a's channel tests live in).

**Interfaces:**
- Consumes: Tasks 9–10.
- Produces:
  - **Registry:**
    - `file.import_dawproject`: menu "File", title "DAWproject…", `document: true`, dialog "Open" (always, answered by `path`). Params:
      - `path` (`Path("dawproject")`, "The .dawproject file to open as a new project (answers the Open dialog).");
      - `mode` (`Choice(&["new"])`, "new (default): the file opens as a new project.").
    - `file.export_dawproject`: menu "File", title "DAWproject…", dialog "Save" (when "no path and no dry_run"). Params:
      - `path` (`Path("dawproject")`);
      - `replace` (Flag);
      - `seq` (Int, as `file.export_midi`);
      - `region_gain` (`Choice(&["apply", "leave_out"])`, "apply (default): region gain is applied to the exported audio; leave_out: it is left out");
      - `dry_run` (Flag, "Return what won't transfer; write nothing").
  - **Swift**, `InterchangeCommands`:
    - `static let importDawproject = "file.import_dawproject", exportDawproject = "file.export_dawproject"`, both joining `ids`;
    - `static func dawprojectRequest(_ args: [ControlArg], _ m: DocumentModel) -> Parsed<DawprojectExportRequestRow>`, with `blocks` = `m.editor.layout.blockRule.rule`;
    - in `run`:
      - **import:** with no `path`, `.dialog("Open", "path")`. Otherwise `.pending`, which reads the free function `dawprojectInfo(path:)` off the main thread (L8). A refusal answers `.failed(.thrown(error, "Import Failed"))` and makes no project. Else `importDawprojectIntoNewProject`: `store.newProject(ProjectFormatRow(sampleRate: info.sampleRate, depth: store.defaultFormat.depth))`, `madeProjects.insert`, `.job(command:, doc: made, job: importDawproject(req:))`.
      - **export:** with `dry_run`, `.pending` with `exportDawprojectCheck`. Without a path, `.dialog("Save", "path")`. Otherwise `.job(…exportDawproject(req:))`.

- [ ] **Step 1: Write the failing tests.**
  - `core-control`:
    - `dawproject_commands_check_their_params`:
      - `{}`, `{path: "/x/a.dawproject"}`, `{mode: "new"}` and `{region_gain: "leave_out", dry_run: true}` validate;
      - `mode: "merge"`, `region_gain: "half"`, `seq: -1`, `replace: "yes"` and extra keys are `InvalidParams`;
    - `every_command_survives_random_params` covers both (2 048 cases).
  - Swift `InterchangeCommandsTests`:
    - `importDawprojectReadsOffTheMainActorThenMakesTheProject`: the fake info answers 44 100 → one `newProject` at 44 100, and one `importDawproject` with the path on the new document. A refused info → no project, `failed`.
    - `importDawprojectWithoutPathAsksForOpen`; `exportDawprojectWithoutPathAsksForSave`; `exportDawprojectDryRunIsPendingAndWritesNothing`.
    - `exportDawprojectBuildsTheRequest`: `{path, replace: true, region_gain: "leave_out", seq}` → the row, `blocks` from the layout.
    - `dawprojectCommandsRefuseHostileArguments` (these call `InterchangeCommands.run` directly, past the registry's `Path` checks; S7): `path` empty, relative, over 4 KiB or with NUL → the core's `failed` sentence. `seq` not an ID → `invalid_params` naming the command and the parameter, with no session call. An unknown sequence → the core's `failed` sentence.
    - `everyRegisteredMenuCommandHasAHandler` passes.
- [ ] **Step 2: Run** `cargo test -p core-control` and `scripts/swift.sh test --filter InterchangeCommands`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above.
- [ ] **Step 4: Run** `cargo test -p core-control`, both clippies, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (registry 175; about +1 Rust, +7 Swift).
- [ ] **Step 5: Commit** `feat(control): file.import_dawproject and file.export_dawproject (plan 6d Task 11)`.

### Task 12: The app's model: `DawprojectModel`, opening a `.dawproject`, queueing

**Files:**
- Create: `app/Sources/VizDAWAppModel/DawprojectModel.swift`, `app/Tests/VizDAWAppModelTests/DawprojectModelTests.swift`
- Modify:
  - `app/Sources/VizDAWAppModel/DocumentModel.swift` (lazy `dawproject`, as `forDaw`);
  - `ForDawModel.swift` (`blocking` gains `.importProject`);
  - `Interchange.swift` (`InterchangeModel.isDawproject(_:)`);
  - `DocumentStore.swift` (`open(path:)` sends `.dawproject` to `openDawproject` and returns `nil`; an injectable `dawprojectInfo: @Sendable (String) throws -> DawprojectInfoRow`, the free function by default, so tests fake it; L8);
  - `app/Tests/VizDAWAppModelTests/DocumentStoreTests.swift`.

**Interfaces:**
- Consumes: Tasks 9–11.
- Produces:
  - ```swift
    @MainActor @Observable public final class DawprojectModel {
        public init(document: DocumentModel)
        public var shown: Bool                                   // opening refreshes the check and the name
        public var regionGain: RegionGainRow                     // .apply; a change refreshes the check
        public private(set) var wontTransfer: ReportRow?
        public private(set) var checkError: String?
        public private(set) var defaultName: String              // session.dawprojectName(seq:)
        public private(set) var waiting: Bool                    // queued behind a blocking job
        public private(set) var job: UInt64?
        public var running: Bool { get }
        public func refresh()                                    // detached exportDawprojectCheck, generation counter
        public func start(path: String, replace: Bool)           // queues while ForDawModel.blocking or media jobs run
        public func cancelWaiting()
        public func settle() async
    }
    // DocumentStore
    /// Starts opening `path` and returns at once (L8): a detached task calls the free function
    /// `dawprojectInfo(path:)` (no document or session needed), then on the main actor makes a
    /// project at the file's rate and starts `importDawproject`. A refusal makes no project and
    /// alerts "Import Failed"; a job that fails or is cancelled on a still-untouched project
    /// closes it (as `openMidi`). `open(path:)` returns `nil` for a `.dawproject`.
    public func openDawproject(_ path: String)
    /// Waits for every pending open (tests).
    public func settle() async
    ```
  - The report sheet opens through `InterchangeModel.report` when the job ends, as 6a's jobs do. Its title is "Import Report" or "Export Report", and its summary starts "Imported" or "Exported".

- [ ] **Step 1: Write the failing tests.**
  - `DawprojectModelTests`, with a `FakeSession`:
    - `checkRunsDetachedAndKeepsTheLatest`: two quick `regionGain` changes → the second answer wins; `wontTransfer` is set after `settle()`.
    - `defaultNameComesFromTheCore`: the fake's "Song.dawproject" is shown, never built in Swift.
    - `exportWaitsForTheRunningJob`: with a running `.bounce`, `start` sets `waiting` and calls nothing; when the job ends, one `exportDawproject` with the path and `replace` is sent.
    - `cancelWaitingSendsNothing`.
    - `failedJobShowsItsMessage`: a Failed job row puts its message in `checkError` and opens no report.
    - `requestCarriesTheBlockRule`: the layout's `.fixedBars(2)` is in the sent row.
  - `DocumentStoreTests`:
    - `openingADawprojectMakesAProjectAtItsRate`: with no document open, info 44 100 → after `settle()`, one `newProject` at 44 100 and one `importDawproject`; the info closure ran off the main actor (L8).
    - `aRefusedFileMakesNoProject`: the info throws → no new document, and the alert "Import Failed" with the core's sentence.
    - `aCancelledImportClosesTheUntouchedProject`: the job is Cancelled → the made project closes. A project edited meanwhile stays.
    - `openPathRoutesByExtension`: `.dawproject` → `openDawproject`; `.DAWPROJECT` too; `.zip` → the "not a .vizdaw project" alert.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter Dawproject` and `--filter DocumentStore`. Expected: FAIL.
- [ ] **Step 3: Implement** the items above, following `ForDawModel` (generation counter, `blocked(by:)`, `begin`) and `openMidi` (closing an untouched project).
- [ ] **Step 4: Run** `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +10 Swift).
- [ ] **Step 5: Commit** `feat(app): DAWproject model, opening a .dawproject and queueing (plan 6d Task 12)`.

### Task 13: The sheet, menus, document types, name scans, README checks, e2e, backlog, gates

**Files:**
- Create: `app/Sources/VizDAW/DawprojectSheet.swift`
- Modify:
  - `app/Sources/VizDAW/AppCommands.swift`:
    - File › Import Project ▸ DAWproject… (an Open panel → `store.openDawproject`);
    - Export ▸ DAWproject…, shown first in the menu with `.help("Most complete: tracks, mix, plug-ins, automation")`;
    - File › Open's types gain `dawproject`;
  - `app/Sources/VizDAW/ProjectPanels.swift`: `chooseDawprojectToOpen()` and `chooseDawprojectFile(name:)`, through 6b's one private panel helper;
  - `app/Sources/VizDAW/DocumentView.swift` (the sheet's presentation);
  - `scripts/make-app.sh`:
    - a `CFBundleDocumentTypes` entry "DAWproject" (Viewer, Alternate, `org.vizdaw.dawproject`);
    - a `UTImportedTypeDeclarations` entry `org.vizdaw.dawproject` (extension `dawproject`, conforms to `public.zip-archive`);
  - `app/Tests/VizDAWAppModelTests/NameScanTests.swift` and `crates/core-control/tests/names.rs`: the format's origin and the other readers the format's README lists join `NAMES`;
  - `app/README.md`, `docs/feature-backlog.md`, `crates/vizctl/tests/e2e.rs`.

**Interfaces:**
- Consumes: Tasks 9–12.
- Produces:
  - **`DawprojectSheet`:**
    - a header line "Most complete: tracks, mix, plug-ins, automation";
    - a "Region gain" picker ("Apply to the audio" / "Leave out");
    - "What won't transfer" from `ReportText.groups(wontTransfer)`, or "Everything transfers.";
    - Cancel, and Export…, which opens the Save panel with `defaultName`. The panel confirms replacing, so a confirmed name sends `replace: true`;
    - the waiting line "Waiting for the running job to finish…" with Cancel.

    Theme tokens; every control accessibility-labelled.

- [ ] **Step 1: Write the e2e** `dawproject_round_trip_over_the_channel` (`#[ignore = "needs a window server and VIZDAW_E2E_APP"]`, throwaway home, offline):
  1. `app.new_demo`, and read its track count from `state.get document`.
  2. Hostile calls:
     - `file.export_dawproject` with `region_gain: "half"` → `InvalidParams`;
     - with a relative `path` → `InvalidParams` (the registry's `Path` kind refuses it; S7);
     - with no `path` and no `dry_run` → `dialog_required` "Save";
     - `file.import_dawproject` with `mode: "merge"` → `InvalidParams`.

     `window_numbers` is unchanged after them.
  3. `file.export_dawproject {dry_run: true}` returns a report without writing; the temp folder is still empty.
  4. `file.export_dawproject {path: <home>/demo.dawproject}` → the job's report summary starts "Exported". The file starts with `PK\x03\x04`. A second call without `replace` → `Failed` naming the file.
  5. `file.import_dawproject {path}` → a new window. Its `state.get document` has the demo's track count, and the job report holds "This file came from a VizDAW project".
  6. `file.interchange_report` on the new window returns the same report.
- [ ] **Step 2: Add README checks 282–290** after 281, under "### DAWproject (plan 6d)". Start with a line naming the tests that cover the rest:
  - `core-io`'s `zip.rs`, `xml.rs` and `dawproject_*.rs`;
  - `core-ffi/tests/dawproject.rs`;
  - `platform-macos/tests/plist.rs`;
  - `DawprojectModelTests`, `InterchangeCommandsTests` and the e2e `dawproject_round_trip_over_the_channel`.

  The steps:
  - 282: File › Export › DAWproject… opens the sheet with its header line, the Region gain picker and "What won't transfer". Choosing Leave out adds "Region gain" to the list (on a project with region gain). Cancel closes it.
  - 283: Export… proposes "<Song>.dawproject". After the job, the report opens with its summary and groups; Show in Finder selects the file.
  - 284: Export again to the same name. The panel asks to replace; Cancel there leaves the old file; Replace writes the new one.
  - 285: Start a bounce, then export a DAWproject. The sheet says it is waiting and exports when the bounce ends.
  - 286: File › Import Project › DAWproject… with the file from 283. A new untitled window opens, with:
    - the tracks, regions with their fade lengths and the crossfade's X;
    - notes with per-note expression in the Piano Roll;
    - the tempo ramp, markers, levels, sends and automation.

    Edit › Undo is unavailable. The report says "This file came from a VizDAW project".
  - 287: Double-click the `.dawproject` in Finder, and drop it on the Dock icon. Each does what 286 does.
  - 288: Import a file whose instrument is not installed. The track's instrument shows as a placeholder named after the plug-in, and the report says it is not installed and keeps its settings.
  - 289: Cancel an import from its progress. The new window closes and nothing new is in the project's media.
  - 290: Fill every Song info field in Project Settings, then export and import. The new project's Song info matches, and its comment has no "VizDAW export" line.
  - **Owner check (manual, only the owner can do; spec §12, 6d's part):**
    - export the demo song;
    - open it in the DAW you use that reads DAWproject;
    - check tracks, clips, fades, notes with expression, tempo, markers, levels, sends and automation;
    - export a DAWproject back from it and import that into VizDAW as a new project; check what arrived against the report.
- [ ] **Step 3: Add backlog rows** to `docs/feature-backlog.md`, in its style:
  - "DAWproject: readable audio entry names" (Ruling 9);
  - "DAWproject: crossfades as a negative fadeInTime" (Ruling 10);
  - "DAWproject: send pan, VCA channels and folder tracks" (spec §14 already lists folder tracks; append "send pan and VCAs (plan 6d)" to that row);
  - "DAWproject: the built-in Equalizer onto Multi-Q's bands" (spec §8: 6g; note "6d leaves it out with its bands listed");
  - "Third-party licence notices shown in the app" (the MIT notices of `rawzip`, `flate2`, `miniz_oxide`, `adler2`, `crc32fast`, `simd-adler32`, `quick-xml`, `memchr` (as Task 1's `cargo tree` lists them) and the vendored DAWproject XSDs; owner to choose where they show);
  - "DAWproject: `.clap-preset` container" (Ruling 8: raw stream; revisit if hosts agree on a container).
- [ ] **Step 4: Run all the gates:**
  - `cargo fmt --all -- --check`;
  - both clippies and `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `cargo test --workspace`;
  - `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test` (the name scans included);
  - the no-brand build;
  - `scripts/make-app.sh` (`plutil -lint` passes with the new types) and the e2e.

  Afterwards `pgrep -lf MacOS/VizDAW` prints nothing. Expected: all PASS (e2e 27, registry 175, README through 290, `VERSION` 9).
- [ ] **Step 5: Commit** `feat(app): DAWproject sheet, menus and document type; README checks 282–290, e2e and backlog (plan 6d Task 13)`.

## Self-Review

- **Spec coverage (6d's row of §13):**
  - **ZIP and XML readers and writers** → Tasks 1–2.
  - **§7.4 export:**
    - tracks, the notes/audio split, clips along the blocks, reference uses → Tasks 4–5 and Rulings 6 and 11;
    - WAV stored with deflated XML → Ruling 9 and Task 9;
    - `.aupreset` through CoreFoundation and CLAP raw state → Ruling 8 and Task 9;
    - `Application` and `MetaData` → Task 4;
    - the record, tags and snapshot → Ruling 16 and Tasks 4 and 9;
    - XSD checks → Tasks 3–5 and 8.
  - **§7.4 import:**
    - tracks, channels, buses and folders; plug-in matching and the placeholder; VST Left out; the built-in synth → Task 6;
    - re-sharing; beats and seconds; fades; warps; clip launcher; VCA; video; Equalizer and dynamics Left out; external audio and offline → Tasks 6–7 and 10.
  - **§7.1's DAWproject column**, row by row → Rulings 6–15 and Tasks 4–5. The "kept by a merge" rows go into the record (Ruling 16).
  - **§7.0's DAWproject use** → Ruling 10, Tasks 5 and 7.
  - **§7.8 write side** → Ruling 16 and Tasks 4, 6 (`ReturnInfo`) and 9. Plan 6e owns the rest of §7.8.
  - **§6.1** new project at the majority rate, no undo → Ruling 17 and Tasks 10 and 12. **§6.2** jobs, staging and partials → Tasks 9–10. **§6.3** report → every task. **§6.5** limits → Ruling 18 and Tasks 1–3 and 8.
  - **§9:** menus, the sheet, "What won't transfer", the report → Tasks 12–13. **§10:** two commands, hostile tests, `dry_run`, `dialog_required` → Task 11. **§11:** errors → each task's refusal sentences.
  - **§12:**
    - goldens → Tasks 3–5;
    - round trips → Task 8;
    - XSD → Tasks 3 and 8;
    - plug-in state → Tasks 9–10;
    - hostile files and limits → Task 8;
    - jobs and cancel → Tasks 9–10 and 12;
    - control channel → Task 11;
    - e2e, README checks and the owner check → Task 13.
- **Deliberately not here:**
  - the merge prompt, `mode: merge`, `project`, `snapshot` and `preview`, matching and the Conflicts group (6e);
  - the Multi-Q `Equalizer` mapping (6g, which comes after 6d);
  - AAF and OMF (6f, 6h).
- **Names are consistent across tasks:** every name used in a later task is the one an earlier task's Interfaces block produces (checked against each block).
- **Review Focus:** each line names its tests in the owning task.
- **Spec conflicts and questions for the owner:**
  1. Ruling 1: `rawzip` + `flate2` instead of the spec's `zip` crate.
  2. Ruling 2: the spec says DAWproject has "no release tags", but there is a `v1.0.0` tag. Its XSD lacks `Clip@enable` and `Send/Enable`, so our muted clips validate only against `main`.
  3. Ruling 10: the Reference documents a crossfade form (a negative `fadeInTime`) that the spec says does not exist. We export overlapping clips as the spec says, and read both forms.
  4. Ruling 12: the Reference does not fix the time base of a note's expression points; we write them relative to the note.
  5. L1: DAWproject export uses a new job kind, `ExportDawproject`, not spec §6.2's `ExportProject`, which 6b already uses for Export for another DAW.
  6. Ruling 17: until 6e, a file from VizDAW imports as a new project with a Note. `file.import_dawproject` takes only `mode: new`, rather than answering `dialog_required` "Return" (spec §10).
  7. Ruling 9: numbered audio entry names (`audio/001.wav`).
  8. The workspace's `rust-version` goes 1.85 → 1.86 for `quick-xml` 0.42.
