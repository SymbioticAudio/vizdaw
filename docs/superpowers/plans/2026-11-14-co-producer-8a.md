# Co-Producer Safety and the Server Implementation Plan (8a)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Claude can drive a running VizDAW through `vizdaw-mcp`, a local stdio MCP server, over a second, per-user Co-Producer socket that one Settings switch turns on and off. Every request is one named undo step, with a saved copy of the song made first. Adds run freely. A change or removal runs only after a preview with a one-time token and a yes, asked through MCP elicitation where the host supports it. No Co-Producer path ever replaces a file.

**Architecture:**
- **`core-control`.**
  - Every `CommandSpec` gains a `class: Class` field (read, add, change, remove, outward, refused).
  - The new module `co_producer` holds the allow-list gate, the never-replace path check and the Co-Producer's filtered `commands.list`.
  - `Config` gains a `role`. A `Role::CoProducer` server runs every call through the gate before the call reaches the app.
  - Five new protocol commands, `co_producer.begin`, `co_producer.end`, `co_producer.preview`, `co_producer.apply` and `co_producer.status`, are accepted only on that socket.
- **`core-model`.**
  - `History` gains Co-Producer requests. A request reuses the existing group machinery with a group kind. A hand action closes the request's group, and a later Co-Producer step opens "… (continued)".
  - `file` gains saved copies: writing, listing and loading them. `prune_states` keeps every state blob a copy uses.
  - A new `diff` module describes what changed between two documents, as plain items.
- **`core-ffi`.**
  - `Session` gains the request API (`co_producer_begin`, `co_producer_end`, `co_producer_enter`, `co_producer_leave`, `co_producer_idle`, `co_producer_request`) and the saved copy written at the first entry.
  - It also gains preview, token and take: `co_producer_preview`, `co_producer_take`.
  - Further additions: `Session::open_copy`; `retention` keeps media that copies use; `ControlServer::start_co_producer`.
- **`vizdaw-mcp`.** A new workspace binary that speaks hand-written JSON-RPC over stdio. It serves both MCP eras: 2026-07-28 and the `initialize` era, 2025-11-25, 2025-06-18 and 2025-03-26.
  - Its tools are built from the Co-Producer `commands.list`, and its resources from `state.get` targets.
  - It adds its own tools: `begin_request`, `end_request`, `change`, `apply_change` and `screenshot`.
  - Elicitation asks the composer directly: as an `elicitation/create` request in the `initialize` era, and as an `input_required` result in 2026-07-28.
- **Swift.**
  - `CoProducerSettings` and a new Settings › Co-Producer tab hold the switch, which is off by default.
  - `ControlChannel` pumps a second server. `CoProducerCommands` runs Co-Producer calls between `enter` and `leave`, and runs `co_producer.*`.
  - Two other additions: the idle timer, and File › Open Co-Producer Copy….
  - `make-app.sh` bundles `vizdaw-mcp`.

**Tech Stack:**
- Rust 2024: `core-control`, `core-model`, `core-ffi` with UniFFI 0.32.2, `vizctl`, and the new `vizdaw-mcp`.
- Swift 6.1 / SwiftPM with Swift Testing, macOS 14+.
- **No new third-party dependency.** `vizdaw-mcp` depends only on `core-control` and `serde_json`, which are already in the lock file. It is GPL-3.0-only, like the workspace.

**Spec:** `docs/superpowers/specs/2026-11-14-co-producer-design.md`, approved by the owner on 2026-11-14, is binding. This plan builds its §12 row **8a**. Executors also read the control-channel spec, `docs/superpowers/specs/2026-10-18-control-channel-design.md` (§3, §5, §7, §8, §12).

**Not in this plan** (each is a later plan's, per spec §12):
- `Notes.md`, the Co-Producer-added marks and badge, Make Mine, the project format change, dated export folders, and moving an untitled project's copies into its package on first save: **8b**.
- The MCP Apps views, change preview included. 8a returns the same list as text, which is the spec §5 fallback: **8c**.
- Previews of change and remove commands other than Ruling 6's five: **8c**, **8d** and **8e**, as they add their commands.
- **Compact's offer to remove old copies** (spec §6.7, "Compact can ask to delete producer edits"). This is a plan ruling: 8a keeps copies complete, which is the safety half, and 8b, the librarian, adds the offer and its sheet. Until then copies only grow. Settings shows their size (Ruling 10).

## Plan Rulings

These are this plan's decisions where the spec leaves a choice (spec §11). Each one names its cost if wrong.

1. **No MCP crate: JSON-RPC over stdio by hand (spec §11.1).**
   - The official `rmcp` (3.10.0, Apache-2.0 with older MIT contributions) needs Rust 1.88, while the workspace's `rust-version` is 1.86. It also pulls in tokio, schemars and about forty crates. `rust-mcp-sdk` (MIT) is async too.
   - The server needs about ten methods, so it is written by hand on `serde_json`. It borrows no code from any other MCP server.
   - **Cost if wrong:** a new MCP revision is our work to follow. The protocol code stays in one file (`vizdaw-mcp/src/rpc.rs`) to keep that small.
2. **Both MCP eras (spec §11.2).** As of 2026-11, revision 2026-07-28 has no `initialize`: each request carries `_meta["io.modelcontextprotocol/protocolVersion"]` and `clientCapabilities`, and the server must answer `server/discover`. Hosts are mid-switch, so `vizdaw-mcp` serves both:
   - **`initialize` era.** It answers the client's version if that is `2025-11-25`, `2025-06-18` or `2025-03-26`, and `2025-11-25` otherwise. Capabilities are `{tools:{}, resources:{}}`. The client's `capabilities.elicitation` is kept for the process.
   - **2026-07-28.**
     - `server/discover` answers `supportedVersions: ["2026-07-28","2025-11-25","2025-06-18","2025-03-26"]`, with the same capabilities.
     - A request whose `_meta` version is unsupported gets `-32022` with `data: {supported, requested}`.
     - Results carry `resultType: "complete"`, and list and read results also carry `ttlMs: 0` and `cacheScope: "private"`.
   - **Either era.**
     - `ping` answers `{}`.
     - A JSON array, which is a batch, is `-32600`.
     - Request ids are kept as given, string or number.
     - Logs go to stderr only.
     - It exits at EOF on stdin.
3. **Requests in the history (spec §11.4).**
   - The request is a `History` concept, so every path that changes the document sees it: `execute`, `begin`/`commit`, `undo`, `redo`, `amend`, `begin_group`.
   - **The group.** `Group` gains `kind: GroupKind { Record, Request }`. A request's group opens lazily, at its first *acting* step, which is a step made while `History::acting()` is true.
   - **Hand actions.** Any hand action while a request group is open first ends that group as one step. A hand action is one of those calls made with `acting` false.
   - **Labels.** The step is "Co-Producer: <Title>". If an earlier group of the same request already made a step, it is "Co-Producer: <Title> (continued)".
   - **Undo and redo** always close an open request group first, whoever calls them. So a Co-Producer Undo acts on finished steps, as the menu does.
   - **Busy.** `set_acting(true)` is refused (Busy) while a hand transaction (a drag) or a recording group is open.
   - **Idle close.** A request with no Co-Producer call for **5 minutes** closes. The app checks every 5 s.
   - **Requests and windows.** A request needs no connection: `vizdaw-mcp` may reconnect. There is one request per document.
   - **Cost if wrong:** an idle composer-yes that takes over 5 minutes splits a request into "… (continued)". Nothing is lost.
4. **The label (spec §6.2).** `core_model::history::request_label(name) -> Result<(String, String)>` gives the title and the label.
   - **Refusals.** A name that is empty after trimming is refused. So is one with a C0 or C1 control character, or a bidi control (U+202A–U+202E, U+2066–U+2069).
   - **Spaces.** Runs of whitespace become one space.
   - **Title case.** Each word's first character is upper case and the rest is kept. `a an and as at but by for in of on or the to with` stay lower case unless first: "set up a drum bus" gives "Set Up a Drum Bus".
   - **Length.** A title over 27 characters is cut to 26 plus "…", so the label is at most 40 characters. "(continued)" may take it past 40.
   - **Banned names.** The banned-name list cannot be checked at run time without writing it into shipped code, which the project forbids. Instead, the fixed text Co-Producer shows is covered by the name scans: Task 5 adds `vizdaw-mcp/src` to `names.rs`. `begin_request`'s description tells Claude to name requests in plain words, with no product or company names.
   - **Cost if wrong:** a request Claude names after a product shows that name in one undo label.
5. **Classes (spec §6.5, §11.6).** `class` is a plain field, so the compiler makes every entry have one. `co_producer::effective_class(spec, args)` upgrades `mixer.new_bus` with `strip` to `Change`, because it adds a send on the composer's strip. The table:
   - **Read:**
     - `transport.play_stop`, `play`, `stop`, `play_from_edit_in`, `play_from_selection`, `play_selection`, `rewind`, `fast_forward`, `return_to_start`, `go_edit_in`, `go_position`, `go_punch`, `next_marker`, `previous_marker`, `loop` and `scrub`;
     - `locator.go_0`–`go_9`;
     - `file.interchange_report`.
   - **Add:** `track.new_audio`, `track.new_instrument`, `track.new_midi`, `track.duplicate`, `media.import`, `file.import_midi`, and `mixer.new_bus` without `strip`.
   - **Outward:** `file.bounce`, `file.export_midi`, `file.export_midi_clip`, `file.export_dawproject`, `file.export_aaf` and `file.export_for_daw`.
   - **Change:**
     - `track.rename`, `edit.undo`, `edit.redo`, `file.save`, `file.project_settings`;
     - every `modify.*`;
     - `region.normalize`, `region.fade_in_to_cursor`, `region.fade_out_from_cursor`, `region.crossfade`;
     - `lane.set_input`, `lane.set_monitor`, `lane.set_input_fx`, `lane.set_mpe`, `lane.track_synth`;
     - `destination.set_midi_mix`, `destination.set_mpe`, `destination.set_return_latency`, `destination.add_midi`;
     - `automation.set_mode`, every `automation.mode.*`, `automation.gesture`, `automation.suspend`;
     - `drum.add_row`, `drum.split_row`, `sequence.make_reference`, `media.rename`, `media.relink`, `media.convert`;
     - `editors.insert_event` and the four `editors.nudge_*`;
     - `locator.set_*` and `locator.clear_*`.
   - **Remove:** `track.delete`, `region.remove_fades`, `region.remove_crossfade`, `drum.remove_row`.
   - **Refused:** everything else. That covers:
     - spec §6.5's list: `media.compact`, `region.reload_external`, `keys.save`/`load`/`reset`, `transport.record`, every `record.*`, `file.close`, `file.save_as`, `plugins.install_multiq`, `plugins.new_channel_eq`, `audio.set`, `ui.theme` and every layout `ui.*`;
     - commands that open windows or change the UI: `file.new`, `file.open`, the demos, `window.*`, `ui.view_*`, `ui.toggle_*`, `ui.pane`, the editor-opening `editors.*`, `editors.snap`, `editors.close_detail`, `sequence.back`, `drum.show_velocity`, `drum.timeline`, `region.open_external`, every `step.*`;
     - recording set-up: `transport.overdub`, `transport.punch`, `transport.arm_next`, `transport.arm_previous`;
     - live input and devices: `midi.inject`, `destination.measure_return`, every `surface.*`;
     - app-wide settings: `plugins.default_eq`;
     - imports that replace or open a project: `file.import_dawproject`, `file.import_aaf`;
     - this plan's new `settings.co_producer` and `file.open_copy`.
   - **Cost if wrong:** a refused command an owner wants is one line to reclass, plus its test row.
6. **What 8a previews (spec §6.6, §11.5).** Only these change and remove commands are previewable in 8a: `track.rename`, `track.delete`, `edit.undo`, `edit.redo` and `file.save`.
   - **Everything else.** Any other change or remove command, even inside a plan, is `not_allowed`: "Co-Producer cannot preview <id> yet." Later plans lift this command by command.
   - **Explicit targets.** `track.rename` and `track.delete` must name `seq` and `lane` in a plan, so a preview never depends on the selection.
   - **How the preview is worked out.** It runs on a clone of the session's `History`: the core-model commands that `Session::edit` would build, `peek_undo`/`peek_redo` after closing an open request group, or no document change for Save. `core_model::diff::describe(before, after)` then lists the changes. Nothing reaches the host, the engine, the disk or the UI.
   - **Direct calls.** A `command.run` of a change or remove command on the Co-Producer socket is `not_allowed`: "<id> changes your work: preview it with co_producer.preview and apply it with the composer's yes." This applies even when the command is previewable.
7. **The token (spec §6.6, §11.5).**
   - **Form.** 32 lowercase hex characters, from the same random source `project_id::fresh` uses.
   - **What it is tied to.** `Inner.version`, `History::present_serial()` and the plan's canonical text, which is each command's id and its args in registry order, as JSON.
   - **One at a time.** A session keeps only its latest preview. A new preview replaces it.
   - **Take.** `co_producer_take(token, plan)` succeeds once. It fails with `Failed` in these cases:
     - "This preview was already used or replaced." when the token is unknown, used or replaced;
     - "The song changed since the preview; preview again." when the version or serial moved;
     - "These are not the previewed commands." when the plan differs.
   - **No yes needed.** A preview whose items are all Co-Producer's own has `needs_yes: false`. In 8a that is only an Undo or Redo of a "Co-Producer: …" step. Such a preview is applied at once with no question asked.
8. **Who says yes (spec §6.6, §11.3).** As of 2026-11, form elicitation works in Claude Code and several editor hosts. Claude's desktop app does not support it yet (feature request open). `vizdaw-mcp`'s `change` tool:
   1. previews;
   2. if `needs_yes` is false, applies at once;
   3. else, if the client declared `elicitation` (form), asks the composer and applies on an accept with `allow: true`. The token never reaches Claude;
   4. else returns the list and the token, and Claude must show the list and wait for the composer's yes before calling `apply_change`.

   `apply_change` refuses a token handed out through elicitation, because `vizdaw-mcp` never returns one.
   - **The elicitation:**
     - `message`: "Co-Producer wants to change your work:\n" plus one "- <thing>: <change>" line per item (at most 40 lines, then "- and N more"), then "\nAllow these changes?";
     - `requestedSchema`: `{type:"object", properties:{allow:{type:"boolean", title:"Allow these changes", default:false}}, required:["allow"]}`.
   - **A no.** Decline, cancel, or `allow: false` returns the text "The composer said no. Nothing changed." with `isError: false`.
9. **Saved copies (spec §6.7, §11.7).**
   - **Where.** A copy is a package `Co-Producer/Copies/<stamp> before <Title>.vizdaw/` holding only `project.json`. The stamp is "YYYY-MM-DD HH-MM" in local time, which Swift passes. Media and state blobs stay the parent's.
   - **The name.** `/` and `:` in the title become `-`, and a leading `.` is dropped. A taken name gets " 2", " 3", and so on. A copy is never replaced.
   - **When.** `co_producer_enter` writes it at the first entry of each request: one copy per request, none for "(continued)". It first writes the present document's unsaved state blobs into the parent's `States/`, as `autosave` does.
   - **A failed copy** refuses the command: "The saved copy could not be written (<reason>), so Co-Producer changed nothing."
   - **An untitled document.** The copy goes in `<support>/Co-Producer/Untitled/<project id>/Copies/`, with its blobs in that folder's `States/`. A project ID is stamped first if the document has none. Moving these copies into the package is 8b's. Until then such a copy needs the untitled project's media, and 8b's backlog row says so.
   - **Kept complete.**
     - `retention::on_disk` reads every copy's `project.json` alongside the autosaves.
     - `prune_states` keeps every blob a copy names.
     - A copy that cannot be read or parsed stops all trashing and pruning in that pass. Retention then adds a problem line: "Unused media files were not moved to the Trash: a Co-Producer copy could not be read."
   - **Opening a copy.** `Session::open_copy(path)` makes a new untitled session. Its scratch root receives a copy (`std::fs::copy`, which clones on APFS) of every media file and blob the copy names. Its title is the copy's name, so Save As makes it a project of its own.
     - **The menu item.** File › Open Co-Producer Copy… opens a panel in the front project's `Co-Producer/Copies`. It is enabled when the front project is saved and has copies.
     - **Other routes.** `DocumentStore.open(path:)` also routes a copy path there, for example from Finder.
     - **The registry entry** is `file.open_copy`.
   - **Cost if wrong:** opening a large copy duplicates its media, but on APFS the clones take no space until written.
10. **The switch and the socket (spec §6.1, §6.4).**
    - **The setting.** `UserDefaults` key `VizDAW.coProducer` (Bool), absent meaning off.
    - **Turning it on:**
      1. makes `<support>` mode 0700 when this user owns it, so the socket's directory passes control spec §8. If this user does not own it, Co-Producer does not start, and Settings shows why;
      2. removes `<support>/co-producer.off`;
      3. starts `ControlServer.startCoProducer(path: "<support>/co-producer.sock")`.
    - **Turning it off, quitting, or a failed start:**
      1. stops the server, so queued calls get `shutting_down` and connections drop;
      2. ends every open request as one step;
      3. writes the empty file `<support>/co-producer.off`, except at quit.
    - **What `vizdaw-mcp` says when it cannot connect:**
      - "Co-Producer is turned off in VizDAW's Settings." when `co-producer.off` exists;
      - else "VizDAW is not running, or Co-Producer is off in its Settings.".
    - **Its folder.** `vizdaw-mcp` finds `<support>` as the app does: `VIZDAW_SUPPORT_DIR`, else `$HOME/Library/Application Support/VizDAW`.
    - **Security.** The socket is 0600 in a 0700 folder this user owns. The existing `prepare` and `verify_dir` checks apply unchanged. `core-control` forbids unsafe code, so the peer's uid is not read: the folder's mode is what keeps other users out.
    - **For the e2e.** A new registry command, `settings.co_producer {on: Flag, required}`, flips the switch over the developer channel. It is `key_only`, menu "Settings", `document: false`, and class refused.
    - **Shown in Settings.** The tab shows the count and total size of the saved copies of the open saved projects.
11. **The protocol additions (control-channel spec §5).**
    - **New commands.** `COMMAND_NAMES` grows from 13 to 18, and the new `co_producer.*` commands are refused on the developer socket with `not_allowed` "only on the Co-Producer socket".
    - **A new error code.** `ErrorCode::NotAllowed` (`not_allowed`) brings `ErrorCode::ALL` to 16. `ControlErrorCode` and Swift gain it too.
    - **On the Co-Producer socket:**
      - `ui.tree`, `ui.click`, `ui.drag`, `ui.key`, `app.new_demo`, `app.open` and `app.quit` are `not_allowed`;
      - `ui.screenshot` takes only `window` and `scale` (at most 1.0), with no `path` and no `live`, at most one per second (`busy` "one screenshot a second");
      - `state.get input` is `not_allowed`, because Co-Producer does not listen;
      - `commands.list` answers only the non-refused commands, each with its `class` and a `previewable` flag.
    - **The new commands' params:**
      - `co_producer.begin`: `name` (Text, 1–200 characters after trimming), `window?`;
      - `co_producer.end`: `window?`;
      - `co_producer.status`: `window?`;
      - `co_producer.preview`: `commands` (an array of 1–32 `{id, params?}`), `window?`;
      - `co_producer.apply`: `token` (32 lowercase hex characters), `commands`, `window?`.

      Each planned command is checked by `registry::validate_args`, Ruling 6's previewable set and Ruling 6's explicit targets.
    - **Paths never replace (spec §6.7).** For outward commands, `co_producer::check_output(path)` refuses:
      - a relative path, or one with a `..` component;
      - a path that exists, by `symlink_metadata`, whatever it is, link included;
      - a parent that is not an existing directory, or whose final component is a link;
      - a path inside any `.vizdaw` package.

      Each is `not_allowed` and names the path and the reason. It checks every Path and File param of an outward command (`path` today). `media.import` and `file.import_midi` read their path and are not checked.
12. **`vizdaw-mcp`'s tools and resources (spec §6.1, §11.12).**
    - **Tool names.** A tool is named by its registry id with `.` turned into `_` (`track_new_audio`). A test checks that the names are unique and at most 64 characters.
    - **One tool per read, add or outward command** from the Co-Producer `commands.list`. Its `inputSchema` comes from the params:
      - Text: `string` with `maxLength` 65 536;
      - Path and File: `string`, with the help plus "An absolute path to a new file.";
      - Flag: `boolean`;
      - Choice: `enum`;
      - Int: `integer` with min and max;
      - IntOrNull: `["integer","null"]`;
      - ChoiceOrPrefixed: `string`.

      Every tool also takes `window` (integer, optional) and has `additionalProperties: false`.
    - **Annotations:**
      - read: `readOnlyHint: true`;
      - add and outward: `destructiveHint: false`, `openWorldHint: false`;
      - `change` and `apply_change`: `destructiveHint: true`.
    - **The descriptions** are the command's title, its menu, then one class line:
      - add: "Adds something new; needs an open request (begin_request).";
      - outward: "Writes new files only; refuses a path that exists.";
      - read: "Reads or plays; changes nothing.".
    - **The own tools, with their exact descriptions:**
      - `begin_request {name}`: "Start one thing the composer asked for, before any command that adds or changes anything. Name it in a few plain words, with no product or company names, as the composer would say it ("set up a drum bus"). Everything until end_request becomes one undo step, "Co-Producer: <Name>". Only one request is open per song."
      - `end_request {}`: "Finish the open request. Its steps become one undo step. Call it as soon as the request is done."
      - `change {commands}`: "Change or delete something that already exists, such as renaming or deleting a track, Undo, Redo, or Save. VizDAW first works out exactly what will change. If the composer must agree, VizDAW asks them itself when it can. Otherwise this returns the list and a token: show the composer the whole list, word for word, and call apply_change only after they say yes in this chat. Never call apply_change on your own judgement."
      - `apply_change {token, commands}`: "Apply a change the composer has agreed to in this chat, with the token and the same commands change returned. Fails if the song changed since, or the token was used."
      - `screenshot {window?}`: "A picture of a VizDAW window, for reading what is on screen. It cannot click or type." It returns MCP image content: base64 PNG, read from the path the app answered, which `vizdaw-mcp` then deletes.
    - **Resources:**
      - `vizdaw://state/<target>`, `mimeType` `application/json`, for every `state.get` target but `input`;
      - `vizdaw://commands`, the Co-Producer `commands.list`.
    - **The server's instructions:** "VizDAW's Co-Producer. Adds are free; anything that changes or deletes the composer's own work goes through the change tool and their yes. Start every request with begin_request and finish it with end_request."
13. **The app runs Co-Producer calls as menus do.** `CoProducerCommands.run` resolves the document and sets `acting` around the work:
    - `runCommand` runs `ControlCommands.run`, between `session.coProducerEnter(stamp:)` and `session.coProducerLeave()`. It enters for every class but read.
    - `co_producer.apply` runs `coProducerTake`, then each planned command in turn between enter and leave.
      - A command that fails stops the rest. The reply's `results` has one line per planned command: "<id>: done", "<id>: <failure message>", or "<id>: not run (an earlier command failed)". So every command that did not run has its line.
    - Alerts a call raises are taken as its failure, as `ControlCommands.run` already does. No sheet opens.
    - `ControlChannel.active` stays the developer channel's alone, so with only Co-Producer on, the composer's own sheets and alerts show as usual.
    - **Cost if wrong:** a job that ends later (a bounce placed on new tracks) lands without `acting` and so closes the request's group as a hand step. That is 8c's row.

## Global Constraints

Every task's requirements include these.

- **Shell.** Every command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` and `cd /Users/lattawb/workspace/vizdaw`.
- **Swift.**
  - Build only through `scripts/build-ffi.sh` (after any FFI change) and `scripts/swift.sh build|test`, plus `scripts/make-app.sh` in Tasks 7 and 8. Never run `swift`, `xcodebuild` or `xcrun` directly.
  - Swift 6.1:
    - no `[weak self]`, and no non-Sendable value captured inside `MainActor.assumeIsolated`;
    - no `@State` or `@FocusState`: view state lives in the `@Observable` models.
  - Every new control has an accessibility label, and colours, fonts and spacing come from theme tokens (`theme.text…`, `theme.type…`, `theme.spacing…`).
- **Tests never touch the machine.**
  - They never open real audio or MIDI, and never trigger the microphone or TCC prompt.
  - They never touch the real `~/Library`: Application Support/VizDAW, preferences, the Trash, `Audio/Plug-Ins`. Use temp folders, `MemoryDefaults()`, the e2e's throwaway home, and fakes.
  - Socket tests use a private temp folder under `/tmp`, as `server.rs`'s `private_tmp` does. `vizdaw-mcp` tests set `VIZDAW_SUPPORT_DIR` on the child they spawn, never in their own process.
  - Every `core-ffi` test file calls `force_offline()` through its `serial` helper.
  - Swift tests that make a real `Session` set `TestSupportDir.shared`.
  - Tests that load real plug-ins run in a child process whose `HOME` and `CFFIXED_USER_HOME` are a temp folder, through `crates/core-host/tests/support/temp_home.rs`. This plan adds none.
  - Linux CI has no AU or CoreFoundation, so any test that needs either is `#[cfg(target_os = "macos")]`. `vizdaw-mcp` and the `core-control` socket tests are plain Unix and run on Linux.
  - The e2e uses vizctl's throwaway home.
- **Tools.**
  - Never run `auval`.
  - No GUI, computer-use or accessibility-automation tools. Launch VizDAW only through the e2e with `VIZDAW_AUDIO=offline`. Afterwards `pgrep -lf MacOS/VizDAW` prints nothing.
  - Never use `dangerouslyDisableSandbox`.
  - Never read `~/workspace/SVP/research`.
- **Programmatic calls open nothing.** Control-channel and Co-Producer calls, and programmatic calls, never open dialogs, sheets, alerts, panels, menus or windows.
- **Hostile input.**
  - Every MCP message and every Co-Producer call is hostile.
  - Every new FFI export runs inside `ffi_guard` (and so `catch_unwind`). It returns `Result<_, VizError>`, or a safe default for the never-failing ones.
  - `vizdaw-mcp` handles each message inside `std::panic::catch_unwind`. A panic answers `-32603` and the server goes on.
  - Each new command gets hostile-input tests: wrong types, unknown params, missing required params, a 70 000-character string, a NUL, IDs out of range.
  - Every planned command that does not run gets a line in the reply (Ruling 13).
- **Counts.**
  - The registry goes from 181 to **183** (`settings.co_producer`, `file.open_copy`).
  - `COMMAND_NAMES` goes from 13 to **18**, and `ErrorCode::ALL` from 15 to **16**.
- **Names.**
  - The repo and UI never name the vendors and DAWs on the project's banned list, which now includes the feature's old working name (spec, "Name"). That list lives only in the name scans (`app/Tests/VizDAWAppModelTests/NameScanTests.swift`, `crates/core-control/tests/names.rs`); read them, keep all new text clear of it, and never copy the list.
  - The feature is "Co-Producer" everywhere.
  - Text the app shows says "track", never "lane". Identifiers and JSON keys keep `lane`.
- **Commits.**
  - Stay on branch `sp6-import`. Never switch branches, never push.
  - Commit at the end of every task by explicit paths, never `git add -A`. Never commit `.superpowers/`, `.DS_Store`, `target/`, generated FFI (`app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`) or `dist/`.
  - Every message ends with exactly these two lines:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01F7df6grqTdqHgc2d2X359P
    ```
  - Check each commit with `git log -1 --oneline && git show --stat HEAD`.
  - `Cargo.lock` changes only by the new `vizdaw-mcp` package entry, in Task 5.
- **Brand.** Brand assets are never in the repo. `VIZDAW_NO_BRAND=1` builds without them.
- **Lints.**
  - Every crate keeps `#![deny(missing_docs)]` and the `clippy::{unwrap_used, expect_used, indexing_slicing, panic}` denials outside tests. `vizdaw-mcp` gets both, and `#![forbid(unsafe_code)]`.
  - `core-model` stays free of OS APIs, apart from the `std::fs` that `file.rs` already uses.
- **Format.** `core_model::file::VERSION` stays 9. Copies are separate files, and `project.json` does not change.
- **TDD.** Every new or changed test is run and seen to fail before the code that makes it pass.
- **Each task ends green.**
  - Always:
    - `cargo fmt --all -- --check`;
    - `cargo clippy --workspace --all-targets -- -D warnings`;
    - the Linux clippy: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu --target-dir target/linux-check -- -D warnings`;
    - `cargo test` for the crates the task touches.
  - Tasks 3, 4 and 7 also run `cargo clippy -p core-ffi --lib -- -D warnings`, `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`.
  - Known flaky: the `damaged_files` and `dawproject_fuzz` proptests, `platform-macos/tests/midi.rs`, `core-ffi` catalog tests under load, and the latency e2e timing. Re-run once before investigating, and say so in the report.
  - **Baseline:** measure at the start of Task 1: HEAD `08034d4`, registry 181, `COMMAND_NAMES` 13, `ErrorCode::ALL` 15, README through step 325, e2e 31 ignored, `VERSION` 9. Each task reports the counts it adds.

## Review Focus

These are the five failure modes a composer is most likely to hit, most likely first. Each is pinned by named tests in its task.

1. **The composer works by hand while a request is open.**
   - Examples:
     - a drag in progress when Claude's command arrives;
     - Edit › Undo from the menu mid-request;
     - starting to record;
     - renaming a track while Claude adds buses.
   - Expected:
     - a Co-Producer command during a drag or a recording is `busy`, and changes nothing;
     - the hand edit is its own undo step;
     - the menu Undo takes back the Co-Producer steps so far as one;
     - Claude's next step lands in "Co-Producer: <Title> (continued)";
     - no Undo ever takes the composer's edit with Co-Producer's.
   - Pinned by Task 2's `a_hand_edit_closes_the_request_and_the_next_step_continues` and `acting_is_busy_during_a_drag_or_a_recording`, and Task 4's `enter_is_busy_during_a_hand_edit`.
2. **A yes is reused or goes stale.**
   - Examples:
     - apply after the composer edited the song;
     - apply twice;
     - apply with one command changed or reordered;
     - a token from another window's preview;
     - a second preview, then the first token.
   - Expected: each is refused with Ruling 7's sentence, and the document and its undo history are unchanged.
   - Pinned by Task 4's `take_refuses_every_stale_or_altered_yes`.
3. **An outward path would replace something.**
   - Examples:
     - an existing file;
     - a link to a file;
     - a dangling link;
     - a folder;
     - `..` in the path;
     - a relative path;
     - a path inside the open package;
     - a parent folder that is a link.
   - Expected: `not_allowed` with the path and the reason, before the app sees the call, and nothing is written.
   - Pinned by Task 1's `outward_paths_never_replace`.
4. **The switch goes off mid-work.**
   - Examples:
     - a request open with steps made;
     - a call queued behind a slow one;
     - `vizdaw-mcp` connected and idle;
     - the switch turned on again at once.
   - Expected:
     - queued calls answer `shutting_down`;
     - the open request ends as one step;
     - the next MCP tool call says "Co-Producer is turned off in VizDAW's Settings.";
     - turning it back on listens again on the same path.
   - Pinned by Task 7's `turningOffStopsAndClosesRequests` and Task 8's e2e.
5. **A save or Compact Media takes what a copy needs, or a copy is damaged.**
   - Examples:
     - Compact Media after a request deleted the only track using a file;
     - Save after Undo;
     - a plug-in's state changed after the copy;
     - a junk or truncated `project.json` in `Co-Producer/Copies`.
   - Expected:
     - every copy opens complete;
     - a damaged copy stops that pass's trashing and pruning, with Ruling 9's problem line;
     - nothing panics.
   - Pinned by Task 3's `retention_and_prune_keep_what_copies_use` and `a_damaged_copy_stops_trashing`.

---

### Task 1: Command classes, the Co-Producer gate and the never-replace check (`core-control`)

**Files:**
- Modify:
  - `crates/core-control/src/registry.rs` (the `class` field, `settings.co_producer`, `file.open_copy`, `class` in `list_json`);
  - `crates/core-control/src/request.rs` (the five commands);
  - `crates/core-control/src/error.rs` (`NotAllowed`);
  - `crates/core-control/src/server.rs` (`Role`, the gate in `handle`, the screenshot rate);
  - `crates/core-control/src/lib.rs`;
  - `crates/core-control/tests/commands.rs`, `tests/protocol.rs`, `tests/server.rs`.
- Create: `crates/core-control/src/co_producer.rs`, `crates/core-control/tests/co_producer.rs`.

**Interfaces:**
- Produces:
  ```rust
  // registry.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub enum Class { Read, Add, Change, Remove, Outward, Refused }
  impl Class { pub fn name(self) -> &'static str }   // "read", "add", ...
  // CommandSpec gains `pub class: Class`; `cmd()` defaults to Refused, so every
  // entry outside Ruling 5's lists stays refused without an edit.
  // co_producer.rs
  pub const PREVIEWABLE: &[&str] = &["track.rename", "track.delete", "edit.undo", "edit.redo", "file.save"];
  pub const MAX_PLAN: usize = 32;
  pub fn effective_class(spec: &CommandSpec, args: &[(String, ArgValue)]) -> Class;
  pub fn admit(c: &Command) -> Result<(), ControlError>;        // Rulings 5, 6, 11
  pub fn check_output(path: &Path) -> Result<(), ControlError>;  // Ruling 11
  pub fn list_json() -> Value;                                   // non-refused, with class, previewable
  /// One planned command of a preview or apply.
  #[derive(Debug, Clone, PartialEq)]
  pub struct Planned { pub id: String, pub args: Vec<(String, ArgValue)> }
  pub fn canonical(plan: &[Planned]) -> String;                  // Ruling 7's canonical text
  // request.rs: Command gains
  CoProducerBegin { name: String, window: Option<WindowRef> },
  CoProducerEnd { window: Option<WindowRef> },
  CoProducerStatus { window: Option<WindowRef> },
  CoProducerPreview { plan: Vec<Planned>, window: Option<WindowRef> },
  CoProducerApply { token: String, plan: Vec<Planned>, window: Option<WindowRef> },
  // server.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum Role { #[default] Developer, CoProducer }
  // Config gains `pub role: Role`.
  ```

- [ ] **Step 1: Write the failing tests** in `tests/co_producer.rs`.
  - `every_command_has_the_spec_class`:
    - each id in Ruling 5's read, add, outward, change and remove lists has that class;
    - each of spec §6.5's refused ids is `Refused`;
    - the registry has 183 entries, and no id outside the lists is anything but `Refused`.
  - `mixer_new_bus_with_a_strip_is_a_change`: `effective_class` gives `Add` for `{}` and `Change` for `{strip: 3}`.
  - `the_gate_admits_only_the_allow_list`. Through `admit`:
    - `commands.list`, `state.get document`, `wait stopped`, `command.run track.new_audio` and `command.run file.export_midi {path: <new file in a temp dir>}` pass;
    - each of these is `not_allowed`:
      - `ui.click`, `ui.drag`, `ui.key`, `ui.tree`, `app.open`, `app.quit`, `app.new_demo`, `state.get input`;
      - `command.run media.compact`, `transport.record`, `keys.save`, `file.save_as`, `settings.co_producer`;
      - `command.run track.delete {seq: 1, lane: 1}`, with Ruling 6's message;
      - `command.run region.normalize`;
    - `ui.screenshot {path: …}` and `{live: true}` are `invalid_params`, and `{scale: 1.5}` is `invalid_params`.
  - `plans_check_every_command`. `co_producer.preview` parses `[{id: "track.rename", params: {seq: 1, lane: 2, name: "Vox"}}]`, and each of these fails:
    - an empty array, and 33 commands: `invalid_params`;
    - `track.rename` without `seq` or `lane`: `invalid_params` "name the track's `seq` and `lane`";
    - `region.normalize`: `not_allowed` "Co-Producer cannot preview region.normalize yet.";
    - `track.new_audio`: `not_allowed`;
    - an unknown id: `unknown_id`;
    - `params` as a string, and a `name` of 70 000 characters: `invalid_params`.
  - `apply_checks_its_token`: a token of 31 or 33 characters, uppercase hex, a NUL, or a number is `invalid_params`.
  - `begin_checks_its_name`: `""`, `"   "`, 201 characters, a NUL and `name: 5` are `invalid_params`.
  - `outward_paths_never_replace`. In a private temp dir, `check_output` refuses each of these with `not_allowed`, naming the path:
    - an existing file;
    - a symlink to it;
    - a dangling symlink;
    - an existing folder;
    - `<dir>/a/../b.mid`;
    - `b.mid` (relative);
    - `<dir>/Song.vizdaw/out.mid`;
    - `<link-to-dir>/new.mid`;
    - `<dir>/missing/new.mid`.

    `<dir>/new.mid` passes.
  - `the_filtered_list_has_classes`: Co-Producer `list_json` has no refused id, each entry has `class`, and only Ruling 6's five have `previewable: true`.
  - `canonical_text_is_order_and_value_exact`: two plans that differ only in command order, or in one arg's value, give different text; the same plan twice gives the same.
- [ ] **Step 2: Add to `tests/server.rs`** `the_co_producer_socket_gates_and_the_developer_socket_refuses_co_producer_calls`:
  - a `Role::CoProducer` server answers `ui.click` with `not_allowed`, and the app never sees it (`next_call` is `None`);
  - `commands.list` there has no `ui.theme`;
  - a second `ui.screenshot` within a second is `busy`;
  - a `Role::Developer` server answers `co_producer.begin {name: "x"}` with `not_allowed` "only on the Co-Producer socket".

  In `tests/protocol.rs`, `ErrorCode::ALL` has 16 codes and `not_allowed` round-trips. In `tests/commands.rs`:
  - the count becomes **183**, with "; 8a adds settings.co_producer and file.open_copy" appended to the message;
  - `settings_co_producer_and_open_copy_check_their_params`: `on` missing, `on: 1` and an unknown param are `invalid_params`; `file.open_copy` without `path` is `dialog_required` "Open".
- [ ] **Step 3: Run** `cargo test -p core-control`. Expected: FAIL (no `Class`, no `co_producer`).
- [ ] **Step 4: Implement.**
  - **The field and the table.** Add `class` to `CommandSpec`, defaulting to `Refused` in `cmd()`, and set Ruling 5's classes with `..` struct updates. Add the two registry entries, and `"class"` to the developer `list_json` too.
  - **The gate.** `server::handle` calls `co_producer::admit` after `validate` when `role` is `CoProducer`, and answers `commands.list` with `co_producer::list_json()` there. On `Developer`, any `co_producer.*` is refused.
  - **The screenshot rate** is a `Mutex<Option<Instant>>` in `Shared`.
  - **The path check** runs inside `admit`, which keeps `validate` pure for the developer socket.
  - **Docs.** Update the module docs of `lib.rs` and `server.rs`.
- [ ] **Step 5: Run** `cargo test -p core-control`. Expected: PASS, including `names.rs` and the random-params proptest over 183 entries.
- [ ] **Step 6: End-of-task checks. Commit** `feat(control): Co-Producer command classes, the allow-list gate and the never-replace path check (plan 8a Task 1)`.

### Task 2: Requests as one named undo step (`core-model`)

**Files:**
- Modify: `crates/core-model/src/history.rs`.
- Create: `crates/core-model/tests/co_producer_requests.rs`.

**Interfaces:**
- Produces:
  ```rust
  /// What a request is (8a Ruling 3).
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct RequestInfo {
      pub name: String,       // as given, trimmed
      pub title: String,      // Ruling 4
      pub label: String,      // "Co-Producer: <title>"
      pub group_open: bool,   // a group of it is open now
      pub continued: bool,    // an earlier group of it made a step
  }
  /// Ruling 4: (title, label); `ModelError::Invalid` for a refused name.
  pub fn request_label(name: &str) -> Result<(String, String)>;
  impl History {
      /// Refused while one is open: "Co-Producer request \"<label>\" is still open."
      pub fn begin_request(&mut self, name: &str) -> Result<RequestInfo>;
      /// Ends the request (its open group as one step); the label of that step, if one was made.
      pub fn end_request(&mut self) -> Result<Option<String>>;
      pub fn request(&self) -> Option<RequestInfo>;
      /// Ruling 3; `ModelError::Busy`-style refusal (the crate's existing busy or invalid variant,
      /// message "Finish the drag first." / "Stop recording first.") when turning it on during a
      /// hand transaction or a recording group. Turning it off never fails.
      pub fn set_acting(&mut self, on: bool) -> Result<()>;
      pub fn acting(&self) -> bool;
  }
  ```

- [ ] **Step 1: Write the failing tests** in `co_producer_requests.rs`. Use the fixtures other history tests use, with `AddLane` and `LaneEdit` rename commands as the steps.
  - `labels_are_title_case_and_short`:
    - "set up a drum bus" gives ("Set Up a Drum Bus", "Co-Producer: Set Up a Drum Bus");
    - "  the   kick  " gives "The Kick";
    - a 60-character name gives a 27-character title ending "…" and a label of 40 characters;
    - "", "   ", "a\u{0}b", "a\nb" and "a\u{202E}b" are refused.
  - `a_request_is_one_undo_step`:
    - `begin_request("add two tracks")`, `set_acting(true)`, two `AddLane`s, `set_acting(false)`, then `end_request()`, which returns `Some("Co-Producer: Add Two Tracks")`;
    - `undo_label()` is that label;
    - one `undo` restores the document exactly, and `redo` brings both lanes back.
  - `a_request_that_made_nothing_adds_no_step`: `end_request()` is `None`, and `undo_depth` and the redo stack are unchanged.
  - `a_hand_edit_closes_the_request_and_the_next_step_continues`:
    - acting `AddLane`, then a hand `LaneEdit` rename (acting off), then acting `AddLane`, then `end_request`;
    - the undo labels from the top are "Co-Producer: Add Two Tracks (continued)", the rename's label, and "Co-Producer: Add Two Tracks";
    - undoing the top step leaves the rename in place.
  - `undo_and_redo_close_the_request_group_first`:
    - an acting step, then a hand `undo`: the step is undone whole, and `request().unwrap().group_open` is false;
    - an acting `undo` behaves the same.
  - `recording_closes_the_request_group`: a hand `begin_group()` during an open request group first closes it as one step, then opens the recording group.
  - `acting_is_busy_during_a_drag_or_a_recording`: `set_acting(true)` fails after a hand `begin("Drag")`, and after `begin_group()`. The document is unchanged.
  - `only_one_request_at_a_time`: a second `begin_request` fails with the open label in its message. After `end_request`, a new one opens.
  - `prop_request_then_undo_is_the_original` (proptest, 64 cases):
    - from a fixture document, `begin_request`, then 1–12 random acting steps (add a lane, rename one, delete one, using valid ids picked from the current doc), then `end_request`;
    - one `undo` gives a document equal to the original.
- [ ] **Step 2: Run** `cargo test -p core-model --test co_producer_requests`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 3.
  - **Request state.** `GroupKind` goes on `Group`. The request's own state (name, title, label, continued) is a new `request: Option<Request>` field.
  - **One helper for hand actions.** `fn before_hand_action(&mut self)` ends an open request group. It is called at the top of `execute`, `begin`, `amend` and `begin_group` when `!acting`, and always at the top of `undo` and `redo`.
  - **The lazy group.** An acting `execute` or `begin` with no open group opens one of kind `Request`.
  - **End labels.** `end_group`'s label logic is reused for both kinds: a request group's label is computed at close.
  - **Unchanged.** `referenced_media_anywhere` and `stamp_project_id` already walk `group`, so they need no change.
- [ ] **Step 4: Run** `cargo test -p core-model`. Expected: PASS, including the existing history and recording-group tests.
- [ ] **Step 5: End-of-task checks. Commit** `feat(model): Co-Producer requests as one named undo step (plan 8a Task 2)`.

### Task 3: Saved copies, kept complete, and opening one (`core-model`, `core-ffi`)

**Files:**
- Modify:
  - `crates/core-model/src/file.rs` (copies, `prune_states`);
  - `crates/core-ffi/src/retention.rs` (`on_disk`);
  - `crates/core-ffi/src/session.rs` (`open_copy`, `write_co_producer_copy`).
- Create: `crates/core-model/tests/copies.rs`, `crates/core-ffi/tests/co_producer_copies.rs`.

**Interfaces:**
- Produces (`core-model::file`):
  ```rust
  pub const COPIES_DIR: &str = "Co-Producer/Copies";
  /// Ruling 9's name: "<stamp> before <title>.vizdaw", with " 2", " 3"… when taken.
  /// `stamp` must be "YYYY-MM-DD HH-MM" (digits checked), else Invalid.
  pub fn copy_name(dir: &Path, stamp: &str, title: &str) -> Result<String>;
  /// Writes `doc`'s blobs into `package/States/` and a new `<COPIES_DIR>/<name>/project.json`;
  /// never replaces (the folder is created with `create_dir`, which fails if it exists). Returns the copy's path.
  pub fn write_copy(doc: &Document, package: &Path, stamp: &str, title: &str) -> Result<PathBuf>;
  /// The copies' `project.json` paths, sorted; Err when the folder cannot be read.
  pub fn list_copies(package: &Path) -> Result<Vec<PathBuf>>;
  /// The package a copy belongs to, when `path` is `<pkg>.vizdaw/Co-Producer/Copies/<x>.vizdaw`.
  pub fn copy_parent(path: &Path) -> Option<PathBuf>;
  /// The copy's document with the parent's blobs (like `load_autosave`).
  pub fn load_copy(copy: &Path) -> Result<Document>;
  ```
- Produces (`core-ffi`):
  ```rust
  impl Session {
      /// Ruling 9: a new untitled session from a copy; media and blobs copied into its scratch root.
      #[uniffi::constructor] pub fn open_copy(path: String) -> Result<Arc<Session>, VizError>;
      /// Count and bytes of `package`'s copies (their project.json files), for Settings.
      pub fn co_producer_copies_size(&self) -> Result<CopiesSizeRow, VizError>;
  }
  #[derive(uniffi::Record)] pub struct CopiesSizeRow { pub count: u64, pub bytes: u64 }
  // crate-internal, used by Task 4:
  impl Inner { fn write_co_producer_copy(&mut self, id: u64, stamp: &str, title: &str) -> Result<PathBuf, VizError>; }
  ```

- [ ] **Step 1: Write the failing `core-model` tests** in `copies.rs`, with temp folders.
  - `a_copy_is_new_and_never_replaces`:
    - `write_copy(doc, pkg, "2026-11-14 15-02", "Set Up a Drum Bus")` makes `Co-Producer/Copies/2026-11-14 15-02 before Set Up a Drum Bus.vizdaw/project.json`, and `load_copy` gives a document equal to `doc`;
    - the same call again makes `… Drum Bus 2.vizdaw` and leaves the first file's bytes unchanged.
  - `copy_names_are_safe`:
    - the title "a/b:c" gives "… before a-b-c.vizdaw";
    - ".hidden" gives "… before hidden.vizdaw";
    - each of these stamps is Invalid: "2026-11-14", "2026-11-14 15:02", "x" and "2026-11-14 15-02/..".
  - `copy_parent_finds_the_package`:
    - `/x/Song.vizdaw/Co-Producer/Copies/a.vizdaw` gives `/x/Song.vizdaw`;
    - `/x/Song.vizdaw`, `/x/Copies/a.vizdaw` and `/x/Song.vizdaw/Co-Producer/Copies/a` give `None`.
  - `prune_keeps_blobs_copies_use`:
    - save a doc with a plug-in state blob, then write a copy;
    - capture a new state, so the present no longer names the old blob, and save;
    - the old blob file is still there, and `load_copy` has its bytes.
  - `a_damaged_copy_stops_pruning`: a copy whose `project.json` is "{" leaves every blob in place after a save.
- [ ] **Step 2: Write the failing `core-ffi` tests** in `co_producer_copies.rs`, through a `serial` helper.
  - `retention_and_prune_keep_what_copies_use`:
    - a session with an imported audio file is saved; write a copy through the test hook `debug_write_co_producer_copy("2026-11-14 15-02", "Tidy")`;
    - delete the track and `compact_media`, then `save`. The media file is still in `Media/`, and the trash folder (`set_trash_dir`, a temp folder) is empty;
    - `Session::open_copy(<copy>)` opens untitled, with the track and its region, and `media_file` exists inside the new session's scratch root.
  - `a_damaged_copy_stops_trashing`: plant junk `project.json` in a second copy. The next save trashes nothing, and the snapshot's problems hold Ruling 9's line.
  - `open_copy_refuses_what_is_not_a_copy`: a normal package, a missing path, a 70 000-character path and a path with a NUL each give `Err`, and nothing panics.
  - `copies_size_counts_them`: two copies give `count == 2` and `bytes` equal to the sum of their `project.json` sizes.
- [ ] **Step 3: Run** `cargo test -p core-model --test copies` and `cargo test -p core-ffi --test co_producer_copies`. Expected: FAIL.
- [ ] **Step 4: Implement** the `file` functions and the copy branch in `prune_states`.
  - **Retention.** `retention::on_disk` takes the copies' media too, and returns `None` when a copy fails to read. The `run` caller then uses Ruling 9's line when it was a copy that failed. Keep the autosave line for an autosave.
  - **`open_copy`** uses `std::fs::copy` per file into the new scratch root. A file that fails to copy goes in the snapshot's problems, one line per file: "<path> was not copied (<reason>)."
  - **The test hook** `debug_write_co_producer_copy` goes behind `test-hooks`, and calls `write_co_producer_copy`. For an untitled session that writes under `support_dir()/Co-Producer/Untitled/<project id>/`.
- [ ] **Step 5: Run** the Step 3 commands, then `cargo test -p core-model -p core-ffi`. Expected: PASS.
- [ ] **Step 6: End-of-task checks**, including `scripts/build-ffi.sh`, `scripts/swift.sh build` and `scripts/swift.sh test`. **Commit** `feat(model): Co-Producer saved copies, kept whole by Save and Compact, and opened as a new project (plan 8a Task 3)`.

### Task 4: The request API, preview, token and take (`core-model::diff`, `core-ffi`)

**Files:**
- Modify:
  - `crates/core-model/src/lib.rs`;
  - `crates/core-ffi/src/session.rs`;
  - `crates/core-ffi/src/control.rs` (`ControlRequest` and `ControlResult` variants, `ControlServer::start_co_producer`, the `NotAllowed` code);
  - `crates/core-ffi/src/lib.rs`.
- Create:
  - `crates/core-model/src/diff.rs`, `crates/core-model/tests/diff.rs`;
  - `crates/core-ffi/src/co_producer.rs`, `crates/core-ffi/tests/co_producer.rs`.

**Interfaces:**
- Consumes: Task 1's `Planned`, `canonical`, `Command::CoProducer*`, `Role`; Task 2's request API; Task 3's `write_co_producer_copy`.
- Produces (`core-model::diff`):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct ChangeItem { pub thing: String, pub change: String }
  /// Plain items for what differs (Ruling 6); `[]` when equal.
  pub fn describe(before: &Document, after: &Document) -> Vec<ChangeItem>;
  ```
  The items, by sequence then track order. "Track N" is the 1-based position in its sequence, before the change for removals and after it otherwise:
  - a track removed: "Track 3, Lead Vox" / "deleted" plus ", with R regions and K notes", where any part that is zero is left out ("deleted, with 4 notes");
  - a track added: "Track 4, Bass" / "added";
  - a track renamed: "Track 3, Lead Vox" / "renamed to \"Vox\"";
  - a track's event counts changed: "Track 3, Lead Vox" / "12 regions → 10 regions" or "340 notes → 336 notes";
  - a sequence added, removed or renamed: "Sequence 2, Verse" / the same verbs;
  - a strip added, removed or renamed: "Strip Drums" / the same verbs;
  - when the documents differ but none of the above applies: one "The song" / "other changes".
- Produces (`core-ffi`):
  ```rust
  #[derive(uniffi::Record)] pub struct CoProducerRequestRow { pub name: String, pub label: String, pub group_open: bool, pub continued: bool }
  #[derive(uniffi::Record)] pub struct ChangeItemRow { pub thing: String, pub change: String, pub composers_work: bool }
  #[derive(uniffi::Record)] pub struct PreviewRow { pub token: String, pub needs_yes: bool, pub items: Vec<ChangeItemRow> }
  #[derive(uniffi::Record)] pub struct PlannedRow { pub id: String, pub args: Vec<ControlArg> }
  impl Session {
      pub fn co_producer_begin(&self, name: String) -> Result<CoProducerRequestRow, VizError>;
      pub fn co_producer_end(&self) -> Result<Option<String>, VizError>;          // ends; the label made
      pub fn co_producer_request(&self) -> Option<CoProducerRequestRow>;          // never fails
      /// Before a non-read Co-Producer command: no open request → Failed "Open a request first
      /// (begin_request)."; Busy per Ruling 3; the first entry of a request writes the copy (Ruling 9)
      /// and returns its path; sets acting and the request's last-activity time.
      pub fn co_producer_enter(&self, stamp: String) -> Result<Option<String>, VizError>;
      pub fn co_producer_leave(&self);                                              // never fails
      /// Ends a request idle for `idle_ms` or more (Ruling 3); its label, if a step was made.
      pub fn co_producer_idle(&self, idle_ms: u64) -> Option<String>;               // never fails
      pub fn co_producer_preview(&self, plan: Vec<PlannedRow>) -> Result<PreviewRow, VizError>;
      pub fn co_producer_take(&self, token: String, plan: Vec<PlannedRow>) -> Result<(), VizError>;
  }
  // control.rs
  ControlRequest gains CoProducerBegin { name, window }, CoProducerEnd { window },
      CoProducerStatus { window }, CoProducerPreview { plan: Vec<PlannedRow>, window },
      CoProducerApply { token, plan: Vec<PlannedRow>, window };
  ControlResult gains CoProducerRequest { doc: String, request: Option<CoProducerRequestRow>, undo_label: Option<String> },
      CoProducerPreview { doc: String, preview: PreviewRow },
      CoProducerApplied { doc: String, results: Vec<String>, undo_label: Option<String> };
  // result_json: {request:{name,label,group_open,continued}|null, undo_label},
  //   {token, needs_yes, items:[{thing,change,composers_work}]}, {results:[...], undo_label}
  impl ControlServer { #[uniffi::constructor] pub fn start_co_producer(path: String, app_timeout_ms: u64) -> Result<Arc<ControlServer>, VizError>; }
  ```
  **The preview items for Ruling 6's five:**
  - **rename and delete:** `describe` of a cloned `History` after the same core-model command `Session::edit` builds for the track menu's Rename and Delete;
  - **`edit.undo` and `edit.redo`:** a first item "Undo" (or "Redo") / "\"<label>\"", then `describe(present, peek)`, worked out on a clone whose request group was closed first;
  - **`file.save`:** "<package file name>" / "saved over the file on disk". An untitled document is refused with `Failed` "The song has no file yet: ask the composer to save it (File › Save).".
  - **`composers_work`.** It is false only for every item of an Undo or Redo whose label starts "Co-Producer: ". `needs_yes` is true when any item's `composers_work` is.
  - **A combined plan** works through the clone in order.

- [ ] **Step 1: Write the failing diff tests** in `core-model/tests/diff.rs`: one test per item kind above, with the exact strings, plus:
  - `equal_documents_have_no_items`;
  - `an_unlisted_change_is_other_changes`, using a tempo change.
- [ ] **Step 2: Write the failing session tests** in `core-ffi/tests/co_producer.rs`, through a `serial` helper.
  - `a_request_through_the_session_is_one_step_with_a_copy`:
    - save a session to a temp package, `co_producer_begin("add a track")`;
    - `co_producer_enter("2026-11-14 15-02")` returns the copy's path, which exists;
    - `new_track(Audio)`, then `co_producer_leave()`;
    - a second enter returns `None`, so there is no second copy;
    - after `co_producer_end()`, `undo_label` is "Co-Producer: Add a Track", and one `undo` leaves no track.
  - `enter_needs_a_request`: enter without one is `Err`, with "Open a request first (begin_request).".
  - `enter_is_busy_during_a_hand_edit`: with a hand edit transaction open (the Session's drag-begin call used by the gesture tests), enter is `VizError::Busy`, and no copy is written.
  - `a_failed_copy_changes_nothing`: make `Co-Producer` a file, not a folder. Enter is `Err` with Ruling 9's sentence, and `acting` stays off, so the next hand edit is an ordinary step.
  - `idle_ends_the_request`:
    - `co_producer_idle(0)` after one acting step returns the label;
    - `co_producer_request()` is `None`;
    - `co_producer_idle(300_000)` right after a call returns `None`.
  - `previews_list_exactly_what_changes`:
    - renaming track 2 to "Vox" gives one item, "Track 2, …" / "renamed to \"Vox\"", with `composers_work: true` and `needs_yes: true`;
    - deleting a track with 4 notes says "deleted, with 4 notes";
    - Undo of a "Co-Producer: Add a Track" step gives `needs_yes: false`, while Undo of a hand rename gives `true`;
    - Save on an untitled session is `Err`;
    - after each preview the document, `undo_label`, version and the disk are unchanged, and no host reload happened (`hosts_created` is unchanged).
  - `take_refuses_every_stale_or_altered_yes`. Each of these fails with Ruling 7's sentence:
    - a take after a hand edit;
    - a second take of the same token;
    - a take with the plan's `name` changed;
    - a take with the commands swapped;
    - a take of the first token after a second preview;
    - another session's token.

    Each leaves the document unchanged. A fresh preview, then a take with the same plan, succeeds once.
  - `hostile_request_names_are_refused`: `co_producer_begin` with "", a NUL, "a\u{202E}b" or 70 000 characters is `Err`, and no request opens.
- [ ] **Step 3: Run** `cargo test -p core-model --test diff` and `cargo test -p core-ffi --test co_producer`. Expected: FAIL.
- [ ] **Step 4: Implement**:
  - `diff.rs`;
  - `core-ffi/src/co_producer.rs`, holding the preview builder and the pending preview, `{token, version, serial, canonical}`;
  - the Session methods, each under one lock;
  - the control rows and `request_of` mapping;
  - `start_co_producer`, which is `start` with `Config { role: Role::CoProducer, app_timeout, idle_timeout: 900 s, .. }`.

  `co_producer_end`, `idle` and `take` bump `version` and reload the host only when they change the document. They do not, because they only relabel.
- [ ] **Step 5: Run** the Step 3 commands, then `cargo test -p core-ffi`. Expected: PASS.
- [ ] **Step 6: End-of-task checks**, including the Swift build and test. Swift gains the new `ControlRequest` cases: give them a temporary `.failed(.of(.unavailable, "Co-Producer is not on."))` arm in `ControlChannel.execute`, which Task 7 replaces. **Commit** `feat(ffi): Co-Producer requests, saved copy at the first step, previews and one-time tokens (plan 8a Task 4)`.

### Task 5: `vizdaw-mcp`: the stdio server, tools and resources

**Files:**
- Create:
  - `crates/vizdaw-mcp/Cargo.toml` (`core-control` path dependency, `serde_json = "1"`; dev: `tempfile = "3"`);
  - `crates/vizdaw-mcp/src/main.rs` (stdin and stdout loop, `catch_unwind` per line);
  - `crates/vizdaw-mcp/src/lib.rs`;
  - `crates/vizdaw-mcp/src/rpc.rs` (JSON-RPC framing, eras, errors);
  - `crates/vizdaw-mcp/src/tools.rs` (the registry-to-tool mapping, schemas, the own tools);
  - `crates/vizdaw-mcp/src/app.rs` (the socket client, reconnect once, Ruling 10's messages);
  - `crates/vizdaw-mcp/src/base64.rs`;
  - `crates/vizdaw-mcp/tests/server.rs` and `tests/support/fake_app.rs`.
- Modify:
  - `crates/core-control/tests/names.rs` (scan `vizdaw-mcp/src` too);
  - `Cargo.lock`, which gains the package entry only.

**Interfaces:**
- Consumes: Task 1's Co-Producer `commands.list` JSON (`class`, `previewable`), `client::Client`, `co_producer::MAX_PLAN`.
- Produces:
  ```rust
  // lib.rs
  pub struct Server { /* era, client caps, app connection, pending elicitations (Task 6) */ }
  impl Server {
      pub fn new(support: PathBuf) -> Server;
      /// One input line → zero or more output lines (a reply, or Task 6's elicitation request).
      pub fn handle_line(&mut self, line: &str) -> Vec<String>;
  }
  pub fn support_dir_from_env() -> PathBuf;    // Ruling 10
  pub const MAX_LINE: usize = 1 << 20;         // a longer line: -32700, the line skipped
  // tools.rs
  pub fn tool_name(id: &str) -> String;        // '.' → '_'
  pub fn tools_json(commands: &Value) -> Vec<Value>;   // Ruling 12
  pub fn arguments_to_params(spec: &Value, args: &Map<String, Value>) -> Result<(Map<String, Value>, Option<i64>), String>;
  ```
  The tools' text answers are compact JSON of the app's result. For a non-read command that ran, a trailing line says "Undo step: <label>" when the document's `undo_label` shows the open request.

- [ ] **Step 1: Write the fake app** in `tests/support/fake_app.rs`: a `core_control::Server` with `Role::CoProducer` in a private temp support folder. A thread answers queued calls from a script:
  - `command.run` gives `{"status":"ran <id>"}`;
  - `state.get` gives `{"target": …}`;
  - `co_producer.*` gives canned results;
  - a screenshot writes a 1×1 PNG and answers its path.

  Tests spawn the built binary (`env!("CARGO_BIN_EXE_vizdaw-mcp")`) with `VIZDAW_SUPPORT_DIR` set on the child, and talk to it over pipes.
- [ ] **Step 2: Write the failing tests** in `tests/server.rs`.
  - `initialize_era_handshake`:
    - `initialize` with `protocolVersion: "2025-06-18"` answers that version, `capabilities.tools`, `resources`, `serverInfo.name == "vizdaw-mcp"` and Ruling 12's instructions;
    - `"1999-01-01"` answers `"2025-11-25"`;
    - `notifications/initialized` gets no reply.
  - `modern_era_discover_and_versions`:
    - `server/discover` answers Ruling 2's `supportedVersions`;
    - a `tools/list` with `_meta` version "2026-07-28" answers `resultType: "complete"`, `ttlMs: 0` and `cacheScope: "private"`;
    - one with "2030-01-01" is `-32022`, with `data.supported` and `data.requested`.
  - `tools_come_from_the_allow_list`:
    - `tools/list` has `track_new_audio` (with `destructiveHint: false`), `transport_play_stop` (with `readOnlyHint: true`), `file_export_midi`, `begin_request`, `end_request`, `change`, `apply_change` and `screenshot`;
    - it has no `ui_click`, `media_compact`, `transport_record`, `track_delete` or `settings_co_producer`;
    - every name matches `^[A-Za-z0-9_.-]{1,64}$`, the names are unique, and every `inputSchema` has `type: "object"` and `additionalProperties: false`.
  - `tools_reach_the_app`:
    - `tools/call track_new_audio {seq: 1}` sends `command.run {id: "track.new_audio", params: {seq: 1}}`, and answers the app's result as text with `isError: false`;
    - an app error comes back as `isError: true` with the error's code and message;
    - `begin_request {name: "x"}` sends `co_producer.begin`.
  - `resources_read_state`:
    - `resources/list` has `vizdaw://state/document`, `vizdaw://state/mixer` and `vizdaw://commands`, and no `vizdaw://state/input`;
    - reading `vizdaw://state/mixer` sends `state.get {target: "mixer"}`;
    - an unknown URI is `-32602`.
  - `screenshot_returns_an_image`: the content is `{type: "image", mimeType: "image/png", data: <base64 of the PNG>}`, and the file is gone afterwards.
  - `hostile_messages_never_end_the_server`. After each of these the next `ping` still answers `{}`:
    - not JSON: `-32700`;
    - a 2 MiB line: `-32700`;
    - a batch `[…]`: `-32600`;
    - `"id": null`: `-32600`;
    - an unknown method: `-32601`;
    - `tools/call` with no name, with an unknown tool (both `-32602`), with `arguments` as a string, or with `{seq: "1"}` (`isError: true` naming `seq`);
    - a NUL in a string argument: passed through to the app's own refusal;
    - 70 000-character arguments;
    - nesting 10 000 levels deep: `-32700`, from serde_json's recursion limit.
  - `says_why_it_cannot_reach_vizdaw`:
    - with no socket, and `co-producer.off` present in the support folder, a tool call answers `isError: true` with "Co-Producer is turned off in VizDAW's Settings.";
    - without that file it answers "VizDAW is not running, or Co-Producer is off in its Settings.".
  - `reconnects_after_the_app_drops_it`: stop the fake app and start it again on the same path. The next tool call succeeds.
  - `base64_matches_rfc_4648`: "", "f", "fo", "foo", "foob", "fooba" and "foobar".
- [ ] **Step 3: Run** `cargo test -p vizdaw-mcp`. Expected: FAIL (no crate).
- [ ] **Step 4: Implement** Rulings 1, 2, 10 and 12.
  - `app.rs` wraps `core_control::client::Client`, with a 70 s call timeout (above the app's 60 s). On an `Io` error it reconnects once.
  - Tools for read and add commands that take `window` pass it as `window`.
  - `main.rs` reads stdin line by line, writes each output line with a single `\n` and flushes, and exits 0 at EOF.
  - `names.rs`'s literal scan adds `"vizdaw-mcp/src"` to its directory list.
- [ ] **Step 5: Run** `cargo test -p vizdaw-mcp -p core-control`. Expected: PASS.
- [ ] **Step 6: End-of-task checks. Commit** by paths: `crates/vizdaw-mcp`, `crates/core-control/tests/names.rs`, `Cargo.lock`. Message `feat(mcp): vizdaw-mcp, a stdio MCP server over the Co-Producer socket (plan 8a Task 5)`.

### Task 6: The `change` tool: preview, then the composer's yes

**Files:**
- Modify:
  - `crates/vizdaw-mcp/src/lib.rs`, `src/tools.rs`, `src/rpc.rs`;
  - `crates/vizdaw-mcp/tests/server.rs`, `tests/support/fake_app.rs`.

**Interfaces:**
- Consumes: Task 4's `co_producer.preview`, which gives `{token, needs_yes, items}`, and `co_producer.apply`, which gives `{results, undo_label}`. Also Task 5's `Server`.
- Produces: Ruling 8's flow.
  - **The `initialize` era.** `handle_line` returns an `elicitation/create` request line with id `"vz-elicit-<n>"`, then keeps the call pending.
    - Lines that arrive meanwhile are queued, and handled after the wait ends. Two kinds are handled at once: a `ping`, and a `notifications/cancelled` for the pending call, which ends the wait as a cancel with no reply.
    - The response with that id resumes the call.
  - **2026-07-28.** `tools/call` answers `{resultType: "input_required", inputRequests: {yes: {method: "elicitation/create", params}}, requestState: <24 hex>}`. The retry carries `inputResponses.yes` and that `requestState`.
    - A `requestState` is one-time, and is kept for at most 10 minutes and at most 8 at once.
    - An unknown or reused one is `isError: true`: "This question expired; call change again.".
  - **Tokens.** Tokens handed to the model are remembered, at most 8. `apply_change` with a token not among them is `isError: true`: "Use the token change returned in this conversation.".

- [ ] **Step 1: Write the failing tests** in `tests/server.rs`. The fake app's preview answers 2 items with `needs_yes: true`, unless the plan is `[edit.undo]`, which answers `needs_yes: false`.
  - `no_yes_needed_applies_at_once`: `change {commands: [{id: "edit.undo"}]}` calls preview, then apply, and answers the results text. No elicitation is sent.
  - `legacy_elicitation_accept_applies`:
    - after `initialize` with `capabilities.elicitation: {}`, a `change` emits an `elicitation/create` line whose `message` is Ruling 8's text with both items, and whose `requestedSchema` is exact;
    - answering `{action: "accept", content: {allow: true}}` sends `co_producer.apply` with the preview's token and the same commands;
    - the reply has the results and contains no token.
  - `legacy_elicitation_no_changes_nothing`: each of `decline`, `cancel`, accept with `allow: false`, accept with `allow: "yes"`, and a JSON-RPC error reply answers "The composer said no. Nothing changed.". No apply reaches the app.
  - `legacy_elicitation_meanwhile`:
    - a `tools/list` that arrives during the wait is answered after the wait;
    - a `ping` is answered at once;
    - `notifications/cancelled` for the call ends it with no apply and no reply.
  - `modern_elicitation_round_trip`:
    - with per-request `clientCapabilities.elicitation: {}`, `change` returns `input_required`;
    - the retry with `inputResponses.yes = {action: "accept", content: {allow: true}}` and the `requestState` applies;
    - a second retry with the same `requestState` is `isError: true` with "This question expired; call change again.", and no second apply.
  - `without_elicitation_the_model_gets_the_list_and_a_token`:
    - with no elicitation capability, `change` answers the items as "- <thing>: <change>" lines, then "Show this list to the composer and ask for a yes. Then call apply_change with token <token> and the same commands.";
    - `apply_change` with that token sends apply;
    - `apply_change` with `"0" * 32` is refused without reaching the app.
  - `the_list_is_capped`: a preview of 45 items gives 40 lines and "- and 5 more".
  - `hostile_elicitation_replies`: an `elicitation/create` response with an unknown id, a `content` that is an array, a 70 000-character message field, or a `requestState` of 70 000 characters never panics. Each is ignored or answered `isError: true`.
- [ ] **Step 2: Run** `cargo test -p vizdaw-mcp`. Expected: FAIL.
- [ ] **Step 3: Implement** Ruling 8 and the flow above. `change` and `apply_change` validate `commands`: an array of 1–`MAX_PLAN` objects with a string `id` and an optional `params` object. Anything else is `isError: true` before any socket call.
- [ ] **Step 4: Run** `cargo test -p vizdaw-mcp`. Expected: PASS.
- [ ] **Step 5: End-of-task checks. Commit** `feat(mcp): the change tool asks the composer through elicitation, with a token fallback (plan 8a Task 6)`.

### Task 7: The switch, the Co-Producer channel and copies in the app (Swift)

**Files:**
- Create:
  - `app/Sources/VizDAWAppModel/CoProducerSettings.swift`;
  - `app/Sources/VizDAWAppModel/CoProducerCommands.swift`;
  - `app/Sources/VizDAW/CoProducerSettingsView.swift`;
  - `app/Tests/VizDAWAppModelTests/CoProducerSettingsTests.swift`, `CoProducerCommandsTests.swift`.
- Modify:
  - `app/Sources/VizDAW/ControlChannel.swift` (a second server; `Deferred` gains `server`; the dispatch by server; the idle check);
  - `app/Sources/VizDAW/VizDAWApp.swift` (the Settings tab, `LiveCoProducerSink`, start at launch when on, stop at quit);
  - `app/Sources/VizDAW/AppCommands.swift` (File › Open Co-Producer Copy…);
  - `app/Sources/VizDAWAppModel/DocumentStore.swift` (`openCopy(path:)`, `open(path:)` routes copies);
  - `app/Sources/VizDAWAppModel/ControlCommands.swift` (`settings.co_producer` and `file.open_copy` in `handled` and dispatch; the `notAllowed` code);
  - `app/Sources/VizDAWAppModel/AppIcon.swift` (`settingsCoProducer`, SF Symbol `sparkles`);
  - `app/Tests/VizDAWAppModelTests/FakeSession.swift` (Task 4's methods, scripted);
  - `scripts/build-ffi.sh` (build `-p vizdaw-mcp`, copy it to `target/ffi/vizdaw-mcp`);
  - `scripts/make-app.sh` (copy it to `Contents/MacOS/vizdaw-mcp`, signed with the app as `vizdaw-scan` is).

**Interfaces:**
- Consumes: Task 3's `Session.openCopy(path:)` and `coProducerCopiesSize()`; Task 4's Session methods, `ControlServer.startCoProducer(path:appTimeoutMs:)` and the new request and result cases.
- Produces:
  ```swift
  @MainActor public protocol CoProducerSink {
      func start(path: String) -> String?        // nil: listening; else the reason it could not
      func stop()
      func markOff(_ off: Bool)                  // writes or removes <support>/co-producer.off
  }
  @MainActor @Observable public final class CoProducerSettings {
      public static let key = "VizDAW.coProducer"
      public static let socketName = "co-producer.sock"
      public static let idleMs: UInt64 = 300_000
      public private(set) var on: Bool          // false when the key is absent or not a Bool
      public private(set) var problem: String?  // "Co-Producer could not start: <reason>"
      public init(defaults: UserDefaults, supportDir: URL, sink: CoProducerSink)  // starts when on
      public func setOn(_ on: Bool)              // persists, starts or stops (Ruling 10)
      public var status: String                  // Ruling 10's three sentences
  }
  @MainActor public enum CoProducerCommands {
      public static let setting = "settings.co_producer"
      public static let openCopy = "file.open_copy"
      /// Ruling 13; `stamp` is "yyyy-MM-dd HH-mm", in local time, now.
      public static func run(_ request: ControlRequest, doc: (id: UUID, model: DocumentModel)?,
                             store: DocumentStore, host: ControlHost, stamp: String) -> ControlOutcome
      /// Every document: `coProducerIdle(idleMs)`; ends requests when `all` (switch off).
      public static func tidy(_ store: DocumentStore, all: Bool)
      public static func stamp(_ date: Date) -> String
  }
  ```
  **The tab** "Co-Producer", with `AppIcon.settingsCoProducer`, holds:
  - `Toggle("Co-Producer", isOn:)`, accessibility label "Co-Producer";
  - the caption "Lets Claude work on your songs through VizDAW's Co-Producer connection. Each request is one undo step, a copy of the song is saved first, and nothing that changes or deletes your own work happens without your yes.";
  - `status`;
  - when on, the `vizdaw-mcp` path (`Bundle.main.executableURL`'s folder + `vizdaw-mcp`) as selectable text, with a "Copy Path" button, accessibility label "Copy the Co-Producer server's path";
  - "Saved copies in open projects: N (X MB)", read on appear;
  - `problem`, when set, in `theme.state.warning`.

- [ ] **Step 1: Write the failing Swift tests**, with `MemoryDefaults()`, a fake sink and `FakeSession`.
  - `CoProducerSettingsTests`:
    - `offByDefaultAndPersists`:
      - a new model has `on == false`, the sink got no `start`, and it got `markOff(true)`;
      - `setOn(true)` stores `true`, calls `start(path: "<support>/co-producer.sock")` and `markOff(false)`;
      - a new model on the same defaults starts at once;
      - `setOn(false)` calls `stop()` and `markOff(true)`.
    - `aFailedStartSaysWhyAndStaysOff`: the sink returns "the folder belongs to another user". `on` is false, `problem` is "Co-Producer could not start: the folder belongs to another user", and `status` is the off sentence.
    - `aDamagedSettingReadsAsOff`: a String under `key` reads as off.
  - `CoProducerCommandsTests`:
    - `commandsRunBetweenEnterAndLeave`: `runCommand track.new_audio` makes the fake record `enter <stamp>`, `newTrack audio`, `leave`, in that order. A read command (`transport.stop`) records no enter.
    - `noRequestNoChange`: the fake's enter throws "Open a request first (begin_request).". The command is not run, and the outcome is `.failed` with that text.
    - `applyRunsThePlanAndReportsEveryCommand`:
      - `coProducerApply` with a plan of `track.rename` then `track.delete` makes the fake record `take`, enter, rename, leave, enter, delete, leave;
      - with the rename made to fail, the results are `["track.rename: <message>", "track.delete: not run (an earlier command failed)"]`, and delete is never called.
    - `beginEndAndPreviewPassThrough`: each maps to its Session call and result case. An empty `name` reaching Swift is still refused by the session's error, passed on.
    - `turningOffStopsAndClosesRequests`: `tidy(store, all: true)` calls `coProducerEnd` on every document with a request.
    - `idleTidyEndsOnlyIdleRequests`: `tidy(store, all: false)` calls `coProducerIdle(300000)` on each document.
    - `alertsAreTheCallsFailure`: a command whose run raises `m.alert` answers `.failed`, and `m.alert` is nil afterwards.
    - `stampIsLocalMinutes`: `stamp(<2026-11-14 15:02:59 local>)` is "2026-11-14 15-02".
    - `settingAndOpenCopyCommandsRefuseBadInput`:
      - `settings.co_producer {on: true}` sets the model on;
      - `file.open_copy {path: <a normal package>}` is `.failed` "That is not a Co-Producer copy.";
      - neither opens a panel.
  - **`NameScanTests` and `noLaneInWhatTheAppShows`** pass with the new copy.
- [ ] **Step 2: Run** `scripts/build-ffi.sh && scripts/swift.sh test --filter 'CoProducer'`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **The channel.** `ControlChannel` keeps `coProducer: ControlServer?` beside `server`. `pump` takes calls from both. `execute(call, server, coProducer:)` sends Co-Producer calls through `CoProducerCommands.run`; their `.opened`, `.job` and `.pending` outcomes use the same deferred machinery, with `Deferred.server`. `active` still means the developer server only.
  - **The idle check.** `pump` calls `CoProducerCommands.tidy(appStore, all: false)` every 100 ticks (5 s).
  - **`LiveCoProducerSink.start`:**
    1. checks that the support folder's owner is this user, from `FileManager.attributesOfItem`, and sets its mode to 0700;
    2. starts the server;
    3. returns `AlertItem(error, title: "Co-Producer").message` on failure.

    At quit, `stop()` runs before `ControlChannel.stop()`.
  - **The menu item** "Open Co-Producer Copy…" sits under File › Open Recent's group. It is enabled when the front document has a path and `coProducerCopiesSize().count > 0`. Its `NSOpenPanel` has `directoryURL` set to `<package>/Co-Producer/Copies`, `allowedContentTypes` set to the VizDAW package type, and `treatsFilePackagesAsDirectories = false`. It then calls `appStore.openCopy(path:)`.
  - **The scripts.** `build-ffi.sh` and `make-app.sh` gain `vizdaw-mcp`.
- [ ] **Step 4: Run** the Step 2 command, then `scripts/swift.sh test`. Expected: PASS.
- [ ] **Step 5: End-of-task checks**, including `scripts/make-app.sh`. Check that `dist/VizDAW.app/Contents/MacOS/vizdaw-mcp` exists and answers `ping` (`printf '{"jsonrpc":"2.0","id":1,"method":"ping"}\n' | dist/VizDAW.app/Contents/MacOS/vizdaw-mcp` prints `{"jsonrpc":"2.0","id":1,"result":{}}`). **Commit** `feat(app): the Co-Producer switch, its socket and requests in the app, and opening a saved copy (plan 8a Task 7)`.

### Task 8: README checks, the e2e, the backlog and the gates

**Files:**
- Modify:
  - `app/README.md` (new steps from 326);
  - `crates/vizctl/tests/e2e.rs`;
  - `docs/feature-backlog.md`.

**Interfaces:**
- Consumes: Tasks 1–7. No production code, unless a step finds a bug. Such a bug is fixed in its owner's module, with a regression test.

- [ ] **Step 1: Add the e2e** `co_producer_over_mcp`, `#[ignore]` and in the style of `multiq_install_and_switch_over_the_channel`.
  - **Start.** Launch the app, then spawn `dist/VizDAW.app/Contents/MacOS/vizdaw-mcp` with `VIZDAW_SUPPORT_DIR=<the launch's support folder>`, and talk MCP over its pipes with a small helper.
  - **Off.** A tool call answers "Co-Producer is turned off in VizDAW's Settings.".
  - **On.** Over the developer channel, `settings.co_producer {on: true}`. Then:
    - `<support>/co-producer.sock` exists with mode 0600;
    - `<support>` has mode 0700.
  - **Set up the song.** Over the developer channel, `file.new`, then `file.save {path: <home>/Song.vizdaw}`.
  - **A request over MCP, in the `initialize` era without elicitation:**
    1. `begin_request {name: "add two tracks"}`;
    2. `track_new_audio`, twice;
    3. `end_request`.

    Then over the developer channel:
    - `state.get document` has two tracks and undo label "Co-Producer: Add Two Tracks";
    - one copy exists under `Song.vizdaw/Co-Producer/Copies/`.
  - **A change with a token:**
    1. `begin_request {name: "rename the first track"}`, then `change` with `track.rename {seq, lane, name: "Kick"}`. It answers the list and a token;
    2. `apply_change` with a token of 32 zeros is refused;
    3. `apply_change` with the right token renames the track;
    4. the same token again is refused;
    5. `end_request`.
  - **The socket gate.** A raw `core_control::client::Client` on the Co-Producer socket gets `not_allowed` for:
    - `ui.click`;
    - `command.run media.compact`;
    - `command.run track.delete` with explicit ids;
    - `file.export_midi` with an existing path.
  - **A second MCP process with elicitation.** It sends `initialize` with `elicitation: {}`. `change` with `track.delete` emits `elicitation/create`:
    - answering `decline` leaves the track;
    - a second `change` answered `accept`/`allow: true` deletes it in one step. Its label is "Co-Producer: <the open request's title>".
  - **Undo from VizDAW.** Over the developer channel, `edit.undo` brings the track back.
  - **Off again.** `settings.co_producer {on: false}`. The next MCP tool call answers the off sentence, and the socket file is gone.
- [ ] **Step 2: Write the README steps** in `app/README.md`, under a heading "Plan 8a, Co-Producer safety and the server:". Each step states what to do and what is seen, and says "track":
  - **326.** Settings › Co-Producer: the switch is off on a fresh install. Turn it on: the status says it is on and shows the path of `vizdaw-mcp`. "Copy Path" copies it.
  - **327.** Add the copied path to Claude Code as an MCP server (`claude mcp add vizdaw -- <path>`), and ask Claude to "add two audio tracks". Two tracks appear. Edit › Undo once: both go together. The undo item reads "Undo Co-Producer: Add Two Audio Tracks", or the name Claude gave.
  - **328.** While Claude is adding things, rename one of your tracks by hand. Undo twice: first Claude's later steps, "(continued)", go; then your rename goes on its own.
  - **329.** Ask Claude to rename one of your tracks. Claude Code asks you directly, with the exact change listed. Say no: nothing changes. Ask again and say yes: it is renamed, as one undo step.
  - **330.** In a host that cannot ask you itself (Claude's desktop app, as of 2026-11), ask for the same: Claude shows the list in the chat and waits for your yes before applying it.
  - **331.** Ask Claude to delete a track, or to compact media, or to record: deleting goes through your yes; Compact Media and recording are refused.
  - **332.** Ask Claude to export MIDI to a file that already exists: it is refused, and the file is unchanged.
  - **333.** After a request, File › Open Co-Producer Copy…: the copy named "<date> <time> before <request>" opens as a new untitled project, as the song was before the request. Close it without saving.
  - **334.** Delete the track Claude added, Compact Media and Save, then open the copy again: it still opens complete, with its audio.
  - **335.** Turn Co-Producer off in Settings while Claude is connected. Claude's next action says Co-Producer is turned off in VizDAW's Settings. Turn it back on: Claude works again.
  - **336.** Settings › Co-Producer shows the number and size of saved copies in your open projects.
- [ ] **Step 3: Update the backlog** (`docs/feature-backlog.md`).
  - In the "Co-Producer over MCP" row, note 8a's delivery: the separate socket, off by default, requests, previews with tokens, elicitation with a token fallback, and copies.
  - **New rows:**
    - "Compact Media offers to remove old Co-Producer copies, with sizes (spec §6.7)" — 8b;
    - "Move an untitled project's Co-Producer copies into its package on first save; until then they need its scratch media" — 8b;
    - "A job that ends after its Co-Producer call (a bounce placed on new tracks) lands outside the request's undo step" — 8c;
    - "Previews for the remaining change and remove commands" — 8c, 8d and 8e;
    - "A runtime check of request names against the banned-name list, if a way is found that keeps the list out of shipped code";
    - "Recheck which MCP hosts support elicitation and the 2026-07-28 revision before release";
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
  - the e2e, `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`: 32 ignored tests run, and all pass.

  Then `pgrep -lf MacOS/VizDAW` and `pgrep -lf vizdaw-mcp` print nothing. Report against the baseline: registry 183, `COMMAND_NAMES` 18, error codes 16, README through 336, e2e 32, `VERSION` 9.
- [ ] **Step 5: Commit** `docs(app): Co-Producer README checks, e2e and backlog (plan 8a Task 8)`.

## Self-Review

1. **Spec coverage (the §12 row 8a).**
   - **The socket and the switch** (§6.1, §6.4):
     - the separate socket at the fixed path, with the same checks: Ruling 10 and Tasks 1, 4 and 7;
     - off by default, persisted, and closing everything when off: Task 7;
     - `vizctl`'s channel uncovered: `active` stays the developer server's;
     - the off and not-running messages: Task 5.
   - **`vizdaw-mcp`** (§6.1, §11.1, §11.2, §11.12):
     - tools from the registry and `state.get` resources: Task 5;
     - the own tools: Tasks 5 and 6. The notes, plug-in list and metrics tools belong to 8b, 8d and 8f;
     - its licence and dependencies: Ruling 1.
   - **Classes and the allow-list** (§6.5, §11.6): Ruling 5 and Task 1, including spec §6.5's refused list verbatim, and a test that every entry has its class.
   - **One undo step per request** (§6.2, §11.4):
     - Task 2, with the property test and the hand-edit "(continued)" test;
     - idle close and one request per document: Rulings 3 and 4, and Tasks 2 and 4.
   - **Preview, token, apply and elicitation** (§6.6, §11.3, §11.5): Rulings 6–8 and Tasks 4 and 6. The views are 8c's; the text fallback is the 8a form.
   - **The saved copy** (§6.7, §11.7):
     - Ruling 9 and Tasks 3 and 4;
     - "copies stay complete": `retention_and_prune_keep_what_copies_use`;
     - opening a copy: `open_copy` and the menu item;
     - the copies' total size in Settings: Task 7.
   - **Never replace a file** (§6.7): Ruling 11 and `outward_paths_never_replace`.
   - **"Read-only and add commands only at first, then change and remove behind the yes":** Tasks 1 and 5 ship read, add and outward; Tasks 4 and 6 add change and remove behind the yes.
   - **§11.15's tests:**
     - hostile input on the socket and through `vizdaw-mcp`: Tasks 1, 5 and 6;
     - the property test: Task 2;
     - the hand-edit test: Task 2;
     - Save and Compact keeping copies: Task 3;
     - no path replaces: Task 1;
     - no token, no change: Tasks 1 and 4;
     - the switch off refuses everything: Tasks 7 and 8;
     - the e2e: Task 8.
   - **Moved out on purpose:** Compact's offer to remove copies, to 8b ("Not in this plan").
2. **Steps.** Every test step names its tests and assertions with the plan's values: ids, sentences, counts, paths and modes. Code steps give signatures and point to the ruling that fixes the behaviour. Tool descriptions, elicitation text and Settings copy are written out because the spec leaves them to the plan (§11.3, §11.12).
3. **Type consistency.** These names are spelled the same in every task:
   - `Class`, `Role`, `Planned`, `PREVIEWABLE`, `MAX_PLAN`;
   - `co_producer::admit`, `check_output`, `effective_class`, `canonical`;
   - `ErrorCode::NotAllowed`;
   - `RequestInfo`, `request_label`, `begin_request`, `end_request`, `set_acting`;
   - `COPIES_DIR`, `write_copy`, `load_copy`, `copy_parent`, `list_copies`;
   - `diff::describe`, `ChangeItem`;
   - `co_producer_begin`, `co_producer_end`, `co_producer_enter`, `co_producer_leave`, `co_producer_idle`, `co_producer_preview`, `co_producer_take`, `co_producer_request`, `co_producer_copies_size`, `open_copy`;
   - `PlannedRow`, `PreviewRow`, `ChangeItemRow`, `CoProducerRequestRow`;
   - `start_co_producer`;
   - `CoProducerSettings.key`, `CoProducerCommands.run`, `tidy`, `stamp`;
   - `settings.co_producer`, `file.open_copy`.
4. **Review Focus.** Each of the five lines names the tests that pin it, in Tasks 2–8.
5. **Proportion.** Eight tasks, at the top of the owner's lighter pace, because 8a carries the safety core that every later Co-Producer plan stands on. Code appears only as signatures. The plan reuses the history group, the autosave pattern for copies, the retention rule, the control server and its checks, and `ControlCommands.run`.
6. **Owner decisions.** None are open. Three calls are stated for the owner's review:
   - Compact's offer moves to 8b ("Not in this plan");
   - request names are not checked against the banned list at run time (Ruling 4);
   - the support folder becomes 0700 so the socket can live at the spec's fixed path (Ruling 10).
