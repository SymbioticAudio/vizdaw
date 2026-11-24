//! The sheet host (slice 7b Task 2) with a dummy sheet: shown over the
//! window with its label and buttons in the tree, Return presses the
//! default button and Esc the cancel one, clicks and keys never reach
//! the window under it, Tab moves between its fields and typing goes to
//! them, a field edit turns the document's keys off, one sheet per
//! window, none under the control channel; and GPUI's prompts drawn as
//! the sheet card off macOS.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use core_control::ControlError;
use core_control::server::Call;
use futures::FutureExt;
use gpui::{
    AnyElement, App, AppContext, Entity, FocusHandle, Focusable, IntoElement, Modifiers,
    ParentElement, PromptLevel, Styled, TestAppContext, VisualTestContext, Window, WindowHandle,
    div,
};
use serde_json::Value;
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_gpui::control::{self, CallSource};
use vizdaw_gpui::registry::RegistryScope;
use vizdaw_gpui::sheet_host::{ButtonRole, Sheet, SheetButton};
use vizdaw_gpui::text_input::TextInput;
use vizdaw_gpui::{AppState, DocumentWindow};

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext) -> Setup {
    let fake = Arc::new(demo_fake());
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

/// A sheet with two fields, Cancel, a disabled "Later" and Apply (the
/// default); its presses are logged, and Apply closes it only when
/// `closes`.
struct Dummy {
    fields: [Entity<TextInput>; 2],
    pressed: Rc<RefCell<Vec<usize>>>,
    default_enabled: bool,
}

impl Sheet for Dummy {
    fn label(&self) -> String {
        "Dummy Settings".into()
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, _: &mut App) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(self.fields[0].clone())
                    .child(scope.control("field", "Name", true)),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(self.fields[1].clone())
                    .child(scope.control("field", "Rate", true)),
            )
            .into_any_element()
    }

    fn buttons(&self, _: &App) -> Vec<SheetButton> {
        let apply = SheetButton::new("Apply", ButtonRole::Default);
        vec![
            SheetButton::new("Cancel", ButtonRole::Cancel),
            SheetButton::new("Later", ButtonRole::Plain).disabled(),
            if self.default_enabled {
                apply
            } else {
                apply.disabled()
            },
        ]
    }

    fn focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        self.fields.iter().map(|f| f.focus_handle(cx)).collect()
    }

    fn press(&mut self, index: usize, _: &mut Window, _: &mut App) -> bool {
        self.pressed.borrow_mut().push(index);
        true
    }
}

struct Shown {
    pressed: Rc<RefCell<Vec<usize>>>,
    fields: [Entity<TextInput>; 2],
}

fn show_dummy(s: &mut Setup, default_enabled: bool) -> Shown {
    let pressed = Rc::new(RefCell::new(vec![]));
    let log = pressed.clone();
    let (shown, fields) = s
        .window
        .update(&mut s.cx, |v, window, cx| {
            let fields = [
                cx.new(|cx| TextInput::new("Name", cx)),
                cx.new(|cx| TextInput::new("Rate", cx)),
            ];
            let sheet = Dummy {
                fields: fields.clone(),
                pressed: log,
                default_enabled,
            };
            (v.show_sheet(Box::new(sheet), window, cx).is_ok(), fields)
        })
        .unwrap();
    assert!(shown, "the sheet shows");
    s.cx.run_until_parked();
    Shown { pressed, fields }
}

fn tree(s: &mut Setup) -> Value {
    s.window
        .read_with(&s.cx, |v, _| v.registry().borrow().tree())
        .unwrap()
}

fn node(s: &mut Setup, role: &str, label: &str) -> Option<Value> {
    tree(s)["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["role"] == role && n["label"] == label)
        .cloned()
}

fn click(s: &mut Setup, role: &str, label: &str) {
    let id = node(s, role, label).unwrap_or_else(|| panic!("no {role} {label}"))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let at = s
        .window
        .read_with(&s.cx, |v, _| {
            v.registry().borrow().resolve(&id).unwrap().bounds.center()
        })
        .unwrap();
    s.cx.simulate_click(at, Modifiers::none());
    s.cx.run_until_parked();
}

fn sheet_label(s: &mut Setup) -> Option<String> {
    s.window.read_with(&s.cx, |v, _| v.sheet_label()).unwrap()
}

fn text(s: &mut Setup, field: &Entity<TextInput>) -> String {
    field.read_with(&s.cx, |f, _| f.text().to_owned())
}

#[gpui::test]
fn a_sheet_shows_over_the_window_with_its_label_and_buttons_and_the_windows_paths_stay(
    cx: &mut TestAppContext,
) {
    let mut s = setup(cx);
    let play = node(&mut s, "button", "Play").unwrap()["id"].clone();
    show_dummy(&mut s, true);
    assert_eq!(sheet_label(&mut s).as_deref(), Some("Dummy Settings"));
    assert!(node(&mut s, "sheet", "Dummy Settings").is_some());
    assert_eq!(node(&mut s, "button", "Apply").unwrap()["enabled"], true);
    assert_eq!(node(&mut s, "button", "Later").unwrap()["enabled"], false);
    assert!(node(&mut s, "field", "Name").is_some());
    // The window's own controls keep their IDs under the sheet.
    assert_eq!(node(&mut s, "button", "Play").unwrap()["id"], play);
}

#[gpui::test]
fn return_presses_the_default_button_and_esc_the_cancel_one(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let d = show_dummy(&mut s, true);
    s.cx.simulate_keystrokes("enter");
    assert_eq!(*d.pressed.borrow(), [2]);
    assert_eq!(sheet_label(&mut s), None, "Apply closed it");
    let d = show_dummy(&mut s, true);
    s.cx.simulate_keystrokes("escape");
    assert_eq!(*d.pressed.borrow(), [0]);
    assert_eq!(sheet_label(&mut s), None);
    // A disabled default: Return does nothing.
    let d = show_dummy(&mut s, false);
    s.cx.simulate_keystrokes("enter");
    assert!(d.pressed.borrow().is_empty());
    assert!(sheet_label(&mut s).is_some());
}

#[gpui::test]
fn a_click_presses_an_enabled_button_and_nothing_under_the_sheet(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let play = node(&mut s, "button", "Play").unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let at = s
        .window
        .read_with(&s.cx, |v, _| {
            v.registry()
                .borrow()
                .resolve(&play)
                .unwrap()
                .bounds
                .center()
        })
        .unwrap();
    let d = show_dummy(&mut s, true);
    // Play is under the dimmed backdrop.
    s.cx.simulate_click(at, Modifiers::none());
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("play(3)"), 0);
    click(&mut s, "button", "Later");
    assert!(d.pressed.borrow().is_empty(), "a disabled button");
    click(&mut s, "button", "Cancel");
    assert_eq!(*d.pressed.borrow(), [0]);
    assert_eq!(sheet_label(&mut s), None);
    // Gone, Play plays again.
    s.cx.simulate_click(at, Modifiers::none());
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("play(3)"), 1);
}

#[gpui::test]
fn no_window_key_acts_while_a_sheet_is_up(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let d = show_dummy(&mut s, true);
    s.cx.simulate_keystrokes("space");
    s.cx.simulate_keystrokes("s");
    s.cx.simulate_keystrokes("shift-.");
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("play(3)"), 0);
    assert_eq!(s.fake.count("locate(0)"), 0);
    // The window's own commands do nothing either (Save, Close).
    s.cx.simulate_keystrokes("secondary-w");
    s.cx.run_until_parked();
    assert!(s.window.read_with(&s.cx, |_, _| ()).is_ok(), "still open");
    s.cx.simulate_keystrokes("escape");
    assert_eq!(*d.pressed.borrow(), [0]);
    s.cx.simulate_keystrokes("space");
    assert_eq!(s.fake.count("play(3)"), 1);
}

#[gpui::test]
fn tab_moves_between_the_sheets_fields_and_typing_goes_to_them(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let d = show_dummy(&mut s, true);
    s.cx.simulate_keystrokes("tab");
    s.cx.simulate_input("a b");
    s.cx.simulate_keystrokes("space");
    assert_eq!(text(&mut s, &d.fields[0]), "a b ");
    assert_eq!(s.fake.count("play(3)"), 0, "space typed, nothing played");
    s.cx.simulate_keystrokes("tab");
    s.cx.simulate_input("48");
    assert_eq!(text(&mut s, &d.fields[1]), "48");
    // Shift-Tab goes back, and round the end.
    s.cx.simulate_keystrokes("shift-tab");
    s.cx.simulate_input("c");
    assert_eq!(text(&mut s, &d.fields[0]), "a b c");
    s.cx.simulate_keystrokes("shift-tab");
    s.cx.simulate_input("0");
    assert_eq!(text(&mut s, &d.fields[1]), "480");
    // Return in a field still presses the default.
    s.cx.simulate_keystrokes("enter");
    assert_eq!(*d.pressed.borrow(), [2]);
}

#[gpui::test]
fn a_sheet_field_edit_turns_the_documents_keys_off_until_it_closes(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let doc = s.window.read_with(&s.cx, |v, _| v.doc()).unwrap();
    let keys = |s: &mut Setup| {
        s.app.read_with(&s.cx, |a, _| {
            a.store.document(doc).unwrap().transport_keys_enabled()
        })
    };
    show_dummy(&mut s, true);
    assert!(keys(&mut s), "no field has the keyboard yet");
    s.cx.simulate_keystrokes("tab");
    s.cx.run_until_parked();
    assert!(!keys(&mut s), "a field edits");
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert!(keys(&mut s));
}

#[gpui::test]
fn one_sheet_per_window(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    show_dummy(&mut s, true);
    let second = s
        .window
        .update(&mut s.cx, |v, window, cx| {
            let fields = [
                cx.new(|cx| TextInput::new("Name", cx)),
                cx.new(|cx| TextInput::new("Rate", cx)),
            ];
            let sheet = Dummy {
                fields,
                pressed: Rc::default(),
                default_enabled: true,
            };
            v.show_sheet(Box::new(sheet), window, cx).is_ok()
        })
        .unwrap();
    assert!(!second);
}

struct NoCalls;

impl CallSource for NoCalls {
    fn next_call(&self) -> Option<Call> {
        None
    }
    fn reply(&self, _: u64, _: Result<Value, ControlError>) -> bool {
        true
    }
}

#[gpui::test]
fn no_sheet_shows_under_the_control_channel(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.cx.update(|_, cx| control::pump(&NoCalls, cx));
    let shown = s
        .window
        .update(&mut s.cx, |v, window, cx| {
            let fields = [
                cx.new(|cx| TextInput::new("Name", cx)),
                cx.new(|cx| TextInput::new("Rate", cx)),
            ];
            let sheet = Dummy {
                fields,
                pressed: Rc::default(),
                default_enabled: true,
            };
            v.show_sheet(Box::new(sheet), window, cx).is_ok()
        })
        .unwrap();
    assert!(!shown);
    assert_eq!(sheet_label(&mut s), None);
}

#[gpui::test]
fn off_macos_gpuis_prompts_are_the_sheet_card_with_return_and_esc(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.cx.update(|_, cx| vizdaw_gpui::sheet_host::use_sheet_prompts(cx));
    let ask = |s: &mut Setup| {
        s.window
            .update(&mut s.cx, |_, window, cx| {
                window.prompt(
                    PromptLevel::Warning,
                    "Save changes?",
                    Some("Your changes are lost."),
                    &["Save", "Don't Save", "Cancel"],
                    cx,
                )
            })
            .unwrap()
    };
    let answer = ask(&mut s);
    s.cx.run_until_parked();
    let up = s
        .window
        .update(&mut s.cx, |_, window, _| window.has_active_prompt())
        .unwrap();
    assert!(up, "drawn in the window");
    // No window key acts under it.
    s.cx.simulate_keystrokes("space");
    assert_eq!(s.fake.count("play(3)"), 0);
    s.cx.simulate_keystrokes("escape");
    assert_eq!(answer.now_or_never(), Some(Ok(2)));
    let answer = ask(&mut s);
    s.cx.run_until_parked();
    s.cx.simulate_keystrokes("enter");
    assert_eq!(answer.now_or_never(), Some(Ok(0)));
    let gone = s
        .window
        .update(&mut s.cx, |_, window, _| !window.has_active_prompt())
        .unwrap();
    assert!(gone);
}
