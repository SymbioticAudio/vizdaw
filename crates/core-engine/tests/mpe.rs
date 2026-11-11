//! The MPE codec (plan 5g-2 Task 2; Rulings 4–7): pitch-bend scaling, the
//! configuration bytes, decoding a controller's channel-per-note MIDI 1.0
//! into per-note events, and encoding them back onto rotating member
//! channels. Byte-level golden tests.

use core_engine::input_fx::{CENTER, Expr, LiveMsg, NoteExpr, up7, up32};
use core_engine::midi1::Midi1Msg;
use core_engine::mpe::{MpeDecoder, MpeEncoder, bend_in, bend_out, configuration};
use core_model::mpe::{MpeConfig, MpeZone};

fn m(b: [u8; 3]) -> Midi1Msg {
    match b[0] & 0xF0 {
        0xC0 | 0xD0 => Midi1Msg::two(b[0], b[1]),
        _ => Midi1Msg::three(b[0], b[1], b[2]),
    }
}

fn lower(members: u8) -> MpeConfig {
    MpeConfig {
        zone: MpeZone::Lower,
        members,
        bend_range: 48,
    }
}

fn decode(d: &mut MpeDecoder, bytes: &[[u8; 3]]) -> Vec<LiveMsg> {
    let mut out = Vec::new();
    for b in bytes {
        d.decode(m(*b), &mut |x| out.push(x));
    }
    out
}

fn encode(e: &mut MpeEncoder, msgs: &[LiveMsg]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for x in msgs {
        e.encode(*x, &mut |b| out.push(b.as_slice().to_vec()));
    }
    out
}

fn on(pitch: u8, v: u8, bend: u32, pressure: u32, timbre: u32) -> LiveMsg {
    LiveMsg::NoteOn {
        pitch,
        velocity: up7(v),
        expr: NoteExpr {
            bend,
            pressure,
            timbre,
        },
    }
}

#[test]
fn bends_scale_by_range_and_clamp_at_48() {
    assert_eq!(bend_in(8192, 48), CENTER);
    assert_eq!(bend_in(0, 48), 0);
    assert_eq!(bend_in(16383, 48), 0xFFFC_0000);
    assert_eq!(bend_in(16383, 24), 0xBFFE_0000);
    assert_eq!(bend_in(16383, 96), u32::MAX, "past ±48 semitones clamps");
    assert_eq!(bend_out(CENTER, 2), 8192);
    assert_eq!(bend_out(0, 48), 0);
    assert_eq!(bend_out(u32::MAX, 24), 16383, "past the range clamps");
    for range in 1u8..=48 {
        for v in (0..=16383u16).step_by(7).chain([16383]) {
            assert_eq!(
                bend_out(bend_in(v, range), range),
                v,
                "range {range} value {v}"
            );
        }
    }
    // A 2-semitone bend reads as the same pitch at another range.
    let two_up = bend_in(16383, 2);
    assert_eq!(bend_out(two_up, 48), 8192 + 341);
}

#[test]
fn the_configuration_message_is_the_mcm_then_the_bend_range() {
    let got: Vec<Vec<u8>> = configuration(&MpeConfig {
        zone: MpeZone::Upper,
        members: 7,
        bend_range: 24,
    })
    .iter()
    .map(|x| x.as_slice().to_vec())
    .collect();
    assert_eq!(
        got,
        vec![
            vec![0xBF, 101, 0],
            vec![0xBF, 100, 6],
            vec![0xBF, 6, 7],
            vec![0xBF, 101, 127],
            vec![0xBF, 100, 127],
            vec![0xBE, 101, 0],
            vec![0xBE, 100, 0],
            vec![0xBE, 6, 24],
            vec![0xBE, 38, 0],
            vec![0xBE, 101, 127],
            vec![0xBE, 100, 127],
        ]
    );
}

#[test]
fn expression_sent_before_a_note_is_its_start() {
    let mut d = MpeDecoder::new(lower(15));
    let out = decode(
        &mut d,
        &[
            [0xE1, 0x00, 0x50],
            [0xB1, 74, 0x30],
            [0xD1, 0x20, 0],
            [0x91, 60, 100],
        ],
    );
    assert_eq!(
        out,
        vec![on(
            60,
            100,
            bend_in(0x50 << 7, 48),
            up32(0x20, 7),
            up32(0x30, 7)
        )]
    );
    // While it sounds, expression is per-note.
    let out = decode(
        &mut d,
        &[
            [0xE1, 0x10, 0x60],
            [0xD1, 0x40, 0],
            [0xB1, 74, 0x7F],
            [0xA1, 60, 9],
        ],
    );
    assert_eq!(
        out,
        vec![
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Bend,
                value: bend_in(0x10 | (0x60 << 7), 48)
            },
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Pressure,
                value: up32(0x40, 7)
            },
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Timbre,
                value: up32(0x7F, 7)
            },
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Pressure,
                value: up32(9, 7)
            },
        ]
    );
    assert_eq!(
        decode(&mut d, &[[0x81, 60, 0]]),
        vec![LiveMsg::NoteOff {
            pitch: 60,
            velocity: 0
        }]
    );
    // No note sounds: a bend only sets the channel's next start.
    assert!(decode(&mut d, &[[0xE1, 0x00, 0x40]]).is_empty());
    assert_eq!(
        decode(&mut d, &[[0x91, 62, 90]]),
        vec![on(62, 90, CENTER, up32(0x40, 7), up32(0x7F, 7))]
    );
}

#[test]
fn manager_messages_are_channel_wide() {
    let mut d = MpeDecoder::new(lower(15));
    assert_eq!(
        decode(&mut d, &[[0xB0, 64, 127], [0xE0, 0, 0x50], [0x90, 48, 80]]),
        vec![
            LiveMsg::Channel(Midi1Msg::three(0xB0, 64, 127)),
            LiveMsg::Channel(Midi1Msg::three(0xE0, 0, 0x50)),
            LiveMsg::on(48, 80),
        ]
    );
    // Other member CCs are channel messages; channels outside the zone are not this lane's.
    assert_eq!(
        decode(&mut d, &[[0xB2, 1, 64]]),
        vec![LiveMsg::Channel(Midi1Msg::three(0xB2, 1, 64))]
    );
    let mut small = MpeDecoder::new(lower(3));
    assert!(small.accepts(0) && small.accepts(3) && !small.accepts(4) && !small.accepts(15));
    let up = MpeDecoder::new(MpeConfig {
        zone: MpeZone::Upper,
        members: 2,
        bend_range: 48,
    });
    assert!(
        up.accepts(15) && up.accepts(14) && up.accepts(13) && !up.accepts(12) && !up.accepts(0)
    );
    assert!(
        decode(&mut small, &[[0x95, 60, 100]]).is_empty(),
        "a channel outside the zone is ignored if offered"
    );
}

#[test]
fn the_mcm_and_rpn_0_reconfigure_the_zone() {
    let mut d = MpeDecoder::new(lower(15));
    assert_eq!(d.take_changed(), None);
    // MCM on the manager channel: 7 members. The RPN bytes are consumed.
    assert!(decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 7]]).is_empty());
    assert_eq!(d.config(), Some(lower(7)));
    assert_eq!(d.take_changed(), Some(Some(lower(7))));
    assert_eq!(d.take_changed(), None, "once");
    assert!(!d.accepts(8));
    // RPN 0 on a member: the bend range.
    assert!(
        decode(
            &mut d,
            &[[0xB3, 101, 0], [0xB3, 100, 0], [0xB3, 6, 24], [0xB3, 38, 0]]
        )
        .is_empty()
    );
    assert_eq!(
        d.config(),
        Some(MpeConfig {
            zone: MpeZone::Lower,
            members: 7,
            bend_range: 24
        })
    );
    assert!(d.take_changed().is_some());
    let out = decode(&mut d, &[[0x93, 60, 100], [0xE3, 0x7F, 0x7F]]);
    assert_eq!(
        out[1],
        LiveMsg::PerNote {
            pitch: 60,
            expr: Expr::Bend,
            value: bend_in(16383, 24)
        }
    );
    // An MCM for the other zone is ignored; members 0 turns this zone off.
    assert!(decode(&mut d, &[[0xBF, 101, 0], [0xBF, 100, 6], [0xBF, 6, 3]]).is_empty());
    assert_eq!(d.config().map(|c| c.members), Some(7));
    decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 0]]);
    assert_eq!(d.config(), None);
    assert_eq!(d.take_changed(), Some(None));
    assert!(d.accepts(9), "off: every channel, as plain MIDI");
    assert_eq!(
        decode(&mut d, &[[0x99, 40, 100]]),
        vec![LiveMsg::on(40, 100)]
    );
    assert_eq!(MpeDecoder::new(lower(15)), MpeDecoder::new(lower(15)));
    assert_eq!(d.initial(), lower(15));
}

#[test]
fn hostile_mpe_bytes_are_ignored_or_passed() {
    let mut d = MpeDecoder::new(lower(15));
    // Data entry with no RPN selected, an NRPN, a null RPN, then data.
    let out = decode(
        &mut d,
        &[
            [0xB0, 6, 9],
            [0xB1, 99, 1],
            [0xB1, 98, 2],
            [0xB1, 6, 3],
            [0xB0, 101, 127],
            [0xB0, 100, 127],
            [0xB0, 6, 5],
        ],
    );
    assert!(
        out.iter().all(|x| matches!(x, LiveMsg::Channel(_))),
        "{out:?}"
    );
    assert_eq!(d.config(), Some(lower(15)), "nothing reconfigured");
    // A note-off for a note never started, a pressure with no note.
    assert_eq!(
        decode(&mut d, &[[0x82, 61, 0]]),
        vec![LiveMsg::NoteOff {
            pitch: 61,
            velocity: 0
        }]
    );
    assert!(decode(&mut d, &[[0xD4, 99, 0]]).is_empty());
    // A bend range of 0 is ignored.
    decode(&mut d, &[[0xB1, 101, 0], [0xB1, 100, 0], [0xB1, 6, 0]]);
    assert_eq!(d.config().map(|c| c.bend_range), Some(48));
}

#[test]
fn notes_go_out_on_member_channels_with_their_start_before_them() {
    let mut e = MpeEncoder::new(lower(15));
    let got = encode(
        &mut e,
        &[
            on(
                60,
                100,
                bend_in(0x50 << 7, 48),
                up32(0x20, 7),
                up32(0x30, 7),
            ),
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Bend,
                value: bend_in(0x10 | (0x60 << 7), 48),
            },
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Timbre,
                value: up32(0x7F, 7),
            },
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Pressure,
                value: up32(0x40, 7),
            },
            LiveMsg::Channel(Midi1Msg::three(0xB5, 64, 127)),
            LiveMsg::NoteOff {
                pitch: 60,
                velocity: 0,
            },
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Bend,
                value: CENTER,
            },
        ],
    );
    assert_eq!(
        got,
        vec![
            vec![0xE1, 0x00, 0x50],
            vec![0xB1, 74, 0x30],
            vec![0xD1, 0x20],
            vec![0x91, 60, 100],
            vec![0xE1, 0x10, 0x60],
            vec![0xB1, 74, 0x7F],
            vec![0xD1, 0x40],
            vec![0xB0, 64, 127],
            vec![0x81, 60, 0],
        ],
        "a per-note message after its note ended is dropped"
    );
    let mut up = MpeEncoder::new(MpeConfig {
        zone: MpeZone::Upper,
        members: 3,
        bend_range: 48,
    });
    let got = encode(
        &mut up,
        &[
            LiveMsg::on(60, 100),
            LiveMsg::on(64, 100),
            LiveMsg::Channel(Midi1Msg::three(0xB0, 1, 2)),
        ],
    );
    assert_eq!(got[3], vec![0x9E, 60, 100]);
    assert_eq!(got[7], vec![0x9D, 64, 100]);
    assert_eq!(got[8], vec![0xBF, 1, 2]);
}

#[test]
fn notes_rotate_to_the_channel_released_longest_ago() {
    let mut e = MpeEncoder::new(lower(3));
    let ch = |out: Vec<Vec<u8>>| {
        out.iter()
            .filter(|b| b[0] & 0xF0 == 0x90)
            .map(|b| b[0] & 0x0F)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ch(encode(
            &mut e,
            &[LiveMsg::on(60, 100), LiveMsg::on(62, 100)]
        )),
        vec![1, 2]
    );
    encode(&mut e, &[LiveMsg::off(60)]);
    // Channel 3 was never used: it goes before channel 1, just released.
    assert_eq!(ch(encode(&mut e, &[LiveMsg::on(64, 100)])), vec![3]);
    encode(&mut e, &[LiveMsg::off(62), LiveMsg::off(64)]);
    // Now 1 was released first, then 2, then 3.
    assert_eq!(
        ch(encode(
            &mut e,
            &[LiveMsg::on(65, 100), LiveMsg::on(67, 100)]
        )),
        vec![1, 2]
    );
    assert_eq!(e.take_stolen(), 0);
}

#[test]
fn the_oldest_note_is_stolen_and_counted() {
    let mut e = MpeEncoder::new(lower(2));
    encode(&mut e, &[LiveMsg::on(60, 100), LiveMsg::on(64, 100)]);
    let got = encode(&mut e, &[LiveMsg::on(67, 100)]);
    assert_eq!(got[0], vec![0x81, 60, 0], "the oldest ends first");
    assert_eq!(got.last().unwrap(), &vec![0x91, 67, 100]);
    assert_eq!(e.take_stolen(), 1);
    // The stolen note's own off later does nothing.
    assert!(encode(&mut e, &[LiveMsg::off(60)]).is_empty());
    let mut flushed = Vec::new();
    e.flush(&mut |b| flushed.push(b.as_slice().to_vec()));
    assert_eq!(flushed.len(), 2);
    assert!(flushed.contains(&vec![0x81, 67, 0]) && flushed.contains(&vec![0x82, 64, 0]));
}

#[test]
fn a_shrinking_mcm_ends_the_notes_it_drops() {
    let mut d = MpeDecoder::new(lower(15));
    decode(
        &mut d,
        &[
            [0x99, 60, 100],
            [0x92, 64, 100],
            [0xE9, 0, 0x60],
            [0xB9, 101, 0],
        ],
    );
    let out = decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 7]]);
    assert_eq!(
        out,
        vec![LiveMsg::NoteOff {
            pitch: 60,
            velocity: 0
        }],
        "only channel 10 left the zone"
    );
    assert!(!d.accepts(9) && d.accepts(2));
    // Its state is gone: after rejoining, the first note starts from defaults.
    decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 15]]);
    assert_eq!(
        decode(&mut d, &[[0x99, 62, 90]]),
        vec![on(62, 90, CENTER, 0, CENTER)]
    );
    // Turning the zone off ends every member note too.
    let out = decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 0]]);
    let mut pitches: Vec<u8> = out.iter().filter_map(|x| x.pitch()).collect();
    pitches.sort();
    assert_eq!(pitches, vec![62, 64]);
    assert!(out.iter().all(|x| matches!(x, LiveMsg::NoteOff { .. })));
}

#[test]
fn an_mcm_can_turn_the_zone_back_on() {
    let mut d = MpeDecoder::new(MpeConfig {
        zone: MpeZone::Lower,
        members: 15,
        bend_range: 24,
    });
    decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 0]]);
    assert_eq!(d.take_changed(), Some(None));
    // Off: a manager-channel controller is plain MIDI, the RPN bytes are consumed.
    assert_eq!(
        decode(&mut d, &[[0xB0, 64, 127]]),
        vec![LiveMsg::Channel(Midi1Msg::three(0xB0, 64, 127))]
    );
    assert!(decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 4]]).is_empty());
    assert_eq!(
        d.config(),
        Some(MpeConfig {
            zone: MpeZone::Lower,
            members: 4,
            bend_range: 48
        })
    );
    assert_eq!(d.take_changed(), Some(Some(d.config().unwrap())));
    assert!(d.accepts(4) && !d.accepts(5));
    // A controller on another channel while off is never an MCM.
    let mut off = MpeDecoder::new(lower(15));
    decode(&mut off, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 0]]);
    assert_eq!(
        decode(&mut off, &[[0xB5, 6, 3]]),
        vec![LiveMsg::Channel(Midi1Msg::three(0xB5, 6, 3))]
    );
    assert_eq!(off.config(), None);
}

#[test]
fn an_mcm_resets_the_member_bend_range_to_48() {
    let mut d = MpeDecoder::new(MpeConfig {
        zone: MpeZone::Lower,
        members: 15,
        bend_range: 24,
    });
    decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 15]]);
    assert_eq!(d.config().map(|c| c.bend_range), Some(48));
    // A later RPN 0 sets it again.
    decode(&mut d, &[[0xB1, 101, 0], [0xB1, 100, 0], [0xB1, 6, 12]]);
    assert_eq!(d.config().map(|c| c.bend_range), Some(12));
}

#[test]
fn the_upper_zone_decodes_on_channels_16_down() {
    let up = MpeConfig {
        zone: MpeZone::Upper,
        members: 7,
        bend_range: 48,
    };
    let mut d = MpeDecoder::new(up);
    let out = decode(
        &mut d,
        &[
            [0xEE, 0, 0x50],
            [0x9E, 60, 100],
            [0xBE, 74, 0x10],
            [0x8E, 60, 0],
        ],
    );
    assert_eq!(
        out,
        vec![
            on(60, 100, bend_in(0x50 << 7, 48), 0, CENTER),
            LiveMsg::PerNote {
                pitch: 60,
                expr: Expr::Timbre,
                value: up32(0x10, 7)
            },
            LiveMsg::NoteOff {
                pitch: 60,
                velocity: 0
            },
        ]
    );
    // Manager 16 is plain; the lower manager is not this zone's.
    assert_eq!(
        decode(&mut d, &[[0xBF, 64, 127]]),
        vec![LiveMsg::Channel(Midi1Msg::three(0xBF, 64, 127))]
    );
    assert!(decode(&mut d, &[[0x90, 60, 100]]).is_empty());
    // An MCM on channel 16 resizes it; a lower-zone MCM on channel 1 is ignored.
    assert!(decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 3]]).is_empty());
    decode(&mut d, &[[0xBF, 101, 0], [0xBF, 100, 6], [0xBF, 6, 3]]);
    assert_eq!(d.config().map(|c| c.members), Some(3));
    assert!(d.accepts(12) && !d.accepts(11));
}

#[test]
fn mcm_and_range_values_clamp() {
    let mut d = MpeDecoder::new(lower(3));
    decode(&mut d, &[[0xB0, 101, 0], [0xB0, 100, 6], [0xB0, 6, 127]]);
    assert_eq!(d.config().map(|c| c.members), Some(15));
    decode(&mut d, &[[0xB1, 101, 0], [0xB1, 100, 0], [0xB1, 6, 127]]);
    assert_eq!(d.config().map(|c| c.bend_range), Some(96));
}

#[test]
fn the_same_pitch_twice_ends_the_first() {
    let mut e = MpeEncoder::new(lower(3));
    encode(&mut e, &[LiveMsg::on(60, 100)]);
    let got = encode(&mut e, &[LiveMsg::on(60, 100)]);
    assert_eq!(
        got[0],
        vec![0x81, 60, 0],
        "the first ends before the second starts"
    );
    assert_eq!(
        got.last().unwrap(),
        &vec![0x92, 60, 100],
        "on the channel released longest ago"
    );
    assert_eq!(encode(&mut e, &[LiveMsg::off(60)]), vec![vec![0x82, 60, 0]]);
    assert!(encode(&mut e, &[LiveMsg::off(60)]).is_empty());
    assert_eq!(e.take_stolen(), 0);
}
