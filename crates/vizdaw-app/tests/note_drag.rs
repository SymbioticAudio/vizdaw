//! Swift `PianoRollViewTests`' gesture cases, played against the model:
//! the tool machine, the roll's hit test and its drags together. The
//! audition requests are replayed through Swift's `sound`/`silence`
//! rules (the audition itself comes with Task 3) so the logs read as
//! Swift's.

mod support;

use std::sync::Arc;

use support::{FakeSession, note_row, roll_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{Modifiers, Point, TimeAxis};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, GESTURE_OPEN, LANE_REMOVED, Snap, Tool};
use vizdaw_app::piano_roll::{
    AuditionRequest, CursorKind, NoteZone, Pencil, PitchAxis, RollFrame, RollGeometry, RollHit,
    RollPointer,
};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::EventRow;

const NONE: Modifiers = Modifiers::NONE;
const SHIFT: Modifiers = Modifiers::SHIFT;
const OPTION: Modifiers = Modifiers::OPTION;
const COMMAND: Modifiers = Modifiers::COMMAND;
const CHORDS: LaneRef = LaneRef { seq: 3, lane: 31 };
const BASS: LaneRef = LaneRef { seq: 3, lane: 32 };

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

fn ev(edit: EventEdit) -> EditCommand {
    EditCommand::Event { edit }
}

/// The roll canvas: 800 × 400 points of note area at the default zoom
/// (160 pt per quarter), rows of 10 centred on D4.
struct Roll {
    f: Arc<FakeSession>,
    m: DocumentModel,
    pointer: RollPointer,
    frame: RollFrame,
    requests: Vec<AuditionRequest>,
}

impl Roll {
    fn on(f: FakeSession, target: EditorTarget, lanes: Option<Vec<u64>>) -> Roll {
        let f = Arc::new(f);
        let mut m = DocumentModel::new(f.clone()).unwrap();
        m.open_detail(target, Some(DetailEditorKind::PianoRoll));
        if let Some(lanes) = lanes {
            m.update_layout(|l| l.detail.as_mut().unwrap().set_lanes(Some(lanes)));
        }
        let frame = m.roll_frame(
            TimeAxis::new(96., 0),
            PitchAxis::new(10., 0.).centering(62, 400.),
            800.,
            400.,
        );
        f.clear_calls();
        Roll {
            f,
            m,
            pointer: RollPointer::default(),
            frame,
            requests: vec![],
        }
    }

    /// Chords (31) of `roll_fake`: notes 1 (C4 at 0) and 2 (E4 at a
    /// quarter).
    fn chords() -> Roll {
        Roll::on(roll_fake(), EditorTarget::Lane { seq: 3, lane: 31 }, None)
    }

    fn down(&mut self, at: Point, modifiers: Modifiers) {
        let o = self
            .m
            .roll_mouse_down(&mut self.pointer, &self.frame, at, 1, modifiers);
        self.requests.extend(o.audition);
    }

    fn drag(&mut self, to: Point, modifiers: Modifiers) -> Option<vizdaw_app::arrangement::Rect> {
        let o = self
            .m
            .roll_mouse_drag(&mut self.pointer, &self.frame, to, modifiers);
        self.requests.extend(o.audition);
        o.band
    }

    fn up(&mut self, at: Point, modifiers: Modifiers) {
        let o = self
            .m
            .roll_mouse_up(&mut self.pointer, &self.frame, at, modifiers);
        assert_eq!(o.band, None);
        self.requests.extend(o.audition);
    }

    fn click(&mut self, at: Point, modifiers: Modifiers) {
        self.down(at, modifiers);
        self.up(at, modifiers);
    }

    fn cancel(&mut self) {
        let o = self.m.cancel_roll_drag(&mut self.pointer, &self.frame);
        self.requests.extend(o.audition);
    }

    fn note(&self, id: u64) -> EventRow {
        self.m
            .roll_lane_notes()
            .iter()
            .flat_map(|l| l.notes().iter().cloned().collect::<Vec<_>>())
            .find(|n| n.id == id)
            .unwrap()
    }

    /// A point inside a zone of note `id`.
    fn point(&self, id: u64, zone: NoteZone) -> Point {
        let r = RollGeometry::rect(&self.note(id), &self.frame.axis, &self.frame.pitch);
        let y = r.y + r.height / 2.;
        match zone {
            NoteZone::Start => Point { x: r.x + 2., y },
            NoteZone::Middle => Point {
                x: r.x + r.width / 2.,
                y,
            },
            NoteZone::End => Point {
                x: r.max_x() - 2.,
                y,
            },
        }
    }

    fn row(&self) -> f32 {
        self.frame.pitch.row_height()
    }

    /// The requests replayed as Swift's audition log: a sound ends the
    /// one before unless it is the same note; no destination is silence.
    fn log(&self) -> Vec<String> {
        let mut out = vec![];
        let mut sounding: Option<(u64, u8)> = None;
        for r in &self.requests {
            match *r {
                AuditionRequest::Sound { pitch, dest, .. } => {
                    let next = dest.map(|d| (d, pitch));
                    if next == sounding {
                        continue;
                    }
                    if let Some((_, p)) = sounding.take() {
                        out.push(format!("off {p}"));
                    }
                    if let Some((_, p)) = next {
                        out.push(format!("on {p}"));
                        sounding = next;
                    }
                }
                AuditionRequest::Silence => {
                    if let Some((_, p)) = sounding.take() {
                        out.push(format!("off {p}"));
                    }
                }
            }
        }
        out
    }

    fn begins(&self) -> Vec<String> {
        self.f
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("begin_edit"))
            .collect()
    }
}

fn up_by(p: Point, rows: f32, row: f32) -> Point {
    Point {
        x: p.x,
        y: p.y - rows * row,
    }
}

fn right_by(p: Point, dx: f32) -> Point {
    Point {
        x: p.x + dx,
        y: p.y,
    }
}

// MARK: Selecting

#[test]
fn pressing_a_note_selects_and_sounds_it_until_release() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    assert_eq!(v.log(), ["on 60"]);
    assert_eq!(
        v.requests[0],
        AuditionRequest::Sound {
            pitch: 60,
            velocity: 0x8000,
            dest: Some(5)
        },
        "Chords' destination"
    );
    v.up(p, NONE);
    assert_eq!(v.log(), ["on 60", "off 60"]);
    assert_eq!(v.m.selection().events, [1]);
    assert_eq!(v.m.selection().lanes, [CHORDS]);
    assert!(v.begins().is_empty(), "selecting is not an edit");
}

#[test]
fn shift_clicks_toggle_and_an_empty_click_clears() {
    let mut v = Roll::chords();
    v.click(v.point(1, NoteZone::Middle), NONE);
    v.click(v.point(2, NoteZone::Middle), SHIFT);
    assert_eq!(v.m.selection().events, [1, 2]);
    v.click(v.point(1, NoteZone::Middle), SHIFT);
    assert_eq!(v.m.selection().events, [2]);
    let empty = Point { x: 700., y: 10. };
    v.click(empty, SHIFT);
    assert_eq!(v.m.selection().events, [2], "⇧ on empty space keeps it");
    v.click(empty, NONE);
    assert!(v.m.selection().events.is_empty());
}

#[test]
fn a_rubber_band_selects_notes() {
    let mut v = Roll::chords();
    let a = v.point(1, NoteZone::Start);
    let b = v.point(2, NoteZone::End);
    // From empty space above E4 to below C4.
    let from = Point {
        x: a.x - 1.,
        y: b.y - 15.,
    };
    let to = Point {
        x: b.x,
        y: a.y + 15.,
    };
    v.down(from, NONE);
    let band = v.drag(to, NONE).expect("a band to draw");
    assert_eq!((band.x, band.y), (from.x, from.y));
    v.up(to, NONE);
    let mut got = v.m.selection().events.clone();
    got.sort();
    assert_eq!(got, [1, 2]);
    assert!(!v.m.gesture_open(), "selecting is not an edit");
    assert!(v.begins().is_empty());
    assert!(v.log().is_empty(), "a band is silent");
}

#[test]
fn an_empty_band_clears_unless_shift_adds() {
    let mut v = Roll::chords();
    v.click(v.point(1, NoteZone::Middle), NONE);
    let (a, b) = (Point { x: 600., y: 5. }, Point { x: 700., y: 30. });
    v.down(a, SHIFT);
    v.drag(b, SHIFT);
    v.up(b, SHIFT);
    assert_eq!(v.m.selection().events, [1]);
    v.down(a, NONE);
    v.drag(b, NONE);
    v.up(b, NONE);
    assert!(v.m.selection().events.is_empty());
}

#[test]
fn the_hit_note_is_the_one_drawn_on_top() {
    let f = roll_fake();
    // Note 4 of Groove (30) lies exactly over note 3 of Bass (32).
    f.set_events(3, 30, vec![note_row(4, 0, 40)]);
    let mut v = Roll::on(f, EditorTarget::Sequence { seq: 3 }, Some(vec![30, 31, 32]));
    v.m.roll_set_edit_lane(CHORDS);
    let p = v.point(3, NoteZone::Middle);
    v.click(p, NONE);
    assert_eq!(
        v.m.selection().events,
        [4],
        "Groove is drawn over Bass, so it is hit"
    );
}

// MARK: Pencil

#[test]
fn pencil_click_inserts_a_grid_note_in_the_edit_lane() {
    let mut v = Roll::chords();
    v.m.set_tool(Tool::Pencil);
    // x 400 is tick 38 400: the note starts on the quarter at 30 720.
    let p = Point {
        x: 400.,
        y: v.frame.pitch.center(67),
    };
    v.click(p, NONE);
    assert_eq!(v.begins(), ["begin_edit(Insert Note)"]);
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&Pencil::command(CHORDS, 30_720, 67, 15_360))
    );
    assert!(v.f.edits().is_empty());
    assert!(!v.m.gesture_open());
    assert_eq!(v.m.snapshot().undo_label.as_deref(), Some("Insert Note"));
    assert_eq!(
        v.log(),
        ["on 67", "off 67"],
        "the new pitch sounds while pressed"
    );
    assert_eq!(
        v.requests[0],
        AuditionRequest::Sound {
            pitch: 67,
            velocity: Pencil::VELOCITY,
            dest: Some(5)
        }
    );
}

#[test]
fn pencil_click_without_snap_is_a_quarter_at_the_tick() {
    let mut v = Roll::chords();
    v.m.set_tool(Tool::Pencil);
    v.m.update_layout(|l| l.snap.enabled = false);
    v.click(
        Point {
            x: 400.,
            y: v.frame.pitch.center(67),
        },
        NONE,
    );
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&Pencil::command(CHORDS, 38_400, 67, 15_360))
    );
}

#[test]
fn pencil_drag_paints_a_length() {
    let mut v = Roll::chords();
    v.m.set_tool(Tool::Pencil);
    let y = v.frame.pitch.center(67);
    v.down(Point { x: 400., y }, NONE);
    v.drag(Point { x: 700., y }, NONE);
    v.up(Point { x: 700., y }, NONE);
    // x 700 is tick 67 200; nearest quarter 61 440: 30 720 long.
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&Pencil::command(CHORDS, 30_720, 67, 30_720))
    );
    assert_eq!(v.begins(), ["begin_edit(Insert Note)"]);
    assert!(!v.m.gesture_open());
    // Back left of the start: one step, never zero.
    v.down(Point { x: 400., y }, NONE);
    v.drag(Point { x: 100., y }, NONE);
    v.up(Point { x: 100., y }, NONE);
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&Pencil::command(CHORDS, 30_720, 67, 15_360))
    );
}

#[test]
fn a_pencil_click_while_a_gesture_finishes_says_so_and_is_silent() {
    let mut v = Roll::chords();
    assert!(v.m.begin_gesture("Move Notes", vec![(3, 31)]));
    v.m.set_tool(Tool::Pencil);
    let p = Point {
        x: 400.,
        y: v.frame.pitch.center(67),
    };
    v.click(p, NONE);
    assert_eq!(v.m.status(), Some(GESTURE_OPEN));
    assert_eq!(v.begins(), ["begin_edit(Move Notes)"]);
    assert_eq!(v.log(), ["on 67", "off 67"]);
    assert_eq!(v.m.gesture().unwrap().label, "Move Notes");
}

// MARK: Eraser

#[test]
fn an_eraser_click_deletes_the_note() {
    let mut v = Roll::chords();
    v.m.set_tool(Tool::Eraser);
    v.click(v.point(2, NoteZone::Middle), NONE);
    assert_eq!(v.f.edits(), [ev(EventEdit::Delete { events: vec![2] })]);
    assert!(v.begins().is_empty() && !v.m.gesture_open());
    assert!(v.log().is_empty(), "the eraser is silent");
}

#[test]
fn the_eraser_deletes_every_note_it_crosses() {
    let mut v = Roll::chords();
    v.m.set_tool(Tool::Eraser);
    let a = v.point(1, NoteZone::Middle);
    let b = v.point(2, NoteZone::Middle);
    v.down(a, NONE);
    v.drag(right_by(a, 10.), NONE);
    assert_eq!(
        v.f.gesture_edits(),
        [ev(EventEdit::Delete { events: vec![1] })],
        "sent once: the second sample adds nothing"
    );
    v.drag(b, NONE);
    v.up(b, NONE);
    assert_eq!(v.begins(), ["begin_edit(Erase Notes)"]);
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Delete { events: vec![1, 2] }))
    );
    assert!(!v.m.gesture_open());
    assert_eq!(v.m.snapshot().undo_label.as_deref(), Some("Erase Notes"));
    assert!(v.log().is_empty(), "the eraser is silent");
}

#[test]
fn a_fast_eraser_drag_erases_the_notes_between_samples() {
    let mut v = Roll::chords();
    v.m.set_tool(Tool::Eraser);
    // A diagonal from below-left of C4 to above-right of E4: neither
    // sample lands on a note, the segment crosses both.
    let a = v.point(1, NoteZone::Middle);
    let b = v.point(2, NoteZone::Middle);
    let from = Point {
        x: a.x - (b.x - a.x) / 4.,
        y: a.y - (b.y - a.y) / 4.,
    };
    let to = Point {
        x: b.x + (b.x - a.x) / 4.,
        y: b.y + (b.y - a.y) / 4.,
    };
    assert!(matches!(
        v.m.roll_hit(&v.frame, from),
        RollHit::Empty { .. }
    ));
    assert!(matches!(v.m.roll_hit(&v.frame, to), RollHit::Empty { .. }));
    v.down(from, NONE);
    v.drag(to, NONE);
    v.up(to, NONE);
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Delete { events: vec![1, 2] }))
    );
}

// MARK: Note drags

#[test]
fn a_middle_drag_transposes_and_resounds_at_the_new_pitch() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::Middle);
    let up2 = up_by(p, 2., v.row());
    v.down(p, NONE);
    v.drag(up2, NONE);
    assert_eq!(v.m.gesture().unwrap().label, "Transpose Notes");
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Nudge {
            events: vec![1],
            ticks: 0,
            semitones: 2
        }))
    );
    v.up(up2, NONE);
    assert!(!v.m.gesture_open());
    assert_eq!(
        v.m.snapshot().undo_label.as_deref(),
        Some("Transpose Notes")
    );
    assert_eq!(v.log(), ["on 60", "off 60", "on 62", "off 62"]);
}

#[test]
fn an_option_start_drag_copies_in_time() {
    let mut v = Roll::chords();
    let p = v.point(2, NoteZone::Start);
    let right = right_by(p, 160.);
    v.down(p, OPTION);
    v.drag(right, OPTION);
    v.up(right, OPTION);
    assert_eq!(v.begins(), ["begin_edit(Copy Notes)"]);
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Duplicate {
            events: vec![2],
            ticks: 15_360,
            semitones: 0
        }))
    );
}

#[test]
fn a_start_drag_moves_snapped_and_raw() {
    let mut v = Roll::chords();
    let p = v.point(2, NoteZone::Start);
    // +100 pt is 9 600 ticks: the start (15 360) goes to 24 960, nearest
    // quarter 30 720.
    let right = right_by(p, 100.);
    v.down(p, NONE);
    v.drag(right, NONE);
    v.up(right, NONE);
    assert_eq!(v.begins(), ["begin_edit(Move Notes)"]);
    let mv = |delta| {
        ev(EventEdit::Move {
            events: vec![2],
            delta,
            to_lane: None,
        })
    };
    assert_eq!(v.f.gesture_edits().last(), Some(&mv(15_360)));
    v.m.update_layout(|l| l.snap.enabled = false);
    v.down(p, NONE);
    v.drag(right, NONE);
    v.up(right, NONE);
    assert_eq!(v.f.gesture_edits().last(), Some(&mv(9_600)));
}

#[test]
fn dragging_a_selected_note_moves_the_selection() {
    let mut v = Roll::chords();
    v.m.roll_select(&[(CHORDS, 1), (CHORDS, 2)], false);
    let p = v.point(2, NoteZone::Start);
    v.down(p, NONE);
    v.drag(right_by(p, 160.), NONE);
    v.up(right_by(p, 160.), NONE);
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Move {
            events: vec![2, 1],
            delta: 15_360,
            to_lane: None
        })),
        "the pressed note first"
    );
    // An unselected note is selected alone first and dragged alone.
    v.m.roll_select(&[(CHORDS, 2)], false);
    let p = v.point(1, NoteZone::Start);
    v.down(p, NONE);
    v.drag(right_by(p, 160.), NONE);
    v.up(right_by(p, 160.), NONE);
    assert_eq!(v.m.selection().events, [1]);
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Move {
            events: vec![1],
            delta: 15_360,
            to_lane: None
        }))
    );
}

#[test]
fn an_end_drag_resizes() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::End);
    // +100 pt is 9 600 ticks: the end (15 360) goes to 24 960, nearest
    // quarter 30 720.
    let right = right_by(p, 100.);
    v.down(p, OPTION);
    v.drag(right, OPTION);
    v.up(right, OPTION);
    assert_eq!(
        v.begins(),
        ["begin_edit(Resize Notes)"],
        "⌥ is ignored at the end"
    );
    let resize = |delta| {
        ev(EventEdit::Resize {
            events: vec![1],
            delta,
        })
    };
    assert_eq!(v.f.gesture_edits().last(), Some(&resize(15_360)));
    v.m.update_layout(|l| {
        l.snap = Snap {
            enabled: false,
            ..l.snap
        }
    });
    v.down(p, NONE);
    v.drag(right, NONE);
    v.up(right, NONE);
    assert_eq!(v.f.gesture_edits().last(), Some(&resize(9_600)));
    // Far left: the note keeps one tick.
    let left = right_by(p, -600.);
    v.down(p, NONE);
    v.drag(left, NONE);
    v.up(left, NONE);
    assert_eq!(v.f.gesture_edits().last(), Some(&resize(1 - 15_360)));
}

#[test]
fn a_group_transpose_clamps_across_lanes() {
    let mut v = Roll::on(roll_fake(), EditorTarget::Sequence { seq: 3 }, None);
    v.m.roll_select(&[(CHORDS, 1), (BASS, 3)], false);
    let p = v.point(1, NoteZone::Middle);
    let down50 = up_by(p, -50., v.row());
    v.down(p, NONE);
    v.drag(down50, NONE);
    v.up(down50, NONE);
    // E2 (40) in Bass is the lowest of the group: it stops at 0.
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Nudge {
            events: vec![1, 3],
            ticks: 0,
            semitones: -40
        }))
    );
    // Both tracks are watched.
    let up = up_by(p, 100., v.row());
    v.down(p, NONE);
    v.drag(up, NONE);
    let lanes: Vec<_> = v.m.gesture().unwrap().lanes.iter().copied().collect();
    assert_eq!(lanes, [(3, 31), (3, 32)]);
    v.up(up, NONE);
    // E4 is not dragged (not selected); C4 (60) is the highest: +67.
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Nudge {
            events: vec![1, 3],
            ticks: 0,
            semitones: 67
        }))
    );
}

#[test]
fn a_zero_length_note_is_not_resized_without_horizontal_motion() {
    let f = roll_fake();
    let mut zero = note_row(1, 0, 60);
    zero.duration = 0;
    f.set_events(3, 31, vec![zero]);
    let mut v = Roll::on(f, EditorTarget::Lane { seq: 3, lane: 31 }, None);
    let r = RollGeometry::rect(&v.note(1), &v.frame.axis, &v.frame.pitch);
    let p = Point {
        x: r.max_x() - 0.5,
        y: r.y + r.height / 2.,
    };
    v.down(p, NONE);
    v.drag(
        Point {
            x: p.x,
            y: p.y + 2.,
        },
        NONE,
    );
    v.up(
        Point {
            x: p.x,
            y: p.y + 2.,
        },
        NONE,
    );
    assert!(
        v.f.gesture_edits().is_empty() && v.f.edits().is_empty(),
        "inside the dead zone: a click"
    );
    v.down(p, NONE);
    v.drag(
        Point {
            x: p.x,
            y: p.y + 6.,
        },
        NONE,
    );
    v.up(
        Point {
            x: p.x,
            y: p.y + 6.,
        },
        NONE,
    );
    assert!(
        v.f.gesture_edits().is_empty(),
        "no horizontal motion: no resize"
    );
    assert!(!v.m.gesture_open());
    assert_eq!(v.m.snapshot().undo_label, None);
}

#[test]
fn a_command_end_drag_sends_nothing() {
    // Scale Time is not in this slice (D5): no gesture, no edit.
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::End);
    v.down(p, COMMAND);
    v.drag(right_by(p, 100.), COMMAND);
    assert!(!v.m.gesture_open());
    v.up(right_by(p, 100.), COMMAND);
    assert!(v.begins().is_empty());
    assert!(v.f.gesture_edits().is_empty() && v.f.edits().is_empty());
    assert_eq!(v.log(), ["on 60", "off 60"]);
    // ⌘ elsewhere on a note is a plain drag.
    let s = v.point(1, NoteZone::Start);
    v.down(s, COMMAND);
    v.drag(right_by(s, 160.), COMMAND);
    v.up(right_by(s, 160.), COMMAND);
    assert_eq!(v.begins(), ["begin_edit(Move Notes)"]);
}

// MARK: Cancelling and refusals

#[test]
fn escape_cancels_the_drag_and_silences() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    v.drag(up_by(p, 3., v.row()), NONE);
    v.cancel();
    assert!(!v.m.gesture_open());
    assert_eq!(v.f.calls().last().unwrap(), "abort_edit");
    assert_eq!(v.log().last().unwrap(), "off 63");
    // The release that follows does nothing more.
    let calls = v.f.calls().len();
    v.up(up_by(p, 3., v.row()), NONE);
    assert_eq!(v.f.calls().len(), calls);
    assert_eq!(v.m.snapshot().undo_label, None);
}

#[test]
fn escape_drops_a_rubber_band() {
    let mut v = Roll::chords();
    v.down(Point { x: 600., y: 5. }, NONE);
    assert!(v.drag(Point { x: 700., y: 50. }, NONE).is_some());
    let o = v.m.cancel_roll_drag(&mut v.pointer, &v.frame);
    assert_eq!(o.band, None);
    assert!(v.pointer.drag.is_none());
    assert!(v.f.calls().is_empty());
}

#[test]
fn escape_aborts_a_final_edit_still_pending_on_busy() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    v.drag(up_by(p, 1., v.row()), NONE);
    v.f.fail_next("edit_gesture", busy());
    v.up(up_by(p, 2., v.row()), NONE);
    assert!(v.m.gesture().unwrap().finishing);
    v.cancel();
    assert!(!v.m.gesture_open());
    assert_eq!(v.f.calls().last().unwrap(), "abort_edit");
    assert!(!v.f.calls().iter().any(|c| c == "end_edit"));
}

#[test]
fn a_note_drag_ends_its_sound_even_when_the_final_edit_waits() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    v.drag(up_by(p, 1., v.row()), NONE);
    v.f.fail_next("edit_gesture", busy());
    v.up(up_by(p, 2., v.row()), NONE);
    assert!(v.m.gesture().unwrap().finishing);
    v.m.idle();
    assert!(!v.m.gesture_open());
    assert_eq!(
        v.f.gesture_edits().last(),
        Some(&ev(EventEdit::Nudge {
            events: vec![1],
            ticks: 0,
            semitones: 2
        })),
        "the final target went through"
    );
    assert_eq!(
        v.m.snapshot().undo_label.as_deref(),
        Some("Transpose Notes")
    );
    assert_eq!(v.log(), ["on 60", "off 60", "on 61", "off 61"]);
}

#[test]
fn leaving_the_window_cancels_the_drag_and_silences() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    v.drag(up_by(p, 3., v.row()), NONE);
    let o = v.m.roll_window_lost(&mut v.pointer, &v.frame);
    v.requests.extend(o.audition);
    assert!(!v.m.gesture_open());
    assert_eq!(v.f.calls().last().unwrap(), "abort_edit");
    assert_eq!(v.log(), ["on 60", "off 60", "on 63", "off 63"]);
}

#[test]
fn a_refused_final_cancels_the_drag_and_says_why() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    v.drag(up_by(p, 1., v.row()), NONE);
    v.f.fail_next(
        "edit_gesture",
        VizError::Model {
            message: "No room there.".into(),
        },
    );
    v.up(up_by(p, 2., v.row()), NONE);
    assert!(!v.m.gesture_open());
    assert_eq!(v.f.calls().last().unwrap(), "abort_edit");
    assert!(!v.f.calls().iter().any(|c| c == "end_edit"));
    assert!(v.m.status().is_some_and(|s| s.contains("No room there.")));
    assert_eq!(v.m.snapshot().undo_label, None);
}

#[test]
fn a_note_drag_while_a_gesture_finishes_says_so_and_silences() {
    let mut v = Roll::chords();
    assert!(v.m.begin_gesture("Move Notes", vec![(3, 31)]));
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    assert_eq!(v.log(), ["on 60"]);
    v.drag(up_by(p, 3., v.row()), NONE);
    assert_eq!(v.m.status(), Some(GESTURE_OPEN));
    assert_eq!(
        v.log(),
        ["on 60", "off 60"],
        "the pressed note stops when the drag is refused"
    );
    v.up(up_by(p, 3., v.row()), NONE);
    assert_eq!(
        v.m.gesture().unwrap().label,
        "Move Notes",
        "the other gesture is untouched"
    );
    assert!(v.f.gesture_edits().is_empty());
}

#[test]
fn a_refused_begin_is_the_status_and_silences() {
    let mut v = Roll::chords();
    v.f.fail_next(
        "begin_edit",
        VizError::Model {
            message: "Locked.".into(),
        },
    );
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    v.drag(up_by(p, 3., v.row()), NONE);
    assert!(
        v.m.status()
            .is_some_and(|s| s != GESTURE_OPEN && s.contains("Locked."))
    );
    assert_eq!(v.log(), ["on 60", "off 60"]);
    v.up(up_by(p, 3., v.row()), NONE);
    assert!(!v.m.gesture_open() && v.f.gesture_edits().is_empty());
}

#[test]
fn an_eraser_click_while_a_gesture_finishes_says_so() {
    let mut v = Roll::chords();
    assert!(v.m.begin_gesture("Move Notes", vec![(3, 31)]));
    v.m.set_tool(Tool::Eraser);
    v.click(v.point(1, NoteZone::Middle), NONE);
    assert_eq!(v.m.status(), Some(GESTURE_OPEN));
    assert!(v.f.edits().is_empty());
}

#[test]
fn a_drag_whose_track_vanishes_is_cancelled() {
    let mut v = Roll::chords();
    let p = v.point(1, NoteZone::Middle);
    v.down(p, NONE);
    v.drag(up_by(p, 1., v.row()), NONE);
    let mut s = v.f.snap();
    s.version += 1;
    s.lane_revs.retain(|r| r.lane != 31);
    v.f.set_idle(Some(s));
    v.m.idle();
    assert!(!v.m.gesture_open());
    assert_eq!(v.m.status(), Some(LANE_REMOVED));
    assert_eq!(v.f.calls().last().unwrap(), "abort_edit");
    v.up(up_by(p, 2., v.row()), NONE);
    assert!(!v.m.gesture_open());
    assert_eq!(v.f.calls().last().unwrap(), "abort_edit", "nothing after");
}

// MARK: Cursors

#[test]
fn the_cursor_follows_the_zone_and_is_an_arrow_over_the_ruler() {
    let v = Roll::chords();
    assert_eq!(
        v.m.roll_cursor(&v.frame, v.point(1, NoteZone::Start)),
        CursorKind::OpenHand
    );
    assert_eq!(
        v.m.roll_cursor(&v.frame, v.point(1, NoteZone::Middle)),
        CursorKind::UpDown
    );
    assert_eq!(
        v.m.roll_cursor(&v.frame, v.point(1, NoteZone::End)),
        CursorKind::ResizeRight
    );
    assert_eq!(
        v.m.roll_cursor(&v.frame, Point { x: 10., y: -5. }),
        CursorKind::Arrow
    );
}
