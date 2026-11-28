//! The detail pane's model: Swift `NavigationTests`, the `editors.*` part
//! of `ControlCommandsTests.editorsAndWindowsGoWhereTheMenuSendsThem`,
//! targets, titles and pruning.

mod support;

use std::sync::Arc;

use core_control::ErrorCode;
use support::{FakeSession, demo_fake, editor_fake};
use vizdaw_app::editor::{DetailEditorKind, DetailState, EditorLayout, EditorTarget};
use vizdaw_app::{DocumentModel, commands};
use vizdaw_ffi::events::{MeterEntryRow, SequenceInfo};
use vizdaw_ffi::selection::TickRange;

const BAR: i64 = 61_440;

fn model(f: &Arc<FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

fn block(start: i64, end: i64) -> EditorTarget {
    EditorTarget::Block {
        seq: 3,
        lane: 31,
        start,
        end,
    }
}

#[test]
fn a_block_opens_in_the_last_used_detail_editor() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.open_detail(block(0, 10), None);
    assert_eq!(
        m.layout().detail,
        Some(DetailState::new(DetailEditorKind::PianoRoll, block(0, 10)))
    );
    m.open_detail(
        EditorTarget::Lane { seq: 3, lane: 31 },
        Some(DetailEditorKind::List),
    );
    m.open_detail(EditorTarget::Lane { seq: 3, lane: 30 }, None);
    assert_eq!(
        m.layout().detail.as_ref().map(|d| d.kind),
        Some(DetailEditorKind::List),
        "the last used editor"
    );
    assert_eq!(m.layout().last_kind, DetailEditorKind::List);
}

#[test]
fn opening_a_detail_shows_the_bottom_pane_on_the_editor() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.update_layout(|l| {
        l.views.arrange.bottom.shown = false;
        l.views.arrange.bottom.view = vizdaw_app::editor::PaneView::Mixer;
    });
    m.open_detail(block(0, 10), Some(DetailEditorKind::List));
    let bottom = m.layout().views.arrange.bottom;
    assert!(bottom.shown);
    assert_eq!(bottom.view, vizdaw_app::editor::PaneView::Editor);
}

#[test]
fn the_list_command_opens_and_close_detail_closes() {
    let f = Arc::new(demo_fake());
    let mut m = model(&f);
    commands::run("editors.list", &[], &mut m).unwrap();
    let d = m.layout().detail.clone().unwrap();
    assert_eq!(d.kind, DetailEditorKind::List);
    assert_eq!(d.target, EditorTarget::Sequence { seq: 3 });
    assert_eq!(m.detail_kind_name(), "list");
    assert!(commands::enabled("editors.close_detail", &m));
    commands::run("editors.close_detail", &[], &mut m).unwrap();
    assert!(m.layout().detail.is_none());
    assert_eq!(m.detail_kind_name(), "none");
    assert!(!commands::enabled("editors.close_detail", &m));
    let e = commands::run("editors.close_detail", &[], &mut m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "No detail editor is open.");
    let e = commands::perform_from_menu("editors.close_detail", &mut m).unwrap_err();
    assert_eq!(e.message, "No detail editor is open.");
    assert_eq!(m.status(), Some("No detail editor is open."));
    commands::perform_from_menu("editors.list", &mut m).unwrap();
    assert_eq!(m.detail_kind_name(), "list");
}

#[test]
fn the_list_with_nothing_to_show_opens_nothing() {
    let f = Arc::new(FakeSession::new(vec![]));
    let mut m = model(&f);
    assert!(commands::enabled("editors.list", &m));
    let ran = commands::run("editors.list", &[], &mut m).unwrap();
    assert_eq!(ran.status, None);
    assert!(m.layout().detail.is_none());
}

#[test]
fn the_list_reopens_the_open_detail_on_its_target() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.open_detail(block(0, 10), None);
    // The selection points elsewhere now: the open detail's target wins.
    m.select_track(30, false);
    m.show_detail(DetailEditorKind::List);
    let d = m.layout().detail.clone().unwrap();
    assert_eq!(d.kind, DetailEditorKind::List);
    assert_eq!(d.target, block(0, 10));
}

#[test]
fn the_detail_target_follows_the_selection() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    assert_eq!(m.detail_target(), Some(EditorTarget::Sequence { seq: 3 }));
    m.select_track(31, false);
    assert_eq!(
        m.detail_target(),
        Some(EditorTarget::Lane { seq: 3, lane: 31 })
    );
    m.set_time_range(TickRange { start: 0, end: 10 });
    assert_eq!(m.detail_target(), Some(block(0, 10)));
    m.select_track(30, true);
    assert_eq!(
        m.detail_target(),
        Some(EditorTarget::Sequence { seq: 3 }),
        "two tracks: the sequence"
    );
}

#[test]
fn the_detail_closes_when_its_track_is_deleted() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.open_detail(
        EditorTarget::Lane { seq: 3, lane: 31 },
        Some(DetailEditorKind::List),
    );
    m.select_track(30, false);
    commands::run("track.delete", &[], &mut m).unwrap();
    assert!(m.layout().detail.is_some(), "another track went");
    m.select_track(31, false);
    commands::run("track.delete", &[], &mut m).unwrap();
    assert!(m.layout().detail.is_none());
    assert_eq!(m.detail_kind_name(), "none");
}

#[test]
fn a_sequence_detail_stays_while_its_sequence_does() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    m.open_detail(
        EditorTarget::Sequence { seq: 3 },
        Some(DetailEditorKind::List),
    );
    m.select_track(31, false);
    commands::run("track.delete", &[], &mut m).unwrap();
    assert!(m.layout().detail.is_some());
    let mut next = f.snap();
    next.version += 1;
    next.sequences.retain(|s| s.id != 3);
    f.set_idle(Some(next));
    assert!(m.idle());
    assert!(m.layout().detail.is_none(), "its sequence went");
}

#[test]
fn the_title_names_the_sequence_track_and_bars() {
    let f = Arc::new(editor_fake());
    f.set_info(
        3,
        SequenceInfo {
            id: 3,
            name: "Song".into(),
            comment: String::new(),
            keyboard_slot: None,
            start_offset: 0,
            length_ticks: 0,
            tempo: vec![],
            meter: vec![MeterEntryRow {
                tick: 0,
                num: 4,
                den: 4,
                sharps: 0,
                minor: false,
            }],
            markers: vec![],
            generated: false,
        },
    );
    let mut m = model(&f);
    assert_eq!(m.detail_title(), "");
    m.open_detail(EditorTarget::Sequence { seq: 3 }, None);
    assert_eq!(m.detail_title(), "Song");
    m.open_detail(EditorTarget::Lane { seq: 3, lane: 31 }, None);
    assert_eq!(m.detail_title(), "Song \u{203a} Chords");
    m.open_detail(block(0, BAR), None);
    assert_eq!(m.detail_title(), "Song \u{203a} Chords, bar 1");
    m.open_detail(block(0, 2 * BAR), None);
    assert_eq!(m.detail_title(), "Song \u{203a} Chords, bars 1\u{2013}2");
    m.open_detail(block(BAR, BAR), None);
    assert_eq!(m.detail_title(), "Song \u{203a} Chords, bar 2");
    // A track of a sequence not shown is read from the session.
    m.open_detail(EditorTarget::Lane { seq: 1, lane: 10 }, None);
    assert_eq!(m.detail_title(), "Beat \u{203a} Kick");
    m.open_detail(EditorTarget::Lane { seq: 1, lane: 99 }, None);
    assert_eq!(m.detail_title(), "Beat \u{203a} Track");
}

#[test]
fn the_detail_reads_and_writes_the_swift_shapes() {
    let l = EditorLayout::decode(Some(
        r#"{"detail":{"kind":"pianoRoll","target":{"sequence":{"_0":3}}},"lastKind":"stripChart"}"#,
    ));
    let d = l.detail.clone().unwrap();
    assert_eq!(d.kind, DetailEditorKind::PianoRoll);
    assert_eq!(d.target, EditorTarget::Sequence { seq: 3 });
    assert_eq!(l.last_kind, DetailEditorKind::StripChart);
    let v: serde_json::Value =
        serde_json::to_value(DetailState::new(DetailEditorKind::DrumGrid, block(0, 10))).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"kind": "drumGrid",
            "target": {"block": {"seq": 3, "lane": 31, "start": 0, "end": 10}}})
    );
    let names: Vec<_> = DetailEditorKind::ALL.iter().map(|k| k.name()).collect();
    assert_eq!(names, ["piano_roll", "list", "strip_chart", "drum_grid"]);
    let titles: Vec<_> = DetailEditorKind::ALL.iter().map(|k| k.title()).collect();
    assert_eq!(titles, ["Piano Roll", "List", "Strip Chart", "Drum Grid"]);
    // A list filter is kept with its Swift keys.
    let l = EditorLayout::decode(Some(
        r#"{"detail":{"kind":"list","target":{"lane":{"seq":3,"lane":31}},"listFilter":{"hiddenKinds":["cc"],"shownDests":[2]}}}"#,
    ));
    let filter = l.detail.unwrap().list_filter.unwrap();
    assert!(
        filter
            .hidden_kinds
            .contains(&vizdaw_app::list::ListKind::Cc)
    );
    assert_eq!(
        filter
            .shown_dests
            .map(|d| d.into_iter().collect::<Vec<_>>()),
        Some(vec![2])
    );
}

#[test]
fn another_target_drops_the_piano_roll_lanes_but_keeps_the_rest() {
    let f = Arc::new(editor_fake());
    f.set_layout(Some(
        r#"{"detail":{"kind":"pianoRoll","target":{"lane":{"seq":3,"lane":31}},"lanes":[30],"stripRows":[1]}}"#,
    ));
    let mut m = model(&f);
    m.show_detail(DetailEditorKind::List);
    let v = serde_json::to_value(m.layout().detail.clone().unwrap()).unwrap();
    assert_eq!(v["lanes"], serde_json::json!([30]), "same target: kept");
    m.open_detail(block(0, 10), None);
    let v = serde_json::to_value(m.layout().detail.clone().unwrap()).unwrap();
    assert!(v.get("lanes").is_none());
    assert_eq!(v["stripRows"], serde_json::json!([1]));
}

/// A hostile `ui.json` block target at the ends of the tick range opens
/// without overflowing (debug builds panic on overflow).
#[test]
fn a_block_target_at_the_tick_limits_does_not_overflow() {
    for (start, end) in [(i64::MAX, i64::MIN), (0, i64::MIN), (i64::MAX, i64::MAX)] {
        let f = Arc::new(editor_fake());
        f.set_layout(Some(&format!(
            r#"{{"detail":{{"kind":"list","target":{{"block":{{"seq":3,"lane":31,"start":{start},"end":{end}}}}}}}}}"#
        )));
        let mut m = model(&f);
        assert_eq!(
            m.layout().detail.as_ref().map(|d| d.target),
            Some(vizdaw_app::editor::EditorTarget::Block {
                seq: 3,
                lane: 31,
                start,
                end
            }),
            "the hostile target was read"
        );
        let _ = m.detail_title();
        let _ = m.list_rows();
        m.list_insert(None);
    }
}

/// The editors this app has: the Piano Roll, the List and the Strip
/// Chart (slice 10c); the Drum Grid comes later.
#[test]
fn the_strip_chart_is_available() {
    let available: Vec<_> = DetailEditorKind::ALL
        .into_iter()
        .filter(|k| k.is_available())
        .collect();
    assert_eq!(
        available,
        [
            DetailEditorKind::PianoRoll,
            DetailEditorKind::List,
            DetailEditorKind::StripChart
        ]
    );
}

/// A song saved with a Strip Chart as the last used editor: a
/// double-click opens it.
#[test]
fn a_saved_strip_chart_last_editor_opens_the_chart() {
    let f = Arc::new(editor_fake());
    f.set_layout(Some(r#"{"lastKind":"stripChart"}"#));
    let mut m = model(&f);
    m.open_detail(block(0, 10), None);
    assert_eq!(m.detail_kind_name(), "strip_chart");
}

/// A song saved with an editor this app does not have yet as the last
/// used one (the Drum Grid): a double-click opens the Piano Roll, and the
/// saved last-used editor stays.
#[test]
fn an_unported_last_editor_opens_the_piano_roll() {
    let (json, kind) = ("drumGrid", DetailEditorKind::DrumGrid);
    let f = Arc::new(editor_fake());
    f.set_layout(Some(&format!(r#"{{"lastKind":"{json}"}}"#)));
    let mut m = model(&f);
    assert_eq!(m.layout().last_kind, kind);
    m.open_detail(block(0, 10), None);
    assert_eq!(m.detail_kind_name(), "piano_roll");
    assert_eq!(m.layout().last_kind, kind, "the saved one stays");
    let v: serde_json::Value = serde_json::from_str(&m.layout().encoded()).unwrap();
    assert_eq!(v["lastKind"], json);
}

/// `editors.strip_chart` (⌘3) shows the Strip Chart; the torn-off window
/// comes later.
#[test]
fn editors_strip_chart_shows_the_chart() {
    let f = Arc::new(editor_fake());
    let mut m = model(&f);
    assert!(commands::EDITOR_COMMANDS.contains(&"editors.strip_chart"));
    assert!(!commands::EDITOR_COMMANDS.contains(&"editors.strip_chart_window"));
    commands::run("editors.strip_chart", &[], &mut m).unwrap();
    assert_eq!(m.detail_kind_name(), "strip_chart");
}
