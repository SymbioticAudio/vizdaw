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
