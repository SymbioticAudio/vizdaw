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
    let mute = point(row.right() - px(60.), row.center().y);
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
    assert_eq!(s.calls_from_edit(), vec!["edit", "save"]);
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
