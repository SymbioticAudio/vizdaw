# vizdaw-play

Plays a sequence of a VizDAW project through Apple's built-in DLS synth
(or the "VizDAW Out" virtual MIDI source).

```
vizdaw-play <project.vizdaw> [--seq NAME] [--loop] [--device NAME] [--buffer N] [--workers N]
            [--to builtin|virtual|both] [--midi-offset-ms N] [--offline OUT.txt] [--list]
vizdaw-play --write-demo <dir/demo.vizdaw>
```

Exit codes: 0 played to the end (or Ctrl-C), 1 error, 2 bad arguments or
no `--seq` when the project has several top-level sequences (they are
listed on stderr).

`--offline` renders at 48 kHz in 512-frame blocks without audio hardware
and writes one line per MIDI message: `frame tTARGET hex-bytes`. With
`--loop` it renders three passes and the tail.

Playback goes through the project's mixer: the built-in synth plays on its
"Built-in Synth" strip (inserts, fader, pan, sends) into master, rendered
by the audio thread plus `--workers N` worker threads (0–15; default:
performance cores − 1, at most 7; 0 for `--offline`). Workers run with
real-time priority in the output device's workgroup. The status line shows
the DSP load (render time ÷ block time, peak over 0.1 s).

## Manual smoke test

Run from the repository root with the toolchain environment exported
(`export PATH="$HOME/.cargo/bin:$PATH" DEVELOPER_DIR=/Library/Developer/CommandLineTools`).

1. `cargo run -p vizdaw-play --release -- fixtures/demo.vizdaw --seq Song`
   — drums, bass and chords are audible; bars 5–8 are a fourth higher (the
   drums stay as they are there: drum lanes are never transposed); the ritardando over the last two bars is
   audible; the process exits 0 about a second after the last chord.
2. Add `--loop`, let it wrap twice, press Ctrl-C — no hanging notes.
3. Add `--to virtual` and watch the events in MIDI Monitor (or
   `receivemidi dev "VizDAW Out"`).
4. While it plays, switch the output device in Control Center — playback
   continues on the new device or stops with a reported fault; it never
   crashes.
5. `cargo run -p vizdaw-play --release --features rt-check -- fixtures/demo.vizdaw`
   — the last line reads `rt-check: 0 allocation(s) on the render path`.
6. `cargo run -p vizdaw-play --release -- fixtures/demo.vizdaw --seq Song --workers 0`
   then the same with the default and with `--workers 7` — the music sounds
   the same each time (at −3 dB compared with 2b: the built-in strip's
   centred pan); the startup line names the worker count and "in the device
   workgroup"; the load figure stays well below 100 %.
7. While it plays with workers, switch the output device or its buffer size
   in Audio MIDI Setup — playback continues; no `workgroup-join-failed` or
   `thread-policy-failed` fault is printed.
8. `cargo run -p vizdaw-play --release --features rt-check -- fixtures/demo.vizdaw --workers 4`
   — the last line reads `rt-check: 0 allocation(s) on the render path`
   (workers arm themselves under `rt-check`).
