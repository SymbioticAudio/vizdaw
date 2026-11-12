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
