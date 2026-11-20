# 26.0.0 Release-Gate Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clear the three release-gate blockers and the six backlog rows the owner pulled into 26.0.0, so main can be tagged 26.0.0 and offered as an unsigned download from GitHub.

**Architecture:** These are small, independent fixes across the Rust core, the Co-Producer gate and the Swift app shell. There is one new FFI call (`Session::has_unsaved_changes`), one new Swift decision type (`ClosePrompts`) behind the save prompts, one notices generator script, and one new Settings section. Nothing changes in the file format.

**Tech Stack:** Rust 2024 (workspace, uniffi FFI), Swift 6.1 / SwiftUI + AppKit (macOS 14+), shell scripts.

**Spec:** There is no separate design document. The plan argues from four sources:
- the owner's approvals in chat on 2026-11-18;
- the release-gate review (blockers B1–B3 and the licence-notice rule violation; the review sits in the session scratchpad, and the facts this plan needs are copied in below);
- the backlog rows named in each task (`docs/feature-backlog.md`);
- the Co-Producer design (`docs/superpowers/specs/2026-11-14-co-producer-design.md`).

Rulings made without a spec are recorded in the ledger as provisional.

## Global Constraints

- Version: `26.0.0` everywhere a version is shown or written.
- Co-Producer: in 26.0.0 Co-Producer never writes MIDI notes, on any track, its own included (owner, 2026-11-16). Changing or deleting the composer's work, routing or sound needs an approval-sheet yes. A Co-Producer bus's own send going to −∞ or being retargeted needs no yes (owner, 2026-11-18).
- Programmatic and control-channel calls never open dialogs. The only exceptions are the import's new-project window, Co-Producer's approval sheet (only for a pending request) and the starter project's one new song window. The new save prompts are user-only.
- New commands get hostile-input tests. New FFI goes inside `ffi_guard`.
- Swift: build and test only through `scripts/swift.sh`, `scripts/build-ffi.sh` and `scripts/make-app.sh`. Swift 6.1 rules:
  - no `weak self` or non-Sendable values inside `MainActor.assumeIsolated`;
  - no `@State` or `@FocusState`;
  - AppKit main-actor globals (`NSApp` and friends) only from `@MainActor` code; CI's Xcode 16.4 refuses other uses.
- UI: accessibility labels and theme tokens on every control. The UI says "track", never "lane".
- Tests:
  - never open real audio or MIDI devices, and never trigger mic/TCC prompts;
  - never touch the real `~/Library` (Application Support/VizDAW, prefs, Trash, Audio/Plug-Ins, Keychain) or the real UserDefaults;
  - Swift tests that make a real Session set `TestSupportDir.shared`;
  - the e2e uses vizctl's throwaway home and `VIZDAW_TEST_HOOKS=1`;
  - on Linux CI, AU/CF/importer tests are macOS-gated.
- Never run `auval`. No GUI or computer-use tools. Never use dangerouslyDisableSandbox.
- Names:
  - banned vendor and DAW names appear only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`) and in licence notices;
  - the reference DAW's repo name stays out of repo text.
- Commits:
  - by explicit paths;
  - never commit `.DS_Store`, `.superpowers/` or generated FFI files;
  - brand assets are never committed;
  - each commit ends with the two trailer lines `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P`.
- Env: `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`.
- Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`.
- Pace (owner): fix crashes, data loss, wrong overwrites, security holes and rule violations. Narrow edge cases go to the backlog.

## Review Focus

1. **A dirty untitled song closed with ⌘W, the red button or ⌘Q must ask, and Cancel must keep everything.** That covers a Save panel cancelled after choosing Save, too. (Task 3: `cancelled_save_panel_keeps_the_window`.)
2. **Quitting with several dirty songs must not lose the ones after a Cancel.** Cancel at the second prompt cancels the whole quit, and the first song's choice stands. (Task 3: `cancel_at_the_second_song_stops_the_quit`.)
3. **The control channel and the e2e must never meet a save prompt.** `file.close` over the channel and the e2e's app shutdown must not hang. (Task 3: `control_close_never_prompts` and the e2e run in Task 10.)
4. **Co-Producer finds another way to write notes.** Task 2 covers `file.import_midi`, `media.import` with `lane`, and `track.duplicate`. Also check by sweep that no other Add command copies or creates notes. (Task 2: `no_add_command_writes_notes`.)
5. **The version is set in one place and every reader agrees with it.** (Task 1: `bundle_and_crate_versions_agree`.)

---

### Task 1: Version 26.0.0 from one place

**Files:**
- Modify: `Cargo.toml` (`[workspace.package]`), `crates/*/Cargo.toml` (`version.workspace = true`)
- Modify: `scripts/make-app.sh:71` (read the version, no literal)
- Test: `crates/core-control/tests/version.rs` (new)

- [ ] **Step 1: Write a failing test.** Add `bundle_and_crate_versions_agree` in `crates/core-control/tests/version.rs`:
  - `assert_eq!(env!("CARGO_PKG_VERSION"), "26.0.0")`;
  - read `../../scripts/make-app.sh` and assert it contains no `<string>0.` or other literal version for `CFBundleShortVersionString`, and that it takes the value from the workspace (for example from `cargo metadata --no-deps --format-version 1` or a `grep` of `Cargo.toml`'s `[workspace.package] version`).
- [ ] **Step 2: Confirm it fails.** Run `cargo test -p core-control --test version`. It should fail.
- [ ] **Step 3: Set the version.**
  - Add `version = "26.0.0"` under `[workspace.package]`.
  - Replace every crate's `version = "0.1.0"` with `version.workspace = true`.
  - In `make-app.sh`, write `CFBundleShortVersionString` from the workspace version. `CFBundleVersion` keeps its build-stamp line (166).
  - Update the test fixture `crates/core-ffi/tests/aaf.rs:1226` (`app_version: "0.1.0"`) to use `env!("CARGO_PKG_VERSION")`.
- [ ] **Step 4: Confirm everything passes.** Run `cargo test --workspace` (green) and `scripts/make-app.sh`, then check: `plutil -extract CFBundleShortVersionString raw dist/VizDAW.app/Contents/Info.plist` should print `26.0.0`.
- [ ] **Step 5: Commit** as `chore(release): version 26.0.0 from the workspace, read by the app bundle`.

### Task 2: Co-Producer never writes notes through imports or duplicates (blocker B2)

**Files:**
- Modify: `crates/core-control/src/co_producer.rs` (`effective_class`, ~line 143)
- Modify: `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (the Swift side's mirror check, `changesWork` ~lines 44–62)
- Test: `crates/core-control/src/co_producer.rs` tests (next to `import_midi_into_a_new_project_is_refused`), `app/Tests/VizDAWAppModelTests/` (the Co-Producer command tests)

The facts, from the review's probe: `file.import_midi` (Class::Add, with `lane`, `tick` and `channel_map`) lands notes on the composer's track with no yes. `media.import` with `lane` places audio over the composer's region, and the later region plays (`core-engine/src/schedule/audio.rs:1-6`). `track.duplicate` copies the composer's notes onto a new track. All three pass `admit_run` today, and the MCP tool list offers them, because `tools.rs` lists every non-Refused command.

Ruling (provisional): Rust cannot tell Co-Producer's own tracks from the composer's at the gate. So in 26.0.0:
- `file.import_midi` is Refused to Co-Producer in every form;
- `media.import` is Refused when it has a `lane`; without one, the import goes on new tracks, which is still an Add;
- `track.duplicate` is Refused.

Nothing in the recipes or `starter.rs` uses any of them (checked).

- [ ] **Step 1: Write the failing Rust tests:**
  - `import_midi_is_refused_in_every_form`: `{path}`, `{path, seq:1, lane:2, tick:0}` and `{path, new_project:true}` each give `admit` an `Err` whose message contains `cannot run file.import_midi`.
  - `media_import_onto_an_existing_track_is_refused`: `{path, seq:1, lane:2, tick:0}` is an Err; `{path}` is Ok.
  - `track_duplicate_is_refused`.
  - `refused_imports_are_not_offered_as_tools`: in `vizdaw-mcp` tools, `co_producer::list_json()` does not contain `file.import_midi` or `track.duplicate`.
  - `no_add_command_writes_notes`: a guard list. Every `Class::Add` id is in an allow-list in the test, and the test fails when a new Add id appears, so a reviewer has to classify it. In the list's doc comment, say which ones create notes (none may).
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-control co_producer` and `cargo test -p vizdaw-mcp`.
- [ ] **Step 3: Implement.** Extend `effective_class`:
  - `"file.import_midi" => Class::Refused`;
  - `"media.import" if arg(args, "lane").is_some() => Class::Refused`;
  - `"track.duplicate" => Class::Refused`.

  Make the list filter at `co_producer.rs:438` use `effective_class` for ids whose class depends on args only where it already does; `file.import_midi` and `track.duplicate` simply drop out. Mirror the same three refusals in Swift's Co-Producer dispatch, which is the second check, with a Swift test `coProducerImportOntoATrackIsRefused`.
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-control -p vizdaw-mcp`, `scripts/swift.sh test --filter CoProducer`, and the name scans.
- [ ] **Step 5: Commit** as `fix(co-producer): no MIDI import, no import onto an existing track, no track duplicate (release gate B2)`.

### Task 3: Ask before closing or quitting with unsaved changes (blocker B1)

**Files:**
- Modify: `crates/core-ffi/src/session.rs` (export `has_unsaved_changes`, wrapping `Inner::unsaved` at ~line 851)
- Create: `app/Sources/VizDAWAppModel/ClosePrompts.swift` (the pure decision logic)
- Modify: `app/Sources/VizDAWAppModel/DocumentStore.swift` (~666–695), `app/Sources/VizDAW/DocumentWindow.swift` (~31, 50, 131), `app/Sources/VizDAW/VizDAWApp.swift` (`AppDelegate`, ~279–318), `app/Sources/VizDAW/AppCommands.swift:98`
- Modify: `app/README.md:1415` (which says "VizDAW never asks when a window closes") and add README manual checks
- Test: `crates/core-ffi/tests/` (session tests), `app/Tests/VizDAWAppModelTests/ClosePromptsTests.swift` (new)

The facts:
- Close today: `NSApp.keyWindow?.performClose(nil)` triggers the window's `willClose` and `onDisappear`, then `store.close(id)`, then `session.close()`. That never saves, and for an untitled song it moves recordings to the Trash and removes the scratch package.
- `AppDelegate` has no `applicationShouldTerminate`.
- The FFI has no dirty flag.

**Behaviour (macOS standard, owner-approved):**
- Closing a document window with unsaved changes shows a sheet on that window:
  - message: `Do you want to save the changes you made to “<name>”?`
  - informative text: `Your changes will be lost if you don't save them.`
  - buttons: **Save** (default, Return), **Don't Save** (⌘D) and **Cancel** (Escape).
- **Save** on a saved song saves, then closes. On an untitled song it opens the existing Save As panel; if the panel is cancelled or the save fails, the window stays open.
- **Don't Save** closes the way close works today.
- **Cancel** keeps the window open.
- A clean document closes without asking.
- This applies to ⌘W, File › Close, the red close button and Window › Close.
- The window's close button shows the edited dot (`isDocumentEdited`) while the song has unsaved changes. Update it from the poll.
- **Quit (⌘Q, logout, restart)** uses `applicationShouldTerminate`:
  - With no dirty documents, it returns `.terminateNow`.
  - With one dirty document, it shows that document's sheet; Save or Don't Save leads to the quit, and Cancel cancels it.
  - With two or more, it shows an app alert: `You have <N> VizDAW documents with unsaved changes. Do you want to review these changes before quitting?` with **Review Changes…** (default), **Discard Changes** and **Cancel**. Review walks the dirty documents one at a time with the sheet above, bringing each window to the front. A Cancel at any step stops the quit, and choices already made stand.
  - It uses `.terminateLater` and `reply(toApplicationShouldTerminate:)`.
- **Never prompted:**
  - the control channel's `file.close`, which closes as today (programmatic calls never open dialogs; Co-Producer cannot run `file.close`, because it is Refused);
  - a terminate during e2e runs. Find how `crates/vizctl/tests/e2e.rs` ends the app (for example SIGTERM or `NSApp.terminate`), and make sure that path never waits on a prompt: under `VIZDAW_TEST_HOOKS=1`, `applicationShouldTerminate` answers `.terminateNow`.

**Interfaces:**
- Produces, in Rust: `#[uniffi::export] fn has_unsaved_changes(&self) -> Result<bool, VizError>` on `Session`, inside `ffi_guard`.
- Produces, in Swift (VizDAWAppModel):

  ```swift
  public enum CloseChoice: Sendable { case save, dontSave, cancel }
  public enum QuitStep: Equatable, Sendable { case quitNow, askOne(UUID), askMany(count: Int), cancel }
  public enum ClosePrompts {
      /// What a quit does first, for the documents in window order.
      public static func quitStep(dirty: [UUID], testHooks: Bool) -> QuitStep
      /// Message and informative text of a document's sheet.
      public static func sheetText(name: String) -> (message: String, informative: String)
  }
  ```

  Also produces `DocumentModel.hasUnsavedChanges: Bool`, read through the FFI.

- [ ] **Step 1: Write the failing tests:**
  - Rust: `unsaved_changes_follow_edits_and_saves`. A fresh untitled session gives `false`; after one edit command, `true`; after a save to a temp path, `false`; after an undo past the save, `true`.
  - Swift (`ClosePromptsTests`):
    - `clean_documents_quit_now`;
    - `one_dirty_document_asks_one`;
    - `several_dirty_documents_ask_many` (count 3);
    - `test_hooks_quit_now_even_when_dirty`;
    - `sheet_text_names_the_song` (the exact strings above);
    - `cancel_at_the_second_song_stops_the_quit` (drive a small `QuitWalk` state machine in `ClosePrompts.swift`: `[a,b,c]` with choices `[.dontSave, .cancel]` ends cancelled, with `a` closed and `b` and `c` still open);
    - `cancelled_save_panel_keeps_the_window` (a `.save` choice whose save reports cancelled leaves the document open in the walk);
    - `control_close_never_prompts` (the control-channel `file.close` path calls `store.close` directly even when dirty; use the existing ControlCommands test fixture).
- [ ] **Step 2: Confirm they fail.** Run `cargo test -p core-ffi --test session` (or the file holding the session tests), then `scripts/build-ffi.sh` and `scripts/swift.sh test --filter ClosePrompts`.
- [ ] **Step 3: Implement the FFI export and `ClosePrompts`/`QuitWalk`.** Then wire up the app:
  - Install a window-close hook on each document window. SwiftUI owns `NSWindow.delegate`, so install a forwarding proxy delegate that keeps SwiftUI's delegate, forwards every selector it does not handle (`forwardingTarget(for:)`/`responds(to:)`), and answers `windowShouldClose(_:)`.
  - Route `AppCommands` File › Close through the same hook (`performClose` already asks `windowShouldClose`).
  - Add `applicationShouldTerminate`.
  - Keep `applicationWillTerminate`'s order as it is.
- [ ] **Step 4: Confirm they pass.** Run `cargo test -p core-ffi`, `scripts/swift.sh test`, `scripts/make-app.sh`, and the e2e (`cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`, with `VIZDAW_E2E_APP=dist/VizDAW.app`). The e2e must not hang.
- [ ] **Step 5: Update the README.** Replace the "never asks" line at `app/README.md:1415`. Add manual checks: ⌘W on a dirty untitled song (Save panel → Cancel keeps it); the red button on a dirty saved song (Don't Save); ⌘Q with two dirty songs (Review → Cancel at the second); the edited dot.
- [ ] **Step 6: Commit** as `feat(app): ask to save before closing or quitting with unsaved changes (release gate B1)`.

### Task 4: Third-party licence notices in the app

**Files:**
- Create: `scripts/third-party-notices.sh`
- Modify: `scripts/make-app.sh` (run it and copy its output to `Contents/Resources/Licences/THIRD-PARTY.txt`)
- Create: `app/Sources/VizDAW/LicencesWindow.swift`; Modify: `app/Sources/VizDAW/AppCommands.swift` (VizDAW › Licences…)
- Modify: the README licence section
- Test: `app/Tests/VizDAWAppModelTests/LicencesTests.swift`, plus a Rust or shell check in CI that the script runs offline

Backlog rows "Third-party licence notices shown in the app" (`docs/feature-backlog.md:226`) and "Show the bundled licences in the app" (`:219`) become one Licences window, VizDAW › Licences…. It lists:
- VizDAW's own `LICENSE` (GPL-3.0-only), with the line `VizDAW bundles Dusk Audio Multi-Q, which includes parts under the GNU AGPLv3 (JUCE).`;
- `THIRD-PARTY.txt`;
- the files in `Licences/Multi-Q/`.

It is a read-only text view with a file picker list, themed and labelled.

- [ ] **Step 1: Write the generator, `scripts/third-party-notices.sh`.**
  - It runs `cargo metadata --format-version 1 --locked --offline` for the normal (non-dev) dependency graph of `core-ffi`, `vizdaw-mcp` and `vizctl`.
  - For each third-party crate it writes the name, version and licence expression, followed by the crate's own `LICENSE*`, `COPYING*` and `NOTICE*` texts from its `manifest_path` directory.
  - It writes sections for the vendored DAWproject XSDs (`crates/core-io/tests/data/dawproject/LICENSE`) and pyaaf2 (the MIT header in `crates/core-io/src/aaf/metadict.rs`: Copyright (c) 2017 Mark Reid).
  - It writes a section for the Swift package dependencies in `app/Package.resolved`, using each checkout's licence file under `.build/checkouts`. Skip the brand package and say it is not bundled when `VIZDAW_NO_BRAND=1`.
  - The Multi-Q list (JUCE AGPLv3 and its module dependencies: AudioUnitSDK Apache-2.0, FLAC/Ogg Vorbis BSD, zlib/pnglib zlib, jpeglib IJG, HarfBuzz Old MIT, SheenBidi Apache-2.0; clap-juce-extensions MIT; CLAP and clap-helpers MIT) stays in `Licences/Multi-Q/`. Check that folder holds those notices, and add any that are missing to `build-multiq.sh`'s copy step.
  - It fails if any crate has no licence text and is not on an explicit, commented exceptions list.
- [ ] **Step 2: Write the failing Swift tests:**
  - `licences_list_names_the_three_sources` (a `LicencesModel` in VizDAWAppModel reads a temp folder with `LICENSE`, `THIRD-PARTY.txt` and `Multi-Q/x.txt` and lists them in that order);
  - `missing_folder_shows_a_plain_note`.
- [ ] **Step 3: Implement the model, the window and the menu item, and wire `make-app.sh`.**
- [ ] **Step 4: Confirm.** Run `scripts/make-app.sh`, check that `dist/VizDAW.app/Contents/Resources/Licences/THIRD-PARTY.txt` exists and contains `quick-xml` and `cfb`, then run `scripts/swift.sh test --filter Licences`.
- [ ] **Step 5: Commit** as `feat(app): third-party licence notices bundled and shown in VizDAW › Licences…`.

### Task 5: Co-Producer socket and server hardening

**Files:**
- `crates/core-control/src/server.rs`, `crates/core-control/src/client.rs:140`, `crates/core-control/src/co_producer.rs`, `crates/vizdaw-mcp/src/*.rs`, `crates/core-ffi/src/session/co_producer*.rs` (where `open_copy`, `copy_parent` and the Copies folder live)
- Tests: beside each fix (hostile-input style)

This is backlog row "Co-Producer socket and server hardening" (`docs/feature-backlog.md:151`). Fix the security bullets:
1. The server runs an unterminated last line at EOF. Refuse it, with test `unterminated_last_line_is_not_run`.
2. `Client::read_line` has no length cap. Cap it at the server's own line cap (`read_line_capped`), with test `client_refuses_an_overlong_reply`.
3. `vizdaw-mcp` opens screenshots without `O_NOFOLLOW | O_NONBLOCK`, and checks no owner on a support folder it did not make. Open with both flags and check that the folder's owner is the uid, with tests `screenshot_symlink_is_refused` and `foreign_owned_support_folder_is_refused`.
4. The developer socket's `free()` has a trailing-`/` check that Co-Producer's path checks lack. Add the same check, with test `trailing_slash_path_is_refused`.
5. A symlinked `Co-Producer` or `Copies` folder is written through. Refuse symlinks there (`symlink_metadata`), with test `symlinked_copies_folder_is_refused`.
6. `open_copy` follows links and FIFOs inside a package. Open with `O_NOFOLLOW | O_NONBLOCK` and require a regular file, with test `copy_that_is_a_fifo_or_link_is_refused`.
7. The token is not bound to the document's path across Save As. Bind it to the path: a Save As ends the pending token, with test `token_dies_with_save_as`.
8. `gate()` uses up the screenshot slot on a failed screenshot. Refund the slot on failure, with test `failed_screenshot_keeps_the_slot`.

Leave the rest of the row (the check-then-write gap already covered by `RENAME_EXCL`, `.caf` for WAV data, unread screenshots, the "quitting" wording, `copy_parent`, the plug-in gesture order) as backlog with a note "26.0.0: security bullets done (plan release-gate-fixes Task 5)".

- [ ] Write each test, run it and see it fail, fix it, run it and see it pass. Then run `cargo test -p core-control -p vizdaw-mcp -p core-ffi` and Linux clippy.
- [ ] **Commit** as `fix(co-producer): socket and server hardening (release gate)`.

### Task 6: MCP elicitation recheck before release

**Files:** `crates/vizdaw-mcp/src/lib.rs`, `crates/vizdaw-mcp/src/tools.rs`; tests in `crates/vizdaw-mcp/tests/`

This is backlog row "Recheck which MCP hosts support elicitation, and the 2026-07-28 revision, before release" (`docs/feature-backlog.md:150`).

- [ ] **Step 1: Research.** Read the current MCP specification's elicitation section and the 2026-07-28 revision's final text, at modelcontextprotocol.io. Check the current docs for Claude Code and Claude's desktop app for whether they support form elicitation. Write the findings, with URLs and dates, into the task report.
- [ ] **Step 2: Adjust.** If `clientCapabilities` sits beside `_meta` or the eras mix differently from what `vizdaw-mcp` expects, change the initialize handling, with a test per shape (`initialize_with_<shape>`).
- [ ] **Step 3: Fix the row's listed bugs:**
  - bind the fallback token to the planned commands inside `vizdaw-mcp` as well (test `fallback_token_is_bound_to_its_commands`);
  - send `notifications/cancelled` for a lingering form (test `lingering_form_is_cancelled`);
  - a cancel for a queued call must not run it (test `cancelled_queued_call_never_runs`);
  - keep the blank line before "Allow these changes?" (test `allow_question_has_its_blank_line`).
- [ ] **Step 4: Confirm.** Run `cargo test -p vizdaw-mcp` and the e2e's MCP cases.
- [ ] **Step 5: Commit** as `fix(mcp): elicitation rechecked against the current hosts and revision; token, cancel and form fixes`.

### Task 7: Cap memory while importing a hostile MIDI file

**Files:** the SMF reader in `crates/core-io/src/` (the 6a MIDI importer; find it with `git grep -n "10_000_000" crates/core-io`); tests beside it

This is backlog row `docs/feature-backlog.md:179`: the 10 million event limit lets a hostile 64 MiB file allocate about 1 GB before it is refused.

- [ ] **Step 1: Write a failing test.** `hostile_file_is_refused_before_it_allocates`: build in memory a 64 MiB SMF of minimal running-status events (more than 10 million). Import it with a counting global allocator in that test binary, or a peak check through a `&mut usize` budget the reader exposes. Assert that it is refused with the existing too-many-events error and that the peak stays under 256 MiB.
- [ ] **Step 2: Implement.** First a counting pass over the track chunks (no allocation per event), refusing over the limit. Then reserve exactly and parse. Keep error messages unchanged.
- [ ] **Step 3: Confirm.** Run `cargo test -p core-io` and the 6a fuzz/property tests for the reader.
- [ ] **Step 4: Commit** as `fix(midi): count events before allocating, so a hostile file is refused cheaply`.

### Task 8: Untitled songs' stand-in folders shown and cleared in Settings

**Files:** `crates/core-ffi/src/session.rs` (~641, 672, 918, where the stand-in `<support>/Co-Producer/Untitled/<id>/` lives) or a free FFI function beside it; `app/Sources/VizDAW/CoProducerSettingsView.swift`; `app/Sources/VizDAWAppModel/` (a `StandInFolders` model); tests in both

This is backlog row `docs/feature-backlog.md:155`.

- **Behaviour:** Settings › Co-Producer gets a line `Notes and copies kept for <N> unsaved songs (<size>)` with **Show in Finder** and **Delete…**. Delete asks first, with a confirm in Settings, which is user-initiated: `Delete the notes and copies Co-Producer kept for <N> songs that were never saved?` with **Delete** and **Cancel**. Folders that belong to a song open right now are never listed or deleted. The line is hidden when N is 0.
- **Interfaces:** `list_stand_ins() -> Result<Vec<StandIn>, VizError>` (id, bytes, modified) and `delete_stand_ins(open_ids: Vec<String>) -> Result<u32, VizError>`, both inside `ffi_guard`. They delete with `remove_dir_all` only on directories (`symlink_metadata`, no symlinks followed), under `<support>/Co-Producer/Untitled/`.
- [ ] **Tests:**
  - Rust: `lists_only_untitled_stand_ins`;
  - Rust: `delete_skips_open_songs_and_symlinks`;
  - Rust: hostile entries (a file, a symlink to `$HOME`, a FIFO) are skipped;
  - Swift: `line_hidden_when_none` and `line_counts_and_sizes`.
- [ ] Confirm with `cargo test -p core-ffi` and `scripts/swift.sh test`. Add a README manual check.
- [ ] **Commit** as `feat(co-producer): show and clear the notes and copies of songs never saved`.

### Task 9: Flaky tests

**Files:** as listed in backlog row "Flaky tests" (`docs/feature-backlog.md:284`)

Make each listed test deterministic, or give it margins that hold on a loaded CI runner:
- `dawproject_round_trip`: the one-frame rounding (144 000 against 143 999). Find the rounding in 6d's import and fix it, keeping the saved seed as a regression case if it is still in the repo's proptest-regressions.
- `platform-macos` `a_source_listener_reconnects_new_sources_without_doubling_old_ones`.
- The `core-ffi` holders tests.
- `automation::glides_the_wrap_cuts_land_as_far_as_they_went`.
- `core-ffi/tests/transport.rs` count-in wall-clock window (line ~688) and `engine_claim.rs` `arming_while_an_engine_retires_does_not_wait_for_the_drain`: assert on order and event counts, not wall-clock, or widen the margins and say why in a comment.
- `kontrol::a_surfaces_file_with_an_unknown_kind_keeps_the_rest`.
- The e2e `keys_drive_step_entry_locators_and_bindings`.

- [ ] For each, find the cause first (superpowers:systematic-debugging): run it 50 times in a loop (`for i in $(seq 50); do cargo test -p <crate> <name> -- --exact || break; done`) to reproduce, fix it, then loop again to confirm. When a test cannot be made to fail in 50 runs, record that in the report and fix only what the code shows to be racy.
- [ ] Update the row: the known-flaky list keeps only what remains.
- [ ] **Commit** as `test: deterministic or CI-safe margins for the known-flaky tests`.

### Task 10: Release download, README, backlog truth, e2e

**Files:** `README.md` (top level), `app/README.md`, `scripts/package-release.sh` (new), `docs/feature-backlog.md`, `crates/vizctl/tests/e2e.rs`

- [ ] **Step 1: Write `scripts/package-release.sh`.** It runs `make-app.sh` and then `ditto -c -k --keepParent dist/VizDAW.app dist/VizDAW-26.0.0.zip`, taking the version from the workspace as in Task 1, and prints the SHA-256. It never publishes; publishing a GitHub release is the owner's step.
- [ ] **Step 2: Add a README "Download and install" section.**
  - Download `VizDAW-<version>.zip` from the GitHub release and unzip it.
  - Move VizDAW to Applications and open it once.
  - macOS says it cannot verify the developer, because 26.0.0 is not signed with a Developer ID yet. Open **System Settings › Privacy & Security**, choose **Open Anyway** for VizDAW, and confirm. You only need to do this once.
  - Note that `vizdaw-mcp` lives inside the app, at `VizDAW.app/Contents/MacOS/vizdaw-mcp`.
  - Building from source stays as it is.
- [ ] **Step 3: Update the backlog.**
  - Mark "Co-Producer over MCP" and "Crossfade the waveform drawing" **Done**.
  - Mark "A built-in VizDAW EQ after 1.0" **Superseded (plan 6g): Multi-Q ships in the EQ slot; an EQ of VizDAW's own is the "Multi-Q 2" row.**
  - Mark "Rename or delete a take that is not playing, from the Takes menu" **Built; owner's eye check left**.
  - Apply the controller's backlog-truth table (path given in the dispatch).
  - Mark this plan's rows Done, or keep their leftovers as in Tasks 5 and 9.
  - Add a row "Sign VizDAW and vizdaw-mcp with Developer ID, notarize" under 27.1.0 or "when the owner has a Developer ID", merging it with the existing after-1.0 signing row.
- [ ] **Step 4: Add e2e cases:**
  - `co_producer_import_midi_is_refused`, over the Co-Producer socket through vizdaw-mcp;
  - `control_close_of_a_dirty_song_does_not_prompt` (make an edit, then `file.close`; the window count drops, and the call returns within the normal timeout).
- [ ] **Step 5: Confirm.** Run the full verification: `cargo test --workspace`, Linux clippy, `scripts/swift.sh test`, `scripts/make-app.sh`, the no-brand build (`VIZDAW_NO_BRAND=1` with a fresh `--scratch-path`), the e2e, and `scripts/package-release.sh`.
- [ ] **Step 6: Commit** as `docs(release): download and install, backlog truth pass, e2e for the gate fixes`.
