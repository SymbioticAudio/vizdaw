//! The MIDI Learn Maps sheet (Swift `LearnMapsSheet`; slice 9d Task 6),
//! through GPUI's test platform with the scripted session and core: the
//! strip name's "MIDI Learn Maps…", the rows (source, target or
//! "(missing)", mode, pick-up, Remove), a refusal's line, Done and Esc.

mod mixer_support;

use std::sync::Arc;

use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use mixer_support::Setup;
use vizdaw_app::DocumentStore;
use vizdaw_app::learn::{MISSING, NO_MAPS, PICKUP_HELP};
use vizdaw_app::testing::{FakeCore, mixer_fake};
use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::surfaces::{
    LearnKindRow, LearnMapRow, LearnModeRow, LearnSourceRow, SurfaceStateRow,
};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{DocumentWindow, RunCommand};

const ITEM: &str = "MIDI Learn Maps\u{2026}";
const TITLE: &str = "MIDI Learn Maps";

fn map(number: u8, target: T, mode: LearnModeRow, picks_up: bool) -> LearnMapRow {
    LearnMapRow {
        source: LearnSourceRow {
            port: "Knobs".into(),
            kind: LearnKindRow::Cc7,
            channel: 0,
            number,
        },
        text: format!("Knobs \u{00B7} CC {number} ch 1"),
        target,
        missing: false,
        mode,
        pickup: true,
        picks_up,
    }
}

/// CC 7 drives Synth's fader (absolute, picks up); CC 8 is relative.
fn two_maps() -> Vec<LearnMapRow> {
    vec![
        map(7, T::StripFader { strip: 1 }, LearnModeRow::Absolute, true),
        map(
            8,
            T::StripPan { strip: 1 },
            LearnModeRow::Relative2sComplement,
            false,
        ),
    ]
}

fn setup(cx: &mut TestAppContext, maps: Vec<LearnMapRow>) -> (Setup, Arc<FakeCore>) {
    let f = mixer_fake();
    f.set_learn_maps(maps);
    let core = Arc::new(FakeCore::new());
    let fake = Arc::new(f);
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(core.clone());
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let window = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, window)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    let mut s = Setup {
        fake,
        app,
        window,
        cx: vcx,
    };
    s.cx.dispatch_action(RunCommand::new("ui.view_mix"));
    s.cx.run_until_parked();
    (s, core)
}

fn sheet(s: &mut Setup) -> Option<String> {
    s.window.read_with(&s.cx, |v, _| v.sheet_label()).unwrap()
}

/// Opens the sheet from Synth's name menu.
fn open(s: &mut Setup) {
    s.right_click("text", "Synth");
    s.click("menuitem", ITEM);
}

fn texts(s: &mut Setup) -> Vec<Element> {
    s.elements()
        .into_iter()
        .filter(|e| e.role == "text")
        .collect()
}

#[gpui::test]
fn the_strip_menu_opens_the_maps_sheet(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, two_maps());
    s.right_click("text", "Synth");
    let titles = s.titles();
    assert_eq!(titles.last().map(String::as_str), Some(ITEM), "{titles:?}");
    // After the group and VCA items.
    let vca = titles.iter().position(|t| t == "Remove from VCA").unwrap();
    assert_eq!(titles.iter().position(|t| t == ITEM), Some(vca + 1));
    s.click("menuitem", ITEM);
    assert!(s.titles().is_empty(), "the menu closed");
    assert_eq!(sheet(&mut s).as_deref(), Some(TITLE));
    assert!(s.find("sheet", TITLE).is_some());
    assert!(s.model(|d| d.learn.maps_open() && d.learn.is_active()));
    let card = s.element("sheet", TITLE).bounds;
    // 640 wide; the tree's frame is inside its 1-point border.
    assert_eq!(f32::from(card.size.width), 638.);
    // Master's menu has it too.
    s.click("button", "Done");
    s.right_click("text", "Master");
    assert_eq!(s.titles().last().map(String::as_str), Some(ITEM));
}

#[gpui::test]
fn an_empty_project_says_how_to_learn(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, vec![]);
    open(&mut s);
    assert!(s.find("text", NO_MAPS).is_some(), "{:#?}", texts(&mut s));
    assert!(s.find("button", "Remove").is_none());
    assert!(s.find("button", "Done").is_some());
}

#[gpui::test]
fn a_row_shows_source_target_and_missing(cx: &mut TestAppContext) {
    let mut maps = two_maps();
    maps[1].missing = true;
    let (mut s, _) = setup(cx, maps);
    open(&mut s);
    let row = s.element("text", "Knobs \u{00B7} CC 7 ch 1");
    let caption = s.element("text", "Fader \u{00B7} Synth");
    assert!(caption.bounds.origin.y > row.bounds.origin.y);
    assert!(s.find("text", "Knobs \u{00B7} CC 8 ch 1").is_some());
    assert!(s.find("text", MISSING).is_some(), "{:#?}", texts(&mut s));
    assert!(s.find("text", "Pan \u{00B7} Synth").is_none());
    // Each row: the mode picker (190 wide), Pick-up, Remove.
    let mode = s.element("menu", "Mode of Knobs \u{00B7} CC 7 ch 1");
    assert_eq!(mode.value.as_deref(), Some("Absolute"));
    assert_eq!(f32::from(mode.bounds.size.width), 190.);
    let pickup = s.element("checkbox", "Pick-up of Knobs \u{00B7} CC 7 ch 1");
    assert!(pickup.enabled && pickup.selected);
    assert_eq!(pickup.value.as_deref(), Some(PICKUP_HELP));
    assert!(mode.bounds.origin.x < pickup.bounds.origin.x);
    let remove = s.element("button", "Remove Knobs \u{00B7} CC 7 ch 1");
    assert!(pickup.bounds.origin.x < remove.bounds.origin.x);
    assert_eq!(
        s.value("menu", "Mode of Knobs \u{00B7} CC 8 ch 1")
            .as_deref(),
        Some("Relative (2's complement)")
    );
}

#[gpui::test]
fn the_mode_picker_sets_the_mode_as_one_undo_step(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, two_maps());
    open(&mut s);
    s.click("menu", "Mode of Knobs \u{00B7} CC 7 ch 1");
    assert_eq!(
        s.titles(),
        [
            "Absolute",
            "Relative (2's complement)",
            "Relative (binary offset)"
        ]
    );
    assert!(s.element("menuitem", "Absolute").selected);
    s.click("menuitem", "Relative (binary offset)");
    assert!(s.titles().is_empty(), "the picker closed");
    assert_eq!(
        s.fake.learn_modes(),
        vec![(
            two_maps()[0].source.clone(),
            LearnModeRow::RelativeBinOffset
        )]
    );
    assert_eq!(
        s.model(|d| d.snapshot().undo_label.clone()).as_deref(),
        Some("MIDI Learn")
    );
    assert_eq!(
        s.value("menu", "Mode of Knobs \u{00B7} CC 7 ch 1")
            .as_deref(),
        Some("Relative (binary offset)")
    );
    assert_eq!(sheet(&mut s).as_deref(), Some(TITLE));
    // Pick-up off is a step too.
    s.click("checkbox", "Pick-up of Knobs \u{00B7} CC 7 ch 1");
    assert_eq!(
        s.fake.learn_pickups(),
        vec![(two_maps()[0].source.clone(), false)]
    );
    assert!(
        !s.element("checkbox", "Pick-up of Knobs \u{00B7} CC 7 ch 1")
            .selected
    );
}

#[gpui::test]
fn pick_up_is_disabled_for_a_relative_map(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, two_maps());
    open(&mut s);
    let pickup = s.element("checkbox", "Pick-up of Knobs \u{00B7} CC 8 ch 1");
    assert!(!pickup.enabled);
    assert!(!pickup.selected, "unticked though its pickup is on");
    s.click("checkbox", "Pick-up of Knobs \u{00B7} CC 8 ch 1");
    assert!(s.fake.learn_pickups().is_empty());
}

#[gpui::test]
fn remove_removes_and_the_list_follows(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, two_maps());
    open(&mut s);
    s.click("button", "Remove Knobs \u{00B7} CC 7 ch 1");
    assert_eq!(s.fake.removed_learns(), vec![two_maps()[0].source.clone()]);
    assert!(s.find("text", "Knobs \u{00B7} CC 7 ch 1").is_none());
    assert!(s.find("text", "Knobs \u{00B7} CC 8 ch 1").is_some());
    assert_eq!(
        s.model(|d| d.snapshot().undo_label.clone()).as_deref(),
        Some("Remove MIDI Learn")
    );
    s.click("button", "Remove Knobs \u{00B7} CC 8 ch 1");
    assert!(s.find("text", NO_MAPS).is_some());
    assert_eq!(sheet(&mut s).as_deref(), Some(TITLE));
}

#[gpui::test]
fn a_refusal_shows_in_the_sheet_and_it_stays_open(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, two_maps());
    open(&mut s);
    s.fake.fail_next(
        "remove_learn",
        VizError::Model {
            message: "unknown learn source".into(),
        },
    );
    s.click("button", "Remove Knobs \u{00B7} CC 7 ch 1");
    assert!(!s.cx.has_pending_prompt(), "no alert over the sheet");
    let line = s.element("text", "unknown learn source");
    let last_row = s.element("text", "Knobs \u{00B7} CC 8 ch 1");
    assert!(
        line.bounds.origin.y > last_row.bounds.origin.y,
        "under the list"
    );
    assert_eq!(sheet(&mut s).as_deref(), Some(TITLE));
    // The next change that lands clears it.
    s.click("button", "Remove Knobs \u{00B7} CC 7 ch 1");
    assert!(s.find("text", "unknown learn source").is_none());
}

#[gpui::test]
fn done_closes_and_polling_stops(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, two_maps());
    open(&mut s);
    s.click("button", "Done");
    assert_eq!(sheet(&mut s), None);
    assert!(!s.model(|d| d.learn.is_active()));
    // Return presses Done too.
    open(&mut s);
    s.cx.simulate_keystrokes("enter");
    assert_eq!(sheet(&mut s), None);
    assert!(!s.model(|d| d.learn.is_active()));
}

/// Esc with the sheet up closes the sheet; a waiting learn still waits
/// (the learn's Esc defers to the sheet).
#[gpui::test]
fn escape_closes_the_sheet_and_not_the_learn(cx: &mut TestAppContext) {
    let (mut s, core) = setup(cx, two_maps());
    s.fake.set_surface_state(SurfaceStateRow {
        learning: Some("Fader \u{00B7} Synth".into()),
        ..SurfaceStateRow::default()
    });
    s.right_click("slider", "Fader of Synth");
    s.click("menuitem", "MIDI Learn");
    assert!(s.model(|d| d.learn.is_learning()));
    open(&mut s);
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(sheet(&mut s), None);
    assert!(!s.model(|d| d.learn.maps_open()));
    assert_eq!(core.learn_cancels(), 0);
    assert!(s.model(|d| d.learn.is_learning()));
    // The next Esc is the learn's.
    s.cx.simulate_keystrokes("escape");
    assert_eq!(core.learn_cancels(), 1);
}

/// The sheet opens in the window that holds the mixer: the Mixer
/// window's menu opens it there, and one is open at a time.
#[gpui::test]
fn the_mixer_window_opens_it_in_itself(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, two_maps());
    s.cx.dispatch_action(RunCommand::new("window.mixer"));
    s.cx.run_until_parked();
    let doc = s.doc();
    let w: WindowHandle<DocumentWindow> = s
        .app
        .read_with(&s.cx, |a, _| {
            a.mixer_window_of(doc).and_then(|w| w.document_window())
        })
        .expect("a Mixer window");
    let mut mcx = VisualTestContext::from_window(w.into(), &s.cx);
    mcx.run_until_parked();
    let find = |mcx: &mut VisualTestContext, role: &str, label: &str| {
        w.read_with(mcx, |v, _| {
            v.registry()
                .borrow()
                .elements()
                .iter()
                .find(|e| e.role == role && e.label == label)
                .map(|e| e.bounds.center())
        })
        .unwrap()
    };
    let name = find(&mut mcx, "text", "Synth").expect("Synth in the Mixer window");
    mcx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Right,
        position: name,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    mcx.simulate_event(gpui::MouseUpEvent {
        button: gpui::MouseButton::Right,
        position: name,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    mcx.run_until_parked();
    let item = find(&mut mcx, "menuitem", ITEM).expect("the item");
    mcx.simulate_click(item, Modifiers::none());
    mcx.run_until_parked();
    let label = w.read_with(&mcx, |v, _| v.sheet_label()).unwrap();
    assert_eq!(label.as_deref(), Some(TITLE));
    assert_eq!(sheet(&mut s), None, "not in the document window");
    // The document window's menu opens no second one.
    open(&mut s);
    assert_eq!(sheet(&mut s), None);
    // Closing the Mixer window with the sheet up ends its polling.
    w.update(&mut mcx, |_, window, _| window.remove_window())
        .unwrap();
    s.cx.run_until_parked();
    assert!(!s.model(|d| d.learn.is_active()));
}
