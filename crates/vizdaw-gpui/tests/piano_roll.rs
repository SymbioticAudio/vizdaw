//! The Piano Roll in the bottom pane (Swift `PianoRollEditor`,
//! `PianoRollBar`, `PianoRollContainer`, `PianoRollView` and
//! `PianoKeyboardView`), read-only: its bar, the 64-point side column,
//! the notes in the tree, rows and zoom saved in the layout, centring and
//! a long track drawn a screen at a time, driven through GPUI's test
//! platform with the scripted session.

use std::sync::Arc;

use gpui::{
    Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, ScrollDelta,
    ScrollEvent, TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::arrangement::{RULER_HEIGHT, TimeAxis};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Tool};
use vizdaw_app::piano_roll::PitchAxis;
use vizdaw_app::testing::{FakeSession, demo_fake, note_row, roll_fake};
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::events::EventRow;
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
        let keymap = self.app.read_with(&self.cx, |a, _| a.keymap.clone());
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
    assert_eq!(s.element("button", PANE).value.as_deref(), Some(ROLL));

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

    // The pane's picker lists the Piano Roll, then the List.
    s.click("button", PANE);
    let items: Vec<String> = s.menu_items().into_iter().map(|e| e.label).collect();
    assert_eq!(items, [ROLL, "List"]);
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
