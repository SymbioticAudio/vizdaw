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

fn lane_ids(m: &DocumentModel) -> Vec<String> {
    m.lanes().iter().map(|l| l.name.clone()).collect()
}

#[test]
fn first_top_level_sequence_is_selected_at_start() {
    let f = Arc::new(demo_fake());
    let m = model(&f);
    assert_eq!(m.selected_seq(), Some(3));
    assert_eq!(lane_ids(&m), ["Groove", "Chords"]);
}

#[test]
fn empty_document_selects_nothing() {
    let f = Arc::new(support::FakeSession::new(vec![]));
    let m = model(&f);
    assert_eq!(m.selected_seq(), None);
    assert!(m.lanes().is_empty());
    assert_eq!(m.lanes_fetches(), 0);
}

#[test]
fn selection_survives_a_snapshot_that_keeps_the_sequence() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select(Some(2));
    let mut next = f.snap();
    next.version += 1;
    f.bump(next);
    m.undo();
    assert_eq!(m.selected_seq(), Some(2));
}

#[test]
fn selection_falls_back_when_the_sequence_is_deleted() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select(Some(1));
    let mut next = f.snap();
    next.version += 1;
    next.sequences.retain(|s| s.id != 1);
    f.bump(next);
    m.undo();
    assert_eq!(m.selected_seq(), Some(3));
    assert_eq!(lane_ids(&m), ["Groove", "Chords"]);
}

#[test]
fn lanes_are_refetched_only_when_version_or_selection_changes() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert_eq!(m.lanes_fetches(), 1);
    m.select(Some(3)); // already shown
    assert_eq!(m.lanes_fetches(), 1);
    // Same version again: no fetch.
    f.bump(f.snap());
    m.undo();
    assert_eq!(m.lanes_fetches(), 1);
    // New version: fetch.
    m.redo();
    assert_eq!(m.lanes_fetches(), 2);
    m.select(Some(2));
    assert_eq!(m.lanes_fetches(), 3);
}

#[test]
fn has_unsaved_changes_reads_the_session() {
    let f = Arc::new(demo_fake());
    let m = model(&f);
    assert!(!m.snapshot().dirty);
    assert!(!m.has_unsaved_changes());
    // The snapshot says clean; the session knows better.
    f.set_unsaved(Some(Ok(true)));
    assert!(!m.snapshot().dirty);
    assert!(m.has_unsaved_changes());
    // An unreadable answer is treated as unsaved.
    f.set_unsaved(Some(Err(VizError::Internal {
        message: "gone".into(),
    })));
    assert!(m.has_unsaved_changes());
}

#[test]
fn idle_applies_a_new_snapshot() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    assert!(!m.idle());
    assert_eq!(m.title(), "Untitled");
    let mut next = f.snap();
    next.version += 1;
    next.dirty = true;
    f.set_idle(Some(next));
    assert!(m.idle());
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
    assert!(!m.idle());
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
}

#[test]
fn undo_to_the_saved_state_is_clean() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    let mut dirty = f.snap();
    dirty.version += 1;
    dirty.dirty = true;
    let mut clean = f.snap();
    clean.version += 2;
    f.bump(dirty.clone());
    m.redo();
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
    f.bump(clean);
    m.undo();
    assert_eq!(m.title(), "Untitled");
    f.bump(dirty);
    m.redo();
    assert_eq!(m.title(), "Untitled \u{2014} Edited");
}

#[test]
fn a_selected_track_that_is_deleted_falls_back() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(31, false);
    assert_eq!(m.selected_lane(), Some(31));
    assert_eq!(m.selected_lanes(), [31]);
    // The session no longer names the lane after the next step.
    f.force_selection(vizdaw_ffi::selection::SelectionRow {
        time_range: None,
        lanes: vec![],
        events: vec![],
    });
    m.undo();
    assert_eq!(m.selected_lane(), None);
    assert!(m.selected_lanes().is_empty());
}

#[test]
fn select_track_replaces_or_extends() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.select_track(30, false);
    m.select_track(31, true);
    assert_eq!(m.selected_lanes(), [30, 31]);
    assert_eq!(m.selected_lane(), Some(30));
    m.select_track(30, true); // toggles off
    assert_eq!(m.selected_lanes(), [31]);
    m.select_track(30, false);
    assert_eq!(m.selected_lanes(), [30]);
}
