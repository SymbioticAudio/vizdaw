# VizDAW 5f-3 — External Sample Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** "Open in External Editor" on one audio region works like this:
- VizDAW writes the region's span (no gain, no fades) as a WAV in the project's rate and depth to `Media/Edits/<name> edit <n>.wav`.
- It opens that file in the sample editor chosen in Settings › Editing.
- It watches the file. Once the file's size and modification time have stayed the same for 1 s after a change, the file is imported (converted to CAF in the project format, origin `ExternalEdit { from }`).
- That region alone is relinked to the import (offset 0, the new length, fades clamped), as one undo step "External Edit".
- "Reload External Edit" re-checks the file by hand.
- The control channel's `region.open_external` and `region.reload_external` do the same without launching anything when the stub editor is configured.

**Architecture:**
- **`core-model`:** `AudioCmd::SetSource { event, media, source_offset, length }` (fades clamped).
- **`core-ffi`:**
  - `external.rs`:
    - writing the span with 5f-2's `core_media::export::PcmWriter`, through a partial file and an exclusive rename;
    - the session's live edit copies (event → copy, origin media), which retention keeps until the project closes;
    - `Session::open_external`, `external_edit`, `apply_external_edit` (an `ExternalEdit` job importing through the import path, generalised in 5f-2 Task 4, with one undo step "External Edit").
- **Swift:**
  - `ExternalEditWatcher`: a `DispatchSource` file-system-object watch (write, rename, delete) behind a `FileWatching` protocol, and the 1 s stable rule driven by an injected clock and a `FileProbe`, tested with temp files.
  - `ExternalOpener`: `NSWorkspace` live; a stub (`VIZDAW_EXTERNAL_EDITOR=stub`) and a fake that launch nothing.
  - `ExternalEditModel` per document.
  - Settings › Editing "External sample editor"; the Region menu's two items.
- **Channel, e2e and README:** `region.open_external { app? }` and `region.reload_external`; the e2e "edit a region through the stub editor"; README 185–188.

**Tech Stack:** Rust 2024 (`core-model`, `core-media`, `core-ffi` with UniFFI 0.32.2, `core-control`, `vizctl`), Swift 6 / SwiftPM (Swift Testing; `Dispatch`, `AppKit` in the app target only), macOS 14+.

**Spec:** `docs/superpowers/specs/2026-10-18-recording-audio-design.md`, §13 row "5f-3 — External sample editor" (split from the former 5f row by plan 5f-1 Ruling 1). This plan delivers:
- §11 in full: the Settings chooser, the WAV of the span in the project format, opening, watching, the stable rule, the import as `ExternalEdit`, the one-region relink as "External Edit", watching until close, "Reload External Edit", and the copies under the retention rule;
- §12.1 `region.open_external` (on CI it uses a configured stub editor command);
- §12.3 "the external-edit watcher's stable-file rule (temp files)";
- spec Rulings 26 and 32 and the manual checklist's "external editor round trip" (README 186).

Read the whole spec (§3 binds), plan 5f-2 (its `core_media::export` and `ImportAs` are consumed here), and plan 5b's retention rulings (`docs/superpowers/plans/2026-10-19-regions-in-editors.md`).

## Global Constraints

- **Shell setup.**
  - Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`. Every bash block starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
  - A `git commit` without `DEVELOPER_DIR` fails silently: check every commit with `git log -1 --oneline` and `git show --stat HEAD`.
- **Swift only through the scripts, from the repo root** (`scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`, `scripts/make-app.sh`). Never call `swift`, `xcodebuild` or `xcrun` directly.
- **Branch and commits.**
  - Work on branch `sp5-audio` after plan 5f-2. Never switch branches, never push.
  - Commit at the end of every task, adding only the files the task lists, by path (never `git add -A`). No manifest changes: `Cargo.lock` never changes.
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
    - Swift tests use `FakeOpener`, which records and launches nothing.
    - The e2e sets `VIZDAW_EXTERNAL_EDITOR=stub`, which makes the app use `StubOpener`. It records the path and launches nothing; the test edits the file itself.
    - No test calls `NSWorkspace`.
  - **Programmatic and control-channel calls never open dialogs.** `region.open_external` without a configured editor, and without `app` and the stub, fails with `unavailable` ("Choose an external sample editor in Settings › Editing, or pass `app`"); it never shows a panel.
  - Launch VizDAW only through the e2e (Task 5) with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing and `ls -d /tmp/vze*` prints nothing new.
  - Tests never open audio input or MIDI ports and never trigger the microphone prompt: every `core-ffi` test calls `force_offline()` through its `serial` helper.
  - Never touch the real `~/Library/Application Support/VizDAW`, the real preferences, the real Trash or any device. Sessions that save or close get `Session::debug_set_trash_dir`, and Swift tests use `MemoryDefaults()` and temp folders.
- **Test waits.**
  - `core-ffi` job waits poll with `s.idle()` under `common::wait_limit()` (`VIZDAW_TEST_WAIT_SECS`).
  - The watcher's 1 s rule is tested with an injected clock, never by sleeping a second. The temp-file tests write, then advance the fake clock.
  - The e2e polls with its `WAIT_MS` conditions.
- **Crates and dependencies.** Rust stable, edition 2024, license GPL-3.0-only. No new crate and no new dependency (Swift uses Foundation, Dispatch and AppKit only).
- **Lints** as in every plan: `#![deny(missing_docs)]`, plus no unwrap, expect, indexing or panic outside tests.
- **Platform-neutral cores.** `core-model`, `core-media` and `core-control` stay free of OS APIs.
- **Linux CI.**
  - CI runs clippy and the tests on `ubuntu-latest`.
  - Applying an edit imports a WAV, which only the macOS system importer reads (the CAF importer reads CAF). So the `core-ffi` tests that apply an edit are `#[cfg(target_os = "macos")]`, with the note "// macOS only: importing a WAV goes through ExtAudioFile (the CAF importer reads CAF only)", as in `crates/core-ffi/tests/media_pool.rs`.
  - Writing the span, the copies, retention and the hostile-input tests run everywhere.
  - Every task that changes Rust checks `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- **TDD.** Every new test is run and seen to fail (RED) before the code that makes it pass.
- **Real-time rules.** Nothing here runs on the real-time thread. The relinked region reaches the engine through the existing document reload. No rt-check gate is needed beyond the final run in Task 5, which confirms nothing moved.
- **Media is write-once.**
  - The edit copy is a new file, never written over: `<name> edit <n>.wav` takes the first free `n`, by an exclusive create through `.<name>.partial`.
  - The edit is imported into a new CAF; the original media item and its file are never changed.
  - Other regions using the original keep it.
- **File compatibility.** `core_model::file::VERSION` stays 3. `MediaOrigin::ExternalEdit { from }` exists since 5a. No model field is added; edit copies are runtime state (Ruling 3).
- **FFI safety.** Every new export returns `Result<_, VizError>` inside `ffi_guard`.
- **UniFFI changes** (Task 2):
  - New `ExternalEditRow` and `JobKindRow::ExternalEdit`.
  - New `Session` methods `open_external`, `external_edit` and `apply_external_edit`.
  - `FakeSession` implements them. `ProjectMedia.swift`'s `JobKindRow` switches gain `.externalEdit`.
- **Swift.** Generated bindings in Swift 5 mode; the targets and tests in Swift 6 mode with no new warnings. `DispatchSource` and `NSWorkspace` live only in the `VizDAW` target (`LiveFileWatching`, `WorkspaceOpener`); `VizDAWAppModel` holds the protocols, the rule and the fakes.
- **Control channel.**
  - The registry count moves from 144 (after 5f-2) to 146 in Task 4.
  - Hostile-input tests: `external_commands_check_their_params` (core-control), `externalCommandsCheckTheirArguments` (app), `hostile_external_calls_are_refused` (core).
- **Generated files** are never committed.
- **Each task ends green.**
  - Always: `cargo fmt --all` then `cargo fmt --all -- --check`, both clippies, and `cargo test` for the crates touched (the workspace in Tasks 2 and 5).
  - Tasks 2–5 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Task 5 also runs `scripts/make-app.sh`, the e2e and the rt-check gates (`cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings`; `cargo test -p core-engine --features rt-check --test rt_alloc --test rt_alloc_graph --test rt_alloc_record --test rt_alloc_gestures`).
  - Baseline: what plan 5f-2 Task 7 measured. Report the measured counts.
- **How code is given.** As in plans 5f-1 and 5f-2.
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

1. **Editors that save by replacing the file** (write a temp file, rename it over, or delete and recreate), save twice in a second, or save while VizDAW applies the last change.
   - Expected:
     - a rename or delete re-arms the watch on the new file at the same path;
     - only a file that has been stable for 1 s is imported;
     - a second save within that second restarts the wait;
     - one edit makes one undo step;
     - a save during a running import is picked up after it.
   - Pinned by `a_rename_over_the_file_rearms_the_watch`, `a_second_save_restarts_the_second`, `one_change_is_applied_once` and `a_save_during_an_import_is_applied_after_it` (Task 3).
2. **What the editor hands back.** A different length; a different rate; mono instead of stereo; more than two channels; an empty or truncated file; a file deleted and never recreated.
   - Expected:
     - the region takes the new length, with its fades clamped (fade in first, then fade out within what is left);
     - another rate is converted to the project rate;
     - mono or stereo is accepted;
     - more than two channels, an empty file or an unreadable file is refused with a clear message: the document is unchanged, and the imported file is removed;
     - a deleted file stays watched for its return.
   - Pinned by `fades_are_clamped_to_the_new_length` (Task 1), `a_shorter_mono_edit_relinks_the_region` and `an_unreadable_or_wide_edit_changes_nothing` (Task 2), and `a_deleted_file_waits_for_its_return` (Task 3).
3. **The region or project changes meanwhile.** The region is deleted, split or moved while the editor is open; the project is saved under a new name; the project closes; Undo of the "External Edit".
   - Expected:
     - a deleted region's edit is dropped with the message "The region was deleted; the edit was not applied" and its file removed;
     - a moved region is relinked where it is now;
     - a split relinks only the part with the original event ID;
     - closing stops the watches and lets the copies go to the Trash;
     - Undo restores the original media and span, and Redo the edit.
   - Pinned by `an_edit_of_a_deleted_region_is_dropped`, `undo_restores_the_original_span` and `copies_are_kept_until_close_then_trashed` (Task 2), and `closing_stops_every_watch` (Task 3).
4. **Names and folders.** A media name with `/`, `:` or 200 characters; a copy that already exists from an earlier edit; `Media/Edits` missing or a file.
   - Expected: the copy's name is made safe (`/`, `:` and control characters become `-`, the stem is cut to 100 characters), the first free `n` is taken, `Media/Edits` is created when missing, and a file in its place is refused with a message.
   - Pinned by `edit_copies_get_safe_free_names` (Task 2).
5. **Hostile or careless calls.** `region.open_external` on a MIDI note, on two regions, on none, on an offline region; an `app` that is not an `.app` folder; `region.reload_external` on a region with no edit; a 70,000-character path.
   - Expected: each is refused with a clear message, and nothing is written or launched.
   - Pinned by `hostile_external_calls_are_refused` (Task 2), `externalCommandsCheckTheirArguments` (Task 4) and `external_commands_check_their_params` (Task 4).

## Plan Rulings

Each ruling decides something the spec leaves open, in the format "what — why — cost if wrong".

1. **Ruling (relinking one region): `AudioCmd::SetSource { event, media, source_offset, length }` changes one region's media and span.**
   - It refuses a span past the media's end, `length == 0` and a non-region. It clamps fades: `fade_in = min(fade_in, length)`, then `fade_out = min(fade_out, length − fade_in)`.
   - Its name is "Set Region Source". The external edit runs it inside `history.begin("External Edit")`, after `MediaCmd::Add`, so the step is labelled "External Edit".
   - The region's start tick stays, and its tick duration follows the new length as `Trim` computes it.
   - Why: §11's relink of that region alone. `MediaCmd::Relink` would move every region using the item.
   - Cost if wrong: none expected.
2. **Ruling (the copy): the span is read with `CafReader` from the region's media file and written with 5f-2's `PcmWriter`.**
   - Format: WAV; the project's rate (the media's own); the project's depth, with dither at 16 bits, as every VizDAW 16-bit write.
   - It goes to `Media/Edits/<stem> edit <n>.wav` through `.<stem> edit <n>.wav.partial`, renamed exclusively.
   - `<stem>` is the media item's name made safe (Review Focus 4). `n` starts at 1.
   - An offline region (missing media) is refused ("The region's audio is offline; relink it first").
   - Why: §11 and spec Ruling 32.
   - Cost if wrong: none expected.
3. **Ruling (edit copies are runtime state, kept until close): the session keeps `edits: BTreeMap<EventId, EditCopy { path, from: MediaId, applied: Option<(u64, i64)> }>`.**
   - `applied` is the size and modification time last imported.
   - Retention (`Inner::tidy`) keeps every copy's path while the session is open. At close, the map is cleared before the final tidy, so the copies go to the Trash with the other unreferenced files.
   - Save As does not carry the copies: watching continues at the old path. Where that package is removed (a scratch package), the next apply says "The edit copy is gone; Open in External Editor again".
   - Reload works while the copy exists, in this session.
   - Why: the spec's "watching lasts until the project closes", and the retention rule "once nothing refers to them". Nothing in the model refers to a copy, so no file-format field is needed.
   - Cost if wrong: after reopening a project, "Reload External Edit" says the copy is gone. Keeping copies across sessions needs a saved list (a later format change).
4. **Ruling (where the stable rule lives): in Swift (`ExternalEditWatcher`), driven by the 60 Hz document ticker with an injected clock and file probe.**
   - The core only imports what it is handed (`apply_external_edit(event, path)`), and refuses a path that is not the event's copy.
   - The rule:
     - each watch event records the time;
     - a tick at least 1 s after the last event, when the probe's `(size, mtime)` equals what it was at that event and differs from the last applied pair, fires one apply;
     - an apply in progress defers the next until it ends.
   - Why: §11 names `DispatchSource` (an app API). §12.3 tests the rule in Swift with temp files.
   - Cost if wrong: none expected.
5. **Ruling (the editor application): Settings › Editing "External sample editor" stores the chosen `.app` path in preferences (`VizDAW.externalEditor`), chosen with an `NSOpenPanel` limited to applications.**
   - "Open in External Editor" uses it, through `NSWorkspace.open(_:withApplicationAt:configuration:)`.
   - With none chosen, the menu item opens the panel first (a menu action, not a programmatic call).
   - `region.open_external { app? }` takes `app` (an absolute path ending in `.app`) over the preference.
   - `VIZDAW_EXTERNAL_EDITOR=stub` selects the stub opener, which launches nothing.
   - Why: §11 and §12.1's "configured stub editor command".
   - Cost if wrong: none expected.
6. **Ruling (the job): `JobKindRow::ExternalEdit` imports the copy with the system importer into the project format (`import_reserved`, stem `<media name> edit <n>`).**
   - The job fails for more than two channels ("The edit has <n> channels; regions hold one or two") and for an empty file. It removes what it wrote.
   - Its apply goes through 5f-2's `ImportAs` with `{ label: "External Edit", origin: ExternalEdit { from } }` and no lane creation. Its target is replaced by a closure that runs `AudioCmd::SetSource` on the event in the same transaction.
   - A deleted event fails the apply with "The region was deleted; the edit was not applied" and removes the imported file.
   - Why: it reuses the import's apply and its failure clean-up.
   - Cost if wrong: none expected.
7. **Ruling (the channel):**
   - `region.open_external { app? }` acts on the editor selection, which must be exactly one audio region. Its answer status is `"editing <absolute path of the copy>"`.
   - `region.reload_external` acts on the same selection, which must be a region with an edit copy in this session. It answers once the re-check's import job ends (`.job`), or `ran` with "unchanged" when nothing changed.
   - Both are Region-menu items ("Open in External Editor", "Reload External Edit").
   - Why: §12.1. The e2e needs the copy's path.
   - Cost if wrong: one more answer field.

## Carried items

- None name the external editor. Spec Rulings 26 and 32 bind (Rulings 1–2).

## File map

| File | Task | Role |
|---|---|---|
| `crates/core-model/src/command/audio.rs`, `crates/core-model/tests/cmd_external.rs` | 1 | `SetSource` |
| `crates/core-ffi/src/external.rs`, `src/session.rs`, `src/jobs.rs`, `src/lib.rs`, `tests/external.rs`; `app/Sources/VizDAWAppModel/ProjectMedia.swift`; FakeSession | 2 | copy, copies, job, apply |
| `app/Sources/VizDAWAppModel/{ExternalEditWatcher.swift,ExternalEditModel.swift}`, `app/Sources/VizDAW/ExternalEditing.swift`, tests | 3 | watcher, opener, model |
| `crates/core-control/src/registry.rs`, tests; `app/Sources/VizDAWAppModel/{RegionCommands.swift,ControlCommands.swift,Nudge.swift}`, `app/Sources/VizDAW/{EditingSettingsView.swift,AppCommands.swift}`, tests | 4 | channel, menu, Settings |
| `crates/vizctl/tests/e2e.rs`, `app/README.md` | 5 | e2e, README |

---

### Task 1: `AudioCmd::SetSource`

**Files:**
- Modify: `crates/core-model/src/command/audio.rs`
- Test: create `crates/core-model/tests/cmd_external.rs`

**Interfaces:**
- Produces: `AudioCmd::SetSource { event: EventId, media: MediaId, source_offset: u64, length: u64 }`, named "Set Region Source" (Ruling 1).

Find:
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
    /// Point one region at `media` from `source_offset` for `length`
    /// frames, its fades clamped (spec §11; plan 5f-3 Ruling 1).
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

Find:
```rust
            AudioCmd::Normalize { .. } => "Normalize",
```
Replace:
```rust
            AudioCmd::Normalize { .. } => "Normalize",
            AudioCmd::SetSource { .. } => "Set Region Source",
```

- [ ] **Step 1: Write the failing tests** in `cmd_external.rs`, using `core_engine`-free fixtures from `core_model` (the file's own small builder, after `crates/core-model/tests/cmd_regions.rs`):
  - `set_source_relinks_one_region_only`: two regions on media A. `SetSource` of the first to media B (offset 0, length 1000) changes the first's `media`, `source_offset` and `length`; the second still plays A; the start tick is unchanged.
  - `fades_are_clamped_to_the_new_length`: fades 800 in and 600 out; `SetSource` length 1000 gives fade in 800 and fade out 200. Length 500 gives 500 and 0.
  - `set_source_refuses_bad_spans`: a span past B's end; length 0; an unknown media; a MIDI note event; an unknown event. Each is an error, with the document unchanged.
  - `undo_of_set_source_restores_the_span`: through `History`, undo gives back the exact old body.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-model --test cmd_external`). Expected: FAIL — no variant `SetSource`.

- [ ] **Step 3: Implement** the arm in `apply`. Use the `region`/`store` helpers, the media check of `AudioCmd::Insert` (span within `length_frames`), and `Trim`'s duration computation.

- [ ] **Step 4: Run the tests and the gates** (`cargo test -p core-model`; `cargo test -p core-ffi --lib`, whose exhaustive matches over `AudioCmd`, if any, gain the arm). Expected: +4.

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
Counts: Rust +4.

---

### Task 2: The edit copy, the copies kept until close, the External Edit job

**Files:**
- Create: `crates/core-ffi/src/external.rs`, `crates/core-ffi/tests/external.rs`
- Modify: `crates/core-ffi/src/lib.rs`, `crates/core-ffi/src/session.rs` (`Inner.edits`; `tidy`; close; the job's apply), `crates/core-ffi/src/jobs.rs`, `app/Sources/VizDAWAppModel/ProjectMedia.swift`, `app/Tests/VizDAWAppModelTests/FakeSession.swift`

**Interfaces:**
- Consumes: `core_media::export::{PcmWriter, ExportFormat, Container}`, `core_media::caf::CafReader`, 5f-2's `ImportAs` and `apply_import_as`, `AudioCmd::SetSource` (Task 1).
- Produces:
  ```rust
  // external.rs
  #[derive(Debug, Clone, PartialEq, uniffi::Record)]
  pub struct ExternalEditRow { pub event: u64, /// Absolute path of the copy.
      pub path: String, /// The media item the edit started from.
      pub from: u64 }
  /// The copy's file name for media `name` and number `n` (Review Focus 4).
  pub fn copy_name(name: &str, n: u32) -> String;   // "<safe stem> edit <n>.wav"
  pub const EDITS_DIR: &str = "Media/Edits";
  // Session (exported)
  /// Writes the region's span (Ruling 2) and remembers the copy; `Model` for
  /// anything but one online audio region.
  pub fn open_external(&self, event: u64) -> Result<ExternalEditRow, VizError>;
  /// The region's copy in this session, if any (Reload).
  pub fn external_edit(&self, event: u64) -> Result<Option<ExternalEditRow>, VizError>;
  /// Imports `path` (the event's copy only) as an ExternalEdit job (Ruling 6); the job ID.
  pub fn apply_external_edit(&self, event: u64, path: String) -> Result<u64, VizError>;
  // jobs.rs
  JobKindRow::ExternalEdit
  Work::ExternalEdit { event: EventId, from: MediaId, file: Result<Vec<Imported>, String>, copy: (u64, i64) }
  ```

How the job and retention fit in:
- `Inner` gains `edits: BTreeMap<EventId, EditCopy>`.
- `apply_jobs` applies `Work::ExternalEdit`: `begin("External Edit")`, then `MediaCmd::Add` (origin `ExternalEdit { from }`), then `AudioCmd::SetSource { event, media: new, source_offset: 0, length: frames }`, then commit. On success, `edits[event].applied = copy`.
- `discard` handles `Work::ExternalEdit` by removing its imported files.

In `session.rs`, `tidy` keeps the copies:

Find:
```rust
        for p in self.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
```
Replace:
```rust
        for p in self.recorded_keep() {
            refs.entry(p).or_insert(None);
        }
        // Edit copies while the project is open (plan 5f-3 Ruling 3).
        for e in self.edits.values() {
            refs.entry(e.path.clone()).or_insert(None);
        }
```

and the close lets them go:

Find:
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

In `jobs.rs`:

Find:
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
This is the text plan 5f-2 Task 4 left. `lib.rs` gains `pub mod external;` after `pub mod events;`:

Find:
```rust
pub mod events;
```
Replace:
```rust
pub mod events;
pub mod external;
```

Swift `ProjectMedia.swift` gains `case .externalEdit: "Importing the external edit…"` and `case .externalEdit: "External Edit Failed"`.

- [ ] **Step 1: Write the failing tests** in `crates/core-ffi/tests/external.rs` (a `serial()` with `force_offline()` and `reset_host()`; projects from `Session::new_demo_audio()` saved into a `tempfile` folder with `debug_set_trash_dir` pointing into it; `region()` picks the demo's first audio region and its row):
  - `the_copy_is_the_span_as_wav_in_the_project_format`:
    - `open_external(region)` gives a path under `<package>/Media/Edits/`, named `<media name> edit 1.wav`;
    - parsed (the 5f-2 test reader, copied here), it holds `length` frames at the project rate and depth, with the region's channels;
    - its samples equal the media CAF's frames `[source_offset, source_offset + length)` quantized to the project depth: no gain and no fades, though the region has gain and a fade;
    - a second `open_external` of the same region gives `edit 2.wav`.
  - `edit_copies_get_safe_free_names`: `copy_name("a/b:c\u{7}", 3) == "a-b-c- edit 3.wav"`, and a 200-character name is cut to 100 characters plus the suffix. `Media/Edits` is made when missing. A file named `Edits` in its place makes `open_external` fail with a message, and nothing is written.
  - `hostile_external_calls_are_refused`: `open_external` on a note event; an unknown ID; an offline region (its media file moved away, after `recheck_media`). `apply_external_edit` with a path that is not the event's copy; a relative path; a 70,000-byte path; an event with no copy. Each is `Model` and nothing is written.
  - `copies_are_kept_until_close_then_trashed`: after `open_external`, `save()` keeps the copy (it is not moved to the test trash). `close()` moves it to the test trash folder.
  - `#[cfg(target_os = "macos")]` (macOS only: importing a WAV goes through ExtAudioFile):
    - `a_shorter_mono_edit_relinks_the_region`: overwrite the copy (remove it and write a new one) with a mono WAV, 30 % shorter, at 44.1 kHz in a 48 kHz project. `apply_external_edit` with the job waited gives the undo label "External Edit". The region's media is a new item with origin `ExternalEdit { from }`, `source_offset == 0`, and a length equal to the converted frames (±1). The fades are clamped. Another region on the original media is unchanged.
    - `undo_restores_the_original_span`: undo gives back the old media, offset and length, and redo the new ones.
    - `an_unreadable_or_wide_edit_changes_nothing`: an empty file, garbage bytes, and a 3-channel WAV (written with `PcmWriter`'s raw RIFF layout by hand, or by `PcmWriter` with 3 channels if it allows it; else 3 mono interleaved by hand). Each job fails with its message ("channels" for the wide file), the document is unchanged, and `Media/` gains no file.
    - `an_edit_of_a_deleted_region_is_dropped`: start the job with jobs held, delete the region, release. The job is Failed with "The region was deleted; the edit was not applied", and the imported file is removed.

- [ ] **Step 2: Run them to see them fail** (`cargo test -p core-ffi --test external`). Expected: FAIL.

- [ ] **Step 3: Implement** as above.

- [ ] **Step 4: Run the tests and the gates** (the workspace; FFI and Swift with the `FakeSession` stubs: `openExternal` returns `externalRow` or throws `externalError`, `externalEdit` returns `externalRow`, `applyExternalEdit` records `(event, path)` and returns a job ID). Expected: +8 on macOS (+4 on Linux).

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/external.rs crates/core-ffi/src/lib.rs crates/core-ffi/src/session.rs crates/core-ffi/src/jobs.rs crates/core-ffi/tests/external.rs app/Sources/VizDAWAppModel/ProjectMedia.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "core-ffi: external-edit copies of a region's span, kept until close, and the External Edit job relinking that region alone (plan 5f-3 Task 2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -9
```
Counts: Rust +8 (macOS).

---

### Task 3: The watcher, the openers, `ExternalEditModel`

**Files:**
- Create: `app/Sources/VizDAWAppModel/ExternalEditWatcher.swift`, `app/Sources/VizDAWAppModel/ExternalEditModel.swift`, `app/Sources/VizDAW/ExternalEditing.swift` (`LiveFileWatching` with `DispatchSource`, `WorkspaceOpener`), `app/Tests/VizDAWAppModelTests/ExternalEditWatcherTests.swift`, `app/Tests/VizDAWAppModelTests/ExternalEditModelTests.swift`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift` (`external: ExternalEditModel`; its tick in `idle`; stop on close)

**Interfaces:**
- Produces:
  ```swift
  public struct FileStamp: Equatable, Sendable { public let size: UInt64; public let modified: Date }
  public protocol FileProbe: Sendable { func stamp(_ path: String) -> FileStamp? }
  public struct DiskProbe: FileProbe { public init() }            // FileManager attributes
  /// Watch events for one path; the token stops it when released.
  public protocol FileWatching { func watch(_ path: String, _ changed: @escaping @MainActor () -> Void) -> AnyObject? }
  /// The 1 s stable rule (Ruling 4).
  @MainActor public final class ExternalEditWatcher {
      public static let stableSeconds: TimeInterval = 1
      public init(path: String, probe: any FileProbe, watching: any FileWatching, now: @escaping () -> Date)
      /// A watch event (also called by Reload).
      public func changed()
      /// True once per stable change since the last `applied(_:)`.
      public func due() -> FileStamp?
      public func applied(_ s: FileStamp)
      public func stop()
  }
  public protocol ExternalOpener { func open(_ file: URL, with app: URL?) throws }
  public final class FakeOpener: ExternalOpener { public private(set) var opened: [(URL, URL?)] }
  public final class StubOpener: ExternalOpener { public private(set) var last: URL? }   // VIZDAW_EXTERNAL_EDITOR=stub
  @MainActor @Observable public final class ExternalEditModel {
      public init(document: DocumentModel, opener: any ExternalOpener, watching: any FileWatching,
                  probe: any FileProbe = DiskProbe(), now: @escaping () -> Date = Date.init)
      /// Writes the copy, opens it, starts watching; the copy's path.
      public func open(_ region: UInt64, app: URL?) throws -> String
      /// Re-checks by hand (Ruling 7): `true` when an import started.
      public func reload(_ region: UInt64) throws -> Bool
      public func tick()       // DocumentModel.idle
      public func stopAll()    // the document closes
      public var importing: Set<UInt64> { get }
  }
  ```

`tick()` drives each watcher:
- When a watcher is `due()` and its region has no import running, `tick` calls `session.applyExternalEdit(event:path:)` and records the job.
- When that job ends Done, the watcher is told `applied(stamp)`. When it ends Failed, the alert "External Edit Failed" shows the job's message.
- `LiveFileWatching` opens the path `O_EVTONLY` with a `DispatchSource.makeFileSystemObjectSource` on `.write`, `.rename` and `.delete`, on the main queue. On rename or delete, it cancels the source and reopens the path every 0.5 s until it exists again, then reports `changed()`.

- [ ] **Step 1: Write the failing tests.**
  - `ExternalEditWatcherTests`: temp files in `FileManager.default.temporaryDirectory`, a `FakeWatching` that hands back its callback, and a mutable fake clock.
    - `aChangeIsDueAfterOneStableSecond`: write, `changed()` at t=0: not due at 0.5 s; due at 1.0 s with the file's stamp; not due again.
    - `aSecondSaveRestartsTheSecond` (Review Focus 1's `a_second_save_restarts_the_second`): change at 0, rewrite with other bytes and `changed()` at 0.8: not due at 1.2, due at 1.8.
    - `aFileStillGrowingIsNotDue`: change at 0; the file's size changes at 0.9 with no event (an editor still writing): not due at 1.0 (the stamp differs from the one at the event), due 1 s after the next event.
    - `oneChangeIsAppliedOnce`: due, `applied(stamp)`, then `changed()` with the same stamp: not due.
    - `aRenameOverTheFileRearmsTheWatch`: replace the file by writing a temp and `rename(2)`ing it over the path, then `changed()`: due after 1 s with the new stamp (the probe reads the path, not a handle).
    - `aDeletedFileWaitsForItsReturn`: delete, `changed()`: never due while missing; recreate, `changed()`: due after 1 s.
  - `ExternalEditModelTests` (`FakeSession`, `FakeOpener`, `FakeWatching`, a fake clock):
    - `openWritesOpensAndWatches`: `open(7, app:)` calls `session.openExternal(event: 7)`, then `opener.open(copy, app)`, and starts a watch on the copy's path.
    - `aStableChangeStartsOneImport`: after a change and 1 s, `tick()` calls `applyExternalEdit(event: 7, path:)` once; more ticks do nothing while it runs.
    - `aSaveDuringAnImportIsAppliedAfterIt` (`a_save_during_an_import_is_applied_after_it`): a change during the running job is applied after the job ends Done.
    - `reloadReChecks`: `reload(7)` with an unchanged stamp is false; with a changed stamp it starts an import at once (no second wait: the user asked).
    - `closingStopsEveryWatch` (`closing_stops_every_watch`): `stopAll()` releases every token (`FakeWatching.live == 0`), and `tick()` does nothing after.
    - `aFailedOpenIsAnAlert`: `externalError` raises "Open in External Editor Failed" and starts no watch.

- [ ] **Step 2: Run them to see them fail** (`scripts/swift.sh test`). Expected: FAIL.

- [ ] **Step 3: Implement.** In `DocumentModel.idle()`, after `if !jobs.isEmpty { pollJobs() }`, add `external.tick()`. Where the document closes (its existing close path), add `external.stopAll()`. The app makes the model with `WorkspaceOpener` (or `StubOpener` when `ProcessInfo.processInfo.environment["VIZDAW_EXTERNAL_EDITOR"] == "stub"`) and `LiveFileWatching`.

- [ ] **Step 4: Run the tests and the gates.** Expected: Swift +12.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/ExternalEditWatcher.swift app/Sources/VizDAWAppModel/ExternalEditModel.swift app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAW/ExternalEditing.swift app/Tests/VizDAWAppModelTests/ExternalEditWatcherTests.swift app/Tests/VizDAWAppModelTests/ExternalEditModelTests.swift
git commit -m "app: the external-edit watcher (1 s stable rule, replaced and deleted files), openers that launch nothing in tests, the per-document model (plan 5f-3 Task 3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -8
```
Counts: Swift +12.

---

### Task 4: Settings › Editing, the Region menu, `region.open_external` and `region.reload_external`

**Files:**
- Modify: `crates/core-control/src/registry.rs`, `crates/core-control/tests/commands.rs`, `app/Sources/VizDAWAppModel/RegionCommands.swift`, `app/Sources/VizDAWAppModel/ControlCommands.swift`, `app/Sources/VizDAWAppModel/Nudge.swift` (`EditingPreferences.externalEditor`), `app/Sources/VizDAW/EditingSettingsView.swift`, `app/Sources/VizDAW/AppCommands.swift`
- Test: create `app/Tests/VizDAWAppModelTests/ExternalCommandsTests.swift`

**Interfaces:**
- Produces:
  - `RegionCommands.openExternal = "region.open_external"` and `RegionCommands.reloadExternal = "region.reload_external"`, in `ids`.
  - `RegionCommands.runExternal(_ id: String, args: [ControlArg], doc: UUID, _ m: DocumentModel, editor: String?) -> ControlOutcome`.
  - `EditingPreferences.externalEditor: String?` (key `VizDAW.externalEditor`).

In `registry.rs`:

Find:
```rust
    cmd("region.remove_fades", "Region", "Remove Fades", None, true),
```
Replace:
```rust
    cmd("region.remove_fades", "Region", "Remove Fades", None, true),
    CommandSpec {
        params: EXTERNAL_PARAMS,
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

`EXTERNAL_PARAMS` holds one entry: `app`, `ParamKind::Path("app")`, optional, with the help "The editor application (an absolute path to a .app); default: Settings › Editing".

Settings › Editing gains a section "External Sample Editor" with the chosen app's name, Choose… (an `NSOpenPanel` limited to `.application` in `/Applications`) and Clear.

- [ ] **Step 1: Write the failing tests.**
  - `external_commands_check_their_params` (core-control): `{}` and `{app: "/Applications/X.app"}` are accepted; `{app: "X.app"}` (relative) and `{app: "/tmp/x.wav"}` are refused. The count is 146, with "; 5f-3 adds region.open_external and region.reload_external".
  - `ExternalCommandsTests`:
    - `externalCommandsCheckTheirArguments`: with no selection, `unavailable` ("Select one audio region first."); with two selected events, the same; with a MIDI note selected, `failed` with the core's message; with no editor, no `app` and no stub, `unavailable` with the Settings message. `f.externalOpened` is empty after each.
    - `openAnswersWithTheCopysPath`: one region selected, the stub opener gives status `"editing /tmp/p.vizdaw/Media/Edits/Kick edit 1.wav"` (from `f.externalRow`).
    - `reloadWithNothingChangedAnswersUnchanged` and `reloadWithAChangeIsAJob`.
    - `theMenuItemsAreEnabledForOneRegion`: `RegionCommands.enabled` for both IDs needs exactly one selected event.

- [ ] **Step 2: Run them to see them fail.** Expected: FAIL (count 144, unknown IDs).

- [ ] **Step 3: Implement.** In `ControlCommands.run`, the existing `case _ where RegionCommands.ids.contains(id):` branch must route the two new IDs to `runExternal`. Add before it:
   ```swift
   case RegionCommands.openExternal, RegionCommands.reloadExternal:
       return RegionCommands.runExternal(id, args: args, doc: docID, m, editor: host.externalEditor)
   ```
   with `ControlHost` gaining `var externalEditor: String? { get }`, which the app answers from `editingPreferences.externalEditor` and the stub. In `AppCommands.swift`'s Region menu (where Remove Fades is), add the two items. The menu's Open asks for an editor with the panel when none is set.

- [ ] **Step 4: Run the tests and the gates.** Expected: core-control +1, Swift +5, registry 146.

- [ ] **Step 5: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-control/src/registry.rs crates/core-control/tests/commands.rs app/Sources/VizDAWAppModel/RegionCommands.swift app/Sources/VizDAWAppModel/ControlCommands.swift app/Sources/VizDAWAppModel/Nudge.swift app/Sources/VizDAW/EditingSettingsView.swift app/Sources/VizDAW/AppCommands.swift app/Sources/VizDAW/ControlChannel.swift app/Tests/VizDAWAppModelTests/ExternalCommandsTests.swift app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "app, control: Settings > Editing external sample editor, Open in External Editor and Reload External Edit, on the menu and the channel (plan 5f-3 Task 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -12
```
Counts: Rust +1, Swift +5.

---

### Task 5: The e2e through the stub editor, README 185–188, the full gates

**Files:**
- Modify: `crates/vizctl/tests/e2e.rs`, `app/README.md`

- [ ] **Step 1: Write the e2e** `edits_a_region_through_the_stub_editor`:
  1. Start with `App::start_with(&[("VIZDAW_EXTERNAL_EDITOR", "stub")])`, then `app.new_demo_audio`, and select the first audio region (`state.get document`'s first region ID, then the selection command the file already uses for regions; `regions_normalize_relink_and_trash` shows how).
  2. `region.open_external` answers a status starting with `editing ` and naming a path under the throwaway home.
  3. The test removes that file and writes a new mono 48 kHz WAV of 24,000 frames at 0.5 in its place, with `core_media::export::PcmWriter`, through a temp name renamed over it, as editors do.
  4. Wait (`wait_for_state` on `document`) for `undo == "External Edit"`.
  5. `state.get media` lists a new item with origin `external_edit`, 24,000 frames.
  6. `region.reload_external` answers `ran` with status "unchanged".
  7. Hostile: `region.open_external` with `{"app": "relative.app"}` is `invalid_params`; with no selection, `unavailable`.

- [ ] **Step 2: Write README 185–188:**

```markdown
### External sample editor (plan 5f-3)

By hand; the watcher is tested (`ExternalEditWatcherTests`), the core
(`core-ffi/tests/external.rs`) and the e2e
`edits_a_region_through_the_stub_editor`.

185. Settings › Editing › External Sample Editor › Choose… lists your
     applications; choose a sample editor.
186. The manual checklist's "external editor round trip": select one
     audio region, Region › Open in External Editor: the editor opens
     "<name> edit 1.wav". Trim it shorter and save: about a second later
     the region plays the edit, Edit shows "Undo External Edit", and the
     media pool lists the edit as External Edit. Other regions using the
     original sound as before. Undo puts the original back.
187. Save again in the editor, twice quickly: one more "External Edit"
     step, not two. Region › Reload External Edit with nothing changed
     says the edit is unchanged.
188. Close the project: the edit copies go to the Trash (with the other
     unused files); reopen it: Reload External Edit says the copy is gone.
```

- [ ] **Step 3: Run the full gates**, including the rt-check gates, the workspace tests, Swift, `scripts/make-app.sh` and the e2e. Expected: Rust +13 (macOS), Swift +17, e2e 13, registry 146.

- [ ] **Step 4: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/vizctl/tests/e2e.rs app/README.md
git commit -m "e2e: an external edit through the stub editor; README 185-188 (plan 5f-3 Task 5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P"
git log -1 --oneline && git show --stat HEAD | tail -4
```

---

## Self-Review

1. **Spec coverage.** Every requirement of §11 has a task:
   - the Settings chooser (Task 4);
   - the WAV of the span with no gain or fades, in the project rate and depth, at `Media/Edits/<name> edit <n>.wav` (Task 2);
   - opening (Tasks 3–4);
   - the `DispatchSource` watch for write, rename and delete (Task 3);
   - the 1 s stable rule (Task 3);
   - the import to CAF with `ExternalEdit { from }` (Task 2);
   - that region alone, offset 0, new length, fades clamped, as one "External Edit" step (Tasks 1–2);
   - other regions unaffected (Task 2);
   - watching until close (Tasks 2–3);
   - Reload (Tasks 3–4);
   - retention (Task 2).

   Also covered: §12.1 `region.open_external` with the stub (Tasks 4–5), and §12.3's watcher test with temp files (Task 3). No gaps.
2. **Step scan.** Every step names its files, its signatures and the spec values. The tests for Tasks 1–4 are named with their assertions; their harnesses are those files' own.
3. **Type consistency.**
   - `ExternalEditRow` (Task 2) is used by `ExternalEditModel.open` (Task 3) and the channel answer (Task 4).
   - `Work::ExternalEdit` and `JobKindRow::ExternalEdit` (Task 2) map to `.externalEdit` in Swift.
   - `FileStamp` and `FileProbe` are shared by the watcher and the model.
4. **Review Focus.** Each of the five lines names its tests in their tasks.
5. **Proportion.** About 450 lines for spec §11's 20 lines. The rules (stable file, names, clamping, retention) are written once in the Rulings.

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-11-02-external-editor.md`. The controller runs it subagent-driven after 5f-2 under the standing authorization. It is small; one reviewer per task, and the final review should replay Review Focus 1 with a real editor (README 186–187).
