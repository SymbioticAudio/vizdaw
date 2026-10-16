# vizdaw-play

Plays a sequence of a VizDAW project through Apple's built-in DLS synth
(or the "VizDAW Out" virtual MIDI source).

```
vizdaw-play <project.vizdaw> [--seq NAME] [--loop] [--device NAME] [--buffer N]
            [--to builtin|virtual|both] [--midi-offset-ms N] [--offline OUT.txt] [--list]
vizdaw-play --write-demo <dir/demo.vizdaw>
```

Exit codes: 0 played to the end (or Ctrl-C), 1 error, 2 bad arguments or
no `--seq` when the project has several top-level sequences (they are
listed on stderr).

`--offline` renders at 48 kHz in 512-frame blocks without audio hardware
and writes one line per MIDI message: `frame tTARGET hex-bytes`. With
`--loop` it renders three passes and the tail.

## Manual smoke test

Run from the repository root with the toolchain environment exported
(`export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`).

1. `cargo run -p vizdaw-play --release -- fixtures/demo.vizdaw --seq Song`
   — drums, bass and chords are audible; bars 5–8 are a fourth higher (the
   drum kit becomes toms there); the ritardando over the last two bars is
   audible; the process exits 0 about a second after the last chord.
2. Add `--loop`, let it wrap twice, press Ctrl-C — no hanging notes.
3. Add `--to virtual` and watch the events in MIDI Monitor (or
   `receivemidi dev "VizDAW Out"`).
4. While it plays, switch the output device in Control Center — playback
   continues on the new device or stops with a reported fault; it never
   crashes.
5. `cargo run -p vizdaw-play --release --features rt-check -- fixtures/demo.vizdaw`
   — the last line reads `rt-check: 0 allocation(s) on the render path`.
