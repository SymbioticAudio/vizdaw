# VizDAW 3c — Plugin App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The SwiftUI app hosts AU and CLAP plugins end to end: every open project loads its plugins on the main thread and plays them through the parallel mixer graph; a mixer window edits strips, inserts, sends, routing and shows meters at 30 Hz; a plugin browser adds effects and instruments; plugin windows show each plugin's own editor (CLAP `gui`, AU Cocoa UI / view controller / generic view) or its parameters; a Plugin Manager scans, enables, disables and clears blocklist entries; and autosave, a crash marker and safe mode recover from a plugin that crashes VizDAW.

**Architecture:** `core-ffi` gains one `PluginHost` per session, kept on the thread that made the session (the app's main thread) in a thread-local registry (Plan Ruling 1); the process-wide engine host takes the playing session's plugin host as its insert factory (Ruling 2) and runs with the spec's worker default (Ruling 5). `Session::idle` is spec §5's `host_idle`: a 60 Hz main-thread step that runs `PluginHost::service` for the playing session (`sync` + `idle` for the others), commits plugin-UI gestures as undo steps and autosaves. New FFI surfaces: the mixer read model and its commands (`mixer.rs`), parameters, a `PluginCatalog` object (catalog, scans in the background, Plugin Manager actions), editors (`editor.rs`, the one `unsafe` block of core-ffi) and recovery (`recovery.rs`: crash markers, `recovery_check`, autosave timing). `core-host` gains held slots (safe mode), the list of plugins loading (crash marker), a gesture timeout and per-host catalog marks; `platform-macos` gains AU editors and AU transport through host callbacks. The Swift side adds `@Observable` models in `VizDAWAppModel` (`AppPlugins`, `PluginBrowserModel`, `MixerModel`, `PluginWindowModel`, `PluginManagerModel`, `Recovery`) tested with fakes, and thin views in `VizDAW`.

**Tech Stack:** Rust stable (1.99.1 installed; workspace `rust-version = "1.85"`), edition 2024; UniFFI `=0.32.2`; new direct dependencies only from crates already in `Cargo.lock`: `objc2 0.6` and more `objc2-core-foundation 0.3.2` features (`CFBundle`, `CFURL`, `CFRunLoop`, `CFDate`, `CFCGTypes`, `objc2`) in `platform-macos`, `core-host`, `serde 1` and `serde_json 1` in `core-ffi`, `test-plugins` as a `core-ffi` dev-dependency. Swift 6.4 from the Command Line Tools, SwiftPM tools-version 6.0 (`--build-system native`), SwiftUI + AppKit on macOS 14+, Observation, Swift Testing.

**Spec:** `docs/superpowers/specs/2026-10-16-plugin-hosting-design.md` — this plan implements slice **3c** of its §14: §9 (Plugin Manager, plugin browser, plugin windows, placeholders), §10 (autosave, crash marker, safe mode), §11 (FFI and app), §13's App tests, the app-facing parts of §5 (`host_idle()` from a main-thread timer, AU editors deferred by 3b Ruling 7, AU transport deferred by the 3b final review) and §7 (worker default, meters and load polled at 30 Hz). Binding alongside it: the 3a Plan Rulings (`docs/superpowers/plans/2026-10-16-mixer-graph.md`), the 3b Plan Rulings (`docs/superpowers/plans/2026-10-16-plugin-hosts.md`) and the app-shell plan's Global Constraints and Rulings (`docs/superpowers/plans/2026-10-15-app-shell.md`, notably no SwiftUI `@State`, `VizError.message`, `scripts/swift.sh`). Executors read the 3c slice of the spec, this plan's Plan Rulings and `app/README.md`.

## Global Constraints

- Every shell command runs after `export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools` (the Xcode license is not accepted, so `git`, `cc`, `swift` and the linker work only with the Command Line Tools). Every bash block below starts with that line and `cd /Users/lattawb/workspace/vizdaw`.
- Work on branch `sp3-plugins` (already checked out, at `6456646` before Task 1). Commit at the end of every task, adding only the files the task lists plus `Cargo.lock`; never push. End every commit message with the attribution line(s) your harness requires (the blocks below show `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`).
- Rust stable, edition 2024; every crate manifest keeps `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`. License GPL-3.0-only.
- `core-model`, `core-engine` and `core-host` stay free of OS-specific dependencies and build and test on Linux; `core-ffi` is platform-neutral except `#[cfg(target_os = "macos")]` code (HAL, AU format, AU discovery) and its tests gate macOS-only cases. `platform-macos` code and dependencies stay under `cfg(target_os = "macos")`.
- `core-model`, `core-engine`, `core-host`, `platform-macos` and `core-ffi` keep `#![deny(missing_docs)]` and `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic))]`. Tests may `unwrap` and index.
- `unsafe` lives only in the files 3a/3b allowed (`core-engine/src/graph/exec.rs`, `core-engine/src/pool.rs`, `core-engine/src/rt_check.rs`, `platform-macos/src/{hal,dls,midi,rt}.rs`, `core-host/src/clap/ffi.rs`, `platform-macos/src/au/ffi.rs`, the crasher in `test-plugins/src/plugin.rs`) plus, new in 3c, `core-ffi/src/editor.rs` (the parent-view address, Ruling 20) and the test `platform-macos/tests/au_editor.rs` (AppKit calls). Every `unsafe` block and `unsafe impl` has a `// SAFETY:` comment.
- Everything reachable from the render callback and the workers stays `// RT`: no allocation, lock, I/O or logging. The AU host callbacks (Task 2) read and write atomics only.
- No panic crosses the FFI: every exported function and method returns `Result<_, VizError>` and runs inside `ffi_guard` (2c).
- Plugin hosts are main-thread only (3b Ruling 1): every `core-ffi` call that touches plugins runs on the thread that created the session; Swift calls the FFI only from the main actor. Engine before plugin host: a session's plugin host is dropped only after the engine gave back its processors (Ruling 2).
- Swift: the generated bindings compile in Swift 5 mode; `VizDAWAppModel`, `VizDAW` and the tests in Swift 6 mode. Never use SwiftUI's `@State` (the CLT SDK cannot expand it); keep view state in `@Observable` models. Models live in `VizDAWAppModel` and are tested with fakes (`FakeSession`, `FakeCatalog`); views live in `VizDAW`.
- Generated files are never committed: `app/Sources/VizDAWCore/vizdaw_ffi.swift`, `app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`. Run `scripts/build-ffi.sh` before any `scripts/swift.sh` command.
- Never touch the user's real files from automation: tests and rehearsals use temporary folders (`core-ffi`'s test builds default the support folder to a temporary one, `--smoke` uses a temporary one). Never run `vizdaw-play --scan` (or a scan of the default catalog path `~/Library/Application Support/VizDAW/plugins.json`) in automated steps.
- Automated tests use only Apple's built-in AUs (`aufx/dely/appl`, `aufx/mrev/appl`, `aumu/dls /appl`) and the in-repo CLAP test plugins; third-party plugins appear only in the manual checklist (Task 14). Automated steps never play audible audio for more than 3 s; use `VIZDAW_AUDIO=offline` for `--smoke`.
- Each task ends green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and from Task 4 on also `scripts/build-ffi.sh`, `scripts/swift.sh build`, `scripts/swift.sh test`. Run `cargo fmt --all` first.
- Code in the steps is complete: new files are written whole with `cat > … <<'VIZDAW_EOF'`; changes to existing files are `git apply` patches against the tree the previous task left. If a patch does not apply (a fix landed on the branch meanwhile), make the same change by hand and keep going.

## Review Focus

The five failure modes a user is most likely to hit that the spec implies but no single requirement spells out, each pinned by a named test in its owning task:

1. **Two documents with plugins taking turns at playing** — their inserts usually have the same instance IDs (each document counts from 0); one document must never play the other's plugin instance, and closing either must not strand processors in the engine. Pinned: Task 4 `another_documents_plugins_never_play_for_same_numbered_slots`, `another_session_takes_the_engine_and_the_first_keeps_its_plugins`, `closing_a_session_releases_its_plugins`.
2. **Undo right after touching a plugin** — a knob turned in the plugin's own window is the step Undo reverts even when the 60 Hz timer has not run yet, and a plugin gesture during a fader drag does not merge into the fader's undo step. Pinned: Task 6 `an_undo_before_the_next_tick_undoes_the_gesture`, `gestures_wait_while_a_drag_is_open`, `a_gesture_in_the_plugin_becomes_an_undo_step`.
3. **Removing a plugin, then Undo or Redo** — it comes back with the settings it had (presets, wavetables: non-parameter state), and saving keeps what the plugin holds. Pinned: Task 5 `a_removed_plugin_keeps_its_state_for_undo_and_redo`; Task 9 `saving_captures_plugin_state`.
4. **The audio device changes rate or disappears while plugins play** — the plugins are re-activated for the new rate before they play again and the new engine gets them. Pinned: Task 4 `a_sample_rate_change_reactivates_the_plugins_at_the_new_rate`, `a_device_rebuild_hands_the_plugins_to_the_new_engine`.
5. **VizDAW crashes while a plugin loads, or the catalog file is damaged** — reopening offers safe mode naming the plugin; holding slots does not dirty the project; a newer or corrupt `plugins.json` is reported and never overwritten. Pinned: Task 9 `after_a_crash_the_project_opens_with_plugins_off`, `the_marker_names_loading_plugins_and_a_clean_close_removes_it`; Task 7 `a_newer_catalog_is_reported_and_never_overwritten`, `a_corrupt_catalog_is_moved_aside_and_reported`; Task 14 `afterACrashPluginsOffOrCancel`.

## Plan Rulings

Decisions the spec leaves open or the carry-overs ask for, each implemented and tested as stated (format: Ruling — why — cost if wrong):

1. **Ruling:** Each session's `PluginHost` lives in a thread-local registry (`core-ffi/src/plugins.rs`) on the thread that created the session; `plugins::with(id, …)` reaches it, and a session used from another thread sees no plugin host (its slots read "no plugin host on this thread") — why: `PluginHost` is not `Send` (3b Ruling 1), UniFFI objects must be `Send + Sync`, and core-ffi may not fake `Send` with `unsafe`; Swift calls the FFI only from the main actor — cost if wrong: a caller off the main thread silently gets placeholders.
2. **Ruling:** One engine, one factory: the engine's insert factory is the plugin host of the session that played last (`Host::attach`). When a session plays after another session's plugins were in the engine (`plugins_in_engine`), the engine host is rebuilt first; when the session whose host is the factory closes with plugins loaded, the engine is dropped before its plugin host; a closed session's host waits up to 5 s in a closing list for processors still in the engine's retire ring — why: the engine reuses inserts and plugin-instrument targets by `PluginInstanceId` (3a Ruling 15, 3b Ruling 4), and IDs are per document, so two documents' slot 0 would share an instance; processors must go back to a live host (3b) — cost if wrong: switching playback between two documents that both have plugins reopens the device (a short gap).
3. **Ruling:** New plugin hosts activate at the last engine's rate and block size, else 48 kHz and 4096 frames; `Host::attach` reconfigures (re-activates) a session's plugins when the engine's rate differs or its block is larger — on Play and after a device rebuild at a new rate (the 3b carry-over "call `reconfigure` on a sample-rate change") — why: the engine host is created lazily on Play (2c) while plugins must load when a project opens (§9 windows, §10 marker) — cost if wrong: one re-activation per session on the first Play at a non-48 kHz device.
4. **Ruling:** AU re-activation (a restart, a latency change, a larger block, a new rate) keeps initializing on the main thread — why: 3b Ruling 2 moved only loads off the main thread; re-activations are rare and moving them needs a loader-thread protocol through the engine's restart path — cost if wrong: a heavy AU v2 instrument stalls the UI for its initialize time on those events.
5. **Ruling:** The live engine uses `platform_macos::rt::default_workers()` (spec §7: `min(performance cores − 1, 7)`), deadline 0.8, `MacRtPlatform`, and hands the workers the device's workgroup and period at open and after a buffer-size change; the offline backend keeps 0 workers. There is no worker setting in the UI yet — why: 3a Ruling 20 left the app's default to 3c; a settings window is later work — cost if wrong: one line in `host::live_config`.
6. **Ruling:** `Session::idle()` is spec §5's `host_idle`: the app calls it for every open document from one 60 Hz main-thread timer in the common run-loop modes. For the playing session it runs `PluginHost::service` with the engine (3b: the entry point that drives the engine and holds the refusal guard); for the others `sync` then `idle`. The scan progress is read at 4 Hz from the same timer; the mixer window polls levels at 30 Hz. `IdleReport.params_changed` refreshes plugin windows, `state_dirty` marks the project unsaved (Ruling 11), `IdleReport.reload` is not used by the app — why: spec §5, §7; 3b final-review notes — cost if wrong: a per-document cost of O(slots) per tick.
7. **Ruling:** Plugin-UI gestures are committed as `SetPluginParams` undo steps at the next timer tick (then `sync`, as 3b requires before any undo); while a drag (`begin_edit`…`end_edit`) is open they wait and are committed after it; Undo and Redo first run one timer step so a gesture finished since the last tick is the step they act on; an open gesture closes after 10 s without events (`GESTURE_TIMEOUT`, its `End` was lost) or when its editor window closes (`close_editor` → `PluginHost::end_gestures`) — why: the 3b carry-overs — cost if wrong: a user holding a plugin control still for more than 10 s gets two undo steps.
8. **Ruling:** Before any history change that removes plugin slots (an edit, an undo, a redo), the FFI computes the result's slot set (a dry-run `apply`, `History::peek_undo` / `peek_redo`) and captures every running instance that would vanish; `Report.removed` is not used (it misses plugins of removed `InstrumentOutput` strips) — why: spec §4 "capture before an instance is removed"; 3a/3b carry-overs — cost if wrong: every edit is applied twice (the dry run), which costs little next to the engine recompile.
9. **Ruling:** Save, Save As and autosave capture every running plugin first (`PluginHost::capture_all`); after an undo past a capture the running plugin gets the older state (3b Ruling 15) and the next save captures again — the 3a carry-over "recapture after undo" — why: spec §4 — cost if wrong: a save stores every plugin's state even when unchanged (identical blobs share a file, 3a Ruling 8).
10. **Ruling:** Save, Save As and autosave of every session run one at a time under a process-wide lock (and each session's own lock) — why: the 3a carry-over (blob pruning reads the autosaves) — cost if wrong: none.
11. **Ruling:** A project with a path and unsaved changes (its document differs from the last save, or a plugin marked its own state dirty) is autosaved every 120 s and at the first tick after a plugin finished loading; never inside a drag. Untitled projects are not autosaved. A restored autosave opens as Edited; a plugin marking its state dirty also shows Edited — why: spec §10 leaves untitled projects and plugin-only changes open — cost if wrong: a crash loses an untitled project's work.
12. **Ruling:** The crash marker is `<support>/open-projects/<FNV-1a of the canonical project path>.json` naming every plugin loading at that moment; it is rewritten whenever that set changes — after every edit, undo or redo and at every timer tick, before a load can run plugin code on the main thread — and removed on a clean close (the window closes, `applicationWillTerminate`, or the session drops). `recovery_check` reports a crash when a marker exists and no session in this process has the project open — why: spec §10 names the mechanism but not its keys; loads run concurrently — cost if wrong: a project open in two VizDAW processes at once looks crashed to the second.
13. **Ruling:** Which plugin windows are open is saved in the package as `ui.json` (not `project.json`; no undo step) and offered again on open for slots that still exist — why: spec §9 "stored per instance"; window state is not document content — cost if wrong: copying `project.json` alone loses the window list.
14. **Ruling:** Safe mode holds slots in the plugin host (`PluginHost::hold`, Task 1): held slots are placeholders "Plugins off (safe mode)" that keep their saved state; a slot's Load and a strip's Load All release them; a hold survives the slot's removal and undo; slots added later load normally; holding does not dirty the project — why: §10's "Plugins Off" is per slot, the catalog's unavailable list is per plugin — cost if wrong: none.
15. **Ruling:** Enabling a plugin in the Plugin Manager loads its waiting slots in every open document at once (no "load the waiting slots?" question); disabling a plugin does not stop running instances (they become placeholders the next time the project opens) — why: spec §9 says "offers to load"; per-slot Load exists for the cautious path — cost if wrong: a plugin enabled by mistake loads immediately.
16. **Ruling:** The catalog is one per process (`PluginCatalog`, opened at launch from the support folder); each plugin host keeps its own `HostMarks` (`Catalog::apply_to`) — why: 3b's `Catalog::apply` kept one set of marks per catalog, so with two hosts a re-enabled plugin stayed unavailable in the second — cost if wrong: none.
17. **Ruling:** A catalog file the app cannot read (newer, corrupt, unreadable) and a failed catalog save are reported once as an alert; the app goes on with an empty catalog and, through 3b's `Catalog::save`, never writes over a file it could not read — why: the 3b carry-over "surface the newer-version / corrupt-file errors and never save over them" — cost if wrong: none.
18. **Ruling:** Scans run on a background thread with up to 4 `vizdaw-scan` processes, record each bundle as it finishes and save at the end; the app polls progress. At launch only new or changed bundles are scanned; "Rescan All" scans every bundle found; AU v3 components get 30 s. `--smoke` and `--no-scan` runs never scan — why: spec §8 — cost if wrong: a first scan of hundreds of plugins takes minutes (progress shows).
19. **Ruling:** AU editors (Task 3): the unit's Cocoa view (`kAudioUnitProperty_CocoaUI`, AU v2), else its view controller's view (`kAudioUnitProperty_RequestViewController`, AU v3; waited for up to 3 s while the main run loop runs), else CoreAudioKit's `AUGenericView`; fixed size; opened only on the main thread; the view leaves its window before the unit is disposed — why: 3b Ruling 7 deferred AU editors here; the C API reaches v3 views through the request property; the generic view gives every AU a window — cost if wrong: a v3 unit slower than 3 s shows the generic view.
20. **Ruling:** A plugin window is an AppKit `NSWindow` per instance; the app owns the `NSView` the editor goes into and passes its address to `open_editor` (`core-ffi/src/editor.rs`), keeping it alive until `close_editor`; title "‹strip› — ‹plugin›", a bypass header; plugins without an editor (the in-repo test plugins among them) show their parameters as sliders, a drag being one undo step — why: spec §9; spec §11's "editor open … returning an NSView pointer" is inverted because CLAP's `gui` embeds into a parent the host owns — cost if wrong: none.
21. **Ruling:** `scripts/build-ffi.sh` also builds `vizdaw-scan` into `target/ffi/`, `scripts/make-app.sh` copies it into `VizDAW.app/Contents/MacOS/`, `scripts/swift.sh` exports `VIZDAW_SCANNER` for `swift run`; `VIZDAW_SUPPORT_DIR` moves the support folder; `core-ffi` test builds default it to a temporary folder and `--smoke` uses a temporary one — why: automation must never touch the user's catalog — cost if wrong: none.
22. **Ruling:** Muting a strip also silences its pre-fader sends, as it silences its sidechain feed (3a's behaviour, kept as the product decision and already pinned by `core-engine`'s `graph_mixer::mute_and_solo_become_gains`) — why: a muted strip goes silent everywhere, as in Studio Vision — cost if wrong: a pre-fader cue mix of a muted strip needs the strip unmuted and its fader down.
23. **Ruling:** The built-in synth plays 3 dB quieter than in 2c (constant-power pan through its strip, spec ruling, 3a final review); `app/README.md` says so — cost if wrong: none.
24. **Ruling:** AU transport (Task 2): `kAudioUnitProperty_HostCallbacks` with beat-and-tempo, musical-time (4/4, 3b Ruling 11) and transport-state callbacks that read atomics the processor stores from each block's `BlockInfo`; "changed" is reported on start, stop and jumps; no loop information — why: the 3b final review deferred spec §5's transport for AU to 3c — cost if wrong: AUs see 4/4 and no cycle.
25. **Ruling:** Fader, pan and send-level drags are History transactions (`begin_edit`/`end_edit`); every move recompiles the engine and is heard at once (no smoothing, 3a Ruling 16) — cost if wrong: zipper noise on fast moves.
26. **Ruling:** The compile report the fault popover shows also lists the mixer graph's placeholders ("pluginN: why"), warnings and a refused graph; instances the engine bypassed for faults are sticky for that engine (3a Ruling 23) and show red in their slot; removing and re-adding the plugin brings it back — cost if wrong: none.
27. **Ruling:** 3b's whole-`Document` comparison per tick while a load is refused stays; a generation counter waits for profiling — cost if wrong: O(document) per tick only in the refused state.
28. **Ruling:** The mixer's Instruments menu offers "New Plugin Instrument…" (a destination named after the plugin, no name prompt) and, for each existing destination, "→ Plugin…", which makes it play a plugin instrument (`EditDestination`) so the lanes already using it play the plugin — why: spec §9 names only "New Plugin Instrument…", but the app has no lane-routing UI yet, so a new destination alone could not be heard — cost if wrong: a menu item.

## Spec gaps and contradictions found while planning

- §6/§11: the engine reuses instances and targets by `PluginInstanceId`, but IDs are per document while the engine is shared by every open document (Ruling 2).
- §5: "`host_idle()` … from a main-thread timer" — with one plugin host per document it is a per-session `idle` driven by one app timer (Ruling 6).
- §11: "editor open/close returning an NSView pointer to Swift" — the host owns the parent view instead (Ruling 20).
- §9: "enabling it there offers to load the waiting slots" (Ruling 15); "Windows reopen … (stored per instance)" does not say where (Ruling 13).
- §10: untitled projects and plugin-only state changes (Ruling 11); several plugins load at once while the marker names "the plugin being loaded" (Ruling 12); safe mode versus slots added or undone later (Ruling 14).
- §7: workers are a "setting 0–15" with no settings UI in any slice (Ruling 5).
- §4/§12: mute versus pre-fader sends is unspecified (Ruling 22).
- Carry-overs settled here: 3a — worker default (Ruling 5), save/autosave serialization (Ruling 10), recapture after undo (Ruling 9), `Report.removed` gap (Ruling 8), mute and pre-fader sends (Ruling 22), the 3 dB quieter built-in synth (Ruling 23). 3b — `sync` after `commit_gestures` before undo (Ruling 7, Task 6), gesture timeout (Ruling 7, Task 1), `reconfigure` on a rate change (Ruling 3, Task 4), capture before removal (Ruling 8, Task 5), catalog errors surfaced (Ruling 17, Task 7), `params_changed` / `state_dirty` (Rulings 6 and 11), AU transport (Ruling 24, Task 2), AU re-activation on the main thread (Ruling 4), `service` as the engine-driving entry point at a steady main-thread tick (Ruling 6, Task 4), AU editors (Ruling 19, Task 3), per-host catalog marks (Ruling 16, Task 1).

## File Structure

```
crates/core-model/
  src/history.rs              peek_undo / peek_redo (Task 5)
  src/file.rs                 load_autosave (Task 9)
  tests/{history,plugin_state}.rs   + one test each
crates/core-host/
  src/host.rs                 SlotState, state(), loading(), hold()/held(), GESTURE_TIMEOUT,
                              set_gesture_timeout(), end_gestures() (Task 1)
  src/catalog.rs              HostMarks, Catalog::apply_to (Task 1)
  src/instance.rs             ParentView::as_ptr (Task 3)
  tests/{host,catalog}.rs     + app-hook tests (Task 1)
crates/platform-macos/
  Cargo.toml                  objc2, CF features, [[test]] au_editor harness = false (Task 3)
  src/au/ffi.rs               HostTransport + host callbacks (Task 2); View, editor views (Task 3)
  src/au/mod.rs               transport in Opened/AuInstance/AuProcessor, host_callbacks_after (Task 2);
                              AuEditor, V3_VIEW_WAIT (Task 3)
  tests/au_transport.rs       NEW (Task 2)
  tests/au_editor.rs          NEW, own main on the main thread (Task 3)
crates/core-ffi/
  Cargo.toml                  core-host, serde, serde_json, test-plugins (Tasks 4, 8)
  src/plugins.rs              NEW registry of plugin hosts, slot rows (Task 4)
  src/catalog.rs              NEW process catalog (Task 4); PluginCatalog object (Task 7)
  src/host.rs                 factory, attach/reconfigure, live config, service, levels (Tasks 4, 5, 7)
  src/session.rs              plugins in sessions, idle (4); mixer edits, capture (5); params, gestures (6);
                              editors, ui.json (8); autosave, markers, safe mode, open_with (9)
  src/snapshot.rs             PluginFormatTag, PluginRefRow, SlotStatus, SlotRow, IdleUpdate (4);
                              ParamRow, ParamValue (6); has_editor (8)
  src/mixer.rs                NEW mixer read model (Task 5)
  src/editor.rs               NEW parent view, EditorSize (Task 8)
  src/recovery.rs             NEW markers, recovery_check, support folder, autosave interval (Task 9)
  tests/{plugins,mixer,params,catalog,editors,recovery}.rs   NEW
app/
  Package.swift               AppKit, CoreAudioKit (Task 8)
  Sources/VizDAWAppModel/     DocumentModel.idle (4); AppPlugins, PluginBrowser (10); MixerModel,
                              DocumentStore.mixer (11); PluginWindowModel (12); PluginManagerModel (13);
                              Recovery, DocumentStore.recovery/closeAll (14)
  Sources/VizDAW/             PluginTicker (4); AppSetup, PluginBrowserView (10); MixerView, Window menu (11);
                              PluginWindows (12); PluginManagerView (13); RecoveryAlerts (14)
  Tests/VizDAWAppModelTests/  FakeSession grows with each FFI task; FakeCatalog; new suites per task
  README.md                   plugins section and checklist steps 9–20 (Task 14)
scripts/{build-ffi,swift,make-app}.sh   the scanner next to the app (Task 10)
```

---

### Task 1: core-host hooks for the app — held slots, the loading list, lost gesture ends, per-host catalog marks


**Files:**
- Modify: `crates/core-host/src/host.rs`
- Modify: `crates/core-host/src/catalog.rs`
- Modify: `crates/core-host/src/lib.rs`
- Test: `crates/core-host/tests/host.rs` (four tests appended), `crates/core-host/tests/catalog.rs` (one test appended)

**Interfaces:**
- Consumes: 3b's `PluginHost` (its private `Slot`, `State`, `Live`, `collect_gestures`, `flush_gesture`), `Catalog::apply`.
- Produces: `core_host::SlotState::{Loading, Live, Failed(String)}`; `PluginHost::state(&self, PluginInstanceId) -> Option<SlotState>`; `PluginHost::loading(&self) -> Vec<(PluginInstanceId, PluginRef)>`; `PluginHost::hold(&mut self, PluginInstanceId, Option<String>)` and `held(&self) -> Vec<PluginInstanceId>`; `core_host::GESTURE_TIMEOUT: Duration` (10 s), `PluginHost::set_gesture_timeout(&mut self, Duration)`, `PluginHost::end_gestures(&mut self, PluginInstanceId)`; `core_host::catalog::HostMarks` (`Default`) and `Catalog::apply_to(&self, &mut PluginHost, &mut HostMarks)`.

Rulings 7, 14 and 16. A held slot is a placeholder showing its reason and never starts loading; releasing it (`hold(id, None)`) turns its failure into the empty "load at the next `sync`" state. `loading()` lists the slots in `State::Loading` (loader thread or waiting for the main-thread step). A gesture whose `End` was lost closes when no event came for `gesture_timeout`; `end_gestures` drains the queue and closes what is open, reported by the next `idle`/`service`. `apply` keeps its behaviour through `apply_to` with the catalog's own marks.

- [ ] **Step 1: Write the failing tests**

Change `crates/core-host/tests/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/tests/host.rs b/crates/core-host/tests/host.rs
index 34c1b25..d712ebf 100644
--- a/crates/core-host/tests/host.rs
+++ b/crates/core-host/tests/host.rs
@@ -877,3 +877,87 @@ fn a_base_value_that_did_not_fit_the_queue_is_sent_again() {
     assert_eq!(script.held(GAIN), 1.0, "the base was sent again");
     drop(engine);
 }
+
+// ---------------------------------------------------------------------------
+// What the app needs (plan 3c): held slots (safe mode), the plugins loading
+// now (crash marker), gestures whose End was lost, closing an editor.
+
+#[test]
+fn held_slots_are_placeholders_until_released() {
+    let (hist, song, gain) = with_gain();
+    let mut host = host(4096);
+    host.hold(gain, Some("Plugins off".into()));
+    assert!(host.sync(hist.doc()), "a held slot does not load");
+    assert_eq!(
+        host.state(gain),
+        Some(core_host::SlotState::Failed("Plugins off".into()))
+    );
+    assert_eq!(host.held(), vec![gain]);
+    let (mut engine, _be) = engine(&host, 4096);
+    let report = engine.load(hist.doc(), song).unwrap();
+    assert_eq!(
+        report.graph.placeholders,
+        vec![(gain, "Plugins off".to_string())]
+    );
+    host.hold(gain, None);
+    assert!(host.held().is_empty());
+    assert!(!host.sync(hist.doc()), "released: it loads");
+    assert_eq!(host.state(gain), Some(core_host::SlotState::Loading));
+    let r = host.wait_ready(WAIT);
+    assert_eq!(r.loaded, vec![gain], "{:?}", r.failed);
+    assert_eq!(host.state(gain), Some(core_host::SlotState::Live));
+    drop(engine);
+}
+
+#[test]
+fn loading_names_the_plugins_in_flight() {
+    let (hist, _song, gain) = with_gain();
+    let mut host = host(4096);
+    assert!(host.loading().is_empty());
+    host.sync(hist.doc());
+    let names: Vec<(PluginInstanceId, String)> = host
+        .loading()
+        .into_iter()
+        .map(|(id, p)| (id, p.id))
+        .collect();
+    assert_eq!(names, vec![(gain, "vizdaw.test.gain".to_string())]);
+    host.wait_ready(WAIT);
+    assert!(host.loading().is_empty());
+    assert_eq!(host.state(PluginInstanceId(999)), None);
+}
+
+#[test]
+fn a_gesture_whose_end_was_lost_closes_after_the_timeout() {
+    let (hist, song, id, mut host, script) = with_fake();
+    host.set_gesture_timeout(Duration::from_millis(50));
+    let (mut engine, mut be) = engine(&host, 4096);
+    engine.load(hist.doc(), song).unwrap();
+    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.5)]);
+    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
+    assert!(r.gestures.is_empty(), "still open");
+    std::thread::sleep(Duration::from_millis(80));
+    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
+    assert_eq!(r.gestures.len(), 1, "closed by the timeout");
+    assert_eq!(r.gestures[0].slot, id);
+    assert_eq!(r.gestures[0].values, vec![(GAIN, 0.5)]);
+    drop(engine);
+}
+
+#[test]
+fn closing_the_editor_ends_its_gesture() {
+    let (hist, song, id, mut host, script) = with_fake();
+    let (mut engine, mut be) = engine(&host, 4096);
+    engine.load(hist.doc(), song).unwrap();
+    script.emit(&[ParamEvent::Begin(GAIN), ParamEvent::Value(GAIN, 0.25)]);
+    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
+    assert!(r.gestures.is_empty());
+    host.end_gestures(id);
+    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
+    assert_eq!(r.gestures.len(), 1);
+    assert_eq!(r.gestures[0].values, vec![(GAIN, 0.25)]);
+    // A later End of the closed gesture opens nothing.
+    script.emit(&[ParamEvent::End(GAIN)]);
+    let r = tick(&mut host, &mut engine, &mut be, &hist, song);
+    assert!(r.gestures.is_empty());
+    drop(engine);
+}
VIZDAW_EOF
```
Change `crates/core-host/tests/catalog.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/tests/catalog.rs b/crates/core-host/tests/catalog.rs
index 7074224..a15d248 100644
--- a/crates/core-host/tests/catalog.rs
+++ b/crates/core-host/tests/catalog.rs
@@ -268,3 +268,44 @@ fn save_refuses_to_overwrite_a_file_it_cannot_read() {
     c.save(&path).unwrap();
     assert_eq!(Catalog::load(&path).unwrap().entries(), c.entries());
 }
+
+#[test]
+fn a_catalog_applied_to_two_hosts_clears_each_ones_marks() {
+    use core_host::catalog::HostMarks;
+    let bundle = test_plugins::bundle();
+    let src = Source::clap_bundle(&bundle);
+    let mut c = Catalog::new();
+    c.record(
+        &src,
+        vec![entry(
+            &src,
+            "vizdaw.test.gain",
+            PluginKind::Effect,
+            ScanStatus::Ok,
+        )],
+    );
+    c.set_enabled(PluginFormat::Clap, "vizdaw.test.gain", false);
+    let mut b = core_engine::fixtures::Builder::new();
+    let (_, gain) = b.plugin_dest("G", plugin("vizdaw.test.gain")).unwrap();
+    let mut hosts: Vec<(PluginHost, HostMarks)> = (0..2)
+        .map(|_| {
+            (
+                PluginHost::new(48_000.0, 512).unwrap(),
+                HostMarks::default(),
+            )
+        })
+        .collect();
+    for (h, m) in &mut hosts {
+        c.apply_to(h, m);
+        h.sync(b.doc());
+        h.wait_ready(std::time::Duration::from_secs(10));
+        assert_eq!(h.failure(gain), Some("Disabled in Plugin Manager"));
+    }
+    c.set_enabled(PluginFormat::Clap, "vizdaw.test.gain", true);
+    for (h, m) in &mut hosts {
+        c.apply_to(h, m);
+        h.sync(b.doc());
+        h.wait_ready(std::time::Duration::from_secs(10));
+        assert!(h.instance(gain).is_some(), "{:?}", h.failure(gain));
+    }
+}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test host --test catalog
```

Expected: compile errors — no method `hold`, `state`, `held`, `loading`, `set_gesture_timeout`, `end_gestures` on `PluginHost`; unresolved `core_host::catalog::HostMarks`.

- [ ] **Step 3: Implement**

Change `crates/core-host/src/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/host.rs b/crates/core-host/src/host.rs
index 40296ab..3bfe21d 100644
--- a/crates/core-host/src/host.rs
+++ b/crates/core-host/src/host.rs
@@ -83,6 +83,22 @@ impl IdleReport {
     }
 }
 
+/// Where a slot stands (for the mixer and the Plugin Manager).
+#[derive(Debug, Clone, PartialEq, Eq)]
+pub enum SlotState {
+    /// Loading (or waiting for the next `sync` to load it).
+    Loading,
+    /// Running.
+    Live,
+    /// A placeholder, and why: it failed to load, is unavailable, or is
+    /// held ([`PluginHost::hold`]).
+    Failed(String),
+}
+
+/// How long an open gesture may go without an event from the plugin before
+/// the host closes it (its `End` was lost).
+pub const GESTURE_TIMEOUT: Duration = Duration::from_secs(10);
+
 /// What the document wants of a slot.
 #[derive(Clone)]
 struct Want {
@@ -147,6 +163,8 @@ struct Live {
     gesture: Vec<(ParamId, f64)>,
     /// Parameters whose gesture is open.
     open: BTreeSet<ParamId>,
+    /// When the plugin last reported a parameter event.
+    last_event: Option<Instant>,
 }
 
 /// A removed instance whose processor is still out.
@@ -229,6 +247,12 @@ pub struct PluginHost {
     /// main-thread step.
     finishing: VecDeque<(PluginInstanceId, u64, Prepared)>,
     generation: u64,
+    /// Slots kept from loading (safe mode), with the reason they show.
+    held: BTreeMap<PluginInstanceId, String>,
+    /// Gestures closed by [`PluginHost::end_gestures`], reported by the
+    /// next `idle` or `service`.
+    ended: Vec<Gesture>,
+    gesture_timeout: Duration,
     shelf: Shelf,
     back_tx: Sender<Returned>,
     back_rx: Receiver<Returned>,
@@ -280,6 +304,9 @@ impl PluginHost {
             parked: BTreeSet::new(),
             finishing: VecDeque::new(),
             generation: 0,
+            held: BTreeMap::new(),
+            ended: Vec::new(),
+            gesture_timeout: GESTURE_TIMEOUT,
             shelf: Shelf::new(),
             back_tx,
             back_rx,
@@ -371,6 +398,70 @@ impl PluginHost {
         }
     }
 
+    /// Where slot `id` stands, if the document has it.
+    pub fn state(&self, id: PluginInstanceId) -> Option<SlotState> {
+        Some(match &self.slots.get(&id)?.state {
+            State::Loading(_) => SlotState::Loading,
+            State::Live(_) => SlotState::Live,
+            State::Failed(w) if w.is_empty() => SlotState::Loading,
+            State::Failed(w) => SlotState::Failed(w.clone()),
+        })
+    }
+
+    /// The plugins loading now (the crash marker names them, spec §10).
+    pub fn loading(&self) -> Vec<(PluginInstanceId, PluginRef)> {
+        self.slots
+            .iter()
+            .filter(|(_, s)| matches!(s.state, State::Loading(_)))
+            .map(|(id, s)| (*id, s.want.plugin.clone()))
+            .collect()
+    }
+
+    /// Keeps slot `id` from loading (safe mode, spec §10): until released
+    /// it plays as a placeholder showing `why`. `None` releases it; it
+    /// loads at the next `sync`. A slot already running is not stopped.
+    pub fn hold(&mut self, id: PluginInstanceId, why: Option<String>) {
+        match why {
+            Some(w) => {
+                self.held.insert(id, w);
+            }
+            None => {
+                if self.held.remove(&id).is_some() {
+                    if let Some(s) = self.slots.get_mut(&id) {
+                        if matches!(s.state, State::Failed(_)) {
+                            s.state = State::Failed(String::new());
+                        }
+                    }
+                }
+            }
+        }
+    }
+
+    /// The slots kept from loading.
+    pub fn held(&self) -> Vec<PluginInstanceId> {
+        self.held.keys().copied().collect()
+    }
+
+    /// How long an open gesture may go without an event before it is
+    /// closed ([`GESTURE_TIMEOUT`] by default).
+    pub fn set_gesture_timeout(&mut self, timeout: Duration) {
+        self.gesture_timeout = timeout;
+    }
+
+    /// Closes instance `id`'s open gesture (its editor window closed): what
+    /// it changed so far is reported by the next `idle` or `service`.
+    pub fn end_gestures(&mut self, id: PluginInstanceId) {
+        let Some(Slot { want, state }) = self.slots.get_mut(&id) else {
+            return;
+        };
+        let State::Live(live) = state else {
+            return;
+        };
+        collect_gestures(id, live, &want.params, &mut self.ended);
+        live.open.clear();
+        flush_gesture(id, live, &mut self.ended);
+    }
+
     /// Sets a parameter now (a mixer control): sent to the running plugin;
     /// the app also records it in the overlay with `SetPluginParams`. Until
     /// the document has that value (or a newer edit of the parameter),
@@ -459,7 +550,7 @@ impl PluginHost {
                             state: State::Failed(String::new()),
                         },
                     );
-                    match unavailable {
+                    match self.held.get(&id).cloned().or(unavailable) {
                         Some(why) => self.fail(id, why),
                         None => self.start_load(id),
                     }
@@ -486,6 +577,10 @@ impl PluginHost {
         let Some(want) = self.slots.get(&id).map(|s| s.want.clone()) else {
             return;
         };
+        if let Some(why) = self.held.get(&id).cloned() {
+            self.fail(id, why);
+            return;
+        }
         if let Some(why) = self
             .unavailable
             .get(&(want.plugin.format, want.plugin.id.clone()))
@@ -616,6 +711,7 @@ impl PluginHost {
     /// [`PluginHost::idle`]'s work, leaving the owed load to the caller.
     fn tick(&mut self) -> IdleReport {
         let mut r = self.carry.take().unwrap_or_default();
+        r.gestures.append(&mut self.ended);
         while let Ok((id, generation, result)) = self.done.try_recv() {
             if !self.is_loading(id, generation) {
                 continue;
@@ -694,6 +790,7 @@ impl PluginHost {
             pending: BTreeMap::new(),
             gesture: Vec::new(),
             open: BTreeSet::new(),
+            last_event: None,
         });
         // State first, then the overlay (spec §4).
         if let Some((sid, bytes)) = &want.state {
@@ -839,6 +936,14 @@ impl PluginHost {
         };
         let ev = live.inst.idle();
         collect_gestures(id, live, &want.params, &mut r.gestures);
+        let stale = live
+            .last_event
+            .is_some_and(|t| t.elapsed() >= self.gesture_timeout);
+        if !live.open.is_empty() && stale {
+            // Its `End` was lost: what it changed so far is one gesture.
+            live.open.clear();
+            flush_gesture(id, live, &mut r.gestures);
+        }
         if ev.params_changed {
             r.params_changed.push(id);
         }
@@ -1115,6 +1220,9 @@ fn collect_gestures(
 ) {
     let mut events = l.gestures.drain();
     events.extend(l.inst.gestures());
+    if !events.is_empty() {
+        l.last_event = Some(Instant::now());
+    }
     for e in events {
         match e {
             ParamEvent::Begin(p) => {
VIZDAW_EOF
```
Change `crates/core-host/src/catalog.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/catalog.rs b/crates/core-host/src/catalog.rs
index 46d6a73..4112865 100644
--- a/crates/core-host/src/catalog.rs
+++ b/crates/core-host/src/catalog.rs
@@ -152,6 +152,13 @@ struct Repr {
     disabled: Vec<(PluginFormat, String)>,
 }
 
+/// The plugins one host was marked unavailable by
+/// [`Catalog::apply_to`]: each host keeps its own, so a catalog applied to
+/// several hosts (one per open document) clears exactly what it set on
+/// each.
+#[derive(Debug, Clone, Default, PartialEq, Eq)]
+pub struct HostMarks(BTreeSet<(PluginFormat, String)>);
+
 /// The catalog.
 #[derive(Debug, Clone, Default, PartialEq)]
 pub struct Catalog {
@@ -160,7 +167,7 @@ pub struct Catalog {
     /// Sources found missing by the last [`Catalog::mark_found`].
     missing: BTreeSet<Source>,
     /// Plugins the last [`Catalog::apply`] marked unavailable on the host.
-    marked: BTreeSet<(PluginFormat, String)>,
+    marked: HostMarks,
 }
 
 impl Catalog {
@@ -234,7 +241,7 @@ impl Catalog {
             entries: r.entries,
             disabled: r.disabled.into_iter().collect(),
             missing: BTreeSet::new(),
-            marked: BTreeSet::new(),
+            marked: HostMarks::default(),
         })
     }
 
@@ -416,7 +423,16 @@ impl Catalog {
     /// unavailable (their slots load as placeholders with the reason, at
     /// the next load; live instances are not stopped). It clears only
     /// reasons it set itself earlier, never a failure of the host's own.
+    /// For one host; with several, use [`Catalog::apply_to`].
     pub fn apply(&mut self, host: &mut PluginHost) {
+        let mut marks = std::mem::take(&mut self.marked);
+        self.apply_to(host, &mut marks);
+        self.marked = marks;
+    }
+
+    /// [`Catalog::apply`] for one of several hosts: `marks` is what this
+    /// catalog told `host` last time (start with `HostMarks::default()`).
+    pub fn apply_to(&self, host: &mut PluginHost, marks: &mut HostMarks) {
         let mut now = BTreeSet::new();
         for e in &self.entries {
             if e.plugin.id.is_empty() {
@@ -437,9 +453,9 @@ impl Catalog {
                 now.insert(key);
             }
         }
-        for (format, id) in self.marked.difference(&now) {
+        for (format, id) in marks.0.difference(&now) {
             host.set_unavailable(*format, id, None);
         }
-        self.marked = now;
+        marks.0 = now;
     }
 }
VIZDAW_EOF
```
Change `crates/core-host/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/lib.rs b/crates/core-host/src/lib.rs
index 897674c..ddf6353 100644
--- a/crates/core-host/src/lib.rs
+++ b/crates/core-host/src/lib.rs
@@ -32,7 +32,7 @@ pub mod scan;
 pub mod shelf;
 
 pub use error::HostError;
-pub use host::{Gesture, IdleReport, PluginHost};
+pub use host::{GESTURE_TIMEOUT, Gesture, IdleReport, PluginHost, SlotState};
 pub use info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
 pub use instance::{
     AudioProcessor, InstanceEvents, LoadRequest, ParentView, PluginEditor, PluginFormat,
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-host --test host --test catalog
```

Expected: `host` 22 passed (among them `held_slots_are_placeholders_until_released`, `loading_names_the_plugins_in_flight`, `a_gesture_whose_end_was_lost_closes_after_the_timeout`, `closing_the_editor_ends_its_gesture`), `catalog` 8 passed (among them `a_catalog_applied_to_two_hosts_clears_each_ones_marks`).

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: no formatting changes left, no clippy warnings, every test passes (515).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-host/src/host.rs crates/core-host/src/catalog.rs crates/core-host/src/lib.rs \
  crates/core-host/tests/host.rs crates/core-host/tests/catalog.rs
git commit -m "$(cat <<'MSG'
feat(core-host): held slots, loading list, gesture timeout, per-host catalog marks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 2: AU transport through host callbacks


**Files:**
- Modify: `crates/platform-macos/src/au/ffi.rs`
- Modify: `crates/platform-macos/src/au/mod.rs`
- Create: `crates/platform-macos/tests/au_transport.rs`

**Interfaces:**
- Consumes: `core_engine::backend::BlockInfo { playing, frame, beats, tempo_bpm }` (3b Task 3), the AU loader path (`AuFormat::prepare`, `Opened`, `AuInstance`, `AuProcessor`).
- Produces: `AuInstance::host_callbacks_installed(&self) -> bool`; `platform_macos::au::HostCallbacksSeen { beat, tempo, to_next_beat: u32, numerator: f32, denominator: u32, downbeat, playing, changed, sample }`; `platform_macos::au::host_callbacks_after(rate: f64, blocks: &[(BlockInfo, u32)]) -> Vec<HostCallbacksSeen>` (calls the callbacks the way a unit does; Apple's units never ask).

Ruling 24. `ffi.rs` gets `HostTransport` (atomics: beat, tempo, sample, rate, playing, changed) and the four `extern "C-unwind"` callbacks behind `kAudioUnitProperty_HostCallbacks`, installed by `prepare` before `AudioUnitInitialize` (the property survives uninitialize, so re-activations keep it). `AuProcessor::process` stores each block's facts first (`store_block`): a playing block that does not follow the previous one is a jump. All of it is `// RT`: loads and stores of atomics.

- [ ] **Step 1: Write the failing test**

Create `crates/platform-macos/tests/au_transport.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/platform-macos/tests
cat > crates/platform-macos/tests/au_transport.rs <<'VIZDAW_EOF'
#![cfg(target_os = "macos")]
//! AU transport through `kAudioUnitProperty_HostCallbacks` (spec §5): the
//! units take the callbacks, and the callbacks report what the engine's
//! blocks said, the way a tempo-synced unit asks for it.

use core_engine::backend::BlockInfo;
use platform_macos::au;

fn block(playing: bool, frame: u64, beats: f64, tempo_bpm: f64) -> BlockInfo {
    BlockInfo {
        playing,
        frame,
        beats,
        tempo_bpm,
    }
}

#[test]
fn apple_units_take_the_host_callbacks() {
    for id in ["aufx/dely/appl", "aumu/dls /appl"] {
        let u = au::load(id, 48_000.0, 512).unwrap();
        assert!(u.host_callbacks_installed(), "{id}");
    }
}

#[test]
fn the_callbacks_report_tempo_position_and_jumps() {
    let seen = au::host_callbacks_after(
        48_000.0,
        &[
            (block(false, 0, 0.0, 120.0), 512),
            (block(true, 0, 0.0, 120.0), 512),
            (block(true, 512, 512.0 / 24_000.0, 120.0), 512),
            (block(true, 96_000, 6.5, 90.0), 512),
        ],
    );
    assert!(!seen[0].playing && !seen[0].changed, "{:?}", seen[0]);
    assert!(seen[1].playing && seen[1].changed, "start: {:?}", seen[1]);
    assert!(!seen[2].changed, "a following block: {:?}", seen[2]);
    assert_eq!(seen[2].sample, 512.0);
    assert_eq!(seen[2].tempo, 120.0);
    let s = seen[3];
    assert!(s.changed, "a locate is a jump");
    assert_eq!((s.beat, s.tempo, s.sample), (6.5, 90.0, 96_000.0));
    // Half a beat at 90 BPM and 48 kHz is 16 000 frames.
    assert_eq!(s.to_next_beat, 16_000);
    assert_eq!((s.numerator, s.denominator, s.downbeat), (4.0, 4, 4.0));
}
VIZDAW_EOF
```

- [ ] **Step 2: Run it to see it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test au_transport
```

Expected: compile errors — no method `host_callbacks_installed`, no function `host_callbacks_after` in `platform_macos::au`.

- [ ] **Step 3: Implement**

Change `crates/platform-macos/src/au/ffi.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/platform-macos/src/au/ffi.rs b/crates/platform-macos/src/au/ffi.rs
index 5670ad9..5b71515 100644
--- a/crates/platform-macos/src/au/ffi.rs
+++ b/crates/platform-macos/src/au/ffi.rs
@@ -14,8 +14,8 @@ use objc2_audio_toolbox::{
     AudioUnitInitialize, AudioUnitParameter, AudioUnitParameterInfo, AudioUnitParameterOptions,
     AudioUnitParameterUnit, AudioUnitPropertyID, AudioUnitRender, AudioUnitRenderActionFlags,
     AudioUnitReset, AudioUnitScope, AudioUnitSetParameter, AudioUnitSetProperty,
-    AudioUnitUninitialize, MusicDeviceMIDIEvent, kAudioUnitProperty_ClassInfo,
-    kAudioUnitProperty_ElementCount, kAudioUnitProperty_Latency,
+    AudioUnitUninitialize, HostCallbackInfo, MusicDeviceMIDIEvent, kAudioUnitProperty_ClassInfo,
+    kAudioUnitProperty_ElementCount, kAudioUnitProperty_HostCallbacks, kAudioUnitProperty_Latency,
     kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitProperty_ParameterInfo,
     kAudioUnitProperty_ParameterList, kAudioUnitProperty_SetRenderCallback,
     kAudioUnitProperty_StreamFormat, kAudioUnitProperty_TailTime, kAudioUnitScope_Global,
@@ -31,7 +31,7 @@ use objc2_core_foundation::{
 };
 use std::ffi::c_void;
 use std::ptr::{self, NonNull};
-use std::sync::atomic::{AtomicBool, Ordering};
+use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
 use std::sync::mpsc::{Sender, channel};
 use std::sync::{Arc, Mutex, PoisonError};
 use std::time::Duration;
@@ -1016,3 +1016,282 @@ pub(crate) fn ui_gesture(unit: &Unit, id: u32, value: f32) {
         AUEventListenerNotify(ptr::null_mut(), ptr::null_mut(), &end);
     }
 }
+
+// ---------------------------------------------------------------------------
+// Transport for AUs (`kAudioUnitProperty_HostCallbacks`, spec §5): the
+// processor stores each block's facts in atomics before rendering; the
+// unit's calls read them on the render thread. // RT
+
+/// The transport facts one unit's host callbacks report.
+#[derive(Debug, Default)]
+pub(crate) struct HostTransport {
+    /// Quarter-note position (f64 bits).
+    beat: AtomicU64,
+    /// Tempo in BPM (f64 bits).
+    tempo: AtomicU64,
+    /// Schedule frame (f64 bits).
+    sample: AtomicU64,
+    /// Stream rate (f64 bits).
+    rate: AtomicU64,
+    playing: AtomicBool,
+    /// Set when playing toggled or the position jumped; cleared when a
+    /// unit reads it.
+    changed: AtomicBool,
+}
+
+impl HostTransport {
+    /// A transport at 120 BPM, stopped at 0, for `rate`.
+    pub(crate) fn new(rate: f64) -> HostTransport {
+        let t = HostTransport::default();
+        t.tempo.store(120f64.to_bits(), Ordering::Relaxed);
+        t.rate.store(rate.to_bits(), Ordering::Relaxed);
+        t
+    }
+
+    /// The stream rate changed (activation). Main thread.
+    pub(crate) fn set_rate(&self, rate: f64) {
+        self.rate.store(rate.to_bits(), Ordering::Relaxed);
+    }
+
+    /// Stores one block's facts (`expected` is the frame the previous
+    /// block's end predicted: anything else is a jump). // RT
+    pub(crate) fn set(&self, playing: bool, frame: u64, beats: f64, tempo: f64, expected: u64) {
+        let was = self.playing.swap(playing, Ordering::Relaxed);
+        if was != playing || frame != expected {
+            self.changed.store(true, Ordering::Relaxed);
+        }
+        self.beat.store(beats.to_bits(), Ordering::Relaxed);
+        let tempo = if tempo.is_finite() && tempo > 0.0 {
+            tempo
+        } else {
+            120.0
+        };
+        self.tempo.store(tempo.to_bits(), Ordering::Relaxed);
+        self.sample
+            .store((frame as f64).to_bits(), Ordering::Relaxed);
+    }
+
+    fn f(a: &AtomicU64) -> f64 {
+        f64::from_bits(a.load(Ordering::Relaxed))
+    }
+
+    /// What `beatAndTempoProc` reports: beat, tempo. // RT
+    pub(crate) fn beat_and_tempo(&self) -> (f64, f64) {
+        (Self::f(&self.beat), Self::f(&self.tempo))
+    }
+
+    /// What `musicalTimeLocationProc` reports: frames to the next whole
+    /// beat, the 4/4 time signature (3b Plan Ruling 11) and the downbeat of
+    /// the current bar. // RT
+    pub(crate) fn musical_time(&self) -> (u32, f32, u32, f64) {
+        let beat = Self::f(&self.beat);
+        let frames_per_beat = 60.0 / Self::f(&self.tempo) * Self::f(&self.rate);
+        let to_next = (beat.ceil() - beat) * frames_per_beat;
+        let downbeat = (beat / 4.0).floor() * 4.0;
+        (to_next.round().max(0.0) as u32, 4.0, 4, downbeat)
+    }
+
+    /// What `transportStateProc2` reports: playing, changed (cleared by the
+    /// read), sample position. // RT
+    pub(crate) fn state(&self) -> (bool, bool, f64) {
+        (
+            self.playing.load(Ordering::Relaxed),
+            self.changed.swap(false, Ordering::Relaxed),
+            Self::f(&self.sample),
+        )
+    }
+}
+
+/// `refcon` is the `HostTransport` the callbacks were installed with.
+unsafe fn transport<'a>(refcon: *mut c_void) -> Option<&'a HostTransport> {
+    // SAFETY: the callbacks are installed with a pointer to a
+    // `HostTransport` that the instance and its processor keep alive (in an
+    // `Arc`) for as long as the unit exists.
+    unsafe { refcon.cast::<HostTransport>().as_ref() }
+}
+
+unsafe extern "C-unwind" fn beat_and_tempo_proc(
+    refcon: *mut c_void,
+    beat: *mut f64,
+    tempo: *mut f64,
+) -> i32 {
+    // SAFETY: see `transport`.
+    let Some(t) = (unsafe { transport(refcon) }) else {
+        return -10863; // kAudioUnitErr_CannotDoInCurrentContext
+    };
+    let (b, bpm) = t.beat_and_tempo();
+    // SAFETY: the unit passes valid out-pointers or null for values it
+    // does not want.
+    unsafe {
+        if let Some(p) = beat.as_mut() {
+            *p = b;
+        }
+        if let Some(p) = tempo.as_mut() {
+            *p = bpm;
+        }
+    }
+    0
+}
+
+unsafe extern "C-unwind" fn musical_time_proc(
+    refcon: *mut c_void,
+    to_next_beat: *mut u32,
+    numerator: *mut f32,
+    denominator: *mut u32,
+    downbeat: *mut f64,
+) -> i32 {
+    // SAFETY: see `transport`.
+    let Some(t) = (unsafe { transport(refcon) }) else {
+        return -10863;
+    };
+    let (next, num, den, down) = t.musical_time();
+    // SAFETY: valid out-pointers or null, as above.
+    unsafe {
+        if let Some(p) = to_next_beat.as_mut() {
+            *p = next;
+        }
+        if let Some(p) = numerator.as_mut() {
+            *p = num;
+        }
+        if let Some(p) = denominator.as_mut() {
+            *p = den;
+        }
+        if let Some(p) = downbeat.as_mut() {
+            *p = down;
+        }
+    }
+    0
+}
+
+#[allow(clippy::too_many_arguments)]
+unsafe extern "C-unwind" fn transport_state_proc2(
+    refcon: *mut c_void,
+    playing: *mut u8,
+    recording: *mut u8,
+    changed: *mut u8,
+    sample: *mut f64,
+    cycling: *mut u8,
+    cycle_start: *mut f64,
+    cycle_end: *mut f64,
+) -> i32 {
+    // SAFETY: see `transport`.
+    let Some(t) = (unsafe { transport(refcon) }) else {
+        return -10863;
+    };
+    let (play, moved, at) = t.state();
+    // SAFETY: valid out-pointers or null, as above.
+    unsafe {
+        if let Some(p) = playing.as_mut() {
+            *p = u8::from(play);
+        }
+        if let Some(p) = recording.as_mut() {
+            *p = 0;
+        }
+        if let Some(p) = changed.as_mut() {
+            *p = u8::from(moved);
+        }
+        if let Some(p) = sample.as_mut() {
+            *p = at;
+        }
+        if let Some(p) = cycling.as_mut() {
+            *p = 0;
+        }
+        if let Some(p) = cycle_start.as_mut() {
+            *p = 0.0;
+        }
+        if let Some(p) = cycle_end.as_mut() {
+            *p = 0.0;
+        }
+    }
+    0
+}
+
+unsafe extern "C-unwind" fn transport_state_proc(
+    refcon: *mut c_void,
+    playing: *mut u8,
+    changed: *mut u8,
+    sample: *mut f64,
+    cycling: *mut u8,
+    cycle_start: *mut f64,
+    cycle_end: *mut f64,
+) -> i32 {
+    // SAFETY: forwards the same pointers (recording not asked).
+    unsafe {
+        transport_state_proc2(
+            refcon,
+            playing,
+            ptr::null_mut(),
+            changed,
+            sample,
+            cycling,
+            cycle_start,
+            cycle_end,
+        )
+    }
+}
+
+fn callbacks(t: &HostTransport) -> HostCallbackInfo {
+    HostCallbackInfo {
+        hostUserData: ptr::from_ref(t).cast_mut().cast::<c_void>(),
+        beatAndTempoProc: Some(beat_and_tempo_proc),
+        musicalTimeLocationProc: Some(musical_time_proc),
+        transportStateProc: Some(transport_state_proc),
+        transportStateProc2: Some(transport_state_proc2),
+    }
+}
+
+impl Unit {
+    /// Installs the host callbacks reading `t` (which must outlive the
+    /// unit: callers keep it in the same `Arc`s as the unit).
+    pub(crate) fn set_host_callbacks(&self, t: &Arc<HostTransport>) -> Result<(), String> {
+        let info = callbacks(t);
+        self.set(
+            "set HostCallbacks",
+            kAudioUnitProperty_HostCallbacks,
+            kAudioUnitScope_Global,
+            0,
+            &info,
+        )
+    }
+}
+
+/// What the callbacks installed for `t` report, called the way a unit
+/// calls them (tests): beat and tempo; frames to the next beat, numerator,
+/// denominator, downbeat; playing, changed, sample.
+#[allow(clippy::type_complexity)]
+pub(crate) fn call_host_callbacks(
+    t: &HostTransport,
+) -> ((f64, f64), (u32, f32, u32, f64), (bool, bool, f64)) {
+    let info = callbacks(t);
+    let (mut b, mut bpm) = (0.0, 0.0);
+    let (mut next, mut num, mut den, mut down) = (0u32, 0f32, 0u32, 0.0);
+    let (mut play, mut rec, mut moved, mut at, mut cyc, mut cs, mut ce) =
+        (0u8, 0u8, 0u8, 0.0, 0u8, 0.0, 0.0);
+    // SAFETY: the callbacks get the pointer they were made for and valid
+    // out-pointers, exactly as a unit would pass them.
+    unsafe {
+        if let Some(f) = info.beatAndTempoProc {
+            f(info.hostUserData, &mut b, &mut bpm);
+        }
+        if let Some(f) = info.musicalTimeLocationProc {
+            f(info.hostUserData, &mut next, &mut num, &mut den, &mut down);
+        }
+        if let Some(f) = info.transportStateProc2 {
+            f(
+                info.hostUserData,
+                &mut play,
+                &mut rec,
+                &mut moved,
+                &mut at,
+                &mut cyc,
+                &mut cs,
+                &mut ce,
+            );
+        }
+    }
+    (
+        (b, bpm),
+        (next, num, den, down),
+        (play != 0, moved != 0, at),
+    )
+}
VIZDAW_EOF
```
Change `crates/platform-macos/src/au/mod.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/platform-macos/src/au/mod.rs b/crates/platform-macos/src/au/mod.rs
index 8b86c98..b94ce5b 100644
--- a/crates/platform-macos/src/au/mod.rs
+++ b/crates/platform-macos/src/au/mod.rs
@@ -7,6 +7,7 @@
 
 mod ffi;
 
+use core_engine::backend::BlockInfo;
 use core_engine::graph::ProcessStatus;
 use core_host::catalog::Source;
 use core_host::{
@@ -15,7 +16,7 @@ use core_host::{
     Prepared, ProcessCtx, Tail,
 };
 use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};
-use ffi::{ComponentInfo, Feed, GestureListener, Unit};
+use ffi::{ComponentInfo, Feed, GestureListener, HostTransport, Unit};
 use objc2_audio_toolbox::{
     AudioUnitParameterOptions, kAudioUnitScope_Input, kAudioUnitScope_Output,
     kAudioUnitType_Effect, kAudioUnitType_MusicDevice, kAudioUnitType_MusicEffect,
@@ -156,6 +157,9 @@ struct Opened {
     kind: u32,
     config: (f64, u32),
     feed: Feed,
+    /// What its host callbacks read (installed when the unit took them).
+    transport: Arc<HostTransport>,
+    callbacks: bool,
 }
 
 /// The AU [`PluginFormat`].
@@ -185,6 +189,10 @@ impl PluginFormat for AuFormat {
             // take their connections only at initialization.
             let feed = Feed::new(max);
             connect(&unit, &feed)?;
+            // Tempo and position for tempo-synced units (spec §5); a unit
+            // that refuses the callbacks still plays.
+            let transport = Arc::new(HostTransport::new(sr));
+            let callbacks = unit.set_host_callbacks(&transport).is_ok();
             unit.initialize().map_err(HostError::ActivateFailed)?;
             Ok(Prepared(Box::new(Opened {
                 desc: describe(&c),
@@ -192,6 +200,8 @@ impl PluginFormat for AuFormat {
                 unit,
                 config: (sr, max),
                 feed,
+                transport,
+                callbacks,
             })))
         })
     }
@@ -330,6 +340,9 @@ pub struct AuInstance {
     /// activation at that configuration hands it to the processor without
     /// initializing the unit again.
     ready: Option<Feed>,
+    /// What the unit's host callbacks read.
+    transport: Arc<HostTransport>,
+    callbacks: bool,
     /// Initializations done by `activate` (on the main thread).
     #[cfg(test)]
     main_inits: u32,
@@ -367,6 +380,8 @@ impl AuInstance {
             listener: None,
             writes: Arc::new(HostWrites::new(Vec::new())),
             ready: Some(o.feed),
+            transport: o.transport,
+            callbacks: o.callbacks,
             #[cfg(test)]
             main_inits: 0,
         };
@@ -463,6 +478,12 @@ impl AuInstance {
         };
     }
 
+    /// Whether the unit took VizDAW's host callbacks (tempo, beat
+    /// position, transport state).
+    pub fn host_callbacks_installed(&self) -> bool {
+        self.callbacks
+    }
+
     /// Acts like the plugin's own UI changing parameter `id` (begin
     /// gesture, value, end gesture). For tests: Apple's built-in units have
     /// no scriptable UI.
@@ -511,11 +532,14 @@ impl PluginInstance for AuInstance {
         // is a change after this point (`read_info` reads the latency next).
         self.latency.store(false, Ordering::Release);
         self.config = Some((sample_rate, max_frames));
+        self.transport.set_rate(sample_rate);
         self.read_info();
         self.active = true;
         Ok(Box::new(AuProcessor {
             unit: Arc::clone(&self.unit),
             writes: Arc::clone(&self.writes),
+            transport: Arc::clone(&self.transport),
+            last: None,
             feed,
             sample_time: 0.0,
             out_counts: self
@@ -613,6 +637,10 @@ impl PluginInstance for AuInstance {
 pub struct AuProcessor {
     unit: Arc<Unit>,
     writes: Arc<HostWrites>,
+    /// What the unit's host callbacks read; set before each render.
+    transport: Arc<HostTransport>,
+    /// The previous block's playing frame and length.
+    last: Option<(u64, u32)>,
     feed: Feed,
     sample_time: f64,
     out_counts: Vec<usize>,
@@ -628,6 +656,7 @@ impl std::fmt::Debug for AuProcessor {
 impl AudioProcessor for AuProcessor {
     fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus {
         let n = ctx.frames.min(self.max_frames);
+        store_block(&self.transport, &mut self.last, ctx.transport, n);
         for p in ctx.params_in {
             if let Ok(id) = u32::try_from(p.id.0) {
                 self.writes.note(id, p.value as f32);
@@ -668,6 +697,76 @@ impl AudioProcessor for AuProcessor {
     }
 }
 
+/// Stores one block's transport facts for the host callbacks; a playing
+/// block that does not follow the previous one is a jump. // RT
+fn store_block(t: &HostTransport, last: &mut Option<(u64, u32)>, info: &BlockInfo, n: u32) {
+    let expected = match last {
+        Some((frame, len)) if info.playing => *frame + u64::from(*len),
+        _ => info.frame,
+    };
+    t.set(
+        info.playing,
+        info.frame,
+        info.beats,
+        info.tempo_bpm,
+        expected,
+    );
+    *last = info.playing.then_some((info.frame, n));
+}
+
+/// What an AU's host callbacks report (`kAudioUnitProperty_HostCallbacks`).
+#[derive(Debug, Clone, Copy, PartialEq)]
+pub struct HostCallbacksSeen {
+    /// Quarter-note position.
+    pub beat: f64,
+    /// Tempo in BPM.
+    pub tempo: f64,
+    /// Frames from the block start to the next whole beat.
+    pub to_next_beat: u32,
+    /// Time signature numerator.
+    pub numerator: f32,
+    /// Time signature denominator.
+    pub denominator: u32,
+    /// Beat of the current bar's downbeat.
+    pub downbeat: f64,
+    /// The transport plays.
+    pub playing: bool,
+    /// Playing toggled or the position jumped since the last read.
+    pub changed: bool,
+    /// Schedule frame of the block start.
+    pub sample: f64,
+}
+
+/// Calls the host callbacks the way a unit does after a processor stored
+/// `blocks` (each with its frame count), at `rate`. For tests: Apple's
+/// built-in units do not ask for the transport.
+pub fn host_callbacks_after(rate: f64, blocks: &[(BlockInfo, u32)]) -> Vec<HostCallbacksSeen> {
+    let t = HostTransport::new(rate);
+    let mut last = None;
+    blocks
+        .iter()
+        .map(|(info, n)| {
+            store_block(&t, &mut last, info, *n);
+            let (
+                (beat, tempo),
+                (to_next_beat, numerator, denominator, downbeat),
+                (playing, changed, sample),
+            ) = ffi::call_host_callbacks(&t);
+            HostCallbacksSeen {
+                beat,
+                tempo,
+                to_next_beat,
+                numerator,
+                denominator,
+                downbeat,
+                playing,
+                changed,
+                sample,
+            }
+        })
+        .collect()
+}
+
 /// Loads a component for the scanner and the tests: instantiate, configure
 /// and initialize at `sample_rate` / `max_frames`.
 pub fn load(id: &str, sample_rate: f64, max_frames: u32) -> Result<AuInstance, HostError> {
VIZDAW_EOF
```

- [ ] **Step 4: Run the test to see it pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test au_transport
cargo test -p platform-macos --test au --test au_host
```

Expected: `au_transport` 2 passed (`apple_units_take_the_host_callbacks`, `the_callbacks_report_tempo_position_and_jumps`); the existing AU tests still pass.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace
```

Expected: clean; every test passes (517).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/platform-macos/src/au/ffi.rs crates/platform-macos/src/au/mod.rs \
  crates/platform-macos/tests/au_transport.rs
git commit -m "$(cat <<'MSG'
feat(platform-macos): AU transport through host callbacks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 3: AU editors — Cocoa UI, v3 view controller, generic view


**Files:**
- Modify: `crates/platform-macos/Cargo.toml` (`objc2`, CF features, the `au_editor` test target)
- Modify: `crates/platform-macos/src/au/ffi.rs`
- Modify: `crates/platform-macos/src/au/mod.rs`
- Modify: `crates/core-host/src/instance.rs` (`ParentView::as_ptr`)
- Create: `crates/platform-macos/tests/au_editor.rs` (`harness = false`: AppKit needs the process main thread)
- Lockfile: `Cargo.lock`

**Interfaces:**
- Consumes: 3b's `PluginEditor` trait (`open(ParentView) -> Result<(u32, u32), HostError>`, `resize`, `can_resize`, `close`), `ParentView::new` (`unsafe`, the caller keeps the view alive).
- Produces: `ParentView::as_ptr(self) -> *mut c_void`; `platform_macos::au::AuEditor` (a `PluginEditor`; `view_kind(&self) -> Option<&'static str>`: `"cocoa"`, `"controller"`, `"generic"`); `AuInstance::editor()` now returns `Some` for every unit; `AuInstance::au_editor(&mut self) -> &mut AuEditor`; `platform_macos::au::V3_VIEW_WAIT` (3 s).

Ruling 19. All AppKit and Objective-C calls are in `au/ffi.rs` (`Unit::editor_view`, `View::{attach, size}`, `Drop` removes the view from its parent), each with a `// SAFETY:` comment; `MainThreadMarker` refuses other threads with "plugin editors open on the main thread only". `AuEditor` is the first field of `AuInstance`, so the view leaves its window before the unit is disposed. AU views are fixed size. `platform-macos` links AppKit and CoreAudioKit (the `AUGenericView` class).

- [ ] **Step 1: Write the failing test (with its test target)**

Create `crates/platform-macos/tests/au_editor.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/platform-macos/tests
cat > crates/platform-macos/tests/au_editor.rs <<'VIZDAW_EOF'
//! AU editors (spec §9) against Apple's built-in units, on the process main
//! thread as AppKit requires (this test has its own `main`): AUDelay brings
//! its own Cocoa view, AUMatrixReverb gets CoreAudioKit's generic view, a
//! closed or dropped editor leaves the window, and another thread cannot
//! open one.

#[cfg(target_os = "macos")]
fn main() {
    use core_host::{ParentView, PluginInstance};
    use objc2::msg_send;
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use platform_macos::au;

    fn subviews(parent: &AnyObject) -> usize {
        // SAFETY: `subviews` of a live NSView is an NSArray.
        unsafe {
            let a: Retained<AnyObject> = msg_send![parent, subviews];
            msg_send![&a, count]
        }
    }

    // The window's content view stand-in.
    let cls = AnyClass::get(c"NSView").unwrap();
    let frame = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 800.0,
            height: 600.0,
        },
    };
    // SAFETY: an NSView made on the main thread.
    let parent: Retained<AnyObject> = unsafe {
        let a: Allocated<AnyObject> = msg_send![cls, alloc];
        msg_send![a, initWithFrame: frame]
    };
    let raw = Retained::as_ptr(&parent).cast_mut().cast();
    // SAFETY: `parent` lives until the end of `main`, after every editor.
    let view = || unsafe { ParentView::new(raw) };

    let mut delay = au::load("aufx/dely/appl", 48_000.0, 512).unwrap();
    let (w, h) = delay.editor().unwrap().open(view()).unwrap();
    assert!(w > 50 && h > 50, "AUDelay's own view: {w}×{h}");
    assert_eq!(delay.au_editor().view_kind(), Some("cocoa"));
    assert_eq!(subviews(&parent), 1);
    assert!(!delay.editor().unwrap().can_resize());
    delay.editor().unwrap().close();
    assert_eq!(subviews(&parent), 0, "close removes the view");
    println!("au_editor: AUDelay Cocoa view {w}×{h}");

    let mut reverb = au::load("aufx/mrev/appl", 48_000.0, 512).unwrap();
    let (w, h) = reverb.editor().unwrap().open(view()).unwrap();
    assert!(w > 0 && h > 0);
    assert_eq!(reverb.au_editor().view_kind(), Some("generic"));
    assert_eq!(subviews(&parent), 1);
    drop(reverb);
    assert_eq!(subviews(&parent), 0, "dropping the unit removes its view");
    println!("au_editor: AUMatrixReverb generic view {w}×{h}");

    let addr = raw as usize;
    let off_main = std::thread::spawn(move || {
        let mut d = au::load("aufx/dely/appl", 48_000.0, 512).unwrap();
        // SAFETY: never attached: refused before the view is touched.
        let p = unsafe { ParentView::new(addr as *mut std::ffi::c_void) };
        d.editor()
            .unwrap()
            .open(p)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .join()
    .unwrap();
    assert!(
        off_main.as_ref().is_err_and(|e| e.contains("main thread")),
        "{off_main:?}"
    );
    assert_eq!(subviews(&parent), 0);
    println!("au_editor: ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
VIZDAW_EOF
```
Change `crates/platform-macos/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/platform-macos/Cargo.toml b/crates/platform-macos/Cargo.toml
index ef545e4..fec54a2 100644
--- a/crates/platform-macos/Cargo.toml
+++ b/crates/platform-macos/Cargo.toml
@@ -11,11 +11,17 @@ core-host = { path = "../core-host" }
 core-model = { path = "../core-model" }
 rtrb = "0.4"
 block2 = "0.6"
+objc2 = "0.6"
 objc2-core-audio = { version = "0.3.2", default-features = false, features = ["std", "AudioHardware", "HostTime", "objc2-core-audio-types"] }
 objc2-core-audio-types = { version = "0.3.2", default-features = false, features = ["std", "CoreAudioBaseTypes"] }
 objc2-audio-toolbox = { version = "0.3.2", default-features = false, features = ["std", "AudioComponent", "AUComponent", "AudioUnitProperties", "AudioUnitUtilities", "MusicDevice", "objc2-core-audio-types", "objc2-core-foundation", "block2"] }
 objc2-core-midi = { version = "0.3.2", default-features = false, features = ["std", "MIDIServices", "MIDIMessages", "objc2", "objc2-core-foundation", "block2"] }
-objc2-core-foundation = { version = "0.3.2", default-features = false, features = ["std", "CFString", "CFData", "CFError", "CFPropertyList"] }
+objc2-core-foundation = { version = "0.3.2", default-features = false, features = ["std", "CFString", "CFData", "CFError", "CFPropertyList", "CFBundle", "CFURL", "CFRunLoop", "CFDate", "CFCGTypes", "objc2"] }
 
 [target.'cfg(target_os = "macos")'.dev-dependencies]
 core-engine = { path = "../core-engine", features = ["fixtures"] }
+
+# AppKit needs the process main thread, so this test brings its own `main`.
+[[test]]
+name = "au_editor"
+harness = false
VIZDAW_EOF
```

- [ ] **Step 2: Run it to see it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test au_editor
```

Expected: compile errors — no method `au_editor` on `AuInstance`, no method `view_kind`.

- [ ] **Step 3: Implement**

Change `crates/core-host/src/instance.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-host/src/instance.rs b/crates/core-host/src/instance.rs
index 20cda33..e41b201 100644
--- a/crates/core-host/src/instance.rs
+++ b/crates/core-host/src/instance.rs
@@ -50,6 +50,11 @@ impl ParentView {
     pub unsafe fn new(view: *mut c_void) -> ParentView {
         ParentView(view)
     }
+
+    /// The native view (for format hosts outside this crate).
+    pub fn as_ptr(self) -> *mut c_void {
+        self.0
+    }
 }
 
 /// A plugin's own editor. Main thread only.
VIZDAW_EOF
```
Change `crates/platform-macos/src/au/ffi.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/platform-macos/src/au/ffi.rs b/crates/platform-macos/src/au/ffi.rs
index 5b71515..9f9bc1c 100644
--- a/crates/platform-macos/src/au/ffi.rs
+++ b/crates/platform-macos/src/au/ffi.rs
@@ -4,21 +4,26 @@
 //! UI gestures.
 
 use block2::RcBlock;
+use objc2::rc::{Allocated, Retained};
+use objc2::runtime::{AnyClass, AnyObject};
+use objc2::{MainThreadMarker, msg_send};
 use objc2_audio_toolbox::{
     AUListenerDispose, AUParameterListenerRef, AUParameterSet, AURenderCallbackStruct,
     AudioComponent, AudioComponentCopyName, AudioComponentDescription, AudioComponentFindNext,
     AudioComponentFlags, AudioComponentGetDescription, AudioComponentGetVersion,
     AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioComponentInstantiate,
     AudioComponentInstantiationOptions, AudioUnit, AudioUnitAddPropertyListener,
-    AudioUnitEventType, AudioUnitGetParameter, AudioUnitGetProperty, AudioUnitGetPropertyInfo,
-    AudioUnitInitialize, AudioUnitParameter, AudioUnitParameterInfo, AudioUnitParameterOptions,
-    AudioUnitParameterUnit, AudioUnitPropertyID, AudioUnitRender, AudioUnitRenderActionFlags,
-    AudioUnitReset, AudioUnitScope, AudioUnitSetParameter, AudioUnitSetProperty,
-    AudioUnitUninitialize, HostCallbackInfo, MusicDeviceMIDIEvent, kAudioUnitProperty_ClassInfo,
-    kAudioUnitProperty_ElementCount, kAudioUnitProperty_HostCallbacks, kAudioUnitProperty_Latency,
+    AudioUnitCocoaViewInfo, AudioUnitEventType, AudioUnitGetParameter, AudioUnitGetProperty,
+    AudioUnitGetPropertyInfo, AudioUnitInitialize, AudioUnitParameter, AudioUnitParameterInfo,
+    AudioUnitParameterOptions, AudioUnitParameterUnit, AudioUnitPropertyID, AudioUnitRender,
+    AudioUnitRenderActionFlags, AudioUnitReset, AudioUnitScope, AudioUnitSetParameter,
+    AudioUnitSetProperty, AudioUnitUninitialize, HostCallbackInfo, MusicDeviceMIDIEvent,
+    kAudioUnitProperty_ClassInfo, kAudioUnitProperty_CocoaUI, kAudioUnitProperty_ElementCount,
+    kAudioUnitProperty_HostCallbacks, kAudioUnitProperty_Latency,
     kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitProperty_ParameterInfo,
-    kAudioUnitProperty_ParameterList, kAudioUnitProperty_SetRenderCallback,
-    kAudioUnitProperty_StreamFormat, kAudioUnitProperty_TailTime, kAudioUnitScope_Global,
+    kAudioUnitProperty_ParameterList, kAudioUnitProperty_RequestViewController,
+    kAudioUnitProperty_SetRenderCallback, kAudioUnitProperty_StreamFormat,
+    kAudioUnitProperty_TailTime, kAudioUnitScope_Global,
 };
 use objc2_core_audio_types::{
     AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
@@ -26,9 +31,11 @@ use objc2_core_audio_types::{
     kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM,
 };
 use objc2_core_foundation::{
-    CFData, CFPropertyList, CFPropertyListCreateData, CFPropertyListCreateWithData,
-    CFPropertyListFormat, CFRetained, CFString,
+    CFBundle, CFData, CFPropertyList, CFPropertyListCreateData, CFPropertyListCreateWithData,
+    CFPropertyListFormat, CFRetained, CFRunLoop, CFString, CFURL, CGPoint, CGRect, CGSize,
+    kCFRunLoopDefaultMode,
 };
+use std::ffi::CString;
 use std::ffi::c_void;
 use std::ptr::{self, NonNull};
 use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
@@ -1295,3 +1302,234 @@ pub(crate) fn call_host_callbacks(
         (play != 0, moved != 0, at),
     )
 }
+
+// ---------------------------------------------------------------------------
+// Editors (spec §9; 3b Plan Ruling 7 left them to 3c): the unit's own Cocoa
+// view (AU v2, `kAudioUnitProperty_CocoaUI`), its view controller's view
+// (AU v3, `kAudioUnitProperty_RequestViewController`), or CoreAudioKit's
+// generic view. AppKit: main thread only.
+
+#[link(name = "AppKit", kind = "framework")]
+unsafe extern "C" {}
+#[link(name = "CoreAudioKit", kind = "framework")]
+unsafe extern "C" {}
+
+/// Where an editor's view came from.
+#[derive(Debug, Clone, Copy, PartialEq, Eq)]
+pub(crate) enum ViewKind {
+    /// The unit's Cocoa view factory (AU v2).
+    Cocoa,
+    /// The unit's view controller (AU v3).
+    Controller,
+    /// CoreAudioKit's `AUGenericView` (a unit without a UI of its own).
+    Generic,
+}
+
+/// A plugin's `NSView`, shown inside a parent view the app owns. Removed
+/// from its parent on drop (before the unit can go: `AuInstance` drops its
+/// editor first).
+pub(crate) struct View {
+    view: Retained<AnyObject>,
+    /// A v3 view controller owns its view: kept while the view is shown.
+    _controller: Option<Retained<AnyObject>>,
+    pub(crate) kind: ViewKind,
+}
+
+impl std::fmt::Debug for View {
+    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
+        f.debug_struct("View").field("kind", &self.kind).finish()
+    }
+}
+
+/// The size an editor is asked for when the unit has no preference.
+const PREFERRED: CGSize = CGSize {
+    width: 400.0,
+    height: 300.0,
+};
+
+impl Unit {
+    /// The unit's editor view: its Cocoa view, else its v3 view controller
+    /// (waited for up to `wait` while the main run loop runs), else the
+    /// generic view. Main thread only.
+    pub(crate) fn editor_view(&self, wait: Duration) -> Result<View, String> {
+        if MainThreadMarker::new().is_none() {
+            return Err("plugin editors open on the main thread only".into());
+        }
+        self.cocoa_view()
+            .or_else(|_| self.controller_view(wait))
+            .or_else(|_| self.generic_view())
+    }
+
+    fn cocoa_view(&self) -> Result<View, String> {
+        let mut size = 0u32;
+        // SAFETY: valid unit and out-pointer; no data is written.
+        os("CocoaUI info", unsafe {
+            AudioUnitGetPropertyInfo(
+                self.unit,
+                kAudioUnitProperty_CocoaUI,
+                kAudioUnitScope_Global,
+                0,
+                &mut size,
+                ptr::null_mut(),
+            )
+        })?;
+        let n = size as usize;
+        if n < size_of::<AudioUnitCocoaViewInfo>() {
+            return Err("no Cocoa view".into());
+        }
+        // One URL followed by at least one class name, each a CF object.
+        let mut raw = vec![ptr::null::<c_void>(); n.div_ceil(size_of::<*const c_void>())];
+        // SAFETY: `raw` has room for `size` bytes.
+        os("CocoaUI", unsafe {
+            AudioUnitGetProperty(
+                self.unit,
+                kAudioUnitProperty_CocoaUI,
+                kAudioUnitScope_Global,
+                0,
+                NonNull::new_unchecked(raw.as_mut_ptr().cast::<c_void>()),
+                NonNull::from(&mut size),
+            )
+        })?;
+        let (url, class) = match raw.as_slice() {
+            [u, c, ..] if !u.is_null() && !c.is_null() => (*u, *c),
+            _ => return Err("empty Cocoa view info".into()),
+        };
+        // SAFETY: the property hands the host one reference to each CF
+        // object (the URL and every class name), which the host releases;
+        // further class names are released here too.
+        let (url, class) = unsafe {
+            for extra in raw.iter().skip(2).take(n / size_of::<*const c_void>() - 2) {
+                if !extra.is_null() {
+                    drop(CFRetained::<CFString>::from_raw(NonNull::new_unchecked(
+                        extra.cast_mut().cast(),
+                    )));
+                }
+            }
+            (
+                CFRetained::<CFURL>::from_raw(NonNull::new_unchecked(url.cast_mut().cast())),
+                CFRetained::<CFString>::from_raw(NonNull::new_unchecked(class.cast_mut().cast())),
+            )
+        };
+        let bundle = CFBundle::new(None, Some(&url)).ok_or("cannot open the view bundle")?;
+        // SAFETY: loading the plugin's own view bundle runs its
+        // initializers; the plugin is trusted in-process code by then.
+        if !unsafe { bundle.load_executable() } {
+            return Err("cannot load the view bundle".into());
+        }
+        let name = CString::new(class.to_string()).map_err(|e| e.to_string())?;
+        let cls = AnyClass::get(&name).ok_or("no view factory class")?;
+        // SAFETY: the class implements `AUCocoaUIBase` (that is what the
+        // property names); `new` returns an owned factory and
+        // `uiViewForAudioUnit:withSize:` an autoreleased `NSView` for a live
+        // unit.
+        let view: Option<Retained<AnyObject>> = unsafe {
+            let factory: Retained<AnyObject> = msg_send![cls, new];
+            msg_send![&factory, uiViewForAudioUnit: self.unit, withSize: PREFERRED]
+        };
+        Ok(View {
+            view: view.ok_or("the view factory made no view")?,
+            _controller: None,
+            kind: ViewKind::Cocoa,
+        })
+    }
+
+    fn controller_view(&self, wait: Duration) -> Result<View, String> {
+        let slot: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
+        let fill = Arc::clone(&slot);
+        let block = RcBlock::new(move |vc: *mut AnyObject| {
+            // SAFETY: the unit hands over a valid view controller (or
+            // null); one reference is taken here and adopted below.
+            if let Some(vc) = unsafe { Retained::retain(vc) } {
+                let raw = Retained::into_raw(vc) as usize;
+                *fill.lock().unwrap_or_else(PoisonError::into_inner) = Some(raw);
+            }
+        });
+        let ptr_to_block: *const block2::Block<dyn Fn(*mut AnyObject)> = &*block;
+        self.set(
+            "RequestViewController",
+            kAudioUnitProperty_RequestViewController,
+            kAudioUnitScope_Global,
+            0,
+            &ptr_to_block,
+        )?;
+        let until = std::time::Instant::now() + wait;
+        let raw = loop {
+            if let Some(raw) = slot.lock().unwrap_or_else(PoisonError::into_inner).take() {
+                break raw;
+            }
+            if std::time::Instant::now() >= until {
+                return Err("no view controller".into());
+            }
+            // SAFETY: the default mode is a valid static; the main run loop
+            // delivers the unit's completion.
+            let mode = unsafe { kCFRunLoopDefaultMode };
+            CFRunLoop::run_in_mode(mode, 0.02, true);
+        };
+        // SAFETY: `raw` is the reference taken in the block.
+        let vc =
+            unsafe { Retained::from_raw(raw as *mut AnyObject) }.ok_or("no view controller")?;
+        // SAFETY: an `NSViewController`'s `view` is a retained-by-it NSView.
+        let view: Option<Retained<AnyObject>> = unsafe { msg_send![&vc, view] };
+        Ok(View {
+            view: view.ok_or("the view controller has no view")?,
+            _controller: Some(vc),
+            kind: ViewKind::Controller,
+        })
+    }
+
+    fn generic_view(&self) -> Result<View, String> {
+        let cls = AnyClass::get(c"AUGenericView").ok_or("CoreAudioKit is missing")?;
+        // SAFETY: `AUGenericView` is an NSView subclass whose
+        // `initWithAudioUnit:` takes a live unit.
+        let view: Option<Retained<AnyObject>> = unsafe {
+            let obj: Allocated<AnyObject> = msg_send![cls, alloc];
+            msg_send![obj, initWithAudioUnit: self.unit]
+        };
+        Ok(View {
+            view: view.ok_or("no generic view")?,
+            _controller: None,
+            kind: ViewKind::Generic,
+        })
+    }
+}
+
+impl View {
+    /// Adds the view to `parent` (an `NSView *` the caller keeps alive while
+    /// it is shown) at its top-left; returns its size in points.
+    pub(crate) fn attach(&self, parent: *mut c_void) -> Result<(u32, u32), String> {
+        if MainThreadMarker::new().is_none() {
+            return Err("plugin editors open on the main thread only".into());
+        }
+        let parent = parent.cast::<AnyObject>();
+        // SAFETY: `parent` is a live `NSView` (the `ParentView` contract);
+        // the view is ours.
+        unsafe {
+            let parent = parent.as_ref().ok_or("no parent view")?;
+            let _: () = msg_send![parent, addSubview: &*self.view];
+            let _: () = msg_send![&*self.view, setFrameOrigin: CGPoint { x: 0.0, y: 0.0 }];
+        }
+        Ok(self.size())
+    }
+
+    /// The view's size in points.
+    pub(crate) fn size(&self) -> (u32, u32) {
+        // SAFETY: `frame` of a live view.
+        let f: CGRect = unsafe { msg_send![&*self.view, frame] };
+        (
+            f.size.width.round().max(1.0) as u32,
+            f.size.height.round().max(1.0) as u32,
+        )
+    }
+}
+
+impl Drop for View {
+    fn drop(&mut self) {
+        if MainThreadMarker::new().is_some() {
+            // SAFETY: removing a live view from its parent (a no-op when it
+            // has none).
+            unsafe {
+                let _: () = msg_send![&*self.view, removeFromSuperview];
+            }
+        }
+    }
+}
VIZDAW_EOF
```
Change `crates/platform-macos/src/au/mod.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/platform-macos/src/au/mod.rs b/crates/platform-macos/src/au/mod.rs
index b94ce5b..10f82dd 100644
--- a/crates/platform-macos/src/au/mod.rs
+++ b/crates/platform-macos/src/au/mod.rs
@@ -12,8 +12,8 @@ use core_engine::graph::ProcessStatus;
 use core_host::catalog::Source;
 use core_host::{
     AudioPortInfo, AudioProcessor, HostError, InstanceEvents, LoadRequest, ParamEvent, ParamInfo,
-    PluginEditor, PluginFormat, PluginInfo, PluginInstance, PluginKind, PortRole, PrepareJob,
-    Prepared, ProcessCtx, Tail,
+    ParentView, PluginEditor, PluginFormat, PluginInfo, PluginInstance, PluginKind, PortRole,
+    PrepareJob, Prepared, ProcessCtx, Tail,
 };
 use core_model::mixer::{ParamId, PluginFormat as Format, PluginRef};
 use ffi::{ComponentInfo, Feed, GestureListener, HostTransport, Unit};
@@ -325,8 +325,11 @@ impl HostWrites {
 
 /// A loaded AU (main-thread half).
 pub struct AuInstance {
-    // Declared first so it is dropped first: the listener's registrations
-    // go before the unit is disposed.
+    // Declared first so it is dropped first: its view leaves the window
+    // before anything else of the unit goes.
+    editor: AuEditor,
+    // Dropped next: the listener's registrations go before the unit is
+    // disposed.
     listener: Option<GestureListener>,
     unit: Arc<Unit>,
     kind: u32,
@@ -361,8 +364,13 @@ impl AuInstance {
         let latency = Arc::new(AtomicBool::new(false));
         // A unit that cannot report latency changes still works.
         let _ = o.unit.watch_latency(Arc::clone(&latency));
+        let unit = Arc::new(o.unit);
         let mut me = AuInstance {
-            unit: Arc::new(o.unit),
+            editor: AuEditor {
+                unit: Arc::clone(&unit),
+                view: None,
+            },
+            unit,
             kind: o.kind,
             info: PluginInfo {
                 plugin: o.desc.plugin,
@@ -590,9 +598,8 @@ impl PluginInstance for AuInstance {
     }
 
     fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
-        // AU editors (view controllers, Cocoa UI) arrive with the app's
-        // plugin windows (3c, Plan Ruling 7).
-        None
+        // Every unit has one: its own view, or the generic view.
+        Some(&mut self.editor)
     }
 
     fn idle(&mut self) -> InstanceEvents {
@@ -633,6 +640,74 @@ impl PluginInstance for AuInstance {
     }
 }
 
+/// How long opening an AU v3 editor waits for its view controller.
+pub const V3_VIEW_WAIT: Duration = Duration::from_secs(3);
+
+/// An AU's editor (spec §9): the unit's own view (AU v2 Cocoa UI, AU v3
+/// view controller) or CoreAudioKit's generic view, added to the window's
+/// parent view. Fixed size: AU views size themselves.
+pub struct AuEditor {
+    unit: Arc<Unit>,
+    view: Option<ffi::View>,
+}
+
+impl std::fmt::Debug for AuEditor {
+    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
+        f.debug_struct("AuEditor")
+            .field("open", &self.view.is_some())
+            .finish_non_exhaustive()
+    }
+}
+
+impl AuEditor {
+    /// Where the open view came from: `"cocoa"`, `"controller"` or
+    /// `"generic"` (`None` while closed).
+    pub fn view_kind(&self) -> Option<&'static str> {
+        self.view.as_ref().map(|v| match v.kind {
+            ffi::ViewKind::Cocoa => "cocoa",
+            ffi::ViewKind::Controller => "controller",
+            ffi::ViewKind::Generic => "generic",
+        })
+    }
+}
+
+impl PluginEditor for AuEditor {
+    fn open(&mut self, parent: ParentView) -> Result<(u32, u32), HostError> {
+        self.close();
+        let view = self
+            .unit
+            .editor_view(V3_VIEW_WAIT)
+            .map_err(HostError::EditorUnavailable)?;
+        let size = view
+            .attach(parent.as_ptr())
+            .map_err(HostError::EditorUnavailable)?;
+        self.view = Some(view);
+        Ok(size)
+    }
+
+    fn resize(&mut self, _width: u32, _height: u32) -> Result<(u32, u32), HostError> {
+        self.view
+            .as_ref()
+            .map(ffi::View::size)
+            .ok_or_else(|| HostError::EditorUnavailable("not open".into()))
+    }
+
+    fn can_resize(&mut self) -> bool {
+        false
+    }
+
+    fn close(&mut self) {
+        self.view = None;
+    }
+}
+
+impl AuInstance {
+    /// The editor, typed (tests read [`AuEditor::view_kind`]).
+    pub fn au_editor(&mut self) -> &mut AuEditor {
+        &mut self.editor
+    }
+}
+
 /// The real-time half of an [`AuInstance`].
 pub struct AuProcessor {
     unit: Arc<Unit>,
VIZDAW_EOF
```

- [ ] **Step 4: Run the test to see it pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p platform-macos --test au_editor
```

Expected: the test binary prints `au_editor: AUDelay Cocoa view 484×255` (the size may differ by macOS version), `au_editor: AUMatrixReverb generic view …` and `au_editor: ok`, and exits 0. Two `objc[…]: Class RendezvousService is implemented in both …` lines from Apple's own frameworks are harmless.

- [ ] **Step 5: Check the whole workspace**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: clean; every test passes (517; `au_editor` runs as its own binary).

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/platform-macos/Cargo.toml crates/platform-macos/src/au/ffi.rs \
  crates/platform-macos/src/au/mod.rs crates/core-host/src/instance.rs \
  crates/platform-macos/tests/au_editor.rs Cargo.lock
git commit -m "$(cat <<'MSG'
feat(platform-macos): AU editors (Cocoa UI, v3 view controller, generic view)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 4: Sessions host their plugins; the plugin timer


**Files:**
- Modify: `crates/core-ffi/Cargo.toml` (`core-host`; dev-dependency `test-plugins`)
- Create: `crates/core-ffi/src/catalog.rs` (the process catalog; the `PluginCatalog` object comes in Task 7)
- Create: `crates/core-ffi/src/plugins.rs`
- Modify: `crates/core-ffi/src/host.rs`, `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/snapshot.rs`, `crates/core-ffi/src/lib.rs`
- Create: `crates/core-ffi/tests/plugins.rs`
- Modify: `app/Sources/VizDAWAppModel/DocumentModel.swift`, `app/Sources/VizDAW/DocumentView.swift`, `app/Sources/VizDAW/VizDAWApp.swift`
- Test: `app/Tests/VizDAWAppModelTests/PluginIdleTests.swift` (new), `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Lockfile: `Cargo.lock`

**Interfaces:**
- Consumes: Task 1's `PluginHost::{state, loading}`, `SlotState`, `Catalog::apply_to`, `HostMarks`; 3b's `PluginHost::{new, add_format, sync, idle, service, reconfigure, config, factory, instance, live_count, retiring, commit_gestures}`, `platform_macos::au::AuFormat`, `platform_macos::rt::{MacRtPlatform, default_workers}`, `CoreAudioBackend::workgroup`.
- Produces:
  - `vizdaw_ffi::plugins::{create(u64) -> Result<(), VizError>, with<T>(u64, impl FnOnce(Option<&mut PluginHost>) -> T) -> T, apply_catalog(), has_plugins(u64) -> bool, close(u64), tick_closing(), config() -> (f64, u32), set_config(f64, u32), slot_row(&PluginSlot, bool, Option<&mut PluginHost>) -> SlotRow, slot_rows(&Document, Option<&mut PluginHost>) -> Vec<SlotRow>, CLOSING_WAIT}`.
  - `vizdaw_ffi::catalog::{with<T>(impl FnOnce(&mut Catalog) -> T) -> T, apply(&mut PluginHost, &mut HostMarks)}`; test hook `add_test_bundle(&Path)`.
  - `vizdaw_ffi::host::{service(u64, &Document, &mut PluginHost) -> IdleReport, factory_owner() -> Option<u64>, live_config(f64) -> EngineConfig (macOS), DEADLINE}`; `play`, `reload`, `state` take `Option<&mut PluginHost>`; test hooks `engine_workers() -> Option<usize>`, `closing_hosts_waiting() -> usize`, `set_offline_rate(f64)`.
  - FFI records `PluginFormatTag::{Au, Clap}`, `PluginRefRow { format, id, name, vendor, version }` (with `From` both ways to `PluginRef`), `SlotStatus::{Loading, Live, Placeholder { reason }}`, `SlotRow { id, plugin, instrument, bypass, sidechain: Option<u64>, status, latency: u32, has_sidechain_input, outputs: u32 }`, `IdleUpdate { snapshot: Option<DocSnapshot>, slots_changed, params_changed: Vec<u64>, problems: Vec<String> }`.
  - `Session::idle(&self) -> Result<IdleUpdate, VizError>`, `Session::plugin_slots(&self) -> Result<Vec<SlotRow>, VizError>`; test hook `Session::debug_plugin_rate() -> Result<f64, VizError>`.
  - Swift: `DocumentModel.idle()`, `pluginProblems: [String]`, `slotsVersion: Int`, `paramsChanged: Set<UInt64>`, `messages: [String]`; `PluginTicker.start(_:)` in the app.

Rulings 1, 2, 3, 5 and 6. A session creates its plugin host on the calling thread and syncs its document at once (loads start before any Play). Every edit, undo and redo goes through `host::reload`, which syncs the plugin host first and recompiles only when no load is in progress (`service` loads once they are in). `play` makes the session's host the engine's insert factory (`Host::attach`, rebuilding the engine first when another document's plugins were in it), reconfigures the plugins to the engine's rate and block, and always loads: slots still loading play as placeholders until `service` loads again. A device rebuild attaches the new engine the same way. `Session::idle` runs `host::service` and commits finished gestures as undo steps. A dropped session drops the engine first when its plugin host feeds it, then closes its host (processors still in the engine's retire ring are waited for by `tick_closing`). The compile report lines now include the graph's placeholders, so the fault popover shows "pluginN: Test Gain is loading".

- [ ] **Step 1: Write the failing Rust tests**

Create `crates/core-ffi/tests/plugins.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/plugins.rs <<'VIZDAW_EOF'
//! Sessions host their plugins (spec §5, §11): a project's plugins load on
//! open, play through the engine once in, survive another session taking
//! the engine and a device rebuild, and are released when the session
//! closes. The engine host is process-wide, so these tests run one at a
//! time; plugin hosts live on each test's own thread.

use core_engine::backend::BackendEvent;
use core_engine::engine::find_sequence;
use core_engine::fixtures::{Builder, demo_document};
use core_model::mixer::{PluginFormat, PluginRef, StripKind};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{
    closing_hosts_waiting, engine_active, force_offline, hosts_created, reset_host, simulate_event,
};
use vizdaw_ffi::{Session, SlotRow, SlotStatus};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

fn gain_ref(id: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: "Test Gain".into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

/// The demo saved with a `plugin` insert on the built-in synth's strip:
/// (folder, package path, Song, slot ID).
fn project(plugin: &str) -> (tempfile::TempDir, String, u64, u64) {
    let doc = demo_document().unwrap();
    let song = find_sequence(&doc, "Song").unwrap();
    let mut b = Builder::from_doc(doc);
    let synth = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let slot = b.insert(synth, gain_ref(plugin)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.vizdaw");
    core_model::file::save(&b.finish(), &path).unwrap();
    (dir, path.to_string_lossy().into_owned(), song.0, slot.0)
}

fn slot(s: &Session, id: u64) -> SlotRow {
    s.plugin_slots()
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
}

/// Runs the plugin timer until `done` (10 s at most).
fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let start = Instant::now();
    while !done(s) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn placeholder_lines(s: &Session) -> Vec<String> {
    s.transport()
        .unwrap()
        .report
        .into_iter()
        .filter(|l| l.contains("loading") || l.contains("Test Gain"))
        .collect()
}

/// The audio thread applies Play on its next block (2c Plan Ruling 30).
fn playing(s: &Session) -> bool {
    s.transport().unwrap().playing
}

fn open(path: &str) -> Arc<Session> {
    Session::open(path.to_string()).unwrap()
}

#[test]
fn a_projects_plugins_load_on_open_and_reach_the_engine() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let s = open(&path);
    assert_eq!(slot(&s, id).status, SlotStatus::Loading);
    // Played before the plugin is in: it is a placeholder for now.
    s.play(song).unwrap();
    assert!(!placeholder_lines(&s).is_empty());
    idle_until(&s, |s| slot(s, id).status == SlotStatus::Live);
    // The next timer step loads the engine with the running plugin.
    idle_until(&s, |s| placeholder_lines(s).is_empty() && playing(s));
    let row = slot(&s, id);
    assert_eq!(row.plugin.name, "Test Gain");
    assert!(!row.instrument && !row.has_sidechain_input);
    assert_eq!(row.outputs, 1);
}

#[test]
fn an_unknown_plugin_is_a_placeholder_with_its_reason() {
    let _g = serial();
    let (_d, path, _song, id) = project("vizdaw.test.nope");
    let s = open(&path);
    let mut problems = Vec::new();
    let start = Instant::now();
    while !matches!(slot(&s, id).status, SlotStatus::Placeholder { .. }) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        problems.extend(s.idle().unwrap().problems);
        std::thread::sleep(Duration::from_millis(2));
    }
    let SlotStatus::Placeholder { reason } = slot(&s, id).status else {
        unreachable!()
    };
    assert!(reason.contains("failed"), "{reason}");
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("Built-in Synth — Test Gain: failed")),
        "{problems:?}"
    );
}

#[test]
fn another_session_takes_the_engine_and_the_first_keeps_its_plugins() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let a = open(&path);
    a.play(song).unwrap();
    idle_until(&a, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    let b = Session::new_demo().unwrap();
    let b_song = b
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    b.play(b_song).unwrap();
    // A's processor comes back from the engine and is activated again.
    for _ in 0..50 {
        a.idle().unwrap();
        b.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(slot(&a, id).status, SlotStatus::Live);
    a.play(song).unwrap();
    idle_until(&a, |s| placeholder_lines(s).is_empty() && playing(s));
}

#[test]
fn another_documents_plugins_never_play_for_same_numbered_slots() {
    let _g = serial();
    // Two projects whose inserts have the same instance ID and plugin.
    let (_d1, p1, song, id) = project("vizdaw.test.gain");
    let (_d2, p2, _, id2) = project("vizdaw.test.gain");
    assert_eq!(id, id2);
    let a = open(&p1);
    let b = open(&p2);
    a.play(song).unwrap();
    idle_until(&a, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    let created = hosts_created();
    b.play(song).unwrap();
    assert_eq!(hosts_created(), created + 1, "a fresh engine for b");
    idle_until(&b, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live && playing(s)
    });
    // A's processor came back to A (the old engine went): closing A
    // leaves nothing waiting.
    a.idle().unwrap();
    drop(a);
    assert_eq!(closing_hosts_waiting(), 0);
    // Sessions without plugins switch without a rebuild.
    let c = Session::new_demo().unwrap();
    let d = Session::new_demo().unwrap();
    let c_song = c.snapshot().unwrap().sequences[2].id;
    c.play(c_song).unwrap();
    let created = hosts_created();
    d.play(c_song).unwrap();
    // b's engine had plugins: c rebuilt it; d inherits c's (none).
    assert_eq!(hosts_created(), created);
}

#[test]
fn a_device_rebuild_hands_the_plugins_to_the_new_engine() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let s = open(&path);
    s.play(song).unwrap();
    idle_until(&s, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    let created = hosts_created();
    simulate_event(BackendEvent::DeviceLost);
    s.transport().unwrap();
    assert_eq!(hosts_created(), created + 1);
    // The old engine gave the processor back; it is activated again and
    // the new engine loads it.
    idle_until(&s, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live && playing(s)
    });
}

#[test]
fn a_sample_rate_change_reactivates_the_plugins_at_the_new_rate() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let s = open(&path);
    s.play(song).unwrap();
    idle_until(&s, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    // The device now runs at 44.1 kHz: the engine is rebuilt at that rate
    // and the plugins are activated for it before they play again.
    assert_eq!(s.debug_plugin_rate().unwrap(), 48_000.0);
    vizdaw_ffi::host::set_offline_rate(44_100.0);
    simulate_event(BackendEvent::SampleRateChanged(44_100.0));
    assert_eq!(s.transport().unwrap().sample_rate, 44_100.0);
    assert_eq!(vizdaw_ffi::plugins::config().0, 44_100.0);
    assert_eq!(s.debug_plugin_rate().unwrap(), 44_100.0, "reconfigured");
    idle_until(&s, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live && playing(s)
    });
    vizdaw_ffi::host::set_offline_rate(48_000.0);
}

#[test]
fn closing_a_session_releases_its_plugins() {
    let _g = serial();
    let (_d, path, song, id) = project("vizdaw.test.gain");
    let a = open(&path);
    a.play(song).unwrap();
    idle_until(&a, |s| {
        placeholder_lines(s).is_empty() && slot(s, id).status == SlotStatus::Live
    });
    drop(a);
    assert!(!engine_active(), "the engine went first");
    assert_eq!(closing_hosts_waiting(), 0, "every processor came back");
    // A session that never played closes at once too.
    let b = open(&path);
    idle_until(&b, |s| slot(s, id).status == SlotStatus::Live);
    drop(b);
    assert_eq!(closing_hosts_waiting(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn the_live_engine_uses_the_default_worker_count() {
    let cfg = vizdaw_ffi::host::live_config(48_000.0);
    assert_eq!(cfg.workers, platform_macos::rt::default_workers());
    assert_eq!(cfg.deadline, Some(vizdaw_ffi::host::DEADLINE));
    // Offline keeps rendering on the callback thread.
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let song = s.snapshot().unwrap().sequences[0].id;
    s.play(song).unwrap();
    assert_eq!(vizdaw_ffi::host::engine_workers(), Some(0));
}
VIZDAW_EOF
```
Change `crates/core-ffi/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/Cargo.toml b/crates/core-ffi/Cargo.toml
index dc3967e..d55cc91 100644
--- a/crates/core-ffi/Cargo.toml
+++ b/crates/core-ffi/Cargo.toml
@@ -13,12 +13,14 @@ crate-type = ["staticlib", "cdylib", "lib"]
 [features]
 # `debug_panic`, `Session::debug_panic_locked`, `force_offline`,
 # `reset_host`, `drain_offline_log`, `simulate_event`, `hosts_created`,
-# `inject_reload_fault` and `engine_active`, for tests only.
+# `inject_reload_fault`, `engine_active`, `engine_workers`,
+# `add_test_bundle` and `closing_hosts`, for tests only.
 test-hooks = []
 
 [dependencies]
 core-model = { path = "../core-model" }
 core-engine = { path = "../core-engine", features = ["fixtures"] }
+core-host = { path = "../core-host" }
 thiserror = "2"
 uniffi = { version = "=0.32.2", default-features = false }
 
@@ -28,3 +30,4 @@ platform-macos = { path = "../platform-macos" }
 [dev-dependencies]
 core-ffi = { path = ".", features = ["test-hooks"] }
 tempfile = "3"
+test-plugins = { path = "../test-plugins" }
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test plugins
```

Expected: compile errors — unresolved `vizdaw_ffi::catalog`, `vizdaw_ffi::SlotRow`, `vizdaw_ffi::SlotStatus`, `closing_hosts_waiting`, no method `plugin_slots` / `idle` on `Session`.

- [ ] **Step 3: Implement the FFI side**

Create `crates/core-ffi/src/catalog.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/src
cat > crates/core-ffi/src/catalog.rs <<'VIZDAW_EOF'
//! The process-wide plugin catalog (spec §8, §9): where plugins live and
//! which are unavailable. Every session's plugin host is told through
//! [`apply`], with its own marks (one catalog, many hosts).

use crate::guard::lock;
use core_host::PluginHost;
use core_host::catalog::{Catalog, HostMarks};
use std::sync::Mutex;

static CATALOG: Mutex<Option<Catalog>> = Mutex::new(None);

/// Runs `f` on the catalog (empty until one is loaded).
pub fn with<T>(f: impl FnOnce(&mut Catalog) -> T) -> T {
    let mut g = lock(&CATALOG);
    f(g.get_or_insert_with(Catalog::new))
}

/// Tells `host` what the catalog knows (paths, unavailable plugins);
/// `marks` is what it told this host last time.
pub fn apply(host: &mut PluginHost, marks: &mut HostMarks) {
    with(|c| c.apply_to(host, marks));
}

/// Adds every plugin of the CLAP bundle at `path` as scanned OK, without
/// running the scanner (test hook).
#[cfg(feature = "test-hooks")]
pub fn add_test_bundle(path: &std::path::Path) -> Result<(), crate::VizError> {
    use core_host::catalog::{CatalogEntry, ScanStatus, Source};
    let src = Source::clap_bundle(path);
    let listed = core_host::clap::list_bundle(path).map_err(|e| crate::VizError::Engine {
        message: e.to_string(),
    })?;
    let entries = listed
        .into_iter()
        .map(|(plugin, kind)| CatalogEntry {
            source: src.clone(),
            plugin,
            kind: Some(kind),
            info: None,
            status: ScanStatus::Ok,
        })
        .collect();
    with(|c| c.record(&src, entries));
    crate::plugins::apply_catalog();
    Ok(())
}
VIZDAW_EOF
```
Create `crates/core-ffi/src/plugins.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/src
cat > crates/core-ffi/src/plugins.rs <<'VIZDAW_EOF'
//! Plugin hosts, one per session (spec §5, §11). A `PluginHost` is not
//! `Send` (CLAP and AppKit tie plugins to the thread that made them), so
//! each lives in a registry on the thread that created its session: the
//! app's main thread (tests: the test's thread). Sessions reach theirs
//! with [`with`]; a session used from another thread sees no plugin host
//! (Plan Ruling 1).

use crate::catalog;
use crate::error::VizError;
use crate::guard::lock;
use core_engine::backend::MAX_FRAMES;
use core_host::PluginHost;
use core_host::catalog::HostMarks;
use core_model::document::Document;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a closed session's host waits for its processors to come back
/// from the engine before it is dropped anyway.
pub const CLOSING_WAIT: Duration = Duration::from_secs(5);

/// Rate and block size new hosts activate plugins for: the last engine's,
/// else 48 kHz and the engine's default block (Plan Ruling 3).
static CONFIG: Mutex<(f64, u32)> = Mutex::new((48_000.0, MAX_FRAMES));

struct Entry {
    host: PluginHost,
    marks: HostMarks,
}

#[derive(Default)]
struct Registry {
    hosts: BTreeMap<u64, Entry>,
    /// Hosts of closed sessions, waiting for their processors.
    closing: Vec<(PluginHost, Instant)>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

/// The configuration new hosts start with.
pub fn config() -> (f64, u32) {
    *lock(&CONFIG)
}

/// Remembers the engine's configuration for hosts made later.
pub fn set_config(sample_rate: f64, max_frames: u32) {
    *lock(&CONFIG) = (sample_rate, max_frames);
}

/// Makes session `id`'s plugin host on this thread (AU and CLAP), told
/// what the catalog knows.
pub fn create(id: u64) -> Result<(), VizError> {
    let (rate, max) = config();
    let mut host = PluginHost::new(rate, max).map_err(|e| VizError::Engine {
        message: e.to_string(),
    })?;
    #[cfg(target_os = "macos")]
    host.add_format(Box::new(platform_macos::au::AuFormat::new()));
    let mut marks = HostMarks::default();
    catalog::apply(&mut host, &mut marks);
    REGISTRY.with(|r| r.borrow_mut().hosts.insert(id, Entry { host, marks }));
    Ok(())
}

/// Runs `f` with session `id`'s plugin host (`None` on another thread, or
/// after [`close`]). `f` must not call `with` again.
pub fn with<T>(id: u64, f: impl FnOnce(Option<&mut PluginHost>) -> T) -> T {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        f(r.hosts.get_mut(&id).map(|e| &mut e.host))
    })
}

/// Tells every host on this thread what the catalog knows now.
pub fn apply_catalog() {
    REGISTRY.with(|r| {
        for e in r.borrow_mut().hosts.values_mut() {
            catalog::apply(&mut e.host, &mut e.marks);
        }
    });
}

/// Whether session `id` has plugins loaded or processors still out.
pub fn has_plugins(id: u64) -> bool {
    with(id, |p| p.is_some_and(|p| p.live_count() + p.retiring() > 0))
}

/// Closes session `id`'s host: every slot is dropped (shelved processors
/// are deactivated now); a host whose processors are still in the engine
/// waits in the closing list.
pub fn close(id: u64) {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        if let Some(mut e) = r.hosts.remove(&id) {
            e.host.sync(&Document::new());
            e.host.idle();
            if e.host.retiring() > 0 {
                r.closing.push((e.host, Instant::now()));
            }
        }
    });
    tick_closing();
}

/// Services closed hosts; drops those whose processors all came back (or
/// that waited [`CLOSING_WAIT`]).
pub fn tick_closing() {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        for (h, _) in &mut r.closing {
            h.idle();
        }
        r.closing
            .retain(|(h, t)| h.retiring() > 0 && t.elapsed() < CLOSING_WAIT);
    });
}

/// Closed hosts still waiting (test hook).
#[cfg(feature = "test-hooks")]
pub fn closing_hosts() -> usize {
    REGISTRY.with(|r| r.borrow().closing.len())
}

/// The row of `slot` (an instrument's own slot when `instrument`), with
/// what `host` knows about its instance.
pub fn slot_row(
    slot: &core_model::mixer::PluginSlot,
    instrument: bool,
    host: Option<&mut PluginHost>,
) -> crate::snapshot::SlotRow {
    use crate::snapshot::{SlotRow, SlotStatus};
    use core_host::SlotState;
    let mut row = SlotRow {
        id: slot.id.0,
        plugin: (&slot.plugin).into(),
        instrument,
        bypass: slot.bypass,
        sidechain: slot.sidechain.map(|s| s.0),
        status: SlotStatus::Placeholder {
            reason: "no plugin host on this thread".into(),
        },
        latency: 0,
        has_sidechain_input: false,
        outputs: 0,
    };
    let Some(host) = host else {
        return row;
    };
    row.status = match host.state(slot.id) {
        None | Some(SlotState::Loading) => SlotStatus::Loading,
        Some(SlotState::Live) => SlotStatus::Live,
        Some(SlotState::Failed(reason)) => SlotStatus::Placeholder { reason },
    };
    if let Some(inst) = host.instance(slot.id) {
        let info = inst.info();
        row.latency = info.latency;
        row.has_sidechain_input = info.sidechain_input().is_some();
        row.outputs = u32::try_from(info.outputs.len()).unwrap_or(u32::MAX);
    }
    row
}

/// Every plugin slot of `doc`: instruments' own slots (by destination),
/// then inserts (by strip, in chain order).
pub fn slot_rows(
    doc: &Document,
    mut host: Option<&mut PluginHost>,
) -> Vec<crate::snapshot::SlotRow> {
    use core_model::document::DestKind;
    let mut rows = Vec::new();
    for d in doc.instruments().values() {
        if let DestKind::PluginInstrument { slot } = &d.kind {
            rows.push(slot_row(slot, true, host.as_deref_mut()));
        }
    }
    for (_, _, slot) in doc.mixer().inserts() {
        rows.push(slot_row(slot, false, host.as_deref_mut()));
    }
    rows
}
VIZDAW_EOF
```
Change `crates/core-ffi/src/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/host.rs b/crates/core-ffi/src/host.rs
index 90bd60e..d498c97 100644
--- a/crates/core-ffi/src/host.rs
+++ b/crates/core-ffi/src/host.rs
@@ -1,14 +1,19 @@
 //! The process-wide engine host: one engine, one output, one playing
-//! session at a time. Created lazily on the first Play.
+//! session at a time. Created lazily on the first Play. The engine plays
+//! the inserts and plugin instruments of the session that played last
+//! (its plugin host is the engine's insert factory, Plan Ruling 2).
 
 use crate::error::VizError;
 use crate::guard::lock;
+use crate::plugins;
 use core_engine::backend::{AudioBackend, BackendEvent};
 use core_engine::engine::{Engine, EngineConfig};
+use core_engine::graph::NoInserts;
 use core_engine::offline::{EventLog, OfflineBackend, OfflineSinks};
 use core_engine::render::Renderer;
 use core_engine::schedule::CompileReport;
 use core_engine::transport::PlayState;
+use core_host::{IdleReport, PluginHost};
 use core_model::document::Document;
 use core_model::ids::SeqId;
 use core_model::time::Tick;
@@ -19,6 +24,21 @@ use std::time::{Duration, Instant};
 
 /// Offline sample rate.
 pub const OFFLINE_RATE: f64 = 48_000.0;
+
+/// The offline rate in use (f64 bits): `OFFLINE_RATE` unless a test set
+/// another (`set_offline_rate`).
+static OFFLINE_RATE_BITS: AtomicU64 = AtomicU64::new(0x40E7_7000_0000_0000);
+
+fn offline_rate() -> f64 {
+    f64::from_bits(OFFLINE_RATE_BITS.load(Ordering::Relaxed))
+}
+
+/// Makes offline hosts created from now on run at `rate` (test hook: a
+/// device whose rate changed).
+#[cfg(feature = "test-hooks")]
+pub fn set_offline_rate(rate: f64) {
+    OFFLINE_RATE_BITS.store(rate.to_bits(), Ordering::Relaxed);
+}
 /// Offline block size.
 pub const OFFLINE_BLOCK: u32 = 512;
 /// Device label of the offline backend.
@@ -100,6 +120,18 @@ pub fn inject_reload_fault(f: Option<ReloadFault>) {
     *lock(&RELOAD_FAULT) = f;
 }
 
+/// Closed sessions' plugin hosts still waiting for processors (test hook).
+#[cfg(feature = "test-hooks")]
+pub fn closing_hosts_waiting() -> usize {
+    plugins::closing_hosts()
+}
+
+/// The live engine's worker count (test hook; `None` without a host).
+#[cfg(feature = "test-hooks")]
+pub fn engine_workers() -> Option<usize> {
+    lock(&HOST).as_ref().map(|h| h.engine.config().workers)
+}
+
 /// True when a host exists and its engine is not stopped (test hook).
 #[cfg(feature = "test-hooks")]
 pub fn engine_active() -> bool {
@@ -122,7 +154,8 @@ struct OfflineDriver {
 
 impl OfflineDriver {
     fn start(renderer: Renderer) -> Result<OfflineDriver, VizError> {
-        let mut backend = OfflineBackend::new(OFFLINE_RATE, OFFLINE_BLOCK);
+        let rate = offline_rate();
+        let mut backend = OfflineBackend::new(rate, OFFLINE_BLOCK);
         backend.start(renderer)?;
         let stop = Arc::new(AtomicBool::new(false));
         let flag = Arc::clone(&stop);
@@ -131,7 +164,7 @@ impl OfflineDriver {
             .spawn(move || {
                 let t0 = Instant::now();
                 while !flag.load(Ordering::Relaxed) {
-                    let due = (t0.elapsed().as_secs_f64() * OFFLINE_RATE) as u64;
+                    let due = (t0.elapsed().as_secs_f64() * rate) as u64;
                     if due > backend.frames() {
                         backend.run_frames(due - backend.frames());
                     }
@@ -172,6 +205,14 @@ struct Host {
     output: Output,
     engine: Engine,
     owner: u64,
+    /// The session whose plugin host is the engine's insert factory (0:
+    /// none).
+    factory_owner: u64,
+    /// The engine loaded a document with plugin slots since it was made:
+    /// its instances and plugin targets are keyed by that document's
+    /// instance IDs, so another session must not inherit them (Plan
+    /// Ruling 2).
+    plugins_in_engine: bool,
     seq: SeqId,
     looped: bool,
     device: String,
@@ -192,15 +233,10 @@ impl Host {
         if offline_mode() {
             let sinks = OfflineSinks::new(1 << 16);
             let log = sinks.log();
-            let (engine, renderer) = Engine::new(EngineConfig::new(OFFLINE_RATE), Box::new(sinks))?;
+            let rate = offline_rate();
+            let (engine, renderer) = Engine::new(EngineConfig::new(rate), Box::new(sinks))?;
             let output = Output::Offline(OfflineDriver::start(renderer)?);
-            return Ok(Host::new(
-                engine,
-                output,
-                OFFLINE_DEVICE,
-                OFFLINE_RATE,
-                Some(log),
-            ));
+            return Ok(Host::new(engine, output, OFFLINE_DEVICE, rate, Some(log)));
         }
         Host::create_live()
     }
@@ -209,13 +245,17 @@ impl Host {
     fn create_live() -> Result<Host, VizError> {
         use core_engine::backend::AudioConfig;
         use platform_macos::hal::CoreAudioBackend;
+        use platform_macos::rt::MacRtPlatform;
         use platform_macos::sinks::MacSinks;
+        use std::sync::Arc;
         let mut backend = CoreAudioBackend::new();
         let info = backend.open(&AudioConfig::default())?;
-        let (engine, renderer) = Engine::new(
-            EngineConfig::new(info.sample_rate),
+        let (engine, renderer) = Engine::with_platform(
+            live_config(info.sample_rate),
             Box::new(MacSinks::new()),
+            Arc::new(MacRtPlatform),
         )?;
+        engine.set_rt_env(rt_env(&backend, info.buffer_frames, info.sample_rate));
         backend.start(renderer)?;
         Ok(Host::new(
             engine,
@@ -244,6 +284,8 @@ impl Host {
             output,
             engine,
             owner: 0,
+            factory_owner: 0,
+            plugins_in_engine: false,
             seq: SeqId(0),
             looped: false,
             device: device.to_string(),
@@ -255,7 +297,29 @@ impl Host {
         }
     }
 
+    /// Makes `owner`'s plugin host the engine's insert factory (if it is
+    /// not already) and activates its plugins for this engine's rate and
+    /// block size.
+    fn attach(&mut self, owner: u64, plugins: Option<&mut PluginHost>) {
+        let block = self.engine.block_frames();
+        if self.factory_owner != owner {
+            match plugins.as_deref() {
+                Some(p) => self.engine.set_insert_factory(Box::new(p.factory())),
+                None => self.engine.set_insert_factory(Box::new(NoInserts)),
+            }
+            self.factory_owner = owner;
+        }
+        if let Some(p) = plugins {
+            let (rate, max) = p.config();
+            if rate != self.sample_rate || max < block {
+                p.reconfigure(self.sample_rate, block.max(max));
+            }
+        }
+        plugins::set_config(self.sample_rate, block);
+    }
+
     fn load(&mut self, doc: &Document, seq: SeqId, looped: bool) -> Result<(), VizError> {
+        self.plugins_in_engine |= has_slots(doc);
         let report = self.engine.load(doc, seq)?;
         self.report = report_lines(&report);
         self.engine.set_loop(looped)?;
@@ -299,17 +363,52 @@ impl Host {
                 if frames > r.config().max_frames {
                     r.prepare(frames);
                 }
-                Ok(b.start(r)?)
+                b.start(r)?;
+                // The workers follow the new block period.
+                self.engine.set_rt_env(rt_env(b, frames, self.sample_rate));
+                Ok(())
             }
         }
     }
 }
 
+/// Live blocks may use 80 % of their duration (3a Plan Ruling 18).
+pub const DEADLINE: f64 = 0.8;
+
+/// The live engine's settings: the spec's default worker count
+/// `min(performance cores − 1, 7)` (3a Plan Ruling 20) and the deadline.
+/// The offline backend keeps 0 workers and no deadline.
+#[cfg(target_os = "macos")]
+pub fn live_config(sample_rate: f64) -> EngineConfig {
+    let mut cfg = EngineConfig::new(sample_rate);
+    cfg.workers = platform_macos::rt::default_workers();
+    cfg.deadline = Some(DEADLINE);
+    cfg
+}
+
+/// The device's block period and workgroup, for the workers.
+#[cfg(target_os = "macos")]
+fn rt_env(
+    backend: &platform_macos::hal::CoreAudioBackend,
+    frames: u32,
+    sample_rate: f64,
+) -> core_engine::pool::RtEnv {
+    core_engine::pool::RtEnv {
+        period_ns: (f64::from(frames) * 1e9 / sample_rate.max(1.0)).round() as u64,
+        workgroup: backend.workgroup(),
+    }
+}
+
 /// Handles device events for the owner (mirrors `vizdaw-play`'s live
 /// loop): a buffer-size change restarts the output; a lost device, a new
 /// default device or a rate change rebuild output and engine, recompile,
-/// and resume at the same tick. A failed rebuild leaves no host.
-fn handle_events(slot: &mut Option<Host>, doc: &Document) -> Result<(), VizError> {
+/// and resume at the same tick. A failed rebuild leaves no host. The
+/// owner's plugins follow the new engine's rate (`attach`).
+fn handle_events(
+    slot: &mut Option<Host>,
+    doc: &Document,
+    plugins: Option<&mut PluginHost>,
+) -> Result<(), VizError> {
     let Some(h) = slot.as_mut() else {
         return Ok(());
     };
@@ -334,9 +433,11 @@ fn handle_events(slot: &mut Option<Host>, doc: &Document) -> Result<(), VizError
                     std::thread::sleep(DRAIN);
                 }
                 let (owner, seq, looped) = (old.owner, old.seq, old.looped);
-                // Output first (frees the renderer), then the engine.
+                // Output first (frees the renderer), then the engine: its
+                // processors go back to the plugin host.
                 drop(old);
                 let mut new = Host::create()?;
+                new.attach(owner, plugins);
                 new.load(doc, seq, looped)?;
                 new.engine.locate_tick(at)?;
                 if playing {
@@ -353,9 +454,20 @@ fn handle_events(slot: &mut Option<Host>, doc: &Document) -> Result<(), VizError
     Ok(())
 }
 
+/// The compile report as messages: warnings, unroutable events, then the
+/// mixer graph's (placeholders as "plugin N: why", other warnings, and why
+/// a graph was not used).
 fn report_lines(r: &CompileReport) -> Vec<String> {
     let mut lines = r.warnings.clone();
     lines.extend(r.unroutable.iter().map(|u| u.reason.clone()));
+    let g = &r.graph;
+    lines.extend(
+        g.placeholders
+            .iter()
+            .map(|(id, why)| format!("{id}: {why}")),
+    );
+    lines.extend(g.warnings.iter().cloned());
+    lines.extend(g.error.iter().map(|e| format!("mixer graph not used: {e}")));
     lines
 }
 
@@ -387,6 +499,7 @@ pub fn play(
     seq: SeqId,
     start: Option<Tick>,
     looped: bool,
+    mut plugins: Option<&mut PluginHost>,
 ) -> Result<(), VizError> {
     let mut slot = lock(&HOST);
     // Device events nobody polled (the host had no owner, or another one):
@@ -402,7 +515,15 @@ pub fn play(
             }
         }
     } else {
-        handle_events(&mut slot, doc)?;
+        handle_events(&mut slot, doc, plugins.as_deref_mut())?;
+    }
+    // Another session's plugins played here: a fresh engine, so none of
+    // its instances is taken for this document's (same-numbered) slots.
+    if slot
+        .as_ref()
+        .is_some_and(|h| h.factory_owner != owner && h.plugins_in_engine)
+    {
+        *slot = None;
     }
     if slot.is_none() {
         *slot = Some(Host::create()?);
@@ -410,6 +531,12 @@ pub fn play(
     let host = slot.as_mut().ok_or_else(|| VizError::Internal {
         message: "engine host missing".into(),
     })?;
+    host.attach(owner, plugins.as_deref_mut());
+    if let Some(p) = plugins {
+        // Slots still loading play as placeholders; `service` loads again
+        // once they are in.
+        p.sync(doc);
+    }
     if host.owner != owner || host.seq != seq || start.is_some() {
         host.engine.stop()?;
         host.load(doc, seq, looped)?;
@@ -457,8 +584,16 @@ pub fn set_loop(owner: u64, on: bool) -> Result<(), VizError> {
     }
 }
 
-/// Recompiles after an edit if `owner` holds the engine.
-pub fn reload(owner: u64, doc: &Document, looped: bool) -> Result<(), VizError> {
+/// After an edit: starts loading the plugins `doc` gained (and drops the
+/// ones it lost), then recompiles if `owner` holds the engine and no
+/// plugin is loading (otherwise `service` loads once they are in).
+pub fn reload(
+    owner: u64,
+    doc: &Document,
+    looped: bool,
+    plugins: Option<&mut PluginHost>,
+) -> Result<(), VizError> {
+    let ready = plugins.is_none_or(|p| p.sync(doc));
     let mut slot = lock(&HOST);
     match slot.as_mut() {
         Some(h) if h.owner == owner => {
@@ -480,6 +615,9 @@ pub fn reload(owner: u64, doc: &Document, looped: bool) -> Result<(), VizError>
                 h.owner = 0;
                 return Ok(());
             }
+            if !ready {
+                return Ok(());
+            }
             h.load(doc, seq, looped)
         }
         _ => Ok(()),
@@ -489,13 +627,17 @@ pub fn reload(owner: u64, doc: &Document, looped: bool) -> Result<(), VizError>
 /// `owner`'s view of the transport, or `None` when it does not hold the
 /// engine. For the owner, device events are handled first (`doc` is the
 /// owner's document, needed to recompile after a rebuild).
-pub fn state(owner: u64, doc: &Document) -> Result<Option<HostState>, VizError> {
+pub fn state(
+    owner: u64,
+    doc: &Document,
+    plugins: Option<&mut PluginHost>,
+) -> Result<Option<HostState>, VizError> {
     let mut slot = lock(&HOST);
     match slot.as_ref() {
         Some(h) if h.owner == owner => {}
         _ => return Ok(None),
     }
-    handle_events(&mut slot, doc)?;
+    handle_events(&mut slot, doc, plugins)?;
     let Some(h) = slot.as_mut() else {
         return Ok(None);
     };
@@ -521,3 +663,52 @@ pub fn release(owner: u64) {
         }
     }
 }
+
+/// The main-thread timer's plugin step for session `owner` (spec §5,
+/// 3b `PluginHost::service`): with the engine when `owner` plays through
+/// it, so finished loads, restarts and edits that waited reach it; else
+/// the host's own `idle`. An engine error is kept as a fault (the host
+/// retries once something changes).
+pub fn service(owner: u64, doc: &Document, plugins: &mut PluginHost) -> IdleReport {
+    let mut slot = lock(&HOST);
+    match slot.as_mut() {
+        Some(h) if h.owner == owner && h.factory_owner == owner => {
+            h.plugins_in_engine |= has_slots(doc);
+            match plugins.service(&mut h.engine, doc, h.seq) {
+                Ok(r) => {
+                    if let Some(c) = &r.compiled {
+                        h.report = report_lines(c);
+                    }
+                    r
+                }
+                Err(e) => {
+                    let f = format!("plugin load: {e}");
+                    if !h.faults.contains(&f) {
+                        h.faults.push(f);
+                    }
+                    IdleReport::default()
+                }
+            }
+        }
+        _ => plugins.idle(),
+    }
+}
+
+/// Whether `doc` has a plugin slot (an insert or a plugin instrument).
+fn has_slots(doc: &Document) -> bool {
+    doc.mixer().inserts().next().is_some()
+        || doc.instruments().values().any(|d| {
+            matches!(
+                d.kind,
+                core_model::document::DestKind::PluginInstrument { .. }
+            )
+        })
+}
+
+/// The session whose plugin host feeds the engine, if any.
+pub fn factory_owner() -> Option<u64> {
+    lock(&HOST)
+        .as_ref()
+        .map(|h| h.factory_owner)
+        .filter(|o| *o != 0)
+}
VIZDAW_EOF
```
Change `crates/core-ffi/src/session.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/session.rs b/crates/core-ffi/src/session.rs
index 7ca64ac..95600ae 100644
--- a/crates/core-ffi/src/session.rs
+++ b/crates/core-ffi/src/session.rs
@@ -3,10 +3,12 @@
 use crate::error::VizError;
 use crate::guard::{ffi_guard, lock};
 use crate::host;
+use crate::plugins;
 use crate::snapshot::{
-    DocSnapshot, LaneRow, TransportState, lane_rows, position_bbu, seconds, seq_id, seq_rows,
-    warnings,
+    DocSnapshot, IdleUpdate, LaneRow, SlotRow, TransportState, lane_rows, position_bbu, seconds,
+    seq_id, seq_rows, warnings,
 };
+use core_host::PluginHost;
 use core_model::command::Command;
 use core_model::command::lane::LaneCmd;
 use core_model::command::sequence::SequenceCmd;
@@ -70,10 +72,12 @@ impl Inner {
             .map(|r| SeqId(r.id))
     }
 
-    /// Recompiles the engine after an edit. A failure (or panic, which also
-    /// resets the host) never fails the edit: it becomes a sticky fault.
+    /// After an edit: the plugin host follows the document and the engine
+    /// recompiles. A failure (or panic, which also resets the host) never
+    /// fails the edit: it becomes a sticky fault.
     fn reload_host(&mut self, id: u64) {
-        let r = ffi_guard(|| host::reload(id, self.history.doc(), self.looped));
+        let (doc, looped) = (self.history.doc(), self.looped);
+        let r = ffi_guard(|| plugins::with(id, |p| host::reload(id, doc, looped, p)));
         self.reload_fault = match r {
             Ok(()) => None,
             Err(e) => {
@@ -139,11 +143,31 @@ impl std::fmt::Debug for Session {
 }
 
 impl Session {
-    fn with(doc: Document, path: Option<PathBuf>) -> Arc<Session> {
-        Arc::new(Session {
-            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
+    /// A session for `doc`; its plugins start loading on this thread.
+    fn with(doc: Document, path: Option<PathBuf>) -> Result<Arc<Session>, VizError> {
+        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
+        plugins::create(id)?;
+        plugins::with(id, |p| p.map(|p| p.sync(&doc)));
+        Ok(Arc::new(Session {
+            id,
             inner: Mutex::new(Inner::new(doc, path)),
-        })
+        }))
+    }
+
+    /// Commits plugin-UI gestures as undo steps (spec §4), then lets the
+    /// plugin host and the engine follow (the host must see the committed
+    /// document before any undo, 3b `PluginHost::commit_gestures`).
+    fn commit_gestures(&self, inner: &mut Inner, gestures: &[core_host::Gesture]) -> bool {
+        if gestures.is_empty() {
+            return false;
+        }
+        let r = PluginHost::commit_gestures(&mut inner.history, gestures);
+        inner.version += 1;
+        inner.reload_host(self.id);
+        if let Err(e) = r {
+            inner.reload_fault = Some(format!("plugin gesture refused: {e}"));
+        }
+        true
     }
 
     /// Applies one command as one undo step and recompiles if playing.
@@ -184,7 +208,7 @@ impl Session {
     /// An untitled, empty document.
     #[uniffi::constructor]
     pub fn new_empty() -> Result<Arc<Session>, VizError> {
-        ffi_guard(|| Ok(Session::with(Document::new(), None)))
+        ffi_guard(|| Session::with(Document::new(), None))
     }
 
     /// An untitled copy of the demo project.
@@ -192,7 +216,7 @@ impl Session {
     pub fn new_demo() -> Result<Arc<Session>, VizError> {
         ffi_guard(|| {
             let doc = core_engine::fixtures::demo_document()?;
-            Ok(Session::with(doc, None))
+            Session::with(doc, None)
         })
     }
 
@@ -205,7 +229,7 @@ impl Session {
                 return Err(VizError::NotFound { path });
             }
             let doc = file::load(&p)?;
-            Ok(Session::with(doc, Some(p)))
+            Session::with(doc, Some(p))
         })
     }
 
@@ -301,7 +325,10 @@ impl Session {
                 (Some(t), Some(s)) if s == id => Some(t),
                 _ => None,
             };
-            host::play(self.id, inner.doc(), id, start, inner.looped)?;
+            let looped = inner.looped;
+            plugins::with(self.id, |p| {
+                host::play(self.id, inner.doc(), id, start, looped, p)
+            })?;
             inner.reload_fault = None;
             inner.cursor = None;
             inner.cursor_seq = Some(id);
@@ -340,7 +367,8 @@ impl Session {
         self.transport_call(|| {
             let inner = lock(&self.inner);
             let doc = inner.doc();
-            Ok(match host::state(self.id, doc)? {
+            let state = plugins::with(self.id, |p| host::state(self.id, doc, p))?;
+            Ok(match state {
                 Some(mut h) => TransportState {
                     playing: h.playing,
                     seq: Some(h.seq.0),
@@ -376,9 +404,93 @@ impl Session {
     }
 }
 
+#[uniffi::export]
+impl Session {
+    /// The plugin timer's step (spec §5 `host_idle`; the app calls it about
+    /// 60 times a second on the main thread): finishes plugin loads, runs
+    /// plugin callbacks, lets the engine follow, and turns plugin-UI
+    /// gestures into undo steps.
+    pub fn idle(&self) -> Result<IdleUpdate, VizError> {
+        ffi_guard(|| {
+            plugins::tick_closing();
+            let mut inner = lock(&self.inner);
+            let report = {
+                let doc = inner.doc();
+                plugins::with(self.id, |p| p.map(|p| host::service(self.id, doc, p)))
+            };
+            let Some(r) = report else {
+                return Ok(IdleUpdate {
+                    snapshot: None,
+                    slots_changed: false,
+                    params_changed: Vec::new(),
+                    problems: Vec::new(),
+                });
+            };
+            let mut problems: Vec<String> = Vec::new();
+            for (id, why) in &r.failed {
+                problems.push(format!("{}: {why}", slot_name(inner.doc(), *id)));
+            }
+            for (id, why) in &r.rejected_state {
+                problems.push(format!(
+                    "{}: saved state refused ({why})",
+                    slot_name(inner.doc(), *id)
+                ));
+            }
+            let changed = self.commit_gestures(&mut inner, &r.gestures);
+            Ok(IdleUpdate {
+                snapshot: if changed {
+                    Some(inner.snapshot()?)
+                } else {
+                    None
+                },
+                slots_changed: !r.loaded.is_empty()
+                    || !r.failed.is_empty()
+                    || !r.restart.is_empty()
+                    || r.compiled.is_some(),
+                params_changed: r.params_changed.iter().map(|i| i.0).collect(),
+                problems,
+            })
+        })
+    }
+
+    /// Every plugin slot with its status: instruments' own slots, then
+    /// inserts.
+    pub fn plugin_slots(&self) -> Result<Vec<SlotRow>, VizError> {
+        ffi_guard(|| {
+            let inner = lock(&self.inner);
+            Ok(plugins::with(self.id, |p| {
+                plugins::slot_rows(inner.doc(), p)
+            }))
+        })
+    }
+}
+
+/// "Strip — Plugin" for messages about slot `id`.
+fn slot_name(doc: &Document, id: core_model::ids::PluginInstanceId) -> String {
+    use core_model::document::SlotOwner;
+    let plugin = doc
+        .plugin_slot(id)
+        .map_or_else(|| id.to_string(), |s| s.plugin.name.clone());
+    let place = match doc.find_slot(id) {
+        Some(SlotOwner::Insert(strip, _)) => doc.mixer().strip(strip).ok().map(|s| s.name.clone()),
+        Some(SlotOwner::Instrument(d)) => doc.instruments().get(&d).map(|d| d.name.clone()),
+        None => None,
+    };
+    match place {
+        Some(p) => format!("{p} — {plugin}"),
+        None => plugin,
+    }
+}
+
 #[cfg(feature = "test-hooks")]
 #[uniffi::export]
 impl Session {
+    /// The sample rate this session's plugins are activated for (test
+    /// hook).
+    pub fn debug_plugin_rate(&self) -> Result<f64, VizError> {
+        ffi_guard(|| Ok(plugins::with(self.id, |p| p.map_or(0.0, |p| p.config().0))))
+    }
+
     /// Panics while holding the session lock (test hook): poisons it.
     pub fn debug_panic_locked(&self) -> Result<(), VizError> {
         ffi_guard(|| -> Result<(), VizError> {
@@ -393,6 +505,14 @@ impl Session {
 
 impl Drop for Session {
     fn drop(&mut self) {
-        host::release(self.id);
+        // The engine may still hold this session's processors: when its
+        // plugin host feeds the engine, the engine goes first (it gives
+        // them back), then the plugin host.
+        if plugins::has_plugins(self.id) && host::factory_owner() == Some(self.id) {
+            host::reset();
+        } else {
+            host::release(self.id);
+        }
+        plugins::close(self.id);
     }
 }
VIZDAW_EOF
```
Change `crates/core-ffi/src/snapshot.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/snapshot.rs b/crates/core-ffi/src/snapshot.rs
index aaddef0..61c7c70 100644
--- a/crates/core-ffi/src/snapshot.rs
+++ b/crates/core-ffi/src/snapshot.rs
@@ -195,3 +195,122 @@ pub fn seconds(doc: &Document, seq: SeqId, tick: Tick) -> f64 {
     doc.sequence(seq)
         .map_or(0.0, |s| s.tempo.tick_to_seconds(tick))
 }
+
+/// A plugin format.
+#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
+pub enum PluginFormatTag {
+    /// Audio Unit (v2 or v3).
+    Au,
+    /// CLAP.
+    Clap,
+}
+
+impl From<core_model::mixer::PluginFormat> for PluginFormatTag {
+    fn from(f: core_model::mixer::PluginFormat) -> Self {
+        match f {
+            core_model::mixer::PluginFormat::Au => PluginFormatTag::Au,
+            core_model::mixer::PluginFormat::Clap => PluginFormatTag::Clap,
+        }
+    }
+}
+
+impl From<PluginFormatTag> for core_model::mixer::PluginFormat {
+    fn from(f: PluginFormatTag) -> Self {
+        match f {
+            PluginFormatTag::Au => core_model::mixer::PluginFormat::Au,
+            PluginFormatTag::Clap => core_model::mixer::PluginFormat::Clap,
+        }
+    }
+}
+
+/// Which plugin, as the app names it (spec §4 `PluginRef`).
+#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
+pub struct PluginRefRow {
+    /// Format.
+    pub format: PluginFormatTag,
+    /// AU `type/subtype/manufacturer`, or the CLAP id.
+    pub id: String,
+    /// Display name.
+    pub name: String,
+    /// Vendor.
+    pub vendor: String,
+    /// Version.
+    pub version: String,
+}
+
+impl From<&core_model::mixer::PluginRef> for PluginRefRow {
+    fn from(p: &core_model::mixer::PluginRef) -> Self {
+        PluginRefRow {
+            format: p.format.into(),
+            id: p.id.clone(),
+            name: p.name.clone(),
+            vendor: p.vendor.clone(),
+            version: p.version.clone(),
+        }
+    }
+}
+
+impl From<&PluginRefRow> for core_model::mixer::PluginRef {
+    fn from(p: &PluginRefRow) -> Self {
+        core_model::mixer::PluginRef {
+            format: p.format.into(),
+            id: p.id.clone(),
+            name: p.name.clone(),
+            vendor: p.vendor.clone(),
+            version: p.version.clone(),
+        }
+    }
+}
+
+/// Where a plugin slot stands.
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
+pub enum SlotStatus {
+    /// Loading.
+    Loading,
+    /// Running.
+    Live,
+    /// A placeholder (spec §9): it failed to load, is disabled, missing or
+    /// blocklisted, or safe mode keeps it off; audio passes through.
+    Placeholder {
+        /// Why, as shown in the slot and its window.
+        reason: String,
+    },
+}
+
+/// One plugin slot: an insert, or a plugin instrument's own slot.
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
+pub struct SlotRow {
+    /// Instance ID.
+    pub id: u64,
+    /// The plugin.
+    pub plugin: PluginRefRow,
+    /// A plugin instrument's own slot (not an insert).
+    pub instrument: bool,
+    /// Bypassed.
+    pub bypass: bool,
+    /// Strip feeding its sidechain input.
+    pub sidechain: Option<u64>,
+    /// Loading, running or a placeholder.
+    pub status: SlotStatus,
+    /// Latency in frames (running plugins).
+    pub latency: u32,
+    /// It has a sidechain input (only then the sidechain menu shows).
+    pub has_sidechain_input: bool,
+    /// Output ports (multi-output instruments have more than one).
+    pub outputs: u32,
+}
+
+/// What a plugin timer tick changed.
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
+pub struct IdleUpdate {
+    /// The document, when plugin-UI gestures became undo steps.
+    pub snapshot: Option<DocSnapshot>,
+    /// Slots finished loading, failed, restarted or came back: refetch
+    /// the mixer.
+    pub slots_changed: bool,
+    /// Slots whose parameter list or values changed outside a gesture.
+    pub params_changed: Vec<u64>,
+    /// New problems: plugins that failed to load or refused their saved
+    /// state.
+    pub problems: Vec<String>,
+}
VIZDAW_EOF
```
Change `crates/core-ffi/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/lib.rs b/crates/core-ffi/src/lib.rs
index 2db34f5..86e675e 100644
--- a/crates/core-ffi/src/lib.rs
+++ b/crates/core-ffi/src/lib.rs
@@ -16,16 +16,21 @@
 
 uniffi::setup_scaffolding!();
 
+pub mod catalog;
 pub mod error;
 pub mod guard;
 pub mod host;
+pub mod plugins;
 pub mod session;
 pub mod snapshot;
 
 pub use error::VizError;
 pub use guard::ffi_guard;
 pub use session::Session;
-pub use snapshot::{DocSnapshot, LaneRow, SeqRow, TransportState};
+pub use snapshot::{
+    DocSnapshot, IdleUpdate, LaneRow, PluginFormatTag, PluginRefRow, SeqRow, SlotRow, SlotStatus,
+    TransportState,
+};
 
 /// Panics inside the guard (test hook): returns `VizError::Internal`.
 #[cfg(feature = "test-hooks")]
VIZDAW_EOF
```

- [ ] **Step 4: Run the Rust tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi
```

Expected: `plugins` 8 passed (`a_projects_plugins_load_on_open_and_reach_the_engine`, `an_unknown_plugin_is_a_placeholder_with_its_reason`, `another_session_takes_the_engine_and_the_first_keeps_its_plugins`, `another_documents_plugins_never_play_for_same_numbered_slots`, `a_device_rebuild_hands_the_plugins_to_the_new_engine`, `a_sample_rate_change_reactivates_the_plugins_at_the_new_rate`, `closing_a_session_releases_its_plugins`, `the_live_engine_uses_the_default_worker_count`); the 2c suites (`transport` 16, `persistence`, `commands`, `lifecycle`, `errors`) pass unchanged.

- [ ] **Step 5: Write the failing Swift test and keep the fake session in step**

Create `app/Tests/VizDAWAppModelTests/PluginIdleTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/PluginIdleTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// The plugin timer's step: gestures become undo steps, slot changes bump
/// the slot version, problems go to the fault popover (never an alert).
@MainActor
@Suite struct PluginIdleTests {
    @Test func aGestureSnapshotIsApplied() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        var next = f.snap
        next.version += 1
        next.undoLabel = "Change Plugin Parameter"
        f.idleUpdates = [
            IdleUpdate(snapshot: next, slotsChanged: false, paramsChanged: [7], problems: [])
        ]
        m.idle()
        #expect(m.undoTitle == "Undo Change Plugin Parameter")
        #expect(m.paramsChanged == [7])
        #expect(m.slotsVersion == 0)
    }

    @Test func slotChangesAndProblemsAreKept() throws {
        let f = demoFake()
        let m = try DocumentModel(session: f)
        f.idleUpdates = [
            IdleUpdate(
                snapshot: nil, slotsChanged: true, paramsChanged: [],
                problems: ["Strip — Diva: failed: load failed"])
        ]
        m.idle()
        m.idle()
        #expect(f.idleCalls == 2)
        #expect(m.slotsVersion == 1)
        #expect(m.alert == nil)
        #expect(m.hasReport)
        #expect(m.messages == ["Strip — Diva: failed: load failed"])
    }
}
VIZDAW_EOF
```
Change `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Tests/VizDAWAppModelTests/FakeSession.swift b/app/Tests/VizDAWAppModelTests/FakeSession.swift
index dc1941a..f3df8cf 100644
--- a/app/Tests/VizDAWAppModelTests/FakeSession.swift
+++ b/app/Tests/VizDAWAppModelTests/FakeSession.swift
@@ -15,6 +15,10 @@ final class FakeSession: SessionProtocol, @unchecked Sendable {
     var renames: [(UInt64, String)] = []
     var mutes: [(seq: UInt64, lane: UInt64, mute: Bool)] = []
     var nextSnapshot: DocSnapshot?
+    /// What the next `idle()` calls return, oldest first.
+    var idleUpdates: [IdleUpdate] = []
+    var idleCalls = 0
+    var slots: [SlotRow] = []
 
     init(_ sequences: [SeqRow], version: UInt64 = 0) {
         snap = DocSnapshot(
@@ -83,6 +87,18 @@ final class FakeSession: SessionProtocol, @unchecked Sendable {
         return state
     }
     func undo() throws -> DocSnapshot { commandResult() }
+
+    func idle() throws -> IdleUpdate {
+        idleCalls += 1
+        if let u = idleUpdates.first {
+            idleUpdates.removeFirst()
+            if let s = u.snapshot { snap = s }
+            return u
+        }
+        return IdleUpdate(snapshot: nil, slotsChanged: false, paramsChanged: [], problems: [])
+    }
+
+    func pluginSlots() throws -> [SlotRow] { slots }
 }
 
 func seq(_ id: UInt64, _ name: String, topLevel: Bool, uses: UInt32 = 0) -> SeqRow {
VIZDAW_EOF
```

- [ ] **Step 6: Run it to see it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh
scripts/swift.sh test
```

Expected: compile errors — `DocumentModel` has no member `idle`, `paramsChanged`, `slotsVersion`, `messages`.

- [ ] **Step 7: Implement the Swift side**

Change `app/Sources/VizDAWAppModel/DocumentModel.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAWAppModel/DocumentModel.swift b/app/Sources/VizDAWAppModel/DocumentModel.swift
index 8ce4719..3a8fa8e 100644
--- a/app/Sources/VizDAWAppModel/DocumentModel.swift
+++ b/app/Sources/VizDAWAppModel/DocumentModel.swift
@@ -25,6 +25,14 @@ public final class DocumentModel {
     private var lanesKey: LanesKey?
     /// How often `lanes` were fetched (tests check refetch rules).
     public private(set) var laneFetches = 0
+    /// Plugins that failed to load or refused their state (newest last).
+    public private(set) var pluginProblems: [String] = []
+    /// Bumped when plugin slots change (loaded, failed, restarted): views
+    /// that show slots refetch.
+    public private(set) var slotsVersion = 0
+    /// Slots whose parameters changed outside a gesture (their windows
+    /// refetch), cleared by whoever handles them.
+    public var paramsChanged: Set<UInt64> = []
 
     private struct LanesKey: Equatable {
         let version: UInt64
@@ -55,7 +63,10 @@ public final class DocumentModel {
     /// 33 ms while playing, 250 ms when stopped (to catch the end).
     public var pollInterval: Duration { isPlaying ? .milliseconds(33) : .milliseconds(250) }
     public var hasFaults: Bool { !transport.faults.isEmpty }
-    public var hasReport: Bool { !transport.report.isEmpty }
+    public var hasReport: Bool { !transport.report.isEmpty || !pluginProblems.isEmpty }
+    /// Every line of the fault popover: faults, compile messages, plugin
+    /// problems.
+    public var messages: [String] { transport.faults + transport.report + pluginProblems }
     public var selectedName: String? {
         snapshot.sequences.first { $0.id == selectedSeq }?.name
     }
@@ -144,6 +155,20 @@ public final class DocumentModel {
         }
     }
 
+    /// The plugin timer's step (about 60 times a second, spec §5): plugin
+    /// loads finish, plugin-UI gestures become undo steps. Failures go to
+    /// the fault popover, not to alerts.
+    public func idle() {
+        guard let u = try? session.idle() else { return }
+        if let s = u.snapshot { apply(s) }
+        if u.slotsChanged { slotsVersion += 1 }
+        paramsChanged.formUnion(u.paramsChanged)
+        if !u.problems.isEmpty {
+            pluginProblems.append(contentsOf: u.problems)
+            if pluginProblems.count > 50 { pluginProblems.removeFirst(pluginProblems.count - 50) }
+        }
+    }
+
     /// True when Save needs a path first (untitled).
     public var needsSaveAs: Bool { ((try? session.path()) ?? nil) == nil }
 
VIZDAW_EOF
```
Change `app/Sources/VizDAW/DocumentView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/DocumentView.swift b/app/Sources/VizDAW/DocumentView.swift
index 56965a1..a80af47 100644
--- a/app/Sources/VizDAW/DocumentView.swift
+++ b/app/Sources/VizDAW/DocumentView.swift
@@ -149,8 +149,7 @@ struct TransportToolbar: ToolbarContent {
                 .help("Faults and compile messages")
                 .popover(isPresented: $model.showsFaults) {
                     VStack(alignment: .leading, spacing: 4) {
-                        let lines = model.transport.faults + model.transport.report
-                        ForEach(Array(lines.enumerated()), id: \.offset) {
+                        ForEach(Array(model.messages.enumerated()), id: \.offset) {
                             Text($0.element)
                         }
                     }
VIZDAW_EOF
```
Change `app/Sources/VizDAW/VizDAWApp.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/VizDAWApp.swift b/app/Sources/VizDAW/VizDAWApp.swift
index 2caf64a..2471c1b 100644
--- a/app/Sources/VizDAW/VizDAWApp.swift
+++ b/app/Sources/VizDAW/VizDAWApp.swift
@@ -29,6 +29,7 @@ final class AppDelegate: NSObject, NSApplicationDelegate {
         NSApp.setActivationPolicy(.regular)
         NSApp.activate()
         StoreAlerts.watch(appStore)
+        PluginTicker.start(appStore)
     }
 
     func application(_ application: NSApplication, open urls: [URL]) {
@@ -38,6 +39,25 @@ final class AppDelegate: NSObject, NSApplicationDelegate {
     }
 }
 
+/// The main-thread plugin timer (spec §5 `host_idle`, 60 Hz): every open
+/// document's plugins load, run their callbacks and report gestures here.
+@MainActor
+enum PluginTicker {
+    private static var timer: Timer?
+
+    static func start(_ store: DocumentStore) {
+        guard timer == nil else { return }
+        let t = Timer(timeInterval: 1.0 / 60.0, repeats: true) { _ in
+            MainActor.assumeIsolated {
+                for model in store.documents.values { model.idle() }
+            }
+        }
+        // Common modes: keeps ticking while a menu or a drag tracks.
+        RunLoop.main.add(t, forMode: .common)
+        timer = t
+    }
+}
+
 /// `VizDAW [--demo] [--smoke] [path.vizdaw ...]`: documents for the first
 /// windows. `--smoke` (implies `--demo`) plays the first window for 3 s,
 /// prints the transport and exits 0 if the position advanced, else 1.
VIZDAW_EOF
```

- [ ] **Step 8: Run the Swift tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build
scripts/swift.sh test
```

Expected: build complete; 36 tests in 5 suites pass (`PluginIdleTests` 2 new).

- [ ] **Step 9: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every Rust test passes (525); the Swift suite passes.

- [ ] **Step 10: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/Cargo.toml crates/core-ffi/src/catalog.rs crates/core-ffi/src/plugins.rs \
  crates/core-ffi/src/host.rs crates/core-ffi/src/session.rs crates/core-ffi/src/snapshot.rs \
  crates/core-ffi/src/lib.rs crates/core-ffi/tests/plugins.rs \
  app/Sources/VizDAWAppModel/DocumentModel.swift app/Sources/VizDAW/DocumentView.swift \
  app/Sources/VizDAW/VizDAWApp.swift app/Tests/VizDAWAppModelTests/PluginIdleTests.swift \
  app/Tests/VizDAWAppModelTests/FakeSession.swift Cargo.lock
git commit -m "$(cat <<'MSG'
feat(core-ffi): sessions host their plugins; plugin timer

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 5: The mixer through the FFI — read model, edits, drags, levels, capture before removal


**Files:**
- Modify: `crates/core-model/src/history.rs` (`peek_undo`, `peek_redo`)
- Test: `crates/core-model/tests/history.rs` (one test appended)
- Create: `crates/core-ffi/src/mixer.rs`
- Modify: `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/host.rs`, `crates/core-ffi/src/lib.rs`
- Create: `crates/core-ffi/tests/mixer.rs`
- Test: `app/Tests/VizDAWAppModelTests/FakeSession.swift` (the new session methods)

**Interfaces:**
- Consumes: Task 4's `plugins::{with, slot_row}`, `SlotRow`; `core_model::command::{MixerCmd, RoutingCmd, apply}`, `mixer_rules::audible`, `Engine::{meters, dsp_load, latency_frames, plugin_faults}`.
- Produces:
  - `History::peek_undo(&self) -> Option<&Document>`, `History::peek_redo(&self) -> Option<&Document>`.
  - FFI records `StripKindRow::{Instrument { dest }, InstrumentOutput { dest, index }, BuiltinSynth, AudioPath { path }, Bus, Master}`, `SendRow { target, level_db, pre_fader, enabled }`, `StripRow { id, name, kind, kind_label, instrument: Option<SlotRow>, inserts: Vec<SlotRow>, gain_db, pan, mute, solo, solo_muted, sends, output: Option<u64>, free_outputs: Vec<u16> }`, `BusRow { id, name }`, `MixerSnapshot { version, strips (display order, master last), buses }`, `MeterRow { strip, peak_l, peak_r }`, `MixerLevels { meters, dsp_load, dsp_peak, latency_frames, faulted: Vec<u64> }`.
  - `vizdaw_ffi::mixer::{snapshot(&Document, u64, Option<&mut PluginHost>) -> MixerSnapshot, slot_ids(&Document) -> BTreeSet<PluginInstanceId>}`; `host::levels(u64) -> Option<MixerLevels>`.
  - `Session` methods (all `Result<…, VizError>`): `mixer() -> MixerSnapshot`, `levels() -> MixerLevels`, `begin_edit(label: String)`, `end_edit() -> DocSnapshot`, and, each returning `DocSnapshot`: `add_bus(name)`, `remove_bus(strip)`, `rename_strip(strip, name)`, `move_strip(strip, index: u32)`, `set_gain(strip, gain_db)`, `set_pan(strip, pan)`, `set_strip_mute(strip, mute)`, `set_strip_solo(strip, solo)`, `set_output(strip, bus: Option<u64>)`, `add_send(strip, target)`, `edit_send(strip, index: u32, send: SendRow)`, `remove_send(strip, index: u32)`, `add_insert(strip, index: Option<u32>, plugin: PluginRefRow)`, `remove_insert(slot)`, `move_insert(slot, strip, index: u32)`, `set_bypass(slot, bypass)`, `set_sidechain(slot, source: Option<u64>)`, `add_instrument_output(dest, index: u16, name)`, `add_plugin_instrument(name, plugin: PluginRefRow)`, `set_destination_plugin(dest, plugin: PluginRefRow)` (an existing destination now plays a plugin instrument: `EditDestination`, its default patch kept); `destinations() -> Vec<DestRow>` with record `DestRow { id, name, plugin: Option<PluginRefRow> }`; test hook `debug_slot_state(slot) -> Option<u64>`.

Rulings 8, 22, 25 and 26. `execute` dry-runs the command (`core_model::command::apply`) and captures every running plugin the result no longer has before it commits; `step` does the same with `peek_undo`/`peek_redo`. `solo_muted` is "not in `audible`". `free_outputs` lists the outputs (≥ 1) of a running multi-output instrument that have no strip ("Add Outputs…"). `set_destination_plugin` is how the app makes a lane play a plugin instrument (the app has no lane-routing UI yet): every lane of that destination then plays it (Ruling 28). `levels` is empty for a session that does not hold the engine; fault-bypassed instances are kept per engine.

- [ ] **Step 1: Write the failing tests**

Change `crates/core-model/tests/history.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-model/tests/history.rs b/crates/core-model/tests/history.rs
index ae9afd3..cfe746b 100644
--- a/crates/core-model/tests/history.rs
+++ b/crates/core-model/tests/history.rs
@@ -281,3 +281,22 @@ fn redo_is_refused_inside_an_open_transaction() {
     assert_eq!(name(&h), "A");
     assert_eq!(h.undo_depth(), 1);
 }
+
+#[test]
+fn peeking_shows_what_undo_and_redo_would_bring_back() {
+    let mut h = hist();
+    assert!(h.peek_undo().is_none() && h.peek_redo().is_none());
+    h.execute(&rename("A"), &f()).unwrap();
+    let before = h.peek_undo().unwrap().clone();
+    assert_eq!(before.sequence(SeqId(0)).unwrap().name, "S");
+    h.undo().unwrap();
+    assert_eq!(h.doc(), &before);
+    assert_eq!(h.peek_redo().unwrap().sequence(SeqId(0)).unwrap().name, "A");
+    // Inside a transaction: nothing to redo, and only its own steps undo.
+    h.begin("T").unwrap();
+    assert!(h.peek_redo().is_none());
+    assert!(h.peek_undo().is_none());
+    h.execute(&rename("B"), &f()).unwrap();
+    assert_eq!(h.peek_undo().unwrap().sequence(SeqId(0)).unwrap().name, "S");
+    h.commit().unwrap();
+}
VIZDAW_EOF
```
Create `crates/core-ffi/tests/mixer.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/mixer.rs <<'VIZDAW_EOF'
//! The mixer through the FFI (spec §11): the read model, edits as undo
//! steps, drags as one step, routing refusals, solo, levels, and plugin
//! state captured before a plugin goes. The engine host is process-wide,
//! so these tests run one at a time.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    PluginFormatTag, PluginRefRow, SendRow, Session, SlotStatus, StripKindRow, StripRow, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

fn test_plugin(id: &str, name: &str) -> PluginRefRow {
    PluginRefRow {
        format: PluginFormatTag::Clap,
        id: id.into(),
        name: name.into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

fn strip(s: &Session, name: &str) -> StripRow {
    s.mixer()
        .unwrap()
        .strips
        .into_iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no strip {name}"))
}

fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let start = Instant::now();
    while !done(s) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn demo() -> Arc<Session> {
    Session::new_demo().unwrap()
}

/// The state blob slot `id` has in the document (None: never captured).
fn saved_state(s: &Session, id: u64) -> Option<u64> {
    s.debug_slot_state(id).unwrap()
}

#[test]
fn the_demo_mixer_lists_its_strips_master_last() {
    let _g = serial();
    let s = demo();
    let m = s.mixer().unwrap();
    let names: Vec<&str> = m.strips.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Built-in Synth", "Master"]);
    assert_eq!(m.strips[0].kind, StripKindRow::BuiltinSynth);
    assert_eq!(m.strips[0].kind_label, "Built-in Synth");
    assert_eq!(m.strips[1].kind, StripKindRow::Master);
    assert_eq!(m.strips[0].output, None);
    assert!(m.buses.is_empty());
    assert!(
        m.strips
            .iter()
            .all(|r| !r.solo_muted && r.inserts.is_empty())
    );
}

#[test]
fn mixer_edits_are_undo_steps() {
    let _g = serial();
    let s = demo();
    let snap = s.add_bus("Reverb".into()).unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Add Bus"));
    let m = s.mixer().unwrap();
    assert_eq!(m.version, snap.version);
    let bus = m.buses[0].id;
    assert_eq!(m.buses[0].name, "Reverb");
    let synth = strip(&s, "Built-in Synth").id;
    s.add_send(synth, bus).unwrap();
    s.edit_send(
        synth,
        0,
        SendRow {
            target: bus,
            level_db: -6.0,
            pre_fader: true,
            enabled: true,
        },
    )
    .unwrap();
    s.set_output(synth, Some(bus)).unwrap();
    let row = strip(&s, "Built-in Synth");
    assert_eq!(row.output, Some(bus));
    assert_eq!(row.sends.len(), 1);
    assert_eq!(
        (row.sends[0].level_db, row.sends[0].pre_fader),
        (-6.0, true)
    );
    s.set_pan(synth, -0.5).unwrap();
    s.rename_strip(bus, "Hall".into()).unwrap();
    assert_eq!(strip(&s, "Hall").kind_label, "Bus");
    for _ in 0..5 {
        s.undo().unwrap();
    }
    let row = strip(&s, "Built-in Synth");
    assert!(row.sends.is_empty() && row.output.is_none() && row.pan == 0.0);
    // Removing a bus retargets what used it, in one step.
    s.redo().unwrap();
    s.redo().unwrap();
    s.redo().unwrap();
    s.remove_bus(bus).unwrap();
    let row = strip(&s, "Built-in Synth");
    assert!(row.output.is_none() && row.sends.is_empty());
}

#[test]
fn a_routing_cycle_is_refused_and_changes_nothing() {
    let _g = serial();
    let s = demo();
    s.add_bus("A".into()).unwrap();
    s.add_bus("B".into()).unwrap();
    let (a, b) = (strip(&s, "A").id, strip(&s, "B").id);
    s.set_output(a, Some(b)).unwrap();
    let before = s.mixer().unwrap();
    let e = s.set_output(b, Some(a)).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    assert!(s.add_send(b, a).is_err());
    assert_eq!(s.mixer().unwrap(), before);
}

#[test]
fn a_drag_is_one_undo_step() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    s.begin_edit("Fader".into()).unwrap();
    for db in [-1.0, -3.0, -9.5] {
        s.set_gain(synth, db).unwrap();
        assert_eq!(strip(&s, "Built-in Synth").gain_db, db, "heard at once");
    }
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Fader"));
    s.undo().unwrap();
    assert_eq!(strip(&s, "Built-in Synth").gain_db, 0.0);
    assert!(s.snapshot().unwrap().undo_label.is_none());
    // An end without a begin changes nothing.
    s.end_edit().unwrap();
    assert!(s.set_gain(synth, 13.0).is_err(), "above +12 dB");
}

#[test]
fn solo_mutes_the_strips_it_does_not_hear() {
    let _g = serial();
    let s = demo();
    s.add_bus("A".into()).unwrap();
    s.add_bus("B".into()).unwrap();
    s.set_strip_solo(strip(&s, "A").id, true).unwrap();
    assert!(!strip(&s, "A").solo_muted);
    assert!(strip(&s, "B").solo_muted);
    assert!(strip(&s, "Built-in Synth").solo_muted);
    assert!(!strip(&s, "Master").solo_muted, "downstream of A");
    s.set_strip_mute(strip(&s, "B").id, true).unwrap();
    assert!(strip(&s, "B").mute);
}

#[test]
fn levels_are_for_the_session_that_plays() {
    let _g = serial();
    let a = demo();
    let b = demo();
    let song = a.snapshot().unwrap().sequences[2].id;
    assert!(a.levels().unwrap().meters.is_empty());
    a.play(song).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    let l = a.levels().unwrap();
    let strips: Vec<u64> = l.meters.iter().map(|m| m.strip).collect();
    assert!(strips.contains(&strip(&a, "Built-in Synth").id), "{l:?}");
    assert!(strips.contains(&strip(&a, "Master").id), "{l:?}");
    assert!(l.dsp_peak >= 0.0 && l.faulted.is_empty());
    assert!(b.levels().unwrap().meters.is_empty(), "b does not play");
}

#[test]
fn a_removed_plugin_keeps_its_state_for_undo_and_redo() {
    let _g = serial();
    let s = demo();
    let synth = strip(&s, "Built-in Synth").id;
    s.add_insert(synth, None, test_plugin("vizdaw.test.gain", "Test Gain"))
        .unwrap();
    let id = strip(&s, "Built-in Synth").inserts[0].id;
    idle_until(&s, |s| {
        strip(s, "Built-in Synth").inserts[0].status == SlotStatus::Live
    });
    assert_eq!(saved_state(&s, id), None, "never captured yet");
    s.remove_insert(id).unwrap();
    assert!(strip(&s, "Built-in Synth").inserts.is_empty());
    s.undo().unwrap();
    let back = strip(&s, "Built-in Synth").inserts[0].clone();
    assert_eq!(back.id, id);
    assert!(saved_state(&s, id).is_some(), "captured before the removal");
    idle_until(&s, |s| {
        strip(s, "Built-in Synth").inserts[0].status == SlotStatus::Live
    });
    // Undoing the add removes it too: captured for the redo.
    s.undo().unwrap();
    assert!(strip(&s, "Built-in Synth").inserts.is_empty());
    s.redo().unwrap();
    assert!(saved_state(&s, id).is_some());
}

#[test]
fn plugin_instruments_bring_strips_and_free_outputs() {
    let _g = serial();
    let s = demo();
    s.add_plugin_instrument(
        "Keys".into(),
        test_plugin("vizdaw.test.synth4", "Test Synth 4-Out"),
    )
    .unwrap();
    let keys = strip(&s, "Keys");
    let StripKindRow::Instrument { dest } = keys.kind else {
        panic!("{:?}", keys.kind)
    };
    assert_eq!(keys.kind_label, "Keys");
    assert!(keys.instrument.as_ref().unwrap().instrument);
    idle_until(&s, |s| {
        strip(s, "Keys").instrument.unwrap().status == SlotStatus::Live
    });
    let keys = strip(&s, "Keys");
    assert_eq!(keys.instrument.as_ref().unwrap().outputs, 4);
    assert_eq!(keys.free_outputs, vec![1, 2, 3]);
    s.add_instrument_output(dest, 1, "Keys 2".into()).unwrap();
    let out = strip(&s, "Keys 2");
    assert_eq!(out.kind, StripKindRow::InstrumentOutput { dest, index: 1 });
    assert_eq!(out.kind_label, "Keys out 2");
    assert_eq!(strip(&s, "Keys").free_outputs, vec![2, 3]);
}

#[test]
fn inserts_reorder_bypass_and_take_a_sidechain() {
    let _g = serial();
    let s = demo();
    s.add_bus("Kick".into()).unwrap();
    let synth = strip(&s, "Built-in Synth").id;
    s.add_insert(synth, None, test_plugin("vizdaw.test.gain", "Test Gain"))
        .unwrap();
    s.add_insert(
        synth,
        None,
        test_plugin("vizdaw.test.ducker", "Test Ducker"),
    )
    .unwrap();
    idle_until(&s, |s| {
        strip(s, "Built-in Synth")
            .inserts
            .iter()
            .all(|i| i.status == SlotStatus::Live)
    });
    let ins = strip(&s, "Built-in Synth").inserts;
    let (gain, ducker) = (ins[0].clone(), ins[1].clone());
    assert!(!gain.has_sidechain_input);
    assert!(
        ducker.has_sidechain_input,
        "the ducker has a sidechain port"
    );
    s.set_sidechain(ducker.id, Some(strip(&s, "Kick").id))
        .unwrap();
    s.set_bypass(gain.id, true).unwrap();
    s.move_insert(ducker.id, synth, 0).unwrap();
    let ins = strip(&s, "Built-in Synth").inserts;
    assert_eq!(
        ins.iter().map(|i| i.id).collect::<Vec<_>>(),
        [ducker.id, gain.id]
    );
    assert_eq!(ins[0].sidechain, Some(strip(&s, "Kick").id));
    assert!(ins[1].bypass);
    // Moving keeps the running instance.
    assert_eq!(ins[0].status, SlotStatus::Live);
}

#[test]
fn a_destination_can_play_a_plugin_instrument() {
    let _g = serial();
    let s = demo();
    let dests = s.destinations().unwrap();
    let piano = dests
        .iter()
        .find(|d| d.name == "Built-in Piano")
        .unwrap()
        .clone();
    assert!(piano.plugin.is_none(), "a MIDI destination");
    let snap = s
        .set_destination_plugin(
            piano.id,
            test_plugin("vizdaw.test.synth4", "Test Synth 4-Out"),
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Edit Destination"));
    let now = s.destinations().unwrap();
    let piano = now.iter().find(|d| d.id == piano.id).unwrap();
    assert_eq!(piano.plugin.as_ref().unwrap().id, "vizdaw.test.synth4");
    // Its instrument strip appears and the plugin loads.
    assert_eq!(
        strip(&s, "Built-in Piano").kind,
        StripKindRow::Instrument { dest: piano.id }
    );
    idle_until(&s, |s| {
        strip(s, "Built-in Piano").instrument.unwrap().status == SlotStatus::Live
    });
    assert!(
        s.set_destination_plugin(999, test_plugin("x", "x"))
            .is_err()
    );
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test history peeking
cargo test -p core-ffi --test mixer
```

Expected: compile errors — no method `peek_undo` on `History`; unresolved `vizdaw_ffi::{SendRow, StripKindRow, StripRow}`, no method `mixer`/`add_bus`/… on `Session`.

- [ ] **Step 3: Implement**

Change `crates/core-model/src/history.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-model/src/history.rs b/crates/core-model/src/history.rs
index 906ff31..8e6f720 100644
--- a/crates/core-model/src/history.rs
+++ b/crates/core-model/src/history.rs
@@ -113,6 +113,25 @@ impl History {
         self.future.last().map(|v| v.label.as_str())
     }
 
+    /// The document [`History::undo`] would go back to (inside a
+    /// transaction, only its own steps can be undone).
+    pub fn peek_undo(&self) -> Option<&Document> {
+        let floor = self.txn.as_ref().map_or(0, |t| t.start);
+        if self.past.len() <= floor {
+            return None;
+        }
+        self.past.last().map(|v| &v.doc)
+    }
+
+    /// The document [`History::redo`] would bring back (none while a
+    /// transaction is open).
+    pub fn peek_redo(&self) -> Option<&Document> {
+        if self.txn.is_some() {
+            return None;
+        }
+        self.future.last().map(|v| &v.doc)
+    }
+
     /// Whether a transaction is open.
     pub fn in_transaction(&self) -> bool {
         self.txn.is_some()
VIZDAW_EOF
```
Create `crates/core-ffi/src/mixer.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/src
cat > crates/core-ffi/src/mixer.rs <<'VIZDAW_EOF'
//! The mixer read model (spec §11): strips with their slots, sends and
//! routing, and the levels the mixer window polls at 30 Hz.

use crate::plugins::slot_row;
use crate::snapshot::SlotRow;
use core_host::PluginHost;
use core_model::document::{DestKind, Document};
use core_model::mixer::{Output, Strip, StripKind};
use core_model::mixer_rules::audible;
use std::collections::BTreeSet;

/// What feeds a strip.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StripKindRow {
    /// A plugin instrument's main output.
    Instrument {
        /// Its destination.
        dest: u64,
    },
    /// Extra output `index` (≥ 1) of a plugin instrument.
    InstrumentOutput {
        /// Its destination.
        dest: u64,
        /// Output index.
        index: u16,
    },
    /// The built-in synth.
    BuiltinSynth,
    /// An audio output path (silent until sub-project 5).
    AudioPath {
        /// The output path.
        path: u64,
    },
    /// An aux bus.
    Bus,
    /// The master.
    Master,
}

/// An aux send.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SendRow {
    /// The bus it feeds.
    pub target: u64,
    /// Level in dB (−144 is −∞).
    pub level_db: f64,
    /// Tapped before the fader.
    pub pre_fader: bool,
    /// On.
    pub enabled: bool,
}

/// One mixer strip.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct StripRow {
    /// Strip ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// What feeds it.
    pub kind: StripKindRow,
    /// "Bus", "Master", "Built-in Synth", the instrument, "‹instrument›
    /// out 3" or "Audio: ‹output›".
    pub kind_label: String,
    /// The plugin instrument's own slot (instrument strips only).
    pub instrument: Option<SlotRow>,
    /// Inserts, in processing order.
    pub inserts: Vec<SlotRow>,
    /// Fader in dB (−144 is −∞).
    pub gain_db: f64,
    /// Pan −1 … +1 (not applied on master).
    pub pan: f64,
    /// Muted.
    pub mute: bool,
    /// Soloed.
    pub solo: bool,
    /// Silent because other strips are soloed.
    pub solo_muted: bool,
    /// Sends.
    pub sends: Vec<SendRow>,
    /// The bus its output goes to; `None` is master (and master itself).
    pub output: Option<u64>,
    /// Outputs of its multi-output instrument that have no strip yet
    /// ("Add Outputs…").
    pub free_outputs: Vec<u16>,
}

/// A destination of the instrument list (lanes play through these).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DestRow {
    /// Destination ID.
    pub id: u64,
    /// Name.
    pub name: String,
    /// The plugin instrument it plays, if it is one.
    pub plugin: Option<crate::snapshot::PluginRefRow>,
}

/// A bus, for output and send menus.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BusRow {
    /// Strip ID.
    pub id: u64,
    /// Name.
    pub name: String,
}

/// The whole mixer.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MixerSnapshot {
    /// The document version it was read at.
    pub version: u64,
    /// Strips in display order, master last.
    pub strips: Vec<StripRow>,
    /// The buses, in display order.
    pub buses: Vec<BusRow>,
}

/// One strip's peaks since the last read (linear, 1.0 is full scale).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct MeterRow {
    /// Strip ID.
    pub strip: u64,
    /// Left peak.
    pub peak_l: f32,
    /// Right peak.
    pub peak_r: f32,
}

/// What the mixer window polls (spec §7: 30 Hz). Empty for a session that
/// does not hold the engine.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MixerLevels {
    /// Peaks per strip.
    pub meters: Vec<MeterRow>,
    /// DSP load of the last block (render time ÷ block time).
    pub dsp_load: f32,
    /// Peak DSP load since the last read.
    pub dsp_peak: f32,
    /// Plugin delay compensation of the graph, in frames.
    pub latency_frames: u64,
    /// Instances bypassed for faults (NaN output, errors), sticky until
    /// the engine is rebuilt.
    pub faulted: Vec<u64>,
}

impl MixerLevels {
    /// Nothing to show.
    pub fn empty() -> MixerLevels {
        MixerLevels {
            meters: Vec::new(),
            dsp_load: 0.0,
            dsp_peak: 0.0,
            latency_frames: 0,
            faulted: Vec::new(),
        }
    }
}

fn kind_row(k: StripKind) -> StripKindRow {
    match k {
        StripKind::Instrument(d) => StripKindRow::Instrument { dest: d.0 },
        StripKind::InstrumentOutput(d, i) => StripKindRow::InstrumentOutput {
            dest: d.0,
            index: i,
        },
        StripKind::BuiltinSynth => StripKindRow::BuiltinSynth,
        StripKind::AudioPath(p) => StripKindRow::AudioPath { path: p.0 },
        StripKind::Bus => StripKindRow::Bus,
        StripKind::Master => StripKindRow::Master,
    }
}

fn kind_label(doc: &Document, k: StripKind) -> String {
    let dest = |d| {
        doc.instruments()
            .get(&d)
            .map_or_else(|| d.to_string(), |x| x.name.clone())
    };
    match k {
        StripKind::Instrument(d) => dest(d),
        StripKind::InstrumentOutput(d, i) => format!("{} out {}", dest(d), u32::from(i) + 1),
        StripKind::BuiltinSynth => "Built-in Synth".into(),
        StripKind::AudioPath(p) => format!(
            "Audio: {}",
            doc.outputs()
                .get(&p)
                .map_or_else(|| p.to_string(), |o| o.name.clone())
        ),
        StripKind::Bus => "Bus".into(),
        StripKind::Master => "Master".into(),
    }
}

fn strip_row(
    doc: &Document,
    s: &Strip,
    heard: &BTreeSet<core_model::ids::StripId>,
    host: &mut Option<&mut PluginHost>,
) -> StripRow {
    let instrument = match s.kind {
        StripKind::Instrument(d) => match doc.instruments().get(&d).map(|x| &x.kind) {
            Some(DestKind::PluginInstrument { slot }) => {
                Some(slot_row(slot, true, host.as_deref_mut()))
            }
            _ => None,
        },
        _ => None,
    };
    let free_outputs = match (s.kind, &instrument) {
        (StripKind::Instrument(d), Some(slot)) => (1..slot.outputs)
            .filter_map(|i| u16::try_from(i).ok())
            .filter(|i| {
                doc.mixer()
                    .find_kind(StripKind::InstrumentOutput(d, *i))
                    .is_none()
            })
            .collect(),
        _ => Vec::new(),
    };
    StripRow {
        id: s.id.0,
        name: s.name.clone(),
        kind: kind_row(s.kind),
        kind_label: kind_label(doc, s.kind),
        instrument,
        inserts: s
            .inserts
            .iter()
            .map(|slot| slot_row(slot, false, host.as_deref_mut()))
            .collect(),
        gain_db: s.gain_db,
        pan: s.pan,
        mute: s.mute,
        solo: s.solo,
        solo_muted: !heard.contains(&s.id),
        sends: s
            .sends
            .iter()
            .map(|x| SendRow {
                target: x.target.0,
                level_db: x.level_db,
                pre_fader: x.pre_fader,
                enabled: x.enabled,
            })
            .collect(),
        output: match s.output {
            Output::Master => None,
            Output::Bus(b) => Some(b.0),
        },
        free_outputs,
    }
}

/// The mixer of `doc` at document `version`, with what `host` knows about
/// each plugin.
pub fn snapshot(doc: &Document, version: u64, mut host: Option<&mut PluginHost>) -> MixerSnapshot {
    let m = doc.mixer();
    let heard = audible(m);
    let mut strips: Vec<StripRow> = m
        .order()
        .iter()
        .filter_map(|id| m.strip(*id).ok())
        .map(|s| strip_row(doc, s, &heard, &mut host))
        .collect();
    if let Ok(master) = m.strip(m.master()) {
        strips.push(strip_row(doc, master, &heard, &mut host));
    }
    let buses = m
        .order()
        .iter()
        .filter_map(|id| m.strip(*id).ok())
        .filter(|s| s.kind == StripKind::Bus)
        .map(|s| BusRow {
            id: s.id.0,
            name: s.name.clone(),
        })
        .collect();
    MixerSnapshot {
        version,
        strips,
        buses,
    }
}

/// Every plugin slot ID of `doc` (instruments' own slots and inserts).
pub fn slot_ids(doc: &Document) -> BTreeSet<core_model::ids::PluginInstanceId> {
    let mut ids: BTreeSet<_> = doc.mixer().inserts().map(|(_, _, s)| s.id).collect();
    for d in doc.instruments().values() {
        if let DestKind::PluginInstrument { slot } = &d.kind {
            ids.insert(slot.id);
        }
    }
    ids
}
VIZDAW_EOF
```
Change `crates/core-ffi/src/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/host.rs b/crates/core-ffi/src/host.rs
index d498c97..1833877 100644
--- a/crates/core-ffi/src/host.rs
+++ b/crates/core-ffi/src/host.rs
@@ -219,6 +219,9 @@ struct Host {
     sample_rate: f64,
     report: Vec<String>,
     faults: Vec<String>,
+    /// Instances the engine bypassed for faults (sticky for this engine,
+    /// 3a Plan Ruling 23).
+    faulted: std::collections::BTreeSet<u64>,
     /// Read only by the `drain_offline_log` test hook.
     #[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
     log: Option<EventLog>,
@@ -292,6 +295,7 @@ impl Host {
             sample_rate,
             report: Vec::new(),
             faults: Vec::new(),
+            faulted: std::collections::BTreeSet::new(),
             log,
             injected: Vec::new(),
         }
@@ -712,3 +716,29 @@ pub fn factory_owner() -> Option<u64> {
         .map(|h| h.factory_owner)
         .filter(|o| *o != 0)
 }
+
+/// `owner`'s meters, DSP load, latency and fault-bypassed instances, or
+/// `None` when it does not hold the engine.
+pub fn levels(owner: u64) -> Option<crate::mixer::MixerLevels> {
+    let mut slot = lock(&HOST);
+    let h = slot.as_mut().filter(|h| h.owner == owner)?;
+    h.faulted
+        .extend(h.engine.plugin_faults().into_iter().map(|i| i.0));
+    let load = h.engine.dsp_load();
+    Some(crate::mixer::MixerLevels {
+        meters: h
+            .engine
+            .meters()
+            .into_iter()
+            .map(|m| crate::mixer::MeterRow {
+                strip: m.strip.0,
+                peak_l: m.peak_l,
+                peak_r: m.peak_r,
+            })
+            .collect(),
+        dsp_load: load.last,
+        dsp_peak: load.peak,
+        latency_frames: h.engine.latency_frames(),
+        faulted: h.faulted.iter().copied().collect(),
+    })
+}
VIZDAW_EOF
```
Change `crates/core-ffi/src/session.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/session.rs b/crates/core-ffi/src/session.rs
index 95600ae..56f897b 100644
--- a/crates/core-ffi/src/session.rs
+++ b/crates/core-ffi/src/session.rs
@@ -3,19 +3,25 @@
 use crate::error::VizError;
 use crate::guard::{ffi_guard, lock};
 use crate::host;
+use crate::mixer::{self, DestRow, MixerLevels, MixerSnapshot};
 use crate::plugins;
+use crate::snapshot::PluginRefRow;
 use crate::snapshot::{
     DocSnapshot, IdleUpdate, LaneRow, SlotRow, TransportState, lane_rows, position_bbu, seconds,
     seq_id, seq_rows, warnings,
 };
 use core_host::PluginHost;
 use core_model::command::Command;
+use core_model::command::MixerCmd;
+use core_model::command::RoutingCmd;
 use core_model::command::lane::LaneCmd;
 use core_model::command::sequence::SequenceCmd;
+use core_model::document::DestKind;
 use core_model::document::Document;
 use core_model::file;
 use core_model::history::History;
-use core_model::ids::{LaneId, SeqId};
+use core_model::ids::{DestId, LaneId, PluginInstanceId, SeqId, StripId};
+use core_model::mixer::{AuxSend, Output, PluginSlot};
 use core_model::selection::ViewFilter;
 use core_model::time::Tick;
 use std::path::{Path, PathBuf};
@@ -171,17 +177,53 @@ impl Session {
     }
 
     /// Applies one command as one undo step and recompiles if playing.
+    /// Running plugins it removes are captured first (spec §4), so an undo
+    /// brings them back with their state.
     fn execute(&self, cmd: Command) -> Result<DocSnapshot, VizError> {
         let mut inner = lock(&self.inner);
+        if let Ok(next) = core_model::command::apply(inner.doc(), &cmd) {
+            self.capture_removed(&mut inner, &next.doc);
+        }
         inner.history.execute(&cmd, &ViewFilter::default())?;
         inner.version += 1;
         inner.reload_host(self.id);
         inner.snapshot()
     }
 
+    /// Captures the state of every running plugin that `next` (the
+    /// document an edit, undo or redo is about to make) no longer has
+    /// (Plan Ruling 8; the model's `Report.removed` does not list them
+    /// all). Refused inside an open edit: a drag removes no plugin.
+    fn capture_removed(&self, inner: &mut Inner, next: &Document) {
+        let before = mixer::slot_ids(inner.doc());
+        let after = mixer::slot_ids(next);
+        let gone: Vec<_> = before.difference(&after).copied().collect();
+        if gone.is_empty() {
+            return;
+        }
+        let history = &mut inner.history;
+        plugins::with(self.id, |p| {
+            if let Some(p) = p {
+                for id in gone {
+                    // A placeholder has nothing to capture; its saved state
+                    // stays in the document.
+                    let _ = p.capture(history, id);
+                }
+            }
+        });
+    }
+
     /// Undo or redo, then recompile if playing.
     fn step(&self, undo: bool) -> Result<DocSnapshot, VizError> {
         let mut inner = lock(&self.inner);
+        let next = if undo {
+            inner.history.peek_undo().cloned()
+        } else {
+            inner.history.peek_redo().cloned()
+        };
+        if let Some(next) = next {
+            self.capture_removed(&mut inner, &next);
+        }
         if undo {
             inner.history.undo()?;
         } else {
@@ -465,6 +507,314 @@ impl Session {
     }
 }
 
+/// Mixer reads and edits (spec §11). Every edit is one undo step; a drag
+/// (fader, pan, send level) is bracketed by `begin_edit` / `end_edit` and
+/// becomes one step.
+#[uniffi::export]
+impl Session {
+    /// The mixer: strips in display order (master last) with their slots.
+    pub fn mixer(&self) -> Result<MixerSnapshot, VizError> {
+        ffi_guard(|| {
+            let inner = lock(&self.inner);
+            Ok(plugins::with(self.id, |p| {
+                mixer::snapshot(inner.doc(), inner.version, p)
+            }))
+        })
+    }
+
+    /// Meters, DSP load, latency and fault-bypassed plugins (empty unless
+    /// this session holds the engine). The mixer polls it at 30 Hz.
+    pub fn levels(&self) -> Result<MixerLevels, VizError> {
+        ffi_guard(|| Ok(host::levels(self.id).unwrap_or_else(MixerLevels::empty)))
+    }
+
+    /// Opens a continuous edit (a drag): commands until `end_edit` are one
+    /// undo step named `label`, and each is heard at once.
+    pub fn begin_edit(&self, label: String) -> Result<(), VizError> {
+        ffi_guard(|| Ok(lock(&self.inner).history.begin(label)?))
+    }
+
+    /// Closes the edit `begin_edit` opened (nothing happens when none is
+    /// open).
+    pub fn end_edit(&self) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            let mut inner = lock(&self.inner);
+            if inner.history.in_transaction() {
+                inner.history.commit()?;
+                inner.version += 1;
+            }
+            inner.snapshot()
+        })
+    }
+
+    /// Adds an aux bus at the end.
+    pub fn add_bus(&self, name: String) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| self.execute(Command::Mixer(MixerCmd::AddBus { name, index: None })))
+    }
+
+    /// Removes a bus (outputs to it go to master, sends to it go).
+    pub fn remove_bus(&self, strip: u64) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::RemoveBus {
+                strip: StripId(strip),
+            }))
+        })
+    }
+
+    /// Renames a strip.
+    pub fn rename_strip(&self, strip: u64, name: String) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::RenameStrip {
+                strip: StripId(strip),
+                name,
+            }))
+        })
+    }
+
+    /// Moves a strip (not master) to display position `index`.
+    pub fn move_strip(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::MoveStrip {
+                strip: StripId(strip),
+                index: index as usize,
+            }))
+        })
+    }
+
+    /// Sets a fader (dB; −144 is −∞, at most +12).
+    pub fn set_gain(&self, strip: u64, gain_db: f64) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::SetGain {
+                strip: StripId(strip),
+                gain_db,
+            }))
+        })
+    }
+
+    /// Sets a pan (−1 … +1).
+    pub fn set_pan(&self, strip: u64, pan: f64) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::SetPan {
+                strip: StripId(strip),
+                pan,
+            }))
+        })
+    }
+
+    /// Mutes or unmutes a strip.
+    pub fn set_strip_mute(&self, strip: u64, mute: bool) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::SetMute {
+                strip: StripId(strip),
+                mute,
+            }))
+        })
+    }
+
+    /// Solos or unsolos a strip.
+    pub fn set_strip_solo(&self, strip: u64, solo: bool) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::SetSolo {
+                strip: StripId(strip),
+                solo,
+            }))
+        })
+    }
+
+    /// Routes a strip's output to bus `bus`, or to master (`None`).
+    pub fn set_output(&self, strip: u64, bus: Option<u64>) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::SetOutput {
+                strip: StripId(strip),
+                output: bus.map_or(Output::Master, |b| Output::Bus(StripId(b))),
+            }))
+        })
+    }
+
+    /// Adds a post-fader send at 0 dB to bus `target`.
+    pub fn add_send(&self, strip: u64, target: u64) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::AddSend {
+                strip: StripId(strip),
+                send: AuxSend::new(StripId(target)),
+            }))
+        })
+    }
+
+    /// Changes send `index` of a strip.
+    pub fn edit_send(
+        &self,
+        strip: u64,
+        index: u32,
+        send: crate::mixer::SendRow,
+    ) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::EditSend {
+                strip: StripId(strip),
+                index: index as usize,
+                send: AuxSend {
+                    target: StripId(send.target),
+                    level_db: send.level_db,
+                    pre_fader: send.pre_fader,
+                    enabled: send.enabled,
+                },
+            }))
+        })
+    }
+
+    /// Removes send `index` of a strip.
+    pub fn remove_send(&self, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::RemoveSend {
+                strip: StripId(strip),
+                index: index as usize,
+            }))
+        })
+    }
+
+    /// Inserts `plugin` at `index` of a strip's chain (default: the end);
+    /// it starts loading at once.
+    pub fn add_insert(
+        &self,
+        strip: u64,
+        index: Option<u32>,
+        plugin: PluginRefRow,
+    ) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::AddInsert {
+                strip: StripId(strip),
+                index: index.map(|i| i as usize),
+                plugin: (&plugin).into(),
+            }))
+        })
+    }
+
+    /// Removes an insert (its state is captured first, so Undo restores
+    /// it).
+    pub fn remove_insert(&self, slot: u64) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::RemoveInsert {
+                slot: PluginInstanceId(slot),
+            }))
+        })
+    }
+
+    /// Moves an insert to position `index` of `strip` (drag to reorder).
+    pub fn move_insert(&self, slot: u64, strip: u64, index: u32) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::MoveInsert {
+                slot: PluginInstanceId(slot),
+                strip: StripId(strip),
+                index: index as usize,
+            }))
+        })
+    }
+
+    /// Bypasses a plugin, or turns it back on.
+    pub fn set_bypass(&self, slot: u64, bypass: bool) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::SetBypass {
+                slot: PluginInstanceId(slot),
+                bypass,
+            }))
+        })
+    }
+
+    /// Feeds an insert's sidechain from strip `source` (post-fader), or
+    /// from nothing.
+    pub fn set_sidechain(&self, slot: u64, source: Option<u64>) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::SetSidechain {
+                slot: PluginInstanceId(slot),
+                source: source.map(StripId),
+            }))
+        })
+    }
+
+    /// Adds a strip for extra output `index` of a multi-output plugin
+    /// instrument ("Add Outputs…").
+    pub fn add_instrument_output(
+        &self,
+        dest: u64,
+        index: u16,
+        name: String,
+    ) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::AddInstrumentOutput {
+                dest: DestId(dest),
+                index,
+                name,
+            }))
+        })
+    }
+
+    /// Adds a plugin instrument destination named `name` (and its
+    /// instrument strip): "New Plugin Instrument…".
+    pub fn add_plugin_instrument(
+        &self,
+        name: String,
+        plugin: PluginRefRow,
+    ) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Routing(RoutingCmd::AddDestination {
+                name,
+                kind: DestKind::PluginInstrument {
+                    slot: PluginSlot::new(PluginInstanceId(0), (&plugin).into()),
+                },
+                default_patch: None,
+            }))
+        })
+    }
+
+    /// The instrument list: every destination, with the plugin it plays
+    /// (none: a MIDI endpoint or the built-in synth).
+    pub fn destinations(&self) -> Result<Vec<DestRow>, VizError> {
+        ffi_guard(|| {
+            let inner = lock(&self.inner);
+            Ok(inner
+                .doc()
+                .instruments()
+                .values()
+                .map(|d| DestRow {
+                    id: d.id.0,
+                    name: d.name.clone(),
+                    plugin: match &d.kind {
+                        DestKind::PluginInstrument { slot } => Some((&slot.plugin).into()),
+                        DestKind::MidiEndpoint { .. } => None,
+                    },
+                })
+                .collect())
+        })
+    }
+
+    /// Makes destination `dest` play `plugin` (every lane using it now
+    /// plays the plugin; one undo step). The same plugin keeps its
+    /// instance and state (3a Plan Ruling 10).
+    pub fn set_destination_plugin(
+        &self,
+        dest: u64,
+        plugin: PluginRefRow,
+    ) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            let default_patch = lock(&self.inner)
+                .doc()
+                .instruments()
+                .get(&DestId(dest))
+                .ok_or_else(|| VizError::Model {
+                    message: format!("unknown id {}", DestId(dest)),
+                })?
+                .default_patch;
+            self.execute(Command::Routing(RoutingCmd::EditDestination {
+                dest: DestId(dest),
+                kind: DestKind::PluginInstrument {
+                    slot: PluginSlot::new(PluginInstanceId(0), (&plugin).into()),
+                },
+                default_patch,
+            }))
+        })
+    }
+}
+
 /// "Strip — Plugin" for messages about slot `id`.
 fn slot_name(doc: &Document, id: core_model::ids::PluginInstanceId) -> String {
     use core_model::document::SlotOwner;
@@ -491,6 +841,19 @@ impl Session {
         ffi_guard(|| Ok(plugins::with(self.id, |p| p.map_or(0.0, |p| p.config().0))))
     }
 
+    /// Slot `slot`'s state blob in the document, if it has one (test
+    /// hook).
+    pub fn debug_slot_state(&self, slot: u64) -> Result<Option<u64>, VizError> {
+        ffi_guard(|| {
+            let inner = lock(&self.inner);
+            Ok(inner
+                .doc()
+                .plugin_slot(PluginInstanceId(slot))
+                .and_then(|s| s.state)
+                .map(|b| b.0))
+        })
+    }
+
     /// Panics while holding the session lock (test hook): poisons it.
     pub fn debug_panic_locked(&self) -> Result<(), VizError> {
         ffi_guard(|| -> Result<(), VizError> {
VIZDAW_EOF
```
Change `crates/core-ffi/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/lib.rs b/crates/core-ffi/src/lib.rs
index 86e675e..61f9c29 100644
--- a/crates/core-ffi/src/lib.rs
+++ b/crates/core-ffi/src/lib.rs
@@ -20,12 +20,16 @@ pub mod catalog;
 pub mod error;
 pub mod guard;
 pub mod host;
+pub mod mixer;
 pub mod plugins;
 pub mod session;
 pub mod snapshot;
 
 pub use error::VizError;
 pub use guard::ffi_guard;
+pub use mixer::{
+    BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
+};
 pub use session::Session;
 pub use snapshot::{
     DocSnapshot, IdleUpdate, LaneRow, PluginFormatTag, PluginRefRow, SeqRow, SlotRow, SlotStatus,
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test history
cargo test -p core-ffi --test mixer
```

Expected: `history` passes with `peeking_shows_what_undo_and_redo_would_bring_back`; `mixer` 10 passed (`the_demo_mixer_lists_its_strips_master_last`, `mixer_edits_are_undo_steps`, `a_routing_cycle_is_refused_and_changes_nothing`, `a_drag_is_one_undo_step`, `solo_mutes_the_strips_it_does_not_hear`, `levels_are_for_the_session_that_plays`, `a_removed_plugin_keeps_its_state_for_undo_and_redo`, `plugin_instruments_bring_strips_and_free_outputs`, `inserts_reorder_bypass_and_take_a_sidechain`, `a_destination_can_play_a_plugin_instrument`).

- [ ] **Step 5: Keep the Swift fake session in step**

Change `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Tests/VizDAWAppModelTests/FakeSession.swift b/app/Tests/VizDAWAppModelTests/FakeSession.swift
index f3df8cf..b9589db 100644
--- a/app/Tests/VizDAWAppModelTests/FakeSession.swift
+++ b/app/Tests/VizDAWAppModelTests/FakeSession.swift
@@ -99,6 +99,87 @@ final class FakeSession: SessionProtocol, @unchecked Sendable {
     }
 
     func pluginSlots() throws -> [SlotRow] { slots }
+
+    // MARK: Mixer
+
+    var mixerSnap = MixerSnapshot(version: 0, strips: [], buses: [])
+    var levelsValue = MixerLevels(
+        meters: [], dspLoad: 0, dspPeak: 0, latencyFrames: 0, faulted: [])
+    var levelsCalls = 0
+    /// Every mixer call, as text (`"setGain 1 -3.0"`).
+    var calls: [String] = []
+    var mixerError: VizError?
+
+    private func mixerCall(_ text: String) throws -> DocSnapshot {
+        if let e = mixerError { throw e }
+        calls.append(text)
+        return commandResult()
+    }
+
+    func mixer() throws -> MixerSnapshot { mixerSnap }
+    func levels() throws -> MixerLevels {
+        levelsCalls += 1
+        return levelsValue
+    }
+    func beginEdit(label: String) throws { calls.append("begin \(label)") }
+    func endEdit() throws -> DocSnapshot { try mixerCall("end") }
+    func addBus(name: String) throws -> DocSnapshot { try mixerCall("addBus \(name)") }
+    func removeBus(strip: UInt64) throws -> DocSnapshot { try mixerCall("removeBus \(strip)") }
+    func renameStrip(strip: UInt64, name: String) throws -> DocSnapshot {
+        try mixerCall("renameStrip \(strip) \(name)")
+    }
+    func moveStrip(strip: UInt64, index: UInt32) throws -> DocSnapshot {
+        try mixerCall("moveStrip \(strip) \(index)")
+    }
+    func setGain(strip: UInt64, gainDb: Double) throws -> DocSnapshot {
+        try mixerCall("setGain \(strip) \(gainDb)")
+    }
+    func setPan(strip: UInt64, pan: Double) throws -> DocSnapshot {
+        try mixerCall("setPan \(strip) \(pan)")
+    }
+    func setStripMute(strip: UInt64, mute: Bool) throws -> DocSnapshot {
+        try mixerCall("mute \(strip) \(mute)")
+    }
+    func setStripSolo(strip: UInt64, solo: Bool) throws -> DocSnapshot {
+        try mixerCall("solo \(strip) \(solo)")
+    }
+    func setOutput(strip: UInt64, bus: UInt64?) throws -> DocSnapshot {
+        try mixerCall("output \(strip) \(bus.map(String.init) ?? "master")")
+    }
+    func addSend(strip: UInt64, target: UInt64) throws -> DocSnapshot {
+        try mixerCall("addSend \(strip) \(target)")
+    }
+    func editSend(strip: UInt64, index: UInt32, send: SendRow) throws -> DocSnapshot {
+        try mixerCall(
+            "editSend \(strip) \(index) \(send.target) \(send.levelDb) \(send.preFader) \(send.enabled)")
+    }
+    func removeSend(strip: UInt64, index: UInt32) throws -> DocSnapshot {
+        try mixerCall("removeSend \(strip) \(index)")
+    }
+    func addInsert(strip: UInt64, index: UInt32?, plugin: PluginRefRow) throws -> DocSnapshot {
+        try mixerCall("addInsert \(strip) \(index.map(String.init) ?? "end") \(plugin.id)")
+    }
+    func removeInsert(slot: UInt64) throws -> DocSnapshot { try mixerCall("removeInsert \(slot)") }
+    func moveInsert(slot: UInt64, strip: UInt64, index: UInt32) throws -> DocSnapshot {
+        try mixerCall("moveInsert \(slot) \(strip) \(index)")
+    }
+    func setBypass(slot: UInt64, bypass: Bool) throws -> DocSnapshot {
+        try mixerCall("bypass \(slot) \(bypass)")
+    }
+    func setSidechain(slot: UInt64, source: UInt64?) throws -> DocSnapshot {
+        try mixerCall("sidechain \(slot) \(source.map(String.init) ?? "none")")
+    }
+    func addInstrumentOutput(dest: UInt64, index: UInt16, name: String) throws -> DocSnapshot {
+        try mixerCall("addOutput \(dest) \(index) \(name)")
+    }
+    func addPluginInstrument(name: String, plugin: PluginRefRow) throws -> DocSnapshot {
+        try mixerCall("addInstrument \(name) \(plugin.id)")
+    }
+    var dests: [DestRow] = []
+    func destinations() throws -> [DestRow] { dests }
+    func setDestinationPlugin(dest: UInt64, plugin: PluginRefRow) throws -> DocSnapshot {
+        try mixerCall("destPlugin \(dest) \(plugin.id)")
+    }
 }
 
 func seq(_ id: UInt64, _ name: String, topLevel: Bool, uses: UInt32 = 0) -> SeqRow {
VIZDAW_EOF
```

- [ ] **Step 6: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every Rust test passes (536); 36 Swift tests pass.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/history.rs crates/core-model/tests/history.rs \
  crates/core-ffi/src/mixer.rs crates/core-ffi/src/host.rs crates/core-ffi/src/session.rs \
  crates/core-ffi/src/lib.rs crates/core-ffi/tests/mixer.rs \
  app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "$(cat <<'MSG'
feat(core-ffi): mixer read model, edits, levels, capture before removal

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 6: Plugin parameters and gestures as undo steps


**Files:**
- Modify: `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/snapshot.rs`, `crates/core-ffi/src/lib.rs`
- Create: `crates/core-ffi/tests/params.rs`
- Test: `app/Tests/VizDAWAppModelTests/FakeSession.swift`

**Interfaces:**
- Consumes: Task 1's `PluginHost::end_gestures`; Task 4's `Session::idle`, `commit_gestures`; Task 5's `begin_edit`/`end_edit`, `execute`; 3b's `PluginHost::{set_param, instance}`, `PluginInstance::{info, param_value}`.
- Produces: FFI records `ParamRow { id, name, module, min, max, default, value, stepped, automatable, hidden }`, `ParamValue { id, value }`; `Session::params(slot) -> Vec<ParamRow>`, `Session::set_param(slot, param, value)` (live, no undo step; refuses non-finite values), `Session::commit_params(slot, values: Vec<ParamValue>) -> DocSnapshot` (one `SetPluginParams` step), `Session::end_plugin_gestures(slot)`. Internally `Session::pump` (one timer step: service, news for the next `idle`, gestures held while an edit is open, then committed) used by `idle` and by `undo`/`redo`.

Ruling 7. `idle` becomes `pump` plus handing out the news (problems, slot and parameter changes) collected since the last call, so an undo's own pump loses nothing. `end_edit` commits the gestures that waited for the drag.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/params.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/params.rs <<'VIZDAW_EOF'
//! Plugin parameters through the FFI (spec §4, §11): the list, a live set
//! and its one-step commit, plugin-UI gestures as undo steps (also when an
//! undo comes before the next timer tick, and when a drag is open), and a
//! gesture an editor window left open. One test at a time (the engine
//! host is process-wide).

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{ParamRow, ParamValue, PluginFormatTag, PluginRefRow, Session, SlotStatus};

static SERIAL: Mutex<()> = Mutex::new(());

const GAIN: u64 = 0;
const WIGGLE: u64 = 2;

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let start = Instant::now();
    while !done(s) {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The demo with a running Test Gain on the built-in synth, saved and
/// reopened (so it has no undo history), playing: (session, slot).
fn playing_gain() -> (tempfile::TempDir, Arc<Session>, u64) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.vizdaw").to_string_lossy().into_owned();
    {
        let s = Session::new_demo().unwrap();
        let synth = s.mixer().unwrap().strips[0].id;
        s.add_insert(
            synth,
            None,
            PluginRefRow {
                format: PluginFormatTag::Clap,
                id: "vizdaw.test.gain".into(),
                name: "Test Gain".into(),
                vendor: "VizDAW".into(),
                version: "1.0.0".into(),
            },
        )
        .unwrap();
        s.save_as(path.clone()).unwrap();
    }
    let s = Session::open(path).unwrap();
    let id = s.plugin_slots().unwrap()[0].id;
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|r| r.name == "Song")
        .unwrap()
        .id;
    s.play(song).unwrap();
    idle_until(&s, |s| {
        s.plugin_slots().unwrap()[0].status == SlotStatus::Live
            && s.transport().unwrap().report.is_empty()
            && s.transport().unwrap().playing
    });
    (dir, s, id)
}

fn param(s: &Session, slot: u64, id: u64) -> ParamRow {
    s.params(slot)
        .unwrap()
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
}

#[test]
fn params_list_the_plugins_parameters_with_values() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let names: Vec<String> = s.params(id).unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["Gain", "Latency", "Wiggle"]);
    let g = param(&s, id, GAIN);
    assert_eq!((g.min, g.max, g.default, g.value), (0.0, 2.0, 1.0, 1.0));
    assert!(param(&s, id, 1).stepped);
    assert!(s.params(999).unwrap().is_empty(), "no such slot");
    assert!(s.set_param(id, GAIN, f64::NAN).is_err());
}

#[test]
fn a_dragged_control_is_heard_at_once_and_commits_one_step() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    for v in [0.9, 0.6, 0.25] {
        s.set_param(id, GAIN, v).unwrap();
    }
    let snap = s
        .commit_params(
            id,
            vec![ParamValue {
                id: GAIN,
                value: 0.25,
            }],
        )
        .unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Change Plugin Parameter"));
    idle_until(&s, |s| param(s, id, GAIN).value == 0.25);
    s.undo().unwrap();
    // The plugin goes back to the value it was loaded with.
    idle_until(&s, |s| param(s, id, GAIN).value == 1.0);
    assert!(s.snapshot().unwrap().undo_label.is_none());
}

#[test]
fn a_gesture_in_the_plugin_becomes_an_undo_step() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    // "Wiggle" makes the plugin perform a gesture on Gain, as a user would.
    s.set_param(id, WIGGLE, 0.4).unwrap();
    // A timer tick commits it as an undo step.
    idle_until(&s, |s| s.snapshot().unwrap().undo_label.is_some());
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Change Plugin Parameter")
    );
    assert_eq!(param(&s, id, GAIN).value, 0.4);
    s.undo().unwrap();
    idle_until(&s, |s| param(s, id, GAIN).value == 1.0);
}

#[test]
fn an_undo_before_the_next_tick_undoes_the_gesture() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    s.set_param(id, WIGGLE, 0.7).unwrap();
    // The audio thread runs the gesture; no timer tick happens before the
    // user presses Undo.
    std::thread::sleep(Duration::from_millis(80));
    let snap = s.undo().unwrap();
    assert_eq!(snap.redo_label.as_deref(), Some("Change Plugin Parameter"));
    idle_until(&s, |s| param(s, id, GAIN).value == 1.0);
}

#[test]
fn gestures_wait_while_a_drag_is_open() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    let synth = s.mixer().unwrap().strips[0].id;
    s.begin_edit("Fader".into()).unwrap();
    s.set_gain(synth, -6.0).unwrap();
    s.set_param(id, WIGGLE, 0.3).unwrap();
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(150) {
        assert!(s.idle().unwrap().snapshot.is_none(), "held while dragging");
        std::thread::sleep(Duration::from_millis(5));
    }
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Change Plugin Parameter"));
    s.undo().unwrap();
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Fader"),
        "the drag stayed its own step"
    );
}

#[test]
fn a_closed_editor_ends_its_open_gesture() {
    let _g = serial();
    let (_d, s, id) = playing_gain();
    // Nothing is open: ending changes nothing.
    s.end_plugin_gestures(id).unwrap();
    assert!(s.idle().unwrap().snapshot.is_none());
    s.end_plugin_gestures(999).unwrap();
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test params
```

Expected: compile errors — unresolved `vizdaw_ffi::{ParamRow, ParamValue}`, no method `params`/`set_param`/`commit_params`/`end_plugin_gestures` on `Session`.

- [ ] **Step 3: Implement**

Change `crates/core-ffi/src/snapshot.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/snapshot.rs b/crates/core-ffi/src/snapshot.rs
index 61c7c70..f86bfb0 100644
--- a/crates/core-ffi/src/snapshot.rs
+++ b/crates/core-ffi/src/snapshot.rs
@@ -314,3 +314,37 @@ pub struct IdleUpdate {
     /// state.
     pub problems: Vec<String>,
 }
+
+/// One plugin parameter and its current value.
+#[derive(Debug, Clone, PartialEq, uniffi::Record)]
+pub struct ParamRow {
+    /// Stable ID.
+    pub id: u64,
+    /// Name.
+    pub name: String,
+    /// Group path (`/`-separated, may be empty).
+    pub module: String,
+    /// Lowest value.
+    pub min: f64,
+    /// Highest value.
+    pub max: f64,
+    /// Default value.
+    pub default: f64,
+    /// Current value.
+    pub value: f64,
+    /// Whole values only.
+    pub stepped: bool,
+    /// Automatable.
+    pub automatable: bool,
+    /// Hidden from the user.
+    pub hidden: bool,
+}
+
+/// A parameter value to record.
+#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
+pub struct ParamValue {
+    /// Parameter ID.
+    pub id: u64,
+    /// Value.
+    pub value: f64,
+}
VIZDAW_EOF
```
Change `crates/core-ffi/src/session.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/session.rs b/crates/core-ffi/src/session.rs
index 56f897b..9720020 100644
--- a/crates/core-ffi/src/session.rs
+++ b/crates/core-ffi/src/session.rs
@@ -5,11 +5,11 @@ use crate::guard::{ffi_guard, lock};
 use crate::host;
 use crate::mixer::{self, DestRow, MixerLevels, MixerSnapshot};
 use crate::plugins;
-use crate::snapshot::PluginRefRow;
 use crate::snapshot::{
     DocSnapshot, IdleUpdate, LaneRow, SlotRow, TransportState, lane_rows, position_bbu, seconds,
     seq_id, seq_rows, warnings,
 };
+use crate::snapshot::{ParamRow, ParamValue, PluginRefRow};
 use core_host::PluginHost;
 use core_model::command::Command;
 use core_model::command::MixerCmd;
@@ -21,7 +21,7 @@ use core_model::document::Document;
 use core_model::file;
 use core_model::history::History;
 use core_model::ids::{DestId, LaneId, PluginInstanceId, SeqId, StripId};
-use core_model::mixer::{AuxSend, Output, PluginSlot};
+use core_model::mixer::{AuxSend, Output, ParamId, PluginSlot};
 use core_model::selection::ViewFilter;
 use core_model::time::Tick;
 use std::path::{Path, PathBuf};
@@ -48,6 +48,19 @@ struct Inner {
     /// Sticky: the last engine recompile after an edit failed. Cleared by
     /// the next successful recompile or Play.
     reload_fault: Option<String>,
+    /// Plugin gestures waiting for an open edit to end.
+    held: Vec<core_host::Gesture>,
+    /// What plugin timer steps found since the last `idle` call.
+    news: News,
+}
+
+/// What plugin timer steps found, handed out by the next `idle`.
+#[derive(Debug, Default)]
+struct News {
+    doc_changed: bool,
+    slots_changed: bool,
+    params_changed: std::collections::BTreeSet<u64>,
+    problems: Vec<String>,
 }
 
 impl Inner {
@@ -61,6 +74,8 @@ impl Inner {
             cursor: None,
             cursor_seq: None,
             reload_fault: None,
+            held: Vec::new(),
+            news: News::default(),
         }
     }
 
@@ -160,6 +175,51 @@ impl Session {
         }))
     }
 
+    /// One plugin timer step: the host services its plugins (with the
+    /// engine when this session plays), news are kept for the next `idle`,
+    /// and finished gestures become undo steps. Returns true when the
+    /// document changed.
+    fn pump(&self, inner: &mut Inner) -> bool {
+        let report = {
+            let doc = inner.doc();
+            plugins::with(self.id, |p| p.map(|p| host::service(self.id, doc, p)))
+        };
+        let Some(r) = report else {
+            return false;
+        };
+        for (id, why) in &r.failed {
+            let line = format!("{}: {why}", slot_name(inner.doc(), *id));
+            inner.news.problems.push(line);
+        }
+        for (id, why) in &r.rejected_state {
+            let line = format!(
+                "{}: saved state refused ({why})",
+                slot_name(inner.doc(), *id)
+            );
+            inner.news.problems.push(line);
+        }
+        inner.news.slots_changed |= !r.loaded.is_empty()
+            || !r.failed.is_empty()
+            || !r.restart.is_empty()
+            || r.compiled.is_some();
+        inner
+            .news
+            .params_changed
+            .extend(r.params_changed.iter().map(|i| i.0));
+        inner.held.extend(r.gestures);
+        self.commit_held(inner)
+    }
+
+    /// Commits the gestures waiting for an open edit (a drag) to end.
+    /// Inside the edit they would merge into its undo step, so they wait.
+    fn commit_held(&self, inner: &mut Inner) -> bool {
+        if inner.history.in_transaction() {
+            return false;
+        }
+        let gestures = std::mem::take(&mut inner.held);
+        self.commit_gestures(inner, &gestures)
+    }
+
     /// Commits plugin-UI gestures as undo steps (spec §4), then lets the
     /// plugin host and the engine follow (the host must see the committed
     /// document before any undo, 3b `PluginHost::commit_gestures`).
@@ -216,6 +276,12 @@ impl Session {
     /// Undo or redo, then recompile if playing.
     fn step(&self, undo: bool) -> Result<DocSnapshot, VizError> {
         let mut inner = lock(&self.inner);
+        // A gesture the plugin finished since the last timer tick is the
+        // newest step: undo must see it (3b: sync after commit, before
+        // undo).
+        if self.pump(&mut inner) {
+            inner.news.doc_changed = true;
+        }
         let next = if undo {
             inner.history.peek_undo().cloned()
         } else {
@@ -456,41 +522,17 @@ impl Session {
         ffi_guard(|| {
             plugins::tick_closing();
             let mut inner = lock(&self.inner);
-            let report = {
-                let doc = inner.doc();
-                plugins::with(self.id, |p| p.map(|p| host::service(self.id, doc, p)))
-            };
-            let Some(r) = report else {
-                return Ok(IdleUpdate {
-                    snapshot: None,
-                    slots_changed: false,
-                    params_changed: Vec::new(),
-                    problems: Vec::new(),
-                });
-            };
-            let mut problems: Vec<String> = Vec::new();
-            for (id, why) in &r.failed {
-                problems.push(format!("{}: {why}", slot_name(inner.doc(), *id)));
-            }
-            for (id, why) in &r.rejected_state {
-                problems.push(format!(
-                    "{}: saved state refused ({why})",
-                    slot_name(inner.doc(), *id)
-                ));
-            }
-            let changed = self.commit_gestures(&mut inner, &r.gestures);
+            let changed = self.pump(&mut inner);
+            let n = std::mem::take(&mut inner.news);
             Ok(IdleUpdate {
-                snapshot: if changed {
+                snapshot: if changed || n.doc_changed {
                     Some(inner.snapshot()?)
                 } else {
                     None
                 },
-                slots_changed: !r.loaded.is_empty()
-                    || !r.failed.is_empty()
-                    || !r.restart.is_empty()
-                    || r.compiled.is_some(),
-                params_changed: r.params_changed.iter().map(|i| i.0).collect(),
-                problems,
+                slots_changed: n.slots_changed,
+                params_changed: n.params_changed.into_iter().collect(),
+                problems: n.problems,
             })
         })
     }
@@ -507,6 +549,85 @@ impl Session {
     }
 }
 
+/// Plugin parameters (spec §11): the list with current values, a live
+/// set for controls being dragged, and the commit that makes one undo step.
+#[uniffi::export]
+impl Session {
+    /// Slot `slot`'s parameters with their current values (empty while it
+    /// is not running).
+    pub fn params(&self, slot: u64) -> Result<Vec<ParamRow>, VizError> {
+        ffi_guard(|| {
+            Ok(plugins::with(self.id, |p| {
+                let Some(inst) = p.and_then(|p| p.instance(PluginInstanceId(slot))) else {
+                    return Vec::new();
+                };
+                let infos = inst.info().params.clone();
+                infos
+                    .into_iter()
+                    .map(|i| ParamRow {
+                        id: i.id.0,
+                        value: inst.param_value(i.id).unwrap_or(i.default),
+                        name: i.name,
+                        module: i.module,
+                        min: i.min,
+                        max: i.max,
+                        default: i.default,
+                        stepped: i.stepped,
+                        automatable: i.automatable,
+                        hidden: i.hidden,
+                    })
+                    .collect()
+            }))
+        })
+    }
+
+    /// Sends a value to the running plugin now, without an undo step (a
+    /// control being dragged); `commit_params` records the final value.
+    pub fn set_param(&self, slot: u64, param: u64, value: f64) -> Result<(), VizError> {
+        ffi_guard(|| {
+            if !value.is_finite() {
+                return Err(VizError::Model {
+                    message: "parameter values must be finite".into(),
+                });
+            }
+            plugins::with(self.id, |p| {
+                if let Some(p) = p {
+                    p.set_param(PluginInstanceId(slot), ParamId(param), value);
+                }
+            });
+            Ok(())
+        })
+    }
+
+    /// Records parameter values in the slot's overlay as one undo step
+    /// (`SetPluginParams`, spec §4).
+    pub fn commit_params(
+        &self,
+        slot: u64,
+        values: Vec<ParamValue>,
+    ) -> Result<DocSnapshot, VizError> {
+        ffi_guard(|| {
+            self.execute(Command::Mixer(MixerCmd::SetPluginParams {
+                slot: PluginInstanceId(slot),
+                params: values.iter().map(|v| (ParamId(v.id), v.value)).collect(),
+            }))
+        })
+    }
+
+    /// Closes the gesture slot `slot`'s editor left open (its window
+    /// closed); it becomes an undo step at the next timer tick.
+    pub fn end_plugin_gestures(&self, slot: u64) -> Result<(), VizError> {
+        ffi_guard(|| {
+            plugins::with(self.id, |p| {
+                if let Some(p) = p {
+                    p.end_gestures(PluginInstanceId(slot));
+                }
+            });
+            Ok(())
+        })
+    }
+}
+
 /// Mixer reads and edits (spec §11). Every edit is one undo step; a drag
 /// (fader, pan, send level) is bracketed by `begin_edit` / `end_edit` and
 /// becomes one step.
@@ -543,6 +664,7 @@ impl Session {
                 inner.history.commit()?;
                 inner.version += 1;
             }
+            self.commit_held(&mut inner);
             inner.snapshot()
         })
     }
VIZDAW_EOF
```
Change `crates/core-ffi/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/lib.rs b/crates/core-ffi/src/lib.rs
index 61f9c29..ca3df35 100644
--- a/crates/core-ffi/src/lib.rs
+++ b/crates/core-ffi/src/lib.rs
@@ -32,8 +32,8 @@ pub use mixer::{
 };
 pub use session::Session;
 pub use snapshot::{
-    DocSnapshot, IdleUpdate, LaneRow, PluginFormatTag, PluginRefRow, SeqRow, SlotRow, SlotStatus,
-    TransportState,
+    DocSnapshot, IdleUpdate, LaneRow, ParamRow, ParamValue, PluginFormatTag, PluginRefRow, SeqRow,
+    SlotRow, SlotStatus, TransportState,
 };
 
 /// Panics inside the guard (test hook): returns `VizError::Internal`.
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test params --test plugins --test mixer
```

Expected: `params` 6 passed (`params_list_the_plugins_parameters_with_values`, `a_dragged_control_is_heard_at_once_and_commits_one_step`, `a_gesture_in_the_plugin_becomes_an_undo_step`, `an_undo_before_the_next_tick_undoes_the_gesture`, `gestures_wait_while_a_drag_is_open`, `a_closed_editor_ends_its_open_gesture`); `plugins` and `mixer` still pass.

- [ ] **Step 5: Keep the Swift fake session in step**

Change `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Tests/VizDAWAppModelTests/FakeSession.swift b/app/Tests/VizDAWAppModelTests/FakeSession.swift
index b9589db..159d5df 100644
--- a/app/Tests/VizDAWAppModelTests/FakeSession.swift
+++ b/app/Tests/VizDAWAppModelTests/FakeSession.swift
@@ -180,6 +180,22 @@ final class FakeSession: SessionProtocol, @unchecked Sendable {
     func setDestinationPlugin(dest: UInt64, plugin: PluginRefRow) throws -> DocSnapshot {
         try mixerCall("destPlugin \(dest) \(plugin.id)")
     }
+
+    // MARK: Parameters
+
+    var paramTable: [UInt64: [ParamRow]] = [:]
+    /// Live sets, in order: (slot, param, value).
+    var liveSets: [(UInt64, UInt64, Double)] = []
+    var endedGestures: [UInt64] = []
+
+    func params(slot: UInt64) throws -> [ParamRow] { paramTable[slot] ?? [] }
+    func setParam(slot: UInt64, param: UInt64, value: Double) throws {
+        liveSets.append((slot, param, value))
+    }
+    func commitParams(slot: UInt64, values: [ParamValue]) throws -> DocSnapshot {
+        try mixerCall("commitParams \(slot) " + values.map { "\($0.id)=\($0.value)" }.joined(separator: ","))
+    }
+    func endPluginGestures(slot: UInt64) throws { endedGestures.append(slot) }
 }
 
 func seq(_ id: UInt64, _ name: String, topLevel: Bool, uses: UInt32 = 0) -> SeqRow {
VIZDAW_EOF
```

- [ ] **Step 6: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every Rust test passes (542); 36 Swift tests pass.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/session.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/params.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "$(cat <<'MSG'
feat(core-ffi): plugin parameters, gestures as undo steps

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 7: The plugin catalog object — scans, Plugin Manager actions, browser lists


**Files:**
- Modify: `crates/core-ffi/src/catalog.rs`, `crates/core-ffi/src/host.rs`, `crates/core-ffi/src/lib.rs`
- Create: `crates/core-ffi/tests/catalog.rs`

**Interfaces:**
- Consumes: Task 4's `catalog::with`, `plugins::apply_catalog`; 3b's `Catalog::{load, save, rows, browser, set_enabled, clear_blocklist, mark_found, record, get}`, `scan::{scan_source, TIMEOUT, TIMEOUT_AU_V3, PARALLEL}`, `core_host::clap::{search_paths, find_bundles}`, `platform_macos::au::{sources, components}`; `test_plugins::scanner()`.
- Produces: FFI object `PluginCatalog` with `#[uniffi::constructor] open(config: CatalogConfig) -> Arc<PluginCatalog>` and methods (all `Result<…, VizError>`): `take_problem() -> Option<String>`, `rows() -> Vec<CatalogRowFfi>`, `browser(kind: PluginKindTag) -> Vec<CatalogRowFfi>`, `set_enabled(plugin: PluginKey, enabled: bool)`, `clear_blocklist(plugin: PluginKey)`, `rescan(plugins: Vec<PluginKey>)`, `scan(all: bool)`, `progress() -> ScanProgress`, `reveal_path(plugin: PluginKey) -> Option<String>`; records `CatalogConfig { path, scanner, clap_dirs, include_au, timeout_ms: Option<u64> }`, `CatalogRowFfi { plugin: PluginRefRow, kind: Option<PluginKindTag>, status: PluginStatusRow, path }`, `PluginKindTag::{Instrument, Effect}`, `PluginStatusRow::{Ok, Disabled, Blocklisted { reason }, NotValidated, Missing}`, `PluginKey { format, id }`, `ScanProgress { running, done, total }`. Swift sees `PluginCatalogProtocol`.

Rulings 15–18. Every change saves the catalog and tells every plugin host on the main thread (`plugins::apply_catalog`); a scan thread records each bundle as it finishes, and `progress()` (called from the main thread) applies what it recorded since the last call. Non-owner sessions now `sync` on every timer tick, so slots of a re-enabled plugin load without an edit.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/catalog.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/catalog.rs <<'VIZDAW_EOF'
//! The plugin catalog through the FFI (spec §8, §9): scanning in the
//! background with the real scanner against the in-repo test bundle, the
//! Plugin Manager's statuses and actions, the browser's lists, disabling a
//! plugin a project uses, and catalog files the app must not overwrite.
//! Never touches the user's real catalog: every test has its own folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    CatalogConfig, CatalogRowFfi, PluginCatalog, PluginFormatTag, PluginKey, PluginKindTag,
    PluginRefRow, PluginStatusRow, Session, SlotStatus,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

/// A folder with a copy of the test bundle in a `CLAP` subfolder.
fn plugins_dir() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let clap = d.path().join("CLAP");
    std::fs::create_dir_all(&clap).unwrap();
    let bundle = test_plugins::bundle();
    copy_dir(&bundle, &clap.join(bundle.file_name().unwrap()));
    (d, clap)
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

fn config(dir: &Path, clap: &Path) -> CatalogConfig {
    CatalogConfig {
        path: dir.join("plugins.json").to_string_lossy().into_owned(),
        scanner: test_plugins::scanner().to_string_lossy().into_owned(),
        clap_dirs: vec![clap.to_string_lossy().into_owned()],
        include_au: false,
        timeout_ms: Some(2_000),
    }
}

fn wait_scan(c: &PluginCatalog) {
    let t = Instant::now();
    while c.progress().unwrap().running {
        assert!(t.elapsed() < Duration::from_secs(60), "scan never ended");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn status(c: &PluginCatalog, id: &str) -> PluginStatusRow {
    c.rows()
        .unwrap()
        .into_iter()
        .find(|r| r.plugin.id == id)
        .map(|r| r.status)
        .unwrap_or_else(|| panic!("{id} not in the catalog"))
}

fn key(id: &str) -> PluginKey {
    PluginKey {
        format: PluginFormatTag::Clap,
        id: id.into(),
    }
}

fn scanned() -> (tempfile::TempDir, Arc<PluginCatalog>) {
    let (d, clap) = plugins_dir();
    let c = PluginCatalog::open(config(d.path(), &clap)).unwrap();
    assert!(
        c.take_problem().unwrap().is_none(),
        "a missing file is an empty catalog"
    );
    assert!(c.rows().unwrap().is_empty());
    c.scan(false).unwrap();
    wait_scan(&c);
    (d, c)
}

#[test]
fn a_scan_fills_the_manager_and_blocklists_crashers() {
    let _g = serial();
    let (d, c) = scanned();
    assert_eq!(status(&c, "vizdaw.test.gain"), PluginStatusRow::Ok);
    assert_eq!(
        status(&c, "vizdaw.test.crasher"),
        PluginStatusRow::Blocklisted {
            reason: "crashed (signal 11)".into()
        }
    );
    assert_eq!(
        status(&c, "vizdaw.test.hanger"),
        PluginStatusRow::Blocklisted {
            reason: "timed out".into()
        }
    );
    let p = c.progress().unwrap();
    assert_eq!((p.done, p.total), (1, 1), "one bundle");
    // The browser lists usable plugins of one kind.
    let ids = |rows: Vec<CatalogRowFfi>| -> Vec<String> {
        rows.into_iter().map(|r| r.plugin.id).collect()
    };
    let fx = ids(c.browser(PluginKindTag::Effect).unwrap());
    assert!(fx.contains(&"vizdaw.test.gain".to_string()));
    assert!(!fx.contains(&"vizdaw.test.crasher".to_string()));
    assert_eq!(
        ids(c.browser(PluginKindTag::Instrument).unwrap()),
        ["vizdaw.test.synth4"]
    );
    // Saved: a new catalog reads the same.
    let again = PluginCatalog::open(config(d.path(), Path::new("/nonexistent"))).unwrap();
    assert_eq!(again.rows().unwrap(), c.rows().unwrap());
    // Unchanged bundles are not scanned again.
    again.scan(false).unwrap();
    assert!(!again.progress().unwrap().running);
    assert!(
        c.reveal_path(key("vizdaw.test.gain"))
            .unwrap()
            .unwrap()
            .ends_with(".clap")
    );
}

#[test]
fn disabling_a_plugin_makes_its_slots_placeholders_until_enabled() {
    let _g = serial();
    let (_d, c) = scanned();
    c.set_enabled(key("vizdaw.test.gain"), false).unwrap();
    assert_eq!(status(&c, "vizdaw.test.gain"), PluginStatusRow::Disabled);
    assert!(
        !c.browser(PluginKindTag::Effect)
            .unwrap()
            .iter()
            .any(|r| r.plugin.id == "vizdaw.test.gain")
    );
    let s = Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(
        synth,
        None,
        PluginRefRow {
            format: PluginFormatTag::Clap,
            id: "vizdaw.test.gain".into(),
            name: "Test Gain".into(),
            vendor: "VizDAW".into(),
            version: "1.0.0".into(),
        },
    )
    .unwrap();
    let slot = |s: &Session| s.plugin_slots().unwrap()[0].status.clone();
    assert_eq!(
        slot(&s),
        SlotStatus::Placeholder {
            reason: "Disabled in Plugin Manager".into()
        }
    );
    c.set_enabled(key("vizdaw.test.gain"), true).unwrap();
    let t = Instant::now();
    while slot(&s) != SlotStatus::Live {
        assert!(t.elapsed() < Duration::from_secs(10), "{:?}", slot(&s));
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn clearing_a_blocklist_entry_scans_the_bundle_again() {
    let _g = serial();
    let (_d, c) = scanned();
    c.clear_blocklist(key("vizdaw.test.crasher")).unwrap();
    wait_scan(&c);
    // It crashes again, so it is blocklisted again; the others are back.
    assert!(matches!(
        status(&c, "vizdaw.test.crasher"),
        PluginStatusRow::Blocklisted { .. }
    ));
    assert_eq!(status(&c, "vizdaw.test.gain"), PluginStatusRow::Ok);
    c.rescan(vec![key("vizdaw.test.gain")]).unwrap();
    wait_scan(&c);
    assert_eq!(status(&c, "vizdaw.test.gain"), PluginStatusRow::Ok);
}

#[test]
fn a_newer_catalog_is_reported_and_never_overwritten() {
    let _g = serial();
    let (d, clap) = plugins_dir();
    let path = d.path().join("plugins.json");
    let newer = r#"{"version":99,"entries":[],"disabled":[]}"#;
    std::fs::write(&path, newer).unwrap();
    let c = PluginCatalog::open(config(d.path(), &clap)).unwrap();
    let p = c.take_problem().unwrap().unwrap();
    assert!(p.contains("newer"), "{p}");
    assert!(c.take_problem().unwrap().is_none(), "shown once");
    c.set_enabled(key("vizdaw.test.gain"), false).unwrap();
    let p = c.take_problem().unwrap().unwrap();
    assert!(p.contains("not overwriting"), "{p}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
}

#[test]
fn a_corrupt_catalog_is_moved_aside_and_reported() {
    let _g = serial();
    let (d, clap) = plugins_dir();
    let path = d.path().join("plugins.json");
    std::fs::write(&path, "{ not json").unwrap();
    let c = PluginCatalog::open(config(d.path(), &clap)).unwrap();
    assert!(c.take_problem().unwrap().unwrap().contains(".corrupt"));
    assert!(d.path().join("plugins.json.corrupt").exists());
    assert!(c.rows().unwrap().is_empty());
    // A missing scanner is an error, not a hang.
    let mut cfg = config(d.path(), &clap);
    cfg.scanner = "/nonexistent/vizdaw-scan".into();
    let c = PluginCatalog::open(cfg).unwrap();
    assert!(c.scan(true).is_err());
    assert!(!c.progress().unwrap().running);
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test catalog
```

Expected: compile errors — unresolved `vizdaw_ffi::{CatalogConfig, CatalogRowFfi, PluginCatalog, PluginKey, PluginKindTag, PluginStatusRow}`.

- [ ] **Step 3: Implement**

Change `crates/core-ffi/src/catalog.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/catalog.rs b/crates/core-ffi/src/catalog.rs
index dd5327e..01ab7d2 100644
--- a/crates/core-ffi/src/catalog.rs
+++ b/crates/core-ffi/src/catalog.rs
@@ -1,11 +1,20 @@
 //! The process-wide plugin catalog (spec §8, §9): where plugins live and
 //! which are unavailable. Every session's plugin host is told through
-//! [`apply`], with its own marks (one catalog, many hosts).
+//! [`apply`], with its own marks (one catalog, many hosts). The Plugin
+//! Manager and the plugin browser use it through [`PluginCatalog`]; scans
+//! run on background threads, one `vizdaw-scan` process per plugin.
 
-use crate::guard::lock;
+use crate::error::VizError;
+use crate::guard::{ffi_guard, lock};
+use crate::snapshot::{PluginFormatTag, PluginRefRow};
 use core_host::PluginHost;
-use core_host::catalog::{Catalog, HostMarks};
-use std::sync::Mutex;
+use core_host::catalog::{Catalog, CatalogRow, HostMarks, PluginStatus, Source};
+use core_host::{PluginKind, scan};
+use core_model::mixer::PluginFormat;
+use std::path::PathBuf;
+use std::sync::atomic::{AtomicU64, Ordering};
+use std::sync::{Arc, Mutex};
+use std::time::Duration;
 
 static CATALOG: Mutex<Option<Catalog>> = Mutex::new(None);
 
@@ -44,3 +53,395 @@ pub fn add_test_bundle(path: &std::path::Path) -> Result<(), crate::VizError> {
     crate::plugins::apply_catalog();
     Ok(())
 }
+
+/// Instrument or effect.
+#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
+pub enum PluginKindTag {
+    /// Takes notes, makes audio.
+    Instrument,
+    /// Processes audio.
+    Effect,
+}
+
+impl From<PluginKind> for PluginKindTag {
+    fn from(k: PluginKind) -> Self {
+        match k {
+            PluginKind::Instrument => PluginKindTag::Instrument,
+            PluginKind::Effect => PluginKindTag::Effect,
+        }
+    }
+}
+
+/// A plugin's status in the Plugin Manager (spec §9).
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
+pub enum PluginStatusRow {
+    /// Usable.
+    Ok,
+    /// Turned off by the user.
+    Disabled,
+    /// Crashed, hung or failed while scanned.
+    Blocklisted {
+        /// `crashed (signal 11)`, `timed out`, `failed: …`.
+        reason: String,
+    },
+    /// Not checked yet (AU v3): checked on first use.
+    NotValidated,
+    /// Its bundle or component is gone.
+    Missing,
+}
+
+/// One row of the Plugin Manager or the plugin browser.
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
+pub struct CatalogRowFfi {
+    /// The plugin (id empty for a bundle that failed as a whole).
+    pub plugin: PluginRefRow,
+    /// Instrument or effect, when known.
+    pub kind: Option<PluginKindTag>,
+    /// Status.
+    pub status: PluginStatusRow,
+    /// Where it lives: a bundle path, or `au:<id>`.
+    pub path: String,
+}
+
+fn row(r: CatalogRow) -> CatalogRowFfi {
+    CatalogRowFfi {
+        plugin: (&r.plugin).into(),
+        kind: r.kind.map(Into::into),
+        status: match r.status {
+            PluginStatus::Ok => PluginStatusRow::Ok,
+            PluginStatus::Disabled => PluginStatusRow::Disabled,
+            PluginStatus::Blocklisted(reason) => PluginStatusRow::Blocklisted { reason },
+            PluginStatus::NotValidated => PluginStatusRow::NotValidated,
+            PluginStatus::Missing => PluginStatusRow::Missing,
+        },
+        path: r.source.path.to_string_lossy().into_owned(),
+    }
+}
+
+/// Where the catalog lives and how to scan.
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
+pub struct CatalogConfig {
+    /// `plugins.json` (the app passes
+    /// `~/Library/Application Support/VizDAW/plugins.json`).
+    pub path: String,
+    /// The `vizdaw-scan` binary.
+    pub scanner: String,
+    /// Folders searched for CLAP bundles (empty: the standard ones).
+    pub clap_dirs: Vec<String>,
+    /// Also find Audio Units.
+    pub include_au: bool,
+    /// Time each scan may take, in ms (`None`: 10 s, AU v3 30 s).
+    pub timeout_ms: Option<u64>,
+}
+
+/// Where a scan stands.
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
+pub struct ScanProgress {
+    /// A scan is running.
+    pub running: bool,
+    /// Sources scanned so far in this scan.
+    pub done: u32,
+    /// Sources this scan checks.
+    pub total: u32,
+}
+
+#[derive(Debug, Default)]
+struct ScanState {
+    running: bool,
+    done: u32,
+    total: u32,
+}
+
+/// The catalog as the app sees it (one per process): the Plugin Manager's
+/// rows and actions, the browser's lists, and scans. Every change is
+/// saved to `plugins.json` (never over a file the app cannot read) and
+/// told to every open document's plugin host.
+#[derive(uniffi::Object)]
+pub struct PluginCatalog {
+    config: CatalogConfig,
+    /// Why the catalog file was not read or not written, if it was not.
+    problem: Mutex<Option<String>>,
+    scan: Arc<Mutex<ScanState>>,
+    /// Bumped by scan threads after recording; `progress` applies the
+    /// catalog to the hosts when it moved.
+    recorded: Arc<AtomicU64>,
+    applied: AtomicU64,
+}
+
+impl std::fmt::Debug for PluginCatalog {
+    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
+        f.debug_struct("PluginCatalog")
+            .field("path", &self.config.path)
+            .finish_non_exhaustive()
+    }
+}
+
+impl PluginCatalog {
+    fn save(&self) {
+        let r = with(|c| c.save(&PathBuf::from(&self.config.path)));
+        if let Err(e) = r {
+            *lock(&self.problem) = Some(format!("plugin settings not saved: {e}"));
+        }
+    }
+
+    /// Saves, then tells every host on this (the main) thread.
+    fn changed(&self) {
+        self.save();
+        crate::plugins::apply_catalog();
+    }
+
+    fn timeout(&self, src: &Source, v3: &[PathBuf]) -> Duration {
+        if let Some(ms) = self.config.timeout_ms {
+            return Duration::from_millis(ms);
+        }
+        if src.format == PluginFormat::Au && v3.contains(&src.path) {
+            scan::TIMEOUT_AU_V3
+        } else {
+            scan::TIMEOUT
+        }
+    }
+
+    /// Every source discovery finds now.
+    fn discover(&self) -> Vec<Source> {
+        let dirs: Vec<PathBuf> = if self.config.clap_dirs.is_empty() {
+            core_host::clap::search_paths()
+        } else {
+            self.config.clap_dirs.iter().map(PathBuf::from).collect()
+        };
+        #[allow(unused_mut)]
+        let mut found: Vec<Source> = core_host::clap::find_bundles(&dirs)
+            .iter()
+            .map(|p| Source::clap_bundle(p))
+            .collect();
+        #[cfg(target_os = "macos")]
+        if self.config.include_au {
+            found.extend(platform_macos::au::sources());
+        }
+        found
+    }
+
+    /// Scans `sources` on a background thread, up to [`scan::PARALLEL`] at
+    /// a time, recording (and saving) each as it finishes.
+    fn start(&self, sources: Vec<Source>) -> Result<(), VizError> {
+        let exe = PathBuf::from(&self.config.scanner);
+        if !exe.is_file() {
+            return Err(VizError::NotFound {
+                path: self.config.scanner.clone(),
+            });
+        }
+        {
+            let mut st = lock(&self.scan);
+            if st.running {
+                return Err(VizError::Engine {
+                    message: "a scan is already running".into(),
+                });
+            }
+            *st = ScanState {
+                running: true,
+                done: 0,
+                total: u32::try_from(sources.len()).unwrap_or(u32::MAX),
+            };
+        }
+        #[cfg(target_os = "macos")]
+        let v3: Vec<PathBuf> = if self.config.include_au {
+            platform_macos::au::components()
+                .into_iter()
+                .filter(|c| c.v3)
+                .map(|c| PathBuf::from(format!("au:{}", c.plugin.id)))
+                .collect()
+        } else {
+            Vec::new()
+        };
+        #[cfg(not(target_os = "macos"))]
+        let v3: Vec<PathBuf> = Vec::new();
+        let jobs: Vec<(Source, Duration)> = sources
+            .into_iter()
+            .map(|s| {
+                let t = self.timeout(&s, &v3);
+                (s, t)
+            })
+            .collect();
+        let state = Arc::clone(&self.scan);
+        let recorded = Arc::clone(&self.recorded);
+        let path = PathBuf::from(&self.config.path);
+        let queue = Arc::new(Mutex::new(jobs));
+        let spawned = std::thread::Builder::new()
+            .name("vizdaw-scan-pool".into())
+            .spawn(move || {
+                std::thread::scope(|sc| {
+                    for _ in 0..scan::PARALLEL {
+                        sc.spawn(|| {
+                            loop {
+                                let job = lock(&queue).pop();
+                                let Some((src, timeout)) = job else {
+                                    break;
+                                };
+                                let entries = scan::scan_source(&exe, &src, timeout);
+                                with(|c| c.record(&src, entries));
+                                lock(&state).done += 1;
+                                recorded.fetch_add(1, Ordering::Release);
+                            }
+                        });
+                    }
+                });
+                // A file the app cannot read is never overwritten (the
+                // problem shows on the next action).
+                let _ = with(|c| c.save(&path));
+                lock(&state).running = false;
+                recorded.fetch_add(1, Ordering::Release);
+            });
+        if let Err(e) = spawned {
+            lock(&self.scan).running = false;
+            return Err(VizError::Engine {
+                message: format!("cannot start the scan: {e}"),
+            });
+        }
+        Ok(())
+    }
+
+    fn sources_of(&self, keys: &[PluginKey]) -> Vec<Source> {
+        with(|c| {
+            let mut v: Vec<Source> = keys
+                .iter()
+                .filter_map(|k| c.get(k.format.into(), &k.id).map(|e| e.source.clone()))
+                .collect();
+            v.sort();
+            v.dedup();
+            v
+        })
+    }
+}
+
+/// A plugin, by format and id.
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
+pub struct PluginKey {
+    /// Format.
+    pub format: PluginFormatTag,
+    /// Id.
+    pub id: String,
+}
+
+#[uniffi::export]
+impl PluginCatalog {
+    /// Reads the catalog at `config.path` and makes it the app's. A newer
+    /// or corrupt file is reported by [`PluginCatalog::problem`] (a corrupt
+    /// one is moved to `plugins.json.corrupt`; a newer one is left alone
+    /// and never overwritten), and the app goes on with an empty catalog.
+    #[uniffi::constructor]
+    pub fn open(config: CatalogConfig) -> Result<Arc<PluginCatalog>, VizError> {
+        ffi_guard(|| {
+            let (cat, problem) = match Catalog::load(&PathBuf::from(&config.path)) {
+                Ok(c) => (c, None),
+                Err(e) => (
+                    Catalog::new(),
+                    Some(format!("plugin settings not read: {e}")),
+                ),
+            };
+            *lock(&CATALOG) = Some(cat);
+            crate::plugins::apply_catalog();
+            Ok(Arc::new(PluginCatalog {
+                config,
+                problem: Mutex::new(problem),
+                scan: Arc::new(Mutex::new(ScanState::default())),
+                recorded: Arc::new(AtomicU64::new(0)),
+                applied: AtomicU64::new(0),
+            }))
+        })
+    }
+
+    /// Why the catalog file was not read or written, if so (shown once by
+    /// the app: it is cleared by this call).
+    pub fn take_problem(&self) -> Result<Option<String>, VizError> {
+        ffi_guard(|| Ok(lock(&self.problem).take()))
+    }
+
+    /// Every known plugin, by name (the Plugin Manager's table).
+    pub fn rows(&self) -> Result<Vec<CatalogRowFfi>, VizError> {
+        ffi_guard(|| Ok(with(|c| c.rows()).into_iter().map(row).collect()))
+    }
+
+    /// What the plugin browser lists for `kind`: usable plugins only
+    /// (disabled, blocklisted and missing ones are hidden).
+    pub fn browser(&self, kind: PluginKindTag) -> Result<Vec<CatalogRowFfi>, VizError> {
+        ffi_guard(|| {
+            let k = match kind {
+                PluginKindTag::Instrument => PluginKind::Instrument,
+                PluginKindTag::Effect => PluginKind::Effect,
+            };
+            Ok(with(|c| c.browser(k)).into_iter().map(row).collect())
+        })
+    }
+
+    /// Turns a plugin on or off (an app preference). Slots naming a
+    /// disabled plugin load as placeholders "Disabled in Plugin Manager";
+    /// enabling it loads the waiting slots of every open document.
+    pub fn set_enabled(&self, plugin: PluginKey, enabled: bool) -> Result<(), VizError> {
+        ffi_guard(|| {
+            with(|c| c.set_enabled(plugin.format.into(), &plugin.id, enabled));
+            self.changed();
+            Ok(())
+        })
+    }
+
+    /// Forgets a plugin's blocklist entry and scans its bundle again.
+    pub fn clear_blocklist(&self, plugin: PluginKey) -> Result<(), VizError> {
+        ffi_guard(|| {
+            let src = with(|c| c.clear_blocklist(plugin.format.into(), &plugin.id));
+            self.changed();
+            match src {
+                Some(s) => self.start(vec![s]),
+                None => Ok(()),
+            }
+        })
+    }
+
+    /// Scans the bundles of `plugins` again.
+    pub fn rescan(&self, plugins: Vec<PluginKey>) -> Result<(), VizError> {
+        ffi_guard(|| self.start(self.sources_of(&plugins)))
+    }
+
+    /// Finds plugins and scans the new or changed ones (at launch), or
+    /// every one (`all`, "Rescan All"). Gone ones become Missing.
+    pub fn scan(&self, all: bool) -> Result<(), VizError> {
+        ffi_guard(|| {
+            let found = self.discover();
+            let todo = with(|c| c.mark_found(&found));
+            if all {
+                self.start(found)
+            } else if todo.is_empty() {
+                Ok(())
+            } else {
+                self.start(todo)
+            }
+        })
+    }
+
+    /// Where the running scan stands. Call it from the main thread: what
+    /// scans recorded since the last call reaches the open documents here.
+    pub fn progress(&self) -> Result<ScanProgress, VizError> {
+        ffi_guard(|| {
+            let now = self.recorded.load(Ordering::Acquire);
+            if self.applied.swap(now, Ordering::AcqRel) != now {
+                crate::plugins::apply_catalog();
+            }
+            let st = lock(&self.scan);
+            Ok(ScanProgress {
+                running: st.running,
+                done: st.done,
+                total: st.total,
+            })
+        })
+    }
+
+    /// The bundle to reveal in Finder (CLAP); `None` for Audio Units,
+    /// which live in the component registry.
+    pub fn reveal_path(&self, plugin: PluginKey) -> Result<Option<String>, VizError> {
+        ffi_guard(|| {
+            Ok(with(|c| {
+                c.get(plugin.format.into(), &plugin.id)
+                    .filter(|e| e.source.format == PluginFormat::Clap)
+                    .map(|e| e.source.path.to_string_lossy().into_owned())
+            }))
+        })
+    }
+}
VIZDAW_EOF
```
Change `crates/core-ffi/src/host.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/host.rs b/crates/core-ffi/src/host.rs
index 1833877..ddcd082 100644
--- a/crates/core-ffi/src/host.rs
+++ b/crates/core-ffi/src/host.rs
@@ -694,7 +694,12 @@ pub fn service(owner: u64, doc: &Document, plugins: &mut PluginHost) -> IdleRepo
                 }
             }
         }
-        _ => plugins.idle(),
+        _ => {
+            // Not playing: slots the catalog released (a plugin enabled
+            // again) still load.
+            plugins.sync(doc);
+            plugins.idle()
+        }
     }
 }
 
VIZDAW_EOF
```
Change `crates/core-ffi/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/lib.rs b/crates/core-ffi/src/lib.rs
index ca3df35..cc9391d 100644
--- a/crates/core-ffi/src/lib.rs
+++ b/crates/core-ffi/src/lib.rs
@@ -25,6 +25,10 @@ pub mod plugins;
 pub mod session;
 pub mod snapshot;
 
+pub use catalog::{
+    CatalogConfig, CatalogRowFfi, PluginCatalog, PluginKey, PluginKindTag, PluginStatusRow,
+    ScanProgress,
+};
 pub use error::VizError;
 pub use guard::ffi_guard;
 pub use mixer::{
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test catalog
```

Expected: 5 passed (`a_scan_fills_the_manager_and_blocklists_crashers`, `disabling_a_plugin_makes_its_slots_placeholders_until_enabled`, `clearing_a_blocklist_entry_scans_the_bundle_again`, `a_newer_catalog_is_reported_and_never_overwritten`, `a_corrupt_catalog_is_moved_aside_and_reported`), in about 15–25 s (the hanger times out after 2 s per scan).

- [ ] **Step 5: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every Rust test passes (547); 36 Swift tests pass.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/src/catalog.rs crates/core-ffi/src/host.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/catalog.rs
git commit -m "$(cat <<'MSG'
feat(core-ffi): plugin catalog, scans and Plugin Manager actions

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 8: Plugin editors and saved plugin windows


**Files:**
- Modify: `crates/core-ffi/Cargo.toml` (`serde`, `serde_json`)
- Create: `crates/core-ffi/src/editor.rs`
- Modify: `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/plugins.rs`, `crates/core-ffi/src/snapshot.rs`, `crates/core-ffi/src/lib.rs`
- Create: `crates/core-ffi/tests/editors.rs`
- Modify: `app/Package.swift` (AppKit, CoreAudioKit)
- Test: `app/Tests/VizDAWAppModelTests/FakeSession.swift`
- Lockfile: `Cargo.lock`

**Interfaces:**
- Consumes: Task 3's AU editors, 3b's CLAP editor (`PluginInstance::editor`, `PluginEditor`), Task 1's `end_gestures`, Task 5's `mixer::slot_ids`.
- Produces: `vizdaw_ffi::editor::{parent(u64) -> Result<ParentView, VizError>, EditorSize { width: u32, height: u32, resizable: bool }}`; `SlotRow.has_editor: bool`; `Session::open_editor(slot, parent: u64) -> EditorSize`, `Session::resize_editor(slot, width, height) -> EditorSize`, `Session::close_editor(slot)`, `Session::set_window_open(slot, open: bool)`, `Session::open_windows() -> Vec<u64>`; the package file `ui.json` (`{"open_plugin_windows": [ids]}`, `session::UI_FILE`).

Rulings 13 and 20. `editor.rs` holds core-ffi's only `unsafe` block: it turns the address Swift passes into a `ParentView` (the app keeps that `NSView` alive until `close_editor`). Errors are `VizError::Engine { message: "editor unavailable: …" }` (no GUI, not running, null view, not the main thread). `close_editor` also ends a gesture the editor left open. Saving writes `ui.json` next to `project.json` (best effort); opening reads it.

- [ ] **Step 1: Write the failing tests**

Create `crates/core-ffi/tests/editors.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/editors.rs <<'VIZDAW_EOF'
//! Plugin windows through the FFI (spec §9): editors refuse what they
//! cannot show (no GUI, no parent view, not the main thread), and the open
//! windows are saved with the project and offered again on open. Opening a
//! real editor needs AppKit on the main thread: platform-macos's
//! `au_editor` test and the manual checklist cover that.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{PluginFormatTag, PluginRefRow, Session, SlotStatus, VizError};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    g
}

fn plugin(format: PluginFormatTag, id: &str) -> PluginRefRow {
    PluginRefRow {
        format,
        id: id.into(),
        name: id.into(),
        vendor: String::new(),
        version: String::new(),
    }
}

/// The demo with `p` on the built-in synth, running: (session, slot).
fn with_insert(p: PluginRefRow) -> (Arc<Session>, u64) {
    let s = Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(synth, None, p).unwrap();
    let id = s.plugin_slots().unwrap()[0].id;
    let t = Instant::now();
    while s.plugin_slots().unwrap()[0].status != SlotStatus::Live {
        assert!(t.elapsed() < Duration::from_secs(10));
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    (s, id)
}

fn unavailable(r: Result<vizdaw_ffi::EditorSize, VizError>) -> String {
    match r {
        Err(VizError::Engine { message }) if message.starts_with("editor unavailable") => message,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_plugin_without_a_gui_has_no_editor() {
    let _g = serial();
    let (s, id) = with_insert(plugin(PluginFormatTag::Clap, "vizdaw.test.gain"));
    assert!(!s.plugin_slots().unwrap()[0].has_editor);
    unavailable(s.open_editor(id, 0x1000));
    unavailable(s.open_editor(id, 0));
    unavailable(s.open_editor(999, 0x1000));
    unavailable(s.resize_editor(id, 100, 100));
    // Closing what is not open is fine.
    s.close_editor(id).unwrap();
    s.close_editor(999).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn an_au_editor_opens_only_on_the_main_thread() {
    let _g = serial();
    let (s, id) = with_insert(plugin(PluginFormatTag::Au, "aufx/dely/appl"));
    assert!(s.plugin_slots().unwrap()[0].has_editor);
    // Test threads are not the main thread: refused before AppKit is used.
    let m = unavailable(s.open_editor(id, 0x1000));
    assert!(m.contains("main thread"), "{m}");
}

#[test]
fn open_windows_are_saved_with_the_project() {
    let _g = serial();
    let (s, id) = with_insert(plugin(PluginFormatTag::Clap, "vizdaw.test.gain"));
    s.set_window_open(id, true).unwrap();
    s.set_window_open(4242, true).unwrap();
    assert_eq!(s.open_windows().unwrap(), vec![id], "only existing slots");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("w.vizdaw");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    assert!(path.join("ui.json").exists());
    let again = Session::open(path.to_string_lossy().into_owned()).unwrap();
    assert_eq!(again.open_windows().unwrap(), vec![id]);
    again.set_window_open(id, false).unwrap();
    again.save().unwrap();
    let third = Session::open(path.to_string_lossy().into_owned()).unwrap();
    assert!(third.open_windows().unwrap().is_empty());
    // A project without the file (older, or written elsewhere) opens none.
    std::fs::remove_file(path.join("ui.json")).unwrap();
    let fourth = Session::open(path.to_string_lossy().into_owned()).unwrap();
    assert!(fourth.open_windows().unwrap().is_empty());
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test editors
```

Expected: compile errors — unresolved `vizdaw_ffi::EditorSize`, no field `has_editor` on `SlotRow`, no method `open_editor`/`set_window_open`/`open_windows` on `Session`.

- [ ] **Step 3: Implement**

Change `crates/core-ffi/Cargo.toml`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/Cargo.toml b/crates/core-ffi/Cargo.toml
index d55cc91..f87dccd 100644
--- a/crates/core-ffi/Cargo.toml
+++ b/crates/core-ffi/Cargo.toml
@@ -21,6 +21,8 @@ test-hooks = []
 core-model = { path = "../core-model" }
 core-engine = { path = "../core-engine", features = ["fixtures"] }
 core-host = { path = "../core-host" }
+serde = { version = "1", features = ["derive"] }
+serde_json = "1"
 thiserror = "2"
 uniffi = { version = "=0.32.2", default-features = false }
 
VIZDAW_EOF
```
Create `crates/core-ffi/src/editor.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/src
cat > crates/core-ffi/src/editor.rs <<'VIZDAW_EOF'
//! Plugin editors in the app's windows (spec §9): the app hands over the
//! `NSView` a plugin's own view goes into, as an address. The only
//! `unsafe` of core-ffi is here.

use crate::error::VizError;
use core_host::ParentView;
use std::ffi::c_void;

/// An editor's size in points, and whether its window may resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct EditorSize {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// The user may resize it.
    pub resizable: bool,
}

/// The parent view at address `view` (an `NSView *` from the app).
///
/// The app's contract (Swift `PluginWindow`): the view lives until it has
/// called `close_editor` for the slot, and every call is made on the main
/// thread.
pub fn parent(view: u64) -> Result<ParentView, VizError> {
    if view == 0 {
        return Err(VizError::Engine {
            message: "editor unavailable: no parent view".into(),
        });
    }
    let ptr = usize::try_from(view).map_err(|_| VizError::Engine {
        message: "editor unavailable: bad view address".into(),
    })? as *mut c_void;
    // SAFETY: `ptr` is the address of an `NSView` the app keeps alive until
    // it closes the editor (the contract above, which is `ParentView`'s).
    Ok(unsafe { ParentView::new(ptr) })
}
VIZDAW_EOF
```
Change `crates/core-ffi/src/session.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/session.rs b/crates/core-ffi/src/session.rs
index 9720020..45428f1 100644
--- a/crates/core-ffi/src/session.rs
+++ b/crates/core-ffi/src/session.rs
@@ -1,5 +1,6 @@
 //! `Session`: one open document (one window).
 
+use crate::editor::{self, EditorSize};
 use crate::error::VizError;
 use crate::guard::{ffi_guard, lock};
 use crate::host;
@@ -50,6 +51,8 @@ struct Inner {
     reload_fault: Option<String>,
     /// Plugin gestures waiting for an open edit to end.
     held: Vec<core_host::Gesture>,
+    /// Slots whose plugin window is open (saved with the project, spec §9).
+    windows: std::collections::BTreeSet<u64>,
     /// What plugin timer steps found since the last `idle` call.
     news: News,
 }
@@ -75,6 +78,7 @@ impl Inner {
             cursor_seq: None,
             reload_fault: None,
             held: Vec::new(),
+            windows: std::collections::BTreeSet::new(),
             news: News::default(),
         }
     }
@@ -337,7 +341,10 @@ impl Session {
                 return Err(VizError::NotFound { path });
             }
             let doc = file::load(&p)?;
-            Session::with(doc, Some(p))
+            let windows = read_windows(&p);
+            let s = Session::with(doc, Some(p))?;
+            lock(&s.inner).windows = windows;
+            Ok(s)
         })
     }
 
@@ -349,6 +356,7 @@ impl Session {
                 message: "the document has no file yet; use Save As".into(),
             })?;
             file::save(inner.doc(), &path)?;
+            write_windows(&path, &inner.windows);
             inner.clean = inner.doc().clone();
             Ok(())
         })
@@ -361,6 +369,7 @@ impl Session {
             let mut inner = lock(&self.inner);
             let p = with_extension(&path);
             file::save(inner.doc(), &p)?;
+            write_windows(&p, &inner.windows);
             inner.clean = inner.doc().clone();
             inner.path = Some(p);
             Ok(())
@@ -937,6 +946,136 @@ impl Session {
     }
 }
 
+/// The package file holding which plugin windows were open (not part of
+/// the document: no undo, Plan Ruling 13).
+pub const UI_FILE: &str = "ui.json";
+
+fn read_windows(package: &Path) -> std::collections::BTreeSet<u64> {
+    std::fs::read_to_string(package.join(UI_FILE))
+        .ok()
+        .and_then(|t| serde_json::from_str::<UiState>(&t).ok())
+        .map(|u| u.open_plugin_windows.into_iter().collect())
+        .unwrap_or_default()
+}
+
+/// Best-effort: a project whose window list cannot be written still saved.
+fn write_windows(package: &Path, windows: &std::collections::BTreeSet<u64>) {
+    let ui = UiState {
+        open_plugin_windows: windows.iter().copied().collect(),
+    };
+    if let Ok(text) = serde_json::to_string_pretty(&ui) {
+        let _ = file::write_atomic(&package.join(UI_FILE), text.as_bytes());
+    }
+}
+
+#[derive(serde::Serialize, serde::Deserialize)]
+struct UiState {
+    open_plugin_windows: Vec<u64>,
+}
+
+/// Plugin windows (spec §9): the plugin's own editor inside a window the
+/// app owns, and which windows are open (they reopen with the project).
+#[uniffi::export]
+impl Session {
+    /// Shows slot `slot`'s editor inside the `NSView` at address `parent`
+    /// (main thread; the view lives until `close_editor`). Fails with
+    /// "editor unavailable" when the plugin has none (the app then shows
+    /// its parameters).
+    pub fn open_editor(&self, slot: u64, parent: u64) -> Result<EditorSize, VizError> {
+        ffi_guard(|| {
+            let parent = editor::parent(parent)?;
+            plugins::with(self.id, |p| {
+                let ed = p
+                    .and_then(|p| p.instance(PluginInstanceId(slot)))
+                    .and_then(|i| i.editor())
+                    .ok_or_else(|| unavailable("the plugin has no editor"))?;
+                let (width, height) = ed.open(parent).map_err(|e| unavailable(&e.to_string()))?;
+                Ok(EditorSize {
+                    width,
+                    height,
+                    resizable: ed.can_resize(),
+                })
+            })
+        })
+    }
+
+    /// Asks slot `slot`'s editor for a new size; returns what it took.
+    pub fn resize_editor(
+        &self,
+        slot: u64,
+        width: u32,
+        height: u32,
+    ) -> Result<EditorSize, VizError> {
+        ffi_guard(|| {
+            plugins::with(self.id, |p| {
+                let ed = p
+                    .and_then(|p| p.instance(PluginInstanceId(slot)))
+                    .and_then(|i| i.editor())
+                    .ok_or_else(|| unavailable("the plugin has no editor"))?;
+                let (width, height) = ed
+                    .resize(width, height)
+                    .map_err(|e| unavailable(&e.to_string()))?;
+                Ok(EditorSize {
+                    width,
+                    height,
+                    resizable: ed.can_resize(),
+                })
+            })
+        })
+    }
+
+    /// Hides slot `slot`'s editor (its window is closing) and ends any
+    /// gesture it left open.
+    pub fn close_editor(&self, slot: u64) -> Result<(), VizError> {
+        ffi_guard(|| {
+            plugins::with(self.id, |p| {
+                if let Some(p) = p {
+                    if let Some(ed) = p.instance(PluginInstanceId(slot)).and_then(|i| i.editor()) {
+                        ed.close();
+                    }
+                    p.end_gestures(PluginInstanceId(slot));
+                }
+            });
+            Ok(())
+        })
+    }
+
+    /// Records that slot `slot`'s window is open or closed (saved with the
+    /// project).
+    pub fn set_window_open(&self, slot: u64, open: bool) -> Result<(), VizError> {
+        ffi_guard(|| {
+            let mut inner = lock(&self.inner);
+            if open {
+                inner.windows.insert(slot);
+            } else {
+                inner.windows.remove(&slot);
+            }
+            Ok(())
+        })
+    }
+
+    /// The plugin windows to open now: those open when the project was
+    /// saved whose slot still exists.
+    pub fn open_windows(&self) -> Result<Vec<u64>, VizError> {
+        ffi_guard(|| {
+            let inner = lock(&self.inner);
+            let slots = mixer::slot_ids(inner.doc());
+            Ok(inner
+                .windows
+                .iter()
+                .copied()
+                .filter(|w| slots.contains(&PluginInstanceId(*w)))
+                .collect())
+        })
+    }
+}
+
+fn unavailable(why: &str) -> VizError {
+    VizError::Engine {
+        message: format!("editor unavailable: {why}"),
+    }
+}
+
 /// "Strip — Plugin" for messages about slot `id`.
 fn slot_name(doc: &Document, id: core_model::ids::PluginInstanceId) -> String {
     use core_model::document::SlotOwner;
VIZDAW_EOF
```
Change `crates/core-ffi/src/plugins.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/plugins.rs b/crates/core-ffi/src/plugins.rs
index 8f563f7..3725468 100644
--- a/crates/core-ffi/src/plugins.rs
+++ b/crates/core-ffi/src/plugins.rs
@@ -146,6 +146,7 @@ pub fn slot_row(
         latency: 0,
         has_sidechain_input: false,
         outputs: 0,
+        has_editor: false,
     };
     let Some(host) = host else {
         return row;
@@ -160,6 +161,7 @@ pub fn slot_row(
         row.latency = info.latency;
         row.has_sidechain_input = info.sidechain_input().is_some();
         row.outputs = u32::try_from(info.outputs.len()).unwrap_or(u32::MAX);
+        row.has_editor = inst.editor().is_some();
     }
     row
 }
VIZDAW_EOF
```
Change `crates/core-ffi/src/snapshot.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/snapshot.rs b/crates/core-ffi/src/snapshot.rs
index f86bfb0..edea88e 100644
--- a/crates/core-ffi/src/snapshot.rs
+++ b/crates/core-ffi/src/snapshot.rs
@@ -298,6 +298,9 @@ pub struct SlotRow {
     pub has_sidechain_input: bool,
     /// Output ports (multi-output instruments have more than one).
     pub outputs: u32,
+    /// It has its own editor (otherwise its window lists its
+    /// parameters).
+    pub has_editor: bool,
 }
 
 /// What a plugin timer tick changed.
VIZDAW_EOF
```
Change `crates/core-ffi/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/lib.rs b/crates/core-ffi/src/lib.rs
index cc9391d..f77461f 100644
--- a/crates/core-ffi/src/lib.rs
+++ b/crates/core-ffi/src/lib.rs
@@ -17,6 +17,7 @@
 uniffi::setup_scaffolding!();
 
 pub mod catalog;
+pub mod editor;
 pub mod error;
 pub mod guard;
 pub mod host;
@@ -29,6 +30,7 @@ pub use catalog::{
     CatalogConfig, CatalogRowFfi, PluginCatalog, PluginKey, PluginKindTag, PluginStatusRow,
     ScanProgress,
 };
+pub use editor::EditorSize;
 pub use error::VizError;
 pub use guard::ffi_guard;
 pub use mixer::{
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-ffi --test editors
```

Expected: 3 passed (`a_plugin_without_a_gui_has_no_editor`, `an_au_editor_opens_only_on_the_main_thread`, `open_windows_are_saved_with_the_project`).

- [ ] **Step 5: Link the editor frameworks and keep the Swift fake session in step**

Change `app/Package.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Package.swift b/app/Package.swift
index c99fa8f..cb76606 100644
--- a/app/Package.swift
+++ b/app/Package.swift
@@ -21,6 +21,10 @@ let package = Package(
                 .linkedLibrary("vizdaw_ffi"),
                 .linkedFramework("CoreAudio"),
                 .linkedFramework("AudioToolbox"),
+                // Plugin editors: AppKit views and AU generic views (the
+                // static library's own link requests do not reach SwiftPM).
+                .linkedFramework("AppKit"),
+                .linkedFramework("CoreAudioKit"),
                 .linkedFramework("CoreMIDI"),
                 .linkedFramework("CoreFoundation"),
                 .linkedFramework("Foundation"),
VIZDAW_EOF
```
Change `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Tests/VizDAWAppModelTests/FakeSession.swift b/app/Tests/VizDAWAppModelTests/FakeSession.swift
index 159d5df..74dcd01 100644
--- a/app/Tests/VizDAWAppModelTests/FakeSession.swift
+++ b/app/Tests/VizDAWAppModelTests/FakeSession.swift
@@ -196,6 +196,33 @@ final class FakeSession: SessionProtocol, @unchecked Sendable {
         try mixerCall("commitParams \(slot) " + values.map { "\($0.id)=\($0.value)" }.joined(separator: ","))
     }
     func endPluginGestures(slot: UInt64) throws { endedGestures.append(slot) }
+
+    // MARK: Plugin windows
+
+    /// Editor sizes by slot; slots not listed have no editor.
+    var editors: [UInt64: EditorSize] = [:]
+    var openedEditors: [UInt64] = []
+    var closedEditors: [UInt64] = []
+    var windowsOpen: Set<UInt64> = []
+
+    func openEditor(slot: UInt64, parent: UInt64) throws -> EditorSize {
+        guard let e = editors[slot] else {
+            throw VizError.Engine(message: "editor unavailable: the plugin has no editor")
+        }
+        openedEditors.append(slot)
+        return e
+    }
+    func resizeEditor(slot: UInt64, width: UInt32, height: UInt32) throws -> EditorSize {
+        guard let e = editors[slot] else {
+            throw VizError.Engine(message: "editor unavailable: the plugin has no editor")
+        }
+        return e.resizable ? EditorSize(width: width, height: height, resizable: true) : e
+    }
+    func closeEditor(slot: UInt64) throws { closedEditors.append(slot) }
+    func setWindowOpen(slot: UInt64, open: Bool) throws {
+        if open { windowsOpen.insert(slot) } else { windowsOpen.remove(slot) }
+    }
+    func openWindows() throws -> [UInt64] { windowsOpen.sorted() }
 }
 
 func seq(_ id: UInt64, _ name: String, topLevel: Bool, uses: UInt32 = 0) -> SeqRow {
VIZDAW_EOF
```

- [ ] **Step 6: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every Rust test passes (550); 36 Swift tests pass.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-ffi/Cargo.toml crates/core-ffi/src/editor.rs crates/core-ffi/src/session.rs \
  crates/core-ffi/src/plugins.rs crates/core-ffi/src/snapshot.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/editors.rs app/Package.swift \
  app/Tests/VizDAWAppModelTests/FakeSession.swift Cargo.lock
git commit -m "$(cat <<'MSG'
feat(core-ffi): plugin editors and saved plugin windows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 9: Autosave, crash marker, safe mode; saving captures plugins


**Files:**
- Modify: `crates/core-model/src/file.rs` (`load_autosave`)
- Test: `crates/core-model/tests/plugin_state.rs` (one test appended)
- Create: `crates/core-ffi/src/recovery.rs`
- Modify: `crates/core-ffi/src/session.rs`, `crates/core-ffi/src/lib.rs`
- Create: `crates/core-ffi/tests/recovery.rs`
- Test: `app/Tests/VizDAWAppModelTests/FakeSession.swift`

**Interfaces:**
- Consumes: Task 1's `PluginHost::{hold, held, loading}`; Task 4's `pump` path; Task 5's `mixer::slot_ids`; 3b's `PluginHost::capture_all`; `core_model::file::{autosave, list_autosaves, write_atomic, load, save}`.
- Produces:
  - `core_model::file::load_autosave(package: &Path, autosave: &Path) -> Result<Document>`.
  - `vizdaw_ffi::recovery::{AUTOSAVE_INTERVAL, support_dir() -> PathBuf, default_support_dir() -> PathBuf, set_support_dir(path: String) (exported), autosave_interval() -> Duration, opened(&Path, &[PluginRef]), write_marker(&Path, &[PluginRef]), closed(&Path), recovery_check(path: String) -> RecoveryInfo (exported)}`; record `RecoveryInfo { crashed, loading: Vec<PluginRefRow>, autosave: Option<String> }`; test hooks `set_autosave_interval_ms(u64)`, `forget_open(&Path)`.
  - `OpenOptions { plugins_off: bool, autosave: Option<String> }`; `#[uniffi::constructor] Session::open_with(path, options)`; `Session::load_plugins(slots: Vec<u64>)`, `Session::held_slots() -> Vec<u64>`, `Session::close()`; `session::PLUGINS_OFF` ("Plugins off (safe mode)"); test hook `session::simulate_crash(Arc<Session>)`.

Rulings 9–12 and 14. A session with a path writes its marker before any plugin loads and rewrites it whenever the set of loading plugins changes; `close()` and `Drop` remove it. `recovery_check` reports a crash, the plugins it was loading, and the newest autosave when it is newer than `project.json`. Saving captures every running plugin first; autosave runs from the timer step under the same lock. `DocSnapshot.dirty` also covers plugin-only state changes. In `test-hooks` builds the support folder defaults to a temporary one, so no test touches the user's.

- [ ] **Step 1: Write the failing tests**

Change `crates/core-model/tests/plugin_state.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-model/tests/plugin_state.rs b/crates/core-model/tests/plugin_state.rs
index c01b6ae..406e4ed 100644
--- a/crates/core-model/tests/plugin_state.rs
+++ b/crates/core-model/tests/plugin_state.rs
@@ -164,3 +164,16 @@ fn an_unreadable_autosave_skips_the_prune_but_save_succeeds() {
 fn a_garbage_autosave_skips_the_prune_but_save_succeeds() {
     prune_is_skipped_with("garbage", |p| std::fs::write(p, b"not json {{").unwrap());
 }
+
+#[test]
+fn an_autosave_loads_with_the_packages_blobs() {
+    let pkg = temp_package("restore");
+    save(&pad(), &pkg).unwrap();
+    let (d, id) = capture(&pad(), PluginInstanceId(0), b"autosaved").unwrap();
+    let path = autosave(&d, &pkg, Timestamp(7000)).unwrap();
+    let restored = core_model::file::load_autosave(&pkg, &path).unwrap();
+    assert_eq!(restored, d);
+    assert_eq!(restored.state_blob(id), Some(&b"autosaved"[..]));
+    assert!(core_model::file::load_autosave(&pkg, &pkg.join("Autosave/none.json")).is_err());
+    std::fs::remove_dir_all(&pkg).unwrap();
+}
VIZDAW_EOF
```
Create `crates/core-ffi/tests/recovery.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/tests
cat > crates/core-ffi/tests/recovery.rs <<'VIZDAW_EOF'
//! Crash recovery through the FFI (spec §10): the crash marker names the
//! plugins loading and goes on a clean close; a crash leaves it, and the
//! project then opens with plugins off (each slot loads on request);
//! autosaves capture plugin state, wait for drags, and restore as an
//! edited document; saving captures plugin state too.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::recovery::{recovery_check, set_autosave_interval_ms, set_support_dir};
use vizdaw_ffi::session::simulate_crash;
use vizdaw_ffi::{OpenOptions, ParamValue, PluginFormatTag, PluginRefRow, Session, SlotStatus};

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time, each with its own support folder.
fn serial() -> (MutexGuard<'static, ()>, tempfile::TempDir) {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    set_autosave_interval_ms(120_000);
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    let support = tempfile::tempdir().unwrap();
    set_support_dir(support.path().to_string_lossy().into_owned()).unwrap();
    (g, support)
}

fn gain() -> PluginRefRow {
    PluginRefRow {
        format: PluginFormatTag::Clap,
        id: "vizdaw.test.gain".into(),
        name: "Test Gain".into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

/// The demo with a Test Gain insert, saved: (folder, path, slot).
fn project() -> (tempfile::TempDir, String, u64) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.vizdaw").to_string_lossy().into_owned();
    let s = Session::new_demo().unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.add_insert(synth, None, gain()).unwrap();
    s.save_as(path.clone()).unwrap();
    let id = s.plugin_slots().unwrap()[0].id;
    (dir, path, id)
}

fn markers(support: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(support.join("open-projects"))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn idle_until(s: &Session, mut done: impl FnMut(&Session) -> bool) {
    let t = Instant::now();
    while !done(s) {
        assert!(t.elapsed() < Duration::from_secs(10), "timed out");
        s.idle().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn status(s: &Session) -> SlotStatus {
    s.plugin_slots().unwrap()[0].status.clone()
}

#[test]
fn the_marker_names_loading_plugins_and_a_clean_close_removes_it() {
    let (_g, support) = serial();
    let (_d, path, _id) = project();
    assert!(
        markers(support.path()).is_empty(),
        "the untitled save left none"
    );
    let s = Session::open(path.clone()).unwrap();
    let m = markers(support.path());
    assert_eq!(m.len(), 1);
    let text = std::fs::read_to_string(&m[0]).unwrap();
    assert!(text.contains("vizdaw.test.gain"), "names the load: {text}");
    let check = recovery_check(path.clone()).unwrap();
    assert!(!check.crashed, "open here is not a crash");
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    let text = std::fs::read_to_string(&m[0]).unwrap();
    assert!(!text.contains("vizdaw.test.gain"), "loaded: {text}");
    s.close().unwrap();
    assert!(markers(support.path()).is_empty());
    s.close().unwrap();
    drop(s);
    assert!(markers(support.path()).is_empty());
}

#[test]
fn after_a_crash_the_project_opens_with_plugins_off() {
    let (_g, _support) = serial();
    let (_d, path, id) = project();
    let s = Session::open(path.clone()).unwrap();
    // It crashes while the plugin is still loading.
    simulate_crash(s);
    let check = recovery_check(path.clone()).unwrap();
    assert!(check.crashed);
    assert_eq!(check.loading.len(), 1);
    assert_eq!(check.loading[0].id, "vizdaw.test.gain");
    assert_eq!(check.autosave, None);
    let s = Session::open_with(
        path.clone(),
        OpenOptions {
            plugins_off: true,
            autosave: None,
        },
    )
    .unwrap();
    for _ in 0..20 {
        s.idle().unwrap();
    }
    assert_eq!(
        status(&s),
        SlotStatus::Placeholder {
            reason: "Plugins off (safe mode)".into()
        }
    );
    assert_eq!(s.held_slots().unwrap(), vec![id]);
    assert!(!s.snapshot().unwrap().dirty, "holding is not an edit");
    s.load_plugins(vec![id]).unwrap();
    assert!(s.held_slots().unwrap().is_empty());
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    // Open here now: no longer reported as a crash.
    assert!(!recovery_check(path).unwrap().crashed);
}

#[test]
fn saving_captures_plugin_state() {
    let (_g, _support) = serial();
    let (_d, path, id) = project();
    let s = Session::open(path.clone()).unwrap();
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    s.set_param(id, 0, 0.5).unwrap();
    s.commit_params(id, vec![ParamValue { id: 0, value: 0.5 }])
        .unwrap();
    s.save().unwrap();
    let doc = core_model::file::load(Path::new(&path)).unwrap();
    let slot = doc
        .plugin_slot(core_model::ids::PluginInstanceId(id))
        .unwrap();
    assert!(slot.state.is_some(), "captured on save");
    assert!(slot.params.is_empty(), "the overlay went into the state");
    assert!(!s.snapshot().unwrap().dirty);
}

#[test]
fn autosave_runs_after_edits_and_restores_as_edited() {
    let (_g, _support) = serial();
    let (_d, path, _id) = project();
    let s = Session::open(path.clone()).unwrap();
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    set_autosave_interval_ms(30);
    let synth = s.mixer().unwrap().strips[0].id;
    // Not during a drag.
    s.begin_edit("Fader".into()).unwrap();
    s.set_gain(synth, -9.0).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    s.idle().unwrap();
    let autosaves = |p: &str| core_model::file::list_autosaves(Path::new(p)).unwrap();
    assert!(autosaves(&path).is_empty(), "no autosave inside a drag");
    s.end_edit().unwrap();
    idle_until(&s, |_| !autosaves(&path).is_empty());
    let saved = autosaves(&path)[0].clone();
    let auto = core_model::file::load_autosave(Path::new(&path), &saved).unwrap();
    let slot = auto.mixer().inserts().next().unwrap().2.clone();
    assert!(
        slot.state.is_some(),
        "plugin state captured into the autosave"
    );
    // The project closes without saving; reopening offers the autosave.
    drop(s);
    let check = recovery_check(path.clone()).unwrap();
    assert!(!check.crashed);
    let auto_path = check.autosave.expect("a newer autosave");
    let r = Session::open_with(
        path.clone(),
        OpenOptions {
            plugins_off: false,
            autosave: Some(auto_path),
        },
    )
    .unwrap();
    assert_eq!(r.mixer().unwrap().strips[0].gain_db, -9.0);
    assert!(r.snapshot().unwrap().dirty, "Save keeps the restored copy");
    r.save().unwrap();
    assert!(recovery_check(path).unwrap().autosave.is_none());
}

#[test]
fn a_plugin_loading_into_an_edited_project_autosaves_at_once() {
    let (_g, _support) = serial();
    let (_d, path, _id) = project();
    let s = Session::open(path.clone()).unwrap();
    let synth = s.mixer().unwrap().strips[0].id;
    s.set_gain(synth, -3.0).unwrap();
    // The interval is long; the load itself triggers the autosave.
    idle_until(&s, |s| status(s) == SlotStatus::Live);
    s.idle().unwrap();
    assert_eq!(
        core_model::file::list_autosaves(Path::new(&path))
            .unwrap()
            .len(),
        1
    );
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test plugin_state
cargo test -p core-ffi --test recovery
```

Expected: compile errors — no function `load_autosave` in `core_model::file`; unresolved `vizdaw_ffi::recovery`, `vizdaw_ffi::OpenOptions`, `vizdaw_ffi::session::simulate_crash`.

- [ ] **Step 3: Implement**

Change `crates/core-model/src/file.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-model/src/file.rs b/crates/core-model/src/file.rs
index 88cb210..2c026f3 100644
--- a/crates/core-model/src/file.rs
+++ b/crates/core-model/src/file.rs
@@ -250,6 +250,15 @@ pub fn load(package: &Path) -> Result<Document> {
     Ok(doc)
 }
 
+/// Loads the autosave copy at `autosave` (a file in the package's
+/// `Autosave/` folder) with the package's plugin state blobs ("Restore
+/// autosave", spec §10).
+pub fn load_autosave(package: &Path, autosave: &Path) -> Result<Document> {
+    let mut doc = from_json(&fs::read_to_string(autosave)?)?;
+    read_states(&mut doc, package);
+    Ok(doc)
+}
+
 /// Autosave copies in the package, newest first.
 pub fn list_autosaves(package: &Path) -> Result<Vec<PathBuf>> {
     let dir = package.join("Autosave");
VIZDAW_EOF
```
Create `crates/core-ffi/src/recovery.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p crates/core-ffi/src
cat > crates/core-ffi/src/recovery.rs <<'VIZDAW_EOF'
//! Crash recovery (spec §10): the crash marker a project keeps while it is
//! open, what the app checks before opening a project (a leftover marker,
//! a newer autosave), and the autosave interval.

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use crate::snapshot::PluginRefRow;
use core_model::file;
use core_model::mixer::PluginRef;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// How often a project with unsaved changes is autosaved (spec §10).
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);

static SUPPORT: Mutex<Option<PathBuf>> = Mutex::new(None);
/// Projects open in this process (their markers are not crashes).
static OPEN: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());
static INTERVAL_MS: AtomicU64 = AtomicU64::new(120_000);

/// Where VizDAW keeps its own files: set by the app, else
/// `~/Library/Application Support/VizDAW`. Test builds (`test-hooks`)
/// default to a per-process temporary folder, so tests never touch the
/// user's.
pub fn support_dir() -> PathBuf {
    if let Some(p) = lock(&SUPPORT).clone() {
        return p;
    }
    #[cfg(feature = "test-hooks")]
    {
        std::env::temp_dir().join(format!("vizdaw-test-support-{}", std::process::id()))
    }
    #[cfg(not(feature = "test-hooks"))]
    {
        default_support_dir()
    }
}

/// `~/Library/Application Support/VizDAW` (macOS), `~/.local/share/vizdaw`
/// elsewhere.
pub fn default_support_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/VizDAW")
    } else {
        home.join(".local/share/vizdaw")
    }
}

/// Makes `path` the folder for crash markers (and, in the app, the plugin
/// catalog's folder).
#[uniffi::export]
pub fn set_support_dir(path: String) -> Result<(), VizError> {
    ffi_guard(|| {
        *lock(&SUPPORT) = Some(PathBuf::from(path));
        Ok(())
    })
}

/// The autosave interval (tests shorten it).
pub fn autosave_interval() -> Duration {
    Duration::from_millis(INTERVAL_MS.load(Ordering::Relaxed))
}

/// Sets the autosave interval (test hook).
#[cfg(feature = "test-hooks")]
pub fn set_autosave_interval_ms(ms: u64) {
    INTERVAL_MS.store(ms, Ordering::Relaxed);
}

fn key(project: &Path) -> PathBuf {
    std::fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf())
}

/// `open-projects/<FNV-1a of the project path>.json`.
fn marker_path(project: &Path) -> PathBuf {
    let k = key(project);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in k.to_string_lossy().as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    support_dir()
        .join("open-projects")
        .join(format!("{h:016x}.json"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Marker {
    project: String,
    /// The plugins being loaded when it was written.
    loading: Vec<PluginRef>,
}

/// Notes that `project` is open here and writes its marker.
pub fn opened(project: &Path, loading: &[PluginRef]) -> Result<(), String> {
    lock(&OPEN).insert(key(project));
    write_marker(project, loading)
}

/// Rewrites `project`'s marker naming the plugins loading now.
pub fn write_marker(project: &Path, loading: &[PluginRef]) -> Result<(), String> {
    let path = marker_path(project);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let m = Marker {
        project: key(project).to_string_lossy().into_owned(),
        loading: loading.to_vec(),
    };
    let text = serde_json::to_string(&m).map_err(|e| e.to_string())?;
    file::write_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())
}

/// A clean close: the marker goes.
pub fn closed(project: &Path) {
    lock(&OPEN).remove(&key(project));
    let _ = std::fs::remove_file(marker_path(project));
}

/// Forgets that `project` is open without removing its marker, as a crash
/// would (test hook).
#[cfg(feature = "test-hooks")]
pub fn forget_open(project: &Path) {
    lock(&OPEN).remove(&key(project));
}

/// What the app must ask before opening a project (spec §10).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecoveryInfo {
    /// VizDAW quit unexpectedly while it was open (a marker is left and no
    /// window here has it open): offer "Open with Plugins Off".
    pub crashed: bool,
    /// The plugins it was loading then (offer to disable them).
    pub loading: Vec<PluginRefRow>,
    /// The newest autosave, when it is newer than `project.json`: offer
    /// "Restore autosave".
    pub autosave: Option<String>,
}

/// Checks `path` before it is opened.
#[uniffi::export]
pub fn recovery_check(path: String) -> Result<RecoveryInfo, VizError> {
    ffi_guard(|| {
        let p = PathBuf::from(&path);
        let marker = std::fs::read_to_string(marker_path(&p)).ok();
        let crashed = marker.is_some() && !lock(&OPEN).contains(&key(&p));
        let loading = marker
            .filter(|_| crashed)
            .and_then(|t| serde_json::from_str::<Marker>(&t).ok())
            .map(|m| m.loading.iter().map(Into::into).collect())
            .unwrap_or_default();
        let mtime = |f: &Path| std::fs::metadata(f).and_then(|m| m.modified()).ok();
        let saved = mtime(&p.join(file::PROJECT_FILE));
        let autosave = file::list_autosaves(&p)
            .ok()
            .and_then(|v| v.into_iter().next())
            .filter(|a| match (mtime(a), saved) {
                (Some(a), Some(s)) => a > s,
                (Some(_), None) => true,
                _ => false,
            })
            .map(|a| a.to_string_lossy().into_owned());
        Ok(RecoveryInfo {
            crashed,
            loading,
            autosave,
        })
    })
}
VIZDAW_EOF
```
Change `crates/core-ffi/src/session.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/session.rs b/crates/core-ffi/src/session.rs
index 45428f1..eb82bdb 100644
--- a/crates/core-ffi/src/session.rs
+++ b/crates/core-ffi/src/session.rs
@@ -6,6 +6,7 @@ use crate::guard::{ffi_guard, lock};
 use crate::host;
 use crate::mixer::{self, DestRow, MixerLevels, MixerSnapshot};
 use crate::plugins;
+use crate::recovery;
 use crate::snapshot::{
     DocSnapshot, IdleUpdate, LaneRow, SlotRow, TransportState, lane_rows, position_bbu, seconds,
     seq_id, seq_rows, warnings,
@@ -20,14 +21,15 @@ use core_model::command::sequence::SequenceCmd;
 use core_model::document::DestKind;
 use core_model::document::Document;
 use core_model::file;
-use core_model::history::History;
+use core_model::history::{History, Timestamp};
 use core_model::ids::{DestId, LaneId, PluginInstanceId, SeqId, StripId};
-use core_model::mixer::{AuxSend, Output, ParamId, PluginSlot};
+use core_model::mixer::{AuxSend, Output, ParamId, PluginRef, PluginSlot};
 use core_model::selection::ViewFilter;
 use core_model::time::Tick;
 use std::path::{Path, PathBuf};
 use std::sync::atomic::{AtomicU64, Ordering};
 use std::sync::{Arc, Mutex};
+use std::time::Instant;
 
 /// Package extension.
 pub const EXTENSION: &str = "vizdaw";
@@ -55,6 +57,15 @@ struct Inner {
     windows: std::collections::BTreeSet<u64>,
     /// What plugin timer steps found since the last `idle` call.
     news: News,
+    /// The plugins the crash marker names (as last written).
+    marked: Vec<PluginRef>,
+    /// When the project was last autosaved (or opened).
+    last_autosave: Instant,
+    /// A plugin loaded: autosave at the next tick (spec §10).
+    autosave_soon: bool,
+    /// A plugin changed its own state (not a parameter) since the last
+    /// save: unsaved, though the document did not change.
+    plugin_dirty: bool,
 }
 
 /// What plugin timer steps found, handed out by the next `idle`.
@@ -80,6 +91,10 @@ impl Inner {
             held: Vec::new(),
             windows: std::collections::BTreeSet::new(),
             news: News::default(),
+            marked: Vec::new(),
+            last_autosave: Instant::now(),
+            autosave_soon: false,
+            plugin_dirty: false,
         }
     }
 
@@ -118,12 +133,18 @@ impl Inner {
         self.history.doc()
     }
 
+    /// The document changed since it was opened or saved, or a plugin
+    /// changed its own state.
+    fn unsaved(&self) -> bool {
+        self.plugin_dirty || *self.doc() != self.clean
+    }
+
     fn snapshot(&self) -> Result<DocSnapshot, VizError> {
         let doc = self.doc();
         Ok(DocSnapshot {
             version: self.version,
             title: title(self.path.as_deref()),
-            dirty: *doc != self.clean,
+            dirty: self.unsaved(),
             undo_label: self.history.undo_label().map(str::to_string),
             redo_label: self.history.redo_label().map(str::to_string),
             sequences: seq_rows(doc)?,
@@ -167,16 +188,124 @@ impl std::fmt::Debug for Session {
     }
 }
 
+/// Saving and autosaving one at a time (the package's blob pruning reads
+/// the autosaves; 3a: the FFI serializes them).
+static SAVE_LOCK: Mutex<()> = Mutex::new(());
+
+/// What a held slot shows in safe mode.
+pub const PLUGINS_OFF: &str = "Plugins off (safe mode)";
+
 impl Session {
-    /// A session for `doc`; its plugins start loading on this thread.
-    fn with(doc: Document, path: Option<PathBuf>) -> Result<Arc<Session>, VizError> {
+    /// A session for `doc`; its plugins start loading on this thread (none
+    /// with `plugins_off`: every slot is held, spec §10). A project with a
+    /// path gets its crash marker before any plugin loads.
+    fn with(
+        doc: Document,
+        path: Option<PathBuf>,
+        plugins_off: bool,
+    ) -> Result<Arc<Session>, VizError> {
         let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
+        // Without a marker the project still opens; recovery is just not
+        // offered after a crash.
+        let unmarked = path.as_deref().and_then(|p| recovery::opened(p, &[]).err());
         plugins::create(id)?;
-        plugins::with(id, |p| p.map(|p| p.sync(&doc)));
-        Ok(Arc::new(Session {
+        plugins::with(id, |p| {
+            if let Some(p) = p {
+                if plugins_off {
+                    for slot in mixer::slot_ids(&doc) {
+                        p.hold(slot, Some(PLUGINS_OFF.to_string()));
+                    }
+                }
+                p.sync(&doc);
+            }
+        });
+        let s = Arc::new(Session {
             id,
             inner: Mutex::new(Inner::new(doc, path)),
-        }))
+        });
+        {
+            let mut inner = lock(&s.inner);
+            if let Some(e) = unmarked {
+                inner
+                    .news
+                    .problems
+                    .push(format!("crash marker not written: {e}"));
+            }
+            s.mark(&mut inner);
+        }
+        Ok(s)
+    }
+
+    /// Rewrites the crash marker when the plugins loading changed (before
+    /// any of them can run code on this thread).
+    fn mark(&self, inner: &mut Inner) {
+        let Some(path) = inner.path.clone() else {
+            return;
+        };
+        let loading: Vec<PluginRef> = plugins::with(self.id, |p| {
+            p.map(|p| p.loading().into_iter().map(|(_, r)| r).collect())
+                .unwrap_or_default()
+        });
+        if loading != inner.marked {
+            if let Err(e) = recovery::write_marker(&path, &loading) {
+                inner
+                    .news
+                    .problems
+                    .push(format!("crash marker not written: {e}"));
+            }
+            inner.marked = loading;
+        }
+    }
+
+    /// Captures every running plugin's state into the document (spec §4:
+    /// on save and autosave). Refused inside an open edit.
+    fn capture_all(&self, inner: &mut Inner) {
+        let history = &mut inner.history;
+        let failed = plugins::with(self.id, |p| {
+            p.map(|p| p.capture_all(history)).unwrap_or_default()
+        });
+        for (id, e) in failed {
+            let line = format!("{}: state not saved ({e})", slot_name(inner.doc(), id));
+            inner.news.problems.push(line);
+        }
+    }
+
+    /// Autosaves (spec §10) a project with a path and unsaved changes,
+    /// every [`recovery::autosave_interval`] and right after a plugin
+    /// loaded; never inside an open edit (a drag).
+    fn maybe_autosave(&self, inner: &mut Inner) {
+        let Some(path) = inner.path.clone() else {
+            return;
+        };
+        let due =
+            inner.autosave_soon || inner.last_autosave.elapsed() >= recovery::autosave_interval();
+        if !due || inner.history.in_transaction() {
+            return;
+        }
+        if !inner.unsaved() {
+            // Nothing unsaved: nothing to keep.
+            inner.autosave_soon = false;
+            inner.last_autosave = Instant::now();
+            return;
+        }
+        inner.autosave_soon = false;
+        inner.last_autosave = Instant::now();
+        self.capture_all(inner);
+        let _g = lock(&SAVE_LOCK);
+        if let Err(e) = file::autosave(inner.doc(), &path, Timestamp::now()) {
+            inner.news.problems.push(format!("autosave failed: {e}"));
+        }
+    }
+
+    /// Saves to `path`: plugin states captured first.
+    fn save_to(&self, inner: &mut Inner, path: &Path) -> Result<(), VizError> {
+        self.capture_all(inner);
+        let _g = lock(&SAVE_LOCK);
+        file::save(inner.doc(), path)?;
+        write_windows(path, &inner.windows);
+        inner.clean = inner.doc().clone();
+        inner.plugin_dirty = false;
+        Ok(())
     }
 
     /// One plugin timer step: the host services its plugins (with the
@@ -184,6 +313,7 @@ impl Session {
     /// and finished gestures become undo steps. Returns true when the
     /// document changed.
     fn pump(&self, inner: &mut Inner) -> bool {
+        self.mark(inner);
         let report = {
             let doc = inner.doc();
             plugins::with(self.id, |p| p.map(|p| host::service(self.id, doc, p)))
@@ -210,8 +340,17 @@ impl Session {
             .news
             .params_changed
             .extend(r.params_changed.iter().map(|i| i.0));
+        inner.autosave_soon |= !r.loaded.is_empty();
+        if !r.state_dirty.is_empty() && !inner.plugin_dirty {
+            inner.plugin_dirty = true;
+            // The title shows Edited.
+            inner.news.doc_changed = true;
+        }
         inner.held.extend(r.gestures);
-        self.commit_held(inner)
+        let changed = self.commit_held(inner);
+        self.mark(inner);
+        self.maybe_autosave(inner);
+        changed
     }
 
     /// Commits the gestures waiting for an open edit (a drag) to end.
@@ -251,6 +390,7 @@ impl Session {
         inner.history.execute(&cmd, &ViewFilter::default())?;
         inner.version += 1;
         inner.reload_host(self.id);
+        self.mark(&mut inner);
         inner.snapshot()
     }
 
@@ -301,6 +441,7 @@ impl Session {
         }
         inner.version += 1;
         inner.reload_host(self.id);
+        self.mark(&mut inner);
         inner.snapshot()
     }
 
@@ -320,7 +461,7 @@ impl Session {
     /// An untitled, empty document.
     #[uniffi::constructor]
     pub fn new_empty() -> Result<Arc<Session>, VizError> {
-        ffi_guard(|| Session::with(Document::new(), None))
+        ffi_guard(|| Session::with(Document::new(), None, false))
     }
 
     /// An untitled copy of the demo project.
@@ -328,22 +469,51 @@ impl Session {
     pub fn new_demo() -> Result<Arc<Session>, VizError> {
         ffi_guard(|| {
             let doc = core_engine::fixtures::demo_document()?;
-            Session::with(doc, None)
+            Session::with(doc, None, false)
         })
     }
 
     /// Opens the `.vizdaw` package at `path`.
     #[uniffi::constructor]
     pub fn open(path: String) -> Result<Arc<Session>, VizError> {
+        Session::open_with(
+            path,
+            OpenOptions {
+                plugins_off: false,
+                autosave: None,
+            },
+        )
+    }
+
+    /// Opens the `.vizdaw` package at `path` after the app asked what
+    /// `recovery_check` found (spec §10): with every plugin slot held
+    /// ("Open with Plugins Off"), and/or from an autosave copy ("Restore
+    /// autosave": it opens as Edited, so Save keeps it).
+    #[uniffi::constructor]
+    pub fn open_with(path: String, options: OpenOptions) -> Result<Arc<Session>, VizError> {
         ffi_guard(|| {
             let p = PathBuf::from(&path);
             if !p.join(file::PROJECT_FILE).is_file() {
                 return Err(VizError::NotFound { path });
             }
-            let doc = file::load(&p)?;
+            let (doc, clean) = match &options.autosave {
+                Some(a) => {
+                    let doc = file::load_autosave(&p, Path::new(a))?;
+                    let clean = file::load(&p).unwrap_or_else(|_| doc.clone());
+                    (doc, clean)
+                }
+                None => {
+                    let doc = file::load(&p)?;
+                    (doc.clone(), doc)
+                }
+            };
             let windows = read_windows(&p);
-            let s = Session::with(doc, Some(p))?;
-            lock(&s.inner).windows = windows;
+            let s = Session::with(doc, Some(p), options.plugins_off)?;
+            {
+                let mut inner = lock(&s.inner);
+                inner.windows = windows;
+                inner.clean = clean;
+            }
             Ok(s)
         })
     }
@@ -355,10 +525,7 @@ impl Session {
             let path = inner.path.clone().ok_or_else(|| VizError::Io {
                 message: "the document has no file yet; use Save As".into(),
             })?;
-            file::save(inner.doc(), &path)?;
-            write_windows(&path, &inner.windows);
-            inner.clean = inner.doc().clone();
-            Ok(())
+            self.save_to(&mut inner, &path)
         })
     }
 
@@ -368,10 +535,15 @@ impl Session {
         ffi_guard(|| {
             let mut inner = lock(&self.inner);
             let p = with_extension(&path);
-            file::save(inner.doc(), &p)?;
-            write_windows(&p, &inner.windows);
-            inner.clean = inner.doc().clone();
-            inner.path = Some(p);
+            self.save_to(&mut inner, &p)?;
+            if inner.path.as_deref() != Some(p.as_path()) {
+                // The marker follows the project to its new place.
+                if let Some(old) = inner.path.take() {
+                    recovery::closed(&old);
+                }
+                let _ = recovery::opened(&p, &inner.marked);
+                inner.path = Some(p);
+            }
             Ok(())
         })
     }
@@ -1127,8 +1299,79 @@ impl Session {
     }
 }
 
+/// Safe mode (spec §10) and closing.
+#[uniffi::export]
+impl Session {
+    /// Loads held slots (safe mode's "Load" on a slot, "Load All" on a
+    /// strip or the document): they load from their saved state.
+    pub fn load_plugins(&self, slots: Vec<u64>) -> Result<(), VizError> {
+        ffi_guard(|| {
+            let mut inner = lock(&self.inner);
+            plugins::with(self.id, |p| {
+                if let Some(p) = p {
+                    for s in slots {
+                        p.hold(PluginInstanceId(s), None);
+                    }
+                }
+            });
+            inner.reload_host(self.id);
+            self.mark(&mut inner);
+            inner.news.slots_changed = true;
+            Ok(())
+        })
+    }
+
+    /// The slots safe mode holds (placeholders until loaded).
+    pub fn held_slots(&self) -> Result<Vec<u64>, VizError> {
+        ffi_guard(|| {
+            let inner = lock(&self.inner);
+            let slots = mixer::slot_ids(inner.doc());
+            Ok(
+                plugins::with(self.id, |p| p.map(|p| p.held()).unwrap_or_default())
+                    .into_iter()
+                    .filter(|s| slots.contains(s))
+                    .map(|s| s.0)
+                    .collect(),
+            )
+        })
+    }
+
+    /// A clean close (the window closes, or the app quits): the crash
+    /// marker goes. Safe to call more than once.
+    pub fn close(&self) -> Result<(), VizError> {
+        ffi_guard(|| {
+            if let Some(p) = lock(&self.inner).path.clone() {
+                recovery::closed(&p);
+            }
+            Ok(())
+        })
+    }
+}
+
+/// How to open a project after a crash or with a newer autosave.
+#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
+pub struct OpenOptions {
+    /// Hold every plugin slot ("Open with Plugins Off").
+    pub plugins_off: bool,
+    /// Open this autosave copy instead of `project.json`.
+    pub autosave: Option<String>,
+}
+
+/// Leaves session `s` the way a crash would: its crash marker stays and it
+/// no longer counts as open here (test hook).
+#[cfg(feature = "test-hooks")]
+pub fn simulate_crash(s: Arc<Session>) {
+    if let Some(p) = lock(&s.inner).path.clone() {
+        recovery::forget_open(&p);
+    }
+    std::mem::forget(s);
+}
+
 impl Drop for Session {
     fn drop(&mut self) {
+        if let Some(p) = lock(&self.inner).path.clone() {
+            recovery::closed(&p);
+        }
         // The engine may still hold this session's processors: when its
         // plugin host feeds the engine, the engine goes first (it gives
         // them back), then the plugin host.
VIZDAW_EOF
```
Change `crates/core-ffi/src/lib.rs`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/crates/core-ffi/src/lib.rs b/crates/core-ffi/src/lib.rs
index f77461f..2537973 100644
--- a/crates/core-ffi/src/lib.rs
+++ b/crates/core-ffi/src/lib.rs
@@ -23,6 +23,7 @@ pub mod guard;
 pub mod host;
 pub mod mixer;
 pub mod plugins;
+pub mod recovery;
 pub mod session;
 pub mod snapshot;
 
@@ -36,7 +37,8 @@ pub use guard::ffi_guard;
 pub use mixer::{
     BusRow, DestRow, MeterRow, MixerLevels, MixerSnapshot, SendRow, StripKindRow, StripRow,
 };
-pub use session::Session;
+pub use recovery::RecoveryInfo;
+pub use session::{OpenOptions, Session};
 pub use snapshot::{
     DocSnapshot, IdleUpdate, LaneRow, ParamRow, ParamValue, PluginFormatTag, PluginRefRow, SeqRow,
     SlotRow, SlotStatus, TransportState,
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo test -p core-model --test plugin_state
cargo test -p core-ffi
```

Expected: `plugin_state` passes with `an_autosave_loads_with_the_packages_blobs`; `recovery` 5 passed (`the_marker_names_loading_plugins_and_a_clean_close_removes_it`, `after_a_crash_the_project_opens_with_plugins_off`, `saving_captures_plugin_state`, `autosave_runs_after_edits_and_restores_as_edited`, `a_plugin_loading_into_an_edited_project_autosaves_at_once`); every other core-ffi suite still passes.

- [ ] **Step 5: Keep the Swift fake session in step**

Change `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Tests/VizDAWAppModelTests/FakeSession.swift b/app/Tests/VizDAWAppModelTests/FakeSession.swift
index 74dcd01..d2aff9a 100644
--- a/app/Tests/VizDAWAppModelTests/FakeSession.swift
+++ b/app/Tests/VizDAWAppModelTests/FakeSession.swift
@@ -223,6 +223,19 @@ final class FakeSession: SessionProtocol, @unchecked Sendable {
         if open { windowsOpen.insert(slot) } else { windowsOpen.remove(slot) }
     }
     func openWindows() throws -> [UInt64] { windowsOpen.sorted() }
+
+    // MARK: Safe mode and closing
+
+    var held: [UInt64] = []
+    var loadedPlugins: [[UInt64]] = []
+    var closeCalls = 0
+
+    func loadPlugins(slots: [UInt64]) throws {
+        loadedPlugins.append(slots)
+        held.removeAll { slots.contains($0) }
+    }
+    func heldSlots() throws -> [UInt64] { held }
+    func close() throws { closeCalls += 1 }
 }
 
 func seq(_ id: UInt64, _ name: String, topLevel: Bool, uses: UInt32 = 0) -> SeqRow {
VIZDAW_EOF
```

- [ ] **Step 6: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
ls "$HOME/Library/Application Support/VizDAW/open-projects" 2>/dev/null || echo "no markers in the user's folder"
```

Expected: clean; every Rust test passes (556); 36 Swift tests pass; the last line prints `no markers in the user's folder`.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add crates/core-model/src/file.rs crates/core-model/tests/plugin_state.rs \
  crates/core-ffi/src/recovery.rs crates/core-ffi/src/session.rs crates/core-ffi/src/lib.rs \
  crates/core-ffi/tests/recovery.rs app/Tests/VizDAWAppModelTests/FakeSession.swift
git commit -m "$(cat <<'MSG'
feat(core-ffi): autosave, crash marker, safe mode; saving captures plugins

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 10: The catalog in the app and the plugin browser


**Files:**
- Create: `app/Sources/VizDAWAppModel/AppPlugins.swift`, `app/Sources/VizDAWAppModel/PluginBrowser.swift`
- Create: `app/Sources/VizDAW/PluginBrowserView.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift` (`appPlugins`, `AppSetup`, `PluginAlerts`, scan progress in `PluginTicker`, `--no-scan`)
- Modify: `scripts/build-ffi.sh`, `scripts/swift.sh`, `scripts/make-app.sh` (the scanner next to the app)
- Test: `app/Tests/VizDAWAppModelTests/FakeCatalog.swift` (new), `app/Tests/VizDAWAppModelTests/PluginBrowserTests.swift` (new)

**Interfaces:**
- Consumes: Task 7's `PluginCatalogProtocol`, `PluginCatalog.open(config:)`, `CatalogConfig`, `CatalogRowFfi`, `PluginKey`, `PluginKindTag`, `ScanProgress`; Task 9's `setSupportDir(path:)`; 2c's `AlertItem`.
- Produces (Swift, `VizDAWAppModel`): `AppPlugins(open: (CatalogConfig) throws -> any PluginCatalogProtocol)` with `rows`, `progress`, `alert`, `version`, `isOpen`, `isScanning`, `start(config:scan:)`, `refresh()`, `browser(_:) -> [CatalogRowFfi]`, `poll()`, `scan(all:)`, `rescan(_:)`, `setEnabled(_:_:)`, `clearBlocklist(_:)`, `revealPath(_:) -> String?`, `static config(supportDir:scanner:) -> CatalogConfig`; `PluginKey.init(_ plugin: PluginRefRow)`; `PluginBrowserModel(kind:rows:)` with `search`, `groups: [VendorGroup]`, `static groups(_:search:)`, `VendorGroup { vendor, plugins: [PluginRefRow] }`. In `VizDAW`: `appPlugins`, `AppSetup.{supportDir, scanner, startPlugins()}`, `PluginBrowserView(model:pick:)`, `LaunchArguments.{smokeRun, noScan}`.

Rulings 17, 18 and 21. At launch the app sets the support folder (`VIZDAW_SUPPORT_DIR`, else `~/Library/Application Support/VizDAW`; a temporary one for `--smoke`), opens the catalog and scans new or changed plugins unless `--smoke` or `--no-scan`. Catalog problems become one modal alert each. The browser groups usable plugins by vendor (no vendor: "Other", last) and searches name, vendor and id.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/FakeCatalog.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/FakeCatalog.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// A scripted `PluginCatalogProtocol` for the plugin models' tests.
final class FakeCatalog: PluginCatalogProtocol, @unchecked Sendable {
    var table: [CatalogRowFfi] = []
    var problems: [String] = []
    var scans: [Bool] = []
    var rescans: [[PluginKey]] = []
    var enabled: [(PluginKey, Bool)] = []
    var cleared: [PluginKey] = []
    var state = ScanProgress(running: false, done: 0, total: 0)
    var scanError: VizError?

    func browser(kind: PluginKindTag) throws -> [CatalogRowFfi] {
        table.filter { $0.kind == kind && $0.status == .ok }
    }
    func clearBlocklist(plugin: PluginKey) throws { cleared.append(plugin) }
    func progress() throws -> ScanProgress { state }
    func rescan(plugins: [PluginKey]) throws { rescans.append(plugins) }
    func revealPath(plugin: PluginKey) throws -> String? {
        plugin.format == .clap ? "/Library/Audio/Plug-Ins/CLAP/\(plugin.id).clap" : nil
    }
    func rows() throws -> [CatalogRowFfi] { table }
    func scan(all: Bool) throws {
        if let e = scanError { throw e }
        scans.append(all)
        state = ScanProgress(running: true, done: 0, total: 2)
    }
    func setEnabled(plugin: PluginKey, enabled on: Bool) throws {
        enabled.append((plugin, on))
        if let i = table.firstIndex(where: { $0.plugin.id == plugin.id }) {
            table[i].status = on ? .ok : .disabled
        }
    }
    func takeProblem() throws -> String? { problems.isEmpty ? nil : problems.removeFirst() }
}

func catalogRow(
    _ id: String, _ name: String, vendor: String, kind: PluginKindTag = .effect,
    format: PluginFormatTag = .clap, status: PluginStatusRow = .ok
) -> CatalogRowFfi {
    CatalogRowFfi(
        plugin: PluginRefRow(format: format, id: id, name: name, vendor: vendor, version: "1.0"),
        kind: kind, status: status, path: "/p/\(id)")
}
VIZDAW_EOF
```
Create `app/Tests/VizDAWAppModelTests/PluginBrowserTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/PluginBrowserTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct PluginBrowserTests {
    let rows = [
        catalogRow("u.diva", "Diva", vendor: "u-he", kind: .instrument),
        catalogRow("v.zeta", "zeta", vendor: "Vendor"),
        catalogRow("v.alpha", "Alpha", vendor: "Vendor"),
        catalogRow("x.loose", "Loose", vendor: ""),
        catalogRow("aufx/dely/appl", "AUDelay", vendor: "Apple", format: .au),
    ]

    @Test func groupsByVendorWithOtherLast() {
        let g = PluginBrowserModel.groups(rows, search: "")
        #expect(g.map(\.vendor) == ["Apple", "u-he", "Vendor", "Other"])
        #expect(g[2].plugins.map(\.name) == ["Alpha", "zeta"])
    }

    @Test func searchMatchesNameVendorOrIdInAnyCase() {
        #expect(PluginBrowserModel.groups(rows, search: "DIVA").map(\.vendor) == ["u-he"])
        #expect(PluginBrowserModel.groups(rows, search: " vendor ").flatMap(\.plugins).count == 2)
        #expect(PluginBrowserModel.groups(rows, search: "dely").first?.plugins.first?.name == "AUDelay")
        #expect(PluginBrowserModel.groups(rows, search: "nothing").isEmpty)
    }

    @Test func theModelListsItsKindOnly() {
        let cat = FakeCatalog()
        cat.table = rows
        let plugins = AppPlugins(open: { _ in cat })
        plugins.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: false)
        let m = PluginBrowserModel(kind: .instrument) { plugins.browser(.instrument) }
        #expect(m.groups.flatMap(\.plugins).map(\.name) == ["Diva"])
        m.search = "zzz"
        #expect(m.groups.isEmpty)
    }
}

@MainActor
@Suite struct AppPluginsTests {
    @Test func startOpensScansAndShowsAProblemOnce() {
        let cat = FakeCatalog()
        cat.problems = ["plugin settings not read: newer"]
        var configs: [CatalogConfig] = []
        let p = AppPlugins(open: { configs.append($0); return cat })
        let support = URL(fileURLWithPath: "/tmp/VizDAW")
        p.start(config: AppPlugins.config(supportDir: support, scanner: "/x/vizdaw-scan"), scan: true)
        #expect(configs.first?.path == "/tmp/VizDAW/plugins.json")
        #expect(configs.first?.includeAu == true)
        #expect(cat.scans == [false])
        #expect(p.isScanning)
        #expect(p.alert?.message == "plugin settings not read: newer")
        p.alert = nil
        p.refresh()
        #expect(p.alert == nil)
    }

    @Test func aFinishedScanRefreshesTheRows() {
        let cat = FakeCatalog()
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: true)
        #expect(p.rows.isEmpty)
        cat.table = [catalogRow("a", "A", vendor: "V")]
        cat.state = ScanProgress(running: false, done: 2, total: 2)
        p.poll()
        #expect(p.rows.count == 1)
        #expect(!p.isScanning)
    }

    @Test func actionsReachTheCatalogAndErrorsBecomeAlerts() {
        let cat = FakeCatalog()
        cat.table = [catalogRow("a", "A", vendor: "V")]
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: false)
        let key = PluginKey(cat.table[0].plugin)
        p.setEnabled(key, false)
        #expect(p.rows.first?.status == .disabled)
        p.rescan([key])
        p.clearBlocklist(key)
        #expect(cat.rescans == [[key]])
        #expect(cat.cleared == [key])
        #expect(p.revealPath(key) == "/Library/Audio/Plug-Ins/CLAP/a.clap")
        cat.scanError = .NotFound(path: "/x/vizdaw-scan")
        p.scan(all: true)
        #expect(p.alert?.title == "Scan Failed")
    }

    @Test func aCatalogThatCannotOpenIsAnAlert() {
        let p = AppPlugins(open: { _ in throw VizError.Io(message: "denied") })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: true)
        #expect(!p.isOpen)
        #expect(p.alert?.message == "denied")
        #expect(p.browser(.effect).isEmpty)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh
scripts/swift.sh test
```

Expected: compile errors — cannot find `AppPlugins`, `PluginBrowserModel` in scope.

- [ ] **Step 3: Implement**

Create `app/Sources/VizDAWAppModel/AppPlugins.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/AppPlugins.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// The app's plugin catalog (spec §8, §9): opened once at launch, scanned
/// for new or changed plugins, and read by the plugin browser and the
/// Plugin Manager. Problems (a catalog file it cannot read or write, a
/// failed scan) become one alert each.
@Observable
@MainActor
public final class AppPlugins {
    public typealias Open = (CatalogConfig) throws -> any PluginCatalogProtocol

    /// Every known plugin, by name (refreshed after each change and scan).
    public private(set) var rows: [CatalogRowFfi] = []
    public private(set) var progress = ScanProgress(running: false, done: 0, total: 0)
    public var alert: AlertItem?
    /// Bumped whenever `rows` changes (browsers refetch).
    public private(set) var version = 0

    private let open: Open
    private var catalog: (any PluginCatalogProtocol)?

    public init(open: @escaping Open) {
        self.open = open
    }

    public var isOpen: Bool { catalog != nil }
    public var isScanning: Bool { progress.running }

    /// Opens the catalog; with `scan`, new or changed plugins are scanned
    /// in the background.
    public func start(config: CatalogConfig, scan: Bool) {
        do {
            catalog = try open(config)
        } catch {
            alert = AlertItem(error, title: "Plugins Unavailable")
            return
        }
        refresh()
        if scan { self.scan(all: false) }
    }

    /// The configuration the app uses: the catalog in `supportDir`, the
    /// scanner at `scanner`, the standard CLAP folders and Audio Units.
    public static func config(supportDir: URL, scanner: String) -> CatalogConfig {
        CatalogConfig(
            path: supportDir.appendingPathComponent("plugins.json").path,
            scanner: scanner, clapDirs: [], includeAu: true, timeoutMs: nil)
    }

    /// Rereads the rows and shows a catalog problem, if there is one.
    public func refresh() {
        guard let c = catalog else { return }
        do {
            rows = try c.rows()
            version += 1
            if let p = try c.takeProblem(), alert == nil {
                alert = AlertItem(title: "Plugin Settings", message: p)
            }
        } catch {
            alert = AlertItem(error, title: "Plugins Unavailable")
        }
    }

    /// The plugin browser's list for `kind` (usable plugins only).
    public func browser(_ kind: PluginKindTag) -> [CatalogRowFfi] {
        (try? catalog?.browser(kind: kind)) ?? []
    }

    /// Reads the scan's progress (the app calls it a few times a second);
    /// a scan that finished refreshes the rows.
    public func poll() {
        guard let c = catalog, let p = try? c.progress() else { return }
        let finished = progress.running && !p.running
        let advanced = p.done != progress.done
        progress = p
        if finished || advanced { refresh() }
    }

    public func scan(all: Bool) {
        perform("Scan Failed") { try $0.scan(all: all) }
    }

    public func rescan(_ keys: [PluginKey]) {
        perform("Rescan Failed") { try $0.rescan(plugins: keys) }
    }

    public func setEnabled(_ key: PluginKey, _ on: Bool) {
        perform("Plugin Settings") { try $0.setEnabled(plugin: key, enabled: on) }
    }

    public func clearBlocklist(_ key: PluginKey) {
        perform("Rescan Failed") { try $0.clearBlocklist(plugin: key) }
    }

    /// The bundle to show in Finder (CLAP), if any.
    public func revealPath(_ key: PluginKey) -> String? {
        (try? catalog?.revealPath(plugin: key)) ?? nil
    }

    private func perform(_ title: String, _ body: (any PluginCatalogProtocol) throws -> Void) {
        guard let c = catalog else { return }
        do {
            try body(c)
        } catch {
            alert = AlertItem(error, title: title)
        }
        refresh()
        poll()
    }
}

extension PluginKey {
    /// The key of `plugin`.
    public init(_ plugin: PluginRefRow) {
        self.init(format: plugin.format, id: plugin.id)
    }
}
VIZDAW_EOF
```
Create `app/Sources/VizDAWAppModel/PluginBrowser.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/PluginBrowser.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// The plugin browser (spec §9): a popover from an insert slot ("+") or
/// from New Plugin Instrument…, listing usable plugins of one kind grouped
/// by vendor, filtered by a search.
@Observable
@MainActor
public final class PluginBrowserModel {
    public let kind: PluginKindTag
    public var search = ""
    private let rows: () -> [CatalogRowFfi]

    public init(kind: PluginKindTag, rows: @escaping () -> [CatalogRowFfi]) {
        self.kind = kind
        self.rows = rows
    }

    /// The groups the popover shows now.
    public var groups: [VendorGroup] { Self.groups(rows(), search: search) }

    /// One vendor's plugins.
    public struct VendorGroup: Identifiable, Equatable, Sendable {
        public let vendor: String
        public let plugins: [PluginRefRow]
        public var id: String { vendor }
    }

    /// `rows` grouped by vendor (vendors by name, "Other" last for none),
    /// plugins by name; a search matches name, vendor or id, any case.
    public nonisolated static func groups(_ rows: [CatalogRowFfi], search: String) -> [VendorGroup] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        let matching = rows.map(\.plugin).filter { p in
            q.isEmpty
                || p.name.lowercased().contains(q)
                || p.vendor.lowercased().contains(q)
                || p.id.lowercased().contains(q)
        }
        let byVendor = Dictionary(grouping: matching) { $0.vendor.isEmpty ? "" : $0.vendor }
        return byVendor.keys
            .sorted { a, b in
                if a.isEmpty != b.isEmpty { return !a.isEmpty }
                return a.localizedCaseInsensitiveCompare(b) == .orderedAscending
            }
            .map { v in
                VendorGroup(
                    vendor: v.isEmpty ? "Other" : v,
                    plugins: (byVendor[v] ?? []).sorted {
                        $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending
                    })
            }
    }
}
VIZDAW_EOF
```
Create `app/Sources/VizDAW/PluginBrowserView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAW
cat > app/Sources/VizDAW/PluginBrowserView.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The plugin browser popover (spec §9): search, plugins grouped by
/// vendor; choosing one calls `pick`.
struct PluginBrowserView: View {
    @Bindable var model: PluginBrowserModel
    let pick: (PluginRefRow) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextField("Search", text: $model.search)
                .textFieldStyle(.roundedBorder)
            let groups = model.groups
            if groups.isEmpty {
                Text(model.search.isEmpty ? "No plugins found. Scan in Window > Plugin Manager." : "No match.")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 80)
            } else {
                List {
                    ForEach(groups) { g in
                        Section(g.vendor) {
                            ForEach(g.plugins, id: \.self) { p in
                                Button {
                                    pick(p)
                                } label: {
                                    HStack {
                                        Text(p.name)
                                        Spacer()
                                        Text(p.format == .au ? "AU" : "CLAP")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
                .frame(minHeight: 240)
            }
        }
        .padding(10)
        .frame(width: 320)
    }
}
VIZDAW_EOF
```
Change `app/Sources/VizDAW/VizDAWApp.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/VizDAWApp.swift b/app/Sources/VizDAW/VizDAWApp.swift
index 2471c1b..ba558d9 100644
--- a/app/Sources/VizDAW/VizDAWApp.swift
+++ b/app/Sources/VizDAW/VizDAWApp.swift
@@ -1,10 +1,14 @@
 import AppKit
 import SwiftUI
 import VizDAWAppModel
+import VizDAWCore
 
 /// The one store of open documents (main actor).
 @MainActor let appStore = DocumentStore.live()
 
+/// The app's plugin catalog (main actor).
+@MainActor let appPlugins = AppPlugins(open: { try PluginCatalog.open(config: $0) })
+
 @main
 struct VizDAWApp: App {
     @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
@@ -29,6 +33,7 @@ final class AppDelegate: NSObject, NSApplicationDelegate {
         NSApp.setActivationPolicy(.regular)
         NSApp.activate()
         StoreAlerts.watch(appStore)
+        AppSetup.startPlugins()
         PluginTicker.start(appStore)
     }
 
@@ -39,17 +44,81 @@ final class AppDelegate: NSObject, NSApplicationDelegate {
     }
 }
 
+/// Where VizDAW keeps its files and finds its scanner (Plan Ruling 21).
+@MainActor
+enum AppSetup {
+    /// `VIZDAW_SUPPORT_DIR`, else `~/Library/Application Support/VizDAW`;
+    /// `--smoke` runs use a fresh temporary folder (never the user's).
+    static var supportDir: URL {
+        if LaunchArguments.smokeRun {
+            return FileManager.default.temporaryDirectory
+                .appendingPathComponent("vizdaw-smoke-\(ProcessInfo.processInfo.processIdentifier)")
+        }
+        if let d = ProcessInfo.processInfo.environment["VIZDAW_SUPPORT_DIR"] {
+            return URL(fileURLWithPath: d)
+        }
+        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
+        return base.appendingPathComponent("VizDAW")
+    }
+
+    /// `VIZDAW_SCANNER` (scripts/swift.sh sets it), else `vizdaw-scan` next
+    /// to the app's executable (the bundle).
+    static var scanner: String {
+        if let s = ProcessInfo.processInfo.environment["VIZDAW_SCANNER"] { return s }
+        let exe = Bundle.main.executableURL ?? URL(fileURLWithPath: CommandLine.arguments[0])
+        return exe.deletingLastPathComponent().appendingPathComponent("vizdaw-scan").path
+    }
+
+    /// Opens the catalog and scans new or changed plugins (not in `--smoke`
+    /// or `--no-scan` runs).
+    static func startPlugins() {
+        let dir = supportDir
+        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
+        try? setSupportDir(path: dir.path)
+        appPlugins.start(
+            config: AppPlugins.config(supportDir: dir, scanner: scanner),
+            scan: !LaunchArguments.smokeRun && !LaunchArguments.noScan)
+        PluginAlerts.watch(appPlugins)
+    }
+}
+
+/// Catalog problems are app-level: shown as modal alerts.
+@MainActor
+enum PluginAlerts {
+    static func watch(_ plugins: AppPlugins) {
+        withObservationTracking {
+            _ = plugins.alert
+        } onChange: {
+            DispatchQueue.main.async { watch(plugins) }
+        }
+        DispatchQueue.main.async {
+            guard let item = plugins.alert else { return }
+            plugins.alert = nil
+            let alert = NSAlert()
+            alert.alertStyle = .warning
+            alert.messageText = item.title
+            alert.informativeText = item.message
+            alert.addButton(withTitle: "OK")
+            alert.runModal()
+        }
+    }
+}
+
 /// The main-thread plugin timer (spec §5 `host_idle`, 60 Hz): every open
-/// document's plugins load, run their callbacks and report gestures here.
+/// document's plugins load, run their callbacks and report gestures here;
+/// a running scan's progress is read a few times a second.
 @MainActor
 enum PluginTicker {
     private static var timer: Timer?
+    private static var ticks = 0
 
     static func start(_ store: DocumentStore) {
         guard timer == nil else { return }
         let t = Timer(timeInterval: 1.0 / 60.0, repeats: true) { _ in
             MainActor.assumeIsolated {
                 for model in store.documents.values { model.idle() }
+                ticks += 1
+                if ticks % 15 == 0 { appPlugins.poll() }
             }
         }
         // Common modes: keeps ticking while a menu or a drag tracks.
@@ -63,11 +132,18 @@ enum PluginTicker {
 /// prints the transport and exits 0 if the position advanced, else 1.
 enum LaunchArguments {
     @MainActor static var smoke = false
+    /// This is a `--smoke` run (stays true after the run started).
+    @MainActor static var smokeRun = false
+    /// `--no-scan`: do not scan plugins at launch.
+    @MainActor static var noScan = false
 
     @MainActor static func apply(_ args: [String], to store: DocumentStore) {
         for arg in args.dropFirst() {
-            if arg == "--smoke" {
+            if arg == "--no-scan" {
+                noScan = true
+            } else if arg == "--smoke" {
                 smoke = true
+                smokeRun = true
                 store.enqueue(store.newDemo())
             } else if arg == "--demo" {
                 store.enqueue(store.newDemo())
VIZDAW_EOF
```
Change `scripts/build-ffi.sh`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/scripts/build-ffi.sh b/scripts/build-ffi.sh
index ee51b90..d6f7b47 100755
--- a/scripts/build-ffi.sh
+++ b/scripts/build-ffi.sh
@@ -16,7 +16,7 @@ if [[ "${1:-}" == "--debug" ]]; then
     flag=
 fi
 
-cargo build -p core-ffi $flag
+cargo build -p core-ffi -p vizdaw-scan $flag
 lib="target/$profile/libvizdaw_ffi"
 gen="target/ffi/gen"
 rm -rf "$gen"
@@ -24,6 +24,8 @@ mkdir -p "$gen" target/ffi
 cargo run -q -p uniffi-bindgen -- generate --library "$lib.dylib" \
     --language swift --out-dir "$gen"
 cp "$lib.a" target/ffi/libvizdaw_ffi.a
+# The plugin scanner the app runs (one process per plugin, spec §8).
+cp "target/$profile/vizdaw-scan" target/ffi/vizdaw-scan
 
 for f in vizdaw_ffi.swift vizdaw_ffiFFI.h vizdaw_ffiFFI.modulemap; do
     if [[ ! -f "$gen/$f" ]]; then
VIZDAW_EOF
```
Change `scripts/swift.sh`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/scripts/swift.sh b/scripts/swift.sh
index 7407d72..abed18b 100755
--- a/scripts/swift.sh
+++ b/scripts/swift.sh
@@ -9,6 +9,8 @@ fi
 root="$(cd "$(dirname "$0")/.." && pwd)"
 cmd="$1"
 shift
+# `swift run` has no app bundle: the scanner comes from build-ffi.sh.
+export VIZDAW_SCANNER="${VIZDAW_SCANNER:-$root/target/ffi/vizdaw-scan}"
 
 if xcodebuild -version >/dev/null 2>&1; then
     # Full Xcode (CI runners): default build system, XCTest/Testing in place.
VIZDAW_EOF
```
Change `scripts/make-app.sh`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/scripts/make-app.sh b/scripts/make-app.sh
index ef6bdbf..06667e8 100755
--- a/scripts/make-app.sh
+++ b/scripts/make-app.sh
@@ -12,6 +12,7 @@ app="dist/VizDAW.app"
 rm -rf "$app"
 mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
 cp "$bin/VizDAW" "$app/Contents/MacOS/VizDAW"
+cp target/ffi/vizdaw-scan "$app/Contents/MacOS/vizdaw-scan"
 cat > "$app/Contents/Info.plist" <<'PLIST'
 <?xml version="1.0" encoding="UTF-8"?>
 <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh
ls target/ffi/vizdaw-scan
scripts/swift.sh build
scripts/swift.sh test
```

Expected: `target/ffi/vizdaw-scan` exists; build complete; 43 tests in 7 suites pass (`PluginBrowserTests` 3, `AppPluginsTests` 4 new).

- [ ] **Step 5: Smoke run (offline audio, temporary support folder)**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --smoke 2>&1 | grep 'smoke:' | tail -2
```

Expected: the last line is `smoke: ok` (a window opens for about 4 s; nothing is audible).

- [ ] **Step 6: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every test passes.

- [ ] **Step 7: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/AppPlugins.swift app/Sources/VizDAWAppModel/PluginBrowser.swift \
  app/Sources/VizDAW/PluginBrowserView.swift app/Sources/VizDAW/VizDAWApp.swift \
  app/Tests/VizDAWAppModelTests/FakeCatalog.swift app/Tests/VizDAWAppModelTests/PluginBrowserTests.swift \
  scripts/build-ffi.sh scripts/swift.sh scripts/make-app.sh
git commit -m "$(cat <<'MSG'
feat(app): plugin catalog at launch and the plugin browser

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 11: The mixer window


**Files:**
- Create: `app/Sources/VizDAWAppModel/MixerModel.swift` (`MixerMath`, `MeterLevel`, `MixerModel`)
- Modify: `app/Sources/VizDAWAppModel/DocumentStore.swift` (`mixer(_:)`, mixers dropped with their document)
- Create: `app/Sources/VizDAW/MixerView.swift`
- Modify: `app/Sources/VizDAW/AppCommands.swift` (`documentID` focus value, Window > Mixer ⌥⌘M), `app/Sources/VizDAW/DocumentWindow.swift`, `app/Sources/VizDAW/VizDAWApp.swift` (the mixer scene; `--smoke` opens the mixer)
- Test: `app/Tests/VizDAWAppModelTests/MixerModelTests.swift` (new), `app/Tests/VizDAWAppModelTests/FakeSession.swift` (`mixerCalls`, row builders, `mixerFake()`)

**Interfaces:**
- Consumes: Task 5's mixer FFI (`MixerSnapshot`, `StripRow`, `SlotRow`, `SendRow`, `BusRow`, `DestRow`, `MixerLevels`, the session's mixer methods, `destinations()`, `setDestinationPlugin(dest:plugin:)`, `beginEdit`/`endEdit`), Task 9's `heldSlots()`/`loadPlugins(slots:)`, Task 10's `PluginBrowserView`, `appPlugins.browser(_:)`; `DocumentModel.apply(_:)` (internal), `slotsVersion`, `alert`.
- Produces: `MixerMath.{floorDb, maxDb, faderBottomDb, faderPosition(_:), faderDb(_:), dbLabel(_:), meterFill(_:), panLabel(_:), meterDecay}`; `MeterLevel(left:right:)`, `.silent`; `MixerModel(document:) throws` with `snapshot`, `meters`, `dspLoad`, `dspPeak`, `latencyFrames`, `faulted`, `held`, `renamingStrip`, `renameDraft`, `browser: BrowserTarget?` (`.insert(strip:)`, `.instrument`, `.destination(UInt64)`; `wantsInstrument`), `destinations: [DestRow]`, `fetches`, `strips`, `master`, `outputName(_:)`, `targets(for:)`, `sidechainSources(for:)`, `stripName(_:)`, `loadText`, `heldSlots(of:)`, `refreshIfNeeded()`, `pollLevels()`, intents `addBus()`, `removeBus(_:)`, `beginRename(_:)`, `commitRename()`, `cancelRename()`, `dragStarted(_:)`, `dragEnded()`, `setGain(_:db:)`, `setPan(_:_:)`, `toggleMute(_:)`, `toggleSolo(_:)`, `setOutput(_:bus:)`, `addSend(_:to:)`, `editSend(_:_:_:)`, `removeSend(_:_:)`, `pick(_:)`, `removeInsert(_:)`, `moveInsert(_:to:at:)`, `toggleBypass(_:)`, `setSidechain(_:source:)`, `addOutput(_:_:)`, `load(_:)`; `DocumentStore.mixer(_ id: UUID) -> MixerModel?`. In `VizDAW`: `MixerWindow`, `MixerView`, `StripView`, `SlotView` (`slotID(_:)` for drag and drop), `SendView`, `FaderView`, `MeterView`; `FocusedValues.documentID`.

Spec §11's functional mixer; Rulings 22, 25 and 26. Strips scroll horizontally with master at the right; each has its name (double-click to rename; context menu: Rename…, Remove Bus, Add Outputs), kind label, the instrument's slot, inserts (colored status dot, dimmed name and the reason as help text for placeholders, red for fault-bypassed, context menu Bypass / Sidechain (only with a sidechain input) / Remove / Load in safe mode, drag to reorder), "+" opening the effect browser, sends (target, Pre, On, level; drag is one step), output menu, pan, fader (−∞ … +12 dB, double-click the value for 0 dB), M/S and a meter with a falling peak. The toolbar has the DSP load and PDC, "Add Bus" and an Instruments menu: "New Plugin Instrument…" and, for every destination, "‹name›: ‹plugin or MIDI› → Plugin…" (both open the instrument browser; the second makes that destination play the plugin, so the lanes using it play it). The window's task refreshes and polls levels at 30 Hz.

- [ ] **Step 1: Write the failing tests**

Change `app/Tests/VizDAWAppModelTests/FakeSession.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Tests/VizDAWAppModelTests/FakeSession.swift b/app/Tests/VizDAWAppModelTests/FakeSession.swift
index d2aff9a..28fc229 100644
--- a/app/Tests/VizDAWAppModelTests/FakeSession.swift
+++ b/app/Tests/VizDAWAppModelTests/FakeSession.swift
@@ -116,7 +116,11 @@ final class FakeSession: SessionProtocol, @unchecked Sendable {
         return commandResult()
     }
 
-    func mixer() throws -> MixerSnapshot { mixerSnap }
+    var mixerCalls = 0
+    func mixer() throws -> MixerSnapshot {
+        mixerCalls += 1
+        return mixerSnap
+    }
     func levels() throws -> MixerLevels {
         levelsCalls += 1
         return levelsValue
@@ -262,3 +266,34 @@ func demoFake() -> FakeSession {
                    3: [lane(30, "Groove"), lane(31, "Chords")]]
     return f
 }
+
+func slotRow(_ id: UInt64, _ name: String, status: SlotStatus = .live, sidechain: Bool = false) -> SlotRow {
+    SlotRow(
+        id: id, plugin: PluginRefRow(format: .clap, id: "x.\(name)", name: name, vendor: "V", version: "1"),
+        instrument: false, bypass: false, sidechain: nil, status: status, latency: 0,
+        hasSidechainInput: sidechain, outputs: 1, hasEditor: false)
+}
+
+func stripRow(
+    _ id: UInt64, _ name: String, kind: StripKindRow = .bus, inserts: [SlotRow] = [],
+    sends: [SendRow] = [], output: UInt64? = nil
+) -> StripRow {
+    StripRow(
+        id: id, name: name, kind: kind, kindLabel: "\(kind)", instrument: nil, inserts: inserts,
+        gainDb: 0, pan: 0, mute: false, solo: false, soloMuted: false, sends: sends,
+        output: output, freeOutputs: [])
+}
+
+/// A mixer: Synth (built-in, one insert) and Bus 1, then Master.
+func mixerFake() -> FakeSession {
+    let f = demoFake()
+    f.mixerSnap = MixerSnapshot(
+        version: f.snap.version,
+        strips: [
+            stripRow(1, "Synth", kind: .builtinSynth, inserts: [slotRow(10, "Gain")]),
+            stripRow(2, "Bus 1"),
+            stripRow(0, "Master", kind: .master),
+        ],
+        buses: [BusRow(id: 2, name: "Bus 1")])
+    return f
+}
VIZDAW_EOF
```
Create `app/Tests/VizDAWAppModelTests/MixerModelTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/MixerModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct MixerMathTests {
    @Test func faderPositionsRoundTripAndFloorIsMinusInfinity() {
        #expect(MixerMath.faderPosition(-144) == 0)
        #expect(MixerMath.faderPosition(12) == 1)
        #expect(MixerMath.faderDb(0) == -144)
        for db in [-59.5, -24.5, -3.0, 0.0, 6.0, 12.0] {
            #expect(abs(MixerMath.faderDb(MixerMath.faderPosition(db)) - db) < 0.06, "\(db)")
        }
    }

    @Test func labels() {
        #expect(MixerMath.dbLabel(-144) == "−∞")
        #expect(MixerMath.dbLabel(3.5) == "+3.5")
        #expect(MixerMath.dbLabel(-12) == "-12.0")
        #expect(MixerMath.dbLabel(0) == "0.0")
        #expect(MixerMath.panLabel(0) == "C")
        #expect(MixerMath.panLabel(-0.5) == "L50")
        #expect(MixerMath.panLabel(0.25) == "R25")
        #expect(MixerMath.meterFill(0) == 0)
        #expect(MixerMath.meterFill(1) > 0.9 && MixerMath.meterFill(1) < 0.91)
        #expect(MixerMath.meterFill(.nan) == 0)
    }
}

@MainActor
@Suite struct MixerModelTests {
    func model(_ f: FakeSession) throws -> MixerModel {
        try MixerModel(document: DocumentModel(session: f))
    }

    @Test func masterIsSeparateAndOutputsAreNamed() throws {
        let f = mixerFake()
        f.dests = [DestRow(id: 7, name: "Piano", plugin: nil)]
        f.mixerSnap.strips[0].output = 2
        let m = try model(f)
        #expect(m.strips.map(\.name) == ["Synth", "Bus 1"])
        #expect(m.master?.name == "Master")
        #expect(m.outputName(m.strips[0]) == "Bus 1")
        #expect(m.outputName(m.strips[1]) == "Master")
        #expect(m.targets(for: m.strips[1]).isEmpty, "a bus does not send to itself")
        #expect(m.sidechainSources(for: m.strips[0]).map(\.name) == ["Bus 1"])
        #expect(m.destinations.map(\.name) == ["Piano"])
    }

    @Test func aFaderDragIsOneEditAndHeardOnEveryMove() throws {
        let f = mixerFake()
        let m = try model(f)
        m.dragStarted("Set Fader")
        m.setGain(1, db: -3)
        m.setGain(1, db: -6)
        m.dragEnded()
        #expect(f.calls == ["begin Set Fader", "setGain 1 -3.0", "setGain 1 -6.0", "end"])
    }

    @Test func theMixerIsRefetchedOnlyWhenTheDocumentOrSlotsChange() throws {
        let f = mixerFake()
        let doc = try DocumentModel(session: f)
        let m = try MixerModel(document: doc)
        let before = f.mixerCalls
        m.refreshIfNeeded()
        #expect(f.mixerCalls == before)
        doc.rename(1, to: "Kit")
        m.refreshIfNeeded()
        #expect(f.mixerCalls == before + 1)
        f.idleUpdates = [IdleUpdate(snapshot: nil, slotsChanged: true, paramsChanged: [], problems: [])]
        doc.idle()
        m.refreshIfNeeded()
        #expect(f.mixerCalls == before + 2)
    }

    @Test func metersHoldPeaksAndFall() throws {
        let f = mixerFake()
        let m = try model(f)
        f.levelsValue = MixerLevels(
            meters: [MeterRow(strip: 1, peakL: 1, peakR: 0.5)], dspLoad: 0.25, dspPeak: 0.5,
            latencyFrames: 480, faulted: [10])
        m.pollLevels()
        #expect(m.meters[1] == MeterLevel(left: 1, right: 0.5))
        #expect(m.faulted == [10])
        f.levelsValue.meters = [MeterRow(strip: 1, peakL: 0, peakR: 0.6)]
        m.pollLevels()
        #expect(m.meters[1]?.left == MixerMath.meterDecay)
        #expect(m.meters[1]?.right == 0.6)
        #expect(m.loadText == "DSP 25% · peak 50%", "no rate before Play")
    }

    @Test func busesGetFreeNamesAndEditsReachTheSession() throws {
        let f = mixerFake()
        let m = try model(f)
        m.addBus()
        m.toggleMute(m.strips[0])
        m.setOutput(1, bus: 2)
        // The session answers with the strip's new send.
        f.mixerSnap.strips[0].sends = [SendRow(target: 2, levelDb: 0, preFader: false, enabled: true)]
        m.addSend(1, to: 2)
        m.editSend(1, 0) { $0.preFader = true }
        m.toggleBypass(m.strips[0].inserts[0])
        m.setSidechain(10, source: 2)
        m.moveInsert(10, to: 2, at: 0)
        #expect(f.calls == [
            "addBus Bus 2", "mute 1 true", "output 1 2", "addSend 1 2",
            "editSend 1 0 2 0.0 true true", "bypass 10 true", "sidechain 10 2", "moveInsert 10 2 0",
        ])
    }

    @Test func theBrowserPickGoesWhereItWasOpened() throws {
        let f = mixerFake()
        let m = try model(f)
        let p = PluginRefRow(format: .clap, id: "u.diva", name: "Diva", vendor: "u-he", version: "1")
        m.browser = .insert(strip: 1)
        m.pick(p)
        m.browser = .instrument
        #expect(m.browser?.wantsInstrument == true)
        m.pick(p)
        m.browser = .destination(7)
        m.pick(p)
        #expect(m.browser == nil)
        #expect(f.calls == ["addInsert 1 end u.diva", "addInstrument Diva u.diva", "destPlugin 7 u.diva"])
        #expect(MixerModel.BrowserTarget.insert(strip: 1).wantsInstrument == false)
    }

    @Test func renamingAndErrors() throws {
        let f = mixerFake()
        let m = try model(f)
        m.beginRename(m.strips[1])
        #expect(m.renameDraft == "Bus 1")
        m.renameDraft = "  Verb "
        m.commitRename()
        m.beginRename(m.strips[1])
        m.commitRename()  // unchanged: no command
        #expect(f.calls == ["renameStrip 2 Verb"])
        f.mixerError = .Model(message: "routing cycle")
        m.setOutput(2, bus: 1)
        #expect(m.document.alert?.title == "Output Failed")
        #expect(m.document.alert?.message == "routing cycle")
    }

    @Test func safeModeLoadsAStripsHeldSlots() throws {
        let f = mixerFake()
        f.held = [10]
        let m = try model(f)
        #expect(m.heldSlots(of: m.strips[0]) == [10])
        m.load(m.heldSlots(of: m.strips[0]))
        #expect(f.loadedPlugins == [[10]])
        #expect(m.held.isEmpty)
    }
}

@MainActor
@Suite struct StoreMixerTests {
    @Test func oneMixerPerDocumentGoneWithIt() throws {
        let f = mixerFake()
        let store = DocumentStore(makeEmpty: { f }, makeDemo: { f }, open: { _ in f })
        let id = try #require(store.newEmpty())
        let m = try #require(store.mixer(id))
        #expect(store.mixer(id) === m)
        store.close(id)
        #expect(store.mixer(id) == nil)
        #expect(store.mixer(UUID()) == nil)
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh
scripts/swift.sh test
```

Expected: compile errors — cannot find `MixerModel`, `MixerMath`, `MeterLevel`; `DocumentStore` has no member `mixer`.

- [ ] **Step 3: Implement**

Create `app/Sources/VizDAWAppModel/MixerModel.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/MixerModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// Fader, meter and pan arithmetic for the mixer window.
public enum MixerMath {
    /// Fader floor: at or below it the fader is −∞ (3a Plan Ruling 4).
    public static let floorDb = -144.0
    public static let maxDb = 12.0
    /// The lowest dB a fader position other than the bottom shows.
    public static let faderBottomDb = -60.0

    /// Fader position 0…1 for `db`: −∞ at 0, then linear in dB from −60
    /// to +12.
    public static func faderPosition(_ db: Double) -> Double {
        guard db > faderBottomDb else { return 0 }
        return min(1, (db - faderBottomDb) / (maxDb - faderBottomDb))
    }

    /// The dB of fader position `p` (the inverse of `faderPosition`).
    public static func faderDb(_ p: Double) -> Double {
        guard p > 0.001 else { return floorDb }
        let db = faderBottomDb + min(1, p) * (maxDb - faderBottomDb)
        return (db * 10).rounded() / 10
    }

    /// "−∞", "0.0", "+3.5", "-12.0".
    public static func dbLabel(_ db: Double) -> String {
        if db <= floorDb { return "−∞" }
        let text = String(format: "%.1f", db)
        return db > 0 ? "+" + text : text
    }

    /// Meter fill 0…1 for a linear peak: −60 dBFS to +6 dBFS.
    public static func meterFill(_ peak: Float) -> Double {
        guard peak > 0, peak.isFinite else { return 0 }
        let db = 20 * log10(Double(peak))
        return max(0, min(1, (db + 60) / 66))
    }

    /// "C", "L50", "R25".
    public static func panLabel(_ pan: Double) -> String {
        let n = Int((abs(pan) * 100).rounded())
        if n == 0 { return "C" }
        return (pan < 0 ? "L" : "R") + "\(n)"
    }

    /// How much a held meter keeps per 30 Hz poll (it falls about 40 dB
    /// a second).
    public static let meterDecay: Float = 0.85
}

/// A meter's two channels as shown (peaks with a falling hold).
public struct MeterLevel: Equatable, Sendable {
    public var left: Float
    public var right: Float

    public init(left: Float, right: Float) {
        self.left = left
        self.right = right
    }

    /// No signal.
    public static let silent = MeterLevel(left: 0, right: 0)
}

/// One document's mixer window (spec §11): strips, slots, sends, routing,
/// meters at 30 Hz. Edits go through the session as undo steps; a drag
/// (fader, pan, send level) is one step.
@Observable
@MainActor
public final class MixerModel {
    public let document: DocumentModel
    public private(set) var snapshot: MixerSnapshot
    public private(set) var meters: [UInt64: MeterLevel] = [:]
    public private(set) var dspLoad: Float = 0
    public private(set) var dspPeak: Float = 0
    public private(set) var latencyFrames: UInt64 = 0
    /// Plugins the engine bypassed for faults (NaN output, errors).
    public private(set) var faulted: Set<UInt64> = []
    /// Slots safe mode keeps off.
    public private(set) var held: Set<UInt64> = []
    /// The instrument list (lanes play through these).
    public private(set) var destinations: [DestRow] = []
    /// The strip whose name is being edited.
    public private(set) var renamingStrip: UInt64?
    public var renameDraft = ""
    /// The open plugin browser.
    public var browser: BrowserTarget?
    /// Times the mixer was fetched (tests check refetch rules).
    public private(set) var fetches = 0

    /// Where a plugin picked in the browser goes.
    public enum BrowserTarget: Equatable, Sendable {
        /// The end of a strip's insert chain.
        case insert(strip: UInt64)
        /// A new plugin instrument.
        case instrument
        /// An existing destination, which then plays the plugin.
        case destination(UInt64)

        /// The browser lists instruments for it (effects for an insert).
        public var wantsInstrument: Bool {
            if case .insert = self { return false }
            return true
        }
    }

    private var seen: (version: UInt64, slots: Int)

    public init(document: DocumentModel) throws {
        self.document = document
        snapshot = try document.session.mixer()
        seen = (document.snapshot.version, document.slotsVersion)
        held = Set((try? document.session.heldSlots()) ?? [])
        destinations = (try? document.session.destinations()) ?? []
        fetches = 1
    }

    private var session: any SessionProtocol { document.session }

    // MARK: Derived

    /// Every strip but master, in display order.
    public var strips: [StripRow] { snapshot.strips.filter { $0.kind != .master } }
    public var master: StripRow? { snapshot.strips.first { $0.kind == .master } }

    /// "Master" or the bus's name.
    public func outputName(_ strip: StripRow) -> String {
        guard let b = strip.output else { return "Master" }
        return snapshot.buses.first { $0.id == b }?.name ?? "Bus \(b)"
    }

    /// Buses `strip` may send or output to (not itself).
    public func targets(for strip: StripRow) -> [BusRow] {
        snapshot.buses.filter { $0.id != strip.id }
    }

    /// Strips that may feed a sidechain of `strip`'s inserts (not itself,
    /// not master).
    public func sidechainSources(for strip: StripRow) -> [StripRow] {
        strips.filter { $0.id != strip.id }
    }

    public func stripName(_ id: UInt64) -> String {
        snapshot.strips.first { $0.id == id }?.name ?? "Strip \(id)"
    }

    /// "DSP 12% · peak 30% · PDC 10.7 ms".
    public var loadText: String {
        let rate = document.transport.sampleRate
        let pdc = rate > 0 ? String(format: " · PDC %.1f ms", Double(latencyFrames) / rate * 1000) : ""
        return String(format: "DSP %.0f%% · peak %.0f%%", dspLoad * 100, dspPeak * 100) + pdc
    }

    /// The held slots of `strip` (its "Load All").
    public func heldSlots(of strip: StripRow) -> [UInt64] {
        ([strip.instrument].compactMap { $0 } + strip.inserts).map(\.id).filter(held.contains)
    }

    // MARK: Polling

    /// Refetches the mixer when the document or its slots changed.
    public func refreshIfNeeded() {
        let now = (document.snapshot.version, document.slotsVersion)
        guard now != seen else { return }
        refetch()
    }

    /// Reads meters, load and faults (the window calls it at 30 Hz).
    public func pollLevels() {
        guard let l = try? session.levels() else { return }
        var next: [UInt64: MeterLevel] = [:]
        for m in l.meters {
            let old = meters[m.strip] ?? MeterLevel(left: 0, right: 0)
            next[m.strip] = MeterLevel(
                left: max(m.peakL, old.left * MixerMath.meterDecay),
                right: max(m.peakR, old.right * MixerMath.meterDecay))
        }
        meters = next
        dspLoad = l.dspLoad
        dspPeak = l.dspPeak
        latencyFrames = l.latencyFrames
        faulted = Set(l.faulted)
    }

    // MARK: Intents

    public func addBus() {
        let names = Set(snapshot.buses.map(\.name))
        let name = (1...).lazy.map { "Bus \($0)" }.first { !names.contains($0) } ?? "Bus"
        command("Add Bus Failed") { try $0.addBus(name: name) }
    }

    public func removeBus(_ strip: UInt64) {
        command("Remove Bus Failed") { try $0.removeBus(strip: strip) }
    }

    public func beginRename(_ strip: StripRow) {
        renameDraft = strip.name
        renamingStrip = strip.id
    }

    public func commitRename() {
        guard let id = renamingStrip else { return }
        renamingStrip = nil
        let name = renameDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, name != stripName(id) else { return }
        command("Rename Failed") { try $0.renameStrip(strip: id, name: name) }
    }

    public func cancelRename() { renamingStrip = nil }

    /// A drag starts: until `dragEnded`, edits are one undo step.
    public func dragStarted(_ label: String) {
        do {
            try session.beginEdit(label: label)
        } catch {
            document.alert = AlertItem(error, title: "Edit Failed")
        }
    }

    public func dragEnded() {
        command("Edit Failed") { try $0.endEdit() }
    }

    public func setGain(_ strip: UInt64, db: Double) {
        command("Fader Failed") { try $0.setGain(strip: strip, gainDb: db) }
    }

    public func setPan(_ strip: UInt64, _ pan: Double) {
        command("Pan Failed") { try $0.setPan(strip: strip, pan: pan) }
    }

    public func toggleMute(_ strip: StripRow) {
        command("Mute Failed") { try $0.setStripMute(strip: strip.id, mute: !strip.mute) }
    }

    public func toggleSolo(_ strip: StripRow) {
        command("Solo Failed") { try $0.setStripSolo(strip: strip.id, solo: !strip.solo) }
    }

    public func setOutput(_ strip: UInt64, bus: UInt64?) {
        command("Output Failed") { try $0.setOutput(strip: strip, bus: bus) }
    }

    public func addSend(_ strip: UInt64, to bus: UInt64) {
        command("Send Failed") { try $0.addSend(strip: strip, target: bus) }
    }

    public func editSend(_ strip: UInt64, _ index: Int, _ change: (inout SendRow) -> Void) {
        guard let s = snapshot.strips.first(where: { $0.id == strip }), index < s.sends.count else {
            return
        }
        var send = s.sends[index]
        change(&send)
        command("Send Failed") { try $0.editSend(strip: strip, index: UInt32(index), send: send) }
    }

    public func removeSend(_ strip: UInt64, _ index: Int) {
        command("Send Failed") { try $0.removeSend(strip: strip, index: UInt32(index)) }
    }

    /// Adds the plugin picked in the browser where `browser` says.
    public func pick(_ plugin: PluginRefRow) {
        let target = browser
        browser = nil
        switch target {
        case .insert(let strip):
            command("Add Plugin Failed") { try $0.addInsert(strip: strip, index: nil, plugin: plugin) }
        case .instrument:
            command("New Instrument Failed") { try $0.addPluginInstrument(name: plugin.name, plugin: plugin) }
        case .destination(let dest):
            command("Instrument Failed") { try $0.setDestinationPlugin(dest: dest, plugin: plugin) }
        case nil:
            break
        }
    }

    public func removeInsert(_ slot: UInt64) {
        command("Remove Plugin Failed") { try $0.removeInsert(slot: slot) }
    }

    /// Drops slot `slot` at `index` of `strip` (drag to reorder).
    public func moveInsert(_ slot: UInt64, to strip: UInt64, at index: Int) {
        command("Move Plugin Failed") {
            try $0.moveInsert(slot: slot, strip: strip, index: UInt32(max(0, index)))
        }
    }

    public func toggleBypass(_ slot: SlotRow) {
        command("Bypass Failed") { try $0.setBypass(slot: slot.id, bypass: !slot.bypass) }
    }

    public func setSidechain(_ slot: UInt64, source: UInt64?) {
        command("Sidechain Failed") { try $0.setSidechain(slot: slot, source: source) }
    }

    /// Adds a strip for output `index` of `strip`'s instrument.
    public func addOutput(_ strip: StripRow, _ index: UInt16) {
        guard case .instrument(let dest) = strip.kind else { return }
        let name = "\(strip.name) \(Int(index) + 1)"
        command("Add Output Failed") {
            try $0.addInstrumentOutput(dest: dest, index: index, name: name)
        }
    }

    /// Loads held slots (safe mode's Load and Load All).
    public func load(_ slots: [UInt64]) {
        do {
            try session.loadPlugins(slots: slots)
        } catch {
            document.alert = AlertItem(error, title: "Load Failed")
        }
        refetch()
    }

    // MARK: Plumbing

    private func command(_ title: String, _ body: (any SessionProtocol) throws -> DocSnapshot) {
        do {
            document.apply(try body(session))
        } catch {
            document.alert = AlertItem(error, title: title)
        }
        refetch()
    }

    private func refetch() {
        do {
            snapshot = try session.mixer()
            fetches += 1
            held = Set(try session.heldSlots())
            destinations = try session.destinations()
        } catch {
            if document.alert == nil {
                document.alert = AlertItem(error, title: "Mixer Unavailable")
            }
        }
        seen = (document.snapshot.version, document.slotsVersion)
    }
}
VIZDAW_EOF
```
Change `app/Sources/VizDAWAppModel/DocumentStore.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAWAppModel/DocumentStore.swift b/app/Sources/VizDAWAppModel/DocumentStore.swift
index b9cb521..f1ed9f8 100644
--- a/app/Sources/VizDAWAppModel/DocumentStore.swift
+++ b/app/Sources/VizDAWAppModel/DocumentStore.swift
@@ -10,6 +10,9 @@ public final class DocumentStore {
     public typealias Open = (String) throws -> any SessionProtocol
 
     public private(set) var documents: [UUID: DocumentModel] = [:]
+    /// Mixer windows' models, made on first use and dropped with their
+    /// document.
+    private var mixers: [UUID: MixerModel] = [:]
     /// Documents created without a window yet (Finder opens, launch
     /// arguments); the next window to appear adopts the first one.
     public private(set) var pending: [UUID] = []
@@ -37,6 +40,20 @@ public final class DocumentStore {
 
     public func model(_ id: UUID) -> DocumentModel? { documents[id] }
 
+    /// Document `id`'s mixer (one per document), or nil once it is closed.
+    public func mixer(_ id: UUID) -> MixerModel? {
+        if let m = mixers[id] { return m }
+        guard let doc = documents[id] else { return nil }
+        do {
+            let m = try MixerModel(document: doc)
+            mixers[id] = m
+            return m
+        } catch {
+            doc.alert = AlertItem(error, title: "Mixer Unavailable")
+            return nil
+        }
+    }
+
     public func newEmpty() -> UUID? { add("New Document Failed", makeEmpty) }
 
     public func newDemo() -> UUID? { add("Demo Project Failed", makeDemo) }
@@ -77,6 +94,7 @@ public final class DocumentStore {
     }
 
     public func close(_ id: UUID) {
+        mixers.removeValue(forKey: id)
         if let doc = documents.removeValue(forKey: id), doc.isPlaying {
             try? doc.session.stop()
         }
VIZDAW_EOF
```
Create `app/Sources/VizDAW/MixerView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAW
cat > app/Sources/VizDAW/MixerView.swift <<'VIZDAW_EOF'
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The mixer window of document `id` (spec §11; functional, not polished).
struct MixerWindow: View {
    @Binding var id: UUID?
    let store: DocumentStore

    var body: some View {
        if let id, let mixer = store.mixer(id) {
            MixerView(model: mixer)
                .focusedSceneValue(\.documentModel, mixer.document)
                .focusedSceneValue(\.documentID, id)
                .navigationTitle("Mixer — \(mixer.document.snapshot.title)")
        } else {
            ContentUnavailableView(
                "No Document", systemImage: "slider.vertical.3",
                description: Text("The document of this mixer is closed."))
        }
    }
}

struct MixerView: View {
    @Bindable var model: MixerModel

    var body: some View {
        ScrollView(.horizontal) {
            HStack(alignment: .top, spacing: 6) {
                ForEach(model.strips, id: \.id) { s in
                    StripView(model: model, strip: s)
                }
                if let master = model.master {
                    Divider()
                    StripView(model: model, strip: master)
                }
            }
            .padding(8)
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Text(model.loadText)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                Button("Add Bus") { model.addBus() }
                Menu("Instruments") {
                    Button("New Plugin Instrument…") { model.browser = .instrument }
                    Divider()
                    ForEach(model.destinations, id: \.id) { d in
                        Button("\(d.name): \(d.plugin?.name ?? "MIDI") → Plugin…") {
                            model.browser = .destination(d.id)
                        }
                    }
                }
                .popover(isPresented: instrumentBrowserShown) {
                    PluginBrowserView(model: browserModel(.instrument)) { model.pick($0) }
                }
            }
        }
        .task {
            // Meters at 30 Hz (spec §7); the strips follow the document.
            while !Task.isCancelled {
                model.refreshIfNeeded()
                model.pollLevels()
                try? await Task.sleep(for: .milliseconds(33))
            }
        }
    }

    /// The instrument browser: for a new instrument or a destination.
    private var instrumentBrowserShown: Binding<Bool> {
        Binding(
            get: { model.browser?.wantsInstrument == true },
            set: { if !$0 { model.browser = nil } })
    }

    private func browserModel(_ kind: PluginKindTag) -> PluginBrowserModel {
        PluginBrowserModel(kind: kind) { appPlugins.browser(kind) }
    }
}

/// One strip: name, kind, slots, sends, output, pan, fader, mute/solo,
/// meter.
struct StripView: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    @FocusState private var renaming: Bool

    var body: some View {
        VStack(spacing: 4) {
            header
            if let inst = strip.instrument {
                SlotView(model: model, strip: strip, slot: inst, index: nil)
            }
            inserts
            sends
            output
            panControl
            HStack(alignment: .bottom, spacing: 4) {
                FaderView(model: model, strip: strip)
                MeterView(level: model.meters[strip.id] ?? .silent)
            }
            HStack(spacing: 4) {
                Toggle("M", isOn: Binding(get: { strip.mute }, set: { _ in model.toggleMute(strip) }))
                    .toggleStyle(.button)
                    .tint(.orange)
                if strip.kind != .master {
                    Toggle("S", isOn: Binding(get: { strip.solo }, set: { _ in model.toggleSolo(strip) }))
                        .toggleStyle(.button)
                        .tint(.yellow)
                }
            }
            .help(strip.soloMuted ? "Silent: other strips are soloed" : "Mute / Solo")
        }
        .padding(6)
        .frame(width: 120)
        .background(RoundedRectangle(cornerRadius: 6).fill(.quaternary.opacity(strip.soloMuted ? 0.3 : 0.6)))
    }

    @ViewBuilder private var header: some View {
        if model.renamingStrip == strip.id {
            TextField("Name", text: $model.renameDraft)
                .focused($renaming)
                .onSubmit { model.commitRename() }
                .onExitCommand { model.cancelRename() }
                .onAppear { renaming = true }
        } else {
            Text(strip.name)
                .font(.headline)
                .lineLimit(1)
                .onTapGesture(count: 2) { model.beginRename(strip) }
                .contextMenu {
                    Button("Rename…") { model.beginRename(strip) }
                    if strip.kind == .bus {
                        Button("Remove Bus") { model.removeBus(strip.id) }
                    }
                    if !strip.freeOutputs.isEmpty {
                        Menu("Add Outputs") {
                            ForEach(strip.freeOutputs, id: \.self) { i in
                                Button("Output \(Int(i) + 1)") { model.addOutput(strip, i) }
                            }
                        }
                    }
                }
        }
        Text(strip.kindLabel)
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        let held = model.heldSlots(of: strip)
        if !held.isEmpty {
            Button("Load All") { model.load(held) }
                .controlSize(.small)
                .help("Safe mode kept these plugins off")
        }
    }

    private var inserts: some View {
        VStack(spacing: 2) {
            ForEach(Array(strip.inserts.enumerated()), id: \.element.id) { i, slot in
                SlotView(model: model, strip: strip, slot: slot, index: i)
            }
            Button {
                model.browser = .insert(strip: strip.id)
            } label: {
                Image(systemName: "plus")
                    .frame(maxWidth: .infinity)
            }
            .controlSize(.small)
            .help("Add an effect")
            .popover(
                isPresented: Binding(
                    get: { model.browser == .insert(strip: strip.id) },
                    set: { if !$0 { model.browser = nil } })
            ) {
                PluginBrowserView(model: PluginBrowserModel(kind: .effect) { appPlugins.browser(.effect) }) {
                    model.pick($0)
                }
            }
            .dropDestination(for: String.self) { items, _ in
                drop(items, at: strip.inserts.count)
            }
        }
    }

    private func drop(_ items: [String], at index: Int) -> Bool {
        guard let id = items.first.flatMap(SlotView.slotID) else { return false }
        model.moveInsert(id, to: strip.id, at: index)
        return true
    }

    @ViewBuilder private var sends: some View {
        if strip.kind != .master {
            ForEach(Array(strip.sends.enumerated()), id: \.offset) { i, send in
                SendView(model: model, strip: strip, index: i, send: send)
            }
            let targets = model.targets(for: strip)
            if !targets.isEmpty {
                Menu("Send") {
                    ForEach(targets, id: \.id) { b in
                        Button(b.name) { model.addSend(strip.id, to: b.id) }
                    }
                }
                .controlSize(.small)
            }
        }
    }

    @ViewBuilder private var output: some View {
        if strip.kind != .master {
            Menu("→ \(model.outputName(strip))") {
                Button("Master") { model.setOutput(strip.id, bus: nil) }
                ForEach(model.targets(for: strip), id: \.id) { b in
                    Button(b.name) { model.setOutput(strip.id, bus: b.id) }
                }
            }
            .controlSize(.small)
        }
    }

    @ViewBuilder private var panControl: some View {
        if strip.kind != .master {
            HStack(spacing: 2) {
                Slider(
                    value: Binding(get: { strip.pan }, set: { model.setPan(strip.id, $0) }),
                    in: -1...1
                ) { editing in
                    if editing { model.dragStarted("Set Pan") } else { model.dragEnded() }
                }
                .controlSize(.mini)
                Text(MixerMath.panLabel(strip.pan))
                    .font(.caption2.monospacedDigit())
                    .frame(width: 28)
            }
        }
    }
}

/// One plugin slot: name (dimmed with its reason when a placeholder),
/// actions in its menu, drag to reorder.
struct SlotView: View {
    @Bindable var model: MixerModel
    let strip: StripRow
    let slot: SlotRow
    /// Position among the inserts (`nil`: an instrument's own slot).
    let index: Int?

    static func slotID(_ text: String) -> UInt64? {
        text.hasPrefix("slot:") ? UInt64(text.dropFirst(5)) : nil
    }

    private var reason: String? {
        if case .placeholder(let r) = slot.status { return r }
        if model.faulted.contains(slot.id) { return "Bypassed: the plugin misbehaved" }
        return nil
    }

    var body: some View {
        HStack(spacing: 3) {
            Circle()
                .fill(color)
                .frame(width: 6, height: 6)
            Text(slot.plugin.name)
                .font(.caption)
                .lineLimit(1)
                .strikethrough(slot.bypass)
                .foregroundStyle(reason == nil ? .primary : .secondary)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 4)
        .frame(maxWidth: .infinity, minHeight: 18)
        .background(RoundedRectangle(cornerRadius: 3).fill(.background))
        .help(reason ?? "\(slot.plugin.name) (\(slot.plugin.vendor))")
        .contextMenu { menu }
        .modifier(Reorder(model: model, strip: strip, slot: slot, index: index))
    }

    private var color: Color {
        if model.faulted.contains(slot.id) { return .red }
        switch slot.status {
        case .live: return slot.bypass ? .gray : .green
        case .loading: return .yellow
        case .placeholder: return .orange
        }
    }

    @ViewBuilder private var menu: some View {
        if model.held.contains(slot.id) {
            Button("Load") { model.load([slot.id]) }
        }
        Button(slot.bypass ? "Turn On" : "Bypass") { model.toggleBypass(slot) }
        if index != nil {
            if slot.hasSidechainInput {
                Menu("Sidechain") {
                    Button("None") { model.setSidechain(slot.id, source: nil) }
                    ForEach(model.sidechainSources(for: strip), id: \.id) { s in
                        Button((slot.sidechain == s.id ? "✓ " : "") + s.name) {
                            model.setSidechain(slot.id, source: s.id)
                        }
                    }
                }
            }
            Button("Remove") { model.removeInsert(slot.id) }
        }
    }

    /// Inserts drag and drop (an instrument's own slot does not move).
    private struct Reorder: ViewModifier {
        let model: MixerModel
        let strip: StripRow
        let slot: SlotRow
        let index: Int?

        func body(content: Content) -> some View {
            if let index {
                content
                    .draggable("slot:\(slot.id)")
                    .dropDestination(for: String.self) { items, _ in
                        guard let id = items.first.flatMap(SlotView.slotID) else { return false }
                        model.moveInsert(id, to: strip.id, at: index)
                        return true
                    }
            } else {
                content
            }
        }
    }
}

/// One send: target, level (a drag is one undo step), pre/post, on/off.
struct SendView: View {
    let model: MixerModel
    let strip: StripRow
    let index: Int
    let send: SendRow

    var body: some View {
        VStack(spacing: 1) {
            HStack(spacing: 2) {
                Text(model.stripName(send.target))
                    .font(.caption2)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Toggle("Pre", isOn: Binding(get: { send.preFader }, set: { v in model.editSend(strip.id, index) { $0.preFader = v } }))
                    .toggleStyle(.button)
                    .controlSize(.mini)
                Toggle("On", isOn: Binding(get: { send.enabled }, set: { v in model.editSend(strip.id, index) { $0.enabled = v } }))
                    .toggleStyle(.button)
                    .controlSize(.mini)
            }
            Slider(
                value: Binding(
                    get: { MixerMath.faderPosition(send.levelDb) },
                    set: { p in model.editSend(strip.id, index) { $0.levelDb = MixerMath.faderDb(p) } }),
                in: 0...1
            ) { editing in
                if editing { model.dragStarted("Set Send Level") } else { model.dragEnded() }
            }
            .controlSize(.mini)
        }
        .contextMenu { Button("Remove Send") { model.removeSend(strip.id, index) } }
        .help("Send to \(model.stripName(send.target)): \(MixerMath.dbLabel(send.levelDb)) dB")
    }
}

/// A vertical fader (a horizontal slider turned); a drag is one undo step.
struct FaderView: View {
    let model: MixerModel
    let strip: StripRow

    var body: some View {
        VStack(spacing: 2) {
            Slider(
                value: Binding(
                    get: { MixerMath.faderPosition(strip.gainDb) },
                    set: { model.setGain(strip.id, db: MixerMath.faderDb($0)) }),
                in: 0...1
            ) { editing in
                if editing { model.dragStarted("Set Fader") } else { model.dragEnded() }
            }
            .frame(width: 140)
            .rotationEffect(.degrees(-90))
            .frame(width: 30, height: 140)
            Text(MixerMath.dbLabel(strip.gainDb))
                .font(.caption2.monospacedDigit())
                .onTapGesture(count: 2) { model.setGain(strip.id, db: 0) }
                .help("Double-click: 0 dB")
        }
    }
}

/// Peak meter, left and right.
struct MeterView: View {
    let level: MeterLevel

    var body: some View {
        HStack(spacing: 2) {
            bar(level.left)
            bar(level.right)
        }
        .frame(width: 14, height: 140)
    }

    private func bar(_ peak: Float) -> some View {
        GeometryReader { g in
            let fill = MixerMath.meterFill(peak)
            ZStack(alignment: .bottom) {
                Rectangle().fill(.black.opacity(0.6))
                Rectangle()
                    .fill(peak >= 1 ? .red : .green)
                    .frame(height: g.size.height * fill)
            }
        }
    }
}
VIZDAW_EOF
```
Change `app/Sources/VizDAW/AppCommands.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/AppCommands.swift b/app/Sources/VizDAW/AppCommands.swift
index 640d662..12ae9d4 100644
--- a/app/Sources/VizDAW/AppCommands.swift
+++ b/app/Sources/VizDAW/AppCommands.swift
@@ -7,17 +7,28 @@ struct DocumentModelKey: FocusedValueKey {
     typealias Value = DocumentModel
 }
 
+struct DocumentIDKey: FocusedValueKey {
+    typealias Value = UUID
+}
+
 extension FocusedValues {
     var documentModel: DocumentModel? {
         get { self[DocumentModelKey.self] }
         set { self[DocumentModelKey.self] = newValue }
     }
+
+    /// The store's key of the focused window's document.
+    var documentID: UUID? {
+        get { self[DocumentIDKey.self] }
+        set { self[DocumentIDKey.self] = newValue }
+    }
 }
 
 /// File, Edit and Transport menus.
 struct AppCommands: Commands {
     let store: DocumentStore
     @FocusedValue(\.documentModel) private var doc
+    @FocusedValue(\.documentID) private var docID
     @Environment(\.openWindow) private var openWindow
 
     var body: some Commands {
@@ -46,6 +57,12 @@ struct AppCommands: Commands {
                 .keyboardShortcut("z", modifiers: [.command, .shift])
                 .disabled(!(doc?.canRedo ?? false))
         }
+        CommandGroup(before: .windowList) {
+            Button("Mixer") { if let docID { openWindow(id: "mixer", value: docID) } }
+                .keyboardShortcut("m", modifiers: [.command, .option])
+                .disabled(docID == nil)
+            Divider()
+        }
         CommandMenu("Transport") {
             Button(doc?.isPlaying == true ? "Stop" : "Play") { doc?.playStop() }
                 .keyboardShortcut(.space, modifiers: [])
VIZDAW_EOF
```
Change `app/Sources/VizDAW/DocumentWindow.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/DocumentWindow.swift b/app/Sources/VizDAW/DocumentWindow.swift
index 561c8b5..f8de0b1 100644
--- a/app/Sources/VizDAW/DocumentWindow.swift
+++ b/app/Sources/VizDAW/DocumentWindow.swift
@@ -15,6 +15,7 @@ struct DocumentWindow: View {
             if let id, let model = store.model(id) {
                 DocumentView(model: model)
                     .focusedSceneValue(\.documentModel, model)
+                    .focusedSceneValue(\.documentID, id)
                     .task(id: PollKey(key: activeState == .key, playing: model.isPlaying)) {
                         // Background playback keeps polling so device
                         // events are handled (O27).
@@ -24,7 +25,9 @@ struct DocumentWindow: View {
                             try? await Task.sleep(for: model.pollInterval)
                         }
                     }
-                    .onAppear { SmokeRun.start(model) }
+                    .onAppear {
+                        SmokeRun.start(model) { openWindow(id: "mixer", value: id) }
+                    }
                     .onDisappear { store.close(id) }
             } else {
                 ProgressView()
VIZDAW_EOF
```
Change `app/Sources/VizDAW/VizDAWApp.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/VizDAWApp.swift b/app/Sources/VizDAW/VizDAWApp.swift
index ba558d9..6d22030 100644
--- a/app/Sources/VizDAW/VizDAWApp.swift
+++ b/app/Sources/VizDAW/VizDAWApp.swift
@@ -23,6 +23,10 @@ struct VizDAWApp: App {
         }
         .defaultSize(width: 980, height: 620)
         .commands { AppCommands(store: appStore) }
+        WindowGroup("Mixer", id: "mixer", for: UUID.self) { $id in
+            MixerWindow(id: $id, store: appStore)
+        }
+        .defaultSize(width: 760, height: 560)
     }
 }
 
@@ -154,14 +158,16 @@ enum LaunchArguments {
     }
 }
 
-/// The `--smoke` run: Play, poll for 3 s through the window's model, exit.
+/// The `--smoke` run: Play, open the mixer, poll for 3 s through the
+/// window's model, exit.
 enum SmokeRun {
-    @MainActor static func start(_ model: DocumentModel) {
+    @MainActor static func start(_ model: DocumentModel, openMixer: @escaping () -> Void) {
         guard LaunchArguments.smoke else { return }
         LaunchArguments.smoke = false
         Task { @MainActor in
             try? await Task.sleep(for: .milliseconds(300))
             model.playStop()
+            openMixer()
             var ticks: [Int64] = []
             for _ in 0..<12 {
                 try? await Task.sleep(for: .milliseconds(250))
@@ -170,7 +176,13 @@ enum SmokeRun {
                 FileHandle.standardError.write(Data(
                     "smoke: \(model.selectedName ?? "-") \(Formatting.position(model.transport)) playing=\(model.isPlaying) device=\(Formatting.device(model.transport))\n".utf8))
             }
+            // The mixer window polls its levels: every strip has a meter.
+            let mixer = appStore.documents.first { $0.value === model }.flatMap { appStore.mixer($0.key) }
+            let strips = mixer?.snapshot.strips.count ?? 0
+            let meters = mixer?.meters.count ?? 0
+            FileHandle.standardError.write(Data("smoke: mixer strips \(strips) meters \(meters)\n".utf8))
             let ok = model.alert == nil && (ticks.last ?? 0) > (ticks.first ?? 0)
+                && strips >= 2 && meters == strips
             if let a = model.alert {
                 FileHandle.standardError.write(Data("smoke: alert \(a.title): \(a.message)\n".utf8))
             }
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests and the smoke run**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build
scripts/swift.sh test
VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --smoke 2>&1 | grep 'smoke:' | tail -2
```

Expected: 54 tests in 10 suites pass (`MixerMathTests` 2, `MixerModelTests` 8, `StoreMixerTests` 1 new); the smoke run prints `smoke: mixer strips 2 meters 2` then `smoke: ok`.

- [ ] **Step 5: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/MixerModel.swift app/Sources/VizDAWAppModel/DocumentStore.swift \
  app/Sources/VizDAW/MixerView.swift app/Sources/VizDAW/AppCommands.swift \
  app/Sources/VizDAW/DocumentWindow.swift app/Sources/VizDAW/VizDAWApp.swift \
  app/Tests/VizDAWAppModelTests/FakeSession.swift app/Tests/VizDAWAppModelTests/MixerModelTests.swift
git commit -m "$(cat <<'MSG'
feat(app): the mixer window

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 12: Plugin windows


**Files:**
- Create: `app/Sources/VizDAWAppModel/PluginWindowModel.swift`
- Create: `app/Sources/VizDAW/PluginWindows.swift` (`PluginWindows`, `PluginWindowController`, `PluginWindowContent`, `EditorHost`, `ParameterList`, the `openPluginWindow` environment action)
- Modify: `app/Sources/VizDAW/MixerView.swift` (Open, double-click a slot), `app/Sources/VizDAW/DocumentWindow.swift` (reopen saved windows), `app/Sources/VizDAW/VizDAWApp.swift` (windows follow their slots on every tick; `--smoke` opens AUDelay's window)
- Test: `app/Tests/VizDAWAppModelTests/PluginWindowModelTests.swift` (new)

**Interfaces:**
- Consumes: Task 8's `openEditor(slot:parent:)`, `resizeEditor`, `closeEditor`, `setWindowOpen`, `openWindows`, `EditorSize`; Task 6's `params(slot:)`, `setParam`, `commitParams`; Task 5's `mixer()`, `setBypass`; Task 11's `MixerModel`, `SlotView`; Task 4's `DocumentModel.{paramsChanged, slotsVersion}`.
- Produces: `PluginWindowModel(document:slot:)` with `row`, `stripName`, `params`, `editorSize`, `editorProblem`, `title`, `isGone`, `isLive`, `placeholderReason`, `visibleParams`, `static valueText(_:)`, `refresh()`, `openEditor(parent:) -> Bool`, `resize(width:height:)`, `opened()`, `closed()`, `toggleBypass()`, `setParam(_:_:)`, `commitParam(_:)`. In `VizDAW`: `PluginWindows.{show(_:doc:slot:), reopen(_:doc:), tick(_:), editorSize(doc:slot:)}`, `PluginWindowController` (`Seen`, `update(_:)`).

Rulings 13 and 20. One window per instance, titled "‹strip› — ‹plugin›", with a Bypass toggle and the placeholder reason in its header. The editor goes into `editorHost`, an `NSView` the controller owns and passes to Rust as an address; `windowWillClose` calls `close_editor` before the view can go (the FFI contract). A plugin without an editor shows its parameters (a slider drag commits one undo step on release). Windows follow their slot every timer tick: the title, the editor once the plugin runs, the parameters when they changed, and the window closes when its slot left the document (removed, or the undo of its add). Saved windows reopen with their project.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/PluginWindowModelTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/PluginWindowModelTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct PluginWindowModelTests {
    func param(_ id: UInt64, _ name: String, value: Double = 0.5, stepped: Bool = false, hidden: Bool = false) -> ParamRow {
        ParamRow(
            id: id, name: name, module: "", min: 0, max: 1, default: 0.5, value: value,
            stepped: stepped, automatable: true, hidden: hidden)
    }

    @Test func titleNamesStripAndPluginAndHiddenParamsAreLeftOut() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain"), param(1, "Secret", hidden: true), param(2, "Mode", value: 2.6, stepped: true)]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(w.title == "Synth — Gain")
        #expect(w.visibleParams.map(\.name) == ["Gain", "Mode"])
        #expect(PluginWindowModel.valueText(w.visibleParams[1]) == "3")
        #expect(PluginWindowModel.valueText(w.visibleParams[0]) == "0.50")
        #expect(!w.isGone && w.isLive)
    }

    @Test func aParameterDragIsHeardAndCommittedOnce() throws {
        let f = mixerFake()
        f.paramTable[10] = [param(0, "Gain")]
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.setParam(0, 0.7)
        w.setParam(0, 0.2)
        w.commitParam(0)
        #expect(f.liveSets.map(\.2) == [0.7, 0.2])
        #expect(f.calls == ["commitParams 10 0=0.2"])
    }

    @Test func theEditorOpensWhenThePluginHasOneElseParametersShow() throws {
        let f = mixerFake()
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(!w.openEditor(parent: 0x1000))
        #expect(w.editorSize == nil)
        #expect(w.editorProblem?.contains("editor unavailable") == true)
        f.editors[10] = EditorSize(width: 400, height: 300, resizable: false)
        #expect(w.openEditor(parent: 0x1000))
        #expect(w.editorSize?.width == 400)
        w.resize(width: 500, height: 500)
        #expect(w.editorSize?.width == 400, "a fixed-size editor keeps its size")
        w.opened()
        #expect(f.windowsOpen == [10])
        w.closed()
        #expect(f.closedEditors == [10])
        #expect(f.windowsOpen.isEmpty)
    }

    @Test func aPlaceholderShowsItsReasonAndARemovedSlotIsGone() throws {
        let f = mixerFake()
        f.mixerSnap.strips[0].inserts[0].status = .placeholder(reason: "Disabled in Plugin Manager")
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        #expect(!w.openEditor(parent: 0x1000))
        #expect(w.editorProblem == "Disabled in Plugin Manager")
        f.mixerSnap.strips[0].inserts = []
        w.refresh()
        #expect(w.isGone)
    }

    @Test func bypassFromTheHeader() throws {
        let f = mixerFake()
        let w = PluginWindowModel(document: try DocumentModel(session: f), slot: 10)
        w.toggleBypass()
        #expect(f.calls == ["bypass 10 true"])
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh
scripts/swift.sh test
```

Expected: compile errors — cannot find `PluginWindowModel` in scope.

- [ ] **Step 3: Implement**

Create `app/Sources/VizDAWAppModel/PluginWindowModel.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/PluginWindowModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// One plugin window (spec §9): titled "‹strip› — ‹plugin›", the plugin's
/// own editor (or, without one, its parameters), a bypass switch in the
/// header. Closing it never removes the plugin.
@Observable
@MainActor
public final class PluginWindowModel {
    public let document: DocumentModel
    public let slot: UInt64
    public private(set) var row: SlotRow?
    public private(set) var stripName = ""
    public private(set) var params: [ParamRow] = []
    /// The open editor's size; nil while the window shows parameters.
    public private(set) var editorSize: EditorSize?
    /// Why the editor is not shown (no editor, not loaded yet).
    public private(set) var editorProblem: String?

    public init(document: DocumentModel, slot: UInt64) {
        self.document = document
        self.slot = slot
        refresh()
    }

    private var session: any SessionProtocol { document.session }

    // MARK: Derived

    public var title: String {
        let plugin = row?.plugin.name ?? "Plugin"
        return stripName.isEmpty ? plugin : "\(stripName) — \(plugin)"
    }

    /// The slot left the document (removed, or an undo of its add): the
    /// window closes.
    public var isGone: Bool { row == nil }

    public var isLive: Bool { row?.status == .live }

    /// The placeholder reason, if the plugin is not running.
    public var placeholderReason: String? {
        if case .placeholder(let r) = row?.status { return r }
        return nil
    }

    /// Parameters a user sees (not hidden).
    public var visibleParams: [ParamRow] { params.filter { !$0.hidden } }

    /// "0.50", or a whole number for stepped parameters.
    public static func valueText(_ p: ParamRow) -> String {
        p.stepped ? String(Int(p.value.rounded())) : String(format: "%.2f", p.value)
    }

    // MARK: Updates

    /// Rereads the slot, its strip and its parameters.
    public func refresh() {
        let mixer = try? session.mixer()
        row = nil
        stripName = ""
        for s in mixer?.strips ?? [] {
            if let r = ([s.instrument].compactMap { $0 } + s.inserts).first(where: { $0.id == slot }) {
                row = r
                stripName = s.name
            }
        }
        params = (try? session.params(slot: slot)) ?? []
    }

    /// Opens the plugin's editor inside the view at address `parent`;
    /// false (and the reason kept) when it has none or is not running.
    @discardableResult
    public func openEditor(parent: UInt64) -> Bool {
        guard isLive else {
            editorProblem = placeholderReason ?? "Loading…"
            return false
        }
        do {
            editorSize = try session.openEditor(slot: slot, parent: parent)
            editorProblem = nil
            return true
        } catch {
            editorSize = nil
            editorProblem = AlertItem(error, title: "").message
            return false
        }
    }

    /// The window was resized by the user: the editor follows (or keeps
    /// its own size).
    public func resize(width: UInt32, height: UInt32) {
        guard editorSize?.resizable == true else { return }
        if let s = try? session.resizeEditor(slot: slot, width: width, height: height) {
            editorSize = s
        }
    }

    public func opened() {
        try? session.setWindowOpen(slot: slot, open: true)
    }

    /// The window closes: the editor detaches (ending any gesture it left
    /// open) and the project no longer reopens it.
    public func closed() {
        try? session.closeEditor(slot: slot)
        try? session.setWindowOpen(slot: slot, open: false)
        editorSize = nil
    }

    // MARK: Intents

    public func toggleBypass() {
        guard let r = row else { return }
        do {
            document.apply(try session.setBypass(slot: slot, bypass: !r.bypass))
        } catch {
            document.alert = AlertItem(error, title: "Bypass Failed")
        }
        refresh()
    }

    /// A parameter control moves: heard now, no undo step yet.
    public func setParam(_ id: UInt64, _ value: Double) {
        try? session.setParam(slot: slot, param: id, value: value)
        if let i = params.firstIndex(where: { $0.id == id }) { params[i].value = value }
    }

    /// The control is let go: one undo step with its final value.
    public func commitParam(_ id: UInt64) {
        guard let p = params.first(where: { $0.id == id }) else { return }
        do {
            document.apply(try session.commitParams(slot: slot, values: [ParamValue(id: id, value: p.value)]))
        } catch {
            document.alert = AlertItem(error, title: "Parameter Failed")
        }
    }
}
VIZDAW_EOF
```
Create `app/Sources/VizDAW/PluginWindows.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAW
cat > app/Sources/VizDAW/PluginWindows.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// Opens slot windows from the mixer (an environment action).
struct OpenPluginWindowKey: EnvironmentKey {
    static let defaultValue: @MainActor (UInt64) -> Void = { _ in }
}

extension EnvironmentValues {
    var openPluginWindow: @MainActor (UInt64) -> Void {
        get { self[OpenPluginWindowKey.self] }
        set { self[OpenPluginWindowKey.self] = newValue }
    }
}

/// Every open plugin window, by document and slot (spec §9: one window
/// per instance).
@MainActor
enum PluginWindows {
    private struct Key: Hashable {
        let doc: UUID
        let slot: UInt64
    }

    private static var open: [Key: PluginWindowController] = [:]

    /// Shows slot `slot`'s window of document `doc` (brings it to the
    /// front when open).
    static func show(_ store: DocumentStore, doc: UUID, slot: UInt64) {
        let key = Key(doc: doc, slot: slot)
        if let w = open[key] {
            w.showWindow(nil)
            return
        }
        guard let model = store.model(doc) else { return }
        let window = PluginWindowModel(document: model, slot: slot)
        // A slot that is gone (a stale saved window) gets no window.
        guard !window.isGone else { return }
        let c = PluginWindowController(model: window) {
            open[key] = nil
        }
        open[key] = c
        c.showWindow(nil)
    }

    /// The editor size an open window shows (the `--smoke` run).
    static func editorSize(doc: UUID, slot: UInt64) -> EditorSize? {
        open[Key(doc: doc, slot: slot)]?.model.editorSize
    }

    /// Reopens the windows that were open when the project was saved.
    static func reopen(_ store: DocumentStore, doc: UUID) {
        guard let model = store.model(doc), let slots = try? model.session.openWindows() else { return }
        for s in slots { show(store, doc: doc, slot: s) }
    }

    /// The timer's step: windows follow their slot (close when it is gone,
    /// open the editor once it runs, reread changed parameters).
    static func tick(_ store: DocumentStore) {
        for (key, c) in open {
            guard let doc = store.model(key.doc) else {
                c.close()
                continue
            }
            let now = PluginWindowController.Seen(slots: doc.slotsVersion, doc: doc.snapshot.version)
            if doc.paramsChanged.contains(key.slot) || c.seen != now {
                c.update(now)
            }
        }
        for doc in store.documents.values { doc.paramsChanged.removeAll() }
    }
}

/// One plugin window: header (bypass, status), then the plugin's editor in
/// a plain `NSView` handed to Rust, or its parameters.
final class PluginWindowController: NSWindowController, NSWindowDelegate {
    let model: PluginWindowModel
    /// The view the plugin's editor goes into: alive until `close_editor`
    /// ran (in `windowWillClose`), as the FFI requires.
    let editorHost = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 300))
    private let onClose: @MainActor () -> Void

    /// What the window last followed: an edit (an undo of the add removes
    /// the slot) or a slot change (loaded, failed) makes it look again.
    struct Seen: Equatable {
        let slots: Int
        let doc: UInt64
    }

    private(set) var seen: Seen?

    init(model: PluginWindowModel, onClose: @escaping @MainActor () -> Void) {
        self.model = model
        self.onClose = onClose
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 420, height: 360),
            styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        super.init(window: window)
        window.delegate = self
        window.contentView = NSHostingView(
            rootView: PluginWindowContent(model: model, editorHost: editorHost))
        window.center()
        model.opened()
        update(Seen(slots: model.document.slotsVersion, doc: model.document.snapshot.version))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    /// Follows the slot: title, closing when gone, the editor once live.
    func update(_ now: Seen) {
        seen = now
        model.refresh()
        guard let window else { return }
        if model.isGone {
            close()
            return
        }
        window.title = model.title
        if model.editorSize == nil && model.isLive {
            let address = UInt64(UInt(bitPattern: Unmanaged.passUnretained(editorHost).toOpaque()))
            if model.openEditor(parent: address), let size = model.editorSize {
                editorHost.setFrameSize(NSSize(width: Int(size.width), height: Int(size.height)))
                if size.resizable { window.styleMask.insert(.resizable) }
            }
        }
        if let content = window.contentView {
            window.setContentSize(content.fittingSize)
        }
    }

    func windowDidResize(_ notification: Notification) {
        guard model.editorSize?.resizable == true else { return }
        let s = editorHost.frame.size
        model.resize(width: UInt32(max(1, s.width)), height: UInt32(max(1, s.height)))
    }

    func windowWillClose(_ notification: Notification) {
        model.closed()
        onClose()
    }
}

/// The window's SwiftUI content.
struct PluginWindowContent: View {
    let model: PluginWindowModel
    let editorHost: NSView

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Toggle(
                    "Bypass",
                    isOn: Binding(get: { model.row?.bypass ?? false }, set: { _ in model.toggleBypass() })
                )
                .toggleStyle(.button)
                if let r = model.placeholderReason {
                    Text(r).foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding(6)
            Divider()
            if let size = model.editorSize {
                EditorHost(view: editorHost)
                    .frame(width: CGFloat(size.width), height: CGFloat(size.height))
            } else {
                ParameterList(model: model)
            }
        }
    }
}

/// The plain `NSView` the plugin's editor lives in.
struct EditorHost: NSViewRepresentable {
    let view: NSView
    func makeNSView(context: Context) -> NSView { view }
    func updateNSView(_ nsView: NSView, context: Context) {}
}

/// A plugin without an editor (or not running): its parameters as
/// sliders; a drag is one undo step.
struct ParameterList: View {
    let model: PluginWindowModel

    var body: some View {
        if model.visibleParams.isEmpty {
            Text(model.editorProblem ?? "No parameters.")
                .foregroundStyle(.secondary)
                .frame(width: 360, height: 120)
        } else {
            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(model.visibleParams, id: \.id) { p in
                        HStack {
                            Text(p.name).frame(width: 110, alignment: .leading).lineLimit(1)
                            Slider(
                                value: Binding(get: { p.value }, set: { model.setParam(p.id, $0) }),
                                in: p.min...max(p.max, p.min + 1e-9)
                            ) { editing in
                                if !editing { model.commitParam(p.id) }
                            }
                            Text(PluginWindowModel.valueText(p))
                                .font(.caption.monospacedDigit())
                                .frame(width: 48, alignment: .trailing)
                        }
                    }
                }
                .padding(10)
            }
            .frame(width: 400, height: min(420, CGFloat(model.visibleParams.count) * 30 + 30))
        }
    }
}
VIZDAW_EOF
```
Change `app/Sources/VizDAW/MixerView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/MixerView.swift b/app/Sources/VizDAW/MixerView.swift
index 158a879..87b4bff 100644
--- a/app/Sources/VizDAW/MixerView.swift
+++ b/app/Sources/VizDAW/MixerView.swift
@@ -10,6 +10,7 @@ struct MixerWindow: View {
     var body: some View {
         if let id, let mixer = store.mixer(id) {
             MixerView(model: mixer)
+                .environment(\.openPluginWindow, { slot in PluginWindows.show(store, doc: id, slot: slot) })
                 .focusedSceneValue(\.documentModel, mixer.document)
                 .focusedSceneValue(\.documentID, id)
                 .navigationTitle("Mixer — \(mixer.document.snapshot.title)")
@@ -244,6 +245,7 @@ struct SlotView: View {
     let slot: SlotRow
     /// Position among the inserts (`nil`: an instrument's own slot).
     let index: Int?
+    @Environment(\.openPluginWindow) private var openWindow
 
     static func slotID(_ text: String) -> UInt64? {
         text.hasPrefix("slot:") ? UInt64(text.dropFirst(5)) : nil
@@ -271,6 +273,7 @@ struct SlotView: View {
         .frame(maxWidth: .infinity, minHeight: 18)
         .background(RoundedRectangle(cornerRadius: 3).fill(.background))
         .help(reason ?? "\(slot.plugin.name) (\(slot.plugin.vendor))")
+        .onTapGesture(count: 2) { openWindow(slot.id) }
         .contextMenu { menu }
         .modifier(Reorder(model: model, strip: strip, slot: slot, index: index))
     }
@@ -285,6 +288,7 @@ struct SlotView: View {
     }
 
     @ViewBuilder private var menu: some View {
+        Button("Open") { openWindow(slot.id) }
         if model.held.contains(slot.id) {
             Button("Load") { model.load([slot.id]) }
         }
VIZDAW_EOF
```
Change `app/Sources/VizDAW/DocumentWindow.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/DocumentWindow.swift b/app/Sources/VizDAW/DocumentWindow.swift
index f8de0b1..d2bf6db 100644
--- a/app/Sources/VizDAW/DocumentWindow.swift
+++ b/app/Sources/VizDAW/DocumentWindow.swift
@@ -27,6 +27,7 @@ struct DocumentWindow: View {
                     }
                     .onAppear {
                         SmokeRun.start(model) { openWindow(id: "mixer", value: id) }
+                        PluginWindows.reopen(store, doc: id)
                     }
                     .onDisappear { store.close(id) }
             } else {
VIZDAW_EOF
```
Change `app/Sources/VizDAW/VizDAWApp.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/VizDAWApp.swift b/app/Sources/VizDAW/VizDAWApp.swift
index 6d22030..fc68ec6 100644
--- a/app/Sources/VizDAW/VizDAWApp.swift
+++ b/app/Sources/VizDAW/VizDAWApp.swift
@@ -121,6 +121,7 @@ enum PluginTicker {
         let t = Timer(timeInterval: 1.0 / 60.0, repeats: true) { _ in
             MainActor.assumeIsolated {
                 for model in store.documents.values { model.idle() }
+                PluginWindows.tick(store)
                 ticks += 1
                 if ticks % 15 == 0 { appPlugins.poll() }
             }
@@ -181,8 +182,10 @@ enum SmokeRun {
             let strips = mixer?.snapshot.strips.count ?? 0
             let meters = mixer?.meters.count ?? 0
             FileHandle.standardError.write(Data("smoke: mixer strips \(strips) meters \(meters)\n".utf8))
+            let editor = await smokePluginWindow(model)
+            FileHandle.standardError.write(Data("smoke: plugin window \(editor ?? "none")\n".utf8))
             let ok = model.alert == nil && (ticks.last ?? 0) > (ticks.first ?? 0)
-                && strips >= 2 && meters == strips
+                && strips >= 2 && meters == strips && editor != nil
             if let a = model.alert {
                 FileHandle.standardError.write(Data("smoke: alert \(a.title): \(a.message)\n".utf8))
             }
@@ -190,4 +193,24 @@ enum SmokeRun {
             exit(ok ? 0 : 1)
         }
     }
+
+    /// Puts Apple's AUDelay on the first strip and opens its window: the
+    /// size of the editor it shows, or nil.
+    @MainActor static func smokePluginWindow(_ model: DocumentModel) async -> String? {
+        guard let id = appStore.documents.first(where: { $0.value === model })?.key,
+            let mixer = appStore.mixer(id), let strip = mixer.strips.first
+        else { return nil }
+        mixer.browser = .insert(strip: strip.id)
+        mixer.pick(PluginRefRow(format: .au, id: "aufx/dely/appl", name: "AUDelay", vendor: "Apple", version: ""))
+        guard let slot = mixer.snapshot.strips.first(where: { $0.id == strip.id })?.inserts.last?.id else {
+            return nil
+        }
+        for _ in 0..<40 {
+            try? await Task.sleep(for: .milliseconds(50))
+            if model.slotsVersion > 0 { break }
+        }
+        PluginWindows.show(appStore, doc: id, slot: slot)
+        try? await Task.sleep(for: .milliseconds(300))
+        return PluginWindows.editorSize(doc: id, slot: slot).map { "\($0.width)x\($0.height)" }
+    }
 }
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests and the smoke run**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build
scripts/swift.sh test
VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --smoke 2>&1 | grep 'smoke:' | tail -3
```

Expected: 59 tests in 11 suites pass (`PluginWindowModelTests` 5 new); the smoke run prints `smoke: mixer strips 2 meters 2`, `smoke: plugin window 484x255` (AUDelay's own view, opened on the main thread through the FFI; the size may differ by macOS version) and `smoke: ok`.

- [ ] **Step 5: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/PluginWindowModel.swift app/Sources/VizDAW/PluginWindows.swift \
  app/Sources/VizDAW/MixerView.swift app/Sources/VizDAW/DocumentWindow.swift \
  app/Sources/VizDAW/VizDAWApp.swift app/Tests/VizDAWAppModelTests/PluginWindowModelTests.swift
git commit -m "$(cat <<'MSG'
feat(app): plugin windows

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 13: The Plugin Manager window


**Files:**
- Create: `app/Sources/VizDAWAppModel/PluginManagerModel.swift`
- Create: `app/Sources/VizDAW/PluginManagerView.swift`
- Modify: `app/Sources/VizDAW/VizDAWApp.swift` (`pluginManager`, the window scene), `app/Sources/VizDAW/AppCommands.swift` (Window > Plugin Manager ⌥⌘P)
- Test: `app/Tests/VizDAWAppModelTests/PluginManagerTests.swift` (new)

**Interfaces:**
- Consumes: Task 10's `AppPlugins` (`rows`, `progress`, `isScanning`, `setEnabled`, `rescan`, `scan(all:)`, `clearBlocklist`, `revealPath`), `FakeCatalog`, `catalogRow(...)`.
- Produces: `PluginManagerModel(plugins:)` with `search`, `format: FormatFilter` (`.all`, `.au`, `.clap`), `selection: Set<String>`, `rows: [ManagerRow]`, `progressText`, `setEnabled(_:_:)`, `rescanSelected()`, `rescanAll()`, `clearBlocklist()`, `canClearBlocklist`, `canRescan`, `revealPath`; `ManagerRow { row, id, name, vendor, version, formatText, kindText, statusText, isEnabled, isBlocklisted, hasPlugin, key }`. In `VizDAW`: `PluginManagerView(model:)`, `pluginManager`.

Spec §9: a table of every known plugin (an On switch, name, vendor, format, version, type, status with the exact texts `OK`, `Disabled`, `Blocklisted: reason`, `Not validated`, `Missing`; the bundle path as help), a search field, an AU/CLAP filter, Rescan Selected, Rescan All, Clear Blocklist (rescans) and Reveal in Finder. A bundle that failed as a whole (no plugin id) cannot be enabled or disabled.

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/PluginManagerTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/PluginManagerTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

@MainActor
@Suite struct PluginManagerTests {
    func manager() -> (FakeCatalog, PluginManagerModel) {
        let cat = FakeCatalog()
        cat.table = [
            catalogRow("u.diva", "Diva", vendor: "u-he", kind: .instrument),
            catalogRow("x.crash", "Crasher", vendor: "Bad", status: .blocklisted(reason: "crashed (signal 11)")),
            catalogRow("aufx/dely/appl", "AUDelay", vendor: "Apple", format: .au, status: .notValidated),
            catalogRow("", "Broken", vendor: "", status: .blocklisted(reason: "timed out")),
        ]
        let p = AppPlugins(open: { _ in cat })
        p.start(config: AppPlugins.config(supportDir: URL(fileURLWithPath: "/tmp"), scanner: "x"), scan: false)
        return (cat, PluginManagerModel(plugins: p))
    }

    @Test func rowsShowTheSpecStatusesAndFilter() {
        let (_, m) = manager()
        #expect(m.rows.map(\.statusText) == ["OK", "Blocklisted: crashed (signal 11)", "Not validated", "Blocklisted: timed out"])
        #expect(m.rows[0].kindText == "Instrument")
        m.format = .au
        #expect(m.rows.map(\.name) == ["AUDelay"])
        m.format = .all
        m.search = "U-HE"
        #expect(m.rows.map(\.name) == ["Diva"])
    }

    @Test func disablingAndEnablingReachTheCatalog() {
        let (cat, m) = manager()
        m.setEnabled(m.rows[0], false)
        #expect(cat.enabled.map(\.1) == [false])
        #expect(m.rows[0].statusText == "Disabled")
        #expect(!m.rows[0].isEnabled)
        // A bundle that failed as a whole has nothing to enable.
        m.setEnabled(m.rows[3], false)
        #expect(cat.enabled.count == 1)
    }

    @Test func selectionDrivesRescanClearAndReveal() {
        let (cat, m) = manager()
        #expect(!m.canRescan && !m.canClearBlocklist)
        m.selection = [m.rows[1].id, m.rows[3].id]
        #expect(m.canClearBlocklist)
        m.clearBlocklist()
        #expect(cat.cleared.map(\.id) == ["x.crash"])
        m.selection = [m.rows[0].id]
        m.rescanSelected()
        #expect(cat.rescans.first?.map(\.id) == ["u.diva"])
        #expect(m.revealPath == "/Library/Audio/Plug-Ins/CLAP/u.diva.clap")
        m.rescanAll()
        #expect(cat.scans == [true])
        #expect(m.progressText == "Scanning 0 of 2…")
        #expect(!m.canRescan, "one scan at a time")
    }
}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh
scripts/swift.sh test
```

Expected: compile errors — cannot find `PluginManagerModel` in scope.

- [ ] **Step 3: Implement**

Create `app/Sources/VizDAWAppModel/PluginManagerModel.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/PluginManagerModel.swift <<'VIZDAW_EOF'
import Foundation
import Observation
import VizDAWCore

/// The Plugin Manager window (spec §9): every known plugin with its
/// status; enable/disable, rescan selected or all, clear a blocklist entry,
/// reveal in Finder; a search field and a format filter.
@Observable
@MainActor
public final class PluginManagerModel {
    public let plugins: AppPlugins
    public var search = ""
    public var format: FormatFilter = .all
    /// Selected rows (`ManagerRow.id`).
    public var selection: Set<String> = []

    public enum FormatFilter: String, CaseIterable, Identifiable, Sendable {
        case all = "All"
        case au = "AU"
        case clap = "CLAP"
        public var id: String { rawValue }
    }

    public init(plugins: AppPlugins) {
        self.plugins = plugins
    }

    /// One table row.
    public struct ManagerRow: Identifiable, Equatable, Sendable {
        public let row: CatalogRowFfi
        public var id: String { "\(row.plugin.format):\(row.plugin.id)|\(row.path)" }
        public var name: String { row.plugin.name }
        public var vendor: String { row.plugin.vendor }
        public var version: String { row.plugin.version }
        public var formatText: String { row.plugin.format == .au ? "AU" : "CLAP" }
        public var kindText: String {
            switch row.kind {
            case .instrument: "Instrument"
            case .effect: "Effect"
            case nil: "—"
            }
        }
        /// `OK`, `Disabled`, `Blocklisted: reason`, `Not validated`,
        /// `Missing` (spec §9).
        public var statusText: String {
            switch row.status {
            case .ok: "OK"
            case .disabled: "Disabled"
            case .blocklisted(let reason): "Blocklisted: \(reason)"
            case .notValidated: "Not validated"
            case .missing: "Missing"
            }
        }
        public var isEnabled: Bool { row.status != .disabled }
        public var isBlocklisted: Bool {
            if case .blocklisted = row.status { return true }
            return false
        }
        /// A whole bundle that failed has no plugin id: nothing to enable.
        public var hasPlugin: Bool { !row.plugin.id.isEmpty }
        public var key: PluginKey { PluginKey(row.plugin) }
    }

    /// The rows the table shows now.
    public var rows: [ManagerRow] {
        let q = search.trimmingCharacters(in: .whitespaces).lowercased()
        return plugins.rows.map(ManagerRow.init).filter { r in
            let f: Bool =
                switch format {
                case .all: true
                case .au: r.row.plugin.format == .au
                case .clap: r.row.plugin.format == .clap
                }
            return f && (q.isEmpty || r.name.lowercased().contains(q) || r.vendor.lowercased().contains(q))
        }
    }

    private var selected: [ManagerRow] { rows.filter { selection.contains($0.id) } }

    /// "Scanning 3 of 40…", or "" when idle.
    public var progressText: String {
        let p = plugins.progress
        return p.running ? "Scanning \(p.done) of \(p.total)…" : ""
    }

    // MARK: Intents

    public func setEnabled(_ r: ManagerRow, _ on: Bool) {
        guard r.hasPlugin else { return }
        plugins.setEnabled(r.key, on)
    }

    public func rescanSelected() {
        let keys = selected.filter(\.hasPlugin).map(\.key)
        if !keys.isEmpty { plugins.rescan(keys) }
    }

    public func rescanAll() { plugins.scan(all: true) }

    /// Clears the selected blocklist entries; each bundle is scanned again.
    public func clearBlocklist() {
        for r in selected where r.isBlocklisted && r.hasPlugin { plugins.clearBlocklist(r.key) }
    }

    public var canClearBlocklist: Bool { selected.contains { $0.isBlocklisted && $0.hasPlugin } }
    public var canRescan: Bool { !selected.isEmpty && !plugins.isScanning }

    /// The first selected row's bundle, for Reveal in Finder.
    public var revealPath: String? {
        selected.lazy.compactMap { self.plugins.revealPath($0.key) }.first
    }
}
VIZDAW_EOF
```
Create `app/Sources/VizDAW/PluginManagerView.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAW
cat > app/Sources/VizDAW/PluginManagerView.swift <<'VIZDAW_EOF'
import AppKit
import SwiftUI
import VizDAWAppModel
import VizDAWCore

/// The Plugin Manager window (spec §9).
struct PluginManagerView: View {
    @Bindable var model: PluginManagerModel

    var body: some View {
        Table(model.rows, selection: $model.selection) {
            TableColumn("On") { r in
                Toggle("Enabled", isOn: Binding(get: { r.isEnabled }, set: { model.setEnabled(r, $0) }))
                    .labelsHidden()
                    .disabled(!r.hasPlugin)
            }
            .width(30)
            TableColumn("Name", value: \.name)
            TableColumn("Vendor", value: \.vendor)
            TableColumn("Format", value: \.formatText).width(50)
            TableColumn("Version", value: \.version).width(70)
            TableColumn("Type", value: \.kindText).width(80)
            TableColumn("Status") { r in
                Text(r.statusText)
                    .foregroundStyle(r.row.status == .ok ? Color.primary : Color.orange)
                    .help(r.row.path)
            }
        }
        .searchable(text: $model.search)
        .toolbar {
            ToolbarItemGroup {
                Picker("Format", selection: $model.format) {
                    ForEach(PluginManagerModel.FormatFilter.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                Text(model.progressText).font(.caption).foregroundStyle(.secondary)
                Button("Rescan Selected") { model.rescanSelected() }
                    .disabled(!model.canRescan)
                Button("Rescan All") { model.rescanAll() }
                    .disabled(model.plugins.isScanning)
                Button("Clear Blocklist") { model.clearBlocklist() }
                    .disabled(!model.canClearBlocklist)
                Button("Reveal in Finder") {
                    if let p = model.revealPath {
                        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: p)])
                    }
                }
                .disabled(model.revealPath == nil)
            }
        }
        .frame(minWidth: 760, minHeight: 360)
    }
}
VIZDAW_EOF
```
Change `app/Sources/VizDAW/VizDAWApp.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/VizDAWApp.swift b/app/Sources/VizDAW/VizDAWApp.swift
index fc68ec6..cec92b5 100644
--- a/app/Sources/VizDAW/VizDAWApp.swift
+++ b/app/Sources/VizDAW/VizDAWApp.swift
@@ -9,6 +9,9 @@ import VizDAWCore
 /// The app's plugin catalog (main actor).
 @MainActor let appPlugins = AppPlugins(open: { try PluginCatalog.open(config: $0) })
 
+/// The Plugin Manager window's model.
+@MainActor let pluginManager = PluginManagerModel(plugins: appPlugins)
+
 @main
 struct VizDAWApp: App {
     @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
@@ -27,6 +30,9 @@ struct VizDAWApp: App {
             MixerWindow(id: $id, store: appStore)
         }
         .defaultSize(width: 760, height: 560)
+        Window("Plugin Manager", id: "plugins") {
+            PluginManagerView(model: pluginManager)
+        }
     }
 }
 
VIZDAW_EOF
```
Change `app/Sources/VizDAW/AppCommands.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/AppCommands.swift b/app/Sources/VizDAW/AppCommands.swift
index 12ae9d4..7be819f 100644
--- a/app/Sources/VizDAW/AppCommands.swift
+++ b/app/Sources/VizDAW/AppCommands.swift
@@ -61,6 +61,8 @@ struct AppCommands: Commands {
             Button("Mixer") { if let docID { openWindow(id: "mixer", value: docID) } }
                 .keyboardShortcut("m", modifiers: [.command, .option])
                 .disabled(docID == nil)
+            Button("Plugin Manager") { openWindow(id: "plugins") }
+                .keyboardShortcut("p", modifiers: [.command, .option])
             Divider()
         }
         CommandMenu("Transport") {
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build
scripts/swift.sh test
```

Expected: 62 tests in 12 suites pass (`PluginManagerTests` 3 new).

- [ ] **Step 5: Check everything**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
```

Expected: clean; every test passes.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/PluginManagerModel.swift app/Sources/VizDAW/PluginManagerView.swift \
  app/Sources/VizDAW/VizDAWApp.swift app/Sources/VizDAW/AppCommands.swift \
  app/Tests/VizDAWAppModelTests/PluginManagerTests.swift
git commit -m "$(cat <<'MSG'
feat(app): the Plugin Manager window

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

---

### Task 14: Recovery prompts, clean quit, the plugin checklist and the final gate


**Files:**
- Create: `app/Sources/VizDAWAppModel/Recovery.swift` (`CrashChoice`, `RecoveryQuestions`, `Recovery`)
- Modify: `app/Sources/VizDAWAppModel/DocumentStore.swift` (`recovery`, `close` closes the session, `closeAll()`)
- Modify: `app/Sources/VizDAW/VizDAWApp.swift` (`appStore.recovery` with `RecoveryAlerts`, `applicationWillTerminate`)
- Modify: `app/README.md` (plugins section, checklist steps 9–20)
- Test: `app/Tests/VizDAWAppModelTests/RecoveryTests.swift` (new), `app/Tests/VizDAWAppModelTests/IntegrationTests.swift` (the mixer and recovery through the real library)

**Interfaces:**
- Consumes: Task 9's `recoveryCheck(path:)`, `Session.openWith(path:options:)`, `OpenOptions`, `RecoveryInfo`, `Session.close()`; Task 10's `appPlugins.setEnabled`; Task 11's `MixerModel`.
- Produces: `CrashChoice::{pluginsOff, normally, cancel, disable}`; `protocol RecoveryQuestions { afterCrash(project:loading:crashed:) -> CrashChoice; restoreAutosave(project:) -> Bool }`; `Recovery(check:openWith:disable:shiftHeld:questions:)` with `options(for:) -> OpenOptions?`; `DocumentStore.recovery: Recovery?`, `DocumentStore.closeAll()`. In `VizDAW`: `RecoveryAlerts`.

Spec §10's prompts. Before a project opens: after a crash (or with Shift held) the store asks "VizDAW quit unexpectedly while this project was open[, while loading ‹plugin›]" — Open with Plugins Off / Open Normally / Cancel, plus Disable ‹plugin› when one was loading (it is disabled in the Plugin Manager, then the project opens normally); then, for a newer autosave, Restore Autosave / Open Last Saved. Closing a window and quitting close the sessions cleanly (no marker left). The README documents the plugin features and the manual checklist (spec §13: plugin windows, Plugin Manager, safe mode by killing VizDAW during a load, autosave restore, a sidechain and a multi-out session with the user's plugins).

- [ ] **Step 1: Write the failing tests**

Create `app/Tests/VizDAWAppModelTests/RecoveryTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Tests/VizDAWAppModelTests
cat > app/Tests/VizDAWAppModelTests/RecoveryTests.swift <<'VIZDAW_EOF'
import Foundation
import Testing
import VizDAWCore
@testable import VizDAWAppModel

/// Scripted answers.
@MainActor
final class FakeQuestions: RecoveryQuestions {
    var crash: CrashChoice = .normally
    var restore = false
    var asked: [String] = []

    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice {
        asked.append("crash \(project) \(crashed) \(loading.map(\.name))")
        return crash
    }

    func restoreAutosave(project: String) -> Bool {
        asked.append("restore \(project)")
        return restore
    }
}

@MainActor
@Suite struct RecoveryTests {
    let diva = PluginRefRow(format: .au, id: "aumu/Diva/UHfX", name: "Diva", vendor: "u-he", version: "1")

    func store(
        _ info: RecoveryInfo, _ q: FakeQuestions, shift: Bool = false
    ) -> (DocumentStore, () -> [OpenOptions], () -> [[PluginRefRow]]) {
        var opened: [OpenOptions] = []
        var disabled: [[PluginRefRow]] = []
        let s = DocumentStore(makeEmpty: { FakeSession([]) }, makeDemo: { demoFake() }, open: { _ in demoFake() })
        s.recovery = Recovery(
            check: { _ in info },
            openWith: { _, o in
                opened.append(o)
                return demoFake()
            },
            disable: { disabled.append($0) },
            shiftHeld: { shift },
            questions: q)
        return (s, { opened }, { disabled })
    }

    @Test func aCleanProjectOpensWithoutQuestions() {
        let q = FakeQuestions()
        let (s, opened, _) = store(RecoveryInfo(crashed: false, loading: [], autosave: nil), q)
        #expect(s.open(path: "/p/Song.vizdaw") != nil)
        #expect(q.asked.isEmpty)
        #expect(opened() == [OpenOptions(pluginsOff: false, autosave: nil)])
    }

    @Test func afterACrashPluginsOffOrCancel() {
        let q = FakeQuestions()
        q.crash = .pluginsOff
        let info = RecoveryInfo(crashed: true, loading: [diva], autosave: nil)
        let (s, opened, _) = store(info, q)
        #expect(s.open(path: "/p/Song.vizdaw") != nil)
        #expect(q.asked == ["crash Song true [\"Diva\"]"])
        #expect(opened() == [OpenOptions(pluginsOff: true, autosave: nil)])
        q.crash = .cancel
        #expect(s.open(path: "/p/Song.vizdaw") == nil)
        #expect(opened().count == 1)
        #expect(s.alert == nil)
    }

    @Test func disablingTheCrashingPluginThenOpening() {
        let q = FakeQuestions()
        q.crash = .disable
        let (s, opened, disabled) = store(RecoveryInfo(crashed: true, loading: [diva], autosave: nil), q)
        #expect(s.open(path: "/p/Song.vizdaw") != nil)
        #expect(disabled() == [[diva]])
        #expect(opened() == [OpenOptions(pluginsOff: false, autosave: nil)])
    }

    @Test func shiftAsksEvenWithoutACrash() {
        let q = FakeQuestions()
        q.crash = .pluginsOff
        let (s, opened, _) = store(RecoveryInfo(crashed: false, loading: [], autosave: nil), q, shift: true)
        _ = s.open(path: "/p/Song.vizdaw")
        #expect(q.asked == ["crash Song false []"])
        #expect(opened().first?.pluginsOff == true)
    }

    @Test func aNewerAutosaveIsOfferedAndRestored() {
        let q = FakeQuestions()
        q.restore = true
        let info = RecoveryInfo(crashed: false, loading: [], autosave: "/p/Song.vizdaw/Autosave/project-1.json")
        let (s, opened, _) = store(info, q)
        _ = s.open(path: "/p/Song.vizdaw")
        #expect(q.asked == ["restore Song"])
        #expect(opened().first?.autosave == "/p/Song.vizdaw/Autosave/project-1.json")
        q.restore = false
        _ = s.open(path: "/p/Song.vizdaw")
        #expect(opened().last?.autosave == nil)
    }

    @Test func closingADocumentClosesItsSessionCleanly() throws {
        let f = demoFake()
        let s = DocumentStore(makeEmpty: { f }, makeDemo: { f }, open: { _ in f })
        _ = try #require(s.newEmpty())
        _ = try #require(s.newDemo())
        s.closeAll()
        #expect(f.closeCalls == 2)
        #expect(s.documents.isEmpty)
    }
}
VIZDAW_EOF
```
Change `app/Tests/VizDAWAppModelTests/IntegrationTests.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Tests/VizDAWAppModelTests/IntegrationTests.swift b/app/Tests/VizDAWAppModelTests/IntegrationTests.swift
index d787022..531fe05 100644
--- a/app/Tests/VizDAWAppModelTests/IntegrationTests.swift
+++ b/app/Tests/VizDAWAppModelTests/IntegrationTests.swift
@@ -26,3 +26,30 @@ import VizDAWCore
         #expect(!m.isPlaying)
     }
 }
+
+/// The mixer and recovery through the real library (offline audio; a
+/// temporary support folder, never the user's).
+@MainActor
+@Suite struct MixerIntegrationTests {
+    @Test func aBusAndASaveThroughTheRealLibrary() throws {
+        setenv("VIZDAW_AUDIO", "offline", 1)
+        let support = FileManager.default.temporaryDirectory
+            .appendingPathComponent("vizdaw-swift-test-\(ProcessInfo.processInfo.processIdentifier)")
+        try setSupportDir(path: support.path)
+        let doc = try DocumentModel(session: Session.newDemo())
+        let mixer = try MixerModel(document: doc)
+        #expect(mixer.strips.map(\.name) == ["Built-in Synth"])
+        #expect(mixer.master?.name == "Master")
+        mixer.addBus()
+        #expect(mixer.strips.map(\.name) == ["Built-in Synth", "Bus 1"])
+        #expect(doc.undoTitle == "Undo Add Bus")
+        let path = support.appendingPathComponent("mix.vizdaw").path
+        doc.saveAs(path: path)
+        #expect(doc.alert == nil)
+        let info = try recoveryCheck(path: path)
+        #expect(!info.crashed, "open here, not a crash")
+        #expect(info.autosave == nil)
+        try doc.session.close()
+        try? FileManager.default.removeItem(at: support)
+    }
+}
VIZDAW_EOF
```

- [ ] **Step 2: Run them to see them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/build-ffi.sh
scripts/swift.sh test
```

Expected: compile errors — cannot find `RecoveryQuestions`, `CrashChoice`, `Recovery`; `DocumentStore` has no member `recovery`, `closeAll`.

- [ ] **Step 3: Implement**

Create `app/Sources/VizDAWAppModel/Recovery.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
mkdir -p app/Sources/VizDAWAppModel
cat > app/Sources/VizDAWAppModel/Recovery.swift <<'VIZDAW_EOF'
import Foundation
import VizDAWCore

/// What the user chose after a crash (spec §10): "Open with Plugins Off /
/// Open Normally / Cancel", plus "Disable ‹plugin›" when one was loading.
public enum CrashChoice: Equatable, Sendable {
    case pluginsOff
    case normally
    case cancel
    /// Disable the plugins that were loading, then open normally.
    case disable
}

/// The questions the store asks before opening a project.
@MainActor
public protocol RecoveryQuestions: AnyObject {
    /// VizDAW quit unexpectedly while `project` was open (`crashed`), or
    /// Shift was held while opening it.
    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice
    /// A newer autosave of `project` exists: restore it?
    func restoreAutosave(project: String) -> Bool
}

/// How the store checks and opens projects (the app gives the real ones;
/// tests give fakes).
@MainActor
public struct Recovery {
    public var check: (String) throws -> RecoveryInfo
    public var openWith: (String, OpenOptions) throws -> any SessionProtocol
    /// Disables plugins in the Plugin Manager.
    public var disable: ([PluginRefRow]) -> Void
    public var shiftHeld: () -> Bool
    public var questions: any RecoveryQuestions

    public init(
        check: @escaping (String) throws -> RecoveryInfo,
        openWith: @escaping (String, OpenOptions) throws -> any SessionProtocol,
        disable: @escaping ([PluginRefRow]) -> Void,
        shiftHeld: @escaping () -> Bool,
        questions: any RecoveryQuestions
    ) {
        self.check = check
        self.openWith = openWith
        self.disable = disable
        self.shiftHeld = shiftHeld
        self.questions = questions
    }

    /// The options to open `path` with, or nil when the user cancelled.
    public func options(for path: String) -> OpenOptions? {
        var options = OpenOptions(pluginsOff: false, autosave: nil)
        guard let info = try? check(path) else { return options }
        let name = ((path as NSString).lastPathComponent as NSString).deletingPathExtension
        if info.crashed || shiftHeld() {
            switch questions.afterCrash(project: name, loading: info.loading, crashed: info.crashed) {
            case .cancel: return nil
            case .pluginsOff: options.pluginsOff = true
            case .normally: break
            case .disable: disable(info.loading)
            }
        }
        if let a = info.autosave, questions.restoreAutosave(project: name) {
            options.autosave = a
        }
        return options
    }
}
VIZDAW_EOF
```
Change `app/Sources/VizDAWAppModel/DocumentStore.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAWAppModel/DocumentStore.swift b/app/Sources/VizDAWAppModel/DocumentStore.swift
index f1ed9f8..2cb923a 100644
--- a/app/Sources/VizDAWAppModel/DocumentStore.swift
+++ b/app/Sources/VizDAWAppModel/DocumentStore.swift
@@ -18,6 +18,9 @@ public final class DocumentStore {
     public private(set) var pending: [UUID] = []
     /// Failures that have no window to show them in.
     public var alert: AlertItem?
+    /// Crash and autosave checks before a project opens (spec §10); none
+    /// in tests that do not set it.
+    public var recovery: Recovery?
 
     private let makeEmpty: Make
     private let makeDemo: Make
@@ -66,7 +69,11 @@ public final class DocumentStore {
                 message: "\((path as NSString).lastPathComponent) is not a .vizdaw project.")
             return nil
         }
-        return add("Open Failed") { try self.openPath(path) }
+        guard let recovery else {
+            return add("Open Failed") { try self.openPath(path) }
+        }
+        guard let options = recovery.options(for: path) else { return nil }
+        return add("Open Failed") { try recovery.openWith(path, options) }
     }
 
     /// Queues a document for the next window to appear.
@@ -93,13 +100,21 @@ public final class DocumentStore {
         return alert
     }
 
+    /// A window closed: its document goes (a clean close: the crash
+    /// marker is removed).
     public func close(_ id: UUID) {
         mixers.removeValue(forKey: id)
-        if let doc = documents.removeValue(forKey: id), doc.isPlaying {
-            try? doc.session.stop()
+        if let doc = documents.removeValue(forKey: id) {
+            if doc.isPlaying { try? doc.session.stop() }
+            try? doc.session.close()
         }
     }
 
+    /// The app quits: every document closes cleanly.
+    public func closeAll() {
+        for id in Array(documents.keys) { close(id) }
+    }
+
     private func add(_ title: String, _ make: Make) -> UUID? {
         do {
             let model = try DocumentModel(session: make())
VIZDAW_EOF
```
Change `app/Sources/VizDAW/VizDAWApp.swift`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/Sources/VizDAW/VizDAWApp.swift b/app/Sources/VizDAW/VizDAWApp.swift
index cec92b5..d7831ed 100644
--- a/app/Sources/VizDAW/VizDAWApp.swift
+++ b/app/Sources/VizDAW/VizDAWApp.swift
@@ -17,6 +17,14 @@ struct VizDAWApp: App {
     @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
 
     init() {
+        appStore.recovery = Recovery(
+            check: { try recoveryCheck(path: $0) },
+            openWith: { try Session.openWith(path: $0, options: $1) },
+            disable: { plugins in
+                for p in plugins { appPlugins.setEnabled(PluginKey(p), false) }
+            },
+            shiftHeld: { NSEvent.modifierFlags.contains(.shift) },
+            questions: RecoveryAlerts())
         LaunchArguments.apply(CommandLine.arguments, to: appStore)
     }
 
@@ -47,6 +55,11 @@ final class AppDelegate: NSObject, NSApplicationDelegate {
         PluginTicker.start(appStore)
     }
 
+    func applicationWillTerminate(_ notification: Notification) {
+        // A clean quit leaves no crash markers.
+        MainActor.assumeIsolated { appStore.closeAll() }
+    }
+
     func application(_ application: NSApplication, open urls: [URL]) {
         for url in urls {
             appStore.enqueue(appStore.open(path: url.path))
@@ -220,3 +233,42 @@ enum SmokeRun {
         return PluginWindows.editorSize(doc: id, slot: slot).map { "\($0.width)x\($0.height)" }
     }
 }
+
+/// The recovery questions as modal alerts (spec §10).
+@MainActor
+final class RecoveryAlerts: RecoveryQuestions {
+    func afterCrash(project: String, loading: [PluginRefRow], crashed: Bool) -> CrashChoice {
+        let alert = NSAlert()
+        alert.alertStyle = .warning
+        let names = loading.map(\.name).joined(separator: ", ")
+        if crashed {
+            alert.messageText = loading.isEmpty
+                ? "VizDAW quit unexpectedly while this project was open."
+                : "VizDAW quit unexpectedly while this project was open, while loading \(names)."
+        } else {
+            alert.messageText = "Open “\(project)” with plugins off?"
+        }
+        alert.informativeText = "With plugins off, every plugin is a placeholder you can load one by one."
+        alert.addButton(withTitle: "Open with Plugins Off")
+        alert.addButton(withTitle: "Open Normally")
+        alert.addButton(withTitle: "Cancel")
+        if crashed && !loading.isEmpty {
+            alert.addButton(withTitle: "Disable \(names)")
+        }
+        switch alert.runModal() {
+        case .alertFirstButtonReturn: return .pluginsOff
+        case .alertSecondButtonReturn: return .normally
+        case .alertThirdButtonReturn: return .cancel
+        default: return .disable
+        }
+    }
+
+    func restoreAutosave(project: String) -> Bool {
+        let alert = NSAlert()
+        alert.messageText = "“\(project)” has an autosave newer than its last save."
+        alert.informativeText = "Restoring opens the autosaved version as an unsaved change."
+        alert.addButton(withTitle: "Restore Autosave")
+        alert.addButton(withTitle: "Open Last Saved")
+        return alert.runModal() == .alertFirstButtonReturn
+    }
+}
VIZDAW_EOF
```
Change `app/README.md`:

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git apply <<'VIZDAW_EOF'
diff --git a/app/README.md b/app/README.md
index 33c9516..8f10977 100644
--- a/app/README.md
+++ b/app/README.md
@@ -17,8 +17,26 @@ framework paths that the Command Line Tools need. With full Xcode installed
 it runs plain `swift`.
 
 Launch arguments: `--demo` opens the demo project; a path ending in
-`.vizdaw` opens that project; `--smoke` plays the demo for 3 s, prints the
-transport and exits 0 when the position advanced.
+`.vizdaw` opens that project; `--no-scan` skips the plugin scan at launch;
+`--smoke` plays the demo for 3 s with the mixer open, puts Apple's AUDelay
+on the first strip and opens its window, prints the transport, the mixer
+and the editor size, and exits 0 when all of them worked. `--smoke` uses a
+temporary support folder and never scans.
+
+## Plugins
+
+- The catalog lives in `~/Library/Application Support/VizDAW/plugins.json`
+  (`VIZDAW_SUPPORT_DIR` moves the whole folder, crash markers included).
+  At launch VizDAW scans new or changed AU and CLAP plugins in the
+  background with `vizdaw-scan` (next to the app's executable in the
+  bundle; `scripts/swift.sh` points `VIZDAW_SCANNER` at
+  `target/ffi/vizdaw-scan`).
+- Window > Plugin Manager (⌥⌘P) lists every plugin with its status;
+  disable one that crashes VizDAW there. Window > Mixer (⌥⌘M) shows the
+  focused document's mixer.
+- The built-in synth plays through its strip's constant-power pan, so it
+  is 3 dB quieter than in 2c (3a ruling).
+- Muting a strip also silences its pre-fader sends (plan 3c ruling).
 
 `VIZDAW_AUDIO=offline` renders through the offline backend in real time
 (no sound, device label "Offline"). Never set it for normal use.
@@ -51,3 +69,58 @@ Generated files (not in git): `app/Sources/VizDAWCore/vizdaw_ffi.swift`,
    pause briefly); if no output device is left, an alert appears.
 8. `scripts/make-app.sh`, then `open dist/VizDAW.app`; double-click
    `~/Desktop/demo.vizdaw` in Finder: it opens in VizDAW.
+
+### Plugins (plan 3c)
+
+Use `dist/VizDAW.app` from step 8 and your own plugins (for example
+Diva, a CLAP effect, a reverb, a compressor with a sidechain input and a
+multi-output drum instrument).
+
+9. Launch: within a minute the scan finishes (Window > Plugin Manager
+   shows "Scanning n of m…" until then). Every installed plugin is listed
+   with name, vendor, format, version, type and status. The search field
+   and the AU/CLAP filter narrow the table.
+10. Open the demo, then Window > Mixer: strips "Built-in Synth" and
+    "Master", each with a fader, pan, M/S, an output menu and a meter that
+    moves while playing. The toolbar shows the DSP load.
+11. Mixer toolbar > Instruments > "Built-in Piano: MIDI → Plugin…": the
+    browser lists instruments grouped by vendor; pick Diva. A "Built-in
+    Piano" instrument strip appears; its slot turns green once loaded.
+    Play Song: the chords sound through Diva. (Instruments > New Plugin
+    Instrument… adds a destination of its own.) Double-click the slot: a window
+    "Diva — Diva" shows Diva's own editor. Turning a knob in it and then
+    Edit > Undo returns the knob (one undo step per drag). Closing the
+    window does not remove the plugin.
+12. On a strip, "+": add a CLAP effect and an AU effect. Drag the second
+    above the first: the order changes. Bypass one from its slot menu
+    and from its window's header. Remove one, then Undo: it comes back
+    with the settings it had.
+13. "Add Bus", rename it "Verb" (double-click its name), add a reverb to
+    it, add a send from the Diva strip to Verb; drag the send level and
+    toggle Pre: the reverb follows. Solo the Diva strip: the bus still
+    sounds, other strips go dim and silent.
+14. Sidechain: put a compressor with a sidechain input on a pad strip,
+    choose the kick's strip in its Sidechain menu, play: the pad ducks on
+    the kick.
+15. Multi-output: with a multi-output instrument, the strip's name menu
+    offers "Add Outputs"; add one: a new strip plays that output.
+16. Tempo sync: an AU delay set to a note value follows the song's tempo
+    (it slows over the last two bars of Song).
+17. Plugin Manager: disable the CLAP effect. Close and reopen the
+    project: its slot is dimmed "Disabled in Plugin Manager" and passes
+    audio. Enable it again: the slot loads. "Rescan All" scans every
+    plugin; a crashing plugin shows "Blocklisted: crashed (signal 11)" and
+    "Clear Blocklist" scans it again.
+18. Save, leave two plugin windows open, close the project and reopen
+    it: both windows reopen.
+19. Safe mode: open a project with a heavy plugin and quit VizDAW with
+    `kill -9` while it loads (Activity Monitor > Force Quit works too).
+    Reopen the project: "VizDAW quit unexpectedly while this project was
+    open, while loading ‹plugin›" offers Open with Plugins Off / Open
+    Normally / Cancel / Disable ‹plugin›. With plugins off, every slot is
+    dimmed "Plugins off (safe mode)"; a slot's Load and a strip's "Load
+    All" load them. Opening with Shift held asks the same question.
+20. Autosave: edit a saved project, wait 2 minutes (or load a plugin),
+    then `kill -9` VizDAW. Reopen: "has an autosave newer than its last
+    save" offers Restore Autosave / Open Last Saved; restoring shows the
+    edit and the title says Edited.
VIZDAW_EOF
```

- [ ] **Step 4: Run the tests to see them pass**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
scripts/swift.sh build
scripts/swift.sh test
```

Expected: 69 tests in 14 suites pass (`RecoveryTests` 6, `MixerIntegrationTests` 1 new).

- [ ] **Step 5: The final gate**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vizdaw-play --all-targets --features rt-check -- -D warnings
cargo test --workspace
cargo run -q -p vizdaw-play -- fixtures/demo.vizdaw --seq Song --offline "$TMPDIR/demo.txt"
diff fixtures/demo.offline.txt "$TMPDIR/demo.txt" && echo "offline golden unchanged"
scripts/build-ffi.sh
scripts/swift.sh build
scripts/swift.sh test
scripts/make-app.sh
ls dist/VizDAW.app/Contents/MacOS
VIZDAW_AUDIO=offline scripts/swift.sh run VizDAW --smoke 2>&1 | grep 'smoke:' | tail -3
git status --short
```

Expected: no formatting changes, no clippy warnings (both builds), every Rust test passes (556), `offline golden unchanged`, 69 Swift tests pass, the bundle holds `VizDAW` and `vizdaw-scan`, the smoke run ends with `smoke: ok`, and `git status` lists only this task's files.

- [ ] **Step 6: Commit**

```bash
export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools
cd /Users/lattawb/workspace/vizdaw
git add app/Sources/VizDAWAppModel/Recovery.swift app/Sources/VizDAWAppModel/DocumentStore.swift \
  app/Sources/VizDAW/VizDAWApp.swift app/README.md \
  app/Tests/VizDAWAppModelTests/RecoveryTests.swift app/Tests/VizDAWAppModelTests/IntegrationTests.swift
git commit -m "$(cat <<'MSG'
feat(app): crash recovery prompts, clean quit, plugin checklist

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
MSG
)"
```

- [ ] **Step 7: Hand the manual checklist to the human**

`app/README.md` steps 9–20 need the user's own plugins, a speaker and a person: plugin windows (CLAP and AU editors, resizing, gestures as undo steps), the Plugin Manager against the real catalog, safe mode by killing VizDAW during a load, autosave restore, a sidechain and a multi-out session, an AU delay following the tempo, and an AU v3 editor. Report which steps pass; nothing in this step is automated.

---

## Self-Review

**1. Spec coverage (slice 3c of §14):**

| Spec | Task |
|---|---|
| §5 `host_idle()` from a 60 Hz main-thread timer | 4 (`Session::idle`, `PluginTicker`) |
| §5 AU editor (`requestViewController` / `kAudioUnitProperty_CocoaUI`), deferred by 3b Ruling 7 | 3, 8, 12 |
| §5 transport for AU (deferred by the 3b final review) | 2 |
| §7 worker default `min(perf cores − 1, 7)`; meters and DSP load polled at 30 Hz | 4 (`live_config`), 5 (`levels`), 11 |
| §9 Plugin Manager: table, statuses, enable/disable, rescan selected/all, clear blocklist (rescans), reveal in Finder, search, format filter | 7, 10, 13 |
| §9 disabled/blocklisted hidden from the browser; slots load as placeholders "Disabled in Plugin Manager"; enabling loads the waiting slots | 7 (Ruling 15) |
| §9 plugin browser: popover from insert slots and New Plugin Instrument, grouped by vendor, filtered, search | 10, 11 |
| §9 plugin windows: one per instance, title, resizing when allowed, bypass header, closing keeps the plugin, removing closes the window, reopen on project open | 8, 12 |
| §9 placeholders: dimmed name with the reason, saved state untouched | 4 (`SlotStatus`), 11, 12 |
| §10 autosave every 2 min and after a plugin loads, with captured states; "Restore autosave / Open last saved" | 9, 14 |
| §10 crash marker naming the project and the plugin loading; removed on a clean close | 9, 14 |
| §10 safe mode prompt (leftover marker or Shift), Plugins Off, Load per slot, Load All per strip, Disable ‹plugin› | 1, 9, 11, 14 |
| §11 FFI: mixer read model and commands, catalog and manager actions, params with gestures, editor open/close, `host_idle`, safe-mode/marker helpers, autosave | 4–9 |
| §11 mixer window: strips with name, kind, inserts (add, bypass, open, remove, drag, sidechain menu when supported), sends (target, level, pre/post), output menu, pan, fader, mute/solo, meter, master at the right, Add Bus, DSP load, Add Outputs | 11, 12 |
| §12 errors and faults surface in the UI | 4 (placeholders in the report, problems in the fault popover), 5 (`faulted`), 7 (catalog problems) |
| §13 App: Swift tests for the mixer view model; a smoke script; a manual checklist | 11 (`MixerModelTests`), 10–14 (`--smoke`), 14 (README steps 9–20) |

Carry-overs: every item of the brief maps to a ruling and a task (see "Spec gaps and contradictions", last bullet). Out of scope here and left for later sub-projects: a worker-count setting, mixer polish (spec §1), MIDI 2.0 to plugins (3b Ruling 10).

**2. Placeholder scan:** every step carries its full code (new files whole, changes as patches generated from a working tree) and its exact command; no "TBD", "similar to", or untested references.

**3. Type consistency:** the names the Swift side uses come from the generated bindings of the FFI records (`SlotRow.hasSidechainInput`, `StripRow.freeOutputs`, `MixerLevels.faulted`, `EditorSize.resizable`, `CatalogRowFfi.status`, `RecoveryInfo.autosave`, …) and were compiled against them; `FakeSession` implements the whole generated `SessionProtocol` after each FFI task, and `FakeCatalog` the whole `PluginCatalogProtocol`.

**4. Review Focus:** each of the five items has its named tests in its owning task (Tasks 4, 5, 6, 7, 9, 14). Other input classes checked and covered by tests: a plugin that is unknown (Task 4), a missing scanner (Task 7), opening an editor off the main thread or with a null view (Tasks 3, 8), a project without `ui.json` (Task 8), a routing cycle from the mixer (Task 5), a non-finite parameter value (Task 6), a drag left open around autosave (Task 9).

**Rehearsal:** every task was built on a scratch clone of `sp3-plugins` at `6456646`, then this plan's own bash blocks were replayed in order on a fresh clone: every patch applied, every "see them fail" step failed, every other step passed, and each task ended green (fmt, clippy `-D warnings`, the workspace tests, `build-ffi.sh`, `swift.sh build`/`test`, and the `--smoke` runs with offline audio).
