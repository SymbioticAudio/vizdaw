# VizDAW Agent Control Channel — Design

Status: design approved in conversation 2026-10-18 (§3 is the approved design, binding); this written spec fleshes it out and is pending review.
Parent spec: `2026-10-15-vizdaw-design.md` (§4 invariants, §10 faults, §12 testing). Related: `2026-10-16-editors-design.md` (§3.5 accessibility, §3.8 Busy contract; plan 4f's command registry).

## 1. Intent

Agents that build and review VizDAW (implementers, reviewers, CI) need to launch the app, run the manual-checklist steps in `app/README.md`, read back what happened and attach screenshots — without computer-use tools, which stay banned in this project. The control channel gives them a narrow, local, scriptable door into VizDAW's own state and windows:

- run any menu command by a stable ID, with the answers its dialogs would ask for as parameters;
- read transport, document, editor, selection and window state;
- read a window's accessibility tree (the one VoiceOver uses, which the editors already build) with element IDs, and click, drag and type into VizDAW's own windows;
- take a PNG of a window, rendered by VizDAW itself;
- wait for a condition (playing, a tick reached, a window shown) instead of sleeping.

Success: an agent with only a shell runs `vizctl launch`, creates the demo, plays it, checks the position advanced, takes a screenshot, stops and quits — and the same steps run as an end-to-end test on CI.

### Non-goals

- An MCP server (later; `vizctl` is the only client now).
- Any network transport, remote access, multi-user use or authentication beyond file-system permissions.
- System-wide input (Quartz events), moving the real pointer, or controlling other apps.
- A scripting language or macro recorder for users; this is a developer and agent tool, off by default.
- Driving plugin editors' own UIs (they run in other processes or draw through Metal); their windows are listed and screenshotted as far as the app can render them.
- Every command of the app: the registry covers today's menu commands; plan 4f extends it to every command (menu or not) and binds keys from it.

## 2. Terms

- **Call:** one request line and its reply line.
- **Registry:** the table of menu commands by stable ID (§6), in Rust.
- **Target document:** the document a call acts on: the document shown by the call's `window`, else the front-most document window's, else any open document.
- **Window point:** a position in a window in points, origin at the top-left of the window frame (title bar included). Screenshot pixels divided by the scale are window points.

## 3. Approved design (binding)

1. Purpose: agents can launch VizDAW, run README checklist steps, check state and attach screenshots without computer-use tools.
2. Off by default: enabled only by the environment variable `VIZDAW_CONTROL=<socket path>` or a `--control <path>` launch flag. A Unix domain socket in a private directory with 0600 permissions; no network port.
3. Protocol: JSON lines; request `{id, cmd, params}`, reply `{id, ok, result | error}`. Parsing, validation and dispatch types live in Rust; the Swift app executes UI commands on the main thread.
4. Commands: `commands.list`; `command.run {id, params}` (menu commands by stable ID); `state.get {transport | document | editor | selection | windows}`; `ui.tree {window}` (accessibility tree with element IDs); `ui.click` / `ui.drag` / `ui.key {element or point, window}` (events into VizDAW's own windows, not system-wide events, no real cursor movement); `ui.screenshot {window}` → PNG rendered by the app (no Screen Recording permission; Core Animation content included); `app.new_demo`, `app.open {path}`, `app.quit`; `wait {condition, timeout}`.
5. Rules: a programmatic call never opens a dialog (it takes the dialog's answers as parameters or fails with an error naming the dialog); Busy returns an error; malformed or hostile requests return errors and never crash the app.
6. Tool: a small `vizctl` CLI (a Rust binary in the workspace). MCP later.
7. Tests: property/fuzz tests of the parser and of every command's parameters; fuzz tests of loading damaged project files (a proptest harness in `cargo test`; cargo-fuzz targets optional); an end-to-end test that launches the built app with the channel, drives one README step and checks the results, robust on slow CI.
8. Process change: implementers and reviewers may use `vizctl` against a throwaway HOME; the ban on computer-use tools stays (§12).
9. Size: about 10–12 tasks, in the order Rust protocol → registry/state → Swift server and UI commands → screenshot → `vizctl` → tests/E2E → docs.

## 4. Architecture

```
vizctl (Rust CLI)          ── JSON lines over a Unix socket ──┐
                                                                ▼
core-control  (new crate, Rust, Unix, no UniFFI)
   protocol: framing, limits, ids, errors   request: validation of every command
   registry: menu commands, dialogs, shortcuts   wait: conditions and facts
   server: accept thread + one thread per connection; call queue; timeouts
   client: blocking client (vizctl, tests)
        │ validated Command (calls that need the app)
        ▼
core-ffi::control  (UniFFI)   ControlServer object · typed ControlRequest / ControlResult
        │ next_call() / reply() / fail() / wait_timed_out()    result → JSON here
        ▼
Swift (main thread)
   VizDAW/ControlChannel     20 Hz pump (common run-loop modes), deferred answers, windows
   VizDAWAppModel/ControlCommands   registry handlers on DocumentModel, state rows, dialogs
   VizDAWEditors/ControlUI   accessibility tree, element IDs, mouse and key events
   VizDAWEditors/WindowSnapshot   PNG from the window's layers and self-drawing views
```

### 4.1 Crates and targets

- `crates/core-control` (new, GPL-3.0-only, `#![forbid(unsafe_code)]`): everything about the wire — parsing, limits, validation, registry, wait conditions, the socket server and a client. Platform-neutral Unix code (builds and tests on the Ubuntu CI job). No UniFFI, no AppKit.
- `crates/core-ffi/src/control.rs`: the UniFFI surface. It maps a validated `Command` to a typed `ControlRequest` for Swift and a typed `ControlResult` back to JSON, so the wire schema lives in Rust only; Swift never builds JSON.
- `crates/vizctl` (new): library (`cli`, `launch`) and binary. Depends on `core-control` only.
- Swift: model-level logic (command handlers, state rows, opening without dialogs, launch-flag parsing) in `VizDAWAppModel` (tested with `FakeSession`); window-side logic (tree, events, snapshot) in `VizDAWEditors` (tested with off-screen windows); the pump, window registry and app wiring in `VizDAW`.

### 4.2 Threading

- The server's accept thread and one thread per connection read lines, validate them and answer `commands.list` and `ping` themselves. Every other call becomes a `Call { token, command }` in a queue, and its connection thread blocks on a channel for the answer.
- Swift's `ControlChannel` pump runs every 50 ms on the main thread in the common run-loop modes (so it keeps answering while a menu or a drag tracks). Each tick it first advances deferred answers, then takes up to 8 calls.
- A call the app does not answer within 60 s (a wait: its own timeout + 15 s) is answered `timeout` by the server; the app's late answer is dropped. A blocked main thread therefore never hangs a client.
- No Rust object is held by Swift across the run loop except the `ControlServer` itself (an `Arc`, thread-safe).

### 4.3 Deferred answers

Some calls cannot be answered in the tick they arrive: a new document's window appears a few run-loop turns later; posted input events are handled by the event loop; a `wait` holds until its condition. The pump keeps these as deferred answers (token, deadline, step) and steps them every tick; one whose client has gone (`is_waiting` false) is dropped. The server does not detect a client hanging up mid-call: that call keeps its connection slot until its timeout.

### 4.4 Lifecycle

- `App.init` reads `--control PATH` (wins) or `VIZDAW_CONTROL`, and starts the channel before any document opens, so launch-time questions already follow §7. If the socket cannot be made the app prints why and exits with status 2 (a launching agent learns at once).
- `applicationWillTerminate` stops the channel first: pending calls get `shutting_down`, connections close, the socket file is removed.
- `app.quit` replies `{quitting: true}` and terminates on the next run-loop turn.

## 5. Protocol

### 5.1 Framing and limits

| Limit | Value | Over the limit |
|---|---|---|
| Request line | 1 MiB (newline excluded) | `parse_error`, the connection stays usable |
| Nesting of arrays/objects | 32 | `invalid_request` |
| String `id` | 128 characters | `invalid_request` |
| String parameter | 4096 characters (paths: 1024 bytes) | `invalid_params` |
| Coordinates | ±100 000 points, finite | `invalid_params` |
| Connections | 4 at once | one `unavailable` line, then closed |
| Idle connection | 300 s (mid-line or between lines) | closed |
| Unread reply | 30 s write timeout | connection dropped |
| App answer | 60 s | `timeout` |
| `wait` | 1–600 000 ms, default 30 000 | `invalid_params` |
| `ui.tree` | depth ≤ 64, at most 5000 nodes | `truncated: true` |
| `ui.drag` | 1–200 intermediate points, default 16 | `invalid_params` |
| `ui.key` text | 1–256 characters | `invalid_params` |

UTF-8 only; blank lines are skipped; a reply is one line. Requests on one connection are answered in order; parallel clients use parallel connections.

### 5.2 Request and reply

```json
{"id": 7, "cmd": "state.get", "params": {"target": "transport"}}
{"id": 7, "ok": true, "result": {"doc": "…", "playing": false, "tick": 0, …}}
{"id": 8, "ok": false, "error": {"code": "dialog_required", "message": "this would open the \"Save\" dialog; pass `path` instead", "dialog": "Save"}}
```

- `id`: an integer or a string (≤ 128 characters), echoed. A line whose id cannot be read is answered with `"id": null`.
- `cmd`: lowercase letters, digits, `.` and `_`, ≤ 64 characters.
- `params`: an object; absent or null is `{}`; any other type is `invalid_params`. JSON nested deeper than serde_json's 128-level limit is `parse_error`. Unknown top-level fields are `invalid_request`; unknown parameters are `invalid_params`.

### 5.3 Errors

`error` is `{code, message, dialog?, details?}`.

| Code | Meaning |
|---|---|
| `parse_error` | not UTF-8 JSON, or a line over 1 MiB |
| `invalid_request` | not a request object (id, cmd, params) |
| `unknown_command` | no such `cmd` |
| `invalid_params` | a parameter is missing, unknown, of the wrong type or out of range; the message names the command and the parameter |
| `unknown_id` | `command.run` names no registered command |
| `no_document` | no document is open (or the window shows none) |
| `no_window` | the window number names no VizDAW window |
| `no_element` | the element ID resolves to nothing, or to an element whose role or label changed |
| `dialog_required` | the call would open a dialog; `dialog` names it |
| `busy` | a plugin call is on the main thread's stack (spec 4 §3.8); try again |
| `failed` | the action ran and failed; the message is what the alert would have said |
| `unavailable` | the command is disabled now (its menu item would be grey); the message says why |
| `timeout` | a wait's condition never held (`details`: condition, timeout, last facts), or the app did not answer |
| `shutting_down` | VizDAW is quitting |
| `internal` | a bug (a caught panic) |

### 5.4 Common parameters

- `window`: a window number from `state.get windows`, `"key"` or `"main"`. Absent: the front-most document window.
- Element IDs (`ui.tree`): `"<child index path>:<check>"`, e.g. `"0.0.2.0.0.9:30241aac"`. The path walks `accessibilityChildren` from the window; the check is FNV-1a (32-bit) of the element's role, U+001F and its label (its title when it has no label). An ID is resolved again on every use; if the element there now has another role or label the call fails `no_element` rather than click the wrong thing.
- Points: `{x, y}` in window points (§2). Element targets use the centre of the element's frame.
- Modifiers: an array of `shift`, `control` (`ctrl`), `option` (`alt`), `command` (`cmd`).

### 5.5 Commands

**`ping`** → `{pong: true}` (answered by the server; readiness checks).

**`commands.list`** → `{commands: [{id, menu, title, shortcut, document, params: [{name, type, required, help, values?, extension?}], dialogs: [{name, when, answered_by}]}]}` (answered by the server).

**`command.run {id, params?, window?}`** runs registered command `id` (§6) on the target document the way its menu item does, with these differences: a disabled item answers `unavailable`; a dialog follows §7; an alert the command raises is the call's `failed` error instead of a sheet. Result `{command, doc, window, undo, status}`: `undo` is the undo label afterwards, `status` the editors' status line. Commands that make or open a document (`file.new`, `file.open`, `file.create_demo*`) answer once its window is on screen (≤ 30 s) with that window's number.

**`state.get {target, window?}`**

- `transport` (read fresh): `{doc, playing, seq, tick, position, seconds, sample_rate, device, faults, report}`.
- `document`: `{doc, title, path, dirty, version, undo, redo, selected_seq, sequences: [{id, name, top_level, uses, length, lanes}], lanes: [{id, name, destination, events, regions, references, mute, solo, arm}] (of the selected sequence), warnings, alert}`; `alert` is the alert the window would show, or null.
- `editor`: `{doc, detail: {kind, seq, lane, start, end} | null, windows: [{id, detail}], tool, snap: {enabled, division}, exact_mode, gesture, status}`; `kind` is `piano_roll`, `list`, `strip_chart` or `drum_grid`.
- `selection`: `{doc, time_range: {start, end} | null, lanes: [{seq, lane}], events: [ids]}`.
- `windows`: `{windows: [{number, title, kind, doc, key, main, visible, frame}], alerts: [{title, message}]}`, front to back; `kind` is `document`, `mixer`, `editor`, `plugin`, `modify_panel`, `plugin_manager`, `settings` or `other`; `frame` is in screen points (AppKit, bottom-left origin); `alerts` are the alerts (app-level and documents') the channel kept from showing (§7).

**`ui.tree {window?, max_depth?}`** → `{window, truncated, nodes: [{id, role, subrole?, label?, title?, value?, frame: {x, y, width, height}, enabled, selected?, focused?, actions?, children?}]}` in window points. Absent fields are left out. `actions` is a hint (`["press"]` for buttons, cells, menu items). SwiftUI builds its accessibility tree only while an assistive app asks for one; the first tree or element call sets AppKit's enhanced-user-interface attribute on the application, as VoiceOver does.

**`ui.click {element | point, window?, button?, count?, modifiers?}`**, **`ui.drag {from, to, window?, steps?, modifiers?}`** (`from`/`to`: an element ID string or a point object) → `{window, events, x, y}` once the app has handled the events. Mouse events are built with `NSEvent.mouseEvent` for the window and posted to VizDAW's own event queue (`NSApp.postEvent`), so controls that track the mouse in a loop read the rest of the gesture from the queue. The window is ordered to the front without activating the app (macOS 14+ does not let a background app take focus), and while channel events are queued the editors accept them as first clicks instead of spending them on activation. Clicks that open a menu (context menus, pop-up buttons) leave it open and tracking; the channel keeps answering (common modes) and `ui.key escape` closes it.

**`ui.key {key | text, modifiers?, element?, window?}`** → `{window, events, x: 0, y: 0}`. `key` is one character or a named key (`return`, `enter`, `tab`, `space`, `delete`, `escape`, `forward_delete`, `home`, `end`, `page_up`, `page_down`, `left`, `right`, `up`, `down`, `f1`–`f11`); `text` types characters (no modifiers). Strokes carry the US-ANSI virtual key code and the characters `NSEvent` reports. `element` is clicked first to give it the keyboard. A stroke that is a registered menu shortcut (§6) runs that command on the window's document, as the menu would for the key window; when that command is unavailable (a text field has the keyboard), or the stroke is no shortcut, the key goes to the window's first responder (`NSWindow.sendEvent`). This works whether or not VizDAW is the active app.

**`ui.screenshot {window?, path?, scale?}`** → `{window, path, width, height}`. `path`: an absolute `.png` path in an existing folder; absent: `screenshot-<n>.png` in the socket's private folder. `scale`: 0.25–4 pixels per point; absent: the window's backing scale. See §9.

**`app.new_demo {audio?}`**, **`app.open {path, plugins?, restore_autosave?}`** → `{doc, window}` once the new window is on screen (≤ 30 s). `app.open` without `path` is `dialog_required` "Open".

**`app.quit`** → `{quitting: true}`, then VizDAW quits (§4.4).

**`wait {condition, …, window?, timeout_ms?}`** → `{condition, elapsed_ms}`; on timeout the error's `details` hold the condition, the timeout and the last facts seen. Conditions:

| `condition` | Parameter | Holds when |
|---|---|---|
| `playing` | | the target document's transport plays |
| `stopped` | | it is stopped |
| `tick_at_least` | `tick` | its tick is ≥ `tick` |
| `documents_at_least` | `count` | at least `count` documents are open |
| `window_titled` | `title` | a visible VizDAW window's title contains `title` |
| `window_gone` | `title` | no visible window's title contains it |
| `detail_kind` | `kind` (`none`, `piano_roll`, `list`, `strip_chart`, `drum_grid`) | the target document's detail pane shows it |

The facts are gathered on the main thread every tick (the target document's transport is polled, since background windows do not poll); `control_wait_met` (Rust) decides.

## 6. Command registry

`core_control::registry::COMMANDS` lists every menu command of the app today by stable ID: menu, title, default shortcut, whether it needs a document, its parameters and the dialogs they answer. Swift's `ControlCommands.handled` must equal the registry's IDs (a test checks). `commands.list` publishes it. Plan 4f extends it to non-menu commands and drives key bindings and menu titles from it.

| ID | Menu item | Shortcut | Parameters (dialog answered) |
|---|---|---|---|
| `file.new` | File > New | ⌘N | |
| `file.open` | File > Open… | ⌘O | `path` ("Open", always), `plugins` ("Open with Plugins Off?"), `restore_autosave` ("Restore Autosave") |
| `file.create_demo` | File > Create Demo Project | | |
| `file.create_demo_audio` | File > Create Demo Project with Audio | | |
| `file.close` | File > Close | ⌘W | |
| `file.save` | File > Save | ⌘S | `path` ("Save", when never saved) |
| `file.save_as` | File > Save As… | ⇧⌘S | `path` ("Save", always) |
| `edit.undo`, `edit.redo` | Edit > Undo, Redo | ⌘Z, ⇧⌘Z | |
| `window.mixer`, `window.plugin_manager` | Window > Mixer, Plugin Manager | ⌥⌘M, ⌥⌘P | |
| `editors.piano_roll`, `.list`, `.strip_chart`, `.drum_grid` | Editors > … | ⌘1–⌘4 | |
| `editors.piano_roll_window`, … `_window` | Editors > Open … in Window | ⌥⌘1–⌥⌘4 | |
| `editors.close_detail` | Editors > Close Detail Editor | | |
| `editors.insert_event` | Editors > Insert Event | ⌘I | |
| `editors.nudge_earlier`, `_later`, `_up`, `_down` | Editors > Nudge … | ⌥←→↑↓ | |
| `modify.select_by_rule`, `.transpose`, `.quantize`, `.change`, `.move` | Modify > … (opens the non-modal panel) | ⌃⌘S, T, U, M, V | |
| `modify.again` | Modify > Modify Again | ⌃⌘R | |
| `modify.template` | Modify > Templates > name | | `name` (required) |
| `modify.exact_mode` | Modify > Exact Mode | ⌃⌘E | `on` (absent toggles) |
| `sequence.make_reference` | Sequence > Make Reference from Selection | ⌥⌘R | |
| `sequence.back` | Sequence > Back to Parent Sequence | ⌘[ | |
| `transport.play_stop` | Transport > Play / Stop | Space | |
| `transport.loop` | Transport > Loop | | `on` (absent toggles) |
| `transport.return_to_start` | Transport > Return to Start | Return | |

The select-and-modify panel is a non-modal window, not a dialog: the `modify.*` panel commands open it.

## 7. Dialogs, alerts and Busy

- **Always-opened dialogs** (Open, Save As): without their answering parameter the call is refused in Rust (`dialog_required`) and never reaches the app.
- **Conditional dialogs** are decided by the app: Save of a never-saved document needs `path`; opening a project after a crash, or with Shift held, asks "Open with Plugins Off?" (`plugins`: `normal`, `off`, `disable`); a newer autosave asks "Restore Autosave" (`restore_autosave`). A control call installs its own answers for the store's recovery questions; a question without an answer fails the call `dialog_required` naming it, and a document it opened meanwhile is closed again.
- **Alerts raised by a call** (a document's alert sheet, the store's alert) are taken and returned as the call's `failed` error; nothing is shown.
- **Alerts not caused by a call** (catalog problems, a launch-argument open's recovery questions, a document's alert raised later, such as a transport or mixer failure) are not shown while the channel is on: each is printed to stderr once and listed in `state.get windows` → `alerts` (the newest 20); a document's alert also stays in `state.get document` → `alert`. Recovery questions of such opens are answered Cancel / "last save".
- **Busy:** before every call the pump asks Rust whether a plugin call is on the main thread's stack (`control_host_busy`); if so the call answers `busy`. Only transport Play/Stop and undo/redo map a thrown `VizError::Busy` to `busy` (never an alert); other commands rely on the pre-call busy check and otherwise answer `failed`.
- **Raw input is not a programmatic command:** a `ui.click` on a control that opens a modal panel behaves as a user's click would. The channel keeps answering in modal and tracking modes, so the panel can be read (`state.get windows`, `ui.tree`) and dismissed (`ui.key`).

## 8. Security

- Off unless `--control PATH` or `VIZDAW_CONTROL` names an absolute socket path (≤ 103 bytes, the `sun_path` limit).
- The socket's directory is created with mode 0700 when missing; an existing one must be a real directory (not a symlink) with no group or other permission bits, owned by the user (checked by comparing its owner with the new socket's). Otherwise VizDAW refuses to start the channel.
- A stale socket file (nobody answers a connect) is removed; a live one is refused ("another VizDAW is listening"); any other file at the path is refused.
- The socket is made 0600 right after `bind` (the private directory closes the window between the two). It is removed when VizDAW quits.
- No network port, no authentication beyond the file system: anything able to connect is the user. The channel can do what the user can do in VizDAW's windows, nothing more: it opens `.vizdaw` packages and writes screenshots and saves to paths the call names, as the user.
- Every request is validated in Rust with the limits of §5.1; parsing is total (no panic on any input, fuzzed in tests). Connections are capped, idle ones closed, and a slow reader cannot block the app (the app answers into a channel; only the connection thread writes).

## 9. Screenshots

Rendered in-process, no Screen Recording permission:

1. The window's frame view's layer tree is drawn into a bitmap with `CALayer.render(in:)`, which includes SwiftUI and Core Animation content (the editors' layers, the playhead).
2. AppKit does not redraw an occluded window — one behind the agent's terminal, or on a locked or sleeping screen — so its layers can hold a stale picture. Every view whose class draws itself (`draw(_:)` into its layer) is therefore drawn again in place, front to back, with its own Core Animation sublayers (playhead, overlays) on top. The editors show what they hold now.
3. A flipped view rendered on its own is flipped back (its layer is geometry-flipped).

Known limits: content the window server composites alone (vibrancy materials such as the sidebar, Metal and video layers, out-of-process plugin views) renders flat or blank; SwiftUI-hosted AppKit controls that never drew while occluded may be missing; views redrawn in step 2 are painted over floating overlays in front of them (the sidebar). Screenshots are evidence for agents and reviewers, not pixel-exact golden images.

## 10. `vizctl`

```
vizctl [--socket PATH] [--timeout SECONDS] COMMAND [ARGS]      (socket default: $VIZDAW_CONTROL)
  launch [--app PATH] [--home DIR] [--demo] [--env K=V]...      ping · commands · run ID [PARAMS-JSON]
  state TARGET · tree [--depth N] · find TEXT · click ID | X Y [--right] [--count N] [--mod LIST]
  drag FROM TO [--steps N] · key KEY [--mod LIST] · type TEXT · screenshot [--out PATH] [--scale S]
  wait CONDITION [--tick N | --count N | --title T | --kind K] [--timeout-ms N]
  new-demo [--audio] · open PATH [--plugins …] [--restore-autosave true|false] · quit · call CMD [PARAMS-JSON]
  every command but launch takes --window N
```

- Prints the result as JSON on stdout; an error reply prints `{"error": …}` and exits 1; bad usage exits 2; no connection, a launch failure (including another VizDAW already answering in that home) or a timeout (no reply, closed connection, malformed reply) exits 3.
- `launch` makes a throwaway home (default `/tmp/vizctl-<pid>-<n>`, private) with `ctl/vizdaw.sock`, `support/` (as `VIZDAW_SUPPORT_DIR`) and `vizdaw.log`; starts the app (`--app` a `.app` or executable, default `$VIZDAW_APP` or `dist/VizDAW.app`) with `HOME` set to it, `--control`, `--no-scan`, optional `--demo` and extra environment; waits (≥ 120 s) until `ping` answers; prints `{pid, socket, home, log}` and the `export VIZDAW_CONTROL=…` line on stderr. It never touches the user's own home or support folder.
- `find TEXT` filters `ui.tree` client-side to the elements whose label, title or value contains TEXT.
- The client's reply timeout (default 120 s) always exceeds the server's 60 s and a wait's own timeout.

## 11. Testing

- **Protocol (Rust, `core-control`):** request reading, ids, limits, error codes, replies round-trip; element IDs and key strokes. Proptest: random bytes, random JSON and random requests are always answered (a reply line that parses), never a panic.
- **Validation and registry:** the minimal valid form of every command; every refusal path; registry IDs unique, dialogs answered by real parameters, shortcuts unambiguous and mapped back to their commands; wait conditions over facts. Proptest: 2048 random parameter maps for every command and every registry entry.
- **Server:** private directory and modes, refusal of unsafe places, stale versus live sockets, calls reaching a fake app thread, local commands, garbage and over-long lines on a surviving connection, the app-answer timeout and dropped late answers, a wait's own timeout, the connection cap, shutdown answering pending calls, parallel clients.
- **FFI:** typed requests and results round-trip through the real server; failures carry codes and dialogs; a timed-out wait carries its facts; tree nodes nest by path with checked IDs.
- **Damaged project files:** byte damage (flip, insert, delete, truncate) and JSON edits (type-keeping tweaks to extremes, nasty values, removals) of the demo project either are refused or load and answer every read the editors make (sequences, lanes, events, blocks under every rule, mixer, destinations, templates) — never a panic and never `VizError::Internal`; at least a tenth of the edited documents must load so the reads are exercised. Cases default to 128/256 and scale with `VIZDAW_FUZZ_CASES`. Damaged packages on disk are refused with an error. cargo-fuzz targets are optional and not part of the plan.
- **Swift:** every registry ID has a handler; transport, save, open-with-recovery, disabled commands and Busy map to the right outcomes (with `FakeSession`); the accessibility tree lists canvas items with checked paths and correct (not mirrored) frames; click, drag and key events land where aimed; snapshots are the right way up, include Core Animation content and draw stale layers fresh.
- **End to end (`crates/vizctl/tests/e2e.rs`, `#[ignore]`):** `VIZDAW_E2E_APP=dist/VizDAW.app cargo test -p vizctl --test e2e -- --ignored --nocapture --test-threads 1`. (1) README steps 2–3: launch, create the demo, check Song is selected, check `file.save` answers `dialog_required` "Save", play, wait for `playing` and for the tick to advance, screenshot (a PNG ≥ 400 px wide), stop, wait `stopped`, find "Arrangement" in the tree, open the list editor and wait `detail_kind list`, quit and wait for the process to exit with the socket gone. (2) Click the toolbar's Play button by element ID, wait `playing`, press Space, wait `stopped`. Offline audio (`VIZDAW_AUDIO=offline`). Every wait is a condition with a 60 s timeout; launch waits up to 180 s; no fixed sleeps. CI runs it in the `app` job (macos-15, which has a window server) after `scripts/make-app.sh`.
- **Manual checklist:** steps for the channel in `app/README.md`.

## 12. Process change

Implementers and reviewers (human or agent) may drive VizDAW through `vizctl` to run checklist steps, read state and attach screenshots, always against a throwaway home made by `vizctl launch` (never the user's real `HOME` or support folder), with `VIZDAW_AUDIO=offline` unless sound is the point, and they quit it (`vizctl quit`) when done. A screenshot or state dump is evidence in a review, not a substitute for the human checklist steps that need ears, VoiceOver or real devices. The ban on computer-use tools stays: no system-wide input, no screen capture of other apps.

## 13. Phases

One plan, `docs/superpowers/plans/2026-10-18-control-channel.md` (plan CC), executed after 4d and before 4e, in this order:

1. Carried follow-ups from plan 4d (allocation-free automation-queue collapse with ≥ 20 ramping lanes on one plugin; a condition wait instead of a fixed sleep in a transport test).
2. `core-control`: protocol, errors, parameters, element IDs, keys.
3. Validation of every command, the registry, wait conditions.
4. The socket server and client.
5. The FFI surface.
6. Swift: command handlers, state rows, opening without dialogs, launch flag.
7. Swift: accessibility tree, element IDs, mouse and key events (and the canvases' mirrored element frames).
8. Swift: window snapshots.
9. Swift: the channel in the app (pump, windows, deferred answers, alerts).
10. `vizctl`.
11. Damaged-file fuzzing (and the model limits it calls for) and the end-to-end test in CI.
12. README: the channel, `vizctl`, the process change, checklist steps.
