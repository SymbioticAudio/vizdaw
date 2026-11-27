//! The surface relay in the GPUI app (slice 9d Task 3): a session's
//! surface events run in its own document's window, from the window's
//! poll, whichever window is in front; a surface button acts while a text
//! field has the keyboard; a programmatic command opens no dialog.

use std::sync::Arc;
use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};
use vizdaw_app::testing::{FakeCore, FakeSession, demo_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::surfaces::SurfaceEventRow;
use vizdaw_gpui::{AppState, DocumentWindow};

struct Setup {
    app: Entity<AppState>,
    cx: VisualTestContext,
}

/// The app over a fake core, with one demo window per fake (the last
/// one opened in front).
fn setup(
    cx: &mut TestAppContext,
    fakes: &[Arc<FakeSession>],
) -> (Setup, Vec<WindowHandle<DocumentWindow>>) {
    let core = Arc::new(FakeCore::new());
    for f in fakes {
        core.hand_out("new_demo", f.clone());
    }
    let store = DocumentStore::with_core(core);
    let (app, windows) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let windows: Vec<_> = fakes
            .iter()
            .map(|_| vizdaw_gpui::new_demo(cx).expect("a demo window"))
            .collect();
        (app, windows)
    });
    let vcx = VisualTestContext::from_window(windows[0].into(), cx);
    vcx.run_until_parked();
    (Setup { app, cx: vcx }, windows)
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
