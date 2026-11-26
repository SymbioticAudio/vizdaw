//! The track header column: rows, labels, selection, the M/S/R toggles,
//! inline rename and the "+ Add Track" row, driven through GPUI's test
//! platform with the scripted session.

use std::sync::Arc;

use core_control::protocol::{RawRequest, RequestId};
use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext,
    VisualTestContext, WindowHandle, point, px,
};
use serde_json::{Map, Value, json};
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, demo_fake, lane_row};
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::snapshot::TakeRow;
use vizdaw_ffi::tracks::TrackKindRow;
use vizdaw_gpui::control;
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand};

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

    /// The header column's children, in order: (role, label).
    fn header_children(&mut self) -> Vec<(String, String)> {
        let group = self.element("group", "Track Headers");
        self.elements()
            .into_iter()
            .filter(|e| e.path.len() == group.path.len() + 1 && e.path.starts_with(&group.path))
            .map(|e| (e.role, e.label))
            .collect()
    }

    fn click_at(&mut self, at: Point<Pixels>, modifiers: Modifiers) {
        self.cx.simulate_click(at, modifiers);
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.click_at(at, Modifiers::none());
    }

    /// A double-click on a header's name (Swift `LaneHeaderLayout`: the
    /// name's line is the row's top 26 points).
    fn double_click(&mut self, role: &str, label: &str) {
        let row = self.element(role, label).bounds;
        let at = point(row.origin.x + px(60.), row.origin.y + px(13.));
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

    /// `ui.click {element}` over the control channel.
    fn channel_click(&mut self, role: &str, label: &str) -> Value {
        let id = core_control::element::element_id(&self.element(role, label).path, role, label);
        let mut params = Map::new();
        params.insert("element".into(), json!(id));
        let raw = RawRequest {
            id: RequestId::Num(1),
            cmd: "ui.click".into(),
            params,
        };
        let command = core_control::validate(&raw).expect("a valid request");
        let handled = self.cx.cx.update(|cx| control::handle(&command, cx));
        self.cx.run_until_parked();
        match handled {
            control::Handled::Reply(Ok(v)) => v,
            other => panic!("ui.click failed: {other:?}"),
        }
    }

    fn selected(&mut self) -> Vec<u64> {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().selected_lanes()
        })
    }

    fn status(&mut self) -> Option<String> {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().status().map(String::from)
        })
    }

    fn renames(&self) -> Vec<String> {
        self.fake
            .edits()
            .into_iter()
            .filter_map(|e| match e {
                EditCommand::Lane {
                    edit: LaneEdit::Rename { name, .. },
                } => Some(name),
                _ => None,
            })
            .collect()
    }
}

const GROOVE: &str = "MIDI track, Groove, no destination";
const CHORDS: &str = "MIDI track, Chords, no destination";

#[gpui::test]
fn the_headers_and_add_row_show_beside_the_canvas(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    assert_eq!(
        s.header_children(),
        vec![
            ("button".to_string(), GROOVE.to_string()),
            ("button".to_string(), CHORDS.to_string()),
            ("button".to_string(), "Add Track".to_string()),
        ]
    );
    // Under the transport bar, 200 px wide, a row under the one before.
    let group = s.element("group", "Track Headers");
    let play = s.element("button", "Play");
    assert!(group.bounds.origin.y >= play.bounds.bottom());
    assert_eq!(group.bounds.size.width, px(200.));
    let (a, b) = (s.element("button", GROOVE), s.element("button", CHORDS));
    assert!(b.bounds.origin.y > a.bounds.origin.y);
    assert_eq!(a.bounds.size.width, px(200.));
}

#[gpui::test]
fn the_header_names_kind_lane_and_routing(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let mut audio = lane_row(30, "Vocal", true);
    audio.kind = TrackKindRow::Audio;
    audio.audio_out_name = Some("Main Out".into());
    let mut silent = lane_row(31, "Room", false);
    silent.kind = TrackKindRow::Audio;
    silent.solo = true;
    let mut synth = lane_row(32, "Pad", false);
    synth.kind = TrackKindRow::Instrument;
    synth.destination = Some("Synth 1".into());
    synth.takes = vec![
        TakeRow {
            id: 1,
            name: "Take 1".into(),
            midi_event_count: 0,
            region_count: 0,
        },
        TakeRow {
            id: 2,
            name: "Take 2".into(),
            midi_event_count: 0,
            region_count: 0,
        },
    ];
    synth.active_take_id = 2;
    let mut midi = lane_row(33, "Lead", true);
    midi.solo = true;
    midi.arm = true;
    fake.set_lanes(3, vec![audio, silent, synth, midi]);
    let mut s = setup(cx, fake);
    let labels: Vec<String> = s.header_children().into_iter().map(|(_, l)| l).collect();
    assert_eq!(
        labels,
        vec![
            "Audio track, Vocal, output Main Out, muted",
            "Audio track, Room, output none, soloed",
            "Instrument track, Pad \u{b7} Take 2, Synth 1",
            "MIDI track, Lead, no destination, muted, soloed, armed",
            "Add Track",
        ]
    );
}

#[gpui::test]
fn a_header_click_selects_its_track(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("button", CHORDS);
    assert_eq!(s.selected(), vec![31]);
    s.click("button", GROOVE);
    assert_eq!(s.selected(), vec![30]);
    // Shift adds a track.
    let at = s.element("button", CHORDS).bounds.center();
    s.click_at(at, Modifiers::shift());
    assert_eq!(s.selected(), vec![30, 31]);
    // M toggles mute and leaves the selection.
    let row = s.element("button", GROOVE).bounds;
    let mute = point(row.right() - px(60.), row.origin.y + px(13.));
    s.click_at(mute, Modifiers::none());
    assert_eq!(s.selected(), vec![30, 31]);
    let flags: Vec<_> = s
        .fake
        .edits()
        .into_iter()
        .filter_map(|e| match e {
            EditCommand::Lane {
                edit:
                    LaneEdit::SetFlags {
                        lane,
                        mute,
                        solo,
                        arm,
                        ..
                    },
            } => Some((lane, mute, solo, arm)),
            _ => None,
        })
        .collect();
    assert_eq!(flags, vec![(30, Some(true), None, None)]);
}

#[gpui::test]
fn a_channel_click_on_add_track_opens_no_menu(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.channel_click("button", "Add Track");
    assert_eq!(
        s.status().as_deref(),
        Some(
            "Add Track opens a menu: use track.new_audio, track.new_instrument or \
             track.new_midi over the control channel."
        )
    );
    assert!(s.find("menuitem", "New Audio Track").is_none());
    // A mouse click opens the menu; an item runs its command.
    s.click("button", "Add Track");
    assert!(s.find("menuitem", "New Audio Track").is_some());
    assert!(s.find("menuitem", "New Instrument Track").is_some());
    assert!(s.find("menuitem", "New MIDI Track").is_some());
    s.click("menuitem", "New MIDI Track");
    assert_eq!(s.fake.count("new_track(3,Midi,None)"), 1);
    assert!(s.find("menuitem", "New Audio Track").is_none());
}

#[gpui::test]
fn a_channel_press_on_a_row_toggles_mute(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.channel_click("button", CHORDS);
    let muted = "MIDI track, Chords, no destination, muted";
    assert!(s.find("button", muted).is_some());
    // The selection is left alone.
    assert!(s.selected().is_empty());
    s.channel_click("button", muted);
    assert!(s.find("button", CHORDS).is_some());
}

#[gpui::test]
fn rename_commits_a_trimmed_name(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.double_click("button", CHORDS);
    assert!(s.window.read_with(&s.cx, |v, _| v.renaming()).unwrap() == Some(31));
    // The field holds the name, all selected: typing replaces it.
    s.cx.simulate_input("  Keys  ");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.renames(), vec!["Keys".to_string()]);
    assert!(
        s.find("button", "MIDI track, Keys, no destination")
            .is_some()
    );
    assert_eq!(
        s.window.read_with(&s.cx, |v, _| v.renaming()).unwrap(),
        None
    );
    // The keys work again.
    s.cx.simulate_keystrokes("space");
    assert_eq!(s.fake.count("play(3)"), 1);
    // A blank name sends nothing.
    s.window
        .update(&mut s.cx, |v, window, cx| v.begin_rename(30, window, cx))
        .unwrap();
    s.cx.run_until_parked();
    s.cx.simulate_keystrokes("backspace enter");
    s.cx.run_until_parked();
    assert_eq!(s.renames(), vec!["Keys".to_string()]);
}

#[gpui::test]
fn escape_in_the_rename_field_cancels(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.window
        .update(&mut s.cx, |v, window, cx| v.begin_rename(30, window, cx))
        .unwrap();
    s.cx.run_until_parked();
    s.cx.simulate_input("Drums");
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert!(s.renames().is_empty());
    assert_eq!(
        s.window.read_with(&s.cx, |v, _| v.renaming()).unwrap(),
        None
    );
    s.cx.simulate_keystrokes("space");
    assert_eq!(s.fake.count("play(3)"), 1);
}

#[gpui::test]
fn focus_loss_commits_the_rename(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.window
        .update(&mut s.cx, |v, window, cx| v.begin_rename(30, window, cx))
        .unwrap();
    s.cx.run_until_parked();
    s.cx.simulate_input("Drums");
    // A click on the empty canvas takes the focus.
    s.click_at(point(px(700.), px(500.)), Modifiers::none());
    assert_eq!(s.renames(), vec!["Drums".to_string()]);
    assert_eq!(
        s.window.read_with(&s.cx, |v, _| v.renaming()).unwrap(),
        None
    );
}

#[gpui::test]
fn a_header_torn_down_mid_rename_re_enables_transport_keys(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("button", GROOVE);
    s.window
        .update(&mut s.cx, |v, window, cx| v.begin_rename(30, window, cx))
        .unwrap();
    s.cx.run_until_parked();
    s.cx.simulate_input("Gone");
    // The track goes while its name is being edited (as an Undo would).
    s.cx.dispatch_action(RunCommand::new("track.delete"));
    s.cx.run_until_parked();
    assert_eq!(
        s.window.read_with(&s.cx, |v, _| v.renaming()).unwrap(),
        None
    );
    assert!(s.renames().is_empty());
    assert_eq!(
        s.header_children(),
        vec![
            ("button".to_string(), CHORDS.to_string()),
            ("button".to_string(), "Add Track".to_string()),
        ]
    );
    s.cx.simulate_keystrokes("space");
    assert_eq!(s.fake.count("play(3)"), 1);
}

#[gpui::test]
fn delete_the_only_track_leaves_the_add_row(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_lanes(3, vec![lane_row(30, "Solo", false)]);
    let mut s = setup(cx, fake);
    s.click("button", "MIDI track, Solo, no destination");
    s.cx.dispatch_action(RunCommand::new("track.delete"));
    s.cx.run_until_parked();
    assert_eq!(
        s.header_children(),
        vec![("button".to_string(), "Add Track".to_string())]
    );
    // And it still adds one.
    s.channel_click("button", "Add Track");
    s.click("button", "Add Track");
    s.click("menuitem", "New Audio Track");
    assert_eq!(s.fake.count("new_track(3,Audio,None)"), 1);
}

#[gpui::test]
fn the_rename_command_opens_the_field_on_the_selected_track(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("button", CHORDS);
    s.cx.dispatch_action(RunCommand::new("track.rename"));
    s.cx.run_until_parked();
    assert_eq!(
        s.window.read_with(&s.cx, |v, _| v.renaming()).unwrap(),
        Some(31)
    );
    // Single keys type into it.
    s.cx.simulate_input("s ");
    assert_eq!(s.fake.count("play(3)"), 0);
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.renames(), vec!["s".to_string()]);
}

impl Setup {
    /// Sets the document's status as an editor action would.
    fn set_status(&mut self, text: &str) {
        let doc = self.doc();
        let text = text.to_owned();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| d.set_status(Some(text)));
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    fn wait(&mut self, seconds: f32) {
        self.cx
            .executor()
            .advance_clock(std::time::Duration::from_secs_f32(seconds));
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn the_status_line_clears_six_seconds_after_its_text_appears(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.set_status("First");
    s.wait(5.);
    assert_eq!(s.status().as_deref(), Some("First"));
    // A new message restarts the six seconds.
    s.set_status("Second");
    s.wait(5.);
    assert_eq!(s.status().as_deref(), Some("Second"));
    s.wait(1.5);
    assert_eq!(s.status(), None);
    assert!(s.find("button", "Dismiss").is_none());
}

#[gpui::test]
fn the_status_line_dismiss_button_clears_it(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    assert!(s.find("button", "Dismiss").is_none());
    s.channel_click("button", "Add Track");
    assert!(s.status().is_some());
    s.click("button", "Dismiss");
    assert_eq!(s.status(), None);
    assert!(s.find("button", "Dismiss").is_none());
}

#[gpui::test]
fn the_header_column_scrolls_to_reach_every_track(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let many: Vec<_> = (0..20)
        .map(|i| lane_row(100 + i, &format!("Track {i}"), false))
        .collect();
    fake.set_lanes(3, many);
    let mut s = setup(cx, fake);
    let viewport = s.cx.update(|window, _| window.viewport_size());
    let group = s.element("group", "Track Headers");
    let add = s.element("button", "Add Track");
    // More tracks than fit: "+ Add Track" is drawn below the window.
    assert!(add.bounds.origin.y >= viewport.height, "{add:?}");
    s.cx.simulate_event(gpui::ScrollEvent {
        position: group.bounds.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-2000.))),
        ..Default::default()
    });
    s.cx.run_until_parked();
    // The column stays put; its rows move up with the scroll.
    assert_eq!(s.element("group", "Track Headers").bounds, group.bounds);
    let add = s.element("button", "Add Track");
    assert!(add.bounds.bottom() <= viewport.height, "{add:?}");
    assert!(add.bounds.origin.y >= group.bounds.origin.y, "{add:?}");
    let first = s.element("button", "MIDI track, Track 0, no destination");
    assert!(first.bounds.bottom() <= group.bounds.origin.y, "{first:?}");
    // And it works where it now is.
    s.click("button", "Add Track");
    assert!(s.find("menuitem", "New Audio Track").is_some());
}

impl Setup {
    fn renaming(&self) -> Option<u64> {
        self.window
            .read_with(&self.cx, |v, _| v.renaming())
            .unwrap()
    }

    fn begin_rename(&mut self, lane: u64) {
        self.window
            .update(&mut self.cx, |v, window, cx| {
                v.begin_rename(lane, window, cx)
            })
            .unwrap();
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn starting_a_second_rename_commits_the_first(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("button", CHORDS);
    s.begin_rename(30);
    s.cx.simulate_input("Drums");
    // The Rename command (its menu item) on the selected track, while the
    // field has the keyboard.
    s.cx.dispatch_action(RunCommand::new("track.rename"));
    s.cx.run_until_parked();
    assert_eq!(s.renames(), vec!["Drums".to_string()]);
    assert_eq!(s.renaming(), Some(31));
    // A double-click on another track's name.
    s.cx.simulate_input("Keys");
    s.double_click("button", "MIDI track, Drums, no destination");
    assert_eq!(s.renames(), vec!["Drums".to_string(), "Keys".to_string()]);
    assert_eq!(s.renaming(), Some(30));
}

#[gpui::test]
fn a_click_elsewhere_in_the_window_commits_the_rename(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    // Under "+ Add Track", in the header column.
    let add = s.element("button", "Add Track").bounds;
    let below = point(add.center().x, add.bottom() + px(100.));
    // The transport bar, left of its first button.
    let play = s.element("button", "Play").bounds;
    let bar = point(play.origin.x - px(4.), play.center().y);
    // The status line's text.
    s.set_status("Something");
    let dismiss = s.element("button", "Dismiss").bounds;
    let status = point(px(20.), dismiss.center().y);
    for (i, at) in [below, bar, status].into_iter().enumerate() {
        s.begin_rename(30);
        let name = format!("Name {i}");
        s.cx.simulate_input(&name);
        s.click_at(at, Modifiers::none());
        assert_eq!(s.renames().last(), Some(&name), "click {i} at {at:?}");
        assert_eq!(s.renaming(), None, "click {i}");
    }
}

impl Setup {
    /// The next edit leaves the song edited (the fake's edits keep the
    /// snapshot as it is otherwise).
    fn next_edit_dirties(&self) {
        let mut n = self.fake.snap();
        n.version += 1;
        n.dirty = true;
        n.undo_label = Some("Rename Track".into());
        self.fake.bump(n);
    }

    /// A rename of Groove open with "Drums" typed, on a clean song.
    fn typed_rename(&mut self) {
        self.begin_rename(30);
        self.cx.simulate_input("Drums");
        self.next_edit_dirties();
        assert!(self.renames().is_empty());
    }

    /// The fake's calls from the rename edit on: the edit, then the rest.
    fn calls_from_edit(&self) -> Vec<String> {
        let calls = self.fake.calls();
        let at = calls
            .iter()
            .position(|c| c == "edit")
            .unwrap_or_else(|| panic!("no edit in {calls:?}"));
        calls[at..].to_vec()
    }

    fn control_call(&mut self, cmd: &str, params: Value) -> control::Handled {
        let raw = RawRequest {
            id: RequestId::Num(1),
            cmd: cmd.into(),
            params: params.as_object().cloned().unwrap_or_default(),
        };
        let command = core_control::validate(&raw).expect("a valid request");
        let handled = self.cx.cx.update(|cx| control::handle(&command, cx));
        self.cx.run_until_parked();
        handled
    }
}

#[gpui::test]
fn close_commits_an_open_rename_before_asking(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.typed_rename();
    s.cx.dispatch_action(vizdaw_gpui::CloseWindow);
    s.cx.run_until_parked();
    assert_eq!(s.renames(), vec!["Drums".to_string()]);
    // The song is now unsaved: the close asks.
    assert!(s.cx.has_pending_prompt());
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
    assert_eq!(s.app.read_with(&s.cx, |a, _| a.windows().len()), 1);
}

#[gpui::test]
fn the_close_button_commits_an_open_rename_before_asking(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.typed_rename();
    let closed = s.cx.simulate_close();
    s.cx.run_until_parked();
    assert!(!closed);
    assert_eq!(s.renames(), vec!["Drums".to_string()]);
    assert!(s.cx.has_pending_prompt());
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
}

#[gpui::test]
fn save_commits_an_open_rename_first(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Song.vizdaw");
    fake.set_path(Some(path.to_str().unwrap()));
    let mut s = setup(cx, fake);
    s.typed_rename();
    s.cx.dispatch_action(vizdaw_gpui::Save);
    s.cx.run_until_parked();
    assert_eq!(s.renames(), vec!["Drums".to_string()]);
    // Save hands the editor layout (`ui.json`) to the session first.
    assert_eq!(s.calls_from_edit(), vec!["edit", "set_ui_layout", "save"]);
    assert_eq!(s.renaming(), None);
}

#[gpui::test]
fn save_as_commits_an_open_rename_first(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let dir = tempfile::tempdir().unwrap();
    let pick = dir.path().join("New");
    s.typed_rename();
    s.cx.dispatch_action(vizdaw_gpui::SaveAs);
    s.cx.run_until_parked();
    s.cx.simulate_new_path_selection(move |_| Some(pick.clone()));
    s.cx.run_until_parked();
    assert_eq!(s.renames(), vec!["Drums".to_string()]);
    let saved = format!("save_as({})", dir.path().join("New.vizdaw").display());
    let calls = s.calls_from_edit();
    assert_eq!(calls.first().map(String::as_str), Some("edit"));
    assert!(calls.contains(&saved), "{calls:?}");
}

#[gpui::test]
fn quit_commits_an_open_rename_before_asking(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.typed_rename();
    s.cx.dispatch_action(vizdaw_gpui::Quit);
    s.cx.run_until_parked();
    assert_eq!(s.renames(), vec!["Drums".to_string()]);
    // The song is now unsaved: Quit asks.
    assert!(s.cx.has_pending_prompt());
    assert_eq!(s.fake.count("close"), 0);
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
}

#[gpui::test]
fn control_quit_commits_an_open_rename_before_refusing(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.typed_rename();
    let handled = s.control_call("app.quit", json!({}));
    assert_eq!(s.renames(), vec!["Drums".to_string()]);
    match handled {
        control::Handled::Reply(Err(e)) => {
            assert_eq!(e.code, core_control::error::ErrorCode::UnsavedChanges)
        }
        other => panic!("app.quit should refuse: {other:?}"),
    }
    assert_eq!(s.fake.count("close"), 0);
}

// ---- Slice 8c: the monitor button, the input row and picker, the meter ----

use std::time::{Duration, Instant};

use vizdaw_app::lane_inputs::LaneInputs;
use vizdaw_ffi::InputInfoRow;
use vizdaw_ffi::edit::{LaneInputRow, MonitorModeRow};
use vizdaw_gpui::track_headers;

fn audio(id: u64, name: &str, input: Option<LaneInputRow>, monitor: MonitorModeRow) -> LaneRowT {
    let mut l = lane_row(id, name, false);
    l.kind = TrackKindRow::Audio;
    l.audio_out = Some(7);
    l.audio_out_name = Some("Out 1".into());
    l.input = input;
    l.monitor = monitor;
    l
}

type LaneRowT = vizdaw_ffi::snapshot::LaneRow;

fn mono(first: u16) -> Option<LaneInputRow> {
    Some(LaneInputRow::Audio {
        first,
        stereo: false,
        return_of: None,
    })
}

fn input_info(opening: bool) -> InputInfoRow {
    InputInfoRow {
        device: "Fake".into(),
        channels: 2,
        names: vec!["L".into(), "R".into()],
        input_latency_frames: 0,
        output_latency_frames: 0,
        buffer_frames: 0,
        round_trip_frames: 0,
        opening,
        sample_rate: 0.0,
        midi_fx_dropped: 0,
        per_note_dropped: 0,
        mpe_stolen: 0,
    }
}

impl Setup {
    /// Row `label`'s own children, in order: (role, label, value).
    fn row_children(&mut self, label: &str) -> Vec<(String, String, Option<String>)> {
        let row = self.element("button", label);
        self.elements()
            .into_iter()
            .filter(|e| e.path.len() == row.path.len() + 1 && e.path.starts_with(&row.path))
            .map(|e| (e.role, e.label, e.value))
            .collect()
    }

    /// The header row labels, in order (without "Add Track").
    fn row_labels(&mut self) -> Vec<String> {
        self.header_children()
            .into_iter()
            .map(|(_, l)| l)
            .filter(|l| l != "Add Track")
            .collect()
    }

    fn with_doc(&mut self, f: impl FnOnce(&mut vizdaw_app::DocumentModel)) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| f(d));
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    /// The fake's lanes changed: a new snapshot version fetches them.
    fn relanes(&mut self) {
        let mut n = self.fake.snap();
        n.version += 1;
        self.with_doc(|d| d.apply(n));
    }

    fn menuitems(&mut self) -> Vec<(String, bool, bool)> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "menuitem")
            .map(|e| (e.label, e.enabled, e.selected))
            .collect()
    }

    fn picker_open(&mut self) -> bool {
        self.window
            .read_with(&self.cx, |v, _| v.track_headers().input_picker())
            .unwrap()
            .is_some()
    }

    fn monitor_edits(&self) -> Vec<(u64, MonitorModeRow)> {
        self.fake
            .edits()
            .into_iter()
            .filter_map(|e| match e {
                EditCommand::Lane {
                    edit: LaneEdit::SetMonitor { lane, monitor, .. },
                } => Some((lane, monitor)),
                _ => None,
            })
            .collect()
    }

    fn input_edits(&self) -> Vec<(u64, Option<LaneInputRow>)> {
        self.fake
            .edits()
            .into_iter()
            .filter_map(|e| match e {
                EditCommand::Lane {
                    edit: LaneEdit::SetInput { lane, input, .. },
                } => Some((lane, input)),
                _ => None,
            })
            .collect()
    }
}

const AUTO_HELP: &str =
    "Input monitoring: Auto (heard while armed and stopped, or recording in range)";
const INPUT_HELP: &str = "Input monitoring: always the input";
const OFF_HELP: &str = "Input monitoring: off";

/// The header's words for each input, return and monitor case (Swift
/// `ArrangementA11y.header`), and ", input opening" while the input opens
/// on a metered track.
#[gpui::test]
fn the_header_words_name_the_input_its_return_and_monitoring(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let mut keys = lane_row(33, "Keys", false);
    keys.input = Some(LaneInputRow::Midi {
        port: Some("Pads".into()),
        channel: Some(2),
    });
    keys.monitor = MonitorModeRow::Off;
    let ret = Some(LaneInputRow::Audio {
        first: 2,
        stereo: true,
        return_of: Some(5),
    });
    fake.set_destinations(vec![vizdaw_app::testing::dest_row(5, "Moog")]);
    fake.set_lanes(
        3,
        vec![
            audio(30, "Plain", None, MonitorModeRow::Auto),
            audio(31, "Vocal", mono(0), MonitorModeRow::Input),
            audio(32, "Return", ret, MonitorModeRow::Auto),
            keys,
        ],
    );
    let mut s = setup(cx, fake);
    assert_eq!(
        s.row_labels(),
        vec![
            "Audio track, Plain, output Out 1",
            "Audio track, Vocal, output Out 1, input In 1, monitoring input",
            "Audio track, Return, output Out 1, input In 3+4, recording the return of Moog",
            "MIDI track, Keys, no destination, input Pads \u{b7} Ch 3, monitoring off",
        ]
    );
    // Armed and opening: the meter shows, and the label says so.
    let mut armed = audio(31, "Vocal", mono(0), MonitorModeRow::Auto);
    armed.arm = true;
    s.fake.set_lanes(3, vec![armed]);
    s.fake.set_input(input_info(true));
    s.relanes();
    s.with_doc(|d| {
        d.poll_meters(Instant::now());
    });
    assert_eq!(
        s.row_labels(),
        vec!["Audio track, Vocal, output Out 1, armed, input In 1, input opening"]
    );
    s.fake.set_input(input_info(false));
    s.with_doc(|d| {
        d.poll_meters(Instant::now());
    });
    assert_eq!(
        s.row_labels(),
        vec!["Audio track, Vocal, output Out 1, armed, input In 1"]
    );
}

/// The tree names a header's monitor button (its help, also its tooltip
/// and value), its input row (`Input of <track>`, its text the value) and,
/// while the track meters, its input meter.
#[gpui::test]
fn the_tree_names_the_monitor_button_input_row_and_meter(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let mut vocal = audio(30, "Vocal", mono(1), MonitorModeRow::Input);
    vocal.arm = true;
    fake.set_lanes(3, vec![vocal, audio(31, "Room", None, MonitorModeRow::Off)]);
    let mut s = setup(cx, fake);
    let vocal = "Audio track, Vocal, output Out 1, armed, input In 2, monitoring input";
    assert_eq!(
        s.row_children(vocal),
        vec![
            ("button".into(), INPUT_HELP.into(), Some(INPUT_HELP.into())),
            ("menu".into(), "Input of Vocal".into(), Some("In 2".into())),
            (
                "level".into(),
                "Input level of Vocal".into(),
                Some("0.00".into())
            ),
        ]
    );
    // No meter on a track that is neither armed nor monitoring its input.
    assert_eq!(
        s.row_children("Audio track, Room, output Out 1, monitoring off"),
        vec![
            ("button".into(), OFF_HELP.into(), Some(OFF_HELP.into())),
            (
                "menu".into(),
                "Input of Room".into(),
                Some("No Input".into())
            ),
        ]
    );
    // The children sit where Swift's `LaneHeaderLayout` puts them.
    let row = s.element("button", vocal).bounds;
    let monitor = s.element("button", INPUT_HELP).bounds;
    assert_eq!(
        (
            monitor.origin.x - row.origin.x,
            monitor.origin.y - row.origin.y
        ),
        (px(113.), px(4.))
    );
    assert_eq!(monitor.size, gpui::size(px(18.), px(18.)));
    let input = s.element("menu", "Input of Vocal").bounds;
    assert_eq!(
        (input.origin.x - row.origin.x, input.origin.y - row.origin.y),
        (px(104.), px(24.))
    );
    let meter = s.element("level", "Input level of Vocal").bounds;
    assert_eq!(
        (meter.origin.x - row.origin.x, meter.origin.y - row.origin.y),
        (px(8.), px(43.))
    );
    assert_eq!(meter.size, gpui::size(px(178.), px(3.)));
}

/// Swift `theMonitorButtonHasItsTooltip`: the monitor button explains its
/// mode (its tooltip, which the tree shows as its value: GPUI's test
/// platform does not expose a shown tooltip); the name has none.
#[gpui::test]
fn the_monitor_button_has_its_tooltip(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let mut chords = lane_row(31, "Chords", false);
    chords.monitor = MonitorModeRow::Input;
    fake.set_lanes(3, vec![lane_row(30, "Groove", false), chords]);
    let mut s = setup(cx, fake);
    let row = "MIDI track, Chords, no destination, monitoring input";
    let kids = s.row_children(row);
    assert_eq!(
        kids[0],
        ("button".into(), INPUT_HELP.into(), Some(INPUT_HELP.into()))
    );
    assert!(kids.iter().all(|(_, l, _)| l != "Chords"), "{kids:?}");
    assert_eq!(s.row_children(GROOVE)[0].2.as_deref(), Some(AUTO_HELP));
}

/// Swift `theMeterHidesUnder46PointsAndTheMonitorClickCycles`: the meter
/// needs 46 points of row; a click on the monitor button is one lane edit
/// to the next mode and leaves the selection.
#[gpui::test]
fn the_meter_hides_under_46_points_and_the_monitor_click_cycles(cx: &mut TestAppContext) {
    assert!(!track_headers::shows_meter(45.));
    for h in [46., 47., 48., 96.] {
        assert!(track_headers::shows_meter(h), "{h}");
    }
    let mut s = setup(cx, demo_fake());
    let monitor = s.row_children(CHORDS)[0].clone();
    assert_eq!(monitor.1, AUTO_HELP);
    // Row 1's button: the first labelled so under Chords.
    let row = s.element("button", CHORDS);
    let at = s
        .elements()
        .into_iter()
        .find(|e| e.path.starts_with(&row.path) && e.label == AUTO_HELP)
        .unwrap()
        .bounds
        .center();
    s.click_at(at, Modifiers::none());
    assert_eq!(s.monitor_edits(), vec![(31, MonitorModeRow::Input)]);
    assert!(s.selected().is_empty(), "the selection is left alone");
    // The control channel presses it the same way.
    let mut chords = lane_row(31, "Chords", false);
    chords.monitor = MonitorModeRow::Input;
    s.fake
        .set_lanes(3, vec![lane_row(30, "Groove", false), chords]);
    s.relanes();
    let row = "MIDI track, Chords, no destination, monitoring input";
    let path = s.element("button", row).path;
    let id = s
        .elements()
        .into_iter()
        .find(|e| e.path.starts_with(&path) && e.label == INPUT_HELP)
        .map(|e| core_control::element::element_id(&e.path, "button", INPUT_HELP))
        .unwrap();
    let mut params = Map::new();
    params.insert("element".into(), json!(id));
    let command = core_control::validate(&RawRequest {
        id: RequestId::Num(1),
        cmd: "ui.click".into(),
        params,
    })
    .unwrap();
    s.cx.cx.update(|cx| control::handle(&command, cx));
    s.cx.run_until_parked();
    assert_eq!(
        s.monitor_edits(),
        vec![(31, MonitorModeRow::Input), (31, MonitorModeRow::Off)]
    );
}

/// Swift `monitorTooltipsCoverTheVisibleRowsAndFollowAScroll`: of 200
/// tracks, only the rows in view show a monitor button (each with its own
/// tooltip), and a scroll moves them: row 10's button is where row 0's
/// was, explaining row 10's mode.
#[gpui::test]
fn monitor_tooltips_cover_the_visible_rows_and_follow_a_scroll(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let rows: Vec<_> = (0..200)
        .map(|i| {
            let mut l = lane_row(1000 + i, &format!("T{i}"), false);
            l.monitor = if i == 10 {
                MonitorModeRow::Input
            } else {
                MonitorModeRow::Off
            };
            l
        })
        .collect();
    fake.set_lanes(3, rows);
    let mut s = setup(cx, fake);
    let shown = |s: &mut Setup| -> Vec<vizdaw_gpui::registry::Element> {
        let registry = s
            .window
            .read_with(&s.cx, |v, _| v.registry().clone())
            .unwrap();
        let r = registry.borrow();
        r.elements()
            .iter()
            .filter(|e| e.label.starts_with("Input monitoring") && r.visible_part(e).is_some())
            .cloned()
            .collect()
    };
    let group = s.element("group", "Track Headers").bounds;
    let visible = (f32::from(group.size.height) / track_headers::ROW_HEIGHT).ceil() as usize + 1;
    let before = shown(&mut s);
    assert!(
        !before.is_empty() && before.len() <= visible,
        "{} buttons",
        before.len()
    );
    let first = before[0].bounds;
    assert_eq!(before[0].label, OFF_HELP);
    s.cx.simulate_event(gpui::ScrollEvent {
        position: group.center(),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-10. * track_headers::ROW_HEIGHT))),
        ..Default::default()
    });
    s.cx.run_until_parked();
    let after = shown(&mut s);
    assert!(after.len() <= visible);
    let at = after
        .iter()
        .find(|e| e.bounds == first)
        .expect("a button where row 0's was");
    assert_eq!(at.label, INPUT_HELP);
    assert_eq!(at.value.as_deref(), Some(INPUT_HELP));
    assert!(
        after
            .iter()
            .all(|e| e.bounds.bottom() > group.origin.y && e.bounds.origin.y < group.bottom()),
        "only rows in view"
    );
}

/// The picker: a click on the input row opens it (reading the input
/// again) under the row, with Swift's sections in order and a ✓ on the
/// track's input; a choice is one lane edit, one undo step, and closes
/// it; the current input again makes none; Escape and a click outside
/// close it.
#[gpui::test]
fn the_input_picker_lists_sections_checks_the_input_and_chooses(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_lanes(3, vec![audio(30, "Vocal", mono(1), MonitorModeRow::Auto)]);
    fake.set_input(input_info(false));
    let mut s = setup(cx, fake);
    assert!(!s.picker_open());
    s.click("menu", "Input of Vocal");
    assert!(s.picker_open());
    // The press selected the track too (Swift).
    assert_eq!(s.selected(), vec![30]);
    // The engine's input was read as the picker opened: its names.
    assert_eq!(
        s.menuitems(),
        vec![
            ("No Input".into(), true, false),
            ("Audio".into(), false, false),
            ("In 1 (L)".into(), true, false),
            ("In 2 (R)".into(), true, true),
            ("In 1+2".into(), true, false),
        ]
    );
    // Under the input row.
    let input = s.element("menu", "Input of Vocal").bounds;
    let first = s.element("menuitem", "No Input").bounds;
    assert!(
        first.origin.y >= input.bottom(),
        "{first:?} under {input:?}"
    );
    // The current input again: no edit; the picker closes.
    s.click("menuitem", "In 2 (R)");
    assert!(!s.picker_open());
    assert!(s.input_edits().is_empty());
    assert!(s.find("menuitem", "No Input").is_none());
    // A choice: one edit, its undo step the core's.
    s.click("menu", "Input of Vocal");
    s.click("menuitem", "In 1+2");
    assert!(!s.picker_open());
    assert_eq!(
        s.input_edits(),
        vec![(
            30,
            Some(LaneInputRow::Audio {
                first: 0,
                stereo: true,
                return_of: None
            })
        )]
    );
    assert_eq!(s.fake.snap().undo_label.as_deref(), Some("Set Track Input"));
    // Escape closes it.
    s.click("menu", "Input of Vocal");
    assert!(s.picker_open());
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert!(!s.picker_open());
    // So does a click outside, which reaches nothing else.
    s.click("menu", "Input of Vocal");
    s.click_at(point(px(700.), px(500.)), Modifiers::none());
    assert!(!s.picker_open());
    assert!(s.find("menuitem", "No Input").is_none());
    assert!(
        s.find("button", "Audio track, Vocal, output Out 1, input In 1+2")
            .is_some()
    );
    assert_eq!(s.input_edits().len(), 1);
}

/// A MIDI track's picker: "MIDI Input" (all, then each source) and "MIDI
/// Channel" sections, the track's port and channel checked.
#[gpui::test]
fn a_midi_track_picker_lists_sources_and_channels(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let mut keys = lane_row(31, "Keys", false);
    keys.midi_dest = Some(1);
    keys.input = Some(LaneInputRow::Midi {
        port: None,
        channel: Some(0),
    });
    fake.set_lanes(3, vec![keys]);
    let mut s = setup(cx, fake);
    s.click("menu", "Input of Keys");
    let items = s.menuitems();
    let titles: Vec<&str> = items.iter().map(|(t, _, _)| t.as_str()).collect();
    assert_eq!(&titles[..3], &["No Input", "MIDI Input", "All MIDI Inputs"]);
    let channel = titles.iter().position(|t| *t == "MIDI Channel").unwrap();
    assert_eq!(titles[channel + 1], "Any Channel");
    assert_eq!(titles[channel + 2], "Channel 1");
    assert_eq!(titles.last(), Some(&"Channel 16"));
    let checked: Vec<&str> = items
        .iter()
        .filter(|(_, _, on)| *on)
        .map(|(t, _, _)| t.as_str())
        .collect();
    assert_eq!(checked, vec!["All MIDI Inputs", "Channel 1"]);
}

/// While a pass records, the core refuses an input change: the status
/// line says why and nothing changes.
#[gpui::test]
fn a_choice_while_recording_shows_busy(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let mut vocal = audio(30, "Vocal", mono(0), MonitorModeRow::Auto);
    vocal.arm = true;
    fake.set_lanes(3, vec![vocal]);
    let mut s = setup(cx, fake);
    s.cx.dispatch_action(RunCommand::new("transport.record"));
    s.cx.run_until_parked();
    s.click("menu", "Input of Vocal");
    s.click("menuitem", "In 2");
    assert!(s.input_edits().is_empty());
    assert_eq!(
        s.status().as_deref(),
        Some(vizdaw_ffi::session::RECORDING_LANES)
    );
}

/// The meter follows the poll's scripted peaks; the clip mark lights at
/// full scale and goes 2 s later (the poll's clock is the test's).
#[gpui::test]
fn the_meter_follows_scripted_peaks_and_holds_the_clip_two_seconds(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let mut vocal = audio(30, "Vocal", mono(0), MonitorModeRow::Auto);
    vocal.arm = true;
    fake.set_lanes(3, vec![vocal]);
    let mut s = setup(cx, fake);
    let meter = |s: &mut Setup| s.value_of("level", "Input level of Vocal");
    let t0 = Instant::now();
    s.fake.set_input_levels(vec![0.5]);
    s.with_doc(|d| {
        d.poll_meters(t0);
    });
    assert_eq!(meter(&mut s).as_deref(), Some("0.50"));
    let half = s.element("level", "Input level of Vocal");
    s.fake.set_input_levels(vec![1.0]);
    s.with_doc(|d| {
        d.poll_meters(t0 + Duration::from_millis(100));
    });
    assert_eq!(meter(&mut s).as_deref(), Some("1.00, clipped"));
    // Still lit just inside the hold, falling meanwhile.
    s.with_doc(|d| {
        d.poll_meters(t0 + Duration::from_millis(2000));
    });
    let v = meter(&mut s).unwrap();
    assert!(v.ends_with(", clipped"), "{v}");
    // Gone after it.
    s.with_doc(|d| {
        d.poll_meters(t0 + Duration::from_millis(2200));
    });
    assert!(!meter(&mut s).unwrap().contains("clipped"));
    assert_eq!(half.bounds.size.width, px(178.));
    // Disarmed: the meter goes.
    let vocal = audio(30, "Vocal", mono(0), MonitorModeRow::Auto);
    s.fake.set_lanes(3, vec![vocal]);
    s.relanes();
    assert!(s.find("level", "Input level of Vocal").is_none());
}

/// The views read the return badge's names from the model's cache: a
/// draw makes no session call (slice 8c Task 2 carry).
#[gpui::test]
fn drawing_the_headers_makes_no_destinations_call(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let ret = Some(LaneInputRow::Audio {
        first: 0,
        stereo: false,
        return_of: Some(5),
    });
    fake.set_destinations(vec![vizdaw_app::testing::dest_row(5, "Moog")]);
    fake.set_lanes(3, vec![audio(30, "Return", ret, MonitorModeRow::Auto)]);
    let mut s = setup(cx, fake);
    let label = "Audio track, Return, output Out 1, input In 1, recording the return of Moog";
    assert!(s.find("button", label).is_some());
    assert_eq!(
        s.value_of("menu", "Input of Return").as_deref(),
        Some("\u{21E0} Moog \u{b7} In 1")
    );
    let reads = s.fake.destination_reads();
    for _ in 0..5 {
        s.window
            .update(&mut s.cx, |_, window, _| window.refresh())
            .unwrap();
        s.cx.run_until_parked();
    }
    assert!(s.find("button", label).is_some());
    assert_eq!(s.fake.destination_reads(), reads, "no read in a draw");
}

impl Setup {
    fn value_of(&mut self, role: &str, label: &str) -> Option<String> {
        self.element(role, label).value
    }
}

/// `LaneInputs` titles the header's input row as the strip does.
#[test]
fn the_input_row_text_leads_with_an_arrow() {
    let input = track_headers::HeaderInput {
        text: LaneInputs::title(mono(2).as_ref()),
        return_name: None,
        meter: None,
        opening: false,
    };
    assert_eq!(track_headers::input_text(&input), "\u{25BE} In 3");
}

/// One picker is open per window: with one open, a press elsewhere (on
/// another track's input row, or Add Track) only closes it.
#[gpui::test]
fn one_input_picker_is_open_per_window(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_lanes(
        3,
        vec![
            audio(30, "Vocal", mono(0), MonitorModeRow::Auto),
            audio(31, "Guitar", mono(1), MonitorModeRow::Auto),
        ],
    );
    let mut s = setup(cx, fake);
    let open = |s: &mut Setup| {
        s.window
            .read_with(&s.cx, |v, _| v.track_headers().input_picker())
            .unwrap()
    };
    // Guitar's list hangs below its row, clear of Vocal's.
    s.click("menu", "Input of Guitar");
    assert_eq!(open(&mut s), Some(31));
    s.click("menu", "Input of Vocal");
    assert_eq!(open(&mut s), None);
    s.click("menu", "Input of Vocal");
    assert_eq!(open(&mut s), Some(30));
    s.click("menu", "Input of Vocal");
    s.click("menu", "Input of Guitar");
    s.click("button", "Add Track");
    assert_eq!(open(&mut s), None);
    assert!(s.find("menuitem", "New Audio Track").is_none());
}
