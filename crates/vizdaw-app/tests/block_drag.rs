//! Swift `ArrangementLayoutTests` (hit testing and block drags) and the
//! drag cases of `ArrangementViewTests`, played against the model: the
//! tool machine, the hit test and the block drag together.

mod support;

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use support::{FakeSession, block_row, editor_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{
    ArrangementFrame, ArrangementHit, BlockDrag, BlockDragState, Geometry, MachineEvent, Modifiers,
    Point, TimeAxis, ToolMachine, hit_test,
};
use vizdaw_app::editor::Snap;
use vizdaw_ffi::VizError;
use vizdaw_ffi::blocks::{BlockKind, BlockRow};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::{EventKindRow, EventRow, MeterEntryRow};

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;
const FOUR_FOUR: [MeterEntryRow; 1] = [MeterEntryRow {
    tick: 0,
    num: 4,
    den: 4,
    sharps: 0,
    minor: false,
}];

const A: LaneRef = LaneRef { seq: 3, lane: 30 };
const B: LaneRef = LaneRef { seq: 3, lane: 31 };

fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

fn reference(start: i64, end: i64, ids: &[u64]) -> BlockRow {
    BlockRow {
        kind: BlockKind::Reference,
        label: Some("Groove".into()),
        ref_seq: Some(2),
        ..block_row(start, end, ids)
    }
}

fn row(id: u64, tick: i64) -> EventRow {
    EventRow {
        pitch: 60,
        velocity: 0x8000,
        duration: Q,
        ..EventRow::blank(id, tick, EventKindRow::Note)
    }
}

fn mv(events: &[u64], delta: i64, to_lane: Option<LaneRef>) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Move {
            events: events.to_vec(),
            delta,
            to_lane,
        },
    }
}

fn axis() -> TimeAxis {
    TimeAxis::new(384., 0)
}

// MARK: ArrangementLayoutTests

#[test]
fn hitting_prefers_the_topmost_block() {
    let rows = vec![
        (
            30,
            vec![
                block_row(0, 2 * BAR, &[1, 2]),
                reference(BAR, 2 * BAR, &[9]),
            ],
        ),
        (31, vec![]),
    ];
    let g = Geometry::default();
    assert_eq!(
        hit_test(&g, &axis(), &rows, p(200., 10.)),
        ArrangementHit::Block { lane: 30, index: 1 }
    );
    assert_eq!(
        hit_test(&g, &axis(), &rows, p(10., 60.)),
        ArrangementHit::Empty {
            lane: Some(31),
            tick: 3_840
        }
    );
    assert_eq!(
        hit_test(&g, &axis(), &rows, p(10., 500.)),
        ArrangementHit::Empty {
            lane: None,
            tick: 3_840
        }
    );
}

#[test]
fn drag_delta_snaps_the_block_start() {
    // 1000 ticks per point: 15 points is 15 000 ticks.
    let axis = TimeAxis::new(1_000., 0);
    assert_eq!(
        BlockDrag::delta(&axis, 0, 15., 1_000, &Snap::default(), &FOUR_FOUR),
        14_360
    );
    let off = Snap {
        enabled: false,
        ..Snap::default()
    };
    assert_eq!(
        BlockDrag::delta(&axis, 0, 15., 1_000, &off, &FOUR_FOUR),
        15_000
    );
}

#[test]
fn moves_stay_in_their_lane_or_merge_into_another() {
    assert_eq!(BlockDrag::command(&[1], &[], A, A, 0, false), None);
    assert_eq!(
        BlockDrag::command(&[1], &[], A, A, Q, false),
        Some(mv(&[1], Q, None))
    );
    assert_eq!(
        BlockDrag::command(&[1], &[], A, B, 0, false),
        Some(mv(&[1], 0, Some(B)))
    );
}

#[test]
fn option_copies_shifted_rows_into_the_target_lane() {
    let rows = [row(1, 0), row(2, Q), row(3, 5 * BAR)];
    let cmd = BlockDrag::command(&[1, 2], &rows, A, B, BAR, true);
    let Some(EditCommand::Event {
        edit: EventEdit::Insert { seq, lane, events },
    }) = cmd
    else {
        panic!("expected an insert, got {cmd:?}");
    };
    assert_eq!((seq, lane), (3, 31));
    let ticks: Vec<_> = events.iter().map(|e| e.tick).collect();
    assert_eq!(ticks, [BAR, BAR + Q]);
    assert_eq!(BlockDrag::label(true), "Copy Block");
    assert_eq!(BlockDrag::label(false), "Move Block");
}

#[test]
fn option_copies_in_the_same_lane_duplicate_the_shifted_events() {
    let rows = [row(1, 0), row(2, Q), row(3, 5 * BAR)];
    assert_eq!(
        BlockDrag::command(&[1, 2], &rows, A, A, BAR, true),
        Some(EditCommand::Event {
            edit: EventEdit::Duplicate {
                events: vec![1, 2],
                ticks: BAR,
                semitones: 0
            }
        })
    );
    assert_eq!(BlockDrag::command(&[1, 2], &rows, A, A, 0, true), None);
    assert_eq!(BlockDrag::command(&[9], &rows, A, A, BAR, true), None);
}

#[test]
fn dragging_a_selected_block_carries_the_other_selected_blocks_of_its_lane() {
    let lane = [
        block_row(0, BAR, &[1, 2]),
        block_row(2 * BAR, 3 * BAR, &[3]),
        block_row(4 * BAR, 5 * BAR, &[4]),
    ];
    let selected: HashSet<u64> = [1, 2, 4].into();
    assert_eq!(BlockDrag::events(&lane[0], &lane, &selected), [1, 2, 4]);
    assert_eq!(BlockDrag::events(&lane[1], &lane, &selected), [3]);
}

// MARK: ArrangementViewTests, as model tests

/// Song's rows: Groove (30) is row 0, Chords (31) row 1; the Chords block
/// spans x 0–160 at the default zoom.
const ON_CHORDS: Point = Point { x: 50., y: 70. };

/// What the arrangement canvas does with the pointer: the machine's
/// events, hit tested, become a block drag (clicks are the canvas's own).
struct Canvas {
    machine: ToolMachine<ArrangementHit>,
    drag: Option<BlockDragState>,
    frame: ArrangementFrame,
}

impl Canvas {
    fn new() -> Self {
        Canvas {
            machine: ToolMachine::new(),
            drag: None,
            frame: ArrangementFrame {
                geometry: Geometry::default(),
                axis: axis(),
            },
        }
    }

    fn handle(&mut self, m: &mut DocumentModel, events: Vec<MachineEvent<ArrangementHit>>) {
        m.block_drag_events(&mut self.drag, &events, &self.frame);
    }

    fn down(&mut self, m: &mut DocumentModel, at: Point, modifiers: Modifiers) {
        let rows: Vec<(u64, Arc<[BlockRow]>)> =
            m.lanes().iter().map(|l| (l.id, m.blocks(l.id))).collect();
        let hit = hit_test(&self.frame.geometry, &self.frame.axis, &rows, at);
        let fx = self.machine.down(hit, at, 1, modifiers);
        self.handle(m, fx);
    }

    fn drag(&mut self, m: &mut DocumentModel, to: Point, modifiers: Modifiers) {
        let fx = self.machine.drag(to, modifiers);
        self.handle(m, fx);
    }

    fn up(&mut self, m: &mut DocumentModel, at: Point, modifiers: Modifiers) {
        let fx = self.machine.up(at, modifiers);
        self.handle(m, fx);
    }

    /// Escape: also aborts a drag whose final edit waits on Busy.
    fn cancel(&mut self, m: &mut DocumentModel) {
        let fx = self.machine.cancel();
        self.handle(m, fx);
        self.drag = None;
        if m.gesture_open() {
            m.cancel_gesture();
        }
    }
}

fn arrangement() -> (Arc<FakeSession>, DocumentModel, Canvas) {
    let f = Arc::new(editor_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    f.clear_calls();
    (f, m, Canvas::new())
}

const NONE: Modifiers = Modifiers::NONE;
const OPTION: Modifiers = Modifiers::OPTION;

#[test]
fn a_plain_move_is_one_begin_end_pair() {
    let (f, mut m, mut v) = arrangement();
    v.down(&mut m, ON_CHORDS, NONE);
    v.drag(&mut m, p(300., 70.), NONE);
    v.up(&mut m, p(300., 70.), NONE);
    assert_eq!(
        f.calls(),
        ["begin_edit(Move Block)", "edit_gesture", "end_edit"]
    );
    let last = f.gesture_edits().last().cloned();
    let Some(EditCommand::Event {
        edit:
            EventEdit::Move {
                events,
                delta,
                to_lane,
            },
    }) = last
    else {
        panic!("expected a move, got {:?}", f.gesture_edits());
    };
    assert!(events == [1] && delta > 0 && to_lane.is_none());
    assert!(!m.gesture_open());
    assert_eq!(m.snapshot().undo_label.as_deref(), Some("Move Block"));
}

#[test]
fn an_option_drag_copies_into_the_target_lane() {
    let (f, mut m, mut v) = arrangement();
    v.down(&mut m, ON_CHORDS, OPTION);
    v.drag(&mut m, p(300., 20.), OPTION);
    v.up(&mut m, p(300., 20.), OPTION);
    assert_eq!(
        f.calls(),
        ["begin_edit(Copy Block)", "edit_gesture", "end_edit"]
    );
    let last = f.gesture_edits().last().cloned();
    let Some(EditCommand::Event {
        edit: EventEdit::Insert { seq, lane, events },
    }) = last
    else {
        panic!("expected an insert, got {:?}", f.gesture_edits());
    };
    assert_eq!((seq, lane), (3, 30));
    assert!(events.len() == 1 && events[0].tick > 0);
}

#[test]
fn a_drag_onto_another_row_merges_into_its_lane() {
    let (f, mut m, mut v) = arrangement();
    v.down(&mut m, ON_CHORDS, NONE);
    v.drag(&mut m, p(50., 20.), NONE);
    v.up(&mut m, p(50., 20.), NONE);
    assert_eq!(
        f.calls(),
        ["begin_edit(Move Block)", "edit_gesture", "end_edit"]
    );
    assert_eq!(f.gesture_edits(), [mv(&[1], 0, Some(A))]);
}

#[test]
fn escape_mid_drag_leaves_no_change() {
    let (f, mut m, mut v) = arrangement();
    v.down(&mut m, ON_CHORDS, NONE);
    v.drag(&mut m, p(300., 70.), NONE);
    v.cancel(&mut m);
    v.up(&mut m, p(300., 70.), NONE);
    assert_eq!(
        f.calls(),
        ["begin_edit(Move Block)", "edit_gesture", "abort_edit"]
    );
    assert!(!m.gesture_open());
    assert_eq!(m.snapshot().undo_label, None);
}

#[test]
fn option_pressed_mid_drag_does_not_turn_a_move_into_a_copy() {
    let (f, mut m, mut v) = arrangement();
    v.down(&mut m, ON_CHORDS, NONE);
    v.drag(&mut m, p(300., 70.), NONE);
    v.drag(&mut m, p(300., 20.), OPTION);
    v.up(&mut m, p(300., 20.), OPTION);
    let calls = f.calls();
    assert_eq!(calls.first().unwrap(), "begin_edit(Move Block)");
    assert_eq!(calls.last().unwrap(), "end_edit");
    for e in f.gesture_edits() {
        assert!(
            matches!(
                e,
                EditCommand::Event {
                    edit: EventEdit::Move { .. }
                }
            ),
            "expected only moves, got {e:?}"
        );
    }
    let Some(EditCommand::Event {
        edit: EventEdit::Move { to_lane, .. },
    }) = f.gesture_edits().last().cloned()
    else {
        panic!("no move");
    };
    assert_eq!(to_lane, Some(A));
}

#[test]
fn escape_aborts_a_final_edit_still_pending_on_busy() {
    let (f, mut m, mut v) = arrangement();
    v.down(&mut m, ON_CHORDS, NONE);
    v.drag(&mut m, p(300., 70.), NONE);
    assert!(m.gesture_open());
    f.fail_next(
        "edit_gesture",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    v.up(&mut m, p(600., 70.), NONE);
    assert!(
        m.gesture().unwrap().finishing,
        "the final edit waits for the plugin timer"
    );
    v.cancel(&mut m);
    assert!(!m.gesture_open());
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
    assert!(!f.calls().iter().any(|c| c == "end_edit"));
}

#[test]
fn a_drag_watches_its_source_and_target_lanes() {
    let (f, mut m, mut v) = arrangement();
    let lanes = |m: &DocumentModel| m.gesture().unwrap().lanes.clone();
    let both: BTreeSet<_> = [(3, 31), (3, 30)].into();
    let chords: BTreeSet<_> = [(3, 31)].into();
    v.down(&mut m, ON_CHORDS, NONE);
    v.drag(&mut m, p(300., 20.), NONE);
    assert_eq!(lanes(&m), both, "begun straight onto the Groove row");
    v.drag(&mut m, p(300., 70.), NONE);
    assert_eq!(lanes(&m), chords);
    v.drag(&mut m, p(300., 20.), NONE);
    assert_eq!(lanes(&m), both);
    let mut s = f.snap();
    s.version += 1;
    s.lane_revs.retain(|r| r.lane != 30);
    f.set_idle(Some(s));
    m.idle();
    assert!(
        !m.gesture_open(),
        "the target lane vanished: the drag is aborted"
    );
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
}

// Beyond the Swift suites.

#[test]
fn only_the_arrow_drags_blocks() {
    let (f, mut m, mut v) = arrangement();
    m.set_tool(vizdaw_app::editor::Tool::Eraser);
    v.down(&mut m, ON_CHORDS, NONE);
    v.drag(&mut m, p(300., 70.), NONE);
    v.up(&mut m, p(300., 70.), NONE);
    assert!(f.calls().is_empty());
}

#[test]
fn a_drag_from_empty_space_opens_nothing() {
    let (f, mut m, mut v) = arrangement();
    v.down(&mut m, p(400., 70.), NONE);
    v.drag(&mut m, p(500., 70.), NONE);
    v.up(&mut m, p(500., 70.), NONE);
    assert!(f.calls().is_empty());
}
