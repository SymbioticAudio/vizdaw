# VizDAW 5f-3 — External Sample Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** "Open in External Editor" on one audio region works like this:
- VizDAW writes the region's span (no gain, no fades, no dither) as a WAV in the project's rate and depth to `Media/Edits/<name> edit <n>.wav`.
- It opens that file in the sample editor chosen in Settings › Editing.
- It watches the file. Once the file's size and modification time have stayed the same for 1 s after a change, the file is imported (converted to CAF in the project format, origin `ExternalEdit { from }`).
- That region alone is relinked to the import (offset 0, the new length, fades clamped), as one undo step "External Edit".
- "Reload External Edit" re-checks the file by hand.
- The control channel's `region.open_external` and `region.reload_external` do the same; with `VIZDAW_EXTERNAL_EDITOR=stub` nothing is launched.

**Architecture:**
- **`core-model`:** `AudioCmd::SetSource { event, media, source_offset, length }` (fades clamped).
- **`core-media`:** `export::write_span`: frames `[start, start + frames)` of a VizDAW CAF as `<dir>/<stem> edit <n>.wav` (first free `n`), through 5f-2's flock'd partial and `rename_excl` (pre-flight I5).
- **`core-ffi`:**
  - `external.rs`: the copy's safe names, `EDITS_DIR`, `ExternalEditRow`, `ExternalEditStateRow`, the runtime `EditCopy` (absolute path), the region-span read, the copy writer and the untitled Trash helper.
  - `session.rs`: `Inner.edits`; retention keeps the copies while the project is open (package prefix stripped in `tidy`); close and an untitled Save As send them to the Trash; `Session::open_external`, `external_edit`, `apply_external_edit`; the External Edit job and its own apply (`apply_external_edit_work`), one undo step "External Edit".
- **Swift:**
  - `ExternalEditWatcher` (`VizDAWAppModel`): the 1 s stable rule over a `FileProbe`, a `FileWatching` and an injected clock; tested with temp files.
  - `ExternalEditModel` per document: Open, Reload, the tick, the job, `stopAll`; `ExternalTools` (opener, watching, probe, clock) set on the store by the app, fakes by default.
  - `ExternalEditing.swift` (`VizDAW` target only): `LiveFileWatching` (`DispatchSource`), `WorkspaceOpener` (`NSWorkspace`), the application chooser panel.
  - Settings › Editing "External Sample Editor"; the Region menu's two items; `RegionCommands.runExternal` / `runMenu`.
- **Channel, e2e and README:** `region.open_external { app? }` and `region.reload_external`; the e2e `edits_a_region_through_the_stub_editor`; README 185–188.

**Tech Stack:** Rust 2024 (`core-model`, `core-media`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6 / SwiftPM (Swift Testing; `Dispatch` and `AppKit` in the app target only), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5f-3 — External sample editor" (split from the former 5f row by plan 5f-1 Ruling 1). This plan delivers:
- §11 in full: the Settings chooser, the WAV of the span in the project format, opening, watching, the stable rule, the import as `ExternalEdit`, the one-region relink as "External Edit", watching until close, "Reload External Edit", and the copies under the retention rule;
- §12.1 `region.open_external` (on CI it uses a configured stub editor command);
- §12.3 "the external-edit watcher's stable-file rule (temp files)";
- spec Rulings 13, 26, 30 and 32, and the manual checklist's "external editor round trip" (README 186).

Read the whole spec (§3 binds), plan 5f-2 (its `core_media::export` partial-file logic is reused by `write_span`; its `ImportAs` is **not** used, Ruling 6), and plan 5b's retention rulings (`docs/superpowers/plans/2026-10-19-regions-in-editors.md`).

**Revision.** Written at `f3291d0`; revised at `9c2606a` (branch `sp5-audio`, after 5f-1 and 5f-2) to fold the pre-flight (`.superpowers/sdd/2026-11-02-external-editor/preflight.md`: 1 Critical, 11 Important, 15 Minor, every binding change adopted by the controller) and the owner answers of 2026-11-03. The table "Pre-flight and owner answers" below says where each went.

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root** (`scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`). Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` at or after `9c2606a`. Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`). No manifest changes: `Cargo.lock` and every `Cargo.toml` stay as they are.
  - Never commit `.superpowers/`, `.DS_Store` or generated files.
  - End every commit message with these two lines (use the `Co-Authored-By` line naming the model you actually are):
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
- **Clean room.**
  - Never read or copy anything from `~/workspace/SVP/research` or any Studio Vision material.
  - No Opcode, Midas, Mackie, Avid, Steinberg, Apple Logic or Soundcraft assets, names, artwork or branding.
  - No third-party editor is named in UI text: it says "External sample editor".
- **No GUI tools, no real devices, no real applications.**
  - Never use GUI, computer-use or accessibility-automation tools. Views are checked by the owner by eye from README steps 185–188.
  - **Tests never launch a real application.**
    - Swift tests use `FakeOpener`, which records and launches nothing. `ExternalTools()` defaults to `FakeOpener` and `FakeWatching`, so a model or store a test makes can launch or watch nothing real.
    - The e2e sets `VIZDAW_EXTERNAL_EDITOR=stub`: the app uses `StubOpener`, which records the path and launches nothing; the test edits the file itself.
    - No test calls `NSWorkspace` or makes a `DispatchSource` (`WorkspaceOpener` and `LiveFileWatching` live in the `VizDAW` target, which no test target links; the e2e exercises them in the built app).
  - **Programmatic and control-channel calls never open dialogs.** With no editor chosen, no `app` and no stub, `region.open_external` fails `unavailable` with "Choose an external editor in Settings › Editing." (`RegionCommands.noEditor`); so does a key or a surface button, whose status line shows it (owner answer 2026-11-03). Only the Region menu item opens the chooser (Ruling 5).
  - Launch VizDAW only through the e2e (Task 6) with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - Tests never open audio input or MIDI ports and never trigger the microphone prompt: every `core-ffi` test calls `force_offline()` and `reset_host()` through its `serial()` helper.
  - **Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any device.** Every session that saves, closes or Saves As gets `Session::debug_set_trash_dir` (a folder in its `tempfile` dir); with the `test-hooks` feature the core refuses the platform Trash anyway (`retention.rs` `platform()`), and its untitled scratch packages go to a per-process temp folder (`media::scratch_parent`). Swift tests use `MemoryDefaults()`, temp folders, `FakeProbe` paths and `scripts/swift.sh test`'s own `VIZDAW_TRASH_DIR`/`VIZDAW_SCRATCH_DIR`. The e2e's Trash is `VIZDAW_TRASH_DIR` in its throwaway home.
- **Test waits.**
  - `core-ffi` job waits poll `s.job(id)` with `s.idle()` under `common::wait_limit()` (`VIZDAW_TEST_WAIT_SECS`).
  - The watcher's 1 s rule is tested with an injected clock, never by sleeping a second. The temp-file tests write, then set the fake clock.
  - The e2e polls with its `WAIT_MS` conditions (`wait_for_undo`, `wait_for_state`).
- **Crates and dependencies.** Rust stable, edition 2024, license GPL-3.0-only. No new crate and no new dependency (Swift uses Foundation, Dispatch, AppKit and UniformTypeIdentifiers only; `core-media` already depends on `libc`, `core-ffi` does not and does not gain it: the exclusive rename stays in `core-media`, pre-flight I5).
- **Lints** as in every plan: `#![deny(missing_docs)]`, plus no unwrap, expect, indexing or panic outside tests.
- **Platform-neutral cores.** `core-model`, `core-media` and `core-control` stay free of OS APIs beyond what they already use (`core-media`'s `libc` flock and rename).
- **Linux CI.**
  - CI runs clippy and the tests on `ubuntu-latest`. Swift is macOS only.
  - Applying an edit imports a WAV, which only the macOS system importer reads (the CAF importer reads CAF). So the `core-ffi` tests that apply an edit and check the result are `#[cfg(target_os = "macos")]`, with the note "// macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)", as in `crates/core-ffi/tests/media_pool.rs`. Helpers only those tests use are gated the same way (no dead code on Linux).
  - Everything else runs everywhere: `SetSource`; `write_span`; the copy, its names and folder; retention, close, Save As and the untitled Trash; Reload's states; the hostile calls and the `Busy` races (on Linux the held External Edit job ends Failed, which those tests allow). The test projects import a generated 48 kHz CAF, which the CAF importer reads on Linux (pre-flight C1).
  - File watching (`DispatchSource`) and launching (`NSWorkspace`) are macOS only, in the `VizDAW` target; the stable rule and the model are Swift and run in `scripts/swift.sh test` on macOS.
  - Every task that changes Rust checks `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **Real-time rules.** Nothing here runs on the real-time thread. The relinked region reaches the engine through the existing document reload. No rt-check gate is needed beyond the final run in Task 6, which confirms nothing moved.
- **Media is write-once.**
  - The edit copy is a new file, never written over: `<name> edit <n>.wav` takes the first free `n` by an exclusive rename of `.<name> edit <n>.wav.partial` (`write_span`).
  - The edit is imported into a new CAF and a **new** media item (`MediaCmd::Add`, origin `ExternalEdit { from }`); the original item and its file are never changed, and other regions using the original keep it. `MediaCmd::Relink` is not used (it would move every region of the item).
  - Copies are moved to the Trash, never unlinked: at close (saved and untitled projects) and at an untitled project's Save As (owner answer 2026-11-03).
- **File compatibility.** `core_model::file::VERSION` stays 3. `MediaOrigin::ExternalEdit { from }` exists since 5a. No model field is added; edit copies are runtime state (Ruling 3).
- **FFI safety.** Every new export returns `Result<_, VizError>` inside `ffi_guard`.
- **UniFFI changes** (Task 3):
  - New `ExternalEditRow` (record), `ExternalEditStateRow` (enum: `Copy { row }`, `Gone`, `NoEdit`) and `JobKindRow::ExternalEdit`.
  - New `Session` methods `open_external`, `external_edit` and `apply_external_edit`.
  - `FakeSession` implements them. `ProjectMedia.swift`'s two `JobKindRow` switches gain `.externalEdit`.
- **Swift.** Generated bindings in Swift 5 mode; the targets and tests in Swift 6 mode with no new warnings. `DispatchSource` and `NSWorkspace` live only in the `VizDAW` target (`LiveFileWatching`, `WorkspaceOpener`, `ExternalEditorPanel`); `VizDAWAppModel` holds the protocols, the rule, the model, `StubOpener` and the fakes.
- **Control channel.**
  - The registry count moves from **145** (after 5f-2) to **147** in Task 5 (two IDs).
  - Hostile-input tests: `external_commands_check_their_params` (core-control), `externalCommandsCheckTheirArguments` (app), `hostile_external_calls_are_refused` (core).
- **Generated files** are never committed.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, both clippies, and `cargo test` for the crates touched (the workspace in Tasks 3 and 6).
  - Tasks 3–6 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 6 also runs `scripts/make-app.sh`, the e2e and the rt-check gates (`cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings`; `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures`).
- **Baselines and expected counts.** Measured after the 5f-2 final fix wave (5f-2 `progress.md`; `9c2606a` added no tests): **Rust 1871 passed / 16 ignored (macOS), Swift 946, e2e 12, registry 145, README last step 184.**

  | Task | Rust (macOS) | Rust (Linux) | Swift | Other |
  |---|---|---|---|---|
  | 1 `SetSource` | +4 | +4 | — | |
  | 2 `write_span` | +2 | +2 | — | |
  | 3 core-ffi | +12 | +7 | +0 (compiles) | |
  | 4 watcher, model | — | — | +17 | |
  | 5 channel, menu | +1 | +1 | +8 | registry 147 |
  | 6 e2e, README | ignored +1 | ignored +1 | — | e2e 13, README 185–188 |
  | **After** | **1890 / 17 ignored** | +14 | **971** | |

  Report the measured counts at every task end; a difference is explained, never hidden.
- **How code is given.** As in plans 5f-1 and 5f-2: whole new files where their text matters (every new test file and every test is given whole), then Find/Replace excerpts. Each `Find in` block occurs exactly once in the named file at `9c2606a` (or after the earlier steps of this plan; no two excerpts overlap). Where a name differs from the tree, use the tree's and say so in the report.
- **Spec values used verbatim:**
  - "writes the region's span (`source_offset`, `length`, with no gain or fades) as WAV at the project rate and depth to `Media/Edits/<name> edit <n>.wav`";
  - "watches it with a `DispatchSource` file-system-object watch for write, rename or delete events";
  - "A change is accepted once the file's size and modification time have stayed the same for 1 s";
  - "converted to CAF in the project format, origin `ExternalEdit { from }`";
  - "That region alone is relinked to it: `media` = new, `source_offset` = 0, `length` = the new file's length, with fades clamped. This is one undo step, "External Edit". Other regions using the original are unaffected";
  - "Watching lasts until the project closes. "Reload External Edit" on a region whose media came from an external edit re-checks the `.wav` by hand later";
  - "The `.wav` copies follow the media retention rule (§6.5) once nothing refers to them".

## Review Focus

These are the five failure modes a user is most likely to hit that the spec implies but does not spell out, most likely first. Each is pinned by named tests in its task.

1. **Editors that save by replacing the file** (write a temp file, rename it over, or delete and recreate), save twice in a second, save while VizDAW applies the last change, or save while still writing.
   - Expected:
     - a rename or delete re-arms the watch on the new file at the same path;
     - only a file that has been stable for 1 s is imported, and a file still growing without an event is not (pre-flight M1);
     - a second save within that second restarts the wait;
     - one edit makes one undo step;
     - a save during a running import is picked up after it.
   - Pinned by `aRenameOverTheFileRearmsTheWatch`, `aSecondSaveRestartsTheSecond`, `aFileStillGrowingIsNotDue`, `oneChangeIsAppliedOnce` (Task 4 watcher), `aStableChangeStartsOneImport`, `aSaveDuringAnImportIsAppliedAfterIt` (Task 4 model), and the e2e's second, in-place save after the rename-over save (Task 6): only a watch re-armed on the replaced inode sees it (pre-flight I8).
2. **What the editor hands back.** A different length; a different rate; mono instead of stereo; more than two channels; an empty or truncated file; garbage; a file deleted and never recreated.
   - Expected:
     - the region takes the new length, with its fades clamped (fade in first, then fade out within what is left);
     - another rate is converted to the project rate;
     - mono or stereo is accepted;
     - more than two channels, an empty file or an unreadable file is refused with a clear message: the job is **Failed** (never Done, pre-flight I3), the document is unchanged, and what it imported is removed;
     - a deleted file stays watched for its return; Reload meanwhile says the copy is gone.
   - Pinned by `fades_are_clamped_to_the_new_length` (Task 1), `a_shorter_mono_edit_relinks_the_region` and `an_unreadable_or_wide_edit_changes_nothing` (Task 3), and `aDeletedFileWaitsForItsReturn` (Task 4).
3. **The region or project changes meanwhile.** The region is deleted, split or moved while the editor is open; the project is saved, saved under a new name, or closed; it is untitled; Undo of the "External Edit".
   - Expected:
     - a deleted region's edit is dropped with "The region was deleted; the edit was not applied.", what it imported is removed, and that region's watch stops (pre-flight M10);
     - a moved region is relinked where it is now;
     - a split relinks only the part with the original event ID (offset 0, the whole edit), the other part keeps the original;
     - a Save keeps the copy; closing stops the watches and sends the copies to the Trash;
     - a titled Save As leaves the copies in the old package, still watched and still applying (pre-flight I1);
     - an untitled project's copies go to the Trash at close and at Save As, never unlinked (owner answer 2026-11-03);
     - Undo restores the original media and span, and Redo the edit.
   - Pinned by `an_edit_of_a_deleted_region_is_dropped`, `a_split_or_moved_region_is_relinked_where_it_is`, `copies_are_kept_until_close_then_trashed`, `save_as_leaves_the_copies_where_they_are`, `an_untitled_projects_copies_go_to_the_trash`, `undo_restores_the_original_span` (Task 3), `closingStopsEveryWatch` and `aDeletedRegionStopsItsWatch` (Task 4).
4. **Names and folders.** A media name with `/`, `:`, control characters, a leading `.`, nothing, or 300 characters; a copy that already exists from an earlier edit; a crash's stale partial; `Media/Edits` missing or a file.
   - Expected: the stem is made safe (`/`, `\`, `:` and control characters become `-`; a leading `.` becomes `-`; an empty stem is "Audio"; cut to at most 200 UTF-8 bytes at a character boundary, pre-flight M3), the first free `n` is taken, a stale partial is cleared, `Media/Edits` is created when missing, and a file in its place is refused with a message.
   - Pinned by `spans_take_the_first_free_number_and_clear_a_stale_partial` (Task 2) and `edit_copies_get_safe_free_names` (Task 3).
5. **Hostile or careless calls.** `region.open_external` on a MIDI note, on two regions, on none, on an offline region; an `app` that is relative, not `.app`, missing or a plain file; `region.reload_external` on a region with no edit; a 70 000-byte path; an apply racing another apply or a Format job.
   - Expected: each is refused with a clear message, and nothing is written or launched; the races are `Busy` both ways (pre-flight I11).
   - Pinned by `hostile_external_calls_are_refused` (Task 3), `externalCommandsCheckTheirArguments` and `external_commands_check_their_params` (Task 5), and the e2e's hostile calls (Task 6).

## Plan Rulings

Each ruling decides something the spec leaves open, in the format "what — why — cost if wrong".

1. **Ruling (relinking one region): `AudioCmd::SetSource { event, media, source_offset, length }` changes one region's media and span.**
   - It refuses (through `store` → `check_event_fields`) a span past the new media's end, `length == 0`, an unknown media, a non-region and an unknown event. It clamps fades with the existing `clamp_fades`: `fade_in = min(fade_in, length)`, then `fade_out = min(fade_out, length − fade_in)`.
   - Its name is "Set Region Source". The external edit runs it inside `history.begin("External Edit")`, after `MediaCmd::Add`, so the step is labelled "External Edit".
   - The region's start tick, gain and fade shapes stay. (Regions store no tick duration; pre-flight M5.)
   - A region inside a referenced sequence is one event: every instance of the reference plays the edit (pre-flight M6).
   - Why: §11's relink of that region alone. `MediaCmd::Relink` would move every region using the item.
   - Cost if wrong: none expected.
2. **Ruling (the copy): the span is copied by `core_media::export::write_span` from the region's media file.**
   - Format: WAV; the project's rate (the media's own: an online item is in the project format, and `write_span` refuses another rate); the project's depth; **dither off**. The source already holds project-depth samples, so the copy is bit-exact at 16 and 24 bits; float is written as float (WAV format 3) (pre-flight I4).
   - It goes to `Media/Edits/<stem> edit <n>.wav` through `.<stem> edit <n>.wav.partial`, with 5f-2's flock'd partial logic: a stale partial (its lock free) is removed, one another writer holds skips that number, a taken name skips that number; the partial is removed on any error; the rename is `rename_excl` (pre-flight I5).
   - `<stem>` is the media item's name made safe (`external::safe_stem`, Review Focus 4). `n` starts at 1 and goes to `export::MAX_NUMBER`.
   - An offline region (missing, unreadable, other format, changed) is refused: "The region's audio is offline; relink it first."
   - `open_external` reads the path, span and format under the session lock, writes **without** it (hundreds of MB must not hold the main thread's lock; pre-flight M4), then re-locks to record the copy, refusing if the region changed meanwhile ("The region changed while its copy was written; try again."; the file is left to retention). While a copy is written, retention waits (`Inner.edits_writing`).
   - `Busy` while a Format, Convert or Relink job is unapplied (each replaces media files).
   - Why: §11 and spec Ruling 32.
   - Cost if wrong: none expected.
3. **Ruling (edit copies are runtime state, kept until close): the session keeps `edits: BTreeMap<EventId, EditCopy { abs: PathBuf, from: MediaId }>`.**
   - `abs` is **absolute** (pre-flight I1). `Inner::tidy` inserts `abs` stripped of the package prefix, `/`-joined, only when it lies under that package: retention's paths are package-relative. A titled Save As leaves `abs` where it was, so watching and applying go on at the old package, whose files stay until that package is next saved or closed (pre-flight M14; README 188). `apply_external_edit` compares absolute paths.
   - Rust keeps no "applied" stamp: Swift owns the stable rule (Ruling 4; pre-flight M2).
   - At a saved project's close the map is cleared before the final tidy, so the copies go to the Trash with the other unreferenced files.
   - An **untitled** project's copies go to the Trash too, never unlinked (owner answer 2026-11-03; pre-flight I2): its close puts the files of the scratch package's `Media/Edits` through the same Trash loop as `Media/Recorded` (counted in `left`; the scratch package stays if any is left); its Save As trashes them before the scratch package is removed (Save As does not carry copies). The next apply of such a copy, and Reload, say "The edit copy is gone; Open in External Editor again."
   - Reload after the project is reopened says the same: `external_edit` answers `Gone` for a region whose media's origin is `ExternalEdit` and that has no copy in this session, or whose copy is not on disk; `NoEdit` for any other region (pre-flight I10).
   - Why: the spec's "watching lasts until the project closes", and the retention rule "once nothing refers to them". Nothing in the model refers to a copy, so no file-format field is needed (owner-accepted R3).
   - Cost if wrong: after reopening, "Reload External Edit" says the copy is gone; keeping copies across sessions needs a saved list (a later format change).
4. **Ruling (where the stable rule lives): in Swift (`ExternalEditWatcher`), driven by the 60 Hz document tick (`DocumentModel.idle`) with an injected clock and file probe.**
   - The core only imports what it is handed (`apply_external_edit(event, path)`), and refuses a path that is not the event's copy.
   - The rule:
     - each watch event records the time and the probe's `(size, mtime)` then;
     - a tick whose probe differs from the recorded pair records a new change itself (an event lost or coalesced, a file still growing; pre-flight M1);
     - a tick at least 1 s after the recorded change, with the probe equal to it, existing, and different from the pair last **applied** and the pair last **skipped**, is due;
     - Open records the pair it wrote as applied, so Reload right after Open is "unchanged" (pre-flight I10);
     - a region with an import running is not due until the import ends; a Done import marks its pair applied; a Failed or Cancelled one marks it skipped (not retried by itself, watch kept armed; Reload retries it); a `Busy` refusal is retried at the next tick, without an alert (pre-flight I11);
     - Reload asks for no second: the user asked.
   - `FileWatching.watch` returns nil for a path it cannot open; the model then waits for Reload (pre-flight M15).
   - Why: §11 names `DispatchSource` (an app API). §12.3 tests the rule in Swift with temp files.
   - Cost if wrong: none expected.
5. **Ruling (the editor application): Settings › Editing "External Sample Editor" stores the chosen `.app` path in preferences (`VizDAW.externalEditor`), chosen with an `NSOpenPanel` limited to applications (owner-accepted R5).**
   - "Open in External Editor" uses it, through `NSWorkspace.open(_:withApplicationAt:configuration:completionHandler:)`. `WorkspaceOpener.open` checks only that the app exists; the system's later failure, in its completion, becomes the alert "Open in External Editor Failed" on the main actor, and the watch stays (pre-flight M9).
   - With none chosen, **only the Region menu item** opens the chooser first (a menu action, not a programmatic call). A key, a surface button or the channel gets `unavailable` "Choose an external editor in Settings › Editing." (owner answer 2026-11-03; pre-flight M12); `KeyDispatch.after` shows that message in the status line and uses the key up, because passing it on would reach the menu item and its panel.
   - `region.open_external { app? }` takes `app` (an absolute path ending in `.app`, checked by core-control) over the preference; the app refuses before anything is written an `app` that is missing or not a folder: "<path> is not an application." (pre-flight I11). The registry declares the dialog `Choose External Editor` (`always: false`, answered by `app`) so `state.get commands` documents it (pre-flight M13).
   - `VIZDAW_EXTERNAL_EDITOR=stub` makes the app use `StubOpener`, which launches nothing, and `ControlHost.externalEditor` answers `.stub` (pre-flight I7(c)): the channel then needs no `app`.
   - Why: §11 and §12.1's "configured stub editor command".
   - Cost if wrong: none expected.
6. **Ruling (the job): `JobKindRow::ExternalEdit` imports the copy with the system importer into the project format (`import_reserved`, stem = the copy's file stem, "`<media name> edit <n>`"), and has its own apply, `apply_external_edit_work` (pre-flight I3).**
   - Not `ImportAs`: its `origin` is a `fn` pointer (it cannot capture `from`), and `apply_import_as` ends **Done** when every file failed.
   - The apply: an import error is **Failed** with its message; more than one item (a file wider than stereo, which `import_reserved` splits) is Failed "The edit has <n> channels; regions hold one or two.", every item removed; a region that is gone is Failed "The region was deleted; the edit was not applied.", the item removed and the region's copy forgotten, so its watch stops (pre-flight M10); otherwise `begin("External Edit")`, `MediaCmd::Add` (origin `ExternalEdit { from }`), `SetSource { event, media, 0, frames }`, `commit`; any error aborts, removes the item and is Failed.
   - `discard` gains the `Work::ExternalEdit` arm (what it imported; the copy stays). `ImportAs` is left unchanged.
   - `apply_external_edit` is `Busy` inside an open edit and while a Format, Convert, Relink, Bounce or External Edit job is unapplied: `JobKindRow::ExternalEdit` joins `Inner::media_jobs_idle`'s list, which it calls as Bounce does. So two applies never overlap (one session imports one edit at a time; the others retry at the next tick), and a Format job never starts beside an edit, both ways (pre-flight I11).
   - Why: §11's import and one-step relink; a refused edit must not look applied.
   - Cost if wrong: none expected.
7. **Ruling (the channel and the menu):**
   - `region.open_external { app? }` acts on the editor selection, which must be exactly one event (else `unavailable` "Select one audio region first."); the core refuses a non-region. Its answer status is `"editing <absolute path of the copy>"`.
   - `region.reload_external` acts on the same selection: the re-check's import job answers when it ends (`.job`); `ran` with status `"unchanged"` when nothing changed; `failed` "The edit copy is gone; Open in External Editor again." for a gone copy; `unavailable` "This region has no external edit; use Open in External Editor." for a region with none; `busy` while its import runs.
   - The channel's Reload starts its job directly (the channel answers it: no banner, no second report); the menu's Reload and the watcher's imports go through `DocumentModel.startJob(.externalEdit, …)`, so the banner and a failure's **single** alert come from `pollJobs` (pre-flight I7(d)).
   - Both are Region-menu items ("Open in External Editor", "Reload External Edit"), explicit Buttons calling `RegionCommands.runMenu`, never `commandItem` (pre-flight I6). `RegionCommands.run` answers `unavailable` for both and never sends an edit. Keys, surface buttons and the channel reach `RegionCommands.runExternal` through `ControlCommands.run`.
   - Why: §12.1. The e2e needs the copy's path.
   - Cost if wrong: one more answer field.

## Pre-flight and owner answers (folded)

| Item | Where |
|---|---|
| C1 offline demo | Task 3 `build()` (new project, imported 48 kHz CAF, gain, fades, split); Task 6 e2e step 2 (`file.create_demo` + `media.import` + `ui.click`) |
| I1 absolute copy path | Ruling 3; Task 3 `EditCopy.abs`, `package_relative` in `tidy`; tests `copies_are_kept_until_close_then_trashed` (two saves), `save_as_leaves_the_copies_where_they_are` |
| I2 / owner answer 1 untitled copies | Ruling 3; Task 3 close and Save As replacements, `trash_files_in`; test `an_untitled_projects_copies_go_to_the_trash` |
| I3 own apply | Ruling 6; Task 3 `apply_external_edit_work` |
| I4 no dither | Ruling 2; Task 2 `a_span_is_written_exactly`; Task 3 `the_copy_is_the_span…` (16, 24, 32f) |
| I5 `write_span` | Task 2 (new function, 2 tests) |
| I6 `run` routing | Ruling 7; Task 5 `RegionCommands.run`, menu Buttons; `theMenuItemsAreEnabledForOneRegion` |
| I7 Swift wiring | Task 4 `ExternalTools`, `DocumentStore`, `VizDAWApp`, `DocumentStore.close` → `stopExternalEdits`; Task 5 `ExternalEditorChoice`, `ControlHost`, `FakeControlHost`, file lists |
| I8 tests as code | every test below is code; e2e step 5 (in-place second save); `aCancelledImportStaysArmed` |
| I9 counts | Global Constraints table; registry 145 → 147 |
| I10 Reload | Rulings 3, 4, 7; `ExternalEditStateRow`; tests `reload_finds_the_copy_or_says_it_is_gone`, `reloadAfterReopenSaysTheCopyIsGone`, `reloadRightAfterOpenIsUnchanged` |
| I11 hostile, races | Rulings 5, 6; `hostile_external_calls_are_refused`, `externalCommandsCheckTheirArguments`, `aBusyImportIsRetried` |
| M1 lost events | Ruling 4; `aFileStillGrowingIsNotDue` |
| M2 Rust stamp | dropped (Ruling 3) |
| M3 safe names | Ruling 2; `safe_stem`; `edit_copies_get_safe_free_names` |
| M4 lock | Ruling 2; `open_external`, `edits_writing` |
| M5 tick duration | clause deleted (Ruling 1) |
| M6 split, moved, references | Ruling 1; `a_split_or_moved_region_is_relinked_where_it_is` |
| M7 wide fixture | `wav_at` copied from `media_pool.rs` |
| M8 `Media/*.caf` | `cafs()` helper |
| M9 async open failure | Ruling 5; `WorkspaceOpener`; `openFailuresAreReported` |
| M10 deleted region's watch | Rulings 6, 4; `aDeletedRegionStopsItsWatch` |
| M11 e2e step wording | e2e step 4 (`.tmp.wav` beside the copy, renamed over it) |
| M12 / owner answer 2 keys | Ruling 5; `KeyDispatch.after`; `aKeyWithNoEditorShowsTheSettingsMessage` |
| M13 dialog spec | Ruling 5; Task 5 registry |
| M14 old package | Ruling 3; README 188 |
| M15 counts, nil watch | Global Constraints table; Ruling 4 |

## Carried items

- None name the external editor. Spec Rulings 13 (retention), 26, 30 and 32 bind (Rulings 1–3).

## File map

| File | Task | Role |
|---|---|---|
| `crates/core-model/src/command/audio.rs`, `crates/core-model/tests/cmd_external.rs` | 1 | `SetSource` |
| `crates/core-media/src/export.rs`, `crates/core-media/tests/export.rs` | 2 | `write_span` |
| `crates/core-ffi/src/external.rs` (new), `src/session.rs`, `src/jobs.rs`, `src/lib.rs`, `tests/external.rs` (new); `app/Sources/VizDAWAppModel/ProjectMedia.swift`; `app/Tests/VizDAWAppModelTests/FakeSession.swift` | 3 | copy, copies, retention, job, apply |
| `app/Sources/VizDAWAppModel/{ExternalEditWatcher.swift,ExternalEditModel.swift}` (new), `DocumentModel.swift`, `DocumentStore.swift`; `app/Sources/VizDAW/ExternalEditing.swift` (new), `VizDAWApp.swift`; `app/Tests/VizDAWAppModelTests/{ExternalEditWatcherTests.swift,ExternalEditModelTests.swift}` (new) | 4 | watcher, openers, model, wiring |
| `crates/core-control/src/registry.rs`, `crates/core-control/tests/commands.rs`; `app/Sources/VizDAWAppModel/{RegionCommands.swift,ControlCommands.swift,KeyBindings.swift,Nudge.swift}`; `app/Sources/VizDAW/{EditingSettingsView.swift,AppCommands.swift,ControlChannel.swift}`; `app/Tests/VizDAWAppModelTests/{ExternalCommandsTests.swift (new),ControlCommandsTests.swift}` | 5 | channel, menu, keys, Settings |
| `crates/vizctl/tests/e2e.rs`, `app/README.md` | 6 | e2e, README |

---

### Task 1: `AudioCmd::SetSource`

**Files:**
- Modify: `crates/core-model/src/command/audio.rs`
- Test: create `crates/core-model/tests/cmd_external.rs`

**Interfaces:**
- Consumes: `region`, `clamp_fades`, `store` (`audio.rs`), `check_event_fields` (`rules.rs`).
- Produces: `AudioCmd::SetSource { event: EventId, media: MediaId, source_offset: u64, length: u64 }`, named "Set Region Source" (Ruling 1). `Command::name` delegates to `AudioCmd::name`; no other exhaustive `AudioCmd` match exists outside `audio.rs`.

Find in `crates/core-model/src/command/audio.rs`:
```rust
    /// Removes both fades of every region.
    RemoveFades {
        /// The events (non-regions are ignored).
        events: Vec<EventId>,
    },
}
```
Replace:
```rust
    /// Removes both fades of every region.
    RemoveFades {
        /// The events (non-regions are ignored).
        events: Vec<EventId>,
    },
    /// Points one region at `media` from `source_offset` for `length`
    /// frames, its fades clamped (spec §11; plan 5f-3 Ruling 1). Its start
    /// tick, gain and fade shapes stay.
    SetSource {
        /// The region.
        event: EventId,
        /// The new media item.
        media: MediaId,
        /// Start within it (frames).
        source_offset: u64,
        /// Length (frames, >= 1).
        length: u64,
    },
}
```

Find in `crates/core-model/src/command/audio.rs`:
```rust
            AudioCmd::Normalize { .. } => "Normalize",
```
Replace:
```rust
            AudioCmd::Normalize { .. } => "Normalize",
            AudioCmd::SetSource { .. } => "Set Region Source",
```

Find in `crates/core-model/src/command/audio.rs`:
```rust
                    "select audio regions to remove their fades",
                ));
            }
        }
    }
    Ok(())
}
```
Replace:
```rust
                    "select audio regions to remove their fades",
                ));
            }
        }
        AudioCmd::SetSource {
            event,
            media,
            source_offset,
            length,
        } => {
            // `region` refuses a non-region and an unknown event; `store`
            // refuses an unknown media, a span past its end and length 0.
            let (mut ev, _, _) = region(d, *event)?;
            if let EventBody::AudioRegion {
                media: m,
                source_offset: o,
                length: l,
                ..
            } = &mut ev.body
            {
                *m = *media;
                *o = *source_offset;
                *l = *length;
            }
            clamp_fades(&mut ev);
            store(d, ev)?;
        }
    }
    Ok(())
}
```

- [ ] **Step 1: Write the failing tests.** Create `crates/core-model/tests/cmd_external.rs`:

```rust
//! `AudioCmd::SetSource` (plan 5f-3 Task 1, Ruling 1; spec §11): one
//! region points at new media, its fades clamped, as one undoable command.

use core_model::ModelError;
use core_model::command::{AudioCmd, Command, EventCmd, MediaCmd, SequenceCmd, apply};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::history::History;
use core_model::ids::{EventId, LaneId, MediaId, SeqId};
use core_model::selection::ViewFilter;
use core_model::time::Tick;

fn run(d: &Document, c: Command) -> core_model::Result<Document> {
    apply(d, &c).map(|a| a.doc)
}

fn fade(n: u64) -> Fade {
    Fade {
        length_frames: n,
        shape: FadeShape::Linear,
    }
}

fn add_media(d: &Document, path: &str, frames: u64) -> Document {
    run(
        d,
        Command::Media(MediaCmd::Add {
            path: path.into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: frames,
            content_hash: None,
            name: None,
            origin: None,
        }),
    )
    .unwrap()
}

fn region_at(d: &Document, tick: i64) -> Document {
    run(
        d,
        Command::Audio(AudioCmd::Insert {
            seq: SeqId(0),
            lane: LaneId(0),
            tick: Tick(tick),
            media: MediaId(0),
            source_offset: 1000,
            length: 4000,
            gain_db: -6.0,
            fade_in: fade(800),
            fade_out: fade(600),
        }),
    )
    .unwrap()
}

/// Sequence 0 with lane 0; media 0 (`Media/a.caf`, 48 000 frames) and 1
/// (`Media/b.caf`, 2000 frames); regions 0 (tick 0) and 1 (tick 96 000)
/// on media 0 (offset 1000, 4000 frames, −6 dB, fades 800 in and 600
/// out); a note (event 2).
fn doc() -> Document {
    let d = run(
        &Document::new(),
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    )
    .unwrap();
    let d = add_media(&d, "Media/a.caf", 48_000);
    let d = add_media(&d, "Media/b.caf", 2_000);
    let d = region_at(&d, 0);
    let d = region_at(&d, 96_000);
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

fn set_source(event: u64, media: u64, source_offset: u64, length: u64) -> Command {
    Command::Audio(AudioCmd::SetSource {
        event: EventId(event),
        media: MediaId(media),
        source_offset,
        length,
    })
}

/// `(media, source_offset, length, fade in, fade out, tick, gain)` of
/// region `e`.
fn body(d: &Document, e: u64) -> (MediaId, u64, u64, u64, u64, Tick, f32) {
    let (ev, _) = d.event(EventId(e)).unwrap();
    match &ev.body {
        EventBody::AudioRegion {
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => (
            *media,
            *source_offset,
            *length,
            fade_in.length_frames,
            fade_out.length_frames,
            ev.tick,
            *gain_db,
        ),
        _ => panic!("event {e} is not a region"),
    }
}

#[test]
fn set_source_relinks_one_region_only() {
    let d = doc();
    let c = set_source(0, 1, 0, 1000);
    assert_eq!(c.name(), "Set Region Source");
    let after = run(&d, c).unwrap();
    let (media, offset, length, _, _, tick, gain) = body(&after, 0);
    assert_eq!(
        (media, offset, length, tick, gain),
        (MediaId(1), 0, 1000, Tick(0), -6.0)
    );
    assert_eq!(body(&after, 1), body(&d, 1), "the other region still plays media 0");
    assert_eq!(after.media().len(), 2, "no media is added or removed");
}

#[test]
fn fades_are_clamped_to_the_new_length() {
    let d = doc();
    // 800 in and 600 out: 1000 frames keep the fade in and leave 200.
    let a = run(&d, set_source(0, 1, 0, 1000)).unwrap();
    assert_eq!((body(&a, 0).3, body(&a, 0).4), (800, 200));
    // 500 frames: the fade in takes them all.
    let b = run(&d, set_source(0, 1, 500, 500)).unwrap();
    assert_eq!((body(&b, 0).3, body(&b, 0).4), (500, 0));
    // Long enough for both: they stay.
    let c = run(&d, set_source(0, 1, 0, 2000)).unwrap();
    assert_eq!((body(&c, 0).3, body(&c, 0).4), (800, 600));
}

#[test]
fn set_source_refuses_bad_spans() {
    let d = doc();
    for (e, m, offset, length) in [
        (0, 1, 1500, 501),    // past media 1's end
        (0, 1, 0, 0),         // no frames
        (0, 1, u64::MAX, 2),  // an offset that overflows
        (0, 7, 0, 10),        // an unknown media item
        (2, 1, 0, 10),        // a note
        (99, 1, 0, 10),       // an unknown event
    ] {
        let c = set_source(e, m, offset, length);
        assert!(run(&d, c.clone()).is_err(), "{e} {m} {offset} {length}");
        let mut h = History::new(d.clone());
        assert!(h.execute(&c, &ViewFilter::default()).is_err());
        assert_eq!(h.doc(), &d, "the document is unchanged");
    }
    assert!(matches!(
        run(&d, set_source(0, 1, 0, 0)),
        Err(ModelError::OutOfRange { .. })
    ));
    assert!(matches!(
        run(&d, set_source(0, 7, 0, 10)),
        Err(ModelError::UnknownId(_))
    ));
    assert!(matches!(
        run(&d, set_source(99, 1, 0, 10)),
        Err(ModelError::UnknownId(_))
    ));
}

#[test]
fn undo_of_set_source_restores_the_span() {
    let d = doc();
    let mut h = History::new(d.clone());
    h.execute(&set_source(0, 1, 0, 1000), &ViewFilter::default())
        .unwrap();
    assert_eq!(h.undo_label(), Some("Set Region Source"));
    let edited = h.doc().clone();
    h.undo().unwrap();
    assert_eq!(h.doc(), &d, "the exact old body");
    h.redo().unwrap();
    assert_eq!(h.doc(), &edited);
}
```

- [ ] **Step 2: Run them to see them fail.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test cmd_external
```
Expected: FAIL to compile — no variant `SetSource`.

- [ ] **Step 3: Implement** the three excerpts above.

- [ ] **Step 4: Run the tests and the gates.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo test -p core-model
cargo test -p core-ffi --lib
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings
```
Expected: PASS; Rust +4.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/command/audio.rs crates/core-model/tests/cmd_external.rs
git commit -m "core-model: AudioCmd::SetSource points one region at new media with its fades clamped (plan 5f-3 Task 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -4
```
Counts: Rust +4 (1875 / 16 ignored on macOS).

---

### Task 2: `core_media::export::write_span`

**Files:**
- Modify: `crates/core-media/src/export.rs`, `crates/core-media/tests/export.rs`

**Interfaces:**
- Consumes (all already in `export.rs` or `import.rs`): `CafReader`, `PcmWriter`, `check_dest`, `partial_path`, `clear_stale_partial`, `lock_own_partial`, `PartialGuard`, `sync_parent`, `display_name`, `MAX_NUMBER`, `crate::import::rename_excl` (crate-private, which is why the copy is written here and not in `core-ffi`; pre-flight I5).
- Produces:
  ```rust
  pub fn write_span(source: &Path, start: u64, frames: u64, dir: &Path, stem: &str, fmt: &ExportFormat) -> Result<Exported, MediaError>
  ```

Find in `crates/core-media/src/export.rs`:
```rust
    check_destination(dest, fmt, replace)?;
    write_partial(
        importer, source, dest, fmt, frames, replace, progress, cancel,
    )?
    .commit()
}
```
Replace:
```rust
    check_destination(dest, fmt, replace)?;
    write_partial(
        importer, source, dest, fmt, frames, replace, progress, cancel,
    )?
    .commit()
}

/// Frames a span copy reads at a time.
const SPAN_CHUNK: usize = 8192;

/// Frames `[start, start + frames)` of the VizDAW CAF `source` as
/// `<dir>/<stem> edit <n>.wav` for the first free `n` (1..=[`MAX_NUMBER`];
/// plan 5f-3 Ruling 2, pre-flight I4, I5): no gain, no fades, no dither,
/// no rate change, so a source at `fmt.depth` is copied bit for bit.
/// Written through `.<stem> edit <n>.wav.partial`: a partial a crash left
/// (its lock free) is removed and the number used; a name that exists, or
/// a partial another writer holds, skips to the next number. The partial
/// is moved into place by `rename_excl`, never over a file, and removed on
/// any error. Refuses before writing: `fmt` not WAV or with dither, a rate
/// other than the source's, an empty span or one past the source's end,
/// a folder that is missing or cannot be written, a name too long.
pub fn write_span(
    source: &Path,
    start: u64,
    frames: u64,
    dir: &Path,
    stem: &str,
    fmt: &ExportFormat,
) -> Result<Exported, MediaError> {
    if fmt.container != Container::Wav || fmt.dither {
        return Err(MediaError::Unsupported(
            "an edit copy is a WAV without dither".into(),
        ));
    }
    let r = CafReader::open(source)?;
    let (rate, channels, have) = (r.info().sample_rate, r.info().channels, r.info().frames);
    if rate != fmt.rate {
        return Err(MediaError::Unsupported(format!(
            "{rate} Hz to {} Hz: an edit copy keeps the media's rate",
            fmt.rate
        )));
    }
    let end = start
        .checked_add(frames)
        .filter(|e| frames > 0 && *e <= have)
        .ok_or_else(|| {
            MediaError::Unsupported(format!(
                "frames {start} to {} of a file of {have}",
                start.saturating_add(frames)
            ))
        })?;
    let ch = usize::from(channels);
    for n in 1..=MAX_NUMBER {
        let dest = dir.join(format!("{stem} edit {n}.wav"));
        if std::fs::symlink_metadata(&dest).is_ok() {
            continue;
        }
        check_dest(&dest, fmt)?;
        let partial = partial_path(&dest);
        match clear_stale_partial(&partial, &dest) {
            Ok(()) => {}
            // Another writer holds this number (or, without locks, a
            // leftover blocks it): the next one.
            Err(MediaError::Unsupported(_)) => continue,
            Err(e) => return Err(e),
        }
        let mut w = match PcmWriter::create(&partial, fmt, channels) {
            Ok(w) => w,
            // Something took the partial's name meanwhile: the next number.
            Err(_) if std::fs::symlink_metadata(&partial).is_ok() => continue,
            Err(e) => return Err(e),
        };
        // From here on the partial is ours: any way out but the rename
        // removes it, before its lock is released.
        let mut guard = PartialGuard(Some(partial.clone()));
        let lock = match lock_own_partial(&partial, &dest) {
            Ok(l) => l,
            Err(MediaError::Unsupported(_)) => {
                // Another writer has the name now: leave its file.
                guard.0 = None;
                continue;
            }
            Err(e) => return Err(e),
        };
        // `Some`: in place; `None`: the name appeared meanwhile.
        let done = copy_frames(&r, start, end, ch, &mut w, source)
            .and_then(|()| w.finish())
            .and_then(|e| match rename_excl(&partial, &dest) {
                Ok(()) => Ok(Some(e)),
                Err(x) if x.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
                Err(x) => Err(x.into()),
            });
        match done {
            Ok(Some(mut exported)) => {
                guard.0 = None;
                drop(lock);
                sync_parent(&dest);
                exported.path = dest;
                return Ok(exported);
            }
            // The partial goes first, then its lock (another writer must
            // never take a live partial for a stale one).
            Ok(None) => {
                drop(guard);
                drop(lock);
            }
            Err(e) => {
                drop(guard);
                drop(lock);
                return Err(e);
            }
        }
    }
    Err(MediaError::Io(format!("no free name for {stem} edit <n>.wav")))
}

/// Copies frames `[start, end)` of `r` into `w` (`ch` channels).
fn copy_frames(
    r: &CafReader,
    start: u64,
    end: u64,
    ch: usize,
    w: &mut PcmWriter,
    source: &Path,
) -> Result<(), MediaError> {
    let mut buf = vec![0f32; SPAN_CHUNK * ch];
    let mut scratch = Vec::new();
    let mut at = start;
    while at < end {
        let want = usize::try_from(end - at)
            .unwrap_or(SPAN_CHUNK)
            .min(SPAN_CHUNK);
        let got = r.read(at, buf.get_mut(..want * ch).unwrap_or(&mut []), &mut scratch)?;
        if got == 0 {
            return Err(MediaError::Io(format!(
                "{} ended before frame {end}",
                display_name(source)
            )));
        }
        w.write(buf.get(..got * ch).unwrap_or(&[]))?;
        at += got as u64;
    }
    Ok(())
}
```

Find in `crates/core-media/tests/export.rs`:
```rust
use core_media::export::{
    Container, ExportFormat, PcmWriter, check_destination, check_folder, export, partial_path,
    write_partial,
};
```
Replace:
```rust
use core_media::export::{
    Container, ExportFormat, PcmWriter, check_destination, check_folder, export, partial_path,
    write_partial, write_span,
};
```

Find in `crates/core-media/tests/export.rs`:
```rust
    assert!(!other.exists());
}
```
Replace:
```rust
    assert!(!other.exists());
}

/// A 48 kHz stereo CAF of `signal(frames)` in `depth`, without dither:
/// the media a region plays (plan 5f-3).
fn media_caf(dir: &Path, depth: BitDepth, frames: usize) -> PathBuf {
    let p = dir.join(format!("media-{}.caf", depth.bits()));
    let mut w = CafWriter::create(&p, 48_000, 2, depth, WriteMode::Normal).unwrap();
    w.set_dither(false);
    w.write(&signal(frames)).unwrap();
    w.finish().unwrap();
    p
}

/// Plan 5f-3 Task 2 (pre-flight I4, I5): an edit copy is the span's own
/// samples, bit for bit, in every depth; refusals write nothing.
#[test]
fn a_span_is_written_exactly() {
    let d = tempfile::tempdir().unwrap();
    for depth in [BitDepth::Int16, BitDepth::Int24, BitDepth::Float32] {
        let src = media_caf(d.path(), depth, 1000);
        let out = d.path().join(format!("out-{}", depth.bits()));
        std::fs::create_dir(&out).unwrap();
        let f = fmt(Container::Wav, depth, false);
        let e = write_span(&src, 300, 500, &out, "Kick", &f).unwrap();
        assert_eq!(
            (e.path.clone(), e.frames, e.channels),
            (out.join("Kick edit 1.wav"), 500, 2)
        );
        let (rate, ch, bits, float, frames, samples) = read_wav(&e.path);
        assert_eq!(
            (rate, ch, bits, float, frames),
            (
                48_000,
                2,
                depth.bits() as u16,
                depth == BitDepth::Float32,
                500
            ),
            "{depth:?}"
        );
        let r = CafReader::open(&src).unwrap();
        let mut want = vec![0f32; 1000];
        assert_eq!(r.read(300, &mut want, &mut Vec::new()).unwrap(), 500);
        let want: Vec<i64> = want
            .iter()
            .map(|x| match depth {
                BitDepth::Int16 => (x * 32_768.0).round() as i64,
                BitDepth::Int24 => (x * 8_388_608.0).round() as i64,
                BitDepth::Float32 => i64::from(x.to_bits()),
            })
            .collect();
        assert_eq!(samples, want, "{depth:?}: the media's own samples");
        assert_eq!(names(&out), ["Kick edit 1.wav"], "no partial is left");
    }
    // Refused before anything is written.
    let src = d.path().join("media-24.caf");
    let out = d.path().join("refused");
    std::fs::create_dir(&out).unwrap();
    let f = fmt(Container::Wav, BitDepth::Int24, false);
    for (start, frames) in [(0, 0), (900, 101), (u64::MAX, 2)] {
        assert!(
            write_span(&src, start, frames, &out, "Kick", &f).is_err(),
            "{start} {frames}"
        );
    }
    for bad in [
        fmt(Container::Wav, BitDepth::Int24, true),
        fmt(Container::Aiff, BitDepth::Int24, false),
        ExportFormat { rate: 44_100, ..f },
    ] {
        assert!(write_span(&src, 0, 10, &out, "Kick", &bad).is_err(), "{bad:?}");
    }
    assert!(write_span(&src, 0, 10, &d.path().join("missing"), "Kick", &f).is_err());
    assert!(write_span(&d.path().join("nope.caf"), 0, 10, &out, "Kick", &f).is_err());
    assert!(names(&out).is_empty(), "nothing was written");
}

/// Plan 5f-3 Task 2 (pre-flight I5, M3): copies take the first free
/// number and never write over a file; a crash's partial is cleared and
/// its number used; a partial another writer holds keeps its number.
#[test]
fn spans_take_the_first_free_number_and_clear_a_stale_partial() {
    let d = tempfile::tempdir().unwrap();
    let src = media_caf(d.path(), BitDepth::Int24, 100);
    let out = d.path().join("Edits");
    std::fs::create_dir(&out).unwrap();
    let f = fmt(Container::Wav, BitDepth::Int24, false);
    let span = || write_span(&src, 0, 100, &out, "Kick", &f).unwrap().path;
    assert_eq!(span(), out.join("Kick edit 1.wav"));
    // A copy from an earlier edit is never written over.
    std::fs::write(out.join("Kick edit 2.wav"), b"the user's").unwrap();
    // A partial a crash left (nobody holds its lock) is cleared and used.
    std::fs::write(partial_path(&out.join("Kick edit 3.wav")), b"left by a crash").unwrap();
    assert_eq!(span(), out.join("Kick edit 3.wav"));
    assert_eq!(read_wav(&out.join("Kick edit 3.wav")).4, 100);
    assert_eq!(std::fs::read(out.join("Kick edit 2.wav")).unwrap(), b"the user's");
    assert_eq!(
        names(&out),
        ["Kick edit 1.wav", "Kick edit 2.wav", "Kick edit 3.wav"]
    );
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let held = std::fs::File::create(partial_path(&out.join("Kick edit 4.wav"))).unwrap();
        // SAFETY: `held` owns a valid open descriptor for the call.
        assert_eq!(
            unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert_eq!(
            span(),
            out.join("Kick edit 5.wav"),
            "another writer's number is skipped"
        );
        assert!(partial_path(&out.join("Kick edit 4.wav")).is_file(), "and its file left");
        drop(held);
    }
}
```

- [ ] **Step 1: Write the failing tests** (the two test excerpts above).
- [ ] **Step 2: Run them to see them fail.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-media --test export
```
Expected: FAIL to compile — no `write_span`.

- [ ] **Step 3: Implement** the `export.rs` excerpt.
- [ ] **Step 4: Run the tests and the gates.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo test -p core-media
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings
```
Expected: PASS; Rust +2. Both tests run on Linux too.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-media/src/export.rs crates/core-media/tests/export.rs
git commit -m "core-media: write_span copies a CAF span to a numbered WAV, exactly and write-once (plan 5f-3 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -4
```
Counts: Rust +2 (1877 / 16 ignored on macOS).

---

### Task 3: The edit copy, the copies kept until close, the External Edit job

**Files:**
- Create: `crates/core-ffi/src/external.rs`, `crates/core-ffi/tests/external.rs`
- Modify: `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/session.rs` (`Inner.edits`, `Inner.edits_writing`; `tidy`; `media_jobs_idle`; `save_as`; `close`; `discard`; `apply_jobs`; the three exports; `apply_external_edit_work`), `crates/core-ffi/src/jobs.rs`, `app/Sources/VizDAWAppModel/ProjectMedia.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`

**Interfaces:**
- Consumes: `core_media::export::{write_span, ExportFormat, Container}` (Task 2), `core_media::import::import_reserved`, `AudioCmd::SetSource` (Task 1), `MediaCmd::Add`, `crate::retention::{trash, trash_dir}`, `crate::relink::regular_file`, `Inner::{root, kept, reserved_names, refresh_media, media_jobs_idle}`, `remove_unkept`.
- Produces:
  ```rust
  // external.rs
  pub const EDITS_DIR: &str = "Media/Edits";
  pub const MAX_STEM_BYTES: usize = 200;
  pub const MAX_PATH_BYTES: usize = 4096;
  pub const OFFLINE: &str; pub const NO_EDIT: &str; pub const GONE: &str; pub const REGION_DELETED: &str;
  pub struct ExternalEditRow { pub event: u64, pub path: String, pub from: u64 }           // uniffi::Record
  pub enum ExternalEditStateRow { Copy { row: ExternalEditRow }, Gone, NoEdit }            // uniffi::Enum
  pub fn safe_stem(name: &str) -> String;
  pub fn copy_name(name: &str, n: u32) -> String;                                          // "<safe stem> edit <n>.wav"
  pub(crate) struct EditCopy { pub(crate) abs: PathBuf, pub(crate) from: MediaId }
  pub(crate) fn region_span(doc: &Document, event: EventId) -> Result<(MediaId, u64, u64), VizError>;
  pub(crate) fn package_relative(abs: &Path, package: &Path) -> Option<String>;
  pub(crate) fn write_copy(source: &Path, start: u64, frames: u64, dir: &Path, stem: &str, fmt: &ExportFormat) -> Result<PathBuf, VizError>;
  pub(crate) fn trash_files_in(dir: &Path, trash: Option<&Path>) -> usize;
  // Session (exported)
  pub fn open_external(&self, event: u64) -> Result<ExternalEditRow, VizError>;
  pub fn external_edit(&self, event: u64) -> Result<ExternalEditStateRow, VizError>;
  pub fn apply_external_edit(&self, event: u64, path: String) -> Result<u64, VizError>;
  // jobs.rs
  JobKindRow::ExternalEdit
  Work::ExternalEdit { event: EventId, from: MediaId, file: Result<Vec<Imported>, String> }
  ```
  Swift sees `ExternalEditRow(event:path:from:)`, `ExternalEditStateRow.copy(row:)/.gone/.noEdit`, `JobKindRow.externalEdit`, and `openExternal(event:)`, `externalEdit(event:)`, `applyExternalEdit(event:path:)`.

Create `crates/core-ffi/src/external.rs`:

```rust
//! The external sample editor's copies (sub-project 5 spec §11; plan 5f-3
//! Rulings 2, 3): their names, rows and runtime record, and the file work
//! the session does for them. The session keeps the live copies
//! (`Inner.edits`) and runs the copy and the import (`session.rs`).

use crate::error::VizError;
use core_media::export::ExportFormat;
use core_model::document::Document;
use core_model::event::EventBody;
use core_model::ids::{EventId, MediaId};
use std::path::{Component, Path, PathBuf};

/// Where edit copies go, package-relative.
pub const EDITS_DIR: &str = "Media/Edits";
/// The longest stem, in UTF-8 bytes, so `.<stem> edit 9999.wav.partial`
/// stays within 255 bytes (pre-flight M3).
pub const MAX_STEM_BYTES: usize = 200;
/// The longest path `apply_external_edit` takes, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// Open on a region whose media is offline (Ruling 2).
pub const OFFLINE: &str = "The region's audio is offline; relink it first.";
/// Reload or apply on a region without a copy (pre-flight I10).
pub const NO_EDIT: &str = "This region has no external edit; use Open in External Editor.";
/// A copy that is not on disk, or not in this session (Ruling 3).
pub const GONE: &str = "The edit copy is gone; Open in External Editor again.";
/// An edit of a region that is gone (Ruling 6).
pub const REGION_DELETED: &str = "The region was deleted; the edit was not applied.";

/// A region's copy in this session (Ruling 3; runtime only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditCopy {
    /// The copy, absolute (pre-flight I1): Save As leaves it where it is.
    pub(crate) abs: PathBuf,
    /// The media item the edit started from.
    pub(crate) from: MediaId,
}

/// A region's edit copy as the app sees it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ExternalEditRow {
    /// The region.
    pub event: u64,
    /// The copy's absolute path.
    pub path: String,
    /// The media item the edit started from.
    pub from: u64,
}

/// What Reload finds for a region (Ruling 7; pre-flight I10).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ExternalEditStateRow {
    /// Its copy in this session, on disk.
    Copy {
        /// The copy.
        row: ExternalEditRow,
    },
    /// Its copy is gone: not on disk, or not in this session while its
    /// media came from an external edit (the project was reopened).
    Gone,
    /// It has no external edit.
    NoEdit,
}

/// `name` as a file stem (Review Focus 4; pre-flight M3): `/`, `\`, `:`
/// and control characters become `-`, spaces at the ends go, a leading
/// `.` becomes `-` (a dot-name is hidden, and retention never sees it),
/// the result is cut to [`MAX_STEM_BYTES`] at a character boundary, and an
/// empty stem is "Audio".
pub fn safe_stem(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut s = mapped.trim().to_string();
    if s.starts_with('.') {
        s.replace_range(..1, "-");
    }
    let mut cut = s.len().min(MAX_STEM_BYTES);
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    let s = s.trim_end();
    if s.is_empty() {
        "Audio".to_string()
    } else {
        s.to_string()
    }
}

/// The copy's file name for media `name` and number `n`.
pub fn copy_name(name: &str, n: u32) -> String {
    format!("{} edit {n}.wav", safe_stem(name))
}

/// Region `event`'s media and span; `Model` for an unknown event or one
/// that is no audio region.
pub(crate) fn region_span(doc: &Document, event: EventId) -> Result<(MediaId, u64, u64), VizError> {
    let (ev, _) = doc.event(event)?;
    match &ev.body {
        EventBody::AudioRegion {
            media,
            source_offset,
            length,
            ..
        } => Ok((*media, *source_offset, *length)),
        _ => Err(VizError::Model {
            message: format!("{event} is not an audio region; select one audio region."),
        }),
    }
}

/// `abs` as a package-relative, `/`-joined path when it lies under
/// `package` (retention compares those; pre-flight I1).
pub(crate) fn package_relative(abs: &Path, package: &Path) -> Option<String> {
    let rel = abs.strip_prefix(package).ok()?;
    let parts: Option<Vec<&str>> = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    Some(parts?.join("/"))
}

/// Writes the copy (Ruling 2): `dir` (the package's `Media/Edits`) is made
/// when missing and refused when something else is there, then
/// [`core_media::export::write_span`]. The copy's absolute path.
pub(crate) fn write_copy(
    source: &Path,
    start: u64,
    frames: u64,
    dir: &Path,
    stem: &str,
    fmt: &ExportFormat,
) -> Result<PathBuf, VizError> {
    if std::fs::symlink_metadata(dir).is_ok_and(|m| !m.is_dir()) {
        return Err(VizError::Io {
            message: format!("{EDITS_DIR} is a file, not a folder; move it away and try again."),
        });
    }
    std::fs::create_dir_all(dir).map_err(|e| VizError::Io {
        message: format!("{EDITS_DIR}: {e}"),
    })?;
    core_media::export::write_span(source, start, frames, dir, stem, fmt)
        .map(|e| e.path)
        .map_err(|e| VizError::Io {
            message: e.to_string(),
        })
}

/// Moves every regular file in `dir` to the Trash (`trash`: the folder
/// standing for it, as [`crate::retention::trash_dir`] resolved it, else
/// the platform's); the count left behind (`dir` unreadable, or a file
/// not moved), each named on stderr. A missing `dir` leaves none. Never
/// unlinks (plan 5c Ruling 15; plan 5f-3 owner answer 2026-11-03).
pub(crate) fn trash_files_in(dir: &Path, trash: Option<&Path>) -> usize {
    let rd = match std::fs::read_dir(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
        // Unreadable: files may be in it (5c review minor b).
        Err(e) => {
            eprintln!("VizDAW: {}: not read ({e})", dir.display());
            return 1;
        }
        Ok(rd) => rd,
    };
    let mut left = 0;
    for e in rd.flatten() {
        if e.path().is_file() {
            if let Err(why) = crate::retention::trash(trash, &e.path()) {
                eprintln!(
                    "VizDAW: {}: not moved to the Trash ({why})",
                    e.path().display()
                );
                left += 1;
            }
        }
    }
    left
}
```

Find in `crates/core-ffi/src/lib.rs`:
```rust
pub mod events;
```
Replace:
```rust
pub mod events;
pub mod external;
```

Find in `crates/core-ffi/src/lib.rs`:
```rust
pub use editor::EditorSize;
```
Replace:
```rust
pub use editor::EditorSize;
pub use external::{ExternalEditRow, ExternalEditStateRow};
```

Find in `crates/core-ffi/src/jobs.rs`:
```rust
    /// Bounce (plan 5f-2).
    Bounce,
}
```
Replace:
```rust
    /// Bounce (plan 5f-2).
    Bounce,
    /// Import an external edit (plan 5f-3).
    ExternalEdit,
}
```
This is the text plan 5f-2 left.

Find in `crates/core-ffi/src/jobs.rs`:
```rust
    /// Cancelled before it finished (whatever it wrote is removed).
    Cancelled,
```
Replace:
```rust
    /// An external edit's import (plan 5f-3 Ruling 6).
    ExternalEdit {
        /// The region it relinks.
        event: core_model::ids::EventId,
        /// The item the edit started from (`ExternalEdit { from }`).
        from: core_model::ids::MediaId,
        /// What the copy became (more than one item: a file wider than
        /// stereo), or why not.
        file: Result<Vec<Imported>, String>,
    },
    /// Cancelled before it finished (whatever it wrote is removed).
    Cancelled,
```

In `crates/core-ffi/src/session.rs`, `Inner` gains the copies:

Find in `crates/core-ffi/src/session.rs`:
```rust
    /// Finished bounces' files and media, by job (Ruling 10).
    bounce_results: std::collections::BTreeMap<u64, crate::bounce::BounceResultRow>,
```
Replace:
```rust
    /// Finished bounces' files and media, by job (Ruling 10).
    bounce_results: std::collections::BTreeMap<u64, crate::bounce::BounceResultRow>,
    /// Each region's external-edit copy while the project is open (plan
    /// 5f-3 Ruling 3; runtime only): retention keeps them, close lets them go.
    edits: std::collections::BTreeMap<core_model::ids::EventId, crate::external::EditCopy>,
    /// Copies being written without the lock (pre-flight M4): retention
    /// waits for them.
    edits_writing: u32,
```

Find in `crates/core-ffi/src/session.rs`:
```rust
            bounce_results: Default::default(),
```
Replace:
```rust
            bounce_results: Default::default(),
            edits: Default::default(),
            edits_writing: 0,
```

`tidy` keeps the copies under this package, by package-relative path (pre-flight I1):

Find in `crates/core-ffi/src/session.rs`:
```rust
        if !jobs_done && self.jobs.busy() {
            return;
        }
        let dir = crate::retention::trash_dir(self.trash_dir.as_deref());
        let mut refs = self.history.referenced_media_anywhere();
        for p in self.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
```
Replace:
```rust
        if !jobs_done && (self.jobs.busy() || self.edits_writing > 0) {
            return;
        }
        let dir = crate::retention::trash_dir(self.trash_dir.as_deref());
        let mut refs = self.history.referenced_media_anywhere();
        for p in self.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
        // Edit copies while the project is open (plan 5f-3 Ruling 3):
        // `abs` is absolute, retention's paths package-relative; a copy in
        // another package (after Save As) is not this package's.
        for e in self.edits.values() {
            if let Some(rel) = crate::external::package_relative(&e.abs, package) {
                refs.entry(rel).or_insert(None);
            }
        }
```

`media_jobs_idle` gains the External Edit job (pre-flight I11):

Find in `crates/core-ffi/src/session.rs`:
```rust
    /// `Busy` while a Format, Convert, Relink or Bounce job has not been
    /// applied: each captures the project format and the items' files when
    /// it starts, so another such job may not start meanwhile (5a Task 11
    /// review Minor 4; preflight M3; a bounce's streamer reads the media,
    /// plan 5f-2 Ruling 9). An import does not block them.
    fn media_jobs_idle(&self) -> Result<(), VizError> {
        use crate::jobs::JobKindRow;
        if self.jobs.pending(&[
            JobKindRow::Format,
            JobKindRow::Convert,
            JobKindRow::Relink,
            JobKindRow::Bounce,
        ]) {
```
Replace:
```rust
    /// `Busy` while a Format, Convert, Relink, Bounce or External Edit job
    /// has not been applied: each captures the project format and the
    /// items' files when it starts, so another such job may not start
    /// meanwhile (5a Task 11 review Minor 4; preflight M3; a bounce's
    /// streamer reads the media, plan 5f-2 Ruling 9; an external edit
    /// imports in the format it started with and relinks a region, plan
    /// 5f-3 Ruling 6). An import does not block them.
    fn media_jobs_idle(&self) -> Result<(), VizError> {
        use crate::jobs::JobKindRow;
        if self.jobs.pending(&[
            JobKindRow::Format,
            JobKindRow::Convert,
            JobKindRow::Relink,
            JobKindRow::Bounce,
            JobKindRow::ExternalEdit,
        ]) {
```

An untitled project's Save As sends its copies to the Trash before the scratch package goes (owner answer 2026-11-03; pre-flight I2):

Find in `crates/core-ffi/src/session.rs`:
```rust
                // The scratch package of an untitled session is done with.
                let _ = std::fs::remove_dir_all(crate::media::scratch_root(self.id));
```
Replace:
```rust
                // The scratch package of an untitled session is done with.
                // Its edit copies go to the Trash first, never unlinked
                // (plan 5f-3 Ruling 3, owner answer 2026-11-03): Save As
                // does not carry them, so their next apply says they are
                // gone. A titled session has no scratch package.
                let scratch = crate::media::scratch_root(self.id);
                let trash = crate::retention::trash_dir(inner.trash_dir.as_deref());
                let left = crate::external::trash_files_in(
                    &scratch.join(crate::external::EDITS_DIR),
                    trash.as_deref(),
                );
                if left == 0 {
                    let _ = std::fs::remove_dir_all(&scratch);
                } else {
                    inner.news.problems.push(format!(
                        "{left} edit copies were left in {}.",
                        scratch.display()
                    ));
                }
```

A saved project's close lets its copies go:

Find in `crates/core-ffi/src/session.rs`:
```rust
                before_close_tidy();
                inner.tidy(&p, true);
```
Replace:
```rust
                before_close_tidy();
                // Watching ends with the project: its copies may go (plan
                // 5f-3 Ruling 3).
                inner.edits.clear();
                inner.tidy(&p, true);
```

An untitled project's close trashes `Media/Edits` with `Media/Recorded`:

Find in `crates/core-ffi/src/session.rs`:
```rust
                let mut left = 0usize;
                let recorded = scratch.join(crate::record::RECORDED_DIR);
                match std::fs::read_dir(&recorded) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    // Unreadable: recordings may be in it (review minor b).
                    Err(e) => {
                        eprintln!("VizDAW: {}: not read ({e})", recorded.display());
                        left += 1;
                    }
                    Ok(rd) => {
                        for e in rd.flatten() {
                            if e.path().is_file() {
                                if let Err(why) = crate::retention::trash(dir.as_deref(), &e.path())
                                {
                                    eprintln!(
                                        "VizDAW: {}: not moved to the Trash ({why})",
                                        e.path().display()
                                    );
                                    left += 1;
                                }
                            }
                        }
                    }
                }
                if left == 0 {
                    let _ = std::fs::remove_dir_all(scratch);
                } else {
                    // Never unlinked (pre-flight I2): the package stays.
                    eprintln!(
                        "VizDAW: {left} recording(s) were left in {}",
```
Replace:
```rust
                // Edit copies too (plan 5f-3 Ruling 3, owner answer
                // 2026-11-03): an editor may hold work VizDAW has not
                // imported yet.
                let mut left = 0usize;
                for d in [crate::record::RECORDED_DIR, crate::external::EDITS_DIR] {
                    left += crate::external::trash_files_in(&scratch.join(d), dir.as_deref());
                }
                if left == 0 {
                    let _ = std::fs::remove_dir_all(scratch);
                } else {
                    // Never unlinked (pre-flight I2): the package stays.
                    eprintln!(
                        "VizDAW: {left} recording(s) or edit copies were left in {}",
```

`discard` removes what an unapplied edit imported (its copy stays):

Find in `crates/core-ffi/src/session.rs`:
```rust
        Work::Cancelled => Vec::new(),
    };
```
Replace:
```rust
        // What an external edit imported; its copy stays (plan 5f-3).
        Work::ExternalEdit { file, .. } => file.into_iter().flatten().map(|i| i.path).collect(),
        Work::Cancelled => Vec::new(),
    };
```

`apply_jobs` applies it with its own apply (Ruling 6):

Find in `crates/core-ffi/src/session.rs`:
```rust
                Work::Import { files, target } => {
                    match apply_import_as(inner, &root, files, target, &ImportAs::IMPORT) {
```
Replace:
```rust
                Work::ExternalEdit { event, from, file } => {
                    let r = apply_external_edit_work(inner, &root, id, event, from, file);
                    changed |= r.state == JobStateRow::Done;
                    r
                }
                Work::Import { files, target } => {
                    match apply_import_as(inner, &root, files, target, &ImportAs::IMPORT) {
```

The exports and the apply, after the bounce exports:

Find in `crates/core-ffi/src/session.rs`:
```rust
            Ok(inner.bounce_results.get(&job).cloned())
        })
    }
}
```
Replace:
```rust
            Ok(inner.bounce_results.get(&job).cloned())
        })
    }
}

/// The external sample editor (plan 5f-3; spec §11).
#[uniffi::export]
impl Session {
    /// Open in External Editor (Ruling 2): writes region `event`'s span
    /// (no gain, no fades, no dither) as a WAV in the project's rate and
    /// depth to `Media/Edits/<name> edit <n>.wav` and keeps it as the
    /// region's copy while the project is open (Ruling 3). `Model` for an
    /// unknown event, a non-region or an offline region; `Busy` while a
    /// Format, Convert or Relink job is unapplied; `Io` when `Media/Edits`
    /// is a file or the copy cannot be written. The copy is written
    /// without the session lock (pre-flight M4); a region that changed
    /// meanwhile is refused and the file left to retention.
    pub fn open_external(&self, event: u64) -> Result<crate::external::ExternalEditRow, VizError> {
        use crate::jobs::JobKindRow;
        ffi_guard(|| {
            let ev = core_model::ids::EventId(event);
            let (source, start, frames, dir, stem, fmt, from) = {
                let mut inner = lock(&self.inner);
                if inner.closed {
                    return Err(crate::events::bad("this project was closed"));
                }
                if inner.jobs.pending(&[
                    JobKindRow::Format,
                    JobKindRow::Convert,
                    JobKindRow::Relink,
                ]) {
                    return Err(VizError::Busy {
                        message: "Wait for the media job to finish.".into(),
                    });
                }
                inner.refresh_media(self.id, false);
                let (media, start, frames) = crate::external::region_span(inner.doc(), ev)?;
                if inner.offline.contains_key(&media) {
                    return Err(crate::events::bad(crate::external::OFFLINE));
                }
                let format = inner.doc().settings().format;
                let root = inner.root(self.id);
                let item = inner
                    .doc()
                    .media()
                    .get(&media)
                    .ok_or_else(|| crate::events::bad(crate::external::OFFLINE))?;
                let out = (
                    root.join(&item.path),
                    start,
                    frames,
                    root.join(crate::external::EDITS_DIR),
                    crate::external::safe_stem(&item.name),
                    core_media::export::ExportFormat {
                        container: core_media::export::Container::Wav,
                        depth: format.depth,
                        rate: format.sample_rate,
                        dither: false,
                        dither_stream: 0,
                    },
                    media,
                );
                inner.edits_writing += 1;
                out
            };
            let written = crate::external::write_copy(&source, start, frames, &dir, &stem, &fmt);
            let mut inner = lock(&self.inner);
            inner.edits_writing = inner.edits_writing.saturating_sub(1);
            let written = written?;
            match crate::external::region_span(inner.doc(), ev) {
                Ok((m, ..)) if m == from && !inner.closed => {}
                _ => {
                    return Err(crate::events::bad(
                        "The region changed while its copy was written; try again.",
                    ));
                }
            }
            inner.edits.insert(
                ev,
                crate::external::EditCopy {
                    abs: written.clone(),
                    from,
                },
            );
            Ok(crate::external::ExternalEditRow {
                event,
                path: written.display().to_string(),
                from: from.0,
            })
        })
    }

    /// What Reload finds for region `event` (Ruling 7; pre-flight I10):
    /// its copy in this session; `Gone` when that copy is not on disk, or
    /// when there is none in this session and the region's media came from
    /// an external edit (the project was reopened); `NoEdit` otherwise.
    /// `Model` for an unknown event or a non-region.
    pub fn external_edit(
        &self,
        event: u64,
    ) -> Result<crate::external::ExternalEditStateRow, VizError> {
        use crate::external::{ExternalEditRow, ExternalEditStateRow};
        ffi_guard(|| {
            let ev = core_model::ids::EventId(event);
            let inner = lock(&self.inner);
            let (media, ..) = crate::external::region_span(inner.doc(), ev)?;
            if let Some(c) = inner.edits.get(&ev) {
                return Ok(
                    if std::fs::symlink_metadata(&c.abs).is_ok_and(|m| m.is_file()) {
                        ExternalEditStateRow::Copy {
                            row: ExternalEditRow {
                                event,
                                path: c.abs.display().to_string(),
                                from: c.from.0,
                            },
                        }
                    } else {
                        ExternalEditStateRow::Gone
                    },
                );
            }
            let edited = inner.doc().media().get(&media).is_some_and(|m| {
                matches!(
                    m.origin,
                    core_model::document::MediaOrigin::ExternalEdit { .. }
                )
            });
            Ok(if edited {
                ExternalEditStateRow::Gone
            } else {
                ExternalEditStateRow::NoEdit
            })
        })
    }

    /// Imports `path` — region `event`'s copy, nothing else — as an
    /// External Edit job (Ruling 6): the job ID; `job` reports it and
    /// applies it as one undo step "External Edit". `Model` for a path
    /// that is not the region's copy (relative, over
    /// [`crate::external::MAX_PATH_BYTES`], another file or spelling), a
    /// region without a copy, a region that is gone (its copy is then
    /// forgotten), or a copy that is gone; `Busy` inside an open edit and
    /// while a Format, Convert, Relink, Bounce or External Edit job is
    /// unapplied (pre-flight I11).
    pub fn apply_external_edit(&self, event: u64, path: String) -> Result<u64, VizError> {
        use crate::external::{GONE, MAX_PATH_BYTES, NO_EDIT, REGION_DELETED};
        ffi_guard(|| {
            if path.len() > MAX_PATH_BYTES {
                return Err(crate::events::bad("the path is too long"));
            }
            let p = PathBuf::from(&path);
            if !p.is_absolute() {
                return Err(VizError::Model {
                    message: format!("{path} is not an absolute path"),
                });
            }
            let ev = core_model::ids::EventId(event);
            let mut inner = lock(&self.inner);
            if inner.history.in_transaction() {
                return Err(VizError::Busy {
                    message: "Finish the open edit first.".into(),
                });
            }
            self.apply_jobs_after_edit(&mut inner);
            let Some(copy) = inner.edits.get(&ev).cloned() else {
                return Err(crate::events::bad(NO_EDIT));
            };
            if copy.abs != p {
                return Err(VizError::Model {
                    message: format!("{path} is not this region's edit copy"),
                });
            }
            if crate::external::region_span(inner.doc(), ev).is_err() {
                inner.edits.remove(&ev);
                return Err(crate::events::bad(REGION_DELETED));
            }
            if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
                return Err(crate::events::bad(GONE));
            }
            inner.media_jobs_idle()?;
            let root = inner.root(self.id);
            let format = inner.doc().settings().format;
            let reserved = inner.reserved_names();
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let from = copy.from;
            inner.jobs.spawn(
                crate::jobs::JobKindRow::ExternalEdit,
                move |progress, cancel| {
                    let importer = crate::media::importer();
                    let file = crate::relink::regular_file(&p).and_then(|()| {
                        core_media::import::import_reserved(
                            importer.as_ref(),
                            &p,
                            &root,
                            format,
                            &stem,
                            &reserved,
                            &mut |f| {
                                progress.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed)
                            },
                            cancel,
                        )
                    });
                    match file {
                        Err(core_media::MediaError::Cancelled) => crate::jobs::Work::Cancelled,
                        file => crate::jobs::Work::ExternalEdit {
                            event: ev,
                            from,
                            file: file.map_err(|e| e.to_string()),
                        },
                    }
                },
            )
        })
    }
}

/// Applies a finished external edit (Ruling 6; pre-flight I3): one undo
/// step "External Edit" adding the new item (origin `ExternalEdit { from }`)
/// and pointing that region alone at it (offset 0, its length, fades
/// clamped). Failed — with what it imported removed — when the import
/// failed, when it made more than one item (a file wider than stereo),
/// when the region is gone (its copy is then forgotten, so the app stops
/// watching it; pre-flight M10), or when the model refuses.
fn apply_external_edit_work(
    inner: &mut Inner,
    root: &Path,
    id: u64,
    event: core_model::ids::EventId,
    from: core_model::ids::MediaId,
    file: Result<Vec<core_media::import::Imported>, String>,
) -> crate::jobs::JobRow {
    use crate::jobs::{JobKindRow, JobStateRow, row};
    use core_model::command::{AudioCmd, MediaCmd};
    let failed = |m: String| {
        row(
            id,
            JobKindRow::ExternalEdit,
            JobStateRow::Failed,
            Some(m),
            vec![],
            vec![],
        )
    };
    let items = match file {
        Ok(v) => v,
        Err(why) => return failed(why),
    };
    let keep = inner.kept();
    let remove = |items: &[core_media::import::Imported]| {
        for i in items {
            remove_unkept(root, &i.path, &keep);
        }
    };
    let item = match items.as_slice() {
        [one] => one.clone(),
        _ => {
            remove(&items);
            return failed(format!(
                "The edit has {} channels; regions hold one or two.",
                items.len()
            ));
        }
    };
    if crate::external::region_span(inner.doc(), event).is_err() {
        remove(&items);
        inner.edits.remove(&event);
        return failed(crate::external::REGION_DELETED.into());
    }
    let filter = ViewFilter::default();
    let h = &mut inner.history;
    if let Err(e) = h.begin("External Edit") {
        remove(&items);
        return failed(VizError::from(e).to_string());
    }
    let run = (|| -> Result<u64, VizError> {
        let r = h.execute(
            &Command::Media(MediaCmd::Add {
                path: item.path.clone(),
                sample_rate: item.sample_rate,
                channels: item.channels,
                length_frames: item.length_frames,
                content_hash: Some(item.content_hash.clone()),
                name: Some(item.name.clone()),
                origin: Some(core_model::document::MediaOrigin::ExternalEdit { from }),
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
        h.execute(
            &Command::Audio(AudioCmd::SetSource {
                event,
                media,
                source_offset: 0,
                length: item.length_frames,
            }),
            &filter,
        )?;
        Ok(media.0)
    })();
    let done = run.and_then(|media| h.commit().map(|()| media).map_err(VizError::from));
    match done {
        Ok(media) => row(
            id,
            JobKindRow::ExternalEdit,
            JobStateRow::Done,
            None,
            vec![],
            vec![media],
        ),
        Err(e) => {
            if h.in_transaction() {
                let _ = h.abort();
            }
            remove(&items);
            failed(e.to_string())
        }
    }
}
```

Swift: in `app/Sources/VizDAWAppModel/ProjectMedia.swift`,

Find in `app/Sources/VizDAWAppModel/ProjectMedia.swift`:
```swift
        case .bounce: "Bouncing…"
```
Replace:
```swift
        case .bounce: "Bouncing…"
        case .externalEdit: "Importing the external edit…"
```

Find in `app/Sources/VizDAWAppModel/ProjectMedia.swift`:
```swift
        case .bounce: "Bounce Failed"
```
Replace:
```swift
        case .bounce: "Bounce Failed"
        case .externalEdit: "External Edit Failed"
```

Find in `app/Tests/VizDAWAppModelTests/FakeSession.swift`:
```swift
    func bounceResult(job: UInt64) throws -> BounceResultRow? { resultRow }
```
Replace:
```swift
    func bounceResult(job: UInt64) throws -> BounceResultRow? { resultRow }

    // MARK: External sample editor (plan 5f-3)

    /// One `applyExternalEdit` call.
    struct ExternalApply: Equatable {
        let event: UInt64
        let path: String
    }
    /// What `openExternal` answers (nil: a row for the event under
    /// /tmp/p.vizdaw), and its refusal.
    var externalRow: ExternalEditRow?
    var externalError: VizError?
    /// The regions `openExternal` was called for (refused calls too).
    var externalOpened: [UInt64] = []
    /// What `externalEdit` answers per region: the copy `openExternal`
    /// made, else `.noEdit`.
    var externalStates: [UInt64: ExternalEditStateRow] = [:]
    /// `applyExternalEdit` calls, and its refusal.
    var externalApplies: [ExternalApply] = []
    var applyError: VizError?
    func openExternal(event: UInt64) throws -> ExternalEditRow {
        externalOpened.append(event)
        if let e = externalError { throw e }
        let row = externalRow ?? ExternalEditRow(event: event, path: "/tmp/p.vizdaw/Media/Edits/Kick edit 1.wav", from: 1)
        externalStates[event] = .copy(row: row)
        return row
    }
    func externalEdit(event: UInt64) throws -> ExternalEditStateRow { externalStates[event] ?? .noEdit }
    func applyExternalEdit(event: UInt64, path: String) throws -> UInt64 {
        if let e = applyError { throw e }
        externalApplies.append(ExternalApply(event: event, path: path))
        let id = UInt64(5000 + externalApplies.count)
        jobRows[id] = JobRow(
            id: id, kind: .externalEdit, progress: jobStartState == .running ? 0.4 : 1, state: jobStartState,
            message: nil, problems: [], created: [])
        return id
    }
```

- [ ] **Step 1: Write the failing tests.** Create `crates/core-ffi/tests/external.rs`:

```rust
//! The external sample editor in the core (plan 5f-3 Task 3; spec §11,
//! §6.5; Rulings 2, 3, 6; pre-flight C1, I1–I5, I10, I11, M3, M6–M8, M10;
//! owner answers 2026-11-03). Projects are built here, not from the demo
//! (pre-flight C1: the demo's media does not exist): a 48 kHz project in
//! the depth under test, saved in a temporary folder (or untitled) with a
//! private Trash, with a generated 48 kHz stereo CAF imported at tick 0
//! (the CAF importer reads it on Linux too), gain −6 dB, fades and a split
//! at 1 s, so two regions use the tone. One test at a time: the engine
//! host is process-wide.
//!
//! Applying an edit imports a WAV: those tests are macOS only.

mod common;

use core_engine::fixtures::Builder;
use core_media::caf::{CafReader, CafWriter, WriteMode};
use core_model::command::{Command, MediaCmd};
use core_model::document::MediaOrigin;
use core_model::event::Fade;
use core_model::format::BitDepth;
use core_model::ids::AnyId;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use vizdaw_ffi::external::copy_name;
use vizdaw_ffi::host::{force_offline, reset_host};
#[cfg(target_os = "macos")]
use vizdaw_ffi::{MediaOriginRow, MediaRow};
use vizdaw_ffi::{
    AudioEdit, BitDepthRow, EditCommand, EventEdit, EventKindRow, EventRow, ExternalEditRow,
    ExternalEditStateRow, FadeRow, FadeShapeRow, ImportTargetRow, JobRow, JobStateRow,
    ProjectFormatRow, SequenceEdit, Session, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

/// The tone: 2 s at 48 kHz.
const FRAMES: u64 = 96_000;
const GONE: &str = "The edit copy is gone; Open in External Editor again.";

fn value(f: u64) -> f32 {
    ((f % 1000) as f32 / 1000.0 - 0.5) * 0.8
}

struct Project {
    dir: tempfile::TempDir,
    /// The package (`None`: untitled).
    pkg: Option<PathBuf>,
    s: Arc<Session>,
    seq: u64,
    lane: u64,
    /// The tone's media item.
    media: u64,
    /// The split's first part (the imported region's ID).
    first: u64,
    /// Its second part (from 1 s, with the fade out): the region edited.
    second: u64,
}

impl Project {
    fn trash(&self) -> PathBuf {
        self.dir.path().join("Trash")
    }

    fn pkg(&self) -> PathBuf {
        self.pkg.clone().unwrap()
    }

    fn region(&self, id: u64) -> EventRow {
        region_rows(&self.s, self.seq, self.lane)
            .into_iter()
            .find(|r| r.id == id)
            .unwrap()
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

fn region_rows(s: &Session, seq: u64, lane: u64) -> Vec<EventRow> {
    s.lane_events(seq, lane)
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.kind == EventKindRow::AudioRegion)
        .collect()
}

fn fade(n: i64) -> FadeRow {
    FadeRow {
        length: n,
        shape: FadeShapeRow::Linear,
    }
}

/// A 48 kHz project in `depth`, saved as `P.vizdaw` (`saved`) or untitled,
/// with a private Trash: sequence "Song" whose lane plays `tone.caf`
/// (imported: 48 kHz stereo float, value(f) left, −value(f) right) from
/// tick 0 at −6 dB with a 4800-frame fade in and a 2400-frame fade out,
/// split at 1 s, so two regions use the tone (pre-flight C1).
fn build(depth: BitDepthRow, saved: bool) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_project(ProjectFormatRow {
        sample_rate: 48_000,
        depth,
    })
    .unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    let pkg = saved.then(|| dir.path().join("P.vizdaw"));
    if let Some(p) = &pkg {
        s.save_as(p.display().to_string()).unwrap();
    }
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::New {
                name: "Song".into(),
            },
        },
        None,
    )
    .unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let lane = s.lanes(seq).unwrap()[0].id;
    let src = dir.path().join("tone.caf");
    let mut w = CafWriter::create(&src, 48_000, 2, BitDepth::Float32, WriteMode::Normal).unwrap();
    w.write(
        &(0..FRAMES)
            .flat_map(|f| [value(f), -value(f)])
            .collect::<Vec<f32>>(),
    )
    .unwrap();
    w.finish().unwrap();
    let job = s
        .import_audio(
            vec![src.display().to_string()],
            Some(ImportTargetRow {
                seq,
                lane: Some(lane),
                tick: 0,
            }),
        )
        .unwrap();
    assert_eq!(wait_job(&s, job).state, JobStateRow::Done);
    let media = s.media().unwrap()[0].id;
    let first = region_rows(&s, seq, lane)[0].id;
    for edit in [
        AudioEdit::SetGain {
            event: first,
            gain_db: -6.0,
        },
        AudioEdit::SetFades {
            event: first,
            fade_in: fade(4800),
            fade_out: fade(2400),
        },
        AudioEdit::Split {
            event: first,
            at: s.ticks_at(seq, vec![1.0]).unwrap()[0],
        },
    ] {
        s.edit(EditCommand::Audio { edit }, None).unwrap();
    }
    let second = region_rows(&s, seq, lane)
        .into_iter()
        .find(|r| r.id != first)
        .unwrap()
        .id;
    let p = Project {
        dir,
        pkg,
        s,
        seq,
        lane,
        media,
        first,
        second,
    };
    let r = p.region(second);
    assert!(r.source_offset > 0 && r.media == Some(media), "{r:?}");
    p
}

/// The package's `Media/*.caf` names (pre-flight M8: `Media/Edits`,
/// `.overviews` and `.incoming` are left out).
fn cafs(root: &Path) -> BTreeSet<String> {
    std::fs::read_dir(root.join("Media"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".caf"))
        .collect()
}

/// `(rate, channels, bits, float, frames, samples)` of a WAV (5f-2's reader).
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
    (
        rate,
        ch,
        bits,
        tag == 3,
        (samples.len() / usize::from(ch)) as u64,
        samples,
    )
}

/// Frames `[start, start + n)` of media `id`'s file under `root`, as the
/// integers (or float bits) a WAV of `depth` holds.
fn media_samples(p: &Project, root: &Path, id: u64, start: u64, n: u64, depth: BitDepth) -> Vec<i64> {
    let row = p.s.media().unwrap().into_iter().find(|r| r.id == id).unwrap();
    let r = CafReader::open(&root.join(&row.path)).unwrap();
    let ch = usize::from(r.info().channels);
    let mut buf = vec![0f32; n as usize * ch];
    assert_eq!(r.read(start, &mut buf, &mut Vec::new()).unwrap() as u64, n);
    buf.iter()
        .map(|x| match depth {
            BitDepth::Int16 => (x * 32_768.0).round() as i64,
            BitDepth::Int24 => (x * 8_388_608.0).round() as i64,
            BitDepth::Float32 => i64::from(x.to_bits()),
        })
        .collect()
}

/// A 16-bit WAV of silence (`media_pool.rs`'s, any channel count;
/// pre-flight M7: `PcmWriter` refuses three channels).
#[cfg(target_os = "macos")]
fn wav_at(path: &Path, rate: u32, channels: u16, frames: u32) {
    let data = frames * u32::from(channels) * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 0);
    std::fs::write(path, b).unwrap();
}

/// The editor's save: the copy removed and written again (`wav_at`).
#[cfg(target_os = "macos")]
fn replace_with_wav(copy: &Path, channels: u16, rate: u32, frames: u32) {
    let _ = std::fs::remove_file(copy);
    wav_at(copy, rate, channels, frames);
}

#[cfg(target_os = "macos")]
fn media_row(s: &Session, id: u64) -> MediaRow {
    s.media().unwrap().into_iter().find(|m| m.id == id).unwrap()
}

/// Ruling 2 (pre-flight I4): the copy is the region's span — not its gain
/// or fades — as a WAV in the project's rate and depth, the media's own
/// samples bit for bit (no dither); a second Open takes the next number.
#[test]
fn the_copy_is_the_span_as_wav_in_the_project_format() {
    let _g = serial();
    for (row, depth) in [
        (BitDepthRow::Int16, BitDepth::Int16),
        (BitDepthRow::Int24, BitDepth::Int24),
        (BitDepthRow::Float32, BitDepth::Float32),
    ] {
        let p = build(row, true);
        let pkg = p.pkg();
        let r = p.region(p.second);
        assert!(
            r.gain_db == -6.0 && r.fade_out.length == 2400,
            "the region has gain and a fade: {r:?}"
        );
        let e = p.s.open_external(p.second).unwrap();
        assert_eq!((e.event, e.from), (p.second, p.media));
        assert_eq!(
            PathBuf::from(&e.path),
            pkg.join("Media/Edits/tone edit 1.wav")
        );
        let (rate, ch, bits, float, frames, samples) = read_wav(Path::new(&e.path));
        assert_eq!(
            (rate, ch, bits, float, frames),
            (
                48_000,
                2,
                depth.bits() as u16,
                depth == BitDepth::Float32,
                r.frames
            ),
            "{depth:?}"
        );
        assert_eq!(
            samples,
            media_samples(&p, &pkg, p.media, r.source_offset, r.frames, depth),
            "{depth:?}: the media's samples"
        );
        assert_eq!(
            PathBuf::from(p.s.open_external(p.second).unwrap().path),
            pkg.join("Media/Edits/tone edit 2.wav")
        );
        p.s.close().unwrap();
    }
}

/// Review Focus 4 (pre-flight M3): safe names, free numbers, the folder.
#[test]
fn edit_copies_get_safe_free_names() {
    assert_eq!(copy_name("a/b:c\u{7}", 3), "a-b-c- edit 3.wav");
    assert_eq!(copy_name(".hidden", 1), "-hidden edit 1.wav", "never hidden");
    assert_eq!(copy_name("", 1), "Audio edit 1.wav");
    assert_eq!(copy_name("   ", 1), "Audio edit 1.wav");
    assert_eq!(
        copy_name(&"a".repeat(300), 2),
        format!("{} edit 2.wav", "a".repeat(200))
    );
    assert_eq!(
        copy_name(&"é".repeat(150), 1),
        format!("{} edit 1.wav", "é".repeat(100)),
        "cut at a character boundary, within 200 bytes"
    );
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let pkg = p.pkg();
    assert!(!pkg.join("Media/Edits").exists());
    p.s.rename_media(p.media, "a/b:c".into()).unwrap();
    let first = p.s.open_external(p.second).unwrap().path;
    assert_eq!(
        PathBuf::from(&first),
        pkg.join("Media/Edits/a-b-c edit 1.wav"),
        "the folder is made, the name made safe"
    );
    // A copy from an earlier edit is never written over.
    std::fs::write(pkg.join("Media/Edits/a-b-c edit 2.wav"), b"an earlier edit").unwrap();
    assert_eq!(
        PathBuf::from(p.s.open_external(p.second).unwrap().path),
        pkg.join("Media/Edits/a-b-c edit 3.wav")
    );
    assert_eq!(
        std::fs::read(pkg.join("Media/Edits/a-b-c edit 2.wav")).unwrap(),
        b"an earlier edit"
    );
    p.s.close().unwrap();
    // A file where the folder goes: refused with a message, nothing written.
    let q = build(BitDepthRow::Int24, true);
    let qpkg = q.pkg();
    std::fs::write(qpkg.join("Media/Edits"), b"not a folder").unwrap();
    let before = cafs(&qpkg);
    let e = q.s.open_external(q.second).unwrap_err();
    assert!(
        matches!(&e, VizError::Io { message } if message.contains("Media/Edits")),
        "{e}"
    );
    assert_eq!(std::fs::read(qpkg.join("Media/Edits")).unwrap(), b"not a folder");
    assert_eq!(cafs(&qpkg), before);
    assert_eq!(
        q.s.external_edit(q.second).unwrap(),
        ExternalEditStateRow::NoEdit
    );
    q.s.close().unwrap();
}

/// Review Focus 5 (pre-flight I11): hostile and racing calls are refused,
/// and nothing is written. On Linux the held job ends Failed (the CAF
/// importer reads no WAV), which the checks allow.
#[test]
fn hostile_external_calls_are_refused() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let pkg = p.pkg();
    let model = |r: Result<u64, VizError>| matches!(r, Err(VizError::Model { .. }));
    // Open: a note, an unknown ID, an offline region.
    let mut note = EventRow::blank(0, 0, EventKindRow::Note);
    note.pitch = 60;
    note.velocity = 0x8000;
    note.duration = 960;
    p.s.edit(
        EditCommand::Event {
            edit: EventEdit::Insert {
                seq: p.seq,
                lane: p.lane,
                events: vec![note],
            },
        },
        None,
    )
    .unwrap();
    let note = p
        .s
        .lane_events(p.seq, p.lane)
        .unwrap()
        .events
        .into_iter()
        .find(|e| e.kind == EventKindRow::Note)
        .unwrap()
        .id;
    for ev in [note, 999_999] {
        assert!(
            matches!(p.s.open_external(ev), Err(VizError::Model { .. })),
            "{ev}"
        );
        assert!(p.s.external_edit(ev).is_err(), "{ev}");
    }
    let tone = pkg.join("Media/tone.caf");
    let away = p.dir.path().join("tone-away.caf");
    std::fs::rename(&tone, &away).unwrap();
    p.s.recheck_media().unwrap();
    let e = p.s.open_external(p.second).unwrap_err();
    assert_eq!(e.to_string(), "The region's audio is offline; relink it first.");
    std::fs::rename(&away, &tone).unwrap();
    p.s.recheck_media().unwrap();
    assert!(!pkg.join("Media/Edits").exists(), "nothing was written");
    // Apply: no copy yet; then every path that is not the copy.
    let copy = pkg.join("Media/Edits/tone edit 1.wav").display().to_string();
    let e = p.s.apply_external_edit(p.second, copy.clone()).unwrap_err();
    assert_eq!(
        e.to_string(),
        "This region has no external edit; use Open in External Editor."
    );
    assert_eq!(p.s.open_external(p.second).unwrap().path, copy);
    let before = cafs(&pkg);
    for bad in [
        tone.display().to_string(),
        "Media/Edits/tone edit 1.wav".to_string(),
        format!("/{}", "a".repeat(70_000)),
        format!("{}/../Edits/tone edit 1.wav", pkg.join("Media/Edits").display()),
    ] {
        assert!(
            model(p.s.apply_external_edit(p.second, bad.clone())),
            "{}",
            &bad[..bad.len().min(80)]
        );
    }
    assert!(
        model(p.s.apply_external_edit(p.first, copy.clone())),
        "another region's copy"
    );
    assert_eq!(cafs(&pkg), before, "nothing was imported");
    // The copy goes missing: gone.
    let moved = p.dir.path().join("away.wav");
    std::fs::rename(&copy, &moved).unwrap();
    assert_eq!(
        p.s.apply_external_edit(p.second, copy.clone())
            .unwrap_err()
            .to_string(),
        GONE
    );
    std::fs::rename(&moved, &copy).unwrap();
    // Races: one apply at a time, and none beside a Format job, both ways.
    let int16 = ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int16,
    };
    p.s.debug_hold_jobs(true).unwrap();
    let job = p.s.apply_external_edit(p.second, copy.clone()).unwrap();
    assert!(
        matches!(
            p.s.apply_external_edit(p.second, copy.clone()),
            Err(VizError::Busy { .. })
        ),
        "a second apply while one is unapplied"
    );
    assert!(
        matches!(p.s.set_project_format(int16), Err(VizError::Busy { .. })),
        "a Format job while an edit is unapplied"
    );
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, job);
    p.s.debug_hold_jobs(true).unwrap();
    let format = p.s.set_project_format(int16).unwrap();
    assert!(
        matches!(
            p.s.apply_external_edit(p.second, copy.clone()),
            Err(VizError::Busy { .. })
        ),
        "an apply while a Format job is unapplied"
    );
    assert!(
        matches!(p.s.open_external(p.second), Err(VizError::Busy { .. })),
        "an Open while a Format job is unapplied"
    );
    p.s.debug_hold_jobs(false).unwrap();
    wait_job(&p.s, format);
    p.s.close().unwrap();
}

/// Ruling 3 (pre-flight I1): each Save keeps the live copy; closing sends
/// it to the Trash, never unlinked.
#[test]
fn copies_are_kept_until_close_then_trashed() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let copy = PathBuf::from(p.s.open_external(p.second).unwrap().path);
    for _ in 0..2 {
        p.s.save().unwrap();
        assert!(copy.is_file(), "kept while the project is open");
        assert!(!p.trash().join("tone edit 1.wav").exists());
    }
    p.s.close().unwrap();
    assert!(!copy.exists(), "closing lets it go");
    assert!(
        p.trash().join("tone edit 1.wav").is_file(),
        "to the Trash, never unlinked"
    );
    assert!(p.pkg().join("Media/tone.caf").is_file(), "the media stays");
}

/// Ruling 3 (pre-flight I1, M14): a titled Save As neither moves nor
/// copies the copy; it stays watched at the old package (and on macOS
/// still applies), and closing the new package leaves it.
#[test]
fn save_as_leaves_the_copies_where_they_are() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let old = p.pkg();
    let row = p.s.open_external(p.second).unwrap();
    let copy = PathBuf::from(&row.path);
    let new = p.dir.path().join("Q.vizdaw");
    p.s.save_as(new.display().to_string()).unwrap();
    assert!(copy.is_file() && copy.starts_with(&old), "not moved");
    assert!(!new.join("Media/Edits").exists(), "not copied");
    assert_eq!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::Copy {
            row: ExternalEditRow {
                event: p.second,
                path: row.path.clone(),
                from: p.media,
            }
        }
    );
    p.s.save().unwrap();
    assert!(copy.is_file(), "the new package's tidy leaves the old one's files");
    #[cfg(target_os = "macos")]
    {
        // macOS only: importing a WAV goes through ExtAudioFile (the CAF
        // importer reads CAF only).
        replace_with_wav(&copy, 2, 48_000, 24_000);
        let job = p.s.apply_external_edit(p.second, row.path.clone()).unwrap();
        assert_eq!(wait_job(&p.s, job).state, JobStateRow::Done);
        let m = media_row(&p.s, p.region(p.second).media.unwrap());
        assert!(new.join(&m.path).is_file(), "imported into the new package");
    }
    p.s.close().unwrap();
    assert!(copy.is_file(), "the old package keeps it until it is saved or closed");
}

/// Owner answer 2026-11-03 (pre-flight I2): an untitled project's copies
/// go to the Trash at close and at Save As, never unlinked; after Save As
/// the copy is gone.
#[test]
fn an_untitled_projects_copies_go_to_the_trash() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, false);
    let copy = PathBuf::from(p.s.open_external(p.second).unwrap().path);
    // <scratch package>/Media/Edits/<copy>
    let scratch = copy.ancestors().nth(3).unwrap().to_path_buf();
    assert!(copy.is_file() && scratch.extension().is_some_and(|e| e == "vizdaw"));
    p.s.close().unwrap();
    assert!(!scratch.exists(), "the scratch package is removed");
    assert!(
        p.trash().join("tone edit 1.wav").is_file(),
        "its copy went to the Trash first"
    );
    let q = build(BitDepthRow::Int24, false);
    let copy = PathBuf::from(q.s.open_external(q.second).unwrap().path);
    let pkg = q.dir.path().join("Named.vizdaw");
    q.s.save_as(pkg.display().to_string()).unwrap();
    assert!(!copy.exists() && q.trash().join("tone edit 1.wav").is_file());
    assert!(!pkg.join("Media/Edits").exists(), "Save As does not carry it");
    assert_eq!(
        q.s.external_edit(q.second).unwrap(),
        ExternalEditStateRow::Gone
    );
    assert_eq!(
        q.s.apply_external_edit(q.second, copy.display().to_string())
            .unwrap_err()
            .to_string(),
        GONE
    );
    q.s.close().unwrap();
}

/// Ruling 7 (pre-flight I10): what Reload finds.
#[test]
fn reload_finds_the_copy_or_says_it_is_gone() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    assert_eq!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::NoEdit
    );
    let row = p.s.open_external(p.second).unwrap();
    assert_eq!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::Copy { row: row.clone() }
    );
    assert_eq!(
        p.s.external_edit(p.first).unwrap(),
        ExternalEditStateRow::NoEdit,
        "the other part has none"
    );
    std::fs::remove_file(&row.path).unwrap();
    assert_eq!(
        p.s.external_edit(p.second).unwrap(),
        ExternalEditStateRow::Gone
    );
    p.s.close().unwrap();
    // A reopened project whose region plays an external edit's media: no
    // copy in this session.
    let dir = tempfile::tempdir().unwrap();
    let pkg = dir.path().join("E.vizdaw");
    let mut b = Builder::new();
    let (seq, lane) = b.sequence("Song", "Audio", None).unwrap();
    let a = b.media("Media/a.caf", 4_800, 2).unwrap();
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: "Media/a edit 1.caf".into(),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 4_800,
            content_hash: None,
            name: Some("a edit 1".into()),
            origin: Some(MediaOrigin::ExternalEdit { from: a }),
        }))
        .unwrap();
    let edited = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap();
    let on_edit = b
        .region(seq, lane, 0, edited, 0, 4_800, 0.0, Fade::default(), Fade::default())
        .unwrap();
    let on_a = b
        .region(seq, lane, 96_000, a, 0, 4_800, 0.0, Fade::default(), Fade::default())
        .unwrap();
    core_model::file::save(&b.finish(), &pkg).unwrap();
    let s = Session::open(pkg.display().to_string()).unwrap();
    s.debug_set_trash_dir(Some(dir.path().join("Trash").display().to_string()));
    assert_eq!(
        s.external_edit(on_edit.0).unwrap(),
        ExternalEditStateRow::Gone
    );
    assert_eq!(s.external_edit(on_a.0).unwrap(), ExternalEditStateRow::NoEdit);
    assert!(matches!(
        s.external_edit(999_999),
        Err(VizError::Model { .. })
    ));
    s.close().unwrap();
}

/// Review Focus 2: a mono, shorter edit at another rate relinks that
/// region alone as one step "External Edit"; its fades are clamped.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn a_shorter_mono_edit_relinks_the_region() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    // Long fades, so the shorter edit must clamp them.
    p.s.edit(
        EditCommand::Audio {
            edit: AudioEdit::SetFades {
                event: p.second,
                fade_in: fade(20_000),
                fade_out: fade(20_000),
            },
        },
        None,
    )
    .unwrap();
    let r = p.region(p.second);
    let other = p.region(p.first);
    let copy = p.s.open_external(p.second).unwrap().path;
    // 30 % shorter, mono, at 44.1 kHz in this 48 kHz project.
    let src = ((r.frames as f64) * 0.7 * 44_100.0 / 48_000.0).round() as u32;
    replace_with_wav(Path::new(&copy), 1, 44_100, src);
    let job = p.s.apply_external_edit(p.second, copy).unwrap();
    let j = wait_job(&p.s, job);
    assert_eq!(j.state, JobStateRow::Done, "{j:?}");
    assert_eq!(
        p.s.snapshot().unwrap().undo_label.as_deref(),
        Some("External Edit")
    );
    let new = p.region(p.second);
    let m = media_row(&p.s, new.media.unwrap());
    assert_eq!(j.created, vec![m.id]);
    assert_eq!(
        (m.origin, m.channels, m.sample_rate, m.name.as_str()),
        (MediaOriginRow::ExternalEdit, 1, 48_000, "tone edit 1")
    );
    let want = (f64::from(src) * 48_000.0 / 44_100.0).round() as u64;
    assert!(m.frames.abs_diff(want) <= 1, "{} vs {want}", m.frames);
    assert_eq!(
        (new.source_offset, new.frames, new.tick, new.gain_db),
        (0, m.frames, r.tick, r.gain_db)
    );
    assert_eq!(
        (new.fade_in.length, new.fade_out.length),
        (20_000, m.frames as i64 - 20_000),
        "fade in first, then the fade out within what is left"
    );
    assert_eq!(p.region(p.first), other, "the other region is unaffected");
    assert_eq!(media_row(&p.s, p.media).origin, MediaOriginRow::Imported);
    assert!(p.pkg().join("Media/tone.caf").is_file(), "write-once");
    p.s.close().unwrap();
}

#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn undo_restores_the_original_span() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let before = p.region(p.second);
    let copy = p.s.open_external(p.second).unwrap().path;
    replace_with_wav(Path::new(&copy), 2, 48_000, 12_000);
    let job = p.s.apply_external_edit(p.second, copy).unwrap();
    assert_eq!(wait_job(&p.s, job).state, JobStateRow::Done);
    let after = p.region(p.second);
    assert_eq!((after.source_offset, after.frames), (0, 12_000));
    p.s.undo().unwrap();
    assert_eq!(p.region(p.second), before, "the original media, offset and length");
    assert_eq!(
        p.s.snapshot().unwrap().redo_label.as_deref(),
        Some("External Edit")
    );
    p.s.redo().unwrap();
    assert_eq!(p.region(p.second), after);
    p.s.close().unwrap();
}

/// Review Focus 2 (pre-flight I3): an empty, garbage or 3-channel edit is
/// a Failed job, the document unchanged, what it imported removed.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn an_unreadable_or_wide_edit_changes_nothing() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let pkg = p.pkg();
    let copy = PathBuf::from(p.s.open_external(p.second).unwrap().path);
    let region = p.region(p.second);
    let undo = p.s.snapshot().unwrap().undo_label;
    let before = cafs(&pkg);
    let cases: [(&str, &dyn Fn(&Path), Option<&str>); 3] = [
        ("empty", &|c: &Path| std::fs::write(c, b"").unwrap(), None),
        (
            "garbage",
            &|c: &Path| std::fs::write(c, b"not audio at all, only bytes").unwrap(),
            None,
        ),
        (
            "wide",
            &|c: &Path| wav_at(c, 48_000, 3, 4_800),
            Some("The edit has 3 channels; regions hold one or two."),
        ),
    ];
    for (what, write, message) in cases {
        let _ = std::fs::remove_file(&copy);
        write(&copy);
        let job = p
            .s
            .apply_external_edit(p.second, copy.display().to_string())
            .unwrap();
        let j = wait_job(&p.s, job);
        assert_eq!(j.state, JobStateRow::Failed, "{what}: {j:?}");
        match message {
            Some(m) => assert_eq!(j.message.as_deref(), Some(m), "{what}"),
            None => assert!(j.message.is_some(), "{what}"),
        }
        assert_eq!(p.region(p.second), region, "{what}: unchanged");
        assert_eq!(p.s.snapshot().unwrap().undo_label, undo, "{what}");
        assert_eq!(cafs(&pkg), before, "{what}: what it imported is removed");
    }
    p.s.close().unwrap();
}

/// Review Focus 3 (pre-flight M10): an edit of a region deleted meanwhile
/// is dropped, what it imported removed, and the copy forgotten.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn an_edit_of_a_deleted_region_is_dropped() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let pkg = p.pkg();
    let copy = p.s.open_external(p.second).unwrap().path;
    replace_with_wav(Path::new(&copy), 2, 48_000, 12_000);
    let before = cafs(&pkg);
    p.s.debug_hold_jobs(true).unwrap();
    let job = p.s.apply_external_edit(p.second, copy.clone()).unwrap();
    p.s.edit(
        EditCommand::Event {
            edit: EventEdit::Delete {
                events: vec![p.second],
            },
        },
        None,
    )
    .unwrap();
    p.s.debug_hold_jobs(false).unwrap();
    let j = wait_job(&p.s, job);
    assert_eq!(
        (j.state, j.message.as_deref()),
        (
            JobStateRow::Failed,
            Some("The region was deleted; the edit was not applied.")
        )
    );
    assert_eq!(cafs(&pkg), before, "the imported file is removed");
    assert!(p.s.external_edit(p.second).is_err(), "the region is gone");
    assert!(
        matches!(
            p.s.apply_external_edit(p.second, copy),
            Err(VizError::Model { .. })
        ),
        "its copy is forgotten"
    );
    p.s.close().unwrap();
}

/// Review Focus 3 (pre-flight M6): a region moved while the editor has it
/// is relinked where it is now; after a split the part keeping the ID
/// takes the whole edit (offset 0, its length) and the other keeps the
/// original.
#[cfg(target_os = "macos")] // macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)
#[test]
fn a_split_or_moved_region_is_relinked_where_it_is() {
    let _g = serial();
    let p = build(BitDepthRow::Int24, true);
    let copy = p.s.open_external(p.second).unwrap().path;
    p.s.edit(
        EditCommand::Event {
            edit: EventEdit::Move {
                events: vec![p.second],
                delta: 61_440,
                to_lane: None,
            },
        },
        None,
    )
    .unwrap();
    let moved = p.region(p.second);
    p.s.edit(
        EditCommand::Audio {
            edit: AudioEdit::Split {
                event: p.second,
                at: moved.tick + moved.duration / 2,
            },
        },
        None,
    )
    .unwrap();
    let third = region_rows(&p.s, p.seq, p.lane)
        .into_iter()
        .find(|r| r.id != p.first && r.id != p.second)
        .unwrap();
    replace_with_wav(Path::new(&copy), 2, 48_000, 12_000);
    let job = p.s.apply_external_edit(p.second, copy).unwrap();
    assert_eq!(wait_job(&p.s, job).state, JobStateRow::Done);
    let r = p.region(p.second);
    assert_eq!(
        (r.tick, r.source_offset, r.frames),
        (moved.tick, 0, 12_000),
        "where it is now"
    );
    assert_ne!(r.media, Some(p.media));
    assert_eq!(p.region(third.id), third, "the split's other part keeps the original");
    assert_eq!(p.region(p.first).media, Some(p.media));
    p.s.close().unwrap();
}
```

- [ ] **Step 2: Run them to see them fail.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test external
```
Expected: FAIL to compile — no `external` module, no `open_external`.

- [ ] **Step 3: Implement** the excerpts above (`external.rs` whole; `lib.rs`, `jobs.rs`, `session.rs`; `ProjectMedia.swift`; `FakeSession.swift`).

- [ ] **Step 4: Run the tests and the gates.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
```
Expected: PASS; Rust +12 on macOS (+7 on Linux: the six tests without a cfg and `save_as_leaves_the_copies_where_they_are`); Swift unchanged (946; it compiles with the new cases and stubs).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/external.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/session.rs crates/core-ffi/src/jobs.rs crates/core-ffi/tests/external.rs app/Sources/VizDAWAppModel/ProjectMedia.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: external-edit copies of a region's span, kept until close and trashed, and the External Edit job relinking that region alone (plan 5f-3 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -9
```
Counts: Rust +12 on macOS (1889 / 16 ignored), +7 on Linux.

---

### Task 4: The watcher, the openers, `ExternalEditModel`, the wiring

**Files:**
- Create: `app/Sources/VizDAWAppModel/ExternalEditWatcher.swift`, `app/Sources/VizDAWAppModel/ExternalEditModel.swift`, `app/Sources/VizDAW/ExternalEditing.swift`, `app/Tests/VizDAWAppModelTests/ExternalEditWatcherTests.swift`, `app/Tests/VizDAWAppModelTests/ExternalEditModelTests.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift` (`externalTools`, `external`, `stopExternalEdits`, the tick in `idle`), `app/Sources/VizDAWAppModel/DocumentStore.swift` (`externalTools`, handed to each model; `close` stops the watches before the session closes), `app/Sources/VizDAW/VizDAWApp.swift` (the live tools)

**Interfaces:**
- Consumes: `openExternal(event:)`, `externalEdit(event:)`, `applyExternalEdit(event:path:)`, `job(id:)` (Task 3); `DocumentModel.startJob` (`ProjectMedia.swift`, internal to the module), `DocumentModel.alert`, `pollJobs`.
- Produces (`VizDAWAppModel`):
  ```swift
  public struct FileStamp: Equatable, Sendable { public let size: UInt64; public let modified: Date }
  public protocol FileProbe { func stamp(_ path: String) -> FileStamp? }
  public struct DiskProbe: FileProbe
  @MainActor public protocol FileWatching { func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject? }
  @MainActor public final class FakeWatching: FileWatching { started: [String]; live: Int; fire(_:) }
  @MainActor public final class ExternalEditWatcher { stableSeconds; path; init(path:probe:watching:now:applied:); changed(); due() -> FileStamp?; applied(_:); skip(_:); changedSinceApplied() -> FileStamp?; stop() }
  @MainActor public protocol ExternalOpener { func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws }
  @MainActor public final class FakeOpener: ExternalOpener; @MainActor public final class StubOpener: ExternalOpener
  public enum ExternalEditorChoice: Equatable, Sendable { case none, app(String), stub }
  public enum ExternalOutcome: Equatable, Sendable { case importing(UInt64), unchanged, unavailable(String), busy(String), failed(String) }
  @MainActor public struct ExternalTools { opener; watching; probe; now }
  @MainActor public final class ExternalEditModel { open(_:app:) throws -> String; reload(_:announce:) -> ExternalOutcome; tick(); stopAll(); importing; watched }
  DocumentModel.externalTools: ExternalTools?; DocumentModel.external: ExternalEditModel; DocumentModel.stopExternalEdits()
  DocumentStore.externalTools: ExternalTools?
  ```
- Produces (`VizDAW` target): `LiveFileWatching`, `WorkspaceOpener`, `ExternalEditorPanel` (used by Task 5).

Create `app/Sources/VizDAWAppModel/ExternalEditWatcher.swift`:

```swift
import Foundation

/// A file's size and modification time: the stable rule compares them
/// (spec §11; plan 5f-3 Ruling 4).
public struct FileStamp: Equatable, Sendable {
    public let size: UInt64
    public let modified: Date

    public init(size: UInt64, modified: Date) {
        self.size = size
        self.modified = modified
    }
}

/// Reads a path's stamp; nil when it is missing or not a regular file.
public protocol FileProbe {
    func stamp(_ path: String) -> FileStamp?
}

/// The disk's stamps (`FileManager` attributes).
public struct DiskProbe: FileProbe {
    public init() {}

    public func stamp(_ path: String) -> FileStamp? {
        guard let a = try? FileManager.default.attributesOfItem(atPath: path),
            (a[.type] as? FileAttributeType) == .typeRegular,
            let size = (a[.size] as? NSNumber)?.uint64Value,
            let modified = a[.modificationDate] as? Date
        else { return nil }
        return FileStamp(size: size, modified: modified)
    }
}

/// Watch events for one path (the app: a `DispatchSource`; tests: a fake).
/// The token stops the watch when released. Nil when the path cannot be
/// watched (it is missing): the model then waits for Reload (pre-flight
/// M15).
@MainActor
public protocol FileWatching {
    func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject?
}

/// Watching for tests and for documents the app gave no tools: records
/// the paths and sends events only when told (`fire`).
@MainActor
public final class FakeWatching: FileWatching {
    @MainActor final class Token {
        let path: String
        let changed: @MainActor () -> Void

        init(path: String, changed: @escaping @MainActor () -> Void) {
            self.path = path
            self.changed = changed
        }
    }

    private struct Weak {
        weak var token: Token?
    }

    private var tokens: [Weak] = []
    /// Every path a watch started on, in order.
    public private(set) var started: [String] = []

    public init() {}

    public func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject? {
        started.append(path)
        let t = Token(path: path, changed: changed)
        tokens.append(Weak(token: t))
        return t
    }

    /// Watches still held.
    public var live: Int { tokens.filter { $0.token != nil }.count }

    /// A watch event for `path`, to every live watch on it.
    public func fire(_ path: String) {
        for w in tokens {
            if let t = w.token, t.path == path { t.changed() }
        }
    }
}

/// The 1 s stable rule for one edit copy (Ruling 4): a change is due once
/// the file's stamp has stayed the same for `stableSeconds` since the last
/// change, exists, and is neither the stamp last applied nor the one last
/// skipped.
@MainActor
public final class ExternalEditWatcher {
    public static let stableSeconds: TimeInterval = 1

    public let path: String
    private let probe: any FileProbe
    private let now: () -> Date
    private var token: AnyObject?
    /// The last change: when, and the stamp then (nil: missing).
    private var change: (at: Date, stamp: FileStamp?)?
    /// The stamp last imported (or written by Open).
    public private(set) var appliedStamp: FileStamp?
    /// The stamp of the last import that failed or was cancelled: not
    /// retried by itself.
    private var skipped: FileStamp?
    public private(set) var isStopped = false

    /// Watches `path`; `applied` is the stamp already in the project (the
    /// copy Open wrote: pre-flight I10).
    public init(
        path: String, probe: any FileProbe, watching: any FileWatching, now: @escaping () -> Date,
        applied: FileStamp? = nil
    ) {
        self.path = path
        self.probe = probe
        self.now = now
        self.appliedStamp = applied
        token = watching.watch(path) { [weak self] in self?.changed() }
    }

    /// A watch event (Reload calls it too): the stamp now, and when.
    public func changed() {
        guard !isStopped else { return }
        change = (now(), probe.stamp(path))
    }

    /// The stamp to import now, or nil. A stamp that differs from the last
    /// change's is a change of its own (an event lost or coalesced, a file
    /// still growing; pre-flight M1): the second starts again from now.
    public func due() -> FileStamp? {
        guard !isStopped, let c = change else { return nil }
        let s = probe.stamp(path)
        if s != c.stamp {
            change = (now(), s)
            return nil
        }
        guard let s, s != appliedStamp, s != skipped,
            now().timeIntervalSince(c.at) >= Self.stableSeconds
        else { return nil }
        return s
    }

    /// `s` was imported: it is never due again.
    public func applied(_ s: FileStamp) {
        appliedStamp = s
        if change?.stamp == s { change = nil }
    }

    /// `s` failed or was cancelled: not retried until the file changes
    /// (Reload still retries it).
    public func skip(_ s: FileStamp) {
        skipped = s
    }

    /// Reload's check (no second's wait: the user asked): the stamp now
    /// when it exists and differs from the one applied.
    public func changedSinceApplied() -> FileStamp? {
        guard let s = probe.stamp(path), s != appliedStamp else { return nil }
        return s
    }

    /// Stops watching (the document closes, the region is gone).
    public func stop() {
        isStopped = true
        token = nil
        change = nil
    }
}
```

Create `app/Sources/VizDAWAppModel/ExternalEditModel.swift`:

```swift
import Foundation
import VizDAWCore

/// The editor `region.open_external` and keys use without `app` (plan
/// 5f-3 Ruling 5): none chosen, the chosen application's path, or the
/// stub (`VIZDAW_EXTERNAL_EDITOR=stub`), which needs none.
public enum ExternalEditorChoice: Equatable, Sendable {
    case none
    case app(String)
    case stub
}

/// What Reload External Edit (or an import the watcher starts) came to.
public enum ExternalOutcome: Equatable, Sendable {
    /// An import started (its job).
    case importing(UInt64)
    /// Nothing changed since the last import.
    case unchanged
    /// Not for this region; the message says why.
    case unavailable(String)
    /// The core is busy (an import runs); try again.
    case busy(String)
    /// Refused or gone; the message says why.
    case failed(String)
}

/// Opens an edit copy in the chosen application (Ruling 5).
@MainActor
public protocol ExternalOpener {
    /// Opens `file` with `app` (nil: the stub, which needs none). A
    /// refusal found at once throws; one the system reports later comes to
    /// `failed` (pre-flight M9).
    func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws
}

/// Records what it would open and launches nothing (tests).
@MainActor
public final class FakeOpener: ExternalOpener {
    public struct Opened: Equatable, Sendable {
        public let file: URL
        public let app: URL?

        public init(file: URL, app: URL?) {
            self.file = file
            self.app = app
        }
    }

    public private(set) var opened: [Opened] = []
    /// Thrown by each open (an application refused at once).
    public var error: VizError?
    /// Reported after each open (the system's open failing later).
    public var lateFailure: String?

    public init() {}

    public func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws {
        if let error { throw error }
        opened.append(Opened(file: file, app: app))
        if let lateFailure { failed(lateFailure) }
    }
}

/// `VIZDAW_EXTERNAL_EDITOR=stub` (spec §12.1; Ruling 5): records the copy
/// and launches nothing; the e2e edits the file itself.
@MainActor
public final class StubOpener: ExternalOpener {
    public private(set) var last: URL?

    public init() {}

    public func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws {
        last = file
    }
}

/// How documents open and watch edit copies (pre-flight I7(a)): the app
/// sets the live ones on the store (`VizDAWApp`); fakes otherwise, so no
/// test launches an application or watches the disk.
@MainActor
public struct ExternalTools {
    public var opener: any ExternalOpener
    public var watching: any FileWatching
    public var probe: any FileProbe
    public var now: () -> Date

    public init(
        opener: any ExternalOpener = FakeOpener(), watching: any FileWatching = FakeWatching(),
        probe: any FileProbe = DiskProbe(), now: @escaping () -> Date = { Date() }
    ) {
        self.opener = opener
        self.watching = watching
        self.probe = probe
        self.now = now
    }
}

/// One document's external edits (spec §11; plan 5f-3 Rulings 4, 5, 7):
/// Open writes and opens a copy and watches it; each tick imports the
/// copies that are due; Reload imports by hand; closing stops every
/// watch. Imports the tick or the menu start go through
/// `DocumentModel.startJob`, so their banner and a failure's single alert
/// come from `pollJobs` (pre-flight I7(d)); this model only reads its
/// jobs to know when a stamp was applied.
@MainActor
public final class ExternalEditModel {
    public static let openFailedTitle = "Open in External Editor Failed"
    public static let failedTitle = "External Edit Failed"
    public static let goneMessage = "The edit copy is gone; Open in External Editor again."
    public static let noEditMessage = "This region has no external edit; use Open in External Editor."
    public static let importingMessage = "The external edit is being imported."

    private unowned let document: DocumentModel
    private let opener: any ExternalOpener
    private let watching: any FileWatching
    private let probe: any FileProbe
    private let now: () -> Date
    private var watchers: [UInt64: ExternalEditWatcher] = [:]
    private struct Running {
        let job: UInt64
        let stamp: FileStamp
    }
    private var running: [UInt64: Running] = [:]
    private var closed = false

    public init(
        document: DocumentModel, opener: any ExternalOpener, watching: any FileWatching,
        probe: any FileProbe = DiskProbe(), now: @escaping () -> Date = { Date() }
    ) {
        self.document = document
        self.opener = opener
        self.watching = watching
        self.probe = probe
        self.now = now
    }

    /// Regions whose edit is being imported.
    public var importing: Set<UInt64> { Set(running.keys) }
    /// Regions whose copy is watched.
    public var watched: Set<UInt64> { Set(watchers.keys) }

    /// Open in External Editor (Ruling 5): the core writes the copy, the
    /// opener opens it with `app`, and the copy is watched from the stamp
    /// it was written with (Reload right after it is unchanged; pre-flight
    /// I10). A refusal throws, and nothing is watched. A failure the
    /// system reports later is the alert "Open in External Editor Failed";
    /// the watch stays (the user can open the copy by hand).
    @discardableResult
    public func open(_ region: UInt64, app: URL?) throws -> String {
        let row = try document.session.openExternal(event: region)
        try opener.open(URL(fileURLWithPath: row.path), with: app) { [weak d = document] message in
            d?.alert = AlertItem(title: ExternalEditModel.openFailedTitle, message: message)
        }
        watch(region, row.path, applied: probe.stamp(row.path))
        return row.path
    }

    /// Reload External Edit (Ruling 7): imports the copy now when it
    /// differs from what was imported. `announce`: the import shows its
    /// banner and a failure's alert, as any job (the menu); the channel
    /// answers for itself.
    public func reload(_ region: UInt64, announce: Bool = true) -> ExternalOutcome {
        guard !closed else { return .unavailable(Self.noEditMessage) }
        poll()
        if running[region] != nil { return .busy(Self.importingMessage) }
        let state: ExternalEditStateRow
        do {
            state = try document.session.externalEdit(event: region)
        } catch {
            return refusal(error)
        }
        switch state {
        case .noEdit:
            return .unavailable(Self.noEditMessage)
        case .gone:
            return .failed(Self.goneMessage)
        case .copy(let row):
            // A watch the region lost (its copy forgotten, then undone)
            // starts again with nothing applied.
            if watchers[region]?.path != row.path { watch(region, row.path, applied: nil) }
            guard let w = watchers[region] else { return .failed(Self.goneMessage) }
            guard let s = w.changedSinceApplied() else {
                return probe.stamp(row.path) == nil ? .failed(Self.goneMessage) : .unchanged
            }
            return start(region, w, s, announce: announce)
        }
    }

    /// The document's tick (`DocumentModel.idle`): ended imports are noted,
    /// then each copy that is due and has no import running starts one.
    public func tick() {
        guard !closed else { return }
        poll()
        for (region, w) in watchers where running[region] == nil {
            if let s = w.due() { _ = start(region, w, s, announce: true) }
        }
    }

    /// The document closes: every watch stops; nothing starts again.
    public func stopAll() {
        closed = true
        for w in watchers.values { w.stop() }
        watchers = [:]
        running = [:]
    }

    private func watch(_ region: UInt64, _ path: String, applied: FileStamp?) {
        watchers[region]?.stop()
        watchers[region] = ExternalEditWatcher(
            path: path, probe: probe, watching: watching, now: now, applied: applied)
    }

    private func stopWatching(_ region: UInt64) {
        watchers.removeValue(forKey: region)?.stop()
    }

    /// Starts importing `s` of `w`. A busy core is retried at the next
    /// tick, without an alert (pre-flight I11); any other refusal skips
    /// `s` (and, from the tick, is an alert).
    private func start(_ region: UInt64, _ w: ExternalEditWatcher, _ s: FileStamp, announce: Bool)
        -> ExternalOutcome
    {
        let path = w.path
        var refused: (any Error)?
        var job: UInt64?
        if announce {
            job = document.startJob(.externalEdit, Self.failedTitle, failure: { refused = $0 }) {
                try $0.applyExternalEdit(event: region, path: path)
            }
        } else {
            do {
                job = try document.session.applyExternalEdit(event: region, path: path)
            } catch {
                refused = error
            }
        }
        if let job {
            running[region] = Running(job: job, stamp: s)
            return .importing(job)
        }
        let outcome = refusal(refused)
        if case .busy = outcome { return outcome }
        w.skip(s)
        if announce, case .failed(let why) = outcome {
            document.alert = AlertItem(title: Self.failedTitle, message: why)
        }
        forgetIfGone(region)
        return outcome
    }

    /// Notes ended imports: Done marks the stamp applied; Failed or
    /// Cancelled skips it, the watch kept (pre-flight I7(d), I8); a region
    /// the core forgot stops being watched (pre-flight M10).
    private func poll() {
        for (region, r) in running {
            let j: JobRow
            do {
                j = try document.session.job(id: r.job)
            } catch {
                if case .Busy? = error as? VizError { continue }
                running[region] = nil
                continue
            }
            guard j.state != .running else { continue }
            running[region] = nil
            if j.state == .done {
                watchers[region]?.applied(r.stamp)
            } else {
                watchers[region]?.skip(r.stamp)
                if j.state == .failed { forgetIfGone(region) }
            }
        }
    }

    /// Stops the watch of a region that is gone or whose copy the core
    /// forgot, so later saves in the editor raise nothing (pre-flight M10).
    /// A copy that is only missing stays watched for its return.
    private func forgetIfGone(_ region: UInt64) {
        switch try? document.session.externalEdit(event: region) {
        case .copy?, .gone?: return
        case .noEdit?, nil: stopWatching(region)
        }
    }

    private func refusal(_ error: (any Error)?) -> ExternalOutcome {
        switch error as? VizError {
        case .Busy(let message)?: return .busy(message)
        case let e?: return .failed(e.message)
        case nil: return .failed(error?.localizedDescription ?? "The external edit was refused.")
        }
    }
}
```

Create `app/Sources/VizDAW/ExternalEditing.swift`:

```swift
import AppKit
import Foundation
import UniformTypeIdentifiers
import VizDAWAppModel
import VizDAWCore

/// Watches one path with a `DispatchSource` file-system-object source for
/// write, rename or delete events (spec §11; plan 5f-3 Ruling 4), on the
/// main queue. An editor that saves by replacing the file leaves the
/// source on the old file: on a rename or delete the source is cancelled
/// and the path reopened every 0.5 s until it exists again; the new file
/// is then watched and reported as a change (Review Focus 1; the e2e's
/// second, in-place save pins it).
@MainActor
final class LiveFileWatching: FileWatching {
    func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject? {
        let w = FileWatch(path: path, changed: changed)
        return w.arm() ? w : nil
    }
}

/// One path's watch; used on the main queue only. Released, it cancels
/// its source (which closes the descriptor) and stops reopening.
private final class FileWatch: @unchecked Sendable {
    let path: String
    let changed: @MainActor () -> Void
    private var source: DispatchSourceFileSystemObject?

    init(path: String, changed: @escaping @MainActor () -> Void) {
        self.path = path
        self.changed = changed
    }

    deinit { source?.cancel() }

    /// Opens the path (`O_EVTONLY`) and starts its source; false when it
    /// cannot be opened.
    func arm() -> Bool {
        let fd = open(path, O_EVTONLY)
        guard fd >= 0 else { return false }
        let s = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fd, eventMask: [.write, .rename, .delete], queue: .main)
        s.setEventHandler { [weak self] in
            MainActor.assumeIsolated { self?.event() }
        }
        s.setCancelHandler { close(fd) }
        source = s
        s.resume()
        return true
    }

    @MainActor private func event() {
        guard let s = source else { return }
        let flags = s.data
        changed()
        if flags.contains(.rename) || flags.contains(.delete) {
            s.cancel()
            source = nil
            reopen()
        }
    }

    @MainActor private func reopen() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            MainActor.assumeIsolated {
                guard let self else { return }
                if self.arm() { self.changed() } else { self.reopen() }
            }
        }
    }
}

/// Opens the copy in the chosen application with `NSWorkspace` (Ruling 5).
/// The system reports a failure later, in its completion (pre-flight M9).
@MainActor
final class WorkspaceOpener: ExternalOpener {
    func open(_ file: URL, with app: URL?, failed: @escaping @MainActor @Sendable (String) -> Void) throws {
        guard let app else { throw VizError.Model(message: "Choose an external editor in Settings › Editing.") }
        guard FileManager.default.fileExists(atPath: app.path) else {
            throw VizError.Io(message: "\(app.path) is not an application.")
        }
        NSWorkspace.shared.open([file], withApplicationAt: app, configuration: NSWorkspace.OpenConfiguration()) {
            _, error in
            guard let error else { return }
            let message = error.localizedDescription
            Task { @MainActor in failed(message) }
        }
    }
}

/// The application chooser of Settings › Editing and of the first Region ›
/// Open in External Editor (Ruling 5): applications only. Never shown for
/// a key, a surface button or the channel (owner answer 2026-11-03).
@MainActor
enum ExternalEditorPanel {
    static func choose() -> String? {
        let panel = NSOpenPanel()
        panel.title = "Choose External Sample Editor"
        panel.prompt = "Choose"
        panel.message = "Choose the application that edits audio files for Open in External Editor."
        panel.allowedContentTypes = [.application]
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.treatsFilePackagesAsDirectories = false
        panel.allowsMultipleSelection = false
        return panel.runModal() == .OK ? panel.url?.path : nil
    }
}
```

(The literal is `RegionCommands.noEditor`, which comes in Task 5; with the app's stub a nil `app` never reaches this opener.)

Find in `app/Sources/VizDAWAppModel/DocumentModel.swift`:
```swift
    @ObservationIgnored private var synthModel: SynthSheets?
```
Replace:
```swift
    @ObservationIgnored private var synthModel: SynthSheets?
    @ObservationIgnored private var externalModel: ExternalEditModel?
    /// How this document opens and watches edit copies (plan 5f-3): the
    /// store hands on the app's; nil gives fakes, which launch and watch
    /// nothing.
    @ObservationIgnored public var externalTools: ExternalTools?
```

Find in `app/Sources/VizDAWAppModel/DocumentModel.swift`:
```swift
    /// The media pool (plan 5b), made once.
```
Replace:
```swift
    /// The external sample editor (plan 5f-3), made once from
    /// `externalTools`.
    public var external: ExternalEditModel {
        if let m = externalModel { return m }
        let t = externalTools ?? ExternalTools()
        let m = ExternalEditModel(document: self, opener: t.opener, watching: t.watching, probe: t.probe, now: t.now)
        externalModel = m
        return m
    }

    /// The document closes: every external-edit watch stops
    /// (`DocumentStore.close`; pre-flight I7(b)).
    public func stopExternalEdits() { externalModel?.stopAll() }

    /// The media pool (plan 5b), made once.
```

Find in `app/Sources/VizDAWAppModel/DocumentModel.swift`:
```swift
        if !jobs.isEmpty { pollJobs() }
```
Replace:
```swift
        if !jobs.isEmpty { pollJobs() }
        // Edit copies that are due start their import (plan 5f-3 Ruling 4).
        externalModel?.tick()
```

Find in `app/Sources/VizDAWAppModel/DocumentStore.swift`:
```swift
    public var keyBindings: KeyBindings?
```
Replace:
```swift
    public var keyBindings: KeyBindings?
    /// The external sample editor's opener and watching, handed to every
    /// document (plan 5f-3; the app sets the live ones, nil: fakes).
    @ObservationIgnored public var externalTools: ExternalTools?
```

Find in `app/Sources/VizDAWAppModel/DocumentStore.swift`:
```swift
            model.keys = keyBindings
```
Replace:
```swift
            model.keys = keyBindings
            model.externalTools = externalTools
```

Find in `app/Sources/VizDAWAppModel/DocumentStore.swift`:
```swift
            if doc.isPlaying { try? doc.session.stop() }
```
Replace:
```swift
            if doc.isPlaying { try? doc.session.stop() }
            // Watching lasts until the project closes (spec §11).
            doc.stopExternalEdits()
```

Find in `app/Sources/VizDAW/VizDAWApp.swift`:
```swift
        appStore.keyBindings = keyBindings
```
Replace:
```swift
        appStore.keyBindings = keyBindings
        // The external sample editor (plan 5f-3 Ruling 5): the stub launches
        // nothing (CI, the e2e).
        let stub = ProcessInfo.processInfo.environment["VIZDAW_EXTERNAL_EDITOR"] == "stub"
        appStore.externalTools = ExternalTools(
            opener: stub ? StubOpener() as any ExternalOpener : WorkspaceOpener(),
            watching: LiveFileWatching())
```

- [ ] **Step 1: Write the failing tests.**

Create `app/Tests/VizDAWAppModelTests/ExternalEditWatcherTests.swift`:

```swift
import Foundation
import Testing

@testable import VizDAWAppModel

/// A clock the tests set; never a sleep.
final class FakeClock {
    var t = Date(timeIntervalSince1970: 1_000)

    func advance(_ s: TimeInterval) { t = t.addingTimeInterval(s) }
}

/// The external-edit watcher's stable-file rule with temp files (spec
/// §12.3; plan 5f-3 Ruling 4, Review Focus 1).
@MainActor
@Suite struct ExternalEditWatcherTests {
    let clock = FakeClock()
    let watching = FakeWatching()
    let dir: URL
    let path: String

    init() throws {
        dir = FileManager.default.temporaryDirectory.appendingPathComponent("vizdaw-watch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        path = dir.appendingPathComponent("Kick edit 1.wav").path
    }

    func write(_ bytes: Int, _ fill: UInt8 = 1) throws {
        try Data(repeating: fill, count: bytes).write(to: URL(fileURLWithPath: path))
    }

    func watcher() -> ExternalEditWatcher {
        ExternalEditWatcher(path: path, probe: DiskProbe(), watching: watching, now: { [clock] in clock.t })
    }

    /// The fake clock at `s` seconds.
    func at(_ s: TimeInterval) { clock.t = Date(timeIntervalSince1970: 1_000 + s) }

    @Test func aChangeIsDueAfterOneStableSecond() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        #expect(watching.started == [path] && watching.live == 1)
        #expect(w.due() == nil, "no change yet")
        at(0)
        watching.fire(path)
        at(0.5)
        #expect(w.due() == nil)
        at(1.0)
        let s = try #require(w.due())
        #expect(s == DiskProbe().stamp(path) && s.size == 100)
        #expect(w.due() == s, "still due until it is applied")
        w.applied(s)
        at(5)
        #expect(w.due() == nil)
    }

    @Test func aSecondSaveRestartsTheSecond() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        at(0)
        watching.fire(path)
        at(0.8)
        try write(200, 2)
        watching.fire(path)
        at(1.2)
        #expect(w.due() == nil, "0.4 s after the second save")
        at(1.8)
        #expect(w.due()?.size == 200)
    }

    @Test func aFileStillGrowingIsNotDue() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        at(0)
        watching.fire(path)
        at(0.9)
        try write(150)  // no event: the editor is still writing
        at(1.0)
        #expect(w.due() == nil, "the stamp differs from the change's")
        at(1.9)
        #expect(w.due() == nil)
        at(2.0)
        #expect(w.due()?.size == 150, "a second after the tick that saw it grow")
    }

    @Test func oneChangeIsAppliedOnce() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        at(0)
        watching.fire(path)
        at(1)
        let s = try #require(w.due())
        w.applied(s)
        at(1.5)
        watching.fire(path)  // an event with nothing changed
        at(3)
        #expect(w.due() == nil)
        w.stop()
        #expect(watching.live == 0 && w.isStopped)
        watching.fire(path)
        #expect(w.due() == nil)
    }

    @Test func aRenameOverTheFileRearmsTheWatch() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        let tmp = dir.appendingPathComponent(".Kick edit 1.tmp")
        try Data(repeating: 2, count: 300).write(to: tmp)
        #expect(rename(tmp.path, path) == 0)
        at(0)
        watching.fire(path)
        at(1)
        #expect(w.due()?.size == 300, "the probe reads the path, not the replaced file")
    }

    @Test func aDeletedFileWaitsForItsReturn() throws {
        defer { try? FileManager.default.removeItem(at: dir) }
        try write(100)
        let w = watcher()
        try FileManager.default.removeItem(atPath: path)
        at(0)
        watching.fire(path)
        at(5)
        #expect(w.due() == nil, "never due while it is missing")
        #expect(watching.live == 1, "still watched")
        try write(400)
        at(6)
        watching.fire(path)
        at(6.5)
        #expect(w.due() == nil)
        at(7)
        #expect(w.due()?.size == 400)
    }
}
```

Create `app/Tests/VizDAWAppModelTests/ExternalEditModelTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// Stamps the tests set by path.
final class FakeProbe: FileProbe {
    var stamps: [String: FileStamp] = [:]

    func stamp(_ path: String) -> FileStamp? { stamps[path] }
}

/// One document's external edits (plan 5f-3 Task 4; Rulings 4, 5, 7;
/// pre-flight I7, I8, I10, I11, M9, M10), through the document's own
/// model and tick (`m.external`, `m.idle()`).
@MainActor
@Suite struct ExternalEditModelTests {
    let clock = FakeClock()
    let probe = FakeProbe()
    let opener = FakeOpener()
    let watching = FakeWatching()
    let path = "/tmp/p.vizdaw/Media/Edits/Kick edit 1.wav"

    func model() throws -> (DocumentModel, FakeSession) {
        let f = editorFake()
        f.jobStartState = .running
        let m = try DocumentModel(session: f)
        m.externalTools = ExternalTools(opener: opener, watching: watching, probe: probe, now: { [clock] in clock.t })
        probe.stamps[path] = stamp(1000)
        return (m, f)
    }

    func stamp(_ size: UInt64) -> FileStamp { FileStamp(size: size, modified: clock.t) }

    /// The editor saves: a new stamp and its watch event.
    func save(_ size: UInt64) {
        probe.stamps[path] = stamp(size)
        watching.fire(path)
    }

    @Test func openWritesOpensAndWatches() throws {
        let (m, f) = try model()
        let app = URL(fileURLWithPath: "/Applications/Editor.app")
        #expect(try m.external.open(7, app: app) == path)
        #expect(f.externalOpened == [7])
        #expect(opener.opened == [FakeOpener.Opened(file: URL(fileURLWithPath: path), app: app)])
        #expect(watching.started == [path] && watching.live == 1 && m.external.watched == [7])
        clock.advance(5)
        m.idle()
        #expect(f.externalApplies.isEmpty, "nothing changed yet")
    }

    @Test func aStableChangeStartsOneImport() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        clock.advance(0.1)
        save(2000)
        clock.advance(0.5)
        m.idle()
        #expect(f.externalApplies.isEmpty, "not stable for a second yet")
        clock.advance(0.6)
        m.idle()
        #expect(f.externalApplies == [FakeSession.ExternalApply(event: 7, path: path)])
        #expect(m.external.importing == [7])
        #expect(m.banners.contains(.job(id: 5001, kind: .externalEdit, progress: 0.4)), "the job's banner")
        clock.advance(2)
        m.idle()
        m.idle()
        #expect(f.externalApplies.count == 1, "one import while it runs")
        f.jobRows[5001]?.state = .done
        m.idle()
        clock.advance(2)
        m.idle()
        #expect(f.externalApplies.count == 1, "one change is applied once")
        #expect(m.external.importing.isEmpty && m.alert == nil)
    }

    @Test func aSaveDuringAnImportIsAppliedAfterIt() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        save(2000)
        clock.advance(1)
        m.idle()
        #expect(f.externalApplies.count == 1)
        clock.advance(0.2)
        save(3000)
        clock.advance(1.5)
        m.idle()
        #expect(f.externalApplies.count == 1, "deferred while the import runs")
        f.jobRows[5001]?.state = .done
        m.idle()
        #expect(f.externalApplies.count == 2, "then picked up: stable for over a second")
        f.jobRows[5002]?.state = .done
        clock.advance(3)
        m.idle()
        #expect(f.externalApplies.count == 2)
    }

    @Test func reloadReChecks() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        #expect(m.external.reload(7) == .unchanged)
        probe.stamps[path] = stamp(2000)  // changed, no event, no wait
        #expect(m.external.reload(7) == .importing(5001))
        #expect(f.externalApplies.count == 1)
        #expect(m.external.reload(7) == .busy(ExternalEditModel.importingMessage))
        f.jobRows[5001]?.state = .done
        #expect(m.external.reload(7) == .unchanged, "the import's stamp is applied")
    }

    @Test func reloadRightAfterOpenIsUnchanged() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        watching.fire(path)  // the editor touched the file, nothing changed
        clock.advance(3)
        m.idle()
        #expect(m.external.reload(7) == .unchanged)
        #expect(f.externalApplies.isEmpty, "the copy Open wrote is not imported back")
    }

    @Test func reloadAfterReopenSaysTheCopyIsGone() throws {
        let (m, f) = try model()
        f.externalStates[7] = .gone
        #expect(m.external.reload(7) == .failed(ExternalEditModel.goneMessage))
        f.externalStates[7] = .noEdit
        #expect(m.external.reload(7) == .unavailable(ExternalEditModel.noEditMessage))
        f.externalStates[7] = .copy(row: ExternalEditRow(event: 7, path: path, from: 1))
        probe.stamps[path] = nil
        #expect(m.external.reload(7) == .failed(ExternalEditModel.goneMessage), "the file is not on disk")
        #expect(f.externalApplies.isEmpty && f.externalOpened.isEmpty)
    }

    @Test func closingStopsEveryWatch() throws {
        let (m, f) = try model()
        let other = "/tmp/p.vizdaw/Media/Edits/Snare edit 1.wav"
        try m.external.open(7, app: nil)
        f.externalRow = ExternalEditRow(event: 8, path: other, from: 2)
        probe.stamps[other] = stamp(500)
        try m.external.open(8, app: nil)
        #expect(watching.live == 2)
        m.stopExternalEdits()
        #expect(watching.live == 0 && m.external.watched.isEmpty)
        save(2000)
        clock.advance(2)
        m.idle()
        #expect(f.externalApplies.isEmpty)
        // Through the store: a document's close stops its watches (pre-flight I7(b)).
        let store = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        store.externalTools = ExternalTools(opener: opener, watching: watching, probe: probe, now: { [clock] in clock.t })
        let id = try #require(store.newDemo())
        let doc = try #require(store.model(id))
        try doc.external.open(5, app: nil)
        #expect(watching.live == 1)
        store.close(id)
        #expect(watching.live == 0)
    }

    @Test func openFailuresAreReported() throws {
        let (m, f) = try model()
        f.externalError = .Model(message: "Event 9 is not an audio region; select one audio region.")
        #expect(throws: VizError.self) { try m.external.open(9, app: nil) }
        f.externalError = nil
        opener.error = .Io(message: "/Applications/Nope.app is not an application.")
        #expect(throws: VizError.self) { try m.external.open(7, app: nil) }
        #expect(watching.started.isEmpty && opener.opened.isEmpty, "nothing is watched or opened")
        opener.error = nil
        opener.lateFailure = "The application could not open the file."
        try m.external.open(7, app: nil)
        #expect(m.alert?.title == ExternalEditModel.openFailedTitle)
        #expect(m.alert?.message == "The application could not open the file.")
        #expect(watching.live == 1, "the watch stays: the copy can be opened by hand")
    }

    @Test func aCancelledImportStaysArmed() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        save(2000)
        clock.advance(1)
        m.idle()
        f.jobRows[5001]?.state = .cancelled
        m.idle()
        clock.advance(3)
        m.idle()
        #expect(f.externalApplies.count == 1, "the cancelled change is not retried by itself")
        #expect(watching.live == 1 && m.alert == nil)
        clock.advance(0.1)
        save(3000)
        clock.advance(1)
        m.idle()
        #expect(f.externalApplies.count == 2, "the next save imports")
        f.jobRows[5002]?.state = .cancelled
        m.idle()
        #expect(m.external.reload(7) == .importing(5003), "Reload retries it")
    }

    @Test func aDeletedRegionStopsItsWatch() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        save(2000)
        clock.advance(1)
        m.idle()
        f.jobRows[5001] = JobRow(
            id: 5001, kind: .externalEdit, progress: 1, state: .failed,
            message: "The region was deleted; the edit was not applied.", problems: [], created: [])
        f.externalStates[7] = .noEdit  // the core forgot the copy
        m.idle()
        #expect(m.alert?.title == "External Edit Failed")
        #expect(m.alert?.message == "The region was deleted; the edit was not applied.")
        #expect(watching.live == 0 && m.external.watched.isEmpty)
        m.alert = nil
        save(3000)
        clock.advance(2)
        m.idle()
        #expect(f.externalApplies.count == 1 && m.alert == nil, "later saves raise nothing")
    }

    @Test func aBusyImportIsRetried() throws {
        let (m, f) = try model()
        try m.external.open(7, app: nil)
        f.applyError = .Busy(message: "Wait for the media job to finish.")
        save(2000)
        clock.advance(1)
        m.idle()
        #expect(f.externalApplies.isEmpty && m.alert == nil, "a busy core is no failure")
        f.applyError = nil
        m.idle()
        #expect(f.externalApplies.count == 1, "retried at the next tick")
        f.applyError = .Model(message: "This region has no external edit; use Open in External Editor.")
        f.jobRows[5001]?.state = .done
        m.idle()
        save(3000)
        clock.advance(1)
        m.idle()
        #expect(m.alert?.title == "External Edit Failed", "another refusal is an alert")
        m.alert = nil
        m.idle()
        #expect(m.alert == nil, "once")
    }
}
```

- [ ] **Step 2: Run them to see them fail.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh test
```
Expected: FAIL to compile — no `ExternalEditWatcher`, `ExternalEditModel`, `ExternalTools`.

- [ ] **Step 3: Implement** the three new files and the excerpts above.

- [ ] **Step 4: Run the tests and the gates.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo clippy -p core-ffi --lib -- -D warnings
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
```
Expected: PASS; Swift +17 (963), no new warnings.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/ExternalEditWatcher.swift app/Sources/VizDAWAppModel/ExternalEditModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAWAppModel/DocumentStore.swift app/Sources/VizDAW/ExternalEditing.swift app/Sources/VizDAW/VizDAWApp.swift app/Tests/VizDAWAppModelTests/ExternalEditWatcherTests.swift app/Tests/VizDAWAppModelTests/ExternalEditModelTests.swift
git commit -m "app: the external-edit watcher (1 s stable rule, replaced and deleted files), openers that launch nothing in tests, the per-document model and its wiring (plan 5f-3 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -10
```
Counts: Swift +17 (963).

---

### Task 5: Settings › Editing, the Region menu, keys, `region.open_external` and `region.reload_external`

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/tests/commands.rs`, `app/Sources/VizDAWAppModel/RegionCommands.swift`, `app/Sources/VizDAWAppModel/ControlCommands.swift` (`ControlHost.externalEditor`; the routing), `app/Sources/VizDAWAppModel/KeyBindings.swift` (`KeyDispatch.after`), `app/Sources/VizDAWAppModel/Nudge.swift` (`EditingPreferences.externalEditor`), `app/Sources/VizDAW/EditingSettingsView.swift`, `app/Sources/VizDAW/AppCommands.swift`, `app/Sources/VizDAW/ControlChannel.swift` (`AppControlHost.externalEditor`), `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift` (`FakeControlHost.externalEditor`)
- Test: create `app/Tests/VizDAWAppModelTests/ExternalCommandsTests.swift`

**Interfaces:**
- Consumes: `ExternalEditModel`, `ExternalEditorChoice`, `ExternalOutcome`, `ExternalTools`, `FakeOpener`, `FakeWatching` (Task 4); `ExternalEditorPanel` (Task 4, `VizDAW` target); `FakeSession`'s external stubs (Task 3).
- Produces:
  - `RegionCommands.openExternal = "region.open_external"`, `RegionCommands.reloadExternal = "region.reload_external"`, `RegionCommands.external`, in `ids`; `RegionCommands.noEditor`.
  - `RegionCommands.runExternal(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel, editor: ExternalEditorChoice) -> ControlOutcome` (the channel, keys, surfaces) and `RegionCommands.runMenu(_ id: String, _ m: DocumentModel, editor: ExternalEditorChoice)` (the menu).
  - `ControlHost.externalEditor: ExternalEditorChoice { get }`.
  - `EditingPreferences.externalEditor: String?` (key `EditingPreferences.externalEditorKey` = `"VizDAW.externalEditor"`).

In `registry.rs`, the parameter list goes after `NORMALIZE_PARAMS`:

Find in `crates/core-control/src/registry.rs`:
```rust
    help: "The peak level in dBFS, -96 to 0, as text (\"-0.3\"); answers the Normalize dialog.",
}];
```
Replace:
```rust
    help: "The peak level in dBFS, -96 to 0, as text (\"-0.3\"); answers the Normalize dialog.",
}];

/// `region.open_external` (plan 5f-3 Ruling 5).
const EXTERNAL_PARAMS: &[ParamSpec] = &[ParamSpec {
    name: "app",
    kind: ParamKind::Path("app"),
    required: false,
    help: "The editor application (an absolute path to a .app); default: Settings > Editing (answers the Choose External Editor dialog).",
}];
```

Find in `crates/core-control/src/registry.rs`:
```rust
    cmd("region.remove_fades", "Region", "Remove Fades", None, true),
```
Replace:
```rust
    cmd("region.remove_fades", "Region", "Remove Fades", None, true),
    CommandSpec {
        params: EXTERNAL_PARAMS,
        dialogs: &[DialogSpec {
            name: "Choose External Editor",
            when: "no editor is set in Settings > Editing",
            always: false,
            answered_by: &["app"],
        }],
        ..cmd(
            "region.open_external",
            "Region",
            "Open in External Editor",
            None,
            true,
        )
    },
    cmd(
        "region.reload_external",
        "Region",
        "Reload External Edit",
        None,
        true,
    ),
```

The count (pre-flight I9):

Find in `crates/core-control/tests/commands.rs`:
```rust
        145,
```
Replace:
```rust
        147,
```

Find in `crates/core-control/tests/commands.rs`:
```rust
5f-2 adds file.bounce"
```
Replace:
```rust
5f-2 adds file.bounce; 5f-3 adds region.open_external and region.reload_external"
```

The new core-control test, at the end of the file:

Find in `crates/core-control/tests/commands.rs`:
```rust
    assert!(message.contains("Bounce"), "{message}");
}
```
Replace:
```rust
    assert!(message.contains("Bounce"), "{message}");
}

/// Plan 5f-3 (Ruling 5): `region.open_external` takes an optional
/// absolute `.app` path and opens no dialog for a programmatic call;
/// `region.reload_external` takes nothing.
#[test]
fn external_commands_check_their_params() {
    let run = |id: &str, params: Value| json!({ "id": id, "params": params });
    for good in [
        json!({}),
        json!({ "app": "/Applications/X.app" }),
        json!({ "app": "/Applications/Sound Editor.app" }),
    ] {
        assert!(
            matches!(
                ok("command.run", run("region.open_external", good.clone())),
                Command::CommandRun { .. }
            ),
            "{good}"
        );
    }
    for bad in [
        json!({ "app": "X.app" }),
        json!({ "app": "/tmp/x.wav" }),
        json!({ "app": "/Applications/X" }),
        json!({ "app": 7 }),
        json!({ "app": "/tmp/x\u{0}.app" }),
        json!({ "app": format!("/{}.app", "a".repeat(1100)) }),
        json!({ "path": "/tmp/x.app" }),
    ] {
        let (code, _) = err("command.run", run("region.open_external", bad.clone()));
        assert_eq!(code, ErrorCode::InvalidParams, "{bad}");
    }
    assert!(matches!(
        ok("command.run", run("region.reload_external", json!({}))),
        Command::CommandRun { .. }
    ));
    let (code, _) = err(
        "command.run",
        run("region.reload_external", json!({ "app": "/Applications/X.app" })),
    );
    assert_eq!(code, ErrorCode::InvalidParams, "Reload takes no parameters");
    for id in ["region.open_external", "region.reload_external"] {
        let c = COMMANDS.iter().find(|c| c.id == id).unwrap();
        assert!(c.document && c.menu == "Region" && c.menu_item, "{id}");
        assert!(
            c.dialogs.iter().all(|d| !d.always),
            "{id}: a programmatic call never needs a dialog"
        );
    }
    let open = COMMANDS
        .iter()
        .find(|c| c.id == "region.open_external")
        .unwrap();
    assert_eq!(open.dialogs[0].name, "Choose External Editor");
    assert_eq!(open.dialogs[0].answered_by, ["app"]);
}
```

In `RegionCommands.swift`:

Find in `app/Sources/VizDAWAppModel/RegionCommands.swift`:
```swift
    public static let removeFades = "region.remove_fades"

    public static let ids: Set<String> = [normalize, fadeIn, fadeOut, removeFades]
```
Replace:
```swift
    public static let removeFades = "region.remove_fades"
    /// Open in External Editor and Reload External Edit (plan 5f-3): one
    /// region, through `runExternal` (the channel, keys, surfaces) or
    /// `runMenu` (the Region menu), never `run` (pre-flight I6).
    public static let openExternal = "region.open_external"
    public static let reloadExternal = "region.reload_external"
    public static let external: Set<String> = [openExternal, reloadExternal]

    public static let ids: Set<String> = [normalize, fadeIn, fadeOut, removeFades, openExternal, reloadExternal]

    /// Without a chosen editor, keys, surface buttons and the channel say
    /// this; only the menu item opens the chooser (owner answer 2026-11-03).
    public static let noEditor = "Choose an external editor in Settings › Editing."
    static let oneRegionMessage = "Select one audio region first."
```

Find in `app/Sources/VizDAWAppModel/RegionCommands.swift`:
```swift
        ids.contains(id) && !m.editor.selection.events.isEmpty && m.editor.gesture == nil
```
Replace:
```swift
        guard ids.contains(id), m.editor.gesture == nil else { return false }
        let n = m.editor.selection.events.count
        return external.contains(id) ? n == 1 : n > 0
```

Find in `app/Sources/VizDAWAppModel/RegionCommands.swift`:
```swift
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        let cmd: EditCommand =
```
Replace:
```swift
        guard m.editor.gesture == nil else { return .unavailable(EditorModel.gestureOpenMessage) }
        // Never an edit (pre-flight I6): these run through `runExternal`
        // or `runMenu`.
        if id == openExternal { return .unavailable("Use Region › Open in External Editor.") }
        if id == reloadExternal { return .unavailable("Use Region › Reload External Edit.") }
        let cmd: EditCommand =
```

Find in `app/Sources/VizDAWAppModel/RegionCommands.swift`:
```swift
extension RegionCommands: MenuCommands {}
```
Replace:
```swift
extension RegionCommands: MenuCommands {}

extension RegionCommands {
    /// The application `id` opens with: `app` when given, else the
    /// preference; the stub needs none. An application must be an
    /// existing folder (a bundle), checked before anything is written
    /// (pre-flight I11).
    enum EditorApp {
        case app(URL?)
        case refused(ControlFailure)
    }

    static func editorApp(_ arg: String?, _ editor: ExternalEditorChoice) -> EditorApp {
        let path: String
        if let arg {
            path = arg
        } else {
            switch editor {
            case .none: return .refused(.of(.unavailable, noEditor))
            case .stub: return .app(nil)
            case .app(let p): path = p
            }
        }
        var folder: ObjCBool = false
        guard FileManager.default.fileExists(atPath: path, isDirectory: &folder), folder.boolValue else {
            return .refused(.of(.failed, "\(path) is not an application."))
        }
        return .app(URL(fileURLWithPath: path))
    }

    /// The one selected event, or nil.
    static func oneRegion(_ m: DocumentModel) -> UInt64? {
        let events = m.editor.selection.events
        return events.count == 1 ? events.first : nil
    }

    /// `region.open_external` and `region.reload_external` (Ruling 7), also
    /// for keys and surface buttons: never a dialog. Open answers
    /// "editing <copy>"; Reload answers its import job (`.job`), or
    /// "unchanged".
    public static func runExternal(
        _ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel, editor: ExternalEditorChoice
    ) -> ControlOutcome {
        guard let region = oneRegion(m) else { return ControlCommands.unavailable(oneRegionMessage) }
        guard m.editor.gesture == nil else { return ControlCommands.unavailable(EditorModel.gestureOpenMessage) }
        if id == reloadExternal {
            switch m.external.reload(region, announce: false) {
            case .importing(let job): return .job(command: id, doc: doc, job: job)
            case .unchanged:
                m.editor.status = "unchanged"
                return .done(ControlCommands.ran(id, doc, m))
            case .unavailable(let why): return ControlCommands.unavailable(why)
            case .busy(let why): return .failed(.of(.busy, why))
            case .failed(let why): return .failed(.of(.failed, why))
            }
        }
        switch editorApp(args.text("app"), editor) {
        case .refused(let f):
            return .failed(f)
        case .app(let app):
            do {
                let path = try m.external.open(region, app: app)
                m.editor.status = "editing \(path)"
                return .done(ControlCommands.ran(id, doc, m))
            } catch {
                return .failed(.thrown(error, ExternalEditModel.openFailedTitle))
            }
        }
    }

    /// The Region menu's two items (Ruling 7): the app's menu action has
    /// already asked for an editor when none was set. Reload's import shows
    /// its banner and a failure's alert, as any job; a refusal is an alert
    /// titled for the item; `unavailable` goes to the status line.
    public static func runMenu(_ id: String, _ m: DocumentModel, editor: ExternalEditorChoice) {
        guard let region = oneRegion(m) else {
            m.editor.status = oneRegionMessage
            return
        }
        if id == reloadExternal {
            switch m.external.reload(region) {
            case .unchanged: m.editor.status = "The external edit is unchanged."
            case .failed(let why): m.alert = AlertItem(title: "Reload External Edit Failed", message: why)
            case .unavailable(let why), .busy(let why): m.editor.status = why
            case .importing: break
            }
            return
        }
        switch editorApp(nil, editor) {
        case .refused(let f):
            m.alert = AlertItem(title: ExternalEditModel.openFailedTitle, message: f.message)
        case .app(let app):
            do {
                try m.external.open(region, app: app)
            } catch {
                m.alert = AlertItem(error, title: ExternalEditModel.openFailedTitle)
            }
        }
    }
}
```

In `ControlCommands.swift`:

Find in `app/Sources/VizDAWAppModel/ControlCommands.swift`:
```swift
    var surfaces: any SurfaceCore { get }
}
```
Replace:
```swift
    var surfaces: any SurfaceCore { get }
    /// The editor `region.open_external` uses without `app` (plan 5f-3
    /// Ruling 5): the stub when `VIZDAW_EXTERNAL_EDITOR=stub`, else Settings
    /// › Editing's choice (pre-flight I7(c)).
    var externalEditor: ExternalEditorChoice { get }
}
```

Find in `app/Sources/VizDAWAppModel/ControlCommands.swift`:
```swift
        case RegionCommands.normalize:
            guard let text = args.text("peak_db") else { return .failed(.dialog("Normalize", "peak_db")) }
```
Replace:
```swift
        case RegionCommands.openExternal, RegionCommands.reloadExternal:
            return RegionCommands.runExternal(id, args: args, doc: docID, m, editor: host.externalEditor)
        case RegionCommands.normalize:
            guard let text = args.text("peak_db") else { return .failed(.dialog("Normalize", "peak_db")) }
```

In `KeyBindings.swift` (owner answer 2026-11-03; pre-flight M12):

Find in `app/Sources/VizDAWAppModel/KeyBindings.swift`:
```swift
            case .unavailable, .dialogRequired, .unknownId, .noDocument: return .pass
```
Replace:
```swift
            // No editor chosen (plan 5f-3, owner answer 2026-11-03): the
            // key or surface button says so; passed on, it would reach the
            // menu item, which opens the chooser.
            case .unavailable where f.message == RegionCommands.noEditor:
                doc?.editor.status = f.message
                return .handled
            case .unavailable, .dialogRequired, .unknownId, .noDocument: return .pass
```

In `Nudge.swift`:

Find in `app/Sources/VizDAWAppModel/Nudge.swift`:
```swift
    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        nudge =
            defaults.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(NudgeSettings.self, from: $0) }
            ?? NudgeSettings()
    }
```
Replace:
```swift
    /// The external sample editor's `.app` path (plan 5f-3 Ruling 5).
    public static let externalEditorKey = "VizDAW.externalEditor"

    /// The chosen external sample editor (nil: none chosen).
    public var externalEditor: String? {
        didSet { defaults.set(externalEditor, forKey: Self.externalEditorKey) }
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        nudge =
            defaults.data(forKey: Self.key).flatMap { try? JSONDecoder().decode(NudgeSettings.self, from: $0) }
            ?? NudgeSettings()
        externalEditor = defaults.string(forKey: Self.externalEditorKey)
    }
```

In `ControlChannel.swift`:

Find in `app/Sources/VizDAW/ControlChannel.swift`:
```swift
    var keys: KeyBindings { keyBindings }
```
Replace:
```swift
    var keys: KeyBindings { keyBindings }
    var externalEditor: ExternalEditorChoice {
        if ProcessInfo.processInfo.environment["VIZDAW_EXTERNAL_EDITOR"] == "stub" { return .stub }
        return editingPreferences.externalEditor.map { .app($0) } ?? .none
    }
```

In `ControlCommandsTests.swift`:

Find in `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`:
```swift
    var surfaces: any SurfaceCore = FakeSurfaceCore()
```
Replace:
```swift
    var surfaces: any SurfaceCore = FakeSurfaceCore()
    /// The editor `region.open_external` uses without `app` (plan 5f-3).
    var externalEditor: ExternalEditorChoice = .none
```

In `EditingSettingsView.swift`:

Find in `app/Sources/VizDAW/EditingSettingsView.swift`:
```swift
                .disabled(!scaleSteps.wrappedValue || prefs.nudge.followKeySignature)
            }
        }
        .formStyle(.grouped)
```
Replace:
```swift
                .disabled(!scaleSteps.wrappedValue || prefs.nudge.followKeySignature)
            }
            // Plan 5f-3 Ruling 5: applications only, chosen with a panel.
            Section("External Sample Editor") {
                LabeledContent("Application") {
                    Text(prefs.externalEditor.map { FileManager.default.displayName(atPath: $0) } ?? "None")
                }
                HStack {
                    Button("Choose…") {
                        if let p = ExternalEditorPanel.choose() { prefs.externalEditor = p }
                    }
                    Button("Clear") { prefs.externalEditor = nil }
                        .disabled(prefs.externalEditor == nil)
                }
            }
        }
        .formStyle(.grouped)
```

In `AppCommands.swift`, the Region menu's two explicit Buttons (pre-flight I6), after Remove Fades:

Find in `app/Sources/VizDAW/AppCommands.swift`:
```swift
            commandItem(RegionCommands.removeFades, RegionCommands.self)
```
Replace:
```swift
            commandItem(RegionCommands.removeFades, RegionCommands.self)
            Divider()
            Button(keyBindings.row(RegionCommands.openExternal)?.title ?? "Open in External Editor") {
                if let doc { openExternal(doc) }
            }
            .keyboardShortcut(KeyMenu.shortcut(RegionCommands.openExternal))
            .disabled(doc.map { !RegionCommands.enabled(RegionCommands.openExternal, $0) } ?? true)
            Button(keyBindings.row(RegionCommands.reloadExternal)?.title ?? "Reload External Edit") {
                if let doc {
                    RegionCommands.runMenu(
                        RegionCommands.reloadExternal, doc, editor: AppControlHost.shared.externalEditor)
                }
            }
            .keyboardShortcut(KeyMenu.shortcut(RegionCommands.reloadExternal))
            .disabled(doc.map { !RegionCommands.enabled(RegionCommands.reloadExternal, $0) } ?? true)
```

Find in `app/Sources/VizDAW/AppCommands.swift`:
```swift
    private func normalize(_ doc: DocumentModel) {
```
Replace:
```swift
    /// Region › Open in External Editor (plan 5f-3 Ruling 5): with no
    /// editor chosen, the panel asks first — the menu item alone (owner
    /// answer 2026-11-03); then the copy opens.
    private func openExternal(_ doc: DocumentModel) {
        if case .none = AppControlHost.shared.externalEditor {
            guard let app = ExternalEditorPanel.choose() else { return }
            editingPreferences.externalEditor = app
        }
        RegionCommands.runMenu(RegionCommands.openExternal, doc, editor: AppControlHost.shared.externalEditor)
    }

    private func normalize(_ doc: DocumentModel) {
```

- [ ] **Step 1: Write the failing tests.** The core-control test is the `commands.rs` excerpt above (with the count change). Create `app/Tests/VizDAWAppModelTests/ExternalCommandsTests.swift`:

```swift
import Foundation
import Testing
import VizDAWCore

@testable import VizDAWAppModel

/// `region.open_external` and `region.reload_external`, the menu items,
/// the keys and the editor preference (plan 5f-3 Task 5; Rulings 5, 7;
/// pre-flight I6, I7, I10, I11, M12; owner answers 2026-11-03).
@MainActor
@Suite struct ExternalCommandsTests {
    let host = FakeControlHost()
    let opener = FakeOpener()
    let watching = FakeWatching()
    let probe = FakeProbe()
    let clock = FakeClock()
    let path = "/tmp/p.vizdaw/Media/Edits/Kick edit 1.wav"

    /// A store with one demo document, given fake tools; jobs stay running.
    func demo() throws -> (DocumentStore, UUID, DocumentModel, FakeSession) {
        let f = demoFake()
        f.jobStartState = .running
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { f }, open: { _ in demoFake() })
        s.externalTools = ExternalTools(opener: opener, watching: watching, probe: probe, now: { [clock] in clock.t })
        let id = try #require(s.newDemo())
        let m = try #require(s.model(id))
        probe.stamps[path] = FileStamp(size: 1000, modified: clock.t)
        return (s, id, m, f)
    }

    func run(_ id: String, _ args: [ControlArg], _ s: DocumentStore, _ doc: UUID) -> ControlOutcome {
        ControlCommands.run(id, args: args, doc: s.model(doc).map { (doc, $0) }, store: s, host: host)
    }

    func run(_ id: String, _ s: DocumentStore, _ doc: UUID) -> ControlOutcome { run(id, [], s, doc) }

    func app(_ path: String) -> [ControlArg] { [ControlArg(name: "app", value: .text(value: path))] }

    func select(_ m: DocumentModel, _ events: [UInt64]) {
        m.editor.setSelection(SelectionRow(timeRange: nil, lanes: [], events: events))
    }

    func failure(_ o: ControlOutcome) -> ControlFailure? {
        if case .failed(let f) = o { return f }
        return nil
    }

    func status(_ o: ControlOutcome) -> String? {
        if case .done(.ran(_, _, _, _, let status)) = o { return status }
        return nil
    }

    /// A temporary folder, removed by the caller.
    func folder() throws -> URL {
        let d = FileManager.default.temporaryDirectory.appendingPathComponent("vizdaw-ext-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
        return d
    }

    @Test func externalCommandsCheckTheirArguments() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        for events in [[], [5, 6]] as [[UInt64]] {
            select(m, events)
            for c in [RegionCommands.openExternal, RegionCommands.reloadExternal] {
                let e = try #require(failure(run(c, s, id)))
                #expect(e.code == .unavailable && e.message == "Select one audio region first.", "\(c) \(events)")
            }
        }
        // A MIDI note: the core's refusal.
        select(m, [9])
        f.externalError = .Model(message: "Event 9 is not an audio region; select one audio region.")
        var e = try #require(failure(run(RegionCommands.openExternal, s, id)))
        #expect(e.code == .failed && e.message.contains("not an audio region"))
        f.externalError = nil
        select(m, [5])
        // No editor, no `app`, no stub: the Settings message, no panel.
        host.externalEditor = .none
        e = try #require(failure(run(RegionCommands.openExternal, s, id)))
        #expect(e.code == .unavailable && e.message == RegionCommands.noEditor)
        // An `app` that is missing or a plain file: refused before anything is written.
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("Plain.app").path
        #expect(FileManager.default.createFile(atPath: file, contents: Data()))
        host.externalEditor = .stub
        for bad in ["/Applications/Nope-\(UUID().uuidString).app", file] {
            e = try #require(failure(run(RegionCommands.openExternal, app(bad), s, id)))
            #expect(e.code == .failed && e.message == "\(bad) is not an application.", "\(bad)")
        }
        // Reload of a region with no edit.
        e = try #require(failure(run(RegionCommands.reloadExternal, s, id)))
        #expect(e.code == .unavailable && e.message == ExternalEditModel.noEditMessage)
        #expect(f.externalOpened == [9], "only the note reached the core")
        #expect(opener.opened.isEmpty && watching.started.isEmpty && f.externalApplies.isEmpty)
    }

    @Test func openAnswersWithTheCopysPath() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        select(m, [5])
        #expect(status(run(RegionCommands.openExternal, s, id)) == "editing \(path)")
        #expect(f.externalOpened == [5])
        #expect(opener.opened == [FakeOpener.Opened(file: URL(fileURLWithPath: path), app: nil)])
        #expect(m.external.watched == [5])
        // `app` wins over the preference; a folder ending in .app passes.
        let dir = try folder()
        defer { try? FileManager.default.removeItem(at: dir) }
        let editor = dir.appendingPathComponent("Editor.app")
        try FileManager.default.createDirectory(at: editor, withIntermediateDirectories: true)
        host.externalEditor = .app("/Applications/Other.app")
        #expect(status(run(RegionCommands.openExternal, app(editor.path), s, id)) == "editing \(path)")
        #expect(opener.opened.last?.app == URL(fileURLWithPath: editor.path))
    }

    @Test func reloadWithNothingChangedAnswersUnchanged() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        select(m, [5])
        _ = run(RegionCommands.openExternal, s, id)
        #expect(status(run(RegionCommands.reloadExternal, s, id)) == "unchanged")
        #expect(f.externalApplies.isEmpty)
    }

    @Test func reloadWithAChangeIsAJob() throws {
        let (s, id, m, f) = try demo()
        host.externalEditor = .stub
        select(m, [5])
        _ = run(RegionCommands.openExternal, s, id)
        probe.stamps[path] = FileStamp(size: 2000, modified: clock.t)
        guard case .job(let command, let doc, let job) = run(RegionCommands.reloadExternal, s, id) else {
            Issue.record("Reload did not start a job")
            return
        }
        #expect(command == RegionCommands.reloadExternal && doc == id && job == 5001)
        #expect(f.externalApplies == [FakeSession.ExternalApply(event: 5, path: path)])
        #expect(m.jobs.isEmpty, "the channel answers this job: no banner, no second report")
        let e = try #require(failure(run(RegionCommands.reloadExternal, s, id)))
        #expect(e.code == .busy && e.message == ExternalEditModel.importingMessage)
    }

    @Test func theMenuItemsAreEnabledForOneRegion() throws {
        let (_, _, m, f) = try demo()
        for c in RegionCommands.external {
            select(m, [])
            #expect(!RegionCommands.enabled(c, m))
            select(m, [5])
            #expect(RegionCommands.enabled(c, m))
            select(m, [5, 6])
            #expect(!RegionCommands.enabled(c, m))
            // Never an edit (pre-flight I6).
            select(m, [5])
            if case .unavailable = RegionCommands.run(c, m) {} else { Issue.record("\(c) ran as an edit") }
        }
        #expect(f.edits.isEmpty && f.externalOpened.isEmpty)
        select(m, [5, 6])
        #expect(RegionCommands.enabled(RegionCommands.removeFades, m), "the others take several regions")
    }

    @Test func aKeyWithNoEditorShowsTheSettingsMessage() throws {
        let (s, id, m, _) = try demo()
        select(m, [5])
        host.externalEditor = .none
        let o = run(RegionCommands.openExternal, s, id)
        #expect(KeyDispatch.after(o, doc: m) == .handled, "used up: passed on, it would reach the chooser")
        #expect(m.editor.status == RegionCommands.noEditor)
        #expect(opener.opened.isEmpty)
        select(m, [])
        #expect(KeyDispatch.after(run(RegionCommands.removeFades, s, id), doc: m) == .pass, "others still pass")
    }

    @Test func theMenuItemsReportInTheWindow() throws {
        let (_, _, m, f) = try demo()
        select(m, [5])
        RegionCommands.runMenu(RegionCommands.reloadExternal, m, editor: .stub)
        #expect(m.editor.status == ExternalEditModel.noEditMessage)
        RegionCommands.runMenu(RegionCommands.openExternal, m, editor: .stub)
        #expect(f.externalOpened == [5] && m.external.watched == [5] && m.alert == nil)
        RegionCommands.runMenu(RegionCommands.reloadExternal, m, editor: .stub)
        #expect(m.editor.status == "The external edit is unchanged.")
        probe.stamps[path] = FileStamp(size: 2000, modified: clock.t)
        RegionCommands.runMenu(RegionCommands.reloadExternal, m, editor: .stub)
        #expect(m.jobs.map(\.kind) == [.externalEdit], "the menu's import shows its banner")
        f.externalError = .Io(message: "Media/Edits is a file, not a folder; move it away and try again.")
        RegionCommands.runMenu(RegionCommands.openExternal, m, editor: .stub)
        #expect(m.alert?.title == ExternalEditModel.openFailedTitle)
    }

    @Test func theEditorPreferenceIsKept() {
        let d = MemoryDefaults()
        let p = EditingPreferences(defaults: d)
        #expect(p.externalEditor == nil)
        p.externalEditor = "/Applications/Editor.app"
        #expect(d.string(forKey: EditingPreferences.externalEditorKey) == "/Applications/Editor.app")
        #expect(EditingPreferences(defaults: d).externalEditor == "/Applications/Editor.app")
        p.externalEditor = nil
        #expect(EditingPreferences(defaults: d).externalEditor == nil)
    }
}
```

- [ ] **Step 2: Run them to see them fail.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-control --test commands
scripts/swift.sh test
```
Expected: FAIL — the count is 145 and the IDs are unknown; Swift fails to compile (`runExternal`, `externalEditor`, `noEditor`).

- [ ] **Step 3: Implement** the excerpts above. `ControlCommands.handled` already unions `RegionCommands.ids`, so `everyRegisteredMenuCommandHasAHandler` stays green; `RegionCommandsTests.everyCommandNeedsSelectedEvents` (every ID in `ids`, no selection: "Select audio regions first.") stays green because `run` checks the selection before the new guard.

- [ ] **Step 4: Run the tests and the gates.**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo test -p core-control
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
```
Expected: PASS; core-control +1, Swift +8 (971), registry 147.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs app/Sources/VizDAWAppModel/RegionCommands.swift app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAWAppModel/KeyBindings.swift app/Sources/VizDAWAppModel/Nudge.swift app/Sources/VizDAW/EditingSettingsView.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/ControlChannel.swift app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift app/Tests/VizDAWAppModelTests/ExternalCommandsTests.swift
git commit -m "app, control: Settings > Editing external sample editor, Open in External Editor and Reload External Edit on the menu, keys and channel (plan 5f-3 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -13
```
Counts: Rust +1 (1890 / 16 ignored on macOS), Swift +8 (971), registry 147.

---

### Task 6: The e2e through the stub editor, README 185–188, the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`

**Interfaces:**
- Consumes: everything above; the e2e's `App::start_with`, `audio_file`, `wait_for_label_like`, `wait_for_undo`, `wait_for_state` (all at `9c2606a`); `core_media::export::{PcmWriter, ExportFormat, Container}` (a dev-dependency already).

The steps of the e2e (pre-flight C1, I8, M11):
1. Start with `VIZDAW_EXTERNAL_EDITOR=stub` (the app uses `StubOpener` and the channel `.stub`). `file.create_demo`. Hostile, before any region is selected: `region.open_external` is `unavailable`; with `{"app": "relative.app"}` it is `invalid_params`.
2. Import `tone.caf` (`audio_file`, 96 000 frames) at tick 0 into the shown sequence (`media.import`), then select its region by its element (`wait_for_label_like` "Region tone on " … "gain 0.0 dB", `ui.click`): the demo's own audio is offline, as `regions_normalize_relink_and_trash` does.
3. `region.open_external` answers `editing <path>`: the untitled demo's scratch package in the throwaway home, `…/Media/Edits/tone edit 1.wav`, a file. `region.reload_external` right after is `unchanged`.
4. The "editor" saves by replacing the file: a mono 48 kHz 16-bit WAV of 24 000 frames at 0.5 written as `.tone edit 1.tmp.wav` beside the copy with `PcmWriter`, then renamed over it. Wait for `undo == "External Edit"` and a media item with origin `external_edit` and 24 000 frames.
5. A second save, in place on the file the rename put there (open, truncate, write a 12 000-frame WAV's bytes; no rename): wait for a second `external_edit` item of 12 000 frames. Only a watch re-armed on the new file sees this write (Review Focus 1; pre-flight I8).
6. `region.reload_external` answers `unchanged`; `region.open_external` with an `app` that does not exist is `failed`.

Find in `crates/vizctl/tests/e2e.rs`:
```rust
        "the app's check (pre-flight M6): {e:?}"
    );
    app.quit();
}
```
Replace:
```rust
        "the app's check (pre-flight M6): {e:?}"
    );
    app.quit();
}

/// A 48 kHz mono 16-bit WAV of `frames` frames at `level`: an editor's
/// save (plan 5f-3).
fn wav(path: &std::path::Path, frames: usize, level: f32) {
    let fmt = core_media::export::ExportFormat {
        container: core_media::export::Container::Wav,
        depth: core_model::format::BitDepth::Int16,
        rate: 48_000,
        dither: false,
        dither_stream: 0,
    };
    let mut w = core_media::export::PcmWriter::create(path, &fmt, 1).unwrap();
    w.write(&vec![level; frames]).unwrap();
    w.finish().unwrap();
}

/// README steps 185–188 through the channel (plan 5f-3; spec §11, §12.1):
/// with `VIZDAW_EXTERNAL_EDITOR=stub`, which launches nothing, a region of
/// an imported tone opens in the "editor"; the test saves the copy as an
/// editor that replaces its file would, and the region plays the edit as
/// one "External Edit" step; a second save in place, on the file the
/// rename put there, lands too, which pins the watch re-armed on the
/// replaced file (pre-flight I8); Reload with nothing changed is
/// "unchanged"; hostile calls are refused.
#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn edits_a_region_through_the_stub_editor() {
    let mut app = App::start_with(&[("VIZDAW_EXTERNAL_EDITOR", "stub")]);
    let home = app.launched.home.clone();
    let tone = home.join("tone.caf");
    audio_file(&tone, 96_000);
    let demo = app.call("command.run", json!({"id": "file.create_demo"}));
    let w = demo["window"].clone();

    // 1. Nothing selected: refused; a relative `app`: refused by type.
    let e = app.call_err(
        "command.run",
        json!({"id": "region.open_external", "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::Unavailable, "{e}");
    let e = app.call_err(
        "command.run",
        json!({"id": "region.open_external", "params": {"app": "relative.app"}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::InvalidParams, "{e}");

    // 2. The tone at bar 1 (the demo's own audio is offline: pre-flight
    // C1), its region selected by its element.
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    let seq = doc["selected_seq"].as_u64().unwrap();
    let r = app.call(
        "command.run",
        json!({"id": "media.import", "params": {"path": tone, "seq": seq, "tick": 0}, "window": w}),
    );
    assert_eq!(r["undo"], "Import Audio", "{r}");
    let (region, label) = wait_for_label_like(&mut app, &w, "Region tone on ", "gain 0.0 dB");
    app.call("ui.click", json!({"window": w, "element": region}));
    let sel = app.call("state.get", json!({"target": "selection", "window": w}));
    assert_eq!(sel["events"].as_array().unwrap().len(), 1, "{label}: {sel}");

    // 3. Open: the copy's path, in the scratch package in the home.
    let r = app.call(
        "command.run",
        json!({"id": "region.open_external", "window": w}),
    );
    let status = r["status"].as_str().unwrap_or("").to_string();
    let copy = PathBuf::from(
        status
            .strip_prefix("editing ")
            .unwrap_or_else(|| panic!("{r}")),
    );
    assert!(
        copy.starts_with(&home) && copy.ends_with("Media/Edits/tone edit 1.wav"),
        "{r}"
    );
    assert!(copy.is_file(), "{}", copy.display());
    let r = app.call(
        "command.run",
        json!({"id": "region.reload_external", "window": w}),
    );
    assert_eq!(r["status"], "unchanged", "right after Open (pre-flight I10): {r}");

    // 4. The editor saves by replacing the file (pre-flight M11).
    let tmp = copy.with_file_name(".tone edit 1.tmp.wav");
    wav(&tmp, 24_000, 0.5);
    std::fs::rename(&tmp, &copy).unwrap();
    wait_for_undo(&mut app, &w, "External Edit");
    let edited = |m: &Value, frames: u64| {
        m["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["origin"] == "external_edit" && i["frames"] == frames)
    };
    wait_for_state(&mut app, &w, "media", |m| edited(m, 24_000));

    // 5. A second save, in place on the file the rename put there: it
    // lands only if the watch followed the rename (pre-flight I8).
    let bytes = {
        let t = home.join("short.wav");
        wav(&t, 12_000, 0.25);
        std::fs::read(&t).unwrap()
    };
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&copy)
            .unwrap();
        f.write_all(&bytes).unwrap();
        f.sync_all().unwrap();
    }
    let m = wait_for_state(&mut app, &w, "media", |m| edited(m, 12_000));
    let edits = m["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["origin"] == "external_edit")
        .count();
    assert_eq!(edits, 2, "one import per save: {m}");
    let doc = app.call("state.get", json!({"target": "document", "window": w}));
    assert_eq!(doc["undo"], "External Edit", "{doc}");

    // 6. Reload with nothing changed since; an `app` that is no application.
    let r = app.call(
        "command.run",
        json!({"id": "region.reload_external", "window": w}),
    );
    assert_eq!(r["status"], "unchanged", "{r}");
    let e = app.call_err(
        "command.run",
        json!({"id": "region.open_external", "params": {"app": "/Applications/Nope-VizDAW-e2e.app"}, "window": w}),
    );
    assert_eq!(e.code, core_control::ErrorCode::Failed, "{e}");
    app.quit();
}
```

- [ ] **Step 1: Write the e2e** (the excerpt above).

- [ ] **Step 2: Write README 185–188.**

Find in `app/README.md`:
```markdown
     A name ending in your own digits keeps them: a taken "Kick 808.wav"
     becomes "Kick 808 2.wav".
```
Replace:
```markdown
     A name ending in your own digits keeps them: a taken "Kick 808.wav"
     becomes "Kick 808 2.wav".

### External sample editor (plan 5f-3)

By hand; the watcher is tested (`ExternalEditWatcherTests`), the core
(`core-ffi/tests/external.rs`) and the e2e
`edits_a_region_through_the_stub_editor`.

185. Settings › Editing › External Sample Editor › Choose… lists your
     applications; choose a sample editor. Clear forgets it. With none
     chosen, Region › Open in External Editor asks for one first; a key
     bound to it (or a control-surface button) says "Choose an external
     editor in Settings › Editing." in the status line and opens nothing.
186. The manual checklist's "external editor round trip": select one
     audio region, Region › Open in External Editor: the editor opens
     "<name> edit 1.wav". Trim it shorter and save: about a second later
     the region plays the edit, Edit shows "Undo External Edit", and the
     media pool lists the edit as External Edit. Other regions using the
     original sound as before. Undo puts the original back.
187. Save again in the editor, twice quickly: one more "External Edit"
     step, not two. Region › Reload External Edit with nothing changed
     says the edit is unchanged. Delete the region and save in the editor:
     the alert says the region was deleted, and later saves say nothing.
188. Close the project: the edit copies go to the Trash (with the other
     unused files); reopen it: Reload External Edit says the copy is gone.
     An untitled project's copies go to the Trash too, when it closes or
     is first saved. After Save As to a new name, the copies stay in the
     old package, still watched, until that package is next saved or
     closed.
```

- [ ] **Step 3: Run the full gates**, including the rt-check gates, the workspace tests, Swift, `scripts/make-app.sh` and the e2e (13 tests; run them all, not only the new one).

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all && cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p core-ffi --lib -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures
scripts/build-ffi.sh && scripts/swift.sh build && scripts/swift.sh test
scripts/make-app.sh
VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1
pgrep -lf MacOS/VizDAW; ls -d /tmp/vze* 2>/dev/null
```
Expected: Rust 1890 passed / 17 ignored (macOS), Swift 971, e2e 13 passed, registry 147; `pgrep` and `ls` print nothing.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl/tests/e2e.rs app/README.md
git commit -m "e2e: an external edit through the stub editor, replaced and then rewritten in place; README 185-188 (plan 5f-3 Task 6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -4
```
Counts: ignored +1 (17), e2e 13.

---

## Self-Review

1. **Spec coverage.** Every requirement of §11 has a task:
   - the Settings chooser (Task 5);
   - the WAV of the span with no gain or fades, in the project rate and depth, at `Media/Edits/<name> edit <n>.wav` (Tasks 2–3);
   - opening (Tasks 4–5);
   - the `DispatchSource` watch for write, rename and delete (Task 4; pinned in the built app by Task 6 step 5);
   - the 1 s stable rule (Task 4);
   - the import to CAF with `ExternalEdit { from }` (Task 3);
   - that region alone, offset 0, new length, fades clamped, as one "External Edit" step (Tasks 1, 3);
   - other regions unaffected (Task 3);
   - watching until close (Tasks 3–4);
   - Reload, and its "gone" after a reopen (Tasks 3–5);
   - retention, Trash at close and at an untitled Save As (Task 3).

   Also covered: §12.1 `region.open_external` with the stub (Tasks 5–6), and §12.3's watcher test with temp files (Task 4). No gaps.
2. **Step scan.** Every step names its files, its code and the spec values. Every test of Tasks 1–5 is given whole; the e2e is given whole.
3. **Type consistency.**
   - `ExternalEditRow` and `ExternalEditStateRow` (Task 3) are used by `ExternalEditModel.open`/`reload` (Task 4) and the channel answers (Task 5).
   - `Work::ExternalEdit` and `JobKindRow::ExternalEdit` (Task 3) map to `.externalEdit` in Swift (`ProjectMedia.swift`, `FakeSession`).
   - The message strings agree across Rust and Swift: `external::GONE` = `ExternalEditModel.goneMessage`, `external::NO_EDIT` = `ExternalEditModel.noEditMessage`, `external::REGION_DELETED` (the alert of `aDeletedRegionStopsItsWatch`), `RegionCommands.noEditor` (the channel, keys, `KeyDispatch`).
   - `FileStamp` and `FileProbe` are shared by the watcher and the model; `FakeClock`/`FakeProbe` by the three Swift test files.
4. **Review Focus.** Each of the five lines names its tests in their tasks.
5. **Find excerpts.** Each occurs exactly once in its file at `9c2606a`; none overlaps another.
6. **Proportion.** The rules (stable file, names, clamping, retention, the untitled Trash, Reload's answers) are written once in the Rulings; the tasks give the code.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-02-external-editor.md`. The controller runs it subagent-driven after 5f-2 under the standing authorization, one reviewer per task; Task 3 is the largest. The final review should replay Review Focus 1 with a real editor (README 186–187).
