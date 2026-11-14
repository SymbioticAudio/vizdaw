# VizDAW

VizDAW (pronounced "Vista") is a MIDI-first, open-source digital audio
workstation for macOS, inspired by the classic integrated MIDI-and-audio
sequencers of the 1990s.

**Status:** VizDAW is in development and not yet released. Today it
records, edits and mixes MIDI and audio in one window. The
[design spec](docs/superpowers/specs/2026-10-15-vizdaw-design.md) describes
where the project is going.

## What works today

- Sequences, subsequences and alternate takes
- The arrangement, piano roll, list, Strip Chart and drum grid editors,
  with step entry and changeable key bindings
- Audio and MIDI recording, with monitoring and count-in
- Audio import, regions, fades and normalize
- The mixer, with inserts, buses, sends and automation
- AU (v2/v3) and CLAP plug-ins (no VST3)
- Control surfaces (Mackie Control with its XT extenders, HUI, and one
  family of keyboard controllers in its host-integration mode: plug-in pages,
  transport) and
  MIDI learn
- Bounce to WAV, AIFF or CAF
- Round trips to an external sample editor
- MIDI 2.0 throughout, on Arm-based Macs with macOS 14 or newer
- Six themes, Classic Light by default, and a channel strip with typed
  levels, pan and sends, send knobs, an EQ slot and an optional colour per
  strip
- An audio editor in the bottom pane: a region's waveform drawn large with
  zoom and a level scale, and its gain, fades and fade shapes, trim and
  normalize, each by dragging or by typing a value

Planned: Standard MIDI File import, and OMF and AAF import (which may come
after the first release).

## Getting around

- Each project has one window. The transport bar along its top has Return
  to Start, Rewind and Fast Forward (click for a bar, hold to wind,
  ⇧-click for a marker), Play, Loop and Record, bar, time, tempo and signature
  readouts, and the transport fields.
- Two main views: Arrange (⌃1) and Mix (⌃2). Switching keeps each view's
  panes as you left them.
- Three panes, left (⌃⌘←), bottom (⌃⌘↓) and right (⌃⌘→), each with a
  picker for what it shows. Drag the lines between them to resize them.
- ⌘1 to ⌘4 open the piano roll, list, Strip Chart and drum grid in the
  bottom pane.
- ⌘5 opens the audio editor for the selected audio region.
- Window > Mixer opens the mixer in a window of its own.
- Track menu: new audio, instrument and MIDI tracks, duplicate, delete and rename.
- Change any key in Settings › Keys.

## Themes

VizDAW has six themes: Classic Light (the default), Studio Dark, Flat,
Console, Symbiotic and EG13. Choose one in Settings › Appearance; it
changes every window at once, the mixer included, and it stays chosen on
this Mac rather than in your projects. In every theme the mixer colours its
fader caps by strip type (channel, instrument, aux return, bus, master) and
its knobs by what they do, like a mixing console. Only Console draws a
hardware look, in code, with no photographs.

You can make your own themes as JSON files in
`~/Library/Application Support/VizDAW/Themes`. Settings › Appearance ›
Duplicate as File… writes the current theme there as a starting point, and
Show Themes Folder opens the folder. A file needs a "name" no built-in
theme has and a "base", one of classic-light, studio-dark, flat, console,
symbiotic or eg13; any token it leaves out or gets wrong comes from the
base, and Settings › Appearance lists each problem. VizDAW reads the files
again as soon as they change.

## Platforms

VizDAW runs on macOS 14 or newer on Arm-based Macs. Its core is
platform-neutral Rust, and it builds and passes its tests on Linux in CI.
A Linux app, with its own Rust interface and PipeWire audio, is planned
after 1.0. A later 2.0 may bring one interface for every platform, and
Windows.

## Building from source

You need macOS 14 or newer, Rust 1.86 or newer, and the Xcode Command Line
Tools. Then, from the project folder:

```bash
scripts/build-ffi.sh              # Rust static library + generated Swift bindings
scripts/swift.sh run VizDAW       # opens a window (add --demo to start with the demo)
scripts/swift.sh test             # Swift Testing suite (needs build-ffi.sh first)
scripts/make-app.sh               # dist/VizDAW.app, ad-hoc signed
```

The first build fetches symbiotic-design 0.2.2 from GitHub; `VIZDAW_NO_BRAND=1` builds without SymbioticAudio's branded icons.

Building on Linux: the core builds and tests with `cargo test --workspace`
(there is no Linux app yet).

See [app/README.md](app/README.md) for more.

## License

GPL-3.0 — see [LICENSE](LICENSE).

VizDAW's own build links SymbioticAudio's branded icons under a GPL section 7 additional permission (see LICENSE). The artwork stays all rights reserved and is not covered by the GPL; forks build without it with `VIZDAW_NO_BRAND=1`.

The VizDAW.app bundle also carries Dusk Audio Multi-Q (GPL-3.0-only), built with JUCE (AGPLv3) and clap-juce-extensions (MIT); their licence texts and the source offer are in Contents/Resources/Licences/Multi-Q/.

## Disclaimer

VizDAW is an independent project. It is not affiliated with or endorsed by
any other software or hardware company. Any product names it mentions are
trademarks of their respective owners.
