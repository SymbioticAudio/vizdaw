# VizDAW 5d-1 — Monitoring and Recording Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything the record UI (plan 5d-2) stands on works in the cores and over the control channel: a monitored input is never delayed beyond the round trip and its own path's plugins (playback delay compensation reads regions ahead), the recording lane's own regions fall silent while its input is heard, count-in plays a click, Software Monitoring and the I/O Buffer Size are settings with a round-trip readout, input meters exist, the input opens off the calling thread (asking for the microphone first), arming claims an idle engine so monitoring and MIDI thru work before the first Play, arming and saving are guarded while recording, the take's waveform can be drawn while it is written, and external MIDI destinations can be listed and added.

**Architecture:** Mostly `core-engine` and `core-ffi`, with the control channel and Swift plumbing last.
- **`core-engine`**: the PDC planner gives each `Regions` node a *lead* (`Plan.leads`): the compensation its slowest path to master would otherwise add. The node reads its regions that many frames behind the renderer's frame, which is ahead of what is heard, so no delay line follows it; its monitored input is then delayed only by its own inserts. `RegionRun` maps the read frame through the roll's start, the loop seam and the sequence end. Auto-input and the new muting of the recording lane's own segments are decided at each path's read frame. The renderer also plays a count-in click, meters every input channel, and publishes "counting in".
- **`core-media`**: the record writer keeps live 256-frame peaks per lane.
- **`core-ffi`**: process-wide audio settings (Software Monitoring, I/O buffer size, stream sizes, default monitor mode, MIDI offset note); the host opens input on its own thread (asking for microphone access first), claims an idle engine for an arm/monitor/input edit, reconnects MIDI sources on arm; the session refuses arming changes and Save while recording and answers `Busy` for edits during an open edit; `recording_waveform`, `input_levels`, `midi_destination_ports`, `add_midi_destination`; Measure keeps a good result when the input change after it fails; unused MIDI-destination targets are released.
- **`core-control` and Swift**: `audio.set`, `destination.add_midi`, the new `state.get input` fields, `FakeSession`, and the e2e.

**Tech Stack:** Rust 2024 (`core-engine`, `core-media`, `core-ffi` with UniFFI 0.32.2, `core-control`, `platform-macos`, `vizctl`), Swift 6 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 rows "5d-1 — Monitoring and recording engine" and "5d-2 — Record UI" (split from the former 5d row by Ruling 1 of this plan, which amends §13):
- §6.6 Monitoring and MIDI thru (Ruling 33): Software Monitoring, the latency of a monitored input, playback delay compensation by reading regions ahead, I/O Buffer Size with a round-trip readout, refused while recording;
- §6.2 overviews "built while the file is written, so the region draws as it grows" (the data; 5d-2 draws it);
- §6.4 count-in ("Count-in plays the click before the start point without moving the timeline");
- §6.1 microphone permission, input opened only when needed;
- §6.9's Settings › Audio and Settings › Recording values (the settings; 5d-2 shows them);
- §12.1 `state.get input` (device, channels, latency, meters and clip counts);
- §12.3 `rt-check` covers monitoring, and the e2e "record audio from the fake input and check the region and waveform".

Read the whole spec (its §3 rules bind here), plan 5c (`docs/superpowers/plans/2026-10-20-recording-engine.md`) and plan 5c-2 (`docs/superpowers/plans/2026-10-26-synth-tracking.md`). The 5c and 5c-2 ledgers (`.superpowers/sdd/2026-10-20-recording-engine/` and `.superpowers/sdd/2026-10-26-synth-tracking/`, `progress.md` and `final-review.md` in each) carry the items this plan picks up (see "Carried items" below).

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root.** `scripts/build-ffi.sh` (after any Rust change that touches the FFI, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`. Swift tests run only via `scripts/swift.sh test` from the repo root. Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` (HEAD `8a6334a` before this plan's commit, plus that commit). Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`). No manifest changes, so `Cargo.lock` never changes. Never commit `.superpowers/`, `.DS_Store` or generated files.
  - End every commit message with the two attribution lines your dispatch gives you. The blocks below show:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
    Use the `Co-Authored-By` line naming the model you actually are.
- **Clean room.** Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any research material. No Opcode, Midas, Steinberg, Apple Logic or Soundcraft assets, names, artwork or branding anywhere ("works like Logic Pro" in the spec describes behaviour only).
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e test (Task 12) or `vizctl launch` with `VIZDAW_AUDIO=offline`; quit it when done (`vizctl quit`; `pkill -f "VizDAW --control"` if a test left one). At the end of a task that launched the app, `pgrep -lf MacOS/VizDAW` prints nothing; remove the `/tmp/vze*` homes your e2e runs left.
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any audio device's settings (the I/O buffer size included). Tests use temp directories; sessions that save get a trash folder (`Session::debug_set_trash_dir`); the e2e sets `VIZDAW_TRASH_DIR` inside its throwaway home.
  - Never save a project in a `vizctl`-launched app.
- **Tests never open real audio input and never trigger the microphone prompt (TCC).** Every `core-ffi` test that plays, records, monitors or measures calls `vizdaw_ffi::host::force_offline()` first and chooses its fake input with `host::set_fake_input`, never the environment. Test builds (`test-hooks`) never open live input (`live_input_allowed` is false there) and never call `platform_macos::permission::request_microphone`: the fake permission hooks (`set_fake_microphone_denied`, Task 7's `set_fake_microphone_prompt`) stand in. The e2e runs with `VIZDAW_AUDIO=offline` and `VIZDAW_AUDIO_INPUT=fake`. Real hardware is checked by eye, by the user, through `app/README.md` (plan 5d-2 Task 10); implementers never run those steps.
- **Test waits.** Every `core-ffi` integration test that waits for a recording, a measurement, the input or the engine polls with a deadline from `common::wait_limit()` (`crates/core-ffi/tests/common/mod.rs`, which reads `VIZDAW_TEST_WAIT_SECS`), never a fixed sleep for the result itself. The e2e polls with its `WAIT_MS` conditions.
- **Crates and dependencies.** Rust stable, edition 2024. No new crate and no new dependency. License GPL-3.0-only.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. New non-test Rust code uses `.get()`, iterators and `checked_*`/`saturating_*`, never indexing or unwraps.
- **Platform-neutral cores.** `core-model`, `core-engine`, `core-media` and `core-control` stay free of OS APIs.
- **Real-time rules (spec §3).** The real-time thread never allocates, frees, locks or does file I/O. Everything new on it is marked `// RT`: `RegionRun::map`/`read_frame`/`head`/`render`/`render_piece`/`add_monitor`/`silenced`, the renderer's read-head, roll-floor, wrap and after-end bookkeeping, `CountIn::render`/`advance`, `Shared::hold_input`, and the measurement of input peaks. **rt-check gates** (Tasks 2, 3, 4, 5 and 12): `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record`.
- **Media is write-once.** Recordings still go to new files under `Media/Recorded/` (`CafWriter::create` never overwrites); the live peaks only read what the writer already has in memory.
- **File compatibility.** `core_model::file::VERSION` stays 3. Nothing here changes the document model or the project file.
- **FFI safety.** No panic crosses the FFI: every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard` (or `transport_call`). Nothing new blocks a caller while holding the session lock except Record's wait for an input that is still opening (Ruling 13), which is bounded.
- **UniFFI changes.**
  - Changed (all `#[uniffi(default)]`, so existing Swift literals compile): `InputInfoRow` gains `buffer_frames`, `round_trip_frames`, `opening`, `sample_rate` (and loses `Eq`); `TransportState` gains `counting_in`; `RecordStateRow` gains `pass_start_tick`, `pass_start_file_frame`, `range_start_tick`; `RecordLaneRow` gains `peak`; `MediaRow` gains `peak`. `RecordLaneRow` and `RecordStateRow` lose `Eq` (they hold `f32`/`f64`).
  - New records: `AudioSettingsRow` (Task 6). New exported functions: `set_software_monitoring`, `set_io_buffer_frames`, `set_stream_settings`, `set_default_monitor_mode`, `audio_settings` (Task 6), `midi_destination_ports` (Task 11). New `Session` methods: `input_levels() -> Vec<f32>` (Task 4), `saves_after_pass` (Task 9), `recording_waveform` (Task 10), `add_midi_destination` (Task 11). Changed `ControlResult` variant: `InputState` gains `levels` (Task 12); the new commands answer `Ran`.
  - `FakeSession` (`app/Tests/VizDAWAppModelTests/FakeSession.swift`) implements each new `Session` method in the task that adds it.
- **Swift.** Generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests in Swift 6 mode with no new warnings. No new views in this plan (they are 5d-2's). No SwiftUI `@State`/`@FocusState`. New model tests go to `VizDAWAppModelTests`.
- **Control channel** (control spec §6, §7). Programmatic calls never open dialogs: no new command has one. Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`; the Swift test `everyRegisteredMenuCommandHasAHandler` compares them. The registry count test moves from 127 to 129 in Task 12. Every new command gets hostile-input tests: by type and range in `core-control` (`audio_and_destination_commands_check_their_params`), in the app (`audioSetChecksItsValues`, `addMidiDestinationChecksItsValues`) and in the cores (`hostile_audio_settings_are_refused`, `hostile_midi_destinations_are_refused`).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test` for the crates the task touches (the workspace in Tasks 2, 6, 8 and 12).
  - Tasks 2, 3, 4, 5 and 12 also run the rt-check gates.
  - Tasks 4 and 6–12 also run `cargo clippy -p core-ffi --lib -- -D warnings` (code under `cfg(not(feature = "test-hooks"))` is linted only by it), `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 12 also runs `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests: the `damaged_files` proptest, the MIDI listener tests (`platform-macos/tests/midi.rs`, the reconnect test), `core-ffi` catalog tests under parallel load and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - Baseline at `8a6334a` (as the 5c-2 ledger reports at `69ae822`, which `8a6334a` changes only in the spec): Rust 1350 passed, 0 failed, 12 ignored (plus the three rt tests); Swift 794 tests; e2e 8 tests.
- **How code is given.**
  - New files are written whole (`cat > … <<'VIZDAW_EOF'`); a file this plan rewrites whole (`graph/pdc.rs`, `graph/regions.rs`) is also given whole.
  - Changes to existing files are exact **Find / Replace** excerpts of the tree the previous task left, applied in order with an editor (the Edit tool). Each Find occurs exactly once in the file when it is applied.
  - The writer replayed all twelve tasks of this plan, in order, in a scratch clone of `8a6334a` outside the repo, and every gate above passed there (Rust 1392 passed, 0 failed, 13 ignored; Swift 796; e2e 9). The excerpts are the replayed ones; the replay found and fixed its slips before this text was final. If an excerpt does not match, make the same change to the current text; if a name differs from the tree (a helper, a field), use the tree's; if the compiler finds a slip (a missing import, a borrow), fix it the obvious way. Say so in the report in each case.
- **Spec values used verbatim:**
  - "Software Monitoring (Settings › Audio, default on). When off, VizDAW never plays an input back … Recording and MIDI thru are unaffected";
  - "It is the device round trip at the current I/O buffer size plus the latency of the plugins on the input's own path, and nothing more. Delay compensation for playback works by reading regions ahead, which a live input cannot do";
  - "I/O Buffer Size (Settings › Audio): 32, 64, 128, 256, 512 or 1024 frames, default 128, with a readout of the resulting round-trip latency in ms. A change reopens the device and is refused while recording";
  - "Count-in plays the click before the start point without moving the timeline"; "`count_in_bars: u8` (0–4)";
  - "Overviews (§10.1) are built while the file is written, so the region draws as it grows";
  - "the streaming setting "Disk read-ahead" (2–8 s, default 3 s; ring size follows)"; "RAM cache budget" (512 MiB default, spec §5.5);
  - "default monitor mode for new lanes";
  - "Input is opened only while some lane is armed or monitored, or the input meters are shown"; "If permission is denied, arming an audio lane shows a message pointing to System Settings › Privacy".

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out. Each is pinned by named tests in its owning task.

1. **Monitoring through a mix with a latent plugin somewhere else** (a linear-phase EQ on the master or on a bus, a look-ahead limiter on another track) while the transport plays, loops, reaches the end of the song or is located. Expected: the performer hears the input with no more delay than the device round trip and the plugins on their own strip; playback stays sample-aligned exactly as before, through loop wraps and the song's last frames; nothing from before the play or locate point is heard. Pinned by `regions_read_ahead_play_exactly_what_a_delay_would`, `a_located_lead_path_starts_silent_and_aligned`, `a_monitored_input_is_heard_through_its_own_path_only` (Task 2) and `auto_monitoring_switches_at_the_audible_punch_points` (Task 2, updated).
2. **Arming before the first Play** (a new or reopened project, a second window, a lane in another sequence, a keyboard plugged in after launch). Expected: the armed lane is heard and MIDI thru plays at once with the transport stopped; another window that is playing or recording is never interrupted; a keyboard plugged in later is heard after the next arm. Pinned by `arming_claims_an_idle_engine_and_hears_the_input`, `arming_never_takes_a_busy_engine` and `arming_a_midi_lane_listens_to_new_sources` (Task 8).
3. **The input still opening, refused or failing** (the first microphone prompt still on screen, a denial, an interface that is busy, Record pressed right after arming). Expected: the UI never stalls while the device opens; Record waits for it (bounded) or says the input is still opening; a denial is reported when arming, not after recording silence; after a failure, re-arming or choosing the input again retries instead of failing forever. Pinned by `an_input_change_runs_off_the_calling_thread`, `record_waits_for_an_input_that_is_opening`, `a_denied_first_prompt_is_reported_before_recording` and `choosing_the_input_again_retries_a_failed_open` (Task 7).
4. **Settings changed at the wrong moment** (the buffer size while recording, Software Monitoring while monitoring, stream sizes while playing, nonsense values over the channel). Expected: the buffer change is refused with a message while recording and nothing changes; Software Monitoring takes effect at once without a Play; stream sizes apply from the next engine without cutting playback; nonsense is `invalid_params` or `Model` before anything changes. Pinned by `the_buffer_size_is_refused_while_recording`, `software_monitoring_off_drops_monitoring_at_once_but_keeps_thru`, `stream_settings_wait_for_an_idle_engine` and `hostile_audio_settings_are_refused` (Task 6), `audio_and_destination_commands_check_their_params` (Task 12).
5. **Changes during a recording pass** (arming or disarming a lane, changing its input, Save, Move Record Arm, an edit during a drag). Expected: arm and input changes refused as `Busy` with a message naming why, the pass undisturbed and the take complete; monitor changes still apply; a Save saves now and again when the take lands, so the saved project has the take. Pinned by `arming_changes_are_refused_while_recording`, `a_save_while_recording_saves_the_take_after_stop` and `an_edit_during_an_open_edit_is_busy` (Task 9).

## Plan Rulings

Decisions the spec leaves open, in the format "what — why — cost if wrong".

1. **Ruling (the split):** the former §13 row "5d — Record UI" is two plans, run in order: **5d-1 — Monitoring and recording engine** (this plan: everything below the views, plus the control channel) and **5d-2 — Record UI** (`docs/superpowers/plans/2026-10-26-record-ui.md`: the toolbar Record button and popover, the lane header and mixer controls, Settings › Audio and › Recording, the Track Synth and New MIDI Destination sheets, the growing waveform's drawing, the README). This commit amends spec §13 to match.
   - Why: 5d carried 26 items from 5c and 5c-2 on top of its own row; one plan would have been about 23 tasks, and the engine half (PDC read-ahead, input threading, the engine claim) must be reviewed apart from the views.
   - Cost if wrong: one more plan header and final review.
2. **Ruling (the lead):** a `Regions` node's lead is `L − d`, where `L` is the graph latency and `d` is the insert latency on the node's slowest *signal* path to master (inputs, not sidechains, which carry no signal to master), capped at half the maximum edge delay (1 s). A node that reaches no master leads 0. If the leads would raise `L` (only possible through a sidechain fed from a led path), the graph is planned with no leads. Plugin instruments are not led: MIDI thru to a plugin instrument keeps the compensation delay of its strip (live MIDI cannot be read ahead either; spec §6.6 speaks of audio monitoring). `EngineConfig.region_leads` (default true) turns leads off for tests.
   - Cost if wrong: a software instrument played live through a mix with latency is heard late by that latency (backlog item, Task 12 adds it to `docs/feature-backlog.md`).
3. **Ruling (reading ahead at the edges):** a led node reads frame `f − lead` while the renderer renders frame `f`. Reads before the frame the transport rolled from (Play or Locate) are silent, so nothing before the play point is heard; once the loop has wrapped, a read before the loop start comes from the loop's end (`start + (x − start) mod length`); past the sequence end (or Play Selection's end) the led node keeps reading until the end frame, through the tail; Stop ends reading at once; a lead that changes with a new graph jumps, as any compensation change does. The streamer is told the earliest frame any node reads (the read head).
   - Cost if wrong: a few milliseconds at a locate, a stop or a plugin change, as today.
4. **Ruling (where auto-input is decided):** Auto monitoring's "inside the record range" and the muting of the recording lane's own regions (Ruling 6) are decided at each path's read frame, per run: the input mixed into a led node now is heard together with the regions frame it reads. This replaces 5c's single "playhead minus `L`" test, which assumed every path was delayed by `L`.
   - Cost if wrong: the switch lands up to one run (one device buffer) off.
5. **Ruling (automation on led paths):** strip automation is applied at the renderer's frame, as before. On an audio path whose regions are led, a fader or pan move therefore lands up to the lead early, as automation on master and on buses behind a latent path already does. Aligning automation by each strip's arrival time is a backlog item (Task 12).
   - Cost if wrong: automation on audio strips is a few milliseconds early in mixes with latent plugins.
6. **Ruling (the recording lane's own regions, 5c carry M10):** while a pass records, an armed lane whose input is heard (Auto or Input) at a read frame inside the record range plays none of its own regions there: the regions on that lane in the playing sequence, directly or through references (`Segment.top_lane`). Outside the range, or when its input is not heard (monitor Off, Software Monitoring off), its regions play as usual.
   - Cost if wrong: the old take is heard under the new one in Off mode, which is what Off means.
7. **Ruling (count-in):** count-in is Record's only: `RecordSpecRow.count_in_bars` 1–4 plays that many bars of clicks, one per meter beat (the meter's denominator note) at the tempo and meter at the roll point, the first beat of each bar accented (1 kHz, 0.5 peak) and the others plain (800 Hz, 0.35), each a 25 ms sine burst with a 6 ms exponential decay, mixed straight into the output (no strip, no fader). The timeline does not move; the roll starts at the first device block that starts at or after the count-in's end (the count-in pads its start so that, at a constant block size, that is exactly its end); the clicks are delayed by the graph latency, so the downbeat is heard one beat after the last click. Stop cancels a count-in. The transport reads `counting_in` meanwhile. No metronome while playing (not in the spec).
   - Cost if wrong: with a changing block size the downbeat is up to one block late.
8. **Ruling (Software Monitoring):** a process-wide setting, default on. Off, `input_spec` publishes no audio monitor routes, so no input is played back and the recording lane's regions are not muted; MIDI thru and recording are unaffected; an unarmed lane monitored Input no longer needs the input open. It reaches the engine holder at its session's next idle (Ruling 12), without a Play.
   - Cost if wrong: none (one flag).
9. **Ruling (I/O buffer size):** a process-wide setting, default 128, one of 32, 64, 128, 256, 512, 1024 (else `Model`). `set_io_buffer_frames` is `Busy` while any session records ("Stop recording to change the I/O buffer size."); otherwise it stores the value and a live host reopens its device at once with the same renderer (an offline host keeps its 512-frame blocks and reports them). The round trip is `input latency + output latency + 2 × device buffer`, in frames and ms (`InputInfoRow.round_trip_frames`).
   - Cost if wrong: the readout ignores latency a device does not report (README step, plan 5d-2).
10. **Ruling (stream settings):** the RAM cache budget (64–8192 MiB, default 512) and the disk read-ahead (2–8 s, default 3; each stream voice's ring is the read-ahead minus 1 s, so 2 s by default) are process-wide and apply to the next engine made. `set_stream_settings` drops the engine at once when no session plays, records or measures (the next Play or arm makes a new one); otherwise the change waits for that next engine.
    - Cost if wrong: a change made while playing waits for the next device change or restart.
11. **Ruling (default monitor mode):** a process-wide setting (default Auto) for lanes added by Add Lane (`LaneEdit::Add`): when it is not Auto, the new lane's monitor is set in the same undo step "Add Lane". Lanes made by Track Synth (Auto, spec §6.7), recovery and imports are unaffected.
    - Cost if wrong: one more click per lane.
12. **Ruling (settings epoch):** a setting that changes what a load publishes (Software Monitoring, the default monitor's sibling settings) bumps a process-wide epoch; each session's `idle` reloads its engine when the epoch moved since its last load and it holds the engine.
    - Cost if wrong: up to one idle tick (about 16 ms) before the change is heard.
13. **Ruling (input off the calling thread, 5c carry T11 M6):** a live host opens or closes its input on a short-lived thread with the host lock released (the backend sits behind its own mutex; host calls that would touch it while it opens skip or defer). The host reads `opening` meanwhile, and an input change asked for meanwhile waits for it. Record and Measure wait up to 3 s (`INPUT_WAIT`) with the session lock held and no host lock; still opening after that, they are `Busy` ("The audio input is still opening; try again in a moment."). Offline hosts change their fake input at once, unless the `debug_input_open_delay` test hook makes them go through the same thread.
    - Cost if wrong: Record right after the first arm can wait up to 3 s.
14. **Ruling (the first microphone prompt, 5c carry T11 M2):** before a live input opens for the first time, while macOS has not been asked (`Undetermined`), the input thread asks with `requestAccessForMediaType:` and waits for the answer (up to 120 s); only a grant opens the input. A denial is reported as the host fault `DENIED_MESSAGE`, which arming shows and Record refuses with. The `set_fake_microphone_prompt` test hook stands in for the prompt.
    - Cost if wrong: the input opens up to two minutes late if the user leaves the prompt up.
15. **Ruling (retrying a failed open, 5c carry T11 M3):** `input_failed` is cleared, so the next open tries the device again, by `set_audio_input_device` (the Settings › Audio picker) and by any edit of a lane's arm or input (the arm button and the input picker), not by Play.
    - Cost if wrong: a broken interface is retried once per arm click (each try restarts the device).
16. **Ruling (the engine claim, 5c carry I2):** an edit of a lane's arm, monitor or input in sequence S (`LaneEdit::SetFlags` with `arm`, `SetMonitor`, `SetInput`, and Move Record Arm) by a session that does not play claims the engine for S when it is idle: no session plays, records or measures. The claim loads S stopped at this session's position (its cursor in S, else bar 1), opens input as the lanes want and, on a live host, connects the MIDI sources (off the calling thread). A busy engine is never taken: the edit still applies and is heard at this session's next Play. Only edits that leave some lane of S wanting input or thru claim.
    - Cost if wrong: arming in a second window takes the engine from an idle first window (as its Play would).
17. **Ruling (MIDI sources on arm, 5c carry T12 M5):** every arm or input edit of a MIDI lane reconnects the MIDI sources on a background thread when the host is live, so a keyboard plugged in after launch is heard after the next arm.
    - Cost if wrong: none (reconnecting is idempotent).
18. **Ruling (arming during a pass, 5c carry M6):** while this session records, finishes a pass or waits to apply one, edits of a lane's arm or input (including Move Record Arm and Track Synth, already `Busy`) are refused `Busy` ("Stop recording to change which lanes record."). Monitor changes stay allowed: they change only what is heard.
    - Cost if wrong: a lane armed late misses that pass.
19. **Ruling (Save while recording, 5c carry M5):** File › Save while this session records, finishes a pass or waits to apply one saves the project now (without the take, which is not placed yet) and marks the session (`Session.saves_after_pass()`), so the pass, once applied at Stop, is saved too; a failure of that second save is reported as a problem. Autosave continues (recordings are kept for recovery). 5d-2's Save says "The take is saved when recording stops." meanwhile. Refusing the Save (`Busy`) was the alternative: a user who presses Cmd-S out of habit mid-take would lose nothing either way, but a refusal reads as an error.
    - Cost if wrong: one extra save at Stop.
20. **Ruling (busy edits, 5c-2 carry finding 13):** `Session.edit` and `edit_all` answer `Busy` ("Finish the open edit first.") while an edit is open, as Track Synth and Measure do, so a script retrying on `busy` retries `lane.set_input` during a mixer drag.
    - Cost if wrong: a refusal kind.
21. **Ruling (live peaks, 5c Ruling 23 carry):** the record writer keeps each lane's min and max per 256 frames (per channel) in memory as it writes. `Session.recording_waveform(lane, from_file_frame, to_file_frame, columns)` answers columns of them, `Ready`. `RecordStateRow` gains the current pass's start (`pass_start_tick`, `pass_start_file_frame`) and the record range's start (`range_start_tick`), so a view can place the growing region; that placement is the roll's, not latency-compensated (a few milliseconds early or late, corrected when the pass is placed at Stop). `RecordLaneRow.peak` is the lane's peak so far and `MediaRow.peak` the overview's peak (`None` until built): the e2e checks both, about 0.25 for the fake 440 Hz sine at −12 dBFS (5c carry M8).
    - Cost if wrong: the growing region can sit up to the round trip off its final place.
22. **Ruling (a good measurement survives, 5c-2 carry finding 12):** when Measure found its onsets but the input change at its end fails, the step "Set Return Latency" is still applied and the failure is kept as a host fault (shown in the fault popover), not returned as Measure's error.
    - Cost if wrong: none.
23. **Ruling (MIDI-destination targets, 5c-2 carry finding 6):** at each load, a MIDI-destination target whose port no destination of the document names, and that no measurement uses, is released (its notes were ended by the load's thru reset).
    - Cost if wrong: a target is made again when a destination naming it comes back.
24. **Ruling (MIDI output offset, 5c-2 carry finding 11):** the app has no MIDI output offset setting (the engine's `midi_offset_ns` is 0 in every host); `synth::return_frames` documents that a non-zero offset would have to be added to the shift.
    - Cost if wrong: a doc note.
25. **Ruling (New MIDI Destination, 5c-2 Ruling carry):** `midi_destination_ports()` lists the system's CoreMIDI destinations on a live host (offline: "Fake Synth" when the fake synth is on, else none); `Session.add_midi_destination(name, port, channel)` adds a `MidiEndpoint` destination for that port, one undo step "Add Destination". Name 1–255 characters without control characters, port 1–255, channel 0–15 (else `Model`).
    - Cost if wrong: a destination for a port that is gone is reported at load ("no MIDI destination named …"), as any missing port is.
26. **Ruling (control channel):** `audio.set { software_monitoring?, buffer_frames?, input_device?, latency_trim? }` (group "Audio", key-only; `input_device` "" is "Same as output") and `destination.add_midi { name, port, channel? (1–16, default 1) }` (group "Destination", key-only). `state.get input` gains `software_monitoring`, `buffer_frames` (asked), `device_buffer_frames`, `round_trip_frames`, `round_trip_ms`, `opening`, `levels` (per device channel) and `latency_trim`. The registry goes from 127 to 129.
    - Cost if wrong: aliases later.
27. **Ruling (input meters):** the renderer holds each input channel's peak since the last read (`Shared::hold_input`); `Session.input_levels()` reads and clears them (empty unless this session holds the engine with its input open). A clip mark is the recording's clipped count (`RecordLaneRow.clipped`) or a peak at or above 1.0; plan 5d-2 holds it for 2 s. The input opens for the meters only when a lane is armed or monitored (spec §6.1); 5d-2 shows meters only then.
    - Cost if wrong: none.

## Carried items

Every item the 5c and 5c-2 ledgers carry to 5d lands in this plan or in plan 5d-2 (and the 5d-2 plan lists its share again):

| Item (source) | Where |
|---|---|
| I2: arming or monitoring claims an idle engine, connects MIDI sources, opens input (5c final review) | Task 8 (Ruling 16) |
| M10: mute the recording lane's own segments while its input is heard in range (5c ledger) | Task 3 (Ruling 6) |
| PDC M4: monitored input inherits PDC latency (5c ledger, owner's §6.6 / Ruling 33) | Tasks 1–2 (Rulings 2–5) |
| Growing waveform; e2e asserts recorded content and overview peak ≈ 0.25 (5c Ruling 23, final review M8) | Task 10 (data), Task 12 (e2e); drawing in 5d-2 Task 9 |
| M5: Save while recording (5c final review) | Task 9 (Ruling 19); menu in 5d-2 Task 2 |
| M6: arm changes during a pass (5c final review) | Task 9 (Ruling 18) |
| T11 M2: denial on the first prompt surfaces late | Task 7 (Ruling 14) |
| T11 M3: no retry on Play after a failed input open | Task 7 (Ruling 15); picker in 5d-2 Tasks 3 and 6 |
| T11 M6: UI stall while input opens | Task 7 (Ruling 13) |
| T12 M5: hotplug; connect sources on arm | Task 8 (Ruling 17) |
| README 131/132 (trim, aggregate loopback/safety offset, input nominal rate, locked-rate fault, leftover "VizDAW Input") | 5d-2 Task 10 (the settings they need come from Tasks 6–7 here) |
| "New MIDI Destination…" (5c-2 ledger) | Task 11 (core); sheet in 5d-2 Task 8 |
| Track Synth sheet and lane menu (5c-2 ledger) | 5d-2 Task 8 |
| Measure off the main thread in the sheet (5c-2 ledger) | 5d-2 Task 8 |
| Finding 4: calls queued behind Measure time out; timed-out tokens still run (5c-2 final review) | 5d-2 Task 8 |
| Finding 6: MIDI-destination targets never released | Task 11 (Ruling 23) |
| Finding 11: MIDI output offset not in the record shift | Task 6 (Ruling 24) |
| Finding 12: a failed input change at Measure's end discards a good measurement | Task 11 (Ruling 22) |
| Finding 13: `lane.set_input` answers `failed`, not `busy`, for an open non-gesture edit | Task 9 (Ruling 20) |
| Trim test: the global latency trim reaches the engine; `serial()` resets it (5c-2 final review) | Task 6 |

## File map

| Crate / folder | New | Changed |
|---|---|---|
| `core-engine` | `count_in.rs`; tests `region_leads.rs`, `count_in.rs` | `graph/pdc.rs` (whole), `graph/regions.rs` (whole), `graph/exec.rs`, `graph/mod.rs`, `render.rs`, `transport.rs`, `record.rs`, `engine.rs`, `input.rs`, `schedule/audio.rs`, `lib.rs`; tests `graph_plan.rs`, `live_input.rs`, `schedule_audio.rs`, `rt_alloc.rs`, `rt_alloc_record.rs` |
| `core-media` | — | `record.rs`; tests `record.rs` |
| `platform-macos` | — | `permission.rs` |
| `core-ffi` | `audio.rs`; tests `audio_settings.rs`, `input_thread.rs`, `engine_claim.rs` | `host.rs`, `session.rs`, `record.rs`, `synth.rs`, `live.rs`, `media.rs`, `overviews.rs`, `snapshot.rs`, `control.rs`, `edit.rs`, `lib.rs`; tests `record.rs`, `synth.rs`, `live_input.rs`, `edit.rs`, `control.rs` |
| `core-control` | — | `registry.rs`; tests `commands.rs` |
| `vizctl` | — | `tests/e2e.rs` |
| app | `VizDAWAppModel/AudioCommands.swift`; tests `AudioCommandsTests.swift` | `VizDAWAppModel/ControlCommands.swift`, `VizDAW/ControlChannel.swift`; tests `FakeSession.swift`, `ControlCommandsTests.swift`, `RecordModelTests.swift` |
| docs | — | `docs/feature-backlog.md` |

### Task 1: PDC — `Regions` nodes lead instead of being delayed

**Files:**
- Rewrite: `crates/core-engine/src/graph/pdc.rs` (whole file)
- Modify: `crates/core-engine/src/graph/mod.rs` (export `plan_with`)
- Modify: `crates/core-engine/src/engine.rs` (`compile_graph` plans with no leads until Task 2)
- Test: `crates/core-engine/tests/graph_plan.rs`

**Interfaces:**
- Consumes: `GraphSpec`, `NodeSpec::{Instrument, Regions, Strip}`, `GraphSpec::deps` (unchanged).
- Produces: `core_engine::graph::Plan.leads: Vec<u64>` (per node: a `Regions` node's lead, else 0; a `Regions` node's `t_out` is its lead); `core_engine::graph::plan(spec, max_delay)` plans with leads; `core_engine::graph::plan_with(spec: &GraphSpec, max_delay: u64, leads: bool) -> Result<Plan, PlanError>` (`leads: false` is the old plan, leads all 0). Task 2 builds graphs from `Plan.leads` and switches the engine to `plan_with(.., cfg.region_leads)`.

Spec §6.6 (Ruling 33); plan Ruling 2. Pure code: no engine, no I/O. The engine keeps planning without leads in this task (its renderer cannot read ahead yet), so nothing audible changes.

- [ ] **Step 1: Write the failing tests**

In `crates/core-engine/tests/graph_plan.rs`, import `plan_with`.

Find:
```rust
use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, MAX_NODES, NodeSpec, PlanError, StripSpec, Tap, plan,
};
```
Replace:
```rust
use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, MAX_NODES, NodeSpec, PlanError, StripSpec, Tap, plan,
    plan_with,
};
```

Then append the tests:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-engine/tests/graph_plan.rs <<'VIZDAW_EOF'

fn regions() -> NodeSpec {
    NodeSpec::Regions {
        path: core_model::ids::OutputPathId(0),
    }
}

/// Plan 5d-1 Ruling 2: a Regions node on a path with no plugins leads by
/// the whole latency, so no delay follows it; without leads the same
/// compensation is a delay after its strip.
#[test]
fn a_regions_node_leads_by_what_its_path_lacks() {
    // 0 Regions → 1 audio strip; 2 a bus with a 1000-frame insert;
    // 3 master (1, 2).
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2], vec![]),
        ],
        master: 3,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.latency, 1000);
    assert_eq!(p.leads, vec![1000, 0, 0, 0]);
    assert_eq!(p.t_out[0], 1000);
    assert_eq!(p.input_delays[1], vec![0]);
    assert_eq!(p.input_delays[3], vec![0, 0]);
    let q = plan_with(&g, MAX, false).unwrap();
    assert_eq!(q.leads, vec![0; 4]);
    assert_eq!(q.t_out[0], 0);
    assert_eq!(q.input_delays[3], vec![1000, 0]);
    assert_eq!(q.latency, 1000);
}

/// The plugins on the node's own path still delay it: the lead is what
/// the path lacks of `L`.
#[test]
fn a_regions_nodes_own_inserts_still_delay_it() {
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![ins(300)]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2], vec![]),
        ],
        master: 3,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![700, 0, 0, 0]);
    assert_eq!(p.t_out[1], 1000);
    assert_eq!(p.input_delays[1], vec![0]);
    assert_eq!(p.input_delays[3], vec![0, 0]);
    assert_eq!(p.latency, 1000);
}

/// A send through a latent bus is part of the node's own path: the lead
/// stops at the slowest path, and its direct path keeps the difference
/// as a delay.
#[test]
fn a_send_through_a_latent_bus_limits_the_lead() {
    // 0 Regions → 1 audio strip → master and → 2 bus (500) → master;
    // 3 another strip (1000) → master; 4 master.
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[1], vec![ins(500)]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2, 3], vec![]),
        ],
        master: 4,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![500, 0, 0, 0, 0]);
    assert_eq!(p.input_delays[1], vec![0]);
    assert_eq!(p.input_delays[2], vec![0]);
    assert_eq!(p.input_delays[4], vec![500, 0, 0]);
    assert_eq!(p.latency, 1000);
}

/// No latency, no path to master, or a lead above the cap.
#[test]
fn leads_need_latency_and_a_path_to_master_and_stay_under_the_cap() {
    let flat = GraphSpec {
        nodes: vec![regions(), strip(&[0], vec![]), strip(&[1], vec![])],
        master: 2,
    };
    assert_eq!(plan(&flat, MAX).unwrap().leads, vec![0, 0, 0]);
    // Node 0's strip feeds nothing; master is fed by a latent bus.
    let dead = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![ins(1000)]),
            strip(&[2], vec![]),
        ],
        master: 3,
    };
    assert_eq!(plan(&dead, MAX).unwrap().leads, vec![0, 0, 0, 0]);
    // Max delay 2000: the lead stops at 1000 and the rest is a delay.
    let big = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![ins(1500)]),
            strip(&[1, 2], vec![]),
        ],
        master: 3,
    };
    let p = plan(&big, 2000).unwrap();
    assert_eq!(p.leads, vec![1000, 0, 0, 0]);
    assert_eq!(p.input_delays[3], vec![500, 0]);
    assert_eq!(p.latency, 1500);
}

/// A sidechain fed from a led path would make the latency grow: then
/// nothing leads.
#[test]
fn a_lead_that_would_raise_the_latency_through_a_sidechain_is_dropped() {
    let mut duck = ins(0);
    duck.sidechain = Some(input(1));
    // 0 Regions → 1 audio strip → master; 2 a strip whose ducker listens
    // to 1, then a 200-frame insert; 3 a latent bus (1000); 4 master.
    let g = GraphSpec {
        nodes: vec![
            regions(),
            strip(&[0], vec![]),
            strip(&[], vec![duck, ins(200)]),
            strip(&[], vec![ins(1000)]),
            strip(&[1, 2, 3], vec![]),
        ],
        master: 4,
    };
    let p = plan(&g, MAX).unwrap();
    assert_eq!(p.leads, vec![0; 5]);
    assert_eq!(p.latency, 1000);
    assert_eq!(p, plan_with(&g, MAX, false).unwrap());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_plan 2>&1 | tail -15
```
Expected: FAIL to compile with "unresolved import `core_engine::graph::plan_with`" and "no field `leads` on type `Plan`".

- [ ] **Step 3: Write the planner**

Replace the whole of `crates/core-engine/src/graph/pdc.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/graph/pdc.rs <<'VIZDAW_EOF'
//! Plugin delay compensation and evaluation order (spec §6, non-RT).
//!
//! Every path to master is aligned to the slowest one, the total latency
//! `L`. Since plan 5d-1 (sub-project 5 spec §6.6, Ruling 33) a `Regions`
//! node is compensated by reading ahead instead of being delayed: it gets
//! a *lead*, the compensation its slowest path to master would otherwise
//! add, and plays its regions that many frames before the renderer's
//! frame (`RegionRun` reads `frame − lead`). The plan treats the lead as
//! the node's latency, so no delay line sits after it on that path, and a
//! live input mixed into the node is delayed only by the inserts on its
//! own path.

use crate::graph::spec::{GraphSpec, NodeSpec};
use std::collections::BTreeSet;

/// Most nodes a graph may have.
pub const MAX_NODES: usize = 1024;
/// Most inserts a graph may have.
pub const MAX_INSERTS: usize = 4096;

/// Why a graph cannot be planned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// Beyond [`MAX_NODES`] or [`MAX_INSERTS`].
    #[error("graph too large: {nodes} node(s), {inserts} insert(s) (limits {MAX_NODES} and {MAX_INSERTS})", MAX_NODES = MAX_NODES, MAX_INSERTS = MAX_INSERTS)]
    TooLarge {
        /// Node count.
        nodes: usize,
        /// Insert count.
        inserts: usize,
    },
    /// The edges are cyclic.
    #[error("graph has a cycle")]
    Cycle,
    /// An edge or the master names a node that does not exist.
    #[error("graph refers to node {node}, which does not exist")]
    BadIndex {
        /// The missing index.
        node: usize,
    },
    /// An insert, instrument or edge needs more than the maximum delay.
    #[error("a latency or compensation delay of {frames} frames exceeds the maximum delay")]
    LatencyTooLarge {
        /// The delay needed.
        frames: u64,
    },
}

/// The result of planning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Topological order, ties broken by lower index.
    pub order: Vec<usize>,
    /// Per node, when its (aligned) inputs arrive.
    pub t_in: Vec<u64>,
    /// Per node, when its output is ready (`t_in + latency`; a `Regions`
    /// node's is its lead).
    pub t_out: Vec<u64>,
    /// Per node, the delay of each input edge.
    pub input_delays: Vec<Vec<u32>>,
    /// Per strip node, the delay of each insert's sidechain (0 when none).
    pub sidechain_delays: Vec<Vec<u32>>,
    /// Total latency `L` (master's `t_out`).
    pub latency: u64,
    /// Per node, how many frames ahead of the renderer's frame it plays:
    /// a `Regions` node's lead (plan 5d-1 Ruling 2), 0 for every other
    /// node.
    pub leads: Vec<u64>,
}

fn at(v: &[u64], i: usize) -> u64 {
    v.get(i).copied().unwrap_or(0)
}

fn bounded(frames: u64, max: u64) -> Result<u32, PlanError> {
    if frames > max {
        return Err(PlanError::LatencyTooLarge { frames });
    }
    u32::try_from(frames).map_err(|_| PlanError::LatencyTooLarge { frames })
}

/// Plans `spec` (see the PDC rule in spec §6), giving `Regions` nodes
/// their leads. `max_delay` bounds any one edge delay (2 s of frames in
/// the engine); a lead is at most half of it (1 s), so a read never runs
/// further behind the streamer's playhead than its rings hold.
pub fn plan(spec: &GraphSpec, max_delay: u64) -> Result<Plan, PlanError> {
    plan_with(spec, max_delay, true)
}

/// [`plan`], with `leads` false planning every `Regions` node with no
/// lead: its compensation is a delay line after it, as before plan 5d-1
/// (tests compare the two).
pub fn plan_with(spec: &GraphSpec, max_delay: u64, leads: bool) -> Result<Plan, PlanError> {
    let n = spec.nodes.len();
    let inserts: usize = spec
        .nodes
        .iter()
        .map(|x| match x {
            NodeSpec::Strip(s) => s.inserts.len(),
            NodeSpec::Instrument { .. } | NodeSpec::Regions { .. } => 0,
        })
        .sum();
    if n > MAX_NODES || inserts > MAX_INSERTS {
        return Err(PlanError::TooLarge { nodes: n, inserts });
    }
    if spec.master >= n {
        return Err(PlanError::BadIndex { node: spec.master });
    }
    for x in &spec.nodes {
        let lats: Vec<u32> = match x {
            NodeSpec::Instrument { latency, .. } => vec![*latency],
            NodeSpec::Regions { .. } => Vec::new(),
            NodeSpec::Strip(s) => s.inserts.iter().map(|k| k.latency).collect(),
        };
        for l in lats {
            bounded(u64::from(l), max_delay)?;
        }
    }
    let mut indeg = vec![0usize; n];
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for d in spec.deps(i) {
            if d >= n {
                return Err(PlanError::BadIndex { node: d });
            }
            if let Some(v) = succ.get_mut(d) {
                v.push(i);
            }
            if let Some(x) = indeg.get_mut(i) {
                *x += 1;
            }
        }
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|i| indeg.get(*i) == Some(&0)).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for &s in succ.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(x) = indeg.get_mut(s) {
                *x -= 1;
                if *x == 0 {
                    ready.insert(s);
                }
            }
        }
    }
    if order.len() != n {
        return Err(PlanError::Cycle);
    }
    let none = vec![0u64; n];
    let base = forward(spec, &order, &none, max_delay)?;
    if !leads
        || !spec
            .nodes
            .iter()
            .any(|x| matches!(x, NodeSpec::Regions { .. }))
    {
        return Ok(base);
    }
    let lead = region_leads(spec, &order, &succ, base.latency, max_delay / 2);
    if lead.iter().all(|l| *l == 0) {
        return Ok(base);
    }
    let led = forward(spec, &order, &lead, max_delay)?;
    // A lead never raises `L` along a signal path; it can only through a
    // sidechain fed by a led path (Ruling 2). Then nothing leads.
    if led.latency != base.latency {
        return Ok(base);
    }
    Ok(led)
}

/// The forward pass: arrival and ready times, edge and sidechain delays,
/// with `lead[i]` as `Regions` node `i`'s output time.
fn forward(
    spec: &GraphSpec,
    order: &[usize],
    lead: &[u64],
    max_delay: u64,
) -> Result<Plan, PlanError> {
    let n = spec.nodes.len();
    let mut t_in = vec![0u64; n];
    let mut t_out = vec![0u64; n];
    let mut input_delays = vec![Vec::new(); n];
    let mut sidechain_delays = vec![Vec::new(); n];
    for &i in order {
        let Some(node) = spec.nodes.get(i) else {
            continue;
        };
        match node {
            NodeSpec::Instrument { latency, .. } => {
                if let Some(x) = t_out.get_mut(i) {
                    *x = u64::from(*latency);
                }
            }
            NodeSpec::Regions { .. } => {
                if let Some(x) = t_out.get_mut(i) {
                    *x = at(lead, i);
                }
            }
            NodeSpec::Strip(s) => {
                let mut tin = s
                    .inputs
                    .iter()
                    .map(|x| at(&t_out, x.from))
                    .max()
                    .unwrap_or(0);
                let mut prefix = 0u64;
                for k in &s.inserts {
                    if let Some(sc) = &k.sidechain {
                        tin = tin.max(at(&t_out, sc.from).saturating_sub(prefix));
                    }
                    prefix += u64::from(k.latency);
                }
                let mut d = Vec::with_capacity(s.inputs.len());
                for x in &s.inputs {
                    d.push(bounded(tin.saturating_sub(at(&t_out, x.from)), max_delay)?);
                }
                let mut sd = Vec::with_capacity(s.inserts.len());
                let mut before = 0u64;
                for k in &s.inserts {
                    sd.push(match &k.sidechain {
                        Some(sc) => bounded(
                            (tin + before).saturating_sub(at(&t_out, sc.from)),
                            max_delay,
                        )?,
                        None => 0,
                    });
                    before += u64::from(k.latency);
                }
                if let (Some(a), Some(b), Some(c), Some(e)) = (
                    t_in.get_mut(i),
                    t_out.get_mut(i),
                    input_delays.get_mut(i),
                    sidechain_delays.get_mut(i),
                ) {
                    *a = tin;
                    *b = tin + prefix;
                    *c = d;
                    *e = sd;
                }
            }
        }
    }
    let leads = (0..n)
        .map(|i| match spec.nodes.get(i) {
            Some(NodeSpec::Regions { .. }) => at(lead, i),
            _ => 0,
        })
        .collect();
    Ok(Plan {
        latency: at(&t_out, spec.master),
        order: order.to_vec(),
        t_in,
        t_out,
        input_delays,
        sidechain_delays,
        leads,
    })
}

/// Each `Regions` node's lead: `latency` minus the insert latency on its
/// slowest signal path to master (inputs only: a sidechain carries no
/// signal to master), at most `cap`; 0 for a node that reaches no master.
fn region_leads(
    spec: &GraphSpec,
    order: &[usize],
    succ: &[Vec<usize>],
    latency: u64,
    cap: u64,
) -> Vec<u64> {
    let n = spec.nodes.len();
    // `down[i]`: insert latency from node i's output to master's output
    // along its slowest path; `None` when no path reaches master.
    let mut down: Vec<Option<u64>> = vec![None; n];
    if let Some(m) = down.get_mut(spec.master) {
        *m = Some(0);
    }
    for &i in order.iter().rev() {
        if i == spec.master {
            continue;
        }
        let mut best: Option<u64> = None;
        for &k in succ.get(i).map(Vec::as_slice).unwrap_or(&[]) {
            let Some(NodeSpec::Strip(s)) = spec.nodes.get(k) else {
                continue;
            };
            if k == i || !s.inputs.iter().any(|x| x.from == i) {
                continue;
            }
            let Some(below) = down.get(k).copied().flatten() else {
                continue;
            };
            let own: u64 = s.inserts.iter().map(|x| u64::from(x.latency)).sum();
            let via = own.saturating_add(below);
            best = Some(best.map_or(via, |b| b.max(via)));
        }
        if let Some(d) = down.get_mut(i) {
            *d = best;
        }
    }
    (0..n)
        .map(
            |i| match (spec.nodes.get(i), down.get(i).copied().flatten()) {
                (Some(NodeSpec::Regions { .. }), Some(d)) => latency.saturating_sub(d).min(cap),
                _ => 0,
            },
        )
        .collect()
}
VIZDAW_EOF
```

In `crates/core-engine/src/graph/mod.rs`:

Find:
```rust
pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan};
```
Replace:
```rust
pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan, plan_with};
```

In `crates/core-engine/src/engine.rs`, the engine plans without leads until Task 2:

Find:
```rust
    PlanError, plan,
};
```
Replace:
```rust
    PlanError, plan_with,
};
```

Find:
```rust
        let p = match plan(&mg.spec, max_delay) {
```
Replace:
```rust
        // No leads until the renderer reads regions ahead (plan 5d-1
        // Task 2).
        let p = match plan_with(&mg.spec, max_delay, false) {
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test graph_plan 2>&1 | tail -16
```
Expected: 12 passed (the 7 existing tests and `a_regions_node_leads_by_what_its_path_lacks`, `a_regions_nodes_own_inserts_still_delay_it`, `a_send_through_a_latent_bus_limits_the_lead`, `leads_need_latency_and_a_path_to_master_and_stay_under_the_cap`, `a_lead_that_would_raise_the_latency_through_a_sidechain_is_dropped`).

- [ ] **Step 5: Check the crate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo test -p core-engine 2>&1 | grep -E "test result|FAILED"
```
Expected: no fmt diff, clippy clean, every core-engine test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/graph/pdc.rs crates/core-engine/src/graph/mod.rs crates/core-engine/src/engine.rs crates/core-engine/tests/graph_plan.rs
git commit -m "pdc: Regions nodes lead by what their path lacks of the latency (5d-1 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -6
```

### Task 2: The renderer reads led regions ahead; auto-input at each path's read frame

**Files:**
- Rewrite: `crates/core-engine/src/graph/regions.rs` (whole file)
- Modify: `crates/core-engine/src/graph/exec.rs` (`NodeDef::Regions.lead`, `Graph::max_lead`, `render_node`)
- Modify: `crates/core-engine/src/render.rs` (roll floor, wrap and after-end bookkeeping, `region_run`, `audio_run`, the read head)
- Modify: `crates/core-engine/src/transport.rs` (`active_loop` public)
- Modify: `crates/core-engine/src/record.rs` (`RecordTap::range_or_all`)
- Modify: `crates/core-engine/src/engine.rs` (`EngineConfig.region_leads`; plan with it)
- Test: `crates/core-engine/tests/region_leads.rs` (new), `crates/core-engine/tests/live_input.rs`, `crates/core-engine/tests/rt_alloc.rs`

**Interfaces:**
- Consumes: Task 1's `Plan.leads` and `plan_with`.
- Produces:
  - `core_engine::engine::EngineConfig.region_leads: bool` (default `true`).
  - `core_engine::graph::Graph::max_lead(&self) -> u64`.
  - `core_engine::graph::RegionRun` gains the public fields `floor: u64`, `ceiling: u64`, `wrapped: Option<(u64, u64)>` and `RegionRun::transport(frame, playing) -> RegionRun`, `with_bounds(self, floor, ceiling, wrapped) -> RegionRun`, `read_frame(&self, lead) -> Option<u64>`, `head(&self, lead) -> Option<u64>`; `with_monitor(self, input, routes, offset, playing: bool, rec: Option<(u64, u64)>)` (crate-private) replaces the old `heard` bitmask argument; `render(&self, path, lead, frames, out, scratch)` (crate-private) takes the node's lead.
  - `core_engine::transport::Transport::active_loop(&self) -> Option<(u64, u64)>` is public.
  - `core_engine::record::RecordTap::range_or_all(&self) -> (u64, u64)`.
  - Task 3 extends `RegionRun::add_monitor` and `render_piece` (in this file) to silence the recording lane's regions.

Spec §6.6 (Ruling 33); plan Rulings 2–4. The engine now plans with leads. A led node reads `frame − lead`; `RegionRun::map` keeps that read inside what was played: silent before the roll's start, the loop's end before the loop's start once the loop wrapped (a wrap due at the run's start counts), silent at or past the end of playing, where the renderer keeps counting frames (`after_end`) so a led node plays out its last frames through the tail. The streamer is told the earliest frame read. With leads on, a path plays bit-exactly what the compensation delay played (pinned by `regions_read_ahead_play_exactly_what_a_delay_would`), and its monitored input is no longer delayed by the rest of the mix.

5c's test `auto_monitoring_switches_at_the_audible_punch_points` changes its expectations here: the input is now heard in the very blocks whose read frame is in the punch range, not `L` frames later (plan Ruling 4; the 5c ledger's PDC M4 carry).

- [ ] **Step 1: Write the failing tests**

Create `crates/core-engine/tests/region_leads.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/region_leads.rs <<'VIZDAW_EOF'
//! Regions read ahead instead of being delayed (plan 5d-1 Rulings 2–3;
//! sub-project 5 spec §6.6, Ruling 33): with a latent plugin elsewhere in
//! the mix, a led audio path plays exactly what the compensation delay
//! would have played, through loop wraps, Play Selection and the end of
//! the sequence; after a locate it starts silent, then aligned.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId};
use core_model::time::Tick;
use std::path::Path;

const QF: u64 = 24_000;
const FILE_FRAMES: u64 = 480_000;
const BLOCK: u32 = 512;

fn value(f: u64) -> f32 {
    ((f % 997) as f32 + 1.0) / 2000.0
}

fn caf(path: &Path) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..FILE_FRAMES)
        .flat_map(|f| [value(f), -value(f)])
        .collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

/// One region (beat 1, offset 100, six beats) on lane "Audio"; with
/// `latency` > 0 a bus with a test insert of that latency feeds master,
/// so the graph latency is `latency` and the audio path leads by it.
fn doc(latency: u32) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FILE_FRAMES, 2).unwrap();
    b.region(
        seq,
        lane,
        Q,
        m,
        100,
        6 * QF,
        0.0,
        Fade::default(),
        Fade::default(),
    )
    .unwrap();
    if latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={latency}")))
            .unwrap();
    }
    (b.finish(), seq)
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    _dir: tempfile::TempDir,
}

/// Every region streams (no RAM cache), so the streamer's read head is
/// exercised too.
fn rig(doc: &Document, seq: SeqId, leads: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    cfg.stream = StreamConfig {
        cache_file_bytes: 0,
        ..StreamConfig::default()
    };
    cfg.region_leads = leads;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![MediaFile {
        id: MediaId(0),
        path: dir.path().join("a.caf"),
    }]);
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    Rig {
        engine,
        out,
        _dir: dir,
    }
}

impl Rig {
    /// Renders `blocks` blocks, servicing the disk first; both channels.
    fn render(&mut self, blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for _ in 0..blocks {
            self.engine.streamer().service();
            self.out.block(BLOCK);
            let (a, b) = self.out.output(BLOCK);
            l.extend_from_slice(a);
            r.extend_from_slice(b);
        }
        (l, r)
    }
}

/// What `scenario` renders with and without leads, for graph latency `l`.
fn both(l: u32, scenario: impl Fn(&mut Rig) -> (Vec<f32>, Vec<f32>)) -> [(Vec<f32>, Vec<f32>); 2] {
    let (d, seq) = doc(l);
    [true, false].map(|leads| {
        let mut r = rig(&d, seq, leads);
        assert_eq!(r.engine.latency_frames(), u64::from(l));
        let got = scenario(&mut r);
        let f = r.engine.faults();
        assert!(
            !f.contains(Faults::STREAM_UNDERRUN) && !f.contains(Faults::LATE_START),
            "leads {leads}: {:?}",
            f.names()
        );
        got
    })
}

/// Ruling 3: playing to the end, looping and Play Selection sound
/// bit-exactly as the compensation delay did, for latencies around the
/// block size and the 100 ms a look-ahead limiter may add.
#[test]
fn regions_read_ahead_play_exactly_what_a_delay_would() {
    for l in [1u32, 511, 512, 4800] {
        // From bar 1 through the region's end (beat 7) and the tail.
        let [led, delayed] = both(l, |r| {
            r.engine.play().unwrap();
            r.render(460)
        });
        assert_eq!(led, delayed, "to the end, latency {l}");
        // The region's last frame is heard L late, in both.
        let last = (7 * QF - 1 + u64::from(l)) as usize;
        assert!(led.0[last] != 0.0 && led.0[last + 1] == 0.0, "latency {l}");
        // A loop over beats 2–4, wrapping six times.
        let [led, delayed] = both(l, |r| {
            r.engine.set_loop_range(Some((2 * QF, 4 * QF))).unwrap();
            r.engine.locate_frame(2 * QF).unwrap();
            r.engine.play().unwrap();
            r.render(600)
        });
        assert_eq!(led, delayed, "looping, latency {l}");
        // Play Selection to beat 3 plays out its last frames.
        let [led, delayed] = both(l, |r| {
            r.engine.locate_frame(QF).unwrap();
            r.engine.play_until(Tick(3 * Q)).unwrap();
            r.render(300)
        });
        assert_eq!(led, delayed, "play selection, latency {l}");
    }
}

/// Ruling 3: after a locate a led path reads nothing from before the new
/// position: it is silent for its lead, then plays the new position as
/// it is heard.
#[test]
fn a_located_lead_path_starts_silent_and_aligned() {
    const L: u64 = 4800;
    let (d, seq) = doc(L as u32);
    let mut r = rig(&d, seq, true);
    r.engine.play().unwrap();
    let (before, _) = r.render(100);
    // Heard from beat 1 plus L: the pan law scales by a constant k.
    let k = before[(QF + L) as usize] / value(100);
    assert!(k > 0.5 && k <= 1.0, "{k}");
    r.engine.locate_frame(4 * QF).unwrap();
    let (after, _) = r.render(40);
    assert!(
        after[..L as usize].iter().all(|x| *x == 0.0),
        "nothing from before the locate is heard"
    );
    for i in 0..10_000u64 {
        let want = k * value(100 + 4 * QF - QF + i);
        let got = after[(L + i) as usize];
        assert!((got - want).abs() < 1e-6, "frame {i}: {got} for {want}");
    }
}
VIZDAW_EOF
```

In `crates/core-engine/tests/live_input.rs`, the punch-point test follows Ruling 4, and the own-path test is added after it.

Find:
```rust
/// Fix round 1, M2: Auto monitoring switches when the punch points are
/// heard (the playhead minus the graph latency `L`), not `L` frames early.
#[test]
fn auto_monitoring_switches_at_the_audible_punch_points() {
```
Replace:
```rust
/// Fix round 1, M2: Auto monitoring switches when the punch points are
/// heard, not `L` frames early. Since plan 5d-1 (Rulings 2 and 4) the
/// audio path reads its regions `L` ahead, so the punch points are tested
/// at that read frame and the input is heard with no compensation delay.
#[test]
fn auto_monitoring_switches_at_the_audible_punch_points() {
```

Find:
```rust
    let peaks: Vec<f32> = (0..60).map(|_| b.block(256)).collect();
    // Block k renders playhead 37 600 + 256·k and is heard L later. The
    // punch-in is heard from playhead 42 400 (block 19), whose input is
    // heard from output frame 4864 + L = 7264 (block 28); the punch-out
    // from playhead 47 400 (block 39: output block 48).
    for (k, p) in peaks.iter().enumerate() {
        match k {
            11..=27 | 49.. => assert!(*p < 0.001, "block {k}: {p} ({peaks:?})"),
            29..=47 => assert!(*p > 0.05, "block {k}: {p} ({peaks:?})"),
            _ => {}
        }
    }
}
```
Replace:
```rust
    let peaks: Vec<f32> = (0..60).map(|_| b.block(256)).collect();
    // Block k renders playhead 37 600 + 256·k; the audio path reads
    // 35 200 + 256·k, which is what is heard then. The punch-in is read
    // from block 19 (40 064) and the punch-out from block 39 (45 184), and
    // the input is heard in those same blocks: nothing delays it.
    for (k, p) in peaks.iter().enumerate() {
        match k {
            0..=18 | 39.. => assert!(*p < 0.001, "block {k}: {p} ({peaks:?})"),
            _ => assert!(*p > 0.05, "block {k}: {p} ({peaks:?})"),
        }
    }
}

/// Plan 5d-1 Ruling 2 (spec §6.6; 5c carry PDC M4): a monitored input is
/// delayed only by the plugins on its own path, however latent the rest
/// of the mix is; without leads it waited for the slowest path.
#[test]
fn a_monitored_input_is_heard_through_its_own_path_only() {
    use core_engine::graph::TestInserts;
    use core_model::mixer::StripKind;
    const L: u32 = 2400;
    let d = doc();
    let impulses = |own: u32, leads: bool| -> Vec<usize> {
        let mut bb = Builder::from_doc(d.doc.clone());
        let synth = bb.strip_of(StripKind::BuiltinSynth).unwrap();
        bb.insert(synth, TestInserts::plugin(&format!("latency={L}")))
            .unwrap();
        if own > 0 {
            let path = bb.strip_of(StripKind::AudioPath(d.path)).unwrap();
            bb.insert(path, TestInserts::plugin(&format!("latency={own}")))
                .unwrap();
        }
        let latent = bb.finish();
        let mut cfg = EngineConfig::new(SR);
        cfg.region_leads = leads;
        let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(10_000))).unwrap();
        engine.set_insert_factory(Box::new(TestInserts::new()));
        // Fake input channel 2: an impulse every 0.5 s from frame 0.
        engine.set_inputs(InputSpec {
            midi: vec![],
            monitor: vec![MonitorRoute {
                path: d.path,
                left: 1,
                right: 1,
                mode: MonitorMode::Input,
                armed: false,
            }],
        });
        engine.load(&latent, d.seq).unwrap();
        assert_eq!(engine.latency_frames(), u64::from(L));
        let mut b = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
        b.start(renderer).unwrap();
        let mut left = Vec::new();
        for _ in 0..120 {
            b.block(256);
            left.extend_from_slice(b.output(256).0);
        }
        left.iter()
            .enumerate()
            .filter(|(_, x)| x.abs() > 0.5)
            .map(|(i, _)| i)
            .collect()
    };
    assert_eq!(impulses(0, true), vec![0, 24_000]);
    assert_eq!(impulses(300, true), vec![300, 24_300]);
    assert_eq!(impulses(0, false), vec![2400, 26_400]);
}
```

In `crates/core-engine/tests/rt_alloc.rs`, the regions phase gets a latent bus, so its audio path is led across the loop seam and the locates while the allocator counts.

Find:
```rust
    let audio_doc = bld.finish();
    let mut cfg = EngineConfig::new(48_000.0);
```
Replace:
```rust
    // A latent bus (plan 5d-1 Task 2): the audio path's regions read 4800
    // frames ahead of the renderer, across the loop seam and the locates.
    let bus = bld.bus("Latent").unwrap();
    bld.insert(bus, TestInserts::plugin("latency=4800"))
        .unwrap();
    let audio_doc = bld.finish();
    let mut cfg = EngineConfig::new(48_000.0);
```

Find:
```rust
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_media(vec![core_media::stream::MediaFile {
        id: m,
        path: path.clone(),
    }]);
    let report = engine.load(&audio_doc, seq).unwrap();
```
Replace:
```rust
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![core_media::stream::MediaFile {
        id: m,
        path: path.clone(),
    }]);
    let report = engine.load(&audio_doc, seq).unwrap();
    assert_eq!(engine.latency_frames(), 4800);
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test region_leads --test live_input 2>&1 | tail -8
```
Expected: FAIL to compile with "no field `region_leads` on type `EngineConfig`".

- [ ] **Step 3: Read led regions ahead**

Replace the whole of `crates/core-engine/src/graph/regions.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/graph/regions.rs <<'VIZDAW_EOF'
//! The Regions node's run context and render (sub-project 5 spec §5.4,
//! §6.6; plan 5a Ruling 11, plan 5d-1 Rulings 3–4). // RT
//!
//! A `Regions` node with lead `a` (`Plan.leads`) renders, while the
//! renderer renders frame `f`, the regions at frame `f − a`: ahead of what
//! is heard by its path's own latency, so no compensation delay follows
//! it and a monitored input mixed into it is heard after its own path's
//! plugins only. [`RegionRun::map`] turns a read frame into the frame read:
//! nothing before the roll's start, the loop's end before the loop's start
//! once the loop wrapped, nothing past the end of playing.

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
    /// The run's first frame within the copied block.
    offset: u32,
    /// The transport plays.
    playing: bool,
    /// The record range while a recording rolls (`None`: not recording).
    rec: Option<(u64, u64)>,
}

/// What `Regions` nodes read during one run: the playing schedule's audio,
/// the stream reader, where the run is, and the monitored input. Built by
/// the renderer (or a test `Runner`) before each run.
#[derive(Debug, Clone, Copy)]
pub struct RegionRun {
    audio: *const AudioSchedule,
    reader: *const StreamReader,
    monitor: Option<Monitor>,
    /// Schedule frame the renderer renders at the run's first frame (a
    /// node with lead `a` reads `frame − a`).
    pub frame: u64,
    /// The transport plays through the whole run (or plays out the end).
    pub playing: bool,
    /// Misses count as late starts (spec §5.5, Ruling 10).
    pub late: bool,
    /// The device runs at another rate than the project (Ruling 20).
    pub muted: bool,
    /// Reads before this frame are silent (where the transport rolled
    /// from), unless the loop wrapped.
    pub floor: u64,
    /// Reads at or after this frame are silent (where playing ended;
    /// `u64::MAX` while it plays).
    pub ceiling: u64,
    /// The loop `[start, end)` once the transport wrapped it since it
    /// rolled: a read before its start comes from its end.
    pub wrapped: Option<(u64, u64)>,
}

// SAFETY: the pointees are `Sync` (`AudioSchedule`, `InputCopy` and the
// monitor routes are plain data, `StreamReader` only touches atomics);
// the callers of `RegionRun::new` and `with_monitor` keep them alive while
// any thread renders the run.
unsafe impl Send for RegionRun {}
// SAFETY: as for `Send`.
unsafe impl Sync for RegionRun {}

impl RegionRun {
    /// No audio: Regions nodes render silence.
    pub const NONE: RegionRun = RegionRun {
        audio: std::ptr::null(),
        reader: std::ptr::null(),
        monitor: None,
        frame: 0,
        playing: false,
        late: false,
        muted: false,
        floor: 0,
        ceiling: u64::MAX,
        wrapped: None,
    };

    /// A run reading `audio` through `reader` from schedule frame `frame`.
    ///
    /// # Safety
    /// `audio` and `reader` must stay alive and unchanged until the runs
    /// given this value have finished (`Exec::try_finish` returned true).
    pub unsafe fn new(
        audio: &AudioSchedule,
        reader: &StreamReader,
        frame: u64,
        playing: bool,
        late: bool,
        muted: bool,
    ) -> RegionRun {
        RegionRun {
            audio,
            reader,
            frame,
            playing,
            late,
            muted,
            ..RegionRun::NONE
        }
    }

    /// A run with no audio at frame `frame` (monitoring only).
    pub fn transport(frame: u64, playing: bool) -> RegionRun {
        RegionRun {
            frame,
            playing,
            ..RegionRun::NONE
        }
    }

    /// The run with the roll's start `floor`, the end of playing `ceiling`
    /// and the loop it wrapped (see the fields).
    pub fn with_bounds(self, floor: u64, ceiling: u64, wrapped: Option<(u64, u64)>) -> RegionRun {
        RegionRun {
            floor,
            ceiling,
            wrapped: wrapped.filter(|(s, e)| e > s),
            ..self
        }
    }

    /// The run also mixes the monitor routes into their paths, from frame
    /// `offset` of `input` (spec §6.6): each route as its mode says, with
    /// the transport `playing` and, while a recording rolls, its record
    /// range `rec`.
    ///
    /// # Safety
    /// `input` and `routes` must stay alive and unchanged until the runs
    /// given the result have finished (`Exec::try_finish` returned true).
    pub(crate) unsafe fn with_monitor(
        self,
        input: &InputCopy,
        routes: &[MonitorRoute],
        offset: u32,
        playing: bool,
        rec: Option<(u64, u64)>,
    ) -> RegionRun {
        RegionRun {
            monitor: (!routes.is_empty()).then_some(Monitor {
                input,
                routes,
                offset,
                playing,
                rec,
            }),
            ..self
        }
    }

    /// Where read frame `x` (`frame − lead`, signed) reads from, and for
    /// how many of the next `left` (≥ 1) frames that holds: `None` is
    /// silence. Never 0 frames. // RT
    fn map(&self, x: i128, left: u64) -> (Option<u64>, u64) {
        let left = i128::from(left.max(1));
        if let Some((s, e)) = self.wrapped {
            let (s, e) = (i128::from(s), i128::from(e));
            if x < s {
                // The previous pass of the loop: its end.
                let y = s + (x - s).rem_euclid(e - s);
                let n = left.min(e - y).min(s - x).max(1);
                return (u64::try_from(y).ok(), n as u64);
            }
        } else {
            let floor = i128::from(self.floor);
            if x < floor {
                return (None, left.min(floor - x).max(1) as u64);
            }
        }
        let ceiling = i128::from(self.ceiling);
        if x >= ceiling {
            return (None, left as u64);
        }
        (u64::try_from(x).ok(), left.min(ceiling - x).max(1) as u64)
    }

    /// The frame a node with lead `lead` reads at the run's first frame,
    /// or `None` when it reads silence there. // RT
    pub fn read_frame(&self, lead: u64) -> Option<u64> {
        self.map(i128::from(self.frame) - i128::from(lead), 1).0
    }

    /// The earliest frame a node with lead `lead` will read from the run's
    /// first frame on: the frame it reads, else the roll's start while it
    /// is silent before it; `None` once it reads nothing more (the
    /// streamer's read head, Ruling 3). // RT
    pub fn head(&self, lead: u64) -> Option<u64> {
        if !self.playing {
            return None;
        }
        let x = i128::from(self.frame) - i128::from(lead);
        match self.map(x, 1) {
            (Some(f), _) => Some(f),
            (None, _) if x < i128::from(self.floor) => Some(self.floor),
            (None, _) => None,
        }
    }

    /// Adds the heard monitor routes of `path` to `out`, deciding Auto at
    /// read frame `read` (Ruling 4). // RT
    fn add_monitor(&self, path: OutputPathId, read: Option<u64>, frames: u32, out: &mut StereoBuf) {
        let Some(m) = self.monitor else {
            return;
        };
        // SAFETY: `with_monitor`'s contract keeps both alive for this run.
        let (Some(input), Some(routes)) =
            (unsafe { m.input.as_ref() }, unsafe { m.routes.as_ref() })
        else {
            return;
        };
        let in_range = m
            .rec
            .is_some_and(|(a, b)| read.is_some_and(|x| x >= a && x < b));
        let n = frames as usize;
        let at = m.offset as usize;
        for r in routes.iter().take(64) {
            if r.path != path || !r.heard(m.playing, in_range) {
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

    /// Renders path `path`, whose node leads by `lead`, for `frames`
    /// frames into `out` (cleared first), using `scratch` (at least
    /// `frames` long). // RT
    pub(crate) fn render(
        &self,
        path: OutputPathId,
        lead: u64,
        frames: u32,
        out: &mut StereoBuf,
        scratch: &mut StereoBuf,
    ) {
        out.clear(frames);
        self.add_monitor(path, self.read_frame(lead), frames, out);
        if !self.playing || self.muted {
            return;
        }
        // SAFETY: `new`'s contract keeps both alive for this run.
        let (Some(audio), Some(reader)) = (unsafe { self.audio.as_ref() }, unsafe {
            self.reader.as_ref()
        }) else {
            return;
        };
        let mut off = 0u32;
        let mut x = i128::from(self.frame) - i128::from(lead);
        while off < frames {
            let (src, n) = self.map(x, u64::from(frames - off));
            let n = u32::try_from(n)
                .unwrap_or(frames - off)
                .clamp(1, frames - off);
            if let Some(from) = src {
                render_piece(audio, reader, self.late, path, from, off, n, out, scratch);
            }
            off += n;
            x += i128::from(n);
        }
    }
}

/// Adds path `path`'s segments over schedule frames `[from, from + n)` to
/// frames `[off, off + n)` of `out`. // RT
#[allow(clippy::too_many_arguments)]
fn render_piece(
    audio: &AudioSchedule,
    reader: &StreamReader,
    late: bool,
    path: OutputPathId,
    from: u64,
    off: u32,
    n: u32,
    out: &mut StereoBuf,
    scratch: &mut StereoBuf,
) {
    let to = from.saturating_add(u64::from(n));
    for seg in audio.overlapping(path, from, to) {
        if !seg.online {
            continue;
        }
        let a = from.max(seg.start);
        let b = to.min(seg.end());
        let k = (b - a) as usize;
        let at = off as usize + (a - from) as usize;
        let file = seg.file_frame + (a - seg.start);
        let (Some(sl), Some(sr)) = (scratch.left.get_mut(..k), scratch.right.get_mut(..k)) else {
            continue;
        };
        match reader.read(seg.media, seg.start, seg.file_frame, file, sl, sr) {
            Read::Done => {
                let (Some(ol), Some(or)) =
                    (out.left.get_mut(at..at + k), out.right.get_mut(at..at + k))
                else {
                    continue;
                };
                for i in 0..k {
                    let g = seg.gain_at(file + i as u64);
                    if let (Some(o), Some(x)) = (ol.get_mut(i), sl.get(i)) {
                        *o += x * g;
                    }
                    if let (Some(o), Some(x)) = (or.get_mut(i), sr.get(i)) {
                        *o += x * g;
                    }
                }
            }
            Read::Miss => reader.count_miss(late),
            Read::Offline => {}
        }
    }
}
VIZDAW_EOF
```

In `crates/core-engine/src/graph/exec.rs`, each `Regions` node keeps its lead and the graph its largest:

Find:
```rust
    Regions {
        path: OutputPathId,
    },
    Strip {
```
Replace:
```rust
    Regions {
        path: OutputPathId,
        /// Frames it reads ahead of what is heard (`Plan.leads`).
        lead: u64,
    },
    Strip {
```

Find:
```rust
    master: usize,
    latency: u64,
    shape: u64,
```
Replace:
```rust
    master: usize,
    latency: u64,
    /// The largest `Regions` lead (the streamer's read head trails the
    /// renderer by it).
    max_lead: u64,
    shape: u64,
```

Find:
```rust
                NodeSpec::Regions { path } => {
                    defs.push(NodeDef::Regions { path: *path });
```
Replace:
```rust
                NodeSpec::Regions { path } => {
                    defs.push(NodeDef::Regions {
                        path: *path,
                        lead: plan.leads.get(i).copied().unwrap_or(0),
                    });
```

Find:
```rust
            latency: plan.latency,
            shape: spec.shape(),
```
Replace:
```rust
            latency: plan.latency,
            max_lead: plan.leads.iter().copied().max().unwrap_or(0),
            shape: spec.shape(),
```

Find:
```rust
    /// The structure hash of the spec it was built from.
    pub fn shape(&self) -> u64 {
```
Replace:
```rust
    /// The largest lead of its `Regions` nodes (plan 5d-1 Ruling 3).
    pub fn max_lead(&self) -> u64 {
        self.max_lead
    }

    /// The structure hash of the spec it was built from.
    pub fn shape(&self) -> u64 {
```

Find:
```rust
        NodeDef::Regions { path } => {
            let NodeState { outs, scratch, .. } = st;
            if let Some(out) = outs.first_mut() {
                regions.render(*path, frames, out, scratch);
            }
        }
```
Replace:
```rust
        NodeDef::Regions { path, lead } => {
            let NodeState { outs, scratch, .. } = st;
            if let Some(out) = outs.first_mut() {
                regions.render(*path, *lead, frames, out, scratch);
            }
        }
```

In `crates/core-engine/src/transport.rs`:

Find:
```rust
    fn active_loop(&self) -> Option<(u64, u64)> {
```
Replace:
```rust
    /// The loop playing obeys: the loop range when it is not empty and no
    /// Play Selection end is set. // RT
    pub fn active_loop(&self) -> Option<(u64, u64)> {
```

In `crates/core-engine/src/record.rs`:

Find:
```rust
    /// Whether `frame` lies in the record range. // RT
    pub fn in_range(&self, frame: u64) -> bool {
```
Replace:
```rust
    /// The record range, `(0, u64::MAX)` when it has none. // RT
    pub fn range_or_all(&self) -> (u64, u64) {
        self.range.unwrap_or((0, u64::MAX))
    }

    /// Whether `frame` lies in the record range. // RT
    pub fn in_range(&self, frame: u64) -> bool {
```

In `crates/core-engine/src/engine.rs`:

Find:
```rust
    /// Streaming sizes (spec §5.5).
    pub stream: StreamConfig,
}
```
Replace:
```rust
    /// Streaming sizes (spec §5.5).
    pub stream: StreamConfig,
    /// `Regions` nodes read ahead instead of being delayed (plan 5d-1
    /// Ruling 2); tests turn it off to compare.
    pub region_leads: bool,
}
```

Find:
```rust
            disk: DiskMode::Thread,
            stream: StreamConfig::default(),
        }
```
Replace:
```rust
            disk: DiskMode::Thread,
            stream: StreamConfig::default(),
            region_leads: true,
        }
```

Find:
```rust
        // No leads until the renderer reads regions ahead (plan 5d-1
        // Task 2).
        let p = match plan_with(&mg.spec, max_delay, false) {
```
Replace:
```rust
        let p = match plan_with(&mg.spec, max_delay, self.cfg.region_leads) {
```

In `crates/core-engine/src/render.rs`, the renderer keeps where the transport rolled from, whether the loop wrapped and how far past the end it plays:

Find:
```rust
    /// A return-latency measurement (spec §6.7), while one is installed.
    measure: Option<Box<MeasureTap>>,
}

impl std::fmt::Debug for Renderer {
```
Replace:
```rust
    /// A return-latency measurement (spec §6.7), while one is installed.
    measure: Option<Box<MeasureTap>>,
    /// Where the transport last rolled from (a Play while not playing, a
    /// Locate): led regions read nothing before it (plan 5d-1 Ruling 3).
    roll_floor: u64,
    /// The loop wrapped since the transport rolled.
    wrapped: bool,
    /// Frames rendered since playing reached its end: led regions play
    /// out their last frames meanwhile.
    after_end: u64,
}

impl std::fmt::Debug for Renderer {
```

Find:
```rust
            input_copy: Box::new(InputCopy::new(0, 0)),
            measure: None,
        };
```
Replace:
```rust
            input_copy: Box::new(InputCopy::new(0, 0)),
            measure: None,
            roll_floor: 0,
            wrapped: false,
            after_end: 0,
        };
```

Find:
```rust
        self.reader.begin_block(self.transport.pos);
```
Replace:
```rust
        // Led regions read up to the largest lead behind the transport:
        // the streamer plans from the earliest frame read (Ruling 3).
        let lead = self.graph.as_deref().map_or(0, Graph::max_lead);
        let head = self.audio_run(length).head(lead);
        self.reader.begin_block(head.unwrap_or(self.transport.pos));
```

Find:
```rust
            let run = self.next_run(length, frames - offset);
            let end = offset + run;
```
Replace:
```rust
            let run = self.next_run(length, frames - offset);
            let end = offset + run;
            // A run that starts at or past the end of playing plays out
            // the led regions' last frames (Ruling 3).
            let past_end = self.transport.play_pos(length).is_none()
                && matches!(self.transport.state, PlayState::Playing | PlayState::Tail);
```

Find:
```rust
                    Step::Wrap { offset, to } => {
                        if let Some(t) = self.tap.as_deref_mut() {
                            t.wrap(offset, to);
                        }
```
Replace:
```rust
                    Step::Wrap { offset, to } => {
                        if let Some(t) = self.tap.as_deref_mut() {
                            t.wrap(offset, to);
                        }
                        // Led regions read the loop's end before its start.
                        self.wrapped = true;
```

Find:
```rust
            let lost = self.render_graph(out, offset, run, info, regions, deadline);
            if regions.playing {
                self.since_start = self.since_start.saturating_add(u64::from(run));
            }
```
Replace:
```rust
            let lost = self.render_graph(out, offset, run, info, regions, deadline);
            if regions.playing {
                self.since_start = self.since_start.saturating_add(u64::from(run));
            }
            if past_end {
                self.after_end = self.after_end.saturating_add(u64::from(run));
            }
```

Find:
```rust
                    self.transport.end = self
                        .transport
                        .end
                        .map(|e| new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)));
```
Replace:
```rust
                    self.transport.end = self
                        .transport
                        .end
                        .map(|e| new.tempo.tick_to_frame(old.tempo.frame_to_tick(e)));
                    self.roll_floor = new
                        .tempo
                        .tick_to_frame(old.tempo.frame_to_tick(self.roll_floor));
```

Find:
```rust
                self.transport.end = until;
                if self.transport.state != PlayState::Playing {
                    self.since_start = 0;
                }
                self.transport.play(length);
```
Replace:
```rust
                self.transport.end = until;
                let rolls = self.transport.state != PlayState::Playing;
                if rolls {
                    self.since_start = 0;
                }
                self.transport.play(length);
                if rolls {
                    self.roll_floor = self.transport.pos;
                    self.wrapped = false;
                    self.after_end = 0;
                }
```

Find:
```rust
            EngineCmd::Locate(frame) => {
                self.release_now(true);
                self.transport.locate(frame);
```
Replace:
```rust
            EngineCmd::Locate(frame) => {
                self.release_now(true);
                self.transport.locate(frame);
                self.roll_floor = frame;
                self.wrapped = false;
                self.after_end = 0;
```

Find:
```rust
            EngineCmd::SetLoop(range) => self.transport.loop_range = range,
```
Replace:
```rust
            EngineCmd::SetLoop(range) => {
                // Another loop: its seam is not the one led regions read
                // across (every load sends the same range again).
                if range != self.transport.loop_range {
                    self.wrapped = false;
                }
                self.transport.loop_range = range;
            }
```

Find:
```rust
    /// The `Regions` nodes' context for the run starting now: the
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
        // The record range is where the punch points are heard: the frame
        // rendered now is heard `L` (the graph latency) later, so the
        // frame heard now is `p − L` (fix round 1, M2).
        let in_range = match (self.tap.as_deref(), pos) {
            (Some(t), Some(p)) => t.rolling() && t.in_range(p.saturating_sub(self.latency)),
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
        if s.audio.is_empty() {
            return crate::graph::RegionRun::NONE;
        }
        let Some(pos) = self.transport.play_pos(length) else {
            return crate::graph::RegionRun::NONE;
        };
        // SAFETY: the schedule (boxed) and the reader (boxed) change only
        // in `drain_commands` or `Drop`, after `try_finish` succeeded, so
        // they outlive every run given this value.
        unsafe {
            crate::graph::RegionRun::new(
                &s.audio,
                &self.reader,
                pos,
                true,
                self.since_start < self.late_frames(),
                self.shared.regions_muted(),
            )
        }
    }
```
Replace:
```rust
    /// The `Regions` nodes' context for the run starting now: the
    /// schedule's audio from the playhead (led nodes read behind it, plan
    /// 5d-1 Ruling 3), late within `LATE_SECONDS` of playing after a Play
    /// or Locate, muted when the device refused the rate, and the
    /// monitored input with the record range while a recording rolls
    /// (each node decides Auto at its read frame, Ruling 4). // RT
    fn region_run(&self, length: u64, monitored: bool, offset: u32) -> crate::graph::RegionRun {
        let base = self.audio_run(length);
        if !monitored {
            return base;
        }
        let Some(routes) = self.inputs.as_deref() else {
            return base;
        };
        let playing = self.transport.state == PlayState::Playing;
        let rec = self
            .tap
            .as_deref()
            .filter(|t| t.rolling())
            .map(RecordTap::range_or_all);
        // SAFETY: the routes (boxed) and the input copy (boxed) change only
        // in `drain_commands`, `copy_monitored` or `prepare`, after
        // `try_finish` succeeded, so they outlive every run given this.
        unsafe { base.with_monitor(&self.input_copy, &routes.monitor, offset, playing, rec) }
    }

    /// The schedule's audio from the playhead, or past the end while
    /// playing plays out (none while stopped). // RT
    fn audio_run(&self, length: u64) -> crate::graph::RegionRun {
        let Some(s) = self.schedule.as_deref() else {
            return crate::graph::RegionRun::NONE;
        };
        let (frame, ceiling) = match self.transport.play_pos(length) {
            Some(p) => (p, u64::MAX),
            // At or past the end: led nodes read on to it (Ruling 3).
            None if matches!(self.transport.state, PlayState::Playing | PlayState::Tail) => (
                self.transport.pos.saturating_add(self.after_end),
                self.transport.pos,
            ),
            None => return crate::graph::RegionRun::NONE,
        };
        // A wrap due at this run's start (the transport sits at the loop
        // end) counts: the run plays from the loop start.
        let lp = self.transport.active_loop();
        let due = lp.is_some_and(|(_, e)| self.transport.pos == e);
        let wrapped = if self.wrapped || due { lp } else { None };
        let run = if s.audio.is_empty() {
            crate::graph::RegionRun::transport(frame, true)
        } else {
            // SAFETY: the schedule (boxed) and the reader (boxed) change
            // only in `drain_commands` or `Drop`, after `try_finish`
            // succeeded, so they outlive every run given this value.
            unsafe {
                crate::graph::RegionRun::new(
                    &s.audio,
                    &self.reader,
                    frame,
                    true,
                    self.since_start < self.late_frames(),
                    self.shared.regions_muted(),
                )
            }
        };
        run.with_bounds(self.roll_floor, ceiling, wrapped)
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-engine --test region_leads --test live_input --test graph_regions --test audio_playback 2>&1 | grep -E "^test |test result"
```
Expected: `regions_read_ahead_play_exactly_what_a_delay_would`, `a_located_lead_path_starts_silent_and_aligned`, `a_monitored_input_is_heard_through_its_own_path_only` and `auto_monitoring_switches_at_the_audible_punch_points` pass, with every other test of those files.

- [ ] **Step 5: Check the crates and the real-time gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep "test result"
cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed" ; echo "workspace done"
```
Expected: clippy clean; the three rt tests pass (the regions phase now runs led, with `latency_frames() == 4800`); no failing workspace test (only the known flakes may fail once; re-run them alone).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/graph/regions.rs crates/core-engine/src/graph/exec.rs crates/core-engine/src/render.rs crates/core-engine/src/transport.rs crates/core-engine/src/record.rs crates/core-engine/src/engine.rs crates/core-engine/tests/region_leads.rs crates/core-engine/tests/live_input.rs crates/core-engine/tests/rt_alloc.rs
git commit -m "engine: led audio paths read their regions ahead; a monitored input waits for no other path (5d-1 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -11
```

### Task 3: The recording lane's own regions fall silent while its input is heard

**Files:**
- Modify: `crates/core-engine/src/schedule/audio.rs` (`Segment.top_lane`)
- Modify: `crates/core-engine/src/input.rs` (`MonitorRoute.lane`)
- Modify: `crates/core-engine/src/graph/regions.rs` (`add_monitor` returns the silenced routes; `silenced`; `render_piece` skips their lanes)
- Modify: `crates/core-ffi/src/host.rs` (`input_spec` names the lane)
- Test: `crates/core-engine/tests/live_input.rs`, `crates/core-engine/tests/schedule_audio.rs`, `crates/core-engine/tests/rt_alloc_record.rs`

**Interfaces:**
- Consumes: Task 2's `RegionRun` (`add_monitor`, `render`, `render_piece`).
- Produces: `core_engine::schedule::Segment.top_lane: LaneId` (the playing sequence's lane holding the region, directly or through references); `core_engine::input::MonitorRoute.lane: LaneId` (first field). Every `MonitorRoute` literal names its lane from here on.

Spec §6.6; plan Ruling 6 (5c ledger carry M10). While a pass records, an armed lane whose input is heard at a read frame inside the record range plays none of its own regions there; the decision is the same per-run, per-path one as Auto's (Ruling 4).

- [ ] **Step 1: Write the failing tests**

In `crates/core-engine/tests/live_input.rs`, the test document remembers its audio lane and every monitor route names it:

Find:
```rust
    keys: LaneId,
    synth: DestId,
    path: OutputPathId,
}
```
Replace:
```rust
    keys: LaneId,
    mic: LaneId,
    synth: DestId,
    path: OutputPathId,
}
```

Find:
```rust
        seq,
        keys,
        synth,
        path,
    }
}
```
Replace:
```rust
        seq,
        keys,
        mic: audio,
        synth,
        path,
    }
}
```

Find:
```rust
    let route = |mode, armed| MonitorRoute {
        path: d.path,
```
Replace:
```rust
    let route = |mode, armed| MonitorRoute {
        lane: d.mic,
        path: d.path,
```

Find:
```rust
        monitor: vec![MonitorRoute {
            path: d.path,
            left: 0,
            right: 0,
            mode: MonitorMode::Auto,
            armed: true,
        }],
```
Replace:
```rust
        monitor: vec![MonitorRoute {
            lane: d.mic,
            path: d.path,
            left: 0,
            right: 0,
            mode: MonitorMode::Auto,
            armed: true,
        }],
```

Find:
```rust
            monitor: vec![MonitorRoute {
                path: d.path,
                left: 1,
```
Replace:
```rust
            monitor: vec![MonitorRoute {
                lane: d.mic,
                path: d.path,
                left: 1,
```

Then append the behaviour test:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-engine/tests/live_input.rs <<'VIZDAW_EOF'

/// Plan 5d-1 Ruling 6 (5c carry M10): while a pass records, an armed lane
/// whose input is heard plays none of its own regions inside the record
/// range; outside it, unarmed, or with no recording, they play.
#[test]
fn the_recording_lanes_own_regions_are_silent_while_its_input_is_heard() {
    use core_media::caf::{CafWriter, WriteMode};
    use core_media::stream::{DiskMode, MediaFile};
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.caf");
    let mut w = CafWriter::create(
        &file,
        48_000,
        2,
        core_model::format::BitDepth::Float32,
        WriteMode::Normal,
    )
    .unwrap();
    w.write(&vec![0.25f32; 2 * 240_000]).unwrap();
    w.finish().unwrap();
    // Lane "Mic" plays a 5 s region of 0.25 on "Audio 1"; nothing else.
    let mut b = Builder::new();
    let (seq, mic) = b.sequence("Song", "Mic", None).unwrap();
    let m = b.media("Media/a.caf", 240_000, 2).unwrap();
    b.region(
        seq,
        mic,
        0,
        m,
        0,
        240_000,
        0.0,
        core_model::event::Fade::default(),
        core_model::event::Fade::default(),
    )
    .unwrap();
    let doc = b.finish();
    let path = doc.lane(seq, mic).unwrap().audio_out.unwrap();
    // Fake input channel 3 is silent: only the region can sound.
    let route = |mode, armed| MonitorRoute {
        lane: mic,
        path,
        left: 2,
        right: 2,
        mode,
        armed,
    };
    let blocks = |route: Option<MonitorRoute>, range: Option<(u64, u64)>| -> Vec<bool> {
        let mut cfg = EngineConfig::new(SR);
        cfg.disk = DiskMode::Manual;
        let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(10_000))).unwrap();
        engine.set_media(vec![MediaFile {
            id: m,
            path: file.clone(),
        }]);
        engine.set_inputs(InputSpec {
            midi: vec![],
            monitor: route.into_iter().collect(),
        });
        engine.load(&doc, seq).unwrap();
        let mut out = OfflineBackend::new(SR, 256).with_input(FakeInput::Signals);
        out.start(renderer).unwrap();
        let _tap = range.map(|r| {
            engine
                .start_recording(&RecordRequest {
                    channels: vec![2],
                    range: Some(r),
                    ring_frames: 48_000,
                })
                .unwrap()
        });
        engine.play().unwrap();
        (0..220)
            .map(|_| {
                engine.streamer().service();
                out.block(256) > 0.1
            })
            .collect()
    };
    // Block k reads frames from 256·k: the range [24 000, 48 000) is read
    // from block 94 (24 064) to block 187 (47 872).
    let silent_in_range: Vec<bool> = (0..220).map(|k| !(94..=187).contains(&k)).collect();
    let range = Some((24_000, 48_000));
    assert_eq!(
        blocks(Some(route(MonitorMode::Auto, true)), range),
        silent_in_range,
        "auto, armed, recording"
    );
    assert_eq!(
        blocks(Some(route(MonitorMode::Input, true)), range),
        silent_in_range,
        "input, armed, recording"
    );
    for (r, why) in [
        (Some(route(MonitorMode::Auto, true)), "not recording"),
        (Some(route(MonitorMode::Input, false)), "unarmed"),
        (None, "monitor off"),
    ] {
        let rec = if why == "not recording" { None } else { range };
        assert!(blocks(r, rec).iter().all(|x| *x), "{why}");
    }
}
VIZDAW_EOF
```

In `crates/core-engine/tests/schedule_audio.rs`, the literal names a lane and the reference test checks whose segments they are:

Find:
```rust
        fade_out: fade(10, FadeShape::Linear),
        online: true,
    };
```
Replace:
```rust
        fade_out: fade(10, FadeShape::Linear),
        online: true,
        top_lane: LaneId(0),
    };
```

Find:
```rust
    let (song, sl) = b.sequence("Song", "Refs", None).unwrap();
    b.reference(song, sl, BAR, child, 2, 0).unwrap();
    let (s, _) = run(b.doc(), song, &[]);
    // The child's length rounds up to a barline (4 QF); two uses from bar 2.
    assert_eq!(
        segs(&s.audio, 0),
        vec![(4 * QF, QF, 0, 0), (8 * QF, QF, 0, 0)]
    );
}
```
Replace:
```rust
    let (song, sl) = b.sequence("Song", "Refs", None).unwrap();
    b.reference(song, sl, BAR, child, 2, 0).unwrap();
    let (s, _) = run(b.doc(), song, &[]);
    // The child's length rounds up to a barline (4 QF); two uses from bar 2.
    assert_eq!(
        segs(&s.audio, 0),
        vec![(4 * QF, QF, 0, 0), (8 * QF, QF, 0, 0)]
    );
    // Plan 5d-1 Ruling 6: they belong to the song's lane holding the
    // reference, the lane that records in the song.
    assert!(
        s.audio
            .segments(OutputPathId(0))
            .iter()
            .all(|g| g.top_lane == sl)
    );
    // A region directly on a lane belongs to that lane.
    let (c, _) = run(b.doc(), child, &[]);
    assert!(c.audio.segments(OutputPathId(0)).iter().all(|g| g.top_lane == cl));
}
```

In `crates/core-engine/tests/rt_alloc_record.rs`:

Find:
```rust
        monitor: vec![MonitorRoute {
            path,
            left: 0,
            right: 1,
```
Replace:
```rust
        monitor: vec![MonitorRoute {
            lane,
            path,
            left: 0,
            right: 1,
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test live_input --test schedule_audio 2>&1 | tail -8
```
Expected: FAIL to compile with "struct `MonitorRoute` has no field named `lane`" and "struct `Segment` has no field named `top_lane`".

- [ ] **Step 3: Silence the lane's own segments**

In `crates/core-engine/src/schedule/audio.rs`:

Find:
```rust
    /// The media is readable (else silence, nothing counted).
    pub online: bool,
}

impl Segment {
```
Replace:
```rust
    /// The media is readable (else silence, nothing counted).
    pub online: bool,
    /// The lane of the playing sequence that holds the region, directly
    /// or through references: while that lane records with its input
    /// heard, the segment is silent (plan 5d-1 Ruling 6).
    pub top_lane: LaneId,
}

impl Segment {
```

Find:
```rust
                fade_in,
                fade_out,
                online: !offline.contains(&media),
            },
```
Replace:
```rust
                fade_in,
                fade_out,
                online: !offline.contains(&media),
                top_lane: r.path.first().map_or(last.lane, |p| p.lane),
            },
```

In `crates/core-engine/src/input.rs`:

Find:
```rust
/// One audio lane's monitoring (spec §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRoute {
    /// The output path whose strip hears it.
    pub path: OutputPathId,
```
Replace:
```rust
/// One audio lane's monitoring (spec §6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRoute {
    /// The lane: while it records with its input heard, its own regions
    /// are silent (plan 5d-1 Ruling 6).
    pub lane: LaneId,
    /// The output path whose strip hears it.
    pub path: OutputPathId,
```

In `crates/core-engine/src/graph/regions.rs`:

Find:
```rust
    /// Adds the heard monitor routes of `path` to `out`, deciding Auto at
    /// read frame `read` (Ruling 4). // RT
    fn add_monitor(&self, path: OutputPathId, read: Option<u64>, frames: u32, out: &mut StereoBuf) {
        let Some(m) = self.monitor else {
            return;
        };
        // SAFETY: `with_monitor`'s contract keeps both alive for this run.
        let (Some(input), Some(routes)) =
            (unsafe { m.input.as_ref() }, unsafe { m.routes.as_ref() })
        else {
            return;
        };
        let in_range = m
            .rec
            .is_some_and(|(a, b)| read.is_some_and(|x| x >= a && x < b));
        let n = frames as usize;
        let at = m.offset as usize;
        for r in routes.iter().take(64) {
            if r.path != path || !r.heard(m.playing, in_range) {
                continue;
            }
```
Replace:
```rust
    /// Adds the heard monitor routes of `path` to `out`, deciding Auto at
    /// read frame `read` (Ruling 4). Returns the routes whose lane's own
    /// regions are silent now (bit `k`: route `k`; Ruling 6): armed, heard
    /// and recording inside the record range. // RT
    fn add_monitor(
        &self,
        path: OutputPathId,
        read: Option<u64>,
        frames: u32,
        out: &mut StereoBuf,
    ) -> u64 {
        let Some(m) = self.monitor else {
            return 0;
        };
        // SAFETY: `with_monitor`'s contract keeps both alive for this run.
        let (Some(input), Some(routes)) =
            (unsafe { m.input.as_ref() }, unsafe { m.routes.as_ref() })
        else {
            return 0;
        };
        let in_range = m
            .rec
            .is_some_and(|(a, b)| read.is_some_and(|x| x >= a && x < b));
        let n = frames as usize;
        let at = m.offset as usize;
        let mut silenced = 0u64;
        for (k, r) in routes.iter().enumerate().take(64) {
            if r.path != path || !r.heard(m.playing, in_range) {
                continue;
            }
            if r.armed && in_range {
                silenced |= 1u64 << k;
            }
```

Find:
```rust
            for (dst, ch) in [(&mut out.left, r.left), (&mut out.right, r.right)] {
                let src = input.get(ch, at, n);
                for (d, x) in dst.iter_mut().zip(src) {
                    *d += *x;
                }
            }
        }
    }
```
Replace:
```rust
            for (dst, ch) in [(&mut out.left, r.left), (&mut out.right, r.right)] {
                let src = input.get(ch, at, n);
                for (d, x) in dst.iter_mut().zip(src) {
                    *d += *x;
                }
            }
        }
        silenced
    }

    /// Whether `lane`'s regions are silent in this run: a route in
    /// `silenced` is that lane's. // RT
    fn silenced(&self, silenced: u64, lane: core_model::ids::LaneId) -> bool {
        if silenced == 0 {
            return false;
        }
        let Some(m) = self.monitor else {
            return false;
        };
        // SAFETY: `with_monitor`'s contract keeps the routes alive.
        let Some(routes) = (unsafe { m.routes.as_ref() }) else {
            return false;
        };
        routes
            .iter()
            .enumerate()
            .take(64)
            .any(|(k, r)| silenced & (1u64 << k) != 0 && r.lane == lane)
    }
```

Find:
```rust
        out.clear(frames);
        self.add_monitor(path, self.read_frame(lead), frames, out);
```
Replace:
```rust
        out.clear(frames);
        let silenced = self.add_monitor(path, self.read_frame(lead), frames, out);
```

Find:
```rust
            if let Some(from) = src {
                render_piece(audio, reader, self.late, path, from, off, n, out, scratch);
            }
```
Replace:
```rust
            if let Some(from) = src {
                let skip = |lane| self.silenced(silenced, lane);
                render_piece(
                    audio, reader, self.late, path, from, off, n, &skip, out, scratch,
                );
            }
```

Find:
```rust
/// Adds path `path`'s segments over schedule frames `[from, from + n)` to
/// frames `[off, off + n)` of `out`. // RT
#[allow(clippy::too_many_arguments)]
fn render_piece(
    audio: &AudioSchedule,
    reader: &StreamReader,
    late: bool,
    path: OutputPathId,
    from: u64,
    off: u32,
    n: u32,
    out: &mut StereoBuf,
    scratch: &mut StereoBuf,
) {
    let to = from.saturating_add(u64::from(n));
    for seg in audio.overlapping(path, from, to) {
        if !seg.online {
            continue;
        }
```
Replace:
```rust
/// Adds path `path`'s segments over schedule frames `[from, from + n)` to
/// frames `[off, off + n)` of `out`, but none whose lane `skip` names. // RT
#[allow(clippy::too_many_arguments)]
fn render_piece(
    audio: &AudioSchedule,
    reader: &StreamReader,
    late: bool,
    path: OutputPathId,
    from: u64,
    off: u32,
    n: u32,
    skip: &dyn Fn(core_model::ids::LaneId) -> bool,
    out: &mut StereoBuf,
    scratch: &mut StereoBuf,
) {
    let to = from.saturating_add(u64::from(n));
    for seg in audio.overlapping(path, from, to) {
        if !seg.online || skip(seg.top_lane) {
            continue;
        }
```

In `crates/core-ffi/src/host.rs` (`input_spec`):

Find:
```rust
                    spec.monitor.push(MonitorRoute {
                        path,
                        left: channels.first(),
```
Replace:
```rust
                    spec.monitor.push(MonitorRoute {
                        lane: l.id,
                        path,
                        left: channels.first(),
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-engine --test live_input --test schedule_audio 2>&1 | grep -E "^test |test result"
```
Expected: `the_recording_lanes_own_regions_are_silent_while_its_input_is_heard` and `references_place_their_regions_in_the_parent_tempo` pass, with the rest.

- [ ] **Step 5: Check the crates and the real-time gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep "test result"
cargo test -p core-engine 2>&1 | grep -E "test result|FAILED" | grep -v " 0 failed"; echo engine done
cargo test -p core-ffi --test live_input --test record 2>&1 | grep -E "test result|FAILED"
```
Expected: clean; all pass.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/schedule/audio.rs crates/core-engine/src/input.rs crates/core-engine/src/graph/regions.rs crates/core-ffi/src/host.rs crates/core-engine/tests/live_input.rs crates/core-engine/tests/schedule_audio.rs crates/core-engine/tests/rt_alloc_record.rs
git commit -m "engine: the recording lane's own regions are silent while its input is heard in range (5d-1 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -9
```

### Task 4: Input meters

**Files:**
- Modify: `crates/core-engine/src/render.rs` (`Shared.input_peaks`, `hold_input`, `take_input_peaks`; every block meters its input)
- Modify: `crates/core-engine/src/engine.rs` (`Engine::input_peaks`)
- Modify: `crates/core-ffi/src/host.rs` (`input_levels`)
- Modify: `crates/core-ffi/src/session.rs` (`Session.input_levels`)
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-engine/tests/live_input.rs`, `crates/core-ffi/tests/live_input.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `core_engine::render::Shared::hold_input(&self, c: usize, peak: f32)` (RT), `Shared::take_input_peaks(&self, channels: usize) -> Vec<f32>`; `core_engine::engine::Engine::input_peaks(&self, channels: usize) -> Vec<f32>`; `vizdaw_ffi::host::input_levels(owner: u64) -> Vec<f32>`; `Session.input_levels() -> Result<Vec<f32>, VizError>` (index = device channel, linear peak since the last call; Swift `inputLevels() throws -> [Float]`). Plan 5d-2's lane and mixer meters read it; Task 12's `state.get input` reports it as `levels`.

Spec §6.9 ("a thin input meter while the lane is armed or monitoring, with a clip mark"), §12.1 (`state.get input`: meters); plan Ruling 27. The renderer holds each device input channel's peak in an atomic, whatever lane hears it, on every block (a busy or oversized block is metered too).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-engine/tests/live_input.rs <<'VIZDAW_EOF'

/// Plan 5d-1 Ruling 27 (spec §12.1 meters): every input channel's peak
/// is held whether or not a lane hears it, and a read clears it.
#[test]
fn input_channels_are_metered_whether_heard_or_not() {
    use core_engine::offline::FAKE_SINE_AMPLITUDE;
    let d = doc();
    let (engine, mut b, _log) = start(&d, InputSpec::default(), FakeInput::Signals);
    b.run_frames(2048);
    let p = engine.input_peaks(3);
    assert_eq!(p.len(), 3);
    assert!((p[0] - FAKE_SINE_AMPLITUDE).abs() < 0.01, "{p:?}");
    assert_eq!(p[1], 1.0, "the impulse at frame 0");
    assert_eq!(p[2], 0.0);
    b.run_frames(2048);
    let q = engine.input_peaks(3);
    assert!(q[0] > 0.2 && q[1] == 0.0, "{q:?}");
    assert_eq!(engine.input_peaks(3), vec![0.0; 3], "read and cleared");
}
VIZDAW_EOF
cat >> crates/core-ffi/tests/live_input.rs <<'VIZDAW_EOF'

/// Plan 5d-1 Ruling 27: a session's input levels are the open input's
/// channels, once a lane wants the input; none before.
#[test]
fn input_levels_follow_the_open_input() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = lane(&s, seq, "Loop").id;
    assert!(s.input_levels().unwrap().is_empty(), "no engine");
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: lp,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            }),
        },
    )
    .unwrap();
    take_engine(&s, seq);
    assert!(
        s.input_levels().unwrap().is_empty(),
        "no lane wants the input"
    );
    arm(&s, seq, lp);
    let until = Instant::now() + common::wait_limit();
    loop {
        let l = s.input_levels().unwrap();
        if l.len() == 3 && l[0] > 0.2 {
            assert!(l[0] < 0.26 && l[2] == 0.0, "{l:?}");
            break;
        }
        assert!(Instant::now() < until, "never metered: {l:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test live_input input_channels 2>&1 | tail -5
cargo test -p core-ffi --test live_input input_levels 2>&1 | tail -5
```
Expected: FAIL to compile with "no method named `input_peaks` found for struct `Engine`" and "no method named `input_levels` found for struct `Arc<Session>`".

- [ ] **Step 3: Meter the input**

In `crates/core-engine/src/render.rs`:

Find:
```rust
    meters: Box<[MeterCell]>,
    board: Box<[AtomicU64]>,
}

impl Default for Shared {
```
Replace:
```rust
    meters: Box<[MeterCell]>,
    board: Box<[AtomicU64]>,
    /// Per device input channel, the peak since the last read (`f32` bits;
    /// plan 5d-1 Ruling 27).
    input_peaks: Box<[AtomicU32]>,
}

impl Default for Shared {
```

Find:
```rust
            meters: (0..MAX_NODES).map(|_| MeterCell::default()).collect(),
            board: (0..MAX_INSERTS).map(|_| AtomicU64::new(0)).collect(),
        }
```
Replace:
```rust
            meters: (0..MAX_NODES).map(|_| MeterCell::default()).collect(),
            board: (0..MAX_INSERTS).map(|_| AtomicU64::new(0)).collect(),
            input_peaks: (0..crate::backend::MAX_INPUT_CHANNELS)
                .map(|_| AtomicU32::new(0))
                .collect(),
        }
```

Find:
```rust
    /// The meter of graph node `i`. // RT
    pub fn meter(&self, i: usize) -> Option<&MeterCell> {
        self.meters.get(i)
    }
```
Replace:
```rust
    /// The meter of graph node `i`. // RT
    pub fn meter(&self, i: usize) -> Option<&MeterCell> {
        self.meters.get(i)
    }

    /// Raises input channel `c`'s held peak to `peak` (non-negative floats
    /// order like their bits). // RT
    pub fn hold_input(&self, c: usize, peak: f32) {
        if let Some(cell) = self.input_peaks.get(c) {
            if peak.is_finite() && peak > 0.0 {
                cell.fetch_max(peak.to_bits(), Ordering::Relaxed);
            }
        }
    }

    /// The first `channels` input channels' peaks since the last read,
    /// cleared.
    pub fn take_input_peaks(&self, channels: usize) -> Vec<f32> {
        self.input_peaks
            .iter()
            .take(channels)
            .map(|c| f32::from_bits(c.swap(0, Ordering::Relaxed)))
            .collect()
    }
```

Find:
```rust
        let n = frames as usize;
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        if !self.exec.try_finish(deadline) {
```
Replace:
```rust
        let n = frames as usize;
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        // The input meters see every block, rendered or not.
        for (c, ch) in input.channels.iter().enumerate() {
            let peak = ch
                .iter()
                .take(n)
                .fold(0f32, |m, x| if x.is_finite() { m.max(x.abs()) } else { m });
            self.shared.hold_input(c, peak);
        }
        if !self.exec.try_finish(deadline) {
```

In `crates/core-engine/src/engine.rs`:

Find:
```rust
    pub fn dsp_load(&self) -> DspLoad {
        self.shared.take_load()
    }
```
Replace:
```rust
    pub fn dsp_load(&self) -> DspLoad {
        self.shared.take_load()
    }

    /// The peak of each of the first `channels` device input channels
    /// since the last call, read and cleared (plan 5d-1 Ruling 27).
    pub fn input_peaks(&self, channels: usize) -> Vec<f32> {
        self.shared.take_input_peaks(channels)
    }
```

In `crates/core-ffi/src/host.rs`:

Find:
```rust
/// Whether `owner`'s host has (or, offline, would have) its input open
/// (test hook, Task 11 review I1).
```
Replace:
```rust
/// `owner`'s device input channel peaks since the last read (plan 5d-1
/// Ruling 27): empty unless it holds the engine with its input open.
pub fn input_levels(owner: u64) -> Vec<f32> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner && h.input_open)
        .map(|h| {
            h.engine
                .input_peaks(usize::try_from(h.inputs.channels).unwrap_or(0))
        })
        .unwrap_or_default()
}

/// Whether `owner`'s host has (or, offline, would have) its input open
/// (test hook, Task 11 review I1).
```

In `crates/core-ffi/src/session.rs` (the "Live input" `impl Session` block):

Find:
```rust
    /// What the audio input offers while this document holds the engine
    /// (nothing before its first Play or Record).
    pub fn input_info(&self) -> Result<crate::live::InputInfoRow, VizError> {
```
Replace:
```rust
    /// Each device input channel's peak since the last call, linear (plan
    /// 5d-1 Ruling 27; spec §12.1 `state.get input` meters): empty unless
    /// this document holds the engine with its input open.
    pub fn input_levels(&self) -> Result<Vec<f32>, VizError> {
        ffi_guard(|| Ok(host::input_levels(self.id)))
    }

    /// What the audio input offers while this document holds the engine
    /// (nothing before its first Play or Record).
    pub fn input_info(&self) -> Result<crate::live::InputInfoRow, VizError> {
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
    func inputInfo() throws -> InputInfoRow { inputRow }

```
Replace:
```swift
    func inputInfo() throws -> InputInfoRow { inputRow }

    var inputLevelsRow: [Float] = []

    func inputLevels() throws -> [Float] { inputLevelsRow }

```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-engine --test live_input input_channels 2>&1 | grep -E "^test |test result"
cargo test -p core-ffi --test live_input 2>&1 | grep -E "^test |test result"
```
Expected: both new tests pass, with the rest of both files.

- [ ] **Step 5: Check everything this touches**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep "test result"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: clean; the three rt tests pass (`rt_alloc_record` feeds input every block); Swift builds and every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/render.rs crates/core-engine/src/engine.rs crates/core-ffi/src/host.rs crates/core-ffi/src/session.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-engine/tests/live_input.rs crates/core-ffi/tests/live_input.rs
git commit -m "engine: meter every input channel; Session.input_levels (5d-1 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -9
```

### Task 5: Count-in

**Files:**
- Create: `crates/core-engine/src/count_in.rs`
- Modify: `crates/core-engine/src/lib.rs`, `crates/core-engine/src/render.rs` (`EngineCmd::Play.count_in`, `Renderer.counting`, `Renderer::roll`, `Shared.counting`), `crates/core-engine/src/engine.rs` (`PendingPlay::set_count_in`, `Engine::counting_in`)
- Modify: `crates/core-ffi/src/host.rs` (`HostPlay::count_in`, `HostState.counting_in`), `crates/core-ffi/src/snapshot.rs` (`TransportState.counting_in`), `crates/core-ffi/src/session.rs` (`transport`, `record`/`start_recording`), `crates/core-ffi/src/record.rs` (doc), `crates/core-control/src/registry.rs` (help)
- Test: `crates/core-engine/tests/count_in.rs` (new), `crates/core-engine/tests/rt_alloc.rs`, `crates/core-ffi/tests/record.rs`

**Interfaces:**
- Consumes: Task 2's renderer (`roll_floor` and friends, which the new `Renderer::roll` keeps).
- Produces: `core_engine::count_in::{CountIn { beats: u32, per_bar: u32, beat_frames: f64 }, Counting, ACCENT_HZ, ACCENT_LEVEL, BEAT_HZ, BEAT_LEVEL, CLICK_SECONDS, DECAY_SECONDS}`; `EngineCmd::Play { until, count_in: Option<CountIn> }`; `PendingPlay::set_count_in(&mut self, Option<CountIn>)`; `Engine::counting_in(&self) -> bool`; `vizdaw_ffi::host::HostPlay::count_in(&mut self, Option<CountIn>)`; `HostState.counting_in: bool`; `TransportState.counting_in: bool` (`#[uniffi(default = false)]`, last field; Swift `countingIn`). `Session.record` accepts `count_in_bars` 1–4. Plan 5d-2's toolbar shows "Count-in…" from `countingIn`.

Spec §6.4; plan Ruling 7. 5c refused a count-in above 0 (no click existed). The renderer now counts in instead of rolling when Record asks: the timeline stands still (the transport is Stopped and `counting_in` is true), clicks are mixed straight into the output after the graph, and the transport rolls at the block where the count-in ends. The record tap does not capture before the roll (it starts at the first Playing block, as 5c built it), so the take starts where Record was asked to start.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-engine/tests/count_in.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/count_in.rs <<'VIZDAW_EOF'
//! Record's count-in (sub-project 5 spec §6.4; plan 5d-1 Ruling 7): one
//! click per beat, the first of each bar accented, with the timeline
//! standing still; the transport rolls on the block where the count-in
//! ends, and the clicks are delayed by the graph latency so the downbeat
//! is heard one beat after the last click. Stop cancels it.

use core_engine::backend::AudioBackend;
use core_engine::count_in::{ACCENT_LEVEL, BEAT_LEVEL, CountIn};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, clicks};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::transport::PlayState;

const BLOCK: u32 = 512;

/// An engine on an empty sequence, its graph latency `latency`.
fn rig(latency: u32) -> (Engine, OfflineBackend) {
    let mut b = Builder::new();
    let (seq, _) = b.sequence("Song", "Lane", None).unwrap();
    if latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={latency}")))
            .unwrap();
    }
    let doc = b.finish();
    let (mut engine, renderer) =
        Engine::new(EngineConfig::new(48_000.0), Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, seq).unwrap();
    assert_eq!(engine.latency_frames(), u64::from(latency));
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    (engine, out)
}

/// One 4/4 bar at 120 BPM: four beats of 24 000 frames.
const BAR: CountIn = CountIn {
    beats: 4,
    per_bar: 4,
    beat_frames: 24_000.0,
};

fn count_in(engine: &mut Engine, c: CountIn) {
    let mut p = engine.begin_play(None);
    p.set_count_in(Some(c));
    p.wait();
    engine.finish_play(p).unwrap();
}

/// Renders `blocks` blocks; the left channel, and per block whether the
/// transport played and whether a count-in was on after it.
fn render(
    engine: &Engine,
    out: &mut OfflineBackend,
    blocks: usize,
) -> (Vec<f32>, Vec<(bool, bool)>) {
    let mut left = Vec::new();
    let mut states = Vec::new();
    for _ in 0..blocks {
        out.block(BLOCK);
        left.extend_from_slice(out.output(BLOCK).0);
        states.push((engine.state() == PlayState::Playing, engine.counting_in()));
    }
    (left, states)
}

/// The peak of the 25 ms click starting at `at`.
fn peak(left: &[f32], at: usize) -> f32 {
    left[at..at + 1200].iter().fold(0f32, |m, x| m.max(x.abs()))
}

#[test]
fn a_count_in_clicks_each_beat_then_rolls_on_the_beat() {
    let (mut engine, mut out) = rig(0);
    count_in(&mut engine, BAR);
    let (left, states) = render(&engine, &mut out, 200);
    // 96 000 frames of count-in, padded so the roll lands on a block
    // boundary: 256 frames of silence (96 000 % 512 = 256), clicks at
    // 256 + 24 000·k, the roll at 96 256 (block 188).
    let at: Vec<usize> = (0..4).map(|k| 256 + 24_000 * k).collect();
    let got = clicks(&left, 0, 0.01, 200);
    assert_eq!(got.len(), 4, "{got:?}");
    for (g, a) in got.iter().zip(&at) {
        assert!(*g >= *a && *g <= a + 2, "click at {g}, beat at {a}");
    }
    assert!((peak(&left, at[0]) - ACCENT_LEVEL).abs() < 0.05);
    for a in &at[1..] {
        assert!((peak(&left, *a) - BEAT_LEVEL).abs() < 0.05);
    }
    // The timeline stands still until block 188, then plays.
    for (k, (playing, counting)) in states.iter().enumerate() {
        if k < 188 {
            assert!(!playing && *counting, "block {k}");
        } else {
            assert!(*playing && !counting, "block {k}");
        }
    }
    assert_eq!(engine.transport_frames(), (200 - 188) * u64::from(BLOCK));
}

#[test]
fn the_clicks_wait_for_the_graph_latency_and_the_roll_does_not() {
    let (mut engine, mut out) = rig(1000);
    count_in(&mut engine, BAR);
    let (left, states) = render(&engine, &mut out, 200);
    let got = clicks(&left, 0, 0.01, 200);
    assert_eq!(got.len(), 4, "{got:?}");
    for (k, g) in got.iter().enumerate() {
        let a = 256 + 1000 + 24_000 * k;
        assert!(*g >= a && *g <= a + 2, "click at {g}, beat at {a}");
    }
    // Rolled at 96 256, heard 1000 frames later: a beat after the last
    // click, which is heard at 73 256.
    assert!(states[187].1 && states[188].0);
}

#[test]
fn stop_cancels_a_count_in() {
    let (mut engine, mut out) = rig(0);
    count_in(&mut engine, BAR);
    let (_, states) = render(&engine, &mut out, 60);
    assert!(states.iter().all(|(p, c)| !p && *c));
    engine.stop().unwrap();
    let (left, states) = render(&engine, &mut out, 200);
    assert!(states.iter().all(|(p, c)| !p && !c), "it never rolls");
    assert!(
        clicks(&left, 0, 0.01, 100).is_empty(),
        "no click after the stop"
    );
}
VIZDAW_EOF
```

In `crates/core-engine/tests/rt_alloc.rs`, a count-in phase follows the regions phase:

Find:
```rust
    let f = engine.faults();
    assert!(!f.contains(Faults::STREAM_UNDERRUN), "{:?}", f.names());
}
```
Replace:
```rust
    let f = engine.faults();
    assert!(!f.contains(Faults::STREAM_UNDERRUN), "{:?}", f.names());

    // A count-in (plan 5d-1 Task 5): its clicks and the roll it starts,
    // in blocks of every size.
    engine.stop().unwrap();
    let counted = rt_check::count();
    let mut p = engine.begin_play(None);
    p.set_count_in(Some(core_engine::count_in::CountIn {
        beats: 2,
        per_bar: 2,
        beat_frames: 4800.0,
    }));
    p.wait();
    engine.finish_play(p).unwrap();
    for i in 0..60u32 {
        engine.streamer().service();
        armed_block(&mut b, 1 + (i * 37) % 1024);
    }
    assert_eq!(
        rt_check::count() - counted,
        0,
        "allocations while counting in"
    );
    assert_eq!(engine.state(), PlayState::Playing, "the count-in rolled");
}
```

In `crates/core-ffi/tests/record.rs`, a count-in of 1 is no longer refused:

Find:
```rust
    for bad in [
        RecordSpecRow {
            count_in_bars: 1,
            ..spec()
        },
        RecordSpecRow {
            count_in_bars: 5,
            ..spec()
        },
```
Replace:
```rust
    for bad in [
        RecordSpecRow {
            count_in_bars: 5,
            ..spec()
        },
```

Then append the session test:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/record.rs <<'VIZDAW_EOF'

/// Plan 5d-1 Ruling 7 (spec §6.4): a count-in plays before the transport
/// rolls, the timeline standing still: nothing is captured while it
/// counts, and the take starts where the recording was asked to.
#[test]
fn a_count_in_plays_before_the_pass_and_does_not_move_the_take() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.locate(4 * Q).unwrap();
    s.record(
        d.seq.0,
        RecordSpecRow {
            count_in_bars: 1,
            ..spec()
        },
    )
    .unwrap();
    // One 4/4 bar at 120 BPM: two seconds of clicks.
    wait_until("the count-in", || s.transport().unwrap().counting_in);
    assert_eq!(frames(&s), 0, "nothing is captured while it counts");
    assert!(!s.transport().unwrap().playing);
    assert!(s.record_state().unwrap().recording);
    wait_until("the roll", || {
        !s.transport().unwrap().counting_in && frames(&s) > 0
    });
    wait_until("a second of audio", || frames(&s) >= 48_000);
    s.stop().unwrap();
    let r = regions(&s, d.seq.0, d.mic.0);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].tick, r[0].source_offset), (4 * Q, 0));
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-engine --test count_in 2>&1 | tail -5
cargo test -p core-ffi --test record count_in 2>&1 | tail -5
```
Expected: FAIL to compile with "unresolved import `core_engine::count_in`" and "no field `counting_in` on type `TransportState`".

- [ ] **Step 3: Write the count-in**

Create `crates/core-engine/src/count_in.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/count_in.rs <<'VIZDAW_EOF'
//! The count-in click (sub-project 5 spec §6.4: "Count-in plays the click
//! before the start point without moving the timeline"; plan 5d-1 Ruling
//! 7). The renderer runs a [`Counting`] instead of rolling: it plays one
//! click per beat, the first of each bar accented, then rolls the
//! transport at the block that starts where the count-in ends. The clicks
//! are delayed by the graph latency, so the downbeat is heard one beat
//! after the last click. // RT

/// What Record asks the renderer to count in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CountIn {
    /// Clicks: bars × beats per bar (at least 1).
    pub beats: u32,
    /// Beats per bar: the first of each is accented.
    pub per_bar: u32,
    /// Frames per beat at the tempo of the roll point.
    pub beat_frames: f64,
}

/// A click's length.
pub const CLICK_SECONDS: f64 = 0.025;
/// A click's decay time constant.
pub const DECAY_SECONDS: f64 = 0.006;
/// The accented (first) beat's pitch.
pub const ACCENT_HZ: f64 = 1000.0;
/// The accented beat's peak.
pub const ACCENT_LEVEL: f32 = 0.5;
/// The other beats' pitch.
pub const BEAT_HZ: f64 = 800.0;
/// The other beats' peak.
pub const BEAT_LEVEL: f32 = 0.35;

/// A count-in in progress on the real-time side. `Copy`, so starting one
/// never allocates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Counting {
    c: CountIn,
    /// Where playing stops (the Play's `until`).
    until: Option<u64>,
    rate: f64,
    /// Frames between rendering a click and hearing it (the graph latency
    /// when the count-in started).
    delay: u64,
    /// Silence before the first click, chosen at the first block so the
    /// roll falls on a block boundary; `None` before it.
    pad: Option<u64>,
    /// Frames rendered since it started.
    elapsed: u64,
    rolled: bool,
}

impl Counting {
    /// A count-in for `c` at `rate` Hz with graph latency `latency`, for a
    /// Play that stops at `until`.
    pub fn new(c: CountIn, until: Option<u64>, rate: f64, latency: u64) -> Counting {
        Counting {
            c: CountIn {
                beats: c.beats.max(1),
                per_bar: c.per_bar.max(1),
                beat_frames: if c.beat_frames.is_finite() {
                    c.beat_frames.max(1.0)
                } else {
                    1.0
                },
            },
            until,
            rate: rate.max(1.0),
            delay: latency,
            pad: None,
            elapsed: 0,
            rolled: false,
        }
    }

    /// The Play's `until`.
    pub fn until(&self) -> Option<u64> {
        self.until
    }

    /// Whether the transport has rolled.
    pub fn rolled(&self) -> bool {
        self.rolled
    }

    /// Frames from the first click to the roll.
    fn total(&self) -> u64 {
        (f64::from(self.c.beats) * self.c.beat_frames).round() as u64
    }

    /// Where click `k` starts, in frames since the count-in started.
    fn click_at(&self, k: u32) -> u64 {
        let pad = self.pad.unwrap_or(0);
        pad.saturating_add(self.delay)
            .saturating_add((f64::from(k) * self.c.beat_frames).round() as u64)
    }

    fn click_len(&self) -> u64 {
        (CLICK_SECONDS * self.rate).round() as u64
    }

    /// At the start of a block of `frames` frames: whether the transport
    /// rolls now. The first block chooses the pad, so that at a constant
    /// block size the roll is exactly where the count-in ends. // RT
    pub fn rolls(&mut self, frames: u32) -> bool {
        let n = u64::from(frames.max(1));
        let pad = *self.pad.get_or_insert((n - self.total() % n) % n);
        if !self.rolled && self.elapsed >= pad.saturating_add(self.total()) {
            self.rolled = true;
            return true;
        }
        false
    }

    /// Adds this block's clicks to channels 0–1 of `out` and advances.
    /// // RT
    pub fn render(&mut self, out: &mut [&mut [f32]], frames: u32) {
        let from = self.elapsed;
        let to = from.saturating_add(u64::from(frames));
        let len = self.click_len();
        for k in 0..self.c.beats {
            let at = self.click_at(k);
            if at >= to || at.saturating_add(len) <= from {
                continue;
            }
            let (hz, level) = if k % self.c.per_bar == 0 {
                (ACCENT_HZ, ACCENT_LEVEL)
            } else {
                (BEAT_HZ, BEAT_LEVEL)
            };
            let a = at.max(from);
            let b = at.saturating_add(len).min(to);
            for f in a..b {
                let t = (f - at) as f64 / self.rate;
                let v = (f64::from(level)
                    * (std::f64::consts::TAU * hz * t).sin()
                    * (-t / DECAY_SECONDS).exp()) as f32;
                let i = (f - from) as usize;
                for ch in out.iter_mut().take(2) {
                    if let Some(x) = ch.get_mut(i) {
                        *x += v;
                    }
                }
            }
        }
        self.elapsed = to;
    }

    /// Whether it rolled and every click has sounded.
    pub fn done(&self) -> bool {
        self.rolled
            && self.elapsed
                >= self
                    .click_at(self.c.beats.saturating_sub(1))
                    .saturating_add(self.click_len())
    }
}
VIZDAW_EOF
```

In `crates/core-engine/src/lib.rs`:

Find:
```rust
pub mod backend;
pub mod engine;
```
Replace:
```rust
pub mod backend;
pub mod count_in;
pub mod engine;
```

In `crates/core-engine/src/render.rs`, a Play with a count-in counts instead of rolling, the block where it ends rolls, Stop cancels it, and the old Play body becomes `Renderer::roll`:

Find:
```rust
use crate::backend::{BlockInfo, BlockTime, InputBlock, MidiOut, StereoBuf};
```
Replace:
```rust
use crate::backend::{BlockInfo, BlockTime, InputBlock, MidiOut, StereoBuf};
use crate::count_in::{CountIn, Counting};
```

Find:
```rust
    /// Regions play silence (the device refused the project rate, 5a).
    regions_muted: AtomicBool,
```
Replace:
```rust
    /// Regions play silence (the device refused the project rate, 5a).
    regions_muted: AtomicBool,
    /// A count-in is playing (plan 5d-1 Ruling 7).
    counting: AtomicBool,
```

Find:
```rust
            regions_muted: AtomicBool::new(false),
            position: AtomicU64::new(0),
```
Replace:
```rust
            regions_muted: AtomicBool::new(false),
            counting: AtomicBool::new(false),
            position: AtomicU64::new(0),
```

Find:
```rust
    /// Transport state.
    pub fn state(&self) -> PlayState {
        PlayState::from_u8(self.state.load(Ordering::Relaxed))
    }
```
Replace:
```rust
    /// Transport state.
    pub fn state(&self) -> PlayState {
        PlayState::from_u8(self.state.load(Ordering::Relaxed))
    }

    /// Whether a count-in plays before the transport rolls.
    pub fn counting_in(&self) -> bool {
        self.counting.load(Ordering::Relaxed)
    }
```

Find:
```rust
    /// Start playing; `until` (a schedule frame) stops it there instead
    /// of at the end, ignoring the loop (Play Selection, plan 4f).
    Play {
        /// The frame playing stops at.
        until: Option<u64>,
    },
```
Replace:
```rust
    /// Start playing; `until` (a schedule frame) stops it there instead
    /// of at the end, ignoring the loop (Play Selection, plan 4f). With
    /// `count_in`, a count-in plays first and the transport rolls when it
    /// ends (plan 5d-1 Ruling 7).
    Play {
        /// The frame playing stops at.
        until: Option<u64>,
        /// The count-in to play before rolling.
        count_in: Option<CountIn>,
    },
```

Find:
```rust
    /// Frames rendered since playing reached its end: led regions play
    /// out their last frames meanwhile.
    after_end: u64,
}
```
Replace:
```rust
    /// Frames rendered since playing reached its end: led regions play
    /// out their last frames meanwhile.
    after_end: u64,
    /// The count-in playing before the transport rolls, and its last
    /// clicks after (Ruling 7).
    counting: Option<Counting>,
}
```

Find:
```rust
            roll_floor: 0,
            wrapped: false,
            after_end: 0,
        };
```
Replace:
```rust
            roll_floor: 0,
            wrapped: false,
            after_end: 0,
            counting: None,
        };
```

Find:
```rust
        self.block_ns = time.host_time_ns;
        self.drain_commands();
```
Replace:
```rust
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        // A count-in that ends at this block's start rolls the transport.
        let roll = self
            .counting
            .as_mut()
            .and_then(|c| c.rolls(frames).then(|| c.until()));
        if let Some(until) = roll {
            self.roll(until);
        }
```

Find:
```rust
        self.end_tap(input, frames, !missed);
        self.publish();
```
Replace:
```rust
        if let Some(c) = self.counting.as_mut() {
            c.render(out, frames);
            clamp_master(out, 0, frames, &self.shared);
            if c.done() {
                self.counting = None;
            }
        }
        self.end_tap(input, frames, !missed);
        self.publish();
```

Find:
```rust
            EngineCmd::Play { until } => {
                let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
                self.transport.end = until;
                let rolls = self.transport.state != PlayState::Playing;
                if rolls {
                    self.since_start = 0;
                }
                self.transport.play(length);
                if rolls {
                    self.roll_floor = self.transport.pos;
                    self.wrapped = false;
                    self.after_end = 0;
                }
                self.seek();
                self.forget_unautomated();
                self.auto_last.fill(f64::NAN);
            }
```
Replace:
```rust
            EngineCmd::Play { until, count_in } => {
                match count_in {
                    // Counted in: the transport rolls when it ends.
                    Some(c) if self.transport.state != PlayState::Playing => {
                        self.counting =
                            Some(Counting::new(c, until, self.cfg.sample_rate, self.latency));
                    }
                    _ => self.roll(until),
                }
            }
```

Find:
```rust
                self.release_now(true);
                self.transport.stop();
            }
```
Replace:
```rust
                self.release_now(true);
                self.transport.stop();
                // A count-in still playing never rolls.
                self.counting = None;
            }
```

Find:
```rust
    fn publish(&mut self) {
        let s = &self.shared;
```
Replace:
```rust
    /// Starts playing (a Play, or the end of its count-in). // RT
    fn roll(&mut self, until: Option<u64>) {
        let length = self.schedule.as_ref().map_or(0, |s| s.length_frames);
        self.transport.end = until;
        let rolls = self.transport.state != PlayState::Playing;
        if rolls {
            self.since_start = 0;
        }
        self.transport.play(length);
        if rolls {
            self.roll_floor = self.transport.pos;
            self.wrapped = false;
            self.after_end = 0;
        }
        self.seek();
        self.forget_unautomated();
        self.auto_last.fill(f64::NAN);
    }

    fn publish(&mut self) {
        let s = &self.shared;
        s.counting.store(
            self.counting.is_some_and(|c| !c.rolled()),
            Ordering::Relaxed,
        );
```

In `crates/core-engine/src/engine.rs`:

Find:
```rust
pub struct PendingPlay {
    /// Where to wait for the disk (`None`: no wait).
    wait: Option<(StreamWaiter, u64)>,
    until: Option<Tick>,
}
```
Replace:
```rust
pub struct PendingPlay {
    /// Where to wait for the disk (`None`: no wait).
    wait: Option<(StreamWaiter, u64)>,
    until: Option<Tick>,
    /// The count-in to play before rolling (plan 5d-1 Ruling 7).
    count_in: Option<crate::count_in::CountIn>,
}
```

Find:
```rust
    /// True when [`PendingPlay::wait`] may wait at all.
    pub fn waits(&self) -> bool {
        self.wait.is_some()
    }
```
Replace:
```rust
    /// True when [`PendingPlay::wait`] may wait at all.
    pub fn waits(&self) -> bool {
        self.wait.is_some()
    }

    /// Plays `c` before rolling (Record's count-in, plan 5d-1 Ruling 7).
    pub fn set_count_in(&mut self, c: Option<crate::count_in::CountIn>) {
        self.count_in = c;
    }
```

Find:
```rust
        PendingPlay {
            wait: at.map(|a| (self.streamer.waiter(), a)),
            until,
        }
```
Replace:
```rust
        PendingPlay {
            wait: at.map(|a| (self.streamer.waiter(), a)),
            until,
            count_in: None,
        }
```

Find:
```rust
        self.push(EngineCmd::Play { until })?;
```
Replace:
```rust
        self.push(EngineCmd::Play {
            until,
            count_in: p.count_in,
        })?;
```

Find:
```rust
    pub fn state(&self) -> PlayState {
        self.shared.state()
    }
```
Replace:
```rust
    pub fn state(&self) -> PlayState {
        self.shared.state()
    }

    /// Whether a count-in plays before the transport rolls (plan 5d-1
    /// Ruling 7).
    pub fn counting_in(&self) -> bool {
        self.shared.counting_in()
    }
```

In `crates/core-ffi/src/snapshot.rs` (`TransportState`, the new field last):

Find:
```rust
    #[uniffi(default = false)]
    pub rate_mismatch: bool,
}
```
Replace:
```rust
    #[uniffi(default = false)]
    pub rate_mismatch: bool,
    /// Record's count-in is playing: the transport rolls when it ends
    /// (plan 5d-1 Ruling 7).
    #[uniffi(default = false)]
    pub counting_in: bool,
}
```

In `crates/core-ffi/src/host.rs`:

Find:
```rust
    /// The device runs at another rate than the project: regions are
    /// muted (Ruling 20).
    pub rate_mismatch: bool,
}
```
Replace:
```rust
    /// The device runs at another rate than the project: regions are
    /// muted (Ruling 20).
    pub rate_mismatch: bool,
    /// Record's count-in is playing (plan 5d-1 Ruling 7).
    pub counting_in: bool,
}
```

Find:
```rust
        rate_mismatch: h.rate_mismatch,
    }))
}
```
Replace:
```rust
        rate_mismatch: h.rate_mismatch,
        counting_in: h.engine.counting_in(),
    }))
}
```

Find:
```rust
/// Waits for the disk (up to `LOCATE_WAIT`, without any lock: the UI's
```
Replace:
```rust
impl HostPlay {
    /// Plays count-in `c` before the transport rolls (Record's, plan 5d-1
    /// Ruling 7).
    pub fn count_in(&mut self, c: Option<core_engine::count_in::CountIn>) {
        self.pending.set_count_in(c);
    }
}

/// Waits for the disk (up to `LOCATE_WAIT`, without any lock: the UI's
```

In `crates/core-ffi/src/session.rs`, the transport reports the count-in and Record plays it:

Find:
```rust
                    rate_mismatch: h.rate_mismatch,
                },
```
Replace:
```rust
                    rate_mismatch: h.rate_mismatch,
                    counting_in: h.counting_in,
                },
```

Find:
```rust
                        rate_mismatch: false,
                    }
```
Replace:
```rust
                        rate_mismatch: false,
                        counting_in: false,
                    }
```

Find:
```rust
    /// input does not have, the spec is out of range, or count-in is asked
    /// for (plan 5d plays it); `Busy` while recording or finishing a pass,
```
Replace:
```rust
    /// input does not have, or the spec is out of range; `Busy` while
    /// recording or finishing a pass,
```

Find:
```rust
        if spec.count_in_bars > 0 {
            return Err(crate::events::bad(
                "count-in arrives with the record controls (plan 5d); record with a count-in of 0",
            ));
        }

```
Replace:
```rust

```

Find:
```rust
        let frame = |t: i64| tempo.tick_to_frame(Tick(t));
        let punch = spec.punch.as_ref().map(|p| (frame(p.start), frame(p.end)));
```
Replace:
```rust
        let frame = |t: i64| tempo.tick_to_frame(Tick(t));
        // Count-in (plan 5d-1 Ruling 7): one click per beat of the meter at
        // the roll point, at its tempo, before the transport rolls.
        let count_in = (spec.count_in_bars > 0).then(|| {
            let meter = s.meter.entry_at(play_from).meter;
            let per_bar = u32::from(meter.num.max(1));
            let beat = frame(play_from.0.saturating_add(meter.beat_ticks()))
                .saturating_sub(frame(play_from.0));
            core_engine::count_in::CountIn {
                beats: per_bar * u32::from(spec.count_in_bars),
                per_bar,
                beat_frames: beat as f64,
            }
        });
        let punch = spec.punch.as_ref().map(|p| (frame(p.start), frame(p.end)));
```

Find:
```rust
        let pending = match played {
            Ok(p) => p,
            Err(e) => {
                inner.reload_owed = true;
                return Err(e);
            }
        };
        let fail = |e: VizError| {
```
Replace:
```rust
        let mut pending = match played {
            Ok(p) => p,
            Err(e) => {
                inner.reload_owed = true;
                return Err(e);
            }
        };
        pending.count_in(count_in);
        let fail = |e: VizError| {
```

The Find for the removed refusal ends with an empty line: delete the `if spec.count_in_bars > 0 { … }` block and its blank line, nothing else.

In `crates/core-ffi/src/record.rs`:

Find:
```rust
    /// Bars of count-in, 0–4 (refused above 0 until plan 5d plays them).
    pub count_in_bars: u8,
```
Replace:
```rust
    /// Bars of count-in, 0–4 (plan 5d-1 Ruling 7: clicks before the roll).
    pub count_in_bars: u8,
```

In `crates/core-control/src/registry.rs` (`RECORD_SET_PARAMS`):

Find:
```rust
        help: "Bars of count-in (Record refuses more than 0 until plan 5d plays it).",
```
Replace:
```rust
        help: "Bars of count-in: clicks before the transport rolls.",
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-engine --test count_in 2>&1 | grep -E "^test |test result"
cargo test -p core-ffi --test record count_in 2>&1 | grep -E "^test |test result"
cargo test -p core-ffi --test record bad_requests 2>&1 | grep -E "test result"
```
Expected: `a_count_in_clicks_each_beat_then_rolls_on_the_beat`, `the_clicks_wait_for_the_graph_latency_and_the_roll_does_not`, `stop_cancels_a_count_in` and `a_count_in_plays_before_the_pass_and_does_not_move_the_take` pass; `bad_requests_are_refused_and_recording_holds_the_transport` still passes.

- [ ] **Step 5: Check the crates and the real-time gates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep "test result"
cargo test -p core-engine -p core-control 2>&1 | grep -E "test result|FAILED" | grep -v " 0 failed"; echo done
cargo test -p core-ffi --test record --test transport 2>&1 | grep -E "test result|FAILED"
```
Expected: clean; all pass.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/count_in.rs crates/core-engine/src/lib.rs crates/core-engine/src/render.rs crates/core-engine/src/engine.rs crates/core-ffi/src/host.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/session.rs crates/core-ffi/src/record.rs crates/core-control/src/registry.rs crates/core-engine/tests/count_in.rs crates/core-engine/tests/rt_alloc.rs crates/core-ffi/tests/record.rs
git commit -m "engine: Record counts in with a click before the roll (5d-1 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -14
```

### Task 6: Audio settings — Software Monitoring, I/O buffer size, stream sizes, default monitor mode

**Files:**
- Create: `crates/core-ffi/src/audio.rs`
- Modify: `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/host.rs` (`wants_audio_input`, `input_spec`, `InputInfo.buffer_frames`/`round_trip_frames`, `Host::reopen`, the configs, `any_recording`, `reopen_device`, `drop_if_idle`), `crates/core-ffi/src/live.rs` (`InputInfoRow`), `crates/core-ffi/src/session.rs` (`input_info`, the settings epoch in `idle`, Add Lane's monitor), `crates/core-ffi/src/synth.rs` (doc)
- Test: `crates/core-ffi/tests/audio_settings.rs` (new), `crates/core-ffi/tests/synth.rs`, `crates/core-ffi/tests/control.rs`

**Interfaces:**
- Consumes: Task 3's `input_spec`, Task 4's `Session.input_info` neighbourhood.
- Produces (all `vizdaw_ffi::audio`, exported to Swift as top-level functions): `audio_settings() -> AudioSettingsRow { software_monitoring, buffer_frames, cache_mb, read_ahead_seconds, default_monitor: MonitorModeRow, latency_trim: i32 }`; `set_software_monitoring(on: bool)`; `set_io_buffer_frames(frames: u32) -> Result<(), VizError>`; `set_stream_settings(cache_mb: u32, read_ahead_seconds: u32) -> Result<(), VizError>`; `set_default_monitor_mode(mode: MonitorModeRow)`; Rust-only `software_monitoring()`, `io_buffer_frames()`, `stream_config()`, `default_monitor()`, `epoch()`, test hook `reset_audio_settings()`; constants `BUFFER_SIZES`, `DEFAULT_BUFFER_FRAMES`, `CACHE_MB`, `READ_AHEAD_S`. `InputInfoRow` gains `buffer_frames` and `round_trip_frames` (`#[uniffi(default)]`); `vizdaw_ffi::host::{any_recording, reopen_device, drop_if_idle}`; `host::InputInfo.buffer_frames`, `InputInfo::round_trip_frames()`. The latency trim stays `set_record_latency_trim` (5c). Plan 5d-2's Settings › Audio and › Recording call these; Task 12's `audio.set` too.

Spec §6.6 (Software Monitoring; I/O Buffer Size 32–1024, default 128, refused while recording, with a round-trip readout), §6.9 (RAM cache budget; "Disk read-ahead (2–8 s, default 3 s; ring size follows)"; default monitor mode for new lanes); plan Rulings 8–12 and 24. Also two carried items: the 5c-2 final review's trim test (`the_latency_trim_reaches_the_engine`, `serial()` resets the trim) and its finding 11 (the MIDI output offset note on `return_frames`).

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/audio_settings.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/audio_settings.rs <<'VIZDAW_EOF'
//! The process-wide audio settings (plan 5d-1 Rulings 8–12; sub-project 5
//! spec §6.6, §6.9): Software Monitoring takes effect at once and leaves
//! thru alone; the I/O buffer size is refused while recording; the stream
//! sizes wait for an idle engine; Add Lane takes the default monitor mode
//! in one step; the round trip is reported; hostile values change
//! nothing. Offline only: no device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::audio::{
    audio_settings, reset_audio_settings, set_default_monitor_mode, set_io_buffer_frames,
    set_software_monitoring, set_stream_settings,
};
use vizdaw_ffi::host::{force_offline, hosts_created, reset_host, set_fake_input};
use vizdaw_ffi::{
    EditCommand, LaneEdit, LaneInputRow, LaneRow, MonitorModeRow, RecordSpecRow, Session, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial(input: Option<FakeInput>) -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(input);
    reset_audio_settings();
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

/// Plays and stops, so the session holds the (idle) engine.
fn take_engine(s: &Session, seq: u64) {
    s.play(seq).unwrap();
    let until = Instant::now() + common::wait_limit();
    while !s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never played");
        std::thread::sleep(Duration::from_millis(2));
    }
    s.stop().unwrap();
    while s.transport().unwrap().playing {
        assert!(Instant::now() < until, "never stopped");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The demo's audio lane "Loop" records fake input 1 and is monitored
/// Input; "Chords" plays thru from MIDI source "Keys", armed.
fn monitored(s: &Session, seq: u64) {
    let lp = lane(s, seq, "Loop").id;
    let chords = lane(s, seq, "Chords").id;
    for e in [
        LaneEdit::SetInput {
            seq,
            lane: lp,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            }),
        },
        LaneEdit::SetMonitor {
            seq,
            lane: lp,
            monitor: MonitorModeRow::Input,
        },
        LaneEdit::SetInput {
            seq,
            lane: chords,
            input: Some(LaneInputRow::Midi {
                port: Some("Keys".into()),
                channel: None,
            }),
        },
        LaneEdit::SetFlags {
            seq,
            lane: chords,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    ] {
        edit(s, e).unwrap();
    }
}

/// Spec §6.6, Ruling 8 (Review Focus 4): Software Monitoring off drops
/// the monitor routes at the holder's next idle, without a Play; MIDI
/// thru stays; an unarmed lane monitored Input no longer needs the input.
#[test]
fn software_monitoring_off_drops_monitoring_at_once_but_keeps_thru() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    monitored(&s, seq);
    take_engine(&s, seq);
    let inputs = s.debug_engine_inputs().unwrap();
    assert_eq!((inputs.monitor.len(), inputs.midi.len()), (1, 1));
    assert_eq!(s.debug_input_open(), Some(true));
    set_software_monitoring(false);
    assert!(!audio_settings().software_monitoring);
    s.idle().unwrap();
    wait_until("the monitor routes to go", || {
        s.debug_engine_inputs().unwrap().monitor.is_empty()
    });
    let inputs = s.debug_engine_inputs().unwrap();
    assert_eq!(inputs.midi.len(), 1, "thru stays");
    assert!(inputs.midi[0].thru);
    assert_eq!(s.debug_input_open(), Some(false), "nothing needs input");
    set_software_monitoring(true);
    s.idle().unwrap();
    wait_until("the monitor routes to come back", || {
        s.debug_engine_inputs().unwrap().monitor.len() == 1
    });
    s.close().unwrap();
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

/// Spec §6.6, Ruling 9 (Review Focus 4): the I/O buffer size is one of
/// six, and refused while recording.
#[test]
fn the_buffer_size_is_refused_while_recording() {
    let _g = serial(Some(FakeInput::Signals));
    assert_eq!(audio_settings().buffer_frames, 128, "the default");
    set_io_buffer_frames(256).unwrap();
    assert_eq!(audio_settings().buffer_frames, 256);
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = lane(&s, seq, "Loop").id;
    edit(
        &s,
        LaneEdit::SetInput {
            seq,
            lane: lp,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            }),
        },
    )
    .unwrap();
    edit(
        &s,
        LaneEdit::SetFlags {
            seq,
            lane: lp,
            mute: None,
            solo: None,
            arm: Some(true),
        },
    )
    .unwrap();
    s.record(seq, spec()).unwrap();
    let r = set_io_buffer_frames(512);
    assert!(
        matches!(&r, Err(VizError::Busy { message }) if message.contains("Stop recording")),
        "{r:?}"
    );
    assert_eq!(audio_settings().buffer_frames, 256, "unchanged");
    s.stop().unwrap();
    set_io_buffer_frames(512).unwrap();
    assert_eq!(audio_settings().buffer_frames, 512);
    s.close().unwrap();
}

/// Ruling 10 (Review Focus 4): new stream sizes drop an idle engine (the
/// next Play makes one with them) and leave a playing one alone.
#[test]
fn stream_settings_wait_for_an_idle_engine() {
    let _g = serial(Some(FakeInput::Signals));
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    take_engine(&s, seq);
    let made = hosts_created();
    set_stream_settings(256, 4).unwrap();
    let a = audio_settings();
    assert_eq!((a.cache_mb, a.read_ahead_seconds), (256, 4));
    assert_eq!(vizdaw_ffi::host::holder(), None, "the idle engine went");
    take_engine(&s, seq);
    assert_eq!(hosts_created(), made + 1, "the next Play made one");
    s.play(seq).unwrap();
    wait_until("playing", || s.transport().unwrap().playing);
    set_stream_settings(512, 3).unwrap();
    assert!(vizdaw_ffi::host::holder().is_some(), "the engine stays");
    assert!(s.transport().unwrap().playing, "playing goes on");
    s.stop().unwrap();
    s.close().unwrap();
}

/// Ruling 11: Add Lane gives a new lane the default monitor mode in its
/// one undo step "Add Lane"; Auto is a plain add.
#[test]
fn added_lanes_take_the_default_monitor_mode_in_one_step() {
    let _g = serial(None);
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    set_default_monitor_mode(MonitorModeRow::Off);
    assert_eq!(audio_settings().default_monitor, MonitorModeRow::Off);
    let before = s.lanes(seq).unwrap().len();
    edit(
        &s,
        LaneEdit::Add {
            seq,
            name: "New".into(),
            index: None,
        },
    )
    .unwrap();
    assert_eq!(lane(&s, seq, "New").monitor, MonitorModeRow::Off);
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Add Lane")
    );
    s.undo().unwrap();
    assert_eq!(s.lanes(seq).unwrap().len(), before, "one step");
    set_default_monitor_mode(MonitorModeRow::Auto);
    edit(
        &s,
        LaneEdit::Add {
            seq,
            name: "Plain".into(),
            index: None,
        },
    )
    .unwrap();
    assert_eq!(lane(&s, seq, "Plain").monitor, MonitorModeRow::Auto);
    s.close().unwrap();
}

/// Ruling 9: the round trip is both latencies and a buffer each way (the
/// offline device's buffer is its 512-frame block).
#[test]
fn the_round_trip_is_both_latencies_and_two_buffers() {
    let _g = serial(Some(FakeInput::Loopback(1024)));
    let s = Session::new_demo().unwrap();
    take_engine(&s, song(&s));
    let i = s.input_info().unwrap();
    assert_eq!(i.buffer_frames, 512);
    assert_eq!(i.round_trip_frames, 512 + 512 + 2 * 512);
    s.close().unwrap();
}

/// Review Focus 4: hostile values are `Model` and change nothing.
#[test]
fn hostile_audio_settings_are_refused() {
    let _g = serial(None);
    let before = audio_settings();
    for frames in [0, 1, 31, 33, 100, 127, 2048, u32::MAX] {
        assert!(
            matches!(set_io_buffer_frames(frames), Err(VizError::Model { .. })),
            "{frames}"
        );
    }
    for (mb, s) in [
        (0, 3),
        (63, 3),
        (8193, 3),
        (u32::MAX, 3),
        (512, 0),
        (512, 1),
        (512, 9),
    ] {
        assert!(
            matches!(set_stream_settings(mb, s), Err(VizError::Model { .. })),
            "{mb} MiB, {s} s"
        );
    }
    assert_eq!(audio_settings(), before);
}
VIZDAW_EOF
```

In `crates/core-ffi/tests/synth.rs`, `serial()` resets the trim, and the trim test is appended:

Find:
```rust
    vizdaw_ffi::record::set_free_bytes(None);
    // A test that failed holding a measurement must not hang the next.
```
Replace:
```rust
    vizdaw_ffi::record::set_free_bytes(None);
    // A test that set a trim must not shift the next (5c-2 final review).
    vizdaw_ffi::set_record_latency_trim(0);
    // A test that failed holding a measurement must not hang the next.
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/synth.rs <<'VIZDAW_EOF'

/// 5c-2 final review carry (plan 5d-1 Task 6): the latency trim set for
/// the whole app reaches the engine: a measurement made with a trim of 96
/// frames (2 ms) reads 2 ms more. `serial` puts the trim back to 0.
#[test]
fn the_latency_trim_reaches_the_engine() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::set_record_latency_trim(96);
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let m = s.measure_return(d.synth.0);
    vizdaw_ffi::set_record_latency_trim(0);
    let m = m.unwrap();
    assert!(
        (m.return_latency_ms - (DELAY_MS as f32 + 2.0)).abs() <= 0.25,
        "{m:?}"
    );
    s.close().unwrap();
}
VIZDAW_EOF
```

In `crates/core-ffi/tests/control.rs`, the `InputInfoRow` literal names the new fields:

Find:
```rust
            input_latency_frames: 512,
            output_latency_frames: 512,
        },
    });
```
Replace:
```rust
            input_latency_frames: 512,
            output_latency_frames: 512,
            buffer_frames: 512,
            round_trip_frames: 2048,
        },
    });
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test audio_settings 2>&1 | tail -5
```
Expected: FAIL to compile with "could not find `audio` in `vizdaw_ffi`".

- [ ] **Step 3: Write the settings**

Create `crates/core-ffi/src/audio.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/audio.rs <<'VIZDAW_EOF'
//! Process-wide audio settings (sub-project 5 spec §6.6, §6.9 Settings ›
//! Audio and › Recording; plan 5d-1 Rulings 8–12): Software Monitoring,
//! the I/O buffer size, the streamer's RAM cache and read-ahead, and the
//! monitor mode new lanes get. The app keeps them in its preferences and
//! sets them at launch; the control channel's `audio.set` sets them too.
//! They belong to the machine, not to a project, so none is saved in one.

use crate::edit::MonitorModeRow;
use crate::error::VizError;
use core_media::stream::StreamConfig;
use core_model::lane::MonitorMode;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

/// The I/O buffer sizes a device may be asked for (spec §6.6).
pub const BUFFER_SIZES: [u32; 6] = [32, 64, 128, 256, 512, 1024];
/// The I/O buffer size until one is chosen (spec §6.6: 128).
pub const DEFAULT_BUFFER_FRAMES: u32 = 128;
/// The RAM cache budget's range and default, in MiB (spec §5.5: 512).
pub const CACHE_MB: (u32, u32, u32) = (64, 8192, 512);
/// The disk read-ahead's range and default, in seconds (spec §6.9: 2–8,
/// 3).
pub const READ_AHEAD_S: (u32, u32, u32) = (2, 8, 3);

static SOFTWARE_MONITORING: AtomicBool = AtomicBool::new(true);
static IO_BUFFER: AtomicU32 = AtomicU32::new(DEFAULT_BUFFER_FRAMES);
static CACHE: AtomicU32 = AtomicU32::new(CACHE_MB.2);
static READ_AHEAD: AtomicU32 = AtomicU32::new(READ_AHEAD_S.2);
/// 0 Auto, 1 Input, 2 Off.
static DEFAULT_MONITOR: AtomicU8 = AtomicU8::new(0);
/// Bumped by every change a load publishes (Ruling 12).
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// The audio settings (Settings › Audio and › Recording).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AudioSettingsRow {
    /// Inputs are played back through their strips (spec §6.6).
    pub software_monitoring: bool,
    /// The I/O buffer size asked of the device, in frames.
    pub buffer_frames: u32,
    /// The RAM cache budget, MiB.
    pub cache_mb: u32,
    /// The disk read-ahead, seconds.
    pub read_ahead_seconds: u32,
    /// The monitor mode Add Lane gives a new lane.
    pub default_monitor: MonitorModeRow,
    /// The recording latency trim, frames (spec §6.3).
    pub latency_trim: i32,
}

/// The audio settings now.
#[uniffi::export]
pub fn audio_settings() -> AudioSettingsRow {
    AudioSettingsRow {
        software_monitoring: software_monitoring(),
        buffer_frames: io_buffer_frames(),
        cache_mb: CACHE.load(Ordering::Relaxed),
        read_ahead_seconds: READ_AHEAD.load(Ordering::Relaxed),
        default_monitor: default_monitor().into(),
        latency_trim: i32::try_from(crate::record::latency_trim()).unwrap_or(0),
    }
}

/// Software Monitoring (spec §6.6, Ruling 8): off, no input is played
/// back; recording and MIDI thru go on. The engine holder hears the change
/// at its session's next idle (Ruling 12).
#[uniffi::export]
pub fn set_software_monitoring(on: bool) {
    if SOFTWARE_MONITORING.swap(on, Ordering::Relaxed) != on {
        EPOCH.fetch_add(1, Ordering::Relaxed);
    }
}

/// The I/O buffer size (spec §6.6, Ruling 9): one of [`BUFFER_SIZES`]
/// (else `Model`); `Busy` while any session records. A live device
/// reopens at once with it.
#[uniffi::export]
pub fn set_io_buffer_frames(frames: u32) -> Result<(), VizError> {
    crate::guard::ffi_guard(|| {
        if !BUFFER_SIZES.contains(&frames) {
            return Err(crate::events::bad(
                "the I/O buffer size is 32, 64, 128, 256, 512 or 1024 frames",
            ));
        }
        if crate::host::any_recording() {
            return Err(VizError::Busy {
                message: "Stop recording to change the I/O buffer size.".into(),
            });
        }
        if IO_BUFFER.swap(frames, Ordering::Relaxed) != frames {
            crate::host::reopen_device()?;
        }
        Ok(())
    })
}

/// The RAM cache budget in MiB (64–8192) and the disk read-ahead in
/// seconds (2–8; each stream voice's ring is a second shorter), else
/// `Model` (Ruling 10). They apply to the next engine; an idle one is
/// dropped now so the next Play or arm makes it.
#[uniffi::export]
pub fn set_stream_settings(cache_mb: u32, read_ahead_seconds: u32) -> Result<(), VizError> {
    crate::guard::ffi_guard(|| {
        if !(CACHE_MB.0..=CACHE_MB.1).contains(&cache_mb) {
            return Err(crate::events::bad("the RAM cache is 64 to 8192 MiB"));
        }
        if !(READ_AHEAD_S.0..=READ_AHEAD_S.1).contains(&read_ahead_seconds) {
            return Err(crate::events::bad("the disk read-ahead is 2 to 8 seconds"));
        }
        let a = CACHE.swap(cache_mb, Ordering::Relaxed);
        let b = READ_AHEAD.swap(read_ahead_seconds, Ordering::Relaxed);
        if (a, b) != (cache_mb, read_ahead_seconds) {
            crate::host::drop_if_idle();
        }
        Ok(())
    })
}

/// The monitor mode Add Lane gives new lanes (spec §6.9, Ruling 11).
#[uniffi::export]
pub fn set_default_monitor_mode(mode: MonitorModeRow) {
    let v = match mode {
        MonitorModeRow::Auto => 0,
        MonitorModeRow::Input => 1,
        MonitorModeRow::Off => 2,
    };
    DEFAULT_MONITOR.store(v, Ordering::Relaxed);
}

/// Whether inputs are played back (Ruling 8).
pub fn software_monitoring() -> bool {
    SOFTWARE_MONITORING.load(Ordering::Relaxed)
}

/// The I/O buffer size asked of a device.
pub fn io_buffer_frames() -> u32 {
    IO_BUFFER.load(Ordering::Relaxed)
}

/// The streamer's sizes for the next engine (Ruling 10).
pub fn stream_config() -> StreamConfig {
    let ahead = f64::from(READ_AHEAD.load(Ordering::Relaxed));
    StreamConfig {
        ahead_seconds: ahead,
        voice_seconds: (ahead - 1.0).max(1.0),
        cache_budget_bytes: u64::from(CACHE.load(Ordering::Relaxed)) << 20,
        ..StreamConfig::default()
    }
}

/// The monitor mode new lanes get.
pub fn default_monitor() -> MonitorMode {
    match DEFAULT_MONITOR.load(Ordering::Relaxed) {
        1 => MonitorMode::Input,
        2 => MonitorMode::Off,
        _ => MonitorMode::Auto,
    }
}

/// Changes so far that a load publishes (Ruling 12).
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// Every setting back to its default (test hook: a test binary's tests
/// share the process).
#[cfg(feature = "test-hooks")]
pub fn reset_audio_settings() {
    set_software_monitoring(true);
    IO_BUFFER.store(DEFAULT_BUFFER_FRAMES, Ordering::Relaxed);
    CACHE.store(CACHE_MB.2, Ordering::Relaxed);
    READ_AHEAD.store(READ_AHEAD_S.2, Ordering::Relaxed);
    DEFAULT_MONITOR.store(0, Ordering::Relaxed);
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod blocks;
```
Replace:
```rust
pub mod audio;
pub mod blocks;
```

Find:
```rust
pub use overviews::{OverviewRows, OverviewStateRow};
```
Replace:
```rust
pub use audio::AudioSettingsRow;
pub use overviews::{OverviewRows, OverviewStateRow};
```

In `crates/core-ffi/src/host.rs`, Software Monitoring gates the monitor routes and the input, the device is asked for the buffer size, engines get the stream sizes, and the host can reopen or drop itself:

Find:
```rust
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
```
Replace:
```rust
/// Whether the lanes of `seq` need audio input: an armed lane with an
/// audio input, or one monitored Input while Software Monitoring is on
/// (spec §6.1: input opens only while one is; plan 5d-1 Ruling 8).
pub fn wants_audio_input(doc: &Document, seq: SeqId) -> bool {
    let software = crate::audio::software_monitoring();
    doc.sequence(seq).is_ok_and(|s| {
        s.lanes.iter().any(|l| {
            (l.arm || (software && l.monitor == MonitorMode::Input))
                && l.input.as_ref().is_some_and(LaneInput::is_audio)
        })
    })
}
```

Find:
```rust
    /// Output latency in frames.
    pub output_latency_frames: u32,
}

impl InputInfo {
```
Replace:
```rust
    /// Output latency in frames.
    pub output_latency_frames: u32,
    /// The device's I/O buffer, frames (0 before a device opened).
    pub buffer_frames: u32,
}

impl InputInfo {
```

Find:
```rust
            input_latency_frames: info.input_latency_frames,
            output_latency_frames: info.output_latency_frames,
        }
    }
}
```
Replace:
```rust
            input_latency_frames: info.input_latency_frames,
            output_latency_frames: info.output_latency_frames,
            buffer_frames: info.buffer_frames,
        }
    }

    /// The round trip a monitored input takes (plan 5d-1 Ruling 9): both
    /// latencies and a buffer each way.
    pub fn round_trip_frames(&self) -> u32 {
        self.input_latency_frames
            .saturating_add(self.output_latency_frames)
            .saturating_add(self.buffer_frames.saturating_mul(2))
    }
}
```

Find:
```rust
/// What the lanes of `seq` listen to (spec §6.6): armed or Input-monitored
/// lanes with an input. MIDI lanes play thru unless their monitor is Off;
/// audio lanes are heard through their output path (the document's first
/// when they have none) unless it is Off.
pub fn input_spec(doc: &Document, seq: SeqId) -> InputSpec {
    let mut spec = InputSpec::default();
```
Replace:
```rust
/// What the lanes of `seq` listen to (spec §6.6): armed or Input-monitored
/// lanes with an input. MIDI lanes play thru unless their monitor is Off;
/// audio lanes are heard through their output path (the document's first
/// when they have none) unless it is Off or Software Monitoring is off
/// (plan 5d-1 Ruling 8).
pub fn input_spec(doc: &Document, seq: SeqId) -> InputSpec {
    let software = crate::audio::software_monitoring();
    let mut spec = InputSpec::default();
```

Find:
```rust
            Some(LaneInput::Audio { channels, .. }) if l.monitor != MonitorMode::Off => {
```
Replace:
```rust
            Some(LaneInput::Audio { channels, .. })
                if software && l.monitor != MonitorMode::Off =>
            {
```

Find:
```rust
            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
```
Replace:
```rust
            let mut cfg = EngineConfig::new(rate);
            cfg.stream = crate::audio::stream_config();
            let (engine, renderer) = Engine::new(cfg, Box::new(sinks))?;
```

Find:
```rust
    /// Restarts the device, preparing a larger buffer if needed (as the
    /// CLI does).
```
Replace:
```rust
    /// Reopens a live device as it is (input open or not) with the I/O
    /// buffer size now asked for (plan 5d-1 Ruling 9). Offline outputs
    /// keep their blocks.
    fn reopen(&mut self) -> Result<(), VizError> {
        match &mut self.output {
            Output::Offline(_) => Ok(()),
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                let mut r = b.stop()?;
                let info = b.open(&live_audio_config(self.project_rate, self.input_open))?;
                if info.buffer_frames > r.config().max_frames {
                    r.prepare(info.buffer_frames);
                }
                b.start(r)?;
                self.engine
                    .set_rt_env(rt_env(b, info.buffer_frames, info.sample_rate));
                self.input_open = info.input_channels > 0;
                self.inputs = InputInfo::of(&info);
                Ok(())
            }
        }
    }

    /// Restarts the device, preparing a larger buffer if needed (as the
    /// CLI does).
```

Find:
```rust
    core_engine::backend::AudioConfig {
        sample_rate: Some(project),
        input_enabled: input,
```
Replace:
```rust
    core_engine::backend::AudioConfig {
        sample_rate: Some(project),
        buffer_frames: Some(crate::audio::io_buffer_frames()),
        input_enabled: input,
```

Find:
```rust
    let mut cfg = EngineConfig::new(sample_rate);
    cfg.workers = platform_macos::rt::default_workers();
    cfg.deadline = Some(DEADLINE);
    cfg
}
```
Replace:
```rust
    let mut cfg = EngineConfig::new(sample_rate);
    cfg.workers = platform_macos::rt::default_workers();
    cfg.deadline = Some(DEADLINE);
    cfg.stream = crate::audio::stream_config();
    cfg
}
```

Find:
```rust
/// Releases the engine if `owner` holds it (a session is closing).
```
Replace:
```rust
/// Whether a session records: its tap holds the engine (plan 5d-1 Ruling
/// 9 refuses a buffer change then).
pub fn any_recording() -> bool {
    lock(&HOST).as_ref().is_some_and(|h| h.record_hold)
}

/// Reopens a live device with the I/O buffer size now asked for (plan
/// 5d-1 Ruling 9); nothing for an offline host or none.
pub fn reopen_device() -> Result<(), VizError> {
    match lock(&HOST).as_mut() {
        Some(h) => h.reopen(),
        None => Ok(()),
    }
}

/// Drops the engine when nobody plays, records or measures, so the next
/// one is made with the stream settings now asked for (plan 5d-1 Ruling
/// 10); a busy engine is left alone.
pub fn drop_if_idle() {
    let mut slot = lock(&HOST);
    if !slot
        .as_mut()
        .is_some_and(|h| h.measure.is_none() && !h.busy())
    {
        return;
    }
    let mut old = slot.take();
    // No engine: nothing hears the sources until the next host.
    *lock(&MIDI_TO) = None;
    drop(slot);
    if let Some(h) = old.as_mut() {
        let _ = h.engine.stop();
    }
    retire(old);
}

/// Releases the engine if `owner` holds it (a session is closing).
```

Find:
```rust
            InputInfo {
                channels: 2,
                names: vec!["In 1".into(), "In 2".into()],
                input_latency_frames: 40,
                output_latency_frames: 60,
            }
        );
    }
```
Replace:
```rust
            InputInfo {
                channels: 2,
                names: vec!["In 1".into(), "In 2".into()],
                input_latency_frames: 40,
                output_latency_frames: 60,
                buffer_frames: 256,
            }
        );
        // Plan 5d-1 Ruling 9: both latencies and a buffer each way.
        assert_eq!(InputInfo::of(&info).round_trip_frames(), 40 + 60 + 2 * 256);
    }
```

In `crates/core-ffi/src/live.rs` (`InputInfoRow`):

Find:
```rust
    /// Output latency in frames.
    pub output_latency_frames: u32,
}
```
Replace:
```rust
    /// Output latency in frames.
    pub output_latency_frames: u32,
    /// The device's I/O buffer in frames (plan 5d-1 Ruling 9; 0 before
    /// the first Play).
    #[uniffi(default)]
    pub buffer_frames: u32,
    /// What a monitored input takes from input to output: both latencies
    /// and a buffer each way.
    #[uniffi(default)]
    pub round_trip_frames: u32,
}
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
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
```
Replace:
```rust
                Some((device, i)) => crate::live::InputInfoRow {
                    device,
                    channels: i.channels,
                    round_trip_frames: i.round_trip_frames(),
                    names: i.names,
                    input_latency_frames: i.input_latency_frames,
                    output_latency_frames: i.output_latency_frames,
                    buffer_frames: i.buffer_frames,
                },
                None => crate::live::InputInfoRow {
                    device: String::new(),
                    channels: 0,
                    names: Vec::new(),
                    input_latency_frames: 0,
                    output_latency_frames: 0,
                    buffer_frames: 0,
                    round_trip_frames: 0,
                },
```

Find:
```rust
    synth_mutes: std::collections::BTreeMap<(SeqId, LaneId), LaneId>,
}

/// The plugins the crash marker names
```
Replace:
```rust
    synth_mutes: std::collections::BTreeMap<(SeqId, LaneId), LaneId>,
    /// The audio settings epoch this session's engine last loaded under
    /// (plan 5d-1 Ruling 12).
    settings_seen: u64,
}

/// The plugins the crash marker names
```

Find:
```rust
            recovered: Vec::new(),
            synth_mutes: Default::default(),
        }
```
Replace:
```rust
            recovered: Vec::new(),
            synth_mutes: Default::default(),
            settings_seen: crate::audio::epoch(),
        }
```

Find:
```rust
            let mut inner = lock(&self.inner);
            self.apply_pending_pass(&mut inner);
            if inner.reload_owed {
                inner.reload_host(self.id);
            }
```
Replace:
```rust
            let mut inner = lock(&self.inner);
            self.apply_pending_pass(&mut inner);
            // A setting a load publishes changed (plan 5d-1 Ruling 12).
            let epoch = crate::audio::epoch();
            if inner.settings_seen != epoch {
                inner.settings_seen = epoch;
                inner.reload_owed |= host::holder() == Some(self.id);
            }
            if inner.reload_owed {
                inner.reload_host(self.id);
            }
```

Find:
```rust
            if let crate::edit::EditCommand::Settings { edit } = &cmd {
                crate::modify::check_settings_edit(lock(&self.inner).doc(), edit)?;
            }
            let c = crate::edit::to_command(&cmd)?;
            self.execute_with(c, &filter)
```
Replace:
```rust
            if let crate::edit::EditCommand::Settings { edit } = &cmd {
                crate::modify::check_settings_edit(lock(&self.inner).doc(), edit)?;
            }
            let c = crate::edit::to_command(&cmd)?;
            // Add Lane gives the lane the default monitor mode (plan 5d-1
            // Ruling 11).
            let monitor = crate::audio::default_monitor();
            if let (
                crate::edit::EditCommand::Lane {
                    edit: crate::edit::LaneEdit::Add { seq, .. },
                },
                false,
            ) = (&cmd, monitor == core_model::lane::MonitorMode::Auto)
            {
                return self.add_lane_monitored(c, SeqId(*seq), monitor, &filter);
            }
            self.execute_with(c, &filter)
```

Find:
```rust
    /// [`Session::execute`] with a view filter (editor commands that act
    /// on the selection).
```
Replace:
```rust
    /// Add Lane `cmd` and the new lane's monitor `monitor`, one undo step
    /// "Add Lane" (plan 5d-1 Ruling 11).
    fn add_lane_monitored(
        &self,
        cmd: Command,
        seq: SeqId,
        monitor: core_model::lane::MonitorMode,
        filter: &ViewFilter,
    ) -> Result<DocSnapshot, VizError> {
        let mut inner = lock(&self.inner);
        let h = &mut inner.history;
        h.begin("Add Lane")?;
        let made = h.execute(&cmd, filter).and_then(|r| {
            let lane = r
                .created
                .iter()
                .find_map(|id| match id {
                    core_model::ids::AnyId::Lane(l) => Some(*l),
                    _ => None,
                })
                .ok_or_else(|| core_model::error::ModelError::invalid("no lane was added"))?;
            h.execute(
                &Command::Lane(LaneCmd::SetMonitor { seq, lane, monitor }),
                filter,
            )
        });
        if let Err(e) = made {
            let _ = h.abort();
            return Err(e.into());
        }
        h.commit()?;
        inner.version += 1;
        inner.reload_host(self.id);
        self.mark(&mut inner);
        inner.snapshot()
    }

    /// [`Session::execute`] with a view filter (editor commands that act
    /// on the selection).
```

In `crates/core-ffi/src/synth.rs` (finding 11, Ruling 24):

Find:
```rust
/// Frames a lane recording `dest`'s return moves earlier (spec §6.7; plan
/// 5c-2 Ruling 4): the destination's return latency at `rate`, while it
/// is an external MIDI destination; else 0.
```
Replace:
```rust
/// Frames a lane recording `dest`'s return moves earlier (spec §6.7; plan
/// 5c-2 Ruling 4): the destination's return latency at `rate`, while it
/// is an external MIDI destination; else 0. External MIDI also leaves the
/// engine `midi_offset_ns` after its frame is heard, so a non-zero offset
/// would belong in this shift too; the app has no MIDI output offset (its
/// engines all use 0; plan 5d-1 Ruling 24, 5c-2 final review finding 11).
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-ffi --test audio_settings 2>&1 | grep -E "^test |test result"
cargo test -p core-ffi --test synth the_latency_trim 2>&1 | grep -E "^test |test result"
cargo test -p core-ffi --lib 2>&1 | grep -E "test result"
```
Expected: `software_monitoring_off_drops_monitoring_at_once_but_keeps_thru`, `the_buffer_size_is_refused_while_recording`, `stream_settings_wait_for_an_idle_engine`, `added_lanes_take_the_default_monitor_mode_in_one_step`, `the_round_trip_is_both_latencies_and_two_buffers`, `hostile_audio_settings_are_refused` and `the_latency_trim_reaches_the_engine` pass; the host unit tests pass.

- [ ] **Step 5: Check the crates and the Swift side**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: clean; no failing test (known flakes aside); Swift builds (the new `InputInfoRow` fields have defaults) and passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/audio.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/host.rs crates/core-ffi/src/live.rs crates/core-ffi/src/session.rs crates/core-ffi/src/synth.rs crates/core-ffi/tests/audio_settings.rs crates/core-ffi/tests/synth.rs crates/core-ffi/tests/control.rs
git commit -m "ffi: Software Monitoring, I/O buffer size, stream sizes and the default monitor mode (5d-1 Task 6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -11
```

### Task 7: The input opens on its own thread; the first prompt is asked there; a failed open is retried

**Files:**
- Modify: `crates/platform-macos/src/permission.rs` (`request_microphone`)
- Modify: `crates/core-ffi/src/host.rs` (`Output::Hal` shares the backend; `InputJob`, `Opener`, `Opened`, `open_input`, `reopen_backend`, `hand_back`; `Host.generation`/`job`/`pending_reopen`, `Drop for Host`; `ensure_input`, `apply_input`, `blocked_by_failure`, `start_job`, `finish_job`, `take_events`, `reopen`, `restart_output`; `start_measure` waits; `set_audio_input_device`, `clear_input_failure`, `input_opening`, `wait_input`, `INPUT_WAIT`, `INPUT_OPENING`; test hooks `debug_input_open_delay`, `set_fake_microphone_prompt`, `debug_fail_input_opens`)
- Modify: `crates/core-ffi/src/live.rs` (`InputInfoRow.opening`), `crates/core-ffi/src/edit.rs` (`retries_input`), `crates/core-ffi/src/session.rs` (`input_info`, Record waits, edits clear the failure)
- Test: `crates/core-ffi/tests/input_thread.rs` (new), `crates/core-ffi/tests/control.rs`

**Interfaces:**
- Consumes: Task 6's `Host::reopen` (now through the input thread) and `InputInfoRow` fields.
- Produces: `platform_macos::permission::request_microphone(limit: Duration) -> MicAccess`; `vizdaw_ffi::host::{INPUT_WAIT, INPUT_OPENING, clear_input_failure(), input_opening(owner) -> bool, wait_input(limit) -> bool}`; test hooks `vizdaw_ffi::host::{debug_input_open_delay(Option<Duration>), set_fake_microphone_prompt(Option<bool>), debug_fail_input_opens(u32)}`; `InputInfoRow.opening: bool` (`#[uniffi(default)]`; Swift `opening`); `vizdaw_ffi::edit::retries_input(&EditCommand) -> bool`. Task 8's engine claim and plan 5d-2's lane header read `opening`.

Spec §6.1 (input opened only when needed; the microphone permission); plan Rulings 13–15 (5c carries T11 M6, M2, M3). Design:
- A live host's backend sits behind its own `Mutex` (`Output::Hal(Arc<Mutex<CoreAudioBackend>>)`). An input change starts a short-lived "vizdaw-input" thread (`start_job`) that takes only the device's lock: it asks for the microphone first when macOS never asked, then stops, reopens and restarts the device with the same renderer (`reopen_backend`, the old `apply_input` body). It hands its result back with `HOST.try_lock` (never a blocking lock: a host being dropped holds the host lock while it joins the thread, after setting `cancel`), and only to the host that started it (`generation`).
- While it works, `job` is set: input changes wait in `pending_input` (as during a record hold or a measurement), a buffer-size reopen waits in `pending_reopen`, `take_events` skips the device (`try_lock`), `restart_output` leaves the size to the reopen.
- Offline hosts change at once, as before, unless a test hook asks for the thread (`debug_input_open_delay`, `set_fake_microphone_prompt`) or for a failure (`debug_fail_input_opens`, which fails like a busy interface: `input_failed`).
- Record (a pass with audio lanes) and Measure wait up to `INPUT_WAIT` (3 s) for the thread, without the host lock; then Busy `INPUT_OPENING`. Measure checks afterwards that nothing took the engine meanwhile.
- `input_failed` is cleared by `set_audio_input_device` and by every edit of a lane's input or arm (`retries_input`), never by Play.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/input_thread.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/input_thread.rs <<'VIZDAW_EOF'
//! The input opens on its own thread (plan 5d-1 Rulings 13–15; 5c carries
//! T11 M2, M3 and M6): an arm returns at once while the device opens, the
//! transport and the meters stay answerable meanwhile, Record waits for
//! the input (a bounded while), a denial at the first microphone prompt is
//! reported when arming, and an input that failed to open is tried again
//! when a lane's input or arm changes. The offline device's test hooks
//! stand in for the device's slowness, the prompt and a busy interface:
//! no device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use platform_macos::permission::DENIED_MESSAGE;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{
    debug_fail_input_opens, debug_input_open_delay, force_offline, reset_host, set_fake_input,
    set_fake_microphone_denied, set_fake_microphone_prompt,
};
use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow, LaneRow, RecordSpecRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(Some(FakeInput::Signals));
    debug_input_open_delay(None);
    set_fake_microphone_prompt(None);
    set_fake_microphone_denied(false);
    debug_fail_input_opens(0);
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

fn arm(s: &Session, seq: u64, lane: u64, on: bool) {
    edit(
        s,
        LaneEdit::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(on),
        },
    )
    .unwrap();
}

/// Plays and stops, so the session holds the (idle) engine.
fn take_engine(s: &Session, seq: u64) {
    s.play(seq).unwrap();
    wait_until("playing", || s.transport().unwrap().playing);
    s.stop().unwrap();
    wait_until("stopped", || !s.transport().unwrap().playing);
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The demo's lane "Loop" records fake input 1 (not armed yet).
fn audio_lane(s: &Session, seq: u64) -> u64 {
    let lp = lane(s, seq, "Loop").id;
    edit(
        s,
        LaneEdit::SetInput {
            seq,
            lane: lp,
            input: Some(LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            }),
        },
    )
    .unwrap();
    lp
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

/// Ruling 13 (5c carry T11 M6; Review Focus 3): arming returns while the
/// input opens on its own thread, and the transport and the meters
/// answer meanwhile.
#[test]
fn an_input_change_runs_off_the_calling_thread() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    debug_input_open_delay(Some(Duration::from_millis(600)));
    let t = Instant::now();
    arm(&s, seq, lp, true);
    assert!(
        t.elapsed() < Duration::from_millis(300),
        "{:?}",
        t.elapsed()
    );
    assert!(s.input_info().unwrap().opening, "it opens meanwhile");
    assert_eq!(s.debug_input_open(), Some(false));
    let t = Instant::now();
    let _ = s.transport().unwrap();
    let _ = s.levels().unwrap();
    let _ = s.input_levels().unwrap();
    assert!(
        t.elapsed() < Duration::from_millis(200),
        "{:?}",
        t.elapsed()
    );
    wait_until("the input to open", || !s.input_info().unwrap().opening);
    assert_eq!(s.debug_input_open(), Some(true));
    // Disarming while it opens: the input ends closed.
    arm(&s, seq, lp, false);
    arm(&s, seq, lp, true);
    arm(&s, seq, lp, false);
    wait_until("the input to settle", || !s.input_info().unwrap().opening);
    assert_eq!(s.debug_input_open(), Some(false), "the last asked wins");
    s.close().unwrap();
}

/// Ruling 13 (Review Focus 3): Record waits for an input that is opening,
/// and is Busy (recording nothing) when it takes longer than
/// `INPUT_WAIT`.
#[test]
fn record_waits_for_an_input_that_is_opening() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    debug_input_open_delay(Some(Duration::from_millis(500)));
    arm(&s, seq, lp, true);
    assert!(s.input_info().unwrap().opening);
    s.record(seq, spec()).unwrap();
    assert!(s.record_state().unwrap().recording);
    assert!(!s.input_info().unwrap().opening, "Record waited for it");
    s.stop().unwrap();
    // Longer than the wait: Busy, nothing records.
    arm(&s, seq, lp, false);
    wait_until("closed", || s.debug_input_open() == Some(false));
    debug_input_open_delay(Some(vizdaw_ffi::host::INPUT_WAIT + Duration::from_secs(2)));
    arm(&s, seq, lp, true);
    let r = s.record(seq, spec());
    assert!(
        matches!(&r, Err(VizError::Busy { message }) if message.contains("still opening")),
        "{r:?}"
    );
    assert!(!s.record_state().unwrap().recording);
    debug_input_open_delay(None);
    wait_until("the input to open", || s.debug_input_open() == Some(true));
    s.close().unwrap();
}

/// Ruling 14 (5c carry T11 M2; Review Focus 3): a "no" to the first
/// microphone prompt is reported when arming, and Record refuses with it,
/// instead of a pass recording silence.
#[test]
fn a_denied_first_prompt_is_reported_before_recording() {
    let _g = serial();
    // A device whose input macOS refused offers no input channels.
    set_fake_input(None);
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    set_fake_microphone_prompt(Some(false));
    arm(&s, seq, lp, true);
    wait_until("the prompt's answer", || !s.input_info().unwrap().opening);
    assert!(
        s.transport()
            .unwrap()
            .faults
            .iter()
            .any(|f| f == DENIED_MESSAGE),
        "{:?}",
        s.transport().unwrap().faults
    );
    assert_eq!(s.debug_input_open(), Some(false));
    let r = s.record(seq, spec());
    assert!(
        matches!(&r, Err(VizError::Model { message }) if message.starts_with("VizDAW may not use audio input")),
        "{r:?}"
    );
    assert!(!s.record_state().unwrap().recording);
    s.close().unwrap();
}

/// Ruling 15 (5c carry T11 M3; Review Focus 3): an input that failed to
/// open is not retried by Play, but is by arming again or choosing the
/// input again.
#[test]
fn choosing_the_input_again_retries_a_failed_open() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    let lp = audio_lane(&s, seq);
    take_engine(&s, seq);
    debug_fail_input_opens(1);
    arm(&s, seq, lp, true);
    let failed = |s: &Session| {
        s.transport()
            .unwrap()
            .faults
            .iter()
            .any(|f| f.starts_with("audio input could not open"))
    };
    assert!(failed(&s));
    assert_eq!(s.debug_input_open(), Some(false));
    // Play does not try again (each try restarts the device).
    take_engine(&s, seq);
    assert!(failed(&s), "said again after Play cleared the faults");
    assert_eq!(s.debug_input_open(), Some(false));
    // Choosing the input again does.
    audio_lane(&s, seq);
    wait_until("the input to open", || s.debug_input_open() == Some(true));
    // And so does arming, after another failure.
    arm(&s, seq, lp, false);
    debug_fail_input_opens(1);
    arm(&s, seq, lp, true);
    assert_eq!(s.debug_input_open(), Some(false));
    arm(&s, seq, lp, true);
    wait_until("the input to open again", || {
        s.debug_input_open() == Some(true)
    });
    // The input device picker clears it too.
    arm(&s, seq, lp, false);
    debug_fail_input_opens(1);
    arm(&s, seq, lp, true);
    assert_eq!(s.debug_input_open(), Some(false));
    vizdaw_ffi::host::set_audio_input_device(None);
    s.idle().unwrap();
    take_engine(&s, seq);
    wait_until("the input to open a third time", || {
        s.debug_input_open() == Some(true)
    });
    s.close().unwrap();
}
VIZDAW_EOF
```

In `crates/core-ffi/tests/control.rs`:

Find:
```rust
            buffer_frames: 512,
            round_trip_frames: 2048,
        },
    });
```
Replace:
```rust
            buffer_frames: 512,
            round_trip_frames: 2048,
            opening: false,
        },
    });
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test input_thread 2>&1 | tail -5
```
Expected: FAIL to compile with "unresolved imports `vizdaw_ffi::host::debug_fail_input_opens`, `vizdaw_ffi::host::debug_input_open_delay`, `vizdaw_ffi::host::set_fake_microphone_prompt`".

- [ ] **Step 3: Ask for the microphone off the main thread**

In `crates/platform-macos/src/permission.rs`:

Find:
```rust
/// The message arming an audio lane shows when input is refused (spec
/// §6.1).
```
Replace:
```rust
/// Asks macOS for microphone access when it was never asked (plan 5d-1
/// Ruling 14), and waits up to `limit` for the user's answer to the
/// system prompt; the access then. Returns at once when the access is
/// already known. Blocks for as long as the user takes: never call it on
/// the main thread (the host's input thread does). Tests never call it.
pub fn request_microphone(limit: std::time::Duration) -> MicAccess {
    let now = microphone();
    if now != MicAccess::Undetermined {
        return now;
    }
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
        return now;
    };
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let tx = std::sync::Mutex::new(Some(tx));
    let block = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
        let sender = tx.lock().ok().and_then(|mut t| t.take());
        if let Some(sender) = sender {
            let _ = sender.send(granted.as_bool());
        }
    });
    // SAFETY: `requestAccessForMediaType:completionHandler:` is a class
    // method taking an `AVMediaType` (the framework's own NSString
    // constant) and a block it calls once with a BOOL, on any queue; the
    // block owns its sender and is retained by the framework until then.
    let _: () = unsafe {
        msg_send![cls, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*block]
    };
    match rx.recv_timeout(limit) {
        Ok(true) => MicAccess::Granted,
        Ok(false) => MicAccess::Denied,
        Err(_) => microphone(),
    }
}

/// The message arming an audio lane shows when input is refused (spec
/// §6.1).
```

- [ ] **Step 4: Move the input change onto its own thread**

In `crates/core-ffi/src/host.rs`, apply these in order:

Find:
```rust
/// Makes offline hosts behave as if macOS refused the microphone (test
/// hook; pre-flight M6).
#[cfg(feature = "test-hooks")]
pub fn set_fake_microphone_denied(denied: bool) {
    FAKE_MIC_DENIED.store(denied, Ordering::Relaxed);
}
```
Replace:
```rust
/// Makes offline hosts behave as if macOS refused the microphone (test
/// hook; pre-flight M6).
#[cfg(feature = "test-hooks")]
pub fn set_fake_microphone_denied(denied: bool) {
    FAKE_MIC_DENIED.store(denied, Ordering::Relaxed);
}

/// How long Record and Measure wait for an input that is opening (plan
/// 5d-1 Ruling 13).
pub const INPUT_WAIT: Duration = Duration::from_secs(3);

/// Why Record or Measure did not wait longer.
pub const INPUT_OPENING: &str = "The audio input is still opening; try again in a moment.";

/// How long the input thread waits for the answer to the first
/// microphone prompt (Ruling 14).
#[cfg(target_os = "macos")]
const PROMPT_WAIT: Duration = Duration::from_secs(120);

/// Hands out [`Host::generation`] values.
static GENERATIONS: AtomicU64 = AtomicU64::new(1);

/// Offline hosts open their fake input on the input thread, this long
/// (ms; 0: at once) — test hook, plan 5d-1 Ruling 13.
#[cfg(feature = "test-hooks")]
static INPUT_OPEN_DELAY_MS: AtomicU64 = AtomicU64::new(0);

/// Makes offline hosts open (or close) their fake input on the input
/// thread after `delay` (`None`: at once, as before; test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_input_open_delay(delay: Option<Duration>) {
    let ms = delay.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    INPUT_OPEN_DELAY_MS.store(ms, Ordering::Relaxed);
}

/// The answer the fake user gives the next (first) microphone prompt
/// (`None`: no prompt; test hook, Ruling 14).
#[cfg(feature = "test-hooks")]
static FAKE_PROMPT: Mutex<Option<bool>> = Mutex::new(None);

/// Makes the next offline input open ask the fake user, who answers
/// `answer` (true: allow; false: deny, after which macOS reads denied).
/// `None`: no prompt (test hook, Ruling 14).
#[cfg(feature = "test-hooks")]
pub fn set_fake_microphone_prompt(answer: Option<bool>) {
    *lock(&FAKE_PROMPT) = answer;
}

/// Offline input opens that fail before one works (test hook, Ruling 15).
#[cfg(feature = "test-hooks")]
static FAIL_OPENS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Makes the next `n` offline input opens fail as a busy device would
/// (test hook, Ruling 15).
#[cfg(feature = "test-hooks")]
pub fn debug_fail_input_opens(n: u32) {
    FAIL_OPENS.store(n, Ordering::Relaxed);
}

/// Whether an offline input open fails now (one fewer to fail).
#[cfg(feature = "test-hooks")]
fn take_fake_failure() -> bool {
    let mut n = FAIL_OPENS.load(Ordering::Relaxed);
    while n > 0 {
        match FAIL_OPENS.compare_exchange(n, n - 1, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(v) => n = v,
        }
    }
    false
}
```

Find:
```rust
enum Output {
    /// Held for its `Drop`, which stops the render thread.
    Offline(#[allow(dead_code)] OfflineDriver),
    #[cfg(target_os = "macos")]
    Hal(platform_macos::hal::CoreAudioBackend),
}
```
Replace:
```rust
enum Output {
    /// Held for its `Drop`, which stops the render thread.
    Offline(#[allow(dead_code)] OfflineDriver),
    /// The device, shared with the input thread while it reopens it
    /// without the host lock (plan 5d-1 Ruling 13).
    #[cfg(target_os = "macos")]
    Hal(Arc<Mutex<platform_macos::hal::CoreAudioBackend>>),
}

/// The thread changing a host's input (plan 5d-1 Ruling 13).
struct InputJob {
    /// Set when the host goes: the thread then hands nothing back.
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// What an input thread does without the host lock.
enum Opener {
    /// Reopens a live device with `cfg`, asking for the microphone first
    /// when macOS never asked (Ruling 14).
    #[cfg(target_os = "macos")]
    Hal {
        backend: Arc<Mutex<platform_macos::hal::CoreAudioBackend>>,
        cfg: core_engine::backend::AudioConfig,
    },
    /// The offline device under the test hooks: waits `delay`, the fake
    /// user answering `prompt` to the first microphone prompt.
    #[cfg(feature = "test-hooks")]
    Fake {
        delay: Duration,
        prompt: Option<bool>,
    },
}

/// What an input thread found.
enum Opened {
    /// The device reopened: what it offers, the workers' environment, and
    /// why its input failed to open (it then reopened without input).
    #[cfg(target_os = "macos")]
    Device {
        info: core_engine::backend::StreamInfo,
        env: core_engine::pool::RtEnv,
        failed: Option<String>,
    },
    /// The device could not reopen at all.
    #[cfg(target_os = "macos")]
    Broken(String),
    /// macOS (or the fake user) refused the microphone.
    Denied,
    /// The fake input changed.
    #[cfg(feature = "test-hooks")]
    Fake,
}

/// Does `opener`'s work for an input that should end `want` (open or
/// closed). Runs on the input thread, with no lock but the device's.
fn open_input(opener: Opener, want: bool) -> Opened {
    match opener {
        #[cfg(target_os = "macos")]
        Opener::Hal { backend, cfg } => {
            use platform_macos::permission::{MicAccess, microphone, request_microphone};
            // The first open asks, here, off the caller's thread; a denial
            // is known before anything records (Ruling 14). Test builds
            // never ask (they never open a live device either).
            if want && cfg!(not(feature = "test-hooks")) && microphone() == MicAccess::Undetermined
            {
                let _ = request_microphone(PROMPT_WAIT);
            }
            if want && microphone() == MicAccess::Denied {
                return Opened::Denied;
            }
            let mut b = lock(&backend);
            match reopen_backend(&mut b, &cfg) {
                Ok((info, env, failed)) => Opened::Device { info, env, failed },
                Err(e) => Opened::Broken(e.to_string()),
            }
        }
        #[cfg(feature = "test-hooks")]
        Opener::Fake { delay, prompt } => {
            std::thread::sleep(delay);
            if want && prompt == Some(false) {
                Opened::Denied
            } else {
                Opened::Fake
            }
        }
    }
}

/// Stops `b`, reopens it with `cfg` (without input when the input fails
/// to open: its reason is returned) and restarts it with the same
/// renderer; what it offers and the workers' environment.
#[cfg(target_os = "macos")]
fn reopen_backend(
    b: &mut platform_macos::hal::CoreAudioBackend,
    cfg: &core_engine::backend::AudioConfig,
) -> Result<
    (
        core_engine::backend::StreamInfo,
        core_engine::pool::RtEnv,
        Option<String>,
    ),
    VizError,
> {
    let mut r = b.stop()?;
    let (info, failed) = match b.open(cfg) {
        Ok(i) => (i, None),
        Err(e) if cfg.input_enabled => {
            let f = format!("{}: {e}", crate::record::INPUT_FAILED_PREFIX);
            let out = core_engine::backend::AudioConfig {
                input_enabled: false,
                ..cfg.clone()
            };
            (b.open(&out)?, Some(f))
        }
        Err(e) => return Err(e.into()),
    };
    if info.buffer_frames > r.config().max_frames {
        r.prepare(info.buffer_frames);
    }
    b.start(r)?;
    let env = rt_env(b, info.buffer_frames, info.sample_rate);
    Ok((info, env, failed))
}

/// Gives what an input thread found back to host `generation`, under
/// the host lock (tried, never waited for: the host being dropped may
/// hold it while it joins this thread, after setting `cancel`).
fn hand_back(generation: u64, cancel: &AtomicBool, want: bool, done: Opened) {
    loop {
        if cancel.load(Ordering::Acquire) {
            return;
        }
        let mut slot = match HOST.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
        };
        if let Some(h) = slot.as_mut().filter(|h| h.generation == generation) {
            h.finish_job(want, done);
        }
        return;
    }
}
```

Find:
```rust
    /// The return-latency measurement installed, if any (plan 5c-2
    /// Ruling 9): Play, Record and another Measure are `Busy` until it
    /// ends, and the input stays as it is (pre-flight M1). Its status
    /// tells whose it is (Task 9 review).
    measure: Option<Arc<MeasureStatus>>,
}
```
Replace:
```rust
    /// The return-latency measurement installed, if any (plan 5c-2
    /// Ruling 9): Play, Record and another Measure are `Busy` until it
    /// ends, and the input stays as it is (pre-flight M1). Its status
    /// tells whose it is (Task 9 review).
    measure: Option<Arc<MeasureStatus>>,
    /// This host among all hosts made: an input thread hands back only to
    /// the host that started it.
    generation: u64,
    /// The input changing on its own thread (plan 5d-1 Ruling 13): input
    /// changes asked meanwhile wait for it in `pending_input`.
    job: Option<InputJob>,
    /// A device reopen (a new I/O buffer size) waits for the input thread.
    pending_reopen: bool,
}

impl Drop for Host {
    fn drop(&mut self) {
        // The input thread hands nothing back to a host that is going;
        // it finishes with the device before the output drops it.
        if let Some(mut j) = self.job.take() {
            j.cancel.store(true, Ordering::Release);
            if let Some(t) = j.thread.take() {
                let _ = t.join();
            }
        }
    }
}
```

Find:
```rust
            pending_input: None,
            input_failed: None,
            measure: None,
        }
    }
```
Replace:
```rust
            pending_input: None,
            input_failed: None,
            measure: None,
            generation: GENERATIONS.fetch_add(1, Ordering::Relaxed),
            job: None,
            pending_reopen: false,
        }
    }
```

Find:
```rust
        let mut h = Host::new(
            engine,
            Output::Hal(backend),
            &info.device_name,
```
Replace:
```rust
        let mut h = Host::new(
            engine,
            Output::Hal(Arc::new(Mutex::new(backend))),
            &info.device_name,
```

Find:
```rust
    /// Opens or closes a live output's input to match `want` (spec §6.1),
    /// restarting the device with the same renderer. Offline outputs keep
    /// their fake input.
    /// While a record tap or a measurement is in, the change waits for it
    /// to go ([`Host::end_record_hold`], [`Host::end_measuring`]).
    fn ensure_input(&mut self, want: bool) -> Result<(), VizError> {
        if self.record_hold || self.measure.is_some() {
```
Replace:
```rust
    /// Opens or closes a live output's input to match `want` (spec §6.1),
    /// restarting the device with the same renderer on the input thread
    /// (plan 5d-1 Ruling 13). Offline outputs keep their fake input.
    /// While a record tap, a measurement or the input thread is at work,
    /// the change waits for it ([`Host::end_record_hold`],
    /// [`Host::end_measuring`], [`Host::finish_job`]).
    fn ensure_input(&mut self, want: bool) -> Result<(), VizError> {
        if self.record_hold || self.measure.is_some() || self.job.is_some() {
```

Find:
```rust
    fn apply_input(&mut self, want: bool) -> Result<(), VizError> {
        match &mut self.output {
            Output::Offline(_) => {
                #[cfg(all(feature = "test-hooks", target_os = "macos"))]
                if want && FAKE_MIC_DENIED.load(Ordering::Relaxed) {
                    admit_input(
                        platform_macos::permission::MicAccess::Denied,
                        &mut self.faults,
                    );
                }
                // The fake input stays; what would be open is tracked.
                self.input_open = want;
                Ok(())
            }
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                let want = want && live_input_allowed(&mut self.faults);
                if !want {
                    self.input_failed = None;
                } else if let Some(f) = &self.input_failed {
                    // Reported again after Play cleared the faults.
                    if !self.faults.contains(f) {
                        self.faults.push(f.clone());
                    }
                    return Ok(());
                }
                if want == self.input_open {
                    return Ok(());
                }
                let mut r = b.stop()?;
                let info = match b.open(&live_audio_config(self.project_rate, want)) {
                    Ok(i) => i,
                    Err(e) if want => {
                        let f = format!("{}: {e}", crate::record::INPUT_FAILED_PREFIX);
                        self.faults.push(f.clone());
                        self.input_failed = Some(f);
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
                self.inputs = InputInfo::of(&info);
                Ok(())
            }
        }
    }
```
Replace:
```rust
    fn apply_input(&mut self, want: bool) -> Result<(), VizError> {
        #[cfg(target_os = "macos")]
        if let Output::Hal(b) = &self.output {
            let backend = Arc::clone(b);
            let want = want && live_input_allowed(&mut self.faults);
            if self.blocked_by_failure(want) || want == self.input_open {
                return Ok(());
            }
            let cfg = live_audio_config(self.project_rate, want);
            self.start_job(Opener::Hal { backend, cfg }, want);
            return Ok(());
        }
        #[cfg(all(feature = "test-hooks", target_os = "macos"))]
        if want && FAKE_MIC_DENIED.load(Ordering::Relaxed) {
            admit_input(
                platform_macos::permission::MicAccess::Denied,
                &mut self.faults,
            );
        }
        if self.blocked_by_failure(want) {
            return Ok(());
        }
        #[cfg(feature = "test-hooks")]
        if want != self.input_open {
            if want && take_fake_failure() {
                let f = format!(
                    "{}: the fake device is busy",
                    crate::record::INPUT_FAILED_PREFIX
                );
                self.faults.push(f.clone());
                self.input_failed = Some(f);
                return Ok(());
            }
            let delay = Duration::from_millis(INPUT_OPEN_DELAY_MS.load(Ordering::Relaxed));
            let prompt = if want { lock(&FAKE_PROMPT).take() } else { None };
            if !delay.is_zero() || prompt.is_some() {
                self.start_job(Opener::Fake { delay, prompt }, want);
                return Ok(());
            }
        }
        // The fake input stays; what would be open is tracked.
        self.input_open = want;
        Ok(())
    }

    /// Whether an input that failed to open stops this change (Ruling 15:
    /// it is not tried again until the input or the arm of a lane changes,
    /// or another input device is chosen): it is reported again after Play
    /// cleared the faults. Closing always clears the failure.
    fn blocked_by_failure(&mut self, want: bool) -> bool {
        if !want {
            self.input_failed = None;
            return false;
        }
        match &self.input_failed {
            Some(f) => {
                if !self.faults.contains(f) {
                    self.faults.push(f.clone());
                }
                true
            }
            None => false,
        }
    }

    /// Starts the input thread on `opener` for an input that should end
    /// `want` (Ruling 13).
    fn start_job(&mut self, opener: Opener, want: bool) {
        let cancel = Arc::new(AtomicBool::new(false));
        let (generation, flag) = (self.generation, Arc::clone(&cancel));
        let spawned = std::thread::Builder::new()
            .name("vizdaw-input".into())
            .spawn(move || {
                let done = open_input(opener, want);
                hand_back(generation, &flag, want, done);
            });
        match spawned {
            Ok(t) => {
                self.job = Some(InputJob {
                    cancel,
                    thread: Some(t),
                })
            }
            Err(e) => {
                let f = format!("{}: no input thread ({e})", crate::record::INPUT_FAILED_PREFIX);
                self.faults.push(f);
            }
        }
    }

    /// Takes what the input thread found (under the host lock), then
    /// applies what was asked meanwhile.
    fn finish_job(&mut self, want: bool, done: Opened) {
        self.job = None;
        let was = self.input_open;
        match done {
            #[cfg(target_os = "macos")]
            Opened::Device { info, env, failed } => {
                self.engine.set_rt_env(env);
                self.input_open = info.input_channels > 0;
                self.inputs = InputInfo::of(&info);
                if let Some(f) = failed {
                    if !self.faults.contains(&f) {
                        self.faults.push(f.clone());
                    }
                    self.input_failed = Some(f);
                }
            }
            #[cfg(target_os = "macos")]
            Opened::Broken(why) => {
                let f = format!("the audio device could not reopen: {why}");
                if !self.faults.contains(&f) {
                    self.faults.push(f);
                }
            }
            Opened::Denied => {
                #[cfg(target_os = "macos")]
                admit_input(
                    platform_macos::permission::MicAccess::Denied,
                    &mut self.faults,
                );
                // The fake user said no: macOS reads denied from now on.
                #[cfg(feature = "test-hooks")]
                FAKE_MIC_DENIED.store(true, Ordering::Relaxed);
                self.input_open = false;
            }
            #[cfg(feature = "test-hooks")]
            Opened::Fake => self.input_open = want,
        }
        let _ = want;
        if self.input_open != was {
            self.input_changes += 1;
        }
        if std::mem::take(&mut self.pending_reopen) {
            let _ = self.reopen();
        }
        if let Some(w) = self.pending_input.take() {
            let _ = self.ensure_input(w);
        }
    }
```

Find:
```rust
        match &mut self.output {
            Output::Offline(_) => {}
            #[cfg(target_os = "macos")]
            Output::Hal(b) => events.extend(b.take_events()),
        }
        events
    }
```
Replace:
```rust
        match &mut self.output {
            Output::Offline(_) => {}
            // Not while the input thread reopens it: the next poll reads
            // them (plan 5d-1 Ruling 13).
            #[cfg(target_os = "macos")]
            Output::Hal(b) => match b.try_lock() {
                Ok(mut b) => events.extend(b.take_events()),
                Err(std::sync::TryLockError::Poisoned(p)) => {
                    events.extend(p.into_inner().take_events())
                }
                Err(std::sync::TryLockError::WouldBlock) => {}
            },
        }
        events
    }
```

Find:
```rust
    fn reopen(&mut self) -> Result<(), VizError> {
        match &mut self.output {
            Output::Offline(_) => Ok(()),
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                let mut r = b.stop()?;
                let info = b.open(&live_audio_config(self.project_rate, self.input_open))?;
                if info.buffer_frames > r.config().max_frames {
                    r.prepare(info.buffer_frames);
                }
                b.start(r)?;
                self.engine
                    .set_rt_env(rt_env(b, info.buffer_frames, info.sample_rate));
                self.input_open = info.input_channels > 0;
                self.inputs = InputInfo::of(&info);
                Ok(())
            }
        }
    }
```
Replace:
```rust
    fn reopen(&mut self) -> Result<(), VizError> {
        if self.job.is_some() {
            self.pending_reopen = true;
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        if let Output::Hal(b) = &self.output {
            let backend = Arc::clone(b);
            let cfg = live_audio_config(self.project_rate, self.input_open);
            let open = self.input_open;
            self.start_job(Opener::Hal { backend, cfg }, open);
        }
        Ok(())
    }
```

Find:
```rust
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                let mut r = b.stop()?;
                if frames > r.config().max_frames {
                    r.prepare(frames);
                }
                b.start(r)?;
                // The workers follow the new block period.
                self.engine.set_rt_env(rt_env(b, frames, self.sample_rate));
                Ok(())
            }
```
Replace:
```rust
            // The input thread's reopen takes the new size anyway.
            #[cfg(target_os = "macos")]
            Output::Hal(_) if self.job.is_some() => Ok(()),
            #[cfg(target_os = "macos")]
            Output::Hal(b) => {
                let mut b = lock(b);
                let mut r = b.stop()?;
                if frames > r.config().max_frames {
                    r.prepare(frames);
                }
                b.start(r)?;
                // The workers follow the new block period.
                self.engine.set_rt_env(rt_env(&b, frames, self.sample_rate));
                Ok(())
            }
```

In `crates/core-ffi/src/host.rs`:

Find:
```rust
/// Chooses the audio input device by name (`None`: "Same as output").
/// Takes effect when the next host opens its input.
#[uniffi::export]
pub fn set_audio_input_device(name: Option<String>) {
    *lock(&INPUT_DEVICE) = name.filter(|n| !n.is_empty());
}
```
Replace:
```rust
/// Chooses the audio input device by name (`None`: "Same as output").
/// Takes effect when the next host opens its input; a device that failed
/// to open is tried again then (plan 5d-1 Ruling 15).
#[uniffi::export]
pub fn set_audio_input_device(name: Option<String>) {
    *lock(&INPUT_DEVICE) = name.filter(|n| !n.is_empty());
    clear_input_failure();
}

/// Lets the next input open try the device again after it failed (plan
/// 5d-1 Ruling 15: the input picker and the arm button).
pub fn clear_input_failure() {
    if let Some(h) = lock(&HOST).as_mut() {
        h.input_failed = None;
    }
}

/// Whether `owner`'s host is opening or closing its input on the input
/// thread (plan 5d-1 Ruling 13).
pub fn input_opening(owner: u64) -> bool {
    lock(&HOST)
        .as_ref()
        .is_some_and(|h| h.owner == owner && h.job.is_some())
}

/// Waits up to `limit`, with no lock held between looks, until no input
/// thread is at work; false when one still is.
pub fn wait_input(limit: Duration) -> bool {
    let until = Instant::now() + limit;
    loop {
        if !lock(&HOST).as_ref().is_some_and(|h| h.job.is_some()) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
```

Find:
```rust
    crate::synth::external_dest(doc, dest)?;
    let (mut slot, _) = claim_host(owner, doc, plugins, true)?;
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    host.faults.clear();
    host.ensure_input(true)?;
    match install_measure(host, owner, doc, seq, dest, lane, channels) {
```
Replace:
```rust
    crate::synth::external_dest(doc, dest)?;
    let generation = {
        let (mut slot, _) = claim_host(owner, doc, plugins, true)?;
        let host = slot.as_mut().ok_or_else(|| VizError::Internal {
            message: "engine host missing".into(),
        })?;
        host.faults.clear();
        host.ensure_input(true)?;
        host.generation
    };
    // The input opens on its own thread, without the host lock (plan 5d-1
    // Ruling 13); then nothing may have taken the engine meanwhile.
    let opened = wait_input(INPUT_WAIT);
    let mut slot = lock(&HOST);
    let host = slot
        .as_mut()
        .filter(|h| h.generation == generation)
        .ok_or_else(|| VizError::Busy {
            message: "The audio engine changed; measure again.".into(),
        })?;
    if host.measure.is_some() {
        return Err(measuring_busy());
    }
    if host.busy() {
        return Err(VizError::Busy {
            message: STOP_FIRST.into(),
        });
    }
    if !opened {
        return Err(VizError::Busy {
            message: INPUT_OPENING.into(),
        });
    }
    match install_measure(host, owner, doc, seq, dest, lane, channels) {
```

In `crates/core-ffi/src/live.rs`:

Find:
```rust
    /// What a monitored input takes from input to output: both latencies
    /// and a buffer each way.
    #[uniffi(default)]
    pub round_trip_frames: u32,
}
```
Replace:
```rust
    /// What a monitored input takes from input to output: both latencies
    /// and a buffer each way.
    #[uniffi(default)]
    pub round_trip_frames: u32,
    /// The input is opening or closing on its own thread (plan 5d-1
    /// Ruling 13).
    #[uniffi(default)]
    pub opening: bool,
}
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
                    output_latency_frames: i.output_latency_frames,
                    buffer_frames: i.buffer_frames,
                },
```
Replace:
```rust
                    output_latency_frames: i.output_latency_frames,
                    buffer_frames: i.buffer_frames,
                    opening: host::input_opening(self.id),
                },
```

Find:
```rust
                    buffer_frames: 0,
                    round_trip_frames: 0,
                },
```
Replace:
```rust
                    buffer_frames: 0,
                    round_trip_frames: 0,
                    opening: false,
                },
```

Find:
```rust
        pending.count_in(count_in);
        let fail = |e: VizError| {
            let _ = host::stop_recording(self.id);
            Err(e)
        };
```
Replace:
```rust
        pending.count_in(count_in);
        let fail = |e: VizError| {
            let _ = host::stop_recording(self.id);
            Err(e)
        };
        // The input opens on its own thread (plan 5d-1 Ruling 13): a pass
        // that records audio waits for it, a bounded while.
        if !audio.is_empty() && !host::wait_input(host::INPUT_WAIT) {
            return fail(VizError::Busy {
                message: host::INPUT_OPENING.into(),
            });
        }
```

Find:
```rust
            let c = crate::edit::to_command(&cmd)?;
            // Add Lane gives the lane the default monitor mode (plan 5d-1
            // Ruling 11).
```
Replace:
```rust
            let c = crate::edit::to_command(&cmd)?;
            if crate::edit::retries_input(&cmd) {
                host::clear_input_failure();
            }
            // Add Lane gives the lane the default monitor mode (plan 5d-1
            // Ruling 11).
```

Find:
```rust
                    crate::edit::EditCommand::Settings { edit } => {
                        crate::modify::check_settings_edit(lock(&self.inner).doc(), edit)?;
                    }
                    _ => {}
                }
                core.push(crate::edit::to_command(cmd)?);
```
Replace:
```rust
                    crate::edit::EditCommand::Settings { edit } => {
                        crate::modify::check_settings_edit(lock(&self.inner).doc(), edit)?;
                    }
                    _ => {}
                }
                if crate::edit::retries_input(cmd) {
                    host::clear_input_failure();
                }
                core.push(crate::edit::to_command(cmd)?);
```

In `crates/core-ffi/src/edit.rs`:

Find:
```rust
/// What a lane records (mirrors `core_model::lane::LaneInput`).
```
Replace:
```rust
/// Whether `cmd` changes a lane's arm or input: the input that failed to
/// open is tried again (plan 5d-1 Ruling 15).
pub fn retries_input(cmd: &EditCommand) -> bool {
    matches!(
        cmd,
        EditCommand::Lane {
            edit: LaneEdit::SetInput { .. } | LaneEdit::SetFlags { arm: Some(_), .. }
        }
    )
}

/// What a lane records (mirrors `core_model::lane::LaneInput`).
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-ffi --test input_thread 2>&1 | grep -E "^test |test result"
```
Expected: `an_input_change_runs_off_the_calling_thread`, `record_waits_for_an_input_that_is_opening`, `a_denied_first_prompt_is_reported_before_recording` and `choosing_the_input_again_retries_a_failed_open` pass.

- [ ] **Step 6: Check every crate this touches and the Swift side**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test -p core-ffi --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "core-ffi done"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: clean (`cargo clippy -p core-ffi --lib` compiles the live path, `request_microphone` and the device mutex); every core-ffi test passes, the 5c and 5c-2 recording and Measure tests among them (offline hosts change their input at once); Swift passes.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/platform-macos/src/permission.rs crates/core-ffi/src/host.rs crates/core-ffi/src/live.rs crates/core-ffi/src/edit.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/input_thread.rs crates/core-ffi/tests/control.rs
git commit -m "host: the input opens on its own thread, asks for the microphone first and retries after a failure (5d-1 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -9
```

### Task 8: Arming claims an idle engine; MIDI sources reconnect on arm

**Files:**
- Modify: `crates/core-ffi/src/host.rs` (`wants_live`, `claim_idle`, `listen_to_midi_sources_soon`, test hook `debug_pretend_live_midi`)
- Modify: `crates/core-ffi/src/edit.rs` (`claims_engine`)
- Modify: `crates/core-ffi/src/session.rs` (`claim_for_input`; `edit`, `edit_all` and `track_synth` call it)
- Test: `crates/core-ffi/tests/engine_claim.rs` (new)

**Interfaces:**
- Consumes: Task 7's `ensure_input` (the claim opens input on the input thread), `retries_input`.
- Produces: `vizdaw_ffi::host::{wants_live(doc, seq) -> bool, claim_idle(owner, doc, seq, at: Tick, looped, plugins) -> Result<bool, VizError>, listen_to_midi_sources_soon()}`; test hook `debug_pretend_live_midi(bool)`; `vizdaw_ffi::edit::claims_engine(&EditCommand) -> Option<u64>`. No FFI change: plan 5d-2's arm button and input picker get this through `Session.edit`.

Spec §6.6 (Auto: "the input is heard when the lane is armed and … the transport is stopped"; MIDI thru); plan Rulings 16–17 (5c final review I2, T12 M5). A session that does not play claims the engine for the edited sequence when no session plays, records or measures (`claim_host` with `idle_only`, whose `Busy` the claim swallows), loads it stopped at the session's position and opens the input its lanes want. Every arm, monitor or input edit then reconnects the MIDI sources on a thread of its own (a live host only; the hook lets tests count the tries).

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/engine_claim.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/engine_claim.rs <<'VIZDAW_EOF'
//! Arming claims an idle engine (plan 5d-1 Rulings 16–17; 5c carries I2
//! and T12 M5): with the transport stopped and no Play yet, an armed or
//! monitored lane is heard and MIDI thru plays; a playing, recording or
//! measuring engine is never taken; a MIDI lane's arm reconnects the MIDI
//! sources. Offline only: no device is opened and no microphone asked.

#![recursion_limit = "256"]

mod common;

use core_engine::offline::FakeInput;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{
    debug_pretend_live_midi, drain_offline_log, force_offline, holder, midi_source_connects,
    reset_host, set_fake_input,
};
use vizdaw_ffi::{EditCommand, LaneEdit, LaneInputRow, LaneRow, MonitorModeRow, Session, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    set_fake_input(Some(FakeInput::Signals));
    debug_pretend_live_midi(false);
    vizdaw_ffi::host::set_midi_connect_failure(None);
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

fn arm(s: &Session, seq: u64, lane: u64, on: bool) {
    edit(
        s,
        LaneEdit::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: Some(on),
        },
    )
    .unwrap();
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The demo's "Chords" listens to MIDI source "Keys"; "Loop" (demo audio)
/// records fake input 1.
fn inputs(s: &Session, seq: u64, audio: bool) -> u64 {
    let (name, input) = if audio {
        (
            "Loop",
            LaneInputRow::Audio {
                first: 0,
                stereo: false,
                return_of: None,
            },
        )
    } else {
        (
            "Chords",
            LaneInputRow::Midi {
                port: Some("Keys".into()),
                channel: None,
            },
        )
    };
    let id = lane(s, seq, name).id;
    edit(
        s,
        LaneEdit::SetInput {
            seq,
            lane: id,
            input: Some(input),
        },
    )
    .unwrap();
    id
}

/// Ruling 16 (Review Focus 2): before any Play, arming an audio lane
/// claims the engine and opens its input (monitored Auto, armed, stopped:
/// heard); arming a MIDI lane makes thru play.
#[test]
fn arming_claims_an_idle_engine_and_hears_the_input() {
    let _g = serial();
    let s = Session::new_demo_audio().unwrap();
    let seq = song(&s);
    assert_eq!(holder(), None);
    let lp = inputs(&s, seq, true);
    assert_eq!(holder(), None, "an input alone wants nothing");
    arm(&s, seq, lp, true);
    wait_until("the input to open", || s.debug_input_open() == Some(true));
    let routes = s.debug_engine_inputs().unwrap();
    assert_eq!(routes.monitor.len(), 1, "monitored Auto, armed");
    assert!(!s.transport().unwrap().playing, "nothing played");
    assert_eq!(s.input_info().unwrap().channels, 3);
    s.close().unwrap();
    // MIDI thru with no Play.
    reset_host();
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = inputs(&s, seq, false);
    arm(&s, seq, chords, true);
    std::thread::sleep(Duration::from_millis(50));
    drain_offline_log();
    assert!(s.inject_midi("Keys".into(), vec![0x90, 70, 99]).unwrap());
    let mut log = Vec::new();
    wait_until("thru", || {
        log.extend(drain_offline_log());
        log.iter()
            .any(|(_, b): &(u64, Vec<u8>)| b == &vec![0x90, 70, 99])
    });
    s.close().unwrap();
}

/// Ruling 16 (Review Focus 2): a playing engine stays with its window;
/// once it stands idle, arming in the other window takes it. A monitor
/// edit claims too.
#[test]
fn arming_never_takes_a_busy_engine() {
    let _g = serial();
    let a = Session::new_demo().unwrap();
    let b = Session::new_demo_audio().unwrap();
    let (sa, sb) = (song(&a), song(&b));
    a.play(sa).unwrap();
    wait_until("a plays", || a.transport().unwrap().playing);
    let owner = holder();
    let lp = inputs(&b, sb, true);
    arm(&b, sb, lp, true);
    assert_eq!(holder(), owner, "a playing engine is not taken");
    assert!(a.transport().unwrap().playing, "a plays on");
    assert!(b.debug_engine_inputs().is_none());
    a.stop().unwrap();
    wait_until("a stops", || !a.transport().unwrap().playing);
    edit(
        &b,
        LaneEdit::SetMonitor {
            seq: sb,
            lane: lp,
            monitor: MonitorModeRow::Input,
        },
    )
    .unwrap();
    wait_until("b's input to open", || b.debug_input_open() == Some(true));
    assert_ne!(holder(), owner, "an idle engine is taken");
    a.close().unwrap();
    b.close().unwrap();
}

/// Ruling 17 (5c carry T12 M5; Review Focus 2): arming a MIDI lane
/// reconnects the MIDI sources, so a keyboard plugged in after launch is
/// heard (offline hosts connect nothing real; the hook counts the tries).
#[test]
fn arming_a_midi_lane_listens_to_new_sources() {
    let _g = serial();
    debug_pretend_live_midi(true);
    let s = Session::new_demo().unwrap();
    let seq = song(&s);
    let chords = inputs(&s, seq, false);
    let before = midi_source_connects();
    arm(&s, seq, chords, true);
    wait_until("a connect", || midi_source_connects() > before);
    let once = midi_source_connects();
    arm(&s, seq, chords, false);
    arm(&s, seq, chords, true);
    wait_until("another connect", || midi_source_connects() > once);
    debug_pretend_live_midi(false);
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test engine_claim 2>&1 | tail -5
```
Expected: FAIL to compile with "unresolved import `vizdaw_ffi::host::debug_pretend_live_midi`".

- [ ] **Step 3: Claim the engine on an arm**

In `crates/core-ffi/src/host.rs`:

Find:
```rust
pub fn listen_to_midi_sources() {
    let live = lock(&HOST).as_ref().is_some_and(Host::is_live);
    if live {
        connect_and_report();
    }
}
```
Replace:
```rust
pub fn listen_to_midi_sources() {
    let live = lock(&HOST).as_ref().is_some_and(Host::is_live);
    #[cfg(feature = "test-hooks")]
    let live = live || PRETEND_LIVE_MIDI.load(Ordering::Relaxed);
    if live {
        connect_and_report();
    }
}

/// [`listen_to_midi_sources`] on a thread of its own: an arm never waits
/// for a cold CoreMIDI server (plan 5d-1 Ruling 17).
pub fn listen_to_midi_sources_soon() {
    let _ = std::thread::Builder::new()
        .name("vizdaw-midi-connect".into())
        .spawn(listen_to_midi_sources);
}

/// Offline hosts connect the MIDI sources as live ones do (test hook:
/// counts connects; test builds connect nothing real).
#[cfg(feature = "test-hooks")]
static PRETEND_LIVE_MIDI: AtomicBool = AtomicBool::new(false);

/// Makes [`listen_to_midi_sources`] connect for offline hosts too (test
/// hook, plan 5d-1 Ruling 17).
#[cfg(feature = "test-hooks")]
pub fn debug_pretend_live_midi(on: bool) {
    PRETEND_LIVE_MIDI.store(on, Ordering::Relaxed);
}
```

Find:
```rust
/// Whether the lanes of `seq` need audio input: an armed lane with an
```
Replace:
```rust
/// Whether the lanes of `seq` want the engine while it stands still (plan
/// 5d-1 Ruling 16): an input to open, a monitored input or MIDI thru.
pub fn wants_live(doc: &Document, seq: SeqId) -> bool {
    if wants_audio_input(doc, seq) {
        return true;
    }
    let spec = input_spec(doc, seq);
    !spec.monitor.is_empty() || spec.midi.iter().any(|m| m.thru || m.armed)
}

/// Whether the lanes of `seq` need audio input: an armed lane with an
```

Find:
```rust
/// `owner`'s view of the transport, or `None` when it does not hold the
/// engine.
```
Replace:
```rust
/// Claims the engine for session `owner`'s sequence `seq` of `doc` while
/// it stands idle (plan 5d-1 Ruling 16, 5c carry I2): no session plays,
/// records or measures. It loads `seq` stopped at `at` and opens the input
/// its lanes want, so armed and monitored lanes are heard and thru plays
/// before the first Play. Nothing when `owner` holds the engine with `seq`
/// loaded already, or when the engine is busy (never taken then). True
/// when it claimed a live host: the caller connects the MIDI sources
/// without the lock ([`listen_to_midi_sources_soon`]).
pub fn claim_idle(
    owner: u64,
    doc: &Document,
    seq: SeqId,
    at: Tick,
    looped: bool,
    plugins: Option<&mut PluginHost>,
) -> Result<bool, VizError> {
    if lock(&HOST)
        .as_ref()
        .is_some_and(|h| h.owner == owner && h.seq == seq)
    {
        return Ok(false);
    }
    let (mut slot, _) = match claim_host(owner, doc, plugins, true) {
        Ok(s) => s,
        Err(VizError::Busy { .. }) => return Ok(false),
        Err(e) => return Err(e),
    };
    let host = slot.as_mut().ok_or_else(|| VizError::Internal {
        message: "engine host missing".into(),
    })?;
    if host.owner != owner {
        // Another session's held position and plugins go with its claim.
        host.record_hold = false;
        host.pending_input = None;
    }
    host.engine.stop()?;
    host.load(owner, doc, seq, looped)?;
    host.engine.locate_tick(at)?;
    host.owner = owner;
    host.seq = seq;
    host.want_play = false;
    host.until = None;
    host.epoch = next_epoch();
    host.ensure_input(wants_audio_input(doc, seq))?;
    Ok(host.is_live())
}

/// `owner`'s view of the transport, or `None` when it does not hold the
/// engine.
```

In `crates/core-ffi/src/edit.rs`:

Find:
```rust
/// Whether `cmd` changes a lane's arm or input: the input that failed to
/// open is tried again (plan 5d-1 Ruling 15).
```
Replace:
```rust
/// The sequence whose engine claim `cmd` asks for (plan 5d-1 Ruling 16):
/// an edit of a lane's arm, monitor or input.
pub fn claims_engine(cmd: &EditCommand) -> Option<u64> {
    match cmd {
        EditCommand::Lane {
            edit:
                LaneEdit::SetInput { seq, .. }
                | LaneEdit::SetMonitor { seq, .. }
                | LaneEdit::SetFlags {
                    seq, arm: Some(_), ..
                },
        } => Some(*seq),
        _ => None,
    }
}

/// Whether `cmd` changes a lane's arm or input: the input that failed to
/// open is tried again (plan 5d-1 Ruling 15).
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
                return self.add_lane_monitored(c, SeqId(*seq), monitor, &filter);
            }
            self.execute_with(c, &filter)
```
Replace:
```rust
                return self.add_lane_monitored(c, SeqId(*seq), monitor, &filter);
            }
            let snapshot = self.execute_with(c, &filter)?;
            if let Some(seq) = crate::edit::claims_engine(&cmd) {
                self.claim_for_input(SeqId(seq));
            }
            Ok(snapshot)
```

Find:
```rust
            let mut inner = lock(&self.inner);
            inner.edit_label = None;
            inner.history.commit()?;
            inner.version += 1;
            self.commit_held(&mut inner);
            self.apply_jobs_after_edit(&mut inner);
            inner.snapshot()
        })
    }
```
Replace:
```rust
            let mut inner = lock(&self.inner);
            inner.edit_label = None;
            inner.history.commit()?;
            inner.version += 1;
            self.commit_held(&mut inner);
            self.apply_jobs_after_edit(&mut inner);
            let snapshot = inner.snapshot()?;
            drop(inner);
            if let Some(seq) = cmds.iter().find_map(crate::edit::claims_engine) {
                self.claim_for_input(SeqId(seq));
            }
            Ok(snapshot)
        })
    }
```

Find:
```rust
            if let Some(audio) = audio {
                if mute_after {
                    inner.synth_mutes.insert((id, audio), LaneId(lane));
                } else {
                    inner.synth_mutes.remove(&(id, audio));
                }
            }
            inner.snapshot()
        })
    }
```
Replace:
```rust
            if let Some(audio) = audio {
                if mute_after {
                    inner.synth_mutes.insert((id, audio), LaneId(lane));
                } else {
                    inner.synth_mutes.remove(&(id, audio));
                }
            }
            let snapshot = inner.snapshot()?;
            drop(inner);
            // Track Synth arms the return lane (plan 5d-1 Ruling 16).
            self.claim_for_input(id);
            Ok(snapshot)
        })
    }
```

Find:
```rust
    /// Why Measure or Track Synth cannot start or take its step now:
```
Replace:
```rust
    /// After an edit of a lane's arm, monitor or input in `seq` (plan 5d-1
    /// Rulings 16–17): claims an idle engine for `seq` when its lanes want
    /// it, so they are heard before the first Play, and reconnects the MIDI
    /// sources off this thread. Never while this session records; a failure
    /// is a fault, never the edit's.
    fn claim_for_input(&self, seq: SeqId) {
        let mut inner = lock(&self.inner);
        if inner.recording.is_some() || inner.finishing {
            return;
        }
        if !host::wants_live(inner.doc(), seq) {
            return;
        }
        let at = match (inner.cursor, inner.cursor_seq) {
            (Some(t), None) => t,
            (Some(t), Some(s)) if s == seq => t,
            _ => Tick::ZERO,
        };
        let looped = inner.looped;
        inner.mark(self.id, true);
        let r = plugins::with(self.id, |p| {
            host::claim_idle(self.id, inner.doc(), seq, at, looped, p)
        })
        .and_then(|r| r);
        match r {
            Ok(_) => {
                if host::holder() == Some(self.id) {
                    inner.cursor = None;
                    inner.cursor_seq = Some(seq);
                }
            }
            Err(e) => inner.reload_fault = Some(format!("the engine could not be claimed: {e}")),
        }
        drop(inner);
        host::listen_to_midi_sources_soon();
    }

    /// Why Measure or Track Synth cannot start or take its step now:
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-ffi --test engine_claim 2>&1 | grep -E "^test |test result"
```
Expected: `arming_claims_an_idle_engine_and_hears_the_input`, `arming_never_takes_a_busy_engine` and `arming_a_midi_lane_listens_to_new_sources` pass.

- [ ] **Step 5: Check the crates**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test --workspace --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: clean; no failing test (the 5c and 5c-2 tests that arm through edits now find the engine claimed, which changes none of their assertions; a known flake may fail once).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/host.rs crates/core-ffi/src/edit.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/engine_claim.rs
git commit -m "host: an arm, monitor or input edit claims an idle engine and reconnects MIDI sources (5d-1 Task 8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -6
```

### Task 9: Recording guards — arming during a pass, Save during a pass, busy edits

**Files:**
- Modify: `crates/core-ffi/src/session.rs` (`edit`, `edit_all`: `edit_open_busy`, `check_records`; `save` and `saves_after_pass`; `apply_pending_pass` saves; `Inner.save_after_pass`; `RECORDING_LANES`)
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/record.rs`, `crates/core-ffi/tests/edit.rs` (5b's open-edit test now expects `Busy`)

**Interfaces:**
- Consumes: Task 7's `vizdaw_ffi::edit::retries_input` (an arm or input change).
- Produces: `Session.saves_after_pass() -> Result<bool, VizError>` (Swift `savesAfterPass()`); `vizdaw_ffi::session::RECORDING_LANES`; `Session.edit`/`edit_all` answer `Busy` "Finish the open edit first." while an edit is open, and `Busy` `RECORDING_LANES` for an arm or input change while recording. Plan 5d-2's File › Save says "the take is saved when recording stops" from `savesAfterPass`, and its `lane.set_input` answers `busy` through these.

Plan Rulings 18–20 (5c final review M5, M6; 5c-2 final review finding 13). Save while recording stays allowed (5c's data-safety tests save mid-pass and while a pass finishes): it saves now and marks the session, and the pass's own apply saves again, so the take is in the saved project once it lands; `Save As` stays `Busy` while recording (5c).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/record.rs <<'VIZDAW_EOF'

fn arm_edit(seq: SeqId, lane: LaneId, on: bool) -> vizdaw_ffi::EditCommand {
    vizdaw_ffi::EditCommand::Lane {
        edit: vizdaw_ffi::LaneEdit::SetFlags {
            seq: seq.0,
            lane: lane.0,
            mute: None,
            solo: None,
            arm: Some(on),
        },
    }
}

/// Plan 5d-1 Ruling 18 (5c carry M6; Review Focus 5): while a pass
/// records, arming, disarming, Move Record Arm and input changes are
/// Busy and change nothing; monitor changes apply; the take is whole.
#[test]
fn arming_changes_are_refused_while_recording() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    let busy = |r: Result<vizdaw_ffi::DocSnapshot, VizError>| {
        matches!(&r, Err(VizError::Busy { message }) if message.contains("Stop recording"))
    };
    assert!(busy(s.edit(arm_edit(d.seq, d.mic, false), None)));
    assert!(busy(s.edit(arm_edit(d.seq, d.keys, true), None)));
    assert!(busy(s.move_record_arm(d.seq.0, false)));
    assert!(busy(s.edit_all(
        "Arm".into(),
        vec![arm_edit(d.seq, d.keys, true), arm_edit(d.seq, d.src, true)],
        None
    )));
    assert!(busy(s.edit(
        vizdaw_ffi::EditCommand::Lane {
            edit: vizdaw_ffi::LaneEdit::SetInput {
                seq: d.seq.0,
                lane: d.mic.0,
                input: None,
            },
        },
        None
    )));
    // Monitoring changes only what is heard.
    s.edit(
        vizdaw_ffi::EditCommand::Lane {
            edit: vizdaw_ffi::LaneEdit::SetMonitor {
                seq: d.seq.0,
                lane: d.mic.0,
                monitor: vizdaw_ffi::MonitorModeRow::Input,
            },
        },
        None,
    )
    .unwrap();
    let lanes = s.lanes(d.seq.0).unwrap();
    assert!(lanes.iter().find(|l| l.id == d.mic.0).unwrap().arm);
    assert!(!lanes.iter().find(|l| l.id == d.keys.0).unwrap().arm);
    wait_until("more audio", || frames(&s) >= 48_000);
    s.stop().unwrap();
    assert_eq!(regions(&s, d.seq.0, d.mic.0).len(), 1);
    // After Stop, arming works again.
    s.edit(arm_edit(d.seq, d.keys, true), None).unwrap();
    s.close().unwrap();
}

/// Plan 5d-1 Ruling 19 (5c carry M5; Review Focus 5): a Save while
/// recording saves the project now and again when the pass lands, so the
/// take is in the saved project after Stop.
#[test]
fn a_save_while_recording_saves_the_take_after_stop() {
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    let pkg = g.dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("audio", || frames(&s) >= 4800);
    assert!(!s.saves_after_pass().unwrap());
    s.save().unwrap();
    assert!(s.saves_after_pass().unwrap(), "the take is saved at Stop");
    let saved = core_model::file::load(&pkg).unwrap();
    assert!(
        saved.media().is_empty(),
        "the take is not in the project yet"
    );
    s.stop().unwrap();
    assert!(!s.saves_after_pass().unwrap());
    let saved = core_model::file::load(&pkg).unwrap();
    assert_eq!(saved.media().len(), 1, "saved with the take");
    assert!(!s.snapshot().unwrap().dirty);
    s.close().unwrap();
}

/// Plan 5d-1 Ruling 20 (5c-2 carry finding 13): an edit while a drag or
/// fader holds an edit open is Busy, for one command and for several.
#[test]
fn an_edit_during_an_open_edit_is_busy() {
    let g = serial(None);
    let d = doc(8, false, false);
    let s = untitled(&g, d.doc.clone());
    s.begin_edit("Drag".into()).unwrap();
    let busy = |r: Result<vizdaw_ffi::DocSnapshot, VizError>| {
        matches!(&r, Err(VizError::Busy { message }) if message == "Finish the open edit first.")
    };
    assert!(busy(s.edit(arm_edit(d.seq, d.keys, true), None)));
    assert!(busy(s.edit_all(
        "Arm".into(),
        vec![arm_edit(d.seq, d.keys, true), arm_edit(d.seq, d.mic, true)],
        None
    )));
    s.end_edit().unwrap();
    s.edit(arm_edit(d.seq, d.keys, true), None).unwrap();
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test record -- arming_changes a_save_while_recording an_edit_during 2>&1 | tail -5
```
Expected: FAIL to compile with "no method named `saves_after_pass` found"; once that exists, `arming_changes_are_refused_while_recording` fails (the arm applies) and `an_edit_during_an_open_edit_is_busy` fails (a `Model` error).

- [ ] **Step 3: Guard what changes during a pass**

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    pub fn edit(
        &self,
        cmd: crate::edit::EditCommand,
        filter: Option<crate::edit::ViewFilterRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            if lock(&self.inner).edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
```
Replace:
```rust
    pub fn edit(
        &self,
        cmd: crate::edit::EditCommand,
        filter: Option<crate::edit::ViewFilterRow>,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            if lock(&self.inner).edit_label.is_some() {
                return Err(edit_open_busy());
            }
            self.check_records(std::slice::from_ref(&cmd))?;
```

Find:
```rust
        ffi_guard(|| {
            if lock(&self.inner).edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            let filter = crate::edit::view_filter(filter.as_ref());
            let mut core = Vec::with_capacity(cmds.len());
```
Replace:
```rust
        ffi_guard(|| {
            if lock(&self.inner).edit_label.is_some() {
                return Err(edit_open_busy());
            }
            self.check_records(&cmds)?;
            let filter = crate::edit::view_filter(filter.as_ref());
            let mut core = Vec::with_capacity(cmds.len());
```

Find:
```rust
fn recording_busy() -> VizError {
    VizError::Busy {
        message: "Stop recording first.".into(),
    }
}
```
Replace:
```rust
fn recording_busy() -> VizError {
    VizError::Busy {
        message: "Stop recording first.".into(),
    }
}

/// Why an edit waits for the edit a drag or a fader opened (plan 5d-1
/// Ruling 20: `Busy`, as Track Synth and Measure answer).
fn edit_open_busy() -> VizError {
    VizError::Busy {
        message: "Finish the open edit first.".into(),
    }
}

/// Why an arm or input change waits for Stop (plan 5d-1 Ruling 18).
pub const RECORDING_LANES: &str = "Stop recording to change which lanes record.";
```

Find:
```rust
    /// Why Measure or Track Synth cannot start or take its step now:
```
Replace:
```rust
    /// `Busy` when one of `cmds` changes which lanes record (an arm or an
    /// input) while this session records, finishes a pass or waits to
    /// apply one (plan 5d-1 Ruling 18, 5c carry M6); monitor changes pass.
    fn check_records(&self, cmds: &[crate::edit::EditCommand]) -> Result<(), VizError> {
        if !cmds.iter().any(crate::edit::retries_input) {
            return Ok(());
        }
        let inner = lock(&self.inner);
        if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
            return Err(VizError::Busy {
                message: RECORDING_LANES.into(),
            });
        }
        Ok(())
    }

    /// Why Measure or Track Synth cannot start or take its step now:
```

Find:
```rust
    pub fn save(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let path = inner.path.clone().ok_or_else(|| VizError::Io {
                message: "the document has no file yet; use Save As".into(),
            })?;
            self.save_to(&mut inner, &path)
        })
    }
```
Replace:
```rust
    pub fn save(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let path = inner.path.clone().ok_or_else(|| VizError::Io {
                message: "the document has no file yet; use Save As".into(),
            })?;
            // The take lands at Stop: it is saved then too (plan 5d-1
            // Ruling 19, 5c carry M5).
            if inner.recording.is_some() || inner.finishing || inner.pending_pass.is_some() {
                inner.save_after_pass = true;
            }
            self.save_to(&mut inner, &path)
        })
    }

    /// Whether a Save made while recording saves again once the pass
    /// lands (plan 5d-1 Ruling 19; the app says so).
    pub fn saves_after_pass(&self) -> Result<bool, VizError> {
        ffi_guard(|| Ok(lock(&self.inner).save_after_pass))
    }
```

Find:
```rust
        let _ = inner.history.end_group(label);
        inner.version += 1;
        inner.news.doc_changed = true;
        inner.news.media_changed = true;
        inner.reload_host(self.id);
        self.mark(inner);
    }
```
Replace:
```rust
        let _ = inner.history.end_group(label);
        inner.version += 1;
        inner.news.doc_changed = true;
        inner.news.media_changed = true;
        inner.reload_host(self.id);
        self.mark(inner);
        // Saved while recording: the take is saved now (Ruling 19).
        if std::mem::take(&mut inner.save_after_pass) {
            if let Some(path) = inner.path.clone() {
                if let Err(e) = self.save_to(inner, &path) {
                    inner
                        .news
                        .problems
                        .push(format!("The take could not be saved: {e}"));
                }
            }
        }
    }
```

Find:
```rust
    /// The audio settings epoch this session's engine last loaded under
    /// (plan 5d-1 Ruling 12).
    settings_seen: u64,
}
```
Replace:
```rust
    /// The audio settings epoch this session's engine last loaded under
    /// (plan 5d-1 Ruling 12).
    settings_seen: u64,
    /// A Save came while recording: the pass is saved when it lands (plan
    /// 5d-1 Ruling 19).
    save_after_pass: bool,
}
```

Find:
```rust
            settings_seen: crate::audio::epoch(),
        }
```
Replace:
```rust
            settings_seen: crate::audio::epoch(),
            save_after_pass: false,
        }
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
    func save() throws {
        guard savedPath != nil else { throw VizError.Io(message: "use Save As") }
        snap.dirty = false
    }
```
Replace:
```swift
    func save() throws {
        guard savedPath != nil else { throw VizError.Io(message: "use Save As") }
        snap.dirty = false
    }

    /// A Save came while recording (plan 5d-1 Ruling 19).
    var savesAfterPassRow = false

    func savesAfterPass() throws -> Bool { savesAfterPassRow }
```

In `crates/core-ffi/tests/edit.rs`:

Find:
```rust
    let during = state(&s);
    assert!(matches!(
        s.edit(move_note(&f, 2 * Q, None), None),
        Err(VizError::Model { .. })
    ));
```
Replace:
```rust
    let during = state(&s);
    // Busy since plan 5d-1 Ruling 20 (it waits for the open edit).
    assert!(matches!(
        s.edit(move_note(&f, 2 * Q, None), None),
        Err(VizError::Busy { .. })
    ));
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-ffi --test record -- arming_changes a_save_while_recording an_edit_during 2>&1 | grep -E "^test |test result"
```
Expected: the three pass.

- [ ] **Step 5: Check the crate and the Swift side**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test -p core-ffi --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "core-ffi done"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: clean; all pass (5c's `saving_while_recording_never_trashes_the_file_being_written` and `a_save_while_the_pass_finishes_keeps_its_file` still pass: Save during a pass is allowed). (`an_edit_during_an_open_edit_is_refused_and_changes_nothing` in `tests/edit.rs` was changed above to expect `Busy`; if another test asserts `Model` for an edit during an open edit, change it the same way and say so in the report.)

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/session.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-ffi/tests/record.rs crates/core-ffi/tests/edit.rs
git commit -m "session: no arming changes during a pass; a Save during it saves the take after Stop; busy edits are Busy (5d-1 Task 9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -6
```

### Task 10: The take's waveform while it is written; peaks in the rows

**Files:**
- Modify: `crates/core-media/src/record.rs` (`LIVE_PEAK_FRAMES`, `LivePeaks`; the writer feeds each lane's; `RecordWriter::peaks`)
- Modify: `crates/core-ffi/src/record.rs` (`RecordLaneRow.peak`; `RecordStateRow.pass_start_tick`/`pass_start_file_frame`/`range_start_tick`; `Plan.tempo`; `Recording::waveform`), `crates/core-ffi/src/session.rs` (`recording_waveform`, the rows, `media` peaks), `crates/core-ffi/src/media.rs` (`MediaRow.peak`), `crates/core-ffi/src/overviews.rs` (`peak_of`)
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-media/tests/record.rs`, `crates/core-ffi/tests/record.rs`, `crates/core-ffi/tests/control.rs`

**Interfaces:**
- Consumes: 5c's writer and `Recording`; 5b's `OverviewRows` (channel-major: index `channel * columns + column`) and overview cache.
- Produces: `core_media::record::{LIVE_PEAK_FRAMES, LivePeaks { new, feed, channels, frames, range, peak }}`, `RecordWriter::peaks(lane) -> Option<Arc<Mutex<LivePeaks>>>`; `Session.recording_waveform(lane: u64, from_file_frame: u64, to_file_frame: u64, columns: u32) -> Result<OverviewRows, VizError>` (Swift `recordingWaveform(lane:fromFileFrame:toFileFrame:columns:)`); `RecordLaneRow.peak: f32`, `RecordStateRow.{pass_start_tick: Option<i64>, pass_start_file_frame: u64, range_start_tick: i64}`, `MediaRow.peak: Option<f32>` (all `#[uniffi(default)]`); `vizdaw_ffi::overviews::peak_of(&Overview) -> f32`. `RecordLaneRow` and `RecordStateRow` lose `Eq` (an `f32`). Plan 5d-2's arrangement draws the growing take from these; Task 12's `state.get` reports the peaks.

Spec §6.2 ("Overviews … are built while the file is written, so the region draws as it grows"); plan Ruling 21 (5c Ruling 23 carry; 5c final review M8). A file frame of a take is its run frame (the writer pads gaps), so the current pass maps file frame `pass_start_file_frame + i` to the sequence frame of the pass's mark plus `i`.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-media/tests/record.rs <<'VIZDAW_EOF'

/// Plan 5d-1 Ruling 21 (spec §6.2: the region draws as it grows): while it
/// writes, the writer keeps each lane's min and max per 256 frames, gaps
/// included as silence, so a view can draw the take before it is placed.
#[test]
fn the_writer_keeps_live_peaks_as_it_writes() {
    use core_media::record::{LIVE_PEAK_FRAMES, LivePeaks};
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
    for (run, n) in [(0u64, 256u32), (256, 256), (1024, 512)] {
        assert!(w.push(run, n, |c, f| value(c, run + f as u64)));
    }
    let mono = writer.peaks(0).unwrap();
    let stereo = writer.peaks(1).unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    while mono.lock().unwrap().frames() < 1536 {
        assert!(Instant::now() < until, "never written");
        std::thread::sleep(Duration::from_millis(2));
    }
    let p = mono.lock().unwrap().clone();
    assert_eq!((p.channels(), p.frames()), (1, 1536));
    // Frames 0..256 of ring channel 2.
    let want = (0..256u64).map(|f| value(2, f));
    let (lo, hi) = want.fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), x| {
        (a.min(x), b.max(x))
    });
    assert_eq!(p.range(0, 0, LIVE_PEAK_FRAMES), Some((lo, hi)));
    // The lost frames 512..1024 are silence.
    assert_eq!(p.range(0, 512, 1024), Some((0.0, 0.0)));
    assert_eq!(p.range(0, 1536, 2000), None, "not written yet");
    assert_eq!(p.range(1, 0, 256), None, "one channel");
    let s = stereo.lock().unwrap().clone();
    assert_eq!(s.channels(), 2);
    assert!((s.peak() - 0.5).abs() < 0.01, "{}", s.peak());
    // A peak being filled counts too.
    let mut q = LivePeaks::new(1);
    q.feed(&[0.25, -0.5, 0.125]);
    assert_eq!((q.frames(), q.range(0, 0, 3)), (3, Some((-0.5, 0.25))));
    assert_eq!(q.peak(), 0.5);
    w.set_end(2048);
    w.close();
    assert_eq!(writer.finish(Duration::from_secs(30)).error, None);
}
VIZDAW_EOF
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/record.rs <<'VIZDAW_EOF'

/// Plan 5d-1 Ruling 21 (spec §6.2: the region draws as it grows; 5c carry
/// M8): while it records, the take's waveform is there to draw, placed at
/// the pass's start; after Stop the media's overview peaks at the fake
/// sine's level (−12 dBFS, 0.2512).
#[test]
fn the_take_draws_while_it_grows_and_its_overview_peaks_at_the_sine() {
    use vizdaw_ffi::OverviewStateRow;
    let g = serial(Some(FakeInput::Signals));
    let d = doc(8, false, true);
    let s = untitled(&g, d.doc.clone());
    s.locate(4 * Q).unwrap();
    s.record(d.seq.0, spec()).unwrap();
    wait_until("a second of audio", || frames(&s) >= 48_000);
    let st = s.record_state().unwrap();
    assert_eq!(
        (st.pass_start_tick, st.pass_start_file_frame, st.range_start_tick),
        (Some(4 * Q), 0, 4 * Q)
    );
    assert!((st.lanes[0].peak - 0.2512).abs() < 0.01, "{}", st.lanes[0].peak);
    let w = s.recording_waveform(d.mic.0, 0, 48_000, 100).unwrap();
    assert_eq!((w.state, w.channels, w.columns), (OverviewStateRow::Ready, 1, 100));
    assert!(w.max.iter().all(|x| (x - 0.2512).abs() < 0.02), "{:?}", w.max);
    assert!(w.min.iter().all(|x| (x + 0.2512).abs() < 0.02), "{:?}", w.min);
    // Not yet written: silent columns.
    let ahead = s.recording_waveform(d.mic.0, 10_000_000, 10_000_100, 4).unwrap();
    assert!(ahead.max.iter().all(|x| *x == 0.0));
    let model = |r: Result<vizdaw_ffi::OverviewRows, VizError>| {
        matches!(r, Err(VizError::Model { .. }))
    };
    assert!(model(s.recording_waveform(d.keys.0, 0, 10, 10)), "not recording audio");
    assert!(model(s.recording_waveform(d.mic.0, 10, 10, 10)), "empty");
    assert!(model(s.recording_waveform(d.mic.0, 0, 10, 0)), "no columns");
    assert!(model(s.recording_waveform(d.mic.0, 0, 10, u32::MAX)), "too many");
    s.stop().unwrap();
    assert!(model(s.recording_waveform(d.mic.0, 0, 10, 10)), "nothing records");
    wait_until("the overview", || s.media().unwrap()[0].peak.is_some());
    let p = s.media().unwrap()[0].peak.unwrap();
    assert!((p - 0.2512).abs() < 0.01, "{p}");
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-media --test record live_peaks 2>&1 | tail -4
cargo test -p core-ffi --test record the_take_draws 2>&1 | tail -4
```
Expected: FAIL to compile with "unresolved imports `core_media::record::LIVE_PEAK_FRAMES`, `core_media::record::LivePeaks`" and "no method named `recording_waveform`".

- [ ] **Step 3: Keep live peaks in the writer**

In `crates/core-media/src/record.rs`:

Find:
```rust
/// One armed audio lane's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterLane {
```
Replace:
```rust
/// Frames per peak of [`LivePeaks`] (the overviews' finest level).
pub const LIVE_PEAK_FRAMES: u64 = 256;

/// A lane's peaks so far, per [`LIVE_PEAK_FRAMES`] frames and channel,
/// kept by the writer as it writes (sub-project 5 spec §6.2: "the region
/// draws as it grows"; plan 5d-1 Ruling 21). In memory only: the
/// overview proper is built once the take is placed.
#[derive(Debug, Clone, PartialEq)]
pub struct LivePeaks {
    channels: usize,
    /// Finished peaks: per peak, per channel, min then max.
    done: Vec<f32>,
    /// The peak being filled (same layout, one peak).
    cur: Vec<f32>,
    /// Frames in `cur`.
    cur_frames: u64,
}

impl LivePeaks {
    /// No frames yet, `channels` (1 or 2) channels.
    pub fn new(channels: usize) -> LivePeaks {
        let channels = channels.clamp(1, 2);
        LivePeaks {
            channels,
            done: Vec::new(),
            cur: Self::empty(channels),
            cur_frames: 0,
        }
    }

    fn empty(channels: usize) -> Vec<f32> {
        (0..channels)
            .flat_map(|_| [f32::INFINITY, f32::NEG_INFINITY])
            .collect()
    }

    /// Takes interleaved frames (non-finite samples count as 0).
    pub fn feed(&mut self, interleaved: &[f32]) {
        let ch = self.channels;
        for frame in interleaved.chunks_exact(ch) {
            for (c, v) in frame.iter().enumerate() {
                let v = if v.is_finite() { *v } else { 0.0 };
                if let Some(x) = self.cur.get_mut(c * 2) {
                    *x = x.min(v);
                }
                if let Some(x) = self.cur.get_mut(c * 2 + 1) {
                    *x = x.max(v);
                }
            }
            self.cur_frames += 1;
            if self.cur_frames == LIVE_PEAK_FRAMES {
                let full = std::mem::replace(&mut self.cur, Self::empty(ch));
                self.done.extend(full);
                self.cur_frames = 0;
            }
        }
    }

    /// Channels.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Frames taken so far.
    pub fn frames(&self) -> u64 {
        let peaks = (self.done.len() / (self.channels * 2)) as u64;
        peaks * LIVE_PEAK_FRAMES + self.cur_frames
    }

    /// The lowest and highest sample of `channel` over frames `[from,
    /// to)`, from the whole peaks that overlap it; `None` outside what was
    /// taken.
    pub fn range(&self, channel: usize, from: u64, to: u64) -> Option<(f32, f32)> {
        let to = to.min(self.frames());
        if from >= to || channel >= self.channels {
            return None;
        }
        let ch = self.channels;
        let (b0, b1) = (from / LIVE_PEAK_FRAMES, to.div_ceil(LIVE_PEAK_FRAMES));
        let finished = (self.done.len() / (ch * 2)) as u64;
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for b in b0..b1 {
            let peak = if b < finished {
                self.done.get(b as usize * ch * 2..(b as usize + 1) * ch * 2)
            } else {
                Some(self.cur.as_slice())
            };
            if let Some(p) = peak {
                if let (Some(a), Some(z)) = (p.get(channel * 2), p.get(channel * 2 + 1)) {
                    lo = lo.min(*a);
                    hi = hi.max(*z);
                }
            }
        }
        (lo <= hi).then_some((lo, hi))
    }

    /// The largest absolute sample so far.
    pub fn peak(&self) -> f32 {
        self.done
            .iter()
            .chain(self.cur.iter())
            .filter(|v| v.is_finite())
            .fold(0f32, |m, v| m.max(v.abs()))
    }
}

/// One armed audio lane's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterLane {
```

Find:
```rust
    clipped: Box<[AtomicU64]>,
    error: Mutex<Option<String>>,
}
```
Replace:
```rust
    clipped: Box<[AtomicU64]>,
    error: Mutex<Option<String>>,
    /// Each lane's live peaks (plan 5d-1 Ruling 21).
    peaks: Box<[Arc<Mutex<LivePeaks>>]>,
}
```

Find:
```rust
struct Lane {
    writer: CafWriter,
    path: PathBuf,
    channels: Vec<usize>,
}
```
Replace:
```rust
struct Lane {
    writer: CafWriter,
    path: PathBuf,
    channels: Vec<usize>,
    peaks: Arc<Mutex<LivePeaks>>,
}
```

Find:
```rust
                    lanes.push(Lane {
                        writer,
                        path: l.path.clone(),
                        channels: l.channels.clone(),
                    });
```
Replace:
```rust
                    lanes.push(Lane {
                        writer,
                        path: l.path.clone(),
                        channels: l.channels.clone(),
                        peaks: Arc::new(Mutex::new(LivePeaks::new(n))),
                    });
```

Find:
```rust
            clipped: (0..lanes.len()).map(|_| AtomicU64::new(0)).collect(),
            error: Mutex::new(None),
        });
```
Replace:
```rust
            clipped: (0..lanes.len()).map(|_| AtomicU64::new(0)).collect(),
            error: Mutex::new(None),
            peaks: lanes.iter().map(|l| Arc::clone(&l.peaks)).collect(),
        });
```

Find:
```rust
    /// Finishes: drains the ring until it is closed (or for `wait` at
    /// most), pads to the end, patches every file's size and syncs it.
```
Replace:
```rust
    /// Lane `lane`'s peaks so far (in [`WriterConfig::lanes`] order).
    pub fn peaks(&self, lane: usize) -> Option<Arc<Mutex<LivePeaks>>> {
        self.shared.peaks.get(lane).map(Arc::clone)
    }

    /// Finishes: drains the ring until it is closed (or for `wait` at
    /// most), pads to the end, patches every file's size and syncs it.
```

Find:
```rust
        for l in lanes.iter_mut() {
            scratch.clear();
            scratch.resize(n * l.channels.len(), 0.0);
            l.writer.write(scratch)?;
        }
```
Replace:
```rust
        for l in lanes.iter_mut() {
            scratch.clear();
            scratch.resize(n * l.channels.len(), 0.0);
            l.writer.write(scratch)?;
            guard(&l.peaks).feed(scratch);
        }
```

Find:
```rust
        for f in skip..frames {
            for c in &l.channels {
                scratch.push(block.get(f * ch + c).copied().unwrap_or(0.0));
            }
        }
        l.writer.write(scratch)?;
```
Replace:
```rust
        for f in skip..frames {
            for c in &l.channels {
                scratch.push(block.get(f * ch + c).copied().unwrap_or(0.0));
            }
        }
        l.writer.write(scratch)?;
        guard(&l.peaks).feed(scratch);
```

In `crates/core-media/src/record.rs`:

Find:
```rust
            lanes.push(Lane {
                writer,
                path,
                channels: vec![usize::from(n == "b.caf")],
            });
```
Replace:
```rust
            lanes.push(Lane {
                writer,
                path,
                channels: vec![usize::from(n == "b.caf")],
                peaks: Arc::new(Mutex::new(LivePeaks::new(1))),
            });
```

Find:
```rust
            clipped: (0..2).map(|_| AtomicU64::new(0)).collect(),
            error: Mutex::new(None),
        };
        let res = run(r, lanes, &shared);
```
Replace:
```rust
            clipped: (0..2).map(|_| AtomicU64::new(0)).collect(),
            error: Mutex::new(None),
            peaks: lanes.iter().map(|l| Arc::clone(&l.peaks)).collect(),
        };
        let res = run(r, lanes, &shared);
```

- [ ] **Step 4: Serve them and the overview peaks**

In `crates/core-ffi/src/record.rs`:

Find:
```rust
use core_media::record::{RING_SECONDS, RecordWriter, WriterConfig, WriterResult};
```
Replace:
```rust
use crate::guard::lock;
use core_media::record::{LivePeaks, RING_SECONDS, RecordWriter, WriterConfig, WriterResult};
```

Find:
```rust
    /// Its file (package-relative), for audio.
    pub path: Option<String>,
    /// Samples clipped so far (integer depths).
    pub clipped: u64,
}
```
Replace:
```rust
    /// Its file (package-relative), for audio.
    pub path: Option<String>,
    /// Samples clipped so far (integer depths).
    pub clipped: u64,
    /// The largest absolute sample written so far (audio; plan 5d-1
    /// Ruling 21).
    #[uniffi(default)]
    pub peak: f32,
}
```

Find:
```rust
    /// The same files with what each holds (pre-flight M9).
    #[uniffi(default)]
    pub recovered_files: Vec<RecoveredFileRow>,
}
```
Replace:
```rust
    /// The same files with what each holds (pre-flight M9).
    #[uniffi(default)]
    pub recovered_files: Vec<RecoveredFileRow>,
    /// Where the pass recording now started, as a tick of the sequence
    /// (plan 5d-1 Ruling 21; `None` before the roll): with
    /// `pass_start_file_frame`, where a view draws the growing take.
    #[uniffi(default)]
    pub pass_start_tick: Option<i64>,
    /// The file frame recorded at `pass_start_tick` (one file holds every
    /// loop pass).
    #[uniffi(default)]
    pub pass_start_file_frame: u64,
    /// Where the record range starts, a tick of the sequence: regions
    /// cover only the range, so a view draws nothing before it.
    #[uniffi(default)]
    pub range_start_tick: i64,
}
```

Find:
```rust
    /// The project format.
    pub format: AudioFormat,
    /// The pass number media get.
    pub pass_number: u32,
}

impl Recording {
```
Replace:
```rust
    /// The project format.
    pub format: AudioFormat,
    /// The pass number media get.
    pub pass_number: u32,
    /// The sequence's tempo, in schedule frames (for the rows' ticks).
    pub tempo: core_engine::schedule::TempoTable,
}

impl Recording {
```

Find:
```rust
    /// The pass number media get.
    pub pass_number: u32,
    marks: Vec<Mark>,
```
Replace:
```rust
    /// The pass number media get.
    pub pass_number: u32,
    /// The sequence's tempo, in schedule frames.
    tempo: core_engine::schedule::TempoTable,
    marks: Vec<Mark>,
```

Find:
```rust
            pass_number: plan.pass_number,
            marks: Vec::new(),
```
Replace:
```rust
            pass_number: plan.pass_number,
            tempo: plan.tempo,
            marks: Vec::new(),
```

Find:
```rust
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
```
Replace:
```rust
            .map(|(k, a)| RecordLaneRow {
                lane: a.lane.0,
                audio: true,
                path: Some(a.path.clone()),
                clipped: w
                    .as_ref()
                    .and_then(|w| w.clipped.get(k).copied())
                    .unwrap_or(0),
                peak: self.peaks(k).map_or(0.0, |p| lock(&p).peak()),
            })
            .collect();
        lanes.extend(self.midi_lanes.iter().map(|l| RecordLaneRow {
            lane: l.0,
            audio: false,
            path: None,
            clipped: 0,
            peak: 0.0,
        }));
        // The pass recording now: from the last wrap, else the roll.
        let pass = self
            .marks
            .iter()
            .rev()
            .find(|m| m.wrap)
            .or_else(|| self.marks.first());
```

Find:
```rust
            late_starts: stream.1,
            recovered: Vec::new(),
            recovered_files: Vec::new(),
        }
    }
```
Replace:
```rust
            late_starts: stream.1,
            recovered: Vec::new(),
            recovered_files: Vec::new(),
            pass_start_tick: pass.map(|m| self.tempo.frame_to_tick(m.frame).0),
            pass_start_file_frame: pass.map_or(0, |m| m.run),
            range_start_tick: self.tempo.frame_to_tick(self.range.start).0,
        }
    }

    /// Audio lane `k`'s live peaks (plan 5d-1 Ruling 21).
    fn peaks(&self, k: usize) -> Option<Arc<std::sync::Mutex<LivePeaks>>> {
        self.writer.as_ref().and_then(|w| w.peaks(k))
    }

    /// `columns` columns of lane `lane`'s take so far over file frames
    /// `[from, to)` (plan 5d-1 Ruling 21): what was written, `Ready`;
    /// `None` when the lane records no audio here.
    pub fn waveform(
        &self,
        lane: LaneId,
        from: u64,
        to: u64,
        columns: u32,
    ) -> Option<crate::overviews::OverviewRows> {
        use crate::overviews::{OverviewRows, OverviewStateRow};
        let k = self.audio.iter().position(|a| a.lane == lane)?;
        let peaks = lock(&self.peaks(k)?).clone();
        let channels = u16::try_from(peaks.channels()).unwrap_or(1);
        let mut out = OverviewRows::silent(OverviewStateRow::Ready, channels, columns);
        let n = out.columns as usize;
        if n == 0 {
            return Some(out);
        }
        let w = (to.saturating_sub(from)) as f64 / n as f64;
        for c in 0..n {
            let a = from + (c as f64 * w).floor() as u64;
            let b = (from + ((c + 1) as f64 * w).ceil() as u64).min(to);
            for ch in 0..peaks.channels() {
                if let Some((lo, hi)) = peaks.range(ch, a, b) {
                    let i = ch * n + c;
                    if let Some(v) = out.min.get_mut(i) {
                        *v = lo;
                    }
                    if let Some(v) = out.max.get_mut(i) {
                        *v = hi;
                    }
                }
            }
        }
        Some(out)
    }
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
            root,
            format,
            pass_number: crate::record::next_pass(&doc),
        };
```
Replace:
```rust
            root,
            format,
            pass_number: crate::record::next_pass(&doc),
            tempo: tempo.clone(),
        };
```

Find:
```rust
                    late_starts: 0,
                    recovered: Vec::new(),
                    recovered_files: Vec::new(),
                },
```
Replace:
```rust
                    late_starts: 0,
                    recovered: Vec::new(),
                    recovered_files: Vec::new(),
                    pass_start_tick: None,
                    pass_start_file_frame: 0,
                    range_start_tick: 0,
                },
```

Find:
```rust
    /// What recording is doing (`state.get record`).
    pub fn record_state(&self) -> Result<crate::record::RecordStateRow, VizError> {
```
Replace:
```rust
    /// `columns` columns of the take lane `lane` is recording, over file
    /// frames `[from_file_frame, to_file_frame)` (plan 5d-1 Ruling 21;
    /// spec §6.2: the region draws as it grows); place them with
    /// `RecordStateRow.pass_start_tick` and `pass_start_file_frame`.
    /// `Model` when nothing records here, the lane records no audio, or
    /// the arguments are out of range.
    pub fn recording_waveform(
        &self,
        lane: u64,
        from_file_frame: u64,
        to_file_frame: u64,
        columns: u32,
    ) -> Result<crate::overviews::OverviewRows, VizError> {
        ffi_guard(|| {
            crate::overviews::check_columns(columns)?;
            if from_file_frame >= to_file_frame {
                return Err(crate::events::bad(
                    "from_file_frame must be before to_file_frame",
                ));
            }
            let inner = lock(&self.inner);
            let r = inner
                .recording
                .as_ref()
                .ok_or_else(|| crate::events::bad("nothing is recording"))?;
            r.waveform(LaneId(lane), from_file_frame, to_file_frame, columns)
                .ok_or_else(|| {
                    crate::events::bad(&format!("lane {lane} records no audio in this pass"))
                })
        })
    }

    /// What recording is doing (`state.get record`).
    pub fn record_state(&self) -> Result<crate::record::RecordStateRow, VizError> {
```

Find:
```rust
            Ok(crate::media::rows(inner.doc(), &inner.offline))
```
Replace:
```rust
            let mut rows = crate::media::rows(inner.doc(), &inner.offline);
            // The overview's peak, once it is built (plan 5d-1 Ruling 21).
            for r in &mut rows {
                r.peak = inner
                    .overviews
                    .get(core_model::ids::MediaId(r.id))
                    .map(|ov| crate::overviews::peak_of(&ov));
            }
            Ok(rows)
```

In `crates/core-ffi/src/media.rs`:

Find:
```rust
    /// How much of its audio regions play, 0–100 (plan 5b Ruling 13).
    #[uniffi(default = 0.0)]
    pub used_percent: f32,
}
```
Replace:
```rust
    /// How much of its audio regions play, 0–100 (plan 5b Ruling 13).
    #[uniffi(default = 0.0)]
    pub used_percent: f32,
    /// The largest absolute sample, from its overview (`None` until the
    /// overview is built; plan 5d-1 Ruling 21).
    #[uniffi(default)]
    pub peak: Option<f32>,
}
```

Find:
```rust
                uses: uses.get(&m.id).copied().unwrap_or(0),
                used_percent: used_percent(spans.get(&m.id), m.length_frames),
            }
```
Replace:
```rust
                uses: uses.get(&m.id).copied().unwrap_or(0),
                used_percent: used_percent(spans.get(&m.id), m.length_frames),
                peak: None,
            }
```

In `crates/core-ffi/src/overviews.rs`:

Find:
```rust
/// The fade gain at region frame `r` of a region of `length` frames (the
```
Replace:
```rust
/// The largest absolute sample an overview holds (every channel, the
/// whole file).
pub fn peak_of(ov: &Overview) -> f32 {
    (0..ov.channels)
        .filter_map(|k| ov.range(k, 0, ov.frames))
        .fold(0f32, |m, (lo, hi)| m.max(lo.abs()).max(hi.abs()))
}

/// The fade gain at region frame `r` of a region of `length` frames (the
```

In `crates/core-ffi/tests/control.rs`:

Find:
```rust
            offline: Some(OfflineReasonRow::Format),
            uses: 3,
            used_percent: 40.0,
        }],
```
Replace:
```rust
            offline: Some(OfflineReasonRow::Format),
            uses: 3,
            used_percent: 40.0,
            peak: Some(0.25),
        }],
```

Find:
```rust
            offline: Some(OfflineReasonRow::Changed),
            uses: 0,
            used_percent: 0.0,
        }],
```
Replace:
```rust
            offline: Some(OfflineReasonRow::Changed),
            uses: 0,
            used_percent: 0.0,
            peak: None,
        }],
```

Find:
```rust
                path: Some("Media/Recorded/Mic.caf".into()),
                clipped: 4,
            }],
```
Replace:
```rust
                path: Some("Media/Recorded/Mic.caf".into()),
                clipped: 4,
                peak: 0.25,
            }],
```

Find:
```rust
            recovered: vec!["Media/Recorded/old.caf".into()],
            recovered_files: vec![],
```
Replace:
```rust
            recovered: vec!["Media/Recorded/old.caf".into()],
            recovered_files: vec![],
            pass_start_tick: Some(7680),
            pass_start_file_frame: 48_000,
            range_start_tick: 0,
```

In `crates/core-ffi/src/record.rs`:

Find:
```rust
/// One armed lane while recording.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecordLaneRow {
```
Replace:
```rust
/// One armed lane while recording.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecordLaneRow {
```

Find:
```rust
/// What a recording is doing (`state.get record`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecordStateRow {
```
Replace:
```rust
/// What a recording is doing (`state.get record`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecordStateRow {
```

Find:
```rust
/// besides the tap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
```
Replace:
```rust
/// besides the tap).
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
```

Find:
```rust
        let peaks = lock(&self.peaks(k)?).clone();
```
Replace:
```rust
        let peaks = lock(&*self.peaks(k)?).clone();
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
    func recordState() throws -> RecordStateRow {
```
Replace:
```swift
    /// What `recordingWaveform` answers (plan 5d-1 Ruling 21); nil: silent.
    var recordingRows: OverviewRows?
    var recordingWaveformCalls: [(lane: UInt64, from: UInt64, to: UInt64, columns: UInt32)] = []

    func recordingWaveform(lane: UInt64, fromFileFrame: UInt64, toFileFrame: UInt64, columns: UInt32) throws
        -> OverviewRows
    {
        recordingWaveformCalls.append((lane, fromFileFrame, toFileFrame, columns))
        return recordingRows
            ?? OverviewRows(
                state: .ready, channels: 1, columns: columns, min: Array(repeating: 0, count: Int(columns)),
                max: Array(repeating: 0, count: Int(columns)))
    }

    func recordState() throws -> RecordStateRow {
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-media --test record 2>&1 | grep -E "^test |test result"
cargo test -p core-ffi --test record the_take_draws 2>&1 | grep -E "^test |test result"
cargo test -p core-ffi --test control 2>&1 | grep -E "test result"
```
Expected: `the_writer_keeps_live_peaks_as_it_writes` and `the_take_draws_while_it_grows_and_its_overview_peaks_at_the_sine` pass; the rest of both files too.

- [ ] **Step 6: Check the crates and the Swift side**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test --workspace --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: clean; all pass.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-media/src/record.rs crates/core-ffi/src/record.rs crates/core-ffi/src/session.rs crates/core-ffi/src/media.rs crates/core-ffi/src/overviews.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-media/tests/record.rs crates/core-ffi/tests/record.rs crates/core-ffi/tests/control.rs
git commit -m "record: live peaks while the take is written; Session.recording_waveform; peaks in the rows (5d-1 Task 10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -11
```

### Task 11: Measure keeps a good result; unused MIDI-destination targets go; New MIDI Destination in the core

**Files:**
- Modify: `crates/core-engine/src/engine.rs` (`measure_target`; `release_stale_targets` releases unnamed MIDI destinations)
- Modify: `crates/core-ffi/src/host.rs` (`end_measuring` keeps the input failure as a fault; `midi_destination_ports`)
- Modify: `crates/core-ffi/src/session.rs` (`add_midi_destination`)
- Modify: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `crates/core-ffi/tests/synth.rs`

**Interfaces:**
- Consumes: Task 7's input thread (the change after a measurement runs there).
- Produces: `vizdaw_ffi::host::midi_destination_ports() -> Vec<String>` (exported; Swift `midiDestinationPorts()`); `Session.add_midi_destination(name: String, port: String, channel: u8) -> Result<DocSnapshot, VizError>` (Swift `addMidiDestination(name:port:channel:)`; `channel` 0–15). Plan 5d-2's "New MIDI Destination…" sheet calls both; Task 12's `destination.add_midi` too.

Plan Rulings 22, 23 and 25 (5c-2 final review findings 12 and 6; the 5c-2 ledger's "New MIDI Destination…" carry). `end_measuring` no longer returns the input change's error with the measurement's: a failure there is the device's, kept as a host fault, and Measure applies its step. At each load, a MIDI-destination target whose port no destination of the document names (through `kinds_for`), and that the installed measurement does not play, is released like a stale plugin instrument.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-ffi/tests/synth.rs <<'VIZDAW_EOF'

/// Plan 5d-1 Ruling 25 ("New MIDI Destination…", 5c-2 carry): the ports
/// a new destination may play (offline: the fake synth's), and the
/// destination as one undo step.
#[test]
fn a_new_midi_destination_names_a_port_in_one_step() {
    let g = serial(Some(DELAY_MS), None);
    assert_eq!(
        vizdaw_ffi::host::midi_destination_ports(),
        vec![FAKE_SYNTH_PORT.to_string()]
    );
    let s = untitled(&g, Builder::new().finish());
    let before = s.destinations().unwrap().len();
    let snap = s
        .add_midi_destination("Synth A".into(), FAKE_SYNTH_PORT.into(), 2)
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Add Destination"));
    let d = s
        .destinations()
        .unwrap()
        .into_iter()
        .find(|d| d.name == "Synth A")
        .unwrap();
    assert!(d.external && d.plugin.is_none());
    s.undo().unwrap();
    assert_eq!(s.destinations().unwrap().len(), before);
    s.close().unwrap();
    set_fake_synth(None);
    assert!(vizdaw_ffi::host::midi_destination_ports().is_empty());
}

/// Review Focus 4: hostile names, ports and channels are `Model` and add
/// nothing.
#[test]
fn hostile_midi_destinations_are_refused() {
    let g = serial(Some(DELAY_MS), None);
    let s = untitled(&g, Builder::new().finish());
    let long = "x".repeat(256);
    for (name, port, channel) in [
        ("", FAKE_SYNTH_PORT, 0u8),
        ("   ", FAKE_SYNTH_PORT, 0),
        (long.as_str(), FAKE_SYNTH_PORT, 0),
        ("a\nb", FAKE_SYNTH_PORT, 0),
        ("Synth", "", 0),
        ("Synth", long.as_str(), 0),
        ("Synth", "Port\u{7}", 0),
        ("Synth", FAKE_SYNTH_PORT, 16),
        ("Synth", FAKE_SYNTH_PORT, 255),
    ] {
        assert!(
            matches!(
                s.add_midi_destination(name.into(), port.into(), channel),
                Err(VizError::Model { .. })
            ),
            "{name:?} {port:?} {channel}"
        );
    }
    assert!(s.destinations().unwrap().is_empty());
    s.close().unwrap();
}

/// Plan 5d-1 Ruling 23 (5c-2 final review finding 6): a MIDI-destination
/// target is released at the load after no destination names its port.
#[test]
fn unused_midi_destination_targets_are_released() {
    let g = serial(Some(DELAY_MS), None);
    let mut b = Builder::new();
    let (seq, rec) = b.sequence("S", "Rec", None).unwrap();
    let s = untitled(&g, b.finish());
    let snap = s
        .add_midi_destination("Synth".into(), FAKE_SYNTH_PORT.into(), 0)
        .unwrap();
    let dest = s
        .destinations()
        .unwrap()
        .into_iter()
        .find(|d| d.name == "Synth")
        .unwrap()
        .id;
    assert_eq!(snap.undo_label.as_deref(), Some("Add Destination"));
    // A lane records its return: every load interns its target.
    set_input(
        &s,
        seq,
        rec.0,
        Some(LaneInputRow::Audio {
            first: 2,
            stereo: false,
            return_of: Some(dest),
        }),
    );
    s.play(seq.0).unwrap();
    assert_eq!(
        midi_destination_targets(),
        vec![FAKE_SYNTH_PORT.to_string()]
    );
    s.stop().unwrap();
    s.undo().unwrap();
    s.undo().unwrap();
    assert!(s.destinations().unwrap().is_empty());
    assert!(
        midi_destination_targets().is_empty(),
        "nothing names the port: {:?}",
        midi_destination_targets()
    );
    s.close().unwrap();
}

/// Plan 5d-1 Ruling 22 (5c-2 final review finding 12): the input change
/// after a measurement runs on the input thread and never takes the
/// measurement with it: the step is applied.
#[test]
fn a_measurement_stands_whatever_the_input_does_after_it() {
    let g = serial(Some(DELAY_MS), None);
    vizdaw_ffi::host::debug_input_open_delay(Some(Duration::from_millis(200)));
    let d = doc(true, 2);
    let s = untitled(&g, d.doc.clone());
    let m = s.measure_return(d.synth.0);
    vizdaw_ffi::host::debug_input_open_delay(None);
    let m = m.unwrap();
    assert_eq!(m.snapshot.undo_label.as_deref(), Some("Set Return Latency"));
    assert!((m.return_latency_ms - DELAY_MS as f32).abs() <= 1.0, "{m:?}");
    // The return lane is armed: the input stays open, once settled.
    wait_until("the input to settle", || !s.input_info().unwrap().opening);
    assert_eq!(s.debug_input_open(), Some(true));
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test synth -- a_new_midi hostile_midi unused_midi a_measurement_stands 2>&1 | tail -5
```
Expected: FAIL to compile with "cannot find function `midi_destination_ports`" and "no method named `add_midi_destination`".

- [ ] **Step 3: Write it**

In `crates/core-engine/src/engine.rs`:

Find:
```rust
    /// Targets were released and no load has published a schedule and a
    /// graph since: the published ones may still name them.
    release_unpublished: bool,
```
Replace:
```rust
    /// Targets were released and no load has published a schedule and a
    /// graph since: the published ones may still name them.
    release_unpublished: bool,
    /// The target the installed measurement plays: never released (plan
    /// 5d-1 Ruling 23).
    measure_target: Option<TargetIdx>,
```

Find:
```rust
            restart: BTreeSet::new(),
            release_unpublished: false,
```
Replace:
```rust
            restart: BTreeSet::new(),
            release_unpublished: false,
            measure_target: None,
```

Find:
```rust
    fn release_stale_targets(&mut self, doc: &Document, max: u32) -> Result<(), EngineError> {
        let stale: Vec<TargetIdx> = self
```
Replace:
```rust
    fn release_stale_targets(&mut self, doc: &Document, max: u32) -> Result<(), EngineError> {
        // The MIDI destinations the document still names (plan 5d-1
        // Ruling 23; 5c-2 final review finding 6).
        let ports = self.factory.ports();
        let named: Vec<TargetKind> = doc
            .instruments()
            .values()
            .filter_map(|d| kinds_for(&d.kind, self.cfg.route, &ports).ok())
            .flatten()
            .collect();
        let measured = self.measure_target;
        let stale: Vec<TargetIdx> = self
```

Find:
```rust
                    TargetKind::MidiVirtualSource | TargetKind::MidiDestination(_) => false,
```
Replace:
```rust
                    TargetKind::MidiVirtualSource => false,
                    TargetKind::MidiDestination(_) => {
                        measured != Some(*i) && !named.contains(kind)
                    }
```

Find:
```rust
        let (tap, status) = crate::measure::tap(&req, self.cfg.sample_rate);
        self.push(EngineCmd::Measure(Some(Box::new(tap))))?;
        Ok(Ok(status))
```
Replace:
```rust
        let (tap, status) = crate::measure::tap(&req, self.cfg.sample_rate);
        self.push(EngineCmd::Measure(Some(Box::new(tap))))?;
        self.measure_target = Some(target);
        Ok(Ok(status))
```

Find:
```rust
    pub fn stop_measure(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Measure(None))
    }
```
Replace:
```rust
    pub fn stop_measure(&mut self) -> Result<(), EngineError> {
        self.measure_target = None;
        self.push(EngineCmd::Measure(None))
    }
```

In `crates/core-ffi/src/host.rs`:

Find:
```rust
        self.measure = None;
        let removed = self.engine.stop_measure().map_err(VizError::from);
        let input = match want.or(self.pending_input.take()) {
            Some(want) => {
                self.pending_input = None;
                self.ensure_input(want)
            }
            None => Ok(()),
        };
        removed.and(input)
    }
```
Replace:
```rust
        self.measure = None;
        let removed = self.engine.stop_measure().map_err(VizError::from);
        let input = match want.or(self.pending_input.take()) {
            Some(want) => {
                self.pending_input = None;
                self.ensure_input(want)
            }
            None => Ok(()),
        };
        // A good measurement stands when the input change after it fails:
        // the failure is the device's, kept as a fault (plan 5d-1 Ruling
        // 22; 5c-2 final review finding 12).
        if let Err(e) = input {
            let f = format!("audio input after the measurement: {e}");
            if !self.faults.contains(&f) {
                self.faults.push(f);
            }
        }
        removed
    }
```

Find:
```rust
/// How many engine loads the host made (test hook).
```
Replace:
```rust
/// The MIDI destinations a new destination may play (plan 5d-1 Ruling
/// 25; "New MIDI Destination…"): the system's CoreMIDI destinations on a
/// live host; offline, the fake synth's port when it is on, else none.
#[uniffi::export]
pub fn midi_destination_ports() -> Vec<String> {
    if offline_mode() {
        return fake_synth()
            .map(|_| vec![core_engine::offline::FAKE_SYNTH_PORT.to_string()])
            .unwrap_or_default();
    }
    #[cfg(target_os = "macos")]
    {
        platform_macos::midi::list_destinations()
            .into_iter()
            .map(|p| p.name)
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// How many engine loads the host made (test hook).
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// Makes destination `dest` play `plugin` (every lane using it now
```
Replace:
```rust
    /// "New MIDI Destination…" (plan 5d-1 Ruling 25): a destination named
    /// `name` playing CoreMIDI destination `port` on `channel` (0–15), one
    /// undo step "Add Destination". `Model` for an empty or over-long name
    /// or port (1–255 characters, no control characters) or a channel
    /// above 15.
    pub fn add_midi_destination(
        &self,
        name: String,
        port: String,
        channel: u8,
    ) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let name = name.trim().to_string();
            let port = port.trim().to_string();
            for (what, v) in [("name", &name), ("port", &port)] {
                if v.is_empty() || v.chars().count() > 255 || v.chars().any(char::is_control) {
                    return Err(crate::events::bad(&format!(
                        "the destination's {what} is 1 to 255 characters"
                    )));
                }
            }
            if channel > 15 {
                return Err(crate::events::bad("the MIDI channel is 1 to 16"));
            }
            self.execute(Command::Routing(RoutingCmd::AddDestination {
                name,
                kind: DestKind::MidiEndpoint {
                    endpoint_ref: port,
                    group: 0,
                    channel,
                },
                default_patch: None,
            }))
        })
    }

    /// Makes destination `dest` play `plugin` (every lane using it now
```

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
    func savesAfterPass() throws -> Bool { savesAfterPassRow }
```
Replace:
```swift
    func savesAfterPass() throws -> Bool { savesAfterPassRow }

    /// What `addMidiDestination` was asked (plan 5d-1 Ruling 25).
    var addedDestinations: [(name: String, port: String, channel: UInt8)] = []

    func addMidiDestination(name: String, port: String, channel: UInt8) throws -> DocSnapshot {
        if let e = synthError {
            synthError = nil
            throw e
        }
        addedDestinations.append((name, port, channel))
        return snap
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-ffi --test synth -- a_new_midi hostile_midi unused_midi a_measurement_stands 2>&1 | grep -E "^test |test result"
```
Expected: `a_new_midi_destination_names_a_port_in_one_step`, `hostile_midi_destinations_are_refused`, `unused_midi_destination_targets_are_released` and `a_measurement_stands_whatever_the_input_does_after_it` pass.

- [ ] **Step 5: Check the crates and the Swift side**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo test --workspace --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3 && scripts/swift.sh test 2>&1 | tail -3
```
Expected: clean; all pass (5c-2's synth tests too: a destination the document names keeps its target).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/engine.rs crates/core-ffi/src/host.rs crates/core-ffi/src/session.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-ffi/tests/synth.rs
git commit -m "ffi: Measure keeps a good result; unused MIDI-destination targets go; add a MIDI destination (5d-1 Task 11)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -7
```

### Task 12: The control channel, the Swift plumbing, the backlog and the e2e

**Files:**
- Create: `app/Sources/VizDAWAppModel/AudioCommands.swift`, `app/Tests/VizDAWAppModelTests/AudioCommandsTests.swift`
- Modify: `crates/core-control/src/registry.rs` (`audio.set`, `destination.add_midi`), `crates/core-ffi/src/control.rs` (`ControlResult::InputState.levels`; the input, record, media and transport JSON), `crates/core-ffi/src/live.rs` (`InputInfoRow.sample_rate`, loses `Eq`), `crates/core-ffi/src/session.rs` (`input_info`)
- Modify: `app/Sources/VizDAWAppModel/ControlCommands.swift` (`ControlHost.applyAudio`, the two commands, `state.get input` levels), `app/Sources/VizDAW/ControlChannel.swift` (`AppControlHost.applyAudio`), `docs/feature-backlog.md`
- Test: `crates/core-control/tests/commands.rs`, `crates/core-ffi/tests/control.rs`, `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift` (`FakeControlHost`), `app/Tests/VizDAWAppModelTests/RecordModelTests.swift` (the `.inputState` pattern), `crates/vizctl/tests/e2e.rs`

**Interfaces:**
- Consumes: Tasks 4–11's FFI (`input_levels`, `audio_settings` and the setters, `InputInfoRow` fields, `TransportState.counting_in`, `RecordStateRow`/`RecordLaneRow`/`MediaRow` peaks, `add_midi_destination`).
- Produces: registry ids `audio.set` (group "Audio", key-only, no document) and `destination.add_midi` (group "Destination", key-only); `ControlResult::InputState { doc, info, levels }`; Swift `AudioChange`, `AudioCommands { set, addMidi, ids, bufferSizes, maxTrim, change(_:), runSet(_:host:), runAddMidi(_:doc:_:) }`; `ControlHost.applyAudio(_ change: AudioChange) throws`. Plan 5d-2 replaces `AppControlHost.applyAudio`'s body with the app's audio preferences (which keep the values).

Plan Ruling 26 and the backlog items of Rulings 2 and 5. `state.get input` now also says `software_monitoring`, `buffer_frames` (asked), `device_buffer_frames`, `round_trip_frames`, `round_trip_ms`, `opening`, `levels` and `latency_trim`; `state.get record` each lane's `peak` and the pass placement; `state.get media` each item's `peak`; `state.get transport` `counting_in`. The e2e checks, through the channel: arming before Play claims the engine and meters the input (I2), `audio.set`, a count-in, the growing take's peak and the recorded overview's peak at about 0.25 (5c final review M8).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/core-control/tests/commands.rs <<'VIZDAW_EOF'

/// Plan 5d-1 Task 12 (Rulings 25–26): `audio.set` and
/// `destination.add_midi` take their values by type and range; the app
/// checks the buffer size's six values and the text.
#[test]
fn audio_and_destination_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for (id, params) in [
        ("audio.set", json!({ "software_monitoring": false })),
        (
            "audio.set",
            json!({ "buffer_frames": 256, "input_device": "", "latency_trim": -48 }),
        ),
        ("audio.set", json!({ "buffer_frames": 32 })),
        ("audio.set", json!({ "latency_trim": 192_000 })),
        (
            "destination.add_midi",
            json!({ "name": "Juno", "port": "Juno MIDI" }),
        ),
        (
            "destination.add_midi",
            json!({ "name": "Juno", "port": "Juno MIDI", "channel": 16 }),
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
        ("audio.set", json!({ "buffer_frames": 31 })),
        ("audio.set", json!({ "buffer_frames": 1025 })),
        ("audio.set", json!({ "buffer_frames": "128" })),
        ("audio.set", json!({ "buffer_frames": 128.5 })),
        ("audio.set", json!({ "software_monitoring": "off" })),
        ("audio.set", json!({ "latency_trim": 192_001 })),
        ("audio.set", json!({ "latency_trim": -192_001 })),
        ("audio.set", json!({ "input_device": 3 })),
        ("audio.set", json!({ "junk": "x".repeat(100_000) })),
        ("destination.add_midi", json!({ "name": "Juno" })),
        ("destination.add_midi", json!({ "port": "Juno MIDI" })),
        (
            "destination.add_midi",
            json!({ "name": "Juno", "port": "P", "channel": 0 }),
        ),
        (
            "destination.add_midi",
            json!({ "name": "Juno", "port": "P", "channel": 17 }),
        ),
        (
            "destination.add_midi",
            json!({ "name": 4, "port": "P" }),
        ),
        (
            "destination.add_midi",
            json!({ "name": "x".repeat(100_000), "port": "P" }),
        ),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
}
VIZDAW_EOF
```

Create `app/Tests/VizDAWAppModelTests/AudioCommandsTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/AudioCommandsTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The audio settings and New MIDI Destination over the control channel
/// (plan 5d-1 Task 12, Rulings 25–26): values of the wrong shape or range
/// are `invalid_params` and change nothing; the host (the app's
/// preferences) gets the rest; the core's refusals are the call's failure.
@MainActor
@Suite struct AudioCommandsTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ id: String, _ pairs: [(String, ControlValue)], _ s: DocumentStore, _ doc: UUID?) -> ControlOutcome {
        ControlCommands.run(
            id, args: pairs.map { ControlArg(name: $0.0, value: $0.1) },
            doc: doc.flatMap { d in s.model(d).map { (d, $0) } }, store: s, host: host)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func audioSetChecksItsValues() throws {
        let (s, _, _, _) = try demo()
        // No document needed: the settings are the app's.
        #expect(
            failure(
                run(
                    "audio.set",
                    [
                        ("software_monitoring", .flag(value: false)), ("buffer_frames", .int(value: 256)),
                        ("input_device", .text(value: "Studio In")), ("latency_trim", .int(value: -12)),
                    ], s, nil)) == nil)
        #expect(
            host.audio.last
                == AudioChange(softwareMonitoring: false, bufferFrames: 256, inputDevice: .some("Studio In"), latencyTrim: -12))
        #expect(failure(run("audio.set", [("input_device", .text(value: " "))], s, nil)) == nil)
        #expect(host.audio.last?.inputDevice == .some(nil), "blank is Same as output")
        let before = host.audio.count
        for bad: [(String, ControlValue)] in [
            [], [("buffer_frames", .int(value: 100))], [("buffer_frames", .int(value: 0))],
            [("buffer_frames", .int(value: 2048))], [("buffer_frames", .int(value: -128))],
            [("latency_trim", .int(value: 192_001))], [("latency_trim", .int(value: .min))],
            [("input_device", .text(value: String(repeating: "x", count: 300)))],
            [("input_device", .text(value: "a\nb"))],
        ] {
            #expect(failure(run("audio.set", bad, s, nil))?.code == .invalidParams, "\(bad)")
        }
        #expect(host.audio.count == before, "nothing applied")
        // The core refuses a buffer change while recording: busy.
        host.audioError = .Busy(message: "Stop recording to change the I/O buffer size.")
        #expect(failure(run("audio.set", [("buffer_frames", .int(value: 512))], s, nil))?.code == .busy)
    }

    @Test func addMidiDestinationChecksItsValues() throws {
        let (s, id, _, f) = try demo()
        #expect(
            failure(
                run(
                    "destination.add_midi",
                    [("name", .text(value: " Juno ")), ("port", .text(value: "Juno MIDI")), ("channel", .int(value: 3))],
                    s, id)) == nil)
        #expect(f.addedDestinations.last?.name == "Juno" && f.addedDestinations.last?.channel == 2)
        #expect(
            failure(run("destination.add_midi", [("name", .text(value: "A")), ("port", .text(value: "P"))], s, id)) == nil)
        #expect(f.addedDestinations.last?.channel == 0, "channel 1 by default")
        let before = f.addedDestinations.count
        for bad: [(String, ControlValue)] in [
            [("port", .text(value: "P"))], [("name", .text(value: "A"))],
            [("name", .text(value: " ")), ("port", .text(value: "P"))],
            [("name", .text(value: "A")), ("port", .text(value: "P")), ("channel", .int(value: 0))],
            [("name", .text(value: "A")), ("port", .text(value: "P")), ("channel", .int(value: 17))],
            [("name", .text(value: String(repeating: "x", count: 256))), ("port", .text(value: "P"))],
        ] {
            #expect(failure(run("destination.add_midi", bad, s, id))?.code == .invalidParams, "\(bad)")
        }
        #expect(f.addedDestinations.count == before)
        f.synthError = .Model(message: "the destination's name is 1 to 255 characters")
        #expect(
            failure(run("destination.add_midi", [("name", .text(value: "A")), ("port", .text(value: "P"))], s, id))?
                .code == .failed)
        #expect(failure(run("destination.add_midi", [("name", .text(value: "A"))], s, nil))?.code == .noDocument)
    }
}
VIZDAW_EOF
```

Append the e2e test (it uses the file's `wait_for_record`, `wait_for_undo` and `App`):

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat >> crates/vizctl/tests/e2e.rs <<'VIZDAW_EOF'

/// Polls `state.get target` until `ok` holds.
fn wait_for_state(app: &mut App, window: &Value, target: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let r = app.call("state.get", json!({"target": target, "window": window}));
        if ok(&r) {
            return r;
        }
        assert!(
            Instant::now() < deadline,
            "{target} state within {WAIT_MS} ms: {r}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Plan 5d-1 through the channel (README 5d-2 steps): arming before any
/// Play claims the engine and meters the fake input; `audio.set` changes
/// the buffer size asked for and refuses one that is not offered; a
/// count-in plays before the roll with the timeline standing still; the
/// take's peak grows while it records, and its overview peaks at the fake
/// sine's −12 dBFS after Stop (5c final review M8). The offline device:
/// no hardware, no permission prompt, nothing saved.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn arms_meters_counts_in_and_draws_the_take() {
    use core_engine::fixtures::{Builder, Q};
    use core_model::command::{Command, LaneCmd, RoutingCmd, SequenceCmd};
    use core_model::ids::AnyId;
    use core_model::lane::{InputChannels, LaneInput, MonitorMode};
    use core_model::sequence::SeqLength;
    use core_model::time::Tick;
    let mut app = App::start_with(&[("VIZDAW_AUDIO_INPUT", "fake")]);
    // Eight bars at 120 BPM; lane "Mic" records fake input 1, heard never.
    let mut b = Builder::new();
    let (seq, mic) = b.sequence("Take", "Mic", None).unwrap();
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
            lane: mic,
            input: Some(LaneInput::Audio {
                channels: InputChannels::Mono(0),
                return_of: None,
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
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(32 * Q)),
    }))
    .unwrap();
    let path = app.launched.home.join("Arm.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    let w = app.call("app.open", json!({"path": path}))["window"].clone();
    app.wait(&w, json!({"condition": "documents_at_least", "count": 1}));

    // Armed before any Play: the engine is claimed and the input metered.
    app.call(
        "command.run",
        json!({"id": "transport.arm_previous", "window": w}),
    );
    let input = wait_for_state(&mut app, &w, "input", |i| {
        i["levels"][0].as_f64().is_some_and(|l| l > 0.2)
    });
    assert_eq!(input["channels"], 3, "{input}");
    assert_eq!(input["opening"], false, "{input}");
    let t = app.call("state.get", json!({"target": "transport", "window": w}));
    assert_eq!(t["playing"], false, "nothing played: {t}");

    // The buffer size: one of six, as asked (the offline device keeps its
    // 512-frame blocks).
    app.call(
        "command.run",
        json!({"id": "audio.set", "params": {"buffer_frames": 256}, "window": w}),
    );
    let input = app.call("state.get", json!({"target": "input", "window": w}));
    assert_eq!(
        (input["buffer_frames"].clone(), input["device_buffer_frames"].clone()),
        (json!(256), json!(512)),
        "{input}"
    );
    let e = app.call_err(
        "command.run",
        json!({"id": "audio.set", "params": {"buffer_frames": 100}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e}");

    // One bar of count-in: the timeline stands still, nothing records.
    app.call(
        "command.run",
        json!({"id": "record.set", "params": {"count_in": 1, "loop": false, "replace": true}, "window": w}),
    );
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    let t = wait_for_state(&mut app, &w, "transport", |t| t["counting_in"] == true);
    assert_eq!(t["playing"], false, "{t}");
    let r = app.call("state.get", json!({"target": "record", "window": w}));
    assert_eq!((r["recording"].clone(), r["frames"].clone()), (json!(true), json!(0)), "{r}");
    // Then it rolls; the take's peak grows to the sine's.
    let r = wait_for_record(&mut app, &w, |r| {
        r["frames"].as_u64() >= Some(48_000) && r["lanes"][0]["peak"].as_f64() > Some(0.2)
    });
    assert!(
        (r["lanes"][0]["peak"].as_f64().unwrap() - 0.251).abs() < 0.01,
        "{r}"
    );
    assert_eq!(r["pass_start_tick"], json!(0), "{r}");
    app.call(
        "command.run",
        json!({"id": "transport.record", "window": w}),
    );
    wait_for_record(&mut app, &w, |r| r["recording"] == false);
    wait_for_undo(&mut app, &w, "Record");
    let m = wait_for_state(&mut app, &w, "media", |m| !m["items"][0]["peak"].is_null());
    assert!(
        (m["items"][0]["peak"].as_f64().unwrap() - 0.251).abs() < 0.01,
        "{m}"
    );
    app.quit();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands audio_and_destination 2>&1 | tail -4
```
Expected: FAIL: `audio.set` is an unknown command id (`invalid_params` is not what comes back for the valid forms).

- [ ] **Step 3: Register the commands and widen the JSON**

In `crates/core-control/src/registry.rs`:

Find:
```rust
/// `destination.measure_return` (plan 5c-2 Ruling 11).
```
Replace:
```rust
/// `audio.set` (plan 5d-1 Ruling 26): the app's audio settings.
const AUDIO_SET_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "software_monitoring",
        kind: ParamKind::Flag,
        required: false,
        help: "Play monitored inputs back through their strips (off: direct monitoring on the interface).",
    },
    ParamSpec {
        name: "buffer_frames",
        kind: ParamKind::Int { min: 32, max: 1024 },
        required: false,
        help: "The I/O buffer size: 32, 64, 128, 256, 512 or 1024 frames (refused while recording).",
    },
    ParamSpec {
        name: "input_device",
        kind: ParamKind::Text,
        required: false,
        help: "The audio input device by name (\"\": the output device's own input).",
    },
    ParamSpec {
        name: "latency_trim",
        kind: ParamKind::Int {
            min: -192_000,
            max: 192_000,
        },
        required: false,
        help: "Frames added to where recorded audio lands, for a device that misreports its latency.",
    },
];

/// `destination.add_midi` (plan 5d-1 Ruling 26): New MIDI Destination.
const ADD_MIDI_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "name",
        kind: ParamKind::Text,
        required: true,
        help: "The destination's name.",
    },
    ParamSpec {
        name: "port",
        kind: ParamKind::Text,
        required: true,
        help: "The MIDI destination it plays (its CoreMIDI name).",
    },
    ParamSpec {
        name: "channel",
        kind: ParamKind::Int { min: 1, max: 16 },
        required: false,
        help: "The MIDI channel, 1 to 16 (default 1).",
    },
];

/// `destination.measure_return` (plan 5c-2 Ruling 11).
```

Find:
```rust
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
Replace:
```rust
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
    CommandSpec {
        params: ADD_MIDI_PARAMS,
        ..key_only(cmd(
            "destination.add_midi",
            "Destination",
            "New MIDI Destination",
            None,
            true,
        ))
    },
    CommandSpec {
        params: AUDIO_SET_PARAMS,
        ..key_only(cmd("audio.set", "Audio", "Audio Settings", None, false))
    },
```

In `crates/core-control/tests/commands.rs`:

Find:
```rust
        127,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four; 5c adds record.set, record.recover, lane.set_input, lane.set_monitor and midi.inject; 5c-2 adds lane.track_synth, destination.set_return_latency and destination.measure_return"
    );
```
Replace:
```rust
        129,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four; 5c adds record.set, record.recover, lane.set_input, lane.set_monitor and midi.inject; 5c-2 adds lane.track_synth, destination.set_return_latency and destination.measure_return; 5d-1 adds audio.set and destination.add_midi"
    );
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
    /// `state.get input` (plan 5c; plan 5d-1 Ruling 26).
    InputState {
        /// Document ID.
        doc: String,
        /// What the input offers.
        info: crate::live::InputInfoRow,
        /// Each input channel's peak since the last read.
        levels: Vec<f32>,
    },
```

Find:
```rust
        ControlResult::InputState { doc, info } => json!({
            "doc": doc, "device": info.device, "channels": info.channels,
            "names": info.names, "input_latency": info.input_latency_frames,
            "output_latency": info.output_latency_frames,
        }),
```
Replace:
```rust
        ControlResult::InputState { doc, info, levels } => {
            let a = crate::audio::audio_settings();
            let ms = if info.sample_rate > 0.0 {
                (f64::from(info.round_trip_frames) * 1000.0 / info.sample_rate * 100.0).round() / 100.0
            } else {
                0.0
            };
            json!({
                "doc": doc, "device": info.device, "channels": info.channels,
                "names": info.names, "input_latency": info.input_latency_frames,
                "output_latency": info.output_latency_frames,
                "software_monitoring": a.software_monitoring,
                "buffer_frames": a.buffer_frames,
                "device_buffer_frames": info.buffer_frames,
                "round_trip_frames": info.round_trip_frames,
                "round_trip_ms": ms,
                "opening": info.opening,
                "levels": levels.iter().map(|l| (f64::from(*l) * 1000.0).round() / 1000.0).collect::<Vec<_>>(),
                "latency_trim": a.latency_trim,
            })
        }
```

Find:
```rust
                "uses": m.uses,
                "used_percent": m.used_percent,
            })).collect::<Vec<_>>(),
```
Replace:
```rust
                "uses": m.uses,
                "used_percent": m.used_percent,
                "peak": m.peak.map(|p| (f64::from(p) * 1000.0).round() / 1000.0),
            })).collect::<Vec<_>>(),
```

Find:
```rust
            "lanes": r.lanes.iter().map(|l| json!({
                "lane": l.lane, "audio": l.audio, "path": l.path, "clipped": l.clipped,
            })).collect::<Vec<_>>(),
```
Replace:
```rust
            "lanes": r.lanes.iter().map(|l| json!({
                "lane": l.lane, "audio": l.audio, "path": l.path, "clipped": l.clipped,
                "peak": (f64::from(l.peak) * 1000.0).round() / 1000.0,
            })).collect::<Vec<_>>(),
            "pass_start_tick": r.pass_start_tick,
            "pass_start_file_frame": r.pass_start_file_frame,
            "range_start_tick": r.range_start_tick,
```

Find:
```rust
        "device": t.device, "faults": t.faults, "report": t.report,
        "rate_mismatch": t.rate_mismatch,
    })
```
Replace:
```rust
        "device": t.device, "faults": t.faults, "report": t.report,
        "rate_mismatch": t.rate_mismatch, "counting_in": t.counting_in,
    })
```

In `crates/core-ffi/src/live.rs`:

Find:
```rust
    /// The input is opening or closing on its own thread (plan 5d-1
    /// Ruling 13).
    #[uniffi(default)]
    pub opening: bool,
}
```
Replace:
```rust
    /// The input is opening or closing on its own thread (plan 5d-1
    /// Ruling 13).
    #[uniffi(default)]
    pub opening: bool,
    /// The device's rate (0 before the first Play), for the round trip in
    /// milliseconds.
    #[uniffi(default)]
    pub sample_rate: f64,
}
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
                    buffer_frames: i.buffer_frames,
                    opening: host::input_opening(self.id),
                },
```
Replace:
```rust
                    buffer_frames: i.buffer_frames,
                    opening: host::input_opening(self.id),
                    sample_rate: host::device(self.id).map_or(0.0, |(_, r)| r),
                },
```

Find:
```rust
                    round_trip_frames: 0,
                    opening: false,
                },
```
Replace:
```rust
                    round_trip_frames: 0,
                    opening: false,
                    sample_rate: 0.0,
                },
```

In `crates/core-ffi/tests/control.rs`:

Find:
```rust
            buffer_frames: 512,
            round_trip_frames: 2048,
            opening: false,
        },
    });
    assert_eq!(i["channels"], json!(3));
    assert_eq!(i["input_latency"], json!(512));
}
```
Replace:
```rust
            buffer_frames: 512,
            round_trip_frames: 2048,
            opening: false,
            sample_rate: 48_000.0,
        },
        levels: vec![0.25, 1.0, 0.0],
    });
    assert_eq!(i["channels"], json!(3));
    assert_eq!(i["input_latency"], json!(512));
    // Plan 5d-1 Ruling 26: the settings, the round trip and the meters.
    assert_eq!(i["device_buffer_frames"], json!(512));
    assert_eq!(i["round_trip_frames"], json!(2048));
    assert_eq!(i["round_trip_ms"], json!(42.67));
    assert_eq!(i["levels"], json!([0.25, 1.0, 0.0]));
    assert_eq!(i["opening"], json!(false));
    assert!(i["software_monitoring"].is_boolean() && i["buffer_frames"].is_u64());
    assert_eq!(v["lanes"][0]["peak"], json!(0.25));
    assert_eq!(v["pass_start_tick"], json!(7680));
    assert_eq!(v["pass_start_file_frame"], json!(48_000));
    assert_eq!(v["range_start_tick"], json!(0));
}
```

Find:
```rust
    assert_eq!(v["items"][0]["used_percent"], json!(40.0));
```
Replace:
```rust
    assert_eq!(v["items"][0]["used_percent"], json!(40.0));
    assert_eq!(v["items"][0]["peak"], json!(0.25));
```

Find:
```rust
    assert_eq!(changed["items"][0]["offline_reason"], "changed");
```
Replace:
```rust
    assert_eq!(changed["items"][0]["offline_reason"], "changed");
    assert_eq!(changed["items"][0]["peak"], Value::Null);
```

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:
```swift
    /// Gives a transport field of document `doc`'s window the keyboard
    /// (plan 4f Ruling 16); false when the window shows none.
    func focus(_ field: TransportField, _ doc: UUID) -> Bool
}
```
Replace:
```swift
    /// Gives a transport field of document `doc`'s window the keyboard
    /// (plan 4f Ruling 16); false when the window shows none.
    func focus(_ field: TransportField, _ doc: UUID) -> Bool
    /// Applies `audio.set`'s change to the app's audio settings (plan
    /// 5d-1 Ruling 26); the core's refusal is thrown.
    func applyAudio(_ change: AudioChange) throws
}
```

Find:
```swift
            .union(fieldKeys.keys).union(RegionCommands.ids).union(SynthCommands.ids)
            .union(TransportCommands.ids).union(StepCommands.ids)
```
Replace:
```swift
            .union(fieldKeys.keys).union(RegionCommands.ids).union(SynthCommands.ids)
            .union(TransportCommands.ids).union(StepCommands.ids).union(AudioCommands.ids)
```

Find:
```swift
        case "keys.reset":
            host.keys.resetAll()
            return .done(ran(id, nil))
        default: break
        }
```
Replace:
```swift
        case "keys.reset":
            host.keys.resetAll()
            return .done(ran(id, nil))
        case AudioCommands.set:
            return AudioCommands.runSet(args, host: host)
        default: break
        }
```

Find:
```swift
        case _ where SynthCommands.ids.contains(id):
            return SynthCommands.run(id, args: args, doc: docID, m)
```
Replace:
```swift
        case _ where SynthCommands.ids.contains(id):
            return SynthCommands.run(id, args: args, doc: docID, m)
        case AudioCommands.addMidi:
            return AudioCommands.runAddMidi(args, doc: docID, m)
```

Find:
```swift
            guard let info = try? m.session.inputInfo() else { return nil }
            return .inputState(doc: doc, info: info)
```
Replace:
```swift
            guard let info = try? m.session.inputInfo() else { return nil }
            return .inputState(doc: doc, info: info, levels: (try? m.session.inputLevels()) ?? [])
```

In `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`:

Find:
```swift
    func closeDocument(_ doc: UUID) { calls.append("close") }
    func showMediaPool(_ doc: UUID) { calls.append("media pool") }
}
```
Replace:
```swift
    func closeDocument(_ doc: UUID) { calls.append("close") }
    func showMediaPool(_ doc: UUID) { calls.append("media pool") }
    /// What `audio.set` applied (plan 5d-1 Task 12), and its refusal.
    var audio: [AudioChange] = []
    var audioError: VizError?
    func applyAudio(_ change: AudioChange) throws {
        if let e = audioError { throw e }
        audio.append(change)
    }
}
```

In `app/Sources/VizDAW/ControlChannel.swift`:

Find:
```swift
    var nudge: NudgeSettings { editingPreferences.nudge }
    var keys: KeyBindings { keyBindings }

```
Replace:
```swift
    var nudge: NudgeSettings { editingPreferences.nudge }
    var keys: KeyBindings { keyBindings }

    /// `audio.set` (plan 5d-1 Ruling 26). Plan 5d-2 routes this through
    /// the app's audio preferences, which also keep it.
    func applyAudio(_ change: AudioChange) throws {
        if let b = change.bufferFrames { try setIoBufferFrames(frames: b) }
        if let on = change.softwareMonitoring { setSoftwareMonitoring(on: on) }
        if let d = change.inputDevice { setAudioInputDevice(name: d) }
        if let t = change.latencyTrim { setRecordLatencyTrim(frames: t) }
    }

```

In `docs/feature-backlog.md`:

Find:
```markdown
| Low Latency Mode (Logic Pro style) | While a lane's input is monitored, bypass plugins on its path whose latency exceeds a limit (default 5 ms); they return when monitoring stops. Deferred from SP5 plan 5d by owner decision (spec Ruling 33). |
```
Replace:
```markdown
| Low Latency Mode (Logic Pro style) | While a lane's input is monitored, bypass plugins on its path whose latency exceeds a limit (default 5 ms); they return when monitoring stops. Deferred from SP5 plan 5d by owner decision (spec Ruling 33). |
| Live MIDI through software instruments without compensation delay | Plan 5d-1 leads audio regions so a monitored *audio* input waits for no other path (Ruling 2). A plugin instrument played live through thru still waits for the mix's latency: its strip's compensation delay would have to move ahead of the instrument, scheduling its MIDI early (Logic's "live mode" for the selected track). |
| Strip automation aligned by each strip's arrival time | Automation applies at the renderer's frame on every strip, so a strip whose inputs arrive late (master, a bus behind a latent path, an audio path whose regions are led since plan 5d-1, Ruling 5) moves up to that much early. Offset each strip's automation by its PDC arrival time. |
```

In `crates/core-ffi/src/live.rs`:

Find:
```rust
/// What the audio input offers (`state.get input`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InputInfoRow {
```
Replace:
```rust
/// What the audio input offers (`state.get input`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct InputInfoRow {
```

In `app/Tests/VizDAWAppModelTests/RecordModelTests.swift`:

Find:
```swift
        guard case .inputState(_, let info)? = ControlStates.state(.input, id, m) else {
            Issue.record("input")
            return
        }
        #expect(info.channels == 3)
```
Replace:
```swift
        f.inputLevelsRow = [0.25, 1, 0]
        guard case .inputState(_, let info, let levels)? = ControlStates.state(.input, id, m) else {
            Issue.record("input")
            return
        }
        #expect(info.channels == 3)
        #expect(levels == [0.25, 1, 0], "plan 5d-1 Task 12: the meters ride along")
```

Create `app/Sources/VizDAWAppModel/AudioCommands.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/AudioCommands.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// What `audio.set` changes (plan 5d-1 Ruling 26): each field set is one
/// setting, nil leaves it. `inputDevice` set to `.some(nil)` is "Same as
/// output".
public struct AudioChange: Equatable, Sendable {
    public var softwareMonitoring: Bool?
    public var bufferFrames: UInt32?
    public var inputDevice: String??
    public var latencyTrim: Int32?

    public init(
        softwareMonitoring: Bool? = nil, bufferFrames: UInt32? = nil, inputDevice: String?? = nil,
        latencyTrim: Int32? = nil
    ) {
        self.softwareMonitoring = softwareMonitoring
        self.bufferFrames = bufferFrames
        self.inputDevice = inputDevice
        self.latencyTrim = latencyTrim
    }

    /// Nothing set.
    public var isEmpty: Bool { self == AudioChange() }
}

/// The audio settings and new MIDI destinations from the control channel
/// (plan 5d-1 Rulings 25–26): `audio.set` (app-wide, no document) and
/// `destination.add_midi`. Values of the wrong shape or range are
/// `invalid_params` and never reach the core; the core's refusals (a
/// buffer change while recording is `busy`) are the call's failure. No
/// dialog opens.
@MainActor
public enum AudioCommands {
    public static let set = "audio.set"
    public static let addMidi = "destination.add_midi"
    public static let ids: Set<String> = [set, addMidi]

    /// The I/O buffer sizes (spec §6.6).
    public static let bufferSizes: [UInt32] = [32, 64, 128, 256, 512, 1024]
    /// The latency trim's range in frames (5c `MAX_TRIM`).
    public static let maxTrim: Int64 = 192_000

    /// `audio.set`'s arguments as a change; nil for a value out of range
    /// or no value at all.
    public static func change(_ args: [ControlArg]) -> AudioChange? {
        var c = AudioChange()
        c.softwareMonitoring = args.flag("software_monitoring")
        if let n = args.int("buffer_frames") {
            guard let f = UInt32(exactly: n), bufferSizes.contains(f) else { return nil }
            c.bufferFrames = f
        }
        if let name = args.text("input_device") {
            let t = name.trimmingCharacters(in: .whitespacesAndNewlines)
            guard t.count <= 255, !t.contains(where: { $0.isNewline }) else { return nil }
            c.inputDevice = .some(t.isEmpty ? nil : t)
        }
        if let n = args.int("latency_trim") {
            guard (-maxTrim...maxTrim).contains(n) else { return nil }
            c.latencyTrim = Int32(n)
        }
        return c.isEmpty ? nil : c
    }

    /// `audio.set`: applies `args` through `host` (the app's preferences).
    public static func runSet(_ args: [ControlArg], host: any ControlHost) -> ControlOutcome {
        guard let c = change(args) else {
            return .failed(
                .of(
                    .invalidParams,
                    "audio.set: give software_monitoring, buffer_frames (32, 64, 128, 256, 512 or 1024), input_device or latency_trim (±192000)"
                ))
        }
        do {
            try host.applyAudio(c)
        } catch {
            return .failed(.thrown(error, "Audio Settings Failed"))
        }
        return .done(ControlCommands.ran(set, nil))
    }

    /// `destination.add_midi`: a destination for a MIDI port, one undo step.
    public static func runAddMidi(_ args: [ControlArg], doc: UUID, _ m: DocumentModel) -> ControlOutcome {
        let name = args.text("name")?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let port = args.text("port")?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let channel = args.int("channel") ?? 1
        guard !name.isEmpty, !port.isEmpty, name.count <= 255, port.count <= 255, (1...16).contains(channel) else {
            return .failed(
                .of(
                    .invalidParams,
                    "destination.add_midi: `name` and `port` are 1 to 255 characters, `channel` 1 to 16"))
        }
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        do {
            m.apply(try m.session.addMidiDestination(name: name, port: port, channel: UInt8(channel - 1)))
        } catch {
            return .failed(.thrown(error, "New MIDI Destination Failed"))
        }
        return .done(ControlCommands.ran(addMidi, doc, m))
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo test -p core-control 2>&1 | grep -E "test result|FAILED"
cargo test -p core-ffi --test control 2>&1 | grep -E "test result|FAILED"
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | tail -3
scripts/swift.sh test 2>&1 | grep -E "AudioCommands|everyRegistered|Test run"
```
Expected: `audio_and_destination_commands_check_their_params`, `the_registry_has_unique_ids_and_answers_dialogs_by_parameter` (129), `record_and_input_results_as_json`, `media_and_project_results_as_json`, `audioSetChecksItsValues`, `addMidiDestinationChecksItsValues` and `everyRegisteredMenuCommandHasAHandler` pass; every Swift test passes.

- [ ] **Step 5: The full gates, the app and the e2e**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy -p core-ffi --lib -- -D warnings 2>&1 | tail -2
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings 2>&1 | tail -2
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record 2>&1 | grep "test result"
cargo test --workspace --no-fail-fast 2>&1 | grep -E "test result|FAILED|panicked" | grep -v " 0 failed"; echo "workspace done"
scripts/make-app.sh 2>&1 | tail -2
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1 2>&1 | grep -E "^test |test result"
pgrep -lf MacOS/VizDAW; ls -d /tmp/vze* 2>/dev/null
```
Expected: everything clean and green; 9 e2e tests pass, `arms_meters_counts_in_and_draws_the_take` among them; `pgrep` prints nothing. Remove the `/tmp/vze*` homes the e2e left (`rm -rf /tmp/vze*` — they are the e2e's own throwaway homes).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-ffi/src/control.rs crates/core-ffi/src/live.rs crates/core-ffi/src/session.rs app/Sources/VizDAWAppModel/AudioCommands.swift app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAW/ControlChannel.swift docs/feature-backlog.md crates/core-control/tests/commands.rs crates/core-ffi/tests/control.rs app/Tests/VizDAWAppModelTests/AudioCommandsTests.swift app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift app/Tests/VizDAWAppModelTests/RecordModelTests.swift crates/vizctl/tests/e2e.rs
git commit -m "control: audio.set and destination.add_midi; state.get reports meters, round trip and peaks; e2e (5d-1 Task 12)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -15
```


## Self-Review

- **Spec coverage.** §6.6 (Software Monitoring: Tasks 6, 3; the round trip and own plugins only: Tasks 1–2; I/O Buffer Size with readout and the refusal while recording: Task 6; Low Latency Mode stays in the backlog and nothing here plans it), §6.1 input opening and permission (Task 7), §6.3 the trim (Task 6), §6.4 count-in (Task 5), §6.5 live overviews (Task 10), §6.9's values (Task 6; the views are 5d-2's), §12.1 `state.get input` (Task 12), §12.3 rt-check (Tasks 2–5) and the e2e (Task 12). Every carried item is in the "Carried items" table, with its task.
- **Placeholders.** None: every step has its code or command and its expected output.
- **Type consistency.** Checked by the replay: every name a later task uses (`Plan.leads`, `RegionRun`, `CountIn`, `InputInfoRow` fields, `AudioSettingsRow`, `saves_after_pass`, `recording_waveform`, `add_midi_destination`, `AudioChange`, `ControlHost.applyAudio`) is the one the earlier task produced.
- **What 5d-2 consumes.** `audio_settings` and the four setters plus `set_audio_input_device`/`set_record_latency_trim`; `Session.input_levels`, `input_info` (`round_trip_frames`, `opening`, `buffer_frames`, `sample_rate`); `TransportState.counting_in`; `RecordStateRow` pass placement and `RecordLaneRow.peak`; `Session.recording_waveform`; `Session.saves_after_pass`; `midi_destination_ports` and `Session.add_midi_destination`; Swift `AudioChange` and `ControlHost.applyAudio`.
