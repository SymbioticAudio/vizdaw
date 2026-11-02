# VizDAW 5f-2 — Bounce and Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** File › Bounce… renders the song or the time selection faster than real time, as a mix or as stems (each chosen strip post-fader), with a tail that rings out to −96 dBFS. It writes WAV, AIFF or CAF at 16 or 24 bits (TPDF dither, on by default at 16 bits) or 32-bit float, at any rate from 8 to 384 kHz. Optionally it places the result back in the project (in the project format) on a new lane at the range start, as one undo step. Live playback keeps running, with plugin copies in offline mode, while the bounce renders. A cancelled or failed bounce leaves no partial file, and the channel's `file.bounce` does all of it with no dialog.

**Architecture:**
- **`core-media`.** `export`: pure-Rust WAV, AIFF(-C) and CAF writers that share the CAF writer's TPDF quantizer (dither switchable), plus `export()`. `export()` decodes a float render through the `MediaImporter` (whose macOS ExtAudioFile importer converts the rate), quantizes, writes `.<name>.partial` beside the destination and renames it into place.
- **`core-engine`.** `bounce`: drives a dedicated `Engine` and `OfflineBackend` on the calling (job) thread, with the disk serviced before every block (`DiskMode::Manual`). Each source (mix, stems) is aligned by its strip's arrival in the PDC plan, so stems and mix line up with each other and with what is heard. Tail detection and the cap, cancel and progress live here too. New supporting pieces:
  - `Graph::strip_post` and `Renderer::strip_output`, which read a strip node's post-fader block between blocks;
  - `Engine::tick_frame`;
  - the `ring=<frames>` test insert.
- **`core-host` / `platform-macos`.**
  - `PluginInstance::set_offline`: CLAP's `render` extension and AU's `kAudioUnitProperty_OfflineRender`.
  - `PluginHost::set_offline_render`, so a bounce host's copies render offline.
  - `platform_macos::sinks::BounceSinks`: the DLS synth, with external MIDI silent.
- **`core-ffi`.**
  - `bounce`: process-wide bounce settings (tail cap); the request, check and result rows; the plugin copies (a second `PluginHost` per bounce, made and serviced on the main thread, from the document captured as Save captures it).
  - The `Bounce` job: render to temporary float CAFs in `Media/.incoming`, export each, and optionally import them as `Bounced` media with regions on new lanes, using the import path, generalised.
- **`core-control`, Swift, `vizctl`.** `file.bounce`, the Bounce sheet (File › Bounce…), the external-MIDI warning, progress and cancel, Settings › Audio "Bounce tail limit", the e2e "bounce and check the file", and README 180–184.

**Tech Stack:** Rust 2024 (`core-media`, `core-engine`, `core-host`, `platform-macos`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`, `test-plugins`), Swift 6 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5f-2 — Bounce and export" (split from the former 5f row by plan 5f-1 Ruling 1). This plan delivers:
- §9.1 the offline render (plugins switched to offline rendering), the sources (Mix, Stems, Selection), the range and tail, the external-MIDI warning, and live playback running beside;
- §9.2 the formats and dither, the destinations (a chosen file; "Add to project" as one undo step), and progress and cancel with no partial file;
- §12.1 `bounce { source, range, format, bits, rate, dither, path, add_to_project }`, which waits for completion and returns the file info (as `file.bounce`, Ruling 2);
- §12.3 the bounce tests and the e2e "bounce and check the file".

Read the whole spec (its §3 rules bind here), plan 5a's jobs and import rulings (`docs/superpowers/plans/2026-10-18-audio-playback.md`), and 5d-1 Ruling 2 (region leads) in `docs/superpowers/plans/2026-10-26-monitoring-engine.md`.

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root:** `scripts/build-ffi.sh` (after any Rust change that touches the FFI, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test` and `scripts/make-app.sh`. Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` after plan 5f-1. Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`).
  - `Cargo.lock` does not change: the only manifest change is the `render` feature of `clack-extensions` (Ruling 7), and features are not in the lock. Check it with `git diff --stat Cargo.lock` (empty).
  - Never commit `.superpowers/`, `.DS_Store` or generated files.
  - End every commit message with these two attribution lines (use the `Co-Authored-By` line naming the model you actually are):
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
- **Clean room.**
  - Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any Studio Vision material.
  - No Opcode, Midas, Mackie, Avid, Steinberg, Apple Logic or Soundcraft assets, names, artwork or branding.
  - The file formats come from their public descriptions (RIFF WAVE, AIFF/AIFF-C, CAF).
- **No GUI tools, no real devices.**
  - Never use GUI, computer-use or accessibility-automation tools. Views are checked by the owner by eye from README steps 180–184.
  - Launch VizDAW only through the e2e (Task 7) with `VIZDAW_AUDIO=offline`. At the end of a task that launched it, `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - **Tests never open real audio input or MIDI ports, and never trigger the microphone prompt.**
    - Every `core-ffi` test that plays or bounces calls `vizdaw_ffi::host::force_offline()` first (through its file's `serial` helper).
    - With `force_offline()` (and `VIZDAW_AUDIO=offline`), the bounce engine uses `OfflineSinks`, never `BounceSinks`, so no CoreMIDI client and no DLS unit is made.
    - Plugin copies in tests are the repo's CLAP test plugin (`test-plugins`). AU offline mode is tested on Apple's built-in units in `platform-macos`'s own tests, as 5e-1 did.
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any device. Bounce destinations in tests are inside `tempfile` folders, and sessions that save get `Session::debug_set_trash_dir`.
- **Test waits.** Every `core-ffi` integration test that waits for a job or the transport polls with a deadline from `common::wait_limit()` (`crates/core-ffi/tests/common/mod.rs`, which reads `VIZDAW_TEST_WAIT_SECS`), calling `s.idle()` between polls (the bounce's plugin copies are serviced there). There are no fixed sleeps for results. The e2e polls with its `WAIT_MS` conditions.
- **Crates and dependencies.**
  - Rust stable, edition 2024, license GPL-3.0-only.
  - No new crate and no new dependency. The `render` feature of the existing `clack-extensions` is enabled in `core-host` and `test-plugins` (Ruling 7).
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. New non-test code uses `.get()`, iterators and `checked_*`/`saturating_*`.
- **Platform-neutral cores.** `core-media`, `core-engine`, `core-model` and `core-control` stay free of OS APIs. The WAV, AIFF and CAF writers are pure Rust (Ruling 1).
- **Linux CI.**
  - CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on `ubuntu-latest`.
  - macOS-only code: `BounceSinks`, AU offline mode, and the ExtAudioFile cross-checks in `crates/platform-macos/tests/media.rs` (that crate is macOS-only).
  - A `core-ffi` test that needs a rate change goes through `ExtAudioImporter`, so it is `#[cfg(target_os = "macos")]` with the note "// macOS only: rate conversion goes through ExtAudioFile (the CAF importer converts depth only)", as in `crates/core-ffi/tests/media_pool.rs`.
  - Every task that changes Rust checks `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **Real-time rules (spec §3).**
  - Nothing new runs on a device's real-time thread.
  - The bounce renderer is a separate `Renderer` driven by `OfflineBackend::block` on the job thread. The rt-check builds still cover its code paths: `Renderer::process` is unchanged. `Graph::strip_post` and `Renderer::strip_output` are control-side reads between blocks, marked `// not RT`.
  - The new `TestMode::Ring` arm is `// RT` (it allocates nothing).
  - **rt-check gates** (Tasks 2 and 7): `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures`.
- **Media is write-once.**
  - Bounce writes new files only: exports to `.<name>.partial`, then renames (Ruling 3); "Add to project" through the import path into new names in `Media/`; temporaries in `Media/.incoming/`, which retention already cleans.
  - It never writes into an existing media file. An existing destination is replaced only when the call says `replace` (the save panel's confirmation), and only by a rename after the new file is complete.
- **File compatibility.** `core_model::file::VERSION` stays 3. Bounce adds no model field; the `Bounced` origin exists since 5a.
- **FFI safety.**
  - No panic crosses the FFI. Every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`.
  - The exceptions are the infallible `bounce_settings` and `set_bounce_settings`. Each body runs inside `std::panic::catch_unwind` and answers the defaults or nothing on a panic.
- **UniFFI changes** (Task 4):
  - New: `BounceSourceRow`, `BounceRangeRow`, `ContainerRow`, `BounceRequestRow`, `BounceCheckRow`, `BouncedFileRow`, `BounceResultRow`, `BounceSettingsRow`; `JobKindRow::Bounce`.
  - Free functions: `bounce_settings`, `set_bounce_settings`.
  - `Session` methods: `bounce`, `bounce_check`, `bounce_result`.
  - `FakeSession` implements each new method in Task 4. Swift switches over `JobKindRow` (`ProjectMedia.swift`) gain `.bounce` in Task 4.
- **Swift.** Generated bindings compile in Swift 5 mode, and the app targets and tests in Swift 6 mode with no new warnings. New model tests go to `VizDAWAppModelTests`.
- **Control channel.**
  - Programmatic calls never open dialogs. `file.bounce` without `path` and without `add_to_project` fails before the app with the dialog error (`DialogSpec` "Bounce", `always`, answered by `path` or `add_to_project`).
  - The registry count moves from 143 (after 5f-1) to 144 in Task 5.
  - Hostile-input tests: `bounce_commands_check_their_params` (core-control), `bounceCommandsCheckTheirArguments` (app), `hostile_bounce_requests_are_refused` (core), `hostile_exports_are_refused` (core-media).
- **Generated files** are never committed (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`).
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the Linux clippy, and `cargo test` for the crates touched (the workspace in Tasks 3, 4 and 7).
  - Tasks 4–7 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 7 runs `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests are as in plan 5f-1. Re-run once before investigating, and say so in the report.
  - Baseline: what plan 5f-1 Task 12 measured (written there as Rust 1688 / 14 ignored, Swift 909, e2e 11, registry 143). Each task's last step gives the counts it adds; report the measured counts.
- **How code is given.** As in plan 5f-1: whole new files where their text matters, then Find/Replace excerpts, each occurring once at the stated base. Where a name differs from the tree, use the tree's and say so.
- **Spec values used verbatim:**
  - "drives a copy of the engine's renderer on a worker thread, faster than real time, with the streamer in synchronous mode";
  - "AU: `kAudioUnitProperty_OfflineRender`; CLAP: the `render` extension's offline mode, when the plugin offers it";
  - "**Mix**: master; **Stems**: each chosen strip, post-fader, one file each; **Selection**: master, limited to the arrangement's time selection";
  - "the song (start to end) or the time selection, plus a **tail**: render on until the output stays below −96 dBFS for 0.5 s, capped at 10 s (Setting)";
  - "<n> lanes play external MIDI and will be silent. Track them as audio first: Track Synth…";
  - "Live playback keeps running on the device while a bounce renders. The two never share a renderer, and plugin instances are separate copies restored from the saved state";
  - "WAV, AIFF or CAF; 16- or 24-bit integer with TPDF dither (on by default for 16-bit), or 32-bit float; any rate the converter supports"; "The sheet defaults to the project's rate and depth (§4.1), as WAV";
  - "'Add to project', which stores the result in the project format (§4.1) as a media item (origin `Bounced`) and places it as a region on a new lane at the range start. This is one undo step";
  - "A cancelled or failed bounce deletes its partial file".

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out, most likely first. Each is pinned by named tests in its task.

1. **A bounce that does not line up with what was heard.** A track behind a latent plugin on another bus (PDC leads), a stem of a bus fed by sends, and the mix, all bounced together.
   - Expected: every file starts at the range start as heard. Stems and the mix line up to the sample, with the latency neither cut nor padded. The stems sum to the mix when master is at 0 dB with no inserts.
   - Pinned by `the_mix_is_the_region_as_heard_with_and_without_latency` and `stems_line_up_and_add_up_to_the_mix` (Task 2), and `a_bounce_equals_an_offline_render_of_the_range` (Task 4).
2. **The tail cuts a reverb short, or never ends.** Silence after the range; a plugin that rings for 2 s; a plugin that hisses forever; a tail cap of 0.
   - Expected:
     - with silence, the file is exactly the range;
     - a ring is kept until it is below −96 dBFS, and the file ends at its last loud frame;
     - endless noise stops at the cap;
     - a cap of 0 gives exactly the range;
     - every file of one bounce has the same length.
   - Pinned by `a_silent_end_adds_no_tail`, `a_ring_is_kept_to_its_last_loud_frame`, `endless_noise_stops_at_the_cap` and `a_zero_cap_is_the_range` (Task 2).
3. **A destination that is not what it should be.** The path exists already; it names a folder, or a folder that does not exist; its extension does not match the format; a stem name repeats or has a `/`; the disk is full; the bounce is cancelled at 90 %.
   - Expected:
     - without `replace`, an existing file is refused before anything is rendered;
     - with `replace`, the old file stays intact until the new one is complete;
     - a wrong extension or folder is refused with a clear message;
     - stem names are made safe and unique;
     - a cancel or failure leaves no `.partial`, no temporary CAF, and no media item.
   - Pinned by `an_existing_file_is_refused_unless_replaced`, `replace_keeps_the_old_file_until_the_new_one_is_whole`, `cancel_and_failure_leave_no_partial_file` (Task 1), and `stem_files_get_safe_unique_names` and `cancel_removes_everything_the_bounce_wrote` (Task 4).
4. **Plugins in the bounce differ from playback, or playback stops.** A plugin with unsaved knob changes, a plugin that fails to load as a copy, a bounce started while playing, a second bounce while one runs.
   - Expected:
     - the copies take the live state (captured as Save captures it);
     - a copy that fails renders as a pass-through, and the job says which plugin;
     - live playback goes on, and its engine is not reloaded;
     - a second bounce is refused (Busy) while one runs.
   - Pinned by `copies_render_offline_with_the_live_state`, `a_bounce_while_playing_keeps_playback_running` and `one_bounce_at_a_time` (Task 4), and `clap_copies_are_told_to_render_offline` (Task 3).
5. **Dither and depth surprises.** 16 bits with dither off, 24 bits with dither on, float with dither on, a mix that clips, a rate the CAF importer cannot convert on Linux.
   - Expected:
     - dither changes only the lowest bit;
     - float ignores dither;
     - clipped samples are counted and reported in the job's problems ("12 samples clipped");
     - a rate change without a system converter fails the job with a clear message, never a silent wrong-rate file.
   - Pinned by `dither_changes_only_the_lowest_bit`, `float_ignores_dither`, `clipping_is_counted` (Task 1), and `clipped_samples_are_reported` and `a_rate_change_without_the_system_converter_fails_cleanly` (Task 4).

## Plan Rulings

Each ruling decides something the spec leaves open, in the format "what — why — cost if wrong".

1. **Ruling (writers in Rust, rate conversion by the importer): the containers, quantization and dither are pure Rust (`core_media::export`); only a rate change goes through `MediaImporter::decode` at the target rate.**
   - The spec reads "written through `MediaImporter`'s converter on macOS (`ExtAudioFile` with a float client format …)". On macOS that is ExtAudioFile's converter; on Linux the CAF importer refuses a rate change.
   - Quantization (TPDF from the CAF writer's generator) and the WAV, AIFF-C and CAF layouts are written by VizDAW.
   - Why:
     - "dither changes only the least significant bits" and "a bounce equals an offline render" are provable only with our own quantizer, since ExtAudioFile's float-to-int conversion does not dither;
     - pure writers run on Linux CI;
     - no new unsafe FFI is needed.
   - Cost if wrong: if the owner wants ExtAudioFile-written files (e.g. for broadcast-WAV metadata), a second writer goes behind the same `PcmWriter` API.
2. **Ruling (`file.bounce`, not `bounce`): the registry ID is `file.bounce`, in the File menu ("Bounce…").**
   - Every registry ID has a group and a dot (`the_registry_has_unique_ids_and_answers_dialogs_by_parameter` enforces it).
   - Its params:
     - `source` mix|stems|selection;
     - `strips` (comma-separated IDs, for stems);
     - `range` song|selection, with `start` and `end` ticks for selection when given;
     - `format` wav|aiff|caf;
     - `bits` 16|24|32f;
     - `rate`;
     - `dither`;
     - `path`;
     - `add_to_project`;
     - `replace`.
   - `source: selection` means the mix over the time selection, as the spec names it.
   - Why: the spec names the command `bounce`, but the registry rule predates it.
   - Cost if wrong: a rename.
3. **Ruling (destinations):**
   - For a mix, `path` is the file and its extension must be the container's (`.wav`, `.aif`/`.aiff`, `.caf`).
   - For stems, `path` is an existing folder. Each stem becomes `<folder>/<strip name>.<ext>`, where `/`, `:` and control characters become `-`, and a name taken in this bounce or on disk gets ` 2`, ` 3`, ….
   - Nothing is rendered while a destination is refused:
     - the parent folder is missing;
     - the extension is wrong;
     - the file exists and `replace` is false;
     - a stem name exists and `replace` is false.
   - The export writes `<dir>/.<name>.partial` and renames it onto the destination when complete (replacing only with `replace`). A cancel or an error removes the partial.
   - "Add to project" alone (no `path`) writes no outside file.
   - Why: §9.2's "a chosen file", and the write-once rule for everything VizDAW writes.
   - Cost if wrong: a stems folder layout the owner does not like (a naming change).
4. **Ruling (alignment and length):**
   - The bounce plays the range with `Engine::locate_tick(start)` and `play_until(end)`.
   - The mix drops its first `latency_frames()` frames; stem `s` drops its first `graph.strip_arrival(s)` frames. Both are counted from the first block that starts at the range start while playing.
   - Every file then holds `range + tail` frames:
     - `range` is `tick_frame(end) − tick_frame(start)`;
     - `tail` ends at the last frame where any source is at or above −96 dBFS (1.5849e-5 linear), once all sources have stayed below it for 0.5 s;
     - `tail` is at most the cap (Setting "Bounce tail limit", 0–30 s, default 10 s).
   - Rendering stops when the quiet run reaches 0.5 s or the cap is reached.
   - Why: §9.1's tail rule, with one length for all files of a bounce, so stems line up end to end.
   - Cost if wrong: if the owner wants each stem trimmed to its own tail, compute per source.
5. **Ruling (stems: export to files ignores mute and solo; bounce in place respects them). Owner answer 2026-11-02, replacing this plan's first reading ("every stem is post-fader and pan, before mute and solo").**
   - **Export to files** (`path`): a stem is the strip node's post-fader output (`st.post`: inserts, fader and pan), before mute and solo, as an OMF-style export hands every track over. Mute and solo are edge gains in the graph (plan 5e-1 controller ruling), so a muted strip's stem file still sounds and a soloed strip does not silence the other stem files.
   - **Bounce in place** ("Add to project"): each imported stem is that post-fader output gated by the strip's audibility as the mix hears it (the graph's `audible`: its own mute, another strip's solo, static or automated). A muted or solo-silenced strip bounces silent, and a mute written at a bar line cuts the imported stem exactly where it cuts the mix.
   - The Mix is always as heard; mute and solo are in it either way, so one Mix source serves both the file and the import.
   - With both a `path` and "Add to project", one pass renders each chosen strip twice: `BounceSpec.stems` (the files) and `BounceSpec.heard_stems` (the imports).
   - **How the gate lines up.** Mute and solo act at the strip's main edge, after that edge's delay line (`Graph::feed_arrival(strip, None)`), while the stem is read at the strip's output (`strip_arrival`). A heard stem is therefore delayed by the edge's delay (`feed_arrival − strip_arrival` frames) before each frame is multiplied by the audibility in force at it, and drops `feed_arrival` frames instead of `strip_arrival`, so it lines up with the file stems and the mix to the sample. The audibility switches only at run boundaries (plan 4d Ruling 11), so the renderer reports it per run (`StripOutput.gates`).
   - Why: the owner's answer. A bounce in place replaces what the user hears in the project, so it must sound as heard; an export hands raw stems to another program, which makes its own mix.
   - Cost if wrong: heard stems to files, or raw stems in place, become a sheet option; rendering both kinds in one pass costs one more stem buffer (and a delay line of at most the graph's latency) per chosen strip.
6. **Ruling (plugin copies on the main thread): `Session::bounce` makes the copies on the session's plugin thread (the main thread in the app).**
   1. It first captures the live states into the document exactly as Save does (`capture_all`).
   2. It builds a new `PluginHost` at the project rate with `set_offline_render(true)` and syncs it to that document.
   3. The job thread waits, polling every 5 ms, until `plugins::tick_bounces()` reports no pending loads. That function is called from `Session::idle`. The wait gives up after 60 s, and slots still loading render as pass-through placeholders, listed in the job's problems.
   4. The job thread builds its engine with the copies' `ShelfFactory`.
   5. When the job ends, the bounce host goes to the existing `closing` list, which waits for its processors.
   - A document with no plugin slots needs no main thread.
   - A bounce while another bounce of this session runs is `Busy`.
   - Why:
     - `PluginHost` is not `Send` (plan 2b Ruling 1);
     - the spec's "separate copies restored from the saved state";
     - one at a time keeps memory bounded.
   - Cost if wrong: a plugin that takes more than 60 s to load is a placeholder in the bounce (reported).
7. **Ruling (offline mode in plugins): `PluginInstance::set_offline(&mut self, on: bool) -> bool` defaults to `false`, meaning not supported.**
   - CLAP calls `clap_plugin_render.set(CLAP_RENDER_OFFLINE)` when the plugin offers the extension. The `render` feature of `clack-extensions` 0.2 is enabled in `core-host`, and in `test-plugins` for the gain plugin's implementation.
   - AU sets `kAudioUnitProperty_OfflineRender` (37) to 1 on the global scope.
   - The bounce host applies it to every instance before its first `activate`.
   - Why: §9.1. A feature flag on a crate already in the lock is not a new dependency.
   - Cost if wrong: a plugin that renders differently offline (higher quality) sounds different in the bounce, which is the spec's intent.
8. **Ruling (sinks for the bounce engine):**
   - Offline hosts (`host::offline_mode()`, i.e. tests and the e2e) use `OfflineSinks`, exactly as the offline host does.
   - Live builds use `platform_macos::sinks::BounceSinks`: the DLS synth for the built-in synth, and a silent `MidiOut` for the virtual source and every external destination ("external MIDI silent").
   - The warning counts the distinct lanes (by `(sequence, lane)` of the flattened range) whose notes resolve to an external MIDI destination (`DestKind::MidiEndpoint`).
   - Why: §9.1. External hardware must not play during a bounce.
   - Cost if wrong: none expected; a bounce never sends MIDI.
9. **Ruling ("Add to project"):**
   - Each bounced file (the mix, or each stem) is imported from its temporary float CAF into `Media/`, in the project format (16-bit dithered, as every import), with origin `Bounced` and the name of the file stem (or "Mix" and the strip names when there is no `path`).
   - Each gets a new lane at the end of the bounced sequence, named like its media item, with a region at the range start.
   - All of it is one undo step "Bounce".
   - This reuses the import's apply, generalised to a label, a lane name and an origin.
   - Why: §9.2. The import's apply already does lanes, regions, overviews and the failure clean-up.
   - Cost if wrong: an owner who wants the bounce on the selected lane moves the region.
10. **Ruling (the result): `Session::bounce_result(job) -> Option<BounceResultRow>` returns `files` (path, frames, rate, bits, channels, clipped) and `media` (items added).**
    - The channel's answer status is `"wrote <path> (<frames> frames, <rate> Hz, <bits>)"` per file, joined by `; `, plus `"; added <n> media"` when it added any.
    - Why: §12.1's "returns the file info". `JobRow` stays unchanged.
    - Cost if wrong: one more field in the answer.
11. **Ruling (formats in the sheet):**
    - Defaults: WAV at the project's rate and depth, with dither on when the depth is 16.
    - Rates: the six project rates in the popup, plus "Other…" for any rate from 8,000 to 384,000 Hz.
    - Bits: 16, 24 and 32 float. Dither applies to 16 and 24 bits, and is shown disabled for float.
    - Why: §9.2.
    - Cost if wrong: a popup's contents.

## Carried items

- No carry from 5e names bounce. README 164 says "Zoom in on a bounce (plan 5f) to check to the sample"; README 181 (Task 7) completes it.

## File map

| File | Task | Role |
|---|---|---|
| `crates/core-media/src/export.rs`, `src/caf.rs`, `src/lib.rs`, `crates/platform-macos/tests/media.rs` | 1 | writers, quantizer, `export()` |
| `crates/core-engine/src/bounce.rs`, `src/engine.rs`, `src/render.rs`, `src/graph/exec.rs`, `src/graph/node.rs`, `src/lib.rs`, `tests/bounce.rs` | 2 | the bounce renderer |
| `crates/core-host/{Cargo.toml,src/instance.rs,src/host.rs,src/clap/handlers.rs,src/clap/instance.rs}`, `crates/test-plugins/{Cargo.toml,src/plugin.rs}`, `crates/platform-macos/src/au/{mod.rs,ffi.rs}`, `crates/platform-macos/src/sinks.rs`, tests | 3 | offline mode in plugins; `BounceSinks` |
| `crates/core-ffi/src/bounce.rs`, `src/jobs.rs`, `src/plugins.rs`, `src/session.rs`, `src/lib.rs`, `tests/bounce.rs` | 4 | the bounce job |
| `crates/core-control/src/registry.rs`, tests; `app/Sources/VizDAWAppModel/{BounceCommands.swift,ControlCommands.swift}`, `app/Sources/VizDAW/ControlChannel.swift` | 5 | channel |
| `app/Sources/VizDAWAppModel/{BounceModel.swift,ProjectMedia.swift,AudioPreferences.swift}`, `app/Sources/VizDAW/{BounceSheet.swift,AppCommands.swift,AudioSettingsView.swift}` | 6 | sheet, menu, setting |
| `crates/vizctl/tests/e2e.rs`, `app/README.md` | 7 | e2e, README |

---

### Task 1: `core-media` export: WAV, AIFF and CAF writers, dither, `export()`

**Files:**
- Create: `crates/core-media/src/export.rs`, `crates/core-media/tests/export.rs`
- Modify: `crates/core-media/src/lib.rs`, `crates/core-media/src/caf.rs` (make `Tpdf` `pub(crate)`; `CafWriter::set_dither`), `crates/platform-macos/tests/media.rs`

**Interfaces:**
- Consumes: `core_media::caf::{CafWriter, CafReader, WriteMode}`, `core_media::import::MediaImporter`, `core_model::format::BitDepth`.
- Produces (`core_media::export`):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Container { Wav, Aiff, Caf }
  impl Container {
      /// "wav", "aif", "caf".
      pub fn extension(self) -> &'static str;
      /// Whether `ext` (lowercase compared) names this container ("aiff" too).
      pub fn accepts(self, ext: &str) -> bool;
  }
  pub const MIN_RATE: u32 = 8_000;
  pub const MAX_RATE: u32 = 384_000;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct ExportFormat { pub container: Container, pub depth: BitDepth, pub rate: u32, pub dither: bool }
  impl ExportFormat { pub fn check(&self) -> Result<(), MediaError>; }
  /// What an export wrote.
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Exported { pub path: PathBuf, pub frames: u64, pub channels: u16, pub clipped: u64 }
  /// Writes one file (never over an existing one).
  pub struct PcmWriter { … }
  impl PcmWriter {
      pub fn create(path: &Path, fmt: &ExportFormat, channels: u16) -> Result<PcmWriter, MediaError>;
      pub fn write(&mut self, interleaved: &[f32]) -> Result<(), MediaError>;
      pub fn finish(self) -> Result<Exported, MediaError>;
  }
  /// `source` (a VizDAW float CAF) → `dest` in `fmt`: at most `frames` source frames
  /// (scaled by the rate ratio), through `.<name>.partial` beside `dest`, renamed on success
  /// (onto an existing `dest` only when `replace`). A cancel or error removes the partial.
  pub fn export(importer: &dyn MediaImporter, source: &Path, dest: &Path, fmt: &ExportFormat,
      frames: Option<u64>, replace: bool, progress: &mut dyn FnMut(f32), cancel: &AtomicBool)
      -> Result<Exported, MediaError>;
  /// The partial file `export` writes for `dest`.
  pub fn partial_path(dest: &Path) -> PathBuf;
  // caf.rs
  impl CafWriter { /// Dither on or off (default: on at 16 bits only). Float ignores it.
      pub fn set_dither(&mut self, on: bool); }
  ```

The layouts:
- **WAV.**
  - `RIFF`/`WAVE` with a 16-byte `fmt ` chunk: format tag 1 (PCM) for 16/24-bit, 3 (IEEE float) for float; channels; rate; byte rate; block align; bits.
  - A float file adds a 4-byte `fact` chunk (frames).
  - Then `data`, little-endian. The sizes are patched on `finish`.
  - Data over `u32::MAX − 64` bytes is refused ("WAV files hold at most 4 GB; choose CAF").
- **AIFF.**
  - Integers: `FORM`/`AIFF` with `COMM` (channels, frames, bits, rate as 80-bit extended) and `SSND` (offset 0, block 0, big-endian samples).
  - Float: `FORM`/`AIFC` with `FVER` (0xA2805140), `COMM` with compression `fl32` and the pascal string "32-bit floating point" (padded even), and `SSND` big-endian floats.
  - The same 4 GB limit applies.
- **CAF** is `CafWriter` with `set_dither(fmt.dither)`.

Quantization is `CafWriter`'s: `(v × scale + dither).round()` clamped to the integer range. A sample whose `v × scale` lies outside the range counts as clipped. The same `Tpdf` seed is used, so a WAV and a CAF of one signal hold the same integers.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-media/tests/export.rs <<'VIZDAW_EOF'
//! Bounce files (plan 5f-2 Task 1; spec §9.2, §12.3; Rulings 1, 3).

use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_media::export::{Container, ExportFormat, PcmWriter, export, partial_path};
use core_media::import::CafImporter;
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn signal(frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|f| {
            let x = ((f % 97) as f32 / 97.0) - 0.5;
            [x * 0.9, -x * 0.5]
        })
        .collect()
}

fn float_caf(dir: &Path, frames: usize) -> PathBuf {
    let p = dir.join("render.caf");
    let mut w = CafWriter::create(&p, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&signal(frames)).unwrap();
    w.finish().unwrap();
    p
}

fn fmt(container: Container, depth: BitDepth, dither: bool) -> ExportFormat {
    ExportFormat { container, depth, rate: 48_000, dither }
}

/// `(rate, channels, bits, float, frames, samples as i32 or f32 bits)` of a WAV.
fn read_wav(p: &Path) -> (u32, u16, u16, bool, u64, Vec<i64>) {
    let b = std::fs::read(p).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize, b.len() - 8, "RIFF size patched");
    assert_eq!(&b[8..12], b"WAVE");
    let mut at = 12;
    let (mut rate, mut ch, mut bits, mut tag) = (0, 0, 0, 0);
    let mut data = Vec::new();
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let n = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..at + 8 + n];
        if id == b"fmt " {
            tag = u16::from_le_bytes(body[0..2].try_into().unwrap());
            ch = u16::from_le_bytes(body[2..4].try_into().unwrap());
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
        }
        if id == b"data" {
            data = body.to_vec();
        }
        at += 8 + n + (n & 1);
    }
    let bytes = usize::from(bits / 8);
    let samples: Vec<i64> = data
        .chunks(bytes)
        .map(|c| match bytes {
            2 => i64::from(i16::from_le_bytes([c[0], c[1]])),
            3 => i64::from(i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8),
            _ => i64::from(u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
        })
        .collect();
    (rate, ch, bits, tag == 3, (samples.len() / usize::from(ch)) as u64, samples)
}

/// `(form type, channels, frames, bits, first 8 bytes of SSND samples)` of an AIFF/AIFC.
fn read_aiff(p: &Path) -> ([u8; 4], u16, u32, u16, Vec<u8>) {
    let b = std::fs::read(p).unwrap();
    assert_eq!(&b[0..4], b"FORM");
    assert_eq!(u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize, b.len() - 8);
    let form: [u8; 4] = b[8..12].try_into().unwrap();
    let mut at = 12;
    let (mut ch, mut frames, mut bits, mut ssnd) = (0, 0, 0, Vec::new());
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let n = u32::from_be_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..at + 8 + n];
        if id == b"COMM" {
            ch = u16::from_be_bytes(body[0..2].try_into().unwrap());
            frames = u32::from_be_bytes(body[2..6].try_into().unwrap());
            bits = u16::from_be_bytes(body[6..8].try_into().unwrap());
            assert_eq!(&body[8..10], &[0x40, 0x0E], "48 kHz as an 80-bit extended");
            if &form == b"AIFC" {
                assert_eq!(&body[18..22], b"fl32");
            }
        }
        if id == b"SSND" {
            ssnd = body[8..16.min(body.len())].to_vec();
        }
        at += 8 + n + (n & 1);
    }
    (form, ch, frames, bits, ssnd)
}

fn run(src: &Path, dest: &Path, f: &ExportFormat, replace: bool) -> Result<core_media::export::Exported, core_media::MediaError> {
    export(&CafImporter, src, dest, f, None, replace, &mut |_| {}, &AtomicBool::new(false))
}

#[test]
fn every_container_and_depth_writes_the_frames() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 1000);
    for (c, depth, bits) in [
        (Container::Wav, BitDepth::Int16, 16),
        (Container::Wav, BitDepth::Int24, 24),
        (Container::Wav, BitDepth::Float32, 32),
        (Container::Aiff, BitDepth::Int16, 16),
        (Container::Aiff, BitDepth::Int24, 24),
        (Container::Aiff, BitDepth::Float32, 32),
        (Container::Caf, BitDepth::Int24, 24),
    ] {
        let dest = d.path().join(format!("out {bits}.{}", c.extension()));
        let e = run(&src, &dest, &fmt(c, depth, false), false).unwrap();
        assert_eq!((e.frames, e.channels), (1000, 2), "{c:?} {depth:?}");
        assert!(!partial_path(&dest).exists());
        match c {
            Container::Wav => {
                let (rate, ch, b, float, frames, _) = read_wav(&dest);
                assert_eq!((rate, ch, b, float, frames), (48_000, 2, bits, depth == BitDepth::Float32, 1000));
            }
            Container::Aiff => {
                let (form, ch, frames, b, _) = read_aiff(&dest);
                assert_eq!(&form, if depth == BitDepth::Float32 { b"AIFC" } else { b"AIFF" });
                assert_eq!((ch, frames, b), (2, 1000, bits));
            }
            Container::Caf => {
                let r = CafReader::open(&dest).unwrap();
                assert_eq!((r.info().frames, r.info().depth), (1000, BitDepth::Int24));
            }
        }
    }
}

#[test]
fn wav_and_caf_quantize_alike() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 500);
    run(&src, &d.path().join("a.wav"), &fmt(Container::Wav, BitDepth::Int16, true), false).unwrap();
    run(&src, &d.path().join("a.caf"), &fmt(Container::Caf, BitDepth::Int16, true), false).unwrap();
    let (_, _, _, _, _, wav) = read_wav(&d.path().join("a.wav"));
    let r = CafReader::open(&d.path().join("a.caf")).unwrap();
    let mut back = vec![0f32; 1000];
    r.read(0, &mut back, &mut Vec::new()).unwrap();
    for (w, c) in wav.iter().zip(back.iter()) {
        assert_eq!(*w, (*c * 32_768.0).round() as i64);
    }
}

#[test]
fn dither_changes_only_the_lowest_bit() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 4000);
    for depth in [BitDepth::Int16, BitDepth::Int24] {
        let a = d.path().join(format!("plain {depth:?}.wav"));
        let b = d.path().join(format!("dither {depth:?}.wav"));
        run(&src, &a, &fmt(Container::Wav, depth, false), false).unwrap();
        run(&src, &b, &fmt(Container::Wav, depth, true), false).unwrap();
        let (.., x) = read_wav(&a);
        let (.., y) = read_wav(&b);
        assert!(x.iter().zip(&y).all(|(p, q)| (p - q).abs() <= 1), "{depth:?}");
        assert!(x.iter().zip(&y).any(|(p, q)| p != q), "dither is there: {depth:?}");
    }
}

#[test]
fn float_ignores_dither() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 300);
    run(&src, &d.path().join("a.wav"), &fmt(Container::Wav, BitDepth::Float32, false), false).unwrap();
    run(&src, &d.path().join("b.wav"), &fmt(Container::Wav, BitDepth::Float32, true), false).unwrap();
    assert_eq!(std::fs::read(d.path().join("a.wav")).unwrap(), std::fs::read(d.path().join("b.wav")).unwrap());
}

#[test]
fn clipping_is_counted() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("loud.wav");
    let mut w = PcmWriter::create(&p, &fmt(Container::Wav, BitDepth::Int16, false), 1).unwrap();
    w.write(&[0.5, 1.5, -2.0, 1.0]).unwrap();
    assert_eq!(w.finish().unwrap().clipped, 3, "1.0 is one past the largest 16-bit value too");
}

#[test]
fn a_frame_limit_cuts_the_file() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 1000);
    let dest = d.path().join("cut.wav");
    let e = export(&CafImporter, &src, &dest, &fmt(Container::Wav, BitDepth::Int24, false), Some(640), false, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(e.frames, 640);
}

#[test]
fn an_existing_file_is_refused_unless_replaced() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 100);
    let dest = d.path().join("taken.wav");
    std::fs::write(&dest, b"keep me").unwrap();
    let e = run(&src, &dest, &fmt(Container::Wav, BitDepth::Int16, true), false).unwrap_err();
    assert!(e.to_string().contains("exists"), "{e}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"keep me");
    run(&src, &dest, &fmt(Container::Wav, BitDepth::Int16, true), true).unwrap();
    assert_eq!(read_wav(&dest).4, 100);
}

#[test]
fn replace_keeps_the_old_file_until_the_new_one_is_whole() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("missing.caf");
    let dest = d.path().join("old.wav");
    std::fs::write(&dest, b"old").unwrap();
    assert!(run(&src, &dest, &fmt(Container::Wav, BitDepth::Int16, true), true).is_err());
    assert_eq!(std::fs::read(&dest).unwrap(), b"old", "a failed export leaves the old file");
    assert!(!partial_path(&dest).exists());
}

#[test]
fn cancel_and_failure_leave_no_partial_file() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 50_000);
    let dest = d.path().join("never.wav");
    let cancel = AtomicBool::new(true);
    let e = export(&CafImporter, &src, &dest, &fmt(Container::Wav, BitDepth::Int16, true), None, false, &mut |_| {}, &cancel);
    assert!(matches!(e, Err(core_media::MediaError::Cancelled)));
    assert!(!dest.exists() && !partial_path(&dest).exists());
    let names: Vec<_> = std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 1, "only the source: {names:?}");
}

#[test]
fn hostile_exports_are_refused() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 10);
    for f in [
        ExportFormat { rate: 0, ..fmt(Container::Wav, BitDepth::Int16, true) },
        ExportFormat { rate: 7_999, ..fmt(Container::Wav, BitDepth::Int16, true) },
        ExportFormat { rate: 384_001, ..fmt(Container::Wav, BitDepth::Int16, true) },
    ] {
        assert!(f.check().is_err() && run(&src, &d.path().join("x.wav"), &f, false).is_err());
    }
    assert!(run(&src, &d.path().join("no/such/folder/x.wav"), &fmt(Container::Wav, BitDepth::Int16, true), false).is_err());
    assert!(Container::Aiff.accepts("AIFF") && Container::Aiff.accepts("aif") && !Container::Wav.accepts("caf"));
    let other_rate = ExportFormat { rate: 44_100, ..fmt(Container::Wav, BitDepth::Int16, true) };
    let e = run(&src, &d.path().join("rate.wav"), &other_rate, false).unwrap_err();
    assert!(e.to_string().contains("system importer"), "the CAF importer converts depth only: {e}");
    assert!(!d.path().join("rate.wav").exists() && !partial_path(&d.path().join("rate.wav")).exists());
}
VIZDAW_EOF
```

  Then add to `crates/platform-macos/tests/media.rs` (macOS-only crate):
  - `the_system_reader_reads_every_export`: the six WAV and AIFF combinations from `every_container_and_depth_writes_the_frames`, written from a 48 kHz float CAF, probe through `ExtAudioImporter` with the right channels, rate, bits and frames; decoding each at 48 kHz gives the source within one 16-bit or 24-bit step (exact for float).
  - `export_resamples_48k_to_44k1`: `export(&ExtAudioImporter, …, rate: 44_100)` of 48,000 frames writes 44,100 ± 1 frames.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-media --test export`; `cargo test -p platform-macos --test media`). Expected: FAIL — `core_media::export` not found.

- [ ] **Step 3: Implement `export.rs`** and `CafWriter::set_dither`, and make `Tpdf` `pub(crate)`. In `crates/core-media/src/lib.rs`:

   Find:
   ```rust
   pub mod caf;
   ```
   Replace:
   ```rust
   pub mod caf;
   pub mod export;
   ```

   `export()` refuses (before writing anything) when:
   - the format fails `check`;
   - `dest`'s parent is not an existing folder;
   - `dest` exists and `replace` is false (`MediaError::Unsupported("<name> exists; choose another name or replace it")`);
   - the source is not a VizDAW CAF.

   It decodes through `importer.decode(source, fmt.rate, …)`, writes with `PcmWriter` to `partial_path(dest)`, and then renames. With `replace`, it uses `rename` (which replaces); otherwise the `rename_excl` of `import.rs` (make it `pub(crate)`).

- [ ] **Step 4: Run the tests and the gates.** Expected: core-media +11, platform-macos +2.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-media/src/export.rs crates/core-media/src/lib.rs crates/core-media/src/caf.rs crates/core-media/src/import.rs crates/core-media/tests/export.rs crates/platform-macos/tests/media.rs
git commit -m "core-media: WAV, AIFF and CAF export with switchable TPDF dither, written whole or not at all (plan 5f-2 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +13 (11 on Linux).

---

### Task 2: `core-engine` bounce: sources aligned by arrival, the tail, cancel

**Files:**
- Create: `crates/core-engine/src/bounce.rs`, `crates/core-engine/tests/bounce.rs`
- Modify: `crates/core-engine/src/lib.rs`, `crates/core-engine/src/engine.rs` (`tick_frame`), `crates/core-engine/src/render.rs` (`strip_output`), `crates/core-engine/src/graph/exec.rs` (`strip_nodes`, `strip_post`), `crates/core-engine/src/graph/node.rs` (`TestMode::Ring`)

**Interfaces:**
- Consumes: `Engine::{locate_tick, play_until, latency_frames, streamer, state}`, `OfflineBackend::{block, output, renderer_mut}`, `Graph::strip_arrival`.
- Produces:
  ```rust
  // bounce.rs
  /// −96 dBFS (Ruling 4).
  pub const TAIL_THRESHOLD: f32 = 1.584_893_2e-5;
  /// The quiet run that ends a tail.
  pub const TAIL_QUIET_SECONDS: f64 = 0.5;
  #[derive(Debug, Clone, PartialEq)]
  pub struct BounceSpec {
      /// Render the mix (source 0 when set).
      pub mix: bool,
      /// Strips whose post-fader output is a stem, before mute and solo
      /// (export to files, Ruling 5), in order (after the mix).
      pub stems: Vec<StripId>,
      /// Strips whose stem is gated by mute and solo as heard (Add to
      /// project, Ruling 5), in order (after `stems`).
      pub heard_stems: Vec<StripId>,
      pub start: Tick, pub end: Tick,
      /// The longest tail, in frames.
      pub tail_cap: u64,
  }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct BounceStats { /// Frames every source's file holds (range + tail).
      pub frames: u64, pub range: u64, pub tail: u64, pub underruns: u64 }
  #[derive(Debug, thiserror::Error)] pub enum BounceError { #[error("cancelled")] Cancelled, #[error("{0}")] Engine(#[from] EngineError), #[error("{0}")] Sink(String), #[error("the range is empty")] Empty }
  /// Renders `spec` with `engine`, whose renderer `out` runs, handing each source's
  /// stereo frames to `sink(source, left, right)` in order. Sources write past the
  /// final length while the tail is decided: callers keep the first `frames`.
  pub fn bounce(engine: &mut Engine, out: &mut OfflineBackend, spec: &BounceSpec, block: u32,
      sink: &mut dyn FnMut(usize, &[f32], &[f32]) -> Result<(), String>,
      progress: &mut dyn FnMut(f32), cancel: &AtomicBool) -> Result<BounceStats, BounceError>;
  // engine.rs
  impl Engine { /// The frame of `tick` in the loaded sequence (0 before a load).
      pub fn tick_frame(&self, tick: Tick) -> u64; }
  // render.rs
  /// One strip's output of the last block (Rulings 4, 5).
  pub struct StripOutput<'a> {
      pub left: &'a [f32], pub right: &'a [f32],
      /// `Graph::strip_arrival`: where the post-fader signal is read.
      pub arrival: u64,
      /// `Graph::feed_arrival(strip, None)`: where its mute and solo act.
      pub gate_arrival: u64,
      /// The strip's audibility through the block: `(first frame, audible)` per
      /// change, the first at frame 0 (the renderer's runs, plan 4d Ruling 11).
      pub gates: &'a [(u32, bool)],
  }
  impl Renderer {
      /// `strip`'s post-fader output of the last block. // not RT
      pub fn strip_output(&self, strip: StripId, frames: u32) -> Option<StripOutput<'_>>;
      /// The strips whose audibility each run records (the heard stems); allocates the
      /// lists once, `frames / MIN_RUN + 2` entries each. Called before a bounce. // not RT
      pub fn set_gate_taps(&mut self, strips: &[StripId]);
  }
  // graph/exec.rs
  impl Graph {
      pub fn strip_post(&self, strip: StripId, frames: u32) -> Option<(&[f32], &[f32])>; // not RT
      /// The strip's `audible` now (mute and solo as the mix hears them). // RT
      pub fn strip_audible(&self, strip: StripId) -> Option<bool>;
  }
  // graph/node.rs
  TestMode::Ring(u32)  // id key `ring=<frames>`: passes input; after it falls silent, outputs 0.25 for that many frames
  ```

`Graph::strip_post`:
- It reads the strip node's `NodeState::post` through its `UnsafeCell`. The `// SAFETY:` line says that it is called only between blocks, by the thread that ran the last block, while no job is published.
- `Graph` gains `strip_nodes: Vec<(StripId, usize)>`, sorted, built in `build` beside `strip_arrive`.

The gate taps (Ruling 5): at the start of each run, after `automate` committed the run's controls, the renderer appends `(run offset, strip_audible(strip))` to each tapped strip's list when it differs from the list's last entry, into the lists `set_gate_taps` allocated (no allocation on the real-time thread; the rt-check gates cover it). Each block starts each list afresh with its first run's value. With no taps (every live engine), nothing is recorded.

The algorithm of `bounce` (Ruling 4):
1. Refuse `end <= start` with `Empty`.
2. `range = tick_frame(end) − tick_frame(start)`.
3. Run `locate_tick(start)`, `set_loop(false)` and `play_until(end)`.
4. Before the first block, `out.renderer_mut().set_gate_taps(&spec.heard_stems)`. Per block: `engine.streamer().service()`, then `out.block(block)`. Start counting at the first block after which `engine.state()` is playing. Source `k`'s skip is `latency_frames()` for the mix, `strip_output(strip).arrival` for a stem and `gate_arrival` for a heard stem; its frames are handed on once the block counter passes its skip. A heard stem's frames first pass through a delay line of `gate_arrival − arrival` frames, then each frame is multiplied by 1 or 0 by the audibility `gates` hold for its frame in the block (Ruling 5).
5. After every source has handed on `range` frames, track `quiet` (consecutive frames where every source's samples are all below `TAIL_THRESHOLD` in magnitude) and `last_loud`. Stop when `quiet ≥ TAIL_QUIET_SECONDS × rate` or `tail_rendered ≥ tail_cap`.
6. `frames = range + min(last_loud_after_range, tail_cap)`.
7. Check `cancel` every block.
8. `progress` is `handed / (range + tail_cap)`, rising to 1 at the end.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/bounce.rs <<'VIZDAW_EOF'
//! The bounce renderer (plan 5f-2 Task 2; spec §9.1, §12.3; Rulings 4, 5).

use core_engine::backend::AudioBackend;
use core_engine::bounce::{BounceSpec, TAIL_QUIET_SECONDS, bounce};
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId, StripId};
use core_model::mixer::StripKind;
use core_model::time::Tick;
use std::path::Path;
use std::sync::atomic::AtomicBool;

const QF: u64 = 24_000;
const FRAMES: u64 = 480_000;
const BLOCK: u32 = 512;

fn value(f: u64) -> f32 {
    ((f % 997) as f32 + 1.0) / 2000.0
}

fn caf(path: &Path) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..FRAMES).flat_map(|f| [value(f), -value(f)]).collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

/// One region at beat 1 (frames 100.. of the file) for 6 beats; `latency`
/// on a bus puts the region's path behind PDC leads; `master` an insert on master.
fn doc(latency: u32, master: Option<&str>) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FRAMES, 2).unwrap();
    b.region(seq, lane, Q, m, 100, 6 * QF, 0.0, Fade::default(), Fade::default()).unwrap();
    if latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={latency}"))).unwrap();
    }
    if let Some(p) = master {
        let ms = b.strip_of(StripKind::Master).unwrap();
        b.insert(ms, TestInserts::plugin(p)).unwrap();
    }
    (b.finish(), seq)
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    _dir: tempfile::TempDir,
}

fn rig(doc: &Document, seq: SeqId) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![MediaFile { id: MediaId(0), path: dir.path().join("a.caf") }]);
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    Rig { engine, out, _dir: dir }
}

fn run(r: &mut Rig, spec: &BounceSpec) -> (Vec<Vec<f32>>, core_engine::bounce::BounceStats) {
    let sources = usize::from(spec.mix) + spec.stems.len() + spec.heard_stems.len();
    let mut got = vec![Vec::new(); sources];
    let stats = bounce(&mut r.engine, &mut r.out, spec, BLOCK, &mut |k, l, _| {
        got[k].extend_from_slice(l);
        Ok(())
    }, &mut |_| {}, &AtomicBool::new(false))
    .unwrap();
    for g in &mut got {
        assert!(g.len() as u64 >= stats.frames);
        g.truncate(stats.frames as usize);
    }
    (got, stats)
}

fn spec(end_beats: i64) -> BounceSpec {
    BounceSpec { mix: true, stems: vec![], heard_stems: vec![], start: Tick::ZERO, end: Tick(end_beats * Q), tail_cap: 480_000 }
}

/// The region's AudioPath strip and the "Latent" bus of `doc(latency > 0, _)`.
fn path_and_bus(d: &Document) -> (StripId, StripId) {
    let find = |want: fn(&StripKind) -> bool| {
        d.mixer().order().iter().copied().find(|s| d.mixer().strip(*s).is_ok_and(|x| want(&x.kind))).unwrap()
    };
    (find(|k| matches!(k, StripKind::AudioPath(_))), find(|k| matches!(k, StripKind::Bus)))
}

#[test]
fn the_mix_is_the_region_as_heard_with_and_without_latency() {
    for l in [0, 1000] {
        let (d, seq) = doc(l, None);
        let mut r = rig(&d, seq);
        let (got, stats) = run(&mut r, &spec(8));
        let mix = &got[0];
        assert_eq!(stats.range, 8 * QF);
        assert_eq!(stats.frames, 8 * QF, "silence after the range: no tail (L = {l})");
        assert!(mix[..QF as usize].iter().all(|x| *x == 0.0), "nothing before beat 1 (L = {l})");
        let k = mix[QF as usize] / value(100);
        assert!(k > 0.1, "the strip's pan law is a constant");
        for i in 0..(6 * QF) {
            let want = k * value(100 + i);
            assert!((mix[(QF + i) as usize] - want).abs() < 1e-6, "L = {l}, frame {i}");
        }
    }
}

#[test]
fn stems_line_up_and_add_up_to_the_mix() {
    let (d, seq) = doc(1000, None);
    let strips: Vec<StripId> = d
        .mixer()
        .order()
        .iter()
        .copied()
        .filter(|s| d.mixer().strip(*s).is_ok_and(|x| matches!(x.kind, StripKind::AudioPath(_) | StripKind::Bus)))
        .collect();
    let mut r = rig(&d, seq);
    let (got, _) = run(&mut r, &BounceSpec { stems: strips.clone(), ..spec(8) });
    let path = strips.iter().position(|s| matches!(d.mixer().strip(*s).unwrap().kind, StripKind::AudioPath(_))).unwrap();
    let stem = &got[1 + path];
    assert!(stem[..QF as usize].iter().all(|x| *x == 0.0), "the stem starts at beat 1 too");
    assert!(stem[QF as usize] != 0.0);
    for f in 0..got[0].len() {
        let sum: f32 = got[1..].iter().map(|s| s[f]).sum();
        assert!((sum - got[0][f]).abs() < 1e-5, "frame {f}: stems sum to the mix at 0 dB master");
    }
}

/// Owner answer 2026-11-02 (Ruling 5): a heard stem (Add to project)
/// respects mute and solo; a file stem does not; unmuted they are equal.
#[test]
fn heard_stems_respect_mute_and_solo_and_file_stems_do_not() {
    use core_model::command::MixerCmd;
    for how in ["mute", "solo another strip"] {
        let (d, seq) = doc(1000, None);
        let (path, bus) = path_and_bus(&d);
        let mut b = Builder::from_doc(d);
        b.mixer(if how == "mute" {
            MixerCmd::SetMute { strip: path, mute: true }
        } else {
            MixerCmd::SetSolo { strip: bus, solo: true }
        })
        .unwrap();
        let d = b.finish();
        let mut r = rig(&d, seq);
        let (got, stats) = run(&mut r, &BounceSpec { stems: vec![path], heard_stems: vec![path], ..spec(8) });
        assert_eq!(stats.frames, 8 * QF, "{how}");
        assert!(got[0].iter().all(|x| *x == 0.0), "{how}: the mix is as heard");
        assert!(got[1][QF as usize] != 0.0, "{how}: the file stem ignores it");
        assert!(got[2].iter().all(|x| *x == 0.0), "{how}: the heard stem is silent");
    }
    let (d, seq) = doc(1000, None);
    let (path, _) = path_and_bus(&d);
    let mut r = rig(&d, seq);
    let (got, _) = run(&mut r, &BounceSpec { stems: vec![path], heard_stems: vec![path], ..spec(8) });
    assert!(got[2][QF as usize] != 0.0);
    assert_eq!(got[1], got[2], "unmuted, the heard stem is the file stem, sample for sample");
}

#[test]
fn a_silent_end_adds_no_tail() {
    let (d, seq) = doc(0, None);
    let mut r = rig(&d, seq);
    let (_, stats) = run(&mut r, &spec(7));
    assert_eq!(stats.tail, 0, "the range ends where the region does");
    assert_eq!(stats.frames, 7 * QF);
}

#[test]
fn a_ring_is_kept_to_its_last_loud_frame() {
    let (d, seq) = doc(0, Some("ring=12000"));
    let mut r = rig(&d, seq);
    let (got, stats) = run(&mut r, &spec(7));
    assert_eq!(stats.tail, 12_000, "the ring after the region's end at the range end");
    assert!(got[0][(stats.frames - 1) as usize].abs() > 0.1);
    assert!(stats.frames < 7 * QF + 12_000 + (TAIL_QUIET_SECONDS * 48_000.0) as u64);
}

#[test]
fn endless_noise_stops_at_the_cap() {
    let (d, seq) = doc(0, Some("noise=3"));
    let mut r = rig(&d, seq);
    let (_, stats) = run(&mut r, &BounceSpec { tail_cap: 30_000, ..spec(7) });
    assert_eq!(stats.tail, 30_000);
    assert_eq!(stats.frames, 7 * QF + 30_000);
}

#[test]
fn a_zero_cap_is_the_range() {
    let (d, seq) = doc(0, Some("noise=3"));
    let mut r = rig(&d, seq);
    let (_, stats) = run(&mut r, &BounceSpec { tail_cap: 0, ..spec(7) });
    assert_eq!(stats.frames, 7 * QF);
}

#[test]
fn a_selection_starts_mid_song() {
    let (d, seq) = doc(1000, None);
    let mut r = rig(&d, seq);
    let (got, stats) = run(&mut r, &BounceSpec { start: Tick(3 * Q), end: Tick(5 * Q), ..spec(0) });
    assert_eq!(stats.range, 2 * QF);
    let k = got[0][0] / value(100 + 2 * QF);
    for i in 0..(2 * QF) {
        assert!((got[0][i as usize] - k * value(100 + 2 * QF + i)).abs() < 1e-6, "frame {i}");
    }
}

#[test]
fn cancel_and_an_empty_range_stop_cleanly() {
    let (d, seq) = doc(0, None);
    let mut r = rig(&d, seq);
    let e = bounce(&mut r.engine, &mut r.out, &spec(8), BLOCK, &mut |_, _, _| Ok(()), &mut |_| {}, &AtomicBool::new(true));
    assert!(matches!(e, Err(core_engine::bounce::BounceError::Cancelled)));
    let e = bounce(&mut r.engine, &mut r.out, &BounceSpec { start: Tick(Q), end: Tick(Q), ..spec(8) }, BLOCK, &mut |_, _, _| Ok(()), &mut |_| {}, &AtomicBool::new(false));
    assert!(matches!(e, Err(core_engine::bounce::BounceError::Empty)));
    let e = bounce(&mut r.engine, &mut r.out, &spec(8), BLOCK, &mut |_, _, _| Err("disk full".into()), &mut |_| {}, &AtomicBool::new(false));
    assert!(matches!(e, Err(core_engine::bounce::BounceError::Sink(m)) if m == "disk full"));
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-engine --test bounce`). Expected: FAIL — `core_engine::bounce` not found.

- [ ] **Step 3: Implement.**

   In `crates/core-engine/src/lib.rs`:

   Find:
   ```rust
   pub mod backend;
   ```
   Replace:
   ```rust
   pub mod backend;
   pub mod bounce;
   ```

   In `graph/node.rs`, after the `Impulse` variant:

   Find:
   ```rust
       /// Adds a unit impulse at the very first frame it processes.
       Impulse,
   }
   ```
   Replace:
   ```rust
       /// Adds a unit impulse at the very first frame it processes.
       Impulse,
       /// Passes its input; once the input falls silent, outputs 0.25 for
       /// this many frames (a tail to bounce, plan 5f-2).
       Ring(u32),
   }
   ```

   Find:
   ```rust
                   "impulse" => mode = TestMode::Impulse,
   ```
   Replace:
   ```rust
                   "impulse" => mode = TestMode::Impulse,
                   "ring" => mode = TestMode::Ring(v.parse().map_err(|_| bad())?),
   ```
   Add a `TestNode` field `ring_left: u32` and the `TestMode::Ring(n)` arm in `process`. Per frame: a non-zero input sets `ring_left = n`; a silent input with `ring_left > 0` writes 0.25 to both channels and decrements `ring_left`. `reset` leaves `ring_left` alone.

   In `engine.rs`, after `locate_tick`:

   Find:
   ```rust
       pub fn locate_tick(&mut self, tick: Tick) -> Result<(), EngineError> {
           let frame = self.tempo.as_ref().map_or(0, |t| t.tick_to_frame(tick));
           self.locate_frame(frame)
       }
   ```
   Replace:
   ```rust
       pub fn locate_tick(&mut self, tick: Tick) -> Result<(), EngineError> {
           let frame = self.tick_frame(tick);
           self.locate_frame(frame)
       }

       /// The frame of `tick` in the loaded sequence (0 before a load).
       pub fn tick_frame(&self, tick: Tick) -> u64 {
           self.tempo.as_ref().map_or(0, |t| t.tick_to_frame(tick))
       }
   ```

   Write `strip_post` and `strip_output`, and `bounce.rs` by the algorithm above.

- [ ] **Step 4: Run the tests and the gates**, including the rt-check gates. Expected: core-engine +9, and rt-check green.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/bounce.rs crates/core-engine/src/lib.rs crates/core-engine/src/engine.rs crates/core-engine/src/render.rs crates/core-engine/src/graph/exec.rs crates/core-engine/src/graph/node.rs crates/core-engine/tests/bounce.rs
git commit -m "core-engine: offline bounce of the mix and stems aligned by arrival, with the -96 dBFS tail and its cap (plan 5f-2 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -9
```
Counts: Rust +9.

---

### Task 3: Plugins render offline; `BounceSinks`

**Files:**
- Modify: `crates/core-host/Cargo.toml` and `crates/test-plugins/Cargo.toml` (the `render` feature), `crates/core-host/src/instance.rs`, `crates/core-host/src/host.rs`, `crates/core-host/src/clap/handlers.rs`, `crates/core-host/src/clap/instance.rs`, `crates/test-plugins/src/plugin.rs`, `crates/platform-macos/src/au/mod.rs`, `crates/platform-macos/src/au/ffi.rs`, `crates/platform-macos/src/sinks.rs`
- Test: modify `crates/core-host/tests/clap_host.rs`, `crates/platform-macos/tests/au_host.rs`

**Interfaces:**
- Produces:
  ```rust
  // core_host::instance::PluginInstance
  /// Asks the plugin to render offline (`on`) or in real time (spec §9.1); false when it cannot.
  fn set_offline(&mut self, on: bool) -> bool { let _ = on; false }
  // core_host::PluginHost
  /// Instances loaded from now on (and live ones) render offline (Ruling 7).
  pub fn set_offline_render(&mut self, on: bool);
  // platform_macos::sinks
  /// The bounce's sinks (Ruling 8): the DLS synth; MIDI goes nowhere.
  #[derive(Debug, Default)] pub struct BounceSinks;
  impl SinkFactory for BounceSinks { … }
  ```

Changes:
- **CLAP.** `PluginExts` gains `pub render: Option<PluginRender>`, filled by `instance.get_extension()`. `ClapInstance::set_offline` calls `render.set(&mut handle, RenderMode::Offline / Realtime)` and returns its success, or false without the extension.
- **AU.** `AuInstance::set_offline` sets property 37 (`kAudioUnitProperty_OfflineRender`, global scope, element 0) to `u32::from(on)` through `Unit::set`, and returns `is_ok()`.
- **The test plugin.** The gain kind registers `PluginRender`, whose `set` stores the mode in its main-thread state and returns `Ok`. `has_hard_realtime_requirement` is false.

In `core-host/src/host.rs`, `PluginHost` gains `offline: bool`, false in `new`. Then:

Find:
```rust
        let proc = live.inst.activate(self.sample_rate, self.max_frames)?;
```
Replace:
```rust
        if self.offline {
            // A bounce's copies render offline (plan 5f-2 Ruling 7).
            live.inst.set_offline(true);
        }
        let proc = live.inst.activate(self.sample_rate, self.max_frames)?;
```

In `clap/handlers.rs`:

Find:
```rust
    pub gui: Option<PluginGui>,
}
```
Replace:
```rust
    pub gui: Option<PluginGui>,
    pub render: Option<clack_extensions::render::PluginRender>,
}
```

Find:
```rust
            gui: instance.get_extension(),
```
Replace:
```rust
            gui: instance.get_extension(),
            render: instance.get_extension(),
```

In `crates/core-host/Cargo.toml`, the `clack-extensions` features list gains `"render"`. In `crates/test-plugins/Cargo.toml`, `"render"` is added to `features = ["clack-plugin", "audio-ports", "note-ports", "params", "state", "latency"]`.

- [ ] **Step 1: Write the failing tests.**
  - In `crates/core-host/tests/clap_host.rs`, add `clap_copies_are_told_to_render_offline`. Load `vizdaw.test.gain` from `test_plugins::bundle()` as that file's other tests do: `set_offline(true)` is true, `set_offline(false)` is true. Load the bank kind (no render extension): `set_offline(true)` is false. A `PluginHost` with `set_offline_render(true)` that loads the gain plugin through its `sync` path reports offline mode on the instance: add the test hook `PluginHost::debug_offline_set(id) -> Option<bool>`, recording the last value passed to `set_offline`, under the crate's existing test-only API pattern.
  - In `crates/platform-macos/tests/au_host.rs`, add `au_units_take_offline_render`: Apple's built-in delay (the unit the file already uses) answers `set_offline(true)` with true.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-host --test clap_host`; `cargo test -p platform-macos --test au_host`). Expected: FAIL — `set_offline` not found.

- [ ] **Step 3: Implement** the trait method, the three implementations, the host flag, the feature flags and `BounceSinks`. `BounceSinks::make` gives `TargetKind::BuiltinSynth` a `DlsSynth`, and every MIDI target a `Sink::Midi(Box::new(Silent))`, where `struct Silent; impl MidiOut for Silent { fn send(&mut self, _: u64, _: &[u8]) -> bool { true } }` is `// RT`. It refuses plugin instruments as `MacSinks` does. `ports()` is empty.

- [ ] **Step 4: Run the tests and the gates** (the workspace; `git diff --stat Cargo.lock` empty). Expected: core-host +1, platform-macos +1.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/Cargo.toml crates/test-plugins/Cargo.toml crates/core-host/src/instance.rs crates/core-host/src/host.rs crates/core-host/src/clap/handlers.rs crates/core-host/src/clap/instance.rs crates/test-plugins/src/plugin.rs crates/platform-macos/src/au/mod.rs crates/platform-macos/src/au/ffi.rs crates/platform-macos/src/sinks.rs crates/core-host/tests/clap_host.rs crates/platform-macos/tests/au_host.rs
git commit -m "core-host, platform-macos: plugins render offline in a bounce (CLAP render extension, AU OfflineRender); bounce sinks with MIDI silent (plan 5f-2 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -14
```
Counts: Rust +2.

---

### Task 4: The bounce job in `core-ffi`

**Files:**
- Create: `crates/core-ffi/src/bounce.rs`, `crates/core-ffi/tests/bounce.rs`
- Modify: `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/jobs.rs`, `crates/core-ffi/src/plugins.rs`, `crates/core-ffi/src/session.rs`, `app/Sources/VizDAWAppModel/ProjectMedia.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`

**Interfaces:**
- Consumes: Tasks 1–3; `crate::media::{files, importer, caf_importer}`, `core_media::import::{import_reserved, INCOMING_DIR}`, `core_model::query::flatten`, `Session::capture_all`, `host::offline_mode()`.
- Produces:
  ```rust
  // bounce.rs
  #[derive(Debug, Clone, PartialEq, uniffi::Enum)] pub enum BounceSourceRow { Mix, Stems { strips: Vec<u64> } }
  #[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)] pub enum BounceRangeRow { Song, Ticks { start: i64, end: i64 } }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)] pub enum ContainerRow { Wav, Aiff, Caf }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct BounceRequestRow { pub seq: u64, pub source: BounceSourceRow, pub range: BounceRangeRow,
      pub container: ContainerRow, pub depth: crate::format::BitDepthRow, pub rate: u32, pub dither: bool,
      /// A file (mix) or an existing folder (stems); None: add to the project only.
      pub path: Option<String>, pub add_to_project: bool, pub replace: bool }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct BounceCheckRow { /// Ruling 8.
      pub external_lanes: u32, pub frames: u64, /// The files it would write, in order.
      pub files: Vec<String>, /// The spec's sentence, or None.
      pub warning: Option<String> }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct BouncedFileRow { pub path: String, pub frames: u64, pub rate: u32, pub bits: String, pub channels: u16, pub clipped: u64 }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct BounceResultRow { pub files: Vec<BouncedFileRow>, pub media: Vec<u64> }
  #[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)] pub struct BounceSettingsRow { /// 0–30 000 (Ruling 4).
      pub tail_cap_ms: u32 }
  #[uniffi::export] pub fn bounce_settings() -> BounceSettingsRow;              // default 10 000
  #[uniffi::export] pub fn set_bounce_settings(settings: BounceSettingsRow);    // clamped
  // jobs.rs
  JobKindRow::Bounce
  Work::Bounce { files: ImportFiles, target: Option<ImportTargetRow>, exported: Vec<BouncedFileRow>, copies: Option<u64> }
  // plugins.rs
  pub fn start_bounce(session: u64, doc: &Document, rate: f64, max: u32) -> Result<BounceCopies, VizError>;
  pub struct BounceCopies { pub token: u64, pub factory: core_host::ShelfFactory, pub ready: Arc<AtomicBool>, pub problems: Arc<Mutex<Vec<String>>> }
  pub fn tick_bounces();             // Session::idle; sets `ready` when nothing is loading
  pub fn end_bounce(token: u64);     // to `closing`
  // Session (exported)
  pub fn bounce(&self, req: BounceRequestRow) -> Result<u64, VizError>;           // the job ID
  pub fn bounce_check(&self, req: BounceRequestRow) -> Result<BounceCheckRow, VizError>;
  pub fn bounce_result(&self, job: u64) -> Result<Option<BounceResultRow>, VizError>;
  ```

`Session::bounce` validates everything before it starts:
- `seq` exists, and a `Stems` list is non-empty with known strips, each once;
- the range is not empty, and `Song` is `[0, seq_length)`;
- `rate` is 8,000–384,000, and the depth and dither are valid;
- the destinations follow Ruling 3;
- there is a path or `add_to_project`.

Then it runs `capture_all`, and for a document with plugin slots it calls `plugins::start_bounce`. It spawns `JobKindRow::Bounce`, whose thread:
1. waits for `ready` (or the cancel, or the 60 s deadline);
2. builds `Engine::new(cfg at the project rate, DiskMode::Manual, sinks by Ruling 8)`, the insert factory (the copies' or `NoInserts`), `set_media(media::files(…))` and `load(doc, seq)`;
3. starts an `OfflineBackend` and runs `core_engine::bounce::bounce`, writing each source to a temporary float CAF in `Media/.incoming/`. The `BounceSpec` follows Ruling 5: `mix` for a Mix; for Stems, `stems` = the strips when there is a `path` and `heard_stems` = the strips when `add_to_project` is set (both when both are);
4. drops the engine;
5. exports the files' sources (`core_media::export::export`, `frames: Some(stats.frames)`): the Mix, or each of `stems`;
6. if `add_to_project`, imports the project's sources with `import_reserved(caf_importer, …, project format, stem)`: the Mix, or each of `heard_stems`;
7. removes the temporaries, and returns `Work::Bounce`.

On cancel or error, every file it wrote is removed: partials, temporaries, exported files, and imported media.

`apply_jobs` applies `Work::Bounce` through `apply_import_as`, the import's apply generalised by this refactor:

Find:
```rust
    if let Err(e) = h.begin("Import Audio") {
```
Replace:
```rust
    if let Err(e) = h.begin(how.label) {
```

Find:
```rust
                    origin: Some(MediaOrigin::Imported {
                        original_name: item.original_name.clone(),
                    }),
```
Replace:
```rust
                    origin: Some((how.origin)(item)),
```

Find:
```rust
                            name: format!("Audio {n}"),
```
Replace:
```rust
                            name: how.lane_name(item, n),
```

The refactor:
- `apply_import` becomes `apply_import_as(inner, root, files, target, &ImportAs::IMPORT)`.
- `ImportAs { label: &'static str, origin: fn(&Imported) -> MediaOrigin, named_lanes: bool }`:
  - `IMPORT`: "Import Audio", `Imported { original_name }`, lanes "Audio <n>";
  - `BOUNCE`: "Bounce", `Bounced`, lanes named after the item.
- `ImportAs::lane_name(item, n)` gives `item.name.clone()` for named lanes, else `format!("Audio {n}")`.
- The bounce's target is `ImportTargetRow { seq, lane: None, tick: range start }`.
- `Work::Bounce` is also handled in `discard` (its files and exports) and in `apply_jobs` (calling `plugins::end_bounce(copies)` and storing the `BounceResultRow` for `bounce_result`).
- In Swift, `ProjectMedia.swift`'s two switches gain `case .bounce: "Bouncing…"` and `case .bounce: "Bounce Failed"`.

- [ ] **Step 1: Write the failing tests** in `crates/core-ffi/tests/bounce.rs`, with a `serial()` that calls `force_offline()`, `reset_host()` and `add_test_bundle`, and a `wait_job(s, id)` that polls `s.job(id)` with `s.idle()` until it is not Running, within `common::wait_limit()`:
  - `a_bounce_equals_an_offline_render_of_the_range`: `Session::new_demo_audio()`, Mix of the song to `<tmp>/mix.wav` at 48 kHz/24-bit, no dither. The job is Done. The WAV (read with Task 1's helper, copied here) has `frames == bounce_check(...).frames` and is not silent where the demo's region plays. Its samples equal, to one 24-bit step, the region's media frames from the overview-free CAF read (`CafReader` on the media file) scaled by the constant `k` measured at the region start (the Task 2 method). The result row names the file.
  - `stem_files_get_safe_unique_names`: rename two strips "A/B" and "A:B" (both become "A-B"). Bounce Stems to a temp folder that already holds `A-B.wav`, without replace: refused before rendering ("exists"). With an empty folder: files `A-B.wav` and `A-B 2.wav`.
  - `add_to_project_places_the_bounce_on_a_new_lane`: `path: None, add_to_project: true`. After Done, the undo label is "Bounce", the new last lane of the song is named "Mix" with a region at tick 0, `media()` has a `Bounced` item in the project format, and one undo removes lane and item.
  - `add_to_project_respects_mute_and_solo_and_files_do_not` (Ruling 5): `Session::new_demo_audio()` with its audio strip muted (`set_strip_mute`), Stems of that strip and of one unmuted strip, to a temp folder *and* added to the project. After Done: the muted strip's stem file is not silent where the demo's region plays; its imported media item is silent throughout (every sample of its file in `Media/` is 0); the unmuted strip's file and import both sound. Then unmute it and solo the other strip (`set_strip_solo`): a second bounce gives the same result for the solo-silenced strip.
  - `cancel_removes_everything_the_bounce_wrote`: hold jobs (`debug_hold_jobs(true)`), start, `cancel_job`, release; the state is Cancelled, and there is no `.partial`, no file in `Media/.incoming`, no destination file and no new media.
  - `copies_render_offline_with_the_live_state`: a project with `vizdaw.test.gain` on the synth strip, with its gain parameter set live to 0 (`set_param`, not saved). A Mix bounce is silent (the copy has the live state). `plugins` host count is back to the session's after Done + idle (the copies went to closing).
  - `a_bounce_while_playing_keeps_playback_running`: play the song, bounce, wait. The transport is still playing and `host::loads()` (the engine-load counter test hook) did not change during the bounce.
  - `one_bounce_at_a_time`: with jobs held, a second `bounce` is `Busy`.
  - `external_midi_is_counted_and_silent`: a lane routed to a fake external destination (`destination.add_midi`'s session call, or `Builder::dest`). `bounce_check` gives `external_lanes == 1` and the warning "1 lane plays external MIDI and will be silent. Track it as audio first: Track Synth…" (Ruling 8's singular; plural "<n> lanes play … Track them …"). The bounce completes and the offline log shows no MIDI sent by the bounce engine.
  - `clipped_samples_are_reported`: a region with gain +24 dB to 16-bit. The job's problems contain "samples clipped" and the result's `clipped > 0`.
  - `hostile_bounce_requests_are_refused`: unknown `seq`; empty stems; a stem strip twice; an unknown strip; start ≥ end; negative start; rate 0 and 1,000,000; neither path nor add; a relative path; a missing folder; a wrong extension; a stems path that is a file. Each is `Model` with nothing written to the temp folder.
  - `#[cfg(target_os = "macos")] a_bounce_at_another_rate_converts` (macOS only: rate conversion goes through ExtAudioFile): 44.1 kHz from a 48 kHz project gives a WAV at 44,100 Hz with `frames ≈ range × 44100 / 48000 ± 1`.
  - `#[cfg(not(target_os = "macos"))] a_rate_change_without_the_system_converter_fails_cleanly`: the job fails with "system importer", and nothing is left.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-ffi --test bounce`). Expected: FAIL — `bounce` not found.

- [ ] **Step 3: Implement** as above. `plugins::tick_bounces()` is called from `Session::idle` next to `plugins::tick_closing()`:

   Find:
   ```rust
               plugins::tick_closing();
   ```
   Replace:
   ```rust
               plugins::tick_closing();
               // A bounce's plugin copies load here (plan 5f-2 Ruling 6).
               plugins::tick_bounces();
   ```

   Register `pub mod bounce;` in `lib.rs` after `pub mod blocks;`:

   Find:
   ```rust
   pub mod blocks;
   ```
   Replace:
   ```rust
   pub mod blocks;
   pub mod bounce;
   ```

   In `jobs.rs`:

   Find:
   ```rust
       /// Relink media (a chosen file, or a folder search; plan 5b).
       Relink,
   }
   ```
   Replace:
   ```rust
       /// Relink media (a chosen file, or a folder search; plan 5b).
       Relink,
       /// Bounce (plan 5f-2).
       Bounce,
   }
   ```

   Find:
   ```rust
       /// Cancelled before it finished (whatever it wrote is removed).
       Cancelled,
   ```
   Replace:
   ```rust
       /// A bounce: the files it exported and, for "Add to project", its
       /// imports (plan 5f-2 Ruling 9); `copies` is its plugin copies' token.
       Bounce {
           /// Per source: what its project import became (empty without "Add to project").
           files: Vec<(String, Result<Vec<Imported>, String>)>,
           /// Where regions go.
           target: Option<ImportTargetRow>,
           /// The files written outside the project.
           exported: Vec<crate::bounce::BouncedFileRow>,
           /// The plugin copies to retire.
           copies: Option<u64>,
       },
       /// Cancelled before it finished (whatever it wrote is removed).
       Cancelled,
   ```

- [ ] **Step 4: Run the tests and the gates** (the workspace; FFI; Swift with `FakeSession` stubs: `bounce` records the request and returns a job ID, `bounceCheck` returns `checkRow`, `bounceResult` returns `resultRow`). Expected: core-ffi +12 on macOS (+12 on Linux, with the other cfg test).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/bounce.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/jobs.rs crates/core-ffi/src/plugins.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/bounce.rs app/Sources/VizDAWAppModel/ProjectMedia.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: the Bounce job (plugin copies, external MIDI silent, export, Add to project as one step, nothing left on cancel) (plan 5f-2 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Rust +12.

---

### Task 5: `file.bounce` on the channel

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/tests/commands.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAW/ControlChannel.swift`
- Create: `app/Sources/VizDAWAppModel/BounceCommands.swift`, `app/Tests/VizDAWAppModelTests/BounceCommandsTests.swift`

**Interfaces:**
- Produces: `BounceCommands.id = "file.bounce"`; `BounceCommands.request(_ args: [ControlArg], _ m: DocumentModel) -> Result<BounceRequestRow, ControlFailure>`; `ControlJobs.answer(_ j: JobRow, bounce: BounceResultRow?)` (Ruling 10's status).

In `registry.rs`, after the `file.project_settings` entry:

Find:
```rust
        ..cmd(
            "file.project_settings",
            "File",
            "Project Settings…",
            None,
            true,
        )
    },
```
Replace:
```rust
        ..cmd(
            "file.project_settings",
            "File",
            "Project Settings…",
            None,
            true,
        )
    },
    CommandSpec {
        params: BOUNCE_PARAMS,
        dialogs: &[DialogSpec {
            name: "Bounce",
            when: "always",
            always: true,
            answered_by: &["path", "add_to_project"],
        }],
        ..cmd("file.bounce", "File", "Bounce…", None, true)
    },
```

`BOUNCE_PARAMS` holds:
- `source` Choice mix|stems|selection;
- `strips` Text;
- `range` Choice song|selection;
- `start` and `end` Int (0..=i64::MAX/2);
- `format` Choice wav|aiff|caf;
- `bits` Choice 16|24|32f;
- `rate` Int 8000..=384000;
- `dither` Flag;
- `path` File;
- `add_to_project` Flag;
- `replace` Flag.

Each has a help line saying its default:
- source mix, range song, format wav;
- bits and rate: the project's;
- dither on for 16 bits, add_to_project false, replace false.

The `add_to_project` help also says what Ruling 5 decides: "bounces as heard: a muted or solo-silenced strip is silent; files ignore mute and solo".

`range: selection` without `start`/`end` uses the editor's time selection, else is refused ("no time selection"). `source: selection` is mix plus `range: selection`. In `ControlChannel.swift`:

Find:
```swift
            switch ControlJobs.answer(j) {
```
Replace:
```swift
            let bounced = j.kind == .bounce ? (try? m.session.bounceResult(job: job)) ?? nil : nil
            switch ControlJobs.answer(j, bounce: bounced) {
```

- [ ] **Step 1: Write the failing tests.**
  - `bounce_commands_check_their_params` (core-control):
    - accepted: `{source: mix, path: "/tmp/x.wav"}`, `{add_to_project: true}`, `{source: stems, strips: "1,2", path: "/tmp/stems"}`, `{range: selection, start: 0, end: 15360, path: "/tmp/x.aif", format: aiff, bits: "16", rate: 44100, dither: false}`;
    - refused (`invalid_params`): `format: mp3`, `bits: 8`, `rate: 1000`, `path: "relative.wav"`, `start: -1`;
    - the dialog error when neither path nor add_to_project is given;
    - the count test is 144, with "; 5f-2 adds file.bounce".
  - `BounceCommandsTests.bounceCommandsCheckTheirArguments` (app): a wrong extension for the format; stems without strips; strips not digits; a strip twice; `range: selection` with only `start`, and with start ≥ end; `source: selection` with `range: song`. Each is `invalid_params`, and `f.bounces` is empty.
  - `bounceStartsAJobAndAnswersWithTheFiles`: a valid call returns `.job(command: "file.bounce", …)` with the request as built (the project's rate and depth by default; dither true at 16 bits). `ControlJobs.answer(doneRow, bounce: result)` is `.ok(status: "wrote /tmp/x.wav (96000 frames, 48000 Hz, 24-bit)")`.

- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests and the gates.** Expected: core-control +1, Swift +2, and the registry at 144.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAWAppModel/BounceCommands.swift app/Sources/VizDAW/ControlChannel.swift app/Tests/VizDAWAppModelTests/BounceCommandsTests.swift
git commit -m "control: file.bounce waits for the bounce and answers with the files (plan 5f-2 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +1, Swift +2.

---

### Task 6: The Bounce sheet, File › Bounce…, the tail setting

**Files:**
- Create: `app/Sources/VizDAWAppModel/BounceModel.swift`, `app/Sources/VizDAW/BounceSheet.swift`, `app/Tests/VizDAWAppModelTests/BounceModelTests.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/DocumentView.swift` (present the sheet), `app/Sources/VizDAWAppModel/AudioPreferences.swift` (`bounceTailSeconds`), `app/Sources/VizDAW/AudioSettingsView.swift`, `app/Sources/VizDAWAppModel/DocumentModel.swift` (`bounce: BounceModel`)

**Interfaces:**
- Produces:
  ```swift
  @MainActor @Observable public final class BounceModel {
      public init(document: DocumentModel)
      public var shown: Bool
      public var source: Source               // .mix, .stems
      public var stems: Set<UInt64>
      public var useSelection: Bool           // disabled without a time selection
      public var container: ContainerRow      // .wav
      public var depth: BitDepthRow           // the project's
      public var rate: UInt32                 // the project's
      public var dither: Bool                 // true when depth is .int16; ignored for float
      public var addToProject: Bool
      /// Ruling 8's sentence (from `bounceCheck`), refreshed on every change.
      public private(set) var warning: String?
      public private(set) var length: String  // "1:32.4 + tail"
      /// The save panel's answer (a file, or a folder for stems) → the job.
      public func start(path: String?, replace: Bool)
      public var job: UInt64? { get }         // progress through DocumentModel.jobs
      public func cancel()
      public static let rates: [UInt32]       // the six project rates (Ruling 11)
  }
  ```

Behaviour:
- `DocumentModel` owns `bounce` (lazy).
- The sheet has the source picker, a strip checklist for stems, "Time selection only", format, bits, rate (with "Other…" as a field from 8,000 to 384,000), Dither (disabled for float), "Add to project", the warning line, and Bounce/Cancel. Under the stems checklist a caption says Ruling 5: "Files ignore mute and solo. Add to project bounces as heard: muted and solo-silenced strips are silent."
- Bounce opens an `NSSavePanel` (file) or an `NSOpenPanel` set to folders only (stems), unless only "Add to project" is on.
- While the job runs, the sheet shows progress and Cancel. On Done it closes; on Failed it alerts "Bounce Failed".
- `AppCommands` adds `Button("Bounce…") { doc?.bounce.shown = true }.disabled(doc == nil)` after "Relink Media…".
- Settings › Audio gains a "Bounce tail limit" stepper, 0–30 s (default 10), stored in `AudioPreferences` and applied through `setBounceSettings`.

Find (AppCommands.swift):
```swift
                Button("Relink Media…") { doc?.media.relinkShown = true }
```
Replace:
```swift
                Button("Bounce…") { doc?.bounce.shown = true }
                    .disabled(doc == nil)
                Button("Relink Media…") { doc?.media.relinkShown = true }
```

- [ ] **Step 1: Write the failing tests** (`BounceModelTests`):
  - `defaultsFollowTheProject`: a 44.1 kHz/16-bit project gives WAV, 44,100, `.int16`, dither on. Switching to `.int24` keeps dither as set, and `.float32` makes dither ignored (the request carries false).
  - `theWarningComesFromTheCore`: `f.checkRow.warning` is shown, and is refetched after `stems` changes.
  - `startBuildsTheRequest`: stems {1, 2} to a folder gives `.stems(strips: [1, 2])` with the path; add-only gives `path: nil, addToProject: true`.
  - `aRefusedStartIsAnAlert`: `bounceError` raises "Bounce Failed".
  - `tailSettingIsKeptAndApplied`: setting the preference to 4 s stores it, and the sink receives `tailCapMs == 4000`; values are clamped to 0–30.

- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.

- [ ] **Step 3: Implement** the model, the sheet, the menu item and the setting.

- [ ] **Step 4: Run the tests and the gates.** Expected: Swift +5.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/BounceModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/AudioPreferences.swift app/Sources/VizDAW/BounceSheet.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/DocumentView.swift app/Sources/VizDAW/AudioSettingsView.swift app/Tests/VizDAWAppModelTests/BounceModelTests.swift
git commit -m "app: File > Bounce... sheet with the external-MIDI warning, progress and cancel; Settings > Audio bounce tail limit (plan 5f-2 Task 6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Swift +5.

---

### Task 7: The bounce e2e, README 180–184, the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`

- [ ] **Step 1: Write the e2e** `bounces_the_demo_and_checks_the_file`:
  1. `app.new_demo_audio`.
  2. Call `file.bounce` with `{"path": "<home>/mix.wav", "bits": "16", "rate": 48000}`.
  3. The answer's `status` starts with "wrote " and names the path.
  4. The file exists and parses as a 16-bit stereo 48 kHz WAV, with a frame count equal to the song's length in frames (from the demo audio project's `state.get document` length in ticks at 120 bpm), plus a tail of at most 10 s. Some sample is non-zero.
  5. A second call with the same path and no `replace` fails (`failed`, "exists"), and the file is unchanged.
  6. With `{"add_to_project": true}`, `state.get document` shows the undo "Bounce" and a lane "Mix", and `state.get media` shows an item with origin `bounced`.
  7. Hostile: `{"format": "mp3", "path": …}` is `invalid_params`; `{}` is the dialog error.

  Use the e2e's throwaway home for paths, and its WAV-reading helper (`audio_file` writes one; add a small reader beside it).

- [ ] **Step 2: Write README 180–184** at the end of `app/README.md`:

```markdown
### Bounce and export (plan 5f-2)

By ear and by eye; the renderer is tested (`core-engine/tests/bounce.rs`),
the files (`core-media/tests/export.rs`), the job (`core-ffi/tests/bounce.rs`)
and the e2e `bounces_the_demo_and_checks_the_file`.

180. File › Bounce…: the sheet defaults to WAV at the project's rate and
     depth (Dither on at 16 bits). Bounce the demo song to a file while it
     plays: playback goes on; the file plays in Finder's preview and
     sounds like the mix, reverb tails included.
181. README 164's check: with a latent plugin on a bus, bounce Stems of
     two tracks and the Mix to files; zoom in on them in another editor
     (or import them back): every file starts at the same sample, a mute
     written at a bar line cuts the Mix exactly there, and the muted
     track's stem file plays on through it (files ignore mute and solo).
182. Add to project: the bounce appears on a new lane "Mix" at the range
     start, in the media pool as Bounced; one Undo removes it. Mute one
     track, solo nothing, and bounce Stems with Add to project: the muted
     track's new lane is silent and the others sound; with a mute written
     at a bar line, its lane cuts there, as the mix does.
183. A project with a lane playing a hardware synth: the sheet says
     "1 lane plays external MIDI and will be silent. Track it as audio
     first: Track Synth…", and the synth is silent in the file.
184. Cancel a long bounce: no file and no ".partial" is left next to
     where it was going; bounce over an existing file: the save panel
     asks to replace it, and the old file stays until the new one is
     complete.
```

- [ ] **Step 3: Run the full gates**, including the rt-check gates, the workspace tests, Swift, `scripts/make-app.sh` and the e2e. Expected: Rust baseline + 13 + 9 + 2 + 12 + 1 = **+37** (macOS); Swift +7; e2e 12; registry 144.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl/tests/e2e.rs app/README.md
git commit -m "e2e: bounce the demo and check the file; README 180-184 (plan 5f-2 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -4
```

---

## Self-Review

1. **Spec coverage.**
   - §9.1:
     - offline render on a worker (the job) thread with the streamer synchronous: Tasks 2 and 4;
     - plugins offline (AU and CLAP): Task 3;
     - Mix, Stems and Selection: Tasks 2, 4 and 5;
     - range and tail with the 10 s Setting: Tasks 2 and 6;
     - the external-MIDI warning with the spec's sentence: Tasks 4 and 6;
     - live playback beside, with separate copies: Task 4.
   - §9.2:
     - WAV, AIFF and CAF at 16, 24 and float; TPDF dither, default on at 16 bits; any converter rate: Task 1 (Ruling 1 explains "through the converter");
     - the sheet defaults: Task 6;
     - a chosen file, with no dialog programmatically: Tasks 4–5;
     - Add to project as `Bounced`, one step: Task 4;
     - progress and cancel, with the partial deleted: Tasks 1, 4 and 6.
   - §12.1 `bounce`: Task 5 (Ruling 2's ID).
   - §12.3:
     - "bounce equals an offline render" (Tasks 2 and 4);
     - "dither changes only the LSBs" (Task 1);
     - "the tail stops at the threshold" (Task 2);
     - "Add to project places the region correctly" (Task 4);
     - "cancel removes the partial file" (Tasks 1 and 4);
     - the e2e (Task 7).
   - No gaps.
2. **Step scan.** Every step names its files, its signatures and the spec values. The test bodies for Tasks 3–6 are given as named tests with their assertions in prose, because they follow those files' existing harnesses (`clap_host.rs`, `FakeSession`).
3. **Type consistency.**
   - `Container` (core-media) maps to `ContainerRow` (core-ffi), and `BitDepth` to the existing `BitDepthRow`.
   - `BounceSpec.stems` (files) and `BounceSpec.heard_stems` (Add to project) take `StripId`s from `BounceSourceRow::Stems { strips }` (Ruling 5).
   - `Work::Bounce.exported` holds `BouncedFileRow` (Task 4), which `ControlJobs.answer(…, bounce:)` reads (Task 5).
   - `ImportAs` is used by both import and bounce.
4. **Review Focus.** Each of the five lines names its tests in its owning task.
5. **Proportion.** About 1,100 lines for spec §9's 25 lines plus the tests. The format layouts and the alignment rule are the only algorithm text; the rest is signatures and tests.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-02-bounce-export.md`. The controller runs it subagent-driven after 5f-1 under the standing authorization. Tasks 1–3 are independent of each other; Task 4 joins them, and its review should replay the cancel and replace paths.
