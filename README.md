# VizDAW

VizDAW (pronounced "Vista") is a MIDI-first, open-source digital audio
workstation for macOS, inspired by Opcode Studio Vision.

**Status:** design phase — see [the design spec](docs/superpowers/specs/2026-10-15-vizdaw-design.md).

- Sequences and subsequences as the arrangement model, with alternate takes
- List, piano-roll, Strip Chart and arrangement editors over one shared model
- MIDI 2.0 throughout; low-latency CoreAudio and CoreMIDI
- AU (v2/v3) and CLAP plugin hosting
- SMF, OMF and AAF import
- Rust core with a native SwiftUI/AppKit interface (Apple Silicon, macOS 14+)

## License

GPL-3.0 — see [LICENSE](LICENSE).

## Disclaimer

VizDAW is an independent project. It is not affiliated with or endorsed by
Opcode Systems, Gibson, or Apple. Studio Vision is a trademark of its
respective owner and is mentioned only to describe the project's inspiration.
