# Co-Producer Time Tracking and Coaching Implementation Plan (8f)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The composer can choose to have VizDAW count where their time goes. It counts composing, recording, editing and mixing in minute blocks, per song and across all songs, and notes three kinds of friction: bursts of undos, the same task repeated, and long hunts through the menus. It is off until turned on in Settings › Co-Producer. It stays on this Mac in a private file in VizDAW's support folder, and one button deletes it all. Claude sees only totals, and only when the composer asks through Co-Producer ("where does my time go?", "what could you help with?"). Claude then suggests chores Co-Producer could take off the composer's hands. It never brings this up by itself.

**Architecture:**
- **`core-control`.**
  - Two registry commands, both class refused, so the developer channel and the e2e can drive them and Co-Producer cannot: `settings.co_producer_metrics` and `settings.delete_time_records`.
  - One protocol command on the Co-Producer socket: `co_producer.metrics` (read).
- **`core-ffi`.**
  - A new `metrics` module holds everything that is not AppKit:
    - the store's format and strict parse;
    - the private file;
    - the blocks and the friction thresholds;
    - the summary;
    - delete-all.
  - It is a `uniffi::Object`, `MetricsRecorder`, plus three free functions. All times are passed in, so every rule is tested with a fake clock.
  - `DocSnapshot` gains `step`, the history's present serial, so the app can tell a new step from an undo or a redo.
- **Swift.**
  - `VizDAWAppModel`:
    - `CoProducerMetrics`, the switch and the delete-all control;
    - `MetricsCategory.classify`, which maps the window and tool in use to a category;
    - `StepWatch`, which turns snapshot steps into step, undo and redo events;
    - `MetricsTicker`, the sampling and menu-timing state machine;
    - `CoProducerCommands` answers `co_producer.metrics`.
  - `VizDAW`: `MetricsProbe`, the AppKit side. It runs a timer, watches for input (only that it happened) and times main-menu tracking, and it adds the Time section of the Co-Producer Settings tab.
- **`vizdaw-mcp`.** One read tool, `time_report`, and an eighth recipe and prompt, `coaching`, in 8e's `recipes::RECIPES`.

**Tech Stack:**
- Rust 2024: `core-control`, `core-ffi` with UniFFI 0.32.2, `vizdaw-mcp`, `vizctl`.
- Swift 6.1 / SwiftPM with Swift Testing, macOS 14+.
- **No new third-party dependency.** Dates are `YYYY-MM-DD` strings from Swift. The one piece of date arithmetic, the cutoff for "the last N days", is a small days-from-civil helper in `metrics`.

**Spec:** `docs/superpowers/specs/2026-11-14-co-producer-design.md`, approved by the owner on 2026-11-14, is binding. This plan builds its §12 row **8f**:
- §9, metrics;
- §10 gap 13, counting and a read command;
- §2 goal 8;
- the anti-goals in §4: never interrupting, and never sending the composer's data anywhere by itself;
- §6.4's last bullet: the metrics have their own switch, off until turned on.

**The owner's decisions are binding:**
- time is counted both per song and across all songs;
- it is off until turned on in Settings;
- it stays local, with VizDAW's support files;
- Claude sees it only when asked through Co-Producer;
- it records broad categories (composing, recording, editing, mixing), from which window or tool is in use, in time blocks, not clicks;
- the friction signals are bursts of undos, repeated tasks and long menu hunting;
- coaching happens only on request and never interrupts.

**Privacy rules this plan carries** (controller's call, from the brief):
- Nothing is collected while the setting is off. No recorder exists, no sample is taken, and nothing is written or created.
- There is a "delete all" control.
- The data file is private: `time.json` is 0600, in `Metrics/` (0700), inside the support folder (0700).
- No note or MIDI content is recorded, and no file name other than the song's own name.

It builds on these plans, whose rulings stay in force. Executors read all six documents:
- plan 8a, `docs/superpowers/plans/2026-11-14-co-producer-8a.md`: Rulings 1–13, especially 5 (classes and the refused list), 10 (the switch, the support folder at 0700) and 11 (the protocol); pre-flight rulings P1–P11;
- plan 8b, `docs/superpowers/plans/2026-11-16-co-producer-8b.md`: Rulings 1–10;
- plan 8c, `docs/superpowers/plans/2026-11-16-co-producer-8c.md`: Rulings 1–8;
- plan 8d, `docs/superpowers/plans/2026-11-16-co-producer-8d.md`: Rulings 1–5;
- plan 8e, `docs/superpowers/plans/2026-11-16-co-producer-8e.md`: Rulings 1–7, especially 6 (recipes as a tool and as MCP prompts from one table).

8f runs last. It reuses 8a's switch, support folder, socket gate and `CoProducerCommands`, 8a's Settings tab, and 8e's recipes and prompts. It does not duplicate them.

**Not in this plan:**
- **A report inside VizDAW.** Settings shows one overview line ("Time records: 3 songs since 2026-11-16, 4 h 12 min in all."). The full report is what Claude reads on request. A backlog row covers a report the composer can read in Settings. (Controller's call.)
- **Playing a MIDI keyboard as composing.** Live MIDI input reaches the engine, not Swift. Composing is told from the editor and tool in use (Ruling 3). A backlog row covers it.
- **Per-day rows for Claude.** Claude gets totals over a range, never days, blocks or events (spec §9, "never raw logs").
- **Retention.** Records stay until Delete All. At about 300 bytes per song-day, a year of daily work is well under 1 MB. The read cap is 8 MiB (Ruling 5). A backlog row covers folding old days.
- **A view.** Spec §6.3 lists no metrics view. `time_report` is text only.

## Plan Rulings

These are this plan's decisions where the spec or 8a–8e leave a choice. Each one names its cost if wrong.

1. **The switch and delete-all (spec §9, §6.4).**
   - **The setting.** The `UserDefaults` key is `VizDAW.coProducerMetrics` (Bool). Absent, or not a Bool, means off. It is independent of `VizDAW.coProducer`: counting works with Co-Producer off, and reading needs Co-Producer on (spec §9).
   - **Turning it on:**
     1. makes `<support>` private by 8a Ruling 10's rule, which `SupportFolder.makePrivate` now shares (Task 4): this user's, mode 0700;
     2. opens `MetricsRecorder.open(dir: <support>, …)`;
     3. starts `MetricsProbe`.

     A failure leaves the switch off, stores `false`, and sets `problem` to "Time counting could not start: <reason>".
   - **Turning it off, or quitting:**
     1. stops the probe;
     2. calls `close()`, which writes the friction counts that are pending and drops the open block;
     3. drops the recorder.

     Nothing is sampled from then on. A partial block is never written.
   - **Delete All Time Records…** is a button in the same section. It is enabled when `metrics_overview` finds records.
     - It asks first, with an alert from the model's `confirmingDelete`: the title "Delete all time records?", the message "This deletes everything VizDAW has counted about where your time goes, for every song. It cannot be undone.", and the buttons "Delete" (destructive) and "Cancel".
     - "Delete" runs `recorder?.clear()`, then `metrics_delete_all(<support>)`.
     - If the switch is on, counting goes on into a fresh file.
   - **Only the composer.** Both registry commands are class refused (8a Ruling 5), so Co-Producer can neither turn counting on or off nor delete records. Over the developer channel, `settings.delete_time_records` deletes without the alert, as programmatic calls open nothing.
   - **Cost if wrong:** up to one block (a minute) is lost at each switch-off or quit.
2. **What is stored, and where (spec §9; the privacy rules).**
   - **The file.** `<support>/Metrics/time.json`, one JSON object, strictly parsed (`deny_unknown_fields`):
     ```json
     {"version": 1, "since": "2026-11-16",
      "songs": {"<project id>": {"name": "Night Drive", "days": {"2026-11-16": <day>}}},
      "untitled": {"days": {"2026-11-16": <day>}}}
     ```
     A `<day>` is `{"seconds": {"composing": n, "recording": n, "editing": n, "mixing": n, "other": n}, "undo_bursts": n, "repeats": {"<label>": n}, "hunts": n, "hunt_ends": {"<label>": n}}`.
   - **Keys.** A song is keyed by its project ID, so a renamed song keeps its history. A song key must pass `core_model::document::check_project_id`. A document with no project ID (never saved) counts under `untitled`, which is shown as "Untitled songs". Its time stays there after a first save. (Controller's call: stamping an ID would mark the document edited.)
   - **The name** is the document's title, which is the song's own name. It is never a path. It is trimmed, has control characters dropped, is cut to `NAME_MAX` 128 characters, and is updated by each counted block. No other name is stored.
   - **Labels** are the only other text. A label is kept only if it is one of VizDAW's own words: a `core_control::registry::COMMANDS` title, compared with its trailing "…" removed. Anything else is stored as "another edit" for steps and "another item" for menus. A hunt that chose nothing is "nothing chosen". So no template, track, region or request name can reach the file. A day holds at most `MAX_LABELS` 32 labels of each kind; past that, counts go to "another edit" or "another item".
   - **Never stored:** notes, MIDI, events, key codes, click positions, paths, file names, track names, plug-in names, request names.
   - **Bounds on read.** Counts are `u32`, and seconds are `u64` at most 10^9 per day and category. Dates are `YYYY-MM-DD`. There are at most 100 000 songs. The file is at most `MAX_STORE_BYTES` 8 MiB. Anything else is a damaged store (Ruling 5).
   - **Cost if wrong:** a repeated task whose undo name differs from every registry title shows as "another edit" and is not coached by name. The repeat signal ignores "another edit" (Ruling 4).
3. **Categories from the window and tool in use (spec §9).** `MetricsCategory.classify(_ c: MetricsContext) -> MetricsCategoryRow?` is pure. `nil` means idle, which is not counted. The rules, in order:
   1. any open document is recording: `.recording`;
   2. not active, meaning not playing and not (the app is active and the last keyDown, mouseDown or scrollWheel was at most `IDLE_SECS` 90 s ago): `nil`;
   3. the window is `.mixer` or `.plugin`: `.mixing`;
   4. the window is `.document` or `.editor`:
      - `.composing` when its detail editor is `.pianoRoll` or `.drumGrid` and the tool is `.pencil` or step entry is on;
      - otherwise `.editing`, which covers the arrangement, list, strip chart and audio editor, and the arrow and eraser tools;
   5. the window is `.mediaPool` or `.modifyPanel`: `.editing`;
   6. `.settings`, `.pluginManager`, `.other` or none: `.other`.

   - **The window** is the key window's `ControlWindows` tag while the app is active, else the last key window's tag.
   - **The detail editor** is the document's `layout.detail?.kind`, or for a torn-off editor window its own kind.
   - **The song** is the recording document while recording, else the window's document, else `appStore.frontDocument`. It is `MetricsSongRow {id: try? session.projectId(), name: snapshot.title}`.
   - **"other"** is a fifth category (controller's call): it holds Settings, the Plugin Manager and the like, so the four the owner named are not padded with setup time.
   - **Cost if wrong:** time spent reading the arrangement while playing counts as editing.
4. **Blocks and friction (spec §9; §11.11 leaves the block length, signals and thresholds to the plan).** These live in `core-ffi/src/metrics/blocks.rs`, and every time is passed in.
   - **Samples.** The probe calls `sample` every `SAMPLE_MS` 5 000. A block is `BLOCK_MS` 60 000.
     - A block starts at the first sample after the last one closed.
     - It closes at the first sample with `ms >= start + BLOCK_MS`, and that sample starts the next block.
     - A sample at or after `start + 2 × BLOCK_MS` (sleep, a stall) closes the old block with what it had, and the gap is not counted.
     - A sample whose `ms` is earlier than the last one is ignored.
   - **A block counts** when at least half of its expected samples (`BLOCK_MS / SAMPLE_MS`, so 6 of 12) were active.
     - Its whole length, `BLOCK_MS / 1000` seconds, goes to the (song, category) with the most active samples.
     - A tie goes to the category in the order recording, composing, mixing, editing, other, then to the song sampled last.
     - The block lands on the day of its first sample.
     - Otherwise the block counts nothing.
   - **Undo bursts.** At least `UNDO_BURST` 5 undos in one song within `UNDO_WINDOW_MS` 60 000 make one burst. Further undos less than 60 s apart extend that burst rather than start a new one.
   - **Repeated tasks.** The same kept label (Ruling 2), committed `REPEAT_COUNT` 6 times in one song within `REPEAT_WINDOW_MS` 600 000, makes one repeat of that label. Its count then restarts. "another edit" never makes a repeat.
     - A step whose label starts "Co-Producer:" is ignored entirely: it is not the composer's friction.
   - **Long menu hunts.** A main-menu tracking session of at least `HUNT_MS` 8 000 is one hunt. It also counts one `hunt_ends` entry, for the item chosen (as a kept label, or "another item") or "nothing chosen".
   - **Steps, undos and redos come from snapshots.** `StepWatch` keeps the highest `step` seen per document, starting at the first snapshot.
     - A step above it is a new step, with the snapshot's `undoLabel`.
     - A step below the last one is an undo.
     - A step above the last one but not above the highest is a redo, which is not counted.

     Two steps between snapshots count as one: a cost accepted.
   - **Writing.** The file is written when a block closes with time or friction pending, and on `flush()` and `close()`. That is at most once a minute.
   - **Fast timing, for the e2e only.** When `VIZDAW_METRICS_FAST` is `"1"` and `VIZDAW_SUPPORT_DIR` is set, the app opens the recorder with `BLOCK_MS` 2 000 and `SAMPLE_MS` 200. Otherwise these variables are ignored.
   - **Cost if wrong:** with minute blocks, a 20-second visit to the mixer inside an editing minute is not counted as mixing.
5. **The private file (the privacy rules; control spec §8's pattern).** In `core-ffi/src/metrics/file.rs`:
   - **The folder.** `<support>/Metrics` is made with `DirBuilder::mode(0o700)`. If it already exists, it must be a directory and not a link (by `symlink_metadata`), with the same uid as `<support>`; its mode is then set to 0700. Otherwise the call is refused: "The Metrics folder is not VizDAW's (<what>).".
   - **Writing.**
     - The temp file `time.json.tmp-<pid>` is opened with `create_new` and mode 0600, written, `sync_all`ed, then renamed onto `time.json`.
     - An existing `time.json` that is a link or not a regular file refuses the write, and nothing is written.
     - A stray temp file from a crash is removed at open.
   - **Reading.** A missing file is an empty store. A file over 8 MiB, one that fails the strict parse, or one with a bad key, date or bound is damaged:
     - it is renamed to `time.damaged.json`, or "time.damaged 2.json" and so on. It is never replaced;
     - an empty store starts;
     - `problem()` gives "The time records could not be read, so they were set aside as <name> and counting started afresh.". Settings shows it.
   - **Delete all.** `metrics_delete_all(support)` removes `<support>/Metrics` when it is a real directory, by `std::fs::remove_dir_all`, which does not follow links. A link there is removed as a link and its target is untouched. A missing folder is `Ok`.
   - **`vizdaw-mcp` never opens `Metrics/`.** The read goes through the app (Ruling 6), so the Co-Producer switch gates it. A test in Task 5 scans `vizdaw-mcp/src` for `"Metrics"` and `time.json`.
   - **Cost if wrong:** a damaged file is kept aside, not merged. The composer can delete it with Delete All.
6. **The read, only on request (spec §9, §4).**
   - **The protocol command** is `co_producer.metrics {days?: Int 1–3650, window?}`.
     - It is read class and needs no open request.
     - On the developer socket it is `not_allowed` "only on the Co-Producer socket" (8a Ruling 11).
   - **The app's answer.**
     1. `flush()`, when counting is on;
     2. the `window`'s document's project ID, when `window` is given, by 8a's window rules;
     3. `metrics_summary(support, today, days, song_id)`.

     It answers `{on, since, days, all, songs, more_songs}`:
     - `all` and each song are `{seconds: {composing, recording, editing, mixing, other}, undo_bursts, repeats: [[label, n]], hunts, hunt_ends: [[label, n]]}`;
     - each song also has `name`, `this_song` and `untitled`;
     - `songs` holds at most `MAX_SONGS_SHOWN` 20, the most time first, always including `this_song`;
     - `repeats` and `hunt_ends` hold at most 10 for `all` and 5 for each song, largest first, then by label;
     - `days: null` means all time; `days: n` is today and the n − 1 days before it.
   - **No resource and no push.** `co_producer.metrics` is not a `state.get` target, so it is no MCP resource either. Nothing is offered to the host unasked, and no notification carries it.
   - **The tool.** `time_report {days?, window?}` has `readOnlyHint: true` and sends `co_producer.metrics`. Its description, exactly: "Read where the composer's time has gone in VizDAW, counted on their Mac if they turned it on: time composing, recording, editing, mixing and other, per song and across all songs, with friction signals (bursts of undos, tasks repeated many times, long hunts through the menus). Call it only when the composer asks about their time or habits, or what you could help with. Never bring it up yourself. Read the coaching recipe first. Changes nothing."
   - **Its text:**
     - With `since: null` and `on: false`: "Time counting is off. The composer can turn it on in VizDAW's Settings › Co-Producer › Count where my time goes.".
     - With `since: null` and `on: true`: "No time has been counted yet.".
     - Otherwise:
       - "Time counted on this Mac since <since>" plus " (all time):" or " (the last <n> days):";
       - "All songs: <total> — composing <d>, recording <d>, editing <d>, mixing <d>, other <d>.";
       - "Friction: <n> undo bursts; repeated tasks: <label> ×<n>, …; long menu hunts: <n>, ending at <label> ×<n>, …." Empty lists read "none";
       - "Songs, most time first:" and one "- <name>: <total> — <categories with time>; <n> undo bursts; <n> long menu hunts" line each, with " (this song)" after the name for `this_song`;
       - "- and <n> more songs" when `more_songs > 0`;
       - when `on` is false, a last line "Counting is off now; these are earlier records.".
     - A duration `<d>` is "<h> h <m> min", "<m> min", or "under a minute".
     - Every name and label has 8a's `disguises` replaced by U+FFFD and passes `rpc::truncate`.
   - **The server's instructions** gain: "Only call time_report when the composer asks where their time goes or what you could help with; never raise it yourself."
   - **Cost if wrong:** a host that ignores tool descriptions could call `time_report` unasked. It reads totals, changes nothing, and the composer chose to count.
7. **Coaching on request (spec §9; 8e Ruling 6).** `recipes::RECIPES` gains an eighth entry, last: `coaching`, with the title "Where does my time go, and what could you help with?". Like the other recipes, it is served as a tool answer and as an MCP prompt with no arguments. Its steps:
   1. "Call time_report. If counting is off, say where to turn it on, once, and stop."
   2. "Tell the composer in a few plain sentences where their time went, overall and for the song they are on, without judging."
   3. "Point to the friction signals: bursts of undos, repeated tasks by name, and long menu hunts and what they ended at."
   4. "Suggest at most three chores Co-Producer could take over, each tied to a signal. For example: many repeated renames or colours are one request; long hunts ending at a bounce or export are a chore to ask for; heavy mixing time fits the mix recipes; heavy editing time fits quantize, transpose and velocity chores."
   5. "Offer, do not start: begin a request only after the composer picks one."
   6. "Do not bring the time records up again unless the composer asks."

   It neither starts nor ends a request, so it is the one recipe without 8e's first and last steps. 8e's tests that every recipe begins and ends with those steps exempt `coaching` by name.
   - **Cost if wrong:** a text change, with no code risk.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift.**
  - Build only through `scripts/build-ffi.sh` (after any FFI change) and `scripts/swift.sh build|test`, plus `scripts/make-app.sh` in Tasks 4 and 6. Never run `swift`, `xcodebuild` or `xcrun` directly.
  - Swift 6.1:
    - no `[weak self]`, and no non-Sendable value captured inside `MainActor.assumeIsolated`;
    - no `@State` or `@FocusState`: view state lives in the `@Observable` models.
  - Every new control has an accessibility label, and colours, fonts and spacing come from theme tokens (`theme.text…`, `theme.type…`, `theme.spacing…`).
- **Tests never touch the machine.**
  - They never open real audio or MIDI, and never trigger the microphone or TCC prompt.
  - They never touch the real `~/Library`: Application Support/VizDAW, preferences, the Trash, `Audio/Plug-Ins`. Use temp folders, `MemoryDefaults()`, the e2e's throwaway home, and fakes.
  - Every `metrics` test passes its own temp support folder as `dir`. No `metrics` function has a default folder.
  - Socket tests use a private temp folder under `/tmp`, as `server.rs`'s `private_tmp` does. `vizdaw-mcp` tests set `VIZDAW_SUPPORT_DIR` on the child they spawn, never in their own process.
  - Every `core-ffi` test file calls `force_offline()` through its `serial` helper.
  - Swift tests use a fake `MetricsRecording` and a temp `supportDir`. Those that make a real `Session` set `TestSupportDir.shared`.
  - Linux CI has no AU or CoreFoundation. The `metrics` module and its tests are plain Unix and run on Linux, as do `vizdaw-mcp` and the `core-control` tests.
  - The e2e uses vizctl's throwaway home.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`.
  - Never read the owner's off-limits research folder outside this repo, which 8b's Global Constraints name. Its path holds a banned name, so this plan does not spell it.
- **Programmatic calls open nothing.** Control-channel and Co-Producer calls, and programmatic calls, never open dialogs, sheets, alerts, panels, menus or windows. Only the composer's own click on Delete All Time Records… shows its alert.
- **Privacy.**
  - With the switch off, no `MetricsRecorder` exists, `MetricsProbe` is stopped, and `<support>/Metrics` is neither created nor written.
  - The file holds only what Ruling 2 lists.
  - The input monitor records only the time of the last input: never the key, the position or the window.
  - The menu watcher records only a duration and the chosen item's title, which `note_menu` then keeps only if it is a registry title.
- **Hostile input.**
  - Every MCP input is hostile: every MCP message, `time_report`'s arguments, `co_producer.metrics`'s params, and the whole of `time.json` on disk.
  - Song names and labels from the store are hostile too, and so are titles and labels passed to the recorder from Swift.
  - Every new FFI export runs inside `ffi_guard` (and so `catch_unwind`). It returns `Result<_, VizError>`, or a safe default for the never-failing ones (`sample`, `note_*`, `clear`, `problem`).
  - `vizdaw-mcp` handles each message inside `std::panic::catch_unwind`. A panic answers `-32603` and the server goes on.
  - Each new command and call gets hostile-input tests: wrong types, unknown params or keys, missing required params, a 70 000-character string, a NUL, and numbers out of range.
- **Who may act.**
  - Co-Producer can only read time records, with `co_producer.metrics`.
  - `settings.co_producer_metrics` and `settings.delete_time_records` are class refused: `command.run` of either on the Co-Producer socket is `not_allowed`.
- **Counts.** Check every count at the start of Task 1, and report each task's change against it.
  - The registry grows by **2**, from the count measured at the start. It is expected to be 212, after 8e's +3.
  - `COMMAND_NAMES` grows by **1**, expected 27 → 28. `ErrorCode::ALL` stays as measured, expected 16.
  - `PREVIEWABLE` stays as measured, expected 26.
  - `recipes::RECIPES` grows by **1**, expected 7 → 8.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list. The list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`); read them, keep all new text clear of them, and never copy the list.
  - The list includes the feature's old working name, which plan 8a added to the scans (spec, "Name"). Like every banned name, it is never written in the repo, this plan included.
  - Settings copy, tool descriptions, the coaching recipe, report text and refusals are covered by the same rule.
  - The feature is "Co-Producer" everywhere.
  - Text the app or a view shows says "track", never "lane".
- **Merging with release-fixes.** `session.rs` gains only the one line that fills `DocSnapshot.step`. The metrics code lives in `crates/core-ffi/src/metrics/`. This plan adds no command named in the release-fixes plan. At the merge, the registry count tests take this branch's count plus the fixes' 4, as 8e's "At the release-fixes merge" says.
- **Commits.**
  - Stay on branch `takes-and-co-producer`. Never switch branches. Never push: the owner is asked before every push.
  - Other agents work in this worktree. Commit at the end of every task by explicit paths, never `git add -A`, and retry when the index is locked. Never commit `.superpowers/`, `.DS_Store`, `target/`, generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`) or `dist/`.
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
  - `metrics` uses no `unsafe` and no OS API beyond `std::fs` and `std::os::unix::fs`.
- **Format.** This plan changes no project format: time records are not in the package (spec §9). `core_model::file::VERSION` stays as measured at the start, expected 10.
- **TDD.** Every new or changed test is run and seen to fail before the code that makes it pass.
- **Each task ends green.**
  - Always:
    - `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches.
  - Tasks 1–4 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Known flaky: the `damaged_files` and `dawproject_fuzz` proptests, `platform-macos/tests/midi.rs`, `core-ffi` catalog tests under load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline: check at start.** At the start of Task 1, measure and record:
    - HEAD, which is 8e's last commit on `takes-and-co-producer`, or this plan's pre-flight commit if there is one;
    - the registry count (expected 212);
    - `COMMAND_NAMES` (expected 27) and `ErrorCode::ALL` (expected 16);
    - `PREVIEWABLE`'s length (expected 26, after 8e);
    - `recipes::RECIPES`'s length (expected 7);
    - the last README step (expected 389, after 8e Task 7);
    - the e2e's ignored tests (expected 37);
    - `VERSION` (expected 10);
    - whether release-fixes has merged (expected: no).

    If 8a, 8b, 8c, 8d and 8e are not all fully committed, stop and say so. If a measured value differs from the expected one, use the measured value and report the difference.

## Review Focus

These are the five failure modes a composer is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **Something is counted while the setting is off.**
   - Examples:
     - a fresh install;
     - Co-Producer on but counting off;
     - counting turned off in the middle of a block;
     - quitting with a block open;
     - a damaged setting.
   - Expected: no recorder, no samples, no `Metrics` folder created, and the open block dropped.
   - Pinned by Task 3's `nothingIsCountedWhileOff` and `turningOffDropsTheOpenBlock`, Task 2's `close_drops_the_open_block`, and the e2e's first and last stages.
2. **The private file meets an odd or hostile disk.**
   - Examples:
     - `Metrics` is a link, or `time.json` is a link;
     - `Metrics` is 0755, or `time.json` is 0644;
     - truncated JSON, a 9 MiB file, an unknown key, a bad date, a song key that is not a project ID, `u64::MAX` seconds;
     - a stray temp file;
     - "time.damaged.json" is already taken.
   - Expected: links are never followed or written through, modes end 0700 and 0600, a damaged file is set aside under a new name and counting goes on, and nothing panics.
   - Pinned by Task 2's `the_store_stays_private`, `a_damaged_store_is_set_aside` and `delete_all_never_follows_links`.
3. **Content leaks into the file.**
   - Examples:
     - a template named "Mum's Swing";
     - a request step "Co-Producer: Tidy Lead Vox";
     - a track rename "Rename Lead Vox";
     - a song title holding a path, a newline or 70 000 characters;
     - a menu item from a plug-in's name.
   - Expected: only registry titles, "another edit", "another item" or "nothing chosen" are stored as labels. The name is the bounded song title. No `/` appears anywhere in the file that the composer did not put in the song's name.
   - Pinned by Task 2's `only_vizdaws_words_are_kept` and the e2e's file scan.
4. **Clocks and documents behave oddly.**
   - Examples:
     - the Mac sleeps mid-block;
     - the clock goes back;
     - a block spans midnight;
     - two songs alternate within one block;
     - an untitled song is saved;
     - five undos come in one snapshot, or undo and redo alternate.
   - Expected: gaps are not counted, a backwards sample is ignored, the block lands on its first day, the song with the most samples wins, and the untitled time stays untitled. Redo is not an undo.
   - Pinned by Task 2's `blocks_survive_odd_clocks` and `friction_thresholds`, and Task 3's `stepsUndosAndRedosAreTold`.
5. **Claude reaches the records it should not, or reads them badly.**
   - Examples:
     - `co_producer.metrics` on the developer socket;
     - `command.run settings.delete_time_records` or `settings.co_producer_metrics` on the Co-Producer socket;
     - `days: 0`, `days: 3651` or `days: "7"`;
     - a song named `a\u{202E}b`;
     - 500 songs;
     - Co-Producer off.
   - Expected: `not_allowed` or `invalid_params` as Ruling 6 says. Names show with U+FFFD, lists stop at 20 with "and N more songs", and with Co-Producer off the tool says 8a's sentence.
   - Pinned by Task 1's `metrics_reads_only_on_the_co_producer_socket` and `time_settings_are_never_co_producers`, and Task 5's `time_report_refuses_hostile_input` and `time_report_is_shown_safely`.

---

### Task 1: The settings commands and the read protocol (`core-control`)

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (the two entries; `settings.co_producer_metrics`'s `on` param, beside `settings.co_producer`'s);
  - `crates/core-control/src/request.rs` (`COMMAND_NAMES`; `co_producer.metrics`);
  - `crates/core-control/src/co_producer.rs` (`admit` for `co_producer.metrics`; the refused class for the two entries);
  - `crates/core-control/tests/co_producer.rs`, `tests/commands.rs`, `tests/protocol.rs`;
  - `crates/core-ffi/src/control.rs` (`ControlRequest::CoProducerMetrics` and its mapping);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` and `CoProducerCommands.swift` (temporary arms answering `.failed(.of(.unavailable, "Not ready yet."))` until Task 3).

**Interfaces:**
- Consumes: 8a's `co_producer::{admit, effective_class}`, `registry::validate_args`, and the `settings.co_producer` entry as the pattern.
- Produces:
  ```rust
  // registry.rs: two CommandSpec entries, key_only, menu "Settings", document: false, class Refused:
  //   "settings.co_producer_metrics" {on: Flag, required}, title "Count Where My Time Goes"
  //   "settings.delete_time_records" {},                  title "Delete All Time Records"
  pub const METRICS_SETTING: &str = "settings.co_producer_metrics";
  pub const DELETE_TIME_RECORDS: &str = "settings.delete_time_records";
  // request.rs, Command gains:
  CoProducerMetrics { days: Option<u32>, window: Option<WindowRef> },   // days 1..=3650
  pub const METRICS_DAYS_MAX: u32 = 3650;
  ```
  In `core-ffi/src/control.rs`, `ControlRequest` gains `CoProducerMetrics { days: Option<u32>, window: Option<WindowRefRow> }`.

- [ ] **Step 1: Measure the baseline** (Global Constraints) and write it at the top of the task report.
- [ ] **Step 2: Write the failing tests.**
  - In `tests/co_producer.rs`:
    - `time_settings_are_never_co_producers`:
      - both entries are `Refused`;
      - `list_json()` on the Co-Producer socket lists neither;
      - `command.run` of either on the Co-Producer socket is `not_allowed`;
      - on the developer socket, `settings.co_producer_metrics {on: true}` and `settings.delete_time_records {}` are admitted.
    - `metrics_reads_only_on_the_co_producer_socket`:
      - on a developer server, `co_producer.metrics {}` is `not_allowed` "only on the Co-Producer socket";
      - on the Co-Producer socket, `co_producer.metrics {}` and `{days: 7, window: 3}` are admitted with no open request.
    - `metrics_read_refuses_hostile_input`. Each is `invalid_params`:
      - `days` of `0`, `3651`, `-1`, `"7"`, `7.5`, `null` or `2^53`;
      - `{extra: 1}`;
      - `window` of `"key"` given as a number, or out of range, by 8a's window rules.
  - In `tests/commands.rs`:
    - the registry count is the baseline plus 2, and `COMMAND_NAMES.len()` is the baseline plus 1;
    - `settings.co_producer_metrics` with no `on`, or `on: "yes"`, is `invalid_params`.
  - In `tests/protocol.rs`: the round-trip parse of `co_producer.metrics {}` and `co_producer.metrics {days: 30, window: 2}`.
- [ ] **Step 3: Run** `cargo test -p core-control`. Expected: FAIL.
- [ ] **Step 4: Implement** the two entries, the command, and their place in `admit`'s Co-Producer-only list (8a Ruling 11). `core-ffi/src/control.rs` maps the request. Swift gets the temporary arms, so `everyRegisteredMenuCommandHasAHandler` passes.
- [ ] **Step 5: Run** `cargo test -p core-control -p core-ffi`. Expected: PASS, including `names.rs`.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. **Commit** `feat(control): time-record settings commands and the Co-Producer metrics read (plan 8f Task 1)`.

### Task 2: The recorder, the private store and the summary (`core-ffi`)

**Files:**
- Create:
  - `crates/core-ffi/src/metrics/mod.rs` (`MetricsRecorder`, the rows, the free functions, the constants);
  - `crates/core-ffi/src/metrics/store.rs` (the format, strict parse, merge, labels);
  - `crates/core-ffi/src/metrics/blocks.rs` (blocks and friction, Ruling 4);
  - `crates/core-ffi/src/metrics/file.rs` (Ruling 5);
  - `crates/core-ffi/src/metrics/summary.rs` (Ruling 6's totals; the days-from-civil helper);
  - `crates/core-ffi/tests/metrics.rs`.
- Modify:
  - `crates/core-ffi/src/lib.rs`;
  - `crates/core-ffi/src/snapshot.rs` (`DocSnapshot.step`);
  - `crates/core-ffi/src/session.rs` (one line: `step: inner.history.present_serial()` where the snapshot is built);
  - `crates/core-ffi/src/control.rs` (`ControlResult::CoProducerMetrics` and its JSON);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` and any Swift test that builds a `DocSnapshot` (`step: 0`).

**Interfaces:**
- Consumes: `core_control::registry::COMMANDS` titles; `core_model::document::check_project_id`; `History::present_serial`; `ffi_guard`.
- Produces:
  ```rust
  pub const METRICS_DIR: &str = "Metrics";  pub const STORE: &str = "time.json";
  pub const BLOCK_MS: u64 = 60_000;  pub const SAMPLE_MS: u64 = 5_000;
  pub const UNDO_BURST: u32 = 5;  pub const UNDO_WINDOW_MS: u64 = 60_000;
  pub const REPEAT_COUNT: u32 = 6;  pub const REPEAT_WINDOW_MS: u64 = 600_000;
  pub const HUNT_MS: u64 = 8_000;  pub const MAX_LABELS: usize = 32;  pub const NAME_MAX: usize = 128;
  pub const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;  pub const MAX_SONGS_SHOWN: usize = 20;
  #[derive(uniffi::Enum)] pub enum MetricsCategoryRow { Composing, Recording, Editing, Mixing, Other }
  #[derive(uniffi::Record)] pub struct MetricsClockRow { pub ms: u64, pub day: String }   // day "YYYY-MM-DD", local
  #[derive(uniffi::Record)] pub struct MetricsSongRow { pub id: Option<String>, pub name: String }
  #[derive(uniffi::Object)] pub struct MetricsRecorder { /* Mutex<…> */ }
  #[uniffi::export] impl MetricsRecorder {
      /// Ruling 5's folder checks and read; `support` is VizDAW's support folder.
      #[uniffi::constructor] pub fn open(support: String, block_ms: u64, sample_ms: u64) -> Result<Arc<Self>, VizError>;
      pub fn problem(&self) -> Option<String>;
      pub fn sample(&self, clock: MetricsClockRow, category: Option<MetricsCategoryRow>, song: Option<MetricsSongRow>);
      pub fn note_undo(&self, clock: MetricsClockRow, song: Option<MetricsSongRow>);
      pub fn note_step(&self, clock: MetricsClockRow, song: Option<MetricsSongRow>, label: String);
      pub fn note_menu(&self, clock: MetricsClockRow, duration_ms: u64, chosen: Option<String>);
      pub fn flush(&self) -> Result<(), VizError>;   // writes pending friction; keeps the open block
      pub fn close(&self) -> Result<(), VizError>;   // flush, drop the open block; later calls do nothing
      pub fn clear(&self);                            // drops everything pending (delete-all)
  }
  #[derive(uniffi::Record)] pub struct MetricsSecondsRow { pub composing: u64, pub recording: u64,
      pub editing: u64, pub mixing: u64, pub other: u64 }
  #[derive(uniffi::Record)] pub struct MetricsCountRow { pub label: String, pub count: u32 }
  #[derive(uniffi::Record)] pub struct MetricsTotalsRow { pub seconds: MetricsSecondsRow, pub undo_bursts: u32,
      pub repeats: Vec<MetricsCountRow>, pub hunts: u32, pub hunt_ends: Vec<MetricsCountRow> }
  #[derive(uniffi::Record)] pub struct MetricsSongTotalsRow { pub name: String, pub this_song: bool,
      pub untitled: bool, pub totals: MetricsTotalsRow }
  #[derive(uniffi::Record)] pub struct MetricsSummaryRow { pub since: Option<String>, pub days: Option<u32>,
      pub all: MetricsTotalsRow, pub songs: Vec<MetricsSongTotalsRow>, pub more_songs: u32 }
  #[derive(uniffi::Record)] pub struct MetricsOverviewRow { pub songs: u32, pub since: Option<String>, pub total_seconds: u64 }
  #[uniffi::export] pub fn metrics_summary(support: String, today: String, days: Option<u32>, song_id: Option<String>)
      -> Result<MetricsSummaryRow, VizError>;   // reads only; a damaged file reads as empty and is not moved
  #[uniffi::export] pub fn metrics_overview(support: String) -> Result<MetricsOverviewRow, VizError>;
  #[uniffi::export] pub fn metrics_delete_all(support: String) -> Result<(), VizError>;
  // snapshot.rs: DocSnapshot gains `pub step: u64` (History::present_serial).
  // control.rs: ControlResult::CoProducerMetrics { on: bool, row: MetricsSummaryRow }
  //   → {"on", "since", "days", "all": {"seconds": {...}, "undo_bursts", "repeats": [[l, n]], "hunts",
  //      "hunt_ends": [[l, n]]}, "songs": [{"name", "this_song", "untitled", "seconds", "undo_bursts",
  //      "repeats", "hunts", "hunt_ends"}], "more_songs"}
  ```

- [ ] **Step 1: Write the failing tests** in `tests/metrics.rs`, each with its own temp support folder (0700) and a fake clock that starts at day "2026-11-16".
  - `blocks_count_by_majority`:
    - 12 samples 5 s apart, 8 editing and 4 mixing for song A, then one more sample: A gets 60 s of editing that day;
    - 5 active samples and 7 idle ones count nothing;
    - a tie of 6 composing and 6 mixing goes to composing;
    - 6 for A and 6 for B in one category go to the song sampled last.
  - `blocks_survive_odd_clocks`:
    - a jump of 3 minutes closes the open block with what it had, and the gap counts nothing;
    - a sample earlier than the last is ignored;
    - a block starting at 23:59:30 on "2026-11-16" and closing on "2026-11-17" lands on the 16th;
    - `ms: u64::MAX` and `day: "16/11/2026"` are ignored with no panic.
  - `close_drops_the_open_block`: 11 active samples, then `close()`, leave no seconds in the file. Pending friction is written. Later `sample` calls write nothing.
  - `friction_thresholds`:
    - 4 undos in 60 s are no burst, and 5 are one;
    - 9 undos, each 10 s after the last, are still one burst;
    - 5 more undos 2 minutes later are a second burst;
    - 5 undos split across songs A and B are no burst;
    - 6 "New Audio Track" steps in 10 minutes are one repeat, 12 are two, and 6 spread over 11 minutes are none;
    - a menu session of 7 999 ms is no hunt, and one of 8 000 ms is one hunt, with `hunt_ends` counting its label;
    - a chosen title "Quantize…" is kept as "Quantize" when that is a registry title.
  - `only_vizdaws_words_are_kept`:
    - step labels "Mum's Swing", "Rename Lead Vox" and "x/y.vizdaw" are stored as "another edit";
    - "Co-Producer: Tidy Lead Vox" is not stored at all and makes no repeat;
    - 6 × "another edit" make no repeat;
    - a menu item "Some Plug-in…" is "another item", and `None` is "nothing chosen";
    - the 33rd distinct label of a day is counted under "another edit";
    - a song name "a\nb", a 70 000-character name, and one holding NUL are stored trimmed, with control characters dropped, at most 128 characters;
    - a song id that fails `check_project_id` is counted under `untitled`;
    - after a run with all of these, the file's text holds none of "Mum", "Lead Vox", ".vizdaw" or "Tidy".
  - `the_store_stays_private`:
    - after the first write, `Metrics` is 0700 and `time.json` is 0600;
    - a `Metrics` made 0755 beforehand ends 0700;
    - a `time.json` made 0644 ends 0600 after the next write;
    - `Metrics` as a link to another folder refuses `open` with "The Metrics folder is not VizDAW's (a link)." and writes nothing in the target;
    - `time.json` as a link refuses the write and leaves the target unchanged;
    - a stray `time.json.tmp-1` is removed at open.
  - `a_damaged_store_is_set_aside`:
    - each of truncated JSON, a 9 MiB file, an unknown key, `"day": "2026-13-40"`, a song key "x", and seconds `1e10` is renamed to `time.damaged.json` at `open`, with `problem()` naming it, and counting starts afresh;
    - with `time.damaged.json` taken, the next one is "time.damaged 2.json";
    - `metrics_summary` on a damaged file answers an empty summary and moves nothing.
  - `summaries_total_and_rank`, on a store of 3 songs, untitled time, and days 2026-11-01 to 2026-11-16:
    - `days: None` totals everything, `since` is "2026-11-01", and songs are ranked by total;
    - `days: Some(7)` with `today` "2026-11-16" counts 2026-11-10 to 2026-11-16 only;
    - `days: Some(30)` crosses the month boundary correctly;
    - untitled shows as "Untitled songs", `untitled: true`;
    - `song_id` of the third song sets its `this_song`;
    - with 25 songs, 20 are shown, `this_song` included even when it ranks 25th, and `more_songs` is 5;
    - `repeats` are cut to 10 for `all` and 5 for each song, largest first, then by label;
    - an empty or missing store answers `since: None`.
  - `overview_and_delete_all`:
    - `metrics_overview` counts the songs (untitled as one), gives `since` and the total seconds;
    - `metrics_delete_all` removes `Metrics`, and a second call is `Ok`;
    - `delete_all_never_follows_links`: with `Metrics` a link to a folder holding a file, delete removes the link only, and the file stays.
  - `recorder_refuses_hostile_input`: `open` with a relative `support`, a missing `support`, or one that is a file is `Err`, with no panic. `note_menu` with `duration_ms: u64::MAX` counts one hunt.
  - `snapshot_step_follows_history`: on a session, a new step raises `snapshot().step`, undo lowers it back, and redo brings it back without passing the highest.
  - `metrics_result_json`: `ControlResult::CoProducerMetrics` serialises to the keys in Interfaces.
- [ ] **Step 2: Run** `cargo test -p core-ffi --test metrics`. Expected: FAIL.
- [ ] **Step 3: Implement** Rulings 2, 4, 5 and 6's summary.
  - `store.rs` parses with `serde` and `deny_unknown_fields`, then checks bounds.
  - `blocks.rs` is pure, with no `std::fs`.
  - `file.rs` does Ruling 5's IO.
  - `MetricsRecorder` holds one `Mutex` over the store, the open block and the friction windows. A poisoned lock answers the safe default.
  - `snapshot.rs` and the one line in `session.rs` add `step`. **Swift:** test snapshots gain `step: 0`.
- [ ] **Step 4: Run** `cargo test -p core-ffi`. Expected: PASS, including 8a's `co_producer` tests and 8e's `creative` and `starter` tests.
- [ ] **Step 5: End-of-task checks**, including the Swift build and test. **Commit** `feat(ffi): private local time records in blocks, with friction signals and a summary (plan 8f Task 2)`.

### Task 3: The switch, categories, steps and the read in the app model (Swift)

**Files:**
- Create:
  - `app/Sources/VizDAWAppModel/CoProducerMetrics.swift` (`MetricsRecording`, `CoProducerMetrics`);
  - `app/Sources/VizDAWAppModel/MetricsCategory.swift` (`MetricsContext`, `classify`);
  - `app/Sources/VizDAWAppModel/StepWatch.swift`;
  - `app/Tests/VizDAWAppModelTests/CoProducerMetricsTests.swift`, `MetricsCategoryTests.swift`, `StepWatchTests.swift`.
- Modify:
  - `app/Sources/VizDAWAppModel/DocumentModel.swift` (the `snapshot` `didSet` feeds a `StepWatch`; `onStep`);
  - `app/Sources/VizDAWAppModel/DocumentStore.swift` (`stepObserver`, set on each document as it is added);
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift` (`co_producer.metrics`, replacing Task 1's arm);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (the two settings commands' real dispatch);
  - `app/Tests/VizDAWAppModelTests/CoProducerCommandsTests.swift`, `FakeSession.swift`.

**Interfaces:**
- Consumes: Task 2's `MetricsRecorder`, `metricsSummary`, `metricsOverview`, `metricsDeleteAll` and rows; Task 1's request case; 8a's `CoProducerSettings` as the pattern and its `supportDir`; `ControlWindows`' `WindowKindRow` cases; `DetailEditorKind`, `Tool` and `StepEntry.isOn`.
- Produces:
  ```swift
  @MainActor public protocol MetricsRecording: AnyObject {
      func problem() -> String?
      func sample(clock: MetricsClockRow, category: MetricsCategoryRow?, song: MetricsSongRow?)
      func noteUndo(clock: MetricsClockRow, song: MetricsSongRow?)
      func noteStep(clock: MetricsClockRow, song: MetricsSongRow?, label: String)
      func noteMenu(clock: MetricsClockRow, durationMs: UInt64, chosen: String?)
      func flush() throws
      func close() throws
      func clear()
  }
  extension MetricsRecorder: MetricsRecording {}
  @MainActor @Observable public final class CoProducerMetrics {
      public static let key = "VizDAW.coProducerMetrics"
      public static let label = "Count where my time goes"
      public static let caption: String      // Task 4's exact copy
      public private(set) var on: Bool
      public private(set) var problem: String?
      public private(set) var overview: MetricsOverviewRow?   // nil: no records
      public var confirmingDelete = false
      public private(set) var recorder: (any MetricsRecording)?
      /// `open` makes the recorder; `makePrivate` is Task 4's SupportFolder check (nil: fine).
      public init(defaults: UserDefaults, supportDir: URL, environment: [String: String],
                  makePrivate: @escaping @MainActor (URL) -> String?,
                  open: @escaping @MainActor (URL, UInt64, UInt64) throws -> any MetricsRecording)
      public func setOn(_ on: Bool)         // Ruling 1
      public func deleteAll()               // clear, then metricsDeleteAll; refreshes overview
      public func quit()                    // close; the setting is kept
      public func refreshOverview()
      public var overviewLine: String       // "Time records: N songs since D, X in all." or "No time records."
      public nonisolated static func timing(environment: [String: String]) -> (blockMs: UInt64, sampleMs: UInt64)
      public static func clock(_ date: Date) -> MetricsClockRow   // local "yyyy-MM-dd", en_US_POSIX
  }
  public struct MetricsContext: Equatable, Sendable {
      public var recording, playing, appActive: Bool
      public var secondsSinceInput: Double?
      public var window: WindowKindRow?
      public var detail: DetailEditorKind?
      public var tool: Tool
      public var stepEntry: Bool
  }
  public enum MetricsCategory {
      public static let idleSeconds: Double = 90
      public static func classify(_ c: MetricsContext) -> MetricsCategoryRow?   // Ruling 3
  }
  public enum StepEvent: Equatable, Sendable { case newStep(label: String), undo, redo }
  public struct StepWatch: Sendable {
      public init()
      public mutating func observe(step: UInt64, undoLabel: String?) -> StepEvent?   // Ruling 4
  }
  // DocumentModel: public var onStep: (@MainActor (StepEvent) -> Void)?
  // DocumentStore: public var stepObserver: (@MainActor (UUID, StepEvent) -> Void)?
  ```

- [ ] **Step 1: Write the failing Swift tests** with `MemoryDefaults()`, a temp `supportDir`, a fake `MetricsRecording` that records its calls, and `FakeSession`.
  - `CoProducerMetricsTests`:
    - `nothingIsCountedWhileOff`:
      - a new model on empty defaults has `on == false` and never calls `open`;
      - a `String` under `key` reads as off;
      - `CoProducerSettings` turned on does not turn counting on;
      - `<supportDir>/Metrics` does not exist.
    - `turningOnOpensAndPersists`:
      - `setOn(true)` calls `makePrivate(supportDir)`, then `open(supportDir, 60000, 5000)`, and stores `true`;
      - a new model on the same defaults opens at once.
    - `turningOffDropsTheOpenBlock`: `setOn(false)` calls `close()` once, drops the recorder and stores `false`. `quit()` calls `close()` and keeps `true`.
    - `aFailedStartSaysWhy`: `makePrivate` answering "the folder belongs to another user", or `open` throwing, leaves `on == false` and `false` stored, and `problem` is "Time counting could not start: <reason>". The recorder's own `problem()` shows in `problem` after a successful open.
    - `deleteAllClearsThenDeletes`: `deleteAll()` calls `clear()` before the folder is removed, and `overview` is nil afterwards. With counting on, the recorder stays.
    - `fastTimingOnlyForTests`: `timing` is (60000, 5000) by default; it is (2000, 200) only when both `VIZDAW_METRICS_FAST` is "1" and `VIZDAW_SUPPORT_DIR` is set; `VIZDAW_METRICS_FAST` alone changes nothing.
    - `clockIsLocalDays`: `clock(<2026-11-16 23:59 local>).day` is "2026-11-16".
    - `overviewLineReads`: 3 songs since "2026-11-16" and 15 120 s give "Time records: 3 songs since 2026-11-16, 4 h 12 min in all."; 1 song gives "1 song"; nil gives "No time records.".
  - `MetricsCategoryTests`, `everyRuleInOrder`, one case per rule of Ruling 3:
    - recording wins over a mixer window;
    - not playing with input 91 s ago is `nil`, and the app inactive and not playing is `nil`;
    - playing with the app inactive still counts;
    - `.mixer` and `.plugin` are mixing;
    - a piano roll with the pencil is composing, and with the arrow is editing;
    - a drum grid with step entry on is composing;
    - a list editor with the pencil is editing;
    - `.mediaPool` is editing;
    - `.settings` and nil are other.
  - `StepWatchTests`, `stepsUndosAndRedosAreTold`:
    - the first `observe(step: 4, …)` is `nil`;
    - 5 with "Rename Track" is `.newStep("Rename Track")`;
    - 4 is `.undo`, 5 again is `.redo`, and 6 is a new step;
    - the same step twice is `nil`;
    - a fall of several steps at once is one `.undo`.
  - `CoProducerCommandsTests`:
    - `metricsReadAnswersTotals`:
      - `co_producer.metrics {days: 7, window: W}` flushes the recorder when on;
      - it calls the summary with W's document's project ID and `days` 7, and answers `on` with the row;
      - it never calls `coProducerEnter`;
      - with counting off, it answers `on: false` and calls no recorder.
    - `metricsReadNeedsAKnownWindow`: an unknown window is `.failed` with 8a's window sentence.
    - `timeSettingsOverTheDeveloperChannel`: `settings.co_producer_metrics {on: true}` turns the model on; `settings.delete_time_records {}` calls `deleteAll()` and sets no `confirmingDelete`.
  - `DocumentModel` forwards each `StepEvent` to `onStep`, and `DocumentStore` forwards it to `stepObserver` with the document's ID. This is checked in `StepWatchTests`' `documentsForwardTheirSteps`, with `FakeSession` snapshots.
- [ ] **Step 2: Run** `scripts/build-ffi.sh && scripts/swift.sh test --filter 'CoProducerMetrics|MetricsCategory|StepWatch|CoProducerCommands'`. Expected: FAIL.
- [ ] **Step 3: Implement** Rulings 1, 3 and 6's app side. The `didSet` runs `StepWatch.observe` and calls `onStep` only for a non-nil event, so a document with no observer pays one comparison.
- [ ] **Step 4: Run** the Step 2 command, then `scripts/swift.sh test`. Expected: PASS, including `NameScanTests`, `SupportDirScanTests` and `everyRegisteredMenuCommandHasAHandler`.
- [ ] **Step 5: End-of-task checks. Commit** `feat(app): the time-counting switch, categories from the window in use, and the metrics read (plan 8f Task 3)`.

### Task 4: Sampling, input and menu timing, and the Settings section (Swift app)

**Files:**
- Create:
  - `app/Sources/VizDAWAppModel/MetricsTicker.swift` (the pure sampling and menu-timing state);
  - `app/Sources/VizDAWAppModel/SupportFolder.swift` (8a's owner and 0700 check, moved here);
  - `app/Sources/VizDAW/MetricsProbe.swift` (the timer, the input monitor, the menu observers, building `MetricsContext`);
  - `app/Tests/VizDAWAppModelTests/MetricsTickerTests.swift`, `SupportFolderTests.swift`.
- Modify:
  - `app/Sources/VizDAW/VizDAWApp.swift` (make `CoProducerMetrics` at launch; start and stop the probe with the switch; `quit()` at quit, before `ControlChannel.stop()`; `LiveCoProducerSink.start` calls `SupportFolder.makePrivate`; `appStore.stepObserver` forwards to the recorder);
  - `app/Sources/VizDAW/CoProducerSettingsView.swift` (the Time section).

**Interfaces:**
- Consumes: Task 3's `CoProducerMetrics`, `MetricsCategory.classify`, `MetricsContext`, `StepEvent` and `stepObserver`; `ControlWindows.tagOf`; `DocumentModel.isRecording`, `isPlaying`, `editor.layout`, `editor.tool`.
- Produces:
  ```swift
  public enum SupportFolder {
      /// Makes `dir` (creating it) mode 0700 when this user owns it; else the reason (8a Ruling 10's sentences).
      @MainActor public static func makePrivate(_ dir: URL) -> String?
  }
  public struct MetricsTicker: Sendable {
      public init(sampleMs: UInt64)
      public mutating func input(at ms: UInt64)                         // only that input happened
      public func secondsSinceInput(at ms: UInt64) -> Double?
      public mutating func due(at ms: UInt64) -> Bool                   // true once per sampleMs
      public mutating func menuBegan(at ms: UInt64)
      public mutating func menuChose(_ title: String)                   // the last item chosen while tracking
      public mutating func menuEnded(at ms: UInt64) -> (durationMs: UInt64, chosen: String?)?
  }
  ```
  **The probe** runs only while `CoProducerMetrics.on`:
  - a `Timer` at `sampleMs`, so 5 s, or 200 ms in fast timing;
  - an `NSEvent` local monitor for `.keyDown`, `.leftMouseDown`, `.rightMouseDown`, `.otherMouseDown` and `.scrollWheel`. It calls `ticker.input(at:)` and returns the event unchanged;
  - observers of `NSMenu.didBeginTrackingNotification` and `didEndTrackingNotification` for `NSApp.mainMenu`, and of `didSendActionNotification`, for the chosen item's title;
  - forwarding of the `stepObserver` events, as `noteStep` and `noteUndo`.

  Starting the probe twice is a no-op. Stopping it removes the timer, the monitor and every observer.

  **The Time section** of the Co-Producer tab, under 8a's controls, with the heading "Time":
  - `Toggle(CoProducerMetrics.label, isOn:)`, accessibility label "Count where my time goes";
  - the caption, exactly: "Counts, on this Mac only, how long you spend composing, recording, editing and mixing, per song and across all songs, in minute blocks. It also notes bursts of undos, tasks repeated many times and long hunts through the menus. It never records your notes, MIDI or any file name but the song's own. Claude sees it only when you ask through Co-Producer, and never brings it up by itself. Counting works even when Co-Producer is off.";
  - `overviewLine`, read on appear and after each change;
  - the button "Delete All Time Records…", with accessibility label "Delete all time records", disabled when `overview` is nil. It sets `confirmingDelete`, and Ruling 1's alert follows;
  - `problem`, when set, in `theme.state.warning`.

- [ ] **Step 1: Write the failing Swift tests.**
  - `MetricsTickerTests`:
    - `samplesAreDueOncePerInterval`: `due` is true at 0, false at 4 999, true at 5 000, and true once after a 3-minute gap;
    - `inputAgeOnly`: no input gives nil, and `input(at: 1000)` then `secondsSinceInput(at: 91000)` gives 90;
    - `menuSessionsAreTimed`: began at 0, chose "Quantize…", ended at 9 000 gives (9000, "Quantize…"); an end with no begin is nil; a second begin restarts.
  - `SupportFolderTests`: `makePrivate` on a new temp folder makes it and leaves 0700; on a 0755 temp folder it leaves 0700; on a path that is a file it answers a reason.
  - `NameScanTests`, `noLaneInWhatTheAppShows` and `AppIconTests` pass with the new copy.
- [ ] **Step 2: Run** `scripts/swift.sh test --filter 'MetricsTicker|SupportFolder'`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - On each due tick, `MetricsProbe` builds `MetricsContext` by Ruling 3: the key window's tag when `NSApp.isActive`, else the last one; its document's recording, playing, detail kind, tool and step entry. It then calls `recorder.sample(clock:category:song:)` with `classify(…)`.
  - Menu ends are passed to `noteMenu` as they are. The 8-second threshold is the recorder's.
  - `LiveCoProducerSink.start` uses `SupportFolder.makePrivate` instead of its own copy, and its sentences are unchanged.
- [ ] **Step 4: Run** the Step 2 command, then `scripts/swift.sh test`. Expected: PASS, including 8a's `CoProducerSettingsTests`.
- [ ] **Step 5: End-of-task checks**, including `scripts/make-app.sh`. **Commit** `feat(app): time blocks from the window in use, menu-hunt timing, and Settings › Co-Producer › Time (plan 8f Task 4)`.

### Task 5: `vizdaw-mcp`: `time_report` and the coaching recipe

**Files:**
- Modify:
  - `crates/vizdaw-mcp/src/tools.rs` (`time_report`, `time_answer`, `INSTRUCTIONS`);
  - `crates/vizdaw-mcp/src/recipes.rs` (the `coaching` entry; the begin and end steps' exemption);
  - `crates/vizdaw-mcp/src/lib.rs` (the tool in `tools/list` and its call);
  - `crates/vizdaw-mcp/tests/server.rs`, `tests/support/fake_app.rs`.

**Interfaces:**
- Consumes: Task 1's `co_producer.metrics`; Task 2's answer keys; 8e's `recipes::{RECIPES, recipes_answer, prompts_list, prompt_get}`; 8a's `disguises` and `rpc::truncate`.
- Produces:
  ```rust
  pub const TIME_REPORT: &str = "time_report";
  /// Ruling 6's text from the app's answer.
  pub fn time_answer(result: &Value) -> String;
  pub fn duration_text(seconds: u64) -> String;   // "4 h 12 min", "12 min", "under a minute"
  // recipes.rs: RECIPES gains Recipe { name: "coaching", title: "Where does my time go, and what could you help with?", steps: Ruling 7's six }
  ```

- [ ] **Step 1: Write the failing tests** in `tests/server.rs`. The fake app answers `co_producer.metrics` from scripts.
  - `time_report_is_listed`:
    - it has Ruling 6's description exactly, `readOnlyHint: true`, and an `inputSchema` of `days` (integer 1–3650) and `window`, with `additionalProperties: false`;
    - the instructions hold Ruling 6's sentence;
    - `resources/list` has no time or metrics resource.
  - `time_report_reaches_the_app`: `time_report {days: 7}` sends `co_producer.metrics {days: 7}` and nothing else, and opens no request.
  - `time_report_reads_plainly`. From a scripted answer of 2 songs:
    - the text holds "since 2026-11-01 (the last 7 days):" and "All songs: 4 h 12 min — composing 1 h 30 min, recording 40 min, editing 1 h 2 min, mixing 50 min, other 10 min.";
    - it holds "Friction: 3 undo bursts; repeated tasks: Rename Track ×2; long menu hunts: 4, ending at Quantize ×2, nothing chosen ×2.";
    - the song lines are in order, with " (this song)" on the right one;
    - `duration_text(59)` is "under a minute", `(60)` is "1 min", and `(3600)` is "1 h 0 min";
    - `since: null` gives Ruling 6's off or none sentence by `on`;
    - `on: false` with records adds "Counting is off now; these are earlier records.".
  - `time_report_is_shown_safely`:
    - a song named `"a\u{202E}b"` and a label `"x\u{2028}y"` show with U+FFFD;
    - a 70 000-character name is truncated;
    - with `more_songs: 480`, the text ends "- and 480 more songs".
  - `time_report_refuses_hostile_input`. Each is `isError: true` before any socket call:
    - `days` of 0, 3651, `"7"` or `7.5`;
    - `{extra: 1}`.

    An app answer missing `all`, or with `seconds` a string, is `isError: true` "VizDAW's answer could not be read.", with no panic.
  - `time_report_needs_co_producer`: with the off marker, the text is 8a's "Co-Producer is turned off in VizDAW's Settings.".
  - `coaching_is_a_recipe_and_a_prompt`:
    - `recipes {}` lists eight, `coaching` last;
    - `recipes {name: "coaching"}` starts with "Where does my time go, and what could you help with?:" and holds Ruling 7's six steps in order;
    - it holds neither begin_request nor end_request;
    - `prompts/list` has `coaching` with no arguments;
    - 8e's every-recipe begin and end test exempts `coaching` by name.
  - `mcp_never_opens_the_records`: no file in `crates/vizdaw-mcp/src` holds `"Metrics"` or `time.json`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-mcp`. Expected: FAIL.
- [ ] **Step 3: Implement** Rulings 6 and 7. `time_answer` reads the answer defensively, and any missing or mistyped key fails the read with the sentence above.
- [ ] **Step 4: Run** `cargo test -p vizdaw-mcp -p core-control`. Expected: PASS, including `names.rs`'s scan of `vizdaw-mcp/src`.
- [ ] **Step 5: End-of-task checks. Commit** `feat(mcp): time_report and the coaching recipe, read only on request (plan 8f Task 5)`.

### Task 6: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (new steps after the last one);
  - `crates/vizctl/tests/e2e.rs`;
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–5. No production code, unless a step finds a bug. Such a bug is fixed in its owner's module, with a regression test.

- [ ] **Step 1: Add the e2e** `co_producer_time_over_mcp`, `#[ignore]`, in the style of 8e's `co_producer_creative_over_mcp`, with 8a's MCP helper. It launches with `env` `VIZDAW_AUDIO=offline` and `VIZDAW_METRICS_FAST=1`.
  - **Off by default.**
    1. Switch Co-Producer on over the developer channel. Run `file.create_demo`, then `file.save {path: <home>/Song.vizdaw}`.
    2. `transport.play` for 3 s, then `transport.stop`.
    3. `<support>/Metrics` does not exist, and `time_report {}` gives "Time counting is off. …".
  - **Counting.**
    1. `settings.co_producer_metrics {on: true}`.
    2. `transport.play` for 6 s (three fast blocks), then `transport.stop`.
    3. Six `track.new_audio`, then five `edit.undo`.
    4. `time_report {}` gives a "- Song: " line, "1 undo bursts" or more, and a repeated task.
    5. `co_producer.metrics` on the developer socket is `not_allowed`.
  - **Private.**
    - `<support>` and `<support>/Metrics` are 0700, and `time.json` is 0600.
    - The file's bytes hold no `/`, no `.vizdaw`, and no track name of the demo's.
    - The file parses as Ruling 2's shape.
  - **Only the composer.** A raw client on the Co-Producer socket gets `not_allowed` for `command.run settings.delete_time_records` and `command.run settings.co_producer_metrics`.
  - **Delete all.** `settings.delete_time_records {}` over the developer channel: `<support>/Metrics` is gone, and `time_report {}` gives "No time has been counted yet.".
  - **Off again.**
    1. `settings.co_producer_metrics {on: false}`.
    2. `transport.play` for 3 s, then stop.
    3. `<support>/Metrics` still does not exist.
- [ ] **Step 2: Write the README steps** in `app/README.md`, after the last step, under a heading "Plan 8f, Co-Producer time tracking and coaching:". Each step states what to do and what is seen, and says "track":
  1. Open Settings › Co-Producer on a fresh install: "Count where my time goes" is off, and the overview reads "No time records.". No `Metrics` folder is in VizDAW's support folder.
  2. Turn counting on with Co-Producer itself off. Work in a piano roll with the pencil for a few minutes, then in the mixer. The overview line shows one song and some minutes.
  3. Look at `~/Library/Application Support/VizDAW/Metrics/time.json` in Terminal (`ls -l`): it is readable and writable by you alone, and the folder is yours alone. The file holds the song's name, dates, minutes and VizDAW's own command names, and nothing about your notes or other files.
  4. Leave VizDAW untouched for five minutes without playing: the overview total does not grow.
  5. Undo quickly six times, rename tracks one after another a dozen times, and leave a menu open for ten seconds before choosing something.
  6. Turn Co-Producer on and ask Claude "where does my time go?". Claude tells you the time per category for this song and all songs, and names the undo bursts, the repeated renames and the long menu hunt. It suggests at most three chores it could take over and starts none until you pick one.
  7. Work on for a while without asking. Claude never mentions your time unless you ask again.
  8. Ask "what could you help with this week?": the answer covers only the last seven days.
  9. Turn Co-Producer off and ask again: Claude says Co-Producer is off in Settings and cannot read anything.
  10. Click Delete All Time Records…: it asks first. Cancel keeps everything. Delete removes the `Metrics` folder, and the overview reads "No time records.".
  11. Turn counting off, work for a few minutes, and check that no `Metrics` folder appears.
  12. In a host that offers prompts, the coaching prompt is in the list beside the other recipes.
- [ ] **Step 3: Update the backlog** (`docs/feature-backlog.md`).
  - **The "Co-Producer over MCP" row.** Note 8f's delivery:
    - opt-in time counting, per song and across all songs, in minute blocks from the window and tool in use;
    - the three friction signals;
    - the private local file and Delete All;
    - `time_report`, read only on request, and the coaching recipe and prompt.

    Note that sub-project 8's six plans are built.
  - **New rows:**
    - "A time report the composer can read in Settings, not only through Claude";
    - "Count playing a MIDI keyboard as composing";
    - "Fold time records older than a year into totals";
    - "Name more repeated tasks: undo names that are not registry titles show as 'another edit'";
    - "Undos Co-Producer makes through change are counted in undo bursts";
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
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: the baseline's ignored tests plus one run, and all pass, 8e's `co_producer_creative_over_mcp` included.

  Then `pgrep -lf MacOS/VizDAW` and `pgrep -lf vizdaw-mcp` print nothing. Report against the baseline:
  - registry +2, `COMMAND_NAMES` +1, `PREVIEWABLE` unchanged, error codes unchanged, `RECIPES` +1;
  - README +12, e2e +1, `VERSION` unchanged;
  - no change under `crates/core-host`, `crates/core-engine` or `crates/platform-macos` (`git diff --stat <baseline HEAD> -- crates/core-host crates/core-engine crates/platform-macos` is empty).
- [ ] **Step 5: Commit** `docs(app): Co-Producer time tracking README checks, e2e and backlog (plan 8f Task 6)`.

## Self-Review

1. **Spec coverage (the §12 row 8f).**
   - **Counted per song and across all songs** (§9; owner): Ruling 2's store, keyed by project ID, and Ruling 6's `all` and `songs`.
   - **Broad categories in time blocks, from the window and tool in use, not clicks** (§9; owner): Rulings 3 and 4, and Tasks 2–4. Input is used only as "something happened", never as clicks.
   - **Friction signals** (§9; owner): undo bursts, repeated tasks and long menu hunts, in Ruling 4, with thresholds fixed as §11.11 asks.
   - **Off until turned on in Settings** (§9, §6.4): Ruling 1 and Task 3's `nothingIsCountedWhileOff`. The label is the spec's own "Count where my time goes".
   - **Kept local, in VizDAW's support folder, not the package** (§9): Rulings 2 and 5, and the e2e's file checks. The 0600 file, 0700 folders and Delete All are the brief's privacy rules.
   - **Claude sees it only when asked, as totals, never raw logs** (§9, §4): Ruling 6. There is no resource and no push, the tool's description says so, and reading goes through the app's switch-gated socket. `vizdaw-mcp` never opens the file.
   - **Coaching only on request, never interrupting** (§9, §4): Ruling 7's recipe and Ruling 6's instructions sentence.
   - **Counting works with Co-Producer off; reading needs it on** (§9): Ruling 1, Task 3, and README steps 2 and 9.
   - **No note or MIDI content, and no file name beyond the song's own** (brief): Ruling 2's label rule and Task 2's `only_vizdaws_words_are_kept`.
2. **Steps.** Every test step names its tests and assertions with the plan's values: thresholds, modes, keys, sentences and caps. Code steps give signatures and point to the ruling. The tool description, the Settings caption, the report text and the recipe are written out, because the spec leaves them to the plan (§11.11, §11.12).
3. **Type consistency.** These names are spelled the same in every task:
   - `settings.co_producer_metrics`, `settings.delete_time_records`, `METRICS_SETTING`, `DELETE_TIME_RECORDS`, `Command::CoProducerMetrics`, `METRICS_DAYS_MAX`, `ControlRequest::CoProducerMetrics` and `ControlResult::CoProducerMetrics`;
   - `MetricsRecorder` and its methods `open`, `problem`, `sample`, `note_undo`, `note_step`, `note_menu`, `flush`, `close` and `clear`;
   - `metrics_summary`, `metrics_overview` and `metrics_delete_all`;
   - `MetricsCategoryRow`, `MetricsClockRow`, `MetricsSongRow`, `MetricsSecondsRow`, `MetricsCountRow`, `MetricsTotalsRow`, `MetricsSongTotalsRow`, `MetricsSummaryRow` and `MetricsOverviewRow`;
   - `DocSnapshot.step`;
   - `MetricsRecording`, `CoProducerMetrics`, `MetricsContext`, `MetricsCategory.classify`, `StepEvent`, `StepWatch`, `onStep`, `stepObserver`, `MetricsTicker`, `SupportFolder.makePrivate` and `MetricsProbe`;
   - `tools::{TIME_REPORT, time_answer, duration_text}` and the recipe `coaching`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 1–5 and the e2e.
5. **Proportion.** Six tasks, as the owner asked for 26.0.0 by 2026-11-20. Code appears only as signatures and the exact text the spec leaves to the plan. The plan reuses:
   - 8a's support folder rule, socket gate, refused class, window rules, `CoProducerCommands` and Settings tab;
   - 8e's recipes table and prompts;
   - the existing window tags and the history's step serials.
6. **Owner decisions.** None are open. These calls are stated for the owner's review:
   - a fifth category, "other", for Settings and the like (Ruling 3);
   - minute blocks, which go to the majority category, with a 90-second idle cutoff (Rulings 3 and 4);
   - the thresholds: 5 undos in a minute, the same task 6 times in 10 minutes, and a menu open 8 seconds (Ruling 4);
   - time on a never-saved song stays under "Untitled songs" (Ruling 2);
   - a partial block is dropped at switch-off and quit (Ruling 1);
   - labels are kept only when they are registry titles (Ruling 2).
