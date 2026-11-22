mod support;

use std::sync::Arc;

use support::demo_fake;
use vizdaw_app::DocumentModel;
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit};

fn model(f: &Arc<support::FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

#[test]
fn blank_or_unchanged_rename_sends_no_command() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.rename_track(31, "   ");
    m.rename_track(31, "Chords");
    m.rename_track(31, "  Chords ");
    assert_eq!(f.count("edit"), 0);
    m.rename_track(31, "  Keys ");
    assert_eq!(
        f.edits(),
        [EditCommand::Lane {
            edit: LaneEdit::Rename {
                seq: 3,
                lane: 31,
                name: "Keys".into()
            }
        }]
    );
    assert_eq!(m.lanes()[1].name, "Keys");
}

#[test]
fn toggle_mute_flips_the_lane_flag() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    let before = f.snap().version;
    m.toggle_mute(31);
    assert_eq!(f.snap().version, before + 1);
    assert!(m.alert.is_none());
    assert_eq!(f.calls(), ["set_lane_mute(3,31,true)"]);
    assert!(m.lanes()[1].mute);
}

#[test]
fn toggle_mute_unmutes_a_muted_lane() {
    let f = Arc::new(demo_fake());
    f.set_lanes(
        3,
        vec![
            support::lane_row(30, "Groove", false),
            support::lane_row(31, "Chords", true),
        ],
    );
    let mut m = model(&f);
    m.toggle_mute(31);
    assert_eq!(f.calls(), ["set_lane_mute(3,31,false)"]);
}

#[test]
fn set_flags_sends_one_edit() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    m.set_flags(30, None, Some(true), None);
    assert_eq!(
        f.edits(),
        [EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: 3,
                lane: 30,
                mute: None,
                solo: Some(true),
                arm: None
            }
        }]
    );
    assert!(m.lanes()[0].solo);
}

#[test]
fn edit_errors_alert_and_busy_is_a_status() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    f.set_edit_error(Some(VizError::Model {
        message: "no such lane".into(),
    }));
    m.rename_track(31, "Keys");
    assert_eq!(m.alert.as_ref().unwrap().title, "Rename Failed");
    m.alert = None;
    m.toggle_mute(31);
    assert_eq!(m.alert.as_ref().unwrap().title, "Mute Failed");
    m.alert = None;
    f.set_edit_error(Some(VizError::Busy {
        message: "plugin host is busy".into(),
    }));
    m.rename_track(31, "Keys");
    assert!(m.alert.is_none());
    assert_eq!(m.status(), Some("plugin host is busy"));
    m.set_status(None);
    assert_eq!(m.status(), None);
}
