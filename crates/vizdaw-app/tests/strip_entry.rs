//! The typed-value grammars and the mixer's arithmetic: Swift
//! `StripEntryTests` and `MixerMathTests`.

use vizdaw_app::mixer::{AutomationModes, MixerMath, StripEntry};
use vizdaw_ffi::automation::AutomationModeRow;
use vizdaw_ffi::mixer::BusRow;

#[test]
fn levels_accept_decibels_with_or_without_the_unit() {
    for text in ["-4.2", "\u{2212}4.2 dB", "-4.2db", " -4.2 DB "] {
        assert_eq!(StripEntry::level(text), Some(-4.2), "{text}");
    }
    assert_eq!(StripEntry::level("+3"), Some(3.0));
    assert_eq!(StripEntry::level("0"), Some(0.0));
    assert_eq!(StripEntry::level(".5"), Some(0.5));
    assert_eq!(StripEntry::level("-4.25"), Some(-4.3));
}

#[test]
fn levels_clamp_and_read_minus_infinity() {
    assert_eq!(StripEntry::level("40"), Some(12.0));
    for text in [
        "-200",
        "-144",
        "-inf",
        "\u{2212}\u{221E}",
        "-\u{221E} dB",
        "\u{2212}inf",
        "-INF dB",
    ] {
        assert_eq!(StripEntry::level(text), Some(-144.0), "{text}");
    }
    assert_eq!(StripEntry::level_text(-144.0), "\u{2212}\u{221E} dB");
    for text in ["-0.04", "-0"] {
        let v = StripEntry::level(text).unwrap();
        assert!(v == 0.0 && v.is_sign_positive(), "{text}");
    }
    assert_eq!(StripEntry::level_text(-0.0), "0.0 dB");
    assert_eq!(StripEntry::level_text(-4.2), "\u{2212}4.2 dB");
    assert_eq!(StripEntry::level_text(3.0), "+3.0 dB");
}

#[test]
fn hostile_levels_are_refused() {
    let long = "1".repeat(33);
    let huge = "9".repeat(300);
    let bad = [
        "",
        "   ",
        "abc",
        "4.2.1",
        "--3",
        "+-3",
        "nan",
        "NaN",
        "inf",
        "+inf",
        "1e3",
        "1e309",
        "1,5",
        "-4.2 dBFS",
        "-4.2 dB dB",
        "4\u{0}",
        &long,
        &huge,
        "\u{FF14}",
        "\u{0663}",
        "4.",
        "- 4",
        "4\u{200B}",
    ];
    for text in bad {
        assert_eq!(
            StripEntry::level(text),
            None,
            "{}",
            &text[..text.len().min(20)]
        );
    }
}

#[test]
fn thirty_two_scalars_are_taken() {
    let text = format!("-{}", "0".repeat(31));
    assert_eq!(text.chars().count(), 32);
    assert_eq!(StripEntry::level(&text), Some(0.0));
}

#[test]
fn pans_accept_left_centre_right() {
    for text in ["C", "c", "0", "L0"] {
        assert_eq!(StripEntry::pan(text), Some(0.0), "{text}");
    }
    for text in ["L24", "L 24", "l 24", "-24", "\u{2212}24"] {
        assert_eq!(StripEntry::pan(text), Some(-0.24), "{text}");
    }
    for text in ["R10", "r 10", "10", "+10"] {
        assert_eq!(StripEntry::pan(text), Some(0.10), "{text}");
    }
    assert_eq!(StripEntry::pan("L150"), Some(-1.0));
    assert_eq!(StripEntry::pan("L 24.6"), Some(-0.25));
    let r = StripEntry::pan("R 0.4").unwrap();
    assert!(r == 0.0 && r.is_sign_positive());
}

#[test]
fn hostile_pans_are_refused() {
    let long = "L".repeat(33);
    let bad = [
        "",
        "LR",
        "L",
        "L-5",
        "L R",
        "LL24",
        "24L",
        "C 5",
        "nan",
        "L nan",
        "\u{221E}",
        "L1e2",
        "R\u{7}",
        "L \u{FF12}\u{FF14}",
        "R \u{0663}",
        &long,
        "L  24",
    ];
    for text in bad {
        assert_eq!(StripEntry::pan(text), None, "{text:?}");
    }
}

#[test]
fn a_level_that_rounds_to_the_floor_reads_minus_infinity() {
    assert_eq!(StripEntry::level_text(-143.96), "\u{2212}\u{221E} dB");
    assert_eq!(StripEntry::level_text(-143.94), "\u{2212}143.9 dB");
    let back = StripEntry::level(&StripEntry::level_text(-143.96));
    assert_eq!(back, StripEntry::level("\u{2212}\u{221E} dB"));
    assert_eq!(back, Some(-144.0));
}

#[test]
fn typed_values_read_back_as_they_show() {
    for db in [-60.0, -4.2, 0.0, 12.0] {
        assert_eq!(
            StripEntry::level(&StripEntry::level_text(db)),
            Some(db),
            "{db}"
        );
    }
    for p in [-1.0, -0.24, 0.0, 0.1] {
        assert_eq!(StripEntry::pan(&StripEntry::pan_text(p)), Some(p), "{p}");
    }
    assert_eq!(StripEntry::pan_text(-0.24), "L 24");
    assert_eq!(StripEntry::pan_text(0.0), "C");
    assert_eq!(StripEntry::pan_text(0.1), "R 10");
    assert_eq!(StripEntry::pan_text(f64::NAN), "C");
    assert_eq!(StripEntry::level_text(f64::NAN), "\u{2212}\u{221E} dB");
    assert_eq!(StripEntry::level_text(40.0), "+12.0 dB", "clamped at +12");
}

#[test]
fn the_hints_are_swifts() {
    assert_eq!(
        StripEntry::LEVEL_HINT,
        "Type a level in dB, like \u{2212}4.2 or \u{2212}inf."
    );
    assert_eq!(StripEntry::PAN_HINT, "Type a pan like L 24, C or R 10.");
    assert_eq!(StripEntry::TOUCHED_HINT, "Release the control first.");
}

#[test]
fn fader_positions_round_trip_and_floor_is_minus_infinity() {
    assert_eq!(MixerMath::fader_position(-144.0), 0.0);
    assert_eq!(MixerMath::fader_position(12.0), 1.0);
    assert_eq!(MixerMath::fader_position(f64::NAN), 0.0);
    assert_eq!(MixerMath::fader_db(0.0), -144.0);
    for db in [-59.5, -24.5, -3.0, 0.0, 6.0, 12.0] {
        let back = MixerMath::fader_db(MixerMath::fader_position(db));
        assert!((back - db).abs() < 0.06, "{db}");
    }
}

#[test]
fn labels() {
    assert_eq!(MixerMath::db_label(-144.0), "\u{2212}\u{221E}");
    assert_eq!(MixerMath::db_label(3.5), "+3.5");
    assert_eq!(MixerMath::db_label(-12.0), "-12.0");
    assert_eq!(MixerMath::db_label(0.0), "0.0");
    assert_eq!(MixerMath::db_label(f64::NAN), "\u{2212}\u{221E}");
    assert_eq!(MixerMath::pan_label(0.0), "C");
    assert_eq!(MixerMath::pan_label(-0.5), "L50");
    assert_eq!(MixerMath::pan_label(0.25), "R25");
    assert_eq!(MixerMath::meter_fill(0.0), 0.0);
    let full = MixerMath::meter_fill(1.0);
    assert!(full > 0.9 && full < 0.91);
    assert_eq!(MixerMath::meter_fill(f32::NAN), 0.0);
}

#[test]
fn new_bus_names_take_the_lowest_free_number() {
    let bus = |id, name: &str| BusRow {
        id,
        name: name.into(),
    };
    assert_eq!(MixerMath::next_bus_name(&[]), "Bus 1");
    assert_eq!(
        MixerMath::next_bus_name(&[bus(2, "Bus 1"), bus(3, "Bus 3")]),
        "Bus 2"
    );
    assert_eq!(MixerMath::next_bus_name(&[bus(2, "Drums")]), "Bus 1");
}

#[test]
fn modes_have_swifts_titles_in_menu_order() {
    let titles: Vec<&str> = AutomationModes::ALL
        .iter()
        .map(|m| AutomationModes::title(*m))
        .collect();
    assert_eq!(titles, ["Off", "Read", "Touch", "Latch", "Write", "Trim"]);
    assert_eq!(AutomationModes::ALL[2], AutomationModeRow::Touch);
    assert_eq!(AutomationModes::FAILED, "Automation Mode Failed");
}
