//! The Drum Grid in the bottom pane (Swift `S/DrumGridPane.swift`):
//! Editors › Drum Grid (⌘4), its bar (Paint/Erase, View, Step, Spacing,
//! Length, Vel, Add Row and the Track menu), the empty state, the Drum
//! menu and a saved last editor, driven through GPUI's test platform with
//! the scripted session.

use std::sync::Arc;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, TestAppContext,
    VisualTestContext, WindowHandle,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::drum_grid::drum_quantize_choices;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Tool};
use vizdaw_app::testing::{FakeSession, demo_fake, drum_fake, lane_row, roll_fake};
use vizdaw_ffi::drum::DrumEdit;
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::snapshot::LaneRow;
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_gpui::menus::{self, ItemSpec};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow};

const GRID: &str = "Drum Grid";
const ROWS: &str = "Drum Rows";
const PANE: &str = "Bottom Pane";

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
    let mut s = Setup {
        fake,
        app,
        window,
        cx: vcx,
    };
    // A 360-point bottom pane, as the roll's tests have.
    s.update_model(|d| d.update_layout(|l| l.views.current_mut().bottom.size = 360.));
    s
}

/// The grid on Drums (`Lane{3,40}`), calls cleared.
fn drums_grid(cx: &mut TestAppContext) -> Setup {
    let mut s = setup(cx, drum_fake());
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 40 },
            Some(DetailEditorKind::DrumGrid),
        )
    });
    s.fake.clear_calls();
    s
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

    /// The `role` labelled `label` in the bottom pane.
    fn in_pane(&mut self, role: &str, label: &str) -> Option<Element> {
        let p = self.find("group", PANE)?;
        self.elements().into_iter().find(|e| {
            e.path.len() > p.path.len()
                && e.path.starts_with(&p.path)
                && e.role == role
                && e.label == label
        })
    }

    fn control(&mut self, role: &str, label: &str) -> Element {
        self.in_pane(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in the bottom pane"))
    }

    fn has(&mut self, role: &str, label: &str) -> bool {
        self.in_pane(role, label).is_some()
    }

    fn value(&mut self, role: &str, label: &str) -> Option<String> {
        self.control(role, label).value
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    fn click_at(&mut self, position: gpui::Point<Pixels>, count: usize) {
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: count,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: count,
        });
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let e = self.control(role, label);
        self.click_at(e.bounds.center(), 1);
    }

    /// Opens picker `menu` and picks `item`.
    fn pick(&mut self, menu: &str, item: &str) {
        self.click("menu", menu);
        let e = self
            .find("menuitem", item)
            .unwrap_or_else(|| panic!("no menuitem {item:?} in {menu}"));
        self.click_at(e.bounds.center(), 1);
    }

    fn edits(&self) -> Vec<EditCommand> {
        self.fake.edits()
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

    fn menu_model(&mut self) -> menus::MenuModel {
        let keymap = self.app.read_with(&self.cx, |a, _| a.keys.keymap().clone());
        self.model(|d| menus::menu_model(Some(d), &keymap))
    }
}

fn mark(lane: u64) -> EditCommand {
    EditCommand::Drum {
        edit: DrumEdit::MarkLane {
            seq: 3,
            lane,
            drum: true,
        },
    }
}

#[gpui::test]
fn editors_drum_grid_opens_the_grid(cx: &mut TestAppContext) {
    let mut s = setup(cx, drum_fake());
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 40 },
            Some(DetailEditorKind::PianoRoll),
        )
    });
    assert!(s.find("group", GRID).is_none());
    s.keys("secondary-4");
    assert_eq!(s.model(|d| d.detail_kind_name()), "drum_grid");
    assert!(s.has("group", GRID), "the grid shows");
    assert!(s.has("group", ROWS), "its row headers show");
    assert_eq!(s.value("menu", "View").as_deref(), Some("Step Matrix"));
    assert_eq!(s.value("menu", "Step").as_deref(), Some("1/16"));
    assert_eq!(s.value("menu", "Spacing").as_deref(), Some("1/16"));
    assert_eq!(s.value("menu", "Length").as_deref(), Some("1/32"));
    assert!(s.has("text", "Vel 100"));
    assert!(s.control("button", "Paint").selected);
    assert!(!s.control("button", "Erase").selected);
    assert_eq!(s.value("menu", "Track").as_deref(), Some("Drums: Off"));
    assert!(s.has("button", "Add Row"));
    // The bottom pane's picker offers it.
    let picker = s.find("button", PANE).expect("the bottom pane's picker");
    assert_eq!(picker.value.as_deref(), Some("Drum Grid"));
}

#[gpui::test]
fn bar_changes_are_saved_with_the_project(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    s.pick("View", "Timeline");
    assert_eq!(s.value("menu", "View").as_deref(), Some("Timeline"));
    s.pick("Step", "1/8");
    s.pick("Spacing", "1/32");
    s.pick("Length", "1/64");
    assert_eq!(s.value("menu", "Step").as_deref(), Some("1/8"));
    let saved = s.saved_layout("vizdaw-drum-bar.vizdaw");
    assert_eq!(saved["drumGrid"]["view"], "timeline");
    assert_eq!(saved["drumGrid"]["step"], 7_680);
    assert_eq!(saved["drumGrid"]["spacing"], 1_920);
    assert_eq!(saved["drumGrid"]["duration"], 960);
    s.click("button", "Erase");
    assert_eq!(s.model(|d| d.tool()), Tool::Eraser);
    assert!(s.control("button", "Erase").selected);
    assert!(!s.control("button", "Paint").selected);
    s.click("button", "Paint");
    assert_eq!(s.model(|d| d.tool()), Tool::Pencil);
}

#[gpui::test]
fn the_velocity_stepper_stops_at_its_ends(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    for _ in 0..6 {
        s.click("button", "Increase Velocity");
    }
    assert!(s.has("text", "Vel 124"));
    s.click("button", "Increase Velocity");
    assert!(s.has("text", "Vel 127"));
    s.click("button", "Increase Velocity");
    assert!(s.has("text", "Vel 127"), "it stops at 127");
    s.update_model(|d| d.update_drum_settings(|g| g.velocity = 3));
    s.click("button", "Decrease Velocity");
    assert!(s.has("text", "Vel 1"));
    s.click("button", "Decrease Velocity");
    assert!(s.has("text", "Vel 1"), "it stops at 1");
}

#[gpui::test]
fn the_track_menu_sets_quantize_and_nudges(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let swing = drum_quantize_choices()
        .iter()
        .find(|c| c.title == "1/8 Swing 66%")
        .expect("a swing choice")
        .clone();
    s.click("menu", "Track");
    assert!(s.find("text", "Play Quantize").is_some());
    assert!(s.find("text", "Nudge: 0 ticks").is_some());
    assert!(s.find("menuitem", "Off").unwrap().selected);
    let e = s.find("menuitem", "1/8 Swing 66%").unwrap();
    s.click_at(e.bounds.center(), 1);
    assert_eq!(
        s.edits(),
        [EditCommand::Lane {
            edit: LaneEdit::SetPlayQuantize {
                seq: 3,
                lane: 40,
                quantize: swing.quantize,
            }
        }]
    );
    let n = s.edits().len();
    s.pick("Track", "Nudge Later");
    assert_eq!(
        s.edits()[n..],
        [EditCommand::Lane {
            edit: LaneEdit::SetPlayShift {
                seq: 3,
                lane: 40,
                shift: 120,
            }
        }]
    );
}

#[gpui::test]
fn add_row_adds_the_next_kit_row(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    s.click("button", "Add Row");
    assert_eq!(
        s.edits(),
        [EditCommand::Drum {
            edit: DrumEdit::AddRow {
                seq: 3,
                lane: 40,
                name: "Closed Hat".into(),
                note: 42,
                dest: None,
            }
        }]
    );
}

#[gpui::test]
fn a_sequence_without_drum_tracks_offers_its_tracks(cx: &mut TestAppContext) {
    let fake = drum_fake();
    fake.set_drum_lanes(3, vec![]);
    let mut s = setup(cx, fake);
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Sequence { seq: 3 },
            Some(DetailEditorKind::DrumGrid),
        )
    });
    assert!(s.has("text", "Choose a Track"));
    assert!(s.has(
        "text",
        "The Drum Grid edits a MIDI or Instrument track as drum rows."
    ));
    for t in ["Groove", "Drums", "Perc"] {
        assert!(s.has("button", t), "{t}");
    }
    assert!(!s.has("button", "Vocals"), "an audio track is not offered");
    assert!(!s.has("group", GRID));
    assert!(!s.has("menu", "Track"), "no current track");
    s.fake.clear_calls();
    s.click("button", "Groove");
    assert_eq!(s.edits(), [mark(30)]);
    assert_eq!(
        s.model(|d| d.layout().detail.as_ref().map(|d| d.target)),
        Some(EditorTarget::Lane { seq: 3, lane: 30 })
    );

    // Every track audio: nothing to offer.
    let audio = |id, name| LaneRow {
        kind: TrackKindRow::Audio,
        ..lane_row(id, name, false)
    };
    let fake = drum_fake();
    fake.set_drum_lanes(3, vec![]);
    fake.set_lanes(3, vec![audio(30, "Groove"), audio(42, "Vocals")]);
    let mut s = setup(cx, fake);
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Sequence { seq: 3 },
            Some(DetailEditorKind::DrumGrid),
        )
    });
    assert!(s.has("text", "No MIDI or Instrument Tracks"));
    assert!(s.has(
        "text",
        "Add a MIDI or Instrument track to edit it as drums."
    ));
    assert!(!s.has("text", "Choose a Track"));
}

#[gpui::test]
fn the_drum_menu_is_in_the_menu_bar(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let m = s.menu_model();
    let titles: Vec<&str> = m.menus.iter().map(|m| m.title.as_str()).collect();
    let track = titles.iter().position(|t| *t == "Track").unwrap();
    assert_eq!(titles[track + 1], "Drum", "{titles:?}");
    let drum = &m.menus[track + 1];
    let items: Vec<(Option<&str>, Option<bool>)> = drum
        .items
        .iter()
        .map(|i| match i {
            ItemSpec::Command { title, checked, .. } => (Some(title.as_str()), *checked),
            _ => (None, None),
        })
        .collect();
    assert_eq!(
        items,
        [
            (Some("Add Drum Row"), None),
            (Some("Show Velocity"), Some(false)),
            (Some("Split Row to Own Track"), None),
            (Some("Remove Drum Row"), None),
            (None, None),
            (Some("Timeline View"), Some(false)),
        ]
    );
    // The Editors menu lists the grid after the Strip Chart.
    let editors = m.menus.iter().find(|m| m.title == "Editors").unwrap();
    let ids: Vec<&str> = editors
        .items
        .iter()
        .filter_map(|i| match i {
            ItemSpec::Command { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    let chart = ids
        .iter()
        .position(|i| *i == "editors.strip_chart")
        .unwrap();
    assert_eq!(ids[chart + 1], "editors.drum_grid");

    // The menu runs on the grid: Timeline View checks.
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("drum.timeline".to_owned()));
    s.cx.run_until_parked();
    assert_eq!(s.value("menu", "View").as_deref(), Some("Timeline"));
    let m = s.menu_model();
    let drum = m.menus.iter().find(|m| m.title == "Drum").unwrap();
    assert!(drum.items.iter().any(|i| matches!(
        i,
        ItemSpec::Command { title, checked: Some(true), .. } if title == "Timeline View"
    )));
}

/// A song saved with the Drum Grid as its last editor: a block's
/// double-click opens the grid, which marks the MIDI track a drum track
/// (Review Focus 3).
#[gpui::test]
fn a_saved_drum_grid_last_editor_opens_the_grid(cx: &mut TestAppContext) {
    let fake = roll_fake();
    fake.set_layout(Some(r#"{"lastKind":"drumGrid"}"#));
    let mut s = setup(cx, fake);
    let block = s
        .find("button", "Block on Chords, bar 1, 1 event")
        .expect("the Chords block")
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
    assert_eq!(s.model(|d| d.detail_kind_name()), "drum_grid");
    assert_eq!(s.edits().last(), Some(&mark(31)));
    let saved = s.saved_layout("vizdaw-saved-drum-grid.vizdaw");
    assert_eq!(saved["lastKind"], "drumGrid");
}
