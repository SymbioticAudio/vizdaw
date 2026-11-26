//! Swift `ListEditTests`: typed cell text to commands, inserts and deletes.

mod support;

use std::sync::Arc;

use support::four_four;
use vizdaw_app::list::{
    Bend14, ListChange, ListColumn, ListContext, ListDelete, ListEdit, ListInsert, ListKind,
    ListRow, ListSource, ListText, Value7, Velocity7,
};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef, SequenceEdit};
use vizdaw_ffi::events::{
    EventKindRow, EventRow, MarkerRow, MeterEntryRow, TempoPointRow, TextKindRow,
};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;
const LANE_A: LaneRef = LaneRef { seq: 3, lane: 31 };

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
    c.dest_names = [(5, "Piano".into()), (6, "Bass Synth".into())].into();
    c.seq_names = [(2, "Groove".into())].into();
    c
}

fn note_row_at(tick: i64) -> ListRow {
    ListRow::event_row(
        LANE_A,
        EventRow {
            pitch: 60,
            velocity: Velocity7::up(64),
            duration: Q,
            ..ev(1, tick, EventKindRow::Note)
        },
    )
}

fn event_row(e: EventRow) -> ListRow {
    ListRow::event_row(LANE_A, e)
}

fn commit_in(text: &str, col: ListColumn, r: &ListRow, c: &ListContext) -> ListChange {
    ListEdit::commit(text, col, r, c).unwrap()
}

fn commit(text: &str, col: ListColumn, r: &ListRow) -> ListChange {
    commit_in(text, col, r, &context())
}

fn message_in(text: &str, col: ListColumn, r: &ListRow, c: &ListContext) -> Option<String> {
    ListEdit::commit(text, col, r, c).err().map(|e| e.0)
}

fn message(text: &str, col: ListColumn, r: &ListRow) -> Option<String> {
    message_in(text, col, r, &context())
}

fn replaced(r: &ListRow, change: impl FnOnce(&mut EventRow)) -> ListChange {
    let mut n = r.event().unwrap().clone();
    change(&mut n);
    let tick = n.tick;
    ListChange::Edit(
        EditCommand::Event {
            edit: EventEdit::Replace {
                event: n.id,
                row: n,
            },
        },
        tick,
    )
}

fn moved(delta: i64, tick: i64) -> ListChange {
    ListChange::Edit(
        EditCommand::Event {
            edit: EventEdit::Move {
                events: vec![1],
                delta,
                to_lane: None,
            },
        },
        tick,
    )
}

fn seq_edit(edit: SequenceEdit, tick: i64) -> ListChange {
    ListChange::Edit(EditCommand::Sequence { edit }, tick)
}

#[test]
fn typing_a_bar_number_moves_to_its_downbeat() {
    let r = note_row_at(Q + 100);
    assert_eq!(
        commit("5", ListColumn::Position, &r),
        moved(4 * BAR - (Q + 100), 4 * BAR)
    );
    // q + 100 is 1.2.6; ".3" changes the beat only: 1.3.6 = 2q + 96.
    assert_eq!(
        commit(".3", ListColumn::Position, &r),
        moved(Q - 4, 2 * Q + 96)
    );
    assert_eq!(
        commit("1.2.6", ListColumn::Position, &r),
        ListChange::Unchanged
    );
}

#[test]
fn committing_shown_text_sends_no_edit() {
    let r = note_row_at(Q + 100);
    let c = context();
    // The position shows 1.2.6 (the 4 ticks off the unit grid are not
    // shown); typing it back must not move.
    for col in [
        ListColumn::Position,
        ListColumn::End,
        ListColumn::Duration,
        ListColumn::Data1,
        ListColumn::Data2,
        ListColumn::Release,
        ListColumn::Kind,
    ] {
        let shown = ListText::cell(&r, col, &c);
        assert_eq!(
            commit_in(&shown, col, &r, &c),
            ListChange::Unchanged,
            "{col:?}"
        );
    }
}

#[test]
fn equal_positions_written_differently_change_nothing() {
    let r = note_row_at(Q + 100);
    for text in [".2", "1.2.06", "1..6", "1.2.6"] {
        assert_eq!(
            commit(text, ListColumn::Position, &r),
            ListChange::Unchanged,
            "{text}"
        );
    }
    assert_eq!(commit("1.3.06", ListColumn::End, &r), ListChange::Unchanged);
    assert_eq!(
        commit("1.00", ListColumn::Duration, &r),
        ListChange::Unchanged
    );
}

#[test]
fn text_keeps_its_spaces() {
    let row = event_row(EventRow {
        text: Some("a".into()),
        text_kind: Some(TextKindRow::Text),
        ..ev(5, 0, EventKindRow::Text)
    });
    assert_eq!(
        commit(" b ", ListColumn::Data2, &row),
        replaced(&row, |n| n.text = Some(" b ".into()))
    );
    assert_eq!(commit("a", ListColumn::Data2, &row), ListChange::Unchanged);
    let info = context().info.unwrap();
    let marker = ListRow::new(ListSource::Marker {
        index: 0,
        marker: info.markers[0].clone(),
    });
    let mut markers = info.markers.clone();
    markers[0].name = " Chorus ".into();
    assert_eq!(
        commit(" Chorus ", ListColumn::Data2, &marker),
        seq_edit(SequenceEdit::SetMarkers { seq: 3, markers }, Q)
    );
}

#[test]
fn note_cells_replace_the_event() {
    let r = note_row_at(0);
    assert_eq!(
        commit("D4", ListColumn::Data1, &r),
        replaced(&r, |n| n.pitch = 62)
    );
    assert_eq!(
        commit("100", ListColumn::Data2, &r),
        replaced(&r, |n| n.velocity = Velocity7::up(100))
    );
    assert_eq!(
        commit("64", ListColumn::Data2, &r),
        ListChange::Unchanged,
        "the shown value keeps its 16 bits"
    );
    assert_eq!(
        commit("2.0", ListColumn::Duration, &r),
        replaced(&r, |n| n.duration = 2 * Q)
    );
    assert_eq!(
        commit("1.3", ListColumn::End, &r),
        replaced(&r, |n| n.duration = 2 * Q)
    );
    assert_eq!(
        commit("90", ListColumn::Release, &r),
        replaced(&r, |n| n.release_velocity = Velocity7::up(90))
    );
    assert_eq!(
        commit("M", ListColumn::Mute, &r),
        ListChange::Edit(
            EditCommand::Event {
                edit: EventEdit::SetMute {
                    events: vec![1],
                    mute: true
                }
            },
            0
        )
    );
    assert_eq!(
        commit("bass synth", ListColumn::Dest, &r),
        ListChange::Edit(
            EditCommand::Event {
                edit: EventEdit::SetDest {
                    events: vec![1],
                    dest: Some(6)
                }
            },
            0
        )
    );
    assert_eq!(
        commit("", ListColumn::Dest, &r),
        ListChange::Unchanged,
        "already the lane's"
    );
}

#[test]
fn other_kinds_edit_their_fields() {
    let cc = event_row(ev(2, 0, EventKindRow::Cc));
    assert_eq!(
        commit("11", ListColumn::Data1, &cc),
        replaced(&cc, |n| n.value2 = 11)
    );
    assert_eq!(
        commit("127", ListColumn::Data2, &cc),
        replaced(&cc, |n| n.value = i64::from(Value7::up(127)))
    );
    let pb = event_row(ev(3, 0, EventKindRow::PitchBend));
    assert_eq!(
        commit("-8192", ListColumn::Data2, &pb),
        ListChange::Unchanged,
        "0 is the lowest bend"
    );
    assert_eq!(
        commit("0", ListColumn::Data2, &pb),
        replaced(&pb, |n| n.value = i64::from(Bend14::up(0)))
    );
    let program = event_row(EventRow {
        value2: -1,
        ..ev(4, 0, EventKindRow::Program)
    });
    assert_eq!(
        commit("1:2", ListColumn::Data2, &program),
        replaced(&program, |n| n.value2 = 130)
    );
    let text = event_row(EventRow {
        text: Some("a".into()),
        text_kind: Some(TextKindRow::Text),
        ..ev(5, 0, EventKindRow::Text)
    });
    assert_eq!(
        commit("Cue", ListColumn::Data1, &text),
        replaced(&text, |n| n.text_kind = Some(TextKindRow::Cue))
    );
    assert_eq!(
        commit("b", ListColumn::Data2, &text),
        replaced(&text, |n| n.text = Some("b".into()))
    );
    let reference = event_row(EventRow {
        ref_loops: 1,
        ..ev(6, 0, EventKindRow::Reference)
    });
    assert_eq!(
        commit("\u{d7}3 +2", ListColumn::Data2, &reference),
        replaced(&reference, |n| {
            n.ref_loops = 3;
            n.ref_transpose = 2;
        })
    );
    assert_eq!(
        commit("fill", ListColumn::Data2, &reference),
        replaced(&reference, |n| n.ref_loops = 0)
    );
    let region = event_row(ev(7, 0, EventKindRow::AudioRegion));
    assert_eq!(
        commit("-6 dB", ListColumn::Data2, &region),
        replaced(&region, |n| n.gain_db = -6.)
    );
}

#[test]
fn bad_text_is_refused_with_a_message() {
    let r = note_row_at(0);
    assert!(
        message("abc", ListColumn::Position, &r)
            .unwrap()
            .contains("not a position")
    );
    assert_eq!(
        message("abc", ListColumn::Position, &r).as_deref(),
        Some("\"abc\" is not a position: type a bar, bar.beat or bar.beat.unit.")
    );
    assert_eq!(
        message("300", ListColumn::Data2, &r).as_deref(),
        Some("Velocity must be 0 to 127.")
    );
    assert!(
        message("H9", ListColumn::Data1, &r)
            .unwrap()
            .contains("not a pitch")
    );
    assert_eq!(
        message("0.4.0", ListColumn::End, &r).as_deref(),
        Some("The end is before the start.")
    );
    assert_eq!(
        message("Organ", ListColumn::Dest, &r).as_deref(),
        Some("No destination is named \"Organ\".")
    );
    assert_eq!(
        message("maybe", ListColumn::Mute, &r).as_deref(),
        Some("Type M to mute, or clear the cell.")
    );
    let c = context();
    let tempo = ListRow::new(ListSource::Tempo {
        index: 1,
        point: c.info.as_ref().unwrap().tempo[1],
    });
    for t in ["0", "3"] {
        assert_eq!(
            message(t, ListColumn::Data1, &tempo).as_deref(),
            Some("Tempo must be 5 to 999 BPM.")
        );
    }
    let region = event_row(ev(7, 0, EventKindRow::AudioRegion));
    assert_eq!(
        message("+30 dB", ListColumn::Data2, &region).as_deref(),
        Some("Gain must be -144 to 24 dB.")
    );
}

#[test]
fn map_rows_replace_their_whole_map() {
    let info = context().info.unwrap();
    let tempo = ListRow::new(ListSource::Tempo {
        index: 1,
        point: info.tempo[1],
    });
    let mut t = info.tempo.clone();
    t[1].bpm = 100.;
    assert_eq!(
        commit("100", ListColumn::Data1, &tempo),
        seq_edit(SequenceEdit::SetTempo { seq: 3, tempo: t }, 16 * Q)
    );
    let meter = ListRow::new(ListSource::Meter {
        index: 0,
        entry: info.meter[0],
    });
    let mut m = info.meter.clone();
    m[0].num = 6;
    m[0].den = 8;
    assert_eq!(
        commit("6/8", ListColumn::Data1, &meter),
        seq_edit(SequenceEdit::SetMeter { seq: 3, meter: m }, 0)
    );
    let mut m = info.meter.clone();
    m[0].sharps = 3;
    m[0].minor = true;
    assert_eq!(
        commit("F# minor", ListColumn::Data2, &meter),
        seq_edit(SequenceEdit::SetMeter { seq: 3, meter: m }, 0)
    );
    let marker = ListRow::new(ListSource::Marker {
        index: 0,
        marker: info.markers[0].clone(),
    });
    assert_eq!(
        commit("3", ListColumn::Position, &marker),
        seq_edit(
            SequenceEdit::SetMarkers {
                seq: 3,
                markers: vec![MarkerRow {
                    tick: 2 * BAR,
                    name: "Verse".into()
                }]
            },
            2 * BAR
        )
    );
    let first = ListRow::new(ListSource::Tempo {
        index: 0,
        point: info.tempo[0],
    });
    assert_eq!(
        commit("2", ListColumn::Position, &first),
        ListChange::Unchanged,
        "the first tempo stays at 0"
    );
}

#[test]
fn map_edits_the_core_would_reject_are_refused() {
    let mut c = context();
    let info = c.info.clone().unwrap();
    let tempo = ListRow::new(ListSource::Tempo {
        index: 1,
        point: info.tempo[1],
    });
    assert_eq!(
        message_in("1", ListColumn::Position, &tempo, &c).as_deref(),
        Some("A tempo change is already at that position.")
    );
    let mut info2 = info.clone();
    info2.tempo.push(TempoPointRow {
        tick: 8 * Q,
        bpm: 100.,
        ramp: false,
    });
    c.info = Some(info2);
    assert_eq!(
        message_in("3", ListColumn::Position, &tempo, &c).as_deref(),
        Some("A tempo change is already at that position.")
    );
    // Meters: 4/4 at 0, 3/4 at bar 3 (tick 8q).
    let mut info3 = info.clone();
    info3.meter.push(MeterEntryRow {
        tick: 2 * BAR,
        num: 3,
        den: 4,
        sharps: 0,
        minor: false,
    });
    c.meter = info3.meter.clone();
    c.info = Some(info3.clone());
    let second = ListRow::new(ListSource::Meter {
        index: 1,
        entry: info3.meter[1],
    });
    assert_eq!(
        message_in("1", ListColumn::Position, &second, &c).as_deref(),
        Some("A meter change is already at that position.")
    );
    assert_eq!(message_in("2", ListColumn::Position, &second, &c), None);
    assert_eq!(
        message_in("2.2", ListColumn::Position, &second, &c).as_deref(),
        Some("Meter changes must start on a barline.")
    );
    // 4/4 -> 3/4 first: bars shrink, tick 2*bar is no longer a barline.
    let first = ListRow::new(ListSource::Meter {
        index: 0,
        entry: info3.meter[0],
    });
    assert_eq!(
        message_in("3/4", ListColumn::Data1, &first, &c).as_deref(),
        Some("That meter would move a later meter change off its barline.")
    );
}

#[test]
fn typed_smpte_rounds_to_a_display_unit() {
    let mut c = context();
    c.fps = 25;
    // 1.48 s at 120 BPM is 45 465.6 ticks; a unit is 16 ticks: 45 472.
    c.tick_at_seconds = Arc::new(|s| Some((s * 30_720.).round() as i64));
    let r = note_row_at(0);
    assert_eq!(
        commit_in("00:00:01:12", ListColumn::Smpte, &r, &c),
        moved(45_472, 45_472)
    );
    assert_eq!(
        message_in("1:99", ListColumn::Smpte, &r, &c).as_deref(),
        Some("\"1:99\" is not an SMPTE time (hh:mm:ss:ff).")
    );
}

#[test]
fn insert_defaults_and_delete_commands() {
    let c = context();
    let info = c.info.clone().unwrap();
    let Some(EditCommand::Event {
        edit: EventEdit::Insert { events: rows, .. },
    }) = ListInsert::command(ListKind::Note, Q, Some(LANE_A), 3, Some(&info))
    else {
        panic!("no insert");
    };
    assert!(
        rows.len() == 1
            && rows[0].pitch == 60
            && rows[0].velocity == Velocity7::up(100)
            && rows[0].duration == Q
    );
    assert_eq!(
        ListInsert::command(ListKind::Reference, 0, Some(LANE_A), 3, Some(&info)),
        None
    );
    assert_eq!(
        ListInsert::command(ListKind::Note, 0, None, 3, Some(&info)),
        None
    );
    let Some(EditCommand::Sequence {
        edit: SequenceEdit::SetMeter { meter, .. },
    }) = ListInsert::command(ListKind::Meter, Q, None, 3, Some(&info))
    else {
        panic!("no meter");
    };
    assert_eq!(
        meter.iter().map(|m| m.tick).collect::<Vec<_>>(),
        [0, BAR],
        "the next barline"
    );
    let Some(EditCommand::Sequence {
        edit: SequenceEdit::SetMarkers { markers, .. },
    }) = ListInsert::command(ListKind::Marker, 0, None, 3, Some(&info))
    else {
        panic!("no marker");
    };
    assert_eq!(
        markers.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        ["Marker 2", "Verse"]
    );
    let doomed = [
        note_row_at(0),
        ListRow::new(ListSource::Tempo {
            index: 0,
            point: info.tempo[0],
        }),
        ListRow::new(ListSource::Tempo {
            index: 1,
            point: info.tempo[1],
        }),
        ListRow::new(ListSource::Marker {
            index: 0,
            marker: info.markers[0].clone(),
        }),
    ];
    assert_eq!(
        ListDelete::commands(&doomed, 3, Some(&info)),
        [
            EditCommand::Event {
                edit: EventEdit::Delete { events: vec![1] }
            },
            EditCommand::Sequence {
                edit: SequenceEdit::SetTempo {
                    seq: 3,
                    tempo: vec![info.tempo[0]]
                }
            },
            EditCommand::Sequence {
                edit: SequenceEdit::SetMarkers {
                    seq: 3,
                    markers: vec![]
                }
            },
        ]
    );
}

#[test]
fn hostile_text_never_traps() {
    let c = context();
    let r = note_row_at(0);
    let tempo = ListRow::new(ListSource::Tempo {
        index: 1,
        point: c.info.as_ref().unwrap().tempo[1],
    });
    for text in [
        "5..",
        "99999999999999999999",
        "1000000.999.959",
        "-1000000",
        "C99999999999",
        "\u{d7}",
        "::::",
        "-",
        "nan",
        "inf",
        "1e400",
        "9223372036854775807",
    ] {
        for col in ListColumn::ALL {
            let _ = ListEdit::commit(text, col, &r, &c);
            let _ = ListEdit::commit(text, col, &tempo, &c);
        }
    }
}
