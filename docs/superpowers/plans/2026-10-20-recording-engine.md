# VizDAW 5c — Recording Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The third plan of sub-project 5. VizDAW records: armed lanes record audio from a device's input (the output device's own, or another device joined in a private aggregate) and MIDI from CoreMIDI sources, with replace and overdub, punch, pre-roll, loop passes with keep and discard, and takes; captured audio and MIDI land at the timeline position the performer heard (device and graph latency taken off); every pass, with the edits made while it ran, is one undo step "Record"; recordings that a crash leaves are offered at the next open; and the offline device's fake input and loopback make all of it testable without hardware or a microphone prompt.

**Architecture:** The real-time thread only copies and counts; every mapping happens on the control side.
- **`core-model`**: `Lane.input` (`LaneInput::Audio { channels } | Midi { port, channel }`) and `Lane.monitor` (`Auto | Input | Off`) as lane edits; `Command::Record(RecordPass)`, which applies a whole pass (new media, cleared ranges, events, regions, takes) or nothing; history groups (`begin_group`/`end_group`) that turn everything made while recording into one step.
- **`core-media`**: the input ring (lock-free records of whole blocks, stamped with *run frames*) and `RecordWriter`, a thread that writes one recording-mode CAF per armed audio lane, fills lost blocks with silence and syncs every 2 s.
- **`core-engine`**: `Renderer::process_input` takes the device's input; a `RecordTap` pushes it into the ring with run stamps and pushes *marks* (which schedule frame a run frame played: the roll, every loop wrap, every resync); live input routes play MIDI thru to a lane's destination through an input-chain hook (empty until 5g) and record armed lanes, and monitored audio input is mixed into its path's `Regions` node; the offline backend gains the fake input (signals or loopback).
- **`core-ffi`**: a pure planner turns runs, marks, the input delay, the graph latency and the record range into a `RecordPass`; `Session.record` / `stop` / `keep_pass` / `discard_pass` / `record_state` run the recording (writer lifecycle, history group, disk checks, input loss); crash recovery; lane inputs and `inject_midi` over the FFI; the host opens live input only when a lane needs it and routes CoreMIDI sources into the engine.
- **`platform-macos`**: HAL input (duplex IOProc, private aggregate device, per-direction latency, input delay from the IOProc time stamps, MIDI scheduled at the audible time), the microphone permission status, and a CoreMIDI listener that keeps source names and time stamps.
- **`core-control` and Swift**: `RecordModel` (Record from the transport's modes, the poll following a pass, the crash-recovery alert), the `transport.record` and pass commands, five new channel commands and `state.get record` / `input`.

**Tech Stack:** Rust 2024 (`core-model`, `core-media`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `platform-macos` with `objc2` 0.6, `objc2-core-audio` 0.3.2, `objc2-core-foundation` 0.3.2 and `objc2-core-midi` 0.3.2, `vizctl`), Swift 6 / SwiftPM (SwiftUI, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`. This plan covers the spec's 5c row in §13, except synth tracking, which moves to a follow-up plan (Ruling 1):
- §4.5 lanes (`input` with `InputChannels`, `monitor`), §4.7 recording pass and history groups;
- §6.1 devices (HAL input, duplex, private aggregate, input only when needed, microphone permission), §6.2 the input ring and writer (overflow as silence, clip counts, disk checks), §6.3 latency compensation (audio, MIDI, external MIDI at audible time, the trim), §6.4 record modes and transport (`RecordSpec`, arming, pre-roll, record range, replace and overdub, loop passes, keep/discard, takes, step-entry refusal), §6.5 finishing a pass, recorded media and undo, crash recovery, §6.6 monitoring and MIDI thru (with §6.8's chain as an empty hook);
- §12.1 `transport.record` (handler), `record.keep_pass`, `record.discard_pass`, `record.set`, `lane.set_input`, `lane.set_monitor`, `midi.inject`, `state.get record` and `input`, the fake input (`VIZDAW_AUDIO_INPUT=fake`, `VIZDAW_AUDIO_LOOPBACK=<frames>`); §12.2 `record_overflow` and `input_lost` (as the pass ending with a message); §12.3 the recording tests (apply/undo property, replace in range, overdub, loop passes to takes with keep/discard, history group, loopback within ±2 frames, overflow as silence, crash recovery, project format), `rt-check` of the input ring and monitoring, the record e2e and the manual checklist's recording steps.

The parent spec is `2026-10-15-vizdaw-design.md` (§5 tracks, takes and undo of recording passes; §6 recording and CoreAudio I/O). Plans 5a (`2026-10-18-audio-playback.md`) and 5b (`2026-10-19-regions-in-editors.md`) are complete on `sp5-audio` at `98931a8`; their ledgers (`.superpowers/sdd/archive/5a-*.md`, `.superpowers/5b-*.md`) carry the items under "Carried and parked items". Read both specs.

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. The Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools.
  - A `git commit` without `DEVELOPER_DIR` fails silently, so check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
  - Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift only through the scripts.** `scripts/build-ffi.sh` (after any Rust change, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`. Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` (head `98931a8` before this plan's commit, plus that commit). Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists (by path, never `git add -A`), plus `Cargo.lock` when a manifest changed (no task here changes it: the one manifest edit only turns on features of existing dependencies). Never commit `.superpowers/` or `.DS_Store`.
  - End every commit message with the two attribution lines your dispatch gives you. The blocks below show:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Use the `Co-Authored-By` line naming the model you actually are.
- **Clean room.** Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any research material. No Opcode, Midas or Steinberg assets, names, artwork or branding anywhere in code, UI text, icons or docs.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e test (Task 15) or `vizctl launch` with `VIZDAW_AUDIO=offline`, and quit it when done (`vizctl quit`; `pkill -f "VizDAW --control"` if a test left one). At the end of a task that launched the app, check that `pgrep -lf MacOS/VizDAW` prints nothing, and remove the `/tmp/vze*` homes your e2e runs left.
  - Never write to the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any audio device's settings. Tests use temp directories (`tempfile`); sessions that save get a trash folder (`Session::debug_set_trash_dir`); the e2e sets `VIZDAW_TRASH_DIR` inside its throwaway home.
- **Tests never open real audio input and never trigger the microphone prompt** (macOS TCC). This is how:
  1. Every `core-ffi` test that plays or records calls `vizdaw_ffi::host::force_offline()` first: the host is the offline backend, which opens no audio hardware. Its input is the fake input chosen with `vizdaw_ffi::host::set_fake_input` (Task 8), never the environment.
  2. In builds with the `test-hooks` feature (every `core-ffi` test binary) the live host can never open input either: `host::live_input_allowed` returns false without asking macOS (Task 11), and `host::listen_to_midi_sources` connects nothing (Task 12).
  3. `platform-macos` tests cover the HAL input code through pure functions only (`audible_ns`, `input_delay`, `deinterleave` on a hand-built buffer list, `aggregate_description` building a dictionary). No test calls `CoreAudioBackend::open` with `input_enabled`, creates an aggregate device, or calls `permission::microphone()` (only its mapping `access_of`).
  4. The e2e runs the app with `VIZDAW_AUDIO=offline` and `VIZDAW_AUDIO_INPUT=fake`. The Swift tests use `FakeSession`.
  5. Real input, the aggregate device and the permission prompt are checked by eye, by the user, through `app/README.md` steps 129–136 (Task 15). Implementers never run them.
- **Crates and dependencies.**
  - Rust stable, edition 2024. No new crate and no new dependency (the spec allows only `blake3`, added in 5a). Task 11 turns on the `CFArray`, `CFDictionary` and `CFNumber` features of `objc2-core-foundation`, which `platform-macos` already depends on (`Cargo.lock` does not change), and links the AVFoundation system framework (`#[link]` in `permission.rs`; `.linkedFramework("AVFoundation")` in `app/Package.swift`).
  - License GPL-3.0-only.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. New non-test Rust code uses `.get()`, iterators and `checked_*`/`saturating_*` instead of indexing and unwraps.
- **Platform-neutral cores.** `core-model`, `core-media`, `core-engine` and `core-control` stay free of OS APIs (the one `libc::statvfs` call is `cfg(unix)` with a `None` fallback, beside 5a's `pread`). macOS code lives in `platform-macos` and behind `cfg(target_os = "macos")` in `core-ffi`. Linux is deferred: do not try to make Linux build or pass.
- **Real-time rules (spec §3).** The real-time thread never allocates, frees, locks or does file I/O. Everything new on it is marked `// RT`: `RecordTap`'s methods, `RingWriter::push`/`set_end`/`close`, `Renderer::live_midi`/`copy_monitored`/`region_run`/`audio_run`, `RegionRun::add_monitor`, `InputChain::process`, `MidiRoute::matches`, `MonitorRoute::heard`, `InputCopy::fill`, the fake input's `fill_fake`, and the HAL's `deinterleave`. Taps and input routes arrive as boxed engine commands and leave through the retire ring; the ring's `Arc` is dropped only on the control side. **rt-check gates** (Tasks 5 and 6, and Task 15's final run): `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record` (Task 5 adds `rt_alloc_record`; before Task 5 the gate is the first two tests).
- **Media files are write-once; recordings are never deleted.** Recordings go to new files under `Media/Recorded/` (`CafWriter::create` never overwrites). Nothing opens an existing media file for writing. The retention pass keeps every file a recording is writing, a finished pass is waiting to apply, a failed pass left, or a crash left and the user has not answered (Ruling 15); an untitled project's recordings go to the Trash at close instead of being removed with its scratch package; files are moved to the Trash, never unlinked. Crash recovery only imports or moves to the Trash.
- **File compatibility.** `core_model::file::VERSION` stays 3. `Lane.input` and `Lane.monitor` are written only when set (`serde(default, skip_serializing_if)`), so projects that use no input read and write exactly as before (Ruling 2).
- **FFI safety.** No panic crosses the FFI: every new exported function or method returns `Result<_, VizError>` (or nothing) and runs inside `ffi_guard`. The session `inner` lock is never held while a recording's files are finished (`Session::finish_recording` takes the recording out, releases the lock, joins the writer, then locks again). Device reopening for input (Task 11) runs under the host lock like the other device opens (5a m2, accepted).
- **UniFFI changes.**
  - New records and enums: `LaneInputRow`, `MonitorModeRow`, `InputInfoRow` (Task 8); `RecordSpecRow`, `RecordLaneRow`, `RecordStateRow` (Task 9).
  - Changed records: `LaneRow.input: Option<LaneInputRow>` and `LaneRow.monitor: MonitorModeRow`, appended without a UniFFI default (an enum cannot have one here); the only Swift literal, `FakeSession`'s `lane(…)`, is updated in Task 8. `RecordStateRow.recovered: Vec<String>` is appended with `#[uniffi(default)]` in Task 10.
  - Enums that gain variants (appended last): `LaneEdit::{SetInput, SetMonitor}` (Task 8); `StateTargetRow::{Record, Input}` and `ControlResult::{Record, InputState}` (Task 14; `ControlResult::Input` already names UI input).
  - New `Session` methods: `input_info`, `inject_midi` (Task 8); `record`, `keep_pass`, `discard_pass`, `record_state` (Task 9); `recovered_recordings`, `recover_recordings` (Task 10). New functions: `set_record_latency_trim` (Task 9), `set_audio_input_device` (Task 11).
  - UniFFI generates `SessionProtocol` from every exported `Session` method, so `FakeSession` (`app/Tests/VizDAWAppModelTests/FakeSession.swift`) implements each new method in the task that adds it.
- **Swift.**
  - The generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests compile in Swift 6 mode with no new warnings (the `MemoryDefaults` Sendable warning is older).
  - Never use SwiftUI's `@State` or `@FocusState` in new views (the Command Line Tools cannot expand them); view state lives in `@Observable` models. Closures stored by views capture `[weak self]`.
  - **Semantic colours only** in new UI (system and SwiftUI styles, no RGB or hex). New controls carry text or accessibility labels. The record controls themselves (button, popover, lane pickers, meters) are plan 5d's.
  - New model tests go to `VizDAWAppModelTests`.
- **Control channel** (control spec §6, §7).
  - Programmatic calls never open dialogs: the crash-recovery alert never shows while `ControlChannel.active` (the channel answers with `record.recover`); no new command has a dialog.
  - Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`; the Swift test `everyRegisteredMenuCommandHasAHandler` compares the two. The registry count test moves from 119 to 124 in Task 14.
  - Every new command gets hostile-input tests: by type and range in `core-control` (`record_and_lane_commands_check_their_params`), in the app (`midiInjectSendsHexBytes`, `laneInputsAndMonitorsAreLaneEdits`) and in the core (`hostile_injections_are_refused`, `hostile_inputs_are_refused_and_change_nothing`, `bad_requests_are_refused_and_recording_holds_the_transport`).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Test waits.** New `core-ffi` integration tests that wait for a recording, the engine or a job poll with a deadline from `common::wait_limit()` (`crates/core-ffi/tests/common/mod.rs`, which reads `VIZDAW_TEST_WAIT_SECS`), never a fixed sleep for the result itself.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test` for the crates the task touches (the workspace for Tasks 9 and 15).
  - Tasks 5 and 6 also run the rt-check gates above.
  - Tasks 8–12 also run `cargo clippy -p core-ffi --lib -- -D warnings`: the workspace clippy builds `core-ffi` with its `test-hooks` feature (its tests turn it on), so code under `cfg(not(feature = "test-hooks"))` is linted only by this.
  - Tasks 8–15 also run `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 15 also runs `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests: `core-ffi` catalog tests under parallel load, the `damaged_files` proptest and the MIDI listener tests (`platform-macos/tests/midi.rs`). Re-run once before investigating, and say so in the report.
  - Baseline at `98931a8`: Rust 1105 passed, 0 failed, 10 ignored; Swift 757 tests in 89 suites; e2e 6 tests.
- **How code is given.**
  - New files are written whole (`cat > … <<'VIZDAW_EOF'`).
  - Changes to existing files are exact **Find / Replace** excerpts of the tree the previous task left, applied in order with an editor (the Edit tool). Each Find occurs exactly once in the file when it is applied.
  - If an excerpt does not match because a fix landed meanwhile, make the same change to the current text and say so in the report. If a name in the plan differs from the tree, use the tree's and say so.
- **Spec values used verbatim:**
  - "**4 s** capacity, single producer and single consumer"; "It flushes the CAF header every 2 s"; "Below **1 GiB**, a warning is shown. Below **100 MiB**, recording stops cleanly"; "every 10 s while it runs";
  - "`Media/Recorded/<lane name> <yyyy-mm-dd HHmmss>.caf`";
  - "The input buffer's first frame lands at `T − (out_time − in_time) − output_latency − input_latency + user_trim`"; "subtract the output latency"; "expect alignment within ±2 frames";
  - "channel 1 carries a 440 Hz sine at −12 dBFS; channel 2 carries an impulse every 0.5 s; `VIZDAW_AUDIO_LOOPBACK=<frames>` instead feeds the output back into the input after that many frames";
  - "`count_in_bars: u8` (0–4) and `pre_roll_bars: u8` (0–8)"; "`passes_as_takes: bool` (default true)"; "Starting Record while step entry is on is refused with a message";
  - "`record.keep_pass` keeps all passes so far. `record.discard_pass` drops every pass since the last keep"; "becomes a new take named "Take n", and the last becomes the active take";
  - "Recovered recordings: import into a new lane / move to Trash";
  - "`kAudioAggregateDevicePrivateKey = 1`) with the output device as clock master and drift correction on the input".

## Review Focus

These are the five failure modes a user is most likely to hit. The spec implies them but no requirement spells them out. Each is pinned by named tests in its owning task.

1. **Editing, undoing or dragging while recording.** The user renames a lane, undoes it, redoes it, drags a note, or presses Stop in the middle of a drag; or undid a step just before pressing Record.
   - Expected: edits made while recording stay undoable and redoable one by one until Stop; Stop during a drag waits for the drag to end; then the pass and everything made while recording are one step "Record"; a step undone before Record is never redone into the pass; undo never reaches below where recording started.
   - Pinned by `a_group_makes_one_record_step_of_everything_inside_it`, `a_group_that_made_nothing_keeps_the_redo_stack` (Task 3), `edits_made_while_recording_join_the_record_step` and `a_pass_finished_during_a_drag_lands_when_it_ends` (Task 9).
2. **A recording that ends by itself.** The interface is unplugged, the disk fills, the song ends without a loop, another window plays.
   - Expected: the pass ends as if Stop were pressed, keeps every frame recorded, lands as one "Record" step, and the user is told why ("Recording stopped: the audio device went away; what was recorded is kept." / "the disk is almost full"); a warning comes first below 1 GiB; nothing panics or half-applies.
   - Pinned by `a_device_that_goes_away_ends_the_pass_cleanly`, `a_full_disk_ends_the_pass_and_keeps_what_was_recorded` (Task 9), `the_tap_reads_closed_when_its_renderer_goes` (Task 5), and the e2e's end-of-song pass (Task 15).
3. **Losing a take.** The user saves while recording, closes an untitled project, crashes, or the pass fails to apply.
   - Expected: a file being written is never trashed; a finished take is kept while any history state names it; an untitled project's recordings go to the Trash at close, never unlinked; after a crash the next open offers the file (import or Trash) and keeps it until answered; a failed pass keeps its files for that offer.
   - Pinned by `saving_while_recording_never_trashes_the_file_being_written`, `closing_an_untitled_recording_moves_it_to_the_trash` (Task 9), `a_recording_a_crash_left_is_found_and_offered`, `declined_recordings_go_to_the_trash` (Task 10), `a_file_that_cannot_be_made_leaves_nothing_behind` (Task 4).
4. **A take that lands in the wrong place.** Device latency, plugin delay compensation, loop wraps, an xrun mid-pass, an edit that recompiles the schedule, a tempo whose frames per tick are not whole, a negative latency trim.
   - Expected: audio lands where it was heard within ±2 frames; MIDI lands where it was played against what was heard; regions never reach outside their file; pre-roll audio stays as a handle.
   - Pinned by `a_looped_back_click_lands_within_two_frames_of_where_it_played` (Task 9), `a_region_on_an_awkward_tempo_still_lands_on_its_frame`, `the_device_delay_and_graph_latency_come_off_the_file`, `regions_stay_inside_their_file_and_range` (Task 7), `a_block_the_transport_skipped_is_captured_and_marked_after`, `a_schedule_swap_while_recording_resyncs` (Task 5), `armed_lanes_record_at_the_run_heard_when_the_message_arrived` (Task 6).
5. **Hostile or careless channel input.** `midi.inject` with SysEx, running status, a 2000-byte string or a bad port; `lane.set_input` channel 0, 257 or a pair past the last channel; a lane that does not exist; Record with nothing armed, a count-in, an empty punch, a channel the device lacks, a device at another rate.
   - Expected: refused with a clear `invalid_params` or `Model` error before anything changes; nothing is half-started.
   - Pinned by `record_and_lane_commands_check_their_params` (Task 14), `midiInjectSendsHexBytes` (Task 14), `hostile_injections_are_refused` (Task 8), `hostile_inputs_are_refused_and_change_nothing` (Task 2), `bad_requests_are_refused_and_recording_holds_the_transport`, `a_device_at_another_rate_refuses_recording` (Task 9).

## Plan Rulings

These are decisions the spec leaves open. Each is implemented and tested as stated, in the format "what — why — cost if wrong".

1. **Ruling (scope split):** this plan is the recording engine. **Synth tracking moves to a follow-up plan, "5c-2 — Synth tracking"**: `LaneInput::Audio.return_of`, the destination's `return_latency_ms` (and `destination.set_return_latency`), the `return_of` alignment shift, Measure (`destination.measure_return`), the fake synth (`VIZDAW_FAKE_SYNTH`), Track Synth (`lane.track_synth`), and the synth-tracking tests and e2e (spec §4.5 `return_of`, §4.5a, §6.7, the §12.1 destination and track-synth commands, the §12.3 "Synth tracking" list and its e2e line). The spec's §6.3 "External MIDI output … is when the event's frame is heard … pins this with a test" is in this plan (Task 11), because MIDI recording needs it too.
   - Why: with fifteen tasks this plan is already larger than 5b; synth tracking builds only on what this plan delivers (fake input channel 3 is reserved silent for it; recordings, marks and the planner's input delay are the hooks the shift adds to). Adding `return_of` later needs no file migration: it is a new field of a new enum variant with `serde(default)`.
   - Cost if wrong: one more plan; nothing here is rewritten by it.
2. **Ruling (file format):** `Lane.input: Option<LaneInput>` and `Lane.monitor: MonitorMode` are `serde(default, skip_serializing_if = …)`, as the 4e groove precedent; `VERSION` stays 3. A project without inputs is byte-identical to before.
   - Cost if wrong: a 5b build opening a project with an input reports it damaged (an unknown field) rather than "made by a newer VizDAW".
3. **Ruling (run frames and marks):** the tap counts *run frames*: output frames from the first block that plays after the tap is installed, continuous across wraps, stops and lost blocks. It pushes a mark `(run, schedule frame, wrap)` at the roll, at every loop wrap (`wrap: true`) and whenever the transport did not move with the run (an xrun, a missed deadline, a busy block) or a schedule swap remapped it (a resync). Marks go through their own 4096-slot ring; MIDI through a 16 384-slot ring; a full ring is counted (`lost_marks`, `lost_midi`) and marks the pass "with gaps".
   - Why: loop wraps, PDC, xruns and tempo edits are then pure arithmetic on the control side; the real-time side only stores atomics.
   - Cost if wrong: none (the planner is pure and tested).
4. **Ruling (the input ring):** one ring per recording carries only the device channels armed audio lanes use (sorted, each once), sized 4 s plus block headers (`RING_SECONDS`). A block is written whole or lost whole (`lost_blocks`, reported as `overflows` in `state.get record`). The writer fills a gap between stamps, and the end up to the last run, with silence, so file frame *k* is always run frame *k*. MIDI-only recordings have no ring channels and no writer.
   - Cost if wrong: one lost block costs the block's frames, as the spec's "the lost frames are written as silence".
5. **Ruling (the writer):** one `vizdaw-record` thread per recording; one recording-mode CAF per armed audio lane in the project format, mono or stereo as its `InputChannels`; `sync_data` every 2 s (the CAF's `data` size stays −1 until finished, so a crash leaves a readable file); files are made before the recording starts, and when one cannot be made nothing starts and none is left. File names: `Media/Recorded/<lane name> <yyyy-mm-dd HHmmss>.caf`, then `… 2.caf`, `… 3.caf`, never an existing file or a path any history state names (compared without case). Lane names lose `/ \ :` and control characters and a leading dot, at most 100 characters, "Lane" when nothing is left.
   - Cost if wrong: file names only.
6. **Ruling (latency compensation):** the device reports the input delay of a block: the output and input buffers' sample-time difference plus the output and input latencies (each device + stream latency + safety offset, read when the stream opens). The planner places input file frame *k* at the timeline frame heard at output run `k − D`, `D` = input delay − latency trim + graph latency `L` (the spec's formula, with PDC). Recorded MIDI is stamped at the output run heard when it arrived and placed at the frame heard at `run − L`. The HAL's `BlockTime.host_time_ns` becomes the block's audible time (output time + output latency), so MIDI out and MIDI in share the audible clock (§6.3). The trim is `set_record_latency_trim(frames)`, clamped to ±192 000, process-wide (5d's Settings › Recording sets it). The delay of the first captured block holds for the pass. A region starts on a tick, and its source offset follows the frame that tick plays at, so the audio lands on the captured frame exactly.
   - Cost if wrong: misreported device latency shows as a constant offset, which the trim fixes.
7. **Ruling (the fake input):** `FakeInput::Signals`: channel 1 a 440 Hz sine at −12 dBFS (0.251 188 64), channel 2 a 1.0 impulse every 0.5 s from absolute frame 0, channel 3 silent (5c-2's fake synth will use it). `FakeInput::Loopback(n)`: channels 1–2 are the output's left and right `max(n, one block)` frames later (the offline driver's block is 512); the fake device reports half the delay as output latency and the rest as input latency, and its blocks' host time includes the output latency. Hosts read `VIZDAW_AUDIO_LOOPBACK=<frames>` first, then `VIZDAW_AUDIO_INPUT=fake`; tests use the `set_fake_input` hook. The fake device names its channels "Fake In 1…3".
   - Cost if wrong: only the fake device.
8. **Ruling (injected and CoreMIDI time):** `MidiIn.host_ns == 0` means "now": the message is placed at the start of the block that takes it (`midi.inject`, whose caller's clock is not the offline device's). CoreMIDI messages keep their time stamps.
   - Cost if wrong: injected notes land up to one block late, only in tests and the e2e.
9. **Ruling (MIDI routes and thru):** a lane listens while it is armed or its monitor is Input and its input is MIDI: every message from its source (`port_key`, FNV-1a of the source name; none: every source) on its channel (none: every channel) goes through its `InputChain` (passes messages unchanged until plan 5g). The chain's output plays thru at once (block offset 0) on the lane's destination's targets and channel, unless the monitor is Off, and is recorded while a tap rolls and the lane is armed. Only channel messages are recorded and played thru (no SysEx in 5c). Recorded events carry no channel override (they play on the lane's destination, like step entry). Notes started by thru end when the routes change (a lane disarmed, re-routed or removed). The engine interns thru's targets at load like the schedule's, so thru reaches a destination the sequence never plays, and sends routes only when they changed, so a load still retires just a schedule and a graph.
   - Cost if wrong: SysEx recording waits for a later plan.
10. **Ruling (monitoring):** an audio lane listens while armed or monitored Input (and not Off); its channel or pair is mixed into its output path's `Regions` node (the document's first path when it has none) at unity, mono to both sides, as spec §6.6's modes say. The node already exists for every AudioPath strip. Playback of the lane's own regions is **not** muted while its input is heard (segments carry no lane).
    - Cost if wrong: during a punch the old take and the input are both heard; a later plan can give segments their lane.
11. **Ruling (record range and pre-roll):** with punch the range is the punch fields; with loop it is the transport loop (bar 1 to the end, what the engine loops), narrowed by the punch; otherwise from where recording starts (the heard position, or the session's cursor) until Stop. Pre-roll starts playback that many bars (of the meter at the start) earlier, never before the sequence origin; what it captures stays as region handles. `count_in_bars > 0` is refused ("count-in arrives with the record controls (plan 5d)"): 5d plays the click.
    - Cost if wrong: count-in waits for 5d, as the spec's 5d row lists it.
12. **Ruling (passes and takes):** a recording without a loop records into the active take (MIDI replace clears the recorded range first; audio regions are added, and the overlap rule makes audio overdub behave like replace, spec §6.4). With a loop, each kept pass of an armed audio lane becomes a new take "Take n" (n continues the lane's count) whatever `passes_as_takes` says; MIDI passes become takes with `passes_as_takes`, else merge into the active take. With takes, every armed lane gets one take per kept pass, even an empty one, so MIDI and audio takes stay matched.
    - Cost if wrong: an empty take the user deletes.
13. **Ruling (keep and discard):** `record.keep_pass` and `record.discard_pass` act on the passes completed before the press: keep keeps every undecided one (a later discard leaves it); discard drops every undecided one (those since the last keep). The pass playing is never touched; at Stop every pass not discarded is kept, the one playing included. They are refused (`Model`) unless loop recording.
    - Why: the spec's "plus the last pass if not discarded" read so that no audio is dropped silently.
    - Cost if wrong: an extra take.
14. **Ruling (time stamps in names):** the file name's date and time are UTC (no time-zone database in the cores).
    - Cost if wrong: names read hours off local time.
15. **Ruling (retention of recordings):** the retention keep set (5b Ruling 11) also holds the files of the recording being written, of a finished pass waiting for a drag to end, of a pass that failed to apply (`keep_recordings`), and of recordings a crash left that are offered and not yet answered. At close an untitled project's `Media/Recorded/` files go to the Trash (the scratch package is then removed as before).
    - Cost if wrong: a declined recording stays on disk until answered.
16. **Ruling (history groups):** a group is separate from a transaction: transactions (drags, `edit_all`) open inside it; undo inside it stops at its start; redo inside it reaches only steps undone inside it; `end_group` is refused while a transaction is open (Stop during a drag keeps the finished pass pending and applies it at `end_edit` or the next `idle`); a group that made no step adds nothing and keeps the redo stack. `Command::Record` is the group's last step; the step's label is the pass's ("Record", or "Record (with gaps)").
    - Cost if wrong: none.
17. **Ruling (what Record refuses and what ends it):** refused: a recording already running or finishing, an open edit, a sequence with no armed lane that has an input, an audio channel the device does not have, free space under 100 MiB (a warning under 1 GiB), a device at another rate than the project's (it would record at the wrong speed), a count-in, an empty or out-of-range punch, an empty record range. While recording, Locate, Play, Loop and Save As are `Busy` ("Stop recording first."); Stop (and `transport.record` again) finishes the pass. A pass ends by itself when the tap closes under it (the output stopped: a device change or loss, or another session took the engine), the writer fails, free space falls under 100 MiB (checked every 10 s), or the transport stops at the end; the reason goes to `idle`'s problems.
    - Cost if wrong: a refusal message.
18. **Ruling (channel names):** `record.set { replace?, punch?, loop?, count_in? (0–4), pre_roll? (0–8), passes_as_takes? }` sets the transport's Overdub (as `!replace`), Punch and Loop and the record options (refused while recording); `record.recover { action: import | trash }`; `lane.set_input { seq, lane, kind: none | audio | midi, channel? (1–256, default 1), stereo?, port?, midi_channel? (1–16) }` (channels counted from 1 on the channel, from 0 in the core); `lane.set_monitor { seq, lane, mode: auto | input | off }`; `midi.inject { port, bytes }`, `bytes` as hex pairs ("90 3c 64", at most 1024). All are key-only (no menu item; plan 5d adds the controls), groups "Record", "Lane" and "MIDI". `state.get record` reports recording, the spec, pass, kept, discarded, each armed lane (file, clipped samples), frames, overflows, gap frames, free space and recovered recordings; `state.get input` the device, its input channels, their names and both latencies. `state.get document`'s lanes gain `takes`, `active_take`, `input` and `monitor`. The registry goes from 119 to 124 commands.
    - Cost if wrong: aliases later.
19. **Ruling (HAL input):** the output device's own input runs in the same IOProc (duplex). Another input device is joined with the output device in a private aggregate (`private` 1, the output device first and as its main sub-device, drift compensation on the input, UID `org.vizdaw.input.<pid>`, name "VizDAW Input"), destroyed on close; its inputs list the output device's first, so the lane's channel *c* is aggregate input `offset + c`. The output's rate and buffer size are set on the output device before the aggregate is made. The input device's alive flag is listened to as well. A live host opens input only while the loaded sequence has an armed or Input-monitored audio lane, by reopening the device with the same renderer (`ensure_input`, at Play, after an edit and after a device rebuild); macOS refusing the microphone is a transport fault "VizDAW may not use audio input. Allow it in System Settings › Privacy & Security › Microphone, then arm the lane again." The input device is chosen with `set_audio_input_device(name)` (5d's Settings › Audio).
    - Cost if wrong: untested by machine (pure helpers are); the README steps check it.
20. **Ruling (the CoreMIDI record listener):** one process-wide `SourceListener` on every source except VizDAW's own virtual sources, connected when a live host is created and again at every Record with an armed MIDI lane (sources plugged in since); it feeds the newest host's engine. Test builds never connect it (`debug_midi_from_source` stands in).
    - Cost if wrong: a keyboard plugged in while playing is heard after the next Record.
21. **Ruling (crash recovery):** at open, regular `.caf` files directly in `Media/Recorded/` (not hidden) that carry the recording mark, read as a VizDAW CAF with at least one frame, and that no media item of the opened document names, are offered. Import puts each on a new lane named after the file at bar 1 of the first top-level sequence, as one undo step "Recover Recordings" (media origin `Recorded`); Move to Trash moves them; Later (or closing the alert) leaves them to be offered at the next open. The alert never shows under the control channel (`record.recover` answers). An untitled project's scratch package after a crash is not searched (5a m7 stays parked).
    - Cost if wrong: a crash in an untitled project loses its recording to the temporary folder.
22. **Ruling (recording in the app):** `RecordModel` builds the spec from the transport (replace = not Overdub; punch = the punch fields when Punch is on; loop = the transport Loop) and its own count-in, pre-roll and passes-as-takes (for 5d's popover and `record.set`). `transport.record` records the shown sequence or, while recording, stops; it is refused while step entry is on ("Step entry is on: turn it off to record.") or a drag is open. The document's poll reads `record_state` while playing or recording. Keep and discard are enabled only while loop recording. The recovery alert reads "Recovered Recordings" with Import into New Lanes, Move to Trash and Later.
    - Cost if wrong: none; the record controls are 5d's.
23. **Ruling (left to 5d):** the record button and popover, lane input pickers, monitor buttons, input meters and clip marks (5c reports clip counts in `state.get record`), the record-time readout, Settings › Audio and Recording, the region drawing as it grows (5c builds its overview when the pass lands, through 5b's worker), and count-in. Recording never needs them: Transport > Record, the channel and the README's channel steps drive it.
    - Cost if wrong: none.

## Carried and parked items

Picked up by this plan (each has an owning task and a test):
- **5b final review m1** (a Normalize note's event ID matched as a bare prefix, `ev1` taking `ev12`'s note, `session.rs` ~3366): Task 1, `normalize_warning` matches the ID whole; `a_normalize_note_names_the_region_its_id_matches_whole`.
- **5b final review m2 / 5b Task 6 N1** (two lost items of one hash each copy the found file into `Media/`): **fixed in Task 1**, because it is cheap: a search keeps one copy per found file and gives it to every item of that hash, and a relink job removes the files of refused relinks only after the whole batch, and only when no state names them (so one item's refusal never removes another's file); `search_relinks_every_item_of_a_found_hash` now checks one path and one file. N1's other cause (a copy forced while an unrelated job is pending) stays as 5b ruled: disk only.
- **Media lifetime contract** (5a final review recommendation 3; 5b Ruling 11): recordings follow it; Ruling 15 extends the keep set for files no history state names yet.
- **VIZDAW_TEST_WAIT_SECS** (5b `common::wait_limit`): every new `core-ffi` integration test that waits uses it.

Parked (not 5c's; listed so reviewers do not raise them):
- 5a final review r1 (one spurious underrun when an item restarts mid-playback; `stream.rs`): unchanged. 5c adds media only when a pass lands (new IDs, no restart) and does not touch `stream.rs`.
- 5a final review m5 (Save As copies media on the main thread) and m6 (`recheck_media` on activation): unchanged; recordings make m5 more common for untitled projects (Save As hard-links on one volume).
- 5a final review m7 (untitled scratch packages leak after a crash): unchanged; Ruling 21 says what it means for recordings.
- 5a final review m4 (`vizdaw-play` ignores the project rate): unchanged (Task 5 only adds `..AudioConfig::default()` to its config).
- 4b/4c "`handle_events` sleeps 300 ms under the HOST lock": already resolved by 4d (the rebuild drains without the HOST lock); the drain under the session lock (4d preflight F12) is unchanged. Task 11's input reopen runs under the HOST lock like every device open (5a m2).
- Synth tracking: plan 5c-2 (Ruling 1). Record UI, meters, count-in, Settings: plan 5d. MIDI input effects: plan 5g (the hook is Task 6's `InputChain`).

## File map

| Crate / folder | New | Changed |
|---|---|---|
| `core-model` | `command/record.rs`; tests `cmd_lane_input.rs`, `cmd_record.rs` | `lane.rs`, `command/lane.rs`, `command/mod.rs`, `history.rs`, `rules.rs` |
| `core-media` | `record.rs`; tests `record.rs` | `caf.rs` (`sync`, `clipped`), `lib.rs` |
| `core-engine` | `record.rs`, `input.rs`; tests `record_tap.rs`, `live_input.rs`, `rt_alloc_record.rs` | `backend.rs`, `render.rs`, `engine.rs`, `offline.rs`, `graph/regions.rs`, `lib.rs` |
| `core-ffi` | `passes.rs`, `record.rs`, `live.rs`; tests `passes.rs`, `record.rs`, `recovered.rs`, `live_input.rs` | `session.rs`, `host.rs`, `edit.rs`, `snapshot.rs`, `control.rs`, `relink.rs`, `jobs.rs`, `lib.rs`; tests `media_pool.rs`, `control.rs` |
| `platform-macos` | `permission.rs` | `hal.rs`, `midi.rs`, `lib.rs`, `Cargo.toml`; tests `midi.rs` |
| `core-control` | — | `registry.rs`, `request.rs`; tests `commands.rs` |
| `vizdaw-play`, `vizctl` | — | `src/live.rs` (one line); `tests/e2e.rs` |
| app | `VizDAWAppModel/RecordModel.swift`; tests `RecordModelTests.swift` | `DocumentModel.swift`, `TransportCommands.swift`, `ControlCommands.swift`, `VizDAW/DocumentView.swift`, `Package.swift`, `README.md`; tests `FakeSession.swift`, `TransportCommandsTests.swift` |
| scripts | — | `make-app.sh` (`NSMicrophoneUsageDescription`) |

---

### Task 1: Carry-overs — exact Normalize note IDs, one copy per found file

**Files:**
- Modify: `crates/core-ffi/src/session.rs` (`normalize_warning`, and a unit test in `mod tests`)
- Modify: `crates/core-ffi/src/relink.rs` (`search` keeps one copy per found file)
- Modify: `crates/core-ffi/src/jobs.rs` (`RelinkFile` derives `Clone`; `apply_relink` removes refused files after the batch)
- Test: `crates/core-ffi/tests/media_pool.rs`

**Interfaces:**
- Consumes: 5b's `Session::normalize_regions`, `relink::search`, `jobs::apply_relink`.
- Produces: `session::normalize_warning(w: &str, regions: &[RegionRow]) -> Option<String>` (private) matching the warning's event ID whole; `relink::search` returning the same `RelinkFile` for every lost item whose hash the found file has.

Two 5b final-review items (see "Carried and parked items"). The Normalize warning names its region's event ID followed by a colon; the old code took the first region whose ID the warning *started with*, so `ev1` claimed `ev12`'s note. A search that found one file for two lost items of one hash copied it twice; now the copy is made once and shared, and a relink job that refuses one item removes the refused item's made file only after the batch, when no item and no history state names it.

- [ ] **Step 1: Write the failing tests**

In `crates/core-ffi/tests/media_pool.rs`:

Find:
```rust
    for r in &rows {
        assert_eq!(row(&s, r.id).offline, None);
    }
    s.close().unwrap();
}
```
Replace with:
```rust
    for r in &rows {
        assert_eq!(row(&s, r.id).offline, None);
    }
    // 5b final review m2: one copy serves both items.
    let after = s.media().unwrap();
    assert_eq!(after[0].path, after[1].path, "{after:?}");
    let cafs = std::fs::read_dir(pkg.join("Media"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "caf"))
        .count();
    assert_eq!(cafs, 1, "one file in Media/");
    s.close().unwrap();
}
```

`search_relinks_every_item_of_a_found_hash` now also asserts that both items got the same new path and that `Media/` holds exactly one copy.

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test media_pool search_relinks_every_item_of_a_found_hash 2>&1 | tail -5
```
Expected: FAILED: the two items carry different paths (`… 2.wav`) and `Media/` holds two copies.

- [ ] **Step 3: Implement**

In `crates/core-ffi/src/session.rs`:

Find:
```rust
) -> (Vec<u64>, Vec<String>) {
    use core_model::command::MediaCmd;
    let mut done = Vec::new();
    let mut problems = Vec::new();
    for (m, r) in files {
        let f = match r {
            Ok(f) => f,
            Err(e) => {
```
Replace with:
```rust
) -> (Vec<u64>, Vec<String>) {
    use core_model::command::MediaCmd;
    let mut done = Vec::new();
    let mut problems = Vec::new();
    // Files this job wrote for relinks that were refused: removed only
    // after the whole batch, and only if no state names them, since a
    // search gives every item of one hash the same file (5b final review
    // m2).
    let mut refused: Vec<String> = Vec::new();
    for (m, r) in files {
        let f = match r {
            Ok(f) => f,
            Err(e) => {
```

Find:
```rust
        // Never the model's region-length refusal for a file without
        // audio (Task 1 carry); `relink_file` refuses one already.
        if f.file.length_frames == 0 {
            if f.made {
                remove_unkept(root, &f.file.path, &inner.kept());
            }
            problems.push(format!("{name}: the file holds no audio"));
            continue;
        }
```
Replace with:
```rust
        // Never the model's region-length refusal for a file without
        // audio (Task 1 carry); `relink_file` refuses one already.
        if f.file.length_frames == 0 {
            if f.made {
                refused.push(f.file.path.clone());
            }
            problems.push(format!("{name}: the file holds no audio"));
            continue;
        }
```

Find:
```rust
                done.push(m.0);
            }
            Err(e) => {
                if f.made {
                    remove_unkept(root, &f.file.path, &inner.kept());
                }
                let why = match e {
                    core_model::ModelError::OutOfRange {
                        field: "relink.length_frames",
```
Replace with:
```rust
                done.push(m.0);
            }
            Err(e) => {
                if f.made {
                    refused.push(f.file.path.clone());
                }
                let why = match e {
                    core_model::ModelError::OutOfRange {
                        field: "relink.length_frames",
```

Find:
```rust
                };
                problems.push(format!("{name}: {why}"));
            }
        }
    }
    (done, problems)
}
```
Replace with:
```rust
                };
                problems.push(format!("{name}: {why}"));
            }
        }
    }
    let keep = inner.kept();
    for p in refused {
        remove_unkept(root, &p, &keep);
    }
    (done, problems)
}
```

Find:
```rust
    from: u64,
    to: u64,
}

/// "Busy" for Normalize.
fn still_building(name: &str) -> VizError {
    VizError::Busy {
        message: format!("The waveform of {name} is still being built; try again in a moment."),
```
Replace with:
```rust
    from: u64,
    to: u64,
}

/// A Normalize note as the user reads it: the model names a region by its
/// event ID (`"ev12: gain limited …"`, `"ev4 is silent: …"`); the region's
/// media name replaces the ID only when the ID matches whole, never as a
/// prefix of a longer one (5b final review m1: `ev1` must not take
/// `ev12`'s note).
pub(crate) fn normalize_warning(w: &str, regions: &[(core_model::ids::EventId, &str)]) -> String {
    for (id, name) in regions {
        let id = id.to_string();
        if let Some(rest) = w.strip_prefix(id.as_str()) {
            if rest.starts_with(':') || rest.starts_with(' ') {
                return format!("Normalize: a region of {name}{rest}");
            }
        }
    }
    format!("Normalize: {w}")
}

/// "Busy" for Normalize.
fn still_building(name: &str) -> VizError {
    VizError::Busy {
        message: format!("The waveform of {name} is still being built; try again in a moment."),
```

Find:
```rust
            let report = inner.history.execute(&cmd, &ViewFilter::default())?;
            // The model's notes ("silent", "gain limited") name the event
            // ID: the user reads the media name in the next `idle`'s
            // problems (final review m3 / T5 m2).
            for w in report.warnings {
                let named = measure.iter().find_map(|x| {
                    w.strip_prefix(&x.id.to_string())
                        .map(|rest| format!("Normalize: a region of {}{rest}", x.name))
                });
                inner
                    .news
                    .problems
                    .push(named.unwrap_or_else(|| format!("Normalize: {w}")));
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
```
Replace with:
```rust
            let report = inner.history.execute(&cmd, &ViewFilter::default())?;
            // The model's notes ("silent", "gain limited") name the event
            // ID: the user reads the media name in the next `idle`'s
            // problems (final review m3 / T5 m2).
            let names: Vec<(core_model::ids::EventId, &str)> =
                measure.iter().map(|x| (x.id, x.name.as_str())).collect();
            for w in report.warnings {
                let line = normalize_warning(&w, &names);
                inner.news.problems.push(line);
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
```

Find:
```rust
                })
        })
    }
}
```
Replace with:
```rust
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_warning;
    use core_model::ids::EventId;

    /// 5b final review m1: a note about `ev12` is never named after
    /// `ev1`'s media, whichever comes first.
    #[test]
    fn a_normalize_note_names_the_region_its_id_matches_whole() {
        let regions = [(EventId(1), "kick"), (EventId(12), "snare")];
        assert_eq!(
            normalize_warning("ev12: gain limited to +24.0 dB", &regions),
            "Normalize: a region of snare: gain limited to +24.0 dB"
        );
        assert_eq!(
            normalize_warning("ev1 is silent: not normalized", &regions),
            "Normalize: a region of kick is silent: not normalized"
        );
        assert_eq!(
            normalize_warning("ev123: gain limited to +24.0 dB", &regions),
            "Normalize: ev123: gain limited to +24.0 dB"
        );
        assert_eq!(
            normalize_warning("nothing to normalize", &regions),
            "Normalize: nothing to normalize"
        );
    }
}
```

In `crates/core-ffi/src/relink.rs`:

Find:
```rust
    let n = found.iter().flatten().count().max(1) as f32;
    let mut done = 0f32;
    let mut out = Vec::with_capacity(lost.len());
    for (l, f) in lost.iter().zip(found) {
        let Some(f) = f else {
            out.push((
```
Replace with:
```rust
    let n = found.iter().flatten().count().max(1) as f32;
    let mut done = 0f32;
    let mut out = Vec::with_capacity(lost.len());
    // One file in `Media/` per found file (5b final review m2, Task 6
    // N1): items of one hash share the copy made for the first of them.
    let mut copies: BTreeMap<PathBuf, Result<RelinkFile, String>> = BTreeMap::new();
    for (l, f) in lost.iter().zip(found) {
        let Some(f) = f else {
            out.push((
```

Find:
```rust
            ));
            continue;
        };
        let base = done;
        let r = relink_file(
            importer,
```
Replace with:
```rust
            ));
            continue;
        };
        if let Some(r) = copies.get(&f) {
            out.push((l.media, r.clone().map_err(|e| format!("{}: {e}", l.name))));
            done += 1.0;
            continue;
        }
        let base = done;
        let r = relink_file(
            importer,
```

Find:
```rust
                crate::session::discard(root, Work::Relink { files: out }, &BTreeSet::new());
                return Work::Cancelled;
            }
            r => out.push((l.media, r.map_err(|e| format!("{}: {e}", l.name)))),
        }
    }
    Work::Relink { files: out }
```
Replace with:
```rust
                crate::session::discard(root, Work::Relink { files: out }, &BTreeSet::new());
                return Work::Cancelled;
            }
            r => {
                let r = r.map_err(|e| e.to_string());
                copies.insert(f, r.clone());
                out.push((l.media, r.map_err(|e| format!("{}: {e}", l.name))));
            }
        }
    }
    Work::Relink { files: out }
```

In `crates/core-ffi/src/jobs.rs`:

Find:
```rust
}

/// A file in `Media/` a media item is to be relinked to.
#[derive(Debug)]
pub struct RelinkFile {
    /// The file.
    pub file: Imported,
```
Replace with:
```rust
}

/// A file in `Media/` a media item is to be relinked to.
#[derive(Debug, Clone)]
pub struct RelinkFile {
    /// The file.
    pub file: Imported,
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test media_pool 2>&1 | tail -3
cargo test -p core-ffi --lib a_normalize_note_names_the_region_its_id_matches_whole 2>&1 | tail -3
```
Expected: both `test result: ok.`; the unit test `a_normalize_note_names_the_region_its_id_matches_whole` (in `session.rs`) passes.

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/session.rs crates/core-ffi/src/relink.rs crates/core-ffi/src/jobs.rs crates/core-ffi/tests/media_pool.rs
git commit -F - <<'VIZDAW_MSG'
core-ffi: exact Normalize note IDs; one copy per found file

The Normalize warning's region is the one whose event ID the note
names whole (ev1 no longer takes ev12's note; 5b final m1). A relink
search copies a found file once and gives it to every lost item of
its hash, and a relink job removes refused items' files only after
the batch and only when nothing names them (5b final m2, Task 6 N1).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 2: `core-model` — lane inputs and monitor modes

**Files:**
- Modify: `crates/core-model/src/lane.rs` (`InputChannels`, `LaneInput`, `MonitorMode`, `Lane.input`, `Lane.monitor`)
- Modify: `crates/core-model/src/command/lane.rs` (`LaneCmd::SetInput`, `LaneCmd::SetMonitor`)
- Modify: `crates/core-model/src/rules.rs` (a lane's input is checked when a file is read)
- Test: `crates/core-model/tests/cmd_lane_input.rs`

**Interfaces:**
- Produces:
  - `core_model::lane::InputChannels { Mono(u16), Stereo(u16) }` (device channels from 0; `Stereo(c)` is `c` and `c + 1`), `first()`, `count()`;
  - `LaneInput { Audio { channels: InputChannels }, Midi { port: Option<String>, channel: Option<u8> } }` with `check() -> Result<()>` (`MAX_INPUT_CHANNEL` 255 for the last channel used, `MAX_PORT_NAME` 255 bytes, MIDI channel 0–15);
  - `MonitorMode { Auto, Input, Off }` (`Auto` by default);
  - `Lane.input: Option<LaneInput>`, `Lane.monitor: MonitorMode`, both written only when set (Ruling 2);
  - `LaneCmd::SetInput { seq, lane, input: Option<LaneInput> }` ("Set Lane Input"), `LaneCmd::SetMonitor { seq, lane, mode }` ("Set Lane Monitor").

Spec §4.5. A lane's input says what it records and monitors; the monitor mode says when its input is heard (Task 6 routes both). The 5c-2 field `return_of` is not added yet (Ruling 1).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/cmd_lane_input.rs <<'VIZDAW_EOF'
//! Lane inputs and monitor modes (plan 5c Task 2; sub-project 5 spec §4.5,
//! §6.6): what a lane records and when its input is heard, as undoable
//! lane edits that old files read without and new files write only when
//! set.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::file::{from_json, to_json};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MAX_INPUT_CHANNEL, MonitorMode};
use core_model::selection::ViewFilter;
use serde_json::{Value, json};

fn doc() -> Document {
    apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc
}

fn set_input(input: Option<LaneInput>) -> Command {
    Command::Lane(LaneCmd::SetInput {
        seq: SeqId(0),
        lane: LaneId(0),
        input,
    })
}

fn lane(d: &Document) -> &core_model::lane::Lane {
    d.lane(SeqId(0), LaneId(0)).unwrap()
}

#[test]
fn input_and_monitor_are_undoable_lane_edits() {
    let mut h = History::new(doc());
    let f = ViewFilter::default();
    assert_eq!(lane(h.doc()).input, None);
    assert_eq!(lane(h.doc()).monitor, MonitorMode::Auto);
    let audio = LaneInput::Audio {
        channels: InputChannels::Stereo(2),
    };
    h.execute(&set_input(Some(audio.clone())), &f).unwrap();
    assert_eq!(h.undo_label(), Some("Set Lane Input"));
    h.execute(
        &Command::Lane(LaneCmd::SetMonitor {
            seq: SeqId(0),
            lane: LaneId(0),
            monitor: MonitorMode::Input,
        }),
        &f,
    )
    .unwrap();
    assert_eq!(h.undo_label(), Some("Set Lane Monitor"));
    assert_eq!(lane(h.doc()).input, Some(audio));
    assert_eq!(lane(h.doc()).monitor, MonitorMode::Input);
    h.undo().unwrap();
    assert_eq!(lane(h.doc()).monitor, MonitorMode::Auto);
    h.undo().unwrap();
    assert_eq!(lane(h.doc()).input, None);
    let midi = LaneInput::Midi {
        port: Some("Keys".into()),
        channel: Some(15),
    };
    h.execute(&set_input(Some(midi.clone())), &f).unwrap();
    assert_eq!(lane(h.doc()).input, Some(midi));
    h.execute(&set_input(None), &f).unwrap();
    assert_eq!(lane(h.doc()).input, None);
}

#[test]
fn hostile_inputs_are_refused_and_change_nothing() {
    let d = doc();
    let long = "x".repeat(256);
    for bad in [
        LaneInput::Audio {
            channels: InputChannels::Mono(MAX_INPUT_CHANNEL + 1),
        },
        LaneInput::Audio {
            channels: InputChannels::Stereo(MAX_INPUT_CHANNEL),
        },
        LaneInput::Audio {
            channels: InputChannels::Stereo(u16::MAX),
        },
        LaneInput::Midi {
            port: Some(String::new()),
            channel: None,
        },
        LaneInput::Midi {
            port: Some(long),
            channel: None,
        },
        LaneInput::Midi {
            port: Some("a\nb".into()),
            channel: None,
        },
        LaneInput::Midi {
            port: None,
            channel: Some(16),
        },
    ] {
        match apply(&d, &set_input(Some(bad.clone()))) {
            Err(ModelError::OutOfRange { .. }) => {}
            other => panic!("{bad:?}: {other:?}"),
        }
    }
    assert!(
        apply(
            &d,
            &Command::Lane(LaneCmd::SetMonitor {
                seq: SeqId(0),
                lane: LaneId(9),
                monitor: MonitorMode::Off,
            })
        )
        .is_err()
    );
    // The highest pair and channel are fine.
    let ok = LaneInput::Audio {
        channels: InputChannels::Stereo(MAX_INPUT_CHANNEL - 1),
    };
    assert!(apply(&d, &set_input(Some(ok))).is_ok());
}

fn lane_json(text: &str) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    v["document"]["sequences"][0]["lanes"][0].clone()
}

#[test]
fn files_write_inputs_only_when_set_and_old_lanes_read_without_them() {
    let d = doc();
    let text = to_json(&d).unwrap();
    let l = lane_json(&text);
    assert!(l.get("input").is_none(), "{l}");
    assert!(l.get("monitor").is_none(), "{l}");
    // A lane written before 5c (no keys) reads as no input, Auto.
    assert_eq!(from_json(&text).unwrap(), d);
    let d2 = apply(
        &d,
        &set_input(Some(LaneInput::Midi {
            port: None,
            channel: Some(3),
        })),
    )
    .unwrap()
    .doc;
    let d2 = apply(
        &d2,
        &Command::Lane(LaneCmd::SetMonitor {
            seq: SeqId(0),
            lane: LaneId(0),
            monitor: MonitorMode::Off,
        }),
    )
    .unwrap()
    .doc;
    let text = to_json(&d2).unwrap();
    let l = lane_json(&text);
    assert_eq!(l["input"], json!({"midi": {"port": null, "channel": 3}}));
    assert_eq!(l["monitor"], json!("off"));
    assert_eq!(from_json(&text).unwrap(), d2);
}

#[test]
fn a_damaged_input_in_a_file_is_refused() {
    let text = to_json(&doc()).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["document"]["sequences"][0]["lanes"][0]["input"] =
        json!({"audio": {"channels": {"mono": 4000}}});
    let bad = serde_json::to_string(&v).unwrap();
    match from_json(&bad) {
        Err(ModelError::Corrupt { problems }) => {
            assert!(
                problems.iter().any(|p| p.contains("input.channels")),
                "{problems:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    v["document"]["sequences"][0]["lanes"][0]["input"] =
        json!({"audio": {"channels": {"quad": 0}}});
    assert!(from_json(&serde_json::to_string(&v).unwrap()).is_err());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_lane_input 2>&1 | tail -5
```
Expected: does not compile: `LaneInput`, `InputChannels`, `MonitorMode` and `LaneCmd::SetInput` are not defined.

- [ ] **Step 3: Implement**

In `crates/core-model/src/lane.rs`:

Find:
```rust
    pub takes: Vector<Take>,
    /// The take that plays.
    pub active_take: TakeId,
}

impl Lane {
```
Replace with:
```rust
    pub takes: Vector<Take>,
    /// The take that plays.
    pub active_take: TakeId,
    /// What the lane records (sub-project 5 spec §4.5): audio from device
    /// input channels or MIDI from a port. `None` records nothing; absent
    /// in files, so lanes without one read and write as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<LaneInput>,
    /// When the lane's input is heard (spec §6.6); `Auto` is absent in
    /// files.
    #[serde(default, skip_serializing_if = "MonitorMode::is_auto")]
    pub monitor: MonitorMode,
}

impl Lane {
```

Find:
```rust
            drum: None,
            takes: Vector::unit(Take::new(take, "Take 1")),
            active_take: take,
        }
    }
```
Replace with:
```rust
            drum: None,
            takes: Vector::unit(Take::new(take, "Take 1")),
            active_take: take,
            input: None,
            monitor: MonitorMode::Auto,
        }
    }
```

Find:
```rust
    /// Index of a take by ID.
    pub fn take_index(&self, id: TakeId) -> Option<usize> {
        self.takes.iter().position(|t| t.id == id)
    }
}
```
Replace with:
```rust
    /// Index of a take by ID.
    pub fn take_index(&self, id: TakeId) -> Option<usize> {
        self.takes.iter().position(|t| t.id == id)
    }
}

/// The highest device input channel a lane may name (0-based): a stereo
/// pair's second channel is at most this.
pub const MAX_INPUT_CHANNEL: u16 = 255;
/// The longest MIDI port name a lane input may name, in bytes.
pub const MAX_PORT_NAME: usize = 255;

/// The device input channels an audio lane records (spec §4.5), 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputChannels {
    /// One channel.
    Mono(u16),
    /// This channel and the next, as left and right.
    Stereo(u16),
}

impl InputChannels {
    /// 1 or 2.
    pub fn count(self) -> u16 {
        match self {
            InputChannels::Mono(_) => 1,
            InputChannels::Stereo(_) => 2,
        }
    }

    /// The first (left) channel.
    pub fn first(self) -> u16 {
        match self {
            InputChannels::Mono(c) | InputChannels::Stereo(c) => c,
        }
    }

    /// The last channel (the right one of a pair), or `None` past
    /// `u16::MAX`.
    pub fn last(self) -> Option<u16> {
        self.first().checked_add(self.count() - 1)
    }
}

/// What a lane records (spec §4.5). A lane records audio only with an
/// `Audio` input and MIDI only with a `Midi` one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LaneInput {
    /// Audio from device input channels.
    Audio {
        /// Which channels.
        channels: InputChannels,
    },
    /// MIDI from a port.
    Midi {
        /// The source's name; `None`: every source.
        port: Option<String>,
        /// The MIDI channel 0–15; `None`: every channel.
        channel: Option<u8>,
    },
}

impl LaneInput {
    /// Checks the channels (at most [`MAX_INPUT_CHANNEL`]), the port name
    /// (not empty, at most [`MAX_PORT_NAME`] bytes, no control characters)
    /// and the MIDI channel (0–15).
    pub fn check(&self) -> crate::error::Result<()> {
        match self {
            LaneInput::Audio { channels } => {
                if channels.last().is_none_or(|c| c > MAX_INPUT_CHANNEL) {
                    return Err(ModelError::range("input.channels", channels.first()));
                }
            }
            LaneInput::Midi { port, channel } => {
                if let Some(p) = port {
                    if p.is_empty() || p.len() > MAX_PORT_NAME || p.chars().any(char::is_control) {
                        return Err(ModelError::range("input.port", p.len()));
                    }
                }
                if channel.is_some_and(|c| c > 15) {
                    return Err(ModelError::range("input.channel", channel.unwrap_or(0)));
                }
            }
        }
        Ok(())
    }

    /// True for an audio input.
    pub fn is_audio(&self) -> bool {
        matches!(self, LaneInput::Audio { .. })
    }
}

/// When a lane's input is heard (spec §6.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorMode {
    /// While the lane is armed and the transport is stopped, or while it
    /// records inside the record range (auto-input).
    #[default]
    Auto,
    /// Always.
    Input,
    /// Never.
    Off,
}

impl MonitorMode {
    /// True for [`MonitorMode::Auto`] (the default, absent in files).
    pub fn is_auto(&self) -> bool {
        *self == MonitorMode::Auto
    }
}
```

In `crates/core-model/src/command/lane.rs`:

Find:
```rust
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId, TakeId};
use crate::lane::{Lane, QuantizeSpec, Take};
use crate::rules::check_positive;
use crate::time::Tick;
```
Replace with:
```rust
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId, TakeId};
use crate::lane::{Lane, LaneInput, MonitorMode, QuantizeSpec, Take};
use crate::rules::check_positive;
use crate::time::Tick;
```

Find:
```rust
        /// Take (must exist in the lane).
        take: TakeId,
    },
}

impl LaneCmd {
```
Replace with:
```rust
        /// Take (must exist in the lane).
        take: TakeId,
    },
    /// Set what the lane records (sub-project 5 spec §4.5).
    SetInput {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// The input (checked by [`LaneInput::check`]) or none.
        input: Option<LaneInput>,
    },
    /// Set when the lane's input is heard (spec §6.6).
    SetMonitor {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// The mode.
        monitor: MonitorMode,
    },
}

impl LaneCmd {
```

Find:
```rust
            LaneCmd::SetPlayShift { .. } => "Set Play Shift",
            LaneCmd::AddTake { .. } => "Add Take",
            LaneCmd::SetActiveTake { .. } => "Set Active Take",
        }
    }
}
```
Replace with:
```rust
            LaneCmd::SetPlayShift { .. } => "Set Play Shift",
            LaneCmd::AddTake { .. } => "Add Take",
            LaneCmd::SetActiveTake { .. } => "Set Active Take",
            LaneCmd::SetInput { .. } => "Set Lane Input",
            LaneCmd::SetMonitor { .. } => "Set Lane Monitor",
        }
    }
}
```

Find:
```rust
            }
            l.active_take = *take;
        }
    }
    Ok(())
}
```
Replace with:
```rust
            }
            l.active_take = *take;
        }
        LaneCmd::SetInput { seq, lane, input } => {
            if let Some(i) = input {
                i.check()?;
            }
            d.lane_mut(*seq, *lane)?.input = input.clone();
        }
        LaneCmd::SetMonitor { seq, lane, monitor } => {
            d.lane_mut(*seq, *lane)?.monitor = *monitor;
        }
    }
    Ok(())
}
```

In `crates/core-model/src/rules.rs`:

Find:
```rust
            if let Some(q) = &l.play_quantize {
                push(q.check(), &lctx);
            }
            if l.take(l.active_take).is_none() {
                push(
                    Err(ModelError::UnknownId(AnyId::Take(l.active_take))),
```
Replace with:
```rust
            if let Some(q) = &l.play_quantize {
                push(q.check(), &lctx);
            }
            if let Some(i) = &l.input {
                push(i.check(), &lctx);
            }
            if l.take(l.active_take).is_none() {
                push(
                    Err(ModelError::UnknownId(AnyId::Take(l.active_take))),
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_lane_input 2>&1 | tail -3
```
Expected: `test result: ok. 4 passed` (`input_and_monitor_are_undoable_lane_edits`, `hostile_inputs_are_refused_and_change_nothing`, `files_write_inputs_only_when_set_and_old_lanes_read_without_them`, `a_damaged_input_in_a_file_is_refused`).

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p core-model 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
```
Expected: no warnings, every `test result: ok`; Every `Lane { … }` literal outside `core-model` is built with `..Default::default()` or `Lane::new`, so `core-ffi` compiles unchanged; run its tests to be sure..

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/lane.rs crates/core-model/src/command/lane.rs crates/core-model/src/rules.rs crates/core-model/tests/cmd_lane_input.rs
git commit -F - <<'VIZDAW_MSG'
core-model: lane inputs and monitor modes

Lane.input (audio channels, mono or a stereo pair, or a MIDI port and
channel) and Lane.monitor (Auto, Input, Off) are undoable lane edits,
checked when set and when a file is read, and written only when set,
so projects without inputs read and write exactly as before.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 3: `core-model` — the recording pass and history groups

**Files:**
- Create: `crates/core-model/src/command/record.rs`
- Modify: `crates/core-model/src/command/mod.rs` (`Command::Record`)
- Modify: `crates/core-model/src/history.rs` (groups)
- Test: `crates/core-model/tests/cmd_record.rs`

**Interfaces:**
- Consumes: Task 2's lanes; 5a's `MediaItem`, `AudioRegion`, `MediaOrigin::Recorded`; 4e's takes.
- Produces:
  - `command::record::{RecordedMedia, RecordedRegion, RecordedContent, LaneRecord, RecordPass}`; `RecordPass { with_gaps, media, lanes }` and `name()` ("Record" / "Record (with gaps)"); `MAX_PASS_TAKES` 256;
  - `Command::Record(RecordPass)`: adds the media, then per lane clears the replace range's MIDI (`clear`), adds content to the active take or makes "Take n" takes (`takes`), and selects the last (`active`); any bad part refuses the whole pass;
  - `History::begin_group()`, `end_group(label) -> Result<()>`, `in_group()` (Ruling 16).

Spec §4.7. One pass applies whole or not at all; the group makes the pass and every edit made while recording one undo step.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/cmd_record.rs <<'VIZDAW_EOF'
//! Recording passes and history groups (plan 5c Task 3; sub-project 5
//! spec §4.7, §6.4, §6.5): `Command::Record` applies a whole pass or
//! nothing, replace clears only MIDI in the recorded range, takes are
//! added with the last made active, and a group turns everything made
//! while recording into one "Record" step.

use core_model::ModelError;
use core_model::command::{
    Command, EventCmd, LaneCmd, LaneRecord, RecordPass, RecordedContent, RecordedMedia,
    RecordedRegion, SequenceCmd, apply,
};
use core_model::document::{Document, MediaOrigin};
use core_model::event::{EventBody, EventSpec};
use core_model::history::History;
use core_model::ids::{LaneId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

const Q: i64 = PPQ;

/// Sequence 0 with lanes 0 and 1; lane 0 holds notes at beats 0, 2 and 4
/// (pitches 60, 62, 64) and a CC at beat 2.
fn doc() -> Document {
    let mut d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    d = apply(
        &d,
        &Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        }),
    )
    .unwrap()
    .doc;
    apply(
        &d,
        &Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![
                EventSpec::new(Tick(0), EventBody::note(60, 30_000, Tick(Q))),
                EventSpec::new(Tick(2 * Q), EventBody::note(62, 30_000, Tick(Q))),
                EventSpec::new(Tick(2 * Q), EventBody::Cc { index: 1, value: 5 }),
                EventSpec::new(Tick(4 * Q), EventBody::note(64, 30_000, Tick(Q))),
            ],
        }),
    )
    .unwrap()
    .doc
}

fn media(frames: u64) -> RecordedMedia {
    RecordedMedia {
        path: "Media/Recorded/B 2026-10-20 120000.caf".into(),
        sample_rate: 48_000,
        channels: 1,
        length_frames: frames,
        content_hash: "ab".repeat(32),
        name: "B 2026-10-20 120000".into(),
        pass: 1,
    }
}

fn lane_record(lane: u64) -> LaneRecord {
    LaneRecord {
        seq: SeqId(0),
        lane: LaneId(lane),
        clear: Vec::new(),
        active: RecordedContent::default(),
        takes: Vec::new(),
    }
}

fn pitches(d: &Document, lane: u64) -> Vec<(i64, u8)> {
    d.lane(SeqId(0), LaneId(lane))
        .unwrap()
        .active()
        .unwrap()
        .events
        .values()
        .filter_map(|e| match e.body {
            EventBody::Note { pitch, .. } => Some((e.tick.0 / Q, pitch)),
            _ => None,
        })
        .collect()
}

fn note(beat: i64, pitch: u8) -> EventSpec {
    EventSpec::new(Tick(beat * Q), EventBody::note(pitch, 40_000, Tick(Q / 2)))
}

#[test]
fn replace_clears_only_midi_starting_in_the_range() {
    let mut l = lane_record(0);
    l.clear = vec![(Tick(Q), Tick(3 * Q))];
    l.active.events = vec![note(2, 70)];
    let pass = RecordPass {
        lanes: vec![l],
        ..RecordPass::default()
    };
    let d = apply(&doc(), &Command::Record(pass)).unwrap().doc;
    // 62 at beat 2 went, the CC at beat 2 went, 60 and 64 stay.
    assert_eq!(pitches(&d, 0), vec![(0, 60), (2, 70), (4, 64)]);
    let take = d.lane(SeqId(0), LaneId(0)).unwrap().active().unwrap();
    assert!(
        !take
            .events
            .values()
            .any(|e| matches!(e.body, EventBody::Cc { .. }))
    );
}

#[test]
fn overdub_merges_with_what_is_there() {
    let mut l = lane_record(0);
    l.active.events = vec![note(2, 70), note(3, 71)];
    let pass = RecordPass {
        lanes: vec![l],
        ..RecordPass::default()
    };
    let d = apply(&doc(), &Command::Record(pass)).unwrap().doc;
    assert_eq!(
        pitches(&d, 0),
        vec![(0, 60), (2, 62), (2, 70), (3, 71), (4, 64)]
    );
}

#[test]
fn a_pass_adds_its_media_regions_and_takes_and_selects_the_last() {
    let mut l = lane_record(1);
    let region = |offset: u64| RecordedRegion {
        media: 0,
        tick: Tick(0),
        source_offset: offset,
        length: 48_000,
    };
    l.takes = vec![
        (
            "Take 2".into(),
            RecordedContent {
                events: vec![],
                regions: vec![region(1000)],
            },
        ),
        (
            "Take 3".into(),
            RecordedContent {
                events: vec![],
                regions: vec![region(49_000)],
            },
        ),
    ];
    let pass = RecordPass {
        media: vec![media(100_000)],
        lanes: vec![l],
        ..RecordPass::default()
    };
    let a = apply(&doc(), &Command::Record(pass.clone())).unwrap();
    let d = a.doc;
    let item = d.media().values().next().unwrap();
    assert_eq!(item.origin, MediaOrigin::Recorded { pass: 1 });
    assert_eq!(item.content_hash.as_deref(), Some("ab".repeat(32).as_str()));
    let lane = d.lane(SeqId(0), LaneId(1)).unwrap();
    let names: Vec<&str> = lane.takes.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["Take 1", "Take 2", "Take 3"]);
    assert_eq!(lane.active().unwrap().name, "Take 3");
    let offsets: Vec<u64> = lane
        .takes
        .iter()
        .flat_map(|t| t.events.values())
        .filter_map(|e| match e.body {
            EventBody::AudioRegion {
                media,
                source_offset,
                ..
            } if media == item.id => Some(source_offset),
            _ => None,
        })
        .collect();
    assert_eq!(offsets, vec![1000, 49_000]);
    // The lane had no output: the region gave it the first one ("Audio 1").
    assert!(lane.audio_out.is_some());
    assert_eq!(Command::Record(pass).name(), "Record");
    let gaps = RecordPass {
        with_gaps: true,
        ..RecordPass::default()
    };
    assert_eq!(Command::Record(gaps).name(), "Record (with gaps)");
}

#[test]
fn a_bad_part_refuses_the_whole_pass() {
    let d = doc();
    let mut ok = lane_record(0);
    ok.active.events = vec![note(1, 72)];
    let bad_region = RecordedRegion {
        media: 0,
        tick: Tick(0),
        source_offset: 90_000,
        length: 20_000,
    };
    let mut bad = lane_record(1);
    bad.active.regions = vec![bad_region];
    for pass in [
        // A region past its file's end.
        RecordPass {
            media: vec![media(100_000)],
            lanes: vec![ok.clone(), bad.clone()],
            ..RecordPass::default()
        },
        // A region naming a file the pass does not have.
        RecordPass {
            media: vec![],
            lanes: vec![ok.clone(), bad.clone()],
            ..RecordPass::default()
        },
        // An empty file.
        RecordPass {
            media: vec![media(0)],
            lanes: vec![ok.clone()],
            ..RecordPass::default()
        },
        // A path outside the package.
        RecordPass {
            media: vec![RecordedMedia {
                path: "../x.caf".into(),
                ..media(10)
            }],
            lanes: vec![ok.clone()],
            ..RecordPass::default()
        },
        // An empty clear range, and an unknown lane.
        RecordPass {
            lanes: vec![LaneRecord {
                clear: vec![(Tick(Q), Tick(Q))],
                ..ok.clone()
            }],
            ..RecordPass::default()
        },
        RecordPass {
            lanes: vec![ok.clone(), lane_record(9)],
            ..RecordPass::default()
        },
    ] {
        let mut h = History::new(d.clone());
        assert!(
            h.execute(&Command::Record(pass.clone()), &ViewFilter::default())
                .is_err(),
            "{pass:?}"
        );
        assert_eq!(h.doc(), &d, "nothing applied");
        assert_eq!(h.undo_depth(), 0);
    }
}

#[test]
fn a_group_makes_one_record_step_of_everything_inside_it() {
    let f = ViewFilter::default();
    let d = doc();
    let mut h = History::new(d.clone());
    h.execute(
        &Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(1),
            name: "Before".into(),
        }),
        &f,
    )
    .unwrap();
    h.undo().unwrap();
    // A step undone before recording is not redone into the pass.
    h.begin_group().unwrap();
    assert!(h.in_group());
    assert!(h.redo().is_err());
    assert!(h.peek_redo().is_none());
    assert!(h.begin_group().is_err(), "one group at a time");
    let rename = |name: &str| {
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: name.into(),
        })
    };
    h.execute(&rename("X"), &f).unwrap();
    h.execute(&rename("Y"), &f).unwrap();
    // Steps inside stay undoable and redoable one by one.
    h.undo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().name, "X");
    h.redo().unwrap();
    // A drag (transaction) inside the group is fine; the group cannot end
    // while it is open.
    h.begin("Drag").unwrap();
    h.execute(&rename("Z"), &f).unwrap();
    assert!(h.end_group("Record").is_err());
    h.commit().unwrap();
    // Undo never reaches below the group's start.
    let mut l = lane_record(0);
    l.active.events = vec![note(6, 80)];
    h.execute(
        &Command::Record(RecordPass {
            lanes: vec![l],
            ..RecordPass::default()
        }),
        &f,
    )
    .unwrap();
    h.end_group("Record").unwrap();
    assert!(!h.in_group());
    assert_eq!(h.undo_label(), Some("Record"));
    assert_eq!(h.undo_depth(), 1);
    assert_eq!(
        pitches(h.doc(), 0),
        vec![(0, 60), (2, 62), (4, 64), (6, 80)]
    );
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
    h.redo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().name, "Z");
    assert!(
        h.redo().is_err(),
        "the rename undone before the pass is gone"
    );
}

#[test]
fn a_group_that_made_nothing_keeps_the_redo_stack() {
    let f = ViewFilter::default();
    let mut h = History::new(doc());
    h.execute(
        &Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "X".into(),
        }),
        &f,
    )
    .unwrap();
    h.undo().unwrap();
    h.begin_group().unwrap();
    assert!(h.undo().is_err(), "nothing of its own to undo");
    h.end_group("Record").unwrap();
    assert_eq!(h.undo_depth(), 0);
    h.redo().unwrap();
    assert_eq!(h.doc().lane(SeqId(0), LaneId(0)).unwrap().name, "X");
    // A group cannot open inside a transaction, nor end without opening.
    h.begin("Drag").unwrap();
    assert!(h.begin_group().is_err());
    h.abort().unwrap();
    assert!(matches!(
        h.end_group("Record"),
        Err(ModelError::Invalid { .. })
    ));
}

#[derive(Debug, Clone)]
enum Part {
    Clear(i64, i64),
    Note(i64, u8),
    Region(u64, u64, i64),
    Take(Vec<(i64, u8)>, Vec<(u64, u64)>),
}

fn part() -> impl Strategy<Value = Part> {
    prop_oneof![
        (0i64..8, 1i64..8).prop_map(|(a, n)| Part::Clear(a, a + n)),
        (0i64..8, 0u8..128).prop_map(|(b, p)| Part::Note(b, p)),
        (0u64..80_000, 1u64..20_000, 0i64..8).prop_map(|(o, n, b)| Part::Region(o, n, b)),
        (
            proptest::collection::vec((0i64..8, 0u8..128), 0..4),
            proptest::collection::vec((0u64..90_000, 1u64..10_000), 0..3)
        )
            .prop_map(|(n, r)| Part::Take(n, r)),
    ]
}

fn pass_of(parts: &[(u64, Part)]) -> RecordPass {
    let mut lanes = vec![lane_record(0), lane_record(1)];
    for (lane, p) in parts {
        let Some(l) = lanes.get_mut(*lane as usize % 2) else {
            continue;
        };
        match p {
            Part::Clear(a, b) => l.clear.push((Tick(a * Q), Tick(b * Q))),
            Part::Note(b, p) => l.active.events.push(note(*b, *p)),
            Part::Region(o, n, b) => l.active.regions.push(RecordedRegion {
                media: 0,
                tick: Tick(b * Q),
                source_offset: *o,
                length: *n,
            }),
            Part::Take(notes, regions) => l.takes.push((
                format!("Take {}", l.takes.len() + 2),
                RecordedContent {
                    events: notes.iter().map(|(b, p)| note(*b, *p)).collect(),
                    regions: regions
                        .iter()
                        .map(|(o, n)| RecordedRegion {
                            media: 0,
                            tick: Tick(0),
                            source_offset: *o,
                            length: *n,
                        })
                        .collect(),
                },
            )),
        }
    }
    RecordPass {
        with_gaps: false,
        media: vec![media(100_000)],
        lanes,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Spec §12.3: applying a pass and undoing it restores the document
    /// exactly; redo brings the same pass back.
    #[test]
    fn record_then_undo_restores_the_document(parts in proptest::collection::vec((0u64..2, part()), 0..12)) {
        let d = doc();
        let mut h = History::new(d.clone());
        h.begin_group().unwrap();
        h.execute(&Command::Record(pass_of(&parts)), &ViewFilter::default()).unwrap();
        h.end_group("Record").unwrap();
        let after = h.doc().clone();
        if after != d {
            prop_assert_eq!(h.undo_depth(), 1);
            h.undo().unwrap();
        }
        prop_assert_eq!(h.doc(), &d);
        if after != d {
            h.redo().unwrap();
        }
        prop_assert_eq!(h.doc(), &after);
        prop_assert!(core_model::rules::validate(&after).is_empty());
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_record 2>&1 | tail -5
```
Expected: does not compile: `core_model::command::record` and `History::begin_group` do not exist.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/src/command/record.rs <<'VIZDAW_EOF'
//! Recording passes (sub-project 5 spec §4.7, §6.4, §6.5): everything one
//! Record pass adds, applied atomically as `Command::Record` at Stop. The
//! pass is built outside the model (plan 5c Task 7); here it is checked
//! and applied in one step, so it applies whole or not at all.

use super::Report;
use crate::document::{Document, MediaItem, MediaOrigin};
use crate::error::{ModelError, Result};
use crate::event::{EventBody, EventSpec, Fade};
use crate::ids::{AnyId, LaneId, MediaId, SeqId, TakeId};
use crate::lane::Take;
use crate::rules::{check_event, check_media_path, check_ticks};
use crate::time::Tick;

/// A file the pass recorded, added to the media pool (origin
/// `Recorded { pass }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedMedia {
    /// Package-relative path (`Media/Recorded/…`).
    pub path: String,
    /// Sample rate (> 0).
    pub sample_rate: u32,
    /// Channels (1 or 2).
    pub channels: u16,
    /// Length in frames (> 0).
    pub length_frames: u64,
    /// BLAKE3 hex of the sample bytes.
    pub content_hash: String,
    /// Display name.
    pub name: String,
    /// The pass number.
    pub pass: u32,
}

/// A region over a slice of one of the pass's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedRegion {
    /// Index into [`RecordPass::media`].
    pub media: usize,
    /// Start tick.
    pub tick: Tick,
    /// First frame of the file it plays.
    pub source_offset: u64,
    /// Frames (≥ 1, within the file).
    pub length: u64,
}

/// What a pass puts into one take.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordedContent {
    /// MIDI events.
    pub events: Vec<EventSpec>,
    /// Audio regions.
    pub regions: Vec<RecordedRegion>,
}

impl RecordedContent {
    /// Nothing recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.regions.is_empty()
    }
}

/// One lane's part of a pass.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneRecord {
    /// Sequence.
    pub seq: SeqId,
    /// Lane.
    pub lane: LaneId,
    /// Replace mode: the active take's MIDI events (not regions,
    /// references, text or automation) that start in one of these
    /// `[start, end)` ranges are removed first (spec §6.4).
    pub clear: Vec<(Tick, Tick)>,
    /// Into the active take.
    pub active: RecordedContent,
    /// New takes, in order, each with its name; the last becomes active.
    pub takes: Vec<(String, RecordedContent)>,
}

/// A finished recording pass (spec §4.7).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordPass {
    /// The input ring overflowed or blocks were lost: the lost frames are
    /// silence, and the undo label says so (spec §6.2).
    pub with_gaps: bool,
    /// The files recorded.
    pub media: Vec<RecordedMedia>,
    /// Each lane's part.
    pub lanes: Vec<LaneRecord>,
}

impl RecordPass {
    /// The undo label: "Record", or "Record (with gaps)".
    pub fn name(&self) -> &'static str {
        if self.with_gaps {
            "Record (with gaps)"
        } else {
            "Record"
        }
    }
}

/// The most takes a pass may add to one lane.
pub const MAX_PASS_TAKES: usize = 256;

fn check_media(m: &RecordedMedia) -> Result<()> {
    check_media_path(&m.path)?;
    if m.sample_rate == 0 || !(1..=2).contains(&m.channels) || m.length_frames == 0 {
        return Err(ModelError::range("record.media", &m.path));
    }
    if m.content_hash.is_empty() || m.name.is_empty() {
        return Err(ModelError::invalid(format!(
            "recorded file {} has no hash or name",
            m.path
        )));
    }
    Ok(())
}

/// The pass's files with the media IDs the pool gave them.
struct Files<'a> {
    pass: &'a RecordPass,
    ids: &'a [MediaId],
}

/// The region event of `r`, checked against the file it slices.
fn region_spec(r: &RecordedRegion, files: &Files<'_>) -> Result<EventSpec> {
    let (Some(m), Some(id)) = (files.pass.media.get(r.media), files.ids.get(r.media)) else {
        return Err(ModelError::range("record.region.media", r.media));
    };
    let end = r.source_offset.checked_add(r.length);
    if r.length == 0 || end.is_none_or(|e| e > m.length_frames) {
        return Err(ModelError::range("record.region.length", r.length));
    }
    Ok(EventSpec::new(
        r.tick,
        EventBody::AudioRegion {
            media: *id,
            source_offset: r.source_offset,
            length: r.length,
            gain_db: 0.0,
            fade_in: Fade::default(),
            fade_out: Fade::default(),
        },
    ))
}

/// Inserts `content` into take `take` (the active one when `None`).
fn insert_content(
    d: &mut Document,
    seq: SeqId,
    lane: LaneId,
    take: Option<TakeId>,
    content: &RecordedContent,
    files: &Files<'_>,
    r: &mut Report,
) -> Result<()> {
    let mut specs: Vec<EventSpec> = content.events.clone();
    for reg in &content.regions {
        specs.push(region_spec(reg, files)?);
    }
    if !content.regions.is_empty() && d.lane(seq, lane)?.audio_out.is_none() {
        // A region plays on its lane's output path: a lane without one
        // takes the first, or a new "Audio 1" (5a Ruling 6).
        let out = match d.outputs().keys().next().copied() {
            Some(o) => o,
            None => super::routing::add_output(d, "Audio 1", "audio:1-2", r),
        };
        d.lane_mut(seq, lane)?.audio_out = Some(out);
    }
    for spec in specs {
        check_event(d, seq, &spec)?;
        let id = d.counters_mut().alloc_event();
        d.insert_event(seq, lane, take, spec.with_id(id))?;
        r.created.push(AnyId::Event(id));
        super::event::ensure_drum_row(d, id, r)?;
    }
    Ok(())
}

pub(crate) fn apply(d: &mut Document, pass: &RecordPass, r: &mut Report) -> Result<()> {
    let mut ids = Vec::with_capacity(pass.media.len());
    for m in &pass.media {
        check_media(m)?;
        let id = d.counters_mut().alloc_media();
        d.media_mut().insert(
            id,
            MediaItem {
                id,
                path: m.path.clone(),
                sample_rate: m.sample_rate,
                channels: m.channels,
                length_frames: m.length_frames,
                content_hash: Some(m.content_hash.clone()),
                name: m.name.clone(),
                origin: MediaOrigin::Recorded { pass: m.pass },
            },
        );
        r.created.push(AnyId::Media(id));
        ids.push(id);
    }
    let files = Files { pass, ids: &ids };
    for l in &pass.lanes {
        d.lane(l.seq, l.lane)?;
        if l.takes.len() > MAX_PASS_TAKES {
            return Err(ModelError::range("record.takes", l.takes.len()));
        }
        for (a, b) in &l.clear {
            check_ticks("record.clear", a.0)?;
            check_ticks("record.clear", b.0)?;
            if a >= b {
                return Err(ModelError::range("record.clear", b.0));
            }
        }
        if !l.clear.is_empty() {
            let gone: Vec<_> = d
                .lane(l.seq, l.lane)?
                .active()
                .map(|t| {
                    t.events
                        .values()
                        .filter(|e| {
                            e.body.is_midi()
                                && l.clear.iter().any(|(a, b)| e.tick >= *a && e.tick < *b)
                        })
                        .map(|e| e.id)
                        .collect()
                })
                .unwrap_or_default();
            for id in gone {
                d.remove_event(id)?;
                r.removed.push(AnyId::Event(id));
            }
        }
        insert_content(d, l.seq, l.lane, None, &l.active, &files, r)?;
        for (name, content) in &l.takes {
            let take = d.counters_mut().alloc_take();
            let lane = d.lane_mut(l.seq, l.lane)?;
            lane.takes.push_back(Take::new(take, name.clone()));
            lane.active_take = take;
            r.created.push(AnyId::Take(take));
            insert_content(d, l.seq, l.lane, Some(take), content, &files, r)?;
        }
    }
    Ok(())
}
VIZDAW_EOF
```

In `crates/core-model/src/command/mod.rs`:

Find:
```rust
pub mod lane;
pub mod media;
pub mod mixer;
pub mod reference;
pub mod routing;
mod select;
```
Replace with:
```rust
pub mod lane;
pub mod media;
pub mod mixer;
pub mod record;
pub mod reference;
pub mod routing;
mod select;
```

Find:
```rust
pub use lane::LaneCmd;
pub use media::{MediaCmd, MediaSwap};
pub use mixer::MixerCmd;
pub use reference::ReferenceCmd;
pub use routing::RoutingCmd;
pub use sequence::SequenceCmd;
```
Replace with:
```rust
pub use lane::LaneCmd;
pub use media::{MediaCmd, MediaSwap};
pub use mixer::MixerCmd;
pub use record::{LaneRecord, RecordPass, RecordedContent, RecordedMedia, RecordedRegion};
pub use reference::ReferenceCmd;
pub use routing::RoutingCmd;
pub use sequence::SequenceCmd;
```

Find:
```rust
    Mixer(MixerCmd),
    /// Document-settings edits.
    Settings(SettingsCmd),
    /// Select events by rule, then optionally modify them (or copies).
    SelectAndModify {
        /// Rule; `None` uses the current selection as is.
```
Replace with:
```rust
    Mixer(MixerCmd),
    /// Document-settings edits.
    Settings(SettingsCmd),
    /// A finished recording pass, applied whole (sub-project 5 spec §4.7).
    Record(RecordPass),
    /// Select events by rule, then optionally modify them (or copies).
    SelectAndModify {
        /// Rule; `None` uses the current selection as is.
```

Find:
```rust
            Command::Media(c) => c.name(),
            Command::Mixer(c) => c.name(),
            Command::Settings(c) => c.name(),
            Command::SelectAndModify { modify, .. } => {
                modify.as_ref().map_or("Select", Modify::name)
            }
```
Replace with:
```rust
            Command::Media(c) => c.name(),
            Command::Mixer(c) => c.name(),
            Command::Settings(c) => c.name(),
            Command::Record(p) => p.name(),
            Command::SelectAndModify { modify, .. } => {
                modify.as_ref().map_or("Select", Modify::name)
            }
```

Find:
```rust
            settings::apply(&mut d, c, &mut report)?;
            None
        }
        Command::SelectAndModify {
            rule,
            mode,
```
Replace with:
```rust
            settings::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Record(p) => {
            record::apply(&mut d, p, &mut report)?;
            None
        }
        Command::SelectAndModify {
            rule,
            mode,
```

In `crates/core-model/src/history.rs`:

Find:
```rust
    executed: bool,
}

/// Undo/redo stacks plus the selection and the last modification.
#[derive(Debug, Clone)]
pub struct History {
```
Replace with:
```rust
    executed: bool,
}

/// A history group (sub-project 5 spec §4.7): every step made between
/// [`History::begin_group`] and [`History::end_group`] becomes one step.
/// Unlike a transaction, steps inside it stay undoable and redoable one by
/// one until it ends, and transactions (drags, edits) may open inside it.
#[derive(Debug, Clone)]
struct Group {
    /// `past.len()` when it opened: undo inside it stops there.
    start: usize,
    /// The redo stack when it opened (restored when it made no step).
    base_future: Vec<Version>,
    /// Redo inside it may not reach below this many future versions:
    /// those were undone before it opened (0 once it made a step, which
    /// clears the redo stack).
    redo_floor: usize,
    /// Whether any step was made inside it.
    executed: bool,
}

/// Undo/redo stacks plus the selection and the last modification.
#[derive(Debug, Clone)]
pub struct History {
```

Find:
```rust
    last_modify: Option<Modify>,
    selection: Selection,
    txn: Option<Txn>,
}

impl History {
```
Replace with:
```rust
    last_modify: Option<Modify>,
    selection: Selection,
    txn: Option<Txn>,
    group: Option<Group>,
}

impl History {
```

Find:
```rust
            last_modify: None,
            selection: Selection::default(),
            txn: None,
        }
    }
```
Replace with:
```rust
            last_modify: None,
            selection: Selection::default(),
            txn: None,
            group: None,
        }
    }
```

Find:
```rust
        self.future.last().map(|v| v.label.as_str())
    }

    /// The document [`History::undo`] would go back to (inside a
    /// transaction, only its own steps can be undone).
    pub fn peek_undo(&self) -> Option<&Document> {
        let floor = self.txn.as_ref().map_or(0, |t| t.start);
        if self.past.len() <= floor {
            return None;
        }
        self.past.last().map(|v| &v.doc)
```
Replace with:
```rust
        self.future.last().map(|v| v.label.as_str())
    }

    /// The lowest `past.len()` undo may reach: inside a transaction or a
    /// group, only their own steps can be undone.
    fn undo_floor(&self) -> usize {
        let t = self.txn.as_ref().map_or(0, |t| t.start);
        let g = self.group.as_ref().map_or(0, |g| g.start);
        t.max(g)
    }

    /// Whether redo may run: never inside a transaction; inside a group,
    /// only steps undone inside it.
    fn can_redo(&self) -> bool {
        self.txn.is_none()
            && self
                .group
                .as_ref()
                .is_none_or(|g| self.future.len() > g.redo_floor)
    }

    /// The document [`History::undo`] would go back to (inside a
    /// transaction, only its own steps can be undone).
    pub fn peek_undo(&self) -> Option<&Document> {
        if self.past.len() <= self.undo_floor() {
            return None;
        }
        self.past.last().map(|v| &v.doc)
```

Find:
```rust
    /// The document [`History::redo`] would bring back (none while a
    /// transaction is open).
    pub fn peek_redo(&self) -> Option<&Document> {
        if self.txn.is_some() {
            return None;
        }
        self.future.last().map(|v| &v.doc)
```
Replace with:
```rust
    /// The document [`History::redo`] would bring back (none while a
    /// transaction is open).
    pub fn peek_redo(&self) -> Option<&Document> {
        if !self.can_redo() {
            return None;
        }
        self.future.last().map(|v| &v.doc)
```

Find:
```rust
        let txn = self
            .txn
            .iter()
            .flat_map(|t| std::iter::once(&t.base).chain(t.base_future.iter()));
        let mut last: Option<&Document> = None;
        for v in self
            .past
```
Replace with:
```rust
        let txn = self
            .txn
            .iter()
            .flat_map(|t| std::iter::once(&t.base).chain(t.base_future.iter()))
            .chain(self.group.iter().flat_map(|g| g.base_future.iter()));
        let mut last: Option<&Document> = None;
        for v in self
            .past
```

Find:
```rust
        self.txn.is_some()
    }

    fn trim(&mut self) {
        if self.txn.is_some() {
            return;
        }
        let depth =
```
Replace with:
```rust
        self.txn.is_some()
    }

    /// Whether a group is open.
    pub fn in_group(&self) -> bool {
        self.group.is_some()
    }

    fn trim(&mut self) {
        if self.txn.is_some() || self.group.is_some() {
            return;
        }
        let depth =
```

Find:
```rust
            if let Some(t) = self.txn.as_mut() {
                t.executed = true;
            }
            self.trim();
        }
        let sel = applied.selection.unwrap_or_else(|| self.selection.clone());
```
Replace with:
```rust
            if let Some(t) = self.txn.as_mut() {
                t.executed = true;
            }
            self.group_stepped();
            self.trim();
        }
        let sel = applied.selection.unwrap_or_else(|| self.selection.clone());
```

Find:
```rust
    /// Steps back one version. Inside a transaction, only the transaction's
    /// own steps can be undone.
    pub fn undo(&mut self) -> Result<()> {
        let floor = self.txn.as_ref().map_or(0, |t| t.start);
        if self.past.len() <= floor {
            return Err(ModelError::invalid("nothing to undo"));
        }
        let prev = self
```
Replace with:
```rust
    /// Steps back one version. Inside a transaction, only the transaction's
    /// own steps can be undone.
    pub fn undo(&mut self) -> Result<()> {
        if self.past.len() <= self.undo_floor() {
            return Err(ModelError::invalid("nothing to undo"));
        }
        let prev = self
```

Find:
```rust
        Ok(())
    }

    /// Steps forward one version. Refused while a transaction is open.
    pub fn redo(&mut self) -> Result<()> {
        if self.txn.is_some() {
            return Err(ModelError::invalid("nothing to redo"));
        }
        let next = self
```
Replace with:
```rust
        Ok(())
    }

    /// Steps forward one version. Refused while a transaction is open,
    /// and inside a group for steps undone before it opened.
    pub fn redo(&mut self) -> Result<()> {
        if !self.can_redo() {
            return Err(ModelError::invalid("nothing to redo"));
        }
        let next = self
```

Find:
```rust
        } else {
            t.base_future
        };
        self.trim();
        Ok(())
    }
```
Replace with:
```rust
        } else {
            t.base_future
        };
        if t.executed {
            self.group_stepped();
        }
        self.trim();
        Ok(())
    }

    /// A step was made: an open group made one, and its redo stack is gone.
    fn group_stepped(&mut self) {
        if let Some(g) = self.group.as_mut() {
            g.executed = true;
            g.redo_floor = 0;
        }
    }

    /// Opens a group (a recording pass, spec §4.7): every step until
    /// [`History::end_group`] becomes one. Refused while a group or a
    /// transaction is open.
    pub fn begin_group(&mut self) -> Result<()> {
        if self.group.is_some() {
            return Err(ModelError::invalid("a group is already open"));
        }
        if self.txn.is_some() {
            return Err(ModelError::invalid("finish the open edit first"));
        }
        self.group = Some(Group {
            start: self.past.len(),
            base_future: self.future.clone(),
            redo_floor: self.future.len(),
            executed: false,
        });
        Ok(())
    }

    /// Closes the group: its steps become one step named `label` ("Undo
    /// Record"); a group that made none adds nothing and keeps the redo
    /// stack it found. Refused while a transaction is open (the step
    /// would cut it in two) or when no group is open.
    pub fn end_group(&mut self, label: impl Into<String>) -> Result<()> {
        if self.txn.is_some() {
            return Err(ModelError::invalid("finish the open edit first"));
        }
        let g = self
            .group
            .take()
            .ok_or_else(|| ModelError::invalid("no open group"))?;
        if self.past.len() > g.start {
            self.past.truncate(g.start + 1);
            self.present.label = label.into();
            self.present.at = Timestamp::now();
        }
        // Versions undone inside the group are intermediate states.
        self.future = if g.executed {
            Vec::new()
        } else {
            g.base_future
        };
        self.trim();
        Ok(())
    }
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_record 2>&1 | tail -3
```
Expected: `test result: ok. 8 passed`, including the proptest `record_then_undo_restores_the_document` and Review Focus 1's `a_group_makes_one_record_step_of_everything_inside_it` and `a_group_that_made_nothing_keeps_the_redo_stack`.

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p core-model 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/record.rs crates/core-model/src/command/mod.rs crates/core-model/src/history.rs crates/core-model/tests/cmd_record.rs
git commit -F - <<'VIZDAW_MSG'
core-model: the recording pass and history groups

Command::Record applies a pass whole (recorded media, cleared ranges,
events, regions, new takes, the active take) or refuses it whole. A
history group turns everything made while recording into one step
named after the pass; undo and redo inside it stay within it.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 4: `core-media` — the input ring and the record writer

**Files:**
- Create: `crates/core-media/src/record.rs`
- Modify: `crates/core-media/src/lib.rs`
- Modify: `crates/core-media/src/caf.rs` (`CafWriter::sync`, `CafWriter::clipped`)
- Test: `crates/core-media/tests/record.rs` (the crate's first `tests/` file)

**Interfaces:**
- Produces:
  - `record::input_ring(channels, frames) -> (RingWriter, RingReader)`; `RingWriter::push(run, frames, sample: impl Fn(channel, frame) -> f32) -> bool` (whole block or nothing; `// RT`), `set_end(run)`, `close()`; `RingReader::pop(&mut Vec<f32>) -> Option<Block>`, `has_records`, `closed`, `end_run`; `RING_SECONDS` 4.0, `SYNC_EVERY` 2 s, `MAX_RING_CHANNELS` 64;
  - `RecordWriter::start(reader, WriterConfig) -> Result<RecordWriter, MediaError>` (makes every file first; on failure removes those it made), `status() -> WriterStatus`, `finish(wait) -> WriterResult`; `WriterLane { path, channels: Vec<usize> }`, `LaneWritten { path, frames, clipped }`;
  - `free_bytes(path) -> Option<u64>` (`statvfs` on unix);
  - `CafWriter::sync()` (writes the buffered data and `sync_data`), `CafWriter::clipped()` (samples beyond ±1.0 so far).

Spec §6.2, Rulings 4 and 5. Lost blocks and the tail become silence, so file frame *k* is run frame *k*.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-media/tests
cat > crates/core-media/tests/record.rs <<'VIZDAW_EOF'
//! The recording ring and writer (plan 5c Task 4; sub-project 5 spec
//! §6.2): blocks go through the ring whole or not at all, each lane's
//! channels land in its own CAF in the project format, lost blocks and a
//! short end are silence so file frame k is run frame k, files read while
//! they are written (a crash leaves them valid), and a writer whose ring
//! never closes still finishes.

use core_media::caf::CafReader;
use core_media::record::{RecordWriter, WriterConfig, WriterLane, free_bytes, input_ring};
use core_model::format::BitDepth;
use std::path::Path;
use std::time::{Duration, Instant};

/// Sample `f` of ring channel `c` in the tests' blocks: unique per
/// channel and frame, within ±1.
fn value(c: usize, f: u64) -> f32 {
    ((f % 1000) as f32 / 1000.0 - 0.5) * (1.0 - 0.25 * c as f32)
}

fn read(path: &Path) -> (core_media::caf::CafInfo, Vec<f32>) {
    let r = CafReader::open(path).unwrap();
    let info = r.info().clone();
    let mut out = vec![0.0; info.frames as usize * usize::from(info.channels)];
    r.read(0, &mut out, &mut Vec::new()).unwrap();
    (info, out)
}

#[test]
fn blocks_go_through_whole_or_not_at_all() {
    let (mut w, mut r) = input_ring(3, 256);
    let mut out = Vec::new();
    assert!(r.pop(&mut out).is_none());
    let mut run = 0u64;
    // Many blocks: the indices wrap around the buffer many times.
    for k in 0..500u64 {
        let frames = 13 + (k % 50) as u32;
        let base = run;
        assert!(w.push(run, frames, |c, f| value(c, base + f as u64)));
        let b = r.pop(&mut out).unwrap();
        assert_eq!((b.run, b.frames), (run, frames));
        assert_eq!(out.len(), frames as usize * 3);
        for (i, x) in out.iter().enumerate() {
            assert_eq!(*x, value(i % 3, run + (i / 3) as u64));
        }
        run += u64::from(frames);
    }
    // A full ring refuses a block and keeps what it holds.
    let mut pushed = 0;
    while w.push(run + pushed * 64, 64, |_, _| 0.5) {
        pushed += 1;
    }
    assert!(pushed > 0);
    for k in 0..pushed {
        let b = r.pop(&mut out).unwrap();
        assert_eq!(b.run, run + k * 64);
        assert!(out.iter().all(|x| *x == 0.5));
    }
    assert!(r.pop(&mut out).is_none());
    // A block bigger than the whole ring never fits.
    assert!(!w.push(0, 1_000_000, |_, _| 0.0));
    assert!(!r.closed());
    drop(w);
    assert!(r.closed(), "dropping the writer closes the ring");
}

fn lanes(dir: &Path) -> Vec<WriterLane> {
    vec![
        WriterLane {
            path: dir.join("Recorded/mono.caf"),
            channels: vec![2],
        },
        WriterLane {
            path: dir.join("Recorded/stereo.caf"),
            channels: vec![0, 1],
        },
    ]
}

#[test]
fn each_lane_gets_its_channels_and_gaps_are_silence() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, r) = input_ring(3, 48_000);
    let writer = RecordWriter::start(
        r,
        WriterConfig {
            rate: 48_000,
            depth: BitDepth::Float32,
            lanes: lanes(dir.path()),
        },
    )
    .unwrap();
    // Run frames 0..512 and 1024..1536; 512..1024 were lost; the end is at
    // 2048 (the last block was lost too).
    for (run, n) in [(0u64, 256u32), (256, 256), (1024, 512)] {
        assert!(w.push(run, n, |c, f| value(c, run + f as u64)));
    }
    w.set_end(2048);
    w.close();
    let res = writer.finish(Duration::from_secs(30));
    assert_eq!(res.error, None);
    assert_eq!(res.gap_frames, 512 + 512);
    assert_eq!(res.lanes.len(), 2);
    let (info, mono) = read(&dir.path().join("Recorded/mono.caf"));
    assert_eq!(
        (info.channels, info.frames, info.size_known, info.recording),
        (1, 2048, true, true)
    );
    for (f, x) in mono.iter().enumerate() {
        let f = f as u64;
        let want = if f < 512 || (1024..1536).contains(&f) {
            value(2, f)
        } else {
            0.0
        };
        assert_eq!(*x, want, "frame {f}");
    }
    let (info, st) = read(&dir.path().join("Recorded/stereo.caf"));
    assert_eq!((info.channels, info.frames), (2, 2048));
    assert_eq!(st[0], value(0, 0));
    assert_eq!(st[1], value(1, 0));
    assert_eq!(st[2 * 1100], value(0, 1100));
    assert_eq!(st[2 * 1100 + 1], value(1, 1100));
    assert_eq!(res.lanes[0].written.frames, 2048);
    assert_eq!(res.lanes[0].written.content_hash.len(), 64);
}

#[test]
fn files_are_readable_while_recording_and_in_the_project_depth() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, r) = input_ring(3, 48_000);
    let writer = RecordWriter::start(
        r,
        WriterConfig {
            rate: 44_100,
            depth: BitDepth::Int16,
            lanes: lanes(dir.path()),
        },
    )
    .unwrap();
    assert!(w.push(0, 4410, |c, f| value(c, f as u64)));
    // A clipped sample on the mono lane.
    assert!(w.push(4410, 1, |c, _| if c == 2 { 1.5 } else { 0.0 }));
    w.set_end(4411);
    let until = Instant::now() + Duration::from_secs(60);
    while writer.status().frames < 4411 {
        assert!(Instant::now() < until, "the writer never caught up");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(writer.status().clipped, vec![1, 0]);
    // As after a crash: open while the writer holds it.
    let (info, _) = read(&dir.path().join("Recorded/mono.caf"));
    assert_eq!(
        (
            info.sample_rate,
            info.depth,
            info.size_known,
            info.recording
        ),
        (44_100, BitDepth::Int16, false, true)
    );
    assert!(info.frames >= 4410, "{}", info.frames);
    w.close();
    let res = writer.finish(Duration::from_secs(30));
    assert_eq!(res.lanes[0].written.clipped, 1);
    let (info, _) = read(&dir.path().join("Recorded/mono.caf"));
    assert_eq!((info.frames, info.size_known), (4411, true));
}

#[test]
fn a_ring_that_never_closes_still_finishes_padded_to_its_end() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, r) = input_ring(3, 4800);
    let writer = RecordWriter::start(
        r,
        WriterConfig {
            rate: 48_000,
            depth: BitDepth::Int24,
            lanes: lanes(dir.path()),
        },
    )
    .unwrap();
    assert!(w.push(0, 100, |_, _| 0.25));
    w.set_end(300);
    let t = Instant::now();
    // The producer vanished without closing (an input that died).
    let res = writer.finish(Duration::from_millis(200));
    assert!(t.elapsed() < Duration::from_secs(20));
    assert_eq!(res.lanes.len(), 2);
    assert_eq!(res.lanes[1].written.frames, 300);
    assert_eq!(res.gap_frames, 200);
    drop(w);
}

#[test]
fn a_file_that_cannot_be_made_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Recorded")).unwrap();
    std::fs::write(dir.path().join("Recorded/stereo.caf"), b"taken").unwrap();
    let (_w, r) = input_ring(3, 4800);
    let e = RecordWriter::start(
        r,
        WriterConfig {
            rate: 48_000,
            depth: BitDepth::Int24,
            lanes: lanes(dir.path()),
        },
    );
    assert!(e.is_err());
    assert!(!dir.path().join("Recorded/mono.caf").exists());
    assert_eq!(
        std::fs::read(dir.path().join("Recorded/stereo.caf")).unwrap(),
        b"taken",
        "never overwritten"
    );
    // A lane naming a channel the ring does not carry.
    let (_w, r) = input_ring(1, 4800);
    let bad = RecordWriter::start(
        r,
        WriterConfig {
            rate: 48_000,
            depth: BitDepth::Int24,
            lanes: vec![WriterLane {
                path: dir.path().join("x.caf"),
                channels: vec![3],
            }],
        },
    );
    assert!(bad.is_err());
    assert!(!dir.path().join("x.caf").exists());
}

#[test]
fn free_space_is_known_for_a_real_folder() {
    let dir = tempfile::tempdir().unwrap();
    assert!(free_bytes(dir.path()).is_some_and(|b| b > 0));
    assert_eq!(free_bytes(Path::new("/no/such/folder/anywhere")), None);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-media --test record 2>&1 | tail -5
```
Expected: does not compile: `core_media::record` does not exist.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-media/src/record.rs <<'VIZDAW_EOF'
//! Recording (sub-project 5 spec §6.2; plan 5c Rulings 3–5): the input
//! ring the real-time thread fills and the writer thread that drains it
//! into one CAF per armed audio lane.
//!
//! **The ring** holds records of whole blocks: a four-word header
//! (frames, the block's run frame as two words, channels) and the block's
//! samples interleaved by ring channel, as `f32` bits in `AtomicU32`s. The
//! producer ([`RingWriter`], real-time) writes a record only when all of
//! it fits and publishes it with one `Release` store of the head; the
//! consumer ([`RingReader`]) frees it with one `Release` store of the
//! tail. A block that does not fit is lost; the next record's run frame
//! shows the gap, and the writer fills it with silence.
//!
//! **The writer** ([`RecordWriter`]) runs on its own thread: it writes
//! each lane's channels (one, or a pair) to its own recording-mode CAF
//! (data size −1 until finished, so a crash leaves a readable file),
//! syncs the files every [`SYNC_EVERY`], and pads gaps and the end with
//! silence so file frame `k` is always run frame `k`.

use crate::MediaError;
use crate::caf::{CafWriter, WriteMode, Written};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Words in a record header.
pub const HEADER: usize = 4;
/// How long the ring holds (spec §6.2: 4 s).
pub const RING_SECONDS: f64 = 4.0;
/// How often the writer syncs its files (spec §6.2: 2 s).
pub const SYNC_EVERY: Duration = Duration::from_secs(2);
/// The most channels one ring carries.
pub const MAX_RING_CHANNELS: usize = 64;

#[derive(Debug)]
struct Ring {
    buf: Box<[AtomicU32]>,
    channels: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
    closed: AtomicBool,
    end_run: AtomicU64,
}

impl Ring {
    fn word(&self, at: usize) -> Option<&AtomicU32> {
        self.buf.get(at % self.buf.len().max(1))
    }
}

/// The real-time side of an input ring. Dropping it closes the ring.
#[derive(Debug)]
pub struct RingWriter {
    ring: Arc<Ring>,
    head: usize,
}

/// The writer thread's side of an input ring.
#[derive(Debug)]
pub struct RingReader {
    ring: Arc<Ring>,
    tail: usize,
}

/// A ring for `channels` channels (1–[`MAX_RING_CHANNELS`]) holding
/// `frames` frames of samples plus the headers of blocks of 8 frames or
/// more. Allocates; call off the real-time thread.
pub fn input_ring(channels: usize, frames: usize) -> (RingWriter, RingReader) {
    let channels = channels.clamp(1, MAX_RING_CHANNELS);
    let words = frames
        .saturating_mul(channels)
        .saturating_add((frames / 8 + 16).saturating_mul(HEADER))
        .max(HEADER + channels);
    let ring = Arc::new(Ring {
        buf: (0..words).map(|_| AtomicU32::new(0)).collect(),
        channels,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
        end_run: AtomicU64::new(0),
    });
    (
        RingWriter {
            ring: Arc::clone(&ring),
            head: 0,
        },
        RingReader { ring, tail: 0 },
    )
}

impl RingWriter {
    /// The ring's channels.
    pub fn channels(&self) -> usize {
        self.ring.channels
    }

    /// Writes one block of `frames` frames stamped `run`, each sample from
    /// `sample(ring channel, frame)`. False when it does not fit (the
    /// block is lost). // RT
    pub fn push(&mut self, run: u64, frames: u32, sample: impl Fn(usize, usize) -> f32) -> bool {
        let ch = self.ring.channels;
        let n = frames as usize;
        let need = HEADER + n.saturating_mul(ch);
        let tail = self.ring.tail.load(Ordering::Acquire);
        let used = self.head.wrapping_sub(tail);
        if need > self.ring.buf.len().saturating_sub(used) {
            return false;
        }
        let header = [frames, run as u32, (run >> 32) as u32, ch as u32];
        for (k, w) in header.iter().enumerate() {
            if let Some(cell) = self.ring.word(self.head.wrapping_add(k)) {
                cell.store(*w, Ordering::Relaxed);
            }
        }
        let mut at = self.head.wrapping_add(HEADER);
        for f in 0..n {
            for c in 0..ch {
                if let Some(cell) = self.ring.word(at) {
                    cell.store(sample(c, f).to_bits(), Ordering::Relaxed);
                }
                at = at.wrapping_add(1);
            }
        }
        self.head = self.head.wrapping_add(need);
        self.ring.head.store(self.head, Ordering::Release);
        true
    }

    /// Records that run frames `[0, run)` were captured or lost: the
    /// writer pads to it at the end. // RT
    pub fn set_end(&self, run: u64) {
        self.ring.end_run.store(run, Ordering::Release);
    }

    /// No more records will come. // RT
    pub fn close(&self) {
        self.ring.closed.store(true, Ordering::Release);
    }
}

impl Drop for RingWriter {
    fn drop(&mut self) {
        self.close();
    }
}

/// One record taken from the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// The run frame of its first frame.
    pub run: u64,
    /// Its frames.
    pub frames: u32,
}

impl RingReader {
    /// The ring's channels.
    pub fn channels(&self) -> usize {
        self.ring.channels
    }

    /// The next record: its samples (interleaved by ring channel) replace
    /// `out`'s contents.
    pub fn pop(&mut self, out: &mut Vec<f32>) -> Option<Block> {
        let head = self.ring.head.load(Ordering::Acquire);
        if head.wrapping_sub(self.tail) < HEADER {
            return None;
        }
        let w = |k: usize| {
            self.ring
                .word(self.tail.wrapping_add(k))
                .map_or(0, |c| c.load(Ordering::Relaxed))
        };
        let frames = w(0);
        let run = u64::from(w(1)) | (u64::from(w(2)) << 32);
        let ch = (w(3) as usize).max(1);
        let n = (frames as usize).saturating_mul(ch);
        out.clear();
        out.extend((0..n).map(|k| f32::from_bits(w(HEADER + k))));
        self.tail = self.tail.wrapping_add(HEADER + n);
        self.ring.tail.store(self.tail, Ordering::Release);
        Some(Block { run, frames })
    }

    /// A record is waiting.
    pub fn has_records(&self) -> bool {
        self.ring
            .head
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail)
            >= HEADER
    }

    /// The producer closed the ring (or was dropped).
    pub fn closed(&self) -> bool {
        self.ring.closed.load(Ordering::Acquire)
    }

    /// Run frames captured or lost so far ([`RingWriter::set_end`]).
    pub fn end_run(&self) -> u64 {
        self.ring.end_run.load(Ordering::Acquire)
    }
}

/// One armed audio lane's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterLane {
    /// The CAF to create (it must not exist).
    pub path: PathBuf,
    /// Its ring channels: one (mono) or two (left, right).
    pub channels: Vec<usize>,
}

/// What a [`RecordWriter`] writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterConfig {
    /// The project rate.
    pub rate: u32,
    /// The project depth.
    pub depth: BitDepth,
    /// Each lane's file.
    pub lanes: Vec<WriterLane>,
}

/// A finished lane file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneWritten {
    /// Its path.
    pub path: PathBuf,
    /// What was written.
    pub written: Written,
}

/// What a finished writer wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterResult {
    /// Each lane's file, in [`WriterConfig::lanes`] order (a lane whose
    /// file failed is missing; see `error`).
    pub lanes: Vec<LaneWritten>,
    /// Frames written as silence because their blocks were lost.
    pub gap_frames: u64,
    /// Why writing stopped early (a full disk), if it did.
    pub error: Option<String>,
}

/// What a running writer has done so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterStatus {
    /// Frames written per file (gaps included).
    pub frames: u64,
    /// Frames written as silence for lost blocks.
    pub gap_frames: u64,
    /// Samples clipped per lane (integer depths).
    pub clipped: Vec<u64>,
    /// Why writing stopped, if it did.
    pub error: Option<String>,
}

#[derive(Debug)]
struct Shared {
    /// Stop once the ring is drained, or at this instant at the latest.
    stop_by: Mutex<Option<Instant>>,
    frames: AtomicU64,
    gaps: AtomicU64,
    clipped: Box<[AtomicU64]>,
    error: Mutex<Option<String>>,
}

/// The writer thread (spec §6.2).
#[derive(Debug)]
pub struct RecordWriter {
    thread: Option<JoinHandle<WriterResult>>,
    shared: Arc<Shared>,
}

/// The lock of a mutex even if a panic poisoned it.
fn guard<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Lane {
    writer: CafWriter,
    path: PathBuf,
    channels: Vec<usize>,
}

impl RecordWriter {
    /// Creates every lane's file (parent folders too; never overwriting)
    /// and starts the thread. When a file cannot be created, the files
    /// made so far are removed and nothing runs.
    pub fn start(reader: RingReader, cfg: WriterConfig) -> Result<RecordWriter, MediaError> {
        let mut lanes: Vec<Lane> = Vec::new();
        let made = |lanes: &[Lane]| {
            for l in lanes {
                let _ = std::fs::remove_file(&l.path);
            }
        };
        for l in &cfg.lanes {
            let n = l.channels.len();
            if !(1..=2).contains(&n) || l.channels.iter().any(|c| *c >= reader.channels()) {
                made(&lanes);
                return Err(MediaError::Unsupported(format!(
                    "{}: ring channels {:?}",
                    l.path.display(),
                    l.channels
                )));
            }
            if let Some(dir) = l.path.parent() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    made(&lanes);
                    return Err(e.into());
                }
            }
            match CafWriter::create(&l.path, cfg.rate, n as u16, cfg.depth, WriteMode::Recording) {
                Ok(writer) => lanes.push(Lane {
                    writer,
                    path: l.path.clone(),
                    channels: l.channels.clone(),
                }),
                Err(e) => {
                    made(&lanes);
                    return Err(e);
                }
            }
        }
        let shared = Arc::new(Shared {
            stop_by: Mutex::new(None),
            frames: AtomicU64::new(0),
            gaps: AtomicU64::new(0),
            clipped: (0..lanes.len()).map(|_| AtomicU64::new(0)).collect(),
            error: Mutex::new(None),
        });
        let s = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("vizdaw-record".into())
            .spawn(move || run(reader, lanes, &s));
        match thread {
            Ok(t) => Ok(RecordWriter {
                thread: Some(t),
                shared,
            }),
            Err(e) => Err(MediaError::Io(format!("record writer thread: {e}"))),
        }
    }

    /// What it has done so far.
    pub fn status(&self) -> WriterStatus {
        WriterStatus {
            frames: self.shared.frames.load(Ordering::Relaxed),
            gap_frames: self.shared.gaps.load(Ordering::Relaxed),
            clipped: self
                .shared
                .clipped
                .iter()
                .map(|c| c.load(Ordering::Relaxed))
                .collect(),
            error: guard(&self.shared.error).clone(),
        }
    }

    /// Finishes: drains the ring until it is closed (or for `wait` at
    /// most), pads to the end, patches every file's size and syncs it.
    pub fn finish(mut self, wait: Duration) -> WriterResult {
        *guard(&self.shared.stop_by) = Some(Instant::now() + wait);
        let r = self.thread.take().map(JoinHandle::join);
        match r {
            Some(Ok(r)) => r,
            _ => WriterResult {
                lanes: Vec::new(),
                gap_frames: self.shared.gaps.load(Ordering::Relaxed),
                error: Some("the record writer stopped unexpectedly".into()),
            },
        }
    }
}

impl Drop for RecordWriter {
    fn drop(&mut self) {
        if let Some(t) = self.thread.take() {
            *guard(&self.shared.stop_by) = Some(Instant::now());
            let _ = t.join();
        }
    }
}

/// Writes `frames` frames of silence to every lane.
fn pad(lanes: &mut [Lane], frames: u64, scratch: &mut Vec<f32>) -> Result<(), MediaError> {
    let mut left = frames;
    while left > 0 {
        let n = left.min(4096) as usize;
        for l in lanes.iter_mut() {
            scratch.clear();
            scratch.resize(n * l.channels.len(), 0.0);
            l.writer.write(scratch)?;
        }
        left -= n as u64;
    }
    Ok(())
}

/// Writes one block (interleaved by ring channel) to every lane.
fn write_block(
    lanes: &mut [Lane],
    block: &[f32],
    ch: usize,
    skip: usize,
    scratch: &mut Vec<f32>,
) -> Result<(), MediaError> {
    let frames = block.len() / ch.max(1);
    for l in lanes.iter_mut() {
        scratch.clear();
        for f in skip..frames {
            for c in &l.channels {
                scratch.push(block.get(f * ch + c).copied().unwrap_or(0.0));
            }
        }
        l.writer.write(scratch)?;
    }
    Ok(())
}

fn run(mut reader: RingReader, mut lanes: Vec<Lane>, s: &Shared) -> WriterResult {
    let ch = reader.channels();
    let mut block = Vec::new();
    let mut scratch = Vec::new();
    let mut next = 0u64;
    let mut gaps = 0u64;
    let mut synced = Instant::now();
    let mut error: Option<String> = None;
    let fail = |e: MediaError, error: &mut Option<String>| {
        let why = e.to_string();
        *guard(&s.error) = Some(why.clone());
        *error = Some(why);
    };
    loop {
        if error.is_some() {
            break;
        }
        match reader.pop(&mut block) {
            Some(b) => {
                let end = b.run.saturating_add(u64::from(b.frames));
                if end <= next {
                    continue;
                }
                if b.run > next {
                    let gap = b.run - next;
                    if let Err(e) = pad(&mut lanes, gap, &mut scratch) {
                        fail(e, &mut error);
                        continue;
                    }
                    gaps += gap;
                    s.gaps.store(gaps, Ordering::Relaxed);
                    next = b.run;
                }
                let skip = (next - b.run) as usize;
                if let Err(e) = write_block(&mut lanes, &block, ch, skip, &mut scratch) {
                    fail(e, &mut error);
                    continue;
                }
                next = end;
                s.frames.store(next, Ordering::Relaxed);
                for (l, c) in lanes.iter().zip(s.clipped.iter()) {
                    c.store(l.writer.clipped(), Ordering::Relaxed);
                }
            }
            None => {
                let stop_by = *guard(&s.stop_by);
                // `closed` is read before the head is read again, so a
                // record pushed before the close is never missed.
                let closed = reader.closed();
                let late = stop_by.is_some_and(|t| Instant::now() >= t);
                if (stop_by.is_some() && closed) || late {
                    if !late && reader.has_records() {
                        continue;
                    }
                    let end = reader.end_run();
                    if end > next {
                        match pad(&mut lanes, end - next, &mut scratch) {
                            Ok(()) => {
                                gaps += end - next;
                                next = end;
                            }
                            Err(e) => fail(e, &mut error),
                        }
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        if synced.elapsed() >= SYNC_EVERY {
            for l in &lanes {
                if let Err(e) = l.writer.sync() {
                    fail(e, &mut error);
                    break;
                }
            }
            synced = Instant::now();
        }
    }
    s.gaps.store(gaps, Ordering::Relaxed);
    s.frames.store(next, Ordering::Relaxed);
    let mut out = Vec::with_capacity(lanes.len());
    for l in lanes {
        let path = l.path.clone();
        match l.writer.finish() {
            Ok(written) => out.push(LaneWritten { path, written }),
            Err(e) => {
                if error.is_none() {
                    error = Some(format!("{}: {e}", path.display()));
                }
            }
        }
    }
    WriterResult {
        lanes: out,
        gap_frames: gaps,
        error,
    }
}

/// Free bytes on the volume holding `path`, or `None` when the system
/// cannot say.
pub fn free_bytes(path: &Path) -> Option<u64> {
    free_bytes_impl(path)
}

#[cfg(unix)]
fn free_bytes_impl(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statvfs` only writes the zeroed struct we pass and reads the
    // NUL-terminated path.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    let r = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if r != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)]
    Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

#[cfg(not(unix))]
fn free_bytes_impl(_path: &Path) -> Option<u64> {
    None
}
VIZDAW_EOF
```

In `crates/core-media/src/lib.rs`:

Find:
```rust
//! `core-media`: VizDAW's audio files (sub-project 5 spec §5). The CAF
//! reader and writer every media file goes through, the import pipeline
//! over a platform `MediaImporter`, and the disk streamer the real-time
//! thread reads regions from. Platform-neutral: no OS audio APIs here.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
```
Replace with:
```rust
//! `core-media`: VizDAW's audio files (sub-project 5 spec §5). The CAF
//! reader and writer every media file goes through, the import pipeline
//! over a platform `MediaImporter`, the disk streamer the real-time
//! thread reads regions from, and the recording ring and writer (5c).
//! Platform-neutral: no OS audio APIs here.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
```

Find:
```rust
mod error;
pub mod import;
pub mod overview;
pub mod stream;

pub use error::MediaError;
```
Replace with:
```rust
mod error;
pub mod import;
pub mod overview;
pub mod record;
pub mod stream;

pub use error::MediaError;
```

In `crates/core-media/src/caf.rs`:

Find:
```rust
    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Patches the data size and syncs the file.
```
Replace with:
```rust
    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Samples clipped so far.
    pub fn clipped(&self) -> u64 {
        self.clipped
    }

    /// Flushes what was written to the disk (a recording does this every
    /// 2 s, spec §6.2); the data size stays −1 until [`CafWriter::finish`].
    pub fn sync(&self) -> Result<(), MediaError> {
        self.file.sync_data()?;
        Ok(())
    }

    /// Patches the data size and syncs the file.
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-media --test record 2>&1 | tail -3
```
Expected: `test result: ok. 6 passed`, including `a_file_that_cannot_be_made_leaves_nothing_behind` (Review Focus 3).

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p core-media 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-media/src/record.rs crates/core-media/src/lib.rs crates/core-media/src/caf.rs crates/core-media/tests/record.rs
git commit -F - <<'VIZDAW_MSG'
core-media: the input ring and the record writer

A lock-free ring carries whole input blocks stamped with run frames
(a full ring loses whole blocks and counts them). The writer thread
writes one recording-mode CAF per armed audio lane, fills gaps and the
tail with silence, counts clipped samples, syncs every 2 s, and makes
every file before it starts or leaves none.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 5: `core-engine` — the record tap and the fake input

**Files:**
- Create: `crates/core-engine/src/record.rs`
- Modify: `crates/core-engine/src/backend.rs` (`InputBlock`, `MAX_INPUT_CHANNELS`, `AudioConfig.input_enabled`/`input_device`, `StreamInfo` input fields)
- Modify: `crates/core-engine/src/render.rs` (`process_input`, `EngineCmd::Record`, the tap at roll, wrap, resync and removal)
- Modify: `crates/core-engine/src/engine.rs` (`start_recording`, `stop_recording`)
- Modify: `crates/core-engine/src/offline.rs` (`FakeInput`, `fake_signal`, `OfflineBackend::with_input`)
- Modify: `crates/core-engine/src/lib.rs`
- Modify: `crates/platform-macos/src/hal.rs` (zero input fields until Task 11)
- Modify: `crates/vizdaw-play/src/live.rs` (`..AudioConfig::default()`)
- Test: `crates/core-engine/tests/record_tap.rs`, `crates/core-engine/tests/rt_alloc_record.rs`

**Interfaces:**
- Consumes: Task 4's ring.
- Produces:
  - `record::{Mark { run, frame, wrap }, RecordedMidi { run, lane, msg }, RecordStatus, RecordRequest { ring, range, midi_lanes }, RecordHandles { status, marks: MarkReader, midi: MidiReader }, tap(req, latency)}`; `MARK_CAPACITY` 4096, `MIDI_CAPACITY` 16 384;
  - `Renderer::process_input(out, input: &InputBlock, time)` (`process` is it with no input);
  - `Engine::start_recording(RecordRequest) -> Result<RecordHandles, EngineError>`, `Engine::stop_recording()`;
  - `offline::FakeInput { Silent, Signals, Loopback(u32) }`, `fake_signal(channel, frame)`, `OfflineBackend::with_input(FakeInput)`; `StreamInfo { input_channels, input_names, input_latency, output_latency, .. }`.

Ruling 3: the tap counts run frames, pushes the armed channels of each block into the ring, and marks the roll, every wrap and every resync. Ruling 7: the offline device's fake input. The rt-check gate starts covering the ring here.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/record_tap.rs <<'VIZDAW_EOF'
//! The renderer's record tap and the fake input (plan 5c Task 5;
//! sub-project 5 spec §6.2–§6.4, §12.1): blocks of input are stamped with
//! continuous run frames from the first block that plays, marks say which
//! schedule frame plays at the roll and at every loop wrap (and after a
//! block the transport did not advance through), a full ring loses whole
//! blocks and counts them, and the tap reads closed once it is removed or
//! its renderer goes.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks, fake_signal};
use core_engine::record::{Mark, RecordHandles, RecordRequest};
use core_model::document::Document;
use core_model::ids::SeqId;

const SR: f64 = 48_000.0;

/// One bar of notes on the built-in synth (2 s at 120 BPM).
fn one_bar() -> (Document, SeqId) {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = b.sequence("Bar", "Notes", Some(synth)).unwrap();
    b.notes(
        seq,
        lane,
        &[
            (0, 60, 0x8000, Q),
            (Q, 64, 0x8000, Q),
            (2 * Q, 67, 0x8000, Q),
            (3 * Q, 72, 0x8000, Q),
        ],
    )
    .unwrap();
    (b.finish(), seq)
}

fn setup(input: FakeInput, block: u32, sinks: OfflineSinks) -> (Engine, OfflineBackend) {
    let (doc, seq) = one_bar();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, block).with_input(input);
    b.start(renderer).unwrap();
    (engine, b)
}

fn request(channels: Vec<u16>, ring_frames: usize) -> RecordRequest {
    RecordRequest {
        channels,
        range: None,
        ring_frames,
    }
}

/// Every block in the ring as `(run, frames, samples)`.
fn drain(h: &mut RecordHandles) -> Vec<(u64, u32, Vec<f32>)> {
    let mut out = Vec::new();
    let mut buf = Vec::new();
    while let Some(b) = h.ring.pop(&mut buf) {
        out.push((b.run, b.frames, buf.clone()));
    }
    out
}

fn marks(h: &mut RecordHandles) -> Vec<(u64, u64)> {
    full_marks(h).iter().map(|m| (m.run, m.frame)).collect()
}

fn full_marks(h: &mut RecordHandles) -> Vec<Mark> {
    let mut v = Vec::new();
    while let Ok(m) = h.marks.pop() {
        v.push(m);
    }
    v
}

#[test]
fn input_is_stamped_from_the_roll_with_continuous_runs() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let info = b.open(&Default::default()).unwrap();
    assert_eq!(info.input_channels, 3);
    assert_eq!(info.input_channel_names[0], "Fake In 1");
    assert_eq!(
        (info.input_latency_frames, info.output_latency_frames),
        (0, 0)
    );
    b.run_frames(1024);
    // Device channels 1 then 0 into the ring.
    let mut h = engine
        .start_recording(&request(vec![1, 0], 48_000))
        .unwrap();
    // Installed, but nothing plays yet: nothing is captured.
    b.run_frames(512);
    assert!(drain(&mut h).is_empty());
    engine.locate_frame(1000).unwrap();
    engine.play().unwrap();
    // Play is applied at the next block, absolute frame 1536.
    b.run_frames(2560);
    engine.stop_recording().unwrap();
    b.run_frames(256);
    assert!(h.status.closed());
    assert_eq!(h.status.run(), 2560);
    assert_eq!(h.status.delay(), Some(0));
    assert_eq!(h.status.lost_blocks(), 0);
    assert_eq!(marks(&mut h), vec![(0, 1000)]);
    let blocks = drain(&mut h);
    let mut next = 0u64;
    for (run, frames, s) in &blocks {
        assert_eq!(*run, next, "contiguous");
        for f in 0..*frames as usize {
            let abs = 1536 + run + f as u64;
            assert_eq!(s[2 * f], fake_signal(1, abs, SR));
            assert_eq!(s[2 * f + 1], fake_signal(0, abs, SR));
        }
        next += u64::from(*frames);
    }
    assert_eq!(next, 2560);
}

#[test]
fn every_loop_wrap_is_marked() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    engine.set_loop_range(Some((1000, 1700))).unwrap();
    engine.locate_frame(1000).unwrap();
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 12);
    let m = full_marks(&mut h);
    assert_eq!(
        m.iter().map(|m| (m.run, m.frame)).collect::<Vec<_>>(),
        vec![
            (0, 1000),
            (700, 1000),
            (1400, 1000),
            (2100, 1000),
            (2800, 1000)
        ]
    );
    // The roll is no wrap; every later one is.
    assert_eq!(
        m.iter().map(|m| m.wrap).collect::<Vec<_>>(),
        vec![false, true, true, true, true]
    );
    // MIDI-only recordings have no ring channels and still mark.
    engine.stop().unwrap();
    engine.stop_recording().unwrap();
    b.run_frames(256);
    let mut h = engine.start_recording(&request(vec![], 48_000)).unwrap();
    engine.locate_frame(1200).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 4);
    assert_eq!(marks(&mut h), vec![(0, 1200), (500, 1000)]);
    assert!(drain(&mut h).is_empty());
}

#[test]
fn loopback_input_is_the_output_later_and_reports_its_delay() {
    let sinks = OfflineSinks::new(10_000).with_tone();
    let (mut engine, mut b) = setup(FakeInput::Loopback(1000), 256, sinks);
    let info = b.open(&Default::default()).unwrap();
    assert_eq!(
        (info.output_latency_frames, info.input_latency_frames),
        (500, 500)
    );
    let mut h = engine.start_recording(&request(vec![0], 96_000)).unwrap();
    engine.play().unwrap();
    let mut out: Vec<f32> = Vec::new();
    for _ in 0..40 {
        b.block(256);
        out.extend_from_slice(b.output(256).0);
    }
    assert_eq!(h.status.delay(), Some(1000));
    let blocks = drain(&mut h);
    assert!(out.iter().any(|x| x.abs() > 0.001), "the tone plays");
    // Play applied at the first block: run r is absolute frame r.
    let mut checked = 0;
    for (run, frames, s) in &blocks {
        assert_eq!(s.len(), *frames as usize);
        for (f, x) in s.iter().enumerate() {
            let abs = (*run as usize) + f;
            let want = abs.checked_sub(1000).map_or(0.0, |k| out[k]);
            assert_eq!(*x, want, "frame {abs}");
            checked += 1;
        }
    }
    assert_eq!(checked, 40 * 256);
}

#[test]
fn a_full_ring_loses_whole_blocks_and_counts_them() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    // Room for one block of 256.
    let mut h = engine.start_recording(&request(vec![0, 1], 300)).unwrap();
    engine.play().unwrap();
    b.run_frames(256 * 5);
    assert!(h.status.lost_blocks() >= 3, "{}", h.status.lost_blocks());
    let first = drain(&mut h);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, 0);
    b.run_frames(256);
    let later = drain(&mut h);
    // The next block that fits shows the gap in its run.
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].0, 256 * 5);
}

#[test]
fn a_block_the_transport_skipped_is_captured_and_marked_after() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let mut h = engine.start_recording(&request(vec![0], 96_000)).unwrap();
    engine.locate_frame(500).unwrap();
    engine.play().unwrap();
    // 8192 frames is more than the renderer was prepared for: an xrun,
    // rendered as silence, the transport does not move.
    b.run_blocks([256, 8192, 256]);
    assert_eq!(h.status.run(), 256 + 8192 + 256);
    assert_eq!(marks(&mut h), vec![(0, 500), (256 + 8192, 756)]);
    let runs: Vec<u64> = drain(&mut h).iter().map(|b| b.0).collect();
    assert_eq!(runs, vec![0, 256, 256 + 8192]);
}

#[test]
fn a_schedule_swap_while_recording_resyncs() {
    let (doc, seq) = one_bar();
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let mut h = engine.start_recording(&request(vec![0], 48_000)).unwrap();
    engine.locate_frame(100).unwrap();
    engine.play().unwrap();
    b.run_frames(512);
    // An edit recompiles: the next block marks where the transport is.
    engine.load(&doc, seq).unwrap();
    b.run_frames(256);
    let m = full_marks(&mut h);
    assert_eq!(m.len(), 2, "{m:?}");
    assert_eq!((m[0].run, m[0].frame, m[0].wrap), (0, 100, false));
    assert_eq!((m[1].run, m[1].wrap), (512, false));
    // The swap maps the position through ticks (within a frame): the mark
    // says where the transport really is.
    assert!(m[1].frame.abs_diff(612) <= 1, "{m:?}");
}

#[test]
fn the_tap_reads_closed_when_its_renderer_goes() {
    let (mut engine, mut b) = setup(FakeInput::Signals, 256, OfflineSinks::new(10_000));
    let h = engine.start_recording(&request(vec![0], 4800)).unwrap();
    engine.play().unwrap();
    b.run_frames(512);
    assert!(!h.status.closed());
    drop(b.stop().unwrap());
    assert!(
        h.status.closed(),
        "an output that stopped ends the recording"
    );
    assert!(h.ring.closed());
}
VIZDAW_EOF
cat > crates/core-engine/tests/rt_alloc_record.rs <<'VIZDAW_EOF'
//! Recording never allocates on the render path (plan 5c Task 5; spec
//! §3, §12.3 "`rt-check` covers … the input ring"): the counting
//! allocator is armed only around `OfflineBackend::block`, while a record
//! tap captures fake input through loop wraps, a full ring, tap swaps and
//! an xrun.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn armed_block(b: &mut OfflineBackend, frames: u32) {
    rt_check::arm();
    b.block(frames);
    rt_check::disarm();
}

/// One test, so no other test thread allocates while the global counter
/// is compared.
#[test]
fn recording_never_allocates() {
    let mut bld = Builder::new();
    let synth = bld.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, lane) = bld.sequence("Bar", "Notes", Some(synth)).unwrap();
    bld.notes(seq, lane, &[(0, 60, 0x8000, Q), (2 * Q, 67, 0x8000, Q)])
        .unwrap();
    let doc = bld.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512).with_input(FakeInput::Loopback(1000));
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop_range(Some((0, 30_000))).unwrap();
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![0, 1, 2],
            range: Some((1000, 20_000)),
            ring_frames: 48_000,
        })
        .unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    let mut buf = Vec::with_capacity(1 << 20);
    for i in 0..3_000u32 {
        armed_block(&mut b, 1 + (i * 37) % 1024);
        // Drained (unarmed) except between blocks 1000 and 1500: the ring
        // fills and blocks are lost.
        if !(1000..1500).contains(&i) {
            while h.ring.pop(&mut buf).is_some() {}
            while h.marks.pop().is_ok() {}
        }
        if i % 100 == 0 {
            log.drain();
        }
        if i == 2000 {
            // A new tap replaces the old one while playing.
            h = engine
                .start_recording(&RecordRequest {
                    channels: vec![2],
                    range: None,
                    ring_frames: 4800,
                })
                .unwrap();
        }
    }
    assert!(h.status.run() > 0);
    assert_eq!(rt_check::count() - before, 0, "allocations while recording");
    // An xrun while recording, then the tap goes.
    armed_block(&mut b, 8192);
    engine.stop_recording().unwrap();
    armed_block(&mut b, 512);
    assert!(h.status.closed());
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations on xrun and stop"
    );
    drop(b.stop().unwrap());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test record_tap 2>&1 | tail -5
```
Expected: does not compile: `core_engine::record` and `FakeInput` do not exist.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/record.rs <<'VIZDAW_EOF'
//! The renderer's record tap (sub-project 5 spec §6.2–§6.4; plan 5c
//! Rulings 3–6). While a tap is installed and the transport plays, every
//! block of device input goes into the input ring stamped with its *run
//! frame*: frames counted from the first block that played, continuous
//! across loop wraps. *Marks* say which schedule frame the output played
//! at a run frame: one when the transport rolls and one at every wrap (and
//! after a block the transport did not advance through). The control side
//! (core-ffi) turns runs into timeline positions with the marks, the
//! device's input delay and the graph latency, so the real-time side does
//! no mapping at all. // RT where marked

use crate::midi1::Midi1Msg;
use core_media::record::{RingReader, RingWriter, input_ring};
use core_model::ids::LaneId;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

/// Marks a tap can hold until the control side reads them.
pub const MARK_CAPACITY: usize = 4096;
/// Recorded MIDI messages a tap can hold until the control side reads
/// them.
pub const MIDI_CAPACITY: usize = 16_384;

/// Output run frame `run` played schedule frame `frame`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    /// The run frame.
    pub run: u64,
    /// The schedule frame.
    pub frame: u64,
    /// A loop wrap: a new pass starts here (spec §6.4). The first mark is
    /// the roll; others are resyncs (after a block the transport did not
    /// advance through, or a schedule swap).
    pub wrap: bool,
}

/// A MIDI message an armed lane recorded, at the output run frame that
/// was being heard when it arrived (spec §6.3; the graph latency is not
/// yet taken off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedMidi {
    /// The lane.
    pub lane: LaneId,
    /// The output run frame (negative: before the transport rolled).
    pub run: i64,
    /// The message (after the lane's input chain).
    pub msg: Midi1Msg,
}

/// Where the control side reads a tap's marks.
pub type MarkReader = Consumer<Mark>;
/// Where the control side reads a tap's recorded MIDI.
pub type MidiReader = Consumer<RecordedMidi>;

/// What a tap shares with the control side.
#[derive(Debug)]
pub struct RecordStatus {
    run: AtomicU64,
    delay: AtomicI64,
    closed: AtomicBool,
    lost_blocks: AtomicU64,
    lost_marks: AtomicU64,
    lost_midi: AtomicU64,
    /// The graph latency `L` when recording started, in frames.
    pub latency: u64,
}

impl RecordStatus {
    /// Run frames played (or lost) so far.
    pub fn run(&self) -> u64 {
        self.run.load(Ordering::Acquire)
    }

    /// The device's input delay at the first captured block (spec §6.3),
    /// once one was captured.
    pub fn delay(&self) -> Option<i64> {
        let d = self.delay.load(Ordering::Acquire);
        (d != i64::MIN).then_some(d)
    }

    /// The tap is gone: removed, or dropped with its renderer (an output
    /// that stopped).
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Input blocks that did not fit into the ring.
    pub fn lost_blocks(&self) -> u64 {
        self.lost_blocks.load(Ordering::Relaxed)
    }

    /// Marks that did not fit (the pass cannot be placed exactly).
    pub fn lost_marks(&self) -> u64 {
        self.lost_marks.load(Ordering::Relaxed)
    }

    /// Recorded MIDI messages that did not fit.
    pub fn lost_midi(&self) -> u64 {
        self.lost_midi.load(Ordering::Relaxed)
    }
}

/// What to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRequest {
    /// The device input channels the ring carries, in ring order (empty:
    /// MIDI only).
    pub channels: Vec<u16>,
    /// The record range in schedule frames, for auto-input monitoring
    /// (`None`: from the start until Stop).
    pub range: Option<(u64, u64)>,
    /// Ring length in frames.
    pub ring_frames: usize,
}

/// The control side of a tap.
#[derive(Debug)]
pub struct RecordHandles {
    /// The input ring's reader (for the writer thread).
    pub ring: RingReader,
    /// Marks, oldest first.
    pub marks: MarkReader,
    /// Recorded MIDI, oldest first.
    pub midi: MidiReader,
    /// Counters and the run.
    pub status: Arc<RecordStatus>,
}

/// The real-time side of a recording, owned by the renderer.
#[derive(Debug)]
pub struct RecordTap {
    ring: RingWriter,
    channels: Box<[u16]>,
    marks: Producer<Mark>,
    midi: Producer<RecordedMidi>,
    status: Arc<RecordStatus>,
    range: Option<(u64, u64)>,
    rolling: bool,
    run: u64,
    remark: bool,
}

impl Drop for RecordTap {
    fn drop(&mut self) {
        self.status.closed.store(true, Ordering::Release);
    }
}

/// A tap and its control side. Allocates; call off the real-time thread.
pub fn tap(req: &RecordRequest, latency: u64) -> (RecordTap, RecordHandles) {
    let (ring, reader) = input_ring(req.channels.len().max(1), req.ring_frames.max(1));
    let (mark_tx, mark_rx) = RingBuffer::new(MARK_CAPACITY);
    let (midi_tx, midi_rx) = RingBuffer::new(MIDI_CAPACITY);
    let status = Arc::new(RecordStatus {
        run: AtomicU64::new(0),
        delay: AtomicI64::new(i64::MIN),
        closed: AtomicBool::new(false),
        lost_blocks: AtomicU64::new(0),
        lost_marks: AtomicU64::new(0),
        lost_midi: AtomicU64::new(0),
        latency,
    });
    (
        RecordTap {
            ring,
            channels: req.channels.clone().into_boxed_slice(),
            marks: mark_tx,
            midi: midi_tx,
            status: Arc::clone(&status),
            range: req.range,
            rolling: false,
            run: 0,
            remark: false,
        },
        RecordHandles {
            ring: reader,
            marks: mark_rx,
            midi: midi_rx,
            status,
        },
    )
}

impl RecordTap {
    /// Whether the transport rolled since the tap was installed.
    pub fn rolling(&self) -> bool {
        self.rolling
    }

    /// The run frame of the block about to render.
    pub fn run(&self) -> u64 {
        self.run
    }

    /// The tap is removed: the control side reads it closed now, before
    /// the retire thread frees it. // RT
    pub fn close(&self) {
        self.ring.close();
        self.status.closed.store(true, Ordering::Release);
    }

    /// Whether `frame` lies in the record range. // RT
    pub fn in_range(&self, frame: u64) -> bool {
        self.range.is_none_or(|(a, b)| frame >= a && frame < b)
    }

    fn mark(&mut self, run: u64, frame: u64, wrap: bool) {
        if self.marks.push(Mark { run, frame, wrap }).is_err() {
            self.status.lost_marks.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// At a block's start while playing: rolls (run 0 plays `frame`), or
    /// marks where the transport is after a block it did not advance
    /// through. // RT
    pub fn begin_block(&mut self, frame: u64, delay: i64) {
        if !self.rolling {
            self.rolling = true;
            self.run = 0;
            self.status.delay.store(delay, Ordering::Release);
            self.mark(0, frame, false);
        } else if self.remark {
            self.mark(self.run, frame, false);
        }
        self.remark = false;
    }

    /// A loop wrap at block offset `offset` continues at schedule frame
    /// `to`. // RT
    pub fn wrap(&mut self, offset: u32, to: u64) {
        if self.rolling {
            let run = self.run.saturating_add(u64::from(offset));
            self.mark(run, to, true);
        }
    }

    /// The schedule changed under the transport (an edit while recording
    /// recompiled it): the next block marks where it is. // RT
    pub fn resync(&mut self) {
        if self.rolling {
            self.remark = true;
        }
    }

    /// Records an armed lane's message heard at block offset `offset`
    /// (negative: before the block). // RT
    pub fn midi(&mut self, lane: LaneId, offset: i64, msg: Midi1Msg) {
        if !self.rolling {
            return;
        }
        let run = i64::try_from(self.run)
            .unwrap_or(i64::MAX)
            .saturating_add(offset);
        if self.midi.push(RecordedMidi { lane, run, msg }).is_err() {
            self.status.lost_midi.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// At a block's end: pushes its input (device channel `c` from
    /// `input[c]`) and advances the run. `advanced` is false when the
    /// transport did not play the whole block (an xrun or a missed
    /// deadline): the next block marks where it is. // RT
    pub fn end_block(&mut self, input: &[&[f32]], frames: u32, advanced: bool) {
        if !self.rolling {
            return;
        }
        let channels = &self.channels;
        if !channels.is_empty() {
            let pushed = self.ring.push(self.run, frames, |c, f| {
                channels
                    .get(c)
                    .and_then(|d| input.get(usize::from(*d)))
                    .and_then(|ch| ch.get(f))
                    .copied()
                    .unwrap_or(0.0)
            });
            if !pushed {
                self.status.lost_blocks.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.run = self.run.saturating_add(u64::from(frames));
        self.ring.set_end(self.run);
        self.status.run.store(self.run, Ordering::Release);
        if !advanced {
            self.remark = true;
        }
    }
}
VIZDAW_EOF
```

In `crates/core-engine/src/backend.rs`:

Find:
```rust

/// Largest block any node or renderer is prepared for.
pub const MAX_FRAMES: u32 = 4096;

/// Timing of one block: its first frame reaches the output at
/// `host_time_ns`.
```
Replace with:
```rust

/// Largest block any node or renderer is prepared for.
pub const MAX_FRAMES: u32 = 4096;
/// The most device input channels the renderer takes (spec §6.1).
pub const MAX_INPUT_CHANNELS: usize = 64;

/// One block of device input (spec §6.1, §6.3), planar.
#[derive(Debug, Clone, Copy)]
pub struct InputBlock<'a> {
    /// Device input channel `c` is `channels[c]` (at least the block's
    /// frames long); empty while input is closed.
    pub channels: &'a [&'a [f32]],
    /// How much earlier than the output block's first frame is heard its
    /// input's first frame was captured, in frames: the output and input
    /// buffers' sample-time difference plus the output and input
    /// latencies, as the device reports them (spec §6.3).
    pub delay_frames: i64,
}

impl InputBlock<'static> {
    /// No input.
    pub const NONE: InputBlock<'static> = InputBlock {
        channels: &[],
        delay_frames: 0,
    };
}

/// Timing of one block: its first frame reaches the output at
/// `host_time_ns`.
```

Find:
```rust
    /// Requested nominal sample rate (the project rate, 5a); `None` keeps
    /// the device's. `StreamInfo::sample_rate` says what it runs at.
    pub sample_rate: Option<f64>,
}

/// What a backend opened.
```
Replace with:
```rust
    /// Requested nominal sample rate (the project rate, 5a); `None` keeps
    /// the device's. `StreamInfo::sample_rate` says what it runs at.
    pub sample_rate: Option<f64>,
    /// Open input too (spec §6.1: only while a lane is armed or
    /// monitored).
    pub input_enabled: bool,
    /// The input device by name; `None`: the output device (one duplex
    /// stream). Another device is joined to the output in a private
    /// aggregate device.
    pub input_device: Option<String>,
}

/// What a backend opened.
```

Find:
```rust
    pub buffer_frames: u32,
    /// Output channel count.
    pub channels: u32,
}

/// Something that happened to the device, reported by
```
Replace with:
```rust
    pub buffer_frames: u32,
    /// Output channel count.
    pub channels: u32,
    /// Input channels (0 when input is closed).
    pub input_channels: u32,
    /// The input channels' names, as the device gives them.
    pub input_channel_names: Vec<String>,
    /// Input latency in frames: device + stream latency + safety offset
    /// (spec §6.1).
    pub input_latency_frames: u32,
    /// Output latency in frames, the same way.
    pub output_latency_frames: u32,
}

/// Something that happened to the device, reported by
```

In `crates/core-engine/src/render.rs`:

Find:
```rust
//! The real-time renderer and the state it publishes.

use crate::backend::{BlockInfo, BlockTime, MidiOut, StereoBuf};
use crate::graph::exec::{Exec, RunResult, SinkTable};
use crate::graph::{Graph, MAX_INSERTS, MAX_NODES};
use crate::midi1::Midi1Msg;
use crate::notes::ActiveNotes;
use crate::pool::PoolShared;
use crate::schedule::automation::StripParam;
use crate::schedule::{AutoDest, MAX_AUTO_LANES, MAX_TARGETS, MIN_RUN, Schedule, TargetIdx};
use crate::transport::{PlayState, Step, Transport};
```
Replace with:
```rust
//! The real-time renderer and the state it publishes.

use crate::backend::{BlockInfo, BlockTime, InputBlock, MidiOut, StereoBuf};
use crate::graph::exec::{Exec, RunResult, SinkTable};
use crate::graph::{Graph, MAX_INSERTS, MAX_NODES};
use crate::midi1::Midi1Msg;
use crate::notes::ActiveNotes;
use crate::pool::PoolShared;
use crate::record::RecordTap;
use crate::schedule::automation::StripParam;
use crate::schedule::{AutoDest, MAX_AUTO_LANES, MAX_TARGETS, MIN_RUN, Schedule, TargetIdx};
use crate::transport::{PlayState, Step, Transport};
```

Find:
```rust
    /// the end do not, so a note dragged in an editor keeps sounding until
    /// the editor ends it (4b Plan Ruling 1).
    Audition(TargetIdx, Midi1Msg),
}

/// Something the renderer hands back for freeing off the audio thread.
```
Replace with:
```rust
    /// the end do not, so a note dragged in an editor keeps sounding until
    /// the editor ends it (4b Plan Ruling 1).
    Audition(TargetIdx, Midi1Msg),
    /// Install a record tap, or remove the one installed (it goes back
    /// through the retire ring; dropping it tells the control side the
    /// recording's input ended). Spec §6.2.
    Record(Option<Box<RecordTap>>),
}

/// Something the renderer hands back for freeing off the audio thread.
```

Find:
```rust
    Graph(Box<Graph>),
    /// A removed sink.
    Sink(Sink),
}

/// Capacity of the command ring.
```
Replace with:
```rust
    Graph(Box<Graph>),
    /// A removed sink.
    Sink(Sink),
    /// A removed record tap.
    Tap(Box<RecordTap>),
}

/// Capacity of the command ring.
```

Find:
```rust
    pending_skip: u64,
    /// Per automation lane, the plugin value last sent (NaN: none yet).
    auto_last: Box<[f64; MAX_AUTO_LANES]>,
}

impl std::fmt::Debug for Renderer {
```
Replace with:
```rust
    pending_skip: u64,
    /// Per automation lane, the plugin value last sent (NaN: none yet).
    auto_last: Box<[f64; MAX_AUTO_LANES]>,
    /// The recording's tap (spec §6.2), while one is installed.
    tap: Option<Box<RecordTap>>,
}

impl std::fmt::Debug for Renderer {
```

Find:
```rust
            latency: 0,
            pending_skip: 0,
            auto_last: Box::new([f64::NAN; MAX_AUTO_LANES]),
        };
        r.prepare(cfg.max_frames);
        r
```
Replace with:
```rust
            latency: 0,
            pending_skip: 0,
            auto_last: Box::new([f64::NAN; MAX_AUTO_LANES]),
            tap: None,
        };
        r.prepare(cfg.max_frames);
        r
```

Find:
```rust
    /// Renders one block of `frames` frames into planar `out` channels.
    /// Channels 0–1 get master, the rest zeros. // RT
    pub fn process(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        #[cfg(feature = "rt-check")]
        crate::rt_check::arm();
        self.process_inner(out, frames, time);
        #[cfg(feature = "rt-check")]
        crate::rt_check::disarm();
    }

    fn latency_ns(&self) -> i64 {
        (self.latency as f64 * 1e9 / self.cfg.sample_rate.max(1.0)).round() as i64
    }

    fn process_inner(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        let started = Instant::now();
        let block_s = f64::from(frames) / self.cfg.sample_rate.max(1.0);
        let deadline = self
```
Replace with:
```rust
    /// Renders one block of `frames` frames into planar `out` channels.
    /// Channels 0–1 get master, the rest zeros. // RT
    pub fn process(&mut self, out: &mut [&mut [f32]], frames: u32, time: BlockTime) {
        self.process_input(out, frames, time, InputBlock::NONE);
    }

    /// [`Renderer::process`] with this block's device input (spec §6.1):
    /// a record tap takes it, and monitored lanes hear it. // RT
    pub fn process_input(
        &mut self,
        out: &mut [&mut [f32]],
        frames: u32,
        time: BlockTime,
        input: InputBlock<'_>,
    ) {
        #[cfg(feature = "rt-check")]
        crate::rt_check::arm();
        self.process_inner(out, frames, time, &input);
        #[cfg(feature = "rt-check")]
        crate::rt_check::disarm();
    }

    /// The tap's end of a block (see [`RecordTap::end_block`]). // RT
    fn end_tap(&mut self, input: &InputBlock<'_>, frames: u32, advanced: bool) {
        if let Some(t) = self.tap.as_deref_mut() {
            t.end_block(input.channels, frames, advanced);
        }
    }

    fn latency_ns(&self) -> i64 {
        (self.latency as f64 * 1e9 / self.cfg.sample_rate.max(1.0)).round() as i64
    }

    fn process_inner(
        &mut self,
        out: &mut [&mut [f32]],
        frames: u32,
        time: BlockTime,
        input: &InputBlock<'_>,
    ) {
        let started = Instant::now();
        let block_s = f64::from(frames) / self.cfg.sample_rate.max(1.0);
        let deadline = self
```

Find:
```rust
            self.pending_skip = self.pending_skip.saturating_add(u64::from(frames));
            // The block was lost to the graph: report it as full load.
            self.shared.set_load(1.0);
            return;
        }
        // No run is in flight: the last run's ramps have arrived (Plan 4d
```
Replace with:
```rust
            self.pending_skip = self.pending_skip.saturating_add(u64::from(frames));
            // The block was lost to the graph: report it as full load.
            self.shared.set_load(1.0);
            // Its input is still recorded; the transport did not move.
            self.end_tap(input, frames, false);
            return;
        }
        // No run is in flight: the last run's ramps have arrived (Plan 4d
```

Find:
```rust
        if frames > self.cfg.max_frames || out.iter().any(|c| c.len() < n) {
            skip_sinks(&mut self.sinks, u64::from(frames), 0);
            self.shared.count_xrun();
            return;
        }
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        // No run is in flight (`try_finish` succeeded above): the streamer
        // may count a new epoch (5a Ruling 9) and plan around the playhead
        // as this block's commands left it (a Locate's new position, never
        // the stale one: commands are applied only here).
        self.reader.begin_block(self.transport.pos);
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let ns_per_frame = 1e9 / self.cfg.sample_rate.max(1.0);
        let mut missed = false;
```
Replace with:
```rust
        if frames > self.cfg.max_frames || out.iter().any(|c| c.len() < n) {
            skip_sinks(&mut self.sinks, u64::from(frames), 0);
            self.shared.count_xrun();
            self.end_tap(input, frames, false);
            return;
        }
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        if self.transport.state == PlayState::Playing {
            let at = self
                .transport
                .play_pos(length)
                .unwrap_or(self.transport.pos);
            if let Some(t) = self.tap.as_deref_mut() {
                t.begin_block(at, input.delay_frames);
            }
        }
        // No run is in flight (`try_finish` succeeded above): the streamer
        // may count a new epoch (5a Ruling 9) and plan around the playhead
        // as this block's commands left it (a Locate's new position, never
        // the stale one: commands are applied only here).
        self.reader.begin_block(self.transport.pos);
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let ns_per_frame = 1e9 / self.cfg.sample_rate.max(1.0);
        let mut missed = false;
```

Find:
```rust
                        }
                    }
                    Step::Wrap { offset, to } => {
                        release_all(&mut self.notes, &mut o, offset);
                        if let Some(s) = self.schedule.as_deref() {
                            reset_controllers(s, &mut o, offset);
```
Replace with:
```rust
                        }
                    }
                    Step::Wrap { offset, to } => {
                        if let Some(t) = self.tap.as_deref_mut() {
                            t.wrap(offset, to);
                        }
                        release_all(&mut self.notes, &mut o, offset);
                        if let Some(s) = self.schedule.as_deref() {
                            reset_controllers(s, &mut o, offset);
```

Find:
```rust
            // stragglers are done.
            self.pending_skip = self.pending_skip.saturating_add(u64::from(frames - offset));
        }
        self.publish();
        if block_s > 0.0 {
            let load = (started.elapsed().as_secs_f64() / block_s) as f32;
```
Replace with:
```rust
            // stragglers are done.
            self.pending_skip = self.pending_skip.saturating_add(u64::from(frames - offset));
        }
        self.end_tap(input, frames, !missed);
        self.publish();
        if block_s > 0.0 {
            let load = (started.elapsed().as_secs_f64() / block_s) as f32;
```

Find:
```rust
                }
                Ok(EngineCmd::SetGraph(_)) => self.graph.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::RemoveSink(_)) => self.retire.slots() == 0,
                Ok(_) => false,
                Err(_) => break,
            };
```
Replace with:
```rust
                }
                Ok(EngineCmd::SetGraph(_)) => self.graph.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::RemoveSink(_)) => self.retire.slots() == 0,
                Ok(EngineCmd::Record(_)) => self.tap.is_some() && self.retire.slots() == 0,
                Ok(_) => false,
                Err(_) => break,
            };
```

Find:
```rust
                }
                self.cursor = new.cursor_at(self.transport.pos);
                self.schedule = Some(new);
                self.auto_last.fill(f64::NAN);
                if self.transport.state == PlayState::Playing {
                    self.forget_unautomated();
```
Replace with:
```rust
                }
                self.cursor = new.cursor_at(self.transport.pos);
                self.schedule = Some(new);
                if let Some(t) = self.tap.as_deref_mut() {
                    t.resync();
                }
                self.auto_last.fill(f64::NAN);
                if self.transport.state == PlayState::Playing {
                    self.forget_unautomated();
```

Find:
```rust
                };
                o.send(target, 0, msg.as_slice());
                self.audition.observe(target, msg.as_slice());
            }
            EngineCmd::AddSink(idx, sink) => {
                if let Some(slot) = self.sinks.get_mut(usize::from(idx.0)) {
```
Replace with:
```rust
                };
                o.send(target, 0, msg.as_slice());
                self.audition.observe(target, msg.as_slice());
            }
            EngineCmd::Record(new) => {
                if let Some(old) = std::mem::replace(&mut self.tap, new) {
                    old.close();
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Tap(old)) {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
            }
            EngineCmd::AddSink(idx, sink) => {
                if let Some(slot) = self.sinks.get_mut(usize::from(idx.0)) {
```

In `crates/core-engine/src/engine.rs`:

Find:
```rust
    pub fn plugin_faults(&self) -> Vec<PluginInstanceId> {
        self.shared.take_plugin_faults()
    }
}

impl Drop for Engine {
```
Replace with:
```rust
    pub fn plugin_faults(&self) -> Vec<PluginInstanceId> {
        self.shared.take_plugin_faults()
    }

    /// Installs a record tap (spec §6.2): from the next block that plays,
    /// the renderer captures input and MIDI for it. Replaces a tap already
    /// installed. Returns the control side.
    pub fn start_recording(
        &mut self,
        req: &crate::record::RecordRequest,
    ) -> Result<crate::record::RecordHandles, EngineError> {
        let (tap, handles) = crate::record::tap(req, self.latency);
        self.push(EngineCmd::Record(Some(Box::new(tap))))?;
        Ok(handles)
    }

    /// Removes the record tap: the block that applies this captures no
    /// more, and the tap's status reads closed.
    pub fn stop_recording(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Record(None))
    }
}

impl Drop for Engine {
```

In `crates/core-engine/src/offline.rs`:

Find:
```rust
//! chosen block sizes, and records every event its sinks receive.

use crate::backend::{
    AudioBackend, AudioConfig, BackendEvent, BlockTime, InstrumentNode, MidiOut, MidiPortInfo,
    StereoBuf, StreamInfo,
};
use crate::engine::SinkFactory;
use crate::error::BackendError;
```
Replace with:
```rust
//! chosen block sizes, and records every event its sinks receive.

use crate::backend::{
    AudioBackend, AudioConfig, BackendEvent, BlockTime, InputBlock, InstrumentNode, MidiOut,
    MidiPortInfo, StereoBuf, StreamInfo,
};
use crate::engine::SinkFactory;
use crate::error::BackendError;
```

Find:
```rust
    }
}

/// Drives a renderer without hardware. Block `n` starts at host time
/// `frames_so_far × 1e9 / sample_rate` ns.
#[derive(Debug)]
pub struct OfflineBackend {
    sample_rate: f64,
```
Replace with:
```rust
    }
}

/// A fake audio input (sub-project 5 spec §12.1): no device and no
/// microphone permission, for tests and the e2e.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeInput {
    /// Channel 1 carries a 440 Hz sine at −12 dBFS, channel 2 an impulse
    /// every 0.5 s (from frame 0), channel 3 silence.
    Signals,
    /// Channels 1 and 2 are the output's left and right this many frames
    /// later (at least one block, plan 5c Ruling 8); channel 3 silence.
    /// The device reports half the delay as output latency and the rest
    /// as input latency, as a real loopback cable would show.
    Loopback(u32),
}

/// Channels of the fake input.
pub const FAKE_INPUT_CHANNELS: usize = 3;
/// The fake sine's frequency.
pub const FAKE_SINE_HZ: f64 = 440.0;
/// The fake sine's amplitude: −12 dBFS.
pub const FAKE_SINE_AMPLITUDE: f32 = 0.251_188_64;

/// Sample `frame` of fake input channel `c` for [`FakeInput::Signals`].
pub fn fake_signal(c: usize, frame: u64, sample_rate: f64) -> f32 {
    match c {
        0 => {
            let t = frame as f64 / sample_rate.max(1.0);
            (FAKE_SINE_AMPLITUDE as f64 * (std::f64::consts::TAU * FAKE_SINE_HZ * t).sin()) as f32
        }
        1 => {
            let every = (sample_rate / 2.0).round().max(1.0) as u64;
            if frame % every == 0 { 1.0 } else { 0.0 }
        }
        _ => 0.0,
    }
}

/// The fake input's buffers.
#[derive(Debug)]
struct Fake {
    kind: FakeInput,
    /// The loopback delay in frames (0 for signals).
    delay: u32,
    chans: [Vec<f32>; FAKE_INPUT_CHANNELS],
    /// The output's left and right, by absolute frame modulo the length.
    history: [Vec<f32>; 2],
}

/// Drives a renderer without hardware. Block `n` starts at host time
/// `(frames_so_far + output latency) × 1e9 / sample_rate` ns: the time
/// its first frame is heard (the fake device's output latency is 0
/// unless it loops back).
#[derive(Debug)]
pub struct OfflineBackend {
    sample_rate: f64,
```

Find:
```rust
    frames: u64,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl OfflineBackend {
```
Replace with:
```rust
    frames: u64,
    left: Vec<f32>,
    right: Vec<f32>,
    fake: Option<Fake>,
}

impl OfflineBackend {
```

Find:
```rust
            frames: 0,
            left: Vec::new(),
            right: Vec::new(),
        }
    }
```
Replace with:
```rust
            frames: 0,
            left: Vec::new(),
            right: Vec::new(),
            fake: None,
        }
    }

    /// The backend with a fake input (spec §12.1).
    pub fn with_input(mut self, kind: FakeInput) -> OfflineBackend {
        let delay = match kind {
            FakeInput::Signals => 0,
            FakeInput::Loopback(n) => n.max(self.block),
        };
        self.fake = Some(Fake {
            kind,
            delay,
            chans: Default::default(),
            history: Default::default(),
        });
        let n = self.left.len() as u32;
        self.reserve(n.max(self.block));
        self
    }

    /// `(output, input)` latency the fake device reports, in frames.
    fn latencies(&self) -> (u32, u32) {
        match &self.fake {
            Some(f) => (f.delay / 2, f.delay - f.delay / 2),
            None => (0, 0),
        }
    }
```

Find:
```rust
            self.left.resize(n, 0.0);
            self.right.resize(n, 0.0);
        }
    }

    /// Renders one block of `frames` frames; returns the peak absolute
    /// sample. Call [`OfflineBackend::reserve`] first for blocks larger than
    /// any seen before.
    pub fn block(&mut self, frames: u32) -> f32 {
        let Some(r) = self.renderer.as_mut() else {
            return 0.0;
        };
        let n = (frames as usize).min(self.left.len());
        let time = BlockTime {
            host_time_ns: (self.frames as f64 * 1e9 / self.sample_rate).round() as u64,
            sample_rate: self.sample_rate,
        };
        let (Some(l), Some(rt)) = (self.left.get_mut(..n), self.right.get_mut(..n)) else {
            return 0.0;
        };
        let mut chans: [&mut [f32]; 2] = [l, rt];
        r.process(&mut chans, frames, time);
        self.frames += u64::from(frames);
        chans
            .iter()
```
Replace with:
```rust
            self.left.resize(n, 0.0);
            self.right.resize(n, 0.0);
        }
        let n = self.left.len();
        if let Some(f) = self.fake.as_mut() {
            for c in f.chans.iter_mut() {
                if c.len() < n {
                    c.resize(n, 0.0);
                }
            }
            let h = f.delay as usize + n;
            for c in f.history.iter_mut() {
                if c.len() < h {
                    // Resizing loses the history: only before playing.
                    *c = vec![0.0; h];
                }
            }
        }
    }

    /// Renders one block of `frames` frames; returns the peak absolute
    /// sample. Call [`OfflineBackend::reserve`] first for blocks larger than
    /// any seen before.
    pub fn block(&mut self, frames: u32) -> f32 {
        let (out_latency, _) = self.latencies();
        let Some(r) = self.renderer.as_mut() else {
            return 0.0;
        };
        let n = (frames as usize).min(self.left.len());
        let time = BlockTime {
            host_time_ns: ((self.frames + u64::from(out_latency)) as f64 * 1e9 / self.sample_rate)
                .round() as u64,
            sample_rate: self.sample_rate,
        };
        let abs = self.frames;
        let rate = self.sample_rate;
        if let Some(f) = self.fake.as_mut() {
            fill_fake(f, abs, n, rate);
        }
        let (Some(l), Some(rt)) = (self.left.get_mut(..n), self.right.get_mut(..n)) else {
            return 0.0;
        };
        let mut chans: [&mut [f32]; 2] = [l, rt];
        match &self.fake {
            Some(f) => {
                let ins: [&[f32]; FAKE_INPUT_CHANNELS] = [
                    f.chans[0].get(..n).unwrap_or(&[]),
                    f.chans[1].get(..n).unwrap_or(&[]),
                    f.chans[2].get(..n).unwrap_or(&[]),
                ];
                let input = InputBlock {
                    channels: &ins,
                    delay_frames: i64::from(f.delay),
                };
                r.process_input(&mut chans, frames, time, input);
            }
            None => r.process(&mut chans, frames, time),
        }
        if let Some(f) = self.fake.as_mut() {
            if f.kind != FakeInput::Signals {
                for (h, src) in f.history.iter_mut().zip([&*chans[0], &*chans[1]]) {
                    let len = h.len().max(1);
                    for (k, x) in src.iter().enumerate() {
                        if let Some(d) = h.get_mut((abs as usize + k) % len) {
                            *d = *x;
                        }
                    }
                }
            }
        }
        self.frames += u64::from(frames);
        chans
            .iter()
```

Find:
```rust

impl AudioBackend for OfflineBackend {
    fn open(&mut self, _cfg: &AudioConfig) -> Result<StreamInfo, BackendError> {
        Ok(StreamInfo {
            device_name: "offline".into(),
            sample_rate: self.sample_rate,
            buffer_frames: self.block,
            channels: u32::try_from(self.channels).unwrap_or(2),
        })
    }
```
Replace with:
```rust

impl AudioBackend for OfflineBackend {
    fn open(&mut self, _cfg: &AudioConfig) -> Result<StreamInfo, BackendError> {
        let (output_latency_frames, input_latency_frames) = self.latencies();
        let inputs = if self.fake.is_some() {
            FAKE_INPUT_CHANNELS
        } else {
            0
        };
        Ok(StreamInfo {
            device_name: "offline".into(),
            sample_rate: self.sample_rate,
            buffer_frames: self.block,
            channels: u32::try_from(self.channels).unwrap_or(2),
            input_channels: inputs as u32,
            input_channel_names: (1..=inputs).map(|k| format!("Fake In {k}")).collect(),
            input_latency_frames,
            output_latency_frames,
        })
    }
```

Find:
```rust
        Vec::new()
    }
}
```
Replace with:
```rust
        Vec::new()
    }
}

/// Fills the fake input's channels for `n` frames from absolute frame
/// `abs`. // RT
fn fill_fake(f: &mut Fake, abs: u64, n: usize, rate: f64) {
    let [c0, c1, c2] = &mut f.chans;
    let (Some(c0), Some(c1), Some(c2)) = (c0.get_mut(..n), c1.get_mut(..n), c2.get_mut(..n)) else {
        return;
    };
    c2.fill(0.0);
    match f.kind {
        FakeInput::Signals => {
            for k in 0..n {
                let at = abs + k as u64;
                if let (Some(a), Some(b)) = (c0.get_mut(k), c1.get_mut(k)) {
                    *a = fake_signal(0, at, rate);
                    *b = fake_signal(1, at, rate);
                }
            }
        }
        FakeInput::Loopback(_) => {
            let delay = u64::from(f.delay);
            for (c, h) in [c0, c1].into_iter().zip(f.history.iter()) {
                let len = h.len().max(1) as u64;
                for (k, x) in c.iter_mut().enumerate() {
                    let at = abs + k as u64;
                    // Frames the output has not played yet read 0 (a block
                    // longer than the delay).
                    *x = match at.checked_sub(delay) {
                        Some(src) if src < abs => {
                            h.get((src % len) as usize).copied().unwrap_or(0.0)
                        }
                        _ => 0.0,
                    };
                }
            }
        }
    }
}
```

In `crates/core-engine/src/lib.rs`:

Find:
```rust
pub mod notes;
pub mod offline;
pub mod pool;
pub mod render;
pub mod rt_check;
pub mod schedule;
```
Replace with:
```rust
pub mod notes;
pub mod offline;
pub mod pool;
pub mod record;
pub mod render;
pub mod rt_check;
pub mod schedule;
```

In `crates/platform-macos/src/hal.rs`:

Find:
```rust
            sample_rate,
            buffer_frames,
            channels,
        };
        self.device = Some(device);
        self.info = Some(info.clone());
```
Replace with:
```rust
            sample_rate,
            buffer_frames,
            channels,
            // Input and the latencies come with plan 5c Task 10.
            input_channels: 0,
            input_channel_names: Vec::new(),
            input_latency_frames: 0,
            output_latency_frames: 0,
        };
        self.device = Some(device);
        self.info = Some(info.clone());
```

In `crates/vizdaw-play/src/live.rs`:

Find:
```rust
        device: args.device.clone(),
        buffer_frames: args.buffer,
        sample_rate: None,
    };
    let info = backend
        .open(&cfg)
```
Replace with:
```rust
        device: args.device.clone(),
        buffer_frames: args.buffer,
        sample_rate: None,
        ..AudioConfig::default()
    };
    let info = backend
        .open(&cfg)
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test record_tap 2>&1 | tail -3
cargo test -p core-engine --features rt-check --test rt_alloc_record 2>&1 | tail -3
```
Expected: `record_tap` 7 passed (Review Focus 2 and 4: `the_tap_reads_closed_when_its_renderer_goes`, `a_block_the_transport_skipped_is_captured_and_marked_after`, `a_schedule_swap_while_recording_resyncs`); `rt_alloc_record` 1 passed (`recording_never_allocates`).

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep -E "^test result|FAILED|panicked"
cargo test -p core-engine 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p platform-macos 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p vizdaw-play 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/record.rs crates/core-engine/src/backend.rs crates/core-engine/src/render.rs crates/core-engine/src/engine.rs crates/core-engine/src/offline.rs crates/core-engine/src/lib.rs crates/platform-macos/src/hal.rs crates/vizdaw-play/src/live.rs crates/core-engine/tests/record_tap.rs crates/core-engine/tests/rt_alloc_record.rs
git commit -F - <<'VIZDAW_MSG'
core-engine: the record tap and the fake input

The renderer takes the device's input; a record tap counts run frames,
pushes armed channels into the input ring and marks the roll, every
loop wrap and every resync (xruns, busy blocks, schedule swaps), in
real time without allocating. The offline device gains the fake input:
a sine, an impulse train, or the output looped back after n frames.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 6: `core-engine` — MIDI input, thru and monitoring

**Files:**
- Create: `crates/core-engine/src/input.rs`
- Modify: `crates/core-engine/src/render.rs` (`live_midi`, `copy_monitored`, `EngineCmd::SetInputs`)
- Modify: `crates/core-engine/src/graph/regions.rs` (`InputCopy`, `RegionRun::with_monitor`)
- Modify: `crates/core-engine/src/engine.rs` (`set_inputs`, `inputs`, `midi_sender`)
- Modify: `crates/core-engine/src/lib.rs`
- Test: `crates/core-engine/tests/live_input.rs`; extend `crates/core-engine/tests/rt_alloc_record.rs`

**Interfaces:**
- Consumes: Task 5's tap; 4c's destination interning; 5a's `Regions` node.
- Produces:
  - `input::{port_key, MidiIn { port, host_ns, msg }, channel_message, MidiSender (send, dropped), InputChain, MidiInputSpec, MonitorRoute (heard), InputSpec, MidiRoute, InputRoutes, on_channel}`; `MIDI_IN_CAPACITY` 4096, `MIDI_IN_PER_BLOCK` 1024, `MAX_MONITOR_ROUTES` 64;
  - `Engine::set_inputs(InputSpec)` (sent only when it changed), `Engine::inputs()`, `Engine::midi_sender() -> MidiSender`.

Rulings 8, 9 and 10. Input MIDI is matched to listening lanes, passed through the (empty) input chain, played thru on the lane's destination and recorded with the run heard when it arrived; monitored audio is mixed into the lane's path.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/live_input.rs <<'VIZDAW_EOF'
//! Live input (plan 5c Task 6; sub-project 5 spec §6.3, §6.6, §6.8's
//! hook): MIDI a lane listens to plays through to its destination at once
//! (stopped or playing) and, while recording an armed lane, is recorded at
//! the run frame heard when it arrived; re-routing ends thru notes; and an
//! audio lane's input is heard through its path by the monitor mode.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{
    InputSpec, MidiIn, MidiInputSpec, MonitorRoute, channel_message, port_key,
};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::{RecordRequest, RecordedMidi};
use core_model::command::{Command, LaneCmd, RoutingCmd};
use core_model::document::Document;
use core_model::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId};
use core_model::lane::MonitorMode;

const SR: f64 = 48_000.0;

struct Doc {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    synth: DestId,
    path: OutputPathId,
}

/// A bar with one note on lane "Notes"; lane "Keys" plays on the synth
/// (channel 3) and has no events; lane "Mic" plays on output "Audio 1".
fn doc() -> Doc {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 3).unwrap();
    let (seq, notes) = b.sequence("Bar", "Notes", Some(synth)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, Q)]).unwrap();
    let keys = b.lane(seq, "Keys", Some(synth)).unwrap();
    let audio = b.lane(seq, "Mic", None).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }))
        .unwrap();
    let path = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap();
    b.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane: audio,
        output: Some(path),
    }))
    .unwrap();
    Doc {
        doc: b.finish(),
        seq,
        keys,
        synth,
        path,
    }
}

fn keys_spec(d: &Doc, thru: bool, armed: bool) -> MidiInputSpec {
    MidiInputSpec {
        lane: d.keys,
        port: Some(port_key("Keys")),
        channel: Some(0),
        dest: Some(d.synth),
        thru,
        armed,
    }
}

fn start(d: &Doc, spec: InputSpec, input: FakeInput) -> (Engine, OfflineBackend, EventLog) {
    let sinks = OfflineSinks::new(10_000);
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(SR), Box::new(sinks)).unwrap();
    engine.set_inputs(spec);
    engine.load(&d.doc, d.seq).unwrap();
    let mut b = OfflineBackend::new(SR, 256).with_input(input);
    b.start(renderer).unwrap();
    (engine, b, log)
}

fn note(on: bool, pitch: u8) -> Midi1Msg {
    if on {
        Midi1Msg::three(0x90, pitch, 100)
    } else {
        Midi1Msg::three(0x80, pitch, 0)
    }
}

fn send(engine: &Engine, port: &str, host_ns: u64, msg: Midi1Msg) {
    assert!(engine.midi_sender().send(MidiIn {
        port: port_key(port),
        host_ns,
        msg,
    }));
}

#[test]
fn channel_messages_are_parsed_and_others_refused() {
    assert_eq!(
        channel_message(&[0x90, 60, 100]),
        Some(Midi1Msg::three(0x90, 60, 100))
    );
    assert_eq!(channel_message(&[0xC5, 7]), Some(Midi1Msg::two(0xC5, 7)));
    for bad in [
        &[][..],
        &[0x90, 60],
        &[0x90, 60, 100, 1],
        &[0x90, 0x80, 100],
        &[0xF0, 1, 0xF7],
        &[0x40, 1, 2],
        &[0xF8],
    ] {
        assert_eq!(channel_message(bad), None, "{bad:?}");
    }
    assert_ne!(port_key("Keys"), port_key("keys"));
}

#[test]
fn thru_plays_at_once_on_the_destination_channel_stopped_or_playing() {
    let d = doc();
    let (engine, mut b, log) = start(
        &d,
        InputSpec {
            midi: vec![keys_spec(&d, true, false)],
            monitor: vec![],
        },
        FakeInput::Signals,
    );
    b.run_frames(512);
    log.drain();
    send(&engine, "Keys", 0, note(true, 64));
    // Another source, another channel: not this lane's.
    send(&engine, "Pads", 0, note(true, 65));
    send(&engine, "Keys", 0, Midi1Msg::three(0x91, 66, 100));
    b.run_frames(256);
    let got: Vec<(u64, Vec<u8>)> = log
        .drain()
        .into_iter()
        .map(|r| (r.frame, r.bytes().to_vec()))
        .collect();
    // On the synth's channel 3, at the block that took it.
    assert_eq!(got, vec![(512, vec![0x93, 64, 100])]);
}

#[test]
fn rerouting_ends_notes_thru_started() {
    let d = doc();
    let (mut engine, mut b, log) = start(
        &d,
        InputSpec {
            midi: vec![keys_spec(&d, true, true)],
            monitor: vec![],
        },
        FakeInput::Signals,
    );
    send(&engine, "Keys", 0, note(true, 64));
    b.run_frames(256);
    log.drain();
    // The lane is disarmed: its routes go and the held note ends.
    engine.set_inputs(InputSpec::default());
    engine.load(&d.doc, d.seq).unwrap();
    b.run_frames(256);
    let got: Vec<Vec<u8>> = log.drain().iter().map(|r| r.bytes().to_vec()).collect();
    assert!(got.contains(&vec![0x83, 64, 0]), "{got:?}");
    // Nothing more goes through.
    send(&engine, "Keys", 0, note(true, 67));
    b.run_frames(256);
    assert!(log.drain().is_empty());
}

#[test]
fn armed_lanes_record_at_the_run_heard_when_the_message_arrived() {
    let d = doc();
    let mut quiet = keys_spec(&d, false, false);
    quiet.lane = LaneId(99);
    quiet.channel = None;
    let (mut engine, mut b, _log) = start(
        &d,
        InputSpec {
            midi: vec![keys_spec(&d, true, true), quiet],
            monitor: vec![],
        },
        FakeInput::Signals,
    );
    let mut h = engine
        .start_recording(&RecordRequest {
            channels: vec![],
            range: None,
            ring_frames: 4800,
        })
        .unwrap();
    // Before the roll: not recorded.
    send(&engine, "Keys", 0, note(true, 50));
    b.run_frames(256);
    engine.play().unwrap();
    // The roll is at absolute frame 256 (run 0).
    b.run_frames(256 * 4);
    // Arrived at absolute frame 256 + 300 (the offline backend's host time
    // is frames ÷ rate), taken by a later block: recorded at run 300.
    let ns = |frame: u64| (frame as f64 * 1e9 / SR).round() as u64;
    send(&engine, "Keys", ns(256 + 300), note(true, 60));
    send(&engine, "Keys", ns(256 + 700), note(false, 60));
    // "Now": at the run of the block that takes it.
    send(&engine, "Keys", 0, note(true, 62));
    b.run_frames(256);
    let mut got: Vec<RecordedMidi> = Vec::new();
    while let Ok(m) = h.midi.pop() {
        got.push(m);
    }
    let runs: Vec<(u64, i64, u8)> = got
        .iter()
        .map(|m| (m.lane.0, m.run, m.msg.bytes[1]))
        .collect();
    assert_eq!(
        runs,
        vec![
            (d.keys.0, 300, 60),
            (d.keys.0, 700, 60),
            (d.keys.0, 1024, 62)
        ]
    );
    // Recorded as played (the chain is empty until plan 5g), not on the
    // destination's channel.
    assert_eq!(got[0].msg, note(true, 60));
    assert_eq!(h.status.lost_midi(), 0);
}

fn peak(b: &mut OfflineBackend, blocks: usize) -> f32 {
    let mut p = 0.0f32;
    for _ in 0..blocks {
        p = p.max(b.block(256));
    }
    p
}

#[test]
fn input_is_heard_through_its_path_by_the_monitor_mode() {
    let d = doc();
    let route = |mode, armed| MonitorRoute {
        path: d.path,
        left: 0,
        right: 0,
        mode,
        armed,
    };
    // Off: silence. Input: the sine, stopped or playing.
    for (mode, armed, heard) in [
        (MonitorMode::Off, true, false),
        (MonitorMode::Input, false, true),
        (MonitorMode::Auto, false, false),
        (MonitorMode::Auto, true, true),
    ] {
        let (_e, mut b, _log) = start(
            &d,
            InputSpec {
                midi: vec![],
                monitor: vec![route(mode, armed)],
            },
            FakeInput::Signals,
        );
        let p = peak(&mut b, 8);
        assert_eq!(p > 0.05, heard, "{mode:?} armed {armed}: {p}");
    }
    // Auto while armed: heard when stopped, not while only playing, again
    // while recording inside the record range, not outside it.
    let (mut engine, mut b, _log) = start(
        &d,
        InputSpec {
            midi: vec![],
            monitor: vec![route(MonitorMode::Auto, true)],
        },
        FakeInput::Signals,
    );
    assert!(peak(&mut b, 4) > 0.05, "stopped");
    engine.locate_frame(40_000).unwrap();
    engine.play().unwrap();
    b.block(256);
    assert!(peak(&mut b, 4) < 0.001, "playing back");
    let _h = engine
        .start_recording(&RecordRequest {
            channels: vec![0],
            range: Some((40_000, 45_000)),
            ring_frames: 48_000,
        })
        .unwrap();
    b.block(256);
    assert!(peak(&mut b, 4) > 0.05, "recording in range");
    // 40 000 + 6 × 256 played; run on past the range end.
    peak(&mut b, 16);
    assert!(peak(&mut b, 4) < 0.001, "recording past the range");
}
VIZDAW_EOF
```

In `crates/core-engine/tests/rt_alloc_record.rs`:

Find:
```rust
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;
```
Replace with:
```rust
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, MonitorRoute, port_key};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};
use core_model::command::{Command, LaneCmd, RoutingCmd};
use core_model::ids::AnyId;
use core_model::lane::MonitorMode;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;
```

Find:
```rust
    let (seq, lane) = bld.sequence("Bar", "Notes", Some(synth)).unwrap();
    bld.notes(seq, lane, &[(0, 60, 0x8000, Q), (2 * Q, 67, 0x8000, Q)])
        .unwrap();
    let doc = bld.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512).with_input(FakeInput::Loopback(1000));
    b.start(renderer).unwrap();
    b.reserve(4096);
```
Replace with:
```rust
    let (seq, lane) = bld.sequence("Bar", "Notes", Some(synth)).unwrap();
    bld.notes(seq, lane, &[(0, 60, 0x8000, Q), (2 * Q, 67, 0x8000, Q)])
        .unwrap();
    let r = bld
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }))
        .unwrap();
    let path = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap();
    bld.run(Command::Lane(LaneCmd::SetOutput {
        seq,
        lane,
        output: Some(path),
    }))
    .unwrap();
    let doc = bld.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    // A MIDI lane with thru and recording, and a monitored input.
    let inputs = InputSpec {
        midi: vec![MidiInputSpec {
            lane,
            port: None,
            channel: None,
            dest: Some(synth),
            thru: true,
            armed: true,
        }],
        monitor: vec![MonitorRoute {
            path,
            left: 0,
            right: 1,
            mode: MonitorMode::Input,
            armed: true,
        }],
    };
    engine.set_inputs(inputs.clone());
    engine.load(&doc, seq).unwrap();
    let midi = engine.midi_sender();
    let mut b = OfflineBackend::new(48_000.0, 512).with_input(FakeInput::Loopback(1000));
    b.start(renderer).unwrap();
    b.reserve(4096);
```

Find:
```rust
        }
        if i % 100 == 0 {
            log.drain();
        }
        if i == 2000 {
            // A new tap replaces the old one while playing.
```
Replace with:
```rust
        }
        if i % 100 == 0 {
            log.drain();
            while h.midi.pop().is_ok() {}
        }
        if i % 7 == 0 {
            let on = i % 14 == 0;
            let msg = Midi1Msg::three(if on { 0x90 } else { 0x80 }, 60 + (i % 12) as u8, 90);
            midi.send(MidiIn {
                port: port_key("Keys"),
                host_ns: 0,
                msg,
            });
        }
        if i == 1500 {
            // Routes change (a reload): thru notes end, the old routes
            // retire.
            let mut changed = inputs.clone();
            changed.monitor[0].mode = MonitorMode::Off;
            engine.set_inputs(changed);
            engine.load(&doc, seq).unwrap();
        }
        if i == 2000 {
            // A new tap replaces the old one while playing.
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test live_input 2>&1 | tail -5
```
Expected: does not compile: `core_engine::input` and `Engine::set_inputs` do not exist.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/input.rs <<'VIZDAW_EOF'
//! Live input (sub-project 5 spec §6.6; §6.8's hook): MIDI from sources to
//! armed lanes' destinations (thru) and to the recorder, and audio input
//! monitoring through a lane's output path. The control side describes
//! what each lane listens to ([`InputSpec`]); the engine resolves it into
//! [`InputRoutes`] at its next load, and the renderer applies them each
//! block. // RT where marked

use crate::midi1::Midi1Msg;
use crate::schedule::TargetIdx;
use core_model::ids::{DestId, LaneId, OutputPathId};
use core_model::lane::MonitorMode;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// MIDI messages the input queue holds until the renderer takes them.
pub const MIDI_IN_CAPACITY: usize = 4096;
/// Messages the renderer takes from the queue per block at most.
pub const MIDI_IN_PER_BLOCK: usize = 1024;
/// The most audio lanes monitored at once.
pub const MAX_MONITOR_ROUTES: usize = 64;

/// How lanes name a MIDI source to the real-time side: FNV-1a of its
/// name.
pub fn port_key(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// A MIDI channel message from a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiIn {
    /// The source ([`port_key`] of its name).
    pub port: u32,
    /// When it arrived, in host nanoseconds; 0: now (an injected message,
    /// placed at the block that takes it; plan 5c Ruling 10).
    pub host_ns: u64,
    /// The message.
    pub msg: Midi1Msg,
}

/// The channel message `bytes` holds (`80`–`EF` status, with its data
/// bytes), if it is one.
pub fn channel_message(bytes: &[u8]) -> Option<Midi1Msg> {
    let (&status, rest) = bytes.split_first()?;
    let len = match status & 0xF0 {
        0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2,
        0xC0 | 0xD0 => 1,
        _ => return None,
    };
    if rest.len() != len || rest.iter().any(|b| *b > 0x7F) {
        return None;
    }
    Some(match rest {
        [a] => Midi1Msg::two(status, *a),
        [a, b] => Midi1Msg::three(status, *a, *b),
        _ => return None,
    })
}

/// Feeds the renderer's MIDI input queue from any thread (CoreMIDI's
/// callback, the control channel). Cheap to clone.
#[derive(Clone)]
pub struct MidiSender {
    tx: Arc<Mutex<Producer<MidiIn>>>,
    dropped: Arc<AtomicU64>,
}

impl std::fmt::Debug for MidiSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MidiSender")
    }
}

impl MidiSender {
    /// Queues `ev`; false when the queue is full (it is dropped and
    /// counted).
    pub fn send(&self, ev: MidiIn) -> bool {
        let ok = self
            .tx
            .lock()
            .map(|mut tx| tx.push(ev).is_ok())
            .unwrap_or(false);
        if !ok {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        ok
    }

    /// Messages dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The MIDI input queue: the sender and the renderer's end.
pub(crate) fn midi_queue() -> (MidiSender, Consumer<MidiIn>) {
    let (tx, rx) = RingBuffer::new(MIDI_IN_CAPACITY);
    (
        MidiSender {
            tx: Arc::new(Mutex::new(tx)),
            dropped: Arc::new(AtomicU64::new(0)),
        },
        rx,
    )
}

/// A lane's MIDI input effect chain (spec §6.8, D26h). Plan 5g fills it
/// with the built-in effects; until then it passes every message through
/// unchanged, so thru and record hear the input as played.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputChain;

impl InputChain {
    /// Runs `msg` through the chain, handing every output to `out`. // RT
    pub fn process(&mut self, msg: Midi1Msg, out: &mut impl FnMut(Midi1Msg)) {
        out(msg);
    }
}

/// What a MIDI lane listens to, before targets are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiInputSpec {
    /// The lane.
    pub lane: LaneId,
    /// The source ([`port_key`]); `None`: every source.
    pub port: Option<u32>,
    /// The channel 0–15; `None`: every channel.
    pub channel: Option<u8>,
    /// Where thru goes (the lane's destination).
    pub dest: Option<DestId>,
    /// Thru is on (the lane's monitor is not Off).
    pub thru: bool,
    /// The lane is armed: it records while a tap rolls.
    pub armed: bool,
}

/// One audio lane's monitoring (spec §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRoute {
    /// The output path whose strip hears it.
    pub path: OutputPathId,
    /// The device channel heard left.
    pub left: u16,
    /// The device channel heard right (the same one for a mono input).
    pub right: u16,
    /// When it is heard.
    pub mode: MonitorMode,
    /// The lane is armed.
    pub armed: bool,
}

impl MonitorRoute {
    /// Whether the input is heard now: Input always, Off never, Auto
    /// while armed and either stopped or recording inside the record range
    /// (spec §6.6). // RT
    pub fn heard(&self, playing: bool, recording_in_range: bool) -> bool {
        match self.mode {
            MonitorMode::Input => true,
            MonitorMode::Off => false,
            MonitorMode::Auto => self.armed && (!playing || recording_in_range),
        }
    }
}

/// What every lane listens to (the control side's request).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputSpec {
    /// MIDI lanes.
    pub midi: Vec<MidiInputSpec>,
    /// Monitored audio lanes (at most [`MAX_MONITOR_ROUTES`]).
    pub monitor: Vec<MonitorRoute>,
}

/// One MIDI lane's resolved route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiRoute {
    /// The request.
    pub spec: MidiInputSpec,
    /// The targets thru plays on (two for a built-in synth routed both
    /// ways).
    pub targets: [Option<TargetIdx>; 2],
    /// The channel thru plays on (the destination's).
    pub dest_channel: u8,
    /// The lane's input chain.
    pub chain: InputChain,
}

impl MidiRoute {
    /// Whether `ev` is for this lane. // RT
    pub fn matches(&self, ev: &MidiIn) -> bool {
        let ch = ev.msg.bytes[0] & 0x0F;
        self.spec.port.is_none_or(|p| p == ev.port) && self.spec.channel.is_none_or(|c| c == ch)
    }
}

/// The renderer's live input routes, published at every load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputRoutes {
    /// MIDI lanes.
    pub midi: Box<[MidiRoute]>,
    /// Monitored audio lanes.
    pub monitor: Box<[MonitorRoute]>,
}

/// `msg` on channel `ch`. // RT
pub fn on_channel(msg: Midi1Msg, ch: u8) -> Midi1Msg {
    let mut m = msg;
    m.bytes[0] = (m.bytes[0] & 0xF0) | (ch & 0x0F);
    m
}
VIZDAW_EOF
```

In `crates/core-engine/src/render.rs`:

Find:
```rust

use crate::backend::{BlockInfo, BlockTime, InputBlock, MidiOut, StereoBuf};
use crate::graph::exec::{Exec, RunResult, SinkTable};
use crate::graph::{Graph, MAX_INSERTS, MAX_NODES};
use crate::midi1::Midi1Msg;
use crate::notes::ActiveNotes;
use crate::pool::PoolShared;
```
Replace with:
```rust

use crate::backend::{BlockInfo, BlockTime, InputBlock, MidiOut, StereoBuf};
use crate::graph::exec::{Exec, RunResult, SinkTable};
use crate::graph::regions::InputCopy;
use crate::graph::{Graph, MAX_INSERTS, MAX_NODES};
use crate::input::{InputRoutes, MIDI_IN_PER_BLOCK, MidiIn, on_channel};
use crate::midi1::Midi1Msg;
use crate::notes::ActiveNotes;
use crate::pool::PoolShared;
```

Find:
```rust
    /// through the retire ring; dropping it tells the control side the
    /// recording's input ended). Spec §6.2.
    Record(Option<Box<RecordTap>>),
}

/// Something the renderer hands back for freeing off the audio thread.
```
Replace with:
```rust
    /// through the retire ring; dropping it tells the control side the
    /// recording's input ended). Spec §6.2.
    Record(Option<Box<RecordTap>>),
    /// Replace the live input routes (spec §6.6); notes sounding through
    /// thru end first.
    SetInputs(Box<InputRoutes>),
}

/// Something the renderer hands back for freeing off the audio thread.
```

Find:
```rust
    Sink(Sink),
    /// A removed record tap.
    Tap(Box<RecordTap>),
}

/// Capacity of the command ring.
```
Replace with:
```rust
    Sink(Sink),
    /// A removed record tap.
    Tap(Box<RecordTap>),
    /// Replaced input routes.
    Inputs(Box<InputRoutes>),
}

/// Capacity of the command ring.
```

Find:
```rust
    auto_last: Box<[f64; MAX_AUTO_LANES]>,
    /// The recording's tap (spec §6.2), while one is installed.
    tap: Option<Box<RecordTap>>,
}

impl std::fmt::Debug for Renderer {
```
Replace with:
```rust
    auto_last: Box<[f64; MAX_AUTO_LANES]>,
    /// The recording's tap (spec §6.2), while one is installed.
    tap: Option<Box<RecordTap>>,
    /// MIDI from sources (spec §6.6).
    midi_in: Consumer<MidiIn>,
    /// The live input routes.
    inputs: Option<Box<InputRoutes>>,
    /// Notes sounding through thru (boxed: the renderer moves between
    /// threads by value).
    thru: Box<ActiveNotes>,
    /// This block's input, for monitoring.
    input_copy: Box<InputCopy>,
}

impl std::fmt::Debug for Renderer {
```

Find:
```rust
        shared: Arc<Shared>,
        pool: Option<Arc<PoolShared>>,
        reader: core_media::stream::StreamReader,
    ) -> Renderer {
        let mut r = Renderer {
            cfg,
```
Replace with:
```rust
        shared: Arc<Shared>,
        pool: Option<Arc<PoolShared>>,
        reader: core_media::stream::StreamReader,
        midi_in: Consumer<MidiIn>,
    ) -> Renderer {
        let mut r = Renderer {
            cfg,
```

Find:
```rust
            pending_skip: 0,
            auto_last: Box::new([f64::NAN; MAX_AUTO_LANES]),
            tap: None,
        };
        r.prepare(cfg.max_frames);
        r
```
Replace with:
```rust
            pending_skip: 0,
            auto_last: Box::new([f64::NAN; MAX_AUTO_LANES]),
            tap: None,
            midi_in,
            inputs: None,
            thru: Box::default(),
            input_copy: Box::new(InputCopy::new(0, 0)),
        };
        r.prepare(cfg.max_frames);
        r
```

Find:
```rust
        self.bufs = (0..MAX_TARGETS)
            .map(|_| StereoBuf::new(max_frames))
            .collect();
        if let Some(g) = self.graph.as_mut() {
            g.prepare(max_frames);
        }
```
Replace with:
```rust
        self.bufs = (0..MAX_TARGETS)
            .map(|_| StereoBuf::new(max_frames))
            .collect();
        *self.input_copy = InputCopy::new(crate::backend::MAX_INPUT_CHANNELS, max_frames);
        if let Some(g) = self.graph.as_mut() {
            g.prepare(max_frames);
        }
```

Find:
```rust
                t.begin_block(at, input.delay_frames);
            }
        }
        // No run is in flight (`try_finish` succeeded above): the streamer
        // may count a new epoch (5a Ruling 9) and plan around the playhead
        // as this block's commands left it (a Locate's new position, never
```
Replace with:
```rust
                t.begin_block(at, input.delay_frames);
            }
        }
        self.live_midi();
        let monitored = self.copy_monitored(input, frames);
        // No run is in flight (`try_finish` succeeded above): the streamer
        // may count a new epoch (5a Ruling 9) and plan around the playhead
        // as this block's commands left it (a Locate's new position, never
```

Find:
```rust
            let end = offset + run;
            self.automate(length, run);
            let info = self.block_info();
            let regions = self.region_run(length);
            let mut o = Out {
                sinks: &mut self.sinks,
                shared: &self.shared,
```
Replace with:
```rust
            let end = offset + run;
            self.automate(length, run);
            let info = self.block_info();
            let regions = self.region_run(length, monitored, offset);
            let mut o = Out {
                sinks: &mut self.sinks,
                shared: &self.shared,
```

Find:
```rust
                Ok(EngineCmd::SetGraph(_)) => self.graph.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::RemoveSink(_)) => self.retire.slots() == 0,
                Ok(EngineCmd::Record(_)) => self.tap.is_some() && self.retire.slots() == 0,
                Ok(_) => false,
                Err(_) => break,
            };
```
Replace with:
```rust
                Ok(EngineCmd::SetGraph(_)) => self.graph.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::RemoveSink(_)) => self.retire.slots() == 0,
                Ok(EngineCmd::Record(_)) => self.tap.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::SetInputs(_)) => self.inputs.is_some() && self.retire.slots() == 0,
                Ok(_) => false,
                Err(_) => break,
            };
```

Find:
```rust
                // Forget its sounding notes: nothing will play them now.
                self.notes.release(idx, |_| {});
                self.audition.release(idx, |_| {});
                let taken = self
                    .sinks
                    .get_mut(usize::from(idx.0))
```
Replace with:
```rust
                // Forget its sounding notes: nothing will play them now.
                self.notes.release(idx, |_| {});
                self.audition.release(idx, |_| {});
                self.thru.release(idx, |_| {});
                let taken = self
                    .sinks
                    .get_mut(usize::from(idx.0))
```

Find:
```rust
                    }
                }
            }
            EngineCmd::AddSink(idx, sink) => {
                if let Some(slot) = self.sinks.get_mut(usize::from(idx.0)) {
                    if let Some(old) = slot.replace(sink) {
```
Replace with:
```rust
                    }
                }
            }
            EngineCmd::SetInputs(new) => {
                // Notes thru started end: a lane disarmed or re-routed
                // must not leave one hanging.
                let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
                let mut o = Out {
                    sinks: &mut self.sinks,
                    shared: &self.shared,
                    block_ns: self.block_ns,
                    ns_per_frame: 0.0,
                    late_ns,
                    base: 0,
                };
                release_all(&mut self.thru, &mut o, 0);
                if let Some(old) = self.inputs.replace(new) {
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Inputs(old))
                    {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
            }
            EngineCmd::AddSink(idx, sink) => {
                if let Some(slot) = self.sinks.get_mut(usize::from(idx.0)) {
                    if let Some(old) = slot.replace(sink) {
```

Find:
```rust
    /// schedule's audio from the playhead, late within `LATE_SECONDS` of
    /// playing after a Play or Locate, muted when the device refused the
    /// rate. // RT
    fn region_run(&self, length: u64) -> crate::graph::RegionRun {
        let Some(s) = self.schedule.as_deref() else {
            return crate::graph::RegionRun::NONE;
        };
```
Replace with:
```rust
    /// schedule's audio from the playhead, late within `LATE_SECONDS` of
    /// playing after a Play or Locate, muted when the device refused the
    /// rate. // RT
    fn region_run(&self, length: u64, monitored: bool, offset: u32) -> crate::graph::RegionRun {
        let base = self.audio_run(length);
        if !monitored {
            return base;
        }
        let Some(routes) = self.inputs.as_deref() else {
            return base;
        };
        let playing = self.transport.state == PlayState::Playing;
        let pos = self.transport.play_pos(length);
        let in_range = match (self.tap.as_deref(), pos) {
            (Some(t), Some(p)) => t.rolling() && t.in_range(p),
            _ => false,
        };
        let mut heard = 0u64;
        for (k, r) in routes.monitor.iter().enumerate().take(64) {
            if r.heard(playing, in_range) {
                heard |= 1u64 << k;
            }
        }
        // SAFETY: the routes (boxed) and the input copy (boxed) change only
        // in `drain_commands`, `copy_monitored` or `prepare`, after
        // `try_finish` succeeded, so they outlive every run given this.
        unsafe { base.with_monitor(&self.input_copy, &routes.monitor, heard, offset) }
    }

    /// The schedule's audio from the playhead (none while stopped). // RT
    fn audio_run(&self, length: u64) -> crate::graph::RegionRun {
        let Some(s) = self.schedule.as_deref() else {
            return crate::graph::RegionRun::NONE;
        };
```

Find:
```rust
                self.since_start < self.late_frames(),
                self.shared.regions_muted(),
            )
        }
    }
```
Replace with:
```rust
                self.since_start < self.late_frames(),
                self.shared.regions_muted(),
            )
        }
    }

    /// Copies the input channels monitor routes name; true when there is
    /// one. // RT
    fn copy_monitored(&mut self, input: &InputBlock<'_>, frames: u32) -> bool {
        let Some(routes) = self.inputs.as_deref() else {
            return false;
        };
        if routes.monitor.is_empty() {
            return false;
        }
        let r = &routes.monitor;
        self.input_copy.fill(input.channels, frames, |c| {
            r.iter()
                .any(|m| usize::from(m.left) == c || usize::from(m.right) == c)
        });
        true
    }

    /// Takes the queued MIDI input: each message a lane listens to goes
    /// through its input chain to thru (its destination, now) and, while a
    /// tap rolls and the lane is armed, to the recorder, stamped at the
    /// run frame heard when it arrived (spec §6.3, §6.6). // RT
    fn live_midi(&mut self) {
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let sr = self.cfg.sample_rate;
        let block_ns = self.block_ns;
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns,
            ns_per_frame: 0.0,
            late_ns,
            base: 0,
        };
        let thru = &mut self.thru;
        let mut tap = self.tap.as_deref_mut();
        let mut routes = self.inputs.as_deref_mut();
        for _ in 0..MIDI_IN_PER_BLOCK {
            let Ok(ev) = self.midi_in.pop() else {
                break;
            };
            let Some(routes) = routes.as_deref_mut() else {
                continue;
            };
            let offset = if ev.host_ns == 0 {
                0
            } else {
                let ns = i128::from(ev.host_ns) - i128::from(block_ns);
                (ns as f64 * sr / 1e9).round() as i64
            };
            for r in routes.midi.iter_mut() {
                if !r.matches(&ev) {
                    continue;
                }
                let (spec, targets, ch) = (&r.spec, r.targets, r.dest_channel);
                r.chain.process(ev.msg, &mut |m| {
                    if spec.thru {
                        let m = on_channel(m, ch);
                        for t in targets.iter().flatten() {
                            o.send(*t, 0, m.as_slice());
                            thru.observe(*t, m.as_slice());
                        }
                    }
                    if spec.armed {
                        if let Some(t) = tap.as_deref_mut() {
                            t.midi(spec.lane, offset, m);
                        }
                    }
                });
            }
        }
    }
```

In `crates/core-engine/src/graph/regions.rs`:

Find:
```rust
//! plan 5a Ruling 11). // RT

use crate::backend::StereoBuf;
use crate::schedule::AudioSchedule;
use core_media::stream::{Read, StreamReader};
use core_model::ids::OutputPathId;

/// What `Regions` nodes read during one run: the playing schedule's audio,
/// the stream reader, and where the run is. Built by the renderer (or a
/// test `Runner`) before each run.
#[derive(Debug, Clone, Copy)]
pub struct RegionRun {
    audio: *const AudioSchedule,
    reader: *const StreamReader,
    /// Schedule frame of the run's first frame.
    pub frame: u64,
    /// The transport plays through the whole run.
```
Replace with:
```rust
//! plan 5a Ruling 11). // RT

use crate::backend::StereoBuf;
use crate::input::MonitorRoute;
use crate::schedule::AudioSchedule;
use core_media::stream::{Read, StreamReader};
use core_model::ids::OutputPathId;

/// The device input `Regions` nodes monitor (spec §6.6): the renderer's
/// copy of this block's input channels, so a run that outlives the
/// device's call still reads valid memory.
#[derive(Debug)]
pub struct InputCopy {
    chans: Box<[Box<[f32]>]>,
}

impl InputCopy {
    /// `channels` channels of `frames` frames, zeroed. Allocates.
    pub fn new(channels: usize, frames: u32) -> InputCopy {
        InputCopy {
            chans: (0..channels)
                .map(|_| vec![0.0; frames as usize].into_boxed_slice())
                .collect(),
        }
    }

    /// Copies the first `frames` frames of the channels `want` names from
    /// `input` (missing channels read 0). // RT
    pub fn fill(&mut self, input: &[&[f32]], frames: u32, want: impl Fn(usize) -> bool) {
        let n = frames as usize;
        for (c, dst) in self.chans.iter_mut().enumerate() {
            if !want(c) {
                continue;
            }
            let Some(dst) = dst.get_mut(..n.min(dst.len())) else {
                continue;
            };
            match input.get(c) {
                Some(src) => {
                    for (k, d) in dst.iter_mut().enumerate() {
                        *d = src.get(k).copied().unwrap_or(0.0);
                    }
                }
                None => dst.fill(0.0),
            }
        }
    }

    /// Channel `c`'s frames `[at, at + n)`. // RT
    fn get(&self, c: u16, at: usize, n: usize) -> &[f32] {
        self.chans
            .get(usize::from(c))
            .and_then(|ch| ch.get(at..at + n))
            .unwrap_or(&[])
    }
}

/// What `Regions` nodes monitor during a run.
#[derive(Debug, Clone, Copy)]
struct Monitor {
    input: *const InputCopy,
    routes: *const [MonitorRoute],
    /// Bit `k`: route `k` is heard.
    heard: u64,
    /// The run's first frame within the copied block.
    offset: u32,
}

/// What `Regions` nodes read during one run: the playing schedule's audio,
/// the stream reader, where the run is, and the monitored input. Built by
/// the renderer (or a test `Runner`) before each run.
#[derive(Debug, Clone, Copy)]
pub struct RegionRun {
    audio: *const AudioSchedule,
    reader: *const StreamReader,
    monitor: Option<Monitor>,
    /// Schedule frame of the run's first frame.
    pub frame: u64,
    /// The transport plays through the whole run.
```

Find:
```rust
    pub muted: bool,
}

// SAFETY: the pointees are `Sync` (`AudioSchedule` is plain data,
// `StreamReader` only touches atomics); `RegionRun::new`'s caller keeps
// them alive while any thread renders the run.
unsafe impl Send for RegionRun {}
// SAFETY: as for `Send`.
unsafe impl Sync for RegionRun {}
```
Replace with:
```rust
    pub muted: bool,
}

// SAFETY: the pointees are `Sync` (`AudioSchedule`, `InputCopy` and the
// monitor routes are plain data, `StreamReader` only touches atomics);
// the callers of `RegionRun::new` and `with_monitor` keep them alive while
// any thread renders the run.
unsafe impl Send for RegionRun {}
// SAFETY: as for `Send`.
unsafe impl Sync for RegionRun {}
```

Find:
```rust
    pub const NONE: RegionRun = RegionRun {
        audio: std::ptr::null(),
        reader: std::ptr::null(),
        frame: 0,
        playing: false,
        late: false,
```
Replace with:
```rust
    pub const NONE: RegionRun = RegionRun {
        audio: std::ptr::null(),
        reader: std::ptr::null(),
        monitor: None,
        frame: 0,
        playing: false,
        late: false,
```

Find:
```rust
        RegionRun {
            audio,
            reader,
            frame,
            playing,
            late,
            muted,
        }
    }
```
Replace with:
```rust
        RegionRun {
            audio,
            reader,
            monitor: None,
            frame,
            playing,
            late,
            muted,
        }
    }

    /// The run also mixes the input routes `heard` names (bit `k`: route
    /// `k`) into their paths, from frame `offset` of `input` (spec §6.6).
    ///
    /// # Safety
    /// `input` and `routes` must stay alive and unchanged until the runs
    /// given the result have finished (`Exec::try_finish` returned true).
    pub(crate) unsafe fn with_monitor(
        self,
        input: &InputCopy,
        routes: &[MonitorRoute],
        heard: u64,
        offset: u32,
    ) -> RegionRun {
        RegionRun {
            monitor: (heard != 0).then_some(Monitor {
                input,
                routes,
                heard,
                offset,
            }),
            ..self
        }
    }

    /// Adds the heard input routes of `path` to `out`. // RT
    fn add_monitor(&self, path: OutputPathId, frames: u32, out: &mut StereoBuf) {
        let Some(m) = self.monitor else {
            return;
        };
        // SAFETY: `with_monitor`'s contract keeps both alive for this run.
        let (Some(input), Some(routes)) =
            (unsafe { m.input.as_ref() }, unsafe { m.routes.as_ref() })
        else {
            return;
        };
        let n = frames as usize;
        let at = m.offset as usize;
        for (k, r) in routes.iter().enumerate() {
            if r.path != path || k >= 64 || m.heard & (1u64 << k) == 0 {
                continue;
            }
            for (dst, ch) in [(&mut out.left, r.left), (&mut out.right, r.right)] {
                let src = input.get(ch, at, n);
                for (d, x) in dst.iter_mut().zip(src) {
                    *d += *x;
                }
            }
        }
    }
```

Find:
```rust
        scratch: &mut StereoBuf,
    ) {
        out.clear(frames);
        if !self.playing || self.muted {
            return;
        }
```
Replace with:
```rust
        scratch: &mut StereoBuf,
    ) {
        out.clear(frames);
        self.add_monitor(path, frames, out);
        if !self.playing || self.muted {
            return;
        }
```

In `crates/core-engine/src/engine.rs`:

Find:
```rust
    moved_since_play: bool,
    /// The streamer's (underruns, late) counters at the last `faults`.
    stream_seen: (AtomicU64, AtomicU64),
}

impl std::fmt::Debug for Engine {
```
Replace with:
```rust
    moved_since_play: bool,
    /// The streamer's (underruns, late) counters at the last `faults`.
    stream_seen: (AtomicU64, AtomicU64),
    /// What lanes listen to (spec §6.6); resolved at every load.
    input_spec: crate::input::InputSpec,
    /// Feeds the renderer's MIDI input queue.
    midi_tx: crate::input::MidiSender,
    /// The routes last sent to the renderer: a load sends them only when
    /// they change, so a load still retires just a schedule and a graph.
    inputs_sent: crate::input::InputRoutes,
}

impl std::fmt::Debug for Engine {
```

Find:
```rust
        } else {
            None
        };
        let (streamer, reader) =
            Streamer::new(cfg.sample_rate, cfg.stream, cfg.disk).map_err(|e| {
                EngineError::Thread {
```
Replace with:
```rust
        } else {
            None
        };
        let (midi_tx, midi_rx) = crate::input::midi_queue();
        let (streamer, reader) =
            Streamer::new(cfg.sample_rate, cfg.stream, cfg.disk).map_err(|e| {
                EngineError::Thread {
```

Find:
```rust
            Arc::clone(&shared),
            pool.as_ref().map(Pool::handle),
            reader,
        );
        let engine = Engine {
            cfg,
```
Replace with:
```rust
            Arc::clone(&shared),
            pool.as_ref().map(Pool::handle),
            reader,
            midi_rx,
        );
        let engine = Engine {
            cfg,
```

Find:
```rust
            commanded_playing: false,
            moved_since_play: false,
            stream_seen: (AtomicU64::new(0), AtomicU64::new(0)),
        };
        Ok((engine, renderer))
    }
```
Replace with:
```rust
            commanded_playing: false,
            moved_since_play: false,
            stream_seen: (AtomicU64::new(0), AtomicU64::new(0)),
            input_spec: crate::input::InputSpec::default(),
            midi_tx,
            inputs_sent: crate::input::InputRoutes::default(),
        };
        Ok((engine, renderer))
    }
```

Find:
```rust
        // Room for removing and adding every possible sink plus the graph,
        // the schedule and the loop, so no push below can fail after
        // targets were interned.
        if self.cmds.slots() < 2 * MAX_TARGETS + 3 {
            return Err(EngineError::Busy);
        }
        let opts = CompileOptions {
```
Replace with:
```rust
        // Room for removing and adding every possible sink plus the graph,
        // the schedule and the loop, so no push below can fail after
        // targets were interned.
        if self.cmds.slots() < 2 * MAX_TARGETS + 4 {
            return Err(EngineError::Busy);
        }
        let opts = CompileOptions {
```

Find:
```rust
            }
        };
        let compiled = compile(doc, seq, &opts, &mut resolve);
        let (mut schedule, mut report) = match compiled {
            Ok(c) => c,
            Err(e) => {
```
Replace with:
```rust
            }
        };
        let compiled = compile(doc, seq, &opts, &mut resolve);
        // The targets live input plays on (spec §6.6): interned like the
        // schedule's, so thru reaches a destination the sequence never
        // plays.
        let spec = self.input_spec.clone();
        let mut midi_routes: Vec<crate::input::MidiRoute> = Vec::with_capacity(spec.midi.len());
        if compiled.is_ok() {
            for m in &spec.midi {
                let d = m.dest.and_then(|d| doc.instruments().get(&d));
                let kinds = d
                    .and_then(|d| kinds_for(&d.kind, opts.route, &opts.ports).ok())
                    .unwrap_or_default();
                let mut targets = [None, None];
                for (slot, k) in targets.iter_mut().zip(&kinds) {
                    *slot = resolve(k).ok().and_then(Result::ok);
                }
                let dest_channel = match d.map(|d| &d.kind) {
                    Some(DestKind::MidiEndpoint { channel, .. }) => channel & 0x0F,
                    _ => 0,
                };
                midi_routes.push(crate::input::MidiRoute {
                    spec: m.clone(),
                    targets,
                    dest_channel,
                    chain: crate::input::InputChain,
                });
            }
        }
        let (mut schedule, mut report) = match compiled {
            Ok(c) => c,
            Err(e) => {
```

Find:
```rust
        self.length_frames = schedule.length_frames;
        self.loop_range = schedule.loop_range;
        self.push(EngineCmd::SetSchedule(Box::new(schedule)))?;
        if self.default_loop {
            // The length may have changed; the mapped old range would not
            // follow it.
```
Replace with:
```rust
        self.length_frames = schedule.length_frames;
        self.loop_range = schedule.loop_range;
        self.push(EngineCmd::SetSchedule(Box::new(schedule)))?;
        let monitor: Box<[crate::input::MonitorRoute]> = spec
            .monitor
            .iter()
            .take(crate::input::MAX_MONITOR_ROUTES)
            .copied()
            .collect();
        let routes = crate::input::InputRoutes {
            midi: midi_routes.into_boxed_slice(),
            monitor,
        };
        if routes != self.inputs_sent {
            self.inputs_sent = routes.clone();
            self.push(EngineCmd::SetInputs(Box::new(routes)))?;
        }
        if self.default_loop {
            // The length may have changed; the mapped old range would not
            // follow it.
```

Find:
```rust
    pub fn stop_recording(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Record(None))
    }
}

impl Drop for Engine {
```
Replace with:
```rust
    pub fn stop_recording(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Record(None))
    }

    /// What lanes listen to (spec §6.6); the next [`Engine::load`]
    /// resolves and publishes it.
    pub fn set_inputs(&mut self, spec: crate::input::InputSpec) {
        self.input_spec = spec;
    }

    /// What lanes listen to, as last set.
    pub fn inputs(&self) -> &crate::input::InputSpec {
        &self.input_spec
    }

    /// Feeds this engine's MIDI input queue (CoreMIDI sources,
    /// `midi.inject`).
    pub fn midi_sender(&self) -> crate::input::MidiSender {
        self.midi_tx.clone()
    }
}

impl Drop for Engine {
```

In `crates/core-engine/src/lib.rs`:

Find:
```rust
#[cfg(feature = "fixtures")]
pub mod fixtures;
pub mod graph;
pub mod midi1;
pub mod notes;
pub mod offline;
```
Replace with:
```rust
#[cfg(feature = "fixtures")]
pub mod fixtures;
pub mod graph;
pub mod input;
pub mod midi1;
pub mod notes;
pub mod offline;
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test live_input 2>&1 | tail -3
cargo test -p core-engine --features rt-check --test rt_alloc_record 2>&1 | tail -3
```
Expected: `live_input` 5 passed (including `armed_lanes_record_at_the_run_heard_when_the_message_arrived`, Review Focus 4); `rt_alloc_record` still 1 passed, now with thru and monitoring in the armed block.

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep -E "^test result|FAILED|panicked"
cargo test -p core-engine 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/input.rs crates/core-engine/src/render.rs crates/core-engine/src/graph/regions.rs crates/core-engine/src/engine.rs crates/core-engine/src/lib.rs crates/core-engine/tests/live_input.rs crates/core-engine/tests/rt_alloc_record.rs
git commit -F - <<'VIZDAW_MSG'
core-engine: MIDI input, thru and monitoring

Input MIDI reaches every listening lane by source and channel, passes
the input chain (a pass-through hook until plan 5g), plays thru at once
on the lane's destination and is recorded at the run heard when it
arrived. Monitored audio input is mixed into the lane's path. Notes
thru started end when the routes change.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 7: `core-ffi` — the pass planner

**Files:**
- Create: `crates/core-ffi/src/passes.rs`
- Modify: `crates/core-ffi/src/lib.rs`
- Test: `crates/core-ffi/tests/passes.rs`

**Interfaces:**
- Consumes: Task 3's `RecordPass`; Task 5's `Mark`, `RecordedMidi`; 4a's `TempoTable`.
- Produces: `passes::{RecordRange { start, end: Option<u64> }, PassFate { Undecided, Kept, Discarded }, decide(fates, completed, keep), AudioTake { lane, media, .. }, Captured { doc, seq, .. }, plan(&Captured) -> Result<Option<RecordPass>, String>}`.

Rulings 6, 11, 12 and 13, as pure arithmetic: runs and marks become timeline frames, those become regions and events per pass, and fates decide which passes are kept.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/passes.rs <<'VIZDAW_EOF'
//! Placing a recording (plan 5c Task 7; sub-project 5 spec §6.3–§6.5):
//! input files become regions at the timeline frame that was heard when
//! they were captured (device delay and graph latency taken off), pre-roll
//! stays as region handles, punch and loop ranges cut what is kept, loop
//! passes become takes (discarded ones dropped), and MIDI lands where it
//! was played with replace clearing only the recorded range.

use core_engine::fixtures::{Builder, Q};
use core_engine::midi1::Midi1Msg;
use core_engine::record::{Mark, RecordedMidi};
use core_engine::schedule::TempoTable;
use core_model::command::{RecordPass, RecordedMedia};
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{LaneId, SeqId};
use core_model::time::Tick;
use vizdaw_ffi::passes::{AudioTake, Captured, PassFate, RecordRange, decide, plan};

/// 120 BPM at 48 kHz: one beat (Q ticks) is 24 000 frames.
const BEAT: u64 = 24_000;

struct Doc {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    mic: LaneId,
}

fn doc(bpm: Option<u32>) -> Doc {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Song", "Keys", Some(synth)).unwrap();
    // An old note at beat 2 and a CC at beat 6.
    b.notes(seq, keys, &[(2 * Q, 40, 0x8000, Q)]).unwrap();
    let mic = b.lane(seq, "Mic", None).unwrap();
    if let Some(bpm) = bpm {
        b.tempo(seq, &[(0, bpm, false)]).unwrap();
    }
    Doc {
        doc: b.finish(),
        seq,
        keys,
        mic,
    }
}

fn file(frames: u64) -> RecordedMedia {
    RecordedMedia {
        path: "Media/Recorded/Mic 2026-10-20 120000.caf".into(),
        sample_rate: 48_000,
        channels: 1,
        length_frames: frames,
        content_hash: "cd".repeat(32),
        name: "Mic 2026-10-20 120000".into(),
        pass: 1,
    }
}

fn mark(run: u64, frame: u64, wrap: bool) -> Mark {
    Mark { run, frame, wrap }
}

struct Case<'a> {
    marks: Vec<Mark>,
    end_run: u64,
    latency: u64,
    input_delay: i64,
    range: RecordRange,
    looped: bool,
    replace: bool,
    takes: bool,
    fates: Vec<PassFate>,
    audio: Vec<AudioTake>,
    midi: Vec<RecordedMidi>,
    midi_lanes: Vec<LaneId>,
    d: &'a Doc,
}

impl<'a> Case<'a> {
    fn new(d: &'a Doc, start: u64) -> Case<'a> {
        Case {
            marks: vec![mark(0, start, false)],
            end_run: 4 * BEAT,
            latency: 0,
            input_delay: 0,
            range: RecordRange { start, end: None },
            looped: false,
            replace: true,
            takes: true,
            fates: Vec::new(),
            audio: Vec::new(),
            midi: Vec::new(),
            midi_lanes: Vec::new(),
            d,
        }
    }

    fn run(&self) -> Option<RecordPass> {
        plan(&Captured {
            doc: &self.d.doc,
            seq: self.d.seq,
            marks: &self.marks,
            end_run: self.end_run,
            latency: self.latency,
            input_delay: self.input_delay,
            range: self.range,
            looped: self.looped,
            replace: self.replace,
            passes_as_takes: self.takes,
            fates: &self.fates,
            audio: &self.audio,
            midi_lanes: &self.midi_lanes,
            midi: &self.midi,
            with_gaps: false,
        })
        .unwrap()
    }
}

/// `(tick, source_offset, length)` of every region in the pass's active
/// content of lane `lane`.
fn active_regions(p: &RecordPass, lane: LaneId) -> Vec<(i64, u64, u64)> {
    p.lanes
        .iter()
        .filter(|l| l.lane == lane)
        .flat_map(|l| l.active.regions.iter())
        .map(|r| (r.tick.0, r.source_offset, r.length))
        .collect()
}

fn audio_case(d: &Doc, start: u64, frames: u64) -> Case<'_> {
    let mut c = Case::new(d, start);
    c.end_run = frames;
    c.audio = vec![AudioTake {
        lane: d.mic,
        media: file(frames),
    }];
    c
}

#[test]
fn a_take_lands_where_it_was_heard_from_the_record_start() {
    let d = doc(None);
    let c = audio_case(&d, 2 * BEAT, 4 * BEAT);
    let p = c.run().unwrap();
    assert_eq!(p.media.len(), 1);
    assert_eq!(active_regions(&p, d.mic), vec![(2 * Q, 0, 4 * BEAT)]);
    assert!(
        p.lanes.iter().all(|l| l.takes.is_empty()),
        "no takes without a loop"
    );
}

#[test]
fn the_device_delay_and_graph_latency_come_off_the_file() {
    let d = doc(None);
    let mut c = audio_case(&d, 2 * BEAT, 4 * BEAT);
    c.latency = 512;
    c.input_delay = 300;
    let p = c.run().unwrap();
    // File frame 812 was captured while the record start was heard.
    assert_eq!(
        active_regions(&p, d.mic),
        vec![(2 * Q, 812, 4 * BEAT - 812)]
    );
}

#[test]
fn pre_roll_is_kept_as_a_handle_and_punch_cuts_the_rest() {
    let d = doc(None);
    // Playing from beat 1, recording from beat 2: the first beat of the
    // file is a handle before the region.
    let mut c = audio_case(&d, BEAT, 5 * BEAT);
    c.marks = vec![mark(0, 0, false)];
    let p = c.run().unwrap();
    assert_eq!(active_regions(&p, d.mic), vec![(Q, BEAT, 4 * BEAT)]);
    // Punch beats 3–4: only that much, the rest of the file as handles.
    c.range = RecordRange {
        start: 2 * BEAT,
        end: Some(4 * BEAT),
    };
    let p = c.run().unwrap();
    assert_eq!(active_regions(&p, d.mic), vec![(2 * Q, 2 * BEAT, 2 * BEAT)]);
    // A punch the playback never reached records nothing.
    c.range = RecordRange {
        start: 9 * BEAT,
        end: Some(10 * BEAT),
    };
    assert_eq!(c.run(), None);
}

#[test]
fn loop_passes_become_takes_and_discarded_ones_are_dropped() {
    let d = doc(None);
    let len = 2 * BEAT;
    let mut c = audio_case(&d, BEAT, 3 * len);
    c.looped = true;
    c.range = RecordRange {
        start: BEAT,
        end: Some(BEAT + len),
    };
    c.marks = vec![
        mark(0, BEAT, false),
        mark(len, BEAT, true),
        mark(2 * len, BEAT, true),
    ];
    let p = c.run().unwrap();
    let l = p.lanes.iter().find(|l| l.lane == d.mic).unwrap();
    let names: Vec<&str> = l.takes.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["Take 2", "Take 3", "Take 4"]);
    let offsets: Vec<u64> = l
        .takes
        .iter()
        .map(|(_, t)| t.regions[0].source_offset)
        .collect();
    assert_eq!(offsets, vec![0, len, 2 * len]);
    assert!(l.takes.iter().all(|(_, t)| t.regions[0].tick == Tick(Q)));
    // Keep after pass 1, discard after pass 2: passes 1 and 3 stay.
    let mut fates = Vec::new();
    decide(&mut fates, 1, true);
    decide(&mut fates, 2, false);
    assert_eq!(fates, vec![PassFate::Kept, PassFate::Discarded]);
    c.fates = fates;
    let p = c.run().unwrap();
    let l = p.lanes.iter().find(|l| l.lane == d.mic).unwrap();
    let offsets: Vec<u64> = l
        .takes
        .iter()
        .map(|(_, t)| t.regions[0].source_offset)
        .collect();
    assert_eq!(offsets, vec![0, 2 * len]);
    assert_eq!(l.takes[1].0, "Take 3");
    // Every pass discarded: nothing to apply.
    c.fates = vec![PassFate::Discarded; 3];
    assert_eq!(c.run(), None);
}

#[test]
fn keep_protects_and_discard_drops_since_the_last_keep() {
    let mut f = Vec::new();
    decide(&mut f, 2, false);
    assert_eq!(f, vec![PassFate::Discarded; 2]);
    decide(&mut f, 3, true);
    assert_eq!(f[2], PassFate::Kept);
    decide(&mut f, 5, false);
    assert_eq!(
        f,
        vec![
            PassFate::Discarded,
            PassFate::Discarded,
            PassFate::Kept,
            PassFate::Discarded,
            PassFate::Discarded
        ]
    );
    // A pass playing when nothing completed is untouched.
    let mut g = Vec::new();
    decide(&mut g, 0, false);
    assert!(g.is_empty());
}

fn on(pitch: u8, vel: u8) -> Midi1Msg {
    Midi1Msg::three(0x90, pitch, vel)
}

fn off(pitch: u8) -> Midi1Msg {
    Midi1Msg::three(0x80, pitch, 64)
}

fn midi(lane: LaneId, run: i64, msg: Midi1Msg) -> RecordedMidi {
    RecordedMidi { lane, run, msg }
}

#[test]
fn midi_lands_where_it_was_played_and_replace_clears_the_range() {
    let d = doc(None);
    let mut c = Case::new(&d, 2 * BEAT);
    // Four beats were heard: the output ran on by the latency.
    c.end_run = 4 * BEAT + 256;
    c.latency = 256;
    c.midi_lanes = vec![d.keys];
    let r = |beats: f64| (beats * BEAT as f64) as i64 + 256;
    c.midi = vec![
        // Before the record start (pre-roll): dropped.
        midi(d.keys, r(-0.5), on(50, 100)),
        midi(d.keys, r(0.0), on(60, 127)),
        midi(d.keys, r(0.5), off(60)),
        midi(d.keys, r(1.0), Midi1Msg::three(0xB0, 1, 64)),
        // Held at Stop: it ends at the end of what was recorded.
        midi(d.keys, r(3.0), on(62, 1)),
    ];
    let p = c.run().unwrap();
    let l = p.lanes.iter().find(|l| l.lane == d.keys).unwrap();
    // Replace: the recorded range, from the record start to Stop.
    assert_eq!(l.clear, vec![(Tick(2 * Q), Tick(6 * Q))]);
    let got: Vec<(i64, String)> = l
        .active
        .events
        .iter()
        .map(|e| {
            let what = match &e.body {
                EventBody::Note {
                    pitch,
                    velocity,
                    release_velocity,
                    duration,
                    ..
                } => format!("note {pitch} {velocity} {release_velocity} {}", duration.0),
                EventBody::Cc { index, value } => format!("cc {index} {value:#x}"),
                other => format!("{other:?}"),
            };
            (e.tick.0, what)
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (2 * Q, format!("note 60 65535 32768 {}", Q / 2)),
            (3 * Q, "cc 1 0x80000000".to_string()),
            (5 * Q, format!("note 62 512 0 {Q}")),
        ]
    );
    // Overdub clears nothing.
    c.replace = false;
    let p = c.run().unwrap();
    assert!(p.lanes[0].clear.is_empty());
    // Replace with nothing played still clears the range.
    c.replace = true;
    c.midi.clear();
    let p = c.run().unwrap();
    assert_eq!(p.lanes[0].clear, vec![(Tick(2 * Q), Tick(6 * Q))]);
    assert!(p.lanes[0].active.events.is_empty());
}

#[test]
fn midi_loop_passes_make_takes_or_merge() {
    let d = doc(None);
    let len = 2 * BEAT;
    let mut c = Case::new(&d, 0);
    c.looped = true;
    c.end_run = 2 * len;
    c.range = RecordRange {
        start: 0,
        end: Some(len),
    };
    c.marks = vec![mark(0, 0, false), mark(len, 0, true)];
    c.midi_lanes = vec![d.keys];
    let half = (BEAT / 2) as i64;
    c.midi = vec![
        midi(d.keys, half, on(60, 100)),
        midi(d.keys, 2 * half, off(60)),
        // In pass 2, at the same place in the loop.
        midi(d.keys, len as i64 + half, on(64, 100)),
        midi(d.keys, len as i64 + 2 * half, off(64)),
    ];
    let p = c.run().unwrap();
    let l = &p.lanes[0];
    assert_eq!(l.takes.len(), 2);
    let pitch = |e: &core_model::event::EventSpec| match e.body {
        EventBody::Note { pitch, .. } => pitch,
        _ => 0,
    };
    assert_eq!(pitch(&l.takes[0].1.events[0]), 60);
    assert_eq!(pitch(&l.takes[1].1.events[0]), 64);
    assert_eq!(l.takes[1].1.events[0].tick, Tick(Q / 2));
    // Without passes as takes, MIDI passes merge into the active take.
    c.takes = false;
    let p = c.run().unwrap();
    let l = &p.lanes[0];
    assert!(l.takes.is_empty());
    let pitches: Vec<u8> = l.active.events.iter().map(pitch).collect();
    assert_eq!(pitches, vec![60, 64]);
}

#[test]
fn a_region_on_an_awkward_tempo_still_lands_on_its_frame() {
    // 97 BPM: frames per tick are not whole.
    let d = doc(Some(97));
    let start = 12_345;
    let mut c = audio_case(&d, start, 100_000);
    c.input_delay = 77;
    let p = c.run().unwrap();
    let (tick, offset, length) = active_regions(&p, d.mic)[0];
    let seq = d.doc.sequence(d.seq).unwrap();
    let t = TempoTable::build(&seq.tempo, Tick(0), 48_000.0);
    let at = t.tick_to_frame(Tick(tick));
    // File frame `offset` plays at frame `at`: the same pairing the
    // capture had (frame `start` ↔ file frame 77).
    assert_eq!(at as i64 - offset as i64, start as i64 - 77);
    assert!(at.abs_diff(start) <= 2, "{at} {start}");
    assert_eq!(offset + length, 100_000);
}

#[test]
fn nothing_armed_or_recorded_is_no_pass() {
    let d = doc(None);
    let c = Case::new(&d, 0);
    assert_eq!(c.run(), None);
    let mut c = Case::new(&d, 0);
    c.marks.clear();
    c.audio = vec![AudioTake {
        lane: d.mic,
        media: file(1000),
    }];
    assert_eq!(c.run(), None, "no marks: the transport never rolled");
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

    /// Whatever the marks, delays and ranges: every region lies inside
    /// its file and starts inside the record range.
    #[test]
    fn regions_stay_inside_their_file_and_range(
        len in 1_000u64..50_000,
        passes in 1u64..5,
        delay in -500i64..2_000,
        latency in 0u64..1_024,
        start in 0u64..60_000,
        span in proptest::option::of(1u64..60_000),
        looped in proptest::bool::ANY,
    ) {
        let d = doc(None);
        let frames = passes * len;
        let mut c = audio_case(&d, start, frames);
        c.looped = looped;
        c.latency = latency;
        c.input_delay = delay;
        c.end_run = frames;
        c.range = RecordRange { start, end: span.map(|s| start + s) };
        c.marks = (0..passes).map(|k| mark(k * len, start, k > 0)).collect();
        if let Some(p) = c.run() {
            for l in &p.lanes {
                let all = l.active.regions.iter().chain(l.takes.iter().flat_map(|(_, t)| t.regions.iter()));
                for r in all {
                    proptest::prop_assert!(r.length > 0);
                    proptest::prop_assert!(r.source_offset + r.length <= frames);
                    proptest::prop_assert!(r.tick.0 >= 0);
                }
            }
        }
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test passes 2>&1 | tail -5
```
Expected: does not compile: `vizdaw_ffi::passes` does not exist.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/passes.rs <<'VIZDAW_EOF'
//! Placing a recording (sub-project 5 spec §6.3–§6.5; plan 5c Rulings
//! 11–17): turns what the engine captured (input files, marks, recorded
//! MIDI) into the [`RecordPass`] the model applies at Stop. Pure: no I/O,
//! no locks, no engine.
//!
//! **Heard runs.** The renderer counts output frames from the roll (run
//! frames) and marks which schedule frame each run plays after the roll,
//! every loop wrap and every resync. The output played at run `x` is heard
//! at run `x + L` (the graph latency), so the timeline position heard at
//! run `u` is `S(u)`, where `S` maps runs to schedule frames through the
//! marks. A MIDI message recorded at output run `r` was played against
//! `S(r − L)`; input file frame `k` was captured while `S(k − D)` was
//! heard, `D` being the device's input delay minus the latency trim plus
//! `L`.
//!
//! **Passes.** The roll and every wrap start a pass. A pass's window is the
//! timeline it played, inside the record range. Discarded passes are
//! dropped; every other pass with something in its window is kept (plan
//! 5c Ruling 13).

use core_engine::midi1::Midi1Msg;
use core_engine::record::{Mark, RecordedMidi};
use core_engine::schedule::TempoTable;
use core_model::command::{LaneRecord, RecordPass, RecordedContent, RecordedMedia, RecordedRegion};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{LaneId, SeqId};
use core_model::time::Tick;
use std::collections::BTreeMap;

/// The record range in schedule frames (spec §6.4): the punch range, the
/// loop range (narrowed by the punch), or from where recording started on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordRange {
    /// First frame.
    pub start: u64,
    /// One past the last frame; `None`: until Stop.
    pub end: Option<u64>,
}

impl RecordRange {
    fn clip(&self, a: i64, b: i64) -> Option<(i64, i64)> {
        let lo = a.max(i64::try_from(self.start).unwrap_or(i64::MAX));
        let hi = match self.end {
            Some(e) => b.min(i64::try_from(e).unwrap_or(i64::MAX)),
            None => b,
        };
        (lo < hi).then_some((lo, hi))
    }
}

/// What the user decided about a pass (`record.keep_pass`,
/// `record.discard_pass`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PassFate {
    /// Nothing: it is kept at Stop.
    #[default]
    Undecided,
    /// Kept: a later discard leaves it.
    Kept,
    /// Dropped.
    Discarded,
}

/// Applies a keep (`keep`) or a discard to the passes that completed:
/// a keep keeps every undecided one, a discard drops every undecided one
/// (those since the last keep). The pass still playing is not touched
/// (plan 5c Ruling 13).
pub fn decide(fates: &mut Vec<PassFate>, completed: usize, keep: bool) {
    if fates.len() < completed {
        fates.resize(completed, PassFate::Undecided);
    }
    for f in fates.iter_mut().take(completed) {
        if *f == PassFate::Undecided {
            *f = if keep {
                PassFate::Kept
            } else {
                PassFate::Discarded
            };
        }
    }
}

/// One armed audio lane's finished file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTake {
    /// The lane.
    pub lane: LaneId,
    /// The file (package-relative path, format, frames, hash, name, pass).
    pub media: RecordedMedia,
}

/// Everything a finished recording captured.
#[derive(Debug, Clone, PartialEq)]
pub struct Captured<'a> {
    /// The document as it is at Stop.
    pub doc: &'a Document,
    /// The sequence recorded into.
    pub seq: SeqId,
    /// The marks, oldest first.
    pub marks: &'a [Mark],
    /// Output run frames played until the tap closed.
    pub end_run: u64,
    /// The graph latency `L` (frames).
    pub latency: u64,
    /// The device's input delay minus the latency trim (frames).
    pub input_delay: i64,
    /// The record range.
    pub range: RecordRange,
    /// Loop recording: every wrap ends a pass.
    pub looped: bool,
    /// Replace (else overdub).
    pub replace: bool,
    /// Kept passes become takes (spec §6.4).
    pub passes_as_takes: bool,
    /// Each pass's fate, by pass number (missing: undecided).
    pub fates: &'a [PassFate],
    /// The armed audio lanes' files.
    pub audio: &'a [AudioTake],
    /// The armed MIDI lanes.
    pub midi_lanes: &'a [LaneId],
    /// What they recorded.
    pub midi: &'a [RecordedMidi],
    /// Input was lost somewhere (silence was written).
    pub with_gaps: bool,
}

/// A stretch of runs `[run, end)` that played schedule frames from
/// `frame` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    pass: usize,
    run: i64,
    end: i64,
    frame: i64,
}

impl Piece {
    fn at(&self, u: i64) -> i64 {
        self.frame + (u - self.run)
    }
}

/// The pieces the marks describe, numbered by pass: marks on one run keep
/// the last frame (and a wrap); the first piece reaches back before the
/// roll (pre-roll handles); the last ends at heard run `end`.
fn pieces(marks: &[Mark], end: i64) -> Vec<Piece> {
    let mut m: Vec<Mark> = Vec::with_capacity(marks.len());
    for x in marks {
        match m.last_mut() {
            Some(last) if last.run == x.run => {
                last.frame = x.frame;
                last.wrap |= x.wrap;
            }
            _ => m.push(*x),
        }
    }
    let mut out: Vec<Piece> = Vec::with_capacity(m.len());
    let mut pass = 0usize;
    for (i, x) in m.iter().enumerate() {
        if i > 0 && x.wrap {
            pass += 1;
        }
        let at = i64::try_from(x.run).unwrap_or(i64::MAX);
        let run = if i == 0 { i64::MIN / 4 } else { at };
        let next = m
            .get(i + 1)
            .map_or(end.max(at), |n| i64::try_from(n.run).unwrap_or(i64::MAX));
        let frame = i64::try_from(x.frame).unwrap_or(i64::MAX);
        out.push(Piece {
            pass,
            run,
            end: next,
            // The frame at `run`: a piece reaching back keeps its slope.
            frame: frame - (at - run),
        });
    }
    out
}

fn piece_of(pieces: &[Piece], u: i64) -> Option<&Piece> {
    pieces.iter().find(|p| u >= p.run && u < p.end)
}

/// The timeline `[t0, t1)` piece `p` played, inside the range.
fn window(p: &Piece, range: &RecordRange) -> Option<(i64, i64)> {
    range.clip(p.at(p.run), p.at(p.end))
}

/// MIDI 2.0 min-center-max upscale of a `bits`-bit value to 32 bits.
fn up32(v: u32, bits: u32) -> u32 {
    let scale = 32 - bits;
    let shifted = v << scale;
    let center = 1u32 << (bits - 1);
    if v <= center {
        return shifted;
    }
    let repeat_bits = bits - 1;
    let mask = (1u32 << repeat_bits) - 1;
    let mut repeat = v & mask;
    repeat = if scale > repeat_bits {
        repeat << (scale - repeat_bits)
    } else {
        repeat >> (repeat_bits - scale)
    };
    let mut out = shifted;
    while repeat != 0 {
        out |= repeat;
        repeat >>= repeat_bits;
    }
    out
}

/// The event a non-note channel message records as.
fn body_of(msg: Midi1Msg) -> Option<EventBody> {
    let [s, a, b] = msg.bytes;
    Some(match s & 0xF0 {
        0xA0 => EventBody::PolyPressure {
            pitch: a & 0x7F,
            value: up32(u32::from(b & 0x7F), 7),
        },
        0xB0 => EventBody::Cc {
            index: a & 0x7F,
            value: up32(u32::from(b & 0x7F), 7),
        },
        0xC0 => EventBody::Program {
            program: a & 0x7F,
            bank: None,
        },
        0xD0 => EventBody::ChannelPressure {
            value: up32(u32::from(a & 0x7F), 7),
        },
        0xE0 => EventBody::PitchBend {
            value: up32(u32::from(a & 0x7F) | (u32::from(b & 0x7F) << 7), 14),
        },
        _ => return None,
    })
}

/// One recorded message in heard runs.
#[derive(Debug, Clone, Copy)]
struct Heard {
    u: i64,
    msg: Midi1Msg,
}

/// A lane's MIDI per pass: events placed on the timeline, notes paired
/// with their offs (cut at the pass window's end).
fn midi_events(
    heard: &[Heard],
    pieces: &[Piece],
    range: &RecordRange,
    tempo: &TempoTable,
) -> BTreeMap<usize, Vec<EventSpec>> {
    let mut out: BTreeMap<usize, Vec<EventSpec>> = BTreeMap::new();
    let tick = |f: i64| tempo.frame_to_tick(u64::try_from(f.max(0)).unwrap_or(0));
    for (i, h) in heard.iter().enumerate() {
        let Some(p) = piece_of(pieces, h.u) else {
            continue;
        };
        let Some((t0, t1)) = window(p, range) else {
            continue;
        };
        let t = p.at(h.u);
        if t < t0 || t >= t1 {
            continue;
        }
        let [s, pitch, vel] = h.msg.bytes;
        let body = match s & 0xF0 {
            0x90 if vel > 0 => {
                let ch = s & 0x0F;
                // The off: the next note-off of this pitch and channel in
                // the same pass; else the window's end.
                let off = heard.iter().skip(i + 1).find(|o| {
                    let [os, op, ov] = o.msg.bytes;
                    os & 0x0F == ch
                        && op == pitch
                        && (os & 0xF0 == 0x80 || (os & 0xF0 == 0x90 && ov == 0))
                });
                let (end, release) = match off.and_then(|o| piece_of(pieces, o.u).map(|q| (o, q))) {
                    Some((o, q)) if q.pass == p.pass => (
                        q.at(o.u).min(t1),
                        crate::input::upscale_velocity(o.msg.bytes[2]),
                    ),
                    _ => (t1, 0),
                };
                let mut b = EventBody::note(
                    pitch & 0x7F,
                    crate::input::upscale_velocity(vel),
                    Tick((tick(end).0 - tick(t).0).max(0)),
                );
                if let EventBody::Note {
                    release_velocity, ..
                } = &mut b
                {
                    *release_velocity = release;
                }
                b
            }
            0x80 | 0x90 => continue,
            _ => match body_of(h.msg) {
                Some(b) => b,
                None => continue,
            },
        };
        out.entry(p.pass)
            .or_default()
            .push(EventSpec::new(tick(t), body));
    }
    out
}

/// A lane's audio regions per pass: one per piece, joined when pieces
/// continue each other, cut to the file and the window.
fn audio_regions(
    frames: u64,
    pieces: &[Piece],
    range: &RecordRange,
    d: i64,
    tempo: &TempoTable,
) -> BTreeMap<usize, Vec<RecordedRegion>> {
    let mut out: BTreeMap<usize, Vec<RecordedRegion>> = BTreeMap::new();
    let len = i64::try_from(frames).unwrap_or(i64::MAX);
    let mut last: Option<(usize, i64, i64)> = None; // (pass, timeline end, file end)
    for p in pieces {
        let Some((t0, t1)) = window(p, range) else {
            continue;
        };
        // Heard run → file frame: k = u + D.
        let k_at = |t: i64| (t - p.frame) + p.run + d;
        let (mut t0, mut k0) = (t0, k_at(t0));
        if k0 < 0 {
            t0 -= k0;
            k0 = 0;
        }
        let t1 = t1.min(t0 + (len - k0));
        if t1 <= t0 {
            continue;
        }
        let regions = out.entry(p.pass).or_default();
        let joined = match (last, regions.last_mut()) {
            (Some((pass, te, ke)), Some(r)) if pass == p.pass && te == t0 && ke == k0 => {
                r.length += u64::try_from(t1 - t0).unwrap_or(0);
                true
            }
            _ => false,
        };
        if !joined {
            // The region starts on a tick: the file offset follows the
            // frame that tick plays at, so the audio lands exactly.
            let tick = tempo.frame_to_tick(u64::try_from(t0).unwrap_or(0));
            let f = i64::try_from(tempo.tick_to_frame(tick)).unwrap_or(t0);
            let (start, k) = if k0 + (f - t0) >= 0 {
                (f, k0 + (f - t0))
            } else {
                (t0, k0)
            };
            let length = t1 - start;
            if length <= 0 {
                continue;
            }
            regions.push(RecordedRegion {
                media: 0,
                tick,
                source_offset: u64::try_from(k).unwrap_or(0),
                length: u64::try_from(length).unwrap_or(0),
            });
        }
        last = Some((p.pass, t1, k0 + (t1 - t0)));
    }
    out
}

/// The pass to apply, or `None` when nothing was recorded.
pub fn plan(c: &Captured<'_>) -> Result<Option<RecordPass>, String> {
    let seq = c.doc.sequence(c.seq).map_err(|e| e.to_string())?;
    let rate = f64::from(c.doc.settings().format.sample_rate);
    let tempo = TempoTable::build(&seq.tempo, Tick(-seq.start_offset.0), rate);
    let lat = i64::try_from(c.latency).unwrap_or(0);
    let heard_end = i64::try_from(c.end_run).unwrap_or(i64::MAX) - lat;
    let pieces = pieces(c.marks, heard_end);
    if pieces.is_empty() {
        return Ok(None);
    }
    let d = c.input_delay.saturating_add(lat);
    let fate = |pass: usize| c.fates.get(pass).copied().unwrap_or_default();
    let passes = pieces.last().map_or(0, |p| p.pass + 1);
    let kept: Vec<usize> = (0..passes)
        .filter(|p| fate(*p) != PassFate::Discarded)
        .filter(|p| {
            pieces
                .iter()
                .any(|x| x.pass == *p && window(x, &c.range).is_some())
        })
        .collect();
    let windows: Vec<(Tick, Tick)> = pieces
        .iter()
        .filter(|p| kept.contains(&p.pass))
        .filter_map(|p| window(p, &c.range))
        .map(|(a, b)| {
            let t = |f: i64| tempo.frame_to_tick(u64::try_from(f.max(0)).unwrap_or(0));
            (t(a), t(b))
        })
        .filter(|(a, b)| a < b)
        .collect();
    let takes_from_passes = c.looped && c.passes_as_takes;
    let mut media = Vec::new();
    let mut lanes: BTreeMap<LaneId, LaneRecord> = BTreeMap::new();
    let lane_record = |lane: LaneId| LaneRecord {
        seq: c.seq,
        lane,
        clear: Vec::new(),
        active: RecordedContent::default(),
        takes: Vec::new(),
    };
    let take_name = |lane: LaneId, k: usize| {
        let n = seq.lane(lane).map_or(1, |l| l.takes.len()) + 1 + k;
        format!("Take {n}")
    };
    for a in c.audio {
        let mut regions = audio_regions(a.media.length_frames, &pieces, &c.range, d, &tempo);
        regions.retain(|p, _| kept.contains(p));
        if regions.is_empty() {
            continue;
        }
        let index = media.len();
        media.push(a.media.clone());
        let rec = lanes.entry(a.lane).or_insert_with(|| lane_record(a.lane));
        if c.looped {
            for (k, p) in kept.iter().enumerate() {
                let mut content = RecordedContent::default();
                for r in regions.get(p).into_iter().flatten() {
                    content.regions.push(RecordedRegion { media: index, ..*r });
                }
                rec.takes.push((take_name(a.lane, k), content));
            }
        } else {
            for r in regions.values().flatten() {
                rec.active
                    .regions
                    .push(RecordedRegion { media: index, ..*r });
            }
        }
    }
    for lane in c.midi_lanes {
        let heard: Vec<Heard> = c
            .midi
            .iter()
            .filter(|m| m.lane == *lane)
            .map(|m| Heard {
                u: m.run - lat,
                msg: m.msg,
            })
            .collect();
        let mut events = midi_events(&heard, &pieces, &c.range, &tempo);
        events.retain(|p, _| kept.contains(p));
        if takes_from_passes {
            if kept.is_empty() {
                continue;
            }
            let rec = lanes.entry(*lane).or_insert_with(|| lane_record(*lane));
            for (k, p) in kept.iter().enumerate() {
                let content = RecordedContent {
                    events: events.remove(p).unwrap_or_default(),
                    regions: Vec::new(),
                };
                rec.takes.push((take_name(*lane, k), content));
            }
        } else {
            let all: Vec<EventSpec> = events.into_values().flatten().collect();
            if all.is_empty() && !c.replace {
                continue;
            }
            let rec = lanes.entry(*lane).or_insert_with(|| lane_record(*lane));
            if c.replace {
                rec.clear = windows.clone();
            }
            rec.active.events = all;
        }
    }
    let lanes: Vec<LaneRecord> = lanes
        .into_values()
        .filter(|l| !l.clear.is_empty() || !l.active.is_empty() || !l.takes.is_empty())
        .collect();
    if lanes.is_empty() {
        return Ok(None);
    }
    Ok(Some(RecordPass {
        with_gaps: c.with_gaps,
        media,
        lanes,
    }))
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod mixer;
pub mod modify;
pub mod overviews;
pub mod plugins;
pub mod recovery;
pub mod relink;
```
Replace with:
```rust
pub mod mixer;
pub mod modify;
pub mod overviews;
pub mod passes;
pub mod plugins;
pub mod recovery;
pub mod relink;
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test passes 2>&1 | tail -3
```
Expected: `test result: ok. 10 passed`, including the proptest `regions_stay_inside_their_file_and_range` and `a_region_on_an_awkward_tempo_still_lands_on_its_frame` (Review Focus 4).

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/passes.rs crates/core-ffi/src/lib.rs crates/core-ffi/tests/passes.rs
git commit -F - <<'VIZDAW_MSG'
core-ffi: the recording pass planner

A pure planner maps run frames through the marks, the input delay,
the latency trim and the graph latency to timeline frames, cuts each
loop pass to the record range (pre-roll kept as a handle), and builds
one RecordPass: replace or overdub into the active take, or a take per
kept pass.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 8: `core-ffi` — lane inputs, the fake device and MIDI injection over the FFI

**Files:**
- Create: `crates/core-ffi/src/live.rs`
- Modify: `crates/core-ffi/src/edit.rs` (`LaneInputRow`, `MonitorModeRow`, `LaneEdit::{SetInput, SetMonitor}`)
- Modify: `crates/core-ffi/src/snapshot.rs` (`LaneRow.input`, `LaneRow.monitor`)
- Modify: `crates/core-ffi/src/host.rs` (`set_fake_input`, `InputInfo`, the engine's input spec at every load)
- Modify: `crates/core-ffi/src/session.rs` (`input_info`, `inject_midi`)
- Modify: `crates/core-ffi/src/lib.rs`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/live_input.rs`

**Interfaces:**
- Consumes: Tasks 2, 5 and 6.
- Produces:
  - UniFFI `LaneInputRow { Audio { first, stereo }, Midi { port, channel } }`, `MonitorModeRow`, `LaneEdit::SetInput { seq, lane, input: Option<LaneInputRow> }`, `LaneEdit::SetMonitor { seq, lane, mode }`; `LaneRow.input`, `LaneRow.monitor`;
  - `InputInfoRow { device, channels, names, input_latency_frames, output_latency_frames }`; `Session.input_info()`, `Session.inject_midi(port, bytes)` (`live::parse_injection`, at most `MAX_INJECT_BYTES` 1024, channel messages only);
  - `host::set_fake_input(Option<FakeInput>)` (test hook) and the environment's `VIZDAW_AUDIO_LOOPBACK` / `VIZDAW_AUDIO_INPUT=fake` (Ruling 7).

The host sends the engine its input routes from the loaded document at every load. The fake input is chosen per test, never from the environment (Global Constraints, TCC).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/live_input.rs <<'VIZDAW_EOF'
//! Live input through a session (plan 5c Task 8; sub-project 5 spec
//! §4.5, §6.6, §12.1): lane inputs and monitor modes as undoable lane
//! edits shown in the lane rows; MIDI injected as if a source sent it
//! plays thru to an armed lane's destination; hostile injections are
//! refused; the offline device's fake input is reported and a monitored
//! input is heard through its path's strip. The engine host is
//! process-wide: one test at a time, all offline (no audio device, no
//! microphone permission).

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{drain_offline_log, force_offline, reset_host, set_fake_input};
use vizdaw_ffi::{
    EditCommand, LaneEdit, LaneInputRow, LaneRow, MonitorModeRow, Session, StripKindRow, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial(input: Option<FakeInput>) -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
    reset_host();
    g
}

fn song(s: &Session) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id
}

fn lane(s: &Session, seq: u64, name: &str) -> LaneRow {
    s.lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.name == name)
        .unwrap()
}

fn edit(s: &Session, e: LaneEdit) -> Result<(), VizError> {
    s.edit(EditCommand::Lane { edit: e }, None).map(|_| ())
}

fn arm(s: &Session, seq: u64, lane: u64) {
    edit(
        s,
        LaneEdit::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
}

/// Plays and stops, so the session holds the engine.
fn take_engine(s: &Session, seq: u64) {
    s.play(seq).unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never played");
        std::thread::sleep(Duration::from_millis(2));
    }
    s.stop().unwrap();
}

#[test]
fn inputs_and_monitor_modes_are_lane_edits() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords");
    assert_eq!(
        (chords.input.clone(), chords.monitor),
        (None, MonitorModeRow::Auto)
    );
    let midi = LaneInputRow::Midi {
        port: Some("Keys".into()),
        channel: None,
    };
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: chords.id,
            input: Some(midi.clone()),
        },
    )
    .unwrap();
    edit(
        &s,
        LaneEdit::SetMonitor {
            seq,
            lane: chords.id,
            monitor: MonitorModeRow::Off,
        },
    )
    .unwrap();
    let row = lane(&s, seq, "Chords");
    assert_eq!((row.input, row.monitor), (Some(midi), MonitorModeRow::Off));
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Lane Monitor")
    );
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(lane(&s, seq, "Chords").input, None);
    // Out-of-range channels and MIDI channels are refused, nothing changes.
    for bad in [
        LaneInputRow::Audio {
            first: 300,
            stereo: false,
        },
        LaneInputRow::Audio {
            first: 255,
            stereo: true,
        },
        LaneInputRow::Midi {
            port: None,
            channel: Some(16),
        },
        LaneInputRow::Midi {
            port: Some(String::new()),
            channel: None,
        },
    ] {
        let r = edit(
            &s,
            LaneEdit::SetInput {
                seq,
                lane: chords.id,
                input: Some(bad.clone()),
            },
        );
        assert!(matches!(r, Err(VizError::Model { .. })), "{bad:?}: {r:?}");
    }
    assert_eq!(lane(&s, seq, "Chords").input, None);
    s.close().unwrap();
}

#[test]
fn injected_midi_plays_thru_to_an_armed_lanes_destination() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    // No engine yet: nothing to send to.
    assert!(!s.inject_midi("Keys".into(), vec![0x90, 70, 99]).unwrap());
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: chords,
            input: Some(LaneInputRow::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
    )
    .unwrap();
    arm(&s, seq, chords);
    take_engine(&s, seq);
    std::thread::sleep(Duration::from_millis(50));
    drain_offline_log();
    // Another source: not the lane's.
    assert!(s.inject_midi("Pads".into(), vec![0x90, 71, 99]).unwrap());
    // Played on channel 2: thru plays it on the piano's channel 1.
    assert!(
        s.inject_midi("Keys".into(), vec![0x91, 70, 99, 0x81, 70, 0])
            .unwrap()
    );
    let until = Instant::now() + common::wait_limit();
    let mut log = Vec::new();
    while !log
        .iter()
        .any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x80, 70, 0])
    {
        assert!(Instant::now() < until, "thru never arrived: {log:?}");
        std::thread::sleep(Duration::from_millis(5));
        log.extend(drain_offline_log());
    }
    assert!(log.iter().any(|(_, b)| b == &vec![0x90, 70, 99]), "{log:?}");
    assert!(!log.iter().any(|(_, b)| b.get(1) == Some(&71)), "{log:?}");
    s.close().unwrap();
}

#[test]
fn hostile_injections_are_refused() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    take_engine(&s, song(&s));
    let long = vec![0x90u8; 1025];
    for (port, bytes) in [
        ("Keys", vec![]),
        ("Keys", long),
        ("Keys", vec![0xF0, 0x7E, 0xF7]),
        ("Keys", vec![0xF8]),
        ("Keys", vec![0x90, 60]),
        ("Keys", vec![0x90, 60, 200]),
        ("Keys", vec![60, 100]),
        ("", vec![0x90, 60, 100]),
        ("a\u{0}b", vec![0x90, 60, 100]),
    ] {
        let r = s.inject_midi(port.into(), bytes.clone());
        assert!(
            matches!(r, Err(VizError::Model { .. })),
            "{port:?} {bytes:?}: {r:?}"
        );
    }
    s.close().unwrap();
}

#[test]
fn the_fake_device_reports_its_input() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    assert_eq!(s.input_info().unwrap().channels, 0, "no engine yet");
    take_engine(&s, song(&s));
    let i = s.input_info().unwrap();
    assert_eq!(i.device, "Offline");
    assert_eq!(i.channels, 3);
    assert_eq!(i.names, vec!["Fake In 1", "Fake In 2", "Fake In 3"]);
    assert_eq!((i.input_latency_frames, i.output_latency_frames), (0, 0));
    s.close().unwrap();
    // A loopback device splits its delay between the directions.
    set_fake_input(Some(FakeInput::Loopback(1024)));
    reset_host();
    let s = Session::new_demo().unwrap();
    take_engine(&s, song(&s));
    let i = s.input_info().unwrap();
    assert_eq!(
        (i.input_latency_frames, i.output_latency_frames),
        (512, 512)
    );
    s.close().unwrap();
    // No fake input: none.
    set_fake_input(None);
    reset_host();
    let s = Session::new_demo().unwrap();
    take_engine(&s, song(&s));
    assert_eq!(s.input_info().unwrap().channels, 0);
    s.close().unwrap();
}

/// The audio path strip's peak over about 100 ms.
fn path_peak(s: &Session, strip: u64) -> f32 {
    let _ = s.levels().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    s.levels()
        .unwrap()
        .meters
        .iter()
        .filter(|m| m.strip == strip)
        .map(|m| m.peak_l.max(m.peak_r))
        .fold(0.0, f32::max)
}

#[test]
fn a_monitored_input_is_heard_through_its_path() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = lane(&s, seq, "Loop").id;
    let strip = s
        .mixer()
        .unwrap()
        .strips
        .iter()
        .find(|x| matches!(x.kind, StripKindRow::AudioPath { .. }))
        .unwrap()
        .id;
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: lp,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
            }),
        },
    )
    .unwrap();
    edit(
        &s,
        LaneEdit::SetMonitor {
            seq,
            lane: lp,
            monitor: MonitorModeRow::Input,
        },
    )
    .unwrap();
    take_engine(&s, seq);
    // The fake sine (−12 dBFS) through the strip.
    let until = Instant::now() + common::wait_limit();
    while path_peak(&s, strip) < 0.05 {
        assert!(Instant::now() < until, "the input was never heard");
    }
    edit(
        &s,
        LaneEdit::SetMonitor {
            seq,
            lane: lp,
            monitor: MonitorModeRow::Off,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while path_peak(&s, strip) > 0.001 {
        assert!(Instant::now() < until, "Off still heard the input");
    }
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test live_input 2>&1 | tail -5
```
Expected: does not compile: `LaneInputRow`, `Session::inject_midi` and `host::set_fake_input` do not exist.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/live.rs <<'VIZDAW_EOF'
//! Live input over the FFI (plan 5c Task 8; sub-project 5 spec §6.1,
//! §6.6, §12.1): what the engine's input offers, and MIDI bytes injected
//! as if a named source sent them (`midi.inject`), checked before they
//! reach the real-time queue.

use crate::error::VizError;
use core_engine::input::channel_message;
use core_engine::midi1::Midi1Msg;

/// The most bytes one injection may carry.
pub const MAX_INJECT_BYTES: usize = 1024;

/// What the audio input offers (`state.get input`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InputInfoRow {
    /// The device ("Offline" for the offline backend; "" before the first
    /// Play).
    pub device: String,
    /// Input channels (0: no input).
    pub channels: u32,
    /// Their names.
    pub names: Vec<String>,
    /// Input latency in frames.
    pub input_latency_frames: u32,
    /// Output latency in frames.
    pub output_latency_frames: u32,
}

/// The channel messages in `bytes`, one after another (running status,
/// SysEx and system messages are refused), from source `port` (a name:
/// not empty, at most 255 bytes, no control characters).
pub fn parse_injection(port: &str, bytes: &[u8]) -> Result<Vec<Midi1Msg>, VizError> {
    if port.is_empty() || port.len() > 255 || port.chars().any(char::is_control) {
        return Err(crate::events::bad("`port` must name a MIDI source"));
    }
    if bytes.is_empty() || bytes.len() > MAX_INJECT_BYTES {
        return Err(crate::events::bad(&format!(
            "`bytes` must hold 1 to {MAX_INJECT_BYTES} bytes"
        )));
    }
    let mut out = Vec::new();
    let mut rest = bytes;
    while let Some(&status) = rest.first() {
        let len = match status & 0xF0 {
            0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 3,
            0xC0 | 0xD0 => 2,
            _ => {
                return Err(crate::events::bad(&format!(
                    "byte {status:#04x} does not start a channel message"
                )));
            }
        };
        let (msg, tail) = rest
            .split_at_checked(len)
            .ok_or_else(|| crate::events::bad("a message is cut short"))?;
        let m =
            channel_message(msg).ok_or_else(|| crate::events::bad("a data byte is above 0x7f"))?;
        out.push(m);
        rest = tail;
    }
    Ok(out)
}
VIZDAW_EOF
```

In `crates/core-ffi/src/edit.rs`:

Find:
```rust
        /// Take.
        take: u64,
    },
}

/// Mirrors `core_model::command::SequenceCmd`.
```
Replace with:
```rust
        /// Take.
        take: u64,
    },
    /// Set what the lane records (sub-project 5 spec §4.5).
    SetInput {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// The input, or none.
        input: Option<LaneInputRow>,
    },
    /// Set when the lane's input is heard (spec §6.6).
    SetMonitor {
        /// Sequence.
        seq: u64,
        /// Lane.
        lane: u64,
        /// The mode.
        monitor: MonitorModeRow,
    },
}

/// What a lane records (mirrors `core_model::lane::LaneInput`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum LaneInputRow {
    /// Device input channel `first` (0-based), and the next one too when
    /// `stereo`.
    Audio {
        /// The first (left) channel.
        first: u16,
        /// A pair.
        stereo: bool,
    },
    /// MIDI.
    Midi {
        /// The source's name; none: every source.
        port: Option<String>,
        /// The channel 0–15; none: every channel.
        channel: Option<u8>,
    },
}

impl From<&LaneInputRow> for core_model::lane::LaneInput {
    fn from(r: &LaneInputRow) -> Self {
        use core_model::lane::{InputChannels, LaneInput};
        match r {
            LaneInputRow::Audio { first, stereo } => LaneInput::Audio {
                channels: if *stereo {
                    InputChannels::Stereo(*first)
                } else {
                    InputChannels::Mono(*first)
                },
            },
            LaneInputRow::Midi { port, channel } => LaneInput::Midi {
                port: port.clone(),
                channel: *channel,
            },
        }
    }
}

impl From<&core_model::lane::LaneInput> for LaneInputRow {
    fn from(i: &core_model::lane::LaneInput) -> Self {
        use core_model::lane::{InputChannels, LaneInput};
        match i {
            LaneInput::Audio { channels } => LaneInputRow::Audio {
                first: channels.first(),
                stereo: matches!(channels, InputChannels::Stereo(_)),
            },
            LaneInput::Midi { port, channel } => LaneInputRow::Midi {
                port: port.clone(),
                channel: *channel,
            },
        }
    }
}

/// When a lane's input is heard (mirrors `core_model::lane::MonitorMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MonitorModeRow {
    /// Armed and stopped, or recording inside the record range.
    Auto,
    /// Always.
    Input,
    /// Never.
    Off,
}

impl From<MonitorModeRow> for core_model::lane::MonitorMode {
    fn from(m: MonitorModeRow) -> Self {
        use core_model::lane::MonitorMode;
        match m {
            MonitorModeRow::Auto => MonitorMode::Auto,
            MonitorModeRow::Input => MonitorMode::Input,
            MonitorModeRow::Off => MonitorMode::Off,
        }
    }
}

impl From<core_model::lane::MonitorMode> for MonitorModeRow {
    fn from(m: core_model::lane::MonitorMode) -> Self {
        use core_model::lane::MonitorMode;
        match m {
            MonitorMode::Auto => MonitorModeRow::Auto,
            MonitorMode::Input => MonitorModeRow::Input,
            MonitorMode::Off => MonitorModeRow::Off,
        }
    }
}

/// Mirrors `core_model::command::SequenceCmd`.
```

Find:
```rust
            lane: LaneId(*lane),
            take: TakeId(*take),
        },
    }
}
```
Replace with:
```rust
            lane: LaneId(*lane),
            take: TakeId(*take),
        },
        LaneEdit::SetInput { seq, lane, input } => LaneCmd::SetInput {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            input: input.as_ref().map(Into::into),
        },
        LaneEdit::SetMonitor { seq, lane, monitor } => LaneCmd::SetMonitor {
            seq: SeqId(*seq),
            lane: LaneId(*lane),
            monitor: (*monitor).into(),
        },
    }
}
```

In `crates/core-ffi/src/snapshot.rs`:

Find:
```rust
    pub midi_dest: Option<u64>,
    /// The lane's audio output path ID.
    pub audio_out: Option<u64>,
}

/// What the transport is doing.
```
Replace with:
```rust
    pub midi_dest: Option<u64>,
    /// The lane's audio output path ID.
    pub audio_out: Option<u64>,
    /// What the lane records (plan 5c).
    pub input: Option<crate::edit::LaneInputRow>,
    /// When its input is heard (plan 5c).
    pub monitor: crate::edit::MonitorModeRow,
}

/// What the transport is doing.
```

Find:
```rust
                arm: l.arm,
                midi_dest: l.midi_dest.map(|d| d.0),
                audio_out: l.audio_out.map(|o| o.0),
            }
        })
        .collect())
```
Replace with:
```rust
                arm: l.arm,
                midi_dest: l.midi_dest.map(|d| d.0),
                audio_out: l.audio_out.map(|o| o.0),
                input: l.input.as_ref().map(Into::into),
                monitor: l.monitor.into(),
            }
        })
        .collect())
```

In `crates/core-ffi/src/host.rs`:

Find:
```rust
use core_engine::backend::{AudioBackend, BackendEvent};
use core_engine::engine::{Engine, EngineConfig, PendingPlay};
use core_engine::graph::NoInserts;
use core_engine::offline::{EventLog, OfflineBackend, OfflineSinks};
use core_engine::render::Renderer;
use core_engine::schedule::CompileReport;
use core_engine::transport::PlayState;
use core_host::{IdleReport, PluginHost};
use core_model::document::Document;
use core_model::ids::{DestId, SeqId};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
```
Replace with:
```rust
use core_engine::backend::{AudioBackend, BackendEvent};
use core_engine::engine::{Engine, EngineConfig, PendingPlay};
use core_engine::graph::NoInserts;
use core_engine::input::{InputSpec, MidiInputSpec, MidiSender, MonitorRoute, port_key};
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::{RecordHandles, RecordRequest};
use core_engine::render::Renderer;
use core_engine::schedule::CompileReport;
use core_engine::transport::PlayState;
use core_host::{IdleReport, PluginHost};
use core_model::document::Document;
use core_model::ids::{DestId, SeqId};
use core_model::lane::{LaneInput, MonitorMode};
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
```

Find:
```rust
static EPOCHS: AtomicU64 = AtomicU64::new(1);
/// New hosts' disks stall (the `stall_disk` test hook).
static STALL_DISK: AtomicBool = AtomicBool::new(false);

fn next_epoch() -> u64 {
    EPOCHS.fetch_add(1, Ordering::Relaxed)
```
Replace with:
```rust
static EPOCHS: AtomicU64 = AtomicU64::new(1);
/// New hosts' disks stall (the `stall_disk` test hook).
static STALL_DISK: AtomicBool = AtomicBool::new(false);
/// The fake input offline hosts get: `Some(choice)` set by the
/// `set_fake_input` test hook, `None` read from the environment.
static FAKE_INPUT: Mutex<Option<Option<FakeInput>>> = Mutex::new(None);

/// The fake input of offline hosts created from now on (spec §12.1):
/// `VIZDAW_AUDIO_LOOPBACK=<frames>` loops the output back, else
/// `VIZDAW_AUDIO_INPUT=fake` gives the test signals; neither is no input.
/// Tests choose with the `set_fake_input` hook instead. Never a device:
/// an offline host opens no audio hardware, so no microphone permission
/// is asked (plan 5c Global Constraints).
pub fn fake_input() -> Option<FakeInput> {
    if let Some(v) = *lock(&FAKE_INPUT) {
        return v;
    }
    if let Some(n) = std::env::var("VIZDAW_AUDIO_LOOPBACK")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
    {
        return Some(FakeInput::Loopback(n));
    }
    std::env::var("VIZDAW_AUDIO_INPUT")
        .is_ok_and(|v| v == "fake")
        .then_some(FakeInput::Signals)
}

/// Makes offline hosts created from now on use `input` (test hook; it
/// wins over the environment).
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}

/// What a host's audio input offers (spec §6.1, §12.1 `state.get input`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputInfo {
    /// Input channels (0: no input).
    pub channels: u32,
    /// Their names.
    pub names: Vec<String>,
    /// Input latency in frames.
    pub input_latency_frames: u32,
    /// Output latency in frames.
    pub output_latency_frames: u32,
}

/// What the lanes of `seq` listen to (spec §6.6): armed or Input-monitored
/// lanes with an input. MIDI lanes play thru unless their monitor is Off;
/// audio lanes are heard through their output path (the document's first
/// when they have none) unless it is Off.
pub fn input_spec(doc: &Document, seq: SeqId) -> InputSpec {
    let mut spec = InputSpec::default();
    let Ok(s) = doc.sequence(seq) else {
        return spec;
    };
    let first_out = doc.outputs().keys().next().copied();
    for l in &s.lanes {
        if !(l.arm || l.monitor == MonitorMode::Input) {
            continue;
        }
        match &l.input {
            Some(LaneInput::Midi { port, channel }) => spec.midi.push(MidiInputSpec {
                lane: l.id,
                port: port.as_deref().map(port_key),
                channel: *channel,
                dest: l.midi_dest,
                thru: l.monitor != MonitorMode::Off,
                armed: l.arm,
            }),
            Some(LaneInput::Audio { channels }) if l.monitor != MonitorMode::Off => {
                if let Some(path) = l.audio_out.or(first_out) {
                    spec.monitor.push(MonitorRoute {
                        path,
                        left: channels.first(),
                        right: channels.last().unwrap_or(channels.first()),
                        mode: l.monitor,
                        armed: l.arm,
                    });
                }
            }
            _ => {}
        }
    }
    spec
}

fn next_epoch() -> u64 {
    EPOCHS.fetch_add(1, Ordering::Relaxed)
```

Find:
```rust
}

impl OfflineDriver {
    fn start(renderer: Renderer, rate: f64) -> Result<OfflineDriver, VizError> {
        let mut backend = OfflineBackend::new(rate, OFFLINE_BLOCK);
        backend.start(renderer)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
```
Replace with:
```rust
}

impl OfflineDriver {
    fn start(
        renderer: Renderer,
        rate: f64,
        input: Option<FakeInput>,
    ) -> Result<(OfflineDriver, InputInfo), VizError> {
        let mut backend = OfflineBackend::new(rate, OFFLINE_BLOCK);
        if let Some(k) = input {
            backend = backend.with_input(k);
        }
        let info = backend.open(&core_engine::backend::AudioConfig::default())?;
        let inputs = InputInfo {
            channels: info.input_channels,
            names: info.input_channel_names,
            input_latency_frames: info.input_latency_frames,
            output_latency_frames: info.output_latency_frames,
        };
        backend.start(renderer)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
```

Find:
```rust
            .map_err(|e| VizError::Engine {
                message: format!("offline thread: {e}"),
            })?;
        Ok(OfflineDriver {
            stop,
            thread: Some(thread),
        })
    }
}
```
Replace with:
```rust
            .map_err(|e| VizError::Engine {
                message: format!("offline thread: {e}"),
            })?;
        Ok((
            OfflineDriver {
                stop,
                thread: Some(thread),
            },
            inputs,
        ))
    }
}
```

Find:
```rust
    /// Events queued by the `simulate_event` test hook.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    injected: Vec<BackendEvent>,
}

impl Host {
```
Replace with:
```rust
    /// Events queued by the `simulate_event` test hook.
    #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
    injected: Vec<BackendEvent>,
    /// What the output's input offers.
    inputs: InputInfo,
}

impl Host {
```

Find:
```rust
            let log = sinks.log();
            let rate = offline_rate(project);
            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
            let output = Output::Offline(OfflineDriver::start(renderer, rate)?);
            return Ok(Host::new(engine, output, OFFLINE_DEVICE, rate, Some(log)));
        }
        Host::create_live(project)
    }
```
Replace with:
```rust
            let log = sinks.log();
            let rate = offline_rate(project);
            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
            let (driver, inputs) = OfflineDriver::start(renderer, rate, fake_input())?;
            let mut h = Host::new(
                engine,
                Output::Offline(driver),
                OFFLINE_DEVICE,
                rate,
                Some(log),
            );
            h.inputs = inputs;
            return Ok(h);
        }
        Host::create_live(project)
    }
```

Find:
```rust
            faulted: std::collections::BTreeSet::new(),
            log,
            injected: Vec::new(),
        }
    }
```
Replace with:
```rust
            faulted: std::collections::BTreeSet::new(),
            log,
            injected: Vec::new(),
            inputs: InputInfo::default(),
        }
    }
```

Find:
```rust
        LOADS.fetch_add(1, Ordering::Relaxed);
        self.plugins_in_engine |= has_slots(doc);
        self.engine.set_media(media_of(owner));
        let report = self.engine.load(doc, seq)?;
        self.report = report_lines(&report);
        self.engine.set_loop(looped)?;
```
Replace with:
```rust
        LOADS.fetch_add(1, Ordering::Relaxed);
        self.plugins_in_engine |= has_slots(doc);
        self.engine.set_media(media_of(owner));
        self.engine.set_inputs(input_spec(doc, seq));
        let report = self.engine.load(doc, seq)?;
        self.report = report_lines(&report);
        self.engine.set_loop(looped)?;
```

Find:
```rust
        faulted: h.faulted.iter().copied().collect(),
    })
}
```
Replace with:
```rust
        faulted: h.faulted.iter().copied().collect(),
    })
}

/// Installs a record tap in `owner`'s engine (spec §6.2): the next block
/// that plays starts capturing. `Busy`-free: fails only when `owner` does
/// not hold the engine or its command queue is full.
pub fn start_recording(owner: u64, req: &RecordRequest) -> Result<RecordHandles, VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => Ok(h.engine.start_recording(req)?),
        _ => Err(VizError::Engine {
            message: "this document does not hold the audio engine".into(),
        }),
    }
}

/// Stops and removes `owner`'s record tap (nothing when another session
/// holds the engine: its tap went with the old engine).
pub fn stop_recording(owner: u64) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner => {
            h.want_play = false;
            h.until = None;
            h.epoch = next_epoch();
            h.engine.stop()?;
            Ok(h.engine.stop_recording()?)
        }
        _ => Ok(()),
    }
}

/// `owner`'s input, when it holds the engine.
pub fn input_info(owner: u64) -> Option<(String, InputInfo)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.device.clone(), h.inputs.clone()))
}

/// The MIDI input of `owner`'s engine, when it holds it.
pub fn midi_sender(owner: u64) -> Option<MidiSender> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.midi_sender())
}

/// Whether `owner`'s renderer has its transport stopped (the end of a
/// song without a loop), when it holds the engine. Right after a Play this
/// still reads stopped until the renderer applies it.
pub fn transport_stopped(owner: u64) -> Option<bool> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.state() == PlayState::Stopped)
}

/// The graph latency of `owner`'s engine, when it holds it.
pub fn latency_frames(owner: u64) -> Option<u64> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.latency_frames())
}
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    }
}

/// Waveform overviews (plan 5b Task 4).
#[uniffi::export]
impl Session {
```
Replace with:
```rust
    }
}

/// Live input (plan 5c Task 8; spec §6.1, §6.6, §12.1).
#[uniffi::export]
impl Session {
    /// What the audio input offers while this document holds the engine
    /// (nothing before its first Play or Record).
    pub fn input_info(&self) -> Result<crate::live::InputInfoRow, VizError> {
        ffi_guard(|| {
            Ok(match host::input_info(self.id) {
                Some((device, i)) => crate::live::InputInfoRow {
                    device,
                    channels: i.channels,
                    names: i.names,
                    input_latency_frames: i.input_latency_frames,
                    output_latency_frames: i.output_latency_frames,
                },
                None => crate::live::InputInfoRow {
                    device: String::new(),
                    channels: 0,
                    names: Vec::new(),
                    input_latency_frames: 0,
                    output_latency_frames: 0,
                },
            })
        })
    }

    /// Feeds `bytes` (channel messages, one after another) to the engine
    /// as if source `port` sent them now (`midi.inject`): lanes listening
    /// to that source play them thru and, armed and recording, record
    /// them. False when this document does not hold the engine (nothing
    /// is sent). `Model` for bytes that are not channel messages or a bad
    /// port name; `Busy` when the input queue is full.
    pub fn inject_midi(&self, port: String, bytes: Vec<u8>) -> Result<bool, VizError> {
        ffi_guard(|| {
            let msgs = crate::live::parse_injection(&port, &bytes)?;
            let Some(tx) = host::midi_sender(self.id) else {
                return Ok(false);
            };
            let key = core_engine::input::port_key(&port);
            for msg in msgs {
                let sent = tx.send(core_engine::input::MidiIn {
                    port: key,
                    host_ns: 0,
                    msg,
                });
                if !sent {
                    return Err(VizError::Busy {
                        message: "the MIDI input queue is full".into(),
                    });
                }
            }
            Ok(true)
        })
    }
}

/// Waveform overviews (plan 5b Task 4).
#[uniffi::export]
impl Session {
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod input;
pub mod jobs;
pub mod keys;
pub mod media;
pub mod mixer;
pub mod modify;
```
Replace with:
```rust
pub mod input;
pub mod jobs;
pub mod keys;
pub mod live;
pub mod media;
pub mod mixer;
pub mod modify;
```

Find:
```rust
    DrumViewRow, QuantizeChoiceRow,
};
pub use edit::{
    AudioEdit, EditCommand, EventEdit, GeneratorRow, LaneEdit, LaneRef, QuantizeRow, ReferenceEdit,
    SequenceEdit, ViewFilterRow,
};
pub use editor::EditorSize;
pub use error::VizError;
```
Replace with:
```rust
    DrumViewRow, QuantizeChoiceRow,
};
pub use edit::{
    AudioEdit, EditCommand, EventEdit, GeneratorRow, LaneEdit, LaneInputRow, LaneRef,
    MonitorModeRow, QuantizeRow, ReferenceEdit, SequenceEdit, ViewFilterRow,
};
pub use editor::EditorSize;
pub use error::VizError;
```

Find:
```rust
pub use input::{NoteInRow, NoteInput};
pub use jobs::{ImportTargetRow, JobKindRow, JobRow, JobStateRow};
pub use keys::{BindingRow, Keymap, chord_text, key_code_for};
pub use media::{MediaOriginRow, MediaRow, MediaUseRow, OfflineReasonRow};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
```
Replace with:
```rust
pub use input::{NoteInRow, NoteInput};
pub use jobs::{ImportTargetRow, JobKindRow, JobRow, JobStateRow};
pub use keys::{BindingRow, Keymap, chord_text, key_code_for};
pub use live::InputInfoRow;
pub use media::{MediaOriginRow, MediaRow, MediaUseRow, OfflineReasonRow};
pub use mixer::{
    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
        waveformCalls.append((event, fromTick, toTick, columns))
        return try overview(media: 0, fromFrame: 0, toFrame: 1, columns: columns)
    }
    // MARK: Media pool (plan 5b Task 6)

    var renamed: [(UInt64, String)] = []
```
Replace with:
```swift
        waveformCalls.append((event, fromTick, toTick, columns))
        return try overview(media: 0, fromFrame: 0, toFrame: 1, columns: columns)
    }
    // MARK: Live input (plan 5c Task 8)

    var injected: [(port: String, bytes: Data)] = []
    var inputRow = InputInfoRow(
        device: "", channels: 0, names: [], inputLatencyFrames: 0, outputLatencyFrames: 0)

    func injectMidi(port: String, bytes: Data) throws -> Bool {
        injected.append((port, bytes))
        return true
    }

    func inputInfo() throws -> InputInfoRow { inputRow }

    // MARK: Media pool (plan 5b Task 6)

    var renamed: [(UInt64, String)] = []
```

Find:
```swift
    LaneRow(
        id: id, name: name, destination: nil, midiEventCount: 0, regionCount: 0,
        referenceCount: 0, mute: mute, solo: false, looped: false, takeCount: 1,
        activeTake: "Take 1", isDrum: false, arm: false, midiDest: nil, audioOut: nil)
}

/// Song (top-level) using Groove, which uses Beat.
```
Replace with:
```swift
    LaneRow(
        id: id, name: name, destination: nil, midiEventCount: 0, regionCount: 0,
        referenceCount: 0, mute: mute, solo: false, looped: false, takeCount: 1,
        activeTake: "Take 1", isDrum: false, arm: false, midiDest: nil, audioOut: nil, input: nil,
        monitor: .auto)
}

/// Song (top-level) using Groove, which uses Beat.
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test live_input 2>&1 | tail -3
```
Expected: `test result: ok. 5 passed`, including `hostile_injections_are_refused` (Review Focus 5).

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/live.rs crates/core-ffi/src/edit.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/host.rs crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-ffi/tests/live_input.rs
git commit -F - <<'VIZDAW_MSG'
core-ffi: lane inputs, the fake device and MIDI injection

Lane inputs and monitor modes are lane edits over the FFI and in the
snapshot. The host gives the engine its input routes at every load
and reports the device's input; injected MIDI plays thru and records
like a keyboard. Tests pick the fake input with a hook.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 9: `core-ffi` — recording in the session

**Files:**
- Create: `crates/core-ffi/src/record.rs`
- Modify: `crates/core-ffi/src/session.rs` (record, keep and discard, `record_state`, the history group, busy guards, retention, close, `idle`)
- Modify: `crates/core-ffi/src/host.rs` (`start_recording`, `stop_recording`, `transport_stopped`, `latency_frames`, `position`; stop recording when another session plays)
- Modify: `crates/core-ffi/src/lib.rs`
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/record.rs`

**Interfaces:**
- Consumes: Tasks 3, 4, 5, 7 and 8.
- Produces:
  - UniFFI `RecordSpecRow { replace, punch: Option<TickRange>, looped, count_in_bars, pre_roll_bars, passes_as_takes }`, `RecordLaneRow { lane, audio, path, clipped }`, `RecordStateRow { recording, seq, spec, pass, kept, discarded, lanes, frames, overflows, gap_frames, free_bytes }`; `Session.record(seq, spec)`, `keep_pass()`, `discard_pass()`, `record_state()`; `set_record_latency_trim(frames)`;
  - `record::{Recording, Finished, Ending, build, next_pass, file_safe, stamp, recorded_path, set_free_bytes (test hook), RECORDED_DIR, LOW_DISK_WARN, LOW_DISK_STOP, DISK_CHECK_EVERY}`.

Rulings 5, 13–17. `Session::record` checks everything first (Ruling 17), makes the files, starts the writer and the tap, and opens a history group; `stop` (or the pass ending by itself, checked in `idle`) finishes the writer outside the session lock, plans the pass and applies it as the group's last step.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/record.rs <<'VIZDAW_EOF'
//! Recording through a session (plan 5c Task 9; sub-project 5 spec
//! §6.2–§6.5, §12.3): audio from the fake input lands as one undo step
//! "Record" in the project format; the loopback device lands a click
//! within two frames of where it played; loop passes become takes with
//! keep and discard; MIDI recorded in a punch replaces only that range;
//! edits made while recording join the step; a full disk, a lost device
//! and Stop end a pass cleanly and keep what was recorded; a save while
//! recording never takes the file being written. Offline only: no audio
//! device is opened and no microphone permission asked.

#![recursion_limit = "256"]

mod common;

use core_engine::fixtures::{Builder, Q};
use core_engine::offline::FakeInput;
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::{AnyId, LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use core_model::sequence::SeqLength;
use core_model::time::Tick;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input};
use vizdaw_ffi::record::set_free_bytes;
use vizdaw_ffi::{
    EventKindRow, EventRow, MediaOriginRow, RecordSpecRow, Session, TickRange, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial(input: Option<FakeInput>) -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
    set_free_bytes(None);
    reset_host();
    g
}

/// 120 BPM at 48 kHz: one beat (Q ticks) is 24 000 frames.
const BEAT: i64 = 24_000;

struct Doc {
    doc: Document,
    seq: SeqId,
    keys: LaneId,
    mic: LaneId,
    src: LaneId,
}

/// Sequence "S" (`bars` bars locked): lane "Keys" on the built-in synth
/// with notes at beats 1 and 5, MIDI input from "Keys", armed; lane "Mic"
/// recording fake input 1, armed, monitor Off; lane "Src" (an output
/// path, for regions); one output "Audio 1".
fn doc(bars: i64, arm_keys: bool, arm_mic: bool) -> Doc {
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("S", "Keys", Some(synth)).unwrap();
    b.notes(
        seq,
        keys,
        &[(Q, 40, 0x8000, Q / 2), (5 * Q, 41, 0x8000, Q / 2)],
    )
    .unwrap();
    let mic = b.lane(seq, "Mic", None).unwrap();
    let src = b.lane(seq, "Src", None).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }))
        .unwrap();
    let out = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap();
    for lane in [mic, src] {
        b.run(Command::Lane(LaneCmd::SetOutput {
            seq,
            lane,
            output: Some(out),
        }))
        .unwrap();
    }
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: mic,
        input: Some(LaneInput::Audio {
            channels: InputChannels::Mono(0),
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetMonitor {
        seq,
        lane: mic,
        monitor: MonitorMode::Off,
    }))
    .unwrap();
    for (lane, on) in [(keys, arm_keys), (mic, arm_mic)] {
        b.run(Command::Lane(LaneCmd::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(on),
        }))
        .unwrap();
    }
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(bars * 4 * Q)),
    }))
    .unwrap();
    Doc {
        doc: b.finish(),
        seq,
        keys,
        mic,
        src,
    }
}

fn spec() -> RecordSpecRow {
    RecordSpecRow {
        replace: true,
        punch: None,
        looped: false,
        count_in_bars: 0,
        pre_roll_bars: 0,
        passes_as_takes: true,
    }
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn frames(s: &Session) -> u64 {
    s.record_state().unwrap().frames
}

fn regions(s: &Session, seq: u64, lane: u64) -> Vec<EventRow> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.media.is_some())
        .collect()
}

fn read(path: &str) -> (core_media::caf::CafInfo, Vec<f32>) {
    let r = CafReader::open(Path::new(path)).unwrap();
    let info = r.info().clone();
    let mut out = vec![0.0; info.frames as usize * usize::from(info.channels)];
    r.read(0, &mut out, &mut Vec::new()).unwrap();
    (info, out)
}

#[test]
fn recording_the_fake_input_is_one_undo_step_in_the_project_format() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.locate(4 * Q).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    let st = s.record_state().unwrap();
    assert!(st.recording);
    assert_eq!(st.lanes.len(), 1);
    assert!(st.lanes[0].audio);
    assert!(
        st.lanes[0]
            .path
            .as_deref()
            .unwrap()
            .starts_with("Media/Recorded/Mic ")
    );
    wait_until("a second of audio", || frames(&s) >= 48_000);
    s.stop().unwrap();
    assert!(!s.record_state().unwrap().recording);
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Record"));
    let media = s.media().unwrap();
    assert_eq!(media.len(), 1);
    let m = &media[0];
    assert_eq!(m.origin, MediaOriginRow::Recorded);
    assert_eq!((m.sample_rate, m.channels, m.offline), (48_000, 1, None));
    let r = regions(&s, d.seq.0, d.mic.0);
    assert_eq!(r.len(), 1);
    // Where recording started, from the file's first frame (no latency).
    assert_eq!((r[0].tick, r[0].source_offset), (4 * Q, 0));
    assert_eq!(r[0].frames, m.frames);
    // The sine at −12 dBFS, 24-bit as the project is.
    let (info, data) = read(&s.media_file(m.id).unwrap());
    assert_eq!(info.depth, BitDepth::Int24);
    let peak = data.iter().fold(0.0f32, |a, x| a.max(x.abs()));
    assert!((peak - 0.2512).abs() < 0.01, "{peak}");
    s.undo().unwrap();
    assert!(s.media().unwrap().is_empty());
    assert!(regions(&s, d.seq.0, d.mic.0).is_empty());
    s.redo().unwrap();
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1);
    s.close().unwrap();
}

#[test]
fn files_follow_the_project_format() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let mut doc = d.doc.clone();
    doc = core_model::command::apply(
        &doc,
        &Command::Settings(core_model::command::SettingsCmd::SetFormat {
            format: core_model::format::AudioFormat {
                sample_rate: 44_100,
                depth: BitDepth::Int16,
            },
            swaps: Vec::new(),
        }),
    )
    .unwrap()
    .doc;
    let s = Session::from_document(doc).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4410);
    s.stop().unwrap();
    let m = &s.media().unwrap()[0];
    let (info, _) = read(&s.media_file(m.id).unwrap());
    assert_eq!((info.sample_rate, info.depth), (44_100, BitDepth::Int16));
    assert_eq!(m.sample_rate, 44_100);
    s.close().unwrap();
}

/// A mono CAF of `frames` frames, silent but for 8 frames of 0.9 at `at`.
fn click(path: &Path, frames: usize, at: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut v = vec![0.0f32; frames];
    for x in v.iter_mut().skip(at).take(8) {
        *x = 0.9;
    }
    let mut w = CafWriter::create(path, 48_000, 1, BitDepth::Int24, WriteMode::Normal).unwrap();
    w.write(&v).unwrap();
    w.finish().unwrap();
}

#[test]
fn a_looped_back_click_lands_within_two_frames_of_where_it_played() {
    let _g = serial(Some(FakeInput::Loopback(1000)));
    let d = doc(8, false, true);
    // A region on "Src" from beat 1 whose click is 12 000 frames in: it
    // plays at timeline frame 36 000.
    let mut b = Builder::from_doc(d.doc.clone());
    let media = b.media("Media/click.caf", 48_000, 1).unwrap();
    b.region(
        d.seq,
        d.src,
        Q,
        media,
        0,
        48_000,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    click(Path::new(&s.media_file(media.0).unwrap()), 48_000, 12_000);
    s.recheck_media().unwrap();
    assert_eq!(s.media().unwrap()[0].offline, None);
    s.record(d.seq.0, spec()).unwrap();
    wait_until("the click to come back", || frames(&s) >= 48_000 + 1000);
    s.stop().unwrap();
    let r = regions(&s, d.seq.0, d.mic.0);
    assert_eq!(r.len(), 1, "{r:?}");
    // The device reported 1000 frames of round trip: they come off.
    assert_eq!(r[0].source_offset, 1000);
    let (_, data) = read(&s.media_file(r[0].media.unwrap()).unwrap());
    let k = data.iter().position(|x| x.abs() > 0.2).unwrap() as i64;
    let at = r[0].tick * BEAT / Q + (k - r[0].source_offset as i64);
    assert!((at - 36_000).abs() <= 2, "the click landed at {at}");
    s.close().unwrap();
}

/// Takes of `lane` as `(name, region source offsets)`.
fn takes(s: &Session, seq: u64, lane: u64) -> (u32, String) {
    let row = s
        .lanes(seq)
        .unwrap()
        .into_iter()
        .find(|l| l.id == lane)
        .unwrap();
    (row.take_count, row.active_take)
}

#[test]
fn loop_passes_with_keep_and_discard_become_takes() {
    let _g = serial(Some(FakeInput::Signals));
    // One bar at 240 BPM: a pass a second.
    let d = doc(1, false, true);
    let mut doc = d.doc.clone();
    doc = core_model::command::apply(
        &doc,
        &Command::Sequence(SequenceCmd::SetTempo {
            seq: d.seq,
            tempo: core_model::tempo::TempoMap::new(vec![core_model::tempo::TempoPoint {
                tick: Tick(0),
                bpm: core_model::tempo::Bpm::whole(240),
                ramp: false,
            }])
            .unwrap(),
        }),
    )
    .unwrap()
    .doc;
    let s = Session::from_document(doc).unwrap();
    s.record(
        d.seq.0,
        RecordSpecRow {
            looped: true,
            ..spec()
        },
    )
    .unwrap();
    assert!(s.keep_pass().is_ok(), "nothing completed yet: no effect");
    wait_until("pass 2", || s.record_state().unwrap().pass >= 2);
    s.keep_pass().unwrap();
    wait_until("pass 3", || s.record_state().unwrap().pass >= 3);
    s.discard_pass().unwrap();
    let st = s.record_state().unwrap();
    assert_eq!((st.kept, st.discarded), (1, 1));
    s.stop().unwrap();
    // Pass 1 (kept) and pass 3 (playing at Stop) became takes; pass 2 went.
    assert_eq!(takes(&s, d.seq.0, d.mic.0), (3, "Take 3".into()));
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.undo().unwrap();
    assert_eq!(takes(&s, d.seq.0, d.mic.0), (1, "Take 1".into()));
    s.close().unwrap();
}

fn notes(s: &Session, seq: u64, lane: u64) -> Vec<(i64, u8)> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .filter(|e| e.kind == EventKindRow::Note)
        .map(|e| (e.tick, e.pitch))
        .collect()
}

#[test]
fn midi_recorded_in_a_punch_replaces_only_that_range() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, true, false);
    let s = Session::from_document(d.doc.clone()).unwrap();
    // Punch beats 4–8: the old note at beat 5 goes, the one at beat 1 stays.
    s.record(
        d.seq.0,
        RecordSpecRow {
            punch: Some(TickRange {
                start: 4 * Q,
                end: 8 * Q,
            }),
            ..spec()
        },
    )
    .unwrap();
    // Before the punch: not recorded.
    wait_until("beat 1", || s.transport().unwrap().tick >= Q);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 60, 100]).unwrap());
    assert!(s.inject_midi("Keys".into(), vec![0x80, 60, 0]).unwrap());
    wait_until("the punch", || s.transport().unwrap().tick >= 5 * Q);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 64, 100]).unwrap());
    wait_until("a beat on", || s.transport().unwrap().tick >= 6 * Q);
    assert!(s.inject_midi("Keys".into(), vec![0x80, 64, 0]).unwrap());
    // Another source: not this lane's.
    assert!(s.inject_midi("Pads".into(), vec![0x90, 65, 100]).unwrap());
    wait_until("the punch out", || {
        s.transport().unwrap().tick >= 8 * Q + Q / 2
    });
    s.stop().unwrap();
    let got = notes(&s, d.seq.0, d.keys.0);
    let pitches: Vec<u8> = got.iter().map(|(_, p)| *p).collect();
    assert_eq!(pitches, vec![40, 64], "{got:?}");
    // Played after beat 5, within a block or two.
    let t = got[1].0;
    assert!((5 * Q..6 * Q).contains(&t), "{t}");
    s.close().unwrap();
}

#[test]
fn edits_made_while_recording_join_the_record_step() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    let before = s.lanes(d.seq.0).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    s.rename_sequence(d.seq.0, "Renamed".into()).unwrap();
    // Undoable on its own while recording…
    s.undo().unwrap();
    s.redo().unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.stop().unwrap();
    // …and then one step with the recording.
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().sequences[0].name, "S");
    assert_eq!(s.lanes(d.seq.0).unwrap(), before);
    assert!(s.snapshot().unwrap().undo_label.is_none());
    s.close().unwrap();
}

#[test]
fn bad_requests_are_refused_and_recording_holds_the_transport() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, false);
    let s = Session::from_document(d.doc.clone()).unwrap();
    let model = |r: Result<(), VizError>| matches!(r, Err(VizError::Model { .. }));
    assert!(model(s.record(d.seq.0, spec())), "nothing armed");
    assert!(model(s.record(999, spec())), "no such sequence");
    s.close().unwrap();
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    for bad in [
        RecordSpecRow {
            count_in_bars: 1,
            ..spec()
        },
        RecordSpecRow {
            count_in_bars: 5,
            ..spec()
        },
        RecordSpecRow {
            pre_roll_bars: 9,
            ..spec()
        },
        RecordSpecRow {
            punch: Some(TickRange { start: Q, end: Q }),
            ..spec()
        },
        RecordSpecRow {
            punch: Some(TickRange {
                start: 0,
                end: i64::MAX,
            }),
            ..spec()
        },
    ] {
        assert!(model(s.record(d.seq.0, bad.clone())), "{bad:?}");
    }
    assert!(model(s.keep_pass()), "nothing is recording");
    s.record(d.seq.0, spec()).unwrap();
    for r in [
        s.record(d.seq.0, spec()),
        s.locate(0),
        s.play(d.seq.0),
        s.set_loop(true),
        s.save_as("/tmp/never.vizdaw".into()),
    ] {
        assert!(matches!(r, Err(VizError::Busy { .. })), "{r:?}");
    }
    assert!(model(s.keep_pass()), "not loop recording");
    s.stop().unwrap();
    s.close().unwrap();
    // An input channel the device does not have.
    let mut doc = d.doc.clone();
    doc = core_model::command::apply(
        &doc,
        &Command::Lane(LaneCmd::SetInput {
            seq: d.seq,
            lane: d.mic,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Stereo(2),
            }),
        }),
    )
    .unwrap()
    .doc;
    let s = Session::from_document(doc).unwrap();
    assert!(model(s.record(d.seq.0, spec())));
    assert!(!s.record_state().unwrap().recording);
    assert!(s.media().unwrap().is_empty());
    s.close().unwrap();
}

#[test]
fn a_full_disk_ends_the_pass_and_keeps_what_was_recorded() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    set_free_bytes(Some(50 << 20));
    assert!(matches!(
        s.record(d.seq.0, spec()),
        Err(VizError::Busy { .. })
    ));
    set_free_bytes(Some(500 << 20));
    s.record(d.seq.0, spec()).unwrap();
    assert!(
        s.idle()
            .unwrap()
            .problems
            .iter()
            .any(|p| p.contains("less than 1 GB")),
        "a warning first"
    );
    wait_until("audio", || frames(&s) >= 4800);
    set_free_bytes(Some(50 << 20));
    let mut problems = Vec::new();
    wait_until("the pass to end", || {
        problems.extend(s.idle().unwrap().problems);
        !s.record_state().unwrap().recording
    });
    assert!(
        problems
            .iter()
            .any(|p| p == "Recording stopped: the disk is almost full."),
        "{problems:?}"
    );
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1, "kept");
    set_free_bytes(None);
    s.close().unwrap();
}

#[test]
fn a_device_that_goes_away_ends_the_pass_cleanly() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    // The output (and its renderer, with the tap) goes.
    reset_host();
    let mut problems = Vec::new();
    wait_until("the pass to end", || {
        problems.extend(s.idle().unwrap().problems);
        !s.record_state().unwrap().recording
    });
    assert!(
        problems
            .iter()
            .any(|p| p.contains("audio device went away")),
        "{problems:?}"
    );
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.close().unwrap();
}

#[test]
fn saving_while_recording_never_trashes_the_file_being_written() {
    let _g = serial(Some(FakeInput::Signals));
    let dir = tempfile::tempdir().unwrap();
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file(), "the file being written stays");
    s.stop().unwrap();
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file());
    // Undone and saved: the history still names it.
    s.undo().unwrap();
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file());
    s.close().unwrap();
    // Closed: still there, or in the Trash; never deleted.
    let name = Path::new(&rel).file_name().unwrap().to_owned();
    let trashed = std::fs::read_dir(dir.path().join("Trash"))
        .map(|rd| rd.flatten().any(|e| e.file_name() == name))
        .unwrap_or(false);
    assert!(pkg.join(&rel).is_file() || trashed);
}

#[test]
fn closing_an_untitled_recording_moves_it_to_the_trash() {
    let _g = serial(Some(FakeInput::Signals));
    let dir = tempfile::tempdir().unwrap();
    let d = doc(8, false, true);
    let s: Arc<Session> = Session::from_document(d.doc.clone()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    // Closing while recording finishes the pass first.
    s.close().unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path().join("Trash"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|n| n.starts_with("Mic ")), "{names:?}");
}

/// Stop during a drag: the pass waits for the drag to end, then lands as
/// the one "Record" step.
#[test]
fn a_pass_finished_during_a_drag_lands_when_it_ends() {
    let _g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    s.begin_edit("Drag".into()).unwrap();
    s.stop().unwrap();
    assert!(!s.record_state().unwrap().recording);
    assert!(
        regions(&s, d.seq.0, d.mic.0).is_empty(),
        "not inside the drag"
    );
    s.end_edit().unwrap();
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    s.close().unwrap();
}

/// A device that runs at another rate than the project refuses recording
/// (no resampling: it would play back at the wrong speed).
#[test]
fn a_device_at_another_rate_refuses_recording() {
    let _g = serial(Some(FakeInput::Signals));
    vizdaw_ffi::host::set_offline_rate(44_100.0);
    reset_host();
    let d = doc(8, false, true);
    let s = Session::from_document(d.doc.clone()).unwrap();
    let r = s.record(d.seq.0, spec());
    vizdaw_ffi::host::set_offline_rate(0.0);
    match r {
        Err(VizError::Model { message }) => assert!(message.contains("44100"), "{message}"),
        other => panic!("{other:?}"),
    }
    assert!(!s.record_state().unwrap().recording);
    assert!(s.snapshot().unwrap().undo_label.is_none());
    s.close().unwrap();
    reset_host();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test record 2>&1 | tail -5
```
Expected: does not compile: `RecordSpecRow` and `Session::record` do not exist.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/record.rs <<'VIZDAW_EOF'
//! A recording in progress (sub-project 5 spec §6.2–§6.5; plan 5c Task
//! 9): what `Session.record` started, what it collects while the
//! transport rolls, the disk and input checks `idle` runs, and finishing
//! it into a [`RecordPass`]. The session holds one in its state; nothing
//! here takes the session lock.

use crate::error::VizError;
use crate::passes::{AudioTake, Captured, PassFate, RecordRange, decide, plan};
use core_engine::record::{
    Mark, MarkReader, MidiReader, RecordHandles, RecordStatus, RecordedMidi,
};
use core_media::record::{RecordWriter, WriterConfig, WriterResult};
use core_model::command::{RecordPass, RecordedMedia};
use core_model::document::{Document, MediaItem, MediaOrigin};
use core_model::format::AudioFormat;
use core_model::ids::{LaneId, SeqId};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Where recordings go in a package (spec §6.2).
pub const RECORDED_DIR: &str = "Media/Recorded";
/// Below this, recording warns (spec §6.2: 1 GiB).
pub const LOW_DISK_WARN: u64 = 1 << 30;
/// Below this, recording stops (spec §6.2: 100 MiB).
pub const LOW_DISK_STOP: u64 = 100 << 20;
/// How often free space is checked while recording (spec §6.2: 10 s).
pub const DISK_CHECK_EVERY: Duration = Duration::from_secs(10);
/// How long Stop waits for the last input to be written.
pub const FINISH_WAIT: Duration = Duration::from_secs(2);
/// The largest latency trim, in frames either way (1 s at 192 kHz).
pub const MAX_TRIM: i32 = 192_000;

static TRIM: AtomicI64 = AtomicI64::new(0);
/// A fake free-space reading (test hook; `u64::MAX`: none).
static FAKE_FREE: AtomicU64 = AtomicU64::new(u64::MAX);

/// Sets the recording latency trim (spec §6.3, Settings › Recording ›
/// Latency trim): frames added to where recorded audio lands, for devices
/// that misreport their latency; clamped to ±[`MAX_TRIM`]. Takes effect
/// from the next pass.
#[uniffi::export]
pub fn set_record_latency_trim(frames: i32) {
    TRIM.store(
        i64::from(frames.clamp(-MAX_TRIM, MAX_TRIM)),
        Ordering::Relaxed,
    );
}

/// The latency trim in frames.
pub fn latency_trim() -> i64 {
    TRIM.load(Ordering::Relaxed)
}

/// Makes free-space checks read `bytes` (test hook; `None`: the disk's).
#[cfg(feature = "test-hooks")]
pub fn set_free_bytes(bytes: Option<u64>) {
    FAKE_FREE.store(bytes.unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// Free bytes on the volume of `root` (or of its nearest existing
/// parent).
pub fn free_bytes(root: &Path) -> Option<u64> {
    let fake = FAKE_FREE.load(Ordering::Relaxed);
    if fake != u64::MAX {
        return Some(fake);
    }
    let mut p = Some(root);
    while let Some(d) = p {
        if d.exists() {
            return core_media::record::free_bytes(d);
        }
        p = d.parent();
    }
    None
}

/// What `Session.record` is asked for (spec §6.4 `RecordSpec`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecordSpecRow {
    /// Replace (else overdub).
    pub replace: bool,
    /// The punch range in ticks, when punch is on.
    pub punch: Option<crate::selection::TickRange>,
    /// Loop recording over the transport loop (bar 1 to the end).
    pub looped: bool,
    /// Bars of count-in, 0–4 (refused above 0 until plan 5d plays them).
    pub count_in_bars: u8,
    /// Bars of pre-roll, 0–8.
    pub pre_roll_bars: u8,
    /// Kept loop passes become takes.
    pub passes_as_takes: bool,
}

/// One armed lane while recording.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecordLaneRow {
    /// The lane.
    pub lane: u64,
    /// It records audio (else MIDI).
    pub audio: bool,
    /// Its file (package-relative), for audio.
    pub path: Option<String>,
    /// Samples clipped so far (integer depths).
    pub clipped: u64,
}

/// What a recording is doing (`state.get record`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecordStateRow {
    /// A pass is recording.
    pub recording: bool,
    /// Its sequence.
    pub seq: Option<u64>,
    /// What it was asked for.
    pub spec: Option<RecordSpecRow>,
    /// The pass playing, from 1 (0 before the transport rolls).
    pub pass: u32,
    /// Completed passes kept so far.
    pub kept: u32,
    /// Completed passes discarded so far.
    pub discarded: u32,
    /// The armed lanes.
    pub lanes: Vec<RecordLaneRow>,
    /// Frames recorded so far.
    pub frames: u64,
    /// Input blocks lost because the ring was full (`record_overflows`).
    pub overflows: u64,
    /// Frames written as silence for lost input.
    pub gap_frames: u64,
    /// Free space on the project's volume.
    pub free_bytes: Option<u64>,
}

/// One armed audio lane's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioLane {
    /// The lane.
    pub lane: LaneId,
    /// Package-relative path.
    pub path: String,
    /// Channels in the file (1 or 2).
    pub channels: u16,
}

/// Why a recording must end now (spec §6.2, §12.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The transport stopped by itself (the end without a loop).
    Ended,
    /// Something went wrong; the message says what.
    Because(String),
}

/// A recording in progress.
pub struct Recording {
    /// The sequence.
    pub seq: SeqId,
    /// What was asked.
    pub spec: RecordSpecRow,
    /// The record range in schedule frames.
    pub range: RecordRange,
    /// The armed audio lanes.
    pub audio: Vec<AudioLane>,
    /// The armed MIDI lanes.
    pub midi_lanes: Vec<LaneId>,
    /// The writer (none for MIDI only).
    pub writer: Option<RecordWriter>,
    /// The tap's marks.
    marks_rx: MarkReader,
    /// The tap's recorded MIDI.
    midi_rx: MidiReader,
    /// The tap's counters.
    status: Arc<RecordStatus>,
    /// The package root files are written under.
    pub root: PathBuf,
    /// The project format.
    pub format: AudioFormat,
    /// The pass number media get.
    pub pass_number: u32,
    marks: Vec<Mark>,
    midi: Vec<RecordedMidi>,
    fates: Vec<PassFate>,
    disk_checked: Instant,
    warned_low: bool,
}

impl std::fmt::Debug for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording")
            .field("seq", &self.seq)
            .field("spec", &self.spec)
            .finish_non_exhaustive()
    }
}

/// What a finished recording captured.
#[derive(Debug)]
pub struct Finished {
    /// The sequence.
    pub seq: SeqId,
    /// What was asked.
    pub spec: RecordSpecRow,
    /// The record range.
    pub range: RecordRange,
    /// The armed audio lanes.
    pub audio: Vec<AudioLane>,
    /// The armed MIDI lanes.
    pub midi_lanes: Vec<LaneId>,
    /// What the writer wrote.
    pub written: Option<WriterResult>,
    /// The marks.
    pub marks: Vec<Mark>,
    /// The recorded MIDI.
    pub midi: Vec<RecordedMidi>,
    /// The passes' fates.
    pub fates: Vec<PassFate>,
    /// The tap's counters.
    pub status: Arc<RecordStatus>,
    /// The project format.
    pub format: AudioFormat,
    /// The pass number.
    pub pass_number: u32,
}

impl Finished {
    /// Every file it wrote (package-relative).
    pub fn files(&self) -> Vec<String> {
        self.audio.iter().map(|a| a.path.clone()).collect()
    }
}

/// What a recording records (everything [`Recording::start`] needs
/// besides the tap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The sequence.
    pub seq: SeqId,
    /// What was asked.
    pub spec: RecordSpecRow,
    /// The record range in schedule frames.
    pub range: RecordRange,
    /// The armed audio lanes and their files.
    pub audio: Vec<AudioLane>,
    /// The armed MIDI lanes.
    pub midi_lanes: Vec<LaneId>,
    /// The package root.
    pub root: PathBuf,
    /// The project format.
    pub format: AudioFormat,
    /// The pass number media get.
    pub pass_number: u32,
}

impl Recording {
    /// Starts recording what `handles` taps: with `writer` lanes, a writer
    /// thread takes the tap's ring and makes their files (when they cannot
    /// be made nothing starts).
    pub fn start(
        plan: Plan,
        handles: RecordHandles,
        writer: Option<WriterConfig>,
    ) -> Result<Recording, core_media::MediaError> {
        let RecordHandles {
            ring,
            marks,
            midi,
            status,
        } = handles;
        let writer = match writer {
            Some(cfg) => Some(RecordWriter::start(ring, cfg)?),
            // MIDI only: the ring carries nothing.
            None => None,
        };
        Ok(Recording {
            seq: plan.seq,
            spec: plan.spec,
            range: plan.range,
            audio: plan.audio,
            midi_lanes: plan.midi_lanes,
            writer,
            marks_rx: marks,
            midi_rx: midi,
            status,
            root: plan.root,
            format: plan.format,
            pass_number: plan.pass_number,
            marks: Vec::new(),
            midi: Vec::new(),
            fates: Vec::new(),
            disk_checked: Instant::now(),
            warned_low: false,
        })
    }

    /// Takes what the tap published since the last call.
    pub fn drain(&mut self) {
        while let Ok(m) = self.marks_rx.pop() {
            self.marks.push(m);
        }
        while let Ok(m) = self.midi_rx.pop() {
            self.midi.push(m);
        }
    }

    /// Passes started so far (the roll and every wrap).
    pub fn passes(&self) -> usize {
        if self.marks.is_empty() {
            return 0;
        }
        1 + self.marks.iter().skip(1).filter(|m| m.wrap).count()
    }

    /// `record.keep_pass` (`keep`) or `record.discard_pass`: acts on the
    /// passes completed so far (plan 5c Ruling 13). `Model` unless loop
    /// recording.
    pub fn press(&mut self, keep: bool) -> Result<(), VizError> {
        if !self.spec.looped {
            return Err(crate::events::bad(
                "keep and discard act on loop passes; this recording does not loop",
            ));
        }
        self.drain();
        let completed = self.passes().saturating_sub(1);
        decide(&mut self.fates, completed, keep);
        Ok(())
    }

    /// The files it writes (package-relative).
    pub fn files(&self) -> Vec<String> {
        self.audio.iter().map(|a| a.path.clone()).collect()
    }

    /// Whether it must end now: the tap closed under it (the device went
    /// away), the writer failed (a full disk), free space fell below
    /// [`LOW_DISK_STOP`], or the transport stopped by itself. Pushes a
    /// warning once below [`LOW_DISK_WARN`].
    pub fn check(&mut self, transport_stopped: bool, problems: &mut Vec<String>) -> Option<Ending> {
        self.drain();
        if self.status.closed() {
            return Some(Ending::Because(
                "the audio device went away; what was recorded is kept".into(),
            ));
        }
        if let Some(e) = self.writer.as_ref().and_then(|w| w.status().error) {
            return Some(Ending::Because(format!(
                "the recording could not be written ({e})"
            )));
        }
        // A faked reading (tests) is looked at every time.
        let faked = FAKE_FREE.load(Ordering::Relaxed) != u64::MAX;
        if faked || self.disk_checked.elapsed() >= DISK_CHECK_EVERY {
            self.disk_checked = Instant::now();
            if let Some(free) = free_bytes(&self.root) {
                if free < LOW_DISK_STOP {
                    return Some(Ending::Because("the disk is almost full".into()));
                }
                if free < LOW_DISK_WARN && !self.warned_low {
                    self.warned_low = true;
                    problems.push("Recording: less than 1 GB of disk space is left.".into());
                }
            }
        }
        if transport_stopped && self.status.run() > 0 {
            return Some(Ending::Ended);
        }
        None
    }

    /// `state.get record` while it runs.
    pub fn row(&self) -> RecordStateRow {
        let w = self.writer.as_ref().map(RecordWriter::status);
        let completed = self.passes().saturating_sub(1);
        let count = |f: PassFate| {
            self.fates
                .iter()
                .take(completed)
                .filter(|x| **x == f)
                .count() as u32
        };
        let mut lanes: Vec<RecordLaneRow> = self
            .audio
            .iter()
            .enumerate()
            .map(|(k, a)| RecordLaneRow {
                lane: a.lane.0,
                audio: true,
                path: Some(a.path.clone()),
                clipped: w
                    .as_ref()
                    .and_then(|w| w.clipped.get(k).copied())
                    .unwrap_or(0),
            })
            .collect();
        lanes.extend(self.midi_lanes.iter().map(|l| RecordLaneRow {
            lane: l.0,
            audio: false,
            path: None,
            clipped: 0,
        }));
        RecordStateRow {
            recording: true,
            seq: Some(self.seq.0),
            spec: Some(self.spec.clone()),
            pass: self.passes() as u32,
            kept: count(PassFate::Kept),
            discarded: count(PassFate::Discarded),
            lanes,
            frames: self.status.run(),
            overflows: self.status.lost_blocks(),
            gap_frames: w.map_or(0, |w| w.gap_frames),
            free_bytes: free_bytes(&self.root),
        }
    }

    /// Ends it (the engine was told to stop and drop the tap): waits up
    /// to `wait` for the tap to close, takes the last marks and MIDI and
    /// lets the writer finish its files. Blocks: call without the session
    /// lock.
    pub fn finish(mut self, wait: Duration) -> Finished {
        let until = Instant::now() + wait;
        while !self.status.closed() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(2));
        }
        self.drain();
        let left = until.saturating_duration_since(Instant::now());
        let written = self
            .writer
            .take()
            .map(|w| w.finish(left.max(Duration::from_millis(100))));
        Finished {
            seq: self.seq,
            spec: self.spec,
            range: self.range,
            audio: self.audio,
            midi_lanes: self.midi_lanes,
            written,
            marks: self.marks,
            midi: self.midi,
            fates: self.fates,
            status: Arc::clone(&self.status),
            format: self.format,
            pass_number: self.pass_number,
        }
    }
}

/// The pass a finished recording makes in `doc`, or `None` when nothing
/// landed in the record range. Lanes deleted meanwhile are left out.
pub fn build(f: &Finished, doc: &Document) -> Result<Option<RecordPass>, String> {
    let lanes_alive = |l: &LaneId| doc.lane(f.seq, *l).is_ok();
    let mut audio = Vec::new();
    if let Some(w) = &f.written {
        for a in f.audio.iter().filter(|a| lanes_alive(&a.lane)) {
            let Some(lw) = w.lanes.iter().find(|l| l.path.ends_with(&a.path)) else {
                continue;
            };
            if lw.written.frames == 0 {
                continue;
            }
            let (_, stem) = MediaItem::file_parts(&a.path);
            audio.push(AudioTake {
                lane: a.lane,
                media: RecordedMedia {
                    path: a.path.clone(),
                    sample_rate: f.format.sample_rate,
                    channels: a.channels,
                    length_frames: lw.written.frames,
                    content_hash: lw.written.content_hash.clone(),
                    name: stem,
                    pass: f.pass_number,
                },
            });
        }
    }
    let midi_lanes: Vec<LaneId> = f.midi_lanes.iter().copied().filter(lanes_alive).collect();
    let gaps = f.written.as_ref().is_some_and(|w| w.gap_frames > 0)
        || f.status.lost_blocks() > 0
        || f.status.lost_midi() > 0
        || f.status.lost_marks() > 0;
    let delay = f.status.delay().unwrap_or(0) - latency_trim();
    plan(&Captured {
        doc,
        seq: f.seq,
        marks: &f.marks,
        end_run: f.status.run(),
        latency: f.status.latency,
        input_delay: delay,
        range: f.range,
        looped: f.spec.looped,
        replace: f.spec.replace,
        passes_as_takes: f.spec.passes_as_takes,
        fates: &f.fates,
        audio: &audio,
        midi_lanes: &midi_lanes,
        midi: &f.midi,
        with_gaps: gaps,
    })
}

/// The pass number a new recording's media get: one more than any
/// recorded item of `doc` has.
pub fn next_pass(doc: &Document) -> u32 {
    doc.media()
        .values()
        .filter_map(|m| match m.origin {
            MediaOrigin::Recorded { pass } => Some(pass),
            _ => None,
        })
        .max()
        .map_or(1, |p| p.saturating_add(1))
}

/// `name` as a file-name part: no path or control characters, no leading
/// dot, at most 100 characters; "Lane" when nothing is left.
pub fn file_safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    let s = s.trim().trim_start_matches('.').trim().to_string();
    if s.is_empty() { "Lane".into() } else { s }
}

/// `yyyy-mm-dd HHmmss` of `t` in UTC (plan 5c Ruling 14).
pub fn stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}{:02}{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A free package-relative path for lane `name`'s recording under
/// `Media/Recorded/` in `root`: `<name> <stamp>.caf`, then `… 2.caf`,
/// `… 3.caf`, never a file that exists or a path in `reserved` (compared
/// without case, as macOS volumes resolve names).
pub fn recorded_path(
    root: &Path,
    name: &str,
    at: SystemTime,
    reserved: &BTreeSet<String>,
) -> String {
    let base = format!("{} {}", file_safe(name), stamp(at));
    let taken =
        |rel: &str| root.join(rel).exists() || reserved.iter().any(|r| r.eq_ignore_ascii_case(rel));
    let mut rel = format!("{RECORDED_DIR}/{base}.caf");
    let mut k = 2;
    while taken(&rel) {
        rel = format!("{RECORDED_DIR}/{base} {k}.caf");
        k += 1;
    }
    rel
}
VIZDAW_EOF
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// The folder standing for the Trash (test hook; `None`: the
    /// environment's or the platform's).
    trash_dir: Option<PathBuf>,
}

/// The plugins the crash marker names: those loading now, and with
```
Replace with:
```rust
    /// The folder standing for the Trash (test hook; `None`: the
    /// environment's or the platform's).
    trash_dir: Option<PathBuf>,
    /// The recording in progress (plan 5c Task 9).
    recording: Option<crate::record::Recording>,
    /// A recording is being finished (its files are closing without the
    /// lock): no new one may start.
    finishing: bool,
    /// A finished recording's pass (or `None` when nothing landed), with
    /// its files, waiting for an open edit to end.
    pending_pass: Option<(Option<core_model::command::RecordPass>, Vec<String>)>,
    /// Recorded files no document references that the Trash must not take
    /// (a pass that could not be applied): crash recovery offers them at
    /// the next open.
    keep_recordings: std::collections::BTreeSet<String>,
}

/// The plugins the crash marker names: those loading now, and with
```

Find:
```rust
            overviews: Default::default(),
            changed: Default::default(),
            trash_dir: None,
        }
    }

    /// The retention rule on `package` (plan 5b Ruling 11). Skipped while
```
Replace with:
```rust
            overviews: Default::default(),
            changed: Default::default(),
            trash_dir: None,
            recording: None,
            finishing: false,
            pending_pass: None,
            keep_recordings: Default::default(),
        }
    }

    /// Recorded files the Trash must leave: those being written, those of
    /// a pass waiting to apply, and those kept after a failed pass (plan
    /// 5c Ruling 15).
    fn recorded_keep(&self) -> Vec<String> {
        let mut out: Vec<String> = self.keep_recordings.iter().cloned().collect();
        if let Some(r) = &self.recording {
            out.extend(r.files());
        }
        if let Some((_, files)) = &self.pending_pass {
            out.extend(files.iter().cloned());
        }
        out
    }

    /// The retention rule on `package` (plan 5b Ruling 11). Skipped while
```

Find:
```rust
            return;
        }
        let dir = crate::retention::trash_dir(self.trash_dir.as_deref());
        let out = crate::retention::run(&self.history.referenced_media_anywhere(), package, &|p| {
            crate::retention::trash(dir.as_deref(), p)
        });
        self.news.problems.extend(out.problems);
```
Replace with:
```rust
            return;
        }
        let dir = crate::retention::trash_dir(self.trash_dir.as_deref());
        let mut refs = self.history.referenced_media_anywhere();
        for p in self.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
        let out = crate::retention::run(&refs, package, &|p| {
            crate::retention::trash(dir.as_deref(), p)
        });
        self.news.problems.extend(out.problems);
```

Find:
```rust
        until: Option<Tick>,
    ) -> Result<(), VizError> {
        let mut inner = lock(&self.inner);
        let id = seq_id(inner.doc(), seq)?;
        let start = start.or(match (inner.cursor, inner.cursor_seq) {
            (Some(t), None) => Some(t),
```
Replace with:
```rust
        until: Option<Tick>,
    ) -> Result<(), VizError> {
        let mut inner = lock(&self.inner);
        if inner.recording.is_some() || inner.finishing {
            return Err(recording_busy());
        }
        let id = seq_id(inner.doc(), seq)?;
        let start = start.or(match (inner.cursor, inner.cursor_seq) {
            (Some(t), None) => Some(t),
```

Find:
```rust
    pub fn save_as(&self, path: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            // A job that just finished applies first, under the old root.
            self.apply_jobs(&mut inner);
            if inner.jobs.busy() {
```
Replace with:
```rust
    pub fn save_as(&self, path: String) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            // Recordings are written under the current root.
            if inner.recording.is_some() || inner.finishing {
                return Err(recording_busy());
            }
            // A job that just finished applies first, under the old root.
            self.apply_jobs(&mut inner);
            if inner.jobs.busy() {
```

Find:
```rust
    /// Stops; the position is kept.
    pub fn stop(&self) -> Result<(), VizError> {
        self.transport_call(|| {
            // Behind a rebuild this session's poll is running (its drain
            // has taken the host out of the slot): the Stop must not be
            // lost to an empty slot.
```
Replace with:
```rust
    /// Stops; the position is kept.
    pub fn stop(&self) -> Result<(), VizError> {
        self.transport_call(|| {
            // Stop while recording finishes the pass (spec §6.5).
            if lock(&self.inner).recording.is_some() {
                return self.finish_recording(None);
            }
            // Behind a rebuild this session's poll is running (its drain
            // has taken the host out of the slot): the Stop must not be
            // lost to an empty slot.
```

Find:
```rust
    pub fn locate(&self, tick: i64) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            if !host::locate(self.id, Tick(tick))? {
                inner.cursor = Some(Tick(tick));
                inner.cursor_seq = inner.selected_seq();
```
Replace with:
```rust
    pub fn locate(&self, tick: i64) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            if inner.recording.is_some() {
                return Err(recording_busy());
            }
            if !host::locate(self.id, Tick(tick))? {
                inner.cursor = Some(Tick(tick));
                inner.cursor_seq = inner.selected_seq();
```

Find:
```rust
    pub fn set_loop(&self, on: bool) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            inner.looped = on;
            host::set_loop(self.id, on)
        })
```
Replace with:
```rust
    pub fn set_loop(&self, on: bool) -> Result<(), VizError> {
        self.transport_call(|| {
            let mut inner = lock(&self.inner);
            if inner.recording.is_some() {
                return Err(recording_busy());
            }
            inner.looped = on;
            host::set_loop(self.id, on)
        })
```

Find:
```rust
            }
            plugins::tick_closing();
            self.unload_disabled();
            let mut inner = lock(&self.inner);
            if inner.reload_owed {
                inner.reload_host(self.id);
            }
```
Replace with:
```rust
            }
            plugins::tick_closing();
            self.unload_disabled();
            self.service_recording()?;
            let mut inner = lock(&self.inner);
            self.apply_pending_pass(&mut inner);
            if inner.reload_owed {
                inner.reload_host(self.id);
            }
```

Find:
```rust
    /// marker goes. Safe to call more than once.
    pub fn close(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.closed = true;
            let root = inner.root(self.id);
```
Replace with:
```rust
    /// marker goes. Safe to call more than once.
    pub fn close(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            // A recording ends cleanly first: its files are finished, its
            // pass applied (what is not saved then goes to the Trash).
            if lock(&self.inner).recording.is_some() {
                self.finish_recording(None)?;
            }
            let mut inner = lock(&self.inner);
            inner.closed = true;
            let root = inner.root(self.id);
```

Find:
```rust
            }
            host::forget_media(self.id);
            if inner.path.is_none() {
                let _ = std::fs::remove_dir_all(crate::media::scratch_root(self.id));
            }
            if let Some(k) = inner.key.clone() {
                recovery::closed(&k, self.id);
```
Replace with:
```rust
            }
            host::forget_media(self.id);
            if inner.path.is_none() {
                let scratch = crate::media::scratch_root(self.id);
                // Recordings are never unlinked: an untitled project's go
                // to the Trash before its scratch package is removed
                // (plan 5c Ruling 15).
                let dir = crate::retention::trash_dir(inner.trash_dir.as_deref());
                if let Ok(rd) = std::fs::read_dir(scratch.join(crate::record::RECORDED_DIR)) {
                    for e in rd.flatten() {
                        if e.path().is_file() {
                            if let Err(why) = crate::retention::trash(dir.as_deref(), &e.path()) {
                                eprintln!(
                                    "VizDAW: {}: not moved to the Trash ({why})",
                                    e.path().display()
                                );
                                continue;
                            }
                        }
                    }
                }
                let _ = std::fs::remove_dir_all(scratch);
            }
            if let Some(k) = inner.key.clone() {
                recovery::closed(&k, self.id);
```

Find:
```rust
            inner.news.media_changed = true;
            inner.news.doc_changed = true;
        }
    }
}
```
Replace with:
```rust
            inner.news.media_changed = true;
            inner.news.doc_changed = true;
        }
        self.apply_pending_pass(inner);
    }
}
```

Find:
```rust
    format!("Normalize: {w}")
}

/// "Busy" for Normalize.
fn still_building(name: &str) -> VizError {
    VizError::Busy {
```
Replace with:
```rust
    format!("Normalize: {w}")
}

/// `Busy` while a recording runs or finishes.
fn recording_busy() -> VizError {
    VizError::Busy {
        message: "Stop recording first.".into(),
    }
}

/// "Busy" for Normalize.
fn still_building(name: &str) -> VizError {
    VizError::Busy {
```

Find:
```rust
            self.mark(&mut inner);
            inner.snapshot()
        })
    }
}
```
Replace with:
```rust
            self.mark(&mut inner);
            inner.snapshot()
        })
    }
}

/// Recording (plan 5c Task 9; spec §6.2–§6.5).
#[uniffi::export]
impl Session {
    /// Records into sequence `seq` from where its position is (spec §6.4):
    /// the transport rolls (from `pre_roll_bars` bars earlier), each armed
    /// lane with an audio input records its channels into its own file
    /// under `Media/Recorded/`, armed MIDI lanes record what they hear, and
    /// at Stop everything recorded becomes one undo step "Record". `Model`
    /// when no armed lane has an input, an audio lane names a channel the
    /// input does not have, the spec is out of range, or count-in is asked
    /// for (plan 5d plays it); `Busy` while recording, while an edit is
    /// open, or when the disk is almost full.
    pub fn record(&self, seq: u64, spec: crate::record::RecordSpecRow) -> Result<(), VizError> {
        self.transport_call(|| self.start_recording(seq, spec))
    }

    /// `record.keep_pass`: keeps every loop pass completed so far (they
    /// survive a later discard). `Model` unless loop recording.
    pub fn keep_pass(&self) -> Result<(), VizError> {
        ffi_guard(|| self.press_pass(true))
    }

    /// `record.discard_pass`: drops every loop pass completed since the
    /// last keep. `Model` unless loop recording.
    pub fn discard_pass(&self) -> Result<(), VizError> {
        ffi_guard(|| self.press_pass(false))
    }

    /// What recording is doing (`state.get record`).
    pub fn record_state(&self) -> Result<crate::record::RecordStateRow, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            Ok(match inner.recording.as_mut() {
                Some(r) => {
                    r.drain();
                    r.row()
                }
                None => crate::record::RecordStateRow {
                    recording: false,
                    seq: None,
                    spec: None,
                    pass: 0,
                    kept: 0,
                    discarded: 0,
                    lanes: Vec::new(),
                    frames: 0,
                    overflows: 0,
                    gap_frames: 0,
                    free_bytes: crate::record::free_bytes(&inner.root(self.id)),
                },
            })
        })
    }
}

impl Session {
    fn press_pass(&self, keep: bool) -> Result<(), VizError> {
        let mut inner = lock(&self.inner);
        match inner.recording.as_mut() {
            Some(r) => r.press(keep),
            None => Err(crate::events::bad("nothing is recording")),
        }
    }

    /// [`Session::record`].
    fn start_recording(
        &self,
        seq: u64,
        spec: crate::record::RecordSpecRow,
    ) -> Result<(), VizError> {
        use crate::record::{AudioLane, LOW_DISK_STOP, LOW_DISK_WARN, Plan, Recording};
        use core_engine::record::RecordRequest;
        use core_media::record::{RING_SECONDS, WriterConfig, WriterLane};
        use core_model::lane::LaneInput;
        let mut inner = lock(&self.inner);
        if inner.recording.is_some() || inner.finishing {
            return Err(VizError::Busy {
                message: "A recording is already running.".into(),
            });
        }
        if inner.history.in_transaction() || inner.history.in_group() {
            return Err(VizError::Busy {
                message: "Finish the open edit first.".into(),
            });
        }
        if spec.count_in_bars > 4 || spec.pre_roll_bars > 8 {
            return Err(crate::events::bad(
                "count-in is 0–4 bars and pre-roll 0–8 bars",
            ));
        }
        if spec.count_in_bars > 0 {
            return Err(crate::events::bad(
                "count-in arrives with the record controls (plan 5d); record with a count-in of 0",
            ));
        }
        if let Some(p) = &spec.punch {
            core_model::rules::check_ticks("punch", p.start)?;
            core_model::rules::check_ticks("punch", p.end)?;
            if p.start >= p.end {
                return Err(crate::events::bad("the punch range is empty"));
            }
        }
        let doc = inner.doc().clone();
        let id = seq_id(&doc, seq)?;
        let s = doc.sequence(id)?;
        let mut audio: Vec<(
            core_model::ids::LaneId,
            String,
            core_model::lane::InputChannels,
        )> = Vec::new();
        let mut midi_lanes = Vec::new();
        for l in s.lanes.iter().filter(|l| l.arm) {
            match &l.input {
                Some(LaneInput::Audio { channels }) => {
                    audio.push((l.id, l.name.clone(), *channels))
                }
                Some(LaneInput::Midi { .. }) => midi_lanes.push(l.id),
                None => {}
            }
        }
        if audio.is_empty() && midi_lanes.is_empty() {
            return Err(crate::events::bad("arm a lane that has an input to record"));
        }
        let root = inner.root(self.id);
        if !audio.is_empty() {
            match crate::record::free_bytes(&root) {
                Some(f) if f < LOW_DISK_STOP => {
                    return Err(VizError::Busy {
                        message: "The disk is almost full: free some space to record.".into(),
                    });
                }
                Some(f) if f < LOW_DISK_WARN => inner
                    .news
                    .problems
                    .push("Recording: less than 1 GB of disk space is left.".into()),
                _ => {}
            }
        }
        // Where it starts: the position as heard, then the pre-roll.
        let format = doc.settings().format;
        let rate = f64::from(format.sample_rate);
        let origin = Tick(-s.start_offset.0);
        let tempo = core_engine::schedule::TempoTable::build(&s.tempo, origin, rate);
        let at = match host::position(self.id) {
            Some((sq, t)) if sq == id => t,
            _ => match (inner.cursor, inner.cursor_seq) {
                (Some(t), None) => t,
                (Some(t), Some(sq)) if sq == id => t,
                _ => Tick::ZERO,
            },
        };
        let bar = s.meter.entry_at(at).meter.bar_ticks();
        let play_from = Tick(
            at.0.saturating_sub(bar.saturating_mul(i64::from(spec.pre_roll_bars)))
                .max(origin.0),
        );
        let frame = |t: i64| tempo.tick_to_frame(Tick(t));
        let punch = spec.punch.as_ref().map(|p| (frame(p.start), frame(p.end)));
        let looped = spec.looped.then(|| {
            let end = core_model::query::seq_length(&doc, id).unwrap_or(Tick::ZERO);
            (frame(0), frame(end.0))
        });
        let range = match (looped, punch) {
            (Some((a, b)), Some((pa, pb))) => crate::passes::RecordRange {
                start: a.max(pa),
                end: Some(b.min(pb)),
            },
            (Some((a, b)), None) => crate::passes::RecordRange {
                start: a,
                end: Some(b),
            },
            (None, Some((pa, pb))) => crate::passes::RecordRange {
                start: pa,
                end: Some(pb),
            },
            (None, None) => crate::passes::RecordRange {
                start: frame(at.0),
                end: None,
            },
        };
        if range.end.is_some_and(|e| e <= range.start) {
            return Err(crate::events::bad(
                "the record range is empty (an empty loop, or a punch outside it)",
            ));
        }
        // The engine plays (this loads the sequence and locates); the tap
        // goes in before the Play, so the roll is the first block captured.
        inner.mark(self.id, true);
        let played = plugins::with(self.id, |p| {
            host::play(self.id, &doc, id, Some(play_from), None, spec.looped, p)
        })
        .and_then(|r| r);
        let pending = match played {
            Ok(p) => p,
            Err(e) => {
                inner.reload_owed = true;
                return Err(e);
            }
        };
        let fail = |e: VizError| {
            let _ = host::stop_recording(self.id);
            Err(e)
        };
        // A device at another rate than the project's would record at the
        // wrong speed and place MIDI wrongly (5a Ruling 20: no resampling).
        if let Some((true, rate)) = host::device(self.id) {
            return fail(crate::events::bad(&format!(
                "the device runs at {rate} Hz, not the project's {} Hz: change one of them to record",
                format.sample_rate
            )));
        }
        let inputs = host::input_info(self.id).map_or(0, |(_, i)| i.channels);
        for (_, name, ch) in &audio {
            if ch.last().is_none_or(|c| u32::from(c) >= inputs) {
                return fail(crate::events::bad(&format!(
                    "lane {name} records input {}, but the audio input has {inputs} channel(s)",
                    u32::from(ch.first()) + 1
                )));
            }
        }
        let mut device: Vec<u16> = audio
            .iter()
            .flat_map(|(_, _, ch)| {
                let a = ch.first();
                [a, ch.last().unwrap_or(a)]
            })
            .collect();
        device.sort_unstable();
        device.dedup();
        let req = RecordRequest {
            channels: if audio.is_empty() {
                Vec::new()
            } else {
                device.clone()
            },
            range: Some((range.start, range.end.unwrap_or(u64::MAX))),
            ring_frames: (RING_SECONDS * rate) as usize,
        };
        let handles = match host::start_recording(self.id, &req) {
            Ok(h) => h,
            Err(e) => return fail(e),
        };
        let mut reserved = inner.reserved_names();
        let now = std::time::SystemTime::now();
        let mut lanes = Vec::new();
        let mut writer_lanes = Vec::new();
        for (lane, name, ch) in &audio {
            let rel = crate::record::recorded_path(&root, name, now, &reserved);
            reserved.insert(rel.clone());
            let index = |c: u16| device.iter().position(|d| *d == c).unwrap_or(0);
            let mut chans = vec![index(ch.first())];
            if let Some(r) = ch.last().filter(|r| *r != ch.first()) {
                chans.push(index(r));
            }
            writer_lanes.push(WriterLane {
                path: root.join(&rel),
                channels: chans,
            });
            lanes.push(AudioLane {
                lane: *lane,
                path: rel,
                channels: ch.count(),
            });
        }
        let writer = (!lanes.is_empty()).then_some(WriterConfig {
            rate: format.sample_rate,
            depth: format.depth,
            lanes: writer_lanes,
        });
        let plan = Plan {
            seq: id,
            spec: spec.clone(),
            range,
            audio: lanes,
            midi_lanes,
            root,
            format,
            pass_number: crate::record::next_pass(&doc),
        };
        let recording = match Recording::start(plan, handles, writer) {
            Ok(r) => r,
            Err(e) => {
                return fail(VizError::Io {
                    message: format!("the recording could not start: {e}"),
                });
            }
        };
        if let Err(e) = inner.history.begin_group() {
            return fail(e.into());
        }
        inner.recording = Some(recording);
        inner.looped = spec.looped;
        inner.reload_owed = false;
        inner.reload_fault = None;
        inner.cursor = None;
        inner.cursor_seq = Some(id);
        drop(inner);
        host::finish_play(pending)
    }

    /// Ends the recording in progress (spec §6.5): the engine stops and
    /// drops its tap, the files are finished without the session lock,
    /// then the pass is applied as the last step of the recording's group,
    /// which closes as one undo step. `reason` is told to the user.
    fn finish_recording(&self, reason: Option<String>) -> Result<(), VizError> {
        let rec = {
            let mut inner = lock(&self.inner);
            let Some(r) = inner.recording.take() else {
                return Ok(());
            };
            inner.finishing = true;
            let _ = host::stop_recording(self.id);
            r
        };
        let done = rec.finish(crate::record::FINISH_WAIT);
        let mut inner = lock(&self.inner);
        inner.finishing = false;
        if let Some(why) = reason {
            inner
                .news
                .problems
                .push(format!("Recording stopped: {why}."));
        }
        let files = done.files();
        let doc = inner.doc().clone();
        let pass = match crate::record::build(&done, &doc) {
            Ok(p) => p,
            Err(e) => {
                inner.news.problems.push(format!(
                    "The recording could not be placed: {e}. Its files are kept."
                ));
                inner.keep_recordings.extend(files.iter().cloned());
                None
            }
        };
        inner.pending_pass = Some((pass, files));
        self.apply_pending_pass(&mut inner);
        Ok(())
    }

    /// Applies a finished recording's pass and closes its group, unless
    /// an edit is still open (then `end_edit` or the next `idle` does).
    fn apply_pending_pass(&self, inner: &mut Inner) {
        if inner.history.in_transaction() {
            return;
        }
        let Some((pass, files)) = inner.pending_pass.take() else {
            return;
        };
        let label = pass
            .as_ref()
            .map_or("Record", core_model::command::RecordPass::name);
        if let Some(p) = pass {
            if let Err(e) = inner
                .history
                .execute(&Command::Record(p), &ViewFilter::default())
            {
                inner.news.problems.push(format!(
                    "The recording could not be added ({e}). Its files are kept."
                ));
                inner.keep_recordings.extend(files);
            }
        }
        let _ = inner.history.end_group(label);
        inner.version += 1;
        inner.news.doc_changed = true;
        inner.news.media_changed = true;
        inner.reload_host(self.id);
        self.mark(inner);
    }

    /// `idle`'s recording step: ends the recording when the input went
    /// away, the disk failed or filled, or the transport stopped by itself.
    fn service_recording(&self) -> Result<(), VizError> {
        let ending = {
            let mut inner = lock(&self.inner);
            let stopped = host::transport_stopped(self.id).unwrap_or(false);
            let mut problems = Vec::new();
            let e = inner
                .recording
                .as_mut()
                .and_then(|r| r.check(stopped, &mut problems));
            inner.news.problems.extend(problems);
            e
        };
        match ending {
            None => Ok(()),
            Some(crate::record::Ending::Ended) => self.finish_recording(None),
            Some(crate::record::Ending::Because(why)) => self.finish_recording(Some(why)),
        }
    }
}
```

In `crates/core-ffi/src/host.rs`:

Find:
```rust
    }
    if host.owner != owner || host.seq != seq || start.is_some() {
        host.engine.stop()?;
        host.load(owner, doc, seq, looped)?;
        host.engine.locate_tick(start.unwrap_or(Tick::ZERO))?;
        host.owner = owner;
```
Replace with:
```rust
    }
    if host.owner != owner || host.seq != seq || start.is_some() {
        host.engine.stop()?;
        if host.owner != owner {
            // Another session's recording ends here: its tap must not
            // capture this one's playback.
            host.engine.stop_recording()?;
        }
        host.load(owner, doc, seq, looped)?;
        host.engine.locate_tick(start.unwrap_or(Tick::ZERO))?;
        host.owner = owner;
```

Find:
```rust
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.latency_frames())
}
```
Replace with:
```rust
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.latency_frames())
}

/// Where `owner`'s transport is heard, when it holds the engine.
pub fn position(owner: u64) -> Option<(SeqId, Tick)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.seq, h.engine.position_tick()))
}
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod overviews;
pub mod passes;
pub mod plugins;
pub mod recovery;
pub mod relink;
pub mod retention;
```
Replace with:
```rust
pub mod overviews;
pub mod passes;
pub mod plugins;
pub mod record;
pub mod recovery;
pub mod relink;
pub mod retention;
```

Find:
```rust
    NoteMapEntry, RuleRow, SelectModeRow, SettingsEdit, SubstituteRow, ValueTargetRow, WaveRow,
};
pub use overviews::{OverviewRows, OverviewStateRow};
pub use recovery::RecoveryInfo;
pub use revs::LaneRev;
pub use selection::{SelectionRow, TickRange};
```
Replace with:
```rust
    NoteMapEntry, RuleRow, SelectModeRow, SettingsEdit, SubstituteRow, ValueTargetRow, WaveRow,
};
pub use overviews::{OverviewRows, OverviewStateRow};
pub use record::{RecordLaneRow, RecordSpecRow, RecordStateRow, set_record_latency_trim};
pub use recovery::RecoveryInfo;
pub use revs::LaneRev;
pub use selection::{SelectionRow, TickRange};
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift

    func setLoop(on: Bool) throws {}
    func snapshot() throws -> DocSnapshot { snap }
    func stop() throws { state.playing = false }
    func transport() throws -> TransportState {
        if let e = transportError { throw e }
        return state
```
Replace with:
```swift

    func setLoop(on: Bool) throws {}
    func snapshot() throws -> DocSnapshot { snap }
    func stop() throws {
        state.playing = false
        recordRow.recording = false
    }
    func transport() throws -> TransportState {
        if let e = transportError { throw e }
        return state
```

Find:
```swift
    }

    func inputInfo() throws -> InputInfoRow { inputRow }

    // MARK: Media pool (plan 5b Task 6)
```
Replace with:
```swift
    }

    func inputInfo() throws -> InputInfoRow { inputRow }

    // MARK: Recording (plan 5c Task 9)

    var recordCalls: [(seq: UInt64, spec: RecordSpecRow)] = []
    var recordError: VizError?
    var passPresses: [Bool] = []
    var recordRow = RecordStateRow(
        recording: false, seq: nil, spec: nil, pass: 0, kept: 0, discarded: 0, lanes: [], frames: 0,
        overflows: 0, gapFrames: 0, freeBytes: nil)

    func record(seq: UInt64, spec: RecordSpecRow) throws {
        if let e = recordError { throw e }
        recordCalls.append((seq, spec))
        recordRow.recording = true
        recordRow.seq = seq
        recordRow.spec = spec
        state.playing = true
        state.seq = seq
    }

    func keepPass() throws { passPresses.append(true) }
    func discardPass() throws { passPresses.append(false) }
    func recordState() throws -> RecordStateRow { recordRow }

    // MARK: Media pool (plan 5b Task 6)
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test record 2>&1 | tail -3
```
Expected: `test result: ok. 13 passed`: every Review Focus item 1–4 test of this task, `a_looped_back_click_lands_within_two_frames_of_where_it_played` and `a_device_at_another_rate_refuses_recording` among them.

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/record.rs crates/core-ffi/src/session.rs crates/core-ffi/src/host.rs crates/core-ffi/src/lib.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-ffi/tests/record.rs
git commit -F - <<'VIZDAW_MSG'
core-ffi: recording in the session

Session.record checks the request, makes Media/Recorded files, starts
the writer and the tap and opens a history group; Stop, a lost device,
a full disk or the song's end finish the pass outside the session
lock, plan it and apply it as one "Record" step with every edit made
while recording. Loop passes are kept or discarded; recordings are
kept by retention and go to the Trash when an untitled project closes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 10: `core-ffi` — crash recovery of recordings

**Files:**
- Modify: `crates/core-ffi/src/record.rs` (`recovered`, `RecordStateRow.recovered`)
- Modify: `crates/core-ffi/src/session.rs` (`recovered_recordings`, `recover_recordings`, the keep set)
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/recovered.rs`

**Interfaces:**
- Consumes: Task 4's recording-mode CAFs; Task 9's session.
- Produces: `record::recovered(package, doc) -> Vec<String>`; `RecordStateRow.recovered: Vec<String>` (`#[uniffi(default)]`); `Session.recovered_recordings() -> Vec<String>`, `Session.recover_recordings(import: bool) -> Result<(), VizError>` (import as one step "Recover Recordings", or move to the Trash).

Ruling 21. A recording a crash left is a recording-mode CAF in `Media/Recorded/` that no media item names; recovery never deletes it.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/recovered.rs <<'VIZDAW_EOF'
//! Crash recovery of recordings (plan 5c Task 10; sub-project 5 spec
//! §6.5, §12.3 "crash-recovered files are found and offered"): files a
//! crash left in `Media/Recorded/` are offered at open, kept from the
//! Trash until answered, imported onto new lanes as one undo step, or
//! moved to the Trash, never deleted.

#![recursion_limit = "256"]

mod common;

use core_engine::fixtures::Builder;
use core_engine::offline::FakeInput;
use core_media::caf::{CafWriter, WriteMode};
use core_model::command::{Command, LaneCmd};
use core_model::format::BitDepth;
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input};
use vizdaw_ffi::session::simulate_crash;
use vizdaw_ffi::{MediaOriginRow, RecordSpecRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(Some(FakeInput::Signals));
    reset_host();
    g
}

/// A saved project `P.vizdaw` in `dir`: sequence "S" with an armed lane
/// "Mic" recording fake input 1 (monitor Off).
fn project(dir: &Path) -> (std::sync::Arc<Session>, std::path::PathBuf, u64) {
    let mut b = Builder::new();
    let (seq, mic) = b.sequence("S", "Mic", None).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq,
        lane: mic,
        input: Some(LaneInput::Audio {
            channels: InputChannels::Mono(0),
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetMonitor {
        seq,
        lane: mic,
        monitor: MonitorMode::Off,
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq,
        lane: mic,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    let s = Session::from_document(b.finish()).unwrap();
    s.debug_set_trash_dir(Some(dir.join("Trash").display().to_string()));
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    (s, pkg, seq.0)
}

/// A recording-mode CAF of `frames` frames left unfinished (as a crash
/// leaves it).
fn unfinished(path: &Path, frames: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut w = CafWriter::create(path, 48_000, 1, BitDepth::Int24, WriteMode::Recording).unwrap();
    w.write(&vec![0.25; frames]).unwrap();
    drop(w);
}

#[test]
fn a_recording_a_crash_left_is_found_and_offered() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, seq) = project(dir.path());
    s.record(
        seq,
        RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let rel = s.record_state().unwrap().lanes[0].path.clone().unwrap();
    let until = Instant::now() + common::wait_limit();
    while s.record_state().unwrap().frames < 9600 {
        assert!(Instant::now() < until, "nothing recorded");
        std::thread::sleep(Duration::from_millis(5));
    }
    // VizDAW crashes: nothing finishes the file or applies the pass.
    simulate_crash(s);
    reset_host();
    // Things that are not crash leftovers: a hidden file, a file that is
    // not a recording, and garbage.
    unfinished(&pkg.join("Media/Recorded/.hidden.caf"), 10);
    let mut w = CafWriter::create(
        &pkg.join("Media/Recorded/plain.caf"),
        48_000,
        1,
        BitDepth::Int24,
        WriteMode::Normal,
    )
    .unwrap();
    w.write(&[0.1; 10]).unwrap();
    w.finish().unwrap();
    std::fs::write(pkg.join("Media/Recorded/junk.caf"), b"not audio").unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    assert_eq!(s.recovered_recordings().unwrap(), vec![rel.clone()]);
    assert_eq!(s.record_state().unwrap().recovered, vec![rel.clone()]);
    // Unanswered, a save keeps it from the Trash.
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    s.save().unwrap();
    assert!(pkg.join(&rel).is_file());
    // Imported: on a new lane at bar 1, one undo step.
    s.recover_recordings(true).unwrap();
    assert!(s.recovered_recordings().unwrap().is_empty());
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Recover Recordings")
    );
    let m = s.media().unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(
        (m[0].origin, m[0].offline, &m[0].path),
        (MediaOriginRow::Recorded, None, &rel)
    );
    assert!(m[0].frames >= 9600, "{}", m[0].frames);
    let lanes = s.lanes(seq).unwrap();
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[1].region_count, 1);
    s.undo().unwrap();
    assert!(s.media().unwrap().is_empty());
    assert!(pkg.join(&rel).is_file(), "the file stays for redo");
    assert!(matches!(
        s.recover_recordings(true),
        Err(VizError::Model { .. })
    ));
    s.close().unwrap();
}

#[test]
fn declined_recordings_go_to_the_trash() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, _) = project(dir.path());
    s.close().unwrap();
    let rel = "Media/Recorded/Mic 2026-10-20 120000.caf";
    unfinished(&pkg.join(rel), 4800);
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    assert_eq!(s.recovered_recordings().unwrap(), vec![rel.to_string()]);
    s.recover_recordings(false).unwrap();
    assert!(s.recovered_recordings().unwrap().is_empty());
    assert!(!pkg.join(rel).exists());
    assert!(
        dir.path().join("Trash/Mic 2026-10-20 120000.caf").is_file(),
        "moved, never deleted"
    );
    assert!(s.media().unwrap().is_empty());
    s.close().unwrap();
    // Nothing is offered again.
    let s = Session::open(pkg.display().to_string()).unwrap();
    assert!(s.recovered_recordings().unwrap().is_empty());
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test recovered 2>&1 | tail -5
```
Expected: does not compile: `Session::recovered_recordings` does not exist.

- [ ] **Step 3: Implement**

In `crates/core-ffi/src/record.rs`:

Find:
```rust
    pub gap_frames: u64,
    /// Free space on the project's volume.
    pub free_bytes: Option<u64>,
}

/// One armed audio lane's file.
```
Replace with:
```rust
    pub gap_frames: u64,
    /// Free space on the project's volume.
    pub free_bytes: Option<u64>,
    /// Recordings found after a crash and not yet imported or moved to
    /// the Trash (package-relative; plan 5c Task 10).
    #[uniffi(default)]
    pub recovered: Vec<String>,
}

/// One armed audio lane's file.
```

Find:
```rust
            overflows: self.status.lost_blocks(),
            gap_frames: w.map_or(0, |w| w.gap_frames),
            free_bytes: free_bytes(&self.root),
        }
    }
```
Replace with:
```rust
            overflows: self.status.lost_blocks(),
            gap_frames: w.map_or(0, |w| w.gap_frames),
            free_bytes: free_bytes(&self.root),
            recovered: Vec::new(),
        }
    }
```

Find:
```rust
    }
    rel
}
```
Replace with:
```rust
    }
    rel
}

/// The recordings a crash left in `package` (spec §6.5 "Crash
/// recovery"): regular `.caf` files directly in `Media/Recorded/` (not
/// hidden) that carry the `vizdaw-recording` mark and that no media item
/// of `doc` names, package-relative and sorted. A file that is not a
/// readable VizDAW CAF is never offered.
pub fn recovered(package: &Path, doc: &Document) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(package.join(RECORDED_DIR)) else {
        return Vec::new();
    };
    let named: BTreeSet<String> = doc
        .media()
        .values()
        .map(|m| m.path.to_lowercase())
        .collect();
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if name.starts_with('.') || !name.to_lowercase().ends_with(".caf") {
                return None;
            }
            let rel = format!("{RECORDED_DIR}/{name}");
            if named.contains(&rel.to_lowercase()) || !e.file_type().ok()?.is_file() {
                return None;
            }
            let r = core_media::caf::CafReader::open(&e.path()).ok()?;
            (r.info().recording && r.info().frames > 0).then_some(rel)
        })
        .collect();
    out.sort();
    out
}
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// (a pass that could not be applied): crash recovery offers them at
    /// the next open.
    keep_recordings: std::collections::BTreeSet<String>,
}

/// The plugins the crash marker names: those loading now, and with
```
Replace with:
```rust
    /// (a pass that could not be applied): crash recovery offers them at
    /// the next open.
    keep_recordings: std::collections::BTreeSet<String>,
    /// Recordings a crash left, offered and not yet answered (plan 5c
    /// Task 10): the Trash leaves them until they are.
    recovered: Vec<String>,
}

/// The plugins the crash marker names: those loading now, and with
```

Find:
```rust
            finishing: false,
            pending_pass: None,
            keep_recordings: Default::default(),
        }
    }
```
Replace with:
```rust
            finishing: false,
            pending_pass: None,
            keep_recordings: Default::default(),
            recovered: Vec::new(),
        }
    }
```

Find:
```rust
    /// 5c Ruling 15).
    fn recorded_keep(&self) -> Vec<String> {
        let mut out: Vec<String> = self.keep_recordings.iter().cloned().collect();
        if let Some(r) = &self.recording {
            out.extend(r.files());
        }
```
Replace with:
```rust
    /// 5c Ruling 15).
    fn recorded_keep(&self) -> Vec<String> {
        let mut out: Vec<String> = self.keep_recordings.iter().cloned().collect();
        out.extend(self.recovered.iter().cloned());
        if let Some(r) = &self.recording {
            out.extend(r.files());
        }
```

Find:
```rust
            crate::media::sweep_temp(&p);
            let windows = read_windows(&p);
            let layout = read_layout(&p);
            let s = Session::with(doc, Some(p), options.plugins_off)?;
            {
                let mut inner = lock(&s.inner);
                inner.windows = windows;
                inner.editor_layout = layout;
                inner.clean = clean;
```
Replace with:
```rust
            crate::media::sweep_temp(&p);
            let windows = read_windows(&p);
            let layout = read_layout(&p);
            let recovered = crate::record::recovered(&p, &doc);
            let s = Session::with(doc, Some(p), options.plugins_off)?;
            {
                let mut inner = lock(&s.inner);
                inner.recovered = recovered;
                inner.windows = windows;
                inner.editor_layout = layout;
                inner.clean = clean;
```

Find:
```rust
                    overflows: 0,
                    gap_frames: 0,
                    free_bytes: crate::record::free_bytes(&inner.root(self.id)),
                },
            })
        })
    }
}
```
Replace with:
```rust
                    overflows: 0,
                    gap_frames: 0,
                    free_bytes: crate::record::free_bytes(&inner.root(self.id)),
                    recovered: Vec::new(),
                },
            })
            .map(|mut row| {
                row.recovered = inner.recovered.clone();
                row
            })
        })
    }

    /// Recordings a crash left in this project, offered at open (spec
    /// §6.5: "Recovered recordings: import into a new lane / move to
    /// Trash"); empty once answered.
    pub fn recovered_recordings(&self) -> Result<Vec<String>, VizError> {
        ffi_guard(|| Ok(lock(&self.inner).recovered.clone()))
    }

    /// Answers the offer: `import` puts each recovered file on a new lane
    /// of the first top-level sequence at bar 1, as one undo step "Recover
    /// Recordings"; otherwise each moves to the Trash (never unlinked; a
    /// file the Trash refuses stays and is reported). `Model` when there
    /// is nothing to recover; `Busy` while an edit or a recording is open.
    pub fn recover_recordings(&self, import: bool) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if inner.recovered.is_empty() {
                return Err(crate::events::bad("there are no recovered recordings"));
            }
            if inner.recording.is_some() || inner.history.in_transaction() {
                return Err(VizError::Busy {
                    message: "Finish the open edit or recording first.".into(),
                });
            }
            let root = inner.root(self.id);
            let files = std::mem::take(&mut inner.recovered);
            if !import {
                let dir = crate::retention::trash_dir(inner.trash_dir.as_deref());
                for rel in files {
                    if let Err(e) = crate::retention::trash(dir.as_deref(), &root.join(&rel)) {
                        inner
                            .news
                            .problems
                            .push(format!("{rel}: not moved to the Trash ({e})"));
                    }
                }
                return inner.snapshot();
            }
            match recover_into_lanes(&mut inner, &root, &files) {
                Ok(problems) => inner.news.problems.extend(problems),
                Err(e) => {
                    // Offered again: nothing was applied.
                    inner.recovered = files;
                    return Err(e);
                }
            }
            inner.version += 1;
            inner.news.media_changed = true;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.snapshot()
        })
    }
}

/// [`Session::recover_recordings`]'s import: one transaction "Recover
/// Recordings" adding each readable file as a recorded media item and a
/// region at bar 1 on its own new lane (named after the file) in the
/// first top-level sequence. Unreadable files become problems.
fn recover_into_lanes(
    inner: &mut Inner,
    root: &Path,
    files: &[String],
) -> Result<Vec<String>, VizError> {
    use core_model::command::{AudioCmd, MediaCmd};
    use core_model::document::{MediaItem, MediaOrigin};
    use core_model::event::Fade;
    let seq = seq_rows(inner.doc())?
        .iter()
        .find(|r| r.top_level)
        .map(|r| SeqId(r.id))
        .ok_or_else(|| crate::events::bad("the project has no sequence to recover into"))?;
    let pass = crate::record::next_pass(inner.doc());
    let mut problems = Vec::new();
    let mut found = Vec::new();
    for rel in files {
        let path = root.join(rel);
        let info = core_media::caf::CafReader::open(&path).map(|r| r.info().clone());
        match (info, core_media::caf::hash_file(&path)) {
            (Ok(i), Ok(h)) => found.push((rel.clone(), i, h)),
            (Err(e), _) | (_, Err(e)) => problems.push(format!("{rel}: {e}")),
        }
    }
    if found.is_empty() {
        return Ok(problems);
    }
    let filter = ViewFilter::default();
    let h = &mut inner.history;
    h.begin("Recover Recordings")?;
    let run = (|| -> Result<(), VizError> {
        for (rel, info, hash) in &found {
            let (_, stem) = MediaItem::file_parts(rel);
            let r = h.execute(
                &Command::Media(MediaCmd::Add {
                    path: rel.clone(),
                    sample_rate: info.sample_rate,
                    channels: info.channels,
                    length_frames: info.frames,
                    content_hash: Some(hash.clone()),
                    name: Some(stem.clone()),
                    origin: Some(MediaOrigin::Recorded { pass }),
                }),
                &filter,
            )?;
            let media = r
                .created
                .iter()
                .find_map(|id| match id {
                    core_model::ids::AnyId::Media(m) => Some(*m),
                    _ => None,
                })
                .ok_or_else(|| VizError::Internal {
                    message: "no media created".into(),
                })?;
            let r = h.execute(
                &Command::Lane(LaneCmd::Add {
                    seq,
                    name: stem,
                    index: None,
                }),
                &filter,
            )?;
            let lane = r
                .created
                .iter()
                .find_map(|id| match id {
                    core_model::ids::AnyId::Lane(l) => Some(*l),
                    _ => None,
                })
                .ok_or_else(|| VizError::Internal {
                    message: "no lane created".into(),
                })?;
            h.execute(
                &Command::Audio(AudioCmd::Insert {
                    seq,
                    lane,
                    tick: Tick::ZERO,
                    media,
                    source_offset: 0,
                    length: info.frames,
                    gain_db: 0.0,
                    fade_in: Fade::default(),
                    fade_out: Fade::default(),
                }),
                &filter,
            )?;
        }
        Ok(())
    })();
    match run {
        Ok(()) => {
            h.commit()?;
            Ok(problems)
        }
        Err(e) => {
            let _ = h.abort();
            Err(e)
        }
    }
}
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
    var passPresses: [Bool] = []
    var recordRow = RecordStateRow(
        recording: false, seq: nil, spec: nil, pass: 0, kept: 0, discarded: 0, lanes: [], frames: 0,
        overflows: 0, gapFrames: 0, freeBytes: nil)

    func record(seq: UInt64, spec: RecordSpecRow) throws {
        if let e = recordError { throw e }
```
Replace with:
```swift
    var passPresses: [Bool] = []
    var recordRow = RecordStateRow(
        recording: false, seq: nil, spec: nil, pass: 0, kept: 0, discarded: 0, lanes: [], frames: 0,
        overflows: 0, gapFrames: 0, freeBytes: nil, recovered: [])

    func record(seq: UInt64, spec: RecordSpecRow) throws {
        if let e = recordError { throw e }
```

Find:
```swift
    func keepPass() throws { passPresses.append(true) }
    func discardPass() throws { passPresses.append(false) }
    func recordState() throws -> RecordStateRow { recordRow }

    // MARK: Media pool (plan 5b Task 6)
```
Replace with:
```swift
    func keepPass() throws { passPresses.append(true) }
    func discardPass() throws { passPresses.append(false) }
    func recordState() throws -> RecordStateRow { recordRow }

    var recovered: [String] = []
    var recoverAnswers: [Bool] = []

    func recoveredRecordings() throws -> [String] { recovered }

    func recoverRecordings(import: Bool) throws -> DocSnapshot {
        recoverAnswers.append(`import`)
        recovered = []
        return commandResult()
    }

    // MARK: Media pool (plan 5b Task 6)
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test recovered 2>&1 | tail -3
```
Expected: `test result: ok. 2 passed` (`a_recording_a_crash_left_is_found_and_offered`, `declined_recordings_go_to_the_trash`; Review Focus 3).

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/record.rs crates/core-ffi/src/session.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-ffi/tests/recovered.rs
git commit -F - <<'VIZDAW_MSG'
core-ffi: crash recovery of recordings

At open, recording-mode CAFs in Media/Recorded that no media item
names are offered: imported onto new lanes as one undo step, or moved
to the Trash. Until answered they are kept by retention.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 11: `platform-macos` — HAL input, the aggregate device and the microphone

**Files:**
- Create: `crates/platform-macos/src/permission.rs`
- Modify: `crates/platform-macos/src/hal.rs` (duplex input, private aggregate, latencies, input delay, audible time)
- Modify: `crates/platform-macos/src/lib.rs`, `crates/platform-macos/Cargo.toml` (features of `objc2-core-foundation`)
- Modify: `crates/core-ffi/src/host.rs` (`set_audio_input_device`, `live_input_allowed`, `Host::ensure_input`)
- Modify: `app/Package.swift` (AVFoundation), `scripts/make-app.sh` (`NSMicrophoneUsageDescription`)
- Test: unit tests in `hal.rs` and `permission.rs`; extend `crates/core-ffi/tests/live_input.rs`

**Interfaces:**
- Consumes: Task 5's `InputBlock`, `AudioConfig.input_enabled`/`input_device`, `StreamInfo` input fields.
- Produces:
  - `permission::{MicAccess { Granted, Denied, Undetermined }, access_of(status), microphone(), DENIED_MESSAGE}`;
  - in `hal.rs`, pure: `audible_ns(host_ns, output_latency, rate)`, `input_delay(out_sample, in_sample, output_latency, input_latency)`, `deinterleave(list, offset, inputs, frames)` (private, `unsafe`, `// RT`), `aggregate_description(uid, output, input)`; device: `input_devices()`, input opening in `CoreAudioBackend::open`;
  - `vizdaw_ffi::set_audio_input_device(name: Option<String>)`; `host::wants_audio_input(doc, seq)`.

Ruling 19 and spec §6.1, §6.3. **No test here opens input, makes an aggregate or asks for the microphone** (Global Constraints): the tests cover the pure helpers, and `live_input_allowed` is false under `test-hooks`. The README steps of Task 15 check the device paths by eye.

- [ ] **Step 1: Write the failing tests**

In `crates/core-ffi/tests/live_input.rs`:

Find:
```rust
        assert!(Instant::now() < until, "Off still heard the input");
    }
    s.close().unwrap();
}
```
Replace with:
```rust
        assert!(Instant::now() < until, "Off still heard the input");
    }
    s.close().unwrap();
}

/// Spec §6.1: audio input is wanted only while an audio lane is armed or
/// monitors its input.
#[test]
fn audio_input_is_wanted_only_for_armed_or_monitored_audio_lanes() {
    use core_model::command::{Command, LaneCmd, apply};
    use core_model::lane::{InputChannels, LaneInput, MonitorMode};
    use vizdaw_ffi::host::wants_audio_input;
    let doc = core_engine::fixtures::demo_document().unwrap();
    let song = core_engine::engine::find_sequence(&doc, "Song").unwrap();
    let lane = doc.sequence(song).unwrap().lanes[0].id;
    assert!(!wants_audio_input(&doc, song));
    let run =
        |d: &core_model::document::Document, c: LaneCmd| apply(d, &Command::Lane(c)).unwrap().doc;
    let audio = run(
        &doc,
        LaneCmd::SetInput {
            seq: song,
            lane,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(0),
            }),
        },
    );
    assert!(!wants_audio_input(&audio, song), "an input alone");
    let monitored = run(
        &audio,
        LaneCmd::SetMonitor {
            seq: song,
            lane,
            monitor: MonitorMode::Input,
        },
    );
    assert!(wants_audio_input(&monitored, song));
    let armed = run(
        &audio,
        LaneCmd::SetFlags {
            seq: song,
            lane,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    );
    assert!(wants_audio_input(&armed, song));
    let midi = run(
        &armed,
        LaneCmd::SetInput {
            seq: song,
            lane,
            input: Some(LaneInput::Midi {
                port: None,
                channel: None,
            }),
        },
    );
    assert!(!wants_audio_input(&midi, song), "MIDI needs no audio input");
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test live_input audio_input_is_wanted 2>&1 | tail -5
```
Expected: does not compile: `host::wants_audio_input` does not exist.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/platform-macos/src/permission.rs <<'VIZDAW_EOF'
//! Microphone permission (sub-project 5 spec §6.1): whether macOS lets
//! VizDAW open audio input, read with `+[AVCaptureDevice
//! authorizationStatusForMediaType:]`. Reading the status never shows the
//! permission prompt; only opening input for the first time does, and the
//! app's `Info.plist` carries `NSMicrophoneUsageDescription` for it.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    /// `AVMediaTypeAudio`, an `NSString`.
    static AVMediaTypeAudio: &'static AnyObject;
}

/// What macOS says about audio input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAccess {
    /// The user allowed it.
    Granted,
    /// The user (or a policy) refused it: input reads silence.
    Denied,
    /// Not asked yet: the first input open asks.
    Undetermined,
}

/// `AVAuthorizationStatus` as a [`MicAccess`]: 3 authorized, 1 restricted
/// and 2 denied, anything else not determined.
pub fn access_of(status: isize) -> MicAccess {
    match status {
        3 => MicAccess::Granted,
        1 | 2 => MicAccess::Denied,
        _ => MicAccess::Undetermined,
    }
}

/// The microphone permission now. Never asks.
pub fn microphone() -> MicAccess {
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
        return MicAccess::Undetermined;
    };
    // SAFETY: `authorizationStatusForMediaType:` is a class method taking
    // an `AVMediaType` (an NSString, here the framework's own constant)
    // and returning an `NSInteger`; it has no side effects.
    let status: isize =
        unsafe { msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio] };
    access_of(status)
}

/// The message arming an audio lane shows when input is refused (spec
/// §6.1).
pub const DENIED_MESSAGE: &str = "VizDAW may not use audio input. Allow it in System Settings › Privacy & Security › Microphone, then arm the lane again.";

#[cfg(test)]
mod tests {
    use super::*;

    /// The status mapping only: tests never read the real permission (it
    /// is per-app TCC state).
    #[test]
    fn statuses_map_to_access() {
        assert_eq!(access_of(0), MicAccess::Undetermined);
        assert_eq!(access_of(1), MicAccess::Denied);
        assert_eq!(access_of(2), MicAccess::Denied);
        assert_eq!(access_of(3), MicAccess::Granted);
        assert_eq!(access_of(-7), MicAccess::Undetermined);
    }
}
VIZDAW_EOF
```

In `crates/platform-macos/src/hal.rs`:

Find:
```rust
//! CoreAudio HAL output through an IOProc on the device's own real-time
//! thread (no AUHAL).

use core_engine::BackendError;
use core_engine::backend::{
    AudioBackend, AudioConfig, BackendEvent, BlockTime, MAX_FRAMES, StreamInfo,
};
use core_engine::pool::RtWorkgroup;
use core_engine::render::{Faults, Renderer};
use objc2_core_audio::{
    AudioConvertHostTimeToNanos, AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID,
    AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop, AudioGetCurrentHostTime,
    AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertyScope,
    AudioObjectPropertySelector, AudioObjectRemovePropertyListener, AudioObjectSetPropertyData,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyIOThreadOSWorkgroup, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertyStreamConfiguration, kAudioDevicePropertyStreams,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject, kAudioStreamPropertyVirtualFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioFormatFlagIsFloat, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{CFRetained, CFString};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};
```
Replace with:
```rust
//! CoreAudio HAL output and input through an IOProc on the device's own
//! real-time thread (no AUHAL). Input (sub-project 5 spec §6.1) comes from
//! the output device's own IOProc (one duplex stream) or, from another
//! device, through a private aggregate device with the output device as
//! clock and drift correction on the input; it is opened only when asked.

use core_engine::BackendError;
use core_engine::backend::{
    AudioBackend, AudioConfig, BackendEvent, BlockTime, InputBlock, MAX_FRAMES, MAX_INPUT_CHANNELS,
    StreamInfo,
};
use core_engine::pool::RtWorkgroup;
use core_engine::render::{Faults, Renderer};
use objc2_core_audio::{
    AudioConvertHostTimeToNanos, AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID,
    AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop, AudioGetCurrentHostTime,
    AudioHardwareCreateAggregateDevice, AudioHardwareDestroyAggregateDevice,
    AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertyElement,
    AudioObjectPropertyScope, AudioObjectPropertySelector, AudioObjectRemovePropertyListener,
    AudioObjectSetPropertyData, kAudioAggregateDeviceIsPrivateKey,
    kAudioAggregateDeviceMainSubDeviceKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceUIDKey,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyDeviceUID, kAudioDevicePropertyIOThreadOSWorkgroup,
    kAudioDevicePropertyLatency, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertySafetyOffset, kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreams, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyElementName, kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    kAudioStreamPropertyLatency, kAudioStreamPropertyVirtualFormat,
    kAudioSubDeviceDriftCompensationKey, kAudioSubDeviceUIDKey,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioFormatFlagIsFloat, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};
```

Find:
```rust
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}
```
Replace with:
```rust
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> AudioObjectPropertyAddress {
    addr_at(selector, scope, kAudioObjectPropertyElementMain)
}

fn addr_at(
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    element: AudioObjectPropertyElement,
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: element,
    }
}
```

Find:
```rust
}

fn name_of(obj: AudioObjectID) -> Option<String> {
    let mut a = addr(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal);
    let mut s: *const CFString = ptr::null();
    let mut size = size_of::<*const CFString>() as u32;
    // SAFETY: the name property is a +1 retained CFStringRef.
```
Replace with:
```rust
}

fn name_of(obj: AudioObjectID) -> Option<String> {
    string_prop(
        obj,
        addr(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal),
    )
}

/// A device's persistent UID (what an aggregate device names it by).
fn uid_of(obj: AudioObjectID) -> Option<String> {
    string_prop(
        obj,
        addr(
            kAudioDevicePropertyDeviceUID,
            kAudioObjectPropertyScopeGlobal,
        ),
    )
}

/// A CFString property.
fn string_prop(obj: AudioObjectID, mut a: AudioObjectPropertyAddress) -> Option<String> {
    let mut s: *const CFString = ptr::null();
    let mut size = size_of::<*const CFString>() as u32;
    // SAFETY: the name property is a +1 retained CFStringRef.
```

Find:
```rust

/// Output channel counts of the device's output buffers, in order.
fn output_layout(dev: AudioObjectID) -> Result<Vec<u32>, BackendError> {
    let words = get_bytes(
        dev,
        kAudioDevicePropertyStreamConfiguration,
        kAudioObjectPropertyScopeOutput,
    )?;
    let list = words.as_ptr().cast::<AudioBufferList>();
    // SAFETY: the property is an AudioBufferList with `mNumberBuffers`
    // trailing buffers, stored in `words` (8-byte aligned).
```
Replace with:
```rust

/// Output channel counts of the device's output buffers, in order.
fn output_layout(dev: AudioObjectID) -> Result<Vec<u32>, BackendError> {
    layout(dev, kAudioObjectPropertyScopeOutput)
}

/// Channel counts of the device's buffers in `scope`, in order.
fn layout(dev: AudioObjectID, scope: AudioObjectPropertyScope) -> Result<Vec<u32>, BackendError> {
    let words = get_bytes(dev, kAudioDevicePropertyStreamConfiguration, scope)?;
    let list = words.as_ptr().cast::<AudioBufferList>();
    // SAFETY: the property is an AudioBufferList with `mNumberBuffers`
    // trailing buffers, stored in `words` (8-byte aligned).
```

Find:
```rust

/// Every output stream must be 32-bit float linear PCM.
fn check_formats(dev: AudioObjectID) -> Result<(), BackendError> {
    for stream in get_ids(
        dev,
        kAudioDevicePropertyStreams,
        kAudioObjectPropertyScopeOutput,
    )? {
        let f: AudioStreamBasicDescription = get_asbd(stream)?;
        if f.mFormatID != kAudioFormatLinearPCM
            || f.mFormatFlags & kAudioFormatFlagIsFloat == 0
```
Replace with:
```rust

/// Every output stream must be 32-bit float linear PCM.
fn check_formats(dev: AudioObjectID) -> Result<(), BackendError> {
    check_scope_formats(dev, kAudioObjectPropertyScopeOutput)
}

/// Every stream of `scope` must be 32-bit float linear PCM.
fn check_scope_formats(
    dev: AudioObjectID,
    scope: AudioObjectPropertyScope,
) -> Result<(), BackendError> {
    for stream in get_ids(dev, kAudioDevicePropertyStreams, scope)? {
        let f: AudioStreamBasicDescription = get_asbd(stream)?;
        if f.mFormatID != kAudioFormatLinearPCM
            || f.mFormatFlags & kAudioFormatFlagIsFloat == 0
```

Find:
```rust
    Ok(f)
}

/// Names of every device with output channels.
pub fn output_devices() -> Vec<String> {
    get_ids(
```
Replace with:
```rust
    Ok(f)
}

/// The latency of `dev` in `scope`, in frames: device latency + safety
/// offset + the first stream's latency (spec §6.1). Unknown parts count
/// 0.
fn latency(dev: AudioObjectID, scope: AudioObjectPropertyScope) -> u32 {
    let device: u32 = get(dev, kAudioDevicePropertyLatency, scope).unwrap_or(0);
    let safety: u32 = get(dev, kAudioDevicePropertySafetyOffset, scope).unwrap_or(0);
    let stream: u32 = get_ids(dev, kAudioDevicePropertyStreams, scope)
        .ok()
        .and_then(|v| v.first().copied())
        .and_then(|st| {
            get(
                st,
                kAudioStreamPropertyLatency,
                kAudioObjectPropertyScopeGlobal,
            )
            .ok()
        })
        .unwrap_or(0);
    device.saturating_add(safety).saturating_add(stream)
}

/// The names of `dev`'s first `n` input channels (the device's element
/// names; "Input k" where it has none).
fn input_names(dev: AudioObjectID, n: u32) -> Vec<String> {
    (1..=n)
        .map(|k| {
            string_prop(
                dev,
                addr_at(
                    kAudioObjectPropertyElementName,
                    kAudioObjectPropertyScopeInput,
                    k,
                ),
            )
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Input {k}"))
        })
        .collect()
}

/// A device by name.
fn find_device(name: &str) -> Result<AudioObjectID, BackendError> {
    get_ids(
        SYSTEM,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )?
    .into_iter()
    .find(|d| name_of(*d).as_deref() == Some(name))
    .ok_or(BackendError::NoDevice)
}

/// The description of a private aggregate device (spec §6.1) named
/// `uid`: `output` first and as its clock, `input` drift-corrected.
pub fn aggregate_description(
    uid: &str,
    output: &str,
    input: &str,
) -> CFRetained<CFDictionary<CFString, CFType>> {
    let key = |k: &std::ffi::CStr| CFString::from_str(&k.to_string_lossy());
    let sub = |device: &str, drift: bool| {
        let keys = [
            key(kAudioSubDeviceUIDKey),
            key(kAudioSubDeviceDriftCompensationKey),
        ];
        let uid = CFString::from_str(device);
        let drift = CFNumber::new_i32(i32::from(drift));
        let values: [&CFType; 2] = [uid.as_ref(), drift.as_ref()];
        let keys: [&CFString; 2] = [&keys[0], &keys[1]];
        CFDictionary::<CFString, CFType>::from_slices(&keys, &values)
    };
    let (out, inp) = (sub(output, false), sub(input, true));
    let list = CFArray::<CFType>::from_objects(&[(*out).as_ref(), (*inp).as_ref()]);
    let keys = [
        key(kAudioAggregateDeviceUIDKey),
        key(kAudioAggregateDeviceNameKey),
        key(kAudioAggregateDeviceIsPrivateKey),
        key(kAudioAggregateDeviceMainSubDeviceKey),
        key(kAudioAggregateDeviceSubDeviceListKey),
    ];
    let (u, name, private, main) = (
        CFString::from_str(uid),
        CFString::from_str("VizDAW Input"),
        CFNumber::new_i32(1),
        CFString::from_str(output),
    );
    let values: [&CFType; 5] = [
        u.as_ref(),
        name.as_ref(),
        private.as_ref(),
        main.as_ref(),
        (*list).as_ref(),
    ];
    let keys: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
    CFDictionary::<CFString, CFType>::from_slices(&keys, &values)
}

/// Creates the private aggregate device of `output` and `input`.
fn create_aggregate(
    output: AudioObjectID,
    input: AudioObjectID,
) -> Result<AudioObjectID, BackendError> {
    let (Some(out_uid), Some(in_uid)) = (uid_of(output), uid_of(input)) else {
        return Err(BackendError::NoDevice);
    };
    let uid = format!("org.vizdaw.input.{}", std::process::id());
    let desc = aggregate_description(&uid, &out_uid, &in_uid);
    let mut id: AudioObjectID = 0;
    // SAFETY: `desc` is a valid dictionary of CFString keys for the call
    // and `id` a valid out-pointer.
    os("AudioHardwareCreateAggregateDevice", unsafe {
        AudioHardwareCreateAggregateDevice(desc.as_opaque(), NonNull::from(&mut id))
    })?;
    if id == 0 {
        return Err(BackendError::NoDevice);
    }
    Ok(id)
}

/// When a block's first frame is heard: the output time CoreAudio gives
/// plus the output latency (spec §6.3), so MIDI scheduled against it
/// leaves at audible time.
pub fn audible_ns(out_ns: u64, output_latency: u32, rate: f64) -> u64 {
    let lat = (f64::from(output_latency) * 1e9 / rate.max(1.0)).round() as u64;
    out_ns.saturating_add(lat)
}

/// How much earlier than the output buffer's first frame is heard the
/// input buffer's first frame was captured (spec §6.3): their sample-time
/// difference plus both latencies.
pub fn input_delay(
    out_sample: f64,
    in_sample: f64,
    output_latency: u32,
    input_latency: u32,
) -> i64 {
    let gap = if out_sample.is_finite() && in_sample.is_finite() {
        (out_sample - in_sample).round() as i64
    } else {
        0
    };
    gap.saturating_add(i64::from(output_latency))
        .saturating_add(i64::from(input_latency))
}

/// Copies input channels `offset..offset + inputs.len()` (counted across
/// every buffer of `list`) into the planar `inputs`, `frames` frames; the
/// rest of each is left alone. // RT
///
/// # Safety
///
/// As for [`buffers`].
unsafe fn deinterleave(
    list: NonNull<AudioBufferList>,
    offset: usize,
    inputs: &mut [Box<[f32]>],
    frames: usize,
) {
    // SAFETY: forwarded from the caller.
    let bufs = unsafe { buffers(list) };
    let mut global = 0usize;
    for b in bufs.iter() {
        let nch = b.mNumberChannels.max(1) as usize;
        if b.mData.is_null() {
            global += nch;
            continue;
        }
        let samples = b.mDataByteSize as usize / 4;
        // SAFETY: `mData` holds `mDataByteSize` bytes of f32 samples.
        let data = unsafe { std::slice::from_raw_parts(b.mData.cast::<f32>(), samples) };
        for c in 0..nch {
            let Some(dst) = (global + c)
                .checked_sub(offset)
                .and_then(|k| inputs.get_mut(k))
            else {
                continue;
            };
            for (f, d) in dst.iter_mut().take(frames).enumerate() {
                *d = data.get(f * nch + c).copied().unwrap_or(0.0);
            }
        }
        global += nch;
    }
}

/// Names of every device with output channels.
pub fn output_devices() -> Vec<String> {
    get_ids(
```

Find:
```rust
    .unwrap_or_default()
    .into_iter()
    .filter(|d| output_layout(*d).is_ok_and(|l| l.iter().sum::<u32>() > 0))
    .filter_map(name_of)
    .collect()
}
```
Replace with:
```rust
    .unwrap_or_default()
    .into_iter()
    .filter(|d| output_layout(*d).is_ok_and(|l| l.iter().sum::<u32>() > 0))
    .filter_map(name_of)
    .collect()
}

/// Names of every device with input channels.
pub fn input_devices() -> Vec<String> {
    get_ids(
        SYSTEM,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )
    .unwrap_or_default()
    .into_iter()
    .filter(|d| layout(*d, kAudioObjectPropertyScopeInput).is_ok_and(|l| l.iter().sum::<u32>() > 0))
    .filter_map(name_of)
    .collect()
}
```

Find:
```rust
    left: Box<[f32]>,
    right: Box<[f32]>,
    poisoned: bool,
}

/// The `mNumberBuffers` buffers of `list`. // RT
```
Replace with:
```rust
    left: Box<[f32]>,
    right: Box<[f32]>,
    poisoned: bool,
    /// Planar input, one buffer per input channel (none: input closed).
    inputs: Box<[Box<[f32]>]>,
    /// The first device input channel this stream's input carries (an
    /// aggregate puts the output device's inputs first).
    input_offset: usize,
    output_latency: u32,
    input_latency: u32,
}

/// The `mNumberBuffers` buffers of `list`. // RT
```

Find:
```rust
/// # Safety
///
/// As for [`buffers`].
unsafe fn render(ctx: &mut RtContext, out: NonNull<AudioBufferList>, time: &AudioTimeStamp) {
    // SAFETY: forwarded from the caller (CoreAudio hands the IOProc
    // `mNumberBuffers` trailing buffers, valid for the call).
    let bufs = unsafe { buffers(out) };
```
Replace with:
```rust
/// # Safety
///
/// As for [`buffers`].
unsafe fn render(
    ctx: &mut RtContext,
    out: NonNull<AudioBufferList>,
    time: &AudioTimeStamp,
    input: NonNull<AudioBufferList>,
    input_time: &AudioTimeStamp,
) {
    // SAFETY: forwarded from the caller (CoreAudio hands the IOProc
    // `mNumberBuffers` trailing buffers, valid for the call).
    let bufs = unsafe { buffers(out) };
```

Find:
```rust
        return;
    };
    let mut chans: [&mut [f32]; 2] = [l, r];
    ctx.renderer.process(
        &mut chans,
        frames,
        BlockTime {
            host_time_ns: host_ns,
            sample_rate: sr,
        },
    );
    let mut global = 0usize;
```
Replace with:
```rust
        return;
    };
    let mut chans: [&mut [f32]; 2] = [l, r];
    let valid = AudioTimeStampFlags::SampleTimeValid;
    let delay = if time.mFlags.contains(valid) && input_time.mFlags.contains(valid) {
        input_delay(
            time.mSampleTime,
            input_time.mSampleTime,
            ctx.output_latency,
            ctx.input_latency,
        )
    } else {
        i64::from(ctx.output_latency) + i64::from(ctx.input_latency)
    };
    if !ctx.inputs.is_empty() {
        // SAFETY: CoreAudio's input list is valid for this call.
        unsafe { deinterleave(input, ctx.input_offset, &mut ctx.inputs, len) };
    }
    let mut ins: [&[f32]; MAX_INPUT_CHANNELS] = [&[]; MAX_INPUT_CHANNELS];
    for (slot, buf) in ins.iter_mut().zip(ctx.inputs.iter()) {
        *slot = buf.get(..len).unwrap_or(&[]);
    }
    let n_in = ctx.inputs.len().min(MAX_INPUT_CHANNELS);
    ctx.renderer.process_input(
        &mut chans,
        frames,
        BlockTime {
            // MIDI leaves at the time this block is heard (spec §6.3).
            host_time_ns: audible_ns(host_ns, ctx.output_latency, sr),
            sample_rate: sr,
        },
        InputBlock {
            channels: ins.get(..n_in).unwrap_or(&[]),
            delay_frames: delay,
        },
    );
    let mut global = 0usize;
```

Find:
```rust
unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    _input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    output: NonNull<AudioBufferList>,
    output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    // SAFETY: `client` is the `RtContext` boxed in `start`, owned by this
    // IOProc until `stop` has returned from `AudioDeviceStop`; the buffer
    // list and timestamp are valid for this call. The buffer list stays a
    // raw pointer (see `buffers`). // RT
    let (ctx, time) = unsafe { (&mut *client.cast::<RtContext>(), output_time.as_ref()) };
    if ctx.poisoned {
        // SAFETY: CoreAudio's output list is valid for this call.
        unsafe { silence(output) };
        return 0;
    }
    // SAFETY: as above; `render` is the only user of the list meanwhile.
    let ok = catch_unwind(AssertUnwindSafe(|| unsafe { render(ctx, output, time) })).is_ok();
    if !ok {
        // Never unwind into CoreAudio: go silent for good and report.
        ctx.poisoned = true;
```
Replace with:
```rust
unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    input_time: NonNull<AudioTimeStamp>,
    output: NonNull<AudioBufferList>,
    output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    // SAFETY: `client` is the `RtContext` boxed in `start`, owned by this
    // IOProc until `stop` has returned from `AudioDeviceStop`; the buffer
    // lists and timestamps are valid for this call. The buffer lists stay
    // raw pointers (see `buffers`). // RT
    let (ctx, time, in_time) = unsafe {
        (
            &mut *client.cast::<RtContext>(),
            output_time.as_ref(),
            input_time.as_ref(),
        )
    };
    if ctx.poisoned {
        // SAFETY: CoreAudio's output list is valid for this call.
        unsafe { silence(output) };
        return 0;
    }
    // SAFETY: as above; `render` is the only user of the lists meanwhile.
    let ok = catch_unwind(AssertUnwindSafe(|| unsafe {
        render(ctx, output, time, input, in_time)
    }))
    .is_ok();
    if !ok {
        // Never unwind into CoreAudio: go silent for good and report.
        ctx.poisoned = true;
```

Find:
```rust
    info: Option<StreamInfo>,
    running: Option<Running>,
    listening: Option<Listening>,
    /// Flags of removed listeners. CoreAudio may still be running a
    /// listener call on its notification thread while
    /// `AudioObjectRemovePropertyListener` returns, so the flags live as
```
Replace with:
```rust
    info: Option<StreamInfo>,
    running: Option<Running>,
    listening: Option<Listening>,
    /// The private aggregate device made for a separate input device
    /// (destroyed on close).
    aggregate: Option<AudioObjectID>,
    /// The input device inside it, also listened to (unplugging it ends a
    /// recording).
    input_device: Option<AudioObjectID>,
    /// The first device input channel the stream carries.
    input_offset: usize,
    /// The output device (the aggregate's clock when there is one): what
    /// following the system default compares.
    output: Option<AudioObjectID>,
    /// Flags of removed listeners. CoreAudio may still be running a
    /// listener call on its notification thread while
    /// `AudioObjectRemovePropertyListener` returns, so the flags live as
```

Find:
```rust
            info: None,
            running: None,
            listening: None,
            retired: Vec::new(),
        }
    }
```
Replace with:
```rust
            info: None,
            running: None,
            listening: None,
            aggregate: None,
            input_device: None,
            input_offset: 0,
            output: None,
            retired: Vec::new(),
        }
    }
```

Find:
```rust

    fn listen(&mut self, device: AudioObjectID, follow_default: bool) {
        let flags = Arc::into_raw(Arc::new(Flags::default()));
        for (selector, system) in LISTENED {
            if system && !follow_default {
                continue;
```
Replace with:
```rust

    fn listen(&mut self, device: AudioObjectID, follow_default: bool) {
        let flags = Arc::into_raw(Arc::new(Flags::default()));
        if let Some(input) = self.input_device {
            let mut a = addr(
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            );
            // SAFETY: as below; removed in `unlisten`.
            unsafe {
                AudioObjectAddPropertyListener(
                    input,
                    NonNull::from(&mut a),
                    Some(listener),
                    flags.cast_mut().cast::<c_void>(),
                );
            }
        }
        for (selector, system) in LISTENED {
            if system && !follow_default {
                continue;
```

Find:
```rust
        let Some(l) = self.listening.take() else {
            return;
        };
        for (selector, system) in LISTENED {
            if system && !l.follow_default {
                continue;
```
Replace with:
```rust
        let Some(l) = self.listening.take() else {
            return;
        };
        if let Some(input) = self.input_device {
            let mut a = addr(
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            );
            // SAFETY: removes exactly the listener added in `listen`.
            unsafe {
                AudioObjectRemovePropertyListener(
                    input,
                    NonNull::from(&mut a),
                    Some(listener),
                    l.flags.cast_mut().cast::<c_void>(),
                );
            }
        }
        for (selector, system) in LISTENED {
            if system && !l.follow_default {
                continue;
```

Find:
```rust
        self.retired.push(unsafe { Arc::from_raw(l.flags) });
    }

    /// Stops (if running) and forgets the device.
    pub fn close(&mut self) -> Option<Renderer> {
        let r = self.stop().ok();
        self.unlisten();
        self.device = None;
        self.info = None;
        r
```
Replace with:
```rust
        self.retired.push(unsafe { Arc::from_raw(l.flags) });
    }

    /// Stops (if running) and forgets the device; an aggregate device made
    /// for input is destroyed.
    pub fn close(&mut self) -> Option<Renderer> {
        let r = self.stop().ok();
        self.unlisten();
        if let Some(agg) = self.aggregate.take() {
            // SAFETY: the aggregate was made by this backend and nothing
            // runs on it any more.
            unsafe {
                AudioHardwareDestroyAggregateDevice(agg);
            }
        }
        self.input_device = None;
        self.input_offset = 0;
        self.output = None;
        self.device = None;
        self.info = None;
        r
```

Find:
```rust
        if device == 0 {
            return Err(BackendError::NoDevice);
        }
        let layout = output_layout(device)?;
        let channels: u32 = layout.iter().sum();
        if channels == 0 {
            return Err(BackendError::NoDevice);
        }
```
Replace with:
```rust
        if device == 0 {
            return Err(BackendError::NoDevice);
        }
        let outputs = output_layout(device)?;
        let channels: u32 = outputs.iter().sum();
        if channels == 0 {
            return Err(BackendError::NoDevice);
        }
```

Find:
```rust
                })?;
            }
        }
        let sample_rate: f64 = get(
            device,
            kAudioDevicePropertyNominalSampleRate,
```
Replace with:
```rust
                })?;
            }
        }
        // Input (spec §6.1), once the output runs at its rate: the output
        // device's own, or another device's through a private aggregate
        // (the output device first, as its clock), whose inputs list the
        // output device's before the input device's.
        let output = device;
        let mut device = device;
        let mut input_source = None;
        if cfg.input_enabled {
            match &cfg.input_device {
                Some(name) if name_of(output).as_deref() != Some(name.as_str()) => {
                    let input = find_device(name)?;
                    let agg = create_aggregate(output, input)?;
                    self.input_offset = layout(output, kAudioObjectPropertyScopeInput)
                        .map(|l| l.iter().sum::<u32>() as usize)
                        .unwrap_or(0);
                    self.aggregate = Some(agg);
                    self.input_device = Some(input);
                    input_source = Some(input);
                    device = agg;
                }
                _ => input_source = Some(output),
            }
        }
        let sample_rate: f64 = get(
            device,
            kAudioDevicePropertyNominalSampleRate,
```

Find:
```rust
            kAudioDevicePropertyBufferFrameSize,
            kAudioObjectPropertyScopeGlobal,
        )?;
        let info = StreamInfo {
            device_name: name_of(device).unwrap_or_default(),
            sample_rate,
            buffer_frames,
            channels,
            // Input and the latencies come with plan 5c Task 10.
            input_channels: 0,
            input_channel_names: Vec::new(),
            input_latency_frames: 0,
            output_latency_frames: 0,
        };
        self.device = Some(device);
        self.info = Some(info.clone());
        self.listen(device, cfg.device.is_none());
        Ok(info)
```
Replace with:
```rust
            kAudioDevicePropertyBufferFrameSize,
            kAudioObjectPropertyScopeGlobal,
        )?;
        let input_channels = match input_source {
            Some(src) => {
                let n = layout(src, kAudioObjectPropertyScopeInput)
                    .map(|l| l.iter().sum::<u32>())
                    .unwrap_or(0)
                    .min(MAX_INPUT_CHANNELS as u32);
                if n > 0 {
                    check_scope_formats(device, kAudioObjectPropertyScopeInput)?;
                }
                n
            }
            None => 0,
        };
        let info = StreamInfo {
            device_name: name_of(output).unwrap_or_default(),
            sample_rate,
            buffer_frames,
            channels,
            input_channels,
            input_channel_names: input_source
                .map(|src| input_names(src, input_channels))
                .unwrap_or_default(),
            input_latency_frames: if input_channels > 0 {
                latency(device, kAudioObjectPropertyScopeInput)
            } else {
                0
            },
            output_latency_frames: latency(device, kAudioObjectPropertyScopeOutput),
        };
        self.device = Some(device);
        self.output = Some(output);
        self.info = Some(info.clone());
        self.listen(device, cfg.device.is_none());
        Ok(info)
```

Find:
```rust
            });
        }
        let max = renderer.config().max_frames.max(MAX_FRAMES) as usize;
        let ctx = Box::into_raw(Box::new(RtContext {
            renderer,
            left: vec![0.0; max].into_boxed_slice(),
            right: vec![0.0; max].into_boxed_slice(),
            poisoned: false,
        }));
        let mut proc_id: AudioDeviceIOProcID = None;
        // SAFETY: `io_proc` matches the IOProc signature; `ctx` stays valid
```
Replace with:
```rust
            });
        }
        let max = renderer.config().max_frames.max(MAX_FRAMES) as usize;
        let info = self.info.clone();
        let inputs = info.as_ref().map_or(0, |i| i.input_channels as usize);
        let ctx = Box::into_raw(Box::new(RtContext {
            renderer,
            left: vec![0.0; max].into_boxed_slice(),
            right: vec![0.0; max].into_boxed_slice(),
            poisoned: false,
            inputs: (0..inputs)
                .map(|_| vec![0.0; max].into_boxed_slice())
                .collect(),
            input_offset: self.input_offset,
            output_latency: info.as_ref().map_or(0, |i| i.output_latency_frames),
            input_latency: info.as_ref().map_or(0, |i| i.input_latency_frames),
        }));
        let mut proc_id: AudioDeviceIOProcID = None;
        // SAFETY: `io_proc` matches the IOProc signature; `ctx` stays valid
```

Find:
```rust
                kAudioObjectPropertyScopeGlobal,
            )
            .unwrap_or(0);
            if now != device {
                ev.push(BackendEvent::DefaultDeviceChanged);
            }
        }
```
Replace with:
```rust
                kAudioObjectPropertyScopeGlobal,
            )
            .unwrap_or(0);
            if now != self.output.unwrap_or(device) {
                ev.push(BackendEvent::DefaultDeviceChanged);
            }
        }
```

Find:
```rust
        assert_eq!(b, [0.0; 8]);
    }

    /// Task 11 Minor 1: a device that refused the rate is not polled (no
    /// 1 s wait); one that accepted it is polled until it reports it.
    #[test]
```
Replace with:
```rust
        assert_eq!(b, [0.0; 8]);
    }

    /// Plan 5c Task 11 (spec §6.3): MIDI leaves at the time a block is
    /// heard, the output latency after CoreAudio's output time.
    #[test]
    fn a_block_is_heard_one_output_latency_later() {
        assert_eq!(audible_ns(1_000_000, 0, 48_000.0), 1_000_000);
        assert_eq!(audible_ns(1_000_000, 480, 48_000.0), 11_000_000);
        assert_eq!(audible_ns(u64::MAX - 1, 480, 48_000.0), u64::MAX);
    }

    /// Spec §6.3: the input buffer's first frame was captured the
    /// sample-time difference plus both latencies before the output
    /// buffer's first frame is heard.
    #[test]
    fn the_input_delay_adds_both_latencies_to_the_buffer_gap() {
        assert_eq!(input_delay(1000.0, 744.0, 100, 50), 256 + 150);
        assert_eq!(input_delay(500.0, 500.0, 0, 0), 0);
        assert_eq!(input_delay(f64::NAN, 1.0, 10, 20), 30);
    }

    /// The input channels after `offset`, across every buffer, land in
    /// the planar buffers.
    #[test]
    fn input_buffers_are_split_into_channels_from_the_offset() {
        // Buffer 0: two interleaved channels; buffer 1: one channel.
        let mut a = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut b = [10.0f32, 20.0, 30.0];
        let list = TwoBuffers {
            n: 2,
            bufs: [
                AudioBuffer {
                    mNumberChannels: 2,
                    mDataByteSize: 24,
                    mData: a.as_mut_ptr().cast(),
                },
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: 12,
                    mData: b.as_mut_ptr().cast(),
                },
            ],
        };
        let mut list = list;
        let p = NonNull::from(&mut list).cast::<AudioBufferList>();
        let mut inputs: Box<[Box<[f32]>]> =
            (0..2).map(|_| vec![0.0; 3].into_boxed_slice()).collect();
        // The aggregate's first input (buffer 0, channel 0) belongs to the
        // output device: offset 1.
        // SAFETY: `list` is a valid two-buffer list for the call.
        unsafe { deinterleave(p, 1, &mut inputs, 3) };
        assert_eq!(&*inputs[0], &[2.0, 4.0, 6.0]);
        assert_eq!(&*inputs[1], &[10.0, 20.0, 30.0]);
    }

    /// The aggregate's description names both devices, is private and
    /// clocked by the output.
    #[test]
    fn the_aggregate_is_private_and_clocked_by_the_output() {
        let d = aggregate_description("org.vizdaw.input.1", "OUT-UID", "IN-UID");
        assert_eq!(d.len(), 5);
        let get = |k: &str| d.get(&CFString::from_str(k)).unwrap();
        let private = get("private");
        assert_eq!(
            private.downcast_ref::<CFNumber>().unwrap().as_i64(),
            Some(1)
        );
        let main = get("master");
        assert_eq!(
            main.downcast_ref::<CFString>().unwrap().to_string(),
            "OUT-UID"
        );
        let subs = get("subdevices");
        assert_eq!(subs.downcast_ref::<CFArray>().unwrap().len(), 2);
    }

    /// Task 11 Minor 1: a device that refused the rate is not polled (no
    /// 1 s wait); one that accepted it is polled until it reports it.
    #[test]
```

In `crates/platform-macos/src/lib.rs`:

Find:
```rust
//! and the Audio Unit plugin host (`au`).
//!
//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs`, `rt.rs`,
//! `media.rs`, `trash.rs` and `au/ffi.rs`, with a `// SAFETY:` comment per
//! block.
#![cfg(target_os = "macos")]
#![deny(missing_docs)]
#![cfg_attr(
```
Replace with:
```rust
//! and the Audio Unit plugin host (`au`).
//!
//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs`, `rt.rs`,
//! `media.rs`, `trash.rs`, `permission.rs` and `au/ffi.rs`, with a
//! `// SAFETY:` comment per block.
#![cfg(target_os = "macos")]
#![deny(missing_docs)]
#![cfg_attr(
```

Find:
```rust
pub mod hal;
pub mod media;
pub mod midi;
pub mod rt;
pub mod sinks;
pub mod trash;
```
Replace with:
```rust
pub mod hal;
pub mod media;
pub mod midi;
pub mod permission;
pub mod rt;
pub mod sinks;
pub mod trash;
```

In `crates/platform-macos/Cargo.toml`:

Find:
```toml
objc2-audio-toolbox = { version = "0.3.2", default-features = false, features = ["std", "AudioComponent", "AUComponent", "AudioUnitProperties", "AudioUnitUtilities", "MusicDevice", "ExtendedAudioFile", "AudioFile", "AudioConverter", "objc2-core-audio-types", "objc2-core-foundation", "block2"] }
core-media = { path = "../core-media" }
objc2-core-midi = { version = "0.3.2", default-features = false, features = ["std", "MIDIServices", "MIDIMessages", "objc2", "objc2-core-foundation", "block2"] }
objc2-core-foundation = { version = "0.3.2", default-features = false, features = ["std", "CFString", "CFData", "CFError", "CFPropertyList", "CFBundle", "CFURL", "CFRunLoop", "CFDate", "CFCGTypes", "objc2"] }

[target.'cfg(target_os = "macos")'.dev-dependencies]
core-engine = { path = "../core-engine", features = ["fixtures"] }
```
Replace with:
```toml
objc2-audio-toolbox = { version = "0.3.2", default-features = false, features = ["std", "AudioComponent", "AUComponent", "AudioUnitProperties", "AudioUnitUtilities", "MusicDevice", "ExtendedAudioFile", "AudioFile", "AudioConverter", "objc2-core-audio-types", "objc2-core-foundation", "block2"] }
core-media = { path = "../core-media" }
objc2-core-midi = { version = "0.3.2", default-features = false, features = ["std", "MIDIServices", "MIDIMessages", "objc2", "objc2-core-foundation", "block2"] }
objc2-core-foundation = { version = "0.3.2", default-features = false, features = ["std", "CFString", "CFData", "CFError", "CFPropertyList", "CFBundle", "CFURL", "CFRunLoop", "CFDate", "CFCGTypes", "CFArray", "CFDictionary", "CFNumber", "objc2"] }

[target.'cfg(target_os = "macos")'.dev-dependencies]
core-engine = { path = "../core-engine", features = ["fixtures"] }
```

In `crates/core-ffi/src/host.rs`:

Find:
```rust
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}

/// What a host's audio input offers (spec §6.1, §12.1 `state.get input`).
```
Replace with:
```rust
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}

/// The input device live hosts open (`None`: the output device's own
/// input; spec §6.1 Settings › Audio "Input device").
static INPUT_DEVICE: Mutex<Option<String>> = Mutex::new(None);

/// Chooses the audio input device by name (`None`: "Same as output").
/// Takes effect when the next host opens its input.
#[uniffi::export]
pub fn set_audio_input_device(name: Option<String>) {
    *lock(&INPUT_DEVICE) = name.filter(|n| !n.is_empty());
}

/// Whether the lanes of `seq` need audio input: an armed or
/// Input-monitored lane with an audio input (spec §6.1: input opens only
/// while one is).
pub fn wants_audio_input(doc: &Document, seq: SeqId) -> bool {
    doc.sequence(seq).is_ok_and(|s| {
        s.lanes.iter().any(|l| {
            (l.arm || l.monitor == MonitorMode::Input)
                && l.input.as_ref().is_some_and(LaneInput::is_audio)
        })
    })
}

/// Whether a live host may open audio input now. Never in test builds
/// (plan 5c Global Constraints: tests open no device and never ask for
/// the microphone); otherwise unless macOS refused it, which is reported.
#[cfg(feature = "test-hooks")]
fn live_input_allowed(_faults: &mut Vec<String>) -> bool {
    false
}

#[cfg(all(not(feature = "test-hooks"), target_os = "macos"))]
fn live_input_allowed(faults: &mut Vec<String>) -> bool {
    use platform_macos::permission::{DENIED_MESSAGE, MicAccess, microphone};
    if microphone() == MicAccess::Denied {
        if !faults.iter().any(|f| f == DENIED_MESSAGE) {
            faults.push(DENIED_MESSAGE.to_string());
        }
        return false;
    }
    true
}

/// What a host's audio input offers (spec §6.1, §12.1 `state.get input`).
```

Find:
```rust
    injected: Vec<BackendEvent>,
    /// What the output's input offers.
    inputs: InputInfo,
}

impl Host {
```
Replace with:
```rust
    injected: Vec<BackendEvent>,
    /// What the output's input offers.
    inputs: InputInfo,
    /// A live output has its input open.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    input_open: bool,
}

impl Host {
```

Find:
```rust

    #[cfg(target_os = "macos")]
    fn create_live(project: f64) -> Result<Host, VizError> {
        use core_engine::backend::AudioConfig;
        use platform_macos::hal::CoreAudioBackend;
        use platform_macos::rt::MacRtPlatform;
        use platform_macos::sinks::MacSinks;
        use std::sync::Arc;
        let mut backend = CoreAudioBackend::new();
        let info = backend.open(&AudioConfig {
            sample_rate: Some(project),
            ..AudioConfig::default()
        })?;
        let (engine, renderer) = Engine::with_platform(
            live_config(info.sample_rate),
            Box::new(MacSinks::new()),
```
Replace with:
```rust

    #[cfg(target_os = "macos")]
    fn create_live(project: f64) -> Result<Host, VizError> {
        use platform_macos::hal::CoreAudioBackend;
        use platform_macos::rt::MacRtPlatform;
        use platform_macos::sinks::MacSinks;
        use std::sync::Arc;
        let mut backend = CoreAudioBackend::new();
        // Output only: `ensure_input` opens input once a lane needs it.
        let info = backend.open(&live_audio_config(project, false))?;
        let (engine, renderer) = Engine::with_platform(
            live_config(info.sample_rate),
            Box::new(MacSinks::new()),
```

Find:
```rust
            log,
            injected: Vec::new(),
            inputs: InputInfo::default(),
        }
    }
```
Replace with:
```rust
            log,
            injected: Vec::new(),
            inputs: InputInfo::default(),
            input_open: false,
        }
    }

    /// Opens or closes a live output's input to match `want` (spec §6.1),
    /// restarting the device with the same renderer. Offline outputs keep
    /// their fake input.
    fn ensure_input(&mut self, want: bool) -> Result<(), VizError> {
        match &mut self.output {
            Output::Offline(_) => {
                let _ = want;
                Ok(())
            }
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                let want = want && live_input_allowed(&mut self.faults);
                if want == self.input_open {
                    return Ok(());
                }
                let mut r = b.stop()?;
                let info = match b.open(&live_audio_config(self.project_rate, want)) {
                    Ok(i) => i,
                    Err(e) if want => {
                        self.faults.push(format!("audio input could not open: {e}"));
                        b.open(&live_audio_config(self.project_rate, false))?
                    }
                    Err(e) => return Err(e.into()),
                };
                if info.buffer_frames > r.config().max_frames {
                    r.prepare(info.buffer_frames);
                }
                b.start(r)?;
                self.engine
                    .set_rt_env(rt_env(b, info.buffer_frames, info.sample_rate));
                self.input_open = info.input_channels > 0;
                self.inputs = InputInfo {
                    channels: info.input_channels,
                    names: info.input_channel_names,
                    input_latency_frames: info.input_latency_frames,
                    output_latency_frames: info.output_latency_frames,
                };
                Ok(())
            }
        }
    }
```

Find:
```rust
                Ok(())
            }
        }
    }
}
```
Replace with:
```rust
                Ok(())
            }
        }
    }
}

/// A live output's configuration: the project rate, input when `input`
/// (from the chosen input device).
#[cfg(target_os = "macos")]
fn live_audio_config(project: f64, input: bool) -> core_engine::backend::AudioConfig {
    core_engine::backend::AudioConfig {
        sample_rate: Some(project),
        input_enabled: input,
        input_device: lock(&INPUT_DEVICE).clone(),
        ..core_engine::backend::AudioConfig::default()
    }
}
```

Find:
```rust
    drop(old);
    let mut new = Host::create(project_rate(doc))?;
    new.attach(owner, plugins);
    // Automated values held while stopped survive the rebuild (Plan 4d
    // Ruling 13, final review Minor 1).
    if let Some(o) = overrides {
```
Replace with:
```rust
    drop(old);
    let mut new = Host::create(project_rate(doc))?;
    new.attach(owner, plugins);
    new.ensure_input(wants_audio_input(doc, seq))?;
    // Automated values held while stopped survive the rebuild (Plan 4d
    // Ruling 13, final review Minor 1).
    if let Some(o) = overrides {
```

Find:
```rust
        crate::recovery::before_sync();
        p.sync(doc);
    }
    if host.owner != owner || host.seq != seq || start.is_some() {
        host.engine.stop()?;
        if host.owner != owner {
```
Replace with:
```rust
        crate::recovery::before_sync();
        p.sync(doc);
    }
    host.faults.clear();
    host.ensure_input(wants_audio_input(doc, seq))?;
    if host.owner != owner || host.seq != seq || start.is_some() {
        host.engine.stop()?;
        if host.owner != owner {
```

Find:
```rust
        // Resumed with restarted instances: compile their placeholders.
        host.load(owner, doc, seq, looped)?;
    }
    host.faults.clear();
    // The intent is Play from here on: a rebuild during the wait resumes
    // playing, and a Stop during it cancels this Play.
    let pending = host.engine.begin_play(until);
```
Replace with:
```rust
        // Resumed with restarted instances: compile their placeholders.
        host.load(owner, doc, seq, looped)?;
    }
    // The intent is Play from here on: a rebuild during the wait resumes
    // playing, and a Stop during it cancels this Play.
    let pending = host.engine.begin_play(until);
```

Find:
```rust
            if !ready {
                return Ok(());
            }
            h.load(owner, doc, seq, looped)
        }
        _ => Ok(()),
```
Replace with:
```rust
            if !ready {
                return Ok(());
            }
            // An edit armed or disarmed a lane: input follows (spec §6.1).
            h.ensure_input(wants_audio_input(doc, seq))?;
            h.load(owner, doc, seq, looped)
        }
        _ => Ok(()),
```

In `app/Package.swift`:

Find:
```swift
                .linkedFramework("CoreMIDI"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Foundation"),
            ]
        ),
        .target(
```
Replace with:
```swift
                .linkedFramework("CoreMIDI"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Foundation"),
                // The microphone permission (plan 5c Task 11).
                .linkedFramework("AVFoundation"),
            ]
        ),
        .target(
```

In `scripts/make-app.sh`:

Find:
```bash
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
```
Replace with:
```bash
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>NSMicrophoneUsageDescription</key><string>VizDAW records audio from your audio interface or microphone into armed lanes.</string>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --lib 2>&1 | tail -3
cargo test -p core-ffi --test live_input 2>&1 | tail -3
```
Expected: the `hal` tests `a_block_is_heard_one_output_latency_later`, `the_input_delay_adds_both_latencies_to_the_buffer_gap`, `input_buffers_are_split_into_channels_from_the_offset`, `the_aggregate_is_private_and_clocked_by_the_output` and `permission::tests::statuses_map_to_access` pass; `live_input` 6 passed.

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo test -p platform-macos 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: no warnings, every `test result: ok`; `Cargo.lock` must not change (`git diff --stat Cargo.lock` prints nothing): the new features belong to an existing dependency..

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/platform-macos/src/permission.rs crates/platform-macos/src/hal.rs crates/platform-macos/src/lib.rs crates/platform-macos/Cargo.toml crates/core-ffi/src/host.rs crates/core-ffi/tests/live_input.rs app/Package.swift scripts/make-app.sh
git commit -F - <<'VIZDAW_MSG'
platform-macos: HAL input, the private aggregate and the microphone

The output device's input runs in the same IOProc; another input device
is joined in a private aggregate clocked by the output. Each block
reports its input delay from the buffers' time stamps and both
latencies, and its host time is when it is heard. The live host opens
input only for armed or monitored audio lanes and reports a refused
microphone as a transport fault; test builds never open input.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 12: `platform-macos` and `core-ffi` — CoreMIDI sources into the engine

**Files:**
- Modify: `crates/platform-macos/src/midi.rs` (`SourceListener`)
- Modify: `crates/core-ffi/src/host.rs` (`listen_to_midi_sources`, `debug_midi_from_source`)
- Modify: `crates/core-ffi/src/session.rs` (listen at Record with an armed MIDI lane)
- Test: `crates/platform-macos/tests/midi.rs`; extend `crates/core-ffi/tests/live_input.rs`

**Interfaces:**
- Consumes: Task 6's `MidiSender`, `port_key`.
- Produces: `midi::SourceListener::{connect_all(name, sink: SourceSink), connect_including_own(name, sink), sources() -> &[String]}`, `SourceSink = Box<dyn Fn(port_key, host_ns, &[u8]) + Send + Sync>`; `host::listen_to_midi_sources()` (connects nothing under `test-hooks`), `host::debug_midi_from_source(name, host_ns, bytes)` (test hook).

Ruling 20. The listener's test uses VizDAW's own virtual MIDI output as the source (`connect_including_own`), as 4c's MIDI tests do; it opens no audio device.

- [ ] **Step 1: Write the failing tests**

In `crates/platform-macos/tests/midi.rs`:

Find:
```rust
    std::thread::sleep(Duration::from_millis(300));
    assert!(!got.lock().unwrap().contains(&vec![0x93, 63, 100]));
}
```
Replace with:
```rust
    std::thread::sleep(Duration::from_millis(300));
    assert!(!got.lock().unwrap().contains(&vec![0x93, 63, 100]));
}

/// Plan 5c Task 12: the record listener tells sources apart by the key of
/// their name and keeps each message's host time (spec §6.3).
#[test]
fn a_source_listener_names_the_source_and_keeps_the_time() {
    use core_engine::input::port_key;
    use platform_macos::midi::SourceListener;
    use std::sync::{Arc, Mutex};
    let mut sys = MidiSystem::new("VizDAW Record Test").unwrap();
    let mut out = sys.open_virtual_source("VizDAW Record Test Out").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let got = Arc::new(Mutex::new(Vec::<(u32, u64, Vec<u8>)>::new()));
    let sink = Arc::clone(&got);
    let listener = SourceListener::connect_including_own(
        "VizDAW Record Test In",
        Box::new(move |key: u32, ns: u64, b: &[u8]| {
            sink.lock().unwrap().push((key, ns, b.to_vec()))
        }),
    )
    .unwrap();
    assert!(
        listener
            .sources()
            .iter()
            .any(|n| n == "VizDAW Record Test Out")
    );
    std::thread::sleep(Duration::from_millis(50));
    let at = now_ns() + 20_000_000;
    assert!(out.send(at, &[0x94, 62, 90]));
    let deadline = Instant::now() + Duration::from_secs(3);
    let mine =
        |g: &Vec<(u32, u64, Vec<u8>)>| g.iter().find(|(_, _, b)| b == &vec![0x94, 62, 90]).cloned();
    while mine(&got.lock().unwrap()).is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let (key, ns, _) = mine(&got.lock().unwrap()).expect("the message arrived");
    assert_eq!(key, port_key("VizDAW Record Test Out"));
    // Delivered with the time it was scheduled for (host-clock rounding).
    assert!(ns.abs_diff(at) < 1_000, "{ns} vs {at}");
}
```

In `crates/core-ffi/tests/live_input.rs`:

Find:
```rust
    );
    assert!(!wants_audio_input(&midi, song), "MIDI needs no audio input");
}
```
Replace with:
```rust
    );
    assert!(!wants_audio_input(&midi, song), "MIDI needs no audio input");
}

/// Plan 5c Task 12: what CoreMIDI sources send reaches the newest
/// engine, filtered by the lane's source.
#[test]
fn source_messages_reach_the_engine() {
    use vizdaw_ffi::host::debug_midi_from_source;
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = lane(&s, seq, "Chords").id;
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: chords,
            input: Some(LaneInputRow::Midi {
                port: Some("Keystation".into()),
                channel: Some(0),
            }),
        },
    )
    .unwrap();
    arm(&s, seq, chords);
    take_engine(&s, seq);
    std::thread::sleep(Duration::from_millis(50));
    drain_offline_log();
    // Another channel, another source: not this lane's; a SysEx: dropped.
    debug_midi_from_source("Keystation", 0, &[0x91, 72, 90]);
    debug_midi_from_source("Other", 0, &[0x90, 72, 90]);
    debug_midi_from_source("Keystation", 0, &[0xF0, 1, 0xF7]);
    debug_midi_from_source("Keystation", 0, &[0x90, 74, 90]);
    let until = Instant::now() + common::wait_limit();
    let mut log = Vec::new();
    while !log
        .iter()
        .any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x90, 74, 90])
    {
        assert!(Instant::now() < until, "never arrived: {log:?}");
        std::thread::sleep(Duration::from_millis(5));
        log.extend(drain_offline_log());
    }
    assert!(!log.iter().any(|(_, b)| b.get(1) == Some(&72)), "{log:?}");
    s.close().unwrap();
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test midi a_source_listener 2>&1 | tail -5
```
Expected: does not compile: `SourceListener` does not exist.

- [ ] **Step 3: Implement**

In `crates/platform-macos/src/midi.rs`:

Find:
```rust
    }
}

/// Nanoseconds to host-clock ticks (the timestamps CoreMIDI carries).
pub fn ns_to_host(ns: u64) -> u64 {
    // SAFETY: pure conversion.
```
Replace with:
```rust
    }
}

/// What a [`SourceListener`] hands each received MIDI 1.0 message to (on
/// CoreMIDI's thread): the source's key (`core_engine::input::port_key`
/// of its name), its host time in nanoseconds and its bytes. It must not
/// block.
pub type SourceSink = Box<dyn Fn(u32, u64, &[u8]) + Send + Sync>;

/// Listens to every MIDI source present when it connects, telling them
/// apart and keeping each message's timestamp (plan 5c Task 12: lane
/// inputs name a source, spec §6.3: MIDI lands where it was heard). Like
/// [`NoteListener`] it has its own client and skips this process's own
/// virtual sources; sources added later are picked up by connecting
/// again.
pub struct SourceListener {
    client: MIDIClientRef,
    port: MIDIPortRef,
    names: Vec<String>,
    _block: RcBlock<dyn Fn(NonNull<MIDIEventList>, *mut c_void)>,
}

// SAFETY: as for `NoteListener`: plain CoreMIDI object references, a block
// only CoreMIDI calls (its captures are `Send + Sync`), dropped after the
// port is disposed.
unsafe impl Send for SourceListener {}

impl SourceListener {
    /// Creates client `name` and connects it to every source present now
    /// except this process's own virtual sources.
    pub fn connect_all(name: &str, sink: SourceSink) -> Result<SourceListener, BackendError> {
        SourceListener::connect(name, sink, false)
    }

    /// Like [`SourceListener::connect_all`] but also listening to this
    /// process's own virtual sources (tests only).
    #[doc(hidden)]
    pub fn connect_including_own(
        name: &str,
        sink: SourceSink,
    ) -> Result<SourceListener, BackendError> {
        SourceListener::connect(name, sink, true)
    }

    fn connect(
        name: &str,
        sink: SourceSink,
        include_own: bool,
    ) -> Result<SourceListener, BackendError> {
        let mut client: MIDIClientRef = 0;
        let cf = CFString::from_str(name);
        // SAFETY: `client` is a valid out-pointer; no notification callback.
        os("MIDIClientCreate", unsafe {
            MIDIClientCreate(&cf, None, ptr::null_mut(), NonNull::from(&mut client))
        })?;
        // Scratch buffers reused across callbacks (CoreMIDI calls the block
        // from one thread at a time, so the lock is never contended).
        let scratch = Mutex::new((Vec::<Received>::new(), Vec::<u8>::new()));
        let block = RcBlock::new(move |list: NonNull<MIDIEventList>, src: *mut c_void| {
            // The connection's refCon is the source's key.
            let key = src as usize as u32;
            if let Ok(mut s) = scratch.try_lock() {
                let (out, sx) = &mut *s;
                out.clear();
                decode(list, out, sx);
                for r in out.iter() {
                    sink(key, r.host_ns, &r.bytes);
                }
            }
        });
        let mut port: MIDIPortRef = 0;
        let pcf = CFString::from_str("VizDAW Record In");
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
        let mut names = Vec::new();
        let own = OWN_SOURCES.lock().map(|o| o.clone()).unwrap_or_default();
        for i in 0..n {
            // SAFETY: `i` is below the count just read.
            let src = unsafe { MIDIGetSource(i) };
            if src == 0 {
                continue;
            }
            // SAFETY: the property keys are static CFStrings.
            let (name_key, id_key) = unsafe { (kMIDIPropertyName, kMIDIPropertyUniqueID) };
            let id = int_property(src, id_key);
            let sname = cfstring_property(src, name_key).unwrap_or_default();
            if !include_own && is_own_source(src, id, Some(sname.as_str()), &own) {
                continue;
            }
            let key = core_engine::input::port_key(&sname);
            // SAFETY: both objects are live; the port belongs to our client;
            // the refCon is a plain integer CoreMIDI hands back unread.
            let ok = os("MIDIPortConnectSource", unsafe {
                MIDIPortConnectSource(port, src, key as usize as *mut c_void)
            });
            if ok.is_ok() {
                names.push(sname);
            }
        }
        Ok(SourceListener {
            client,
            port,
            names,
            _block: block,
        })
    }

    /// The names of the sources it listens to.
    pub fn sources(&self) -> &[String] {
        &self.names
    }
}

impl Drop for SourceListener {
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
pub fn ns_to_host(ns: u64) -> u64 {
    // SAFETY: pure conversion.
```

In `crates/core-ffi/src/host.rs`:

Find:
```rust
use core_engine::backend::{AudioBackend, BackendEvent};
use core_engine::engine::{Engine, EngineConfig, PendingPlay};
use core_engine::graph::NoInserts;
use core_engine::input::{InputSpec, MidiInputSpec, MidiSender, MonitorRoute, port_key};
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::{RecordHandles, RecordRequest};
use core_engine::render::Renderer;
```
Replace with:
```rust
use core_engine::backend::{AudioBackend, BackendEvent};
use core_engine::engine::{Engine, EngineConfig, PendingPlay};
use core_engine::graph::NoInserts;
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, MidiSender, MonitorRoute, port_key};
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
use core_engine::record::{RecordHandles, RecordRequest};
use core_engine::render::Renderer;
```

Find:
```rust
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}

/// The input device live hosts open (`None`: the output device's own
```
Replace with:
```rust
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}

/// Where the CoreMIDI sources' messages go: the newest host's engine
/// (spec §6.6).
static MIDI_TO: Mutex<Option<MidiSender>> = Mutex::new(None);

/// The record listener on every CoreMIDI source (plan 5c Task 12).
#[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
static SOURCES: Mutex<Option<platform_macos::midi::SourceListener>> = Mutex::new(None);

/// A message from CoreMIDI source `port` at `host_ns` for the engine.
fn midi_from_source(port: u32, host_ns: u64, bytes: &[u8]) {
    let Some(msg) = core_engine::input::channel_message(bytes) else {
        return;
    };
    if let Some(tx) = lock(&MIDI_TO).as_ref() {
        let _ = tx.send(MidiIn { port, host_ns, msg });
    }
}

/// Hands `bytes` to the engine as if CoreMIDI source `name` sent them at
/// `host_ns` (test hook: tests connect no CoreMIDI listener).
#[cfg(feature = "test-hooks")]
pub fn debug_midi_from_source(name: &str, host_ns: u64, bytes: &[u8]) {
    midi_from_source(port_key(name), host_ns, bytes);
}

/// (Re)connects the record listener to every MIDI source present now:
/// armed MIDI lanes hear them (spec §6.6), stamped with their host time
/// (spec §6.3). Never in test builds, where `midi.inject` stands in.
pub fn listen_to_midi_sources() {
    #[cfg(all(target_os = "macos", not(feature = "test-hooks")))]
    {
        let mut slot = lock(&SOURCES);
        *slot = None;
        if let Ok(l) = platform_macos::midi::SourceListener::connect_all(
            "VizDAW Record Input",
            Box::new(midi_from_source),
        ) {
            *slot = Some(l);
        }
    }
    #[cfg(any(not(target_os = "macos"), feature = "test-hooks"))]
    {
        let _ = midi_from_source;
    }
}

/// The input device live hosts open (`None`: the output device's own
```

Find:
```rust
    /// rate; when it runs at another, regions are muted (Ruling 20).
    fn create(project: f64) -> Result<Host, VizError> {
        let mut h = Host::create_any(project)?;
        if STALL_DISK.load(Ordering::Relaxed) {
            h.engine.streamer().set_stalled(true);
        }
```
Replace with:
```rust
    /// rate; when it runs at another, regions are muted (Ruling 20).
    fn create(project: f64) -> Result<Host, VizError> {
        let mut h = Host::create_any(project)?;
        *lock(&MIDI_TO) = Some(h.engine.midi_sender());
        if STALL_DISK.load(Ordering::Relaxed) {
            h.engine.streamer().set_stalled(true);
        }
```

Find:
```rust
        )?;
        engine.set_rt_env(rt_env(&backend, info.buffer_frames, info.sample_rate));
        backend.start(renderer)?;
        Ok(Host::new(
            engine,
            Output::Hal(backend),
```
Replace with:
```rust
        )?;
        engine.set_rt_env(rt_env(&backend, info.buffer_frames, info.sample_rate));
        backend.start(renderer)?;
        // MIDI sources play into this engine from now on.
        listen_to_midi_sources();
        Ok(Host::new(
            engine,
            Output::Hal(backend),
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
                "the record range is empty (an empty loop, or a punch outside it)",
            ));
        }
        // The engine plays (this loads the sequence and locates); the tap
        // goes in before the Play, so the roll is the first block captured.
        inner.mark(self.id, true);
```
Replace with:
```rust
                "the record range is empty (an empty loop, or a punch outside it)",
            ));
        }
        // MIDI sources plugged in since the engine started are heard too.
        if !midi_lanes.is_empty() {
            host::listen_to_midi_sources();
        }
        // The engine plays (this loads the sequence and locates); the tap
        // goes in before the Play, so the roll is the first block captured.
        inner.mark(self.id, true);
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test midi 2>&1 | tail -3
cargo test -p core-ffi --test live_input 2>&1 | tail -3
```
Expected: `midi` passes `a_source_listener_names_the_source_and_keeps_the_time` (a known-flaky file: re-run once before investigating); `live_input` 7 passed.

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo test -p platform-macos 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/platform-macos/src/midi.rs crates/core-ffi/src/host.rs crates/core-ffi/src/session.rs crates/platform-macos/tests/midi.rs crates/core-ffi/tests/live_input.rs
git commit -F - <<'VIZDAW_MSG'
platform-macos: CoreMIDI sources into the engine

A source listener connects every source but VizDAW's own and calls
back with the source's name, its time stamp and the bytes; the host
feeds them to the newest engine. Test builds never connect it.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 13: The app — `RecordModel`, Transport > Record and the recovery alert

**Files:**
- Create: `app/Sources/VizDAWAppModel/RecordModel.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift` (`record`, the poll, `offerRecoveryIfNeeded`)
- Modify: `app/Sources/VizDAWAppModel/TransportCommands.swift` (`transport.record`, keep, discard)
- Modify: `app/Sources/VizDAW/DocumentView.swift` (the "Recovered Recordings" alert)
- Test: `app/Tests/VizDAWAppModelTests/RecordModelTests.swift`; modify `app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift`

**Interfaces:**
- Consumes: Tasks 9 and 10's `Session` methods.
- Produces: `RecordModel` (`countInBars`, `preRollBars`, `passesAsTakes`, `state`, `recoveryOffer`, `isRecording`, `isLoopRecording`, `spec()`, `toggle() -> KeyOutcome`, `press(keep:)`, `refresh()`, `showsRecoveryAlert(channelOn:)`, `recoveryMessage`, `answerRecovery(import: Bool?)`); `DocumentModel.record`.

Ruling 22. Only Transport > Record (an existing menu item, until now "not yet") and the recovery alert are UI here; the record controls are 5d's.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/RecordModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Recording in the app (plan 5c Task 13): Record asks the core with the
/// transport's replace, punch and loop; Record again stops; keep and
/// discard apply only while loop recording; step entry refuses Record;
/// and a crash's recordings are offered once, unless the control channel
/// is on.
@MainActor
@Suite struct RecordModelTests {
    let host = FakeControlHost()

    func demo(_ f: FakeSession = demoFake()) throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID) -> ControlFailure? {
        if case .failed(let f) = ControlCommands.run(id, args: [], doc: (doc, m), store: s, host: host) { return f }
        return nil
    }

    @Test func recordAsksWithTheTransportsModesAndStopsOnTheSecondPress() throws {
        let (s, id, m, f) = try demo()
        m.select(3)
        m.editor.updateLayout {
            $0.transport.overdub = true
            $0.transport.punch = true
            $0.transport.punchIn = 15_360
            $0.transport.punchOut = 61_440
        }
        m.toggleLoop()
        m.record.preRollBars = 2
        #expect(TransportCommands.enabled("transport.record", m))
        #expect(run("transport.record", m, s, id) == nil)
        let call = try #require(f.recordCalls.last)
        #expect(call.seq == 3)
        #expect(
            call.spec
                == RecordSpecRow(
                    replace: false, punch: TickRange(start: 15_360, end: 61_440), looped: true, countInBars: 0,
                    preRollBars: 2, passesAsTakes: true))
        #expect(m.record.isRecording && m.record.isLoopRecording)
        // Keep and discard while loop recording.
        #expect(TransportCommands.enabled("record.keep_pass", m))
        #expect(run("record.keep_pass", m, s, id) == nil)
        #expect(run("record.discard_pass", m, s, id) == nil)
        #expect(f.passPresses == [true, false])
        // Record again stops (and finishes the pass).
        #expect(run("transport.record", m, s, id) == nil)
        #expect(!m.record.isRecording && !f.state.playing)
        #expect(!TransportCommands.enabled("record.keep_pass", m))
        #expect(run("record.keep_pass", m, s, id)?.code == .unavailable)
        #expect(f.recordCalls.count == 1)
    }

    @Test func recordNeedsASequenceAndNoStepEntry() throws {
        let (s, id, m, f) = try demo()
        m.select(nil)
        #expect(!TransportCommands.enabled("transport.record", m))
        #expect(run("transport.record", m, s, id)?.code == .unavailable)
        m.select(3)
        _ = ControlCommands.run("step.entry", args: [], doc: (id, m), store: s, host: host)
        #expect(m.stepEntry.isOn)
        #expect(run("transport.record", m, s, id) == .of(.failed, RecordModel.stepEntryOn))
        m.stepEntry.stop()
        f.recordError = .Model(message: "arm a lane that has an input to record")
        #expect(run("transport.record", m, s, id) == .of(.failed, "arm a lane that has an input to record"))
        f.recordError = .Busy(message: "Stop recording first.")
        #expect(run("transport.record", m, s, id)?.code == .busy)
        #expect(f.recordCalls.isEmpty)
    }

    @Test func aCrashsRecordingsAreOfferedOnceAndAnswered() throws {
        let f = demoFake()
        f.recovered = ["Media/Recorded/Mic 2026-10-20 120000.caf"]
        let (_, _, m, _) = try demo(f)
        #expect(m.record.showsRecoveryAlert(channelOn: false))
        #expect(!m.record.showsRecoveryAlert(channelOn: true))
        #expect(m.record.recoveryMessage.contains("Mic 2026-10-20 120000"))
        m.record.answerRecovery(import: true)
        #expect(f.recoverAnswers == [true])
        #expect(!m.record.showsRecoveryAlert(channelOn: false))
        // "Later": nothing is asked of the core.
        let g = demoFake()
        g.recovered = ["Media/Recorded/a.caf", "Media/Recorded/b.caf"]
        let (_, _, n, _) = try demo(g)
        #expect(n.record.recoveryMessage.hasPrefix("2 recordings were"))
        n.record.answerRecovery(import: nil)
        #expect(g.recoverAnswers.isEmpty && !n.record.showsRecoveryAlert(channelOn: false))
        // Under the control channel nothing is offered (`record.recover`
        // answers).
        let h = demoFake()
        h.recovered = ["Media/Recorded/a.caf"]
        let (_, _, o, _) = try demo(h)
        o.record.answerRecovery(import: nil)
        o.record.offerRecoveryIfNeeded(channelOn: true)
        #expect(!o.record.showsRecoveryAlert(channelOn: false))
    }

    @Test func thePollFollowsTheRecording() throws {
        let (s, id, m, f) = try demo()
        m.select(3)
        _ = run("transport.record", m, s, id)
        #expect(m.record.isRecording)
        // The core ended it by itself (a full disk): the poll notices.
        f.recordRow.recording = false
        f.state.playing = false
        m.poll()
        #expect(!m.record.isRecording)
    }
}
VIZDAW_EOF
```

In `app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift`:

Find:
```swift
        #expect(!m.editor.layout.snap.enabled && !TransportCommands.isOn("editors.snap", m))
    }

    @Test func recordingCommandsWaitForSubProjectFive() throws {
        let (s, id, m, _) = try demo()
        for cmd in ["transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass"] {
            #expect(failure(run(cmd, s, id)) == .of(.unavailable, TransportCommands.notYet), "\(cmd)")
            #expect(!TransportCommands.enabled(cmd, m))
        }
    }

    @Test func recordArmMovesAndARefusalShows() throws {
```
Replace with:
```swift
        #expect(!m.editor.layout.snap.enabled && !TransportCommands.isOn("editors.snap", m))
    }

    @Test func scrubbingWaits() throws {
        let (s, id, m, _) = try demo()
        #expect(failure(run("transport.scrub", s, id)) == .of(.unavailable, TransportCommands.notYet))
        #expect(!TransportCommands.enabled("transport.scrub", m))
    }

    @Test func recordArmMovesAndARefusalShows() throws {
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test 2>&1 | tail -5
```
Expected: does not compile: `RecordModel` is not defined.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/RecordModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// Recording in a document (plan 5c Task 13; sub-project 5 spec §6.4,
/// §6.5): what Record asks the core for, what the core reports while it
/// records, and the crash-recovery offer. Replace, punch and loop come
/// from the transport (overdub, the punch fields, Loop); count-in,
/// pre-roll and passes-as-takes are kept here for plan 5d's record
/// popover (and `record.set`).
@Observable
@MainActor
public final class RecordModel {
    weak var document: DocumentModel?
    /// Bars of count-in (0–4; the core refuses more than 0 until plan 5d
    /// plays the click).
    public var countInBars: UInt8 = 0
    /// Bars of pre-roll (0–8).
    public var preRollBars: UInt8 = 0
    /// Kept loop passes become takes.
    public var passesAsTakes = true
    /// What the core last reported.
    public private(set) var state: RecordStateRow?
    /// Recordings a crash left, offered once at open and not yet
    /// answered (never under the control channel).
    public private(set) var recoveryOffer: [String] = []

    /// Why Record is refused while step entry is on (spec §6.4).
    public static let stepEntryOn = "Step entry is on: turn it off to record."

    init(document: DocumentModel) { self.document = document }

    /// A pass is recording.
    public var isRecording: Bool { state?.recording ?? false }

    /// A loop pass is recording: keep and discard apply.
    public var isLoopRecording: Bool { isRecording && (state?.spec?.looped ?? false) }

    /// What Record asks for now.
    public func spec() -> RecordSpecRow {
        let t = document?.editor.layout.transport ?? TransportMarks()
        return RecordSpecRow(
            replace: !t.overdub,
            punch: t.punch ? TickRange(start: t.punchIn, end: t.punchOut) : nil,
            looped: document?.looping ?? false,
            countInBars: countInBars, preRollBars: preRollBars, passesAsTakes: passesAsTakes)
    }

    /// `transport.record`: records the shown sequence, or, while recording,
    /// stops (which finishes the pass).
    public func toggle() -> KeyOutcome {
        guard let m = document else { return .unavailable("No document is open.") }
        if isRecording {
            do {
                try m.session.stop()
            } catch {
                return Self.refused(error)
            }
            refresh()
            m.poll()
            return .done
        }
        guard let seq = m.selectedSeq else { return .unavailable("Select a sequence first.") }
        guard m.stepEntry.state == nil else { return .refused(Self.stepEntryOn) }
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        do {
            try m.session.record(seq: seq, spec: spec())
        } catch {
            return Self.refused(error)
        }
        refresh()
        m.poll()
        return .done
    }

    /// `record.keep_pass` (`keep`) or `record.discard_pass`.
    public func press(keep: Bool) -> KeyOutcome {
        guard let m = document else { return .unavailable("No document is open.") }
        guard isLoopRecording else { return .unavailable("Keep and discard act on loop passes while recording.") }
        do {
            if keep { try m.session.keepPass() } else { try m.session.discardPass() }
        } catch {
            return Self.refused(error)
        }
        refresh()
        return .done
    }

    /// Reads what the core is doing (the poll calls this while playing or
    /// recording).
    public func refresh() {
        guard let m = document else { return }
        if let s = try? m.session.recordState(), s != state { state = s }
    }

    /// Offers the recordings a crash left (spec §6.5), unless the control
    /// channel is on (it answers with `record.recover`).
    func offerRecoveryIfNeeded(channelOn: Bool = MediaModel.channelIsOn()) {
        guard !channelOn, let m = document else { return }
        recoveryOffer = (try? m.session.recoveredRecordings()) ?? []
    }

    /// Whether the window shows the recovery alert.
    public func showsRecoveryAlert(channelOn: Bool) -> Bool { !channelOn && !recoveryOffer.isEmpty }

    /// The alert's text: how many recordings and their names.
    public var recoveryMessage: String {
        let names = recoveryOffer.map { (($0 as NSString).lastPathComponent as NSString).deletingPathExtension }
        let what = names.count == 1 ? "A recording was" : "\(names.count) recordings were"
        return "\(what) left by VizDAW quitting unexpectedly: \(names.joined(separator: ", "))."
    }

    /// Answers the offer: import (`true`: a new lane each, one undo step)
    /// or move to the Trash (`false`); `nil` ("Later") leaves them to be
    /// offered at the next open.
    public func answerRecovery(import answer: Bool?) {
        recoveryOffer = []
        guard let m = document, let answer else { return }
        do {
            m.apply(try m.session.recoverRecordings(import: answer))
        } catch {
            m.alert = AlertItem(error, title: "Recovery Failed")
        }
    }

    static func refused(_ error: any Error) -> KeyOutcome {
        if case VizError.Busy(let message) = error { return .busy(message) }
        return .refused(AlertItem(error, title: "").message)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
    /// (plan 5b): views drawing `Pending` or `Coarse` waveforms redraw.
    public internal(set) var overviewVersion = 0
    @ObservationIgnored private var mediaModel: MediaModel?

    /// The media pool (plan 5b), made once.
    public var media: MediaModel {
```
Replace with:
```swift
    /// (plan 5b): views drawing `Pending` or `Coarse` waveforms redraw.
    public internal(set) var overviewVersion = 0
    @ObservationIgnored private var mediaModel: MediaModel?
    @ObservationIgnored private var recordModel: RecordModel?

    /// Recording (plan 5c), made once.
    public var record: RecordModel {
        if let m = recordModel { return m }
        let m = RecordModel(document: self)
        recordModel = m
        return m
    }

    /// The media pool (plan 5b), made once.
    public var media: MediaModel {
```

Find:
```swift
        // relink sheet (plan 5b Ruling 14; the app never shows it while
        // the control channel is on).
        media.offerRelinkIfNeeded()
    }

    /// The transport shown until the first successful read.
```
Replace with:
```swift
        // relink sheet (plan 5b Ruling 14; the app never shows it while
        // the control channel is on).
        media.offerRelinkIfNeeded()
        // Recordings a crash left are offered too (plan 5c Task 13).
        record.offerRecoveryIfNeeded()
    }

    /// The transport shown until the first successful read.
```

Find:
```swift
            updatePlayKey()
            if transport.rateMismatch != (projectInfo?.rateMismatch ?? false) { refreshProjectInfo() }
            if !transport.faults.contains("stream-underrun") { diskWarningDismissed = false }
        } catch VizError.Busy {
            return
        } catch {
```
Replace with:
```swift
            updatePlayKey()
            if transport.rateMismatch != (projectInfo?.rateMismatch ?? false) { refreshProjectInfo() }
            if !transport.faults.contains("stream-underrun") { diskWarningDismissed = false }
            // A recording ends by Stop, at the end, or by itself (a full
            // disk, a lost device): the poll notices.
            if transport.playing || record.isRecording { record.refresh() }
        } catch VizError.Busy {
            return
        } catch {
```

In `app/Sources/VizDAWAppModel/TransportCommands.swift`:

Find:
```swift
/// the control channel's `command.run`. None opens a dialog.
@MainActor
public enum TransportCommands {
    /// Recording, scrubbing and loop passes are sub-project 5's; their
    /// commands exist so keys can be bound now (editors spec §1).
    public static let notYet = "Recording and scrubbing arrive with sub-project 5."
    static let noSequence = "Select a sequence first."

    public static let ids: Set<String> = Set(
```
Replace with:
```swift
/// the control channel's `command.run`. None opens a dialog.
@MainActor
public enum TransportCommands {
    /// Scrubbing comes later; its command exists so a key can be bound now
    /// (editors spec §1). Recording is plan 5c's (`RecordModel`).
    public static let notYet = "Scrubbing is not available yet."
    static let noSequence = "Select a sequence first."

    public static let ids: Set<String> = Set(
```

Find:
```swift
    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        if typesText(id) && !m.transportKeysEnabled { return false }
        return switch id {
        case "transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass": false
        case "transport.stop": m.isPlaying
        case "transport.overdub", "transport.punch", "editors.snap": true
        case "transport.play_from_edit_in": m.selectedSeq != nil && m.editor.selection.timeRange != nil
```
Replace with:
```swift
    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        if typesText(id) && !m.transportKeysEnabled { return false }
        return switch id {
        case "transport.scrub": false
        case "transport.record": m.selectedSeq != nil || m.record.isRecording
        case "record.keep_pass", "record.discard_pass": m.record.isLoopRecording
        case "transport.stop": m.isPlaying
        case "transport.overdub", "transport.punch", "editors.snap": true
        case "transport.play_from_edit_in": m.selectedSeq != nil && m.editor.selection.timeRange != nil
```

Find:
```swift
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> KeyOutcome {
        if typesText(id) && !m.transportKeysEnabled { return .unavailable(textFieldHasKeyboard) }
        switch id {
        case "transport.record", "transport.scrub", "record.keep_pass", "record.discard_pass":
            return .unavailable(notYet)
        case "transport.overdub":
            m.editor.updateLayout { $0.transport.overdub = on ?? !$0.transport.overdub }
            return .done
```
Replace with:
```swift
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> KeyOutcome {
        if typesText(id) && !m.transportKeysEnabled { return .unavailable(textFieldHasKeyboard) }
        switch id {
        case "transport.scrub":
            return .unavailable(notYet)
        case "transport.record":
            return m.record.toggle()
        case "record.keep_pass", "record.discard_pass":
            return m.record.press(keep: id == "record.keep_pass")
        case "transport.overdub":
            m.editor.updateLayout { $0.transport.overdub = on ?? !$0.transport.overdub }
            return .done
```

In `app/Sources/VizDAW/DocumentView.swift`:

Find:
```swift
                set: { if !$0 { model.media.leaveOffline() } })
        ) {
            RelinkSheet(media: model.media)
        }
        .alert(
            model.alert?.title ?? "",
```
Replace with:
```swift
                set: { if !$0 { model.media.leaveOffline() } })
        ) {
            RelinkSheet(media: model.media)
        }
        .alert(
            "Recovered Recordings",
            isPresented: Binding(
                get: { model.record.showsRecoveryAlert(channelOn: ControlChannel.active) },
                set: { if !$0 { model.record.answerRecovery(import: nil) } })
        ) {
            Button("Import into New Lanes") { model.record.answerRecovery(import: true) }
            Button("Move to Trash", role: .destructive) { model.record.answerRecovery(import: false) }
            Button("Later", role: .cancel) { model.record.answerRecovery(import: nil) }
        } message: {
            Text(model.record.recoveryMessage)
        }
        .alert(
            model.alert?.title ?? "",
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: every test passes (the new `RecordModelTests` suite and `scrubbingWaits` among them).

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: no warnings, every `test result: ok`.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/RecordModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/TransportCommands.swift app/Sources/VizDAW/DocumentView.swift app/Tests/VizDAWAppModelTests/RecordModelTests.swift app/Tests/VizDAWAppModelTests/TransportCommandsTests.swift
git commit -F - <<'VIZDAW_MSG'
app: RecordModel, Transport > Record and the recovery alert

Transport > Record records the shown sequence with the transport's
modes and stops on the second press; it is refused while step entry is
on. The poll follows the recording. A crash's recordings are offered
once: import into new lanes, move to the Trash, or later.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 14: Control channel — record, lane input, monitor and MIDI injection commands

**Files:**
- Modify: `crates/core-control/src/registry.rs` (five commands)
- Modify: `crates/core-control/src/request.rs` (`state.get record`, `input`)
- Modify: `crates/core-ffi/src/control.rs` (`StateTargetRow::{Record, Input}`, `ControlResult::{Record, InputState}`, JSON; lanes gain `takes`, `active_take`, `input`, `monitor`)
- Modify: `app/Sources/VizDAWAppModel/ControlCommands.swift`
- Test: `crates/core-control/tests/commands.rs`, `crates/core-ffi/tests/control.rs`, `app/Tests/VizDAWAppModelTests/RecordModelTests.swift`

**Interfaces:**
- Consumes: Tasks 8–13.
- Produces: `record.set`, `record.recover`, `lane.set_input`, `lane.set_monitor`, `midi.inject` (Ruling 18), handled in `ControlCommands.run`; `transport.record`, `record.keep_pass`, `record.discard_pass` (registered since 4f) reach Task 13's handlers.

Every new command checks its params by type and range in `core-control` before the app sees it (Review Focus 5).

- [ ] **Step 1: Write the failing tests**

In `crates/core-control/tests/commands.rs`:

Find:
```rust
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        119,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four"
    );
    for c in COMMANDS {
        // snake_case words (a locator's number too), a dot after the group.
```
Replace with:
```rust
    assert_eq!(ids.len(), COMMANDS.len(), "IDs are unique");
    assert_eq!(
        COMMANDS.len(),
        124,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four; 5c adds record.set, record.recover, lane.set_input, lane.set_monitor and midi.inject"
    );
    for c in COMMANDS {
        // snake_case words (a locator's number too), a dot after the group.
```

Find:
```rust
        }
    ));
}
```
Replace with:
```rust
        }
    ));
}

/// Plan 5c Task 14: the record and lane commands take their parameters
/// by type and range; hostile values never reach the app.
#[test]
fn record_and_lane_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("record.set", json!({})),
        (
            "record.set",
            json!({ "replace": false, "punch": true, "loop": true, "count_in": 0, "pre_roll": 8, "passes_as_takes": false }),
        ),
        ("record.recover", json!({ "action": "import" })),
        ("record.recover", json!({ "action": "trash" })),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "channel": 3, "stereo": true }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "midi", "port": "Keys", "midi_channel": 16 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "none" }),
        ),
        (
            "lane.set_monitor",
            json!({ "seq": 1, "lane": 2, "mode": "input" }),
        ),
        (
            "midi.inject",
            json!({ "port": "Keys", "bytes": "90 3c 64" }),
        ),
    ] {
        assert!(
            matches!(
                ok("command.run", run(id, params.clone())),
                Command::CommandRun { .. }
            ),
            "{id} {params}"
        );
    }
    for (id, params) in [
        ("record.set", json!({ "count_in": 5 })),
        ("record.set", json!({ "pre_roll": -1 })),
        ("record.set", json!({ "loop": "yes" })),
        ("record.set", json!({ "takes": true })),
        ("record.recover", json!({})),
        ("record.recover", json!({ "action": "delete" })),
        ("lane.set_input", json!({ "lane": 2, "kind": "audio" })),
        ("lane.set_input", json!({ "seq": 1, "lane": 2 })),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "video" }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "channel": 0 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "channel": 257 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "midi", "midi_channel": 17 }),
        ),
        (
            "lane.set_input",
            json!({ "seq": -1, "lane": 2, "kind": "none" }),
        ),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "midi", "port": "x".repeat(100_000) }),
        ),
        ("lane.set_monitor", json!({ "seq": 1, "lane": 2 })),
        (
            "lane.set_monitor",
            json!({ "seq": 1, "lane": 2, "mode": "loud" }),
        ),
        ("midi.inject", json!({ "port": "Keys" })),
        ("midi.inject", json!({ "bytes": "90 3c 64" })),
        (
            "midi.inject",
            json!({ "port": "Keys", "bytes": [0x90, 60, 100] }),
        ),
        (
            "midi.inject",
            json!({ "port": "Keys", "bytes": "9".repeat(100_000) }),
        ),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    let spec = |id: &str| registry::find(id).unwrap();
    for id in [
        "record.set",
        "record.recover",
        "lane.set_input",
        "lane.set_monitor",
        "midi.inject",
    ] {
        assert!(!spec(id).menu_item && spec(id).document, "{id}");
    }
}

#[test]
fn state_get_record_and_input() {
    for (name, target) in [
        ("record", StateTarget::Record),
        ("input", StateTarget::Input),
    ] {
        match ok("state.get", json!({ "target": name })) {
            Command::StateGet { target: t, .. } => assert_eq!(t, target),
            other => panic!("{other:?}"),
        }
    }
}
```

In `crates/core-ffi/tests/control.rs`:

Find:
```rust
    assert!(!s.extend_timeout(call.token, 1000).unwrap());
    s.stop().unwrap();
}
```
Replace with:
```rust
    assert!(!s.extend_timeout(call.token, 1000).unwrap());
    s.stop().unwrap();
}

/// Plan 5c Task 14: `state.get record` and `input` as JSON.
#[test]
fn record_and_input_results_as_json() {
    use vizdaw_ffi::{InputInfoRow, RecordLaneRow, RecordSpecRow, RecordStateRow, TickRange};
    let v = result_json(&ControlResult::Record {
        doc: "D".into(),
        state: RecordStateRow {
            recording: true,
            seq: Some(3),
            spec: Some(RecordSpecRow {
                replace: false,
                punch: Some(TickRange { start: 0, end: 10 }),
                looped: true,
                count_in_bars: 0,
                pre_roll_bars: 1,
                passes_as_takes: true,
            }),
            pass: 2,
            kept: 1,
            discarded: 0,
            lanes: vec![RecordLaneRow {
                lane: 7,
                audio: true,
                path: Some("Media/Recorded/Mic.caf".into()),
                clipped: 4,
            }],
            frames: 96_000,
            overflows: 0,
            gap_frames: 0,
            free_bytes: Some(1 << 30),
            recovered: vec!["Media/Recorded/old.caf".into()],
        },
    });
    assert_eq!(v["recording"], json!(true));
    assert_eq!(v["spec"]["loop"], json!(true));
    assert_eq!(v["spec"]["punch"], json!({"start": 0, "end": 10}));
    assert_eq!(v["pass"], json!(2));
    assert_eq!(v["lanes"][0]["clipped"], json!(4));
    assert_eq!(v["recovered"][0], json!("Media/Recorded/old.caf"));
    let i = result_json(&ControlResult::InputState {
        doc: "D".into(),
        info: InputInfoRow {
            device: "Offline".into(),
            channels: 3,
            names: vec!["Fake In 1".into()],
            input_latency_frames: 512,
            output_latency_frames: 512,
        },
    });
    assert_eq!(i["channels"], json!(3));
    assert_eq!(i["input_latency"], json!(512));
}
```

In `app/Tests/VizDAWAppModelTests/RecordModelTests.swift`:

Find:
```swift
        #expect(!m.record.isRecording)
    }
}
```
Replace with:
```swift
        #expect(!m.record.isRecording)
    }
}

/// The record and lane commands over the control channel (plan 5c Task
/// 14): `record.set` sets the transport's modes and the record options,
/// `record.recover` answers the offer, `lane.set_input` and
/// `lane.set_monitor` edit lanes (channels from 1), `midi.inject` sends
/// hex bytes, and `state.get record` / `input` read the core.
@MainActor
@Suite struct RecordControlTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ pairs: [(String, ControlValue)], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlFailure?
    {
        let args = pairs.map { ControlArg(name: $0.0, value: $0.1) }
        if case .failed(let f) = ControlCommands.run(id, args: args, doc: (doc, m), store: s, host: host) { return f }
        return nil
    }

    @Test func recordSetSetsTheModesAndOptions() throws {
        let (s, id, m, _) = try demo()
        #expect(
            run(
                "record.set",
                [
                    ("replace", .flag(value: false)), ("punch", .flag(value: true)), ("loop", .flag(value: true)),
                    ("pre_roll", .int(value: 2)), ("passes_as_takes", .flag(value: false)),
                ], m, s, id) == nil)
        #expect(m.editor.layout.transport.overdub && m.editor.layout.transport.punch && m.looping)
        #expect(m.record.preRollBars == 2 && !m.record.passesAsTakes)
        let spec = m.record.spec()
        #expect(!spec.replace && spec.looped && spec.punch != nil && !spec.passesAsTakes)
        // Loop already on stays on.
        _ = run("record.set", [("loop", .flag(value: true))], m, s, id)
        #expect(m.looping)
    }

    @Test func laneInputsAndMonitorsAreLaneEdits() throws {
        let (s, id, m, f) = try demo()
        #expect(
            run(
                "lane.set_input",
                [
                    ("seq", .int(value: 3)), ("lane", .int(value: 31)), ("kind", .text(value: "audio")),
                    ("channel", .int(value: 3)), ("stereo", .flag(value: true)),
                ], m, s, id) == nil)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 2, stereo: true))))
        _ = run(
            "lane.set_input",
            [
                ("seq", .int(value: 3)), ("lane", .int(value: 31)), ("kind", .text(value: "midi")),
                ("port", .text(value: "Keys")), ("midi_channel", .int(value: 16)),
            ], m, s, id)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .midi(port: "Keys", channel: 15))))
        _ = run(
            "lane.set_input", [("seq", .int(value: 3)), ("lane", .int(value: 31)), ("kind", .text(value: "none"))],
            m, s, id)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: nil)))
        _ = run(
            "lane.set_monitor", [("seq", .int(value: 3)), ("lane", .int(value: 31)), ("mode", .text(value: "off"))],
            m, s, id)
        #expect(f.edits.last == .lane(edit: .setMonitor(seq: 3, lane: 31, monitor: .off)))
        // The core's refusal is the call's failure.
        f.editError = .Model(message: "input.channels out of range")
        #expect(
            run(
                "lane.set_input",
                [("seq", .int(value: 3)), ("lane", .int(value: 31)), ("kind", .text(value: "audio"))], m, s, id)?
                .message.contains("input.channels") == true)
    }

    @Test func midiInjectSendsHexBytes() throws {
        let (s, id, m, f) = try demo()
        #expect(run("midi.inject", [("port", .text(value: "Keys")), ("bytes", .text(value: "90 3c 64"))], m, s, id) == nil)
        #expect(f.injected.last?.port == "Keys" && f.injected.last?.bytes == Data([0x90, 0x3C, 0x64]))
        for bad in ["", "9g", "900", "90,3c", String(repeating: "90 ", count: 1025)] {
            #expect(
                run("midi.inject", [("port", .text(value: "Keys")), ("bytes", .text(value: bad))], m, s, id)?.code
                    == .invalidParams, "\(bad.prefix(10))")
        }
        #expect(ControlLanes.hexBytes("f0 7E f7") == Data([0xF0, 0x7E, 0xF7]))
    }

    @Test func recoverAnswersTheOfferAndStateGetReadsTheCore() throws {
        let (s, id, m, f) = try demo()
        #expect(run("record.recover", [("action", .text(value: "import"))], m, s, id)?.code == .unavailable)
        f.recovered = ["Media/Recorded/a.caf"]
        #expect(run("record.recover", [("action", .text(value: "trash"))], m, s, id) == nil)
        #expect(f.recoverAnswers == [false])
        f.recordRow.frames = 4800
        guard case .record(_, let st)? = ControlStates.state(.record, id, m) else {
            Issue.record("record")
            return
        }
        #expect(st.frames == 4800)
        f.inputRow.channels = 3
        guard case .inputState(_, let info)? = ControlStates.state(.input, id, m) else {
            Issue.record("input")
            return
        }
        #expect(info.channels == 3)
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands record_and_lane 2>&1 | tail -5
```
Expected: FAILED: `record.set` is not a registered command.

- [ ] **Step 3: Implement**

In `crates/core-control/src/registry.rs`:

Find:
```rust
        },
        required: false,
        help: "Where the region starts (negative imports at 0).",
    },
];
```
Replace with:
```rust
        },
        required: false,
        help: "Where the region starts (negative imports at 0).",
    },
];

/// An ID parameter (a sequence, a lane), required.
const fn id_param(name: &'static str, help: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: true,
        help,
    }
}

/// `record.set` (plan 5c Ruling 18): every option of the next Record.
const RECORD_SET_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "replace",
        kind: ParamKind::Flag,
        required: false,
        help: "Replace what the record range holds (true) or overdub it (false); sets transport.overdub.",
    },
    ParamSpec {
        name: "punch",
        kind: ParamKind::Flag,
        required: false,
        help: "Record only between the punch fields (sets transport.punch).",
    },
    ParamSpec {
        name: "loop",
        kind: ParamKind::Flag,
        required: false,
        help: "Loop recording over the transport loop (sets transport.loop).",
    },
    ParamSpec {
        name: "count_in",
        kind: ParamKind::Int { min: 0, max: 4 },
        required: false,
        help: "Bars of count-in (Record refuses more than 0 until plan 5d plays it).",
    },
    ParamSpec {
        name: "pre_roll",
        kind: ParamKind::Int { min: 0, max: 8 },
        required: false,
        help: "Bars of pre-roll.",
    },
    ParamSpec {
        name: "passes_as_takes",
        kind: ParamKind::Flag,
        required: false,
        help: "Kept loop passes become takes.",
    },
];

const RECOVER_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "action",
    kind: ParamKind::Choice(&["import", "trash"]),
    required: true,
    help: "Import the recordings a crash left onto new lanes, or move them to the Trash.",
}];

const SET_INPUT_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The lane's sequence."),
    id_param("lane", "The lane."),
    ParamSpec {
        name: "kind",
        kind: ParamKind::Choice(&["none", "audio", "midi"]),
        required: true,
        help: "What the lane records: nothing, audio or MIDI.",
    },
    ParamSpec {
        name: "channel",
        kind: ParamKind::Int { min: 1, max: 256 },
        required: false,
        help: "Audio: the input channel, from 1 (default 1).",
    },
    ParamSpec {
        name: "stereo",
        kind: ParamKind::Flag,
        required: false,
        help: "Audio: the channel and the next as a pair.",
    },
    ParamSpec {
        name: "port",
        kind: ParamKind::Text,
        required: false,
        help: "MIDI: the source's name (absent: every source).",
    },
    ParamSpec {
        name: "midi_channel",
        kind: ParamKind::Int { min: 1, max: 16 },
        required: false,
        help: "MIDI: the channel 1–16 (absent: every channel).",
    },
];

const SET_MONITOR_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The lane's sequence."),
    id_param("lane", "The lane."),
    ParamSpec {
        name: "mode",
        kind: ParamKind::Choice(&["auto", "input", "off"]),
        required: true,
        help: "When the lane's input is heard: auto (armed, stopped or recording in range), input (always) or off.",
    },
];

const INJECT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "port",
        kind: ParamKind::Text,
        required: true,
        help: "The MIDI source the bytes come from, as lanes name it.",
    },
    ParamSpec {
        name: "bytes",
        kind: ParamKind::Text,
        required: true,
        help: "Channel messages as hex bytes, e.g. \"90 3c 64 80 3c 00\".",
    },
];
```

Find:
```rust
        None,
        true,
    )),
    locator("locator.go_0", "Go to Locator 0", "0"),
    locator("locator.go_1", "Go to Locator 1", "1"),
    locator("locator.go_2", "Go to Locator 2", "2"),
```
Replace with:
```rust
        None,
        true,
    )),
    CommandSpec {
        params: RECORD_SET_PARAMS,
        ..key_only(cmd("record.set", "Record", "Record Options", None, true))
    },
    CommandSpec {
        params: RECOVER_PARAMS,
        ..key_only(cmd(
            "record.recover",
            "Record",
            "Recover Recordings",
            None,
            true,
        ))
    },
    CommandSpec {
        params: SET_INPUT_PARAMS,
        ..key_only(cmd("lane.set_input", "Lane", "Set Lane Input", None, true))
    },
    CommandSpec {
        params: SET_MONITOR_PARAMS,
        ..key_only(cmd(
            "lane.set_monitor",
            "Lane",
            "Set Lane Monitor",
            None,
            true,
        ))
    },
    CommandSpec {
        params: INJECT_PARAMS,
        ..key_only(cmd("midi.inject", "MIDI", "Inject MIDI", None, true))
    },
    locator("locator.go_0", "Go to Locator 0", "0"),
    locator("locator.go_1", "Go to Locator 1", "1"),
    locator("locator.go_2", "Go to Locator 2", "2"),
```

In `crates/core-control/src/request.rs`:

Find:
```rust
    Media,
    /// Its project format and device rate (5a).
    Project,
}

/// Where a click or drag lands.
```
Replace with:
```rust
    Media,
    /// Its project format and device rate (5a).
    Project,
    /// What recording is doing (plan 5c).
    Record,
    /// What the audio input offers (plan 5c).
    Input,
}

/// Where a click or drag lands.
```

Find:
```rust
                "windows" => StateTarget::Windows,
                "media" => StateTarget::Media,
                "project" => StateTarget::Project,
                _ => {
                    return Err(p.error(
                        "`target` must be transport, document, editor, selection, windows, media or project",
                    ));
                }
            };
```
Replace with:
```rust
                "windows" => StateTarget::Windows,
                "media" => StateTarget::Media,
                "project" => StateTarget::Project,
                "record" => StateTarget::Record,
                "input" => StateTarget::Input,
                _ => {
                    return Err(p.error(
                        "`target` must be transport, document, editor, selection, windows, media, project, record or input",
                    ));
                }
            };
```

In `crates/core-ffi/src/control.rs`:

Find:
```rust
    Media,
    /// Project format and device rate.
    Project,
}

/// The after-crash answer.
```
Replace with:
```rust
    Media,
    /// Project format and device rate.
    Project,
    /// Recording (plan 5c).
    Record,
    /// Audio input (plan 5c).
    Input,
}

/// The after-crash answer.
```

Find:
```rust
        /// Format, mismatched media, device rate.
        info: crate::format::ProjectInfoRow,
    },
    /// `command.run`.
    Ran {
        /// Registry ID.
```
Replace with:
```rust
        /// Format, mismatched media, device rate.
        info: crate::format::ProjectInfoRow,
    },
    /// `state.get record` (plan 5c).
    Record {
        /// Document ID.
        doc: String,
        /// What recording is doing.
        state: crate::record::RecordStateRow,
    },
    /// `state.get input` (plan 5c).
    InputState {
        /// Document ID.
        doc: String,
        /// What the input offers.
        info: crate::live::InputInfoRow,
    },
    /// `command.run`.
    Ran {
        /// Registry ID.
```

Find:
```rust
                StateTarget::Windows => StateTargetRow::Windows,
                StateTarget::Media => StateTargetRow::Media,
                StateTarget::Project => StateTargetRow::Project,
            },
            window: window_row(window),
        },
```
Replace with:
```rust
                StateTarget::Windows => StateTargetRow::Windows,
                StateTarget::Media => StateTargetRow::Media,
                StateTarget::Project => StateTargetRow::Project,
                StateTarget::Record => StateTargetRow::Record,
                StateTarget::Input => StateTargetRow::Input,
            },
            window: window_row(window),
        },
```

Find:
```rust
    roots
}

fn transport_json(doc: &str, t: &TransportState) -> Value {
    json!({
        "doc": doc, "playing": t.playing, "seq": t.seq, "tick": t.tick,
```
Replace with:
```rust
    roots
}

/// A lane's input as the channel spells it (`channel`s from 1).
fn lane_input_json(i: &crate::edit::LaneInputRow) -> Value {
    match i {
        crate::edit::LaneInputRow::Audio { first, stereo } => {
            json!({"kind": "audio", "channel": u32::from(*first) + 1, "stereo": stereo})
        }
        crate::edit::LaneInputRow::Midi { port, channel } => json!({
            "kind": "midi", "port": port, "midi_channel": channel.map(|c| u32::from(c) + 1),
        }),
    }
}

fn monitor_name(m: crate::edit::MonitorModeRow) -> &'static str {
    match m {
        crate::edit::MonitorModeRow::Auto => "auto",
        crate::edit::MonitorModeRow::Input => "input",
        crate::edit::MonitorModeRow::Off => "off",
    }
}

fn transport_json(doc: &str, t: &TransportState) -> Value {
    json!({
        "doc": doc, "playing": t.playing, "seq": t.seq, "tick": t.tick,
```

Find:
```rust
                "used_percent": m.used_percent,
            })).collect::<Vec<_>>(),
        }),
        ControlResult::Project { doc, info } => json!({
            "doc": doc,
            "rate": info.format.sample_rate,
```
Replace with:
```rust
                "used_percent": m.used_percent,
            })).collect::<Vec<_>>(),
        }),
        ControlResult::Record { doc, state: r } => json!({
            "doc": doc, "recording": r.recording, "seq": r.seq,
            "spec": r.spec.as_ref().map(|s| json!({
                "replace": s.replace,
                "punch": s.punch.as_ref().map(|p| json!({"start": p.start, "end": p.end})),
                "loop": s.looped, "count_in": s.count_in_bars, "pre_roll": s.pre_roll_bars,
                "passes_as_takes": s.passes_as_takes,
            })),
            "pass": r.pass, "kept": r.kept, "discarded": r.discarded,
            "lanes": r.lanes.iter().map(|l| json!({
                "lane": l.lane, "audio": l.audio, "path": l.path, "clipped": l.clipped,
            })).collect::<Vec<_>>(),
            "frames": r.frames, "overflows": r.overflows, "gap_frames": r.gap_frames,
            "free_bytes": r.free_bytes, "recovered": r.recovered,
        }),
        ControlResult::InputState { doc, info } => json!({
            "doc": doc, "device": info.device, "channels": info.channels,
            "names": info.names, "input_latency": info.input_latency_frames,
            "output_latency": info.output_latency_frames,
        }),
        ControlResult::Project { doc, info } => json!({
            "doc": doc,
            "rate": info.format.sample_rate,
```

Find:
```rust
                "id": l.id, "name": l.name, "destination": l.destination,
                "events": l.midi_event_count, "regions": l.region_count,
                "references": l.reference_count, "mute": l.mute, "solo": l.solo, "arm": l.arm,
            })).collect::<Vec<_>>(),
            "warnings": s.warnings,
            "alert": alert.as_ref().map(alert_json),
```
Replace with:
```rust
                "id": l.id, "name": l.name, "destination": l.destination,
                "events": l.midi_event_count, "regions": l.region_count,
                "references": l.reference_count, "mute": l.mute, "solo": l.solo, "arm": l.arm,
                "takes": l.take_count, "active_take": l.active_take,
                "input": l.input.as_ref().map(lane_input_json),
                "monitor": monitor_name(l.monitor),
            })).collect::<Vec<_>>(),
            "warnings": s.warnings,
            "alert": alert.as_ref().map(alert_json),
```

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:
```swift
        "keys.save", "keys.load", "keys.reset",
        "file.project_settings", "media.import", "media.convert",
        "media.relink", "media.compact", "media.rename", "window.media_pool",
    ]
    /// The keys that focus a transport field.
    public static let fieldKeys: [String: TransportField] = [
```
Replace with:
```swift
        "keys.save", "keys.load", "keys.reset",
        "file.project_settings", "media.import", "media.convert",
        "media.relink", "media.compact", "media.rename", "window.media_pool",
        "record.set", "record.recover", "lane.set_input", "lane.set_monitor", "midi.inject",
    ]
    /// The keys that focus a transport field.
    public static let fieldKeys: [String: TransportField] = [
```

Find:
```swift
            }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            m.media.rename(media, to: name)
        case RegionCommands.normalize:
            guard let text = args.text("peak_db") else { return .failed(.dialog("Normalize", "peak_db")) }
            guard let peak = RegionCommands.parsePeak(text) else {
```
Replace with:
```swift
            }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            m.media.rename(media, to: name)
        case "record.set":
            guard !m.record.isRecording else { return unavailable("Stop recording first.") }
            if let replace = args.flag("replace") { m.editor.updateLayout { $0.transport.overdub = !replace } }
            if let punch = args.flag("punch") { m.editor.updateLayout { $0.transport.punch = punch } }
            if let on = args.flag("loop"), on != m.looping { m.toggleLoop() }
            if let n = args.int("count_in") { m.record.countInBars = UInt8(clamping: n) }
            if let n = args.int("pre_roll") { m.record.preRollBars = UInt8(clamping: n) }
            if let on = args.flag("passes_as_takes") { m.record.passesAsTakes = on }
        case "record.recover":
            let found = (try? m.session.recoveredRecordings()) ?? []
            guard !found.isEmpty else { return unavailable("There are no recovered recordings.") }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            do {
                m.apply(try m.session.recoverRecordings(import: args.text("action") == "import"))
            } catch {
                return .failed(.thrown(error, "Recovery Failed"))
            }
        case "lane.set_input", "lane.set_monitor":
            guard let seq = args.int("seq").flatMap({ UInt64(exactly: $0) }),
                let lane = args.int("lane").flatMap({ UInt64(exactly: $0) })
            else { return .failed(.of(.invalidParams, "\(id): `seq` and `lane` name the lane")) }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            let edit: LaneEdit
            if id == "lane.set_input" {
                guard let input = ControlLanes.input(args) else {
                    return .failed(.of(.invalidParams, "lane.set_input: `kind` is none, audio or midi"))
                }
                edit = .setInput(seq: seq, lane: lane, input: input.value)
            } else {
                guard let mode = ControlLanes.monitor(args.text("mode")) else {
                    return .failed(.of(.invalidParams, "lane.set_monitor: `mode` is auto, input or off"))
                }
                edit = .setMonitor(seq: seq, lane: lane, monitor: mode)
            }
            do {
                m.apply(try m.session.edit(cmd: .lane(edit: edit), filter: nil))
            } catch {
                return .failed(.thrown(error, id == "lane.set_input" ? "Set Input Failed" : "Set Monitor Failed"))
            }
        case "midi.inject":
            guard let port = args.text("port"), let bytes = ControlLanes.hexBytes(args.text("bytes") ?? "") else {
                return .failed(.of(.invalidParams, "midi.inject: `bytes` are hex bytes such as \"90 3c 64\""))
            }
            do {
                guard try m.session.injectMidi(port: port, bytes: bytes) else {
                    return unavailable("Play or record first: this document does not hold the audio engine.")
                }
            } catch {
                return .failed(.thrown(error, "MIDI Injection Failed"))
            }
        case RegionCommands.normalize:
            guard let text = args.text("peak_db") else { return .failed(.dialog("Normalize", "peak_db")) }
            guard let peak = RegionCommands.parsePeak(text) else {
```

Find:
```swift
        case .project:
            guard let info = try? m.session.projectInfo() else { return nil }
            return .project(doc: doc, info: info)
        case .windows:
            return nil
        }
```
Replace with:
```swift
        case .project:
            guard let info = try? m.session.projectInfo() else { return nil }
            return .project(doc: doc, info: info)
        case .record:
            m.record.refresh()
            guard let s = try? m.session.recordState() else { return nil }
            return .record(doc: doc, state: s)
        case .input:
            guard let info = try? m.session.inputInfo() else { return nil }
            return .inputState(doc: doc, info: info)
        case .windows:
            return nil
        }
```

Find:
```swift
        return a
    }
}
```
Replace with:
```swift
        return a
    }
}

/// The lane commands' parameters (plan 5c Task 14): `lane.set_input`'s
/// kind and channels (counted from 1 on the channel, from 0 in the core),
/// `lane.set_monitor`'s mode, and `midi.inject`'s hex bytes.
public enum ControlLanes {
    /// A box so "no input" (nil) differs from a refusal.
    public struct Input: Equatable {
        public let value: LaneInputRow?
    }

    /// The input `args` ask for, or nil when `kind` is missing or wrong.
    public static func input(_ args: [ControlArg]) -> Input? {
        switch args.text("kind") {
        case "none":
            return Input(value: nil)
        case "audio":
            let channel = args.int("channel") ?? 1
            guard (1...256).contains(channel) else { return nil }
            return Input(value: .audio(first: UInt16(channel - 1), stereo: args.flag("stereo") ?? false))
        case "midi":
            let ch = args.int("midi_channel")
            if let ch, !(1...16).contains(ch) { return nil }
            return Input(value: .midi(port: args.text("port"), channel: ch.map { UInt8($0 - 1) }))
        default:
            return nil
        }
    }

    /// The monitor mode `text` names.
    public static func monitor(_ text: String?) -> MonitorModeRow? {
        switch text {
        case "auto": .auto
        case "input": .input
        case "off": .off
        default: nil
        }
    }

    /// `text` as bytes: hex pairs separated by spaces (`"90 3c 64"`), at
    /// most 1024; nil for anything else.
    public static func hexBytes(_ text: String) -> Data? {
        let parts = text.split(whereSeparator: \.isWhitespace)
        guard !parts.isEmpty, parts.count <= 1024 else { return nil }
        var out = Data()
        for p in parts {
            guard p.count <= 2, let b = UInt8(p, radix: 16) else { return nil }
            out.append(b)
        }
        return out
    }
}
```

- [ ] **Step 4: Run the tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control 2>&1 | grep -E "^test result|FAILED"
cargo test -p core-ffi --test control 2>&1 | tail -3
```
Expected: both green; the registry count test reads 124.

- [ ] **Step 5: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo test -p core-control 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: no warnings, every `test result: ok`; Swift: 765 tests in 91 suites, `everyRegisteredMenuCommandHasAHandler` among them..

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/src/request.rs crates/core-ffi/src/control.rs app/Sources/VizDAWAppModel/ControlCommands.swift crates/core-control/tests/commands.rs crates/core-ffi/tests/control.rs app/Tests/VizDAWAppModelTests/RecordModelTests.swift
git commit -F - <<'VIZDAW_MSG'
control: record, lane input, monitor and MIDI injection commands

record.set, record.recover, lane.set_input, lane.set_monitor and
midi.inject check their params by type and range; state.get record and
input report the recording and the device's input; document lanes
gain their takes, input and monitor.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

### Task 15: End to end, and the by-eye checklist

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs` (`records_audio_loop_takes_and_midi`)
- Modify: `app/README.md` (steps 129–137, "Recording (plan 5c)")

**Interfaces:**
- Consumes: everything above.

The e2e runs the app with `VIZDAW_AUDIO=offline` and `VIZDAW_AUDIO_INPUT=fake` (never a real device): it arms the fake-input lane "Mic" and records until the one-bar sequence ends, so the pass ends by itself (Review Focus 2) and lands as one step "Record" with a `recorded` media item; reads `state.get input` (three fake channels); loop-records with a keep after pass 1 and a discard after pass 2, leaving two new takes; then arms "Keys" and records a note sent with `midi.inject`. The README steps are for the user only (real input, the aggregate, the microphone prompt).

- [ ] **Step 1: Write the end-to-end test**

In `crates/vizctl/tests/e2e.rs`:

Find:
```rust

impl App {
    fn start() -> App {
        let app = std::env::var("VIZDAW_E2E_APP")
            .expect("set VIZDAW_E2E_APP to VizDAW.app or its executable");
        // Cargo runs tests in the crate's folder: a relative path is
```
Replace with:
```rust

impl App {
    fn start() -> App {
        App::start_with(&[])
    }

    /// [`App::start`] with more environment (plan 5c: the fake input).
    fn start_with(extra: &[(&str, &str)]) -> App {
        let app = std::env::var("VIZDAW_E2E_APP")
            .expect("set VIZDAW_E2E_APP to VizDAW.app or its executable");
        // Cargo runs tests in the crate's folder: a relative path is
```

Find:
```rust
            home: Some(home),
            demo: false,
            timeout: Duration::from_secs(180),
            env: vec![
                ("VIZDAW_AUDIO".into(), "offline".into()),
                ("VIZDAW_TRASH_DIR".into(), trash),
            ],
        })
        .unwrap_or_else(|e| panic!("launch: {e}"));
        let client = Client::connect(&launched.socket).unwrap();
```
Replace with:
```rust
            home: Some(home),
            demo: false,
            timeout: Duration::from_secs(180),
            env: [
                ("VIZDAW_AUDIO".to_string(), "offline".to_string()),
                ("VIZDAW_TRASH_DIR".to_string(), trash),
            ]
            .into_iter()
            .chain(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())))
            .collect(),
        })
        .unwrap_or_else(|e| panic!("launch: {e}"));
        let client = Client::connect(&launched.socket).unwrap();
```

Find:
```rust
    }
    app.quit();
}
```
Replace with:
```rust
    }
    app.quit();
}

/// Polls `state.get record` until `ok` holds; returns it.
fn wait_for_record(app: &mut App, window: &Value, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let r = app.call("state.get", json!({"target": "record", "window": window}));
        if ok(&r) {
            return r;
        }
        assert!(
            Instant::now() < deadline,
            "record state within {WAIT_MS} ms: {r}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The lane named `name` in `state.get document`.
fn lane_named(app: &mut App, window: &Value, name: &str) -> Value {
    let doc = app.call("state.get", json!({"target": "document", "window": window}));
    doc["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == name)
        .unwrap_or_else(|| panic!("no lane {name} in {doc}"))
        .clone()
}

/// README steps 130, 134 and 137 through the channel (plan 5c): the offline
/// device's fake input records onto an armed lane as one undo step
/// "Record" with a recorded media item; loop recording with a keep after
/// pass 1 and a discard after pass 2 leaves two new takes; MIDI injected
/// into an armed lane while recording lands as a note. No audio device
/// is opened and no microphone permission is asked.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn records_audio_loop_takes_and_midi() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
    use core_model::ids::AnyId;
    use core_model::lane::{InputChannels, LaneInput, MonitorMode};
    use core_model::sequence::SeqLength;
    use core_model::tempo::{Bpm, TempoMap, TempoPoint};
    use core_model::time::Tick;
    let mut app = App::start_with(&[("VIZDAW_AUDIO_INPUT", "fake")]);
    // One bar at 240 BPM (a second): "Keys" (MIDI from source "Keys" on
    // the built-in synth) and "Mic" (fake input 1, heard never).
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, keys) = b.sequence("Take", "Keys", Some(synth)).unwrap();
    let mic = b.lane(seq, "Mic", None).unwrap();
    let r = b
        .run(Command::Routing(RoutingCmd::AddOutput {
            name: "Audio 1".into(),
            reference: "audio:1-2".into(),
        }))
        .unwrap();
    let out = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
        .unwrap();
    for c in [
        LaneCmd::SetOutput {
            seq,
            lane: mic,
            output: Some(out),
        },
        LaneCmd::SetInput {
            seq,
            lane: keys,
            input: Some(LaneInput::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
        LaneCmd::SetInput {
            seq,
            lane: mic,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(0),
            }),
        },
        LaneCmd::SetMonitor {
            seq,
            lane: mic,
            monitor: MonitorMode::Off,
        },
    ] {
        b.run(Command::Lane(c)).unwrap();
    }
    b.run(Command::Sequence(SequenceCmd::SetTempo {
        seq,
        tempo: TempoMap::new(vec![TempoPoint {
            tick: Tick(0),
            bpm: Bpm::whole(240),
            ramp: false,
        }])
        .unwrap(),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(4 * Q)),
    }))
    .unwrap();
    let path = app.launched.home.join("Rec.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));

    // Audio: arm Mic (none armed: the last lane), record to the end.
    app.call(
        "command.run",
        json!({"id": "transport.arm_previous", "window": w}),
    );
    assert_eq!(lane_named(&mut app, &w, "Mic")["arm"], true);
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"replace": true, "loop": false, "pre_roll": 0}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == true);
    // The sequence ends after a second: the pass ends by itself.
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let m = app.call("state.get", json!({"target": "media", "window": w}));
    let item = m["items"][0].clone();
    assert_eq!(item["origin"], "recorded", "{m}");
    assert!(
        item["path"]
            .as_str()
            .unwrap()
            .starts_with("Media/Recorded/Mic "),
        "{item}"
    );
    assert_eq!(
        (item["offline"].clone(), item["rate"].clone()),
        (json!(false), json!(48_000))
    );
    assert_eq!(lane_named(&mut app, &w, "Mic")["regions"], 1);
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(input["channels"], 3, "{input}");

    // Loop: keep after pass 1, discard after pass 2, stop in pass 3.
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"loop": true, "passes_as_takes": true}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["pass"].as_u64() >= Some(2));
    app.call(
        "command.run",
        json!({"id": "record.keep_pass", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["pass"].as_u64() >= Some(3));
    app.call(
        "command.run",
        json!({"id": "record.discard_pass", "window": w}),
    );
    let r = app.call("state.get", json!({"target": "record", "window": w}));
    assert_eq!(
        (r["kept"].clone(), r["discarded"].clone()),
        (json!(1), json!(1)),
        "{r}"
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let lane = lane_named(&mut app, &w, "Mic");
    assert_eq!(
        (lane["takes"].clone(), lane["active_take"].clone()),
        (json!(3), json!("Take 3")),
        "{lane}"
    );

    // MIDI: arm Keys instead, record, play a note in.
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"loop": false}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.arm_previous", "window": w}),
    );
    assert_eq!(lane_named(&mut app, &w, "Keys")["arm"], true);
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| {
        r["recording"] == true && r["frames"].as_u64() >= Some(2_400)
    });
    app.call(
        "command.run",
        json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": "90 3c 64"}, "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["frames"].as_u64() >= Some(9_600));
    app.call(
        "command.run",
        json!({"id": "midi.inject", "params": {"port": "Keys", "bytes": "80 3c 00"}, "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    assert_eq!(lane_named(&mut app, &w, "Keys")["events"], 1);
    app.quit();
}
```

- [ ] **Step 2: Implement**

In `app/README.md`:

Find:
```markdown
     system colours. With VoiceOver, regions read "Region <name> on
     <lane>, bar n, gain …"; the Relink sheet's buttons read "Locate
     <name>", "Search Folder…" and "Leave Offline".
```
Replace with:
```markdown
     system colours. With VoiceOver, regions read "Region <name> on
     <lane>, bar n, gain …"; the Relink sheet's buttons read "Locate
     <name>", "Search Folder…" and "Leave Offline".

### Recording (plan 5c)

These steps use real hardware, so they are by eye: an audio interface
with a microphone or a line source, a MIDI keyboard, and a loopback
cable (an output wired back to an input). Use a throwaway home
(`vizctl launch`) for the channel steps. Plan 5d adds the Record button,
the lane input pickers and meters; until then arm and set inputs through
the channel (`$V run lane.set_input …`) or Transport > Arm Next Lane,
and record with Transport > Record (R).

129. Microphone permission: in a fresh home, set a lane's input to audio
     channel 1 (`$V run lane.set_input '{"seq": 1, "lane": 2, "kind":
     "audio", "channel": 1}'`) and arm it: macOS asks for the
     microphone once ("VizDAW records audio from your audio interface…").
     Deny it, then press Record: the transport's faults say "VizDAW may
     not use audio input. Allow it in System Settings › Privacy &
     Security › Microphone…" and the input stays closed. Allow it in
     System Settings and arm again: input opens.
130. Record and monitor: with the interface as output and input, arm
     an audio lane (monitor Auto): stopped, you hear the input through
     the lane's strip; play without recording: you hear playback only;
     press R: you hear the input while recording, the transport rolls,
     and Stop (or R again) leaves one region on the lane and one undo
     step "Record". The media pool lists "<lane> <date> <time>"
     (Recorded, the project's rate and depth). Set the monitor to Input
     (`$V run lane.set_monitor '{"seq": 1, "lane": 2, "mode":
     "input"}'`): always heard; Off: never.
131. Loopback calibration: wire output 1 to input 1, put a region with
     a sharp click on one lane and record input 1 on another with its
     monitor Off. Zoom into the recorded region: its click starts within
     2 samples of the original's. If it does not, the interface
     misreports its latency: `set_record_latency_trim` (Settings ›
     Recording in plan 5d) moves it; record again to check.
132. Separate input device: choose another device for input
     (`set_audio_input_device`, Settings › Audio in plan 5d), e.g. the
     built-in microphone while playing through the interface: recording
     works, playback stays on the interface, and Audio MIDI Setup shows
     no extra device (the aggregate is private). Quit: nothing is left
     behind.
133. MIDI keyboard: set a lane's input to MIDI from your keyboard
     (`"kind": "midi", "port": "<its name>"`), arm it: what you play
     sounds on the lane's instrument at once (thru), stopped or
     playing. Record a phrase: the notes land where you played them
     against what you heard, not a buffer late. Overdub (transport >
     Overdub) merges a second pass; Replace with Punch on clears only
     the punch range.
134. Loop passes: turn Loop on and record over a short sequence for
     three passes; after the first pass run `$V run record.keep_pass`,
     after the second `$V run record.discard_pass`, then Stop: the lane
     has two new takes ("Take 2", "Take 3"), the last plays, and one
     Undo removes both.
135. Crash recovery: record a few seconds into a saved project, then
     kill VizDAW (`kill -9`). Reopen the project: "Recovered
     Recordings" offers the file; Import into New Lanes adds a lane
     with it at bar 1 (one undo step "Recover Recordings"); repeat and
     choose Move to Trash: the file is in the Finder's Trash; choose
     Later: it is offered again next time and Save never trashes it.
136. Unplug mid-recording: record from a USB interface and pull its
     cable: within a moment recording stops with "Recording stopped: the
     audio device went away; what was recorded is kept." and the region
     holds what was recorded. A full disk stops the same way ("the disk
     is almost full"); below 1 GB a warning shows first.
137. Channel: `$V state record` while recording shows `recording`, the
     `pass`, `frames` and each lane's file; `$V state input` names the
     device's input channels and latencies; `$V run midi.inject
     '{"port": "Keys", "bytes": "90 3c 64"}'` plays into lanes listening
     to "Keys"; malformed bytes are `invalid_params`. The e2e test
     `records_audio_loop_takes_and_midi` runs steps 130, 134 and 137
     against the offline device's fake input
     (`VIZDAW_AUDIO=offline VIZDAW_AUDIO_INPUT=fake`), which opens no
     hardware and asks for no permission.
```

- [ ] **Step 3: Run the gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep -E "^test result|FAILED|panicked"
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
scripts/make-app.sh
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1 2>&1 | tail -3
pgrep -lf MacOS/VizDAW || echo "no VizDAW running"
rm -rf /tmp/vze*
```
Expected: no warnings, every `test result: ok`; Rust 1177 passed, 0 failed, 11 ignored (the e2e file's new ignored test); Swift 765 tests in 91 suites; e2e 7 passed; no VizDAW left running..

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl/tests/e2e.rs app/README.md
git commit -F - <<'VIZDAW_MSG'
e2e: record audio, loop takes and MIDI from the fake input

The channel arms a lane on the fake input and records until the song
ends the pass, loop-records with a keep and a discard leaving two
takes, and records injected MIDI, each as one Record step. README
steps 129-137 check real input, the aggregate and the microphone by
eye.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
VIZDAW_MSG
git log -1 --oneline && git show --stat HEAD
```

---

## Self-review

**Spec coverage (5c row of §13):**

| Spec | Task |
|---|---|
| §4.5 lane `input`, `InputChannels`, `monitor` | 2 (model), 8 (FFI), 14 (channel) |
| §4.5 `return_of`, §4.5a, §6.7 synth tracking | follow-up plan 5c-2 (Ruling 1) |
| §4.7 recording pass, history group | 3, 9 |
| §6.1 HAL input, duplex, private aggregate, input only when needed, microphone | 11 |
| §6.2 input ring, writer, overflow as silence, clip counts, disk checks | 4, 5, 9 |
| §6.3 latency compensation, MIDI at the audible time, trim | 6, 7, 11 |
| §6.4 record modes, arming, pre-roll, range, replace/overdub, loop passes, keep/discard, takes, step-entry refusal | 7, 9, 13 |
| §6.4 count-in | refused until 5d (Ruling 11) |
| §6.5 finishing, recorded media, undo, crash recovery | 9, 10 |
| §6.6 monitoring, MIDI thru; §6.8 input chain hook | 6 |
| §12.1 commands and the fake input | 5, 8, 13, 14 |
| §12.2 `record_overflow`, `input_lost` | 9 (`state.get record` overflows; the pass ends with a message) |
| §12.3 recording tests, `rt-check`, e2e, checklist | 3–10, 15 |

**Review Focus:** each item names its tests, and each named test is in a task's Step 1 file or excerpt.

**Placeholders:** none. Every file a task changes is given whole or as exact Find / Replace excerpts of the tree the previous task left.

**Types:** names used across tasks (`RecordPass`, `Mark`, `RecordedMidi`, `RecordRequest`, `InputSpec`, `MidiSender`, `Captured`, `RecordSpecRow`, `RecordStateRow`, `LaneInputRow`, `MonitorModeRow`, `InputInfoRow`, `RecordModel`) are spelled the same in every task that uses them; the code in this plan was replayed task by task on a scratch copy of `98931a8` with every gate green.

**Follow-ups:**
- **Plan 5c-2 — Synth tracking** (Ruling 1): `LaneInput::Audio.return_of`, `return_latency_ms` and `destination.set_return_latency`, the alignment shift in the planner, `destination.measure_return`, the fake synth on fake input channel 3 (`VIZDAW_FAKE_SYNTH`), `lane.track_synth`, and spec §12.3's synth-tracking tests and e2e.
- **Plan 5d** (Ruling 23): the record controls, meters and clip marks, count-in, Settings › Audio (input device) and Recording (latency trim).
