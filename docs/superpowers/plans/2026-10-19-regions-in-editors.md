# VizDAW 5b — Regions in the Editors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The second plan of sub-project 5. Audio regions become editable in the editors: every media item gets a waveform overview (built in the background, cached in the package, served over FFI at any zoom); the arrangement and the region strip draw waveforms shaped by gain and fades, with fade and gain handles and a fade-shape menu; the Region menu gets Normalize…, Fade In to Cursor, Fade Out from Cursor and Remove Fades; audio files dropped on the editors import; a Media Pool window lists, renames, deletes, compacts, relinks and reveals media; an offline/relink dialog appears when a project opens with missing or changed media; and media files nothing references any more go to the macOS Trash on save and on close.

**Architecture:** Rust does the work and Swift draws it (the user's guideline).
- **`core-model`**
  - `AudioCmd::Normalize` (non-destructive region gain from a measured peak), `AudioCmd::FadeTo` and `AudioCmd::RemoveFades`.
  - `MediaCmd::Relink` rescales regions when the new file's rate differs (the parked 5a Task 2 Minor), like Convert and SetFormat.
- **`core-media`**
  - `overview`: the `.vzov` file (min/max peaks at 256, 4096 and 65 536 frames per peak), built in one read that also hashes the file, read and written atomically, queried by frame range, plus exact sample bins and peaks for zoomed-in drawing and Normalize.
  - Imports build the overview of every file they write, and never pick a file name the caller reserves.
- **`platform-macos`**: `trash::move_to_trash` through `NSFileManager`.
- **`core-ffi`**
  - A per-session overview worker (build, verify by content hash, load, read sample windows) that never touches the streamer or the real-time path; `Session.overview`, `Session.region_waveform`; the offline reason `Changed`.
  - `Session.normalize_regions`; `AudioEdit::{FadeTo, RemoveFades}`.
  - Media-pool calls (rename, remove with its regions, compact, users, file, used %), relink and search-folder jobs, reserved names for every job.
  - The retention rule: on save and on close, files under `Media/` that neither the history nor the saved project nor its autosaves reference go to the Trash.
- **`core-control`**: eight registry commands (`window.media_pool`, `media.relink`, `media.compact`, `media.rename`, `region.normalize`, `region.fade_in_to_cursor`, `region.fade_out_from_cursor`, `region.remove_fades`).
- **Swift**: `MediaModel`, `RegionCommands`, waveform/handle geometry and drawing in `ArrangementView` and `RegionStripView`, file drops, the Media Pool window, the Relink sheet, the Region menu and the control handlers.

**Tech Stack:** Rust 2024 (`core-model`, `core-media`, `core-ffi` with UniFFI 0.32.2, `core-control`, `platform-macos` with `objc2` 0.6 and `objc2-core-foundation` 0.3.2, `vizctl`), Swift 6 / SwiftPM (SwiftUI shell, AppKit views, Swift Testing), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`. This plan covers the spec's 5b row in §13:
- §4.3 media (`RenameMedia`, `RelinkMedia`, `CompactMedia` in the UI; the offline set's "content hash differs" case, deferred from 5a Ruling 4);
- §4.4 regions (`Normalize`, gain range);
- §6.5 "Recorded media and undo" — the retention rule (Trash on save and close), which covers compact;
- §10.1 waveform overviews; §10.2 region editing; §10.3 the media pool window and the relink dialog;
- §12.1, the `media.relink`, `media.compact`, `media.rename` and `region.normalize` commands and the `state.get media` fields;
- §12.3, the overview test ("overview levels against a brute-force min/max"), the Swift lines ("`MediaModel` with `FakeSession`", "handle hit-testing and drag maths for fades and gain", "the overview drawing geometry", "every new registry ID has a handler").

The parent spec is `2026-10-15-vizdaw-design.md` (§5 media pool and audio regions, §8 audio files, §10 offline media). Plan 5a (`docs/superpowers/plans/2026-10-18-audio-playback.md`) is complete at `4f280b8`; its ledger and reviews (`.superpowers/sdd/archive/5a-*.md`) carry the items listed under "Carried and parked items" below. Read both specs.

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. The Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools.
  - A `git commit` without `DEVELOPER_DIR` fails silently, so check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
  - Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift only through the scripts.** `scripts/build-ffi.sh` (after any Rust change, before Swift), `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`. Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` (head `4f280b8` before this plan's commit, plus that commit). Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists (by path, never `git add -A`), plus `Cargo.lock` when a manifest changed. Never commit `.superpowers/` or `.DS_Store`.
  - End every commit message with the two attribution lines your dispatch gives you. The blocks below show:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg
    ```
    Use the `Co-Authored-By` line naming the model you actually are.
- **Clean room.** Never read or copy anything from `~/workspace/SVP/research`, the Studio Vision binary or any research material. No Opcode, Midas or Steinberg assets, names, artwork or branding anywhere in code, UI text, icons or docs.
- **No GUI tools.**
  - Never use GUI, computer-use or accessibility-automation tools. Launch VizDAW only through `vizctl launch` (a throwaway home) or the e2e test, and quit it when done (`vizctl quit`; `pkill -f "VizDAW --control"` if a test left one).
  - At the end of a task that launched the app, check that `pgrep -lf MacOS/VizDAW` prints nothing, and remove the `/tmp/vze*` homes your e2e runs left.
  - Never write to the real `~/Library/Application Support/VizDAW` or the real preferences. Tests use temp directories (`tempfile`).
  - Never move anything to the real Trash from a test: core-ffi tests give each session a trash folder (`Session::debug_set_trash_dir`), and with the `test-hooks` feature the platform Trash is never called (Ruling 11). The e2e sets `VIZDAW_TRASH_DIR` inside its throwaway home.
- **Crates and dependencies.**
  - Rust stable, edition 2024. No new crate and no new dependency. `platform-macos` links the Foundation framework for `NSFileManager` (a `#[link(name = "Foundation", kind = "framework")]` block; Foundation ships with macOS).
  - License GPL-3.0-only.
- **Lints.** Every crate keeps `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. New non-test Rust code uses `.get()`, iterators and `checked_*`/`saturating_*` instead of indexing and unwraps.
- **Real-time rules (spec §3).** The real-time thread never allocates, locks or does file I/O. This plan adds nothing to the render path: overviews are built and read by a per-session worker thread with its own `CafReader`, never through `Streamer`, `StreamReader` or the engine. No task edits `core-engine` or `core-media/src/stream.rs`. Because `core-media` changes, Tasks 2, 3 and 13 still run the rt-check gates (`cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings` and `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph`).
- **Media files are write-once** (5a Task 10 review; Save As hard-links media between packages). Nothing in this plan opens an existing file under `Media/` for writing. Normalize, fades and gain are model edits; relink and search-folder write *new* files under free names through `import_reserved`; retention only moves whole files to the Trash. Overview files (`Media/.overviews/*.vzov`) are a cache, not media: they are written to a temp file and renamed into place, may be replaced, and may be deleted.
- **File compatibility.** `core_model::file::VERSION` stays 3. No model field is added; `MediaItem`, `Fade` and `Settings` serialize exactly as in 5a.
- **FFI safety.** No panic crosses the FFI: every new exported function or method returns `Result<_, VizError>` and runs inside `ffi_guard`. No session `inner` lock is held across a file conversion, an overview build or a whole-file read. Jobs and the overview worker run on their own threads; their results are applied under the lock by `idle`/`job` (5a Ruling 15 and final review I1: never while an edit is open).
- **UniFFI changes.**
  - Changed records (new field last, with a default, so existing Swift literals compile):
    - `IdleUpdate.overviews_changed: bool`, `#[uniffi(default = false)]` (Task 4);
    - `MediaRow.used_percent: f32`, `#[uniffi(default = 0.0)]` (Task 6).
  - Enums that gain a variant (appended last): `OfflineReasonRow::Changed` (Task 4), `AudioEdit::{FadeTo, RemoveFades}` (Task 5), `JobKindRow::Relink` (Task 6), `WindowKindRow::MediaPool` (Task 9).
  - New records and enums: `OverviewRows`, `OverviewStateRow` (Task 4), `MediaUseRow` (Task 6).
  - `FakeSession` in `app/Tests/VizDAWAppModelTests/FakeSession.swift` implements every new `SessionProtocol` method in the same task that adds it.
- **Swift.**
  - The generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAWEditors`, `VizDAW` and the tests compile in Swift 6 mode with no new warnings (the `MemoryDefaults` Sendable warning is older).
  - Never use SwiftUI's `@State` or `@FocusState` in new views (the Command Line Tools cannot expand them); view state lives in `@Observable` models or spelled-out `State(initialValue:)` as `MixerWindow` does.
  - Closures stored by views capture `[weak self]`.
  - **Semantic colours only** in new UI: system/semantic `NSColor`s and SwiftUI styles (`.secondary`, `.bar`, `.tint`), never RGB or hex literals. Every colour the region drawing uses is named once in `RegionPalette` (Task 10), so the planned SP7 light and dark themes can recolour regions, fades, gain lines and offline marks in one place.
  - **Accessibility labels** on every new control: SwiftUI buttons get text or `.accessibilityLabel`, decorative images `.accessibilityHidden(true)`, canvas elements a `CanvasAccessibilityItem` label.
  - New model tests go to `VizDAWAppModelTests`; pure geometry tests to `VizDAWEditorsTests`.
- **Control channel** (control spec §6, §7).
  - Programmatic calls never open dialogs: a command whose menu item opens a dialog takes the dialog's answers as parameters and fails with `dialog_required` without them; the relink sheet never opens while `ControlChannel.active`.
  - Every command is in `core_control::registry::COMMANDS` and handled by `ControlCommands.run`; the Swift test `everyRegisteredMenuCommandHasAHandler` compares the two.
  - The registry count test moves from 111 to 119 in Task 9.
  - Job outcomes in new UI and on the channel go through `ControlJobs.answer`: a failed job is an alert (or the call's failure), warnings a Problems banner (or the result's `status`).
- **Generated files** are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
  - Tasks 2, 3 and 13 also run the rt-check gates above.
  - Tasks 4–13 also run `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 13 also runs `scripts/make-app.sh` and the e2e (`VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`).
  - Known flaky tests: `core-ffi` catalog tests under parallel load, the `damaged_files` proptest and the MIDI listener test. Re-run once before investigating, and say so in the report.
  - Baseline at `4f280b8`: Rust 1034 passed, 0 failed, 8 ignored; Swift 700 tests in 80 suites; e2e 5 tests.
- **How code is given.**
  - New files are written whole (`cat > … <<'VIZDAW_EOF'`).
  - Changes to existing files are exact **Find / Replace** excerpts of the tree the previous task left, applied in order with an editor (the Edit tool).
  - If an excerpt does not match because a fix landed meanwhile, make the same change to the current text and say so in the report. If a name in the plan differs from the tree, use the tree's and say so.
- **Spec values used verbatim:**
  - "levels of 256, 4096 and 65 536 frames per peak, per channel, i16 min/max";
  - "`Media/.overviews/<content hash>.vzov`, rebuilt in the background if missing or stale";
  - "Below 256 frames per column it reads samples directly";
  - "It sets `gain_db` so that the region's loudest sample (taken from its overview) reaches `peak_db`";
  - "Regions keep `gain_db` within −144…+24 dB";
  - "The top-left and top-right corner handles drag the fade lengths"; "The top-edge line drags the gain (±dB readout). ⌥-click resets it to 0 dB"; "Each drag is one transaction";
  - "Normalize… (peak dB), Fade In to Cursor / Fade Out from Cursor, Remove Fades";
  - "Dropping onto empty space below the lanes creates new lanes";
  - media pool rows "name, duration, channels, origin, use count, used percentage, and an offline mark"; actions "select regions using an item; Select Unused; Compact …; Rename; Relink…; Reveal in Finder; Import…";
  - relink "Locate… (a file panel) and "Search folder…", which matches by content hash, then name and length"; "Each relink is one undo step"; ""Leave offline" closes the dialog";
  - retention "files under `Media/` referenced by neither are moved to the macOS Trash. They are never unlinked".

## Review Focus

These are the five failure modes a user is most likely to hit. The spec implies them but no requirement spells them out. Each is pinned by a named test in its owning task.

1. **The Trash takes a file something can still bring back.** The user imports, undoes, saves; or deletes a region and compacts, then closes without saving; or restores an autosave written before a compact; or an autosave is damaged.
   - Expected: a file is trashed only when no undo or redo state, no open transaction, the saved `project.json` and no autosave references it; if any of those cannot be read, nothing is trashed; files are moved, never unlinked; a failed move leaves the file and reports a problem.
   - Pinned by `retention_never_trashes_what_undo_redo_or_disk_documents_reference`, `an_unreadable_autosave_trashes_nothing` and `a_failed_trash_keeps_the_file_and_says_so` (Task 7).
2. **A media file replaced outside VizDAW with one of the same format and length.** The user swaps a file in Finder (or an external tool rewrites it in place).
   - Expected: the next verification (open, recheck, or any time the overview is checked) finds the content hash differs, the item goes offline with reason `changed` (silent, hatched, listed in the relink dialog), never plays the wrong audio after verification, and comes back when the original returns.
   - Pinned by `a_same_length_replacement_goes_offline_changed_and_back` (Task 4).
3. **Damaged or hostile cache and project data.** A truncated or garbage `.vzov`; an overview written by another build; a project whose `content_hash` is `../../evil` or a 10 MB string.
   - Expected: damaged overviews are refused and rebuilt, never trusted, never a panic; a hash that is not lowercase hex never names a file, so nothing is written outside `Media/.overviews/`.
   - Pinned by `damaged_overviews_are_refused` (Task 2) and `a_hostile_content_hash_never_names_a_path` (Task 4).
4. **Relinking to the wrong file.** A shorter file, a file at another rate, a four-channel file, a text file, the file already inside the package, a folder where two files share the item's name, a folder with nothing that matches.
   - Expected: a shorter file is refused (one problem, nothing changed, its copy removed); another rate is converted and the regions rescaled; more than two channels and non-audio are per-item problems; a file already in this package's `Media/` is used as is; a hash match beats a name match; no match is a problem naming the item; each relink is one undo step.
   - Pinned by `relink_refuses_short_and_wide_files_and_converts_rates`, `relink_uses_a_file_already_in_the_package` and `search_prefers_the_hash_and_reports_what_it_misses` (Task 6), and `relink_rescales_regions_when_the_rate_differs` (Task 1).
5. **Waveform requests at absurd zooms and stale ids.** Zoomed to single samples, zoomed out to hours, `columns` 0 or a million, ticks at `i64::MIN`/`i64::MAX`, a region deleted (or undone) between the draw and the call, a region whose media is offline.
   - Expected: a clear `Model` error for bad arguments and unknown ids, `Offline` rows for offline media, columns outside the region drawn as silence, never a panic, never a blocking read on the calling thread.
   - Pinned by `waveform_requests_survive_hostile_ranges` (Task 4) and the Swift geometry test `degenerateRowsDrawNothing` (Task 10).

## Plan Rulings

These are decisions the spec leaves open. Each is implemented and tested as stated, in the format "what — why — cost if wrong".

1. **Ruling (overview file):** `Media/.overviews/<hash>.vzov`, little-endian:
   - header (284 bytes): `"VZOV"`, `u32` version 1, `u16` channels (1–2), `u16` level count (3), `f32` scale, `u64` frames, 64 bytes of the content hash (lowercase hex, NUL-padded), `u32` stamp count (≤ 8), then 8 stamp slots of 24 bytes (`u64` file length, `i64` mtime seconds, `u32` mtime nanoseconds, `u32` zero);
   - then per level (256, 4096, 65 536 frames per peak): `u32` frames per peak, `u64` peak count (= `frames.div_ceil(fpp)`), and per peak, per channel, `i16` min then `i16` max.
   - Values are `sample / scale × 32767`, the min rounded down and the max rounded up (the envelope always contains the samples). `scale` is the file's largest absolute sample, at least 1.0, so float files with overs keep their true peak.
   - Why: spec §10.1's levels and i16; the scale and outward rounding make Normalize safe on float media.
   - Cost if wrong: an overview format bump (the version field) and a rebuild.
2. **Ruling (identity and verification):** an overview is trusted for a media item when its file name and header hash equal the item's `content_hash` and one of its stamps equals the media file's current `(length, mtime)`. Otherwise the worker reads the whole file once, hashing its sample bytes as it builds; a hash that differs from the item's marks the item offline with the new reason `Changed` (spec §4.3 "its content hash differs", deferred from 5a Ruling 4), and nothing is cached under the expected hash. A successful build adds the file's stamp (at most 8, newest kept), so the same audio under several paths stays cached.
   - A `Changed` mark holds until a later full read of the same `(path, hash)` succeeds (the original file is back) or the item's path or hash changes (relink). Rechecks re-verify marked items.
   - Items without a `content_hash` (pre-v3 media) get an overview under the hash the build computes, kept in memory per item.
   - Why: hashing every file on every open is slow (5a Ruling 4); a stat stamp is cheap and a full read is needed to draw anyway.
   - Cost if wrong: a file edited in place by a tool that restores the mtime plays until the next full read.
3. **Ruling (the worker):** each session owns one overview worker thread (`core_ffi::overviews::Overviews`), started on demand and joined on `close`/drop. It processes "ensure" requests for every online item (and every `Changed` item), and "window" requests for exact sample bins, windows first. It reads files with its own `CafReader`; it never touches the engine, the streamer or the RAM cache. Reads on the FFI (`Session.overview`, `Session.region_waveform`) are memory lookups: an overview not loaded yet answers `Pending`; a range under 256 frames per column answers from cached sample bins, or (state `Coarse`) from the level-256 peaks while the worker reads the bins. `IdleUpdate.overviews_changed` tells the app to redraw.
   - Why: spec §10.1 "rebuilt in the background"; the user asked that drawing never block.
   - Cost if wrong: one thread per open document, idle most of the time.
4. **Ruling (region waveforms in Rust):** `Session.region_waveform(event, from_tick, to_tick, columns)` maps each column's tick span through the region's sequence tempo map to media frames, reads min/max, and scales them by the region's gain and by its fade gain at the column's centre (the engine's `fade_curve`). Columns outside the region are zero. `columns` is 1–8192.
   - Why: spec §10.1 "scaled by `gain_db` and shaped by their fades"; tempo changes inside a region make pixels non-linear in frames, which only the model knows.
   - Cost if wrong: none.
5. **Ruling (Normalize):** `AudioCmd::Normalize { regions: Vec<(EventId, f32)>, peak_db }` pairs each region with the linear peak of its span; the model sets `gain_db = peak_db − 20·log10(peak)`, clamped to −144…+24 dB. `peak_db` must be finite in −96…0. Silent regions (peak 0) are skipped with a warning; if none is left, the command is refused. Non-region events are ignored. The FFI measures the peak from the overview's 256-frame level for whole blocks inside the span and from the samples for the partial blocks at both ends, so samples just outside the region never count. Label "Normalize".
   - Why: spec §4.4 and Ruling 28 ("computed from the overview peak"); exact edges matter for short regions cut from loud files.
   - Cost if wrong: Normalize lands up to one i16 step (≤ 0.03 dB at −40 dBFS) under the target.
6. **Ruling (fade commands):** `AudioCmd::FadeTo { events, at, edge }` sets the fade in to end at `at` (edge `In`) or the fade out to start at `at` (`Out`) for every selected region `at` lies strictly inside, converting through the tempo map and media rate, and shortens the other fade if both would no longer fit. `AudioCmd::RemoveFades { events }` sets both fades to zero length, linear. Regions the cursor is not inside are left alone; if no region changed, the command is refused ("the cursor is not inside a selected audio region"). Labels "Fade In to Cursor", "Fade Out from Cursor", "Remove Fades".
   - Cost if wrong: one function.
7. **Ruling (handles):** a region at least 24 points wide shows two 7-point corner squares at the top edge, at the fade-in end and the fade-out start, and a 4-point gain band along the top edge between them. Dragging a corner sets that fade (frames per point = region frames / region width), clamped so the fades fit. Dragging the band changes the gain by 0.25 dB per point up (0.05 with ⇧), rounded to 0.1 dB, clamped to −144…+24; a readout shows the gain while dragging. ⌥-click on the band sets 0 dB. A right-click inside a fade's area (between the region edge and its handle) opens the fade-shape menu (Linear, Equal Power, Exponential, S-Curve). Each drag is one gesture (`beginGesture`/`updateGesture`/`endGesture`) labelled "Fade In", "Fade Out" or "Region Gain", origin `.other` (exact mode does not intercept it). Light drawing mode draws neither waveforms nor handles.
   - Why: spec §10.2.
   - Cost if wrong: geometry constants in one file.
8. **Ruling (drops):** files dropped on the arrangement import at the drop point's tick (snapped when snap is on, never before 0) into the lane under the pointer; below the last lane, `lane` is `nil` (new lanes). The core's Ruling 19 still applies (several files or a split file get new lanes). On the region strip, the lane is that of the region under the pointer, else the piano roll's edit lane. The drop uses the existing import job.
   - Cost if wrong: none.
9. **Ruling (relink):** `Session.relink_media(media, path)` is a job (`JobKindRow::Relink`). The chosen file is imported into `Media/` (converted to the project format by the platform importer, named after the chosen file's stem, avoiding reserved names, its overview built) and the item is relinked to the new file — never to a file outside the package. The exception: a VizDAW CAF in the project format that already lies directly in this package's `Media/` is used as it is. Applying a relink is `MediaCmd::Relink`, one undo step "Relink Media" per item; a refusal (the file is too short) removes the new file and becomes a problem. `MediaCmd::Relink` rescales the item's regions by `new_rate / old_rate` when they differ (rounded; a region may overrun the new end by at most one frame of rounding, and is then clamped); at the same rate a region past the new end is refused, as before.
   - Why: media paths are package-relative (`check_media_path`), and write-once media must not be rewritten in place.
   - Cost if wrong: a large file is copied once.
10. **Ruling (search folder):** `Session.search_media(folder)` is a Relink job over every item offline as `Missing`, `Length` or `Changed`. It walks `folder` (at most 8 levels deep and 20 000 files; hidden entries skipped; symlinks not followed). First every VizDAW CAF is hashed and matched by content hash; then, for items still unmatched, a file whose stem equals the item's file stem or its name, and whose duration (the importer's probe) is within 1 ms of the item's, matches. The first match in walk order wins. Each matched item is relinked as in Ruling 9 (one undo step each); an item with no match is a problem "<name>: not found in <folder>".
    - Cost if wrong: a user locates a file by hand.
11. **Ruling (retention):**
    - **When:** after every successful Save and Save As (on the destination package) and on `close` of a saved project, unless a job is not yet applied (then it waits for the next save or close). Untitled scratch packages are removed whole at close, as in 5a.
    - **Keep set:** every path in `History::referenced_media_anywhere()`, every media path of the package's `project.json` and of every file in its `Autosave/`. If `project.json` exists but cannot be read, or any autosave cannot be listed, read or parsed, nothing is trashed.
    - **Candidates:** regular files anywhere under `Media/` (subfolders included, for 5c's `Media/Recorded/` and 5f's `Media/Edits/`) whose name and every folder on their path do not start with `.`. A candidate outside the keep set is moved to the macOS Trash (`NSFileManager trashItemAtURL`), never unlinked; a failed move keeps the file and reports "<path>: not moved to the Trash (…)" in the next `idle`'s problems.
    - **Caches:** overview files whose hash no kept item (in the history, the saved project or an autosave) has are deleted; files in `Media/.incoming/` are deleted (only an interrupted import leaves them, and no job is running).
    - **Where the Trash is:** a session's test trash folder (`Session::debug_set_trash_dir`, test hook), else `$VIZDAW_TRASH_DIR` (absolute; the e2e), else the platform Trash. Built with the `test-hooks` feature, without either of the first two, moving fails ("no trash folder in tests"), so tests can never touch the real Trash.
    - Why: spec §6.5 and Ruling 13; final review I3 says retention is defined over the history, never the present document alone; autosaves mirror `prune_states`.
    - Cost if wrong: files stay on disk longer than needed.
12. **Ruling (reserved names, final review r2):** every job that writes into `Media/` (import, convert, format change, relink, search) avoids every package-relative path any history state references, even when that file is missing on disk, as well as every file on disk (`link_free` already refuses existing names). So a path in the history always names one audio, and a missing item's name is never claimed by another file (5a Task 11 review Minor 6).
    - Cost if wrong: none.
13. **Ruling (pool operations):** `MediaRow.used_percent` is the union of the item's frames its regions play, over its length, × 100 (0 for an empty item). Deleting an item that regions use removes those regions and the item in one undo step "Remove Media" (`Session.remove_media`); the UI first asks, listing the regions ("Sequence › Lane, bar n"). Compact is `MediaCmd::Compact`, one undo step; its files go to the Trash by the retention rule once no history state references them. "Select regions using an item" selects them in the first sequence that has one and shows that sequence.
14. **Ruling (relink dialog):** when a project opens with items offline as `Missing`, `Length` or `Changed`, its window shows the Relink sheet (never under the control channel). File › Relink Media… shows it again any time. `Format` and `Unreadable` items keep 5a's Convert banner instead.
15. **Ruling (channel names):** the registry gains `window.media_pool` (Window › Media Pool), `media.relink { media, path }` (File › Relink Media…; dialog "Relink" answered by `path`; a job), `media.compact` and `media.rename { media, name }` (no menu item, group "Media"; `name` answers the "Rename Media" dialog), and the Region menu `region.normalize { peak_db }` (dialog "Normalize" answered by `peak_db`, a decimal text such as `"-0.3"`, checked by the app: finite, −96…0), `region.fade_in_to_cursor`, `region.fade_out_from_cursor`, `region.remove_fades`. The region commands act on the editor selection. `state.get media` items gain `used_percent`, and `offline_reason` gains `"changed"`. `state.get windows` reports the media pool as `"media_pool"`. The registry goes from 111 to 119 commands.
    - Why: the registry is the menu; `peak_db` is text because `ArgValue` has no float (adding one would break its `Eq`/`Hash`).
    - Cost if wrong: aliases later.
16. **Ruling (palette):** `RegionPalette` (VizDAWEditors) names every region colour: `body`, `outline`, `waveform`, `fadeLine`, `handle`, `gainLine`, `offline`, `readoutText`, `readoutBackground`. Today each maps to a system colour; SP7's themes replace this one enum.
17. **Ruling (Save As and overviews):** Save As copies the overview files of every hash the history references into the destination (best effort, ignored on failure; a missing one is rebuilt).
18. **Ruling (defaults):** the Normalize panel starts at −0.3 dB. The waveform readout and labels format gain as `"+3.0 dB"`, `"0.0 dB"`, `"-6.5 dB"`.

## Carried and parked items

Picked up by this plan (each has an owning task and a test):
- **Retention over the history** (5a final review I3 and the `referenced_media_anywhere` doc comment): Task 7.
- **Final review r2** (a trashed or missing name reused under one path): Ruling 12, Tasks 6 and 7.
- **5a Task 11 review Minor 6** (convert claims a missing item's name): Task 6.
- **Write-once media** (5a Task 10 review): Global Constraints; Rulings 5, 6, 9, 11.
- **5a Task 2 review Minor** (Relink does not rescale on a rate change): Task 1, and `Streamer::set_media` restarts the item because its path changes (5a final review I2), checked in Task 6.
- **5a Ruling 4** (hash check deferred to 5b): Ruling 2, Task 4.
- **5a Task 13 review M1/M3/M4** (job outcomes, accessibility, banner wording): new job UI goes through `ControlJobs.answer` (Tasks 8, 12), new banners and sheets carry labels (Task 12).
- **5a F20 park** (`.incoming` leftovers): Ruling 11 cleans them.
- **Control-channel rule** "programmatic calls never open dialogs": Rulings 14 and 15, Task 9.
- **e2e and README:** Task 13 continues the README at step 118.

Parked (not 5b's; listed so reviewers do not raise them):
- 5a final review r1 (one spurious underrun when an item restarts mid-play, now also on Relink while playing): Minor, stream.rs is out of scope.
- 5a final review m5 (Save As copies on the main thread) and m6 (`recheck_media` opens every file on the main thread): unchanged; the overview worker does not add main-thread I/O.
- 5c: recording, `Media/Recorded/`, crash recovery; 5f: external editor, `Media/Edits/`, bounce. The retention rule already covers their folders.

---

### Task 1: `core-model` — Normalize, fades to the cursor, Remove Fades, and Relink that rescales

**Files:**
- Modify: `crates/core-model/src/command/audio.rs`, `crates/core-model/src/command/media.rs`, `crates/core-model/src/command/mod.rs`
- Create: `crates/core-model/tests/cmd_regions.rs`

**Interfaces:**
- Consumes: 5a's `AudioCmd`, `MediaCmd`, `audio::{region, clamp_fades, store}`, `media::{regions_using, swap_media}`, `TempoMap::{tick_to_seconds, seconds_to_tick}`.
- Produces:
  - `core_model::command::FadeEdge::{In, Out}` (`Debug, Clone, Copy, PartialEq, Eq`);
  - `AudioCmd::Normalize { regions: Vec<(EventId, f32)>, peak_db: f32 }` — name `"Normalize"`;
  - `AudioCmd::FadeTo { events: Vec<EventId>, at: Tick, edge: FadeEdge }` — names `"Fade In to Cursor"` / `"Fade Out from Cursor"`;
  - `AudioCmd::RemoveFades { events: Vec<EventId> }` — name `"Remove Fades"`;
  - `MediaCmd::Relink` rescales regions on a rate change (Ruling 9);
  - `pub(crate) fn media::rescale_regions(d: &mut Document, media: MediaId, ratio: f64, len: u64) -> Result<()>` (shared by Convert, SetFormat and Relink).

Rulings 5, 6 and 9. All three region commands ignore events that are not audio regions and refuse an unknown event ID. Each is refused when it has nothing to act on: Normalize when no region has sound, FadeTo when the cursor is inside no region, RemoveFades when no event is a region.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-model/tests/cmd_regions.rs <<'VIZDAW_EOF'
//! Region commands for the editors (plan 5b Task 1, Rulings 5, 6, 9):
//! Normalize from a measured peak, fades to the cursor, Remove Fades, and
//! Relink rescaling regions when the new file's rate differs.

use core_model::ModelError;
use core_model::command::{
    AudioCmd, Command, EventCmd, FadeEdge, MediaCmd, SequenceCmd, apply,
};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, MediaId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn fade(n: u64, shape: FadeShape) -> Fade {
    Fade {
        length_frames: n,
        shape,
    }
}

/// Sequence 0 with lane 0; media 0 (`Media/a.caf`, 48 kHz, 192 000
/// frames); region 0 at tick 0 (offset 48 000, 96 000 frames, a 4800-frame
/// linear fade in and a 2400-frame equal-power fade out); a note (event 1)
/// at tick 0.
fn doc() -> Document {
    let mut d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap();
    d = run(
        &d,
        Command::Media(MediaCmd::Add {
            path: "Media/a.caf".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 192_000,
            content_hash: None,
            name: None,
            origin: None,
        }),
    )
    .unwrap();
    d = run(
        &d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(0),
            media: MediaId(0),
            source_offset: 48_000,
            length: 96_000,
            gain_db: 0.0,
            fade_in: fade(4800, FadeShape::Linear),
            fade_out: fade(2400, FadeShape::EqualPower),
        }),
    )
    .unwrap();
    run(
        &d,
        Command::Event(EventCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            events: vec![EventSpec::new(
                Tick(0),
                EventBody::note(60, 0x8000, Tick(100)),
            )],
        }),
    )
    .unwrap()
}

/// `(gain_db, fade_in, fade_out, source_offset, length)` of region 0.
fn region(d: &Document) -> (f32, Fade, Fade, u64, u64) {
    match d.event(EventId(0)).unwrap().0.body {
        EventBody::AudioRegion {
            gain_db,
            fade_in,
            fade_out,
            source_offset,
            length,
            ..
        } => (gain_db, fade_in, fade_out, source_offset, length),
        _ => panic!("event 0 is not a region"),
    }
}

/// The tick `seconds` into sequence 0, and the frame (48 kHz) it maps to.
fn at(d: &Document, seconds: f64) -> (Tick, u64) {
    let tempo = &d.sequence(SeqId(0)).unwrap().tempo;
    let t = tempo.seconds_to_tick(seconds);
    (t, (tempo.tick_to_seconds(t) * 48_000.0).round() as u64)
}

fn normalize(regions: Vec<(u64, f32)>, peak_db: f32) -> Command {
    Command::Audio(AudioCmd::Normalize {
        regions: regions.into_iter().map(|(e, p)| (EventId(e), p)).collect(),
        peak_db,
    })
}

#[test]
fn normalize_sets_gain_from_the_peak_and_is_one_undo_step() {
    let d = doc();
    let cmd = normalize(vec![(0, 0.25), (1, 0.5)], -1.0);
    assert_eq!(cmd.name(), "Normalize");
    let mut h = History::new(d.clone());
    h.execute(&cmd, &ViewFilter::default()).unwrap();
    // −1 − 20·log10(0.25) = 11.0412 dB; the note (event 1) is ignored.
    let (g, ..) = region(h.doc());
    assert!((g - 11.0412).abs() < 1e-3, "gain {g}");
    assert_eq!(h.undo_label(), Some("Normalize"));
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
}

#[test]
fn normalize_clamps_skips_silence_and_checks_its_target() {
    let d = doc();
    // A near-silent region would need +179 dB: limited to +24, with a note.
    let a = apply(&d, &normalize(vec![(0, 1e-9)], -1.0)).unwrap();
    assert_eq!(region(&a.doc).0, 24.0);
    assert!(
        a.report.warnings.iter().any(|w| w.contains("limited")),
        "{:?}",
        a.report.warnings
    );
    // Only silent (or non-finite) peaks: nothing to do.
    assert!(matches!(
        run(&d, normalize(vec![(0, 0.0)], -1.0)),
        Err(ModelError::Invalid { .. })
    ));
    assert!(matches!(
        run(&d, normalize(vec![(0, f32::NAN)], -1.0)),
        Err(ModelError::Invalid { .. })
    ));
    // Only a note, or nothing at all.
    assert!(run(&d, normalize(vec![(1, 0.5)], -1.0)).is_err());
    assert!(run(&d, normalize(vec![], -1.0)).is_err());
    // The target must be a finite level from −96 to 0 dBFS.
    for bad in [0.5, -96.5, f32::NAN, f32::INFINITY] {
        assert!(
            matches!(
                run(&d, normalize(vec![(0, 0.25)], bad)),
                Err(ModelError::OutOfRange { .. })
            ),
            "{bad}"
        );
    }
    // An unknown event is refused, not ignored.
    assert!(matches!(
        run(&d, normalize(vec![(99, 0.25)], -1.0)),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn fades_follow_the_cursor() {
    let d = doc();
    let fade_to = |at: Tick, edge: FadeEdge| {
        Command::Audio(AudioCmd::FadeTo {
            events: vec![EventId(0), EventId(1)],
            at,
            edge,
        })
    };
    // Fade In to Cursor at 0.5 s: the fade in ends there; the fade out
    // still fits and keeps its length and shape.
    let (t, f) = at(&d, 0.5);
    let c = fade_to(t, FadeEdge::In);
    assert_eq!(c.name(), "Fade In to Cursor");
    let (_, fi, fo, ..) = region(&run(&d, c).unwrap());
    assert_eq!((fi.length_frames, fi.shape), (f, FadeShape::Linear));
    assert_eq!((fo.length_frames, fo.shape), (2400, FadeShape::EqualPower));
    // Fade Out from Cursor at 1.5 s: the fade out starts there.
    let (t, f) = at(&d, 1.5);
    let c = fade_to(t, FadeEdge::Out);
    assert_eq!(c.name(), "Fade Out from Cursor");
    let (_, fi, fo, ..) = region(&run(&d, c).unwrap());
    assert_eq!((fi.length_frames, fo.length_frames), (4800, 96_000 - f));
    // A fade in to 1.99 s leaves no room for the whole fade out: it shrinks.
    let (t, f) = at(&d, 1.99);
    let (_, fi, fo, ..) = region(&run(&d, fade_to(t, FadeEdge::In)).unwrap());
    assert_eq!((fi.length_frames, fo.length_frames), (f, 96_000 - f));
    // The cursor at the region's start, or past its end: nothing to do.
    assert!(matches!(
        run(&d, fade_to(Tick(0), FadeEdge::In)),
        Err(ModelError::Invalid { .. })
    ));
    let (t, _) = at(&d, 3.0);
    assert!(run(&d, fade_to(t, FadeEdge::Out)).is_err());
}

#[test]
fn remove_fades_clears_both_and_ignores_other_events() {
    let d = doc();
    let c = Command::Audio(AudioCmd::RemoveFades {
        events: vec![EventId(1), EventId(0)],
    });
    assert_eq!(c.name(), "Remove Fades");
    let (_, fi, fo, ..) = region(&run(&d, c).unwrap());
    assert_eq!(fi, Fade::default());
    assert_eq!(fo, Fade::default());
    assert!(
        run(
            &d,
            Command::Audio(AudioCmd::RemoveFades {
                events: vec![EventId(1)]
            })
        )
        .is_err()
    );
}

#[test]
fn relink_rescales_regions_when_the_rate_differs() {
    let d = doc();
    let relink = |frames: u64, rate: u32| {
        Command::Media(MediaCmd::Relink {
            media: MediaId(0),
            path: "Media/a 44k.caf".into(),
            content_hash: Some("h".into()),
            length_frames: frames,
            channels: 2,
            sample_rate: rate,
        })
    };
    // 44.1 / 48 = 0.91875: 48 000 → 44 100, 96 000 → 88 200, fades
    // 4800 → 4410 and 2400 → 2205.
    let mut h = History::new(d.clone());
    h.execute(&relink(176_400, 44_100), &ViewFilter::default())
        .unwrap();
    let (_, fi, fo, off, len) = region(h.doc());
    assert_eq!(
        (off, len, fi.length_frames, fo.length_frames),
        (44_100, 88_200, 4410, 2205)
    );
    let m = &h.doc().media()[&MediaId(0)];
    assert_eq!(
        (m.sample_rate, m.length_frames, m.path.as_str()),
        (44_100, 176_400, "Media/a 44k.caf")
    );
    h.undo().unwrap();
    assert_eq!(h.doc(), &d);
    // The region ends at 144 000 frames = 132 300 at 44.1 kHz: one frame
    // of rounding is allowed (and clamped), two are refused.
    assert!(matches!(
        run(&d, relink(132_298, 44_100)),
        Err(ModelError::OutOfRange { .. })
    ));
    let (_, _, _, off, len) = region(&run(&d, relink(132_299, 44_100)).unwrap());
    assert_eq!((off, len), (44_100, 88_199));
    // At the same rate nothing is rescaled and nothing may overrun.
    assert!(run(&d, relink(143_999, 48_000)).is_err());
    let (_, fi, _, off, len) = region(&run(&d, relink(144_000, 48_000)).unwrap());
    assert_eq!((off, len, fi.length_frames), (48_000, 96_000, 4800));
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-model --test cmd_regions 2>&1 | tail -5`
Expected: compile errors (`FadeEdge`, `AudioCmd::Normalize`, `AudioCmd::FadeTo`, `AudioCmd::RemoveFades` not found).

- [ ] **Step 3: Add the region commands**

In `crates/core-model/src/command/audio.rs` (its imports already hold `Tick`, `EventId`, `Fade` and `ModelError`):

Find:
```rust
    /// Set fades.
    SetFades {
        /// The region.
        event: EventId,
        /// Fade in.
        fade_in: Fade,
        /// Fade out.
        fade_out: Fade,
    },
}
```
Replace with:
```rust
    /// Set fades.
    SetFades {
        /// The region.
        event: EventId,
        /// Fade in.
        fade_in: Fade,
        /// Fade out.
        fade_out: Fade,
    },
    /// Set each region's gain so its loudest sample reaches `peak_db`
    /// (spec §4.4; non-destructive). Each region comes with the linear
    /// peak of the frames it plays, measured from its overview (5b
    /// Ruling 5). Events that are not regions are ignored.
    Normalize {
        /// `(region, peak)`: the peak as a linear amplitude (> 0).
        regions: Vec<(EventId, f32)>,
        /// The target, −96…0 dBFS.
        peak_db: f32,
    },
    /// The fade in ends (`In`) or the fade out starts (`Out`) at `at`, in
    /// every region `at` lies strictly inside (5b Ruling 6).
    FadeTo {
        /// The events (non-regions are ignored).
        events: Vec<EventId>,
        /// The cursor.
        at: Tick,
        /// Which fade.
        edge: FadeEdge,
    },
    /// Removes both fades of every region.
    RemoveFades {
        /// The events (non-regions are ignored).
        events: Vec<EventId>,
    },
}

/// Which fade a [`AudioCmd::FadeTo`] sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeEdge {
    /// The fade in.
    In,
    /// The fade out.
    Out,
}
```

Find:
```rust
            AudioCmd::SetGain { .. } => "Set Region Gain",
            AudioCmd::SetFades { .. } => "Set Region Fades",
```
Replace with:
```rust
            AudioCmd::SetGain { .. } => "Set Region Gain",
            AudioCmd::SetFades { .. } => "Set Region Fades",
            AudioCmd::Normalize { .. } => "Normalize",
            AudioCmd::FadeTo {
                edge: FadeEdge::In,
                ..
            } => "Fade In to Cursor",
            AudioCmd::FadeTo {
                edge: FadeEdge::Out,
                ..
            } => "Fade Out from Cursor",
            AudioCmd::RemoveFades { .. } => "Remove Fades",
```

At the end of `apply` (the `SetFades` arm and the closing of the match):

Find:
```rust
            {
                *fi = *fade_in;
                *fo = *fade_out;
            }
            store(d, ev)?;
        }
    }
    Ok(())
}
```
Replace with:
```rust
            {
                *fi = *fade_in;
                *fo = *fade_out;
            }
            store(d, ev)?;
        }
        AudioCmd::Normalize { regions, peak_db } => {
            if !(peak_db.is_finite() && (-96.0..=0.0).contains(peak_db)) {
                return Err(ModelError::range("normalize.peak_db", peak_db));
            }
            let mut changed = 0usize;
            for (id, peak) in regions {
                if !is_region(d, *id)? {
                    continue;
                }
                if !(peak.is_finite() && *peak > 0.0) {
                    r.warnings.push(format!("{id} is silent: not normalized"));
                    continue;
                }
                let want = f64::from(*peak_db) - 20.0 * f64::from(*peak).log10();
                let g = want.clamp(-144.0, 24.0) as f32;
                if (want - f64::from(g)).abs() > 0.05 {
                    r.warnings
                        .push(format!("{id}: gain limited to {g:+.1} dB"));
                }
                let (mut ev, _, _) = region(d, *id)?;
                if let EventBody::AudioRegion { gain_db, .. } = &mut ev.body {
                    *gain_db = g;
                }
                store(d, ev)?;
                changed += 1;
            }
            if changed == 0 {
                return Err(ModelError::invalid(
                    "no selected audio region has sound to normalize",
                ));
            }
        }
        AudioCmd::FadeTo { events, at, edge } => {
            let mut changed = 0usize;
            for id in events {
                if !is_region(d, *id)? {
                    continue;
                }
                let (mut ev, seq, rate) = region(d, *id)?;
                let tempo = d.sequence(seq)?.tempo.clone();
                let secs = tempo.tick_to_seconds(*at) - tempo.tick_to_seconds(ev.tick);
                let f = (secs * rate).round();
                let EventBody::AudioRegion {
                    length,
                    fade_in,
                    fade_out,
                    ..
                } = &mut ev.body
                else {
                    continue;
                };
                // Strictly inside: a fade of 0 or of the whole region is
                // not "to the cursor".
                if !(f >= 1.0 && f < *length as f64) {
                    continue;
                }
                let f = f as u64;
                match edge {
                    FadeEdge::In => {
                        fade_in.length_frames = f;
                        fade_out.length_frames = fade_out.length_frames.min(*length - f);
                    }
                    FadeEdge::Out => {
                        fade_out.length_frames = *length - f;
                        fade_in.length_frames = fade_in.length_frames.min(f);
                    }
                }
                store(d, ev)?;
                changed += 1;
            }
            if changed == 0 {
                return Err(ModelError::invalid(
                    "the cursor is not inside a selected audio region",
                ));
            }
        }
        AudioCmd::RemoveFades { events } => {
            let mut regions = 0usize;
            for id in events {
                if !is_region(d, *id)? {
                    continue;
                }
                let (mut ev, _, _) = region(d, *id)?;
                if let EventBody::AudioRegion {
                    fade_in, fade_out, ..
                } = &mut ev.body
                {
                    *fade_in = Fade::default();
                    *fade_out = Fade::default();
                }
                store(d, ev)?;
                regions += 1;
            }
            if regions == 0 {
                return Err(ModelError::invalid(
                    "select audio regions to remove their fades",
                ));
            }
        }
    }
    Ok(())
}

/// Whether event `id` is an audio region (an unknown ID is an error).
fn is_region(d: &Document, id: EventId) -> Result<bool> {
    Ok(matches!(d.event(id)?.0.body, EventBody::AudioRegion { .. }))
}
```

In `crates/core-model/src/command/mod.rs`:

Find:
```rust
pub use audio::AudioCmd;
```
Replace with:
```rust
pub use audio::{AudioCmd, FadeEdge};
```

- [ ] **Step 4: Make Relink rescale on a rate change**

In `crates/core-model/src/command/media.rs`:

Find:
```rust
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
                *length = scale(*length).max(1).min(s.length_frames - *source_offset);
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
```
Replace with:
```rust
        let old_rate = item(d, s.media)?.sample_rate.max(1);
        let ratio = f64::from(rate) / f64::from(old_rate);
        if let Some(m) = d.media_mut().get_mut(&s.media) {
            m.path = s.path.clone();
            m.length_frames = s.length_frames;
            m.content_hash = s.content_hash.clone();
            m.sample_rate = rate;
        }
        rescale_regions(d, s.media, ratio, s.length_frames)?;
    }
    Ok(())
}

/// Rescales the regions of `media` by `ratio` (offset, length and fades
/// rounded to the nearest frame), keeps each length ≥ 1 and the region
/// inside a file of `len` frames, and clamps the fades (5a Ruling 1).
/// Shared by Convert, SetFormat and Relink (5b Ruling 9).
pub(crate) fn rescale_regions(d: &mut Document, media: MediaId, ratio: f64, len: u64) -> Result<()> {
    // Float-to-int `as` saturates; ratio 1 maps every value to itself.
    let scale = |x: u64| (x as f64 * ratio).round() as u64;
    for id in regions_using(d, media) {
        let mut ev = d.event(id)?.0.clone();
        if let EventBody::AudioRegion {
            source_offset,
            length,
            fade_in,
            fade_out,
            ..
        } = &mut ev.body
        {
            *source_offset = scale(*source_offset).min(len.saturating_sub(1));
            *length = scale(*length)
                .max(1)
                .min(len.saturating_sub(*source_offset));
            fade_in.length_frames = scale(fade_in.length_frames);
            fade_out.length_frames = scale(fade_out.length_frames);
        }
        clamp_fades(&mut ev);
        check_event_fields(d, &ev.spec())?;
        d.replace_event(ev)?;
    }
    Ok(())
}
```

Find:
```rust
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
```
Replace with:
```rust
            let old_rate = item(d, *media)?.sample_rate.max(1);
            if *sample_rate == 0 || *channels == 0 {
                return Err(ModelError::range("media.format", sample_rate));
            }
            check_media_path(path)?;
            // Another rate rescales the regions (5b Ruling 9), allowing
            // one frame of rounding past the new end; the same rate allows
            // none.
            let ratio = f64::from(*sample_rate) / f64::from(old_rate);
            let slack = if *sample_rate == old_rate { 0.0 } else { 1.0 };
            for id in regions_using(d, *media) {
                if let EventBody::AudioRegion {
                    source_offset,
                    length,
                    ..
                } = d.event(id)?.0.body
                {
                    let end = (source_offset.saturating_add(length) as f64 * ratio).round();
                    if end > *length_frames as f64 + slack {
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
            if *sample_rate != old_rate {
                rescale_regions(d, *media, ratio, *length_frames)?;
            }
```

Also update the `Relink` variant's doc comment:

Find:
```rust
    /// Point a media item at another file; refused while a region uses
    /// frames beyond the new file's end.
```
Replace with:
```rust
    /// Point a media item at another file; refused while a region uses
    /// frames beyond the new file's end. A file at another rate rescales
    /// the item's regions by the rate ratio (5b Ruling 9).
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_regions --test cmd_media 2>&1 | grep -E "^test result|FAILED|panicked"
```
Expected: both `test result: ok.` (`cmd_regions` 5 passed; `cmd_media` unchanged, its Convert and SetFormat tests now run through `rescale_regions`).

- [ ] **Step 6: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
```
Expected: no warnings; every `test result: ok`. A non-exhaustive `match` on `AudioCmd` outside `core-model` would fail to compile here; there is none in 5a's tree (`core-ffi/src/edit.rs` only builds `AudioCmd`s).

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/audio.rs crates/core-model/src/command/media.rs crates/core-model/src/command/mod.rs crates/core-model/tests/cmd_regions.rs
git commit -m "core-model: Normalize, fades to the cursor, Remove Fades; Relink rescales

AudioCmd::Normalize sets each region's gain from its measured peak
(clamped to -144..+24 dB, silent regions skipped), FadeTo ends the fade
in or starts the fade out at the cursor, RemoveFades clears both.
MediaCmd::Relink rescales the item's regions when the new file's rate
differs, sharing rescale_regions with Convert and SetFormat (parked 5a
Task 2 Minor).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 2: `core-media` — waveform overviews

**Files:**
- Create: `crates/core-media/src/overview.rs`
- Modify: `crates/core-media/src/lib.rs`, `crates/core-media/src/caf.rs` (a doc comment on `CafReader::read`)
- Test: `crates/core-media/src/overview.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `core_media::caf::{CafReader, CafWriter, WriteMode, hash_file}`, `MediaError::{Io, Unsupported, Cancelled}`.
- Produces (`core_media::overview`):
  - `DIR: &str = "Media/.overviews"`, `LEVELS: [u32; 3] = [256, 4096, 65_536]`, `MAX_STAMPS: usize = 8`, `HEADER_BYTES: usize = 284`;
  - `Stamp { len: u64, mtime_s: i64, mtime_ns: u32 }` with `Stamp::of(path) -> Result<Stamp, MediaError>`;
  - `Overview { pub channels: u16, pub frames: u64, pub scale: f32, pub hash: String, pub stamps: Vec<Stamp>, levels (private) }` with
    - `Overview::valid_hash(&str) -> bool` (1–64 lowercase hex characters),
    - `Overview::path(package: &Path, hash: &str) -> Option<PathBuf>` (`None` unless `valid_hash`),
    - `Overview::read(path) -> Result<Overview, MediaError>`, `Overview::write(&self, path) -> Result<(), MediaError>` (temp file + rename),
    - `Overview::with_stamp(self, Stamp) -> Overview`, `Overview::has_stamp(&self, &Stamp) -> bool`,
    - `Overview::range(&self, channel: u16, from: u64, to: u64) -> Option<(f32, f32)>`,
    - `Overview::peak(&self, from: u64, to: u64, exact: &mut dyn FnMut(u64, u64) -> Result<f32, MediaError>) -> Result<f32, MediaError>`;
  - `build(caf: &Path, cancel: &AtomicBool) -> Result<Overview, MediaError>` (one read; `hash` equals `caf::hash_file`'s);
  - `exact_peak(reader: &CafReader, from: u64, to: u64) -> Result<f32, MediaError>`;
  - `Bins { pub from: u64, pub to: u64, pub step: u64, pub channels: u16, values (private) }` with `Bins::read(reader: &CafReader, from: u64, to: u64, step: u64) -> Result<Bins, MediaError>` and `Bins::range(&self, channel: u16, from: u64, to: u64) -> Option<(f32, f32)>`.

Rulings 1, 2, 3 and 5. Everything here is platform-neutral and off the real-time path (it reads files).

- [ ] **Step 1: Document what `CafReader::read` leaves in `scratch`**

`build` hashes the raw bytes `read` already fetched, so the overview's hash is computed in the same pass. In `crates/core-media/src/caf.rs`:

Find:
```rust
    /// Reads frames from `start` into `out` (interleaved float, as many
    /// whole frames as fit); returns the frames read, fewer at the end or
    /// when the file is shorter than it was. `scratch` is reused.
```
Replace with:
```rust
    /// Reads frames from `start` into `out` (interleaved float, as many
    /// whole frames as fit); returns the frames read, fewer at the end or
    /// when the file is shorter than it was. `scratch` is reused; on
    /// return its first `frames read × frame_bytes()` bytes are those
    /// frames' raw sample bytes (overviews hash them, plan 5b Task 2).
```

- [ ] **Step 2: Write the module with its failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-media/src/overview.rs <<'VIZDAW_EOF'
//! Waveform overviews (sub-project 5 spec §10.1; plan 5b Rulings 1–3, 5).
//!
//! An overview holds min/max peaks of a media file at 256, 4096 and
//! 65 536 frames per peak, per channel, as `i16` scaled by the file's
//! largest absolute sample (at least full scale), the min rounded down and
//! the max rounded up so the envelope always contains the samples. It
//! lives in `Media/.overviews/<content hash>.vzov` with the `(length,
//! mtime)` stamps of the files it was built from.
//!
//! [`build`] reads a CAF once and hashes its sample bytes on the way, so a
//! build also tells whether the file still holds the audio a project
//! expects. Overviews are a cache, not media: they are written to a temp
//! file and renamed into place, and may be replaced or deleted. Nothing
//! here runs on (or near) the real-time thread.

use crate::MediaError;
use crate::caf::CafReader;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Where overviews live inside a package.
pub const DIR: &str = "Media/.overviews";
/// Frames per peak of each level, finest first.
pub const LEVELS: [u32; 3] = [256, 4096, 65_536];
/// Stamps an overview keeps (newest last).
pub const MAX_STAMPS: usize = 8;
const MAGIC: &[u8; 4] = b"VZOV";
const VERSION: u32 = 1;
const HASH_BYTES: usize = 64;
const STAMP_BYTES: usize = 24;
/// The fixed header: magic, version, channels, level count, scale,
/// frames, hash, stamp count, stamp slots.
pub const HEADER_BYTES: usize = 4 + 4 + 2 + 2 + 4 + 8 + HASH_BYTES + 4 + MAX_STAMPS * STAMP_BYTES;
/// Frames per peak of the finest level.
const L0: u64 = 256;
/// Each level is this many peaks of the one below.
const STEP: usize = 16;
const CHUNK_FRAMES: usize = 65_536;
const FULL: f64 = 32_767.0;
/// More finest-level peaks than this is a damaged header (24 hours at
/// 768 kHz is about 2^28).
const MAX_PEAKS: u64 = 1 << 28;
/// The most bins one [`Bins::read`] makes.
const MAX_BINS: u64 = 1 << 22;

/// A media file's identity on disk: its length and modification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// File length in bytes.
    pub len: u64,
    /// Modification time, seconds since the epoch.
    pub mtime_s: i64,
    /// Modification time, nanoseconds within the second.
    pub mtime_ns: u32,
}

impl Stamp {
    /// The stamp of the file at `path`.
    pub fn of(path: &Path) -> Result<Stamp, MediaError> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(path)?;
        Ok(Stamp {
            len: m.len(),
            mtime_s: m.mtime(),
            mtime_ns: u32::try_from(m.mtime_nsec()).unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Level {
    frames_per_peak: u32,
    /// Per peak, per channel: min, then max.
    peaks: Vec<i16>,
}

/// One media file's overview.
#[derive(Debug, Clone, PartialEq)]
pub struct Overview {
    /// 1 or 2.
    pub channels: u16,
    /// Frames the file held when built.
    pub frames: u64,
    /// What full scale (32 767) stands for: the file's largest absolute
    /// sample, at least 1.
    pub scale: f32,
    /// BLAKE3 hex of the file's sample bytes.
    pub hash: String,
    /// Files this overview was built from or checked against.
    pub stamps: Vec<Stamp>,
    levels: Vec<Level>,
}

fn quantize(v: f32, scale: f32, up: bool) -> i16 {
    if !v.is_finite() {
        return 0;
    }
    let x = f64::from(v) / f64::from(scale) * FULL;
    let x = if up { x.ceil() } else { x.floor() };
    x.clamp(-32_768.0, 32_767.0) as i16
}

fn decode(q: i16, scale: f32) -> f32 {
    (f64::from(q) / FULL * f64::from(scale)) as f32
}

/// Min and max per peak of `lo`/`hi` (per peak, per channel), `by` peaks
/// to one.
fn coarser(lo: &[f32], hi: &[f32], ch: usize, by: usize) -> (Vec<f32>, Vec<f32>) {
    let n = lo.len() / ch.max(1);
    let m = n.div_ceil(by);
    let mut l = vec![f32::INFINITY; m * ch];
    let mut h = vec![f32::NEG_INFINITY; m * ch];
    for p in 0..n {
        let q = p / by;
        for c in 0..ch {
            if let (Some(a), Some(x)) = (lo.get(p * ch + c), l.get_mut(q * ch + c)) {
                *x = x.min(*a);
            }
            if let (Some(a), Some(x)) = (hi.get(p * ch + c), h.get_mut(q * ch + c)) {
                *x = x.max(*a);
            }
        }
    }
    (l, h)
}

/// Builds the overview of the VizDAW CAF at `caf` in one read, hashing its
/// sample bytes on the way (so `hash` equals [`crate::caf::hash_file`]'s).
/// A file shorter than its header says gives the overview of what is
/// there. `cancel` stops it with `Cancelled`. No stamps are set.
pub fn build(caf: &Path, cancel: &AtomicBool) -> Result<Overview, MediaError> {
    let r = CafReader::open(caf)?;
    let info = r.info().clone();
    let ch = usize::from(info.channels);
    let fb = info.frame_bytes();
    let peaks0 = info.frames.div_ceil(L0);
    if peaks0 > MAX_PEAKS {
        return Err(MediaError::Unsupported(format!(
            "{}: too long for an overview",
            caf.display()
        )));
    }
    let n0 = peaks0 as usize;
    let mut lo = vec![f32::INFINITY; n0 * ch];
    let mut hi = vec![f32::NEG_INFINITY; n0 * ch];
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0.0f32; CHUNK_FRAMES * ch];
    let mut scratch = Vec::new();
    let mut at = 0u64;
    let mut peak = 0.0f32;
    while at < info.frames {
        if cancel.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        let n = r.read(at, &mut buf, &mut scratch)?;
        if n == 0 {
            break;
        }
        hasher.update(scratch.get(..n * fb).unwrap_or(&[]));
        for (k, frame) in buf.chunks_exact(ch).take(n).enumerate() {
            let p = ((at + k as u64) / L0) as usize;
            for (c, v) in frame.iter().enumerate() {
                let v = if v.is_finite() { *v } else { 0.0 };
                peak = peak.max(v.abs());
                if let Some(x) = lo.get_mut(p * ch + c) {
                    *x = x.min(v);
                }
                if let Some(x) = hi.get_mut(p * ch + c) {
                    *x = x.max(v);
                }
            }
        }
        at += n as u64;
    }
    let used = at.div_ceil(L0) as usize * ch;
    lo.truncate(used);
    hi.truncate(used);
    let scale = peak.max(1.0);
    let mut levels = Vec::with_capacity(LEVELS.len());
    for (i, fpp) in LEVELS.iter().enumerate() {
        if i > 0 {
            (lo, hi) = coarser(&lo, &hi, ch, STEP);
        }
        let peaks = lo
            .iter()
            .zip(&hi)
            .flat_map(|(l, h)| [quantize(*l, scale, false), quantize(*h, scale, true)])
            .collect();
        levels.push(Level {
            frames_per_peak: *fpp,
            peaks,
        });
    }
    Ok(Overview {
        channels: info.channels,
        frames: at,
        scale,
        hash: hasher.finalize().to_hex().to_string(),
        stamps: Vec::new(),
        levels,
    })
}

/// The largest absolute sample of `reader` in frames `[from, to)`.
pub fn exact_peak(reader: &CafReader, from: u64, to: u64) -> Result<f32, MediaError> {
    let ch = usize::from(reader.info().channels);
    let to = to.min(reader.info().frames);
    let mut buf = vec![0.0f32; CHUNK_FRAMES * ch];
    let mut scratch = Vec::new();
    let mut at = from;
    let mut p = 0.0f32;
    while at < to {
        let want = ((to - at) as usize).min(CHUNK_FRAMES);
        let slice = buf.get_mut(..want * ch).unwrap_or(&mut []);
        let n = reader.read(at, slice, &mut scratch)?;
        if n == 0 {
            break;
        }
        for v in slice.iter().take(n * ch) {
            if v.is_finite() {
                p = p.max(v.abs());
            }
        }
        at += n as u64;
    }
    Ok(p)
}

/// Exact sample min/max in `step`-frame bins over `[from, to)`: what a
/// view zoomed in below 256 frames per column draws (Ruling 3).
#[derive(Debug, Clone, PartialEq)]
pub struct Bins {
    /// First frame.
    pub from: u64,
    /// End frame (exclusive; cut to the file).
    pub to: u64,
    /// Frames per bin (≥ 1).
    pub step: u64,
    /// Channels.
    pub channels: u16,
    /// Per bin, per channel: min, then max.
    values: Vec<f32>,
}

impl Bins {
    /// Reads `[from, to)` of `reader` into `step`-frame bins.
    pub fn read(reader: &CafReader, from: u64, to: u64, step: u64) -> Result<Bins, MediaError> {
        let info = reader.info();
        let step = step.max(1);
        let to = to.min(info.frames);
        let ch = usize::from(info.channels);
        if from >= to {
            return Ok(Bins {
                from,
                to: from,
                step,
                channels: info.channels,
                values: Vec::new(),
            });
        }
        let bins = (to - from).div_ceil(step);
        if bins > MAX_BINS {
            return Err(MediaError::Unsupported(format!(
                "{bins} bins is too many for one read"
            )));
        }
        let mut values = Vec::with_capacity(bins as usize * ch * 2);
        for _ in 0..bins as usize * ch {
            values.push(f32::INFINITY);
            values.push(f32::NEG_INFINITY);
        }
        let mut buf = vec![0.0f32; CHUNK_FRAMES * ch];
        let mut scratch = Vec::new();
        let mut at = from;
        while at < to {
            let want = ((to - at) as usize).min(CHUNK_FRAMES);
            let slice = buf.get_mut(..want * ch).unwrap_or(&mut []);
            let n = reader.read(at, slice, &mut scratch)?;
            if n == 0 {
                break;
            }
            for (k, frame) in slice.chunks_exact(ch).take(n).enumerate() {
                let b = ((at + k as u64 - from) / step) as usize;
                for (c, v) in frame.iter().enumerate() {
                    let v = if v.is_finite() { *v } else { 0.0 };
                    let i = (b * ch + c) * 2;
                    if let Some(x) = values.get_mut(i) {
                        *x = x.min(v);
                    }
                    if let Some(x) = values.get_mut(i + 1) {
                        *x = x.max(v);
                    }
                }
            }
            at += n as u64;
        }
        for v in &mut values {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
        Ok(Bins {
            from,
            to: at.min(to),
            step,
            channels: info.channels,
            values,
        })
    }

    /// Min and max of `channel` over frames `[from, to)` (whole bins
    /// that overlap it); `None` outside the bins.
    pub fn range(&self, channel: u16, from: u64, to: u64) -> Option<(f32, f32)> {
        let (from, to) = (from.max(self.from), to.min(self.to));
        if from >= to || channel >= self.channels {
            return None;
        }
        let ch = usize::from(self.channels);
        let c = usize::from(channel);
        let (b0, b1) = (
            (from - self.from) / self.step,
            (to - self.from).div_ceil(self.step),
        );
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for b in b0..b1 {
            let i = (b as usize * ch + c) * 2;
            if let (Some(a), Some(z)) = (self.values.get(i), self.values.get(i + 1)) {
                lo = lo.min(*a);
                hi = hi.max(*z);
            }
        }
        (lo <= hi).then_some((lo, hi))
    }
}

/// A little-endian reader over a byte slice.
struct Cur<'a> {
    b: &'a [u8],
    at: usize,
}

impl Cur<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.b.get(self.at..self.at.checked_add(N)?)?;
        self.at += N;
        s.try_into().ok()
    }
    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take().map(u64::from_le_bytes)
    }
    fn i64(&mut self) -> Option<i64> {
        self.take().map(i64::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }
}

/// What the header holds.
struct Header {
    channels: u16,
    frames: u64,
    scale: f32,
    hash: String,
    stamps: Vec<Stamp>,
}

fn parse_header(b: &[u8]) -> Result<Header, &'static str> {
    let mut c = Cur { b, at: 0 };
    if c.take::<4>().as_ref() != Some(MAGIC) {
        return Err("not an overview");
    }
    if c.u32() != Some(VERSION) {
        return Err("another version");
    }
    let channels = c.u16().ok_or("short")?;
    if !(1..=2).contains(&channels) {
        return Err("channels");
    }
    if c.u16() != Some(LEVELS.len() as u16) {
        return Err("levels");
    }
    let scale = c.f32().ok_or("short")?;
    if !(scale.is_finite() && scale >= 1.0) {
        return Err("scale");
    }
    let frames = c.u64().ok_or("short")?;
    if frames.div_ceil(L0) > MAX_PEAKS {
        return Err("frames");
    }
    let raw: [u8; HASH_BYTES] = c.take().ok_or("short")?;
    let end = raw.iter().position(|b| *b == 0).unwrap_or(HASH_BYTES);
    let hash = std::str::from_utf8(raw.get(..end).unwrap_or(&[]))
        .map_err(|_| "hash")?
        .to_string();
    if !Overview::valid_hash(&hash) {
        return Err("hash");
    }
    let n = c.u32().ok_or("short")? as usize;
    if n > MAX_STAMPS {
        return Err("stamps");
    }
    let mut stamps = Vec::with_capacity(n);
    for i in 0..MAX_STAMPS {
        let (len, s, ns, _) = (
            c.u64().ok_or("short")?,
            c.i64().ok_or("short")?,
            c.u32().ok_or("short")?,
            c.u32().ok_or("short")?,
        );
        if i < n {
            stamps.push(Stamp {
                len,
                mtime_s: s,
                mtime_ns: ns,
            });
        }
    }
    Ok(Header {
        channels,
        frames,
        scale,
        hash,
        stamps,
    })
}

/// The body size a header implies (`None` on overflow).
fn body_bytes(channels: u16, frames: u64) -> Option<u64> {
    let mut total = 0u64;
    for fpp in LEVELS {
        let peaks = frames.div_ceil(u64::from(fpp));
        total = total
            .checked_add(12)?
            .checked_add(peaks.checked_mul(u64::from(channels))?.checked_mul(4)?)?;
    }
    Some(total)
}

impl Overview {
    /// Whether `hash` may name an overview file: 1–64 lowercase hex
    /// characters (a project's hash never becomes a path otherwise).
    pub fn valid_hash(hash: &str) -> bool {
        !hash.is_empty()
            && hash.len() <= HASH_BYTES
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    /// The overview file for `hash` in `package`; `None` for a hash that
    /// is not [`Overview::valid_hash`].
    pub fn path(package: &Path, hash: &str) -> Option<PathBuf> {
        Self::valid_hash(hash).then(|| package.join(DIR).join(format!("{hash}.vzov")))
    }

    /// Adds `s` (newest last), keeping at most [`MAX_STAMPS`].
    pub fn with_stamp(mut self, s: Stamp) -> Overview {
        self.stamps.retain(|x| *x != s);
        self.stamps.push(s);
        while self.stamps.len() > MAX_STAMPS {
            self.stamps.remove(0);
        }
        self
    }

    /// Whether a file with stamp `s` is known to hold this audio.
    pub fn has_stamp(&self, s: &Stamp) -> bool {
        self.stamps.contains(s)
    }

    /// Min and max of `channel` over frames `[from, to)`, from the
    /// coarsest level with at least four peaks in the range (else the
    /// finest); the peaks at both ends are taken whole, so the result
    /// contains every sample of the range. `None` when the range is empty.
    pub fn range(&self, channel: u16, from: u64, to: u64) -> Option<(f32, f32)> {
        let to = to.min(self.frames);
        if from >= to || channel >= self.channels {
            return None;
        }
        let span = to - from;
        let level = self
            .levels
            .iter()
            .rev()
            .find(|l| u64::from(l.frames_per_peak) * 4 <= span)
            .or_else(|| self.levels.first())?;
        let fpp = u64::from(level.frames_per_peak).max(1);
        let ch = usize::from(self.channels);
        let c = usize::from(channel);
        let (mut lo, mut hi) = (i16::MAX, i16::MIN);
        for p in from / fpp..to.div_ceil(fpp) {
            let at = (p as usize * ch + c) * 2;
            if let (Some(a), Some(b)) = (level.peaks.get(at), level.peaks.get(at + 1)) {
                lo = lo.min(*a);
                hi = hi.max(*b);
            }
        }
        (lo <= hi).then(|| (decode(lo, self.scale), decode(hi, self.scale)))
    }

    /// The largest absolute sample over frames `[from, to)`: whole
    /// 256-frame blocks from the finest level, the partial blocks at both
    /// ends from `exact(from, to)` (samples outside the range never count;
    /// Ruling 5).
    pub fn peak(
        &self,
        from: u64,
        to: u64,
        exact: &mut dyn FnMut(u64, u64) -> Result<f32, MediaError>,
    ) -> Result<f32, MediaError> {
        let to = to.min(self.frames);
        if from >= to {
            return Ok(0.0);
        }
        let a = from.div_ceil(L0) * L0;
        let b = to / L0 * L0;
        if a >= b {
            return exact(from, to);
        }
        let mut p = 0.0f32;
        if from < a {
            p = p.max(exact(from, a)?);
        }
        if b < to {
            p = p.max(exact(b, to)?);
        }
        if let Some(level) = self.levels.first() {
            let ch = usize::from(self.channels);
            let (i0, i1) = ((a / L0) as usize * ch * 2, (b / L0) as usize * ch * 2);
            for v in level.peaks.get(i0..i1).unwrap_or(&[]) {
                p = p.max(decode(*v, self.scale).abs());
            }
        }
        Ok(p)
    }

    /// Writes the overview to `path` through a temp file beside it and a
    /// rename (an overview is a cache: replacing one is fine).
    pub fn write(&self, path: &Path) -> Result<(), MediaError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = path
            .parent()
            .ok_or_else(|| MediaError::Io(format!("{}: no folder", path.display())))?;
        std::fs::create_dir_all(dir)?;
        let ch = usize::from(self.channels).max(1);
        let mut out = Vec::with_capacity(
            HEADER_BYTES + self.levels.iter().map(|l| 12 + 2 * l.peaks.len()).sum::<usize>(),
        );
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&(self.levels.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.scale.to_le_bytes());
        out.extend_from_slice(&self.frames.to_le_bytes());
        let mut h = [0u8; HASH_BYTES];
        for (d, s) in h.iter_mut().zip(self.hash.bytes()) {
            *d = s;
        }
        out.extend_from_slice(&h);
        let n = self.stamps.len().min(MAX_STAMPS);
        out.extend_from_slice(&(n as u32).to_le_bytes());
        for i in 0..MAX_STAMPS {
            let s = self.stamps.get(i).copied().unwrap_or(Stamp {
                len: 0,
                mtime_s: 0,
                mtime_ns: 0,
            });
            out.extend_from_slice(&s.len.to_le_bytes());
            out.extend_from_slice(&s.mtime_s.to_le_bytes());
            out.extend_from_slice(&s.mtime_ns.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        for l in &self.levels {
            out.extend_from_slice(&l.frames_per_peak.to_le_bytes());
            out.extend_from_slice(&((l.peaks.len() / (2 * ch)) as u64).to_le_bytes());
            for v in &l.peaks {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = dir.join(format!(
            ".{name}.{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let r = std::fs::write(&tmp, &out).and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = r {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }

    /// Reads and checks the overview at `path`: its size must be exactly
    /// what its header implies, every level as [`LEVELS`] says; anything
    /// else is `Unsupported` ("damaged overview"), never a panic.
    pub fn read(path: &Path) -> Result<Overview, MediaError> {
        let bad = |why: &str| {
            MediaError::Unsupported(format!("{}: damaged overview ({why})", path.display()))
        };
        let mut f = std::fs::File::open(path)?;
        let len = f.metadata()?.len();
        let mut head = [0u8; HEADER_BYTES];
        f.read_exact(&mut head).map_err(|_| bad("short"))?;
        let h = parse_header(&head).map_err(bad)?;
        let body = body_bytes(h.channels, h.frames).ok_or_else(|| bad("size"))?;
        if len != HEADER_BYTES as u64 + body {
            return Err(bad("size"));
        }
        let mut rest = Vec::with_capacity(body as usize);
        f.read_to_end(&mut rest)?;
        let mut c = Cur { b: &rest, at: 0 };
        let ch = usize::from(h.channels);
        let mut levels = Vec::with_capacity(LEVELS.len());
        for fpp in LEVELS {
            if c.u32() != Some(fpp) {
                return Err(bad("level"));
            }
            let count = c.u64().ok_or_else(|| bad("short"))?;
            if count != h.frames.div_ceil(u64::from(fpp)) {
                return Err(bad("peak count"));
            }
            let n = count as usize * ch * 2;
            let mut peaks = Vec::with_capacity(n);
            for _ in 0..n {
                peaks.push(i16::from_le_bytes(c.take().ok_or_else(|| bad("short"))?));
            }
            levels.push(Level {
                frames_per_peak: fpp,
                peaks,
            });
        }
        Ok(Overview {
            channels: h.channels,
            frames: h.frames,
            scale: h.scale,
            hash: h.hash,
            stamps: h.stamps,
            levels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caf::{CafWriter, WriteMode, hash_file};
    use core_model::format::BitDepth;

    /// A stereo float CAF of `frames` frames: channel 0 a slow ramp,
    /// channel 1 a faster one at half level, with `spikes` set.
    fn file(dir: &Path, name: &str, frames: usize, spikes: &[(usize, usize, f32)]) -> (PathBuf, Vec<f32>) {
        let mut s = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            s.push(((f % 1000) as f32 / 1000.0 - 0.5) * 0.8);
            s.push(((f % 77) as f32 / 77.0 - 0.5) * 0.5);
        }
        for (f, c, v) in spikes {
            s[f * 2 + c] = *v;
        }
        let p = dir.join(name);
        let mut w = CafWriter::create(&p, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
        w.write(&s).unwrap();
        w.finish().unwrap();
        (p, s)
    }

    fn brute(s: &[f32], c: usize, from: usize, to: usize) -> (f32, f32) {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for f in from..to {
            lo = lo.min(s[f * 2 + c]);
            hi = hi.max(s[f * 2 + c]);
        }
        (lo, hi)
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn levels_match_a_brute_force_min_max() {
        let dir = tempfile::tempdir().unwrap();
        // 70 001 frames: every level ends in a partial peak.
        let (p, s) = file(dir.path(), "a.caf", 70_001, &[]);
        let o = build(&p, &no_cancel()).unwrap();
        assert_eq!((o.channels, o.frames, o.scale), (2, 70_001, 1.0));
        assert_eq!(o.hash, hash_file(&p).unwrap());
        assert_eq!(o.levels.len(), 3);
        for l in &o.levels {
            let fpp = l.frames_per_peak as usize;
            let n = 70_001usize.div_ceil(fpp);
            assert_eq!(l.peaks.len(), n * 2 * 2, "level {fpp}");
            for p in 0..n {
                for c in 0..2 {
                    let (lo, hi) = brute(&s, c, p * fpp, ((p + 1) * fpp).min(70_001));
                    let at = (p * 2 + c) * 2;
                    assert_eq!(l.peaks[at], quantize(lo, 1.0, false), "level {fpp} peak {p}");
                    assert_eq!(l.peaks[at + 1], quantize(hi, 1.0, true), "level {fpp} peak {p}");
                }
            }
        }
    }

    #[test]
    fn float_overs_scale_the_peaks() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _) = file(dir.path(), "loud.caf", 5000, &[(1234, 1, 2.0), (4000, 0, -1.5)]);
        let o = build(&p, &no_cancel()).unwrap();
        assert_eq!(o.scale, 2.0);
        let (lo, hi) = o.range(1, 1200, 1300).unwrap();
        assert!(hi >= 2.0 && hi - 2.0 < 1e-4, "{hi}");
        assert!(lo <= -0.25);
        let (lo, _) = o.range(0, 3900, 4100).unwrap();
        assert!(lo <= -1.5 && lo > -1.5 - 1e-4, "{lo}");
    }

    #[test]
    fn write_then_read_is_identical_and_stamps_keep_the_newest_eight() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _) = file(dir.path(), "b.caf", 9000, &[]);
        let mut o = build(&p, &no_cancel()).unwrap();
        for i in 0..10 {
            o = o.with_stamp(Stamp {
                len: i,
                mtime_s: i as i64,
                mtime_ns: 0,
            });
        }
        assert_eq!(o.stamps.len(), MAX_STAMPS);
        assert_eq!(o.stamps.first().unwrap().len, 2, "the oldest went");
        let at = Overview::path(dir.path(), &o.hash).unwrap();
        o.write(&at).unwrap();
        assert_eq!(Overview::read(&at).unwrap(), o);
        // Rewriting replaces it; no temp file is left.
        o.write(&at).unwrap();
        let left: Vec<_> = std::fs::read_dir(at.parent().unwrap()).unwrap().collect();
        assert_eq!(left.len(), 1);
        let st = Stamp::of(&p).unwrap();
        assert!(!o.has_stamp(&st));
        assert!(o.clone().with_stamp(st).has_stamp(&st));
    }

    /// Review Focus 3: a damaged or foreign overview is refused, never
    /// trusted and never a panic.
    #[test]
    fn damaged_overviews_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _) = file(dir.path(), "c.caf", 3000, &[]);
        let o = build(&p, &no_cancel()).unwrap();
        let good = dir.path().join("good.vzov");
        o.write(&good).unwrap();
        let bytes = std::fs::read(&good).unwrap();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", vec![]),
            ("truncated", bytes[..bytes.len() - 1].to_vec()),
            ("extended", [bytes.clone(), vec![0]].concat()),
            ("header only", bytes[..HEADER_BYTES].to_vec()),
            ("magic", {
                let mut b = bytes.clone();
                b[0] = b'X';
                b
            }),
            ("version", {
                let mut b = bytes.clone();
                b[4] = 2;
                b
            }),
            ("channels", {
                let mut b = bytes.clone();
                b[8] = 3;
                b
            }),
            ("frames", {
                // 3000 → 7096 frames: the peak counts no longer fit the size.
                let mut b = bytes.clone();
                b[17] ^= 0x10;
                b
            }),
            ("hash with a slash", {
                let mut b = bytes.clone();
                b[24] = b'/';
                b
            }),
            ("uppercase hash", {
                let mut b = bytes.clone();
                b[24] = b'A';
                b
            }),
            ("stamp count", {
                let mut b = bytes.clone();
                b[88] = 9;
                b
            }),
            ("level", {
                let mut b = bytes.clone();
                b[HEADER_BYTES] = 1;
                b
            }),
        ];
        for (why, b) in cases {
            let at = dir.path().join(format!("{why}.vzov"));
            std::fs::write(&at, &b).unwrap();
            assert!(Overview::read(&at).is_err(), "{why} was accepted");
        }
        // Random bytes of random lengths never panic.
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for i in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let len = (x % 700) as usize;
            let mut b: Vec<u8> = (0..len).map(|k| (x >> (k % 56)) as u8).collect();
            if i % 2 == 0 && b.len() >= 8 {
                b[..8].copy_from_slice(&bytes[..8]);
            }
            let at = dir.path().join("random.vzov");
            std::fs::write(&at, &b).unwrap();
            let _ = Overview::read(&at);
        }
    }

    #[test]
    fn hashes_name_files_only_when_lowercase_hex() {
        let pkg = Path::new("/p.vizdaw");
        assert_eq!(
            Overview::path(pkg, "0af9"),
            Some(PathBuf::from("/p.vizdaw/Media/.overviews/0af9.vzov"))
        );
        let long = "a".repeat(65);
        for bad in ["", "../../evil", "AB12", "12 3", long.as_str(), "x.vzov"] {
            assert_eq!(Overview::path(pkg, bad), None, "{bad:?}");
        }
    }

    #[test]
    fn ranges_contain_their_samples_and_bins_are_exact() {
        let dir = tempfile::tempdir().unwrap();
        let (p, s) = file(dir.path(), "d.caf", 300_000, &[]);
        let o = build(&p, &no_cancel()).unwrap();
        let r = CafReader::open(&p).unwrap();
        let mut x: u64 = 7;
        for _ in 0..100 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let from = (x >> 33) % 299_000;
            let len = 1 + (x >> 7) % 100_000;
            let to = (from + len).min(300_000);
            for c in 0..2u16 {
                let (blo, bhi) = brute(&s, c as usize, from as usize, to as usize);
                let (lo, hi) = o.range(c, from, to).unwrap();
                assert!(lo <= blo + 1e-6 && hi >= bhi - 1e-6, "{from}..{to}");
            }
            let step = 1 + (x >> 40) % 300;
            let short_to = (from + 600).min(300_000);
            let b = Bins::read(&r, from, short_to, step).unwrap();
            for k in 0..2u16 {
                let mut f = from;
                while f < short_to {
                    let e = (f + step).min(short_to);
                    assert_eq!(b.range(k, f, e), Some(brute(&s, k as usize, f as usize, e as usize)));
                    f = e;
                }
            }
        }
        assert_eq!(o.range(0, 300_000, 300_100), None);
        assert_eq!(o.range(2, 0, 10), None);
    }

    #[test]
    fn the_peak_counts_only_its_span() {
        let dir = tempfile::tempdir().unwrap();
        // A loud spike at frame 50 lies outside [100, 1100); inside, the
        // loudest sample is 0.6 at frame 600.
        let (p, _) = file(dir.path(), "e.caf", 5000, &[(50, 0, 0.95), (600, 1, -0.6)]);
        let o = build(&p, &no_cancel()).unwrap();
        let r = CafReader::open(&p).unwrap();
        let mut exact = |a: u64, b: u64| exact_peak(&r, a, b);
        let got = o.peak(100, 1100, &mut exact).unwrap();
        assert!(got >= 0.6 && got - 0.6 < 1e-4, "{got}");
        // A span inside one block is read exactly.
        assert_eq!(o.peak(590, 610, &mut exact).unwrap(), 0.6);
        assert_eq!(o.peak(10, 10, &mut exact).unwrap(), 0.0);
        assert!(o.peak(0, 5000, &mut exact).unwrap() >= 0.95);
    }

    #[test]
    fn a_build_stops_on_cancel_and_survives_a_cut_file() {
        let dir = tempfile::tempdir().unwrap();
        let (p, _) = file(dir.path(), "f.caf", 100_000, &[]);
        assert_eq!(
            build(&p, &AtomicBool::new(true)).unwrap_err(),
            MediaError::Cancelled
        );
        // Cut the file mid-data: the overview covers what is there.
        let len = std::fs::metadata(&p).unwrap().len();
        let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
        f.set_len(len - 40_003).unwrap();
        let o = build(&p, &no_cancel()).unwrap();
        assert!(o.frames < 100_000 && o.frames > 90_000, "{}", o.frames);
        assert_eq!(o.hash, hash_file(&p).unwrap());
    }
}
VIZDAW_EOF
```

In `crates/core-media/src/lib.rs`:

Find:
```rust
pub mod caf;
mod error;
pub mod import;
pub mod stream;
```
Replace with:
```rust
pub mod caf;
mod error;
pub mod import;
pub mod overview;
pub mod stream;
```

The tests use `s[f * 2 + c]` and other indexing: test code is outside the `cfg_attr(not(test), deny(...))` lints.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p core-media overview 2>&1 | grep -E "^test |test result"`
Expected: 8 tests pass (`levels_match_a_brute_force_min_max`, `float_overs_scale_the_peaks`, `write_then_read_is_identical_and_stamps_keep_the_newest_eight`, `damaged_overviews_are_refused`, `hashes_name_files_only_when_lowercase_hex`, `ranges_contain_their_samples_and_bins_are_exact`, `the_peak_counts_only_its_span`, `a_build_stops_on_cancel_and_survives_a_cut_file`).

If `damaged_overviews_are_refused`'s byte offsets do not hit the fields named (the header layout is 4 magic, 4 version, 2 channels, 2 levels, 4 scale, 8 frames at offset 16, 64 hash at offset 24, 4 stamp count at offset 88; the frames flip must change a level's peak count), fix the offsets in the test, not the format.

- [ ] **Step 4: Run every gate (with rt-check: `core-media` changed)**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph 2>&1 | tail -3
```
Expected: no warnings, every result ok.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-media/src/overview.rs crates/core-media/src/lib.rs crates/core-media/src/caf.rs
git commit -m "core-media: waveform overviews (.vzov), exact bins and peaks

Overviews hold i16 min/max peaks at 256, 4096 and 65536 frames per
peak, scaled by the file's peak, built in one read that also hashes the
samples, read back only when every size and field checks out, and
queried by frame range. Bins give exact sample min/max for zoomed-in
views; peak() counts whole blocks from the overview and the edges from
the samples (Normalize).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 3: `core-media` — imports build overviews and never take reserved names

**Files:**
- Modify: `crates/core-media/src/import.rs`
- Test: `crates/core-media/src/import.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: Task 2's `overview::{build, Overview, Stamp}`.
- Produces:
  - `core_media::import::import_reserved(importer: &dyn MediaImporter, source: &Path, package: &Path, format: AudioFormat, stem: &str, reserved: &BTreeSet<String>, progress: &mut dyn FnMut(f32), cancel: &AtomicBool) -> Result<Vec<Imported>, MediaError>` — `import_as` with names reserved: never names a file `Media/<name>` when that package-relative path is in `reserved` (Ruling 12);
  - `import_as` and `import_file` keep their signatures (an empty reservation);
  - every import writes `Media/.overviews/<content hash>.vzov` for each file it makes, stamped with that file (spec §10.1 "It is built during import").

The overview is built from the incoming copy before it is linked into `Media/`: a hard link (or the exclusive rename that replaces it) keeps the inode and modification time, so the stamp holds for the linked file. A failed build is not fatal (the session builds it later, Task 4); a cancel during the build cancels the import and leaves nothing.

- [ ] **Step 1: Write the failing tests**

In `crates/core-media/src/import.rs`, inside `mod tests`:

Find:
```rust
    #[test]
    fn the_fallback_walks_the_free_name_loop() {
```
Replace with:
```rust
    #[test]
    fn imports_build_their_overviews() {
        let d = package();
        let fake = Fake {
            channels: 2,
            frames: 3000,
            cancel_after: None,
        };
        let got = import(&fake, Path::new("tone.wav"), d.path(), BitDepth::Int24).unwrap();
        let i = &got[0];
        let at = crate::overview::Overview::path(d.path(), &i.content_hash).unwrap();
        let o = crate::overview::Overview::read(&at).unwrap();
        assert_eq!(
            (o.hash.as_str(), o.frames, o.channels),
            (i.content_hash.as_str(), 3000, 2)
        );
        let file = d.path().join(&i.path);
        let stamp = crate::overview::Stamp::of(&file).unwrap();
        assert!(o.has_stamp(&stamp), "the stamp holds for the linked file");
        // A wide file gets one overview per mono file.
        let six = Fake {
            channels: 6,
            frames: 700,
            cancel_after: None,
        };
        for i in import(&six, Path::new("surround.wav"), d.path(), BitDepth::Int16).unwrap() {
            let at = crate::overview::Overview::path(d.path(), &i.content_hash).unwrap();
            assert_eq!(crate::overview::Overview::read(&at).unwrap().channels, 1);
        }
    }

    #[test]
    fn reserved_names_are_never_taken() {
        let d = package();
        let fake = Fake {
            channels: 1,
            frames: 500,
            cancel_after: None,
        };
        // Neither file exists, but a project's history names both.
        let reserved: BTreeSet<String> =
            ["Media/tone.caf".to_string(), "Media/tone 2.caf".to_string()].into();
        let got = import_reserved(
            &fake,
            Path::new("tone.wav"),
            d.path(),
            fmt(BitDepth::Int16),
            "tone",
            &reserved,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(got[0].path, "Media/tone 3.caf");
        // Without reservations the first free name is taken.
        let got = import_as(
            &fake,
            Path::new("tone.wav"),
            d.path(),
            fmt(BitDepth::Int16),
            "tone",
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(got[0].path, "Media/tone.caf");
    }

    #[test]
    fn a_cancel_while_the_overview_builds_leaves_nothing() {
        let d = package();
        let fake = Fake {
            channels: 2,
            frames: 20_000,
            cancel_after: None,
        };
        let cancel = AtomicBool::new(false);
        // The last decoded chunk reports 1.0: cancelling then lets the
        // decode finish and stops the overview build.
        let r = import_as(
            &fake,
            Path::new("t.wav"),
            d.path(),
            fmt(BitDepth::Float32),
            "t",
            &mut |f| {
                if f >= 1.0 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        );
        assert_eq!(r.unwrap_err(), MediaError::Cancelled);
        let (media, incoming) = listing(d.path());
        assert!(media.is_empty() && incoming.is_empty(), "{media:?} {incoming:?}");
        assert!(!d.path().join(crate::overview::DIR).exists());
    }

    #[test]
    fn the_fallback_walks_the_free_name_loop() {
```

Find:
```rust
        let name = link_free_with(&from, &media, "s", true).unwrap();
```
Replace with:
```rust
        let name = link_free_with(&from, &media, "s", &BTreeSet::new(), true).unwrap();
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-media import 2>&1 | tail -5`
Expected: compile errors (`import_reserved`, `BTreeSet` not found; `link_free_with` takes 4 arguments).

- [ ] **Step 3: Implement**

In `crates/core-media/src/import.rs`:

Find:
```rust
//! `Media/.incoming/` and links them into `Media/` under free names. A
//! file wider than two channels becomes one mono file per channel. An
//! error or a cancel removes everything this import wrote.

use crate::MediaError;
use crate::caf::{CafReader, CafWriter, WriteMode, Written};
use core_model::format::AudioFormat;
use std::path::{Path, PathBuf};
```
Replace with:
```rust
//! `Media/.incoming/` and links them into `Media/` under free names (never
//! a name the caller reserves, plan 5b Ruling 12), with each file's
//! waveform overview. A file wider than two channels becomes one mono
//! file per channel. An error or a cancel removes everything this import
//! wrote.

use crate::MediaError;
use crate::caf::{CafReader, CafWriter, WriteMode, Written};
use crate::overview::{Overview, Stamp};
use core_model::format::AudioFormat;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
```

Find:
```rust
/// Links `from` into `media` as `<stem>.caf`, `<stem> 2.caf`, … (the
/// first free name; neither `link` nor the exclusive rename replaces a
/// file); returns the name. Where hard links are unavailable the file is
/// renamed into place instead (the source is then gone).
fn link_free(from: &Path, media: &Path, stem: &str) -> Result<String, MediaError> {
    link_free_with(from, media, stem, false)
}

/// [`link_free`]; `force_fallback` skips the hard link (tests).
fn link_free_with(
    from: &Path,
    media: &Path,
    stem: &str,
    force_fallback: bool,
) -> Result<String, MediaError> {
    for n in 1..=10_000u32 {
        let name = if n == 1 {
            format!("{stem}.caf")
        } else {
            format!("{stem} {n}.caf")
        };
        let to = media.join(&name);
```
Replace with:
```rust
/// Links `from` into `media` as `<stem>.caf`, `<stem> 2.caf`, … (the
/// first free name that `reserved` does not hold as `Media/<name>`;
/// neither `link` nor the exclusive rename replaces a file); returns the
/// name. Where hard links are unavailable the file is renamed into place
/// instead (the source is then gone).
fn link_free(
    from: &Path,
    media: &Path,
    stem: &str,
    reserved: &BTreeSet<String>,
) -> Result<String, MediaError> {
    link_free_with(from, media, stem, reserved, false)
}

/// [`link_free`]; `force_fallback` skips the hard link (tests).
fn link_free_with(
    from: &Path,
    media: &Path,
    stem: &str,
    reserved: &BTreeSet<String>,
    force_fallback: bool,
) -> Result<String, MediaError> {
    for n in 1..=10_000u32 {
        let name = if n == 1 {
            format!("{stem}.caf")
        } else {
            format!("{stem} {n}.caf")
        };
        if reserved.contains(&format!("{MEDIA_DIR}/{name}")) {
            continue;
        }
        let to = media.join(&name);
```

Find:
```rust
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
```
Replace with:
```rust
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
    import_reserved(
        importer,
        source,
        package,
        format,
        stem,
        &BTreeSet::new(),
        progress,
        cancel,
    )
}

/// [`import_as`] that never names a file `Media/<name>` while that
/// package-relative path is in `reserved`: paths a project's undo history
/// references, even when their files are missing (plan 5b Ruling 12).
/// Each new file's overview is written to `Media/.overviews/`.
#[allow(clippy::too_many_arguments)]
pub fn import_reserved(
    importer: &dyn MediaImporter,
    source: &Path,
    package: &Path,
    format: AudioFormat,
    stem: &str,
    reserved: &BTreeSet<String>,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<Vec<Imported>, MediaError> {
    let info = importer.probe(source)?;
```

Find:
```rust
        let written: Vec<Written> = writers
            .into_iter()
            .map(CafWriter::finish)
            .collect::<Result<_, _>>()?;
        std::fs::create_dir_all(&media)?;
```
Replace with:
```rust
        let written: Vec<Written> = writers
            .into_iter()
            .map(CafWriter::finish)
            .collect::<Result<_, _>>()?;
        // Each file's overview, built from its incoming copy before it is
        // linked (the link keeps the inode and mtime, so the stamp holds
        // for the linked file). A failed build is not fatal; a cancel is.
        let mut overviews: Vec<Option<Overview>> = Vec::with_capacity(paths.len());
        for (p, w) in paths.iter().zip(&written) {
            overviews.push(match crate::overview::build(p, cancel) {
                Ok(o) if o.hash == w.content_hash => Stamp::of(p).ok().map(|s| o.with_stamp(s)),
                Err(MediaError::Cancelled) => return Err(MediaError::Cancelled),
                _ => None,
            });
        }
        std::fs::create_dir_all(&media)?;
```

Find:
```rust
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
```
Replace with:
```rust
        for (k, ((p, w), ov)) in paths.iter().zip(written).zip(overviews).enumerate() {
            let s = if split {
                format!("{base} ch{}", k + 1)
            } else {
                base.clone()
            };
            let name = match link_free(p, &media, &s, reserved) {
                Ok(n) => n,
                Err(e) => {
                    remove_all(&linked);
                    return Err(e);
                }
            };
            linked.push(media.join(&name));
            if let Some(o) = ov {
                if let Some(at) = Overview::path(package, &o.hash) {
                    // A cache: a failed write is rebuilt later.
                    let _ = o.write(&at);
                }
            }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p core-media 2>&1 | grep -E "^test result|FAILED|panicked"`
Expected: `test result: ok.` (the 5a import tests unchanged, plus the three new ones and Task 2's eight).

- [ ] **Step 5: Run every gate (with rt-check)**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph 2>&1 | tail -3
```
Expected: all green. `core-ffi`'s 5a media tests still pass: imports now also leave `Media/.overviews/`, a hidden folder none of them lists.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-media/src/import.rs
git commit -m "core-media: imports build overviews and skip reserved names

Every imported file's overview is built from its incoming copy and
written to Media/.overviews/ once linked. import_reserved never names a
file after a path the caller reserves (a history's paths, even when the
file is missing; 5a final review r2).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 4: `core-ffi` — the session's overviews, `Session.overview`, `Session.region_waveform`, and the `Changed` reason

**Files:**
- Create: `crates/core-ffi/src/overviews.rs`, `crates/core-ffi/tests/overviews.rs`
- Modify: `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/media.rs` (`OfflineReasonRow::Changed`), `crates/core-ffi/src/snapshot.rs` (`IdleUpdate.overviews_changed`), `crates/core-ffi/src/control.rs` (`"changed"`), `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `crates/core-ffi/tests/media.rs` (a listing ignores `.overviews`)
- Test: `crates/core-ffi/tests/overviews.rs`

**Interfaces:**
- Consumes: Task 2's `overview::{build, Overview, Stamp, Bins}`; Task 3 (imports already write overviews); `core_engine::schedule::fade_curve`; 5a's `Inner::{refresh_media, root, offline}`, `media::{check, files}`, `host::set_media`.
- Produces:
  - UniFFI: `OverviewStateRow::{Ready, Coarse, Pending, Offline}`; `OverviewRows { state: OverviewStateRow, channels: u16, columns: u32, min: Vec<f32>, max: Vec<f32> }` (channel-major: index `channel * columns + column`);
  - `Session::overview(media: u64, from_frame: u64, to_frame: u64, columns: u32) -> Result<OverviewRows, VizError>`;
  - `Session::region_waveform(event: u64, from_tick: i64, to_tick: i64, columns: u32) -> Result<OverviewRows, VizError>`;
  - `OfflineReasonRow::Changed` (appended); `IdleUpdate.overviews_changed: bool` (default false);
  - Internal: `core_ffi::overviews::{Overviews, Want, News, MAX_COLUMNS, check_columns, columns, fade_gain, copy_cache}`; `Inner::{overviews, changed}`; `Inner::refresh_media_with(id, force, reverify)`;
  - Test hooks (feature `test-hooks`, not exported): `Session::debug_overviews_idle(&self) -> bool`, `Session::debug_hold_overviews(&self, on: bool)`.
- Swift: `FakeSession.overview(media:fromFrame:toFrame:columns:)`, `FakeSession.regionWaveform(event:fromTick:toTick:columns:)` with `waveformRows`/`waveformCalls`.

Rulings 2, 3, 4 and 17. The worker never takes the session lock; the session takes the worker's lock briefly (memory only). `refresh_media` hands the worker every online item plus every `Changed` item; `idle` collects what the worker found: a hash mismatch marks the item `Changed` (once per `(path, hash)`, with one problem line), a successful full verification clears the mark. A refresh caused by those findings does not re-verify (`reverify: false`), so a still-changed file is not read in a loop; `recheck_media` re-verifies everything.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/overviews.rs <<'VIZDAW_EOF'
//! Waveform overviews in the session (plan 5b Task 4, Rulings 2–4, 17):
//! built in the background, served at any zoom, verified by content hash.

use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    AudioEdit, EditCommand, EventKindRow, FadeRow, FadeShapeRow, ImportTargetRow, JobRow,
    JobStateRow, OfflineReasonRow, OverviewStateRow, Session, VizError,
};

/// A 48 kHz CAF in `depth` holding `samples` (interleaved, `ch` channels).
fn caf(path: &Path, ch: u16, depth: BitDepth, samples: &[f32]) -> PathBuf {
    let mut w = CafWriter::create(path, 48_000, ch, depth, WriteMode::Normal).unwrap();
    w.write(samples).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

/// A stereo ramp of `frames` frames: channel 0 repeats every 1000 frames
/// (±0.4), channel 1 every 77 (±0.25).
fn ramp(frames: usize) -> Vec<f32> {
    let mut s = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        s.push(((f % 1000) as f32 / 1000.0 - 0.5) * 0.8);
        s.push(((f % 77) as f32 / 77.0 - 0.5) * 0.5);
    }
    s
}

/// Every sample of the CAF at `path`, interleaved.
fn samples(path: &Path) -> Vec<f32> {
    let r = CafReader::open(path).unwrap();
    let ch = usize::from(r.info().channels);
    let mut out = vec![0.0f32; r.info().frames as usize * ch];
    let mut scratch = Vec::new();
    assert_eq!(r.read(0, &mut out, &mut scratch).unwrap() as u64, r.info().frames);
    out
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

fn import(s: &Arc<Session>, path: &Path, target: Option<ImportTargetRow>) -> JobRow {
    let id = s
        .import_audio(vec![path.display().to_string()], target)
        .unwrap();
    let j = wait(s, id);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    j
}

/// Runs `idle` until the overview worker has been idle across two ticks.
fn settle(s: &Session) {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        s.idle().unwrap();
        if s.debug_overviews_idle() {
            s.idle().unwrap();
            if s.debug_overviews_idle() {
                return;
            }
        }
        assert!(Instant::now() < until, "the overview worker never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The only region of the last lane of the demo's first sequence:
/// `(seq, event, tick, duration ticks)`.
fn last_region(s: &Session) -> (u64, u64, i64, i64) {
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    let ev = s.lane_events(seq, lane).unwrap();
    let r = ev
        .events
        .iter()
        .find(|e| e.kind == EventKindRow::AudioRegion)
        .unwrap();
    (seq, r.id, r.tick, r.duration)
}

fn brute(s: &[f32], c: usize, from: usize, to: usize) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for f in from..to {
        lo = lo.min(s[f * 2 + c]);
        hi = hi.max(s[f * 2 + c]);
    }
    (lo, hi)
}

#[test]
fn an_import_draws_at_every_zoom() {
    let dir = tempfile::tempdir().unwrap();
    // 102 400 frames: 100 columns of 1024 frames, four level-256 peaks each.
    let src = caf(&dir.path().join("ramp.caf"), 2, BitDepth::Float32, &ramp(102_400));
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    settle(&s);
    let m = s.media().unwrap()[0].clone();
    // The stored file (24-bit) is what the overview describes.
    let stored = samples(&pkg.join(&m.path));
    let o = s.overview(m.id, 0, 102_400, 100).unwrap();
    assert_eq!((o.state, o.channels, o.columns), (OverviewStateRow::Ready, 2, 100));
    for c in 0..100usize {
        for k in 0..2usize {
            let (lo, hi) = brute(&stored, k, c * 1024, (c + 1) * 1024);
            let (a, b) = (o.min[k * 100 + c], o.max[k * 100 + c]);
            assert!(a <= lo && lo - a < 1e-4, "column {c} channel {k}: {a} vs {lo}");
            assert!(b >= hi && b - hi < 1e-4, "column {c} channel {k}: {b} vs {hi}");
        }
    }
    // One frame per column: the samples themselves, once the worker read them.
    let first = s.overview(m.id, 5000, 5100, 100).unwrap();
    assert!(
        matches!(first.state, OverviewStateRow::Coarse | OverviewStateRow::Ready),
        "{:?}",
        first.state
    );
    settle(&s);
    let exact = s.overview(m.id, 5000, 5100, 100).unwrap();
    assert_eq!(exact.state, OverviewStateRow::Ready);
    for c in 0..100usize {
        for k in 0..2usize {
            let v = stored[(5000 + c) * 2 + k];
            assert_eq!((exact.min[k * 100 + c], exact.max[k * 100 + c]), (v, v), "column {c}");
        }
    }
    s.close().unwrap();
}

#[test]
fn region_waveform_follows_gain_fades_and_tempo() {
    let dir = tempfile::tempdir().unwrap();
    // Mono, every sample 0.5 (exact at 24 bits), 2 s.
    let src = caf(&dir.path().join("flat.caf"), 1, BitDepth::Float32, &vec![0.5; 96_000]);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    import(&s, &src, Some(ImportTargetRow { seq, lane: None, tick: 0 }));
    let (seq, ev, tick, dur) = last_region(&s);
    // +6.0206 dB doubles; a 1 s linear fade in.
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::SetGain {
                event: ev,
                gain_db: 6.0206,
            },
        },
        None,
    )
    .unwrap();
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event: ev,
                fade_in: FadeRow {
                    length: 48_000,
                    shape: FadeShapeRow::Linear,
                },
                fade_out: FadeRow {
                    length: 0,
                    shape: FadeShapeRow::Linear,
                },
            },
        },
        None,
    )
    .unwrap();
    settle(&s);
    // 200 columns over twice the region: the second half is past its end.
    let w = s.region_waveform(ev, tick, tick + 2 * dur, 200).unwrap();
    assert_eq!((w.state, w.channels, w.columns), (OverviewStateRow::Ready, 1, 200));
    let step = (2 * dur) as f64 / 200.0;
    let ticks: Vec<i64> = (0..200)
        .map(|c| tick + ((c as f64 + 0.5) * step) as i64)
        .collect();
    let secs = s.seconds_at(seq, ticks).unwrap();
    let start = s.seconds_at(seq, vec![tick]).unwrap()[0];
    for (c, sec) in secs.iter().enumerate() {
        let r = (sec - start) * 48_000.0;
        let want = if r >= 96_000.0 {
            0.0
        } else if r < 48_000.0 {
            r / 48_000.0
        } else {
            1.0
        };
        assert!(
            (f64::from(w.max[c]) - want).abs() < 0.03,
            "column {c} at frame {r}: {} vs {want}",
            w.max[c]
        );
    }
    s.close().unwrap();
}

/// Review Focus 2.
#[test]
fn a_same_length_replacement_goes_offline_changed_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("x.caf"), 2, BitDepth::Float32, &vec![0.25; 9600]);
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    settle(&s);
    assert_eq!(s.media().unwrap()[0].offline, None);
    // Another file in the project format, of the same length, other audio.
    let file = pkg.join(&s.media().unwrap()[0].path);
    let kept = dir.path().join("kept.caf");
    std::fs::rename(&file, &kept).unwrap();
    caf(&file, 2, BitDepth::Int24, &vec![0.5; 9600]);
    s.recheck_media().unwrap();
    settle(&s);
    let row = s.media().unwrap()[0].clone();
    assert_eq!(row.offline, Some(OfflineReasonRow::Changed));
    let w = s.overview(row.id, 0, 4800, 10).unwrap();
    assert_eq!(w.state, OverviewStateRow::Offline);
    // The original comes back.
    std::fs::remove_file(&file).unwrap();
    std::fs::rename(&kept, &file).unwrap();
    s.recheck_media().unwrap();
    settle(&s);
    assert_eq!(s.media().unwrap()[0].offline, None);
    s.close().unwrap();
}

/// Review Focus 5.
#[test]
fn waveform_requests_survive_hostile_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("h.caf"), 2, BitDepth::Float32, &ramp(4800));
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    import(&s, &src, Some(ImportTargetRow { seq, lane: None, tick: 0 }));
    let (seq, ev, tick, dur) = last_region(&s);
    let media = s.media().unwrap()[0].id;
    let model = |r: Result<vizdaw_ffi::OverviewRows, VizError>| {
        assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}")
    };
    for cols in [0, 8193, u32::MAX] {
        model(s.region_waveform(ev, tick, tick + dur, cols));
        model(s.overview(media, 0, 100, cols));
    }
    model(s.region_waveform(ev, tick + 10, tick + 10, 10));
    model(s.region_waveform(ev, tick + 10, tick, 10));
    model(s.overview(media, 100, 100, 10));
    model(s.region_waveform(999_999, 0, 10, 10));
    model(s.overview(999_999, 0, 10, 10));
    // A note of the demo is not a region.
    let lane0 = s.lanes(seq).unwrap()[0].id;
    let note = s
        .lane_events(seq, lane0)
        .unwrap()
        .events
        .iter()
        .find(|e| e.kind == EventKindRow::Note)
        .map(|e| e.id)
        .unwrap();
    model(s.region_waveform(note, 0, 100, 10));
    // Absurd but well-formed ranges answer, with silence outside the region.
    let w = s.region_waveform(ev, i64::MIN, i64::MAX, 8192).unwrap();
    assert_eq!((w.min.len(), w.max.len()), (2 * 8192, 2 * 8192));
    assert!(w.min.iter().chain(&w.max).all(|v| v.is_finite()));
    let o = s.overview(media, 0, u64::MAX, 10).unwrap();
    assert_eq!(o.columns, 10);
    // The region goes (Undo the import): its ID is unknown now.
    s.undo().unwrap();
    model(s.region_waveform(ev, tick, tick + dur, 10));
    s.close().unwrap();
}

/// Sets every `content_hash` in a project's JSON to `hash`.
fn set_hashes(v: &mut serde_json::Value, hash: &str) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "content_hash" {
                    *x = serde_json::Value::String(hash.into());
                } else {
                    set_hashes(x, hash);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| set_hashes(x, hash)),
        _ => {}
    }
}

/// Every file under `dir`, recursively.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// Review Focus 3.
#[test]
fn a_hostile_content_hash_never_names_a_path() {
    for hostile in ["../../../evil", "AB", &"f".repeat(100_000)] {
        let dir = tempfile::tempdir().unwrap();
        let src = caf(&dir.path().join("y.caf"), 2, BitDepth::Float32, &ramp(3000));
        let s = Session::new_empty().unwrap();
        import(&s, &src, None);
        let pkg = dir.path().join("P.vizdaw");
        s.save_as(pkg.display().to_string()).unwrap();
        s.close().unwrap();
        let pj = pkg.join("project.json");
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&pj).unwrap()).unwrap();
        set_hashes(&mut v, hostile);
        std::fs::write(&pj, serde_json::to_string(&v).unwrap()).unwrap();
        let s = Session::open(pkg.display().to_string()).unwrap();
        settle(&s);
        assert_eq!(
            s.media().unwrap()[0].offline,
            Some(OfflineReasonRow::Changed),
            "{hostile:.20}"
        );
        for f in walk(dir.path()) {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            assert!(!name.contains("evil"), "{f:?}");
            if f.parent().unwrap().ends_with(".overviews") {
                let stem = name.trim_end_matches(".vzov");
                assert!(
                    stem.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                    "{f:?}"
                );
            }
        }
        s.close().unwrap();
    }
}

#[test]
fn save_as_carries_the_overviews_and_a_damaged_one_is_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("z.caf"), 2, BitDepth::Float32, &ramp(5000));
    let s = Session::new_empty().unwrap();
    import(&s, &src, None);
    let a = dir.path().join("A.vizdaw");
    s.save_as(a.display().to_string()).unwrap();
    settle(&s);
    let b = dir.path().join("B.vizdaw");
    s.save_as(b.display().to_string()).unwrap();
    let list = |p: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(p.join("Media/.overviews"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "vzov"))
            .collect()
    };
    assert_eq!(list(&b).len(), 1, "Save As copied the overview");
    s.close().unwrap();
    // Garbage in the cache: refused, rebuilt, served.
    let ov = list(&b).remove(0);
    std::fs::write(&ov, b"garbage").unwrap();
    let s = Session::open(b.display().to_string()).unwrap();
    settle(&s);
    let m = s.media().unwrap()[0].clone();
    assert_eq!(m.offline, None);
    // 1000 frames per column: served from the overview itself.
    assert_eq!(
        s.overview(m.id, 0, 5000, 5).unwrap().state,
        OverviewStateRow::Ready
    );
    assert!(core_media::overview::Overview::read(&ov).is_ok(), "rewritten");
    s.close().unwrap();
}

#[test]
fn reads_never_wait_for_the_worker() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("w.caf"), 2, BitDepth::Float32, &ramp(48_000));
    let s = Session::new_empty().unwrap();
    // The worker is held: the new item's overview (written by the import)
    // is not loaded yet.
    s.debug_hold_overviews(true);
    import(&s, &src, None);
    let m = s.media().unwrap()[0].clone();
    let started = Instant::now();
    let o = s.overview(m.id, 0, 48_000, 100).unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(o.state, OverviewStateRow::Pending);
    assert!(o.min.iter().chain(&o.max).all(|v| *v == 0.0));
    s.debug_hold_overviews(false);
    settle(&s);
    assert_eq!(s.overview(m.id, 0, 48_000, 100).unwrap().state, OverviewStateRow::Ready);
    s.close().unwrap();
}
VIZDAW_EOF
```

`core-ffi` already has `serde_json` as a dependency and `tempfile` as a dev-dependency; integration tests can use both. The hostile case `"f".repeat(100_000)` is a 100 000-character lowercase-hex string: longer than 64, so it is not a valid overview name either.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-ffi --test overviews 2>&1 | tail -5`
Expected: compile errors (`OverviewStateRow`, `Session::overview`, `Session::region_waveform`, `debug_overviews_idle` not found).

- [ ] **Step 3: The overview store**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/overviews.rs <<'VIZDAW_EOF'
//! Waveform overviews in a session (sub-project 5 spec §10.1; plan 5b
//! Rulings 2–4, 17). One worker thread per session builds, verifies and
//! loads the overview of every online media item and reads exact sample
//! bins for views zoomed in below 256 frames per column. It opens files
//! with its own `CafReader`: it never touches the engine, the streamer or
//! its RAM cache, so nothing here is near the real-time path. Reads on the
//! FFI are memory lookups; what is not loaded yet answers `Pending` (or
//! `Coarse`) and the next `IdleUpdate.overviews_changed` says to ask again.

use crate::error::VizError;
use core_media::caf::CafReader;
use core_media::overview::{Bins, Overview, Stamp};
use core_model::event::Fade;
use core_model::ids::MediaId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

/// How a waveform read came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OverviewStateRow {
    /// The columns are exact (from the overview or from the samples).
    Ready,
    /// Zoomed in below 256 frames per column: these columns come from the
    /// 256-frame peaks while the samples are read; ask again after the
    /// next `IdleUpdate.overviews_changed`.
    Coarse,
    /// The overview is being built or loaded: the columns are silent; ask
    /// again after the next `IdleUpdate.overviews_changed`.
    Pending,
    /// The media is offline: nothing to draw.
    Offline,
}

/// Waveform columns: per channel, then per column (`channel * columns +
/// column`), the lowest and highest sample, linear (±1 at full scale,
/// more with gain).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct OverviewRows {
    /// How they came out.
    pub state: OverviewStateRow,
    /// 1 or 2.
    pub channels: u16,
    /// Columns per channel.
    pub columns: u32,
    /// Lowest sample per column.
    pub min: Vec<f32>,
    /// Highest sample per column.
    pub max: Vec<f32>,
}

impl OverviewRows {
    /// All-zero columns.
    pub fn silent(state: OverviewStateRow, channels: u16, columns: u32) -> OverviewRows {
        let n = usize::from(channels.max(1)) * columns as usize;
        OverviewRows {
            state,
            channels: channels.max(1),
            columns,
            min: vec![0.0; n],
            max: vec![0.0; n],
        }
    }
}

/// The most columns one read returns.
pub const MAX_COLUMNS: u32 = 8192;
/// Below this many frames per column, columns come from the samples.
const SAMPLE_LEVEL: f64 = 256.0;
/// Exact sample windows kept.
const WINDOWS: usize = 32;

/// `Model` error unless `columns` is 1–[`MAX_COLUMNS`].
pub fn check_columns(columns: u32) -> Result<(), VizError> {
    if (1..=MAX_COLUMNS).contains(&columns) {
        Ok(())
    } else {
        Err(VizError::Model {
            message: format!("columns must be 1–{MAX_COLUMNS}, not {columns}"),
        })
    }
}

/// A media item the session wants an overview of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    /// The item.
    pub media: MediaId,
    /// Its package-relative path (as the document says).
    pub path: String,
    /// Its file.
    pub file: PathBuf,
    /// The content hash the document expects (`None`: unknown).
    pub hash: Option<String>,
    /// The package whose `Media/.overviews` caches it.
    pub package: PathBuf,
}

/// What the worker found since the last [`Overviews::take_news`].
#[derive(Debug, Default)]
pub struct News {
    /// Some overview or sample window became available: redraw.
    pub changed: bool,
    /// Files that no longer hold the audio expected: `(item, path, hash)`.
    pub mismatched: Vec<(MediaId, String, Option<String>)>,
    /// Files read and found to hold the expected audio: `(item, path)`.
    pub verified: Vec<(MediaId, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WindowKey {
    media: MediaId,
    file: PathBuf,
    from: u64,
    to: u64,
    step: u64,
}

#[derive(Debug)]
enum Task {
    Ensure(Want),
    Window(WindowKey),
}

#[derive(Default)]
struct State {
    queue: VecDeque<Task>,
    wanted: BTreeMap<MediaId, Want>,
    /// Items queued or done for their current want.
    handled: BTreeSet<MediaId>,
    by_hash: BTreeMap<String, Arc<Overview>>,
    /// Each item's overview hash (the expected one, or the one a build
    /// computed for an item without a hash).
    keys: BTreeMap<MediaId, String>,
    windows: VecDeque<(WindowKey, Arc<Bins>)>,
    asked: BTreeSet<WindowKey>,
    news: News,
    busy: bool,
    held: bool,
    stop: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    cancel: AtomicBool,
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A session's overviews and their worker.
pub struct Overviews {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Default for Overviews {
    fn default() -> Overviews {
        Overviews {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
                cancel: AtomicBool::new(false),
            }),
            thread: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for Overviews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overviews").finish_non_exhaustive()
    }
}

impl Drop for Overviews {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Overviews {
    /// The items to keep overviews for. With `again` every item is checked
    /// again (a recheck); otherwise only items that are new or whose path
    /// or hash changed.
    pub fn ensure(&self, wants: Vec<Want>, again: bool) {
        let mut s = lock(&self.shared.state);
        let next: BTreeMap<MediaId, Want> = wants.into_iter().map(|w| (w.media, w)).collect();
        let prev = std::mem::take(&mut s.wanted);
        s.handled
            .retain(|m| !again && prev.contains_key(m) && prev.get(m) == next.get(m));
        let handled = s.handled.clone();
        s.queue.retain(|t| match t {
            Task::Ensure(w) => handled.contains(&w.media),
            Task::Window(_) => true,
        });
        for w in next.values() {
            if s.handled.insert(w.media) {
                s.queue.push_back(Task::Ensure(w.clone()));
            }
        }
        s.wanted = next;
        let work = !s.queue.is_empty();
        drop(s);
        if work {
            self.start();
            self.shared.wake.notify_all();
        }
    }

    /// The loaded overview of `media`, if any.
    pub fn get(&self, media: MediaId) -> Option<Arc<Overview>> {
        let s = lock(&self.shared.state);
        s.keys.get(&media).and_then(|h| s.by_hash.get(h)).cloned()
    }

    /// Exact sample bins of `file` over `[from, to)` in `step`-frame bins,
    /// if read; otherwise asks the worker (replacing an older request for
    /// the same item) and answers `None`.
    pub fn window(&self, media: MediaId, file: &Path, from: u64, to: u64, step: u64) -> Option<Arc<Bins>> {
        let mut s = lock(&self.shared.state);
        if let Some((_, b)) = s.windows.iter().find(|(k, _)| {
            k.media == media && k.file.as_path() == file && k.step == step && k.from <= from && to <= k.to
        }) {
            return Some(Arc::clone(b));
        }
        let key = WindowKey {
            media,
            file: file.to_path_buf(),
            from,
            to,
            step,
        };
        if s.asked.contains(&key) {
            return None;
        }
        // Scrolling asks again and again: only the newest request per item
        // is worth reading.
        let stale: Vec<WindowKey> = s
            .queue
            .iter()
            .filter_map(|t| match t {
                Task::Window(k) if k.media == media => Some(k.clone()),
                _ => None,
            })
            .collect();
        s.queue
            .retain(|t| !matches!(t, Task::Window(k) if k.media == media));
        for k in stale {
            s.asked.remove(&k);
        }
        s.asked.insert(key.clone());
        s.queue.push_front(Task::Window(key));
        drop(s);
        self.start();
        self.shared.wake.notify_all();
        None
    }

    /// What the worker found since the last call.
    pub fn take_news(&self) -> News {
        std::mem::take(&mut lock(&self.shared.state).news)
    }

    /// True when the worker has nothing queued and is not working (test
    /// hook).
    pub fn idle(&self) -> bool {
        let s = lock(&self.shared.state);
        (s.held || s.queue.is_empty()) && !s.busy
    }

    /// Holds the worker before its next request (`true`), or lets it go
    /// (test hook).
    pub fn set_held(&self, on: bool) {
        lock(&self.shared.state).held = on;
        self.shared.wake.notify_all();
    }

    /// Stops the worker and waits for it (the session closes).
    pub fn shutdown(&self) {
        lock(&self.shared.state).stop = true;
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        let t = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(t) = t {
            let _ = t.join();
        }
    }

    fn start(&self) {
        let mut t = self.thread.lock().unwrap_or_else(PoisonError::into_inner);
        if t.is_some() || lock(&self.shared.state).stop {
            return;
        }
        let sh = Arc::clone(&self.shared);
        // A failed spawn leaves requests pending (drawn as Pending).
        *t = std::thread::Builder::new()
            .name("vizdaw-overviews".into())
            .spawn(move || run(&sh))
            .ok();
    }
}

enum Ensured {
    Loaded(Arc<Overview>),
    Mismatch,
    Failed,
}

/// The worker's loop: windows first (they are at the front), then
/// ensures, until stopped.
fn run(sh: &Shared) {
    loop {
        let task = {
            let mut s = lock(&sh.state);
            loop {
                if s.stop {
                    return;
                }
                if !s.held {
                    if let Some(t) = s.queue.pop_front() {
                        s.busy = true;
                        break t;
                    }
                }
                s = sh.wake.wait(s).unwrap_or_else(PoisonError::into_inner);
            }
        };
        match task {
            Task::Ensure(w) => {
                let out = ensure_one(sh, &w);
                let mut s = lock(&sh.state);
                s.busy = false;
                let current = s.wanted.get(&w.media) == Some(&w);
                match out {
                    Ensured::Loaded(ov) => {
                        let h = ov.hash.clone();
                        s.by_hash.insert(h.clone(), ov);
                        if current {
                            s.keys.insert(w.media, h);
                            s.news.verified.push((w.media, w.path.clone()));
                            s.news.changed = true;
                        }
                    }
                    Ensured::Mismatch => {
                        if current {
                            s.keys.remove(&w.media);
                            s.news.mismatched.push((w.media, w.path.clone(), w.hash.clone()));
                        }
                    }
                    Ensured::Failed => {}
                }
            }
            Task::Window(k) => {
                let r = CafReader::open(&k.file).and_then(|r| Bins::read(&r, k.from, k.to, k.step));
                let mut s = lock(&sh.state);
                s.busy = false;
                s.asked.remove(&k);
                if let Ok(b) = r {
                    s.windows.push_back((k, Arc::new(b)));
                    while s.windows.len() > WINDOWS {
                        s.windows.pop_front();
                    }
                    s.news.changed = true;
                }
            }
        }
    }
}

/// Loads `w`'s overview when a cached one (in memory or on disk) has the
/// expected hash and a stamp of the file as it is now; otherwise builds it
/// (reading the whole file, which checks its hash) and caches it.
fn ensure_one(sh: &Shared, w: &Want) -> Ensured {
    let stamp = Stamp::of(&w.file).ok();
    if let (Some(h), Some(st)) = (w.hash.as_deref(), stamp) {
        let held = lock(&sh.state).by_hash.get(h).cloned();
        if let Some(ov) = held.filter(|o| o.has_stamp(&st)) {
            return Ensured::Loaded(ov);
        }
        if let Some(p) = Overview::path(&w.package, h) {
            if let Ok(ov) = Overview::read(&p) {
                if ov.hash == h && ov.has_stamp(&st) {
                    return Ensured::Loaded(Arc::new(ov));
                }
            }
        }
    }
    let mut ov = match core_media::overview::build(&w.file, &sh.cancel) {
        Ok(o) => o,
        Err(_) => return Ensured::Failed,
    };
    if let Some(h) = &w.hash {
        if ov.hash != *h {
            return Ensured::Mismatch;
        }
    }
    if let Some(p) = Overview::path(&w.package, &ov.hash) {
        // Keep the stamps of other files with the same audio.
        if let Ok(old) = Overview::read(&p) {
            if old.hash == ov.hash {
                for s in old.stamps.iter().copied() {
                    ov = ov.with_stamp(s);
                }
            }
        }
        if let Some(st) = stamp {
            ov = ov.with_stamp(st);
        }
        let _ = ov.write(&p);
    } else if let Some(st) = stamp {
        ov = ov.with_stamp(st);
    }
    Ensured::Loaded(Arc::new(ov))
}

/// Waveform columns of `media` (file `file`; `channels` while nothing is
/// loaded): column `c` covers media frames `spans[c]` (`None`: silence),
/// scaled by `gain(c)`. Below 256 frames per column the columns come from
/// exact sample bins, or (`Coarse`) the overview until those are read.
pub fn columns(
    store: &Overviews,
    media: MediaId,
    file: &Path,
    channels: u16,
    spans: &[Option<(u64, u64)>],
    gain: &dyn Fn(usize) -> f32,
) -> OverviewRows {
    let n = spans.len();
    let Some(ov) = store.get(media) else {
        return OverviewRows::silent(OverviewStateRow::Pending, channels, n as u32);
    };
    let (lo, hi) = spans
        .iter()
        .flatten()
        .fold((u64::MAX, 0u64), |(a, b), (x, y)| (a.min(*x), b.max(*y)));
    let mut state = OverviewStateRow::Ready;
    let mut bins = None;
    if lo < hi && n > 0 {
        let fpc = (hi - lo) as f64 / n as f64;
        if fpc < SAMPLE_LEVEL {
            let step = (fpc.floor() as u64).max(1);
            bins = store.window(media, file, lo, hi, step);
            if bins.is_none() {
                state = OverviewStateRow::Coarse;
            }
        }
    }
    let mut out = OverviewRows::silent(state, ov.channels, n as u32);
    for (c, span) in spans.iter().enumerate() {
        let Some((a, b)) = *span else { continue };
        let g = gain(c);
        for k in 0..ov.channels {
            let r = match &bins {
                Some(bn) => bn.range(k, a, b),
                None => ov.range(k, a, b),
            };
            if let Some((x, y)) = r {
                let i = usize::from(k) * n + c;
                if let Some(v) = out.min.get_mut(i) {
                    *v = (x * g).min(y * g);
                }
                if let Some(v) = out.max.get_mut(i) {
                    *v = (x * g).max(y * g);
                }
            }
        }
    }
    out
}

/// The fade gain at region frame `r` of a region of `length` frames (the
/// engine's `Segment::gain_at`, without the region gain).
pub fn fade_gain(r: u64, length: u64, fade_in: Fade, fade_out: Fade) -> f32 {
    use core_engine::schedule::fade_curve;
    let mut g = 1.0;
    let fi = fade_in.length_frames;
    if fi > 0 && r < fi {
        g *= fade_curve(fade_in.shape, r as f32 / fi as f32);
    }
    let fo = fade_out.length_frames;
    let left = length.saturating_sub(r);
    if fo > 0 && left <= fo {
        g *= fade_curve(fade_out.shape, left as f32 / fo as f32);
    }
    g
}

/// Copies the overview of every hash in `media` (path → hash, the
/// history's `referenced_media_anywhere`) from package `from` to `to`
/// when `to` lacks it (Save As, Ruling 17). Best effort: a missing one is
/// rebuilt later.
pub fn copy_cache(media: &BTreeMap<String, Option<String>>, from: &Path, to: &Path) {
    if from == to {
        return;
    }
    for h in media.values().flatten() {
        let (Some(src), Some(dst)) = (Overview::path(from, h), Overview::path(to, h)) else {
            continue;
        };
        if dst.exists() || !src.is_file() {
            continue;
        }
        if let Some(dir) = dst.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::copy(&src, &dst);
    }
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod modify;
pub mod plugins;
```
Replace with:
```rust
pub mod modify;
pub mod overviews;
pub mod plugins;
```

Find:
```rust
pub use media::{MediaOriginRow, MediaRow, OfflineReasonRow};
```
Replace with:
```rust
pub use media::{MediaOriginRow, MediaRow, OfflineReasonRow};
pub use overviews::{OverviewRows, OverviewStateRow};
```

- [ ] **Step 4: The `Changed` reason and the idle flag**

In `crates/core-ffi/src/media.rs`:

Find:
```rust
    /// The file's length differs from the item's.
    Length,
}
```
Replace with:
```rust
    /// The file's length differs from the item's.
    Length,
    /// The file no longer holds the audio the project recorded for the
    /// item: its content hash differs (spec §4.3; plan 5b Ruling 2).
    Changed,
}
```

In `crates/core-ffi/src/control.rs`:

Find:
```rust
                    crate::media::OfflineReasonRow::Length => "length",
                }),
```
Replace with:
```rust
                    crate::media::OfflineReasonRow::Length => "length",
                    crate::media::OfflineReasonRow::Changed => "changed",
                }),
```

In `crates/core-ffi/src/snapshot.rs`:

Find:
```rust
    /// The media rows changed (a job finished, media went offline):
    /// refetch `Session.media`.
    #[uniffi(default = false)]
    pub media_changed: bool,
}
```
Replace with:
```rust
    /// The media rows changed (a job finished, media went offline):
    /// refetch `Session.media`.
    #[uniffi(default = false)]
    pub media_changed: bool,
    /// Waveform overviews or sample windows became available: views
    /// showing `Pending` or `Coarse` waveforms ask again (plan 5b).
    #[uniffi(default = false)]
    pub overviews_changed: bool,
}
```

- [ ] **Step 5: The session**

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// Imports and conversions (Ruling 15).
    jobs: crate::jobs::Jobs,
}
```
Replace with:
```rust
    /// Imports and conversions (Ruling 15).
    jobs: crate::jobs::Jobs,
    /// Waveform overviews and their worker (plan 5b Ruling 3).
    overviews: crate::overviews::Overviews,
    /// Items whose file failed the content-hash check, with the path and
    /// hash it was checked for (plan 5b Ruling 2; runtime only).
    changed: std::collections::BTreeMap<core_model::ids::MediaId, (String, Option<String>)>,
}
```

Find:
```rust
            offline: Default::default(),
            media_seen: None,
            jobs: Default::default(),
        }
    }
```
Replace with:
```rust
            offline: Default::default(),
            media_seen: None,
            jobs: Default::default(),
            overviews: Default::default(),
            changed: Default::default(),
        }
    }
```

Find:
```rust
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
Replace with:
```rust
    /// Recomputes the offline set when the media pool, the format or the
    /// root changed (or `force`), and hands the online media to the
    /// engine host and to the overview worker (which re-verifies every
    /// item when `force`). True when the offline set changed.
    fn refresh_media(&mut self, id: u64, force: bool) -> bool {
        self.refresh_media_with(id, force, force)
    }

    /// [`Inner::refresh_media`]; `reverify` makes the overview worker
    /// check every item again (a recheck), not only new ones.
    fn refresh_media_with(&mut self, id: u64, force: bool, reverify: bool) -> bool {
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
        let mut offline = crate::media::check(doc, &root);
        // A file that failed the hash check stays offline while the item
        // still names that file and hash (plan 5b Ruling 2).
        for (m, (path, hash)) in &self.changed {
            if let Some(item) = doc.media().get(m) {
                if item.path == *path && item.content_hash == *hash {
                    offline
                        .entry(*m)
                        .or_insert(crate::media::OfflineReasonRow::Changed);
                }
            }
        }
        let changed = offline != self.offline;
        host::set_media(id, crate::media::files(doc, &root, &offline));
        // Overviews of online items, and of changed ones so a returning
        // original is noticed.
        let wants = doc
            .media()
            .values()
            .filter(|m| match offline.get(&m.id) {
                None | Some(crate::media::OfflineReasonRow::Changed) => true,
                Some(_) => false,
            })
            .map(|m| crate::overviews::Want {
                media: m.id,
                path: m.path.clone(),
                file: root.join(&m.path),
                hash: m.content_hash.clone(),
                package: root.clone(),
            })
            .collect();
        self.overviews.ensure(wants, reverify);
        self.offline = offline;
        self.media_seen = Some((doc.clone(), root));
        changed
    }
```

Find:
```rust
            if stream_failed {
                // The failed items leave the engine's table (also after a
                // job reloaded it above), so a recheck that finds them
                // back lists them again and the streamer reopens them
                // (final review I2).
                let root = inner.root(self.id);
                let files = crate::media::files(inner.doc(), &root, &inner.offline);
                host::set_media(self.id, files);
                inner.reload_host(self.id);
            }
            let media_changed = jobs_changed || owed || stream_failed;
            let changed = changed || jobs_changed;
            let n = std::mem::take(&mut inner.news);
            Ok(IdleUpdate {
                snapshot: if changed || n.doc_changed {
                    Some(inner.snapshot()?)
                } else {
                    None
                },
                slots_changed: n.slots_changed,
                params_changed: n.params_changed.into_iter().collect(),
                problems: n.problems,
                media_changed,
            })
```
Replace with:
```rust
            if stream_failed {
                // The failed items leave the engine's table (also after a
                // job reloaded it above), so a recheck that finds them
                // back lists them again and the streamer reopens them
                // (final review I2).
                let root = inner.root(self.id);
                let files = crate::media::files(inner.doc(), &root, &inner.offline);
                host::set_media(self.id, files);
                inner.reload_host(self.id);
            }
            // What the overview worker found (plan 5b Ruling 2).
            let found = inner.overviews.take_news();
            let mut hash_news = false;
            for (m, path, hash) in found.mismatched {
                if inner.changed.get(&m) == Some(&(path.clone(), hash.clone())) {
                    continue;
                }
                let name = inner
                    .doc()
                    .media()
                    .get(&m)
                    .map_or_else(|| m.to_string(), |x| x.name.clone());
                inner.news.problems.push(format!(
                    "{name}: the file no longer holds the audio this project expects; it stays silent until it is relinked or restored"
                ));
                inner.changed.insert(m, (path, hash));
                hash_news = true;
            }
            for (m, path) in found.verified {
                if inner.changed.get(&m).is_some_and(|(p, _)| *p == path) {
                    inner.changed.remove(&m);
                    hash_news = true;
                }
            }
            if hash_news {
                // Not a re-verification: a still-changed file is not read
                // again in a loop.
                inner.refresh_media_with(self.id, true, false);
                inner.reload_host(self.id);
            }
            let media_changed = jobs_changed || owed || stream_failed || hash_news;
            let changed = changed || jobs_changed;
            let n = std::mem::take(&mut inner.news);
            Ok(IdleUpdate {
                snapshot: if changed || n.doc_changed {
                    Some(inner.snapshot()?)
                } else {
                    None
                },
                slots_changed: n.slots_changed,
                params_changed: n.params_changed.into_iter().collect(),
                problems: n.problems,
                media_changed,
                overviews_changed: found.changed,
            })
```

Find:
```rust
            let nothing = || IdleUpdate {
                snapshot: None,
                slots_changed: false,
                params_changed: Vec::new(),
                problems: Vec::new(),
                media_changed: false,
            };
```
Replace with:
```rust
            let nothing = || IdleUpdate {
                snapshot: None,
                slots_changed: false,
                params_changed: Vec::new(),
                problems: Vec::new(),
                media_changed: false,
                overviews_changed: false,
            };
```

Save As carries the overviews (Ruling 17).

Find:
```rust
        crate::media::copy_media(
            &inner.history.referenced_media_anywhere(),
            &inner.root(self.id),
            path,
        )?;
        file::save(inner.doc(), path)?;
```
Replace with:
```rust
        let refs = inner.history.referenced_media_anywhere();
        crate::media::copy_media(&refs, &inner.root(self.id), path)?;
        crate::overviews::copy_cache(&refs, &inner.root(self.id), path);
        file::save(inner.doc(), path)?;
```

The worker stops before the scratch package goes.

Find:
```rust
            inner.closed = true;
            let root = inner.root(self.id);
            for w in inner.jobs.shutdown() {
                discard(&root, w);
            }
```
Replace with:
```rust
            inner.closed = true;
            let root = inner.root(self.id);
            for w in inner.jobs.shutdown() {
                discard(&root, w);
            }
            inner.overviews.shutdown();
```

A changed file must be relinked before a format change can convert it. In `set_project_format`:

Find:
```rust
                        Some(
                            crate::media::OfflineReasonRow::Missing
                                | crate::media::OfflineReasonRow::Length
                        )
```
Replace with:
```rust
                        Some(
                            crate::media::OfflineReasonRow::Missing
                                | crate::media::OfflineReasonRow::Length
                                | crate::media::OfflineReasonRow::Changed
                        )
```

Test hooks.

Find:
```rust
    /// True when every job thread has ended, applied or not (test hook).
    pub fn debug_jobs_settled(&self) -> bool {
        lock(&self.inner).jobs.settled()
    }
}
```
Replace with:
```rust
    /// True when every job thread has ended, applied or not (test hook).
    pub fn debug_jobs_settled(&self) -> bool {
        lock(&self.inner).jobs.settled()
    }

    /// True when the overview worker has nothing to do (test hook).
    pub fn debug_overviews_idle(&self) -> bool {
        lock(&self.inner).overviews.idle()
    }

    /// Holds the overview worker (`true`) or lets it go (test hook).
    pub fn debug_hold_overviews(&self, on: bool) {
        lock(&self.inner).overviews.set_held(on);
    }
}
```

The reads.

Find:
```rust
/// Media and jobs (plan 5a Task 10).
#[uniffi::export]
impl Session {
```
Replace with:
```rust
/// Waveform overviews (plan 5b Task 4).
#[uniffi::export]
impl Session {
    /// `columns` columns of media item `media` over frames `[from_frame,
    /// to_frame)` (cut to the file). Never waits for a file: what is not
    /// loaded answers `Pending` (spec §10.1; plan 5b Ruling 3).
    pub fn overview(
        &self,
        media: u64,
        from_frame: u64,
        to_frame: u64,
        columns: u32,
    ) -> Result<crate::overviews::OverviewRows, VizError> {
        ffi_guard(|| {
            use crate::overviews::{OverviewRows, OverviewStateRow};
            crate::overviews::check_columns(columns)?;
            if from_frame >= to_frame {
                return Err(crate::events::bad("from_frame must be before to_frame"));
            }
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let id = core_model::ids::MediaId(media);
            let m = inner
                .doc()
                .media()
                .get(&id)
                .cloned()
                .ok_or_else(|| crate::events::bad(&format!("unknown media {media}")))?;
            if inner.offline.contains_key(&id) {
                return Ok(OverviewRows::silent(OverviewStateRow::Offline, m.channels, columns));
            }
            let to = to_frame.min(m.length_frames.max(from_frame + 1));
            let n = columns as usize;
            let w = (to - from_frame) as f64 / n as f64;
            let spans: Vec<Option<(u64, u64)>> = (0..n)
                .map(|c| {
                    let a = from_frame + (c as f64 * w).floor() as u64;
                    let b = (from_frame + ((c + 1) as f64 * w).ceil() as u64).min(to);
                    (b > a).then_some((a, b))
                })
                .collect();
            let file = inner.root(self.id).join(&m.path);
            Ok(crate::overviews::columns(
                &inner.overviews,
                id,
                &file,
                m.channels,
                &spans,
                &|_| 1.0,
            ))
        })
    }

    /// `columns` columns of audio region `event` over ticks `[from_tick,
    /// to_tick)` of its sequence, mapped through the tempo map, scaled by
    /// the region's gain and shaped by its fades; columns outside the
    /// region are silent (plan 5b Ruling 4).
    pub fn region_waveform(
        &self,
        event: u64,
        from_tick: i64,
        to_tick: i64,
        columns: u32,
    ) -> Result<crate::overviews::OverviewRows, VizError> {
        ffi_guard(|| {
            use crate::overviews::{OverviewRows, OverviewStateRow};
            use core_model::event::EventBody;
            crate::overviews::check_columns(columns)?;
            if from_tick >= to_tick {
                return Err(crate::events::bad("from_tick must be before to_tick"));
            }
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let doc = inner.doc().clone();
            let (ev, loc) = doc.event(core_model::ids::EventId(event))?;
            let EventBody::AudioRegion {
                media,
                source_offset,
                length,
                gain_db,
                fade_in,
                fade_out,
            } = ev.body
            else {
                return Err(crate::events::bad(&format!("{event} is not an audio region")));
            };
            let m = doc
                .media()
                .get(&media)
                .ok_or_else(|| crate::events::bad(&format!("unknown media {media}")))?;
            if inner.offline.contains_key(&media) {
                return Ok(OverviewRows::silent(OverviewStateRow::Offline, m.channels, columns));
            }
            let tempo = &doc.sequence(loc.seq)?.tempo;
            let rate = f64::from(m.sample_rate.max(1));
            let start = tempo.tick_to_seconds(ev.tick);
            // Region frame at tick `t` (f64 ticks; `as` saturates).
            let frame = |t: f64| (tempo.tick_to_seconds(core_model::time::Tick(t as i64)) - start) * rate;
            let n = columns as usize;
            let (t0, t1) = (from_tick as f64, to_tick as f64);
            let w = (t1 - t0) / n as f64;
            let len = length as f64;
            let spans: Vec<Option<(u64, u64)>> = (0..n)
                .map(|c| {
                    let a = frame(t0 + c as f64 * w).max(0.0);
                    let b = frame(t0 + (c + 1) as f64 * w).min(len);
                    if a.is_nan() || b.is_nan() || b <= a {
                        return None;
                    }
                    let a = a.floor() as u64;
                    let b = (b.ceil() as u64).clamp(a + 1, length.max(a + 1));
                    Some((source_offset.saturating_add(a), source_offset.saturating_add(b)))
                })
                .collect();
            let lin = 10f32.powf(gain_db / 20.0);
            let gain = |c: usize| -> f32 {
                let Some(Some((a, b))) = spans.get(c) else {
                    return 0.0;
                };
                let r = (a + b) / 2 - source_offset.min((a + b) / 2);
                lin * crate::overviews::fade_gain(r, length, fade_in, fade_out)
            };
            let file = inner.root(self.id).join(&m.path);
            Ok(crate::overviews::columns(
                &inner.overviews,
                media,
                &file,
                m.channels,
                &spans,
                &gain,
            ))
        })
    }
}

/// Media and jobs (plan 5a Task 10).
#[uniffi::export]
impl Session {
```

(`(a + b) / 2` cannot overflow: `b` is at most `source_offset + length`, both within the media file's frame count.)

- [ ] **Step 6: `FakeSession` and a 5a test that lists `Media/`**

Save As now also copies `Media/.overviews/`, which a 5a test's listing of `Media/` sees. In `crates/core-ffi/tests/media.rs`:

Find:
```rust
    // No temp file is left behind.
    let names: Vec<String> = std::fs::read_dir(pkg.join("Media"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
```
Replace with:
```rust
    // No temp file is left behind (the overview cache folder aside).
    let names: Vec<String> = std::fs::read_dir(pkg.join("Media"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".overviews")
        .collect();
```


In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
    var mediaRows: [MediaRow] = []
    var jobRows: [UInt64: JobRow] = [:]
```
Replace with:
```swift
    /// What `overview` and `regionWaveform` answer (nil: pending silence).
    var waveformRows: OverviewRows?
    /// `regionWaveform` calls: event, from, to, columns.
    var waveformCalls: [(event: UInt64, from: Int64, to: Int64, columns: UInt32)] = []
    func overview(media: UInt64, fromFrame: UInt64, toFrame: UInt64, columns: UInt32) throws -> OverviewRows {
        waveformRows ?? OverviewRows(
            state: .pending, channels: 1, columns: columns, min: Array(repeating: 0, count: Int(columns)),
            max: Array(repeating: 0, count: Int(columns)))
    }
    func regionWaveform(event: UInt64, fromTick: Int64, toTick: Int64, columns: UInt32) throws -> OverviewRows {
        waveformCalls.append((event, fromTick, toTick, columns))
        return try overview(media: 0, fromFrame: 0, toFrame: 1, columns: columns)
    }
    var mediaRows: [MediaRow] = []
    var jobRows: [UInt64: JobRow] = [:]
```

- [ ] **Step 7: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test overviews --test media 2>&1 | grep -E "^test |^test result|panicked"
```
Expected: `overviews` 7 passed; `media` (5a) unchanged.

- [ ] **Step 8: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: all green; Swift 700 tests still pass (the new `IdleUpdate` field has a default).

- [ ] **Step 9: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/overviews.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/session.rs crates/core-ffi/src/media.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/control.rs crates/core-ffi/tests/overviews.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-ffi/tests/media.rs
git commit -m "core-ffi: overviews in the session, waveform reads, the Changed reason

A per-session worker builds, verifies and loads every online item's
overview off the main thread and away from the streamer, and reads exact
sample bins for zoomed-in views. Session.overview and
Session.region_waveform are memory lookups (Pending/Coarse until ready);
region waveforms follow the tempo map, gain and fades. A file whose hash
no longer matches goes offline as Changed until it is restored or
relinked. Save As carries the overview cache.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 5: `core-ffi` — Normalize, fades to the cursor and Remove Fades

**Files:**
- Modify: `crates/core-ffi/src/edit.rs`, `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/overviews.rs` (a test hook), `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Create: `crates/core-ffi/tests/regions.rs`

**Interfaces:**
- Consumes: Task 1's `AudioCmd::{Normalize, FadeTo, RemoveFades}`, `FadeEdge`; Task 2's `Overview::{peak, path, read, has_stamp}`, `exact_peak`, `Stamp`; Task 4's `Inner::overviews`, `Overviews::get`.
- Produces:
  - `AudioEdit::FadeTo { events: Vec<u64>, at: i64, fade_in: bool }` and `AudioEdit::RemoveFades { events: Vec<u64> }` (UniFFI, appended), mapped to the model commands;
  - `Session::normalize_regions(events: Vec<u64>, peak_db: f32) -> Result<DocSnapshot, VizError>` — one undo step "Normalize";
  - test hook `Session::debug_forget_overviews(&self)` (drops every loaded overview; `test-hooks` only) and `Overviews::forget_all(&self)`.
- Swift: `FakeSession.normalizeRegions(events:peakDb:)` with `normalizations`/`normalizeError`.

Ruling 5. The peak is measured from the overview in memory, else from the cached file when its stamp matches the media file (so Normalize works right after an import even before the worker loads it), else the call is `Busy` ("still being built"). The edge reads (at most 255 frames at each end) run after the session lock is released.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/regions.rs <<'VIZDAW_EOF'
//! Region menu commands (plan 5b Task 5, Rulings 5 and 6): Normalize from
//! the overview, fades to the cursor, Remove Fades.

use core_media::caf::{CafWriter, WriteMode};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    AudioEdit, EditCommand, EventKindRow, EventRow, ImportTargetRow, JobStateRow, Session,
    VizError,
};

fn caf(path: &Path, ch: u16, samples: &[f32]) -> PathBuf {
    let mut w = CafWriter::create(path, 48_000, ch, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(samples).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

/// The demo with `src` imported into a new lane at tick 0: `(session,
/// sequence, lane, region)`.
fn demo_with(src: &Path) -> (Arc<Session>, u64, u64, u64) {
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let id = s
        .import_audio(
            vec![src.display().to_string()],
            Some(ImportTargetRow { seq, lane: None, tick: 0 }),
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while s.job(id).unwrap().state == JobStateRow::Running {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    let ev = region(&s, seq, lane).id;
    (s, seq, lane, ev)
}

fn region(s: &Session, seq: u64, lane: u64) -> EventRow {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .find(|e| e.kind == EventKindRow::AudioRegion)
        .unwrap()
}

/// A note of the demo's first lane.
fn a_note(s: &Session, seq: u64) -> u64 {
    let lane = s.lanes(seq).unwrap()[0].id;
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .iter()
        .find(|e| e.kind == EventKindRow::Note)
        .unwrap()
        .id
}

#[test]
fn normalize_reaches_the_peak_from_the_overview() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("quarter.caf"), 2, &vec![0.25; 9600]);
    let (s, seq, lane, ev) = demo_with(&src);
    let note = a_note(&s, seq);
    let snap = s.normalize_regions(vec![note, ev], -1.0).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Normalize"));
    // −1 − 20·log10(0.25) = 11.04 dB.
    let g = region(&s, seq, lane).gain_db;
    assert!((g - 11.0412).abs() < 0.01, "{g}");
    s.undo().unwrap();
    assert_eq!(region(&s, seq, lane).gain_db, 0.0);
    s.close().unwrap();
}

#[test]
fn normalize_measures_only_what_the_region_plays() {
    let dir = tempfile::tempdir().unwrap();
    // 0.1 everywhere, 0.9 at frame 50 (cut off below), 0.5 at frame 2000.
    let mut v = vec![0.1f32; 4800];
    v[50] = 0.9;
    v[2000] = 0.5;
    let src = caf(&dir.path().join("spike.caf"), 1, &v);
    let (s, seq, lane, ev) = demo_with(&src);
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Trim {
                event: ev,
                front_frames: 100,
                back_frames: 0,
            },
        },
        None,
    )
    .unwrap();
    s.normalize_regions(vec![ev], 0.0).unwrap();
    // 0 − 20·log10(0.5) = 6.02 dB: the 0.9 spike is outside the region.
    let g = region(&s, seq, lane).gain_db;
    assert!((g - 6.0206).abs() < 0.01, "{g}");
    s.close().unwrap();
}

#[test]
fn normalize_refuses_bad_targets_offline_media_and_open_edits() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("x.caf"), 2, &vec![0.25; 4800]);
    let (s, seq, _lane, ev) = demo_with(&src);
    let model = |r: Result<vizdaw_ffi::DocSnapshot, VizError>| match r {
        Err(VizError::Model { message }) => message,
        other => panic!("expected a Model error, got {other:?}"),
    };
    for bad in [0.5, -96.5, f32::NAN, f32::NEG_INFINITY] {
        model(s.normalize_regions(vec![ev], bad));
    }
    model(s.normalize_regions(vec![], -1.0));
    model(s.normalize_regions(vec![a_note(&s, seq)], -1.0));
    model(s.normalize_regions(vec![999_999], -1.0));
    s.begin_edit("Drag".into()).unwrap();
    assert!(model(s.normalize_regions(vec![ev], -1.0)).contains("open edit"));
    s.end_edit().unwrap();
    // Offline media cannot be measured.
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let path = s.media().unwrap()[0].path.clone();
    std::fs::remove_file(pkg.join(&path)).unwrap();
    s.recheck_media().unwrap();
    assert!(model(s.normalize_regions(vec![ev], -1.0)).contains("offline"));
    s.close().unwrap();
}

#[test]
fn normalize_is_busy_while_no_overview_can_be_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("y.caf"), 2, &vec![0.25; 4800]);
    let (s, seq, lane, ev) = demo_with(&src);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    // Nothing in memory, nothing on disk, the worker held.
    s.debug_hold_overviews(true);
    s.debug_forget_overviews();
    std::fs::remove_dir_all(pkg.join("Media/.overviews")).unwrap();
    assert!(matches!(
        s.normalize_regions(vec![ev], -1.0),
        Err(VizError::Busy { .. })
    ));
    s.debug_hold_overviews(false);
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        s.idle().unwrap();
        if s.normalize_regions(vec![ev], -1.0).is_ok() {
            break;
        }
        assert!(Instant::now() < until, "the overview was never rebuilt");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!((region(&s, seq, lane).gain_db - 11.0412).abs() < 0.01);
    s.close().unwrap();
}

#[test]
fn fades_follow_the_cursor_and_go() {
    let dir = tempfile::tempdir().unwrap();
    // 2 s of stereo: 96 000 frames.
    let src = caf(&dir.path().join("z.caf"), 2, &vec![0.25; 2 * 96_000]);
    let (s, seq, lane, ev) = demo_with(&src);
    // The cursor half a second into the region.
    let at = s.ticks_at(seq, vec![0.5]).unwrap()[0];
    let frames = (s.seconds_at(seq, vec![at]).unwrap()[0] * 48_000.0).round() as i64;
    let snap = s
        .edit(
            EditCommand::Audio {
                edit: AudioEdit::FadeTo {
                    events: vec![ev],
                    at,
                    fade_in: true,
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Fade In to Cursor"));
    assert_eq!(region(&s, seq, lane).fade_in.length, frames);
    let snap = s
        .edit(
            EditCommand::Audio {
                edit: AudioEdit::FadeTo {
                    events: vec![ev],
                    at,
                    fade_in: false,
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Fade Out from Cursor"));
    let r = region(&s, seq, lane);
    assert_eq!((r.fade_in.length, r.fade_out.length), (frames, 96_000 - frames));
    let snap = s
        .edit(
            EditCommand::Audio {
                edit: AudioEdit::RemoveFades { events: vec![ev] },
            },
            None,
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Remove Fades"));
    let r = region(&s, seq, lane);
    assert_eq!((r.fade_in.length, r.fade_out.length), (0, 0));
    // The cursor outside every selected region: refused.
    assert!(matches!(
        s.edit(
            EditCommand::Audio {
                edit: AudioEdit::FadeTo {
                    events: vec![ev],
                    at: 0,
                    fade_in: true,
                },
            },
            None,
        ),
        Err(VizError::Model { .. })
    ));
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-ffi --test regions 2>&1 | tail -5`
Expected: compile errors (`AudioEdit::FadeTo`, `AudioEdit::RemoveFades`, `Session::normalize_regions`, `debug_forget_overviews` not found).

- [ ] **Step 3: The edit commands**

In `crates/core-ffi/src/edit.rs`:

Find:
```rust
use core_model::command::{AudioCmd, Command, EventCmd, LaneCmd, ReferenceCmd, SequenceCmd};
```
Replace with:
```rust
use core_model::command::{
    AudioCmd, Command, EventCmd, FadeEdge, LaneCmd, ReferenceCmd, SequenceCmd,
};
```

Find:
```rust
    /// Set fades.
    SetFades {
        /// The region.
        event: u64,
        /// Fade in.
        fade_in: FadeRow,
        /// Fade out.
        fade_out: FadeRow,
    },
}
```
Replace with:
```rust
    /// Set fades.
    SetFades {
        /// The region.
        event: u64,
        /// Fade in.
        fade_in: FadeRow,
        /// Fade out.
        fade_out: FadeRow,
    },
    /// Fade In to Cursor (`fade_in`) or Fade Out from Cursor, on every
    /// region `at` lies inside; other events are ignored (plan 5b
    /// Ruling 6).
    FadeTo {
        /// The events.
        events: Vec<u64>,
        /// The cursor tick.
        at: i64,
        /// The fade in (else the fade out).
        fade_in: bool,
    },
    /// Remove Fades from every region; other events are ignored.
    RemoveFades {
        /// The events.
        events: Vec<u64>,
    },
}
```

Find:
```rust
        AudioEdit::SetFades {
            event,
            fade_in,
            fade_out,
        } => AudioCmd::SetFades {
            event: EventId(*event),
            fade_in: fade_of(fade_in),
            fade_out: fade_of(fade_out),
        },
    }
}
```
Replace with:
```rust
        AudioEdit::SetFades {
            event,
            fade_in,
            fade_out,
        } => AudioCmd::SetFades {
            event: EventId(*event),
            fade_in: fade_of(fade_in),
            fade_out: fade_of(fade_out),
        },
        AudioEdit::FadeTo {
            events,
            at,
            fade_in,
        } => AudioCmd::FadeTo {
            events: ids(events),
            at: Tick(*at),
            edge: if *fade_in {
                FadeEdge::In
            } else {
                FadeEdge::Out
            },
        },
        AudioEdit::RemoveFades { events } => AudioCmd::RemoveFades {
            events: ids(events),
        },
    }
}
```

- [ ] **Step 4: `Session::normalize_regions` and the test hook**

In `crates/core-ffi/src/overviews.rs`:

Find:
```rust
    /// Holds the worker before its next request (`true`), or lets it go
    /// (test hook).
```
Replace with:
```rust
    /// Drops every loaded overview and sample window and asks for every
    /// wanted item again (test hook: what a fresh session holds before its
    /// worker ran).
    pub fn forget_all(&self) {
        let mut s = lock(&self.shared.state);
        s.by_hash.clear();
        s.keys.clear();
        s.windows.clear();
        s.handled.clear();
        s.queue.retain(|t| matches!(t, Task::Window(_)));
        let wanted: Vec<Want> = s.wanted.values().cloned().collect();
        for w in wanted {
            s.handled.insert(w.media);
            s.queue.push_back(Task::Ensure(w));
        }
        drop(s);
        self.start();
        self.shared.wake.notify_all();
    }

    /// Holds the worker before its next request (`true`), or lets it go
    /// (test hook).
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// Holds the overview worker (`true`) or lets it go (test hook).
    pub fn debug_hold_overviews(&self, on: bool) {
        lock(&self.inner).overviews.set_held(on);
    }
}
```
Replace with:
```rust
    /// Holds the overview worker (`true`) or lets it go (test hook).
    pub fn debug_hold_overviews(&self, on: bool) {
        lock(&self.inner).overviews.set_held(on);
    }

    /// Forgets every loaded overview (test hook).
    pub fn debug_forget_overviews(&self) {
        lock(&self.inner).overviews.forget_all();
    }
}
```

Find:
```rust
/// Waveform overviews (plan 5b Task 4).
#[uniffi::export]
impl Session {
```
Replace with:
```rust
/// Region menu commands that need the overviews (plan 5b Task 5).
#[uniffi::export]
impl Session {
    /// Normalize… (spec §4.4; plan 5b Ruling 5): sets each region among
    /// `events` so its loudest sample reaches `peak_db` (−96…0 dBFS), as
    /// one undo step "Normalize". Events that are not regions are ignored.
    /// `Model` error for a bad target, no region, offline media or an open
    /// edit; `Busy` while a region's overview is still being built.
    pub fn normalize_regions(&self, events: Vec<u64>, peak_db: f32) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            use core_media::overview::{Overview, Stamp, exact_peak};
            use core_model::event::EventBody;
            if !(peak_db.is_finite() && (-96.0..=0.0).contains(&peak_db)) {
                return Err(crate::events::bad("the peak must be a level from -96 to 0 dB"));
            }
            // Under the lock: what to measure, and each overview.
            let mut measure = Vec::new();
            {
                let mut inner = lock(&self.inner);
                if inner.edit_label.is_some() {
                    return Err(crate::events::bad("finish the open edit first"));
                }
                inner.refresh_media(self.id, false);
                let root = inner.root(self.id);
                let doc = inner.doc().clone();
                for e in &events {
                    let id = core_model::ids::EventId(*e);
                    let (ev, _) = doc.event(id)?;
                    let EventBody::AudioRegion {
                        media,
                        source_offset,
                        length,
                        ..
                    } = ev.body
                    else {
                        continue;
                    };
                    let m = doc
                        .media()
                        .get(&media)
                        .ok_or_else(|| crate::events::bad(&format!("unknown media {media}")))?;
                    if inner.offline.contains_key(&media) {
                        return Err(crate::events::bad(&format!(
                            "{} is offline: relink it first",
                            m.name
                        )));
                    }
                    let file = root.join(&m.path);
                    // In memory, else the cached file when it is this file's.
                    let ov = inner.overviews.get(media).or_else(|| {
                        let h = m.content_hash.as_deref()?;
                        let o = Overview::read(&Overview::path(&root, h)?).ok()?;
                        let st = Stamp::of(&file).ok()?;
                        (o.hash == h && o.has_stamp(&st)).then(|| std::sync::Arc::new(o))
                    });
                    let Some(ov) = ov else {
                        return Err(VizError::Busy {
                            message: format!(
                                "The waveform of {} is still being built; try again in a moment.",
                                m.name
                            ),
                        });
                    };
                    measure.push((id, ov, file, source_offset, source_offset.saturating_add(length)));
                }
            }
            if measure.is_empty() {
                return Err(crate::events::bad("select audio regions to normalize"));
            }
            // The edge samples, read without the lock.
            let io = |e: core_media::MediaError| VizError::Io {
                message: e.to_string(),
            };
            let mut regions = Vec::with_capacity(measure.len());
            for (id, ov, file, from, to) in measure {
                let r = core_media::caf::CafReader::open(&file).map_err(io)?;
                let mut exact = |a: u64, b: u64| exact_peak(&r, a, b);
                regions.push((id, ov.peak(from, to, &mut exact).map_err(io)?));
            }
            self.execute(Command::Audio(core_model::command::AudioCmd::Normalize {
                regions,
                peak_db,
            }))
        })
    }
}

/// Waveform overviews (plan 5b Task 4).
#[uniffi::export]
impl Session {
```

`Session::execute` checks nothing about open edits itself; the check above runs under the same lock acquisition that reads the document, and `History::execute` refuses inside a transaction anyway.

- [ ] **Step 5: `FakeSession`**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
        return try overview(media: 0, fromFrame: 0, toFrame: 1, columns: columns)
    }
```
Replace with:
```swift
        return try overview(media: 0, fromFrame: 0, toFrame: 1, columns: columns)
    }
    /// `normalizeRegions` calls; `normalizeError` is thrown instead.
    var normalizations: [(events: [UInt64], peakDb: Float)] = []
    var normalizeError: VizError?
    func normalizeRegions(events: [UInt64], peakDb: Float) throws -> DocSnapshot {
        if let e = normalizeError { throw e }
        normalizations.append((events, peakDb))
        return commandResult()
    }
```

- [ ] **Step 6: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test regions 2>&1 | grep -E "^test |^test result|panicked"
```
Expected: 5 passed.

- [ ] **Step 7: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: all green.

- [ ] **Step 8: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/edit.rs crates/core-ffi/src/session.rs crates/core-ffi/src/overviews.rs crates/core-ffi/tests/regions.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: Normalize from the overview, fades to the cursor, Remove Fades

Session.normalize_regions measures each region's peak from its overview
(whole blocks) and its edge samples, then applies AudioCmd::Normalize as
one undo step; offline media, open edits and bad targets are refused,
an overview still being built is Busy. AudioEdit gains FadeTo and
RemoveFades.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 6: `core-ffi` — media pool operations, relinking, and reserved names for every job

**Files:**
- Create: `crates/core-ffi/src/relink.rs`, `crates/core-ffi/tests/media_pool.rs`
- Modify: `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/jobs.rs`, `crates/core-ffi/src/media.rs`, `crates/core-ffi/src/session.rs`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`, `crates/core-ffi/tests/control.rs` (a `MediaRow` literal)
- Test: `crates/core-ffi/tests/media_pool.rs`

**Interfaces:**
- Consumes: Task 1's `MediaCmd::Relink` (rescaling); Task 3's `import_reserved`; 5a's `MediaCmd::{Rename, Remove, Compact}`, `media::regions_using`, `Jobs::spawn`, `apply_jobs`, `discard`.
- Produces:
  - UniFFI: `MediaRow.used_percent: f32` (`#[uniffi(default = 0.0)]`, last); `JobKindRow::Relink` (appended); `MediaUseRow { seq: u64, lane: u64, event: u64, tick: i64, sequence: String, lane_name: String }`;
  - `Session::rename_media(media: u64, name: String) -> Result<DocSnapshot, VizError>` ("Rename Media");
  - `Session::remove_media(media: u64) -> Result<DocSnapshot, VizError>` (its regions and the item, one step "Remove Media");
  - `Session::compact_media() -> Result<DocSnapshot, VizError>` ("Compact Media");
  - `Session::media_users(media: u64) -> Result<Vec<MediaUseRow>, VizError>`;
  - `Session::media_file(media: u64) -> Result<String, VizError>` (absolute path);
  - `Session::relink_media(media: u64, path: String) -> Result<u64, VizError>` and `Session::search_media(folder: String) -> Result<u64, VizError>` (jobs of kind `Relink`);
  - Internal: `jobs::{Work::Relink, RelinkFile}`; `relink::{relink_file, search, Lost}`; `Inner::reserved_names() -> BTreeSet<String>`; import, convert and format jobs pass it to `import_reserved` (Ruling 12).
- Swift: `FakeSession` implements the seven new methods, recording `renamed`, `removedMedia`, `compactions`, `relinks`, `searches`; `usersTable`.

Rulings 9, 10, 12 and 13. A relink job writes a new file (never into an existing one) unless the chosen file already is a VizDAW CAF in the project format directly in this package's `Media/`; such an in-place file is never removed by a failed or cancelled relink (`RelinkFile.made` is false). Applying a relink records it as `MediaCmd::Relink`; the path changes, so the engine's `Streamer::set_media` restarts the item on the new file (5a final review I2).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/media_pool.rs <<'VIZDAW_EOF'
//! The media pool (plan 5b Task 6, Rulings 9, 10, 12, 13): rows, rename,
//! remove, compact, users, relinking by file and by folder, and names no
//! job may take.

use core_media::caf::{CafWriter, WriteMode, hash_file};
use core_model::command::{Command, MediaCmd, apply};
use core_model::document::Document;
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{
    AudioEdit, EditCommand, EventKindRow, ImportTargetRow, JobKindRow, JobRow, JobStateRow,
    OfflineReasonRow, Session, VizError,
};

/// A CAF at `rate` in `depth`, every sample `value`.
fn caf(path: &Path, rate: u32, ch: u16, frames: usize, depth: BitDepth, value: f32) -> PathBuf {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut w = CafWriter::create(path, rate, ch, depth, WriteMode::Normal).unwrap();
    w.write(&vec![value; frames * usize::from(ch)]).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

/// A 16-bit PCM WAV of silence at 48 kHz.
fn wav(path: &Path, channels: u16, frames: u32) {
    let data = frames * u32::from(channels) * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&(48_000 * u32::from(channels) * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 0);
    std::fs::write(path, b).unwrap();
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

fn import(s: &Arc<Session>, path: &Path, target: Option<ImportTargetRow>) {
    let j = wait(s, s.import_audio(vec![path.display().to_string()], target).unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
}

/// The demo with `src` imported into a new lane at tick 0, saved as
/// `P.vizdaw` in `dir`: `(session, package, media id)`.
fn saved_demo_with(dir: &Path, src: &Path) -> (Arc<Session>, PathBuf, u64) {
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    import(&s, src, Some(ImportTargetRow { seq, lane: None, tick: 0 }));
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let id = s.media().unwrap()[0].id;
    (s, pkg, id)
}

fn row(s: &Session, id: u64) -> vizdaw_ffi::MediaRow {
    s.media().unwrap().into_iter().find(|r| r.id == id).unwrap()
}

/// Regular files directly in `Media/` (the hidden cache folder aside).
fn media_files(pkg: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(pkg.join("Media"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    v.sort();
    v
}

#[test]
fn pool_rows_rename_remove_and_compact() {
    let dir = tempfile::tempdir().unwrap();
    let used = caf(&dir.path().join("used.caf"), 48_000, 2, 4800, BitDepth::Float32, 0.25);
    let spare = caf(&dir.path().join("spare.caf"), 48_000, 1, 100, BitDepth::Float32, 0.25);
    let s = Session::new_demo().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    import(&s, &used, Some(ImportTargetRow { seq, lane: None, tick: 0 }));
    import(&s, &spare, None);
    let rows = s.media().unwrap();
    let (u, p) = (rows[0].clone(), rows[1].clone());
    assert_eq!((u.name.as_str(), u.uses, u.used_percent), ("used", 1, 100.0));
    assert_eq!((p.name.as_str(), p.uses, p.used_percent), ("spare", 0, 0.0));
    // Trimming 1200 of 4800 frames off the front: 75 % is used.
    let lane = s.lanes(seq).unwrap().last().unwrap().id;
    let ev = s.lane_events(seq, lane).unwrap().events[0].id;
    s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Trim {
                event: ev,
                front_frames: 1200,
                back_frames: 0,
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(row(&s, u.id).used_percent, 75.0);
    // Users, with the names the delete confirmation lists.
    let users = s.media_users(u.id).unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!((users[0].seq, users[0].lane, users[0].event), (seq, lane, ev));
    assert_eq!(users[0].lane_name, s.lanes(seq).unwrap().last().unwrap().name);
    assert!(s.media_users(p.id).unwrap().is_empty());
    assert!(Path::new(&s.media_file(u.id).unwrap()).is_file());
    // Rename.
    let snap = s.rename_media(u.id, "  Drums  ".into()).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Rename Media"));
    assert_eq!(row(&s, u.id).name, "Drums");
    assert!(matches!(s.rename_media(u.id, "   ".into()), Err(VizError::Model { .. })));
    assert!(matches!(s.rename_media(999, "x".into()), Err(VizError::Model { .. })));
    // Compact removes the unused item only.
    let snap = s.compact_media().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Compact Media"));
    assert_eq!(s.media().unwrap().iter().map(|r| r.id).collect::<Vec<_>>(), vec![u.id]);
    // Removing an item in use removes its regions too, as one step.
    let snap = s.remove_media(u.id).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Remove Media"));
    assert!(s.media().unwrap().is_empty());
    assert!(s.lane_events(seq, lane).unwrap().events.is_empty());
    s.undo().unwrap();
    assert_eq!(s.media().unwrap().len(), 1);
    assert_eq!(s.lane_events(seq, lane).unwrap().events.len(), 1);
    // Not while an edit is open.
    s.begin_edit("Drag".into()).unwrap();
    assert!(matches!(s.compact_media(), Err(VizError::Model { .. })));
    s.end_edit().unwrap();
    s.close().unwrap();
}

#[test]
fn relink_copies_a_file_into_the_package_as_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("x.caf"), 48_000, 2, 4800, BitDepth::Float32, 0.25);
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    let path = row(&s, id).path;
    let moved = dir.path().join("elsewhere/x.caf");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(pkg.join(&path), &moved).unwrap();
    s.recheck_media().unwrap();
    assert_eq!(row(&s, id).offline, Some(OfflineReasonRow::Missing));
    let j = wait(&s, s.relink_media(id, moved.display().to_string()).unwrap());
    assert_eq!((j.kind, j.state, j.created.clone()), (JobKindRow::Relink, JobStateRow::Done, vec![id]), "{j:?}");
    let r = row(&s, id);
    // `x.caf` is still named by the undo history: the copy takes `x 2`.
    assert_eq!((r.path.as_str(), r.offline), ("Media/x 2.caf", None));
    assert_eq!(hash_file(&pkg.join(&r.path)).unwrap(), hash_file(&moved).unwrap());
    assert_eq!(s.snapshot().unwrap().undo_label.as_deref(), Some("Relink Media"));
    s.undo().unwrap();
    assert_eq!(row(&s, id).path, path);
    s.close().unwrap();
}

/// Review Focus 4.
#[test]
fn relink_uses_a_file_already_in_the_package() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("y.caf"), 48_000, 2, 4800, BitDepth::Float32, 0.25);
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    let path = row(&s, id).path;
    std::fs::rename(pkg.join(&path), pkg.join("Media/renamed.caf")).unwrap();
    s.recheck_media().unwrap();
    let j = wait(
        &s,
        s.relink_media(id, pkg.join("Media/renamed.caf").display().to_string())
            .unwrap(),
    );
    assert_eq!(j.created, vec![id], "{j:?}");
    assert_eq!(row(&s, id).path, "Media/renamed.caf");
    assert_eq!(media_files(&pkg), vec!["renamed.caf".to_string()], "no copy");
    s.close().unwrap();
}

/// Review Focus 4.
#[test]
fn relink_refuses_short_and_wide_files_and_converts_rates() {
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("z.caf"), 48_000, 2, 4800, BitDepth::Float32, 0.25);
    let (s, pkg, id) = saved_demo_with(dir.path(), &src);
    let before = media_files(&pkg);
    // Shorter than the region (4800 frames): refused, its copy removed.
    let short = caf(&dir.path().join("short.caf"), 48_000, 2, 1000, BitDepth::Float32, 0.25);
    let j = wait(&s, s.relink_media(id, short.display().to_string()).unwrap());
    assert!(j.created.is_empty() && j.problems.len() == 1, "{j:?}");
    assert!(j.problems[0].contains("shorter"), "{j:?}");
    assert_eq!(media_files(&pkg), before);
    // Four channels, and not audio at all: problems, nothing written.
    let wide = dir.path().join("wide.wav");
    wav(&wide, 4, 9600);
    let j = wait(&s, s.relink_media(id, wide.display().to_string()).unwrap());
    assert!(j.created.is_empty() && j.problems[0].contains("two channels"), "{j:?}");
    let text = dir.path().join("notes.wav");
    std::fs::write(&text, "not audio").unwrap();
    let j = wait(&s, s.relink_media(id, text.display().to_string()).unwrap());
    assert!(j.created.is_empty() && !j.problems.is_empty(), "{j:?}");
    assert_eq!(media_files(&pkg), before);
    // Another rate: converted to 48 kHz, long enough, relinked.
    let other = caf(&dir.path().join("other.caf"), 44_100, 2, 8820, BitDepth::Int16, 0.25);
    let j = wait(&s, s.relink_media(id, other.display().to_string()).unwrap());
    assert_eq!(j.created, vec![id], "{j:?}");
    let r = row(&s, id);
    assert_eq!((r.sample_rate, r.offline), (48_000, None));
    assert!(r.frames.abs_diff(9600) <= 1, "{r:?}");
    // Hostile arguments.
    assert!(matches!(s.relink_media(999, other.display().to_string()), Err(VizError::Model { .. })));
    assert!(matches!(s.relink_media(id, "relative.caf".into()), Err(VizError::Model { .. })));
    assert!(matches!(s.relink_media(id, String::new()), Err(VizError::Model { .. })));
    s.close().unwrap();
}

/// Review Focus 4.
#[test]
fn search_prefers_the_hash_and_reports_what_it_misses() {
    let dir = tempfile::tempdir().unwrap();
    let a = caf(&dir.path().join("in/a.caf"), 48_000, 2, 4800, BitDepth::Float32, 0.25);
    let b = caf(&dir.path().join("in/b.caf"), 48_000, 2, 4800, BitDepth::Float32, 0.5);
    let s = Session::new_empty().unwrap();
    import(&s, &a, None);
    import(&s, &b, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    let rows = s.media().unwrap();
    let (ia, ib) = (rows[0].clone(), rows[1].clone());
    let original = hash_file(&pkg.join(&ia.path)).unwrap();
    // The folder: a's audio under another name (deeper), a file named like
    // a with other audio (found first by name), nothing for b.
    let folder = dir.path().join("found");
    std::fs::create_dir_all(folder.join("zz")).unwrap();
    std::fs::rename(pkg.join(&ia.path), folder.join("zz/renamed.caf")).unwrap();
    caf(&folder.join("a/a.caf"), 48_000, 2, 4800, BitDepth::Int24, 0.75);
    std::fs::remove_file(pkg.join(&ib.path)).unwrap();
    s.recheck_media().unwrap();
    let undo_depth_before = s.snapshot().unwrap().undo_label.clone();
    let j = wait(&s, s.search_media(folder.display().to_string()).unwrap());
    assert_eq!((j.kind, j.created.clone()), (JobKindRow::Relink, vec![ia.id]), "{j:?}");
    assert_eq!(j.problems.len(), 1, "{j:?}");
    assert!(j.problems[0].starts_with("b: not found in"), "{j:?}");
    let r = row(&s, ia.id);
    assert_eq!(r.offline, None);
    assert_eq!(hash_file(&pkg.join(&r.path)).unwrap(), original, "the hash match won");
    assert_eq!(row(&s, ib.id).offline, Some(OfflineReasonRow::Missing));
    // One relink, one undo step.
    s.undo().unwrap();
    assert_eq!(s.snapshot().unwrap().undo_label, undo_depth_before);
    // A folder that does not exist: the item is not found.
    let j = wait(&s, s.search_media(dir.path().join("nowhere").display().to_string()).unwrap());
    assert!(j.created.is_empty() && j.problems.len() == 2, "{j:?}");
    assert!(matches!(s.search_media("relative".into()), Err(VizError::Model { .. })));
    s.close().unwrap();
}

/// Final review r2: a path the history names is never given to a new file.
#[test]
fn an_import_never_takes_a_name_the_history_references() {
    let dir = tempfile::tempdir().unwrap();
    let a = caf(&dir.path().join("a.caf"), 48_000, 2, 100, BitDepth::Float32, 0.25);
    let s = Session::new_empty().unwrap();
    import(&s, &a, None);
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    assert_eq!(s.media().unwrap()[0].path, "Media/a.caf");
    // The file goes missing; the item still names it.
    std::fs::remove_file(pkg.join("Media/a.caf")).unwrap();
    import(&s, &a, None);
    let paths: Vec<String> = s.media().unwrap().into_iter().map(|r| r.path).collect();
    assert_eq!(paths, vec!["Media/a.caf".to_string(), "Media/a 2.caf".to_string()]);
    // Only the redo history names it now: still reserved.
    s.undo().unwrap();
    s.undo().unwrap();
    std::fs::remove_file(pkg.join("Media/a 2.caf")).unwrap();
    let c = caf(&dir.path().join("c/a.caf"), 48_000, 2, 100, BitDepth::Float32, 0.5);
    import(&s, &c, None);
    assert_eq!(s.media().unwrap()[0].path, "Media/a 3.caf");
    s.close().unwrap();
}

/// 5a Task 11 review Minor 6: converting an item never claims the name of
/// a missing one.
#[test]
fn a_conversion_never_claims_a_missing_items_name() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    let add = |d: &Document, path: &str, rate: u32| {
        apply(
            d,
            &Command::Media(MediaCmd::Add {
                path: path.into(),
                sample_rate: rate,
                channels: 2,
                length_frames: 1000,
                content_hash: None,
                name: None,
                origin: None,
            }),
        )
        .unwrap()
        .doc
    };
    let d = add(&Document::new(), "Media/two.caf", 48_000);
    let d = add(&d, "Media/Other/two.caf", 44_100);
    core_model::file::save(&d, &pkg).unwrap();
    // `Media/two.caf` is missing; `Media/Other/two.caf` is at 44.1 kHz.
    caf(&pkg.join("Media/Other/two.caf"), 44_100, 2, 1000, BitDepth::Int24, 0.25);
    let s = Session::open(pkg.display().to_string()).unwrap();
    let j = wait(&s, s.convert_media().unwrap());
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    let rows = s.media().unwrap();
    assert_eq!(rows[0].path, "Media/two.caf");
    assert_eq!(rows[0].offline, Some(OfflineReasonRow::Missing));
    assert_eq!(rows[1].path, "Media/two 2.caf", "not the missing item's name");
    assert_eq!(rows[1].offline, None);
    s.close().unwrap();
}

#[test]
fn the_region_kinds_stay_put() {
    // Sanity for the helpers above: a demo import makes one region.
    let dir = tempfile::tempdir().unwrap();
    let src = caf(&dir.path().join("k.caf"), 48_000, 1, 480, BitDepth::Float32, 0.25);
    let (s, _pkg, id) = saved_demo_with(dir.path(), &src);
    let u = s.media_users(id).unwrap();
    let seq = u[0].seq;
    let kinds: Vec<EventKindRow> = s
        .lane_events(seq, u[0].lane)
        .unwrap()
        .events
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, vec![EventKindRow::AudioRegion]);
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-ffi --test media_pool 2>&1 | tail -5`
Expected: compile errors (`used_percent`, `media_users`, `rename_media`, `relink_media`, `search_media`, `JobKindRow::Relink` not found).

- [ ] **Step 3: Rows, job kind and relink work**

In `crates/core-ffi/src/media.rs`:

Find:
```rust
    /// Regions using it (any take).
    pub uses: u32,
}
```
Replace with:
```rust
    /// Regions using it (any take).
    pub uses: u32,
    /// How much of its audio regions play, 0–100 (plan 5b Ruling 13).
    #[uniffi(default = 0.0)]
    pub used_percent: f32,
}

/// One region using a media item (the pool's "select regions" and the
/// delete confirmation; plan 5b Ruling 13).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MediaUseRow {
    /// Sequence.
    pub seq: u64,
    /// Lane.
    pub lane: u64,
    /// The region.
    pub event: u64,
    /// Its start tick.
    pub tick: i64,
    /// The sequence's name.
    pub sequence: String,
    /// The lane's name.
    pub lane_name: String,
}

/// The regions using `media`, in sequence, lane and time order.
pub fn users(doc: &Document, media: MediaId) -> Vec<MediaUseRow> {
    let mut out: Vec<MediaUseRow> = doc
        .all_events()
        .filter(|(_, e)| matches!(e.body, EventBody::AudioRegion { media: m, .. } if m == media))
        .map(|(loc, e)| {
            let seq = doc.sequence(loc.seq).ok();
            MediaUseRow {
                seq: loc.seq.0,
                lane: loc.lane.0,
                event: e.id.0,
                tick: e.tick.0,
                sequence: seq.map(|s| s.name.clone()).unwrap_or_default(),
                lane_name: doc
                    .lane(loc.seq, loc.lane)
                    .map(|l| l.name.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();
    out.sort_by_key(|u| (u.seq, u.lane, u.tick, u.event));
    out
}

/// The share of `len` frames the spans cover, 0–100.
fn used_percent(spans: Option<&Vec<(u64, u64)>>, len: u64) -> f32 {
    let Some(spans) = spans else { return 0.0 };
    if len == 0 {
        return 0.0;
    }
    let mut v = spans.clone();
    v.sort_unstable();
    let (mut total, mut end) = (0u64, 0u64);
    for (a, b) in v {
        let (a, b) = (a.min(len), b.min(len));
        if b <= end {
            continue;
        }
        total += b - a.max(end);
        end = b;
    }
    (total as f64 / len as f64 * 100.0) as f32
}
```

Find:
```rust
    let mut uses: BTreeMap<MediaId, u32> = BTreeMap::new();
    for (_, e) in doc.all_events() {
        if let EventBody::AudioRegion { media, .. } = e.body {
            *uses.entry(media).or_default() += 1;
        }
    }
```
Replace with:
```rust
    let mut uses: BTreeMap<MediaId, u32> = BTreeMap::new();
    let mut spans: BTreeMap<MediaId, Vec<(u64, u64)>> = BTreeMap::new();
    for (_, e) in doc.all_events() {
        if let EventBody::AudioRegion {
            media,
            source_offset,
            length,
            ..
        } = e.body
        {
            *uses.entry(media).or_default() += 1;
            spans
                .entry(media)
                .or_default()
                .push((source_offset, source_offset.saturating_add(length)));
        }
    }
```

Find:
```rust
                offline: offline.get(&m.id).copied(),
                uses: uses.get(&m.id).copied().unwrap_or(0),
```
Replace with:
```rust
                offline: offline.get(&m.id).copied(),
                uses: uses.get(&m.id).copied().unwrap_or(0),
                used_percent: used_percent(spans.get(&m.id), m.length_frames),
```

A 5a test builds a `MediaRow` literal. In `crates/core-ffi/tests/control.rs`:

Find:
```rust
            offline: Some(OfflineReasonRow::Format),
            uses: 3,
        }],
```
Replace with:
```rust
            offline: Some(OfflineReasonRow::Format),
            uses: 3,
            used_percent: 40.0,
        }],
```

In `crates/core-ffi/src/jobs.rs`:

Find:
```rust
    /// Change the project format.
    Format,
}
```
Replace with:
```rust
    /// Change the project format.
    Format,
    /// Relink media (a chosen file, or a folder search; plan 5b).
    Relink,
}
```

Find:
```rust
    /// Cancelled before it finished (whatever it wrote is removed).
    Cancelled,
}
```
Replace with:
```rust
    /// Relink targets per media item (plan 5b Rulings 9, 10).
    Relink {
        /// `(item, its new file or why not)`.
        files: Vec<(core_model::ids::MediaId, Result<RelinkFile, String>)>,
    },
    /// Cancelled before it finished (whatever it wrote is removed).
    Cancelled,
}

/// A file in `Media/` a media item is to be relinked to.
#[derive(Debug)]
pub struct RelinkFile {
    /// The file.
    pub file: Imported,
    /// The job wrote it (else it was already in `Media/`, and it is never
    /// removed when the relink is not applied).
    pub made: bool,
}
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/relink.rs <<'VIZDAW_EOF'
//! Relinking (sub-project 5 spec §10.3; plan 5b Rulings 9, 10, 12): the
//! job bodies for "Locate…" (one chosen file) and "Search folder…" (every
//! missing or changed item, by content hash, then name and duration).
//! Run on job threads; nothing here takes the session lock.

use crate::jobs::{RelinkFile, Work};
use core_media::MediaError;
use core_media::caf::{CafReader, hash_file};
use core_media::import::{Imported, MediaImporter, import_reserved};
use core_model::document::MediaItem;
use core_model::format::AudioFormat;
use core_model::ids::MediaId;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Folder levels a search descends.
const DEPTH: u32 = 8;
/// Files a search looks at.
const MAX_FILES: usize = 20_000;

/// An item a search looks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Lost {
    /// The item.
    pub media: MediaId,
    /// Its display name.
    pub name: String,
    /// Its file's stem.
    pub stem: String,
    /// Its content hash.
    pub hash: Option<String>,
    /// Its duration in seconds.
    pub seconds: f64,
}

/// `Media/<name>` when `src` is a file directly in `root`'s `Media/` (not
/// hidden).
fn in_media(root: &Path, src: &Path) -> Option<String> {
    let media = std::fs::canonicalize(root.join("Media")).ok()?;
    let f = std::fs::canonicalize(src).ok()?;
    let name = f.file_name()?.to_str()?.to_string();
    (f.parent()? == media && !name.starts_with('.')).then(|| format!("Media/{name}"))
}

/// The file in `root`'s `Media/` to relink to `src`: `src` itself when it
/// is a VizDAW CAF in `format` directly in `Media/`, else a new file
/// imported (converted) from it under a free, unreserved name.
pub fn relink_file(
    importer: &dyn MediaImporter,
    root: &Path,
    src: &Path,
    format: AudioFormat,
    reserved: &BTreeSet<String>,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<RelinkFile, MediaError> {
    if let Some(rel) = in_media(root, src) {
        if let Ok(r) = CafReader::open(src) {
            let i = r.info().clone();
            if i.sample_rate == format.sample_rate && i.depth == format.depth {
                let (file, stem) = MediaItem::file_parts(&rel);
                return Ok(RelinkFile {
                    file: Imported {
                        path: rel,
                        name: stem,
                        original_name: file,
                        channels: i.channels,
                        sample_rate: i.sample_rate,
                        length_frames: i.frames,
                        content_hash: hash_file(src)?,
                        clipped: 0,
                    },
                    made: false,
                });
            }
        }
    }
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut made = import_reserved(importer, src, root, format, &stem, reserved, progress, cancel)?;
    if made.len() != 1 {
        for i in &made {
            let _ = std::fs::remove_file(root.join(&i.path));
        }
        return Err(MediaError::Unsupported(
            "the file has more than two channels; relink each channel's item to a mono file".into(),
        ));
    }
    Ok(RelinkFile {
        file: made.remove(0),
        made: true,
    })
}

/// Files under `dir`, depth first in name order, at most `DEPTH` levels
/// and `MAX_FILES` files; hidden entries and symlinks are skipped.
fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth == 0 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<std::fs::DirEntry> = rd.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        if out.len() >= MAX_FILES {
            return;
        }
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            walk(&e.path(), depth - 1, out);
        } else if t.is_file() {
            out.push(e.path());
        }
    }
}

/// The "Search folder…" job (Ruling 10): matches `lost` items to files in
/// `folder` by content hash, then by stem or name with a duration within
/// 1 ms, and relinks each match; an item without one is a problem.
#[allow(clippy::too_many_arguments)]
pub fn search(
    importer: &dyn MediaImporter,
    root: &Path,
    folder: &Path,
    lost: Vec<Lost>,
    format: AudioFormat,
    reserved: &BTreeSet<String>,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Work {
    let mut files = Vec::new();
    walk(folder, DEPTH, &mut files);
    let mut found: Vec<Option<PathBuf>> = vec![None; lost.len()];
    // By content hash: only VizDAW CAF files have one to compare.
    for f in &files {
        if cancel.load(Ordering::Relaxed) {
            return Work::Cancelled;
        }
        if found.iter().all(Option::is_some) {
            break;
        }
        if !f.extension().is_some_and(|e| e.eq_ignore_ascii_case("caf")) {
            continue;
        }
        let Ok(h) = hash_file(f) else { continue };
        for (k, l) in lost.iter().enumerate() {
            if found.get(k).is_some_and(Option::is_none) && l.hash.as_deref() == Some(h.as_str()) {
                if let Some(slot) = found.get_mut(k) {
                    *slot = Some(f.clone());
                }
                break;
            }
        }
    }
    // By name and duration.
    for f in &files {
        if found.iter().all(Option::is_some) {
            break;
        }
        let stem = f
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (k, l) in lost.iter().enumerate() {
            if !found.get(k).is_some_and(Option::is_none) || (stem != l.stem && stem != l.name) {
                continue;
            }
            let Ok(info) = importer.probe(f) else { continue };
            let secs = info.frames as f64 / info.sample_rate.max(1.0);
            if (secs - l.seconds).abs() <= 0.001 {
                if let Some(slot) = found.get_mut(k) {
                    *slot = Some(f.clone());
                }
                break;
            }
        }
    }
    let n = found.iter().flatten().count().max(1) as f32;
    let mut done = 0f32;
    let mut out = Vec::with_capacity(lost.len());
    for (l, f) in lost.iter().zip(found) {
        let Some(f) = f else {
            out.push((
                l.media,
                Err(format!("{}: not found in {}", l.name, folder.display())),
            ));
            continue;
        };
        let base = done;
        let r = relink_file(
            importer,
            root,
            &f,
            format,
            reserved,
            &mut |p| progress.store(((base + p) / n).to_bits(), Ordering::Relaxed),
            cancel,
        );
        done += 1.0;
        match r {
            Err(MediaError::Cancelled) => {
                crate::session::discard(root, Work::Relink { files: out });
                return Work::Cancelled;
            }
            r => out.push((l.media, r.map_err(|e| format!("{}: {e}", l.name)))),
        }
    }
    Work::Relink { files: out }
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod recovery;
pub mod revs;
```
Replace with:
```rust
pub mod recovery;
pub mod relink;
pub mod revs;
```

Find:
```rust
pub use media::{MediaOriginRow, MediaRow, OfflineReasonRow};
```
Replace with:
```rust
pub use media::{MediaOriginRow, MediaRow, MediaUseRow, OfflineReasonRow};
```

- [ ] **Step 4: The session**

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// The media root: the package, or the untitled scratch package.
    fn root(&self, id: u64) -> std::path::PathBuf {
```
Replace with:
```rust
    /// Every package-relative path a history state references: no job
    /// may give a new file one of these names, even when the file is
    /// missing (plan 5b Ruling 12; final review r2).
    fn reserved_names(&self) -> std::collections::BTreeSet<String> {
        self.history.referenced_media_anywhere().into_keys().collect()
    }

    /// The media root: the package, or the untitled scratch package.
    fn root(&self, id: u64) -> std::path::PathBuf {
```

The discard path learns relink work.

Find:
```rust
        Work::Convert { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .map(|i| i.path)
            .collect(),
        Work::Cancelled => Vec::new(),
```
Replace with:
```rust
        Work::Convert { files, .. } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .map(|i| i.path)
            .collect(),
        // Only files the job wrote; a file it found in `Media/` stays.
        Work::Relink { files } => files
            .into_iter()
            .filter_map(|(_, r)| r.ok())
            .filter(|r| r.made)
            .map(|r| r.file.path)
            .collect(),
        Work::Cancelled => Vec::new(),
```

Find:
```rust
                Work::Convert { format, files, .. } => {
                    let was_format = format.is_some();
```
Replace with:
```rust
                Work::Relink { files } => {
                    let (done, problems) = apply_relink(inner, &root, files);
                    changed |= !done.is_empty();
                    row(id, kind, JobStateRow::Done, None, problems, done)
                }
                Work::Convert { format, files, .. } => {
                    let was_format = format.is_some();
```

Find:
```rust
/// `(media, absolute file, stem)` per item to convert.
type ConvertList = Vec<(core_model::ids::MediaId, PathBuf, String)>;
```
Replace with:
```rust
/// Applies a finished relink job: one undo step "Relink Media" per item
/// (Ruling 9). A refused relink (the file is too short) removes the file
/// the job made and becomes a problem. Returns the relinked items and
/// the problems.
fn apply_relink(
    inner: &mut Inner,
    root: &Path,
    files: Vec<(core_model::ids::MediaId, Result<crate::jobs::RelinkFile, String>)>,
) -> (Vec<u64>, Vec<String>) {
    use core_model::command::MediaCmd;
    let mut done = Vec::new();
    let mut problems = Vec::new();
    for (m, r) in files {
        let f = match r {
            Ok(f) => f,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        let name = inner
            .doc()
            .media()
            .get(&m)
            .map_or_else(|| m.to_string(), |x| x.name.clone());
        let cmd = Command::Media(MediaCmd::Relink {
            media: m,
            path: f.file.path.clone(),
            content_hash: Some(f.file.content_hash.clone()),
            length_frames: f.file.length_frames,
            channels: f.file.channels,
            sample_rate: f.file.sample_rate,
        });
        match inner.history.execute(&cmd, &ViewFilter::default()) {
            Ok(_) => {
                inner.changed.remove(&m);
                done.push(m.0);
            }
            Err(e) => {
                if f.made {
                    let _ = std::fs::remove_file(root.join(&f.file.path));
                }
                let why = match e {
                    core_model::ModelError::OutOfRange {
                        field: "relink.length_frames",
                        ..
                    } => {
                        "the file is shorter than the regions using this item".to_string()
                    }
                    other => other.to_string(),
                };
                problems.push(format!("{name}: {why}"));
            }
        }
    }
    (done, problems)
}

/// `(media, absolute file, stem)` per item to convert.
type ConvertList = Vec<(core_model::ids::MediaId, PathBuf, String)>;
```

The convert job reserves the history's names.

Find:
```rust
fn convert_files(
    root: &Path,
    items: ConvertList,
    format: core_model::format::AudioFormat,
    progress: &std::sync::atomic::AtomicU32,
    cancel: &std::sync::atomic::AtomicBool,
    kind: crate::jobs::JobKindRow,
) -> crate::jobs::Work {
    let importer = crate::media::importer();
    let n = items.len().max(1) as f32;
    let mut files: ConvertFiles = Vec::new();
    for (k, (media, path, stem)) in items.into_iter().enumerate() {
        let r = core_media::import::import_as(
            importer.as_ref(),
            &path,
            root,
            format,
            &stem,
            &mut |f| {
```
Replace with:
```rust
fn convert_files(
    root: &Path,
    items: ConvertList,
    format: core_model::format::AudioFormat,
    reserved: &std::collections::BTreeSet<String>,
    progress: &std::sync::atomic::AtomicU32,
    cancel: &std::sync::atomic::AtomicBool,
    kind: crate::jobs::JobKindRow,
) -> crate::jobs::Work {
    let importer = crate::media::importer();
    let n = items.len().max(1) as f32;
    let mut files: ConvertFiles = Vec::new();
    for (k, (media, path, stem)) in items.into_iter().enumerate() {
        let r = core_media::import::import_reserved(
            importer.as_ref(),
            &path,
            root,
            format,
            &stem,
            reserved,
            &mut |f| {
```

Find:
```rust
            let items = convert_list(&inner, self.id, |_| true);
            let root = inner.root(self.id);
```
Replace with:
```rust
            let items = convert_list(&inner, self.id, |_| true);
            let root = inner.root(self.id);
            let reserved = inner.reserved_names();
```

Find:
```rust
                    convert_files(
                        &root,
                        items,
                        f,
                        progress,
                        cancel,
                        crate::jobs::JobKindRow::Format,
                    )
```
Replace with:
```rust
                    convert_files(
                        &root,
                        items,
                        f,
                        &reserved,
                        progress,
                        cancel,
                        crate::jobs::JobKindRow::Format,
                    )
```

Find:
```rust
            let f = inner.doc().settings().format;
            let root = inner.root(self.id);
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Convert, move |progress, cancel| {
                    convert_files(
                        &root,
                        items,
                        f,
                        progress,
                        cancel,
                        crate::jobs::JobKindRow::Convert,
                    )
```
Replace with:
```rust
            let f = inner.doc().settings().format;
            let root = inner.root(self.id);
            let reserved = inner.reserved_names();
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Convert, move |progress, cancel| {
                    convert_files(
                        &root,
                        items,
                        f,
                        &reserved,
                        progress,
                        cancel,
                        crate::jobs::JobKindRow::Convert,
                    )
```

The import job reserves them too.

Find:
```rust
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Import, move |progress, cancel| {
```
Replace with:
```rust
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            let reserved = inner.reserved_names();
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Import, move |progress, cancel| {
```

Find:
```rust
                        let r = core_media::import::import_file(
                            importer.as_ref(),
                            &path,
                            &root,
                            format,
                            &mut |f| {
```
Replace with:
```rust
                        let stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let r = core_media::import::import_reserved(
                            importer.as_ref(),
                            &path,
                            &root,
                            format,
                            &stem,
                            &reserved,
                            &mut |f| {
```

The pool calls, after `cancel_job`.

Find:
```rust
    /// Asks job `id` to stop (nothing it did is applied).
    pub fn cancel_job(&self, id: u64) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).jobs.cancel(id);
            Ok(())
        })
    }
}
```
Replace with:
```rust
    /// Asks job `id` to stop (nothing it did is applied).
    pub fn cancel_job(&self, id: u64) -> Result<(), VizError> {
        ffi_guard(|| {
            lock(&self.inner).jobs.cancel(id);
            Ok(())
        })
    }
}

impl Session {
    /// A media-pool edit as one undo step; refused while an edit (a drag)
    /// is open.
    fn pool_edit(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
        if lock(&self.inner).edit_label.is_some() {
            return Err(crate::events::bad("finish the open edit first"));
        }
        self.execute(cmd)
    }

    /// The item `media`, or a `Model` error.
    fn media_item(&self, media: u64) -> Result<core_model::document::MediaItem, VizError> {
        lock(&self.inner)
            .doc()
            .media()
            .get(&core_model::ids::MediaId(media))
            .cloned()
            .ok_or_else(|| crate::events::bad(&format!("unknown media {media}")))
    }
}

/// The media pool window (spec §10.3; plan 5b Task 6).
#[uniffi::export]
impl Session {
    /// Renames media item `media` (display name only; trimmed, not empty):
    /// one undo step "Rename Media".
    pub fn rename_media(&self, media: u64, name: String) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(crate::events::bad("a media name cannot be empty"));
            }
            self.media_item(media)?;
            self.pool_edit(Command::Media(core_model::command::MediaCmd::Rename {
                media: core_model::ids::MediaId(media),
                name,
            }))
        })
    }

    /// Removes media item `media` and every region using it, as one undo
    /// step "Remove Media" (the app asks first; Ruling 13).
    pub fn remove_media(&self, media: u64) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| {
            use core_model::command::{EventCmd, MediaCmd};
            let id = core_model::ids::MediaId(media);
            self.media_item(media)?;
            let mut inner = lock(&self.inner);
            if inner.edit_label.is_some() {
                return Err(crate::events::bad("finish the open edit first"));
            }
            let users = core_model::command::media::regions_using(inner.doc(), id);
            let filter = ViewFilter::default();
            inner.history.begin("Remove Media")?;
            let run = (|| -> Result<(), VizError> {
                if !users.is_empty() {
                    inner
                        .history
                        .execute(&Command::Event(EventCmd::Delete { events: users }), &filter)?;
                }
                inner.history.execute(
                    &Command::Media(MediaCmd::Remove {
                        media: id,
                        replacement: None,
                    }),
                    &filter,
                )?;
                Ok(())
            })();
            match run {
                Ok(()) => inner.history.commit()?,
                Err(e) => {
                    inner.history.abort()?;
                    return Err(e);
                }
            }
            inner.version += 1;
            inner.reload_host(self.id);
            self.mark(&mut inner);
            inner.snapshot()
        })
    }

    /// Removes every item no region uses: one undo step "Compact Media".
    /// The files go to the Trash by the retention rule once no undo or
    /// redo state needs them (Ruling 11).
    pub fn compact_media(&self) -> Result<DocSnapshot, VizError> {
        ffi_guard(|| self.pool_edit(Command::Media(core_model::command::MediaCmd::Compact)))
    }

    /// The regions using media item `media`.
    pub fn media_users(&self, media: u64) -> Result<Vec<crate::media::MediaUseRow>, VizError> {
        ffi_guard(|| {
            self.media_item(media)?;
            Ok(crate::media::users(
                lock(&self.inner).doc(),
                core_model::ids::MediaId(media),
            ))
        })
    }

    /// The absolute path of media item `media`'s file (Reveal in Finder).
    pub fn media_file(&self, media: u64) -> Result<String, VizError> {
        ffi_guard(|| {
            let m = self.media_item(media)?;
            let root = lock(&self.inner).root(self.id);
            Ok(root.join(&m.path).to_string_lossy().into_owned())
        })
    }

    /// Relinks media item `media` to the file at `path` (absolute; spec
    /// §10.3 "Locate…"): a job of kind `Relink` that imports (converts)
    /// the file into `Media/` unless it already is a VizDAW CAF in the
    /// project format there, then applies one undo step "Relink Media".
    pub fn relink_media(&self, media: u64, path: String) -> Result<u64, VizError> {
        ffi_guard(|| {
            let item = self.media_item(media)?;
            let src = PathBuf::from(&path);
            if path.is_empty() || !src.is_absolute() {
                return Err(crate::events::bad("relink needs the absolute path of a file"));
            }
            let mut inner = lock(&self.inner);
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            let reserved = inner.reserved_names();
            let id = core_model::ids::MediaId(media);
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Relink, move |progress, cancel| {
                    let importer = crate::media::importer();
                    let r = crate::relink::relink_file(
                        importer.as_ref(),
                        &root,
                        &src,
                        format,
                        &reserved,
                        &mut |f| progress.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed),
                        cancel,
                    );
                    match r {
                        Err(core_media::MediaError::Cancelled) => crate::jobs::Work::Cancelled,
                        r => crate::jobs::Work::Relink {
                            files: vec![(id, r.map_err(|e| format!("{}: {e}", item.name)))],
                        },
                    }
                })
        })
    }

    /// Searches `folder` (absolute) for every item offline as missing,
    /// changed in length or changed in content (spec §10.3 "Search
    /// folder…", Ruling 10): a job of kind `Relink`, one undo step per
    /// item relinked, a problem per item not found.
    pub fn search_media(&self, folder: String) -> Result<u64, VizError> {
        ffi_guard(|| {
            use crate::media::OfflineReasonRow;
            let dir = PathBuf::from(&folder);
            if folder.is_empty() || !dir.is_absolute() {
                return Err(crate::events::bad("search needs the absolute path of a folder"));
            }
            let mut inner = lock(&self.inner);
            inner.refresh_media(self.id, false);
            let lost: Vec<crate::relink::Lost> = inner
                .doc()
                .media()
                .values()
                .filter(|m| {
                    matches!(
                        inner.offline.get(&m.id),
                        Some(OfflineReasonRow::Missing | OfflineReasonRow::Length | OfflineReasonRow::Changed)
                    )
                })
                .map(|m| crate::relink::Lost {
                    media: m.id,
                    name: m.name.clone(),
                    stem: core_model::document::MediaItem::file_parts(&m.path).1,
                    hash: m.content_hash.clone(),
                    seconds: m.length_frames as f64 / f64::from(m.sample_rate.max(1)),
                })
                .collect();
            if lost.is_empty() {
                return Err(crate::events::bad("no media item is missing"));
            }
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            let reserved = inner.reserved_names();
            inner
                .jobs
                .spawn(crate::jobs::JobKindRow::Relink, move |progress, cancel| {
                    let importer = crate::media::importer();
                    crate::relink::search(
                        importer.as_ref(),
                        &root,
                        &dir,
                        lost,
                        format,
                        &reserved,
                        progress,
                        cancel,
                    )
                })
        })
    }
}
```

- [ ] **Step 5: `FakeSession`**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
    /// `normalizeRegions` calls; `normalizeError` is thrown instead.
```
Replace with:
```swift
    // MARK: Media pool (plan 5b Task 6)

    var renamed: [(UInt64, String)] = []
    var removedMedia: [UInt64] = []
    var compactions = 0
    var relinks: [(media: UInt64, path: String)] = []
    var searches: [String] = []
    /// What `mediaUsers` answers, per item.
    var usersTable: [UInt64: [MediaUseRow]] = [:]
    /// Thrown by the pool's edits.
    var poolError: VizError?
    func renameMedia(media: UInt64, name: String) throws -> DocSnapshot {
        if let e = poolError { throw e }
        renamed.append((media, name))
        return commandResult()
    }
    func removeMedia(media: UInt64) throws -> DocSnapshot {
        if let e = poolError { throw e }
        removedMedia.append(media)
        return commandResult()
    }
    func compactMedia() throws -> DocSnapshot {
        if let e = poolError { throw e }
        compactions += 1
        return commandResult()
    }
    func mediaUsers(media: UInt64) throws -> [MediaUseRow] { usersTable[media] ?? [] }
    func mediaFile(media: UInt64) throws -> String { "/tmp/pool/\(media).caf" }
    func relinkMedia(media: UInt64, path: String) throws -> UInt64 {
        relinks.append((media, path))
        let id = UInt64(3000 + relinks.count)
        jobRows[id] = JobRow(
            id: id, kind: .relink, progress: 1, state: jobStartState, message: nil, problems: [], created: [media])
        return id
    }
    func searchMedia(folder: String) throws -> UInt64 {
        searches.append(folder)
        let id = UInt64(4000 + searches.count)
        jobRows[id] = JobRow(
            id: id, kind: .relink, progress: 1, state: jobStartState, message: nil, problems: [], created: [])
        return id
    }

    /// `normalizeRegions` calls; `normalizeError` is thrown instead.
```

- [ ] **Step 6: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test media_pool --test media --test format 2>&1 | grep -E "^test |^test result|panicked"
```
Expected: `media_pool` 8 passed; 5a's `media` and `format` unchanged (their jobs now reserve names; none of them depended on reusing a referenced name).

- [ ] **Step 7: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: all green.

- [ ] **Step 8: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/relink.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/jobs.rs crates/core-ffi/src/media.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/media_pool.rs app/Tests/VizDAWAppModelTests/FakeSession.swift crates/core-ffi/tests/control.rs
git commit -m "core-ffi: media pool calls, relink and search jobs, reserved names

MediaRow gains used_percent; the pool renames, removes an item with its
regions as one step, compacts, lists users and reveals files. Relink
imports the chosen file into Media/ (or uses one already there) and
applies one Relink Media step per item; Search folder matches by hash,
then name and duration. Every job that writes into Media/ now avoids
the names any history state references (final review r2; 5a Task 11 m6).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 7: Media retention — the Trash on save and on close

**Files:**
- Create: `crates/platform-macos/src/trash.rs`, `crates/core-ffi/src/retention.rs`, `crates/core-ffi/tests/retention.rs`
- Modify: `crates/platform-macos/src/lib.rs`, `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/session.rs`
- Test: `crates/core-ffi/tests/retention.rs`, `crates/platform-macos/src/trash.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `History::referenced_media_anywhere()` (5a final review I3); `core_model::file::{PROJECT_FILE, from_json, list_autosaves}`; Task 2's `overview::DIR`; 5a's `import::INCOMING_DIR`; `Jobs::busy`.
- Produces:
  - `platform_macos::trash::move_to_trash(path: &Path) -> Result<(), String>` (`NSFileManager trashItemAtURL:resultingItemURL:error:`);
  - `core_ffi::retention::{Outcome { trashed: Vec<String>, problems: Vec<String> }, on_disk, candidates, run, trash, trash_dir, move_to}`;
  - `Inner::tidy(&mut self, package: &Path, jobs_done: bool)`; `Inner.trash_dir: Option<PathBuf>`; run after every successful save (on the saved package) and on `close` of a saved project;
  - test hook (feature `test-hooks`, not exported): `Session::debug_set_trash_dir(&self, dir: Option<String>)`.

Ruling 11. The keep set is the union of the history's paths (past, present, future and an open transaction's base), the saved `project.json`'s and every autosave's. If the saved project or any autosave cannot be read, nothing moves and a problem says so. A move that fails keeps the file and reports it in the next `idle`'s problems. Built with `test-hooks`, the platform Trash is never called.

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/tests/retention.rs <<'VIZDAW_EOF'
//! The retention rule (plan 5b Task 7, Ruling 11): on save and on close,
//! media files nothing can bring back go to the Trash; everything an undo
//! or redo state, the saved project or an autosave names stays.

use core_media::caf::{CafWriter, WriteMode, hash_file};
use core_model::command::{Command, MediaCmd, apply};
use core_model::format::BitDepth;
use core_model::history::Timestamp;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use vizdaw_ffi::{JobStateRow, Session};

fn caf(path: &Path, frames: usize, value: f32) -> PathBuf {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut w = CafWriter::create(path, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(&vec![value; frames * 2]).unwrap();
    w.finish().unwrap();
    path.to_path_buf()
}

fn import(s: &Arc<Session>, path: &Path) {
    let id = s.import_audio(vec![path.display().to_string()], None).unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            assert_eq!(j.state, JobStateRow::Done, "{j:?}");
            return;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// An untitled session with its own trash folder, saved as `P.vizdaw`
/// after importing `files` (named after their stems).
fn saved(dir: &Path, files: &[&str]) -> (Arc<Session>, PathBuf, PathBuf) {
    let s = Session::new_empty().unwrap();
    let trash = dir.join("Trash");
    s.debug_set_trash_dir(Some(trash.display().to_string()));
    for (k, f) in files.iter().enumerate() {
        import(&s, &caf(&dir.join("src").join(f), 100 + k, 0.25));
    }
    let pkg = dir.join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    (s, pkg, trash)
}

fn problems(s: &Session) -> Vec<String> {
    s.idle().unwrap().problems
}

#[test]
fn save_trashes_what_nothing_references() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, trash) = saved(dir.path(), &["a.caf", "b.caf"]);
    assert!(!trash.exists(), "nothing to tidy after Save As");
    // Strays, a hidden file, an import leftover and an orphan overview.
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    caf(&pkg.join("Media/Sub/old.caf"), 10, 0.5);
    std::fs::write(pkg.join("Media/.keep"), "x").unwrap();
    caf(&pkg.join("Media/.incoming/left.caf"), 10, 0.5);
    let orphan = pkg.join("Media/.overviews").join(format!("{}.vzov", "0".repeat(64)));
    std::fs::write(&orphan, "x").unwrap();
    let stray_hash = hash_file(&pkg.join("Media/stray.caf")).unwrap();
    s.save().unwrap();
    assert!(!pkg.join("Media/stray.caf").exists());
    assert!(!pkg.join("Media/Sub/old.caf").exists());
    assert_eq!(hash_file(&trash.join("stray.caf")).unwrap(), stray_hash, "moved, not unlinked");
    assert!(trash.join("old.caf").is_file());
    assert!(pkg.join("Media/.keep").is_file(), "hidden files are not media");
    assert!(!pkg.join("Media/.incoming/left.caf").exists());
    assert!(!orphan.exists());
    // The items' files and overviews stay.
    for r in s.media().unwrap() {
        assert!(pkg.join(&r.path).is_file(), "{}", r.path);
    }
    let kept = std::fs::read_dir(pkg.join("Media/.overviews")).unwrap().count();
    assert_eq!(kept, 2);
    assert!(problems(&s).is_empty());
    s.close().unwrap();
}

/// Review Focus 1.
#[test]
fn retention_never_trashes_what_undo_redo_or_disk_documents_reference() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, trash) = saved(dir.path(), &["a.caf"]);
    // B imported, then undone: only the redo history names it.
    import(&s, &caf(&dir.path().join("src/b.caf"), 50, 0.5));
    s.undo().unwrap();
    s.save().unwrap();
    assert!(pkg.join("Media/b.caf").is_file(), "redo can bring B back");
    // A new edit drops the redo history: now nothing names B.
    let a = s.media().unwrap()[0].id;
    s.rename_media(a, "Drums".into()).unwrap();
    s.save().unwrap();
    assert!(!pkg.join("Media/b.caf").exists());
    assert!(trash.join("b.caf").is_file());
    // Compacting A (unused) keeps its file: undo can bring it back.
    s.compact_media().unwrap();
    s.save().unwrap();
    assert!(pkg.join("Media/a.caf").is_file());
    s.close().unwrap();
    // Files only the saved project or an autosave name stay too.
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(trash.display().to_string()));
    let doc = core_model::file::load(&pkg).unwrap();
    let with = |path: &str| {
        apply(
            &doc,
            &Command::Media(MediaCmd::Add {
                path: path.into(),
                sample_rate: 48_000,
                channels: 2,
                length_frames: 10,
                content_hash: None,
                name: None,
                origin: None,
            }),
        )
        .unwrap()
        .doc
    };
    caf(&pkg.join("Media/disk.caf"), 10, 0.5);
    caf(&pkg.join("Media/auto.caf"), 10, 0.5);
    core_model::file::save(&with("Media/disk.caf"), &pkg).unwrap();
    core_model::file::autosave(&with("Media/auto.caf"), &pkg, Timestamp(1)).unwrap();
    s.close().unwrap();
    assert!(pkg.join("Media/disk.caf").is_file(), "the saved project names it");
    assert!(pkg.join("Media/auto.caf").is_file(), "an autosave names it");
}

/// Review Focus 1.
#[test]
fn an_unreadable_autosave_trashes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, _trash) = saved(dir.path(), &["a.caf"]);
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    std::fs::write(pkg.join("Autosave/project-00000000000000000001.json"), "{ not json").unwrap();
    s.save().unwrap();
    assert!(pkg.join("Media/stray.caf").is_file());
    assert!(
        problems(&s).iter().any(|p| p.contains("could not be read")),
        "the user is told why"
    );
    s.close().unwrap();
}

/// Review Focus 1.
#[test]
fn a_failed_trash_keeps_the_file_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, _trash) = saved(dir.path(), &["a.caf"]);
    // The "trash folder" is a file: nothing can move into it.
    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "x").unwrap();
    s.debug_set_trash_dir(Some(blocked.display().to_string()));
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    s.save().unwrap();
    assert!(pkg.join("Media/stray.caf").is_file());
    let p = problems(&s);
    assert!(
        p.iter().any(|l| l.starts_with("Media/stray.caf: not moved to the Trash")),
        "{p:?}"
    );
    s.close().unwrap();
}

#[test]
fn close_tidies_and_an_unapplied_job_holds_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, trash) = saved(dir.path(), &["a.caf"]);
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    // A job not applied yet: its new file is not in the history.
    s.debug_hold_jobs(true).unwrap();
    let id = s
        .import_audio(vec![caf(&dir.path().join("src/c.caf"), 20, 0.5).display().to_string()], None)
        .unwrap();
    s.save().unwrap();
    assert!(pkg.join("Media/stray.caf").is_file(), "held back while a job runs");
    s.debug_hold_jobs(false).unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    while s.job(id).unwrap().state == JobStateRow::Running {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    caf(&pkg.join("Media/late.caf"), 10, 0.5);
    s.close().unwrap();
    assert!(!pkg.join("Media/stray.caf").exists() && !pkg.join("Media/late.caf").exists());
    assert!(trash.join("stray.caf").is_file() && trash.join("late.caf").is_file());
    assert!(pkg.join("Media/c.caf").is_file(), "the applied import's file stays");
}

#[test]
fn tests_never_reach_the_real_trash() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_empty().unwrap();
    import(&s, &caf(&dir.path().join("src/a.caf"), 100, 0.25));
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(pkg.display().to_string()).unwrap();
    caf(&pkg.join("Media/stray.caf"), 10, 0.5);
    s.save().unwrap();
    assert!(pkg.join("Media/stray.caf").is_file());
    assert!(problems(&s).iter().any(|p| p.contains("no trash folder in tests")));
    s.close().unwrap();
}

/// Final review r2, after the Trash: a freed name may be reused, and every
/// path the history names still holds the audio it recorded.
#[test]
fn after_the_trash_every_history_path_names_one_audio() {
    let dir = tempfile::tempdir().unwrap();
    let (s, pkg, trash) = saved(dir.path(), &["x.caf"]);
    caf(&pkg.join("Media/y.caf"), 10, 0.5);
    s.save().unwrap();
    assert!(trash.join("y.caf").is_file());
    import(&s, &caf(&dir.path().join("src2/y.caf"), 30, 0.75));
    let rows = s.media().unwrap();
    assert_eq!(rows[1].path, "Media/y.caf", "the freed name is free");
    s.save().unwrap();
    for r in rows {
        let file = pkg.join(&r.path);
        assert!(file.is_file(), "{}", r.path);
    }
    assert_ne!(
        hash_file(&trash.join("y.caf")).unwrap(),
        hash_file(&pkg.join("Media/y.caf")).unwrap()
    );
    s.close().unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p core-ffi --test retention 2>&1 | tail -5`
Expected: compile error (`debug_set_trash_dir` not found).

- [ ] **Step 3: The platform Trash**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/platform-macos/src/trash.rs <<'VIZDAW_EOF'
//! Moving a file to the Trash (sub-project 5 spec §6.5; plan 5b
//! Ruling 11): `-[NSFileManager trashItemAtURL:resultingItemURL:error:]`,
//! so a media file VizDAW no longer needs stays recoverable.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::CFURL;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

// `NSFileManager` lives in Foundation.
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

/// Moves the file at `path` to the Trash of its volume.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{}: no such file", path.display()));
    }
    let cls = AnyClass::get(c"NSFileManager").ok_or("Foundation is not loaded")?;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `bytes` is a valid buffer of the given length.
    let url = unsafe {
        CFURL::from_file_system_representation(None, bytes.as_ptr(), bytes.len() as isize, false)
    }
    .ok_or_else(|| format!("{}: not a file URL", path.display()))?;
    // A CFURL is toll-free bridged to an NSURL.
    // SAFETY: `url` is a live CFURL for the whole call.
    let ns_url: &AnyObject = unsafe { &*(&*url as *const CFURL).cast::<AnyObject>() };
    // SAFETY: `defaultManager` returns the shared file manager; the call
    // takes a valid NSURL and two optional out pointers (null: unwanted).
    let ok: bool = unsafe {
        let fm: Option<Retained<AnyObject>> = msg_send![cls, defaultManager];
        let fm = fm.ok_or("no file manager")?;
        msg_send![
            &*fm,
            trashItemAtURL: ns_url,
            resultingItemURL: std::ptr::null_mut::<*mut AnyObject>(),
            error: std::ptr::null_mut::<*mut AnyObject>()
        ]
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{}: the Trash refused it", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_fails_before_reaching_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        let e = move_to_trash(&dir.path().join("nothing.caf")).unwrap_err();
        assert!(e.contains("no such file"), "{e}");
    }

    /// Moves a uniquely named empty file to the real Trash: run by hand
    /// (README step 125), never on CI.
    #[test]
    #[ignore = "moves a temp file to the user's real Trash"]
    fn a_file_goes_to_the_real_trash() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join(format!("vizdaw-trash-check-{}.txt", std::process::id()));
        std::fs::write(&f, "VizDAW Trash check; safe to delete").unwrap();
        move_to_trash(&f).unwrap();
        assert!(!f.exists());
    }
}
VIZDAW_EOF
```

In `crates/platform-macos/src/lib.rs`:

Find:
```rust
pub mod rt;
pub mod sinks;
```
Replace with:
```rust
pub mod rt;
pub mod sinks;
pub mod trash;
```

Also extend its module doc's list of files with `unsafe`:

Find:
```rust
//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs`, `rt.rs`,
//! `media.rs` and `au/ffi.rs`, with a `// SAFETY:` comment per block.
```
Replace with:
```rust
//! All `unsafe` lives in `hal.rs`, `dls.rs`, `midi.rs`, `rt.rs`,
//! `media.rs`, `trash.rs` and `au/ffi.rs`, with a `// SAFETY:` comment per
//! block.
```

If `msg_send!`'s argument encodings disagree with objc2 0.6's checks (debug builds verify them), keep the behaviour and adapt the types (for example `*const AnyObject` for the URL), and say so in the report.

- [ ] **Step 4: The retention rule**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > crates/core-ffi/src/retention.rs <<'VIZDAW_EOF'
//! The retention rule (sub-project 5 spec §6.5 and Ruling 13; plan 5b
//! Ruling 11): on save and on close, files under `Media/` that no undo or
//! redo state, no saved project and no autosave references go to the
//! Trash. They are moved, never unlinked. The history's references come
//! from `History::referenced_media_anywhere` (final review I3), never from
//! the present document alone.

use core_model::document::Document;
use core_model::file;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What a pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Package-relative paths moved to the Trash.
    pub trashed: Vec<String>,
    /// Lines for the user.
    pub problems: Vec<String>,
}

/// The media paths and hashes the package's saved project and autosaves
/// name; `None` when any of them cannot be read or parsed (then nothing
/// may be trashed).
pub fn on_disk(package: &Path) -> Option<(BTreeSet<String>, BTreeSet<String>)> {
    let mut paths = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    let mut take = |d: &Document| {
        for m in d.media().values() {
            paths.insert(m.path.clone());
            if let Some(h) = &m.content_hash {
                hashes.insert(h.clone());
            }
        }
    };
    let pj = package.join(file::PROJECT_FILE);
    if pj.exists() {
        take(&file::from_json(&std::fs::read_to_string(&pj).ok()?).ok()?);
    }
    for a in file::list_autosaves(package).ok()? {
        take(&file::from_json(&std::fs::read_to_string(&a).ok()?).ok()?);
    }
    Some((paths, hashes))
}

/// Package-relative paths of the regular files under `package/Media/`
/// (subfolders too) whose name and folders do not start with `.` and are
/// UTF-8 (a media path is a string), sorted.
pub fn candidates(package: &Path) -> Vec<String> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let Ok(name) = e.file_name().into_string() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Ok(t) = e.file_type() else { continue };
            let r = format!("{rel}/{name}");
            if t.is_dir() {
                walk(&e.path(), &r, out);
            } else if t.is_file() {
                out.push(r);
            }
        }
    }
    let mut out = Vec::new();
    walk(&package.join("Media"), "Media", &mut out);
    out.sort();
    out
}

/// One pass over `package`: every candidate that `history` (path → hash,
/// the history's references), the saved project and the autosaves leave
/// out goes to the Trash through `trash`; overviews of hashes none of
/// them has and leftovers in `Media/.incoming` are deleted (caches and
/// interrupted imports, not media).
pub fn run(
    history: &BTreeMap<String, Option<String>>,
    package: &Path,
    trash: &dyn Fn(&Path) -> Result<(), String>,
) -> Outcome {
    let mut out = Outcome::default();
    let Some((disk_paths, disk_hashes)) = on_disk(package) else {
        out.problems.push(
            "Unused media files were not moved to the Trash: the saved project or one of its autosaves could not be read."
                .into(),
        );
        return out;
    };
    let keep: BTreeSet<&str> = history
        .keys()
        .map(String::as_str)
        .chain(disk_paths.iter().map(String::as_str))
        .collect();
    for rel in candidates(package) {
        if keep.contains(rel.as_str()) {
            continue;
        }
        match trash(&package.join(&rel)) {
            Ok(()) => out.trashed.push(rel),
            Err(e) => out
                .problems
                .push(format!("{rel}: not moved to the Trash ({e})")),
        }
    }
    let hashes: BTreeSet<&str> = history
        .values()
        .flatten()
        .map(String::as_str)
        .chain(disk_hashes.iter().map(String::as_str))
        .collect();
    if let Ok(rd) = std::fs::read_dir(package.join(core_media::overview::DIR)) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(h) = name.strip_suffix(".vzov") {
                if !hashes.contains(h) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(package.join(core_media::import::INCOMING_DIR)) {
        for e in rd.flatten() {
            if e.path().is_file() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    out
}

/// The folder that stands for the Trash: a session's (tests), else
/// `$VIZDAW_TRASH_DIR` when absolute (the e2e), else none (the platform
/// Trash).
pub fn trash_dir(session: Option<&Path>) -> Option<PathBuf> {
    session.map(Path::to_path_buf).or_else(|| {
        std::env::var_os("VIZDAW_TRASH_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    })
}

/// Moves `path` into `dir` under its own name (or `n name` when taken).
pub fn move_to(dir: &Path, path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    for n in 1..=10_000u32 {
        let to = if n == 1 {
            dir.join(&name)
        } else {
            dir.join(format!("{n} {name}"))
        };
        if to.exists() {
            continue;
        }
        return match std::fs::rename(path, &to) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => std::fs::copy(path, &to)
                .and_then(|_| std::fs::remove_file(path))
                .map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
    }
    Err("no free name in the trash folder".into())
}

/// Moves `path` to the Trash: into `dir` when given, else the platform's.
pub fn trash(dir: Option<&Path>, path: &Path) -> Result<(), String> {
    match dir {
        Some(d) => move_to(d, path),
        None => platform(path),
    }
}

/// With the test hooks on, the real Trash is never touched.
#[cfg(feature = "test-hooks")]
fn platform(_path: &Path) -> Result<(), String> {
    Err("no trash folder in tests".into())
}

#[cfg(all(not(feature = "test-hooks"), target_os = "macos"))]
fn platform(path: &Path) -> Result<(), String> {
    platform_macos::trash::move_to_trash(path)
}

#[cfg(all(not(feature = "test-hooks"), not(target_os = "macos")))]
fn platform(_path: &Path) -> Result<(), String> {
    Err("this platform has no Trash support yet".into())
}
VIZDAW_EOF
```

In `crates/core-ffi/src/lib.rs`:

Find:
```rust
pub mod relink;
pub mod revs;
```
Replace with:
```rust
pub mod relink;
pub mod retention;
pub mod revs;
```

In `crates/core-ffi/src/session.rs`:

Find:
```rust
    /// Items whose file failed the content-hash check, with the path and
    /// hash it was checked for (plan 5b Ruling 2; runtime only).
    changed: std::collections::BTreeMap<core_model::ids::MediaId, (String, Option<String>)>,
}
```
Replace with:
```rust
    /// Items whose file failed the content-hash check, with the path and
    /// hash it was checked for (plan 5b Ruling 2; runtime only).
    changed: std::collections::BTreeMap<core_model::ids::MediaId, (String, Option<String>)>,
    /// The folder standing for the Trash (test hook; `None`: the
    /// environment's or the platform's).
    trash_dir: Option<PathBuf>,
}
```

Find:
```rust
            overviews: Default::default(),
            changed: Default::default(),
        }
    }
```
Replace with:
```rust
            overviews: Default::default(),
            changed: Default::default(),
            trash_dir: None,
        }
    }

    /// The retention rule on `package` (plan 5b Ruling 11). Skipped while
    /// a job is not applied yet (its files are in no document), unless
    /// `jobs_done` (the jobs were shut down and their files removed).
    /// Failures become problems for the next `idle`.
    fn tidy(&mut self, package: &Path, jobs_done: bool) {
        if !jobs_done && self.jobs.busy() {
            return;
        }
        let dir = crate::retention::trash_dir(self.trash_dir.as_deref());
        let out = crate::retention::run(&self.history.referenced_media_anywhere(), package, &|p| {
            crate::retention::trash(dir.as_deref(), p)
        });
        self.news.problems.extend(out.problems);
    }
```

After a save:

Find:
```rust
        file::save(inner.doc(), path)?;
        write_windows(path, &inner.windows, inner.editor_layout.as_ref());
```
Replace with:
```rust
        file::save(inner.doc(), path)?;
        write_windows(path, &inner.windows, inner.editor_layout.as_ref());
        inner.tidy(path, false);
```

On close (a saved project; an untitled scratch package is removed whole, as in 5a):

Find:
```rust
            inner.overviews.shutdown();
            host::forget_media(self.id);
```
Replace with:
```rust
            inner.overviews.shutdown();
            if let Some(p) = inner.path.clone() {
                // The jobs were shut down and their files removed above.
                inner.tidy(&p, true);
            }
            host::forget_media(self.id);
```

The test hook:

Find:
```rust
    /// Forgets every loaded overview (test hook).
    pub fn debug_forget_overviews(&self) {
        lock(&self.inner).overviews.forget_all();
    }
}
```
Replace with:
```rust
    /// Forgets every loaded overview (test hook).
    pub fn debug_forget_overviews(&self) {
        lock(&self.inner).overviews.forget_all();
    }

    /// The folder this session's retention moves files into instead of
    /// the Trash (test hook; `None`: back to the default).
    pub fn debug_set_trash_dir(&self, dir: Option<String>) {
        lock(&self.inner).trash_dir = dir.map(PathBuf::from);
    }
}
```

`close` already ran `inner.overviews.shutdown()` (Task 4), so the worker cannot be writing into `Media/.overviews` while it is pruned.

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test retention --test media --test media_pool 2>&1 | grep -E "^test |^test result|panicked"
cargo test -p platform-macos trash 2>&1 | grep -E "^test |test result"
```
Expected: `retention` 7 passed; `media` and `media_pool` unchanged (their sessions have no trash folder, and they leave no strays, so retention moves nothing; when it would, the test-hooks Trash refuses and the file stays); `platform-macos` 1 passed, 1 ignored.

- [ ] **Step 6: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: all green.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/platform-macos/src/trash.rs crates/platform-macos/src/lib.rs crates/core-ffi/src/retention.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/session.rs crates/core-ffi/tests/retention.rs
git commit -m "Media retention: unreferenced files go to the Trash on save and close

After every save and on close, files under Media/ that no history state
(final review I3's referenced_media_anywhere), no saved project and no
autosave references are moved to the Trash through NSFileManager; an
unreadable autosave stops the pass, a failed move keeps the file and is
reported. Orphan overviews and .incoming leftovers are cleared. Tests
use a per-session trash folder and can never reach the real Trash.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 8: Swift models — `MediaModel`, `RegionCommands`, overview redraws

**Files:**
- Create: `app/Sources/VizDAWAppModel/MediaModel.swift`, `app/Sources/VizDAWAppModel/RegionCommands.swift`, `app/Tests/VizDAWAppModelTests/MediaModelTests.swift`, `app/Tests/VizDAWAppModelTests/RegionCommandsTests.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Sources/VizDAWAppModel/ProjectMedia.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift` (a `regionRow` helper)
- Test: `app/Tests/VizDAWAppModelTests/MediaModelTests.swift`, `app/Tests/VizDAWAppModelTests/RegionCommandsTests.swift`

**Interfaces:**
- Consumes: Tasks 4–6's `Session` methods (`regionWaveform`, `normalizeRegions`, `renameMedia`, `removeMedia`, `compactMedia`, `mediaUsers`, `mediaFile`, `relinkMedia`, `searchMedia`), `IdleUpdate.overviewsChanged`, `JobKindRow.relink`, `OfflineReasonRow.changed`, `MediaRow.usedPercent`, `AudioEdit.{fadeTo, removeFades}`; 5a's `DocumentModel.{mediaVersion, refreshAfterJob, pollJobs, jobs}`, `ControlJobs.answer`.
- Produces:
  - `DocumentModel.media: MediaModel` (made once), `DocumentModel.overviewVersion: Int` (bumped by `idle` on `overviewsChanged`), `DocumentModel.importAudio(paths:target:)`, `DocumentModel.startJob(_:_:_:)` (internal; 5a's private `start` renamed);
  - `ProjectBanner.jobText(_ kind: JobKindRow) -> String`, `ProjectBanner.failureTitle(_ kind: JobKindRow) -> String`;
  - `MediaPoolRow { id, name, duration, channels, origin, uses, used, offline }`;
  - `MediaModel`: `rows`, `row(_:)`, `offline(_:)`, `relinkable`, `poolRows`, `selection`, `relinkShown`, `rename(_:to:)`, `compact()`, `remove(_:)`, `users(_:)`, `deleteQuestion(_:)`, `selectUsers(_:)`, `selectUnused()`, `relink(_:path:)`, `searchFolder(_:)`, `importIntoPool(_:)`, `file(_:)`, `offerRelinkIfNeeded()`, `leaveOffline()`, `waveform(event:fromTick:toTick:columns:)`; statics `poolRow(_:)`, `duration(frames:rate:)`, `originText(_:)`, `reasonText(_:)`, `relinkReasons`;
  - `RegionCommands` (`MenuCommands`): ids `region.normalize`, `region.fade_in_to_cursor`, `region.fade_out_from_cursor`, `region.remove_fades`; `run(_:on:_:)`, `normalize(peakDb:_:)`, `parsePeak(_:)`, `defaultPeakDb`, `enabled(_:_:)`.

Rulings 6, 13, 14 and 18. `MediaModel.rows` reads `document.mediaVersion` (so views observing it redraw) and refetches only when it moved; a refetch is ignored storage, so reading rows while a view draws never changes observed state. Pool edits fail into the document's alert; job outcomes go through the existing `pollJobs` (`ControlJobs.answer`).

- [ ] **Step 1: Write the failing tests**

In `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

Find:
```swift
/// An events block, or a reference block when `ref` is set.
```
Replace with:
```swift
/// An audio region of media `media`: `frames` frames shown over
/// `duration` ticks.
func regionRow(
    _ id: UInt64, tick: Int64, duration: Int64, frames: UInt64, media: UInt64 = 7, gainDb: Float = 0,
    fadeIn: Int64 = 0, fadeOut: Int64 = 0
) -> EventRow {
    var r = noteRow(id, tick: tick, duration: duration)
    r.kind = .audioRegion
    r.media = media
    r.frames = frames
    r.gainDb = gainDb
    r.fadeIn = FadeRow(length: fadeIn, shape: .linear)
    r.fadeOut = FadeRow(length: fadeOut, shape: .linear)
    return r
}

/// A media-pool row.
func mediaRow(
    _ id: UInt64, _ name: String, frames: UInt64 = 96_000, uses: UInt32 = 0, offline: OfflineReasonRow? = nil,
    used: Float = 0
) -> MediaRow {
    MediaRow(
        id: id, name: name, path: "Media/\(name).caf", channels: 2, frames: frames, sampleRate: 48_000,
        origin: .imported, originalName: "\(name).wav", offline: offline, uses: uses, usedPercent: used)
}

/// An events block, or a reference block when `ref` is set.
```

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/MediaModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The media pool's model (plan 5b Task 8; spec §12.3 "MediaModel with
/// FakeSession").
@MainActor
@Suite struct MediaModelTests {
    func model(_ f: FakeSession) throws -> DocumentModel { try DocumentModel(session: f) }

    @Test func rowsFollowTheMediaVersion() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "kick")]
        let m = try model(f)
        #expect(m.media.rows.map(\.id) == [1])
        f.mediaRows = [mediaRow(1, "kick"), mediaRow(2, "snare")]
        #expect(m.media.rows.count == 1, "cached until the version moves")
        m.refreshAfterJob()
        #expect(m.media.rows.map(\.name) == ["kick", "snare"])
        #expect(m.media.row(2)?.name == "snare")
        #expect(m.media.row(nil) == nil)
    }

    @Test func poolRowsSayWhatTheWindowShows() {
        #expect(MediaModel.duration(frames: 96_000, rate: 48_000) == "0:02.0")
        #expect(MediaModel.duration(frames: 48_000 * 61 + 2_400, rate: 48_000) == "1:01.1")
        #expect(MediaModel.duration(frames: 48_000 * 3_725, rate: 48_000) == "1:02:05.0")
        #expect(MediaModel.duration(frames: 10, rate: 0) == "0:10.0")
        let r = MediaModel.poolRow(mediaRow(3, "pad", uses: 2, offline: .changed, used: 74.6))
        #expect(r.name == "pad" && r.channels == "Stereo" && r.origin == "Imported")
        #expect(r.uses == 2 && r.used == "75 %" && r.offline == "Changed")
        #expect(MediaModel.poolRow(mediaRow(4, "x")).offline == "")
        #expect(MediaModel.reasonText(.missing) == "Missing")
        #expect(MediaModel.reasonText(.length) == "Changed length")
    }

    @Test func renameTrimsAndSkipsBlankOrUnchangedNames() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick")]
        let m = try model(f)
        #expect(m.media.rename(3, to: "  Kick 2 "))
        #expect(f.renamed.map(\.1) == ["Kick 2"])
        #expect(!m.media.rename(3, to: "   "))
        #expect(!m.media.rename(3, to: "kick"))
        #expect(f.renamed.count == 1)
        f.poolError = .Model(message: "nope")
        #expect(!m.media.rename(3, to: "Other"))
        #expect(m.alert?.title == "Rename Failed")
    }

    @Test func deleteAsksFirstWhenRegionsUseTheItem() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick", uses: 2), mediaRow(4, "spare")]
        f.usersTable[3] = [
            MediaUseRow(seq: 3, lane: 31, event: 9, tick: 0, sequence: "Song", laneName: "Audio 1"),
            MediaUseRow(seq: 3, lane: 31, event: 10, tick: 61_440, sequence: "Song", laneName: "Audio 1"),
        ]
        let m = try model(f)
        let q = try #require(m.media.deleteQuestion(3))
        #expect(q.contains("used by 2 regions"))
        #expect(q.contains("Song › Audio 1, bar 1"))
        #expect(q.contains("Song › Audio 1, bar 2"))
        #expect(m.media.deleteQuestion(4) == nil)
        m.media.selection = [3, 4]
        #expect(m.media.remove(3))
        #expect(f.removedMedia == [3])
        #expect(m.media.selection == [4])
    }

    @Test func selectingUsersShowsTheirSequence() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick", uses: 2)]
        f.usersTable[3] = [
            MediaUseRow(seq: 2, lane: 21, event: 9, tick: 0, sequence: "Groove", laneName: "Bass"),
            MediaUseRow(seq: 3, lane: 31, event: 10, tick: 0, sequence: "Song", laneName: "Chords"),
        ]
        let m = try model(f)
        m.media.selectUsers(3)
        #expect(m.selectedSeq == 2)
        #expect(m.editor.selection.events == [9])
        #expect(m.editor.selection.lanes == [LaneRef(seq: 2, lane: 21)])
        m.media.selectUsers(99)
        #expect(m.editor.status == "No region uses this item.")
    }

    @Test func selectUnusedPicksItemsWithoutRegions() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", uses: 1), mediaRow(2, "b"), mediaRow(3, "c")]
        let m = try model(f)
        m.media.selectUnused()
        #expect(m.media.selection == [2, 3])
    }

    @Test func relinkAndSearchRunAsRelinkJobs() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(3, "kick", offline: .missing)]
        f.jobStartState = .running
        let m = try model(f)
        m.media.relink(3, path: "/x/kick.wav")
        #expect(f.relinks.first?.path == "/x/kick.wav")
        #expect(m.jobs.first?.kind == .relink)
        #expect(m.banners.contains { if case .job(3001, .relink, _) = $0 { true } else { false } })
        f.jobRows[3001]?.state = .done
        let before = m.mediaVersion
        m.pollJobs()
        #expect(m.mediaVersion == before + 1)
        m.media.searchFolder("/x")
        #expect(f.searches == ["/x"])
        f.jobRows[4001] = JobRow(
            id: 4001, kind: .relink, progress: 1, state: .done, message: nil,
            problems: ["kick: not found in /x"], created: [])
        m.pollJobs()
        #expect(m.alert?.title == "Relink Failed")
        #expect(m.alert?.message.contains("not found") == true)
    }

    @Test func theRelinkSheetIsOfferedForMissingOrChangedMedia() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", offline: .format)]
        #expect(try !model(f).media.relinkShown, "Convert handles another format")
        f.mediaRows = [mediaRow(1, "a", offline: .changed), mediaRow(2, "b")]
        let m = try model(f)
        #expect(m.media.relinkShown)
        #expect(m.media.relinkable.map(\.id) == [1])
        m.media.leaveOffline()
        #expect(!m.media.relinkShown)
    }

    @Test func jobTextsAndTitles() {
        #expect(ProjectBanner.jobText(.import) == "Importing audio…")
        #expect(ProjectBanner.jobText(.relink) == "Relinking media…")
        #expect(ProjectBanner.jobText(.format) == "Converting media…")
        #expect(ProjectBanner.failureTitle(.relink) == "Relink Failed")
        #expect(ProjectBanner.failureTitle(.import) == "Import Failed")
        #expect(ProjectBanner.failureTitle(.convert) == "Conversion Failed")
    }

    @Test func dropsImportWithTheirOwnTarget() throws {
        let f = demoFake()
        let m = try model(f)
        m.importAudio(paths: ["/a.wav"], target: ImportTargetRow(seq: 3, lane: nil, tick: 1_920))
        #expect(f.imports.last?.1 == ImportTargetRow(seq: 3, lane: nil, tick: 1_920))
        m.media.importIntoPool(["/b.wav"])
        #expect(f.imports.last.map { $0.1 == nil } == true)
        m.importAudio(paths: [], target: nil)
        #expect(f.imports.count == 2)
    }

    @Test func waveformsClampTheirColumnsAndOverviewsBumpAVersion() throws {
        let f = demoFake()
        let m = try model(f)
        #expect(m.media.waveform(event: 1, fromTick: 0, toTick: 100, columns: 0) == nil)
        #expect(m.media.waveform(event: 1, fromTick: 100, toTick: 100, columns: 10) == nil)
        _ = m.media.waveform(event: 1, fromTick: 0, toTick: 100, columns: 100_000)
        #expect(f.waveformCalls.last?.columns == 8192)
        f.idleUpdates = [
            IdleUpdate(
                snapshot: nil, slotsChanged: false, paramsChanged: [], problems: [], mediaChanged: false,
                overviewsChanged: true)
        ]
        m.idle()
        #expect(m.overviewVersion == 1)
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWAppModelTests/RegionCommandsTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// The Region menu (plan 5b Task 8; spec §10.2).
@MainActor
@Suite struct RegionCommandsTests {
    func model() throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        return (try DocumentModel(session: f), f)
    }

    @Test func everyCommandNeedsSelectedEvents() throws {
        let (m, f) = try model()
        for id in RegionCommands.ids {
            #expect(!RegionCommands.enabled(id, m))
            #expect(RegionCommands.run(id, m) == .unavailable("Select audio regions first."), "\(id)")
        }
        #expect(f.edits.isEmpty && f.normalizations.isEmpty)
    }

    @Test func fadesUseTheTransportPosition() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5]))
        f.state.tick = 960
        m.poll()
        #expect(RegionCommands.enabled(RegionCommands.fadeIn, m))
        #expect(RegionCommands.run(RegionCommands.fadeIn, m) == .done)
        #expect(f.edits.last == .audio(edit: .fadeTo(events: [5], at: 960, fadeIn: true)))
        #expect(RegionCommands.run(RegionCommands.fadeOut, m) == .done)
        #expect(f.edits.last == .audio(edit: .fadeTo(events: [5], at: 960, fadeIn: false)))
        #expect(RegionCommands.run(RegionCommands.removeFades, m) == .done)
        #expect(f.edits.last == .audio(edit: .removeFades(events: [5])))
        f.editError = .Model(message: "the cursor is not inside a selected audio region")
        #expect(
            RegionCommands.run(RegionCommands.fadeIn, m)
                == .refused("the cursor is not inside a selected audio region"))
    }

    @Test func normalizeSendsThePeakAndReportsBusy() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5, 6]))
        #expect(RegionCommands.normalize(peakDb: -1, m) == .done)
        #expect(f.normalizations.last?.events == [5, 6])
        #expect(f.normalizations.last?.peakDb == -1)
        #expect(RegionCommands.run(RegionCommands.normalize, m) == .done)
        #expect(f.normalizations.last?.peakDb == Float(RegionCommands.defaultPeakDb))
        f.normalizeError = .Busy(message: "still being built")
        #expect(RegionCommands.normalize(peakDb: -1, m) == .busy("still being built"))
        #expect(m.editor.status == "still being built")
        f.normalizeError = nil
        for bad in [0.5, -96.5, Double.nan, Double.infinity] {
            if case .refused = RegionCommands.normalize(peakDb: bad, m) {} else {
                Issue.record("\(bad) was accepted")
            }
        }
        #expect(f.normalizations.count == 2)
    }

    @Test func peakTextParses() {
        #expect(RegionCommands.parsePeak("-0.3") == -0.3)
        #expect(RegionCommands.parsePeak(" −1 ") == -1)
        #expect(RegionCommands.parsePeak("0") == 0)
        for bad in ["", "abc", "nan", "inf", "-inf", "1", "-96.01", "1e999", "0x10", "-0.3dB"] {
            #expect(RegionCommands.parsePeak(bad) == nil, "\(bad)")
        }
    }

    @Test func aGestureBlocksTheCommands() throws {
        let (m, f) = try model()
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5]))
        #expect(m.editor.beginGesture("Drag", lanes: [], origin: .other))
        #expect(!RegionCommands.enabled(RegionCommands.removeFades, m))
        #expect(RegionCommands.run(RegionCommands.removeFades, m) == .unavailable(EditorModel.gestureOpenMessage))
        #expect(RegionCommands.normalize(peakDb: -1, m) == .unavailable(EditorModel.gestureOpenMessage))
        #expect(f.normalizations.isEmpty)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `scripts/build-ffi.sh && scripts/swift.sh test 2>&1 | tail -5`
Expected: compile errors (`MediaModel`, `RegionCommands`, `DocumentModel.media`, `ProjectBanner.jobText` not found).

- [ ] **Step 3: `MediaModel`**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWAppModel/MediaModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// One media-pool row as the window shows it (spec §10.3).
public struct MediaPoolRow: Identifiable, Equatable, Sendable {
    public let id: UInt64
    public let name: String
    public let duration: String
    public let channels: String
    public let origin: String
    public let uses: UInt32
    public let used: String
    /// Why it is offline; empty when online.
    public let offline: String
}

/// A document's media pool (spec §10.3; plan 5b Task 8): the rows, the
/// pool window's selection, the relink sheet and the pool's actions. The
/// rows are refetched when `DocumentModel.mediaVersion` moves.
@Observable
@MainActor
public final class MediaModel {
    @ObservationIgnored public unowned let document: DocumentModel
    @ObservationIgnored private var cache: (version: Int, rows: [MediaRow])?
    /// The pool window's selected items.
    public var selection: Set<UInt64> = []
    /// The relink sheet is up (Ruling 14).
    public var relinkShown = false

    init(document: DocumentModel) { self.document = document }

    var session: any SessionProtocol { document.session }

    /// The pool. Refetched when the media version moved; a failed read
    /// keeps the last rows. Safe to read while drawing (the cache is not
    /// observed state).
    public var rows: [MediaRow] {
        let v = document.mediaVersion
        if let c = cache, c.version == v { return c.rows }
        let r = (try? session.media()) ?? cache?.rows ?? []
        cache = (v, r)
        return r
    }

    public func row(_ id: UInt64?) -> MediaRow? {
        guard let id else { return nil }
        return rows.first { $0.id == id }
    }

    public func offline(_ id: UInt64?) -> OfflineReasonRow? { row(id)?.offline }

    /// The reasons the relink sheet lists (another format or an unreadable
    /// file get the Convert banner instead).
    public static let relinkReasons: Set<OfflineReasonRow> = [.missing, .length, .changed]

    public var relinkable: [MediaRow] {
        rows.filter { $0.offline.map(Self.relinkReasons.contains) ?? false }
    }

    public var poolRows: [MediaPoolRow] { rows.map(Self.poolRow) }

    public static func poolRow(_ r: MediaRow) -> MediaPoolRow {
        MediaPoolRow(
            id: r.id, name: r.name, duration: duration(frames: r.frames, rate: r.sampleRate),
            channels: r.channels == 1 ? "Mono" : r.channels == 2 ? "Stereo" : "\(r.channels) channels",
            origin: originText(r.origin), uses: r.uses, used: String(format: "%.0f %%", r.usedPercent),
            offline: r.offline.map(reasonText) ?? "")
    }

    /// `m:ss.t`, or `h:mm:ss.t` from an hour on.
    public static func duration(frames: UInt64, rate: UInt32) -> String {
        let tenths = Int((Double(frames) / Double(max(rate, 1)) * 10).rounded())
        let (h, rest) = (tenths / 36_000, tenths % 36_000)
        let (m, t) = (rest / 600, rest % 600)
        let s = String(format: "%02d.%d", t / 10, t % 10)
        return h > 0 ? "\(h):" + String(format: "%02d", m) + ":" + s : "\(m):" + s
    }

    public static func originText(_ o: MediaOriginRow) -> String {
        switch o {
        case .imported: "Imported"
        case .recorded: "Recorded"
        case .bounced: "Bounced"
        case .externalEdit: "External Edit"
        }
    }

    public static func reasonText(_ r: OfflineReasonRow) -> String {
        switch r {
        case .missing: "Missing"
        case .unreadable: "Not a VizDAW file"
        case .format: "Another format"
        case .length: "Changed length"
        case .changed: "Changed"
        }
    }

    // MARK: Actions

    /// A pool edit (one undo step); refused while a drag is open; a
    /// failure is the document's alert.
    @discardableResult
    private func act(_ title: String, _ body: (any SessionProtocol) throws -> DocSnapshot) -> Bool {
        guard document.editor.gesture == nil else {
            document.editor.status = EditorModel.gestureOpenMessage
            return false
        }
        do {
            document.apply(try body(session))
            document.editor.refreshSelection()
            document.mediaVersion += 1
            return true
        } catch {
            document.alert = AlertItem(error, title: title)
            return false
        }
    }

    /// Rename (trimmed; a blank or unchanged name does nothing).
    @discardableResult
    public func rename(_ id: UInt64, to name: String) -> Bool {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !n.isEmpty, n != row(id)?.name else { return false }
        return act("Rename Failed") { try $0.renameMedia(media: id, name: n) }
    }

    /// Compact: every unused item goes (one undo step).
    @discardableResult
    public func compact() -> Bool { act("Compact Failed") { try $0.compactMedia() } }

    /// Deletes the item and its regions (ask with `deleteQuestion` first).
    @discardableResult
    public func remove(_ id: UInt64) -> Bool {
        let ok = act("Delete Failed") { try $0.removeMedia(media: id) }
        if ok { selection.remove(id) }
        return ok
    }

    public func users(_ id: UInt64) -> [MediaUseRow] { (try? session.mediaUsers(media: id)) ?? [] }

    /// What Delete asks first (parent spec §5): nil when no region uses
    /// the item.
    public func deleteQuestion(_ id: UInt64) -> String? {
        let u = users(id)
        guard !u.isEmpty, let r = row(id) else { return nil }
        let places = u.prefix(8).map { use in
            let meter = document.editor.info(use.seq)?.meter ?? []
            return "\(use.sequence) › \(use.laneName), bar \(MeterMath.bar(at: use.tick, meter).number)"
        }
        let more = u.count > 8 ? "\n… and \(u.count - 8) more" : ""
        let n = u.count
        return "“\(r.name)” is used by \(n) region\(n == 1 ? "" : "s"):\n" + places.joined(separator: "\n") + more
            + "\nDelete the item and these regions?"
    }

    /// Selects the regions using `id` in the first sequence that has one,
    /// and shows that sequence (Ruling 13).
    public func selectUsers(_ id: UInt64) {
        let u = users(id)
        guard let first = u.first else {
            document.editor.status = "No region uses this item."
            return
        }
        document.choose(first.seq)
        let mine = u.filter { $0.seq == first.seq }
        var lanes: [LaneRef] = []
        for use in mine where !lanes.contains(LaneRef(seq: use.seq, lane: use.lane)) {
            lanes.append(LaneRef(seq: use.seq, lane: use.lane))
        }
        document.editor.setSelection(SelectionRow(timeRange: nil, lanes: lanes, events: mine.map(\.event)))
    }

    public func selectUnused() { selection = Set(rows.filter { $0.uses == 0 }.map(\.id)) }

    /// Locate…: relinks `id` to the file at `path` (a job).
    public func relink(_ id: UInt64, path: String) {
        document.startJob(.relink, "Relink Failed") { try $0.relinkMedia(media: id, path: path) }
    }

    /// Search folder…: every missing or changed item (a job).
    public func searchFolder(_ path: String) {
        document.startJob(.relink, "Relink Failed") { try $0.searchMedia(folder: path) }
    }

    /// The pool's Import…: into the pool only, no regions.
    public func importIntoPool(_ paths: [String]) { document.importAudio(paths: paths, target: nil) }

    /// The item's file (Reveal in Finder).
    public func file(_ id: UInt64) -> String? { try? session.mediaFile(media: id) }

    /// Shows the relink sheet when items are missing or changed (on open;
    /// Ruling 14).
    public func offerRelinkIfNeeded() {
        if !relinkable.isEmpty { relinkShown = true }
    }

    /// "Leave Offline".
    public func leaveOffline() { relinkShown = false }

    /// Region `event`'s waveform over ticks `[fromTick, toTick)` in
    /// `columns` columns (at most 8192); nil when it cannot be read
    /// (nothing is drawn).
    public func waveform(event: UInt64, fromTick: Int64, toTick: Int64, columns: Int) -> OverviewRows? {
        guard columns > 0, fromTick < toTick else { return nil }
        return try? session.regionWaveform(
            event: event, fromTick: fromTick, toTick: toTick, columns: UInt32(min(columns, 8192)))
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWAppModel/RegionCommands.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// The Region menu (spec §10.2; plan 5b Task 8): one implementation for
/// the menu items and the control channel's `command.run`, acting on the
/// selected events (the core ignores those that are not regions).
@MainActor
public enum RegionCommands {
    public static let normalize = "region.normalize"
    public static let fadeIn = "region.fade_in_to_cursor"
    public static let fadeOut = "region.fade_out_from_cursor"
    public static let removeFades = "region.remove_fades"

    public static let ids: Set<String> = [normalize, fadeIn, fadeOut, removeFades]

    /// The Normalize panel's starting level (Ruling 18).
    public static let defaultPeakDb = -0.3

    static let noSelection = "Select audio regions first."

    public static func enabled(_ id: String, _ m: DocumentModel) -> Bool {
        ids.contains(id) && !m.editor.selection.events.isEmpty && m.editor.gesture == nil
    }

    public static func isOn(_ id: String, _ m: DocumentModel) -> Bool { false }

    /// Runs `id` on the selection. Normalize uses the default level; the
    /// menu item asks for one first and calls `normalize(peakDb:_:)`.
    @discardableResult
    public static func run(_ id: String, on: Bool? = nil, _ m: DocumentModel) -> KeyOutcome {
        guard ids.contains(id) else { return .unavailable("VizDAW has no handler for \(id).") }
        if id == normalize { return Self.normalize(peakDb: defaultPeakDb, m) }
        let events = m.editor.selection.events
        guard !events.isEmpty else { return .unavailable(noSelection) }
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        let cmd: EditCommand =
            id == removeFades
            ? .audio(edit: .removeFades(events: events))
            : .audio(edit: .fadeTo(events: events, at: m.transport.tick, fadeIn: id == fadeIn))
        return m.editor.edit(cmd) ? .done : .refused(m.editor.status ?? "The region command was refused.")
    }

    /// Normalize… to `peakDb` (−96…0 dBFS).
    @discardableResult
    public static func normalize(peakDb: Double, _ m: DocumentModel) -> KeyOutcome {
        let events = m.editor.selection.events
        guard !events.isEmpty else { return .unavailable(noSelection) }
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        guard peakDb.isFinite, (-96...0).contains(peakDb) else {
            return .refused("The peak must be a level from −96 to 0 dB.")
        }
        do {
            m.apply(try m.session.normalizeRegions(events: events, peakDb: Float(peakDb)))
            m.editor.refreshSelection()
            return .done
        } catch VizError.Busy(let message) {
            m.editor.status = message
            return .busy(message)
        } catch {
            m.editor.report(error)
            return .refused(m.editor.status ?? "Normalize was refused.")
        }
    }

    /// The level a text names ("-0.3", "−1", "0"): nil unless it is a
    /// finite number from −96 to 0.
    public static func parsePeak(_ text: String) -> Double? {
        let t = text.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "−", with: "-")
        guard let v = Double(t), v.isFinite, (-96...0).contains(v) else { return nil }
        return v
    }
}

extension RegionCommands: MenuCommands {}
VIZDAW_EOF
```

`Double("0x10")` is 16 in Swift (hex floats parse), which the range check refuses, as the test expects.

- [ ] **Step 4: `DocumentModel` and the job helpers**

In `app/Sources/VizDAWAppModel/DocumentModel.swift`:

Find:
```swift
    /// Bumped when plugin slots change (loaded, failed, restarted): views
    /// that show slots refetch.
    public private(set) var slotsVersion = 0
```
Replace with:
```swift
    /// Bumped when plugin slots change (loaded, failed, restarted): views
    /// that show slots refetch.
    public private(set) var slotsVersion = 0
    /// Bumped when waveform overviews or sample windows became available
    /// (plan 5b): views drawing `Pending` or `Coarse` waveforms redraw.
    public internal(set) var overviewVersion = 0
    @ObservationIgnored private var mediaModel: MediaModel?

    /// The media pool (plan 5b), made once.
    public var media: MediaModel {
        if let m = mediaModel { return m }
        let m = MediaModel(document: self)
        mediaModel = m
        return m
    }
```

Find:
```swift
        editor.layoutChanged = { [weak self] in
            self?.pruneLists()
            self?.pruneStripCharts()
            self?.pruneDrumGrids()
        }
    }
```
Replace with:
```swift
        editor.layoutChanged = { [weak self] in
            self?.pruneLists()
            self?.pruneStripCharts()
            self?.pruneDrumGrids()
        }
        // A project that opens with missing or changed media offers the
        // relink sheet (plan 5b Ruling 14; the app never shows it while
        // the control channel is on).
        media.offerRelinkIfNeeded()
    }
```

Find:
```swift
        if u.mediaChanged {
            mediaVersion += 1
            refreshProjectInfo()
        }
        if !jobs.isEmpty { pollJobs() }
```
Replace with:
```swift
        if u.mediaChanged {
            mediaVersion += 1
            refreshProjectInfo()
        }
        if u.overviewsChanged { overviewVersion += 1 }
        if !jobs.isEmpty { pollJobs() }
```

In `app/Sources/VizDAWAppModel/ProjectMedia.swift`:

Find:
```swift
    /// File › Import Audio…: imports `paths` as a job.
    public func importAudio(paths: [String]) {
        guard !paths.isEmpty else { return }
        let target = Self.importTarget(
            seq: selectedSeq, selection: editor.selection.lanes, files: paths.count, tick: transport.tick)
        start(.import, "Import Failed") { try $0.importAudio(paths: paths, target: target) }
    }

    /// Project Settings: converts every file into `format` (a job).
    public func changeFormat(_ format: ProjectFormatRow) {
        start(.format, "Project Settings Failed") { try $0.setProjectFormat(format: format) }
    }

    /// The banner's Convert.
    public func convertMedia() {
        start(.convert, "Convert Failed") { try $0.convertMedia() }
    }
```
Replace with:
```swift
    /// File › Import Audio…: imports `paths` as a job.
    public func importAudio(paths: [String]) {
        guard !paths.isEmpty else { return }
        let target = Self.importTarget(
            seq: selectedSeq, selection: editor.selection.lanes, files: paths.count, tick: transport.tick)
        importAudio(paths: paths, target: target)
    }

    /// Imports `paths` at `target` (a drop; nil: into the pool only).
    public func importAudio(paths: [String], target: ImportTargetRow?) {
        guard !paths.isEmpty else { return }
        startJob(.import, "Import Failed") { try $0.importAudio(paths: paths, target: target) }
    }

    /// Project Settings: converts every file into `format` (a job).
    public func changeFormat(_ format: ProjectFormatRow) {
        startJob(.format, "Project Settings Failed") { try $0.setProjectFormat(format: format) }
    }

    /// The banner's Convert.
    public func convertMedia() {
        startJob(.convert, "Convert Failed") { try $0.convertMedia() }
    }
```

Find:
```swift
    private func start(_ kind: JobKindRow, _ title: String, _ body: (any SessionProtocol) throws -> UInt64) {
```
Replace with:
```swift
    /// Starts a job and lists it; a refusal is an alert titled `title`.
    func startJob(_ kind: JobKindRow, _ title: String, _ body: (any SessionProtocol) throws -> UInt64) {
```

Find:
```swift
                if r.state != .cancelled {
                    alert = AlertItem(
                        title: r.kind == .import ? "Import Failed" : "Conversion Failed",
                        message: message)
                }
```
Replace with:
```swift
                if r.state != .cancelled {
                    alert = AlertItem(title: ProjectBanner.failureTitle(r.kind), message: message)
                }
```

Find:
```swift
extension DocumentModel {
    /// Where Import Audio puts files (Ruling 19): the selected lane of the
```
Replace with:
```swift
extension ProjectBanner {
    /// A running job's bar text.
    public static func jobText(_ kind: JobKindRow) -> String {
        switch kind {
        case .import: "Importing audio…"
        case .relink: "Relinking media…"
        case .convert, .format: "Converting media…"
        }
    }

    /// The alert title of a failed job.
    public static func failureTitle(_ kind: JobKindRow) -> String {
        switch kind {
        case .import: "Import Failed"
        case .relink: "Relink Failed"
        case .convert, .format: "Conversion Failed"
        }
    }
}

extension DocumentModel {
    /// Where Import Audio puts files (Ruling 19): the selected lane of the
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: every test passes (700 + the 16 new ones).

- [ ] **Step 6: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/MediaModel.swift app/Sources/VizDAWAppModel/RegionCommands.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/ProjectMedia.swift app/Tests/VizDAWAppModelTests/MediaModelTests.swift app/Tests/VizDAWAppModelTests/RegionCommandsTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "app model: MediaModel, Region commands, overview redraws

MediaModel holds the pool rows (refetched by media version), the pool
window's selection and relink sheet, and the pool's actions (rename,
delete with its question, compact, select users or unused, relink and
search jobs, import into the pool, waveforms). RegionCommands runs
Normalize, Fade In to Cursor, Fade Out from Cursor and Remove Fades on
the selection. IdleUpdate.overviewsChanged bumps overviewVersion; drops
import with their own target.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 9: Control channel — media and region commands, `state.get` fields, hostile inputs

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/tests/commands.rs`, `crates/core-ffi/src/control.rs`, `crates/core-ffi/tests/control.rs`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAW/ControlChannel.swift` (`AppControlHost.showMediaPool`), `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`
- Test: `crates/core-control/tests/commands.rs`, `crates/core-ffi/tests/control.rs`, `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`

**Interfaces:**
- Consumes: Task 8's `MediaModel`, `RegionCommands`; Task 6's `Session::relink_media`; 5a's `ControlOutcome.job`, `ControlFailure.{dialog, thrown, of}`, `ControlCommands.failure`.
- Produces:
  - registry entries (in menu order): `media.relink` (File, "Relink Media…", params `media: Int`, `path: File`; dialog "Relink" always, answered by `path`), `media.compact` and `media.rename` (group "Media", no menu item; `media.rename` params `media: Int`, `name: Text`, dialog "Rename Media" always, answered by `name`), `window.media_pool` (Window, "Media Pool"), `region.normalize` (Region, "Normalize…", param `peak_db: Text`, dialog "Normalize" always, answered by `peak_db`), `region.fade_in_to_cursor`, `region.fade_out_from_cursor`, `region.remove_fades` (Region). 111 → 119 commands.
  - `WindowKindRow::MediaPool` (appended; JSON `"media_pool"`); `state.get media` items gain `"used_percent"`.
  - `ControlHost.showMediaPool(_ doc: UUID)`; `ControlCommands` handles all eight.

Ruling 15. The registry validates types and ranges (a negative or huge `media`, a relative `path`, an over-long `name`); the app validates meaning (an unknown item, a blank name, a `peak_db` that is not a level from −96 to 0). Without the parameter that answers its dialog, a command fails `dialog_required` naming the dialog, so a programmatic call never opens one. `media.relink` answers when its job ends (`ControlJobs.answer`).

- [ ] **Step 1: Write the failing tests**

In `crates/core-control/tests/commands.rs`:

Find:
```rust
    assert_eq!(
        COMMANDS.len(),
        111,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media"
    );
```
Replace with:
```rust
    assert_eq!(
        COMMANDS.len(),
        119,
        "44 before plan 4f; 4f adds 51 transport, locator, record and key-binding commands and 13 step entry; 5a adds Project Settings, Import Audio and Convert Media; 5b adds the media pool's four and the Region menu's four"
    );
```

Find:
```rust
#[test]
fn state_get_media_and_project() {
```
Replace with:
```rust
/// Plan 5b: the media pool's and the Region menu's commands take their
/// dialogs' answers as parameters; hostile values are refused by type
/// and range before they reach the app.
#[test]
fn media_and_region_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    // Dialogs without their answers.
    for (id, dialog) in [
        ("media.relink", "Relink"),
        ("media.rename", "Rename Media"),
        ("region.normalize", "Normalize"),
    ] {
        let (code, message) = err("command.run", run(id, json!({})));
        assert_eq!(code, ErrorCode::DialogRequired, "{id}");
        assert!(message.contains(dialog), "{id}: {message}");
    }
    // The good forms.
    let Command::CommandRun { args, .. } = ok(
        "command.run",
        run("media.relink", json!({ "media": 3, "path": "/tmp/x y.wav" })),
    ) else {
        panic!()
    };
    assert_eq!(
        args,
        vec![
            ("media".to_string(), ArgValue::Int(3)),
            ("path".to_string(), ArgValue::Text("/tmp/x y.wav".into())),
        ]
    );
    for (id, params) in [
        ("media.rename", json!({ "media": 0, "name": "Kick" })),
        ("region.normalize", json!({ "peak_db": "-0.3" })),
        ("media.compact", json!({})),
        ("window.media_pool", json!({})),
        ("region.fade_in_to_cursor", json!({})),
        ("region.fade_out_from_cursor", json!({})),
        ("region.remove_fades", json!({})),
    ] {
        assert!(
            matches!(ok("command.run", run(id, params)), Command::CommandRun { .. }),
            "{id}"
        );
    }
    // Hostile values.
    for (id, params) in [
        ("media.relink", json!({ "media": -1, "path": "/a.wav" })),
        ("media.relink", json!({ "media": i64::MAX, "path": "/a.wav" })),
        ("media.relink", json!({ "media": 1.5, "path": "/a.wav" })),
        ("media.relink", json!({ "media": "3", "path": "/a.wav" })),
        ("media.relink", json!({ "media": 3, "path": "relative.wav" })),
        ("media.relink", json!({ "media": 3, "path": "" })),
        ("media.relink", json!({ "media": 3, "path": 7 })),
        ("media.rename", json!({ "media": 3, "name": "x".repeat(100_000) })),
        ("media.rename", json!({ "media": 3, "name": ["x"] })),
        ("region.normalize", json!({ "peak_db": -1 })),
        ("region.normalize", json!({ "peak_db": null, "extra": 1 })),
        ("media.compact", json!({ "media": 1 })),
        ("region.remove_fades", json!({ "events": [1] })),
    ] {
        let (code, _) = err("command.run", run(id, params.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{id} {params}");
    }
    // Where they appear.
    let spec = |id: &str| registry::find(id).unwrap();
    assert_eq!((spec("media.relink").menu, spec("media.relink").menu_item), ("File", true));
    assert_eq!((spec("media.rename").menu, spec("media.rename").menu_item), ("Media", false));
    assert_eq!((spec("media.compact").menu, spec("media.compact").menu_item), ("Media", false));
    assert_eq!(spec("window.media_pool").menu, "Window");
    for id in [
        "region.normalize",
        "region.fade_in_to_cursor",
        "region.fade_out_from_cursor",
        "region.remove_fades",
    ] {
        assert_eq!((spec(id).menu, spec(id).menu_item, spec(id).document), ("Region", true, true), "{id}");
    }
}

#[test]
fn state_get_media_and_project() {
```

In `crates/core-ffi/tests/control.rs`:

Find:
```rust
    assert_eq!(v["items"][0]["offline_reason"], "format");
    assert_eq!(v["items"][0]["offline"], true);
```
Replace with:
```rust
    assert_eq!(v["items"][0]["offline_reason"], "format");
    assert_eq!(v["items"][0]["offline"], true);
    assert_eq!(v["items"][0]["used_percent"], json!(40.0));
    let changed = result_json(&ControlResult::Media {
        doc: "D".into(),
        items: vec![MediaRow {
            id: 3,
            name: "pad".into(),
            path: "Media/pad.caf".into(),
            channels: 2,
            frames: 10,
            sample_rate: 48_000,
            origin: MediaOriginRow::Imported,
            original_name: None,
            offline: Some(OfflineReasonRow::Changed),
            uses: 0,
            used_percent: 0.0,
        }],
    });
    assert_eq!(changed["items"][0]["offline_reason"], "changed");
```

In `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`:

Find:
```swift
    func closeDocument(_ doc: UUID) { calls.append("close") }
}
```
Replace with:
```swift
    func closeDocument(_ doc: UUID) { calls.append("close") }
    func showMediaPool(_ doc: UUID) { calls.append("media pool") }
}
```

Find:
```swift
    // MARK: Fix round 1: how a finished job is answered (I-2)
```
Replace with:
```swift
    // MARK: Plan 5b: the media pool and the Region menu

    func args(_ pairs: [(String, ControlValue)]) -> [ControlArg] {
        pairs.map { ControlArg(name: $0.0, value: $0.1) }
    }

    @Test func relinkTakesTheItemAndTheFileAndAnswersWhenTheJobEnds() throws {
        let (store, host, id, model, fake) = try makeDocument()
        fake.mediaRows = [mediaRow(3, "kick", offline: .missing)]
        model.refreshAfterJob()
        let run = { (a: [ControlArg]) in
            ControlCommands.run("media.relink", args: a, doc: (id, model), store: store, host: host)
        }
        #expect(failure(run([]))?.code == .dialogRequired)
        #expect(failure(run(args([("path", .text(value: "/x/kick.wav"))])))?.code == .invalidParams)
        #expect(
            failure(run(args([("path", .text(value: "/x/kick.wav")), ("media", .int(value: 99))])))?.code
                == .invalidParams)
        let out = run(args([("path", .text(value: "/x/kick.wav")), ("media", .int(value: 3))]))
        guard case .job(let command, _, _) = out else {
            Issue.record("\(out)")
            return
        }
        #expect(command == "media.relink")
        #expect(fake.relinks.first?.media == 3 && fake.relinks.first?.path == "/x/kick.wav")
    }

    @Test func renameAndCompactRunWithoutDialogs() throws {
        let (store, host, id, model, fake) = try makeDocument()
        fake.mediaRows = [mediaRow(3, "kick")]
        model.refreshAfterJob()
        let run = { (cmd: String, a: [ControlArg]) in
            ControlCommands.run(cmd, args: a, doc: (id, model), store: store, host: host)
        }
        #expect(failure(run("media.rename", args([("media", .int(value: 3))])))?.code == .dialogRequired)
        #expect(
            failure(run("media.rename", args([("media", .int(value: 3)), ("name", .text(value: "   "))])))?.code
                == .invalidParams)
        #expect(
            failure(run("media.rename", args([("media", .int(value: 7)), ("name", .text(value: "x"))])))?.code
                == .invalidParams)
        #expect(failure(run("media.rename", args([("name", .text(value: "x"))])))?.code == .invalidParams)
        guard case .done = run("media.rename", args([("media", .int(value: 3)), ("name", .text(value: "Kick"))]))
        else {
            Issue.record("rename")
            return
        }
        #expect(fake.renamed.map(\.1) == ["Kick"])
        guard case .done = run("media.compact", []) else {
            Issue.record("compact")
            return
        }
        #expect(fake.compactions == 1)
        fake.poolError = .Model(message: "nothing to compact")
        #expect(failure(run("media.compact", []))?.message.contains("nothing to compact") == true)
        guard case .done = run("window.media_pool", []) else {
            Issue.record("window")
            return
        }
        #expect(host.calls.contains("media pool"))
    }

    @Test func regionCommandsActOnTheSelectionAndCheckThePeak() throws {
        let (store, host, id, model, fake) = try makeDocument()
        let run = { (cmd: String, a: [ControlArg]) in
            ControlCommands.run(cmd, args: a, doc: (id, model), store: store, host: host)
        }
        #expect(failure(run(RegionCommands.normalize, []))?.code == .dialogRequired)
        for bad in ["nan", "inf", "1", "-97", "abc", "", "-1e999"] {
            #expect(
                failure(run(RegionCommands.normalize, args([("peak_db", .text(value: bad))])))?.code
                    == .invalidParams, "\(bad)")
        }
        // No selection: unavailable, nothing sent.
        #expect(failure(run(RegionCommands.normalize, args([("peak_db", .text(value: "-1"))])))?.code == .unavailable)
        #expect(failure(run(RegionCommands.removeFades, []))?.code == .unavailable)
        model.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: [5]))
        guard case .done = run(RegionCommands.normalize, args([("peak_db", .text(value: "-1"))])) else {
            Issue.record("normalize")
            return
        }
        #expect(fake.normalizations.last?.peakDb == -1)
        fake.normalizeError = .Busy(message: "still being built")
        #expect(failure(run(RegionCommands.normalize, args([("peak_db", .text(value: "-1"))])))?.code == .busy)
        guard case .done = run(RegionCommands.fadeIn, []) else {
            Issue.record("fade")
            return
        }
        #expect(fake.edits.last == .audio(edit: .fadeTo(events: [5], at: 0, fadeIn: true)))
    }

    // MARK: Fix round 1: how a finished job is answered (I-2)
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands 2>&1 | grep -E "^test result|panicked" | head
cargo test -p core-ffi --test control 2>&1 | grep -E "^test result|panicked" | head
```
Expected: `core-control` fails (119 ≠ 111, unknown `media.relink`); `core-ffi` fails (`used_percent` missing from the JSON).

- [ ] **Step 3: The registry**

In `crates/core-control/src/registry.rs`:

Find:
```rust
/// Every menu command, in menu order.
pub const COMMANDS: &[CommandSpec] = &[
```
Replace with:
```rust
/// A media item by ID (plan 5b).
const MEDIA_ID: ParamSpec = ParamSpec {
    name: "media",
    kind: ParamKind::Int {
        min: 0,
        max: i64::MAX / 2,
    },
    required: false,
    help: "The media item (see state.get media).",
};

const RELINK_PARAMS: &[ParamSpec] = &[
    MEDIA_ID,
    ParamSpec {
        name: "path",
        kind: ParamKind::File,
        required: false,
        help: "The file to relink the item to (answers the Relink dialog).",
    },
];

const RENAME_PARAMS: &[ParamSpec] = &[
    MEDIA_ID,
    ParamSpec {
        name: "name",
        kind: ParamKind::Text,
        required: false,
        help: "The item's new name (answers the Rename Media dialog).",
    },
];

const NORMALIZE_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "peak_db",
    kind: ParamKind::Text,
    required: false,
    help: "The peak level in dBFS, -96 to 0, as text (\"-0.3\"); answers the Normalize dialog.",
}];

/// Every menu command, in menu order.
pub const COMMANDS: &[CommandSpec] = &[
```

Find:
```rust
    CommandSpec {
        menu_item: false,
        ..cmd(
            "media.convert",
            "File",
            "Convert Media to Project Format",
            None,
            true,
        )
    },
```
Replace with:
```rust
    CommandSpec {
        menu_item: false,
        ..cmd(
            "media.convert",
            "File",
            "Convert Media to Project Format",
            None,
            true,
        )
    },
    CommandSpec {
        params: RELINK_PARAMS,
        dialogs: &[DialogSpec {
            name: "Relink",
            when: "always",
            always: true,
            answered_by: &["path"],
        }],
        ..cmd("media.relink", "File", "Relink Media…", None, true)
    },
    key_only(cmd("media.compact", "Media", "Compact Media", None, true)),
    CommandSpec {
        params: RENAME_PARAMS,
        dialogs: &[DialogSpec {
            name: "Rename Media",
            when: "always",
            always: true,
            answered_by: &["name"],
        }],
        ..key_only(cmd("media.rename", "Media", "Rename Media…", None, true))
    },
```

Find:
```rust
    cmd(
        "window.plugin_manager",
        "Window",
        "Plugin Manager",
        Some("⌥⌘P"),
        false,
    ),
```
Replace with:
```rust
    cmd(
        "window.plugin_manager",
        "Window",
        "Plugin Manager",
        Some("⌥⌘P"),
        false,
    ),
    cmd("window.media_pool", "Window", "Media Pool", None, true),
```

Find:
```rust
    CommandSpec {
        params: ON,
        ..cmd("drum.timeline", "Drum", "Timeline View", None, true)
    },
```
Replace with:
```rust
    CommandSpec {
        params: ON,
        ..cmd("drum.timeline", "Drum", "Timeline View", None, true)
    },
    CommandSpec {
        params: NORMALIZE_PARAMS,
        dialogs: &[DialogSpec {
            name: "Normalize",
            when: "always",
            always: true,
            answered_by: &["peak_db"],
        }],
        ..cmd("region.normalize", "Region", "Normalize…", None, true)
    },
    cmd(
        "region.fade_in_to_cursor",
        "Region",
        "Fade In to Cursor",
        None,
        true,
    ),
    cmd(
        "region.fade_out_from_cursor",
        "Region",
        "Fade Out from Cursor",
        None,
        true,
    ),
    cmd("region.remove_fades", "Region", "Remove Fades", None, true),
```

`key_only` is a `const fn` defined above `COMMANDS`; struct-update syntax over it (`..key_only(...)`) is allowed in a `const`.

- [ ] **Step 4: JSON and the window kind**

In `crates/core-ffi/src/control.rs`:

Find:
```rust
    /// Anything else (menus' windows are not listed).
    Other,
}
```
Replace with:
```rust
    /// Anything else (menus' windows are not listed).
    Other,
    /// A media pool window (plan 5b).
    MediaPool,
}
```

Find:
```rust
        WindowKindRow::Other => "other",
    }
}
```
Replace with:
```rust
        WindowKindRow::Other => "other",
        WindowKindRow::MediaPool => "media_pool",
    }
}
```

Find:
```rust
                "uses": m.uses,
            })).collect::<Vec<_>>(),
```
Replace with:
```rust
                "uses": m.uses,
                "used_percent": m.used_percent,
            })).collect::<Vec<_>>(),
```

- [ ] **Step 5: The app's handlers**

In `app/Sources/VizDAWAppModel/ControlCommands.swift`:

Find:
```swift
    func closeDocument(_ doc: UUID)
    var nudge: NudgeSettings { get }
```
Replace with:
```swift
    func closeDocument(_ doc: UUID)
    /// Opens document `doc`'s media pool window (plan 5b).
    func showMediaPool(_ doc: UUID)
    var nudge: NudgeSettings { get }
```

Find:
```swift
        "file.project_settings", "media.import", "media.convert",
    ]
```
Replace with:
```swift
        "file.project_settings", "media.import", "media.convert",
        "media.relink", "media.compact", "media.rename", "window.media_pool",
    ]
```

Find:
```swift
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
            .union(fieldKeys.keys)
```
Replace with:
```swift
        other.union(detail.keys).union(torn.keys).union(nudges.keys).union(panels.keys).union(DrumCommands.ids)
            .union(fieldKeys.keys).union(RegionCommands.ids)
```

Find:
```swift
        case "window.mixer":
            host.showMixer(docID)
```
Replace with:
```swift
        case "window.mixer":
            host.showMixer(docID)
        case "window.media_pool":
            host.showMediaPool(docID)
        case "media.relink":
            guard let path = args.text("path") else { return .failed(.dialog("Relink", "path")) }
            guard let media = args.int("media").map(UInt64.init), m.media.row(media) != nil else {
                return .failed(.of(.invalidParams, "media.relink: `media` must name a media item (see state.get media)"))
            }
            do {
                return .job(command: id, doc: docID, job: try m.session.relinkMedia(media: media, path: path))
            } catch {
                return .failed(.thrown(error, "Relink Failed"))
            }
        case "media.compact":
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            m.media.compact()
        case "media.rename":
            guard let name = args.text("name") else { return .failed(.dialog("Rename Media", "name")) }
            guard let media = args.int("media").map(UInt64.init), m.media.row(media) != nil else {
                return .failed(.of(.invalidParams, "media.rename: `media` must name a media item (see state.get media)"))
            }
            guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                return .failed(.of(.invalidParams, "media.rename: the name is empty"))
            }
            guard m.editor.gesture == nil else { return unavailable(EditorModel.gestureOpenMessage) }
            m.media.rename(media, to: name)
        case RegionCommands.normalize:
            guard let text = args.text("peak_db") else { return .failed(.dialog("Normalize", "peak_db")) }
            guard let peak = RegionCommands.parsePeak(text) else {
                return .failed(.of(.invalidParams, "`peak_db` must be a level from -96 to 0 dB, like \"-0.3\""))
            }
            if let f = failure(RegionCommands.normalize(peakDb: peak, m)) { return f }
        case _ where RegionCommands.ids.contains(id):
            if let f = failure(RegionCommands.run(id, m)) { return f }
```

`m.media.compact()` and `m.media.rename(…)` report a failure as the document's alert; the generic check at the end of `run` turns that alert into the call's failure.

In `app/Sources/VizDAW/ControlChannel.swift`:

Find:
```swift
    func showPluginManager() { ControlWindows.openWindow?(id: "plugins") }
```
Replace with:
```swift
    func showPluginManager() { ControlWindows.openWindow?(id: "plugins") }
    func showMediaPool(_ doc: UUID) { ControlWindows.openWindow?(id: "media-pool", value: doc) }
```

(The `media-pool` window group is added in Task 12; until then the call opens nothing, as `openWindow` ignores unknown IDs.)

- [ ] **Step 6: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control 2>&1 | grep -E "^test result|panicked"
cargo test -p core-ffi --test control 2>&1 | grep -E "^test result|panicked"
scripts/build-ffi.sh && scripts/swift.sh test 2>&1 | tail -3
```
Expected: all pass, including `every_registry_entry_survives_random_params` (now over 119 entries) and Swift's `everyRegisteredMenuCommandHasAHandler`.

- [ ] **Step 7: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```

- [ ] **Step 8: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs crates/core-ffi/src/control.rs crates/core-ffi/tests/control.rs app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAW/ControlChannel.swift app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift
git commit -m "control: media.relink/compact/rename, Media Pool window, Region menu

Eight registry commands (111 -> 119): media.relink answers the Relink
dialog with path and waits for its job, media.rename answers Rename
Media with name, media.compact, window.media_pool, and the Region menu
(region.normalize with a peak_db text, fades to the cursor, Remove
Fades) on the selection. state.get media gains used_percent and the
changed reason; windows report media_pool. Hostile parameters are
refused by type and range in the registry and by meaning in the app.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 10: Editors — waveforms, fade and gain handles, fade shapes, offline hatching

**Files:**
- Create: `app/Sources/VizDAWEditors/RegionEditing.swift` (pure geometry), `app/Sources/VizDAWEditors/RegionDrawing.swift` (palette and drawing), `app/Tests/VizDAWEditorsTests/RegionEditingTests.swift`, `app/Tests/VizDAWAppModelTests/RegionHandleViewTests.swift`
- Modify: `app/Sources/VizDAWEditors/ArrangementLayout.swift`, `app/Sources/VizDAWEditors/ArrangementView.swift`, `app/Sources/VizDAWEditors/RegionStrip.swift`, `app/Sources/VizDAWEditors/RegionStripView.swift`
- Test: `app/Tests/VizDAWEditorsTests/RegionEditingTests.swift`, `app/Tests/VizDAWAppModelTests/RegionHandleViewTests.swift`

**Interfaces:**
- Consumes: Task 8's `DocumentModel.{media, overviewVersion, mediaVersion}`, `MediaModel.{waveform, offline, row}`; `OverviewRows`, `OverviewStateRow`; `AudioEdit.{setFades, setGain}`; 5a/4x `EditorModel.{beginGesture, updateGesture, endGesture, cancelGesture, edit, setSelection, isSelected}`, `ToolMachine`, `MenuAction`, `MeterMath`.
- Produces (VizDAWEditors):
  - `RegionHandle.{fadeIn, fadeOut, gain}`; `RegionHandleHit { lane: LaneKey, row: EventRow, handle: RegionHandle, rect: CGRect }`;
  - `RegionHandles`: `size` 7, `band` 4, `minWidth` 24, `dbPerPoint` 0.25, `fineDbPerPoint` 0.05, `shapes`; `fadeInX(_:in:)`, `fadeOutX(_:in:)`, `rects(_:in:)`, `hit(_:row:in:)`, `fades(_:_:dx:in:)`, `fadeCommand(_:_:dx:in:)`, `gain(_:dy:fine:)`, `gainCommand(_:dy:fine:)`, `resetGain(_:)`, `shapeCommand(_:_:_:)`, `fadeArea(_:row:in:)`, `gainLabel(_:)`, `label(_:)`;
  - `WaveformGeometry.bars(_ rows: OverviewRows, in: CGRect) -> [CGRect]`;
  - `RegionA11y.label(_:name:laneName:meter:offline:) -> String`;
  - `RegionPalette` (Ruling 16); `RegionDrawing.{draw(_:in:rows:waveRect:offline:selected:handles:), readout(_:at:), fadeShapeMenu(_:_:apply:)}`;
  - `ArrangementHit.handle(RegionHandleHit)`; `ArrangementGeometry.regionRect(_:row:axis:)`;
  - `RegionHit.handle(RegionItem, RegionHandle, CGRect)`; `RegionStripLayout.handleHit(_:lines:time:side:) -> RegionHit?`.

Rulings 4, 7 and 16; spec §10.1–10.2. Both editors draw a region as its body, its waveform (from `MediaModel.waveform`, already scaled by gain and shaped by fades in Rust), its fade lines, a gain line when the gain is not 0 dB, its outline and, with the arrow tool, its handles; offline regions are hatched and say "Offline". A handle press opens a gesture ("Fade In", "Fade Out" or "Region Gain", origin `.other`); every pointer move sends the full target; the gain band shows a ±dB readout while dragged. Regions are accessibility elements of the arrangement ("Region kick on Audio 3, bar 1, gain +3.0 dB, fades").

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/RegionEditingTests.swift <<'VIZDAW_EOF'
import CoreGraphics
import Foundation
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

/// 2 s at 48 kHz with the given fades and gain.
private func region(fadeIn: Int64 = 0, fadeOut: Int64 = 0, gain: Float = 0) -> EventRow {
    var r = Pencil.note(tick: 0, pitch: 0, length: 61_440)
    r.id = 4
    r.kind = .audioRegion
    r.media = 7
    r.frames = 96_000
    r.gainDb = gain
    r.fadeIn = FadeRow(length: fadeIn, shape: .linear)
    r.fadeOut = FadeRow(length: fadeOut, shape: .equalPower)
    return r
}

/// 160 points wide: 600 frames per point.
private let box = CGRect(x: 0, y: 10, width: 160, height: 40)

/// Spec §12.3: "handle hit-testing and drag maths for fades and gain".
@Suite struct RegionHandlesTests {
    @Test func handlesSitAtTheFadeEndsAndTheTopEdge() {
        let r = region(fadeIn: 12_000, fadeOut: 24_000)
        #expect(RegionHandles.fadeInX(r, in: box) == 20)
        #expect(RegionHandles.fadeOutX(r, in: box) == 120)
        #expect(RegionHandles.hit(CGPoint(x: 20, y: 12), row: r, in: box) == .fadeIn)
        #expect(RegionHandles.hit(CGPoint(x: 121, y: 13), row: r, in: box) == .fadeOut)
        #expect(RegionHandles.hit(CGPoint(x: 70, y: 11), row: r, in: box) == .gain)
        #expect(RegionHandles.hit(CGPoint(x: 70, y: 30), row: r, in: box) == nil, "the body is not a handle")
        // Too narrow for handles.
        let narrow = CGRect(x: 0, y: 10, width: 20, height: 40)
        #expect(RegionHandles.rects(r, in: narrow).isEmpty)
        #expect(RegionHandles.hit(CGPoint(x: 1, y: 11), row: r, in: narrow) == nil)
        // No frames: the handles sit at the edges, nothing divides by zero.
        var empty = r
        empty.frames = 0
        #expect(RegionHandles.fadeInX(empty, in: box).isFinite)
    }

    @Test func fadeDragsKeepBothFadesInsideTheRegion() {
        let r = region(fadeIn: 12_000, fadeOut: 24_000)
        #expect(RegionHandles.fades(r, .fadeIn, dx: 40, in: box) == (36_000, 24_000))
        #expect(RegionHandles.fades(r, .fadeIn, dx: -100, in: box) == (0, 24_000))
        #expect(RegionHandles.fades(r, .fadeIn, dx: 1_000, in: box) == (72_000, 24_000), "up to the fade out")
        #expect(RegionHandles.fades(r, .fadeOut, dx: -20, in: box) == (12_000, 36_000))
        #expect(RegionHandles.fades(r, .fadeOut, dx: 50, in: box) == (12_000, 0))
        #expect(RegionHandles.fadeCommand(r, .fadeIn, dx: 0, in: box) == nil)
        #expect(
            RegionHandles.fadeCommand(r, .fadeIn, dx: 40, in: box)
                == .audio(
                    edit: .setFades(
                        event: 4, fadeIn: FadeRow(length: 36_000, shape: .linear),
                        fadeOut: FadeRow(length: 24_000, shape: .equalPower))))
    }

    @Test func gainDragsInTenthsOfADecibelWithinTheRange() {
        let r = region()
        #expect(RegionHandles.gain(r, dy: -20, fine: false) == 5)
        #expect(RegionHandles.gain(r, dy: -20, fine: true) == 1)
        #expect(RegionHandles.gain(r, dy: 10, fine: false) == -2.5)
        #expect(RegionHandles.gain(r, dy: 1_000, fine: false) == -144)
        #expect(RegionHandles.gain(r, dy: -1_000, fine: false) == 24)
        #expect(RegionHandles.gain(r, dy: -1, fine: false) == 0.3)
        #expect(RegionHandles.gainCommand(r, dy: 0, fine: false) == nil)
        #expect(RegionHandles.gainCommand(r, dy: -20, fine: false) == .audio(edit: .setGain(event: 4, gainDb: 5)))
        #expect(RegionHandles.resetGain(r) == nil)
        #expect(RegionHandles.resetGain(region(gain: 3)) == .audio(edit: .setGain(event: 4, gainDb: 0)))
        #expect(RegionHandles.gainLabel(0) == "0.0 dB")
        #expect(RegionHandles.gainLabel(0.04) == "0.0 dB")
        #expect(RegionHandles.gainLabel(3) == "+3.0 dB")
        #expect(RegionHandles.gainLabel(-6.5) == "-6.5 dB")
        #expect(RegionHandles.label(.gain) == "Region Gain")
        #expect(RegionHandles.label(.fadeIn) == "Fade In")
    }

    @Test func fadeShapesAreSetFromTheFadesArea() {
        let r = region(fadeIn: 12_000, fadeOut: 24_000)
        #expect(RegionHandles.fadeArea(CGPoint(x: 10, y: 30), row: r, in: box) == .fadeIn)
        #expect(RegionHandles.fadeArea(CGPoint(x: 140, y: 30), row: r, in: box) == .fadeOut)
        #expect(RegionHandles.fadeArea(CGPoint(x: 70, y: 30), row: r, in: box) == nil)
        #expect(RegionHandles.fadeArea(CGPoint(x: 10, y: 30), row: region(), in: box) == nil, "no fade, no menu")
        #expect(
            RegionHandles.shapeCommand(r, .fadeIn, .sCurve)
                == .audio(
                    edit: .setFades(
                        event: 4, fadeIn: FadeRow(length: 12_000, shape: .sCurve),
                        fadeOut: FadeRow(length: 24_000, shape: .equalPower))))
        #expect(RegionHandles.shapeCommand(r, .fadeOut, .equalPower) == nil, "already that shape")
        #expect(RegionHandles.shapeCommand(r, .gain, .linear) == nil)
        #expect(RegionHandles.shapes.map(\.1) == ["Linear", "Equal Power", "Exponential", "S-Curve"])
    }
}

/// Spec §12.3: "the overview drawing geometry".
@Suite struct WaveformGeometryTests {
    func rows(_ ch: UInt16, _ min: [Float], _ max: [Float], state: OverviewStateRow = .ready) -> OverviewRows {
        OverviewRows(state: state, channels: ch, columns: UInt32(min.count / Int(ch)), min: min, max: max)
    }

    @Test func barsSpanEachColumnsMinToMax() {
        let b = WaveformGeometry.bars(rows(1, [-1, -0.5, 0, 0.5], [1, 0.5, 0, 1]), in: CGRect(x: 0, y: 0, width: 40, height: 20))
        #expect(b.count == 4)
        #expect(b[0] == CGRect(x: 0, y: 0, width: 10, height: 20))
        #expect(b[1] == CGRect(x: 10, y: 5, width: 10, height: 10))
        #expect(b[2] == CGRect(x: 20, y: 10, width: 10, height: 0.5), "silence is a hairline")
        #expect(b[3] == CGRect(x: 30, y: 0, width: 10, height: 5))
    }

    @Test func channelsStackAndOversClip() {
        let b = WaveformGeometry.bars(rows(2, [-0.5, -3], [0.5, 2]), in: CGRect(x: 0, y: 0, width: 10, height: 40))
        #expect(b.count == 2)
        #expect(b[0] == CGRect(x: 0, y: 5, width: 10, height: 10), "left channel, top half")
        #expect(b[1] == CGRect(x: 0, y: 20, width: 10, height: 20), "right channel clipped to its half")
    }

    /// Review Focus 5.
    @Test func degenerateRowsDrawNothing() {
        let r = CGRect(x: 0, y: 0, width: 10, height: 10)
        #expect(WaveformGeometry.bars(rows(1, [], []), in: r).isEmpty)
        #expect(WaveformGeometry.bars(OverviewRows(state: .ready, channels: 1, columns: 4, min: [0], max: [0]), in: r).isEmpty)
        #expect(WaveformGeometry.bars(OverviewRows(state: .ready, channels: 0, columns: 1, min: [0], max: [0]), in: r).isEmpty)
        #expect(WaveformGeometry.bars(rows(1, [0], [0]), in: CGRect(x: 0, y: 0, width: 0, height: 10)).isEmpty)
        let nan = WaveformGeometry.bars(rows(1, [.nan, -.infinity], [.infinity, .nan]), in: CGRect(x: 0, y: 0, width: 20, height: 10))
        #expect(nan.allSatisfy { $0.minY.isFinite && $0.height.isFinite && $0.height >= 0.5 })
        let many = WaveformGeometry.bars(
            rows(1, Array(repeating: 0, count: 8192), Array(repeating: 0.1, count: 8192)), in: r)
        #expect(many.count == 8192)
    }

    @Test func regionLabelsSayWhereWhatAndHowLoud() {
        let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]
        var r = region(fadeIn: 10, gain: 3)
        #expect(
            RegionA11y.label(r, name: "kick", laneName: "Audio 3", meter: meter, offline: false)
                == "Region kick on Audio 3, bar 1, gain +3.0 dB, fades")
        r.duration = 2 * 61_440
        r.fadeIn = FadeRow(length: 0, shape: .linear)
        r.mute = true
        #expect(
            RegionA11y.label(r, name: "kick", laneName: "Audio 3", meter: meter, offline: true)
                == "Region kick on Audio 3, bars 1 to 2, gain +3.0 dB, offline, muted")
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWAppModelTests/RegionHandleViewTests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// Song's Chords lane (row 1, y 48–96) holds one region instead of a note:
/// 2 s (96 000 frames) over bar 1, x 0–160 at the default zoom.
@MainActor
private func regionArrangement(gain: Float = 0, offline: OfflineReasonRow? = nil) throws
    -> (DocumentModel, ArrangementView, FakeSession)
{
    let f = editorFake()
    f.laneEventTable["3/31"] = LaneEvents(
        seq: 3, lane: 31, rev: 2, events: [regionRow(1, tick: 0, duration: 61_440, frames: 96_000, gainDb: gain)])
    f.mediaRows = [mediaRow(7, "loop", offline: offline)]
    let m = try DocumentModel(session: f)
    let v = ArrangementView(document: m)
    v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
    v.layout()
    f.calls = []
    return (m, v, f)
}

/// The region's rectangle: 51–93 in y (a 3-point inset in the 48-point row).
private let top: CGFloat = 51

@MainActor
@Suite struct RegionHandleViewTests {
    @Test func draggingTheFadeInCornerIsOneFadeInStep() throws {
        let (m, v, f) = try regionArrangement()
        v.contentMouseDown(CGPoint(x: 1, y: top + 2), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 41, y: top + 2), modifiers: [])
        v.contentMouseUp(CGPoint(x: 41, y: top + 2), modifiers: [])
        #expect(f.calls == ["begin Fade In", "gesture", "end"])
        #expect(
            f.gestureEdits.last
                == .audio(
                    edit: .setFades(
                        event: 1, fadeIn: FadeRow(length: 24_000, shape: .linear),
                        fadeOut: FadeRow(length: 0, shape: .linear))))
        #expect(m.editor.gesture == nil)
    }

    @Test func draggingTheTopEdgeChangesTheGain() throws {
        let (_, v, f) = try regionArrangement()
        v.contentMouseDown(CGPoint(x: 80, y: top + 1), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 80, y: top - 19), modifiers: [])
        v.contentMouseUp(CGPoint(x: 80, y: top - 19), modifiers: [])
        #expect(f.calls == ["begin Region Gain", "gesture", "end"])
        #expect(f.gestureEdits.last == .audio(edit: .setGain(event: 1, gainDb: 5)))
    }

    @Test func optionClickOnTheTopEdgeResetsTheGain() throws {
        let (_, v, f) = try regionArrangement(gain: 3)
        v.contentMouseDown(CGPoint(x: 80, y: top + 1), clicks: 1, modifiers: [.option])
        v.contentMouseUp(CGPoint(x: 80, y: top + 1), modifiers: [.option])
        #expect(f.edits.last == .audio(edit: .setGain(event: 1, gainDb: 0)))
    }

    @Test func theBodyStillMovesTheBlock() throws {
        let (_, v, f) = try regionArrangement()
        v.contentMouseDown(CGPoint(x: 80, y: 75), clicks: 1, modifiers: [])
        v.contentMouseDragged(CGPoint(x: 300, y: 75), modifiers: [])
        v.contentMouseUp(CGPoint(x: 300, y: 75), modifiers: [])
        #expect(f.calls == ["begin Move Block", "gesture", "end"])
    }

    @Test func drawingAsksForTheVisibleWaveform() throws {
        let (_, v, f) = try regionArrangement()
        let rep = try #require(v.content.bitmapImageRepForCachingDisplay(in: v.content.bounds))
        v.content.cacheDisplay(in: v.content.bounds, to: rep)
        let call = try #require(f.waveformCalls.last)
        #expect(call.event == 1)
        #expect(call.columns >= 159 && call.columns <= 161, "one column per point")
        #expect(call.from <= 0 && call.to >= 61_000)
    }

    @Test func offlineRegionsAreNotRead() throws {
        let (_, v, f) = try regionArrangement(offline: .missing)
        let rep = try #require(v.content.bitmapImageRepForCachingDisplay(in: v.content.bounds))
        v.content.cacheDisplay(in: v.content.bounds, to: rep)
        #expect(f.waveformCalls.isEmpty)
        let labels = v.accessibilityItems().map(\.label)
        #expect(labels.contains("Region loop on Chords, bar 1, gain 0.0 dB, offline"))
    }

    @Test func regionsAreAccessibilityElementsThatSelect() throws {
        let (m, v, _) = try regionArrangement(gain: 3)
        let item = try #require(v.accessibilityItems().first { $0.label.hasPrefix("Region loop") })
        #expect(item.label == "Region loop on Chords, bar 1, gain +3.0 dB")
        item.press?()
        #expect(m.editor.selection.events == [1])
    }

    @Test func theRegionStripHasHandlesToo() throws {
        let f = audioFake()
        f.laneEventTable["3/33"] = LaneEvents(
            seq: 3, lane: 33, rev: 4, events: [regionRow(8, tick: 0, duration: 61_440, frames: 96_000)])
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let v = RegionStripView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 60)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        let side = PianoRollContainer.sideWidth
        f.calls = []
        v.pressed(at: CGPoint(x: side + 300, y: 3), clicks: 1, modifiers: [])
        v.dragged(to: CGPoint(x: side + 300, y: -17), modifiers: [])
        v.released(at: CGPoint(x: side + 300, y: -17), modifiers: [])
        #expect(f.calls == ["begin Region Gain", "gesture", "end"])
        #expect(f.gestureEdits.last == .audio(edit: .setGain(event: 8, gainDb: 5)))
        // The body still moves the region.
        f.calls = []
        v.pressed(at: CGPoint(x: side + 50, y: 11), clicks: 1, modifiers: [])
        v.dragged(to: CGPoint(x: side + 150, y: 11), modifiers: [])
        v.released(at: CGPoint(x: side + 150, y: 11), modifiers: [])
        #expect(f.calls.first == "begin Move Region")
    }
}
VIZDAW_EOF
```

The fake's `calls` names: `beginEdit` records `"begin <label>"`, `editGesture` `"gesture"`, `endEdit` `"end"` (as the 4a arrangement tests rely on).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `scripts/build-ffi.sh && scripts/swift.sh test 2>&1 | tail -5`
Expected: compile errors (`RegionHandles`, `WaveformGeometry`, `RegionA11y` not found).

- [ ] **Step 3: The geometry**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/RegionEditing.swift <<'VIZDAW_EOF'
import CoreGraphics
import Foundation
import VizDAWAppModel
import VizDAWCore

/// The parts of an audio region a press can grab (spec §10.2).
public enum RegionHandle: Equatable, Sendable {
    case fadeIn, fadeOut, gain
}

/// A handle under the pointer: its region (lane and row) and the region's
/// rectangle when pressed.
public struct RegionHandleHit: Equatable, Sendable {
    public let lane: LaneKey
    public let row: EventRow
    public let handle: RegionHandle
    public let rect: CGRect

    public init(lane: LaneKey, row: EventRow, handle: RegionHandle, rect: CGRect) {
        self.lane = lane
        self.row = row
        self.handle = handle
        self.rect = rect
    }
}

/// Fade and gain handles (plan 5b Ruling 7). Pure: where the handles are,
/// what a press hits, and the full target of a drag.
public enum RegionHandles {
    /// A corner handle's side.
    public static let size: CGFloat = 7
    /// The gain band's height along the top edge.
    public static let band: CGFloat = 4
    /// Narrower regions show no handles.
    public static let minWidth: CGFloat = 24
    public static let dbPerPoint: Float = 0.25
    /// With ⇧.
    public static let fineDbPerPoint: Float = 0.05
    /// The fade shapes, as the menu names them.
    public static let shapes: [(FadeShapeRow, String)] = [
        (.linear, "Linear"), (.equalPower, "Equal Power"), (.exponential, "Exponential"), (.sCurve, "S-Curve"),
    ]

    /// The gesture's (and undo step's) name.
    public static func label(_ h: RegionHandle) -> String {
        switch h {
        case .fadeIn: "Fade In"
        case .fadeOut: "Fade Out"
        case .gain: "Region Gain"
        }
    }

    static func framesPerPoint(_ r: EventRow, _ rect: CGRect) -> Double {
        Double(r.frames) / Double(max(rect.width, 1))
    }

    static func share(_ frames: Int64, _ r: EventRow) -> CGFloat {
        CGFloat(Double(max(frames, 0)) / Double(max(r.frames, 1)))
    }

    /// Where the fade in ends.
    public static func fadeInX(_ r: EventRow, in rect: CGRect) -> CGFloat {
        rect.minX + min(share(r.fadeIn.length, r), 1) * rect.width
    }

    /// Where the fade out starts.
    public static func fadeOutX(_ r: EventRow, in rect: CGRect) -> CGFloat {
        rect.maxX - min(share(r.fadeOut.length, r), 1) * rect.width
    }

    /// The handles of `r` in `rect` (none when it is narrower than
    /// `minWidth`).
    public static func rects(_ r: EventRow, in rect: CGRect) -> [(RegionHandle, CGRect)] {
        guard rect.width >= minWidth, rect.height >= size else { return [] }
        return [
            (.fadeIn, CGRect(x: fadeInX(r, in: rect) - size / 2, y: rect.minY, width: size, height: size)),
            (.fadeOut, CGRect(x: fadeOutX(r, in: rect) - size / 2, y: rect.minY, width: size, height: size)),
            (.gain, CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: band)),
        ]
    }

    /// What a press at `p` grabs: a corner first, then the gain band (each
    /// with 2 points of slack).
    public static func hit(_ p: CGPoint, row r: EventRow, in rect: CGRect) -> RegionHandle? {
        for (h, box) in rects(r, in: rect) where box.insetBy(dx: -2, dy: -2).contains(p) { return h }
        return nil
    }

    /// The fades after dragging `handle` by `dx` points, clamped so both
    /// fit the region.
    public static func fades(_ r: EventRow, _ handle: RegionHandle, dx: CGFloat, in rect: CGRect) -> (
        fadeIn: Int64, fadeOut: Int64
    ) {
        let len = Int64(clamping: r.frames)
        let delta = Int64((Double(dx) * framesPerPoint(r, rect)).rounded())
        let fi = max(r.fadeIn.length, 0)
        let fo = max(r.fadeOut.length, 0)
        switch handle {
        case .fadeIn: return (min(max(fi + delta, 0), max(len - fo, 0)), fo)
        case .fadeOut: return (fi, min(max(fo - delta, 0), max(len - fi, 0)))
        case .gain: return (fi, fo)
        }
    }

    /// A fade drag's full target (nil: no change).
    public static func fadeCommand(_ r: EventRow, _ handle: RegionHandle, dx: CGFloat, in rect: CGRect) -> EditCommand? {
        let (fi, fo) = fades(r, handle, dx: dx, in: rect)
        guard fi != r.fadeIn.length || fo != r.fadeOut.length else { return nil }
        return .audio(
            edit: .setFades(
                event: r.id, fadeIn: FadeRow(length: fi, shape: r.fadeIn.shape),
                fadeOut: FadeRow(length: fo, shape: r.fadeOut.shape)))
    }

    /// The gain after dragging the band `dy` points (up is louder), in
    /// tenths of a dB within −144…+24.
    public static func gain(_ r: EventRow, dy: CGFloat, fine: Bool) -> Float {
        let g = r.gainDb - Float(dy) * (fine ? fineDbPerPoint : dbPerPoint)
        return min(max((g * 10).rounded() / 10, -144), 24)
    }

    /// A gain drag's full target (nil: no change).
    public static func gainCommand(_ r: EventRow, dy: CGFloat, fine: Bool) -> EditCommand? {
        let g = gain(r, dy: dy, fine: fine)
        return g == r.gainDb ? nil : .audio(edit: .setGain(event: r.id, gainDb: g))
    }

    /// ⌥-click on the band: 0 dB (nil when it is already).
    public static func resetGain(_ r: EventRow) -> EditCommand? {
        r.gainDb == 0 ? nil : .audio(edit: .setGain(event: r.id, gainDb: 0))
    }

    /// A fade's new shape (nil: the same shape, or not a fade).
    public static func shapeCommand(_ r: EventRow, _ handle: RegionHandle, _ shape: FadeShapeRow) -> EditCommand? {
        switch handle {
        case .fadeIn where r.fadeIn.shape != shape:
            .audio(edit: .setFades(event: r.id, fadeIn: FadeRow(length: r.fadeIn.length, shape: shape), fadeOut: r.fadeOut))
        case .fadeOut where r.fadeOut.shape != shape:
            .audio(edit: .setFades(event: r.id, fadeIn: r.fadeIn, fadeOut: FadeRow(length: r.fadeOut.length, shape: shape)))
        default:
            nil
        }
    }

    /// Which fade's area `p` is in (between the region's edge and the
    /// fade's handle, for a fade with a length): the shape menu's target.
    public static func fadeArea(_ p: CGPoint, row r: EventRow, in rect: CGRect) -> RegionHandle? {
        guard rect.contains(p) else { return nil }
        if r.fadeIn.length > 0, p.x <= fadeInX(r, in: rect) { return .fadeIn }
        if r.fadeOut.length > 0, p.x >= fadeOutX(r, in: rect) { return .fadeOut }
        return nil
    }

    /// "+3.0 dB", "0.0 dB", "-6.5 dB" (Ruling 18).
    public static func gainLabel(_ db: Float) -> String {
        abs(db) < 0.05 ? "0.0 dB" : String(format: "%+.1f dB", db)
    }
}

/// Waveform bars (spec §12.3 "the overview drawing geometry").
public enum WaveformGeometry {
    /// One bar per column and channel in `rect`: channels stacked top to
    /// bottom, each bar from the column's max to its min around its
    /// channel's centre line, clipped to that channel's band; silence is a
    /// hairline; a non-finite value counts as silence. Rows whose arrays do
    /// not hold `channels × columns` values draw nothing.
    public static func bars(_ rows: OverviewRows, in rect: CGRect) -> [CGRect] {
        let n = Int(rows.columns)
        let ch = Int(rows.channels)
        guard n > 0, ch > 0, rows.min.count >= n * ch, rows.max.count >= n * ch, rect.width > 0, rect.height > 0
        else { return [] }
        let w = rect.width / CGFloat(n)
        let h = rect.height / CGFloat(ch)
        let clip = { (v: Float) -> CGFloat in v.isFinite ? CGFloat(Swift.max(-1, Swift.min(1, v))) : 0 }
        var out: [CGRect] = []
        out.reserveCapacity(n * ch)
        for k in 0..<ch {
            let mid = rect.minY + h * (CGFloat(k) + 0.5)
            for c in 0..<n {
                let lo = clip(rows.min[k * n + c])
                let hi = clip(rows.max[k * n + c])
                let top = mid - Swift.max(hi, lo) * h / 2
                let bottom = mid - Swift.min(hi, lo) * h / 2
                out.append(
                    CGRect(x: rect.minX + CGFloat(c) * w, y: top, width: Swift.max(w, 0.5), height: Swift.max(bottom - top, 0.5)))
            }
        }
        return out
    }
}

/// VoiceOver labels for regions (spec §3.5).
public enum RegionA11y {
    public static func label(_ r: EventRow, name: String, laneName: String, meter: [MeterEntryRow], offline: Bool) -> String {
        let end = r.tick > Int64.max - max(r.duration, 1) ? Int64.max : r.tick + max(r.duration, 1) - 1
        let first = MeterMath.bar(at: r.tick, meter).number
        let last = MeterMath.bar(at: max(end, r.tick), meter).number
        let bars = first == last ? "bar \(first)" : "bars \(first) to \(last)"
        var s = "Region \(name) on \(laneName), \(bars), gain \(RegionHandles.gainLabel(r.gainDb))"
        if r.fadeIn.length > 0 || r.fadeOut.length > 0 { s += ", fades" }
        if offline { s += ", offline" }
        if r.mute { s += ", muted" }
        return s
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAWEditors/RegionDrawing.swift <<'VIZDAW_EOF'
import AppKit
import VizDAWAppModel
import VizDAWCore

/// The colours audio regions are drawn in, by meaning (plan 5b Ruling 16).
/// Every one is a system colour; the SP7 light and dark themes replace
/// this one place.
@MainActor
public enum RegionPalette {
    public static var body: NSColor { .systemTeal }
    public static var outline: NSColor { .systemTeal }
    public static var waveform: NSColor { .labelColor.withAlphaComponent(0.65) }
    public static var fadeLine: NSColor { .secondaryLabelColor }
    public static var handle: NSColor { .controlAccentColor }
    public static var gainLine: NSColor { .systemOrange }
    public static var offline: NSColor { .systemRed }
    public static var readoutText: NSColor { .labelColor }
    public static var readoutBackground: NSColor { .textBackgroundColor }
}

/// Drawing an audio region (spec §10.1–10.2), shared by the arrangement
/// and the region strip.
@MainActor
public enum RegionDrawing {
    /// Draws `r` in `rect` into the current context: its body, its
    /// waveform `rows` in `waveRect` (ready or coarse only), its fade lines,
    /// a gain line when the gain is not 0 dB, its outline and, with
    /// `handles`, its corner handles. An offline region is hatched instead
    /// of showing a waveform.
    public static func draw(
        _ r: EventRow, in rect: CGRect, rows: OverviewRows?, waveRect: CGRect, offline: Bool, selected: Bool,
        handles: Bool
    ) {
        let body = NSBezierPath(roundedRect: rect.insetBy(dx: 0.5, dy: 0.5), xRadius: 2, yRadius: 2)
        RegionPalette.body.withAlphaComponent(r.mute ? 0.10 : 0.22).setFill()
        body.fill()
        NSGraphicsContext.saveGraphicsState()
        body.addClip()
        if offline {
            hatch(rect)
        } else if let rows, rows.state == .ready || rows.state == .coarse {
            RegionPalette.waveform.setFill()
            for b in WaveformGeometry.bars(rows, in: waveRect) { b.fill() }
        }
        RegionPalette.fadeLine.setStroke()
        if r.fadeIn.length > 0 {
            let p = NSBezierPath()
            p.move(to: CGPoint(x: rect.minX, y: rect.maxY))
            p.line(to: CGPoint(x: RegionHandles.fadeInX(r, in: rect), y: rect.minY))
            p.lineWidth = 1
            p.stroke()
        }
        if r.fadeOut.length > 0 {
            let p = NSBezierPath()
            p.move(to: CGPoint(x: RegionHandles.fadeOutX(r, in: rect), y: rect.minY))
            p.line(to: CGPoint(x: rect.maxX, y: rect.maxY))
            p.lineWidth = 1
            p.stroke()
        }
        if r.gainDb != 0 {
            RegionPalette.gainLine.setFill()
            CGRect(x: rect.minX, y: rect.minY + 1, width: rect.width, height: 1).fill()
        }
        NSGraphicsContext.restoreGraphicsState()
        (selected ? NSColor.controlAccentColor : RegionPalette.outline).setStroke()
        body.lineWidth = selected ? 2 : 1
        if r.mute { body.setLineDash([3, 2], count: 2, phase: 0) }
        body.stroke()
        if handles {
            RegionPalette.handle.setFill()
            for (h, box) in RegionHandles.rects(r, in: rect) where h != .gain { NSBezierPath(rect: box).fill() }
        }
        if offline, rect.width > 60 {
            NSString(string: "Offline").draw(
                at: CGPoint(x: rect.minX + 4, y: rect.maxY - 14),
                withAttributes: [
                    .font: NSFont.systemFont(ofSize: 10, weight: .semibold), .foregroundColor: RegionPalette.offline,
                ])
        }
    }

    /// Diagonal hatching for offline media (parent spec §10).
    static func hatch(_ rect: CGRect) {
        RegionPalette.offline.withAlphaComponent(0.55).setStroke()
        let p = NSBezierPath()
        var x = rect.minX - rect.height
        while x < rect.maxX {
            p.move(to: CGPoint(x: x, y: rect.maxY))
            p.line(to: CGPoint(x: x + rect.height, y: rect.minY))
            x += 6
        }
        p.lineWidth = 1
        p.stroke()
    }

    /// The ±dB readout while the gain band is dragged.
    public static func readout(_ text: String, at p: CGPoint) {
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 11, weight: .medium),
            .foregroundColor: RegionPalette.readoutText,
        ]
        let s = NSString(string: text)
        let size = s.size(withAttributes: attrs)
        let box = CGRect(x: p.x, y: p.y, width: size.width + 8, height: size.height + 4)
        RegionPalette.readoutBackground.setFill()
        NSBezierPath(roundedRect: box, xRadius: 3, yRadius: 3).fill()
        s.draw(at: CGPoint(x: box.minX + 4, y: box.minY + 2), withAttributes: attrs)
    }

    /// The context menu of a fade's area: its four shapes, the current one
    /// checked; `apply` sends the edit.
    public static func fadeShapeMenu(
        _ r: EventRow, _ h: RegionHandle, apply: @escaping @MainActor (EditCommand) -> Void
    ) -> NSMenu {
        let menu = NSMenu()
        let current = h == .fadeOut ? r.fadeOut.shape : r.fadeIn.shape
        let fade = RegionHandles.label(h)
        for (shape, title) in RegionHandles.shapes {
            menu.addItem(
                MenuAction(title: "\(fade): \(title)") {
                    if let c = RegionHandles.shapeCommand(r, h, shape) { apply(c) }
                }.item(checked: shape == current))
        }
        return menu
    }
}
VIZDAW_EOF
```

- [ ] **Step 4: Hits and geometry in the layouts**

In `app/Sources/VizDAWEditors/ArrangementLayout.swift`:

Find:
```swift
    /// The lanes plus one spare row (the "Add Lane" row of the headers).
    public func contentHeight(rows: Int) -> CGFloat { CGFloat(rows + 1) * laneHeight }
}
```
Replace with:
```swift
    /// The lanes plus one spare row (the "Add Lane" row of the headers).
    public func contentHeight(rows: Int) -> CGFloat { CGFloat(rows + 1) * laneHeight }

    /// An audio region's rectangle in row `row`: its own span, a point
    /// inside the block band.
    public func regionRect(_ r: EventRow, row: Int, axis: TimeAxis) -> CGRect {
        let len = max(r.duration, 0)
        let end = r.tick > Int64.max - len ? Int64.max : r.tick + len
        let x0 = axis.x(r.tick)
        let x1 = axis.x(end)
        let inset = Self.blockInset + 1
        return CGRect(
            x: x0, y: rowTop(row) + inset, width: max(x1 - x0, Self.minBlockWidth),
            height: max(laneHeight - 2 * inset, 1))
    }
}
```

Find:
```swift
public enum ArrangementHit: Equatable, Sendable {
    case block(BlockHit)
    case empty(lane: LaneKey?, tick: Int64)
}
```
Replace with:
```swift
public enum ArrangementHit: Equatable, Sendable {
    case block(BlockHit)
    case empty(lane: LaneKey?, tick: Int64)
    /// A region's fade or gain handle (plan 5b).
    case handle(RegionHandleHit)
}
```

In `app/Sources/VizDAWEditors/RegionStrip.swift`:

Find:
```swift
public enum RegionHit: Equatable, Sendable {
    case mute(UInt64)
    case solo(UInt64)
    case region(RegionItem)
    case none
}
```
Replace with:
```swift
public enum RegionHit: Equatable, Sendable {
    case mute(UInt64)
    case solo(UInt64)
    case region(RegionItem)
    /// A region's fade or gain handle, with the region's rectangle (plan 5b).
    case handle(RegionItem, RegionHandle, CGRect)
    case none
}
```

Find:
```swift
    /// A region drag's full target: its start snapped (nil: no change).
```
Replace with:
```swift
    /// A region handle under `p`, if any (checked before `hit`).
    public static func handleHit(_ p: CGPoint, lines: [RegionLine], time: TimeAxis, side: CGFloat) -> RegionHit? {
        guard p.x >= side, p.y.isFinite, p.y >= -2 else { return nil }
        let i = max(Int((p.y / rowHeight).rounded(.down)), 0)
        guard i < lines.count else { return nil }
        for item in lines[i].items.reversed() {
            let r = rect(item.row, line: i, time: time, side: side)
            if let h = RegionHandles.hit(p, row: item.row, in: r) { return .handle(item, h, r) }
        }
        return nil
    }

    /// A region drag's full target: its start snapped (nil: no change).
```

- [ ] **Step 5: The arrangement**

In `app/Sources/VizDAWEditors/ArrangementView.swift`:

Find:
```swift
    private var drag: Drag?
```
Replace with:
```swift
    private var drag: Drag?
    /// A fade or gain handle being dragged (plan 5b Ruling 7).
    private var handleDrag: (hit: RegionHandleHit, from: CGPoint)?
    /// The gain readout while the gain band is dragged.
    private var readout: (text: String, at: CGPoint)?
```

Find:
```swift
            _ = document.lanes
            _ = document.selectedSeq
```
Replace with:
```swift
            _ = document.lanes
            _ = document.selectedSeq
            // Waveforms arrive and media go offline or come back.
            _ = document.overviewVersion
            _ = document.mediaVersion
```

Find:
```swift
            for b in blocks { draw(b, row: i, key: key) }
```
Replace with:
```swift
            for b in blocks { draw(b, row: i, key: key, dirty: dirty) }
```

Find:
```swift
    private func draw(_ b: BlockRow, row: Int, key: LaneKey) {
```
Replace with:
```swift
    private func draw(_ b: BlockRow, row: Int, key: LaneKey, dirty: CGRect) {
```

Find:
```swift
        if b.muted { shape.setLineDash([3, 2], count: 2, phase: 0) }
        shape.stroke()
        guard !light, r.width > 24 else { return }
```
Replace with:
```swift
        if b.muted { shape.setLineDash([3, 2], count: 2, phase: 0) }
        shape.stroke()
        if !light, b.kind == .events { drawRegions(b, row: row, key: key, dirty: dirty) }
        guard !light, r.width > 24 else { return }
```

Find:
```swift
    // MARK: Gestures

    private func hit(_ p: CGPoint) -> ArrangementHit {
        let keys = lanes
        var map: [LaneKey: [BlockRow]] = [:]
        if let row = geometry.row(atY: p.y, count: keys.count) { map[keys[row]] = editor.blocks(keys[row]) }
        return ArrangementHitTest.hit(p, lanes: keys, blocks: map, geometry: geometry, axis: axis)
    }
```
Replace with:
```swift
    /// The audio regions of an events block: waveform, fades, gain and
    /// handles (spec §10.1–10.2).
    private func drawRegions(_ b: BlockRow, row: Int, key: LaneKey, dirty: CGRect) {
        let ids = Set(b.events)
        let media = document.media
        for r in regionRows(key) where ids.contains(r.id) {
            let rect = geometry.regionRect(r, row: row, axis: axis)
            let vis = rect.intersection(dirty).intersection(content.bounds)
            guard !vis.isNull, vis.width > 0 else { continue }
            let offline = media.offline(r.media) != nil
            var rows: OverviewRows?
            if !offline {
                let from = axis.tick(atX: vis.minX)
                rows = media.waveform(
                    event: r.id, fromTick: from, toTick: max(axis.tick(atX: vis.maxX), from + 1),
                    columns: Int(vis.width.rounded(.up)))
            }
            let wave = CGRect(
                x: vis.minX, y: rect.minY + RegionHandles.band, width: vis.width,
                height: max(rect.height - RegionHandles.band - 2, 1))
            RegionDrawing.draw(
                r, in: rect, rows: rows, waveRect: wave, offline: offline, selected: editor.isSelected(event: r.id),
                handles: editor.tool == .arrow)
        }
    }

    /// The audio regions of a lane's active take.
    private func regionRows(_ key: LaneKey) -> [EventRow] {
        editor.laneEvents(key)?.events.filter { $0.kind == .audioRegion } ?? []
    }

    // MARK: Gestures

    private func hit(_ p: CGPoint) -> ArrangementHit {
        if let h = handleHit(p) { return .handle(h) }
        return blockHit(p)
    }

    private func blockHit(_ p: CGPoint) -> ArrangementHit {
        let keys = lanes
        var map: [LaneKey: [BlockRow]] = [:]
        if let row = geometry.row(atY: p.y, count: keys.count) { map[keys[row]] = editor.blocks(keys[row]) }
        return ArrangementHitTest.hit(p, lanes: keys, blocks: map, geometry: geometry, axis: axis)
    }

    /// A region's fade or gain handle under `p` (arrow tool, normal
    /// drawing; plan 5b Ruling 7).
    private func handleHit(_ p: CGPoint) -> RegionHandleHit? {
        guard editor.tool == .arrow, !light else { return nil }
        let keys = lanes
        guard let row = geometry.row(atY: p.y, count: keys.count) else { return nil }
        for r in regionRows(keys[row]).reversed() {
            let rect = geometry.regionRect(r, row: row, axis: axis)
            if let h = RegionHandles.hit(p, row: r, in: rect) {
                return RegionHandleHit(lane: keys[row], row: r, handle: h, rect: rect)
            }
        }
        return nil
    }

    /// The fade whose shape a context menu at `p` sets.
    private func fadeArea(_ p: CGPoint) -> (row: EventRow, handle: RegionHandle)? {
        guard !light else { return nil }
        let keys = lanes
        guard let row = geometry.row(atY: p.y, count: keys.count) else { return nil }
        for r in regionRows(keys[row]).reversed() {
            if let h = RegionHandles.fadeArea(p, row: r, in: geometry.regionRect(r, row: row, axis: axis)) {
                return (r, h)
            }
        }
        return nil
    }

    private func handleTarget(_ d: (hit: RegionHandleHit, from: CGPoint), to: CGPoint, modifiers: Modifiers)
        -> EditCommand?
    {
        switch d.hit.handle {
        case .gain: RegionHandles.gainCommand(d.hit.row, dy: to.y - d.from.y, fine: modifiers.contains(.shift))
        case .fadeIn, .fadeOut: RegionHandles.fadeCommand(d.hit.row, d.hit.handle, dx: to.x - d.from.x, in: d.hit.rect)
        }
    }

    private func showReadout(_ d: (hit: RegionHandleHit, from: CGPoint), to: CGPoint, modifiers: Modifiers) {
        guard d.hit.handle == .gain else { return }
        let g = RegionHandles.gain(d.hit.row, dy: to.y - d.from.y, fine: modifiers.contains(.shift))
        readout = (RegionHandles.gainLabel(g), CGPoint(x: to.x + 12, y: to.y - 20))
        overlay.needsDisplay = true
    }

    private func clearReadout() {
        guard readout != nil else { return }
        readout = nil
        overlay.needsDisplay = true
    }

    public override func drawOverlay(in dirty: CGRect) {
        if let r = readout { RegionDrawing.readout(r.text, at: r.at) }
    }
```

Find:
```swift
    public override func contentCancel() {
        handle(machine.cancel())
        drag = nil
        if editor.gesture != nil { editor.cancelGesture() }
    }
```
Replace with:
```swift
    public override func contentCancel() {
        handle(machine.cancel())
        drag = nil
        handleDrag = nil
        clearReadout()
        if editor.gesture != nil { editor.cancelGesture() }
    }
```

Find:
```swift
            case let .begin(.block(b), from, modifiers):
                // The first move arrives with the begin: its row is the
                // target lane the gesture starts out touching.
                var to = from
                if i + 1 < effects.count, case let .move(_, _, p, _) = effects[i + 1] { to = p }
                beginDrag(b, from: from, to: to, copy: modifiers.contains(.option))
            case let .move(_, _, to, _):
                if let d = drag {
                    editor.setGestureLanes(touched(d, to: to))
                    editor.updateGesture(target(d, to: to))
                }
            case let .end(_, _, to, _):
                if let d = drag {
                    drag = nil
                    editor.setGestureLanes(touched(d, to: to))
                    editor.endGesture(target(d, to: to))
                }
            case .cancel:
                if drag != nil {
                    drag = nil
                    editor.cancelGesture()
                }
            case .begin(.empty, _, _):
                break
```
Replace with:
```swift
            case let .begin(.block(b), from, modifiers):
                // The first move arrives with the begin: its row is the
                // target lane the gesture starts out touching.
                var to = from
                if i + 1 < effects.count, case let .move(_, _, p, _) = effects[i + 1] { to = p }
                beginDrag(b, from: from, to: to, copy: modifiers.contains(.option))
            case let .begin(.handle(h), from, _):
                guard editor.beginGesture(RegionHandles.label(h.handle), lanes: [h.lane], origin: .other) else {
                    continue
                }
                handleDrag = (h, from)
            case let .move(_, _, to, modifiers):
                if let d = drag {
                    editor.setGestureLanes(touched(d, to: to))
                    editor.updateGesture(target(d, to: to))
                } else if let h = handleDrag {
                    showReadout(h, to: to, modifiers: modifiers)
                    editor.updateGesture(handleTarget(h, to: to, modifiers: modifiers))
                }
            case let .end(_, _, to, modifiers):
                if let d = drag {
                    drag = nil
                    editor.setGestureLanes(touched(d, to: to))
                    editor.endGesture(target(d, to: to))
                } else if let h = handleDrag {
                    handleDrag = nil
                    clearReadout()
                    editor.endGesture(handleTarget(h, to: to, modifiers: modifiers))
                }
            case .cancel:
                if drag != nil {
                    drag = nil
                    editor.cancelGesture()
                }
                if handleDrag != nil {
                    handleDrag = nil
                    clearReadout()
                    editor.cancelGesture()
                }
            case .begin(.empty, _, _):
                break
```

Find:
```swift
        case .block(let b):
            editor.selectBlock(b.block, in: b.lane, adding: modifiers.contains(.shift))
        case .empty:
            if !modifiers.contains(.shift) { editor.clearSelection() }
        }
```
Replace with:
```swift
        case .block(let b):
            editor.selectBlock(b.block, in: b.lane, adding: modifiers.contains(.shift))
        case .empty:
            if !modifiers.contains(.shift) { editor.clearSelection() }
        case .handle(let h) where h.handle == .gain && modifiers.contains(.option):
            if let c = RegionHandles.resetGain(h.row) { editor.edit(c) }
        case .handle(let h):
            editor.setSelection(SelectionRow(timeRange: nil, lanes: [h.lane.ref], events: [h.row.id]))
        }
```

Find:
```swift
        let p = content.convert(event.locationInWindow, from: nil)
        guard case .block(let b) = hit(p) else { return nil }
```
Replace with:
```swift
        let p = content.convert(event.locationInWindow, from: nil)
        if let area = fadeArea(p) {
            return RegionDrawing.fadeShapeMenu(area.row, area.handle) { [weak self] c in self?.editor.edit(c) }
        }
        guard case .block(let b) = blockHit(p) else { return nil }
```

Find:
```swift
                        press: { [weak self] in self?.editor.selectBlock(b, in: key, adding: false) }))
            }
        }
        return out
    }
}
```
Replace with:
```swift
                        press: { [weak self] in self?.editor.selectBlock(b, in: key, adding: false) }))
            }
            let media = document.media
            for r in regionRows(key) {
                let rect = geometry.regionRect(r, row: i, axis: axis)
                guard rect.intersects(content.bounds) else { continue }
                out.append(
                    CanvasAccessibilityItem(
                        label: RegionA11y.label(
                            r, name: media.row(r.media)?.name ?? "audio", laneName: rows[i].name, meter: meter,
                            offline: media.offline(r.media) != nil),
                        frame: rect, selected: editor.isSelected(event: r.id),
                        press: { [weak self] in
                            self?.editor.setSelection(SelectionRow(timeRange: nil, lanes: [key.ref], events: [r.id]))
                        }))
            }
        }
        return out
    }
}
```

- [ ] **Step 6: The region strip**

In `app/Sources/VizDAWEditors/RegionStripView.swift`:

Find:
```swift
    private var moving: (item: RegionItem, startTick: Int64)?
```
Replace with:
```swift
    private var moving: (item: RegionItem, startTick: Int64)?
    /// A fade or gain handle being dragged (plan 5b Ruling 7).
    private var handleDrag: (item: RegionItem, handle: RegionHandle, rect: CGRect, from: CGPoint)?
    /// The gain readout while the gain band is dragged.
    private var readout: (text: String, at: CGPoint)?
```

Find:
```swift
            _ = editor.drawVersion
            _ = editor.selection
            _ = editor.layout.pianoRoll
```
Replace with:
```swift
            _ = editor.drawVersion
            _ = editor.selection
            _ = editor.layout.pianoRoll
            _ = model.document?.overviewVersion
            _ = model.document?.mediaVersion
```

Find:
```swift
            for item in line.items {
                let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
                guard r.intersects(dirty) else { continue }
                let path = NSBezierPath(roundedRect: r, xRadius: 3, yRadius: 3)
                NSColor.systemTeal.withAlphaComponent(item.row.mute ? 0.15 : 0.35).setFill()
                path.fill()
                (editor.isSelected(event: item.row.id) ? NSColor.controlAccentColor : NSColor.systemTeal).setStroke()
                path.lineWidth = editor.isSelected(event: item.row.id) ? 2 : 1
                path.stroke()
                NSString(string: model.row(item.lane)?.name ?? "").draw(
```
Replace with:
```swift
            let media = model.document?.media
            let lineArea = NSRect(x: side, y: 0, width: max(bounds.width - side, 0), height: bounds.height)
            for item in line.items {
                let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
                guard r.intersects(dirty) else { continue }
                let vis = r.intersection(dirty).intersection(lineArea)
                let offline = media?.offline(item.row.media) != nil
                var rows: OverviewRows?
                if !offline, !vis.isNull, vis.width > 0 {
                    let from = timeAxis.tick(atX: vis.minX - side)
                    rows = media?.waveform(
                        event: item.row.id, fromTick: from, toTick: max(timeAxis.tick(atX: vis.maxX - side), from + 1),
                        columns: Int(vis.width.rounded(.up)))
                }
                let wave = vis.isNull
                    ? CGRect.zero
                    : CGRect(
                        x: vis.minX, y: r.minY + RegionHandles.band, width: vis.width,
                        height: max(r.height - RegionHandles.band - 1, 1))
                RegionDrawing.draw(
                    item.row, in: r, rows: rows, waveRect: wave, offline: offline,
                    selected: editor.isSelected(event: item.row.id), handles: true)
                NSString(string: model.row(item.lane)?.name ?? "").draw(
```

Find:
```swift
            NSString(string: line.title).draw(
                at: CGPoint(x: side + 4, y: RegionStripLayout.rowHeight * CGFloat(i) + 4),
                withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
            NSGraphicsContext.restoreGraphicsState()
        }
    }
```
Replace with:
```swift
            NSString(string: line.title).draw(
                at: CGPoint(x: side + 4, y: RegionStripLayout.rowHeight * CGFloat(i) + 4),
                withAttributes: [.font: small, .foregroundColor: NSColor.secondaryLabelColor])
            NSGraphicsContext.restoreGraphicsState()
        }
        if let r = readout { RegionDrawing.readout(r.text, at: r.at) }
    }
```

Find:
```swift
    func pressed(at p: CGPoint, clicks: Int, modifiers: Modifiers) {
        handle(
            machine.down(
                RegionStripLayout.hit(p, lines: lines(), time: timeAxis, side: side), at: p, clicks: clicks,
                modifiers: modifiers))
    }
```
Replace with:
```swift
    func pressed(at p: CGPoint, clicks: Int, modifiers: Modifiers) {
        let ls = lines()
        let hit =
            RegionStripLayout.handleHit(p, lines: ls, time: timeAxis, side: side)
            ?? RegionStripLayout.hit(p, lines: ls, time: timeAxis, side: side)
        handle(machine.down(hit, at: p, clicks: clicks, modifiers: modifiers))
    }
```

Find:
```swift
        if e.keyCode == 53 {  // Escape
            handle(machine.cancel())
            moving = nil
            if editor.gesture != nil { editor.cancelGesture() }
```
Replace with:
```swift
        if e.keyCode == 53 {  // Escape
            handle(machine.cancel())
            moving = nil
            handleDrag = nil
            readout = nil
            if editor.gesture != nil { editor.cancelGesture() }
```

Find:
```swift
        _ = machine.cancel()
        if moving != nil {
            moving = nil
            editor.cancelGesture()
        }
    }
```
Replace with:
```swift
        _ = machine.cancel()
        if moving != nil || handleDrag != nil {
            moving = nil
            handleDrag = nil
            readout = nil
            editor.cancelGesture()
        }
    }
```

Find:
```swift
    private func handle(_ effects: [ToolMachine<RegionHit>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(hit, _, _, modifiers):
                click(hit, adding: modifiers.contains(.shift))
            case let .begin(.region(item), from, _):
                guard editor.beginGesture("Move Region", lanes: [item.lane], origin: .regionStrip) else { continue }
                moving = (item, timeAxis.tick(atX: from.x - side))
            case .begin:
                break
            case let .move(_, _, to, _):
                if let m = moving { editor.updateGesture(target(m, to)) }
            case let .end(_, _, to, _):
                if let m = moving {
                    moving = nil
                    editor.endGesture(target(m, to))
                }
            case .cancel:
                if moving != nil {
                    moving = nil
                    editor.cancelGesture()
                }
            }
        }
    }
```
Replace with:
```swift
    private func handle(_ effects: [ToolMachine<RegionHit>.Effect]) {
        for fx in effects {
            switch fx {
            case let .click(hit, _, _, modifiers):
                click(hit, modifiers: modifiers)
            case let .begin(.region(item), from, _):
                guard editor.beginGesture("Move Region", lanes: [item.lane], origin: .regionStrip) else { continue }
                moving = (item, timeAxis.tick(atX: from.x - side))
            case let .begin(.handle(item, h, rect), from, _):
                guard editor.beginGesture(RegionHandles.label(h), lanes: [item.lane], origin: .other) else { continue }
                handleDrag = (item, h, rect, from)
            case .begin:
                break
            case let .move(_, _, to, modifiers):
                if let m = moving {
                    editor.updateGesture(target(m, to))
                } else if let d = handleDrag {
                    showReadout(d, to: to, modifiers: modifiers)
                    editor.updateGesture(handleTarget(d, to: to, modifiers: modifiers))
                }
            case let .end(_, _, to, modifiers):
                if let m = moving {
                    moving = nil
                    editor.endGesture(target(m, to))
                } else if let d = handleDrag {
                    handleDrag = nil
                    readout = nil
                    needsDisplay = true
                    editor.endGesture(handleTarget(d, to: to, modifiers: modifiers))
                }
            case .cancel:
                if moving != nil || handleDrag != nil {
                    moving = nil
                    handleDrag = nil
                    readout = nil
                    editor.cancelGesture()
                }
            }
        }
    }

    private func handleTarget(
        _ d: (item: RegionItem, handle: RegionHandle, rect: CGRect, from: CGPoint), to: CGPoint,
        modifiers: Modifiers
    ) -> EditCommand? {
        switch d.handle {
        case .gain: RegionHandles.gainCommand(d.item.row, dy: to.y - d.from.y, fine: modifiers.contains(.shift))
        case .fadeIn, .fadeOut: RegionHandles.fadeCommand(d.item.row, d.handle, dx: to.x - d.from.x, in: d.rect)
        }
    }

    private func showReadout(
        _ d: (item: RegionItem, handle: RegionHandle, rect: CGRect, from: CGPoint), to: CGPoint,
        modifiers: Modifiers
    ) {
        guard d.handle == .gain else { return }
        let g = RegionHandles.gain(d.item.row, dy: to.y - d.from.y, fine: modifiers.contains(.shift))
        readout = (RegionHandles.gainLabel(g), CGPoint(x: to.x + 12, y: max(to.y - 18, 0)))
        needsDisplay = true
    }

    /// The fade-shape menu of a region's fade area.
    public override func menu(for event: NSEvent) -> NSMenu? {
        let p = point(event)
        guard p.x >= side, p.y >= 0 else { return nil }
        let ls = lines()
        let i = Int((p.y / RegionStripLayout.rowHeight).rounded(.down))
        guard i < ls.count else { return nil }
        for item in ls[i].items.reversed() {
            let r = RegionStripLayout.rect(item.row, line: i, time: timeAxis, side: side)
            if let h = RegionHandles.fadeArea(p, row: item.row, in: r) {
                return RegionDrawing.fadeShapeMenu(item.row, h) { [weak self] c in self?.editor.edit(c) }
            }
        }
        return nil
    }
```

Find:
```swift
    private func click(_ hit: RegionHit, adding: Bool) {
        let strips = model.outputStrips()
        switch hit {
        case .mute(let o): model.setOutputMute(o, !(strips[o]?.mute ?? false))
        case .solo(let o): model.setOutputSolo(o, !(strips[o]?.solo ?? false))
        case .region(let item): model.select([(item.lane, item.row.id)], adding: adding)
        case .none: break
        }
    }
```
Replace with:
```swift
    private func click(_ hit: RegionHit, modifiers: Modifiers) {
        let strips = model.outputStrips()
        switch hit {
        case .mute(let o): model.setOutputMute(o, !(strips[o]?.mute ?? false))
        case .solo(let o): model.setOutputSolo(o, !(strips[o]?.solo ?? false))
        case .region(let item): model.select([(item.lane, item.row.id)], adding: modifiers.contains(.shift))
        case .handle(let item, .gain, _) where modifiers.contains(.option):
            if let c = RegionHandles.resetGain(item.row) { editor.edit(c) }
        case .handle(let item, _, _): model.select([(item.lane, item.row.id)], adding: modifiers.contains(.shift))
        case .none: break
        }
    }
```

- [ ] **Step 7: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: every test passes, including the 4a/4b arrangement and region-strip tests (a press in a region's body, below its top 8 points and away from its corners, still moves it).

- [ ] **Step 8: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```

- [ ] **Step 9: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWEditors/RegionEditing.swift app/Sources/VizDAWEditors/RegionDrawing.swift app/Sources/VizDAWEditors/ArrangementLayout.swift app/Sources/VizDAWEditors/ArrangementView.swift app/Sources/VizDAWEditors/RegionStrip.swift app/Sources/VizDAWEditors/RegionStripView.swift app/Tests/VizDAWEditorsTests/RegionEditingTests.swift app/Tests/VizDAWAppModelTests/RegionHandleViewTests.swift
git commit -m "editors: waveforms, fade and gain handles, fade shapes, offline hatching

The arrangement and the region strip draw each audio region's waveform
(gain and fades applied in Rust), fade lines, gain line and handles in
semantic colours named once in RegionPalette; offline media is hatched.
Corner handles drag the fades, the top edge drags the gain with a dB
readout (option-click resets it), a fade's area offers its shapes; each
drag is one gesture. Regions are accessibility elements.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 11: Editors — dropping audio files on the arrangement and the region strip

**Files:**
- Create: `app/Sources/VizDAWEditors/AudioDrop.swift`, `app/Tests/VizDAWEditorsTests/AudioDropTests.swift`, `app/Tests/VizDAWAppModelTests/AudioDropViewTests.swift`
- Modify: `app/Sources/VizDAWEditors/ArrangementView.swift`, `app/Sources/VizDAWEditors/RegionStripView.swift`, `app/Sources/VizDAWEditors/RegionStrip.swift`
- Test: `app/Tests/VizDAWEditorsTests/AudioDropTests.swift`, `app/Tests/VizDAWAppModelTests/AudioDropViewTests.swift`

**Interfaces:**
- Consumes: Task 8's `DocumentModel.importAudio(paths:target:)`; `Snap.apply(_:meter:)`; `ImportTargetRow`; 5a's import job (Ruling 19 in the core still decides new lanes for several or split files).
- Produces:
  - `AudioDrop.tick(_:snap:meter:) -> Int64`, `AudioDrop.target(seq:lanes:row:tick:snap:meter:) -> ImportTargetRow?`, `AudioDrop.paths(_ pb: NSPasteboard) -> [String]`;
  - `RegionStripLayout.dropLane(_:lines:time:side:) -> LaneKey?`;
  - `ArrangementView.drop(_:at:) -> Bool` and `RegionStripView.drop(_:at:) -> Bool` (internal; the dragging methods call them, tests call them directly); both views register for file URLs.

Ruling 8; spec §10.2 "Dropping audio files onto the arrangement or region strip imports them (§5.1) at the drop tick. Dropping onto empty space below the lanes creates new lanes." A drop is any file URL (not folders); the importer reports files it cannot read, per file, through the job's problems (a Problems banner).

- [ ] **Step 1: Write the failing tests**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWEditorsTests/AudioDropTests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
import VizDAWAppModel
import VizDAWCore
@testable import VizDAWEditors

private let meter = [MeterEntryRow(tick: 0, num: 4, den: 4, sharps: 0, minor: false)]

@MainActor
@Suite struct AudioDropTests {
    @Test func aDropLandsOnTheLaneUnderItAtTheSnappedTick() {
        let lanes = [LaneKey(3, 30), LaneKey(3, 31)]
        var snap = Snap()
        #expect(
            AudioDrop.target(seq: 3, lanes: lanes, row: 1, tick: 31_104, snap: snap, meter: meter)
                == ImportTargetRow(seq: 3, lane: 31, tick: 30_720))
        // Below the lanes: new lanes.
        #expect(
            AudioDrop.target(seq: 3, lanes: lanes, row: nil, tick: 31_104, snap: snap, meter: meter)
                == ImportTargetRow(seq: 3, lane: nil, tick: 30_720))
        #expect(AudioDrop.target(seq: 3, lanes: lanes, row: 9, tick: 0, snap: snap, meter: meter)?.lane == nil)
        // Never before 0; no sequence, no target.
        #expect(AudioDrop.target(seq: 3, lanes: lanes, row: 0, tick: -900, snap: snap, meter: meter)?.tick == 0)
        #expect(AudioDrop.target(seq: nil, lanes: lanes, row: 0, tick: 0, snap: snap, meter: meter) == nil)
        snap.enabled = false
        #expect(AudioDrop.tick(31_104, snap: snap, meter: meter) == 31_104)
    }

    @Test func onlyFilesAreTaken() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("vizdaw-drop-\(UUID())")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("a.wav")
        try Data("x".utf8).write(to: file)
        let pb = NSPasteboard(name: NSPasteboard.Name("vizdaw-test-\(UUID())"))
        defer { pb.releaseGlobally() }
        pb.clearContents()
        pb.writeObjects([file as NSURL, dir as NSURL, URL(string: "https://example.com/b.wav")! as NSURL])
        #expect(AudioDrop.paths(pb) == [file.path])
    }
}
VIZDAW_EOF
cat > app/Tests/VizDAWAppModelTests/AudioDropViewTests.swift <<'VIZDAW_EOF'
import AppKit
import Testing
import VizDAWCore
@testable import VizDAWAppModel
@testable import VizDAWEditors

/// Dropping files on the editors (plan 5b Task 11, Ruling 8). Song's
/// rows: Groove (30) at y 0–48, Chords (31) at 48–96; 384 ticks a point.
@MainActor
@Suite struct AudioDropViewTests {
    @Test func theArrangementImportsIntoTheLaneUnderTheDrop() throws {
        let f = editorFake()
        let m = try DocumentModel(session: f)
        let v = ArrangementView(document: m)
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 300)
        v.layout()
        #expect(v.drop(["/a.wav"], at: CGPoint(x: 81, y: 70)))
        #expect(f.imports.last?.0 == ["/a.wav"])
        #expect(f.imports.last?.1 == ImportTargetRow(seq: 3, lane: 31, tick: 30_720))
        // Below the last lane: new lanes.
        #expect(v.drop(["/a.wav", "/b.wav"], at: CGPoint(x: 81, y: 200)))
        #expect(f.imports.last?.1 == ImportTargetRow(seq: 3, lane: nil, tick: 30_720))
        #expect(!v.drop([], at: CGPoint(x: 0, y: 0)))
        #expect(f.imports.count == 2)
    }

    @Test func theRegionStripImportsIntoTheRegionsLaneOrTheEditLane() throws {
        let f = audioFake()
        let m = try DocumentModel(session: f)
        m.editor.openDetail(.sequence(3), kind: .pianoRoll)
        let v = RegionStripView(model: m.pianoRoll(.pane))
        v.frame = NSRect(x: 0, y: 0, width: 800, height: 60)
        v.timeAxis = TimeAxis(ticksPerPoint: 96)
        let side = PianoRollContainer.sideWidth
        // On the Loop region (lane 33): into its lane.
        #expect(v.drop(["/a.wav"], at: CGPoint(x: side + 160, y: 11)))
        #expect(f.imports.last?.1 == ImportTargetRow(seq: 3, lane: 33, tick: 15_360))
        // Past the region: the piano roll's edit lane.
        #expect(v.drop(["/a.wav"], at: CGPoint(x: side + 700, y: 11)))
        let edit = try #require(m.pianoRoll(.pane).editLane)
        #expect(f.imports.last?.1?.lane == edit.lane)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `scripts/build-ffi.sh && scripts/swift.sh test 2>&1 | tail -5`
Expected: compile errors (`AudioDrop`, `drop(_:at:)` not found).

- [ ] **Step 3: Implement**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAWEditors/AudioDrop.swift <<'VIZDAW_EOF'
import AppKit
import VizDAWAppModel
import VizDAWCore

/// Dropping audio files on the editors (spec §10.2; plan 5b Ruling 8).
public enum AudioDrop {
    /// The drop tick: snapped when snap is on, never before 0.
    public static func tick(_ tick: Int64, snap: Snap, meter: [MeterEntryRow]) -> Int64 {
        max(0, snap.apply(tick, meter: meter))
    }

    /// Where files dropped on row `row` (nil: below the lanes) at `tick`
    /// go: that row's lane, else new lanes; nil without a sequence.
    public static func target(
        seq: UInt64?, lanes: [LaneKey], row: Int?, tick: Int64, snap: Snap, meter: [MeterEntryRow]
    ) -> ImportTargetRow? {
        guard let seq else { return nil }
        let lane = row.flatMap { lanes.indices.contains($0) ? lanes[$0].lane : nil }
        return ImportTargetRow(seq: seq, lane: lane, tick: Self.tick(tick, snap: snap, meter: meter))
    }

    /// The files (not folders, not web URLs) on a dragging pasteboard.
    @MainActor
    public static func paths(_ pb: NSPasteboard) -> [String] {
        let urls = pb.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        return urls.filter { u in
            var dir: ObjCBool = false
            return FileManager.default.fileExists(atPath: u.path, isDirectory: &dir) && !dir.boolValue
        }.map(\.path)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAWEditors/ArrangementView.swift`:

Find:
```swift
    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        axis = TimeAxis(ticksPerPoint: document.editor.layout.ticksPerPoint)
```
Replace with:
```swift
    public init(document: DocumentModel) {
        self.document = document
        super.init(frame: .zero)
        registerForDraggedTypes([.fileURL])
        axis = TimeAxis(ticksPerPoint: document.editor.layout.ticksPerPoint)
```

Find:
```swift
    // MARK: Context menus (spec §4a subsequence workflow)
```
Replace with:
```swift
    // MARK: Dropping audio files (spec §10.2; plan 5b Ruling 8)

    public override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        seq != nil && !AudioDrop.paths(sender.draggingPasteboard).isEmpty ? .copy : []
    }

    public override func draggingUpdated(_ sender: any NSDraggingInfo) -> NSDragOperation {
        draggingEntered(sender)
    }

    public override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        drop(AudioDrop.paths(sender.draggingPasteboard), at: content.convert(sender.draggingLocation, from: nil))
    }

    /// Imports `paths` dropped at `p` (content coordinates): into the lane
    /// under it, or new lanes below the last one.
    @discardableResult
    func drop(_ paths: [String], at p: CGPoint) -> Bool {
        let keys = lanes
        guard !paths.isEmpty,
            let target = AudioDrop.target(
                seq: seq, lanes: keys, row: geometry.row(atY: p.y, count: keys.count), tick: axis.tick(atX: p.x),
                snap: snap, meter: meter)
        else { return false }
        document.importAudio(paths: paths, target: target)
        return true
    }

    // MARK: Context menus (spec §4a subsequence workflow)
```

In `app/Sources/VizDAWEditors/RegionStrip.swift`:

Find:
```swift
    /// A region handle under `p`, if any (checked before `hit`).
```
Replace with:
```swift
    /// The lane of the region under `p`: where a drop there goes.
    public static func dropLane(_ p: CGPoint, lines: [RegionLine], time: TimeAxis, side: CGFloat) -> LaneKey? {
        if case .region(let item) = hit(p, lines: lines, time: time, side: side) { return item.lane }
        return nil
    }

    /// A region handle under `p`, if any (checked before `hit`).
```

In `app/Sources/VizDAWEditors/RegionStripView.swift`:

Find:
```swift
    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
        observe()
    }
```
Replace with:
```swift
    public init(model: PianoRollModel) {
        self.model = model
        super.init(frame: .zero)
        wantsLayer = true
        registerForDraggedTypes([.fileURL])
        observe()
    }

    // MARK: Dropping audio files (spec §10.2; plan 5b Ruling 8)

    public override func draggingEntered(_ sender: any NSDraggingInfo) -> NSDragOperation {
        model.seq != nil && !AudioDrop.paths(sender.draggingPasteboard).isEmpty ? .copy : []
    }

    public override func draggingUpdated(_ sender: any NSDraggingInfo) -> NSDragOperation {
        draggingEntered(sender)
    }

    public override func performDragOperation(_ sender: any NSDraggingInfo) -> Bool {
        drop(AudioDrop.paths(sender.draggingPasteboard), at: convert(sender.draggingLocation, from: nil))
    }

    /// Imports `paths` dropped at `p`: into the lane of the region under
    /// it, else the piano roll's edit lane (new lanes without one).
    @discardableResult
    func drop(_ paths: [String], at p: CGPoint) -> Bool {
        guard !paths.isEmpty, let seq = model.seq else { return false }
        let lane = RegionStripLayout.dropLane(p, lines: lines(), time: timeAxis, side: side) ?? model.editLane
        let tick = AudioDrop.tick(timeAxis.tick(atX: p.x - side), snap: editor.layout.snap, meter: meter)
        model.document?.importAudio(
            paths: paths, target: ImportTargetRow(seq: seq, lane: lane?.seq == seq ? lane?.lane : nil, tick: tick))
        return true
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```
Expected: all pass.

- [ ] **Step 5: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWEditors/AudioDrop.swift app/Sources/VizDAWEditors/ArrangementView.swift app/Sources/VizDAWEditors/RegionStripView.swift app/Sources/VizDAWEditors/RegionStrip.swift app/Tests/VizDAWEditorsTests/AudioDropTests.swift app/Tests/VizDAWAppModelTests/AudioDropViewTests.swift
git commit -m "editors: drop audio files on the arrangement and the region strip

Files dropped on a lane import into it at the snapped drop tick; below
the last lane they make new lanes; on the region strip they go to the
region's lane or the piano roll's edit lane. Drops use the existing
import job, so problems and failures arrive as for Import Audio.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 12: The app — Media Pool window, Relink sheet, Region menu

**Files:**
- Create: `app/Sources/VizDAW/MediaPoolWindow.swift`, `app/Sources/VizDAW/RelinkSheet.swift`, `app/Tests/VizDAWAppModelTests/MediaPoolUITests.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift`, `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/DocumentView.swift`, `app/Sources/VizDAW/ProjectBanners.swift`, `app/Sources/VizDAW/ProjectPanels.swift`, `app/Sources/VizDAWAppModel/MediaModel.swift` (`showsRelinkSheet`)
- Test: `app/Tests/VizDAWAppModelTests/MediaPoolUITests.swift`

**Interfaces:**
- Consumes: Task 8's `MediaModel`, `RegionCommands`, `ProjectBanner.{jobText, failureTitle}`; Task 9's `WindowKindRow.mediaPool`, `AppControlHost.showMediaPool` (window id `"media-pool"`); Task 10's `RegionPalette`.
- Produces:
  - `MediaPoolWindow`, `MediaPoolView` (a `Table` of `MediaPoolRow`s with the pool's actions), tagged `.mediaPool`;
  - `RelinkSheet` on the document window, shown by `MediaModel.showsRelinkSheet(channelOn:)` (never under the control channel);
  - `TextPanel.run(title:message:initial:ok:label:) -> String?`, `ConfirmPanel.run(title:message:ok:) -> Bool`, `Panels.{chooseRelink(name:), chooseFolder()}`;
  - menus: File › Relink Media…, Window › Media Pool, Region › Normalize… / Fade In to Cursor / Fade Out from Cursor / Remove Fades;
  - `MediaModel.showsRelinkSheet(channelOn: Bool) -> Bool`.

Rulings 13, 14 and 18; spec §10.3. The pool's rows and actions are `MediaModel`'s (tested in Task 8); this task is the AppKit/SwiftUI surface. Every control has a text label or an accessibility label; decorative images are hidden from VoiceOver; colours are system styles or `RegionPalette`. The rename, delete and normalize questions are modal `NSAlert`s (no SwiftUI state), opened only by the user's own menu or button presses.

- [ ] **Step 1: Write the failing test**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Tests/VizDAWAppModelTests/MediaPoolUITests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// What the app's media pool surface decides (plan 5b Task 12).
@MainActor
@Suite struct MediaPoolUITests {
    @Test func theRelinkSheetNeverOpensUnderTheControlChannel() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", offline: .missing)]
        let m = try DocumentModel(session: f)
        #expect(m.media.showsRelinkSheet(channelOn: false))
        #expect(!m.media.showsRelinkSheet(channelOn: true), "programmatic sessions never see dialogs")
        m.media.leaveOffline()
        #expect(!m.media.showsRelinkSheet(channelOn: false))
    }

    @Test func theSheetClosesItselfOnceNothingIsLeftToRelink() throws {
        let f = demoFake()
        f.mediaRows = [mediaRow(1, "a", offline: .missing)]
        let m = try DocumentModel(session: f)
        f.mediaRows = [mediaRow(1, "a")]
        m.refreshAfterJob()
        #expect(!m.media.showsRelinkSheet(channelOn: false))
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `scripts/build-ffi.sh && scripts/swift.sh test 2>&1 | tail -5`
Expected: compile error (`showsRelinkSheet` not found).

- [ ] **Step 3: The model's switch**

In `app/Sources/VizDAWAppModel/MediaModel.swift`:

Find:
```swift
    /// "Leave Offline".
    public func leaveOffline() { relinkShown = false }
```
Replace with:
```swift
    /// "Leave Offline".
    public func leaveOffline() { relinkShown = false }

    /// Whether the document window shows the relink sheet: asked for,
    /// something left to relink, and never while the control channel is
    /// on (Ruling 14).
    public func showsRelinkSheet(channelOn: Bool) -> Bool {
        relinkShown && !channelOn && !relinkable.isEmpty
    }
```

- [ ] **Step 4: Panels**

In `app/Sources/VizDAW/ProjectPanels.swift`:

Find:
```swift
extension Panels {
    /// Audio files to import (any the system reads).
```
Replace with:
```swift
/// A modal question with one text field (Rename Media, Normalize).
@MainActor
enum TextPanel {
    /// The text entered; nil when cancelled.
    static func run(title: String, message: String, initial: String, ok: String, label: String) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: "Cancel")
        let field = NSTextField(string: initial)
        field.frame = NSRect(x: 0, y: 0, width: 280, height: 24)
        field.setAccessibilityLabel(label)
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        return field.stringValue
    }
}

/// A modal yes/no question (deleting media in use).
@MainActor
enum ConfirmPanel {
    static func run(title: String, message: String, ok: String) -> Bool {
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = title
        alert.informativeText = message
        alert.addButton(withTitle: ok)
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }
}

extension Panels {
    /// The file to relink a media item to (Locate…).
    static func chooseRelink(name: String) -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.audio]
        panel.prompt = "Relink"
        panel.message = "Choose the audio file for “\(name)”."
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// A folder to search for missing media (Search Folder…).
    static func chooseFolder() -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Search"
        panel.message = "Choose a folder to search for the missing audio files."
        return panel.runModal() == .OK ? panel.url?.path : nil
    }

    /// Audio files to import (any the system reads).
```

The existing `extension Panels { static func chooseAudio() … }` keeps its body; the Find above only inserts before it.

- [ ] **Step 5: The Media Pool window and the Relink sheet**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cat > app/Sources/VizDAW/MediaPoolWindow.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The media pool window of document `id` (spec §10.3).
struct MediaPoolWindow: View {
    @Binding var id: UUID?
    let store: DocumentStore

    /// Spelled out: a private stored property makes the memberwise
    /// initializer private under Xcode's compiler.
    init(id: Binding<UUID?>, store: DocumentStore) {
        _id = id
        self.store = store
    }

    var body: some View {
        content.background(WindowTag(kind: .mediaPool, doc: id))
    }

    @ViewBuilder private var content: some View {
        if let id, let doc = store.model(id) {
            MediaPoolView(media: doc.media)
                .focusedSceneValue(\.documentModel, doc)
                .focusedSceneValue(\.documentID, id)
                .navigationTitle("Media Pool — \(doc.snapshot.title)")
        } else {
            ContentUnavailableView(
                "No Document", systemImage: "waveform",
                description: Text("The document of this media pool is closed."))
        }
    }
}

/// The pool's table and actions (rows and actions are `MediaModel`'s).
struct MediaPoolView: View {
    @Bindable var media: MediaModel

    private var selected: UInt64? { media.selection.count == 1 ? media.selection.first : nil }

    var body: some View {
        VStack(spacing: 0) {
            Table(media.poolRows, selection: $media.selection) {
                TableColumn("Name", value: \.name)
                TableColumn("Duration", value: \.duration).width(min: 60, ideal: 72)
                TableColumn("Channels", value: \.channels).width(min: 56, ideal: 64)
                TableColumn("Origin", value: \.origin).width(min: 64, ideal: 88)
                TableColumn("Uses") { r in Text("\(r.uses)").monospacedDigit() }.width(min: 36, ideal: 44)
                TableColumn("Used", value: \.used).width(min: 44, ideal: 52)
                TableColumn("Offline") { r in
                    if r.offline.isEmpty {
                        Text("")
                    } else {
                        Label(r.offline, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(Color(nsColor: RegionPalette.offline))
                            .accessibilityLabel("Offline: \(r.offline)")
                    }
                }
            }
            .contextMenu(forSelectionType: UInt64.self) { ids in
                if let id = ids.first, ids.count == 1 { rowMenu(id) }
            } primaryAction: { ids in
                if let id = ids.first { media.selectUsers(id) }
            }
            .accessibilityLabel("Media items")
            Divider()
            HStack(spacing: 8) {
                Button("Select Regions") { if let id = selected { media.selectUsers(id) } }
                    .disabled(selected == nil)
                Button("Select Unused") { media.selectUnused() }
                Button("Compact") { media.compact() }
                    .help("Remove every item no region uses (one undo step). Their files go to the Trash on save.")
                Spacer()
                Button("Rename…") { if let id = selected { rename(id) } }.disabled(selected == nil)
                Button("Relink…") { if let id = selected { relink(id) } }.disabled(selected == nil)
                Button("Reveal in Finder") { if let id = selected { reveal(id) } }.disabled(selected == nil)
                Button("Import…") { media.importIntoPool(Panels.chooseAudio()) }
                Button("Delete") { if let id = selected { delete(id) } }.disabled(selected == nil)
            }
            .padding(8)
        }
        .frame(minWidth: 620, minHeight: 280)
    }

    @ViewBuilder private func rowMenu(_ id: UInt64) -> some View {
        Button("Select Regions") { media.selectUsers(id) }
        Button("Rename…") { rename(id) }
        Button("Relink…") { relink(id) }
        Button("Reveal in Finder") { reveal(id) }
        Divider()
        Button("Delete") { delete(id) }
    }

    private func rename(_ id: UInt64) {
        guard let r = media.row(id),
            let name = TextPanel.run(
                title: "Rename Media", message: "A new name for “\(r.name)”:", initial: r.name, ok: "Rename",
                label: "Media name")
        else { return }
        media.rename(id, to: name)
    }

    private func relink(_ id: UInt64) {
        guard let r = media.row(id), let path = Panels.chooseRelink(name: r.name) else { return }
        media.relink(id, path: path)
    }

    private func reveal(_ id: UInt64) {
        guard let f = media.file(id) else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: f)])
    }

    private func delete(_ id: UInt64) {
        if let q = media.deleteQuestion(id) {
            guard ConfirmPanel.run(title: "Delete Media", message: q, ok: "Delete") else { return }
        }
        media.remove(id)
    }
}
VIZDAW_EOF
cat > app/Sources/VizDAW/RelinkSheet.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore
import VizDAWEditors

/// The offline/relink dialog (spec §10.3; plan 5b Ruling 14): the items
/// that are missing or changed, each with Locate…, plus Search Folder…
/// and Leave Offline. Relinks run as jobs; the window's banners show them.
struct RelinkSheet: View {
    @Bindable var media: MediaModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Some audio files of this project are missing or changed")
                .font(.headline)
            Text("Their regions stay silent until each item is relinked to its file. Each relink is one undo step.")
                .font(.callout)
                .foregroundStyle(.secondary)
            List(media.relinkable, id: \.id) { r in
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(Color(nsColor: RegionPalette.offline))
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(r.name)
                        Text("\(r.offline.map(MediaModel.reasonText) ?? "") · \(r.path)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Locate…") {
                        if let p = Panels.chooseRelink(name: r.name) { media.relink(r.id, path: p) }
                    }
                    .accessibilityLabel("Locate \(r.name)")
                }
                .accessibilityElement(children: .contain)
            }
            .frame(minHeight: 160)
            HStack {
                Button("Search Folder…") {
                    if let d = Panels.chooseFolder() { media.searchFolder(d) }
                }
                Spacer()
                Button("Leave Offline") { media.leaveOffline() }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .frame(width: 540)
    }
}
VIZDAW_EOF
```

In `app/Sources/VizDAW/VizDAWApp.swift`:

Find:
```swift
        WindowGroup("Mixer", id: "mixer", for: UUID.self) { $id in
            MixerWindow(id: $id, store: appStore)
        }
        .defaultSize(width: 760, height: 560)
```
Replace with:
```swift
        WindowGroup("Mixer", id: "mixer", for: UUID.self) { $id in
            MixerWindow(id: $id, store: appStore)
        }
        .defaultSize(width: 760, height: 560)
        WindowGroup("Media Pool", id: "media-pool", for: UUID.self) { $id in
            MediaPoolWindow(id: $id, store: appStore)
        }
        .defaultSize(width: 760, height: 420)
```

In `app/Sources/VizDAW/DocumentView.swift`:

Find:
```swift
        .sheet(
            item: Binding(
                get: { model.editor.referenceDraft }, set: { model.editor.referenceDraft = $0 })
        ) { _ in
            ReferenceSheet(editor: model.editor)
        }
```
Replace with:
```swift
        .sheet(
            item: Binding(
                get: { model.editor.referenceDraft }, set: { model.editor.referenceDraft = $0 })
        ) { _ in
            ReferenceSheet(editor: model.editor)
        }
        .sheet(
            isPresented: Binding(
                get: { model.media.showsRelinkSheet(channelOn: ControlChannel.active) },
                set: { if !$0 { model.media.leaveOffline() } })
        ) {
            RelinkSheet(media: model.media)
        }
```

In `app/Sources/VizDAW/ProjectBanners.swift`:

Find:
```swift
            case .job(let id, let kind, let progress):
                ProgressView(value: Double(progress)).frame(width: 120)
                Text(kind == .import ? "Importing audio…" : "Converting media…")
```
Replace with:
```swift
            case .job(let id, let kind, let progress):
                ProgressView(value: Double(progress)).frame(width: 120)
                    .accessibilityLabel(ProjectBanner.jobText(kind))
                Text(ProjectBanner.jobText(kind))
```

- [ ] **Step 6: The menus**

In `app/Sources/VizDAW/AppCommands.swift`:

Find:
```swift
            Divider()
            Button("Project Settings…") { projectSettings() }
                .disabled(doc == nil)
        }
```
Replace with:
```swift
            Divider()
            Button("Project Settings…") { projectSettings() }
                .disabled(doc == nil)
            Button("Relink Media…") { doc?.media.relinkShown = true }
                .keyboardShortcut(KeyMenu.shortcut("media.relink"))
                .disabled(doc?.media.relinkable.isEmpty ?? true)
        }
```

Find:
```swift
            Button("Plugin Manager") { openWindow(id: "plugins") }
                .keyboardShortcut(KeyMenu.shortcut("window.plugin_manager"))
            Divider()
        }
```
Replace with:
```swift
            Button("Plugin Manager") { openWindow(id: "plugins") }
                .keyboardShortcut(KeyMenu.shortcut("window.plugin_manager"))
            Button("Media Pool") { if let docID { openWindow(id: "media-pool", value: docID) } }
                .keyboardShortcut(KeyMenu.shortcut("window.media_pool"))
                .disabled(docID == nil)
            Divider()
        }
```

Find:
```swift
        CommandMenu("Drum") {
```
Replace with:
```swift
        CommandMenu("Region") {
            Button("Normalize…") { if let doc { normalize(doc) } }
                .keyboardShortcut(KeyMenu.shortcut(RegionCommands.normalize))
                .disabled(doc.map { !RegionCommands.enabled(RegionCommands.normalize, $0) } ?? true)
            commandItem(RegionCommands.fadeIn, RegionCommands.self)
            commandItem(RegionCommands.fadeOut, RegionCommands.self)
            commandItem(RegionCommands.removeFades, RegionCommands.self)
        }
        CommandMenu("Drum") {
```

Find:
```swift
    private func projectSettings() {
```
Replace with:
```swift
    /// Region › Normalize…: asks for the peak level, then normalizes the
    /// selected regions (Ruling 18's default level).
    private func normalize(_ doc: DocumentModel) {
        guard
            let text = TextPanel.run(
                title: "Normalize",
                message: "Set the gain of each selected region so its loudest sample reaches this peak level (dB).",
                initial: String(RegionCommands.defaultPeakDb), ok: "Normalize", label: "Peak level in dB")
        else { return }
        guard let peak = RegionCommands.parsePeak(text) else {
            doc.alert = AlertItem(title: "Normalize", message: "Enter a level from −96 to 0 dB, like -0.3.")
            return
        }
        report(RegionCommands.normalize(peakDb: peak, doc), doc)
    }

    private func projectSettings() {
```

`commandItem` (5a/4f) takes the title from the key bindings (the registry's "Fade In to Cursor", …) and runs `RegionCommands.run`, reporting a refusal in the status line.

- [ ] **Step 7: Run the tests and build**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/swift.sh build 2>&1 | grep -E "error|warning: " | grep -v "build-system\|MemoryDefaults"
scripts/swift.sh test 2>&1 | tail -3
scripts/make-app.sh 2>&1 | tail -2
```
Expected: no errors and no new warnings; every test passes; the app bundle builds.

- [ ] **Step 8: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test 2>&1 | tail -3
```

- [ ] **Step 9: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAW/MediaPoolWindow.swift app/Sources/VizDAW/RelinkSheet.swift app/Sources/VizDAW/VizDAWApp.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/DocumentView.swift app/Sources/VizDAW/ProjectBanners.swift app/Sources/VizDAW/ProjectPanels.swift app/Sources/VizDAWAppModel/MediaModel.swift app/Tests/VizDAWAppModelTests/MediaPoolUITests.swift
git commit -m "app: Media Pool window, Relink sheet, Region menu

Window > Media Pool lists every item (name, duration, channels, origin,
uses, used %, offline mark) with Select Regions, Select Unused, Compact,
Rename, Relink, Reveal in Finder, Import and Delete (asking first when
regions use the item). A project that opens with missing or changed
media shows the Relink sheet (Locate, Search Folder, Leave Offline),
never under the control channel; File > Relink Media reopens it. The
Region menu has Normalize (asking for the level), the fades to the
cursor and Remove Fades.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```

---

### Task 13: End to end, and the by-eye checklist

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`
- Test: `crates/vizctl/tests/e2e.rs`

**Interfaces:**
- Consumes: everything above, through the control channel only; 5a's e2e helpers (`App`, `audio_file`, `labelled`, `wait_for_label`).
- Produces: the e2e test `regions_normalize_relink_and_trash`; every e2e launch gets `VIZDAW_TRASH_DIR=<home>/Trash`; README checklist steps 118–128 under "### Regions in the editors (plan 5b)".

The e2e drives README steps 121, 123–125 and 127 through the channel: import, a region selected by its accessibility element, Normalize with hostile and good levels, Save As carrying the overview cache, the retention rule moving an orphan into the trash folder (never unlinking it), a relink copying a moved file back under a free name, and the media pool window opened by ID.

- [ ] **Step 1: Write the test**

In `crates/vizctl/tests/e2e.rs`:

Find:
```rust
        let home = tempfile::Builder::new()
            .prefix("vze")
            .tempdir_in("/tmp")
            .unwrap()
            .keep();
        let launched = launch(&LaunchOptions {
            app,
            home: Some(home),
            demo: false,
            timeout: Duration::from_secs(180),
            env: vec![("VIZDAW_AUDIO".into(), "offline".into())],
        })
```
Replace with:
```rust
        let home = tempfile::Builder::new()
            .prefix("vze")
            .tempdir_in("/tmp")
            .unwrap()
            .keep();
        // Media retention moves files here, never to the real Trash (plan 5b).
        let trash = home.join("Trash").display().to_string();
        let launched = launch(&LaunchOptions {
            app,
            home: Some(home),
            demo: false,
            timeout: Duration::from_secs(180),
            env: vec![
                ("VIZDAW_AUDIO".into(), "offline".into()),
                ("VIZDAW_TRASH_DIR".into(), trash),
            ],
        })
```

Append to `crates/vizctl/tests/e2e.rs`:

```rust
/// The first node whose label starts with `prefix` and contains `needle`:
/// its ID and label.
fn labelled_like(nodes: &Value, prefix: &str, needle: &str) -> Option<(Value, String)> {
    nodes.as_array().into_iter().flatten().find_map(|n| {
        let l = n["label"].as_str().unwrap_or("");
        if l.starts_with(prefix) && l.contains(needle) {
            Some((n["id"].clone(), l.to_string()))
        } else {
            labelled_like(&n["children"], prefix, needle)
        }
    })
}

/// Polls the window's tree until such a node exists.
fn wait_for_label_like(app: &mut App, window: &Value, prefix: &str, needle: &str) -> (Value, String) {
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let tree = app.call("ui.tree", json!({"window": window}));
        if let Some(found) = labelled_like(&tree["nodes"], prefix, needle) {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "no {prefix:?}…{needle:?} within {WAIT_MS} ms in {tree}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// README steps 121, 123–125 and 127 through the channel (plan 5b):
/// a region selected by its element is normalized (hostile levels are
/// refused, a missing one names the dialog); Save As carries the
/// overview cache; an import undone and replaced by another edit goes to
/// the trash folder on save, not before; a moved file is relinked and
/// copied back under a free name; the media pool window opens by ID.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn regions_normalize_relink_and_trash() {
    let mut app = App::start();
    let home = app.launched.home.clone();
    let trash = home.join("Trash");
    // 2 s of stereo at 0.25 (−12.04 dBFS), and a short extra file.
    let tone = home.join("tone.caf");
    audio_file(&tone, 96_000);
    let extra = home.join("extra.caf");
    audio_file(&extra, 4_800);

    // The demo, with the tone in a new lane at bar 1.
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": tone, "seq": seq, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let m = app.call("state.get", json!({"target": "media", "window": w}));
    let item = m["items"][0].clone();
    assert_eq!((item["name"].clone(), item["used_percent"].clone()), (json!("tone"), json!(100.0)), "{m}");
    let media = item["id"].as_u64().unwrap();

    // Select the region by its element.
    let (region, label) = wait_for_label_like(&mut app, &w, "Region tone on ", "gain 0.0 dB");
    app.call("ui.click", json!({"window": w, "element": region}));
    let sel = app.call("state.get", json!({"target": "selection", "window": w}));
    assert_eq!(sel["events"].as_array().unwrap().len(), 1, "{label}: {sel}");

    // Normalize: no level names the dialog; nonsense is refused; -1 dB.
    let e = app.call_err("command.run", json!({"id": "region.normalize", "window": w}));
    assert_eq!(e.dialog.as_deref(), Some("Normalize"), "{e}");
    for bad in [json!("loud"), json!("nan"), json!("1"), json!(-1)] {
        let e = app.call_err(
            "command.run",
            json!({"id": "region.normalize", "params": {"peak_db": bad}, "window": w}),
        );
        assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{bad}: {e}");
    }
    let r = app.call(
        "command.run",
        json!({"id": "region.normalize", "params": {"peak_db": "-1"}, "window": w}),
    );
    assert_eq!(r["undo"], "Normalize", "{r}");
    // −1 − (−12.04) = +11.04 dB.
    wait_for_label_like(&mut app, &w, "Region tone on ", "gain +11.0 dB");

    // Save As carries the media and the overview cache.
    let pkg = home.join("Regions.vizdaw");
    app.call(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": pkg}, "window": w}),
    );
    assert!(pkg.join("Media/tone.caf").is_file());
    let caches = std::fs::read_dir(pkg.join("Media/.overviews"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "vzov"))
        .count();
    assert_eq!(caches, 1, "the overview came along");

    // An import, undone: redo can bring it back, so Save keeps its file.
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": extra}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    assert!(pkg.join("Media/extra.caf").is_file());
    app.call("command.run", json!({"id": "edit.undo", "window": w}));
    app.call("command.run", json!({"id": "file.save", "window": w}));
    assert!(pkg.join("Media/extra.caf").is_file(), "the redo history needs it");
    // Another edit drops the redo history; Save moves the orphan to the
    // trash folder. (An autosave written meanwhile would rightly keep it,
    // so none is left lying around.)
    let r = app.call(
        "command.run",
        json!({"id": "media.rename", "params": {"media": media, "name": "Tone"}, "window": w}),
    );
    assert_eq!(r["undo"], "Rename Media", "{r}");
    let _ = std::fs::remove_dir_all(pkg.join("Autosave"));
    app.call("command.run", json!({"id": "file.save", "window": w}));
    assert!(!pkg.join("Media/extra.caf").exists(), "nothing references it");
    assert!(trash.join("extra.caf").is_file(), "moved, never unlinked");

    // Relink: the tone moves out; without a path the dialog is named; the
    // relink copies it back under a free name (`tone.caf` is still the
    // undo history's).
    let moved = home.join("moved/tone.caf");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(pkg.join("Media/tone.caf"), &moved).unwrap();
    let e = app.call_err(
        "command.run",
        json!({"id": "media.relink", "params": {"media": media}, "window": w}),
    );
    assert_eq!(e.dialog.as_deref(), Some("Relink"), "{e}");
    let e = app.call_err(
        "command.run",
        json!({"id": "media.relink", "params": {"media": 999_999, "path": moved}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e}");
    let r = app.call(
        "command.run",
        json!({"id": "media.relink", "params": {"media": media, "path": moved}, "window": w}),
    );
    assert_eq!(r["undo"], "Relink Media", "{r}");
    let m = app.call("state.get", json!({"target": "media", "window": w}));
    let item = &m["items"][0];
    assert_eq!(
        (item["name"].clone(), item["path"].clone(), item["offline"].clone()),
        (json!("Tone"), json!("Media/tone 2.caf"), json!(false)),
        "{m}"
    );

    // The media pool window, by ID.
    app.call("command.run", json!({"id": "window.media_pool", "window": w}));
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let ws = app.call("state.get", json!({"target": "windows"}));
        if ws["windows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["kind"] == "media_pool")
        {
            break;
        }
        assert!(Instant::now() < deadline, "no media pool window in {ws}");
        std::thread::sleep(Duration::from_millis(200));
    }
    app.quit();
}
```

The arrangement shows the selected sequence (Song, from bar 1), whose last lane holds the region; `ui.click` on an element clicks its centre, which is the region's body (not a handle), so the click selects the block that holds just the region.

- [ ] **Step 2: Run the e2e**

The feature is built when this task runs; the test passes when Tasks 1–12 are complete. If it fails, the failure points at the task to fix.

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh && scripts/make-app.sh
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1 2>&1 | tail -15
pgrep -lf MacOS/VizDAW || echo "no VizDAW running"
```
Expected: `test result: ok. 6 passed` (5 earlier + this one); no VizDAW left running.

- [ ] **Step 3: Write the by-eye checklist**

In `app/README.md`:

Find:
```markdown
117. Save As during a job: in a saved project, import a long or large
     audio file (a few hundred MB, or several files at once) so the
     "Importing audio…" bar stays up for a while, and at once choose File
     > Save As…: it is refused with "Wait for the import or conversion
     to finish, then save." Once the bar ends, Save As works.
```
Replace with:
```markdown
117. Save As during a job: in a saved project, import a long or large
     audio file (a few hundred MB, or several files at once) so the
     "Importing audio…" bar stays up for a while, and at once choose File
     > Save As…: it is refused with "Wait for the import or conversion
     to finish, then save." Once the bar ends, Save As works.

### Regions in the editors (plan 5b)

Use a throwaway home (`vizctl launch`), a few audio files of your own
(one long, one stereo WAV), and the Finder.

118. Waveforms: open the demo and Import Audio… a stereo WAV onto a lane.
     Within a second its region in the arrangement shows the waveform of
     both channels. Zoom in until single samples show (the waveform may
     look blocky for a moment, then sharp) and out until the whole song
     fits: it stays drawn, scrolling never stalls. Open the Piano Roll on
     the sequence: the region strip draws the same waveform.
119. Fades: with the arrow tool, drag the region's top-left corner square
     to the right: the waveform ramps in and a line shows the fade; Edit
     > Undo says "Undo Fade In". Drag the top-right square to the left
     ("Fade Out"). Right-click inside a fade: "Fade In: Linear …
     S-Curve" with the current shape checked; choose S-Curve and listen.
     A region narrower than about 24 points shows no handles.
120. Gain: drag the region's top edge up: a readout shows "+3.0 dB" and
     the waveform grows; release: one step "Region Gain". With ⇧ the
     steps are finer. ⌥-click the top edge: back to "0.0 dB".
121. Region menu: select the region; Region > Normalize… offers -0.3;
     enter -1: the loudest peak reaches -1 dBFS (the waveform touches
     near the top), one step "Normalize". Put the playhead inside the
     region: Region > Fade In to Cursor and Fade Out from Cursor set the
     fades there; Remove Fades clears them. With nothing selected the
     items are disabled; with the playhead outside the region the status
     line says "the cursor is not inside a selected audio region".
122. Drag-in: drag a WAV from the Finder onto a lane at bar 3: a bar
     "Importing audio…", then a region at bar 3 (snapped to the grid).
     Drop two files below the last lane: two new lanes "Audio n". Drop a
     file on the Piano Roll's region strip: it lands on that region's
     lane. A text file renamed `.wav` among them is listed in a Problems
     bar.
123. Media Pool: Window > Media Pool lists Name, Duration, Channels,
     Origin, Uses, Used and Offline. Select Unused selects items no
     region uses; Compact removes them (one undo step). Rename… renames
     (the arrangement's VoiceOver label follows). Reveal in Finder shows
     the file. Import… adds files to the pool only. Delete on an item
     in use first lists its regions ("Song › Audio 3, bar 1"), then
     removes the item and those regions as one step. Double-click a row
     (or Select Regions): its sequence shows with its regions selected.
124. Offline and Relink: save, close, move one media file out of the
     package (Show Package Contents > Media), reopen: the "missing or
     changed" sheet lists it as Missing and its region is hatched and
     says "Offline". Locate… the moved file: the bar "Relinking media…",
     then the region plays again (the file is copied into Media/ as
     "<name> 2.caf"). Leave Offline closes the sheet; File > Relink
     Media… reopens it while something is offline. Search Folder… on the
     folder holding the moved file finds it even renamed. Replace a
     media file with another of the same length and format (copy one in
     under its name) and switch back to VizDAW: after a moment the item
     is "Changed" and silent; put the original back: it plays.
125. Retention: in a saved project, import a file, Undo, Save: the file
     is still in Media/. Make any other edit and Save: the file is now
     in the Finder's Trash (Put Back restores it). Compact an unused
     item and Save: its file stays (Undo could bring it back) until
     another edit and Save. Once, run `cargo test -p platform-macos --
     --ignored a_file_goes_to_the_real_trash`: a
     "vizdaw-trash-check-<pid>.txt" appears in the Trash (delete it).
126. Overview cache: quit, delete the package's `Media/.overviews`
     folder, reopen: waveforms come back within a second or two; a
     garbage file put there in place of a `.vzov` is replaced the same
     way.
127. Channel: with a region selected, `$V run region.normalize
     '{"peak_db": "-0.3"}'` answers with `"undo": "Normalize"`; `$V run
     region.normalize` fails naming the "Normalize" dialog; `$V run
     media.relink '{"media": 1, "path": "/abs/file.wav"}'` waits for the
     job; `$V state media` shows `used_percent` and, for a changed file,
     `"offline_reason": "changed"`; `$V run window.media_pool` opens the
     pool and `$V state windows` lists it as `media_pool`.
128. Look and access: switch macOS between Light and Dark appearance:
     regions, waveforms, handles and the offline hatching follow the
     system colours. With VoiceOver, regions read "Region <name> on
     <lane>, bar n, gain …"; the Relink sheet's buttons read "Locate
     <name>", "Search Folder…" and "Leave Offline".
```

- [ ] **Step 4: Run every gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | sort | uniq -c
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph 2>&1 | tail -3
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
git add crates/vizctl/tests/e2e.rs app/README.md
git commit -m "e2e: normalize a region, relink, and move an orphan to the trash

The channel selects a region by its element, normalizes it (refusing
nonsense levels and naming the dialog without one), saves as a package
carrying the overview cache, keeps an undone import's file until another
edit and save move it to the trash folder, relinks a moved file back
under a free name and opens the media pool. README checklist steps
118-128 cover regions in the editors by eye.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01N9fMcKfGadLNAh9zD1cirg"
git log -1 --oneline && git show --stat HEAD
```
