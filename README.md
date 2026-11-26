# VizDAW

VizDAW (pronounced "Vista") is a MIDI-first, open-source digital audio
workstation for macOS and Linux, inspired by the classic integrated
MIDI-and-audio sequencers of the 1990s.

**Status:** VizDAW is in development and not yet released. The first
release will be **27.1.0**: one app, written in Rust with the GPUI
interface toolkit, for macOS and Linux. It replaces an earlier SwiftUI app
for macOS, which was never released and is now retired. The new app is
being ported from it in stages ("slices").
- The [27.1.0 design](docs/superpowers/specs/2026-11-20-cross-platform-27-1-design.md)
  explains the plan.
- The [parity checklist](docs/superpowers/specs/2026-11-22-parity-checklist.md)
  tracks what has been ported on each platform.

## What works today

- **Projects:** new, open and save, the demo songs, Project Settings, and
  prompts to save on close and quit.
- **The arrangement:** tracks, regions and region waveforms, the track
  list, and undo and redo.
- **The transport:** play, loop, locate, markers and locators, the
  transport bar and its fields, and the click.
- **Editors:** the List editor and the piano roll in the bottom pane, with
  block editing and step entry.
- **Recording audio and MIDI:**
  - track inputs and monitoring, and input meters;
  - count-in, pre-roll and punch;
  - loop passes kept as takes;
  - the take drawn as it records.
- **The mixer,** in the Mix view or its own window: channel strips, buses,
  sends, fader groups and VCAs.
- **Plug-ins:** CLAP and VST3 on both platforms, and AU on macOS. A
  project's plug-ins play and open their editor windows (on Linux under X11
  or XWayland). Adding and moving inserts is still to come.
- **Bounce and interchange:**
  - Standard MIDI File import and export;
  - export for another DAW;
  - DAWproject import, export and merge;
  - AAF.
- **An agent control channel** (`vizctl`) that drives the app for tests and
  automation.
- **Settings has begun:** the Settings window, with its Audio, Recording
  and Automation tabs being built now.

## Not yet in the new app

These worked in the retired Swift app and are still to be ported:

- Themes and the rest of Settings. Six themes are planned: Classic Light,
  Studio Dark, Flat, Console, Symbiotic and EG13.
- Adding, moving and bypassing inserts, and the Plugin Manager.
- The Strip Chart, drum grid and audio editors, and editors in windows of
  their own.
- Choosing takes, and the Media Pool.
- Changeable key bindings, control surfaces and MIDI learn.
- Co-Producer, VizDAW's assistant for agents.

On Linux, live audio and MIDI input and output wait for the PipeWire and
ALSA work (host track H3); until then the Linux app runs on an offline
device. Importing WAV and AIFF files on Linux comes with host track H4.

## Platforms

- **macOS:** 14 or newer, on Arm-based Macs.
- **Linux:** an X11 or Wayland desktop. On Wayland, plug-in editors use
  XWayland. CI builds and tests on Ubuntu.

Windows may come later.

## Building from source

You need Rust 1.86 or newer.

**macOS** (with the Xcode Command Line Tools), from the project folder:

```bash
scripts/make-gpui-app.sh release   # builds target/VizDAW-GPUI.app
open target/VizDAW-GPUI.app
```

`scripts/make-gpui-app.sh` with no argument makes a debug build.

**Linux:** first install the libraries the interface needs. On Ubuntu:

```bash
sudo apt-get install libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev \
  libxcb1-dev libvulkan-dev libfontconfig-dev libfreetype-dev
VIZDAW_AUDIO=offline cargo run -p vizdaw-gpui --release
```

`VIZDAW_AUDIO=offline` is needed until host track H3 adds live audio.

**Tests**, on either platform:

```bash
cargo test --workspace
```

The end-to-end tests drive a built app through the control channel; see
`crates/vizctl/tests/e2e_gpui.rs` for how to run them.

[`docs/testing/manual-checks.md`](docs/testing/manual-checks.md) lists the
checks that still need a person. The retired Swift app's sources, and its
notes in [app/README.md](app/README.md), stay in the tree for reference
while the port goes on.

## License

GPL-3.0-only — see [LICENSE](LICENSE). VizDAW bundles Dusk Audio Multi-Q,
which includes parts under the GNU AGPLv3 (JUCE).

VizDAW's own build links SymbioticAudio's branded icons under a GPL section
7 additional permission (see LICENSE). The artwork stays all rights reserved
and is not covered by the GPL.

Release builds will carry Dusk Audio Multi-Q (GPL-3.0-only), built with JUCE
(AGPLv3) and clap-juce-extensions (MIT), with their licence texts and the
source offer. They will also carry the notices of the other third-party code
VizDAW uses, written by `scripts/third-party-notices.sh`.

## Disclaimer

VizDAW is an independent project. It is not affiliated with or endorsed by
any other software or hardware company. Any product names it mentions are
trademarks of their respective owners.
