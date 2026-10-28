# VizDAW 5a — Audio Playback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first plan of sub-project 5. Every project gets an audio format (sample rate and bit depth). Audio files are imported by converting them into CAF files in that format inside the package. A new platform-neutral crate, `core-media`, holds the CAF reader and writer, the import pipeline and the disk streamer. The engine turns audio regions into segments and plays them through a `Regions` node feeding each output path's mixer strip, with gain and fades. Regions loop, locate and underrun safely, and offline media plays silence. The app gets a New Project sheet, Project Settings (with conversion), Import Audio…, banners for media in another format and for a device that would not switch rate, and `media.import` plus `state.get media`/`project` on the control channel.

**Architecture:** Rust does the work and Swift draws it (the user's guideline).
- **`core-model`**
  - `Settings.format` (`AudioFormat { sample_rate, depth }`).
  - Fades in frames, and media items with a name and origin. These three ship as one file-format bump, version 3, with a migration.
  - `MediaCmd`: the media commands moved out of `RoutingCmd`, plus Rename, Relink, Compact and Convert.
  - `SettingsCmd::SetFormat`, which swaps every media file and rescales regions.
  - `AudioCmd::Insert` assigns an output path to a lane that has none.
- **`core-media`** (new, platform-neutral)
  - CAF read and write in int16, int24 and float32, with TPDF dither at 16-bit and data size −1 while recording.
  - The `MediaImporter` trait, `import_file` (an incoming folder, then an atomic rename, mono split above two channels) and `CafImporter`.
  - The `Streamer`: a disk thread and a lock-free voice table read by the real-time thread, with epoch-guarded reuse and a RAM cache of small files.
- **`platform-macos`**: `ExtAudioImporter` (ExtAudioFile decoding at the highest-quality converter) and a HAL request to switch the device's nominal rate.
- **`core-engine`**
  - The schedule compiler makes per-path `Segment`s (the parent overlap rule, fades relative to the region).
  - A `Regions` node per `AudioPath` strip reads the streamer.
  - The renderer cuts runs at loop and sequence ends while audio plays, keeps a late-start window, and reports `stream-underrun` and `late-start` faults.
- **`core-ffi`**
  - A media root: the package, or a scratch package for untitled projects; Save As copies the media.
  - The offline set, import and convert jobs applied as one undo step each, the host's media table, the project format API and device-rate handling.
- **Swift**: the sheets, menu items, banners and the control handlers.

**Tech Stack:** Rust 2024 (`core-model`, the new `core-media`, `core-engine`, `core-ffi` with UniFFI 0.32.2, `core-control`, `platform-macos` with `objc2-audio-toolbox` 0.3.2, `vizctl`, `vizdaw-play`, `proptest`, `blake3`), Swift 6 / SwiftPM (SwiftUI shell, AppKit views, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`. This plan covers the spec's 5a row in §13:
- §4.1 project audio format;
- §4.2 fades in frames;
- §4.3 media;
- §5.1 import;
- §5.2 CAF reader and writer;
- §5.3 schedule segments;
- §5.4 the RegionSource node;
- §5.5 the streamer;
- §5.6 device rate;
- §12.1, the `media.import`, `project.new`, `project.set_format`, `state.get media` and `state.get project` parts;
- §12.2, `stream_underrun` and `late_start` (named `stream-underrun` and `late-start` here, Ruling 21);
- §12.3, the core-media, streaming-property, schedule, `rt-check` and e2e lines that concern playback.

The parent spec is `2026-10-15-vizdaw-design.md` (§5 media pool and audio regions, §8 audio files, §10 offline media). Both specs travel with this plan, so read both.

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. The Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools.
  - A `git commit` without `DEVELOPER_DIR` fails, so check every commit with `git log -1`.
  - Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- **Branch and commits.**
  - Work on branch `sp5-audio` (head `fafae25` before this plan's commit, plus that commit).
  - Commit at the end of every task, adding only the files the task lists, plus `Cargo.lock` when a manifest changed. Never push.
  - End every commit message with the attribution lines your harness requires. The blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and the `Claude-Session:` line; use the ones your dispatch gives you.
- **Clean room.** Never read or copy anything from `~/workspace/SVP/research` or the Studio Vision binary.
- **No GUI tools.**
  - Never use GUI or computer-use tools. Launch VizDAW only through `vizctl launch` or the e2e test (a throwaway home under `/tmp`), and quit it when done (`vizctl quit`; `pkill -f "VizDAW --control"` if a test left one).
  - At the end of a task that launched the app, check that `pgrep -lf MacOS/VizDAW` prints nothing, and remove the `/tmp/vze*` homes your e2e runs left.
  - Never write to the real `~/Library/Application Support/VizDAW` or the real preferences. Tests use temp directories (`tempfile`).
- **Crates and dependencies.**
  - Rust stable, edition 2024. One new crate, `core-media` (`crates/core-media`; the workspace picks up `crates/*`).
  - New dependencies:
    - `blake3 = "1"` (pure Rust, `default-features = false, features = ["std"]`) in `core-media`;
    - `core-media` as a dependency of `core-engine`, `core-ffi`, `platform-macos` (macOS target) and `vizdaw-play`;
    - the `ExtendedAudioFile`, `AudioFile` and `AudioConverter` features on `objc2-audio-toolbox` in `platform-macos`.

    Nothing else.
  - License GPL-3.0-only.
- **Lints.** Every crate keeps or gets:
  - `#![deny(missing_docs)]`;
  - `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. `core-media` gets both from Task 3.
- **Real-time rules (spec §3).** The real-time thread never allocates, locks or does file I/O.
  - `StreamReader::read`, `StreamReader::begin_block`, the `Regions` node, the fade curves and the renderer changes are marked `// RT`, use only atomics and preallocated buffers, and run under `rt-check` in `crates/core-engine/tests/rt_alloc.rs` (Task 9).
  - Anything the real-time thread retires is freed on the existing collector thread, or by the streamer's disk thread after an epoch has passed (Ruling 9).
- **File compatibility.** `core_model::file::VERSION` becomes 3 (Task 1).
  - Migration 2→3 adds `settings.format`, turns `fade_in`/`fade_out` `length` (ticks) into `length_frames`, and gives media items a `name` and an `origin`.
  - Version 1 files migrate through 2.
  - Loading a version 3 file written by this build and saving it again is byte-identical.
- **FFI safety.** No panic crosses the FFI: every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`. No session `inner` lock is held across plugin code or across a file conversion. Jobs run on their own threads and are applied under the lock in `idle`/`job` (Ruling 15).
- **UniFFI changes.**
  - Changed records:
    - `TransportState` gains `rate_mismatch: bool`, last, `#[uniffi(default = false)]`;
    - `FadeRow.length` is now frames (same field, same type `i64`; its doc changes).
  - New records, enums and methods are listed per task.
  - `FakeSession` in `app/Tests/VizDAWAppModelTests/FakeSession.swift` implements every new `SessionProtocol` method.
- **Swift.**
  - The generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests compile in Swift 6 mode with no new warnings (the `MemoryDefaults` Sendable warning is older).
  - Never use SwiftUI's `@State` or `@FocusState` in new views; keep view state in `@Observable` models.
  - Closures stored by views capture `[weak self]`.
  - New Swift tests go to `VizDAWAppModelTests`.
- **Control channel** (control spec §6, §7).
  - Programmatic calls never open dialogs.
  - Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`; the test `everyRegisteredMenuCommandHasAHandler` compares the two.
  - The registry count test moves from 108 to 111 in Task 12.
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command after a Rust change.
- **Each task ends green.**
  - Always: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
  - Rust tasks touching `core-engine` (7, 8, 9) also run `cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc`.
  - Tasks 10–14 also run `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Tasks 13–14 also run `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Run `cargo fmt --all` first.
  - Known flaky tests: `core-ffi` catalog tests under parallel load, and the `damaged_files` proptest. Re-run once before investigating.
- **How code is given.**
  - New files are written whole (`cat > … <<'VIZDAW_EOF'`).
  - Changes to existing files are exact **Find / Replace** excerpts of the tree the previous task left, applied in order with an editor (the Edit tool).
  - Where a step says "every call site" (a mechanical rename across many tests), it gives the exact command or pattern and the compile errors to expect.
  - If an excerpt does not match because a fix landed meanwhile, make the same change to the current text and say so in the report.
  - The bindings named from `objc2-*` are checked against the 0.3.2 generated sources (`~/.cargo/registry/src/*/objc2-audio-toolbox-0.3.2/src/generated/`). If a signature differs (a `NonNull` against a raw pointer), adapt the call, keep the behaviour, and say so.
- **Spec values used verbatim:**
  - **Formats:**
    - allowed rates "44 100, 48 000, 88 200, 96 000, 176 400 and 192 000 Hz";
    - the default "48 000 Hz, 24-bit integer";
    - "Lowering the depth to 16-bit dithers (TPDF). Raising it never alters samples";
    - "24-bit is rounded without dither";
    - "Integer formats clip at full scale and count the clipped samples".
  - **Streaming:**
    - "files of at most 4 MiB of audio data … RAM cache … 512 MiB budget";
    - "256 stream voices … 2 s";
    - "next 3 s of playback";
    - "read sizes of at least 64 KiB";
    - "waits up to 250 ms";
    - "the first 2 s after the loop start is kept prefilled";
    - "that segment plays silence … `stream_underruns` counter";
    - "never stop the transport".
  - **Media:**
    - `Media/.incoming/<uuid>.caf`;
    - "An interrupted import leaves nothing in `Media/`";
    - "A file with more than two channels is split into mono files";
    - "Mono media goes to both channels at unity";
    - fade curves "linear; equal-power `sin`; exponential with a 60 dB range; S-curve as a smoothstep of equal-power".

## Review Focus

These are the five failure modes a user is most likely to hit. The spec implies them but no requirement spells them out. Each is pinned by a named test in its owning task.

1. **A file that changes or vanishes under playback.** The user deletes, truncates, or replaces a media file in Finder while the project is open or while it plays.
   - Expected: reads return errors, never SIGBUS (positioned reads, no mmap). The region plays silence, and the item is reported offline once. No panic, no garbage audio, and the transport keeps rolling.
   - Pinned by `caf::tests` `truncated_file_reads_what_is_there` (Task 3), `stream::tests::a_file_removed_mid_play_goes_offline_and_plays_silence` (Task 6) and `offline_media_plays_silence_and_counts_nothing` (Task 9).
2. **Imports that are not what they claim.** A `.wav` that is text, a zero-length file, a 6-channel file, a file whose name is already taken in `Media/`, an import cancelled half way, two imports of the same file.
   - Expected: a per-file error naming the file, while the other files of the batch still import. Nothing half-written is left in `Media/`, names get a numeric suffix, and a 6-channel file becomes six mono items.
   - Pinned by `import::tests` `garbage_and_empty_files_fail_alone`, `six_channels_split_into_six_mono_items`, `names_never_collide`, `cancel_leaves_nothing_in_media` (Task 4) and `importing_the_same_file_twice_makes_two_items` (Task 10).
3. **Loops and locates at awkward places.** A loop shorter than one block; a loop end inside a fade; a locate into the middle of a region; Play at a position with no region for 10 s and then a region; regions overlapping on one lane; a looped lane whose length cuts a region.
   - Expected: sample-exact audio equal to a fully RAM-cached render, and the overlap rule (the later start takes over, the earlier resumes).
   - Pinned by the property `streamed_render_equals_cached_render` (random layouts, loops and locates, Task 9) and `schedule_audio` tests `later_start_takes_over_and_earlier_resumes` and `looped_lane_repeats_regions_and_cuts_at_its_length` (Task 7).
4. **The disk is slower than playback.** The streamer is starved, for example a test that never services the disk.
   - Expected: silence for exactly the missing frames, the `stream-underrun` fault (or `late-start` within 250 ms of Play or Locate), the transport never stops, and no garbage samples.
   - Pinned by `starved_disk_plays_silence_and_flags_underruns`, the proptest `a_starved_disk_is_only_ever_silent` and `misses_right_after_play_count_as_late_start` (all Task 9).
5. **Formats that do not line up.** A project at 44.1 kHz importing 48 kHz audio; an old project whose media is 44.1 kHz WAV; changing the format from 48 kHz/24-bit to 44.1 kHz/16-bit and undoing it; a device that refuses the project rate.
   - Expected:
     - imports convert, so the stored media is in the project format;
     - old media stays offline with a Convert offer;
     - a format change converts every file and rescales region offsets, lengths and fades by the rate ratio, and undo restores the old files;
     - a refused device rate mutes regions and says so.
   - Pinned by `importing_44k1_into_a_48k_project_converts` (Task 5), `set_format_rescales_regions_and_undo_restores` (Task 2 for the model, Task 11 for files), `old_media_in_another_format_converts_on_request` (Task 11) and `refused_device_rate_mutes_regions` (Task 11).

## Plan Rulings

These are decisions the spec leaves open. Each is implemented and tested as stated, in the format "what — why — cost if wrong".

1. **Ruling:** `Fade` becomes `{ length_frames: u64, shape }`. A region is valid only if `fade_in.length_frames + fade_out.length_frames <= length`.
   - Commands that shorten a region (Trim, Split) clamp the fade in to the new length first, then the fade out to what remains.
   - The FFI `FadeRow` keeps its field `length: i64`, which now holds frames, so Swift's literals still compile.
   - Why: spec §4.2.
   - Cost if wrong: a later preference for "fade out first" means one function changes.
2. **Ruling:** The 2→3 migration works on raw JSON, like 1→2.
   - `settings.format` gets `{ sample_rate, depth: "int24" }`. The rate is the most common media rate (on a tie, the first item's), or 48 000 when there is no media or that rate is not an allowed one.
   - Every media item gets `name` (the file stem) and `origin: { "imported": { "original_name": <file name> } }`.
   - Every audio region's fade `length` (ticks) becomes `length_frames`. The value is the duration of those ticks in the owning sequence's tempo map from the region's start, times the media item's sample rate, rounded. It is clamped as in Ruling 1.
   - Why: spec §4.1–§4.3.
   - Cost if wrong: an old project's fades are off by rounding (under one frame).
3. **Ruling:** `AudioFormat` and `BitDepth` live in a new module `core_model::format`, used by `core-media` and the FFI.
   - Cost if wrong: none.
4. **Ruling:** `MediaItem.content_hash` stays `Option<String>`. Old projects have none, and every import, conversion and relink fills it (BLAKE3 hex of the CAF's audio bytes).
   - The offline check on open covers four cases: a missing file, a file that is not a readable VizDAW CAF, a file in another format, and a length that differs from `length_frames`.
   - The hash is not re-computed on open. Hashing every file of a large project on every open is slow, and 5b's relink dialog ("Search folder…", which matches by hash) is where the hash is needed.
   - Cost if wrong: a file replaced by another of the same format and length plays until 5b adds the check. That is listed for 5b.
5. **Ruling:** `MediaCmd` holds `Add` (with `name` and `origin`), `Remove`, `Rename`, `Relink`, `Compact` and `Convert { swaps }`. `RoutingCmd` loses `AddMedia` and `RemoveMedia`.
   - `Convert` is "Convert Media": new files for some items, keeping the project format. It is used by the old-project banner.
   - `SettingsCmd::SetFormat { format, swaps }` is "Change Project Format". When the format changes and the project has media, the swaps must cover every media item.
   - A swap gives a media item a new file (same `MediaId`, so regions keep their item) and rescales the item's regions by `new_rate / item.sample_rate`. Offset, length and fades are rounded to the nearest frame, the length is kept ≥ 1, and the region is clamped into the new file.
   - Why: spec §4.1, §4.3 and §5.1.
   - Cost if wrong: none.
6. **Ruling:** `AudioCmd::Insert` on a lane with no `audio_out` sets it in the same command.
   - It uses the document's first output path by ID. With none, it adds an output path "Audio 1" with reference `audio:1-2`, which brings its `AudioPath` strip with it.
   - Why: spec §5.3.
   - Cost if wrong: none.
7. **Ruling:** `MediaImporter` is `probe(path) -> SourceInfo` plus `decode(path, rate, sink, cancel) -> frames`, where `decode` delivers interleaved float at the target rate in chunks.
   - `core_media::import` writes the CAF files itself: the depth, dither, clip count, hash, mono split, incoming folder and rename.
   - This replaces the spec's `convert(path, out, format, …)` shape, so depth, channel split and hashing happen once, in platform-neutral Rust. A Linux importer then only decodes.
   - Cost if wrong: none (internal).
8. **Ruling:** `CafImporter` (platform-neutral) reads VizDAW CAF files only, at the same rate. A rate change is refused with "resampling needs the system importer".
   - It serves tests, depth-only conversions and other platforms.
   - On macOS the app always uses `ExtAudioImporter`, which also reads CAF.
   - Cost if wrong: none.
9. **Ruling:** The streamer's voice table has 512 slots.
   - **Stream voices.** 256 voices, each a 2 s ring per channel at the project rate, allocated on demand by the disk thread and kept until the streamer stops. That is about 196 MB only if all 256 are ever in use at 48 kHz stereo.
   - **Cache voices.** 256 voices holding whole files of at most 4 MiB of audio data, within a 512 MiB budget of float data, evicted least recently used.
   - **Voice key.** A voice is keyed by its segment's `(media, start_frame, file_frame)`. The real-time thread finds a voice by key and frame range and only reads; it never updates anything but an epoch counter at each block's start.
   - **Overwrites.** The disk thread overwrites a ring position only once the published playhead has passed it.
   - **Retirement.** A voice is retired by unpublishing it. It is reused, and a cache voice freed, only after the epoch has advanced twice.
   - **Manual mode.** `DiskMode::Manual` runs no thread. Tests call `Streamer::service()` between blocks, and retirement is immediate.
   - There is no blocking mode in 5a; 5f's bounce adds one.
   - Why: spec §5.5's numbers, with allocation on demand so an idle project does not hold 196 MB.
   - Cost if wrong: the reuse rule changes in one module.
10. **Ruling:** These are the late-start rules.
    - A miss within 250 ms of schedule frames after the playhead (re)starts counts as `late_start`. A (re)start is Play, Locate, or a loop wrap with no prefilled seam. Every other miss counts as `stream_underrun`.
    - `Engine::play` waits up to 250 ms (`Streamer::wait_ready`) before the Play command goes out. `Engine::locate_frame` does not wait, but hints the streamer.
    - Why: spec §5.5.
    - Cost if wrong: none.
11. **Ruling:** These are the graph rules for regions.
    - Every `AudioPath` strip gets one `Regions { path }` node, whether or not the schedule has regions on it, so the graph never depends on the schedule.
    - Node order is instruments, then strips, then regions nodes.
    - The node has no latency.
    - Graph tests that count the nodes of a document with output paths gain one node per path.
    - Cost if wrong: an idle node per output path, which costs one empty buffer clear per block.
12. **Ruling:** While the schedule has audio segments, the renderer cuts runs at the loop end and the sequence end (`Transport::boundary`), as it already does with automation. A run is then one contiguous stretch of schedule frames for the `Regions` node.
    - Cost if wrong: smaller graph runs near loop ends.
13. **Ruling:** Fade curves are relative to the region as written: its start, and its full `length` from the source event. Where the overlap rule or a lane or reference window cuts a region, the cut is hard. Users shape joins with fades (spec §1, "Automatic crossfades" are out of scope).
    - Cost if wrong: a click at a hard cut, as the spec accepts.
14. **Ruling:** These are the media-root rules.
    - A saved project's media live in `<package>/Media`. An untitled project's live in a scratch package `std::env::temp_dir()/VizDAW-untitled/<session id>.vizdaw`, removed when the session closes.
    - Save and Save As copy every referenced media file that is not already at the destination, so Save As carries the media, and the scratch package's files reach the real one.
    - Why: imports need a place before the first save. Today Save As copies no media at all.
    - Cost if wrong: a large untitled import is copied once on the first save.
15. **Ruling:** These are the job rules.
    - Import and conversion run on their own threads (`core-ffi/src/jobs.rs`) with progress and cancellation.
    - A finished job is applied as one undo step under the session lock by the next `Session.idle()` or `Session.job(id)` call: "Import Audio", "Change Project Format", or "Convert Media". A job whose document changed shape meanwhile still applies; its commands are validated like any edit.
    - A failed or cancelled job leaves nothing in `Media/`.
    - A control command that "waits for completion" answers through the channel's deferred answers, polling `job(id)` on every tick.
    - Cost if wrong: none.
16. **Ruling:** These are the control-channel names.
    - `project.new` is `file.new` with optional `rate` (`"44100"` … `"192000"`) and `depth` (`"16"`, `"24"`, `"32f"`).
    - `project.set_format` is `file.project_settings` (File › Project Settings…) with `rate` and `depth`. Given either, it converts and waits.
    - `media.import` (File › Import Audio…) takes `path` (any absolute file) and optional `seq`, `lane` and `tick`.
    - `media.convert` (no menu item; the banner's button) converts the media not in the project format.
    - The registry goes from 108 to 111 commands.
    - `ParamKind` gains `Int { min, max }` (→ `ArgValue::Int`) and `File` (an absolute path with any extension).
    - Why: the registry is the menu, and a separate `project.*` namespace would duplicate File menu items.
    - Cost if wrong: two aliases later.
17. **Ruling:** `state.get` gains the targets `media` (per item: `id`, `name`, `path`, `channels`, `frames`, `rate`, `origin`, `offline`, `offline_reason`, `uses`) and `project` (`rate`, `depth`, `mismatched`, the number of media items not in the project format, `rate_mismatch`).
    - Cost if wrong: none.
18. **Ruling:** The New Project sheet (File › New from the menu) remembers its last choice as the app's "Default project format" (UserDefaults key `defaultProjectFormat`, e.g. `"48000/24"`). There is no Settings UI for it until 5d's Settings › Audio. `file.new` from the channel, without parameters, uses that preference.
    - Cost if wrong: a Settings row later.
19. **Ruling:** These are the Import Audio… targets.
    - The menu imports into the selected lane of the shown sequence if a lane is selected, otherwise into a new lane "Audio n" appended to it.
    - The region goes at the transport's tick (0 if negative).
    - With no sequence the files only enter the media pool.
    - A file split into mono items gets one new lane per item. Several files get one new lane each; only a single mono or stereo file goes into the selected lane.
    - The channel's `media.import` takes `seq`, `lane` and `tick` the same way. Without `seq`, the files only enter the pool.
    - Cost if wrong: none (5b adds drag-in placement).
20. **Ruling:** These are the device-rate rules.
    - The live host asks the HAL to set the device's nominal rate to the project rate, and waits up to 1 s for the device to report it.
    - If it does not, regions are muted (`Engine::set_regions_muted`), `TransportState.rate_mismatch` is true, and a banner offers "Change Project Rate to <device rate>" (the §4.1 conversion).
    - A project-format change drops the host, so the next Play opens at the new rate.
    - The offline backend (`VIZDAW_AUDIO=offline`) always runs at the project rate.
    - Why: spec §5.6.
    - Cost if wrong: none.
21. **Ruling:** The faults gain `stream-underrun` (bit 2048) and `late-start` (bit 4096). The names are kebab-case, like the existing `midi-dropped`; the spec's `stream_underrun`/`late_start` name the same flags.
    - While `stream-underrun` is in the transport's faults, the window shows a non-modal "Disk too slow: some audio played as silence" bar. Faults clear at the next Play, so the bar shows at most once per playback.
    - A read error mid-play marks the media offline. `Session.idle()` collects the streamer's errors and reports each once in `problems`.
    - Cost if wrong: none.
22. **Ruling:** The offline reasons are `missing`, `unreadable` (not a VizDAW CAF), `format` (another rate or depth) and `length`.
    - The offline set is recomputed on open, after every media edit, import or conversion, after Save As, and when the app becomes active (`Session.recheck_media()`). There is no file-system watcher in 5a.
    - Cost if wrong: a file changed while VizDAW stays frontmost is noticed when playback reads it (Ruling 21) or at the next activation.
23. **Ruling:** The demo with audio keeps its `Media/loop.wav`, which does not exist. It is offline and plays silence.
    - The 4b, 4d and 4e e2e tests draw the region only, so nothing changes for them.
    - Cost if wrong: none.
24. **Ruling:** `vizdaw-play` passes the package's media to its engine, so `vizdaw-play song.vizdaw` plays regions too.
    - Cost if wrong: none.

## Carried and parked items

Picked up by this plan:
- **Spec ruling 4**, "offline media is runtime state": Ruling 22.
- **The 4d ledger's "rt_alloc_plugins ≥20 ramping lanes"** stays as it is. The new region path gets its own `rt_alloc` test (Task 9).

Parked for later plans, as the spec assigns them:
- 5b: waveforms, fade and gain handles, the media pool window, the relink dialog, the media retention rule (Trash on save and close), and the hash check of Ruling 4.
- 5c: recording.
- 5d: the record UI and Settings › Audio.
- 5f: bounce and its blocking streamer mode.

---

### Task 1: Project format, fades in frames, named media — file format version 3

**Files:**
- Create: `crates/core-model/src/format.rs`
- Modify: `crates/core-model/src/lib.rs`, `crates/core-model/src/document.rs`, `crates/core-model/src/event.rs`, `crates/core-model/src/rules.rs`, `crates/core-model/src/command/audio.rs`, `crates/core-model/src/command/routing.rs`, `crates/core-model/src/file.rs`, `crates/core-ffi/src/events.rs`
- Test: `crates/core-model/tests/file.rs`, `crates/core-model/tests/cmd_audio_routing.rs`, `crates/core-model/tests/model_types.rs`, `crates/core-model/tests/rules.rs`, `crates/core-model/tests/query_usage.rs`

**Interfaces:**
- Consumes: `core_model::tempo::TempoMap::{tick_to_seconds}`, `core_model::file::{migrate_v1_to_v2, run_migrations, MIGRATIONS}`.
- Produces:
  - `core_model::format::{AudioFormat { sample_rate: u32, depth: BitDepth }, BitDepth::{Int16, Int24, Float32}}`, with:
    - `AudioFormat::RATES: [u32; 6]`, `AudioFormat::DEFAULT`;
    - `AudioFormat::is_valid(&self) -> bool`, `AudioFormat::label(&self) -> String` (e.g. `"48 kHz, 24-bit"`);
    - `BitDepth::{bits(self) -> u32, bytes(self) -> usize, label(self) -> &'static str, all() -> [BitDepth; 3]}`.
  - `Settings.format: AudioFormat`.
  - `MediaItem.{name: String, origin: MediaOrigin}`, with `MediaOrigin::{Imported { original_name: String }, Recorded { pass: u32 }, Bounced, ExternalEdit { from: MediaId }}` (`Default` = `Imported { original_name: "" }`).
  - `Fade { length_frames: u64, shape }`.
  - `file::VERSION == 3`, `file::migrate_v2_to_v3`.

Rulings 1–3. Every format change of this plan lands here, in one version bump. Media commands stay in `RoutingCmd` until Task 2.

- [ ] **Step 1: Write the failing tests**

Append to `crates/core-model/tests/file.rs`:

```rust
use core_model::document::MediaOrigin;
use core_model::file::{migrate_v1_to_v2, migrate_v2_to_v3};
use core_model::format::{AudioFormat, BitDepth};

/// `(fade in, fade out)` frames of every region of `d`.
fn region_fades(d: &Document) -> Vec<(u64, u64)> {
    d.all_events()
        .filter_map(|(_, e)| match e.body {
            EventBody::AudioRegion {
                fade_in, fade_out, ..
            } => Some((fade_in.length_frames, fade_out.length_frames)),
            _ => None,
        })
        .collect()
}

#[test]
fn version_2_projects_migrate_to_version_3() {
    let d = from_json(&fixture("mixed_lanes")).unwrap();
    assert_eq!(
        d.settings().format,
        AudioFormat {
            sample_rate: 48_000,
            depth: BitDepth::Int24
        }
    );
    let m = d.media().values().next().unwrap();
    assert_eq!(m.name, "vox");
    assert_eq!(
        m.origin,
        MediaOrigin::Imported {
            original_name: "vox.wav".into()
        }
    );
    // 960 ticks at 120 BPM = 1/16 beat = 31.25 ms = 1500 frames at 48 kHz.
    assert_eq!(region_fades(&d), vec![(1500, 0)]);
    let text = to_json(&d).unwrap();
    assert!(text.contains("\"version\": 3"));
    assert!(text.contains("\"length_frames\": 1500"));
    assert!(text.contains("\"depth\": \"int24\""));
}

/// A version-2 `minimal` project whose media pool has items at `rates`.
fn v2_with_media(rates: &[u64]) -> Value {
    let mut v = migrate_v1_to_v2(serde_json::from_str(&fixture("minimal")).unwrap()).unwrap();
    v["document"]["media"] = Value::Array(
        rates
            .iter()
            .enumerate()
            .map(|(i, r)| {
                json!({ "id": i, "path": format!("Media/m{i}.wav"), "sample_rate": r,
                        "channels": 2, "length_frames": 10, "content_hash": null })
            })
            .collect(),
    );
    v
}

#[test]
fn the_migrated_rate_is_the_most_common_allowed_media_rate() {
    let rate = |rates: &[u64]| {
        migrate_v2_to_v3(v2_with_media(rates)).unwrap()["document"]["settings"]["format"]
            ["sample_rate"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(rate(&[]), 48_000);
    assert_eq!(rate(&[44_100, 48_000, 44_100]), 44_100);
    // A tie goes to the first item's rate.
    assert_eq!(rate(&[96_000, 44_100]), 96_000);
    // The most common rate is not an allowed project rate.
    assert_eq!(rate(&[22_050, 22_050, 44_100]), 48_000);
}

#[test]
fn migrated_fades_are_clamped_to_the_region() {
    let mut v: Value = serde_json::from_str(&fixture("mixed_lanes")).unwrap();
    // The region is 48 000 frames (1 s, two beats at 120 BPM).
    let body = &mut v["document"]["sequences"][0]["lanes"][0]["takes"][0]["events"][2]["body"];
    assert_eq!(body["type"], "audio_region");
    body["fade_in"]["length"] = json!(2 * PPQ);
    body["fade_out"]["length"] = json!(3 * PPQ);
    let d = from_json(&v.to_string()).unwrap();
    assert_eq!(region_fades(&d), vec![(48_000, 0)]);
}

#[test]
fn a_project_format_must_use_an_allowed_rate() {
    let mut v: Value = serde_json::from_str(&to_json(&small_doc()).unwrap()).unwrap();
    v["document"]["settings"]["format"]["sample_rate"] = json!(22_050);
    match from_json(&v.to_string()) {
        Err(ModelError::Corrupt { problems }) => {
            assert!(problems.iter().any(|p| p.contains("settings.format")), "{problems:?}")
        }
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn new_documents_default_to_48k_24_bit() {
    assert_eq!(Document::new().settings().format, AudioFormat::DEFAULT);
    assert_eq!(
        AudioFormat::DEFAULT,
        AudioFormat {
            sample_rate: 48_000,
            depth: BitDepth::Int24
        }
    );
    assert_eq!(AudioFormat::DEFAULT.label(), "48 kHz, 24-bit");
    assert_eq!(
        AudioFormat {
            sample_rate: 44_100,
            depth: BitDepth::Float32
        }
        .label(),
        "44.1 kHz, 32-bit float"
    );
    assert_eq!(
        AudioFormat::RATES,
        [44_100, 48_000, 88_200, 96_000, 176_400, 192_000]
    );
}
```

In the same file, change the version assertions from 2 to 3.

Find:
```rust
    let v2 = to_json(&d).unwrap();
    assert!(v2.contains("\"version\": 2"), "{name}");
    // Canonical: a version-2 file re-serializes byte for byte.
    assert_eq!(to_json(&from_json(&v2).unwrap()).unwrap(), v2, "{name}");
```
Replace:
```rust
    let v3 = to_json(&d).unwrap();
    assert!(v3.contains("\"version\": 3"), "{name}");
    // Canonical: a version-3 file re-serializes byte for byte.
    assert_eq!(to_json(&from_json(&v3).unwrap()).unwrap(), v3, "{name}");
```

Find:
```rust
    let v: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    assert_eq!(v["version"], 2);
```
Replace:
```rust
    let v: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    assert_eq!(v["version"], 3);
```

Then run `grep -n '"version": 2\|\["version"\], 2\|VERSION, 2' crates/core-model/tests/file.rs`. Any further assertion that a freshly written file is version 2 becomes 3. Assertions about the *input* fixtures (version 1) stay as they are.

In `crates/core-model/tests/cmd_audio_routing.rs`, the fades become frames. At 48 kHz and 120 BPM a beat is 24 000 frames.

Find:
```rust
            fade_in: Fade {
                length: Tick(10),
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length: Tick(20),
                shape: FadeShape::SCurve,
            },
```
Replace:
```rust
            fade_in: Fade {
                length_frames: 10,
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length_frames: 20,
                shape: FadeShape::SCurve,
            },
```

Find:
```rust
    let f = Fade {
        length: Tick(99),
        shape: FadeShape::Exponential,
    };
```
Replace:
```rust
    let f = Fade {
        length_frames: 99,
        shape: FadeShape::Exponential,
    };
```

Find:
```rust
                fade_in: Fade {
                    length: Tick(-1),
                    shape: FadeShape::Linear
                },
                fade_out: f
```
Replace:
```rust
                // Together longer than the 96 000-frame region.
                fade_in: Fade {
                    length_frames: 95_950,
                    shape: FadeShape::Linear
                },
                fade_out: f
```

Find:
```rust
            fade_in: Fade {
                length: Tick(3 * PPQ / 2),
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length: Tick(2 * PPQ),
                shape: FadeShape::Linear,
            },
```
Replace:
```rust
            fade_in: Fade {
                length_frames: 36_000,
                shape: FadeShape::Linear,
            },
            fade_out: Fade {
                length_frames: 48_000,
                shape: FadeShape::Linear,
            },
```

Find:
```rust
    assert_eq!(fi0.length, Tick(PPQ));
    assert_eq!(fo1.length, Tick(2 * PPQ));
```
Replace:
```rust
    assert_eq!(fi0.length_frames, 24_000);
    assert_eq!(fo1.length_frames, 48_000);
```

Find:
```rust
    let (_, _, len, fi, fo) = region(&t, 0);
    assert_eq!(len, 24_000);
    assert_eq!((fi.length, fo.length), (Tick(PPQ), Tick(PPQ)));
```
Replace:
```rust
    let (_, _, len, fi, fo) = region(&t, 0);
    assert_eq!(len, 24_000);
    // The fade in takes the whole region; the fade out gets what is left.
    assert_eq!((fi.length_frames, fo.length_frames), (24_000, 0));
```

Update the doc comment of `with_long_fades` to say "a 36 000-frame (1.5-beat) fade in and a 48 000-frame (2-beat) fade out". Then append:

```rust
#[test]
fn fades_may_not_exceed_the_region() {
    let d = base();
    let insert = |fi: u64, fo: u64| {
        run(
            &d,
            audio(AudioCmd::Insert {
                seq: SeqId(0),
                lane: LaneId(0),
                tick: Tick(0),
                media: MediaId(0),
                source_offset: 0,
                length: 1000,
                gain_db: 0.0,
                fade_in: Fade {
                    length_frames: fi,
                    shape: FadeShape::Linear,
                },
                fade_out: Fade {
                    length_frames: fo,
                    shape: FadeShape::Linear,
                },
            }),
        )
    };
    assert!(insert(600, 400).is_ok());
    assert!(insert(1000, 0).is_ok());
    assert!(insert(601, 400).is_err());
    assert!(insert(u64::MAX, 1).is_err());
}
```

In `crates/core-model/tests/model_types.rs`:

Find:
```rust
            fade_out: Fade {
                length: Tick(100),
                shape: FadeShape::EqualPower,
            },
```
Replace:
```rust
            fade_out: Fade {
                length_frames: 100,
                shape: FadeShape::EqualPower,
            },
```

The `MediaItem` literals in `crates/core-model/tests/model_types.rs`, `crates/core-model/tests/rules.rs` and `crates/core-model/tests/query_usage.rs` each gain two fields. Each file has exactly one `content_hash: None,` followed by `}],`.

Find (in each of the three files):
```rust
            content_hash: None,
        }],
```
Replace:
```rust
            content_hash: None,
            name: "a".into(),
            origin: MediaOrigin::default(),
        }],
```
Then add `MediaOrigin` to each file's `use core_model::document::{…}` import.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-model --test file --test cmd_audio_routing 2>&1 | tail -20`
Expected: compile errors: `unresolved import core_model::format`, `no field length_frames`, `migrate_v2_to_v3` not found.

- [ ] **Step 3: Implement**

Create `crates/core-model/src/format.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/src/format.rs <<'VIZDAW_EOF'
//! The project's audio format (sub-project 5 spec §4.1): every media file
//! of a project is stored at this sample rate and bit depth. The engine
//! always processes 32-bit float; the format governs only the files and
//! the rate the device is asked to run at.

use serde::{Deserialize, Serialize};

/// Bit depth of a project's media files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    /// 16-bit integer (written with TPDF dither).
    Int16,
    /// 24-bit integer.
    Int24,
    /// 32-bit float.
    Float32,
}

impl BitDepth {
    /// Bits per sample.
    pub fn bits(self) -> u32 {
        match self {
            BitDepth::Int16 => 16,
            BitDepth::Int24 => 24,
            BitDepth::Float32 => 32,
        }
    }

    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        match self {
            BitDepth::Int16 => 2,
            BitDepth::Int24 => 3,
            BitDepth::Float32 => 4,
        }
    }

    /// Display name ("24-bit", "32-bit float").
    pub fn label(self) -> &'static str {
        match self {
            BitDepth::Int16 => "16-bit",
            BitDepth::Int24 => "24-bit",
            BitDepth::Float32 => "32-bit float",
        }
    }

    /// Every depth, smallest first.
    pub fn all() -> [BitDepth; 3] {
        [BitDepth::Int16, BitDepth::Int24, BitDepth::Float32]
    }
}

/// A project's audio format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioFormat {
    /// Sample rate in Hz, one of [`AudioFormat::RATES`].
    pub sample_rate: u32,
    /// Bit depth of the media files.
    pub depth: BitDepth,
}

impl AudioFormat {
    /// The allowed project rates.
    pub const RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

    /// The default: 48 000 Hz, 24-bit integer.
    pub const DEFAULT: AudioFormat = AudioFormat {
        sample_rate: 48_000,
        depth: BitDepth::Int24,
    };

    /// True when the rate is an allowed project rate.
    pub fn is_valid(&self) -> bool {
        AudioFormat::RATES.contains(&self.sample_rate)
    }

    /// Display text, e.g. "44.1 kHz, 16-bit".
    pub fn label(&self) -> String {
        let khz = f64::from(self.sample_rate) / 1000.0;
        let rate = if self.sample_rate % 1000 == 0 {
            format!("{khz:.0}")
        } else {
            format!("{khz:.1}")
        };
        format!("{rate} kHz, {}", self.depth.label())
    }
}

impl Default for AudioFormat {
    fn default() -> AudioFormat {
        AudioFormat::DEFAULT
    }
}
VIZDAW_EOF
```

In `crates/core-model/src/lib.rs`:

Find:
```rust
pub mod file;
pub mod history;
```
Replace:
```rust
pub mod file;
pub mod format;
pub mod history;
```

In `crates/core-model/src/document.rs`:

Find:
```rust
    /// Named select-and-modify templates.
    pub templates: OrdMap<String, ModifyTemplate>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            history_depth: 200,
            autosave_keep: 10,
            templates: OrdMap::new(),
        }
    }
}
```
Replace:
```rust
    /// Named select-and-modify templates.
    pub templates: OrdMap<String, ModifyTemplate>,
    /// The project's audio format: every media file is stored in it
    /// (sub-project 5 spec §4.1).
    pub format: AudioFormat,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            history_depth: 200,
            autosave_keep: 10,
            templates: OrdMap::new(),
            format: AudioFormat::DEFAULT,
        }
    }
}
```

Find:
```rust
use crate::event::Event;
```
Replace:
```rust
use crate::event::Event;
use crate::format::AudioFormat;
```

Find:
```rust
    /// Optional content hash.
    pub content_hash: Option<String>,
}

/// Where an event lives.
```
Replace:
```rust
    /// BLAKE3 hex of the CAF's audio bytes; `None` in projects older than
    /// version 3 (5a Ruling 4).
    pub content_hash: Option<String>,
    /// Display name (defaults to the file stem).
    pub name: String,
    /// Where the media came from.
    pub origin: MediaOrigin,
}

/// Where a media item came from (sub-project 5 spec §4.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaOrigin {
    /// Imported (converted) from a file.
    Imported {
        /// The original file's name.
        original_name: String,
    },
    /// Recorded in a recording pass.
    Recorded {
        /// The pass number.
        pass: u32,
    },
    /// A bounce placed into the project.
    Bounced,
    /// Returned by the external sample editor.
    ExternalEdit {
        /// The item the edit started from.
        from: MediaId,
    },
}

impl Default for MediaOrigin {
    fn default() -> MediaOrigin {
        MediaOrigin::Imported {
            original_name: String::new(),
        }
    }
}

impl MediaItem {
    /// `path`'s file name and stem (`("a.caf", "a")`).
    pub fn file_parts(path: &str) -> (String, String) {
        let file = path.rsplit('/').next().unwrap_or(path).to_string();
        let stem = match file.rsplit_once('.') {
            Some((s, _)) if !s.is_empty() => s.to_string(),
            _ => file.clone(),
        };
        (file, stem)
    }
}

/// Where an event lives.
```

In `crates/core-model/src/event.rs`:

Find:
```rust
/// A re-editable fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    /// Fade length in ticks (>= 0).
    pub length: Tick,
    /// Curve shape.
    pub shape: FadeShape,
}

impl Default for Fade {
    fn default() -> Self {
        Fade {
            length: Tick::ZERO,
            shape: FadeShape::Linear,
        }
    }
}
```
Replace:
```rust
/// A re-editable fade. Its length is in sample frames of the region's
/// media, so a tempo change never changes it (sub-project 5 spec §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fade {
    /// Fade length in frames; a region's fades together are at most its
    /// length.
    pub length_frames: u64,
    /// Curve shape.
    pub shape: FadeShape,
}

impl Default for Fade {
    fn default() -> Self {
        Fade {
            length_frames: 0,
            shape: FadeShape::Linear,
        }
    }
}
```
If `Tick` is no longer used in `event.rs` after this, the compiler says so. It still is (event ticks), so nothing changes.

In `crates/core-model/src/rules.rs`:

Find:
```rust
            need(fade_in.length.0 >= 0, "fade_in.length", fade_in.length.0)?;
            need(fade_out.length.0 >= 0, "fade_out.length", fade_out.length.0)?;
            check_ticks("fade_in.length", fade_in.length.0)?;
            check_ticks("fade_out.length", fade_out.length.0)?;
```
Replace:
```rust
            let fades = fade_in
                .length_frames
                .saturating_add(fade_out.length_frames);
            need(fades <= *length, "region.fades", fades)?;
```

Find:
```rust
    for m in doc.media().values() {
        if m.id.0 >= c.media {
```
Replace:
```rust
    let f = doc.settings().format;
    push(
        need(f.is_valid(), "settings.format", f.sample_rate),
        "settings",
    );
    for m in doc.media().values() {
        if m.id.0 >= c.media {
```

In `crates/core-model/src/command/audio.rs`:

Find:
```rust
/// Shortens a region's fades so neither is longer than the region itself
/// (measured in ticks under `tempo` from the region's start).
fn clamp_fades(ev: &mut Event, tempo: &TempoMap, rate: f64) {
    if let EventBody::AudioRegion {
        length,
        fade_in,
        fade_out,
        ..
    } = &mut ev.body
    {
        let start = tempo.tick_to_seconds(ev.tick);
        let part = tempo.seconds_to_tick(start + *length as f64 / rate) - ev.tick;
        fade_in.length = fade_in.length.min(part);
        fade_out.length = fade_out.length.min(part);
    }
}
```
Replace:
```rust
/// Shortens a region's fades so together they fit it: the fade in first,
/// then the fade out to what is left (5a Ruling 1).
pub(crate) fn clamp_fades(ev: &mut Event) {
    if let EventBody::AudioRegion {
        length,
        fade_in,
        fade_out,
        ..
    } = &mut ev.body
    {
        fade_in.length_frames = fade_in.length_frames.min(*length);
        fade_out.length_frames = fade_out
            .length_frames
            .min(length.saturating_sub(fade_in.length_frames));
    }
}
```

Then replace every `clamp_fades(&mut ev, &tempo, rate);` with `clamp_fades(&mut ev);`, every `clamp_fades(&mut a, &tempo, rate);` with `clamp_fades(&mut a);`, and every `clamp_fades(&mut b, &tempo, rate);` with `clamp_fades(&mut b);` (three call sites). If `TempoMap` is now unused in this file, remove it from the imports. `Trim` and `Split` still read `tempo` for their tick arithmetic.

In `crates/core-model/src/command/routing.rs` (Task 2 moves this command; here it only fills the new fields):

Find:
```rust
            check_media_path(path)?;
            let id = d.counters_mut().alloc_media();
            d.media_mut().insert(
                id,
                MediaItem {
                    id,
                    path: path.clone(),
                    sample_rate: *sample_rate,
                    channels: *channels,
                    length_frames: *length_frames,
                    content_hash: content_hash.clone(),
                },
            );
```
Replace:
```rust
            check_media_path(path)?;
            let id = d.counters_mut().alloc_media();
            let (file, stem) = MediaItem::file_parts(path);
            d.media_mut().insert(
                id,
                MediaItem {
                    id,
                    path: path.clone(),
                    sample_rate: *sample_rate,
                    channels: *channels,
                    length_frames: *length_frames,
                    content_hash: content_hash.clone(),
                    name: stem,
                    origin: MediaOrigin::Imported {
                        original_name: file,
                    },
                },
            );
```
Add `MediaOrigin` to that file's `use crate::document::{…}`.

In `crates/core-model/src/file.rs`:

Find:
```rust
//! `project.json` is `{ "format": "vizdaw-project", "version": 2,
```
Replace:
```rust
//! `project.json` is `{ "format": "vizdaw-project", "version": 3,
```

Find:
```rust
/// The version this build writes.
pub const VERSION: u64 = 2;
```
Replace:
```rust
/// The version this build writes.
pub const VERSION: u64 = 3;
```

Find:
```rust
/// Migrations indexed by `from - FIRST_VERSION`.
pub const MIGRATIONS: &[Migration] = &[migrate_v1_to_v2];
```
Replace:
```rust
/// Migrations indexed by `from - FIRST_VERSION`.
pub const MIGRATIONS: &[Migration] = &[migrate_v1_to_v2, migrate_v2_to_v3];

/// The project rate a migrated project gets (5a Ruling 2): the most
/// common media rate (the first item's on a tie), or 48 kHz when there is
/// no media or that rate is not an allowed project rate.
fn majority_rate(rates: &[u64]) -> u64 {
    let mut best: Option<(u64, usize)> = None;
    for r in rates {
        let n = rates.iter().filter(|x| *x == r).count();
        if best.is_none_or(|(_, m)| n > m) {
            best = Some((*r, n));
        }
    }
    best.map(|(r, _)| r)
        .filter(|r| u32::try_from(*r).is_ok_and(|r| AudioFormat::RATES.contains(&r)))
        .unwrap_or(u64::from(AudioFormat::DEFAULT.sample_rate))
}

/// A fade of `ticks` from `start` under `tempo`, in frames at `rate`.
fn fade_frames(tempo: &TempoMap, start: i64, ticks: i64, rate: f64) -> u64 {
    let s = tempo.tick_to_seconds(Tick(start));
    let e = tempo.tick_to_seconds(Tick(start.saturating_add(ticks.max(0))));
    // Float-to-int `as` saturates (and maps NaN to 0).
    ((e - s) * rate).round().max(0.0) as u64
}

/// Version 2 → 3 (sub-project 5 spec §4.1–§4.3, 5a Ruling 2): settings
/// gain `format` (int24 at the majority media rate), media items gain
/// `name` and `origin`, and region fades turn from ticks into frames at
/// the tempo in force at the region's start, clamped to the region.
pub fn migrate_v2_to_v3(mut v: Value) -> Result<Value> {
    let doc = v
        .get_mut("document")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| corrupt("missing document"))?;
    let mut rates: Vec<u64> = Vec::new();
    let mut media_rate: BTreeMap<u64, f64> = BTreeMap::new();
    if let Some(Value::Array(items)) = doc.get_mut("media") {
        for m in items.iter_mut() {
            let obj = m
                .as_object_mut()
                .ok_or_else(|| corrupt("media item is not an object"))?;
            let path = obj
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let (file, stem) = crate::document::MediaItem::file_parts(&path);
            obj.insert("name".into(), Value::from(stem));
            obj.insert(
                "origin".into(),
                json!({ "imported": { "original_name": file } }),
            );
            let rate = obj.get("sample_rate").and_then(Value::as_u64).unwrap_or(0);
            if let Some(id) = obj.get("id").and_then(Value::as_u64) {
                media_rate.insert(id, rate as f64);
            }
            rates.push(rate);
        }
    }
    let rate = majority_rate(&rates);
    doc.get_mut("settings")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| corrupt("missing settings"))?
        .insert(
            "format".into(),
            json!({ "sample_rate": rate, "depth": "int24" }),
        );
    if let Some(Value::Array(seqs)) = doc.get_mut("sequences") {
        for s in seqs.iter_mut() {
            let tempo: TempoMap = match s.get("tempo") {
                Some(t) => serde_json::from_value(t.clone()).map_err(corrupt)?,
                None => TempoMap::default(),
            };
            let Some(Value::Array(lanes)) = s.get_mut("lanes") else {
                continue;
            };
            for lane in lanes.iter_mut() {
                let Some(Value::Array(takes)) = lane.get_mut("takes") else {
                    continue;
                };
                for take in takes.iter_mut() {
                    let Some(Value::Array(events)) = take.get_mut("events") else {
                        continue;
                    };
                    for ev in events.iter_mut() {
                        migrate_region_fades(ev, &tempo, &media_rate);
                    }
                }
            }
        }
    }
    Ok(v)
}

/// One event of [`migrate_v2_to_v3`]: an audio region's fades to frames.
fn migrate_region_fades(ev: &mut Value, tempo: &TempoMap, media_rate: &BTreeMap<u64, f64>) {
    let tick = ev.get("tick").and_then(Value::as_i64).unwrap_or(0);
    let Some(body) = ev.get_mut("body").and_then(Value::as_object_mut) else {
        return;
    };
    if body.get("type").and_then(Value::as_str) != Some("audio_region") {
        return;
    }
    let rate = body
        .get("media")
        .and_then(Value::as_u64)
        .and_then(|m| media_rate.get(&m).copied())
        .unwrap_or(f64::from(AudioFormat::DEFAULT.sample_rate));
    let length = body.get("length").and_then(Value::as_u64).unwrap_or(0);
    let ticks = |name: &str, body: &serde_json::Map<String, Value>| {
        body.get(name)
            .and_then(|f| f.get("length"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    let fi = fade_frames(tempo, tick, ticks("fade_in", body), rate).min(length);
    let fo = fade_frames(tempo, tick, ticks("fade_out", body), rate).min(length - fi);
    for (name, frames) in [("fade_in", fi), ("fade_out", fo)] {
        if let Some(f) = body.get_mut(name).and_then(Value::as_object_mut) {
            f.remove("length");
            f.insert("length_frames".into(), Value::from(frames));
        }
    }
}
```

Find:
```rust
use crate::rules::validate;
```
Replace:
```rust
use crate::format::AudioFormat;
use crate::rules::validate;
use crate::tempo::TempoMap;
use crate::time::Tick;
use std::collections::BTreeMap;
```

In `crates/core-ffi/src/events.rs`:

Find:
```rust
pub struct FadeRow {
    /// Length in ticks.
    pub length: i64,
```
Replace:
```rust
pub struct FadeRow {
    /// Length in frames of the region's media (5a Ruling 1; ticks before
    /// file format 3).
    pub length: i64,
```

Find:
```rust
pub fn fade_row(f: Fade) -> FadeRow {
    FadeRow {
        length: f.length.0,
```
Replace:
```rust
pub fn fade_row(f: Fade) -> FadeRow {
    FadeRow {
        length: i64::try_from(f.length_frames).unwrap_or(i64::MAX),
```

Find:
```rust
pub fn fade_of(f: &FadeRow) -> Fade {
    Fade {
        length: Tick(f.length),
```
Replace:
```rust
pub fn fade_of(f: &FadeRow) -> Fade {
    Fade {
        // A negative length is no fade.
        length_frames: u64::try_from(f.length).unwrap_or(0),
```
If `Tick` becomes unused in `events.rs`, the compiler says so; it is still used elsewhere in that file.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-model 2>&1 | tail -30`
Expected: all pass, including the five new `file` tests and `fades_may_not_exceed_the_region`. The insta snapshots of the golden fixtures do not change: their summary lists flattened events, not fades.

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c`
Expected: every `test result: ok`. Two kinds of failure are expected and fixed in the same way:
- A test in another crate that builds a `Fade { length: Tick(n), … }`: use `length_frames: n` in frames.
- A test that checks a saved file's version is 2: change it to 3.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-model crates/core-ffi/src/events.rs
git commit -m "model: project audio format, fades in frames, named media (file format 3)

Settings gain an AudioFormat (rate + bit depth, default 48 kHz/24-bit),
fades are frames so tempo changes never alter them, and media items get a
name and origin. Version 2 projects migrate: the majority media rate,
fades converted at the region's start tempo and clamped.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 2: Media commands, project format change, and output paths for new regions

**Files:**
- Create: `crates/core-model/src/command/media.rs`, `crates/core-model/tests/cmd_media.rs`
- Modify: `crates/core-model/src/command/mod.rs`, `crates/core-model/src/command/routing.rs`, `crates/core-model/src/command/settings.rs`, `crates/core-model/src/command/audio.rs`, `crates/core-engine/src/fixtures.rs`, and the tests that add media: `crates/core-model/tests/{cmd_audio_routing,cmd_reference,query_flatten}.rs`, `crates/core-ffi/tests/{edit,events,revs}.rs`
- Test: `crates/core-model/tests/cmd_media.rs`

**Interfaces:**
- Consumes:
  - Task 1's `AudioFormat`, `MediaOrigin`, `MediaItem::file_parts` and `command::audio::clamp_fades`;
  - `routing::require_replacement`, `strips::add_audio_path_strip`;
  - `Document::{media_mut, replace_event, all_events, event}`, `rules::{check_media_path, check_event_fields}`.
- Produces:
  - `core_model::command::{MediaCmd, MediaSwap}`, with `MediaCmd::{Add { path, sample_rate, channels, length_frames, content_hash, name: Option<String>, origin: Option<MediaOrigin> }, Remove { media, replacement }, Rename { media, name }, Relink { media, path, content_hash, length_frames, channels, sample_rate }, Compact, Convert { swaps: Vec<MediaSwap> }}`;
  - `MediaSwap { media: MediaId, path: String, length_frames: u64, content_hash: Option<String> }`;
  - `Command::Media(MediaCmd)`;
  - `SettingsCmd::SetFormat { format: AudioFormat, swaps: Vec<MediaSwap> }` (name "Change Project Format");
  - `command::media::{used_media(&Document) -> BTreeSet<MediaId>, regions_using(&Document, MediaId) -> Vec<EventId>}`.
- `RoutingCmd::{AddMedia, RemoveMedia}` are gone.

Rulings 5 and 6.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-model/tests/cmd_media.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/cmd_media.rs <<'VIZDAW_EOF'
//! Media-pool commands, project format changes and output paths for new
//! regions (plan 5a Task 2, Rulings 5 and 6).

use core_model::ModelError;
use core_model::command::{
    AudioCmd, Command, LaneCmd, MediaCmd, MediaSwap, RoutingCmd, SequenceCmd, SettingsCmd, apply,
};
use core_model::document::{Document, MediaOrigin};
use core_model::event::{EventBody, Fade, FadeShape};
use core_model::format::{AudioFormat, BitDepth};
use core_model::history::History;
use core_model::ids::{AnyId, EventId, LaneId, MediaId, OutputPathId, SeqId};
use core_model::mixer::StripKind;
use core_model::selection::ViewFilter;
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn add(path: &str, rate: u32, frames: u64) -> Command {
    Command::Media(MediaCmd::Add {
        path: path.into(),
        sample_rate: rate,
        channels: 2,
        length_frames: frames,
        content_hash: None,
        name: None,
        origin: None,
    })
}

fn fade(n: u64) -> Fade {
    Fade {
        length_frames: n,
        shape: FadeShape::Linear,
    }
}

/// Sequence 0 with lane 0; media 0 (`Media/a.caf`, 48 kHz, 192 000
/// frames) and media 1 (`Media/b.caf`, unused).
fn base() -> Document {
    let mut d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap();
    d = run(&d, add("Media/a.caf", 48_000, 192_000)).unwrap();
    run(&d, add("Media/b.caf", 48_000, 1000)).unwrap()
}

/// `base` plus a region of media 0: offset 48 000, 96 000 frames, fades
/// 4800 in and 2400 out.
fn with_region() -> Document {
    run(
        &base(),
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 48_000,
            length: 96_000,
            gain_db: 0.0,
            fade_in: fade(4800),
            fade_out: fade(2400),
        }),
    )
    .unwrap()
}

/// `(source_offset, length, fade in, fade out)` of region `id`.
fn region(d: &Document, id: u64) -> (u64, u64, u64, u64) {
    match d.event(EventId(id)).unwrap().0.body {
        EventBody::AudioRegion {
            source_offset,
            length,
            fade_in,
            fade_out,
            ..
        } => (
            source_offset,
            length,
            fade_in.length_frames,
            fade_out.length_frames,
        ),
        _ => panic!("not a region"),
    }
}

fn swap(media: u64, path: &str, frames: u64) -> MediaSwap {
    MediaSwap {
        media: MediaId(media),
        path: path.into(),
        length_frames: frames,
        content_hash: Some(format!("hash-{media}")),
    }
}

#[test]
fn add_names_items_after_their_file_unless_told() {
    let d = base();
    let a = &d.media()[&MediaId(0)];
    assert_eq!(a.name, "a");
    assert_eq!(
        a.origin,
        MediaOrigin::Imported {
            original_name: "a.caf".into()
        }
    );
    let d = run(
        &d,
        Command::Media(MediaCmd::Add {
            path: "Media/c.caf".into(),
            sample_rate: 48_000,
            channels: 1,
            length_frames: 10,
            content_hash: Some("h".into()),
            name: Some("Vocal".into()),
            origin: Some(MediaOrigin::Imported {
                original_name: "vocal take.wav".into(),
            }),
        }),
    )
    .unwrap();
    let c = &d.media()[&MediaId(2)];
    assert_eq!(
        (c.name.as_str(), c.content_hash.as_deref()),
        ("Vocal", Some("h"))
    );
}

#[test]
fn rename_relink_and_compact() {
    let d = with_region();
    let d = run(
        &d,
        Command::Media(MediaCmd::Rename {
            media: MediaId(0),
            name: "Drums".into(),
        }),
    )
    .unwrap();
    assert_eq!(d.media()[&MediaId(0)].name, "Drums");
    // The region uses frames up to 144 000: a shorter file is refused.
    let relink = |frames: u64| {
        Command::Media(MediaCmd::Relink {
            media: MediaId(0),
            path: "Media/a2.caf".into(),
            content_hash: Some("x".into()),
            length_frames: frames,
            channels: 2,
            sample_rate: 48_000,
        })
    };
    assert!(run(&d, relink(143_999)).is_err());
    let r = run(&d, relink(144_000)).unwrap();
    let m = &r.media()[&MediaId(0)];
    assert_eq!(
        (m.path.as_str(), m.length_frames, m.name.as_str()),
        ("Media/a2.caf", 144_000, "Drums")
    );
    let a = apply(&d, &Command::Media(MediaCmd::Compact)).unwrap();
    assert_eq!(a.report.removed, vec![AnyId::Media(MediaId(1))]);
    assert_eq!(
        a.doc.media().keys().copied().collect::<Vec<_>>(),
        vec![MediaId(0)]
    );
    assert_eq!(
        Command::Media(MediaCmd::Compact).name(),
        "Compact Media"
    );
}

#[test]
fn remove_keeps_its_old_semantics() {
    let d = with_region();
    assert_eq!(
        run(
            &d,
            Command::Media(MediaCmd::Remove {
                media: MediaId(0),
                replacement: None
            })
        ),
        Err(ModelError::InUse {
            users: vec![AnyId::Event(EventId(0))]
        })
    );
    // Media 1 has 1000 frames: too short for the region.
    assert!(
        run(
            &d,
            Command::Media(MediaCmd::Remove {
                media: MediaId(0),
                replacement: Some(MediaId(1))
            })
        )
        .is_err()
    );
}

#[test]
fn set_format_rescales_regions_and_undo_restores() {
    let d = with_region();
    let target = AudioFormat {
        sample_rate: 44_100,
        depth: BitDepth::Int16,
    };
    let cmd = Command::Settings(SettingsCmd::SetFormat {
        format: target,
        swaps: vec![
            swap(0, "Media/a 2.caf", 176_400),
            swap(1, "Media/b 2.caf", 919),
        ],
    });
    assert_eq!(cmd.name(), "Change Project Format");
    let mut h = History::new(d.clone());
    h.execute(&cmd, &ViewFilter::default()).unwrap();
    let after = h.doc().clone();
    assert_eq!(after.settings().format, target);
    // 44.1 / 48: 48 000 → 44 100, 96 000 → 88 200, 4800 → 4410, 2400 → 2205.
    assert_eq!(region(&after, 0), (44_100, 88_200, 4410, 2205));
    let a = &after.media()[&MediaId(0)];
    assert_eq!(
        (
            a.path.as_str(),
            a.sample_rate,
            a.length_frames,
            a.content_hash.as_deref()
        ),
        ("Media/a 2.caf", 44_100, 176_400, Some("hash-0"))
    );
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
}

#[test]
fn set_format_must_convert_every_item_once() {
    let d = with_region();
    let to = |swaps: Vec<MediaSwap>, rate: u32| {
        run(
            &d,
            Command::Settings(SettingsCmd::SetFormat {
                format: AudioFormat {
                    sample_rate: rate,
                    depth: BitDepth::Int24,
                },
                swaps,
            }),
        )
    };
    // Media 1 is missing.
    assert!(to(vec![swap(0, "Media/x.caf", 10)], 44_100).is_err());
    // Media 0 twice.
    assert!(
        to(
            vec![
                swap(0, "Media/x.caf", 10),
                swap(0, "Media/y.caf", 10),
                swap(1, "Media/z.caf", 10)
            ],
            44_100
        )
        .is_err()
    );
    // Not an allowed rate.
    assert!(
        to(
            vec![swap(0, "Media/x.caf", 10), swap(1, "Media/z.caf", 10)],
            22_050
        )
        .is_err()
    );
    // The same format is refused: that is Convert's job.
    assert!(
        to(
            vec![swap(0, "Media/x.caf", 192_000), swap(1, "Media/z.caf", 1000)],
            48_000
        )
        .is_err()
    );
    // An empty project changes format without swaps.
    let e = run(
        &Document::new(),
        Command::Settings(SettingsCmd::SetFormat {
            format: AudioFormat {
                sample_rate: 96_000,
                depth: BitDepth::Float32,
            },
            swaps: vec![],
        }),
    )
    .unwrap();
    assert_eq!(e.settings().format.sample_rate, 96_000);
}

#[test]
fn convert_swaps_some_items_at_the_project_rate() {
    // An old 44.1 kHz item in a 48 kHz project.
    let mut d = run(&base(), add("Media/old.wav", 44_100, 88_200)).unwrap();
    d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            media: MediaId(2),
            source_offset: 0,
            length: 88_200,
            gain_db: 0.0,
            fade_in: fade(0),
            fade_out: fade(0),
        }),
    )
    .unwrap();
    let cmd = Command::Media(MediaCmd::Convert {
        swaps: vec![swap(2, "Media/old.caf", 96_000)],
    });
    assert_eq!(cmd.name(), "Convert Media");
    let after = run(&d, cmd).unwrap();
    assert_eq!(after.media()[&MediaId(2)].sample_rate, 48_000);
    assert_eq!(region(&after, 0), (0, 96_000, 0, 0));
    assert!(
        run(
            &d,
            Command::Media(MediaCmd::Convert { swaps: vec![] })
        )
        .is_err()
    );
}

#[test]
fn a_shorter_converted_file_clamps_its_regions() {
    let d = with_region();
    let after = run(
        &d,
        Command::Media(MediaCmd::Convert {
            swaps: vec![swap(0, "Media/a 2.caf", 100_000)],
        }),
    )
    .unwrap();
    // Offset 48 000 stays; the length shrinks to fit 100 000 frames, and
    // the fade out gets what is left after the fade in.
    assert_eq!(region(&after, 0), (48_000, 52_000, 4800, 2400));
}

#[test]
fn inserting_a_region_gives_the_lane_an_output_path() {
    let d = with_region();
    let lane = d.lane(SeqId(0), LaneId(0)).unwrap();
    assert_eq!(lane.audio_out, Some(OutputPathId(0)));
    let out = &d.outputs()[&OutputPathId(0)];
    assert_eq!((out.name.as_str(), out.reference.as_str()), ("Audio 1", "audio:1-2"));
    assert!(
        d.mixer()
            .find_kind(StripKind::AudioPath(OutputPathId(0)))
            .is_some()
    );
    // A second lane gets the first existing output path.
    let d = run(
        &d,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "B".into(),
            index: None,
        }),
    )
    .unwrap();
    let d = run(
        &d,
        Command::Routing(RoutingCmd::AddOutput {
            name: "Later".into(),
            reference: "bus:9".into(),
        }),
    )
    .unwrap();
    let lane_b = d.sequence(SeqId(0)).unwrap().lanes[1].id;
    let d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: lane_b,
            tick: Tick(0),
            media: MediaId(1),
            source_offset: 0,
            length: 10,
            gain_db: 0.0,
            fade_in: fade(0),
            fade_out: fade(0),
        }),
    )
    .unwrap();
    assert_eq!(
        d.lane(SeqId(0), lane_b).unwrap().audio_out,
        Some(OutputPathId(0))
    );
    assert_eq!(d.outputs().len(), 2);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-model --test cmd_media 2>&1 | tail -5`
Expected: compile errors (`MediaCmd`, `MediaSwap`, `SettingsCmd::SetFormat` not found).

- [ ] **Step 3: Implement**

Create `crates/core-model/src/command/media.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/src/command/media.rs <<'VIZDAW_EOF'
//! Media-pool commands (sub-project 5 spec §4.3; plan 5a Ruling 5).
//! Removing an item in use is refused unless a replacement is given;
//! converting gives items new files and rescales their regions.

use super::Report;
use super::audio::clamp_fades;
use super::routing::require_replacement;
use crate::document::{Document, MediaItem, MediaOrigin};
use crate::error::{ModelError, Result};
use crate::event::EventBody;
use crate::ids::{AnyId, EventId, MediaId};
use crate::rules::{check_event_fields, check_media_path};
use std::collections::BTreeSet;

/// A media item's new file (a conversion): the same item, so its regions
/// keep it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSwap {
    /// The item.
    pub media: MediaId,
    /// The new file, package-relative.
    pub path: String,
    /// The new file's length in frames (> 0).
    pub length_frames: u64,
    /// The new file's content hash.
    pub content_hash: Option<String>,
}

/// Media-pool edits.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaCmd {
    /// Add a media item.
    Add {
        /// Package-relative path.
        path: String,
        /// Sample rate (> 0).
        sample_rate: u32,
        /// Channels (> 0).
        channels: u16,
        /// Length in frames.
        length_frames: u64,
        /// Content hash.
        content_hash: Option<String>,
        /// Display name; `None`: the file stem.
        name: Option<String>,
        /// Origin; `None`: imported from a file of this name.
        origin: Option<MediaOrigin>,
    },
    /// Remove a media item, re-pointing regions to `replacement`.
    Remove {
        /// Target.
        media: MediaId,
        /// Replacement (required when in use; must be long enough).
        replacement: Option<MediaId>,
    },
    /// Rename a media item (display name only).
    Rename {
        /// Target.
        media: MediaId,
        /// New name.
        name: String,
    },
    /// Point a media item at another file; refused while a region uses
    /// frames beyond the new file's end.
    Relink {
        /// Target.
        media: MediaId,
        /// New package-relative path.
        path: String,
        /// The new file's content hash.
        content_hash: Option<String>,
        /// The new file's length in frames.
        length_frames: u64,
        /// The new file's channels (> 0).
        channels: u16,
        /// The new file's sample rate (> 0).
        sample_rate: u32,
    },
    /// Remove every item no region uses.
    Compact,
    /// New files for some items, at the project's rate (the banner's
    /// "Convert" for media in another format).
    Convert {
        /// The conversions (at least one).
        swaps: Vec<MediaSwap>,
    },
}

impl MediaCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            MediaCmd::Add { .. } => "Add Media",
            MediaCmd::Remove { .. } => "Remove Media",
            MediaCmd::Rename { .. } => "Rename Media",
            MediaCmd::Relink { .. } => "Relink Media",
            MediaCmd::Compact => "Compact Media",
            MediaCmd::Convert { .. } => "Convert Media",
        }
    }
}

/// The regions (in any take) that use `media`.
pub fn regions_using(d: &Document, media: MediaId) -> Vec<EventId> {
    d.all_events()
        .filter(|(_, e)| matches!(e.body, EventBody::AudioRegion { media: m, .. } if m == media))
        .map(|(_, e)| e.id)
        .collect()
}

/// Every media item some region uses.
pub fn used_media(d: &Document) -> BTreeSet<MediaId> {
    d.all_events()
        .filter_map(|(_, e)| match e.body {
            EventBody::AudioRegion { media, .. } => Some(media),
            _ => None,
        })
        .collect()
}

fn item(d: &Document, media: MediaId) -> Result<&MediaItem> {
    d.media()
        .get(&media)
        .ok_or(ModelError::UnknownId(AnyId::Media(media)))
}

/// Gives each swapped item its new file at `rate` and rescales its
/// regions by `rate / old rate` (offset, length and fades rounded to the
/// nearest frame; the length kept ≥ 1 and the region clamped into the new
/// file; the fades clamped as in Ruling 1).
pub(crate) fn swap_media(d: &mut Document, swaps: &[MediaSwap], rate: u32) -> Result<()> {
    let mut seen = BTreeSet::new();
    for s in swaps {
        if !seen.insert(s.media) {
            return Err(ModelError::invalid(format!(
                "{} is converted twice",
                s.media
            )));
        }
        check_media_path(&s.path)?;
        if s.length_frames == 0 {
            return Err(ModelError::range("media.length_frames", 0));
        }
        let old_rate = item(d, s.media)?.sample_rate.max(1);
        let ratio = f64::from(rate) / f64::from(old_rate);
        if let Some(m) = d.media_mut().get_mut(&s.media) {
            m.path = s.path.clone();
            m.length_frames = s.length_frames;
            m.content_hash = s.content_hash.clone();
            m.sample_rate = rate;
        }
        // Float-to-int `as` saturates; ratio 1 maps every value to itself.
        let scale = |x: u64| (x as f64 * ratio).round() as u64;
        for id in regions_using(d, s.media) {
            let mut ev = d.event(id)?.0.clone();
            if let EventBody::AudioRegion {
                source_offset,
                length,
                fade_in,
                fade_out,
                ..
            } = &mut ev.body
            {
                *source_offset = scale(*source_offset).min(s.length_frames - 1);
                *length = scale(*length)
                    .max(1)
                    .min(s.length_frames - *source_offset);
                fade_in.length_frames = scale(fade_in.length_frames);
                fade_out.length_frames = scale(fade_out.length_frames);
            }
            clamp_fades(&mut ev);
            check_event_fields(d, &ev.spec())?;
            d.replace_event(ev)?;
        }
    }
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &MediaCmd, r: &mut Report) -> Result<()> {
    match cmd {
        MediaCmd::Add {
            path,
            sample_rate,
            channels,
            length_frames,
            content_hash,
            name,
            origin,
        } => {
            if *sample_rate == 0 || *channels == 0 {
                return Err(ModelError::range("media.format", sample_rate));
            }
            check_media_path(path)?;
            let id = d.counters_mut().alloc_media();
            let (file, stem) = MediaItem::file_parts(path);
            d.media_mut().insert(
                id,
                MediaItem {
                    id,
                    path: path.clone(),
                    sample_rate: *sample_rate,
                    channels: *channels,
                    length_frames: *length_frames,
                    content_hash: content_hash.clone(),
                    name: name.clone().unwrap_or(stem),
                    origin: origin.clone().unwrap_or(MediaOrigin::Imported {
                        original_name: file,
                    }),
                },
            );
            r.created.push(AnyId::Media(id));
        }
        MediaCmd::Remove { media, replacement } => {
            item(d, *media)?;
            let regions = regions_using(d, *media);
            let users = regions.iter().map(|e| AnyId::Event(*e)).collect();
            let exists = replacement.is_some_and(|x| d.media().contains_key(&x));
            let rep = require_replacement(*media, *replacement, exists, users)?;
            if let Some(rep) = rep {
                for id in regions {
                    let mut ev = d.event(id)?.0.clone();
                    if let EventBody::AudioRegion { media: m, .. } = &mut ev.body {
                        *m = rep;
                    }
                    check_event_fields(d, &ev.spec())?;
                    d.replace_event(ev)?;
                }
            }
            d.media_mut().remove(media);
            r.removed.push(AnyId::Media(*media));
        }
        MediaCmd::Rename { media, name } => {
            item(d, *media)?;
            if let Some(m) = d.media_mut().get_mut(media) {
                m.name = name.clone();
            }
        }
        MediaCmd::Relink {
            media,
            path,
            content_hash,
            length_frames,
            channels,
            sample_rate,
        } => {
            item(d, *media)?;
            if *sample_rate == 0 || *channels == 0 {
                return Err(ModelError::range("media.format", sample_rate));
            }
            check_media_path(path)?;
            for id in regions_using(d, *media) {
                if let EventBody::AudioRegion {
                    source_offset,
                    length,
                    ..
                } = d.event(id)?.0.body
                {
                    let end = source_offset.saturating_add(length);
                    if end > *length_frames {
                        return Err(ModelError::range("relink.length_frames", length_frames));
                    }
                }
            }
            if let Some(m) = d.media_mut().get_mut(media) {
                m.path = path.clone();
                m.content_hash = content_hash.clone();
                m.length_frames = *length_frames;
                m.channels = *channels;
                m.sample_rate = *sample_rate;
            }
        }
        MediaCmd::Compact => {
            let used = used_media(d);
            let unused: Vec<MediaId> = d
                .media()
                .keys()
                .filter(|m| !used.contains(m))
                .copied()
                .collect();
            for m in unused {
                d.media_mut().remove(&m);
                r.removed.push(AnyId::Media(m));
            }
        }
        MediaCmd::Convert { swaps } => {
            if swaps.is_empty() {
                return Err(ModelError::invalid("nothing to convert"));
            }
            let rate = d.settings().format.sample_rate;
            swap_media(d, swaps, rate)?;
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
pub mod mixer;
```
Replace:
```rust
pub mod lane;
pub mod media;
pub mod mixer;
```

Find:
```rust
pub use lane::LaneCmd;
```
Replace:
```rust
pub use lane::LaneCmd;
pub use media::{MediaCmd, MediaSwap};
```

Find:
```rust
    /// Destination, output and media edits.
    Routing(RoutingCmd),
```
Replace:
```rust
    /// Destination and output edits.
    Routing(RoutingCmd),
    /// Media-pool edits.
    Media(MediaCmd),
```

Find:
```rust
            Command::Routing(c) => c.name(),
```
Replace:
```rust
            Command::Routing(c) => c.name(),
            Command::Media(c) => c.name(),
```

Find:
```rust
        Command::Routing(c) => {
            routing::apply(&mut d, c, &mut report)?;
            None
        }
```
Replace:
```rust
        Command::Routing(c) => {
            routing::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Media(c) => {
            media::apply(&mut d, c, &mut report)?;
            None
        }
```

In `crates/core-model/src/command/routing.rs`, remove the media variants and their arms, and share `require_replacement` and output creation.

Find:
```rust
//! Instrument-list, output-path and media-pool commands. Removing an item
//! in use is refused unless a replacement is given.
```
Replace:
```rust
//! Instrument-list and output-path commands (media-pool commands are in
//! `media`). Removing an item in use is refused unless a replacement is
//! given.
```

Find:
```rust
/// Destination, output and media edits.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingCmd {
```
Replace:
```rust
/// Destination and output edits.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingCmd {
```

Find:
```rust
    /// Add a media item.
    AddMedia {
        /// Package-relative path.
        path: String,
        /// Sample rate (> 0).
        sample_rate: u32,
        /// Channels (> 0).
        channels: u16,
        /// Length in frames.
        length_frames: u64,
        /// Optional content hash.
        content_hash: Option<String>,
    },
    /// Remove a media item, re-pointing regions to `replacement`.
    RemoveMedia {
        /// Target.
        media: MediaId,
        /// Replacement (required when in use; must be long enough).
        replacement: Option<MediaId>,
    },
}
```
Replace:
```rust
}
```

Find:
```rust
            RoutingCmd::AddMedia { .. } => "Add Media",
            RoutingCmd::RemoveMedia { .. } => "Remove Media",
```
Replace with nothing (delete the two lines).

Find:
```rust
fn require_replacement<T: Copy + PartialEq>(
```
Replace:
```rust
pub(crate) fn require_replacement<T: Copy + PartialEq>(
```

Find:
```rust
        RoutingCmd::AddOutput { name, reference } => {
            let id = d.counters_mut().alloc_output();
            d.outputs_mut().insert(
                id,
                OutputPath {
                    id,
                    name: name.clone(),
                    reference: reference.clone(),
                },
            );
            r.created.push(AnyId::Output(id));
            strips::add_audio_path_strip(d, id, name, r);
        }
```
Replace:
```rust
        RoutingCmd::AddOutput { name, reference } => {
            add_output(d, name, reference, r);
        }
```

Delete the whole `RoutingCmd::AddMedia { … } => { … }` arm and the whole `RoutingCmd::RemoveMedia { media, replacement } => { … }` arm at the end of `apply` (the code now in `media::apply`). Then add, after `require_replacement`:

```rust
/// Adds output path `name` (with its `AudioPath` strip) and returns its ID.
pub(crate) fn add_output(
    d: &mut Document,
    name: &str,
    reference: &str,
    r: &mut Report,
) -> OutputPathId {
    let id = d.counters_mut().alloc_output();
    d.outputs_mut().insert(
        id,
        OutputPath {
            id,
            name: name.to_string(),
            reference: reference.to_string(),
        },
    );
    r.created.push(AnyId::Output(id));
    strips::add_audio_path_strip(d, id, name, r);
    id
}
```

Remove imports the compiler now reports unused (`MediaItem`, `MediaOrigin`, `MediaId`, `check_media_path`, `EventBody` if no longer used).

In `crates/core-model/src/command/settings.rs`:

Find:
```rust
use super::Report;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::modify::ModifyTemplate;
```
Replace:
```rust
use super::Report;
use super::media::{MediaSwap, swap_media};
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::format::AudioFormat;
use crate::modify::ModifyTemplate;
use std::collections::BTreeSet;
```

Find:
```rust
    /// Autosave copies kept (>= 1).
    SetAutosaveKeep {
        /// Copies kept.
        keep: u32,
    },
}
```
Replace:
```rust
    /// Autosave copies kept (>= 1).
    SetAutosaveKeep {
        /// Copies kept.
        keep: u32,
    },
    /// Change the project's audio format (sub-project 5 spec §4.1): every
    /// media item gets a new file in the new format and its regions are
    /// rescaled. A project with media must list every item once; the
    /// same format is refused (that is `MediaCmd::Convert`).
    SetFormat {
        /// The new format (an allowed rate).
        format: AudioFormat,
        /// One new file per media item.
        swaps: Vec<MediaSwap>,
    },
}
```

Find:
```rust
            SettingsCmd::SetAutosaveKeep { .. } => "Set Autosave Count",
        }
    }
}

pub(crate) fn apply(d: &mut Document, cmd: &SettingsCmd, _r: &mut Report) -> Result<()> {
    let s = d.settings_mut();
```
Replace:
```rust
            SettingsCmd::SetAutosaveKeep { .. } => "Set Autosave Count",
            SettingsCmd::SetFormat { .. } => "Change Project Format",
        }
    }
}

fn set_format(d: &mut Document, format: AudioFormat, swaps: &[MediaSwap]) -> Result<()> {
    if !format.is_valid() {
        return Err(ModelError::range("format.sample_rate", format.sample_rate));
    }
    if format == d.settings().format {
        return Err(ModelError::invalid("the project is already in that format"));
    }
    let items: BTreeSet<_> = d.media().keys().copied().collect();
    let listed: BTreeSet<_> = swaps.iter().map(|s| s.media).collect();
    if listed != items || swaps.len() != items.len() {
        return Err(ModelError::invalid(format!(
            "a format change converts every media item once ({} given for {} items)",
            swaps.len(),
            items.len()
        )));
    }
    swap_media(d, swaps, format.sample_rate)?;
    d.settings_mut().format = format;
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &SettingsCmd, _r: &mut Report) -> Result<()> {
    if let SettingsCmd::SetFormat { format, swaps } = cmd {
        return set_format(d, *format, swaps);
    }
    let s = d.settings_mut();
```

Find:
```rust
            s.autosave_keep = *keep;
        }
    }
    Ok(())
}
```
Replace:
```rust
            s.autosave_keep = *keep;
        }
        // Handled above.
        SettingsCmd::SetFormat { .. } => {}
    }
    Ok(())
}
```

In `crates/core-model/src/command/audio.rs`, the insert gives the lane an output path (Ruling 6):

Find:
```rust
        } => {
            let spec = EventSpec::new(
                *tick,
                EventBody::AudioRegion {
```
Replace:
```rust
        } => {
            // A region plays on its lane's output path: a lane without one
            // takes the first, or a new "Audio 1" (5a Ruling 6).
            if d.lane(*seq, *lane)?.audio_out.is_none() {
                let out = match d.outputs().keys().next().copied() {
                    Some(o) => o,
                    None => super::routing::add_output(d, "Audio 1", "audio:1-2", r),
                };
                d.lane_mut(*seq, *lane)?.audio_out = Some(out);
            }
            let spec = EventSpec::new(
                *tick,
                EventBody::AudioRegion {
```

Now the callers that added media through `RoutingCmd`. In `crates/core-engine/src/fixtures.rs`:

Find:
```rust
    let r = b.run(Command::Routing(RoutingCmd::AddMedia {
        path: "Media/loop.wav".into(),
        sample_rate: 48_000,
        channels: 2,
        length_frames: 384_000,
        content_hash: None,
    }))?;
```
Replace:
```rust
    let r = b.run(Command::Media(MediaCmd::Add {
        path: "Media/loop.wav".into(),
        sample_rate: 48_000,
        channels: 2,
        length_frames: 384_000,
        content_hash: None,
        name: None,
        origin: None,
    }))?;
```
Add `MediaCmd` to that file's `core_model::command` import.

The test files `crates/core-model/tests/query_flatten.rs`, `crates/core-ffi/tests/edit.rs`, `crates/core-ffi/tests/events.rs` and `crates/core-ffi/tests/revs.rs` use `Command::Routing(RoutingCmd::AddMedia { …, content_hash: None, })`. Rewrite them mechanically:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
for f in crates/core-model/tests/query_flatten.rs crates/core-ffi/tests/edit.rs \
         crates/core-ffi/tests/events.rs crates/core-ffi/tests/revs.rs \
         crates/core-model/tests/cmd_audio_routing.rs; do
  perl -0pi -e 's/Command::Routing\(RoutingCmd::AddMedia \{/Command::Media(MediaCmd::Add {/g;
                s/routing\(RoutingCmd::AddMedia \{/media(MediaCmd::Add {/g;
                s/routing\(RoutingCmd::RemoveMedia \{/media(MediaCmd::Remove {/g;
                s/(MediaCmd::Add \{\n(?:[^\n]*\n)*?([ ]*)content_hash: ([^\n]*),\n)/$1$2name: None,\n$2origin: None,\n/g' "$f"
done
grep -n "MediaCmd::Add {" -A9 crates/core-ffi/tests/revs.rs | head -24
```
Expected: each `MediaCmd::Add {` block now ends with `name: None,` and `origin: None,` before its closing brace.

Then fix the imports in those files:
- add `MediaCmd` to `use core_model::command::{…}`, or `use core_model::command::MediaCmd;` where the file imports `core_model::command::routing::RoutingCmd`;
- drop `RoutingCmd` where it is no longer used, which the compiler reports.

In `crates/core-model/tests/cmd_audio_routing.rs` add the helper beside `routing`:

```rust
fn media(c: MediaCmd) -> Command {
    Command::Media(c)
}
```

Its `base()` mixed media into a `RoutingCmd` array.

Find:
```rust
        RoutingCmd::AddOutput {
            name: "Aux".into(),
            reference: "bus:1".into(),
        },
        RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
        },
        RoutingCmd::AddMedia {
            path: "Media/b.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
        },
    ] {
        d = run(&d, routing(c)).unwrap();
    }
    d
}
```
Replace:
```rust
        RoutingCmd::AddOutput {
            name: "Aux".into(),
            reference: "bus:1".into(),
        },
    ] {
        d = run(&d, routing(c)).unwrap();
    }
    for path in ["Media/a.wav", "Media/b.wav"] {
        d = run(
            &d,
            media(MediaCmd::Add {
                path: path.into(),
                sample_rate: 48_000,
                channels: 2,
                length_frames: 96_000,
                content_hash: None,
                name: None,
                origin: None,
            }),
        )
        .unwrap();
    }
    d
}
```

(Run the perl loop above after this edit too if you edit in that order; it is idempotent for blocks that already have `name:`. If a block gets `name: None` twice, remove the duplicate.)

In `crates/core-model/tests/cmd_reference.rs`:

Find:
```rust
        RoutingCmd::AddMedia {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 48_000,
            content_hash: None,
        },
    ] {
        d = run(&d, Command::Routing(c)).unwrap();
    }
```
Replace:
```rust
    ] {
        d = run(&d, Command::Routing(c)).unwrap();
    }
    d = run(
        &d,
        Command::Media(MediaCmd::Add {
            path: "Media/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 48_000,
            content_hash: None,
            name: None,
            origin: None,
        }),
    )
    .unwrap();
```
Add `MediaCmd` to its `use core_model::command::{…}`.

Some existing tests now see output paths created by `AudioCmd::Insert`. A test that counted outputs, strips or created IDs after inserting a region into a lane without `audio_out` fails, because the report now also lists the output and strip it created. For each such failure:
- If the test means a lane that has an output, set `LaneCmd::SetOutput` first.
- Otherwise update the expectation to include the created output path and strip.

Say in the report which tests changed and why.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked|error\[" | sort | uniq -c`
Expected: every `test result: ok`; `cmd_media` runs 9 tests.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-model crates/core-engine/src/fixtures.rs crates/core-ffi/tests
git commit -m "model: media commands, project format change, output paths for new regions

MediaCmd (Add with name/origin, Remove, Rename, Relink, Compact, Convert)
replaces RoutingCmd's media variants; SettingsCmd::SetFormat swaps every
item's file and rescales its regions by the rate ratio; inserting a
region on a lane without an output path gives it the first one, or a new
Audio 1.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 3: `core-media` — CAF reader and writer in three depths

**Files:**
- Create: `crates/core-media/Cargo.toml`, `crates/core-media/src/lib.rs`, `crates/core-media/src/error.rs`, `crates/core-media/src/caf.rs`
- Modify: `Cargo.lock` (new crate, `blake3`)
- Test: unit tests in `crates/core-media/src/caf.rs`

**Interfaces:**
- Consumes: `core_model::format::BitDepth`.
- Produces:
  - `core_media::MediaError::{Io(String), NotCaf(String), Unsupported(String), Cancelled}` (with `From<std::io::Error>`);
  - `core_media::caf::{CafWriter, CafReader, CafInfo, Written, WriteMode, hash_file, MAX_CHANNELS}`:
    - `CafWriter::create(path, rate: u32, channels: u16, depth: BitDepth, mode: WriteMode) -> Result<CafWriter, MediaError>`;
    - `CafWriter::write(&mut self, interleaved: &[f32]) -> Result<(), MediaError>`;
    - `CafWriter::frames(&self) -> u64`;
    - `CafWriter::finish(self) -> Result<Written, MediaError>`;
    - `Written { frames: u64, content_hash: String, clipped: u64 }`;
    - `CafReader::open(path) -> Result<CafReader, MediaError>`, `CafReader::info(&self) -> &CafInfo`;
    - `CafReader::read(&self, start: u64, out: &mut [f32], scratch: &mut Vec<u8>) -> Result<usize, MediaError>` (frames read, interleaved float);
    - `CafInfo { sample_rate: u32, channels: u16, depth: BitDepth, frames: u64, data_offset: u64, size_known: bool, recording: bool }`;
    - `hash_file(path) -> Result<String, MediaError>`.

Spec §5.2.
- **The writer** writes `caff`, `desc` (linear PCM, little-endian, interleaved, packed), `chan` (mono or stereo tag), an `info` chunk with `vizdaw-recording` in recording mode, and `data`.
  - The data size starts as −1 ("to end of file") and is patched on `finish`, so a crash leaves a valid file.
  - Integer depths clip at full scale and count the clipped samples. 16-bit adds TPDF dither (±1 LSB, a deterministic xorshift so tests repeat); 24-bit rounds.
  - `create` never overwrites (`create_new`).
- **The reader** accepts only what the writer writes. Reads are positioned (`FileExt::read_at`), never memory-mapped.
  - A declared size longer than the file is cut to the file, so a truncated file reports fewer frames and reads short.
- **The hash** is BLAKE3 of the sample bytes alone: the writer hashes as it writes, and `hash_file` re-reads the same bytes.

- [ ] **Step 1: Create the crate with failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-media/src
cat > crates/core-media/Cargo.toml <<'VIZDAW_EOF'
[package]
name = "core-media"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
blake3 = { version = "1", default-features = false, features = ["std"] }
core-model = { path = "../core-model" }
thiserror = "2"

[dev-dependencies]
proptest = "1"
tempfile = "3"
VIZDAW_EOF
cat > crates/core-media/src/lib.rs <<'VIZDAW_EOF'
//! `core-media`: VizDAW's audio files (sub-project 5 spec §5). The CAF
//! reader and writer every media file goes through, the import pipeline
//! over a platform `MediaImporter`, and the disk streamer the real-time
//! thread reads regions from. Platform-neutral: no OS audio APIs here.
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod caf;
mod error;

pub use error::MediaError;
VIZDAW_EOF
cat > crates/core-media/src/error.rs <<'VIZDAW_EOF'
//! Media errors.

/// What went wrong reading, writing or importing audio.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    /// The file system refused.
    #[error("{0}")]
    Io(String),
    /// Not a CAF file (or a damaged one).
    #[error("not a VizDAW CAF file: {0}")]
    NotCaf(String),
    /// A format VizDAW does not read or write here.
    #[error("unsupported audio: {0}")]
    Unsupported(String),
    /// The operation was cancelled.
    #[error("cancelled")]
    Cancelled,
}

impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> MediaError {
        MediaError::Io(e.to_string())
    }
}
VIZDAW_EOF
```

Write `crates/core-media/src/caf.rs` with its tests first, as below. Run the tests once with every function body `todo!()`, so you see them fail, then fill the bodies (Step 3). The finished file:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-media/src/caf.rs <<'VIZDAW_EOF'
//! CAF files (sub-project 5 spec §5.2): little-endian linear PCM, 1–2
//! channels, int16, int24 or float32, as VizDAW writes them. The writer
//! starts the `data` chunk with size −1 ("to end of file") and patches the
//! size on `finish`, so a crash leaves a valid file. The reader accepts
//! only that, reads with positioned reads (no memory map: a file truncated
//! or replaced underneath yields short reads or errors, never SIGBUS), and
//! converts to float.

use crate::MediaError;
use core_model::format::BitDepth;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::FileExt;
use std::path::Path;

/// The most channels a VizDAW CAF holds (wider files are split on import).
pub const MAX_CHANNELS: u16 = 2;

const FLAG_FLOAT: u32 = 1;
const FLAG_LITTLE_ENDIAN: u32 = 2;
const TAG_MONO: u32 = (100 << 16) | 1;
const TAG_STEREO: u32 = (101 << 16) | 2;
const RECORDING_KEY: &[u8] = b"vizdaw-recording";
const INT16_SCALE: f32 = 32_768.0;
const INT24_SCALE: f32 = 8_388_608.0;

/// How a file is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// An ordinary media file.
    Normal,
    /// A recording: an `info` chunk notes `vizdaw-recording` (crash
    /// recovery finds such files whose size was never patched).
    Recording,
}

/// What a CAF file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CafInfo {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u16,
    /// Sample format.
    pub depth: BitDepth,
    /// Whole frames present (the declared size, cut to the file).
    pub frames: u64,
    /// File offset of the first sample byte.
    pub data_offset: u64,
    /// The `data` size was patched (false: −1, e.g. after a crash).
    pub size_known: bool,
    /// The file carries the recording mark.
    pub recording: bool,
}

impl CafInfo {
    /// Bytes per frame.
    pub fn frame_bytes(&self) -> usize {
        self.depth.bytes() * usize::from(self.channels)
    }
}

/// What a finished writer wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// Frames written.
    pub frames: u64,
    /// BLAKE3 hex of the sample bytes.
    pub content_hash: String,
    /// Samples clipped at full scale (integer depths).
    pub clipped: u64,
}

/// Triangular (TPDF) dither of ±1 LSB from a deterministic xorshift64*.
struct Tpdf(u64);

impl Tpdf {
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        // The top 24 bits as a fraction in [0, 1).
        (x >> 40) as f32 / 16_777_216.0
    }

    fn next(&mut self) -> f32 {
        self.unit() + self.unit() - 1.0
    }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], size: i64) {
    out.extend_from_slice(kind);
    out.extend_from_slice(&size.to_be_bytes());
}

/// Writes a VizDAW CAF file.
pub struct CafWriter {
    file: File,
    channels: u16,
    depth: BitDepth,
    size_at: u64,
    frames: u64,
    clipped: u64,
    hasher: blake3::Hasher,
    dither: Tpdf,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for CafWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CafWriter")
            .field("channels", &self.channels)
            .field("depth", &self.depth)
            .field("frames", &self.frames)
            .finish_non_exhaustive()
    }
}

impl CafWriter {
    /// Creates `path` (never overwriting) for `channels` (1–2) at `rate`
    /// in `depth`.
    pub fn create(
        path: &Path,
        rate: u32,
        channels: u16,
        depth: BitDepth,
        mode: WriteMode,
    ) -> Result<CafWriter, MediaError> {
        if !(1..=MAX_CHANNELS).contains(&channels) {
            return Err(MediaError::Unsupported(format!(
                "{channels} channels (VizDAW CAF files hold 1 or 2)"
            )));
        }
        if rate == 0 {
            return Err(MediaError::Unsupported("a sample rate of 0".into()));
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut h: Vec<u8> = Vec::with_capacity(128);
        h.extend_from_slice(b"caff");
        h.extend_from_slice(&1u16.to_be_bytes());
        h.extend_from_slice(&0u16.to_be_bytes());
        chunk(&mut h, b"desc", 32);
        h.extend_from_slice(&f64::from(rate).to_be_bytes());
        h.extend_from_slice(b"lpcm");
        let float = if depth == BitDepth::Float32 {
            FLAG_FLOAT
        } else {
            0
        };
        h.extend_from_slice(&(FLAG_LITTLE_ENDIAN | float).to_be_bytes());
        let frame_bytes = depth.bytes() as u32 * u32::from(channels);
        h.extend_from_slice(&frame_bytes.to_be_bytes());
        h.extend_from_slice(&1u32.to_be_bytes());
        h.extend_from_slice(&u32::from(channels).to_be_bytes());
        h.extend_from_slice(&depth.bits().to_be_bytes());
        chunk(&mut h, b"chan", 12);
        let tag = if channels == 1 { TAG_MONO } else { TAG_STEREO };
        h.extend_from_slice(&tag.to_be_bytes());
        h.extend_from_slice(&0u32.to_be_bytes());
        h.extend_from_slice(&0u32.to_be_bytes());
        if mode == WriteMode::Recording {
            let mut body = 1u32.to_be_bytes().to_vec();
            body.extend_from_slice(RECORDING_KEY);
            body.push(0);
            body.extend_from_slice(b"1\0");
            chunk(&mut h, b"info", body.len() as i64);
            h.extend_from_slice(&body);
        }
        h.extend_from_slice(b"data");
        let size_at = h.len() as u64;
        h.extend_from_slice(&(-1i64).to_be_bytes());
        // Edit count.
        h.extend_from_slice(&0u32.to_be_bytes());
        file.write_all(&h)?;
        Ok(CafWriter {
            file,
            channels,
            depth,
            size_at,
            frames: 0,
            clipped: 0,
            hasher: blake3::Hasher::new(),
            dither: Tpdf(0x9E37_79B9_7F4A_7C15),
            bytes: Vec::new(),
        })
    }

    /// Appends interleaved float frames (a length that is a whole number
    /// of frames).
    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), MediaError> {
        let ch = usize::from(self.channels);
        if interleaved.len() % ch != 0 {
            return Err(MediaError::Unsupported(format!(
                "{} samples are not whole {ch}-channel frames",
                interleaved.len()
            )));
        }
        self.bytes.clear();
        self.bytes.reserve(interleaved.len() * self.depth.bytes());
        for &x in interleaved {
            let x = if x.is_finite() { x } else { 0.0 };
            match self.depth {
                BitDepth::Float32 => self.bytes.extend_from_slice(&x.to_le_bytes()),
                BitDepth::Int16 => {
                    let v = x * INT16_SCALE;
                    if !(-INT16_SCALE..=INT16_SCALE - 1.0).contains(&v) {
                        self.clipped += 1;
                    }
                    let s = (v + self.dither.next())
                        .round()
                        .clamp(-INT16_SCALE, INT16_SCALE - 1.0) as i16;
                    self.bytes.extend_from_slice(&s.to_le_bytes());
                }
                BitDepth::Int24 => {
                    let v = (x * INT24_SCALE).round();
                    if !(-INT24_SCALE..=INT24_SCALE - 1.0).contains(&v) {
                        self.clipped += 1;
                    }
                    let s = v.clamp(-INT24_SCALE, INT24_SCALE - 1.0) as i32;
                    let b = s.to_le_bytes();
                    self.bytes.extend_from_slice(b.get(..3).unwrap_or(&[0, 0, 0]));
                }
            }
        }
        self.hasher.update(&self.bytes);
        self.file.write_all(&self.bytes)?;
        self.frames += (interleaved.len() / ch) as u64;
        Ok(())
    }

    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Patches the data size and syncs the file.
    pub fn finish(mut self) -> Result<Written, MediaError> {
        let bytes = self.frames * (self.depth.bytes() as u64) * u64::from(self.channels);
        let size = i64::try_from(bytes.saturating_add(4)).unwrap_or(-1);
        self.file.seek(SeekFrom::Start(self.size_at))?;
        self.file.write_all(&size.to_be_bytes())?;
        self.file.sync_all()?;
        Ok(Written {
            frames: self.frames,
            content_hash: self.hasher.finalize().to_hex().to_string(),
            clipped: self.clipped,
        })
    }
}

/// Reads a VizDAW CAF file.
#[derive(Debug)]
pub struct CafReader {
    file: File,
    info: CafInfo,
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Reads exactly `buf.len()` bytes at `at`, or fewer at the end of the
/// file; returns how many.
fn read_full(file: &File, buf: &mut [u8], at: u64) -> Result<usize, MediaError> {
    let mut got = 0usize;
    while got < buf.len() {
        let rest = buf.get_mut(got..).unwrap_or(&mut []);
        match file.read_at(rest, at + got as u64) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(got)
}

impl CafReader {
    /// Opens and checks `path`.
    pub fn open(path: &Path) -> Result<CafReader, MediaError> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let not = |why: &str| MediaError::NotCaf(format!("{}: {why}", path.display()));
        let mut head = [0u8; 8];
        if read_full(&file, &mut head, 0)? < 8 || head.get(..4) != Some(b"caff".as_slice()) {
            return Err(not("no CAF header"));
        }
        let mut pos = 8u64;
        let mut desc: Option<[u8; 32]> = None;
        let mut data: Option<(u64, i64)> = None;
        let mut recording = false;
        while pos + 12 <= len {
            let mut ch = [0u8; 12];
            read_full(&file, &mut ch, pos)?;
            let kind: [u8; 4] = ch.get(..4).and_then(|k| k.try_into().ok()).unwrap_or([0; 4]);
            let size = i64::from_be_bytes(ch.get(4..12).and_then(|s| s.try_into().ok()).unwrap_or([0; 8]));
            let body = pos + 12;
            match &kind {
                b"desc" => {
                    let mut d = [0u8; 32];
                    if size != 32 || read_full(&file, &mut d, body)? < 32 {
                        return Err(not("damaged desc chunk"));
                    }
                    desc = Some(d);
                }
                b"info" if size > 0 && size < 4096 => {
                    let mut b = vec![0u8; size as usize];
                    read_full(&file, &mut b, body)?;
                    recording = b.windows(RECORDING_KEY.len()).any(|w| w == RECORDING_KEY);
                }
                b"data" => {
                    data = Some((body + 4, size));
                    break;
                }
                _ => {}
            }
            if size < 0 {
                return Err(not("a chunk of unknown size before the data"));
            }
            pos = body.saturating_add(size as u64);
        }
        let d = desc.ok_or_else(|| not("no desc chunk"))?;
        let (data_offset, size) = data.ok_or_else(|| not("no data chunk"))?;
        let rate = f64::from_be_bytes(d.get(..8).and_then(|x| x.try_into().ok()).unwrap_or([0; 8]));
        let field = |at| be_u32(&d, at).unwrap_or(0);
        let (format, flags, packet, per_packet, channels, bits) =
            (d.get(8..12).unwrap_or(&[]), field(12), field(16), field(20), field(24), field(28));
        let unsupported = |what: String| MediaError::Unsupported(format!("{}: {what}", path.display()));
        if format != b"lpcm" {
            return Err(unsupported("not linear PCM".into()));
        }
        if flags & FLAG_LITTLE_ENDIAN == 0 {
            return Err(unsupported("big-endian samples".into()));
        }
        let depth = match (bits, flags & FLAG_FLOAT != 0) {
            (16, false) => BitDepth::Int16,
            (24, false) => BitDepth::Int24,
            (32, true) => BitDepth::Float32,
            (b, f) => return Err(unsupported(format!("{b}-bit{}", if f { " float" } else { "" }))),
        };
        if !(1..=u32::from(MAX_CHANNELS)).contains(&channels) {
            return Err(unsupported(format!("{channels} channels")));
        }
        let frame_bytes = depth.bytes() as u64 * u64::from(channels);
        if per_packet != 1 || u64::from(packet) != frame_bytes {
            return Err(unsupported("not packed, one frame per packet".into()));
        }
        if !(rate.is_finite() && rate >= 1.0 && rate <= 768_000.0 && rate.fract() == 0.0) {
            return Err(unsupported(format!("a sample rate of {rate}")));
        }
        let present = len.saturating_sub(data_offset) / frame_bytes;
        let frames = if size >= 4 {
            ((size as u64 - 4) / frame_bytes).min(present)
        } else {
            present
        };
        Ok(CafReader {
            file,
            info: CafInfo {
                sample_rate: rate as u32,
                channels: channels as u16,
                depth,
                frames,
                data_offset,
                size_known: size >= 0,
                recording,
            },
        })
    }

    /// What the file holds.
    pub fn info(&self) -> &CafInfo {
        &self.info
    }

    /// Reads frames from `start` into `out` (interleaved float, as many
    /// whole frames as fit); returns the frames read, fewer at the end or
    /// when the file is shorter than it was. `scratch` is reused.
    pub fn read(
        &self,
        start: u64,
        out: &mut [f32],
        scratch: &mut Vec<u8>,
    ) -> Result<usize, MediaError> {
        let i = &self.info;
        let ch = usize::from(i.channels);
        let want = (out.len() / ch) as u64;
        let n = want.min(i.frames.saturating_sub(start)) as usize;
        if n == 0 {
            return Ok(0);
        }
        let fb = i.frame_bytes();
        scratch.resize(n * fb, 0);
        let at = i.data_offset + start * fb as u64;
        let got = read_full(&self.file, scratch, at)? / fb;
        let sb = i.depth.bytes();
        for (k, o) in out.iter_mut().take(got * ch).enumerate() {
            let b = scratch.get(k * sb..k * sb + sb).unwrap_or(&[]);
            *o = match (i.depth, b) {
                (BitDepth::Float32, [a, b, c, d]) => f32::from_le_bytes([*a, *b, *c, *d]),
                (BitDepth::Int16, [a, b]) => f32::from(i16::from_le_bytes([*a, *b])) / INT16_SCALE,
                (BitDepth::Int24, [a, b, c]) => {
                    // Sign-extend via the top byte.
                    (i32::from_le_bytes([0, *a, *b, *c]) >> 8) as f32 / INT24_SCALE
                }
                _ => 0.0,
            };
        }
        Ok(got)
    }
}

/// BLAKE3 hex of `path`'s sample bytes (the hash the writer reported).
pub fn hash_file(path: &Path) -> Result<String, MediaError> {
    let r = CafReader::open(path)?;
    let total = r.info.frames * r.info.frame_bytes() as u64;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    while done < total {
        let n = (total - done).min(buf.len() as u64) as usize;
        let chunk = buf.get_mut(..n).unwrap_or(&mut []);
        let got = read_full(&r.file, chunk, r.info.data_offset + done)?;
        h.update(chunk.get(..got).unwrap_or(&[]));
        if got < n {
            break;
        }
        done += n as u64;
    }
    Ok(h.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, channels: usize) -> Vec<f32> {
        (0..frames * channels)
            .map(|k| {
                let f = (k / channels) as f32;
                0.8 * (f * 0.01 + (k % channels) as f32).sin()
            })
            .collect()
    }

    fn write(path: &Path, depth: BitDepth, data: &[f32], ch: u16) -> Written {
        let mut w = CafWriter::create(path, 48_000, ch, depth, WriteMode::Normal).unwrap();
        w.write(data).unwrap();
        w.finish().unwrap()
    }

    fn read_all(path: &Path) -> (CafInfo, Vec<f32>) {
        let r = CafReader::open(path).unwrap();
        let info = r.info().clone();
        let mut out = vec![0.0; info.frames as usize * usize::from(info.channels)];
        let n = r.read(0, &mut out, &mut Vec::new()).unwrap();
        assert_eq!(n as u64, info.frames);
        (info, out)
    }

    #[test]
    fn round_trip_in_each_depth_mono_and_stereo() {
        let dir = tempfile::tempdir().unwrap();
        for (depth, tol) in [
            (BitDepth::Float32, 0.0),
            (BitDepth::Int24, 1.0 / INT24_SCALE),
            // Rounding plus up to ±1 LSB of dither.
            (BitDepth::Int16, 2.0 / INT16_SCALE),
        ] {
            for ch in [1u16, 2] {
                let p = dir.path().join(format!("{depth:?}-{ch}.caf"));
                let data = sine(1000, usize::from(ch));
                let w = write(&p, depth, &data, ch);
                assert_eq!((w.frames, w.clipped), (1000, 0));
                let (info, back) = read_all(&p);
                assert_eq!(
                    (info.sample_rate, info.channels, info.depth, info.size_known),
                    (48_000, ch, depth, true)
                );
                for (a, b) in data.iter().zip(&back) {
                    assert!((a - b).abs() <= tol, "{depth:?} {a} {b}");
                }
            }
        }
    }

    #[test]
    fn dither_only_at_16_bit() {
        let dir = tempfile::tempdir().unwrap();
        let silence = vec![0.0f32; 4096];
        for (depth, dithered) in [(BitDepth::Int16, true), (BitDepth::Int24, false)] {
            let p = dir.path().join(format!("{depth:?}.caf"));
            write(&p, depth, &silence, 1);
            let (_, back) = read_all(&p);
            assert_eq!(back.iter().any(|x| *x != 0.0), dithered, "{depth:?}");
            assert!(back.iter().all(|x| x.abs() <= 1.0 / INT16_SCALE));
        }
    }

    #[test]
    fn integer_depths_clip_and_count() {
        let dir = tempfile::tempdir().unwrap();
        let loud = [1.5f32, -2.0, 0.5, f32::NAN];
        let w = write(&dir.path().join("i24.caf"), BitDepth::Int24, &loud, 1);
        assert_eq!(w.clipped, 2);
        let (_, back) = read_all(&dir.path().join("i24.caf"));
        assert_eq!(back, vec![(INT24_SCALE - 1.0) / INT24_SCALE, -1.0, 0.5, 0.0]);
        let w = write(&dir.path().join("f.caf"), BitDepth::Float32, &loud, 1);
        assert_eq!(w.clipped, 0);
        let (_, back) = read_all(&dir.path().join("f.caf"));
        assert_eq!(back, vec![1.5, -2.0, 0.5, 0.0]);
    }

    #[test]
    fn an_unfinished_file_reads_to_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("crash.caf");
        let mut w = CafWriter::create(&p, 44_100, 2, BitDepth::Int24, WriteMode::Recording).unwrap();
        w.write(&sine(300, 2)).unwrap();
        // A crash: the writer goes without `finish`.
        drop(w);
        let r = CafReader::open(&p).unwrap();
        assert_eq!(
            (r.info().frames, r.info().size_known, r.info().recording, r.info().sample_rate),
            (300, false, true, 44_100)
        );
    }

    #[test]
    fn truncated_file_reads_what_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.caf");
        write(&p, BitDepth::Int16, &sine(1000, 2), 2);
        let off = CafReader::open(&p).unwrap().info().data_offset;
        let f = OpenOptions::new().write(true).open(&p).unwrap();
        // 500 whole frames and half of the next.
        f.set_len(off + 500 * 4 + 2).unwrap();
        let r = CafReader::open(&p).unwrap();
        assert_eq!(r.info().frames, 500);
        let mut out = vec![0.0; 2000];
        assert_eq!(r.read(0, &mut out, &mut Vec::new()).unwrap(), 500);
        assert_eq!(r.read(499, &mut out, &mut Vec::new()).unwrap(), 1);
        assert_eq!(r.read(500, &mut out, &mut Vec::new()).unwrap(), 0);
        // Truncated under an open reader: short reads, no error.
        let r = CafReader::open(&dir.path().join("t.caf")).unwrap();
        f.set_len(off + 100 * 4).unwrap();
        assert_eq!(r.read(0, &mut out, &mut Vec::new()).unwrap(), 100);
    }

    #[test]
    fn other_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.wav");
        std::fs::write(&p, b"RIFF\0\0\0\0WAVEfmt this is not caf").unwrap();
        assert!(matches!(CafReader::open(&p), Err(MediaError::NotCaf(_))));
        std::fs::write(&p, b"").unwrap();
        assert!(matches!(CafReader::open(&p), Err(MediaError::NotCaf(_))));
        // A big-endian CAF: flip the little-endian flag of a good file.
        let good = dir.path().join("g.caf");
        write(&good, BitDepth::Int16, &sine(10, 1), 1);
        let mut bytes = std::fs::read(&good).unwrap();
        // desc body starts at 8 + 12; flags at +12, big-endian u32.
        bytes[20 + 12 + 3] &= !(FLAG_LITTLE_ENDIAN as u8);
        std::fs::write(&p, &bytes).unwrap();
        assert!(matches!(CafReader::open(&p), Err(MediaError::Unsupported(_))));
        assert!(matches!(
            CafWriter::create(&dir.path().join("3.caf"), 48_000, 3, BitDepth::Int16, WriteMode::Normal),
            Err(MediaError::Unsupported(_))
        ));
    }

    #[test]
    fn create_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.caf");
        write(&p, BitDepth::Int16, &sine(10, 1), 1);
        assert!(matches!(
            CafWriter::create(&p, 48_000, 1, BitDepth::Int16, WriteMode::Normal),
            Err(MediaError::Io(_))
        ));
    }

    #[test]
    fn the_hash_is_of_the_samples() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.caf");
        let b = dir.path().join("b.caf");
        let w = write(&a, BitDepth::Float32, &sine(100, 2), 2);
        assert_eq!(hash_file(&a).unwrap(), w.content_hash);
        assert_eq!(w.content_hash.len(), 64);
        let mut other = sine(100, 2);
        other[7] = 0.123;
        let w2 = write(&b, BitDepth::Float32, &other, 2);
        assert_ne!(w.content_hash, w2.content_hash);
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

With the bodies stubbed: `cargo test -p core-media 2>&1 | tail -15`
Expected: the eight tests fail (panics at `todo!()`).

- [ ] **Step 3: Implement**

Restore the bodies from the listing above. Outside the `tests` module the crate denies `clippy::indexing_slicing`, so any indexing you add while adapting the code must go through `get`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-media 2>&1 | tail -5`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | sort | uniq -c
git add crates/core-media Cargo.lock
git commit -m "core-media: CAF reader and writer (int16, int24, float32)

Little-endian linear PCM, 1-2 channels; the data size starts at -1 and is
patched on finish, so a crash leaves a valid file; 16-bit gets TPDF
dither, integer depths count clipping; positioned reads only, so a
truncated or replaced file reads short instead of faulting.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 4: `core-media` — the import pipeline and the CAF importer

**Files:**
- Create: `crates/core-media/src/import.rs`
- Modify: `crates/core-media/src/lib.rs`
- Test: unit tests in `crates/core-media/src/import.rs`

**Interfaces:**
- Consumes: Task 3's `caf::{CafWriter, CafReader, WriteMode, Written}`; `core_model::format::AudioFormat`.
- Produces:
  - `core_media::import::{MediaImporter, SourceInfo, Imported, CafImporter, import_file, import_as, MEDIA_DIR, INCOMING_DIR}`, with:
    - `trait MediaImporter: Send + Sync { fn probe(&self, path: &Path) -> Result<SourceInfo, MediaError>; fn decode(&self, path: &Path, rate: u32, sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>, cancel: &AtomicBool) -> Result<u64, MediaError>; }`;
    - `SourceInfo { format_name: String, sample_rate: f64, bits: u32, channels: u16, frames: u64 }`;
    - `Imported { path: String, name: String, original_name: String, channels: u16, sample_rate: u32, length_frames: u64, content_hash: String, clipped: u64 }`;
    - `import_file(importer, source: &Path, package: &Path, format: AudioFormat, progress: &mut dyn FnMut(f32), cancel: &AtomicBool) -> Result<Vec<Imported>, MediaError>`;
    - `import_as(…, stem: &str, …)`, the same with the output name given (conversions).

Rulings 7 and 8; spec §5.1, "Channels" and "Where files go".
- **Decoding.** The importer delivers interleaved float at the project rate.
- **Writing.** This module writes the CAF files into `<package>/Media/.incoming/` under unique random names, then links each into `Media/` under its final name:
  - `<stem>.caf`, else `<stem> 2.caf`, `<stem> 3.caf`, …;
  - a `<stem> ch<k>` stem for each channel of a file wider than two channels.

  `link` fails if the name is taken, so two imports never clobber each other. The incoming file is then removed.
- **Failure.** Any error or cancellation removes every incoming file of that import, so an interrupted import leaves nothing in `Media/`.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-media/src/import.rs` with the tests at the bottom and every function body `todo!()`. The complete file, tests included:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-media/src/import.rs <<'VIZDAW_EOF'
//! Importing audio (sub-project 5 spec §5.1; plan 5a Rulings 7, 8): a
//! platform [`MediaImporter`] decodes a file to float at the project rate;
//! this module writes VizDAW CAF files in the project's depth into
//! `Media/.incoming/` and links them into `Media/` under free names. A
//! file wider than two channels becomes one mono file per channel. An
//! error or a cancel removes everything this import wrote.

use crate::MediaError;
use crate::caf::{CafReader, CafWriter, WriteMode, Written};
use core_model::format::AudioFormat;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The media folder inside a package.
pub const MEDIA_DIR: &str = "Media";
/// Where conversions are written before they get their names.
pub const INCOMING_DIR: &str = "Media/.incoming";
/// Frames per chunk the CAF importer hands on.
const CHUNK_FRAMES: usize = 8192;

/// What a source file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
    /// Format name ("WAVE", "AIFF", "FLAC", "CAF", …).
    pub format_name: String,
    /// Sample rate in Hz.
    pub sample_rate: f64,
    /// Bits per sample (0 for compressed formats).
    pub bits: u32,
    /// Channels.
    pub channels: u16,
    /// Frames at the source rate.
    pub frames: u64,
}

/// Decodes audio files (macOS: `platform_macos::media::ExtAudioImporter`).
pub trait MediaImporter: Send + Sync {
    /// What `path` holds.
    fn probe(&self, path: &Path) -> Result<SourceInfo, MediaError>;
    /// Decodes `path` to interleaved float at `rate` (all of its
    /// channels), handing chunks of whole frames to `sink`; stops with
    /// `Cancelled` once `cancel` is set. Returns the frames delivered.
    fn decode(
        &self,
        path: &Path,
        rate: u32,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
        cancel: &AtomicBool,
    ) -> Result<u64, MediaError>;
}

/// One media file an import made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    /// Package-relative path (`Media/<name>.caf`).
    pub path: String,
    /// Display name (the file stem).
    pub name: String,
    /// The source file's name.
    pub original_name: String,
    /// 1 or 2.
    pub channels: u16,
    /// The project rate it was written at.
    pub sample_rate: u32,
    /// Frames.
    pub length_frames: u64,
    /// BLAKE3 hex of the samples.
    pub content_hash: String,
    /// Samples clipped at full scale.
    pub clipped: u64,
}

/// Reads VizDAW CAF files at the project rate only (Ruling 8): tests,
/// depth-only conversions and platforms without a system importer.
#[derive(Debug, Clone, Copy, Default)]
pub struct CafImporter;

impl MediaImporter for CafImporter {
    fn probe(&self, path: &Path) -> Result<SourceInfo, MediaError> {
        let r = CafReader::open(path)?;
        let i = r.info();
        Ok(SourceInfo {
            format_name: "CAF".into(),
            sample_rate: f64::from(i.sample_rate),
            bits: i.depth.bits(),
            channels: i.channels,
            frames: i.frames,
        })
    }

    fn decode(
        &self,
        path: &Path,
        rate: u32,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
        cancel: &AtomicBool,
    ) -> Result<u64, MediaError> {
        let r = CafReader::open(path)?;
        let (src, ch) = (r.info().sample_rate, usize::from(r.info().channels));
        if src != rate {
            return Err(MediaError::Unsupported(format!(
                "{} Hz to {rate} Hz: resampling needs the system importer",
                src
            )));
        }
        let mut buf = vec![0.0f32; CHUNK_FRAMES * ch];
        let mut scratch = Vec::new();
        let mut at = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            let n = r.read(at, &mut buf, &mut scratch)?;
            if n == 0 {
                return Ok(at);
            }
            sink(buf.get(..n * ch).unwrap_or(&[]))?;
            at += n as u64;
        }
    }
}

/// A fresh name for an incoming file (no `uuid` crate: process, time and
/// a counter make it unique).
fn incoming_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{:x}-{:x}-{:x}.caf",
        std::process::id(),
        nanos,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Characters a file name may not hold: path separators and NUL become
/// "-"; leading dots are dropped (no hidden files).
fn clean_stem(stem: &str) -> String {
    let s: String = stem
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { '-' } else { c })
        .collect();
    let s = s.trim_start_matches('.').trim().to_string();
    if s.is_empty() { "Audio".into() } else { s }
}

/// Links `from` into `media` as `<stem>.caf`, `<stem> 2.caf`, … (the
/// first free name; `link` never replaces a file); returns the name.
fn link_free(from: &Path, media: &Path, stem: &str) -> Result<String, MediaError> {
    for n in 1..=10_000u32 {
        let name = if n == 1 {
            format!("{stem}.caf")
        } else {
            format!("{stem} {n}.caf")
        };
        match std::fs::hard_link(from, media.join(&name)) {
            Ok(()) => return Ok(name),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(MediaError::Io(format!("no free name for {stem}")))
}

/// Removes `paths`, ignoring errors (cleanup after a failure).
fn remove_all(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

/// [`import_as`] named after the source file's stem.
pub fn import_file(
    importer: &dyn MediaImporter,
    source: &Path,
    package: &Path,
    format: AudioFormat,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<Vec<Imported>, MediaError> {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    import_as(importer, source, package, format, &stem, progress, cancel)
}

/// Converts `source` into `package`'s `Media/` in `format`, naming the
/// result after `stem`. One file for mono or stereo; one mono file per
/// channel (`<stem> ch1`, …) for wider sources. `progress` gets 0–1.
pub fn import_as(
    importer: &dyn MediaImporter,
    source: &Path,
    package: &Path,
    format: AudioFormat,
    stem: &str,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<Vec<Imported>, MediaError> {
    let info = importer.probe(source)?;
    if info.channels == 0 {
        return Err(MediaError::Unsupported("a file with no channels".into()));
    }
    let incoming = package.join(INCOMING_DIR);
    std::fs::create_dir_all(&incoming)?;
    let media = package.join(MEDIA_DIR);
    let split = info.channels > 2;
    let files = if split { usize::from(info.channels) } else { 1 };
    let per_file = if split { 1 } else { info.channels };
    let paths: Vec<PathBuf> = (0..files).map(|_| incoming.join(incoming_name())).collect();
    let result = (|| {
        let mut writers = paths
            .iter()
            .map(|p| CafWriter::create(p, format.sample_rate, per_file, format.depth, WriteMode::Normal))
            .collect::<Result<Vec<_>, _>>()?;
        let ch = usize::from(info.channels);
        let expected = if info.sample_rate > 0.0 {
            info.frames as f64 * f64::from(format.sample_rate) / info.sample_rate
        } else {
            0.0
        };
        let mut done = 0u64;
        let mut lanes: Vec<Vec<f32>> = vec![Vec::new(); if split { files } else { 0 }];
        let mut sink = |chunk: &[f32]| -> Result<(), MediaError> {
            if cancel.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            if split {
                for (k, lane) in lanes.iter_mut().enumerate() {
                    lane.clear();
                    lane.extend(chunk.iter().skip(k).step_by(ch).copied());
                }
                for (w, lane) in writers.iter_mut().zip(&lanes) {
                    w.write(lane)?;
                }
            } else if let Some(w) = writers.first_mut() {
                w.write(chunk)?;
            }
            done += (chunk.len() / ch) as u64;
            if expected > 0.0 {
                progress((done as f64 / expected).min(1.0) as f32);
            }
            Ok(())
        };
        let frames = importer.decode(source, format.sample_rate, &mut sink, cancel)?;
        if frames == 0 {
            return Err(MediaError::Unsupported("the file holds no audio".into()));
        }
        let written: Vec<Written> = writers
            .into_iter()
            .map(CafWriter::finish)
            .collect::<Result<_, _>>()?;
        std::fs::create_dir_all(&media)?;
        let base = clean_stem(stem);
        let original_name = source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut out = Vec::with_capacity(files);
        let mut linked: Vec<PathBuf> = Vec::new();
        for (k, (p, w)) in paths.iter().zip(written).enumerate() {
            let s = if split {
                format!("{base} ch{}", k + 1)
            } else {
                base.clone()
            };
            let name = match link_free(p, &media, &s) {
                Ok(n) => n,
                Err(e) => {
                    remove_all(&linked);
                    return Err(e);
                }
            };
            linked.push(media.join(&name));
            let display = name.trim_end_matches(".caf").to_string();
            out.push(Imported {
                path: format!("{MEDIA_DIR}/{name}"),
                name: display,
                original_name: original_name.clone(),
                channels: per_file,
                sample_rate: format.sample_rate,
                length_frames: w.frames,
                content_hash: w.content_hash,
                clipped: w.clipped,
            });
        }
        progress(1.0);
        Ok(out)
    })();
    // The incoming files go either way: linked names keep the data.
    remove_all(&paths);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caf::hash_file;
    use core_model::format::BitDepth;

    /// Decodes nothing real: `channels` channels of `frames` frames where
    /// channel k of frame f is `(k + 1) * 0.01 + f * 1e-6`, delivered in
    /// chunks of 1000 frames; `cancel_after` sets the flag after that
    /// many chunks.
    struct Fake {
        channels: u16,
        frames: u64,
        cancel_after: Option<usize>,
    }

    impl MediaImporter for Fake {
        fn probe(&self, _: &Path) -> Result<SourceInfo, MediaError> {
            Ok(SourceInfo {
                format_name: "Fake".into(),
                sample_rate: 48_000.0,
                bits: 24,
                channels: self.channels,
                frames: self.frames,
            })
        }

        fn decode(
            &self,
            _: &Path,
            _: u32,
            sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
            cancel: &AtomicBool,
        ) -> Result<u64, MediaError> {
            let ch = usize::from(self.channels);
            let mut f = 0u64;
            let mut chunks = 0usize;
            while f < self.frames {
                if self.cancel_after == Some(chunks) {
                    cancel.store(true, Ordering::Relaxed);
                }
                let n = (self.frames - f).min(1000);
                let buf: Vec<f32> = (0..n as usize * ch)
                    .map(|i| {
                        let (fr, k) = (f + (i / ch) as u64, i % ch);
                        (k + 1) as f32 * 0.01 + fr as f32 * 1e-6
                    })
                    .collect();
                sink(&buf)?;
                f += n;
                chunks += 1;
            }
            Ok(f)
        }
    }

    fn package() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn fmt(depth: BitDepth) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            depth,
        }
    }

    fn import(
        imp: &dyn MediaImporter,
        src: &Path,
        pkg: &Path,
        depth: BitDepth,
    ) -> Result<Vec<Imported>, MediaError> {
        import_file(imp, src, pkg, fmt(depth), &mut |_| {}, &AtomicBool::new(false))
    }

    /// Files in `Media/` (not `.incoming`) and in `.incoming`.
    fn listing(pkg: &Path) -> (Vec<String>, Vec<String>) {
        let names = |d: PathBuf| -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(d)
                .map(|r| {
                    r.filter_map(|e| e.ok())
                        .filter(|e| e.path().is_file())
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            v.sort();
            v
        };
        (names(pkg.join(MEDIA_DIR)), names(pkg.join(INCOMING_DIR)))
    }

    #[test]
    fn stereo_imports_into_one_file_in_the_project_depth() {
        let pkg = package();
        let fake = Fake { channels: 2, frames: 2500, cancel_after: None };
        let mut last = 0.0;
        let got = import_file(
            &fake,
            Path::new("/somewhere/Kick Loop.aiff"),
            pkg.path(),
            fmt(BitDepth::Int16),
            &mut |p| last = p,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(last, 1.0);
        assert_eq!(got.len(), 1);
        let i = &got[0];
        assert_eq!(
            (i.path.as_str(), i.name.as_str(), i.original_name.as_str(), i.channels, i.length_frames),
            ("Media/Kick Loop.caf", "Kick Loop", "Kick Loop.aiff", 2, 2500)
        );
        let p = pkg.path().join(&i.path);
        let r = CafReader::open(&p).unwrap();
        assert_eq!((r.info().depth, r.info().frames), (BitDepth::Int16, 2500));
        assert_eq!(hash_file(&p).unwrap(), i.content_hash);
        assert_eq!(listing(pkg.path()), (vec!["Kick Loop.caf".to_string()], vec![]));
    }

    #[test]
    fn six_channels_split_into_six_mono_items() {
        let pkg = package();
        let fake = Fake { channels: 6, frames: 1500, cancel_after: None };
        let got = import(&fake, Path::new("/x/surround.wav"), pkg.path(), BitDepth::Float32).unwrap();
        let names: Vec<&str> = got.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["surround ch1", "surround ch2", "surround ch3", "surround ch4", "surround ch5", "surround ch6"]
        );
        for (k, i) in got.iter().enumerate() {
            assert_eq!((i.channels, i.length_frames), (1, 1500));
            let r = CafReader::open(&pkg.path().join(&i.path)).unwrap();
            let mut out = vec![0.0; 1500];
            r.read(0, &mut out, &mut Vec::new()).unwrap();
            assert_eq!(out[10], (k + 1) as f32 * 0.01 + 10.0 * 1e-6);
        }
    }

    #[test]
    fn names_never_collide() {
        let pkg = package();
        let fake = Fake { channels: 1, frames: 10, cancel_after: None };
        let a = import(&fake, Path::new("/x/a.wav"), pkg.path(), BitDepth::Int24).unwrap();
        let b = import(&fake, Path::new("/y/a.mp3"), pkg.path(), BitDepth::Int24).unwrap();
        let c = import(&fake, Path::new("/y/.hidden"), pkg.path(), BitDepth::Int24).unwrap();
        assert_eq!(a[0].path, "Media/a.caf");
        assert_eq!(b[0].path, "Media/a 2.caf");
        assert_eq!(c[0].path, "Media/hidden.caf");
    }

    #[test]
    fn cancel_leaves_nothing_in_media() {
        let pkg = package();
        let fake = Fake { channels: 2, frames: 10_000, cancel_after: Some(3) };
        let r = import(&fake, Path::new("/x/a.wav"), pkg.path(), BitDepth::Int24);
        assert_eq!(r, Err(MediaError::Cancelled));
        assert_eq!(listing(pkg.path()), (vec![], vec![]));
    }

    #[test]
    fn garbage_and_empty_files_fail_alone() {
        let pkg = package();
        let src = package();
        let text = src.path().join("notes.wav");
        std::fs::write(&text, "not audio at all").unwrap();
        assert!(matches!(
            import(&CafImporter, &text, pkg.path(), BitDepth::Int24),
            Err(MediaError::NotCaf(_))
        ));
        let empty = Fake { channels: 2, frames: 0, cancel_after: None };
        assert!(matches!(
            import(&empty, Path::new("/x/e.wav"), pkg.path(), BitDepth::Int24),
            Err(MediaError::Unsupported(_))
        ));
        assert_eq!(listing(pkg.path()), (vec![], vec![]));
        // The next import still works.
        let ok = Fake { channels: 1, frames: 5, cancel_after: None };
        assert!(import(&ok, Path::new("/x/ok.wav"), pkg.path(), BitDepth::Int24).is_ok());
    }

    #[test]
    fn the_caf_importer_converts_depth_but_not_rate() {
        let pkg = package();
        let src = package();
        let p = src.path().join("f.caf");
        let mut w = CafWriter::create(&p, 48_000, 1, BitDepth::Float32, WriteMode::Normal).unwrap();
        w.write(&[0.5; 20_000]).unwrap();
        w.finish().unwrap();
        let got = import(&CafImporter, &p, pkg.path(), BitDepth::Int24).unwrap();
        assert_eq!(got[0].length_frames, 20_000);
        let r = CafReader::open(&pkg.path().join(&got[0].path)).unwrap();
        assert_eq!(r.info().depth, BitDepth::Int24);
        let at_44k = import_file(
            &CafImporter,
            &p,
            pkg.path(),
            AudioFormat { sample_rate: 44_100, depth: BitDepth::Int24 },
            &mut |_| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(at_44k, Err(MediaError::Unsupported(_))));
    }
}
VIZDAW_EOF
```

In `crates/core-media/src/lib.rs`:

Find:
```rust
pub mod caf;
mod error;
```
Replace:
```rust
pub mod caf;
mod error;
pub mod import;
```

- [ ] **Step 2: Run the tests to verify they fail**

With the bodies of `import_as`, `link_free`, `clean_stem` and `CafImporter::decode` stubbed (`todo!()`): `cargo test -p core-media import 2>&1 | tail -10`
Expected: the six import tests fail.

- [ ] **Step 3: Implement**

Restore the bodies from the listing. `chunk.iter().skip(k).step_by(ch)` needs `ch ≥ 1`, which the `channels == 0` check guarantees.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-media 2>&1 | tail -5`
Expected: `test result: ok. 14 passed` (8 CAF + 6 import).

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-media
git commit -m "core-media: import pipeline (incoming folder, free names, mono split)

A MediaImporter decodes to float at the project rate; core-media writes
CAF files in the project depth into Media/.incoming and links them into
Media/ under the first free name. Wider-than-stereo sources become one
mono file per channel; an error or cancel leaves nothing behind.
CafImporter reads VizDAW CAF at the same rate.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 5: `platform-macos` — the ExtAudioFile importer

**Files:**
- Create: `crates/platform-macos/src/media.rs`, `crates/platform-macos/tests/media.rs`
- Modify: `crates/platform-macos/Cargo.toml`, `crates/platform-macos/src/lib.rs`, `Cargo.lock`
- Test: `crates/platform-macos/tests/media.rs`

**Interfaces:**
- Consumes: Task 4's `core_media::import::{MediaImporter, SourceInfo, import_file}` and `core_media::MediaError`.
- Produces: `platform_macos::media::ExtAudioImporter` (unit struct, `Default`), implementing `MediaImporter`. `probe` returns the file's rate, channels, bits and length, with `format_name` set to the data format's four-character code (`lpcm`, `flac`, `aac `, …). `decode` converts to float32 interleaved at the requested rate with the sample-rate converter at maximum quality and Mastering complexity.

Spec §5.1 and approach B. Only the import path uses AudioToolbox; nothing on the real-time thread does.

- [ ] **Step 1: Write the failing tests**

In `crates/platform-macos/Cargo.toml`:

Find:
```toml
objc2-audio-toolbox = { version = "0.3.2", default-features = false, features = ["std", "AudioComponent", "AUComponent", "AudioUnitProperties", "AudioUnitUtilities", "MusicDevice", "objc2-core-audio-types", "objc2-core-foundation", "block2"] }
```
Replace:
```toml
objc2-audio-toolbox = { version = "0.3.2", default-features = false, features = ["std", "AudioComponent", "AUComponent", "AudioUnitProperties", "AudioUnitUtilities", "MusicDevice", "ExtendedAudioFile", "AudioFile", "AudioConverter", "objc2-core-audio-types", "objc2-core-foundation", "block2"] }
core-media = { path = "../core-media" }
```

Find:
```toml
[target.'cfg(target_os = "macos")'.dev-dependencies]
core-engine = { path = "../core-engine", features = ["fixtures"] }
```
Replace:
```toml
[target.'cfg(target_os = "macos")'.dev-dependencies]
core-engine = { path = "../core-engine", features = ["fixtures"] }
tempfile = "3"
```

Create `crates/platform-macos/tests/media.rs`. It writes its own WAV, AIFF and FLAC fixtures with `ExtAudioFileCreateWithURL`, so no audio files are checked in:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/platform-macos/tests/media.rs <<'VIZDAW_EOF'
//! The ExtAudioFile importer (plan 5a Task 5): WAV, AIFF and FLAC
//! fixtures written here, imported into CAF in a project format.
#![cfg(target_os = "macos")]
#![allow(unsafe_code)]

use core_media::MediaError;
use core_media::caf::CafReader;
use core_media::import::{MediaImporter, import_file};
use core_model::format::{AudioFormat, BitDepth};
use objc2_audio_toolbox::{
    ExtAudioFileCreateWithURL, ExtAudioFileDispose, ExtAudioFileRef, ExtAudioFileSetProperty,
    ExtAudioFileWrite, kAudioFileAIFFType, kAudioFileFLACType, kAudioFileFlags_EraseFile,
    kAudioFileWAVEType, kExtAudioFileProperty_ClientDataFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, kAppleLosslessFormatFlag_16BitSourceData,
    kAudioFormatFLAC, kAudioFormatFlagIsBigEndian, kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked,
    kAudioFormatFlagIsSignedInteger, kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFURL;
use platform_macos::media::ExtAudioImporter;
use std::ffi::c_void;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy)]
enum Kind {
    Wav,
    Aiff,
    Flac,
}

fn pcm(rate: f64, ch: u32, flags: u32) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: flags | kAudioFormatFlagIsPacked,
        mBytesPerPacket: 2 * ch,
        mFramesPerPacket: 1,
        mBytesPerFrame: 2 * ch,
        mChannelsPerFrame: ch,
        mBitsPerChannel: 16,
        mReserved: 0,
    }
}

/// Writes `frames` frames of a 441 Hz sine at amplitude 0.5 (channel k
/// phase-shifted by k) as a 16-bit `kind` file.
fn fixture(path: &Path, kind: Kind, rate: f64, ch: u32, frames: usize) {
    let (file_type, mut desc) = match kind {
        Kind::Wav => (kAudioFileWAVEType, pcm(rate, ch, kAudioFormatFlagIsSignedInteger)),
        Kind::Aiff => (
            kAudioFileAIFFType,
            pcm(rate, ch, kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsBigEndian),
        ),
        Kind::Flac => (
            kAudioFileFLACType,
            AudioStreamBasicDescription {
                mSampleRate: rate,
                mFormatID: kAudioFormatFLAC,
                mFormatFlags: kAppleLosslessFormatFlag_16BitSourceData,
                mBytesPerPacket: 0,
                mFramesPerPacket: 0,
                mBytesPerFrame: 0,
                mChannelsPerFrame: ch,
                mBitsPerChannel: 0,
                mReserved: 0,
            },
        ),
    };
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `bytes` is a valid buffer of the given length.
    let url = unsafe {
        CFURL::from_file_system_representation(None, bytes.as_ptr(), bytes.len() as isize, false)
    }
    .unwrap();
    let mut f: ExtAudioFileRef = std::ptr::null_mut();
    // SAFETY: valid pointers for the duration of the call.
    let st = unsafe {
        ExtAudioFileCreateWithURL(
            &url,
            file_type,
            NonNull::from(&mut desc),
            std::ptr::null(),
            kAudioFileFlags_EraseFile.0,
            NonNull::from(&mut f),
        )
    };
    assert_eq!(st, 0, "create {path:?}");
    let mut client = AudioStreamBasicDescription {
        mSampleRate: rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
        mBytesPerPacket: 4 * ch,
        mFramesPerPacket: 1,
        mBytesPerFrame: 4 * ch,
        mChannelsPerFrame: ch,
        mBitsPerChannel: 32,
        mReserved: 0,
    };
    // SAFETY: `client` is a valid ASBD of the stated size.
    let st = unsafe {
        ExtAudioFileSetProperty(
            f,
            kExtAudioFileProperty_ClientDataFormat,
            size_of::<AudioStreamBasicDescription>() as u32,
            NonNull::from(&mut client).cast::<c_void>(),
        )
    };
    assert_eq!(st, 0);
    let mut data: Vec<f32> = (0..frames * ch as usize)
        .map(|i| {
            let (fr, k) = (i / ch as usize, i % ch as usize);
            0.5 * (2.0 * std::f32::consts::PI * 441.0 * fr as f32 / rate as f32 + k as f32).sin()
        })
        .collect();
    let mut abl = AudioBufferList {
        mNumberBuffers: 1,
        mBuffers: [AudioBuffer {
            mNumberChannels: ch,
            mDataByteSize: (data.len() * 4) as u32,
            mData: data.as_mut_ptr().cast::<c_void>(),
        }],
    };
    // SAFETY: `abl` describes `data`, alive for the call.
    let st = unsafe { ExtAudioFileWrite(f, frames as u32, NonNull::from(&mut abl)) };
    assert_eq!(st, 0, "write {path:?}");
    // SAFETY: `f` was created above and is disposed once.
    unsafe { ExtAudioFileDispose(f) };
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn import(src: &Path, pkg: &Path, rate: u32, depth: BitDepth) -> Result<Vec<core_media::import::Imported>, MediaError> {
    import_file(
        &ExtAudioImporter,
        src,
        pkg,
        AudioFormat { sample_rate: rate, depth },
        &mut |_| {},
        &AtomicBool::new(false),
    )
}

#[test]
fn probe_reads_the_source_format() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.wav");
    fixture(&p, Kind::Wav, 44_100.0, 2, 44_100);
    let i = ExtAudioImporter.probe(&p).unwrap();
    assert_eq!((i.sample_rate, i.channels, i.bits, i.frames), (44_100.0, 2, 16, 44_100));
    assert_eq!(i.format_name, "lpcm");
}

#[test]
fn importing_44k1_into_a_48k_project_converts() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("in.wav");
    fixture(&src, Kind::Wav, 44_100.0, 2, 44_100);
    let pkg = dir.path().join("p.vizdaw");
    let got = import(&src, &pkg, 48_000, BitDepth::Int24).unwrap();
    assert_eq!(got.len(), 1);
    let i = &got[0];
    assert!(i.length_frames.abs_diff(48_000) <= 1, "{}", i.length_frames);
    let r = CafReader::open(&pkg.join(&i.path)).unwrap();
    assert_eq!(
        (r.info().sample_rate, r.info().channels, r.info().depth),
        (48_000, 2, BitDepth::Int24)
    );
    let mut out = vec![0.0; r.info().frames as usize * 2];
    r.read(0, &mut out, &mut Vec::new()).unwrap();
    // A 0.5 sine has an RMS of 0.3536; the converter keeps it.
    let left: Vec<f32> = out.iter().step_by(2).skip(1000).take(40_000).copied().collect();
    assert!((rms(&left) - 0.3536).abs() < 0.005, "{}", rms(&left));
}

#[test]
fn wav_aiff_and_flac_import_sample_for_sample() {
    let dir = tempfile::tempdir().unwrap();
    for (kind, name) in [(Kind::Wav, "a.wav"), (Kind::Aiff, "a.aiff"), (Kind::Flac, "a.flac")] {
        let src = dir.path().join(name);
        fixture(&src, kind, 48_000.0, 1, 48_000);
        let pkg = dir.path().join(format!("{name}.vizdaw"));
        let got = import(&src, &pkg, 48_000, BitDepth::Float32).unwrap();
        assert_eq!(got[0].length_frames, 48_000, "{name}");
        let r = CafReader::open(&pkg.join(&got[0].path)).unwrap();
        let mut out = vec![0.0; 48_000];
        r.read(0, &mut out, &mut Vec::new()).unwrap();
        for (f, x) in out.iter().enumerate().step_by(997) {
            let want = 0.5 * (2.0 * std::f32::consts::PI * 441.0 * f as f32 / 48_000.0).sin();
            // 16-bit source: within two steps.
            assert!((x - want).abs() <= 2.0 / 32_768.0, "{name} frame {f}: {x} vs {want}");
        }
    }
}

#[test]
fn garbage_is_an_error_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bad.wav");
    std::fs::write(&p, "RIFF but not really").unwrap();
    assert!(ExtAudioImporter.probe(&p).is_err());
    assert!(import(&p, &dir.path().join("p.vizdaw"), 48_000, BitDepth::Int24).is_err());
    assert!(ExtAudioImporter.probe(&dir.path().join("missing.wav")).is_err());
}

#[test]
fn decode_stops_when_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("long.wav");
    fixture(&src, Kind::Wav, 48_000.0, 2, 480_000);
    let cancel = AtomicBool::new(true);
    let r = ExtAudioImporter.decode(&src, 48_000, &mut |_| Ok(()), &cancel);
    assert_eq!(r, Err(MediaError::Cancelled));
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p platform-macos --test media 2>&1 | tail -5`
Expected: compile error, `unresolved import platform_macos::media`.

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/platform-macos/src/media.rs <<'VIZDAW_EOF'
//! The macOS importer (sub-project 5 spec §5.1, approach B): ExtAudioFile
//! reads every format the system reads (WAV, AIFF, CAF, FLAC, MP3, AAC,
//! ALAC…) and converts to float32 at the project rate with the
//! highest-quality sample-rate converter. `core_media::import` writes the
//! CAF files. Never used on the real-time thread.

use core_media::MediaError;
use core_media::import::{MediaImporter, SourceInfo};
use objc2_audio_toolbox::{
    AudioConverterRef, AudioConverterSetProperty, ExtAudioFileDispose, ExtAudioFileGetProperty,
    ExtAudioFileOpenURL, ExtAudioFileRead, ExtAudioFileRef, ExtAudioFileSetProperty,
    kAudioConverterQuality_Max, kAudioConverterSampleRateConverterComplexity,
    kAudioConverterSampleRateConverterComplexity_Mastering,
    kAudioConverterSampleRateConverterQuality, kExtAudioFileProperty_AudioConverter,
    kExtAudioFileProperty_ClientDataFormat, kExtAudioFileProperty_ConverterConfig,
    kExtAudioFileProperty_FileDataFormat, kExtAudioFileProperty_FileLengthFrames,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsPacked, kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFURL;
use std::ffi::c_void;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

/// Frames per `ExtAudioFileRead`.
const CHUNK: u32 = 8192;

/// Imports any file the system can read (see the module docs).
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtAudioImporter;

/// An open ExtAudioFile, disposed on drop.
struct File(ExtAudioFileRef);

impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `ExtAudioFileOpenURL` and is disposed
        // exactly once, here.
        unsafe { ExtAudioFileDispose(self.0) };
    }
}

fn check(status: i32, call: &str, path: &Path) -> Result<(), MediaError> {
    if status == 0 {
        Ok(())
    } else {
        let cc = four_cc(status as u32);
        Err(MediaError::Unsupported(format!(
            "{}: {call} failed ({status}{})",
            path.display(),
            if cc.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                format!(", '{cc}'")
            } else {
                String::new()
            }
        )))
    }
}

fn four_cc(v: u32) -> String {
    v.to_be_bytes().iter().map(|b| char::from(*b)).collect()
}

fn open(path: &Path) -> Result<File, MediaError> {
    if !path.is_file() {
        return Err(MediaError::Io(format!("{}: no such file", path.display())));
    }
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `bytes` is a valid buffer of the given length.
    let url = unsafe {
        CFURL::from_file_system_representation(None, bytes.as_ptr(), bytes.len() as isize, false)
    }
    .ok_or_else(|| MediaError::Io(format!("{}: not a file URL", path.display())))?;
    let mut f: ExtAudioFileRef = std::ptr::null_mut();
    // SAFETY: `url` is a valid CFURL and `f` a valid out pointer.
    let st = unsafe { ExtAudioFileOpenURL(&url, NonNull::from(&mut f)) };
    check(st, "ExtAudioFileOpenURL", path)?;
    Ok(File(f))
}

fn get<T: Copy + Default>(f: &File, id: u32, path: &Path) -> Result<T, MediaError> {
    let mut v = T::default();
    let mut size = size_of::<T>() as u32;
    // SAFETY: `v` is a buffer of `size` bytes for a property of type `T`.
    let st = unsafe {
        ExtAudioFileGetProperty(
            f.0,
            id,
            NonNull::from(&mut size),
            NonNull::from(&mut v).cast::<c_void>(),
        )
    };
    check(st, "ExtAudioFileGetProperty", path)?;
    Ok(v)
}

fn file_format(f: &File, path: &Path) -> Result<AudioStreamBasicDescription, MediaError> {
    let d: AudioStreamBasicDescription = get(f, kExtAudioFileProperty_FileDataFormat, path)?;
    if d.mChannelsPerFrame == 0 || !(d.mSampleRate > 0.0) {
        return Err(MediaError::Unsupported(format!(
            "{}: no channels or no rate",
            path.display()
        )));
    }
    Ok(d)
}

/// Asks the file's converter for maximum quality (best effort: a file
/// without a converter, e.g. no rate change, keeps what it has).
fn best_quality(f: &File, path: &Path) {
    let Ok(conv) = get::<AudioConverterRef>(f, kExtAudioFileProperty_AudioConverter, path) else {
        return;
    };
    if conv.is_null() {
        return;
    }
    for (prop, value) in [
        (kAudioConverterSampleRateConverterQuality, kAudioConverterQuality_Max),
        (
            kAudioConverterSampleRateConverterComplexity,
            kAudioConverterSampleRateConverterComplexity_Mastering,
        ),
    ] {
        let mut v = value;
        // SAFETY: `conv` is the file's live converter; `v` a valid u32.
        let _ = unsafe {
            AudioConverterSetProperty(
                conv,
                prop,
                size_of::<u32>() as u32,
                NonNull::from(&mut v).cast::<c_void>(),
            )
        };
    }
    // A null converter configuration makes ExtAudioFile apply the
    // converter's new properties.
    let mut none: *const c_void = std::ptr::null();
    // SAFETY: the property takes a `CFPropertyListRef`; null is allowed.
    let _ = unsafe {
        ExtAudioFileSetProperty(
            f.0,
            kExtAudioFileProperty_ConverterConfig,
            size_of::<*const c_void>() as u32,
            NonNull::from(&mut none).cast::<c_void>(),
        )
    };
}

impl MediaImporter for ExtAudioImporter {
    fn probe(&self, path: &Path) -> Result<SourceInfo, MediaError> {
        let f = open(path)?;
        let d = file_format(&f, path)?;
        let frames: i64 = get(&f, kExtAudioFileProperty_FileLengthFrames, path)?;
        Ok(SourceInfo {
            format_name: four_cc(d.mFormatID),
            sample_rate: d.mSampleRate,
            bits: d.mBitsPerChannel,
            channels: u16::try_from(d.mChannelsPerFrame).unwrap_or(u16::MAX),
            frames: u64::try_from(frames).unwrap_or(0),
        })
    }

    fn decode(
        &self,
        path: &Path,
        rate: u32,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
        cancel: &AtomicBool,
    ) -> Result<u64, MediaError> {
        let f = open(path)?;
        let ch = file_format(&f, path)?.mChannelsPerFrame;
        let mut client = AudioStreamBasicDescription {
            mSampleRate: f64::from(rate),
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
            mBytesPerPacket: 4 * ch,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4 * ch,
            mChannelsPerFrame: ch,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        // SAFETY: `client` is a valid ASBD of the stated size.
        let st = unsafe {
            ExtAudioFileSetProperty(
                f.0,
                kExtAudioFileProperty_ClientDataFormat,
                size_of::<AudioStreamBasicDescription>() as u32,
                NonNull::from(&mut client).cast::<c_void>(),
            )
        };
        check(st, "set client format", path)?;
        best_quality(&f, path);
        let per = ch as usize;
        let mut buf = vec![0.0f32; CHUNK as usize * per];
        let mut total = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            let mut frames = CHUNK;
            let mut abl = AudioBufferList {
                mNumberBuffers: 1,
                mBuffers: [AudioBuffer {
                    mNumberChannels: ch,
                    mDataByteSize: (buf.len() * 4) as u32,
                    mData: buf.as_mut_ptr().cast::<c_void>(),
                }],
            };
            // SAFETY: `abl` describes `buf`, which outlives the call;
            // `frames` is a valid in/out count.
            let st = unsafe { ExtAudioFileRead(f.0, NonNull::from(&mut frames), NonNull::from(&mut abl)) };
            check(st, "ExtAudioFileRead", path)?;
            if frames == 0 {
                return Ok(total);
            }
            let n = (frames as usize).min(CHUNK as usize) * per;
            sink(buf.get(..n).unwrap_or(&[]))?;
            total += u64::from(frames);
        }
    }
}
VIZDAW_EOF
```

In `crates/platform-macos/src/lib.rs`:

Find:
```rust
//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs`, `rt.rs` and
//! `au/ffi.rs`, with a `// SAFETY:` comment per block.
```
Replace:
```rust
//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs`, `rt.rs`,
//! `media.rs` and `au/ffi.rs`, with a `// SAFETY:` comment per block.
```

Find:
```rust
pub mod hal;
pub mod midi;
```
Replace:
```rust
pub mod hal;
pub mod media;
pub mod midi;
```

The `objc2-*` bindings may differ in detail from these calls, so check them against the generated sources (`~/.cargo/registry/src/*/objc2-audio-toolbox-0.3.2/src/generated/{ExtendedAudioFile,AudioConverter,AudioFile}.rs`). Things to look out for:
- whether `ExtAudioFileRead` takes `NonNull<u32>` or `*mut u32`;
- whether `kAudioFileFlags_EraseFile` is a newtype (`.0`) or a plain `u32`;
- whether `AudioConverterRef` is a raw pointer.

Adapt the calls to the real signatures without changing behaviour. If the system cannot encode FLAC on this macOS (`ExtAudioFileCreateWithURL` returns `'fmt?'`), keep WAV and AIFF in `wav_aiff_and_flac_import_sample_for_sample`, make the FLAC fixture fall back to skipping with an `eprintln!`, and say so in the report.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p platform-macos --test media 2>&1 | tail -5`
Expected: `test result: ok. 5 passed`.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | sort | uniq -c
git add crates/platform-macos Cargo.lock
git commit -m "platform-macos: ExtAudioFile importer

Decodes any format the system reads to float32 at the project rate with
the sample-rate converter at maximum quality; core-media writes the CAF.
Tests write their own WAV, AIFF and FLAC fixtures.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 6: `core-media` — the disk streamer

**Files:**
- Create: `crates/core-media/src/stream.rs`
- Modify: `crates/core-media/src/lib.rs`
- Test: unit tests and a proptest in `crates/core-media/src/stream.rs`

**Interfaces:**
- Consumes: Task 3's `caf::CafReader`; `core_model::ids::MediaId`.
- Produces:
  - `core_media::stream::{Streamer, StreamReader, StreamConfig, DiskMode, MediaFile, Need, Read, StreamCounters, SLOTS}`, with:
    - `StreamConfig { voice_seconds: f64, ahead_seconds: f64, seam_seconds: f64, ready_seconds: f64, cache_file_bytes: u64, cache_budget_bytes: u64, min_read_bytes: u64, stream_voices: usize }` (`Default` = the spec's 2 s, 3 s, 2 s, 0.25 s, 4 MiB, 512 MiB, 64 KiB, 256);
    - `MediaFile { id: MediaId, path: PathBuf }`;
    - `Need { media: MediaId, start: u64, frames: u64, file_frame: u64 }`;
    - `Read::{Done, Miss, Offline}`;
    - `StreamCounters { underruns: u64, late: u64, cached_files: usize, stream_voices: usize }`;
    - `Streamer::new(rate: f64, cfg: StreamConfig, mode: DiskMode) -> Result<(Streamer, StreamReader), MediaError>`.
  - `Streamer` methods (control side, may allocate and lock):
    - `set_media(&self, files: Vec<MediaFile>)`, `set_plan(&self, needs: Arc<[Need]>)`, `set_loop(&self, range: Option<(u64, u64)>)`;
    - `hint(&self, frame: u64)`, `ready(&self, frame: u64) -> bool`, `wait_ready(&self, frame: u64, timeout: Duration) -> bool`;
    - `service(&self)` (one disk pass; Manual mode), `counters(&self) -> StreamCounters`, `take_errors(&self) -> Vec<(MediaId, String)>`.
  - `StreamReader` methods (`// RT`):
    - `begin_block(&self, position: u64)`;
    - `read(&self, media: MediaId, start: u64, origin: u64, file_frame: u64, left: &mut [f32], right: &mut [f32]) -> Read`, where `start` and `origin` are the segment's `Need::start` and `Need::file_frame`;
    - `count_miss(&self, late: bool)`.

Spec §5.5 and Ruling 9. The design, which the module doc repeats:

- **Voices.**
  - A voice is a ring of `cap` frames by `stride` channels of `AtomicU32` (f32 bits) plus atomics: `media` (0 = unpublished, else id + 1), `start` and `origin` (the need's key, or `u64::MAX` for whole-file cache voices), `base` (the first file frame it streamed), and `hi` (one past the last frame written, stored with `Release` after the data).
  - Frames `[max(base, hi − cap), hi)` are readable.
  - Stream voices are allocated on demand by the disk thread with `stride` 2 and `cap = voice_seconds × rate`. They stay in their slot until the streamer goes.
  - Cache voices hold a whole small file (`cap` = its frames, `stride` = its channels), live in slots `stream_voices..`, and are freed after retirement.
- **The real-time side** scans the slots for a voice with the key whose readable range covers the request and copies with `Relaxed` loads. It never writes a voice.
- **Overwrites.** The disk thread writes frame `f` only when `f < floor + cap`, where `floor` is the file frame under the published playhead for the voice that holds it (its `base` otherwise). A frame at or after the playhead is never overwritten while it can still be read.
- **Retirement.** To retire a voice, the disk thread unpublishes it (`media := 0`) and notes the epoch, which the real-time thread bumps at each block's start. It changes or frees the voice only after the epoch has advanced twice. Manual mode, single-threaded, retires at once.
- **Planning.** Each disk pass plans for these windows:
  - `[P, P + ahead)`, cut at the loop end and continued at the loop start when it wraps;
  - plus `[loop start, loop start + seam)` while looping.

  `P` is a pending hint, else the published position. Every need overlapping a window gets a voice. Reads are at least `min_read_bytes`, unless they reach the window's end. Voices no window uses are retired.
- **Media.**
  - A media file's reader opens on first use.
  - A file of at most `cache_file_bytes` of sample data is read whole into a cache voice, within `cache_budget_bytes` of float data, evicting the least recently needed.
  - A read error or a short read marks the media failed, reports it once in `take_errors`, and the reader then answers `Read::Offline` for it (no counter). Media not in `set_media` is the caller's to skip; the engine marks its segments offline.

- [ ] **Step 1: Write the failing tests**

Write `crates/core-media/src/stream.rs` (below) with every method body `todo!()` at first, then run the tests to see them fail. In `crates/core-media/src/lib.rs`:

Find:
```rust
pub mod import;
```
Replace:
```rust
pub mod import;
pub mod stream;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-media stream 2>&1 | tail -10`
Expected: the stream tests panic at `todo!()`.

- [ ] **Step 3: Implement**

The complete file:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-media/src/stream.rs <<'VIZDAW_EOF'
//! The disk streamer (sub-project 5 spec §5.5; plan 5a Ruling 9).
//!
//! **Voices.** A voice is a ring of `cap` frames × `stride` channels of
//! f32 bits in `AtomicU32`s, with atomics for its key (`media` + 1, 0 =
//! unpublished; the need's `start` and `origin`, or `u64::MAX` for a
//! whole-file cache voice), `base` (the first file frame it streamed) and
//! `hi` (one past the last frame written, stored with `Release` after the
//! data). Frames `[max(base, hi − cap), hi)` are readable.
//!
//! **Real-time side.** [`StreamReader::read`] scans the slots for a voice
//! with the key whose readable range covers the request and copies with
//! `Relaxed` loads; it never writes a voice. [`StreamReader::begin_block`]
//! bumps the epoch and publishes the playhead.
//!
//! **Disk side.** It writes frame `f` of a voice only when `f < floor +
//! cap`, `floor` being the file frame under the published playhead for
//! that voice (else its `base`), so nothing at or after the playhead is
//! overwritten while it can be read. It retires a voice by unpublishing it
//! and changes or frees it only once the epoch advanced twice (Manual mode
//! is single-threaded and retires at once). Each pass plans the windows
//! `[P, P + ahead)` (wrapping at the loop end) and, while looping,
//! `[loop start, loop start + seam)`.
//!
//! **Media.** Small files are read whole into cache voices within a
//! budget (least recently needed evicted); a read error or a short read
//! marks the media failed, reported once by [`Streamer::take_errors`], and
//! reads of it answer [`Read::Offline`].

use crate::MediaError;
use crate::caf::CafReader;
use core_model::ids::MediaId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Slots for stream voices.
pub const STREAM_VOICES: usize = 256;
/// Slots for whole-file cache voices.
pub const CACHE_VOICES: usize = 256;
/// Every slot.
pub const SLOTS: usize = STREAM_VOICES + CACHE_VOICES;
/// The key of a cache voice (any need of its media).
const ANY: u64 = u64::MAX;
/// Failed media the real-time side can tell apart (beyond this they
/// count as misses).
const FAILED_SLOTS: usize = 64;

/// Streaming sizes (spec §5.5); tests shrink them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamConfig {
    /// Ring length of a stream voice (2 s).
    pub voice_seconds: f64,
    /// Read-ahead window (3 s).
    pub ahead_seconds: f64,
    /// Kept prefilled after the loop start (2 s).
    pub seam_seconds: f64,
    /// What `ready` checks after a position (0.25 s, the locate wait).
    pub ready_seconds: f64,
    /// Files of at most this much sample data are cached whole (4 MiB).
    pub cache_file_bytes: u64,
    /// Float bytes the cache may hold (512 MiB).
    pub cache_budget_bytes: u64,
    /// Smallest read (64 KiB), unless it reaches the window's end.
    pub min_read_bytes: u64,
    /// Stream voices (256).
    pub stream_voices: usize,
}

impl Default for StreamConfig {
    fn default() -> StreamConfig {
        StreamConfig {
            voice_seconds: 2.0,
            ahead_seconds: 3.0,
            seam_seconds: 2.0,
            ready_seconds: 0.25,
            cache_file_bytes: 4 << 20,
            cache_budget_bytes: 512 << 20,
            min_read_bytes: 64 << 10,
            stream_voices: STREAM_VOICES,
        }
    }
}

/// Who runs disk passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskMode {
    /// A disk thread, every few milliseconds and when woken.
    Thread,
    /// Nobody: the owner calls [`Streamer::service`] (tests; retirement
    /// is immediate, so no real-time thread may read meanwhile).
    Manual,
}

/// A media file the streamer may read (an online item).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFile {
    /// The item.
    pub id: MediaId,
    /// Its CAF file.
    pub path: PathBuf,
}

/// A stretch the schedule plays: schedule frames `[start, start +
/// frames)` read media frames from `file_frame` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Need {
    /// The media.
    pub media: MediaId,
    /// First schedule frame.
    pub start: u64,
    /// Frames.
    pub frames: u64,
    /// Media frame at `start`.
    pub file_frame: u64,
}

impl Need {
    fn end(&self) -> u64 {
        self.start.saturating_add(self.frames)
    }
}

/// What a real-time read did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// The frames were copied.
    Done,
    /// Not ready (count it with [`StreamReader::count_miss`]).
    Miss,
    /// The media failed; play silence and count nothing.
    Offline,
}

/// Counters for the UI and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamCounters {
    /// Misses after the late-start window.
    pub underruns: u64,
    /// Misses within it.
    pub late: u64,
    /// Files in the RAM cache.
    pub cached_files: usize,
    /// Stream voices in use.
    pub stream_voices: usize,
}

struct Voice {
    media: AtomicU64,
    start: AtomicU64,
    origin: AtomicU64,
    base: AtomicU64,
    hi: AtomicU64,
    channels: AtomicU32,
    cap: u64,
    stride: usize,
    data: Box<[AtomicU32]>,
}

impl Voice {
    fn new(cap: u64, stride: usize) -> Voice {
        Voice {
            media: AtomicU64::new(0),
            start: AtomicU64::new(0),
            origin: AtomicU64::new(0),
            base: AtomicU64::new(0),
            hi: AtomicU64::new(0),
            channels: AtomicU32::new(1),
            cap: cap.max(1),
            stride,
            data: (0..cap.max(1) as usize * stride)
                .map(|_| AtomicU32::new(0))
                .collect(),
        }
    }

    /// The readable file frames.
    fn range(&self) -> (u64, u64) {
        let hi = self.hi.load(Ordering::Acquire);
        let lo = self
            .base
            .load(Ordering::Relaxed)
            .max(hi.saturating_sub(self.cap));
        (lo, hi)
    }

    fn put(&self, f: u64, ch: usize, x: f32) {
        let i = (f % self.cap) as usize * self.stride + ch;
        if let Some(c) = self.data.get(i) {
            c.store(x.to_bits(), Ordering::Relaxed);
        }
    }

    fn get(&self, f: u64, ch: usize) -> f32 {
        let i = (f % self.cap) as usize * self.stride + ch;
        self.data
            .get(i)
            .map_or(0.0, |c| f32::from_bits(c.load(Ordering::Relaxed)))
    }
}

/// What the control side tells the disk side.
#[derive(Default)]
struct Control {
    media: BTreeMap<MediaId, PathBuf>,
    needs: Arc<[Need]>,
    loop_range: Option<(u64, u64)>,
    hint: Option<u64>,
}

struct Shared {
    rate: f64,
    cfg: StreamConfig,
    manual: bool,
    slots: Box<[AtomicPtr<Voice>]>,
    epoch: AtomicU64,
    position: AtomicU64,
    underruns: AtomicU64,
    late: AtomicU64,
    failed: Box<[AtomicU64]>,
    control: Mutex<Control>,
    wake: Condvar,
    stop: AtomicBool,
    errors: Mutex<Vec<(MediaId, String)>>,
    retired: Mutex<Vec<(*mut Voice, u64)>>,
}

// SAFETY: the raw pointers in `slots` and `retired` come from
// `Box::into_raw` and are only freed by `Drop` or by the disk side after
// the epoch rule; every other access is through atomics.
unsafe impl Send for Shared {}
// SAFETY: as for `Send`.
unsafe impl Sync for Shared {}

impl Drop for Shared {
    fn drop(&mut self) {
        // The last owner: nothing reads any voice any more.
        for s in self.slots.iter() {
            let p = s.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !p.is_null() {
                // SAFETY: `p` came from `Box::into_raw` and is in no other
                // slot or list.
                drop(unsafe { Box::from_raw(p) });
            }
        }
        if let Ok(r) = self.retired.get_mut() {
            for (p, _) in r.drain(..) {
                // SAFETY: as above; retired voices left their slots.
                drop(unsafe { Box::from_raw(p) });
            }
        }
    }
}

impl Shared {
    fn slot(&self, i: usize) -> Option<&Voice> {
        let p = self.slots.get(i)?.load(Ordering::Acquire);
        // SAFETY: a published pointer stays valid until the epoch rule
        // (or `Drop`, which needs every reference gone) frees it.
        (!p.is_null()).then(|| unsafe { &*p })
    }

    fn is_failed(&self, media: MediaId) -> bool {
        let key = media.0.wrapping_add(1);
        self.failed
            .iter()
            .any(|f| f.load(Ordering::Relaxed) == key)
    }
}

/// The real-time handle.
#[derive(Clone)]
pub struct StreamReader {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for StreamReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamReader")
    }
}

impl StreamReader {
    /// A block starts at schedule frame `position` (the playhead the disk
    /// side plans around; pass the current position also while stopped).
    /// // RT
    pub fn begin_block(&self, position: u64) {
        self.shared.position.store(position, Ordering::Relaxed);
        self.shared.epoch.fetch_add(1, Ordering::AcqRel);
    }

    /// Copies `left.len()` frames of `media` from `file_frame` for the
    /// segment keyed `(start, origin)` (its need's `start` and
    /// `file_frame`) into `left`/`right` (mono files copy to both).
    /// Writes nothing on a miss. // RT
    pub fn read(
        &self,
        media: MediaId,
        start: u64,
        origin: u64,
        file_frame: u64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Read {
        let n = left.len().min(right.len()) as u64;
        let key = media.0.wrapping_add(1);
        let s = &self.shared;
        for i in 0..s.slots.len() {
            let Some(v) = s.slot(i) else {
                continue;
            };
            if v.media.load(Ordering::Acquire) != key {
                continue;
            }
            let (vs, vo) = (
                v.start.load(Ordering::Relaxed),
                v.origin.load(Ordering::Relaxed),
            );
            if !(vs == ANY || (vs == start && vo == origin)) {
                continue;
            }
            let (lo, hi) = v.range();
            if file_frame < lo || file_frame.saturating_add(n) > hi {
                continue;
            }
            let stereo = v.channels.load(Ordering::Relaxed) >= 2;
            for (k, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                let f = file_frame + k as u64;
                *l = v.get(f, 0);
                *r = if stereo { v.get(f, 1) } else { *l };
            }
            return Read::Done;
        }
        if s.is_failed(media) {
            Read::Offline
        } else {
            Read::Miss
        }
    }

    /// Counts a miss: a late start (`late`) or an underrun. // RT
    pub fn count_miss(&self, late: bool) {
        let c = if late {
            &self.shared.late
        } else {
            &self.shared.underruns
        };
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// Disk-side state, owned by the disk thread (or `service`'s caller).
#[derive(Default)]
struct Disk {
    readers: BTreeMap<MediaId, CafReader>,
    failed: BTreeSet<MediaId>,
    /// Stream slot → its need key and when it was retired (None: in use
    /// or never published).
    stream: BTreeMap<usize, Option<u64>>,
    /// Cached media → (slot, float bytes, last pass it was needed).
    cache: BTreeMap<MediaId, (usize, u64, u64)>,
    cache_bytes: u64,
    pass: u64,
    scratch: Vec<u8>,
    buf: Vec<f32>,
}

/// The control handle (see the module docs).
pub struct Streamer {
    shared: Arc<Shared>,
    disk: Arc<Mutex<Disk>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Streamer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Streamer")
            .field("rate", &self.shared.rate)
            .finish_non_exhaustive()
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Streamer {
    /// A streamer at `rate` (the project rate) and its real-time reader;
    /// with `DiskMode::Thread`, starts the disk thread.
    pub fn new(
        rate: f64,
        cfg: StreamConfig,
        mode: DiskMode,
    ) -> Result<(Streamer, StreamReader), MediaError> {
        let slots = cfg.stream_voices.min(STREAM_VOICES) + CACHE_VOICES;
        let shared = Arc::new(Shared {
            rate: rate.max(1.0),
            cfg,
            manual: mode == DiskMode::Manual,
            slots: (0..slots).map(|_| AtomicPtr::new(std::ptr::null_mut())).collect(),
            epoch: AtomicU64::new(0),
            position: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            late: AtomicU64::new(0),
            failed: (0..FAILED_SLOTS).map(|_| AtomicU64::new(0)).collect(),
            control: Mutex::new(Control::default()),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            errors: Mutex::new(Vec::new()),
            retired: Mutex::new(Vec::new()),
        });
        let disk = Arc::new(Mutex::new(Disk::default()));
        let thread = match mode {
            DiskMode::Manual => None,
            DiskMode::Thread => {
                let (s, d) = (Arc::clone(&shared), Arc::clone(&disk));
                Some(
                    std::thread::Builder::new()
                        .name("vizdaw-disk".into())
                        .spawn(move || {
                            while !s.stop.load(Ordering::Relaxed) {
                                pass(&s, &mut lock(&d));
                                let c = lock(&s.control);
                                let _ = s.wake.wait_timeout(c, Duration::from_millis(5));
                            }
                        })
                        .map_err(|e| MediaError::Io(format!("disk thread: {e}")))?,
                )
            }
        };
        let reader = StreamReader {
            shared: Arc::clone(&shared),
        };
        Ok((
            Streamer {
                shared,
                disk,
                thread,
            },
            reader,
        ))
    }

    /// The media that may be read (online items); others are never read.
    pub fn set_media(&self, files: Vec<MediaFile>) {
        let mut c = lock(&self.shared.control);
        c.media = files.into_iter().map(|f| (f.id, f.path)).collect();
        drop(c);
        self.shared.wake.notify_all();
    }

    /// What the loaded schedule plays.
    pub fn set_plan(&self, needs: Arc<[Need]>) {
        lock(&self.shared.control).needs = needs;
        self.shared.wake.notify_all();
    }

    /// The loop range, while looping.
    pub fn set_loop(&self, range: Option<(u64, u64)>) {
        lock(&self.shared.control).loop_range = range.filter(|(s, e)| e > s);
        self.shared.wake.notify_all();
    }

    /// Playback is about to start at (or move to) `frame`: plan around it
    /// now.
    pub fn hint(&self, frame: u64) {
        lock(&self.shared.control).hint = Some(frame);
        self.shared.wake.notify_all();
    }

    /// True when every need playing in `[frame, frame + ready_seconds)`
    /// has its frames (failed and unknown media count as ready).
    pub fn ready(&self, frame: u64) -> bool {
        let s = &self.shared;
        let c = lock(&s.control);
        let end = frame.saturating_add((s.cfg.ready_seconds * s.rate) as u64);
        c.needs
            .iter()
            .filter(|n| n.start < end && n.end() > frame)
            .filter(|n| c.media.contains_key(&n.media) && !s.is_failed(n.media))
            .all(|n| {
                let from = n.file_frame + (frame.max(n.start) - n.start);
                let to = n.file_frame + (end.min(n.end()) - n.start);
                (0..s.slots.len()).any(|i| {
                    s.slot(i).is_some_and(|v| {
                        let key_ok = v.media.load(Ordering::Acquire) == n.media.0.wrapping_add(1)
                            && (v.start.load(Ordering::Relaxed) == ANY
                                || (v.start.load(Ordering::Relaxed) == n.start
                                    && v.origin.load(Ordering::Relaxed) == n.file_frame));
                        let (lo, hi) = v.range();
                        key_ok && lo <= from && to <= hi
                    })
                })
            })
    }

    /// Hints `frame` and waits up to `timeout` for [`Streamer::ready`]
    /// (Manual mode runs passes itself). True when ready.
    pub fn wait_ready(&self, frame: u64, timeout: Duration) -> bool {
        self.hint(frame);
        let until = Instant::now() + timeout;
        loop {
            if self.shared.manual {
                self.service();
            }
            if self.ready(frame) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// One disk pass (what the disk thread runs).
    pub fn service(&self) {
        pass(&self.shared, &mut lock(&self.disk));
    }

    /// Counters since the streamer started.
    pub fn counters(&self) -> StreamCounters {
        let d = lock(&self.disk);
        StreamCounters {
            underruns: self.shared.underruns.load(Ordering::Relaxed),
            late: self.shared.late.load(Ordering::Relaxed),
            cached_files: d.cache.len(),
            stream_voices: d.stream.values().filter(|r| r.is_none()).count(),
        }
    }

    /// Media that failed since the last call, with why (each once).
    pub fn take_errors(&self) -> Vec<(MediaId, String)> {
        std::mem::take(&mut *lock(&self.shared.errors))
    }
}

/// The windows a pass fills, around playhead `p`.
fn windows(p: u64, lp: Option<(u64, u64)>, ahead: u64, seam: u64) -> Vec<(u64, u64)> {
    match lp {
        Some((ls, le)) if p < le => {
            let mut w = vec![(p, p.saturating_add(ahead).min(le))];
            let over = p.saturating_add(ahead).saturating_sub(le);
            if over > 0 {
                w.push((ls, ls.saturating_add(over).min(le)));
            }
            w.push((ls, ls.saturating_add(seam).min(le)));
            w
        }
        _ => vec![(p, p.saturating_add(ahead))],
    }
}

/// Marks `media` failed: reported once, then read as offline.
fn fail(s: &Shared, d: &mut Disk, media: MediaId, why: String) {
    if !d.failed.insert(media) {
        return;
    }
    d.readers.remove(&media);
    let key = media.0.wrapping_add(1);
    if let Some(f) = s.failed.iter().find(|f| f.load(Ordering::Relaxed) == 0) {
        f.store(key, Ordering::Relaxed);
    }
    lock(&s.errors).push((media, why));
}

/// Opens (once) the reader of `media`.
fn reader<'a>(
    s: &Shared,
    d: &'a mut Disk,
    media: MediaId,
    path: &std::path::Path,
) -> Option<&'a CafReader> {
    if !d.readers.contains_key(&media) {
        match CafReader::open(path) {
            Ok(r) => {
                d.readers.insert(media, r);
            }
            Err(e) => {
                fail(s, d, media, e.to_string());
                return None;
            }
        }
    }
    d.readers.get(&media)
}

/// Unpublishes slot `i`'s voice, noting the epoch (Manual: free to reuse
/// at once).
fn retire_stream(s: &Shared, d: &mut Disk, i: usize) {
    if let Some(v) = s.slot(i) {
        v.media.store(0, Ordering::Release);
    }
    let at = if s.manual {
        0
    } else {
        s.epoch.load(Ordering::Acquire)
    };
    d.stream.insert(i, Some(at));
}

/// A free stream slot (a never-used one gets a voice), or `None`.
fn free_stream_slot(s: &Shared, d: &mut Disk) -> Option<usize> {
    let now = s.epoch.load(Ordering::Acquire);
    let reusable = d
        .stream
        .iter()
        .find(|(_, r)| matches!(r, Some(e) if s.manual || now >= e + 2))
        .map(|(i, _)| *i);
    if reusable.is_some() {
        return reusable;
    }
    let max = s.cfg.stream_voices.min(STREAM_VOICES);
    let i = (0..max).find(|i| !d.stream.contains_key(i))?;
    let cap = (s.cfg.voice_seconds * s.rate).ceil().max(1.0) as u64;
    let p = Box::into_raw(Box::new(Voice::new(cap, 2)));
    s.slots.get(i)?.store(p, Ordering::Release);
    d.stream.insert(i, Some(0));
    Some(i)
}

/// Frees retired cache voices whose epoch has passed (all in Manual).
fn collect(s: &Shared) {
    let now = s.epoch.load(Ordering::Acquire);
    let mut r = lock(&s.retired);
    r.retain(|(p, e)| {
        if s.manual || now >= e + 2 {
            // SAFETY: `p` left its slot at epoch `e`; two block starts
            // since mean no reader still holds it (Ruling 9).
            drop(unsafe { Box::from_raw(*p) });
            false
        } else {
            true
        }
    });
}

/// Reads `n` frames of `media` from `from` into voice `v` at the same
/// frames; false (and the media failed) on an error or a short read.
fn fill(s: &Shared, d: &mut Disk, v: &Voice, media: MediaId, path: &std::path::Path, from: u64, n: u64) -> bool {
    let mut scratch = std::mem::take(&mut d.scratch);
    let mut buf = std::mem::take(&mut d.buf);
    let ok = (|| {
        let r = reader(s, d, media, path)?;
        let ch = usize::from(r.info().channels);
        v.channels.store(ch as u32, Ordering::Relaxed);
        let chunk = 16_384u64;
        let mut at = from;
        while at < from + n {
            let k = chunk.min(from + n - at);
            buf.resize(k as usize * ch, 0.0);
            let got = match r.read(at, &mut buf, &mut scratch) {
                Ok(g) => g as u64,
                Err(_) => return None,
            };
            for f in 0..got {
                for c in 0..ch.min(v.stride) {
                    let x = buf.get(f as usize * ch + c).copied().unwrap_or(0.0);
                    v.put(at + f, c, x);
                }
            }
            if got < k {
                return None;
            }
            at += k;
        }
        Some(())
    })()
    .is_some();
    d.scratch = scratch;
    d.buf = buf;
    if !ok && !d.failed.contains(&media) {
        fail(s, d, media, format!("{}: read failed or file shorter than expected", path.display()));
    }
    ok
}

/// Loads `media` whole into a cache voice if it is small; true when it is
/// (or already was) cached.
fn ensure_cached(s: &Shared, d: &mut Disk, media: MediaId, path: &std::path::Path) -> bool {
    if let Some(e) = d.cache.get_mut(&media) {
        e.2 = d.pass;
        return true;
    }
    let Some(r) = reader(s, d, media, path) else {
        return false;
    };
    let (frames, ch, fb) = (r.info().frames, usize::from(r.info().channels), r.info().frame_bytes() as u64);
    if frames == 0 || frames.saturating_mul(fb) > s.cfg.cache_file_bytes {
        return false;
    }
    let bytes = frames * ch as u64 * 4;
    if bytes > s.cfg.cache_budget_bytes {
        return false;
    }
    // Evict the least recently needed until it fits (never ones needed in
    // this pass).
    while d.cache_bytes + bytes > s.cfg.cache_budget_bytes {
        let Some((&m, &(slot, b, _))) = d
            .cache
            .iter()
            .filter(|(_, e)| e.2 < d.pass)
            .min_by_key(|(_, e)| e.2)
        else {
            return false;
        };
        evict(s, d, m, slot, b);
    }
    let first = s.cfg.stream_voices.min(STREAM_VOICES);
    let Some(slot) = (first..s.slots.len()).find(|i| s.slot(*i).is_none()) else {
        return false;
    };
    let v = Box::new(Voice::new(frames, ch));
    if !fill(s, d, &v, media, path, 0, frames) {
        return false;
    }
    v.base.store(0, Ordering::Relaxed);
    v.start.store(ANY, Ordering::Relaxed);
    v.origin.store(ANY, Ordering::Relaxed);
    v.hi.store(frames, Ordering::Release);
    v.media.store(media.0.wrapping_add(1), Ordering::Release);
    if let Some(cell) = s.slots.get(slot) {
        cell.store(Box::into_raw(v), Ordering::Release);
    }
    d.cache.insert(media, (slot, bytes, d.pass));
    d.cache_bytes += bytes;
    true
}

fn evict(s: &Shared, d: &mut Disk, media: MediaId, slot: usize, bytes: u64) {
    if let Some(cell) = s.slots.get(slot) {
        let p = cell.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !p.is_null() {
            lock(&s.retired).push((p, s.epoch.load(Ordering::Acquire)));
        }
    }
    d.cache.remove(&media);
    d.cache_bytes = d.cache_bytes.saturating_sub(bytes);
}

/// One disk pass (see the module docs).
fn pass(s: &Shared, d: &mut Disk) {
    d.pass += 1;
    collect(s);
    let (media, needs, lp, hint) = {
        let mut c = lock(&s.control);
        (c.media.clone(), Arc::clone(&c.needs), c.loop_range, c.hint.take())
    };
    // Media gone from the table leave the cache and the readers.
    let gone: Vec<(MediaId, usize, u64)> = d
        .cache
        .iter()
        .filter(|(m, _)| !media.contains_key(m))
        .map(|(m, e)| (*m, e.0, e.1))
        .collect();
    for (m, slot, b) in gone {
        evict(s, d, m, slot, b);
    }
    d.readers.retain(|m, _| media.contains_key(m));
    let p = hint.unwrap_or_else(|| s.position.load(Ordering::Relaxed));
    let rate = s.rate;
    let ahead = (s.cfg.ahead_seconds * rate) as u64;
    let seam = (s.cfg.seam_seconds * rate) as u64;
    let win = windows(p, lp, ahead, seam);
    // Stream slots used this pass, with the file frame to fill up to.
    let mut used: BTreeMap<usize, (Need, u64)> = BTreeMap::new();
    for n in needs.iter() {
        let Some(path) = media.get(&n.media) else {
            continue;
        };
        if d.failed.contains(&n.media) {
            continue;
        }
        let mut wanted = Vec::new();
        for &(a, b) in &win {
            let (from, to) = (a.max(n.start), b.min(n.end()));
            if from < to {
                wanted.push((n.file_frame + (from - n.start), n.file_frame + (to - n.start)));
            }
        }
        if wanted.is_empty() {
            continue;
        }
        if ensure_cached(s, d, n.media, path) {
            continue;
        }
        if d.failed.contains(&n.media) {
            continue;
        }
        for (want, until) in wanted {
            let key = n.media.0.wrapping_add(1);
            let found = d.stream.iter().find_map(|(&i, r)| {
                let v = s.slot(i)?;
                let live = r.is_none()
                    && v.media.load(Ordering::Acquire) == key
                    && v.start.load(Ordering::Relaxed) == n.start
                    && v.origin.load(Ordering::Relaxed) == n.file_frame;
                let (lo, hi) = v.range();
                (live && lo <= want && want <= hi).then_some(i)
            });
            let slot = match found {
                Some(i) => i,
                None => {
                    let Some(i) = free_stream_slot(s, d) else {
                        continue;
                    };
                    if let Some(v) = s.slot(i) {
                        v.start.store(n.start, Ordering::Relaxed);
                        v.origin.store(n.file_frame, Ordering::Relaxed);
                        v.base.store(want, Ordering::Relaxed);
                        v.hi.store(want, Ordering::Release);
                        v.media.store(key, Ordering::Release);
                    }
                    d.stream.insert(i, None);
                    i
                }
            };
            let e = used.entry(slot).or_insert((*n, until));
            e.1 = e.1.max(until);
        }
    }
    // Retire stream voices no window used.
    let idle: Vec<usize> = d
        .stream
        .iter()
        .filter(|(i, r)| r.is_none() && !used.contains_key(i))
        .map(|(i, _)| *i)
        .collect();
    for i in idle {
        retire_stream(s, d, i);
    }
    // Fill the used ones, nearest deadline first.
    let mut order: Vec<(usize, Need, u64)> = used.into_iter().map(|(i, (n, u))| (i, n, u)).collect();
    order.sort_by_key(|(i, n, _)| {
        let hi = s.slot(*i).map_or(0, |v| v.range().1);
        n.start + hi.saturating_sub(n.file_frame)
    });
    for (i, n, until) in order {
        let Some(v) = s.slot(i) else {
            continue;
        };
        let Some(path) = media.get(&n.media).cloned() else {
            continue;
        };
        let (base, hi) = (v.base.load(Ordering::Relaxed), v.hi.load(Ordering::Acquire));
        let under = (p >= n.start && p < n.end()).then(|| n.file_frame + (p - n.start));
        let floor = match under {
            Some(f) if f >= base && f <= hi => f,
            _ => base,
        };
        let space = (floor + v.cap).saturating_sub(hi);
        let count = space.min(until.saturating_sub(hi));
        let fb = 4 * v.stride as u64;
        let min = (s.cfg.min_read_bytes / fb).max(1);
        if count == 0 || (count < min && hi + count < until) {
            continue;
        }
        if fill(s, d, v, n.media, &path, hi, count) {
            v.hi.store(hi + count, Ordering::Release);
        } else {
            retire_stream(s, d, i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caf::{CafWriter, WriteMode};
    use core_model::format::BitDepth;
    use proptest::prelude::*;

    /// Frame f, channel c of the test files.
    fn sample(f: u64, c: usize) -> f32 {
        ((f % 100_000) as f32) * 1e-5 + c as f32 * 0.5 - 0.5
    }

    fn file(dir: &std::path::Path, name: &str, frames: u64, ch: u16) -> PathBuf {
        let p = dir.join(name);
        let mut w = CafWriter::create(&p, 48_000, ch, BitDepth::Float32, WriteMode::Normal).unwrap();
        let data: Vec<f32> = (0..frames)
            .flat_map(|f| (0..usize::from(ch)).map(move |c| sample(f, c)))
            .collect();
        w.write(&data).unwrap();
        w.finish().unwrap();
        p
    }

    /// Small sizes so tests stream small files: 0.1 s voices, 0.2 s ahead.
    fn small() -> StreamConfig {
        StreamConfig {
            voice_seconds: 0.1,
            ahead_seconds: 0.2,
            seam_seconds: 0.05,
            ready_seconds: 0.01,
            cache_file_bytes: 0,
            cache_budget_bytes: 1 << 30,
            min_read_bytes: 4096,
            stream_voices: 8,
        }
    }

    fn need(media: u64, start: u64, frames: u64, file_frame: u64) -> Need {
        Need {
            media: MediaId(media),
            start,
            frames,
            file_frame,
        }
    }

    /// Reads `n` frames at `file_frame` of segment `n0`; checks them when
    /// done.
    fn read(r: &StreamReader, n0: &Need, file_frame: u64, n: usize, stereo: bool) -> Read {
        let (mut l, mut rr) = (vec![9.0; n], vec![9.0; n]);
        let got = r.read(n0.media, n0.start, n0.file_frame, file_frame, &mut l, &mut rr);
        if got == Read::Done {
            for k in 0..n {
                let f = file_frame + k as u64;
                assert_eq!(l[k], sample(f, 0), "left at {f}");
                assert_eq!(rr[k], sample(f, if stereo { 1 } else { 0 }), "right at {f}");
            }
        } else {
            assert!(l.iter().all(|x| *x == 9.0), "a miss wrote");
        }
        got
    }

    fn manual(cfg: StreamConfig) -> (Streamer, StreamReader) {
        Streamer::new(48_000.0, cfg, DiskMode::Manual).unwrap()
    }

    #[test]
    fn a_planned_need_is_streamed_and_read_back_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 96_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile { id: MediaId(0), path: p }]);
        let n = need(0, 1000, 90_000, 500);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(1000);
        assert_eq!(read(&r, &n, 500, 256, true), Read::Miss);
        s.service();
        assert_eq!(read(&r, &n, 500, 256, true), Read::Done);
        // The key matters: the same media for another segment misses.
        let other = need(0, 2000, 10, 500);
        assert_eq!(read(&r, &other, 500, 4, true), Read::Miss);
        // Beyond the ring (0.1 s = 4800 frames).
        assert_eq!(read(&r, &n, 500 + 6000, 4, true), Read::Miss);
        assert_eq!(s.counters().stream_voices, 1);
    }

    #[test]
    fn playback_streams_every_frame_without_tearing() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 200_000, 1);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile { id: MediaId(3), path: p }]);
        let n = need(3, 0, 200_000, 0);
        s.set_plan(Arc::from(vec![n]));
        let mut pos = 0u64;
        while pos + 512 <= 200_000 {
            r.begin_block(pos);
            s.service();
            assert_eq!(read(&r, &n, pos, 512, false), Read::Done, "at {pos}");
            pos += 512;
        }
    }

    #[test]
    fn small_files_are_cached_whole() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "s.caf", 20_000, 2);
        let mut cfg = small();
        cfg.cache_file_bytes = 4 << 20;
        let (s, r) = manual(cfg);
        s.set_media(vec![MediaFile { id: MediaId(1), path: p }]);
        let n = need(1, 0, 20_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(0);
        s.service();
        // Far from the playhead, any segment of the media.
        assert_eq!(read(&r, &n, 19_000, 1000, true), Read::Done);
        assert_eq!(read(&r, &need(1, 7, 7, 7), 3, 10, true), Read::Done);
        assert_eq!(s.counters().cached_files, 1);
        // Media leaving the table leave the cache.
        s.set_media(vec![]);
        s.service();
        assert_eq!(s.counters().cached_files, 0);
        assert_eq!(read(&r, &n, 0, 10, true), Read::Miss);
    }

    #[test]
    fn the_cache_budget_evicts_the_least_recently_needed() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = small();
        cfg.cache_file_bytes = 4 << 20;
        // Room for two 10 000-frame stereo files (80 000 float bytes each).
        cfg.cache_budget_bytes = 170_000;
        let (s, r) = manual(cfg);
        let files: Vec<MediaFile> = (0..3)
            .map(|i| MediaFile {
                id: MediaId(i),
                path: file(dir.path(), &format!("{i}.caf"), 10_000, 2),
            })
            .collect();
        s.set_media(files);
        for i in 0..3u64 {
            s.set_plan(Arc::from(vec![need(i, 0, 10_000, 0)]));
            r.begin_block(0);
            s.service();
        }
        assert_eq!(s.counters().cached_files, 2);
        assert_eq!(read(&r, &need(0, 0, 10_000, 0), 0, 10, true), Read::Miss);
        assert_eq!(read(&r, &need(2, 0, 10_000, 0), 0, 10, true), Read::Done);
    }

    #[test]
    fn locate_fills_from_the_new_position() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile { id: MediaId(0), path: p }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(0);
        s.service();
        assert_eq!(read(&r, &n, 250_000, 64, true), Read::Miss);
        s.hint(250_000);
        s.service();
        r.begin_block(250_000);
        assert_eq!(read(&r, &n, 250_000, 64, true), Read::Done);
        assert!(s.wait_ready(250_000, Duration::from_millis(50)));
    }

    #[test]
    fn the_loop_seam_is_kept_prefilled() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 1);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile { id: MediaId(0), path: p }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        s.set_loop(Some((10_000, 200_000)));
        // Near the loop end: the seam at the loop start is ready too.
        r.begin_block(199_000);
        s.service();
        assert_eq!(read(&r, &n, 199_000, 512, false), Read::Done);
        assert_eq!(read(&r, &n, 10_000, 512, false), Read::Done);
        // After the wrap, the seam voice plays on.
        let mut pos = 10_000;
        for _ in 0..40 {
            r.begin_block(pos);
            s.service();
            assert_eq!(read(&r, &n, pos, 512, false), Read::Done, "at {pos}");
            pos += 512;
        }
    }

    #[test]
    fn a_file_removed_mid_play_goes_offline_and_plays_silence() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile { id: MediaId(5), path: p.clone() }]);
        let n = need(5, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(0);
        s.service();
        assert_eq!(read(&r, &n, 0, 64, true), Read::Done);
        // Truncate it to 1000 frames under the open reader.
        let off = CafReader::open(&p).unwrap().info().data_offset;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .unwrap()
            .set_len(off + 1000 * 8)
            .unwrap();
        let mut pos = 0;
        for _ in 0..100 {
            r.begin_block(pos);
            s.service();
            pos += 512;
        }
        let errors = s.take_errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].0, MediaId(5));
        assert!(s.take_errors().is_empty(), "reported once");
        assert_eq!(read(&r, &n, 40_000, 64, true), Read::Offline);
        assert_eq!(s.counters().underruns, 0);
    }

    #[test]
    fn a_starved_disk_misses_and_counts_only_what_it_is_told() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 100_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile { id: MediaId(0), path: p }]);
        let n = need(0, 0, 100_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(0);
        assert_eq!(read(&r, &n, 0, 64, true), Read::Miss);
        r.count_miss(false);
        r.count_miss(true);
        let c = s.counters();
        assert_eq!((c.underruns, c.late), (1, 1));
    }

    #[test]
    fn the_disk_thread_gets_ready() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 2);
        let (s, r) = Streamer::new(48_000.0, small(), DiskMode::Thread).unwrap();
        s.set_media(vec![MediaFile { id: MediaId(0), path: p }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(100_000);
        assert!(s.wait_ready(100_000, Duration::from_secs(5)));
        assert_eq!(read(&r, &n, 100_000, 256, true), Read::Done);
        drop(s);
        // The reader outlives the streamer safely.
        assert_ne!(read(&r, &n, 100_000, 8, true), Read::Offline);
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 24, .. ProptestConfig::default() })]

        /// Any layout, any playhead path (forward steps, jumps), any
        /// service pattern: a read is exact or misses, never garbage.
        #[test]
        fn reads_are_exact_or_miss(
            segs in prop::collection::vec((0u64..50_000, 1u64..40_000, 0u64..60_000), 1..4),
            steps in prop::collection::vec((0u8..10, 1u64..2048), 1..60),
            looped in any::<bool>(),
        ) {
            let dir = tempfile::tempdir().unwrap();
            let p = file(dir.path(), "a.caf", 100_000, 2);
            let (s, r) = manual(small());
            s.set_media(vec![MediaFile { id: MediaId(0), path: p }]);
            let needs: Vec<Need> = segs
                .iter()
                .map(|&(start, frames, ff)| need(0, start, frames.min(100_000 - ff.min(99_999)), ff.min(99_999)))
                .collect();
            s.set_plan(Arc::from(needs.clone()));
            if looped {
                s.set_loop(Some((5_000, 45_000)));
            }
            let mut pos = 0u64;
            for (what, n) in steps {
                match what {
                    0 => pos = n * 37 % 90_000,
                    1 => {}
                    _ => s.service(),
                }
                r.begin_block(pos);
                for nd in &needs {
                    if pos >= nd.start && pos < nd.start + nd.frames {
                        let f = nd.file_frame + (pos - nd.start);
                        let k = (n as usize).min((nd.start + nd.frames - pos) as usize);
                        let got = read(&r, nd, f, k, true);
                        prop_assert!(got != Read::Offline);
                    }
                }
                pos += n;
            }
        }
    }
}
VIZDAW_EOF
```

Notes for the implementer:
- `pass` holds the `Disk` lock throughout and the `Control` lock only to copy the plan. The real-time side holds neither.
- `fill` writes into a voice the real-time side may be reading. That is sound because each element is atomic, and because of the overwrite rule in the module doc. Keep both properties if you restructure.
- Several functions take `&Shared` and `&mut Disk` together. If the borrow checker objects to `reader(s, d, …)` returning a reference into `d` while `d` is used again, copy what you need out of the reader first (`info`), as `ensure_cached` does, or re-borrow inside a block.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-media 2>&1 | tail -5`
Expected: `test result: ok. 24 passed` (14 earlier + 10 stream, the proptest counted once).

Run the proptest harder once: `PROPTEST_CASES=200 cargo test -p core-media reads_are_exact_or_miss --release 2>&1 | tail -3`
Expected: ok.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-media
git commit -m "core-media: disk streamer with lock-free voices and a RAM cache

A disk thread fills 2 s rings for the regions within 3 s of the playhead
(and the 2 s after the loop start while looping); the real-time reader
finds a voice by segment key and range and only reads. Voices are reused
or freed only after two block epochs. Small files are cached whole
within a budget; a failed file is reported once and reads as offline.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 7: `core-engine` — region segments in the schedule

**Files:**
- Create: `crates/core-engine/src/schedule/audio.rs`, `crates/core-engine/tests/schedule_audio.rs`
- Modify: `crates/core-engine/Cargo.toml`, `crates/core-engine/src/schedule/mod.rs`, `crates/core-engine/src/schedule/compile.rs`, `crates/core-engine/src/fixtures.rs`, `crates/core-engine/src/engine.rs` (the `CompileOptions` literal), `crates/core-engine/tests/schedule.rs` (four `CompileOptions` literals), `crates/core-ffi/src/host.rs` (`report_lines`, if it names `skipped_audio`)
- Test: `crates/core-engine/tests/schedule_audio.rs`

**Interfaces:**
- Consumes:
  - Task 1's `Fade { length_frames, shape }` and Task 2's `MediaCmd::Add`;
  - `core_model::query::{flatten, ResolvedEvent, PathStep}`;
  - Task 6's `core_media::stream::Need`.
- Produces:
  - `core_engine::schedule::{Segment, AudioSchedule, fade_curve}`:
    - `Segment { start: u64, frames: u64, media: MediaId, file_frame: u64, gain: f32, region_offset: u64, region_frames: u64, fade_in: Fade, fade_out: Fade, online: bool }`, with `end()` and `gain_at(file_frame) -> f32` (`// RT`);
    - `fade_curve(shape, x: f32) -> f32` (`// RT`);
    - `AudioSchedule::{is_empty, segments(path) -> &[Segment] (// RT), paths() -> impl Iterator<Item = (OutputPathId, &[Segment])>, overlapping(path, from, to) -> impl Iterator<Item = &Segment> (// RT), needs() -> Vec<Need>, segment_count()}`.
  - `Schedule.audio: AudioSchedule`; `CompileOptions.offline_media: BTreeSet<MediaId>`.
  - `CompileReport` loses `skipped_audio` and gains `audio_regions`, `audio_segments`, `audio_unrouted` and `audio_offline`.
  - Fixtures: `Builder::{media(path, frames, channels) -> Result<MediaId>, region(seq, lane, tick, media, offset, length, gain_db, fade_in, fade_out) -> Result<EventId>}`.

This task implements spec §5.3 and Ruling 13.
- **Grouping.** Flattened regions are grouped by *lane instance*: the path's reference steps plus the final lane.
- **Overlap rule (parent §5).** At any frame, the region with the latest start among those covering it plays. Equal starts go to the one later in flatten order. An earlier, longer region resumes after a later one ends.
- **Pieces.** A piece of a region is a `Segment` whose `file_frame` is the media frame at its start.
- **Fades.** Fades are evaluated against the region as written (`region_offset` = its `source_offset`, `region_frames` = its full length from the source event), so a cut piece keeps the right curve and a cut is hard.
- **Frames.** Start ticks map to frames with the top-level tempo (the nested timing rule). A region starting before frame 0 (a pickup) loses its first part.
- **Silence.** Regions on lanes without an output path are reported unrouted and skipped. Regions whose media is in `offline_media` become offline segments, which play silence and count nothing.

- [ ] **Step 1: Write the failing tests**

In `crates/core-engine/Cargo.toml`:

Find:
```toml
core-model = { path = "../core-model" }
crossbeam-queue = "0.3"
```
Replace:
```toml
core-media = { path = "../core-media" }
core-model = { path = "../core-model" }
crossbeam-queue = "0.3"
```

Add the fixture helpers to `crates/core-engine/src/fixtures.rs` (inside `impl Builder`, after `event`):

```rust
    /// Adds a media item `path` of `frames` frames at 48 kHz.
    pub fn media(&mut self, path: &str, frames: u64, channels: u16) -> Result<MediaId> {
        let r = self.run(Command::Media(MediaCmd::Add {
            path: path.into(),
            sample_rate: 48_000,
            channels,
            length_frames: frames,
            content_hash: None,
            name: None,
            origin: None,
        }))?;
        created(&r, |id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
    }

    /// Inserts an audio region (the lane gets an output path if it has
    /// none, 5a Ruling 6).
    #[allow(clippy::too_many_arguments)]
    pub fn region(
        &mut self,
        seq: SeqId,
        lane: LaneId,
        tick: i64,
        media: MediaId,
        offset: u64,
        length: u64,
        gain_db: f32,
        fade_in: Fade,
        fade_out: Fade,
    ) -> Result<EventId> {
        let r = self.run(Command::Audio(AudioCmd::Insert {
            seq,
            lane,
            tick: Tick(tick),
            media,
            source_offset: offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        }))?;
        created(&r, |id| match id {
            AnyId::Event(e) => Some(*e),
            _ => None,
        })
    }
```
Add `AudioCmd` to its `core_model::command` import and `MediaId` to its `core_model::ids` import.

Create `crates/core-engine/tests/schedule_audio.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/schedule_audio.rs <<'VIZDAW_EOF'
//! Audio regions in the schedule (plan 5a Task 7): segments per output
//! path, the overlap rule, loops, references, fades and routing.

use core_engine::fixtures::{BAR, Builder, Q};
use core_engine::schedule::{
    AudioSchedule, CompileOptions, CompileReport, Schedule, Segment, TargetKind, TargetTable,
    compile, fade_curve,
};
use core_model::command::{Command, LaneCmd};
use core_model::document::Document;
use core_model::event::{Fade, FadeShape};
use core_model::ids::{LaneId, MediaId, OutputPathId, SeqId};
use core_model::time::Tick;
use std::collections::BTreeSet;

/// 120 BPM at 48 kHz: a quarter is 24 000 frames.
const QF: u64 = 24_000;

fn none() -> Fade {
    Fade::default()
}

fn fade(n: u64, shape: FadeShape) -> Fade {
    Fade {
        length_frames: n,
        shape,
    }
}

fn run(doc: &Document, seq: SeqId, offline: &[u64]) -> (Schedule, CompileReport) {
    let mut opts = CompileOptions::new(48_000.0);
    opts.offline_media = offline.iter().map(|m| MediaId(*m)).collect::<BTreeSet<_>>();
    let mut table = TargetTable::new();
    compile(doc, seq, &opts, &mut |k: &TargetKind| table.intern(k.clone()).map(Ok)).unwrap()
}

/// `(start, frames, media, file_frame)` of `path`'s segments.
fn segs(a: &AudioSchedule, path: u64) -> Vec<(u64, u64, u64, u64)> {
    a.segments(OutputPathId(path))
        .iter()
        .map(|s| (s.start, s.frames, s.media.0, s.file_frame))
        .collect()
}

/// One sequence with one lane and two 10 s media items (0 and 1).
fn base() -> (Builder, SeqId, LaneId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    b.media("Media/a.caf", 480_000, 2).unwrap();
    b.media("Media/b.caf", 480_000, 1).unwrap();
    (b, seq, lane)
}

#[test]
fn a_region_becomes_one_segment_on_its_output_path() {
    let (mut b, seq, lane) = base();
    b.region(seq, lane, Q, MediaId(0), 1000, 48_000, -6.0, none(), none())
        .unwrap();
    let (s, r) = run(b.doc(), seq, &[]);
    assert_eq!(segs(&s.audio, 0), vec![(QF, 48_000, 0, 1000)]);
    let g = s.audio.segments(OutputPathId(0))[0].gain;
    assert!((g - 0.501_187).abs() < 1e-5, "{g}");
    assert_eq!((r.audio_regions, r.audio_segments, r.audio_unrouted), (1, 1, 0));
    assert!(r.warnings.iter().all(|w| !w.contains("audio")), "{:?}", r.warnings);
    let needs = s.audio.needs();
    assert_eq!(needs.len(), 1);
    assert_eq!((needs[0].start, needs[0].frames, needs[0].file_frame), (QF, 48_000, 1000));
}

#[test]
fn later_start_takes_over_and_earlier_resumes() {
    let (mut b, seq, lane) = base();
    // A: beat 0, 4 beats; B: beat 1, 1 beat. A plays 0-1, B 1-2, A again 2-4.
    b.region(seq, lane, 0, MediaId(0), 0, 4 * QF, 0.0, none(), none()).unwrap();
    b.region(seq, lane, Q, MediaId(1), 500, QF, 0.0, none(), none()).unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    assert_eq!(
        segs(&s.audio, 0),
        vec![(0, QF, 0, 0), (QF, QF, 1, 500), (2 * QF, 2 * QF, 0, 2 * QF)]
    );
}

#[test]
fn equal_starts_go_to_the_later_region() {
    let (mut b, seq, lane) = base();
    b.region(seq, lane, 0, MediaId(0), 0, QF, 0.0, none(), none()).unwrap();
    b.region(seq, lane, 0, MediaId(1), 0, QF / 2, 0.0, none(), none()).unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    assert_eq!(segs(&s.audio, 0), vec![(0, QF / 2, 1, 0), (QF / 2, QF / 2, 0, QF / 2)]);
}

#[test]
fn two_lanes_overlap_freely() {
    let (mut b, seq, lane) = base();
    let other = b.lane(seq, "Two", None).unwrap();
    b.region(seq, lane, 0, MediaId(0), 0, 2 * QF, 0.0, none(), none()).unwrap();
    b.region(seq, other, 0, MediaId(1), 0, 2 * QF, 0.0, none(), none()).unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    // Both lanes took output path 0: two overlapping segments there.
    assert_eq!(segs(&s.audio, 0), vec![(0, 2 * QF, 0, 0), (0, 2 * QF, 1, 0)]);
    let hits: Vec<u64> = s
        .audio
        .overlapping(OutputPathId(0), QF, QF + 10)
        .map(|g| g.media.0)
        .collect();
    assert_eq!(hits, vec![0, 1]);
    assert_eq!(s.audio.overlapping(OutputPathId(0), 2 * QF, 3 * QF).count(), 0);
}

#[test]
fn looped_lane_repeats_regions_and_cuts_at_its_length() {
    let (mut b, seq, lane) = base();
    // A 3-beat region in a 2-beat looped lane, in a 4-bar sequence.
    b.region(seq, lane, 0, MediaId(0), 0, 3 * QF, 0.0, none(), none()).unwrap();
    b.run(Command::Lane(LaneCmd::SetLength {
        seq,
        lane,
        length: Some(Tick(2 * Q)),
        looped: true,
        locked: false,
    }))
    .unwrap();
    let mut b2 = b.clone();
    let filler = b2.lane(seq, "Len", None).unwrap();
    b2.notes(seq, filler, &[(4 * BAR - Q, 60, 100, Q)]).unwrap();
    let (s, _) = run(b2.doc(), seq, &[]);
    let got = segs(&s.audio, 0);
    assert_eq!(got.len(), 8, "{got:?}");
    for (k, g) in got.iter().enumerate() {
        assert_eq!(*g, (k as u64 * 2 * QF, 2 * QF, 0, 0));
    }
}

#[test]
fn references_place_their_regions_in_the_parent_tempo() {
    let mut b = Builder::new();
    let (child, cl) = b.sequence("Loop", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", 480_000, 2).unwrap();
    b.region(child, cl, 0, m, 0, QF, 0.0, none(), none()).unwrap();
    let (song, sl) = b.sequence("Song", "Refs", None).unwrap();
    b.reference(song, sl, BAR, child, 2, 0).unwrap();
    let (s, _) = run(b.doc(), song, &[]);
    // The child is one beat long (its region); two uses from bar 2.
    assert_eq!(segs(&s.audio, 0), vec![(4 * QF, QF, 0, 0), (5 * QF, QF, 0, 0)]);
}

#[test]
fn lanes_without_an_output_and_offline_media() {
    let (mut b, seq, lane) = base();
    b.region(seq, lane, 0, MediaId(0), 0, QF, 0.0, none(), none()).unwrap();
    b.region(seq, lane, Q, MediaId(1), 0, QF, 0.0, none(), none()).unwrap();
    let (s, r) = run(b.doc(), seq, &[1]);
    let online: Vec<bool> = s.audio.segments(OutputPathId(0)).iter().map(|g| g.online).collect();
    assert_eq!(online, vec![true, false]);
    assert_eq!(r.audio_offline, 1);
    assert_eq!(s.audio.needs().len(), 1);
    assert!(r.warnings.iter().any(|w| w.contains("1 audio region(s) play silence")));
    // Clear the lane's output: nothing plays, and it says so.
    b.run(Command::Lane(LaneCmd::SetOutput { seq, lane, output: None })).unwrap();
    let (s, r) = run(b.doc(), seq, &[]);
    assert!(s.audio.is_empty());
    assert_eq!(r.audio_unrouted, 2);
    assert!(r.warnings.iter().any(|w| w.contains("no output path")));
}

#[test]
fn fades_are_relative_to_the_region_as_written() {
    let (mut b, seq, lane) = base();
    b.region(
        seq,
        lane,
        0,
        MediaId(0),
        100,
        4 * QF,
        0.0,
        fade(1000, FadeShape::Linear),
        fade(2000, FadeShape::Linear),
    )
    .unwrap();
    // A later region cuts the first one in the middle; the resumed piece
    // still fades out at the region's own end.
    b.region(seq, lane, Q, MediaId(1), 0, QF, 0.0, none(), none()).unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    let all = s.audio.segments(OutputPathId(0));
    let (first, last) = (all[0], all[2]);
    assert_eq!(first.region_offset, 100);
    assert_eq!(first.gain_at(100), 0.0);
    assert!((first.gain_at(100 + 500) - 0.5).abs() < 1e-6);
    assert_eq!(first.gain_at(100 + 1000), 1.0);
    // The region's last frame is 100 + 4 QF - 1: 1/2000 of the way in.
    let end = 100 + 4 * QF;
    assert!((last.gain_at(end - 1) - 1.0 / 2000.0).abs() < 1e-6);
    assert!((last.gain_at(end - 1000) - 0.5).abs() < 1e-6);
    assert_eq!(last.gain_at(end - 2001), 1.0);
}

#[test]
fn fade_curves_at_their_ends_and_middle() {
    for shape in [FadeShape::Linear, FadeShape::EqualPower, FadeShape::Exponential, FadeShape::SCurve] {
        assert_eq!(fade_curve(shape, 0.0), 0.0, "{shape:?}");
        assert!((fade_curve(shape, 1.0) - 1.0).abs() < 1e-6, "{shape:?}");
        // Monotonic.
        let mut last = 0.0;
        for k in 1..=100 {
            let v = fade_curve(shape, k as f32 / 100.0);
            assert!(v >= last, "{shape:?} at {k}");
            last = v;
        }
    }
    assert!((fade_curve(FadeShape::Linear, 0.25) - 0.25).abs() < 1e-6);
    // Equal power: sin(π/4) at the middle.
    assert!((fade_curve(FadeShape::EqualPower, 0.5) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    // Exponential over 60 dB: -30 dB (minus the floor) at the middle.
    let mid = fade_curve(FadeShape::Exponential, 0.5);
    assert!((mid - (0.031_622_78 - 0.001) / 0.999).abs() < 1e-5, "{mid}");
    // The S-curve is symmetric about the middle.
    let a = fade_curve(FadeShape::SCurve, 0.3);
    let b = fade_curve(FadeShape::SCurve, 0.7);
    assert!((a * a + b * b - 1.0).abs() < 1e-5);
    // Out of range clamps.
    assert_eq!(fade_curve(FadeShape::Linear, -1.0), 0.0);
    assert_eq!(fade_curve(FadeShape::Linear, 2.0), 1.0);
}

#[test]
fn a_pickup_region_plays_from_frame_zero() {
    let (mut b, seq, lane) = base();
    b.run(Command::Sequence(core_model::command::SequenceCmd::SetStartOffset {
        seq,
        offset: Tick(Q),
    }))
    .unwrap();
    // Schedule frame 0 is the pickup start, one beat before bar 1.
    b.region(seq, lane, -Q, MediaId(0), 0, 3 * QF, 0.0, none(), none()).unwrap();
    b.region(seq, lane, 2 * Q, MediaId(1), 0, QF, 0.0, none(), none()).unwrap();
    let (s, _) = run(b.doc(), seq, &[]);
    assert_eq!(
        segs(&s.audio, 0),
        vec![(0, 3 * QF, 0, 0), (3 * QF, QF, 1, 0)]
    );
    // A region starting before the playing window does not play (flatten
    // keeps only events inside it); `build`'s cut of negative frames is
    // the guard for rounding at the window's start.
    let mut b2 = b.clone();
    b2.region(seq, lane, -2 * Q, MediaId(1), 0, 4 * QF, 0.0, none(), none()).unwrap();
    let (s2, _) = run(b2.doc(), seq, &[]);
    assert_eq!(segs(&s2.audio, 0), segs(&s.audio, 0));
}

#[test]
fn segment_gain_combines_fades() {
    let g = Segment {
        start: 0,
        frames: 10,
        media: MediaId(0),
        file_frame: 0,
        gain: 0.5,
        region_offset: 0,
        region_frames: 10,
        fade_in: fade(10, FadeShape::Linear),
        fade_out: fade(10, FadeShape::Linear),
        online: true,
    };
    // In at 0.5, out (5 frames left of 10) at 0.5: 0.5 × 0.5 × 0.5.
    assert!((g.gain_at(5) - 0.125).abs() < 1e-6);
}
VIZDAW_EOF
```

Check two things against the tree before running.
- **`LaneCmd::SetLength`'s field names.** Use `grep -n "SetLength" -A12 crates/core-model/src/command/lane.rs`. If they differ (`length`, `looped`, `locked`), adapt the test.
- **The start-offset command.** Use `grep -n "StartOffset" crates/core-model/src/command/sequence.rs`. If its variant has another name, adapt the test.
  - In `looped_lane_repeats…` the extra note on a second lane gives the sequence its 4-bar length.
  - In `a_pickup…` the sequence's playing window starts at `-start_offset` = −1 beat, and schedule frame 0 is that point.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-engine --test schedule_audio 2>&1 | tail -5`
Expected: compile errors (`AudioSchedule`, `fade_curve`, `Builder::media` not found).

- [ ] **Step 3: Implement**

Create `crates/core-engine/src/schedule/audio.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/schedule/audio.rs <<'VIZDAW_EOF'
//! Audio regions in the schedule (sub-project 5 spec §5.3; plan 5a Ruling
//! 13). Flattened regions are grouped by lane instance (the reference
//! steps of their path plus their lane); within one, the parent overlap
//! rule decides who plays: at any frame the covering region with the
//! latest start (ties: the later in flatten order), so an earlier, longer
//! region resumes after a later one ends. Each piece is a [`Segment`].
//! Fades are evaluated against the region as written, so a cut piece
//! keeps its curve and the cut itself is hard.

use super::compile::CompileReport;
use core_media::stream::Need;
use core_model::document::Document;
use core_model::event::{EventBody, Fade, FadeShape};
use core_model::ids::{EventId, LaneId, MediaId, OutputPathId, SeqId};
use core_model::mixer::db_to_gain;
use core_model::query::ResolvedEvent;
use core_model::time::Tick;
use std::collections::{BTreeMap, BTreeSet};

/// One stretch of a region an output path plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// First schedule frame.
    pub start: u64,
    /// Frames (≥ 1).
    pub frames: u64,
    /// The media.
    pub media: MediaId,
    /// Media frame at `start`.
    pub file_frame: u64,
    /// The region's gain, linear.
    pub gain: f32,
    /// Media frame where the region as written starts (its source
    /// offset).
    pub region_offset: u64,
    /// The region's written length.
    pub region_frames: u64,
    /// Fade in.
    pub fade_in: Fade,
    /// Fade out.
    pub fade_out: Fade,
    /// The media is readable (else silence, nothing counted).
    pub online: bool,
}

impl Segment {
    /// One past the last schedule frame.
    pub fn end(&self) -> u64 {
        self.start.saturating_add(self.frames)
    }

    /// Gain × fade in × fade out at media frame `f`. // RT
    pub fn gain_at(&self, f: u64) -> f32 {
        let r = f.saturating_sub(self.region_offset);
        let mut g = self.gain;
        let fi = self.fade_in.length_frames;
        if fi > 0 && r < fi {
            g *= fade_curve(self.fade_in.shape, r as f32 / fi as f32);
        }
        let fo = self.fade_out.length_frames;
        let left = self.region_frames.saturating_sub(r);
        if fo > 0 && left <= fo {
            g *= fade_curve(self.fade_out.shape, left as f32 / fo as f32);
        }
        g
    }
}

/// A fade-in curve at `x` (0 silent … 1 full, clamped): linear;
/// equal-power `sin`; exponential over a 60 dB range (normalised to reach
/// 0 at 0); S-curve as equal-power of a smoothstep. A fade out uses the
/// same curve on the frames left. // RT
pub fn fade_curve(shape: FadeShape, x: f32) -> f32 {
    use std::f32::consts::FRAC_PI_2;
    let x = if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) };
    match shape {
        FadeShape::Linear => x,
        FadeShape::EqualPower => (x * FRAC_PI_2).sin(),
        FadeShape::Exponential => {
            const FLOOR: f32 = 0.001;
            ((10f32.powf(3.0 * (x - 1.0)) - FLOOR) / (1.0 - FLOOR)).max(0.0)
        }
        FadeShape::SCurve => {
            let s = x * x * (3.0 - 2.0 * x);
            (s * FRAC_PI_2).sin()
        }
    }
}

/// One output path's segments, sorted by start, with the longest length
/// (to find overlaps by binary search).
#[derive(Debug, Clone, PartialEq)]
struct PathSegments {
    path: OutputPathId,
    segments: Box<[Segment]>,
    longest: u64,
}

/// Every output path's segments.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioSchedule {
    paths: Box<[PathSegments]>,
}

impl AudioSchedule {
    /// No segments at all.
    pub fn is_empty(&self) -> bool {
        self.paths.iter().all(|p| p.segments.is_empty())
    }

    /// Segments in every path.
    pub fn segment_count(&self) -> usize {
        self.paths.iter().map(|p| p.segments.len()).sum()
    }

    /// `path`'s segments, sorted by start (empty when it has none). // RT
    pub fn segments(&self, path: OutputPathId) -> &[Segment] {
        match self.paths.binary_search_by_key(&path, |p| p.path) {
            Ok(i) => self.paths.get(i).map_or(&[], |p| &p.segments),
            Err(_) => &[],
        }
    }

    /// Every path with its segments.
    pub fn paths(&self) -> impl Iterator<Item = (OutputPathId, &[Segment])> + '_ {
        self.paths.iter().map(|p| (p.path, &*p.segments))
    }

    /// `path`'s segments that overlap schedule frames `[from, to)`. // RT
    pub fn overlapping(
        &self,
        path: OutputPathId,
        from: u64,
        to: u64,
    ) -> impl Iterator<Item = &Segment> + '_ {
        let (segs, longest) = match self.paths.binary_search_by_key(&path, |p| p.path) {
            Ok(i) => self
                .paths
                .get(i)
                .map_or((&[][..], 0), |p| (&*p.segments, p.longest)),
            Err(_) => (&[][..], 0),
        };
        let first = segs.partition_point(|s| s.start < from.saturating_sub(longest));
        segs.get(first..)
            .unwrap_or(&[])
            .iter()
            .take_while(move |s| s.start < to)
            .filter(move |s| s.end() > from)
    }

    /// What the streamer must have ready: one need per online segment.
    pub fn needs(&self) -> Vec<Need> {
        self.paths
            .iter()
            .flat_map(|p| p.segments.iter())
            .filter(|s| s.online)
            .map(|s| Need {
                media: s.media,
                start: s.start,
                frames: s.frames,
                file_frame: s.file_frame,
            })
            .collect()
    }
}

/// A flattened region on its lane instance, in schedule frames.
struct Use {
    order: usize,
    start: u64,
    frames: u64,
    file_frame: u64,
    seg: Segment,
}

/// Builds the audio schedule from the flattened regions of `doc`
/// (`frame_of` maps a tick to a signed schedule frame). Counts into
/// `report`.
pub(crate) fn build(
    doc: &Document,
    regions: &[ResolvedEvent],
    frame_of: &dyn Fn(Tick) -> i64,
    offline: &BTreeSet<MediaId>,
    report: &mut CompileReport,
) -> AudioSchedule {
    type Key = (Vec<EventId>, SeqId, LaneId);
    let mut groups: BTreeMap<Key, (OutputPathId, Vec<Use>)> = BTreeMap::new();
    for (order, r) in regions.iter().enumerate() {
        let EventBody::AudioRegion {
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } = r.body
        else {
            continue;
        };
        let Some(path) = r.audio_out else {
            report.audio_unrouted += 1;
            continue;
        };
        let Some(last) = r.path.last() else {
            continue;
        };
        // The region as written (flatten clipped `length` at its window).
        let written = match doc.event(last.event) {
            Ok((e, _)) => match e.body {
                EventBody::AudioRegion { length, .. } => length,
                _ => length,
            },
            Err(_) => length,
        };
        report.audio_regions += 1;
        let start = frame_of(r.tick);
        let (start, cut) = if start < 0 {
            (0u64, start.unsigned_abs())
        } else {
            (start as u64, 0)
        };
        if cut >= length {
            continue;
        }
        let refs: Vec<EventId> = r
            .path
            .iter()
            .take(r.path.len().saturating_sub(1))
            .map(|p| p.event)
            .collect();
        let entry = groups
            .entry((refs, last.seq, last.lane))
            .or_insert_with(|| (path, Vec::new()));
        entry.1.push(Use {
            order,
            start,
            frames: length - cut,
            file_frame: source_offset + cut,
            seg: Segment {
                start,
                frames: length - cut,
                media,
                file_frame: source_offset + cut,
                gain: db_to_gain(f64::from(gain_db)) as f32,
                region_offset: source_offset,
                region_frames: written,
                fade_in,
                fade_out,
                online: !offline.contains(&media),
            },
        });
    }
    let mut per_path: BTreeMap<OutputPathId, Vec<Segment>> = BTreeMap::new();
    for (_, (path, uses)) in groups {
        let out = per_path.entry(path).or_default();
        let mut bounds: Vec<u64> = uses
            .iter()
            .flat_map(|u| [u.start, u.start + u.frames])
            .collect();
        bounds.sort_unstable();
        bounds.dedup();
        let mut last: Option<(usize, u64, u64)> = None;
        let mut flush = |piece: Option<(usize, u64, u64)>, out: &mut Vec<Segment>| {
            if let Some((i, a, b)) = piece {
                if let Some(u) = uses.get(i) {
                    let mut s = u.seg;
                    s.start = a;
                    s.frames = b - a;
                    s.file_frame = u.file_frame + (a - u.start);
                    out.push(s);
                }
            }
        };
        for w in bounds.windows(2) {
            let (a, b) = (w[0], w[1]);
            let winner = uses
                .iter()
                .enumerate()
                .filter(|(_, u)| u.start <= a && u.start + u.frames >= b)
                .max_by_key(|(_, u)| (u.start, u.order))
                .map(|(i, _)| i);
            match (winner, last) {
                (Some(i), Some((j, s, e))) if i == j && e == a => last = Some((j, s, b)),
                (Some(i), prev) => {
                    flush(prev, out);
                    last = Some((i, a, b));
                }
                (None, prev) => {
                    flush(prev, out);
                    last = None;
                }
            }
        }
        flush(last, out);
    }
    let paths: Box<[PathSegments]> = per_path
        .into_iter()
        .map(|(path, mut segments)| {
            segments.sort_by_key(|s| s.start);
            let longest = segments.iter().map(|s| s.frames).max().unwrap_or(0);
            report.audio_segments += segments.len();
            report.audio_offline += segments.iter().filter(|s| !s.online).count();
            PathSegments {
                path,
                segments: segments.into_boxed_slice(),
                longest,
            }
        })
        .collect();
    AudioSchedule { paths }
}
VIZDAW_EOF
```

The `bounds.windows(2)` indexing `w[0]`, `w[1]` is in non-test code, which `clippy::indexing_slicing` denies (`core-engine` has the same lint set). Write it as `let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else { continue };`.

In `crates/core-engine/src/schedule/mod.rs`:

Find:
```rust
pub mod automation;
pub mod compile;
```
Replace:
```rust
pub mod audio;
pub mod automation;
pub mod compile;
```

Find:
```rust
pub use automation::{
```
Replace:
```rust
pub use audio::{AudioSchedule, Segment, fade_curve};
pub use automation::{
```

Find:
```rust
    /// Automation lanes (4d).
    pub automation: Automation,
}
```
Replace:
```rust
    /// Automation lanes (4d).
    pub automation: Automation,
    /// Audio regions per output path (5a).
    pub audio: AudioSchedule,
}
```

In `crates/core-engine/src/schedule/compile.rs`:

Find:
```rust
    /// MIDI destinations that currently exist.
    pub ports: Vec<MidiPortInfo>,
}
```
Replace:
```rust
    /// MIDI destinations that currently exist.
    pub ports: Vec<MidiPortInfo>,
    /// Media that cannot be read: their regions play silence (5a).
    pub offline_media: BTreeSet<MediaId>,
}
```

Find:
```rust
            route: BuiltinRoute::Builtin,
            ports: Vec::new(),
        }
    }
}
```
Replace:
```rust
            route: BuiltinRoute::Builtin,
            ports: Vec::new(),
            offline_media: BTreeSet::new(),
        }
    }
}
```

Find:
```rust
    /// Audio regions skipped (audio playback is not in this engine yet).
    pub skipped_audio: usize,
```
Replace:
```rust
    /// Audio regions that play (uses: a looped or referenced region counts
    /// once per use).
    pub audio_regions: usize,
    /// Segments the regions became.
    pub audio_segments: usize,
    /// Regions on lanes without an output path (not played).
    pub audio_unrouted: usize,
    /// Segments whose media is offline (silence).
    pub audio_offline: usize,
```

Find:
```rust
    let mut report = CompileReport::default();
```
Replace:
```rust
    let frame_signed = |t: Tick| -> i64 {
        let f = ((s.tempo.tick_to_seconds(t) - base) * sr).round();
        // Float-to-int `as` saturates.
        f as i64
    };
    let mut report = CompileReport::default();
    let mut regions = Vec::new();
```

Find:
```rust
    for r in flat {
        if matches!(r.body, EventBody::AudioRegion { .. }) {
            report.skipped_audio += 1;
            continue;
        }
```
Replace:
```rust
    for r in flat {
        if matches!(r.body, EventBody::AudioRegion { .. }) {
            regions.push(r);
            continue;
        }
```

Find:
```rust
    pending.sort_by_key(|p| (p.frame, p.class, p.group, p.order));
```
Replace:
```rust
    let audio = super::audio::build(
        doc,
        &regions,
        &frame_signed,
        &opts.offline_media,
        &mut report,
    );
    pending.sort_by_key(|p| (p.frame, p.class, p.group, p.order));
```

Find:
```rust
        chase,
        automation,
    };
```
Replace:
```rust
        chase,
        automation,
        audio,
    };
```

Find:
```rust
    if r.skipped_audio > 0 {
        w.push(format!(
            "{} audio region(s) skipped (audio playback is not available yet)",
            r.skipped_audio
        ));
    }
```
Replace:
```rust
    if r.audio_unrouted > 0 {
        w.push(format!(
            "{} audio region(s) not played: their lane has no output path",
            r.audio_unrouted
        ));
    }
    if r.audio_offline > 0 {
        w.push(format!(
            "{} audio region(s) play silence: their media is offline",
            r.audio_offline
        ));
    }
```
Add `use core_model::ids::MediaId;` and `use std::collections::BTreeSet;` to `compile.rs` (it already uses `BTreeMap`).

In `crates/core-engine/src/engine.rs`:

Find:
```rust
        let opts = CompileOptions {
            sample_rate: self.cfg.sample_rate,
            route: self.cfg.route,
            ports: self.factory.ports(),
        };
```
Replace:
```rust
        let opts = CompileOptions {
            sample_rate: self.cfg.sample_rate,
            route: self.cfg.route,
            ports: self.factory.ports(),
            offline_media: std::collections::BTreeSet::new(),
        };
```
Task 9 fills `offline_media` from the engine's media table.

In `crates/core-engine/tests/schedule.rs`, each of the four `CompileOptions { … }` literals gets `offline_media: Default::default(),`. Then run `grep -rn "skipped_audio" crates app/Sources` and fix any remaining user. It may be in `crates/core-ffi/src/host.rs`'s `report_lines`, or a test asserting the old warning. A test that asserted "audio region(s) skipped" for the demo with audio now asserts nothing about audio. The demo's region plays on output path "Audio 1-2", and its media becomes offline only once Task 9 passes the media table.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-engine --test schedule_audio 2>&1 | tail -5`
Expected: `test result: ok. 11 passed`.

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c`
Expected: all ok.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
git add crates/core-engine crates/core-ffi/src/host.rs Cargo.lock
git commit -m "engine: audio regions become per-path segments in the schedule

The overlap rule per lane instance (latest start wins, an earlier region
resumes), loops and references through flatten, fades relative to the
region as written, pickups cut at frame 0, unrouted and offline regions
reported. Fade curves: linear, equal-power, exponential over 60 dB,
S-curve.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 8: `core-engine` — the `Regions` node

**Files:**
- Create: `crates/core-engine/src/graph/regions.rs`, `crates/core-engine/tests/graph_regions.rs`
- Modify: `crates/core-engine/Cargo.toml` (dev-dependency `tempfile`), `crates/core-engine/src/graph/mod.rs`, `crates/core-engine/src/graph/spec.rs`, `crates/core-engine/src/graph/pdc.rs`, `crates/core-engine/src/graph/exec.rs`, `crates/core-engine/src/graph/mixer.rs`, `crates/core-engine/src/render.rs` (the one `exec.run` call), and the graph tests whose exhaustive `match` on `NodeSpec` or node counts change (`crates/core-engine/tests/{graph_exec,graph_mixer,graph_plan}.rs`)
- Test: `crates/core-engine/tests/graph_regions.rs`

**Interfaces:**
- Consumes: Task 7's `AudioSchedule::overlapping`, `Segment::gain_at`; Task 6's `StreamReader::{read, count_miss}`, `Read`.
- Produces:
  - `NodeSpec::Regions { path: OutputPathId }`;
  - `core_engine::graph::RegionRun` (`Copy`), with:
    - `RegionRun::NONE`;
    - `unsafe RegionRun::new(audio: &AudioSchedule, reader: &StreamReader, frame: u64, playing: bool, late: bool, muted: bool)`, whose contract is that both references outlive every run given the value, until `try_finish`;
    - `exec::Exec::run(…, regions: RegionRun, deadline)`;
  - `Runner::set_regions(&mut self, audio: Arc<AudioSchedule>, reader: StreamReader, frame: u64)` (graph tests; each `block` then plays from `frame` on and advances it);
  - `mixer_spec` adds one `Regions` node per `AudioPath` strip, after the strips, feeding that strip as its first input.

Spec §5.4 and Ruling 11.
- **Rendering.** In each run the node clears its output. Then, for every online segment of its path overlapping the run's schedule frames, it reads the frames through the stream reader and adds them times `Segment::gain_at`.
  - A miss counts through `count_miss(late)`; an offline answer counts nothing.
  - A mono read arrives as identical left and right, so mono goes to both channels at unity.
- **Delays.** The node has no latency. The existing PDC aligns it like an instrument.

- [ ] **Step 1: Write the failing tests**

In `crates/core-engine/Cargo.toml`:

Find:
```toml
[dev-dependencies]
core-engine = { path = ".", features = ["fixtures"] }
```
Replace:
```toml
[dev-dependencies]
core-engine = { path = ".", features = ["fixtures"] }
tempfile = "3"
```

Create `crates/core-engine/tests/graph_regions.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/graph_regions.rs <<'VIZDAW_EOF'
//! The Regions node (plan 5a Task 8): segments of one output path read
//! through the streamer, with gain and fades, into that path's strip.

use core_engine::fixtures::{Builder, Q};
use core_engine::graph::exec::{Graph, InsertInit, Runner};
use core_engine::graph::{GraphSpec, InputSpec, NodeSpec, StripSpec, Tap, mixer_spec, plan};
use core_engine::schedule::{AudioSchedule, CompileOptions, TargetKind, TargetTable, compile};
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig, Streamer};
use core_model::event::{Fade, FadeShape};
use core_model::format::BitDepth;
use core_model::ids::{MediaId, OutputPathId};
use core_model::mixer::StripKind;
use std::path::Path;
use std::sync::Arc;

const QF: u64 = 24_000;

/// Writes a CAF whose frame f is `(f + 1) / 1e6` left and its negative
/// right (or mono).
fn caf(path: &Path, frames: u64, ch: u16) {
    let mut w = CafWriter::create(path, 48_000, ch, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..frames)
        .flat_map(|f| {
            let x = (f + 1) as f32 / 1e6;
            if ch == 2 { vec![x, -x] } else { vec![x] }
        })
        .collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

fn value(f: u64) -> f32 {
    (f + 1) as f32 / 1e6
}

/// A Regions node for path 0 → a strip → master.
fn graph() -> Graph {
    let mut path_strip = StripSpec::new(None);
    path_strip.inputs.push(InputSpec { from: 0, tap: Tap::Post, gain: 1.0 });
    let mut master = StripSpec::new(None);
    master.inputs.push(InputSpec { from: 1, tap: Tap::Post, gain: 1.0 });
    let spec = GraphSpec {
        nodes: vec![
            NodeSpec::Regions { path: OutputPathId(0) },
            NodeSpec::Strip(path_strip),
            NodeSpec::Strip(master),
        ],
        master: 2,
    };
    let p = plan(&spec, 96_000).unwrap();
    assert_eq!(p.latency, 0);
    Graph::build(&spec, &p, 1024, |_, _| unreachable!("no inserts"))
}

/// The document's audio schedule (sequence "Song").
fn schedule(b: &Builder) -> AudioSchedule {
    let seq = *b.doc().sequences().keys().next().unwrap();
    let mut table = TargetTable::new();
    let (s, _) = compile(b.doc(), seq, &CompileOptions::new(48_000.0), &mut |k: &TargetKind| {
        table.intern(k.clone()).map(Ok)
    })
    .unwrap();
    s.audio
}

fn streamer(dir: &Path, items: &[(u64, &str)]) -> (Streamer, core_media::stream::StreamReader) {
    let (s, r) = Streamer::new(48_000.0, StreamConfig::default(), DiskMode::Manual).unwrap();
    s.set_media(
        items
            .iter()
            .map(|(id, name)| MediaFile { id: MediaId(*id), path: dir.join(name) })
            .collect(),
    );
    (s, r)
}

#[test]
fn a_region_plays_its_frames_with_gain_and_fades() {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"), 100_000, 2);
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", 100_000, 2).unwrap();
    b.region(
        seq,
        lane,
        Q,
        m,
        10,
        2000,
        -6.020_6,
        Fade { length_frames: 100, shape: FadeShape::Linear },
        Fade::default(),
    )
    .unwrap();
    let audio = Arc::new(schedule(&b));
    let (s, r) = streamer(dir.path(), &[(m.0, "a.caf")]);
    s.set_plan(Arc::from(audio.needs()));
    s.hint(QF - 512);
    s.service();
    let mut run = Runner::serial(graph());
    run.set_regions(Arc::clone(&audio), r, QF - 512);
    let (l, rr) = run.render(3072, &[512]);
    // Silence before the region.
    assert!(l[..512].iter().all(|x| *x == 0.0));
    // Frame k of the region reads media frame 10 + k at half gain, faded in
    // over 100 frames.
    for k in [0usize, 50, 99, 100, 1999] {
        let want = value(10 + k as u64) * 0.5 * if k < 100 { k as f32 / 100.0 } else { 1.0 };
        assert!((l[512 + k] - want).abs() < 1e-9, "left {k}: {} vs {want}", l[512 + k]);
        assert!((rr[512 + k] + want).abs() < 1e-9, "right {k}");
    }
    // Silence after it, and nothing missed.
    assert!(l[512 + 2000..].iter().all(|x| *x == 0.0));
    assert_eq!(s.counters().underruns, 0);
}

#[test]
fn mono_plays_on_both_channels_and_misses_count() {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("m.caf"), 50_000, 1);
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/m.caf", 50_000, 1).unwrap();
    b.region(seq, lane, 0, m, 0, 40_000, 0.0, Fade::default(), Fade::default()).unwrap();
    let audio = Arc::new(schedule(&b));
    let (s, r) = streamer(dir.path(), &[(m.0, "m.caf")]);
    s.set_plan(Arc::from(audio.needs()));
    let mut run = Runner::serial(graph());
    run.set_regions(Arc::clone(&audio), r.clone(), 0);
    // Not serviced: every block misses.
    let (l, _) = run.render(1024, &[512]);
    assert!(l.iter().all(|x| *x == 0.0));
    assert_eq!(s.counters().underruns, 2);
    s.service();
    run.set_regions(Arc::clone(&audio), r, 0);
    let (l, rr) = run.render(512, &[512]);
    assert_eq!(l[7], value(7));
    assert_eq!(rr[7], value(7));
}

#[test]
fn offline_segments_play_silence_and_count_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/gone.caf", 50_000, 2).unwrap();
    b.region(seq, lane, 0, m, 0, 40_000, 0.0, Fade::default(), Fade::default()).unwrap();
    let mut opts = CompileOptions::new(48_000.0);
    opts.offline_media.insert(m);
    let mut table = TargetTable::new();
    let (sch, _) = compile(b.doc(), seq, &opts, &mut |k: &TargetKind| table.intern(k.clone()).map(Ok)).unwrap();
    let (s, r) = streamer(dir.path(), &[]);
    let mut run = Runner::serial(graph());
    run.set_regions(Arc::new(sch.audio), r, 0);
    let (l, _) = run.render(2048, &[512]);
    assert!(l.iter().all(|x| *x == 0.0));
    assert_eq!(s.counters().underruns, 0);
}

#[test]
fn mixer_spec_feeds_each_audio_path_strip_from_a_regions_node() {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", 1000, 2).unwrap();
    b.region(seq, lane, 0, m, 0, 100, 0.0, Fade::default(), Fade::default()).unwrap();
    let doc = b.finish();
    let g = mixer_spec(&doc, &[], &mut |_| 0);
    let strip = doc.mixer().find_kind(StripKind::AudioPath(OutputPathId(0))).unwrap();
    let strip_node = g.strips.iter().find(|(_, id)| *id == strip).unwrap().0;
    let regions_node = g
        .spec
        .nodes
        .iter()
        .position(|n| matches!(n, NodeSpec::Regions { path } if *path == OutputPathId(0)))
        .unwrap();
    assert!(regions_node > strip_node, "regions nodes come after the strips");
    let NodeSpec::Strip(s) = &g.spec.nodes[strip_node] else { panic!() };
    assert_eq!(s.inputs[0].from, regions_node);
    // Only one Regions node per path.
    let n = g.spec.nodes.iter().filter(|n| matches!(n, NodeSpec::Regions { .. })).count();
    assert_eq!(n, 1);
}
VIZDAW_EOF
```

Two of these tests' expected values depend on the gain:
- `-6.020_6` dB is a gain of 0.5 (to 1e-5). The tolerance `1e-9` on values around 1e-3 holds because `(f+1)/1e6 × 0.5` is exact enough in f32. If it fails by rounding, use `1e-7`.
- `assert_eq!(s.counters().underruns, 2)`: two 512-frame blocks, one segment, one miss each.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-engine --test graph_regions 2>&1 | tail -5`
Expected: compile errors (`NodeSpec::Regions`, `Runner::set_regions` not found).

- [ ] **Step 3: Implement**

Create `crates/core-engine/src/graph/regions.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/src/graph/regions.rs <<'VIZDAW_EOF'
//! The Regions node's run context and render (sub-project 5 spec §5.4;
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
    pub playing: bool,
    /// Misses count as late starts (spec §5.5, Ruling 10).
    pub late: bool,
    /// The device runs at another rate than the project (Ruling 20).
    pub muted: bool,
}

// SAFETY: the pointees are `Sync` (`AudioSchedule` is plain data,
// `StreamReader` only touches atomics); `RegionRun::new`'s caller keeps
// them alive while any thread renders the run.
unsafe impl Send for RegionRun {}
// SAFETY: as for `Send`.
unsafe impl Sync for RegionRun {}

impl RegionRun {
    /// No audio: Regions nodes render silence.
    pub const NONE: RegionRun = RegionRun {
        audio: std::ptr::null(),
        reader: std::ptr::null(),
        frame: 0,
        playing: false,
        late: false,
        muted: false,
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
        }
    }

    /// Renders path `path` for `frames` frames into `out` (cleared first),
    /// using `scratch` (at least `frames` long). // RT
    pub(crate) fn render(
        &self,
        path: OutputPathId,
        frames: u32,
        out: &mut StereoBuf,
        scratch: &mut StereoBuf,
    ) {
        out.clear(frames);
        if !self.playing || self.muted {
            return;
        }
        // SAFETY: `new`'s contract keeps both alive for this run.
        let (Some(audio), Some(reader)) = (unsafe { self.audio.as_ref() }, unsafe {
            self.reader.as_ref()
        }) else {
            return;
        };
        let from = self.frame;
        let to = from.saturating_add(u64::from(frames));
        for seg in audio.overlapping(path, from, to) {
            if !seg.online {
                continue;
            }
            let a = from.max(seg.start);
            let b = to.min(seg.end());
            let n = (b - a) as usize;
            let off = (a - from) as usize;
            let file = seg.file_frame + (a - seg.start);
            let (Some(sl), Some(sr)) = (scratch.left.get_mut(..n), scratch.right.get_mut(..n))
            else {
                continue;
            };
            match reader.read(seg.media, seg.start, seg.file_frame, file, sl, sr) {
                Read::Done => {
                    let (Some(ol), Some(or)) = (
                        out.left.get_mut(off..off + n),
                        out.right.get_mut(off..off + n),
                    ) else {
                        continue;
                    };
                    for k in 0..n {
                        let g = seg.gain_at(file + k as u64);
                        if let (Some(o), Some(x)) = (ol.get_mut(k), sl.get(k)) {
                            *o += x * g;
                        }
                        if let (Some(o), Some(x)) = (or.get_mut(k), sr.get(k)) {
                            *o += x * g;
                        }
                    }
                }
                Read::Miss => reader.count_miss(self.late),
                Read::Offline => {}
            }
        }
    }
}
VIZDAW_EOF
```

In `crates/core-engine/src/graph/mod.rs`:

Find:
```rust
pub mod pdc;
pub mod spec;
```
Replace:
```rust
pub mod pdc;
pub mod regions;
pub mod spec;
```

Find:
```rust
pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan};
```
Replace:
```rust
pub use mixer::mixer_spec;
pub use pdc::{MAX_INSERTS, MAX_NODES, Plan, PlanError, plan};
pub use regions::RegionRun;
```
If `mixer_spec` is already re-exported, keep one export.

In `crates/core-engine/src/graph/spec.rs`:

Find:
```rust
    /// A mixer strip.
    Strip(StripSpec),
}
```
Replace:
```rust
    /// A mixer strip.
    Strip(StripSpec),
    /// The audio regions of output path `path` (5a), rendering into its
    /// node's `Post` buffer; no latency.
    Regions {
        /// The output path.
        path: OutputPathId,
    },
}
```
and add `OutputPathId` to its `core_model::ids` import.

Find:
```rust
                NodeSpec::Strip(s) => {
                    h.u64(2);
```
Replace:
```rust
                NodeSpec::Regions { path } => {
                    h.u64(3);
                    h.u64(path.0);
                }
                NodeSpec::Strip(s) => {
                    h.u64(2);
```

In `crates/core-engine/src/graph/pdc.rs`, every `match` over `NodeSpec` treats `Regions` like an instrument of latency 0:

Find:
```rust
            NodeSpec::Strip(s) => s.inserts.len(),
            NodeSpec::Instrument { .. } => 0,
```
Replace:
```rust
            NodeSpec::Strip(s) => s.inserts.len(),
            NodeSpec::Instrument { .. } | NodeSpec::Regions { .. } => 0,
```

Find:
```rust
            NodeSpec::Instrument { latency, .. } => vec![*latency],
            NodeSpec::Strip(s) => s.inserts.iter().map(|k| k.latency).collect(),
```
Replace:
```rust
            NodeSpec::Instrument { latency, .. } => vec![*latency],
            NodeSpec::Regions { .. } => Vec::new(),
            NodeSpec::Strip(s) => s.inserts.iter().map(|k| k.latency).collect(),
```

Find:
```rust
            NodeSpec::Instrument { latency, .. } => {
                if let Some(x) = t_out.get_mut(i) {
                    *x = u64::from(*latency);
                }
            }
```
Replace:
```rust
            NodeSpec::Instrument { latency, .. } => {
                if let Some(x) = t_out.get_mut(i) {
                    *x = u64::from(*latency);
                }
            }
            NodeSpec::Regions { .. } => {}
```
Fix any other exhaustive match the compiler reports in `pdc.rs` the same way.

In `crates/core-engine/src/graph/exec.rs`:

Find:
```rust
enum NodeDef {
    Instrument {
        target: usize,
    },
```
Replace:
```rust
enum NodeDef {
    Instrument {
        target: usize,
    },
    Regions {
        path: OutputPathId,
    },
```
and add `use core_model::ids::OutputPathId;`, `use crate::graph::regions::RegionRun;`.

Find:
```rust
    fn strip(
        max: u32,
        has_sc: bool,
```
Replace:
```rust
    fn regions(max: u32) -> NodeState {
        NodeState {
            pre: StereoBuf::new(0),
            post: StereoBuf::new(0),
            outs: Box::new([StereoBuf::new(max)]),
            scratch: StereoBuf::new(max),
            sc: StereoBuf::new(0),
            delays: Box::new([]),
            sc_delays: Box::new([]),
        }
    }

    fn strip(
        max: u32,
        has_sc: bool,
```

Find:
```rust
        (_, Some(NodeDef::Instrument { .. })) => {
```
Replace:
```rust
        (_, Some(NodeDef::Instrument { .. } | NodeDef::Regions { .. })) => {
```

In `Graph::build`, before the `NodeSpec::Strip(s) => {` arm of the node loop:

Find:
```rust
                    defs.push(NodeDef::Instrument { target });
                    nodes.push(UnsafeCell::new(NodeState::instrument(max_frames, *outputs)));
                }
```
Replace:
```rust
                    defs.push(NodeDef::Instrument { target });
                    nodes.push(UnsafeCell::new(NodeState::instrument(max_frames, *outputs)));
                }
                NodeSpec::Regions { path } => {
                    defs.push(NodeDef::Regions { path: *path });
                    nodes.push(UnsafeCell::new(NodeState::regions(max_frames)));
                }
```

`NodeState::resize` already resizes `outs` and the non-empty `scratch`, so nothing changes there.

Now pass the run's `RegionRun` to `render_node`.

Find:
```rust
unsafe fn render_node(
    g: &Graph,
    sinks: &SinkTable,
    shared: &Shared,
    frames: u32,
    info: &BlockInfo,
    i: usize,
) {
```
Replace:
```rust
unsafe fn render_node(
    g: &Graph,
    sinks: &SinkTable,
    shared: &Shared,
    frames: u32,
    info: &BlockInfo,
    regions: &RegionRun,
    i: usize,
) {
```

Find:
```rust
        NodeDef::Strip {
            inputs,
            inserts,
            gain_l,
```
(the arm in `render_node`) and insert before it:
```rust
        NodeDef::Regions { path } => {
            let NodeState { outs, scratch, .. } = st;
            if let Some(out) = outs.first_mut() {
                regions.render(*path, frames, out, scratch);
            }
        }
```

Find:
```rust
pub(crate) struct Job {
    graph: *const Graph,
    sinks: *const SinkTable,
    shared: *const Shared,
    pool: Option<*const PoolShared>,
    frames: u32,
    info: BlockInfo,
}
```
Replace:
```rust
pub(crate) struct Job {
    graph: *const Graph,
    sinks: *const SinkTable,
    shared: *const Shared,
    pool: Option<*const PoolShared>,
    frames: u32,
    info: BlockInfo,
    regions: RegionRun,
}
```

Find:
```rust
            frames: 0,
            info: BlockInfo::default(),
        }
    }
```
Replace:
```rust
            frames: 0,
            info: BlockInfo::default(),
            regions: RegionRun::NONE,
        }
    }
```

Find:
```rust
        unsafe { render_node(g, sinks, shared, self.frames, &self.info, i) };
```
Replace:
```rust
        unsafe { render_node(g, sinks, shared, self.frames, &self.info, &self.regions, i) };
```

Find:
```rust
        frames: u32,
        info: BlockInfo,
        deadline: Option<Instant>,
    ) -> RunResult {
        // Never reset counters or the queue under a run still in flight.
```
Replace:
```rust
        frames: u32,
        info: BlockInfo,
        regions: RegionRun,
        deadline: Option<Instant>,
    ) -> RunResult {
        // Never reset counters or the queue under a run still in flight.
```

Find:
```rust
                pool: pool.map(|p| p as *const PoolShared),
                frames,
                info,
            };
```
Replace:
```rust
                pool: pool.map(|p| p as *const PoolShared),
                frames,
                info,
                regions,
            };
```

The `Runner` gets regions for tests:

Find:
```rust
pub struct Runner {
    exec: Box<Exec>,
    graph: Box<Graph>,
    sinks: Box<SinkTable>,
    shared: Arc<Shared>,
    pool: Option<Pool>,
    deadline: Option<Duration>,
}
```
Replace:
```rust
pub struct Runner {
    exec: Box<Exec>,
    graph: Box<Graph>,
    sinks: Box<SinkTable>,
    shared: Arc<Shared>,
    pool: Option<Pool>,
    deadline: Option<Duration>,
    /// Audio for `Regions` nodes, the reader and the next block's frame.
    regions: Option<(Arc<crate::schedule::AudioSchedule>, Box<core_media::stream::StreamReader>, u64)>,
}
```
Add `regions: None,` to both constructors (`serial` and `parallel`), then:

Find:
```rust
    /// Hands the workers a new real-time environment.
```
Replace:
```rust
    /// Plays `audio` through `reader` from schedule frame `frame`; every
    /// `block` advances the frame (graph tests of the Regions node).
    pub fn set_regions(
        &mut self,
        audio: Arc<crate::schedule::AudioSchedule>,
        reader: core_media::stream::StreamReader,
        frame: u64,
    ) {
        self.exec.wait_idle();
        self.regions = Some((audio, Box::new(reader), frame));
    }

    /// Hands the workers a new real-time environment.
```

Find:
```rust
        match self.exec.run(
            &self.graph,
            &self.sinks,
            &self.shared,
            frames,
            BlockInfo::default(),
            deadline,
        ) {
```
Replace:
```rust
        let regions = match &mut self.regions {
            Some((audio, reader, frame)) => {
                reader.begin_block(*frame);
                // SAFETY: `audio` and `reader` live in `self` (an `Arc` and
                // a `Box`, so their addresses are stable) until the next
                // `set_regions` or drop, both of which wait for the run.
                let r = unsafe { RegionRun::new(audio, reader, *frame, true, false, false) };
                *frame += u64::from(frames);
                r
            }
            None => RegionRun::NONE,
        };
        match self.exec.run(
            &self.graph,
            &self.sinks,
            &self.shared,
            frames,
            BlockInfo::default(),
            regions,
            deadline,
        ) {
```

In `crates/core-engine/src/render.rs`, the renderer's call passes `RegionRun::NONE` for now (Task 9 builds the real one):

Find:
```rust
            .run(g, &self.sinks, &self.shared, frames, info, deadline)
```
Replace:
```rust
            .run(
                g,
                &self.sinks,
                &self.shared,
                frames,
                info,
                crate::graph::RegionRun::NONE,
                deadline,
            )
```

In `crates/core-engine/src/graph/mixer.rs`, add a Regions node per `AudioPath` strip. The node's index comes after every strip, and it is the strip's first input.

Find:
```rust
    let master = node(m.master()).unwrap_or(base);
```
Replace:
```rust
    let master = node(m.master()).unwrap_or(base);
    // One Regions node per AudioPath strip, after the strips (5a Ruling
    // 11), as that strip's first input.
    let mut region_paths: Vec<OutputPathId> = Vec::new();
    for (k, s) in m.strips().values().enumerate() {
        if let StripKind::AudioPath(path) = s.kind {
            let from = base + m.strips().len() + region_paths.len();
            region_paths.push(path);
            if let Some(spec) = strips.get_mut(k) {
                spec.inputs.push(InputSpec {
                    from,
                    tap: Tap::Post,
                    gain: 1.0,
                });
            }
        }
    }
```

Find:
```rust
    nodes.extend(strips.into_iter().map(NodeSpec::Strip));
```
Replace:
```rust
    nodes.extend(strips.into_iter().map(NodeSpec::Strip));
    nodes.extend(region_paths.into_iter().map(|path| NodeSpec::Regions { path }));
```
and add `OutputPathId` to the `core_model::ids` import.

Check whether `master` is used to compute anything that assumes `nodes.len() == base + strips` (`GraphSpec { nodes, master }` only). The engine's meter and control code keys nodes by strip index, which is unchanged. Search `engine.rs` for `spec.nodes.len()` or similar and confirm nothing assumes there are no nodes after the strips.

Then fix the tests that no longer compile or whose counts change:
- `crates/core-engine/tests/graph_exec.rs`, `graph_mixer.rs` and `graph_plan.rs`: a `match` with `NodeSpec::Instrument { .. } => panic!(…)` as its last arm gets `NodeSpec::Regions { .. } => panic!("not a strip")` (or turns into `_ =>`).
- `graph_mixer.rs`: tests that assert `spec.nodes.len()` for documents with output paths add one node per output path. Say in the report which assertions changed.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-engine 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c`
Expected: all ok; `graph_regions` runs 4 tests.

Run the real-time allocation tests, which must stay green: `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph 2>&1 | tail -3`.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | sort | uniq -c
git add crates/core-engine Cargo.lock
git commit -m "engine: Regions node feeding each output path's strip

One Regions node per AudioPath strip reads its path's segments through
the stream reader and sums them with gain and fades; misses count, offline
segments are silent. The executor's job carries the run's RegionRun.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 9: `core-engine` — the streamer in the engine and renderer

**Files:**
- Create: `crates/core-engine/tests/audio_playback.rs`
- Modify: `crates/core-engine/src/engine.rs`, `crates/core-engine/src/render.rs`, `crates/core-engine/tests/rt_alloc.rs`, `crates/vizdaw-play/Cargo.toml`, `crates/vizdaw-play/src/{offline.rs,live.rs,project.rs}` (wherever an engine loads a project package: pass its media), `Cargo.lock`
- Test: `crates/core-engine/tests/audio_playback.rs`, `crates/core-engine/tests/rt_alloc.rs`

**Interfaces:**
- Consumes: Tasks 6–8.
- Produces:
  - `EngineConfig.{disk: DiskMode, stream: StreamConfig}` (defaults `Thread`, `StreamConfig::default()`);
  - `Engine::{set_media(&mut self, files: Vec<MediaFile>), streamer(&self) -> &Streamer, take_stream_errors(&self) -> Vec<(MediaId, String)>, set_regions_muted(&self, on: bool)}`;
  - `Faults::{STREAM_UNDERRUN (2048, "stream-underrun"), LATE_START (4096, "late-start")}`;
  - `Engine::play`/`play_until` wait up to 250 ms for the streamer at the start position. `Engine::locate_frame` hints the streamer. `set_loop`/`set_loop_range` tell it the loop.
  - `core_engine::engine::LOCATE_WAIT: Duration = 250 ms`, `render::LATE_SECONDS: f64 = 0.25`.

Spec §5.5, §12.2 and Rulings 10, 12 and 21.
- **Ownership.** The engine owns the `Streamer`. The renderer owns a boxed `StreamReader`, so its address is stable for the executor's job.
- **The renderer, per block.**
  - After `try_finish` succeeds and the block's commands are applied, it bumps the streamer epoch and publishes the playhead (`begin_block`). The epoch never advances while a run is in flight, and the disk side sees a Locate's new position at once.
  - While the schedule has audio, it cuts runs at the loop end and the sequence end.
  - Before each run it builds a `RegionRun` from `Transport::play_pos`. It is late while the playhead is within `LATE_SECONDS` of where Play or Locate put it, and muted while `Shared::regions_muted`.
- **Compiling.** `Engine::load` passes `offline_media` (the document's media not in the engine's media table) to the compiler, and hands the schedule's needs to the streamer.
- **Faults.** `Engine::faults` adds `STREAM_UNDERRUN`/`LATE_START` when the streamer's counters grew since the last read.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-engine/tests/audio_playback.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-engine/tests/audio_playback.rs <<'VIZDAW_EOF'
//! Regions through the whole engine (plan 5a Task 9): the offline backend,
//! the streamer in Manual mode (serviced between blocks, as the disk
//! thread would), loops, locates, starvation, offline media.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::{Builder, Q};
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_engine::render::Faults;
use core_engine::transport::PlayState;
use core_media::caf::{CafWriter, WriteMode};
use core_media::stream::{DiskMode, MediaFile, StreamConfig};
use core_model::document::Document;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::{MediaId, SeqId};
use proptest::prelude::*;
use std::path::Path;

const QF: u64 = 24_000;
const FILE_FRAMES: u64 = 480_000;
const BLOCK: u32 = 512;

fn value(f: u64) -> f32 {
    ((f % 997) as f32 + 1.0) / 2000.0
}

fn caf(path: &Path) {
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    let data: Vec<f32> = (0..FILE_FRAMES).flat_map(|f| [value(f), -value(f)]).collect();
    w.write(&data).unwrap();
    w.finish().unwrap();
}

/// Regions `(beat, offset, frames)` of one 10 s media item on one lane.
fn doc(regions: &[(i64, u64, u64)]) -> (Document, SeqId) {
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let m = b.media("Media/a.caf", FILE_FRAMES, 2).unwrap();
    for &(beat, off, n) in regions {
        b.region(seq, lane, beat * Q, m, off, n, 0.0, Fade::default(), Fade::default())
            .unwrap();
    }
    (b.finish(), seq)
}

struct Rig {
    engine: Engine,
    out: OfflineBackend,
    _dir: tempfile::TempDir,
}

/// `cached`: files are read whole; otherwise every region streams.
fn rig(doc: &Document, seq: SeqId, cached: bool, online: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    caf(&dir.path().join("a.caf"));
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = DiskMode::Manual;
    cfg.stream = StreamConfig {
        cache_file_bytes: if cached { 64 << 20 } else { 0 },
        ..StreamConfig::default()
    };
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    if online {
        engine.set_media(vec![MediaFile { id: MediaId(0), path: dir.path().join("a.caf") }]);
    }
    engine.load(doc, seq).unwrap();
    let mut out = OfflineBackend::new(48_000.0, BLOCK);
    out.start(renderer).unwrap();
    out.reserve(4096);
    Rig { engine, out, _dir: dir }
}

impl Rig {
    /// Renders `blocks` blocks (servicing the disk first unless `starve`);
    /// returns the left channel.
    fn render(&mut self, blocks: usize, starve: bool) -> Vec<f32> {
        let mut left = Vec::new();
        for _ in 0..blocks {
            if !starve {
                self.engine.streamer().service();
            }
            self.out.block(BLOCK);
            left.extend_from_slice(self.out.output(BLOCK).0);
        }
        left
    }
}

#[test]
fn a_region_plays_through_the_engine() {
    let (d, seq) = doc(&[(1, 100, 2 * QF)]);
    let mut r = rig(&d, seq, false, true);
    r.engine.play().unwrap();
    let left = r.render(200, false);
    assert!(left[..QF as usize].iter().all(|x| *x == 0.0));
    // The strip's pan law scales by a constant k.
    let k = left[QF as usize] / value(100);
    assert!(k > 0.5 && k <= 1.0, "{k}");
    for i in [0u64, 1, 999, QF, 2 * QF - 1] {
        let got = left[(QF + i) as usize];
        assert!((got - k * value(100 + i)).abs() < 1e-6, "frame {i}: {got}");
    }
    assert_eq!(left[(3 * QF) as usize], 0.0);
    assert!(r.engine.faults().is_empty(), "{:?}", r.engine.faults().names());
}

#[test]
fn a_loop_inside_a_region_wraps_seamlessly() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, true);
    // Loop beats 1 to 3 (48 000 frames), well inside the region.
    r.engine.set_loop_range(Some((QF, 3 * QF))).unwrap();
    r.engine.locate_frame(QF).unwrap();
    r.engine.play().unwrap();
    let left = r.render(400, false);
    let k = left[0] / value(QF);
    for (i, x) in left.iter().enumerate() {
        let f = QF + (i as u64 % (2 * QF));
        assert!((x - k * value(f)).abs() < 1e-6, "out {i}: {x}");
    }
    assert!(r.engine.faults().is_empty(), "{:?}", r.engine.faults().names());
}

#[test]
fn starved_disk_plays_silence_and_flags_underruns() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, true);
    // Play waits for the disk (and services it in Manual mode), so starve
    // after a locate the disk has not seen.
    r.engine.play().unwrap();
    r.render(2, false);
    r.engine.locate_frame(5 * QF).unwrap();
    let left = r.render(60, true);
    assert!(left.iter().all(|x| *x == 0.0));
    let f = r.engine.faults();
    assert!(f.contains(Faults::LATE_START), "{:?}", f.names());
    assert!(f.contains(Faults::STREAM_UNDERRUN), "{:?}", f.names());
    assert_eq!(r.engine.state(), PlayState::Playing);
    // The disk catches up: audio returns.
    let left = r.render(10, false);
    assert!(left.iter().any(|x| *x != 0.0));
}

#[test]
fn misses_right_after_play_count_as_late_start() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, true);
    r.engine.play().unwrap();
    r.render(2, false);
    r.engine.faults();
    r.engine.locate_frame(4 * QF).unwrap();
    // 250 ms is 12 000 frames: 23 blocks of 512 are within it.
    r.render(20, true);
    let f = r.engine.faults();
    assert!(f.contains(Faults::LATE_START) && !f.contains(Faults::STREAM_UNDERRUN), "{:?}", f.names());
    assert_eq!(Faults::LATE_START.names(), vec!["late-start"]);
    assert_eq!(Faults::STREAM_UNDERRUN.names(), vec!["stream-underrun"]);
}

#[test]
fn offline_media_plays_silence_and_counts_nothing() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, false, false);
    r.engine.play().unwrap();
    let left = r.render(100, false);
    assert!(left.iter().all(|x| *x == 0.0));
    assert!(r.engine.faults().is_empty(), "{:?}", r.engine.faults().names());
}

#[test]
fn muted_regions_play_silence() {
    let (d, seq) = doc(&[(0, 0, 8 * QF)]);
    let mut r = rig(&d, seq, true, true);
    r.engine.set_regions_muted(true);
    r.engine.play().unwrap();
    assert!(r.render(50, false).iter().all(|x| *x == 0.0));
    r.engine.set_regions_muted(false);
    assert!(r.render(5, false).iter().any(|x| *x != 0.0));
}

/// Plays both rigs through the same actions; returns their left channels.
fn both(
    regions: &[(i64, u64, u64)],
    lp: Option<(u64, u64)>,
    actions: &[(u8, u64)],
) -> (Vec<f32>, Vec<f32>, Faults) {
    let (d, seq) = doc(regions);
    let mut a = rig(&d, seq, true, true);
    let mut b = rig(&d, seq, false, true);
    for r in [&mut a, &mut b] {
        r.engine.set_loop_range(lp).unwrap();
        r.engine.play().unwrap();
    }
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    for &(what, n) in actions {
        if what == 0 {
            let at = n % (16 * QF);
            a.engine.locate_frame(at).unwrap();
            b.engine.locate_frame(at).unwrap();
        }
        la.extend(a.render(4, false));
        lb.extend(b.render(4, false));
    }
    (la, lb, b.engine.faults())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 16, .. ProptestConfig::default() })]

    /// Streaming is invisible: a streamed render equals a fully cached one
    /// sample for sample, through loops and locates (late starts aside,
    /// which only happen when the disk was not serviced).
    #[test]
    fn streamed_render_equals_cached_render(
        regions in prop::collection::vec((0i64..12, 0u64..200_000, 1u64..200_000), 1..5),
        lp in prop::option::of((0u64..8, 1u64..8)),
        actions in prop::collection::vec((0u8..6, 0u64..1_000_000), 1..30),
    ) {
        let regions: Vec<(i64, u64, u64)> = regions
            .into_iter()
            .map(|(b, off, n)| (b, off, n.min(FILE_FRAMES - off)))
            .collect();
        let lp = lp.map(|(s, l)| (s * QF, (s + l) * QF));
        let (a, b, faults) = both(&regions, lp, &actions);
        prop_assert_eq!(a, b);
        prop_assert!(!faults.contains(Faults::STREAM_UNDERRUN), "{:?}", faults.names());
    }

    /// A disk that never runs: silence, never garbage, never a stop.
    #[test]
    fn a_starved_disk_is_only_ever_silent(
        regions in prop::collection::vec((0i64..12, 0u64..200_000, 1u64..200_000), 1..5),
        at in 0u64..(12 * QF),
    ) {
        let regions: Vec<(i64, u64, u64)> = regions
            .into_iter()
            .map(|(b, off, n)| (b, off, n.min(FILE_FRAMES - off)))
            .collect();
        let (d, seq) = doc(&regions);
        let mut r = rig(&d, seq, false, true);
        r.engine.play().unwrap();
        r.engine.locate_frame(at).unwrap();
        let left = r.render(30, true);
        prop_assert!(left.iter().all(|x| *x == 0.0));
        prop_assert_eq!(r.engine.state(), PlayState::Playing);
    }
}
VIZDAW_EOF
```

There is one subtlety in `starved_disk_plays_silence…`. Before the locate, two serviced blocks played audio at the start. After the locate the disk is never serviced, so every block misses: the first ~23 as late starts, the rest as underruns. Once serviced again, it plays.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-engine --test audio_playback 2>&1 | tail -5`
Expected: compile errors (`EngineConfig.disk`, `Engine::set_media`, `Faults::LATE_START` not found).

- [ ] **Step 3: Implement**

In `crates/core-engine/src/render.rs`:

Find:
```rust
    /// A recompiled graph was invalid (cycle or bad index); the previous
    /// graph keeps playing.
    pub const GRAPH_INVALID: Faults = Faults(1024);
```
Replace:
```rust
    /// A recompiled graph was invalid (cycle or bad index); the previous
    /// graph keeps playing.
    pub const GRAPH_INVALID: Faults = Faults(1024);
    /// A region's frames were not streamed in time: they played as
    /// silence (5a).
    pub const STREAM_UNDERRUN: Faults = Faults(2048);
    /// Right after Play or Locate (within [`LATE_SECONDS`]) a region was
    /// not ready yet and started late (5a).
    pub const LATE_START: Faults = Faults(4096);
```

Find:
```rust
            (Faults::GRAPH_INVALID, "graph-invalid"),
        ]
```
Replace:
```rust
            (Faults::GRAPH_INVALID, "graph-invalid"),
            (Faults::STREAM_UNDERRUN, "stream-underrun"),
            (Faults::LATE_START, "late-start"),
        ]
```

Add to `Shared` (the struct and its `Default`):

Find:
```rust
pub struct Shared {
    position: AtomicU64,
```
Replace:
```rust
pub struct Shared {
    /// Regions play silence (the device refused the project rate, 5a).
    regions_muted: AtomicBool,
    position: AtomicU64,
```

Find:
```rust
        Shared {
            position: AtomicU64::new(0),
```
Replace:
```rust
        Shared {
            regions_muted: AtomicBool::new(false),
            position: AtomicU64::new(0),
```

Find:
```rust
    /// Sets fault bits (used by backends, e.g. after a caught panic). // RT
    pub fn raise(&self, f: Faults) {
```
(this is `Shared::raise`, the first of the two `raise` functions; if the excerpt matches `Renderer::raise` instead, put the new methods in `impl Shared`) and insert before it:
```rust
    /// Mutes or unmutes every region (5a Ruling 20).
    pub fn set_regions_muted(&self, on: bool) {
        self.regions_muted.store(on, Ordering::Relaxed);
    }

    /// Whether regions are muted. // RT
    pub fn regions_muted(&self) -> bool {
        self.regions_muted.load(Ordering::Relaxed)
    }

```

Add near `CMD_CAPACITY`:
```rust
/// How long after Play or Locate a miss counts as a late start (spec
/// §5.5: the transport waits this long for the disk at most).
pub const LATE_SECONDS: f64 = 0.25;
```

In `struct Renderer`, after `shared: Arc<Shared>,` add:
```rust
    /// The streamer's real-time side (boxed: the executor's job points at
    /// it while a run is in flight).
    reader: Box<core_media::stream::StreamReader>,
    /// Misses before this schedule frame count as late starts.
    late_until: u64,
```

Change `Renderer::new`:

Find:
```rust
    pub(crate) fn new(
        cfg: RenderConfig,
        cmds: Consumer<EngineCmd>,
        retire: Producer<Retired>,
        shared: Arc<Shared>,
        pool: Option<Arc<PoolShared>>,
    ) -> Renderer {
        let mut r = Renderer {
            cfg,
            cmds,
            retire,
            shared,
```
Replace:
```rust
    pub(crate) fn new(
        cfg: RenderConfig,
        cmds: Consumer<EngineCmd>,
        retire: Producer<Retired>,
        shared: Arc<Shared>,
        pool: Option<Arc<PoolShared>>,
        reader: core_media::stream::StreamReader,
    ) -> Renderer {
        let mut r = Renderer {
            cfg,
            cmds,
            retire,
            shared,
            reader: Box::new(reader),
            late_until: 0,
```

In `process_inner`, bump the epoch once the previous run is known finished and this block's commands are applied. After the commands, so a Locate's new position is what the disk side plans around:

Find:
```rust
        self.block_ns = time.host_time_ns;
        self.drain_commands();
```
Replace:
```rust
        self.block_ns = time.host_time_ns;
        self.drain_commands();
        // No run is in flight (`try_finish` succeeded above): the streamer
        // may count a new epoch (5a Ruling 9) and plan around the playhead
        // as this block's commands left it.
        self.reader.begin_block(self.transport.pos);
```

Find:
```rust
            let run = self.next_run(length, frames - offset);
            let end = offset + run;
            self.automate(length, run);
            let info = self.block_info();
```
Replace:
```rust
            let run = self.next_run(length, frames - offset);
            let end = offset + run;
            self.automate(length, run);
            let info = self.block_info();
            let regions = self.region_run(length);
```

Find:
```rust
            let lost = self.render_graph(out, offset, run, info, deadline);
```
Replace:
```rust
            let lost = self.render_graph(out, offset, run, info, regions, deadline);
```

Find:
```rust
    fn render_graph(
        &mut self,
        out: &mut [&mut [f32]],
        offset: u32,
        frames: u32,
        info: BlockInfo,
        deadline: Option<Instant>,
    ) -> bool {
```
Replace:
```rust
    fn render_graph(
        &mut self,
        out: &mut [&mut [f32]],
        offset: u32,
        frames: u32,
        info: BlockInfo,
        regions: crate::graph::RegionRun,
        deadline: Option<Instant>,
    ) -> bool {
```
and in its `exec.run(…)` call (changed in Task 8) replace `crate::graph::RegionRun::NONE` with `regions`.

Make runs stop at the loop and sequence ends while audio plays (Ruling 12):

Find:
```rust
        let Some(s) = self.schedule.as_deref() else {
            return left;
        };
        if s.automation.is_empty() {
            return left;
        }
        let Some(pos) = self.transport.play_pos(length) else {
            return left;
        };
        let mut run = u64::from(left);
        if let Some(c) = s.automation.next_cut(pos) {
            run = run.min((c - pos).max(u64::from(MIN_RUN)));
        }
```
Replace:
```rust
        let Some(s) = self.schedule.as_deref() else {
            return left;
        };
        if s.automation.is_empty() && s.audio.is_empty() {
            return left;
        }
        let Some(pos) = self.transport.play_pos(length) else {
            return left;
        };
        let mut run = u64::from(left);
        if let Some(c) = s.automation.next_cut(pos) {
            run = run.min((c - pos).max(u64::from(MIN_RUN)));
        }
```
Update `next_run`'s doc comment to say "…while a schedule with automation or audio plays (Plan 4d Ruling 10, 5a Ruling 12)".

Add the region context builder next to `block_info`:

```rust
    /// The `Regions` nodes' context for the run starting now: the
    /// schedule's audio from the playhead, late within `LATE_SECONDS` of
    /// a Play or Locate, muted when the device refused the rate. // RT
    fn region_run(&self, length: u64) -> crate::graph::RegionRun {
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
                pos < self.late_until,
                self.shared.regions_muted(),
            )
        }
    }
```

In `apply`, Play and Locate open the late-start window:

Find:
```rust
                self.transport.end = until;
                self.transport.play(length);
                self.seek();
```
Replace:
```rust
                self.transport.end = until;
                self.transport.play(length);
                self.late_until = self.transport.pos.saturating_add(self.late_frames());
                self.seek();
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
                self.late_until = frame.saturating_add(self.late_frames());
```

and add:
```rust
    fn late_frames(&self) -> u64 {
        (LATE_SECONDS * self.cfg.sample_rate.max(0.0)) as u64
    }
```

In `crates/core-engine/src/engine.rs`:

Find:
```rust
    /// Fraction of a block the graph may take (Plan Ruling 18); `None`
    /// waits (offline).
    pub deadline: Option<f64>,
}
```
Replace:
```rust
    /// Fraction of a block the graph may take (Plan Ruling 18); `None`
    /// waits (offline).
    pub deadline: Option<f64>,
    /// Who runs disk passes (tests: Manual, serviced between blocks).
    pub disk: DiskMode,
    /// Streaming sizes (spec §5.5).
    pub stream: StreamConfig,
}

/// How long Play waits for the disk at most (spec §5.5).
pub const LOCATE_WAIT: Duration = Duration::from_millis(250);
```

Find:
```rust
            workers: 0,
            deadline: None,
        }
    }
}
```
Replace:
```rust
            workers: 0,
            deadline: None,
            disk: DiskMode::Thread,
            stream: StreamConfig::default(),
        }
    }
}
```

In `struct Engine`, after `pool: Option<Pool>,` add:
```rust
    /// The disk streamer (5a).
    streamer: Streamer,
    /// Media the streamer may read; the rest compile as offline.
    online: BTreeSet<MediaId>,
    /// Where the last Locate put the transport (Play waits for the disk
    /// there; the published position may not have caught up).
    located: Option<u64>,
    /// The streamer's (underruns, late) counters at the last `faults`.
    stream_seen: (AtomicU64, AtomicU64),
```

In `with_spawner`, create the streamer before the renderer:

Find:
```rust
        let renderer = Renderer::new(
            RenderConfig {
                sample_rate: cfg.sample_rate,
                max_frames: cfg.max_frames,
                midi_offset_ns: cfg.midi_offset_ns,
                deadline: cfg.deadline,
            },
            cmd_rx,
            retire_tx,
            Arc::clone(&shared),
            pool.as_ref().map(Pool::handle),
        );
```
Replace:
```rust
        let (streamer, reader) =
            Streamer::new(cfg.sample_rate, cfg.stream, cfg.disk).map_err(|e| {
                EngineError::Thread {
                    name: "vizdaw-disk",
                    reason: e.to_string(),
                }
            })?;
        let renderer = Renderer::new(
            RenderConfig {
                sample_rate: cfg.sample_rate,
                max_frames: cfg.max_frames,
                midi_offset_ns: cfg.midi_offset_ns,
                deadline: cfg.deadline,
            },
            cmd_rx,
            retire_tx,
            Arc::clone(&shared),
            pool.as_ref().map(Pool::handle),
            reader,
        );
```
and add to the `Engine { … }` literal:
```rust
            streamer,
            online: BTreeSet::new(),
            located: None,
            stream_seen: (AtomicU64::new(0), AtomicU64::new(0)),
```
Check `EngineError::Thread`'s `name` field type with `grep -n "Thread {" -A6 crates/core-engine/src/error.rs`. If it is not `&'static str`, adapt.

`Engine::load` compiles with the offline set and plans the streamer:

Find:
```rust
            ports: self.factory.ports(),
            offline_media: std::collections::BTreeSet::new(),
        };
```
Replace:
```rust
            ports: self.factory.ports(),
            offline_media: doc
                .media()
                .keys()
                .filter(|m| !self.online.contains(m))
                .copied()
                .collect(),
        };
```

Find:
```rust
        self.tempo = Some(schedule.tempo.clone());
        self.length_frames = schedule.length_frames;
```
Replace:
```rust
        self.streamer.set_plan(Arc::from(schedule.audio.needs()));
        self.tempo = Some(schedule.tempo.clone());
        self.length_frames = schedule.length_frames;
```

Replace the transport methods:

Find:
```rust
    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
        self.push(EngineCmd::Play { until: None })
    }

    /// Starts playback that stops at `tick` of the loaded sequence, the
    /// loop ignored (Play Selection, plan 4f Ruling 9).
    pub fn play_until(&mut self, tick: Tick) -> Result<(), EngineError> {
        let frame = self.tempo.as_ref().map_or(0, |t| t.tick_to_frame(tick));
        self.push(EngineCmd::Play { until: Some(frame) })
    }
```
Replace:
```rust
    /// Waits (up to [`LOCATE_WAIT`]) for the streamer at the frame Play
    /// starts from (spec §5.5); after that, playing rolls anyway.
    fn prime(&mut self) {
        let at = self
            .located
            .take()
            .unwrap_or_else(|| self.transport_frames());
        let at = if at >= self.length_frames { 0 } else { at };
        let _ = self.streamer.wait_ready(at, LOCATE_WAIT);
    }

    /// Starts playback (from the start when at or past the end).
    pub fn play(&mut self) -> Result<(), EngineError> {
        self.prime();
        self.push(EngineCmd::Play { until: None })
    }

    /// Starts playback that stops at `tick` of the loaded sequence, the
    /// loop ignored (Play Selection, plan 4f Ruling 9).
    pub fn play_until(&mut self, tick: Tick) -> Result<(), EngineError> {
        let frame = self.tempo.as_ref().map_or(0, |t| t.tick_to_frame(tick));
        self.prime();
        self.push(EngineCmd::Play { until: Some(frame) })
    }
```

Find:
```rust
    /// Moves to a schedule frame.
    pub fn locate_frame(&mut self, frame: u64) -> Result<(), EngineError> {
        self.push(EngineCmd::Locate(frame))
    }
```
Replace:
```rust
    /// Moves to a schedule frame (the streamer starts filling there).
    pub fn locate_frame(&mut self, frame: u64) -> Result<(), EngineError> {
        self.streamer.hint(frame);
        self.located = Some(frame);
        self.push(EngineCmd::Locate(frame))
    }
```

Find:
```rust
    pub fn set_loop(&mut self, on: bool) -> Result<(), EngineError> {
        let range = if on { self.loop_range } else { None };
        self.push(EngineCmd::SetLoop(range))?;
```
Replace:
```rust
    pub fn set_loop(&mut self, on: bool) -> Result<(), EngineError> {
        let range = if on { self.loop_range } else { None };
        self.streamer.set_loop(range);
        self.push(EngineCmd::SetLoop(range))?;
```

Find:
```rust
    pub fn set_loop_range(&mut self, range: Option<(u64, u64)>) -> Result<(), EngineError> {
        self.push(EngineCmd::SetLoop(range))?;
```
Replace:
```rust
    pub fn set_loop_range(&mut self, range: Option<(u64, u64)>) -> Result<(), EngineError> {
        self.streamer.set_loop(range);
        self.push(EngineCmd::SetLoop(range))?;
```

`load` also re-sends the default loop with `self.push(EngineCmd::SetLoop(self.loop_range))` when `default_loop` is on. Add `self.streamer.set_loop(self.loop_range);` beside that push.

Find:
```rust
    pub fn faults(&self) -> Faults {
```
and replace the function body so that the streamer's counters add their bits. Keep the existing first line, which reads the shared faults, as `let mut f = …;`:
```rust
    pub fn faults(&self) -> Faults {
        let mut f = self.shared.take_faults();
        let c = self.streamer.counters();
        let (u, l) = (&self.stream_seen.0, &self.stream_seen.1);
        if c.underruns > u.swap(c.underruns, Ordering::Relaxed) {
            f = Faults(f.0 | Faults::STREAM_UNDERRUN.0);
        }
        if c.late > l.swap(c.late, Ordering::Relaxed) {
            f = Faults(f.0 | Faults::LATE_START.0);
        }
        f
    }
```
If the existing body is more than `self.shared.take_faults()`, keep what it does and add the two checks.

Then add:
```rust
    /// The media the streamer may read (online items with their CAF
    /// paths); the others compile as offline at the next `load`.
    pub fn set_media(&mut self, files: Vec<MediaFile>) {
        self.online = files.iter().map(|f| f.id).collect();
        self.streamer.set_media(files);
    }

    /// The streamer (tests service it in Manual mode).
    pub fn streamer(&self) -> &Streamer {
        &self.streamer
    }

    /// Media whose reads failed since the last call (each once).
    pub fn take_stream_errors(&self) -> Vec<(MediaId, String)> {
        self.streamer.take_errors()
    }

    /// Mutes every region (the device refused the project rate) or
    /// unmutes them.
    pub fn set_regions_muted(&self, on: bool) {
        self.shared.set_regions_muted(on);
    }
```

Imports in `engine.rs`:
- `use core_media::stream::{DiskMode, MediaFile, StreamConfig, Streamer};`
- `use core_model::ids::MediaId;`
- `use std::collections::BTreeSet;` (if not already)
- `use std::sync::atomic::AtomicU64;`

Fix `EngineConfig { … }` literals elsewhere (tests and `core-ffi`) with `..EngineConfig::new(rate)`, or by adding the two fields. Most code calls `EngineConfig::new`, which needs no change.

Add the real-time check. In `crates/core-engine/tests/rt_alloc.rs`, inside `render_path_never_allocates` before its final summary assertions, append a regions phase:

```rust
    // Regions (5a): streamed from disk with a loop and locates; the disk
    // is serviced outside the armed blocks.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.caf");
    {
        let mut w = core_media::caf::CafWriter::create(
            &path,
            48_000,
            2,
            core_model::format::BitDepth::Float32,
            core_media::caf::WriteMode::Normal,
        )
        .unwrap();
        w.write(&vec![0.25f32; 2 * 480_000]).unwrap();
        w.finish().unwrap();
    }
    let mut bld = Builder::new();
    let (seq, lane) = bld.sequence("Song", "Audio", None).unwrap();
    let m = bld.media("Media/a.caf", 480_000, 2).unwrap();
    for beat in [0i64, 3, 9] {
        bld.region(
            seq,
            lane,
            beat * 15_360,
            m,
            beat as u64 * 1000,
            100_000,
            -3.0,
            core_model::event::Fade { length_frames: 500, shape: core_model::event::FadeShape::SCurve },
            core_model::event::Fade::default(),
        )
        .unwrap();
    }
    let audio_doc = bld.finish();
    let mut cfg = EngineConfig::new(48_000.0);
    cfg.disk = core_media::stream::DiskMode::Manual;
    cfg.stream = core_media::stream::StreamConfig {
        cache_file_bytes: 0,
        ..Default::default()
    };
    let (mut engine, renderer) = Engine::new(cfg, Box::new(OfflineSinks::new(64))).unwrap();
    engine.set_media(vec![core_media::stream::MediaFile { id: m, path: path.clone() }]);
    engine.load(&audio_doc, seq).unwrap();
    let mut b = OfflineBackend::new(48_000.0, 512);
    b.start(renderer).unwrap();
    b.reserve(4096);
    engine.set_loop_range(Some((10_000, 200_000))).unwrap();
    engine.play().unwrap();
    let before = rt_check::count();
    for i in 0..2_000u32 {
        if i % 500 == 250 {
            engine.locate_frame(u64::from(i) * 97).unwrap();
        }
        engine.streamer().service();
        armed_block(&mut b, 1 + (i * 37) % 1024);
    }
    assert_eq!(rt_check::count() - before, 0, "allocations while playing regions");
```
If it is simpler, put this phase in a separate `#[test]` only when the file's comment ("One test, so no other test thread allocates…") allows it. It does not, so keep it inside the one test.

In `vizdaw-play`, wherever a project package is loaded into an engine (`grep -n "engine.load\|Engine::new" crates/vizdaw-play/src/*.rs`), pass the package's media before `load` (Ruling 24):
```rust
engine.set_media(
    doc.media()
        .values()
        .map(|m| core_media::stream::MediaFile { id: m.id, path: package.join(&m.path) })
        .collect(),
);
```
`package` is the `.vizdaw` path the CLI opened. Skip this for documents built in memory (the demo). Add `core-media = { path = "../core-media" }` to `crates/vizdaw-play/Cargo.toml`. The engine opens files lazily, and a file that is not a readable CAF fails once and reads as offline, which is what an old WAV-referencing project gets until it is converted in the app.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-engine --test audio_playback 2>&1 | tail -5`
Expected: `test result: ok. 8 passed`.

Run: `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph 2>&1 | tail -4`
Expected: ok.

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c`
Expected: all ok. Existing engine tests now start a disk thread per engine (default `Thread`), which is harmless. If a test counts threads, it says so; adapt it.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
git add crates/core-engine crates/vizdaw-play Cargo.lock
git commit -m "engine: stream regions from disk while playing

The engine owns the streamer; Play waits up to 250 ms for it, Locate
hints it, loops are passed on. The renderer bumps the streamer's epoch
only between runs, cuts runs at loop and sequence ends while audio plays,
and marks misses within 250 ms of Play/Locate as late starts. New faults
stream-underrun and late-start; offline media compile to silent
segments. vizdaw-play passes a package's media.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 10: `core-ffi` — media root, offline media, import jobs, media rows

**Files:**
- Create: `crates/core-ffi/src/media.rs`, `crates/core-ffi/src/jobs.rs`, `crates/core-ffi/tests/media.rs`
- Modify: `crates/core-ffi/Cargo.toml`, `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/host.rs`, `crates/core-ffi/src/snapshot.rs` (`IdleUpdate.media_changed`), `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `Cargo.lock`
- Test: `crates/core-ffi/tests/media.rs`

**Interfaces:**
- Consumes:
  - Task 2's `MediaCmd::Add`, `AudioCmd::Insert` and `LaneCmd::Add`;
  - Task 4's `import_file`, `Imported` and `CafImporter`; Task 5's `ExtAudioImporter`;
  - Task 9's `Engine::{set_media, take_stream_errors}`.
- Produces (UniFFI):
  - `MediaRow { id: u64, name: String, path: String, channels: u16, frames: u64, sample_rate: u32, origin: MediaOriginRow, original_name: Option<String>, offline: Option<OfflineReasonRow>, uses: u32 }`;
  - `MediaOriginRow::{Imported, Recorded, Bounced, ExternalEdit}`;
  - `OfflineReasonRow::{Missing, Unreadable, Format, Length}`;
  - `ImportTargetRow { seq: u64, lane: Option<u64>, tick: i64 }`;
  - `JobRow { id: u64, kind: JobKindRow, progress: f32, state: JobStateRow, message: Option<String>, problems: Vec<String>, created: Vec<u64> }`;
  - `JobKindRow::{Import, Convert, Format}`, `JobStateRow::{Running, Done, Failed, Cancelled}`;
  - `Session::{media() -> Vec<MediaRow>, recheck_media() -> bool, import_audio(paths: Vec<String>, target: Option<ImportTargetRow>) -> u64, job(id: u64) -> JobRow, cancel_job(id: u64)}`;
  - `IdleUpdate.media_changed: bool` (`#[uniffi(default = false)]`).
- Internal:
  - `media::{scratch_root, check, files, rows, copy_media, importer}`;
  - `jobs::{Jobs, Work}`;
  - `host::{set_media, forget_media, take_stream_errors}`;
  - `Host::load(owner, …)`.

Rulings 14, 15, 19 and 22.
- **Media root.** A session's media root is its package, or a scratch package under the temp dir while it is untitled.
- **Offline set.** The offline set is recomputed whenever the document's media pool or format changes, the root changes, or the app asks (`recheck_media`). The online items go to the engine host's media table, keyed by session, which `Host::load` hands the engine before every load.
- **Imports.** Imports run on a job thread. The next `idle` or `job` applies a finished job under the lock as one undo step, "Import Audio": the media items, plus the regions with new lanes where Ruling 19 says.
- **Saving.** Save and Save As copy the media files the destination lacks. The scratch package goes once the document has a real path, and at `close`.

- [ ] **Step 1: Write the failing tests**

In `crates/core-ffi/Cargo.toml` under `[dependencies]` add `core-media = { path = "../core-media" }`.

Create `crates/core-ffi/tests/media.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/media.rs <<'VIZDAW_EOF'
//! Media in the session (plan 5a Task 10): imports as jobs, the media
//! root (scratch while untitled), Save As copying media, offline media.

use vizdaw_ffi::{ImportTargetRow, JobKindRow, JobRow, JobStateRow, OfflineReasonRow, Session};
use core_media::caf::{CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A 48 kHz float CAF source of `frames` frames (the importer reads CAF on
/// every platform).
fn source(dir: &Path, name: &str, frames: usize, ch: u16) -> PathBuf {
    let p = dir.join(name);
    let mut w = CafWriter::create(&p, 48_000, ch, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25f32; frames * usize::from(ch)]).unwrap();
    w.finish().unwrap();
    p
}

fn wait(s: &Session, id: u64) -> JobRow {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            return j;
        }
        assert!(Instant::now() < until, "job {id} still running");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn import(s: &Arc<Session>, paths: &[&Path], target: Option<ImportTargetRow>) -> JobRow {
    let id = s
        .import_audio(paths.iter().map(|p| p.display().to_string()).collect(), target)
        .unwrap();
    wait(s, id)
}

#[test]
fn an_untitled_import_lands_in_the_scratch_package() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "kick.caf", 4800, 2);
    let s = Session::new_empty().unwrap();
    let j = import(&s, &[&src], None);
    assert_eq!((j.kind, j.state), (JobKindRow::Import, JobStateRow::Done), "{j:?}");
    assert_eq!(j.created.len(), 1);
    let rows = s.media().unwrap();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!((r.name.as_str(), r.path.as_str(), r.channels, r.frames, r.offline), ("kick", "Media/kick.caf", 2, 4800, None));
    assert_eq!(r.original_name.as_deref(), Some("kick.caf"));
    assert_eq!(r.uses, 0);
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Import Audio"));
    s.close().unwrap();
}

#[test]
fn importing_the_same_file_twice_makes_two_items() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "a.caf", 100, 1);
    let s = Session::new_empty().unwrap();
    import(&s, &[&src], None);
    import(&s, &[&src], None);
    let names: Vec<String> = s.media().unwrap().into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["a".to_string(), "a 2".to_string()]);
    s.close().unwrap();
}

#[test]
fn a_bad_file_fails_alone_and_the_batch_imports() {
    let dir = tempfile::tempdir().unwrap();
    let good = source(dir.path(), "good.caf", 100, 2);
    let bad = dir.path().join("bad.wav");
    std::fs::write(&bad, "not audio").unwrap();
    let s = Session::new_empty().unwrap();
    let j = import(&s, &[&bad, &good], None);
    assert_eq!(j.state, JobStateRow::Done);
    assert_eq!(j.created.len(), 1);
    assert!(j.problems.iter().any(|p| p.starts_with("bad.wav: ")), "{:?}", j.problems);
    s.close().unwrap();
}

#[test]
fn importing_into_a_sequence_adds_a_lane_and_a_region_in_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "loop.caf", 48_000, 2);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lanes_before = s.lanes(seq).unwrap().len();
    let j = import(&s, &[&src], Some(ImportTargetRow { seq, lane: None, tick: 15_360 }));
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let lanes = s.lanes(seq).unwrap();
    assert_eq!(lanes.len(), lanes_before + 1);
    let lane = lanes.last().unwrap();
    assert_eq!(lane.name, format!("Audio {}", lanes_before + 1));
    let ev = s.lane_events(seq, lane.id).unwrap();
    assert_eq!(ev.events.len(), 1);
    assert_eq!((ev.events[0].tick, ev.events[0].frames), (15_360, 48_000));
    assert_eq!(s.media().unwrap().iter().filter(|r| r.name == "loop").count(), 1);
    assert_eq!(s.media().unwrap().iter().find(|r| r.name == "loop").unwrap().uses, 1);
    s.undo().unwrap();
    assert_eq!(s.lanes(seq).unwrap().len(), lanes_before);
    assert!(s.media().unwrap().iter().all(|r| r.name != "loop"));
    s.close().unwrap();
}

#[test]
fn importing_into_a_selected_lane_uses_it() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "v.caf", 1000, 1);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lane = s.lanes(seq).unwrap()[0].id;
    let before = s.lane_events(seq, lane).unwrap().events.len();
    let j = import(&s, &[&src], Some(ImportTargetRow { seq, lane: Some(lane), tick: -50 }));
    assert_eq!(j.state, JobStateRow::Done);
    let ev = s.lane_events(seq, lane).unwrap();
    assert_eq!(ev.events.len(), before + 1);
    // A negative tick imports at 0.
    assert!(ev.events.iter().any(|e| e.tick == 0 && e.frames == 1000));
    s.close().unwrap();
}

#[test]
fn save_as_copies_the_media_and_reopens_online() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "pad.caf", 2000, 2);
    let s = Session::new_empty().unwrap();
    import(&s, &[&src], None);
    let pkg = dir.path().join("Song.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    assert!(pkg.join("Media/pad.caf").is_file());
    s.close().unwrap();
    let again = Session::open(pkg.display().to_string()).unwrap();
    let rows = again.media().unwrap();
    assert_eq!(rows[0].offline, None);
    // Save As elsewhere carries the media too.
    let other = dir.path().join("Copy.vizdaw");
    again.save_as(other.display().to_string()).unwrap();
    assert!(other.join("Media/pad.caf").is_file());
    again.close().unwrap();
}

#[test]
fn missing_and_changed_files_are_offline_until_rechecked() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "x.caf", 500, 1);
    let s = Session::new_empty().unwrap();
    import(&s, &[&src], None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.close().unwrap();
    let file = pkg.join("Media/x.caf");
    let kept = dir.path().join("kept.caf");
    std::fs::rename(&file, &kept).unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    assert_eq!(s.media().unwrap()[0].offline, Some(OfflineReasonRow::Missing));
    // Something else under its name: unreadable.
    std::fs::write(&file, "nope").unwrap();
    assert!(s.recheck_media().unwrap());
    assert_eq!(s.media().unwrap()[0].offline, Some(OfflineReasonRow::Unreadable));
    // A CAF of another length.
    std::fs::remove_file(&file).unwrap();
    source(&pkg.join("Media"), "x.caf", 400, 1);
    assert!(s.recheck_media().unwrap());
    assert_eq!(s.media().unwrap()[0].offline, Some(OfflineReasonRow::Length));
    // The real file back: online, and a second recheck changes nothing.
    std::fs::remove_file(&file).unwrap();
    std::fs::rename(&kept, &file).unwrap();
    assert!(s.recheck_media().unwrap());
    assert_eq!(s.media().unwrap()[0].offline, None);
    assert!(!s.recheck_media().unwrap());
    s.close().unwrap();
}

#[test]
fn a_cancelled_import_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path(), "long.caf", 2_000_000, 2);
    let s = Session::new_empty().unwrap();
    let id = s.import_audio(vec![src.display().to_string()], None).unwrap();
    s.cancel_job(id).unwrap();
    let j = wait(&s, id);
    assert!(matches!(j.state, JobStateRow::Cancelled | JobStateRow::Done), "{j:?}");
    if j.state == JobStateRow::Cancelled {
        assert!(s.media().unwrap().is_empty());
    }
    s.close().unwrap();
}
VIZDAW_EOF
```

The cancel test accepts `Done`, because a fast machine may finish before the cancel lands. Every other assertion is deterministic.

Check the names these tests use against the tree: `EventRow.frames` (a region row's frames, `events.rs`), `LaneRow.name`/`id`, `DocSnapshot.undo_label`, and `Session::lane_events(seq, lane)` returning `LaneEvents { events, … }`. If a field is named differently, adapt the test.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-ffi --test media 2>&1 | tail -5`
Expected: compile errors (`ImportTargetRow`, `Session::import_audio` not found).

- [ ] **Step 3: Implement**

Create `crates/core-ffi/src/media.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/media.rs <<'VIZDAW_EOF'
//! Media in a session (sub-project 5 spec §4.3, §5.1; plan 5a Rulings 14,
//! 19, 22): where the files live, which are offline and why, the rows the
//! app shows, and copying files into a package on save.

use crate::error::VizError;
use core_media::caf::CafReader;
use core_media::import::{CafImporter, MediaImporter};
use core_media::stream::MediaFile;
use core_model::document::{Document, MediaOrigin};
use core_model::event::EventBody;
use core_model::ids::MediaId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Why a media item cannot play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OfflineReasonRow {
    /// The file is not there.
    Missing,
    /// The file is not a readable VizDAW CAF.
    Unreadable,
    /// The file is in another rate or depth than the project.
    Format,
    /// The file's length differs from the item's.
    Length,
}

/// Where a media item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MediaOriginRow {
    /// Imported from a file.
    Imported,
    /// Recorded.
    Recorded,
    /// A bounce.
    Bounced,
    /// Returned by the external editor.
    ExternalEdit,
}

/// One media-pool item.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MediaRow {
    /// Media ID.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Package-relative path.
    pub path: String,
    /// Channels.
    pub channels: u16,
    /// Length in frames.
    pub frames: u64,
    /// Sample rate.
    pub sample_rate: u32,
    /// Origin.
    pub origin: MediaOriginRow,
    /// The imported file's name.
    pub original_name: Option<String>,
    /// Why it is offline (`None`: online).
    pub offline: Option<OfflineReasonRow>,
    /// Regions using it (any take).
    pub uses: u32,
}

/// Where an untitled session keeps its media (Ruling 14).
pub fn scratch_root(session: u64) -> PathBuf {
    std::env::temp_dir()
        .join("VizDAW-untitled")
        .join(format!("{}-{session}.vizdaw", std::process::id()))
}

/// The offline items of `doc` under `root`, with why (Ruling 22; the
/// content hash is not re-checked here, Ruling 4).
pub fn check(doc: &Document, root: &Path) -> BTreeMap<MediaId, OfflineReasonRow> {
    let f = doc.settings().format;
    let mut out = BTreeMap::new();
    for m in doc.media().values() {
        let p = root.join(&m.path);
        let reason = if !p.is_file() {
            Some(OfflineReasonRow::Missing)
        } else {
            match CafReader::open(&p) {
                Err(_) => Some(OfflineReasonRow::Unreadable),
                Ok(r) => {
                    let i = r.info();
                    if i.sample_rate != f.sample_rate || i.depth != f.depth {
                        Some(OfflineReasonRow::Format)
                    } else if i.frames != m.length_frames || i.channels != m.channels {
                        Some(OfflineReasonRow::Length)
                    } else {
                        None
                    }
                }
            }
        };
        if let Some(r) = reason {
            out.insert(m.id, r);
        }
    }
    out
}

/// The online items as the engine's media table.
pub fn files(
    doc: &Document,
    root: &Path,
    offline: &BTreeMap<MediaId, OfflineReasonRow>,
) -> Vec<MediaFile> {
    doc.media()
        .values()
        .filter(|m| !offline.contains_key(&m.id))
        .map(|m| MediaFile {
            id: m.id,
            path: root.join(&m.path),
        })
        .collect()
}

/// The media pool as rows.
pub fn rows(doc: &Document, offline: &BTreeMap<MediaId, OfflineReasonRow>) -> Vec<MediaRow> {
    let mut uses: BTreeMap<MediaId, u32> = BTreeMap::new();
    for (_, e) in doc.all_events() {
        if let EventBody::AudioRegion { media, .. } = e.body {
            *uses.entry(media).or_default() += 1;
        }
    }
    doc.media()
        .values()
        .map(|m| {
            let (origin, original_name) = match &m.origin {
                MediaOrigin::Imported { original_name } => (
                    MediaOriginRow::Imported,
                    Some(original_name.clone()).filter(|n| !n.is_empty()),
                ),
                MediaOrigin::Recorded { .. } => (MediaOriginRow::Recorded, None),
                MediaOrigin::Bounced => (MediaOriginRow::Bounced, None),
                MediaOrigin::ExternalEdit { .. } => (MediaOriginRow::ExternalEdit, None),
            };
            MediaRow {
                id: m.id.0,
                name: m.name.clone(),
                path: m.path.clone(),
                channels: m.channels,
                frames: m.length_frames,
                sample_rate: m.sample_rate,
                origin,
                original_name,
                offline: offline.get(&m.id).copied(),
                uses: uses.get(&m.id).copied().unwrap_or(0),
            }
        })
        .collect()
}

/// Copies every media file of `doc` that `to` lacks from `from` (Ruling
/// 14). Missing sources are skipped (they stay offline).
pub fn copy_media(doc: &Document, from: &Path, to: &Path) -> Result<(), VizError> {
    if from == to {
        return Ok(());
    }
    for m in doc.media().values() {
        let (src, dst) = (from.join(&m.path), to.join(&m.path));
        if dst.exists() || !src.is_file() {
            continue;
        }
        if let Some(dir) = dst.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        // A hard link when both are on one volume; else a copy.
        if std::fs::hard_link(&src, &dst).is_err() {
            std::fs::copy(&src, &dst).map_err(io)?;
        }
    }
    Ok(())
}

fn io(e: std::io::Error) -> VizError {
    VizError::Io {
        message: e.to_string(),
    }
}

/// The platform importer: ExtAudioFile on macOS, CAF only elsewhere.
pub fn importer() -> Box<dyn MediaImporter> {
    #[cfg(target_os = "macos")]
    {
        Box::new(platform_macos::media::ExtAudioImporter)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(CafImporter)
    }
}

/// The CAF importer, for depth-only conversions on any platform.
pub fn caf_importer() -> Box<dyn MediaImporter> {
    Box::new(CafImporter)
}
VIZDAW_EOF
```

Create `crates/core-ffi/src/jobs.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/jobs.rs <<'VIZDAW_EOF'
//! Background jobs (plan 5a Ruling 15): imports and conversions run on
//! their own threads with progress and cancellation; the session applies
//! a finished job's result under its lock, as one undo step.

use crate::error::VizError;
use core_media::import::Imported;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JobKindRow {
    /// Import Audio.
    Import,
    /// Convert media to the project format.
    Convert,
    /// Change the project format.
    Format,
}

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum JobStateRow {
    /// Working.
    Running,
    /// Applied.
    Done,
    /// Nothing changed; `message` says why.
    Failed,
    /// Cancelled; nothing changed.
    Cancelled,
}

/// A job as the app sees it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct JobRow {
    /// Job ID.
    pub id: u64,
    /// Kind.
    pub kind: JobKindRow,
    /// 0–1.
    pub progress: f32,
    /// State.
    pub state: JobStateRow,
    /// Why it failed.
    pub message: Option<String>,
    /// Files that failed while the rest went on ("name: why").
    pub problems: Vec<String>,
    /// Media items it created.
    pub created: Vec<u64>,
}

/// Where imported audio goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ImportTargetRow {
    /// Sequence.
    pub seq: u64,
    /// Lane (`None`: new lanes; Ruling 19).
    pub lane: Option<u64>,
    /// Tick (a negative tick imports at 0).
    pub tick: i64,
}

/// A job thread's result, applied by the session.
#[derive(Debug)]
pub enum Work {
    /// Per source file: what it became, or why it failed.
    Import {
        /// `(file name, result)` per source, in order.
        files: Vec<(String, Result<Vec<Imported>, String>)>,
        /// Where regions go.
        target: Option<ImportTargetRow>,
    },
    /// Converted files per media item (Task 11): `(item, result)`.
    Convert {
        /// The kind (Convert or Format).
        kind: JobKindRow,
        /// The format change, if any.
        format: Option<core_model::format::AudioFormat>,
        /// `(media, converted file or error)`.
        files: Vec<(core_model::ids::MediaId, Result<Imported, String>)>,
    },
    /// Cancelled before it finished (whatever it wrote is removed).
    Cancelled,
}

struct Job {
    kind: JobKindRow,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<Work>>,
    row: Option<JobRow>,
}

/// A session's jobs.
#[derive(Default)]
pub struct Jobs {
    next: u64,
    list: BTreeMap<u64, Job>,
}

impl std::fmt::Debug for Jobs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jobs").field("count", &self.list.len()).finish()
    }
}

impl Jobs {
    /// Starts `work` on a thread; it gets a progress cell (f32 bits) and
    /// the cancel flag.
    pub fn spawn(
        &mut self,
        kind: JobKindRow,
        work: impl FnOnce(&AtomicU32, &AtomicBool) -> Work + Send + 'static,
    ) -> Result<u64, VizError> {
        self.next += 1;
        let id = self.next;
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c) = (Arc::clone(&progress), Arc::clone(&cancel));
        let thread = std::thread::Builder::new()
            .name(format!("vizdaw-job-{id}"))
            .spawn(move || work(&p, &c))
            .map_err(|e| VizError::Io {
                message: format!("job thread: {e}"),
            })?;
        self.list.insert(
            id,
            Job {
                kind,
                progress,
                cancel,
                thread: Some(thread),
                row: None,
            },
        );
        Ok(id)
    }

    /// Asks job `id` to stop.
    pub fn cancel(&self, id: u64) {
        if let Some(j) = self.list.get(&id) {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The finished jobs' work, to apply (each once; a panicked thread
    /// becomes a failure row at once).
    pub fn finished(&mut self) -> Vec<(u64, JobKindRow, Work)> {
        let mut out = Vec::new();
        for (id, j) in self.list.iter_mut() {
            if j.thread.as_ref().is_some_and(JoinHandle::is_finished) {
                match j.thread.take().map(JoinHandle::join) {
                    Some(Ok(w)) => out.push((*id, j.kind, w)),
                    _ => {
                        j.row = Some(row(*id, j.kind, JobStateRow::Failed, Some("the job panicked".into()), vec![], vec![]));
                    }
                }
            }
        }
        out
    }

    /// Records how job `id` ended.
    pub fn finish(&mut self, id: u64, row: JobRow) {
        if let Some(j) = self.list.get_mut(&id) {
            j.row = Some(row);
        }
    }

    /// Job `id` as a row.
    pub fn row(&self, id: u64) -> Option<JobRow> {
        let j = self.list.get(&id)?;
        Some(j.row.clone().unwrap_or_else(|| {
            let p = f32::from_bits(j.progress.load(Ordering::Relaxed));
            let mut r = row(id, j.kind, JobStateRow::Running, None, vec![], vec![]);
            r.progress = p;
            r
        }))
    }

    /// Cancels every job and waits for the threads (the session closes).
    pub fn shutdown(&mut self) -> Vec<Work> {
        for j in self.list.values() {
            j.cancel.store(true, Ordering::Relaxed);
        }
        self.list
            .values_mut()
            .filter_map(|j| j.thread.take()?.join().ok())
            .collect()
    }
}

/// A row with every field given.
pub fn row(
    id: u64,
    kind: JobKindRow,
    state: JobStateRow,
    message: Option<String>,
    problems: Vec<String>,
    created: Vec<u64>,
) -> JobRow {
    JobRow {
        id,
        kind,
        progress: if state == JobStateRow::Running { 0.0 } else { 1.0 },
        state,
        message,
        problems,
        created,
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod input;
pub mod keys;
```
Replace:
```rust
pub mod input;
pub mod jobs;
pub mod keys;
pub mod media;
```

Find:
```rust
pub use input::{NoteInRow, NoteInput};
```
Replace:
```rust
pub use input::{NoteInRow, NoteInput};
pub use jobs::{ImportTargetRow, JobKindRow, JobRow, JobStateRow};
pub use media::{MediaOriginRow, MediaRow, OfflineReasonRow};
```

In `crates/core-ffi/src/snapshot.rs`:

Find:
```rust
    /// New problems: plugins that failed to load or refused their saved
    /// state.
    pub problems: Vec<String>,
}
```
Replace:
```rust
    /// New problems: plugins that failed to load or refused their saved
    /// state; media that failed to read.
    pub problems: Vec<String>,
    /// The media rows changed (a job finished, media went offline):
    /// refetch `Session.media`.
    #[uniffi(default = false)]
    pub media_changed: bool,
}
```
Every Rust construction of `IdleUpdate` gets `media_changed: false`, except the one in `idle` (below).

In `crates/core-ffi/src/host.rs`, add the per-session media tables and pass them to the engine:

```rust
/// Each session's online media, handed to the engine before every load
/// (5a Task 10).
static MEDIA: Mutex<BTreeMap<u64, Vec<core_media::stream::MediaFile>>> =
    Mutex::new(BTreeMap::new());

/// Sets session `owner`'s online media; reloads use it.
pub fn set_media(owner: u64, files: Vec<core_media::stream::MediaFile>) {
    lock(&MEDIA).insert(owner, files);
}

/// Forgets session `owner`'s media (it closed).
pub fn forget_media(owner: u64) {
    lock(&MEDIA).remove(&owner);
}

fn media_of(owner: u64) -> Vec<core_media::stream::MediaFile> {
    lock(&MEDIA).get(&owner).cloned().unwrap_or_default()
}

/// Media whose reads failed while `owner` played (each once).
pub fn take_stream_errors(owner: u64) -> Vec<(core_model::ids::MediaId, String)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| h.engine.take_stream_errors())
        .unwrap_or_default()
}
```
(add `use std::collections::BTreeMap;`).

Find:
```rust
    fn load(&mut self, doc: &Document, seq: SeqId, looped: bool) -> Result<(), VizError> {
        LOADS.fetch_add(1, Ordering::Relaxed);
        self.plugins_in_engine |= has_slots(doc);
        let report = self.engine.load(doc, seq)?;
```
Replace:
```rust
    fn load(&mut self, owner: u64, doc: &Document, seq: SeqId, looped: bool) -> Result<(), VizError> {
        LOADS.fetch_add(1, Ordering::Relaxed);
        self.plugins_in_engine |= has_slots(doc);
        self.engine.set_media(media_of(owner));
        let report = self.engine.load(doc, seq)?;
```
and pass the owner at the four call sites:
- `new.load(owner, doc, seq, looped)?;` in `rebuild` (the old host's owner is in scope there);
- `host.load(owner, doc, seq, looped)?;` twice in `play`;
- `h.load(owner, doc, seq, looped)` in `reload`.

`plugins.service(&mut h.engine, …)` reloads on its own. The engine keeps the media table of the last `set_media`, so that load sees it too.

In `crates/core-ffi/src/session.rs`, `Inner` gets:

```rust
    /// Offline media and why (Ruling 22; runtime only, never saved).
    offline: std::collections::BTreeMap<core_model::ids::MediaId, crate::media::OfflineReasonRow>,
    /// The document whose media and format `offline` was computed for, and
    /// the root it was checked under.
    media_seen: Option<(Document, std::path::PathBuf)>,
    /// Imports and conversions (Ruling 15).
    jobs: crate::jobs::Jobs,
```
initialised in `Inner::new` with `offline: Default::default(), media_seen: None, jobs: Default::default(),`.

Add these `Inner` methods:

```rust
    /// The media root: the package, or the untitled scratch package.
    fn root(&self, id: u64) -> std::path::PathBuf {
        self.path
            .clone()
            .unwrap_or_else(|| crate::media::scratch_root(id))
    }

    /// Recomputes the offline set when the media pool, the format or the
    /// root changed (or `force`), and hands the online media to the
    /// engine host. True when the offline set changed.
    fn refresh_media(&mut self, id: u64, force: bool) -> bool {
        let root = self.root(id);
        let doc = self.history.doc();
        let same = !force
            && self.media_seen.as_ref().is_some_and(|(d, r)| {
                *r == root
                    && d.settings().format == doc.settings().format
                    && (d.media().ptr_eq(doc.media()) || d.media() == doc.media())
            });
        if same {
            return false;
        }
        let offline = crate::media::check(doc, &root);
        let changed = offline != self.offline;
        host::set_media(id, crate::media::files(doc, &root, &offline));
        self.offline = offline;
        self.media_seen = Some((doc.clone(), root));
        changed
    }
```

At the start of `Inner::reload_host`:

Find:
```rust
    fn reload_host(&mut self, id: u64) {
        // The marker names any plugin the sync starts loading first.
        self.mark(id, true);
```
Replace:
```rust
    fn reload_host(&mut self, id: u64) {
        self.refresh_media(id, false);
        // The marker names any plugin the sync starts loading first.
        self.mark(id, true);
```

In `Session::with`, after `inner.mark(id, false);`, add `inner.refresh_media(id, true);`.

Saving copies media. In `save_to`:

Find:
```rust
        let captured = self.capture_all(inner)?;
        let _g = lock(&SAVE_LOCK);
        file::save(inner.doc(), path)?;
```
Replace:
```rust
        let captured = self.capture_all(inner)?;
        let _g = lock(&SAVE_LOCK);
        crate::media::copy_media(inner.doc(), &inner.root(self.id), path)?;
        file::save(inner.doc(), path)?;
```

In `save_as`, after `inner.path = Some(p);` add:
```rust
                // The scratch package of an untitled session is done with.
                let _ = std::fs::remove_dir_all(crate::media::scratch_root(self.id));
                inner.refresh_media(self.id, true);
                inner.reload_host(self.id);
```

In `close`, after `inner.closed = true;` add:
```rust
            let root = inner.root(self.id);
            for w in inner.jobs.shutdown() {
                discard(&root, w);
            }
            host::forget_media(self.id);
            if inner.path.is_none() {
                let _ = std::fs::remove_dir_all(crate::media::scratch_root(self.id));
            }
```

Now the job methods and their application. Add to `session.rs`:

```rust
/// Removes the files of a job that was never applied (`root` is the
/// session's media root).
pub(crate) fn discard(root: &Path, w: crate::jobs::Work) {
    use crate::jobs::Work;
    let files: Vec<String> = match w {
        Work::Import { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .flatten()
            .map(|i| i.path)
            .collect(),
        Work::Convert { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .map(|i| i.path)
            .collect(),
        Work::Cancelled => Vec::new(),
    };
    for f in files {
        let _ = std::fs::remove_file(root.join(f));
    }
}
```

The apply step:

```rust
impl Session {
    /// Applies every finished job (one undo step each); true when the
    /// document changed.
    fn apply_jobs(&self, inner: &mut Inner) -> bool {
        let mut changed = false;
        for (id, kind, work) in inner.jobs.finished() {
            let root = inner.root(self.id);
            let row = match work {
                crate::jobs::Work::Cancelled => {
                    crate::jobs::row(id, kind, crate::jobs::JobStateRow::Cancelled, None, vec![], vec![])
                }
                crate::jobs::Work::Import { files, target } => {
                    match apply_import(inner, &root, files, target) {
                        Ok((created, problems)) => {
                            changed |= !created.is_empty();
                            crate::jobs::row(id, kind, crate::jobs::JobStateRow::Done, None, problems, created)
                        }
                        Err((e, problems)) => crate::jobs::row(
                            id,
                            kind,
                            crate::jobs::JobStateRow::Failed,
                            Some(e.to_string()),
                            problems,
                            vec![],
                        ),
                    }
                }
                // Task 11.
                w @ crate::jobs::Work::Convert { .. } => {
                    discard(&root, w);
                    crate::jobs::row(id, kind, crate::jobs::JobStateRow::Failed, Some("not yet".into()), vec![], vec![])
                }
            };
            inner.jobs.finish(id, row);
        }
        if changed {
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(inner);
        }
        changed
    }
}

/// One finished import (Ruling 19): the media items and, with a target,
/// their regions (into the given lane for a single mono/stereo file, else
/// one new "Audio n" lane per item), as one undo step "Import Audio".
/// On an error nothing is applied and the written files are removed.
fn apply_import(
    inner: &mut Inner,
    root: &Path,
    files: Vec<(String, Result<Vec<core_media::import::Imported>, String>)>,
    target: Option<crate::jobs::ImportTargetRow>,
) -> Result<(Vec<u64>, Vec<String>), (VizError, Vec<String>)> {
    use core_model::command::{AudioCmd, LaneCmd, MediaCmd};
    use core_model::document::MediaOrigin;
    use core_model::event::Fade;
    let mut problems = Vec::new();
    let mut items = Vec::new();
    for (name, r) in files {
        match r {
            Ok(v) => items.push(v),
            Err(e) => problems.push(format!("{name}: {e}")),
        }
    }
    let written: Vec<std::path::PathBuf> = items.iter().flatten().map(|i| root.join(&i.path)).collect();
    if items.is_empty() {
        return Ok((Vec::new(), problems));
    }
    let single = items.len() == 1 && items.first().is_some_and(|v| v.len() == 1);
    let filter = ViewFilter::default();
    let h = &mut inner.history;
    let fail = |e: VizError, problems: Vec<String>| {
        for p in &written {
            let _ = std::fs::remove_file(p);
        }
        (e, problems)
    };
    if let Err(e) = h.begin("Import Audio") {
        return Err(fail(e.into(), problems));
    }
    let run = (|| -> Result<Vec<u64>, VizError> {
        let mut created = Vec::new();
        for item in items.iter().flatten() {
            let r = h.execute(
                &Command::Media(MediaCmd::Add {
                    path: item.path.clone(),
                    sample_rate: item.sample_rate,
                    channels: item.channels,
                    length_frames: item.length_frames,
                    content_hash: Some(item.content_hash.clone()),
                    name: Some(item.name.clone()),
                    origin: Some(MediaOrigin::Imported {
                        original_name: item.original_name.clone(),
                    }),
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
                .ok_or_else(|| VizError::Internal { message: "no media created".into() })?;
            created.push(media.0);
            let Some(t) = target else { continue };
            let seq = SeqId(t.seq);
            let lane = match t.lane {
                Some(l) if single => LaneId(l),
                _ => {
                    let n = h.doc().sequence(seq)?.lanes.len() + 1;
                    let r = h.execute(
                        &Command::Lane(LaneCmd::Add { seq, name: format!("Audio {n}"), index: None }),
                        &filter,
                    )?;
                    r.created
                        .iter()
                        .find_map(|id| match id {
                            core_model::ids::AnyId::Lane(l) => Some(*l),
                            _ => None,
                        })
                        .ok_or_else(|| VizError::Internal { message: "no lane created".into() })?
                }
            };
            h.execute(
                &Command::Audio(AudioCmd::Insert {
                    seq,
                    lane,
                    tick: Tick(t.tick.max(0)),
                    media,
                    source_offset: 0,
                    length: item.length_frames,
                    gain_db: 0.0,
                    fade_in: Fade::default(),
                    fade_out: Fade::default(),
                }),
                &filter,
            )?;
        }
        Ok(created)
    })();
    match run {
        Ok(created) => match h.commit() {
            Ok(()) => Ok((created, problems)),
            Err(e) => Err(fail(e.into(), problems)),
        },
        Err(e) => {
            let _ = h.abort();
            Err(fail(e, problems))
        }
    }
}
```

`written` is computed before `h` borrows the history, and `fail` only reads `written`, so the closures borrow disjoint things. If the compiler still objects, inline `fail` and keep the behaviour: on any error the transaction is aborted and every written file is removed.

The exported methods (in an `#[uniffi::export] impl Session` block):

```rust
    /// The media pool, with offline items and use counts.
    pub fn media(&self) -> Result<Vec<crate::media::MediaRow>, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            Ok(crate::media::rows(inner.doc(), &inner.offline))
        })
    }

    /// Checks every media file again (the app became active, or the user
    /// asked); true when the offline set changed.
    pub fn recheck_media(&self) -> Result<bool, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            let changed = inner.refresh_media(self.id, true);
            if changed {
                inner.reload_host(self.id);
            }
            Ok(changed)
        })
    }

    /// Starts importing `paths` (converted into the project format; spec
    /// §5.1) as a job; `target` places regions (Ruling 19). Returns the
    /// job ID; `job` reports it and applies it when done.
    pub fn import_audio(
        &self,
        paths: Vec<String>,
        target: Option<crate::jobs::ImportTargetRow>,
    ) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            if let Some(t) = target {
                let doc = inner.doc();
                doc.sequence(SeqId(t.seq))?;
                if let Some(l) = t.lane {
                    doc.lane(SeqId(t.seq), LaneId(l))?;
                }
            }
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            inner.jobs.spawn(crate::jobs::JobKindRow::Import, move |progress, cancel| {
                let importer = crate::media::importer();
                let n = paths.len().max(1) as f32;
                let mut files = Vec::new();
                for (k, p) in paths.iter().enumerate() {
                    let path = std::path::PathBuf::from(p);
                    let name = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.clone());
                    let r = core_media::import::import_file(
                        importer.as_ref(),
                        &path,
                        &root,
                        format,
                        &mut |f| {
                            progress.store(((k as f32 + f) / n).to_bits(), std::sync::atomic::Ordering::Relaxed)
                        },
                        cancel,
                    );
                    match r {
                        Err(core_media::MediaError::Cancelled) => {
                            // Remove what earlier files of this batch wrote.
                            for (_, done) in &files {
                                if let Ok(items) = done {
                                    for i in items {
                                        let _ = std::fs::remove_file(root.join(&i.path));
                                    }
                                }
                            }
                            return crate::jobs::Work::Cancelled;
                        }
                        r => files.push((name, r.map_err(|e| e.to_string()))),
                    }
                }
                crate::jobs::Work::Import { files, target }
            })
        })
    }

    /// Job `id`, applying it first when it just finished.
    pub fn job(&self, id: u64) -> Result<crate::jobs::JobRow, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            self.apply_jobs(&mut inner);
            inner.jobs.row(id).ok_or_else(|| VizError::Model {
                message: format!("unknown job {id}"),
            })
        })
    }

    /// Asks job `id` to stop (nothing it did is applied).
    pub fn cancel_job(&self, id: u64) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).jobs.cancel(id);
            Ok(())
        })
    }
```

`idle` applies jobs and collects stream errors:

Find:
```rust
            let changed = self.pump(&mut inner);
            let n = std::mem::take(&mut inner.news);
```
Replace:
```rust
            let changed = self.pump(&mut inner);
            let jobs_changed = self.apply_jobs(&mut inner);
            let mut media_changed = jobs_changed;
            for (m, why) in host::take_stream_errors(self.id) {
                inner
                    .offline
                    .insert(m, crate::media::OfflineReasonRow::Unreadable);
                inner.news.problems.push(format!("media {m} went offline: {why}"));
                media_changed = true;
            }
            if media_changed && !jobs_changed {
                let root = inner.root(self.id);
                let files = crate::media::files(inner.doc(), &root, &inner.offline);
                host::set_media(self.id, files);
                inner.reload_host(self.id);
            }
            let changed = changed || jobs_changed;
            let n = std::mem::take(&mut inner.news);
```
In the `IdleUpdate { … }` it returns, set `media_changed`. Also add `media_changed: false` to the `nothing()` closure's literal.

`reload_host` calls `refresh_media`. That does not undo a stream-error offline mark: the document did not change, so `refresh_media(…, false)` returns early. A later `recheck_media` re-reads the file.

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`, add stubs:
```swift
    var mediaRows: [MediaRow] = []
    var jobRows: [UInt64: JobRow] = [:]
    var imports: [([String], ImportTargetRow?)] = []
    func media() throws -> [MediaRow] { mediaRows }
    func recheckMedia() throws -> Bool { false }
    func importAudio(paths: [String], target: ImportTargetRow?) throws -> UInt64 {
        imports.append((paths, target))
        let id = UInt64(imports.count)
        jobRows[id] = JobRow(id: id, kind: .import, progress: 1, state: .done, message: nil, problems: [], created: [])
        return id
    }
    func job(id: UInt64) throws -> JobRow {
        guard let j = jobRows[id] else { throw VizError.Model(message: "unknown job \(id)") }
        return j
    }
    func cancelJob(id: UInt64) throws {}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-ffi --test media 2>&1 | tail -5`
Expected: `test result: ok. 8 passed`.

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c`
Expected: all ok.

Run: `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -5`
Expected: build ok; tests pass (the count is unchanged; FakeSession compiles).

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-ffi app/Tests/VizDAWAppModelTests/FakeSession.swift Cargo.lock
git commit -m "ffi: media root, offline media, import jobs, media rows

Untitled sessions import into a scratch package; Save and Save As copy
the media a package lacks. The offline set (missing, unreadable, other
format, other length) is recomputed when the pool or format changes and
the online media go to the engine before every load. Imports run as jobs
and apply as one undo step, with new lanes and regions when targeted.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 11: Project format — new projects, format changes, media conversion, device rate

**Files:**
- Create: `crates/core-ffi/src/format.rs`, `crates/core-ffi/tests/format.rs`, `crates/core-ffi/tests/device_rate.rs`
- Modify:
  - `crates/core-ffi/src/{lib.rs,session.rs,host.rs,snapshot.rs}`;
  - `crates/core-engine/src/backend.rs` (`AudioConfig.sample_rate`);
  - `crates/platform-macos/src/hal.rs`;
  - `crates/vizdaw-play/src/live.rs` (the `AudioConfig` literal);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift`.
- Test: `crates/core-ffi/tests/format.rs`, `crates/core-ffi/tests/device_rate.rs`

**Interfaces:**
- Consumes: Task 2's `SettingsCmd::SetFormat` and `MediaCmd::Convert`; Task 4's `import_as`; Task 10's jobs, `media::{check, importer}` and `Inner::{root, refresh_media}`.
- Produces (UniFFI):
  - `BitDepthRow::{Int16, Int24, Float32}`, `ProjectFormatRow { sample_rate: u32, depth: BitDepthRow }`, `ProjectInfoRow { format: ProjectFormatRow, mismatched: u32, rate_mismatch: bool, device_rate: f64 }`;
  - `format_label(format: ProjectFormatRow) -> String` (a free function, e.g. "48 kHz, 24-bit") and `project_rates() -> Vec<u32>`;
  - `Session::new_project(format: ProjectFormatRow) -> Arc<Session>` (constructor);
  - `Session::{project_info() -> ProjectInfoRow, set_project_format(format: ProjectFormatRow) -> u64, convert_media() -> u64}` (job IDs; `JobKindRow::Format`/`Convert`);
  - `TransportState.rate_mismatch: bool` (`#[uniffi(default = false)]`, last).
- Engine side:
  - `AudioConfig.sample_rate: Option<f64>`;
  - `CoreAudioBackend::open` sets the device's nominal rate when asked and waits up to 1 s for it;
  - `host::set_offline_rate` now overrides the project rate (0 = follow the project).

Spec §4.1, §5.1 ("Changing the project format") and §5.6, and Ruling 20.
- **Changing the format.** A format change converts every media item, with the platform importer, into new files named after each item's stem (the old files stay for undo). It applies `SettingsCmd::SetFormat` as one undo step, "Change Project Format". It is refused, with the items named, while any media is missing or has another length (those need relinking, 5b).
- **Convert.** Converts the items whose file is unreadable as a VizDAW CAF or is in another format (an old project's WAVs) into project-format files with `MediaCmd::Convert`. An item that fails stays offline and is listed in the job's problems.
- **Device rate.**
  - The engine host opens at the project rate. A device that keeps another rate leaves regions muted and sets `rate_mismatch`.
  - A project at another rate than the open host drops the host, so the next Play opens at the right rate.
  - Offline, the host runs at the project rate unless the test hook overrides it.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/format.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/format.rs <<'VIZDAW_EOF'
//! Project formats (plan 5a Task 11): new projects, format changes that
//! convert every file and rescale regions, converting an old project's
//! media.

use vizdaw_ffi::{
    BitDepthRow, ImportTargetRow, JobRow, JobStateRow, ProjectFormatRow, Session, format_label,
    project_rates,
};
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn fmt(rate: u32, depth: BitDepthRow) -> ProjectFormatRow {
    ProjectFormatRow { sample_rate: rate, depth }
}

fn caf(dir: &Path, name: &str, rate: u32, frames: usize) -> PathBuf {
    let p = dir.join(name);
    let mut w = CafWriter::create(&p, rate, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![0.25f32; frames * 2]).unwrap();
    w.finish().unwrap();
    p
}

fn wait(s: &Session, id: u64) -> JobRow {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            return j;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The demo with one imported 1 s region at beat 1 in a new lane.
fn demo_with_region(dir: &Path) -> (Arc<Session>, u64, u64) {
    let src = caf(dir, "one.caf", 48_000, 48_000);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let id = s
        .import_audio(vec![src.display().to_string()], Some(ImportTargetRow { seq, lane: None, tick: 15_360 }))
        .unwrap();
    assert_eq!(wait(&s, id).state, JobStateRow::Done);
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    (s, seq, lane)
}

#[test]
fn new_projects_take_their_format() {
    let s = Session::new_project(fmt(44_100, BitDepthRow::Int16)).unwrap();
    let i = s.project_info().unwrap();
    assert_eq!(i.format, fmt(44_100, BitDepthRow::Int16));
    assert_eq!((i.mismatched, i.rate_mismatch), (0, false));
    assert!(s.snapshot().unwrap().undo_label.is_none());
    assert_eq!(format_label(fmt(48_000, BitDepthRow::Int24)), "48 kHz, 24-bit");
    assert_eq!(project_rates(), vec![44_100, 48_000, 88_200, 96_000, 176_400, 192_000]);
    // An imported file is stored in the project's format.
    let dir = tempfile::tempdir().unwrap();
    let src = caf(dir.path(), "a.caf", 44_100, 1000);
    let id = s.import_audio(vec![src.display().to_string()], None).unwrap();
    assert_eq!(wait(&s, id).state, JobStateRow::Done);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let r = CafReader::open(&pkg.join("Media/a.caf")).unwrap();
    assert_eq!((r.info().sample_rate, r.info().depth), (44_100, BitDepth::Int16));
    s.close().unwrap();
}

#[test]
fn a_depth_change_converts_every_file_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let (s, seq, lane) = demo_with_region(dir.path());
    let before = s.media().unwrap();
    let id = s.set_project_format(fmt(48_000, BitDepthRow::Float32)).unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Change Project Format"));
    assert_eq!(s.project_info().unwrap().format, fmt(48_000, BitDepthRow::Float32));
    let after = s.media().unwrap();
    let one = after.iter().find(|m| m.name.starts_with("one")).unwrap();
    assert_eq!(one.offline, None);
    assert_ne!(one.path, before.iter().find(|m| m.name == "one").unwrap().path);
    // Same rate: the region keeps its frames.
    assert_eq!(s.lane_events(seq, lane).unwrap().events[0].frames, 48_000);
    s.undo().unwrap();
    assert_eq!(s.project_info().unwrap().format, fmt(48_000, BitDepthRow::Int24));
    assert_eq!(s.media().unwrap().iter().find(|m| m.name == "one").unwrap().offline, None);
    s.close().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn set_format_rescales_regions_and_undo_restores() {
    let dir = tempfile::tempdir().unwrap();
    let (s, seq, lane) = demo_with_region(dir.path());
    let id = s.set_project_format(fmt(44_100, BitDepthRow::Int16)).unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let one = s.media().unwrap().into_iter().find(|m| m.name.starts_with("one")).unwrap();
    assert_eq!((one.sample_rate, one.offline), (44_100, None));
    assert!(one.frames.abs_diff(44_100) <= 1, "{}", one.frames);
    let ev = &s.lane_events(seq, lane).unwrap().events[0];
    assert!(ev.frames.abs_diff(44_100) <= 1, "{}", ev.frames);
    s.undo().unwrap();
    assert_eq!(s.lane_events(seq, lane).unwrap().events[0].frames, 48_000);
    s.close().unwrap();
}

#[test]
fn the_same_format_and_missing_files_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (s, _, _) = demo_with_region(dir.path());
    assert!(s.set_project_format(fmt(48_000, BitDepthRow::Int24)).is_err());
    // The demo's loop.wav does not exist: a format change must refuse.
    let s2 = Session::new_demo_audio().unwrap();
    let id = s2.set_project_format(fmt(48_000, BitDepthRow::Int16)).unwrap();
    let j = wait(&s2, id);
    assert_eq!(j.state, JobStateRow::Failed);
    assert!(j.message.unwrap_or_default().contains("loop"), "names the file");
    s.close().unwrap();
    s2.close().unwrap();
}

// Rate conversion needs the system importer (Ruling 8).
#[cfg(target_os = "macos")]
#[test]
fn old_media_in_another_format_converts_on_request() {
    let dir = tempfile::tempdir().unwrap();
    // A saved 48 kHz project whose one item was replaced by a 44.1 kHz CAF.
    let (s, seq, lane) = demo_with_region(dir.path());
    let pkg = dir.path().join("Old.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    s.close().unwrap();
    std::fs::remove_file(pkg.join("Media/one.caf")).unwrap();
    caf(&pkg.join("Media"), "one.caf", 44_100, 48_000);
    let s = Session::open(pkg.display().to_string()).unwrap();
    let one = s.media().unwrap().into_iter().find(|m| m.name == "one").unwrap();
    assert_eq!(one.offline, Some(vizdaw_ffi::OfflineReasonRow::Format));
    assert_eq!(s.project_info().unwrap().mismatched, 1);
    let id = s.convert_media().unwrap();
    let j = wait(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Convert Media"));
    let one = s.media().unwrap().into_iter().find(|m| m.name.starts_with("one")).unwrap();
    assert_eq!((one.offline, one.sample_rate), (None, 48_000));
    assert_eq!(s.project_info().unwrap().mismatched, 0);
    // The region keeps its frames: the item said 48 kHz, so no rescale.
    assert_eq!(s.lane_events(seq, lane).unwrap().events[0].frames, 48_000);
    s.close().unwrap();
}
VIZDAW_EOF
```

The last test checks something on purpose: a file whose header disagrees with its item is converted at the item's recorded rate, so the region's frames stay as written. The item's `sample_rate` is the model's claim about the region's timing. The file was at fault and is now converted to the project rate.

Create `crates/core-ffi/tests/device_rate.rs`. It is its own test binary, because the offline-rate hook is process-wide:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/device_rate.rs <<'VIZDAW_EOF'
//! A device that refuses the project rate (plan 5a Ruling 20), simulated
//! with the offline backend's rate hook.

use vizdaw_ffi::Session;

#[test]
fn refused_device_rate_mutes_regions() {
    vizdaw_ffi::host::force_offline();
    vizdaw_ffi::host::set_offline_rate(44_100.0);
    let s = Session::new_demo_audio().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    s.play(seq).unwrap();
    let t = s.transport().unwrap();
    assert!(t.rate_mismatch);
    assert_eq!(t.sample_rate, 44_100.0);
    let i = s.project_info().unwrap();
    assert!(i.rate_mismatch);
    assert_eq!(i.device_rate, 44_100.0);
    s.stop().unwrap();
    // Following the project again: a 44.1 kHz project matches.
    vizdaw_ffi::host::set_offline_rate(0.0);
    vizdaw_ffi::host::reset_host();
    let s2 = Session::new_demo().unwrap();
    s2.play(s2.snapshot().unwrap().sequences[0].id).unwrap();
    let t = s2.transport().unwrap();
    assert!(!t.rate_mismatch);
    assert_eq!(t.sample_rate, 48_000.0);
    s2.stop().unwrap();
    s.close().unwrap();
    s2.close().unwrap();
}
VIZDAW_EOF
```

Check the hook names (`force_offline`, `reset_host`, `set_offline_rate`) against `crates/core-ffi/src/host.rs`. They are `test-hooks` functions under `vizdaw_ffi::host`, as `tests/plugins.rs` calls them. The library crate is `vizdaw_ffi`, so every test imports `vizdaw_ffi::…`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-ffi --test format --test device_rate 2>&1 | tail -5`
Expected: compile errors (`ProjectFormatRow`, `Session::new_project` not found).

- [ ] **Step 3: Implement**

Create `crates/core-ffi/src/format.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/format.rs <<'VIZDAW_EOF'
//! The project's audio format at the FFI (sub-project 5 spec §4.1).

use core_model::format::{AudioFormat, BitDepth};

/// A bit depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BitDepthRow {
    /// 16-bit integer.
    Int16,
    /// 24-bit integer.
    Int24,
    /// 32-bit float.
    Float32,
}

/// A project format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ProjectFormatRow {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Bit depth.
    pub depth: BitDepthRow,
}

/// The project as the format UI shows it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ProjectInfoRow {
    /// The project's format.
    pub format: ProjectFormatRow,
    /// Media items in another format or not VizDAW CAF (the "Convert"
    /// banner).
    pub mismatched: u32,
    /// The device runs at another rate: regions are muted (Ruling 20).
    pub rate_mismatch: bool,
    /// The device's rate (0 before the first Play).
    pub device_rate: f64,
}

impl From<ProjectFormatRow> for AudioFormat {
    fn from(f: ProjectFormatRow) -> AudioFormat {
        AudioFormat {
            sample_rate: f.sample_rate,
            depth: match f.depth {
                BitDepthRow::Int16 => BitDepth::Int16,
                BitDepthRow::Int24 => BitDepth::Int24,
                BitDepthRow::Float32 => BitDepth::Float32,
            },
        }
    }
}

impl From<AudioFormat> for ProjectFormatRow {
    fn from(f: AudioFormat) -> ProjectFormatRow {
        ProjectFormatRow {
            sample_rate: f.sample_rate,
            depth: match f.depth {
                BitDepth::Int16 => BitDepthRow::Int16,
                BitDepth::Int24 => BitDepthRow::Int24,
                BitDepth::Float32 => BitDepthRow::Float32,
            },
        }
    }
}

/// Display text, e.g. "44.1 kHz, 16-bit".
#[uniffi::export]
pub fn format_label(format: ProjectFormatRow) -> String {
    AudioFormat::from(format).label()
}

/// The allowed project rates.
#[uniffi::export]
pub fn project_rates() -> Vec<u32> {
    AudioFormat::RATES.to_vec()
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs` add `pub mod format;` and `pub use format::{BitDepthRow, ProjectFormatRow, ProjectInfoRow, format_label, project_rates};`.

In `crates/core-ffi/src/snapshot.rs`:

Find:
```rust
    /// Tempo at `tick` in ticks per second.
    pub ticks_per_second: f64,
}
```
Replace:
```rust
    /// Tempo at `tick` in ticks per second.
    pub ticks_per_second: f64,
    /// The device runs at another rate than the project: regions are
    /// muted (5a Ruling 20).
    #[uniffi(default = false)]
    pub rate_mismatch: bool,
}
```
In `Session::transport`, both literals get `rate_mismatch`: `h.rate_mismatch` from `HostState` (below), or `false` with no host.

In `crates/core-engine/src/backend.rs`:

Find:
```rust
    /// Requested buffer size in frames (32–2048); `None` keeps the device's.
    pub buffer_frames: Option<u32>,
}
```
Replace:
```rust
    /// Requested buffer size in frames (32–2048); `None` keeps the device's.
    pub buffer_frames: Option<u32>,
    /// Requested nominal sample rate (the project rate, 5a); `None` keeps
    /// the device's. `StreamInfo::sample_rate` says what it runs at.
    pub sample_rate: Option<f64>,
}
```
`crates/vizdaw-play/src/live.rs`'s literal gets `sample_rate: None,`.

In `crates/platform-macos/src/hal.rs`, in `open`, before the nominal rate is read:

Find:
```rust
        let sample_rate: f64 = get(
            device,
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
        )?;
        let buffer_frames: u32 = get(
```
Replace:
```rust
        if let Some(want) = cfg.sample_rate {
            let now: f64 = get(
                device,
                kAudioDevicePropertyNominalSampleRate,
                kAudioObjectPropertyScopeGlobal,
            )?;
            if (now - want).abs() > 0.5 {
                // The device may refuse or take a moment: ask, then wait up
                // to 1 s for it to report the new rate (5a Ruling 20).
                let _ = set(
                    device,
                    kAudioDevicePropertyNominalSampleRate,
                    kAudioObjectPropertyScopeGlobal,
                    want,
                );
                let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
                while std::time::Instant::now() < until {
                    let r: f64 = get(
                        device,
                        kAudioDevicePropertyNominalSampleRate,
                        kAudioObjectPropertyScopeGlobal,
                    )?;
                    if (r - want).abs() <= 0.5 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
        let sample_rate: f64 = get(
            device,
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
        )?;
        let buffer_frames: u32 = get(
```

In `crates/core-ffi/src/host.rs`, the offline rate follows the project unless the hook overrides it:

Find:
```rust
/// The offline rate in use (f64 bits): `OFFLINE_RATE` unless a test set
/// another (`set_offline_rate`).
static OFFLINE_RATE_BITS: AtomicU64 = AtomicU64::new(0x40E7_7000_0000_0000);

fn offline_rate() -> f64 {
    f64::from_bits(OFFLINE_RATE_BITS.load(Ordering::Relaxed))
}

/// Makes offline hosts created from now on run at `rate` (test hook: a
/// device whose rate changed).
#[cfg(feature = "test-hooks")]
pub fn set_offline_rate(rate: f64) {
    OFFLINE_RATE_BITS.store(rate.to_bits(), Ordering::Relaxed);
}
```
Replace:
```rust
/// A rate offline hosts run at instead of the project's (f64 bits; 0:
/// follow the project, 5a Ruling 20).
static OFFLINE_RATE_BITS: AtomicU64 = AtomicU64::new(0);

/// The offline device's rate for a project at `project` Hz.
fn offline_rate(project: f64) -> f64 {
    let r = f64::from_bits(OFFLINE_RATE_BITS.load(Ordering::Relaxed));
    if r > 0.0 { r } else { project }
}

/// Makes offline hosts created from now on run at `rate` whatever the
/// project's rate (test hook: a device that refused it); 0 follows the
/// project again.
#[cfg(feature = "test-hooks")]
pub fn set_offline_rate(rate: f64) {
    OFFLINE_RATE_BITS.store(rate.to_bits(), Ordering::Relaxed);
}
```
`OFFLINE_RATE` stays as the default project rate's documentation constant. If nothing uses it, remove it.

`OfflineDriver::start(renderer)` takes the rate: `start(renderer, rate)`, with `let rate = offline_rate();` removed inside it.

`Host` gains `project_rate: f64` and `rate_mismatch: bool` (set in `Host::new`), and `create` takes the project rate:

Find:
```rust
    fn create() -> Result<Host, VizError> {
        HOSTS_CREATED.fetch_add(1, Ordering::Relaxed);
        if offline_mode() {
            let sinks = OfflineSinks::new(1 << 16);
            let log = sinks.log();
            let rate = offline_rate();
            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
            let output = Output::Offline(OfflineDriver::start(renderer)?);
            return Ok(Host::new(engine, output, OFFLINE_DEVICE, rate, Some(log)));
        }
        Host::create_live()
    }
```
Replace:
```rust
    /// A host for a project at `project` Hz: the device is asked for that
    /// rate; when it runs at another, regions are muted (Ruling 20).
    fn create(project: f64) -> Result<Host, VizError> {
        HOSTS_CREATED.fetch_add(1, Ordering::Relaxed);
        let mut h = if offline_mode() {
            let sinks = OfflineSinks::new(1 << 16);
            let log = sinks.log();
            let rate = offline_rate(project);
            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
            let output = Output::Offline(OfflineDriver::start(renderer, rate)?);
            Host::new(engine, output, OFFLINE_DEVICE, rate, Some(log))
        } else {
            Host::create_live(project)?
        };
        h.project_rate = project;
        h.rate_mismatch = (h.sample_rate - project).abs() > 0.5;
        h.engine.set_regions_muted(h.rate_mismatch);
        Ok(h)
    }
```

`create_live` takes `project: f64` and opens with `backend.open(&AudioConfig { sample_rate: Some(project), ..AudioConfig::default() })?`. The non-macOS stub takes and ignores it.

Callers of `Host::create`:
- In `play`: `*slot = Some(Host::create(project)?)`, with `let project = f64::from(doc.settings().format.sample_rate);` computed at the top of `play`.
- In `rebuild`: `Host::create(f64::from(doc.settings().format.sample_rate))?`.

In `play`'s loop, drop a host made for another project rate, as it already drops one made for another session's plugins:

Find:
```rust
        // Another session's plugins played here: a fresh engine, so none
        // of its instances is taken for this document's (same-numbered)
        // slots.
        if slot
            .as_ref()
            .is_some_and(|h| h.factory_owner != owner && h.plugins_in_engine)
        {
```
Replace:
```rust
        // Another session's plugins played here, or the host was opened for
        // another project rate: a fresh engine (for plugins: so none of its
        // instances is taken for this document's same-numbered slots; for
        // rates: so the device is asked for this project's, Ruling 20).
        if slot.as_ref().is_some_and(|h| {
            (h.factory_owner != owner && h.plugins_in_engine) || (h.project_rate - project).abs() > 0.5
        }) {
```

`HostState` gains `rate_mismatch: bool`, filled from `h.rate_mismatch` in `state`. Add a small accessor for the session's project info:

```rust
/// `(rate_mismatch, device rate)` if `owner` holds the engine.
pub fn device(owner: u64) -> Option<(bool, f64)> {
    lock(&HOST)
        .as_ref()
        .filter(|h| h.owner == owner)
        .map(|h| (h.rate_mismatch, h.sample_rate))
}
```

`Session::set_project_format`'s apply step drops the host when this session holds it (`host::release(self.id)` then `host::reset()` when `host::factory_owner() == Some(self.id)` or the host's owner is this session). The next Play then opens at the new rate. `play`'s rate check above does the same lazily, so the explicit drop is only an early release.

In `crates/core-ffi/src/session.rs`, add:

```rust
#[uniffi::export]
impl Session {
    /// An untitled, empty project in `format` (the New Project sheet;
    /// spec §4.1).
    #[uniffi::constructor]
    pub fn new_project(format: crate::format::ProjectFormatRow) -> Result<Arc<Session>, VizError> {
        ffi_guard(|| {
            let f = core_model::format::AudioFormat::from(format);
            let mut doc = Document::new();
            if f != doc.settings().format {
                doc = core_model::command::apply(
                    &doc,
                    &Command::Settings(core_model::command::SettingsCmd::SetFormat {
                        format: f,
                        swaps: Vec::new(),
                    }),
                )?
                .doc;
            }
            Session::with(doc, None, false)
        })
    }
}
```
(Put the constructor in the existing `#[uniffi::export] impl Session` block with the other constructors.)

```rust
    /// The project's format, its mismatched media and the device's rate.
    pub fn project_info(&self) -> Result<crate::format::ProjectInfoRow, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let mismatched = inner
                .offline
                .values()
                .filter(|r| matches!(r, crate::media::OfflineReasonRow::Format | crate::media::OfflineReasonRow::Unreadable))
                .count();
            let (rate_mismatch, device_rate) = host::device(self.id).unwrap_or((false, 0.0));
            Ok(crate::format::ProjectInfoRow {
                format: inner.doc().settings().format.into(),
                mismatched: u32::try_from(mismatched).unwrap_or(u32::MAX),
                rate_mismatch,
                device_rate,
            })
        })
    }

    /// Starts changing the project format: every media file is converted
    /// into the new format (a job; spec §4.1, §5.1). Refused at once for
    /// the same format or an invalid rate; the job fails, changing
    /// nothing, when a file is missing, of another length or cannot be
    /// converted.
    pub fn set_project_format(&self, format: crate::format::ProjectFormatRow) -> Result<u64, VizError> {
        ffi_guard(|| {
            let f = core_model::format::AudioFormat::from(format);
            let mut inner = lock(&self.inner);
            if !f.is_valid() {
                return Err(VizError::Model { message: format!("{} Hz is not a project rate", f.sample_rate) });
            }
            if f == inner.doc().settings().format {
                return Err(VizError::Model { message: "the project is already in that format".into() });
            }
            inner.refresh_media(self.id, false);
            let blocked: Vec<String> = inner
                .doc()
                .media()
                .values()
                .filter(|m| matches!(inner.offline.get(&m.id), Some(crate::media::OfflineReasonRow::Missing | crate::media::OfflineReasonRow::Length)))
                .map(|m| m.name.clone())
                .collect();
            let items = convert_list(&inner, self.id, |_| true);
            let root = inner.root(self.id);
            inner.jobs.spawn(crate::jobs::JobKindRow::Format, move |progress, cancel| {
                if !blocked.is_empty() {
                    return crate::jobs::Work::Convert {
                        kind: crate::jobs::JobKindRow::Format,
                        format: Some(f),
                        files: vec![(core_model::ids::MediaId(u64::MAX), Err(format!(
                            "missing or changed media must be relinked first: {}",
                            blocked.join(", ")
                        )))],
                    };
                }
                convert_files(&root, items, f, progress, cancel, crate::jobs::JobKindRow::Format)
            })
        })
    }

    /// Starts converting the media that is not in the project format
    /// (another rate or depth, or not a VizDAW CAF) into it (the banner's
    /// "Convert"; a job, one undo step "Convert Media").
    pub fn convert_media(&self) -> Result<u64, VizError> {
        ffi_guard(|| {
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let offline = inner.offline.clone();
            let items = convert_list(&inner, self.id, |m| {
                matches!(offline.get(&m), Some(crate::media::OfflineReasonRow::Format | crate::media::OfflineReasonRow::Unreadable))
            });
            let f = inner.doc().settings().format;
            let root = inner.root(self.id);
            inner.jobs.spawn(crate::jobs::JobKindRow::Convert, move |progress, cancel| {
                convert_files(&root, items, f, progress, cancel, crate::jobs::JobKindRow::Convert)
            })
        })
    }
```

Add these helpers as free functions in `session.rs`:

```rust
/// `(media, absolute file, stem)` of the items `pick` chooses.
fn convert_list(
    inner: &Inner,
    id: u64,
    pick: impl Fn(core_model::ids::MediaId) -> bool,
) -> Vec<(core_model::ids::MediaId, std::path::PathBuf, String)> {
    let root = inner.root(id);
    inner
        .doc()
        .media()
        .values()
        .filter(|m| pick(m.id))
        .map(|m| {
            let (_, stem) = core_model::document::MediaItem::file_parts(&m.path);
            (m.id, root.join(&m.path), stem)
        })
        .collect()
}

/// The conversion job's body: each file through the platform importer
/// into the project's `Media/` in `format`.
fn convert_files(
    root: &Path,
    items: Vec<(core_model::ids::MediaId, std::path::PathBuf, String)>,
    format: core_model::format::AudioFormat,
    progress: &std::sync::atomic::AtomicU32,
    cancel: &std::sync::atomic::AtomicBool,
    kind: crate::jobs::JobKindRow,
) -> crate::jobs::Work {
    let importer = crate::media::importer();
    let n = items.len().max(1) as f32;
    let mut files = Vec::new();
    for (k, (media, path, stem)) in items.into_iter().enumerate() {
        let r = core_media::import::import_as(
            importer.as_ref(),
            &path,
            root,
            format,
            &stem,
            &mut |f| progress.store(((k as f32 + f) / n).to_bits(), std::sync::atomic::Ordering::Relaxed),
            cancel,
        );
        match r {
            Err(core_media::MediaError::Cancelled) => {
                discard(root, crate::jobs::Work::Convert { kind, format: None, files });
                return crate::jobs::Work::Cancelled;
            }
            Ok(mut v) if v.len() == 1 => files.push((media, Ok(v.remove(0)))),
            Ok(v) => {
                discard(root, crate::jobs::Work::Import { files: vec![(stem.clone(), Ok(v))], target: None });
                files.push((media, Err(format!("{stem}: has more than two channels"))));
            }
            Err(e) => files.push((media, Err(format!("{stem}: {e}")))),
        }
    }
    crate::jobs::Work::Convert {
        kind,
        format: (kind == crate::jobs::JobKindRow::Format).then_some(format),
        files,
    }
}

/// Applies a finished conversion. A format change applies only when every
/// item converted (else nothing, the converted files are removed, and the
/// failures are the message); a media conversion applies what converted
/// and lists the rest as problems.
fn apply_convert(
    inner: &mut Inner,
    root: &Path,
    kind: crate::jobs::JobKindRow,
    format: Option<core_model::format::AudioFormat>,
    files: Vec<(core_model::ids::MediaId, Result<core_media::import::Imported, String>)>,
) -> Result<(Vec<u64>, Vec<String>), (VizError, Vec<String>)> {
    use core_model::command::{MediaCmd, MediaSwap, SettingsCmd};
    let mut swaps = Vec::new();
    let mut problems = Vec::new();
    for (m, r) in &files {
        match r {
            Ok(i) => swaps.push(MediaSwap {
                media: *m,
                path: i.path.clone(),
                length_frames: i.length_frames,
                content_hash: Some(i.content_hash.clone()),
            }),
            Err(e) => problems.push(e.clone()),
        }
    }
    let remove = |files: &[(core_model::ids::MediaId, Result<core_media::import::Imported, String>)]| {
        for (_, r) in files {
            if let Ok(i) = r {
                let _ = std::fs::remove_file(root.join(&i.path));
            }
        }
    };
    let cmd = match format {
        Some(f) => {
            if !problems.is_empty() {
                remove(&files);
                return Err((VizError::Model { message: problems.join("; ") }, problems));
            }
            Command::Settings(SettingsCmd::SetFormat { format: f, swaps })
        }
        None => {
            if swaps.is_empty() {
                return Ok((Vec::new(), problems));
            }
            Command::Media(MediaCmd::Convert { swaps })
        }
    };
    let _ = kind;
    match inner.history.execute(&cmd, &ViewFilter::default()) {
        Ok(_) => Ok((files.iter().filter(|(_, r)| r.is_ok()).map(|(m, _)| m.0).collect(), problems)),
        Err(e) => {
            remove(&files);
            Err((e.into(), problems))
        }
    }
}
```

In `apply_jobs`, replace the `Work::Convert` placeholder arm from Task 10:
```rust
                crate::jobs::Work::Convert { kind: k, format, files } => {
                    let was_format = format.is_some();
                    match apply_convert(inner, &root, k, format, files) {
                        Ok((created, problems)) => {
                            changed |= !created.is_empty() || was_format;
                            crate::jobs::row(id, kind, crate::jobs::JobStateRow::Done, None, problems, created)
                        }
                        Err((e, problems)) => crate::jobs::row(id, kind, crate::jobs::JobStateRow::Failed, Some(e.to_string()), problems, vec![]),
                    }
                }
```
After a format change applies, release the engine host if this session holds it (`host::release(self.id)` is not enough: the device must reopen at the new rate). Use a new `host::reset_if_owner(self.id)` that drops the host when `owner == id`.

In `FakeSession.swift` add:
```swift
    var info = ProjectInfoRow(
        format: ProjectFormatRow(sampleRate: 48_000, depth: .int24), mismatched: 0, rateMismatch: false,
        deviceRate: 0)
    var formatChanges: [ProjectFormatRow] = []
    var conversions = 0
    func projectInfo() throws -> ProjectInfoRow { info }
    func setProjectFormat(format: ProjectFormatRow) throws -> UInt64 {
        formatChanges.append(format)
        info.format = format
        let id = UInt64(1000 + formatChanges.count)
        jobRows[id] = JobRow(id: id, kind: .format, progress: 1, state: .done, message: nil, problems: [], created: [])
        return id
    }
    func convertMedia() throws -> UInt64 {
        conversions += 1
        info.mismatched = 0
        let id = UInt64(2000 + conversions)
        jobRows[id] = JobRow(id: id, kind: .convert, progress: 1, state: .done, message: nil, problems: [], created: [])
        return id
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-ffi --test format --test device_rate --test plugins 2>&1 | tail -8`
Expected: ok. `format` runs 5 tests on macOS (3 elsewhere), `device_rate` 1, and the existing `plugins` tests still pass with the hook's new meaning: they set 48 kHz, or 44.1 kHz on purpose.

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c` and `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3`
Expected: all green.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-ffi crates/core-engine/src/backend.rs crates/platform-macos/src/hal.rs crates/vizdaw-play/src/live.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "ffi: project format, format changes, media conversion, device rate

New projects take a format; changing it converts every media file into
the new format and rescales regions as one undo step; old or mismatched
media converts on request. The engine host asks the device for the
project rate and mutes regions when it refuses; the offline host follows
the project rate.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 12: Control channel — `file.new` formats, Project Settings, `media.import`, `media.convert`, `state.get media`/`project`

**Files:**
- Modify:
  - Rust: `crates/core-control/src/{registry.rs,params.rs,request.rs}`, `crates/core-ffi/src/control.rs`;
  - Swift: `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/DocumentStore.swift`, `app/Sources/VizDAW/ControlChannel.swift`, `app/Sources/VizDAW/VizDAWApp.swift` (the `makeProject` wiring).
- Test: `crates/core-control/tests/commands.rs`, `crates/core-ffi/tests/control.rs` (or the file that tests `result_json`; `grep -ln result_json crates/core-ffi/tests`), `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`

**Interfaces:**
- Consumes: Task 10's `Session::{media, import_audio, job}` and Task 11's `Session::{new_project, project_info, set_project_format, convert_media}`.
- Produces:
  - `ParamKind::{Int { min: i64, max: i64 }, File}`, `ArgValue::Int(i64)`, `Params::opt_file`;
  - `StateTarget::{Media, Project}`;
  - registry commands `file.project_settings`, `media.import`, `media.convert` (111 commands), with `file.new` gaining `rate`/`depth`;
  - FFI `ControlValue::Int { value: i64 }`, `StateTargetRow::{Media, Project}`, `ControlResult::{Media { doc, items: Vec<MediaRow> }, Project { doc, info: ProjectInfoRow }}`;
  - Swift `ControlOutcome.job(command: String, doc: UUID, job: UInt64)`, `[ControlArg].int(_:)`, `DocumentStore.{makeProject, newProject(_:), defaultFormat}`, `ControlFormats.{format(args:default:), parse(rate:depth:)}`.

Rulings 15–18 and spec §12.1.
- **`file.new`.** With `rate` (`"44100"`…`"192000"`) and/or `depth` (`"16"`, `"24"`, `"32f"`) it makes a project in that format. What it leaves out comes from the "Default project format" preference.
- **`file.project_settings`.** Changes the format with `rate`/`depth` and waits for the conversion. Without them it is a dialog the channel refuses.
- **`media.import`.** `path` (any absolute file, required, else the Open dialog), `seq`, `lane` and `tick` import and wait for the job.
- **`media.convert`.** Converts and waits.
- **`state.get`.** `media` lists the pool; `project` gives the format, the mismatched count and the device-rate mismatch.

- [ ] **Step 1: Write the failing tests**

Append to `crates/core-control/tests/commands.rs`:

```rust
#[test]
fn file_new_takes_a_format() {
    let Command::CommandRun { args, .. } = ok(
        "command.run",
        json!({ "id": "file.new", "params": { "rate": "44100", "depth": "16" } }),
    ) else {
        panic!()
    };
    assert_eq!(
        args,
        vec![
            ("rate".to_string(), ArgValue::Text("44100".into())),
            ("depth".to_string(), ArgValue::Text("16".into()))
        ]
    );
    let (code, msg) = err(
        "command.run",
        json!({ "id": "file.new", "params": { "rate": "22050" } }),
    );
    assert_eq!(code, ErrorCode::InvalidParams);
    assert!(msg.contains("rate"), "{msg}");
}

#[test]
fn media_import_takes_any_absolute_file_and_a_place() {
    let Command::CommandRun { args, .. } = ok(
        "command.run",
        json!({ "id": "media.import",
                "params": { "path": "/tmp/a b.wav", "seq": 1, "lane": 2, "tick": 3840 } }),
    ) else {
        panic!()
    };
    assert_eq!(
        args,
        vec![
            ("path".to_string(), ArgValue::Text("/tmp/a b.wav".into())),
            ("seq".to_string(), ArgValue::Int(1)),
            ("lane".to_string(), ArgValue::Int(2)),
            ("tick".to_string(), ArgValue::Int(3840)),
        ]
    );
    // The dialog is answered only by `path`.
    let (code, _) = err("command.run", json!({ "id": "media.import" }));
    assert_eq!(code, ErrorCode::DialogRequired);
    let (code, _) = err(
        "command.run",
        json!({ "id": "media.import", "params": { "path": "relative.wav" } }),
    );
    assert_eq!(code, ErrorCode::InvalidParams);
    let (code, _) = err(
        "command.run",
        json!({ "id": "media.import", "params": { "path": "/a.wav", "seq": -1 } }),
    );
    assert_eq!(code, ErrorCode::InvalidParams);
}

#[test]
fn project_settings_is_a_dialog_without_a_format() {
    let (code, _) = err("command.run", json!({ "id": "file.project_settings" }));
    assert_eq!(code, ErrorCode::DialogRequired);
    assert!(matches!(
        ok("command.run", json!({ "id": "file.project_settings", "params": { "depth": "32f" } })),
        Command::CommandRun { .. }
    ));
    assert!(matches!(
        ok("command.run", json!({ "id": "media.convert" })),
        Command::CommandRun { .. }
    ));
}

#[test]
fn state_get_media_and_project() {
    assert!(matches!(
        ok("state.get", json!({ "target": "media" })),
        Command::StateGet { target: StateTarget::Media, .. }
    ));
    assert!(matches!(
        ok("state.get", json!({ "target": "project" })),
        Command::StateGet { target: StateTarget::Project, .. }
    ));
}
```

In the same file:

Find:
```rust
        COMMANDS.len(),
        108,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry"
```
Replace:
```rust
        COMMANDS.len(),
        111,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media"
```

Check `ErrorCode::DialogRequired` and `ErrorCode::InvalidParams` against `crates/core-control/src/error.rs` and use the real names.

Add Swift tests to `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`, following that file's existing helpers for a store, a fake host and a document (reuse them; the names below stand for them):

```swift
    @Test func fileNewTakesRateAndDepth() throws {
        let (store, host) = makeStoreAndHost()
        var made: ProjectFormatRow?
        store.makeProject = { f in made = f; return FakeSession([]) }
        _ = ControlCommands.run(
            "file.new", args: [ControlArg(name: "rate", value: .text(value: "96000"))], doc: nil,
            store: store, host: host)
        #expect(made == ProjectFormatRow(sampleRate: 96_000, depth: store.defaultFormat.depth))
    }

    @Test func formatsParseTheChannelsSpelling() {
        #expect(ControlFormats.parse(rate: "44100", depth: "16") == ProjectFormatRow(sampleRate: 44_100, depth: .int16))
        #expect(ControlFormats.parse(rate: nil, depth: "32f")?.depth == .float32)
        #expect(ControlFormats.parse(rate: "nope", depth: nil) == nil)
    }

    @Test func mediaImportStartsAJobAndAnswersLater() throws {
        let (store, host, id, model, fake) = makeDocument()
        let out = ControlCommands.run(
            "media.import",
            args: [
                ControlArg(name: "path", value: .text(value: "/tmp/x.wav")),
                ControlArg(name: "seq", value: .int(value: 4)),
                ControlArg(name: "tick", value: .int(value: 960)),
            ],
            doc: (id, model), store: store, host: host)
        guard case .job(let command, let doc, let job) = out else { Issue.record("\(out)"); return }
        #expect(command == "media.import" && doc == id && job == 1)
        #expect(fake.imports.first?.0 == ["/tmp/x.wav"])
        #expect(fake.imports.first?.1 == ImportTargetRow(seq: 4, lane: nil, tick: 960))
    }

    @Test func projectSettingsChangesTheFormatAsAJob() throws {
        let (store, host, id, model, fake) = makeDocument()
        let out = ControlCommands.run(
            "file.project_settings", args: [ControlArg(name: "depth", value: .text(value: "16"))],
            doc: (id, model), store: store, host: host)
        guard case .job = out else { Issue.record("\(out)"); return }
        #expect(fake.formatChanges == [ProjectFormatRow(sampleRate: 48_000, depth: .int16)])
    }

    @Test func stateGetMediaAndProject() throws {
        let (_, _, id, model, fake) = makeDocument()
        fake.mediaRows = [
            MediaRow(
                id: 3, name: "kick", path: "Media/kick.caf", channels: 2, frames: 10, sampleRate: 48_000,
                origin: .imported, originalName: "kick.wav", offline: .missing, uses: 1)
        ]
        guard case .media(_, let items) = ControlCommands.state(.media, id, model) else { Issue.record("media"); return }
        #expect(items.map(\.id) == [3])
        guard case .project(_, let info) = ControlCommands.state(.project, id, model) else { Issue.record("project"); return }
        #expect(info.format.sampleRate == 48_000)
    }
```

`makeStoreAndHost()` and `makeDocument()` stand for this file's existing fixture helpers. Use the real ones, or add these two as thin wrappers over them. `makeDocument()` returns the store, the host, the document's ID, its `DocumentModel` and its `FakeSession`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-control --test commands 2>&1 | tail -5`
Expected: compile errors (`ArgValue::Int`, `StateTarget::Media`).

- [ ] **Step 3: Implement**

In `crates/core-control/src/params.rs`, after `opt_path`:

```rust
    /// An optional absolute path with any extension (a file to import).
    pub fn opt_file(&self, name: &str) -> Result<Option<PathBuf>, ControlError> {
        let Some(s) = self.opt_str(name, MAX_PATH)? else {
            return Ok(None);
        };
        let p = PathBuf::from(&s);
        if s.len() > MAX_PATH || s.contains('\0') || !p.is_absolute() {
            return Err(self.error(format!("`{name}` must be an absolute path")));
        }
        Ok(Some(p))
    }
```

In `crates/core-control/src/registry.rs`:

Find:
```rust
    /// One of these strings.
    Choice(&'static [&'static str]),
}
```
Replace:
```rust
    /// One of these strings.
    Choice(&'static [&'static str]),
    /// An integer in `min..=max`.
    Int {
        /// Smallest.
        min: i64,
        /// Largest.
        max: i64,
    },
    /// An absolute path with any extension.
    File,
}
```

Find:
```rust
    /// A flag.
    Flag(bool),
}
```
(in `ArgValue`) Replace:
```rust
    /// A flag.
    Flag(bool),
    /// An integer.
    Int(i64),
}
```

In `validate_args`:

Find:
```rust
            ParamKind::Flag => p.opt_bool(s.name)?.map(ArgValue::Flag),
```
Replace:
```rust
            ParamKind::Flag => p.opt_bool(s.name)?.map(ArgValue::Flag),
            ParamKind::Int { min, max } => p.opt_int(s.name, min, max)?.map(ArgValue::Int),
            ParamKind::File => p
                .opt_file(s.name)?
                .map(|path| ArgValue::Text(path.to_string_lossy().into_owned())),
```

In `kind_json`:

Find:
```rust
        ParamKind::Choice(values) => json!({"type": "choice", "values": values}),
    }
```
Replace:
```rust
        ParamKind::Choice(values) => json!({"type": "choice", "values": values}),
        ParamKind::Int { min, max } => json!({"type": "int", "min": min, "max": max}),
        ParamKind::File => json!({"type": "path"}),
    }
```

Add the parameter tables (next to `SAVE_PATH`):

```rust
/// The project rates and depths as the channel spells them (5a Ruling 16).
const RATES: &[&str] = &["44100", "48000", "88200", "96000", "176400", "192000"];
const DEPTHS: &[&str] = &["16", "24", "32f"];

const FORMAT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "rate",
        kind: ParamKind::Choice(RATES),
        required: false,
        help: "Sample rate in Hz.",
    },
    ParamSpec {
        name: "depth",
        kind: ParamKind::Choice(DEPTHS),
        required: false,
        help: "Bit depth: 16, 24 or 32f (float).",
    },
];

const IMPORT_PARAMS: &[ParamSpec] = &[
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The audio file to import (answers the Open dialog).",
    },
    ParamSpec {
        name: "seq",
        kind: ParamKind::Int { min: 0, max: i64::MAX / 2 },
        required: false,
        help: "Sequence to place a region in (without it, the file only enters the media pool).",
    },
    ParamSpec {
        name: "lane",
        kind: ParamKind::Int { min: 0, max: i64::MAX / 2 },
        required: false,
        help: "Lane for a mono or stereo file; without it, new lanes.",
    },
    ParamSpec {
        name: "tick",
        kind: ParamKind::Int { min: -(1 << 40), max: 1 << 40 },
        required: false,
        help: "Where the region starts (negative imports at 0).",
    },
];
```

Change `file.new` and add the three commands:

Find:
```rust
    cmd("file.new", "File", "New", Some("⌘N"), false),
```
Replace:
```rust
    CommandSpec {
        params: FORMAT_PARAMS,
        ..cmd("file.new", "File", "New", Some("⌘N"), false)
    },
```

Find:
```rust
    cmd("file.close", "File", "Close", Some("⌘W"), true),
```
Replace:
```rust
    CommandSpec {
        params: IMPORT_PARAMS,
        dialogs: &[DialogSpec {
            name: "Open",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd("media.import", "File", "Import Audio…", Some("⇧⌘I"), true)
    },
    cmd("file.close", "File", "Close", Some("⌘W"), true),
```
If `⇧⌘I` is already a default key of another command, the bindings test (no two commands on one key) fails. Then use `None` as the shortcut.

After the `file.save_as` entry add:
```rust
    CommandSpec {
        params: FORMAT_PARAMS,
        dialogs: &[DialogSpec {
            name: "Project Settings",
            when: "always",
            always: true,
            answered_by: &["rate", "depth"],
        }],
        ..cmd("file.project_settings", "File", "Project Settings…", None, true)
    },
    CommandSpec {
        menu_item: false,
        ..cmd("media.convert", "File", "Convert Media to Project Format", None, true)
    },
```

In `crates/core-control/src/request.rs`:

Find:
```rust
    /// Every VizDAW window, plus app-level alerts.
    Windows,
}
```
Replace:
```rust
    /// Every VizDAW window, plus app-level alerts.
    Windows,
    /// Its media pool (5a).
    Media,
    /// Its project format and device rate (5a).
    Project,
}
```

Find:
```rust
                "windows" => StateTarget::Windows,
                _ => {
                    return Err(p.error(
                        "`target` must be transport, document, editor, selection or windows",
                    ));
                }
```
Replace:
```rust
                "windows" => StateTarget::Windows,
                "media" => StateTarget::Media,
                "project" => StateTarget::Project,
                _ => {
                    return Err(p.error(
                        "`target` must be transport, document, editor, selection, windows, media or project",
                    ));
                }
```

In `crates/core-ffi/src/control.rs`:
- `ControlValue` gains `Int { value: i64 }`, and the mapping `ArgValue::Int(value) => ControlValue::Int { value }`.
- `StateTargetRow` gains `Media` and `Project`, and the mapping gains `StateTarget::Media => StateTargetRow::Media, StateTarget::Project => StateTargetRow::Project`.
- `ControlResult` gains:

```rust
    /// `state.get media`.
    Media {
        /// Document ID.
        doc: String,
        /// The pool.
        items: Vec<crate::media::MediaRow>,
    },
    /// `state.get project`.
    Project {
        /// Document ID.
        doc: String,
        /// Format, mismatched media, device rate.
        info: crate::format::ProjectInfoRow,
    },
```
and `result_json` gains:

```rust
        ControlResult::Media { doc, items } => json!({
            "doc": doc,
            "items": items.iter().map(|m| json!({
                "id": m.id, "name": m.name, "path": m.path, "channels": m.channels,
                "frames": m.frames, "rate": m.sample_rate,
                "origin": match m.origin {
                    crate::media::MediaOriginRow::Imported => "imported",
                    crate::media::MediaOriginRow::Recorded => "recorded",
                    crate::media::MediaOriginRow::Bounced => "bounced",
                    crate::media::MediaOriginRow::ExternalEdit => "external_edit",
                },
                "original_name": m.original_name,
                "offline": m.offline.is_some(),
                "offline_reason": m.offline.map(|r| match r {
                    crate::media::OfflineReasonRow::Missing => "missing",
                    crate::media::OfflineReasonRow::Unreadable => "unreadable",
                    crate::media::OfflineReasonRow::Format => "format",
                    crate::media::OfflineReasonRow::Length => "length",
                }),
                "uses": m.uses,
            })).collect::<Vec<_>>(),
        }),
        ControlResult::Project { doc, info } => json!({
            "doc": doc,
            "rate": info.format.sample_rate,
            "depth": match info.format.depth {
                crate::format::BitDepthRow::Int16 => "16",
                crate::format::BitDepthRow::Int24 => "24",
                crate::format::BitDepthRow::Float32 => "32f",
            },
            "label": crate::format::format_label(info.format),
            "mismatched": info.mismatched,
            "rate_mismatch": info.rate_mismatch,
            "device_rate": info.device_rate,
        }),
```
Also add `"rate_mismatch": t.rate_mismatch` to `transport_json`.

Add a JSON test where `result_json` is tested (`grep -ln "result_json" crates/core-ffi/tests`). If none exists, add one to `crates/core-ffi/tests/control.rs`:

```rust
#[test]
fn media_and_project_results_as_json() {
    let v = vizdaw_ffi::control::result_json(&vizdaw_ffi::ControlResult::Media {
        doc: "D".into(),
        items: vec![vizdaw_ffi::MediaRow {
            id: 2, name: "kick".into(), path: "Media/kick.caf".into(), channels: 2, frames: 10,
            sample_rate: 48_000, origin: vizdaw_ffi::MediaOriginRow::Imported,
            original_name: Some("kick.wav".into()), offline: Some(vizdaw_ffi::OfflineReasonRow::Format),
            uses: 3,
        }],
    });
    assert_eq!(v["items"][0]["offline_reason"], "format");
    assert_eq!(v["items"][0]["offline"], true);
    let p = vizdaw_ffi::control::result_json(&vizdaw_ffi::ControlResult::Project {
        doc: "D".into(),
        info: vizdaw_ffi::ProjectInfoRow {
            format: vizdaw_ffi::ProjectFormatRow { sample_rate: 44_100, depth: vizdaw_ffi::BitDepthRow::Float32 },
            mismatched: 1, rate_mismatch: true, device_rate: 48_000.0,
        },
    });
    assert_eq!((p["rate"].clone(), p["depth"].clone(), p["label"].clone()), (serde_json::json!(44_100), serde_json::json!("32f"), serde_json::json!("44.1 kHz, 32-bit float")));
}
```

Swift, in `app/Sources/VizDAWAppModel/ControlCommands.swift`:

```swift
/// The project formats as the channel spells them (5a Ruling 16).
public enum ControlFormats {
    /// `rate`/`depth` strings to a format; nil when either is not one.
    public static func parse(rate: String?, depth: String?, base: ProjectFormatRow = .init(sampleRate: 48_000, depth: .int24)) -> ProjectFormatRow? {
        var f = base
        if let r = rate {
            guard let n = UInt32(r), projectRates().contains(n) else { return nil }
            f.sampleRate = n
        }
        if let d = depth {
            switch d {
            case "16": f.depth = .int16
            case "24": f.depth = .int24
            case "32f": f.depth = .float32
            default: return nil
            }
        }
        return f
    }

    /// The format a call's arguments ask for, `base` filling what they
    /// leave out.
    public static func format(_ args: [ControlArg], base: ProjectFormatRow) -> ProjectFormatRow? {
        parse(rate: args.text("rate"), depth: args.text("depth"), base: base)
    }
}
```
`formatsParseTheChannelsSpelling` calls `parse(rate:depth:)` with the default base. With `rate: nil, depth: "32f"` the result is `48000/32f`. Keep that two-argument call working (the default parameter does).

Add to the `[ControlArg]` extension:
```swift
    /// The integer argument `name`.
    public func int(_ name: String) -> Int64? {
        for a in self where a.name == name {
            if case .int(let v) = a.value { return v }
        }
        return nil
    }
```

`ControlOutcome` gains:
```swift
    /// A job started (import, conversion): the app answers when it ends.
    case job(command: String, doc: UUID, job: UInt64)
```

`other` gains `"file.project_settings", "media.import", "media.convert"`. In `run`:

Find:
```swift
        case "file.new": return made(id, store.newEmpty(), store)
```
Replace:
```swift
        case "file.new":
            guard let f = ControlFormats.format(args, base: store.defaultFormat) else {
                return .failed(.of(.invalidParams, "`rate` or `depth` is not a project format"))
            }
            return made(id, store.newProject(f), store)
```
(`.invalidParams` stands for the `ControlErrorCode` case of that name; use the real one.)

After the `case "editors.close_detail":` group, inside the document `switch`, add:
```swift
        case "file.project_settings":
            guard let f = ControlFormats.format(args, base: (try? m.session.projectInfo().format) ?? store.defaultFormat)
            else { return .failed(.of(.invalidParams, "`rate` or `depth` is not a project format")) }
            do {
                return .job(command: id, doc: docID, job: try m.session.setProjectFormat(format: f))
            } catch {
                return .failed(.thrown(error, "Project Settings Failed"))
            }
        case "media.import":
            guard let path = args.text("path") else { return .failed(.dialog("Open", "path")) }
            let target = args.int("seq").map {
                ImportTargetRow(seq: UInt64($0), lane: args.int("lane").map(UInt64.init), tick: args.int("tick") ?? 0)
            }
            do {
                return .job(command: id, doc: docID, job: try m.session.importAudio(paths: [path], target: target))
            } catch {
                return .failed(.thrown(error, "Import Failed"))
            }
        case "media.convert":
            do {
                return .job(command: id, doc: docID, job: try m.session.convertMedia())
            } catch {
                return .failed(.thrown(error, "Convert Failed"))
            }
```

In `ControlCommands.state`:
```swift
        case .media:
            return .media(doc: doc, items: (try? m.session.media()) ?? [])
        case .project:
            guard let info = try? m.session.projectInfo() else { return nil }
            return .project(doc: doc, info: info)
```

In `app/Sources/VizDAWAppModel/DocumentStore.swift`:
```swift
    /// Makes a project in a format (set by `live()`; tests set a fake).
    public var makeProject: ((ProjectFormatRow) throws -> any SessionProtocol)?

    /// The "Default project format" preference (5a Ruling 18), e.g.
    /// "48000/24"; 48 kHz / 24-bit when unset or damaged.
    public var defaultFormat: ProjectFormatRow {
        get {
            let s = defaults.string(forKey: "defaultProjectFormat") ?? ""
            let p = s.split(separator: "/").map(String.init)
            return ControlFormats.parse(rate: p.first, depth: p.count > 1 ? p[1] : nil)
                ?? ProjectFormatRow(sampleRate: 48_000, depth: .int24)
        }
        set {
            let d: String = switch newValue.depth { case .int16: "16"; case .int24: "24"; case .float32: "32f" }
            defaults.set("\(newValue.sampleRate)/\(d)", forKey: "defaultProjectFormat")
        }
    }

    /// A new project in `format` (the New Project sheet, `file.new`).
    public func newProject(_ format: ProjectFormatRow) -> UUID? {
        guard let make = makeProject else { return newEmpty() }
        return add("New Document Failed") { try make(format) }
    }
```
The store has no preferences of its own yet. Add, next to `listPreferences`:
```swift
    /// The app's preferences (in memory in tests; `live()` sets the real
    /// store, which is itself in memory under `VIZDAW_SUPPORT_DIR`).
    public var defaults: UserDefaults = MemoryDefaults()
```
and in `live()` add `s.defaults = MemoryDefaults.appStore()`. `add(_:_:)` takes a `Make` (`() throws -> any SessionProtocol`); the closure `{ try make(format) }` is one.

In `app/Sources/VizDAW/VizDAWApp.swift` (where `makeDemoAudio` is set in `live()`), add `store.makeProject = { try Session.newProject(format: $0) }`.

In `app/Sources/VizDAW/ControlChannel.swift`, `answer` handles the new outcome:

Find:
```swift
        case .opened(let command, let doc): whenShown(server, token, command: command, doc: doc)
        }
```
Replace:
```swift
        case .opened(let command, let doc): whenShown(server, token, command: command, doc: doc)
        case .job(let command, let doc, let job): whenJobEnds(server, token, command: command, doc: doc, job: job)
        }
```
and add:
```swift
    /// Answers once job `job` of `doc` ended (an import or conversion may
    /// take minutes; 10 minutes at most). Polling `job` applies it.
    static func whenJobEnds(_ server: ControlServer, _ token: UInt64, command: String, doc: UUID, job: UInt64) {
        later(token, seconds: 600) { expired in
            guard let m = appStore.model(doc) else {
                _ = try? server.fail(token: token, failure: .of(.noDocument, "the document closed"))
                return true
            }
            guard let j = try? m.session.job(id: job) else { return false }
            switch j.state {
            case .running:
                if expired { _ = try? server.fail(token: token, failure: .of(.timeout, "the job did not finish within 10 minutes")) }
                return expired
            case .done:
                m.refreshAfterJob()
                let window = ControlWindows.window(of: doc).map { Int64($0.windowNumber) }
                let status = j.problems.isEmpty ? nil : j.problems.joined(separator: "; ")
                _ = try? server.reply(
                    token: token,
                    result: .ran(command: command, doc: doc.uuidString, window: window, undoLabel: m.snapshot.undoLabel, status: status))
                return true
            case .failed, .cancelled:
                _ = try? server.fail(token: token, failure: .of(.failed, j.message ?? "the job was cancelled"))
                return true
            }
        }
    }
```
and to `DocumentModel` (in `DocumentModel.swift`):
```swift
    /// After a job applied (an import or conversion): the document, its
    /// lanes and the media rows are new.
    public func refreshAfterJob() {
        if let s = try? session.snapshot() { apply(s) }
        mediaVersion += 1
    }

    /// Bumped when the media pool or its offline state changed (views
    /// showing media refetch).
    public private(set) var mediaVersion = 0
```
`.noDocument`, `.timeout` and `.failed` stand for the `ControlErrorCode` cases with those meanings; use the real ones. Any exhaustive `switch` over `ControlOutcome` in the app or the tests gets a `.job` arm. In tests, `Issue.record` it, unless the test means a job.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-control -p core-ffi 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c`
Expected: all ok.

Run: `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -5`
Expected: ok, including `everyRegisteredMenuCommandHasAHandler` with the three new IDs and the five new tests.

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
git add crates/core-control crates/core-ffi app/Sources app/Tests
git commit -m "control: project formats, Import Audio, Convert Media, media and project state

file.new takes rate and depth; file.project_settings changes the format
and media.import / media.convert import or convert as jobs the channel
answers when they end; state.get media lists the pool with offline
reasons and state.get project the format and device-rate mismatch.
Registry 111 commands.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 13: The app — New Project, Project Settings, Import Audio, banners

**Files:**
- Create: `app/Sources/VizDAWAppModel/ProjectMedia.swift`, `app/Sources/VizDAW/ProjectBanners.swift`, `app/Sources/VizDAW/ProjectPanels.swift`, `app/Tests/VizDAWAppModelTests/ProjectMediaTests.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/DocumentView.swift`, `app/Sources/VizDAW/VizDAWApp.swift` (`applicationDidBecomeActive`), `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Test: `app/Tests/VizDAWAppModelTests/ProjectMediaTests.swift`

**Interfaces:**
- Consumes: Tasks 10–12 (`Session` media, jobs, format; `DocumentStore.{newProject, defaultFormat}`; `DocumentModel.refreshAfterJob`, `mediaVersion`).
- Produces:
  - `ProjectBanner` (`.job(id, kind, progress)`, `.mismatched(count, label)`, `.rateMismatch(device: Double, project: UInt32)`, `.diskTooSlow`, `.problems([String])`);
  - `DocumentModel.{projectInfo, banners, importAudio(paths:), changeFormat(_:), convertMedia(), changeRateToDevice(), cancelJob(_:), pollJobs(), refreshProjectInfo(), recheckMedia(), dismissDiskWarning(), dismissProblems()}`;
  - `DocumentModel.importTarget(seq:selection:files:tick:)` (static, pure).
  - Panels: `FormatPanel.run(title:message:initial:) -> ProjectFormatRow?` (an `NSAlert` with two pop-up buttons, modal) and `Panels.chooseAudio() -> [String]`.

Spec §4.1 (New Project sheet, Project Settings), §5.1 and §5.6 (banner); Rulings 18–21.
- **The menu.** File › New asks for the format (pre-filled with the default, which it then remembers). File › Import Audio… (⇧⌘I) imports into the place of Ruling 19. File › Project Settings… changes the format, saying how many files will be converted.
- **Banners** above the editors show:
  - a running job, with progress and Cancel;
  - media not in the project format, with Convert;
  - a device at another rate, with "Change Project Rate to …";
  - disk underruns, until dismissed or the next Play;
  - a finished job's per-file problems.
- **Rechecks.** When the app becomes active, every document rechecks its media.

The format and import panels are modal AppKit (`NSAlert`, `NSOpenPanel`), so they need no SwiftUI state. Their logic (labels, targets, banners) is in `VizDAWAppModel` and tested there. While the control channel is on, programmatic calls never open them (the menu actions are the only callers).

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/ProjectMediaTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/ProjectMediaTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// Import Audio, the project format and the banners (plan 5a Task 13).
@MainActor
struct ProjectMediaTests {
    func model(_ fake: FakeSession) throws -> DocumentModel {
        try DocumentModel(session: fake)
    }

    func fake() -> FakeSession {
        let f = FakeSession([
            SeqRow(id: 1, name: "Song", kind: .normal, topLevel: true, useCount: 0, lengthTicks: 0, laneCount: 1,
                   keyboardSlot: nil, comment: "")
        ])
        f.laneTable[1] = []
        return f
    }

    @Test func importTargetUsesTheSelectedLaneForOneFile() {
        let sel = [LaneRef(seq: 2, lane: 9), LaneRef(seq: 1, lane: 5)]
        #expect(DocumentModel.importTarget(seq: 1, selection: sel, files: 1, tick: 960)
            == ImportTargetRow(seq: 1, lane: 5, tick: 960))
        #expect(DocumentModel.importTarget(seq: 1, selection: sel, files: 2, tick: 960)
            == ImportTargetRow(seq: 1, lane: nil, tick: 960))
        #expect(DocumentModel.importTarget(seq: 1, selection: [], files: 1, tick: -100)
            == ImportTargetRow(seq: 1, lane: nil, tick: 0))
        #expect(DocumentModel.importTarget(seq: nil, selection: sel, files: 1, tick: 0) == nil)
    }

    @Test func importRunsAsAJobAndRefreshesWhenItEnds() throws {
        let f = fake()
        f.jobStartState = .running
        let m = try model(f)
        m.importAudio(paths: ["/a.wav"])
        #expect(f.imports.count == 1)
        #expect(m.banners.contains { if case .job = $0 { true } else { false } })
        let before = m.mediaVersion
        f.jobRows[1] = JobRow(id: 1, kind: .import, progress: 1, state: .done, message: nil, problems: [], created: [3])
        m.pollJobs()
        #expect(m.mediaVersion == before + 1)
        #expect(!m.banners.contains { if case .job = $0 { true } else { false } })
    }

    @Test func aFailedJobShowsAnAlertAndProblemsShowABanner() throws {
        let f = fake()
        f.jobStartState = .running
        let m = try model(f)
        m.importAudio(paths: ["/a.wav", "/b.wav"])
        f.jobRows[1] = JobRow(id: 1, kind: .import, progress: 1, state: .done, message: nil,
                              problems: ["b.wav: not audio"], created: [7])
        m.pollJobs()
        #expect(m.banners.contains(.problems(["b.wav: not audio"])))
        m.dismissProblems()
        #expect(!m.banners.contains(.problems(["b.wav: not audio"])))
        m.importAudio(paths: ["/c.wav"])
        f.jobRows[2] = JobRow(id: 2, kind: .import, progress: 1, state: .failed, message: "disk full",
                              problems: [], created: [])
        m.pollJobs()
        #expect(m.alert?.message.contains("disk full") == true)
    }

    @Test func mismatchedMediaOffersConvert() throws {
        let f = fake()
        f.info.mismatched = 3
        let m = try model(f)
        m.refreshProjectInfo()
        #expect(m.banners.contains(.mismatched(count: 3, label: "48 kHz, 24-bit")))
        m.convertMedia()
        #expect(f.conversions == 1)
        m.pollJobs()
        #expect(!m.banners.contains { if case .mismatched = $0 { true } else { false } })
    }

    @Test func aDeviceAtAnotherRateOffersToChangeTheProjectRate() throws {
        let f = fake()
        f.info.rateMismatch = true
        f.info.deviceRate = 44_100
        let m = try model(f)
        m.refreshProjectInfo()
        #expect(m.banners.contains(.rateMismatch(device: 44_100, project: 48_000)))
        m.changeRateToDevice()
        #expect(f.formatChanges == [ProjectFormatRow(sampleRate: 44_100, depth: .int24)])
    }

    @Test func diskTooSlowFollowsTheFaults() throws {
        let f = fake()
        let m = try model(f)
        f.state.faults = ["stream-underrun"]
        m.poll()
        #expect(m.banners.contains(.diskTooSlow))
        m.dismissDiskWarning()
        #expect(!m.banners.contains(.diskTooSlow))
        // Cleared at the next Play: a new underrun shows again.
        f.state.faults = []
        m.poll()
        f.state.faults = ["stream-underrun"]
        m.poll()
        #expect(m.banners.contains(.diskTooSlow))
    }

    @Test func cancellingAJobAsksTheSession() throws {
        let f = fake()
        f.jobStartState = .running
        let m = try model(f)
        m.importAudio(paths: ["/a.wav"])
        m.pollJobs()
        #expect(m.banners.contains(.job(id: 1, kind: .import, progress: 0.4)))
        m.cancelJob(1)
        #expect(f.cancelled == [1])
    }

    @Test func theDefaultFormatIsRemembered() {
        let store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { FakeSession([]) }, open: { _ in FakeSession([]) })
        #expect(store.defaultFormat == ProjectFormatRow(sampleRate: 48_000, depth: .int24))
        store.defaultFormat = ProjectFormatRow(sampleRate: 96_000, depth: .float32)
        #expect(store.defaultFormat == ProjectFormatRow(sampleRate: 96_000, depth: .float32))
        store.defaults.set("garbage", forKey: "defaultProjectFormat")
        #expect(store.defaultFormat == ProjectFormatRow(sampleRate: 48_000, depth: .int24))
    }
}
VIZDAW_EOF
```

Match the `SeqRow` initialiser to the generated bindings (`grep -n "public init(id: UInt64, name: String" app/Sources/VizDAWCore/vizdaw_ffi.swift` after `scripts/build-ffi.sh`), or reuse a helper the other tests use to make a sequence row.

In `FakeSession`:
- add `var cancelled: [UInt64] = []` and record into it in `cancelJob(id:)`;
- add `var jobStartState: JobStateRow = .done`;
- make `importAudio` store `JobRow(id: id, kind: .import, progress: jobStartState == .running ? 0.4 : 1, state: jobStartState, message: nil, problems: [], created: [])`. A test that wants to see a job running sets `.running`, then puts the finished row into `jobRows` itself.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `scripts/build-ffi.sh && scripts/swift.sh test 2>&1 | grep -E "error:|ProjectMedia" | head -10`
Expected: compile errors (`importTarget`, `banners`, `ProjectBanner` not found).

- [ ] **Step 3: Implement**

In `app/Sources/VizDAWAppModel/DocumentModel.swift`, add the stored state next to `pluginProblems`:

```swift
    /// The project's format, mismatched media and device rate (5a).
    public internal(set) var projectInfo: ProjectInfoRow?
    /// Running imports and conversions, oldest first.
    public internal(set) var jobs: [JobRow] = []
    /// A finished job's per-file problems, until dismissed.
    public internal(set) var jobProblems: [String] = []
    /// The disk warning was dismissed for this playback.
    @ObservationIgnored var diskWarningDismissed = false
```
In `init`, after `apply(snap)`: `projectInfo = try? session.projectInfo()`.
In `poll()`, after `transport = try session.transport()`:
```swift
            if transport.rateMismatch != (projectInfo?.rateMismatch ?? false) { refreshProjectInfo() }
            if !transport.faults.contains("stream-underrun") { diskWarningDismissed = false }
```
In `idle()`, after `if let s = u.snapshot { apply(s) }`:
```swift
        if u.mediaChanged { mediaVersion += 1; refreshProjectInfo() }
        if !jobs.isEmpty { pollJobs() }
```
In `refreshAfterJob()` (Task 12), add `refreshProjectInfo()`.

Create `app/Sources/VizDAWAppModel/ProjectMedia.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/ProjectMedia.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// A bar above a document's editors (plan 5a Task 13).
public enum ProjectBanner: Equatable, Sendable {
    /// An import or conversion running.
    case job(id: UInt64, kind: JobKindRow, progress: Float)
    /// Media not in the project format: Convert.
    case mismatched(count: UInt32, label: String)
    /// The device would not switch to the project rate: regions are muted.
    case rateMismatch(device: Double, project: UInt32)
    /// Audio was not read from disk in time.
    case diskTooSlow
    /// Files a finished job could not handle.
    case problems([String])
}

extension DocumentModel {
    /// Where Import Audio puts files (Ruling 19): the selected lane of the
    /// shown sequence for one file, else new lanes; the transport's tick
    /// (0 if before it); no target without a sequence.
    public static func importTarget(
        seq: UInt64?, selection: [LaneRef], files: Int, tick: Int64
    ) -> ImportTargetRow? {
        guard let seq else { return nil }
        let lane = files == 1 ? selection.first { $0.seq == seq }?.lane : nil
        return ImportTargetRow(seq: seq, lane: lane, tick: max(0, tick))
    }

    /// File › Import Audio…: imports `paths` as a job.
    public func importAudio(paths: [String]) {
        guard !paths.isEmpty else { return }
        let target = Self.importTarget(
            seq: selectedSeq, selection: editor.selection.lanes, files: paths.count, tick: transport.tick)
        start("Import Failed") { try $0.importAudio(paths: paths, target: target) }
    }

    /// Project Settings: converts every file into `format` (a job).
    public func changeFormat(_ format: ProjectFormatRow) {
        start("Project Settings Failed") { try $0.setProjectFormat(format: format) }
    }

    /// The banner's Convert.
    public func convertMedia() {
        start("Convert Failed") { try $0.convertMedia() }
    }

    /// The device-rate banner's button: the project takes the device's rate.
    public func changeRateToDevice() {
        guard let i = projectInfo, i.rateMismatch, i.deviceRate > 0 else { return }
        changeFormat(ProjectFormatRow(sampleRate: UInt32(i.deviceRate.rounded()), depth: i.format.depth))
    }

    /// Asks job `id` to stop.
    public func cancelJob(_ id: UInt64) {
        try? session.cancelJob(id: id)
    }

    private func start(_ title: String, _ body: (any SessionProtocol) throws -> UInt64) {
        do {
            let id = try body(session)
            jobs.append(JobRow(id: id, kind: .import, progress: 0, state: .running, message: nil, problems: [], created: []))
            pollJobs()
        } catch {
            alert = AlertItem(error, title: title)
        }
    }

    /// Reads every running job (the plugin timer calls this through
    /// `idle`); an ended one is applied by the session and refreshes the
    /// document.
    public func pollJobs() {
        var still: [JobRow] = []
        for j in jobs {
            guard let r = try? session.job(id: j.id) else {
                still.append(j)
                continue
            }
            switch r.state {
            case .running:
                still.append(r)
            case .done:
                if !r.problems.isEmpty { jobProblems = r.problems }
                refreshAfterJob()
            case .failed:
                alert = AlertItem(
                    title: r.kind == .import ? "Import Failed" : "Conversion Failed",
                    message: r.message ?? "The job failed.")
                refreshAfterJob()
            case .cancelled:
                refreshAfterJob()
            }
        }
        jobs = still
    }

    public func refreshProjectInfo() {
        projectInfo = try? session.projectInfo()
    }

    /// The app became active: files may have changed in Finder.
    public func recheckMedia() {
        if (try? session.recheckMedia()) == true {
            mediaVersion += 1
            refreshProjectInfo()
        }
    }

    public func dismissDiskWarning() { diskWarningDismissed = true }

    public func dismissProblems() { jobProblems = [] }

    /// What the bars above the editors show, top first.
    public var banners: [ProjectBanner] {
        var out: [ProjectBanner] = jobs.map { .job(id: $0.id, kind: $0.kind, progress: $0.progress) }
        if let i = projectInfo {
            if i.rateMismatch {
                out.append(.rateMismatch(device: i.deviceRate, project: i.format.sampleRate))
            }
            if i.mismatched > 0, !jobs.contains(where: { $0.kind == .convert || $0.kind == .format }) {
                out.append(.mismatched(count: i.mismatched, label: formatLabel(format: i.format)))
            }
        }
        if transport.faults.contains("stream-underrun"), !diskWarningDismissed {
            out.append(.diskTooSlow)
        }
        if !jobProblems.isEmpty { out.append(.problems(jobProblems)) }
        return out
    }
}
VIZDAW_EOF
```

Some names may not match the tree:
- `AlertItem(title:message:)`. If `AlertItem` has no memberwise initialiser of that shape, use the one the file uses for plain messages.
- `jobs.append(JobRow(… kind: .import …))` records a placeholder. `pollJobs()` right after replaces it with the session's real row, which has the right kind.
- `diskWarningDismissed` is declared `@ObservationIgnored`. The banner must still update when it changes, so make it a plain observed `var` (no `@ObservationIgnored`) if the test `diskTooSlowFollowsTheFaults` fails to see the change.

Create `app/Sources/VizDAW/ProjectBanners.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/ProjectBanners.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The bars above a document's editors (plan 5a Task 13): jobs, media in
/// another format, a device at another rate, disk underruns, problems.
struct ProjectBanners: View {
    @Bindable var model: DocumentModel

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(model.banners.enumerated()), id: \.offset) { _, b in
                row(b)
                Divider()
            }
        }
    }

    @ViewBuilder
    private func row(_ b: ProjectBanner) -> some View {
        HStack(spacing: 8) {
            switch b {
            case .job(let id, let kind, let progress):
                ProgressView(value: Double(progress)).frame(width: 120)
                Text(kind == .import ? "Importing audio…" : "Converting media…")
                Spacer()
                Button("Cancel") { model.cancelJob(id) }
            case .mismatched(let count, let label):
                Image(systemName: "exclamationmark.triangle")
                Text("\(count) media file\(count == 1 ? " is" : "s are") not in the project format (\(label)) and stay silent.")
                Spacer()
                Button("Convert \(count) File\(count == 1 ? "" : "s")") { model.convertMedia() }
            case .rateMismatch(let device, let project):
                Image(systemName: "speaker.slash")
                Text("The audio device runs at \(Self.khz(device)); this project is \(Self.khz(Double(project))). Audio regions are muted.")
                Spacer()
                Button("Change Project Rate to \(Self.khz(device))") { model.changeRateToDevice() }
            case .diskTooSlow:
                Image(systemName: "externaldrive.badge.exclamationmark")
                Text("Disk too slow: some audio played as silence.")
                Spacer()
                Button("Dismiss") { model.dismissDiskWarning() }
            case .problems(let lines):
                Image(systemName: "exclamationmark.circle")
                Text(lines.joined(separator: "\n")).lineLimit(3)
                Spacer()
                Button("Dismiss") { model.dismissProblems() }
            }
        }
        .font(.callout)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(.bar)
    }

    static func khz(_ hz: Double) -> String {
        let k = hz / 1000
        return k.rounded() == k ? "\(Int(k)) kHz" : String(format: "%.1f kHz", k)
    }
}
VIZDAW_EOF
```

Create `app/Sources/VizDAW/ProjectPanels.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/ProjectPanels.swift <<'VIZDAW_EOF'
import AppKit
import UniformTypeIdentifiers
import VizDAWCore

/// The New Project and Project Settings panels (spec §4.1): a modal alert
/// with a rate and a depth pop-up.
@MainActor
enum FormatPanel {
    static let depths: [(BitDepthRow, String)] = [(.int16, "16-bit"), (.int24, "24-bit"), (.float32, "32-bit float")]

    /// Asks for a format; nil when cancelled.
    static func run(title: String, message: String, initial: ProjectFormatRow, ok: String) -> ProjectFormatRow? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: "Cancel")
        let rates = NSPopUpButton()
        for r in projectRates() {
            rates.addItem(withTitle: ProjectBanners.khz(Double(r)))
            rates.lastItem?.tag = Int(r)
        }
        rates.selectItem(withTag: Int(initial.sampleRate))
        let depth = NSPopUpButton()
        for (k, (d, name)) in depths.enumerated() {
            depth.addItem(withTitle: name)
            depth.lastItem?.tag = k
            if d == initial.depth { depth.selectItem(at: k) }
        }
        let row = NSStackView(views: [NSTextField(labelWithString: "Sample rate:"), rates,
                                      NSTextField(labelWithString: "Bit depth:"), depth])
        row.orientation = .horizontal
        row.frame = NSRect(x: 0, y: 0, width: 440, height: 28)
        alert.accessoryView = row
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let d = depths.indices.contains(depth.indexOfSelectedItem) ? depths[depth.indexOfSelectedItem].0 : initial.depth
        return ProjectFormatRow(sampleRate: UInt32(rates.selectedTag()), depth: d)
    }
}

extension Panels {
    /// Audio files to import (any the system reads).
    static func chooseAudio() -> [String] {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.audio]
        panel.prompt = "Import"
        panel.message = "Choose audio files: they are converted into the project's format."
        return panel.runModal() == .OK ? panel.urls.map(\.path) : []
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/AppCommands.swift`:

Find:
```swift
            Button("New") { show(store.newEmpty()) }
                .keyboardShortcut(KeyMenu.shortcut("file.new"))
            Button("Open…") { if let path = Panels.chooseProject() { show(store.open(path: path)) } }
                .keyboardShortcut(KeyMenu.shortcut("file.open"))
```
Replace:
```swift
            Button("New") {
                if let f = FormatPanel.run(
                    title: "New Project", message: "Every audio file of the project is stored in this format.",
                    initial: store.defaultFormat, ok: "Create")
                {
                    store.defaultFormat = f
                    show(store.newProject(f))
                }
            }
            .keyboardShortcut(KeyMenu.shortcut("file.new"))
            Button("Open…") { if let path = Panels.chooseProject() { show(store.open(path: path)) } }
                .keyboardShortcut(KeyMenu.shortcut("file.open"))
            Button("Import Audio…") {
                let paths = Panels.chooseAudio()
                doc?.importAudio(paths: paths)
            }
            .keyboardShortcut(KeyMenu.shortcut("media.import"))
            .disabled(doc == nil)
```

Find:
```swift
            Button("Save As…") { save(as: true) }
                .keyboardShortcut(KeyMenu.shortcut("file.save_as"))
                .disabled(doc == nil)
```
Replace:
```swift
            Button("Save As…") { save(as: true) }
                .keyboardShortcut(KeyMenu.shortcut("file.save_as"))
                .disabled(doc == nil)
            Divider()
            Button("Project Settings…") { projectSettings() }
                .disabled(doc == nil)
```
and add to `AppCommands`:
```swift
    private func projectSettings() {
        guard let m = doc, let info = m.projectInfo ?? (try? m.session.projectInfo()) else { return }
        let media = (try? m.session.media()) ?? []
        let files = media.count
        let seconds = media.reduce(0.0) { $0 + Double($1.frames) / Double(max($1.sampleRate, 1)) }
        let minutes = seconds >= 60 ? String(format: "%.0f min", seconds / 60) : String(format: "%.0f s", seconds)
        let note = files == 0
            ? "The project has no audio files yet."
            : "Changing the format converts \(files) audio file\(files == 1 ? "" : "s") (\(minutes) of audio) into new files (one undo step)."
        guard let f = FormatPanel.run(
            title: "Project Settings", message: "Format: \(formatLabel(format: info.format)). \(note)",
            initial: info.format, ok: "Change")
        else { return }
        if f != info.format { m.changeFormat(f) }
    }
```

In `app/Sources/VizDAW/DocumentView.swift`:

Find:
```swift
        } detail: {
            EditorSplit(model: model, docID: docID)
        }
```
Replace:
```swift
        } detail: {
            VStack(spacing: 0) {
                ProjectBanners(model: model)
                EditorSplit(model: model, docID: docID)
            }
        }
```

In `app/Sources/VizDAW/VizDAWApp.swift`'s `AppDelegate`, add:
```swift
    func applicationDidBecomeActive(_ notification: Notification) {
        MainActor.assumeIsolated {
            for m in appStore.documents.values { m.recheckMedia() }
        }
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -5`
Expected: ok, with 8 new tests.

Run: `scripts/make-app.sh` (the bundle builds).

- [ ] **Step 5: Lint and commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
git add app/Sources app/Tests
git commit -m "app: New Project format, Project Settings, Import Audio, banners

File > New asks for the project format (remembered as the default),
Import Audio converts files into the project as a job, Project Settings
changes the format; bars above the editors show jobs, media in another
format (Convert), a device at another rate, disk underruns and a job's
problems. Media is rechecked when the app becomes active.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```

---

### Task 14: End to end, and the by-eye checklist

**Files:**
- Modify: `crates/vizctl/Cargo.toml` (dev-dependency `core-media`), `crates/vizctl/tests/e2e.rs`, `app/README.md`, `Cargo.lock`
- Test: `crates/vizctl/tests/e2e.rs`

**Interfaces:**
- Consumes: everything above, through the control channel only.
- Produces: the e2e test `audio_import_plays_and_reports_media`, and README checklist steps 109–116 under "### Audio playback (plan 5a)".

Spec §12.3, the playback e2e line ("`project.new` at 44.1 kHz / 16-bit, import a 48 kHz file: check the media's stored format"), extended to playing the imported region and changing the format.

- [ ] **Step 1: Write the failing test**

In `crates/vizctl/Cargo.toml` under `[dev-dependencies]` add `core-media = { path = "../core-media" }`.

Append to `crates/vizctl/tests/e2e.rs`:

```rust
/// A 48 kHz stereo float CAF of `frames` frames of a 0.25 constant (the
/// app's importer reads CAF like any other format).
fn audio_file(path: &std::path::Path, frames: usize) {
    let mut w = core_media::caf::CafWriter::create(
        path,
        48_000,
        2,
        core_model::format::BitDepth::Float32,
        core_media::caf::WriteMode::Normal,
    )
    .unwrap();
    w.write(&vec![0.25f32; 2 * frames]).unwrap();
    w.finish().unwrap();
}

/// README steps 109–116 through the channel (plan 5a): a 44.1 kHz/16-bit
/// project stores an imported 48 kHz file in its own format; the demo
/// imports into a new lane, plays the region without disk faults, and
/// changes its format with the media converted.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn audio_import_plays_and_reports_media() {
    let mut app = App::start();
    let src = app.launched.home.join("tone.caf");
    audio_file(&src, 96_000);

    // A new project in a chosen format.
    let made = app.call(
        "command.run",
        json!({"id": "file.new", "params": {"rate": "44100", "depth": "16"}}),
    );
    let window = made["window"].clone();
    let p = app.call("state.get", json!({"target": "project", "window": window}));
    assert_eq!((p["rate"].clone(), p["depth"].clone()), (json!(44_100), json!("16")), "{p}");
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": src}, "window": window}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let m = app.call("state.get", json!({"target": "media", "window": window}));
    let item = &m["items"][0];
    assert_eq!(item["name"], "tone", "{m}");
    assert_eq!(item["rate"], 44_100);
    assert_eq!(item["offline"], false);
    assert!(
        item["frames"].as_u64().unwrap().abs_diff(88_200) <= 1,
        "converted to 44.1 kHz: {item}"
    );

    // The demo: import into a new lane at bar 1 and play it.
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w2 = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w2}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let lanes_before = doc["lanes"].as_array().unwrap().len();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": src, "seq": seq, "tick": 0}, "window": w2}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let doc = app.call("state.get", json!({"target": "document", "window": w2}));
    let lanes = doc["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), lanes_before + 1, "{doc}");
    assert_eq!(lanes.last().unwrap()["regions"], 1, "{doc}");
    app.call("command.run", json!({"id": "transport.play_stop", "window": w2}));
    app.wait(&w2, json!({"condition": "playing"}));
    app.wait(&w2, json!({"condition": "tick_at_least", "tick": 2 * 15_360}));
    let t = app.call("state.get", json!({"target": "transport", "window": w2}));
    let faults: Vec<String> = t["faults"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap_or("").to_string())
        .collect();
    assert!(
        !faults.iter().any(|f| f == "stream-underrun"),
        "the region streamed in time: {t}"
    );
    assert_eq!(t["rate_mismatch"], false, "{t}");
    app.call("command.run", json!({"id": "transport.play_stop", "window": w2}));
    app.wait(&w2, json!({"condition": "stopped"}));

    // Project Settings: 24-bit to float, every file converted.
    let r = app.call(
        "command.run",
        json!({"id": "file.project_settings", "params": {"depth": "32f"}, "window": w2}),
    );
    assert_eq!(r["undo"], "Change Project Format", "{r}");
    let p = app.call("state.get", json!({"target": "project", "window": w2}));
    assert_eq!((p["depth"].clone(), p["mismatched"].clone()), (json!("32f"), json!(0)), "{p}");
    let m = app.call("state.get", json!({"target": "media", "window": w2}));
    let tone = m["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "tone")
        .unwrap()
        .clone();
    assert_eq!(tone["offline"], false, "{m}");
    app.quit();
}
```

Two things depend on the tree:
- The demo's `"selected_seq"` must be the sequence that plays from bar 1 (Song). If the demo selects another sequence first, use the one named "Song" from `doc["sequences"]`.
- `tick_at_least` waits on the transport's tick. Two beats into Song is well inside the imported 2 s region at 120 BPM.

- [ ] **Step 2: Run the test to verify it fails before the app has the feature**

The feature is already built when this task runs. Build and run the test; it passes when Tasks 1–13 are complete. If it fails, the failure points at the task to fix:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/make-app.sh
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1 2>&1 | tail -15
```
Expected: `test result: ok. 5 passed` (4 earlier + this one).

- [ ] **Step 3: Write the by-eye checklist**

In `app/README.md`, after step 108, add:

```markdown
### Audio playback (plan 5a)

Use a throwaway home (`vizctl launch`) and a few audio files of your own:
a WAV, an AIFF, an MP3 or AAC, and one at 44.1 kHz.

109. New project: File > New asks for the sample rate and bit depth,
     pre-filled with 48 kHz / 24-bit; choose 44.1 kHz / 16-bit. The next
     File > New offers 44.1 kHz / 16-bit. File > Project Settings… shows
     "44.1 kHz, 16-bit" and that the project has no audio files yet.
110. Import Audio: open the demo, select a lane, put the playhead on bar
     3 and choose File > Import Audio… with one stereo WAV. A bar shows
     "Importing audio…" with progress, then the region sits on that lane
     at bar 3. Undo removes it (one step, "Import Audio"). Import two
     files: each gets a new lane "Audio n". A text file renamed `.wav` in
     the batch is listed in a bar ("x.wav: …") and the others import.
111. It plays: Play from bar 1. The region sounds through the output path
     strip "Audio 1" in the mixer; its fader, pan, mute and solo work; the
     region's sound starts at its bar exactly (record the output or
     listen against the click of a MIDI lane).
112. Loops and locates: loop bars 3–4 over a long region: the loop wraps
     with no gap or click beyond the region's own audio at the seam.
     Click in the ruler in the middle of a region while playing: the
     sound resumes from there at once (a late start may show as a fault
     dot; no "Disk too slow" bar on a normal disk).
113. Formats: File > Project Settings… > 48 kHz / 24-bit says how many
     files it converts; Change: a progress bar, then the regions sit
     where they were and sound the same. Edit > Undo ("Change Project
     Format") restores 44.1 kHz and the old files.
114. Media in another format: in a saved project, replace a file in its
     `Media/` folder (Finder > Show Package Contents) with a CAF at
     another rate, then switch back to VizDAW: a bar says "1 media file
     is not in the project format…"; that region is silent. Convert: the
     bar goes and the region plays. `$V state media` lists the item with
     `"offline": false` afterwards.
115. Missing media: rename a media file in Finder and switch back: its
     region is silent and `$V state media` shows `"offline_reason":
     "missing"`; rename it back and switch again: it plays.
116. Device rate: in Audio MIDI Setup set the output to 44.1 kHz, open a
     48 kHz project and press Play: VizDAW switches the device to 48 kHz
     (Audio MIDI Setup shows it). With a device that cannot run at 48 kHz
     (or set the project to 192 kHz on a 48 kHz-only device): a bar says
     regions are muted, MIDI still plays, and "Change Project Rate to …"
     converts the project to the device's rate.
```

- [ ] **Step 4: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-engine --features rt-check --test rt_alloc 2>&1 | tail -2
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
scripts/make-app.sh
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1 2>&1 | tail -3
pgrep -lf MacOS/VizDAW || echo "no VizDAW running"
rm -rf /tmp/vze*
```
Expected: everything green, and no VizDAW left running.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl app/README.md Cargo.lock
git commit -m "e2e: import into a 44.1 kHz/16-bit project, play a region, change format

The channel creates a project in a format, imports a 48 kHz file (stored
at 44.1 kHz/16-bit), imports into the demo's new lane, plays it without
disk faults and converts every file to 32-bit float. README checklist
steps 109-116 cover audio playback by eye.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline
```
