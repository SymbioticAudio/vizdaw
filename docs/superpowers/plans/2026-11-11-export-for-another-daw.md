# Export for Another DAW Implementation Plan (6b)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One sheet and one job write a folder "<Song> for another DAW" with audio stems that all start at the same point (bar 1, or the pickup's start), one Standard MIDI File per MIDI track, all the MIDI in one file, a tempo-map file and `About these files.txt`. A report lists what changed or was left out. An option adds the stems to this project in one undo step.

**Architecture:**
- **`core-io`** gains `for_daw` (platform-neutral). It owns the folder and file names, the default strips, the MIDI files (built on 6a's `midi_out`), the start-point sentence, the route's report items and the About text. It also gains `names::safe_part`, `write::PendingFolder` and `Report::plain`.
- **`core-ffi`** gains `for_daw.rs`, one `ExportProject` job:
  - it writes the MIDI files into a hidden staging folder `.<folder>.partial`;
  - it renders the stems into that folder through the 5f-2 bounce, which gains a start before bar 1 and leading silence;
  - it writes the About file and renames the folder into place.
  - "Add the stems to this project" uses the bounce's existing add-to-project path.
- The control channel gains `file.export_for_daw`. Swift gains `ForDawModel`, `ForDawSheet` and File › Export › For Another DAW…. The report sheet, Show in Finder and the queueing behind running jobs reuse 6a's code.

**Tech Stack:** Rust 2024 (`core-model`, `core-io`, `core-media`, `core-engine` read-only, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6.1 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-11-11-import-export-design.md`. Plan 6b's row of §13 covers §2 goals 4 and 7, §7.1's "For another DAW" column, §7.6, §9 (the For Another DAW menu item and sheet, "What won't transfer", the report), §10 (`file.export_for_daw`, its hostile-input tests, `dry_run`), §11, and §12 (the jobs and limits parts, the 6b e2e part, README checks, the 6b owner check). Builds on plan 6a (`docs/superpowers/plans/2026-11-11-smf-midi-clip.md`) and the 5f-2 bounce (`docs/superpowers/plans/2026-11-02-bounce-export.md`). Read the code that both plans built; this plan does not re-plan it.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`. Run Swift only through `scripts/build-ffi.sh` (after any FFI change), `scripts/swift.sh build|test` and `scripts/make-app.sh`. Never run `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Branch `sp6-import` (at `0804fb7` when this plan was written; plan 6a is finished on it). Never switch branches, never push.
  - Commit at the end of every task by explicit paths (never `git add -A`). Never commit `.superpowers/`, `.DS_Store` or generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`).
  - `Cargo.lock` does not change: no new crate and no new dependency (`core-ffi` already depends on `core-io`).
  - Every message ends with the executing session's own attribution lines: its harness's `Co-Authored-By` line naming the model actually running, then the session line, e.g.
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Then check with `git log -1 --oneline && git show --stat HEAD` (a commit without `DEVELOPER_DIR` fails silently).
- **Clean room and names.**
  - Never read `~/workspace/SVP/research`.
  - No Opcode, Logic, Apple (except existing platform facts), Midas, Soundcraft, NI, Microsoft, Ableton, Steinberg/Cubase or ROLI names in UI or repo text; say "other DAWs". The MIDI Association, General MIDI and MPE are specification names and are allowed.
  - **Ardour** (the source of the re-import idea, spec §2 goal 7) may be named only in docs, the backlog and third-party licence notices, whose notices are kept. Never name it in code, comments, tests, UI text or README steps.
  - The scans enforce this: `NameScanTests.noThirdPartyNamesInWhatTheAppShows` and `core-control/tests/names.rs`.
- **Track, not lane.** Text the app shows (labels, menus, sheets, report lines, alerts, the About file, README steps) says "track". Identifiers, command IDs, JSON keys and the file format keep `lane`. `core-ffi/tests/wording.rs` already scans `core-io`.
- **Never run `auval`**, in any form.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. The owner checks the views by eye from README steps 267–273.
  - Launch VizDAW only through the e2e (Task 5) with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - Tests never open real audio or MIDI, never trigger the microphone prompt or TCC, and never touch the real `~/Library/Application Support/VizDAW`, preferences, Trash or devices. Use temp folders (`tempfile`), `Session::debug_set_trash_dir`, `MemoryDefaults()`, the e2e's throwaway home and fakes only. Every `core-ffi` test file calls `vizdaw_ffi::host::force_offline()` through its `serial` helper.
- **Programmatic calls open nothing.** Control-channel commands and programmatic calls never open dialogs, sheets, menus, panels or windows. The report sheet and the For Another DAW sheet open only for jobs the user started from the UI.
- **Hostile input.** Every new command gets hostile-input tests at every layer (Tasks 2–3). Every name the export makes from user text passes `names::safe_part`. Every computed tick, frame and length uses `checked_*`/`saturating_*`; nothing wraps.
- **FFI safety.** No panic crosses the FFI: every new export returns `Result<_, VizError>` inside `ffi_guard` (`catch_unwind`). The job thread runs its MIDI rendering and file writing inside `std::panic::catch_unwind`; a caught panic fails the job with `internal error: …` and is a bug.
- **Swift 6.1.**
  - Generated bindings compile in Swift 5 mode; app modules and tests compile in Swift 6 mode with no new warnings.
  - No `@State` or `@FocusState` anywhere in this plan's code (view state lives in the `@Observable` models).
  - Never capture `[weak self]` or a non-Sendable value inside `MainActor.assumeIsolated`; bind a strong `let` first, as `DocumentWindow.swift` does.
  - Colours, spacing, radii and fonts come from the theme; every new control is accessibility-labelled.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`: use `.get()`, iterators and `checked_*`/`saturating_*`. Rust stable, edition 2024, GPL-3.0-only. `core-model`, `core-engine`, `core-media`, `core-io` and `core-control` stay free of OS APIs; everything Rust here runs on Linux.
- **TDD.** Every new test is run and seen to fail before the code that makes it pass.
- **Real-time.** This plan adds no real-time code and does not touch `core-engine`. If a task finds it must, it runs the rt-check gate from plan 6a's Global Constraints.
- **Format.** `core_model::file::VERSION` stays 8; the document format does not change.
- **Lessons from plan 6a's reviews (binding):**
  - **The core owns file naming.** Every name the UI shows or checks comes from the core (`ForDawCheckRow.folder`, `.files`); Swift never builds a file name.
  - **Never replace anything without explicit confirmation.** This route never replaces (Ruling 1). Existence is always `symlink_metadata` (lstat): a dangling link counts as taken.
  - **No file I/O or parsing on the main thread.** `export_for_daw` checks only text and the model before it spawns (Ruling 8). Its one carve-out is the one `bounce` already has: `refresh_media` and the plug-in capture. Say so in Task 2's report (F13). The app calls `export_for_daw_check` from a detached task with a generation counter, as 6a's checks do, and the check's lstat runs off the session lock.
  - **Every dropped item gets a report line.** That covers tracks, strips, notes, song info and colours (Task 1's `route_report_lists_everything_dropped`).
  - **Goldens are hand-written before the code and can fail.** This plan's goldens (`for_daw_tempo_map.mid`, `for_daw_about.txt`) are read with a copy of 6a's never-blessed `hand_golden` (`core-io/tests/midi_out.rs`). `VIZDAW_BLESS` never writes them, and a missing one fails (F4).
  - **A single interchange job runs at a time,** so UI flows that start one while another runs queue rather than fail (Task 4). The For-DAW sheet and dropped MIDI files queue. Other sheets keep 6a's readable `Busy` alert (F10).
  - **Cancel mid-job is tested** at every stage (Task 2): before the job runs, while the MIDI files are written, mid-render, after the render, and after the About file before the rename (F6). A late cancel or failure also discards the "Add" imports (F1).
- **UniFFI changes** (Task 2 only; every new field `#[uniffi(default)]` or `#[uniffi(default = None)]` where a record already exists):
  - `JobKindRow::ExportProject`;
  - records `ForDawRequestRow` and `ForDawCheckRow`;
  - `Session::export_for_daw` and `Session::export_for_daw_check`.
  - `FakeSession` gains both methods in Task 2, with the logs and answers that Tasks 3 and 4 use (F8). Every Swift `switch` over `JobKindRow` gains `.exportProject` in Task 2.
- **Control channel.** The registry goes 170 → 171 in Task 3. Every command is in the registry and handled (`everyRegisteredMenuCommandHasAHandler`).
- **Each task ends green.**
  - Always:
    - `cargo fmt --all` then `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches. Run the whole workspace in Tasks 2 and 5.
  - Tasks 2–5 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 5 also runs:
    - the no-brand build: `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <a fresh temp dir>`;
    - `scripts/make-app.sh`;
    - the e2e: `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`.
  - Known flaky:
    - the `damaged_files` proptest;
    - the MIDI listener tests (`platform-macos/tests/midi.rs`);
    - `core-ffi` catalog tests under parallel load;
    - the latency e2e timing.

    Re-run once before investigating, and say so in the report.
  - **Baseline:** measure at the start of Task 1: Rust passed/ignored, Swift, e2e 24, registry 170, README through 266, `VERSION` 8. Each task's last step gives the counts it adds. Report the measured counts; a slip in the writer's arithmetic is not a failure.
- **How code is given.**
  - New test files are written whole.
  - New implementation files get their public items with doc lines, plus the algorithm where the signature and tests leave a choice.
  - Changes to existing files follow the existing pattern named in the step. If a named anchor no longer matches, make the same change to the current text and say so in the report.

## Review Focus

These are the five failure modes a user is most likely to hit, most likely first. The spec implies them, but no ordinary test would exercise them. Each is pinned by named tests in its task.

1. **A song with a pickup.**
   - Examples: one beat before bar 1; one and a half beats in 4/4 (not a whole beat, so the MIDI file pads to a whole bar, plan 6a Ruling 8); a region that starts at the pickup.
   - Expected: every file starts at the same point. The stems' audio at that point lines up with the MIDI files' position 0, and the sheet and the About file say where that point is. Added stems land at the pickup's start, not at bar 1.
   - Pinned by `stems_and_midi_start_together` and `padded_pickup_gets_leading_silence` (Task 2), and `start_sentences` (Task 1).
2. **Names that collide or misbehave.**
   - Examples:
     - an existing "Song for another DAW", or a dangling symlink of that name;
     - a stale `.Song for another DAW.partial` left by a crash;
     - track names "A/B:C", ".hidden", 300 bytes of "é", or empty;
     - two tracks both called "Bass";
     - a song title with a slash.
   - Expected: the folder is numbered " 2" and the old one is untouched. A stale partial is passed over, never removed. Every name is safe, unique by its number and short enough with its `.partial`.
   - Pinned by `names_are_safe_and_numbered` (Task 1) and `a_second_export_numbers_the_folder` (Task 2).
3. **Cancel or failure part-way.**
   - Examples: cancel while the MIDI files are written, mid-render, or after the render but before the rename; an "Add to project" import that fails.
   - Expected: no export folder and no staging folder are left. `Media/` and `Media/.incoming/` hold exactly what they held before, and the document is unchanged.
   - Pinned by `cancel_at_every_stage_leaves_nothing` and `a_failed_add_leaves_nothing` (Task 2).
4. **Tracks that give no audio or no MIDI.**
   - Examples: a track on external MIDI; an empty track; a track with only muted notes; a muted track's strip; a strip the user unticked; a bus the tracks play through (its effects are not in their stems); a coloured strip.
   - Expected: each is in the report with its names, and none is silently missing.
   - Pinned by `route_report_lists_everything_dropped` (Task 1) and `external_midi_gets_a_midi_file_and_no_stem` (Task 2).
5. **Another job already running.**
   - Examples: dropped MIDI files still importing; a bounce; a second export; a plugin project whose bounce copies are in use.
   - Expected:
     - the core answers `Busy`;
     - the sheet's export waits and starts when the running job ends;
     - dropped MIDI files wait for the export;
     - the plugin copies are released when the export ends, so the next bounce works.
   - Pinned by `busy_states_refuse` and `plugin_copies_end_with_the_export` (Task 2), and `exportWaitsForTheRunningJob` and `droppedMidiWaitsForTheExport` (Task 4).

## Plan Rulings

These are this plan's decisions where the spec leaves a choice. Each names its cost if wrong.

1. **`path` is the folder the export folder goes into** (spec §10: "`path` (folder)"; there is no `replace`).
   - The core makes "<Song> for another DAW" inside it. If that name or its `.partial` is taken (lstat), it uses " 2", " 3" … " 9999". Nothing is ever replaced, so no confirmation is needed.
   - `<Song>` is `SongInfo.title`, else the sequence's name, else "Untitled".
   - *Cost if wrong:* a `replace` parameter and a confirmation, if the owner wants to overwrite (owner question).
2. **The common start point is `midi_out::file_start`**: the pickup's start when it is a whole number of beats, else whole bars before bar 1 (6a Ruling 8).
   - The bounce renders from `-start_offset` (frame 0 of the engine). For the padded case, the file stems get leading silence of the frames between the two points (`Plan.lead`).
   - `core-ffi`'s `for_daw` computes `lead` as `TempoTable::build(&s.tempo, file_start, project rate).tick_to_frame(-start_offset)`. The silence goes into the project-rate temporary, so the export's rate conversion scales it with the audio. `ForDawCheckRow.frames` and the About file's frames are at the *export* rate (F7: accepted, with the rate refined to where the samples are).
   - Added stems land at `-start_offset`, where their audio begins.
   - *Cost if wrong:* the MIDI writer would write odd pickups as a short bar instead.
3. **The stems are the bounce's file stems** (5f-2 Ruling 5: post-fader, before mute and solo), so a muted strip still gets its stem, with a Note.
   - "Add the stems to this project" uses the bounce's existing add-to-project path (spec §7.6), which places *heard* stems.
   - A *track* muted in the project (its lane flag) gets the Note "Track 'X' is muted in the project" from `route_report` when no MIDI file's report already carries it. A strip muted in the mixer gets Task 1's strip Note (F14).
   - *Cost if wrong:* importing the written files instead (owner question, since spec §7.6 also says "as heard").
4. **Default strips** are the strips this sequence's tracks play through, plus every bus that some strip sends to and no strip outputs to. These are, in mixer order:
   - the `Instrument(d)`, `InstrumentOutput(d, _)` and `BuiltinSynth` strips of every destination its events resolve to (`ResolvedEvent.dest` from `flatten`);
   - the `AudioPath` strip of every played region's `ResolvedEvent.audio_out` from `flatten`, as `core_engine::schedule` reads it (`schedule/audio.rs`). A region with no `audio_out` is unrouted and plays nowhere: it is Left out, "Audio regions with no output", with track names (F18).

   A chosen strip that no track of this sequence plays through is accepted. Its stem is silent and it is Left out, "Strips no track of this sequence plays through", by name (spec §10 hostile row).

   Master is refused as a stem ("The master is the mix; choose track and bus strips."). `strips: None` means the defaults; `Some([])` means MIDI files only.
   *Cost if wrong:* one function.
5. **File names.**
   - Stems are `Audio/NN <name>.<ext>`, numbered 01… in the chosen strips' mixer order. `<name>` is the track's name when exactly one track of the sequence plays through the strip, else the strip's name.
   - MIDI files are `MIDI/NN <track>.mid`, numbered separately in the sequence's track order.
   - NN has at least two digits.
   - *Cost if wrong:* one function. The spec writes "NN <track>", but stems are per strip.
6. **MIDI files are 6a's writer, unchanged:** type 1, 960 PPQ, `mpe: true`.
   - Each track's file is `to_smf` with `lanes: [l]`. `All MIDI.mid` holds every MIDI track. `Tempo map.mid` is the new `to_tempo_map_smf` (type 1 with the conductor track only).
   - A MIDI track is a track whose played events include a MIDI event (`flatten`, active take).
   - The report takes `All MIDI.mid`'s report once (`Tempo map.mid`'s when there is no MIDI track). It drops 6a's "Song info other than title and copyright" item, because the About file carries the song info, and adds a Note saying so.
   - `export_for_daw_check` renders only that one report (All MIDI or the tempo map), never the per-track files: the check needs the report, not the bytes (F12).
   - *Cost if wrong:* report wording.
7. **The job's order.**
   1. Check the destination and make the staging folder `.<folder>.partial` beside it, with `Audio/` and `MIDI/` inside.
   2. Render and write the MIDI files into it (fast, so it fails early).
   3. Render the bounce into `Audio/`.
   4. Write the About file (it needs the frame count).
   5. Make a last cancel check.
   6. Rename the folder (`rename_excl`).

   Any failure or cancel removes the staging folder recursively; this job made it, and nothing else is removed. A taken name at the rename fails the job, leaving nothing. Every MIDI, About and stem write uses `replace = false`.

   **The "Add" imports** belong to `for_daw::run` from the moment `bounce::run` returns Done. On any later cancel or failure (steps 4–6), it returns `Work::ExportForDaw { files, result: Err(..) }` (a cancel's `Err` is `"Cancelled"`, and the row's state is Cancelled). `apply_jobs` applies `files` only on `Ok` and otherwise discards them through the same `remove_unkept` path, because `Work::Cancelled` carries no files (F1).

   **Paths:** the plan built before the spawn holds stem paths *relative* to the export folder (`Audio/01 Bass.wav`). After step 1, `for_daw::run` rebases `plan.stem_files` onto the staging folder, then runs `partial_fits` on every final path and `outside` on the destination, all in the job (F3).

   Steps 1, 2 and 4 run inside `catch_unwind`. The rename and `PendingFolder`'s `Drop` are outside it, but unwinding still runs `Drop`, so nothing leaks (F21).
   *Cost if wrong:* a retry with the next number.
8. **No file I/O before the spawn.**
   - `Session::export_for_daw` checks the request text, the model and the format. Every file-system check runs first in the job, and a refusal fails the job with its sentence: the destination is a folder, it is outside the project and every package, and the names fit.
   - `export_for_daw_check` does lstat and renders the MIDI, so the app calls it detached.
   - *Cost if wrong:* those refusals move earlier.
9. **One interchange job at a time.**
   - `ExportProject` joins `interchange_idle`'s pending kinds and the bounce's busy check, and `inner.bounce_copies` ends when no `Bounce` *or* `ExportProject` job is pending.
   - `ExportProject` also joins `media_jobs_idle`'s list, because it reads the media as a bounce does. It joins the media-job list `open_external` checks and the session-close shutdown that stops a running bounce first (`session.rs`, "A bounce still running stops first") (F2).
   - The UI queues behind running jobs instead of showing `Busy` (Task 4).
10. **Report and labels.**
    - `Summary` gains `audio_files: u64`, written after tracks ("Exported 2 tracks, 3 audio files, 1 200 notes, the tempo map and 4 markers"). `tracks` counts the MIDI files' tracks.
    - The add-to-project undo step is "Add Stems". `JobRow.written` is `[<the export folder>]`.
    - Bounce problems (clipped samples, bypassed plug-ins, offline regions, underruns) become one Changed item, "Problems while rendering the audio", with each line as a detail.
11. **The sheet comes before the folder panel** (plan 6a Ruling 18), so "What won't transfer" shows before the user clicks Export.

## Pre-flight amendments (2026-11-11)

These come from `.superpowers/sdd/2026-11-11-export-for-another-daw/preflight-report.md`, and the controller accepted every proposed ruling. Each is applied in place in the task it touches; where a step and an older line disagree, the amendment wins.

- **F1** "Add" imports belong to `for_daw::run` after the bounce. Any later cancel or failure returns them in `Err`, and `apply_jobs` discards them (Ruling 7, Task 2).
- **F2** `ExportProject` joins `media_jobs_idle`, `open_external`'s list and the session-close shutdown; there are tests for Convert, external edit and close (Ruling 9, Task 2).
- **F3** Stem paths are planned relative to the export folder and rebased onto the staging folder in the job, where `partial_fits` and `outside` run (Ruling 7, Task 2).
- **F4** The goldens are read with 6a's never-blessed `hand_golden` (Global Constraints, Task 1).
- **F5** The About tempo line comes from the same `origin`/`map_changes` as `Tempo map.mid`, with the pickup meter named; test `tempo_line_with_a_pickup` (Task 1).
- **F6** `debug_for_daw_cancel_at` stages 1 (after the first MIDI file) and 2 (after About, before the rename) join the cancel test (Task 2).
- **F7** `lead` comes from `TempoTable` built from `file_start`, at the project rate of the temporary. The check's and the About file's frames are at the export rate; test `stems_align_at_another_rate` (Ruling 2, Task 2).
- **F8** `FakeSession` gains request logs, a settable check answer and job IDs in Task 2, for Tasks 3–4.
- **F9** The stale-partial test uses the dotted `partial_path` name (Task 1).
- **F10** With "Add", the sheet also waits for media jobs; other sheets keep the `Busy` alert (Task 4).
- **F11** A new `blockingEnded()` restarts waiting drops; `importEnded`'s guard stays; test `cancelledExportKeepsTheDropQueue` (Task 4).
- **F12** `a_thousand_tracks` is measured first, with a shared-source entry point if it is slow; the check renders only one report (Ruling 6, Task 1).
- **F13** The `refresh_media` and capture carve-out is named in the report; the check's lstat runs off the lock (Global Constraints, Task 2).
- **F14** A muted track gets a Note even without MIDI; README 270 separates a muted strip from a muted track (Ruling 3, Tasks 1, 5).
- **F15** README 267 also checks that Bounce… is unchanged after the shared rows (Task 5).
- **F16** One private folder-panel helper serves all three panels (Task 5).
- **F17** e2e step 4 counts `All MIDI.mid`'s `MTrk` chunks (Task 5).
- **F18** Audio strips come from `ResolvedEvent.audio_out`; unrouted regions and strips no track plays through are reported (Ruling 4, Task 1).
- **F19** `snapshot().undo_label` replaces `undo_label()` (Task 2).
- **F20** A backlog row covers a track-to-file map in the About file (Task 5).
- **F21** `catch_unwind` covers steps 1, 2 and 4; `Drop` covers the rest on unwind (Ruling 7).
- **F22** The off-macOS folder-rename fallback is named in Task 2's report.

---

### Task 1: `core-io`: names, the layout, the MIDI files, the report and the About text

**Files:**
- Create: `crates/core-io/src/names.rs`, `crates/core-io/src/for_daw.rs`
- Modify:
  - `crates/core-io/src/lib.rs` (the two modules);
  - `src/midi_out.rs` (`file_start`, `to_tempo_map_smf`, and `OTHER_INFO` becomes `pub` with a doc line);
  - `src/report.rs` (`Summary.audio_files`, `summary_line`, `Report::plain`);
  - `src/write.rs` (`free_folder`, `stage_folder`, `PendingFolder`).
- Test:
  - `crates/core-io/tests/for_daw.rs` (new) and `crates/core-io/tests/base.rs` (new cases);
  - hand-written goldens `crates/core-io/tests/data/for_daw_tempo_map.mid` and `for_daw_about.txt`, described in `tests/data/README.md`.

**Interfaces:**
- Consumes: 6a's `midi_out::{to_smf_until, SmfOptions}`, `smf::write`, `report::{Report, ItemKind}`, `write::write_partial`; `core_model::query::flatten`.
- Produces:
  - `names::safe_part(name: &str, reserve: usize) -> String`:
    - `/`, `:` and control characters become `-`, and a leading `.` becomes `-`;
    - trimmed, then cut at a character boundary to `255 - reserve` bytes;
    - empty gives `"Untitled"`.
  - `midi_out::file_start(s: &Sequence) -> Tick`: the tick at file position 0 (the `Origin.tick` of `origin`, without a report).
  - `midi_out::to_tempo_map_smf(doc: &Document, seq: SeqId, ppq: u16, cancel: &AtomicBool) -> Result<(SmfFile, Report), IoError>`. It builds the same conductor as `to_smf_until`, so factor the conductor into one `fn conductor(...)` that both call. The result is type 1 with that one track.
  - `report::Summary.audio_files: u64`. In `summary_line` it comes right after tracks ("1 audio file" / "N audio files").
  - `Report::plain(&self, verb: &str) -> String`, the same text as Swift's `ReportText.plain`:
    - the summary line;
    - then the groups "Changed", "Left out" and "Notes", each item as `"  <what>"` or `"  <what> (<count>)"`, its details as `"    <detail>"` and `"    and N more"`;
    - an empty report gives `"<summary>\nEverything transferred."`.
  - `write::free_folder(parent: &Path, name: &str) -> Result<PathBuf, IoError>`: the first of `name`, `name 2` … `name 9999` whose path and `.partial` are both absent by `symlink_metadata`. Refused past 9999.
  - `write::stage_folder(dest: &Path) -> Result<PendingFolder, IoError>`:
    - it makes `core_media::export::partial_path(dest)` with `create_dir`. If that already exists, it is Refused "… is left from an earlier export; remove it or choose another name".
    - `PendingFolder::path(&self) -> &Path`.
    - `PendingFolder::commit(self) -> Result<PathBuf, IoError>` uses `core_media::import::rename_excl`. `AlreadyExists` gives Refused "<name> exists; choose another name".
    - `Drop` removes the uncommitted folder with `remove_dir_all`.
  - `for_daw`:
    ```rust
    pub struct Layout {
        pub folder: String,                 // "Song for another DAW" (unnumbered)
        pub audio: Vec<(StripId, String)>,  // ("Audio/01 Bass.wav"), chosen strips in mixer order
        pub midi: Vec<(LaneId, String)>,    // ("MIDI/01 Bass.mid"), MIDI tracks in track order
        pub all_midi: Option<String>,       // Some("MIDI/All MIDI.mid") when `midi` is not empty
        pub tempo_map: String,              // "Tempo map.mid"
        pub about: String,                  // "About these files.txt"
    }
    impl Layout { pub fn files(&self) -> Vec<String> }  // write order: midi, all_midi, tempo_map, audio, about
    pub struct AboutAudio { pub container: &'static str /* "WAV", "AIFF", "CAF" */, pub bits: String /* "24-bit" */, pub rate: u32, pub frames: u64 }
    pub fn default_strips(doc: &Document, seq: SeqId) -> Result<Vec<StripId>, IoError>;          // Ruling 4
    pub fn midi_tracks(doc: &Document, seq: SeqId) -> Result<Vec<LaneId>, IoError>;              // Ruling 6
    pub fn layout(doc: &Document, seq: SeqId, strips: &[StripId], ext: &str) -> Result<Layout, IoError>; // Ruling 5
    pub fn midi_files(doc: &Document, seq: SeqId, l: &Layout, cancel: &AtomicBool)
        -> Result<(Vec<(String, Vec<u8>)>, Report), IoError>;                                     // Ruling 6
    pub fn route_report(doc: &Document, seq: SeqId, strips: &[StripId], midi: Report) -> Report;
    pub fn start_sentence(s: &Sequence) -> String;
    pub fn about_text(doc: &Document, seq: SeqId, l: &Layout, audio: Option<&AboutAudio>, report: &Report) -> String;
    ```
    `layout` refuses an unknown sequence or strip, a strip listed twice, and master, as `IoError::Refused` naming the strip. Name reserves (`safe_part`'s `reserve`):
    - a file part: `"NN ".len() + ext.len() + ".partial".len() + 2`;
    - the folder: `" for another DAW".len() + " 9999".len() + ".partial".len() + 1`.
  - **`route_report`'s items**, merged after `midi`. Each item counts once per name and carries the names as details:
    - Left out, "No audio for tracks playing external MIDI (their MIDI files are written)";
    - Note, "Tracks with nothing to play: no files";
    - Left out, "Strips not chosen";
    - Note, "Strip 'X' is muted in the project; its stem is written" (and the solo form, as 6a words it);
    - Left out, "Strip colours";
    - Left out, "Audio regions with no output" (track names; F18);
    - Left out, "Strips no track of this sequence plays through" (strip names);
    - Note, "Track 'X' is muted in the project", for a muted track the MIDI report does not already name (F14);
    - Note, "Stems are taken before the buses they play through";
    - Note, "Mix, effects and automation are part of the audio files" (only with stems);
    - Note, "Song info is in About these files.txt" (only with song info).

    It sets `summary.audio_files` to the number of stems.
  - **`start_sentence`**:
    - `start_offset == 0`: `"Every file starts at bar 1."`;
    - a whole-beat pickup of n beats: `"Every file starts 1 beat before bar 1, where the song's pickup begins."` ("n beats" for more than one);
    - a padded pickup: `"Every file starts 1 bar before bar 1, so the song's pickup fits."` ("n bars").
  - **`about_text`**: UTF-8, `\n` line ends, ending in one newline. Exactly this, in order:
    1. the folder name, then a blank line;
    2. `Start: <start_sentence>`;
    3. `Tempo: <bpm> BPM, <num>/<den> at the start. Tempo map.mid holds every tempo and meter change and the markers.` The BPM is `bpm_at(file_start)`, at most two decimals with trailing zeros dropped. The tempo and meter come from the same `origin`/`map_changes` the tempo-map file is written from, at file position 0 (F5).
       - With a whole-beat pickup, the meter part reads `<p>/<den> for the pickup, then <num>/<den> from bar 1`, e.g. `1/4 for the pickup, then 4/4 from bar 1`.
       - With a padded pickup it is the meter written at position 0.
    4. `Audio: <container>, <bits>, <rate with report's thousands grouping> Hz. Every audio file is <frames grouped> frames (<m>:<ss.s>) long: to the song's end plus the tail.`, or `Audio: none; no strips were chosen.`;
    5. when any song-info field is set: a blank line, `Song info`, then `  <Field>: <value>` for each set field in `SongInfo` order. The labels are Title, Artist, Album, Original artist, Composer, Songwriter, Producer, Arranger, Year, Genre, Copyright, Website and Comment, and a multi-line comment indents its later lines by 4 spaces.
    6. a blank line, `Files`, then `  <path>` for each of `l.files()` except the About file itself. `All MIDI.mid` gets ` (every MIDI track)` and the tempo map ` (tempo, meter and markers)`.
    7. a blank line, `What changed or was left out`, then `report.plain("Exported")`.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it in the task report.
- [ ] **Step 2: Write the goldens by hand, then the failing tests.**
  - **The small document** (6a Task 6's): sequence "Song" at 120 BPM, 4/4, C major; track "Bass" on the built-in synth, group 0 channel 1; one note, pitch 60, velocity `up7(100)`, tick 0, duration 15 360. Its only default strip is the built-in synth strip, which plays only "Bass".
  - **`for_daw_tempo_map.mid`** is exactly `4D546864 00000006 0001 0001 03C0`, `4D54726B 00000021` + `00FF0304536F6E67 00FF510307A120 00FF580404021808 00FF59020000 00FF2F00`: 6a's golden conductor alone, ntracks 1.
  - **`for_daw_about.txt`**, with `AboutAudio { container: "WAV", bits: "24-bit", rate: 48_000, frames: 96_000 }`:
    ```
    Song for another DAW

    Start: Every file starts at bar 1.
    Tempo: 120 BPM, 4/4 at the start. Tempo map.mid holds every tempo and meter change and the markers.
    Audio: WAV, 24-bit, 48 000 Hz. Every audio file is 96 000 frames (0:02.0) long: to the song's end plus the tail.

    Files
      MIDI/01 Bass.mid
      MIDI/All MIDI.mid (every MIDI track)
      Tempo map.mid (tempo, meter and markers)
      Audio/01 Bass.wav

    What changed or was left out
    Exported 1 track, 1 audio file, 1 note and the tempo map
    Notes
      Mix, effects and automation are part of the audio files
    ```
  - `tests/for_daw.rs`:
    - `small_files_match_the_goldens`:
      - `layout(small, seq, &default_strips(..), "wav")` gives exactly the paths above, with folder "Song for another DAW";
      - `midi_files` gives `MIDI/01 Bass.mid` and `MIDI/All MIDI.mid`, each byte-equal to 6a's `export_small_type1_960.mid`, and `Tempo map.mid` byte-equal to `for_daw_tempo_map.mid`;
      - `about_text` of the route report is byte-equal to `for_daw_about.txt`.
      - Both goldens are read with a copy of 6a's never-blessed `hand_golden`, so a missing or different file fails (F4).
    - `names_are_safe_and_numbered`:
      - track "A/B:C" gives `MIDI/01 A-B-C.mid`, ".hidden" gives `MIDI/02 -hidden.mid`, and "" (made through a strip name) gives `Untitled`;
      - a 300-byte "é…" name is cut at a character boundary so `.NN <name>.mid.partial` is at most 255 bytes;
      - two tracks named "Bass" give `01 Bass.mid` and `02 Bass.mid`;
      - the folder comes from `SongInfo.title` "My/Song" (`My-Song for another DAW`), else the sequence name, else "Untitled".
      - In `base.rs`, `free_folder`: free gives `name`; an existing folder, an existing file, a dangling symlink and a stale `.name.partial` folder (built with `core_media::export::partial_path(&parent.join(name))`) each give `name 2` (lstat). The stale partial is left in place, and an undotted `name.partial` does not count as taken (F9). `stage_folder` + drop leaves nothing; `commit` gives the folder; a `dest` that appears before commit is Refused, and the staging folder is gone afterwards.
    - `default_strips_are_track_strips_and_send_only_buses`:
      - included, in mixer order: a plug-in instrument's strip and its extra output strip, the built-in synth strip, an audio track's output strip, and a bus that only receives sends;
      - excluded: a bus a track's strip outputs to, master, and a strip no track of this sequence plays through;
      - `layout` with master, an unknown strip or a duplicate is Refused naming it.
    - `stem_names_follow_their_track`: a strip one track plays through is named after the track; an instrument strip two tracks share and a bus are named after the strip.
    - `midi_tracks_are_tracks_with_midi_that_plays`: a track with notes on an external destination is in; an audio-only track, an empty track and a track whose only note is muted are out.
    - `route_report_lists_everything_dropped`. In one document:
      - a track on external MIDI and an empty track;
      - an unchosen default strip, a muted strip and a coloured strip;
      - a track strip that outputs to a bus;
      - song info with a composer;
      - an inactive take.

      Each item above appears with its names, and "Song info other than title and copyright" does not. A second document has no MIDI track and holds an audio track muted in the project, plus a region with no output: the report gives the Note "Track 'Gtr' is muted in the project" and the Left out item for the region (F14, F18). 6a's "Inactive takes" appears once with count 1 (not once per MIDI file). `summary.audio_files` equals the chosen strips, and `summary.tracks` equals the MIDI tracks.
    - `tempo_line_with_a_pickup` (F5): with a one-beat pickup in 4/4 at 120 BPM, the About line reads `Tempo: 120 BPM, 1/4 for the pickup, then 4/4 from bar 1. …`. The meter in `Tempo map.mid` at delta 0 is 1/4 (`FF 58 04 01 02 …`).
    - `start_sentences`: the three sentences above for `start_offset` 0, one beat in 4/4, and one and a half beats in 4/4; `file_start` is `Tick(-61_440)` (one 4/4 bar) for the last.
    - `a_thousand_tracks`: 1 000 MIDI tracks of one note give 1 000 files ending `MIDI/1000 T999.mid`, plus All MIDI and the tempo map, in under 10 s.
      - Measure it first (F12). Each `to_smf` call flattens the whole sequence, so if it takes over about 5 s, add an internal shared-source entry point to `midi_out` (one flatten for every file) rather than raising the limit. Report the time either way.
    - `cancel_stops_the_midi_files`: with `cancel` set, `midi_files` is `Err(Cancelled)`.
  - `base.rs`:
    - `report_plain_matches_the_sheet`: 120 details given as 50 give `"  Muted notes (120)"`, 50 lines `"    bar k"`, then `"    and 70 more"`; an empty report gives `"Exported nothing\nEverything transferred."`.
    - `summary_line` with `tracks 2, audio_files 3, notes 1200, tempo_map, markers 4` is `"Exported 2 tracks, 3 audio files, 1 200 notes, the tempo map and 4 markers"`.
- [ ] **Step 3: Run** `cargo test -p core-io --test for_daw --test base`. Expected: fail to compile.
- [ ] **Step 4: Implement** the items above. `midi_files` checks `cancel` between files and passes it to `to_smf_until`.
- [ ] **Step 5: Run** `cargo test -p core-io`, `cargo test -p core-ffi --test wording`, `cargo test -p core-control --test names` and both clippies. Expected: PASS (about +11; every 6a golden still byte-equal).
- [ ] **Step 6: Commit** `feat(io): export-for-another-DAW layout, names, MIDI files, report and About text (plan 6b Task 1)` with every file above.

### Task 2: `core-ffi`: the bounce from the start point, the job, the rows and the session

**Files:**
- Create: `crates/core-ffi/src/for_daw.rs`
- Modify:
  - `crates/core-ffi/src/lib.rs`;
  - `src/jobs.rs` (`JobKindRow::ExportProject`, `Work::ExportForDaw`);
  - `src/bounce.rs` (`stems_plan`, `Plan.lead`, leading silence in `render`, the frames passed to `write_partial`);
  - `src/session.rs`:
    - `Session::bounce`'s body after the plan becomes `fn spawn_render`;
    - `ImportAs::FOR_DAW`;
    - `apply_jobs`, `discard`, `interchange_idle`, the bounce busy check and the `bounce_copies` end check;
    - `media_jobs_idle`, the media-job list `open_external` checks, and the close-time shutdown that stops a running bounce (F2);
    - `debug_for_daw_cancel_at(stage: Option<u32>)` (`test-hooks`; stage 1 sets cancel after the first MIDI file is written, stage 2 after the About file is written; F6);
    - the two new methods.
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift`. It gains the two methods plus `forDawChecks: [ForDawRequestRow]`, `forDawExports: [ForDawRequestRow]`, a settable `forDawCheckAnswer: Result<ForDawCheckRow, VizError>` and job IDs from its `interchangeJob` factory, for Tasks 3–4 (F8).
  - `app/Sources/VizDAWAppModel/ProjectMedia.swift` (`jobText` "Exporting for another DAW…", `failureTitle` "Export Failed"); every other Swift `switch` over `JobKindRow`.
- Test: `crates/core-ffi/tests/for_daw.rs` (new). Its fixtures are built as `tests/bounce.rs`'s `project` builds them: a saved package in a temp folder, 48 kHz float, generated CAF media. Copy the helpers it needs.

**Interfaces:**
- Consumes: Task 1; `bounce::{Plan, Job, run, dest_path, outside, partial_fits, ContainerRow}`; `interchange::{report_row, io_error}`; `apply_import_as`.
- Produces:
  - ```rust
    #[derive(uniffi::Record)] pub struct ForDawRequestRow {
        pub seq: u64,
        pub strips: Option<Vec<u64>>,   // None: the defaults (Ruling 4); Some([]): MIDI files only
        pub container: ContainerRow, pub depth: BitDepthRow, pub rate: u32, pub dither: bool,
        pub path: String,               // the folder the export folder goes in (Ruling 1); "" for a check
        pub add_to_project: bool,
    }
    #[derive(uniffi::Record)] pub struct ForDawCheckRow {
        pub report: ReportRow,          // "What won't transfer" (the route report, verb "Exported")
        pub folder: String,             // the name it would make, numbered when `path` is given
        pub files: Vec<String>,         // Layout::files(), relative to the folder
        pub strips: Vec<u64>,           // the strips it would render (the defaults for None)
        pub start: String,              // start_sentence
        pub frames: u64,                // the render's frames before the tail, with the lead, at the export rate (F7)
        pub warning: Option<String>,    // the bounce check's sentences (external MIDI, offline, space)
    }
    ```
  - `Session::export_for_daw(req: ForDawRequestRow) -> Result<u64, VizError>`:
    - `Model` at once for: an unknown sequence; a strip that is unknown, listed twice or master; a format `ExportFormat::check` refuses; path text `dest_path` refuses (empty, relative, NUL, over 1 024 bytes, `..`); `add_to_project` with `Some([])`.
    - `Busy`: while recording, inside an open edit, and while a `Bounce`, `ImportMidi`, `ExportMidi` or `ExportProject` job is unapplied; with `add_to_project` also while a media job is unapplied (`media_jobs_idle`).
    - It does no file I/O before the spawn (Ruling 8), apart from `refresh_media` and the plug-in capture, as `bounce` does.
  - `Session::export_for_daw_check(req) -> Result<ForDawCheckRow, VizError>`: the same checks. With a non-empty `path`, it also checks the destination and numbers the folder (lstat only). It renders the MIDI on a clone off the session lock. Nothing is written.
  - `bounce::stems_plan(doc: &Document, seq: SeqId, strips: &[StripId], start: Tick, end: Tick, fmt: ExportFormat, files: Vec<PathBuf>, lead: u64, add: bool, offline: &BTreeMap<MediaId, OfflineReasonRow>) -> Result<Plan, VizError>`:
    - it builds a stems `Plan` for files named relative to the export folder, with `stem_files = (f, f)` and `import_names` from the files' stems. `for_daw::run` rebases them onto the staging folder in the job (Ruling 7, F3);
    - `plan` and `stems_plan` share one helper for the strips, the range frames, the format and the external-lane and offline-region counts;
    - `plan` keeps refusing `start < 0` and sets `lead: 0`.
  - `Plan.lead: u64` (project-rate frames, Ruling 2): `render` writes `lead` zero frames to each *file* stem's writer before the render (not to heard stems), and each file stem's `write_partial` is told `lead + stats.frames`.
  - `fn spawn_render(&self, inner: &mut Inner, plan: Plan, kind: JobKindRow, body: impl FnOnce(bounce::Job, &AtomicU32, &AtomicBool) -> Work + Send + 'static) -> Result<u64, VizError>`: the capture, the copies and the hooks, moved out of `bounce`.
  - `ImportAs::FOR_DAW`: label "Add Stems", origin `Bounced`, tracks named after their item. It clamps the tick to `-start_offset` of the target sequence instead of 0; give `ImportAs` a `pickup: bool` field.
  - `Work::ExportForDaw { files: ImportFiles, target: Option<ImportTargetRow>, result: Result<Report, String>, written: Vec<String> }`.
    - `apply_jobs`, on `Err`: it discards `files` (`remove_unkept`), and the row is Failed or, for `"Cancelled"`, Cancelled (F1).
    - `apply_jobs`, on `Ok`: it applies `files` with `FOR_DAW` (when there are any), sets the job row's `report` and `written` and `interchange_report`, and keeps the folder whatever the apply does (an apply error fails the row with `written` kept).
    - `discard` removes `files`' imports, as for `Work::Bounce`.
  - `for_daw::run(job: ForDawJob, progress: &AtomicU32, cancel: &AtomicBool) -> Work`. It follows Ruling 7 and wraps `bounce::run`: `Work::Bounce { failed: Some(_) }` fails the job and `Work::Cancelled` cancels it, and both drop the `PendingFolder`. After a Done bounce it keeps the imports and returns them in every outcome (F1). Steps 1, 2 and 4 run inside `catch_unwind` (F21). The target tick for "Add" is `-start_offset`.

- [ ] **Step 1: Write the failing tests** in `tests/for_daw.rs`. They use temp folders only, `serial` with `force_offline`, and `debug_hold_jobs`, `debug_bounce_cancel_at`, `debug_bounce_fail_import` and the new `debug_for_daw_cancel_at` from the `test-hooks` feature.
  - `writes_the_folder_layout`: a "Bass" MIDI track on the built-in synth and a "Gtr" audio track with a region, exported to `tmp`.
    - The job ends Done with `written == [tmp/"Song for another DAW"]`.
    - The folder holds exactly the check's `files` (two stems, two MIDI files, the tempo map, the About file).
    - No `.partial` remains anywhere under `tmp`.
    - `interchange_report()` equals the job's report.
    - The About file's `Audio:` line names the rendered frames.
  - `stems_and_midi_start_together`:
    - With a pickup of one beat and a click region at `-start_offset`: every stem's first sample over −60 dBFS is at frame 0 (±1); `MIDI/01 Bass.mid`'s note is at delta 0; the About file and the check's `start` say "1 beat before bar 1".
    - Every stem has the same frame count, which is the range plus the tail.
  - `padded_pickup_gets_leading_silence`: with a pickup of one and a half beats in 4/4 at 120 BPM, `file_start` is one bar before bar 1, so each stem begins with `lead` = 2.5 beats = 1.25 s (60 000 frames ± 1) of exact zeros, then the click.
  - `a_second_export_numbers_the_folder`: a second export makes "Song for another DAW 2", leaving the first folder's files byte-equal and its listing unchanged. A dangling symlink named "Song for another DAW 3" makes the third export "… 4".
  - `external_midi_gets_a_midi_file_and_no_stem`: a track on an external destination gets `MIDI/NN <track>.mid`, no stem, and the report's Left out item names it.
  - `add_to_project_is_one_undo_step`: `add_to_project: true` with a one-beat pickup.
    - The tracks increase by one per stem, each region starts at `-start_offset`, and `snapshot().undo_label == Some("Add Stems")` (F19).
    - One `undo` makes `debug_document_json` equal to before. The folder still exists.
  - `cancel_at_every_stage_leaves_nothing`. Cancel:
    - while held, before the job runs;
    - with `debug_for_daw_cancel_at(Some(1))`, after the first MIDI file is written into the staging folder;
    - with `debug_bounce_cancel_at(Some(0.4))` (mid-render);
    - with `Some(1.0)`, which sets cancel as the bounce finishes, after the "Add" imports were handed over and before the About file;
    - with `debug_for_daw_cancel_at(Some(2))`, after the About file and before the rename (F6).

    Each, with and without `add_to_project`, ends `Cancelled`. Afterwards there is no export folder and no `.…partial` folder, the listings of `Media/` and `Media/.incoming/` are equal to before (the late cases with `add_to_project` prove F1), and the document JSON is unchanged.
  - `a_failed_add_leaves_nothing`: `debug_bounce_fail_import(Some(0))` with `add_to_project` ends `Failed`, leaving no folder, the same `Media/` and the same document.
  - `check_writes_nothing_and_names_the_folder`:
    - with `path: ""`, the check gives folder "Song for another DAW", the layout's files, the default strip IDs, the start sentence and `frames > 0`;
    - with `path: tmp` holding that folder, it gives "… 2";
    - `tmp`'s listing is unchanged after both.
  - `hostile_requests_are_refused`. Each is `Err(Model)` at once, or the job ends Failed with a sentence naming the path or field; never `Internal`, and nothing is written:
    - at once: `path` empty, relative, with NUL, 5 000 bytes or with `..`;
    - in the job: `path` that is a file, a FIFO, `/dev/zero`, a symlink loop, missing, inside the open project's package, or inside another `.vizdaw` folder;
    - at once:
      - strips `[999]`, `[s, s]` and `[master]`;
      - `rate` 0, 7 999 and 384 001;
      - an unknown `seq`;
      - `Some([])` with `add_to_project`.
  - `busy_states_refuse`:
    - while a for-DAW job is held, `bounce`, `export_midi`, `import_midi` and a second `export_for_daw` are `Err(Busy)`;
    - while a bounce is held, `export_for_daw` is `Err(Busy)`;
    - while recording (offline, an armed track), it is `Err(Busy)`;
    - with `add_to_project` while a Convert job is held, it is `Err(Busy)`;
    - while a for-DAW job is held, `convert_media` (and a Format change) is `Err(Busy)`, and so is opening a region in the external editor (F2);
    - closing the session while a for-DAW job runs stops it, leaving no folder, no staging folder and no imports (F2).
  - `stems_align_at_another_rate` (F7): an export at 44 100 Hz from a 48 kHz project with a 1.5-beat pickup has lead = 1.25 s × 44 100 (±1 frame), and the check's and the About file's frames equal the written files' frames.
  - `plugin_copies_end_with_the_export`: a project with a test-plugin insert exports, and then a bounce starts and ends Done (the copies were released).
- [ ] **Step 2: Run** `cargo test -p core-ffi --test for_daw`. Expected: fail to compile.
- [ ] **Step 3: Implement** the items above. `tests/bounce.rs` must pass unchanged: `plan`'s behaviour does not change. The task report names the main-thread carve-out (F13). It also says that off macOS, `rename_excl`'s fallbacks handle files only: a folder commit there fails cleanly (the job fails, nothing is left), and no Linux run tests it (F22).
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +12 Rust; Swift unchanged apart from the new enum case and `FakeSession`).
- [ ] **Step 5: Commit** `feat(ffi): export-for-another-DAW job on the bounce, with staging, numbering and add to project (plan 6b Task 2)` with every file above.

### Task 3: The control channel: `file.export_for_daw`

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (`FOR_DAW_PARAMS` and the entry after `file.export_midi_clip`);
  - `crates/core-control/tests/commands.rs` (count 171; the sentence gains "6b adds file.export_for_daw");
  - `app/Sources/VizDAWAppModel/InterchangeCommands.swift` (`exportForDaw`, `forDawRequest`, the handler);
  - `ControlCommands.swift` (`ControlJobs.answer` treats `.exportProject` as `.exportMidi`: Done gives `.ok(status: report?.summary)`, and the failure tail lists `written`);
  - `app/Sources/VizDAW/ControlChannel.swift` (`whenJobEnds` replies `.interchange` for `.exportProject`).
- Test: `crates/core-control/tests/commands.rs`, `app/Tests/VizDAWAppModelTests/InterchangeCommandsTests.swift`

**Interfaces:**
- Consumes: Task 2.
- Produces: the registry entry `file.export_for_daw`, "For Another DAW…", menu "File", `document: true`. Its parameters:
  - `path` (`File`): the folder to put the export in;
  - `seq` (`Int 0..=i64::MAX/2`);
  - `strips` (`Text`): strip IDs, comma-separated, or `none` for MIDI files only; absent for the defaults;
  - `container` (`Choice ["wav","aiff","caf"]`);
  - `depth` (`Choice ["16","24","32f"]`);
  - `rate` (`Int 8000..=384000`);
  - `dither`, `add_to_project` and `dry_run` (`Flag`).

  Its dialog is "Save", answered by `path`, with `always: false` and `when: "no path and no dry_run"`.
- `InterchangeCommands.forDawRequest(_ args: [ControlArg], _ m: DocumentModel) -> Parsed<ForDawRequestRow>`:
  - defaults: the shown sequence; WAV at the project's rate and depth; dither on at 16 bits and off for 32f;
  - `strips` that is not IDs or `none`, or repeats an ID, is `invalid_params` naming the command and `strips`.
- The handler:
  - `dry_run` first: `Self.dryRun` with `exportForDawCheck`, path or not; it answers `.interchange(report: check.report, written: [])`.
  - Then, without `path`, `.dialog("Save", "path")`; otherwise `.job(...)`.
  - Nothing opens a panel or sheet.

- [ ] **Step 1: Write the failing tests.**
  - core-control `export_for_daw_checks_its_params`:
    - the minimal form `{path: "/tmp/x"}` validates, and `{dry_run: true}` validates;
    - refused as `InvalidParams`: `container: "mp3"`, `depth: "8"`, `rate: 0`, `rate: 384001`, a relative `path`, extra keys;
    - `{}` answers `DialogRequired` naming "Save".

    The existing `every_command_survives_random_params` (2048 cases) covers it.
  - Swift `InterchangeCommandsTests`:
    - `forDawBuildsTheRequest`: `{path, strips: "3,5", container: "aiff", depth: "16", rate: 44100, add_to_project: true}` gives exactly that request with dither on. Without `strips` it gives `strips == nil`, and `strips: "none"` gives `[]`.
    - `forDawChecksItsArguments`: `strips` "1,,2", "a", "1,1" and "" are each `invalid_params` naming `file.export_for_daw` and `strips`, with no session call.
    - `forDawDryRunWritesNothing`: `dry_run` with and without `path` calls only `exportForDawCheck` and answers `.interchange` with that report, never a dialog.
    - `forDawWithoutPathAsksForSave`.
    - `everyRegisteredMenuCommandHasAHandler` passes with the new ID.
- [ ] **Step 2: Run** `cargo test -p core-control` and `scripts/swift.sh test --filter InterchangeCommands`. Expected: FAIL.
- [ ] **Step 3: Implement** in the style of 6a's `smfRequest` and handler.
- [ ] **Step 4: Run** `cargo test -p core-control`, both clippies, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (registry 171; about +1 Rust, +4 Swift).
- [ ] **Step 5: Commit** `feat(control): file.export_for_daw (plan 6b Task 3)` with the files above.

### Task 4: The app's model (`VizDAWAppModel`)

**Files:**
- Create: `app/Sources/VizDAWAppModel/ForDawModel.swift`
- Modify:
  - `DocumentModel.swift` (accessor `forDaw`, made once, like `bounce`);
  - `ProjectMedia.swift` (`pollJobs`):
    - an ended `.exportProject` job the UI started calls `interchange.jobEnded` (report, and `written` for Show in Finder);
    - a failed one calls `forDaw.jobFailed`;
    - every ended job of a `ForDawModel.blocking` kind calls `forDaw.jobEnded` and the new `interchange.blockingEnded()`, which only calls `startNextDrop()`. `importEnded` keeps its `dropJob` guard and its rule that a cancelled *drop* job clears the queue (F11).
  - `Interchange.swift`: `jobEnded` keeps `written` for `.exportProject` too; `startNextDrop` waits while a blocking job is listed.
- Test: `app/Tests/VizDAWAppModelTests/ForDawModelTests.swift` (new), `InterchangeTests.swift`

**Interfaces:**
- Consumes: Tasks 2–3; `BounceModel.rates`/`otherRates`; 6a's detached-check pattern (`InterchangeModel.run`, generation counters, `settle()`).
- Produces:
  - `@Observable @MainActor public final class ForDawModel` (weak document):
    - `shown` (showing resets: WAV, the project's rate and depth, dither at 16 bits, `addToProject` false);
    - `strips: [StripRow]` (every strip but master), `stems: Set<UInt64>`;
    - `container`, `depth`, `rate`, `otherRate`, `rateChoice`, `setOtherRate(_:)`, `dither` and `addToProject`, as in `BounceModel`;
    - `check: ForDawCheckRow?`, `error: String?`, `waiting: Bool`, `job: UInt64?`, `running`, `progress`;
    - `refresh()`. The first check after showing sends `strips: nil` and sets `stems` from `check.strips` without another check. Later changes send `Some(stems.sorted())`. Each runs in `Task.detached`, and only the latest generation lands.
    - `start(folder: String)`. If `document.jobs` lists a `.bounce`, `.importMidi`, `.exportMidi` or `.exportProject` job, it keeps the request and sets `waiting`. With `addToProject`, a `.format`, `.convert`, `.relink` or `.externalEdit` job also makes it wait (F10). Otherwise it calls `document.startJob(.exportProject, "Export Failed")`.
    - `cancelWaiting()`;
    - `jobEnded(_ job: JobRow)`: starts the kept request once no blocking job is listed;
    - `jobFailed(_:message:) -> Bool`: the sheet opens again with `error`.
    - `static let blocking: Set<JobKindRow>` (the four kinds above), shared with `InterchangeModel.startNextDrop`.

- [ ] **Step 1: Write the failing tests** with `FakeSession`.
  - `ForDawModelTests`:
    - `showResetsToTheCoresDefaults`: a project at 44.1 kHz/16-bit, with the fake's check answering `strips [2, 5]`. Showing gives WAV, 44 100, 16-bit, dither on, `stems == [2, 5]`, and exactly one check call, with `strips == nil`.
    - `everyChangeRefreshesTheCheckOffTheMainActor`:
      - each change of stems, container, depth, rate or "Add" calls the check once, with `strips == Some(sorted)`;
      - with two changes in a row, only the second result lands;
      - the call is made from a detached task.
    - `startSendsTheChoices`: 32-bit float sends `dither: false`; `start(folder: "/tmp/x")` sends `path "/tmp/x"` and the chosen strips.
    - `exportWaitsForTheRunningJob`:
      - with a running `.importMidi` job in `document.jobs`, `start` makes no `exportForDaw` call and sets `waiting`;
      - after `pollJobs` sees that job end, exactly one call is made;
      - `cancelWaiting` before that makes none.
    - `aFinishedExportShowsTheReportWithTheFolder`: a Done `.exportProject` job sets `interchange.report`, `reportShown` and `written == [folder]`.
    - `aFailedExportReopensTheSheet`: a Failed one shows the sheet with the message, and no alert.
    - `noStringSaysLane`: every string the model makes says "track".
  - `ForDawModelTests.addWaitsForMediaJobs` (F10): with `addToProject` and a running `.convert` job, `start` waits. Without `addToProject` it starts at once.
  - `InterchangeTests.droppedMidiWaitsForTheExport`: two `.mid` paths dropped while an `.exportProject` job runs make no `importMidi` call. When it ends, the first starts.
  - `InterchangeTests.cancelledExportKeepsTheDropQueue` (F11): a *cancelled* `.exportProject` job leaves both waiting drops queued, and the first then starts.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter ForDaw` and `--filter Interchange`. Expected: FAIL.
- [ ] **Step 3: Implement** to pass the tests.
- [ ] **Step 4: Run** `scripts/swift.sh build` and `scripts/swift.sh test`. Expected: PASS (about +8 Swift).
- [ ] **Step 5: Commit** `feat(app): export-for-another-DAW model, queued behind running jobs (plan 6b Task 4)`.

### Task 5: The sheet, the menu, README checks, e2e, backlog, gates

**Files:**
- Create: `app/Sources/VizDAW/ForDawSheet.swift`
- Modify:
  - `app/Sources/VizDAW/BounceSheet.swift`: extract its Format/Bits/Rate/Dither rows into `struct AudioFormatRows: View`, bound to either model through a small protocol `AudioFormatChoices` that `BounceModel` and `ForDawModel` adopt;
  - `app/Sources/VizDAWAppModel/ForDawModel.swift` (conforms to `AudioFormatChoices`);
  - `MidiSheets.swift`: `WontTransfer` loses `private`;
  - `ProjectPanels.swift`: `Panels.chooseExportFolder()`, an `NSOpenPanel` for folders with `canCreateDirectories`, prompt "Export Here" and message "Choose where to put the folder for another DAW.". It goes through one private `chooseFolder(prompt:message:)` that `chooseBounceFolder` and `chooseMidiExportFolder` also use (F16).
  - `AppCommands.swift`: Export submenu, after "MIDI Clip File…";
  - `DocumentView.swift`: attach the sheet beside `BounceSheet`;
  - `app/README.md`, `docs/feature-backlog.md`, `crates/vizctl/tests/e2e.rs`.
- Test: `scripts/swift.sh test` (the name scans), the e2e

**Interfaces:**
- Consumes: Task 4.
- **The sheet** (spec §9), theme tokens only, every control accessibility-labelled:
  - the title "Export for Another DAW" and the line "Audio stems and MIDI files for any DAW";
  - the strips checklist (`"Stem for <strip>"`);
  - `AudioFormatRows`;
  - "Add the stems to this project";
  - the check's `start` sentence and length ("1:32.4 + tail");
  - the check's `warning`;
  - `WontTransfer(report: check?.report, …)`.

  Export opens `Panels.chooseExportFolder()` and calls `start(folder:)`. While `waiting`, the sheet shows "Waiting for the running import, export or bounce to finish…" with Cancel; while `running`, progress and Cancel.
- **The menu item** is "For Another DAW…", using `keyBindings.row(InterchangeCommands.exportForDaw)?.title`. It opens the sheet.

- [ ] **Step 1: Build the sheet, the shared rows, the panel and the menu item** as above. Say in the task report that the sheet comes before the panel (Ruling 11) and that `BounceSheet` looks unchanged.
- [ ] **Step 2: Write the e2e** `exports_for_another_daw_over_the_channel` (`#[ignore = "needs a window server and VIZDAW_E2E_APP"]`, throwaway home, offline):
  1. `file.create_demo`.
  2. `file.export_for_daw` with `dry_run: true` returns an `interchange` result with a report and `written: []`, and `tmp` stays empty.
  3. `file.export_for_daw {path: tmp}`: `written[0]` is `tmp/<demo song> for another DAW`, holding `Tempo map.mid`, `About these files.txt`, `MIDI/All MIDI.mid` and at least one `Audio/*.wav`, and nothing ending `.partial`.
  4. `file.import_midi` of `MIDI/All MIDI.mid` at tick 0 adds as many tracks as that file has `MTrk` chunks after the conductor (counted in the test from the file's bytes; `state.get document`) (F17).
  5. `media.import` of each stem adds one track per stem.
  6. `file.export_for_daw {path: tmp, add_to_project: true}` makes "… 2", adds one track per stem, and gives `undo_label` "Add Stems".
  7. `file.interchange_report` returns its report.
  8. A `path` inside the demo's package fails naming the path, and no `path` answers `dialog_required` naming "Save".
- [ ] **Step 3: Add README checks 267–273** after 266, under "### Export for another DAW (plan 6b)", with a line naming the tests that cover the rest:
  - 267: File › Export › For Another DAW… shows the sheet:
    - the line under the title;
    - every track's strip and each send-only bus ticked;
    - Format, Bits, Rate and Dither as in Bounce;
    - "Add the stems to this project" (off);
    - the start sentence and length;
    - "What won't transfer", which updates as you change any option.
    - Then open File › Bounce…: it looks and behaves as before (the shared format rows; F15).
  - 268: Export asks for a place, then writes "<Song> for another DAW" there, with `Audio/`, `MIDI/` (one file per MIDI track plus `All MIDI.mid`), `Tempo map.mid` and `About these files.txt`. Exporting again makes "… 2", and the first folder is untouched. The report sheet opens, and Show in Finder shows the folder.
  - 269: Open `About these files.txt`: start, tempo, meter, sample rate and format, the song info, every file, and the report.
  - 270: A track on an external MIDI destination gets a MIDI file and no stem, and the report says so. Mute a track's strip in the mixer: its stem is still written, and the report says the strip is muted. Mute an audio track itself: the report names the track as muted (F14). Untick a strip: the report lists it.
  - 271: Give the song a one-beat pickup. The sheet and the About file say every file starts one beat before bar 1. Import the folder's `All MIDI.mid` and a stem into a new project at its start: they line up.
  - 272: Tick "Add the stems to this project": new tracks named after the stems at the start point; one Undo ("Add Stems") removes them all, and the folder stays.
  - 273: Cancel during a long export: no folder is left and the project is unchanged. Drop two MIDI files, then start an export at once: the sheet waits, then exports after the imports.
  - **Owner check (manual, only the owner can do):** drop the folder's stems and MIDI files at bar 1 in the DAW you use that reads only audio and MIDI files. Check that they line up and that the tempo is right. Repeat with a song that has a pickup, placing the files where the About file says.
- [ ] **Step 4: Add backlog rows** to `docs/feature-backlog.md`, in its style:
  - one file-name cleaner for `bounce::safe_name`, `interchange::clip_name` and `core_io::names::safe_part`;
  - Swift's `ReportText.plain` and `core_io::Report::plain` are the same text twice: serve it through the FFI;
  - a choice of stems that include the buses each track plays through;
  - a stem that renders silent is still written (only external MIDI is known before the render);
  - `replace` for the export folder, if the owner wants it (Ruling 1);
  - added stems are the heard stems while the files ignore mute and solo (Ruling 3);
  - the re-import idea's origin (Ardour, spec §2 goal 7), credited here, nowhere in the app;
  - the About file lists files, not a track-to-file map: stems shared by several tracks are named after the strip (spec §7.6 "the track list"; F20).
- [ ] **Step 5: Run all the gates:**
  - `cargo fmt --all -- --check`;
  - both clippies and `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `cargo test --workspace`;
  - `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`;
  - the no-brand build;
  - `scripts/make-app.sh` and the e2e.

  Afterwards `pgrep -lf MacOS/VizDAW` prints nothing. Expected: all PASS (e2e 25, registry 171, README through 273, `VERSION` 8).
- [ ] **Step 6: Commit** `feat(app): For Another DAW sheet and menu, README checks 267–273, e2e (plan 6b Task 5)` with the files above.

## Self-Review

- **Spec coverage (6b's row of §13, §7.6):**
  - the folder and its five kinds of file → Task 1 (names, MIDI, About) and Task 2 (stems, staging, rename);
  - every file starting at bar 1, or at the earlier pickup point, with the sheet and the About file saying so → Rulings 2 and 11, Tasks 1–2 and 5;
  - the same length for every stem → `stems_and_midi_start_together`;
  - silent external-MIDI stems listed → Task 1's report and Task 2's test;
  - the bounce's format choices → Tasks 2 and 5 (`AudioFormatRows`);
  - "Add the stems to this project" on the bounce's path → Task 2 (`ImportAs::FOR_DAW`);
  - §7.1's column (flattened current sequence, active take, built-in synth stem, external MIDI with MIDI only, tempo map and markers in the tempo-map file, song info in the About file, the mix baked in) → Tasks 1–2;
  - §9's menu item, sheet, "What won't transfer" and report → Tasks 4–5;
  - §10's command, `dry_run`, `dialog_required`, `busy` and hostile tests → Tasks 2–3;
  - §11 (no half files; one-sentence refusals) → Task 2;
  - §12 (cancel at every stage, e2e, README checks, the owner check) → Tasks 2 and 5.
- **Deliberately not here:** the DAWproject and AAF menu items (6d, 6f); `ImportProject` (6d); the shared e2e that re-imports DAWproject and AAF (6d, 6f extend it).
- **Names are consistent across tasks:**
  - `names::safe_part`, `write::{free_folder, stage_folder, PendingFolder}`, `Report::plain`, `Summary.audio_files`;
  - `midi_out::{file_start, to_tempo_map_smf}`;
  - `for_daw::{Layout, AboutAudio, default_strips, midi_tracks, layout, midi_files, route_report, start_sentence, about_text}`;
  - `ForDawRequestRow`, `ForDawCheckRow`, `JobKindRow::ExportProject`, `Work::ExportForDaw`, `bounce::stems_plan`, `Plan.lead`, `spawn_render`, `ImportAs::FOR_DAW`, `export_for_daw(_check)`, `debug_for_daw_cancel_at`;
  - `InterchangeCommands.exportForDaw`/`forDawRequest`, `ForDawModel` (`blocking`, `jobEnded`, `jobFailed`), `InterchangeModel.blockingEnded`, `AudioFormatRows`, `ForDawSheet`, `Panels.chooseExportFolder`.
- **Review Focus:** each line names its tests in the owning task.
- **Spec questions for the owner:**
  1. Rulings 1 and 3: no `replace`, and the added stems are the heard stems.
  2. Ruling 5: stems are named after the track when one track plays through the strip.
