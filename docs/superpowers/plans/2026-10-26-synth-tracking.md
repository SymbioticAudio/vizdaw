# VizDAW 5c-2 — Synth Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A hardware synth is printed into the project as audio in one Record pass: Track Synth sets up an armed audio lane that records the synth's return (`return_of`), the synth's destination carries a measured or typed return latency, and the recorded region lands on the MIDI notes that played it within ±1 ms; the offline device's fake synth makes all of it testable without hardware.

**Architecture:** Everything builds on 5c's recording engine; the real-time side only sends, copies and compares.
- **`core-model`**: `LaneInput::Audio.return_of: Option<DestId>`, `Destination.return_latency_ms: f32` (0–500), `DestKind::is_external`, `RoutingCmd::SetReturnLatency` and `LaneCmd::TrackSynth` (one undo step: the return lane, its input, output and arm).
- **`core-engine`**: the fake synth (`FakeSynth`: notes sent to the MIDI destination "Fake Synth" play a click and a tone on fake input channel 3, the synth's delay after their audible time) and a stopped-transport measurement (`MeasureTap`: five pings to the synth's target, each onset found on the return input); the engine interns the return lanes' destinations at every load.
- **`core-ffi`**: the pass planner moves a `return_of` lane's audio earlier by its synth's return latency (and, carried from 5c, cuts every earlier piece a latency fall is heard over); the host wires the fake synth and runs a measurement with the transport stopped; `Session.track_synth`, `set_return_latency` and `measure_return`; "mute the MIDI lane after recording" joins the pass's "Record" step.
- **`core-control` and Swift**: `lane.track_synth`, `destination.set_return_latency`, `destination.measure_return`, `lane.set_input`'s `return_of`, `state.get document`'s `input.return_of`; `SynthCommands` in `VizDAWAppModel` (the Track Synth sheet and lane menu are plan 5d's).

**Tech Stack:** Rust 2024 (`core-model`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5c-2 — Synth tracking":
- §4.5 `LaneInput::Audio.return_of`; §4.5a destinations' `return_latency_ms` (default 0, range 0–500, set by hand or measured, undoable);
- §6.7 tracking a hardware synth: setup (Track Synth), one pass, alignment (§6.3 compensation plus the `return_of` shift), Measure (with the stopped-transport capture), monitoring, "mute the MIDI lane after recording";
- §12.1 `lane.set_input` (including `return_of`), `lane.track_synth`, `destination.set_return_latency`, `destination.measure_return`, `state.get` per lane `input.return_of`, the fake synth (`VIZDAW_FAKE_SYNTH=<delay ms>`);
- §12.3 the "Synth tracking" tests and the e2e line "track the fake synth"; the manual checklist's "tracking a real hardware synth with Measure".

Read the whole spec (its §3 rules, §6.3 and §6.4 bind here) and plan 5c (`docs/superpowers/plans/2026-10-20-recording-engine.md`, whose Ruling 1 split this plan off). The 5c ledger is `.superpowers/sdd/2026-10-20-recording-engine/progress.md`; its final review (`final-review.md` there) carries two items into this plan (Tasks 1 and 6).

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root.** `scripts/build-ffi.sh` (after any Rust change, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`. Swift tests run only via `scripts/swift.sh test` from the repo root (a bare `swift test` fails with an XCBuild plist error). Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` (HEAD `2bc68ed` before this plan's commit, plus that commit). Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`). No manifest changes here, so `Cargo.lock` never changes. Never commit `.superpowers/`, `.DS_Store` or generated files.
  - End every commit message with the two attribution lines your dispatch gives you. The blocks below show:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Use the `Co-Authored-By` line naming the model you actually are. The commit identity is the repo's own (`git config user.name` is set locally).
- **Clean room.** Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any research material. No Opcode, Midas or Steinberg assets, names, artwork or branding anywhere.
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e test (Task 11) or `vizctl launch` with `VIZDAW_AUDIO=offline`; quit it when done (`vizctl quit`; `pkill -f "VizDAW --control"` if a test left one). At the end of a task that launched the app, `pgrep -lf MacOS/VizDAW` prints nothing; remove the `/tmp/vze*` homes your e2e runs left.
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any audio device's settings. Tests use temp directories; sessions that save get a trash folder (`Session::debug_set_trash_dir`); the e2e sets `VIZDAW_TRASH_DIR` inside its throwaway home.
- **Tests never open real audio input and never trigger the microphone prompt.** Every `core-ffi` test that plays, records or measures calls `vizdaw_ffi::host::force_offline()` first and chooses its fake input with `host::set_fake_input` and its fake synth with `host::set_fake_synth` (Task 7), never the environment. Test builds (`test-hooks`) never open live input (`live_input_allowed` is false there). The e2e runs with `VIZDAW_AUDIO=offline` and `VIZDAW_FAKE_SYNTH=25`. Real hardware is checked by eye, by the user, through `app/README.md` steps 140–142 (Task 11); implementers never run them.
- **Test waits.** Every `core-ffi` integration test that waits for a recording, a measurement or the engine polls with a deadline from `common::wait_limit()` (`crates/core-ffi/tests/common/mod.rs`, which reads `VIZDAW_TEST_WAIT_SECS`), never a fixed sleep for the result itself. The e2e polls with its `WAIT_MS` conditions (the file's existing convention).
- **Crates and dependencies.** Rust stable, edition 2024. No new crate and no new dependency. License GPL-3.0-only.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. New non-test Rust code uses `.get()`, iterators and `checked_*`/`saturating_*`, never indexing or unwraps.
- **Platform-neutral cores.** `core-model`, `core-engine` and `core-control` stay free of OS APIs.
- **Real-time rules (spec §3).** The real-time thread never allocates, frees, locks or does file I/O. Everything new on it is marked `// RT`: `FakeSynthOut::send`, `SynthQueue::push`/`peek`/`pop`, `SynthVoice::render`, `fill_fake`, `MeasureTap::sends`/`capture`/`release`/`close`, `Renderer::measure_sends` and the measurement's part of `end_tap`. A measurement arrives as a boxed engine command and leaves through the retire ring. **rt-check gates** (Tasks 4, 5, 6 and 11): `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record`.
- **Media is write-once.** Recordings still go to new files under `Media/Recorded/` (`CafWriter::create` never overwrites); nothing here opens a media file for writing. The return shift only moves where a region points into its file.
- **File compatibility.** `core_model::file::VERSION` stays 3. `LaneInput::Audio.return_of` and `Destination.return_latency_ms` are written only when set (`serde(default, skip_serializing_if)`), so projects that use neither read and write byte-identically (Ruling 1).
- **FFI safety.** No panic crosses the FFI: every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard` (or `transport_call`). Measure waits with the session lock released.
- **UniFFI changes.**
  - Changed: `LaneInputRow::Audio` gains `return_of: Option<u64>` (no default; the Swift literals change in Task 8); `DestRow` gains `external: bool` and `return_latency_ms: f32`, both `#[uniffi(default)]` (it loses `Eq`).
  - New: record `MeasureRow` (Task 9); `Session.track_synth`, `Session.set_return_latency` (Task 8), `Session.measure_return` (Task 9); `ControlResult::Measured` (Task 10, appended).
  - `FakeSession` (`app/Tests/VizDAWAppModelTests/FakeSession.swift`) implements each new `Session` method in the task that adds it.
- **Swift.** Generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings. No SwiftUI `@State`/`@FocusState`. No new views (the Track Synth sheet is 5d's). New model tests go to `VizDAWAppModelTests`.
- **Control channel** (control spec §6, §7). Programmatic calls never open dialogs: no new command has one. Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`; the Swift test `everyRegisteredMenuCommandHasAHandler` compares them. The registry count test moves from 124 to 127 in Task 10. Every new command, and `lane.set_input`'s new `return_of`, gets hostile-input tests: by type and range in `core-control` (`synth_commands_check_their_params`), in the app (`returnLatencyIsTextMillisecondsFrom0To500`, `laneSetInputCarriesReturnOf`, `trackSynthAsksTheCoreWithChannelsFromOne`) and in the cores (`return_latency_is_an_undoable_edit_of_external_destinations_only`, `track_synth_refuses_what_it_cannot_set_up`, `hostile_return_latencies_are_refused`, `measure_refuses_what_it_cannot_measure`).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test` for the crates the task touches (the workspace in Tasks 2, 8 and 11).
  - Tasks 4, 5, 6 and 11 also run the rt-check gates.
  - Tasks 7–11 also run `cargo clippy -p core-ffi --lib -- -D warnings` (code under `cfg(not(feature = "test-hooks"))` is linted only by it), `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 11 also runs `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests: `core-ffi` catalog tests under parallel load, the `damaged_files` proptest, the MIDI listener tests (`platform-macos/tests/midi.rs`) and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - Baseline at `2bc68ed`: Rust 1273 passed, 0 failed, 11 ignored (plus the three rt tests); Swift 782 tests; e2e 7 tests.
- **How code is given.**
  - New files are written whole (`cat > … <<'VIZDAW_EOF'`).
  - Changes to existing files are exact **Find / Replace** excerpts of the tree the previous task left, applied in order with an editor (the Edit tool). Each Find occurs exactly once in the file when it is applied.
  - This plan was written against the tree at `2bc68ed` and was not replayed in a scratch worktree. If an excerpt does not match, make the same change to the current text; if a name differs from the tree (a helper, a field), use the tree's; if the compiler finds a slip (a missing import, a borrow), fix it the obvious way. Say so in the report in each case.
- **Spec values used verbatim:**
  - "`return_latency_ms: f32`, default 0, range 0–500"; "Like other destination edits, it is undoable";
  - "It creates or reuses that audio lane, named "<D name> audio", just below"; "It arms the audio lane"; "It leaves the MIDI lane playing. Optionally, the MIDI lane is armed too";
  - "For a `return_of` lane, the recorded region is additionally shifted earlier by D's `return_latency_ms`"; "The residual target is within ±1 ms once the latency is measured";
  - "It sends 5 short notes (pitch 60, velocity 100, 500 ms apart) to D with the transport stopped"; "the first sample above −40 dBFS after the note"; "It sets `return_latency_ms` to the median delay, minus the round-trip latency the HAL reports"; "It fails with a message ("No signal on the synth's return input") if fewer than 3 onsets are found within 500 ms of their notes";
  - "Mute the MIDI lane after recording" (off by default); "If chosen, the mute is part of the pass's undo step";
  - "`VIZDAW_FAKE_SYNTH=<delay ms>` adds a fake external MIDI destination, "Fake Synth". Every note-on sent to it renders a 5 ms click on fake input channel 3 that many milliseconds after the note's audible time, and note-off ends a 220 Hz tone started with it";
  - "the clicks land on the MIDI note starts within ±1 ms, with `return_latency_ms` set to the fake delay"; "Measure finds the fake delay within ±1 ms and fails cleanly with no signal"; "Track Synth sets up and arms in one undo step"; "a loop pass makes MIDI and audio takes together".

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out. Each is pinned by named tests in its owning task.

1. **Measure on a return that rings, hums or is the wrong input** (a pad's long release into the next note, a noisy channel above −40 dBFS, the mic input chosen by mistake). Expected: no false "0 ms"; the measurement fails cleanly with "No signal on the synth's return input" and changes nothing. Pinned by `an_input_that_never_falls_quiet_finds_no_onset`, `a_silent_return_finds_no_onset` (Task 5), `measured_needs_three_onsets_and_stays_in_range` and `measure_fails_cleanly_without_a_clean_onset` (Task 9).
2. **Measure while something else uses the engine** (the transport playing, a recording, another window playing, a second Measure, Play or Record pressed mid-measure). Expected: `Busy` with a message, the other activity untouched, and nothing left installed afterwards (Play works again). Pinned by `measure_refuses_what_it_cannot_measure` and `play_and_record_wait_for_a_measurement` (Task 9).
3. **The synth or its lanes changing after Track Synth** (the destination removed or replaced, turned into a plugin, Track Synth run twice, run on a lane that plays no external synth). Expected: the return lane follows an external replacement or is unlinked, is reused rather than duplicated, refusals change nothing, and a recording never shifts by a latency the lane no longer has. Pinned by `removing_the_synth_relinks_or_unlinks_its_return_lane` (Task 2), `track_synth_reuses_the_return_lane_and_can_arm_the_midi_lane`, `track_synth_refuses_what_it_cannot_set_up` (Task 3), `a_return_shift_needs_an_external_destination` (Task 8).
4. **A return latency at the edges** (500 ms, longer than the take, a take that starts at the record start, a resync just before a latency fall). Expected: regions never reach outside their file or before the record start, and no file frame is placed twice. Pinned by `a_return_lane_lands_earlier_by_its_return_latency`, `a_return_latency_longer_than_the_take_places_nothing`, `a_latency_fall_cuts_every_piece_it_is_heard_over` (Task 1).
5. **Hostile or careless channel input** (`ms` as "abc", "-1", "500.5", "nan", "inf", 12.5 as a number, 100 000 characters; an unknown or non-external `dest`; `channel` 0 or 257; `return_of` on a MIDI input or negative). Expected: `invalid_params` (or the core's `Model` refusal) before anything changes. Pinned by `synth_commands_check_their_params`, `returnLatencyIsTextMillisecondsFrom0To500`, `laneSetInputCarriesReturnOf` (Task 10), `hostile_return_latencies_are_refused` (Task 8), `return_latency_is_an_undoable_edit_of_external_destinations_only` (Task 2).

## Plan Rulings

Decisions the spec leaves open, in the format "what — why — cost if wrong".

1. **Ruling (file format):** `return_latency_ms` is a field of `Destination` (spec §4.5a says the destination gains it), `f32` milliseconds, written only when not 0; `return_of` is a field of `LaneInput::Audio` (spec §4.5), written only when set. `VERSION` stays 3, as plan 5c Ruling 2 did for `Lane.input`.
   - Cost if wrong: a 5c build opening a project that uses them reports it damaged (an unknown field) rather than "made by a newer VizDAW".
2. **Ruling (external destinations):** an external MIDI destination is a `DestKind::MidiEndpoint` that is not the built-in synth (`DestKind::is_external`); VizDAW's virtual output counts (it feeds another app that may return audio). `SetReturnLatency`, `return_of`, Track Synth and Measure refuse every other destination with a `Model` error.
   - Cost if wrong: a rule change in one function.
3. **Ruling (links that go stale):** `LaneCmd::SetInput` refuses a `return_of` that is not an existing external destination. `RemoveDestination` does not count a `return_of` as a use: with a replacement that is external the link follows it, otherwise the link is cleared (the lane keeps recording its channels, unshifted). `EditDestination` turning the synth into a plugin leaves the link, and the shift is 0 while the destination is not external. A file whose `return_of` names no destination, or whose return latency is outside 0–500 or not finite, is damaged.
   - Why: removing a synth must not be blocked by a recording setup; a link to nothing is harmless but misleading.
   - Cost if wrong: a removed synth's lane needs Track Synth again.
4. **Ruling (the shift):** a `return_of` lane's shift is read when Record starts: `round(return_latency_ms × project rate / 1000)` frames, while its destination is external (else 0). The planner places that lane's file frames with `D + R` instead of `D` (spec §6.3's `D` is the input delay minus the trim, plus the graph latency per piece), so the region moves earlier by `R`; MIDI recorded in the same pass does not move. Regions still never reach outside their file or before the record start.
   - Cost if wrong: an edit of the latency during a pass applies from the next pass.
5. **Ruling (Track Synth):** `LaneCmd::TrackSynth { seq, lane, channels, arm_midi }` is one undo step "Track Synth". It refuses a lane with no destination or a destination that is not external. It reuses the first lane of the sequence (other than the MIDI lane) whose input records that synth's return, else inserts "<D name> audio" just below the MIDI lane. It sets the lane's input to `Audio { channels, return_of: Some(D) }`, gives it an output path when it has none (the document's first, else a new "Audio 1", as 5a Ruling 6 does for a first region), arms it and leaves its monitor as it is (a new lane is Auto). It sets the MIDI lane's arm to `arm_midi`: false (the default) **disarms** it, so a Replace pass never clears the part that plays the synth.
   - Cost if wrong: one more click to arm the MIDI lane.
6. **Ruling (mute after recording):** "Mute the MIDI lane after recording" is session state, not document state: `Session.track_synth(…, mute_after)` remembers (return lane → MIDI lane) until the first pass that places audio on that return lane; that pass's mute runs inside the recording's history group, after `Command::Record`, so it is part of the one "Record" step. Track Synth again without it forgets it. It is not saved.
   - Cost if wrong: after reopening a project the option must be chosen again.
7. **Ruling (the fake synth):** offline hosts only. `VIZDAW_FAKE_SYNTH=<ms>` (0–500, else ignored) adds the MIDI destination port "Fake Synth"; a document reaches it through a `MidiEndpoint` destination whose endpoint is "Fake Synth". It plays on channel 3 of the signals input, which it brings when no fake input is chosen; with the loopback input it stays silent. Each note-on starts a 5 ms click at 0.5 (−6 dBFS) and, while any note is held, a 220 Hz tone at 0.1 (−20 dBFS); all-notes-off (CC 120/123) ends the tone. The delay is at least one block (the loopback's rule, plan 5c Ruling 7): a note is sent during the block it plays in, so an earlier answer could not be heard in time. Events play in the order they were sent. The fake device's output latency is 0, so a note's audible frame is its host time in frames.
   - Cost if wrong: only the fake device.
8. **Ruling (Measure, timing):** the first note goes 100 ms after the measurement starts, then one every 500 ms, each 100 ms long, pitch 60, velocity 100, on the destination's channel, sent at the start of the block it falls due in; its heard run is that block's run plus the graph latency and the MIDI offset. Note *i*'s window on the return input is the 500 ms from its heard run plus the device's round trip (the input delay `D`, as recording uses it). Its onset is the first sample (of either channel of a pair) above 0.01 (−40 dBFS) in the window that follows at least 5 ms at or below 0.01: a release ringing on from the note before, a hum or a never-quiet input finds no onset. The result is the median (the mean of the middle two for an even count) of `onset − heard − (D − trim)` over the notes that found one, in ms at the engine's rate, clamped to 0–500; fewer than 3 onsets fails "No signal on the synth's return input". A success is applied as `RoutingCmd::SetReturnLatency`, one undo step "Set Return Latency".
   - Why the 5 ms quiet rule: the spec's "first sample above −40 dBFS after the note" alone reads a ringing tail as a 0 ms synth.
   - Cost if wrong: a synth whose release never falls quiet within 500 ms cannot be measured (it can be typed).
9. **Ruling (Measure, the engine):** Measure listens on the channels of the first lane (sequences in ID order, lanes in order) whose input records the destination's return; none: `Model` "no lane records the return of <D>: set one up with Track Synth first". It claims the engine as Play does, but is `Busy` while any session plays or records, before anything is replaced; it loads that lane's sequence with the transport stopped, opens the input (a channel the input lacks is refused as Record refuses it, with the same denied or failed-open reason) and installs the measurement. While it runs, Play, Record and Measure from any session are `Busy` ("A synth's return latency is being measured; try again in a moment."). `Session.measure_return` blocks its caller for about 2.7 s (at most 10 s, `MEASURE_WAIT`) with the session lock released; plan 5d's sheet must call it off the main thread. Recording or an open edit in the session itself is `Busy`.
   - Cost if wrong: the control channel's call blocks the app's main thread for those seconds.
10. **Ruling (return destinations in the engine):** `Engine::set_return_dests` names the destinations the loaded sequence's lanes record the return of; every load interns their targets like thru's, so Measure can ping a synth the sequence never plays. The host sets them at every load.
    - Cost if wrong: none (a MIDI-destination target is never released).
11. **Ruling (channel names):** `lane.track_synth { seq, lane, channel? (1–256, default 1), stereo?, arm_midi?, mute_after? }`: the spec's `channels` is spelled as `lane.set_input` spells it, and `seq` is added as every lane command has it. `destination.set_return_latency { dest, ms }` takes `ms` as text ("12.5", 0–500), because the protocol has no number kind (as `region.normalize`'s `peak_db`). `destination.measure_return { dest }` answers `{doc, dest, return_latency_ms, onsets}`. `lane.set_input` gains `return_of` (a destination ID, audio only). `state.get document`'s lane `input` gains `return_of` (null when none). Key-only, groups "Lane" and "Destination". The registry goes from 124 to 127.
    - Cost if wrong: aliases later.
12. **Ruling (reading return latencies back):** no destination list is added to `state.get`; `DestRow` gains `external` and `return_latency_ms` for 5d's sheet, and the channel reads a latency back through `destination.measure_return`'s answer or the saved project.
    - Cost if wrong: a `state.get destinations` target later.
13. **Ruling (5c final review carry a):** where the graph latency falls, the newer piece is heard over the tail of every earlier piece that reaches past its start, not only the one just before; each is cut there. A latency rise is unchanged.
    - Cost if wrong: none (pure planner, tested).
14. **Ruling (Track Synth while recording):** `Session.track_synth` is `Busy` while a pass records, finishes or waits to apply (arming changes mid-pass are 5d's question, 5c final review M6). `set_return_latency` is allowed (the shift is read at Record start).
    - Cost if wrong: a refusal message.

## Carried and parked items

Picked up by this plan:
- **5c final review, new Minor (carry a):** `passes.rs` `pieces()` latency-fall branch cuts only the preceding piece — Task 1 walks back and cuts every earlier piece it overlaps (Ruling 13); `a_latency_fall_cuts_every_piece_it_is_heard_over`.
- **5c final review, judgement 1 (carry b):** `rt_alloc_record` exercises a real latency change — Task 6 swaps in a graph with a latent test insert mid-recording, and back, and asserts the marks carried the new latency.
- **5c ledger "5c-2 carry"**: the latency e2e bounds stay as they are unless it flakes (it is not touched here).

Not this plan's (listed so reviewers do not raise them):
- 5c final review I2, M5, M6, M8, the T11/T12 carries, PDC M4, README 131/132 and the growing waveform: plan 5d.
- **No way to make a MIDI-endpoint destination in the app.** Neither the FFI nor the UI can add an external MIDI destination today (only plugin instruments and the built-in synth); tests and the e2e build their documents with `Builder::dest`. 5d's Track Synth menu needs one (an instrument-list "New MIDI Destination…" listing CoreMIDI destinations). Raised to the owner in the writer report; README steps 140–142 say so.

## File map

| Crate / folder | New | Changed |
|---|---|---|
| `core-model` | tests `cmd_synth.rs` | `lane.rs`, `document.rs`, `command/routing.rs`, `command/lane.rs`, `rules.rs`; tests `cmd_reference.rs`, `model_types.rs`, `mixer_document.rs`, `cmd_drum.rs`, `query_flatten.rs`, `cmd_lane.rs`, `query_usage.rs`, `cmd_lane_input.rs` |
| `core-engine` | `measure.rs`; tests `fake_synth.rs`, `measure.rs` | `offline.rs`, `render.rs`, `engine.rs`, `lib.rs`; tests `rt_alloc_record.rs` |
| `core-ffi` | `synth.rs`; tests `synth.rs` | `passes.rs`, `record.rs`, `host.rs`, `session.rs`, `edit.rs`, `mixer.rs`, `control.rs`, `lib.rs`; tests `passes.rs`, `live_input.rs`, `recovered.rs`, `record.rs`, `control.rs` |
| `core-control` | — | `registry.rs`; tests `commands.rs` |
| `vizctl` | — | `tests/e2e.rs` |
| app | `VizDAWAppModel/SynthCommands.swift`; tests `SynthCommandsTests.swift` | `ControlCommands.swift`, `README.md`; tests `FakeSession.swift`, `RecordModelTests.swift` |

---

### Task 1: Planner — a latency fall cuts every piece it is heard over; the return shift

**Files:**
- Modify: `crates/core-ffi/src/passes.rs` (module docs, `AudioTake.return_frames`, `pieces()`, `plan()`)
- Modify: `crates/core-ffi/src/record.rs` (`build`: `return_frames: 0` until Task 8)
- Test: `crates/core-ffi/tests/passes.rs`

**Interfaces:**
- Consumes: 5c's planner (`Captured`, `Mark { run, frame, wrap, latency }`, `plan`).
- Produces: `vizdaw_ffi::passes::AudioTake { lane: LaneId, media: RecordedMedia, return_frames: i64 }` — `return_frames` are extra input-delay frames for that lane only (Task 8 fills them from the synth's return latency). `pieces()` cuts every earlier piece whose heard end lies past a newer piece's start (Ruling 13).

Spec §6.3, §6.7; Rulings 4 and 13. Pure code: no engine, no I/O.

- [ ] **Step 1: Write the failing tests**

In `crates/core-ffi/tests/passes.rs`, two `AudioTake` literals gain the new field.

Find:
```rust
    c.audio = vec![AudioTake {
        lane: d.mic,
        media: file(frames),
    }];
```
Replace:
```rust
    c.audio = vec![AudioTake {
        lane: d.mic,
        media: file(frames),
        return_frames: 0,
    }];
```

Find:
```rust
    c.audio = vec![AudioTake {
        lane: d.mic,
        media: file(1000),
    }];
```
Replace:
```rust
    c.audio = vec![AudioTake {
        lane: d.mic,
        media: file(1000),
        return_frames: 0,
    }];
```

Then append the tests:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/passes.rs <<'VIZDAW_EOF'

/// 5c final review carry (plan 5c-2 Ruling 13): a schedule-only resync
/// landed less than the latency fall before it, so two older pieces are
/// heard past the new piece's start. Both are cut there: MIDI played after
/// the fall lands with the new latency, and no file frame is placed twice.
#[test]
fn a_latency_fall_cuts_every_piece_it_is_heard_over() {
    let d = doc(None);
    let mut c = audio_case(&d, 0, 8 * BEAT);
    let b = BEAT as i64;
    c.marks = vec![
        latent(0, 0, 4800),
        // An edit recompiled the schedule; 1000 frames later the latent
        // plugin went.
        latent(2 * BEAT, 2 * BEAT, 4800),
        latent(2 * BEAT + 1000, 2 * BEAT + 1000, 0),
    ];
    c.midi_lanes = vec![d.keys];
    c.midi = vec![
        midi(d.keys, b + 4800, on(60, 100)),
        midi(d.keys, b + 4900, off(60)),
        // Heard over both older tails: the new graph's.
        midi(d.keys, 2 * b + 2000, on(61, 100)),
        midi(d.keys, 2 * b + 2100, off(61)),
    ];
    let p = c.run().unwrap();
    assert_eq!(
        active_regions(&p, d.mic),
        vec![
            (0, 4800, 2 * BEAT - 3800),
            (2 * Q + 640, 2 * BEAT + 1000, 6 * BEAT - 1000),
        ]
    );
    let got = active_frames(&p, d.mic, &d);
    assert_no_overlap(&got);
    for w in got.windows(2) {
        assert!(
            w[0].1 + w[0].2 <= w[1].1,
            "file frames placed twice: {got:?}"
        );
    }
    let k = p.lanes.iter().find(|x| x.lane == d.keys).unwrap();
    let ticks: Vec<i64> = k.active.events.iter().map(|e| e.tick.0).collect();
    assert_eq!(ticks, vec![Q, 2 * Q + 1280]);
}

/// Spec §6.7 (plan 5c-2 Ruling 4): a lane recording a synth's return
/// lands earlier by the synth's return latency, on top of the device
/// delay and the graph latency; MIDI recorded in the same pass does not
/// move.
#[test]
fn a_return_lane_lands_earlier_by_its_return_latency() {
    let d = doc(None);
    let mut c = audio_case(&d, 2 * BEAT, 4 * BEAT);
    c.latency = 512;
    c.input_delay = 300;
    c.audio[0].return_frames = 1200;
    c.midi_lanes = vec![d.keys];
    c.midi = vec![
        midi(d.keys, 512 + 2400, on(60, 100)),
        midi(d.keys, 512 + 4800, off(60)),
    ];
    let p = c.run().unwrap();
    // File frame 812 + 1200 holds the synth's answer to what was heard at
    // the record start.
    assert_eq!(
        active_regions(&p, d.mic),
        vec![(2 * Q, 2012, 4 * BEAT - 2012)]
    );
    let k = p.lanes.iter().find(|x| x.lane == d.keys).unwrap();
    let ticks: Vec<i64> = k.active.events.iter().map(|e| e.tick.0).collect();
    assert_eq!(ticks, vec![2 * Q + 2400 * Q / BEAT as i64]);
}

/// Plan 5c-2 Ruling 4: a return latency longer than the take means
/// nothing of the file was heard in time; no region reaches outside it.
#[test]
fn a_return_latency_longer_than_the_take_places_nothing() {
    let d = doc(None);
    let mut c = audio_case(&d, 2 * BEAT, 4 * BEAT);
    c.audio[0].return_frames = 5 * BEAT as i64;
    assert_eq!(c.run(), None);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test passes 2>&1 | tail -15
```
Expected: compile error, `AudioTake` has no field `return_frames`.

- [ ] **Step 3: Implement**

In `crates/core-ffi/src/passes.rs`:

Find:
```rust
//! empty delay: no audio is placed from them, and a message played then
//! lands where the next piece starts. Where `L` shrank, the newer piece is
//! heard over the older one's tail, which it cuts.
```
Replace:
```rust
//! empty delay: no audio is placed from them, and a message played then
//! lands where the next piece starts. Where `L` shrank, the newer piece is
//! heard over the tail of every earlier piece that reaches past its start
//! (the one just before, and one before that when a resync landed less
//! than the fall earlier: 5c final review carry, plan 5c-2 Ruling 13),
//! and cuts each there.
//!
//! **Synth returns (plan 5c-2 Ruling 4).** A lane recording an external
//! synth's return (spec §6.7) captured the synth's answer `R` frames (its
//! return latency) after the note that made it was heard: its file frames
//! are placed with `D + R` ([`AudioTake::return_frames`]), so its regions
//! land earlier by `R`. MIDI does not move.
```

Find:
```rust
/// One armed audio lane's finished file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTake {
    /// The lane.
    pub lane: LaneId,
    /// The file (package-relative path, format, frames, hash, name, pass).
    pub media: RecordedMedia,
}
```
Replace:
```rust
/// One armed audio lane's finished file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTake {
    /// The lane.
    pub lane: LaneId,
    /// The file (package-relative path, format, frames, hash, name, pass).
    pub media: RecordedMedia,
    /// Frames this lane's input arrived after what it records was heard,
    /// besides the device's delay: an external synth's return latency for
    /// a `return_of` lane (spec §6.7), else 0. Its regions move earlier by
    /// that much.
    pub return_frames: i64,
}
```

Find:
```rust
    // Into heard runs, each by its own latency (final review I1).
    let mut prev: Option<(&mut Piece, i64)> = None;
    for (p, x) in out.iter_mut().zip(&m) {
        let l = lat(x);
        p.from += l;
        p.run += l;
        p.end += l;
        p.catch += l;
        if let Some((q, ql)) = prev {
            let rise = l - ql;
            if q.end > p.from {
                // The latency shrank: the newer piece is heard over the
                // older one's tail.
                q.end = p.from.max(q.from);
            } else if rise > 0 {
                // It grew: what is played in the gap it opened lands at
                // this piece's start (a cut before it stays a cut).
                p.catch = (p.from - rise).max(q.end);
            }
        }
        prev = Some((p, l));
    }
    out
}
```
Replace:
```rust
    // Into heard runs, each by its own latency (final review I1).
    let lats: Vec<i64> = m.iter().map(lat).collect();
    for i in 0..out.len() {
        let l = lats.get(i).copied().unwrap_or(0);
        let (done, rest) = out.split_at_mut(i);
        let Some(p) = rest.first_mut() else {
            break;
        };
        p.from += l;
        p.run += l;
        p.end += l;
        p.catch += l;
        let Some(ql) = i.checked_sub(1).and_then(|j| lats.get(j)).copied() else {
            continue;
        };
        // The latency shrank: the newer piece is heard over the tail of
        // every earlier piece reaching past its start, not only the one
        // just before (plan 5c-2 Ruling 13); each is cut there.
        let mut fell = false;
        for q in done.iter_mut() {
            if q.end > p.from {
                q.end = p.from.max(q.from);
                fell = true;
            }
        }
        let rise = l - ql;
        if !fell
            && rise > 0
            && let Some(q) = done.last()
        {
            // It grew: what is played in the gap it opened lands at this
            // piece's start (a cut before it stays a cut).
            p.catch = (p.from - rise).max(q.end);
        }
    }
    out
}
```

Find:
```rust
        let mut regions = audio_regions(a.media.length_frames, &pieces, &c.range, d, &tempo);
```
Replace:
```rust
        // A synth's return lands earlier by its return latency (plan 5c-2
        // Ruling 4).
        let mut regions = audio_regions(
            a.media.length_frames,
            &pieces,
            &c.range,
            d.saturating_add(a.return_frames),
            &tempo,
        );
```

In `crates/core-ffi/src/record.rs`:

Find:
```rust
        audio.push(AudioTake {
            lane: a.lane,
```
Replace:
```rust
        audio.push(AudioTake {
            lane: a.lane,
            // Task 8 of plan 5c-2 gives a `return_of` lane its shift.
            return_frames: 0,
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test passes 2>&1 | tail -5
cargo test -p core-ffi --test record 2>&1 | tail -3
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: `passes` all pass (the 20 from 5c plus 3 new); `record` passes (one latency per pass is unchanged by the walk-back); fmt and clippy clean. Before the change, `a_latency_fall_cuts_every_piece_it_is_heard_over` fails with the first region `(0, 4800, 48000)` and the note at tick `2 * Q - 1792` (the older mapping), which is what the carry described.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/passes.rs crates/core-ffi/src/record.rs crates/core-ffi/tests/passes.rs
git commit -m "planner: a latency fall cuts every piece it is heard over; return shift per lane (5c-2 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 2: `core-model` — `return_of` and return latency on destinations

**Files:**
- Modify: `crates/core-model/src/lane.rs` (`LaneInput::Audio.return_of`, `LaneInput::return_of()`)
- Modify: `crates/core-model/src/document.rs` (`Destination.return_latency_ms`, `MAX_RETURN_LATENCY_MS`, `DestKind::is_external`)
- Modify: `crates/core-model/src/command/routing.rs` (`RoutingCmd::SetReturnLatency`, `check_external`, removal relinks `return_of`)
- Modify: `crates/core-model/src/command/lane.rs` (`SetInput` checks `return_of`)
- Modify: `crates/core-model/src/rules.rs` (a file's links and latencies are checked)
- Modify (the new fields in literals and patterns): `crates/core-model/tests/{cmd_reference,model_types,mixer_document,cmd_drum,query_flatten,cmd_lane,query_usage,cmd_lane_input}.rs`, `crates/core-ffi/src/{edit,session,host}.rs`, `crates/core-ffi/tests/{live_input,recovered,record}.rs`, `crates/vizctl/tests/e2e.rs`
- Test: `crates/core-model/tests/cmd_synth.rs` (new)

**Interfaces:**
- Produces:
  - `core_model::lane::LaneInput::Audio { channels: InputChannels, return_of: Option<DestId> }`; `LaneInput::return_of(&self) -> Option<DestId>`;
  - `core_model::document::Destination.return_latency_ms: f32` (absent in files when 0); `core_model::document::MAX_RETURN_LATENCY_MS: f32 = 500.0`; `DestKind::is_external(&self) -> bool` (Ruling 2);
  - `RoutingCmd::SetReturnLatency { dest: DestId, ms: f32 }`, undo label "Set Return Latency";
  - `pub(crate) fn command::routing::check_external(d: &Document, dest: DestId) -> Result<()>` (Task 3 uses it).

Spec §4.5, §4.5a; Rulings 1–3.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/cmd_synth.rs <<'VIZDAW_EOF'
//! Tracking a hardware synth in the model (plan 5c-2 Tasks 2 and 3;
//! sub-project 5 spec §4.5, §4.5a, §6.7): an audio input can record the
//! return of an external MIDI destination, the destination carries its
//! return latency as an undoable edit, removing the synth relinks or
//! unlinks the return lane, files write both only when set, and Track
//! Synth sets the return lane up in one step.

use core_model::ModelError;
use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::file::{from_json, to_json};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use core_model::selection::ViewFilter;
use serde_json::{Value, json};

struct Doc {
    doc: Document,
    /// An external synth ("Juno"), which lane 0 plays.
    juno: DestId,
    /// Another external synth, played by nothing.
    moog: DestId,
    /// A plugin instrument.
    pad: DestId,
    /// The built-in synth.
    synth: DestId,
}

fn run(d: &mut Document, cmd: Command) -> core_model::command::Report {
    let a = apply(d, &cmd).unwrap();
    *d = a.doc;
    a.report
}

fn add_dest(d: &mut Document, name: &str, kind: DestKind) -> DestId {
    let r = run(
        d,
        Command::Routing(RoutingCmd::AddDestination {
            name: name.into(),
            kind,
            default_patch: None,
        }),
    );
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Dest(x) => Some(*x),
            _ => None,
        })
        .unwrap()
}

fn endpoint(name: &str) -> DestKind {
    DestKind::MidiEndpoint {
        endpoint_ref: name.into(),
        group: 0,
        channel: 0,
    }
}

/// Sequence 0 with lane 0 ("Part") playing "Juno"; no output path.
fn doc() -> Doc {
    let mut d = apply(
        &Document::new(),
        &Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap()
    .doc;
    let juno = add_dest(&mut d, "Juno", endpoint("Juno"));
    let moog = add_dest(&mut d, "Moog", endpoint("Moog"));
    let pad = add_dest(&mut d, "Pad", DestKind::plugin("au:pad"));
    let synth = add_dest(&mut d, "Synth", endpoint("builtin:synth"));
    run(
        &mut d,
        Command::Lane(LaneCmd::Rename {
            seq: SeqId(0),
            lane: LaneId(0),
            name: "Part".into(),
        }),
    );
    run(
        &mut d,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: Some(juno),
        }),
    );
    Doc {
        doc: d,
        juno,
        moog,
        pad,
        synth,
    }
}

fn latency(d: &Document, dest: DestId) -> f32 {
    d.instruments().get(&dest).unwrap().return_latency_ms
}

fn set_latency(dest: DestId, ms: f32) -> Command {
    Command::Routing(RoutingCmd::SetReturnLatency { dest, ms })
}

fn set_input(lane: LaneId, input: Option<LaneInput>) -> Command {
    Command::Lane(LaneCmd::SetInput {
        seq: SeqId(0),
        lane,
        input,
    })
}

fn returning(channels: InputChannels, dest: DestId) -> LaneInput {
    LaneInput::Audio {
        channels,
        return_of: Some(dest),
    }
}

/// Adds lane "Rec" (index 1) recording input 3 as `dest`'s return.
fn with_return_lane(d: &mut Document, dest: DestId) -> LaneId {
    let r = run(
        d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Rec".into(),
            index: None,
        }),
    );
    let lane = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        })
        .unwrap();
    run(
        d,
        set_input(lane, Some(returning(InputChannels::Mono(2), dest))),
    );
    lane
}

#[test]
fn return_latency_is_an_undoable_edit_of_external_destinations_only() {
    let d = doc();
    let mut h = History::new(d.doc.clone());
    let f = ViewFilter::default();
    assert_eq!(latency(h.doc(), d.juno), 0.0);
    h.execute(&set_latency(d.juno, 12.5), &f).unwrap();
    assert_eq!(h.undo_label(), Some("Set Return Latency"));
    assert_eq!(latency(h.doc(), d.juno), 12.5);
    h.execute(&set_latency(d.juno, 500.0), &f).unwrap();
    assert_eq!(latency(h.doc(), d.juno), 500.0);
    h.undo().unwrap();
    h.undo().unwrap();
    assert_eq!(latency(h.doc(), d.juno), 0.0);
    // Not external, unknown, or out of range: refused, nothing changes.
    for (dest, ms) in [
        (d.pad, 1.0),
        (d.synth, 1.0),
        (DestId(99), 1.0),
        (d.juno, -0.5),
        (d.juno, 500.5),
        (d.juno, f32::NAN),
        (d.juno, f32::INFINITY),
    ] {
        assert!(
            apply(&d.doc, &set_latency(dest, ms)).is_err(),
            "{dest} {ms}"
        );
    }
    // An edit of the synth's kind keeps its latency.
    let mut doc = apply(&d.doc, &set_latency(d.juno, 7.0)).unwrap().doc;
    run(
        &mut doc,
        Command::Routing(RoutingCmd::EditDestination {
            dest: d.juno,
            kind: endpoint("Juno II"),
            default_patch: None,
        }),
    );
    assert_eq!(latency(&doc, d.juno), 7.0);
}

#[test]
fn return_of_names_an_external_destination() {
    let d = doc();
    let lane = LaneId(0);
    let ok = apply(
        &d.doc,
        &set_input(lane, Some(returning(InputChannels::Stereo(4), d.juno))),
    )
    .unwrap()
    .doc;
    let input = ok.lane(SeqId(0), lane).unwrap().input.clone().unwrap();
    assert_eq!(input.return_of(), Some(d.juno));
    for bad in [d.pad, d.synth, DestId(99)] {
        assert!(
            apply(
                &d.doc,
                &set_input(lane, Some(returning(InputChannels::Mono(0), bad)))
            )
            .is_err(),
            "{bad}"
        );
    }
    // A plain audio input names none.
    let plain = LaneInput::Audio {
        channels: InputChannels::Mono(0),
        return_of: None,
    };
    assert_eq!(plain.return_of(), None);
    assert!(apply(&d.doc, &set_input(lane, Some(plain))).is_ok());
}

fn lane_json(text: &str, index: usize) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    v["document"]["sequences"][0]["lanes"][index].clone()
}

fn dest_json(text: &str, name: &str) -> Value {
    let v: Value = serde_json::from_str(text).unwrap();
    v["document"]["instruments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == name)
        .unwrap()
        .clone()
}

#[test]
fn files_write_the_link_and_the_latency_only_when_set() {
    let d = doc();
    let text = to_json(&d.doc).unwrap();
    assert!(dest_json(&text, "Juno").get("return_latency_ms").is_none());
    assert_eq!(from_json(&text).unwrap(), d.doc);
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    run(&mut doc, set_latency(d.juno, 12.5));
    let plain = LaneInput::Audio {
        channels: InputChannels::Mono(1),
        return_of: None,
    };
    run(&mut doc, set_input(LaneId(0), Some(plain)));
    let text = to_json(&doc).unwrap();
    assert_eq!(dest_json(&text, "Juno")["return_latency_ms"], json!(12.5));
    assert!(dest_json(&text, "Moog").get("return_latency_ms").is_none());
    assert_eq!(
        lane_json(&text, 1)["input"],
        json!({"audio": {"channels": {"mono": 2}, "return_of": d.juno.0}})
    );
    assert_eq!(
        lane_json(&text, 0)["input"],
        json!({"audio": {"channels": {"mono": 1}}})
    );
    assert_eq!(from_json(&text).unwrap(), doc);
    assert_eq!(
        doc.lane(SeqId(0), rec).unwrap().input.as_ref().unwrap().return_of(),
        Some(d.juno)
    );
}

#[test]
fn a_damaged_link_or_latency_in_a_file_is_refused() {
    let d = doc();
    let mut doc = d.doc.clone();
    with_return_lane(&mut doc, d.juno);
    let text = to_json(&doc).unwrap();
    let base: Value = serde_json::from_str(&text).unwrap();
    let corrupt = |v: &Value, needle: &str| match from_json(&serde_json::to_string(v).unwrap()) {
        Err(ModelError::Corrupt { problems }) => assert!(
            problems.iter().any(|p| p.contains(needle)),
            "{needle}: {problems:?}"
        ),
        other => panic!("{needle}: {other:?}"),
    };
    let mut v = base.clone();
    v["document"]["sequences"][0]["lanes"][1]["input"]["audio"]["return_of"] = json!(77);
    corrupt(&v, "77");
    for ms in [json!(900.0), json!(-1.0)] {
        let mut v = base.clone();
        v["document"]["instruments"][0]["return_latency_ms"] = ms;
        corrupt(&v, "return_latency_ms");
    }
}

#[test]
fn removing_the_synth_relinks_or_unlinks_its_return_lane() {
    let d = doc();
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    let link = |doc: &Document| {
        doc.lane(SeqId(0), rec)
            .unwrap()
            .input
            .as_ref()
            .unwrap()
            .return_of()
    };
    let remove = |doc: &Document, replacement| {
        apply(
            doc,
            &Command::Routing(RoutingCmd::RemoveDestination {
                dest: d.juno,
                replacement,
            }),
        )
    };
    // An external replacement takes the link along.
    assert_eq!(link(&remove(&doc, Some(d.moog)).unwrap().doc), Some(d.moog));
    // A plugin replacement unlinks it (lane 0 now plays the plugin).
    assert_eq!(link(&remove(&doc, Some(d.pad)).unwrap().doc), None);
    // A link alone is no use: with nothing playing the synth, no
    // replacement is needed, and the lane keeps its channels.
    run(
        &mut doc,
        Command::Lane(LaneCmd::SetDest {
            seq: SeqId(0),
            lane: LaneId(0),
            dest: None,
        }),
    );
    let gone = remove(&doc, None).unwrap().doc;
    assert_eq!(link(&gone), None);
    assert_eq!(
        gone.lane(SeqId(0), rec).unwrap().input,
        Some(LaneInput::Audio {
            channels: InputChannels::Mono(2),
            return_of: None,
        })
    );
    assert_eq!(gone.lane(SeqId(0), rec).unwrap().monitor, MonitorMode::Auto);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_synth 2>&1 | tail -15
```
Expected: compile errors (`return_of`, `return_latency_ms`, `SetReturnLatency` unknown).

- [ ] **Step 3: Implement the model**

In `crates/core-model/src/lane.rs`:

Find:
```rust
    /// Audio from device input channels.
    Audio {
        /// Which channels.
        channels: InputChannels,
    },
```
Replace:
```rust
    /// Audio from device input channels.
    Audio {
        /// Which channels.
        channels: InputChannels,
        /// The external MIDI destination whose audio return this lane
        /// records (spec §4.5, §6.7: Track Synth): its recordings land
        /// earlier by that destination's return latency. Absent in files
        /// when none (plan 5c-2 Ruling 1).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        return_of: Option<DestId>,
    },
```

Find:
```rust
            LaneInput::Audio { channels } => {
                if channels.last().is_none_or(|c| c > MAX_INPUT_CHANNEL) {
```
Replace:
```rust
            LaneInput::Audio { channels, .. } => {
                if channels.last().is_none_or(|c| c > MAX_INPUT_CHANNEL) {
```

Find:
```rust
    /// True for an audio input.
    pub fn is_audio(&self) -> bool {
        matches!(self, LaneInput::Audio { .. })
    }
```
Replace:
```rust
    /// True for an audio input.
    pub fn is_audio(&self) -> bool {
        matches!(self, LaneInput::Audio { .. })
    }

    /// The destination whose return an audio input records, if any.
    pub fn return_of(&self) -> Option<DestId> {
        match self {
            LaneInput::Audio { return_of, .. } => *return_of,
            LaneInput::Midi { .. } => None,
        }
    }
```

In `crates/core-model/src/document.rs`:

Find:
```rust
    /// Where events go.
    pub kind: DestKind,
    /// Patch sent when the destination is first used.
    pub default_patch: Option<Patch>,
}
```
Replace:
```rust
    /// Where events go.
    pub kind: DestKind,
    /// Patch sent when the destination is first used.
    pub default_patch: Option<Patch>,
    /// An external MIDI destination's return latency (spec §4.5a): from a
    /// note leaving to its sound at the synth's audio output (the MIDI
    /// interface plus the synth's response), in ms,
    /// 0–[`MAX_RETURN_LATENCY_MS`]. Absent in files when 0 (plan 5c-2
    /// Ruling 1).
    #[serde(default, skip_serializing_if = "is_zero_ms")]
    pub return_latency_ms: f32,
}

/// The largest return latency a destination may have (spec §4.5a).
pub const MAX_RETURN_LATENCY_MS: f32 = 500.0;

fn is_zero_ms(v: &f32) -> bool {
    *v == 0.0
}
```

Find:
```rust
    /// True for MIDI endpoints that play on the built-in synth.
    pub fn is_builtin_synth(&self) -> bool {
```
Replace:
```rust
    /// True for an external MIDI destination (spec §4.5a): a MIDI endpoint
    /// that is not the built-in synth (plan 5c-2 Ruling 2).
    pub fn is_external(&self) -> bool {
        matches!(self, DestKind::MidiEndpoint { .. }) && !self.is_builtin_synth()
    }

    /// True for MIDI endpoints that play on the built-in synth.
    pub fn is_builtin_synth(&self) -> bool {
```

In `crates/core-model/src/command/routing.rs`:

Find:
```rust
use crate::document::{DestKind, Destination, Document, OutputPath, Patch};
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId};
```
Replace:
```rust
use crate::document::{
    DestKind, Destination, Document, MAX_RETURN_LATENCY_MS, OutputPath, Patch,
};
use crate::error::{ModelError, Result};
use crate::ids::{AnyId, DestId, LaneId, OutputPathId, SeqId};
use crate::lane::LaneInput;
```

Find:
```rust
        /// Replacement (required when in use).
        replacement: Option<OutputPathId>,
    },
}
```
Replace:
```rust
        /// Replacement (required when in use).
        replacement: Option<OutputPathId>,
    },
    /// Set an external destination's return latency (spec §4.5a).
    SetReturnLatency {
        /// Target (an external MIDI destination).
        dest: DestId,
        /// Milliseconds, 0–[`MAX_RETURN_LATENCY_MS`].
        ms: f32,
    },
}
```

Find:
```rust
            RoutingCmd::RemoveOutput { .. } => "Remove Output",
```
Replace:
```rust
            RoutingCmd::RemoveOutput { .. } => "Remove Output",
            RoutingCmd::SetReturnLatency { .. } => "Set Return Latency",
```

Find:
```rust
fn all_lanes(d: &Document) -> Vec<(SeqId, LaneId)> {
```
Replace:
```rust
/// Refuses `dest` unless it is an external MIDI destination (plan 5c-2
/// Ruling 2).
pub(crate) fn check_external(d: &Document, dest: DestId) -> Result<()> {
    let x = d
        .instruments()
        .get(&dest)
        .ok_or(ModelError::UnknownId(AnyId::Dest(dest)))?;
    if x.kind.is_external() {
        Ok(())
    } else {
        Err(ModelError::invalid(format!(
            "{} is not an external MIDI destination",
            x.name
        )))
    }
}

fn all_lanes(d: &Document) -> Vec<(SeqId, LaneId)> {
```

Find:
```rust
    let exists = replacement.is_some_and(|x| d.instruments().contains_key(&x));
    let rep = require_replacement(dest, replacement, exists, users)?;
    for (s, l) in all_lanes(d) {
        let lane = d.lane_mut(s, l)?;
        if lane.midi_dest == Some(dest) {
            lane.midi_dest = rep;
        }
```
Replace:
```rust
    let exists = replacement.is_some_and(|x| d.instruments().contains_key(&x));
    let rep = require_replacement(dest, replacement, exists, users)?;
    // A lane recording the synth's return follows an external replacement
    // and is unlinked otherwise; the link alone is no use (plan 5c-2
    // Ruling 3).
    let rep_return = rep.filter(|r| d.instruments().get(r).is_some_and(|x| x.kind.is_external()));
    for (s, l) in all_lanes(d) {
        let lane = d.lane_mut(s, l)?;
        if lane.midi_dest == Some(dest) {
            lane.midi_dest = rep;
        }
        if let Some(LaneInput::Audio { return_of, .. }) = lane.input.as_mut()
            && *return_of == Some(dest)
        {
            *return_of = rep_return;
        }
```

Find:
```rust
                    kind,
                    default_patch: *default_patch,
                },
```
Replace:
```rust
                    kind,
                    default_patch: *default_patch,
                    return_latency_ms: 0.0,
                },
```

Find:
```rust
            d.outputs_mut().remove(output);
            r.removed.push(AnyId::Output(*output));
            strips::remove_kind(d, StripKind::AudioPath(*output), r);
        }
    }
    Ok(())
}
```
Replace:
```rust
            d.outputs_mut().remove(output);
            r.removed.push(AnyId::Output(*output));
            strips::remove_kind(d, StripKind::AudioPath(*output), r);
        }
        RoutingCmd::SetReturnLatency { dest, ms } => {
            check_external(d, *dest)?;
            if !ms.is_finite() || !(0.0..=MAX_RETURN_LATENCY_MS).contains(ms) {
                return Err(ModelError::range("return_latency_ms", ms));
            }
            d.instruments_mut()
                .get_mut(dest)
                .ok_or(ModelError::UnknownId(AnyId::Dest(*dest)))?
                .return_latency_ms = *ms;
        }
    }
    Ok(())
}
```

In `crates/core-model/src/command/lane.rs`:

Find:
```rust
        LaneCmd::SetInput { seq, lane, input } => {
            if let Some(i) = input {
                i.check()?;
            }
```
Replace:
```rust
        LaneCmd::SetInput { seq, lane, input } => {
            if let Some(i) = input {
                i.check()?;
                // A synth's return names an external destination (plan
                // 5c-2 Ruling 3).
                if let Some(dest) = i.return_of() {
                    super::routing::check_external(d, dest)?;
                }
            }
```

In `crates/core-model/src/rules.rs`:

Find:
```rust
use crate::document::{DestKind, Document};
```
Replace:
```rust
use crate::document::{DestKind, Document, MAX_RETURN_LATENCY_MS};
use crate::lane::LaneInput;
```

Find:
```rust
        if let Some(patch) = d.default_patch {
            let ok = patch.program <= 127 && patch.bank.is_none_or(|(m, l)| m <= 127 && l <= 127);
            push(need(ok, "default_patch", patch.program), &d.id.to_string());
        }
```
Replace:
```rust
        if let Some(patch) = d.default_patch {
            let ok = patch.program <= 127 && patch.bank.is_none_or(|(m, l)| m <= 127 && l <= 127);
            push(need(ok, "default_patch", patch.program), &d.id.to_string());
        }
        let ms = d.return_latency_ms;
        push(
            need(
                ms.is_finite() && (0.0..=MAX_RETURN_LATENCY_MS).contains(&ms),
                "return_latency_ms",
                ms,
            ),
            &d.id.to_string(),
        );
```

Find:
```rust
            if let Some(i) = &l.input {
                push(i.check(), &lctx);
            }
```
Replace:
```rust
            if let Some(i) = &l.input {
                push(i.check(), &lctx);
            }
            // Plan 5c-2 Ruling 3: a synth's return names a destination.
            if let Some(dest) = l.input.as_ref().and_then(LaneInput::return_of)
                && !doc.instruments().contains_key(&dest)
            {
                push(Err(ModelError::UnknownId(AnyId::Dest(dest))), &lctx);
            }
```

- [ ] **Step 4: Give every literal and pattern the new fields**

These are mechanical: `return_latency_ms: 0.0` after `default_patch: None` in `Destination` literals, `return_of: None` in `LaneInput::Audio` literals, `..` in patterns.

`crates/core-model/tests/cmd_reference.rs` — Find:
```rust
    let dest = |i: u64| Destination {
        id: DestId(i),
        name: format!("D{i}"),
        kind: DestKind::PluginInstrument {
            slot: PluginSlot::new(PluginInstanceId(i), PluginRef::from_legacy("p")),
        },
        default_patch: None,
    };
```
Replace:
```rust
    let dest = |i: u64| Destination {
        id: DestId(i),
        name: format!("D{i}"),
        kind: DestKind::PluginInstrument {
            slot: PluginSlot::new(PluginInstanceId(i), PluginRef::from_legacy("p")),
        },
        default_patch: None,
        return_latency_ms: 0.0,
    };
```

`crates/core-model/tests/cmd_drum.rs` — the same Find and Replace as `cmd_reference.rs` (its closure is identical).

`crates/core-model/tests/model_types.rs` — Find:
```rust
            name: "Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "ep".into(),
                group: 1,
                channel: 9,
            },
            default_patch: None,
```
Replace:
```rust
            name: "Synth".into(),
            kind: DestKind::MidiEndpoint {
                endpoint_ref: "ep".into(),
                group: 1,
                channel: 9,
            },
            default_patch: None,
            return_latency_ms: 0.0,
```

`crates/core-model/tests/mixer_document.rs` — Find:
```rust
            endpoint_ref: r.into(),
            group: 0,
            channel: ch,
        },
        default_patch: None,
    }
```
Replace:
```rust
            endpoint_ref: r.into(),
            group: 0,
            channel: ch,
        },
        default_patch: None,
        return_latency_ms: 0.0,
    }
```
and Find:
```rust
                name: "Pad".into(),
                kind: DestKind::plugin("au:pad"),
                default_patch: None,
```
Replace:
```rust
                name: "Pad".into(),
                kind: DestKind::plugin("au:pad"),
                default_patch: None,
                return_latency_ms: 0.0,
```

`crates/core-model/tests/query_flatten.rs` — Find:
```rust
                name: "Plug".into(),
                kind: DestKind::plugin("p"),
                default_patch: None,
```
Replace:
```rust
                name: "Plug".into(),
                kind: DestKind::plugin("p"),
                default_patch: None,
                return_latency_ms: 0.0,
```
and Find:
```rust
                    endpoint_ref: "e".into(),
                    group: 2,
                    channel: 5,
                },
                default_patch: None,
```
Replace:
```rust
                    endpoint_ref: "e".into(),
                    group: 2,
                    channel: 5,
                },
                default_patch: None,
                return_latency_ms: 0.0,
```

`crates/core-model/tests/query_usage.rs` — Find:
```rust
                    endpoint_ref: "e".into(),
                    group: 1,
                    channel: 9,
                },
                default_patch: None,
```
Replace:
```rust
                    endpoint_ref: "e".into(),
                    group: 1,
                    channel: 9,
                },
                default_patch: None,
                return_latency_ms: 0.0,
```
and Find:
```rust
                name: "Plug".into(),
                kind: DestKind::plugin("p"),
                default_patch: None,
```
Replace:
```rust
                name: "Plug".into(),
                kind: DestKind::plugin("p"),
                default_patch: None,
                return_latency_ms: 0.0,
```

`crates/core-model/tests/cmd_lane.rs` — Find:
```rust
            name: "Piano".into(),
            kind: DestKind::plugin("p"),
            default_patch: None,
```
Replace:
```rust
            name: "Piano".into(),
            kind: DestKind::plugin("p"),
            default_patch: None,
            return_latency_ms: 0.0,
```

`crates/core-model/tests/cmd_lane_input.rs` — six `LaneInput::Audio` literals:
- Find `        channels: InputChannels::Stereo(2),\n    };` Replace `        channels: InputChannels::Stereo(2),\n        return_of: None,\n    };`
- Find `            channels: InputChannels::Mono(MAX_INPUT_CHANNEL + 1),\n        },` Replace `            channels: InputChannels::Mono(MAX_INPUT_CHANNEL + 1),\n            return_of: None,\n        },`
- Find `            channels: InputChannels::Stereo(MAX_INPUT_CHANNEL),\n        },` Replace `            channels: InputChannels::Stereo(MAX_INPUT_CHANNEL),\n            return_of: None,\n        },`
- Find `            channels: InputChannels::Stereo(u16::MAX),\n        },` Replace `            channels: InputChannels::Stereo(u16::MAX),\n            return_of: None,\n        },`
- Find `        channels: InputChannels::Stereo(MAX_INPUT_CHANNEL - 1),\n    };` Replace `        channels: InputChannels::Stereo(MAX_INPUT_CHANNEL - 1),\n        return_of: None,\n    };`
- Find `set_input(Some(LaneInput::Audio { channels }))` Replace `set_input(Some(LaneInput::Audio {\n            channels,\n            return_of: None,\n        }))`

`crates/core-ffi/src/edit.rs` — Find:
```rust
            LaneInputRow::Audio { first, stereo } => LaneInput::Audio {
                channels: if *stereo {
                    InputChannels::Stereo(*first)
                } else {
                    InputChannels::Mono(*first)
                },
            },
```
Replace:
```rust
            LaneInputRow::Audio { first, stereo } => LaneInput::Audio {
                channels: if *stereo {
                    InputChannels::Stereo(*first)
                } else {
                    InputChannels::Mono(*first)
                },
                // Task 8 of plan 5c-2 carries it over the FFI.
                return_of: None,
            },
```
and Find `            LaneInput::Audio { channels } => LaneInputRow::Audio {` Replace `            LaneInput::Audio { channels, .. } => LaneInputRow::Audio {`.

`crates/core-ffi/src/session.rs` — Find `                Some(LaneInput::Audio { channels }) => {` Replace `                Some(LaneInput::Audio { channels, .. }) => {`.

`crates/core-ffi/src/host.rs` — Find `            Some(LaneInput::Audio { channels }) if l.monitor != MonitorMode::Off => {` Replace `            Some(LaneInput::Audio { channels, .. }) if l.monitor != MonitorMode::Off => {`.

`crates/core-ffi/tests/live_input.rs`, `crates/core-ffi/tests/recovered.rs`, `crates/vizctl/tests/e2e.rs` — in each, Find:
```rust
                channels: InputChannels::Mono(0),
            }),
```
(in `recovered.rs` it is indented four spaces less: `            channels: InputChannels::Mono(0),\n        }),`) and add `return_of: None,` after the `channels` line at the same indentation.

`crates/core-ffi/tests/record.rs` — Find:
```rust
            channels: InputChannels::Mono(0),
        }),
    }))
```
Replace:
```rust
            channels: InputChannels::Mono(0),
            return_of: None,
        }),
    }))
```
and Find:
```rust
                channels: InputChannels::Stereo(2),
            }),
```
Replace:
```rust
                channels: InputChannels::Stereo(2),
                return_of: None,
            }),
```

`grep -rn "LaneInput::Audio {" crates --include='*.rs'` and `grep -rn "Destination {$" crates --include='*.rs'` then list nothing left without the new field (the compiler says so too).

- [ ] **Step 5: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_synth 2>&1 | tail -5
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c | tail -8
```
Expected: `cmd_synth` 5 pass; the workspace passes as at the baseline plus the new tests (snapshot tests of files are unchanged: neither field is written when unset).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/lane.rs crates/core-model/src/document.rs \
  crates/core-model/src/command/routing.rs crates/core-model/src/command/lane.rs \
  crates/core-model/src/rules.rs crates/core-model/tests/cmd_synth.rs \
  crates/core-model/tests/cmd_reference.rs crates/core-model/tests/model_types.rs \
  crates/core-model/tests/mixer_document.rs crates/core-model/tests/cmd_drum.rs \
  crates/core-model/tests/query_flatten.rs crates/core-model/tests/cmd_lane.rs \
  crates/core-model/tests/query_usage.rs crates/core-model/tests/cmd_lane_input.rs \
  crates/core-ffi/src/edit.rs crates/core-ffi/src/session.rs crates/core-ffi/src/host.rs \
  crates/core-ffi/tests/live_input.rs crates/core-ffi/tests/recovered.rs \
  crates/core-ffi/tests/record.rs crates/vizctl/tests/e2e.rs
git commit -m "model: return_of on audio inputs and return latency on external destinations (5c-2 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 3: `core-model` — Track Synth in one undo step

**Files:**
- Modify: `crates/core-model/src/command/lane.rs` (`LaneCmd::TrackSynth`)
- Test: `crates/core-model/tests/cmd_synth.rs` (appended)

**Interfaces:**
- Consumes: `check_external` (Task 2), `routing::add_output`, `LaneInput::return_of`.
- Produces: `core_model::command::LaneCmd::TrackSynth { seq: SeqId, lane: LaneId, channels: InputChannels, arm_midi: bool }`, undo label "Track Synth"; a new return lane is reported in `Report.created` (lane and take; the output too when one was made).

Spec §6.7 "Setup"; Ruling 5.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-model/tests/cmd_synth.rs <<'VIZDAW_EOF'

fn track(lane: LaneId, channels: InputChannels, arm_midi: bool) -> Command {
    Command::Lane(LaneCmd::TrackSynth {
        seq: SeqId(0),
        lane,
        channels,
        arm_midi,
    })
}

/// Spec §6.7, §12.3: Track Synth sets up and arms in one undo step: a lane
/// "<synth> audio" just below records the chosen channels as the synth's
/// return, on the first output path (made when there is none), armed and
/// monitoring Auto; the MIDI lane is disarmed so its part plays (plan
/// 5c-2 Ruling 5).
#[test]
fn track_synth_sets_up_and_arms_in_one_undo_step() {
    let d = doc();
    let mut doc = d.doc.clone();
    run(
        &mut doc,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Below".into(),
            index: None,
        }),
    );
    run(
        &mut doc,
        Command::Lane(LaneCmd::SetFlags {
            seq: SeqId(0),
            lane: LaneId(0),
            mute: None,
            solo: None,
            arm: Some(true),
        }),
    );
    let mut h = History::new(doc.clone());
    let f = ViewFilter::default();
    let r = h
        .execute(&track(LaneId(0), InputChannels::Mono(2), false), &f)
        .unwrap();
    assert_eq!(h.undo_label(), Some("Track Synth"));
    let s = h.doc().sequence(SeqId(0)).unwrap();
    let names: Vec<&str> = s.lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Part", "Juno audio", "Below"]);
    let rec = &s.lanes[1];
    assert!(r.created.contains(&AnyId::Lane(rec.id)));
    assert_eq!(rec.input, Some(returning(InputChannels::Mono(2), d.juno)));
    assert!(rec.arm);
    assert_eq!(rec.monitor, MonitorMode::Auto);
    let out = rec.audio_out.unwrap();
    assert_eq!(h.doc().outputs().get(&out).unwrap().name, "Audio 1");
    assert!(!s.lanes[0].arm, "the MIDI lane plays, unarmed");
    h.undo().unwrap();
    assert_eq!(h.doc(), &doc, "one step undoes all of it");
}

#[test]
fn track_synth_reuses_the_return_lane_and_can_arm_the_midi_lane() {
    let d = doc();
    let mut doc = d.doc.clone();
    run(&mut doc, track(LaneId(0), InputChannels::Mono(2), false));
    let lanes = doc.sequence(SeqId(0)).unwrap().lanes.len();
    let out = doc.sequence(SeqId(0)).unwrap().lanes[1].audio_out;
    // Again, stereo, with the MIDI lane armed: the same lane is set up.
    run(&mut doc, track(LaneId(0), InputChannels::Stereo(4), true));
    let s = doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), lanes);
    assert_eq!(
        s.lanes[1].input,
        Some(returning(InputChannels::Stereo(4), d.juno))
    );
    assert_eq!(s.lanes[1].audio_out, out, "its output is kept");
    assert_eq!(doc.outputs().len(), 1);
    assert!(s.lanes[0].arm && s.lanes[1].arm);
    // A lane that records the synth's return elsewhere in the sequence is
    // the one reused, wherever it is.
    let mut doc = d.doc.clone();
    let rec = with_return_lane(&mut doc, d.juno);
    run(
        &mut doc,
        Command::Lane(LaneCmd::Reorder {
            seq: SeqId(0),
            lane: rec,
            to: 0,
        }),
    );
    run(&mut doc, track(LaneId(0), InputChannels::Mono(5), false));
    let s = doc.sequence(SeqId(0)).unwrap();
    assert_eq!(s.lanes.len(), 2);
    assert_eq!(
        doc.lane(SeqId(0), rec).unwrap().input,
        Some(returning(InputChannels::Mono(5), d.juno))
    );
}

/// Plan 5c-2 Ruling 5: Track Synth refuses a lane that plays no external
/// synth (none, a plugin, the built-in synth), an unknown lane or
/// sequence, and channels past the last; nothing changes.
#[test]
fn track_synth_refuses_what_it_cannot_set_up() {
    let d = doc();
    let mut doc = d.doc.clone();
    for (name, dest) in [("Pad lane", Some(d.pad)), ("Synth lane", Some(d.synth)), ("Bare", None)] {
        let r = run(
            &mut doc,
            Command::Lane(LaneCmd::Add {
                seq: SeqId(0),
                name: name.into(),
                index: None,
            }),
        );
        let lane = r
            .created
            .iter()
            .find_map(|id| match id {
                AnyId::Lane(l) => Some(*l),
                _ => None,
            })
            .unwrap();
        run(
            &mut doc,
            Command::Lane(LaneCmd::SetDest {
                seq: SeqId(0),
                lane,
                dest,
            }),
        );
        assert!(
            apply(&doc, &track(lane, InputChannels::Mono(0), false)).is_err(),
            "{name}"
        );
    }
    assert!(apply(&doc, &track(LaneId(99), InputChannels::Mono(0), false)).is_err());
    assert!(
        apply(
            &doc,
            &Command::Lane(LaneCmd::TrackSynth {
                seq: SeqId(9),
                lane: LaneId(0),
                channels: InputChannels::Mono(0),
                arm_midi: false,
            })
        )
        .is_err()
    );
    assert!(apply(&doc, &track(LaneId(0), InputChannels::Mono(256), false)).is_err());
    assert!(apply(&doc, &track(LaneId(0), InputChannels::Stereo(255), false)).is_err());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_synth 2>&1 | tail -10
```
Expected: compile error, `LaneCmd` has no variant `TrackSynth`.

- [ ] **Step 3: Implement**

In `crates/core-model/src/command/lane.rs`:

Find:
```rust
use crate::lane::{Lane, LaneInput, MonitorMode, QuantizeSpec, Take};
```
Replace:
```rust
use crate::lane::{InputChannels, Lane, LaneInput, MonitorMode, QuantizeSpec, Take};
```

Find:
```rust
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
```
Replace:
```rust
    /// Set when the lane's input is heard (spec §6.6).
    SetMonitor {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// The mode.
        monitor: MonitorMode,
    },
    /// Track Synth (spec §6.7; plan 5c-2 Ruling 5): sets up the audio lane
    /// recording the return of the external synth MIDI lane `lane` plays,
    /// in one step. It reuses the sequence's lane already recording that
    /// synth's return, else adds "<synth> audio" just below `lane`; sets
    /// that lane's input to `channels` as the synth's return, gives it an
    /// output path when it has none (the first, else a new "Audio 1") and
    /// arms it. The MIDI lane is armed when `arm_midi`, else disarmed, so
    /// a Replace pass never clears the part that plays the synth.
    TrackSynth {
        /// Sequence.
        seq: SeqId,
        /// The MIDI lane playing the synth.
        lane: LaneId,
        /// The device input channels the synth's outputs are wired to.
        channels: InputChannels,
        /// Also arm the MIDI lane (to record the performance as well).
        arm_midi: bool,
    },
}
```

Find:
```rust
            LaneCmd::SetMonitor { .. } => "Set Lane Monitor",
```
Replace:
```rust
            LaneCmd::SetMonitor { .. } => "Set Lane Monitor",
            LaneCmd::TrackSynth { .. } => "Track Synth",
```

Find:
```rust
        LaneCmd::SetMonitor { seq, lane, monitor } => {
            d.lane_mut(*seq, *lane)?.monitor = *monitor;
        }
    }
    Ok(())
}
```
Replace:
```rust
        LaneCmd::SetMonitor { seq, lane, monitor } => {
            d.lane_mut(*seq, *lane)?.monitor = *monitor;
        }
        LaneCmd::TrackSynth {
            seq,
            lane,
            channels,
            arm_midi,
        } => track_synth(d, *seq, *lane, *channels, *arm_midi, r)?,
    }
    Ok(())
}

/// [`LaneCmd::TrackSynth`].
fn track_synth(
    d: &mut Document,
    seq: SeqId,
    lane: LaneId,
    channels: InputChannels,
    arm_midi: bool,
    r: &mut Report,
) -> Result<()> {
    let dest = d.lane(seq, lane)?.midi_dest.ok_or_else(|| {
        ModelError::invalid("Track Synth needs a lane that plays an external MIDI destination")
    })?;
    super::routing::check_external(d, dest)?;
    let input = LaneInput::Audio {
        channels,
        return_of: Some(dest),
    };
    input.check()?;
    let name = d
        .instruments()
        .get(&dest)
        .map(|x| format!("{} audio", x.name))
        .unwrap_or_else(|| "Synth audio".into());
    let s = d.sequence(seq)?;
    let at = s
        .lane_index(lane)
        .ok_or(ModelError::UnknownId(AnyId::Lane(lane)))?;
    let existing = s
        .lanes
        .iter()
        .find(|l| l.id != lane && l.input.as_ref().and_then(LaneInput::return_of) == Some(dest))
        .map(|l| l.id);
    let audio = match existing {
        Some(id) => id,
        None => {
            let c = d.counters_mut();
            let (id, take) = (c.alloc_lane(), c.alloc_take());
            let s = d.seq_mut(seq)?;
            let below = at.saturating_add(1).min(s.lanes.len());
            s.lanes.insert(below, Lane::new(id, take, name));
            r.created.extend([AnyId::Lane(id), AnyId::Take(take)]);
            id
        }
    };
    if d.lane(seq, audio)?.audio_out.is_none() {
        // Heard through its strip while tracking (spec §6.7 "Monitoring").
        let out = match d.outputs().keys().next().copied() {
            Some(o) => o,
            None => super::routing::add_output(d, "Audio 1", "audio:1-2", r),
        };
        d.lane_mut(seq, audio)?.audio_out = Some(out);
    }
    let l = d.lane_mut(seq, audio)?;
    l.input = Some(input);
    l.arm = true;
    d.lane_mut(seq, lane)?.arm = arm_midi;
    Ok(())
}
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: `cmd_synth` 8 pass; all of `core-model` passes; clean.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/lane.rs crates/core-model/tests/cmd_synth.rs
git commit -m "model: Track Synth sets up and arms the return lane in one step (5c-2 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 4: `core-engine` — the fake synth

**Files:**
- Modify: `crates/core-engine/src/offline.rs` (`FakeSynth`, its MIDI sink, channel 3 of the signals input)
- Test: `crates/core-engine/tests/fake_synth.rs` (new)

**Interfaces:**
- Produces (in `core_engine::offline`):
  - `FAKE_SYNTH_PORT: &str = "Fake Synth"`, `FAKE_SYNTH_CHANNEL: usize = 2` (0-based: "channel 3"), `FAKE_CLICK_SECONDS = 0.005`, `FAKE_CLICK_LEVEL = 0.5`, `FAKE_TONE_HZ = 220.0`, `FAKE_TONE_LEVEL = 0.1`, `FAKE_SYNTH_EVENTS = 4096`;
  - `FakeSynth::new(delay_ms: f64) -> FakeSynth` (`Clone`, `Debug`), `delay_ms()`, `dropped()`;
  - `OfflineSinks::with_fake_synth(self, &FakeSynth) -> OfflineSinks` (adds the port; call after `with_ports`);
  - `OfflineBackend::with_synth(self, &FakeSynth) -> OfflineBackend` (call after `with_input(FakeInput::Signals)`), `OfflineBackend::synth_delay(&self) -> Option<u64>` (frames), `OfflineBackend::input(&self, c: usize, frames: u32) -> &[f32]` (the last block's fake input channel `c`).

Spec §12.1 `VIZDAW_FAKE_SYNTH`; Ruling 7. The sink and the input both run on the render thread: they share a lock-free queue over atomics.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/fake_synth.rs <<'VIZDAW_EOF'
//! The fake synth (plan 5c-2 Task 4; sub-project 5 spec §12.1
//! `VIZDAW_FAKE_SYNTH`): notes sent to the MIDI destination "Fake Synth"
//! play on fake input channel 3 the synth's delay after their audible
//! time: a 5 ms click at each note-on and a 220 Hz tone while a note is
//! held. The delay is at least one block; with the loopback input there
//! is no synth.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{
    FAKE_CLICK_LEVEL, FAKE_SYNTH_CHANNEL, FAKE_SYNTH_PORT, FAKE_TONE_LEVEL, FakeInput,
    FakeSynth, OfflineBackend, OfflineSinks,
};

/// 120 BPM at 48 kHz: one beat is 24 000 frames.
const BEAT: usize = 24_000;

/// Plays notes at beats 1 and 2 (half a beat each) through the fake
/// synth for `total` frames; returns channel 3 and the synth's delay.
fn play(delay_ms: f64, total: usize) -> (Vec<f32>, u64) {
    let mut b = Builder::new();
    let dest = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, lane) = b.sequence("S", "Part", Some(dest)).unwrap();
    b.notes(
        seq,
        lane,
        &[(Q, 60, 0x8000, Q / 2), (2 * Q, 64, 0x8000, Q / 2)],
    )
    .unwrap();
    let doc = b.finish();
    let synth = FakeSynth::new(delay_ms);
    let sinks = OfflineSinks::new(1 << 16).with_fake_synth(&synth);
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.load(&doc, seq).unwrap();
    let mut backend = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&synth);
    let delay = backend.synth_delay().unwrap();
    backend.start(renderer).unwrap();
    engine.play().unwrap();
    let mut out = Vec::new();
    while out.len() < total {
        backend.block(512);
        out.extend_from_slice(backend.input(FAKE_SYNTH_CHANNEL, 512));
    }
    drop(backend.stop().unwrap());
    assert_eq!(synth.dropped(), 0);
    (out, delay)
}

#[test]
fn a_note_on_clicks_on_channel_3_the_delay_after_it_is_heard() {
    let (ch, delay) = play(25.0, 3 * BEAT);
    assert_eq!(delay, 1200, "25 ms at 48 kHz");
    let first = BEAT + 1200;
    assert!(ch[..first].iter().all(|x| *x == 0.0), "silent before");
    assert!(ch[first] >= FAKE_CLICK_LEVEL - 1e-6, "the click starts on time");
    // The click lasts 5 ms; the tone sounds while the note is held (half
    // a beat), then the note-off ends it.
    let held = first + 240..first + BEAT / 2;
    assert!(ch[held.clone()].iter().all(|x| x.abs() <= FAKE_TONE_LEVEL + 1e-6));
    assert!(ch[held].iter().any(|x| x.abs() > FAKE_TONE_LEVEL / 2.0));
    let second = 2 * BEAT + 1200;
    assert!(
        ch[first + BEAT / 2..second].iter().all(|x| *x == 0.0),
        "the note-off ends the tone"
    );
    assert!(ch[second] >= FAKE_CLICK_LEVEL - 1e-6);
}

#[test]
fn the_delay_is_at_least_one_block() {
    let (ch, delay) = play(1.0, 2 * BEAT);
    assert_eq!(delay, 512);
    assert_eq!(ch.iter().position(|x| x.abs() > 0.01), Some(BEAT + 512));
}

#[test]
fn with_the_loopback_input_there_is_no_synth() {
    let synth = FakeSynth::new(25.0);
    let b = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Loopback(1000))
        .with_synth(&synth);
    assert_eq!(b.synth_delay(), None);
    let b = OfflineBackend::new(48_000.0, 512).with_synth(&synth);
    assert_eq!(b.synth_delay(), None, "no fake input: no synth either");
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --features fixtures --test fake_synth 2>&1 | tail -10
```
Expected: compile errors (`FakeSynth`, `FAKE_SYNTH_PORT` unknown). (`fixtures` is on for the crate's own tests through its dev-dependency; the flag is harmless if it already is.)

- [ ] **Step 3: Implement**

In `crates/core-engine/src/offline.rs`:

Find:
```rust
use std::sync::atomic::{AtomicU64, Ordering};
```
Replace:
```rust
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
```

Find:
```rust
    /// When false, every MIDI output refuses messages (tests the
    /// midi-dropped fault).
    pub midi_accepts: bool,
    tone: bool,
}
```
Replace:
```rust
    /// When false, every MIDI output refuses messages (tests the
    /// midi-dropped fault).
    pub midi_accepts: bool,
    tone: bool,
    /// The fake synth behind the port [`FAKE_SYNTH_PORT`] (plan 5c-2).
    synth: Option<FakeSynth>,
}
```

Find:
```rust
            midi_accepts: true,
            tone: false,
        }
```
Replace:
```rust
            midi_accepts: true,
            tone: false,
            synth: None,
        }
```

Find:
```rust
    /// The shared log.
    pub fn log(&self) -> EventLog {
        self.log.clone()
    }
}
```
Replace:
```rust
    /// Pretends the MIDI destination [`FAKE_SYNTH_PORT`] exists and is
    /// `synth` (spec §12.1): what is sent to it is logged like any MIDI
    /// output and also played by the synth on the fake input. Call after
    /// [`OfflineSinks::with_ports`], which replaces the ports.
    pub fn with_fake_synth(mut self, synth: &FakeSynth) -> OfflineSinks {
        if !self.ports.iter().any(|p| p.name == FAKE_SYNTH_PORT) {
            self.ports.push(MidiPortInfo {
                name: FAKE_SYNTH_PORT.into(),
                unique_id: None,
            });
        }
        self.synth = Some(synth.clone());
        self
    }

    /// The shared log.
    pub fn log(&self) -> EventLog {
        self.log.clone()
    }
}
```

Find:
```rust
            TargetKind::MidiVirtualSource
            | TargetKind::MidiDestination(_)
            | TargetKind::PluginInstrument(_) => Sink::Midi(Box::new(RecordingMidiOut {
```
Replace:
```rust
            TargetKind::MidiDestination(p) if p.name == FAKE_SYNTH_PORT && self.synth.is_some() => {
                let queue = self
                    .synth
                    .as_ref()
                    .map(|s| Arc::clone(&s.queue))
                    .unwrap_or_else(|| Arc::new(SynthQueue::new()));
                Sink::Midi(Box::new(FakeSynthOut {
                    log: RecordingMidiOut {
                        tx,
                        target,
                        sample_rate,
                        accept: self.midi_accepts,
                    },
                    queue,
                }))
            }
            TargetKind::MidiVirtualSource
            | TargetKind::MidiDestination(_)
            | TargetKind::PluginInstrument(_) => Sink::Midi(Box::new(RecordingMidiOut {
```

Find:
```rust
/// The fake input's buffers.
#[derive(Debug)]
struct Fake {
```
Replace:
```rust
/// The fake external MIDI destination's port name (spec §12.1).
pub const FAKE_SYNTH_PORT: &str = "Fake Synth";
/// The fake input channel the fake synth plays on (0-based: "channel 3").
pub const FAKE_SYNTH_CHANNEL: usize = 2;
/// The fake synth's click: 5 ms (spec §12.1).
pub const FAKE_CLICK_SECONDS: f64 = 0.005;
/// The click's level: −6 dBFS (plan 5c-2 Ruling 7).
pub const FAKE_CLICK_LEVEL: f32 = 0.5;
/// The tone a held note sounds (spec §12.1: 220 Hz).
pub const FAKE_TONE_HZ: f64 = 220.0;
/// The tone's amplitude: −20 dBFS.
pub const FAKE_TONE_LEVEL: f32 = 0.1;
/// Note events the fake synth holds until the fake input plays them.
pub const FAKE_SYNTH_EVENTS: usize = 4096;

/// Event kinds in the low two bits of a queued event (the frame is above
/// them).
const EV_OFF: u64 = 0;
const EV_ON: u64 = 1;
const EV_ALL_OFF: u64 = 2;

/// The fake synth's note events: pushed by its MIDI sink and taken by the
/// fake input, both on the render thread, one at a time (a sink replaced
/// by a load takes over the same queue). Single producer, single
/// consumer, over atomics; a full queue drops and counts. // RT where
/// marked
struct SynthQueue {
    slots: Box<[AtomicU64]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

impl std::fmt::Debug for SynthQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SynthQueue")
    }
}

impl SynthQueue {
    fn new() -> SynthQueue {
        SynthQueue {
            slots: (0..FAKE_SYNTH_EVENTS).map(|_| AtomicU64::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    /// Queues `v` (frame << 2 | kind). // RT
    fn push(&self, v: u64) {
        let h = self.head.load(Ordering::Relaxed);
        let t = self.tail.load(Ordering::Acquire);
        let n = self.slots.len().max(1);
        if h.wrapping_sub(t) >= n {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if let Some(s) = self.slots.get(h % n) {
            s.store(v, Ordering::Relaxed);
        }
        self.head.store(h.wrapping_add(1), Ordering::Release);
    }

    /// The oldest queued event. // RT
    fn peek(&self) -> Option<u64> {
        let t = self.tail.load(Ordering::Relaxed);
        if t == self.head.load(Ordering::Acquire) {
            return None;
        }
        let n = self.slots.len().max(1);
        self.slots.get(t % n).map(|s| s.load(Ordering::Relaxed))
    }

    /// Drops the oldest queued event. // RT
    fn pop(&self) {
        let t = self.tail.load(Ordering::Relaxed);
        self.tail.store(t.wrapping_add(1), Ordering::Release);
    }
}

/// A fake external synth (spec §12.1 `VIZDAW_FAKE_SYNTH`; plan 5c-2
/// Ruling 7): what is sent to the MIDI destination [`FAKE_SYNTH_PORT`]
/// plays on fake input channel 3 `delay_ms` after its audible time:
/// every note-on starts a 5 ms click, and a 220 Hz tone sounds while any
/// note is held. Give the same synth to [`OfflineSinks::with_fake_synth`]
/// and [`OfflineBackend::with_synth`].
#[derive(Clone)]
pub struct FakeSynth {
    queue: Arc<SynthQueue>,
    delay_ms: f64,
}

impl std::fmt::Debug for FakeSynth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeSynth")
            .field("delay_ms", &self.delay_ms)
            .finish()
    }
}

impl FakeSynth {
    /// A synth answering `delay_ms` late (not finite or negative: 0).
    pub fn new(delay_ms: f64) -> FakeSynth {
        FakeSynth {
            queue: Arc::new(SynthQueue::new()),
            delay_ms: if delay_ms.is_finite() {
                delay_ms.max(0.0)
            } else {
                0.0
            },
        }
    }

    /// Its delay in milliseconds.
    pub fn delay_ms(&self) -> f64 {
        self.delay_ms
    }

    /// Note events dropped because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.queue.dropped.load(Ordering::Relaxed)
    }
}

/// The fake synth's MIDI destination: logs like [`RecordingMidiOut`] and
/// queues note events at their audible frame for the fake input (the fake
/// device's output latency is 0 with the signals input, so a host time is
/// its frame).
pub struct FakeSynthOut {
    log: RecordingMidiOut,
    queue: Arc<SynthQueue>,
}

impl MidiOut for FakeSynthOut {
    /// // RT
    fn send(&mut self, host_time_ns: u64, msg: &[u8]) -> bool {
        if !self.log.send(host_time_ns, msg) {
            return false;
        }
        let frame = (host_time_ns as f64 * self.log.sample_rate / 1e9).round() as u64;
        let kind = match msg {
            [s, _, v, ..] if s & 0xF0 == 0x90 && *v > 0 => Some(EV_ON),
            [s, ..] if s & 0xF0 == 0x80 || s & 0xF0 == 0x90 => Some(EV_OFF),
            [s, c, ..] if s & 0xF0 == 0xB0 && (*c == 120 || *c == 123) => Some(EV_ALL_OFF),
            _ => None,
        };
        if let Some(k) = kind {
            self.queue.push((frame.min(u64::MAX >> 2) << 2) | k);
        }
        true
    }
}

/// The fake synth's voice in the fake input.
#[derive(Debug)]
struct SynthVoice {
    queue: Arc<SynthQueue>,
    /// Frames after a note's audible frame it sounds.
    delay: u64,
    /// Notes held.
    held: u32,
    /// Where the last click started.
    click_from: Option<u64>,
    /// Where the tone started (its phase).
    tone_from: u64,
}

impl SynthVoice {
    /// Renders channel 3 for absolute frames `abs..abs + out.len()`. // RT
    fn render(&mut self, out: &mut [f32], abs: u64, rate: f64) {
        let click = (FAKE_CLICK_SECONDS * rate).round().max(1.0) as u64;
        for (k, x) in out.iter_mut().enumerate() {
            let at = abs + k as u64;
            while let Some(ev) = self.queue.peek() {
                let when = (ev >> 2).saturating_add(self.delay);
                if when > at {
                    break;
                }
                match ev & 3 {
                    EV_ON => {
                        if self.held == 0 {
                            self.tone_from = when;
                        }
                        self.held = self.held.saturating_add(1);
                        self.click_from = Some(when);
                    }
                    EV_OFF => self.held = self.held.saturating_sub(1),
                    _ => self.held = 0,
                }
                self.queue.pop();
            }
            let mut v = 0.0f32;
            if self.click_from.is_some_and(|c| at >= c && at - c < click) {
                v += FAKE_CLICK_LEVEL;
            }
            if self.held > 0 {
                let t = at.saturating_sub(self.tone_from) as f64 / rate.max(1.0);
                v += (f64::from(FAKE_TONE_LEVEL) * (std::f64::consts::TAU * FAKE_TONE_HZ * t).sin())
                    as f32;
            }
            *x = v;
        }
    }
}

/// The fake input's buffers.
#[derive(Debug)]
struct Fake {
```

Find:
```rust
    /// The output's left and right, by absolute frame modulo the length.
    history: [Vec<f32>; 2],
}
```
Replace:
```rust
    /// The output's left and right, by absolute frame modulo the length.
    history: [Vec<f32>; 2],
    /// The fake synth on channel 3 (plan 5c-2 Ruling 7).
    synth: Option<SynthVoice>,
}
```

Find:
```rust
            chans: Default::default(),
            history: Default::default(),
        });
```
Replace:
```rust
            chans: Default::default(),
            history: Default::default(),
            synth: None,
        });
```

Find:
```rust
    /// Frames rendered so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }
```
Replace:
```rust
    /// Frames rendered so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Plays `synth` on fake input channel 3 (spec §12.1). Only with the
    /// signals input (plan 5c-2 Ruling 7): call after
    /// [`OfflineBackend::with_input`]; with no fake input or the loopback
    /// nothing changes. Its delay is at least one block, as the loopback's
    /// is: a note is sent during the block it plays in.
    pub fn with_synth(mut self, synth: &FakeSynth) -> OfflineBackend {
        let rate = self.sample_rate;
        let block = u64::from(self.block);
        if let Some(f) = self.fake.as_mut().filter(|f| f.kind == FakeInput::Signals) {
            let delay = ((synth.delay_ms * rate / 1000.0).round().max(0.0) as u64).max(block);
            f.synth = Some(SynthVoice {
                queue: Arc::clone(&synth.queue),
                delay,
                held: 0,
                click_from: None,
                tone_from: 0,
            });
        }
        self
    }

    /// Frames the fake synth sounds after a note's audible frame, once
    /// [`OfflineBackend::with_synth`] gave it one.
    pub fn synth_delay(&self) -> Option<u64> {
        self.fake
            .as_ref()
            .and_then(|f| f.synth.as_ref())
            .map(|s| s.delay)
    }

    /// The last block's fake input channel `c`, `frames` long (for tests;
    /// empty without a fake input).
    pub fn input(&self, c: usize, frames: u32) -> &[f32] {
        self.fake
            .as_ref()
            .and_then(|f| f.chans.get(c))
            .and_then(|ch| ch.get(..frames as usize))
            .unwrap_or(&[])
    }
```

Find:
```rust
    let (Some(c0), Some(c1), Some(c2)) = (c0.get_mut(..n), c1.get_mut(..n), c2.get_mut(..n)) else {
        return;
    };
    c2.fill(0.0);
```
Replace:
```rust
    let (Some(c0), Some(c1), Some(c2)) = (c0.get_mut(..n), c1.get_mut(..n), c2.get_mut(..n)) else {
        return;
    };
    c2.fill(0.0);
    // The fake synth plays on channel 3 (plan 5c-2 Ruling 7).
    if let Some(s) = f.synth.as_mut() {
        s.render(c2, abs, rate);
    }
```

Also update the doc of `FakeInput::Signals` — Find:
```rust
    /// Channel 1 carries a 440 Hz sine at −12 dBFS, channel 2 an impulse
    /// every 0.5 s (from frame 0), channel 3 silence.
    Signals,
```
Replace:
```rust
    /// Channel 1 carries a 440 Hz sine at −12 dBFS, channel 2 an impulse
    /// every 0.5 s (from frame 0), channel 3 silence, or the fake synth
    /// ([`OfflineBackend::with_synth`]).
    Signals,
```

- [ ] **Step 4: Run the tests and the rt-check gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test fake_synth 2>&1 | tail -5
cargo test -p core-engine 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -3
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep -E "^test result|FAILED"
```
Expected: `fake_synth` 3 pass; `core-engine` all pass; clean; the three rt tests pass (Task 6 adds the fake synth to `rt_alloc_record`).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/offline.rs crates/core-engine/tests/fake_synth.rs
git commit -m "engine: the fake synth answers on fake input channel 3 (5c-2 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 5: `core-engine` — Measure with the transport stopped

**Files:**
- Create: `crates/core-engine/src/measure.rs`
- Modify: `crates/core-engine/src/lib.rs` (`pub mod measure`)
- Modify: `crates/core-engine/src/render.rs` (`EngineCmd::Measure`, `Retired::Measure`, the renderer's measurement)
- Modify: `crates/core-engine/src/engine.rs` (`set_return_dests`, `start_measure`, `stop_measure`; return destinations interned at load)
- Test: `crates/core-engine/tests/measure.rs` (new)

**Interfaces:**
- Produces (in `core_engine::measure`):
  - `PINGS = 5`, `PING_PITCH = 60`, `PING_VELOCITY = 100`, `PING_EVERY_SECONDS = 0.5`, `PING_SECONDS = 0.1`, `LEAD_SECONDS = 0.1`, `ONSET_LEVEL: f32 = 0.01`, `QUIET_SECONDS = 0.005`;
  - `MeasureRequest { target: TargetIdx, channel: u8, inputs: Vec<u16> }`, `tap(&MeasureRequest, sample_rate: f64) -> (MeasureTap, Arc<MeasureStatus>)`;
  - `MeasureStatus`: `sample_rate() -> f64`, `sent(i) -> Option<i64>` (note *i*'s heard run), `onset(i) -> Option<i64>` (the input run of its onset), `delay() -> Option<i64>` (the device's input delay at the first block), `window() -> i64` (frames each note's return is watched), `done() -> bool`, `closed() -> bool`;
  - `Engine::set_return_dests(&mut self, Vec<DestId>)`; `Engine::start_measure(&mut self, doc: &Document, dest: DestId, inputs: Vec<u16>) -> Result<Result<Arc<MeasureStatus>, String>, EngineError>`; `Engine::stop_measure(&mut self) -> Result<(), EngineError>`.

Spec §6.7 "Measure"; Rulings 8 and 10. The renderer runs the measurement every block, playing or not: the transport is stopped while Measure runs (Task 9 sees to that). Nothing here maps runs to milliseconds; Task 9's `measured` does.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/measure.rs <<'VIZDAW_EOF'
//! Measuring a synth's return latency (plan 5c-2 Task 5; sub-project 5
//! spec §6.7): with the transport stopped, five notes 500 ms apart reach
//! the synth's destination and each one's onset is found on the return
//! input the synth's delay later; a silent input, or one that never falls
//! quiet (a ringing release, a hum), finds none; only a destination the
//! load interned can be measured.

use core_engine::backend::{AudioBackend, MidiPortInfo};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::Builder;
use core_engine::measure::{MeasureStatus, PINGS};
use core_engine::offline::{FAKE_SYNTH_PORT, FakeInput, FakeSynth, OfflineBackend, OfflineSinks};
use core_model::ids::DestId;
use std::sync::Arc;

/// Measures the destination named `port` (its lane's return on `inputs`)
/// against a fake synth 25 ms late; returns the status and the synth's
/// delay in frames.
fn measure(port: &str, inputs: Vec<u16>) -> (Arc<MeasureStatus>, i64) {
    let mut b = Builder::new();
    let dest = b.dest(port, port, 0).unwrap();
    let (seq, _) = b.sequence("S", "Part", Some(dest)).unwrap();
    let doc = b.finish();
    let synth = FakeSynth::new(25.0);
    // "Other" is a MIDI destination nothing answers.
    let sinks = OfflineSinks::new(1 << 16)
        .with_ports(vec![MidiPortInfo {
            name: "Other".into(),
            unique_id: None,
        }])
        .with_fake_synth(&synth);
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_return_dests(vec![dest]);
    engine.load(&doc, seq).unwrap();
    let mut backend = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&synth);
    let delay = backend.synth_delay().unwrap() as i64;
    backend.start(renderer).unwrap();
    let status = engine.start_measure(&doc, dest, inputs).unwrap().unwrap();
    // The lead, five notes 500 ms apart and the last window: under 3 s.
    for _ in 0..(3 * 48_000 / 512) {
        if status.done() {
            break;
        }
        backend.block(512);
    }
    engine.stop_measure().unwrap();
    backend.block(512);
    assert!(status.closed(), "removed, it reads closed");
    drop(backend.stop().unwrap());
    (status, delay)
}

#[test]
fn every_note_onset_is_found_the_synth_delay_after_it_was_heard() {
    let (s, delay) = measure(FAKE_SYNTH_PORT, vec![2]);
    assert!(s.done());
    assert_eq!(s.delay(), Some(0), "the signals input has no round trip");
    assert_eq!(s.window(), 24_000);
    let sent: Vec<i64> = (0..PINGS).map(|i| s.sent(i).unwrap()).collect();
    // The first after the 100 ms lead, then 500 ms apart (each at the
    // first block after it falls due).
    assert!((4800..4800 + 512).contains(&sent[0]), "{sent:?}");
    for w in sent.windows(2) {
        assert!((w[1] - w[0] - 24_000).abs() < 512, "{sent:?}");
    }
    for (i, u) in sent.iter().enumerate() {
        assert_eq!(s.onset(i), Some(u + delay), "note {i}");
    }
}

#[test]
fn a_silent_return_finds_no_onset() {
    let (s, _) = measure("Other", vec![2]);
    assert!(s.done());
    assert!((0..PINGS).all(|i| s.sent(i).is_some() && s.onset(i).is_none()));
}

/// Plan 5c-2 Ruling 8: an onset follows 5 ms of quiet. Fake input 1 (the
/// sine) never falls quiet that long, as a ringing release or a hum would
/// not: no onset is taken from it.
#[test]
fn an_input_that_never_falls_quiet_finds_no_onset() {
    let (s, _) = measure(FAKE_SYNTH_PORT, vec![0]);
    assert!(s.done());
    assert!((0..PINGS).all(|i| s.onset(i).is_none()));
    // A pair: either channel's onset counts.
    let (s, delay) = measure(FAKE_SYNTH_PORT, vec![1, 2]);
    assert!((0..PINGS).all(|i| s.onset(i).is_some()));
    let _ = delay;
}

/// Plan 5c-2 Ruling 10: the return destinations are interned at load, so
/// a synth the sequence never plays can be measured; another cannot.
#[test]
fn only_a_destination_the_load_interned_can_be_measured() {
    let mut b = Builder::new();
    let dest = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, _) = b.sequence("S", "Audio", None).unwrap();
    let doc = b.finish();
    let synth = FakeSynth::new(25.0);
    let (mut engine, _renderer) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(64).with_fake_synth(&synth)),
    )
    .unwrap();
    engine.load(&doc, seq).unwrap();
    assert!(engine.start_measure(&doc, dest, vec![2]).unwrap().is_err());
    assert!(engine.start_measure(&doc, DestId(99), vec![2]).unwrap().is_err());
    engine.set_return_dests(vec![dest]);
    engine.load(&doc, seq).unwrap();
    assert!(engine.start_measure(&doc, dest, vec![2]).unwrap().is_ok());
}
VIZDAW_EOF
```

In `an_input_that_never_falls_quiet_finds_no_onset`, the pair `[1, 2]` is fake input 2 (impulses every 0.5 s) with channel 3 (the synth): the impulse and the synth's click both follow quiet, so every window finds an onset; the test only checks that a pair is watched on both channels.

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test measure 2>&1 | tail -10
```
Expected: compile errors (`core_engine::measure` unknown).

- [ ] **Step 3: Write the measurement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/measure.rs <<'VIZDAW_EOF'
//! Measuring a synth's return latency (sub-project 5 spec §6.7; plan 5c-2
//! Ruling 8): with the transport stopped, the renderer sends five short
//! notes to the synth's destination and watches the lane's return input
//! for each note's onset. It counts *runs* (output frames from the block
//! that installed it); the control side turns what it found into
//! milliseconds. // RT where marked

use crate::midi1::Midi1Msg;
use crate::schedule::TargetIdx;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

/// Notes a measurement sends (spec §6.7: 5).
pub const PINGS: usize = 5;
/// Their pitch (spec §6.7: 60).
pub const PING_PITCH: u8 = 60;
/// Their velocity (spec §6.7: 100).
pub const PING_VELOCITY: u8 = 100;
/// How far apart they start (spec §6.7: 500 ms); each note's return is
/// watched this long.
pub const PING_EVERY_SECONDS: f64 = 0.5;
/// How long each sounds ("short notes"; plan 5c-2 Ruling 8).
pub const PING_SECONDS: f64 = 0.1;
/// From the measurement's start to its first note.
pub const LEAD_SECONDS: f64 = 0.1;
/// An onset is the first sample above this (spec §6.7: −40 dBFS) …
pub const ONSET_LEVEL: f32 = 0.01;
/// … after at least this long at or below it (plan 5c-2 Ruling 8).
pub const QUIET_SECONDS: f64 = 0.005;

const NONE: i64 = i64::MIN;

fn read(a: &AtomicI64) -> Option<i64> {
    let v = a.load(Ordering::Acquire);
    (v != NONE).then_some(v)
}

/// What a measurement shares with the control side.
#[derive(Debug)]
pub struct MeasureStatus {
    sample_rate: f64,
    window: i64,
    sent: [AtomicI64; PINGS],
    onsets: [AtomicI64; PINGS],
    delay: AtomicI64,
    done: AtomicBool,
    closed: AtomicBool,
}

impl MeasureStatus {
    /// The engine's rate.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Frames each note's return is watched (500 ms).
    pub fn window(&self) -> i64 {
        self.window
    }

    /// The run at which note `i` was heard (sent, plus the graph latency
    /// and the MIDI offset), once it was sent.
    pub fn sent(&self, i: usize) -> Option<i64> {
        self.sent.get(i).and_then(read)
    }

    /// The input run of note `i`'s onset, once one was found.
    pub fn onset(&self, i: usize) -> Option<i64> {
        self.onsets.get(i).and_then(read)
    }

    /// The device's input delay (spec §6.3: the round trip) at the first
    /// block.
    pub fn delay(&self) -> Option<i64> {
        read(&self.delay)
    }

    /// Every note was sent and its window has passed.
    pub fn done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// The measurement was removed, or dropped with its renderer.
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

/// What to measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasureRequest {
    /// The synth's target.
    pub target: TargetIdx,
    /// The MIDI channel the notes go on (0–15).
    pub channel: u8,
    /// The return's device input channels (one, or a pair; more are
    /// ignored).
    pub inputs: Vec<u16>,
}

/// The real-time side of a measurement, owned by the renderer.
#[derive(Debug)]
pub struct MeasureTap {
    status: Arc<MeasureStatus>,
    target: TargetIdx,
    on: Midi1Msg,
    off: Midi1Msg,
    inputs: [u16; 2],
    count: usize,
    lead: u64,
    every: u64,
    length: u64,
    quiet_need: u64,
    run: u64,
    next: usize,
    /// The run the sounding note ends at.
    off_at: Option<u64>,
    sent: [Option<i64>; PINGS],
    found: [bool; PINGS],
    delay: Option<i64>,
    /// Samples at or below [`ONSET_LEVEL`] in a row.
    quiet: u64,
}

impl Drop for MeasureTap {
    fn drop(&mut self) {
        self.status.closed.store(true, Ordering::Release);
    }
}

/// A measurement and its control side. Allocates; call off the real-time
/// thread.
pub fn tap(req: &MeasureRequest, sample_rate: f64) -> (MeasureTap, Arc<MeasureStatus>) {
    let frames = |s: f64| (s * sample_rate).round().max(1.0) as u64;
    let every = frames(PING_EVERY_SECONDS);
    let status = Arc::new(MeasureStatus {
        sample_rate,
        window: i64::try_from(every).unwrap_or(i64::MAX),
        sent: std::array::from_fn(|_| AtomicI64::new(NONE)),
        onsets: std::array::from_fn(|_| AtomicI64::new(NONE)),
        delay: AtomicI64::new(NONE),
        done: AtomicBool::new(false),
        closed: AtomicBool::new(false),
    });
    let mut inputs = [0u16; 2];
    for (d, s) in inputs.iter_mut().zip(&req.inputs) {
        *d = *s;
    }
    let ch = req.channel & 0x0F;
    (
        MeasureTap {
            status: Arc::clone(&status),
            target: req.target,
            on: Midi1Msg::three(0x90 | ch, PING_PITCH, PING_VELOCITY),
            off: Midi1Msg::three(0x80 | ch, PING_PITCH, 64),
            inputs,
            count: req.inputs.len().min(2),
            lead: frames(LEAD_SECONDS),
            every,
            length: frames(PING_SECONDS),
            quiet_need: frames(QUIET_SECONDS),
            run: 0,
            next: 0,
            off_at: None,
            sent: [None; PINGS],
            found: [false; PINGS],
            delay: None,
            quiet: 0,
        },
        status,
    )
}

impl MeasureTap {
    /// The synth's target.
    pub fn target(&self) -> TargetIdx {
        self.target
    }

    /// At a block's start: sends what is due (the note-off of the note
    /// sounding, then the next note), each at block offset 0; `late` is
    /// how long after the block's first frame its MIDI is heard (the graph
    /// latency plus the MIDI offset), in frames. // RT
    pub fn sends(&mut self, late: u64, mut send: impl FnMut(Midi1Msg)) {
        if let Some(at) = self.off_at
            && self.run >= at
        {
            send(self.off);
            self.off_at = None;
        }
        if self.off_at.is_some() || self.next >= PINGS {
            return;
        }
        let due = self
            .lead
            .saturating_add(self.every.saturating_mul(self.next as u64));
        if self.run < due {
            return;
        }
        send(self.on);
        let heard = i64::try_from(self.run.saturating_add(late)).unwrap_or(i64::MAX);
        if let (Some(s), Some(st)) = (self.sent.get_mut(self.next), self.status.sent.get(self.next)) {
            *s = Some(heard);
            st.store(heard, Ordering::Release);
        }
        self.off_at = Some(self.run.saturating_add(self.length));
        self.next += 1;
    }

    /// At a block's end: watches its input (device channel `c` is
    /// `input[c]`) for onsets and advances the run; `delay` is the device's
    /// input delay for the block. // RT
    pub fn capture(&mut self, input: &[&[f32]], frames: u32, delay: i64) {
        let d = match self.delay {
            Some(d) => d,
            None => {
                self.delay = Some(delay);
                self.status.delay.store(delay, Ordering::Release);
                delay
            }
        };
        let window = self.status.window;
        let run = i64::try_from(self.run).unwrap_or(i64::MAX);
        for j in 0..frames as usize {
            let mut level = 0.0f32;
            for c in self.inputs.iter().take(self.count) {
                let x = input
                    .get(usize::from(*c))
                    .and_then(|ch| ch.get(j))
                    .copied()
                    .unwrap_or(0.0);
                level = level.max(x.abs());
            }
            if level <= ONSET_LEVEL {
                self.quiet = self.quiet.saturating_add(1);
                continue;
            }
            if self.quiet >= self.quiet_need {
                let k = run.saturating_add(j as i64);
                for ((s, f), st) in self
                    .sent
                    .iter()
                    .zip(self.found.iter_mut())
                    .zip(self.status.onsets.iter())
                {
                    let Some(u) = *s else {
                        continue;
                    };
                    let from = u.saturating_add(d);
                    if !*f && k >= from && k < from.saturating_add(window) {
                        *f = true;
                        st.store(k, Ordering::Release);
                    }
                }
            }
            self.quiet = 0;
        }
        self.run = self.run.saturating_add(u64::from(frames));
        if self.next >= PINGS
            && let Some(Some(u)) = self.sent.last()
        {
            let end = i64::try_from(self.run).unwrap_or(i64::MAX);
            if end >= u.saturating_add(d).saturating_add(window) {
                self.status.done.store(true, Ordering::Release);
            }
        }
    }

    /// The measurement is removed: the sounding note ends. // RT
    pub fn release(&mut self, mut send: impl FnMut(Midi1Msg)) {
        if self.off_at.take().is_some() {
            send(self.off);
        }
    }

    /// The control side reads it closed now, before the retire thread
    /// frees it. // RT
    pub fn close(&self) {
        self.status.closed.store(true, Ordering::Release);
    }
}
VIZDAW_EOF
```

In `crates/core-engine/src/lib.rs` — Find:
```rust
pub mod input;
pub mod midi1;
```
Replace:
```rust
pub mod input;
pub mod measure;
pub mod midi1;
```

- [ ] **Step 4: The renderer runs it**

In `crates/core-engine/src/render.rs`:

Find:
```rust
use crate::record::RecordTap;
```
Replace:
```rust
use crate::measure::MeasureTap;
use crate::record::RecordTap;
```

Find:
```rust
    /// Replace the live input routes (spec §6.6); notes sounding through
    /// thru end first.
    SetInputs(Box<InputRoutes>),
}
```
Replace:
```rust
    /// Replace the live input routes (spec §6.6); notes sounding through
    /// thru end first.
    SetInputs(Box<InputRoutes>),
    /// Install a return-latency measurement (spec §6.7), or remove the one
    /// installed (its sounding note ends; it goes back through the retire
    /// ring).
    Measure(Option<Box<MeasureTap>>),
}
```

Find:
```rust
    /// Replaced input routes.
    Inputs(Box<InputRoutes>),
}
```
Replace:
```rust
    /// Replaced input routes.
    Inputs(Box<InputRoutes>),
    /// A removed measurement.
    Measure(Box<MeasureTap>),
}
```

Find:
```rust
    /// This block's input, for monitoring.
    input_copy: Box<InputCopy>,
}
```
Replace:
```rust
    /// This block's input, for monitoring.
    input_copy: Box<InputCopy>,
    /// A return-latency measurement (spec §6.7), while one is installed.
    measure: Option<Box<MeasureTap>>,
}
```

Find:
```rust
            input_copy: Box::new(InputCopy::new(0, 0)),
        };
```
Replace:
```rust
            input_copy: Box::new(InputCopy::new(0, 0)),
            measure: None,
        };
```

Find:
```rust
    /// The tap's end of a block (see [`RecordTap::end_block`]). // RT
    fn end_tap(&mut self, input: &InputBlock<'_>, frames: u32, advanced: bool) {
        if let Some(t) = self.tap.as_deref_mut() {
            t.end_block(input.channels, frames, advanced);
        }
    }
```
Replace:
```rust
    /// The tap's end of a block (see [`RecordTap::end_block`]), and the
    /// measurement's (it watches every block's input, played or not:
    /// [`MeasureTap::capture`]). // RT
    fn end_tap(&mut self, input: &InputBlock<'_>, frames: u32, advanced: bool) {
        if let Some(t) = self.tap.as_deref_mut() {
            t.end_block(input.channels, frames, advanced);
        }
        if let Some(m) = self.measure.as_deref_mut() {
            m.capture(input.channels, frames, input.delay_frames);
        }
    }
```

Find:
```rust
        self.live_midi();
        let monitored = self.copy_monitored(input, frames);
```
Replace:
```rust
        self.live_midi();
        self.measure_sends();
        let monitored = self.copy_monitored(input, frames);
```

Find:
```rust
                Ok(EngineCmd::SetInputs(_)) => self.inputs.is_some() && self.retire.slots() == 0,
```
Replace:
```rust
                Ok(EngineCmd::SetInputs(_)) => self.inputs.is_some() && self.retire.slots() == 0,
                Ok(EngineCmd::Measure(_)) => self.measure.is_some() && self.retire.slots() == 0,
```

Find:
```rust
            EngineCmd::AddSink(idx, sink) => {
```
Replace:
```rust
            EngineCmd::Measure(new) => {
                if let Some(mut old) = std::mem::replace(&mut self.measure, new) {
                    // Its note must not hang.
                    let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
                    let mut o = Out {
                        sinks: &mut self.sinks,
                        shared: &self.shared,
                        block_ns: self.block_ns,
                        ns_per_frame: 0.0,
                        late_ns,
                        base: 0,
                    };
                    let target = old.target();
                    old.release(|m| o.send(target, 0, m.as_slice()));
                    old.close();
                    if let Err(rtrb::PushError::Full(old)) = self.retire.push(Retired::Measure(old))
                    {
                        // Unreachable: room was checked in `drain_commands`.
                        std::mem::forget(old);
                    }
                }
            }
            EngineCmd::AddSink(idx, sink) => {
```

Find:
```rust
    /// [`LATE_SECONDS`] in frames.
    fn late_frames(&self) -> u64 {
```
Replace:
```rust
    /// Sends what a measurement has due (spec §6.7) at block offset 0,
    /// heard the graph latency and the MIDI offset after the block's first
    /// frame. // RT
    fn measure_sends(&mut self) {
        let late_ns = self.cfg.midi_offset_ns + self.latency_ns();
        let late = (late_ns.max(0) as f64 * self.cfg.sample_rate / 1e9).round() as u64;
        let Some(m) = self.measure.as_deref_mut() else {
            return;
        };
        let mut o = Out {
            sinks: &mut self.sinks,
            shared: &self.shared,
            block_ns: self.block_ns,
            ns_per_frame: 0.0,
            late_ns,
            base: 0,
        };
        let target = m.target();
        m.sends(late, |msg| o.send(target, 0, msg.as_slice()));
    }

    /// [`LATE_SECONDS`] in frames.
    fn late_frames(&self) -> u64 {
```

- [ ] **Step 5: The engine starts it and interns the synths**

In `crates/core-engine/src/engine.rs`:

Find:
```rust
    /// The routes last sent to the renderer: a load sends them only when
    /// they change, so a load still retires just a schedule and a graph.
    inputs_sent: crate::input::InputRoutes,
}
```
Replace:
```rust
    /// The routes last sent to the renderer: a load sends them only when
    /// they change, so a load still retires just a schedule and a graph.
    inputs_sent: crate::input::InputRoutes,
    /// The destinations whose return a lane records (spec §6.7): interned
    /// at every load, so a measurement can play them (plan 5c-2 Ruling 10).
    return_dests: Vec<DestId>,
}
```

Find:
```rust
            inputs_sent: crate::input::InputRoutes::default(),
        };
```
Replace:
```rust
            inputs_sent: crate::input::InputRoutes::default(),
            return_dests: Vec::new(),
        };
```

Find:
```rust
                midi_routes.push(crate::input::MidiRoute {
                    spec: m.clone(),
                    targets,
                    dest_channel,
                    chain: crate::input::InputChain,
                });
            }
        }
```
Replace:
```rust
                midi_routes.push(crate::input::MidiRoute {
                    spec: m.clone(),
                    targets,
                    dest_channel,
                    chain: crate::input::InputChain,
                });
            }
            // The synths lanes record the return of (spec §6.7): interned
            // like thru's, so Measure can play one the sequence never plays
            // (plan 5c-2 Ruling 10). One that cannot play is left out;
            // Measure says why.
            for id in &self.return_dests {
                let Some(d) = doc.instruments().get(id) else {
                    continue;
                };
                if let Ok(kinds) = kinds_for(&d.kind, opts.route, &opts.ports) {
                    for k in &kinds {
                        let _ = resolve(k);
                    }
                }
            }
        }
```

Find:
```rust
    /// Feeds this engine's MIDI input queue (CoreMIDI sources,
    /// `midi.inject`).
    pub fn midi_sender(&self) -> crate::input::MidiSender {
        self.midi_tx.clone()
    }
}
```
Replace:
```rust
    /// Feeds this engine's MIDI input queue (CoreMIDI sources,
    /// `midi.inject`).
    pub fn midi_sender(&self) -> crate::input::MidiSender {
        self.midi_tx.clone()
    }

    /// The destinations whose return the loaded sequence's lanes record
    /// (spec §6.7); the next [`Engine::load`] interns their targets.
    pub fn set_return_dests(&mut self, dests: Vec<DestId>) {
        self.return_dests = dests;
    }

    /// Starts measuring `dest`'s return latency (spec §6.7) on device
    /// input channels `inputs` (its return lane's): from the next block
    /// the renderer plays the notes to the synth and watches the input,
    /// whether or not the transport plays. Replaces a measurement already
    /// installed. `Ok(Err(why))` when `dest` cannot play here: it is not
    /// in `doc`, no MIDI destination has its name, or no load interned it
    /// (no lane of the loaded sequence records its return).
    pub fn start_measure(
        &mut self,
        doc: &Document,
        dest: DestId,
        inputs: Vec<u16>,
    ) -> Result<Result<Arc<crate::measure::MeasureStatus>, String>, EngineError> {
        let Some(d) = doc.instruments().get(&dest) else {
            return Ok(Err(format!("destination {dest} is not in the project")));
        };
        let channel = match &d.kind {
            DestKind::MidiEndpoint { channel, .. } => channel & 0x0F,
            DestKind::PluginInstrument { .. } => 0,
        };
        let kinds = match kinds_for(&d.kind, self.cfg.route, &self.factory.ports()) {
            Ok(k) => k,
            Err(e) => return Ok(Err(e)),
        };
        let Some(target) = kinds.iter().find_map(|k| self.targets.find(k)) else {
            return Ok(Err(format!(
                "{} is not loaded: no lane of the loaded sequence records its return",
                d.name
            )));
        };
        let req = crate::measure::MeasureRequest {
            target,
            channel,
            inputs,
        };
        let (tap, status) = crate::measure::tap(&req, self.cfg.sample_rate);
        self.push(EngineCmd::Measure(Some(Box::new(tap))))?;
        Ok(Ok(status))
    }

    /// Removes the measurement: the block that applies this sends its
    /// sounding note's off, and its status reads closed.
    pub fn stop_measure(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Measure(None))
    }
}
```

- [ ] **Step 6: Run the tests and the rt-check gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test measure 2>&1 | tail -5
cargo test -p core-engine 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -3
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep -E "^test result|FAILED"
```
Expected: `measure` 4 pass; `core-engine` all pass; clean; rt tests pass.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/measure.rs crates/core-engine/src/lib.rs \
  crates/core-engine/src/render.rs crates/core-engine/src/engine.rs \
  crates/core-engine/tests/measure.rs
git commit -m "engine: measure a synth's return with the transport stopped (5c-2 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 6: `rt_alloc_record` — a real latency change, the measurement and the fake synth

**Files:**
- Modify: `crates/core-engine/tests/rt_alloc_record.rs`

**Interfaces:**
- Consumes: `TestInserts::plugin("latency=480")`, `FakeSynth`, `Engine::start_measure`/`stop_measure`/`set_return_dests` (Tasks 4–5).

The 5c final review (judgement 1) asked that the gate exercise a real latency change: a fixture graph with a latent node, swapped in mid-recording. This task does that (and swaps back), and adds the new real-time paths: a measurement with the transport stopped, its replacement and removal, and the fake synth answering while the part plays.

- [ ] **Step 1: Extend the test**

In `crates/core-engine/tests/rt_alloc_record.rs`:

Find:
```rust
//! Recording never allocates on the render path (plan 5c Task 5; spec
//! §3, §12.3 "`rt-check` covers … the input ring"): the counting
//! allocator is armed only around `OfflineBackend::block`, while a record
//! tap captures fake input through loop wraps, a full ring, tap swaps, an
//! xrun, a stop and the tap's removal.
```
Replace:
```rust
//! Recording never allocates on the render path (plan 5c Task 5; spec
//! §3, §12.3 "`rt-check` covers … the input ring"): the counting
//! allocator is armed only around `OfflineBackend::block`, while a record
//! tap captures fake input through loop wraps, a full ring, tap swaps, a
//! graph whose latency really changes and changes back (plan 5c-2 Task 6),
//! an xrun, a stop and the tap's removal; then while a synth's return is
//! measured with the transport stopped, the measurement is replaced and
//! removed, and the fake synth answers the part (plan 5c-2).
```

Find:
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
```
Replace:
```rust
use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, MonitorRoute, port_key};
use core_engine::midi1::Midi1Msg;
use core_engine::offline::{FAKE_SYNTH_PORT, FakeInput, FakeSynth, OfflineBackend, OfflineSinks};
use core_engine::record::RecordRequest;
use core_engine::rt_check::{self, CountingAlloc};
use core_model::command::{Command, LaneCmd, RoutingCmd};
use core_model::ids::AnyId;
use core_model::lane::MonitorMode;
use core_model::mixer::StripKind;
```

Find:
```rust
    let doc = bld.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
```
Replace:
```rust
    let doc = bld.finish();
    // The same document with a latent insert on the master: loaded
    // mid-recording and back, the graph latency really changes (5c final
    // review judgement 1).
    let mut lb = Builder::from_doc(doc.clone());
    let master = lb.strip_of(StripKind::Master).unwrap();
    lb.insert(master, TestInserts::plugin("latency=480")).unwrap();
    let latent = lb.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
```

Find:
```rust
    let before = rt_check::count();
    let mut buf = Vec::with_capacity(1 << 20);
```
Replace:
```rust
    let before = rt_check::count();
    let mut buf = Vec::with_capacity(1 << 20);
    // The graph latencies the marks carried (read between blocks).
    let mut latencies = std::collections::BTreeSet::new();
```

Find:
```rust
        if !(1000..1500).contains(&i) {
            while h.ring.pop(&mut buf).is_some() {}
            while h.marks.pop().is_ok() {}
        }
```
Replace:
```rust
        if !(1000..1500).contains(&i) {
            while h.ring.pop(&mut buf).is_some() {}
            while let Ok(m) = h.marks.pop() {
                latencies.insert(m.latency);
            }
        }
```

Find:
```rust
        if i == 1500 {
            // Routes change (a reload): thru notes end, the old routes
            // retire.
```
Replace:
```rust
        if i == 1200 {
            // A latent plugin comes in while recording: the graph swap
            // changes the latency and the tap marks it (final review I1).
            engine.load(&latent, seq).unwrap();
        }
        if i == 1500 {
            // Routes change (a reload, of the document without the latent
            // insert: the latency falls back): thru notes end, the old
            // routes retire.
```

Find:
```rust
    assert!(h.status.run() > 0);
```
Replace:
```rust
    assert!(h.status.run() > 0);
    assert!(
        latencies.contains(&0) && latencies.contains(&480),
        "the marks carried the latency change: {latencies:?}"
    );
```

Find:
```rust
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations on xrun and stop"
    );
    drop(b.stop().unwrap());
}
```
Replace:
```rust
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations on xrun and stop"
    );
    drop(b.stop().unwrap());

    // Plan 5c-2: a measurement pings the fake synth with the transport
    // stopped and watches its answer; it is replaced, then removed with a
    // note sounding, while the part plays through the synth.
    let mut sb = Builder::new();
    let fake = sb.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (fseq, flane) = sb.sequence("Synth", "Part", Some(fake)).unwrap();
    sb.notes(fseq, flane, &[(0, 60, 0x8000, Q), (2 * Q, 64, 0x8000, Q)])
        .unwrap();
    let sdoc = sb.finish();
    let synth = FakeSynth::new(25.0);
    let (mut se, sr) = Engine::new(
        EngineConfig::new(48_000.0),
        Box::new(OfflineSinks::new(1 << 16).with_fake_synth(&synth)),
    )
    .unwrap();
    se.set_return_dests(vec![fake]);
    se.load(&sdoc, fseq).unwrap();
    let mut sbk = OfflineBackend::new(48_000.0, 512)
        .with_input(FakeInput::Signals)
        .with_synth(&synth);
    sbk.start(sr).unwrap();
    sbk.reserve(4096);
    let status = se.start_measure(&sdoc, fake, vec![2]).unwrap().unwrap();
    let before = rt_check::count();
    for _ in 0..400 {
        armed_block(&mut sbk, 512);
        if status.done() {
            break;
        }
    }
    assert!(status.done(), "the measurement finished");
    assert!(status.onset(0).is_some(), "the synth answered");
    se.play().unwrap();
    let again = se.start_measure(&sdoc, fake, vec![2]).unwrap().unwrap();
    // Its first note goes out after the 100 ms lead and sounds 100 ms:
    // removed 15 blocks in, the note is sounding.
    for _ in 0..15 {
        armed_block(&mut sbk, 512);
    }
    se.stop_measure().unwrap();
    armed_block(&mut sbk, 512);
    assert!(status.closed() && again.closed());
    for _ in 0..100 {
        armed_block(&mut sbk, 512);
    }
    assert_eq!(
        rt_check::count() - before,
        0,
        "allocations while measuring and while the fake synth plays"
    );
    drop(sbk.stop().unwrap());
}
```

- [ ] **Step 2: Run the rt-check gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep -E "^test result|FAILED|panicked"
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -3
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: the three pass. If `rt_alloc_record` reports allocations, the path that allocated is new in Task 4 or 5: find it (run the test under the debugger with a breakpoint in `CountingAlloc::alloc` while armed, or bisect the loop) and fix that path; never loosen the assertion. The second measurement replaces the first (finished but still installed: it is retired) and is removed 15 blocks in, with its first note sounding (sent at block 10, after the 100 ms lead, for 100 ms), so `release` sends the off.

- [ ] **Step 3: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/tests/rt_alloc_record.rs
git commit -m "rt-check: a real latency change, Measure and the fake synth never allocate (5c-2 Task 6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 7: `core-ffi` host — the fake synth and the return destinations

**Files:**
- Modify: `crates/core-ffi/src/host.rs` (`fake_synth`, `set_fake_synth`, offline hosts with the synth, `return_dests` at every load)
- Test: `crates/core-ffi/tests/synth.rs` (new)

**Interfaces:**
- Consumes: `FakeSynth`, `OfflineSinks::with_fake_synth`, `OfflineBackend::with_synth` (Task 4); `Engine::set_return_dests` (Task 5).
- Produces: `vizdaw_ffi::host::fake_synth() -> Option<f64>` (`VIZDAW_FAKE_SYNTH`, 0–500 ms); `#[cfg(feature = "test-hooks")] vizdaw_ffi::host::set_fake_synth(Option<f64>)`; `vizdaw_ffi::host::return_dests(doc: &Document, seq: SeqId) -> Vec<DestId>`.

Spec §12.1; Rulings 7 and 10.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/synth.rs <<'VIZDAW_EOF'
//! Tracking a hardware synth through a session (plan 5c-2; sub-project 5
//! spec §6.7, §12.3 "Synth tracking"): the offline device's fake synth
//! answers notes sent to "Fake Synth" on fake input channel 3, a
//! `return_of` lane's recording lands on the notes that played it, Track
//! Synth sets up in one undo step, a loop pass makes MIDI and audio takes
//! together, and Measure finds the fake delay with the transport stopped.
//! Offline only: no audio device is opened and no microphone asked.

mod common;

use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{FAKE_SYNTH_PORT, FakeInput};
use core_media::caf::CafReader;
use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
use core_model::document::Document;
use core_model::ids::{AnyId, DestId, LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput, MonitorMode};
use core_model::sequence::SeqLength;
use core_model::time::Tick;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host, set_fake_input, set_fake_synth};
use vizdaw_ffi::{EventRow, RecordSpecRow, Session};

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time, with its own folder standing for the Trash.
struct Serial {
    _g: MutexGuard<'static, ()>,
    dir: tempfile::TempDir,
}

/// Offline, with the fake synth answering `delay_ms` late (`None`: no
/// synth) and the fake input `input` (`None`: the signals input the synth
/// brings).
fn serial(delay_ms: Option<f64>, input: Option<FakeInput>) -> Serial {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
    set_fake_synth(delay_ms);
    vizdaw_ffi::record::set_free_bytes(None);
    reset_host();
    Serial {
        _g: g,
        dir: tempfile::tempdir().unwrap(),
    }
}

fn untitled(g: &Serial, doc: Document) -> Arc<Session> {
    let s = Session::from_document(doc).unwrap();
    s.debug_set_trash_dir(Some(g.dir.path().join("Trash").display().to_string()));
    s
}

/// 120 BPM at 48 kHz: one beat (Q ticks) is 24 000 frames; 1 ms is 48.
const BEAT: i64 = 24_000;
const MS: i64 = 48;
/// The fake synth's delay in these tests.
const DELAY_MS: f64 = 25.0;

#[allow(dead_code)]
struct Doc {
    doc: Document,
    seq: SeqId,
    part: LaneId,
    rec: LaneId,
    synth: DestId,
}

/// Sequence "S" (`bars` bars locked, 120 BPM): lane "Part" plays the
/// external "Fake Synth" with notes at beats 1, 2 and 3 (an eighth each)
/// and listens to MIDI from "Keys", unarmed; lane "Rec" records fake
/// input 3 (as the synth's return when `return_of`), armed, monitor Off,
/// on output "Audio 1".
fn doc(return_of: bool, bars: i64) -> Doc {
    let mut b = Builder::new();
    let synth = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let (seq, part) = b.sequence("S", "Part", Some(synth)).unwrap();
    b.notes(
        seq,
        part,
        &[
            (Q, 60, 0x8000, Q / 2),
            (2 * Q, 62, 0x8000, Q / 2),
            (3 * Q, 64, 0x8000, Q / 2),
        ],
    )
    .unwrap();
    let rec = b.lane(seq, "Rec", None).unwrap();
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
        LaneCmd::SetInput {
            seq,
            lane: part,
            input: Some(LaneInput::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
        LaneCmd::SetOutput {
            seq,
            lane: rec,
            output: Some(out),
        },
        LaneCmd::SetInput {
            seq,
            lane: rec,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(2),
                return_of: return_of.then_some(synth),
            }),
        },
        LaneCmd::SetMonitor {
            seq,
            lane: rec,
            monitor: MonitorMode::Off,
        },
        LaneCmd::SetFlags {
            seq,
            lane: rec,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    ] {
        b.run(Command::Lane(c)).unwrap();
    }
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(bars * 4 * Q)),
    }))
    .unwrap();
    Doc {
        doc: b.finish(),
        seq,
        part,
        rec,
        synth,
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

fn frames(s: &Session) -> i64 {
    s.record_state().unwrap().frames as i64
}

fn regions(s: &Session, seq: u64, lane: u64) -> Vec<EventRow> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.media.is_some())
        .collect()
}

/// Where each click in `lane`'s one region landed on the timeline, in
/// frames: a sample above 0.3 after at least 5 ms below it (the synth's
/// tone stays under 0.1).
fn clicks(s: &Session, seq: u64, lane: u64) -> Vec<i64> {
    let r = regions(s, seq, lane);
    assert_eq!(r.len(), 1, "{r:?}");
    let path = s.media_file(r[0].media.unwrap()).unwrap();
    let reader = CafReader::open(Path::new(&path)).unwrap();
    let info = reader.info().clone();
    let mut data = vec![0.0f32; info.frames as usize * usize::from(info.channels)];
    reader.read(0, &mut data, &mut Vec::new()).unwrap();
    let from = r[0].source_offset as usize;
    let mut out = Vec::new();
    let mut quiet = 0usize;
    for (k, x) in data.iter().enumerate().skip(from) {
        if x.abs() > 0.3 {
            if quiet >= 240 {
                out.push(r[0].tick * BEAT / Q + (k - from) as i64);
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    out
}

fn record_to_the_third_note(s: &Session, seq: SeqId) {
    s.record(seq.0, spec()).unwrap();
    wait_until("the third note's answer", || {
        frames(s) >= 3 * BEAT + 60 * MS
    });
    s.stop().unwrap();
}

/// Spec §12.1: the notes the sequence plays to "Fake Synth" come back on
/// fake input channel 3 the synth's delay after they were heard: a lane
/// recording channel 3 that is not linked to the synth takes the clicks
/// 25 ms late.
#[test]
fn the_fake_synth_answers_on_channel_3_its_delay_late() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(false, 2);
    let s = untitled(&g, d.doc.clone());
    record_to_the_third_note(&s, d.seq);
    let got = clicks(&s, d.seq.0, d.rec.0);
    assert_eq!(got.len(), 3, "{got:?}");
    for (at, note) in got.iter().zip([BEAT, 2 * BEAT, 3 * BEAT]) {
        let want = note + 25 * MS;
        assert!((at - want).abs() <= 2, "a click landed at {at}, not {want}");
    }
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 7: with the loopback input the synth stays silent.
#[test]
fn with_the_loopback_input_the_fake_synth_is_silent() {
    let g = serial(Some(DELAY_MS), Some(FakeInput::Loopback(1000)));
    let d = doc(false, 2);
    let s = untitled(&g, d.doc.clone());
    record_to_the_third_note(&s, d.seq);
    assert!(clicks(&s, d.seq.0, d.rec.0).is_empty());
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 10: the host names the return lanes' synths at every
/// load.
#[test]
fn the_return_lanes_synths_are_named_at_load() {
    let d = doc(true, 2);
    assert_eq!(vizdaw_ffi::host::return_dests(&d.doc, d.seq), vec![d.synth]);
    let plain = doc(false, 2);
    assert!(vizdaw_ffi::host::return_dests(&plain.doc, plain.seq).is_empty());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test synth 2>&1 | tail -10
```
Expected: compile errors (`set_fake_synth`, `return_dests` unknown).

- [ ] **Step 3: Implement**

In `crates/core-ffi/src/host.rs`:

Find:
```rust
use core_engine::offline::{EventLog, FakeInput, OfflineBackend, OfflineSinks};
```
Replace:
```rust
use core_engine::offline::{EventLog, FakeInput, FakeSynth, OfflineBackend, OfflineSinks};
```

Find:
```rust
/// Makes offline hosts created from now on use `input` (test hook; it
/// wins over the environment).
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}
```
Replace:
```rust
/// Makes offline hosts created from now on use `input` (test hook; it
/// wins over the environment).
#[cfg(feature = "test-hooks")]
pub fn set_fake_input(input: Option<FakeInput>) {
    *lock(&FAKE_INPUT) = Some(input);
}

/// The fake synth offline hosts get: `Some(choice)` set by the
/// `set_fake_synth` test hook, `None` read from the environment.
static FAKE_SYNTH: Mutex<Option<Option<f64>>> = Mutex::new(None);

/// The fake synth of offline hosts created from now on (spec §12.1;
/// plan 5c-2 Ruling 7): `VIZDAW_FAKE_SYNTH=<delay ms>` (0–500; anything
/// else is ignored) adds the MIDI destination "Fake Synth", which answers
/// every note on fake input channel 3 that long after it is heard. Tests
/// choose with `set_fake_synth`. Live hosts never have it.
pub fn fake_synth() -> Option<f64> {
    if let Some(v) = *lock(&FAKE_SYNTH) {
        return v;
    }
    let max = f64::from(core_model::document::MAX_RETURN_LATENCY_MS);
    std::env::var("VIZDAW_FAKE_SYNTH")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|ms| ms.is_finite() && (0.0..=max).contains(ms))
}

/// Makes offline hosts created from now on have the fake synth answering
/// `delay_ms` late, or none (test hook; it wins over the environment).
#[cfg(feature = "test-hooks")]
pub fn set_fake_synth(delay_ms: Option<f64>) {
    *lock(&FAKE_SYNTH) = Some(delay_ms);
}
```

Find:
```rust
fn next_epoch() -> u64 {
    EPOCHS.fetch_add(1, Ordering::Relaxed)
}
```
Replace:
```rust
/// The external synths whose return the lanes of `seq` record (spec
/// §6.7): the engine interns them at every load, so Measure can play them
/// (plan 5c-2 Ruling 10).
pub fn return_dests(doc: &Document, seq: SeqId) -> Vec<DestId> {
    let mut out: Vec<DestId> = Vec::new();
    if let Ok(s) = doc.sequence(seq) {
        for d in s
            .lanes
            .iter()
            .filter_map(|l| l.input.as_ref().and_then(LaneInput::return_of))
        {
            if !out.contains(&d) {
                out.push(d);
            }
        }
    }
    out
}

fn next_epoch() -> u64 {
    EPOCHS.fetch_add(1, Ordering::Relaxed)
}
```

Find:
```rust
        if offline_mode() {
            let sinks = OfflineSinks::new(1 << 16);
            let log = sinks.log();
            let rate = offline_rate(project);
            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
            let (driver, inputs) = OfflineDriver::start(renderer, rate, fake_input())?;
```
Replace:
```rust
        if offline_mode() {
            // The fake synth (spec §12.1) answers on the signals input's
            // channel 3, so it brings that input when none was chosen
            // (plan 5c-2 Ruling 7).
            let synth = fake_synth().map(FakeSynth::new);
            let mut sinks = OfflineSinks::new(1 << 16);
            if let Some(s) = &synth {
                sinks = sinks.with_fake_synth(s);
            }
            let input = match (fake_input(), &synth) {
                (None, Some(_)) => Some(FakeInput::Signals),
                (i, _) => i,
            };
            let log = sinks.log();
            let rate = offline_rate(project);
            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
            let (driver, inputs) = OfflineDriver::start(renderer, rate, input, synth.as_ref())?;
```

Find:
```rust
    fn start(
        renderer: Renderer,
        rate: f64,
        input: Option<FakeInput>,
    ) -> Result<(OfflineDriver, InputInfo), VizError> {
        let mut backend = OfflineBackend::new(rate, OFFLINE_BLOCK);
        if let Some(k) = input {
            backend = backend.with_input(k);
        }
```
Replace:
```rust
    fn start(
        renderer: Renderer,
        rate: f64,
        input: Option<FakeInput>,
        synth: Option<&FakeSynth>,
    ) -> Result<(OfflineDriver, InputInfo), VizError> {
        let mut backend = OfflineBackend::new(rate, OFFLINE_BLOCK);
        if let Some(k) = input {
            backend = backend.with_input(k);
        }
        if let Some(s) = synth {
            backend = backend.with_synth(s);
        }
```

Find:
```rust
        self.engine.set_inputs(input_spec(doc, seq));
        let report = self.engine.load(doc, seq)?;
```
Replace:
```rust
        self.engine.set_inputs(input_spec(doc, seq));
        self.engine.set_return_dests(return_dests(doc, seq));
        let report = self.engine.load(doc, seq)?;
```

`grep -n "OfflineDriver::start" crates/core-ffi/src/host.rs` must list only the call changed above; if another call exists, pass it `None` for the synth.

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test synth 2>&1 | tail -5
cargo test -p core-ffi --test record --test live_input 2>&1 | grep -E "^test result|FAILED"
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -3
scripts/build-ffi.sh 2>&1 | tail -2 && scripts/swift.sh build 2>&1 | tail -2 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: `synth` 3 pass; `record` and `live_input` unchanged; clean; Swift builds and passes (no FFI change yet).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/host.rs crates/core-ffi/tests/synth.rs
git commit -m "host: the offline fake synth and the return lanes' synths at every load (5c-2 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 8: `core-ffi` session — Track Synth, return latency, the shift, mute after

**Files:**
- Create: `crates/core-ffi/src/synth.rs` (`return_frames`, `return_lane_in`, `mutes_after`)
- Modify: `crates/core-ffi/src/lib.rs` (`pub mod synth`)
- Modify: `crates/core-ffi/src/edit.rs` (`LaneInputRow::Audio.return_of`)
- Modify: `crates/core-ffi/src/mixer.rs` (`DestRow.external`, `DestRow.return_latency_ms`)
- Modify: `crates/core-ffi/src/record.rs` (`AudioLane.return_frames`, `build` passes it on)
- Modify: `crates/core-ffi/src/session.rs` (`destinations`, Record reads the shift, `synth_mutes`, the pass's mute, `track_synth`, `set_return_latency`)
- Modify: `crates/core-ffi/src/control.rs` (`lane_input_json` pattern; Task 10 writes the field)
- Modify (the new field): `crates/core-ffi/tests/live_input.rs`, `crates/core-ffi/tests/control.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Tests/VizDAWAppModelTests/RecordModelTests.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/synth.rs` (appended), `synth.rs` unit tests

**Interfaces:**
- Consumes: `LaneCmd::TrackSynth`, `RoutingCmd::SetReturnLatency` (Tasks 2–3); `AudioTake.return_frames` (Task 1).
- Produces:
  - `LaneInputRow::Audio { first: u16, stereo: bool, return_of: Option<u64> }` (Swift `.audio(first:stereo:returnOf:)`);
  - `DestRow { id, name, plugin, external: bool, return_latency_ms: f32 }` (Swift defaults `external: false, returnLatencyMs: 0`);
  - `Session.track_synth(seq: u64, lane: u64, first: u16, stereo: bool, arm_midi: bool, mute_after: bool) -> Result<DocSnapshot, VizError>`; `Session.set_return_latency(dest: u64, ms: f32) -> Result<DocSnapshot, VizError>`;
  - `vizdaw_ffi::synth::return_frames(doc, dest: Option<DestId>, rate: f64) -> i64`, `return_lane_in(doc, seq, dest, except: LaneId) -> Option<LaneId>`, `mutes_after(pass, mutes) -> Vec<((SeqId, LaneId), SeqId, LaneId)>`;
  - `vizdaw_ffi::record::AudioLane.return_frames: i64`.

Spec §6.7 "Setup", "One pass", "Alignment", "After tracking"; Rulings 4, 5, 6, 14.

- [ ] **Step 1: Write the failing tests**

In `crates/core-ffi/tests/synth.rs`:

Find:
```rust
use vizdaw_ffi::{EventRow, RecordSpecRow, Session};
```
Replace:
```rust
use vizdaw_ffi::{EventRow, LaneInputRow, RecordSpecRow, Session};
```

Then append:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/synth.rs <<'VIZDAW_EOF'

/// Spec §12.3: one pass with an armed `return_of` lane — the clicks land
/// on the MIDI note starts within ±1 ms, with `return_latency_ms` set to
/// the fake delay.
#[test]
fn a_return_lane_lands_on_the_notes_that_played_it() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    s.set_return_latency(d.synth.0, DELAY_MS as f32).unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Set Return Latency")
    );
    let dest = s
        .destinations()
        .unwrap()
        .into_iter()
        .find(|x| x.id == d.synth.0)
        .unwrap();
    assert!(dest.external);
    assert_eq!(dest.return_latency_ms, DELAY_MS as f32);
    record_to_the_third_note(&s, d.seq);
    let got = clicks(&s, d.seq.0, d.rec.0);
    assert_eq!(got.len(), 3, "{got:?}");
    for (at, note) in got.iter().zip([BEAT, 2 * BEAT, 3 * BEAT]) {
        assert!(
            (at - note).abs() <= MS,
            "a click landed at {at}, its note at {note}"
        );
    }
    // The region starts where recording did, its file offset taking the
    // synth's latency.
    let r = regions(&s, d.seq.0, d.rec.0);
    assert_eq!((r[0].tick, r[0].source_offset), (0, 25 * MS as u64));
    s.close().unwrap();
}

/// Spec §6.7, §12.3: Track Synth sets up and arms in one undo step; run
/// again it reuses the lane; it refuses what it cannot set up and
/// changes nothing then.
#[test]
fn track_synth_sets_up_and_arms_in_one_undo_step() {
    let g = serial(Some(DELAY_MS), None);
    let mut b = Builder::new();
    let synth = b.dest(FAKE_SYNTH_PORT, FAKE_SYNTH_PORT, 0).unwrap();
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let (seq, part) = b.sequence("S", "Part", Some(synth)).unwrap();
    let keys = b.lane(seq, "Keys", Some(piano)).unwrap();
    let s = untitled(&g, b.finish());
    s.track_synth(seq.0, part.0, 2, false, false, false).unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Track Synth"));
    let lanes = s.lanes(seq.0).unwrap();
    let names: Vec<&str> = lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Part", "Fake Synth audio", "Keys"]);
    assert_eq!(
        lanes[1].input,
        Some(LaneInputRow::Audio {
            first: 2,
            stereo: false,
            return_of: Some(synth.0),
        })
    );
    assert!(lanes[1].arm && !lanes[0].arm);
    assert!(lanes[1].audio_out.is_some());
    s.track_synth(seq.0, part.0, 4, true, true, false).unwrap();
    let lanes = s.lanes(seq.0).unwrap();
    assert_eq!(lanes.len(), 3, "the same lane, set up again");
    assert_eq!(
        lanes[1].input,
        Some(LaneInputRow::Audio {
            first: 4,
            stereo: true,
            return_of: Some(synth.0),
        })
    );
    assert!(lanes[0].arm);
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.lanes(seq.0).unwrap().len(), 2);
    let v = s.snapshot().unwrap().version;
    for (lane, first) in [(keys.0, 0u16), (part.0, 300), (999, 0)] {
        assert!(
            s.track_synth(seq.0, lane, first, false, false, false).is_err(),
            "lane {lane}, channel {first}"
        );
    }
    assert!(s.track_synth(999, part.0, 0, false, false, false).is_err());
    assert_eq!(s.snapshot().unwrap().version, v, "nothing changed");
    s.close().unwrap();
}

/// Spec §6.7, §12.3: with the MIDI lane armed too, a loop pass makes MIDI
/// and audio takes together; "Mute the MIDI lane after recording" mutes
/// it in the same "Record" step (plan 5c-2 Ruling 6).
#[test]
fn a_loop_pass_makes_midi_and_audio_takes_together_and_mutes_after() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 1);
    let s = untitled(&g, d.doc.clone());
    s.set_return_latency(d.synth.0, DELAY_MS as f32).unwrap();
    s.track_synth(d.seq.0, d.part.0, 2, false, true, true).unwrap();
    let mut spec = spec();
    spec.looped = true;
    s.record(d.seq.0, spec).unwrap();
    // One bar is 96 000 frames: a note in pass 1, then a beat and a half
    // into pass 2.
    wait_until("a little of pass 1", || frames(&s) >= 12_000);
    assert!(s.inject_midi("Keys".into(), vec![0x90, 70, 100]).unwrap());
    wait_until("an eighth on", || frames(&s) >= 24_000);
    assert!(s.inject_midi("Keys".into(), vec![0x80, 70, 0]).unwrap());
    wait_until("pass 2", || {
        let r = s.record_state().unwrap();
        r.pass >= 2 && frames(&s) >= 96_000 + 36_000
    });
    s.stop().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Record"));
    let lane = |id: LaneId| {
        s.lanes(d.seq.0)
            .unwrap()
            .into_iter()
            .find(|l| l.id == id.0)
            .unwrap()
    };
    let (part, rec) = (lane(d.part), lane(d.rec));
    assert_eq!(part.take_count, rec.take_count, "matching takes");
    assert_eq!(rec.take_count, 3, "one take per pass, besides the first");
    assert!(part.mute, "muted after recording");
    s.undo().unwrap();
    let (part, rec) = (lane(d.part), lane(d.rec));
    assert!(!part.mute, "the mute is part of the Record step");
    assert_eq!((part.take_count, rec.take_count), (1, 1));
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 2 over the FFI: return latencies outside 0–500 ms,
/// not finite, or for a destination that is not an external synth are
/// refused and change nothing.
#[test]
fn hostile_return_latencies_are_refused() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let mut b = Builder::from_doc(d.doc.clone());
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let s = untitled(&g, b.finish());
    let v = s.snapshot().unwrap().version;
    for (dest, ms) in [
        (d.synth.0, -1.0f32),
        (d.synth.0, 500.5),
        (d.synth.0, f32::NAN),
        (d.synth.0, f32::INFINITY),
        (piano.0, 10.0),
        (999, 10.0),
    ] {
        assert!(s.set_return_latency(dest, ms).is_err(), "{dest} {ms}");
    }
    assert_eq!(s.snapshot().unwrap().version, v);
    let dests = s.destinations().unwrap();
    assert!(dests.iter().all(|x| x.return_latency_ms == 0.0));
    assert!(dests.iter().any(|x| x.id == piano.0 && !x.external));
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test synth 2>&1 | tail -10
```
Expected: compile errors (`track_synth`, `set_return_latency`, `return_of` on `LaneInputRow` unknown).

- [ ] **Step 3: The synth helpers**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/synth.rs <<'VIZDAW_EOF'
//! Tracking a hardware synth (sub-project 5 spec §6.7; plan 5c-2): the
//! shift a `return_of` lane's recording gets, a synth's return lane, and
//! the mutes "Mute the MIDI lane after recording" owes a pass. Control
//! side only.

use core_model::command::RecordPass;
use core_model::document::Document;
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::lane::LaneInput;
use std::collections::BTreeMap;

/// Frames a lane recording `dest`'s return moves earlier (spec §6.7; plan
/// 5c-2 Ruling 4): the destination's return latency at `rate`, while it
/// is an external MIDI destination; else 0.
pub fn return_frames(doc: &Document, dest: Option<DestId>, rate: f64) -> i64 {
    dest.and_then(|d| doc.instruments().get(&d))
        .filter(|d| d.kind.is_external())
        .map_or(0, |d| {
            (f64::from(d.return_latency_ms) * rate / 1000.0).round() as i64
        })
}

/// The first lane of `seq` other than `except` recording `dest`'s return
/// (the one Track Synth sets up and reuses).
pub fn return_lane_in(doc: &Document, seq: SeqId, dest: DestId, except: LaneId) -> Option<LaneId> {
    doc.sequence(seq)
        .ok()?
        .lanes
        .iter()
        .find(|l| l.id != except && l.input.as_ref().and_then(LaneInput::return_of) == Some(dest))
        .map(|l| l.id)
}

/// What a pass owes "Mute the MIDI lane after recording" (plan 5c-2
/// Ruling 6): for every return lane in `mutes` (keyed by sequence and
/// return lane, naming the MIDI lane) that the pass placed audio on, the
/// key and the MIDI lane to mute.
pub fn mutes_after(
    pass: &RecordPass,
    mutes: &BTreeMap<(SeqId, LaneId), LaneId>,
) -> Vec<((SeqId, LaneId), SeqId, LaneId)> {
    pass.lanes
        .iter()
        .filter(|l| {
            !l.active.regions.is_empty() || l.takes.iter().any(|(_, c)| !c.regions.is_empty())
        })
        .filter_map(|l| {
            mutes
                .get(&(l.seq, l.lane))
                .map(|midi| ((l.seq, l.lane), l.seq, *midi))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_engine::fixtures::Builder;
    use core_model::command::{
        Command, LaneRecord, RecordedContent, RecordedRegion, RoutingCmd,
    };
    use core_model::document::DestKind;
    use core_model::time::Tick;

    /// Review Focus 3: only an external synth's latency shifts a take.
    #[test]
    fn a_return_shift_needs_an_external_destination() {
        let mut b = Builder::new();
        let juno = b.dest("Juno", "Juno", 0).unwrap();
        let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
        let pad = b.dest_kind("Pad", DestKind::plugin("au:pad"), None).unwrap();
        b.run(Command::Routing(RoutingCmd::SetReturnLatency {
            dest: juno,
            ms: 10.0,
        }))
        .unwrap();
        let doc = b.finish();
        assert_eq!(return_frames(&doc, Some(juno), 48_000.0), 480);
        assert_eq!(return_frames(&doc, Some(juno), 44_100.0), 441);
        for d in [Some(piano), Some(pad), Some(DestId(99)), None] {
            assert_eq!(return_frames(&doc, d, 48_000.0), 0, "{d:?}");
        }
    }

    #[test]
    fn a_pass_owes_the_mutes_of_the_return_lanes_it_placed_audio_on() {
        let region = RecordedRegion {
            media: 0,
            tick: Tick(0),
            source_offset: 0,
            length: 10,
        };
        let content = |regions: Vec<RecordedRegion>| RecordedContent {
            events: Vec::new(),
            regions,
        };
        let lane = |l: u64, active: RecordedContent, takes: Vec<(String, RecordedContent)>| {
            LaneRecord {
                seq: SeqId(1),
                lane: LaneId(l),
                clear: Vec::new(),
                active,
                takes,
            }
        };
        let pass = RecordPass {
            with_gaps: false,
            media: Vec::new(),
            lanes: vec![
                lane(5, content(vec![region]), Vec::new()),
                lane(6, RecordedContent::default(), vec![("Take 2".into(), content(vec![region]))]),
                lane(7, RecordedContent::default(), Vec::new()),
            ],
        };
        let mutes = BTreeMap::from([
            ((SeqId(1), LaneId(5)), LaneId(1)),
            ((SeqId(1), LaneId(6)), LaneId(2)),
            ((SeqId(1), LaneId(7)), LaneId(3)),
        ]);
        assert_eq!(
            mutes_after(&pass, &mutes),
            vec![
                ((SeqId(1), LaneId(5)), SeqId(1), LaneId(1)),
                ((SeqId(1), LaneId(6)), SeqId(1), LaneId(2)),
            ]
        );
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs` — Find:
```rust
pub mod strip;
pub mod templates;
```
Replace:
```rust
pub mod strip;
pub mod synth;
pub mod templates;
```

- [ ] **Step 4: Rows, the shift and the session calls**

In `crates/core-ffi/src/edit.rs`:

Find:
```rust
    Audio {
        /// The first (left) channel.
        first: u16,
        /// A pair.
        stereo: bool,
    },
```
Replace:
```rust
    Audio {
        /// The first (left) channel.
        first: u16,
        /// A pair.
        stereo: bool,
        /// The external MIDI destination whose return the lane records
        /// (spec §4.5, §6.7); none: an ordinary input.
        return_of: Option<u64>,
    },
```

Find:
```rust
            LaneInputRow::Audio { first, stereo } => LaneInput::Audio {
```
Replace:
```rust
            LaneInputRow::Audio {
                first,
                stereo,
                return_of,
            } => LaneInput::Audio {
```

Find:
```rust
                // Task 8 of plan 5c-2 carries it over the FFI.
                return_of: None,
```
Replace:
```rust
                return_of: return_of.map(DestId),
```

Find:
```rust
            LaneInput::Audio { channels, .. } => LaneInputRow::Audio {
                first: channels.first(),
                stereo: matches!(channels, InputChannels::Stereo(_)),
            },
```
Replace:
```rust
            LaneInput::Audio {
                channels,
                return_of,
            } => LaneInputRow::Audio {
                first: channels.first(),
                stereo: matches!(channels, InputChannels::Stereo(_)),
                return_of: return_of.map(|d| d.0),
            },
```

In `crates/core-ffi/src/control.rs` — Find:
```rust
        crate::edit::LaneInputRow::Audio { first, stereo } => {
```
Replace:
```rust
        crate::edit::LaneInputRow::Audio { first, stereo, .. } => {
```

In `crates/core-ffi/src/mixer.rs` — Find:
```rust
/// A destination of the instrument list (lanes play through these).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DestRow {
    /// Destination ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// The plugin instrument it plays, if it is one.
    pub plugin: Option<crate::snapshot::PluginRefRow>,
}
```
Replace:
```rust
/// A destination of the instrument list (lanes play through these).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DestRow {
    /// Destination ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// The plugin instrument it plays, if it is one.
    pub plugin: Option<crate::snapshot::PluginRefRow>,
    /// An external MIDI destination: Track Synth and Measure work with it
    /// (spec §6.7; plan 5c-2 Ruling 2).
    #[uniffi(default)]
    pub external: bool,
    /// Its return latency in ms (spec §4.5a).
    #[uniffi(default)]
    pub return_latency_ms: f32,
}
```

In `crates/core-ffi/src/record.rs`:

Find:
```rust
    /// Package-relative path.
    pub path: String,
    /// Channels in the file (1 or 2).
    pub channels: u16,
}
```
Replace:
```rust
    /// Package-relative path.
    pub path: String,
    /// Channels in the file (1 or 2).
    pub channels: u16,
    /// How much earlier its take lands: the return latency of the synth
    /// it records, in frames (spec §6.7; plan 5c-2 Ruling 4), read at
    /// Record.
    pub return_frames: i64,
}
```

Find:
```rust
            // Task 8 of plan 5c-2 gives a `return_of` lane its shift.
            return_frames: 0,
```
Replace:
```rust
            return_frames: a.return_frames,
```

Find:
```rust
        let lane = |lane: LaneId, name: &str| AudioLane {
            lane,
            name: name.into(),
            path: format!("{RECORDED_DIR}/{name}.caf"),
            channels: 1,
        };
```
Replace:
```rust
        let lane = |lane: LaneId, name: &str| AudioLane {
            lane,
            name: name.into(),
            path: format!("{RECORDED_DIR}/{name}.caf"),
            channels: 1,
            return_frames: 0,
        };
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// Recordings a crash left (and kept takes), offered and not yet
    /// answered (plan 5c Task 10): the Trash leaves them until they are.
    recovered: Vec<crate::record::RecoveredFileRow>,
}
```
Replace:
```rust
    /// Recordings a crash left (and kept takes), offered and not yet
    /// answered (plan 5c Task 10): the Trash leaves them until they are.
    recovered: Vec<crate::record::RecoveredFileRow>,
    /// Track Synth's "Mute the MIDI lane after recording" (plan 5c-2
    /// Ruling 6): by (sequence, return lane), the MIDI lane the next pass
    /// that places audio there mutes. Runtime only.
    synth_mutes: std::collections::BTreeMap<(SeqId, LaneId), LaneId>,
}
```

Find:
```rust
            record_faults: Vec::new(),
            recovered: Vec::new(),
        }
```
Replace:
```rust
            record_faults: Vec::new(),
            recovered: Vec::new(),
            synth_mutes: Default::default(),
        }
```

Find:
```rust
                .map(|d| DestRow {
                    id: d.id.0,
                    name: d.name.clone(),
                    plugin: match &d.kind {
                        DestKind::PluginInstrument { slot } => Some((&slot.plugin).into()),
                        DestKind::MidiEndpoint { .. } => None,
                    },
                })
```
Replace:
```rust
                .map(|d| DestRow {
                    id: d.id.0,
                    name: d.name.clone(),
                    plugin: match &d.kind {
                        DestKind::PluginInstrument { slot } => Some((&slot.plugin).into()),
                        DestKind::MidiEndpoint { .. } => None,
                    },
                    external: d.kind.is_external(),
                    return_latency_ms: d.return_latency_ms,
                })
```

Find:
```rust
        let mut audio: Vec<(
            core_model::ids::LaneId,
            String,
            core_model::lane::InputChannels,
        )> = Vec::new();
        let mut midi_lanes = Vec::new();
        for l in s.lanes.iter().filter(|l| l.arm) {
            match &l.input {
                Some(LaneInput::Audio { channels, .. }) => {
                    audio.push((l.id, l.name.clone(), *channels))
                }
```
Replace:
```rust
        // Each armed audio lane with the synth whose return it records.
        let mut audio: Vec<(
            core_model::ids::LaneId,
            String,
            core_model::lane::InputChannels,
            Option<DestId>,
        )> = Vec::new();
        let mut midi_lanes = Vec::new();
        for l in s.lanes.iter().filter(|l| l.arm) {
            match &l.input {
                Some(LaneInput::Audio {
                    channels,
                    return_of,
                }) => audio.push((l.id, l.name.clone(), *channels, *return_of)),
```

Find:
```rust
        for (_, name, ch) in &audio {
```
Replace:
```rust
        for (_, name, ch, _) in &audio {
```

Find:
```rust
            .flat_map(|(_, _, ch)| {
```
Replace:
```rust
            .flat_map(|(_, _, ch, _)| {
```

Find:
```rust
        for (lane, name, ch) in &audio {
```
Replace:
```rust
        for (lane, name, ch, ret) in &audio {
```

Find:
```rust
            lanes.push(AudioLane {
                lane: *lane,
                name: name.clone(),
                path: rel,
                channels: ch.count(),
            });
```
Replace:
```rust
            lanes.push(AudioLane {
                lane: *lane,
                name: name.clone(),
                path: rel,
                channels: ch.count(),
                // Read now: an edit during the pass applies from the next
                // one (plan 5c-2 Ruling 4).
                return_frames: crate::synth::return_frames(&doc, *ret, rate),
            });
```

Find:
```rust
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
```
Replace:
```rust
        if let Some(p) = pass {
            // Track Synth's "Mute the MIDI lane after recording" (spec
            // §6.7): inside the group, so the mute is part of "Record"
            // (plan 5c-2 Ruling 6).
            let mutes = crate::synth::mutes_after(&p, &inner.synth_mutes);
            match inner
                .history
                .execute(&Command::Record(p), &ViewFilter::default())
            {
                Err(e) => {
                    inner.news.problems.push(format!(
                        "The recording could not be added ({e}). Its files are kept."
                    ));
                    inner.keep_recordings.extend(files);
                }
                Ok(_) => {
                    for (key, seq, lane) in mutes {
                        inner.synth_mutes.remove(&key);
                        let mute = Command::Lane(LaneCmd::SetFlags {
                            seq,
                            lane,
                            mute: Some(true),
                            solo: None,
                            arm: None,
                        });
                        if let Err(e) = inner.history.execute(&mute, &ViewFilter::default()) {
                            inner.news.problems.push(format!(
                                "The MIDI lane could not be muted after recording ({e})."
                            ));
                        }
                    }
                }
            }
        }
        let _ = inner.history.end_group(label);
```

Find:
```rust
/// Live input (plan 5c Task 8; spec §6.1, §6.6, §12.1).
#[uniffi::export]
impl Session {
```
Replace:
```rust
/// Tracking a hardware synth (plan 5c-2; spec §6.7).
#[uniffi::export]
impl Session {
    /// Track Synth… on MIDI lane `lane` of `seq` (spec §6.7), as one undo
    /// step "Track Synth": the lane recording its external synth's return
    /// (reused, else "<synth> audio" just below) records device input
    /// `first` (and the next with `stereo`), has an output path and is
    /// armed; the MIDI lane is armed with `arm_midi`, else disarmed (plan
    /// 5c-2 Ruling 5). With `mute_after`, the next pass that records that
    /// return also mutes the MIDI lane, in its own "Record" step (Ruling
    /// 6). `Model` when the lane plays no external synth or the channels
    /// are out of range; `Busy` while recording (Ruling 14).
    pub fn track_synth(
        &self,
        seq: u64,
        lane: u64,
        first: u16,
        stereo: bool,
        arm_midi: bool,
        mute_after: bool,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let id = {
                let inner = lock(&self.inner);
                if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                    return Err(recording_busy());
                }
                seq_id(inner.doc(), seq)?
            };
            let channels = if stereo {
                core_model::lane::InputChannels::Stereo(first)
            } else {
                core_model::lane::InputChannels::Mono(first)
            };
            let snapshot = self.execute(Command::Lane(LaneCmd::TrackSynth {
                seq: id,
                lane: LaneId(lane),
                channels,
                arm_midi,
            }))?;
            let mut inner = lock(&self.inner);
            let dest = inner
                .doc()
                .lane(id, LaneId(lane))
                .ok()
                .and_then(|l| l.midi_dest);
            let audio = dest.and_then(|d| crate::synth::return_lane_in(inner.doc(), id, d, LaneId(lane)));
            if let Some(audio) = audio {
                if mute_after {
                    inner.synth_mutes.insert((id, audio), LaneId(lane));
                } else {
                    inner.synth_mutes.remove(&(id, audio));
                }
            }
            Ok(snapshot)
        })
    }

    /// Sets external destination `dest`'s return latency to `ms` (spec
    /// §4.5a: 0–500), one undo step "Set Return Latency". `Model` for
    /// another destination or a value out of range.
    pub fn set_return_latency(&self, dest: u64, ms: f32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            self.execute(Command::Routing(RoutingCmd::SetReturnLatency {
                dest: DestId(dest),
                ms,
            }))
        })
    }
}

/// Live input (plan 5c Task 8; spec §6.1, §6.6, §12.1).
#[uniffi::export]
impl Session {
```

In the Rust tests — `crates/core-ffi/tests/live_input.rs`, Find:
```rust
        LaneInputRow::Audio {
            first: 300,
            stereo: false,
        },
        LaneInputRow::Audio {
            first: 255,
            stereo: true,
        },
```
Replace:
```rust
        LaneInputRow::Audio {
            first: 300,
            stereo: false,
            return_of: None,
        },
        LaneInputRow::Audio {
            first: 255,
            stereo: true,
            return_of: None,
        },
```
and Find:
```rust
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
            }),
```
Replace:
```rust
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            }),
```

`crates/core-ffi/tests/control.rs`, Find:
```rust
    lanes[0].input = Some(LaneInputRow::Audio {
        first: 2,
        stereo: true,
    });
```
Replace:
```rust
    lanes[0].input = Some(LaneInputRow::Audio {
        first: 2,
        stereo: true,
        return_of: None,
    });
```

In Swift — `app/Sources/VizDAWAppModel/ControlCommands.swift`, Find:
```swift
            return Input(value: .audio(first: UInt16(channel - 1), stereo: args.flag("stereo") ?? false))
```
Replace:
```swift
            return Input(value: .audio(first: UInt16(channel - 1), stereo: args.flag("stereo") ?? false, returnOf: nil))
```

`app/Tests/VizDAWAppModelTests/RecordModelTests.swift`, Find:
```swift
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 2, stereo: true))))
```
Replace:
```swift
        #expect(
            f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 2, stereo: true, returnOf: nil))))
```

`app/Tests/VizDAWAppModelTests/FakeSession.swift`, Find:
```swift
    func audition(dest: UInt64, pitch: UInt8, velocity: UInt16, on: Bool) throws -> Bool {
        let e = auditionErrors.isEmpty ? nil : auditionErrors.removeFirst()
        if let e { throw e }
        auditions.append((dest, pitch, on))
        return auditionSent
    }
}
```
Replace:
```swift
    func audition(dest: UInt64, pitch: UInt8, velocity: UInt16, on: Bool) throws -> Bool {
        let e = auditionErrors.isEmpty ? nil : auditionErrors.removeFirst()
        if let e { throw e }
        auditions.append((dest, pitch, on))
        return auditionSent
    }

    // MARK: Synth tracking (plan 5c-2)

    var trackSynthCalls: [(seq: UInt64, lane: UInt64, first: UInt16, stereo: Bool, armMidi: Bool, muteAfter: Bool)] = []
    var returnLatencies: [(dest: UInt64, ms: Float)] = []
    /// Thrown (once, then cleared) by the synth-tracking calls.
    var synthError: VizError?

    func trackSynth(
        seq: UInt64, lane: UInt64, first: UInt16, stereo: Bool, armMidi: Bool, muteAfter: Bool
    ) throws -> DocSnapshot {
        if let e = synthError {
            synthError = nil
            throw e
        }
        trackSynthCalls.append((seq, lane, first, stereo, armMidi, muteAfter))
        return commandResult()
    }

    func setReturnLatency(dest: UInt64, ms: Float) throws -> DocSnapshot {
        if let e = synthError {
            synthError = nil
            throw e
        }
        returnLatencies.append((dest, ms))
        return commandResult()
    }
}
```

- [ ] **Step 5: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test synth 2>&1 | tail -5
cargo test -p core-ffi --lib synth 2>&1 | tail -3
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -3
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c | tail -8
scripts/build-ffi.sh 2>&1 | tail -2 && scripts/swift.sh build 2>&1 | tail -2 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: `synth` 7 pass and its two unit tests; the workspace passes; Swift builds (the new `DestRow` fields have defaults, so the existing `DestRow(id:name:plugin:)` literals compile) and its tests pass.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/synth.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/edit.rs \
  crates/core-ffi/src/mixer.rs crates/core-ffi/src/record.rs crates/core-ffi/src/session.rs \
  crates/core-ffi/src/control.rs crates/core-ffi/tests/synth.rs crates/core-ffi/tests/live_input.rs \
  crates/core-ffi/tests/control.rs app/Sources/VizDAWAppModel/ControlCommands.swift \
  app/Tests/VizDAWAppModelTests/RecordModelTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "session: Track Synth, return latency, the return shift and mute after recording (5c-2 Task 8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 9: `core-ffi` — Measure

**Files:**
- Modify: `crates/core-ffi/src/host.rs` (`claim_host` out of `play`, `start_measure`, `end_measure`, `measuring`, the measuring guard)
- Modify: `crates/core-ffi/src/synth.rs` (`measured`, `measured_status`, `return_lane`, `MeasureRow`, constants)
- Modify: `crates/core-ffi/src/session.rs` (`measure_return`)
- Modify: `crates/core-ffi/src/lib.rs` (`pub use synth::MeasureRow`)
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift` (`measureReturn`)
- Test: `crates/core-ffi/tests/synth.rs` (appended), `synth.rs` unit tests

**Interfaces:**
- Consumes: `Engine::start_measure`/`stop_measure`, `MeasureStatus` (Task 5); `return_dests` at load (Task 7); `RoutingCmd::SetReturnLatency`; `record::channel_refusal`, `record::latency_trim`.
- Produces:
  - `vizdaw_ffi::synth::MeasureRow { return_latency_ms: f32, onsets: u32, snapshot: DocSnapshot }` (UniFFI record);
  - `Session.measure_return(dest: u64) -> Result<MeasureRow, VizError>`;
  - `vizdaw_ffi::synth::{MEASURE_WAIT, MIN_ONSETS, NO_SIGNAL, Measured, measured, measured_status, return_lane}`;
  - `vizdaw_ffi::host::{start_measure, end_measure, measuring}`.

Spec §6.7 "Measure"; Rulings 8 and 9.

- [ ] **Step 1: Write the failing tests**

In `crates/core-ffi/tests/synth.rs`:

Find:
```rust
use vizdaw_ffi::{EventRow, LaneInputRow, RecordSpecRow, Session};
```
Replace:
```rust
use vizdaw_ffi::{EditCommand, EventRow, LaneEdit, LaneInputRow, RecordSpecRow, Session, VizError};
```

Then append:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/synth.rs <<'VIZDAW_EOF'

fn return_latency(s: &Session, dest: DestId) -> f32 {
    s.destinations()
        .unwrap()
        .into_iter()
        .find(|x| x.id == dest.0)
        .unwrap()
        .return_latency_ms
}

/// Points lane `rec`'s return at `synth` on device input `first`.
fn listen(s: &Session, d: &Doc, first: u16) {
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: d.seq.0,
                lane: d.rec.0,
                input: Some(LaneInputRow::Audio {
                    first,
                    stereo: false,
                    return_of: Some(d.synth.0),
                }),
            },
        },
        None,
    )
    .unwrap();
}

/// Spec §6.7, §12.3: Measure finds the fake delay within ±1 ms with the
/// transport stopped, and sets it as one undo step.
#[test]
fn measure_finds_the_fake_delay_and_sets_it() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let m = s.measure_return(d.synth.0).unwrap();
    assert!(
        (m.return_latency_ms - DELAY_MS as f32).abs() <= 1.0,
        "{m:?}"
    );
    assert_eq!(m.onsets, 5);
    assert_eq!(m.snapshot.undo_label.as_deref(), Some("Set Return Latency"));
    assert_eq!(return_latency(&s, d.synth), m.return_latency_ms);
    assert!(!s.transport().unwrap().playing, "the transport stayed stopped");
    assert!(!vizdaw_ffi::host::measuring());
    s.undo().unwrap();
    assert_eq!(return_latency(&s, d.synth), 0.0);
    // Measured, then recorded: the clicks land on their notes.
    s.redo().unwrap();
    record_to_the_third_note(&s, d.seq);
    for (at, note) in clicks(&s, d.seq.0, d.rec.0).iter().zip([BEAT, 2 * BEAT, 3 * BEAT]) {
        assert!((at - note).abs() <= MS, "a click at {at}, its note at {note}");
    }
    s.close().unwrap();
}

/// Spec §6.7, Review Focus 1: a return input with no clean onset (here
/// fake input 1, a sine that never falls quiet) fails "No signal on the
/// synth's return input" and changes nothing; nothing is left installed.
#[test]
fn measure_fails_cleanly_without_a_clean_onset() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    listen(&s, &d, 0);
    let v = s.snapshot().unwrap().version;
    match s.measure_return(d.synth.0) {
        Err(VizError::Model { message }) => assert!(
            message.contains("No signal on the synth's return input"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
    assert_eq!(s.snapshot().unwrap().version, v);
    assert_eq!(return_latency(&s, d.synth), 0.0);
    assert!(!vizdaw_ffi::host::measuring());
    s.play(d.seq.0).unwrap();
    s.stop().unwrap();
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 9: Measure refuses a destination that is not an
/// external synth, one no lane records the return of, an unknown one, a
/// return channel the input lacks, and anything while the transport plays
/// or records; afterwards it still works.
#[test]
fn measure_refuses_what_it_cannot_measure() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let mut b = Builder::from_doc(d.doc.clone());
    let piano = b.dest("Piano", "builtin:synth", 0).unwrap();
    let juno = b.dest("Juno", "Juno", 0).unwrap();
    let s = untitled(&g, b.finish());
    let model = |r: Result<vizdaw_ffi::synth::MeasureRow, VizError>, needle: &str| match r {
        Err(VizError::Model { message }) => {
            assert!(message.contains(needle), "{needle}: {message}")
        }
        other => panic!("{needle}: {other:?}"),
    };
    model(s.measure_return(piano.0), "not an external MIDI destination");
    model(s.measure_return(juno.0), "no lane records the return of Juno");
    assert!(s.measure_return(999).is_err());
    listen(&s, &d, 8);
    model(s.measure_return(d.synth.0), "the audio input has 3 channel(s)");
    listen(&s, &d, 2);
    s.play(d.seq.0).unwrap();
    assert!(matches!(
        s.measure_return(d.synth.0),
        Err(VizError::Busy { .. })
    ));
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    s.record(d.seq.0, spec()).unwrap();
    assert!(matches!(
        s.measure_return(d.synth.0),
        Err(VizError::Busy { .. })
    ));
    s.stop().unwrap();
    wait_until("the transport to stop", || !s.transport().unwrap().playing);
    assert!(s.measure_return(d.synth.0).is_ok());
    s.close().unwrap();
}

/// Plan 5c-2 Ruling 9: while a measurement runs, Play, Record and another
/// window's Measure are Busy; the measurement then finishes.
#[test]
fn play_and_record_wait_for_a_measurement() {
    let g = serial(Some(DELAY_MS), None);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let other = untitled(&g, d.doc.clone());
    let s2 = Arc::clone(&s);
    let dest = d.synth.0;
    let t = std::thread::spawn(move || s2.measure_return(dest));
    wait_until("the measurement to start", vizdaw_ffi::host::measuring);
    assert!(matches!(s.play(d.seq.0), Err(VizError::Busy { .. })));
    assert!(matches!(s.record(d.seq.0, spec()), Err(VizError::Busy { .. })));
    assert!(matches!(other.measure_return(dest), Err(VizError::Busy { .. })));
    let m = t.join().unwrap().unwrap();
    assert_eq!(m.onsets, 5);
    other.close().unwrap();
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test synth 2>&1 | tail -10
```
Expected: compile errors (`measure_return`, `host::measuring`, `synth::MeasureRow` unknown).

- [ ] **Step 3: What a measurement comes to**

In `crates/core-ffi/src/synth.rs`:

Find:
```rust
use core_model::command::RecordPass;
use core_model::document::Document;
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::lane::LaneInput;
use std::collections::BTreeMap;
```
Replace:
```rust
use crate::snapshot::DocSnapshot;
use core_engine::measure::{MeasureStatus, PINGS};
use core_model::command::RecordPass;
use core_model::document::{Document, MAX_RETURN_LATENCY_MS};
use core_model::ids::{DestId, LaneId, SeqId};
use core_model::lane::{InputChannels, LaneInput};
use std::collections::BTreeMap;
use std::time::Duration;

/// How long Measure waits at most (its notes take about 2.6 s, plus the
/// device's latencies).
pub const MEASURE_WAIT: Duration = Duration::from_secs(10);
/// Onsets Measure needs (spec §6.7: fewer than 3 fails).
pub const MIN_ONSETS: usize = 3;
/// Why Measure fails without them (spec §6.7).
pub const NO_SIGNAL: &str = "No signal on the synth's return input";

/// What Measure found (`destination.measure_return`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MeasureRow {
    /// The return latency set, in ms.
    pub return_latency_ms: f32,
    /// Notes whose onset was found (3–5).
    pub onsets: u32,
    /// The document after the step "Set Return Latency".
    pub snapshot: DocSnapshot,
}

/// A measurement's result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// The return latency in ms, 0–500.
    pub ms: f32,
    /// Notes whose onset was found.
    pub onsets: u32,
}

/// The return latency the notes' heard runs `sent` and their onsets'
/// input runs `onsets` (by note) say (plan 5c-2 Ruling 8): the median of
/// `onset − heard − (delay − trim)` over the notes that found one (the
/// mean of the middle two for an even count), in ms at `rate`, clamped to
/// 0–500. Fewer than [`MIN_ONSETS`] onsets: [`NO_SIGNAL`].
pub fn measured(
    sent: &[Option<i64>],
    onsets: &[Option<i64>],
    delay: i64,
    trim: i64,
    rate: f64,
) -> Result<Measured, String> {
    let d = delay.saturating_sub(trim);
    let mut found: Vec<i64> = sent
        .iter()
        .zip(onsets)
        .filter_map(|(u, k)| match (u, k) {
            (Some(u), Some(k)) => Some(k.saturating_sub(*u).saturating_sub(d)),
            _ => None,
        })
        .collect();
    if found.len() < MIN_ONSETS {
        return Err(NO_SIGNAL.into());
    }
    found.sort_unstable();
    let n = found.len();
    let at = |i: usize| found.get(i).copied().unwrap_or(0) as f64;
    let median = if n % 2 == 1 {
        at(n / 2)
    } else {
        (at(n / 2 - 1) + at(n / 2)) / 2.0
    };
    let ms = (median * 1000.0 / rate.max(1.0)).clamp(0.0, f64::from(MAX_RETURN_LATENCY_MS));
    Ok(Measured {
        ms: ms as f32,
        onsets: u32::try_from(n).unwrap_or(u32::MAX),
    })
}

/// [`measured`] of a finished measurement, with the recording latency
/// trim `trim` (the device's round trip as recording counts it).
pub fn measured_status(s: &MeasureStatus, trim: i64) -> Result<Measured, String> {
    let sent: Vec<Option<i64>> = (0..PINGS).map(|i| s.sent(i)).collect();
    let onsets: Vec<Option<i64>> = (0..PINGS).map(|i| s.onset(i)).collect();
    measured(&sent, &onsets, s.delay().unwrap_or(0), trim, s.sample_rate())
}

/// The first lane recording `dest`'s return (sequences in ID order, lanes
/// in order; plan 5c-2 Ruling 9): its sequence, ID, name and channels.
pub fn return_lane(doc: &Document, dest: DestId) -> Option<(SeqId, LaneId, String, InputChannels)> {
    doc.sequences().values().find_map(|s| {
        s.lanes.iter().find_map(|l| match &l.input {
            Some(LaneInput::Audio {
                channels,
                return_of: Some(d),
            }) if *d == dest => Some((s.id, l.id, l.name.clone(), *channels)),
            _ => None,
        })
    })
}
```

In the same file's tests module — Find:
```rust
    #[test]
    fn a_pass_owes_the_mutes_of_the_return_lanes_it_placed_audio_on() {
```
Replace:
```rust
    /// Spec §6.7 and plan 5c-2 Ruling 8: the median less the round trip;
    /// a stray onset does not move it.
    #[test]
    fn measured_takes_the_median_less_the_round_trip() {
        let sent = [Some(0), Some(24_000), Some(48_000), Some(72_000), Some(96_000)];
        // 1200, 1210, 1190, 5000 (a stray) and 1200 frames, less 100.
        let onsets = [
            Some(1300),
            Some(25_310),
            Some(49_290),
            Some(77_100),
            Some(97_300),
        ];
        let m = measured(&sent, &onsets, 100, 0, 48_000.0).unwrap();
        assert_eq!((m.ms, m.onsets), (25.0, 5));
        // The latency trim counts as the device's (as recording does).
        let t = measured(&sent, &onsets, 100, 100, 48_000.0).unwrap();
        assert!((t.ms - 1300.0 * 1000.0 / 48_000.0).abs() < 1e-3, "{t:?}");
        // An even count: the mean of the middle two.
        let four = [onsets[0], onsets[1], None, onsets[3], onsets[4]];
        let e = measured(&sent, &four, 100, 0, 48_000.0).unwrap();
        assert!((e.ms - 1205.0 * 1000.0 / 48_000.0).abs() < 1e-3, "{e:?}");
        assert_eq!(e.onsets, 4);
    }

    /// Spec §6.7: fewer than 3 onsets fails with the spec's message; the
    /// result stays within 0–500 ms.
    #[test]
    fn measured_needs_three_onsets_and_stays_in_range() {
        let sent = [Some(0), Some(24_000), Some(48_000), Some(72_000), Some(96_000)];
        let two = [Some(1200), None, Some(49_200), None, None];
        assert_eq!(
            measured(&sent, &two, 0, 0, 48_000.0),
            Err(NO_SIGNAL.to_string())
        );
        assert_eq!(
            measured(&sent, &[None; 5], 0, 0, 48_000.0),
            Err(NO_SIGNAL.to_string())
        );
        let late = sent.map(|u| u.map(|u| u + 30_000));
        assert_eq!(measured(&sent, &late, 0, 0, 48_000.0).unwrap().ms, 500.0);
        let early = sent.map(|u| u.map(|u| u + 10));
        assert_eq!(measured(&sent, &early, 100, 0, 48_000.0).unwrap().ms, 0.0);
    }

    #[test]
    fn a_pass_owes_the_mutes_of_the_return_lanes_it_placed_audio_on() {
```

In `crates/core-ffi/src/lib.rs` — Find:
```rust
pub use templates::SequenceTemplate;
```
Replace:
```rust
pub use synth::MeasureRow;
pub use templates::SequenceTemplate;
```

- [ ] **Step 4: The host claims the engine for it**

In `crates/core-ffi/src/host.rs`:

Find:
```rust
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, MidiSender, MonitorRoute, port_key};
```
Replace:
```rust
use core_engine::input::{InputSpec, MidiIn, MidiInputSpec, MidiSender, MonitorRoute, port_key};
use core_engine::measure::MeasureStatus;
```

Find:
```rust
/// Offline block size.
pub const OFFLINE_BLOCK: u32 = 512;
```
Replace:
```rust
/// Offline block size.
pub const OFFLINE_BLOCK: u32 = 512;

/// Why Play, Record and Measure wait while a measurement runs (plan 5c-2
/// Ruling 9).
pub const MEASURING: &str = "A synth's return latency is being measured; try again in a moment.";

/// Why Measure waits for the transport (plan 5c-2 Ruling 9).
pub const STOP_FIRST: &str = "Stop the transport first: Measure runs with it stopped.";

fn measuring_busy() -> VizError {
    VizError::Busy {
        message: MEASURING.into(),
    }
}
```

Find:
```rust
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    input_failed: Option<String>,
}
```
Replace:
```rust
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    input_failed: Option<String>,
    /// A return-latency measurement is installed (plan 5c-2 Ruling 9):
    /// Play, Record and another Measure are `Busy` until it ends.
    measuring: bool,
}
```

Find:
```rust
            record_hold: false,
            pending_input: None,
            input_failed: None,
        }
    }
```
Replace:
```rust
            record_hold: false,
            pending_input: None,
            input_failed: None,
            measuring: false,
        }
    }

    /// The transport plays (or was asked to), or a recording holds the
    /// engine.
    fn busy(&self) -> bool {
        self.record_hold || self.want_play || self.engine.state() == PlayState::Playing
    }
```

Find:
```rust
/// Plays `seq` of `doc` for session `owner`, up to the disk wait (spec
/// §5.5): the caller drops every lock and calls [`finish_play`]. A
/// different owner, sequence or an explicit `start` reloads (stopping
/// whoever played); otherwise the engine resumes where it stopped.
/// `until` stops it there (the loop ignored; plan 4f Ruling 9).
pub fn play(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    start: Option<Tick>,
    until: Option<Tick>,
    looped: bool,
    mut plugins: Option<&mut PluginHost>,
) -> Result<HostPlay, VizError> {
    let project = project_rate(doc);
    let mut slot = loop {
        let mut slot = lock(&HOST);
```
Replace:
```rust
/// Takes the engine host for `owner` as Play does (and Measure, plan 5c-2
/// Ruling 9): device events nobody polled are handled; a host holding
/// another session's plugins, or opened for another project rate, is
/// replaced; one is made when there is none; `owner`'s plugin host
/// becomes the insert factory and is synced. `Busy` while a measurement
/// runs, and with `idle_only` also while the transport plays or a
/// recording holds the engine, before anything is replaced. Returns the
/// locked slot (holding the host) and whether instances were restarted.
fn claim_host(
    owner: u64,
    doc: &Document,
    mut plugins: Option<&mut PluginHost>,
    idle_only: bool,
) -> Result<(std::sync::MutexGuard<'static, Option<Host>>, bool), VizError> {
    let project = project_rate(doc);
    let mut slot = loop {
        let mut slot = lock(&HOST);
        if let Some(h) = slot.as_ref() {
            if h.measuring {
                return Err(measuring_busy());
            }
            if idle_only && h.busy() {
                return Err(VizError::Busy {
                    message: STOP_FIRST.into(),
                });
            }
        }
```

Find:
```rust
    let restarted = host.attach(owner, plugins.as_deref_mut());
    if let Some(p) = plugins {
        // Slots still loading play as placeholders; `service` loads again
        // once they are in.
        crate::recovery::before_sync();
        p.sync(doc);
    }
    host.faults.clear();
    // Still unavailable: said again (Task 12 review M2).
```
Replace:
```rust
    let restarted = host.attach(owner, plugins.as_deref_mut());
    if let Some(p) = plugins {
        // Slots still loading play as placeholders; `service` loads again
        // once they are in.
        crate::recovery::before_sync();
        p.sync(doc);
    }
    Ok((slot, restarted))
}

/// Plays `seq` of `doc` for session `owner`, up to the disk wait (spec
/// §5.5): the caller drops every lock and calls [`finish_play`]. A
/// different owner, sequence or an explicit `start` reloads (stopping
/// whoever played); otherwise the engine resumes where it stopped.
/// `until` stops it there (the loop ignored; plan 4f Ruling 9). `Busy`
/// while a measurement runs (plan 5c-2 Ruling 9).
pub fn play(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    start: Option<Tick>,
    until: Option<Tick>,
    looped: bool,
    plugins: Option<&mut PluginHost>,
) -> Result<HostPlay, VizError> {
    let (mut slot, restarted) = claim_host(owner, doc, plugins, false)?;
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    host.faults.clear();
    // Still unavailable: said again (Task 12 review M2).
```

Find:
```rust
        Some(h) if h.owner == owner => {
            let handles = h.engine.start_recording(req)?;
```
Replace:
```rust
        Some(h) if h.owner == owner => {
            if h.measuring {
                return Err(measuring_busy());
            }
            let handles = h.engine.start_recording(req)?;
```

Find:
```rust
/// Whether `owner`'s renderer has its transport stopped (the end of a
/// song without a loop), when it holds the engine. Right after a Play this
/// still reads stopped until the renderer applies it.
```
Replace:
```rust
/// Starts measuring `dest`'s return latency for session `owner` (spec
/// §6.7; plan 5c-2 Ruling 9): claims the engine as Play would (`Busy`
/// while any session plays or records, or a measurement runs), loads
/// `seq` (whose lane `lane` records `dest`'s return on `channels`) with
/// the transport stopped, opens the input and installs the measurement.
/// A channel the input lacks is refused as Record refuses it (with the
/// denied or failed-open reason when there is one). The caller waits
/// without any lock, then calls [`end_measure`].
pub fn start_measure(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    dest: DestId,
    lane: &str,
    channels: core_model::lane::InputChannels,
    plugins: Option<&mut PluginHost>,
) -> Result<Arc<MeasureStatus>, VizError> {
    let (mut slot, _) = claim_host(owner, doc, plugins, true)?;
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    host.faults.clear();
    host.ensure_input(true)?;
    let back = |h: &mut Host| {
        let _ = h.ensure_input(wants_audio_input(doc, seq));
    };
    if let Some(why) =
        crate::record::channel_refusal(lane, channels, host.inputs.channels, &host.faults)
    {
        back(host);
        return Err(crate::events::bad(&why));
    }
    host.engine.stop()?;
    let looped = host.looped;
    host.load(owner, doc, seq, looped)?;
    host.owner = owner;
    host.seq = seq;
    host.want_play = false;
    host.until = None;
    host.epoch = next_epoch();
    let first = channels.first();
    let mut inputs = vec![first];
    if let Some(r) = channels.last().filter(|r| *r != first) {
        inputs.push(r);
    }
    match host.engine.start_measure(doc, dest, inputs)? {
        Ok(status) => {
            host.measuring = true;
            Ok(status)
        }
        Err(why) => {
            back(host);
            Err(crate::events::bad(&format!(
                "the synth cannot be played: {why}"
            )))
        }
    }
}

/// Ends `owner`'s measurement: it is removed (its sounding note ends) and
/// the input follows the lanes of `seq` again.
pub fn end_measure(owner: u64, doc: &Document, seq: SeqId) -> Result<(), VizError> {
    let mut slot = lock(&HOST);
    match slot.as_mut() {
        Some(h) if h.owner == owner && h.measuring => {
            h.measuring = false;
            let removed = h.engine.stop_measure().map_err(VizError::from);
            let input = h.ensure_input(wants_audio_input(doc, seq));
            removed.and(input)
        }
        _ => Ok(()),
    }
}

/// Whether a return-latency measurement runs (in any session).
pub fn measuring() -> bool {
    lock(&HOST).as_ref().is_some_and(|h| h.measuring)
}

/// Whether `owner`'s renderer has its transport stopped (the end of a
/// song without a loop), when it holds the engine. Right after a Play this
/// still reads stopped until the renderer applies it.
```

- [ ] **Step 5: The session's Measure**

In `crates/core-ffi/src/session.rs` — Find:
```rust
    /// Sets external destination `dest`'s return latency to `ms` (spec
    /// §4.5a: 0–500), one undo step "Set Return Latency". `Model` for
    /// another destination or a value out of range.
```
Replace:
```rust
    /// Measure (spec §6.7; plan 5c-2 Rulings 8–9): with the transport
    /// stopped, plays five notes to external destination `dest` and
    /// listens on the input of the lane recording its return; sets its
    /// return latency to what it found, one undo step "Set Return
    /// Latency". Blocks for about 2.7 s (the session stays usable: its
    /// lock is released while it waits); call it off the main thread.
    /// `Model` for a destination that is not an external synth or that no
    /// lane records the return of, for a channel the input lacks, and with
    /// "No signal on the synth's return input" when fewer than 3 onsets
    /// were found (nothing changes then); `Busy` while this session
    /// records or edits, while any session plays or records, and while
    /// another measurement runs.
    pub fn measure_return(&self, dest: u64) -> Result<crate::synth::MeasureRow, VizError> {
        self.transport_call(|| {
            let dest = DestId(dest);
            let (doc, seq, name, channels) = {
                let inner = lock(&self.inner);
                if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                    return Err(recording_busy());
                }
                if inner.history.in_transaction() {
                    return Err(VizError::Busy {
                        message: "Finish the open edit first.".into(),
                    });
                }
                let doc = inner.doc().clone();
                let d = doc
                    .instruments()
                    .get(&dest)
                    .ok_or_else(|| crate::events::bad(&format!("unknown id {dest}")))?;
                if !d.kind.is_external() {
                    return Err(crate::events::bad(&format!(
                        "{} is not an external MIDI destination: Measure measures a hardware synth",
                        d.name
                    )));
                }
                let found = crate::synth::return_lane(&doc, dest);
                let Some((seq, _, name, channels)) = found else {
                    return Err(crate::events::bad(&format!(
                        "no lane records the return of {}: set one up with Track Synth first",
                        d.name
                    )));
                };
                (doc, seq, name, channels)
            };
            let status = {
                let mut inner = lock(&self.inner);
                inner.mark(self.id, true);
                plugins::with(self.id, |p| {
                    host::start_measure(self.id, &doc, seq, dest, &name, channels, p)
                })
                .and_then(|r| r)?
            };
            // The notes take about 2.6 s; nothing here holds a lock.
            let until = Instant::now() + crate::synth::MEASURE_WAIT;
            while !status.done() && !status.closed() && Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let done = status.done();
            let ended = host::end_measure(self.id, &doc, seq);
            if !done {
                return Err(VizError::Engine {
                    message: if status.closed() {
                        "the measurement stopped: the audio device went away".into()
                    } else {
                        "the measurement did not finish in time".into()
                    },
                });
            }
            ended?;
            let m = crate::synth::measured_status(&status, crate::record::latency_trim())
                .map_err(|e| crate::events::bad(&e))?;
            let snapshot = self.execute(Command::Routing(RoutingCmd::SetReturnLatency {
                dest,
                ms: m.ms,
            }))?;
            Ok(crate::synth::MeasureRow {
                return_latency_ms: m.ms,
                onsets: m.onsets,
                snapshot,
            })
        })
    }

    /// Sets external destination `dest`'s return latency to `ms` (spec
    /// §4.5a: 0–500), one undo step "Set Return Latency". `Model` for
    /// another destination or a value out of range.
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift` — Find:
```swift
    func setReturnLatency(dest: UInt64, ms: Float) throws -> DocSnapshot {
        if let e = synthError {
            synthError = nil
            throw e
        }
        returnLatencies.append((dest, ms))
        return commandResult()
    }
}
```
Replace:
```swift
    func setReturnLatency(dest: UInt64, ms: Float) throws -> DocSnapshot {
        if let e = synthError {
            synthError = nil
            throw e
        }
        returnLatencies.append((dest, ms))
        return commandResult()
    }

    var measures: [UInt64] = []
    /// What `measureReturn` finds.
    var measured: Float = 25

    func measureReturn(dest: UInt64) throws -> MeasureRow {
        if let e = synthError {
            synthError = nil
            throw e
        }
        measures.append(dest)
        return MeasureRow(returnLatencyMs: measured, onsets: 5, snapshot: commandResult())
    }
}
```

- [ ] **Step 6: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test synth 2>&1 | tail -5
cargo test -p core-ffi --lib synth 2>&1 | tail -3
cargo test -p core-ffi --test record --test live_input --test transport 2>&1 | grep -E "^test result|FAILED"
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -3
scripts/build-ffi.sh 2>&1 | tail -2 && scripts/swift.sh build 2>&1 | tail -2 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: `synth` 11 pass (each measurement takes about 2.7 s of wall time) and its four unit tests; Play and Record are unchanged (`claim_host` is the old loop, moved); clean; Swift passes. If a `transport` test binary does not exist, drop it from the command (`ls crates/core-ffi/tests`).

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/host.rs crates/core-ffi/src/synth.rs crates/core-ffi/src/session.rs \
  crates/core-ffi/src/lib.rs crates/core-ffi/tests/synth.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "session: Measure finds a synth's return latency with the transport stopped (5c-2 Task 9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 10: Control channel — `lane.track_synth`, `destination.*`, `return_of`

**Files:**
- Modify: `crates/core-control/src/registry.rs` (three commands, `lane.set_input`'s `return_of`)
- Modify: `crates/core-control/tests/commands.rs` (count 127, `synth_commands_check_their_params`)
- Modify: `crates/core-ffi/src/control.rs` (`ControlResult::Measured`, `input.return_of` in JSON)
- Modify: `crates/core-ffi/tests/control.rs` (the lanes test sees `return_of`; the measured answer)
- Create: `app/Sources/VizDAWAppModel/SynthCommands.swift`
- Modify: `app/Sources/VizDAWAppModel/ControlCommands.swift` (handled IDs, dispatch, `ControlLanes.input`'s `return_of`)
- Test: `app/Tests/VizDAWAppModelTests/SynthCommandsTests.swift` (new)

**Interfaces:**
- Consumes: `Session.track_synth`, `set_return_latency`, `measure_return`, `destinations` (Tasks 8–9).
- Produces: registry IDs `lane.track_synth`, `destination.set_return_latency`, `destination.measure_return` (key-only, document commands); `ControlResult::Measured { doc: String, dest: u64, return_latency_ms: f32, onsets: u32 }` → JSON `{doc, dest, return_latency_ms, onsets}`; lane JSON `input: {kind: "audio", channel, stereo, return_of}`; Swift `SynthCommands` (`ids`, `parseMs`, `externalDestinations`, `run`).

Spec §12.1; Rulings 11 and 12.

- [ ] **Step 1: Write the failing tests**

In `crates/core-control/tests/commands.rs`:

Find:
```rust
        124,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four; 5c adds record.set, record.recover, lane.set_input, lane.set_monitor and midi.inject"
```
Replace:
```rust
        127,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four; 5c adds record.set, record.recover, lane.set_input, lane.set_monitor and midi.inject; 5c-2 adds lane.track_synth, destination.set_return_latency and destination.measure_return"
```

Find:
```rust
#[test]
fn state_get_record_and_input() {
```
Replace:
```rust
/// Plan 5c-2 Task 10: the synth-tracking commands, and `lane.set_input`'s
/// `return_of`, take their parameters by type and range; hostile values
/// never reach the app.
#[test]
fn synth_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("lane.track_synth", json!({ "seq": 1, "lane": 2 })),
        (
            "lane.track_synth",
            json!({ "seq": 1, "lane": 2, "channel": 3, "stereo": true, "arm_midi": true, "mute_after": false }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": "12.5" }),
        ),
        ("destination.measure_return", json!({ "dest": 4 })),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "channel": 3, "return_of": 4 }),
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
        ("lane.track_synth", json!({ "lane": 2 })),
        ("lane.track_synth", json!({ "seq": 1, "lane": 2, "channel": 0 })),
        ("lane.track_synth", json!({ "seq": 1, "lane": 2, "channel": 257 })),
        ("lane.track_synth", json!({ "seq": 1, "lane": 2, "stereo": "yes" })),
        ("lane.track_synth", json!({ "seq": 1, "lane": -2 })),
        ("lane.track_synth", json!({ "seq": 1, "lane": 2, "channels": 3 })),
        ("destination.set_return_latency", json!({ "dest": 4 })),
        ("destination.set_return_latency", json!({ "ms": "12" })),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": 12.5 }),
        ),
        (
            "destination.set_return_latency",
            json!({ "dest": 4, "ms": "x".repeat(100_000) }),
        ),
        ("destination.measure_return", json!({})),
        ("destination.measure_return", json!({ "dest": "Juno" })),
        (
            "lane.set_input",
            json!({ "seq": 1, "lane": 2, "kind": "audio", "return_of": -1 }),
        ),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    let spec = |id: &str| registry::find(id).unwrap();
    for id in [
        "lane.track_synth",
        "destination.set_return_latency",
        "destination.measure_return",
    ] {
        assert!(!spec(id).menu_item && spec(id).document, "{id}");
    }
}

#[test]
fn state_get_record_and_input() {
```

In `crates/core-ffi/tests/control.rs`:

Find:
```rust
    lanes[0].input = Some(LaneInputRow::Audio {
        first: 2,
        stereo: true,
        return_of: None,
    });
```
Replace:
```rust
    lanes[0].input = Some(LaneInputRow::Audio {
        first: 2,
        stereo: true,
        return_of: Some(7),
    });
```

Find:
```rust
    assert_eq!(
        l[0]["input"],
        json!({"kind": "audio", "channel": 3, "stereo": true})
    );
```
Replace:
```rust
    assert_eq!(
        l[0]["input"],
        json!({"kind": "audio", "channel": 3, "stereo": true, "return_of": 7})
    );
```

Find:
```rust
/// Plan 5c Task 14: document lanes show their takes, input and monitor
/// (channels counted from 1), and the transport carries the record faults.
```
Replace:
```rust
/// Plan 5c-2 Task 10: `destination.measure_return` answers with the
/// milliseconds it set and the onsets it found.
#[test]
fn a_measurement_answers_with_its_milliseconds() {
    let v = result_json(&ControlResult::Measured {
        doc: "D".into(),
        dest: 4,
        return_latency_ms: 12.5,
        onsets: 5,
    });
    assert_eq!(
        v,
        json!({"doc": "D", "dest": 4, "return_latency_ms": 12.5, "onsets": 5})
    );
}

/// Plan 5c Task 14: document lanes show their takes, input and monitor
/// (channels counted from 1), and the transport carries the record faults.
```

Then the Swift tests:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/SynthCommandsTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Synth tracking over the control channel (plan 5c-2 Task 10): Track
/// Synth with channels counted from 1, a return latency typed as text
/// (0–500 ms), Measure answering with its milliseconds, and
/// `lane.set_input`'s `return_of`; hostile values are `invalid_params`
/// and the core's refusals the call's failure.
@MainActor
@Suite struct SynthCommandsTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ pairs: [(String, ControlValue)], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlOutcome
    {
        ControlCommands.run(
            id, args: pairs.map { ControlArg(name: $0.0, value: $0.1) }, doc: (doc, m), store: s, host: host)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func trackSynthAsksTheCoreWithChannelsFromOne() throws {
        let (s, id, m, f) = try demo()
        let o = run(
            "lane.track_synth",
            [
                ("seq", .int(value: 3)), ("lane", .int(value: 31)), ("channel", .int(value: 3)),
                ("stereo", .flag(value: true)), ("arm_midi", .flag(value: true)),
                ("mute_after", .flag(value: true)),
            ], m, s, id)
        #expect(failure(o) == nil)
        let c = try #require(f.trackSynthCalls.last)
        #expect(c.seq == 3 && c.lane == 31 && c.first == 2 && c.stereo && c.armMidi && c.muteAfter)
        // Defaults: channel 1, mono, the MIDI lane disarmed, no mute.
        _ = run("lane.track_synth", [("seq", .int(value: 3)), ("lane", .int(value: 31))], m, s, id)
        let d = try #require(f.trackSynthCalls.last)
        #expect(d.first == 0 && !d.stereo && !d.armMidi && !d.muteAfter)
        // Hostile channels and missing ids never reach the core.
        let before = f.trackSynthCalls.count
        for extra: [(String, ControlValue)] in [[("channel", .int(value: 0))], [("channel", .int(value: 257))]] {
            #expect(
                failure(run("lane.track_synth", [("seq", .int(value: 3)), ("lane", .int(value: 31))] + extra, m, s, id))?
                    .code == .invalidParams)
        }
        #expect(failure(run("lane.track_synth", [("lane", .int(value: 31))], m, s, id))?.code == .invalidParams)
        #expect(f.trackSynthCalls.count == before)
        // The core's refusal is the call's failure.
        f.synthError = .Model(message: "Piano is not an external MIDI destination")
        #expect(
            failure(run("lane.track_synth", [("seq", .int(value: 3)), ("lane", .int(value: 30))], m, s, id))?
                .message.contains("external") == true)
    }

    @Test func returnLatencyIsTextMillisecondsFrom0To500() throws {
        let (s, id, m, f) = try demo()
        #expect(
            failure(
                run("destination.set_return_latency", [("dest", .int(value: 7)), ("ms", .text(value: "12.5"))], m, s, id))
                == nil)
        #expect(f.returnLatencies.last?.dest == 7 && f.returnLatencies.last?.ms == 12.5)
        for bad in ["", "abc", "-1", "500.5", "nan", "inf", "1e9", "0x10", String(repeating: "1", count: 40)] {
            #expect(
                failure(
                    run("destination.set_return_latency", [("dest", .int(value: 7)), ("ms", .text(value: bad))], m, s, id))?
                    .code == .invalidParams, "\(bad.prefix(10))")
        }
        #expect(failure(run("destination.set_return_latency", [("ms", .text(value: "3"))], m, s, id))?.code == .invalidParams)
        #expect(f.returnLatencies.count == 1)
        #expect(SynthCommands.parseMs(" 0 ") == 0 && SynthCommands.parseMs("500") == 500)
        f.synthError = .Model(message: "return_latency_ms out of range")
        #expect(
            failure(
                run("destination.set_return_latency", [("dest", .int(value: 7)), ("ms", .text(value: "3"))], m, s, id))?
                .code == .failed)
    }

    @Test func measureAnswersWithTheMillisecondsAndAppliesTheStep() throws {
        let (s, id, m, f) = try demo()
        f.measured = 24.9
        let version = m.snapshot.version
        guard case .done(.measured(let doc, let dest, let ms, let onsets)) =
            run("destination.measure_return", [("dest", .int(value: 7))], m, s, id)
        else {
            Issue.record("Measure did not answer with its milliseconds")
            return
        }
        #expect(doc == id.uuidString && dest == 7 && ms == 24.9 && onsets == 5)
        #expect(f.measures == [7])
        #expect(m.snapshot.version > version, "the step was applied")
        f.synthError = .Model(message: "No signal on the synth's return input")
        let e = failure(run("destination.measure_return", [("dest", .int(value: 7))], m, s, id))
        #expect(e?.code == .failed && e?.message.contains("No signal") == true)
        f.synthError = .Busy(message: "Stop the transport first: Measure runs with it stopped.")
        #expect(failure(run("destination.measure_return", [("dest", .int(value: 7))], m, s, id))?.code == .busy)
        #expect(failure(run("destination.measure_return", [], m, s, id))?.code == .invalidParams)
        #expect(f.measures == [7])
    }

    @Test func laneSetInputCarriesReturnOf() throws {
        let (s, id, m, f) = try demo()
        let lane: [(String, ControlValue)] = [("seq", .int(value: 3)), ("lane", .int(value: 31))]
        _ = run(
            "lane.set_input",
            lane + [("kind", .text(value: "audio")), ("channel", .int(value: 3)), ("return_of", .int(value: 7))], m, s,
            id)
        #expect(f.edits.last == .lane(edit: .setInput(seq: 3, lane: 31, input: .audio(first: 2, stereo: false, returnOf: 7))))
        let before = f.edits.count
        #expect(
            failure(run("lane.set_input", lane + [("kind", .text(value: "midi")), ("return_of", .int(value: 7))], m, s, id))?
                .code == .invalidParams)
        #expect(
            failure(run("lane.set_input", lane + [("kind", .text(value: "audio")), ("return_of", .int(value: -1))], m, s, id))?
                .code == .invalidParams)
        #expect(f.edits.count == before)
    }

    @Test func externalDestinationsAreTheOnesTrackSynthOffers() throws {
        let (_, _, m, f) = try demo()
        f.dests = [
            DestRow(id: 1, name: "Piano", plugin: nil),
            DestRow(id: 7, name: "Juno", plugin: nil, external: true, returnLatencyMs: 12),
        ]
        #expect(SynthCommands.externalDestinations(m).map(\.id) == [7])
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | tail -8
cargo test -p core-ffi --test control lanes_show a_measurement 2>&1 | tail -8
```
Expected: the count test fails (124), `synth_commands_check_their_params` fails (unknown commands), `core-ffi` does not compile (`ControlResult::Measured`). Swift is checked in Step 5.

- [ ] **Step 3: The registry and the JSON**

In `crates/core-control/src/registry.rs`:

Find:
```rust
    ParamSpec {
        name: "midi_channel",
        kind: ParamKind::Int { min: 1, max: 16 },
        required: false,
        help: "MIDI: the channel 1–16 (absent: every channel).",
    },
];
```
Replace:
```rust
    ParamSpec {
        name: "midi_channel",
        kind: ParamKind::Int { min: 1, max: 16 },
        required: false,
        help: "MIDI: the channel 1–16 (absent: every channel).",
    },
    ParamSpec {
        name: "return_of",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "Audio: the external MIDI destination whose return the lane records (Track Synth).",
    },
];

/// `lane.track_synth` (plan 5c-2 Ruling 11).
const TRACK_SYNTH_PARAMS: &[ParamSpec] = &[
    id_param("seq", "The MIDI lane's sequence."),
    id_param("lane", "The MIDI lane playing the external synth."),
    ParamSpec {
        name: "channel",
        kind: ParamKind::Int { min: 1, max: 256 },
        required: false,
        help: "The input channel the synth's output is wired to, from 1 (default 1).",
    },
    ParamSpec {
        name: "stereo",
        kind: ParamKind::Flag,
        required: false,
        help: "The channel and the next as a pair.",
    },
    ParamSpec {
        name: "arm_midi",
        kind: ParamKind::Flag,
        required: false,
        help: "Also arm the MIDI lane to record the performance (default: disarm it, so its part plays and is never replaced).",
    },
    ParamSpec {
        name: "mute_after",
        kind: ParamKind::Flag,
        required: false,
        help: "Mute the MIDI lane after the next recording, in the same undo step.",
    },
];

/// `destination.set_return_latency` (plan 5c-2 Ruling 11).
const RETURN_LATENCY_PARAMS: &[ParamSpec] = &[
    id_param("dest", "The external MIDI destination."),
    ParamSpec {
        name: "ms",
        kind: ParamKind::Text,
        required: true,
        help: "Its return latency in milliseconds, 0 to 500, as text (\"12.5\").",
    },
];

/// `destination.measure_return` (plan 5c-2 Ruling 11).
const MEASURE_RETURN_PARAMS: &[ParamSpec] = &[id_param(
    "dest",
    "The external MIDI destination to measure (a lane must record its return: Track Synth).",
)];
```

Find:
```rust
    CommandSpec {
        params: INJECT_PARAMS,
        ..key_only(cmd("midi.inject", "MIDI", "Inject MIDI", None, true))
    },
```
Replace:
```rust
    CommandSpec {
        params: INJECT_PARAMS,
        ..key_only(cmd("midi.inject", "MIDI", "Inject MIDI", None, true))
    },
    CommandSpec {
        params: TRACK_SYNTH_PARAMS,
        ..key_only(cmd("lane.track_synth", "Lane", "Track Synth", None, true))
    },
    CommandSpec {
        params: RETURN_LATENCY_PARAMS,
        ..key_only(cmd(
            "destination.set_return_latency",
            "Destination",
            "Set Return Latency",
            None,
            true,
        ))
    },
    CommandSpec {
        params: MEASURE_RETURN_PARAMS,
        ..key_only(cmd(
            "destination.measure_return",
            "Destination",
            "Measure Return Latency",
            None,
            true,
        ))
    },
```

In `crates/core-ffi/src/control.rs`:

Find:
```rust
    /// `state.get input` (plan 5c).
    InputState {
        /// Document ID.
        doc: String,
        /// What the input offers.
        info: crate::live::InputInfoRow,
    },
```
Replace:
```rust
    /// `state.get input` (plan 5c).
    InputState {
        /// Document ID.
        doc: String,
        /// What the input offers.
        info: crate::live::InputInfoRow,
    },
    /// `destination.measure_return` (plan 5c-2).
    Measured {
        /// Document ID.
        doc: String,
        /// The destination measured.
        dest: u64,
        /// The return latency set, in ms.
        return_latency_ms: f32,
        /// Notes whose onset was found.
        onsets: u32,
    },
```

Find:
```rust
        crate::edit::LaneInputRow::Audio { first, stereo, .. } => {
            json!({"kind": "audio", "channel": u32::from(*first) + 1, "stereo": stereo})
        }
```
Replace:
```rust
        crate::edit::LaneInputRow::Audio {
            first,
            stereo,
            return_of,
        } => json!({
            "kind": "audio", "channel": u32::from(*first) + 1, "stereo": stereo,
            "return_of": return_of,
        }),
```

Find:
```rust
        ControlResult::InputState { doc, info } => json!({
```
Replace:
```rust
        ControlResult::Measured {
            doc,
            dest,
            return_latency_ms,
            onsets,
        } => json!({
            "doc": doc, "dest": dest,
            // Thousandths: an f32 printed as f64 would show its rounding.
            "return_latency_ms": (f64::from(*return_latency_ms) * 1000.0).round() / 1000.0,
            "onsets": onsets,
        }),
        ControlResult::InputState { doc, info } => json!({
```

- [ ] **Step 4: The app's handlers**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/SynthCommands.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// Tracking a hardware synth from the control channel (plan 5c-2;
/// sub-project 5 spec §6.7, §12.1): `lane.track_synth` sets up the lane
/// recording a synth's return, `destination.set_return_latency` types a
/// synth's return latency and `destination.measure_return` measures it.
/// The Track Synth sheet, the lane menu item and the destination editor
/// are plan 5d's; they call the same session methods (Measure off the
/// main thread: it takes about three seconds).
@MainActor
public enum SynthCommands {
    public static let trackSynth = "lane.track_synth"
    public static let setReturnLatency = "destination.set_return_latency"
    public static let measureReturn = "destination.measure_return"
    public static let ids: Set<String> = [trackSynth, setReturnLatency, measureReturn]

    /// The largest return latency (spec §4.5a: 0–500 ms).
    public static let maxReturnLatencyMs: Float = 500

    /// `text` as milliseconds: a plain decimal number from 0 to 500
    /// ("12.5"); nil for anything else.
    public static func parseMs(_ text: String) -> Float? {
        let t = text.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty, t.count <= 16, t.allSatisfy({ $0.isASCII && ($0.isNumber || $0 == ".") }),
            let v = Float(t), v.isFinite, (0...maxReturnLatencyMs).contains(v)
        else { return nil }
        return v
    }

    /// The external MIDI destinations (spec §4.5a): what Track Synth plays
    /// and Measure measures.
    public static func externalDestinations(_ m: DocumentModel) -> [DestRow] {
        ((try? m.session.destinations()) ?? []).filter(\.external)
    }

    static func id(_ args: [ControlArg], _ name: String) -> UInt64? {
        args.int(name).flatMap { UInt64(exactly: $0) }
    }

    /// Runs `id` (one of `ids`) on document `doc`.
    public static func run(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        switch id {
        case trackSynth:
            guard let seq = Self.id(args, "seq"), let lane = Self.id(args, "lane") else {
                return .failed(.of(.invalidParams, "lane.track_synth: `seq` and `lane` name the MIDI lane"))
            }
            let channel = args.int("channel") ?? 1
            guard (1...256).contains(channel) else {
                return .failed(.of(.invalidParams, "lane.track_synth: `channel` is 1 to 256"))
            }
            do {
                m.apply(
                    try m.session.trackSynth(
                        seq: seq, lane: lane, first: UInt16(channel - 1), stereo: args.flag("stereo") ?? false,
                        armMidi: args.flag("arm_midi") ?? false, muteAfter: args.flag("mute_after") ?? false))
            } catch {
                return .failed(.thrown(error, "Track Synth Failed"))
            }
        case setReturnLatency:
            guard let dest = Self.id(args, "dest"), let ms = args.text("ms").flatMap(parseMs) else {
                return .failed(
                    .of(
                        .invalidParams,
                        "destination.set_return_latency: `dest` names a destination and `ms` is 0 to 500 (\"12.5\")"))
            }
            do {
                m.apply(try m.session.setReturnLatency(dest: dest, ms: ms))
            } catch {
                return .failed(.thrown(error, "Set Return Latency Failed"))
            }
        case measureReturn:
            guard let dest = Self.id(args, "dest") else {
                return .failed(.of(.invalidParams, "destination.measure_return: `dest` names a destination"))
            }
            do {
                let r = try m.session.measureReturn(dest: dest)
                m.apply(r.snapshot)
                return .done(
                    .measured(doc: doc.uuidString, dest: dest, returnLatencyMs: r.returnLatencyMs, onsets: r.onsets))
            } catch {
                return .failed(.thrown(error, "Measure Failed"))
            }
        default:
            return .failed(.of(.unknownId, "VizDAW has no handler for \(id)."))
        }
        return .done(ControlCommands.ran(id, doc, m))
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:
```swift
            .union(fieldKeys.keys).union(RegionCommands.ids)
```
Replace:
```swift
            .union(fieldKeys.keys).union(RegionCommands.ids).union(SynthCommands.ids)
```

Find:
```swift
        case RegionCommands.normalize:
```
Replace:
```swift
        case _ where SynthCommands.ids.contains(id):
            return SynthCommands.run(id, args: args, doc: docID, m)
        case RegionCommands.normalize:
```

Find:
```swift
        let audioOnly: Set<String> = ["channel", "stereo"]
```
Replace:
```swift
        let audioOnly: Set<String> = ["channel", "stereo", "return_of"]
```

Find:
```swift
            return Input(value: .audio(first: UInt16(channel - 1), stereo: args.flag("stereo") ?? false, returnOf: nil))
```
Replace:
```swift
            let returnOf = args.int("return_of")
            if let returnOf, returnOf < 0 { return nil }
            return Input(
                value: .audio(
                    first: UInt16(channel - 1), stereo: args.flag("stereo") ?? false,
                    returnOf: returnOf.map { UInt64($0) }))
```

- [ ] **Step 5: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-ffi --test control 2>&1 | grep -E "^test result|FAILED"
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -3
scripts/build-ffi.sh 2>&1 | tail -2 && scripts/swift.sh build 2>&1 | tail -2
scripts/swift.sh test 2>&1 | tail -3
scripts/swift.sh test --filter SynthCommandsTests 2>&1 | tail -3
```
Expected: `core-control` passes with 127 commands; `core-ffi` control passes; Swift builds with no new warnings; all Swift tests pass, `everyRegisteredMenuCommandHasAHandler` included (the three new IDs are handled), and `SynthCommandsTests` 5 pass. If `scripts/swift.sh test` does not take `--filter`, the full run covers it.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs \
  crates/core-ffi/src/control.rs crates/core-ffi/tests/control.rs \
  app/Sources/VizDAWAppModel/SynthCommands.swift app/Sources/VizDAWAppModel/ControlCommands.swift \
  app/Tests/VizDAWAppModelTests/SynthCommandsTests.swift
git commit -m "control: lane.track_synth, destination.set_return_latency, destination.measure_return (5c-2 Task 10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

### Task 11: The synth-tracking e2e, the README steps and the final gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs` (`tracks_the_fake_synth`)
- Modify: `app/README.md` (steps 140–142, "Synth tracking (plan 5c-2)")

**Interfaces:**
- Consumes: everything above, through the channel: `lane.track_synth`, `destination.measure_return`, `record.set`, `transport.record`, `state.get document`/`record`, `file.save`.

Spec §12.3 e2e "track the fake synth: check the audio region's clicks line up with the MIDI lane's notes"; the manual checklist's "tracking a real hardware synth with Measure, and checking alignment by ear and by zooming in".

- [ ] **Step 1: Write the e2e**

Append to `crates/vizctl/tests/e2e.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/vizctl/tests/e2e.rs <<'VIZDAW_EOF'

/// README steps 140–142 through the channel (plan 5c-2; spec §12.3 "track
/// the fake synth"): Track Synth sets up the return lane in one undo step,
/// Measure finds the fake synth's 25 ms with the transport stopped, and
/// one Record pass lands the synth's clicks on the MIDI notes that played
/// them, within a millisecond. The offline device, its fake input and the
/// fake synth: no hardware, no permission prompt.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn tracks_the_fake_synth() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, RoutingCmd, SequenceCmd};
    use core_model::event::EventBody;
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let mut app = App::start_with(&[("VIZDAW_FAKE_SYNTH", "25")]);
    // Two bars at 120 BPM: "Part" plays "Fake Synth" at beats 1, 2 and 3.
    let mut b = Builder::new();
    let synth = b.dest("Fake Synth", "Fake Synth", 0).unwrap();
    let (seq, part) = b.sequence("Track", "Part", Some(synth)).unwrap();
    b.notes(
        seq,
        part,
        &[
            (Q, 60, 0x8000, Q / 2),
            (2 * Q, 62, 0x8000, Q / 2),
            (3 * Q, 64, 0x8000, Q / 2),
        ],
    )
    .unwrap();
    b.run(Command::Routing(RoutingCmd::AddOutput {
        name: "Audio 1".into(),
        reference: "audio:1-2".into(),
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(8 * Q)),
    }))
    .unwrap();
    let path = app.launched.home.join("Synth.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));

    // Step 140: Track Synth on input 3.
    let r = app.call(
        "command.run",
        json!({"id": "lane.track_synth", "params": {"seq": seq.0, "lane": part.0, "channel": 3}, "window": w}),
    );
    assert_eq!(r["undo"], "Track Synth", "{r}");
    let rec = lane_named(&mut app, &w, "Fake Synth audio");
    assert_eq!(rec["arm"], true, "{rec}");
    assert_eq!(
        rec["input"],
        json!({"kind": "audio", "channel": 3, "stereo": false, "return_of": synth.0})
    );
    assert_eq!(lane_named(&mut app, &w, "Part")["arm"], false);

    // Step 141: Measure.
    let m = app.call(
        "command.run",
        json!({"id": "destination.measure_return", "params": {"dest": synth.0}, "window": w}),
    );
    let ms = m["return_latency_ms"].as_f64().unwrap();
    assert!((ms - 25.0).abs() <= 1.0, "{m}");
    assert_eq!(m["onsets"], 5, "{m}");
    let e = app.call_err(
        "command.run",
        json!({"id": "destination.measure_return", "params": {"dest": 999_999}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::Failed, "{e}");

    // Step 142: one pass to the end of the sequence.
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"replace": true, "loop": false, "pre_roll": 0}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == true);
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    app.call("command.run", json!({"id": "file.save", "window": w}));

    // Where each click landed against the note that played it.
    let doc = core_model::file::load(&path).unwrap();
    let s = doc.sequence(seq).unwrap();
    let lane = s
        .lanes
        .iter()
        .find(|l| l.name == "Fake Synth audio")
        .unwrap();
    let (tick, media, offset) = lane
        .active()
        .unwrap()
        .events
        .values()
        .find_map(|e| match e.body {
            EventBody::AudioRegion {
                media,
                source_offset,
                ..
            } => Some((e.tick.0, media, source_offset)),
            _ => None,
        })
        .expect("a recorded region");
    let item = doc.media().get(&media).unwrap();
    let reader = core_media::caf::CafReader::open(&path.join(&item.path)).unwrap();
    let info = reader.info().clone();
    let mut data = vec![0.0f32; info.frames as usize * usize::from(info.channels)];
    reader.read(0, &mut data, &mut Vec::new()).unwrap();
    let mut clicks = Vec::new();
    let mut quiet = 0usize;
    for (k, x) in data.iter().enumerate().skip(offset as usize) {
        if x.abs() > 0.3 {
            if quiet >= 240 {
                clicks.push(tick * 24_000 / Q + (k as i64 - offset as i64));
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    assert_eq!(clicks.len(), 3, "{clicks:?}");
    for (at, note) in clicks.iter().zip([24_000i64, 48_000, 72_000]) {
        assert!(
            (at - note).abs() <= 48,
            "a click landed at frame {at}, its note at {note}"
        );
    }
    app.quit();
}
VIZDAW_EOF
```

The unknown destination is `failed` (the core's `Model` refusal becomes "Measure Failed: …", as every core refusal does); its hostile parameter forms are `invalid_params` in Task 10's tests.

- [ ] **Step 2: The README steps**

In `app/README.md` — Find:
```markdown
     - The rate-mismatch flag is not refreshed when the device reopens
       for input: if the rate changes on that reopen, the mismatch
       warning may be stale.
```
Replace:
```markdown
     - The rate-mismatch flag is not refreshed when the device reopens
       for input: if the rate changes on that reopen, the mismatch
       warning may be stale.

### Synth tracking (plan 5c-2)

The Track Synth sheet, the lane menu item and the destination editor's
Measure button are plan 5d's; until then these steps run through the
channel. **Blocked until 5d for real hardware:** this build has no menu
that makes a MIDI destination for a hardware synth, so a project needs
one made elsewhere (or these steps wait for 5d). The IDs below come from
`$V state document` (lanes) and the project's instrument list.

140. Track Synth: on a MIDI lane playing the synth's destination, `$V run
     lane.track_synth '{"seq": 1, "lane": 1, "channel": 3}'`: a lane
     "<synth> audio" appears just below, records input 3, is armed, and
     `$V state document` shows its input's `return_of`; the MIDI lane is
     disarmed (so a Replace pass never clears its part); one undo step
     "Track Synth". Again with `"stereo": true`: the same lane now
     records inputs 3–4. With `"arm_midi": true` the MIDI lane is armed
     too; with `"mute_after": true` the next recording also mutes the MIDI
     lane, inside its "Record" step.
141. Measure: with the synth's output wired to input 3 and the transport
     stopped, `$V run destination.measure_return '{"dest": <id>}'` plays
     five middle Cs half a second apart (you hear them through the lane's
     strip) and answers `return_latency_ms` (a few milliseconds for most
     hardware) and `onsets` 5; one undo step "Set Return Latency". Pull the
     cable and measure again: "No signal on the synth's return input", and
     nothing changes. While playing it is `busy`. A pad whose release
     rings into the next note also fails rather than reading 0 ms; type
     its latency instead (`$V run destination.set_return_latency '{"dest":
     <id>, "ms": "6.5"}'`).
142. Track: Record (`transport.record`) to the end: the part plays the
     synth and the audio lane records it in one pass, one "Undo Record".
     Zoom in on a note's attack in the recorded region and on the MIDI
     note above it: they start within a millisecond. By ear: mute the MIDI
     lane and play: the printed synth sits in time with the rest. A loop
     pass with the MIDI lane armed makes matching MIDI and audio takes.
     The e2e test `tracks_the_fake_synth` runs 140–142 against the offline
     device's fake synth (`VIZDAW_AUDIO=offline VIZDAW_FAKE_SYNTH=25`),
     which opens no hardware and asks for no permission.
```

- [ ] **Step 3: The final gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -3
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -3
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep -E "^test result|FAILED"
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | awk '{p+=$4; f+=$6; i+=$8} END {print p" passed, "f" failed, "i" ignored"}'
scripts/build-ffi.sh 2>&1 | tail -2 && scripts/swift.sh build 2>&1 | tail -2 && scripts/swift.sh test 2>&1 | tail -3
scripts/make-app.sh 2>&1 | tail -2
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1 2>&1 | grep -E "^test |test result"
pgrep -lf MacOS/VizDAW || echo "no VizDAW running"
rm -rf /tmp/vze*
```
Expected: everything clean; Rust above the baseline (1273) by this plan's tests, 0 failed; the rt tests pass; Swift above 782, 0 failures; the e2e 8 tests pass (`tracks_the_fake_synth` among them); no VizDAW left running. Do not run README steps 140–142 by hand: they are the user's.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl/tests/e2e.rs app/README.md
git commit -m "e2e: track the fake synth; README synth-tracking steps (5c-2 Task 11)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

## Spec coverage

| Spec item (§13 row 5c-2) | Where |
|---|---|
| §4.5 `LaneInput::Audio.return_of` | Task 2 (model, files), Task 8 (FFI row), Task 10 (`lane.set_input`, `state.get document`) |
| §4.5a `return_latency_ms` (default 0, 0–500, by hand or measured, undoable) | Task 2 (`SetReturnLatency`), Task 8 (`set_return_latency`), Task 9 (Measure sets it), Task 10 (`destination.set_return_latency`) |
| §6.7 Setup (Track Synth: create or reuse "<D> audio" just below, input, arm, MIDI lane optionally armed, one undo step) | Task 3 (model), Task 8 (session), Task 10 (`lane.track_synth`) |
| §6.7 One pass (MIDI plays to the synth, the audio lane records the return; one "Undo Record"; loop passes make matching takes) | Tasks 7–8 (`a_return_lane_lands_on_the_notes_that_played_it`, `a_loop_pass_makes_midi_and_audio_takes_together_and_mutes_after`), Task 11 |
| §6.7 Alignment (§6.3 compensation, external MIDI at audible time, the `return_of` shift; ±1 ms) | Task 1 (planner shift), Task 8 (shift read at Record), Tasks 8, 9, 11 (±1 ms) |
| §6.7 Measure (5 notes, pitch 60, velocity 100, 500 ms apart, transport stopped; onset above −40 dBFS; median minus the round trip; fewer than 3 fails with the message) | Task 5 (engine, stopped-transport capture), Task 9 (result, session, host), Task 10 (`destination.measure_return`) |
| §6.7 Monitoring (the return lane monitors Auto, heard through its strip) | Task 3 (new lane Auto, output path given) |
| §6.7 After tracking ("Mute the MIDI lane after recording", in the pass's step) | Task 8 (Ruling 6) |
| §12.1 fake synth (`VIZDAW_FAKE_SYNTH`, "Fake Synth", 5 ms click on channel 3 the delay after the audible time, 220 Hz tone ended by note-off) | Task 4 (engine), Task 7 (host) |
| §12.1 commands and `state.get` per-lane `input.return_of` | Task 10 |
| §12.3 synth tests (±1 ms pass, Measure ±1 ms and clean failure, Track Synth one step, loop takes together) and the e2e | Tasks 5, 8, 9, 11 |
| Manual checklist: real hardware synth with Measure, by ear and zoomed | Task 11 (README 140–142) |
| 5c final review carries (a) walk-back cut, (b) `rt_alloc_record` real latency change | Task 1, Task 6 |
