//! Alerts and questions as prompts (slice 7b Task 2): a document's alert
//! on its own window with OK, store alerts on the front window, one at a
//! time in order; a dialog window when no window is open (a GUI MIDI open
//! whose import fails); the recovery questions before a GUI open; the
//! DAWproject return prompt as a sheet;
//! Recovered Recordings; and nothing at all under the control channel.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use core_control::ControlError;
use core_control::server::Call;
use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::Value;
use vizdaw_app::testing::{FakeCore, FakeSession, JobScript, demo_fake, seq};
use vizdaw_app::{AlertItem, DocumentStore};
use vizdaw_ffi::record::RecoveredFileRow;
use vizdaw_ffi::snapshot::{PluginFormatTag, PluginRefRow};
use vizdaw_ffi::{DawprojectInfoRow, RecoveryInfo, VizError};
use vizdaw_gpui::alerts::{self, Showing};
use vizdaw_gpui::control::{self, CallSource};
use vizdaw_gpui::sheet_host::AskDialog;
use vizdaw_gpui::{AppState, DocumentWindow};

struct Setup {
    core: Arc<FakeCore>,
    /// The demo's session and window.
    fake: Arc<FakeSession>,
    window: WindowHandle<DocumentWindow>,
}

/// The app over a fake core with the demo open in a window, no channel.
fn setup(cx: &mut TestAppContext) -> Setup {
    let core = Arc::new(FakeCore::new());
    let fake = Arc::new(demo_fake());
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(core.clone());
    let window = cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
        vizdaw_gpui::new_demo(cx).expect("the demo's window")
    });
    cx.run_until_parked();
    Setup { core, fake, window }
}

fn post(cx: &mut TestAppContext, title: &str, message: &str) {
    cx.update(|cx| {
        AppState::global(cx).update(cx, |s, cx| {
            s.store.post_alert(AlertItem::new(title, message));
            cx.notify();
        })
    });
    cx.run_until_parked();
}

fn showing(cx: &mut TestAppContext) -> Vec<(AnyWindowHandle, Showing)> {
    cx.update(|cx| alerts::showing(cx))
}

fn store_alert(cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .store
            .alert()
            .map(|a| a.title.clone())
    })
}

fn windows(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| AppState::global(cx).read(cx).windows().len())
}

fn documents(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| AppState::global(cx).read(cx).store.count())
}

/// Lets the window polls run a while.
fn tick(cx: &mut TestAppContext) {
    for _ in 0..6 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
}

fn answer(cx: &mut TestAppContext, button: &str) {
    cx.simulate_prompt_answer(button);
    cx.run_until_parked();
}

#[gpui::test]
fn a_documents_alert_is_a_prompt_with_ok_on_its_own_window(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.set_path(Some("/tmp/Song.vizdaw"));
    s.fake.set_save_error(Some(VizError::Io {
        message: "disk full".into(),
    }));
    let mut vcx = VisualTestContext::from_window(s.window.into(), cx);
    vcx.simulate_keystrokes("secondary-s");
    vcx.run_until_parked();
    assert_eq!(
        cx.pending_prompt(),
        Some(("Save Failed".into(), "disk full".into()))
    );
    let shown = showing(cx);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].0, AnyWindowHandle::from(s.window));
    assert!(matches!(&shown[0].1, Showing::Alert(Some(_), a) if a.title == "Save Failed"));
    // Taken from the store: no banner, nothing for the channel.
    assert_eq!(store_alert(cx), None);
    answer(cx, "OK");
    assert!(!cx.has_pending_prompt());
    assert!(showing(cx).is_empty());
}

#[gpui::test]
fn a_documents_alert_shows_on_its_own_window_not_the_key_one(cx: &mut TestAppContext) {
    let s = setup(cx);
    let other = cx
        .update(vizdaw_gpui::new_document)
        .expect("a second window");
    cx.run_until_parked();
    let key = cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .store
            .key_window()
            .map(|k| k.0)
    });
    let other_doc = other.read_with(cx, |v, _| v.doc()).unwrap();
    assert_eq!(key, Some(other_doc), "the new window is key");
    s.fake.set_path(Some("/tmp/Song.vizdaw"));
    s.fake.set_save_error(Some(VizError::Io {
        message: "disk full".into(),
    }));
    let demo = s.window.read_with(cx, |v, _| v.doc()).unwrap();
    cx.update(|cx| {
        AppState::global(cx).update(cx, |st, cx| {
            st.store.with_document(demo, |d, _| d.save());
            cx.notify();
        })
    });
    cx.run_until_parked();
    let shown = showing(cx);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].0, AnyWindowHandle::from(s.window));
}

#[gpui::test]
fn alerts_show_one_at_a_time_in_the_order_posted(cx: &mut TestAppContext) {
    let _s = setup(cx);
    post(cx, "Open Failed", "first");
    post(cx, "Import Failed", "second");
    assert_eq!(
        cx.pending_prompt(),
        Some(("Open Failed".into(), "first".into()))
    );
    assert_eq!(store_alert(cx).as_deref(), Some("Import Failed"), "waiting");
    answer(cx, "OK");
    assert_eq!(
        cx.pending_prompt(),
        Some(("Import Failed".into(), "second".into()))
    );
    answer(cx, "OK");
    assert!(!cx.has_pending_prompt());
    assert_eq!(store_alert(cx), None);
}

#[gpui::test]
fn a_closed_documents_alert_shows_on_the_front_window(cx: &mut TestAppContext) {
    let s = setup(cx);
    let other = cx
        .update(vizdaw_gpui::new_document)
        .expect("a second window");
    cx.run_until_parked();
    let doc = other.read_with(cx, |v, _| v.doc()).unwrap();
    cx.update(|cx| {
        AppState::global(cx).update(cx, |s, cx| {
            s.store
                .post_document_alert(doc, AlertItem::new("Undo Failed", "gone"));
            s.close_document(doc);
            cx.notify();
        })
    });
    cx.run_until_parked();
    let shown = showing(cx);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].0, AnyWindowHandle::from(s.window));
    assert_eq!(
        cx.pending_prompt(),
        Some(("Undo Failed".into(), "gone".into()))
    );
}

/// The one dialog window open, and its ask's text.
fn dialog(cx: &mut TestAppContext) -> Option<(WindowHandle<AskDialog>, String, Option<String>)> {
    let d = cx.update(|cx| alerts::dialogs(cx));
    assert!(d.len() <= 1, "one dialog at a time");
    let h = d.first()?.downcast::<AskDialog>()?;
    let (m, detail) = h
        .read_with(cx, |d, _| (d.ask().message.clone(), d.ask().detail.clone()))
        .ok()?;
    Some((h, m, detail))
}

fn close_all(s: &Setup, cx: &mut TestAppContext) {
    s.window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(windows(cx), 0);
}

#[gpui::test]
fn with_no_window_open_an_alert_shows_in_a_dialog_window(cx: &mut TestAppContext) {
    let s = setup(cx);
    close_all(&s, cx);
    post(cx, "Open Failed", "nope");
    let (h, message, detail) = dialog(cx).expect("a dialog window");
    assert_eq!(message, "Open Failed");
    assert_eq!(detail.as_deref(), Some("nope"));
    assert!(!cx.has_pending_prompt(), "not a window's prompt");
    // A second alert waits for it.
    post(cx, "Import Failed", "next");
    assert_eq!(store_alert(cx).as_deref(), Some("Import Failed"));
    let mut vcx = VisualTestContext::from_window(h.into(), cx);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    let (_, message, _) = dialog(cx).expect("the next one");
    assert_eq!(message, "Import Failed");
    assert_eq!(store_alert(cx), None);
}

#[gpui::test]
fn a_gui_midi_open_whose_import_fails_with_no_other_window_shows_its_alert(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    close_all(&s, cx);
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    s.core.hand_out("new_project", made.clone());
    made.script_job(JobScript::failed("the file is damaged").steps(&[0.5]));
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/Song.mid"), cx));
    tick(cx);
    assert_eq!(windows(cx), 0, "the made project closed with its window");
    assert_eq!(documents(cx), 0);
    let (_, message, detail) = dialog(cx).expect("its alert shows");
    assert_eq!(message, "Import Failed");
    assert_eq!(detail.as_deref(), Some("the file is damaged"));
}

fn crashed(loading: bool, autosave: bool) -> RecoveryInfo {
    RecoveryInfo {
        crashed: true,
        loading: if loading {
            vec![PluginRefRow {
                format: PluginFormatTag::Clap,
                id: "com.example.verb".into(),
                name: "Verb".into(),
                vendor: "Example".into(),
                version: "1".into(),
            }]
        } else {
            vec![]
        },
        autosave: autosave.then(|| "/tmp/x/Song.vizdaw/autosave".into()),
    }
}

fn opened(core: &FakeCore) -> Vec<String> {
    core.calls()
        .into_iter()
        .filter(|c| c.starts_with("open"))
        .collect()
}

#[gpui::test]
fn a_gui_open_after_a_crash_asks_first_and_the_answer_opens_it(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core
        .set_recovery("/tmp/x/Song.vizdaw", crashed(true, true));
    let none = cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/x/Song.vizdaw"), cx));
    cx.run_until_parked();
    assert!(none.is_none(), "it asks first");
    assert!(opened(&s.core).is_empty());
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "VizDAW quit unexpectedly while this project was open, while loading Verb.".into(),
            "With plugins off, every plugin is a placeholder you can load one by one.".into()
        ))
    );
    answer(cx, "Open with Plugins Off");
    // Then the autosave question.
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "\u{201c}Song\u{201d} has an autosave newer than its last save.".into(),
            "Restoring opens the autosaved version as an unsaved change.".into()
        ))
    );
    assert!(opened(&s.core).is_empty());
    answer(cx, "Restore Autosave");
    assert_eq!(
        opened(&s.core),
        [
            "open_with(/tmp/x/Song.vizdaw,plugins_off=true,autosave=Some(\"/tmp/x/Song.vizdaw/autosave\"))"
        ]
    );
    assert_eq!(windows(cx), 2, "its window opened");
    assert!(!cx.has_pending_prompt());
}

#[gpui::test]
fn cancel_opens_nothing_and_open_normally_opens_as_saved(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core
        .set_recovery("/tmp/x/Song.vizdaw", crashed(false, false));
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/x/Song.vizdaw"), cx));
    cx.run_until_parked();
    assert_eq!(
        cx.pending_prompt().map(|p| p.0),
        Some("VizDAW quit unexpectedly while this project was open.".into())
    );
    answer(cx, "Cancel");
    assert!(opened(&s.core).is_empty());
    assert_eq!(windows(cx), 1);
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/x/Song.vizdaw"), cx));
    cx.run_until_parked();
    answer(cx, "Open Normally");
    assert_eq!(opened(&s.core), ["open(/tmp/x/Song.vizdaw)"]);
    assert_eq!(windows(cx), 2);
}

#[gpui::test]
fn with_no_window_open_the_question_shows_in_a_dialog_window(cx: &mut TestAppContext) {
    let s = setup(cx);
    close_all(&s, cx);
    s.core.set_recovery(
        "/tmp/x/Song.vizdaw",
        RecoveryInfo {
            crashed: false,
            loading: vec![],
            autosave: Some("/tmp/x/Song.vizdaw/autosave".into()),
        },
    );
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/x/Song.vizdaw"), cx));
    cx.run_until_parked();
    let (h, message, _) = dialog(cx).expect("a dialog window");
    assert_eq!(
        message,
        "\u{201c}Song\u{201d} has an autosave newer than its last save."
    );
    // Esc does nothing (no Cancel); Return restores.
    let mut vcx = VisualTestContext::from_window(h.into(), cx);
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(dialog(cx).is_some());
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        opened(&s.core),
        [
            "open_with(/tmp/x/Song.vizdaw,plugins_off=false,autosave=Some(\"/tmp/x/Song.vizdaw/autosave\"))"
        ]
    );
    assert!(dialog(cx).is_none());
    assert_eq!(windows(cx), 1);
}

fn from_vizdaw() -> DawprojectInfoRow {
    DawprojectInfoRow {
        sample_rate: 48_000,
        tracks: 2,
        audio_files: 0,
        from_vizdaw: true,
        export_id: None,
        project_id: None,
    }
}

fn sheet(cx: &mut TestAppContext, w: WindowHandle<DocumentWindow>) -> Option<String> {
    w.read_with(cx, |v, _| v.sheet_label()).unwrap()
}

#[gpui::test]
fn a_gui_open_of_a_vizdaw_made_dawproject_shows_the_return_prompt(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core
        .set_dawproject_info("/tmp/Back.dawproject", from_vizdaw());
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/Back.dawproject"), cx));
    cx.run_until_parked();
    // Every button enabled (Merge too, slice 7c): the platform's prompt
    // (Swift's `NSAlert`), on the front window.
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "This DAWproject came from a VizDAW project.".into(),
            "Merge it into the project it came from, or open it as a new project.".into()
        ))
    );
    let shown = showing(cx);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0], (AnyWindowHandle::from(s.window), Showing::Return));
    assert_eq!(sheet(cx, s.window), None);
    // Cancel: nothing opens.
    answer(cx, "Cancel");
    assert!(!cx.has_pending_prompt());
    assert_eq!(documents(cx), 1);
    let none = cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .store
            .return_prompt()
            .is_none()
    });
    assert!(none);
    // Again; New Project: a project imports it, in its window.
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    made.script_job(JobScript::done().steps(&[0.5]));
    s.core.hand_out("new_project", made);
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/Back.dawproject"), cx));
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    answer(cx, "New Project");
    assert!(!cx.has_pending_prompt());
    assert_eq!(documents(cx), 2);
    assert_eq!(windows(cx), 2);
}

#[gpui::test]
fn a_return_prompt_whose_window_closes_shows_again_on_the_front_window(cx: &mut TestAppContext) {
    let s = setup(cx);
    // As off macOS: the prompt is drawn as the sheet card.
    cx.update(vizdaw_gpui::sheet_host::use_sheet_prompts);
    let other = cx
        .update(vizdaw_gpui::new_document)
        .expect("a second window");
    cx.run_until_parked();
    s.core
        .set_dawproject_info("/tmp/Back.dawproject", from_vizdaw());
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/Back.dawproject"), cx));
    cx.run_until_parked();
    // The key window asks.
    let shown = showing(cx);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].1, Showing::Return);
    let asking = shown[0].0;
    let waiting = if asking == AnyWindowHandle::from(other) {
        s.window
    } else {
        other
    };
    asking
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        showing(cx),
        [(AnyWindowHandle::from(waiting), Showing::Return)]
    );
}

#[gpui::test]
fn a_saved_project_with_recordings_a_crash_left_offers_them(cx: &mut TestAppContext) {
    let s = setup(cx);
    let song = Arc::new(demo_fake());
    song.set_path(Some("/tmp/x/Song.vizdaw"));
    song.set_recovered_files(vec![RecoveredFileRow {
        path: "Media/Recorded/Take 1.caf".into(),
        frames: 48_000,
        size_known: false,
        readable: true,
    }]);
    s.core.hand_out("open", song.clone());
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/x/Song.vizdaw"), cx));
    cx.run_until_parked();
    assert_eq!(
        cx.pending_prompt(),
        Some((
            "Recovered Recordings".into(),
            "A recording was left by VizDAW quitting unexpectedly: Take 1.".into()
        ))
    );
    let shown = showing(cx);
    assert!(matches!(shown[0].1, Showing::Recovery(_)));
    answer(cx, "Import into New Tracks");
    assert_eq!(song.count("recover_recordings(true)"), 1);
    tick(cx);
    assert!(!cx.has_pending_prompt(), "offered once");
    // The demo (never saved) offers nothing.
    assert_eq!(s.fake.reads("recovered_files"), 0);
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
fn under_the_control_channel_nothing_shows(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| control::pump(&NoCalls, cx));
    post(cx, "Open Failed", "nope");
    assert!(!cx.has_pending_prompt());
    assert_eq!(
        store_alert(cx).as_deref(),
        Some("Open Failed"),
        "the channel's"
    );
    // The channel answers the questions and the return prompt.
    s.core
        .set_dawproject_info("/tmp/Back.dawproject", from_vizdaw());
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/Back.dawproject"), cx));
    cx.run_until_parked();
    assert_eq!(sheet(cx, s.window), None);
    // A saved project's recordings are not offered.
    let song = Arc::new(demo_fake());
    song.set_path(Some("/tmp/x/Song.vizdaw"));
    s.core.hand_out("open", song.clone());
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/x/Song.vizdaw"), cx));
    cx.run_until_parked();
    assert_eq!(song.reads("recovered_files"), 0);
    assert!(!cx.has_pending_prompt());
    assert!(showing(cx).is_empty());
    // With no window, no dialog either.
    let windows: Vec<_> = cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .windows()
            .iter()
            .map(|w| w.handle)
            .collect()
    });
    for w in windows {
        w.update(cx, |_, window, _| window.remove_window()).ok();
    }
    cx.run_until_parked();
    post(cx, "Import Failed", "x");
    assert!(dialog(cx).is_none());
}

// ---------------------------------------------------------------------
// Review fixes (round 1)

#[gpui::test]
fn with_sheet_prompts_an_alert_waits_for_the_close_question(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(vizdaw_gpui::sheet_host::use_sheet_prompts);
    s.fake.set_dirty(true);
    let mut vcx = VisualTestContext::from_window(s.window.into(), cx);
    vcx.simulate_keystrokes("secondary-w");
    vcx.run_until_parked();
    let up = |vcx: &mut VisualTestContext| {
        s.window
            .update(vcx, |_, window, _| window.has_active_prompt())
            .unwrap()
    };
    assert!(up(&mut vcx), "the close question");
    post(cx, "Open Failed", "nope");
    // The close question survives; the alert waits.
    assert!(up(&mut vcx));
    let shown = showing(cx);
    assert!(
        matches!(&shown[..], [(_, Showing::Asked(m))] if m.starts_with("Save changes")),
        "{shown:?}"
    );
    assert_eq!(store_alert(cx).as_deref(), Some("Open Failed"));
    // Esc is the close question's Cancel: the window stays, then the
    // alert shows.
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(windows(cx), 1);
    let shown = showing(cx);
    assert!(
        matches!(&shown[..], [(_, Showing::Alert(_, a))] if a.title == "Open Failed"),
        "{shown:?}"
    );
    assert!(up(&mut vcx));
}

#[gpui::test]
fn with_sheet_prompts_an_alert_whose_window_closes_shows_on_the_front_window(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(vizdaw_gpui::sheet_host::use_sheet_prompts);
    let other = cx
        .update(vizdaw_gpui::new_document)
        .expect("a second window");
    cx.run_until_parked();
    let doc = other.read_with(cx, |v, _| v.doc()).unwrap();
    cx.update(|cx| {
        AppState::global(cx).update(cx, |st, cx| {
            st.store
                .post_document_alert(doc, AlertItem::new("Undo Failed", "gone"));
            cx.notify();
        })
    });
    cx.run_until_parked();
    assert_eq!(showing(cx)[0].0, AnyWindowHandle::from(other));
    other
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    let shown = showing(cx);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0].0, AnyWindowHandle::from(s.window));
    assert!(matches!(&shown[0].1, Showing::Alert(_, a) if a.title == "Undo Failed"));
}

#[gpui::test]
fn disable_turns_off_the_loading_plugins_and_opens_with_plugins_on(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core
        .set_recovery("/tmp/x/Song.vizdaw", crashed(true, false));
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/x/Song.vizdaw"), cx));
    cx.run_until_parked();
    answer(cx, "Disable Verb");
    assert_eq!(
        s.core.disabled_plugins(),
        [(PluginFormatTag::Clap, "com.example.verb".to_owned())]
    );
    assert_eq!(opened(&s.core), ["open(/tmp/x/Song.vizdaw)"]);
    assert_eq!(windows(cx), 2);
}

#[test]
fn the_crash_questions_cancel_is_escs() {
    let q = vizdaw_app::store::OpenQuestion::AfterCrash {
        project: "Song".into(),
        loading: vec![],
        crashed: true,
    };
    let b = alerts::question_ask(&q).prompt_buttons(true);
    let cancels: Vec<bool> = b.iter().map(|b| b.is_cancel()).collect();
    assert_eq!(cancels, [false, false, true]);
    assert_eq!(b[2].label().as_ref(), "Cancel");
    // The autosave question has none (Esc does nothing, as in Swift).
    let q = vizdaw_app::store::OpenQuestion::RestoreAutosave {
        project: "Song".into(),
    };
    assert!(
        alerts::question_ask(&q)
            .prompt_buttons(true)
            .iter()
            .all(|b| !b.is_cancel())
    );
}

fn offer(s: &Setup, cx: &mut TestAppContext) -> Arc<FakeSession> {
    let song = Arc::new(demo_fake());
    song.set_path(Some("/tmp/x/Song.vizdaw"));
    song.set_recovered_files(vec![RecoveredFileRow {
        path: "Media/Recorded/Take 1.caf".into(),
        frames: 48_000,
        size_known: true,
        readable: true,
    }]);
    s.core.hand_out("open", song.clone());
    cx.update(|cx| vizdaw_gpui::open_path(Path::new("/tmp/x/Song.vizdaw"), cx));
    cx.run_until_parked();
    assert_eq!(
        cx.pending_prompt().map(|p| p.0),
        Some("Recovered Recordings".into())
    );
    song
}

#[gpui::test]
fn move_to_trash_and_later_answer_the_offer(cx: &mut TestAppContext) {
    let s = setup(cx);
    let song = offer(&s, cx);
    answer(cx, "Move to Trash");
    assert_eq!(song.count("recover_recordings(false)"), 1);
    assert_eq!(song.count("recover_recordings(true)"), 0);
    tick(cx);
    assert!(!cx.has_pending_prompt());
    // Later: nothing moves, and nothing is offered again until the next
    // open.
    let doc = cx.update(|cx| AppState::global(cx).read(cx).store.front().unwrap());
    cx.update(|cx| {
        AppState::global(cx).update(cx, |st, _| {
            st.store.close(doc);
        })
    });
    cx.run_until_parked();
    let song = offer(&s, cx);
    answer(cx, "Later");
    tick(cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(song.count("recover_recordings(false)"), 0);
    assert_eq!(song.count("recover_recordings(true)"), 0);
}
