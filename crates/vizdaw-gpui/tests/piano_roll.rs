//! The Piano Roll in the bottom pane (Swift `PianoRollEditor`,
//! `PianoRollBar`, `PianoRollContainer`, `PianoRollView` and
//! `PianoKeyboardView`), read-only: its bar, the 64-point side column,
//! the notes in the tree, rows and zoom saved in the layout, centring and
//! a long track drawn a screen at a time, driven through GPUI's test
//! platform with the scripted session.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, ScrollDelta,
    ScrollEvent, TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::arrangement::{RULER_HEIGHT, TimeAxis};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Snap, Tool};
use vizdaw_app::piano_roll::{CursorKind, Pencil, PitchAxis};
use vizdaw_app::testing::{FakeSession, demo_fake, note_row, roll_fake};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::{EventRow, MeterEntryRow};
use vizdaw_ffi::selection::TickRange;
use vizdaw_gpui::menus::{self, ItemSpec};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow};

const BAR: i64 = 61_440;
const ROLL: &str = "Piano Roll";
const KEYBOARD: &str = "Keyboard";
const PANE: &str = "Bottom Pane";
const C4: &str = "Note C4, bar 1 beat 1, velocity 64";
const E4: &str = "Note E4, bar 1 beat 2, velocity 64";
const CHORDS: LaneRef = LaneRef { seq: 3, lane: 31 };
const BASS: LaneRef = LaneRef { seq: 3, lane: 32 };

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext, fake: FakeSession) -> Setup {
    let fake = Arc::new(fake);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let window = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, window)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    Setup {
        fake,
        app,
        window,
        cx: vcx,
    }
}

/// The roll fake with a Piano Roll on `target`.
fn roll_on(cx: &mut TestAppContext, fake: FakeSession, target: EditorTarget) -> Setup {
    let mut s = setup(cx, fake);
    // A pane tall enough for a couple of octaves.
    s.update_model(|d| d.update_layout(|l| l.views.current_mut().bottom.size = 360.));
    s.update_model(|d| d.open_detail(target, Some(DetailEditorKind::PianoRoll)));
    // The first frame measures the canvas; the next centres in it.
    s.cx.run_until_parked();
    s
}

/// Swift `rollView()`: the roll on Chords (C4 at 1.1.0, E4 at 1.2.0).
fn chords_roll(cx: &mut TestAppContext) -> Setup {
    roll_on(cx, roll_fake(), EditorTarget::Lane { seq: 3, lane: 31 })
}

/// ⌘ on macOS, ⌃ elsewhere, as `menus::secondary!`.
fn secondary(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

fn secondary_mods() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::command()
    } else {
        Modifiers::control()
    }
}

impl Setup {
    fn doc(&self) -> vizdaw_app::DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }

    fn model<R>(&mut self, f: impl FnOnce(&vizdaw_app::DocumentModel) -> R) -> R {
        let doc = self.doc();
        self.app
            .read_with(&self.cx, |a, _| f(a.store.document(doc).unwrap()))
    }

    fn update_model(&mut self, f: impl FnOnce(&mut vizdaw_app::DocumentModel)) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| f(d));
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    fn elements(&mut self) -> Vec<Element> {
        self.window
            .read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }

    fn find(&mut self, role: &str, label: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    fn element(&mut self, role: &str, label: &str) -> Element {
        let all = self.elements();
        self.find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {all:#?}"))
    }

    /// The elements inside the container `outer` (by path).
    fn inside(&mut self, outer: &Element) -> Vec<Element> {
        self.elements()
            .into_iter()
            .filter(|e| e.path.len() > outer.path.len() && e.path.starts_with(&outer.path))
            .collect()
    }

    /// The note buttons of the roll, in tree order.
    fn notes(&mut self) -> Vec<Element> {
        let roll = self.element("group", ROLL);
        self.inside(&roll)
            .into_iter()
            .filter(|e| e.role == "button")
            .collect()
    }

    fn note_labels(&mut self) -> Vec<String> {
        self.notes().into_iter().map(|e| e.label).collect()
    }

    fn canvas(&mut self) -> Bounds<Pixels> {
        self.element("group", ROLL).bounds
    }

    fn menu_items(&mut self) -> Vec<Element> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "menuitem")
            .collect()
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    fn click_at(&mut self, position: gpui::Point<Pixels>) {
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    /// The bottom pane's picker (not the transport bar's toggle, whose
    /// value is on or off).
    fn picker(&mut self) -> Element {
        self.elements()
            .into_iter()
            .find(|e| {
                e.role == "button"
                    && e.label == PANE
                    && !matches!(e.value.as_deref(), Some("on" | "off"))
            })
            .expect("the bottom pane's picker")
    }

    fn click_picker(&mut self) {
        let at = self.picker().bounds.center();
        self.click_at(at);
    }

    /// Clicks the `role` labelled `label` inside the roll's editor (the
    /// arrangement's tool bar has the same labels).
    fn click(&mut self, role: &str, label: &str) {
        let e = self.roll_control(role, label);
        self.click_at(e.bounds.center());
    }

    /// The `role` labelled `label` in the bottom pane (not the
    /// arrangement's tool bar).
    fn roll_control(&mut self, role: &str, label: &str) -> Element {
        let pane = self.element("group", PANE);
        let all = self.inside(&pane);
        all.iter()
            .find(|e| e.role == role && e.label == label)
            .cloned()
            .unwrap_or_else(|| panic!("no {role} {label:?} in the pane: {all:#?}"))
    }

    fn wheel(&mut self, at: gpui::Point<Pixels>, dx: f32, dy: f32, modifiers: Modifiers) {
        self.cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            modifiers,
            ..Default::default()
        });
        self.cx.run_until_parked();
    }

    fn axis(&mut self) -> TimeAxis {
        self.window
            .read_with(&self.cx, |v, _| *v.piano_roll().axis())
            .unwrap()
    }

    fn pitch(&mut self) -> PitchAxis {
        self.window
            .read_with(&self.cx, |v, _| *v.piano_roll().pitch())
            .unwrap()
    }

    fn notes_drawn(&mut self) -> usize {
        self.window
            .read_with(&self.cx, |v, _| v.piano_roll().notes_drawn())
            .unwrap()
    }

    /// The notes' area height (the canvas under its ruler).
    fn body_height(&mut self) -> f32 {
        f32::from(self.canvas().size.height) - RULER_HEIGHT
    }

    /// The pitch at the middle of the notes' area.
    fn middle_pitch(&mut self) -> u8 {
        let h = self.body_height();
        self.pitch().pitch_at(h / 2.)
    }

    fn menu_item(&mut self, menu: &str, title: &str) -> (bool, Option<bool>) {
        let keymap = self.app.read_with(&self.cx, |a, _| a.keys.keymap().clone());
        let model = self.model(|d| menus::menu_model(Some(d), &keymap));
        model
            .menus
            .iter()
            .find(|m| m.title == menu)
            .unwrap_or_else(|| panic!("no menu {menu}"))
            .items
            .iter()
            .find_map(|i| match i {
                ItemSpec::Command {
                    title: t,
                    enabled,
                    checked,
                    ..
                } if t == title => Some((*enabled, *checked)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no item {title} in {menu}"))
    }

    /// Saves to a temporary file and answers the `ui.json` written.
    fn saved_layout(&mut self, name: &str) -> serde_json::Value {
        let path = std::env::temp_dir().join(name);
        let doc = self.doc();
        self.app
            .update(&mut self.cx, |a, _| a.save_as(doc, &path))
            .expect("saved");
        serde_json::from_str(&self.fake.layout().expect("a stored layout")).unwrap()
    }
}

#[gpui::test]
fn editors_piano_roll_opens_the_roll(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    assert!(s.find("group", ROLL).is_none());
    assert_eq!(s.menu_item("Editors", "Piano Roll"), (true, None));

    s.keys(&secondary("1"));
    let detail = s.model(|d| d.layout().detail.clone()).expect("a detail");
    assert_eq!(detail.kind, DetailEditorKind::PianoRoll);
    assert_eq!(s.model(|d| d.detail_kind_name()), "piano_roll");
    assert!(s.find("group", ROLL).is_some(), "the roll is drawn");
    assert!(s.find("group", KEYBOARD).is_some(), "with its keyboard");
    assert!(s.find("table", "Event List").is_none());
    assert_eq!(s.picker().value.as_deref(), Some(ROLL));

    // The bar, left to right, in the pane (not the arrangement's).
    let pane = s.element("group", PANE);
    let bar: Vec<String> = s
        .inside(&pane)
        .into_iter()
        .filter(|e| e.role == "button" && !e.label.starts_with("Note "))
        .map(|e| e.label)
        .collect();
    assert_eq!(
        bar,
        [
            PANE,
            "Close Detail Editor",
            "Arrow",
            "Pencil",
            "Eraser",
            "Keyboard",
            "Tracks",
            "Insert Into: Chords",
            "Exact",
            "Shorter Rows",
            "Taller Rows",
            "Zoom Out",
            "Zoom In",
        ]
    );
    assert_eq!(
        s.roll_control("button", "Keyboard").value.as_deref(),
        Some("on")
    );
    // The tool is the arrangement's: choosing it in the roll changes both.
    assert!(s.roll_control("button", "Arrow").selected);
    s.click("button", "Pencil");
    assert_eq!(s.model(|d| d.tool()), Tool::Pencil);
    assert!(s.roll_control("button", "Pencil").selected);

    // The pane's picker lists the Piano Roll, then the List, then the
    // pane's other views (slice 6a).
    s.click_picker();
    let items: Vec<String> = s.menu_items().into_iter().map(|e| e.label).collect();
    assert_eq!(items, [ROLL, "List", "Audio Editor", "Mixer"]);
    assert!(s.element("menuitem", ROLL).selected);
    let list = s.element("menuitem", "List");
    s.click_at(list.bounds.center());
    assert_eq!(s.model(|d| d.detail_kind_name()), "list");
    assert!(s.find("group", ROLL).is_none(), "only one editor shows");
    assert!(s.find("table", "Event List").is_some());

    // ⌘1 again: back to the roll on the same target.
    s.keys(&secondary("1"));
    assert_eq!(s.model(|d| d.detail_kind_name()), "piano_roll");
    assert_eq!(
        s.model(|d| d.layout().detail.as_ref().map(|d| d.target)),
        Some(detail.target)
    );
}

#[gpui::test]
fn the_roll_lists_its_notes_in_the_tree(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    assert_eq!(s.note_labels(), [C4, E4]);
    let notes = s.notes();
    assert!(notes.iter().all(|n| !n.selected));
    // A note's frame is its rectangle: 96 ticks a point, one row high.
    let canvas = s.canvas();
    let axis = s.axis();
    let pitch = s.pitch();
    let c4 = &notes[0];
    assert_eq!(c4.bounds.size.width, px(15_360. / 96.));
    assert_eq!(c4.bounds.size.height, px(pitch.row_height()));
    assert_eq!(c4.bounds.origin.x, canvas.origin.x + px(axis.x(0)));
    assert_eq!(
        c4.bounds.origin.y,
        canvas.origin.y + px(RULER_HEIGHT + pitch.top(60))
    );

    s.update_model(|d| d.roll_select(&[(CHORDS, 2)], false));
    let selected: Vec<(String, bool)> = s
        .notes()
        .into_iter()
        .map(|e| (e.label, e.selected))
        .collect();
    assert_eq!(selected, [(C4.into(), false), (E4.into(), true)]);

    // With more than one track shown, a note names its track; the shown
    // tracks are listed in sequence order (rows short enough to show
    // E2…E4).
    s.update_model(|d| d.update_layout(|l| l.piano_roll.row_height = 6.));
    s.update_model(|d| d.open_detail(EditorTarget::Sequence { seq: 3 }, None));
    s.cx.run_until_parked();
    let labels = s.note_labels();
    assert_eq!(
        labels,
        [
            format!("{C4}, Chords"),
            format!("{E4}, Chords"),
            "Note E2, bar 1 beat 1, velocity 64, Bass".into(),
        ]
    );
}

#[gpui::test]
fn the_keyboard_toggle_keeps_the_column(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    let keyboard = s.element("group", KEYBOARD).bounds;
    let canvas = s.canvas();
    assert_eq!(keyboard.size.width, px(64.));
    assert_eq!(canvas.origin.x, keyboard.origin.x + px(64.));
    assert_eq!(keyboard.size.height, canvas.size.height);

    s.click("button", "Keyboard");
    assert!(!s.model(|d| d.layout().piano_roll.show_keyboard));
    assert!(s.find("group", KEYBOARD).is_none(), "the keyboard hides");
    assert_eq!(
        s.roll_control("button", "Keyboard").value.as_deref(),
        Some("off")
    );
    assert_eq!(s.canvas(), canvas, "a blank 64-point column stays");
    assert_eq!(s.note_labels(), [C4, E4]);

    s.click("button", "Keyboard");
    assert!(s.model(|d| d.layout().piano_roll.show_keyboard));
    assert_eq!(s.element("group", KEYBOARD).bounds, keyboard);
}

#[gpui::test]
fn rows_and_zoom_change_and_persist(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    assert_eq!(s.axis().ticks_per_point(), 96.);
    assert_eq!(s.pitch().row_height(), 10.);
    let middle = s.middle_pitch();

    s.click("button", "Taller Rows");
    assert_eq!(s.model(|d| d.layout().piano_roll.row_height), 12.);
    assert_eq!(s.pitch().row_height(), 12.);
    assert_eq!(s.middle_pitch(), middle, "the middle pitch stays");
    assert_eq!(s.notes()[0].bounds.size.height, px(12.));
    s.click("button", "Shorter Rows");
    s.click("button", "Shorter Rows");
    assert_eq!(s.pitch().row_height(), 8.);
    s.update_model(|d| d.update_layout(|l| l.piano_roll.row_height = 40.));
    assert_eq!(s.pitch().row_height(), 40.);
    s.click("button", "Taller Rows");
    assert_eq!(
        s.model(|d| d.layout().piano_roll.row_height),
        40.,
        "at most 40"
    );
    s.update_model(|d| d.update_layout(|l| l.piano_roll.row_height = 10.));

    // The zoom buttons: ×1.5 out, ÷1.5 in, the origin kept.
    let origin = s.axis().origin();
    s.click("button", "Zoom Out");
    assert_eq!(s.model(|d| d.layout().piano_roll.ticks_per_point), 144.);
    assert_eq!(s.axis().ticks_per_point(), 144.);
    assert_eq!(s.axis().origin(), origin);
    assert_eq!(s.notes()[0].bounds.size.width, px(15_360. / 144.));
    s.click("button", "Zoom In");
    assert_eq!(s.axis().ticks_per_point(), 96.);
    // The arrangement's zoom is its own.
    assert_eq!(
        s.window
            .read_with(&s.cx, |v, _| v.arrangement().axis().ticks_per_point())
            .unwrap(),
        TimeAxis::DEFAULT_TICKS_PER_POINT
    );

    // ⌘-wheel zooms about the pointer, and the layout follows.
    let canvas = s.canvas();
    let at = point(
        canvas.origin.x + px(100.),
        canvas.origin.y + px(RULER_HEIGHT + 10.),
    );
    let under = s.axis().tick_at(100.);
    s.wheel(at, 0., 10., secondary_mods());
    let zoomed = s.axis();
    assert!(zoomed.ticks_per_point() < 96.);
    assert!((zoomed.tick_at(100.) - under).abs() <= 1, "zoomed about x");
    assert_eq!(
        s.model(|d| d.layout().piano_roll.ticks_per_point),
        zoomed.ticks_per_point()
    );

    // A plain wheel scrolls (up shows lower pitches); nothing is saved.
    let offset = s.pitch().offset();
    s.wheel(at, -30., -20., Modifiers::none());
    assert_eq!(s.pitch().offset(), offset + 20.);
    assert!((s.axis().origin() - zoomed.tick_at(30.)).abs() <= 1);

    let saved = s.saved_layout("vizdaw-roll-zoom.vizdaw");
    let roll = &saved["pianoRoll"];
    assert_eq!(roll["rowHeight"], 10.);
    assert_eq!(roll["ticksPerPoint"], zoomed.ticks_per_point());
    let keys: Vec<&str> = roll
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    assert!(
        keys.iter()
            .all(|k| !k.contains("origin") && !k.contains("offset")),
        "no scroll is saved: {keys:?}"
    );
}

#[gpui::test]
fn the_roll_centres_on_its_notes(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    // C4 and E4: the middle of 60…64 is 62.
    let h = s.body_height();
    assert_eq!(s.pitch(), s.pitch().centering(62, h));
    assert_eq!(s.middle_pitch(), 62);
    assert_eq!(s.axis().origin(), 0, "a track target keeps the time");

    // Another target centres again: Bass's note is E2 (40).
    s.update_model(|d| d.open_detail(EditorTarget::Lane { seq: 3, lane: 32 }, None));
    s.cx.run_until_parked();
    assert_eq!(s.middle_pitch(), 40);

    // A block target also scrolls to 16 points before its start.
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Block {
                seq: 3,
                lane: 31,
                start: 2 * BAR,
                end: 3 * BAR,
            },
            None,
        )
    });
    s.cx.run_until_parked();
    assert_eq!(s.axis().origin(), 2 * BAR - 16 * 96);
    assert_eq!(s.middle_pitch(), 62);

    // Scrolled away, the same target stays where it was put.
    let canvas = s.canvas();
    let at = point(canvas.origin.x + px(50.), canvas.origin.y + px(40.));
    s.wheel(at, 0., 30., Modifiers::none());
    let offset = s.pitch().offset();
    s.update_model(|_| {});
    assert_eq!(s.pitch().offset(), offset);
}

#[gpui::test]
fn an_empty_roll_centres_middle_c(cx: &mut TestAppContext) {
    let fake = roll_fake();
    fake.set_events(3, 31, vec![]);
    let mut s = roll_on(cx, fake, EditorTarget::Lane { seq: 3, lane: 31 });
    assert_eq!(s.middle_pitch(), 60);
    assert!(s.notes().is_empty());
}

#[gpui::test]
fn the_pencil_reveal_scrolls_to_its_bar(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    s.update_model(|d| {
        assert!(d.pencil_opens_piano_roll(3, 31, 3 * BAR + 100));
    });
    s.cx.run_until_parked();
    assert_eq!(s.model(|d| d.detail_kind_name()), "piano_roll");
    assert_eq!(s.axis().origin(), 3 * BAR - 4 * 96);
    // Scrolled away, the same reveal is not applied again; a new one is.
    let canvas = s.canvas();
    let at = point(canvas.origin.x + px(50.), canvas.origin.y + px(40.));
    s.wheel(at, -200., 0., Modifiers::none());
    let scrolled = s.axis().origin();
    assert_ne!(scrolled, 3 * BAR - 4 * 96);
    s.update_model(|_| {});
    assert_eq!(s.axis().origin(), scrolled);
    s.update_model(|d| {
        assert!(d.pencil_opens_piano_roll(3, 31, 3 * BAR));
    });
    assert_eq!(s.axis().origin(), 3 * BAR - 4 * 96);
}

#[gpui::test]
fn a_long_track_draws_only_the_notes_in_view(cx: &mut TestAppContext) {
    const COUNT: u64 = 20_000;
    let fake = roll_fake();
    let notes: Vec<EventRow> = (0..COUNT)
        .map(|i| note_row(i + 1, i as i64 * 3_840, 60 + (i % 5) as u8))
        .collect();
    fake.set_events(3, 31, notes);
    let mut s = roll_on(cx, fake, EditorTarget::Lane { seq: 3, lane: 31 });
    let drawn = s.notes_drawn();
    assert!(drawn > 0, "the notes in view are drawn");
    assert!(drawn < 200, "only those in view: {drawn}");
    let labels = s.note_labels();
    assert_eq!(labels.len(), drawn, "and only they are in the tree");
    assert_eq!(labels[0], C4);

    // Scrolled far right, other notes; still only a screen of them.
    let canvas = s.canvas();
    let at = point(canvas.origin.x + px(50.), canvas.origin.y + px(40.));
    s.wheel(at, -400_000., 0., Modifiers::none());
    let drawn = s.notes_drawn();
    assert!(drawn > 0 && drawn < 200, "{drawn}");
    assert!(!s.note_labels().iter().any(|l| l == C4));
    assert_eq!(
        s.model(|d| d.piano_roll().builds()),
        1,
        "the notes are built once"
    );
}

#[gpui::test]
fn tracks_and_insert_into_menus_choose_lanes(cx: &mut TestAppContext) {
    let mut s = roll_on(cx, roll_fake(), EditorTarget::Sequence { seq: 3 });
    assert_eq!(s.model(|d| d.roll_shown_lanes()), [CHORDS, BASS]);

    // Tracks: every track of the sequence, on when shown.
    s.click("button", "Tracks");
    let items: Vec<(String, Option<String>, bool)> = s
        .menu_items()
        .into_iter()
        .map(|e| (e.label, e.value, e.enabled))
        .collect();
    assert_eq!(
        items,
        [
            ("Groove".into(), Some("off".into()), true),
            ("Chords".into(), Some("on".into()), true),
            ("Bass".into(), Some("on".into()), true),
        ]
    );
    let groove = s.element("menuitem", "Groove");
    s.click_at(groove.bounds.center());
    assert!(s.menu_items().is_empty(), "the menu closes");
    assert_eq!(
        s.model(|d| d.layout().detail.as_ref().unwrap().lanes()),
        Some(vec![30, 31, 32])
    );
    s.click("button", "Tracks");
    let bass = s.element("menuitem", "Bass");
    s.click_at(bass.bounds.center());
    assert_eq!(
        s.model(|d| d.roll_shown_lanes()),
        [LaneRef { seq: 3, lane: 30 }, CHORDS]
    );

    // Insert Into: the shown tracks; the choice names the button.
    assert!(s.find("button", "Insert Into: Groove").is_some());
    s.click("button", "Insert Into: Groove");
    let items: Vec<String> = s.menu_items().into_iter().map(|e| e.label).collect();
    assert_eq!(items, ["Groove", "Chords"]);
    let chords = s.element("menuitem", "Chords");
    s.click_at(chords.bounds.center());
    assert_eq!(s.model(|d| d.roll_edit_lane()), Some(CHORDS));
    assert!(s.find("button", "Insert Into: Chords").is_some());

    // A track target's own track cannot be hidden.
    s.update_model(|d| d.open_detail(EditorTarget::Lane { seq: 3, lane: 32 }, None));
    s.click("button", "Tracks");
    assert!(!s.element("menuitem", "Bass").enabled);
    assert_eq!(s.element("menuitem", "Bass").value.as_deref(), Some("on"));
    let bass = s.element("menuitem", "Bass");
    s.click_at(bass.bounds.center());
    assert_eq!(s.model(|d| d.roll_shown_lanes()), [BASS]);
    s.keys("escape");
    assert!(s.menu_items().is_empty(), "Escape closes the menu");
}

// MARK: Editing and audition (Swift `PianoRollViewTests`)

const VELOCITY_64: u16 = 64 << 9;

fn ev(edit: EventEdit) -> EditCommand {
    EditCommand::Event { edit }
}

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

impl Setup {
    /// Window point of canvas x `x` (points from its left edge) on the
    /// middle of `pitch`'s row.
    fn at(&mut self, x: f32, pitch: u8) -> gpui::Point<Pixels> {
        let canvas = self.canvas();
        let p = self.pitch();
        point(
            canvas.origin.x + px(x),
            canvas.origin.y + px(RULER_HEIGHT + p.top(pitch) + p.row_height() / 2.),
        )
    }

    /// Window point of canvas x `x` at `dy` points from the middle of
    /// `pitch`'s row.
    fn at_dy(&mut self, x: f32, pitch: u8, dy: f32) -> gpui::Point<Pixels> {
        let p = self.at(x, pitch);
        point(p.x, p.y + px(dy))
    }

    fn down(&mut self, at: gpui::Point<Pixels>, modifiers: Modifiers) {
        self.cx
            .simulate_mouse_down(at, MouseButton::Left, modifiers);
        self.cx.run_until_parked();
    }

    fn move_to(&mut self, at: gpui::Point<Pixels>, modifiers: Modifiers) {
        self.cx
            .simulate_mouse_move(at, Some(MouseButton::Left), modifiers);
        self.cx.run_until_parked();
    }

    fn hover(&mut self, at: gpui::Point<Pixels>) {
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn up(&mut self, at: gpui::Point<Pixels>, modifiers: Modifiers) {
        self.cx.simulate_mouse_up(at, MouseButton::Left, modifiers);
        self.cx.run_until_parked();
    }

    fn click_with(&mut self, at: gpui::Point<Pixels>, modifiers: Modifiers) {
        self.down(at, modifiers);
        self.up(at, modifiers);
    }

    fn drag(&mut self, from: gpui::Point<Pixels>, to: gpui::Point<Pixels>, m: Modifiers) {
        self.down(from, m);
        self.move_to(to, m);
        self.up(to, m);
    }

    fn selected(&mut self) -> Vec<u64> {
        self.model(|d| d.selection().events.clone())
    }

    fn begins(&self) -> Vec<String> {
        self.fake
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("begin_edit("))
            .collect()
    }

    fn undo_label(&mut self) -> Option<String> {
        self.model(|d| d.snapshot().undo_label.clone())
    }

    /// The audition log as "on 60" / "off 60".
    fn sounds(&self) -> Vec<String> {
        self.fake
            .auditions()
            .into_iter()
            .map(|(on, pitch, _, _)| format!("{on} {pitch}"))
            .collect()
    }

    fn cursor(&mut self) -> CursorKind {
        self.window
            .read_with(&self.cx, |v, _| v.piano_roll().cursor())
            .unwrap()
    }

    fn band(&mut self) -> Option<vizdaw_app::arrangement::Rect> {
        self.window
            .read_with(&self.cx, |v, _| v.piano_roll().band())
            .unwrap()
    }

    fn snap_and_meter(&mut self) -> (Snap, Vec<MeterEntryRow>) {
        self.model(|d| {
            (
                d.layout().snap,
                d.sequence_info(3)
                    .map(|i| i.meter.clone())
                    .unwrap_or_default(),
            )
        })
    }

    /// The keyboard strip's point at `pitch` (or `y` points from its top).
    fn key(&mut self, pitch: u8) -> gpui::Point<Pixels> {
        let k = self.element("group", KEYBOARD).bounds;
        let p = self.pitch();
        point(
            k.origin.x + px(10.),
            k.origin.y + px(RULER_HEIGHT + p.top(pitch) + p.row_height() / 2.),
        )
    }
}

/// Swift `testClickSelectsAndShiftToggles`: a click selects the note
/// alone (with its track) and sounds it while pressed; ⇧-click toggles;
/// a click on empty space clears. The pointer's shape follows the zone.
#[gpui::test]
fn a_note_click_selects_and_shift_toggles(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    s.fake.clear_calls();
    let c4 = s.at(80., 60);
    s.down(c4, Modifiers::none());
    assert_eq!(s.sounds(), ["on 60"], "the note sounds while pressed");
    assert_eq!(
        s.fake.auditions()[0],
        ("on".into(), 60, 5, VELOCITY_64),
        "at its velocity, through its track's instrument"
    );
    s.up(c4, Modifiers::none());
    assert_eq!(s.sounds(), ["on 60", "off 60"]);
    assert_eq!(s.selected(), [1]);
    assert_eq!(s.model(|d| d.selection().lanes.clone()), [CHORDS]);
    assert!(s.notes()[0].selected && !s.notes()[1].selected);

    let e4 = s.at(240., 64);
    s.click_with(e4, Modifiers::shift());
    assert_eq!(s.selected(), [1, 2]);
    s.click_with(c4, Modifiers::shift());
    assert_eq!(s.selected(), [2]);
    let empty = s.at(600., 70);
    s.click_with(empty, Modifiers::shift());
    assert_eq!(s.selected(), [2], "⇧ on empty space keeps the selection");
    s.click_with(empty, Modifiers::none());
    assert!(s.selected().is_empty());
    assert!(s.fake.edits().is_empty() && s.begins().is_empty());

    // Cursors: start, middle and end zones, empty space, the ruler.
    let p = s.at(162., 64);
    s.hover(p);
    assert_eq!(s.cursor(), CursorKind::OpenHand);
    let p = s.at(240., 64);
    s.hover(p);
    assert_eq!(s.cursor(), CursorKind::UpDown);
    let p = s.at(318., 64);
    s.hover(p);
    assert_eq!(s.cursor(), CursorKind::ResizeRight);
    let p = s.at(600., 70);
    s.hover(p);
    assert_eq!(s.cursor(), CursorKind::Arrow);
    s.update_model(|d| d.set_tool(Tool::Pencil));
    let p = s.at(601., 70);
    s.hover(p);
    assert_eq!(s.cursor(), CursorKind::Crosshair);
    let canvas = s.canvas();
    s.hover(point(canvas.origin.x + px(600.), canvas.origin.y + px(10.)));
    assert_eq!(s.cursor(), CursorKind::Arrow, "over the ruler");
    s.update_model(|d| d.set_tool(Tool::Eraser));
    let p = s.at(240., 64);
    s.hover(p);
    assert_eq!(s.cursor(), CursorKind::Eraser);
}

/// Swift `testRubberBandSelects`: a band from empty space selects the
/// notes it touches (⇧ adds); an empty band clears, with ⇧ it keeps.
#[gpui::test]
fn a_rubber_band_selects(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    let from = s.at(400., 68);
    let to = s.at(60., 59);
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    let band = s.band().expect("the band is drawn");
    assert!(band.width > 300. && band.height > 0., "{band:?}");
    s.up(to, Modifiers::none());
    assert_eq!(s.band(), None);
    assert_eq!(s.selected(), [1, 2]);
    assert!(s.sounds().is_empty(), "a band is silent");
    assert!(s.begins().is_empty(), "and no undo step");

    // An empty band: ⇧ keeps the selection, a plain one clears it.
    let (a, b) = (s.at(600., 70), s.at(700., 67));
    s.drag(a, b, Modifiers::shift());
    assert_eq!(s.selected(), [1, 2]);
    s.drag(a, b, Modifiers::none());
    assert!(s.selected().is_empty());
    // ⇧ adds to the selection.
    let p = s.at(80., 60);
    s.click_with(p, Modifiers::none());
    let (a, b) = (s.at(400., 66), s.at(200., 63));
    s.drag(a, b, Modifiers::shift());
    assert_eq!(s.selected(), [1, 2]);
}

/// Swift `testPencilClickInsertsAGridNote` and the pencil drag: one
/// "Insert Note" step each, in the edit track; the new pitch sounds.
#[gpui::test]
fn the_pencil_draws_a_note(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    s.update_model(|d| d.set_tool(Tool::Pencil));
    s.fake.clear_calls();
    // x 400 is tick 38 400: the note starts on the quarter at 30 720.
    let (snap, meter) = s.snap_and_meter();
    let at = s.at(400., 67);
    s.click_with(at, Modifiers::none());
    assert_eq!(s.begins(), ["begin_edit(Insert Note)"]);
    let start = Pencil::start(38_400, &snap, &meter);
    let length = Pencil::step(start, &snap, &meter);
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&Pencil::command(CHORDS, start, 67, length))
    );
    assert_eq!(s.undo_label().as_deref(), Some("Insert Note"));
    assert_eq!(s.sounds(), ["on 67", "off 67"]);

    // A drag paints the length.
    s.fake.clear_calls();
    let (from, to) = (s.at(500., 69), s.at(700., 69));
    s.drag(from, to, Modifiers::none());
    assert_eq!(s.begins(), ["begin_edit(Insert Note)"]);
    let start = Pencil::start(48_000, &snap, &meter);
    let length = Pencil::length(start, 67_200, &snap, &meter);
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&Pencil::command(CHORDS, start, 69, length))
    );
    assert_eq!(s.undo_label().as_deref(), Some("Insert Note"));
    assert!(!s.model(|d| d.gesture_open()));
}

/// Swift `testEraserSwipes`: the eraser deletes every note it lands on
/// or crosses, as one "Erase Notes" step; a click deletes one note; it
/// never sounds.
#[gpui::test]
fn the_eraser_swipes(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    s.update_model(|d| d.set_tool(Tool::Eraser));
    s.fake.clear_calls();
    let from = s.at(80., 70);
    s.down(from, Modifiers::none());
    let p = s.at(80., 60);
    s.move_to(p, Modifiers::none());
    let p = s.at(240., 64);
    s.move_to(p, Modifiers::none());
    let end = s.at(400., 64);
    s.up(end, Modifiers::none());
    assert_eq!(s.begins(), ["begin_edit(Erase Notes)"]);
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&ev(EventEdit::Delete { events: vec![1, 2] }))
    );
    assert_eq!(s.undo_label().as_deref(), Some("Erase Notes"));
    assert!(s.sounds().is_empty(), "the eraser is silent");

    // A click deletes the note under it.
    let p = s.at(240., 64);
    s.click_with(p, Modifiers::none());
    assert_eq!(
        s.fake.edits().last(),
        Some(&ev(EventEdit::Delete { events: vec![2] }))
    );
    assert!(s.sounds().is_empty());
}

/// Swift's note drags: the start zone moves (⌥ copies), the middle
/// transposes (and sounds the new pitch), the end resizes; one undo step
/// each, named for what it did.
#[gpui::test]
fn dragging_moves_copies_transposes_and_resizes(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    let (snap, meter) = s.snap_and_meter();
    let rh = s.pitch().row_height();

    s.fake.clear_calls();
    let (from, to) = (s.at(162., 64), s.at(322., 64));
    s.drag(from, to, Modifiers::none());
    assert_eq!(s.begins(), ["begin_edit(Move Notes)"]);
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&ev(EventEdit::Move {
            events: vec![2],
            delta: 15_360,
            to_lane: None
        }))
    );
    assert_eq!(s.undo_label().as_deref(), Some("Move Notes"));
    assert_eq!(s.selected(), [2], "the pressed note is selected first");

    s.fake.clear_calls();
    s.drag(from, to, Modifiers::alt());
    assert_eq!(s.begins(), ["begin_edit(Copy Notes)"]);
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&ev(EventEdit::Duplicate {
            events: vec![2],
            ticks: 15_360,
            semitones: 0
        }))
    );
    assert_eq!(s.undo_label().as_deref(), Some("Copy Notes"));

    s.fake.clear_calls();
    let heard = s.sounds().len();
    let from = s.at(240., 64);
    let to = s.at_dy(240., 64, -2. * rh);
    s.drag(from, to, Modifiers::none());
    assert_eq!(s.begins(), ["begin_edit(Transpose Notes)"]);
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&ev(EventEdit::Nudge {
            events: vec![2],
            ticks: 0,
            semitones: 2
        }))
    );
    assert_eq!(s.undo_label().as_deref(), Some("Transpose Notes"));
    assert_eq!(
        s.sounds()[heard..],
        ["on 64", "off 64", "on 66", "off 66"],
        "the new pitch sounds as it moves"
    );

    s.fake.clear_calls();
    let (from, to) = (s.at(318., 64), s.at(398., 64));
    s.drag(from, to, Modifiers::none());
    assert_eq!(s.begins(), ["begin_edit(Resize Notes)"]);
    let end = 30_720;
    let delta = snap.apply(end + 80 * 96, &meter).max(15_361) - end;
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&ev(EventEdit::Resize {
            events: vec![2],
            delta
        }))
    );
    assert_eq!(s.undo_label().as_deref(), Some("Resize Notes"));
}

/// Escape mid-drag: no undo step, nothing changed, the sound stops; the
/// release then does nothing.
#[gpui::test]
fn escape_mid_drag_changes_nothing_and_silences(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    let undo = s.undo_label();
    s.fake.clear_calls();
    let (from, to) = (s.at(162., 64), s.at(322., 64));
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    assert!(s.model(|d| d.gesture_open()));
    assert_eq!(s.sounds(), ["on 64"]);
    s.keys("escape");
    assert!(!s.model(|d| d.gesture_open()));
    assert_eq!(s.sounds(), ["on 64", "off 64"]);
    assert!(s.fake.calls().iter().any(|c| c == "abort_edit"));
    assert!(!s.fake.edit_is_open());
    s.up(to, Modifiers::none());
    assert!(!s.fake.calls().iter().any(|c| c == "end_edit"));
    assert_eq!(s.undo_label(), undo);
    assert!(s.fake.edits().is_empty());

    // A band is dropped too.
    let (a, b) = (s.at(600., 70), s.at(700., 67));
    s.down(a, Modifiers::none());
    s.move_to(b, Modifiers::none());
    assert!(s.band().is_some());
    s.keys("escape");
    assert_eq!(s.band(), None);
    s.up(b, Modifiers::none());
    assert_eq!(s.selected(), [2], "the band selected nothing");
}

/// Delete and Forward Delete, while the roll has the keyboard, delete
/// the selected notes as one "Delete Events" step (not the key bindings'
/// step entry); a hidden pane gives the keyboard back first.
#[gpui::test]
fn delete_deletes_the_selected_notes(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    let p = s.at(240., 64);
    s.click_with(p, Modifiers::none());
    assert!(s.model(|d| d.roll_focused()), "the canvas has the keyboard");
    s.fake.clear_calls();
    s.keys("backspace");
    assert_eq!(s.fake.edits(), [ev(EventEdit::Delete { events: vec![2] })]);
    let p = s.at(80., 60);
    s.click_with(p, Modifiers::none());
    s.keys("delete");
    assert_eq!(
        s.fake.edits().last(),
        Some(&ev(EventEdit::Delete { events: vec![1] }))
    );
    assert_eq!(s.fake.edits().len(), 2);

    // The keyboard elsewhere: the roll's Delete is not its own.
    let arrangement = s.element("group", "Arrangement").bounds;
    s.click_at(arrangement.center());
    assert!(s.model(|d| !d.roll_focused()));

    // Hidden, the roll keeps no keyboard.
    let p = s.at(80., 60);
    s.click_with(p, Modifiers::none());
    assert!(s.model(|d| d.roll_focused()));
    s.update_model(|d| d.update_layout(|l| l.views.arrange.bottom.shown = false));
    assert!(s.model(|d| !d.roll_focused()));
    s.keys("backspace");
    s.keys("delete");
    assert_eq!(s.fake.edits().len(), 2, "{:?}", s.fake.edits());

    // Shown again in a window that is not the active one (a channel
    // click on a background app): the click still gives the keyboard.
    s.update_model(|d| d.update_layout(|l| l.views.arrange.bottom.shown = true));
    s.cx.deactivate_window();
    let p = s.at(240., 64);
    s.click_with(p, Modifiers::none());
    assert!(s.model(|d| d.roll_focused()));
    s.keys("backspace");
    assert_eq!(s.fake.edits().len(), 3, "{:?}", s.fake.edits());
}

/// The keyboard strip: a press sounds its key through the edit track's
/// instrument, a drag plays each key crossed, the release silences; the
/// strip's top (the ruler's height) is not a key. A note-off that meets
/// Busy is retried at the fast poll.
#[gpui::test]
fn a_key_press_sounds_and_release_silences(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    s.fake.clear_calls();
    let k62 = s.key(62);
    s.down(k62, Modifiers::none());
    assert_eq!(s.fake.auditions(), [("on".into(), 62, 5, Pencil::VELOCITY)]);
    // Within the key: nothing new; onto the next: it sounds instead.
    let inside = point(k62.x + px(20.), k62.y + px(1.));
    s.move_to(inside, Modifiers::none());
    assert_eq!(s.sounds(), ["on 62"]);
    let k64 = s.key(64);
    s.move_to(k64, Modifiers::none());
    assert_eq!(s.sounds(), ["on 62", "off 62", "on 64"]);
    s.up(k64, Modifiers::none());
    assert_eq!(s.sounds(), ["on 62", "off 62", "on 64", "off 64"]);

    // The top strip is not a key.
    let top = s.element("group", KEYBOARD).bounds.origin;
    s.click_with(point(top.x + px(10.), top.y + px(10.)), Modifiers::none());
    assert_eq!(s.sounds().len(), 4);

    // A note-off refused as Busy is retried soon (the poll runs fast).
    s.down(k62, Modifiers::none());
    s.fake.fail_next("audition", busy());
    s.up(k62, Modifiers::none());
    assert_eq!(s.sounds().last().map(String::as_str), Some("on 62"));
    assert!(s.model(|d| d.wants_fast_poll()));
    s.cx.executor().advance_clock(Duration::from_millis(40));
    s.cx.run_until_parked();
    assert_eq!(s.sounds().last().map(String::as_str), Some("off 62"));
    assert!(s.model(|d| !d.wants_fast_poll()));
}

/// The window losing the keyboard: a note or key held stops sounding and
/// a drag in progress is dropped without an undo step.
#[gpui::test]
fn window_loss_silences(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    let undo = s.undo_label();
    s.fake.clear_calls();
    let (from, to) = (s.at(162., 64), s.at(322., 64));
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    assert!(s.model(|d| d.gesture_open()));
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert_eq!(s.sounds(), ["on 64", "off 64"]);
    assert!(!s.model(|d| d.gesture_open()));
    s.up(to, Modifiers::none());
    assert_eq!(s.undo_label(), undo);
    assert!(!s.fake.calls().iter().any(|c| c == "end_edit"));

    // A key held on the keyboard strip.
    s.cx.update(|window, _| window.activate_window());
    s.cx.run_until_parked();
    let heard = s.sounds().len();
    let k = s.key(62);
    s.down(k, Modifiers::none());
    assert_eq!(s.sounds()[heard..], ["on 62"]);
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert_eq!(s.sounds()[heard..], ["on 62", "off 62"]);
    s.up(k, Modifiers::none());
    assert_eq!(s.sounds()[heard..], ["on 62", "off 62"]);
}

/// The ruler: a click locates (snapped), a drag selects the range over
/// the shown tracks (Swift `onRulerClick` / `onRulerRange`).
#[gpui::test]
fn the_ruler_locates_and_selects_a_range(cx: &mut TestAppContext) {
    let mut s = roll_on(cx, roll_fake(), EditorTarget::Sequence { seq: 3 });
    let (snap, meter) = s.snap_and_meter();
    let canvas = s.canvas();
    let y = canvas.origin.y + px(10.);
    s.fake.clear_calls();
    s.click_at(point(canvas.origin.x + px(203.), y));
    assert_eq!(
        s.fake
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("locate("))
            .collect::<Vec<_>>(),
        [format!("locate({})", snap.apply(203 * 96, &meter))]
    );
    let (a, b) = (
        point(canvas.origin.x + px(100.), y),
        point(canvas.origin.x + px(500.), y),
    );
    s.drag(a, b, Modifiers::none());
    let sel = s.model(|d| d.selection().clone());
    assert_eq!(
        sel.time_range,
        Some(TickRange {
            start: snap.apply(9_600, &meter),
            end: snap.apply(48_000, &meter),
        })
    );
    assert_eq!(sel.lanes, [CHORDS, BASS], "the shown tracks");
    assert!(sel.events.is_empty());
}

/// Leaving the pane (another editor) drops a drag in progress and its
/// sound.
#[gpui::test]
fn leaving_the_roll_cancels_its_drag(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    s.fake.clear_calls();
    let (from, to) = (s.at(162., 64), s.at(322., 64));
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    assert!(s.model(|d| d.gesture_open()));
    s.update_model(|d| {
        let target = d.layout().detail.as_ref().unwrap().target;
        d.open_detail(target, Some(DetailEditorKind::List));
    });
    assert!(!s.model(|d| d.gesture_open()));
    assert_eq!(s.sounds(), ["on 64", "off 64"]);
    assert!(s.model(|d| !d.roll_focused()));
    s.up(to, Modifiers::none());
    assert!(!s.fake.calls().iter().any(|c| c == "end_edit"));
}

/// The roll's release meets Busy, then the roll leaves the pane: its
/// pending final edit is aborted, never landed.
#[gpui::test]
fn leaving_the_roll_aborts_its_pending_release(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    s.fake.set_call_error("end_edit", Some(busy()));
    let (from, to) = (s.at(162., 64), s.at(322., 64));
    s.drag(from, to, Modifiers::none());
    assert!(s.model(|d| d.gesture_open()), "the final edit waits");
    s.fake.clear_calls();
    s.update_model(|d| {
        let target = d.layout().detail.as_ref().unwrap().target;
        d.open_detail(target, Some(DetailEditorKind::List));
    });
    assert!(s.fake.calls().iter().any(|c| c == "abort_edit"));
    assert!(!s.model(|d| d.gesture_open()));
    s.fake.set_call_error("end_edit", None);
    s.cx.executor().advance_clock(Duration::from_millis(100));
    s.cx.run_until_parked();
    assert!(!s.fake.calls().iter().any(|c| c == "end_edit"));
    assert_ne!(s.undo_label().as_deref(), Some("Move Notes"));
}

/// Once the roll's pending final has landed, another view's drag waiting
/// on Busy is not the roll's: hiding the pane leaves it to land.
#[gpui::test]
fn the_roll_cancels_only_its_own_pending_drag(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    s.fake.fail_next("end_edit", busy());
    let (from, to) = (s.at(162., 64), s.at(322., 64));
    s.drag(from, to, Modifiers::none());
    assert!(s.model(|d| d.gesture_open()));
    s.cx.executor().advance_clock(Duration::from_millis(40));
    s.cx.run_until_parked();
    assert!(!s.model(|d| d.gesture_open()), "the retry landed it");
    assert_eq!(s.undo_label().as_deref(), Some("Move Notes"));

    // An arrangement drag whose release meets Busy.
    let block = s
        .element("button", "Block on Chords, bar 1, 1 event")
        .bounds;
    s.fake.set_call_error("end_edit", Some(busy()));
    let from = block.center();
    let to = point(from.x + px(100.), from.y);
    s.drag(from, to, Modifiers::none());
    assert!(s.model(|d| d.gesture_open()), "its final edit waits");
    // A plain click on a note opens no gesture: the pending one is still
    // the arrangement's.
    let p = s.at(80., 60);
    s.click_with(p, Modifiers::none());
    assert_eq!(s.selected(), [1]);
    assert!(s.model(|d| d.gesture_open()));
    s.fake.clear_calls();
    s.update_model(|d| d.update_layout(|l| l.views.arrange.bottom.shown = false));
    assert!(s.model(|d| d.gesture_open()), "hiding the roll keeps it");
    assert!(!s.fake.calls().iter().any(|c| c == "abort_edit"));
    s.fake.set_call_error("end_edit", None);
    s.cx.executor().advance_clock(Duration::from_millis(40));
    s.cx.run_until_parked();
    assert_eq!(s.undo_label().as_deref(), Some("Move Block"));
}

/// The keyboard strip's guards: a drag into its top (the ruler's height)
/// keeps the key; a move with no button down (the release was missed)
/// lets the key go; Escape lets it go.
#[gpui::test]
fn the_key_strip_keeps_to_its_keys(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    let top = s.element("group", KEYBOARD).bounds.origin;
    let top = point(top.x + px(10.), top.y + px(10.));
    let k62 = s.key(62);
    s.down(k62, Modifiers::none());
    s.move_to(top, Modifiers::none());
    assert_eq!(s.sounds(), ["on 62"], "the top is not a key");
    s.up(top, Modifiers::none());
    assert_eq!(s.sounds(), ["on 62", "off 62"]);

    // A move with no button down: the release was missed.
    let k64 = s.key(64);
    s.down(k62, Modifiers::none());
    s.hover(k64);
    assert_eq!(s.sounds()[2..], ["on 62", "off 62"]);
    s.move_to(k64, Modifiers::none());
    assert_eq!(s.sounds().len(), 4, "the key is no longer held");

    // Escape.
    s.down(k62, Modifiers::none());
    assert_eq!(s.sounds()[4..], ["on 62"]);
    s.keys("escape");
    assert_eq!(s.sounds()[4..], ["on 62", "off 62"]);
    s.move_to(k64, Modifiers::none());
    s.up(k64, Modifiers::none());
    assert_eq!(s.sounds().len(), 6);
}

/// A song whose saved last editor is not in this app yet: a block's
/// double-click opens the Piano Roll, and a save keeps the saved value.
#[gpui::test]
fn an_unported_last_editor_double_click_opens_the_roll(cx: &mut TestAppContext) {
    for json in ["stripChart", "drumGrid"] {
        let fake = roll_fake();
        fake.set_layout(Some(&format!(r#"{{"lastKind":"{json}"}}"#)));
        let mut s = setup(cx, fake);
        let block = s
            .element("button", "Block on Chords, bar 1, 1 event")
            .bounds;
        let at = block.center();
        for count in [1, 2] {
            s.cx.simulate_event(MouseDownEvent {
                position: at,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count: count,
                first_mouse: false,
            });
            s.cx.simulate_event(MouseUpEvent {
                position: at,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count: count,
            });
        }
        s.cx.run_until_parked();
        assert_eq!(s.model(|d| d.detail_kind_name()), "piano_roll", "{json}");
        assert!(s.find("group", ROLL).is_some(), "{json}: the roll shows");
        let saved = s.saved_layout(&format!("vizdaw-unported-{json}.vizdaw"));
        assert_eq!(saved["lastKind"], json);
    }
}

/// Swift `S/PianoRollPane.swift:68-69`: the bar's Exact toggle, the Modify
/// menu's check mark and ⌃⌘E are one setting.
#[gpui::test]
fn exact_mode_shows_a_check_mark_that_matches_the_roll_toggle(cx: &mut TestAppContext) {
    let mut s = chords_roll(cx);
    let exact = s.roll_control("button", "Exact");
    assert_eq!(exact.value.as_deref(), Some("off"));
    assert_eq!(s.menu_item("Modify", "Exact Mode"), (true, Some(false)));

    s.click("button", "Exact");
    assert!(s.model(|d| d.exact_mode()));
    assert_eq!(
        s.roll_control("button", "Exact").value.as_deref(),
        Some("on")
    );
    assert_eq!(s.menu_item("Modify", "Exact Mode"), (true, Some(true)));

    // ⌃⌘E turns it off again.
    let chord = core_control::bindings::Chord::parse("⌃⌘E").unwrap();
    let key = menus::keystroke(&chord).unwrap();
    s.keys(&key);
    assert!(!s.model(|d| d.exact_mode()));
    assert_eq!(
        s.roll_control("button", "Exact").value.as_deref(),
        Some("off")
    );
    assert_eq!(s.menu_item("Modify", "Exact Mode"), (true, Some(false)));
}
