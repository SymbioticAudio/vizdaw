mod support;

use std::path::Path;
use std::sync::Arc;

use support::demo_fake;
use vizdaw_app::{DocumentModel, SaveAction};
use vizdaw_ffi::VizError;

fn model(f: &Arc<support::FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

#[test]
fn undo_and_redo_menu_titles() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert_eq!(m.undo_title(), "Undo");
    assert_eq!(m.redo_title(), "Redo");
    assert!(!m.can_undo() && !m.can_redo());
    let mut next = f.snap();
    next.version += 1;
    next.undo_label = Some("Rename Sequence".into());
    next.redo_label = Some("Set Lane Mute/Solo/Arm".into());
    f.bump(next);
    // The Swift test applied the bump through a rename command; the trait
    // has no rename, so undo carries the next snapshot in.
    m.undo();
    assert_eq!(m.undo_title(), "Undo Rename Sequence");
    assert_eq!(m.redo_title(), "Redo Set Lane Mute/Solo/Arm");
    assert!(m.can_undo() && m.can_redo());
}

#[test]
fn dirty_title() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert_eq!(m.title(), "Untitled");
    let mut next = f.snap();
    next.version += 1;
    next.dirty = true;
    f.bump(next);
    m.undo();
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
}

#[test]
fn save_when_untitled_needs_save_as() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert!(m.needs_save_as());
    assert_eq!(m.save(), SaveAction::NeedsSaveAs);
    assert_eq!(f.count("save"), 0, "an untitled document is never written");
    m.save_as(Path::new("/tmp/x/demo.vizdaw")).unwrap();
    assert!(!m.needs_save_as());
    assert_eq!(m.title(), "demo");
    assert!(m.alert.is_none());
    assert_eq!(m.save(), SaveAction::Saved);
}

#[test]
fn save_while_untitled_surfaces_use_save_as() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert!(m.needs_save_as());
    // Untitled routes to Save As: no write, no alert.
    assert_eq!(m.save(), SaveAction::NeedsSaveAs);
    assert_eq!(f.count("save"), 0);
    assert!(m.alert.is_none());
    assert!(m.needs_save_as());
    // A titled document whose core refuses shows the core's sentence.
    let g = Arc::new(demo_fake());
    g.set_path(Some("/tmp/x/demo.vizdaw"));
    g.set_save_error(Some(VizError::Io {
        message: "use Save As".into(),
    }));
    let mut t = model(&g);
    assert!(!t.needs_save_as());
    assert_eq!(t.save(), SaveAction::Failed("use Save As".into()));
    let a = t.alert.as_ref().unwrap();
    assert_eq!(a.title, "Save Failed");
    assert_eq!(a.message, "use Save As");
}

#[test]
fn needs_save_as_follows_the_session_path() {
    let f = Arc::new(demo_fake());
    let m = model(&f);
    assert!(m.needs_save_as());
    f.set_path(Some("/tmp/x/demo.vizdaw"));
    assert!(!m.needs_save_as());
}

#[test]
fn undo_and_redo_while_busy_do_nothing_silently() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    let before = m.snapshot().version;
    f.set_step_error(Some(VizError::Busy {
        message: "plugin host is busy".into(),
    }));
    m.undo();
    m.redo();
    assert!(m.alert.is_none());
    assert_eq!(m.snapshot().version, before);
    // Other failures still alert.
    f.set_step_error(Some(VizError::Model {
        message: "nothing to undo".into(),
    }));
    m.undo();
    assert_eq!(m.alert.as_ref().unwrap().title, "Undo Failed");
}
