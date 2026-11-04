# VizDAW

VizDAW (pronounced "Vista") is a MIDI-first, open-source digital audio
workstation for macOS, inspired by Opcode Studio Vision.

**Status:** VizDAW is in development and not yet released. Today it
records, edits and mixes MIDI and audio in one window. The look of the
interface (themes) and an audio editor are next. The
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
- Control surfaces (Mackie Control with its XT extenders, HUI, and Kontrol
  S-Series MK3 keyboards) and MIDI learn
- Bounce to WAV, AIFF or CAF
- Round trips to an external sample editor
- MIDI 2.0 throughout, on Apple Silicon with macOS 14 or newer

Planned: Standard MIDI File import, and OMF and AAF import (which may come
after the first release).

## Getting around

- Each project has one window. The transport bar along its top has Play,
  Loop, Record, the position and the transport fields.
- Two main views: Arrange (⌃1) and Mix (⌃2). Switching keeps each view's
  panes as you left them.
- Three panes, left (⌃⌘←), bottom (⌃⌘↓) and right (⌃⌘→), each with a
  picker for what it shows. Drag the lines between them to resize them.
- ⌘1 to ⌘4 open the piano roll, list, Strip Chart and drum grid in the
  bottom pane.
- Window > Mixer opens the mixer in a window of its own.
- Change any key in Settings › Keys.

## Platforms

VizDAW runs on macOS 14 or newer on Apple Silicon. Its core is
platform-neutral Rust, and it builds and passes its tests on Linux in CI.
A Linux app, with its own Rust interface and PipeWire audio, is planned
after 1.0. A later 2.0 may bring one interface for every platform, and
Windows.

## Building from source

You need macOS 14 or newer, Rust 1.85 or newer, and the Xcode Command Line
Tools. Then, from the project folder:

```bash
scripts/build-ffi.sh              # Rust static library + generated Swift bindings
scripts/swift.sh run VizDAW       # opens a window (add --demo to start with the demo)
scripts/swift.sh test             # Swift Testing suite (needs build-ffi.sh first)
scripts/make-app.sh               # dist/VizDAW.app, ad-hoc signed
```

The first build fetches symbiotic-design 0.2.0 from GitHub; `VIZDAW_NO_BRAND=1` builds without SymbioticAudio's branded icons.

Building on Linux: the core builds and tests with `cargo test --workspace`
(there is no Linux app yet).

See [app/README.md](app/README.md) for more.

## License

GPL-3.0 — see [LICENSE](LICENSE).

VizDAW's own build links SymbioticAudio's branded icons under a GPL section 7 additional permission (see LICENSE). The artwork stays all rights reserved and is not covered by the GPL; forks build without it with `VIZDAW_NO_BRAND=1`.

## Disclaimer

VizDAW is an independent project. It is not affiliated with or endorsed by
Opcode Systems, Gibson, or Apple. Studio Vision is a trademark of its
respective owner and is mentioned only to describe the project's inspiration.
