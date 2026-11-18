# Co-Producer Chores Implementation Plan (8c)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The owner's first chores work end to end through Co-Producer. "Export all tracks as a DAWproject" and "bounce to stems" are each one `export` call that writes into a new dated folder, logs the files in the song's notes, and never replaces a file. Quantize, transpose and velocity changes take explicit values and explicit tracks. Markers can be added, renamed and removed. Every change to the composer's notes or markers shows exactly what changes and waits for their yes. A bounce placed on new tracks joins the request's undo step. Three MCP Apps views show the change preview, the track list and the export result, each with a plain-text fallback.

**Architecture:**
- **`core-control`.**
  - Six registry commands:
    - three note chores, `modify.quantize_notes`, `modify.transpose_notes` and `modify.velocity_notes`, each class change and previewable;
    - three marker commands: `marker.add` (add), `marker.rename` (change, previewable) and `marker.remove` (remove, previewable).
  - One protocol command on the Co-Producer socket, `co_producer.export`.
  - `file.export_midi_clip` for several tracks writes into a folder.
- **`core-model`.**
  - `Command::ModifyNotes` applies a `Modify` to named tracks and a range, never to the selection.
  - `diff` learns which note values changed, and the markers.
- **`core-ffi`.**
  - A new `chores` module builds the core-model command for each new registry command. The preview and the apply share it, so the two cannot differ.
  - `chores` also turns a `co_producer.export` into a dated folder plus the one outward command the menus run, checked by 8a's `check_output`.
  - Jobs started inside a request land inside it.
  - `state.get document` shows the markers.
- **Swift.**
  - `ControlCommands` runs the six new commands through one Session call.
  - `CoProducerCommands` runs `co_producer.export` as an outward command and answers when its job ends. It records the files the job really wrote.
- **`vizdaw-mcp`.**
  - Two new tools, `export` and `show_tracks`.
  - The MCP Apps extension (`io.modelcontextprotocol/ui`, stable revision 2026-01-26) with three `ui://` views. A view is shown only to a host that declares the extension; every tool keeps its text answer.

**Tech Stack:**
- Rust 2024: `core-control`, `core-model`, `core-ffi` with UniFFI 0.32.2, `vizdaw-mcp`, `vizctl`.
- Swift 6.1 / SwiftPM with Swift Testing, macOS 14+.
- The views are three self-contained HTML files with plain inline JavaScript, built into `vizdaw-mcp` with `include_str!`.
- **No new third-party dependency**, in Rust or in the views.

**Spec:** `docs/superpowers/specs/2026-11-14-co-producer-design.md`, approved by the owner on 2026-11-14, is binding. This plan builds its §12 row **8c**:
- §10's chores table, end to end;
- §10's gaps 7 (direct Modify with parameters) and 10 (markers);
- §6.3's change-preview, track-list and export-result views;
- §7.3's dated folders, used by every export.

It builds on plan 8a, `docs/superpowers/plans/2026-11-14-co-producer-8a.md`, whose Rulings 1–13 and pre-flight rulings P1–P11 (`.superpowers/sdd/2026-11-14-co-producer-8a/preflight.md`) stay in force. It also builds on plan 8b, `docs/superpowers/plans/2026-11-16-co-producer-8b.md`, and its Rulings 1–10. Executors read all three.

**Not in this plan:**
- The mixer snapshot view, mixer editing and the installed plug-in list: **8d**.
- The automation preview view, automation points, fade shapes and writing notes: **8e**.
- A confirm button inside the change-preview view (Ruling 7). It is a backlog row.
- Marks on markers. A marker Co-Producer added still needs a yes to rename or remove (Ruling 4). It is a backlog row.
- Previews for the panel-driven `modify.*` commands, `region.*` fades and normalize. Those commands work on the selection, which a preview must never depend on (8a Ruling 6). The new commands in Ruling 2 cover the chores.
- Bouncing a time selection through `export`. Claude can still do it with `export_folder` and `file_bounce`.

## Plan Rulings

These are this plan's decisions where the spec, 8a or 8b leaves a choice. Each one names its cost if wrong.

1. **One `export` call per chore (spec §10, §7.3).**
   - **The protocol command.** `co_producer.export {what, parent?, strips?, window?}` needs an open request.
     - `what` is one of `stems mix midi dawproject aaf other_daw`. These are 8b's `ExportWhat` without `selection`, which is `invalid_params` "Bounce a selection with export_folder and file_bounce.".
     - `strips` is strip IDs, comma-separated, 1–64 unique, each ≥ 1. It is allowed only with `stems` and `other_daw`; with any other `what` it is `invalid_params` "strips is only for stems and other_daw.".
     - `parent` is checked as 8b's `export_folder` checks it, by `check_output(parent, Output::IntoFolder)` in `admit`.
   - **The plan.** `chores::export_plan` works out everything before it makes anything. So a refusal leaves no empty folder behind. In order, it:
     1. finds the parent by 8b Ruling 6's order;
     2. works out the strips: the given ones, else `core_io::for_daw::default_strips(doc, seq)` for the shown sequence, as File › Export › For Another DAW does. An empty list is `VizError::Model` "There is nothing to bounce: no strip plays audio.";
     3. builds the one command the menus run, with `<folder>` the new dated folder and `<song>` the song part of its name (8b Ruling 6's cleaning, made `pub(crate)` as `notes::clean_song`):

        | `what` | command | args |
        |---|---|---|
        | `stems` | `file.bounce` | `source: "stems"`, `strips`, `path: <folder>` |
        | `mix` | `file.bounce` | `source: "mix"`, `path: <folder>/<song>.wav` |
        | `midi` | `file.export_midi` | `path: <folder>/<song>.mid` |
        | `dawproject` | `file.export_dawproject` | `path: <folder>/<song>.dawproject` |
        | `aaf` | `file.export_aaf` | `path: <folder>` |
        | `other_daw` | `file.export_for_daw` | `path: <folder>`, `strips` when given |

        No plan ever sets `replace`, `dry_run` or `add_to_project`;
     4. runs `registry::validate_args` on the args, and `check_output` on the planned path by `co_producer::output(id, args)`, as if that path had already been made. It runs the same check again once the folder exists;
     5. only then makes the folder, by 8b's `make_export_folder`, which never replaces a folder.
   - **Same checks as the menus.** The app runs the planned command through `ControlCommands.run`, the path the menu's own handlers use. So the app's write-time checks apply too, and the new folder starts empty, so no file in it can clash.
   - **The answer.** It comes when the job ends: `{doc, folder, written, summary}`.
     - `written` lists the files the job wrote: a bounce's files, or an export job's `written`;
     - `summary` is the interchange report's summary for DAWproject, AAF and MIDI, else `null`.
     - A failed job answers its failure as 8a Ruling 13 does. The folder stays, with whatever was already written, and the notes' "Files" line names those files.
   - **Cost if wrong:** a composer who wanted only some stems names the strips. Without them, the strips are the ones For Another DAW uses.
2. **Note chores with explicit values (spec §10 gap 7).** All three commands share the target params:
   - `seq` (Int ≥ 1, required);
   - `lanes`: track IDs, comma-separated, 1–64 unique, required;
   - `start` and `end` (Int ≥ 0, optional): both or neither, with `end > start`. When they are left out, the command covers the whole track.

   The commands:
   - **`modify.quantize_notes`** takes `grid` (Choice, required) and `strength` (Int 1–100, default 100). The grids are `1/4 1/8 1/16 1/32 1/8t 1/16t`, which are 15360, 7680, 3840, 1920, 5120 and 2560 ticks at `PPQ` 15 360. It builds `Modify::Quantize(Quantize::Grid { grid, strength })`.
   - **`modify.transpose_notes`** takes `semitones` (Int −48..=48, not 0). It builds `Modify::Transpose(Transpose::Interval { semitones })`.
   - **`modify.velocity_notes`** takes `op` (Choice `set add scale`, required) and `amount` (Int, required). It builds `Modify::Velocity`:
     - `set`: amount 1–127, as `ValueOp::Set(input::upscale_velocity(amount))`;
     - `add`: amount −126..=126 and not 0, as `ValueOp::Add(amount << 9)`;
     - `scale`: amount 10–400 percent, as `ValueOp::ScalePercent(amount)`.
   - **How they run.** `Command::ModifyNotes { seq, lanes, range, modify }` runs `select::apply` with a selection built from those tracks' events in the range, mode `Select`, and no rule, so `Modify` touches only the events it fits. It leaves `History`'s selection and `last_modify` alone, so Modify Again never repeats a Co-Producer chore. A track that is not in `seq`, or that has no notes in range, is `ModelError::Invalid` "Track <id> has no notes there.".
   - **The menus and the panel.** All three are `key_only`, menu "Modify", with the titles "Quantize Notes", "Transpose Notes" and "Change Velocity". Outside a request their undo labels are `Modify::name()`'s: "Quantize", "Transpose" and "Velocity".
   - **Cost if wrong:** a swing or groove quantize waits for a backlog row.
3. **Every note or marker change needs a yes (spec §6.5, the owner's "destructive edits").** For all five previewable commands, `preview_items` sets `composers_work: true`, even on a Co-Producer track. 8e is the first plan to mark notes, so until then no one can tell Co-Producer's notes from notes the composer recorded on its track (8b Ruling 2's reason). So `needs_yes` is always true for these, and the yes comes through elicitation or the chat (8a Ruling 8).
   - **Cost if wrong:** one extra question when Co-Producer tidies notes on its own track.
4. **Markers (spec §10 gap 10).** The markers of a sequence are kept sorted by tick, with ties in insertion order, and `index` is 1-based in that order. Each command builds one `SequenceCmd::SetMarkers`:
   - **`marker.add {seq, tick: Int ≥ 0, name}`**, class add. A marker with the same tick and name already there is `ModelError::Invalid` "There is already a marker "<name>" there.";
   - **`marker.rename {seq, index: Int ≥ 1, name}`**, class change, previewable;
   - **`marker.remove {seq, index: Int ≥ 1}`**, class remove, previewable. An index past the end is `ModelError::Invalid` "Sequence <seq> has N markers.".
   - **Names** are 1–64 characters after trimming, with 8a P11's character rules for request names.
   - **Labels and menus.** The undo labels are "Add Marker", "Rename Marker" and "Remove Marker". All three are `key_only`, menu "Transport".
   - **State.** `state.get document` gains `"markers": [{"index", "tick", "name"}]` for the selected sequence.
   - **A plan runs in order.** The preview runs on its clone in the same order as the apply, so a rename then a remove in one plan name the indexes as they stand at each step.
   - **Cost if wrong:** with no marks on markers, a marker Co-Producer just added takes a yes to rename.
5. **What the preview says about notes and markers (spec §6.6).** `diff::describe` adds two kinds of item.
   - **Notes on a track present before and after.** Notes are matched by `EventId` over all takes. The item counts the matched notes whose start, pitch, velocity or length changed, and names the values that changed, in that order: `"4 notes changed (start, velocity)"`. It joins the existing parts with ", ", so `"12 notes → 10 notes, 4 notes changed (pitch)"` can appear.
   - **Markers, per sequence, after its tracks.** The thing is `"Marker <i>, <name>"`, with the position before for a removal or rename and after for an add. The change is "added", "deleted", `renamed to "<new>"` or "moved". Markers are matched by equal (tick, name) first. A remaining pair at the same index and tick is a rename, and the rest are deletions and additions.
   - **Safe text.** Names pass `diff::shown_name`, as 8a's do.
   - **Cost if wrong:** "4 notes changed (start)" does not say by how much. The preview names the grid in its plan, and Undo takes it back.
6. **Jobs inside a request land inside it (8a Ruling 13's cost line; 8a's backlog row for 8c).**
   - **Which jobs.** A job started while `History::acting()` is true is Co-Producer's: `jobs::Job` gains `co_producer: bool`, set at spawn. These are `media.import`, `file.import_midi`, and a bounce or For Another DAW export with `add_to_project`.
   - **Landing.** When `apply_jobs_after_edit` lands such a job's document change while that document's request is still open, the landing sets `acting` for itself alone, inside the same lock, and restores it before the lock is released. It then joins "Co-Producer: <Title>", or its "(continued)" group after a hand edit, and 8b's marks settle as for any acting step, so bounced tracks are named "… (Co-Producer)".
   - **When it cannot join.** If the request has ended, or a hand transaction or a recording group is open, the job lands as it does today, as its own hand step. Nothing is lost.
   - **Cost if wrong:** a bounce that ends after an idle close becomes its own undo step, as in 8a.
7. **MCP Apps views (spec §6.3, §11.2).** 8a's research found the MCP Apps extension stable (revision 2026-01-26), so the spec §5 caution about a developer preview no longer applies. The text fallback stays.
   - **Negotiation.** Views are on for a client that declares `capabilities.extensions["io.modelcontextprotocol/ui"].mimeTypes` containing `"text/html;profile=mcp-app"`. It declares it in `initialize`, or in the 2026-07-28 era in each request's `_meta` `clientCapabilities`. `vizdaw-mcp` then declares `extensions: {"io.modelcontextprotocol/ui": {}}` in its own capabilities.
   - **What views add.** With views on:
     - `tools/list` adds `_meta: {ui: {resourceUri}}` to three tools: `change` (`ui://vizdaw/change`), `show_tracks` (`ui://vizdaw/tracks`) and `export` (`ui://vizdaw/export`);
     - `resources/list` adds those three resources, with `mimeType` `text/html;profile=mcp-app`;
     - the three tools' results add `structuredContent` (Ruling 8).

     Without views, none of these appear. `resources/read` serves a view's HTML to anyone who asks: it holds no song data.
   - **The views.**
     - They hold no data of their own. Each one sends `ui/initialize`, then `ui/notifications/initialized`, and draws from the `structuredContent` of `ui/notifications/tool-result`.
     - Every name is set with `textContent`, never `innerHTML`.
     - No view loads anything from the network. A resource has no `csp`, so the host applies its restrictive default, and `_meta.ui.prefersBorder` is `true`.
     - Colours follow the host's `hostContext.theme` (light or dark) through two built-in palettes. VizDAW's own theme is not sent over the socket. That is "where practical" (spec §6.3), and a backlog row covers it.
   - **No confirm button.** The change preview shows the list and the state of the question. It has no button. The yes still comes through elicitation, or in the chat with the token (8a Ruling 8), so it always comes from the composer and never from a view's call. The spec allows "at most" a confirm button, and a backlog row covers adding one.
   - **The export result's "Show in Finder"** sends `ui/open-link {url: "file://<folder>"}` with the path percent-encoded. When the host refuses, the view shows "Open it in Finder: <folder>". VizDAW itself never launches Finder.
   - **Cost if wrong:** a host that shows views but ignores `hostContext.theme` gets the light palette.
8. **Tool answers (spec §6.1, §11.12).**
   - **`export {what, parent?, strips?}`** sends `co_producer.export`. It waits for the answer as long as `vizdaw-mcp` waits for any `command.run` of an outward command.
     - Its text is "Wrote N files in <folder>:" plus one "- <path>" line per file (at most 40, then "- and N more"), then "Report: <summary>" when there is a summary.
     - Its `structuredContent` is `{view: "export", what, folder, written, summary}`.
     - Its description, exactly: "Export the song in one step into a new dated folder, "<song> — <what> — <date>": stems (one audio file per strip; pass strips to choose them), mix (one audio file), midi (one MIDI file), dawproject (a DAWproject file that other music apps open), aaf, or other_daw (stems plus MIDI files). Without parent it uses the folder remembered in the song's notes, else the folder the song is in; for an untitled song with neither, ask the composer where exports should go and pass parent. Never replaces a file. Needs an open request (begin_request). Answers what was written."
   - **`show_tracks {}`** is read only. It sends `state.get document` and `state.get mixer`.
     - Its text is "Song: <title>", then "Tracks:" with one "- Track <n>, <name>: <kind>" line per track of the selected sequence, then "Strips:" with one "- <name>: <kind_label>, colour <colour or none>" line per strip. A line for something Co-Producer added ends " — added by Co-Producer". Each list stops at 200 lines, then "- and N more".
     - Its `structuredContent` is `{view: "tracks", title, tracks: [{seq, lane, n, name, kind, co_producer}], strips: [{id, name, kind, colour, co_producer}]}`, with the same 200-line caps.
     - Its description, exactly: "List the song's tracks and mixer strips, with their kinds and colours and which ones Co-Producer added. Changes nothing."
     - Names in either form have 8a's `disguises` characters replaced by U+FFFD.
   - **`change`.** Its `structuredContent` is `{view: "change", state, items: [{thing, change, composers_work}], undo_label}`, where `state` is one of:
     - `"ask_in_chat"`, for the token fallback;
     - `"applied"`;
     - `"said_no"`, for a decline, a cancel, `allow: false`, or no answer;
     - `"applied_without_asking"`, when `needs_yes` was false.

     The token is never in `structuredContent`, only in the text 8a returns. `change`'s description adds "quantizing, transposing or changing the velocity of notes, and renaming or removing a marker" to its examples.
   - **The registry tools.** `marker_add` appears through 8a Ruling 12, since it is class add. The five change and remove commands are run only through `change`.
   - **Cost if wrong:** a song with more than 200 tracks lists only the first 200, and says so.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift.**
  - Build only through `scripts/build-ffi.sh` (after any FFI change) and `scripts/swift.sh build|test`, plus `scripts/make-app.sh` in Tasks 4 and 7. Never run `swift`, `xcodebuild` or `xcrun` directly.
  - Swift 6.1:
    - no `[weak self]`, and no non-Sendable value captured inside `MainActor.assumeIsolated`;
    - no `@State` or `@FocusState`: view state lives in the `@Observable` models.
  - Every new control has an accessibility label, and colours, fonts and spacing come from theme tokens (`theme.text…`, `theme.type…`, `theme.spacing…`).
- **Tests never touch the machine.**
  - They never open real audio or MIDI, and never trigger the microphone or TCC prompt.
  - They never touch the real `~/Library`: Application Support/VizDAW, preferences, the Trash, `Audio/Plug-Ins`. Use temp folders, `MemoryDefaults()`, the e2e's throwaway home, and fakes. Trash moves in tests go through `debug_set_trash_dir`.
  - Socket tests use a private temp folder under `/tmp`, as `server.rs`'s `private_tmp` does. `vizdaw-mcp` tests set `VIZDAW_SUPPORT_DIR` on the child they spawn, never in their own process.
  - Every `core-ffi` test file calls `force_offline()` through its `serial` helper, and points the support folder at a temp folder as `co_producer_copies.rs` does.
  - Swift tests that make a real `Session` set `TestSupportDir.shared`.
  - Tests that load real plug-ins run in a child process whose `HOME` and `CFFIXED_USER_HOME` are a temp folder, through `crates/core-host/tests/support/temp_home.rs`. This plan adds none.
  - Linux CI has no AU or CoreFoundation, so any test that needs either is `#[cfg(target_os = "macos")]`. `vizdaw-mcp`, the `core-control` tests and the `core-model` tests are plain Unix and run on Linux.
  - The e2e uses vizctl's throwaway home.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`.
  - Never read the owner's off-limits research folder outside this repo, which 8b's Global Constraints name. Its path holds a banned name, so this plan does not spell it.
- **Programmatic calls open nothing.** Control-channel and Co-Producer calls, and programmatic calls, never open dialogs, sheets, alerts, panels, menus or windows. They also never launch another app: "Show in Finder" is the host's `ui/open-link`, never VizDAW's.
- **Hostile input.**
  - Every MCP input is hostile: every MCP message, every tool argument, every Co-Producer call, every file Co-Producer reads, and every name a view draws.
  - Every new FFI export runs inside `ffi_guard` (and so `catch_unwind`). It returns `Result<_, VizError>`, or a safe default for the never-failing ones.
  - `vizdaw-mcp` handles each message inside `std::panic::catch_unwind`. A panic answers `-32603` and the server goes on.
  - Each new command and call gets hostile-input tests: wrong types, unknown params, missing required params, a 70 000-character string, a NUL, IDs out of range.
  - Every item a call skips or leaves behind gets a report or problem line.
- **Files.** Every chore that writes files goes through the same checks as the menus: 8a's `check_output`, then the menu's own handler. No chore ever replaces an existing file, and no chore sets `replace`.
- **The composer's yes.** Every chore that changes or removes existing work is previewable and runs only through `co_producer.apply` after a yes (8a Rulings 6–8; Ruling 3). A direct `command.run` of one on the Co-Producer socket stays `not_allowed`.
- **Counts.** Check every count at the start of Task 1, and report each task's change against it.
  - The registry grows by **6**, from the count measured at the start. It is expected to be 190, after 8b's +4.
  - `COMMAND_NAMES` grows by **1**, expected 21 → 22. `ErrorCode::ALL` stays as measured, expected 16.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list. The list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`); read them, keep all new text clear of them, and never copy the list.
  - The list includes the feature's old working name, which plan 8a added to the scans (spec, "Name"). Like every banned name, it is never written in the repo, this plan included.
  - Notes entries, export folder and file names, undo names, tool descriptions and the views' text are covered by the same rule. Task 6 adds `crates/vizdaw-mcp/views` to `names.rs`'s scanned folders.
  - The feature is "Co-Producer" everywhere.
  - Text the app or a view shows says "track", never "lane". Identifiers and JSON keys keep `lane`.
- **Commits.**
  - Stay on branch `takes-and-co-producer`. Never switch branches. Never push: the owner is asked before every push.
  - Commit at the end of every task by explicit paths, never `git add -A`. Never commit `.superpowers/`, `.DS_Store`, `target/`, generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`) or `dist/`.
  - Every message ends with exactly these two lines:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
  - Check each commit with `git log -1 --oneline && git show --stat HEAD`.
  - `Cargo.lock` does not change.
- **Brand.** Brand assets are never in the repo. `VIZDAW_NO_BRAND=1` builds without them.
- **Lints.**
  - Every crate keeps `#![deny(missing_docs)]` and the `clippy::{unwrap_used, expect_used, indexing_slicing, panic}` denials outside tests. `vizdaw-mcp` keeps `#![forbid(unsafe_code)]`.
  - `core-model` stays free of OS APIs, apart from the `std::fs` that `file.rs` already uses. The export plan is `core-ffi`'s.
- **Format.** This plan changes no project format. `core_model::file::VERSION` stays as measured at the start, expected 10.
- **TDD.** Every new or changed test is run and seen to fail before the code that makes it pass.
- **Each task ends green.**
  - Always:
    - `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches.
  - Tasks 1, 2, 3 and 4 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Known flaky: the `damaged_files` and `dawproject_fuzz` proptests, `platform-macos/tests/midi.rs`, `core-ffi` catalog tests under load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline: check at start.** At the start of Task 1, measure and record:
    - HEAD, which is 8b's last commit on `takes-and-co-producer`, or this plan's pre-flight commit if there is one;
    - the registry count (expected 190);
    - `COMMAND_NAMES` (expected 21) and `ErrorCode::ALL` (expected 16);
    - the last README step (expected 355, after 8b Task 6);
    - the e2e's ignored tests (expected 34);
    - `VERSION` (expected 10).

    If 8a and 8b are not both fully committed, stop and say so. If a measured value differs from the expected one, use the measured value and report the difference.

## Review Focus

These are the five failure modes a composer is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **An export that could land on something that exists.**
   - Examples:
     - a second "bounce to stems" the same day;
     - a song titled `"../Mix"`, `"a/b"` or `".hidden"`;
     - a remembered export folder that is now a link, or inside the package;
     - `strips` naming a strip that does not exist;
     - a file appearing at the planned path between the plan and the job.
   - Expected:
     - the second export makes "… 2";
     - the names are cleaned;
     - a bad parent is refused with no folder made;
     - a bad strip is refused before any folder is made;
     - nothing is ever replaced.
   - Pinned by Task 3's `export_plans_are_new_and_checked`, `a_refused_plan_makes_no_folder` and Task 1's `export_refuses_hostile_input`.
2. **A bounce placed on new tracks ends after the request.**
   - Examples:
     - it ends while the request is open;
     - it ends after a hand edit;
     - it ends after `end_request` or the idle close.
   - Expected:
     - inside the request's step, with its tracks named "… (Co-Producer)";
     - in the "(continued)" step;
     - as its own step, unmarked, with nothing lost.
   - Pinned by Task 3's `jobs_land_inside_their_request` and `a_job_after_its_request_lands_on_its_own`.
3. **Odd targets for a note chore.**
   - Examples:
     - a track of another sequence, an audio track, or the same id twice;
     - `start == end`, or `start` alone;
     - transposing a pitch-127 note up;
     - a hand edit between the preview and the yes.
   - Expected:
     - a clear refusal for the first two;
     - the preview names the clamped result exactly as the apply makes it;
     - an apply after the hand edit is refused with 8a's "The song changed since the preview; preview again.".
   - Pinned by Task 1's `note_chores_refuse_hostile_input`, Task 2's `note_chores_refuse_odd_targets` and `previews_match_what_the_apply_does`.
4. **Several marker steps in one plan.**
   - Examples:
     - rename marker 2, then remove marker 1;
     - remove index 9 of 3;
     - two markers at one tick;
     - a name holding a bidi mark.
   - Expected:
     - each step names the index as it stands then, and the preview and the apply agree;
     - index 9 is refused, and nothing in the plan runs;
     - the bidi name is refused.
   - Pinned by Task 2's `marker_plans_run_in_order` and Task 1's `marker_commands_refuse_hostile_input`.
5. **A view meets hostile or unusual data.**
   - Examples:
     - a track named `<img src=x onerror=alert(1)>`;
     - 500 tracks;
     - a host without MCP Apps;
     - the 2025-03-26 protocol version.
   - Expected:
     - names draw as text;
     - lists stop at 200 with "and N more";
     - with no extension, no `_meta.ui`, no `ui://` resources and no `structuredContent`, and the text is the same.
   - Pinned by Task 6's `views_draw_names_as_text`, `views_only_for_hosts_that_ask` and Task 5's `long_lists_are_capped`.

---

### Task 1: The registry and protocol additions (`core-control`)

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (the six entries; `NOTE_TARGET_PARAMS`, `GRIDS`);
  - `crates/core-control/src/request.rs` (`COMMAND_NAMES`, `co_producer.export`, `MAX_MARKER_NAME`);
  - `crates/core-control/src/co_producer.rs` (`PREVIEWABLE`; `admit` for `co_producer.export`; `id_list`; `output` for `file.export_midi_clip`);
  - `crates/core-control/tests/co_producer.rs`, `tests/commands.rs`, `tests/protocol.rs`;
  - `crates/core-ffi/src/control.rs` (the `ControlRequest` variant and its mapping, so the workspace builds; the Session work is Tasks 2 and 3's);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (the six ids in `handled`, answering `.failed(.of(.unavailable, "Not ready yet."))` until Task 4);
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (`co_producer.export` answers the same until Task 4).

**Interfaces:**
- Consumes: 8a's `co_producer::{admit, check_output, output, Output, PREVIEWABLE}` and `registry::validate_args`, and 8b's `ExportWhat`/`ExportWhatRow`.
- Produces:
  ```rust
  // request.rs
  pub const MAX_MARKER_NAME: usize = 64;
  // Command gains:
  CoProducerExport { what: ExportWhat, parent: Option<PathBuf>, strips: Option<Vec<u64>>, window: Option<WindowRef> },
  // co_producer.rs
  /// "1,2,3": 1–64 unique ids, each ≥ 1; the message names the rule broken.
  pub fn id_list(text: &str) -> Result<Vec<u64>, String>;
  pub const PREVIEWABLE: &[&str] = &["track.rename", "track.delete", "edit.undo", "edit.redo", "file.save",
      "mixer.set_colour", "modify.quantize_notes", "modify.transpose_notes", "modify.velocity_notes",
      "marker.rename", "marker.remove"];
  ```
  In `core-ffi/src/control.rs`, `ControlRequest` gains `CoProducerExport { what: ExportWhatRow, parent: Option<String>, strips: Option<Vec<u64>>, window }`.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it at the top of the task report.
- [ ] **Step 2: Write the failing tests.**
  - In `tests/co_producer.rs`:
    - `chore_commands_have_their_class`:
      - the three `modify.*_notes` and `marker.rename` are `Change`, `marker.remove` is `Remove`, and `marker.add` is `Add`;
      - `list_json()` lists all six, with `previewable: true` for the five in `PREVIEWABLE` and `false` for `marker.add`.
    - `chores_only_through_a_preview`:
      - `command.run modify.transpose_notes {seq: 1, lanes: "1", semitones: 2}` on the Co-Producer socket is `not_allowed` with 8a Ruling 6's direct-call sentence, and so are `marker.rename` and `marker.remove`;
      - a `co_producer.preview` plan holding each is admitted;
      - `command.run marker.add {seq: 1, tick: 0, name: "Verse"}` is admitted.
    - `note_chores_refuse_hostile_input`. Each is `invalid_params`:
      - `lanes`: missing, `""`, `"1,1"`, `"0"`, `"a"`, 65 ids, or 70 000 characters;
      - `start` without `end`, or `start: 10, end: 10`;
      - `grid: "1/5"`, `strength: 0` or `101`;
      - `semitones: 0` or `49`;
      - `op: "set", amount: 0`, `op: "add", amount: 0` or `127`, `op: "scale", amount: 9` or `401`;
      - an unknown param.
    - `marker_commands_refuse_hostile_input`. Each is `invalid_params`:
      - `name` that is `""`, 65 characters, holds a NUL, `"a\u{202E}b"` or `"a\u{2028}b"`;
      - `index: 0`, or `tick: -1`.
    - `export_refuses_hostile_input`:
      - each of these is `invalid_params` with Ruling 1's sentence where it has one: `what: "selection"`, `what: "video"`, `what` missing, `strips` with `what: "dawproject"`, `strips: "1,1"`, or a `parent` of 70 000 characters;
      - a `parent` that is relative, holds `..`, is a file, is a link to a folder, or is inside a temp `X.vizdaw` is `not_allowed` with the reason;
      - an existing plain temp folder, and no `parent`, are admitted.
    - `export_only_on_the_co_producer_socket`: on a developer server it is `not_allowed` "only on the Co-Producer socket".
    - `midi_clips_for_several_tracks_go_into_a_folder`. `output("file.export_midi_clip", …)` is:
      - `NewFile(["midi2"])` with `lanes: "3"`;
      - `IntoFolder` with `lanes: "3,4"` or no `lanes`.

      An existing folder path with `lanes: "3,4"` is admitted, and an existing file is `not_allowed`.
  - In `tests/commands.rs`: the registry count is the baseline plus 6, `COMMAND_NAMES.len()` is the baseline plus 1, and the minimal-shape list gains `co_producer.export`.
  - In `tests/protocol.rs`: the round-trip parse of `co_producer.export {what: "stems", parent, strips: "2,3"}` to `strips: Some(vec![2, 3])`.
- [ ] **Step 3: Run** `cargo test -p core-control`. Expected: FAIL.
- [ ] **Step 4: Implement** Rulings 1, 2 and 4's params and classes, and the `PREVIEWABLE` list.
  - **Registry entries.** All six are `key_only`, with `document: true`. The menus and titles are in Rulings 2 and 4, and `marker.*`'s titles are "Add Marker", "Rename Marker" and "Remove Marker".
  - **Checks.** `validate` parses the shapes: the cross-param rules (`start`/`end`, `semitones ≠ 0`, `amount` by `op`) and `lanes` through `id_list`. Marker names reuse 8a P11's character check.
  - **The two other crates.** `core-ffi/src/control.rs` maps the new command. Swift gets the temporary arms named in Files, so `everyRegisteredMenuCommandHasAHandler` passes.
- [ ] **Step 5: Run** `cargo test -p core-control -p core-ffi`. Expected: PASS, including `names.rs`.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(control): Co-Producer note chores, markers and one-call export, with midi clips into a folder (plan 8c Task 1)`.

### Task 2: Note chores, markers and their previews (`core-model`, `core-ffi`)

**Files:**
- Create: `crates/core-ffi/src/chores.rs`, `crates/core-ffi/tests/chores.rs`, `crates/core-model/tests/modify_notes.rs`.
- Modify:
  - `crates/core-model/src/command/mod.rs` (`Command::ModifyNotes`, its `name` and `apply_in` arm);
  - `crates/core-model/src/history.rs` (leave `selection` and `last_modify` alone for `ModifyNotes`);
  - `crates/core-model/src/diff.rs` (Ruling 5);
  - `crates/core-model/tests/diff.rs`;
  - `crates/core-ffi/src/co_producer.rs` (the five arms of `preview_items`, through `chores::command_for`);
  - `crates/core-ffi/src/session.rs` (`run_chore`);
  - `crates/core-ffi/src/control.rs` (`markers` in `state.get document`);
  - `crates/core-ffi/src/lib.rs`;
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the stub).

**Interfaces:**
- Consumes: Task 1's ids and params; 8a's `Planned`, `ArgValue`, `preview_items`, `diff::shown_name`; 8b's `ChangeItem.co_producers`; `core_ffi::input::upscale_velocity`.
- Produces:
  ```rust
  // core-model command/mod.rs
  Command::ModifyNotes { seq: SeqId, lanes: Vec<LaneId>, range: Option<(Tick, Tick)>, modify: Modify },
  // core-ffi chores.rs
  /// The core-model command for one of Task 1's six ids, built from `doc` (markers need it).
  pub fn command_for(doc: &Document, id: &str, args: &[(String, ArgValue)]) -> Result<Command, VizError>;
  pub const CHORE_IDS: &[&str] = &["modify.quantize_notes", "modify.transpose_notes", "modify.velocity_notes",
      "marker.add", "marker.rename", "marker.remove"];
  // session.rs
  impl Session {
      /// Runs one of CHORE_IDS through `edit`, as the menus' edits run.
      pub fn run_chore(&self, id: String, args: Vec<ControlArg>) -> Result<DocSnapshot, VizError>;
  }
  ```

- [ ] **Step 1: Write the failing `core-model` tests.**
  - In `tests/modify_notes.rs`, on a fixture with two MIDI tracks of four notes each, and one track selected:
    - `modify_notes_touches_only_its_tracks`:
      - `ModifyNotes` transposing track 2 by +2 changes only track 2's pitches;
      - the history's selection is unchanged, and `last_modify` is still `None`;
      - the undo label is "Transpose".
    - `modify_notes_keeps_to_the_range`: a quantize to 3840 ticks over `[0, 15360)` moves only the notes that start there.
    - `note_chores_refuse_odd_targets`: a lane of another sequence, an audio-only lane, and a range with no notes each give `ModelError::Invalid` "Track <id> has no notes there.", and the document is unchanged.
    - `transposing_clamps`: a pitch-127 note transposed +2 stays 127.
  - In `tests/diff.rs`:
    - `note_changes_are_items`:
      - quantizing 4 notes gives "Track 2, Bass" / "4 notes changed (start)";
      - a velocity set on 3 notes whose velocity differed gives "3 notes changed (velocity)";
      - a change of start and velocity together gives "(start, velocity)".
    - `marker_changes_are_items`:
      - an add gives "Marker 2, Chorus" / "added";
      - a rename gives "Marker 1, Verse" / `renamed to "Intro"`;
      - a removal gives "Marker 3, Outro" / "deleted";
      - two markers at one tick, with one renamed, give exactly one rename item.
- [ ] **Step 2: Run** `cargo test -p core-model --test modify_notes --test diff`. Expected: FAIL.
- [ ] **Step 3: Implement** `Command::ModifyNotes` (Ruling 2) and the diff items (Ruling 5).
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS, including 8a's `co_producer_requests` proptest.
- [ ] **Step 5: Write the failing `core-ffi` tests** in `tests/chores.rs`, through a `serial` helper, on a saved session with two MIDI tracks of notes and markers "Verse"@0, "Chorus"@15360, "Outro"@30720.
  - `chores_build_the_right_commands`:
    - `quantize_notes {grid: "1/16t"}` builds a 2560-tick grid at strength 100;
    - `velocity_notes {op: "set", amount: 100}` builds `Set(upscale_velocity(100))`;
    - `{op: "add", amount: -10}` builds `Add(-5120)`;
    - `marker.add` with tick 7680 inserts between Verse and Chorus.
  - `previews_match_what_the_apply_does`:
    - for each previewable chore, inside a request: `co_producer_preview` gives `needs_yes: true` and every item `composers_work: true` (Ruling 3), even on a track Co-Producer added;
    - `co_producer_take` then `run_chore` with the same args gives a document whose `diff::describe` from the start equals the preview's items;
    - a hand rename between the preview and the take makes the take fail with "The song changed since the preview; preview again.".
  - `marker_plans_run_in_order`:
    - the plan `[marker.rename {index: 2, name: "Hook"}, marker.remove {index: 1}]` previews "Marker 2, Chorus: renamed to "Hook"" then "Marker 1, Verse: deleted";
    - applying it leaves "Hook"@15360 and "Outro"@30720;
    - a plan with `marker.remove {index: 9}` fails at preview with "Sequence 1 has 3 markers." and changes nothing.
  - `markers_reach_the_state`: `state.get document` JSON has `"markers": [{"index": 1, "tick": 0, "name": "Verse"}, …]`.
  - `run_chore_refuses_hostile_input`: an unknown id, a missing arg, `lanes` of 70 000 characters, and `u64::MAX` as `seq` are each `Err`, with no panic and no change.
- [ ] **Step 6: Run** `cargo test -p core-ffi --test chores`. Expected: FAIL.
- [ ] **Step 7: Implement.**
  - **`command_for`** reads the args by name, as 8a's `track_command` does.
  - **`preview_items`** gains one arm for the five previewable ids: build with `command_for(h.doc(), …)`, execute on the clone, then `rows(diff::describe(…), true)`.
  - **`run_chore`** builds with `command_for` on the present document and runs it through `edit`.
  - **State.** `state.get document` gains the markers.
  - **Swift.** `FakeSession` gains the stub.
- [ ] **Step 8: Run** `cargo test -p core-model -p core-ffi`. Expected: PASS, including 8a's `co_producer` and 8b's `notes`.
- [ ] **Step 9: End-of-task checks**, including the Swift build and test. **Commit** `feat(model): note chores on named tracks and markers, with exact previews (plan 8c Task 2)`.

### Task 3: The export plan, and jobs that land in their request (`core-ffi`)

**Files:**
- Modify:
  - `crates/core-ffi/src/chores.rs` (`export_plan`);
  - `crates/core-ffi/src/notes.rs` (`clean_song` made `pub(crate)`);
  - `crates/core-ffi/src/session.rs` (`co_producer_export_plan`; the job flag at spawn; the acting landing in `apply_jobs_after_edit`);
  - `crates/core-ffi/src/jobs.rs` (`co_producer: bool`);
  - `crates/core-ffi/src/control.rs` (`ControlResult::CoProducerExported` and its JSON);
  - `crates/core-ffi/tests/chores.rs`;
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the stub).

**Interfaces:**
- Consumes:
  - Task 1's `ControlRequest::CoProducerExport` and `ExportWhatRow`;
  - 8b's `notes::make_export_folder`, the parent order (Ruling 6) and the request log;
  - 8a's `co_producer::{output, check_output}`, `registry::validate_args` and `co_producer_enter`;
  - `core_io::for_daw::default_strips`.
- Produces:
  ```rust
  #[derive(uniffi::Record)] pub struct ExportPlanRow { pub folder: String, pub id: String, pub args: Vec<ControlArg> }
  impl Session {
      /// Ruling 1: works out, checks, then makes the dated folder; needs an open request.
      pub fn co_producer_export_plan(&self, what: ExportWhatRow, date: String,
          parent: Option<String>, strips: Option<Vec<u64>>) -> Result<ExportPlanRow, VizError>;
  }
  // control.rs: ControlResult::CoProducerExported { doc: String, folder: String, written: Vec<String>, summary: Option<String> }
  //   result_json: {"doc", "folder", "written", "summary"}
  ```

- [ ] **Step 1: Write the failing tests** in `tests/chores.rs`, on a saved song "Night Drive" with two audio tracks, the date "2026-11-16", and `parent` a temp folder.
  - `export_plans_are_new_and_checked`:
    - `dawproject` gives `file.export_dawproject` with `path` "<parent>/Night Drive — DAWproject — 2026-11-16/Night Drive.dawproject", and the folder exists and is empty;
    - `stems` with no `strips` gives `file.bounce` with `source: "stems"`, `strips` the `default_strips` ids joined by ",", and `path` the folder;
    - a second `stems` gives "… 2026-11-16 2";
    - `mix`, `midi`, `aaf` and `other_daw` give Ruling 1's table exactly;
    - no plan's args hold `replace`, `dry_run` or `add_to_project`;
    - each planned path passes `check_output` by `output(id, args)`.
  - `a_refused_plan_makes_no_folder`:
    - `strips: [999]` is `Err` naming the strip;
    - a song whose only track plays external MIDI, with `stems`, is `Err` "There is nothing to bounce: no strip plays audio.";
    - a remembered parent inside the package is `Err` with the `IntoFolder` reason.

    In each case `parent` holds no new folder.
  - `odd_song_names_are_cleaned`: the titles `"../Mix"`, `"a/b"` and `".hidden"` give the folders "..-Mix — …", "a-b — …" and "hidden — …", and file names to match.
  - `export_needs_a_request`: with no open request it is `Err` "Open a request first (begin_request).".
  - `jobs_land_inside_their_request`:
    - in a request, enter; start `bounce` with stems and `add_to_project: true` into a temp folder; leave; wait for the job; `apply_jobs_after_edit`; end the request;
    - the undo label is "Co-Producer: <Title>". One undo removes the new tracks with the rest of the request;
    - the new lanes are marked and named "… (Co-Producer)" (8b Ruling 1).

    With a hand rename between the leave and the landing, the landing is in the "(continued)" step.
  - `a_job_after_its_request_lands_on_its_own`: the same bounce, with `co_producer_end` before the job lands, lands as its own step under its usual label, with unmarked tracks, and no step is lost.
  - `export_result_json`: `CoProducerExported` serialises to `{"doc", "folder", "written", "summary"}`.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test chores`. Expected: FAIL.
- [ ] **Step 3: Implement** Rulings 1 and 6.
  - **The parent.** `export_plan` takes 8b's parent order from the same code `co_producer_export_folder` uses, and 8b's log records "- Export folder:" for a given `parent`, as `export_folder` does.
  - **The job flag.** It is set wherever a job is spawned from `History::acting()`.
  - **The landing.** It uses the same `set_acting` as `co_producer_enter`, under one lock.
  - **Swift.** `FakeSession` gains the stub.
- [ ] **Step 4: Run** `cargo test -p core-ffi`. Expected: PASS.
- [ ] **Step 5: End-of-task checks**, including the Swift build and test. **Commit** `feat(ffi): one-call exports into new dated folders, and jobs that land inside their Co-Producer request (plan 8c Task 3)`.

### Task 4: Chores in the app (Swift)

**Files:**
- Modify:
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (the six handlers through `runChore`, replacing Task 1's arms);
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (`co_producer.export`; recording written files);
  - `app/Tests/VizDAWAppModelTests/ControlCommandsTests.swift`, `CoProducerCommandsTests.swift`, `FakeSession.swift`.

**Interfaces:**
- Consumes: Task 2's `runChore(id:args:)`, Task 3's `coProducerExportPlan(what:date:parent:strips:)` and `CoProducerExported`, 8b's `coProducerFile(path:)` and `stamp(now)`, and 8a's deferred job answers (`ControlCommands.waitMs`).
- Produces: the handlers. `CoProducerCommands` answers `co_producer.export` with `.coProducerExported(doc:folder:written:summary:)`.

- [ ] **Step 1: Write the failing Swift tests** with `FakeSession`.
  - `ControlCommandsTests`:
    - `choresRunThroughOneCall`: each of the six ids calls `runChore` with its id and args unchanged;
    - a throwing `runChore` is `.failed` with the session's message.
  - `CoProducerCommandsTests`:
    - `exportRunsThePlannedCommand`:
      - `co_producer.export {what: "stems"}` calls `coProducerExportPlan` with `stamp(now)`'s first 10 characters as the date;
      - it then runs the planned `file.bounce` through `ControlCommands.run`, between `coProducerEnter` and `coProducerLeave`;
      - when the job ends, it answers `folder`, the bounce's file paths as `written`, and `summary` nil.
    - `exportOfADawprojectAnswersItsReport`: a finished `exportDawproject` job answers its `written` and the report's summary.
    - `writtenFilesAreRecorded`:
      - every path in a finished job's `written` (or a bounce's files) goes to `coProducerFile(path:)`, once each;
      - an outward `command.run` records the job's written files, not its `path` arg, which replaces 8b's rule;
      - a failed job records only what it had already written.
    - `aRefusedPlanRunsNothing`: a throwing `coProducerExportPlan` answers `.failed` with its message and runs no command.
    - `noSheetsForChores`: after a failing export job, the document has no alert (8a Ruling 13).
- [ ] **Step 2: Run** `scripts/build-ffi.sh && scripts/swift.sh test --filter 'ControlCommands|CoProducerCommands'`. Expected: FAIL.
- [ ] **Step 3: Implement.** The export runs as 8a's `runCommand` does for an outward command, and so as the menus do. Its deferred answer is built from the finished job.
- [ ] **Step 4: Run** the Step 2 command, then `scripts/swift.sh test`. Expected: PASS, including `NameScanTests` and `noLaneInWhatTheAppShows`.
- [ ] **Step 5: End-of-task checks**, including `scripts/make-app.sh`. **Commit** `feat(app): Co-Producer chores and one-call exports in the app, recording the files really written (plan 8c Task 4)`.

### Task 5: `vizdaw-mcp`: the `export` and `show_tracks` tools

**Files:**
- Modify:
  - `crates/vizdaw-mcp/src/lib.rs`, `src/tools.rs`;
  - `crates/vizdaw-mcp/tests/server.rs`, `tests/support/fake_app.rs`.

**Interfaces:**
- Consumes:
  - Task 1's `co_producer.export` and the six registry entries in `list_json()`;
  - Task 3's answer `{doc, folder, written, summary}`;
  - `state.get document` (with Task 2's markers) and `state.get mixer`, with 8b's `co_producer` fields.
- Produces:
  ```rust
  // tools.rs
  pub const EXPORT: &str = "export";
  pub const SHOW_TRACKS: &str = "show_tracks";
  pub const MAX_SHOWN: usize = 200;
  /// Ruling 8's text and structuredContent for each tool; views (Task 6) read the second.
  pub fn export_answer(result: &Value) -> (String, Value);
  pub fn tracks_answer(document: &Value, mixer: &Value) -> (String, Value);
  pub fn change_view(state: &str, items: &[Value], undo_label: Option<&str>) -> Value;
  ```

- [ ] **Step 1: Write the failing tests** in `tests/server.rs`. The fake app answers `co_producer.export` with `{doc: "d1", folder: "/x/Song — Stems — 2026-11-16", written: ["/x/…/Kick.wav", "/x/…/Bass.wav"], summary: null}`, and scripted `state.get` answers.
  - `chore_tools_are_listed`:
    - `export` and `show_tracks` have Ruling 8's descriptions;
    - `export`'s `what` is an `enum` of the six names, `parent` has `maxLength` 1024, and `strips` has `maxLength` 512;
    - `show_tracks` has `readOnlyHint: true`;
    - `marker_add` is listed;
    - no tool is named `modify_quantize_notes`, `modify_transpose_notes`, `modify_velocity_notes`, `marker_rename` or `marker_remove`;
    - `change`'s description holds Ruling 8's new examples.
  - `export_reaches_the_app`:
    - `export {what: "stems"}` sends `co_producer.export {what: "stems"}`;
    - its text starts "Wrote 2 files in /x/Song — Stems — 2026-11-16:" and has one "- " line per file;
    - an answer with `summary` adds "Report: <summary>";
    - an app failure is `isError: true` with its message.
  - `show_tracks_lists_tracks_and_strips`:
    - the text has "Song: <title>", "Tracks:", "Strips:" and Ruling 8's lines;
    - a Co-Producer track's line ends " — added by Co-Producer".
  - `long_lists_are_capped`: 500 tracks give 200 lines then "- and 300 more", and 45 written files give 40 then "- and 5 more".
  - `chore_tools_refuse_hostile_arguments`. Each is `isError: true` before any socket call:
    - `export` with `what` missing, `"selection"`, a number, an unknown property, or a 70 000-character `parent`;
    - `show_tracks` with any property.
  - `names_are_shown_safely`: a track named `"a\u{202E}b"` shows as "a\u{FFFD}b" in the text.
- [ ] **Step 2: Run** `cargo test -p vizdaw-mcp`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 8's tools and answers. `structuredContent` is built but not yet sent: Task 6 sends it to hosts that ask.
- [ ] **Step 4: Run** `cargo test -p vizdaw-mcp -p core-control`. Expected: PASS, including `names.rs`'s scan of `vizdaw-mcp/src`.
- [ ] **Step 5: End-of-task checks. Commit** `feat(mcp): export in one call and show_tracks (plan 8c Task 5)`.

### Task 6: `vizdaw-mcp`: the MCP Apps views

**Files:**
- Create:
  - `crates/vizdaw-mcp/src/views.rs`;
  - `crates/vizdaw-mcp/views/bridge.js`, `views/change.html`, `views/tracks.html`, `views/export.html`.
- Modify:
  - `crates/vizdaw-mcp/src/lib.rs`, `src/rpc.rs` (the capability, per era);
  - `crates/vizdaw-mcp/tests/server.rs`;
  - `crates/core-control/tests/names.rs` (scan `crates/vizdaw-mcp/views`).

**Interfaces:**
- Consumes: Task 5's `export_answer`, `tracks_answer`, `change_view`; 8a's era handling in `rpc.rs` and the `change` tool's four outcomes.
- Produces:
  ```rust
  // views.rs
  pub const EXTENSION: &str = "io.modelcontextprotocol/ui";
  pub const MIME: &str = "text/html;profile=mcp-app";
  pub const VIEWS: &[(&str, &str)] = &[("ui://vizdaw/change", "change"), ("ui://vizdaw/tracks", "show_tracks"),
      ("ui://vizdaw/export", "export")];   // (uri, tool)
  /// True when `caps` (a client's capabilities) lists MIME under EXTENSION's mimeTypes.
  pub fn wants_views(caps: &Value) -> bool;
  /// The HTML for `uri`, with bridge.js put in place of `<!--BRIDGE-->`; None for another uri.
  pub fn html(uri: &str) -> Option<String>;
  ```

- [ ] **Step 1: Write the failing tests** in `tests/server.rs`, in the `initialize` era (with `capabilities.extensions` set, or not) and in the 2026-07-28 era (with `_meta` `clientCapabilities`).
  - `views_only_for_hosts_that_ask`:
    - with the extension:
      - the server's capabilities have `extensions["io.modelcontextprotocol/ui"]`;
      - `tools/list` gives `change`, `show_tracks` and `export` `_meta.ui.resourceUri` from `VIEWS`;
      - `resources/list` has the three `ui://` resources with `mimeType` `MIME`;
      - each of the three tools' results carries `structuredContent` with its `view`.
    - without it, and with `mimeTypes: ["text/html"]` only, and at protocol 2025-03-26:
      - none of these appear;
      - the text content is byte for byte the same.
  - `views_are_served`: `resources/read ui://vizdaw/tracks` gives one `text` item with `mimeType` `MIME`, `_meta.ui.prefersBorder: true` and no `csp`. It begins `<!DOCTYPE html>`, and holds no `<!--BRIDGE-->` and no other `<!--` marker. `ui://vizdaw/nope` is `-32002`.
  - `views_draw_names_as_text`. Over the three HTML files and `bridge.js`:
    - none holds `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval(`, `new Function`, `http:`, `https:`, `src=` or `<link`;
    - each holds `textContent`.
  - `views_speak_the_extension`: `bridge.js` holds `"ui/initialize"`, `"ui/notifications/initialized"` and `"ui/notifications/tool-result"`. `export.html` holds `"ui/open-link"` and the text "Open it in Finder: ".
  - `change_view_states`. The `structuredContent.state` is:
    - `"ask_in_chat"` for the token fallback, and the token appears only in the text content;
    - `"applied"` after an elicitation accept;
    - `"said_no"` after a decline;
    - `"applied_without_asking"` when `needs_yes` was false.
- [ ] **Step 2: Run** `cargo test -p vizdaw-mcp`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 7.
  - **`bridge.js`** holds the JSON-RPC over `postMessage` with `window.parent`: it numbers requests, answers nothing it was not asked, and ignores messages from any source but `window.parent`.
  - **Each view** draws its `structuredContent`:
    - change: a list of thing, change and "your work" rows, with a line for the state: "Say yes in the chat to apply these changes, or no to leave everything as it is.", "Applied with your yes.", "You said no. Nothing changed." or "Applied: only Co-Producer's own things.";
    - tracks: two lists, with a "Co-Producer" tag on added things;
    - export: the folder, the files, the report, and "Show in Finder".
  - **Theme.** The light and dark palettes follow `hostContext.theme`.
  - **What a view says without data.** A view that receives no result, or an `isError` result, shows the text content as it is.
- [ ] **Step 4: Run** `cargo test -p vizdaw-mcp -p core-control`. Expected: PASS, including `names.rs` with the views folder.
- [ ] **Step 5: End-of-task checks. Commit** `feat(mcp): MCP Apps views for the change preview, the track list and the export result, with text fallbacks (plan 8c Task 6)`.

### Task 7: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (new steps after the last one);
  - `crates/vizctl/tests/e2e.rs`;
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–6. No production code, unless a step finds a bug. Such a bug is fixed in its owner's module, with a regression test.

- [ ] **Step 1: Add the e2e** `co_producer_chores_over_mcp`, `#[ignore]`, in the style of 8b's `co_producer_librarian_over_mcp` and with 8a's MCP helper. Its MCP client sends `initialize` with `elicitation: {}` and `extensions: {"io.modelcontextprotocol/ui": {mimeTypes: ["text/html;profile=mcp-app"]}}`.
  - **Start.** Launch the app, then switch Co-Producer on over the developer channel. Run `file.create_demo`, then `file.save {path: <home>/Song.vizdaw}`. The demo song has MIDI tracks with notes.
  - **"Export all tracks as a DAWproject."**
    1. `begin_request {name: "export all tracks as a dawproject", asked: "export all tracks as a dawproject"}`;
    2. `export {what: "dawproject", parent: <home>}`;
    3. `end_request`.

    Then:
    - `<home>/Song — DAWproject — <today>/Song.dawproject` exists and is not empty;
    - the answer's text names it;
    - `Notes.md`'s new entry has a "- Files: " line naming it, and "- Export folder: <home>".
  - **"Bounce to stems."**
    1. `begin_request {name: "bounce to stems"}`;
    2. `export {what: "stems"}`, with no `parent`;
    3. `export {what: "stems"}` again;
    4. `end_request`.

    Then:
    - "<home>/Song — Stems — <today>" holds one audio file per `default_strips` strip, and nothing else;
    - the second export made "… <today> 2";
    - `structuredContent.view` is `"export"`;
    - nothing outside those two folders changed (compare a listing of `<home>` before and after).
  - **A note chore with a yes.**
    1. `begin_request {name: "transpose the bass"}`;
    2. `change` with `modify.transpose_notes {seq, lanes: <a demo MIDI track>, semitones: 2}`. This emits `elicitation/create` whose message has the line "- Track <n>, <name>: <k> notes changed (pitch)". Answer `decline`: the result is "The composer said no. Nothing changed.", its `structuredContent.state` is `"said_no"`, and the document version is unchanged;
    3. the same `change` again, answered `accept` with `allow: true`, applies it;
    4. `end_request`.

    The undo label is then "Co-Producer: Transpose the Bass".
  - **Markers.**
    1. In a request, `marker_add {seq, tick: 0, name: "Intro"}` applies with no question;
    2. `change` with `marker.rename {seq, index: 1, name: "Top"}` asks, and an accept renames it;
    3. `state.get document`'s `markers` shows "Top".
  - **A bounce placed on new tracks.**
    1. In a request, `file_bounce {source: "stems", strips: <one strip>, path: <home>, add_to_project: true}`;
    2. `end_request` after the job's answer.

    The new track is named "… (Co-Producer)", and one `edit.undo` over the developer channel removes it along with the rest of that request.
  - **The track list view.**
    - `tools/list`'s `show_tracks` has `_meta.ui.resourceUri` "ui://vizdaw/tracks";
    - `resources/read` of it starts `<!DOCTYPE html>`;
    - `show_tracks`' `structuredContent.tracks` has the demo's track count, with `co_producer: true` on the bounced track.
  - **The gate.** A raw client on the Co-Producer socket gets `not_allowed` for:
    - `command.run modify.quantize_notes {seq: 1, lanes: "1", grid: "1/16"}`;
    - `command.run marker.remove {seq: 1, index: 1}`;
    - `co_producer.export {what: "midi", parent: <home>/Song.vizdaw}`;
    - `command.run file.export_midi_clip` with `lanes` naming two tracks and `path` an existing file.
- [ ] **Step 2: Write the README steps** in `app/README.md`, after the last step, under a heading "Plan 8c, Co-Producer chores:". Each step states what to do and what is seen, and says "track":
  1. On a saved song, ask Claude to "export all tracks as a DAWproject". A folder "<song> — DAWproject — <date>" appears next to the song, holding "<song>.dawproject". Open that file in another music app: the tracks are there. Window › Song Notes shows the entry, with the file under "Files".
  2. Ask Claude to "bounce to stems". A folder "<song> — Stems — <date>" holds one audio file per strip. Ask again: a second folder "… 2" appears, and the first is untouched.
  3. Tell Claude to put exports in another folder, then ask for a mix. The mix goes there, and the next export goes there too, without asking.
  4. Ask Claude for an AAF and for "a folder for another DAW": each lands in its own dated folder, and Claude reports what did not transfer.
  5. Ask Claude to "quantize the bass to sixteenths". It first shows exactly which track and how many notes change. Say no: nothing changes. Ask again and say yes: the notes move, as one undo step "Co-Producer: …".
  6. Ask Claude to transpose a track up two semitones, and to set the velocities of another to 100. Each asks first, and Undo takes each back.
  7. Ask Claude to add markers "Verse" and "Chorus": they appear at once. Ask it to rename "Chorus" to "Hook": it asks first.
  8. Ask Claude to bounce a track's stem onto a new track. The new track is named "… (Co-Producer)", has the badge, and one Undo removes it with the rest of the request.
  9. In Claude's desktop app or another host that shows MCP Apps views, ask Claude to "show my tracks". A small list shows each track and strip, with Co-Producer's marked. Ask for a change to your work: a preview shows each item before you answer. After an export, a result view shows the files and "Show in Finder".
  10. In a host without views, the same requests answer in plain text with the same lists.
  11. Ask Claude to export into the song's own package, or over an existing file: it is refused, and nothing is written.
- [ ] **Step 3: Update the backlog** (`docs/feature-backlog.md`).
  - In the "Co-Producer over MCP" row, note 8c's delivery:
    - `export` in one call into dated folders;
    - the three note chores and the three marker commands;
    - jobs landing in their request;
    - the three MCP Apps views.
  - Close 8a's rows "A job that ends after its Co-Producer call … lands outside the request's undo step" and "Exports into several files at once through Co-Producer (`file.export_midi_clip` …)".
  - Note against "Previews for the remaining change and remove commands" that 8c added five, and that 8d and 8e remain.
  - **New rows:**
    - "A confirm button in the change-preview view, once the host can show that the click came from the composer";
    - "Views drawn in VizDAW's own theme colours rather than the host's light or dark";
    - "Marks on markers, so Co-Producer can tidy its own markers without a yes";
    - "Groove and swing quantize, and duration and legato, as Co-Producer chores";
    - "A velocity floor of 1 for Co-Producer's add and scale, so no note becomes a note-off";
    - "Bounce a time selection through `export`";
    - "The mixer snapshot view" — 8d;
    - "The automation preview view" — 8e;
    - any narrow edge cases the task reports listed, at the owner's review pace.
- [ ] **Step 4: Run the gates:**
  - `cargo test --workspace`;
  - both clippies;
  - `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `scripts/build-ffi.sh`;
  - `scripts/swift.sh build`;
  - `scripts/swift.sh test`;
  - the no-brand build, `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <fresh temp dir>`;
  - `scripts/make-app.sh`;
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: the baseline's ignored tests plus one run, and all pass.

  Then `pgrep -lf MacOS/VizDAW` and `pgrep -lf vizdaw-mcp` print nothing. Report against the baseline: registry +6, `COMMAND_NAMES` +1, error codes unchanged, README +11, e2e +1, `VERSION` unchanged.
- [ ] **Step 5: Commit** `docs(app): Co-Producer chores README checks, e2e and backlog (plan 8c Task 7)`.

## Self-Review

1. **Spec coverage (the §12 row 8c).**
   - **The existing exports and bounces end to end, with dated folders and notes** (§10's table, §7.3):
     - DAWproject, stems, mix, MIDI, AAF and For Another DAW go through `export` (Rulings 1 and 8; Tasks 1, 3, 4 and 5);
     - a bounce of a selection still goes through `export_folder` and `file_bounce` (8b), and a backlog row covers adding it to `export`;
     - several MIDI clips go into a folder (Task 1);
     - "Bounce onto new tracks" joins the request (Ruling 6, Task 3);
     - "What survives an export" is the report summary in `export`'s answer.
   - **The owner's examples.** "Export all tracks as a DAWproject" and "bounce to stems" are the e2e's first two requests and README steps 1–2.
   - **Direct Modify with parameters** (§10 gap 7): Ruling 2, Task 2.
   - **Markers** (§10 gap 10): Ruling 4, Task 2.
   - **Changes need the composer's yes** (§6.5, §6.6): Ruling 3, Task 2's `previews_match_what_the_apply_does`, and the e2e's decline then accept.
   - **Files are never replaced, and checks are the menus'** (§6.7): Ruling 1's order and Task 3's tests; the gate rows in the e2e.
   - **The views** (§6.3): change preview, track list and export result (Ruling 7, Task 6), each with a text fallback (§5). The mixer snapshot and automation preview are 8d's and 8e's, by the spec's split.
   - **Deferred to 8c by 8a's ledger and backlog:** the jobs row and the several-files row are covered. Previews for five more commands are added.
   - **Deferred to 8c by 8b:** exports end to end with the dated folders, the real "Files" of a numbered export, and the views.
2. **Steps.** Every test step names its tests and assertions with the plan's values: ids, grids in ticks, sentences, folder and file names, and caps. Code steps give signatures and point to the ruling. The tool descriptions and the views' text are written out, because the spec leaves them to the plan (§11.12).
3. **Type consistency.** These names are spelled the same in every task:
   - `Command::ModifyNotes`, `chores::{command_for, CHORE_IDS, export_plan}`, `notes::clean_song`;
   - `Session::{run_chore, co_producer_export_plan}`, `ExportPlanRow`, `ControlResult::CoProducerExported`, `ControlRequest::CoProducerExport`;
   - `co_producer::id_list`, `MAX_MARKER_NAME`, `PREVIEWABLE`;
   - `tools::{EXPORT, SHOW_TRACKS, MAX_SHOWN, export_answer, tracks_answer, change_view}`, `views::{EXTENSION, MIME, VIEWS, wants_views, html}`;
   - the ids `modify.quantize_notes`, `modify.transpose_notes`, `modify.velocity_notes`, `marker.add`, `marker.rename`, `marker.remove`, `co_producer.export`, and the URIs `ui://vizdaw/change`, `ui://vizdaw/tracks`, `ui://vizdaw/export`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 1, 2, 3, 5 and 6.
5. **Proportion.** Seven tasks, as the owner asked for 26.0.0 by 2026-11-20. Code appears only as signatures and the exact text the spec leaves to the plan. The plan reuses a lot:
   - `select::apply` for the note chores;
   - `SetMarkers` for markers;
   - 8b's folder code and notes log, and 8a's `check_output` and preview clone;
   - the menus' own handlers for every write;
   - `default_strips` for the stems;
   - 8a's MCP helper in the e2e.
6. **Owner decisions.** None are open. Four calls are stated for the owner's review:
   - every note or marker change needs a yes, even on Co-Producer's own track (Ruling 3);
   - the change-preview view has no confirm button, so the yes stays in elicitation or the chat (Ruling 7);
   - views follow the host's light or dark, not VizDAW's theme (Ruling 7);
   - stems with no strips named use For Another DAW's default strips (Ruling 1).
