# vizdaw-play

Plays a sequence of a VizDAW project through Apple's built-in DLS synth
(or the "VizDAW Out" virtual MIDI source).

```
vizdaw-play <project.vizdaw> [--seq NAME] [--loop] [--device NAME] [--buffer N] [--workers N]
            [--to builtin|virtual|both] [--midi-offset-ms N] [--offline OUT.txt] [--list]
vizdaw-play --write-demo <dir/demo.vizdaw>
vizdaw-play --scan [--catalog FILE] [--clap-dir DIR]… [--no-au] [--scan-timeout-ms MS] [--scanner FILE]
vizdaw-play --plugins [--catalog FILE]
vizdaw-play <project.vizdaw> [--insert "STRIP=au:ID|clap:ID"]… [--instrument "DEST=au:ID|clap:ID"]…
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

Plugins: `--scan` finds CLAP bundles (`~/Library/Audio/Plug-Ins/CLAP`,
`/Library/Audio/Plug-Ins/CLAP`) and Audio Units, checks each new or changed
one in its own `vizdaw-scan` process (a crash or a hang blocklists it) and
writes the catalog (`~/Library/Application Support/VizDAW/plugins.json`).
`--insert` adds a plugin to a strip by name and `--instrument` plays a
destination on a plugin instrument, for live playback; the project's own
plugin slots load too. AU ids are `type/subtype/manufacturer` four-char
codes (`--plugins` lists them).

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

## Manual plugin checklist (3b)

Uses plugins installed on this Mac; nothing here is automated. Build first:
`cargo build -p vizdaw-scan -p vizdaw-play --release`.

9. `cargo run -p vizdaw-play --release -- --scan` — lists every installed
   CLAP and AU with its status; a second `--scan` scans nothing new
   ("scanning 0 source(s)").
10. Pick an AU instrument from the list (e.g. Diva) and play the demo's
    Song on it: `cargo run -p vizdaw-play --release -- fixtures/demo.vizdaw
    --seq Song --instrument "Built-in Piano=au:<id>"` with the id
    `--plugins` prints — the instrument plays the piano part; the status
    line load stays sane; Ctrl-C leaves no hanging notes.
11. Add an AU and a CLAP effect on the demo's strips (the CLI can address
    "Built-in Synth" and "Master" only):
    `--insert "Built-in Synth=au:aufx/dely/appl" --insert "Master=clap:<id>"`
    — both audible; a plugin with latency (a lookahead limiter) keeps the
    drums in time with the piano.
12. Sidechain: with a third-party compressor that has a sidechain input,
    set one up in a project (3c's mixer, or a hand-edited project.json:
    `"sidechain": <strip id>` on the insert) — the kick ducks the pad.
13. A multi-output drum instrument: the CLI cannot add instrument-output
    strips (`AddInstrumentOutput`), so use a hand-edited project or 3c's
    mixer — each output plays on its strip.
14. Copy a known-crashing or hanging test plugin into
    `~/Library/Audio/Plug-Ins/CLAP` (the in-repo `target/test-plugins/VizDAW
    Test.clap`) and `--scan` — the crasher shows `Blocklisted: crashed
    (signal 11)`, the hanger `Blocklisted: timed out`; VizDAW never crashes.
    Remove it afterwards.
15. AU v3 parameters and undo: while automation or a mixer control moves a
    plugin parameter, no undo steps appear; dragging in the plugin's own UI
    still records gestures (one undo step per drag). Check stepped parameters
    on third-party AU v2 and v3 units too (a unit that quantises the host's
    value can echo it back as a user change, which would record an undo step
    or a gesture nobody made).
16. AU v3 instantiation timeout: with an AU v3 unit that is slow to start (or
    by loading one while the machine is busy), after the 30 s timeout is
    reported no orphan out-of-process instance remains (Activity Monitor: no
    leftover extension process for it).
17. AU sidechain (plan ruling 8): a third-party AU with a sidechain input
    (e.g. a compressor with an external key) takes its key from the strip set
    as the insert's `sidechain` (as in step 12) — the key ducks the signal; the
    unit's main input is unaffected.
18. A multi-channel (more than 2 channels) CLAP plugin plays its first two
    channels, with no crash.
