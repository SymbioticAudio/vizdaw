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
`.vizdaw` opens that project; `--no-scan` skips the plugin scan at launch;
`--smoke` plays the demo for 3 s with the mixer open, puts Apple's AUDelay
on the first strip and opens its window, prints the transport, the mixer
and the editor size, and exits 0 when all of them worked. `--smoke` uses a
temporary support folder and never scans.

## Plugins

- The catalog lives in `~/Library/Application Support/VizDAW/plugins.json`
  (`VIZDAW_SUPPORT_DIR` moves the whole folder, crash markers included).
  At launch VizDAW scans new or changed AU and CLAP plugins in the
  background with `vizdaw-scan` (next to the app's executable in the
  bundle; `scripts/swift.sh` points `VIZDAW_SCANNER` at
  `target/ffi/vizdaw-scan`).
- Window > Plugin Manager (⌥⌘P) lists every plugin with its status;
  disable one that crashes VizDAW there. Window > Mixer (⌥⌘M) shows the
  focused document's mixer.
- The built-in synth plays through its strip's constant-power pan, so it
  is 3 dB quieter than in 2c (3a ruling).
- Muting a strip also silences its pre-fader sends (plan 3c ruling).

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

### Plugins (plan 3c)

Use `dist/VizDAW.app` from step 8 and your own plugins (for example
Diva, a CLAP effect, a reverb, a compressor with a sidechain input and a
multi-output drum instrument).

9. Launch: within a minute the scan finishes (Window > Plugin Manager
   shows "Scanning n of m…" until then). Every installed plugin is listed
   with name, vendor, format, version, type and status. The search field
   and the AU/CLAP filter narrow the table.
10. Open the demo, then Window > Mixer: strips "Built-in Synth" and
    "Master", each with a fader, pan, M/S, an output menu and a meter that
    moves while playing. The toolbar shows the DSP load.
11. Mixer toolbar > Instruments > "Built-in Piano: MIDI → Plugin…": the
    browser lists instruments grouped by vendor; pick Diva. A "Built-in
    Piano" instrument strip appears; its slot turns green once loaded.
    Play Song: the chords sound through Diva. (Instruments > New Plugin
    Instrument… adds a destination of its own.) Double-click the slot: a window
    "Diva — Diva" shows Diva's own editor. Turning a knob in it and then
    Edit > Undo returns the knob (one undo step per drag). Closing the
    window does not remove the plugin.
12. On a strip, "+": add a CLAP effect and an AU effect. Drag the second
    above the first: the order changes. Bypass one from its slot menu
    and from its window's header. Remove one, then Undo: it comes back
    with the settings it had.
13. "Add Bus", rename it "Verb" (double-click its name), add a reverb to
    it, add a send from the Diva strip to Verb; drag the send level and
    toggle Pre: the reverb follows. Solo the Diva strip: the bus still
    sounds, other strips go dim and silent.
14. Sidechain: put a compressor with a sidechain input on a pad strip,
    choose the kick's strip in its Sidechain menu, play: the pad ducks on
    the kick.
15. Multi-output: with a multi-output instrument, the strip's name menu
    offers "Add Outputs"; add one: a new strip plays that output.
16. Tempo sync: an AU delay set to a note value follows the song's tempo
    (it slows over the last two bars of Song).
17. Plugin Manager: disable the CLAP effect. Close and reopen the
    project: its slot is dimmed "Disabled in Plugin Manager" and passes
    audio. Enable it again: the slot loads. "Rescan All" scans every
    plugin; a crashing plugin shows "Blocklisted: crashed (signal 11)" and
    "Clear Blocklist" scans it again.
18. Save, leave two plugin windows open, close the project and reopen
    it: both windows reopen.
19. Safe mode: open a project with a heavy plugin and quit VizDAW with
    `kill -9` while it loads (Activity Monitor > Force Quit works too).
    Reopen the project: "VizDAW quit unexpectedly while this project was
    open, while loading ‹plugin›" offers Open with Plugins Off / Open
    Normally / Cancel / Disable ‹plugin›. With plugins off, every slot is
    dimmed "Plugins off (safe mode)"; a slot's Load and a strip's "Load
    All" load them. Opening with Shift held asks the same question.
20. Autosave: edit a saved project, wait 2 minutes (or load a plugin),
    then `kill -9` VizDAW. Reopen: "has an autosave newer than its last
    save" offers Restore Autosave / Open Last Saved; restoring shows the
    edit and the title says Edited.
