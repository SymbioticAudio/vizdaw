use core_model::ModelError;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{BarBeatUnit, PPQ, Tick};
use proptest::prelude::*;

fn entry(tick: i64, num: u8, den: u8) -> MeterEntry {
    MeterEntry {
        tick: Tick(tick),
        meter: Meter { num, den },
        key: KeySig::default(),
    }
}

fn bbu(bar: i32, beat: u16, unit: u32) -> BarBeatUnit {
    BarBeatUnit { bar, beat, unit }
}

#[test]
fn tick_zero_is_bar_one() {
    let m = MeterMap::default();
    assert_eq!(m.tick_to_bbu(Tick(0), 4), bbu(1, 1, 0));
    assert_eq!(m.tick_to_bbu(Tick(PPQ * 5 + PPQ / 2), 4), bbu(2, 2, 2));
}

#[test]
fn pickup_ticks_are_bar_zero() {
    let m = MeterMap::default();
    // One beat before bar 1 is bar 0 beat 4.
    assert_eq!(m.tick_to_bbu(Tick(-PPQ), 1), bbu(0, 4, 0));
    assert_eq!(m.bbu_to_tick(bbu(0, 4, 0), 1), Tick(-PPQ));
    assert_eq!(m.tick_to_bbu(Tick(-PPQ * 5), 1), bbu(-1, 4, 0));
}

#[test]
fn meter_change_renumbers_following_bars() {
    // Two bars of 4/4, then 6/8.
    let m = MeterMap::new(vec![entry(0, 4, 4), entry(PPQ * 8, 6, 8)]).unwrap();
    assert_eq!(m.tick_to_bbu(Tick(PPQ * 8), 1), bbu(3, 1, 0));
    // 6/8 bar = 3 quarters; eighth-note beats.
    assert_eq!(m.tick_to_bbu(Tick(PPQ * 11 + PPQ / 2), 1), bbu(4, 2, 0));
    assert_eq!(m.bbu_to_tick(bbu(4, 2, 0), 1), Tick(PPQ * 11 + PPQ / 2));
}

#[test]
fn off_barline_change_is_rejected() {
    let err = MeterMap::new(vec![entry(0, 4, 4), entry(PPQ * 3, 3, 4)]).unwrap_err();
    assert_eq!(err, ModelError::NotOnBarline { tick: PPQ * 3 });
}

#[test]
fn invalid_signatures_are_rejected() {
    assert!(MeterMap::new(vec![entry(0, 4, 3)]).is_err());
    assert!(MeterMap::new(vec![entry(0, 0, 4)]).is_err());
    assert!(MeterMap::new(vec![entry(0, 4, 128)]).is_err());
    assert!(MeterMap::new(vec![entry(PPQ, 4, 4)]).is_err());
    let mut bad_key = entry(0, 4, 4);
    bad_key.key.sharps = 8;
    assert!(MeterMap::new(vec![bad_key]).is_err());
}

#[test]
fn barline_helpers() {
    let m = MeterMap::new(vec![entry(0, 3, 4)]).unwrap();
    assert_eq!(m.barline_at_or_after(Tick(1)), Tick(PPQ * 3));
    assert_eq!(m.barline_at_or_after(Tick(PPQ * 3)), Tick(PPQ * 3));
    assert_eq!(m.barline_at_or_before(Tick(PPQ * 4)), Tick(PPQ * 3));
    assert_eq!(m.barline_at_or_before(Tick(-1)), Tick(-PPQ * 3));
}

#[test]
fn zero_units_per_beat_does_not_panic() {
    let m = MeterMap::default();
    let p = m.tick_to_bbu(Tick(123), 0);
    assert_eq!(p.bar, 1);
    let _ = m.bbu_to_tick(bbu(i32::MAX, u16::MAX, u32::MAX), 0);
    let _ = m.bbu_to_tick(bbu(i32::MIN, 0, 0), 0);
}

fn arb_meter_map() -> impl Strategy<Value = MeterMap> {
    let sig = (1u8..=13, prop::sample::select(vec![1u8, 2, 4, 8, 16, 32]));
    prop::collection::vec((1i64..6, sig), 0..5).prop_map(|changes| {
        let mut entries = vec![entry(0, 4, 4)];
        let mut tick = 0;
        let mut bar_t = Meter { num: 4, den: 4 }.bar_ticks();
        for (bars, (num, den)) in changes {
            tick += bars * bar_t;
            entries.push(entry(tick, num, den));
            bar_t = Meter { num, den }.bar_ticks();
        }
        MeterMap::new(entries).unwrap()
    })
}

proptest! {
    #[test]
    fn tick_bbu_tick_round_trips_at_full_resolution(
        m in arb_meter_map(), t in -PPQ * 16..PPQ * 400
    ) {
        // PPQ*4 units per beat is a multiple of every beat length.
        let upb = (PPQ * 4) as u32;
        prop_assert_eq!(m.bbu_to_tick(m.tick_to_bbu(Tick(t), upb), upb), Tick(t));
    }

    #[test]
    fn bbu_tick_bbu_round_trips_for_coarser_units(
        m in arb_meter_map(), t in -PPQ * 16..PPQ * 400, upb in 1u32..=960
    ) {
        let p = m.tick_to_bbu(Tick(t), upb);
        prop_assert_eq!(m.tick_to_bbu(m.bbu_to_tick(p, upb), upb), p);
    }

    // Spec §8: tick <-> bar·beat·unit <-> seconds across ramps and meter changes.
    #[test]
    fn bbu_seconds_chain_round_trips(
        m in arb_meter_map(), t in 0..PPQ * 400, b in 5u32..=999
    ) {
        let tempo = TempoMap::new(vec![
            TempoPoint { tick: Tick(0), bpm: Bpm::whole(b), ramp: true },
            TempoPoint { tick: Tick(PPQ * 64), bpm: Bpm::whole(120), ramp: false },
        ]).unwrap();
        let upb = (PPQ * 4) as u32;
        let p = m.tick_to_bbu(Tick(t), upb);
        let secs = tempo.tick_to_seconds(m.bbu_to_tick(p, upb));
        let back = m.tick_to_bbu(tempo.seconds_to_tick(secs), upb);
        prop_assert_eq!(back, p);
    }
}
