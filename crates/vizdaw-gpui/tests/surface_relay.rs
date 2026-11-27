//! The surface relay in the GPUI app (slice 9d Task 3): a session's
//! surface events run in its own document's window, from the window's
//! poll, whichever window is in front; a surface button acts while a text
//! field has the keyboard; a programmatic command opens no dialog.

use std::sync::Arc;
use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};
use vizdaw_app::testing::{FakeCore, FakeSession, demo_fake};
use vizdaw_app::{CoreCalls, DocId, DocumentStore};
use vizdaw_ffi::surfaces::{
    CommandMapRow, LearnKindRow, LearnSourceRow, SurfaceEventRow, SurfaceKindRow,
};
use vizdaw_gpui::{AppState, DocumentWindow};

struct Setup {
    core: Arc<FakeCore>,
    app: Entity<AppState>,
    cx: VisualTestContext,
}

/// The app over a fake core, with one demo window per fake (the last
/// one opened in front).
fn setup(
    cx: &mut TestAppContext,
    fakes: &[Arc<FakeSession>],
) -> (Setup, Vec<WindowHandle<DocumentWindow>>) {
    setup_with(cx, fakes, false)
}

/// [`setup`], with an MCU set up first when `surface`.
fn setup_with(
    cx: &mut TestAppContext,
    fakes: &[Arc<FakeSession>],
    surface: bool,
) -> (Setup, Vec<WindowHandle<DocumentWindow>>) {
    let core = Arc::new(FakeCore::new());
    if surface {
        core.surface_add(SurfaceKindRow::Mcu, "In A", "Out A", None)
            .unwrap();
    }
    for f in fakes {
        core.hand_out("new_demo", f.clone());
    }
    let store = DocumentStore::with_core(core.clone());
    let (app, windows) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        app.update(cx, |s, _| s.store.refresh_claimed_ports());
        let windows: Vec<_> = fakes
            .iter()
            .map(|_| vizdaw_gpui::new_demo(cx).expect("a demo window"))
            .collect();
        (app, windows)
    });
    let vcx = VisualTestContext::from_window(windows[0].into(), cx);
    vcx.run_until_parked();
    (Setup { core, app, cx: vcx }, windows)
}

impl Setup {
    fn doc(&self, w: WindowHandle<DocumentWindow>) -> DocId {
        w.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }

    /// Lets the windows' polls and the document idle run (the idle waits
    /// at least 50 ms of real time, which the test clock does not move).
    fn poll(&mut self) {
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(60));
            self.cx.executor().advance_clock(Duration::from_millis(300));
            self.cx.run_until_parked();
        }
    }

    fn bring_to_front(&mut self, w: WindowHandle<DocumentWindow>) {
        let doc = self.doc(w);
        let token = w.read_with(&self.cx, |v, _| v.token()).unwrap();
        self.app.update(&mut self.cx, |s, _| {
            s.store.focus_document(doc, token);
        });
    }

    fn selected_strips(&mut self, doc: DocId) -> Vec<u64> {
        self.app.read_with(&self.cx, |s, _| {
            s.store
                .document(doc)
                .unwrap()
                .selected_strips()
                .iter()
                .copied()
                .collect()
        })
    }

    fn status(&mut self, doc: DocId) -> Option<String> {
        self.app.read_with(&self.cx, |s, _| {
            s.store.document(doc).unwrap().status().map(str::to_owned)
        })
    }
}

fn played(f: &FakeSession) -> bool {
    f.calls().iter().any(|c| c.starts_with("play("))
}

/// Review Focus 5: two documents; the surfaces' events land in B's
/// session while A is in front. They select and play in B, and A is
/// unchanged; the focus stays with the front document.
#[gpui::test]
fn events_apply_to_the_session_that_took_them(cx: &mut TestAppContext) {
    let a = Arc::new(demo_fake());
    let b = Arc::new(demo_fake());
    let (mut s, windows) = setup(cx, &[a.clone(), b.clone()]);
    let (da, db) = (s.doc(windows[0]), s.doc(windows[1]));
    s.bring_to_front(windows[0]);
    s.poll();
    assert_eq!(s.app.read_with(&s.cx, |s, _| s.store.front()), Some(da));
    let focused = (a.focus_calls(), b.focus_calls());
    assert!(focused.0 >= 1, "the front document has the surfaces");
    b.push_surface_events(vec![
        SurfaceEventRow::Select { strip: 2 },
        SurfaceEventRow::Command {
            id: "transport.play".into(),
            programmatic: false,
        },
    ]);
    s.poll();
    assert_eq!(s.selected_strips(db), vec![2]);
    assert!(played(&b), "B plays: {:?}", b.calls());
    assert_eq!(b.surface_selections(), vec![vec![2]]);
    assert!(s.selected_strips(da).is_empty());
    assert!(!played(&a), "A is unchanged: {:?}", a.calls());
    assert!(a.surface_selections().is_empty());
    assert_eq!(
        (a.focus_calls(), b.focus_calls()),
        focused,
        "the front did not change: the focus did not move"
    );
}

/// Ruling T9-1: a surface's Play runs while the track name field has the
/// keyboard; the field keeps it, and the keys still wait once it ran.
#[gpui::test]
fn a_surface_play_runs_while_the_name_field_has_the_keyboard(cx: &mut TestAppContext) {
    let f = Arc::new(demo_fake());
    let (mut s, windows) = setup(cx, std::slice::from_ref(&f));
    let w = windows[0];
    let doc = s.doc(w);
    w.update(&mut s.cx, |v, window, cx| v.begin_rename(30, window, cx))
        .unwrap();
    s.cx.run_until_parked();
    let typing = |s: &mut Setup| {
        s.app.read_with(&s.cx, |a, _| {
            !a.store.document(doc).unwrap().transport_keys_enabled()
        })
    };
    assert!(typing(&mut s));
    f.push_surface_events(vec![SurfaceEventRow::Command {
        id: "transport.play".into(),
        programmatic: false,
    }]);
    s.poll();
    assert!(played(&f), "{:?}", f.calls());
    assert_eq!(w.read_with(&s.cx, |v, _| v.renaming()).unwrap(), Some(30));
    assert!(typing(&mut s), "the keys still wait for the field");
}

/// Pre-flight I9: an injected `file.save` on an untitled document runs as
/// `command.run` does: no Save panel, and the status line says why.
#[gpui::test]
fn a_programmatic_save_answers_no_dialog(cx: &mut TestAppContext) {
    let f = Arc::new(demo_fake());
    let (mut s, windows) = setup(cx, std::slice::from_ref(&f));
    let doc = s.doc(windows[0]);
    f.push_surface_events(vec![SurfaceEventRow::Command {
        id: "file.save".into(),
        programmatic: true,
    }]);
    s.poll();
    assert!(!s.cx.did_prompt_for_new_path(), "no Save panel");
    assert_eq!(
        s.status(doc).as_deref(),
        Some("this would open the \"Save\" dialog; pass `path` instead")
    );
    // A real button's Save asks for a location, as its key does.
    f.push_surface_events(vec![SurfaceEventRow::Command {
        id: "file.save".into(),
        programmatic: false,
    }]);
    s.poll();
    assert!(s.cx.did_prompt_for_new_path(), "the device's Save asks");
}

impl Setup {
    /// One surface tick of the test clock (no real time passes).
    fn tick(&mut self) {
        self.cx.executor().advance_clock(Duration::from_millis(17));
        self.cx.run_until_parked();
    }
}

fn locates(f: &FakeSession) -> Vec<i64> {
    f.calls()
        .iter()
        .filter_map(|c| c.strip_prefix("locate(")?.strip_suffix(')')?.parse().ok())
        .collect()
}

/// Review I1: with a surface set up, a stopped document takes and runs
/// its surface events within one ~16 ms tick (Swift's 60 Hz timer,
/// `S/VizDAWApp.swift:587-589`), not at the next 250 ms transport poll.
#[gpui::test]
fn a_surface_play_runs_within_one_tick_while_stopped(cx: &mut TestAppContext) {
    let f = Arc::new(demo_fake());
    let (mut s, _windows) = setup_with(cx, std::slice::from_ref(&f), true);
    // Let the launch's first polls settle, then wait out the idle throttle.
    s.poll();
    std::thread::sleep(Duration::from_millis(60));
    f.push_surface_events(vec![SurfaceEventRow::Command {
        id: "transport.play".into(),
        programmatic: false,
    }]);
    s.tick();
    assert!(played(&f), "within one tick: {:?}", f.calls());
}

/// Review I1: a jog locates about 150 ms after its first detent, on the
/// surface tick, not at a slow transport poll.
#[gpui::test]
fn a_jog_flushes_about_150_ms_after_its_detent(cx: &mut TestAppContext) {
    let f = Arc::new(demo_fake());
    let (mut s, _windows) = setup_with(cx, std::slice::from_ref(&f), true);
    s.poll();
    let before = locates(&f).len();
    f.push_surface_events(vec![SurfaceEventRow::Jog { detents: 1 }]);
    s.tick();
    s.tick();
    assert_eq!(locates(&f).len(), before, "inside the jog window");
    // The jog window is real time; the ticks are the test clock's.
    std::thread::sleep(Duration::from_millis(160));
    s.tick();
    assert_eq!(locates(&f).len(), before + 1, "{:?}", f.calls());
}

/// With no surface set up the fast tick does no work: events wait for
/// the transport poll.
#[gpui::test]
fn without_a_surface_the_tick_takes_nothing(cx: &mut TestAppContext) {
    let f = Arc::new(demo_fake());
    let (mut s, _windows) = setup(cx, std::slice::from_ref(&f));
    s.poll();
    std::thread::sleep(Duration::from_millis(60));
    f.push_surface_events(vec![SurfaceEventRow::Command {
        id: "transport.play".into(),
        programmatic: false,
    }]);
    s.tick();
    assert!(!played(&f), "no surface: no fast tick");
    s.poll();
    assert!(played(&f), "the transport poll still takes them");
}

fn staged_play(f: &FakeSession) {
    f.push_surface_events_on_idle(vec![SurfaceEventRow::Command {
        id: "transport.play".into(),
        programmatic: false,
    }]);
}

/// Final review I1: the core makes surface events only in the session's
/// `idle` (`service_surfaces`), so the surface tick runs it: a stopped
/// document's Play lands within one ~16 ms tick, not at the 250 ms poll.
#[gpui::test]
fn the_surface_tick_services_the_session(cx: &mut TestAppContext) {
    let f = Arc::new(demo_fake());
    let (mut s, _windows) = setup_with(cx, std::slice::from_ref(&f), true);
    s.poll();
    staged_play(&f);
    // The idle runs at most every 16 ms of real time, across the tick and
    // the transport poll.
    std::thread::sleep(Duration::from_millis(20));
    s.tick();
    assert!(played(&f), "within one tick: {:?}", f.calls());
}

/// A learned command listens on any port with no surface set up: the
/// tick services the session while command maps exist.
#[gpui::test]
fn a_learned_command_keeps_the_tick_fast(cx: &mut TestAppContext) {
    let f = Arc::new(demo_fake());
    let (mut s, _windows) = setup(cx, std::slice::from_ref(&f));
    s.core.set_command_maps(vec![CommandMapRow {
        source: LearnSourceRow {
            port: "Pads".into(),
            kind: LearnKindRow::Note,
            channel: 9,
            number: 36,
        },
        text: "Pads \u{00B7} Note C2 ch 10".into(),
        command: "transport.play".into(),
    }]);
    // The maps are read again on a slow tick.
    s.poll();
    staged_play(&f);
    std::thread::sleep(Duration::from_millis(20));
    s.tick();
    assert!(played(&f), "within one tick: {:?}", f.calls());
}

/// With nothing to listen for, the tick runs no idle: the transport poll
/// alone does, at its own pace.
#[gpui::test]
fn with_nothing_to_listen_for_the_tick_runs_no_idle(cx: &mut TestAppContext) {
    let f = Arc::new(demo_fake());
    let (mut s, _windows) = setup(cx, std::slice::from_ref(&f));
    s.poll();
    let idles = f.idle_calls();
    std::thread::sleep(Duration::from_millis(20));
    s.tick();
    assert_eq!(f.idle_calls(), idles);
}
