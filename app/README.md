# VizDAW app (SwiftUI shell)

A minimal macOS 14+ app over the Rust core (`crates/core-ffi`, UniFFI).
It builds with the Command Line Tools alone; no Xcode project.

## Build and run

```bash
scripts/build-ffi.sh              # Rust static library + generated Swift bindings
scripts/swift.sh run VizDAW       # opens a window (add --demo to start with the demo)
scripts/swift.sh test             # Swift Testing suite (needs build-ffi.sh first)
scripts/make-app.sh               # dist/VizDAW.app, ad-hoc signed
```

`scripts/swift.sh` adds `--build-system native` and the Swift Testing
framework paths that the Command Line Tools need. With full Xcode installed
it runs plain `swift`.

Launch arguments: `--demo` opens the demo project; a path ending in
`.vizdaw` opens that project; `--smoke` plays the demo for 3 s, prints the
transport and exits 0 when the position advanced.

`VIZDAW_AUDIO=offline` renders through the offline backend in real time
(no sound, device label "Offline"). Never set it for normal use.

Generated files (not in git): `app/Sources/VizDAWCore/vizdaw_ffi.swift`,
`app/Sources/vizdaw_ffiFFI/include/`, `target/ffi/`, `dist/`.

## Manual smoke test

1. `scripts/build-ffi.sh && scripts/swift.sh run VizDAW`. A window appears
   in front with a menu bar (File, Edit, Transport).
2. File > Create Demo Project. A second window opens; the sidebar shows
   Song under "Top-level" and Beat (1) and Groove (2) under "Used as
   reference".
3. Song is selected; the table lists the lanes Groove and Chords. Press
   Space: the demo sounds through the speakers, the position display
   advances (bar.beat.unit and m:ss.fff), the tempo slows over the last two
   bars, and playback stops by itself at the end.
4. Press Space again and, while it plays, switch Mute on for Chords: the
   chords drop out within a moment. Edit > Undo Set Lane Mute/Solo/Arm
   brings them back.
5. Double-click Beat in the sidebar, type `Kit`, press Return; the title
   gains " — Edited". Typing a space in the name field types a space and
   does not start playback.
6. File > Save As… `~/Desktop/demo.vizdaw`, close the window (Cmd-W),
   File > Open… and choose `~/Desktop/demo.vizdaw`: the sequence is still
   called Kit. Choosing a folder without `.vizdaw` shows an alert.
7. Unplug or switch the output device during playback: within a moment
   playback continues on the new device from the same position (the UI may
   pause briefly); if no output device is left, an alert appears.
8. `scripts/make-app.sh`, then `open dist/VizDAW.app`; double-click
   `~/Desktop/demo.vizdaw` in Finder: it opens in VizDAW.
