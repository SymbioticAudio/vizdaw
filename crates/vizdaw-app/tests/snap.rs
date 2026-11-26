//! Swift `SnapTests`.

use vizdaw_app::editor::{Division, EditorLayout, Snap};
use vizdaw_ffi::events::MeterEntryRow;

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

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

#[test]
fn quarter_snaps_to_the_nearest_beat() {
    let s = Snap::default();
    assert_eq!(s.apply(Q * 4 / 10, &four_four()), 0);
    assert_eq!(s.apply(Q * 6 / 10, &four_four()), Q);
    assert_eq!(s.apply(Q / 2, &four_four()), Q, "ties go up");
}

#[test]
fn off_leaves_ticks_alone() {
    let s = Snap {
        enabled: false,
        ..Snap::default()
    };
    assert_eq!(s.apply(123, &four_four()), 123);
}

#[test]
fn triplets_and_dots() {
    let s = Snap {
        triplet: true,
        ..Snap::default()
    };
    assert_eq!(s.step(&four_four()[0]), Q * 2 / 3);
    let s = Snap {
        division: Division::Eighth,
        dotted: true,
        ..Snap::default()
    };
    assert_eq!(s.step(&four_four()[0]), Q * 3 / 4);
}

#[test]
fn bar_snap_follows_the_meter() {
    let s = Snap {
        division: Division::Bar,
        ..Snap::default()
    };
    let three_four = vec![meter(0, 3, 4)];
    assert_eq!(s.apply(30_000, &three_four), 3 * Q);
    assert_eq!(s.apply(20_000, &three_four), 0);
}

#[test]
fn a_grid_that_does_not_divide_the_bar_snaps_to_the_bar_line() {
    // 20 480 ticks: three per 4/4 bar, fits exactly.
    let s = Snap {
        division: Division::Half,
        triplet: true,
        ..Snap::default()
    };
    assert_eq!(s.apply(BAR - 100, &four_four()), BAR);
    // 30 720: a 5/4 bar ends 15 360 after the second half.
    let five_four = vec![meter(0, 5, 4)];
    let s = Snap {
        division: Division::Half,
        ..Snap::default()
    };
    assert_eq!(s.apply(5 * Q - 1_000, &five_four), 5 * Q);
}

#[test]
fn pickup_bars_snap_and_number_backwards() {
    let s = Snap::default();
    assert_eq!(s.apply(-Q * 4 / 10, &four_four()), 0);
    assert_eq!(s.apply(-BAR + 100, &four_four()), -BAR);
}

#[test]
fn snap_round_trips_through_json() {
    let s = Snap {
        division: Division::Sixteenth,
        triplet: true,
        ..Snap::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(serde_json::from_str::<Snap>(&json).unwrap(), s);
}

#[test]
fn the_thirty_second_is_snake_case_and_the_old_spelling_still_reads() {
    let s = Snap {
        division: Division::ThirtySecond,
        ..Snap::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains(r#""thirty_second""#) && !json.contains("thirtySecond"));
    let old = r#"{"division":"thirtySecond","triplet":false,"dotted":false,"enabled":true}"#;
    assert_eq!(
        serde_json::from_str::<Snap>(old).unwrap().division,
        Division::ThirtySecond
    );
    // A project saved before the snake-case spelling keeps its grid.
    let layout = EditorLayout::decode(Some(
        r#"{"snap":{"division":"thirtySecond","triplet":true,"dotted":false,"enabled":true}}"#,
    ));
    assert!(layout.snap.division == Division::ThirtySecond && layout.snap.triplet);
    let unknown = EditorLayout::decode(Some(
        r#"{"snap":{"division":"thirtysecond","triplet":true,"dotted":false,"enabled":true}}"#,
    ));
    assert_eq!(unknown.snap, Snap::default(), "unknown: the default");
}

#[test]
fn floor_takes_the_grid_line_at_or_before() {
    let s = Snap::default();
    assert_eq!(s.floor(15_359, &four_four()), 0);
    assert_eq!(s.floor(15_360, &four_four()), 15_360);
    assert_eq!(s.floor(-1, &four_four()), -15_360, "pickup bars");
    let s = Snap {
        enabled: false,
        ..Snap::default()
    };
    assert_eq!(s.floor(15_359, &four_four()), 15_359);
}

#[test]
fn divisions_have_their_titles_and_names() {
    let titles: Vec<_> = Division::ALL.iter().map(|d| d.title()).collect();
    assert_eq!(titles, ["Bar", "1/2", "1/4", "1/8", "1/16", "1/32"]);
    let names: Vec<_> = Division::ALL.iter().map(|d| d.name()).collect();
    assert_eq!(
        names,
        [
            "bar",
            "half",
            "quarter",
            "eighth",
            "sixteenth",
            "thirty_second"
        ]
    );
    let d = Snap::default();
    assert!(d.division == Division::Quarter && d.enabled && !d.triplet && !d.dotted);
}
