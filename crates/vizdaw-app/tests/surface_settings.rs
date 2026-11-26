//! Settings › Surfaces' model (slice 9d Task 1; Swift
//! `SurfaceSettingsTests`, `LearnModelTests.aCommandMapIsRemoved` and
//! `SurfaceCommandsTests.aKontrolIsAddedWithoutAPosition`) over the fake
//! core: no CoreMIDI client, no `surfaces.json`.

use vizdaw_app::surface_settings::{
    KONTROL_HELP, NO_KONTROL, SurfaceSettings, kind_title, row_title, status,
};
use vizdaw_app::testing::FakeCore;
use vizdaw_ffi::VizError;
use vizdaw_ffi::surfaces::{SurfaceKindRow, SurfacePortsRow, SurfaceStateRow, learnable};

const KONTROL: &str = "KONTROL S88 MK3 DAW";

fn refusal(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

fn ports(sources: &[&str], destinations: &[&str]) -> SurfacePortsRow {
    SurfacePortsRow {
        sources: sources.iter().map(|s| s.to_string()).collect(),
        destinations: destinations.iter().map(|s| s.to_string()).collect(),
    }
}

fn problems(lines: &[&str]) -> SurfaceStateRow {
    SurfaceStateRow {
        problems: lines.iter().map(|s| s.to_string()).collect(),
        ..SurfaceStateRow::default()
    }
}

#[test]
fn add_list_and_remove() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    s.add(SurfaceKindRow::Mcu, "In A", "Out A", None, &c);
    let kinds: Vec<_> = s.rows().iter().map(|r| r.kind).collect();
    assert_eq!(kinds, vec![SurfaceKindRow::Mcu]);
    assert_eq!(kind_title(SurfaceKindRow::McuXt), "MCU XT-compatible");
    assert_eq!(status(&s.rows()[0]), "Connected · not answering");
    let id = s.rows()[0].id;
    s.remove(id, &c);
    assert!(s.rows().is_empty());
    assert_eq!(c.surfaces_removed(), vec![id]);
}

#[test]
fn a_refused_add_is_an_alert() {
    let c = FakeCore::new();
    c.fail_surface_add(refusal(
        "VizDAW drives one MCU-compatible main unit and up to three extenders",
    ));
    let mut s = SurfaceSettings::new();
    s.add(SurfaceKindRow::Mcu, "In A", "Out A", None, &c);
    let alert = s.take_alert().expect("an alert");
    assert_eq!(alert.title, "Add Surface Failed");
    assert!(s.take_alert().is_none(), "taken once");
}

#[test]
fn a_refused_add_keeps_the_sheet() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    s.begin_add(&c);
    let why = "VizDAW drives one MCU-compatible main unit and up to three extenders";
    c.fail_surface_add(refusal(why));
    s.commit_add(&c);
    assert!(s.draft().is_some());
    assert!(s.take_alert().is_none());
    assert_eq!(s.draft_problem(), Some(why));
    s.commit_add(&c);
    assert!(s.draft().is_none() && s.draft_problem().is_none());
    assert_eq!(s.rows().len(), 1);
}

#[test]
fn a_kontrol_draft_picks_the_keyboards_daw_ports() {
    let c = FakeCore::new();
    c.set_kontrol_ports(ports(&[KONTROL], &[KONTROL]));
    let mut s = SurfaceSettings::new();
    s.begin_add(&c);
    assert_eq!(s.draft().unwrap().input, "In A");
    s.set_draft_kind(SurfaceKindRow::KontrolMk3, &c);
    let d = s.draft().unwrap();
    assert_eq!(d.kind, SurfaceKindRow::KontrolMk3);
    assert_eq!((d.input.as_str(), d.output.as_str()), (KONTROL, KONTROL));
    assert!(s.draft_note().is_none());
    s.commit_add(&c);
    assert!(s.draft().is_none());
    let added = c.surfaces_added();
    assert_eq!(added[0].0, SurfaceKindRow::KontrolMk3);
    assert_eq!(added[0].3, None);
    assert_eq!(s.rows()[0].position, 0);
}

#[test]
fn a_kontrol_draft_without_a_keyboard_says_so() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    s.begin_add(&c);
    s.set_draft_kind(SurfaceKindRow::KontrolMk3, &c);
    let d = s.draft().unwrap();
    assert_eq!((d.input.as_str(), d.output.as_str()), ("In A", "Out A"));
    assert_eq!(s.draft_note(), Some(NO_KONTROL));
    s.set_draft_kind(SurfaceKindRow::Hui, &c);
    assert!(s.draft_note().is_none());
    assert_eq!(s.draft().unwrap().kind, SurfaceKindRow::Hui);
    s.cancel_add();
    s.set_draft_kind(SurfaceKindRow::KontrolMk3, &c);
    assert!(s.draft().is_none(), "no sheet, nothing to set");
}

#[test]
fn a_kontrol_with_old_firmware_says_why() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    s.add(SurfaceKindRow::KontrolMk3, KONTROL, KONTROL, None, &c);
    let old = "the keyboard answered protocol version 3; VizDAW needs version 4 (keyboard firmware 2.0 or later)";
    let kept =
        "surface 3: “future_box” needs a newer VizDAW; it is kept in surfaces.json and not driven";
    let older = "surface 0: an older line".to_string();
    let newest = format!("surface 0: {old}");
    c.set_surfaces_state(problems(&[&older, &newest, kept]));
    s.refresh(&c);
    assert_eq!(
        s.problem_for(0),
        Some(old),
        "the last line about it, without the prefix"
    );
    assert_eq!(s.problem_for(1), None);
    assert_eq!(s.other_problems(), vec![kept]);
}

#[test]
fn an_answering_row_shows_no_problem_line() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    s.add(SurfaceKindRow::KontrolMk3, KONTROL, KONTROL, None, &c);
    c.set_surfaces_state(problems(&[
        "surface 0: the keyboard answered protocol version 3",
    ]));
    s.refresh(&c);
    assert!(
        s.problem_for(0).is_some(),
        "not answering: the line says why"
    );
    let mut rows = s.rows().to_vec();
    rows[0].online = true;
    c.set_surface_rows(rows);
    s.refresh(&c);
    assert!(
        s.problem_for(0).is_none(),
        "answering: no line under Connected"
    );
}

#[test]
fn a_half_found_kontrol_fills_the_side_found() {
    let c = FakeCore::new();
    c.set_kontrol_ports(ports(&[KONTROL], &[]));
    let mut s = SurfaceSettings::new();
    s.begin_add(&c);
    s.set_draft_kind(SurfaceKindRow::KontrolMk3, &c);
    let d = s.draft().unwrap();
    assert_eq!((d.input.as_str(), d.output.as_str()), (KONTROL, "Out A"));
    assert!(s.draft_note().is_none());
    c.set_kontrol_ports(ports(&[], &["KONTROL S49 MK3 DAW"]));
    s.begin_add(&c);
    s.set_draft_kind(SurfaceKindRow::KontrolMk3, &c);
    let d = s.draft().unwrap();
    assert_eq!(
        (d.input.as_str(), d.output.as_str()),
        ("In A", "KONTROL S49 MK3 DAW")
    );
    assert!(s.draft_note().is_none());
}

#[test]
fn choosing_the_kontrol_refreshes_the_ports() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    s.begin_add(&c);
    let reads = c.port_reads();
    c.set_surface_ports(ports(&["In A", KONTROL], &["Out A", KONTROL]));
    c.set_kontrol_ports(ports(&[KONTROL], &[KONTROL]));
    s.set_draft_kind(SurfaceKindRow::KontrolMk3, &c);
    assert!(c.port_reads() > reads);
    assert!(s.ports().sources.iter().any(|p| p == KONTROL));
    assert_eq!(s.draft().unwrap().input, KONTROL);
}

#[test]
fn no_ports_at_all_and_the_kontrol_give_the_note() {
    let c = FakeCore::new();
    c.set_surface_ports(ports(&[], &[]));
    let mut s = SurfaceSettings::new();
    s.begin_add(&c);
    s.set_draft_kind(SurfaceKindRow::KontrolMk3, &c);
    assert_eq!(s.draft_note(), Some(NO_KONTROL));
    let d = s.draft().unwrap();
    assert_eq!((d.input.as_str(), d.output.as_str()), ("", ""));
}

#[test]
fn the_kontrol_sheet_speaks_descriptively() {
    assert_eq!(
        kind_title(SurfaceKindRow::KontrolMk3),
        "Host-integration keyboard (DAW mode)"
    );
    assert_eq!(
        NO_KONTROL,
        "No host-integration keyboard “DAW” port is connected: pick its ports by hand."
    );
    for text in [
        KONTROL_HELP,
        NO_KONTROL,
        kind_title(SurfaceKindRow::KontrolMk3),
    ] {
        assert!(!text.to_lowercase().contains("official"));
        assert!(!text.contains("NKS"));
    }
}

#[test]
fn a_command_map_is_removed() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    s.remove_command_map("edit.undo", &c);
    assert_eq!(c.removed_command_maps(), vec!["edit.undo".to_string()]);
    assert!(s.take_alert().is_none());
    c.fail_remove_command_map(refusal("no MIDI control is learned for “edit.redo”"));
    s.remove_command_map("edit.redo", &c);
    assert_eq!(
        s.take_alert().map(|a| a.title),
        Some("Remove MIDI Learn Failed".into())
    );
}

#[test]
fn a_hui_and_a_kontrol_are_added_without_a_position() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    s.add(SurfaceKindRow::Hui, "In A", "Out A", Some(2), &c);
    s.add(SurfaceKindRow::KontrolMk3, "In B", "Out B", Some(3), &c);
    s.add(SurfaceKindRow::McuXt, "In C", "Out C", Some(2), &c);
    let positions: Vec<_> = c.surfaces_added().iter().map(|a| a.3).collect();
    assert_eq!(positions, vec![None, None, Some(2)]);
    let titles: Vec<_> = s.rows().iter().map(row_title).collect();
    assert_eq!(
        titles,
        vec![
            "HUI-compatible".to_string(),
            "Host-integration keyboard (DAW mode)".into(),
            "MCU XT-compatible · position 2".into(),
        ]
    );
}

#[test]
fn refresh_reports_only_a_change() {
    let c = FakeCore::new();
    let mut s = SurfaceSettings::new();
    assert!(
        s.rows().is_empty() && s.ports().sources.is_empty(),
        "reads nothing"
    );
    assert!(s.refresh(&c), "the ports arrived");
    assert!(!s.refresh(&c), "nothing new");
    c.set_surfaces_state(problems(&["surfaces.json could not be read"]));
    assert!(s.refresh(&c));
    assert_eq!(s.other_problems(), vec!["surfaces.json could not be read"]);
    assert!(!s.refresh(&c));
}

#[test]
fn learnable_matches_the_keys_rows() {
    assert!(learnable("edit.undo"));
    assert!(!learnable("file.open"), "it always opens a dialog");
    assert!(
        !learnable("surface.add"),
        "the Surfaces group is the channel's"
    );
}
