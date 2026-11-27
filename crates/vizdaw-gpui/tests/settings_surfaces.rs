//! Settings › Surfaces (slice 9d Task 7; Swift `SurfacesSettingsView`):
//! the units with their state and problem lines, Remove and its alert,
//! the learned commands, the Add Surface sheet, and the 2 Hz poll that
//! reads the core off the UI thread, on the scripted core (no CoreMIDI
//! client, no `surfaces.json`).

use std::sync::Arc;
use std::time::Duration;

use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use vizdaw_app::surface_settings::{
    KONTROL_HELP, NO_COMMAND_MAPS, NO_DESTINATIONS, NO_KONTROL, NO_SOURCES, NO_SURFACES, XT_HELP,
};
use vizdaw_app::testing::{FakeCore, FakeSession, ReadHold, demo_fake};
use vizdaw_app::{DocumentStore, Prefs};
use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::surfaces::{
    CommandMapRow, LearnKindRow, LearnSourceRow, SurfaceKindRow, SurfacePortsRow, SurfaceRow,
    SurfaceStateRow,
};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::settings_window::{SURFACES_WIDTH, SettingsTab, SettingsWindow, open_settings};
use vizdaw_gpui::{AppState, DocumentWindow};

const ADD: &str = "Add Surface\u{2026}";
const SHEET: &str = "Add Surface";

struct Setup {
    core: Arc<FakeCore>,
    fake: Arc<FakeSession>,
    app: gpui::Entity<AppState>,
    doc: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn unit(id: u32, kind: SurfaceKindRow, position: u8, connected: bool, online: bool) -> SurfaceRow {
    SurfaceRow {
        id,
        kind,
        input: "In A".into(),
        output: "Out A".into(),
        position,
        connected,
        online,
        ignored: 0,
    }
}

fn ports(sources: &[&str], destinations: &[&str]) -> SurfacePortsRow {
    SurfacePortsRow {
        sources: sources.iter().map(|s| (*s).to_owned()).collect(),
        destinations: destinations.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn pads(command: &str) -> CommandMapRow {
    CommandMapRow {
        source: LearnSourceRow {
            port: "Pads".into(),
            kind: LearnKindRow::Note,
            channel: 9,
            number: 36,
        },
        text: "Pads \u{00B7} Note C2 ch 10".into(),
        command: command.into(),
    }
}

fn refused(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// The app over a scripted core, then Settings on `tab`; `script` runs
/// before launch.
fn setup_on(
    cx: &mut TestAppContext,
    tab: SettingsTab,
    script: impl FnOnce(&FakeCore, &FakeSession),
) -> Setup {
    let core = Arc::new(FakeCore::new());
    let f = demo_fake();
    script(&core, &f);
    let fake = Arc::new(f);
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(core.clone());
    let (app, doc) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        app.update(cx, |s, _| s.set_prefs(Prefs::in_memory()));
        let doc = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, doc)
    });
    let vcx = VisualTestContext::from_window(doc.into(), cx);
    vcx.run_until_parked();
    let mut s = Setup {
        core,
        fake,
        app,
        doc,
        cx: vcx,
    };
    s.cx.update(|_, cx| open_settings(Some(tab), cx));
    s.settle();
    s
}

fn setup(cx: &mut TestAppContext, script: impl FnOnce(&FakeCore, &FakeSession)) -> Setup {
    setup_on(cx, SettingsTab::Surfaces, script)
}

impl Setup {
    /// Lets a poll's off-thread read land (a thread answers into a slot
    /// the window looks in every 20 ms of the app's clock; the poll runs
    /// every 500 ms).
    fn settle(&mut self) {
        for _ in 0..40 {
            self.cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(2));
            self.cx.executor().advance_clock(Duration::from_millis(20));
        }
        self.cx.run_until_parked();
    }

    /// Advances the app's clock by `ms` in 20 ms steps.
    fn advance(&mut self, ms: u64) {
        for _ in 0..ms / 20 {
            self.cx.run_until_parked();
            self.cx.executor().advance_clock(Duration::from_millis(20));
        }
        self.cx.run_until_parked();
    }

    fn handle(&mut self) -> WindowHandle<SettingsWindow> {
        self.app
            .read_with(&self.cx, |a, _| a.settings_window)
            .expect("the settings window")
    }

    fn settings_cx(&mut self) -> VisualTestContext {
        let h = self.handle();
        VisualTestContext::from_window(h.into(), &self.cx)
    }

    fn elements(&mut self) -> Vec<Element> {
        let h = self.handle();
        let mut vcx = self.settings_cx();
        vcx.run_until_parked();
        vcx.update(|w, _| w.refresh());
        vcx.run_until_parked();
        h.read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }

    fn find(&mut self, role: &str, label: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    fn all(&mut self, role: &str, label: &str) -> Vec<Element> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == role && e.label == label)
            .collect()
    }

    fn click_at(&mut self, e: &Element) {
        let mut vcx = self.settings_cx();
        vcx.simulate_click(e.bounds.center(), Modifiers::none());
        vcx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let e = self
            .find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label}: {:#?}", self.texts()));
        self.click_at(&e);
    }

    /// The last `role` `label`: the sheet's, over the tab's.
    fn click_last(&mut self, role: &str, label: &str) {
        let e = self
            .all(role, label)
            .pop()
            .unwrap_or_else(|| panic!("no {role} {label}"));
        self.click_at(&e);
    }

    fn has_text(&mut self, t: &str) -> bool {
        self.find("text", t).is_some()
    }

    fn texts(&mut self) -> Vec<String> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "text")
            .map(|e| e.label)
            .collect()
    }

    fn sheet(&mut self) -> Option<String> {
        let h = self.handle();
        h.read_with(&self.cx, |v, _| v.sheet_label()).unwrap()
    }

    fn menu_value(&mut self, label: &str) -> Option<String> {
        self.find("menu", label).and_then(|e| e.value)
    }

    /// Picks `item` from the sheet's picker `menu`.
    fn pick(&mut self, menu: &str, item: &str) {
        self.click("menu", menu);
        self.click("menuitem", item);
    }

    fn press(&mut self, keys: &str) {
        let mut vcx = self.settings_cx();
        vcx.simulate_keystrokes(keys);
        vcx.run_until_parked();
    }

    fn has_surfaces(&mut self) -> bool {
        self.app.read_with(&self.cx, |a, _| a.store.has_surfaces())
    }
}

#[gpui::test]
fn the_tab_lists_units_and_learned_commands(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_, _| {});
    assert!(s.has_text("Surfaces"));
    assert!(s.has_text(NO_SURFACES), "{:#?}", s.texts());
    assert!(s.has_text("Commands Learned to MIDI"));
    assert!(s.has_text(NO_COMMAND_MAPS));
    assert!(s.find("button", ADD).is_some());
    // 560 wide (`S/SurfacesSettingsView.swift:69`).
    assert_eq!(SURFACES_WIDTH, 560.);
    assert_eq!(SettingsTab::Surfaces.width(), 560.);
    let size = s.settings_cx().update(|w, _| w.viewport_size());
    assert_eq!(f32::from(size.width), 560.);
    // No placeholder line any more.
    assert!(!s.texts().iter().any(|t| t.ends_with("later version.")));

    s.core
        .set_surface_rows(vec![unit(0, SurfaceKindRow::McuXt, 2, true, false)]);
    s.core.set_command_maps(vec![pads("transport.play")]);
    s.settle();
    assert!(!s.has_text(NO_SURFACES));
    assert!(s.has_text("MCU XT-compatible \u{00B7} position 2"));
    assert!(s.has_text("In A \u{2192} Out A"));
    assert!(s.has_text("Connected \u{00B7} not answering"));
    assert!(s.find("button", "Remove").is_some());
    assert!(!s.has_text(NO_COMMAND_MAPS));
    assert!(s.has_text("Pads \u{00B7} Note C2 ch 10"));
    // The command's registry title.
    assert!(s.has_text("Play"), "{:#?}", s.texts());
    // A command the registry does not know shows its id.
    s.core.set_command_maps(vec![pads("no.such.command")]);
    s.settle();
    assert!(s.has_text("no.such.command"));
}

#[gpui::test]
fn a_problem_line_shows_under_a_silent_unit_only(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c, _| {
        let mut quiet = unit(1, SurfaceKindRow::Hui, 0, true, false);
        quiet.input = "In B".into();
        c.set_surface_rows(vec![unit(0, SurfaceKindRow::Mcu, 0, true, true), quiet]);
        c.set_surfaces_state(SurfaceStateRow {
            problems: vec![
                "surface 0: an old line".into(),
                "surface 1: no answer on In B".into(),
                "surfaces.json could not be saved: disk full".into(),
            ],
            ..Default::default()
        });
    });
    assert!(s.has_text("Connected"));
    assert!(s.has_text("no answer on In B"), "{:#?}", s.texts());
    // The answering unit's left-over line is not shown.
    assert!(!s.has_text("an old line"));
    assert!(!s.has_text("surface 0: an old line"));
    // A line that names no row shows under the list.
    assert!(s.has_text("surfaces.json could not be saved: disk full"));
    // Under its row's status.
    let status = s.find("text", "Connected \u{00B7} not answering").unwrap();
    let why = s.find("text", "no answer on In B").unwrap();
    assert!(why.bounds.origin.y > status.bounds.origin.y);
}

#[gpui::test]
fn remove_removes_and_a_refusal_is_an_alert(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c, _| {
        c.set_surface_rows(vec![unit(0, SurfaceKindRow::Mcu, 0, true, true)]);
    });
    // As the launch reads it (`run`; `install` alone does not).
    s.app
        .update(&mut s.cx, |a, _| a.store.refresh_claimed_ports());
    assert!(s.has_surfaces());
    s.click("button", "Remove");
    assert_eq!(s.core.surfaces_removed(), vec![0]);
    assert!(s.has_text(NO_SURFACES));
    // The pickers' claimed ports and the surface tick follow (as the
    // channel's `surface.remove`).
    assert!(!s.has_surfaces());

    s.core
        .set_surface_rows(vec![unit(3, SurfaceKindRow::Hui, 0, false, false)]);
    s.settle();
    s.core
        .refuse_next("surface_remove", refused("the core is busy"));
    s.click("button", "Remove");
    let vcx = s.settings_cx();
    vcx.run_until_parked();
    assert!(vcx.has_pending_prompt(), "a refusal is an alert");
    let (title, detail) = vcx.pending_prompt().expect("the alert");
    assert_eq!(title, "Remove Surface Failed");
    assert_eq!(detail, "the core is busy");
    vcx.simulate_prompt_answer("OK");
    vcx.run_until_parked();
    assert!(s.has_text("Disconnected"), "the row stays");
}

#[gpui::test]
fn add_surface_opens_the_sheet_with_the_first_ports(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c, _| {
        c.set_surface_ports(ports(&["In A", "In B"], &["Out A", "Out B"]));
    });
    assert!(!s.has_surfaces());
    s.click("button", ADD);
    assert_eq!(s.sheet().as_deref(), Some(SHEET));
    let card = s.find("sheet", SHEET).expect("the sheet");
    // 420 wide (`:154`); the tree's frame is inside its 1-point border.
    assert_eq!(f32::from(card.bounds.size.width), 418.);
    assert!(s.has_text(SHEET), "its title");
    assert_eq!(s.menu_value("Type").as_deref(), Some("MCU-compatible"));
    assert_eq!(s.menu_value("Input").as_deref(), Some("In A"));
    assert_eq!(s.menu_value("Output").as_deref(), Some("Out A"));
    // The four types, in Swift's order.
    s.click("menu", "Type");
    let kinds: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(
        kinds,
        [
            "MCU-compatible",
            "MCU XT-compatible",
            "HUI-compatible",
            "Host-integration keyboard (DAW mode)"
        ]
    );
    s.click("menuitem", "MCU-compatible");
    // Cancel closes it; nothing is added.
    s.click_last("button", "Cancel");
    assert_eq!(s.sheet(), None);
    assert!(s.core.surfaces_added().is_empty());
    // Again, with the second ports, and Add.
    s.click("button", ADD);
    s.pick("Input", "In B");
    s.pick("Output", "Out B");
    assert_eq!(s.menu_value("Input").as_deref(), Some("In B"));
    s.click_last("button", "Add");
    assert_eq!(s.sheet(), None);
    assert_eq!(
        s.core.surfaces_added(),
        vec![(SurfaceKindRow::Mcu, "In B".into(), "Out B".into(), None)]
    );
    assert!(s.has_text("MCU-compatible"));
    assert!(s.has_text("In B \u{2192} Out B"));
    // The pickers' claimed ports and the surface tick follow (as the
    // channel's `surface.add`).
    assert!(s.has_surfaces());
    let doc = s.doc.read_with(&s.cx, |v, _| v.doc()).unwrap();
    let interval = s
        .app
        .read_with(&s.cx, |a, _| a.surface_interval(doc))
        .unwrap();
    assert_eq!(interval, vizdaw_gpui::app_state::FAST_POLL);
}

#[gpui::test]
fn the_kontrol_type_fills_its_daw_ports_and_shows_the_help(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c, _| {
        c.set_surface_ports(ports(&["In A", "Keys DAW"], &["Out A", "Keys DAW"]));
        c.set_kontrol_ports(ports(&["Keys DAW"], &["Keys DAW"]));
    });
    s.click("button", ADD);
    assert!(!s.has_text(KONTROL_HELP));
    s.pick("Type", "Host-integration keyboard (DAW mode)");
    assert_eq!(
        s.menu_value("Type").as_deref(),
        Some("Host-integration keyboard (DAW mode)")
    );
    assert_eq!(s.menu_value("Input").as_deref(), Some("Keys DAW"));
    assert_eq!(s.menu_value("Output").as_deref(), Some("Keys DAW"));
    assert!(s.has_text(KONTROL_HELP), "{:#?}", s.texts());
    assert!(!s.has_text(NO_KONTROL));
    // No DAW port: the note, over the help.
    s.core.set_kontrol_ports(SurfacePortsRow::default());
    s.pick("Type", "MCU-compatible");
    assert!(!s.has_text(KONTROL_HELP));
    s.pick("Type", "Host-integration keyboard (DAW mode)");
    let note = s.find("text", NO_KONTROL).expect("the note");
    let help = s.find("text", KONTROL_HELP).expect("the help");
    assert!(help.bounds.origin.y > note.bounds.origin.y);
    s.click_last("button", "Add");
    assert_eq!(
        s.core.surfaces_added(),
        vec![(
            SurfaceKindRow::KontrolMk3,
            "Keys DAW".into(),
            "Keys DAW".into(),
            None
        )]
    );
}

#[gpui::test]
fn the_xt_type_shows_the_position_stepper(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_, _| {});
    s.click("button", ADD);
    assert!(!s.has_text("Position: 1"));
    s.pick("Type", "MCU XT-compatible");
    assert!(s.has_text("Position: 1"), "{:#?}", s.texts());
    assert!(s.has_text(XT_HELP));
    s.click("button", "Increase Position");
    assert!(s.has_text("Position: 2"));
    s.click("button", "Increase Position");
    assert!(s.has_text("Position: 3"));
    assert!(!s.find("button", "Increase Position").unwrap().enabled);
    for _ in 0..3 {
        s.click("button", "Decrease Position");
    }
    assert!(s.has_text("Position: 0"));
    assert!(!s.find("button", "Decrease Position").unwrap().enabled);
    s.click("button", "Increase Position");
    s.click("button", "Increase Position");
    // Return presses Add.
    s.press("enter");
    assert_eq!(s.sheet(), None);
    assert_eq!(
        s.core.surfaces_added(),
        vec![(
            SurfaceKindRow::McuXt,
            "In A".into(),
            "Out A".into(),
            Some(2)
        )]
    );
}

#[gpui::test]
fn a_refused_add_shows_in_the_sheet_and_it_stays(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_, _| {});
    s.click("button", ADD);
    s.core
        .fail_surface_add(refused("In A is in use by surface 2"));
    s.click_last("button", "Add");
    assert_eq!(s.sheet().as_deref(), Some(SHEET), "it stays open");
    assert!(
        s.has_text("In A is in use by surface 2"),
        "{:#?}",
        s.texts()
    );
    assert!(s.core.surfaces_added().is_empty());
    assert!(!s.has_surfaces());
    // Esc presses Cancel.
    s.press("escape");
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn no_ports_disable_add(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c, _| c.set_surface_ports(SurfacePortsRow::default()));
    s.click("button", ADD);
    assert_eq!(s.menu_value("Input").as_deref(), Some(NO_SOURCES));
    assert_eq!(s.menu_value("Output").as_deref(), Some(NO_DESTINATIONS));
    assert_eq!(NO_SOURCES, "No MIDI sources");
    assert_eq!(NO_DESTINATIONS, "No MIDI destinations");
    let add = s.all("button", "Add").pop().expect("Add");
    assert!(!add.enabled);
    // Return does nothing; the sheet stays.
    s.press("enter");
    assert_eq!(s.sheet().as_deref(), Some(SHEET));
    assert!(s.core.surfaces_added().is_empty());
}

/// Review Focus 1.
#[gpui::test]
fn a_port_that_vanishes_reads_disconnected_at_the_next_poll(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c, _| {
        c.set_surface_rows(vec![unit(0, SurfaceKindRow::Mcu, 0, true, true)]);
    });
    assert!(s.has_text("Connected"));
    s.click("button", ADD);
    assert_eq!(s.menu_value("Input").as_deref(), Some("In A"));
    // The port goes.
    s.core
        .set_surface_rows(vec![unit(0, SurfaceKindRow::Mcu, 0, false, true)]);
    s.core.set_surface_ports(SurfacePortsRow::default());
    s.settle();
    assert!(s.has_text("Disconnected"), "{:#?}", s.texts());
    assert!(!s.has_text("Connected"));
    // The open sheet keeps its pick.
    assert_eq!(s.sheet().as_deref(), Some(SHEET));
    assert_eq!(s.menu_value("Input").as_deref(), Some("In A"));
    assert_eq!(s.menu_value("Output").as_deref(), Some("Out A"));
    // Add shows the core's refusal in the sheet, which stays.
    s.core
        .fail_surface_add(refused("no MIDI source named In A"));
    s.click_last("button", "Add");
    assert_eq!(s.sheet().as_deref(), Some(SHEET));
    assert!(s.has_text("no MIDI source named In A"));
}

/// Review Focus 4.
#[gpui::test]
fn the_surfaces_poll_reads_off_thread_one_at_a_time(cx: &mut TestAppContext) {
    let mut s = setup_on(cx, SettingsTab::Appearance, |_, _| {});
    let reads = |s: &Setup| s.core.call_count("surface_list");
    let before = reads(&s);
    s.advance(1000);
    assert_eq!(reads(&s), before, "no poll off the tab");

    let hold = Arc::new(ReadHold::default());
    s.core.hold_surface_reads(hold.clone());
    s.click("tab", "Surfaces");
    assert!(hold.wait_started(Duration::from_secs(5)), "the read starts");
    let ui = std::thread::current().id();
    assert_ne!(
        s.core.read_threads("surface_list").last(),
        Some(&ui),
        "off the UI thread"
    );
    // Two poll periods: the held read is still out, so no second starts.
    s.advance(1000);
    assert_eq!(reads(&s), before + 1);
    // Leaving and coming back while it hangs starts no second thread.
    s.click("tab", "Appearance");
    s.click("tab", "Surfaces");
    s.advance(500);
    assert_eq!(reads(&s), before + 1);
    // Released: it lands, and the next poll starts the next read.
    hold.release();
    s.settle();
    s.advance(600);
    s.settle();
    assert!(reads(&s) >= before + 2);

    // An unchanged read redraws nothing.
    let notes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let window = s.handle();
    let entity = window.entity(&s.cx).unwrap();
    let counted = notes.clone();
    let _watch = s.cx.update(|_, cx| {
        cx.observe(&entity, move |_, _| {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
    });
    let at = reads(&s);
    s.settle();
    s.advance(600);
    s.settle();
    assert!(reads(&s) > at, "the poll still reads");
    assert_eq!(
        notes.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "an unchanged read does not notify"
    );
    // A changed one does.
    s.core
        .set_surface_rows(vec![unit(0, SurfaceKindRow::Mcu, 0, true, true)]);
    s.settle();
    assert!(notes.load(std::sync::atomic::Ordering::SeqCst) > 0);

    // Leaving the tab stops the poll: no read starts in the next second.
    s.click("tab", "Appearance");
    s.settle();
    let left = reads(&s);
    s.advance(1000);
    s.settle();
    assert_eq!(reads(&s), left);
}

#[gpui::test]
fn closing_the_window_stops_the_surfaces_poll(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_, _| {});
    let mut vcx = s.settings_cx();
    vcx.simulate_keystrokes("secondary-w");
    vcx.run_until_parked();
    s.cx.run_until_parked();
    assert!(s.app.read_with(&s.cx, |a, _| a.settings_window.is_none()));
    let at = s.core.call_count("surface_list");
    s.advance(1000);
    s.cx.run_until_parked();
    assert_eq!(s.core.call_count("surface_list"), at);
}

#[gpui::test]
fn removing_a_learned_command_reaches_the_core(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c, _| c.set_command_maps(vec![pads("transport.play")]));
    s.click("button", "Remove");
    assert!(
        s.core
            .calls()
            .contains(&"remove_command_map(transport.play)".to_owned())
    );
    assert!(s.has_text(NO_COMMAND_MAPS));
    // A refusal is an alert.
    s.core.set_command_maps(vec![pads("transport.play")]);
    s.settle();
    s.core.fail_remove_command_map(refused("not learned"));
    s.click("button", "Remove");
    let vcx = s.settings_cx();
    vcx.run_until_parked();
    let (title, detail) = vcx.pending_prompt().expect("the alert");
    assert_eq!(title, "Remove MIDI Learn Failed");
    assert_eq!(detail, "not learned");
}

/// Esc on the Add Surface sheet closes the sheet, not a waiting learn.
#[gpui::test]
fn escape_closes_the_add_sheet_not_a_waiting_learn(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_, f| {
        f.set_surface_state(SurfaceStateRow {
            learning: Some("Fader \u{00B7} Synth".into()),
            ..Default::default()
        });
    });
    let doc = s.doc.read_with(&s.cx, |v, _| v.doc()).unwrap();
    s.app.update(&mut s.cx, |a, _| {
        a.store.with_document(doc, |d, _| {
            d.learn_target(AutomationTargetRow::StripFader { strip: 1 })
        });
    });
    assert!(s.app.read_with(&s.cx, |a, _| a.store.learning_document()) == Some(doc));
    assert!(!s.fake.learn_targets().is_empty());
    s.click("button", ADD);
    assert_eq!(s.sheet().as_deref(), Some(SHEET));
    s.press("escape");
    assert_eq!(s.sheet(), None, "the sheet closes");
    assert_eq!(s.core.learn_cancels(), 0, "the learn still waits");
    assert_eq!(
        s.app.read_with(&s.cx, |a, _| a.store.learning_document()),
        Some(doc)
    );
}
