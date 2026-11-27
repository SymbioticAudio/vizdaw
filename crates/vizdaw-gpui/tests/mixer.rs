//! The mixer view (Swift `MixerView`, `ChannelStrips`, `ChannelStrip`) in
//! the Mix view's centre and the bottom pane, driven through GPUI's test
//! platform with the scripted session: a strip per audio strip and master
//! after them, the Swift labels and values, drags, toggles, menus, Add
//! Bus, rename, selection, the placements' lives, and only the strips in
//! view recorded.

use std::sync::Arc;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::editor::{MainView, PaneSide, PaneView};
use vizdaw_app::mixer::{MeterLevel, MixerPlacement, StripEntry};
use vizdaw_app::testing::{FakeSession, mixer_fake, strip_row};
use vizdaw_ffi::automation::{AutomationModeRow, GesturePhaseRow};
use vizdaw_ffi::edit::{EditCommand, LaneEdit, MonitorModeRow};
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::StripKindRow;
use vizdaw_gpui::controls;
use vizdaw_gpui::mixer_view::{ADD_BUS, MixerView, NAME, POLL_EVERY};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand, WindowKind};

/// Audio 1 (5, path 1: Groove plays through it), Audio 2 (6, path 2:
/// Chords), Synth (1), Bus 1 (2), then Master (0).
fn fake() -> FakeSession {
    let f = mixer_fake();
    let mut groove = vizdaw_app::testing::lane_row(30, "Groove", false);
    groove.audio_out = Some(1);
    let mut chords = vizdaw_app::testing::lane_row(31, "Chords", false);
    chords.audio_out = Some(2);
    chords.monitor = MonitorModeRow::Off;
    f.set_lanes(3, vec![groove, chords]);
    f.edit_mixer(|m| {
        let mut a1 = strip_row(5, "Audio 1", StripKindRow::AudioPath { path: 1 });
        a1.kind_label = "Audio: Out 1".into();
        let a2 = strip_row(6, "Audio 2", StripKindRow::AudioPath { path: 2 });
        m.strips.insert(0, a1);
        m.strips.insert(1, a2);
    });
    f
}

/// `n` audio strips (10, 11, …) and master.
fn many(n: u64) -> FakeSession {
    let f = mixer_fake();
    f.edit_mixer(|m| {
        m.strips = (0..n)
            .map(|i| {
                strip_row(
                    10 + i,
                    &format!("Audio {}", i + 1),
                    StripKindRow::AudioPath { path: i + 1 },
                )
            })
            .chain([strip_row(0, "Master", StripKindRow::Master)])
            .collect();
        m.buses.clear();
    });
    f
}

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext, f: FakeSession) -> Setup {
    let fake = Arc::new(f);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(mixer_fake()))),
    );
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
    s.run_menu("ui.view_mix");
    s.fake.clear_gestures();
    s.fake.clear_calls();
    s
}

fn touch(v: Option<f64>) -> GesturePhaseRow {
    GesturePhaseRow::Touch { value: v }
}

const RELEASE: GesturePhaseRow = GesturePhaseRow::Release;

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

    fn run_menu(&mut self, id: &str) {
        self.cx.dispatch_action(RunCommand::new(id.to_owned()));
        self.cx.run_until_parked();
    }

    fn placements(&mut self) -> Vec<MixerPlacement> {
        self.model(|d| {
            d.mixer()
                .map(|m| m.placements().iter().copied().collect())
                .unwrap_or_default()
        })
    }

    fn view(&mut self, p: MixerPlacement) -> Option<Entity<MixerView>> {
        self.window
            .read_with(&self.cx, |v, _| v.mixer_view(p))
            .unwrap()
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

    fn value(&mut self, role: &str, label: &str) -> Option<String> {
        self.element(role, label).value
    }

    fn faders(&mut self) -> Vec<Element> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "slider" && e.label.starts_with("Fader of "))
            .collect()
    }

    fn fader_labels(&mut self) -> Vec<String> {
        self.faders().into_iter().map(|e| e.label).collect()
    }

    fn click_at(&mut self, at: Point<Pixels>, modifiers: Modifiers) {
        self.cx.simulate_click(at, modifiers);
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.click_at(at, Modifiers::none());
    }

    fn double_click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        for count in [1, 2] {
            self.cx.simulate_event(MouseDownEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::none(),
                click_count: count,
                first_mouse: false,
            });
            self.cx.simulate_event(MouseUpEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::none(),
                click_count: count,
            });
        }
        self.cx.run_until_parked();
    }

    /// The middle of `name`'s fader cap column.
    fn fader_at(&mut self, name: &str) -> Point<Pixels> {
        let b = self.element("slider", &format!("Fader of {name}")).bounds;
        point(
            b.origin.x + px((controls::fader::SCALE_WIDTH + controls::fader::COLUMN / 2.) as f32),
            b.center().y,
        )
    }

    fn down(&mut self, at: Point<Pixels>) {
        self.cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    fn move_to(&mut self, at: Point<Pixels>) {
        self.cx.simulate_event(MouseMoveEvent {
            position: at,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
        self.cx.run_until_parked();
    }

    fn up(&mut self, at: Point<Pixels>) {
        self.cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    fn gain(&mut self, strip: u64) -> f64 {
        self.model(|d| {
            let m = d.mixer().unwrap();
            m.gain_db(m.strip(strip).unwrap())
        })
    }

    fn of(&self, t: T) -> Vec<GesturePhaseRow> {
        self.fake
            .gestures()
            .into_iter()
            .filter(|g| g.0 == t)
            .map(|g| g.1)
            .collect()
    }

    fn right_click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.cx.simulate_event(MouseDownEvent {
            button: MouseButton::Right,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            button: MouseButton::Right,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    /// The open menus' items: (title, enabled, checked).
    fn items(&mut self) -> Vec<(String, bool, bool)> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "menuitem")
            .map(|e| (e.label, e.enabled, e.selected))
            .collect()
    }

    fn titles(&mut self) -> Vec<String> {
        self.items().into_iter().map(|i| i.0).collect()
    }

    fn enabled_item(&mut self, title: &str) -> bool {
        self.element("menuitem", title).enabled
    }

    /// A click on the menu's backdrop closes it.
    fn close_menus(&mut self) {
        if !self.titles().is_empty() {
            self.click_at(point(px(2.), px(2.)), Modifiers::none());
        }
        assert!(self.titles().is_empty());
    }

    fn mixer_calls(&self) -> Vec<(String, String)> {
        self.fake
            .run_mixers()
            .into_iter()
            .map(|(id, args)| (id, format!("{args:?}")))
            .collect()
    }

    fn scroll(&mut self, at: Point<Pixels>, dx: f32, dy: f32) {
        self.cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            ..Default::default()
        });
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn the_mix_view_shows_a_strip_per_track_and_the_master(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    assert_eq!(
        s.fader_labels(),
        [
            "Fader of Audio 1",
            "Fader of Audio 2",
            "Fader of Synth",
            "Fader of Bus 1",
            "Fader of Master"
        ]
    );
    let faders = s.faders();
    let xs: Vec<f32> = faders
        .iter()
        .map(|e| f32::from(e.bounds.origin.x))
        .collect();
    // A strip and a gap apart; master past the divider's own slot.
    assert_eq!(xs[1] - xs[0], 126.);
    assert_eq!(xs[4] - xs[3], 133.);
    // Every fader at one y (the strips share their rows).
    assert!(
        faders
            .iter()
            .all(|e| e.bounds.origin.y == faders[0].bounds.origin.y)
    );
    let group = s.element("group", "Mixer").bounds;
    assert!(group.contains(&faders[0].bounds.center()));
    assert!(s.find("button", ADD_BUS).is_some());
    assert!(
        s.elements()
            .iter()
            .any(|e| e.role == "text" && e.label.starts_with("DSP 0%")),
        "the load text"
    );
    assert_eq!(s.placements(), [MixerPlacement::View]);
    s.run_menu("ui.view_arrange");
    assert!(s.faders().is_empty());
    assert!(s.placements().is_empty(), "the Mix view went");
}

#[gpui::test]
fn strip_labels_and_values_match_swift(cx: &mut TestAppContext) {
    let f = fake();
    f.edit_mixer(|m| {
        m.strips[0].gain_db = -4.2;
        m.strips[0].pan = -0.24;
        m.strips[0].automation = AutomationModeRow::Touch;
    });
    let mut s = setup(cx, f);
    assert_eq!(
        s.value("slider", "Fader of Audio 1").as_deref(),
        Some("\u{2212}4.2 dB")
    );
    assert_eq!(
        s.value("textfield", "Level of Audio 1"),
        Some(StripEntry::level_text(-4.2))
    );
    assert_eq!(
        s.value("slider", "Pan knob of Audio 1").as_deref(),
        Some("L 24")
    );
    assert_eq!(
        s.value("textfield", "Pan of Audio 1").as_deref(),
        Some("L 24")
    );
    assert_eq!(
        s.value("slider", "Fader of Synth").as_deref(),
        Some("0.0 dB")
    );
    assert_eq!(s.value("slider", "Pan knob of Synth").as_deref(), Some("C"));
    assert_eq!(
        s.value("menu", "Output of Audio 1").as_deref(),
        Some("\u{2192} Master")
    );
    assert_eq!(s.value("button", "Mute Audio 1").as_deref(), Some("off"));
    assert_eq!(s.value("button", "Solo Audio 1").as_deref(), Some("off"));
    assert_eq!(
        s.value("button", "Record arm Audio 1").as_deref(),
        Some("off")
    );
    assert!(s.element("button", "Record arm Audio 1").enabled);
    // Groove monitors on Auto: lit; Chords is off.
    assert_eq!(
        s.value("button", "Input monitor Audio 1").as_deref(),
        Some("on")
    );
    assert_eq!(
        s.value("button", "Input monitor Audio 2").as_deref(),
        Some("off")
    );
    assert_eq!(
        s.value("menu", "Automation of Audio 1").as_deref(),
        Some("Touch")
    );
    assert_eq!(
        s.value("menu", "Automation of Synth").as_deref(),
        Some("Read")
    );
    assert!(s.find("text", "Audio 1").is_some());
    assert!(s.find("text", "Audio: Out 1").is_some(), "the kind");
    // R and I are for audio tracks.
    assert!(!s.element("button", "Record arm Synth").enabled);
    assert!(!s.element("button", "Input monitor Synth").enabled);
    // Master: mute, no solo (its place kept), no pan, no output.
    assert!(s.find("button", "Mute Master").is_some());
    assert!(s.find("button", "Solo Master").is_none());
    assert!(s.find("slider", "Pan knob of Master").is_none());
    assert!(s.find("textfield", "Pan of Master").is_none());
    assert!(s.find("menu", "Output of Master").is_none());
    assert!(!s.element("button", "Record arm Master").enabled);
    let m = s.element("button", "Mute Master").bounds.origin.x;
    let r = s.element("button", "Record arm Master").bounds.origin.x;
    let bus_m = s.element("button", "Mute Bus 1").bounds.origin.x;
    let bus_r = s.element("button", "Record arm Bus 1").bounds.origin.x;
    assert_eq!(r - m, bus_r - bus_m, "the solo's place is kept");
    assert_eq!(
        s.element("button", "Mute Master").bounds.origin.y,
        s.element("button", "Mute Audio 1").bounds.origin.y
    );
}

#[gpui::test]
fn a_fader_drag_moves_the_strip(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let at = s.fader_at("Audio 1");
    s.down(at);
    assert!(s.fake.gestures().is_empty(), "a bare press sends nothing");
    s.move_to(point(at.x, at.y - px(2.)));
    s.move_to(point(at.x, at.y - px(40.)));
    s.up(point(at.x, at.y - px(40.)));
    let phases = s.of(T::StripFader { strip: 5 });
    assert_eq!(phases.first(), Some(&touch(None)));
    assert_eq!(phases.last(), Some(&RELEASE));
    assert_eq!(phases.iter().filter(|p| **p == RELEASE).count(), 1);
    assert!(s.gain(5) > 0.0, "the strip moved up: {}", s.gain(5));
    let shown = StripEntry::level_text(s.gain(5));
    assert_eq!(s.value("slider", "Fader of Audio 1"), Some(shown));
    // A press on the fader does not select its strip.
    assert!(s.model(|d| d.selected_strips().is_empty()));
}

#[gpui::test]
fn mute_and_solo_toggle(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("button", "Mute Audio 1");
    assert_eq!(s.value("button", "Mute Audio 1").as_deref(), Some("on"));
    assert_eq!(
        s.of(T::StripMute { strip: 5 }),
        [
            touch(Some(1.0)),
            GesturePhaseRow::Value { value: 1.0 },
            RELEASE
        ]
    );
    s.click("button", "Solo Bus 1");
    assert_eq!(s.value("button", "Solo Bus 1").as_deref(), Some("on"));
    s.click("button", "Mute Audio 1");
    assert_eq!(s.value("button", "Mute Audio 1").as_deref(), Some("off"));
    s.click("button", "Mute Master");
    assert_eq!(s.value("button", "Mute Master").as_deref(), Some("on"));
}

#[gpui::test]
fn record_arm_and_monitor_act_on_the_input_track(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("button", "Record arm Audio 1");
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: 3,
                lane: 30,
                mute: None,
                solo: None,
                arm: Some(true),
            }
        })
    );
    assert_eq!(
        s.value("button", "Record arm Audio 1").as_deref(),
        Some("on")
    );
    s.click("button", "Input monitor Audio 1");
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetMonitor {
                seq: 3,
                lane: 30,
                monitor: MonitorModeRow::Input,
            }
        })
    );
    let n = s.fake.edits().len();
    s.click("button", "Record arm Synth");
    assert_eq!(s.fake.edits().len(), n, "disabled: nothing is sent");
}

#[gpui::test]
fn solo_dims_the_others(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let look = |s: &mut Setup| {
        let v = s.view(MixerPlacement::View).unwrap();
        v.read_with(&s.cx, |v, _| v.drawn().to_vec())
    };
    assert!(look(&mut s).iter().all(|l| !l.dimmed));
    // The core marks the strips the solo silences.
    s.fake.edit_mixer(|m| {
        for st in m.strips.iter_mut().filter(|st| st.id != 5 && st.id != 0) {
            st.solo_muted = true;
        }
    });
    s.click("button", "Solo Audio 1");
    let dimmed: Vec<u64> = look(&mut s)
        .iter()
        .filter(|l| l.dimmed)
        .map(|l| l.strip)
        .collect();
    assert_eq!(dimmed, [6, 1, 2]);
}

#[gpui::test]
fn the_output_menu_routes_to_a_bus(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("menu", "Output of Audio 1");
    let items: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(items, ["Master", "Bus 1"]);
    s.click("menuitem", "Bus 1");
    assert!(
        s.fake
            .calls()
            .contains(&"set_output(5,Some(2))".to_string()),
        "{:?}",
        s.fake.calls()
    );
    assert_eq!(
        s.value("menu", "Output of Audio 1").as_deref(),
        Some("\u{2192} Bus 1")
    );
    assert!(s.find("menuitem", "Master").is_none(), "the menu closed");
    // A bus does not output to itself.
    s.click("menu", "Output of Bus 1");
    assert!(s.find("menuitem", "Bus 1").is_none());
    assert!(s.find("menuitem", "Master").is_some());
}

#[gpui::test]
fn the_automation_menu_sets_the_strips_mode(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("menu", "Automation of Audio 2");
    let items: Vec<(String, bool)> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| (e.label, e.selected))
        .collect();
    let titles: Vec<&str> = items.iter().map(|i| i.0.as_str()).collect();
    assert_eq!(titles, ["Off", "Read", "Touch", "Latch", "Write", "Trim"]);
    assert_eq!(
        items
            .iter()
            .filter(|i| i.1)
            .map(|i| i.0.as_str())
            .collect::<Vec<_>>(),
        ["Read"],
        "the current one checked"
    );
    s.click("menuitem", "Latch");
    assert_eq!(
        s.value("menu", "Automation of Audio 2").as_deref(),
        Some("Latch")
    );
}

#[gpui::test]
fn add_bus_adds_a_strip(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("button", ADD_BUS);
    assert!(s.fake.calls().contains(&"add_bus(Bus 2)".to_string()));
    let labels = s.fader_labels();
    assert_eq!(labels[labels.len() - 2], "Fader of Bus 2");
    assert_eq!(labels.last().map(String::as_str), Some("Fader of Master"));
}

#[gpui::test]
fn rename_by_double_click(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.double_click("text", "Audio 1");
    let field = s.element("textfield", NAME);
    assert_eq!(field.value.as_deref(), Some("Audio 1"));
    assert!(field.focused);
    assert!(
        !s.model(|d| d.transport_keys_enabled()),
        "the bare keys wait"
    );
    // The name is all selected: typing replaces it.
    s.cx.simulate_input("Vox");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert!(s.fake.calls().contains(&"rename_strip(5,Vox)".to_string()));
    assert!(s.find("textfield", NAME).is_none());
    assert!(s.find("slider", "Fader of Vox").is_some());
    assert!(s.model(|d| d.transport_keys_enabled()));
    // Escape cancels.
    s.double_click("text", "Bus 1");
    s.cx.simulate_input("Gone");
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert!(
        !s.fake
            .calls()
            .iter()
            .any(|c| c.starts_with("rename_strip(2"))
    );
    assert!(s.find("text", "Bus 1").is_some());
    assert!(s.model(|d| d.mixer().unwrap().renaming().is_none()));
}

#[gpui::test]
fn selection_reaches_the_core(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let selected = |s: &mut Setup| {
        let v = s.view(MixerPlacement::View).unwrap();
        v.read_with(&s.cx, |v, _| {
            v.drawn()
                .iter()
                .filter(|l| l.selected)
                .map(|l| l.strip)
                .collect::<Vec<_>>()
        })
    };
    s.click("text", "Audio 1");
    assert_eq!(s.fake.surface_selections().last(), Some(&vec![5]));
    assert_eq!(selected(&mut s), [5]);
    let at = s.element("text", "Bus 1").bounds.center();
    let secondary = s.cx.update(|_, cx| Modifiers::secondary_key(cx));
    s.click_at(at, secondary);
    assert_eq!(s.fake.surface_selections().last(), Some(&vec![2, 5]));
    assert_eq!(selected(&mut s), [5, 2]);
    let at = s.element("text", "Audio 1").bounds.center();
    s.click_at(at, Modifiers::shift());
    assert_eq!(s.fake.surface_selections().last(), Some(&vec![2]));
    s.click("text", "Synth");
    assert_eq!(s.fake.surface_selections().last(), Some(&vec![1]));
    // A click on a control of another strip leaves the selection.
    s.click("button", "Mute Audio 2");
    assert_eq!(selected(&mut s), [1]);
}

#[gpui::test]
fn a_strip_that_vanishes_releases_its_drag(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let at = s.fader_at("Audio 2");
    s.down(at);
    s.move_to(point(at.x, at.y - px(2.)));
    s.move_to(point(at.x, at.y - px(30.)));
    let fader = T::StripFader { strip: 6 };
    assert_eq!(s.of(fader).first(), Some(&touch(None)));
    // The strip goes (an undo elsewhere removed it); the mixer reads it.
    s.fake.edit_mixer(|m| m.strips.retain(|st| st.id != 6));
    s.update_model(|d| {
        d.with_mixer(|m, d| m.set_colour(d, 1, Some(vizdaw_ffi::mixer::SwatchRow::Red)));
    });
    assert!(s.find("slider", "Fader of Audio 2").is_none());
    assert_eq!(s.of(fader).last(), Some(&RELEASE));
    assert_eq!(s.of(fader).iter().filter(|p| **p == RELEASE).count(), 1);
    assert_eq!(
        s.model(|d| d.mixer().unwrap().pressed(MixerPlacement::View)),
        None
    );
    // The rest of the drag sends nothing.
    let n = s.fake.gestures().len();
    s.move_to(point(at.x, at.y - px(50.)));
    s.up(point(at.x, at.y - px(50.)));
    assert_eq!(s.fake.gestures().len(), n);
}

#[gpui::test]
fn deactivation_cancels_the_placements_drag(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.window
        .update(&mut s.cx, |_, window, _| window.activate_window())
        .unwrap();
    s.cx.run_until_parked();
    let at = s.fader_at("Audio 1");
    s.down(at);
    s.move_to(point(at.x, at.y - px(2.)));
    s.move_to(point(at.x, at.y - px(20.)));
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    let fader = T::StripFader { strip: 5 };
    assert_eq!(s.of(fader).last(), Some(&RELEASE));
    assert_eq!(
        s.model(|d| d.mixer().unwrap().pressed(MixerPlacement::View)),
        None
    );
}

#[gpui::test]
fn leaving_the_mix_view_mid_drag_releases_the_touch(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let at = s.fader_at("Audio 1");
    s.down(at);
    s.move_to(point(at.x, at.y - px(2.)));
    s.move_to(point(at.x, at.y - px(20.)));
    s.run_menu("ui.view_arrange");
    let fader = T::StripFader { strip: 5 };
    assert_eq!(s.of(fader).last(), Some(&RELEASE));
    assert_eq!(s.of(fader).iter().filter(|p| **p == RELEASE).count(), 1);
    assert!(s.placements().is_empty());
}

#[gpui::test]
fn the_bottom_pane_mixer_is_a_placement_of_its_own(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.run_menu("ui.view_arrange");
    s.update_model(|d| {
        d.update_layout(|l| {
            let _ = l.views.set_pane(
                PaneSide::Bottom,
                Some(PaneView::Mixer),
                Some(true),
                None,
                MainView::Arrange,
            );
        })
    });
    assert_eq!(s.placements(), [MixerPlacement::Pane]);
    assert!(s.find("button", ADD_BUS).is_some());
    let bottom = s.element("group", "Bottom Pane").bounds;
    let name = s.element("text", "Audio 1").bounds;
    assert!(bottom.contains(&name.center()), "drawn in the bottom pane");
    assert!(bottom.contains(&s.element("button", ADD_BUS).bounds.center()));
    // Hidden, it goes.
    s.update_model(|d| {
        d.update_layout(|l| {
            let _ = l
                .views
                .set_pane(PaneSide::Bottom, None, Some(false), None, MainView::Arrange);
        })
    });
    assert!(s.placements().is_empty());
    assert!(s.find("slider", "Fader of Audio 1").is_none());
}

#[gpui::test]
fn many_strips_record_only_the_visible_ones(cx: &mut TestAppContext) {
    let mut s = setup(cx, many(40));
    let faders = s.faders();
    assert!(!faders.is_empty());
    assert!(faders.len() < 41, "{} of 41 recorded", faders.len());
    assert_eq!(faders[0].label, "Fader of Audio 1");
    assert!(s.find("slider", "Fader of Master").is_none());
    // The last strip in view stays in view when scrolled by three.
    let kept = faders.last().unwrap().clone();
    // Every recorded fader shows, at least in part.
    let shown = |s: &mut Setup, label: &str| {
        s.window
            .read_with(&s.cx, |v, _| {
                let r = v.registry().borrow();
                let e = r
                    .elements()
                    .iter()
                    .find(|e| e.label == label)
                    .cloned()
                    .unwrap();
                r.visible_part(&e).is_some()
            })
            .unwrap()
    };
    for f in &faders {
        assert!(shown(&mut s, &f.label), "{} recorded but hidden", f.label);
    }
    // Scrolled across by three strips: the first goes, a later one comes.
    let group = s.element("group", "Mixer").bounds.center();
    s.scroll(group, -(3. * 126.), 0.);
    assert!(s.find("slider", "Fader of Audio 1").is_none());
    let last = faders.last().unwrap().label.clone();
    let next: usize = last.trim_start_matches("Fader of Audio ").parse().unwrap();
    assert!(
        s.find("slider", &format!("Fader of Audio {}", next + 1))
            .is_some()
    );
    assert_eq!(
        s.element("slider", &kept.label).id,
        kept.id,
        "a strip's controls keep their IDs as the strips scroll"
    );
    // A vertical wheel with no room down scrolls across.
    s.scroll(group, 0., -100_000.);
    assert!(s.find("slider", "Fader of Master").is_some());
    assert!(s.find("slider", "Fader of Audio 2").is_none());
    s.scroll(group, 100_000., 100_000.);
    assert!(
        s.find("slider", "Fader of Audio 1").is_some(),
        "back at the start"
    );
}

#[gpui::test]
fn the_mixer_follows_the_document(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    // A strip the core made (a new audio track), then the document moves.
    s.fake.edit_mixer(|m| {
        m.strips.insert(
            2,
            strip_row(7, "Audio 3", StripKindRow::AudioPath { path: 3 }),
        )
    });
    s.update_model(|d| d.rename_track(30, "Kit"));
    assert!(s.find("slider", "Fader of Audio 3").is_some());
}

#[gpui::test]
fn a_mixer_that_cannot_be_read_says_so(cx: &mut TestAppContext) {
    let f = fake();
    f.set_call_error(
        "mixer",
        Some(vizdaw_ffi::VizError::Model {
            message: "broken".into(),
        }),
    );
    let mut s = setup(cx, f);
    assert!(
        s.find("text", vizdaw_gpui::mixer_view::UNAVAILABLE)
            .is_some()
    );
    assert!(
        s.find("text", vizdaw_gpui::mixer_view::UNAVAILABLE_NOTE)
            .is_some()
    );
    assert!(s.faders().is_empty());
}

fn busy() -> vizdaw_ffi::VizError {
    vizdaw_ffi::VizError::Busy {
        message: "editor opening".into(),
    }
}

#[gpui::test]
fn a_typed_name_survives_a_view_change(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.double_click("text", "Audio 1");
    s.cx.simulate_input("Vox");
    s.cx.run_until_parked();
    s.run_menu("ui.view_arrange");
    assert!(
        s.fake.calls().contains(&"rename_strip(5,Vox)".to_string()),
        "{:?}",
        s.fake.calls()
    );
    assert!(s.model(|d| d.transport_keys_enabled()));
}

#[gpui::test]
fn a_typed_name_survives_a_save(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.double_click("text", "Audio 1");
    s.cx.simulate_input("Vox");
    s.cx.run_until_parked();
    s.cx.dispatch_action(vizdaw_gpui::Save);
    s.cx.run_until_parked();
    assert!(
        s.fake.calls().contains(&"rename_strip(5,Vox)".to_string()),
        "{:?}",
        s.fake.calls()
    );
}

#[gpui::test]
fn a_drag_scrolled_out_of_view_keeps_going(cx: &mut TestAppContext) {
    let mut s = setup(cx, many(40));
    let at = s.fader_at("Audio 1");
    s.down(at);
    s.move_to(point(at.x, at.y - px(2.)));
    s.move_to(point(at.x, at.y - px(20.)));
    let before = s.of(T::StripFader { strip: 10 }).len();
    let group = s.element("group", "Mixer").bounds.center();
    s.scroll(group, -2000., 0.);
    let hidden = s
        .window
        .read_with(&s.cx, |v, _| {
            let r = v.registry().borrow();
            let e = r.elements().iter().find(|e| e.label == "Audio 1").cloned();
            e.is_none_or(|e| r.visible_part(&e).is_none())
        })
        .unwrap();
    assert!(hidden, "scrolled out of view");
    s.move_to(point(at.x, at.y - px(60.)));
    s.up(point(at.x, at.y - px(60.)));
    let phases = s.of(T::StripFader { strip: 10 });
    assert_eq!(phases.iter().filter(|p| **p == RELEASE).count(), 1);
    assert_eq!(phases.last(), Some(&RELEASE));
    assert!(
        matches!(phases[before], GesturePhaseRow::Value { .. }),
        "the drag went on after the scroll: {:?}",
        &phases[before..]
    );
}

#[gpui::test]
fn a_busy_mixer_is_not_read_every_frame(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.fake.set_call_error("mixer", Some(busy()));
    let reads = s.fake.mixer_reads();
    // The document moves: the mixer is owed a read, which meets Busy.
    // Drawing again does not read it again at once (no frame-by-frame
    // loop: the window settles).
    s.update_model(|d| d.rename_track(30, "Kit"));
    s.cx.run_until_parked();
    assert!(
        s.fake.mixer_reads() - reads <= 2,
        "{} reads before the window settled",
        s.fake.mixer_reads() - reads
    );
    // Busy ends: the strips follow.
    s.fake.set_call_error("mixer", None);
    s.fake.edit_mixer(|m| {
        m.strips.insert(
            2,
            strip_row(7, "Audio 3", StripKindRow::AudioPath { path: 3 }),
        )
    });
    s.cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    s.cx.run_until_parked();
    assert!(s.find("slider", "Fader of Audio 3").is_some());
}

#[gpui::test]
fn a_placement_registers_once_the_mixer_can_be_read(cx: &mut TestAppContext) {
    let f = fake();
    f.set_call_error("mixer", Some(busy()));
    let mut s = setup(cx, f);
    assert!(s.faders().is_empty());
    s.fake.set_call_error("mixer", None);
    s.cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    s.cx.run_until_parked();
    assert_eq!(s.placements(), [MixerPlacement::View]);
    assert!(s.find("slider", "Fader of Master").is_some());
}

// Meters, heard values and the Mixer window (slice 6a Task 6).

/// A read of the levels: (strip, left, right) for each strip in it.
fn levels(of: &[(u64, f32, f32)]) -> vizdaw_ffi::mixer::MixerLevels {
    vizdaw_ffi::mixer::MixerLevels {
        meters: of
            .iter()
            .map(|&(strip, peak_l, peak_r)| vizdaw_ffi::mixer::MeterRow {
                strip,
                peak_l,
                peak_r,
            })
            .collect(),
        dsp_load: 0.12,
        dsp_peak: 0.3,
        latency_frames: 0,
        faulted: vec![],
        midi: vec![],
    }
}

impl Setup {
    /// One poll period passes: every shown placement polls once.
    fn period(&mut self) {
        self.cx.executor().advance_clock(POLL_EVERY);
        self.cx.run_until_parked();
    }

    /// Strip `strip`'s meter as the Mix view drew it last.
    fn meter(&mut self, strip: u64) -> MeterLevel {
        let v = self.view(MixerPlacement::View).expect("the Mix view");
        v.read_with(&self.cx, |v, _| {
            v.drawn()
                .iter()
                .find(|l| l.strip == strip)
                .map(|l| l.meter)
                .unwrap_or_else(|| panic!("strip {strip} not drawn"))
        })
    }

    /// The document's Mixer window, while open.
    fn mixer_window(&mut self) -> Option<WindowHandle<DocumentWindow>> {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.mixer_window_of(doc).and_then(|w| w.document_window())
        })
    }

    fn kinds(&mut self) -> Vec<WindowKind> {
        self.app.read_with(&self.cx, |a, _| {
            a.windows().iter().map(|w| w.kind).collect()
        })
    }

    /// The elements the Mixer window drew last.
    fn mixer_elements(&mut self) -> Vec<Element> {
        let w = self.mixer_window().expect("a Mixer window");
        w.read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }
}

/// What the bar shows for `levels(..)`'s load (no sample rate: no PDC).
const LOAD: &str = "DSP 12% \u{00B7} peak 30%";

#[gpui::test]
fn meters_decay_and_fall_silent(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let reads = s.fake.levels_reads();
    s.fake.set_levels(levels(&[(5, 1.0, 0.5)]));
    s.period();
    assert!(s.fake.levels_reads() > reads, "the shown placement polls");
    let m = s.meter(5);
    assert!(
        (m.left - 1.0).abs() < 1e-3 && (m.right - 0.5).abs() < 1e-3,
        "{m:?}"
    );
    assert_eq!(s.meter(6), MeterLevel::SILENT);
    let load: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "text" && e.label.starts_with("DSP "))
        .map(|e| e.label)
        .collect();
    assert_eq!(load, [LOAD], "the load follows the read");
    // The strip leaves the take: it falls 0.85 a period…
    s.fake.set_levels(levels(&[]));
    s.period();
    let fallen = s.meter(5);
    assert!(
        (fallen.left - 0.85).abs() < 0.01,
        "one period down: {fallen:?}"
    );
    // … to silence, and no frame is asked for any more.
    for _ in 0..60 {
        s.period();
    }
    assert_eq!(s.meter(5), MeterLevel::SILENT);
    let drawn: Vec<MeterLevel> = s
        .view(MixerPlacement::View)
        .unwrap()
        .read_with(&s.cx, |v, _| v.drawn().iter().map(|l| l.meter).collect());
    assert!(
        drawn.iter().all(|m| !controls::meter::wants_frame(*m)),
        "no meter asks for a frame once silent: {drawn:?}"
    );
    // Nothing more to show: further polls redraw nothing.
    let version = s.model(|d| d.mixer().unwrap().shown_version());
    s.period();
    s.period();
    assert_eq!(s.model(|d| d.mixer().unwrap().shown_version()), version);
    // A placement that goes stops polling.
    s.run_menu("ui.view_arrange");
    let n = s.fake.levels_reads();
    for _ in 0..5 {
        s.period();
    }
    assert_eq!(s.fake.levels_reads(), n, "no placement, no poll");
}

#[gpui::test]
fn heard_values_follow_while_playing(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let rest = s.value("slider", "Fader of Audio 1");
    s.fake.set_heard(std::collections::HashMap::from([
        (T::StripFader { strip: 5 }, -12.0),
        (T::StripPan { strip: 5 }, -0.5),
    ]));
    // Stopped: nothing is heard, the snapshot shows.
    s.period();
    assert!(
        s.fake.heard_calls().is_empty(),
        "no heard values while stopped"
    );
    assert_eq!(s.value("slider", "Fader of Audio 1"), rest);
    s.run_menu("transport.play_stop");
    assert!(s.model(|d| d.is_playing()));
    s.period();
    assert_eq!(
        s.value("slider", "Fader of Audio 1").as_deref(),
        Some(StripEntry::level_text(-12.0).as_str())
    );
    assert_eq!(
        s.value("slider", "Pan knob of Audio 1").as_deref(),
        Some(StripEntry::pan_text(-0.5).as_str())
    );
    let asked = s.fake.heard_calls().last().cloned().unwrap();
    assert!(asked.contains(&T::StripFader { strip: 5 }), "{asked:?}");
    // Automation moves the fader: it follows.
    s.fake.set_heard(std::collections::HashMap::from([(
        T::StripFader { strip: 5 },
        -6.0,
    )]));
    s.period();
    assert_eq!(
        s.value("slider", "Fader of Audio 1").as_deref(),
        Some(StripEntry::level_text(-6.0).as_str())
    );
    // Stopped: the snapshot's value again.
    s.run_menu("transport.play_stop");
    s.period();
    assert_eq!(s.value("slider", "Fader of Audio 1"), rest);
}

#[gpui::test]
fn the_mixer_window_opens_and_reports_its_kind(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.run_menu("window.mixer");
    assert_eq!(s.kinds(), [WindowKind::Document, WindowKind::Mixer]);
    let w = s.mixer_window().unwrap();
    let doc = s.doc();
    let title = s.model(|d| d.snapshot().title.clone());
    let (kind, wdoc, shown_title, size) = w
        .update(&mut s.cx, |v, window, _| {
            (
                v.kind(),
                v.doc(),
                v.title().to_owned(),
                window.bounds().size,
            )
        })
        .unwrap();
    assert_eq!((kind, wdoc), (WindowKind::Mixer, doc));
    assert_eq!(shown_title, format!("Mixer \u{2014} {title}"));
    assert_eq!(size, gpui::size(px(760.), px(560.)));
    // Its placement is one of its own, beside the Mix view's.
    assert_eq!(
        s.placements(),
        [MixerPlacement::Window, MixerPlacement::View]
    );
    // Asked again (the menu, ⌥⌘M), it comes to the front: never a second.
    s.run_menu("window.mixer");
    let mut mcx = VisualTestContext::from_window(w.into(), &s.cx);
    mcx.dispatch_action(RunCommand::new("window.mixer".to_owned()));
    mcx.run_until_parked();
    assert_eq!(s.kinds(), [WindowKind::Document, WindowKind::Mixer]);
    // Its own transport bar, without the document-only items, over the
    // mixer: a short bar, so nothing folds into ».
    s.app.update(&mut s.cx, |a, cx| {
        let items = vec![(
            "items".to_owned(),
            core_control::registry::ArgValue::Text("cluster,main_view,panes".into()),
        )];
        vizdaw_app::commands::run_app("ui.set_transport_bar", &items, &mut a.prefs).unwrap();
        cx.notify();
    });
    s.cx.run_until_parked();
    assert!(
        s.find("button", "Left Pane").is_some(),
        "the document window's"
    );
    let all = s.mixer_elements();
    let has = |role: &str, label: &str| all.iter().any(|e| e.role == role && e.label == label);
    assert!(has("button", "Play"), "{all:#?}");
    assert!(has("slider", "Fader of Audio 1"), "{all:#?}");
    assert!(has("button", ADD_BUS));
    assert!(!all.iter().any(|e| e.label.ends_with(" Pane")), "{all:#?}");
    assert!(!has("group", "Main View"), "{all:#?}");
    // Play in the Mixer window plays the document.
    let play = all
        .iter()
        .find(|e| e.role == "button" && e.label == "Play")
        .unwrap()
        .bounds
        .center();
    mcx.simulate_click(play, Modifiers::none());
    mcx.run_until_parked();
    assert!(s.model(|d| d.is_playing()));
}

#[gpui::test]
fn closing_the_mixer_window_keeps_the_mix_views_drag(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.run_menu("window.mixer");
    let w = s.mixer_window().unwrap();
    // A drag in the Mix view.
    let at = s.fader_at("Audio 1");
    s.down(at);
    s.move_to(point(at.x, at.y - px(2.)));
    s.move_to(point(at.x, at.y - px(20.)));
    let fader = T::StripFader { strip: 5 };
    assert_eq!(s.of(fader).first(), Some(&touch(None)));
    // The Mixer window closes: only its placement goes.
    w.update(&mut s.cx, |_, window, _| window.remove_window())
        .unwrap();
    s.cx.run_until_parked();
    assert_eq!(s.kinds(), [WindowKind::Document]);
    assert_eq!(s.placements(), [MixerPlacement::View]);
    assert!(!s.of(fader).contains(&RELEASE), "{:?}", s.of(fader));
    assert_eq!(
        s.model(|d| d.mixer().unwrap().pressed(MixerPlacement::View)),
        Some(fader)
    );
    // The drag carries on and ends at its mouse-up.
    s.move_to(point(at.x, at.y - px(30.)));
    s.up(point(at.x, at.y - px(30.)));
    assert_eq!(s.of(fader).last(), Some(&RELEASE));
    assert_eq!(s.of(fader).iter().filter(|p| **p == RELEASE).count(), 1);
}

#[gpui::test]
fn closing_the_document_closes_its_mixer_window(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.run_menu("window.mixer");
    let w = s.mixer_window().unwrap();
    s.window
        .update(&mut s.cx, |_, window, _| window.remove_window())
        .unwrap();
    s.cx.run_until_parked();
    assert!(s.app.read_with(&s.cx, |a, _| a.windows().is_empty()));
    assert!(
        w.read_with(&s.cx, |_, _| ()).is_err(),
        "the Mixer window closed"
    );
}

#[gpui::test]
fn the_mixer_window_alone_keeps_polling(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.run_menu("window.mixer");
    s.run_menu("ui.view_arrange");
    assert_eq!(s.placements(), [MixerPlacement::Window]);
    s.fake.set_levels(levels(&[(6, 0.5, 0.5)]));
    let n = s.fake.levels_reads();
    s.period();
    assert!(s.fake.levels_reads() > n);
    let w = s.mixer_window().unwrap();
    let drawn = w
        .read_with(&s.cx, |v, cx| {
            v.mixer_view(MixerPlacement::Window)
                .unwrap()
                .read(cx)
                .drawn()
                .iter()
                .find(|l| l.strip == 6)
                .map(|l| l.meter)
        })
        .unwrap();
    assert_eq!(drawn.map(|m| m.left), Some(0.5));
}

#[gpui::test]
fn a_rename_dropped_with_its_placement_commits(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    // The bottom pane's mixer, the only placement.
    s.run_menu("ui.view_arrange");
    s.update_model(|d| {
        d.update_layout(|l| {
            l.views.arrange.bottom.view = vizdaw_app::editor::PaneView::Mixer;
            l.views.arrange.bottom.shown = true;
        })
    });
    s.double_click("text", "Audio 1");
    s.cx.simulate_input("Vox");
    s.cx.run_until_parked();
    // Too short for the bottom pane for this frame: the placement goes.
    s.cx.simulate_resize(gpui::size(px(1440.), px(300.)));
    s.cx.run_until_parked();
    assert!(s.find("textfield", NAME).is_none());
    assert!(
        s.fake.calls().contains(&"rename_strip(5,Vox)".to_string()),
        "{:?}",
        s.fake.calls()
    );
    assert!(s.model(|d| d.mixer().unwrap().renaming().is_none()));
    assert!(s.model(|d| d.transport_keys_enabled()));
}

#[gpui::test]
fn the_strip_menu_has_the_swift_items(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.right_click("text", "Bus 1");
    assert_eq!(
        s.titles(),
        [
            "Rename\u{2026}",
            "Set All Strips to",
            "Remove Bus",
            "Colour",
            "Group",
            "Assign to VCA",
            "Remove from VCA",
            "MIDI Learn Maps\u{2026}"
        ]
    );
    assert!(s.enabled_item("Group"));
    assert!(!s.enabled_item("Remove from VCA"), "no VCA holds it");
    s.click("menuitem", "Set All Strips to");
    let titles = s.titles();
    assert_eq!(
        titles[8..],
        ["Off", "Read", "Touch", "Latch", "Write", "Trim"],
        "{titles:?}"
    );
    s.close_menus();
    // Only a bus has Remove Bus; master has no group or VCA items, and
    // has MIDI Learn Maps….
    s.right_click("text", "Audio 1");
    assert!(!s.titles().contains(&"Remove Bus".to_owned()));
    s.close_menus();
    s.right_click("text", "Master");
    assert_eq!(
        s.titles(),
        [
            "Rename\u{2026}",
            "Set All Strips to",
            "Colour",
            "MIDI Learn Maps\u{2026}"
        ]
    );
    s.close_menus();
    // A right-click does not select.
    assert!(s.model(|d| d.selected_strips().is_empty()));
}

#[gpui::test]
fn the_strip_menu_colours_renames_sets_all_and_removes_a_bus(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.right_click("text", "Audio 1");
    s.click("menuitem", "Colour");
    let items = s.items();
    // Audio 1 is no bus: seven items, then the submenu's.
    let colours: Vec<&str> = items[7..].iter().map(|i| i.0.as_str()).collect();
    assert_eq!(
        colours,
        [
            "None", "Red", "Orange", "Yellow", "Green", "Teal", "Blue", "Purple", "Pink"
        ]
    );
    assert!(items[7].2, "no colour of its own: None is checked");
    s.click("menuitem", "Teal");
    assert!(
        s.fake
            .calls()
            .contains(&"set_strip_colour(5,Some(Teal))".to_owned()),
        "{:?}",
        s.fake.calls()
    );
    s.right_click("text", "Audio 1");
    s.click("menuitem", "Set All Strips to");
    s.click("menuitem", "Latch");
    assert_eq!(
        s.fake.mode_calls().last(),
        Some(&(vec![5, 6, 1, 2, 0], AutomationModeRow::Latch))
    );
    s.right_click("text", "Audio 2");
    s.click("menuitem", "Rename\u{2026}");
    assert!(s.element("textfield", NAME).focused);
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    s.right_click("text", "Bus 1");
    s.click("menuitem", "Remove Bus");
    assert!(s.fake.calls().contains(&"remove_bus(2)".to_owned()));
    assert!(s.find("slider", "Fader of Bus 1").is_none());
}

/// [`fake`] with VCA 1 (9) holding Audio 2.
fn vca_fake() -> FakeSession {
    let f = fake();
    f.edit_mixer(|m| {
        m.vcas = vec![vizdaw_app::testing::vca_row(9, "VCA 1", &[6], 0.0)];
        for st in m.strips.iter_mut().filter(|st| st.id == 6) {
            st.vcas = vec![9];
        }
    });
    f
}

#[gpui::test]
fn the_strip_menu_groups_and_assigns_its_strips(cx: &mut TestAppContext) {
    let mut s = setup(cx, vca_fake());
    // Not in the selection: the strip alone.
    s.right_click("text", "Audio 1");
    s.click("menuitem", "Assign to VCA");
    assert_eq!(s.titles()[7..], ["VCA 1", "New VCA"]);
    assert!(s.enabled_item("VCA 1"));
    s.click("menuitem", "VCA 1");
    assert_eq!(s.mixer_calls().last().unwrap().0, "mixer.assign_vca");
    assert!(s.mixer_calls().last().unwrap().1.contains("\"5\""));
    // In the selection: the selection.
    s.click("text", "Audio 1");
    let at = s.element("text", "Audio 2").bounds.center();
    s.click_at(at, Modifiers::shift());
    s.right_click("text", "Audio 2");
    s.click("menuitem", "Group");
    let (id, args) = s.mixer_calls().last().unwrap().clone();
    assert_eq!(id, "mixer.new_group");
    assert!(args.contains("\"5,6\""), "{args}");
    s.right_click("text", "Audio 2");
    s.click("menuitem", "Assign to VCA");
    s.click("menuitem", "New VCA");
    let (id, args) = s.mixer_calls().last().unwrap().clone();
    assert_eq!(id, "mixer.new_vca");
    assert!(args.contains("\"5,6\""), "{args}");
    // Remove from VCA ▸ takes the members only.
    s.right_click("text", "Audio 2");
    assert!(s.enabled_item("Remove from VCA"));
    s.click("menuitem", "Remove from VCA");
    s.click("menuitem", "VCA 1");
    let (id, args) = s.mixer_calls().last().unwrap().clone();
    assert_eq!(id, "mixer.unassign_vca");
    assert!(args.contains("\"6\""), "{args}");
    // A VCA holding every strip of the menu is off in Assign to VCA ▸.
    s.close_menus();
    s.click("text", "Audio 2");
    s.right_click("text", "Audio 2");
    s.click("menuitem", "Assign to VCA");
    assert!(!s.enabled_item("VCA 1"));
}

#[gpui::test]
fn the_mixer_menu_acts_on_the_selection(cx: &mut TestAppContext) {
    use vizdaw_gpui::menus::{MixerMenuItem, MixerOp};
    let f = vca_fake();
    f.edit_mixer(|m| {
        m.groups = vec![vizdaw_app::testing::group_row(7, "Drums", &[5, 6])];
        for st in m.strips.iter_mut().filter(|st| st.id == 5 || st.id == 6) {
            st.group = Some(7);
        }
    });
    let mut s = setup(cx, f);
    s.click("text", "Audio 1");
    let at = s.element("text", "Audio 2").bounds.center();
    s.click_at(at, Modifiers::shift());
    let run = |s: &mut Setup, op: MixerOp| {
        s.cx.dispatch_action(MixerMenuItem { op });
        s.cx.run_until_parked();
        s.mixer_calls().last().cloned()
    };
    let (id, args) = run(&mut s, MixerOp::Ungroup).unwrap();
    assert_eq!(id, "mixer.delete_group");
    assert!(args.contains("7"), "{args}");
    let (id, args) = run(&mut s, MixerOp::Assign { vca: 9 }).unwrap();
    assert_eq!(id, "mixer.assign_vca");
    assert!(args.contains("\"5\""), "only the strips it lacks: {args}");
    let (id, args) = run(&mut s, MixerOp::Unassign { vca: 9 }).unwrap();
    assert_eq!(id, "mixer.unassign_vca");
    assert!(args.contains("\"6\""), "only its members: {args}");
    let (id, args) = run(&mut s, MixerOp::NewVca).unwrap();
    assert_eq!(id, "mixer.new_vca");
    assert!(args.contains("\"5,6\""), "{args}");
    let (id, args) = run(&mut s, MixerOp::Group).unwrap();
    assert_eq!(id, "mixer.new_group");
    assert!(args.contains("\"5,6\""), "{args}");
}

/// Puts the app-wide suspend switch back however the test ends.
struct Unsuspend;

impl Drop for Unsuspend {
    fn drop(&mut self) {
        vizdaw_ffi::automation::set_automation_suspended(false);
    }
}

#[gpui::test]
fn suspend_automation_runs_from_its_menu_item(cx: &mut TestAppContext) {
    let _guard = Unsuspend;
    let mut s = setup(cx, fake());
    assert!(!vizdaw_ffi::automation::automation_suspended());
    s.run_menu("automation.suspend");
    assert!(vizdaw_ffi::automation::automation_suspended());
    s.run_menu("automation.suspend");
    assert!(!vizdaw_ffi::automation::automation_suspended());
}

#[gpui::test]
fn a_mode_command_on_a_busy_core_says_so_in_the_status_line(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("text", "Audio 1");
    s.fake.fail_next("set_automation_mode", busy());
    s.run_menu("automation.mode.latch");
    let doc = s.doc();
    let (status, alert) = s.app.read_with(&s.cx, |a, _| {
        let d = a.store.document(doc).unwrap();
        (d.status().map(String::from), a.store.alert().is_some())
    });
    assert_eq!(status.as_deref(), Some("editor opening"));
    assert!(!alert, "Swift's .busy: the status line, no alert");
}

// ---- Slice 8c: the strip's input row (Swift `StripInputView`) ----

const AUTO_HELP: &str =
    "Input monitoring: Auto (heard while armed and stopped, or recording in range)";

impl Setup {
    /// The fake's lanes changed: a new snapshot version fetches them.
    fn relanes(&mut self) {
        let mut n = self.fake.snap();
        n.version += 1;
        self.update_model(|d| d.apply(n));
    }

    fn menuitem_titles(&mut self) -> Vec<(String, bool)> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "menuitem")
            .map(|e| (e.label, e.selected))
            .collect()
    }
}

/// The strip's input row: the picker shows the input (led by the track's
/// name when another track plays through the same path), named "Input of
/// <track>", and the monitor button beside it is `<track>: <help>`, a
/// lane edit to the next mode. The strip's I toggle keeps its own label
/// (Swift `StripButtons`).
#[gpui::test]
fn the_strip_input_row_has_its_picker_and_monitor(cx: &mut TestAppContext) {
    use vizdaw_ffi::edit::LaneInputRow;
    let mut s = setup(cx, fake());
    assert_eq!(
        s.value("menu", "Input of Groove").as_deref(),
        Some("No Input")
    );
    assert_eq!(
        s.value("menu", "Input of Chords").as_deref(),
        Some("No Input")
    );
    assert!(s.find("menu", "Input of Synth").is_none(), "no input track");
    let groove = format!("Groove: {AUTO_HELP}");
    assert_eq!(s.value("button", &groove).as_deref(), Some("on"));
    assert_eq!(
        s.value("button", "Chords: Input monitoring: off")
            .as_deref(),
        Some("off")
    );
    assert!(s.find("button", "Input monitor Audio 1").is_some());
    // Above the EQ, beside the picker.
    let picker = s.element("menu", "Input of Groove").bounds;
    let monitor = s.element("button", &groove).bounds;
    assert_eq!(picker.center().y, monitor.center().y);
    assert!(monitor.origin.x >= picker.right());
    s.click("button", &groove);
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetMonitor {
                seq: 3,
                lane: 30,
                monitor: MonitorModeRow::Input,
            }
        })
    );
    // Another track on Audio 1's path: the picker says whose input.
    let mut groove = vizdaw_app::testing::lane_row(30, "Groove", false);
    groove.audio_out = Some(1);
    groove.input = Some(LaneInputRow::Audio {
        first: 0,
        stereo: false,
        return_of: None,
    });
    let mut bass = vizdaw_app::testing::lane_row(32, "Bass", false);
    bass.audio_out = Some(1);
    let mut chords = vizdaw_app::testing::lane_row(31, "Chords", false);
    chords.audio_out = Some(2);
    s.fake.set_lanes(3, vec![groove, chords, bass]);
    s.relanes();
    assert_eq!(
        s.value("menu", "Input of Groove").as_deref(),
        Some("Groove: In 1")
    );
    assert_eq!(
        s.value("menu", "Input of Chords").as_deref(),
        Some("No Input")
    );
}

/// The strip's picker: the model's sections with the input checked; a
/// choice is one lane edit and closes it; Escape and a click outside
/// close it.
#[gpui::test]
fn the_strip_picker_chooses_an_input(cx: &mut TestAppContext) {
    use vizdaw_ffi::edit::LaneInputRow;
    let mut s = setup(cx, fake());
    s.click("menu", "Input of Groove");
    assert_eq!(
        s.menuitem_titles(),
        vec![
            ("No Input".into(), true),
            ("Audio".into(), false),
            ("In 1".into(), false),
            ("In 2".into(), false),
            ("In 1+2".into(), false),
        ]
    );
    s.click("menuitem", "In 2");
    assert!(s.find("menuitem", "No Input").is_none());
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetInput {
                seq: 3,
                lane: 30,
                input: Some(LaneInputRow::Audio {
                    first: 1,
                    stereo: false,
                    return_of: None
                })
            }
        })
    );
    assert_eq!(s.value("menu", "Input of Groove").as_deref(), Some("In 2"));
    s.click("menu", "Input of Groove");
    assert!(s.find("menuitem", "In 1").is_some());
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert!(s.find("menuitem", "In 1").is_none());
    s.click("menu", "Input of Groove");
    let away = s.element("button", "Mute Master").bounds.center();
    let n = s.fake.edits().len();
    s.click_at(away, Modifiers::none());
    assert!(s.find("menuitem", "In 1").is_none());
    assert_eq!(s.fake.edits().len(), n, "the click outside only closes it");
}

/// The picker's help names a synth's return (Swift `StripInputView`).
#[gpui::test]
fn the_strip_picker_help_names_the_return(cx: &mut TestAppContext) {
    use vizdaw_ffi::edit::LaneInputRow;
    let f = fake();
    f.set_destinations(vec![vizdaw_app::testing::dest_row(9, "Moog")]);
    let mut ret = vizdaw_app::testing::lane_row(30, "Groove", false);
    ret.audio_out = Some(1);
    ret.input = Some(LaneInputRow::Audio {
        first: 0,
        stereo: false,
        return_of: Some(9),
    });
    f.set_lanes(3, vec![ret]);
    let mut s = setup(cx, f);
    let (help, plain) = s.model(|d| {
        let l = d.lanes()[0].clone();
        let mut other = l.clone();
        other.input = None;
        (
            vizdaw_gpui::mixer_view::input_help(d, &l),
            vizdaw_gpui::mixer_view::input_help(d, &other),
        )
    });
    assert_eq!(help, "Input of Groove, recording \u{21E0} Moog");
    assert_eq!(plain, "Input of Groove");
}
