//! Step entry's keys in the document window (Swift `KeyRouter` with step
//! entry): `.`, the number row and Delete keep their global meaning while
//! step entry is off and are step entry's while it is on; "reserve number
//! row" keeps the row the locators'; a focused text field types them; the
//! locator keys (0–9, ⌥0–9, ⌥⇧0–9) as the platforms deliver them; R does
//! not record from a text field; only rest, tie and Delete Last Step
//! repeat while held. Driven through GPUI's test platform.

use std::sync::Arc;

use gpui::{
    Entity, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext, WindowHandle,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_app::testing::{FakeSession, demo_fake, roll_fake};
use vizdaw_ffi::edit::{EditCommand, EventEdit};
use vizdaw_ffi::step::{NoteValueRow, StepInputRow};
use vizdaw_gpui::{AppState, DocumentWindow};

const BAR: i64 = 61_440;

/// `Keystroke::parse` wants a `&'static str`.
fn leak(k: &str) -> &'static str {
    Box::leak(k.to_owned().into_boxed_str())
}

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

    /// Presses `keys` and lets each key up (as a keyboard does).
    fn keys(&mut self, keys: &str) {
        for k in keys.split_whitespace() {
            self.cx.simulate_keystrokes(k);
            self.up(k);
        }
        self.cx.run_until_parked();
    }

    fn up(&mut self, k: &str) {
        let keystroke = Keystroke::parse(leak(k)).unwrap();
        self.cx.simulate_event(KeyUpEvent { keystroke });
    }

    /// A key-down the platform reports as a key repeat.
    fn held(&mut self, k: &str) {
        let keystroke = Keystroke::parse(leak(k)).unwrap();
        self.cx.simulate_event(KeyDownEvent {
            keystroke,
            is_held: true,
            prefer_character_input: false,
        });
        self.cx.run_until_parked();
    }

    /// A key-down as macOS and Linux deliver ⌥⇧`digit` on a US layout:
    /// the shifted character, ⇧ folded into it.
    fn alt_shifted(&mut self, shifted: &str) {
        let keystroke = Keystroke {
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::default()
            },
            key: shifted.into(),
            key_char: None,
        };
        self.cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        self.cx.simulate_event(KeyUpEvent { keystroke });
        self.cx.run_until_parked();
    }

    fn tick(&self) -> i64 {
        use vizdaw_app::DocSession;
        self.fake.transport().unwrap().tick
    }

    fn set_tick(&mut self, tick: i64) {
        self.fake.set_transport(|t| t.tick = tick);
        self.update_model(|d| {
            let _ = d.poll_transport();
        });
    }

    fn locator(&mut self, n: usize) -> Option<i64> {
        self.model(|d| d.layout().transport.locator(n))
    }

    fn step_inputs(&self) -> Vec<StepInputRow> {
        self.fake
            .step_inputs()
            .into_iter()
            .map(|(_, _, i)| i)
            .collect()
    }

    fn step_on(&mut self) -> bool {
        self.model(|d| d.step_entry_on())
    }

    fn element_center(&mut self, label: &str) -> Point<Pixels> {
        self.window
            .read_with(&self.cx, |v, _| {
                let r = v.registry().borrow();
                let e = r.elements().iter().find(|e| e.label == label).cloned();
                e.map(|e| e.bounds.center())
            })
            .unwrap()
            .unwrap_or_else(|| panic!("no {label:?}"))
    }

    fn element_value(&mut self, label: &str) -> Option<String> {
        self.window
            .read_with(&self.cx, |v, _| {
                let r = v.registry().borrow();
                let e = r.elements().iter().find(|e| e.label == label).cloned();
                e.and_then(|e| e.value)
            })
            .unwrap()
    }

    fn click_at(&mut self, position: Point<Pixels>) {
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

    /// The keyboard layout the windows see (`None`: the platform's).
    fn set_layout(&mut self, id: Option<&str>) {
        self.app.update(&mut self.cx, |a, cx| {
            a.keyboard_layout_override = id.map(String::from);
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    fn set_reserve(&mut self, on: bool) {
        self.app.update(&mut self.cx, |a, cx| {
            a.prefs.set_reserve_number_row(on).unwrap();
            cx.notify();
        });
        self.cx.run_until_parked();
    }
}

/// The carry from the locators: with step entry off, 2 goes to locator 2,
/// ⌥2 sets it and ⌥⇧2 clears it, both as the test platform names the
/// chord and as macOS and Linux deliver it (`alt-@` on a US layout).
#[gpui::test]
fn the_locator_keys_go_set_and_clear(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.set_tick(BAR);
    s.keys("alt-2");
    assert_eq!(s.locator(2), Some(BAR), "⌥2 sets locator 2");
    s.set_tick(0);
    s.keys("2");
    assert_eq!(s.tick(), BAR, "2 goes to locator 2");
    s.keys("alt-shift-2");
    assert_eq!(s.locator(2), None, "⌥⇧2 clears locator 2");
    // As the platforms report ⌥⇧2 on a US layout: the shifted character.
    s.set_layout(Some("com.apple.keylayout.US"));
    s.set_tick(2 * BAR);
    s.keys("alt-2");
    assert_eq!(s.locator(2), Some(2 * BAR));
    s.alt_shifted("@");
    assert_eq!(s.locator(2), None, "⌥@ clears locator 2");
    for (n, shifted) in [
        (0, ")"),
        (1, "!"),
        (3, "#"),
        (4, "$"),
        (5, "%"),
        (6, "^"),
        (7, "&"),
        (8, "*"),
        (9, "("),
    ] {
        s.keys(&format!("alt-{n}"));
        assert_eq!(s.locator(n), Some(2 * BAR), "⌥{n}");
        s.alt_shifted(shifted);
        assert_eq!(s.locator(n), None, "⌥⇧{n}");
    }
    s.keys("alt-0");
    s.set_tick(0);
    s.keys("0");
    assert_eq!(s.tick(), 2 * BAR, "0 goes to locator 0");
}

/// On any other layout the shifted character is another key (German
/// ⌥⇧6 arrives as `alt-&`, US ⌥⇧7; French ⌥ on the 1 key too), so the
/// US aliases are not bound: `alt-&` clears nothing. The named chord
/// still works.
#[gpui::test]
fn shifted_characters_clear_nothing_off_a_us_layout(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.set_tick(BAR);
    s.set_layout(Some("com.apple.keylayout.German"));
    s.keys("alt-7");
    s.alt_shifted("&");
    assert_eq!(s.locator(7), Some(BAR), "alt-& is not ⌥⇧7 here");
    s.set_layout(None);
    s.alt_shifted("&");
    assert_eq!(s.locator(7), Some(BAR), "nor on the test platform's layout");
    s.keys("alt-shift-7");
    assert_eq!(s.locator(7), None, "the named chord clears it");
    for (id, us) in [
        ("com.apple.keylayout.US", true),
        ("com.apple.keylayout.ABC", true),
        ("com.apple.keylayout.USExtended", true),
        ("English (US)", true),
        ("com.apple.keylayout.German", false),
        ("com.apple.keylayout.French", false),
        ("com.apple.keylayout.British", false),
        ("English (UK)", false),
        ("English (US, intl., with dead keys)", false),
        ("unknown", false),
        ("", false),
    ] {
        assert_eq!(vizdaw_gpui::menus::us_layout(id), us, "{id}");
    }
}

/// Off, `.` is Next Marker, `2` locator 2 and Delete deletes the selected
/// notes; on (T), they are Dotted, Half Note and Delete Last Step, Delete
/// even while the Piano Roll has the keyboard.
#[gpui::test]
fn step_keys_win_only_during_step_entry(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    s.fake.set_marker_ticks(vec![BAR]);
    s.update_model(|d| d.update_layout(|l| l.views.current_mut().bottom.size = 360.));
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 31 },
            Some(DetailEditorKind::PianoRoll),
        )
    });
    s.keys(".");
    assert_eq!(s.tick(), BAR, ". is Next Marker");
    s.set_tick(2 * BAR);
    s.keys("alt-2");
    s.set_tick(0);
    s.keys("2");
    assert_eq!(s.tick(), 2 * BAR, "2 is locator 2");
    // Select the C4 note on the roll, which then has the keyboard.
    let note = s.element_center("Note C4, bar 1 beat 1, velocity 64");
    s.click_at(note);
    assert!(s.model(|d| d.roll_focused()));
    s.keys("backspace");
    assert!(
        s.fake.edits().iter().any(|e| matches!(
            e,
            EditCommand::Event {
                edit: EventEdit::Delete { .. }
            }
        )),
        "Delete deletes the selected notes: {:?}",
        s.fake.edits()
    );
    assert!(s.step_inputs().is_empty());

    s.set_tick(0);
    s.keys("t");
    assert!(s.step_on(), "T turns step entry on");
    let edits = s.fake.edits().len();
    s.keys(". 2 backspace");
    assert_eq!(
        s.step_inputs(),
        [
            StepInputRow::Dot,
            StepInputRow::Value {
                value: NoteValueRow::Half
            },
            StepInputRow::DeleteLast
        ]
    );
    assert_eq!(s.tick(), 0, "neither Next Marker nor the locator");
    assert_eq!(s.fake.edits().len(), edits, "the roll deleted nothing");
    s.keys("t");
    assert!(!s.step_on(), "T turns it off");
    s.keys("2");
    assert_eq!(s.tick(), 2 * BAR, "the number row is the locators' again");
}

/// With "reserve number row" on, `2` stays locator 2 during step entry;
/// `.` is still Dotted.
#[gpui::test]
fn a_reserved_number_row_stays_the_locators(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.set_tick(BAR);
    s.keys("alt-2");
    s.set_tick(0);
    s.set_reserve(true);
    s.keys("t");
    assert!(s.step_on());
    s.keys("2 .");
    assert_eq!(s.tick(), BAR, "2 is locator 2");
    assert_eq!(s.step_inputs(), [StepInputRow::Dot]);
    s.set_reserve(false);
    s.keys("2");
    assert_eq!(
        s.step_inputs().last(),
        Some(&StepInputRow::Value {
            value: NoteValueRow::Half
        })
    );
}

/// A focused text field types `.`, `2` and Delete during step entry; T
/// and R type too, and R does not record.
#[gpui::test]
fn a_focused_text_field_types_the_step_keys(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.keys("t");
    assert!(s.step_on());
    // The Position field, its text all selected by the click.
    let at = s.element_center("Position");
    s.click_at(at);
    s.keys(". 2 2 backspace t");
    assert_eq!(s.element_value("Position").as_deref(), Some(".2t"));
    assert!(s.step_inputs().is_empty(), "{:?}", s.step_inputs());
    assert!(s.step_on(), "T typed, not run");
    // Record's R: step entry off, the field still typing.
    s.update_model(|d| d.stop_step_entry());
    s.keys("r");
    assert_eq!(s.element_value("Position").as_deref(), Some(".2tr"));
    assert_eq!(s.fake.count("record(3)"), 0, "R typed, not recorded");
    assert_eq!(s.model(|d| d.status().map(String::from)), None);
    // Out of the field, R runs Record (the demo has no armed track).
    s.keys("escape r");
    assert_eq!(
        s.model(|d| d.status().map(String::from)).as_deref(),
        Some("arm a track that has an input to record")
    );
}

fn count(s: &Setup, i: StepInputRow) -> usize {
    s.step_inputs().iter().filter(|x| **x == i).count()
}

/// Only rest, tie and Delete Last Step run again while their key is
/// held; Dotted, a value and T run once per press.
#[gpui::test]
fn only_rest_tie_and_delete_last_repeat(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.keys("t");
    s.cx.simulate_keystrokes(".");
    s.held(".");
    s.held(".");
    s.up(".");
    assert_eq!(s.step_inputs(), [StepInputRow::Dot], "Dotted once");
    assert!(s.model(|d| d.step_entry().state().unwrap().dotted));
    s.cx.simulate_keystrokes("4");
    s.held("4");
    s.up("4");
    assert_eq!(s.step_inputs().len(), 2, "a value once");
    s.cx.simulate_keystrokes("0");
    s.held("0");
    s.held("0");
    s.up("0");
    let rests = s
        .step_inputs()
        .iter()
        .filter(|i| **i == StepInputRow::Rest)
        .count();
    assert_eq!(rests, 3, "Rest repeats");
    s.cx.simulate_keystrokes("=");
    s.held("=");
    s.up("=");
    s.cx.simulate_keystrokes("backspace");
    s.held("backspace");
    s.up("backspace");
    assert_eq!(count(&s, StepInputRow::Tie), 2, "Tie repeats");
    assert_eq!(
        count(&s, StepInputRow::DeleteLast),
        2,
        "Delete Last Step repeats"
    );
    // A pressed key again after letting go runs again.
    s.keys(".");
    assert_eq!(count(&s, StepInputRow::Dot), 2);
    // T held does not flip step entry back and forth.
    s.cx.simulate_keystrokes("t");
    s.held("t");
    s.up("t");
    s.cx.run_until_parked();
    assert!(!s.step_on());
}

/// The Step menu's items, as built for the platform menus.
fn step_menu(s: &mut Setup) -> Vec<vizdaw_gpui::menus::ItemSpec> {
    s.cx.update(|_, cx| vizdaw_gpui::menus::shown(cx))
        .expect("the menus are shown")
        .menus
        .into_iter()
        .find(|m| m.title == "Step")
        .expect("a Step menu")
        .items
}

/// Runs menu item `id` as its platform menu item does.
fn menu_item(s: &mut Setup, id: &str) {
    s.cx.update(|window, cx| window.dispatch_action(vizdaw_gpui::menus::action_for(id), cx));
    s.cx.run_until_parked();
}

/// With the Step menu built, its keys are still step entry's only: off,
/// `2` is locator 2 and `.` Next Marker; on, Half Note and Dotted. The
/// menu items carry the keys' own action (macOS shows a key equivalent
/// only for an action it finds bound) and bind nothing more.
#[gpui::test]
fn the_step_menu_leaves_the_keys_to_step_entry(cx: &mut TestAppContext) {
    use vizdaw_gpui::menus::ItemSpec;
    let mut s = setup(cx, demo_fake());
    s.fake.set_marker_ticks(vec![BAR]);
    let items = step_menu(&mut s);
    let enabled: Vec<(&str, bool)> = items
        .iter()
        .filter_map(|i| match i {
            ItemSpec::Command { id, enabled, .. } => Some((*id, *enabled)),
            _ => None,
        })
        .collect();
    assert_eq!(enabled[0], ("step.entry", true));
    assert!(enabled[1..].iter().all(|(_, e)| !e), "{enabled:?}");
    // Each item's action is the one its key is bound to, and only there.
    for id in ["step.entry", "step.half", "step.dot", "step.delete_last"] {
        let action = vizdaw_gpui::menus::action_for(id);
        // As macOS looks it up for the menu item: in the whole keymap.
        let bound: Vec<gpui::KeyBinding> = s.cx.update(|_, cx| {
            cx.key_bindings()
                .borrow()
                .bindings_for_action(action.as_ref())
                .cloned()
                .collect()
        });
        assert!(!bound.is_empty(), "{id} shows its key");
        assert!(
            bound.iter().all(|b| b.action().name() == "vizdaw::StepKey"),
            "{id}"
        );
    }

    s.set_tick(2 * BAR);
    s.keys("alt-2");
    s.set_tick(0);
    s.keys(".");
    assert_eq!(s.tick(), BAR, ". is Next Marker");
    s.keys("2");
    assert_eq!(s.tick(), 2 * BAR, "2 is locator 2");
    assert!(s.step_inputs().is_empty());

    s.set_tick(0);
    menu_item(&mut s, "step.entry");
    assert!(s.step_on(), "Step › Step Entry turns it on");
    assert!(step_menu(&mut s).iter().all(|i| match i {
        ItemSpec::Command { enabled, .. } => *enabled,
        _ => true,
    }));
    s.keys("2 .");
    assert_eq!(
        s.step_inputs(),
        [
            StepInputRow::Value {
                value: NoteValueRow::Half
            },
            StepInputRow::Dot
        ]
    );
    assert_eq!(s.tick(), 0, "neither the locator nor Next Marker");
}

/// A menu item is not a held key: Dotted chosen twice runs twice, and
/// the `.` pressed after it still runs.
#[gpui::test]
fn step_menu_items_run_every_time(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.keys("t");
    assert!(s.step_on());
    menu_item(&mut s, "step.dot");
    menu_item(&mut s, "step.dot");
    assert_eq!(s.step_inputs(), [StepInputRow::Dot, StepInputRow::Dot]);
    s.cx.simulate_keystrokes(".");
    s.cx.run_until_parked();
    assert_eq!(s.step_inputs().len(), 3, "the key after the menu item");
    // The key is still down: a repeat of it does not run again.
    s.held(".");
    assert_eq!(s.step_inputs().len(), 3, "held, not repeated");
    menu_item(&mut s, "step.dot");
    assert_eq!(
        s.step_inputs().len(),
        4,
        "the menu item while the key is down"
    );
    s.up(".");
    s.keys(".");
    assert_eq!(s.step_inputs().len(), 5);
}
