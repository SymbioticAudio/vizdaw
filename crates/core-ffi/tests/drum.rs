//! The drum grid through the FFI (Plan 4e Task 4): drum lanes, the Drum
//! edit group, strokes and row drops as commands, play quantize grooves.

use core_engine::fixtures::{Builder, Q};
use core_model::document::Document;
use core_model::ids::{LaneId, SeqId};
use proptest::prelude::*;
use std::sync::Arc;
use vizdaw_ffi::drum::{
    drum_grid_cells, drum_grid_hits_at, drum_grid_layout, drum_grid_spot, drum_next_row,
    drum_nudged, drum_quantize_choices, drum_quantize_title, drum_row_drop, drum_row_velocity,
    drum_stroke_begin, drum_stroke_command, drum_stroke_extend,
};
use vizdaw_ffi::{
    DrumBandKindRow, DrumBandRow, DrumEdit, DrumGridLayoutRow, DrumGridSettingsRow, DrumHitRow,
    DrumLaneRow, DrumPaintHitRow, DrumSpotRow, DrumStrokeRow, DrumViewRow, EditCommand, LaneEdit,
    QuantizeRow, Session, VizError,
};

const S16: i64 = Q / 4;

/// "Beat": drum lane "Drums" (Kick 36: hits at 0 and a beat; Snare 38:
/// a hit at a beat), drum lane "Perc" (Clap 39), plain lane "Keys".
fn beat() -> (Document, SeqId, LaneId, LaneId, LaneId) {
    let mut b = Builder::new();
    let kit = b.dest("Kit", "builtin:synth", 9).unwrap();
    let (seq, drums) = b.sequence("Beat", "Drums", Some(kit)).unwrap();
    b.drum_rows(seq, drums, &[("Kick", 36), ("Snare", 38)])
        .unwrap();
    b.notes(
        seq,
        drums,
        &[
            (0, 36, 0x8000, S16),
            (Q, 36, 0x7000, S16),
            (Q, 38, 0x6000, S16),
        ],
    )
    .unwrap();
    let perc = b.lane(seq, "Perc", Some(kit)).unwrap();
    b.drum_rows(seq, perc, &[("Clap", 39)]).unwrap();
    let keys = b.lane(seq, "Keys", Some(kit)).unwrap();
    b.notes(seq, keys, &[(0, 60, 0x8000, Q)]).unwrap();
    (b.finish(), seq, drums, perc, keys)
}

fn session() -> (Arc<Session>, u64, u64, u64, u64) {
    let (d, s, drums, perc, keys) = beat();
    (
        Session::from_document(d).unwrap(),
        s.0,
        drums.0,
        perc.0,
        keys.0,
    )
}

fn matrix() -> DrumGridSettingsRow {
    DrumGridSettingsRow {
        view: DrumViewRow::Matrix,
        step: S16,
        spacing: S16,
        duration: S16 / 2,
        velocity: 100,
        row_height: 20.0,
        velocity_height: 28.0,
    }
}

fn shapes(lanes: &[DrumLaneRow]) -> Vec<Vec<bool>> {
    lanes
        .iter()
        .map(|l| l.rows.iter().map(|r| r.show_velocity).collect())
        .collect()
}

fn drum(edit: DrumEdit) -> EditCommand {
    EditCommand::Drum { edit }
}

#[test]
fn drum_lanes_list_only_drum_lanes_with_their_rows_and_hits() {
    let (s, seq, drums, perc, _) = session();
    let lanes = s.drum_lanes(seq).unwrap();
    assert_eq!(
        lanes
            .iter()
            .map(|l| (l.lane, l.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(drums, "Drums"), (perc, "Perc")]
    );
    let d = &lanes[0];
    assert_eq!(d.destination.as_deref(), Some("Kit"));
    assert_eq!(
        (d.play_quantize.clone(), d.play_shift, d.mute, d.solo),
        (None, 0, false, false)
    );
    type Row<'a> = (&'a str, u8, Vec<(i64, u16)>);
    let rows: Vec<Row> = d
        .rows
        .iter()
        .map(|r| {
            (
                r.name.as_str(),
                r.note,
                r.hits.iter().map(|h| (h.tick, h.velocity)).collect(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("Kick", 36, vec![(0, 0x8000), (Q, 0x7000)]),
            ("Snare", 38, vec![(Q, 0x6000)]),
        ]
    );
    assert_eq!(d.rows[0].destination.as_deref(), Some("Kit"));
    assert!(lanes[1].rows[0].hits.is_empty());
    assert!(matches!(s.drum_lanes(999), Err(VizError::Model { .. })));
}

#[test]
fn every_drum_edit_applies_and_undoes() {
    let (s0, seq, drums, perc, keys) = session();
    let lanes = s0.drum_lanes(seq).unwrap();
    let (kick, snare) = (lanes[0].rows[0].id, lanes[0].rows[1].id);
    let all = [
        DrumEdit::MarkLane {
            seq,
            lane: keys,
            drum: true,
        },
        DrumEdit::MarkLane {
            seq,
            lane: perc,
            drum: false,
        },
        DrumEdit::AddRow {
            seq,
            lane: drums,
            name: "Hat".into(),
            note: 42,
            dest: None,
        },
        DrumEdit::RenameRow {
            seq,
            lane: drums,
            row: kick,
            name: "Bass Drum".into(),
        },
        DrumEdit::ShowVelocity {
            seq,
            lane: drums,
            row: snare,
            show: true,
        },
        DrumEdit::ReorderRow {
            seq,
            lane: drums,
            row: snare,
            to: 0,
        },
        DrumEdit::RemoveRow {
            seq,
            lane: drums,
            row: kick,
        },
        DrumEdit::MoveRowToLane {
            seq,
            lane: drums,
            row: snare,
            to_lane: perc,
        },
        DrumEdit::SplitRow {
            seq,
            lane: drums,
            row: kick,
        },
        DrumEdit::Paint {
            seq,
            lane: drums,
            row: snare,
            hits: vec![DrumPaintHitRow {
                tick: 0,
                velocity: 0x5000,
            }],
            window: S16,
            duration: S16,
        },
    ];
    let labels = [
        "Mark as Drum Lane",
        "Mark as Drum Lane",
        "Add Drum Row",
        "Rename Drum Row",
        "Show Drum Row Velocity",
        "Reorder Drum Row",
        "Remove Drum Row",
        "Move Drum Row to Lane",
        "Split Drum Row to Lane",
        "Paint Drum Hits",
    ];
    for (edit, label) in all.into_iter().zip(labels) {
        let (s, ..) = session();
        let before = (s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap());
        let snap = s
            .edit(drum(edit.clone()), None)
            .unwrap_or_else(|e| panic!("{edit:?}: {e}"));
        assert_eq!(snap.undo_label.as_deref(), Some(label), "{edit:?}");
        assert_ne!(
            (s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap()),
            before,
            "{edit:?}"
        );
        s.undo().unwrap();
        assert_eq!(
            (s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap()),
            before,
            "{edit:?}"
        );
    }
}

#[test]
fn a_click_on_an_empty_cell_paints_and_one_on_a_hit_erases() {
    let (s, seq, drums, ..) = session();
    let lanes = s.drum_lanes(seq).unwrap();
    let layout = drum_grid_layout(shapes(&lanes), matrix()).unwrap();
    // Snare row of lane 0: band 2 (header, kick, snare).
    let DrumSpotRow::Row {
        lane,
        row,
        velocity,
    } = drum_grid_spot(layout.clone(), 22.0 + 20.0 + 5.0).unwrap()
    else {
        panic!("not a row")
    };
    assert_eq!((lane, row, velocity), (0, 1, None));
    assert_eq!(
        drum_row_velocity(layout.clone(), 0, 1, 0.0).unwrap(),
        None,
        "no velocity lane shown"
    );
    let snare = &lanes[0].rows[1];
    // A press in the third cell, dragged to the sixth: four hits.
    let st = drum_stroke_begin(
        lane,
        row,
        snare.hits.clone(),
        2 * S16 + 9,
        0,
        1,
        None,
        false,
        matrix(),
    )
    .unwrap();
    let st = drum_stroke_extend(st, snare.hits.clone(), 5 * S16 + 1, 1, None, matrix()).unwrap();
    assert!(!st.erase);
    assert_eq!(
        st.hits.iter().map(|h| h.tick).collect::<Vec<_>>(),
        vec![2 * S16, 3 * S16, 4 * S16, 5 * S16]
    );
    let cmd = drum_stroke_command(st, seq, drums, snare.id, matrix())
        .unwrap()
        .unwrap();
    s.begin_edit("Paint Drum Hits".into()).unwrap();
    s.edit_gesture(cmd, None).unwrap();
    let snap = s.end_edit().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Paint Drum Hits"));
    let snare = s.drum_lanes(seq).unwrap()[0].rows[1].clone();
    // The hit at a beat (cell 4) was there: it stays, the others come in.
    assert_eq!(
        snare
            .hits
            .iter()
            .map(|h| (h.tick, h.duration, h.velocity))
            .collect::<Vec<_>>(),
        vec![
            (2 * S16, S16 / 2, 100 << 9),
            (3 * S16, S16 / 2, 100 << 9),
            (Q, S16, 0x6000),
            (5 * S16, S16 / 2, 100 << 9)
        ]
    );
    let cells = drum_grid_cells(snare.hits.clone(), 0, 6 * S16, S16).unwrap();
    assert_eq!(
        cells.iter().map(|c| !c.hits.is_empty()).collect::<Vec<_>>(),
        vec![false, false, true, true, true, true]
    );
    // A press on the hit at a beat erases it (a click: no drag).
    let st =
        drum_stroke_begin(0, 1, snare.hits.clone(), Q + 7, 0, 1, None, false, matrix()).unwrap();
    assert!(st.erase);
    let cmd = drum_stroke_command(st, seq, drums, snare.id, matrix())
        .unwrap()
        .unwrap();
    s.edit(cmd, None).unwrap();
    assert_eq!(s.drum_lanes(seq).unwrap()[0].rows[1].hits.len(), 3);
    // A stroke that covers nothing makes no command.
    let st = drum_stroke_begin(0, 1, vec![], 0, 0, 1, None, true, matrix()).unwrap();
    assert_eq!(
        drum_stroke_command(st, seq, drums, snare.id, matrix()).unwrap(),
        None
    );
    assert_eq!(
        drum_grid_hits_at(snare.hits, 2 * S16 + 5, 1).unwrap().len(),
        1
    );
}

#[test]
fn row_drops_reorder_move_and_split() {
    let (s, seq, drums, perc, _) = session();
    let lanes = s.drum_lanes(seq).unwrap();
    let ids: Vec<u64> = lanes.iter().map(|l| l.lane).collect();
    let layout = drum_grid_layout(shapes(&lanes), matrix()).unwrap();
    let kick = lanes[0].rows[0].id;
    // Bands: header 0–22, kick 22–42, snare 42–62, header 62–84, clap
    // 84–104, new lane 104–132.
    let at = |y: f64| drum_row_drop(layout.clone(), ids.clone(), seq, 0, kick, 0, y).unwrap();
    assert_eq!(
        at(50.0),
        Some(drum(DrumEdit::ReorderRow {
            seq,
            lane: drums,
            row: kick,
            to: 1
        }))
    );
    assert_eq!(
        at(90.0),
        Some(drum(DrumEdit::MoveRowToLane {
            seq,
            lane: drums,
            row: kick,
            to_lane: perc
        }))
    );
    assert_eq!(
        at(400.0),
        Some(drum(DrumEdit::SplitRow {
            seq,
            lane: drums,
            row: kick
        }))
    );
    assert_eq!(at(30.0), None);
    assert_eq!(
        drum_row_drop(layout.clone(), ids.clone(), seq, 9, kick, 0, 50.0).unwrap(),
        None,
        "an unknown lane index"
    );
    s.edit(at(400.0).unwrap(), None).unwrap();
    let after = s.drum_lanes(seq).unwrap();
    assert_eq!(
        after
            .iter()
            .map(|l| (l.name.as_str(), l.rows.len()))
            .collect::<Vec<_>>(),
        vec![("Drums", 1), ("Kick", 1), ("Perc", 1)]
    );
    assert_eq!(
        after[1].rows[0].hits.len(),
        2,
        "the kick's notes went along"
    );
}

#[test]
fn play_quantize_grooves_and_nudges_round_trip() {
    let (s, seq, drums, ..) = session();
    let choices = drum_quantize_choices().unwrap();
    assert_eq!(choices.len(), 15);
    let swing = choices
        .iter()
        .find(|c| c.title == "1/8 Swing 66%")
        .unwrap()
        .quantize
        .clone();
    assert_eq!(
        swing.as_ref().map(|q| q.groove.clone()),
        Some(vec![0, 2_457])
    );
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetPlayQuantize {
                seq,
                lane: drums,
                quantize: swing.clone(),
            },
        },
        None,
    )
    .unwrap();
    s.edit(
        EditCommand::Lane {
            edit: LaneEdit::SetPlayShift {
                seq,
                lane: drums,
                shift: drum_nudged(0, -3).unwrap(),
            },
        },
        None,
    )
    .unwrap();
    let lane = &s.drum_lanes(seq).unwrap()[0];
    assert_eq!(lane.play_quantize, swing);
    assert_eq!(lane.play_shift, -360);
    assert_eq!(
        drum_quantize_title(lane.play_quantize.clone()).unwrap(),
        "1/8 Swing 66%"
    );
    assert_eq!(drum_quantize_title(None).unwrap(), "Off");
    let bad = QuantizeRow {
        grid: Q / 2,
        strength: 100,
        groove: vec![Q],
    };
    assert_eq!(drum_quantize_title(Some(bad.clone())).unwrap(), "Custom");
    assert!(matches!(
        s.edit(
            EditCommand::Lane {
                edit: LaneEdit::SetPlayQuantize {
                    seq,
                    lane: drums,
                    quantize: Some(bad)
                },
            },
            None
        ),
        Err(VizError::Model { .. })
    ));
    assert_eq!(
        drum_next_row(vec![36, 38])
            .unwrap()
            .map(|r| (r.note, r.name)),
        Some((42, "Closed Hat".to_string()))
    );
}

fn arb_layout() -> impl Strategy<Value = DrumGridLayoutRow> {
    (
        prop::collection::vec(
            (
                0u32..4,
                0u32..4,
                any::<u8>(),
                any::<f64>(),
                any::<f64>(),
                any::<f64>(),
            ),
            0..8,
        ),
        any::<f64>(),
    )
        .prop_map(|(bands, height)| DrumGridLayoutRow {
            bands: bands
                .into_iter()
                .map(|(lane, row, k, top, h, c)| DrumBandRow {
                    kind: match k % 3 {
                        0 => DrumBandKindRow::Header { lane },
                        1 => DrumBandKindRow::Row { lane, row },
                        _ => DrumBandKindRow::NewLane,
                    },
                    top,
                    height: h,
                    cells: c,
                })
                .collect(),
            height,
        })
}

fn arb_settings() -> impl Strategy<Value = DrumGridSettingsRow> {
    (
        any::<bool>(),
        any::<i64>(),
        any::<i64>(),
        any::<i64>(),
        any::<u8>(),
        any::<f64>(),
        any::<f64>(),
    )
        .prop_map(
            |(t, step, spacing, duration, velocity, row_height, velocity_height)| {
                DrumGridSettingsRow {
                    view: if t {
                        DrumViewRow::Timeline
                    } else {
                        DrumViewRow::Matrix
                    },
                    step,
                    spacing,
                    duration,
                    velocity,
                    row_height,
                    velocity_height,
                }
            },
        )
}

fn arb_hits() -> impl Strategy<Value = Vec<DrumHitRow>> {
    prop::collection::vec(
        (
            any::<u64>(),
            any::<i64>(),
            any::<i64>(),
            any::<u16>(),
            any::<bool>(),
        )
            .prop_map(|(id, tick, duration, velocity, mute)| DrumHitRow {
                id,
                tick,
                duration,
                velocity,
                mute,
            }),
        0..8,
    )
}

fn arb_edit() -> impl Strategy<Value = DrumEdit> {
    let id = || prop_oneof![8 => 0u64..3, 1 => 3u64..12, 1 => any::<u64>()];
    prop_oneof![
        (id(), id(), any::<bool>()).prop_map(|(seq, lane, drum)| DrumEdit::MarkLane {
            seq,
            lane,
            drum
        }),
        (id(), id(), any::<u8>(), prop::option::of(id())).prop_map(|(seq, lane, note, dest)| {
            DrumEdit::AddRow {
                seq,
                lane,
                name: "x".into(),
                note,
                dest,
            }
        }),
        (id(), id(), id(), any::<u32>()).prop_map(|(seq, lane, row, to)| DrumEdit::ReorderRow {
            seq,
            lane,
            row,
            to
        }),
        (id(), id(), id(), id()).prop_map(|(seq, lane, row, to_lane)| DrumEdit::MoveRowToLane {
            seq,
            lane,
            row,
            to_lane
        }),
        (id(), id(), id()).prop_map(|(seq, lane, row)| DrumEdit::SplitRow { seq, lane, row }),
        (id(), id(), id()).prop_map(|(seq, lane, row)| DrumEdit::RemoveRow { seq, lane, row }),
        (
            id(),
            id(),
            id(),
            prop::collection::vec((any::<i64>(), any::<u16>()), 0..6),
            any::<i64>(),
            any::<i64>()
        )
            .prop_map(|(seq, lane, row, hits, window, duration)| DrumEdit::Paint {
                seq,
                lane,
                row,
                hits: hits
                    .into_iter()
                    .map(|(tick, velocity)| DrumPaintHitRow { tick, velocity })
                    .collect(),
                window,
                duration,
            }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Hostile layouts, settings, hits and strokes: every grid function
    /// answers (no `Internal`, which a caught panic would be).
    #[test]
    fn grid_functions_answer_any_input(
        layout in arb_layout(),
        y in any::<f64>(),
        hits in arb_hits(),
        at in any::<i64>(),
        to in any::<i64>(),
        grab in any::<i64>(),
        v in prop::option::of(any::<u8>()),
        erase in any::<bool>(),
        s in arb_settings(),
        shapes in prop::collection::vec(prop::collection::vec(any::<bool>(), 0..4), 0..4),
    ) {
        prop_assert!(drum_grid_spot(layout.clone(), y).is_ok());
        prop_assert!(drum_row_velocity(layout.clone(), 1, 0, y).is_ok());
        prop_assert!(drum_grid_layout(shapes, s).is_ok());
        prop_assert!(drum_grid_cells(hits.clone(), at, to, grab).is_ok());
        prop_assert!(drum_grid_hits_at(hits.clone(), at, grab).is_ok());
        let st = drum_stroke_begin(0, 0, hits.clone(), at, to, grab, v, erase, s).unwrap();
        let st: DrumStrokeRow = drum_stroke_extend(st, hits.clone(), to, grab, v, s).unwrap();
        prop_assert!(drum_stroke_command(st, 1, 2, 3, s).is_ok());
        prop_assert!(drum_row_drop(layout, vec![1, 2], 1, 0, 3, 0, y).is_ok());
        prop_assert!(drum_nudged(at, to).is_ok());
    }

    /// Hostile drum edits on the fixture: applied (and undone to the start),
    /// or refused with a model error that leaves the document as it was;
    /// never a panic.
    #[test]
    fn hostile_drum_edits_apply_or_change_nothing(edit in arb_edit()) {
        let (s, seq, ..) = session();
        let before = (s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap());
        match s.edit(drum(edit), None) {
            Ok(_) => {
                // Accepted: undoing it restores the starting document (a
                // no-op edit, e.g. reordering to where the row is, pushes
                // no undo step and changed nothing).
                match s.undo() {
                    Ok(_) | Err(VizError::Model { .. }) => {}
                    Err(e) => prop_assert!(false, "unexpected {e:?}"),
                }
                prop_assert_eq!((s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap()), before);
            }
            Err(VizError::Model { .. }) => {
                prop_assert_eq!((s.drum_lanes(seq).unwrap(), s.lanes(seq).unwrap()), before);
            }
            Err(e) => prop_assert!(false, "unexpected {e:?}"),
        }
    }
}
