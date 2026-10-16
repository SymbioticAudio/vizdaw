# VizDAW Sub-project 2c — FFI and Minimal App Shell Design

- **Date:** 2026-10-15
- **Status:** Draft, written unattended (decisions logged under Rulings), awaiting review
- **Parent spec:** `2026-10-15-vizdaw-design.md` (Sections 4, 9, 10, 12)
- **Depends on:** 2a `2026-10-15-core-model-design.md` (Document, History, Command, queries) and 2b `2026-10-15-engine-design.md` (Engine, platform-macos, `fixtures::demo_document`)

## 1. Goal

Prove the whole stack end to end: a SwiftUI window opens a `.vizdaw` project, lists its sequences and tracks, and plays the selected sequence through the 2b engine. All state comes from Rust over UniFFI.

Deliverables:
- `crates/core-ffi`: a Rust library (`vizdaw_ffi`) that exposes a small API through UniFFI proc-macros.
- `crates/uniffi-bindgen`: a tiny binary that runs the UniFFI bindings generator.
- `scripts/build-ffi.sh`: builds the static library and generates the Swift bindings, with no Xcode.
- `scripts/swift.sh`: wraps `swift build`/`swift test`/`swift run` with the flags this toolchain needs (§7).
- `scripts/make-app.sh`: assembles an unsigned (ad-hoc signed) `VizDAW.app`.
- `app/`: a SwiftPM package with an executable target using the SwiftUI App lifecycle, for macOS 14+.

**Success:** on this machine (Command Line Tools only), `scripts/build-ffi.sh && scripts/swift.sh run VizDAW` opens a window. **File > Create Demo Project** shows sequences and tracks, and pressing Play sounds the demo through the built-in synth while the position display advances. `cargo test -p core-ffi` and `scripts/swift.sh test` pass.

### Out of scope
- Editors (sub-project 4).
- Any editing beyond the few commands listed in §3.
- Plugin windows.
- Document autosave UI and multiple-device selection UI.
- Notarization; Xcode projects; XCUITest. XCTest is not shipped with the Command Line Tools.
- Live change notifications from Rust: the app polls instead (§4).

## 2. Constraints

- Environment: Rust 1.99.1, Swift 6.4 Command Line Tools, macOS 26.7.1 SDK; the deployment target is macOS 14.
  - There is no `xcodebuild`, so there is no `-create-xcframework`, no `.xcodeproj` and no XCTest.
  - Swift Testing is present at `/Library/Developer/CommandLineTools/Library/Developer/Frameworks/Testing.framework`, and it works with the extra `-F` and rpath flags in §7. This was verified on 2026-10-15.
  - The default SwiftPM build system (swift-build) fails under the CLT with "Could not initialize build system … Unknown error parsing property list". `--build-system native` works, though it prints a deprecation warning. This was also verified on 2026-10-15.
- No panic may cross the FFI. Every exported function returns `Result<_, VizError>`, which UniFFI maps to a Swift `throws`.
- Rust owns all state (parent invariant 1). Swift holds read-only snapshot records and changes things only by calling command functions (invariant 3).
- `core-ffi` is platform-neutral except for constructing the macOS backends, which is `#[cfg(target_os = "macos")]`. A Linux UI could reuse the same API through UniFFI's Kotlin/Python bindings or a C ABI later.

### Crates and tools (verified 2026-10-15)

| Item | Version | Purpose |
|---|---|---|
| `uniffi` | 0.32.2 | Proc-macro bindings (`#[uniffi::export]`, `uniffi::Record/Enum/Object/Error`); the `cli` feature only in `crates/uniffi-bindgen` |
| `thiserror` | 2 | `VizError` |
| `core-model`, `core-engine` (features `fixtures`), `platform-macos` | path | The stack below |
| SwiftPM | tools-version 6.0 | `app/Package.swift` |
| Swift Testing | toolchain | `app/Tests` |

UniFFI is preferred over `swift-bridge` (0.1.59):
- it generates Swift for errors, records, enums and objects with `throws` built in;
- its object model (`Arc<T>` handles) fits a long-lived `Session`;
- it uses one IDL-free proc-macro path;
- it can also target Kotlin and Python, which helps the Linux-port constraint;
- it is maintained by Mozilla with a stable release cadence.

`swift-bridge` has a nicer zero-copy story, but it is pre-1.0 and Swift-only.

## 3. FFI API (`core-ffi`)

```rust
#[derive(uniffi::Error, thiserror::Error, Debug)]
pub enum VizError {
    NotFound { path: String }, Model { message: String }, Io { message: String },
    Engine { message: String }, NoDevice { message: String }, Internal { message: String },
}

#[derive(uniffi::Object)] pub struct Session { inner: Mutex<SessionInner> }   // one per open document/window
#[uniffi::export] impl Session {
    #[uniffi::constructor] fn new_empty() -> Result<Arc<Self>, VizError>;
    #[uniffi::constructor] fn new_demo() -> Result<Arc<Self>, VizError>;     // core_engine::fixtures::demo_document
    #[uniffi::constructor] fn open(path: String) -> Result<Arc<Self>, VizError>;
    fn save(&self) -> Result<(), VizError>;                 // to current path; error if untitled
    fn save_as(&self, path: String) -> Result<(), VizError>; // creates/overwrites .vizdaw package
    fn path(&self) -> Result<Option<String>, VizError>;
    fn snapshot(&self) -> Result<DocSnapshot, VizError>;
    fn lanes(&self, seq: u64) -> Result<Vec<LaneRow>, VizError>;
    // commands (each = one undo step through 2a History)
    fn rename_sequence(&self, seq: u64, name: String) -> Result<DocSnapshot, VizError>;
    fn set_lane_mute(&self, seq: u64, lane: u64, mute: bool) -> Result<DocSnapshot, VizError>;
    fn undo(&self) -> Result<DocSnapshot, VizError>;
    fn redo(&self) -> Result<DocSnapshot, VizError>;
    // transport
    fn play(&self, seq: u64) -> Result<(), VizError>;
    fn stop(&self) -> Result<(), VizError>;
    fn locate(&self, tick: i64) -> Result<(), VizError>;
    fn set_loop(&self, on: bool) -> Result<(), VizError>;
    fn transport(&self) -> Result<TransportState, VizError>;
}
```

### Records
- **`DocSnapshot`:** `{ version: u64, title: String, dirty: bool, undo_label: Option<String>, redo_label: Option<String>, sequences: Vec<SeqRow>, warnings: Vec<String> }`.
  - `version` increases with every applied command, undo and redo, and is never saved.
- **`SeqRow`:** `{ id: u64, name: String, use_count: u32, top_level: bool, length_ticks: i64, lane_count: u32, comment: String }`.
- **`LaneRow`:** `{ id: u64, name: String, destination: Option<String>, midi_event_count: u32, region_count: u32, reference_count: u32, mute: bool, solo: bool, looped: bool, take_count: u32, active_take: String, is_drum: bool }`.
- **`TransportState`:** `{ playing: bool, seq: Option<u64>, tick: i64, position_bbu: String /* "5.2.480" */, seconds: f64, sample_rate: f64, device: String, faults: Vec<String>, report: Vec<String> }`.
  - `report` carries the last `CompileReport`'s messages, such as unroutable events.
- IDs cross the FFI as `u64`, using the integers that 2a serializes. `core-ffi` turns them back into typed IDs and rejects unknown ones with `Model`.

### Behaviour
- **Session:** `Session` holds a 2a `History` (with its Document), the current path, the last-saved version (for `dirty`), and an optional engine lease.
- **Engine host:**
  - A process-wide `EngineHost`, created lazily on the first `play`, owns the 2b `Engine`, the `platform-macos` HAL backend, the DLS synth and the CoreMIDI client.
  - Only one session plays at a time. A `play` from another session stops the first one.
  - If no output device exists, `play` throws `NoDevice`. Everything else keeps working.
  - With `VIZDAW_AUDIO=offline` in the environment, the host uses 2b's `OfflineBackend`, driven by a timer thread in real time. Tests use this to run without hardware.
- **Commands:** every command goes through `History::apply`. If the session is playing, the session then calls `Engine::load` with the new document so playback follows edits. Each command returns a fresh `DocSnapshot`, so Swift needs no extra round trip.
- **Panic guard:**
  - Each exported body runs inside `ffi_guard(|| …)`, which is `catch_unwind(AssertUnwindSafe(..))`. A caught panic becomes `VizError::Internal { message }` and is logged to stderr.
  - The workspace `profile.release` keeps `panic = "unwind"`.
- **Poisoned mutex:** recovered with `into_inner()`. 2a documents are immutable values replaced in one assignment, so a panic mid-command cannot leave a half-edited document.
- **Threading:**
  - All `Session` methods are synchronous and short: compile < 10 ms (2b), and the snapshot is O(sequences).
  - Swift calls them from the main actor.
  - `open`/`save_as` on very large projects may block briefly; asynchronous variants are deferred.

### Errors
`ModelError` maps to `Model` (`Display` text), except that I/O maps to `Io` and a missing package maps to `NotFound`. `EngineError` and `BackendError` map to `Engine`, except that `NoDevice` maps to `NoDevice`.

## 4. App package (`app/`)

```
app/
  Package.swift                 // swift-tools-version:6.0, platforms [.macOS(.v14)]
  Sources/
    vizdaw_ffiFFI/              // C target: include/vizdaw_ffiFFI.h + include/module.modulemap (generated), shim.c
    VizDAWCore/                 // generated vizdaw_ffi.swift (gitignored); Swift 5 language mode
    VizDAWAppModel/             // view models, formatting, no SwiftUI views; Swift 6 mode
    VizDAW/                     // @main App, views, menus; Swift 6 mode
  Tests/VizDAWAppModelTests/    // Swift Testing
```

### Linking
- `VizDAWCore` has `linkerSettings`:
  - `.unsafeFlags(["-L\(Context.packageDirectory)/../target/ffi"])`;
  - `.linkedLibrary("vizdaw_ffi")`;
  - `.linkedFramework` for CoreAudio, AudioToolbox, CoreMIDI, CoreFoundation and Foundation.
- `unsafeFlags` is allowed because `app` is a root package and is never consumed as a dependency.
- The C target is named `vizdaw_ffiFFI` because the generated Swift does `import vizdaw_ffiFFI`.

### UI (functional only, parent §9 subset)
- **Window:** `WindowGroup` with one document per window. Each window's `DocumentModel` owns one `Session`.
- **Menus:**
  - **File:**
    - **New:** an empty session in a new window.
    - **Open…:** an `NSOpenPanel` that lets you choose directories whose extension is `vizdaw`.
    - **Create Demo Project:** a new window with `Session.new_demo()`.
    - **Save** and **Save As…:** an `NSSavePanel` that appends `.vizdaw`.
    - **Close.**
  - **Edit:** **Undo**/**Redo**, labelled from `undo_label`/`redo_label`, replacing the system Undo/Redo group.
  - **Transport:** **Play/Stop** (Space), **Loop** toggle, **Return to Start** (Return).
- **Layout:** `NavigationSplitView`.
  - **Sidebar:** sequences in two sections, "Top-level" and "Used as reference". Each row shows the name and a use-count badge. Double-click renames, which goes through the `rename_sequence` command.
  - **Detail:** a read-only `Table` of the lanes in the selected sequence. Columns: Name, Destination, MIDI events, Regions, References, Takes (active), Loop, Drum. The Mute column holds a toggle that calls `set_lane_mute`.
- **Toolbar:**
  - Play/Stop button, Loop toggle;
  - position display: bar·beat·unit, plus mm:ss.fff;
  - device and sample-rate label;
  - a fault indicator: a red dot whose popover lists faults and compile-report messages.
- **Polling:** while a window is key, a `@MainActor` task polls `transport()` every 33 ms. When stopped it polls every 250 ms to catch `ended`.
- **Errors:** every thrown `VizError` becomes an alert with its message. A failed `play` leaves the UI usable.
- **Running without a bundle:** under `swift run` there is no bundle. The app delegate calls `NSApp.setActivationPolicy(.regular)` and `activate()` so the window comes to the front with a menu bar.

### View-model layer (`VizDAWAppModel`)
- **`DocumentModel` (`@Observable @MainActor`):**
  - Holds `snapshot: DocSnapshot`, `selectedSeq: UInt64?`, `lanes: [LaneRow]`, `transport: TransportState`, `alert: AlertItem?`.
  - Its intents are `open`, `save`, `saveAs`, `select`, `rename`, `toggleMute`, `undo`, `redo`, `playStop`, `toggleLoop` and `poll()`.
  - It depends on UniFFI's generated `SessionProtocol`, not the concrete class, so tests can inject a fake.
- **Rules:**
  - After any snapshot, if the selected sequence no longer exists, select the first top-level sequence.
  - Lanes are refetched only when `snapshot.version` or the selection changes.
  - The window title is `title` plus " — Edited" when `dirty` is set.
- `Formatting` holds pure helpers for position and time strings.

## 5. Build scripts

### `scripts/build-ffi.sh [--debug]`
1. `cargo build -p core-ffi --release`. The crate type is `["staticlib", "cdylib", "lib"]`.
2. `cargo run -p uniffi-bindgen -- generate --library target/release/libvizdaw_ffi.dylib --language swift --out-dir target/ffi/gen`.
3. Copy `libvizdaw_ffi.a` to `target/ffi/`.
4. Place the generated files:
   - `vizdaw_ffi.swift` goes to `app/Sources/VizDAWCore/`;
   - `vizdaw_ffiFFI.h` goes to `app/Sources/vizdaw_ffiFFI/include/`;
   - `vizdaw_ffiFFI.modulemap` goes to `app/Sources/vizdaw_ffiFFI/include/module.modulemap`.
5. Error out if they are missing. All generated files are gitignored.
- The script is idempotent and safe to run repeatedly. It uses `set -euo pipefail` and prints the next command.
- Bindings are generated from the dylib and the app links the static library. The dylib is used only as a metadata source.

### `scripts/swift.sh <build|run|test> [args]`
Runs `swift <cmd> --package-path app --build-system native` with these flags added:
- `-Xswiftc -F$CLT/Library/Developer/Frameworks`
- `-Xlinker -F…`
- `-Xlinker -rpath -Xlinker $CLT/Library/Developer/Frameworks`
- `-Xlinker -rpath -Xlinker $CLT/Library/Developer/usr/lib`

`CLT` is `xcode-select -p`. These flags are needed only for `test`. They are harmless for build and run.

### `scripts/make-app.sh`
1. `build-ffi.sh`.
2. `swift.sh build -c release`.
3. Assemble `dist/VizDAW.app`:
   - `Contents/MacOS/VizDAW`;
   - `Contents/Info.plist` with `CFBundleIdentifier org.vizdaw.VizDAW`, `LSMinimumSystemVersion 14.0`, `NSHighResolutionCapable`, a `CFBundleDocumentTypes` entry, and `UTExportedTypeDeclarations` for `org.vizdaw.project` (conforms to `com.apple.package`, extension `vizdaw`).
4. `codesign --force -s - --deep`, which is an ad-hoc signature.

Packaging limits without Xcode:
- No Developer ID signing, notarization, asset catalog or app icon compilation (`actool`), and no entitlements beyond ad-hoc.
- A locally built app runs. A copy downloaded elsewhere would be quarantined by Gatekeeper.
- Under `swift run`, Finder double-click and "Open With" do not work, because there is no registered UTI. They work only for the assembled `.app`.

## 6. Demo project (built by `core_engine::fixtures::demo_document`)

- **Destinations:** "Built-in Piano" (`builtin:synth`, ch 1), "Built-in Bass" (`builtin:synth`, ch 2), "Built-in Drums" (`builtin:synth`, ch 10).
- **Sequences:**
  - **"Beat":** 1 bar of 4/4 with one drum lane. Rows: kick 36, snare 38, closed hat 42 on 1/8 notes.
  - **"Groove":** 4 bars.
    - Lane "Drums" holds a reference to Beat with `loop_count` 4.
    - Lane "Bass" holds a 4-bar bass line with a program change to 33 (Fingered Bass) at tick 0.
  - **"Song" (top-level):** 120 BPM, 4/4, 12 bars.
    - Lane "Groove" holds a reference to Groove at bar 1. A second reference to Groove at bar 5 is transposed +5 and gets a bass variation.
    - Lane "Chords" holds a program change 0 (Acoustic Grand) at tick 0, then whole-note chords in bars 1–12, including 7-bit CC 7 and pitch-bend events to exercise translation.
    - The final 2 bars carry a linear tempo ramp from 120 to 90 BPM.
- The nesting goes Song → Groove → Beat, three levels. Beat's use count is 1, through Groove, and Groove's use count is 2.
- The demo is written to a checked-in fixture through `vizdaw-play --write-demo` (2b §10).

## 7. Testing

### Rust (`cargo test -p core-ffi`; run with `VIZDAW_AUDIO=offline`)
- **Lifecycle:** `new_empty` and `new_demo` produce snapshots of the expected shape: the demo has 3 sequences, Song is the only top-level sequence, Groove's use count is 2, and Song has 2 lanes.
- **Persistence:** `save_as` then `open` round-trips to an equal snapshot (ignoring `version`). `save` while untitled gives an `Io` error.
- **Errors:**
  - `open` on a missing path gives `NotFound`; on corrupt JSON, `Model`; on a newer format, `Model` whose message mentions the version.
  - Unknown sequence and lane IDs give `Model`.
- **Commands:** `rename_sequence` → `undo` → `redo` produce the right names, labels and `dirty` state. `set_lane_mute` while playing (offline) triggers a recompile, and the change is visible in the offline event log.
- **Transport (offline backend):**
  - after `play(song)`, the position advances monotonically;
  - `stop` holds the position;
  - `locate` works;
  - the loop wraps;
  - playing in a second session stops the first.
- **Panic guard:** a `#[cfg(feature = "test-hooks")]` export, `debug_panic()`, returns `Internal` and does not abort. A poisoned mutex is recovered.
- **Bindings check:** `build-ffi.sh` followed by `swift.sh build` succeeds. This runs in CI on the macOS runner.

### Swift (`scripts/swift.sh test`, Swift Testing)
- **`DocumentModel` with a `FakeSession: SessionProtocol`:**
  - sidebar sections are split by `top_level`;
  - selection survives a snapshot that keeps the sequence and falls back when the sequence is deleted;
  - lanes are refetched only when the version changes;
  - undo and redo menu labels;
  - a thrown `VizError.NoDevice` from `play` becomes an alert, and the state stays "stopped";
  - the dirty title.
- **`Formatting`:** BBU and time strings, including negative and pickup positions.
- **Integration test (real FFI, `VIZDAW_AUDIO=offline` set in the test):** `Session.newDemo()` → `DocumentModel` shows 3 sequences; play, wait 200 ms, poll, and the tick is > 0.

### Manual smoke test (`app/README.md`)
1. Run `scripts/build-ffi.sh && scripts/swift.sh run VizDAW`. A window appears with a menu bar.
2. **File > Create Demo Project:** the sidebar shows Song (top-level) and Groove and Beat (references, with use counts 2 and 1).
3. Select Song and check the lanes Groove and Chords. Press Space: the music sounds through the speakers, the position advances, and the tempo slows over the last 2 bars. Playback stops at the end.
4. Toggle Mute on Chords during playback: the chords drop out within one buffer, and Undo restores them.
5. Rename Beat, then Save As `~/Desktop/demo.vizdaw`, Close, and Open it again: the name is kept.
6. Run `scripts/make-app.sh`, then `open dist/VizDAW.app`. Double-clicking the saved `.vizdaw` package in Finder opens it.

## 8. CI and workspace changes

- **Workspace members:** add `crates/core-ffi` and `crates/uniffi-bindgen`. Add `target/ffi/`, `app/.build/` and the generated sources to `.gitignore`.
- **macOS CI job:**
  1. `cargo test --workspace` (with `VIZDAW_AUDIO=offline`) and `cargo clippy -D warnings`;
  2. `scripts/build-ffi.sh`;
  3. `scripts/swift.sh build`;
  4. `scripts/swift.sh test`.
- Running Swift tests in CI depends on the runner. GitHub's macOS images ship full Xcode, so there the script uses the default build system and the Xcode toolchain whenever `xcodebuild -version` succeeds.
- This replaces the parent §12 plan of `xcodebuild test` and XCUITest for now; see Rulings.

## Rulings

- **Ruling:** Use UniFFI 0.32 in proc-macro mode, not swift-bridge — why: error, record and object generation with Swift `throws` is built in; it is multi-language for the Linux path; and it is the parent spec's stated choice — cost if wrong: generated code is less ergonomic than hand-written Swift, and each call copies data. Both are tolerable for snapshot-sized data. Switching later would rewrite only `core-ffi` and `VizDAWCore`.
- **Ruling:** No xcframework. Link `libvizdaw_ffi.a` through a SwiftPM C target plus `-L` `unsafeFlags` — why: `xcodebuild -create-xcframework` needs full Xcode, which is not installed, and an Apple-Silicon-only target needs one slice — cost if wrong: the package cannot be consumed as a remote dependency (unsafeFlags), which is irrelevant for an app root package. Moving to an xcframework later is a script change.
- **Ruling:** Generate bindings from the cdylib in `--library` mode and link the staticlib — why: library mode reads UniFFI metadata reliably from a dylib, and static linking gives a self-contained executable — cost if wrong: building both crate types makes the build slightly slower.
- **Ruling:** Build with `--build-system native` via `scripts/swift.sh` — why: SwiftPM 6.4's default swift-build backend fails to initialize under the Command Line Tools (verified) — cost if wrong: native is deprecated and may be removed in a future SwiftPM. Then we would need full Xcode, or a fixed CLT that makes the default work. The script is a single place to change.
- **Ruling:** Use Swift Testing (`import Testing`) with explicit `-F` and rpath flags, not XCTest — why: XCTest is not shipped with the CLT, and Swift Testing is, at a non-default path (verified working) — cost if wrong: the flags break on a CLT update. One script to fix.
- **Ruling:** The generated bindings compile in Swift 5 language mode, and the app's own targets in Swift 6 — why: UniFFI's generated code is not guaranteed to be clean under Swift 6 strict concurrency, and isolating it keeps the app strict — cost if wrong: Sendable warnings at the boundary. The view models are `@MainActor`, so the session is used from one actor.
- **Ruling:** `WindowGroup` with an app-managed document per window, not `DocumentGroup` — why: Rust owns saving (an atomic package write per 2a §7), and `DocumentGroup`'s FileWrapper model would write the package itself; `DocumentGroup` also needs Info.plist UTIs, which `swift run` cannot provide — cost if wrong: we hand-roll recent documents, dirty-close prompts and window restoration. Recents and close prompts are deferred to UI polish (sub-project 7).
- **Ruling:** The UI polls `transport()` (33 ms while playing) instead of using a Rust → Swift callback interface — why: the engine publishes atomics anyway; polling avoids reentrancy and thread-hopping callbacks, and it is trivially testable — cost if wrong: about 30 cheap FFI calls a second; future change notifications for collaborative or background edits will need a callback interface.
- **Ruling:** Commands return a whole new `DocSnapshot` (sequence-level) and lanes are fetched per selected sequence, not diffs — why: snapshots are small at this level of detail, and diffs belong to sub-project 4's editors — cost if wrong: editors will need finer-grained, versioned queries. That adds API; nothing here has to be removed.
- **Ruling:** 2c exposes only rename-sequence, lane-mute, undo and redo as commands — why: that is enough to prove the commands → history → snapshot → engine-recompile loop without designing editor APIs early — cost if wrong: none; more commands are additive.
- **Ruling:** One process-wide `EngineHost`, created lazily on the first Play, plays one session at a time — why: there is one output device and one DLS synth, and lazy creation means a machine without audio can still open and inspect projects — cost if wrong: you cannot audition two documents at once. That is not a parent-spec requirement.
- **Ruling:** `VIZDAW_AUDIO=offline` selects 2b's `OfflineBackend` in real time — why: FFI and Swift tests can then exercise the transport without hardware or audio permissions — cost if wrong: an environment variable leaking into user launches would silence the app. The device label then reads "Offline", so it is visible.
- **Ruling:** Panics are caught in an explicit `ffi_guard` on every export, as well as by UniFFI's own handling, and mutex poisoning is recovered with `into_inner` — why: parent §10 requires that no panic crosses the FFI and that the app survives; immutable 2a documents make the recovery state-safe — cost if wrong: a panic in engine-host setup could leave the host half-initialized. The host is reset to `None` on any `Internal` error from transport calls.
- **Ruling:** "Create Demo Project" opens a new untitled window with no undo history, rather than adding the demo to the current document — why: it is a starting point, not an edit, and the same builder serves 2b tests — cost if wrong: none significant.
- **Ruling:** Open accepts any directory with the `.vizdaw` extension through `NSOpenPanel` (directories enabled, validated by extension) — why: without a registered UTI under `swift run`, the package appears as a plain folder — cost if wrong: users can select a folder that is not a VizDAW project. `open` then reports `NotFound` or `Model` cleanly.
- **Ruling:** The bundle id is `org.vizdaw.VizDAW` and the UTI is `org.vizdaw.project` — why: these are neutral identifiers until a domain is settled (parent §3 names the repo only) — cost if wrong: renaming the UTI later breaks file associations for early testers only.
- **Ruling:** Parent §12's `xcodebuild test`/XCUITest is replaced by Swift Testing plus a manual smoke test for 2c — why: neither is available without Xcode — cost if wrong: no automated UI coverage until the UI polish sub-project. View models carry the logic so tests still cover most behaviour.
