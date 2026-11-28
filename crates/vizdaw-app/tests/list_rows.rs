//! Swift `ListRowsTests`: row order, ranges and filters, columns, cell
//! text of every kind, and a long list.

mod support;

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use support::four_four;
use vizdaw_app::list::{
    Bend14, ListColumn, ListColumns, ListContext, ListFilter, ListInput, ListKind, ListLane,
    ListRow, ListSource, ListText, Value7, Velocity7, build_rows,
};
use vizdaw_ffi::edit::{LaneRef, ViewFilterRow};
use vizdaw_ffi::events::{
    EventKindRow, EventRow, MarkerRow, MeterEntryRow, TempoPointRow, TextKindRow,
};

const Q: i64 = 15_360;
const LANE_A: LaneRef = LaneRef { seq: 3, lane: 31 };
const LANE_B: LaneRef = LaneRef { seq: 3, lane: 32 };

fn ev(id: u64, tick: i64, kind: EventKindRow) -> EventRow {
    EventRow::blank(id, tick, kind)
}

fn context() -> ListContext {
    let mut info = four_four(3);
    info.tempo.push(TempoPointRow {
        tick: 16 * Q,
        bpm: 90.,
        ramp: true,
    });
    info.markers = vec![MarkerRow {
        tick: Q,
        name: "Verse".into(),
    }];
    let mut c = ListContext::new(3);
    c.meter = info.meter.clone();
    c.info = Some(info);
    c.lane_names = [(LANE_A, "Chords".into()), (LANE_B, "Bass".into())].into();
    c.dest_names = [(5, "Piano".into()), (6, "Bass Synth".into())].into();
    c.seq_names = [(2, "Groove".into())].into();
    c
}

fn lane(key: LaneRef, events: Vec<EventRow>, dest: u64) -> ListLane {
    ListLane {
        key,
        events: events.into(),
        dest: Some(dest),
    }
}

fn ids(rows: &[ListRow]) -> Vec<&str> {
    rows.iter().map(ListRow::id).collect()
}

fn dests(d: &[u64]) -> Option<BTreeSet<u64>> {
    Some(d.iter().copied().collect())
}

#[test]
fn rows_are_sorted_with_map_rows_first() {
    let a = lane(
        LANE_A,
        vec![ev(6, 0, EventKindRow::Cc), ev(5, Q, EventKindRow::Note)],
        5,
    );
    let b = lane(LANE_B, vec![ev(7, 0, EventKindRow::Note)], 6);
    let input = ListInput {
        lanes: vec![a, b],
        range: None,
        info: context().info,
    };
    let rows = build_rows(&input, &ListFilter::default());
    assert_eq!(ids(&rows), ["t0", "m0", "e6", "e7", "k0", "e5", "t1"]);
    assert!(rows[2].lane() == Some(LANE_A) && rows[3].lane() == Some(LANE_B));
}

#[test]
fn block_ranges_and_filters_leave_rows_out() {
    let text = EventRow {
        text: Some("hi".into()),
        text_kind: Some(TextKindRow::Lyric),
        ..ev(8, 0, EventKindRow::Text)
    };
    let a = lane(
        LANE_A,
        vec![
            ev(6, 0, EventKindRow::Cc),
            ev(5, Q, EventKindRow::Note),
            text,
        ],
        5,
    );
    let b = lane(LANE_B, vec![ev(7, 0, EventKindRow::Note)], 6);
    let only = |lanes: Vec<ListLane>, range, filter: ListFilter| {
        build_rows(
            &ListInput {
                lanes,
                range,
                info: None,
            },
            &filter,
        )
    };
    let in_block = only(vec![a.clone()], Some(0..Q), ListFilter::default());
    assert_eq!(
        ids(&in_block),
        ["e6", "e8"],
        "the note at q starts after the block"
    );
    let no_cc = only(
        vec![a.clone(), b.clone()],
        None,
        ListFilter::new([ListKind::Cc], None),
    );
    assert!(!ids(&no_cc).contains(&"e6"));
    let piano = only(vec![a, b], None, ListFilter::new([], dests(&[5])));
    assert_eq!(
        ids(&piano),
        ["e6", "e8", "e5"],
        "Bass's note is hidden; text ignores destinations"
    );
    let routed = EventRow {
        dest: Some(6),
        ..ev(9, 0, EventKindRow::Note)
    };
    let own = only(
        vec![lane(LANE_A, vec![routed], 5)],
        None,
        ListFilter::new([], dests(&[5])),
    );
    assert!(
        own.is_empty(),
        "an event's own destination wins over its lane's"
    );
    assert_eq!(
        ListFilter::new([ListKind::Cc, ListKind::Tempo], None).view_filter(),
        Some(ViewFilterRow {
            hidden_kinds: vec![EventKindRow::Cc],
            hidden_lanes: vec![]
        })
    );
    assert_eq!(ListFilter::default().view_filter(), None);
}

#[test]
fn optional_columns_and_the_lane_column() {
    use ListColumn::*;
    assert_eq!(
        ListColumns::default().visible(1),
        [Position, Duration, Kind, Dest, Data1, Data2, Mute]
    );
    let all = ListColumns {
        end: true,
        release: true,
        smpte: true,
        ..ListColumns::default()
    };
    assert_eq!(
        all.visible(2),
        [
            Position, End, Duration, Kind, Lane, Dest, Data1, Data2, Release, Smpte, Mute
        ]
    );
    let titles: Vec<_> = ListColumn::ALL.iter().map(|c| c.title()).collect();
    assert_eq!(
        titles,
        [
            "Position",
            "End",
            "Length",
            "Kind",
            "Track",
            "Destination",
            "Data 1",
            "Data 2",
            "Release",
            "SMPTE",
            "Mute"
        ]
    );
    let kinds: Vec<_> = ListKind::ALL.iter().map(|k| k.title()).collect();
    assert_eq!(
        kinds,
        [
            "Note",
            "Controller",
            "Pitch Bend",
            "Channel Pressure",
            "Poly Pressure",
            "Program",
            "SysEx",
            "Text",
            "Reference",
            "Audio Region",
            "Tempo",
            "Meter and Key",
            "Marker",
            "Automation"
        ]
    );
}

#[test]
fn cells_show_every_kind() {
    let c = context();
    let row = |e: &EventRow| ListRow::event_row(LANE_A, e.clone());
    let cells = |e: &EventRow| {
        let r = row(e);
        [ListColumn::Kind, ListColumn::Data1, ListColumn::Data2]
            .map(|col| ListText::cell(&r, col, &c))
    };
    let note = EventRow {
        pitch: 60,
        velocity: Velocity7::up(100),
        duration: Q + Q / 4,
        ..ev(1, Q, EventKindRow::Note)
    };
    let nr = row(&note);
    assert_eq!(cells(&note), ["Note", "C4", "100"]);
    assert_eq!(ListText::cell(&nr, ListColumn::Position, &c), "1.2.0");
    assert_eq!(ListText::cell(&nr, ListColumn::Duration, &c), "1.240");
    assert_eq!(ListText::cell(&nr, ListColumn::End, &c), "1.3.240");
    assert_eq!(ListText::cell(&nr, ListColumn::Dest, &c), "Track");
    assert_eq!(ListText::cell(&nr, ListColumn::Lane, &c), "Chords");
    assert_eq!(
        ListText::summary(&nr, &c),
        "Note C4, bar 1 beat 2, velocity 100"
    );
    let cc = EventRow {
        value2: 7,
        value: i64::from(Value7::up(64)),
        dest: Some(6),
        ..ev(2, 0, EventKindRow::Cc)
    };
    assert_eq!(cells(&cc), ["Controller", "7", "64"]);
    assert_eq!(
        ListText::cell(&row(&cc), ListColumn::Dest, &c),
        "Bass Synth"
    );
    let pb = EventRow {
        value: i64::from(Bend14::up(-100)),
        ..ev(3, 0, EventKindRow::PitchBend)
    };
    assert_eq!(cells(&pb), ["Pitch Bend", "", "-100"]);
    let program = EventRow {
        value: 9,
        value2: 5,
        ..ev(4, 0, EventKindRow::Program)
    };
    assert_eq!(cells(&program), ["Program", "9", "0:5"]);
    let sysex = EventRow {
        data: vec![0x7E, 0x7F, 0x09, 0x01],
        ..ev(5, 0, EventKindRow::SysEx)
    };
    assert_eq!(cells(&sysex), ["SysEx", "4 bytes", "7E 7F 09 01"]);
    let text = EventRow {
        text: Some("verse".into()),
        text_kind: Some(TextKindRow::Lyric),
        ..ev(6, 0, EventKindRow::Text)
    };
    assert_eq!(cells(&text), ["Lyric", "Lyric", "verse"]);
    let mut reference = EventRow {
        ref_seq: Some(2),
        ref_loops: 3,
        ref_transpose: 2,
        ..ev(7, 0, EventKindRow::Reference)
    };
    assert_eq!(cells(&reference), ["Reference", "Groove", "\u{d7}3 +2"]);
    reference.ref_loops = 0;
    reference.ref_transpose = -1;
    assert_eq!(cells(&reference)[2], "fill -1");
    let region = EventRow {
        gain_db: -3.5,
        ..ev(8, 0, EventKindRow::AudioRegion)
    };
    assert_eq!(cells(&region), ["Audio Region", "Audio", "-3.5 dB"]);
    let info = c.info.clone().unwrap();
    let tempo = ListRow::new(ListSource::Tempo {
        index: 1,
        point: info.tempo[1],
    });
    assert_eq!(
        [ListColumn::Data1, ListColumn::Data2].map(|col| ListText::cell(&tempo, col, &c)),
        ["90.00", "ramp"]
    );
    let meter = ListRow::new(ListSource::Meter {
        index: 0,
        entry: MeterEntryRow {
            tick: 0,
            num: 6,
            den: 8,
            sharps: 2,
            minor: false,
        },
    });
    assert_eq!(
        [ListColumn::Data1, ListColumn::Data2].map(|col| ListText::cell(&meter, col, &c)),
        ["6/8", "D major"]
    );
    let marker = ListRow::new(ListSource::Marker {
        index: 0,
        marker: info.markers[0].clone(),
    });
    assert_eq!(ListText::cell(&marker, ListColumn::Data2, &c), "Verse");
    assert!(
        !ListText::editable(&tempo, ListColumn::Kind)
            && ListText::editable(&tempo, ListColumn::Position)
    );
    let first = ListRow::new(ListSource::Tempo {
        index: 0,
        point: info.tempo[0],
    });
    assert!(
        !ListText::editable(&first, ListColumn::Position),
        "the first tempo stays at 0"
    );
    assert!(!ListText::editable(&row(&sysex), ListColumn::Data2));
    // References and regions are shown, not edited, here.
    for e in [&reference, &region] {
        for col in ListColumn::ALL {
            assert!(!ListText::editable(&row(e), col), "{:?} {col:?}", e.kind);
        }
    }
    // An automation point takes its position, mute and value (Swift
    // `ListRows.swift:399-420`).
    let automation = ev(9, 0, EventKindRow::Automation);
    for col in ListColumn::ALL {
        let want = matches!(
            col,
            ListColumn::Position | ListColumn::Smpte | ListColumn::Mute | ListColumn::Data2
        );
        assert_eq!(ListText::editable(&row(&automation), col), want, "{col:?}");
    }
}

#[test]
fn building_rows_for_a_hundred_thousand_events_is_quick() {
    let events: Vec<EventRow> = (0..100_000)
        .map(|i| ev(i, i as i64 * 240, EventKindRow::Note))
        .collect();
    let input = ListInput {
        lanes: vec![lane(LANE_A, events, 5)],
        range: None,
        info: None,
    };
    let start = Instant::now();
    let rows = build_rows(&input, &ListFilter::default());
    assert_eq!(rows.len(), 100_000);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "debug build bound: {:?}",
        start.elapsed()
    );
}
