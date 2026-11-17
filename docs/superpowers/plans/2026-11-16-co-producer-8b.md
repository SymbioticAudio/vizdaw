# Co-Producer the Notes File and the Librarian Implementation Plan (8b)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each song keeps a `Notes.md` that the composer and Co-Producer both write: Co-Producer logs one entry per request, reads the notes first, and only ever appends. What Co-Producer adds is named, coloured, badged and marked until the composer takes it over (Make Mine, or a rename). Exports get clearly named, dated folders. An untitled song's notes and copies move into its package on its first save. Compact Media offers to remove old Co-Producer copies, listing each with its size, and removes nothing without the composer's yes.

**Architecture:**
- **`core-control`.** Four registry commands (`track.make_mine`, `mixer.make_mine`, `mixer.set_colour`, `window.song_notes`). Three protocol commands on the Co-Producer socket (`co_producer.notes`, `co_producer.note`, `co_producer.export_folder`), and `co_producer.begin` gains `asked`. `mixer.set_colour` becomes previewable.
- **`core-model`.** Format version 10: tracks (lanes), strips, sends and plug-in slots gain a `co_producer` mark. A new `marks` module settles marks after every step in `History::execute`, as `fill_new_eqs` already post-processes a step: an acting step marks what it made and gives tracks and strips the "(Co-Producer)" name; a hand step never makes marked things and takes over a marked track or strip it renames. `diff::ChangeItem` learns whether its thing was Co-Producer's own.
- **`core-ffi`.** Rows and state JSON carry the mark, and Make Mine is a Session call. Previews stop asking for a yes for Co-Producer's own things. A new `notes` module holds the notes file, the request's log and its entry, the dated export folder, and the first-save move-in of an untitled song's notes and copies. The Session also lists and removes copies for Compact's offer.
- **`vizdaw-mcp`.** New tools `read_notes`, `add_note` and `export_folder`. `begin_request` takes `asked`, and its first answer per song carries the notes.
- **Swift.** Window › Song Notes; the Co-Producer badge and colour on track headers and mixer strips; Track › Make Mine and the strip's Make Mine; the four registry handlers; Compact's copy-removal sheet; and `CoProducerCommands` running the new calls and recording the files outward commands wrote.

**Tech Stack:**
- Rust 2024: `core-control`, `core-model`, `core-ffi` with UniFFI 0.32.2, `vizdaw-mcp`, `vizctl`.
- Swift 6.1 / SwiftPM with Swift Testing, macOS 14+.
- **No new third-party dependency.**

**Spec:** `docs/superpowers/specs/2026-11-14-co-producer-design.md`, approved by the owner on 2026-11-14, is binding. This plan builds its §12 row **8b** (§7 in full; §6.7's copies offer and untitled projects). It builds on plan 8a, `docs/superpowers/plans/2026-11-14-co-producer-8a.md`, whose Rulings 1–13 and pre-flight rulings P1–P11 (`.superpowers/sdd/2026-11-14-co-producer-8a/preflight.md`) stay in force. Executors read both.

**Not in this plan:**
- Marks on regions, notes and automation that Co-Producer writes onto the composer's tracks. No 8b command writes them; **8e**, which first does, adds the marks with them (spec §7.2's last line).
- Exports end to end through Co-Producer with the dated folders, and the "Files" a numbered export subfolder holds: **8c**. 8b makes the folder and logs the paths it is given.
- The MCP Apps views, including the track list that shows the marks: **8c**.
- A Settings export location for untitled songs (spec §7.3). No such setting exists today, so an untitled song with no remembered folder answers "ask the composer" (Ruling 6). That is spec §7.3's own fallback. A backlog row covers it.
- Save As of a saved song to a new package carrying `Notes.md` along, and stand-in folders left by untitled songs closed without saving: backlog rows.

## Plan Rulings

These are this plan's decisions where the spec or 8a leaves a choice. Each one names its cost if wrong.

1. **The marks (spec §7.2, §11.8).**
   - **Fields.** `Lane`, `Strip`, `AuxSend` and `PluginSlot` each gain `co_producer: bool`, with `#[serde(default, skip_serializing_if = "std::ops::Not::not")]`, so an unmarked project's JSON is unchanged.
   - **Format.** `file::VERSION` goes from 9 to 10, with an identity `migrate_v9_to_v10`, so an older build refuses a version-10 file with its `NewerFormat` message. This is the next free version (spec §5): 8a kept 9.
   - **Settling.** `marks::settle(before, after, acting)` runs on every `History::execute` and `amend` step, after the command and `fill_new_eqs`:
     - **acting:** everything new in `after` is marked: a lane or strip whose id is not in `before`, a send whose (strip, target) pair is not, and a plug-in slot whose instance id is not. Every marked lane and strip whose name does not end with `" (Co-Producer)"` gets it appended;
     - **hand:** everything new is unmarked, so a hand Duplicate Track of a Co-Producer track is the composer's. A marked lane or strip whose name changed is unmarked: that is the take-over by rename.
     - The preview's clone runs with `acting` on (8a `preview_items`), so a preview names the suffixed name that the apply will make.
   - **Make Mine.** `LaneCmd::MakeMine { seq, lane }` unmarks the lane and the strip it plays through (`marks::lane_strip`), and that strip's sends and plug-in slots. `MixerCmd::MakeMine { strip }` unmarks a strip, its sends and its slots. Both drop a trailing `" (Co-Producer)"` from the names they unmark. The undo name is "Make Mine". Making an unmarked thing mine changes nothing, so it adds no step.
   - **Cost if wrong:** a composer who wants to keep the "(Co-Producer)" text after Make Mine types it back.
2. **Co-Producer's own things need no yes, unless work could be lost (spec §6.5).** `ChangeItem` gains `co_producers: bool`. It is true when the track or strip the item names was marked in `before`, and the item takes away no regions and no notes: not "deleted, with …", and not a falling count. `preview_items` sets `composers_work = !item.co_producers` for `track.rename`, `track.delete` and `mixer.set_colour`, and keeps 8a's Undo and Redo rule.
   - **Why.** Spec §7.2 lets the composer record and edit on a Co-Producer track without taking it over. Deleting that track would then delete their regions with no yes, which is what the owner winces at (spec §1).
   - **Cost if wrong:** one extra question when Co-Producer tidies away its own track after putting audio on it.
3. **Colour and badge (spec §7.2, §11.9; the track colours gap).**
   - **The colour.** A marked track or strip with no colour of its own draws the theme's `accent.accent` as its colour bar: `StripPalette.bar` takes its own colour first, then the mark, then the kind. Every theme has that token, so no theme file changes, and the symbiotic-design repo is not touched.
   - **The badge.** `AppIcon.coProducerBadge` draws the Symbiotic role `.controlLink` from the glyph set, as P10's Settings icon does. It is 12 pt, in `accent.accent`, at the end of the track header's name row and beside the strip's name. Its accessibility label is "Added by Co-Producer". Sends and slots show no badge (spec §7.2).
   - **The colour gap.** Strips have had colours since 7b, and the track header's Colour menu sets the track's strip colour. What was missing is a control path: `mixer.set_colour {strip, colour}`, class change, previewable, with `colour` one of `none red orange yellow green teal blue purple pink`. The preview item is "Strip <name>" / "colour <old> → <new>", using those words.
   - **Cost if wrong:** in a theme whose accent is close to a swatch, a Co-Producer bus looks like a coloured one, but the badge still tells them apart.
4. **The notes file (spec §7.1, §11.10).**
   - **Where.** `<package>/Notes.md`. For an untitled song it is `<support>/Co-Producer/Untitled/<project id>/Notes.md`, in the stand-in folder of P9, whose project ID is stamped first as `copies_home(true)` does.
   - **Making it.** The notes file is made with `create_new`, holding `"# <song name>\n"`, the first time Window › Song Notes, a read or an entry needs it. An existing one is never replaced.
   - **Refusals.** A `Notes.md` that is a link, a folder or anything but a regular file is refused: "Notes.md is not a plain file, so Co-Producer leaves it alone.". It is checked with `symlink_metadata` before every open; an `O_NOFOLLOW` open waits on 8a's backlog row.
   - **Reading.** At most the first 1 MiB (`notes::MAX_READ`) is read, decoded as lossy UTF-8. `truncated` says when there was more.
   - **Appending.** An append opens the file with `OpenOptions::append`, so it never truncates or rewrites, whatever another editor did in between. It first re-reads the file's last byte: when the file does not end in `"\n"`, it writes one, then a blank line, then the entry. Co-Producer never rewrites or deletes notes text, its own or the composer's.
   - **Undo.** Notes are outside the document, so Undo leaves the entries.
   - **Cost if wrong:** an editor that holds the whole file and saves it later can drop Co-Producer's newest entry. That is the editor's usual behaviour with any outside change; the composer's text is never lost.
5. **One entry per request (spec §7.1).**
   - **When.** An entry is written when a request ends, by `end_request`, the idle close or the switch going off, provided it entered at least once (8a `co_producer_enter`). A request that only read writes none.
   - **The log.** The Session keeps a `notes::RequestLog` from begin to end:
     - each `co_producer_enter` keeps the present document, and each `co_producer_leave` appends `describe(entered, present)`'s items, flagged `yes` when that call followed a take whose preview had `needs_yes`. So only Co-Producer's own steps are logged, never the composer's hand edits made between calls (spec: "The composer's own edits are not logged");
     - the log also keeps the labels `end_request` and the "(continued)" closes made, the files recorded by `co_producer_file`, the lines of `co_producer_note`, the export folder chosen, and the copy's path relative to the package or stand-in.
   - **The entry, exactly:**
     ```markdown
     ## 2026-11-14 15:02 — Set Up a Drum Bus
     - Asked: "put my drums on a bus with some parallel compression"
     - Added: Strip Drums (Co-Producer): added; Strip Drum Crush (Co-Producer): added
     - Changed (yes in chat): Track 1, Kick: renamed to "Kick In"
     - Deleted (yes in chat): Track 3, Old Hats: deleted, with 4 notes
     - Changed: Strip Drums (Co-Producer): colour none → blue
     - Files: /Users/x/Exports/Night Drive — Stems — 2026-11-14
     - Export folder: /Users/x/Exports
     - Note: the composer wants the drums dry in the verse
     - Undo step: "Co-Producer: Set Up a Drum Bus"
     - Saved copy: Co-Producer/Copies/2026-11-14 15-02 before Set Up a Drum Bus.vizdaw
     ```
     - **The heading** is the first enter's stamp with `HH-MM` shown as `HH:MM`, then " — " and the request's title.
     - **Item lines.** Items are sorted into Added (change "added"), Deleted (change starting "deleted") and Changed (the rest). Yes-flagged items go in their own "(yes in chat)" line. Each item is "<thing>: <change>", joined by "; ", at most 20 per line, then "; and N more".
     - **Optional lines.** Absent parts have no line. A request with no items, no files and no notes gets "- Nothing changed.".
     - **Safe text.** Every name in the entry has already passed `diff::shown_name`. `asked` and note lines are shown the same way, so no line can start a heading or forge another entry.
   - **A failed write** leaves the request ended as usual and adds the problem line "Song notes not written: <reason>." to the snapshot.
   - **Cost if wrong:** a request that touches many things gets a long entry, capped at 20 items a line.
6. **Dated export folders (spec §7.3).** `co_producer.export_folder {what, parent?}` needs an open request. It makes and answers a new folder.
   - **The name.** `<song> — <what> — <date>`.
     - The song is the document's title, with `/` and `:` turned to `-`, a leading `.` dropped, controls removed, and the result cut to 100 characters.
     - `what` is a choice: `stems mix selection midi dawproject aaf other_daw`, shown as "Stems", "Mix", "Selection", "MIDI", "DAWproject", "AAF" and "Other DAW".
     - The date is "YYYY-MM-DD" in local time, passed by the app, never by Claude.
     - A taken name gets " 2", " 3" and so on, through `create_dir`, which never replaces. It gives up after 999.
   - **The parent:**
     1. `parent` when given. The entry then gets "- Export folder: <parent>", which is how the folder is remembered per song (spec: "remembered per song in the notes");
     2. else the last line of `Notes.md` that reads exactly `- Export folder: <absolute path>`;
     3. else the folder the saved package is in;
     4. else, for an untitled song, `VizError::Model` "Ask the composer where exports should go, then call export_folder with parent.".
   - **Checks.** Each parent, remembered ones included, passes 8a's `check_output(parent, Output::IntoFolder)`: absolute, no `..`, an existing directory that is not a link, and not inside any `.vizdaw` package by its canonical path. `admit` checks a given `parent` before the app sees it, and the Session checks the remembered one again.
   - **Cost if wrong:** a composer who edits the remembered line by hand to a bad folder gets the refusal and is asked again.
7. **Untitled songs: move-in on first save (spec §6.7; P9).** After a `save_as` succeeds, and again after any later `save` while the stand-in `<support>/Co-Producer/Untitled/<project id>/` still exists, `notes::move_in(stand_in, package)` runs under `SAVE_LOCK`. It never replaces a file, and it removes a source only after its copy is complete.
   1. **State blobs.** Each `States/<file>` is copied into the package's `States/` when absent. One already there with the same bytes is fine. One with other bytes stops step 2, and all copies stay in the stand-in.
   2. **Copies.** Each `Co-Producer/Copies/<name>` goes to the same name in the package:
      - first by `fs::rename`;
      - across volumes, by a recursive copy into `.<name>.moving` beside the target, then a rename to the name, then `remove_dir_all` of the source;
      - a name already taken in the package leaves that copy where it is. A rename of the copy would break the media paths in its `project.json` (`Co-Producer/Copies/<name>/…`).
   3. **The notes.** `Notes.md` moves when the package has none. Otherwise its text is appended after `"\n## Notes from before the first save\n\n"` by Ruling 4's append, and the source is removed after the append succeeds.
   4. **Tidying.** Empty folders of the stand-in are removed with `remove_dir`, never `remove_dir_all`.

   Each thing left behind adds one problem line: "Co-Producer's <notes|copy "<name>"|copies> from before the first save stayed in <path>: <reason>." The next save tries again.
   - **Cost if wrong:** a copy whose name clashes stays in the support folder, where nothing opens it, until the clash is cleared.
8. **Compact's offer (spec §6.7, "Compact can ask to delete producer edits").**
   - **When it shows.** Only from the Media Pool's Compact buttons (`MediaModel.compactFromMenu()`), never from `media.compact` over a channel, because programmatic calls open nothing. After Compact Media succeeds and `coProducerCopies()` is not empty, the sheet "Remove Old Co-Producer Copies?" opens.
   - **The sheet.**
     - The text: "Compact Media is done. This song also keeps N copies Co-Producer saved before its requests. Choose any you no longer need: they go to the Trash, and the files only they use go when you next save."
     - One row per copy, oldest first: a toggle labelled with its name, then its size ("12.4 MB"). None is ticked.
     - The buttons "Keep All", which is the default and Esc, and "Remove Selected", disabled until a row is ticked.
   - **The size** of a copy is its own folder's files (its `project.json`, plus the media of a copy made untitled). To that it adds the package's media files and state blobs it names that the present song, its autosaves and the other copies do not. It is an estimate of what removing it frees, so the sheet says "about".
   - **Removal.**
     - `remove_co_producer_copies(names)` accepts only names `co_producer_copies()` lists right then. Any other name, `..` or a name with `/` included, is skipped with "Not a copy of this song: <name>.".
     - Each copy goes to the Trash through `retention::trash`, never `remove_dir_all`. A failed move keeps the copy and adds a problem line.
     - The media and blobs that only the removed copies used go at the next Save, by the existing retention rule.
   - **Co-Producer itself** can neither list nor remove copies: `media.compact` stays refused (spec §6.5), and none of the new calls do it.
   - **Cost if wrong:** the freed space shows up only after the next Save.
9. **The protocol and registry additions (control-channel spec §5).**
   - **New protocol commands.** `COMMAND_NAMES` grows from 18 to **21**. Each new command is `not_allowed` on the developer socket with "only on the Co-Producer socket", as 8a's are.
     - `co_producer.notes {window?}` answers `{text, truncated, path}`;
     - `co_producer.note {text, window?}` needs an open request. `text` is 1–2000 characters (`MAX_NOTE`) and at most 20 lines (`MAX_NOTE_LINES`). It has no C0 or C1 control but `\n`, and none of P11's marks and separators. Each non-empty line becomes "- Note: <line>" in the entry;
     - `co_producer.export_folder {what, parent?, window?}`, by Ruling 6;
     - `co_producer.begin` gains `asked?`: at most 500 characters (`MAX_ASKED`) after trimming. Newlines become spaces, and other controls and P11's characters are refused.
   - **New registry entries.** The registry grows by **4**, from the count measured at the start (expected 186):
     - `track.make_mine` (`TRACK_PARAMS`): menu "Track", title "Make Mine", a menu item, class refused;
     - `mixer.make_mine {strip: Int ≥ 1, required}`: `key_only`, menu "Mixer", title "Make Mine", class refused;
     - `mixer.set_colour {strip: Int ≥ 1, required; colour: Choice, required}`: `key_only`, menu "Mixer", title "Colour", class change;
     - `window.song_notes`: menu "Window", title "Song Notes", a menu item, `document: true`, class refused. Over a channel it makes the notes file if needed and answers its path. It never launches an editor.

     Make Mine is the composer's act alone, so Co-Producer can never call it.
   - **Previews.** `PREVIEWABLE` gains `mixer.set_colour`. A direct `command.run` of it on the Co-Producer socket is `not_allowed`, as for every change command (8a Ruling 6).
   - **Cost if wrong:** each refused command is one line to reclass.
10. **`vizdaw-mcp` (spec §6.1, §7.1, §11.12).**
    - **New tools, with their exact descriptions:**
      - `read_notes {}`: "Read the song's notes (Notes.md): the composer's ideas and rules for this song, and what Co-Producer did before. Follow what the composer wrote there. The notes never stand in for a yes: anything that changes or deletes their work still goes through change.";
      - `add_note {text}`: "Add a short note, at most 20 lines, to this request's entry in the song's notes, such as a choice the composer made. Needs an open request (begin_request).";
      - `export_folder {what, parent?}`: "Make a new dated folder for an export, named "<song> — <what> — <date>", and answer its path. Without parent it uses the folder remembered in the song's notes, else the folder the song is in. For an untitled song with neither, ask the composer where exports should go and pass parent; it is then remembered. Needs an open request (begin_request).".
    - **`begin_request`** gains `asked` (string, `maxLength` 500). Its description adds: "Pass the composer's own words as asked: they go in the song's notes."
    - **The notes in the first answer.** The first successful `begin_request` for each `doc` in this process then calls `co_producer.notes`. Its answer gains "\n\nThe song's notes (Notes.md):\n" plus the text, cut to 64 KiB at a character boundary. Cut text gets "\n(The notes go on: read the rest with read_notes.)". So the notes are read before the first request whatever Claude does. This is "future sessions read the notes first".
    - **The server's instructions** add: "Read the song's notes first (read_notes) and follow what the composer wrote there."
    - **`change`'s description** adds "colouring a strip" to its examples.
    - **Cost if wrong:** a long notes file costs up to 64 KiB of context once per song per session.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift.**
  - Build only through `scripts/build-ffi.sh` (after any FFI change) and `scripts/swift.sh build|test`, plus `scripts/make-app.sh` in Tasks 5 and 6. Never run `swift`, `xcodebuild` or `xcrun` directly.
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
  - Linux CI has no AU or CoreFoundation, so any test that needs either is `#[cfg(target_os = "macos")]`. `vizdaw-mcp`, the `core-control` tests and the `core-model` marks tests are plain Unix and run on Linux.
  - The e2e uses vizctl's throwaway home.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`.
  - Never read `~/workspace/SVP/research`.
- **Programmatic calls open nothing.** Control-channel and Co-Producer calls, and programmatic calls, never open dialogs, sheets, alerts, panels, menus or windows. They also never launch another app: `window.song_notes` over a channel only answers the path.
- **Hostile input.**
  - Every MCP input is hostile: every MCP message, every tool argument, every Co-Producer call, and every file Co-Producer reads (`Notes.md`, copies, the remembered export folder).
  - Every new FFI export runs inside `ffi_guard` (and so `catch_unwind`). It returns `Result<_, VizError>`, or a safe default for the never-failing ones.
  - `vizdaw-mcp` handles each message inside `std::panic::catch_unwind`. A panic answers `-32603` and the server goes on.
  - Each new command and call gets hostile-input tests: wrong types, unknown params, missing required params, a 70 000-character string, a NUL, IDs out of range.
  - Every item a call skips or leaves behind gets a report or problem line (Rulings 7 and 8).
- **Counts.** Check every count at the start of Task 1, and report each task's change against it.
  - The registry grows by **4**, from the count measured at the start: 186 plus whatever 8a Tasks 6–8 added, which is expected to be 186.
  - `COMMAND_NAMES` goes from 18 to **21**. `ErrorCode::ALL` stays 16.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list. The list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`); read them, keep all new text clear of them, and never copy the list.
  - The list includes the feature's old working name, which plan 8a added to the scans (spec, "Name"). Like every banned name, it is never written in the repo, this plan included.
  - Notes entries, export folder names, undo names and tool descriptions are covered by the same rule.
  - The feature is "Co-Producer" everywhere.
  - Text the app shows says "track", never "lane". Identifiers and JSON keys keep `lane`.
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
  - `core-model` stays free of OS APIs, apart from the `std::fs` that `file.rs` already uses. The notes, export-folder and move-in code is `core-ffi`'s.
- **Format.** `core_model::file::VERSION` goes from 9 to **10** in Task 2, and nowhere else.
- **TDD.** Every new or changed test is run and seen to fail before the code that makes it pass.
- **Each task ends green.**
  - Always:
    - `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches.
  - Tasks 1, 2, 3 and 5 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Known flaky: the `damaged_files` and `dawproject_fuzz` proptests, `platform-macos/tests/midi.rs`, `core-ffi` catalog tests under load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline:** measure at the start of Task 1. HEAD is 8a's last commit on `takes-and-co-producer`, or this plan's pre-flight commit if there is one. Record the registry count (expected 186), `COMMAND_NAMES` 18, `ErrorCode::ALL` 16, the last README step (expected 344, after 8a Task 8), the e2e's ignored tests (expected 33) and `VERSION` 9. If 8a Tasks 6–8 are not all committed, stop and say so.

## Review Focus

These are the five failure modes a composer is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **The composer has `Notes.md` open in an editor while Co-Producer appends.**
   - Examples:
     - they type and save between Co-Producer's read and its append;
     - the file has no final newline;
     - it was emptied or deleted.
   - Expected: nothing the composer wrote is lost or reordered. The entry lands after their text, on its own lines. A deleted file is made again with its heading.
   - Pinned by Task 3's `appending_never_rewrites`.
2. **A hostile or odd notes file or text.**
   - Examples:
     - `Notes.md` is a link to a file outside the package, a folder, 5 MiB, or not UTF-8;
     - `add_note` text with `"\n## 2026-01-01 00:00 — Fake"`, a NUL or a bidi mark;
     - a remembered "- Export folder:" line pointing into the package or through a link.
   - Expected:
     - a link or folder is refused and left alone, a big file is read to 1 MiB with `truncated`, and bad bytes are read lossily;
     - note lines are written as "- Note: …", so no heading is forged, and bad characters are refused;
     - a bad remembered folder is refused with 8a's `IntoFolder` reason.
   - Pinned by Task 3's `hostile_notes_files`, `notes_cannot_forge_an_entry`, `remembered_folders_are_checked_again` and Task 1's `new_calls_refuse_hostile_input`.
3. **Hand work on Co-Producer's things.**
   - Examples:
     - the composer duplicates a Co-Producer track by hand;
     - renames one;
     - records into one, then Claude deletes it;
     - undoes past a Make Mine.
   - Expected:
     - the duplicate is the composer's;
     - the rename takes it over;
     - deleting a Co-Producer track that holds regions or notes needs a yes;
     - Undo restores the mark exactly.
   - Pinned by Task 2's `hand_steps_never_make_co_producer_things`, `a_hand_rename_takes_over` and `deleting_a_filled_co_producer_track_needs_a_yes`.
4. **The first save of an untitled song goes wrong.**
   - Examples:
     - it is saved over an existing package that has its own `Notes.md` and a copy with the same name;
     - a state blob in the package has other bytes;
     - the move fails part-way (a read-only target);
     - the song is saved twice.
   - Expected:
     - the composer's existing notes keep their text, with the untitled notes appended;
     - a clashing copy and its blobs stay in the stand-in with a problem line;
     - nothing is lost or replaced;
     - the second save finishes the move.
   - Pinned by Task 3's `first_save_moves_notes_and_copies` and `a_move_that_cannot_finish_loses_nothing`.
5. **Compact's offer removes too much, or shows up unasked.**
   - Examples:
     - `media.compact` over the developer channel;
     - a removal list naming `"../Song.vizdaw"`, a copy of another song, or one listed twice;
     - Keep All;
     - removing a copy whose media the present song still uses.
   - Expected:
     - no sheet over a channel;
     - only listed copies go, to the Trash;
     - Keep All removes nothing;
     - media still in use stay after the next Save.
   - Pinned by Task 3's `copies_are_removed_only_by_name_and_to_the_trash` and Task 5's `copiesOfferOnlyFromTheMenu` and `keepAllRemovesNothing`.

---

### Task 1: The registry and protocol additions (`core-control`)

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (the four entries and their params);
  - `crates/core-control/src/request.rs` (`COMMAND_NAMES`, the three commands, `asked`, `MAX_ASKED`, `MAX_NOTE`, `MAX_NOTE_LINES`, `ExportWhat`);
  - `crates/core-control/src/co_producer.rs` (`PREVIEWABLE`, `admit` for the new commands);
  - `crates/core-control/tests/co_producer.rs`, `tests/commands.rs`, `tests/protocol.rs`;
  - `crates/core-ffi/src/control.rs` (the `ControlRequest` variants and their mapping, so the workspace builds; the Session work is Task 3's);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (the four ids in `handled`, answering `.failed(.of(.unavailable, "Not ready yet."))` until Task 5);
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (the three new request cases answer `.failed(.of(.unavailable, "Not ready yet."))` until Task 5; `asked` is ignored until then).

**Interfaces:**
- Consumes: 8a's `Command::CoProducer*`, `co_producer::{admit, check_output, Output, PREVIEWABLE}`, `registry::validate_args`.
- Produces:
  ```rust
  // request.rs
  pub const MAX_ASKED: usize = 500;
  pub const MAX_NOTE: usize = 2000;
  pub const MAX_NOTE_LINES: usize = 20;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum ExportWhat { Stems, Mix, Selection, Midi, Dawproject, Aaf, OtherDaw }
  impl ExportWhat {
      pub fn parse(s: &str) -> Option<ExportWhat>;   // "stems" … "other_daw"
      pub fn label(self) -> &'static str;            // "Stems" … "Other DAW" (Ruling 6)
  }
  // Command gains:
  CoProducerNotes { window: Option<WindowRef> },
  CoProducerNote { text: String, window: Option<WindowRef> },
  CoProducerExportFolder { what: ExportWhat, parent: Option<PathBuf>, window: Option<WindowRef> },
  // and CoProducerBegin gains `asked: Option<String>` (trimmed, newlines as spaces).
  // co_producer.rs
  pub const PREVIEWABLE: &[&str] = &["track.rename", "track.delete", "edit.undo", "edit.redo", "file.save", "mixer.set_colour"];
  ```
  In `core-ffi/src/control.rs`, `ControlRequest` gains `CoProducerNotes { window }`, `CoProducerNote { text, window }` and `CoProducerExportFolder { what: ExportWhatRow, parent: Option<String>, window }`, and `CoProducerBegin` gains `asked: Option<String>`. `ExportWhatRow` is a `uniffi::Enum` mirroring `ExportWhat`.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it at the top of the task report.
- [ ] **Step 2: Write the failing tests.**
  - In `tests/co_producer.rs`:
    - `new_registry_entries_have_their_class`: `track.make_mine`, `mixer.make_mine` and `window.song_notes` are `Refused`, and `mixer.set_colour` is `Change`. `list_json()` holds none of the refused three. `mixer.set_colour` is in it with `previewable: true`.
    - `set_colour_only_through_a_preview`:
      - `command.run mixer.set_colour {strip: 2, colour: "blue"}` on the Co-Producer socket is `not_allowed` with 8a Ruling 6's direct-call sentence;
      - a `co_producer.preview` plan holding it is admitted;
      - `colour: "mauve"` is `invalid_params`.
    - `export_folder_parent_is_checked`: `parent` relative, with `..`, a file, a link to a folder, inside a temp `X.vizdaw`, and inside `x.VIZDAW` through a linked parent are each `not_allowed` with the reason. An existing plain temp folder, and no `parent` at all, are admitted.
    - `new_calls_refuse_hostile_input`. Each of these is `invalid_params`:
      - for `co_producer.note`: `text` missing, `""`, 2001 characters, 21 lines, a NUL, `"a\u{202E}b"`, `"a\u{2028}b"`, a number, or an unknown param;
      - for `co_producer.begin`: `asked` of 501 characters, or holding a NUL;
      - for `co_producer.export_folder`: `what: "video"`, `what` missing, or a `parent` of 70 000 characters.

      `asked: "two\nlines"` parses to `"two lines"`.
    - `new_calls_only_on_the_co_producer_socket`: each of the three is `not_allowed` "only on the Co-Producer socket" on a developer server.
  - In `tests/commands.rs`: the registry count is the baseline plus 4. `COMMAND_NAMES.len() == 21`, and the minimal-shape list gains the three.
  - In `tests/protocol.rs`: the round-trip parse of each new command, and of `co_producer.begin {name, asked}`.
- [ ] **Step 3: Run** `cargo test -p core-control`. Expected: FAIL.
- [ ] **Step 4: Implement** Ruling 9.
  - **The checks.** `validate` parses the shapes. `admit` applies the `parent` rule through `check_output(parent, Output::IntoFolder)`. The text rules reuse 8a's P11 character set.
  - **The two other crates.** `core-ffi/src/control.rs` maps the new commands to the new `ControlRequest` variants. Swift gets the temporary arms named in Files, so the Swift build and `everyRegisteredMenuCommandHasAHandler` pass.
- [ ] **Step 5: Run** `cargo test -p core-control -p core-ffi`. Expected: PASS.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(control): Co-Producer notes, export folder and Make Mine commands, and strip colours behind a preview (plan 8b Task 1)`.

### Task 2: The marks, Make Mine and format 10 (`core-model`, `core-ffi`)

**Files:**
- Create: `crates/core-model/src/marks.rs`, `crates/core-model/tests/marks.rs`.
- Modify:
  - `crates/core-model/src/lane.rs`, `mixer.rs` (the fields);
  - `crates/core-model/src/history.rs` (`settle` in `execute` and `amend`);
  - `crates/core-model/src/command/lane.rs`, `command/mixer.rs` (`MakeMine`);
  - `crates/core-model/src/diff.rs` (`co_producers`, the colour item);
  - `crates/core-model/src/file.rs` (`VERSION` 10, `migrate_v9_to_v10`);
  - `crates/core-model/src/lib.rs`, `crates/core-model/tests/file.rs`, `tests/diff.rs`;
  - struct literals of the four types in `core-io` and the tests, which gain `co_producer: false` where the compiler asks;
  - `crates/core-ffi/src/snapshot.rs`, `mixer.rs` (`LaneRow.co_producer`, `StripRow.co_producer`);
  - `crates/core-ffi/src/control.rs` (`co_producer` in `state.get document` lanes and `state.get mixer` strips);
  - `crates/core-ffi/src/co_producer.rs` (`mixer.set_colour`'s preview, `composers_work`);
  - `crates/core-ffi/src/session.rs` (`make_mine_track`, `make_mine_strip`);
  - `crates/core-ffi/tests/co_producer.rs`;
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (stubs for the two new methods; the row fields where fixtures build rows).

**Interfaces:**
- Produces (`core-model`):
  ```rust
  // marks.rs
  pub const SUFFIX: &str = " (Co-Producer)";
  /// The strip `lane` plays through, as the app's StripFocus finds it: its audio path's
  /// strip; else, for a plug-in destination, its Instrument strip; none for an external
  /// destination; else the built-in synth's strip.
  pub fn lane_strip(doc: &Document, lane: LaneId) -> Option<StripId>;
  /// Ruling 1's settling of `after` against `before`.
  pub fn settle(before: &Document, after: &mut Document, acting: bool);
  // LaneCmd::MakeMine { seq: SeqId, lane: LaneId }, MixerCmd::MakeMine { strip: StripId }: "Make Mine".
  // diff.rs: ChangeItem gains `pub co_producers: bool` (Ruling 2).
  ```
- Produces (`core-ffi`):
  ```rust
  impl Session {
      pub fn make_mine_track(&self, seq: u64, lane: u64) -> Result<DocSnapshot, VizError>;
      pub fn make_mine_strip(&self, strip: u64) -> Result<DocSnapshot, VizError>;
  }
  // LaneRow and StripRow gain `pub co_producer: bool`.
  ```

- [ ] **Step 1: Write the failing `core-model` tests** in `tests/marks.rs`, on a fixture document with one hand track.
  - `acting_steps_mark_what_they_make`:
    - with `set_acting(true)` inside a request, a new audio track gives a lane and a strip that are both marked and named "Audio 2 (Co-Producer)";
    - a new bus with a send from the hand track's strip gives a marked bus and a marked send, and the hand strip itself stays unmarked;
    - the new strip's EQ slot from `fill_new_eqs` is marked.
  - `acting_renames_keep_the_suffix`: an acting rename of the Co-Producer track to "Kick" gives "Kick (Co-Producer)", still marked. An acting rename to "Kick (Co-Producer)" is unchanged.
  - `hand_steps_never_make_co_producer_things`: a hand `track.duplicate` of the marked track gives an unmarked lane and strip. A hand new bus is unmarked.
  - `a_hand_rename_takes_over`: a hand rename of the marked lane to "Bass" unmarks it, and its strip keeps its mark. A hand strip rename unmarks the strip.
  - `make_mine_takes_over_a_track_and_its_strip`:
    - `LaneCmd::MakeMine` on the marked track unmarks the lane, its strip, the strip's sends and slots, and drops " (Co-Producer)" from both names;
    - its label is "Make Mine", and one undo restores every mark and name exactly;
    - Make Mine of an unmarked track adds no undo step;
    - `MixerCmd::MakeMine` on a marked bus unmarks only that bus, its sends and its slots.
  - `lane_strip_matches_the_app`: an audio lane gives its path's strip, a plug-in instrument lane its Instrument strip, an external MIDI lane `None`, and a built-in synth lane the built-in synth's strip.
  - In `tests/file.rs`:
    - `marks_round_trip_and_version_10`: a saved marked document loads equal, with `"version": 10`;
    - a version-9 fixture loads with no marks;
    - an unmarked document's JSON has no `co_producer` key.
  - In `tests/diff.rs`:
    - `items_know_co_producers_things`: renaming a marked track is `co_producers: true`, and renaming a hand track `false`. Deleting an empty marked track is `true`. Deleting a marked track with 4 notes, or a marked track's notes falling from 12 to 10, is `false`;
    - `strip_colour_is_an_item`: "Strip Drums" / "colour none → blue", and "colour blue → none".
- [ ] **Step 2: Run** `cargo test -p core-model --test marks --test file --test diff`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 1, the diff additions of Rulings 2 and 3, and the migration. `settle` runs after `fill_new_eqs` in `execute`, and in `amend`. It is a pure function of the two documents, so undo and redo, which restore whole versions, need nothing.
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS, including 8a's `co_producer_requests` proptest.
- [ ] **Step 5: Write the failing `core-ffi` tests** in `tests/co_producer.rs`.
  - `previews_spare_co_producers_own_things`, inside a request:
    - add a track while acting, then preview `track.rename` of it: `needs_yes: false`, and the item reads "renamed to \"Kick (Co-Producer)\"";
    - preview `mixer.set_colour {strip: <its strip>, colour: "blue"}`: `needs_yes: false`;
    - the same two on a hand track: `needs_yes: true`.
  - `deleting_a_filled_co_producer_track_needs_a_yes`: give the Co-Producer track a note by hand, then preview its `track.delete`: `needs_yes: true`, and the item's change starts "deleted, with".
  - `marks_reach_the_rows_and_state`: `LaneRow.co_producer` and `StripRow.co_producer` are true for the Co-Producer track. The `state.get document` JSON lanes and the `state.get mixer` strips carry `"co_producer": true`.
  - `make_mine_through_the_session`: `make_mine_track` unmarks the track and its strip, and `undo_label` is "Make Mine". `make_mine_strip` with a missing strip id, or `u64::MAX`, is `Err` and changes nothing.
- [ ] **Step 6: Run** `cargo test -p core-ffi --test co_producer`. Expected: FAIL.
- [ ] **Step 7: Implement.**
  - **Previews.** `preview_items` builds `mixer.set_colour` as the menu's `MixerCmd::SetColour`, with `none` as `None`, on the clone. It sets `composers_work` from `co_producers` for the three commands (Ruling 2).
  - **Rows.** The rows and state JSON gain the field.
  - **Make Mine.** The two Session methods run through `edit`, as `set_strip_colour` does.
  - **Swift.** `FakeSession` gains the two stubs.
- [ ] **Step 8: Run** `cargo test -p core-model -p core-ffi -p core-io`. Expected: PASS.
- [ ] **Step 9: End-of-task checks**, including the Swift build and test. **Commit** `feat(model): Co-Producer marks with Make Mine and take-over by rename, project format 10 (plan 8b Task 2)`.

### Task 3: The notes file, the request entry, export folders, move-in and the copies offer (`core-ffi`)

**Files:**
- Create: `crates/core-ffi/src/notes.rs`, `crates/core-ffi/tests/notes.rs`.
- Modify:
  - `crates/core-ffi/src/session.rs` (the log on `Inner`'s request; the new methods; the entry at end and idle; `move_in` after `save_as` and `save`);
  - `crates/core-ffi/src/control.rs` (the result variants);
  - `crates/core-ffi/src/lib.rs`;
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (stubs).

**Interfaces:**
- Consumes:
  - Task 1's `ExportWhatRow` and `ControlRequest` variants;
  - Task 2's `ChangeItem.co_producers`;
  - 8a's `copies_home`, `co_producer_copy`, `list_copies`, `check_output`, `diff::shown_name`, `retention::{trash, trash_dir}` and `SAVE_LOCK`.
- Produces:
  ```rust
  // notes.rs
  pub const NOTES_FILE: &str = "Notes.md";
  pub const MAX_READ: usize = 1 << 20;
  pub struct Notes { pub text: String, pub truncated: bool, pub path: PathBuf }
  /// Ruling 4: makes "# <song>\n" with create_new when absent; refuses a non-regular file.
  pub fn ensure(home: &Path, song: &str) -> Result<PathBuf, VizError>;
  pub fn read(home: &Path, song: &str) -> Result<Notes, VizError>;
  /// Ruling 4's append of `text` (a whole entry or block) after a blank line.
  pub fn append(home: &Path, song: &str, text: &str) -> Result<(), VizError>;
  #[derive(Debug, Clone, Default)]
  pub struct RequestLog {
      pub stamp: Option<String>, pub asked: Option<String>,
      pub items: Vec<(ChangeItem, bool)>,  // bool: after a yes (Ruling 5)
      pub labels: Vec<String>, pub files: Vec<String>, pub notes: Vec<String>,
      pub export_folder: Option<String>, pub copy: Option<String>,
  }
  /// Ruling 5's entry, exactly; `None` when the request never entered.
  pub fn entry(log: &RequestLog, title: &str) -> Option<String>;
  /// The last "- Export folder: <absolute path>" line.
  pub fn remembered_folder(text: &str) -> Option<PathBuf>;
  /// Ruling 6: a new folder "<song> — <what> — <date>" (numbered) inside `parent`.
  pub fn make_export_folder(parent: &Path, song: &str, what: ExportWhat, date: &str) -> Result<PathBuf, VizError>;
  /// Ruling 7; the problem lines for what stayed behind.
  pub fn move_in(stand_in: &Path, package: &Path) -> Vec<String>;
  // session.rs
  #[derive(uniffi::Record)] pub struct NotesRow { pub text: String, pub truncated: bool, pub path: String }
  #[derive(uniffi::Record)] pub struct CopyRow { pub name: String, pub path: String, pub bytes: u64 }
  #[derive(uniffi::Record)] pub struct RemovedCopiesRow { pub removed: u64, pub problems: Vec<String> }
  impl Session {
      pub fn notes_path(&self) -> Result<String, VizError>;          // Window › Song Notes
      pub fn co_producer_notes(&self) -> Result<NotesRow, VizError>;
      pub fn co_producer_set_asked(&self, asked: String) -> Result<(), VizError>;  // needs a request
      pub fn co_producer_note(&self, text: String) -> Result<(), VizError>;        // needs a request
      pub fn co_producer_file(&self, path: String) -> Result<(), VizError>;        // needs a request
      pub fn co_producer_export_folder(&self, what: ExportWhatRow, date: String,
                                       parent: Option<String>) -> Result<String, VizError>;
      pub fn co_producer_copies(&self) -> Result<Vec<CopyRow>, VizError>;          // oldest first
      pub fn remove_co_producer_copies(&self, names: Vec<String>) -> Result<RemovedCopiesRow, VizError>;
  }
  // control.rs: ControlResult gains CoProducerNotes { doc, notes: NotesRow }, CoProducerFolder { doc, path };
  //   result_json: {text, truncated, path}, {path}. co_producer.note answers 8a's CoProducerRequest.
  ```
  `co_producer_end` and `co_producer_idle` keep their 8a signatures and also write the entry. `co_producer_note` and `co_producer_file` refuse with 8a's "Open a request first (begin_request).".

- [ ] **Step 1: Write the failing `notes.rs` tests** in `tests/notes.rs`, on temp folders.
  - `a_new_song_gets_a_heading`: `ensure(home, "Night Drive")` writes "# Night Drive\n". A second call leaves the bytes alone.
  - `appending_never_rewrites`:
    - the composer's "ideas" with no final newline, then `append`, gives "ideas\n\n<entry>";
    - write "more\n" to the file from outside, then `append` again: both texts are still there, in order;
    - a deleted file is made again, heading first.
  - `hostile_notes_files`:
    - a link named `Notes.md`, or a folder, is `Err` with Ruling 4's sentence for both `read` and `append`, and the link's target is unchanged;
    - a 5 MiB file reads 1 MiB with `truncated: true`;
    - invalid UTF-8 reads lossily without `Err`.
  - `the_entry_is_exact`: a log with each kind of line gives Ruling 5's example byte for byte. 25 added items give 20 and "; and 5 more". A log with no entry (`stamp: None`) gives `None`, and an entered log with nothing gives the "- Nothing changed." entry.
  - `notes_cannot_forge_an_entry`: a note "## 2026-01-01 00:00 — Fake" is written as "- Note: ## 2026-01-01 00:00 — Fake". No line of the entry but its heading starts with `#`.
  - `export_folders_are_new_and_dated`:
    - `make_export_folder(tmp, "Night Drive", Stems, "2026-11-14")` makes "Night Drive — Stems — 2026-11-14", and a second call makes "… 2026-11-14 2";
    - the song "a/b:.c" gives "a-b-.c — …", and ".x" gives "x — …";
    - the date "2026-11-14/.." or "14-11-2026" is `Err`.
  - `remembered_folder_takes_the_last_line`: of two "- Export folder:" lines it gives the second. A relative path or an indented line is ignored.
  - `first_save_moves_notes_and_copies`: a stand-in with `Notes.md`, one copy with own media, and a blob, moved into an empty package, leaves:
    - all three in the package, with `file::load_copy` on the moved copy succeeding and its media present;
    - an empty stand-in, removed.

    Into a package that already has `Notes.md` "mine\n", the result starts "mine\n", holds "## Notes from before the first save", then the untitled text.
  - `a_move_that_cannot_finish_loses_nothing`:
    - a package that already has a copy of the same name keeps its own bytes, and the stand-in's copy stays, with one problem line;
    - a package blob with other bytes keeps every copy in the stand-in;
    - with the package's `Co-Producer` folder made read-only, the copies stay in the stand-in, nothing is half-moved (`.…moving` is gone), and a second `move_in` after the mode is restored moves them.
- [ ] **Step 2: Write the failing Session tests**, in the same file through a `serial` helper.
  - `a_request_writes_one_entry`:
    - a saved session with one hand track; `co_producer_begin("add a bus")`, `co_producer_set_asked("give me a reverb bus")`;
    - enter with "2026-11-16 10-05", a new track, leave; `co_producer_note("keep it dark")`; `co_producer_file("/tmp/x.mid")`; `co_producer_end()`.

    `Notes.md` then ends with an entry headed "## 2026-11-16 10:05 — Add a Bus" holding these lines:
    - `- Asked: "give me a reverb bus"`;
    - one `- Added: ` line with two items, the new track's ("Track 2, … (Co-Producer): added") and its strip's;
    - `- Files: /tmp/x.mid`;
    - `- Note: keep it dark`;
    - `- Undo step: "Co-Producer: Add a Bus"`;
    - `- Saved copy: Co-Producer/Copies/2026-11-16 10-05 before Add a Bus.vizdaw`.
  - `hand_edits_are_not_logged`: a hand rename between two Co-Producer calls of one request is in no line of the entry.
  - `idle_and_read_only_requests`: an idle-closed request writes its entry, while a request that never entered writes none.
  - `untitled_notes_live_in_the_stand_in_and_move_on_save`:
    - an untitled session's request writes `<support>/Co-Producer/Untitled/<id>/Notes.md` and its copy;
    - `save_as(<tmp>/Song.vizdaw)` moves both, and the stand-in folder is gone;
    - `co_producer_copies()` then lists the copy, and `notes_path()` is the package's.
  - `export_folder_uses_parent_then_memory_then_the_song`, with the date "2026-11-16":
    - with `parent` it makes the folder there, and the entry gets "- Export folder: <parent>";
    - in the next request, without `parent`, it uses that remembered folder;
    - for another saved song with no memory, it uses the folder the package is in;
    - for an untitled song with no memory it is `Err` with Ruling 6's sentence;
    - with no open request it is `Err` "Open a request first (begin_request).".
  - `remembered_folders_are_checked_again`: a hand-written "- Export folder: <the package itself>", or a link to a folder, is refused with the `IntoFolder` reason, and no folder is made.
  - `copies_are_listed_with_sizes`:
    - two copies, oldest first;
    - a copy that alone names a 1 000-byte media file has `bytes` of at least its `project.json` plus 1 000;
    - a copy whose media the present song also uses does not count those bytes.
  - `copies_are_removed_only_by_name_and_to_the_trash`, with `debug_set_trash_dir(Some(<tmp>))`:
    - removing `[<first copy's name>]` moves it into the trash folder, and `removed == 1`;
    - names `"../Song.vizdaw"`, `"x/y"`, a missing name, and the same name twice each give one "Not a copy of this song: …" line, and nothing else moves;
    - after a Save, the media only the removed copy used are in the trash folder, while media the present song uses stay.
  - `new_session_calls_refuse_hostile_input`: `co_producer_file` with "", a relative path, 70 000 characters or a NUL; `co_producer_note` with 2001 characters; `co_producer_export_folder` with a date "x". Each is `Err`, with no panic and no file written.
- [ ] **Step 3: Run** `cargo test -p core-ffi --test notes`. Expected: FAIL.
- [ ] **Step 4: Implement** Rulings 4–8.
  - **The log.** It lives with the request in `Inner`. `co_producer_enter` sets `stamp` and the copy on the first entry, and keeps the entered document. `co_producer_leave` appends the items. `co_producer_take` notes `needs_yes` for the next leave.
  - **Ending.** End and idle build the entry and append it outside the document lock's critical work, since it is file I/O, as autosave does. A failed write adds Ruling 5's problem line.
  - **`save_as` and `save`** call `move_in` after the save succeeded and push its lines to `news.problems`.
  - **`notes_path`** stamps an untitled song's project ID, as `copies_home(true)` does.
  - **Swift.** `FakeSession` gains stubs.
- [ ] **Step 5: Run** `cargo test -p core-ffi`. Expected: PASS, including 8a's `co_producer` and `co_producer_copies`.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(ffi): song notes with one entry per Co-Producer request, dated export folders, untitled notes and copies moved in on first save, and copy removal by name (plan 8b Task 3)`.

### Task 4: `vizdaw-mcp`: notes and export folder tools

**Files:**
- Modify:
  - `crates/vizdaw-mcp/src/lib.rs`, `src/tools.rs`;
  - `crates/vizdaw-mcp/tests/server.rs`, `tests/support/fake_app.rs`.

**Interfaces:**
- Consumes: Task 1's `co_producer.notes`, `co_producer.note`, `co_producer.export_folder` and `asked`. Task 3's answers: `{text, truncated, path}`, `{path}`, and 8a's request result.
- Produces: Ruling 10's tools, descriptions and first-answer notes.

- [ ] **Step 1: Write the failing tests** in `tests/server.rs`. The fake app answers `co_producer.notes` with a scripted text and `doc: "d1"`, and `co_producer.export_folder` with `{path: "/x/Song — Stems — 2026-11-16"}`.
  - `notes_tools_are_listed`:
    - `tools/list` has `read_notes`, `add_note` and `export_folder` with Ruling 10's descriptions;
    - `begin_request`'s schema has `asked` with `maxLength` 500, and `add_note`'s `text` has `maxLength` 2000;
    - `export_folder`'s `what` is an `enum` of the seven names, and its `parent` has `maxLength` 1024;
    - there is no `track_make_mine`, `mixer_make_mine`, `mixer_set_colour` or `window_song_notes`.
  - `the_first_request_reads_the_notes`:
    - `begin_request {name, asked}` sends `co_producer.begin` with both, then `co_producer.notes`, and its text ends with "The song's notes (Notes.md):\n" and the notes;
    - a second `begin_request` for the same `doc` sends no `co_producer.notes`;
    - with 100 KiB of notes, the text holds 64 KiB of them, cut on a character boundary (a 4-byte character straddling the cut is dropped whole), then "(The notes go on: read the rest with read_notes.)".
  - `notes_tools_reach_the_app`: `read_notes` sends `co_producer.notes` and answers the text, `add_note {text}` sends `co_producer.note`, and `export_folder {what: "stems"}` sends `co_producer.export_folder` and answers the path.
  - `notes_tools_refuse_hostile_arguments`. Each is `isError: true` before any socket call:
    - `add_note` with `text` missing, a number, or 70 000 characters;
    - `export_folder` with `what: "video"`, or an unknown property;
    - `begin_request` with `asked` as an array.
  - `instructions_say_read_the_notes`: the `initialize` answer's instructions contain Ruling 10's sentence.
- [ ] **Step 2: Run** `cargo test -p vizdaw-mcp`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 10. Notes already shown are remembered per `doc` string, for at most 64 documents, oldest dropped. A failed `co_producer.notes` after a good begin leaves the begin's answer as it is, with no notes.
- [ ] **Step 4: Run** `cargo test -p vizdaw-mcp -p core-control`. Expected: PASS, including `names.rs`'s scan of `vizdaw-mcp/src`.
- [ ] **Step 5: End-of-task checks. Commit** `feat(mcp): read_notes, add_note and export_folder, and the song's notes with the first request (plan 8b Task 4)`.

### Task 5: The librarian in the app (Swift)

**Files:**
- Create:
  - `app/Sources/VizDAWAppModel/SongNotes.swift`;
  - `app/Sources/VizDAWAppModel/CopiesOffer.swift`;
  - `app/Sources/VizDAW/CopiesOfferSheet.swift`;
  - `app/Tests/VizDAWAppModelTests/SongNotesTests.swift`, `CopiesOfferTests.swift`.
- Modify:
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (the three calls, `asked`, recording outward files, the export date; replacing Task 1's arms);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (the four registry handlers, replacing Task 1's arms);
  - `app/Sources/VizDAWAppModel/MediaModel.swift` (`compactFromMenu()`, `copiesOffer`);
  - `app/Sources/VizDAWAppModel/StripPalette.swift` (the mark's colour);
  - `app/Sources/VizDAWAppModel/TrackCommands.swift` (Make Mine);
  - `app/Sources/VizDAWAppModel/MixerModel.swift` (`makeMine(_:)`);
  - `app/Sources/VizDAWAppModel/AppIcon.swift` (`coProducerBadge`);
  - `app/Sources/VizDAWEditors/LaneHeaderView.swift` (the badge; Make Mine in the context menu);
  - `app/Sources/VizDAW/ChannelStrip.swift` (the badge; Make Mine in the strip menu);
  - `app/Sources/VizDAW/AppCommands.swift` (Window › Song Notes, Track › Make Mine);
  - `app/Sources/VizDAW/MediaPoolWindow.swift` (both Compact buttons call `compactFromMenu()`; the sheet);
  - `app/Tests/VizDAWAppModelTests/CoProducerCommandsTests.swift`, `ControlCommandsTests.swift`, `FakeSession.swift`, and the existing `StripPalette` and `AppIcon` tests.

**Interfaces:**
- Consumes:
  - Task 2's `makeMineTrack(seq:lane:)`, `makeMineStrip(strip:)`, and `LaneRow.coProducer`/`StripRow.coProducer`;
  - Task 3's `notesPath()`, `coProducerNotes()`, `coProducerSetAsked(asked:)`, `coProducerNote(text:)`, `coProducerFile(path:)`, `coProducerExportFolder(what:date:parent:)`, `coProducerCopies()` and `removeCoProducerCopies(names:)`;
  - 8a's `CoProducerCommands.run` and `stamp`.
- Produces:
  ```swift
  @MainActor public enum SongNotes {
      public static let command = "window.song_notes"
      /// Makes the notes file if needed and opens it with `open`; the alert text when it cannot.
      public static func open(_ m: DocumentModel, with open: (URL) -> Bool) -> String?
  }
  @MainActor @Observable public final class CopiesOffer {
      public struct Row: Identifiable, Equatable { public let id: String; public let size: String; public var chosen: Bool }
      public private(set) var rows: [Row]
      public init?(copies: [CopyRow])               // nil when empty; none chosen
      public func toggle(_ id: String)
      public var canRemove: Bool                    // any chosen
      public var message: String                    // Ruling 8's text with N
      public func remove(_ session: SessionProtocol) -> [String]   // the problems
  }
  // MediaModel: public func compactFromMenu() -> Bool; public var copiesOffer: CopiesOffer?
  // TrackCommands: public static let makeMine = "track.make_mine"
  // MixerModel: public func makeMine(_ strip: StripRow)
  // StripPalette.bar: own colour, else accent.accent when coProducer, else the kind's (Ruling 3)
  ```

- [ ] **Step 1: Write the failing Swift tests**, with `FakeSession`, `MemoryDefaults()` and a fake opener.
  - `SongNotesTests`:
    - `opensTheNotesFile`: `open` calls the fake opener with the URL from `notesPath()` and returns nil;
    - an opener that returns false gives "No app opened Notes.md. It is at <path>.";
    - a throwing `notesPath()` gives "Song notes could not be opened: <message>.".
    - `overTheChannelItOpensNothing`: `ControlCommands.run window.song_notes` answers done with the path, and the opener is never called.
  - `CopiesOfferTests`:
    - `noCopiesNoOffer`: `CopiesOffer(copies: [])` is nil;
    - `noneChosenAtFirst`: two rows, none chosen, `canRemove` false, and the sizes read "about 1.2 MB";
    - `removeSendsOnlyTheChosen`: choosing the second row makes `remove` call `removeCoProducerCopies(names: [second])`;
    - `keepAllRemovesNothing`: dismissing the offer (`copiesOffer = nil`) calls no removal;
    - `copiesOfferOnlyFromTheMenu`: `compactFromMenu()` with two copies sets `copiesOffer`, while `ControlCommands.run media.compact` and `MediaModel.compact()` leave it nil.
  - `CoProducerCommandsTests`:
    - `beginPassesAsked`: `co_producer.begin {name, asked}` calls `coProducerBegin` then `coProducerSetAsked`;
    - `notesNoteAndFolderPassThrough`: each of the three maps to its Session call and result. `export_folder` gets `stamp(now)`'s first 10 characters as its date;
    - `outwardFilesAreRecorded`: an outward command with `path` that succeeds calls `coProducerFile(path:)`. A failed one, or one answered later by a failing job, does not. A job that succeeds records it when it ends.
  - `ControlCommandsTests`:
    - `makeMineAndColourHandlers`: `track.make_mine {seq, lane}` calls `makeMineTrack`, `mixer.make_mine {strip}` calls `makeMineStrip`, and `mixer.set_colour {strip, colour: "none"}` calls `setStripColour` with nil;
    - a missing strip is `.failed` with the session's message.
  - The existing palette test gains `coProducerStripsDrawTheAccent`: with no own colour, a marked strip draws `accent.accent`; with own colour blue, it draws the blue swatch.
  - The existing `AppIcon` test gains `coProducerBadgeIsSymbiotic`: its role is `.controlLink`, its source `.glyph`.
  - **`NameScanTests` and `noLaneInWhatTheAppShows`** pass with the new copy.
- [ ] **Step 2: Run** `scripts/build-ffi.sh && scripts/swift.sh test --filter 'SongNotes|CopiesOffer|CoProducerCommands|ControlCommands'`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **The menus.**
    - **Window › Song Notes** opens the file with `NSWorkspace.shared.open`. It is enabled when a document is in front, and a returned alert text shows as the window's alert.
    - **Track › Make Mine** applies to the selection's first track. It is enabled when that track is marked. The track header's context menu shows "Make Mine" only for a marked track.
    - **The strip's menu** shows "Make Mine" only for a marked strip.
  - **The badge** (Ruling 3) goes at the end of the track header's name row and beside the strip's name, with the accessibility label "Added by Co-Producer". Track headers are AppKit-drawn, so the header draws the glyph image as it draws its other icons.
  - **The sheet.** `MediaPoolWindow` presents `CopiesOfferSheet` while `media.copiesOffer` is set. It has the title "Remove Old Co-Producer Copies?", the message, the toggles (each with the accessibility label "<name>, about <size>"), "Keep All" (`.cancelAction`, default) and "Remove Selected" (disabled unless `canRemove`). Problems from `remove` show as the window's report lines.
  - **`CoProducerCommands`** takes the three calls and `asked`, records outward paths after success, including from the deferred job answer, and passes the export date.
- [ ] **Step 4: Run** the Step 2 command, then `scripts/swift.sh test`. Expected: PASS.
- [ ] **Step 5: End-of-task checks**, including `scripts/make-app.sh`. **Commit** `feat(app): Song Notes, the Co-Producer badge and colour, Make Mine, and Compact's offer to remove old copies (plan 8b Task 5)`.

### Task 6: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (new steps after the last one);
  - `crates/vizctl/tests/e2e.rs`;
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–5. No production code, unless a step finds a bug. Such a bug is fixed in its owner's module, with a regression test.

- [ ] **Step 1: Update 8a's e2e** `co_producer_over_mcp` for Ruling 2. The track it renames with a token, and the one it deletes through elicitation, are made over the developer channel (`track.new_audio`), so they are the composer's and still need a yes. Its other assertions stay as they are. Two expectations change: the tracks Claude adds are named "… (Co-Producer)", and the undo labels are unchanged.
- [ ] **Step 2: Add the e2e** `co_producer_librarian_over_mcp`, `#[ignore]`, in the style of `co_producer_over_mcp` and with its MCP helper.
  - **Start.** Launch the app, switch Co-Producer on over the developer channel, then `file.new` and `file.save {path: <home>/Song.vizdaw}`.
  - **The first request.**
    1. `begin_request {name: "add a bus", asked: "give me a reverb bus"}`. Its text holds "The song's notes (Notes.md):" and "# Song".
    2. `track_new_audio`.
    3. `add_note {text: "## fake\nkeep it dark"}`.
    4. `end_request`.

    Then:
    - `Song.vizdaw/Notes.md` holds an entry headed "## <today> " … "— Add a Bus", with `- Asked: "give me a reverb bus"`, "- Note: ## fake", "- Note: keep it dark" and `- Undo step: "Co-Producer: Add a Bus"`;
    - `read_notes` answers the same text;
    - `state.get document` shows the new track marked and named "… (Co-Producer)".
  - **Tidying its own track.** In a request, `change` with `mixer.set_colour` of the new track's strip, then `track.delete` of it, applies at once with no token and no elicitation. The track is gone.
  - **Export folders.** In a request:
    - `export_folder {what: "stems", parent: <home>}` makes "<home>/Song — Stems — <today>";
    - a second, without `parent`, makes "… <today> 2" in the same parent, from the notes' memory;
    - `export_folder {what: "midi", parent: <home>/Song.vizdaw}` is refused.
  - **Make Mine and Song Notes over the developer channel.** In a request, add a track over MCP. Then:
    - `track.make_mine {seq, lane}` unmarks it, and its name loses " (Co-Producer)";
    - `window.song_notes` answers the notes path, which exists. That nothing is launched is pinned by Task 5's `overTheChannelItOpensNothing`.
  - **An untitled song.**
    1. `file.new`, then a request over MCP that adds a track.
    2. `<support>/Co-Producer/Untitled/<id>/Notes.md` and one copy exist.
    3. `file.save {path: <home>/Second.vizdaw}`.
    4. Both are now inside `Second.vizdaw`, and the stand-in folder is gone.
  - **The gate.** A raw client on the Co-Producer socket gets `not_allowed` for `command.run track.make_mine`, `command.run window.song_notes` and `command.run mixer.set_colour`.
- [ ] **Step 3: Write the README steps** in `app/README.md`, after the last step, under a heading "Plan 8b, the Co-Producer notes file and the librarian:". Each step states what to do and what is seen, and says "track":
  1. Window › Song Notes on a saved song: `Notes.md` opens in the Mac's default Markdown editor, with the song's name as its heading. Write "never touch the piano part" and save.
  2. Ask Claude for something new ("add a reverb bus"). Claude first reads the notes and mentions your rule. When it is done, Window › Song Notes shows one new entry at the end: what you asked, what was added, the undo step and the saved copy. Your own text is untouched.
  3. Keep `Notes.md` open in the editor, type a line, and ask Claude for another change without saving first. After saving, both your line and Claude's new entry are there.
  4. The tracks and buses Claude added are named "… (Co-Producer)", drawn in the theme's accent colour, and show the small Co-Producer badge on the track header and the mixer strip. Check this in each theme.
  5. Ask Claude to rename or colour one of its own tracks: it happens without asking you. Then record a few notes on that track and ask Claude to delete it: now it asks for your yes first.
  6. Track › Make Mine on a Co-Producer track: the badge and the "(Co-Producer)" text go, and the track uses its kind's colour again. Undo brings them back. Renaming a Co-Producer track yourself also makes it yours.
  7. Duplicate a Co-Producer track yourself: the copy is yours, with no badge.
  8. Ask Claude to export stems: the folder is "<song> — Stems — <date>", next to the song or where you told Claude the first time. A second export the same day makes "… 2". Exports never include `Notes.md`.
  9. In a new untitled song, ask Claude to add a track, then File › Save: the song's package now holds `Notes.md` and `Co-Producer/Copies`.
  10. With a song that has Co-Producer copies, Media Pool › Compact: after Compact, a sheet lists each copy with its size, none ticked. Keep All removes nothing. Tick one and Remove Selected: it goes to the Trash. After the next Save, the files only it used go too.
  11. Compact Media asked of Claude is still refused, and no sheet appears.
- [ ] **Step 4: Update the backlog** (`docs/feature-backlog.md`).
  - In the "Co-Producer over MCP" row, note 8b's delivery: `Notes.md` with one entry per request, the marks and badge, Make Mine, format 10, dated export folders, the move-in on first save, and Compact's offer.
  - Close or mark done 8a's rows "Compact Media offers to remove old Co-Producer copies…" and "Move an untitled project's Co-Producer copies into its package on first save…".
  - In the "User track colours" row, note `mixer.set_colour`.
  - **New rows:**
    - "Marks on regions, notes and automation Co-Producer writes onto the composer's tracks" — 8e;
    - "A Settings export location for untitled songs (spec §7.3); until then Co-Producer asks";
    - "Save As of a saved song to a new package carries Notes.md along";
    - "Untitled songs closed unsaved leave a stand-in folder of notes and copies in the support folder; show and clear them in Settings";
    - "A copy whose name clashes stays in the stand-in at first save";
    - "Open Notes.md with `O_NOFOLLOW` once a way without a libc dependency is found";
    - any narrow edge cases the task reports listed, at the owner's review pace.
- [ ] **Step 5: Run the gates:**
  - `cargo test --workspace`;
  - both clippies;
  - `cargo clippy -p core-ffi --lib -- -D warnings`;
  - `scripts/build-ffi.sh`;
  - `scripts/swift.sh build`;
  - `scripts/swift.sh test`;
  - the no-brand build, `VIZDAW_NO_BRAND=1 scripts/swift.sh build --scratch-path <fresh temp dir>`;
  - `scripts/make-app.sh`;
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: the baseline's ignored tests plus one run, and all pass.

  Then `pgrep -lf MacOS/VizDAW` and `pgrep -lf vizdaw-mcp` print nothing. Report against the baseline: registry +4, `COMMAND_NAMES` 21, error codes 16, README +11, e2e +1, `VERSION` 10.
- [ ] **Step 6: Commit** `docs(app): Co-Producer notes and librarian README checks, e2e and backlog (plan 8b Task 6)`.

## Self-Review

1. **Spec coverage (the §12 row 8b).**
   - **`Notes.md`** (§7.1):
     - read: Task 3's `co_producer_notes`, Task 4's `read_notes` and the first-request notes;
     - append only, safely beside another editor: Ruling 4 and `appending_never_rewrites` (§11.10);
     - one entry per request, newest last, with what was asked, what was done, the files, the undo step, and the composer's edits left out: Ruling 5, `a_request_writes_one_entry` and `hand_edits_are_not_logged`;
     - Window › Song Notes: Task 5;
     - the heading on first need: `a_new_song_gets_a_heading`;
     - no notes in exports: nothing copies the package's top level into an export, and README step 8 checks it.
   - **The marks** (§7.2): the name, the colour and the badge (Rulings 1 and 3, Tasks 2 and 5); take-over by rename and Make Mine, on the Track menu and the context menu (Task 2's tests, Task 5); marks on sends and plug-ins Co-Producer adds to the composer's tracks (`acting_steps_mark_what_they_make`). Regions and automation wait for 8e ("Not in this plan").
   - **§6.5's "Co-Producer's own additions need no yes"**: Ruling 2, with the stricter rule for filled tracks.
   - **The project format change** (§5, §11.8): `VERSION` 10, and older projects load with no marks and no notes.
   - **Dated export folders** (§7.3): Ruling 6, with the parent order, the memory in the notes and the numbering. The untitled Settings location is a backlog row, and its "ask" fallback is the answer.
   - **Untitled songs' notes and copies moved on first save** (§6.7; P9): Ruling 7 and Task 3.
   - **Compact's offer** (§6.7; moved from 8a): Ruling 8 and Tasks 3 and 5.
   - **The track colours gap** (§10 item 6): `mixer.set_colour`, Ruling 3.
   - **Deferred by 8a's ledger to 8b:** Compact's offer, the move-in, and the untitled copies' media need (8a backlog row), all covered.
2. **Steps.** Every test step names its tests and assertions with the plan's values: sentences, file names, folder names, counts and dates. Code steps give signatures and point to the ruling. The notes entry, the tool descriptions and the sheet copy are written out, because the spec leaves them to the plan (§7.1 "controller's call on the layout", §11.12).
3. **Type consistency.** These names are spelled the same in every task:
   - `ExportWhat`/`ExportWhatRow`, `MAX_ASKED`, `MAX_NOTE`, `MAX_NOTE_LINES`;
   - `marks::{SUFFIX, lane_strip, settle}`, `LaneCmd::MakeMine`, `MixerCmd::MakeMine`, `ChangeItem.co_producers`;
   - `LaneRow.co_producer`, `StripRow.co_producer`, `make_mine_track`, `make_mine_strip`;
   - `notes::{NOTES_FILE, MAX_READ, ensure, read, append, entry, remembered_folder, make_export_folder, move_in, RequestLog}`;
   - `NotesRow`, `CopyRow`, `RemovedCopiesRow`;
   - `notes_path`, `co_producer_notes`, `co_producer_set_asked`, `co_producer_note`, `co_producer_file`, `co_producer_export_folder`, `co_producer_copies`, `remove_co_producer_copies`;
   - `SongNotes.open`, `CopiesOffer`, `MediaModel.compactFromMenu`, `AppIcon.coProducerBadge`;
   - the ids `track.make_mine`, `mixer.make_mine`, `mixer.set_colour`, `window.song_notes`, `co_producer.notes`, `co_producer.note`, `co_producer.export_folder`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 1, 2, 3 and 5.
5. **Proportion.** Six tasks, as the owner asked for 26.0.0 by 2026-11-20. Code appears only as signatures, and the entry and copy text the spec leaves to the plan. The plan reuses `fill_new_eqs`'s place in `execute` for the marks, 8a's `IntoFolder` check for every export folder, `copies_home`'s stand-in, `create_dir`'s never-replace, retention's Trash for copies, and 8a's MCP helper in the e2e.
6. **Owner decisions.** None are open. Three calls are stated for the owner's review:
   - a Co-Producer track that holds regions or notes needs a yes to delete, which is stricter than spec §6.5 (Ruling 2);
   - with no Settings export location yet, an untitled song with no remembered folder asks the composer (Ruling 6);
   - the Co-Producer colour is the theme's accent (Ruling 3).
