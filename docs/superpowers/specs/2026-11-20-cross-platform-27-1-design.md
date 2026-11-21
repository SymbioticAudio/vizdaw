# 27.1.0: One Cross-Platform VizDAW (Design)

Status: approved approach in conversation on 2026-11-20 (owner). Written design awaiting owner review.
Parent spec: `2026-10-15-vizdaw-design.md` (§3 Platform, §9 Native macOS UI). This document replaces §9's SwiftUI/AppKit shell and §2's "Windows/Linux" non-goal.

## 1. Intent

The owner does not want to maintain two codebases. **(owner)** The SwiftUI app stays a build the owner uses for personal local testing; it is never released publicly. **(owner)** 27.1.0 is VizDAW's first public release, and it is one Rust codebase with one interface that runs on more than one platform. **(owner)**

The original design (2026-10-15) aimed for "a native modern Mac UI" and listed Windows and Linux as non-goals; it kept the core platform-neutral so a port could follow. Cross-platform is now the goal, and the Rust core (about 375k lines with tests) carries over unchanged; the Swift side to move is about 54k lines of code and 37k lines of tests.

## 2. Goals

1. **Parity.** 27.1.0 does everything the Swift app does at the time the rewrite starts, including VCAs and fader groups, takes, Kontrol MK3, Co-Producer and its approval sheet, the five themes, the customizable transport, torn-off editor windows, the Plugin Manager and all import/export sheets. **(owner: "I would need the whole thing for our first release")**
2. **One interface in GPUI** (https://gpui.rs/, Apache-2.0), all in Rust, calling the core directly. **(owner: "let's try with GPUI")**
3. **Platforms at 27.1.0: macOS and Linux.** (controller's call; owner did not object) Windows follows as soon as both are solid, on the same interface.
4. **Plug-in formats:** AU on macOS only (existing host in `platform-macos`); CLAP and VST3 on every platform. VST3 is new (backlog row "VST3 hosting"). (controller's call, from the backlog's existing 27.1.0 schedule)
5. **Linux audio and MIDI through PipeWire; plug-in windows on X11 and Wayland.** (owner, 2026-11-03, backlog row "Linux app")
6. **Song format compatibility:** a project saved by the Swift test build opens in 27.1.0 unchanged (format v11 at the time of writing; any bump follows the usual migration rules).

## 2a. Owner rulings (2026-11-20)

- **Linux may ship with known gaps, but each gap gets an answer.** **(owner)** Every parity row that does not pass on Linux is listed with its answer: the workaround Linux users get at 27.1.0, or the 27.x release that closes it. No gap is left as "unknown". The Mac has full parity. Known gaps today: the bundled Multi-Q default EQ is AU-only (answer: CLAP build, or a built-in EQ default on Linux); Co-Producer approvals are stored in the Mac Keychain (answer: the Secret Service password store on Linux); AU inserts in Mac projects (answer: placeholders, §10).
- **Screen-reader support moves to 27.2.** **(owner)** 27.1.0 keeps labels, roles and keyboard navigation wherever GPUI gives them (the control channel needs the labels anyway), but VoiceOver and Orca support is not a release requirement. Spike check 2 is reported, not a hard fail.

## 3. Non-goals

- A public Swift release (26.0.0 beta or 26.1.0). **(owner)**
- Swift-only polish. Fixes to the Swift app happen only when they block the owner's own testing. (controller's call, follows from the owner's ruling)
- Windows at 27.1.0, iOS, Android (backlog keeps their rows; GPUI is a prerequisite for Android).
- The rest of the backlog's 27.1.0 bucket (OMF, pro features, creative rows, engine edge cases): those move to 27.x point releases unless the owner pulls one in. (controller's call; the backlog already said "the owner may still split this")
- New features. The rewrite carries behaviour across; improvements go to the backlog.

## 4. Anti-goals (failure even if it "works")

- A 27.1.0 that drops a feature the Swift app has.
- A project that opens differently, or loses data, compared with the Swift build.
- Two interfaces maintained side by side at the end (the Swift app is retired once 27.1.0 ships).
- An interface that cannot be driven by the control channel, so the e2e suite and Co-Producer's UI tests are lost.
- Real-time audio work moving onto the interface thread, or the interface blocking on the engine.

## 5. Constraints

- Licence: VizDAW is GPL-3.0; GPUI (Apache-2.0) and the VST3 SDK (MIT since 3.8) are compatible. Keep their notices in the licences window.
- Naming rule: banned vendor and DAW names stay out of repo and UI text; licence notices are exempt.
- GPUI is pre-1.0 and changes often: pin an exact version (git rev or crates.io version) and upgrade deliberately.
- Rust stable, edition 2024, `cargo fmt --check` and clippy in CI on macOS and Linux runners.
- No new subagent GUI access: GUI checks are run by the controller or the owner.
- The `next` branch stays merge-forward from `main` (no rebase) while beta fixes still land for the owner's own testing.

## 6. Approaches considered

| Approach | Trade-offs | Verdict |
|---|---|---|
| **GPUI** (all Rust) | One language; draws dense custom views fast (arrangement, piano roll, meters). Pre-1.0, tied to its parent editor, screen-reader support unproven. | **Chosen (owner)**, behind the spike in §7. |
| Slint | Stable 1.x, good accessibility, GPL option. Built around forms; big custom-drawn views are harder, and those are most of VizDAW. | Rejected. |
| Tauri (web interface over the Rust core) | Most mature, best accessibility, browser canvas/WebGL. Brings back a second language (TypeScript); meters and playhead need care. | **Fallback** if the GPUI spike fails. |
| egui (all Rust, immediate mode) | Proven in a pure-Rust DAW on Mac, Linux, Windows and web; MIT/Apache; AccessKit built in. Owner re-confirmed GPUI on 2026-11-20 after seeing it. | Considered; candidate second fallback. |
| Keep SwiftUI on Mac, separate Linux UI (old approach A) | Least risk short term; two codebases forever. | Rejected (owner). |

## 7. Phase 0: the GPUI spike (decides everything)

A throwaway build, about a week, with fixed pass/fail checks on macOS and Linux:

1. **Plug-in window:** a plug-in's editor (AU on Mac, CLAP on both) opens in its own native window that is a child of / tied to a GPUI window (moves, minimizes and closes with it). Plug-in editors are separate windows today, so embedding inside GPUI's drawing is not required.
2. **Screen reader:** VoiceOver (Mac) reads a fader's name and value and a button's label, through GPUI's AccessKit support; Orca on Linux reads the same (Linux result is reported but not a hard fail).
3. **Control channel:** a `ui.click` and `ui.tree` from the control channel reach a GPUI control (through the accessibility tree or a GPUI-side element registry).
4. **Performance:** 64 track meters plus a moving playhead at 60 fps without frame drops on the owner's Mac; the engine's meter feed is read without locks on the interface thread.
5. **Text input and menus:** a text field, a native-feeling menu bar on Mac (app menu, shortcuts) and keyboard shortcuts routed to the focused pane.

Result goes in a short spike report. Any hard fail (1, 3 or 4) → stop and bring the Tauri fallback to the owner before going further.

## 8. Architecture (after the spike passes)

```
┌──────────── vizdaw (GPUI app crate) ────────────┐
│ windows, panes, editors, sheets, themes         │
├──────────── core-app (new crate) ───────────────┤
│ app model: documents, editor state, commands,   │
│ preferences, Co-Producer approval, close        │
│ prompts… (today's VizDAWAppModel, in Rust)      │
├──── existing core crates (unchanged) ───────────┤
│ core-model/engine/editor/io/media/host/surface/ │
│ control, vizdaw-mcp                             │
├──── platform layer ─────────────────────────────┤
│ platform-macos (CoreAudio/MIDI, AU)             │
│ platform-linux (new: PipeWire audio+MIDI,       │
│                 X11/Wayland plug-in windows)    │
│ core-host gains CLAP + VST3 on both             │
└─────────────────────────────────────────────────┘
```

- **core-app** holds what `VizDAWAppModel` holds today (about 29k lines of Swift, 34k lines of tests). It has no GPUI dependency, so it is tested headless like the Swift model is tested with `FakeSession`.
- **Migration order keeps the Swift app working:** core-app is exposed through the existing uniffi layer, and the Swift app switches to it piece by piece, so the owner keeps a working test build during the move and each piece is proven against the Swift tests before the Swift version is deleted. (controller's call, from the backlog's recorded first step)
- **The GPUI app** replaces `VizDAW` + `VizDAWEditors` (about 24k lines of Swift). Theming moves to a Rust theme table covering the five themes.
- **Control channel and MCP** keep their protocol; only the app-side handler (`ControlChannel.swift` today) is rewritten against GPUI.
- **Platform layer:** Linux gets a `platform-linux` crate mirroring `platform-macos`'s seams (audio device, MIDI, trash, permission, media import). Mac-only services that the Swift app does today through AppKit (file dialogs, Keychain for Co-Producer approvals, document packages) get a Rust seam with a Mac and a Linux implementation.

## 9. Phases

0. GPUI spike (§7).
1. core-app: move the app model into Rust with a Rust-native API that the GPUI app observes directly (no uniffi constraints). The Swift test build is not switched over to it: the owner keeps using it as it is. The Swift model and its tests are the reference: each Swift test is ported to a Rust test on core-app. (controller's call: routing through Swift would shape the API around a build that is never released and put months on the critical path)
2. GPUI app on macOS: windows, panes, editors, sheets, themes, control channel; e2e suite runs against it. Parity checklist (§10) drives it. Phases 1 and 2 run together feature by feature: a feature's model piece and its screens land together. (controller's call)
3. VST3 hosting (both platforms) and CLAP on Linux.
4. platform-linux: PipeWire audio and MIDI, plug-in windows (X11, Wayland), packaging.
5. Release: parity checklist passes on both platforms, owner's manual checks, retire the Swift app, packaging and signing.

Each phase gets its own implementation plan; phases 3 and 4 may overlap with 2.

## 10. Parity checklist

Built at the start of phase 2 from the Swift app's feature set: every menu command (stable IDs from `commands.list`), every window kind from `state.get windows`, every sheet, and the README manual-check steps. A feature is done when its row passes on the GPUI app. The existing e2e suite (44 tests) must pass against the GPUI app with the same commands, element labels and tree; pixel-level screenshots are not compared. (controller's call)

- **Baseline:** the Swift app at a tag cut when phase 2 starts (`swift-parity-baseline`). Fixes that reach the Swift app later get a checklist row only if they change behaviour; the controller adds those rows when merging `main` into `next`. (controller's call)
- **Who runs e2e:** the controller on the Mac; Linux e2e runs in CI under a headless display (Xvfb or a headless Wayland compositor). Subagents still get no GUI access. (controller's call)
- **Mac projects on Linux:** they open, and AU inserts show as missing plug-ins the way a disabled plug-in does today (placeholders keep their settings). No automatic AU-to-CLAP/VST3 swap. (controller's call)
- **Mac release prerequisites:** Developer ID signing and notarization need the owner's Developer ID, which the owner does not have yet (backlog row "Sign VizDAW…"). Phase 5 lists it as an owner dependency.

## 11. Left to the builder

- GPUI version to pin, and how menus and shortcuts map onto it.
- How core-app is split into modules.
- Packaging formats on Linux (Flatpak, AppImage, .deb).
- The order of features within phase 2.
- Whether PipeWire is used natively or through its JACK interface.
