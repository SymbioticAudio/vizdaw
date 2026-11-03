# VizDAW 5f-2 — Bounce and Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** File › Bounce… renders the song or the time selection faster than real time, as a mix or as stems (each chosen strip post-fader), with a tail that rings out to −96 dBFS. It writes WAV, AIFF or CAF at 16 or 24 bits (TPDF dither, on by default at 16 bits) or 32-bit float, at any rate from 8 to 384 kHz. Optionally it places the result back in the project (in the project format) on a new lane at the range start, as one undo step. Live playback keeps running, with plugin copies in offline mode, while the bounce renders. A cancelled or failed bounce leaves no partial file, and the channel's `file.bounce` does all of it with no dialog.

**Architecture:**
- **`core-media`.** `export`: pure-Rust WAV, AIFF(-C) and CAF writers that share the CAF writer's TPDF quantizer (dither switchable, at 16 and 24 bits), plus `write_partial()` and `export()`. `write_partial()` decodes a float render through the `MediaImporter` (whose macOS ExtAudioFile importer converts the rate), quantizes and writes `.<name>.partial` beside the destination; the `PendingExport` it returns is renamed into place by `commit()` (replacing only when asked) or `commit_numbered()` (never replacing: " 2", " 3", …), and removed when dropped uncommitted. `export()` is the two together.
- **`core-engine`.** `bounce`: drives a dedicated `Engine` and `OfflineBackend` on the calling (job) thread, with the disk serviced before every block (`DiskMode::Manual`). New supporting pieces:
  - **per-run stem taps in the renderer** (pre-flight C1): `Renderer::set_stem_taps` allocates, before the render, one capture per tapped strip (master always); in every completed run the renderer copies each tapped strip's post-fader output into its capture at the run's offset (`Graph::copy_strip_post`, `// RT`, allocation-free) and records the strip's audibility for the run; `Renderer::stem_taps` reads the whole block back between blocks. The mix is read from master's tap, before the master's hard clamp (Ruling 13);
  - each source (mix, stems, heard stems) is aligned by its strip's arrival in the PDC plan, so stems and mix line up with each other and with what is heard; a heard stem is gated by the audibility of every strip on its way to master (Ruling 5);
  - the tail rule, the cap, cancel and progress; the sink receives exactly the final frames of every source (pre-flight I8);
  - `SilentMidi`, a sink-factory wrapper that silences every MIDI output (external MIDI is silent in a bounce, on every platform; pre-flight I3);
  - `Engine::tick_frame` and the `ring=<frames>` test insert.
- **`core-host` / `platform-macos`.**
  - `PluginInstance::set_offline`: CLAP's `render` extension and AU's `kAudioUnitProperty_OfflineRender`.
  - `PluginHost::set_offline_render` and `PluginHost::offline_state`, so a bounce host's copies render offline and the job can name those that cannot.
- **`core-ffi`.**
  - `bounce`: process-wide bounce settings (tail cap); the request, check and result rows; the plugin copies (a second `PluginHost` per bounce, made and serviced on the main thread, from the document captured as Save captures it, and always retired when the job ends, is cancelled, panics or its session closes).
  - The `Bounce` job: render to temporary float CAFs in `Media/.incoming`, import them for "Add to project" (as `Bounced` media with regions on new lanes, using the import path, generalised), write every export to its `.partial`, and rename the exports last (Ruling 12).
- **`core-control`, Swift, `vizctl`.** `file.bounce`, the Bounce sheet (File › Bounce…), the external-MIDI warning, progress and cancel, Settings › Audio "Bounce tail limit" (applied at launch), the e2e "bounce and check the file", README 180–184 and one backlog row (Broadcast-WAV metadata).

**Tech Stack:** Rust 2024 (`core-media`, `core-engine`, `core-host`, `platform-macos`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`, `test-plugins`), Swift 6 / SwiftPM (Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5f-2 — Bounce and export" (split from the former 5f row by plan 5f-1 Ruling 1). This plan delivers:
- §9.1 the offline render (plugins switched to offline rendering), the sources (Mix, Stems, Selection), the range and tail, the external-MIDI warning, and live playback running beside;
- §9.2 the formats and dither, the destinations (a chosen file; "Add to project" as one undo step), and progress and cancel with no partial file;
- §12.1 `bounce { source, range, format, bits, rate, dither, path, add_to_project }`, which waits for completion and returns the file info (as `file.bounce`, Ruling 2);
- §12.3 the bounce tests and the e2e "bounce and check the file".

Read the whole spec (its §3 rules bind here), plan 5a's jobs and import rulings (`docs/superpowers/plans/2026-10-18-audio-playback.md`), and 5d-1 Ruling 2 (region leads) in `docs/superpowers/plans/2026-10-26-monitoring-engine.md`.

**Revision.** This text folds in the pre-flight at `49865db` (2 Critical, 12 Important, 20 Minor; every binding change adopted) and the owner's answers of 2026-11-03 (below), which win where they differ from the pre-flight.

## Owner answers (2026-11-03, binding)

1. **Float bounces keep overs.** A 32-bit float bounce keeps samples above 0 dBFS: the mix is read before the master's hard clamp. 16- and 24-bit files clip, and the clip count is reported (Ruling 13).
2. **Stems never replace.** Stems written into a folder that already holds same-named files are numbered (" 2", " 3", …); nothing in the folder is ever replaced, so there is no replace prompt for the stems folder. The single-file mix keeps the save panel's replace rule, with the old file kept until the new one is complete (Ruling 3).
3. **Bounce in place is as heard, through the routing.** "Add to project" of a track routed into a muted or solo-silenced bus is silent, as heard: a heard stem respects mute and solo along its whole way to master. Export to files ignores mute and solo (Ruling 5).
4. **Own writers.** VizDAW's own WAV, AIFF and CAF writers are accepted; ExtAudioFile is used only for the rate change. Broadcast-WAV (`bext`) metadata is a backlog row (Ruling 1, Task 7).

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root:** `scripts/build-ffi.sh` (after any Rust change that touches the FFI, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test` and `scripts/make-app.sh`. Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` after plan 5f-1 (base `49865db`). Never switch branches, never push.
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
    - With `force_offline()` (and `VIZDAW_AUDIO=offline`), the bounce engine's sinks are `SilentMidi(OfflineSinks)`, never `MacSinks`, so no CoreMIDI client and no DLS unit is made (Ruling 8).
    - Plugin copies in tests are the repo's CLAP test plugins (`test-plugins`). AU offline mode is tested on Apple's built-in delay in `platform-macos`'s own tests, as 5e-1 did.
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any device. Bounce destinations in tests are inside `tempfile` folders; core-ffi test projects are saved packages inside `tempfile` folders, opened with `Session::open` and given `Session::debug_set_trash_dir`. Swift tests use `MemoryDefaults` and `FakeSession`.
- **Test waits.** Every `core-ffi` integration test that waits for a job or the transport polls with a deadline from `common::wait_limit()` (`crates/core-ffi/tests/common/mod.rs`, which reads `VIZDAW_TEST_WAIT_SECS`), calling `s.idle()` between polls (the bounce's plugin copies are serviced there). There are no fixed sleeps for results. The e2e polls with its `WAIT_MS` conditions.
- **Crates and dependencies.**
  - Rust stable, edition 2024, license GPL-3.0-only.
  - No new crate and no new dependency. The `render` feature of the existing `clack-extensions` is enabled in `core-host` and `test-plugins` (Ruling 7).
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. New non-test code uses `.get()`, iterators and `checked_*`/`saturating_*`. Functions with more than seven arguments carry `#[allow(clippy::too_many_arguments)]`, as `import_reserved` does.
- **Platform-neutral cores.** `core-media`, `core-engine`, `core-model` and `core-control` stay free of OS APIs. The WAV, AIFF and CAF writers and `SilentMidi` are pure Rust (Rulings 1, 8).
- **Linux CI.**
  - CI runs `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on `ubuntu-latest`.
  - The writers, dither, `export()` (without a rate change), the bounce renderer, its taps and `SilentMidi`, and every core-ffi bounce test that does not change the rate run everywhere.
  - macOS-only code: AU offline mode, the `MacSinks` choice for live bounces in core-ffi (`#[cfg(target_os = "macos")]`, with the note "// macOS only: the DLS synth; other platforms bounce through the offline sinks"), and the ExtAudioFile cross-checks in `crates/platform-macos/tests/media.rs` (that crate is macOS-only).
  - A `core-ffi` test that needs a rate change goes through `ExtAudioImporter`, so it is `#[cfg(target_os = "macos")]` with the note "// macOS only: rate conversion goes through ExtAudioFile (the CAF importer converts depth only)", as in `crates/core-ffi/tests/media_pool.rs`; its Linux twin is `#[cfg(not(target_os = "macos"))]`.
  - Every task that changes Rust checks `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **Real-time rules (spec §3).**
  - Nothing new runs on a device's real-time thread.
  - The bounce renderer is a separate `Renderer` driven by `OfflineBackend::block` on the job thread. `Renderer::process` changes in two places (pre-flight C1): the stem taps are reset at each block's start and filled in every completed run. Both are `// RT`: they write into buffers `set_stem_taps` allocated and never allocate; with no taps (every live engine) they do nothing.
  - `Renderer::set_stem_taps` and `Renderer::stem_taps` are control-side, between blocks, `// not RT`.
  - The new `TestMode::Ring` arm and `SilentMidi`'s `MidiOut::send` are `// RT` (they allocate nothing).
  - **rt-check gates** (Tasks 2 and 7): `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures --test rt_alloc_bounce`. `rt_alloc_bounce` (Task 2) renders with taps set and an automation lane that splits every block into runs, armed.
- **Media is write-once.**
  - Bounce writes new files only: exports to `.<name>.partial`, then renames (Ruling 3); "Add to project" through the import path into new names in `Media/`; temporaries in `Media/.incoming/`, which retention already cleans.
  - It never writes into an existing media file, and never writes inside the open project's package or scratch root except through the import path (Ruling 3 refuses such destinations).
  - An existing destination is replaced only for a single-file mix whose call says `replace` (the save panel's confirmation), and only by a rename after the new file is complete. Stems never replace (owner answer 2). A file renamed into place is never removed afterwards, by the job, by `discard` or at session close (Ruling 12).
- **File compatibility.** `core_model::file::VERSION` stays 3. Bounce adds no model field; the `Bounced` origin exists since 5a.
- **FFI safety.**
  - No panic crosses the FFI. Every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`.
  - The exceptions are the infallible `bounce_settings` and `set_bounce_settings`. Each body runs inside `std::panic::catch_unwind` and answers the defaults or nothing on a panic.
- **UniFFI changes** (Task 4):
  - New: `BounceSourceRow`, `BounceRangeRow`, `ContainerRow`, `BounceRequestRow`, `BounceCheckRow`, `BouncedFileRow`, `BounceResultRow`, `BounceSettingsRow`; `JobKindRow::Bounce`.
  - Free functions: `bounce_settings`, `set_bounce_settings`.
  - `Session` methods: `bounce`, `bounce_check`, `bounce_result`; test hook `debug_bounce_cancel_at`.
  - `FakeSession` implements each new method in Task 4. Swift switches over `JobKindRow` (`ProjectMedia.swift`) gain `.bounce` in Task 4.
- **Swift.** Generated bindings compile in Swift 5 mode, and the app targets and tests in Swift 6 mode with no new warnings. New model tests go to `VizDAWAppModelTests`.
- **Control channel.**
  - Programmatic calls never open dialogs. `file.bounce` without `path` and without `add_to_project` fails before the app with the dialog error (`DialogSpec` "Bounce", `always`, answered by `path` or `add_to_project`). `{add_to_project: false}` alone passes that presence check and is refused by the app with the same dialog error, and by the core with `Model` (pre-flight M6).
  - The registry count moves from **144** (after 5f-1) to **145** in Task 5, with "; 5f-2 adds file.bounce" appended to the count test's sentence.
  - `file.bounce` joins `SurfaceRelay.dialogCommands` (it opens the Bounce sheet from a surface).
  - Hostile-input tests: `bounce_commands_check_their_params` (core-control), `bounceCommandsCheckTheirArguments` (app), `hostile_bounce_requests_are_refused` (core), `hostile_exports_are_refused` (core-media).
- **Generated files** are never committed (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`).
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the Linux clippy, and `cargo test` for the crates touched (the workspace in Tasks 3, 4 and 7).
  - Tasks 4–7 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 7 runs `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests are as in plan 5f-1. Re-run once before investigating, and say so in the report.
  - **Baseline** (measured after the 5f-1 residual fix `49865db`): **Rust 1802 passed / 15 ignored (macOS), Swift 933, e2e 11, registry 144.** Each task's last step gives the counts it adds; report the measured counts.
- **How code is given.** As in plan 5f-1: whole new files where their text matters (every new test file is given whole), then Find/Replace excerpts, each occurring once in the named file at `49865db` (or after the earlier steps of this plan). Where a name differs from the tree, use the tree's and say so.
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

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out, most likely first. Each is pinned by named tests in its task, and a reviewer should replay each pin's failing half (what the test would see if the code were wrong).

1. **A bounce that does not line up with what was heard.** A track behind a latent plugin on another bus (PDC leads), a stem of a bus fed by sends, automation that splits blocks into runs, a range end that is not a block multiple, and the mix, all bounced together.
   - Expected: every file starts at the range start as heard. Stems and the mix line up to the sample over the whole file, with the latency neither cut nor padded, and every run of every block is in every stem (pre-flight C1). The stems sum to the mix when master is at 0 dB with no inserts. A bounce equals a plain offline render of the same document.
   - Pinned by `the_mix_is_the_region_as_heard_with_and_without_latency`, `stems_line_up_and_add_up_to_the_mix` (range end at 7.5 beats, a fader ramp, a send) and `a_bounce_equals_a_plain_offline_render` (Task 2), `rt_alloc_bounce` (Task 2), and `a_bounce_equals_an_offline_render_of_the_range` (Task 4).
2. **The tail cuts a reverb short, or never ends.** Silence after the range; a plugin that rings; a plugin that hisses forever; a tail cap of 0.
   - Expected:
     - with silence, the file is exactly the range;
     - a ring is kept until it is below −96 dBFS, and the file ends at its last loud frame;
     - endless noise stops at the cap;
     - a cap of 0 gives exactly the range;
     - every file of one bounce, and every item "Add to project" makes, has the same length: the sink receives exactly `frames` frames per source (pre-flight I8).
   - Pinned by `a_silent_end_adds_no_tail`, `a_ring_is_kept_to_its_last_loud_frame`, `endless_noise_stops_at_the_cap`, `a_zero_cap_is_the_range` (Task 2, whose `run` helper asserts the exact length of every source), and `add_to_project_places_the_bounce_on_a_new_lane` (Task 4: the item's length equals the file's frames).
3. **A destination that is not what it should be.** The path exists already; it names a folder, or a folder that does not exist; it lies inside the open project; its extension does not match the format; a stem name repeats, is taken on disk, starts with a dot or has a `/`; the disk is full; the bounce is cancelled at 90 %; a later step fails after the file was written.
   - Expected:
     - for a mix without `replace`, an existing file is refused before anything is rendered;
     - for a mix with `replace`, the old file stays byte-identical until the new one is complete, also when the bounce is cancelled at 90 %;
     - stems never replace: a name taken on disk or in this bounce is numbered (owner answer 2);
     - a wrong extension, a folder, a missing folder or a place inside the project is refused with a clear message;
     - a cancel or failure leaves no `.partial`, no temporary CAF, and no media item; a file already renamed into place is never deleted (pre-flight I1).
   - Pinned by `an_existing_file_is_refused_unless_replaced`, `replace_keeps_the_old_file_until_the_new_one_is_whole` (cancel from `progress` past 0.5), `cancel_and_failure_leave_no_partial_file`, `a_written_export_is_renamed_only_by_commit`, `numbered_commits_never_replace` (Task 1), and `stem_files_get_safe_unique_names`, `cancel_removes_everything_the_bounce_wrote`, `a_late_cancel_keeps_the_replaced_file`, `a_failed_import_keeps_the_exported_file` and `hostile_bounce_requests_are_refused` (Task 4).
4. **Plugins in the bounce differ from playback, or playback stops.** A plugin with unsaved knob changes, a plugin that fails to load as a copy, a bounce started while playing, a second bounce while one runs, a bounce cancelled or failed while its copies are loaded.
   - Expected:
     - the copies take the live state (captured as Save captures it);
     - a copy that fails renders as a pass-through, and the job says which plugin;
     - live playback goes on, and its engine is not reloaded;
     - a second bounce is refused (Busy) while one runs;
     - the copies are retired however the job ends (pre-flight I2).
   - Pinned by `copies_render_offline_with_the_live_state` (a live 0.5 halves the bounce; 0 silences it), `a_copy_that_fails_is_a_pass_through_and_named`, `a_bounce_while_playing_keeps_playback_running`, `one_bounce_at_a_time` and `cancel_removes_everything_the_bounce_wrote` (`plugins::bounce_hosts() == 0`) (Task 4), and `clap_copies_are_told_to_render_offline` and `a_bounce_host_renders_its_copies_offline` (Task 3).
5. **Dither, depth and level surprises.** 16 bits with dither off, 24 bits with dither on, float with dither on, a mix that clips both ways, a float bounce of a hot mix, a rate the CAF importer cannot convert on Linux.
   - Expected:
     - dither changes only the lowest bit, at 16 and 24 bits;
     - float ignores dither;
     - a float file keeps samples above 0 dBFS (owner answer 1); integer files clip, and the clipped samples (positive and negative) are counted and reported in the job's problems ("12 samples clipped") while the bounce answers ok (pre-flight I7);
     - a rate change without a system converter fails the job with a clear message, never a silent wrong-rate file.
   - Pinned by `dither_changes_only_the_lowest_bit`, `float_ignores_dither`, `clipping_is_counted` (Task 1), `a_hot_mix_keeps_its_overs_before_the_clamp` (Task 2), `clipped_samples_are_reported` and `a_bounce_at_another_rate_converts` / `a_rate_change_without_the_system_converter_fails_cleanly` (Task 4), and `bounceStartsAJobAndAnswersWithTheFiles` (Task 5, the warning answer).

## Plan Rulings

Each ruling decides something the spec leaves open, in the format "what — why — cost if wrong".

1. **Ruling (writers in Rust, rate conversion by the importer): the containers, quantization and dither are pure Rust (`core_media::export`); only a rate change goes through `MediaImporter::decode` at the target rate.** Accepted by the owner (answer 4).
   - The spec reads "written through `MediaImporter`'s converter on macOS (`ExtAudioFile` with a float client format …)". On macOS that is ExtAudioFile's converter (`kAudioConverterQuality_Max`, Mastering complexity, `platform-macos/src/media.rs`); on Linux the CAF importer refuses a rate change.
   - Quantization (TPDF from the CAF writer's generator, at 16 bits and, when asked, at 24 bits) and the WAV, AIFF-C and CAF layouts are written by VizDAW.
   - Why:
     - "dither changes only the least significant bits" and "a bounce equals an offline render" are provable only with our own quantizer, since ExtAudioFile's float-to-int conversion does not dither;
     - pure writers run on Linux CI;
     - no new unsafe FFI is needed.
   - Cost if wrong: no Broadcast-WAV (`bext`) metadata. That is a backlog row (Task 7); a `bext` chunk can be added to the WAV writer later behind the same `PcmWriter` API.
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
   - The bounced sequence is the window's shown sequence (`DocumentModel.selectedSeq`); there is no `seq` parameter.
   - Why: the spec names the command `bounce`, but the registry rule predates it.
   - Cost if wrong: a rename.
3. **Ruling (destinations):**
   - For a mix, `path` is the file and its extension must be the container's (`.wav`, `.aif`/`.aiff`, `.caf`; `export` checks it too, pre-flight M19).
   - For stems, `path` is an existing folder. Each stem becomes `<folder>/<strip name>.<ext>`:
     - `/`, `:` and control characters become `-`, and a leading `.` becomes `-` (no hidden files);
     - an empty name becomes `Strip <id>`;
     - the name is cut (at a character boundary) so that `.<name> NN.<ext>.partial` stays within 255 bytes;
     - a name taken in this bounce **or on disk** gets ` 2`, ` 3`, … (owner answer 2): stems never replace, so `replace` is ignored for stems and the sheet asks nothing. At the rename, `commit_numbered` takes the next free number if a file appeared meanwhile.
   - Nothing is rendered while a destination is refused:
     - the path is not absolute, has a `..` component, or is longer than 1024 bytes;
     - the parent folder (for stems, the folder) is missing;
     - the canonicalised parent lies inside the session's package or its scratch root: "choose a place outside the project" (pre-flight I4);
     - for a mix: the extension is wrong; the path is an existing folder; the file exists and `replace` is false ("<name> exists; choose another name or replace it");
     - for stems: the path is a file.
   - The export writes `<dir>/.<name>.partial` and renames it onto the destination when complete (replacing only for a mix with `replace`). A cancel or an error before the rename removes the partial.
   - "Add to project" alone (no `path`) writes no outside file.
   - Why: §9.2's "a chosen file", the write-once rule for everything VizDAW writes, and the owner's answer.
   - Cost if wrong: a stems folder layout the owner does not like (a naming change).
4. **Ruling (alignment and length):**
   - The bounce plays the range with `Engine::locate_tick(start)` and `play_until(end)`.
   - Every source is read from a stem tap (pre-flight C1): the mix from master's tap, which drops its first `Graph::latency()` frames; stem `s` drops its first `Graph::strip_arrival(s)` frames. Both are counted from the first block that starts at the range start while playing.
   - Every file then holds `range + tail` frames:
     - `range` is `tick_frame(end) − tick_frame(start)` (the tempo map's own frames: a ritardando is in it);
     - `tail` ends at the last frame where any source is at or above −96 dBFS (1.5849e-5 linear), once all sources have stayed below it for 0.5 s;
     - `tail` is at most the cap (Setting "Bounce tail limit", 0–30 s, default 10 s).
   - Rendering stops when the quiet run reaches 0.5 s or the cap is reached.
   - **The sink receives exactly `frames` frames per source** (pre-flight I8). `bounce` holds back the frames it has not decided yet (the quiet frames after the last loud one, and a heard stem's frames whose gates are not known yet) in a buffer per source of at most `TAIL_QUIET_SECONDS × rate + block + max arrival` frames, hands on the frames that are final, and drops the held quiet frames at the end. So the temporary CAFs, the exported files and the imported items all have the same length.
   - Why: §9.1's tail rule, with one length for all files of a bounce, so stems line up end to end.
   - Cost if wrong: if the owner wants each stem trimmed to its own tail, compute per source.
5. **Ruling (stems: export to files ignores mute and solo; bounce in place respects them, through the routing).** Owner answers 2026-11-02 (this ruling) and 2026-11-03 (answer 3, the routing).
   - **Export to files** (`path`): a stem is the strip node's post-fader output (`st.post`: inserts, fader and pan), before mute and solo, as an OMF-style export hands every track over. Mute and solo are edge gains in the graph (plan 5e-1 controller ruling), so a muted strip's stem file still sounds and a soloed strip does not silence the other stem files. A bus stem holds what reaches the bus.
   - **Bounce in place** ("Add to project"): each imported stem is that post-fader output gated by its audibility as the mix hears it, along its whole main-output chain: the strip itself, each bus its main output feeds in turn, and master (`HeardStem::of(mixer, strip)`). A strip that is muted, solo-silenced, or routed into a muted or solo-silenced bus bounces silent, and a mute written at a bar line on any strip of the chain cuts the imported stem exactly where it cuts the mix. (Sends are not part of the chain: a heard stem is the strip's own output as its main path hears it.)
   - The Mix is always as heard (master's tap gated by master's own audibility, exactly what `write_master` writes); mute and solo are in it either way, so one Mix source serves both the file and the import.
   - With both a `path` and "Add to project", one pass renders each chosen strip twice: `BounceSpec.stems` (the files) and `BounceSpec.heard_stems` (the imports).
   - **How the gates line up.** The renderer records, for every tapped strip, its graph `audible` at the start of every run (after `automate` committed the run's controls), as `(run offset, audible)` per change (plan 4d Ruling 11: audibility switches only at run boundaries). Strip `j`'s mute and solo act at its main edge, after that edge's delay line, so the gate recorded at renderer frame `r` acts on timeline frame `r − feed_arrival(j, None)`. A heard stem's sample at timeline frame `t` (counted after its `strip_arrival` skip) is therefore the file stem's sample times the product, over every strip `j` of its chain, of `j`'s gate at renderer frame `t + feed_arrival(j, None)` (master: `t + latency`). `bounce` keeps each chain strip's gate changes in absolute renderer frames and hands a heard frame on once the render has passed `t + max_j feed_arrival(j)`.
   - Why: the owner's answers. A bounce in place replaces what the user hears in the project, so it must sound as heard; an export hands raw stems to another program, which makes its own mix.
   - Cost if wrong: heard stems to files, or raw stems in place, become a sheet option; rendering both kinds in one pass costs one more stem buffer per chosen strip.
6. **Ruling (plugin copies on the main thread): `Session::bounce` makes the copies on the session's plugin thread (the main thread in the app).**
   1. It first captures the live states into the document exactly as Save does (`capture_all`). Inside an open edit (`begin_edit`) that is refused: `Busy` "Finish the open edit first." (pre-flight M13).
   2. It builds a new `PluginHost` at the project rate and the bounce block (1024) with `set_offline_render(true)` and syncs it to that document. A session used from a thread without its plugin host refuses a document with plugin slots (`Model` "bounce plugins from the window's thread").
   3. The job thread waits, polling every 5 ms, until `plugins::tick_bounces()` reports no pending loads. That function is called from `Session::idle`. The wait gives up after 60 s, and slots still loading render as pass-through placeholders.
   4. The job thread builds its engine with the copies' `ShelfFactory`. A slot the copies could not load (still loading, failed, or unknown) is a pass-through placeholder; its plugin name goes to the job's problems ("<name> could not load for the bounce and was bypassed"), as does every copy that refused offline mode (Ruling 7: "<name> renders in real time only").
   5. The session records the copies as `bounce_copies: Option<(job, token)>`. `apply_jobs` calls `plugins::end_bounce(token)` as soon as the bounce job is no longer pending, **whatever it ended as**: done, failed, cancelled or panicked (pre-flight I2). `Session::close` and a dropped session (`plugins::close_session`) end it too. `end_bounce` empties the host and moves it to the existing `closing` list, which waits for its processors.
   - A document with no plugin slots needs no main thread and makes no copies.
   - One bounce at a time **per session**: a bounce while another bounce of this session is unapplied is `Busy`. Two documents may bounce at once (pre-flight M14).
   - Why:
     - `PluginHost` is not `Send` (plan 2b Ruling 1);
     - the spec's "separate copies restored from the saved state";
     - one at a time keeps memory bounded.
   - Cost if wrong: a plugin that takes more than 60 s to load is a placeholder in the bounce (reported).
7. **Ruling (offline mode in plugins): `PluginInstance::set_offline(&mut self, on: bool) -> bool` defaults to `false`, meaning not supported.**
   - CLAP calls `clap_plugin_render.set(CLAP_RENDER_OFFLINE)` when the plugin offers the extension, and answers false (renders in real time) when the plugin reports `has_hard_realtime_requirement`. The `render` feature of `clack-extensions` 0.2 is enabled in `core-host`, and in `test-plugins` for the gain plugin's implementation.
   - AU sets `kAudioUnitProperty_OfflineRender` (37) to 1 on the global scope.
   - The bounce host applies it to every instance before its first `activate`, and records the answer: `PluginHost::offline_state(id) -> Option<bool>` (a real API, not a test hook; pre-flight M3). The bounce lists the slots that answered false in its problems.
   - Why: §9.1. A feature flag on a crate already in the lock is not a new dependency.
   - Cost if wrong: a plugin that renders differently offline (higher quality) sounds different in the bounce, which is the spec's intent.
8. **Ruling (sinks for the bounce engine): `core_engine::bounce::SilentMidi<F: SinkFactory>` wraps the sinks (pre-flight I3).**
   - It answers the wrapped factory's `ports()` (so destinations resolve as in playback) and makes every `MidiDestination` and `MidiVirtualSource` target a `Sink::Midi` that drops every message (`// RT`). The built-in synth and plugin instruments are the wrapped factory's.
   - Offline hosts (`host::offline_mode()`, i.e. tests and the e2e) wrap `OfflineSinks`, exactly as the offline host makes them. Live builds on macOS wrap `platform_macos::sinks::MacSinks` (the DLS synth); other platforms wrap `OfflineSinks` (pre-flight M9).
   - The warning counts the distinct lanes (by `(sequence, lane)` of the flattened range) whose notes resolve to an external MIDI destination (`DestKind::MidiEndpoint`).
   - `bounce_check` also says "<n> regions are offline and will be silent" for regions in the range whose media is offline (pre-flight M5), and "The bounce needs about <n> MB of temporary space; <m> MB are free" when the estimate (float, stereo, per source, doubled with "Add to project") exceeds `core_media::record::free_bytes` of the media root (pre-flight M20). The sentences are joined with a space in `warning`.
   - Why: §9.1. External hardware must not play during a bounce, on any platform, and the silence is testable at engine level.
   - Cost if wrong: none expected; a bounce never sends MIDI.
9. **Ruling ("Add to project"):**
   - Each bounced source (the mix, or each heard stem) is imported from its temporary float CAF into `Media/`, in the project format (16-bit dithered when the project is 16-bit, as every import), with origin `Bounced` and the name of the file stem (or "Mix" and the strip names when there is no `path`). Its length is the bounce's `frames` (Ruling 4).
   - Each gets a new lane at the end of the bounced sequence, named like its media item, with a region at the range start.
   - All of it is one undo step "Bounce".
   - This reuses the import's apply, generalised to a label, a lane name and an origin.
   - A bounce with "Add to project" is a media job: it waits for, and holds off, Format, Convert and Relink jobs (`media_jobs_idle`), so it never imports at a format that is being changed (pre-flight I9).
   - Why: §9.2. The import's apply already does lanes, regions, overviews and the failure clean-up.
   - Cost if wrong: an owner who wants the bounce on the selected lane moves the region.
10. **Ruling (the result): `Session::bounce_result(job) -> Option<BounceResultRow>` returns `files` (path, frames, rate, bits, channels, clipped) and `media` (items added), for a bounce that wrote files or media (Done, or Failed after its renames).**
    - The channel's answer status is `"wrote <path> (<frames> frames, <rate> Hz, <bits>)"` per file, joined by `; `, plus `"; added <n> media"` when it added any, plus `"; warnings: <problems joined by '; '>"` when the job has problems.
    - `ControlJobs.answer(_ j: JobRow, bounce: BounceResultRow? = nil)` (pre-flight M10): a Done bounce answers `.ok` with that status even when it created no media and has problems (clipped samples, placeholders), unlike an import (pre-flight I7). The app's own job polling uses the same answer.
    - Why: §12.1's "returns the file info". `JobRow` stays unchanged.
    - Cost if wrong: one more field in the answer.
11. **Ruling (formats in the sheet):**
    - Defaults: WAV at the project's rate and depth, with dither on when the depth is 16.
    - Rates: the six project rates in the popup, plus "Other…" for any rate from 8,000 to 384,000 Hz.
    - Bits: 16, 24 and 32 float. Dither applies to 16 and 24 bits, and is shown disabled for float.
    - Why: §9.2.
    - Cost if wrong: a popup's contents.
12. **Ruling (the job's order and what survives it; pre-flight I1, M17):**
    1. render every source to a temporary float CAF in `Media/.incoming/` (progress 0–0.8);
    2. "Add to project" imports into `Media/` (0.8–0.9): new files the job can still remove;
    3. every export written to its `.partial` (`write_partial`, 0.9–1.0);
    4. all renames last (`commit` for the mix, `commit_numbered` for stems), with cancel checked only before the first rename.
    - On cancel or an error before step 4, everything is removed: partials (dropping a `PendingExport` removes it), temporaries and imported media. An error during step 4 keeps the files already renamed and removes the rest.
    - A file renamed into place is never removed afterwards: not by the job, not by `discard` (which removes only files inside the package), not at session close. If the later apply of "Add to project" fails (its sequence was deleted meanwhile), the job is Failed, its imported media are removed, and the exported files stay, named in `bounce_result`.
    - Why: the user's chosen file (and, with `replace`, the one it replaced) must never be lost to a step after it.
    - Cost if wrong: none; the order costs nothing.
13. **Ruling (the output level; owner answer 1, pre-flight I6):**
    - The Mix source is master's post-fader output from its tap, **before** the master's hard clamp at ±1.0 (`clamp_master`): a 32-bit float file keeps samples above 0 dBFS.
    - 16- and 24-bit files clip in the writer, which counts every clipped sample of either sign; the job's problems say "<file>: <n> samples clipped".
    - Non-finite samples (a faulting plugin) become 0 in `bounce`; their count is `BounceStats.nonfinite` and goes to the problems ("<n> non-finite samples were silenced").
    - Why: the owner's answer; a float bounce is for further processing, where overs are recoverable.
    - Cost if wrong: a float file that plays louder than full scale in a player that does not clip; one clamp in `bounce` makes it the speakers' level.

## Carried items

- No carry from 5e names bounce. README 164 says "Zoom in on a bounce (plan 5f) to check to the sample"; README 181 (Task 7) completes it.

## File map

| File | Task | Role |
|---|---|---|
| `crates/core-media/src/export.rs`, `src/caf.rs`, `src/import.rs`, `src/lib.rs`, `tests/export.rs`, `crates/platform-macos/tests/media.rs` | 1 | writers, quantizer, `write_partial`/`export`, `PendingExport` |
| `crates/core-engine/src/bounce.rs`, `src/engine.rs`, `src/render.rs`, `src/graph/exec.rs`, `src/graph/node.rs`, `src/lib.rs`, `tests/bounce.rs`, `tests/rt_alloc_bounce.rs` | 2 | stem taps, the bounce renderer, `SilentMidi` |
| `crates/core-host/{Cargo.toml,src/instance.rs,src/host.rs,src/clap/handlers.rs,src/clap/instance.rs,tests/clap_host.rs,tests/host.rs}`, `crates/test-plugins/{Cargo.toml,src/plugin.rs}`, `crates/platform-macos/src/au/{mod.rs,ffi.rs}`, `crates/platform-macos/tests/au_host.rs` | 3 | offline mode in plugins |
| `crates/core-ffi/src/bounce.rs`, `src/jobs.rs`, `src/plugins.rs`, `src/session.rs`, `src/lib.rs`, `tests/bounce.rs`; `app/Sources/VizDAWAppModel/ProjectMedia.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift` | 4 | the bounce job |
| `crates/core-control/src/registry.rs`, `tests/commands.rs`; `app/Sources/VizDAWAppModel/{BounceCommands.swift,ControlCommands.swift,SurfaceRelay.swift}`, `app/Sources/VizDAW/ControlChannel.swift`, `app/Tests/VizDAWAppModelTests/BounceCommandsTests.swift` | 5 | channel |
| `app/Sources/VizDAWAppModel/{BounceModel.swift,DocumentModel.swift,AudioPreferences.swift}`, `app/Sources/VizDAW/{BounceSheet.swift,AppCommands.swift,DocumentView.swift,AudioSettingsView.swift,AudioSetup.swift}`, `app/Tests/VizDAWAppModelTests/{BounceModelTests.swift,AudioPreferencesTests.swift}` | 6 | sheet, menu, setting |
| `crates/vizctl/tests/e2e.rs`, `app/README.md`, `docs/feature-backlog.md` | 7 | e2e, README, backlog |

---

### Task 1: `core-media` export: WAV, AIFF and CAF writers, dither, `write_partial()` and `export()`

**Files:**
- Create: `crates/core-media/src/export.rs`, `crates/core-media/tests/export.rs`
- Modify: `crates/core-media/src/lib.rs`, `crates/core-media/src/caf.rs` (make `Tpdf` `pub(crate)`; `CafWriter::set_dither`, dither at 24 bits when on), `crates/core-media/src/import.rs` (make `rename_excl` `pub(crate)`, both cfg variants), `crates/platform-macos/tests/media.rs`

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
  /// A complete `.partial` waiting for its rename (Ruling 12). Dropping it
  /// uncommitted removes the partial.
  #[derive(Debug)] pub struct PendingExport { … }
  impl PendingExport {
      /// What the partial holds (`path` is the destination).
      pub fn exported(&self) -> &Exported;
      /// Renames the partial onto the destination: `rename` (replacing) when
      /// `write_partial` was given `replace`, else `rename_excl`, which fails
      /// with "exists" (and removes the partial) if the destination appeared.
      pub fn commit(self) -> Result<Exported, MediaError>;
      /// Renames the partial onto the destination or, when that name is taken,
      /// onto "<stem> 2.<ext>", "<stem> 3.<ext>", … (never replacing; owner
      /// answer 2); `Exported.path` is the name it took.
      pub fn commit_numbered(self) -> Result<Exported, MediaError>;
  }
  /// `source` (a VizDAW float CAF) → `.<name>.partial` beside `dest` in
  /// `fmt`: at most `frames` source frames (scaled by the rate ratio).
  /// Refuses before writing anything (see Step 3). A cancel or error removes
  /// the partial.
  #[allow(clippy::too_many_arguments)]
  pub fn write_partial(importer: &dyn MediaImporter, source: &Path, dest: &Path, fmt: &ExportFormat,
      frames: Option<u64>, replace: bool, progress: &mut dyn FnMut(f32), cancel: &AtomicBool)
      -> Result<PendingExport, MediaError>;
  /// `write_partial` then `commit`.
  #[allow(clippy::too_many_arguments)]
  pub fn export(importer: &dyn MediaImporter, source: &Path, dest: &Path, fmt: &ExportFormat,
      frames: Option<u64>, replace: bool, progress: &mut dyn FnMut(f32), cancel: &AtomicBool)
      -> Result<Exported, MediaError>;
  /// The partial file `write_partial` writes for `dest`.
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

Quantization is `CafWriter`'s: `(v × scale + dither).round()` clamped to the integer range, where `dither` is `Tpdf::next()` when dither is on (16 or 24 bits) and 0 when off. A sample whose undithered `v × scale` lies outside the range counts as clipped (either sign: −2.0 counts, −1.0 does not). Non-finite samples are written as 0. The same `Tpdf` seed is used, so a WAV and a CAF of one signal hold the same integers. `CafWriter`'s 24-bit branch gains the dither term when `set_dither(true)` (pre-flight M1); its default stays as it is (on at 16 bits only), so imports and recordings do not change.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-media/tests/export.rs <<'VIZDAW_EOF'
//! Bounce files (plan 5f-2 Task 1; spec §9.2, §12.3; Rulings 1, 3, 12, 13).

use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_media::export::{
    Container, ExportFormat, PcmWriter, export, partial_path, write_partial,
};
use core_media::import::CafImporter;
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Every file in `dir`, by name.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
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

/// At 16 and 24 bits (pre-flight M1: the 24-bit branch dithers when asked).
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

/// Both signs count (Ruling 13); a float file keeps the overs.
#[test]
fn clipping_is_counted() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("loud.wav");
    let mut w = PcmWriter::create(&p, &fmt(Container::Wav, BitDepth::Int16, false), 1).unwrap();
    w.write(&[0.5, 1.5, -2.0, 1.0, -1.0]).unwrap();
    assert_eq!(w.finish().unwrap().clipped, 3, "1.0 is one past the largest 16-bit value; -1.0 fits");
    let f = d.path().join("loud float.wav");
    let mut w = PcmWriter::create(&f, &fmt(Container::Wav, BitDepth::Float32, false), 1).unwrap();
    w.write(&[0.5, 1.5, -2.0]).unwrap();
    assert_eq!(w.finish().unwrap().clipped, 0);
    let (.., s) = read_wav(&f);
    assert_eq!(f32::from_bits(s[1] as u32), 1.5, "a float file keeps samples above 0 dBFS");
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

/// Review Focus 3: a cancel once more than half is written, over an
/// existing file with `replace`, leaves the old file byte-identical.
#[test]
fn replace_keeps_the_old_file_until_the_new_one_is_whole() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 80_000);
    let dest = d.path().join("old.wav");
    std::fs::write(&dest, b"old").unwrap();
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();
    let e = export(&CafImporter, &src, &dest, &fmt(Container::Wav, BitDepth::Int16, true), None, true,
        &mut |p| {
            seen.push(p);
            if p > 0.5 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel);
    assert!(matches!(e, Err(core_media::MediaError::Cancelled)), "{e:?}");
    assert!(seen.iter().any(|p| *p > 0.5 && *p < 1.0), "cancelled mid-write: {seen:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"old", "a cancelled export leaves the old file");
    assert!(!partial_path(&dest).exists());
    assert_eq!(names(d.path()), ["old.wav", "render.caf"]);
}

#[test]
fn cancel_and_failure_leave_no_partial_file() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 80_000);
    let dest = d.path().join("never.wav");
    // A cancel after the first chunk.
    let cancel = AtomicBool::new(false);
    let mut chunks = 0;
    let e = export(&CafImporter, &src, &dest, &fmt(Container::Wav, BitDepth::Int16, true), None, false,
        &mut |_| {
            chunks += 1;
            cancel.store(true, Ordering::Relaxed);
        },
        &cancel);
    assert!(matches!(e, Err(core_media::MediaError::Cancelled)));
    assert!(chunks >= 1, "the partial was being written");
    assert!(!dest.exists() && !partial_path(&dest).exists());
    assert_eq!(names(d.path()), ["render.caf"]);
    // A failure: the partial cannot be created (a folder holds its name).
    std::fs::create_dir(partial_path(&dest)).unwrap();
    assert!(run(&src, &dest, &fmt(Container::Wav, BitDepth::Int16, true), false).is_err());
    assert!(!dest.exists());
    assert!(partial_path(&dest).is_dir(), "never removes what it did not write");
}

/// Ruling 12 (pre-flight I1): the destination is touched only by `commit`;
/// a dropped pending export leaves nothing.
#[test]
fn a_written_export_is_renamed_only_by_commit() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 2000);
    let dest = d.path().join("mix.wav");
    std::fs::write(&dest, b"old").unwrap();
    let f = fmt(Container::Wav, BitDepth::Int24, false);
    let p = write_partial(&CafImporter, &src, &dest, &f, None, true, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(p.exported().frames, 2000);
    assert!(partial_path(&dest).is_file());
    assert_eq!(std::fs::read(&dest).unwrap(), b"old", "written, not yet renamed");
    drop(p);
    assert!(!partial_path(&dest).exists(), "a dropped pending export removes its partial");
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    let p = write_partial(&CafImporter, &src, &dest, &f, None, true, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    let e = p.commit().unwrap();
    assert_eq!((e.path.clone(), read_wav(&dest).4), (dest.clone(), 2000));
    assert_eq!(names(d.path()), ["mix.wav", "render.caf"]);
    // Without `replace`, a destination that appeared meanwhile is refused at the rename.
    let late = d.path().join("late.wav");
    let p = write_partial(&CafImporter, &src, &late, &f, None, false, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    std::fs::write(&late, b"someone else's").unwrap();
    assert!(p.commit().unwrap_err().to_string().contains("exists"));
    assert_eq!(std::fs::read(&late).unwrap(), b"someone else's");
    assert!(!partial_path(&late).exists());
}

/// Owner answer 2: stems never replace; a taken name is numbered.
#[test]
fn numbered_commits_never_replace() {
    let d = tempfile::tempdir().unwrap();
    let src = float_caf(d.path(), 100);
    let dest = d.path().join("Bass.wav");
    std::fs::write(&dest, b"keep").unwrap();
    std::fs::write(d.path().join("Bass 2.wav"), b"keep 2").unwrap();
    let f = fmt(Container::Wav, BitDepth::Int16, true);
    let p = write_partial(&CafImporter, &src, &dest, &f, None, false, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    let e = p.commit_numbered().unwrap();
    assert_eq!(e.path, d.path().join("Bass 3.wav"));
    assert_eq!(std::fs::read(&dest).unwrap(), b"keep");
    assert_eq!(std::fs::read(d.path().join("Bass 2.wav")).unwrap(), b"keep 2");
    assert_eq!(read_wav(&e.path).4, 100);
    assert_eq!(names(d.path()), ["Bass 2.wav", "Bass 3.wav", "Bass.wav", "render.caf"]);
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
    // The extension must be the container's (pre-flight M19).
    let e = run(&src, &d.path().join("x.caf"), &fmt(Container::Wav, BitDepth::Int16, true), false).unwrap_err();
    assert!(e.to_string().contains(".wav"), "{e}");
    // A destination that is a folder.
    std::fs::create_dir(d.path().join("folder.wav")).unwrap();
    assert!(run(&src, &d.path().join("folder.wav"), &fmt(Container::Wav, BitDepth::Int16, true), true).is_err());
    assert!(d.path().join("folder.wav").is_dir());
    // A source that is not a VizDAW CAF.
    std::fs::write(d.path().join("junk.caf"), b"junk").unwrap();
    assert!(run(&d.path().join("junk.caf"), &d.path().join("j.wav"), &fmt(Container::Wav, BitDepth::Int16, true), false).is_err());
    assert!(Container::Aiff.accepts("AIFF") && Container::Aiff.accepts("aif") && !Container::Wav.accepts("caf"));
    let other_rate = ExportFormat { rate: 44_100, ..fmt(Container::Wav, BitDepth::Int16, true) };
    let e = run(&src, &d.path().join("rate.wav"), &other_rate, false).unwrap_err();
    assert!(e.to_string().contains("system importer"), "the CAF importer converts depth only: {e}");
    assert!(!d.path().join("rate.wav").exists() && !partial_path(&d.path().join("rate.wav")).exists());
    assert_eq!(names(d.path()), ["folder.wav", "junk.caf", "render.caf"]);
}
VIZDAW_EOF
```

  Then append to `crates/platform-macos/tests/media.rs` (macOS-only crate; its existing imports `CafReader`, `MediaImporter`, `BitDepth`, `ExtAudioImporter`, `Path` and `AtomicBool` are used):

```rust
/// A 48 kHz stereo float CAF of a ramp, `frames` frames, in `dir`.
fn render_caf(dir: &Path, frames: usize) -> std::path::PathBuf {
    let p = dir.join("render.caf");
    let mut w = core_media::caf::CafWriter::create(
        &p,
        48_000,
        2,
        BitDepth::Float32,
        core_media::caf::WriteMode::Normal,
    )
    .unwrap();
    let data: Vec<f32> = (0..frames)
        .flat_map(|f| {
            let x = ((f % 97) as f32 / 97.0) - 0.5;
            [x * 0.9, -x * 0.5]
        })
        .collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
    p
}

/// Every frame of `path` decoded at 48 kHz by the system reader.
fn decode_all(path: &Path) -> Vec<f32> {
    let mut out = Vec::new();
    ExtAudioImporter
        .decode(
            path,
            48_000,
            &mut |c| {
                out.extend_from_slice(c);
                Ok(())
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    out
}

/// Plan 5f-2 Task 1: the system reader reads every WAV and AIFF VizDAW
/// writes, with its channels, rate, bits and frames, and the samples
/// within one step of the source (exact for float).
#[test]
fn the_system_reader_reads_every_export() {
    use core_media::export::{Container, ExportFormat, export};
    let d = tempfile::tempdir().unwrap();
    let src = render_caf(d.path(), 4800);
    let want = decode_all(&src);
    for (c, depth) in [
        (Container::Wav, BitDepth::Int16),
        (Container::Wav, BitDepth::Int24),
        (Container::Wav, BitDepth::Float32),
        (Container::Aiff, BitDepth::Int16),
        (Container::Aiff, BitDepth::Int24),
        (Container::Aiff, BitDepth::Float32),
    ] {
        let dest = d.path().join(format!("out {depth:?}.{}", c.extension()));
        let f = ExportFormat { container: c, depth, rate: 48_000, dither: false };
        export(&ExtAudioImporter, &src, &dest, &f, None, false, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let info = ExtAudioImporter.probe(&dest).unwrap();
        assert_eq!(
            (info.channels, info.sample_rate, info.bits, info.frames),
            (2, 48_000.0, depth.bits(), 4800),
            "{c:?} {depth:?}"
        );
        let got = decode_all(&dest);
        let step = match depth {
            BitDepth::Int16 => 1.0 / 32_768.0,
            BitDepth::Int24 => 1.0 / 8_388_608.0,
            BitDepth::Float32 => 0.0,
        };
        assert_eq!(got.len(), want.len());
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!((g - w).abs() <= step * 1.01, "{c:?} {depth:?} sample {i}: {g} vs {w}");
        }
    }
}

/// Plan 5f-2 Task 1: a rate change goes through ExtAudioFile.
#[test]
fn export_resamples_48k_to_44k1() {
    use core_media::export::{Container, ExportFormat, export};
    let d = tempfile::tempdir().unwrap();
    let src = render_caf(d.path(), 48_000);
    let dest = d.path().join("44k1.wav");
    let f = ExportFormat { container: Container::Wav, depth: BitDepth::Int24, rate: 44_100, dither: false };
    let e = export(&ExtAudioImporter, &src, &dest, &f, None, false, &mut |_| {}, &AtomicBool::new(false)).unwrap();
    assert!(e.frames.abs_diff(44_100) <= 1, "{}", e.frames);
    let info = ExtAudioImporter.probe(&dest).unwrap();
    assert_eq!(info.sample_rate, 44_100.0);
    assert!(info.frames.abs_diff(44_100) <= 1);
}
```

  (If `SourceInfo`'s field names or `BitDepth::bits()` differ in the tree, use the tree's and say so; `CafImporter::probe` shows them.)

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-media --test export`; `cargo test -p platform-macos --test media`). Expected: FAIL — `core_media::export` not found.

- [ ] **Step 3: Implement `export.rs`** and `CafWriter::set_dither`, make `Tpdf` `pub(crate)` and `rename_excl` `pub(crate)` (both of its cfg variants in `import.rs`). In `crates/core-media/src/lib.rs`:

   Find:
   ```rust
   pub mod caf;
   ```
   Replace:
   ```rust
   pub mod caf;
   pub mod export;
   ```

   `write_partial()` refuses (before writing anything) when:
   - the format fails `check`;
   - `dest`'s extension is not one `fmt.container.accepts` ("<name> must end in .<ext> for <container>");
   - `dest`'s parent is not an existing folder;
   - `dest` is an existing folder;
   - `dest` exists and `replace` is false (`MediaError::Unsupported("<name> exists; choose another name or replace it")`);
   - the source is not a VizDAW CAF (`CafReader::open` fails).

   It decodes through `importer.decode(source, fmt.rate, …)` and writes with `PcmWriter` to `partial_path(dest)` (`<dir>/.<file name>.partial`, created exclusively). After every decoded chunk it calls `progress(written / expected)` (expected: `frames` scaled by `fmt.rate / source rate`, else the source's frames scaled) and returns `Cancelled` when `cancel` is set. On any error or cancel it removes the partial it created (never a path it did not create). `PendingExport` holds the partial, the destination, `replace` and the `Exported`; its `Drop` removes an uncommitted partial. `commit` uses `std::fs::rename` with `replace`, else `rename_excl`. `commit_numbered` tries `dest`, then `"<stem> <n>.<ext>"` for n = 2, 3, … (up to 9999) with `rename_excl`, taking the first that succeeds. `export` is `write_partial(…)?.commit()`.

- [ ] **Step 4: Run the tests and the gates.** Expected: core-media +12, platform-macos +2.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-media/src/export.rs crates/core-media/src/lib.rs crates/core-media/src/caf.rs crates/core-media/src/import.rs crates/core-media/tests/export.rs crates/platform-macos/tests/media.rs
git commit -m "core-media: WAV, AIFF and CAF export with switchable TPDF dither at 16 and 24 bits, written to a partial and renamed only on commit, numbered commits that never replace (plan 5f-2 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Rust +14 on macOS (core-media 12, platform-macos 2); +12 on Linux.

---

### Task 2: `core-engine` bounce: per-run stem taps, sources aligned by arrival, the tail, cancel, `SilentMidi`

**Files:**
- Create: `crates/core-engine/src/bounce.rs`, `crates/core-engine/tests/bounce.rs`, `crates/core-engine/tests/rt_alloc_bounce.rs`
- Modify: `crates/core-engine/src/lib.rs`, `crates/core-engine/src/engine.rs` (`tick_frame`), `crates/core-engine/src/render.rs` (the stem taps), `crates/core-engine/src/graph/exec.rs` (`strip_nodes`, `strip_node`, `copy_strip_post`, `node_audible`), `crates/core-engine/src/graph/node.rs` (`TestMode::Ring`)

**Interfaces:**
- Consumes: `Engine::{locate_tick, play_until, set_loop, streamer, state}`, `OfflineBackend::{block, renderer_mut}`, `Graph::{strip_arrival, feed_arrival, latency}`, `SinkFactory`, `MidiOut`.
- Produces:
  ```rust
  // bounce.rs
  /// −96 dBFS (Ruling 4).
  pub const TAIL_THRESHOLD: f32 = 1.584_893_2e-5;
  /// The quiet run that ends a tail.
  pub const TAIL_QUIET_SECONDS: f64 = 0.5;
  /// A stem gated as heard (Add to project, Ruling 5): `strip`'s output times
  /// the audibility of every strip in `chain` (the strip, each bus its main
  /// output feeds in turn, then master).
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct HeardStem { pub strip: StripId, pub chain: Vec<StripId> }
  impl HeardStem {
      /// `strip` and its main-output chain in `mixer` (cycle-safe: at most
      /// one visit per strip; an unknown strip's chain is itself and master).
      pub fn of(mixer: &Mixer, strip: StripId) -> HeardStem;
  }
  #[derive(Debug, Clone, PartialEq)]
  pub struct BounceSpec {
      /// Render the mix (source 0 when set).
      pub mix: bool,
      /// Strips whose post-fader output is a stem, before mute and solo
      /// (export to files, Ruling 5), in order (after the mix).
      pub stems: Vec<StripId>,
      /// Stems gated as heard (Add to project, Ruling 5), in order (after `stems`).
      pub heard_stems: Vec<HeardStem>,
      pub start: Tick, pub end: Tick,
      /// The longest tail, in frames.
      pub tail_cap: u64,
  }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct BounceStats {
      /// Frames every source's sink received (range + tail).
      pub frames: u64, pub range: u64, pub tail: u64,
      /// Stream underruns while rendering (`Engine::faults` counters); 0 in Manual mode unless a read failed.
      pub underruns: u64,
      /// Non-finite samples silenced (Ruling 13).
      pub nonfinite: u64,
  }
  #[derive(Debug, thiserror::Error)] pub enum BounceError { #[error("cancelled")] Cancelled, #[error("{0}")] Engine(#[from] EngineError), #[error("{0}")] Sink(String), #[error("the range is empty")] Empty }
  /// Renders `spec` with `engine`, whose renderer `out` runs, handing each source's
  /// stereo frames to `sink(source, left, right)` in order. The sink receives
  /// exactly `BounceStats::frames` frames per source (Ruling 4). Sets the
  /// renderer's stem taps before the first block and clears them at the end.
  pub fn bounce(engine: &mut Engine, out: &mut OfflineBackend, spec: &BounceSpec, block: u32,
      sink: &mut dyn FnMut(usize, &[f32], &[f32]) -> Result<(), String>,
      progress: &mut dyn FnMut(f32), cancel: &AtomicBool) -> Result<BounceStats, BounceError>;
  /// Wraps a sink factory so that no MIDI leaves a bounce (Ruling 8): MIDI
  /// destinations and the virtual source get a sink that drops every message.
  #[derive(Debug)] pub struct SilentMidi<F>(pub F);
  impl<F: SinkFactory> SinkFactory for SilentMidi<F> { … }
  // engine.rs
  impl Engine { /// The frame of `tick` in the loaded sequence (0 before a load).
      pub fn tick_frame(&self, tick: Tick) -> u64; }
  // render.rs
  /// One tapped strip's capture of the last block (pre-flight C1).
  pub struct StemTap<'a> {
      pub strip: StripId,
      /// Every frame of the block, from every run (zeros where no graph ran).
      pub left: &'a [f32], pub right: &'a [f32],
      /// `(first frame, audible)` per change through the block, the first
      /// at the first rendered run (empty: no run rendered, silent).
      pub gates: &'a [(u32, bool)],
      /// `Graph::strip_arrival` (master: `Graph::latency`): where the post-fader signal is read.
      pub arrival: u64,
      /// `Graph::feed_arrival(strip, None)`: where its mute and solo act.
      pub gate_arrival: u64,
  }
  impl Renderer {
      /// Taps on `strips` plus master (deduplicated): allocates, per tap,
      /// `max_frames` per channel and `max_frames / MIN_RUN + 2` gate entries.
      /// An empty list removes the taps. Call between blocks. // not RT
      pub fn set_stem_taps(&mut self, strips: &[StripId]);
      /// The taps' capture of the last block, master first. Between blocks. // not RT
      pub fn stem_taps(&self) -> Vec<StemTap<'_>>;
  }
  // graph/exec.rs
  impl Graph {
      /// The node of `strip` (`strip_nodes`, sorted, built in `build` beside `strip_arrive`). // RT
      pub(crate) fn strip_node(&self, strip: StripId) -> Option<usize>;
      /// Copies frames `0..frames` of `node`'s post-fader output into the slices
      /// (after a completed run, before the next; the `write_master` protocol). // RT
      pub(crate) fn copy_strip_post(&self, node: usize, left: &mut [f32], right: &mut [f32], frames: u32);
      /// `node`'s `audible` as the last commit left it (false for a non-strip node). // RT
      pub(crate) fn node_audible(&self, node: usize) -> bool;
  }
  // graph/node.rs
  TestMode::Ring(u32)  // id key `ring=<frames>`: passes input; after it falls silent, outputs 0.25 for that many frames
  ```

**The stem taps (pre-flight C1).** `Renderer::process_inner` loops over runs, and each run's graph buffers hold only that run, so a strip's `post` read after the block holds only the last run. The taps capture every run instead:
1. `Renderer` gains `stems: Option<Box<StemTaps>>` (None in `new`). `set_stem_taps` (not RT) builds it: master and the given strips, deduplicated, each with `left`/`right` of `cfg.max_frames` and `gates` with capacity `cfg.max_frames / MIN_RUN + 2`. Strips are resolved to nodes lazily, per run, with `Graph::strip_node` (a binary search: the graph may arrive after the taps are set).
2. At the start of each block (before the run loop), `begin_block(frames)` zero-fills `0..frames` of each tap and clears its gates (no deallocation). // RT
3. In `render_graph`'s `RunResult::Done` arm, after `write_master`, `capture(g, at, frames)` copies each tap's post into `tap[at..at + frames]` with `Graph::copy_strip_post` (its `// SAFETY:` is `write_master`'s: after `Done`, before the next run) and pushes `(at, node_audible(node))` when it differs from the last entry (or is the first). A run the graph skips (no graph yet, the `max_frames` guard) leaves zeros and no gate. // RT
4. `stem_taps` (not RT) answers each tap's block with the current graph's `strip_arrival` / `feed_arrival` (master: `latency` for both).
5. No live engine sets taps, so playback does nothing new. `rt_alloc_bounce` proves the capture allocation-free under rt-check.

In `render.rs`, the per-block reset:

Find:
```rust
        let mut missed = false;
        let mut offset = 0u32;
```
Replace:
```rust
        let mut missed = false;
        // A bounce's stem taps start the block afresh (plan 5f-2, pre-flight C1). // RT
        if let Some(t) = self.stems.as_deref_mut() {
            t.begin_block(frames);
        }
        let mut offset = 0u32;
```

and the per-run capture:

Find:
```rust
            RunResult::Done => {
                g.write_master(out, at, frames);
```
Replace:
```rust
            RunResult::Done => {
                g.write_master(out, at, frames);
                // Every run of every tapped strip, before the master clamp
                // (plan 5f-2 Rulings 5, 13; pre-flight C1). // RT
                if let Some(t) = self.stems.as_deref_mut() {
                    t.capture(g, at, frames);
                }
```

(`g` borrows `self.graph` and `t` borrows `self.stems`: disjoint fields.)

**The algorithm of `bounce`** (Rulings 4, 5, 13):
1. Refuse `end <= start` with `Empty`.
2. `range = tick_frame(end) − tick_frame(start)`.
3. `out.renderer_mut().set_stem_taps(…)`: the `stems`, every heard stem's strip and chain, and master. Then `locate_tick(start)`, `set_loop(false)` and `play_until(end)`.
4. Per block: check `cancel`; `engine.streamer().service()`; `out.block(block)`; read `stem_taps()`. Start counting at the first block after which `engine.state()` is playing; let `n` be the renderer frames counted so far (block starts after that point).
5. Per tap, append the block's gate changes to its history in absolute frames (`n + offset`; a block with no gates counts as not audible from its start). The history is a `Vec` on the job thread.
6. Sources: source 0 is the mix (master's tap, skip `latency`, gated by master's own history at `t + latency`); then each file stem (its tap, skip `arrival`, ungated); then each heard stem (its tap, skip `arrival`, gated by every chain strip's history at `t + gate_arrival(j)`). A source's frame `t` is ready once `n` has passed `t + skip` and, for a gated source, `t + max gate arrival`. Non-finite samples become 0 and count in `nonfinite`.
7. Ready frames before `range` are handed to the sink at once. After `range`, track `quiet` (consecutive frames where every source's samples are all below `TAIL_THRESHOLD` in magnitude) and `last_loud`: frames up to `last_loud` are handed on; quiet frames after it are held (in a per-source buffer preallocated at `TAIL_QUIET_SECONDS × rate + block + max skip` frames) until a loud frame makes them final or the tail ends.
8. Stop when `quiet ≥ TAIL_QUIET_SECONDS × rate` or `tail_rendered ≥ tail_cap`. `frames = range + min(last_loud_after_range, tail_cap)`; the held frames past it are dropped.
9. Clear the taps (`set_stem_taps(&[])`).
10. `progress` is `handed / (range + tail_cap)`, rising to 1 at the end.

`SilentMidi<F>`: `ports()` is `self.0.ports()`; `make` gives `TargetKind::MidiDestination(_)` and `TargetKind::MidiVirtualSource` a `Sink::Midi(Box::new(Dropped))` (`struct Dropped; impl MidiOut for Dropped { fn send(&mut self, _: u64, _: &[u8]) -> bool { true } }`, `// RT`) and hands every other kind to `self.0.make`.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/bounce.rs <<'VIZDAW_EOF'
//! The bounce renderer (plan 5f-2 Task 2; spec §9.1, §12.3; Rulings 4, 5,
//! 8, 13; pre-flight C1, I3, I6, I8).

use core_engine::backend::{AudioBackend, MidiPortInfo};
use core_engine::bounce::{BounceSpec, BounceStats, HeardStem, SilentMidi, TAIL_QUIET_SECONDS, bounce};
use core_engine::engine::{Engine, EngineConfig, SinkFactory};
use core_engine::fixtures::{Builder, Q};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile};
use core_model::automation::AutomationTarget;
use core_model::command::MixerCmd;
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade};
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId, StripId};
use core_model::mixer::{AuxSend, Output, StripKind};
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

#[derive(Default)]
struct Opts {
    /// A bus "Latent" with a `latency=` insert (PDC leads on the region's path).
    latency: u32,
    /// An insert on master.
    master: Option<&'static str>,
    /// A post-fader send from the region's path to "Latent".
    send: bool,
    /// A fader ramp on the region's path from beat 1 to beat 5 (a point every
    /// eighth beat: runs split inside blocks), held after.
    ramp: bool,
    /// A written mute of the region's path at this tick.
    mute_at: Option<i64>,
    /// Notes on the built-in synth (its test tone).
    synth: bool,
}

/// One region at beat 1 (frames 100.. of the file) for 6 beats, plus `o`.
fn doc_with(o: Opts) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FRAMES, 2).unwrap();
    b.region(seq, lane, Q, m, 100, 6 * QF, 0.0, Fade::default(), Fade::default()).unwrap();
    let path = path_and_bus(b.doc()).0;
    if o.latency > 0 {
        let bus = b.bus("Latent").unwrap();
        b.insert(bus, TestInserts::plugin(&format!("latency={}", o.latency))).unwrap();
        if o.send {
            b.mixer(MixerCmd::AddSend { strip: path, send: AuxSend::new(bus) }).unwrap();
        }
    }
    if let Some(p) = o.master {
        let ms = b.strip_of(StripKind::Master).unwrap();
        b.insert(ms, TestInserts::plugin(p)).unwrap();
    }
    let mut auto = Vec::new();
    if o.ramp {
        for k in 8..=40i64 {
            auto.push(EventSpec::new(
                Tick(k * Q / 8),
                EventBody::Automation { target: AutomationTarget::StripFader(path), value: 0.5 + (k % 8) as f64 / 16.0 },
            ));
        }
    }
    if let Some(t) = o.mute_at {
        for (tick, v) in [(0, 0.0), (t, 1.0)] {
            auto.push(EventSpec::new(Tick(tick), EventBody::Automation { target: AutomationTarget::StripMute(path), value: v }));
        }
    }
    if !auto.is_empty() {
        let l = b.lane(seq, "Auto", None).unwrap();
        b.events(seq, l, auto).unwrap();
    }
    if o.synth {
        let d = b.dest("Synth", "builtin:synth", 0).unwrap();
        let l = b.lane(seq, "Keys", Some(d)).unwrap();
        b.notes(seq, l, &[(Q, 60, 0x8000, 4 * Q), (2 * Q, 67, 0x8000, 2 * Q)]).unwrap();
    }
    (b.finish(), seq)
}

fn doc(latency: u32, master: Option<&'static str>) -> (Document, SeqId) {
    doc_with(Opts { latency, master, ..Opts::default() })
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    _dir: tempfile::TempDir,
}

fn rig_with(doc: &Document, seq: SeqId, sinks: Box<dyn SinkFactory>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    let (mut engine, renderer) = Engine::new(cfg, sinks).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.set_media(vec![MediaFile { id: MediaId(0), path: dir.path().join("a.caf") }]);
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    Rig { engine, out, _dir: dir }
}

fn rig(doc: &Document, seq: SeqId) -> Rig {
    rig_with(doc, seq, Box::new(OfflineSinks::new(64)))
}

/// Every source's left and right channels; the sink must receive exactly
/// `stats.frames` frames per source (Ruling 4, pre-flight I8).
fn run_lr(r: &mut Rig, spec: &BounceSpec) -> (Vec<(Vec<f32>, Vec<f32>)>, BounceStats) {
    let sources = usize::from(spec.mix) + spec.stems.len() + spec.heard_stems.len();
    let mut got = vec![(Vec::new(), Vec::new()); sources];
    let stats = bounce(&mut r.engine, &mut r.out, spec, BLOCK, &mut |k, l, rt| {
        got[k].0.extend_from_slice(l);
        got[k].1.extend_from_slice(rt);
        Ok(())
    }, &mut |_| {}, &AtomicBool::new(false))
    .unwrap();
    for (k, g) in got.iter().enumerate() {
        assert_eq!(g.0.len() as u64, stats.frames, "source {k}: exactly the bounce's frames");
        assert_eq!(g.1.len() as u64, stats.frames);
    }
    (got, stats)
}

/// The left channels.
fn run(r: &mut Rig, spec: &BounceSpec) -> (Vec<Vec<f32>>, BounceStats) {
    let (got, stats) = run_lr(r, spec);
    (got.into_iter().map(|g| g.0).collect(), stats)
}

fn spec(end_beats: i64) -> BounceSpec {
    BounceSpec { mix: true, stems: vec![], heard_stems: vec![], start: Tick::ZERO, end: Tick(end_beats * Q), tail_cap: 480_000 }
}

/// The region's AudioPath strip and (once added) the "Latent" bus.
fn path_and_bus(d: &Document) -> (StripId, StripId) {
    let find = |want: fn(&StripKind) -> bool| {
        d.mixer().order().iter().copied().find(|s| d.mixer().strip(*s).is_ok_and(|x| want(&x.kind)))
    };
    (
        find(|k| matches!(k, StripKind::AudioPath(_))).unwrap(),
        find(|k| matches!(k, StripKind::Bus)).unwrap_or(StripId(u64::MAX)),
    )
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

/// Pre-flight C1: the range ends mid-block (7.5 beats), a fader ramp splits
/// runs inside blocks, and a send feeds the latent bus; every frame of the
/// whole file lines up.
#[test]
fn stems_line_up_and_add_up_to_the_mix() {
    let (d, seq) = doc_with(Opts { latency: 1000, send: true, ramp: true, ..Opts::default() });
    let (path, bus) = path_and_bus(&d);
    let mut r = rig(&d, seq);
    let end = BounceSpec { stems: vec![path, bus], end: Tick(15 * Q / 2), ..spec(0) };
    let (got, stats) = run(&mut r, &end);
    assert_eq!(stats.range, 15 * QF / 2);
    assert_ne!(stats.range % u64::from(BLOCK), 0, "the range ends inside a block");
    for (k, stem) in got[1..].iter().enumerate() {
        assert!(stem[..QF as usize].iter().all(|x| *x == 0.0), "stem {k} starts at beat 1 too");
        assert!(stem[QF as usize] != 0.0, "stem {k} sounds at beat 1");
    }
    for f in 0..got[0].len() {
        let sum: f32 = got[1..].iter().map(|s| s[f]).sum();
        assert!((sum - got[0][f]).abs() < 1e-5, "frame {f}: stems sum to the mix at 0 dB master");
    }
    // The ramp really moved the fader: the path's stem is not a constant times the media.
    let k0 = got[1][QF as usize] / value(100);
    // Beat 3.5 is the point k = 28 (0.75); beat 1 is k = 8 (0.5).
    let k1 = got[1][(7 * QF / 2) as usize] / value(100 + 5 * QF / 2);
    assert!((k0 - k1).abs() > 1e-3, "{k0} vs {k1}");
}

/// Spec §12.3 "a bounce equals an offline render", at engine level and
/// deterministic (workers 0, deadline None): the mix equals a plain
/// play-and-render of the same document with its first L frames dropped.
#[test]
fn a_bounce_equals_a_plain_offline_render() {
    let (d, seq) = doc_with(Opts { latency: 700, ramp: true, synth: true, ..Opts::default() });
    let end = Tick(15 * Q / 2);
    let mut a = rig_with(&d, seq, Box::new(OfflineSinks::new(1 << 12).with_tone()));
    let (got, stats) = run(&mut a, &BounceSpec { end, ..spec(0) });
    let mut b = rig_with(&d, seq, Box::new(OfflineSinks::new(1 << 12).with_tone()));
    b.engine.play().unwrap();
    let l = b.engine.latency_frames();
    assert_eq!(l, 700);
    let mut plain = Vec::new();
    while (plain.len() as u64) < l + stats.range {
        b.engine.streamer().service();
        b.out.block(BLOCK);
        plain.extend_from_slice(b.out.output(BLOCK).0);
    }
    let plain = &plain[l as usize..(l + stats.range) as usize];
    assert!(plain.iter().any(|x| *x != 0.0));
    assert!(plain.iter().all(|x| x.abs() < 1.0), "below the clamp, so the two compare");
    for (f, (x, y)) in got[0].iter().zip(plain).enumerate() {
        assert_eq!(x, y, "frame {f}");
    }
}

/// Owner answers 2026-11-02 and 2026-11-03 (Ruling 5): a heard stem (Add to
/// project) respects mute and solo along its way to master, also through a
/// muted bus; a file stem does not; unmuted they are equal.
#[test]
fn heard_stems_respect_mute_and_solo_and_file_stems_do_not() {
    for how in ["mute", "solo another strip", "mute the bus it plays into"] {
        let (d, seq) = doc(1000, None);
        let (path, bus) = path_and_bus(&d);
        let mut b = Builder::from_doc(d);
        match how {
            "mute" => b.mixer(MixerCmd::SetMute { strip: path, mute: true }),
            "solo another strip" => b.mixer(MixerCmd::SetSolo { strip: bus, solo: true }),
            _ => {
                b.mixer(MixerCmd::SetOutput { strip: path, output: Output::Bus(bus) }).unwrap();
                b.mixer(MixerCmd::SetMute { strip: bus, mute: true })
            }
        }
        .unwrap();
        let d = b.finish();
        let heard = HeardStem::of(d.mixer(), path);
        if how == "mute the bus it plays into" {
            assert!(heard.chain.contains(&bus), "{heard:?}");
        }
        let mut r = rig(&d, seq);
        let (got, stats) = run(&mut r, &BounceSpec { stems: vec![path], heard_stems: vec![heard], ..spec(8) });
        assert_eq!(stats.frames, 8 * QF, "{how}");
        assert!(got[0].iter().all(|x| *x == 0.0), "{how}: the mix is as heard");
        assert!(got[1][QF as usize] != 0.0, "{how}: the file stem ignores it");
        assert!(got[2].iter().all(|x| *x == 0.0), "{how}: the heard stem is silent");
    }
    let (d, seq) = doc(1000, None);
    let (path, _) = path_and_bus(&d);
    let mut r = rig(&d, seq);
    let heard = HeardStem::of(d.mixer(), path);
    let (got, _) = run(&mut r, &BounceSpec { stems: vec![path], heard_stems: vec![heard], ..spec(8) });
    assert!(got[2][QF as usize] != 0.0);
    assert_eq!(got[1], got[2], "unmuted, the heard stem is the file stem, sample for sample");
}

/// Pre-flight C1: a mute written inside a block cuts the heard stem where
/// it cuts the mix; the file stem plays on.
#[test]
fn a_written_mute_cuts_the_heard_stem_where_it_cuts_the_mix() {
    // 3 beats + 320 ticks = frame 72 500, inside block 141.
    let at = 3 * Q + 320;
    let (d, seq) = doc_with(Opts { latency: 1000, mute_at: Some(at), ..Opts::default() });
    let (path, _) = path_and_bus(&d);
    let mut r = rig(&d, seq);
    let heard = HeardStem::of(d.mixer(), path);
    let (got, _) = run(&mut r, &BounceSpec { stems: vec![path], heard_stems: vec![heard], ..spec(8) });
    let (mix, file, inplace) = (&got[0], &got[1], &got[2]);
    let cut = (QF as usize..mix.len()).find(|f| mix[*f..].iter().all(|x| *x == 0.0)).unwrap();
    assert!(cut.abs_diff(72_500) <= 32, "the mix cuts at the written mute: {cut}");
    assert_ne!(cut % BLOCK as usize, 0, "inside a block");
    assert!(inplace[cut - 1] != 0.0 && inplace[cut..].iter().all(|x| *x == 0.0), "the heard stem cuts at {cut} too");
    assert_eq!(&inplace[..cut], &file[..cut], "and is the file stem before it");
    assert!(file[cut] != 0.0 && file[(6 * QF) as usize] != 0.0, "the file stem plays on");
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

/// Owner answer 1, Ruling 13: the mix is read before the master's hard
/// clamp, both signs.
#[test]
fn a_hot_mix_keeps_its_overs_before_the_clamp() {
    let (d, seq) = doc(0, Some("gain=8"));
    let mut r = rig(&d, seq);
    let (got, stats) = run_lr(&mut r, &spec(7));
    let (l, rt) = &got[0];
    assert!(l.iter().any(|x| *x > 1.0), "positive overs kept");
    assert!(rt.iter().any(|x| *x < -1.0), "negative overs kept");
    assert_eq!(stats.nonfinite, 0);
}

/// Ruling 8 (pre-flight I3): wrapped in `SilentMidi`, a lane on an external
/// MIDI destination sends nothing; the same document unwrapped does.
#[test]
fn external_midi_is_silent_in_a_bounce() {
    let mut b = Builder::new();
    let ext = b.dest("Hardware", "Ext Port", 0).unwrap();
    let (seq, lane) = b.sequence("Song", "Keys", Some(ext)).unwrap();
    b.notes(seq, lane, &[(0, 60, 0x8000, Q), (2 * Q, 64, 0x8000, Q)]).unwrap();
    let d = b.finish();
    let port = || OfflineSinks::new(256).with_ports(vec![MidiPortInfo { name: "Ext Port".into(), unique_id: None }]);
    let plain = port();
    let log = plain.log();
    let mut r = rig_with(&d, seq, Box::new(plain));
    run(&mut r, &spec(4));
    assert!(log.drain().iter().any(|x| x.bytes().first() == Some(&0x90)), "unwrapped, the note-ons go out");
    let silent = port();
    let log = silent.log();
    let mut r = rig_with(&d, seq, Box::new(SilentMidi(silent)));
    let (_, stats) = run(&mut r, &spec(4));
    assert_eq!(stats.range, 4 * QF);
    assert!(log.drain().is_empty(), "nothing reaches the destination");
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
cat > crates/core-engine/tests/rt_alloc_bounce.rs <<'VIZDAW_EOF'
//! A bounce's stem taps never allocate on the render path (plan 5f-2 Task
//! 2, pre-flight C1): the counting allocator is armed only around
//! `OfflineBackend::block`, while taps on the synth, a latent bus and master
//! capture every run of blocks of varying size that a fader lane and a mute
//! lane split into many runs. The taps are read back between blocks,
//! unarmed, as `bounce` does.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig, find_sequence};
use core_engine::fixtures::{Builder, demo_document};
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::rt_check::{self, CountingAlloc};
use core_model::automation::AutomationTarget;
use core_model::command::MixerCmd;
use core_model::event::{EventBody, EventSpec};
use core_model::mixer::{AuxSend, StripKind};
use core_model::time::Tick;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// One test, so no other test thread allocates while the global counter
/// is compared.
#[test]
fn stem_taps_never_allocate() {
    let before = rt_check::count();
    rt_check::arm();
    drop(Vec::<u8>::with_capacity(10));
    rt_check::disarm();
    assert_eq!(rt_check::count() - before, 2, "the counter works on this thread");

    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bus = b.bus("Latent").unwrap();
    b.insert(bus, TestInserts::plugin("latency=300")).unwrap();
    b.mixer(MixerCmd::AddSend { strip: synth, send: AuxSend::new(bus) }).unwrap();
    let lane = b.lane(song, "Auto", None).unwrap();
    let q = 15_360i64;
    let mut events = Vec::new();
    // A fader point every 64 ticks (100 frames) and a mute toggled every beat.
    for k in 0..(16 * q / 64) {
        events.push(EventSpec::new(
            Tick(k * 64),
            EventBody::Automation { target: AutomationTarget::StripFader(synth), value: (k % 50) as f64 / 50.0 },
        ));
    }
    for k in 0..16 {
        events.push(EventSpec::new(
            Tick(k * q),
            EventBody::Automation { target: AutomationTarget::StripMute(synth), value: (k % 2) as f64 },
        ));
    }
    b.events(song, lane, events).unwrap();
    let doc = b.finish();
    let sinks = OfflineSinks::new(1 << 20).with_tone();
    let log = sinks.log();
    let (mut engine, renderer) = Engine::new(EngineConfig::new(48_000.0), Box::new(sinks)).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(&doc, song).unwrap();
    let mut out = OfflineBackend::new(48_000.0, 512);
    out.start(renderer).unwrap();
    out.reserve(4096);
    out.renderer_mut().unwrap().set_stem_taps(&[synth, bus]);
    engine.play().unwrap();
    let before = rt_check::count();
    let (mut most_gates, mut loud, mut taps) = (0usize, false, 0usize);
    for i in 0..3000u32 {
        rt_check::arm();
        out.block(1 + (i * 37) % 1024);
        rt_check::disarm();
        let r = out.renderer_mut().unwrap();
        let t = r.stem_taps();
        taps = t.len();
        for tap in &t {
            most_gates = most_gates.max(tap.gates.len());
            loud |= tap.left.iter().any(|x| *x != 0.0);
        }
        if i % 100 == 0 {
            log.drain();
        }
    }
    assert_eq!(rt_check::count() - before, 0, "allocations while the taps captured");
    assert_eq!(taps, 3, "synth, bus and master");
    assert!(loud, "the taps captured the synth");
    assert!(most_gates >= 2, "a mute toggled inside a block was recorded per run");
    drop(out.stop().unwrap());
    drop(engine);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-engine --test bounce --test rt_alloc_bounce`). Expected: FAIL — `core_engine::bounce` and `set_stem_taps` not found.

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

   In `graph/exec.rs`, `Graph` gains `strip_nodes: Box<[(StripId, usize)]>`, sorted, built in `build` beside `strip_arrive`, and `strip_node`, `copy_strip_post` (the `write_master` pattern: `// SAFETY:` after `Exec::run` returned `Done`, before another run) and `node_audible`. In `render.rs`, `StemTaps` (private: a `Vec` of taps, each `strip`, `left`, `right`, `gates`), `begin_block` and `capture` (both `// RT`), the two Find/Replace edits above, `set_stem_taps`, `stem_taps` and `StemTap`. Then `bounce.rs` by the algorithm above, with `HeardStem::of` following `Strip::output` (`Output::Bus(b)` → `b`, `Output::Master` → master) and `SilentMidi`.

- [ ] **Step 4: Run the tests and the gates**, including the rt-check gates with `--test rt_alloc_bounce`. Expected: core-engine +13 (12 in `bounce.rs`, 1 in `rt_alloc_bounce.rs`), and rt-check green.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-engine/src/bounce.rs crates/core-engine/src/lib.rs crates/core-engine/src/engine.rs crates/core-engine/src/render.rs crates/core-engine/src/graph/exec.rs crates/core-engine/src/graph/node.rs crates/core-engine/tests/bounce.rs crates/core-engine/tests/rt_alloc_bounce.rs
git commit -m "core-engine: offline bounce of the mix and stems from per-run stem taps, aligned by arrival, heard stems gated along their way to master, the -96 dBFS tail and its cap, MIDI silent (plan 5f-2 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Rust +13 (Linux too).

---

### Task 3: Plugins render offline

**Files:**
- Modify: `crates/core-host/Cargo.toml` and `crates/test-plugins/Cargo.toml` (the `render` feature), `crates/core-host/src/instance.rs`, `crates/core-host/src/host.rs`, `crates/core-host/src/clap/handlers.rs`, `crates/core-host/src/clap/instance.rs`, `crates/test-plugins/src/plugin.rs`, `crates/platform-macos/src/au/mod.rs`, `crates/platform-macos/src/au/ffi.rs`
- Test: modify `crates/core-host/tests/clap_host.rs`, `crates/core-host/tests/host.rs`, `crates/platform-macos/tests/au_host.rs`

(The former `platform_macos::sinks::BounceSinks` is gone: `core_engine::bounce::SilentMidi` (Task 2) silences MIDI on every platform, pre-flight I3.)

**Interfaces:**
- Produces:
  ```rust
  // core_host::instance::PluginInstance
  /// Asks the plugin to render offline (`on`) or in real time (spec §9.1);
  /// false when it cannot (no such mode, or a hard real-time requirement).
  fn set_offline(&mut self, on: bool) -> bool { let _ = on; false }
  // core_host::PluginHost
  /// Instances activated from now on are asked to render offline (Ruling 7).
  pub fn set_offline_render(&mut self, on: bool);
  /// What instance `id` answered when asked to render offline; `None` when
  /// it was never asked (a live host, or not activated yet).
  pub fn offline_state(&self, id: PluginInstanceId) -> Option<bool>;
  ```

Changes:
- **CLAP.** `PluginExts` gains `pub render: Option<PluginRender>`, filled by `instance.get_extension()`. `ClapInstance::set_offline` returns false without the extension; with it, false when `on` and `render.has_hard_realtime_requirement(&handle)` is true (pre-flight M3), else `render.set(&mut handle, RenderMode::Offline / Realtime).is_ok()`.
- **AU.** `AuInstance::set_offline` sets property 37 (`kAudioUnitProperty_OfflineRender`, global scope, element 0) to `u32::from(on)` through `Unit::set`, and returns `is_ok()`. `ffi.rs` gains the constant.
- **The test plugin.** The gain kind registers `PluginRender` (main-thread `set` stores the mode in its main-thread state and returns `Ok`; `has_hard_realtime_requirement` is false). The bank kind keeps no render extension.

In `core-host/src/host.rs`, `PluginHost` gains `offline: bool` (false in `new`) and `offline_states: BTreeMap<PluginInstanceId, bool>` (empty in `new`; an entry goes when its slot is dropped). Then:

Find:
```rust
        let proc = live.inst.activate(self.sample_rate, self.max_frames)?;
```
Replace:
```rust
        if self.offline {
            // A bounce's copies render offline (plan 5f-2 Ruling 7); the
            // job names those that cannot.
            let took = live.inst.set_offline(true);
            self.offline_states.insert(id, took);
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

  Append to `crates/core-host/tests/clap_host.rs`:

```rust
/// Plan 5f-2 Ruling 7: the gain kind offers CLAP's render extension and
/// takes both modes; the bank kind has none, so it renders in real time.
#[test]
fn clap_copies_are_told_to_render_offline() {
    let mut gain = load("vizdaw.test.gain");
    assert!(gain.set_offline(true), "offline");
    assert!(gain.set_offline(false), "and back to real time");
    let mut bank = load("vizdaw.test.bank");
    assert!(!bank.set_offline(true), "no render extension");
    // Still a working plugin after the mode change.
    let mut p = gain.activate(48_000.0, 512).unwrap();
    let mut b = Block::new(2, 2, 0.5);
    assert_eq!(b.run(p.as_mut()), ProcessStatus::Ok);
    assert!(b.outputs[0][..N].iter().all(|x| *x == 0.5));
    gain.deactivate(p);
}
```

  Append to `crates/core-host/tests/host.rs`:

```rust
/// Plan 5f-2 Ruling 7: a bounce host asks every copy to render offline
/// before it activates it and keeps the answers; a live host never asks.
#[test]
fn a_bounce_host_renders_its_copies_offline() {
    let (hist, _, gain) = with_gain();
    let mut b = Builder::from_doc(hist.doc().clone());
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let bank = b.insert(synth, plugin("vizdaw.test.bank")).unwrap();
    let doc = b.finish();
    let mut copies = host(1024);
    copies.set_offline_render(true);
    copies.sync(&doc);
    let r = copies.wait_ready(WAIT);
    assert_eq!(r.loaded.len(), 2, "{:?}", r.failed);
    assert_eq!(copies.offline_state(gain), Some(true));
    assert_eq!(copies.offline_state(bank), Some(false), "no render extension: real time only");
    let mut live = host(1024);
    live.sync(&doc);
    live.wait_ready(WAIT);
    assert_eq!(live.offline_state(gain), None, "a live host never asks");
}
```

  Append to `crates/platform-macos/tests/au_host.rs`:

```rust
/// Plan 5f-2 Ruling 7: Apple's delay takes kAudioUnitProperty_OfflineRender
/// in a bounce host.
#[test]
fn au_units_take_offline_render() {
    let doc = demo_document().unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let delay = b.insert(synth, au("aufx/dely/appl")).unwrap();
    let doc = b.finish();
    let mut h = host();
    h.set_offline_render(true);
    h.sync(&doc);
    let r = h.wait_ready(au_wait());
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert_eq!(h.offline_state(delay), Some(true));
}
```

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-host --test clap_host --test host`; `cargo test -p platform-macos --test au_host`). Expected: FAIL — `set_offline`, `set_offline_render` and `offline_state` not found.

- [ ] **Step 3: Implement** the trait method, the three implementations, the host flag and map, and the feature flags.

- [ ] **Step 4: Run the tests and the gates** (the workspace; `git diff --stat Cargo.lock` empty). Expected: core-host +2, platform-macos +1.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/Cargo.toml crates/test-plugins/Cargo.toml crates/core-host/src/instance.rs crates/core-host/src/host.rs crates/core-host/src/clap/handlers.rs crates/core-host/src/clap/instance.rs crates/test-plugins/src/plugin.rs crates/platform-macos/src/au/mod.rs crates/platform-macos/src/au/ffi.rs crates/core-host/tests/clap_host.rs crates/core-host/tests/host.rs crates/platform-macos/tests/au_host.rs
git commit -m "core-host, platform-macos: plugins render offline in a bounce (CLAP render extension, AU OfflineRender), and the host keeps which copies could not (plan 5f-2 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -14
```
Counts: Rust +3 on macOS (core-host 2, platform-macos 1); +2 on Linux.

---

### Task 4: The bounce job in `core-ffi`

**Files:**
- Create: `crates/core-ffi/src/bounce.rs`, `crates/core-ffi/tests/bounce.rs`
- Modify: `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/jobs.rs`, `crates/core-ffi/src/plugins.rs`, `crates/core-ffi/src/session.rs`, `app/Sources/VizDAWAppModel/ProjectMedia.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`

**Interfaces:**
- Consumes: Tasks 1–3; `crate::media::{files, importer, caf_importer, check}`, `core_media::import::{import_reserved, INCOMING_DIR}`, `core_media::record::free_bytes`, `core_model::query::flatten`, `Session::capture_all`, `host::offline_mode()`.
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
      pub path: Option<String>, pub add_to_project: bool,
      /// Replace an existing mix file (the save panel's confirmation); ignored for stems (owner answer 2).
      pub replace: bool }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct BounceCheckRow { /// Ruling 8.
      pub external_lanes: u32, /// The range's frames (the files hold at least this many).
      pub frames: u64, /// The files it would write, in order (stems numbered, Ruling 3).
      pub files: Vec<String>, /// Ruling 8's sentences, or None.
      pub warning: Option<String> }
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct BouncedFileRow { pub path: String, pub frames: u64, pub rate: u32, pub bits: String, pub channels: u16, pub clipped: u64 }
  /// `files` in source order; `media` in source order (Ruling 10).
  #[derive(Debug, Clone, PartialEq, uniffi::Record)] pub struct BounceResultRow { pub files: Vec<BouncedFileRow>, pub media: Vec<u64> }
  #[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)] pub struct BounceSettingsRow { /// 0–30 000 (Ruling 4).
      pub tail_cap_ms: u32 }
  #[uniffi::export] pub fn bounce_settings() -> BounceSettingsRow;              // default 10 000
  #[uniffi::export] pub fn set_bounce_settings(settings: BounceSettingsRow);    // clamped to 0–30 000
  /// The stem files for `names` in `folder` (Ruling 3): made safe, cut to fit,
  /// numbered against this bounce and the folder's files. Not exported.
  pub fn stem_paths(folder: &Path, names: &[String], ext: &str) -> Vec<PathBuf>;
  // jobs.rs
  JobKindRow::Bounce
  Work::Bounce { files: Vec<(String, Result<Vec<Imported>, String>)>, target: Option<ImportTargetRow>,
      exported: Vec<crate::bounce::BouncedFileRow>, problems: Vec<String> }
  // plugins.rs
  pub struct BounceCopies { pub token: u64, pub factory: core_host::ShelfFactory, pub ready: Arc<AtomicBool>, pub problems: Arc<Mutex<Vec<String>>> }
  pub fn start_bounce(session: u64, doc: &Document, rate: f64, max: u32) -> Result<BounceCopies, VizError>;
  pub fn tick_bounces();             // Session::idle; sets `ready` when nothing is loading
  pub fn end_bounce(token: u64);     // empties the host, then to `closing`
  #[cfg(feature = "test-hooks")] pub fn bounce_hosts() -> usize;   // copies hosts not yet ended (pre-flight M4)
  // Session (exported)
  pub fn bounce(&self, req: BounceRequestRow) -> Result<u64, VizError>;           // the job ID
  pub fn bounce_check(&self, req: BounceRequestRow) -> Result<BounceCheckRow, VizError>;
  pub fn bounce_result(&self, job: u64) -> Result<Option<BounceResultRow>, VizError>;
  // Session (test-hooks block, beside debug_hold_jobs)
  /// The next bounce cancels itself once its progress reaches `at` (None: never).
  pub fn debug_bounce_cancel_at(&self, at: Option<f32>) -> Result<(), VizError>;
  ```

`Session::bounce` (and `bounce_check`, which validates the same way and starts nothing) validates everything before it starts, each failure a `Model` error with nothing written:
- `seq` exists; a `Stems` list is non-empty, at most the mixer's strip count, with known strips, each once;
- the range is not empty, `0 ≤ start < end ≤ i64::MAX / 2`, and `Song` is `[0, seq_length)`;
- `rate` is 8,000–384,000, and the depth and dither are valid;
- the destinations follow Ruling 3 (absolute, no `..`, at most 1024 bytes, the folder exists, outside the package and its scratch root after canonicalising the parent, the container's extension for a mix, a mix path that is not a folder, a stems path that is a folder, and an existing mix file only with `replace`);
- there is a `path` or `add_to_project` ("choose a file or Add to project", pre-flight M6).
Then, in order:
- inside an open edit, `Busy` "Finish the open edit first." (M13); while another bounce of this session is unapplied, `Busy` "A bounce is running."; with `add_to_project`, `media_jobs_idle()` (I9);
- `capture_all` (Ruling 6);
- for a document with plugin slots, `plugins::start_bounce(self.id, &doc, rate, 1024)`, recorded as `inner.bounce_copies = Some((job, token))` once the job is spawned;
- `inner.jobs.spawn(JobKindRow::Bounce, …)`, with the copied document, the request, the media table (`media::files`), the root, the reserved names, the tail cap (`bounce_settings()`), the problems known now (offline regions, M5) and the test hook's cancel point.

The job thread (Ruling 12):
1. waits for `ready` (or the cancel, or the 60 s deadline);
2. builds `Engine::new(cfg, sinks)` with `cfg = EngineConfig::new(rate)` (`workers 0` and `deadline None`, the defaults), `cfg.max_frames = 1024`, `cfg.disk = DiskMode::Manual` (pre-flight M8), and `sinks = SilentMidi(…)` by Ruling 8 (`#[cfg(target_os = "macos")]` `MacSinks` when not `offline_mode()`, with the note "// macOS only: the DLS synth; other platforms bounce through the offline sinks"); the insert factory (the copies' `ShelfFactory`, or `NoInserts`); `set_media(…)`; `load(doc, seq)`, whose `graph.placeholders` name the plugins that did not load ("<name> could not load for the bounce and was bypassed");
3. starts an `OfflineBackend::new(rate, 1024)` and runs `core_engine::bounce::bounce(…, block 1024, …)`, writing each source to a temporary float CAF in `Media/.incoming/`. The `BounceSpec` follows Ruling 5: `mix` for a Mix; for Stems, `stems` = the strips when there is a `path` and `heard_stems` = `HeardStem::of(doc.mixer(), s)` for each strip when `add_to_project` is set (both when both are). Progress 0–0.8;
4. drops the engine; adds `stats.nonfinite`, `stats.underruns`, `take_stream_errors` and the copies' problems to the job's problems;
5. if `add_to_project`, imports the project's sources with `import_reserved(caf_importer, …, project format, name)`: the Mix ("Mix" or the file's stem), or each heard stem (the strip's stem file name, or the strip name) (0.8–0.9);
6. writes every export's partial (`core_media::export::write_partial`, `frames: Some(stats.frames)`): the Mix, or each of `stems`, to `path` or `stem_paths(folder, names, ext)` (0.9–1.0);
7. checks `cancel` a last time, then renames: `commit` for the mix, `commit_numbered` for stems. A file's `clipped > 0` adds "<file name>: <n> samples clipped" to the problems;
8. removes the temporaries, and returns `Work::Bounce` (or `Work::Cancelled` after removing the imports, partials and temporaries).

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

The `Work::Bounce` arm and the copies' end, in `apply_jobs`:

Find:
```rust
                Work::Cancelled => row(id, kind, JobStateRow::Cancelled, None, vec![], vec![]),
```
Replace:
```rust
                Work::Cancelled => row(id, kind, JobStateRow::Cancelled, None, vec![], vec![]),
                Work::Bounce {
                    files,
                    target,
                    exported,
                    problems,
                } => {
                    // Exported files are already in place (Ruling 12): kept
                    // whatever the import's apply does.
                    let applied = if files.is_empty() {
                        Ok((Vec::new(), Vec::new()))
                    } else {
                        apply_import_as(inner, &root, files, target, &ImportAs::BOUNCE)
                    };
                    match applied {
                        Ok((created, more)) => {
                            changed |= !created.is_empty();
                            inner.bounce_results.insert(
                                id,
                                crate::bounce::BounceResultRow {
                                    files: exported,
                                    media: created.clone(),
                                },
                            );
                            let all = problems.into_iter().chain(more).collect();
                            row(id, kind, JobStateRow::Done, None, all, created)
                        }
                        Err((e, more)) => {
                            inner.bounce_results.insert(
                                id,
                                crate::bounce::BounceResultRow {
                                    files: exported,
                                    media: Vec::new(),
                                },
                            );
                            let all = problems.into_iter().chain(more).collect();
                            row(id, kind, JobStateRow::Failed, Some(e.to_string()), all, vec![])
                        }
                    }
                }
```

Find:
```rust
            inner.jobs.finish(id, row);
        }
```
Replace:
```rust
            inner.jobs.finish(id, row);
        }
        // A bounce's plugin copies end with its job, however it ended:
        // applied, failed, cancelled or panicked (plan 5f-2 Ruling 6,
        // pre-flight I2).
        if inner.bounce_copies.is_some()
            && !inner.jobs.pending(&[crate::jobs::JobKindRow::Bounce])
        {
            if let Some((_, token)) = inner.bounce_copies.take() {
                plugins::end_bounce(token);
            }
        }
```

In `discard` (a job never applied), only files inside the package go; an export renamed into place is never removed (Ruling 12):

Find:
```rust
        Work::Cancelled => Vec::new(),
```
Replace:
```rust
        // Only the media it imported; the files it exported stay.
        Work::Bounce { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .flatten()
            .map(|i| i.path)
            .collect(),
        Work::Cancelled => Vec::new(),
```

At close, after the jobs are shut down:

Find:
```rust
            for w in inner.jobs.shutdown() {
                discard(&root, w, &keep);
            }
            inner.overviews.shutdown();
```
Replace:
```rust
            for w in inner.jobs.shutdown() {
                discard(&root, w, &keep);
            }
            // The bounce's plugin copies go with it (pre-flight I2).
            if let Some((_, token)) = inner.bounce_copies.take() {
                plugins::end_bounce(token);
            }
            inner.overviews.shutdown();
```

A bounce with "Add to project" is a media job (Ruling 9, pre-flight I9):

Find:
```rust
            .pending(&[JobKindRow::Format, JobKindRow::Convert, JobKindRow::Relink])
```
Replace:
```rust
            .pending(&[
                JobKindRow::Format,
                JobKindRow::Convert,
                JobKindRow::Relink,
                JobKindRow::Bounce,
            ])
```

`Inner` gains `bounce_copies: Option<(u64, u64)>`, `bounce_results: BTreeMap<u64, BounceResultRow>` and (test hook) `bounce_cancel_at: Option<f32>`, all empty in `Session::with`. Save As already refuses while any job is unapplied ("Wait for the import or conversion to finish, then save."), which covers a running bounce (pre-flight M15).

In `plugins.rs`, the registry gains `bounces: BTreeMap<u64, BounceEntry>` (`{ session, host, ready, problems }`, tokens from a counter). `start_bounce` makes a `PluginHost::new(rate, max)` with AU on macOS, `catalog::apply`, `set_offline_render(true)`, `sync(doc)`. `tick_bounces` idles each bounce host; when `pending() == 0` it fills `problems` (the slots whose `offline_state` is `Some(false)`: "<name> renders in real time only") and sets `ready`. `end_bounce` removes the entry, `sync(&Document::new())`, `idle()`, and moves the host to `closing` while it has `retiring() > 0`. A dropped session's bounce hosts end too:

Find:
```rust
pub fn close_session(id: u64) {
```
Replace:
```rust
pub fn close_session(id: u64) {
    // Its bounce's plugin copies first (plan 5f-2, pre-flight I2).
    end_bounces_of(id);
```

(`end_bounces_of(session)` ends every bounce entry of that session; while the registry is busy it queues like `close`.)

In `Session::idle`, next to `plugins::tick_closing()`:

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

Register `pub mod bounce;` in `lib.rs` after `pub mod blocks;`, and re-export its rows:

Find:
```rust
pub mod blocks;
```
Replace:
```rust
pub mod blocks;
pub mod bounce;
```

Find:
```rust
pub use blocks::{BlockKind, BlockRow, BlockRule};
```
Replace:
```rust
pub use blocks::{BlockKind, BlockRow, BlockRule};
pub use bounce::{
    BounceCheckRow, BounceRangeRow, BounceRequestRow, BounceResultRow, BounceSettingsRow,
    BounceSourceRow, BouncedFileRow, ContainerRow, bounce_settings, set_bounce_settings,
};
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
    /// A bounce (plan 5f-2 Rulings 9, 12): its project imports, the files
    /// it exported (already renamed into place) and its problems.
    Bounce {
        /// Per source: what its project import became (empty without "Add to project").
        files: Vec<(String, Result<Vec<Imported>, String>)>,
        /// Where regions go.
        target: Option<ImportTargetRow>,
        /// The files written outside the project, in source order.
        exported: Vec<crate::bounce::BouncedFileRow>,
        /// Clipped samples, placeholders, plugins without offline mode,
        /// underruns, offline media (Rulings 6–8, 13).
        problems: Vec<String>,
    },
    /// Cancelled before it finished (whatever it wrote is removed).
    Cancelled,
```

In Swift, `ProjectMedia.swift`'s two switches gain the bounce (its job polling answers a bounce as the channel does from Task 5 on):

Find:
```swift
        case .convert, .format: "Converting media…"
```
Replace:
```swift
        case .convert, .format: "Converting media…"
        case .bounce: "Bouncing…"
```

Find:
```swift
        case .convert, .format: "Conversion Failed"
```
Replace:
```swift
        case .convert, .format: "Conversion Failed"
        case .bounce: "Bounce Failed"
```

`FakeSession` gains, after `var cancelled: [UInt64] = []`:

Find:
```swift
    var cancelled: [UInt64] = []
```
Replace:
```swift
    var cancelled: [UInt64] = []
    var bounces: [BounceRequestRow] = []
    var bounceError: VizError?
    var checkRow = BounceCheckRow(externalLanes: 0, frames: 96_000, files: [], warning: nil)
    var checkRequests: [BounceRequestRow] = []
    var resultRow: BounceResultRow?
    func bounce(req: BounceRequestRow) throws -> UInt64 {
        if let e = bounceError { throw e }
        bounces.append(req)
        let id = UInt64(3000 + bounces.count)
        jobRows[id] = JobRow(
            id: id, kind: .bounce, progress: jobStartState == .running ? 0.4 : 1, state: jobStartState,
            message: nil, problems: [], created: [])
        return id
    }
    func bounceCheck(req: BounceRequestRow) throws -> BounceCheckRow {
        checkRequests.append(req)
        return checkRow
    }
    func bounceResult(job: UInt64) throws -> BounceResultRow? { resultRow }
```

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/bounce.rs <<'VIZDAW_EOF'
//! The Bounce job (plan 5f-2 Task 4; spec §9, §12.1, §12.3; Rulings 3–6,
//! 8–10, 12, 13; owner answers 2026-11-03). Projects are saved packages
//! in temporary folders, built with the fixtures' `Builder`, at 48 kHz
//! float, with a region of a generated CAF from tick 0 (pre-flight C2: the
//! demo's media does not exist and the offline synth is silent). One test
//! at a time: the engine host and the bounce settings are process-wide.

mod common;

use core_engine::fixtures::{Builder, Q};
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::command::{Command, MixerCmd, SettingsCmd};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::{AudioFormat, BitDepth};
use core_model::ids::{SeqId, StripId};
use core_model::mixer::{Output, PluginFormat, PluginRef, StripKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use vizdaw_ffi::edit::{EditCommand, SequenceEdit};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    BitDepthRow, BounceRangeRow, BounceRequestRow, BounceSettingsRow, BounceSourceRow,
    ContainerRow, JobRow, JobStateRow, MediaOriginRow, ProjectFormatRow, Session, SlotStatus,
    VizError, bounce_settings, set_bounce_settings,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    set_bounce_settings(BounceSettingsRow { tail_cap_ms: 10_000 });
    g
}

const FRAMES: u64 = 480_000;
/// Four bars at 120 BPM (the Builder's tempo): 16 quarters of 15 360 ticks.
const BARS4: i64 = 16 * Q;
/// ... are 16 × 24 000 frames.
const BARS4_FRAMES: u64 = 384_000;
const STEP24: f32 = 1.0 / 8_388_608.0;

fn value(f: u64) -> f32 {
    ((f % 997) as f32 + 1.0) / 2000.0
}

fn plugin(id: &str, name: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: name.into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

fn strip_of(d: &Document, want: fn(&StripKind) -> bool) -> StripId {
    d.mixer().order().iter().copied().find(|s| d.mixer().strip(*s).is_ok_and(|x| want(&x.kind))).unwrap()
}

struct Project {
    dir: tempfile::TempDir,
    pkg: PathBuf,
    s: Arc<Session>,
    seq: u64,
    /// The region's AudioPath strip.
    path: u64,
}

/// `P.vizdaw` in a temporary folder, at 48 kHz float: Song (120 BPM) with
/// lane "Audio" playing `Media/a.caf` (value(f) left, −value(f) right,
/// FRAMES frames) from tick 0 on its AudioPath strip; `extra` adds to the
/// document before it is saved. Opened, with a private Trash.
fn project(extra: impl FnOnce(&mut Builder, SeqId, StripId)) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    let mut b = Builder::new();
    b.run(Command::Settings(SettingsCmd::SetFormat {
        format: AudioFormat { sample_rate: 48_000, depth: BitDepth::Float32 },
        swaps: Vec::new(),
    }))
    .unwrap();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FRAMES, 2).unwrap();
    b.region(seq, lane, 0, m, 0, FRAMES, 0.0, Fade::default(), Fade::default()).unwrap();
    let path = strip_of(b.doc(), |k| matches!(k, StripKind::AudioPath(_)));
    extra(&mut b, seq, path);
    core_model::file::save(&b.finish(), &pkg).unwrap();
    let mut w = CafWriter::create(&pkg.join("Media/a.caf"), 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&(0..FRAMES).flat_map(|f| [value(f), -value(f)]).collect::<Vec<f32>>()).unwrap();
    w.finish().unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    Project { dir, pkg, s, seq: seq.0, path: path.0 }
}

/// A Mix of the first four bars to `dest`, 48 kHz 24-bit WAV, no dither.
fn mix_to(p: &Project, dest: &Path) -> BounceRequestRow {
    BounceRequestRow {
        seq: p.seq,
        source: BounceSourceRow::Mix,
        range: BounceRangeRow::Ticks { start: 0, end: BARS4 },
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: Some(dest.display().to_string()),
        add_to_project: false,
        replace: false,
    }
}

fn wait_job(s: &Session, id: u64) -> JobRow {
    let until = Instant::now() + common::wait_limit();
    loop {
        s.idle().unwrap();
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            s.idle().unwrap();
            return j;
        }
        assert!(Instant::now() < until, "job {id} still running");
        std::thread::yield_now();
    }
}

fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let until = Instant::now() + common::wait_limit();
    while !done(s) {
        assert!(Instant::now() < until, "timed out");
        s.idle().unwrap();
        std::thread::yield_now();
    }
}

/// `(rate, channels, bits, float, frames, samples)` of a WAV (Task 1's reader).
fn read_wav(p: &Path) -> (u32, u16, u16, bool, u64, Vec<i64>) {
    let b = std::fs::read(p).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
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

/// A WAV's channel `c` as floats.
fn wav_channel(p: &Path, c: usize) -> Vec<f32> {
    let (_, ch, bits, float, _, s) = read_wav(p);
    s.iter()
        .skip(c)
        .step_by(usize::from(ch))
        .map(|x| match (bits, float) {
            (16, _) => *x as f32 / 32_768.0,
            (24, _) => *x as f32 / 8_388_608.0,
            _ => f32::from_bits(*x as u32),
        })
        .collect()
}

/// A media item's left channel, from its file in the package.
fn media_left(p: &Project, id: u64) -> Vec<f32> {
    let row = p.s.media().unwrap().into_iter().find(|r| r.id == id).unwrap();
    let r = CafReader::open(&p.pkg.join(&row.path)).unwrap();
    let mut buf = vec![0f32; r.info().frames as usize * usize::from(r.info().channels)];
    r.read(0, &mut buf, &mut Vec::new()).unwrap();
    buf.iter().step_by(usize::from(r.info().channels)).copied().collect()
}

fn sounds(x: &[f32]) -> bool {
    x.iter().any(|v| v.abs() > 1e-3)
}

/// Every name in `dir` (sorted).
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

/// Files left in `Media/.incoming`.
fn incoming(p: &Project) -> Vec<String> {
    let d = p.pkg.join("Media/.incoming");
    if d.is_dir() { names(&d) } else { Vec::new() }
}

#[test]
fn a_bounce_equals_an_offline_render_of_the_range() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("mix.wav");
    let req = mix_to(&p, &dest);
    let check = p.s.bounce_check(req.clone()).unwrap();
    assert_eq!(check.frames, BARS4_FRAMES, "four bars at 120 BPM");
    assert_eq!((check.files.clone(), check.external_lanes, check.warning.clone()), (vec![dest.display().to_string()], 0, None));
    let j = wait_job(&p.s, p.s.bounce(req).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(j.problems.is_empty(), "{j:?}");
    let (rate, ch, bits, float, frames, _) = read_wav(&dest);
    assert_eq!((rate, ch, bits, float), (48_000, 2, 24, false));
    assert!(check.frames <= frames && frames <= check.frames + 480_000, "{frames} (pre-flight M11)");
    let (left, right) = (wav_channel(&dest, 0), wav_channel(&dest, 1));
    let k = left[996] / value(996);
    assert!(k > 0.1, "the strip's pan law is a constant");
    for f in 0..BARS4_FRAMES as usize {
        let want = k * value(f as u64);
        assert!((left[f] - want).abs() <= 2.0 * STEP24, "left frame {f}: {} vs {want}", left[f]);
        assert!((right[f] + want).abs() <= 2.0 * STEP24, "right frame {f}");
    }
    let res = p.s.bounce_result(j.id).unwrap().unwrap();
    assert_eq!(res.media, Vec::<u64>::new());
    let file = &res.files[0];
    assert_eq!(
        (file.path.clone(), file.frames, file.rate, file.bits.clone(), file.channels, file.clipped),
        (dest.display().to_string(), frames, 48_000, "24-bit".to_string(), 2, 0)
    );
    assert!(!p.dir.path().join(".mix.wav.partial").exists(), "no partial");
    assert!(incoming(&p).is_empty(), "no temporary left");
}

/// Ruling 3 and owner answer 2: names are made safe, and a name taken in
/// the bounce or on disk is numbered; nothing in the folder is replaced.
#[test]
fn stem_files_get_safe_unique_names() {
    let _g = serial();
    let mut extra = Vec::new();
    let p = project(|b, _, path| {
        b.mixer(MixerCmd::RenameStrip { strip: path, name: "A/B".into() }).unwrap();
        extra.push(b.bus("A:B").unwrap());
        extra.push(b.bus(".hidden").unwrap());
    });
    let folder = p.dir.path().join("stems");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("A-B.wav"), b"keep").unwrap();
    let strips = vec![p.path, extra[0].0, extra[1].0];
    let req = BounceRequestRow {
        source: BounceSourceRow::Stems { strips },
        path: Some(folder.display().to_string()),
        replace: true,
        ..mix_to(&p, &folder)
    };
    let want: Vec<String> = ["A-B 2.wav", "A-B 3.wav", "-hidden.wav"].iter().map(|n| folder.join(n).display().to_string()).collect();
    assert_eq!(p.s.bounce_check(req.clone()).unwrap().files, want);
    let j = wait_job(&p.s, p.s.bounce(req).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(std::fs::read(folder.join("A-B.wav")).unwrap(), b"keep", "never replaced, even with replace");
    assert_eq!(names(&folder), ["-hidden.wav", "A-B 2.wav", "A-B 3.wav", "A-B.wav"]);
    assert!(sounds(&wav_channel(&folder.join("A-B 2.wav"), 0)), "the track's stem");
    let res = p.s.bounce_result(j.id).unwrap().unwrap();
    assert_eq!(res.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(), want);
    let lens: Vec<u64> = res.files.iter().map(|f| f.frames).collect();
    assert!(lens.iter().all(|n| *n == lens[0]), "one length for every file: {lens:?}");
}

/// Ruling 9 and pre-flight I8: one undo step "Bounce", a new lane at the
/// range start, a `Bounced` item exactly as long as the file.
#[test]
fn add_to_project_places_the_bounce_on_a_new_lane() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let items = p.s.media().unwrap().len();
    let add_only = BounceRequestRow { path: None, add_to_project: true, ..mix_to(&p, p.dir.path()) };
    let j = wait_job(&p.s, p.s.bounce(add_only).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(p.s.snapshot().unwrap().undo_label.as_deref(), Some("Bounce"));
    let lanes = p.s.lanes(p.seq).unwrap();
    let last = lanes.last().unwrap();
    assert_eq!((last.name.as_str(), last.region_count), ("Mix", 1));
    let ev = p.s.lane_events(p.seq, last.id).unwrap();
    assert_eq!(ev.events[0].tick, 0, "at the range start");
    let rows = p.s.media().unwrap();
    assert_eq!(rows.len(), items + 1);
    let item = rows.iter().find(|r| j.created.contains(&r.id)).unwrap();
    assert_eq!((item.origin, item.sample_rate, item.channels), (MediaOriginRow::Bounced, 48_000, 2));
    assert_eq!(p.s.bounce_result(j.id).unwrap().unwrap().media, j.created);
    p.s.undo().unwrap();
    assert_eq!(p.s.lanes(p.seq).unwrap().len(), lanes.len() - 1, "one undo removes the lane");
    assert_eq!(p.s.media().unwrap().len(), items, "and the item");
    // With a file too: the item is as long as the file, and named after it.
    let dest = p.dir.path().join("take one.wav");
    let both = BounceRequestRow { add_to_project: true, ..mix_to(&p, &dest) };
    let j = wait_job(&p.s, p.s.bounce(both).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(p.s.lanes(p.seq).unwrap().last().unwrap().name, "take one");
    let item = p.s.media().unwrap().into_iter().find(|r| j.created.contains(&r.id)).unwrap();
    assert_eq!(item.frames, read_wav(&dest).4, "Add to project holds exactly the file's frames");
    assert!(incoming(&p).is_empty());
}

/// Ruling 5, owner answer 3: files ignore mute and solo; the imports are
/// as heard, through the routing (a track into a muted bus is silent).
#[test]
fn add_to_project_respects_mute_and_solo_through_the_routing_and_files_do_not() {
    let _g = serial();
    let mut ids = Vec::new();
    let p = project(|b, _, path| {
        let bus = b.bus("Bus").unwrap();
        b.mixer(MixerCmd::SetOutput { strip: path, output: Output::Bus(bus) }).unwrap();
        ids.push(bus);
        ids.push(b.bus("Other").unwrap());
    });
    let (track, bus, other) = (p.path, ids[0].0, ids[1].0);
    // (case, file track, file bus, heard track, heard bus)
    let cases: [(&str, bool, bool, bool, bool); 4] = [
        ("nothing", true, true, true, true),
        ("mute the track", true, false, false, false),
        ("mute the bus", true, true, false, false),
        // The track's solo-silenced edge carries nothing into the bus.
        ("solo another strip", true, false, false, false),
    ];
    for (k, (case, ft, fb, ht, hb)) in cases.into_iter().enumerate() {
        p.s.set_strip_mute(track, case == "mute the track").unwrap();
        p.s.set_strip_mute(bus, case == "mute the bus").unwrap();
        p.s.set_strip_solo(other, case == "solo another strip").unwrap();
        let folder = p.dir.path().join(format!("case {k}"));
        std::fs::create_dir(&folder).unwrap();
        let req = BounceRequestRow {
            source: BounceSourceRow::Stems { strips: vec![track, bus] },
            path: Some(folder.display().to_string()),
            add_to_project: true,
            ..mix_to(&p, &folder)
        };
        let j = wait_job(&p.s, p.s.bounce(req).unwrap());
        assert_eq!(j.state, JobStateRow::Done, "{case}: {j:?}");
        let res = p.s.bounce_result(j.id).unwrap().unwrap();
        let file = |i: usize| wav_channel(Path::new(&res.files[i].path), 0);
        assert_eq!(sounds(&file(0)), ft, "{case}: the track's file");
        assert_eq!(sounds(&file(1)), fb, "{case}: the bus's file");
        assert_eq!(res.media.len(), 2, "{case}");
        assert_eq!(sounds(&media_left(&p, res.media[0])), ht, "{case}: the track in place");
        assert_eq!(sounds(&media_left(&p, res.media[1])), hb, "{case}: the bus in place");
    }
}

/// Ruling 12: a cancel while the imports are written (progress 0.85)
/// removes the imports, the temporaries and every partial; the copies end.
#[test]
fn cancel_removes_everything_the_bounce_wrote() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.gain", "Test Gain")).unwrap();
    });
    idle_until(&p.s, |s| s.plugin_slots().unwrap()[0].status == SlotStatus::Live);
    let items = p.s.media().unwrap().len();
    let folder = p.dir.path().join("stems");
    std::fs::create_dir(&folder).unwrap();
    let req = BounceRequestRow {
        source: BounceSourceRow::Stems { strips: vec![p.path] },
        path: Some(folder.display().to_string()),
        add_to_project: true,
        ..mix_to(&p, &folder)
    };
    p.s.debug_bounce_cancel_at(Some(0.85)).unwrap();
    let j = wait_job(&p.s, p.s.bounce(req.clone()).unwrap());
    assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
    assert!(names(&folder).is_empty(), "no file and no partial: {:?}", names(&folder));
    assert!(incoming(&p).is_empty(), "no temporary");
    assert_eq!(p.s.media().unwrap().len(), items, "no media item");
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0, "the copies ended (pre-flight I2)");
    // Cancelled before it starts: the copies end too.
    p.s.debug_bounce_cancel_at(None).unwrap();
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.bounce(req).unwrap();
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 1);
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0);
    assert!(names(&folder).is_empty() && incoming(&p).is_empty());
}

/// Review Focus 3: cancelled at 95 % (the export's partial being written)
/// over a file it was told to replace: the old file is byte-identical.
#[test]
fn a_late_cancel_keeps_the_replaced_file() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("mix.wav");
    std::fs::write(&dest, b"the old mix").unwrap();
    p.s.debug_bounce_cancel_at(Some(0.95)).unwrap();
    let j = wait_job(&p.s, p.s.bounce(BounceRequestRow { replace: true, ..mix_to(&p, &dest) }).unwrap());
    assert_eq!(j.state, JobStateRow::Cancelled, "{j:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"the old mix");
    assert!(!p.dir.path().join(".mix.wav.partial").exists());
    assert!(incoming(&p).is_empty());
    p.s.debug_bounce_cancel_at(None).unwrap();
}

/// Ruling 6: the copies take the live (unsaved) state; pre-flight C2: the
/// check can fail, since the region sounds.
#[test]
fn copies_render_offline_with_the_live_state() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.gain", "Test Gain")).unwrap();
    });
    idle_until(&p.s, |s| s.plugin_slots().unwrap()[0].status == SlotStatus::Live);
    let slot = p.s.plugin_slots().unwrap()[0].id;
    let bounce = |name: &str| {
        let dest = p.dir.path().join(name);
        let j = wait_job(&p.s, p.s.bounce(mix_to(&p, &dest)).unwrap());
        assert_eq!(j.state, JobStateRow::Done, "{j:?}");
        assert!(j.problems.is_empty(), "the gain plugin renders offline: {j:?}");
        wav_channel(&dest, 0)
    };
    let full = bounce("full.wav");
    assert!(sounds(&full));
    p.s.set_param(slot, 0, 0.5).unwrap();
    let half = bounce("half.wav");
    for f in 0..BARS4_FRAMES as usize {
        assert!((half[f] - full[f] / 2.0).abs() <= 2.0 * STEP24, "frame {f}: the copy has the live 0.5");
    }
    p.s.set_param(slot, 0, 0.0).unwrap();
    assert!(!sounds(&bounce("silent.wav")), "and the live 0");
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0, "the copies went to closing");
}

/// Ruling 6: a copy that cannot load is a pass-through, named in the problems.
#[test]
fn a_copy_that_fails_is_a_pass_through_and_named() {
    let _g = serial();
    let dry = project(|_, _, _| {});
    let dest = dry.dir.path().join("dry.wav");
    assert_eq!(wait_job(&dry.s, dry.s.bounce(mix_to(&dry, &dest)).unwrap()).state, JobStateRow::Done);
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.nothere", "Not Here")).unwrap();
    });
    let wet = p.dir.path().join("wet.wav");
    let j = wait_job(&p.s, p.s.bounce(mix_to(&p, &wet)).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(j.problems.iter().any(|m| m.contains("Not Here")), "{j:?}");
    assert_eq!(wav_channel(&wet, 0), wav_channel(&dest, 0), "the dry signal");
}

/// Ruling 6: live playback goes on, and its engine is not reloaded.
#[test]
fn a_bounce_while_playing_keeps_playback_running() {
    let _g = serial();
    let p = project(|_, _, _| {});
    p.s.play(p.seq).unwrap();
    idle_until(&p.s, |s| s.transport().unwrap().playing);
    let loads = vizdaw_ffi::host::loads();
    let j = wait_job(&p.s, p.s.bounce(mix_to(&p, &p.dir.path().join("live.wav"))).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(p.s.transport().unwrap().playing, "playback went on");
    assert_eq!(vizdaw_ffi::host::loads(), loads, "the live engine was not reloaded");
    p.s.stop().unwrap();
}

/// Rulings 6 and 12; pre-flight M13, M15: one bounce per session; not inside
/// an open edit; no Save As while it runs; the copies always end.
#[test]
fn one_bounce_at_a_time() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.insert(path, plugin("vizdaw.test.gain", "Test Gain")).unwrap();
    });
    idle_until(&p.s, |s| s.plugin_slots().unwrap()[0].status == SlotStatus::Live);
    let req = mix_to(&p, &p.dir.path().join("one.wav"));
    p.s.begin_edit("Fader".into()).unwrap();
    let e = p.s.bounce(req.clone()).unwrap_err();
    assert!(matches!(&e, VizError::Busy { message } if message.contains("Finish the open edit")), "{e:?}");
    p.s.abort_edit().unwrap();
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.bounce(req.clone()).unwrap();
    assert!(matches!(p.s.bounce(req.clone()), Err(VizError::Busy { .. })), "a second bounce waits");
    assert!(matches!(p.s.save_as(p.dir.path().join("Q.vizdaw").display().to_string()), Err(VizError::Busy { .. })));
    p.s.cancel_job(id).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&p.s, id).state, JobStateRow::Cancelled);
    assert_eq!(vizdaw_ffi::plugins::bounce_hosts(), 0);
    assert_eq!(wait_job(&p.s, p.s.bounce(req).unwrap()).state, JobStateRow::Done, "then the next one runs");
}

/// Ruling 9, pre-flight I9.
#[test]
fn a_bounce_and_a_format_change_wait_for_each_other() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let add = BounceRequestRow { path: None, add_to_project: true, ..mix_to(&p, p.dir.path()) };
    let fmt = ProjectFormatRow { sample_rate: 48_000, depth: BitDepthRow::Int24 };
    p.s.debug_hold_jobs(true).unwrap();
    let b = p.s.bounce(add.clone()).unwrap();
    assert!(matches!(p.s.set_project_format(fmt.clone()), Err(VizError::Busy { .. })), "no format change under a bounce");
    p.s.cancel_job(b).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, b);
    p.s.debug_hold_jobs(true).unwrap();
    let f = p.s.set_project_format(fmt).unwrap();
    assert!(matches!(p.s.bounce(add), Err(VizError::Busy { .. })), "no bounce in place under a format change");
    p.s.cancel_job(f).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, f);
}

/// Ruling 12 (pre-flight I1): the apply fails (the sequence went), but the
/// exported file, which replaced an old one, stays whole.
#[test]
fn a_failed_import_keeps_the_exported_file() {
    let _g = serial();
    let p = project(|b, _, _| {
        b.sequence("Other", "Audio", None).unwrap();
    });
    let dest = p.dir.path().join("mix.wav");
    std::fs::write(&dest, b"old").unwrap();
    let items = p.s.media().unwrap().len();
    p.s.debug_hold_jobs(true).unwrap();
    let id = p.s.bounce(BounceRequestRow { add_to_project: true, replace: true, ..mix_to(&p, &dest) }).unwrap();
    p.s.edit(EditCommand::Sequence { edit: SequenceEdit::Delete { seq: p.seq, delete_references: true } }, None).unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&p.s, id);
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    let (rate, ch, bits, _, frames, _) = read_wav(&dest);
    assert_eq!((rate, ch, bits), (48_000, 2, 24), "the new file, whole");
    assert!(frames >= BARS4_FRAMES);
    assert_eq!(p.s.media().unwrap().len(), items, "its import was removed");
    assert_eq!(p.s.bounce_result(id).unwrap().unwrap().files[0].path, dest.display().to_string());
    assert!(incoming(&p).is_empty());
}

/// Ruling 8: the check counts lanes on external MIDI and says the spec's
/// sentence, singular and plural (silence itself: Task 2's
/// `external_midi_is_silent_in_a_bounce`).
#[test]
fn external_midi_is_counted_in_the_check() {
    let _g = serial();
    for lanes in [1, 2] {
        let p = project(|b, seq, _| {
            let ext = b.dest("Hardware", "Ext Port", 0).unwrap();
            for n in 0..lanes {
                let l = b.lane(seq, &format!("Keys {n}"), Some(ext)).unwrap();
                b.notes(seq, l, &[(0, 60, 0x8000, Q)]).unwrap();
            }
        });
        let req = mix_to(&p, &p.dir.path().join("ext.wav"));
        let c = p.s.bounce_check(req.clone()).unwrap();
        assert_eq!(c.external_lanes, lanes);
        let want = if lanes == 1 {
            "1 lane plays external MIDI and will be silent. Track it as audio first: Track Synth…"
        } else {
            "2 lanes play external MIDI and will be silent. Track them as audio first: Track Synth…"
        };
        assert_eq!(c.warning.as_deref(), Some(want));
        assert_eq!(wait_job(&p.s, p.s.bounce(req).unwrap()).state, JobStateRow::Done);
    }
}

/// Ruling 13, owner answer 1: a hot mix clips in a 16-bit file (both
/// signs, counted, reported, and the bounce is still Done) and keeps its
/// overs in a float file.
#[test]
fn clipped_samples_are_reported() {
    let _g = serial();
    let p = project(|b, _, path| {
        b.mixer(MixerCmd::SetGain { strip: path, gain_db: 12.0 }).unwrap();
    });
    let loud = p.dir.path().join("loud.wav");
    let j = wait_job(&p.s, p.s.bounce(BounceRequestRow { depth: BitDepthRow::Int16, ..mix_to(&p, &loud) }).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(j.problems.iter().any(|m| m.contains("loud.wav") && m.contains("samples clipped")), "{j:?}");
    let res = p.s.bounce_result(j.id).unwrap().unwrap();
    assert!(res.files[0].clipped > 0);
    let (.., s) = read_wav(&loud);
    assert!(s.contains(&32_767) && s.contains(&-32_768), "clipped at both ends");
    let float = p.dir.path().join("loud float.wav");
    let j = wait_job(&p.s, p.s.bounce(BounceRequestRow { depth: BitDepthRow::Float32, ..mix_to(&p, &float) }).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert!(j.problems.is_empty(), "a float file does not clip: {j:?}");
    let (l, r) = (wav_channel(&float, 0), wav_channel(&float, 1));
    assert!(l.iter().any(|x| *x > 1.0) && r.iter().any(|x| *x < -1.0), "overs kept above 0 dBFS");
}

/// Ruling 3, pre-flight I4, M6, M7: each is `Model`, and nothing is written.
#[test]
fn hostile_bounce_requests_are_refused() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let d = p.dir.path();
    std::fs::create_dir(d.join("folder.wav")).unwrap();
    std::fs::write(d.join("file.txt"), b"x").unwrap();
    let before = names(d);
    let ok = mix_to(&p, &d.join("x.wav"));
    let stems = |strips: Vec<u64>| BounceRequestRow { source: BounceSourceRow::Stems { strips }, path: Some(d.display().to_string()), ..ok.clone() };
    let at = |path: String| BounceRequestRow { path: Some(path), ..ok.clone() };
    let cases: Vec<(&str, BounceRequestRow)> = vec![
        ("unknown seq", BounceRequestRow { seq: 9_999, ..ok.clone() }),
        ("empty stems", stems(vec![])),
        ("a strip twice", stems(vec![p.path, p.path])),
        ("an unknown strip", stems(vec![99_999])),
        ("10 000 strips", stems((0..10_000).collect())),
        ("start = end", BounceRequestRow { range: BounceRangeRow::Ticks { start: BARS4, end: BARS4 }, ..ok.clone() }),
        ("start > end", BounceRequestRow { range: BounceRangeRow::Ticks { start: BARS4, end: Q }, ..ok.clone() }),
        ("negative start", BounceRequestRow { range: BounceRangeRow::Ticks { start: -1, end: Q }, ..ok.clone() }),
        ("end past i64::MAX/2", BounceRequestRow { range: BounceRangeRow::Ticks { start: 0, end: i64::MAX }, ..ok.clone() }),
        ("rate 0", BounceRequestRow { rate: 0, ..ok.clone() }),
        ("rate 7 999", BounceRequestRow { rate: 7_999, ..ok.clone() }),
        ("rate 384 001", BounceRequestRow { rate: 384_001, ..ok.clone() }),
        ("rate 1 000 000", BounceRequestRow { rate: 1_000_000, ..ok.clone() }),
        ("neither path nor add", BounceRequestRow { path: None, add_to_project: false, ..ok.clone() }),
        ("a relative path", at("relative.wav".into())),
        ("a missing folder", at(d.join("no/such/x.wav").display().to_string())),
        ("a wrong extension", at(d.join("x.caf").display().to_string())),
        ("a mix path that is a folder", BounceRequestRow { replace: true, ..at(d.join("folder.wav").display().to_string()) }),
        ("a stems path that is a file", BounceRequestRow { path: Some(d.join("file.txt").display().to_string()), ..stems(vec![p.path]) }),
        ("inside the package", at(p.pkg.join("x.wav").display().to_string())),
        ("inside its media", at(p.pkg.join("Media/x.wav").display().to_string())),
        ("a .. component", at(format!("{}/sub/../x.wav", d.display()))),
        ("a 4097-byte path", at(format!("{}/{}.wav", d.display(), "a".repeat(4097)))),
    ];
    for (what, req) in cases {
        let e = p.s.bounce(req.clone()).unwrap_err();
        assert!(matches!(e, VizError::Model { .. }), "{what}: {e:?}");
        assert!(matches!(p.s.bounce_check(req), Err(VizError::Model { .. })), "{what}: the check refuses it too");
        if what.starts_with("inside") {
            assert!(e.to_string().contains("outside the project"), "{what}: {e}");
        }
    }
    assert_eq!(names(d), before, "nothing was written");
    assert!(!p.pkg.join("x.wav").exists() && !p.pkg.join("Media/x.wav").exists());
}

/// Ruling 4: the tail limit is process-wide and clamped.
#[test]
fn the_tail_limit_is_clamped() {
    let _g = serial();
    assert_eq!(bounce_settings().tail_cap_ms, 10_000);
    set_bounce_settings(BounceSettingsRow { tail_cap_ms: 99_000 });
    assert_eq!(bounce_settings().tail_cap_ms, 30_000);
    set_bounce_settings(BounceSettingsRow { tail_cap_ms: 0 });
    assert_eq!(bounce_settings().tail_cap_ms, 0);
    set_bounce_settings(BounceSettingsRow { tail_cap_ms: 10_000 });
}

// macOS only: rate conversion goes through ExtAudioFile (the CAF importer converts depth only).
#[cfg(target_os = "macos")]
#[test]
fn a_bounce_at_another_rate_converts() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("44k1.wav");
    let j = wait_job(&p.s, p.s.bounce(BounceRequestRow { rate: 44_100, ..mix_to(&p, &dest) }).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let (rate, _, _, _, frames, _) = read_wav(&dest);
    assert_eq!(rate, 44_100);
    let want = BARS4_FRAMES * 44_100 / 48_000;
    assert!(frames.abs_diff(want) <= 1, "{frames} vs {want}");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn a_rate_change_without_the_system_converter_fails_cleanly() {
    let _g = serial();
    let p = project(|_, _, _| {});
    let dest = p.dir.path().join("44k1.wav");
    let j = wait_job(&p.s, p.s.bounce(BounceRequestRow { rate: 44_100, ..mix_to(&p, &dest) }).unwrap());
    assert_eq!(j.state, JobStateRow::Failed, "{j:?}");
    assert!(j.message.as_deref().is_some_and(|m| m.contains("system importer")), "{j:?}");
    assert!(!dest.exists() && !p.dir.path().join(".44k1.wav.partial").exists());
    assert!(incoming(&p).is_empty());
}
VIZDAW_EOF
```

  (If a name differs in the tree — `SettingsCmd::SetFormat`'s fields, `LaneEvents.events`, `EditCommand::Sequence` — use the tree's and say so. The file builds its documents with the fixtures' `Builder`, as `tests/plugins.rs` does.)

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-ffi --test bounce`). Expected: FAIL — `bounce` not found.

- [ ] **Step 3: Implement** as above: `bounce.rs` (the rows, the settings, `stem_paths`, the destination checks, the warning sentences and the job body), the `jobs.rs`, `plugins.rs`, `session.rs` and `lib.rs` edits, the Swift edits and the `FakeSession` stubs.

- [ ] **Step 4: Run the tests and the gates** (the workspace; FFI; Swift). Expected: core-ffi +17 on macOS (+17 on Linux, with the other cfg test); Swift unchanged in count.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/bounce.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/jobs.rs crates/core-ffi/src/plugins.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/bounce.rs app/Sources/VizDAWAppModel/ProjectMedia.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: the Bounce job (plugin copies retired however it ends, MIDI silent, imports then partials then renames, Add to project as one step, stems numbered, nothing left on cancel) (plan 5f-2 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Rust +17 (Linux too).

---

### Task 5: `file.bounce` on the channel

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/tests/commands.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/ProjectMedia.swift`, `app/Sources/VizDAWAppModel/SurfaceRelay.swift`, `app/Sources/VizDAW/ControlChannel.swift`
- Create: `app/Sources/VizDAWAppModel/BounceCommands.swift`, `app/Tests/VizDAWAppModelTests/BounceCommandsTests.swift`

**Interfaces:**
- Produces: `BounceCommands.id = "file.bounce"`, `BounceCommands.ids`; `BounceCommands.request(_ args: [ControlArg], _ m: DocumentModel) -> Result<BounceRequestRow, ControlFailure>`; `BounceCommands.status(_ r: BounceResultRow, problems: [String]) -> String`; `ControlJobs.answer(_ j: JobRow, bounce: BounceResultRow? = nil)` (Ruling 10).

In `registry.rs`, `BOUNCE_PARAMS` goes beside the other `*_PARAMS` consts, after `IMPORT_PARAMS` (pre-flight M16):

Find:
```rust
/// A media item by ID (plan 5b).
```
Replace:
```rust
/// `file.bounce` (plan 5f-2 Ruling 2): the Bounce dialog's answers.
const BOUNCE_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "source",
        kind: ParamKind::Choice(&["mix", "stems", "selection"]),
        required: false,
        help: "mix (default), stems (one file per strip in `strips`) or selection (the mix over the time selection).",
    },
    ParamSpec {
        name: "strips",
        kind: ParamKind::Text,
        required: false,
        help: "For stems: strip IDs, comma-separated (see state.get mixer).",
    },
    ParamSpec {
        name: "range",
        kind: ParamKind::Choice(&["song", "selection"]),
        required: false,
        help: "song (default) or selection (`start` and `end`, else the editor's time selection).",
    },
    ParamSpec {
        name: "start",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The selection's first tick (with `range: selection`).",
    },
    ParamSpec {
        name: "end",
        kind: ParamKind::Int {
            min: 0,
            max: i64::MAX / 2,
        },
        required: false,
        help: "The selection's end tick, exclusive (with `range: selection`).",
    },
    ParamSpec {
        name: "format",
        kind: ParamKind::Choice(&["wav", "aiff", "caf"]),
        required: false,
        help: "wav (default), aiff or caf.",
    },
    ParamSpec {
        name: "bits",
        kind: ParamKind::Choice(DEPTHS),
        required: false,
        help: "16, 24 or 32f (float); default the project's depth.",
    },
    ParamSpec {
        name: "rate",
        kind: ParamKind::Int {
            min: 8_000,
            max: 384_000,
        },
        required: false,
        help: "8000–384000; the project's rate by default.",
    },
    ParamSpec {
        name: "dither",
        kind: ParamKind::Flag,
        required: false,
        help: "TPDF dither at 16 and 24 bits; default on for 16 bits; float ignores it.",
    },
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The mix file, or an existing folder for stems (answers the Bounce dialog); stems never replace: taken names are numbered.",
    },
    ParamSpec {
        name: "add_to_project",
        kind: ParamKind::Flag,
        required: false,
        help: "Also place the bounce on new lanes (default false; answers the Bounce dialog). It bounces as heard: a muted or solo-silenced strip, or one routed into a muted bus, is silent; files ignore mute and solo.",
    },
    ParamSpec {
        name: "replace",
        kind: ParamKind::Flag,
        required: false,
        help: "Replace an existing mix file (default false); the old file stays until the new one is complete.",
    },
];

/// A media item by ID (plan 5b).
```

and the command after `file.project_settings`:

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

In `crates/core-control/tests/commands.rs`, the count (pre-flight I12):

Find:
```rust
        144,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four; 5c adds record.set, record.recover, lane.set_input, lane.set_monitor and midi.inject; 5c-2 adds lane.track_synth, destination.set_return_latency and destination.measure_return; 5d-1 adds audio.set and destination.add_midi; 5e-1 adds automation.set_mode, automation.gesture and automation.suspend; 5e-2 adds the six automation.mode commands; 5f-1 adds transport.play, surface.add, surface.remove, surface.inject, surface.learn and surface.unlearn"
```
Replace:
```rust
        145,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four; 5c adds record.set, record.recover, lane.set_input, lane.set_monitor and midi.inject; 5c-2 adds lane.track_synth, destination.set_return_latency and destination.measure_return; 5d-1 adds audio.set and destination.add_midi; 5e-1 adds automation.set_mode, automation.gesture and automation.suspend; 5e-2 adds the six automation.mode commands; 5f-1 adds transport.play, surface.add, surface.remove, surface.inject, surface.learn and surface.unlearn; 5f-2 adds file.bounce"
```

`range: selection` without `start`/`end` uses the editor's time selection, else is refused ("no time selection"). `source: selection` is mix plus `range: selection`. In `ControlCommands.swift`, the answer (Ruling 10, pre-flight I7, M10):

Find:
```swift
    public static func answer(_ j: JobRow) -> JobAnswer {
        switch j.state {
        case .running: return .wait
        case .done:
            if j.kind != .format, j.created.isEmpty, !j.problems.isEmpty {
                return .failed(j.problems.joined(separator: "; "))
            }
            return .ok(status: j.problems.isEmpty ? nil : "warnings: " + j.problems.joined(separator: "; "))
```
Replace:
```swift
    public static func answer(_ j: JobRow, bounce: BounceResultRow? = nil) -> JobAnswer {
        switch j.state {
        case .running: return .wait
        case .done:
            // A bounce wrote its files even when it has problems (clipped
            // samples, a bypassed plugin): ok, with warnings (plan 5f-2
            // Ruling 10).
            if j.kind == .bounce {
                return .ok(status: BounceCommands.status(bounce ?? BounceResultRow(files: [], media: []), problems: j.problems))
            }
            if j.kind != .format, j.created.isEmpty, !j.problems.isEmpty {
                return .failed(j.problems.joined(separator: "; "))
            }
            return .ok(status: j.problems.isEmpty ? nil : "warnings: " + j.problems.joined(separator: "; "))
```

the handled set and the dispatch:

Find:
```swift
            .union(AutomationCommands.ids).union(SurfaceCommands.ids)
```
Replace:
```swift
            .union(AutomationCommands.ids).union(SurfaceCommands.ids).union(BounceCommands.ids)
```

Find:
```swift
        case "window.mixer":
```
Replace:
```swift
        case BounceCommands.id:
            switch BounceCommands.request(args, m) {
            case .failure(let f):
                return .failed(f)
            case .success(let req):
                do {
                    return .job(command: id, doc: docID, job: try m.session.bounce(req: req))
                } catch {
                    return .failed(.thrown(error, "Bounce Failed"))
                }
            }
        case "window.mixer":
```

`ProjectMedia.swift`'s job polling answers a bounce the same way:

Find:
```swift
            switch ControlJobs.answer(r) {
```
Replace:
```swift
            let bounced = r.kind == .bounce ? (try? session.bounceResult(job: r.id)) ?? nil : nil
            switch ControlJobs.answer(r, bounce: bounced) {
```

`ControlChannel.swift`:

Find:
```swift
            switch ControlJobs.answer(j) {
```
Replace:
```swift
            let bounced = j.kind == .bounce ? (try? m.session.bounceResult(job: job)) ?? nil : nil
            switch ControlJobs.answer(j, bounce: bounced) {
```

`SurfaceRelay.swift` (the Bounce sheet is a dialog):

Find:
```swift
        "media.import", "media.relink", "media.rename", "record.recover", RegionCommands.normalize,
```
Replace:
```swift
        "media.import", "media.relink", "media.rename", "record.recover", RegionCommands.normalize,
        BounceCommands.id,
```

`BounceCommands.swift` (whole file):

```swift
import Foundation
import VizDAWCore

/// `file.bounce` over the control channel (plan 5f-2 Task 5; Rulings 2, 3,
/// 10): the Bounce dialog's answers as parameters, checked here before the
/// core sees them; the core checks the destinations (Ruling 3).
public enum BounceCommands {
    public static let id = "file.bounce"
    public static let ids: Set<String> = [id]

    /// The request `args` ask for on `m`'s shown sequence: WAV at the
    /// project's rate and depth by default, dither on at 16 bits (Ruling 11).
    public static func request(_ args: [ControlArg], _ m: DocumentModel) -> Result<BounceRequestRow, ControlFailure> {
        func bad(_ why: String) -> Result<BounceRequestRow, ControlFailure> {
            .failure(.of(.invalidParams, "file.bounce: \(why)"))
        }
        let path = args.text("path")
        let add = args.flag("add_to_project") ?? false
        // Presence answered the registry's dialog check; `add_to_project:
        // false` alone still needs the dialog (pre-flight M6).
        guard path != nil || add else { return .failure(.dialog("Bounce", "path")) }
        guard let seq = m.selectedSeq else { return .failure(.of(.failed, "file.bounce: no sequence is shown")) }
        let base = (try? m.session.projectInfo().format) ?? ProjectFormatRow(sampleRate: 48_000, depth: .int24)
        let source = args.text("source") ?? "mix"
        if source == "selection", args.text("range") == "song" {
            return bad("`source: selection` bounces the time selection; leave out `range: song`")
        }
        let container: ContainerRow
        switch args.text("format") {
        case "aiff": container = .aiff
        case "caf": container = .caf
        default: container = .wav
        }
        let depth: BitDepthRow
        switch args.text("bits") {
        case "16": depth = .int16
        case "24": depth = .int24
        case "32f": depth = .float32
        default: depth = base.depth
        }
        let rate = args.int("rate").flatMap { UInt32(exactly: $0) } ?? base.sampleRate
        let dither = depth == .float32 ? false : (args.flag("dither") ?? (depth == .int16))
        var src = BounceSourceRow.mix
        if source == "stems" {
            guard let text = args.text("strips") else { return bad("stems need `strips` (strip IDs, e.g. \"1,2\")") }
            var ids: [UInt64] = []
            for part in text.split(separator: ",", omittingEmptySubsequences: false) {
                guard !part.isEmpty, part.unicodeScalars.allSatisfy({ ("0"..."9").contains($0) }), let n = UInt64(part) else {
                    return bad("`strips` is strip IDs separated by commas, e.g. \"1,2\"")
                }
                guard !ids.contains(n) else { return bad("strip \(n) is listed twice") }
                ids.append(n)
            }
            src = .stems(strips: ids)
        } else if let p = path {
            let ext = (p as NSString).pathExtension.lowercased()
            let fits: Bool
            switch container {
            case .wav: fits = ext == "wav"
            case .aiff: fits = ext == "aif" || ext == "aiff"
            case .caf: fits = ext == "caf"
            }
            guard fits else { return bad("`path` must end in .\(container == .aiff ? "aif" : "\(container)") for this format") }
        }
        let range: BounceRangeRow
        if source == "selection" || args.text("range") == "selection" {
            switch (args.int("start"), args.int("end")) {
            case let (s?, e?):
                guard s < e else { return bad("`start` must be before `end`") }
                range = .ticks(start: s, end: e)
            case (nil, nil):
                guard let t = m.editor.selection.timeRange, t.start < t.end else { return bad("no time selection") }
                range = .ticks(start: t.start, end: t.end)
            default:
                return bad("give both `start` and `end`, or neither")
            }
        } else {
            guard args.int("start") == nil, args.int("end") == nil else {
                return bad("`start` and `end` go with `range: selection`")
            }
            range = .song
        }
        return .success(
            BounceRequestRow(
                seq: seq, source: src, range: range, container: container, depth: depth, rate: rate,
                dither: dither, path: path, addToProject: add, replace: args.flag("replace") ?? false))
    }

    /// Ruling 10's status: each file, the media added, the warnings.
    public static func status(_ r: BounceResultRow, problems: [String]) -> String {
        var parts = r.files.map { "wrote \($0.path) (\($0.frames) frames, \($0.rate) Hz, \($0.bits))" }
        if !r.media.isEmpty { parts.append("added \(r.media.count) media") }
        if !problems.isEmpty { parts.append("warnings: " + problems.joined(separator: "; ")) }
        return parts.joined(separator: "; ")
    }
}
```

- [ ] **Step 1: Write the failing tests.**

  Append to `crates/core-control/tests/commands.rs`:

```rust
/// Plan 5f-2 Task 5: `file.bounce` takes the Bounce dialog's answers as
/// parameters; hostile values are refused by type and range before they
/// reach the app (strip lists are text here; the app checks their form).
#[test]
fn bounce_commands_check_their_params() {
    let run = |params: Value| json!({ "id": "file.bounce", "params": params });
    for good in [
        json!({ "source": "mix", "path": "/tmp/x.wav" }),
        json!({ "add_to_project": true }),
        json!({ "source": "stems", "strips": "1,2", "path": "/tmp/stems" }),
        json!({ "range": "selection", "start": 0, "end": 15360, "path": "/tmp/x.aif", "format": "aiff", "bits": "16", "rate": 44100, "dither": false }),
        json!({ "path": "/tmp/x.wav", "replace": true, "bits": "32f", "rate": 8000 }),
        // Presence answers the dialog here; the app refuses this one (pre-flight M6).
        json!({ "add_to_project": false }),
        json!({ "source": "stems", "strips": "1,,2", "path": "/tmp/stems" }),
    ] {
        assert!(
            matches!(ok("command.run", run(good.clone())), Command::CommandRun { .. }),
            "{good}"
        );
    }
    for bad in [
        json!({ "path": "/tmp/x.wav", "format": "mp3" }),
        json!({ "path": "/tmp/x.wav", "bits": "8" }),
        json!({ "path": "/tmp/x.wav", "bits": 16 }),
        json!({ "path": "/tmp/x.wav", "rate": 1000 }),
        json!({ "path": "/tmp/x.wav", "rate": 384_001 }),
        json!({ "path": "relative.wav" }),
        json!({ "path": "/tmp/x.wav", "start": -1 }),
        json!({ "path": "/tmp/x.wav", "end": i64::MAX }),
        json!({ "path": "/tmp/x.wav", "source": "both" }),
        json!({ "path": "/tmp/x.wav", "dither": "yes" }),
        json!({ "path": format!("/tmp/{}.wav", "a".repeat(1100)) }),
        json!({ "path": "/tmp/x.wav", "seq": 1 }),
    ] {
        let (code, _) = err("command.run", run(bad.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
    }
    let (code, message) = err("command.run", run(json!({})));
    assert_eq!(code, ErrorCode::DialogRequired);
    assert!(message.contains("Bounce"), "{message}");
}
```

  Create `app/Tests/VizDAWAppModelTests/BounceCommandsTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// `file.bounce` over the control channel (plan 5f-2 Task 5; Rulings 2, 3,
/// 10, 11; pre-flight I7, M6): the request as built, hostile arguments as
/// `invalid_params`, and the answer with the files and the warnings.
@MainActor
@Suite struct BounceCommandsTests {
    let host = FakeControlHost()

    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        let id = try #require(s.newDemo())
        return (s, id, try #require(s.model(id)), f)
    }

    func run(_ pairs: [(String, ControlValue)], _ m: DocumentModel, _ s: DocumentStore, _ doc: UUID)
        -> ControlOutcome
    {
        ControlCommands.run(
            BounceCommands.id, args: pairs.map { ControlArg(name: $0.0, value: $0.1) }, doc: (doc, m), store: s,
            host: host)
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    @Test func bounceCommandsCheckTheirArguments() throws {
        let (s, id, m, f) = try demo()
        let wav: (String, ControlValue) = ("path", .text(value: "/tmp/x.wav"))
        let folder: (String, ControlValue) = ("path", .text(value: "/tmp/stems"))
        let stems: (String, ControlValue) = ("source", .text(value: "stems"))
        let selection: (String, ControlValue) = ("range", .text(value: "selection"))
        for args: [(String, ControlValue)] in [
            [("path", .text(value: "/tmp/x.aif"))],
            [wav, ("format", .text(value: "caf"))],
            [stems, folder],
            [stems, folder, ("strips", .text(value: "1,,2"))],
            [stems, folder, ("strips", .text(value: "1, 2"))],
            [stems, folder, ("strips", .text(value: "-1"))],
            [stems, folder, ("strips", .text(value: "a,b"))],
            [stems, folder, ("strips", .text(value: "1,1"))],
            [wav, selection, ("start", .int(value: 0))],
            [wav, selection, ("start", .int(value: 100)), ("end", .int(value: 100))],
            [wav, ("source", .text(value: "selection")), ("range", .text(value: "song"))],
            [wav, ("start", .int(value: 0)), ("end", .int(value: 100))],
            [wav, selection],
        ] {
            #expect(failure(run(args, m, s, id))?.code == .invalidParams, "\(args)")
        }
        let d = failure(run([("add_to_project", .flag(value: false))], m, s, id))
        #expect(d?.code == .dialogRequired && d?.dialog == "Bounce", "add_to_project false alone needs the dialog")
        #expect(f.bounces.isEmpty, "nothing reached the core")
    }

    @Test func bounceStartsAJobAndAnswersWithTheFiles() throws {
        let (s, id, m, f) = try demo()
        f.info.format = ProjectFormatRow(sampleRate: 44_100, depth: .int16)
        let o = run([("path", .text(value: "/tmp/x.wav"))], m, s, id)
        guard case .job(let command, let doc, let job) = o else {
            Issue.record("not a job: \(o)")
            return
        }
        #expect(command == "file.bounce" && doc == id)
        let seq = try #require(m.selectedSeq)
        #expect(
            f.bounces.last
                == BounceRequestRow(
                    seq: seq, source: .mix, range: .song, container: .wav, depth: .int16, rate: 44_100, dither: true,
                    path: "/tmp/x.wav", addToProject: false, replace: false),
            "the project's rate and depth; dither on at 16 bits")
        _ = run(
            [
                ("source", .text(value: "stems")), ("strips", .text(value: "2,1")), ("path", .text(value: "/tmp/stems")),
                ("format", .text(value: "aiff")), ("bits", .text(value: "32f")), ("rate", .int(value: 96_000)),
                ("dither", .flag(value: true)), ("range", .text(value: "selection")), ("start", .int(value: 0)),
                ("end", .int(value: 15_360)), ("add_to_project", .flag(value: true)), ("replace", .flag(value: true)),
            ], m, s, id)
        let all = try #require(f.bounces.last)
        #expect(all.source == .stems(strips: [2, 1]) && all.range == .ticks(start: 0, end: 15_360))
        #expect(all.container == .aiff && all.depth == .float32 && all.rate == 96_000 && !all.dither, "float ignores dither")
        #expect(all.addToProject && all.replace)
        _ = run([("source", .text(value: "selection")), wavArg(), ("start", .int(value: 10)), ("end", .int(value: 20))], m, s, id)
        #expect(f.bounces.last?.range == .ticks(start: 10, end: 20) && f.bounces.last?.source == .mix)
        // Ruling 10's answer; a bounce with problems is ok, with warnings (pre-flight I7).
        let file = BouncedFileRow(path: "/tmp/x.wav", frames: 96_000, rate: 48_000, bits: "24-bit", channels: 2, clipped: 0)
        var done = JobRow(id: job, kind: .bounce, progress: 1, state: .done, message: nil, problems: [], created: [])
        #expect(
            ControlJobs.answer(done, bounce: BounceResultRow(files: [file], media: []))
                == .ok(status: "wrote /tmp/x.wav (96000 frames, 48000 Hz, 24-bit)"))
        done.problems = ["x.wav: 12 samples clipped"]
        #expect(
            ControlJobs.answer(done, bounce: BounceResultRow(files: [file], media: [7]))
                == .ok(status: "wrote /tmp/x.wav (96000 frames, 48000 Hz, 24-bit); added 1 media; warnings: x.wav: 12 samples clipped"))
        let imported = JobRow(id: 1, kind: .import, progress: 1, state: .done, message: nil, problems: ["a.wav: unreadable"], created: [])
        #expect(ControlJobs.answer(imported) == .failed("a.wav: unreadable"), "an import that made nothing still fails")
        // The core's refusals are the call's failure.
        f.bounceError = VizError.Model(message: "x.wav exists; choose another name or replace it")
        #expect(failure(run([wavArg()], m, s, id))?.code == .failed)
        f.bounceError = VizError.Busy(message: "A bounce is running.")
        #expect(failure(run([wavArg()], m, s, id))?.code == .busy)
    }

    func wavArg() -> (String, ControlValue) { ("path", .text(value: "/tmp/x.wav")) }
}
```

- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.

- [ ] **Step 3: Implement.**

- [ ] **Step 4: Run the tests and the gates.** Expected: core-control +1, Swift +2, and the registry at 145.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAWAppModel/BounceCommands.swift app/Sources/VizDAWAppModel/ProjectMedia.swift app/Sources/VizDAWAppModel/SurfaceRelay.swift app/Sources/VizDAW/ControlChannel.swift app/Tests/VizDAWAppModelTests/BounceCommandsTests.swift
git commit -m "control: file.bounce waits for the bounce and answers with the files and its warnings (plan 5f-2 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Rust +1, Swift +2.

---

### Task 6: The Bounce sheet, File › Bounce…, the tail setting

**Files:**
- Create: `app/Sources/VizDAWAppModel/BounceModel.swift`, `app/Sources/VizDAW/BounceSheet.swift`, `app/Tests/VizDAWAppModelTests/BounceModelTests.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/DocumentView.swift` (present the sheet), `app/Sources/VizDAWAppModel/AudioPreferences.swift` (`bounceTailSeconds`, `AudioSink.bounceTail`, `applyAll`, `setBounceTail`), `app/Sources/VizDAW/AudioSettingsView.swift`, `app/Sources/VizDAW/AudioSetup.swift` (`LiveAudioSink.bounceTail`), `app/Sources/VizDAWAppModel/DocumentModel.swift` (`bounce: BounceModel`), `app/Tests/VizDAWAppModelTests/AudioPreferencesTests.swift` (`FakeAudioSink.bounceTail`, the launch list)

(`AudioReadoutsTests.swift` uses `AudioPreferencesTests.swift`'s `FakeAudioSink` and needs no change.)

**Interfaces:**
- Produces:
  ```swift
  @MainActor @Observable public final class BounceModel {
      public enum Source: Equatable, Sendable { case mix, stems }
      public init(document: DocumentModel)
      /// Showing resets the choices to the project's defaults (Ruling 11).
      public var shown: Bool
      public var source: Source               // .mix
      public var stems: Set<UInt64>
      public var useSelection: Bool           // only with a time selection
      public var hasSelection: Bool { get }
      public var container: ContainerRow      // .wav
      public var depth: BitDepthRow           // the project's
      public var rate: UInt32                 // the project's
      public var dither: Bool                 // true when depth is .int16; ignored for float
      public var addToProject: Bool
      /// Ruling 8's sentences (from `bounceCheck`), refreshed on every change.
      public private(set) var warning: String?
      /// The files the bounce would write (`bounceCheck.files`): for stems, the
      /// numbered names it will take in the chosen folder (owner answer 2).
      public private(set) var files: [String]
      public private(set) var length: String  // "1:32.4 + tail"
      /// The request for `path` (a file, a folder for stems, nil: add only).
      /// `replace` only for a mix (the save panel's confirmation).
      public func request(path: String?, replace: Bool) -> BounceRequestRow?
      /// Refreshes `warning`, `files` and `length` for `path`.
      public func check(path: String?)
      /// Starts the job (`DocumentModel.startJob(.bounce, "Bounce Failed")`).
      public func start(path: String?, replace: Bool)
      public private(set) var job: UInt64?    // progress through DocumentModel.jobs
      public var progress: Float? { get }
      public func cancel()
      public static let rates: [UInt32]       // the six project rates (Ruling 11)
  }
  // AudioPreferences.swift
  public var bounceTailSeconds: UInt32 = 10   // AudioSettings; tolerant decode: out of 0–30 → 10
  func bounceTail(ms: UInt32)                 // AudioSink
  public func setBounceTail(seconds: UInt32)  // AudioPreferences; clamped 0–30
  ```

Behaviour:
- `DocumentModel` owns `bounce` (lazy, as `media`).
- The sheet has the source picker, a strip checklist for stems, "Time selection only", format, bits, rate (with "Other…" as a field from 8,000 to 384,000), Dither (disabled for float), "Add to project", the warning line, the files line, and Bounce/Cancel. Under the stems checklist a caption says Ruling 5: "Files ignore mute and solo. Add to project bounces as heard: muted and solo-silenced strips, and strips routed into a muted bus, are silent." Under it, for stems: "Files already in the folder are kept; new stems take the next free number."
- Bounce opens an `NSSavePanel` (a mix file; its own replace confirmation means `replace: true`) or an `NSOpenPanel` set to folders only (stems; no replace prompt: stems are numbered, owner answer 2, and the sheet shows `files` from `check(path:)` before it starts), unless only "Add to project" is on.
- While the job runs, the sheet shows progress and Cancel. On Done it closes (warnings go to the Problems banner, as for imports); on Failed it alerts "Bounce Failed".
- The sheet is not shown while the control channel is active (as `RelinkSheet`).
- `AppCommands` adds `Button("Bounce…") { doc?.bounce.shown = true }.disabled(doc == nil)` after "Project Settings…".
- Settings › Audio gains a "Bounce" section with a "Bounce tail limit" stepper, 0–30 s (default 10), stored in `AudioPreferences` and applied through `setBounceSettings` on every change **and at launch** (`applyAll`, pre-flight I10).

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

Find (DocumentView.swift):
```swift
            RelinkSheet(media: model.media)
        }
```
Replace:
```swift
            RelinkSheet(media: model.media)
        }
        .sheet(
            isPresented: Binding(
                get: { model.bounce.shown && !ControlChannel.active }, set: { model.bounce.shown = $0 })
        ) {
            BounceSheet(bounce: model.bounce)
        }
```

Find (DocumentModel.swift):
```swift
    @ObservationIgnored private var mediaModel: MediaModel?
```
Replace:
```swift
    @ObservationIgnored private var mediaModel: MediaModel?
    @ObservationIgnored private var bounceModel: BounceModel?
```

Find (DocumentModel.swift):
```swift
    /// The media pool (plan 5b), made once.
```
Replace:
```swift
    /// File › Bounce… (plan 5f-2), made once.
    public var bounce: BounceModel {
        if let m = bounceModel { return m }
        let m = BounceModel(document: self)
        bounceModel = m
        return m
    }

    /// The media pool (plan 5b), made once.
```

In `AudioPreferences.swift`, the setting:

Find:
```swift
    public init() {}

    enum CodingKeys: String, CodingKey {
        case softwareMonitoring, bufferFrames, inputDevice, latencyTrim, cacheMb, readAheadSeconds
        case defaultMonitor, countInBars, preRollBars, passesAsTakes
    }
```
Replace:
```swift
    /// The bounce's longest tail in seconds, 0–30 (plan 5f-2 Ruling 4).
    public var bounceTailSeconds: UInt32 = 10

    public init() {}

    enum CodingKeys: String, CodingKey {
        case softwareMonitoring, bufferFrames, inputDevice, latencyTrim, cacheMb, readAheadSeconds
        case defaultMonitor, countInBars, preRollBars, passesAsTakes, bounceTailSeconds
    }
```

Find:
```swift
        passesAsTakes = c.field(.passesAsTakes, d.passesAsTakes)
```
Replace:
```swift
        passesAsTakes = c.field(.passesAsTakes, d.passesAsTakes)
        let tail: UInt32 = c.field(.bounceTailSeconds, d.bounceTailSeconds)
        bounceTailSeconds = tail <= 30 ? tail : d.bounceTailSeconds
```

Find:
```swift
    func defaultMonitor(_ mode: MonitorModeRow)
}
```
Replace:
```swift
    func defaultMonitor(_ mode: MonitorModeRow)
    /// The bounce's tail limit (plan 5f-2).
    func bounceTail(ms: UInt32)
}
```

Find:
```swift
        sink.defaultMonitor(s.monitorRow)
        RecordModel.defaults = RecordDefaults(s)
```
Replace:
```swift
        sink.defaultMonitor(s.monitorRow)
        sink.bounceTail(ms: s.bounceTailSeconds * 1000)
        RecordModel.defaults = RecordDefaults(s)
```

Find:
```swift
    /// Settings › Recording's defaults for new documents' record options.
```
Replace:
```swift
    /// Settings › Audio's bounce tail limit, clamped to 0–30 s (plan 5f-2).
    public func setBounceTail(seconds: UInt32) {
        var s = settings
        s.bounceTailSeconds = min(seconds, 30)
        sink.bounceTail(ms: s.bounceTailSeconds * 1000)
        save(s)
    }

    /// Settings › Recording's defaults for new documents' record options.
```

In `AudioSetup.swift`:

Find:
```swift
    func defaultMonitor(_ mode: MonitorModeRow) { setDefaultMonitorMode(mode: mode) }
```
Replace:
```swift
    func defaultMonitor(_ mode: MonitorModeRow) { setDefaultMonitorMode(mode: mode) }
    func bounceTail(ms: UInt32) { setBounceSettings(settings: BounceSettingsRow(tailCapMs: ms)) }
```

In `AudioSettingsView.swift`, before the Streaming section:

Find:
```swift
            Section("Streaming") {
```
Replace:
```swift
            Section("Bounce") {
                Stepper(
                    "Bounce tail limit: \(prefs.settings.bounceTailSeconds) s",
                    value: Binding(
                        get: { prefs.settings.bounceTailSeconds }, set: { prefs.setBounceTail(seconds: $0) }),
                    in: 0...30)
                Text("A bounce renders on after the range until it is quiet, at most this long.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Section("Streaming") {
```

In `AudioPreferencesTests.swift`, the fake and the launch list:

Find:
```swift
    func defaultMonitor(_ mode: MonitorModeRow) { calls.append("monitor \(mode)") }
```
Replace:
```swift
    func defaultMonitor(_ mode: MonitorModeRow) { calls.append("monitor \(mode)") }
    func bounceTail(ms: UInt32) { calls.append("bounce tail \(ms)") }
```

Find:
```swift
                "monitoring true", "input same", "trim 0", "monitor auto", "buffer 128", "stream 512 3",
```
Replace:
```swift
                "monitoring true", "input same", "trim 0", "monitor auto", "bounce tail 10000", "buffer 128",
                "stream 512 3",
```

- [ ] **Step 1: Write the failing tests**

  Create `app/Tests/VizDAWAppModelTests/BounceModelTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The Bounce sheet's model (plan 5f-2 Task 6; Rulings 3, 8, 11; owner
/// answer 2) and the tail setting (pre-flight I10).
@MainActor
@Suite(.serialized) struct BounceModelTests {
    func model(_ f: FakeSession) throws -> DocumentModel { try DocumentModel(session: f) }

    @Test func defaultsFollowTheProject() throws {
        let f = demoFake()
        f.info.format = ProjectFormatRow(sampleRate: 44_100, depth: .int16)
        let m = try model(f)
        m.bounce.shown = true
        let b = m.bounce
        #expect(b.source == .mix && b.container == .wav && b.rate == 44_100 && b.depth == .int16 && b.dither)
        #expect(!b.addToProject && !b.useSelection)
        b.depth = .int24
        #expect(b.dither, "kept as set")
        #expect(b.request(path: "/tmp/x.wav", replace: false)?.dither == true)
        b.depth = .float32
        #expect(b.request(path: "/tmp/x.wav", replace: false)?.dither == false, "float ignores dither")
        #expect(BounceModel.rates == [44_100, 48_000, 88_200, 96_000, 176_400, 192_000])
        // Showing again starts from the project's defaults.
        f.info.format = ProjectFormatRow(sampleRate: 96_000, depth: .int24)
        m.bounce.shown = false
        m.bounce.shown = true
        #expect(m.bounce.rate == 96_000 && m.bounce.depth == .int24 && !m.bounce.dither)
    }

    @Test func theWarningComesFromTheCore() throws {
        let f = demoFake()
        f.checkRow = BounceCheckRow(
            externalLanes: 1, frames: 96_000, files: [],
            warning: "1 lane plays external MIDI and will be silent. Track it as audio first: Track Synth…")
        let m = try model(f)
        m.bounce.shown = true
        #expect(m.bounce.warning == f.checkRow.warning)
        let asked = f.checkRequests.count
        m.bounce.source = .stems
        m.bounce.stems = [1]
        #expect(f.checkRequests.count > asked, "refetched after a change")
        #expect(f.checkRequests.last?.source == .stems(strips: [1]))
    }

    @Test func startBuildsTheRequest() throws {
        let f = demoFake()
        f.jobStartState = .running
        let m = try model(f)
        m.bounce.shown = true
        m.bounce.source = .stems
        m.bounce.stems = [2, 1]
        m.bounce.start(path: "/tmp/stems", replace: false)
        let r = try #require(f.bounces.last)
        #expect(r.source == .stems(strips: [1, 2]) && r.path == "/tmp/stems" && !r.addToProject)
        #expect(m.bounce.job != nil && m.jobs.contains { $0.kind == .bounce })
        m.bounce.cancel()
        #expect(f.cancelled.last == m.bounce.job)
        m.bounce.source = .mix
        m.bounce.addToProject = true
        m.bounce.start(path: nil, replace: false)
        let add = try #require(f.bounces.last)
        #expect(add.path == nil && add.addToProject && add.source == .mix)
    }

    @Test func aRefusedStartIsAnAlert() throws {
        let f = demoFake()
        f.bounceError = VizError.Model(message: "x.wav exists; choose another name or replace it")
        let m = try model(f)
        m.bounce.shown = true
        m.bounce.start(path: "/tmp/x.wav", replace: false)
        #expect(m.alert?.title == "Bounce Failed")
        #expect(m.bounce.job == nil)
    }

    /// Owner answer 2: stems never replace; the sheet shows the numbered
    /// names the core will take, and asks nothing.
    @Test func existingStemFilesAreNumberedNotReplaced() throws {
        let f = demoFake()
        f.checkRow = BounceCheckRow(externalLanes: 0, frames: 96_000, files: ["/tmp/s/Bass 2.wav"], warning: nil)
        let m = try model(f)
        m.bounce.shown = true
        m.bounce.source = .stems
        m.bounce.stems = [1]
        m.bounce.check(path: "/tmp/s")
        #expect(m.bounce.files == ["/tmp/s/Bass 2.wav"])
        #expect(f.checkRequests.last?.path == "/tmp/s")
        m.bounce.start(path: "/tmp/s", replace: true)
        #expect(f.bounces.last?.replace == false, "stems never replace")
    }

    /// Pre-flight I10: kept, clamped, applied on change and at launch.
    @Test func tailSettingIsKeptAndApplied() throws {
        let defaults = MemoryDefaults()
        let sink = FakeAudioSink()
        let p = AudioPreferences(defaults: defaults, sink: sink)
        #expect(p.settings.bounceTailSeconds == 10)
        p.setBounceTail(seconds: 4)
        #expect(sink.calls.last == "bounce tail 4000" && p.settings.bounceTailSeconds == 4)
        p.setBounceTail(seconds: 99)
        #expect(sink.calls.last == "bounce tail 30000" && p.settings.bounceTailSeconds == 30)
        let again = FakeAudioSink()
        let q = AudioPreferences(defaults: defaults, sink: again)
        #expect(q.settings.bounceTailSeconds == 30, "read back from the preferences")
        q.applyAll()
        #expect(again.calls.contains("bounce tail 30000"), "applied at launch")
        // A damaged value keeps the default.
        let bad = MemoryDefaults()
        bad.set(Data(#"{"bounceTailSeconds": 999}"#.utf8), forKey: AudioPreferences.key)
        #expect(AudioPreferences(defaults: bad, sink: FakeAudioSink()).settings.bounceTailSeconds == 10)
    }
}
```

- [ ] **Step 2: Run them to see them fail.** Expected: FAIL.

- [ ] **Step 3: Implement** the model, the sheet, the menu item and the setting. `BounceModel` keeps its choices; every `didSet` calls `check(path: lastPath)` through `document.session.bounceCheck(req:)` (a failure clears `warning` and `files`; the sheet shows the core's message on Bounce). `request` builds the row as `BounceCommands.request` does (seq `document.selectedSeq`; `.stems(strips: stems.sorted())`; the editor's time selection when `useSelection`; `replace` only for a mix). `start` goes through `document.startJob(.bounce, "Bounce Failed") { try $0.bounce(req: r) }` and keeps the new job's ID; `cancel` is `document.session.cancelJob(id:)`; `progress` reads `document.jobs`.

- [ ] **Step 4: Run the tests and the gates.** Expected: Swift +6.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/BounceModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/AudioPreferences.swift app/Sources/VizDAW/BounceSheet.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/DocumentView.swift app/Sources/VizDAW/AudioSettingsView.swift app/Sources/VizDAW/AudioSetup.swift app/Tests/VizDAWAppModelTests/BounceModelTests.swift app/Tests/VizDAWAppModelTests/AudioPreferencesTests.swift
git commit -m "app: File > Bounce... sheet with the external-MIDI warning, numbered stem names, progress and cancel; Settings > Audio bounce tail limit, applied at launch (plan 5f-2 Task 6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -12
```
Counts: Swift +6.

---

### Task 7: The bounce e2e, README 180–184, the backlog row, the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`, `docs/feature-backlog.md`

- [ ] **Step 1: Write the e2e.** Append to `crates/vizctl/tests/e2e.rs` (pre-flight C2: the demo's `Media/loop.wav` does not exist and the offline synth is silent, so the test imports `tone.caf` and bounces four bars, which are all at 120 BPM before the Song's ritardando):

```rust
/// `(rate, channels, bits, frames, 16-bit samples)` of a 16-bit PCM WAV.
fn read_wav16(path: &std::path::Path) -> (u32, u16, u16, u64, Vec<i16>) {
    let b = std::fs::read(path).unwrap();
    assert_eq!((&b[0..4], &b[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    let mut at = 12;
    let (mut rate, mut ch, mut bits, mut data) = (0, 0, 0, Vec::new());
    while at + 8 <= b.len() {
        let n = u32::from_le_bytes(b[at + 4..at + 8].try_into().unwrap()) as usize;
        let body = &b[at + 8..at + 8 + n];
        match &b[at..at + 4] {
            b"fmt " => {
                ch = u16::from_le_bytes(body[2..4].try_into().unwrap());
                rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                bits = u16::from_le_bytes(body[14..16].try_into().unwrap());
            }
            b"data" => {
                data = body
                    .chunks(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect();
            }
            _ => {}
        }
        at += 8 + n + (n & 1);
    }
    let frames = (data.len() / usize::from(ch.max(1))) as u64;
    (rate, ch, bits, frames, data)
}

/// README steps 180–184 through the channel (plan 5f-2; spec §12.3 "bounce
/// and check the file"): a bounce of four bars to a 16-bit WAV, refused
/// over the same file without `replace`, then Add to project; hostile
/// calls refused.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn bounces_the_demo_and_checks_the_file() {
    let mut app = App::start();
    let home = app.launched.home.clone();
    let src = home.join("tone.caf");
    audio_file(&src, 96_000);
    let demo = app.call("command.run", json!({"id": "file.create_demo_audio"}));
    let w = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": src, "seq": seq, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");

    // Four bars from tick 0 at 120 BPM: 16 × 15 360 ticks, 16 × 24 000 frames.
    let (end, range) = (16 * 15_360, 384_000u64);
    let mix = home.join("mix.wav");
    let params = json!({"path": mix, "bits": "16", "rate": 48_000, "range": "selection", "start": 0, "end": end});
    let r = app.call("command.run", json!({"id": "file.bounce", "params": params.clone(), "window": w}));
    let status = r["status"].as_str().unwrap_or("").to_string();
    assert!(status.starts_with("wrote ") && status.contains(&mix.display().to_string()), "{r}");
    let (rate, ch, bits, frames, samples) = read_wav16(&mix);
    assert_eq!((rate, ch, bits), (48_000, 2, 16), "{status}");
    assert!(
        (range..=range + 480_000).contains(&frames),
        "{frames} frames: the range plus at most the 10 s tail (pre-flight M11)"
    );
    assert!(
        samples[..2 * 96_000].iter().any(|s| *s != 0),
        "the imported 0.25 tone sounds in the first bar"
    );
    let before = std::fs::read(&mix).unwrap();

    // The same path without `replace`: refused, the file unchanged.
    let e = app.call_err("command.run", json!({"id": "file.bounce", "params": params, "window": w}));
    assert_eq!(e.code, core_control::ErrorCode::Failed, "{e:?}");
    assert!(e.message.contains("exists"), "{e:?}");
    assert_eq!(std::fs::read(&mix).unwrap(), before);

    // Add to project: one undo step, a lane "Mix", a Bounced item.
    let r = app.call(
        "command.run",
        json!({"id": "file.bounce", "params": {"add_to_project": true, "range": "selection", "start": 0, "end": end}, "window": w}),
    );
    assert_eq!(r["undo"], "Bounce", "{r}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(doc["lanes"].as_array().unwrap().last().unwrap()["name"], "Mix", "{doc}");
    let m = app.call("state.get", json!({"target": "media", "window": w}));
    assert!(
        m["items"].as_array().unwrap().iter().any(|i| i["origin"] == "bounced"),
        "{m}"
    );

    // Hostile calls.
    let e = app.call_err(
        "command.run",
        json!({"id": "file.bounce", "params": {"format": "mp3", "path": mix}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e:?}");
    let e = app.call_err("command.run", json!({"id": "file.bounce", "params": {}, "window": w}));
    assert_eq!(e.code, core_control::ErrorCode::DialogRequired, "{e:?}");
    let e = app.call_err(
        "command.run",
        json!({"id": "file.bounce", "params": {"add_to_project": false}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::DialogRequired, "the app's check (pre-flight M6): {e:?}");
    app.quit();
}
```

  (`ControlError`'s fields and `ErrorCode`'s variant names are the tree's; adjust if they differ and say so.)

- [ ] **Step 2: Write README 180–184** at the end of `app/README.md`:

```markdown
### Bounce and export (plan 5f-2)

By ear and by eye; the renderer is tested (`core-engine/tests/bounce.rs`),
the files (`core-media/tests/export.rs`), the job (`core-ffi/tests/bounce.rs`)
and the e2e `bounces_the_demo_and_checks_the_file`.

180. File › Bounce…: the sheet defaults to WAV at the project's rate and
     depth (Dither on at 16 bits). Bounce the demo song (with an audio
     file imported) to a file while it plays: playback goes on; the file
     plays in Finder's preview and sounds like the mix, reverb tails
     included. Push master hot and bounce at 32-bit float: the file holds
     peaks above 0 dBFS; at 16 bits the job reports the clipped samples.
181. README 164's check: with a latent plugin on a bus, bounce Stems of
     two tracks and the Mix to files; zoom in on them in another editor
     (or import them back): every file starts at the same sample, a mute
     written at a bar line cuts the Mix exactly there, and the muted
     track's stem file plays on through it (files ignore mute and solo).
182. Add to project: the bounce appears on a new lane "Mix" at the range
     start, in the media pool as Bounced; one Undo removes it. Mute one
     track, solo nothing, and bounce Stems with Add to project: the muted
     track's new lane is silent and the others sound; with a mute written
     at a bar line, its lane cuts there, as the mix does. Route a track
     into a bus and mute the bus: the track's bounced-in-place lane is
     silent too, as heard.
183. A project with a lane playing a hardware synth: the sheet says
     "1 lane plays external MIDI and will be silent. Track it as audio
     first: Track Synth…", and the synth is silent in the file.
184. Cancel a long bounce: no file and no ".partial" is left next to
     where it was going. Bounce the mix over an existing file: the save
     panel asks to replace it, and the old file stays until the new one
     is complete. Bounce Stems into a folder that already holds them: the
     sheet lists the new names ("Bass 2.wav"), nothing is asked, and the
     old files are untouched.
```

- [ ] **Step 3: Add the backlog row** (owner answer 4) at the end of the table in `docs/feature-backlog.md`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
tail -c 1 docs/feature-backlog.md | od -c | head -1   # ends in \n
cat >> docs/feature-backlog.md <<'VIZDAW_EOF'
| Broadcast-WAV (`bext`) metadata in bounces | Plan 5f-2 Ruling 1 (owner, 2026-11-03): VizDAW writes WAV, AIFF and CAF itself and uses ExtAudioFile only for a rate change, so a bounced WAV carries no `bext` chunk (description, originator, origination date and time, time reference of the range start). Add the chunk to the WAV writer behind `PcmWriter`, from the public EBU Tech 3285 description, with the time reference set to the range start in samples. |
VIZDAW_EOF
tail -2 docs/feature-backlog.md
```

- [ ] **Step 4: Run the full gates**, including the rt-check gates (with `--test rt_alloc_bounce`), the workspace tests, the Linux clippy, Swift, `scripts/make-app.sh` and the e2e. Expected (from the baseline Rust 1802 / 15 ignored, Swift 933, e2e 11, registry 144): Rust +14 +13 +3 +17 +1 = **+48 → 1850 passed, 16 ignored** (macOS; the e2e adds one ignored test); on Linux +45 (12 + 13 + 2 + 17 + 1); Swift +8 → **941**; e2e **12**; registry **145**.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl/tests/e2e.rs app/README.md docs/feature-backlog.md
git commit -m "e2e: bounce four bars and check the file; README 180-184; backlog row for Broadcast-WAV metadata (plan 5f-2 Task 7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -5
```

---

## Self-Review

1. **Spec coverage.**
   - §9.1:
     - offline render on a worker (the job) thread with the streamer synchronous: Tasks 2 and 4;
     - plugins offline (AU and CLAP): Task 3;
     - Mix, Stems and Selection: Tasks 2, 4 and 5 (every run of every block in every stem: Task 2's taps);
     - range and tail with the 10 s Setting: Tasks 2 and 6 (applied at launch);
     - the external-MIDI warning with the spec's sentence, and the silence: Tasks 2 (`SilentMidi`), 4 and 6;
     - live playback beside, with separate copies, retired however the job ends: Task 4.
   - §9.2:
     - WAV, AIFF and CAF at 16, 24 and float; TPDF dither, default on at 16 bits; any converter rate: Task 1 (Ruling 1 explains "through the converter", accepted by the owner);
     - the sheet defaults: Task 6;
     - a chosen file, with no dialog programmatically: Tasks 4–5;
     - Add to project as `Bounced`, one step, exactly as long as the files: Task 4;
     - progress and cancel, with the partial deleted and a replaced file kept: Tasks 1, 4 and 6.
   - §12.1 `bounce`: Task 5 (Ruling 2's ID).
   - §12.3:
     - "bounce equals an offline render" (Task 2 at engine level, Task 4 at session level);
     - "dither changes only the LSBs" (Task 1);
     - "the tail stops at the threshold" (Task 2);
     - "Add to project places the region correctly" (Task 4);
     - "cancel removes the partial file" (Tasks 1 and 4);
     - the e2e (Task 7).
   - The owner's answers of 2026-11-03: float overs (Ruling 13; Tasks 1, 2, 4), numbered stems (Ruling 3; Tasks 1, 4, 6), heard through the routing (Ruling 5; Tasks 2, 4), own writers and the `bext` backlog row (Ruling 1; Task 7).
   - No gaps.
2. **Step scan.** Every step names its files, its signatures and the spec values. Every test is given as code (Tasks 1–7, including the platform-macos, core-host, core-ffi, Swift and e2e tests). Prose is left only for implementation bodies whose behaviour the tests pin.
3. **Type consistency.**
   - `Container` (core-media) maps to `ContainerRow` (core-ffi), and `BitDepth` to the existing `BitDepthRow`.
   - `BounceSpec.stems` (files) and `BounceSpec.heard_stems` (`HeardStem`, Add to project) take `StripId`s from `BounceSourceRow::Stems { strips }` (Ruling 5).
   - `StemTap.arrival`/`gate_arrival` are `Graph::strip_arrival`/`feed_arrival` (master: `latency`).
   - `Work::Bounce.exported` holds `BouncedFileRow` (Task 4), which `BounceResultRow.files` carries and `ControlJobs.answer(…, bounce:)` reads (Task 5).
   - `PendingExport::{commit, commit_numbered}` (Task 1) are the job's renames (Task 4, Ruling 12).
   - `ImportAs` is used by both import and bounce.
4. **Review Focus.** Each of the five lines names its tests in its owning task, and every named test exists in the task's code.
5. **Proportion.** The format layouts, the stem taps and the alignment and gate rules are the only algorithm text; the rest is signatures and tests.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-02-bounce-export.md`. The controller runs it subagent-driven after 5f-1 under the standing authorization. Tasks 1–3 are independent of each other; Task 4 joins them, and its review should replay the cancel, replace and failed-apply paths (Ruling 12) and the copies' end on every path (Ruling 6). Task 2's review should replay the run-split cases against a build with the taps disabled (the stems test must fail there).
