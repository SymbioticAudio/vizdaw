//! The drum grid's logic (Plan 4e Task 3).

use core_editor::drum::{
    Band, BandKind, Drop, HEADER_HEIGHT, Hit, LaneShape, Layout, MAX_CELLS, MAX_NUDGE,
    NEW_LANE_HEIGHT, Settings, Spot, Tool, View, begin, cell_start, cells, drop_row, extend,
    hits_at, layout, next_row, nudged, paint_window, quantize_choices, quantize_title, row_name,
    row_velocity, spot, velocity_at, velocity16,
};
use core_model::command::drum::MAX_PAINT_HITS;
use core_model::lane::QuantizeSpec;
use core_model::time::{PPQ, Tick};
use proptest::prelude::*;

const S16: i64 = PPQ / 4;

fn matrix() -> Settings {
    Settings::default()
}

fn timeline() -> Settings {
    Settings {
        view: View::Timeline,
        ..Settings::default()
    }
}

/// Two lanes: lane 0 with rows 0 and 1 (row 1 shows velocity), lane 1
/// with one row.
fn two_lanes() -> Layout {
    layout(
        &[
            LaneShape {
                rows: vec![false, true],
            },
            LaneShape { rows: vec![false] },
        ],
        &matrix(),
    )
}

fn hit(id: u64, tick: i64) -> Hit {
    Hit {
        id,
        tick,
        duration: S16 / 2,
        velocity: 100 << 9,
    }
}

#[test]
fn lanes_stack_headers_rows_and_the_new_lane_band() {
    let l = two_lanes();
    let kinds: Vec<(BandKind, f64, f64, f64)> = l
        .bands
        .iter()
        .map(|b| (b.kind, b.top, b.height, b.cells))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (BandKind::Header { lane: 0 }, 0.0, 22.0, 22.0),
            (BandKind::Row { lane: 0, row: 0 }, 22.0, 20.0, 20.0),
            (BandKind::Row { lane: 0, row: 1 }, 42.0, 48.0, 20.0),
            (BandKind::Header { lane: 1 }, 90.0, 22.0, 22.0),
            (BandKind::Row { lane: 1, row: 0 }, 112.0, 20.0, 20.0),
            (BandKind::NewLane, 132.0, NEW_LANE_HEIGHT, NEW_LANE_HEIGHT),
        ]
    );
    assert_eq!(l.height, 160.0);
    assert_eq!(HEADER_HEIGHT, 22.0);
}

#[test]
fn a_height_names_its_band_and_a_shown_velocity_lane_its_velocity() {
    let l = two_lanes();
    assert_eq!(spot(&l, 5.0), Spot::Header { lane: 0 });
    assert_eq!(
        spot(&l, 30.0),
        Spot::Row {
            lane: 0,
            row: 0,
            velocity: None
        }
    );
    // Row 1 spans 42–90: its top is 127, its bottom 1.
    assert_eq!(
        spot(&l, 42.0),
        Spot::Row {
            lane: 0,
            row: 1,
            velocity: Some(127)
        }
    );
    assert_eq!(
        spot(&l, 89.99),
        Spot::Row {
            lane: 0,
            row: 1,
            velocity: Some(1)
        }
    );
    assert_eq!(
        spot(&l, 66.0),
        Spot::Row {
            lane: 0,
            row: 1,
            velocity: Some(64)
        }
    );
    // A stroke on row 1 dragged above or below it is clamped to the row;
    // row 0 shows no velocity lane.
    assert_eq!(row_velocity(&l, 0, 1, 0.0), Some(127));
    assert_eq!(row_velocity(&l, 0, 1, 500.0), Some(1));
    assert_eq!(row_velocity(&l, 0, 0, 30.0), None);
    assert_eq!(row_velocity(&l, 3, 0, 30.0), None);
    assert_eq!(spot(&l, 140.0), Spot::NewLane);
    assert_eq!(
        spot(&l, 10_000.0),
        Spot::NewLane,
        "anywhere below the last lane"
    );
    assert_eq!(spot(&l, -1.0), Spot::Nothing);
    assert_eq!(spot(&l, f64::NAN), Spot::Nothing);
    assert_eq!(spot(&Layout::default(), 3.0), Spot::Nothing);
    let b = Band {
        kind: BandKind::Row { lane: 0, row: 0 },
        top: 0.0,
        height: 0.0,
        cells: 0.0,
    };
    assert_eq!(
        velocity_at(&b, 0.0),
        64,
        "a zero-height band has a middle velocity"
    );
}

#[test]
fn settings_are_clamped_and_matrix_strides_are_whole_cells() {
    let wild = Settings {
        view: View::Matrix,
        step: 0,
        spacing: -4,
        duration: i64::MIN,
        velocity: 0,
        row_height: f64::NAN,
        velocity_height: 1e9,
    };
    let s = wild.sane();
    assert_eq!(
        (s.step, s.spacing, s.duration, s.velocity),
        (S16, S16, PPQ / 8, 1)
    );
    assert_eq!((s.row_height, s.velocity_height), (20.0, 96.0));
    assert_eq!(
        Settings {
            step: i64::MAX,
            ..matrix()
        }
        .sane()
        .step,
        4 * PPQ
    );
    // An eighth's spacing over sixteenth cells is two cells; a dotted
    // sixteenth rounds to two; a 32nd rounds up to one.
    for (spacing, stride) in [(PPQ / 2, PPQ / 2), (S16 * 3 / 2, PPQ / 2), (PPQ / 8, S16)] {
        assert_eq!(
            Settings {
                spacing,
                ..matrix()
            }
            .stride(),
            stride,
            "{spacing}"
        );
    }
    assert_eq!(
        Settings {
            spacing: PPQ / 8,
            ..timeline()
        }
        .stride(),
        PPQ / 8
    );
    assert_eq!(paint_window(&matrix()), S16);
    assert_eq!(paint_window(&timeline()), 1);
}

#[test]
fn cells_hold_the_hits_that_start_in_them() {
    let hits = [
        hit(7, S16 + 10),
        hit(3, 0),
        hit(9, S16 + 20),
        hit(4, 4 * S16),
    ];
    let c = cells(&hits, -10, 3 * S16, S16);
    let summary: Vec<(i64, Vec<u64>)> = c.iter().map(|c| (c.tick, c.hits.clone())).collect();
    assert_eq!(
        summary,
        vec![
            (-S16, vec![]),
            (0, vec![3]),
            (S16, vec![7, 9]),
            (2 * S16, vec![]),
        ]
    );
    assert_eq!(c[1].velocity, 100 << 9);
    assert_eq!(c[0].velocity, 0);
    assert_eq!(cells(&hits, 0, i64::MAX, 1).len(), MAX_CELLS);
    assert_eq!(cell_start(-1, S16), -S16);
    assert_eq!(cell_start(5, 0), 5, "a step below 1 counts as 1");
    assert_eq!(
        cell_start(-1, i64::MAX),
        -(1 << 40),
        "a step above 2^40 counts as 2^40"
    );
    assert_eq!(hits_at(&hits, S16 + 15, 1), vec![7]);
    assert_eq!(hits_at(&hits, S16 + 25, 1), vec![7, 9]);
    assert_eq!(hits_at(&hits, 3 * S16, 1), Vec::<u64>::new());
}

#[test]
fn a_press_on_an_empty_cell_paints_and_a_drag_paints_a_run() {
    let hits = [hit(1, 2 * S16)];
    let s = Settings {
        spacing: PPQ / 2,
        ..matrix()
    };
    let st = begin(&hits, 10, 0, 1, None, Tool::Paint, &s);
    assert!(!st.erase);
    assert_eq!(st.start, 0);
    assert_eq!(st.hits, vec![(0, 100 << 9)]);
    // Dragged to the fifth cell with the velocity lane at 20: every
    // second cell, the first keeping its velocity.
    let st = extend(&st, &hits, 4 * S16 + 5, 1, Some(20), &s);
    assert_eq!(
        st.hits,
        vec![(0, 100 << 9), (2 * S16, 20 << 9), (4 * S16, 20 << 9)]
    );
    // Back left of the start: the run turns around and shrinks.
    let st = extend(&st, &hits, -S16 * 3, 1, Some(90), &s);
    assert_eq!(st.hits, vec![(0, 100 << 9), (-2 * S16, 90 << 9)]);
    assert!(st.erased.is_empty());
    assert_eq!(st.to, -S16 * 3);
}

#[test]
fn a_press_on_a_hit_erases_and_the_erase_tool_always_does() {
    let hits = [hit(1, S16 + 3), hit(2, 2 * S16), hit(3, 3 * S16)];
    let st = begin(&hits, S16 + 100, 0, 1, None, Tool::Paint, &matrix());
    assert!(st.erase);
    assert_eq!(st.erased, vec![1]);
    let st = extend(&st, &hits, 3 * S16 + 1, 1, None, &matrix());
    assert_eq!(st.erased, vec![1, 2, 3]);
    assert!(st.hits.is_empty());
    let st = begin(&hits, 0, 0, 1, None, Tool::Erase, &matrix());
    assert!(st.erase && st.erased.is_empty());
    // The timeline erases what the swept range touches; a press inside a
    // long hit counts.
    let long = [Hit {
        id: 5,
        tick: 0,
        duration: PPQ,
        velocity: 1,
    }];
    let st = begin(&long, PPQ / 2, PPQ / 2, 1, None, Tool::Paint, &timeline());
    assert!(st.erase);
    assert_eq!(st.erased, vec![5]);
    // The timeline paints from the snapped press every spacing.
    let st = begin(&[], 1_000, 0, 1, Some(127), Tool::Paint, &timeline());
    let st = extend(&st, &[], 2 * S16 + 7, 1, Some(1), &timeline());
    assert_eq!(
        st.hits,
        vec![(0, 127 << 9), (S16, 1 << 9), (2 * S16, 1 << 9)]
    );
}

#[test]
fn strokes_stop_at_the_paint_limit_and_the_tick_limit() {
    let s = Settings {
        step: PPQ / 32,
        spacing: 1,
        view: View::Timeline,
        ..matrix()
    };
    let st = begin(&[], 0, 0, 1, None, Tool::Paint, &s);
    let st = extend(&st, &[], i64::MAX, 1, None, &s);
    assert_eq!(st.hits.len(), MAX_PAINT_HITS);
    let st = begin(&[], i64::MIN, i64::MIN, 1, None, Tool::Paint, &s);
    assert_eq!(st.start, -(1 << 40));
    assert_eq!(velocity16(0), 1 << 9);
    assert_eq!(velocity16(200), 127 << 9);
}

#[test]
fn a_dragged_row_lands_by_height() {
    let l = two_lanes();
    assert_eq!(drop_row(&l, 0, 0, 60.0), Drop::Reorder { to: 1 });
    assert_eq!(drop_row(&l, 0, 1, 5.0), Drop::Reorder { to: 0 });
    assert_eq!(drop_row(&l, 0, 0, 5.0), Drop::Nothing);
    assert_eq!(drop_row(&l, 0, 0, 30.0), Drop::Nothing);
    assert_eq!(drop_row(&l, 0, 1, 115.0), Drop::ToLane { lane: 1 });
    assert_eq!(drop_row(&l, 1, 0, 95.0), Drop::Nothing);
    assert_eq!(drop_row(&l, 1, 0, 30.0), Drop::ToLane { lane: 0 });
    assert_eq!(drop_row(&l, 1, 0, 500.0), Drop::NewLane);
    assert_eq!(drop_row(&l, 1, 0, f64::INFINITY), Drop::Nothing);
}

#[test]
fn nudge_quantize_menu_and_new_rows() {
    assert_eq!(nudged(0, 2), 240);
    assert_eq!(nudged(0, i64::MAX), MAX_NUDGE);
    assert_eq!(nudged(i64::MIN, -1), -MAX_NUDGE);
    let choices = quantize_choices();
    assert_eq!(choices.len(), 15);
    assert_eq!(choices[0], ("Off".to_string(), None));
    assert_eq!(quantize_title(None), "Off");
    let swing = QuantizeSpec::swing(Tick(PPQ / 4), 58);
    assert_eq!(quantize_title(Some(&swing)), "1/16 Swing 58%");
    assert_eq!(
        quantize_title(Some(&QuantizeSpec::grid(Tick(PPQ / 4), 50))),
        "Custom"
    );
    assert!(
        choices
            .iter()
            .all(|(_, q)| q.as_ref().is_none_or(|q| q.check().is_ok()))
    );
    assert_eq!(next_row(&[]), Some((36, "Kick".to_string())));
    assert_eq!(next_row(&[36, 38]), Some((42, "Closed Hat".to_string())));
    let all: Vec<u8> = (0..=127).collect();
    assert_eq!(next_row(&all), None);
    let gm: Vec<u8> = (35..=81).collect();
    assert_eq!(next_row(&gm), Some((0, "C-1".to_string())));
    assert_eq!(row_name(81), "Open Triangle");
    assert_eq!(row_name(60), "Hi Bongo");
    assert_eq!(row_name(84), "C6");
}

fn arb_settings() -> impl Strategy<Value = Settings> {
    (
        any::<bool>(),
        prop_oneof![any::<i64>(), 1i64..PPQ],
        prop_oneof![any::<i64>(), 1i64..PPQ],
        any::<i64>(),
        any::<u8>(),
        prop_oneof![any::<f64>(), 0.0f64..100.0],
        prop_oneof![any::<f64>(), 0.0f64..100.0],
    )
        .prop_map(|(t, step, spacing, duration, velocity, rh, vh)| Settings {
            view: if t { View::Timeline } else { View::Matrix },
            step,
            spacing,
            duration,
            velocity,
            row_height: rh,
            velocity_height: vh,
        })
}

fn arb_hits() -> impl Strategy<Value = Vec<Hit>> {
    prop::collection::vec(
        (
            0u64..20,
            prop_oneof![any::<i64>(), -PPQ..(8 * PPQ)],
            prop_oneof![any::<i64>(), 0i64..PPQ],
            any::<u16>(),
        )
            .prop_map(|(id, tick, duration, velocity)| Hit {
                id,
                tick,
                duration,
                velocity,
            }),
        0..12,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any shapes and settings: bands tile the height without gaps, and
    /// every band's middle is that band's spot.
    #[test]
    fn layouts_tile_and_spots_agree(
        rows in prop::collection::vec(prop::collection::vec(any::<bool>(), 0..5), 0..4),
        s in arb_settings(),
    ) {
        let lanes: Vec<LaneShape> = rows.into_iter().map(|rows| LaneShape { rows }).collect();
        let l = layout(&lanes, &s);
        let mut y = 0.0;
        for b in &l.bands {
            prop_assert_eq!(b.top, y);
            prop_assert!(b.height > 0.0 && b.cells > 0.0 && b.cells <= b.height);
            y += b.height;
            let mid = spot(&l, b.top + b.height / 2.0);
            let ok = match (b.kind, mid) {
                (BandKind::Header { lane }, Spot::Header { lane: l2 }) => lane == l2,
                (BandKind::Row { lane, row }, Spot::Row { lane: l2, row: r2, velocity }) => {
                    lane == l2 && row == r2 && velocity.is_some() == b.shows_velocity()
                }
                (BandKind::NewLane, Spot::NewLane) => true,
                _ => false,
            };
            prop_assert!(ok, "{:?} vs {:?}", b, mid);
        }
        prop_assert_eq!(l.height, y);
    }

    /// Any hits, window and step: cells are consecutive and at most
    /// MAX_CELLS; within ±2^40 ticks every hit starting inside the cells
    /// shows in exactly one.
    #[test]
    fn cells_partition_the_hits(
        hits in arb_hits(),
        from in prop_oneof![any::<i64>(), -(1i64 << 40)..(1i64 << 40)],
        len in 0i64..(64 * PPQ),
        step in prop_oneof![any::<i64>(), 1i64..PPQ],
    ) {
        let to = from.saturating_add(len);
        let c = cells(&hits, from, to, step);
        prop_assert!(c.len() <= MAX_CELLS);
        for w in c.windows(2) {
            prop_assert!(w[1].tick > w[0].tick);
        }
        let sane = |t: i64| t.unsigned_abs() <= 1 << 40;
        if let (Some(first), Some(last)) = (c.first(), c.last()) {
            let end = last.tick.saturating_add(step.clamp(1, 1 << 40));
            for h in hits.iter().filter(|h| sane(h.tick) && sane(from)) {
                let n = c.iter().filter(|x| x.hits.contains(&h.id) && cell_start(h.tick, step) == x.tick).count();
                if h.tick >= first.tick && h.tick < end {
                    prop_assert_eq!(n, 1, "{:?}", h);
                }
            }
        }
    }

    /// Any press and drags: a stroke holds at most MAX_PAINT_HITS distinct
    /// hits within ±2^40 ticks, on its stride from its start; it erases
    /// only hits of the row, and paints nothing while erasing.
    #[test]
    fn strokes_stay_bounded(
        hits in arb_hits(),
        at in any::<i64>(),
        snapped in any::<i64>(),
        moves in prop::collection::vec((any::<i64>(), prop::option::of(any::<u8>())), 0..4),
        erase in any::<bool>(),
        grab in any::<i64>(),
        s in arb_settings(),
    ) {
        let tool = if erase { Tool::Erase } else { Tool::Paint };
        let mut st = begin(&hits, at, snapped, grab, None, tool, &s);
        for (to, v) in moves {
            st = extend(&st, &hits, to, grab, v, &s);
        }
        prop_assert!(st.hits.len() <= MAX_PAINT_HITS);
        let stride = s.stride();
        let mut seen = std::collections::BTreeSet::new();
        for (t, v) in &st.hits {
            prop_assert!(t.unsigned_abs() <= 1 << 40);
            prop_assert!(seen.insert(*t), "distinct");
            prop_assert!((i128::from(*t) - i128::from(st.start)) % i128::from(stride) == 0);
            prop_assert!(*v >= 1 << 9);
        }
        if st.erase {
            prop_assert!(st.hits.is_empty());
        }
        prop_assert!(st.erased.iter().all(|id| hits.iter().any(|h| h.id == *id)));
    }

    /// Dropping a row anywhere never panics, and a drop into its own lane
    /// names one of that lane's indexes.
    #[test]
    fn drops_never_panic(rows in prop::collection::vec(1usize..5, 1..4), lane in 0usize..4, row in 0usize..5, y in any::<f64>()) {
        let lanes: Vec<LaneShape> = rows.iter().map(|n| LaneShape { rows: vec![false; *n] }).collect();
        let l = layout(&lanes, &Settings::default());
        if let Drop::Reorder { to } = drop_row(&l, lane, row, y) {
            prop_assert!(to < rows.get(lane).copied().unwrap_or(0));
        }
    }
}
