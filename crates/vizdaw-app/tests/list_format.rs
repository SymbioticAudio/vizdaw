//! Swift `ListFormatTests`: positions, lengths, SMPTE, 7-bit values, pitch
//! and key names.

use vizdaw_app::list::{
    Bbu, Bend14, DurationText, KeyName, PitchName, Smpte, Value7, Velocity7, parse_position,
};
use vizdaw_ffi::events::MeterEntryRow;

const Q: i64 = 15_360;

fn meter(tick: i64, num: u8, den: u8) -> MeterEntryRow {
    MeterEntryRow {
        tick,
        num,
        den,
        sharps: 0,
        minor: false,
    }
}

fn four_four() -> Vec<MeterEntryRow> {
    vec![meter(0, 4, 4)]
}

fn bbu(bar: i64, beat: i64, unit: i64) -> Bbu {
    Bbu { bar, beat, unit }
}

#[test]
fn positions_read_like_the_transport() {
    let m = four_four();
    assert_eq!(Bbu::of(0, &m).text(), "1.1.0");
    assert_eq!(Bbu::of(Q, &m).text(), "1.2.0");
    assert_eq!(Bbu::of(4 * Q + Q / 2, &m).text(), "2.1.480");
    assert_eq!(Bbu::of(-Q, &m).text(), "0.4.0", "a pickup");
    let mut six_eight = four_four();
    six_eight.push(meter(4 * Q, 6, 8));
    assert_eq!(Bbu::of(4 * Q + 3 * (Q / 2), &six_eight).text(), "2.4.0");
    assert_eq!(bbu(2, 1, 480).tick(&m), 4 * Q + Q / 2);
    for t in [0, 1, Q - 1, 7 * Q + 123, -5 * Q] {
        let b = Bbu::of(t, &m);
        let back = b.tick(&m);
        assert!(back <= t && Bbu::of(back, &m) == b, "{t}");
    }
}

#[test]
fn typed_positions_keep_the_sub_fields_left_empty() {
    let cur = bbu(3, 2, 240);
    let p = |t| parse_position(t, cur);
    assert_eq!(p("5"), Some(bbu(5, 1, 0)), "bar number: downbeat");
    assert_eq!(p("5.3"), Some(bbu(5, 3, 0)));
    assert_eq!(p("5.3.480"), Some(bbu(5, 3, 480)));
    assert_eq!(p(".4"), Some(bbu(3, 4, 240)), "beat only");
    assert_eq!(p("..480"), Some(bbu(3, 2, 480)), "unit only");
    assert_eq!(p("7.."), Some(bbu(7, 2, 240)), "bar only");
    assert_eq!(p(" 6 "), Some(bbu(6, 1, 0)));
    assert_eq!(p("-1"), Some(bbu(-1, 1, 0)));
}

#[test]
fn huge_and_bad_positions_are_refused() {
    let cur = bbu(1, 1, 0);
    for bad in [
        "",
        "x",
        "5.0",
        "5.1.960",
        "1.2.3.4",
        "99999999999999999999",
        "1.1.99999999999",
        "1000001",
        "5.1000",
    ] {
        assert_eq!(parse_position(bad, cur), None, "{bad}");
    }
    let far = bbu(1_000_000, 999, 959);
    assert!(
        far.tick(&four_four()) > 0,
        "the largest typed position does not trap"
    );
    assert_eq!(DurationText::parse("99999999999", 0, Q), None);
}

#[test]
fn lengths_are_beats_and_units() {
    assert_eq!(DurationText::text(Q, Q), "1.0");
    assert_eq!(DurationText::text(Q + Q / 4, Q), "1.240");
    assert_eq!(DurationText::text(0, Q), "0.0");
    assert_eq!(DurationText::parse("2", Q, Q), Some(2 * Q));
    assert_eq!(
        DurationText::parse(".480", Q, Q),
        Some(Q + Q / 2),
        "units only"
    );
    assert_eq!(DurationText::parse("1.5", 0, Q), Some(Q + 80));
    assert_eq!(DurationText::parse("a", 0, Q), None);
    assert_eq!(DurationText::parse("-1", 0, Q), None);
}

#[test]
fn smpte_round_trips() {
    assert_eq!(Smpte::text(3661.5, 25), "01:01:01:12");
    assert_eq!(Smpte::text(-0.5, 25), "-00:00:00:12");
    assert_eq!(Smpte::text(f64::NAN, 30), "--:--:--:--");
    assert!((Smpte::parse("01:01:01:12", 25).unwrap() - 3661.48).abs() < 1e-9);
    assert!(
        (Smpte::parse("1:12", 25).unwrap() - 1.48).abs() < 1e-9,
        "fields count from the right"
    );
    assert_eq!(Smpte::parse("-0:12", 24), Some(-0.5));
    assert_eq!(Smpte::parse("00:00:00:25", 25), None, "frame out of range");
    assert_eq!(Smpte::parse("1:60:00:00", 25), None);
    assert_eq!(Smpte::parse("x", 25), None);
}

#[test]
fn seven_bit_values_upscale_by_the_midi2_rule() {
    assert_eq!(Velocity7::up(100), 0xC924);
    assert_eq!(Velocity7::up(127), 0xFFFF);
    assert_eq!(Velocity7::up(64), 0x8000);
    assert!(Velocity7::up(200) == 0xFFFF && Velocity7::up(-5) == 0);
    assert_eq!(Velocity7::of(0xC924), 100);
    assert!(Value7::up(127) == 0xFFFF_FFFF && Value7::up(64) == 0x8000_0000);
    assert_eq!(Value7::of(i64::from(Value7::up(100))), 100);
    assert!(
        Bend14::up(0) == 0x8000_0000 && Bend14::up(8191) == 0xFFFF_FFFF && Bend14::up(-8192) == 0
    );
    assert_eq!(Bend14::of(i64::from(Bend14::up(-100))), -100);
}

#[test]
fn pitch_and_key_names_parse() {
    assert_eq!(PitchName::parse("C4"), Some(60));
    assert_eq!(PitchName::parse("c#4"), Some(61));
    assert_eq!(PitchName::parse("Db4"), Some(61));
    assert_eq!(PitchName::parse("bb3"), Some(58));
    assert_eq!(PitchName::parse("C-1"), Some(0));
    assert_eq!(PitchName::parse("G9"), Some(127));
    assert_eq!(PitchName::parse("G#9"), None);
    assert_eq!(PitchName::parse("64"), Some(64));
    assert!(
        PitchName::parse("H2").is_none()
            && PitchName::parse("C").is_none()
            && PitchName::parse("200").is_none()
    );
    assert_eq!(KeyName::name(2, false), "D major");
    assert_eq!(KeyName::name(-3, true), "C minor");
    assert_eq!(KeyName::parse("D major"), Some((2, false)));
    assert_eq!(KeyName::parse("c minor"), Some((-3, true)));
    assert_eq!(KeyName::parse("F# minor"), Some((3, true)));
    assert_eq!(KeyName::parse("Gb major"), Some((-6, false)));
    assert!(KeyName::parse("H major").is_none() && KeyName::parse("C").is_none());
}

#[test]
fn huge_octaves_are_refused() {
    for bad in [
        "C99999999999999999999",
        "C9223372036854775807",
        "C-9223372036854775808",
    ] {
        assert_eq!(PitchName::parse(bad), None, "{bad}");
    }
}

#[test]
fn smpte_every_frame_round_trips() {
    for fps in Smpte::RATES {
        for n in 0..fps * 5 {
            let t = Smpte::text(f64::from(n) / f64::from(fps), fps);
            let s = Smpte::parse(&t, fps);
            assert!(s.is_some_and(|s| Smpte::text(s, fps) == t), "{fps} {n} {t}");
        }
    }
}
