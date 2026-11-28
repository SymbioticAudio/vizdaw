//! The Drum Grid in the bottom pane (Swift `S/DrumGridPane.swift`):
//! Editors › Drum Grid (⌘4), its bar (Paint/Erase, View, Step, Spacing,
//! Length, Vel, Add Row and the Track menu), the empty state, the Drum
//! menu and a saved last editor, driven through GPUI's test platform with
//! the scripted session.

use std::sync::Arc;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, ScrollDelta, ScrollEvent,
    TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::arrangement::RULER_HEIGHT;
use vizdaw_app::drum_grid::{DrumRowKey, DrumView, MAX_TREE_ITEMS, drum_quantize_choices};
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, PaneView, Tool};
use vizdaw_app::testing::{FakeSession, demo_fake, drum_fake, lane_row, roll_fake};
use vizdaw_app::{DocSession as _, DocumentStore};
use vizdaw_ffi::drum::{DrumEdit, DrumPaintHitRow};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneEdit, LaneRef};
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

// ---- The canvas (Swift `T/DrumGridViewTests.swift`) ----
//
// Points are canvas content points: x from the canvas's left edge, y
// below the ruler. At the default 96 ticks a point a sixteenth is 40
// points. The bands: header 0–22, Kick 22–42, Snare 42–62 (to 90 with its
// velocity lane), Perc's header 62–84, Clap 84–104, the new-track band.

const Q: i64 = 15_360;
const S16: i64 = Q / 4;

/// Swift's `paint` (`T/DrumGridViewTests.swift:29-35`): row 2 (Snare) of
/// Drums at velocity 100 unless given.
fn paint(ticks: &[i64], velocity: u16, row: u64) -> EditCommand {
    EditCommand::Drum {
        edit: DrumEdit::Paint {
            seq: 3,
            lane: 40,
            row,
            hits: ticks
                .iter()
                .map(|&tick| DrumPaintHitRow { tick, velocity })
                .collect(),
            window: S16,
            duration: Q / 8,
        },
    }
}

fn snare(ticks: &[i64]) -> EditCommand {
    paint(ticks, 100 << 9, 2)
}

/// The grid on Drums with Snare's velocity lane shown.
fn velocity_grid(cx: &mut TestAppContext) -> Setup {
    let f = drum_fake();
    let mut lanes = f.drum_lanes(3).expect("the drum lanes");
    lanes[0].rows[1].show_velocity = true;
    f.set_drum_lanes(3, lanes);
    let mut s = setup(cx, f);
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 40 },
            Some(DetailEditorKind::DrumGrid),
        )
    });
    s.fake.clear_calls();
    s
}

/// ⌘ on macOS, ⌃ elsewhere, as `menus::secondary!`.
fn secondary_mods() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::command()
    } else {
        Modifiers::control()
    }
}

impl Setup {
    /// Window point of canvas content point `(x, y)` (below the ruler).
    fn at(&mut self, x: f32, y: f32) -> gpui::Point<Pixels> {
        let o = self.control("group", GRID).bounds.origin;
        point(o.x + px(x), o.y + px(RULER_HEIGHT + y))
    }

    fn down(&mut self, at: gpui::Point<Pixels>) {
        self.cx
            .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn move_to(&mut self, at: gpui::Point<Pixels>) {
        self.cx
            .simulate_mouse_move(at, Some(MouseButton::Left), Modifiers::none());
        self.cx.run_until_parked();
    }

    fn up(&mut self, at: gpui::Point<Pixels>) {
        self.cx
            .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        self.cx.run_until_parked();
    }

    /// The session's edit calls (reads left out).
    fn edit_calls(&self) -> Vec<String> {
        self.fake
            .calls()
            .into_iter()
            .filter(|c| c.contains("edit"))
            .collect()
    }

    fn last_edit_call(&self) -> Option<String> {
        self.edit_calls().last().cloned()
    }

    /// The labels of the `button`s under "Drum Grid".
    fn tree_buttons(&mut self) -> Vec<Element> {
        let g = self.control("group", GRID);
        self.elements()
            .into_iter()
            .filter(|e| {
                e.path.len() > g.path.len() && e.path.starts_with(&g.path) && e.role == "button"
            })
            .collect()
    }

    fn tree_labels(&mut self) -> Vec<String> {
        self.tree_buttons().into_iter().map(|e| e.label).collect()
    }

    /// Neither a stroke nor a gesture is left.
    fn no_stroke(&mut self) -> bool {
        self.model(|d| d.gesture().is_none() && d.drum_stroke().is_none())
    }

    /// A paint stroke held from (5, 50) to (125, 70), its gesture open.
    fn mid_stroke(&mut self) -> gpui::Point<Pixels> {
        let (from, to) = (self.at(5., 50.), self.at(125., 70.));
        self.down(from);
        self.move_to(to);
        assert!(self.model(|d| d.gesture().is_some()), "the gesture opened");
        to
    }

    /// After a stroke was cancelled: no undo step, and the release sends
    /// nothing.
    fn assert_cancelled(&mut self, to: gpui::Point<Pixels>) {
        assert_eq!(self.last_edit_call().as_deref(), Some("abort_edit"));
        assert!(!self.edit_calls().iter().any(|c| c == "end_edit"));
        assert!(self.no_stroke());
        let n = self.edit_calls().len();
        self.up(to);
        assert_eq!(self.edit_calls().len(), n, "the release sends nothing");
    }
}

#[gpui::test]
fn a_click_on_an_empty_cell_paints_it(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let at = s.at(85., 50.);
    s.down(at);
    s.up(at);
    assert_eq!(s.fake.gesture_edits().last(), Some(&snare(&[2 * S16])));
    assert_eq!(
        s.edit_calls(),
        ["begin_edit(Paint Drum Hits)", "edit_gesture", "end_edit"]
    );
    assert_eq!(
        s.model(|d| d.drum_selected_row()),
        Some(DrumRowKey { lane: 40, row: 2 })
    );
}

#[gpui::test]
fn a_drag_paints_a_run_and_escape_takes_it_back(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let to = s.mid_stroke();
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&snare(&[0, S16, 2 * S16, 3 * S16])),
        "the stroke stays on its row"
    );
    s.keys("escape");
    s.assert_cancelled(to);
}

#[gpui::test]
fn the_eraser_erases_the_hits_it_crosses(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    s.update_model(|d| d.set_tool(Tool::Eraser));
    s.fake.clear_calls();
    let (from, to) = (s.at(5., 30.), s.at(165., 30.));
    s.down(from);
    s.move_to(to);
    s.up(to);
    assert_eq!(
        s.edit_calls().first().map(String::as_str),
        Some("begin_edit(Erase Drum Hits)")
    );
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete {
                events: vec![11, 12]
            }
        })
    );
}

#[gpui::test]
fn a_shown_velocity_lane_sets_the_velocity_from_the_height(cx: &mut TestAppContext) {
    let mut s = velocity_grid(cx);
    // The Snare band is 42–90: its top is 127.
    let at = s.at(85., 42.);
    s.down(at);
    s.up(at);
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&paint(&[2 * S16], 127 << 9, 2))
    );
    // Header, Kick, Snare with its lane, header, Clap, new-track band.
    assert_eq!(s.model(|d| d.drum_layout().height), 160.);
}

#[gpui::test]
fn cells_and_hits_are_in_the_tree_and_a_click_toggles(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    assert_eq!(
        s.tree_labels().first().map(String::as_str),
        Some("Kick, bar 1 beat 1, on, velocity 100")
    );
    let off = s
        .tree_buttons()
        .into_iter()
        .find(|e| e.label == "Snare, bar 1 beat 1 tick 7680, off")
        .expect("the empty Snare cell");
    s.click_at(off.bounds.center(), 1);
    assert_eq!(s.fake.gesture_edits().last(), Some(&snare(&[2 * S16])));
    s.update_model(|d| d.update_drum_settings(|g| g.view = DrumView::Timeline));
    assert_eq!(
        s.tree_labels(),
        [
            "Kick, bar 1 beat 1, on, velocity 100",
            "Kick, bar 1 beat 2, on, velocity 100",
            "Snare, bar 1 beat 2, on, velocity 100",
        ]
    );
}

/// Review Focus 1: the detail switched away or closed, the bottom pane
/// on another view or hidden, the window losing the keyboard, closing,
/// and the track removed: no open edit and no undo step stays.
#[gpui::test]
fn leaving_the_pane_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let to = s.mid_stroke();
    s.update_model(|d| d.show_detail(DetailEditorKind::List));
    s.assert_cancelled(to);

    let mut s = drums_grid(cx);
    let to = s.mid_stroke();
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    s.assert_cancelled(to);
}

#[gpui::test]
fn closing_the_detail_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let to = s.mid_stroke();
    s.update_model(|d| d.close_detail());
    s.assert_cancelled(to);
}

#[gpui::test]
fn another_bottom_view_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let to = s.mid_stroke();
    s.update_model(|d| d.update_layout(|l| l.views.current_mut().bottom.view = PaneView::Mixer));
    assert_eq!(
        s.model(|d| d.detail_kind_name()),
        "drum_grid",
        "the detail stays"
    );
    s.assert_cancelled(to);
}

#[gpui::test]
fn hiding_the_bottom_pane_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let to = s.mid_stroke();
    s.update_model(|d| d.update_layout(|l| l.views.current_mut().bottom.shown = false));
    s.assert_cancelled(to);
}

#[gpui::test]
fn closing_the_window_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    s.mid_stroke();
    s.window
        .update(&mut s.cx, |_, window, _| window.remove_window())
        .unwrap();
    s.cx.run_until_parked();
    assert_eq!(s.last_edit_call().as_deref(), Some("abort_edit"));
    assert!(!s.edit_calls().iter().any(|c| c == "end_edit"));
}

/// The stroke's track removed under it, with its gesture open and before
/// the dead zone.
#[gpui::test]
fn removing_the_track_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    fn remove_drums(s: &mut Setup) {
        let mut snap = s.fake.snap();
        snap.version += 1;
        snap.lane_revs.retain(|r| r.lane != 40);
        s.fake.set_idle(Some(snap));
        s.update_model(|d| {
            d.idle();
        });
    }
    let mut s = drums_grid(cx);
    let to = s.mid_stroke();
    remove_drums(&mut s);
    s.assert_cancelled(to);

    let mut s = drums_grid(cx);
    let (from, to) = (s.at(5., 50.), s.at(125., 50.));
    s.down(from);
    remove_drums(&mut s);
    s.move_to(to);
    s.up(to);
    assert!(s.edit_calls().is_empty(), "{:?}", s.edit_calls());
    assert!(s.no_stroke());
}

/// Review Focus 5.
#[gpui::test]
fn a_stroke_reads_the_drum_lanes_once(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let reads = s.fake.drum_lanes_reads();
    let from = s.at(5., 50.);
    s.down(from);
    // 20 moves, all in the first cell (its command does not change).
    for i in 0..20 {
        let at = s.at(9. + i as f32, 50.);
        s.move_to(at);
    }
    assert!(s.model(|d| d.gesture().is_some()));
    assert!(
        s.fake.drum_lanes_reads() <= reads + 1,
        "{} reads",
        s.fake.drum_lanes_reads() - reads
    );
}

/// Review Focus 5: the widest zoom.
#[gpui::test]
fn the_tree_lists_at_most_2048_cells(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    s.update_model(|d| {
        d.update_drum_settings(|g| {
            g.ticks_per_point = 61_440.;
            g.step = 480;
        })
    });
    assert_eq!(s.tree_buttons().len(), MAX_TREE_ITEMS);
}

#[gpui::test]
fn a_ruler_drag_selects_the_range_over_the_drum_tracks(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let (from, to) = (s.at(40., -10.), s.at(200., -10.));
    s.down(from);
    s.move_to(to);
    s.up(to);
    let sel = s.model(|d| d.selection().clone());
    assert!(sel.time_range.is_some());
    assert_eq!(
        sel.lanes,
        [LaneRef { seq: 3, lane: 40 }, LaneRef { seq: 3, lane: 41 }]
    );
    assert!(sel.events.is_empty());
    assert!(s.edit_calls().is_empty());
}

#[gpui::test]
fn the_wheel_zoom_is_saved(cx: &mut TestAppContext) {
    let mut s = drums_grid(cx);
    let before = s.model(|d| d.drum_settings().ticks_per_point);
    let at = s.at(100., 30.);
    s.cx.simulate_event(ScrollEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), px(10.))),
        modifiers: secondary_mods(),
        ..Default::default()
    });
    s.cx.run_until_parked();
    let after = s.model(|d| d.drum_settings().ticks_per_point);
    assert_ne!(after, before);
    let axis = s
        .window
        .read_with(&s.cx, |v, _| v.drum_grid().axis().ticks_per_point())
        .unwrap();
    assert_eq!(axis, after);
}
