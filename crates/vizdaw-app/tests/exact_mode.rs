//! Exact Mode (slice 10b Task 8): a note or block drag opens the Select and
//! Modify panel with its amounts instead of applying (Swift
//! `ExactModeTests`, `ExactGestureTests` and the two exact cases of
//! `SelectModifyModelTests`).

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;

use support::{FakeSession, edit_calls, editor_fake, roll_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{
    ArrangementFrame, ArrangementHit, BlockDragState, Geometry, MachineEvent, Modifiers, Point,
    TimeAxis, ToolMachine, hit_test,
};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Tool};
use vizdaw_app::piano_roll::{PitchAxis, RollFrame, RollGeometry, RollPointer};
use vizdaw_app::select_modify::model::{DroppedAxis, ModifyRequest};
use vizdaw_app::select_modify::{
    GestureOrigin, ModifyConfig, PanelTask, exact_request, select_request,
};
use vizdaw_app::session::DocSession;
use vizdaw_app::strip_chart::{StripLane, line_command};
use vizdaw_ffi::VizError;
use vizdaw_ffi::blocks::BlockRow;
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneEdit, LaneRef};
use vizdaw_ffi::events::{EventKindRow, PerNoteKindRow};
use vizdaw_ffi::modify::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, ModifyRow, SelectModeRow, ValueTargetRow,
};
use vizdaw_ffi::selection::SelectionRow;
use vizdaw_ffi::strip::{LineOpRow, LineRow, LineShapeRow, LineTargetRow, SeriesRow};

const CHORDS: (u64, u64) = (3, 31);
const Q16: i64 = 15_360 / 4;

fn chords() -> BTreeSet<(u64, u64)> {
    [CHORDS].into()
}

fn ev(edit: EventEdit) -> EditCommand {
    EditCommand::Event { edit }
}

fn mv(events: &[u64], delta: i64, to_lane: Option<LaneRef>) -> EditCommand {
    ev(EventEdit::Move {
        events: events.to_vec(),
        delta,
        to_lane,
    })
}

fn nudge(ticks: i64, semitones: i8) -> EditCommand {
    ev(EventEdit::Nudge {
        events: vec![1],
        ticks,
        semitones,
    })
}

fn duplicate(events: &[u64], ticks: i64, semitones: i8) -> EditCommand {
    ev(EventEdit::Duplicate {
        events: events.to_vec(),
        ticks,
        semitones,
    })
}

fn move200() -> EditCommand {
    mv(&[1], 200, None)
}

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

fn request(cmd: &EditCommand, origin: GestureOrigin) -> ModifyRequest {
    exact_request(cmd, &chords(), origin).expect("a request")
}

// MARK: ExactModeTests

#[test]
fn a_move_opens_move_on_the_dragged_events() {
    let r = request(&mv(&[1, 2], 100, None), GestureOrigin::Notes);
    assert_eq!(r.task, PanelTask::Move);
    assert_eq!(r.config.modify.op, ModifyRow::Move { delta: 100 });
    assert!(!r.config.modify.to_copy);
    assert_eq!(r.events, Some(vec![1, 2]));
    assert_eq!(r.lanes, vec![CHORDS]);
    assert!(!r.config.select.enabled);
}

#[test]
fn copies_and_transposes() {
    let copy = request(&duplicate(&[1], 50, 0), GestureOrigin::Notes);
    assert_eq!(copy.config.modify.op, ModifyRow::Move { delta: 50 });
    assert!(copy.config.modify.to_copy);
    let harmony = request(&duplicate(&[1], 0, 3), GestureOrigin::Notes);
    assert_eq!(harmony.task, PanelTask::Transpose);
    assert_eq!(
        harmony.config.modify.op,
        ModifyRow::TransposeInterval { semitones: 3 }
    );
    assert!(harmony.config.modify.to_copy);
    let up = request(&nudge(0, -2), GestureOrigin::Notes);
    assert_eq!(up.task, PanelTask::Transpose);
    assert_eq!(
        up.config.modify.op,
        ModifyRow::TransposeInterval { semitones: -2 }
    );
    let longer = request(
        &ev(EventEdit::Resize {
            events: vec![1],
            delta: 960,
        }),
        GestureOrigin::Notes,
    );
    assert_eq!(longer.task, PanelTask::Change);
    assert_eq!(
        longer.config.modify.op,
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Add,
            amount: AmountRow::Constant { value: 960 },
        }
    );
}

#[test]
fn a_same_track_block_copy_opens_move_to_a_copy() {
    let r = request(&duplicate(&[1, 2], 61_440, 0), GestureOrigin::Blocks);
    assert_eq!(r.task, PanelTask::Move);
    assert_eq!(r.config.modify.op, ModifyRow::Move { delta: 61_440 });
    assert!(r.config.modify.to_copy);
    assert_eq!(r.events, Some(vec![1, 2]));
    assert_eq!(r.dropped, None);
    // A copy into another track applies as usual.
    let other = ev(EventEdit::Insert {
        seq: 3,
        lane: 30,
        events: vec![],
    });
    let both: BTreeSet<_> = [CHORDS, (3, 30)].into();
    assert_eq!(exact_request(&other, &both, GestureOrigin::Blocks), None);
}

#[test]
fn other_drags_apply_as_usual() {
    let to = LaneRef { seq: 3, lane: 32 };
    let cases = [
        mv(&[1], 5, Some(to)),
        ev(EventEdit::Delete { events: vec![1] }),
        ev(EventEdit::Insert {
            seq: 3,
            lane: 31,
            events: vec![],
        }),
        EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq: 3,
                lane: 31,
                name: "x".into(),
            },
        },
    ];
    for c in cases {
        assert_eq!(
            exact_request(&c, &chords(), GestureOrigin::Notes),
            None,
            "{c:?}"
        );
    }
}

#[test]
fn region_moves_apply_as_usual() {
    let m = mv(&[1], 100, None);
    for o in [
        GestureOrigin::RegionStrip,
        GestureOrigin::Expression,
        GestureOrigin::Other,
    ] {
        assert_eq!(exact_request(&m, &chords(), o), None, "{o:?}");
    }
    assert!(exact_request(&m, &chords(), GestureOrigin::Notes).is_some());
    assert!(exact_request(&m, &chords(), GestureOrigin::Blocks).is_some());
}

#[test]
fn diagonal_drags_keep_their_stronger_axis() {
    let side = request(&nudge(4 * Q16, 3), GestureOrigin::Notes);
    assert_eq!(side.task, PanelTask::Move);
    assert_eq!(side.config.modify.op, ModifyRow::Move { delta: 4 * Q16 });
    assert_eq!(side.dropped, Some(DroppedAxis::Vertical));
    let up = request(&duplicate(&[1], 5, 3), GestureOrigin::Notes);
    assert_eq!(up.task, PanelTask::Transpose);
    assert_eq!(
        up.config.modify.op,
        ModifyRow::TransposeInterval { semitones: 3 }
    );
    assert!(up.config.modify.to_copy);
    assert_eq!(up.dropped, Some(DroppedAxis::Horizontal));
    assert_eq!(
        up.dropped.unwrap().message(),
        "The horizontal part of the drag was left out."
    );
    let tie = request(&nudge(3 * Q16, -3), GestureOrigin::Notes);
    assert_eq!(tie.task, PanelTask::Move, "time wins ties");
    let pure = request(&nudge(0, 2), GestureOrigin::Notes);
    assert_eq!(pure.dropped, None);
}

#[test]
fn the_panel_carries_the_drags_exact_amounts() {
    let resize = request(
        &ev(EventEdit::Resize {
            events: vec![7],
            delta: -1_234,
        }),
        GestureOrigin::Notes,
    );
    assert_eq!(
        resize.config.modify.op,
        ModifyRow::Change {
            target: ValueTargetRow::Duration,
            op: ChangeOpRow::Add,
            amount: AmountRow::Constant { value: -1_234 },
        }
    );
    assert_eq!(
        vizdaw_app::select_modify::modify_numbers(&resize.config.modify.op),
        vec![-1_234]
    );
    assert_eq!(resize.events, Some(vec![7]));
    let n = request(
        &ev(EventEdit::Nudge {
            events: vec![7],
            ticks: 777,
            semitones: 0,
        }),
        GestureOrigin::Notes,
    );
    assert_eq!(
        vizdaw_app::select_modify::modify_numbers(&n.config.modify.op),
        vec![777]
    );
    assert_eq!(n.config.modify.op, ModifyRow::Move { delta: 777 });
}

#[test]
fn a_rubber_band_becomes_a_select_rule() {
    let r = select_request((0, 15_360), (60, 64), vec![CHORDS], true);
    assert_eq!(r.task, PanelTask::SelectByRule);
    assert_eq!(r.events, None);
    assert_eq!(r.range, Some((0, 15_360)));
    assert!(r.config.select.enabled);
    assert_eq!(r.config.select.mode, SelectModeRow::Add);
    assert!(!r.config.modify.enabled);
    assert_eq!(
        r.config.select.conditions,
        vec![
            ConditionRow {
                test: ConditionTest::Kinds {
                    kinds: vec![EventKindRow::Note],
                },
                negate: false,
            },
            ConditionRow {
                test: ConditionTest::TimeRange {
                    start: 0,
                    end: 15_360,
                },
                negate: false,
            },
            ConditionRow {
                test: ConditionTest::Pitch { low: 60, high: 64 },
                negate: false,
            },
        ]
    );
}

#[test]
fn a_roll_scale_time_opens_change_with_scale_time() {
    let cmd = EditCommand::SelectAndModify {
        rule: None,
        mode: SelectModeRow::Select,
        modify: Some(ModifyRow::ScaleTime {
            num: 3,
            den: 2,
            anchor: 0,
        }),
        to_copy: false,
    };
    let r = request(&cmd, GestureOrigin::Notes);
    assert_eq!(r.task, PanelTask::Change);
    assert_eq!(
        r.config.modify.op,
        ModifyRow::ScaleTime {
            num: 3,
            den: 2,
            anchor: 0,
        }
    );
    assert_eq!(r.events, None);
    assert_eq!(exact_request(&cmd, &chords(), GestureOrigin::Blocks), None);
}

// MARK: ExactGestureTests

fn open_with(exact: bool, f: FakeSession) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(f);
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.set_exact_mode(exact);
    f.clear_calls();
    (f, m)
}

fn open(exact: bool) -> (Arc<FakeSession>, DocumentModel) {
    open_with(exact, editor_fake())
}

/// Whether the panel asked to be shown, and is open.
fn presented(m: &mut DocumentModel) -> bool {
    m.take_modify_present() && m.select_modify().is_open()
}

#[test]
fn an_exact_drag_leaves_no_undo_step_and_opens_the_panel() {
    let (f, mut m) = open(true);
    assert!(m.begin_gesture_from("Move Notes", vec![CHORDS], GestureOrigin::Notes));
    m.update_gesture(Some(move200()));
    m.end_gesture(Some(move200()));
    assert_eq!(
        edit_calls(&f),
        ["begin_edit(Move Notes)", "edit_gesture", "abort_edit"],
        "previewed, then put back"
    );
    assert!(m.gesture().is_none());
    assert_eq!(m.snapshot().undo_label, None);
    assert!(presented(&mut m));
    assert_eq!(m.select_modify().task, PanelTask::Move);
    assert_eq!(
        m.select_modify().config.modify.op,
        ModifyRow::Move { delta: 200 }
    );
    let sel = f.selection().unwrap();
    assert_eq!(sel.events, vec![1], "it acts on the dragged notes");
    assert_eq!(sel.lanes, vec![LaneRef { seq: 3, lane: 31 }]);
    assert!(f.edits().is_empty());
}

#[test]
fn the_baseline_is_taken_after_the_revert_and_the_selection() {
    let (_, mut m) = open(true);
    assert!(m.begin_gesture_from("Move Block", vec![CHORDS], GestureOrigin::Blocks));
    m.end_gesture(Some(move200()));
    // The request is live: the selection is the one it set, not stale.
    assert!(m.modify_acts_on_dragged_events());
}

#[test]
fn a_diagonal_drag_shows_the_dropped_axis_notice() {
    let (_, mut m) = open(true);
    assert!(m.begin_gesture_from("Move Notes", vec![CHORDS], GestureOrigin::Notes));
    m.end_gesture(Some(nudge(15_360, 1)));
    assert_eq!(m.modify_notice(), Some(DroppedAxis::Vertical.message()));
}

#[test]
fn only_note_and_block_drags_go_exact() {
    for origin in [
        GestureOrigin::RegionStrip,
        GestureOrigin::Expression,
        GestureOrigin::Other,
    ] {
        let (f, mut m) = open(true);
        assert!(m.begin_gesture_from("Drag", vec![CHORDS], origin));
        m.end_gesture(Some(move200()));
        assert_eq!(f.calls().last().unwrap(), "end_edit", "{origin:?}");
        assert!(!presented(&mut m), "{origin:?} applies as usual");
    }
    for origin in [GestureOrigin::Notes, GestureOrigin::Blocks] {
        let (f, mut m) = open(true);
        assert!(m.begin_gesture_from("Drag", vec![CHORDS], origin));
        m.end_gesture(Some(move200()));
        assert_eq!(edit_calls(&f).last().unwrap(), "abort_edit", "{origin:?}");
        assert!(presented(&mut m), "{origin:?} opens the panel");
    }
}

#[test]
fn begin_gesture_is_an_other_gesture() {
    let (f, mut m) = open(true);
    assert!(m.begin_gesture("Drag", vec![CHORDS]));
    assert_eq!(m.gesture().unwrap().origin, GestureOrigin::Other);
    m.end_gesture(Some(move200()));
    assert_eq!(f.calls().last().unwrap(), "end_edit");
}

#[test]
fn a_cross_track_move_applies_as_usual() {
    let (f, mut m) = open(true);
    let cross = mv(&[1], 0, Some(LaneRef { seq: 3, lane: 30 }));
    assert!(m.begin_gesture_from("Move Block", vec![CHORDS], GestureOrigin::Blocks));
    m.end_gesture(Some(cross));
    assert_eq!(f.calls().last().unwrap(), "end_edit");
    assert!(!presented(&mut m));
}

#[test]
fn escape_in_exact_mode_opens_nothing() {
    let (f, mut m) = open(true);
    assert!(m.begin_gesture_from("Move Notes", vec![CHORDS], GestureOrigin::Notes));
    m.update_gesture(Some(move200()));
    m.cancel_gesture();
    assert!(!presented(&mut m));
    assert!(!m.select_modify().is_open());
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
}

#[test]
fn escape_on_a_busy_exact_revert_opens_nothing() {
    let (f, mut m) = open(true);
    f.fail_next("abort_edit", busy());
    assert!(m.begin_gesture_from("Move Notes", vec![CHORDS], GestureOrigin::Notes));
    m.update_gesture(Some(move200()));
    m.end_gesture(Some(move200()));
    assert!(m.gesture().is_some());
    // Escape while the revert waits: the retry puts the drag back only.
    m.cancel_gesture();
    m.tick();
    assert!(m.gesture().is_none());
    assert!(!presented(&mut m));
    assert!(f.edits().is_empty());
}

#[test]
fn a_pencil_click_in_exact_mode_still_inserts() {
    let (f, mut m) = open(true);
    let insert = ev(EventEdit::Insert {
        seq: 3,
        lane: 31,
        events: vec![],
    });
    assert!(m.begin_gesture_from("Insert Note", vec![CHORDS], GestureOrigin::Other));
    m.end_gesture(Some(insert));
    assert_eq!(f.calls().last().unwrap(), "end_edit");
    assert!(!presented(&mut m));
}

#[test]
fn with_exact_mode_off_a_drag_applies() {
    let (f, mut m) = open(false);
    assert!(m.begin_gesture_from("Move Notes", vec![CHORDS], GestureOrigin::Notes));
    m.update_gesture(Some(move200()));
    m.end_gesture(Some(move200()));
    assert_eq!(f.calls().last().unwrap(), "end_edit");
    assert!(!presented(&mut m));
}

#[test]
fn a_busy_revert_keeps_the_gesture_and_the_retry_opens_the_panel() {
    let (f, mut m) = open(true);
    f.fail_next("abort_edit", busy());
    assert!(m.begin_gesture_from("Move Notes", vec![CHORDS], GestureOrigin::Notes));
    m.update_gesture(Some(move200()));
    m.end_gesture(Some(move200()));
    assert!(
        m.gesture().is_some(),
        "the session edit is still open, so the gesture is too"
    );
    assert!(
        !presented(&mut m) && !m.select_modify().is_open(),
        "no panel over an unreverted document"
    );
    // A move or a second mouse-up meanwhile sends nothing.
    m.update_gesture(Some(mv(&[1], 300, None)));
    m.end_gesture(Some(mv(&[1], 300, None)));
    // The fake logs only the calls that went through: the Busy abort is
    // not in it.
    assert_eq!(edit_calls(&f), ["begin_edit(Move Notes)", "edit_gesture"]);
    assert!(f.edit_is_open());
    m.tick();
    assert!(m.gesture().is_none());
    assert_eq!(
        edit_calls(&f),
        ["begin_edit(Move Notes)", "edit_gesture", "abort_edit"]
    );
    assert!(presented(&mut m), "the revert succeeded on the retry");
    assert_eq!(m.select_modify().task, PanelTask::Move);
    assert_eq!(
        m.select_modify().config.modify.op,
        ModifyRow::Move { delta: 200 }
    );
    // Further ticks open it no more, and nothing was applied.
    m.tick();
    assert!(!m.take_modify_present());
    assert_eq!(f.count("abort_edit"), 1);
    assert_eq!(f.count("end_edit"), 0);
    assert!(f.edits().is_empty());
    assert!(!f.edit_is_open());
}

#[test]
fn a_failed_revert_opens_no_panel() {
    let (f, mut m) = open(true);
    f.fail_next(
        "abort_edit",
        VizError::Internal {
            message: "boom".into(),
        },
    );
    assert!(m.begin_gesture_from("Move Notes", vec![CHORDS], GestureOrigin::Notes));
    m.end_gesture(Some(move200()));
    assert!(!presented(&mut m));
    assert!(!m.select_modify().is_open());
    assert!(m.gesture().is_none());
    m.tick();
    assert!(!presented(&mut m));
}

#[test]
fn escape_on_a_busy_abort_is_retried_by_the_tick() {
    let (f, mut m) = open(false);
    f.fail_next("abort_edit", busy());
    assert!(m.begin_gesture_from("Move Notes", vec![CHORDS], GestureOrigin::Notes));
    m.update_gesture(Some(move200()));
    m.cancel_gesture();
    assert!(m.gesture().is_some());
    m.update_gesture(None);
    m.end_gesture(Some(move200()));
    assert_eq!(
        edit_calls(&f),
        ["begin_edit(Move Notes)", "edit_gesture"],
        "a reverting gesture takes no more edits (the Busy abort is not logged)"
    );
    m.tick();
    assert!(m.gesture().is_none());
    assert_eq!(edit_calls(&f).last().unwrap(), "abort_edit");
    assert!(!presented(&mut m));
    assert_eq!(f.count("abort_edit"), 1);
    assert!(!f.edit_is_open());
}

#[test]
fn a_shift_band_in_exact_mode_keeps_the_selection() {
    let (f, mut m) = open(true);
    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 3, lane: 31 }],
        events: vec![2],
    });
    m.request_exact(select_request((0, 100), (60, 64), vec![CHORDS], true));
    assert_eq!(f.selection().unwrap().events, vec![2]);
    assert_eq!(m.selection().events, vec![2]);
    assert!(presented(&mut m));
    assert_eq!(m.select_modify().task, PanelTask::SelectByRule);
}

// MARK: Each view passes its own origin

const ON_CHORDS: Point = Point { x: 50., y: 70. };

/// The arrangement canvas of `tests/block_drag.rs`.
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
                axis: TimeAxis::new(384., 0),
            },
        }
    }

    fn handle(&mut self, m: &mut DocumentModel, events: Vec<MachineEvent<ArrangementHit>>) {
        m.block_drag_events(&mut self.drag, &events, &self.frame);
    }

    fn down(&mut self, m: &mut DocumentModel, at: Point) {
        let rows: Vec<(u64, Arc<[BlockRow]>)> =
            m.lanes().iter().map(|l| (l.id, m.blocks(l.id))).collect();
        let hit = hit_test(&self.frame.geometry, &self.frame.axis, &rows, at);
        let fx = self.machine.down(hit, at, 1, Modifiers::NONE);
        self.handle(m, fx);
    }

    fn drag(&mut self, m: &mut DocumentModel, to: Point) {
        let fx = self.machine.drag(to, Modifiers::NONE);
        self.handle(m, fx);
    }

    fn up(&mut self, m: &mut DocumentModel, at: Point) {
        let fx = self.machine.up(at, Modifiers::NONE);
        self.handle(m, fx);
    }

    fn cancel(&mut self, m: &mut DocumentModel) {
        m.cancel_block_drag(&mut self.machine, &mut self.drag, &self.frame);
    }
}

#[test]
fn an_arrangement_block_drag_goes_exact() {
    let (f, mut m) = open(true);
    let mut v = Canvas::new();
    v.down(&mut m, ON_CHORDS);
    v.drag(&mut m, Point { x: 300., y: 70. });
    v.up(&mut m, Point { x: 300., y: 70. });
    assert_eq!(
        edit_calls(&f),
        ["begin_edit(Move Block)", "edit_gesture", "abort_edit"]
    );
    assert!(presented(&mut m));
    assert_eq!(m.select_modify().task, PanelTask::Move);
    assert_eq!(m.snapshot().undo_label, None);
}

#[test]
fn escape_during_an_arrangement_exact_drag_opens_nothing() {
    let (f, mut m) = open(true);
    let mut v = Canvas::new();
    v.down(&mut m, ON_CHORDS);
    v.drag(&mut m, Point { x: 300., y: 70. });
    v.cancel(&mut m);
    v.up(&mut m, Point { x: 300., y: 70. });
    assert!(!presented(&mut m));
    assert_eq!(f.calls().last().unwrap(), "abort_edit");
}

/// The roll canvas of `tests/note_drag.rs` on Chords (31).
struct Roll {
    pointer: RollPointer,
    frame: RollFrame,
}

impl Roll {
    fn on(m: &mut DocumentModel) -> Roll {
        m.open_detail(
            EditorTarget::Lane { seq: 3, lane: 31 },
            Some(DetailEditorKind::PianoRoll),
        );
        let frame = m.roll_frame(
            TimeAxis::new(96., 0),
            PitchAxis::new(10., 0.).centering(62, 400.),
            800.,
            400.,
        );
        Roll {
            pointer: RollPointer::default(),
            frame,
        }
    }

    fn down(&mut self, m: &mut DocumentModel, at: Point, modifiers: Modifiers) {
        m.roll_mouse_down(&mut self.pointer, &self.frame, at, 1, modifiers);
    }

    fn drag(&mut self, m: &mut DocumentModel, to: Point, modifiers: Modifiers) {
        m.roll_mouse_drag(&mut self.pointer, &self.frame, to, modifiers);
    }

    fn up(&mut self, m: &mut DocumentModel, at: Point, modifiers: Modifiers) {
        m.roll_mouse_up(&mut self.pointer, &self.frame, at, modifiers);
    }

    /// The middle of note `id`.
    fn middle(&self, m: &DocumentModel, id: u64) -> Point {
        let n = m
            .roll_lane_notes()
            .iter()
            .flat_map(|l| l.notes().iter().cloned().collect::<Vec<_>>())
            .find(|n| n.id == id)
            .unwrap();
        let r = RollGeometry::rect(&n, &self.frame.axis, &self.frame.pitch);
        Point {
            x: r.x + r.width / 2.,
            y: r.y + r.height / 2.,
        }
    }
}

#[test]
fn a_piano_roll_note_drag_goes_exact_but_a_pencil_click_does_not() {
    let (f, mut m) = open_with(true, roll_fake());
    let mut v = Roll::on(&mut m);
    f.clear_calls();
    let p = v.middle(&m, 1);
    let up2 = Point {
        x: p.x,
        y: p.y - 2. * v.frame.pitch.row_height(),
    };
    v.down(&mut m, p, Modifiers::NONE);
    v.drag(&mut m, up2, Modifiers::NONE);
    v.up(&mut m, up2, Modifiers::NONE);
    assert!(presented(&mut m));
    assert_eq!(m.select_modify().task, PanelTask::Transpose);
    assert!(m.gesture().is_none());
    assert!(f.edits().is_empty() && m.snapshot().undo_label.is_none());
    m.modify_panel_closed();
    m.set_tool(Tool::Pencil);
    f.clear_calls();
    let q = Point {
        x: 400.,
        y: v.frame.pitch.center(67),
    };
    v.down(&mut m, q, Modifiers::NONE);
    v.up(&mut m, q, Modifiers::NONE);
    assert_eq!(edit_calls(&f).last().unwrap(), "end_edit", "pencil applies");
    assert!(!presented(&mut m));
}

#[test]
fn an_exact_rubber_band_in_the_roll_opens_select_by_rule() {
    let (f, mut m) = open_with(true, roll_fake());
    let mut v = Roll::on(&mut m);
    let from = Point { x: 600., y: 10. };
    let to = Point { x: 700., y: 200. };
    v.down(&mut m, from, Modifiers::SHIFT);
    v.drag(&mut m, to, Modifiers::SHIFT);
    v.up(&mut m, to, Modifiers::SHIFT);
    assert!(presented(&mut m));
    let s = m.select_modify();
    assert_eq!(s.task, PanelTask::SelectByRule);
    assert_eq!(s.config.select.mode, SelectModeRow::Add);
    let start = v.frame.axis.tick_at(600.);
    let end = v.frame.axis.tick_at(700.);
    let (lo, hi) = (v.frame.pitch.pitch_at(200.), v.frame.pitch.pitch_at(10.));
    assert_eq!(
        s.config.select.conditions[1..],
        [
            ConditionRow {
                test: ConditionTest::TimeRange { start, end },
                negate: false,
            },
            ConditionRow {
                test: ConditionTest::Pitch { low: lo, high: hi },
                negate: false,
            },
        ]
    );
    assert_eq!(
        f.selection().unwrap().lanes,
        vec![LaneRef { seq: 3, lane: 31 }],
        "the shown tracks are the selection's"
    );
}

#[test]
fn a_region_move_applies_in_exact_mode() {
    // The region strip is not ported; its gesture's origin is what counts.
    let (f, mut m) = open(true);
    assert!(m.begin_gesture_from("Move Region", vec![CHORDS], GestureOrigin::RegionStrip));
    m.update_gesture(Some(move200()));
    m.end_gesture(Some(move200()));
    assert_eq!(f.calls().last().unwrap(), "end_edit");
    assert!(!presented(&mut m));
}

#[test]
fn an_expression_stroke_applies_in_exact_mode() {
    // The expression pane is not ported; its gesture's origin is what
    // counts.
    let (f, mut m) = open(true);
    let stroke = ev(EventEdit::SetPerNote {
        events: vec![1],
        kind: PerNoteKindRow::Registered { index: 1 },
        from: 0,
        to: 100,
        points: vec![],
    });
    assert!(m.begin_gesture_from("Draw Expression", vec![CHORDS], GestureOrigin::Expression));
    m.end_gesture(Some(stroke));
    assert_eq!(f.calls().last().unwrap(), "end_edit");
    assert!(!presented(&mut m));
}

// MARK: SelectModifyModelTests (exact cases)

#[test]
fn exact_mode_selects_the_dragged_events_and_applies_off_grid_amounts_unedited() {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    let off = 1_234; // not on the length display grid
    let mut config = ModifyConfig::preset(PanelTask::Change, None);
    config.modify.op = ModifyRow::Change {
        target: ValueTargetRow::Duration,
        op: ChangeOpRow::Add,
        amount: AmountRow::Constant { value: off },
    };
    m.open_modify_request(ModifyRequest {
        task: PanelTask::Change,
        config,
        events: Some(vec![1, 2]),
        lanes: vec![CHORDS],
        range: None,
        dropped: Some(DroppedAxis::Vertical),
    });
    assert_eq!(m.modify_notice(), Some(DroppedAxis::Vertical.message()));
    assert!(m.apply_modify());
    assert_eq!(f.selection().unwrap().events, vec![1, 2]);
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::SelectAndModify {
            rule: None,
            mode: SelectModeRow::Select,
            modify: Some(ModifyRow::Change {
                target: ValueTargetRow::Duration,
                op: ChangeOpRow::Add,
                amount: AmountRow::Constant { value: off },
            }),
            to_copy: false,
        })
    );
    assert_eq!(m.modify_notice(), None);
}

#[test]
fn exact_mode_selection_is_set_before_the_command() {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_modify_request(ModifyRequest {
        task: PanelTask::Move,
        config: ModifyConfig::preset(PanelTask::Move, None),
        events: Some(vec![1]),
        lanes: vec![CHORDS],
        range: None,
        dropped: None,
    });
    assert!(m.take_modify_present() && m.select_modify().is_open());
    f.clear_calls();
    assert!(m.apply_modify());
    let calls = f.calls();
    let set = calls.iter().position(|c| c == "set_selection").unwrap();
    let edit = calls
        .iter()
        .position(|c| c.starts_with("edit"))
        .expect("an edit");
    assert!(set < edit, "{calls:?}");
    let sel = f.selection().unwrap();
    assert_eq!(sel.events, vec![1]);
    assert_eq!(sel.lanes, vec![LaneRef { seq: 3, lane: 31 }]);
}

// MARK: The Strip Chart (slice 10c Task 7, `T/StripChartTests.swift`)

fn cc7_line(from: i64, to: i64) -> LineRow {
    LineRow {
        from,
        to,
        start: 0,
        end: 100,
        step: 1,
        shape: LineShapeRow::Straight,
    }
}

#[test]
fn exact_mode_keeps_the_option_refine() {
    let l = cc7_line(4 * Q16, 8 * Q16);
    let cmd = line_command(StripLane::Controller(7), LineOpRow::Add, l.clone(), true);
    let r = request(&cmd, GestureOrigin::StripChart);
    assert_eq!(
        r.config.select.mode,
        SelectModeRow::Refine,
        "⌥ stays a refine in the panel"
    );
    let plain = line_command(StripLane::Controller(7), LineOpRow::Add, l, false);
    let r = request(&plain, GestureOrigin::StripChart);
    assert_eq!(r.config.select.mode, SelectModeRow::Select);
}

#[test]
fn exact_mode_opens_the_line_in_the_panel() {
    let q = 4 * Q16;
    let l = cc7_line(q, 2 * q);
    let cmd = line_command(StripLane::Controller(7), LineOpRow::Add, l.clone(), false);
    let r = request(&cmd, GestureOrigin::StripChart);
    assert_eq!(r.task, PanelTask::Change);
    assert_eq!(
        r.config.modify.op,
        ModifyRow::Line {
            target: LineTargetRow::Series {
                series: SeriesRow::Controller { index: 7 }
            },
            op: LineOpRow::Add,
            line: l,
        }
    );
    assert!(r.config.modify.enabled && !r.config.modify.to_copy);
    assert!(r.config.select.enabled);
    assert_eq!(r.config.select.conditions.len(), 1);
    assert_eq!(r.events, None);
    assert_eq!(r.lanes, vec![CHORDS]);
    assert_eq!(r.range, Some((q, 2 * q + 1)));
    assert_eq!(r.dropped, None);
    assert_eq!(
        exact_request(
            &ev(EventEdit::Delete { events: vec![1] }),
            &BTreeSet::new(),
            GestureOrigin::StripChart
        ),
        None
    );
    // A note drag's command from the chart is not a line either.
    assert_eq!(
        exact_request(&move200(), &chords(), GestureOrigin::StripChart),
        None
    );
}

#[test]
fn a_line_without_a_rule_turns_select_off() {
    let l = cc7_line(0, 4 * Q16);
    let EditCommand::SelectAndModify { mode, modify, .. } =
        line_command(StripLane::Controller(7), LineOpRow::Set, l, false)
    else {
        panic!("a line is a select and modify");
    };
    let cmd = EditCommand::SelectAndModify {
        rule: None,
        mode,
        modify,
        to_copy: true,
    };
    let r = request(&cmd, GestureOrigin::StripChart);
    assert!(!r.config.select.enabled && r.config.select.conditions.is_empty());
    assert!(!r.config.modify.to_copy, "a line never goes to a copy");
}

#[test]
fn a_line_request_at_the_last_tick_does_not_overflow() {
    let l = cc7_line(0, i64::MAX);
    let cmd = line_command(StripLane::Controller(7), LineOpRow::Set, l, false);
    let r = request(&cmd, GestureOrigin::StripChart);
    assert_eq!(r.range, Some((0, i64::MAX)));
}
