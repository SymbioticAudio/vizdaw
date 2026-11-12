# core-io test data

Every file here was made by the VizDAW project and is distributed under
GPL-3.0 with the rest of the repository. None comes from another program
or a third party.

## Standard MIDI Files (plan 6a Task 3)

These were written by hand as hex, byte by byte, from the SMF 1.0 layout.
A throwaway script only joined the hex and put each chunk's 4-byte length
in front of it (`MThd` length 6; format, track count and division as given
below). The bytes of each track body are listed here, one event per line.

### `golden_type1.mid`

Type 1, 960 PPQ (`03C0`), two tracks. It is the golden of
`smf_bytes.rs::reads_and_writes_the_golden`; `smf::write` must produce it
byte for byte.

Conductor:

```
00 FF 03 04 53 6F 6E 67   track name "Song"
00 FF 51 03 07 A1 20      tempo 500 000 us per quarter (120 BPM)
00 FF 58 04 04 02 18 08   time signature 4/4, 24 clocks, 8 32nds
00 FF 59 02 00 00         key signature C major
00 FF 2F 00               end of track
```

Track 2:

```
00 FF 03 04 42 61 73 73   track name "Bass"
00 90 3C 64               note on C4, velocity 100
87 40 80 3C 00            960 ticks later, note off C4
00 FF 2F 00               end of track
```

### `type0_running_status.mid`

Type 0, 480 PPQ (`01E0`), one track on channel 10, using running status,
a single-packet SysEx, a SysEx split over two packets, an escape and a
note-on of velocity 0:

```
00 FF 03 05 44 72 75 6D 73   track name "Drums"
00 FF 51 03 08 52 AE         tempo 545 454 us (110 BPM)
00 C9 00                     program change 0
00 99 24 64                  note on 36
00 2A 50                     running status: note on 42
00 F0 05 7E 7F 09 01 F7      SysEx GM System On (one packet)
83 60 89 24 00               480 ticks: note off 36
00 2A 00                     running status: note off 42
00 99 26 64                  note on 38
00 2A 50                     running status: note on 42
00 F0 03 43 10 4C            SysEx first packet, no F7
10 F7 03 00 00 F7            16 ticks: continuation packet ending in F7
00 F7 02 F3 01               escape: Song Select 1
83 60 99 26 00               480 ticks: note on 38, velocity 0
00 2A 00                     running status: note on 42, velocity 0
00 B9 07 64                  controller 7 = 100
00 E9 00 40                  pitch bend centre
00 FF 2F 00                  end of track
```

### `type1_three_tracks.mid`

Type 1, 96 PPQ (`0060`), three tracks, with tempo changes, markers and
lyrics.

Conductor:

```
00 FF 03 04 54 75 6E 65      track name "Tune"
00 FF 58 04 03 02 18 08      time signature 3/4
00 FF 51 03 07 A1 20         tempo 120 BPM
00 FF 06 05 49 6E 74 72 6F   marker "Intro"
82 20 FF 51 03 06 1A 80      288 ticks: tempo 400 000 us (150 BPM)
00 FF 06 05 56 65 72 73 65   marker "Verse"
82 20 FF 51 03 09 27 C0      288 ticks: tempo 600 000 us (100 BPM)
00 FF 2F 00                  end of track
```

Track 2:

```
00 FF 03 06 4D 65 6C 6F 64 79   track name "Melody"
00 90 3C 50                     note on 60
00 FF 05 02 4C 61               lyric "La"
60 80 3C 40                     96 ticks: note off 60
00 90 3E 50                     note on 62
00 FF 05 02 44 69               lyric "Di"
60 80 3E 40                     96 ticks: note off 62
00 FF 2F 00                     end of track
```

Track 3:

```
00 FF 03 04 42 61 73 73   track name "Bass"
00 C1 21                  program change 33, channel 2
00 91 24 64               note on 36
81 40 81 24 00            192 ticks: note off 36
00 FF 01 04 6E 6F 74 65   text "note"
00 91 2B 64               note on 43
81 40 81 2B 00            192 ticks: note off 43
00 FF 2F 00               end of track
```

## MIDI Clip Files (plan 6a Task 4)

These were written by hand as hex, 32-bit word by word, from the MIDI Clip
File layout (M2-116-U) and the UMP layouts (M2-104-UM v1.1.2). A throwaway
script only turned the hex below into bytes (the text after each line's
words is a comment). Every word is big-endian; every message has its own
Delta Clockstamp (DCS), as `clip::write` writes them, so `clip::write`
gives each file back byte for byte.

### `golden_minimal.midi2`

The golden of `clip_bytes.rs::writes_the_minimal_golden`: 15 360 ticks per
quarter, one note.

```
534D4632 434C4950                              SMF2CLIP
00400000 00303C00                              DCS 0, DCTPQ 15 360
00400000 D0100000 02FAF080 00000000 00000000   Set Tempo 50 000 000 x 10 ns (120 BPM)
00400000 D0100001 04020800 00000000 00000000   Set Time Signature 4/4, 8 32nds
00400000 F0200000 00000000 00000000 00000000   Start of Clip
00400000 40903C00 80000000                     note on 60, velocity 0x8000
00403C00 40803C00 00000000                     15 360 ticks: note off 60
00400000 F0210000 00000000 00000000 00000000   End of Clip
```

### `every_kind.midi2`

960 ticks per quarter, with every message kind the reader knows, a text
and SysEx 7 and SysEx 8 over two packets, a gap restarted with a NOOP, and
two things the reader drops with a report item: a profile setting before
DCTPQ (Note) and a JR Timestamp (Left out, "Unknown MIDI messages").
`clip::write` gives the file back without those two (and the DCS before
the JR Timestamp):

```
534D4632 434C4950                              SMF2CLIP
30167E7F 0D220100                              SysEx 7 before DCTPQ, a profile setting: 7E 7F 0D 22 01 00
30357E00 00000000                              ... end: 7E 00 00 00 00
00400000 003003C0                              DCS 0, DCTPQ 960
00400000 D0100000 02FAF080 00000000 00000000   Set Tempo (120 BPM)
00400000 D0100001 03020800 00000000 00000000   Set Time Signature 3/4
00400000 F0200000 00000000 00000000 00000000   Start of Clip
00400000 D0100005 15000000 00000000 00000000   Set Key Signature 1 sharp, tonic E (E minor)
00400000 D0100102 536F6E67 00000000 00000000   composition name "Song"
00400000 D0100103 42617373 00000000 00000000   clip name "Bass"
00400000 D0500201 48656C6C 6F2C2056 697A4441   lyric "Hello, VizDA" (start)
         D0D00201 57212100 00000000 00000000   ... "W!!" (end)
00400000 10F60000                              MIDI 1.0 system: tune request
00400000 20903C64                              MIDI 1.0 note on 60, velocity 100
00400000 40903C03 C9247800                     MIDI 2.0 note on 60, velocity 0xC924, pitch 7.9 attribute
00400000 40A03C00 80000000                     poly pressure 60
00400000 40B00700 80000000                     controller 7
00400000 40C00001 21000102                     program 33, bank 1/2
00400000 40D00000 40000000                     channel pressure
00400000 40E00000 80000000                     pitch bend centre
00400000 40003C03 80000000                     per-note registered controller 3 on 60
00400000 40103C05 12345678                     per-note assignable controller 5 on 60
00400000 40603C00 80000000                     per-note pitch bend centre on 60
00400000 30047E7F 09010000                     SysEx 7: 7E 7F 09 01 (one packet)
00400000 30164310 4C00007E                     SysEx 7 start: 43 10 4C 00 00 7E
         30340102 03040000                     ... end: 01 02 03 04
00400000 5005007E 7F0D7000 00000000 00000000   SysEx 8, stream 0: 7E 7F 0D 70
00400000 501E0000 01020304 05060708 090A0B0C   SysEx 8 start, stream 0: 00 01 ... 0C
         5038000D 0E0F1011 12130000 00000000   ... end: 0D 0E ... 13
00400000 00201234                              JR Timestamp (not used in a file)
004003C0 40803C03 00007800                     960 ticks: MIDI 2.0 note off 60, velocity 0, pitch 7.9 attribute
004FFFFF 00000000                              DCS 1 048 575, NOOP
00400001 20803C00                              1 tick: MIDI 1.0 note off 60
00400000 F0210000 00000000 00000000 00000000   End of Clip
```

## MIDI export goldens (plan 6a Task 6)

`midi_out.rs` compares `midi_out`'s output with these byte for byte. A
missing golden fails its test.

### Hand-written: `export_small_type1_960.mid` and `export_small_clip.midi2`

Written by hand as hex before the export code existed (a throwaway script
only turned the hex into bytes), from Ruling 10, the text table in
`midi_in.rs` and the layouts above. They are the export of one sequence
"Song" at 120 BPM in 4/4 and C major, no markers, with one track "Bass" on
a built-in synth on group 1, channel 1 (no patch, no mix) holding one note:
pitch 60, velocity `up7(100)` (0xC924), release 0, at tick 0 for 15 360
ticks (a quarter). `small_goldens_match` checks them; they are never
blessed.

`export_small_type1_960.mid` is `golden_type1.mid` above, byte for byte:
the conductor is the sequence name "Song", tempo, time and key signatures;
track 2 is "Bass" with `00 90 3C 64`, `87 40 80 3C 00`.

`export_small_clip.midi2` (every message with its own DCS):

```
534D4632 434C4950                              SMF2CLIP
00400000 00303C00                              DCS 0, DCTPQ 15 360
00400000 D0100000 02FAF080 00000000 00000000   Set Tempo 50 000 000 x 10 ns (120 BPM)
00400000 D0100001 04020800 00000000 00000000   Set Time Signature 4/4
00400000 F0200000 00000000 00000000 00000000   Start of Clip
00400000 D0100102 536F6E67 00000000 00000000   composition name "Song"
00400000 D0100103 42617373 00000000 00000000   clip name "Bass"
00400000 D0100005 03000000 00000000 00000000   Set Key Signature 0 sharps, tonic C
00400000 40903C00 C9240000                     note on 60, velocity 0xC924
00403C00 40803C00 00000000                     15 360 ticks: note off 60, velocity 0
00400000 F0210000 00000000 00000000 00000000   End of Clip
```

### Regression goldens

`export_type1_960.mid`, `export_type0_960.mid`, `export_type1_480.mid` and
`export_type1_15360.mid` (`golden_options`) and `export_clip.midi2`
(`clip_export_is_exact`) were made by one run with `VIZDAW_BLESS=1` (never
set in CI) and then decoded by hand, as below, against Ruling 10 and the
layouts above. The document (`golden_doc` in `midi_out.rs`): "Song" at
120 BPM, ramping 120 → 140 over bar 2, 3/4 from bar 3, D major, markers
"Intro" (bar 1) and "Verse" (bar 3), copyright "Copyright 2026 the VizDAW
tests"; "Bass" on an external destination on channel 1 with patch bank
0/1 program 33 and MIDI volume 100 (a MIDI mix is only kept for an
external destination), holding notes, a lyric, a SysEx, a muted note, a
controller and a bend; "Drums" on a built-in synth on channel 10, a drum
track. `export_clip.midi2` adds a track "Lead" (channel 3) and exports it
for bar 2 to bar 4.

`export_type1_960.mid`: `MThd`, type 1, 3 tracks, 960 (`03C0`).

Conductor:

```
00 FF 03 04 "Song"                    title (the sequence name)
00 FF 02 1F "Copyright 2026 ..."      copyright
00 FF 51 03 07 A1 20                  tempo 500 000 us (120 BPM)
00 FF 58 04 04 02 18 08               4/4
00 FF 59 02 02 00                     D major
00 FF 06 05 "Intro"                   marker
9E 00 FF 51 03 07 97 06               3 840 (bar 2): 497 414 us, the ramp's first step
81 70 FF 51 03 07 83 18               240 (1/16) later: the next step; 16 steps in all,
...                                   each shorter (07 6F 92 ... 06 91 A0)
81 70 FF 51 03 06 8A 1B               7 680 (bar 3): 428 571 us (140 BPM)
00 FF 58 04 03 02 18 08               3/4
00 FF 06 05 "Verse"                   marker
00 FF 2F 00                           end of track
```

Track 2:

```
00 FF 03 04 "Bass"                    track name
00 FF 05 04 "Down"                    lyric (metas first, Ruling 10)
00 B0 00 00   00 B0 20 01   00 C0 21  default patch: bank 0/1, program 33 (patch class)
00 B0 07 64                           MIDI mix volume 100 (control class)
00 F0 05 7E 7F 09 01 F7               SysEx (after the control messages)
00 90 28 5A                           note on 40 (note-ons last)
83 60 80 28 00   00 90 2B 64          480: note off 40, then note on 43
83 60 80 2B 00                        960: note off 43 (the muted note 45 is not written)
87 40 B0 01 40                        1 920: CC 1 = 64
87 40 E0 64 40                        2 880: pitch bend 8 292
A5 40 90 28 50   87 40 80 28 00       7 680 (bar 3): note 40 for a quarter
87 40 90 2B 50   87 40 80 2B 00       9 600: note 43 for a quarter
00 FF 2F 00                           end of track
```

Track 3:

```
00 FF 03 05 "Drums"
00 99 24 6E   00 99 2A 46             notes 36 and 42 on channel 10
81 70 89 24 00   00 89 2A 00          240: their note-offs
81 70 99 2A 46   81 70 89 2A 00       480 and 720: note 42
81 70 99 26 64   81 70 89 26 00       960 and 1 200: note 38
94 50 99 24 6E   81 70 89 24 00       3 840 (bar 2) and 4 080: note 36
00 FF 2F 00
```

`export_type1_480.mid` and `export_type1_15360.mid` are the same events at
480 and 15 360 ticks per quarter; `export_type0_960.mid` is the conductor's
and both tracks' events in one track (type 0, no track names).

`export_clip.midi2` ("Lead", bar 2 to bar 4, at 15 360 ticks per quarter):

```
534D4632 434C4950                              SMF2CLIP
00400000 00303C00                              DCTPQ 15 360
00400000 D0100000 02F6FE42 00000000 00000000   header: Set Tempo 49 741 378 (the ramp's first step)
00400000 D0100001 04020800 00000000 00000000   header: 4/4 (in force at bar 2)
00400000 F0200000 00000000 00000000 00000000   Start of Clip
00400000 D0100102 536F6E67 00000000 00000000   composition name "Song"
00400000 D0100103 4C656164 00000000 00000000   clip name "Lead"
00400000 D0500104 ... D0900104 ... D0D00104 ... copyright in three packets (start, continue, end)
00400000 D0100005 24000000 00000000 00000000   key: 2 sharps, tonic D (in the data at 0, F8)
00400064 40923C03 12347800                     100: note on 60 ch 3, velocity 0x1234, pitch 7.9 attribute 0x7800
00400000 40023C4A 90000000                     per-note registered controller 74 (after its note-on)
004001F4 40623C00 81000000                     600: per-note pitch bend
004001F4 40823C00 04000000                     1 100: note off 60, release 0x0400
00400384 40B24A00 12345678                     2 000: CC 74, 32 bits
00400730 D0100000 02EF3561 ...                 3 840: the ramp's second step; then 14 more,
00400F00 D0100000 02E794F7 ...                 3 840 ticks apart, each faster
...
00400F00 D0100000 028DF2B7 ...                 61 440 (bar 3): Set Tempo 42 857 143 (140 BPM)
00400000 D0100001 03020800 ...                 3/4
00400000 D0500100 6D61726B 65723A20 56657273   "marker: Vers" (start) ...
         D0D00100 65000000 00000000 00000000   ... "e" (end)
00403C00 40923E00 FFFF0000                     76 800: note on 62, velocity 0xFFFF
00403C00 40823E00 00000000                     92 160: note off 62
00403C00 F0210000 00000000 00000000 00000000   107 520 (bar 4): End of Clip
```

The notes of "Lead" at bar 1 and at bar 4 start outside the range and are
not written.

## Export for another DAW goldens (plan 6b Task 1)

`for_daw.rs::small_files_match_the_goldens` compares `for_daw`'s output
with these byte for byte. Both were written by hand before the code
existed; they are never blessed, and a missing one fails. The document is
the small one above: "Song" at 120 BPM in 4/4 and C major, one track
"Bass" on the built-in synth on group 1, channel 1, holding one note
(pitch 60, velocity `up7(100)`, tick 0, a quarter long).

### `for_daw_tempo_map.mid`

`Tempo map.mid`: type 1, one track, 960 PPQ (`MThd` `0001 0001 03C0`).
The track is the conductor of `export_small_type1_960.mid`, byte for
byte:

```
00 FF 03 04 53 6F 6E 67   track name "Song"
00 FF 51 03 07 A1 20      tempo 500 000 us per quarter (120 BPM)
00 FF 58 04 04 02 18 08   time signature 4/4, 24 clocks, 8 32nds
00 FF 59 02 00 00         key signature C major
00 FF 2F 00               end of track
```

### `for_daw_about.txt`

`About these files.txt` for that document, with one WAV stem (24-bit,
48 000 Hz, 96 000 frames): UTF-8, `\n` line ends, one newline at the end.
The file is the text itself; it was typed from the layout in plan 6b
Task 1 (folder name, Start, Tempo, Audio, Files, then the report).

## DAWproject (plan 6d)

`tests/data/dawproject/` holds the format's schema files, vendored byte for
byte, and the hand-written fixtures the typed model is tested against.

- **Reference:** https://github.com/bitwig/dawproject (MIT), commit
  `ee4dcdde75940f30e14e55401a26955a58b8322b` (`main`, 2025-07-12). The tag
  `v1.0.0` lacks `Send/Enable` and `Clip@enable`, which `main` has, so the
  schema we write and check against is `main`'s.
- **`Project.xsd`:** SHA-256
  `58e2fd9864772850aac3eab1f3de8693857dc5384df6d29fbc320ae1de2347cc`.
- **`MetaData.xsd`:** SHA-256
  `fb3ba378271770dddbcced8990aba537de3d36ff2d58573523460a699221c99f`.
  `dawproject_schema.rs::xsds_are_the_pinned_ones` checks both.
- **`LICENSE`:** the reference's MIT licence, verbatim (SHA-256
  `3aaee5877c9df985f935e40f42f4452182df9e8a8308dcc6cf1a2341458cdfeb`). The
  notice must stay with the schema files.
- **`hand_minimal.project.xml` and `hand_minimal.metadata.xml`:** written by
  hand from plan 6d's format section, element by element in the XSD's order,
  before any code existed; never generated or blessed. The project is one
  notes track "Keys" (a channel with mute, pan, volume and one send, and one
  CLAP device with a state file), a master track, an arrangement with one
  clip holding two notes (one with a transpose points child) and one marker
  "Verse". `dawproject_schema.rs` reads them, writes them back byte for byte
  and validates them (and a project built in code with every element kind)
  with `xmllint --schema Project.xsd`.
- **`hand_timeline.project.xml`:** written by hand before the timeline
  import existed (plan 6d Task 7); never generated or blessed. At 120 BPM in
  4/4, beside a master "Master" (channel `mc`, volume `mcv`) and an effect
  bus "FX" (`fxc`), it holds one track per test of
  `dawproject_timeline_in.rs`, named after the test, with exactly the clips
  that test states (a "<test> 2" or "3" track where a test compares two
  cases):
  - `notes_land_where_the_clips_play`: a clip at beat 4, `playStart` 1,
    `playStop` 3, notes at 0.5, 1, 2.5 and 3 (keys 60 to 63, a beat each);
  - `looped_clip_becomes_a_looped_use`: "Loop A" (loop 0–2, 6 beats), "Loop
    B" (loop 0–2, 5 beats, at 8) and "Loop C" (`playStart` 1, loop 0–2, 4
    beats, at 16; keys 62 at 0 and 64 at 1);
  - `alias_clips_share_one_sequence`: a clip whose `Notes id="riff"` a
    second clip references; tracks 2 and 3 (CLAP instruments X and Y) share
    "Shared" (`riff2`);
  - `same_name_same_content_is_reshared`: "A" (key 60) at 0 and 4, "A" (key
    62) at 8, "B" (key 60) at 12;
  - `note_expressions_map_to_per_note_expression`: a note with transpose 2,
    timbre 0.5 at 0.25, pressure 1 at 0.5, gain 0.5, pan 0.25 and formant;
    a note at 2 with transpose 60;
  - `midi_points_become_events`: CC 1 = 0.5 at 0, pitch bend 0.5 at 1, CC 0
    = 0, CC 32 = 1/127 and program 5 at 2, linear CC 7 points at 3 and 3.5;
  - `audio_clips_map_to_regions`: "Region" at beat 2 (seconds content 0.5 to
    1.5, fade-in 0.01 s) and "Muted" at 8 (fade-out 0.25 beats,
    `enable="false"`), all audio clips on `audio/a.wav` (48 kHz, stereo, 10 s);
  - `negative_fade_in_starts_the_clip_earlier`: a clip at 4 with
    `fadeInTime` −0.5 beats, then a pair at 10 and 12 where the second has
    the same negative fade-in over the first;
  - `nested_clips_compose_their_offsets`: a clip at 8 (3 beats) holding clips
    at 0 and 2 (one second of audio each);
  - `identity_warps_are_not_reported`: warps (0, 0 s) and (8, 4 s); track 2
    (0, 0 s) and (8, 2 s);
  - `overlapping_clips_with_covering_fades_become_crossfades`: A at 0 (1 s,
    fade-out 0.5 s) and B at beat 1 (fade-in 0.5 s); track 2 the same with
    no fade-out and a 0.1 s fade-in;
  - `automation_points_become_events`: a track (channel `a12c`: volume, pan,
    mute, a send to FX and a CLAP effect with parameter 7, −12 to 12 dB) with
    volume, pan (one held step), mute, send and parameter points; points at
    the arrangement's top level for track 2's volume (`a12bv`) and the
    master's (`mcv`);
  - `seconds_timelines_map_through_the_tempo_map`: a seconds timeline with a
    clip at 0.5 s; track 2, a beats `Clips` inside a seconds `Lanes` (clip at
    4);
  - "Bass": the scenes' track (two scenes, one slot each, keys 36 and 38),
    and a `Video` in its lanes;
  - `hostile_numbers_leave_items_out`: a clip at 1e300, notes of duration −1,
    key 200, channel 99 and velocity 2.0, a `NaN` point, an `Audio` with
    `sampleRate="0"` and a clip of duration −1, beside readable notes at 1
    and 2 and a CC point at 1.
