//! The View menu's commands, `ui.pane` and `state.get layout` (Swift
//! `ViewCommandsTests`), the channel's automation commands (Swift
//! `AutomationCommandsTests`, the parts this slice ports) and
//! `mixer.new_bus` (Swift `MixerCommandsTests`), through `commands::run`.

mod support;

use std::sync::Arc;

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use support::{FakeSession, dest_row, lane_row, mixer_fake, strip_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{self, MIXER_COMMANDS, VIEW_COMMANDS, perform_from_menu, run};
use vizdaw_app::editor::{DetailEditorKind, MainView, PaneSide, PaneState, PaneView};
use vizdaw_app::mixer::MixerPlacement;
use vizdaw_app::view::{self, SET_PANE_NEEDS, StripFocus};
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{
    AutomationModeRow, GestureHolderRow, GesturePhaseRow, parse_automation_target,
};
use vizdaw_ffi::mixer::StripKindRow;
use vizdaw_ffi::tracks::TrackKindRow;

type Args = Vec<(String, ArgValue)>;

fn t(n: &str, v: &str) -> (String, ArgValue) {
    (n.into(), ArgValue::Text(v.into()))
}
fn i(n: &str, v: i64) -> (String, ArgValue) {
    (n.into(), ArgValue::Int(v))
}
fn b(n: &str, v: bool) -> (String, ArgValue) {
    (n.into(), ArgValue::Flag(v))
}

/// Synth (1), Bus 1 (2) and Master (0), with the mixer made.
fn doc() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(mixer_fake());
    let d = DocumentModel::new(f.clone()).unwrap();
    (f, d)
}

fn code(r: &Result<commands::Ran, ControlError>) -> Option<ErrorCode> {
    r.as_ref().err().map(|e| e.code)
}

fn status(r: &Result<commands::Ran, ControlError>) -> Option<String> {
    r.as_ref().ok().and_then(|r| r.status.clone())
}

fn pane(d: &DocumentModel, main: MainView, side: PaneSide) -> PaneState {
    *d.layout().views.setup(main).pane(side)
}

fn model_err(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn busy(m: &str) -> VizError {
    VizError::Busy { message: m.into() }
}

// View commands (Swift `ViewCommandsTests`).

#[test]
fn view_keys_switch_the_main_view() {
    let (_f, mut d) = doc();
    assert!(run("ui.view_mix", &[], &mut d).is_ok());
    assert_eq!(d.layout().views.main, MainView::Mix);
    assert!(commands::is_on("ui.view_mix", &d));
    assert!(!commands::is_on("ui.view_arrange", &d));
    run("ui.view_arrange", &[], &mut d).unwrap();
    assert_eq!(d.layout().views.main, MainView::Arrange);
    // The checked item chosen again keeps its view.
    perform_from_menu("ui.view_mix", &mut d).unwrap();
    perform_from_menu("ui.view_mix", &mut d).unwrap();
    assert_eq!(d.layout().views.main, MainView::Mix);
    for id in VIEW_COMMANDS.iter().chain(MIXER_COMMANDS) {
        assert!(commands::enabled(id, &d), "{id}");
    }
}

#[test]
fn pane_toggles_show_and_hide() {
    let (_f, mut d) = doc();
    run("ui.toggle_right", &[], &mut d).unwrap();
    assert!(d.layout().views.arrange.right.shown);
    assert!(commands::is_on("ui.toggle_right", &d));
    run("ui.toggle_right", &[b("on", true)], &mut d).unwrap();
    assert!(d.layout().views.arrange.right.shown);
    run("ui.toggle_left", &[b("on", false)], &mut d).unwrap();
    assert!(!d.layout().views.arrange.left.shown);
    run("ui.toggle_bottom", &[], &mut d).unwrap();
    assert!(!d.layout().views.arrange.bottom.shown);
    // The current view's panes: the mix view's are its own.
    run("ui.view_mix", &[], &mut d).unwrap();
    run("ui.toggle_left", &[], &mut d).unwrap();
    assert!(d.layout().views.mix.left.shown);
    assert!(!d.layout().views.arrange.left.shown);
    let e = run("ui.toggle_left", &[t("on", "yes")], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

#[test]
fn ui_pane_sets_view_shown_and_size() {
    let (_f, mut d) = doc();
    let r = run(
        "ui.pane",
        &[t("pane", "left"), t("view", "sequences"), i("size", 9_000)],
        &mut d,
    );
    assert_eq!(status(&r).as_deref(), Some("left sequences 480 shown"));
    // The answer carries it, not the status line.
    assert_eq!(d.status(), None);

    run("ui.pane", &[t("pane", "bottom"), t("view", "list")], &mut d).unwrap();
    assert_eq!(d.layout().views.arrange.bottom.view, PaneView::Editor);
    assert_eq!(d.layout().last_kind, DetailEditorKind::List);
    assert_eq!(d.detail_kind_name(), "list");

    let r = run(
        "ui.pane",
        &[
            t("pane", "right"),
            t("view", "markers"),
            b("shown", true),
            t("main", "mix"),
        ],
        &mut d,
    );
    assert_eq!(status(&r).as_deref(), Some("right markers 260 shown"));
    assert_eq!(d.layout().views.main, MainView::Arrange);
    assert_eq!(
        d.layout().views.mix.right,
        PaneState {
            shown: true,
            view: PaneView::Markers,
            size: 260.
        }
    );

    run(
        "ui.pane",
        &[
            t("pane", "bottom"),
            t("view", "drum_grid"),
            b("shown", false),
        ],
        &mut d,
    )
    .unwrap();
    assert!(!d.layout().views.arrange.bottom.shown);
    assert_eq!(d.layout().last_kind, DetailEditorKind::DrumGrid);

    run(
        "ui.pane",
        &[t("pane", "bottom"), t("view", "list"), t("main", "mix")],
        &mut d,
    )
    .unwrap();
    assert!(d.layout().views.mix.bottom.shown);
    assert!(!d.layout().views.arrange.bottom.shown);

    // The e2e's step: right markers 300 in the mix view.
    let r = run(
        "ui.pane",
        &[
            t("pane", "right"),
            t("view", "markers"),
            i("size", 300),
            b("shown", true),
            t("main", "mix"),
        ],
        &mut d,
    );
    assert_eq!(status(&r).as_deref(), Some("right markers 300 shown"));
    // A size below the range is clamped; `shown` alone keeps the view.
    let r = run(
        "ui.pane",
        &[t("pane", "right"), i("size", 3), b("shown", false)],
        &mut d,
    );
    assert_eq!(status(&r).as_deref(), Some("right controllers 180 hidden"));
}

#[test]
fn ui_pane_with_no_detail_target_remembers_the_kind() {
    let (_f, mut d) = doc();
    d.select(None);
    run("ui.pane", &[t("pane", "bottom"), t("view", "list")], &mut d).unwrap();
    assert_eq!(d.layout().detail, None);
    assert_eq!(d.layout().last_kind, DetailEditorKind::List);
}

#[test]
fn ui_pane_takes_the_audio_editor_only_at_the_bottom() {
    let (_f, mut d) = doc();
    let r = run(
        "ui.pane",
        &[t("pane", "bottom"), t("view", "audio_editor")],
        &mut d,
    );
    assert_eq!(d.layout().views.arrange.bottom.view, PaneView::AudioEditor);
    assert_eq!(status(&r).as_deref(), Some("bottom audio_editor 260 shown"));
    let before = d.layout().clone();
    let e = run(
        "ui.pane",
        &[t("pane", "left"), t("view", "audio_editor")],
        &mut d,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(
        e.message,
        "audio_editor is not on the left pane's list in the arrange view."
    );
    assert_eq!(*d.layout(), before);
}

#[test]
fn ui_commands_check_their_arguments() {
    let (_f, mut d) = doc();
    let before = d.layout().clone();
    for args in [
        vec![t("pane", "left")],
        vec![t("pane", "left"), t("view", "mixer")],
        vec![t("pane", "right"), t("view", "piano_roll")],
        vec![t("pane", "bottom"), t("view", "mixer"), t("main", "mix")],
        vec![t("pane", "middle"), b("shown", true)],
        vec![t("pane", "left"), t("main", "edit"), b("shown", true)],
        vec![t("pane", "left"), t("size", "wide")],
        vec![t("pane", "left"), t("view", "nothing")],
        vec![i("pane", 1), b("shown", true)],
        vec![t("pane", "left"), b("shown", true), b("shown", false)],
        vec![t("pane", "left"), b("shown", true), t("width", "1")],
    ] {
        let r = run("ui.pane", &args, &mut d);
        assert_eq!(code(&r), Some(ErrorCode::InvalidParams), "{args:?}");
    }
    let e = run("ui.pane", &[t("pane", "left")], &mut d).unwrap_err();
    assert_eq!(e.message, "ui.pane: give `view`, `shown` or `size`");
    let e = run("ui.pane", &[t("pane", "middle")], &mut d).unwrap_err();
    assert_eq!(e.message, "ui.pane: `pane` is left, bottom or right");
    let e = run(
        "ui.pane",
        &[t("pane", "left"), t("main", "edit"), b("shown", true)],
        &mut d,
    )
    .unwrap_err();
    assert_eq!(e.message, "ui.pane: `main` is arrange or mix");
    let e = run(
        "ui.pane",
        &[t("pane", "right"), t("view", "piano_roll")],
        &mut d,
    )
    .unwrap_err();
    assert_eq!(
        e.message,
        "piano_roll is not on the right pane's list in the arrange view."
    );
    let e = run(
        "ui.pane",
        &[t("pane", "bottom"), t("view", "mixer"), t("main", "mix")],
        &mut d,
    )
    .unwrap_err();
    assert_eq!(
        e.message,
        "mixer is not on the bottom pane's list in the mix view."
    );
    // With no arguments (a key or the menu): unavailable, in the key's
    // wording, and in the status line from the menu.
    let e = run("ui.pane", &[], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, SET_PANE_NEEDS);
    assert!(perform_from_menu("ui.pane", &mut d).is_err());
    assert_eq!(d.status(), Some(SET_PANE_NEEDS));
    d.set_status(None);
    // No detail opened, the last kind and the bottom pane unchanged.
    assert_eq!(*d.layout(), before);
    let e = run("ui.view_mix", &[b("on", true)], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(*d.layout(), before);
}

/// Lane 30 plays through audio path 7, which strip 10 is.
fn lane_on_strip_10() -> (Arc<FakeSession>, DocumentModel) {
    let f = mixer_fake();
    let mut on_path = lane_row(30, "Groove", false);
    on_path.audio_out = Some(7);
    f.set_lanes(3, vec![on_path, lane_row(31, "Chords", false)]);
    f.edit_mixer(|m| {
        m.strips.insert(
            0,
            strip_row(10, "Out 1", StripKindRow::AudioPath { path: 7 }),
        )
    });
    let f = Arc::new(f);
    let d = DocumentModel::new(f.clone()).unwrap();
    (f, d)
}

#[test]
fn state_get_layout_reports_both_views_and_the_left_strip() {
    let (f, mut d) = lane_on_strip_10();
    d.select_track(30, false);
    let m = f.mixer_snapshot();
    let row = view::layout_row(&d, &m.strips, &[]);
    assert_eq!(row.main, "arrange");
    assert_eq!(
        row.views
            .iter()
            .map(|v| v.main.as_str())
            .collect::<Vec<_>>(),
        ["arrange", "mix"]
    );
    assert_eq!(
        row.views[0]
            .panes
            .iter()
            .map(|p| p.pane.as_str())
            .collect::<Vec<_>>(),
        ["left", "bottom", "right"]
    );
    assert_eq!(row.views[0].panes[1].view, "piano_roll");
    assert_eq!(
        row.views[1].panes[1].views,
        [
            "piano_roll",
            "list",
            "strip_chart",
            "drum_grid",
            "audio_editor"
        ]
    );
    assert_eq!(
        row.views[0].panes[1].views,
        [
            "piano_roll",
            "list",
            "strip_chart",
            "drum_grid",
            "audio_editor",
            "mixer"
        ]
    );
    assert_eq!(
        row.views[0].panes[0].views,
        ["strip", "sequences", "inspector"]
    );
    assert_eq!(
        row.views[0].panes[2].views,
        ["controllers", "media_pool", "plugins", "markers"]
    );
    assert_eq!(row.views[0].panes[0].size, 220.);
    assert!(row.views[0].panes[0].shown && !row.views[0].panes[2].shown);
    assert!(row.views[1].panes.iter().all(|p| !p.shown));
    assert_eq!(row.left_strip, Some(10));
    assert_eq!(row.left_shows, "strip");
    run("ui.toggle_left", &[b("on", false)], &mut d).unwrap();
    let hidden = view::layout_row(&d, &m.strips, &[]);
    assert_eq!(hidden.left_shows, "hidden");
    assert_eq!(hidden.left_strip, Some(10));
    run("ui.toggle_left", &[b("on", true)], &mut d).unwrap();
    run(
        "ui.pane",
        &[t("pane", "left"), t("view", "sequences")],
        &mut d,
    )
    .unwrap();
    assert_eq!(view::layout_row(&d, &m.strips, &[]).left_shows, "sequences");
}

#[test]
fn the_left_strip_rule_follows_the_selection_else_the_armed_track() {
    let (f, mut d) = lane_on_strip_10();
    let strips = f.mixer_snapshot().strips;
    // Nothing selected, nothing armed.
    assert_eq!(d.left_strip(&strips, &[]), StripFocus::Nothing);
    assert_eq!(view::layout_row(&d, &strips, &[]).left_shows, "nothing");
    // Lane 31 is a MIDI track with no destination: its MIDI strip.
    d.select_track(31, false);
    assert_eq!(
        d.left_strip(&strips, &[]),
        StripFocus::Midi {
            dest: None,
            lane: 31
        }
    );
    assert_eq!(view::layout_row(&d, &strips, &[]).left_shows, "midi");
    d.select_track(30, false);
    assert_eq!(d.left_strip(&strips, &[]), StripFocus::Strip(10));
    // An audio track that plays nothing shows the inspector.
    let mut audio = lane_row(31, "Chords", false);
    audio.kind = TrackKindRow::Audio;
    assert_eq!(
        StripFocus::of(&[&audio], &[], &strips),
        StripFocus::Inspector
    );
    // A track on the built-in synth's destination shows the synth's strip;
    // one on an external destination its MIDI strip.
    let mut synth = lane_row(32, "Keys", false);
    synth.midi_dest = Some(5);
    let mut dests = vec![dest_row(5, "Synth")];
    assert_eq!(
        StripFocus::of(&[&synth], &dests, &strips),
        StripFocus::Strip(1)
    );
    dests[0].external = true;
    assert_eq!(
        StripFocus::of(&[&synth], &dests, &strips),
        StripFocus::Midi {
            dest: Some(5),
            lane: 32
        }
    );
    // Nothing selected: the first armed track decides.
    d.clear_selection();
    let mut armed = lane_row(30, "Groove", false);
    armed.arm = true;
    armed.audio_out = Some(7);
    f.set_lanes(3, vec![lane_row(31, "Chords", false), armed]);
    let mut d = DocumentModel::new(f.clone()).unwrap();
    d.clear_selection();
    assert_eq!(d.left_strip(&strips, &[]), StripFocus::Strip(10));
}

#[test]
fn a_view_change_commits_the_strip_rename() {
    let (f, mut d) = doc();
    assert!(d.ensure_mixer());
    d.with_mixer(|m, _| {
        m.begin_rename(1, MixerPlacement::View);
        m.rename_draft = "Vox 2".into();
    });
    // Arrange chosen while Arrange shows changes nothing and keeps it.
    run("ui.view_arrange", &[], &mut d).unwrap();
    assert!(d.mixer().unwrap().renaming().is_some());
    // A refused `ui.pane` keeps it.
    let _ = run(
        "ui.pane",
        &[t("pane", "left"), t("view", "markers")],
        &mut d,
    );
    assert!(d.mixer().unwrap().renaming().is_some());
    run("ui.toggle_left", &[], &mut d).unwrap();
    assert!(d.mixer().unwrap().renaming().is_none());
    assert_eq!(f.count("rename_strip(1,Vox 2)"), 1);
    // A toggle that changes nothing ends nothing.
    d.with_mixer(|m, _| m.begin_rename(1, MixerPlacement::View));
    run("ui.toggle_left", &[b("on", false)], &mut d).unwrap();
    assert!(d.mixer().unwrap().renaming().is_some());
}

#[test]
fn a_detail_key_over_the_mixer_commits_the_strip_rename() {
    let (f, mut d) = doc();
    assert!(d.ensure_mixer());
    run(
        "ui.pane",
        &[t("pane", "bottom"), t("view", "mixer")],
        &mut d,
    )
    .unwrap();
    assert_eq!(d.layout().views.current().bottom.view, PaneView::Mixer);
    d.with_mixer(|m, _| {
        m.begin_rename(1, MixerPlacement::Pane);
        m.rename_draft = "Vox 2".into();
    });
    run("editors.piano_roll", &[], &mut d).unwrap();
    assert_eq!(f.count("rename_strip(1,Vox 2)"), 1);
    assert!(d.mixer().unwrap().renaming().is_none());
    assert_eq!(d.layout().views.current().bottom.view, PaneView::Editor);
    // Already on the editor: this path commits nothing.
    d.with_mixer(|m, _| {
        m.begin_rename(1, MixerPlacement::Pane);
        m.rename_draft = "Vox 3".into();
    });
    run("editors.list", &[], &mut d).unwrap();
    assert_eq!(f.count("rename_strip(1,Vox 3)"), 0);
    assert!(d.mixer().unwrap().renaming().is_some());
}

#[test]
fn a_refused_choose_leaves_the_layout_untouched() {
    let (_f, mut d) = doc();
    d.update_layout(|l| l.views.arrange.bottom.shown = false);
    let kept = d.layout().clone();
    let refusal = view::choose(
        PaneSide::Right,
        PaneView::Editor,
        Some(DetailEditorKind::PianoRoll),
        None,
        None,
        None,
        &mut d,
    );
    assert_eq!(
        refusal,
        Err("editor is not on the right pane's list in the arrange view.".into())
    );
    assert_eq!(*d.layout(), kept);
    assert_eq!(d.layout().detail, None);
    assert_eq!(
        view::choose(
            PaneSide::Bottom,
            PaneView::Editor,
            Some(DetailEditorKind::List),
            None,
            None,
            None,
            &mut d,
        ),
        Ok(())
    );
    assert!(d.layout().views.arrange.bottom.shown);
    assert_eq!(d.layout().last_kind, DetailEditorKind::List);
}

#[test]
fn ui_pane_does_not_reach_the_session() {
    let (f, mut d) = doc();
    f.clear_calls();
    run(
        "ui.pane",
        &[t("pane", "right"), t("view", "markers"), b("shown", true)],
        &mut d,
    )
    .unwrap();
    assert!(f.calls().is_empty(), "{:?}", f.calls());
    assert_eq!(
        pane(&d, MainView::Arrange, PaneSide::Right).view,
        PaneView::Markers
    );
}

// The Mixer window.

#[test]
fn window_mixer_asks_the_ui_to_open_the_window() {
    let (_f, mut d) = doc();
    let ran = run("window.mixer", &[], &mut d).unwrap();
    assert!(ran.open_mixer_window);
    assert!(
        perform_from_menu("window.mixer", &mut d)
            .unwrap()
            .open_mixer_window
    );
    // No other command asks for it.
    assert!(!run("ui.view_mix", &[], &mut d).unwrap().open_mixer_window);
    let e = run("window.mixer", &[i("strip", 1)], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

// Automation (Swift `AutomationCommandsTests`).

#[test]
fn automation_commands_check_their_arguments() {
    let (f, mut d) = doc();
    let set = |d: &mut DocumentModel, strips: &str, mode: &str| {
        run(
            "automation.set_mode",
            &[t("strips", strips), t("mode", mode)],
            d,
        )
    };
    assert!(set(&mut d, "all", "touch").is_ok());
    assert_eq!(
        f.mode_calls().last().cloned(),
        Some((vec![1, 2, 0], AutomationModeRow::Touch))
    );
    assert!(set(&mut d, "2,1", "off").is_ok());
    assert_eq!(f.mode_calls().last().unwrap().0, [2, 1]);
    assert!(set(&mut d, "1,1", "read").is_ok());
    assert_eq!(
        f.mode_calls().last().unwrap().0,
        [1],
        "a strip listed twice is set once"
    );
    let calls = f.mode_calls().len();
    for bad in [
        "",
        "1,,2",
        "1,x",
        "-1",
        "1.5",
        " 1",
        "99999999999999999999999",
    ] {
        assert_eq!(
            code(&set(&mut d, bad, "read")),
            Some(ErrorCode::InvalidParams),
            "{bad}"
        );
    }
    assert_eq!(f.mode_calls().len(), calls);

    let gesture =
        |d: &mut DocumentModel, args: &[(String, ArgValue)]| run("automation.gesture", args, d);
    assert!(gesture(&mut d, &[t("target", "fader:1"), t("phase", "touch")]).is_ok());
    assert_eq!(
        f.gestures().last().unwrap().1,
        GesturePhaseRow::Touch { value: None }
    );
    assert!(
        gesture(
            &mut d,
            &[
                t("target", "fader:1"),
                t("phase", "value"),
                t("value", "0.25")
            ]
        )
        .is_ok()
    );
    assert_eq!(
        f.gestures().last().unwrap().1,
        GesturePhaseRow::Value { value: 0.25 }
    );
    let taken = f.gestures().len();
    for (target, phase, value) in [
        ("fader:x", "touch", None),
        ("velocity:1", "touch", None),
        ("fader:1", "value", None),
        ("fader:1", "value", Some("nan")),
        ("fader:1", "value", Some("1e400")),
        ("fader:1", "value", Some("-0.5")),
        ("fader:1", "value", Some("1.5")),
        ("fader:1", "value", Some("0,5")),
        ("fader:1", "touch", Some("inf")),
        ("fader:1", "value", Some("1e-1")),
        ("fader:1", "value", Some("0x1p-2")),
        ("fader:1", "value", Some(" 0.5")),
    ] {
        let mut args = vec![t("target", target), t("phase", phase)];
        if let Some(v) = value {
            args.push(t("value", v));
        }
        assert_eq!(
            code(&gesture(&mut d, &args)),
            Some(ErrorCode::InvalidParams),
            "{target} {phase} {value:?}"
        );
    }
    assert_eq!(f.gestures().len(), taken);
    // The core's refusal is the call's failure; its Busy is `busy`.
    f.fail_next("gesture", model_err("unknown id strip9"));
    let e = gesture(&mut d, &[t("target", "fader:9"), t("phase", "touch")]).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "Automation Failed: unknown id strip9");
    f.fail_next("gesture", busy("Finish the open edit first."));
    let e = gesture(&mut d, &[t("target", "fader:1"), t("phase", "touch")]).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert_eq!(e.message, "Finish the open edit first.");
    assert!(d.alert.is_none());
}

#[test]
fn release_all_releases_the_channels_touches() {
    let (f, mut d) = doc();
    f.clear_calls();
    f.set_channel_touches(2);
    let two = run("automation.release_all", &[], &mut d);
    assert_eq!(status(&two).as_deref(), Some("Released 2 touches."));
    f.set_channel_touches(1);
    let one = run("automation.release_all", &[], &mut d);
    assert_eq!(status(&one).as_deref(), Some("Released 1 touch."));
    f.set_channel_touches(0);
    let none = run("automation.release_all", &[], &mut d);
    assert_eq!(
        status(&none).as_deref(),
        Some("The channel holds no touch.")
    );
    assert_eq!(d.status(), Some("The channel holds no touch."));
    assert_eq!(f.count("release_channel_touches"), 3);
    for args in [
        vec![i("x", 1)],
        vec![t("all", "\0")],
        vec![t("target", &"x".repeat(70_000))],
        vec![i("strip", 99_999)],
    ] {
        let r = run("automation.release_all", &args, &mut d);
        assert_eq!(code(&r), Some(ErrorCode::InvalidParams), "{:?}", args[0].0);
    }
    assert_eq!(f.count("release_channel_touches"), 3, "no call made");
    f.fail_next(
        "release_channel_touches",
        busy("Finish the open edit first."),
    );
    let r = run("automation.release_all", &[], &mut d);
    assert_eq!(code(&r), Some(ErrorCode::Busy));
    // The menu and key path: the count in the status line.
    f.set_channel_touches(1);
    perform_from_menu("automation.release_all", &mut d).unwrap();
    assert_eq!(d.status(), Some("Released 1 touch."));
}

#[test]
fn the_channel_gestures_as_its_own_holder() {
    let (f, mut d) = doc();
    run(
        "automation.gesture",
        &[t("target", "fader:1"), t("phase", "touch")],
        &mut d,
    )
    .unwrap();
    assert_eq!(f.holders(), [GestureHolderRow::Channel]);
    assert!(!f.gestures().last().unwrap().2, "the group follows");
    run(
        "automation.gesture",
        &[
            t("target", "fader:1"),
            t("phase", "release"),
            b("alone", true),
        ],
        &mut d,
    )
    .unwrap();
    assert!(f.gestures().last().unwrap().2);
}

#[test]
fn set_mode_is_unavailable_during_an_editor_drag() {
    let (f, mut d) = doc();
    assert!(d.begin_gesture("Move", vec![]));
    let r = run(
        "automation.set_mode",
        &[t("strips", "all"), t("mode", "touch")],
        &mut d,
    );
    let e = r.unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Finish the drag first.");
    assert!(f.mode_calls().is_empty());
}

#[test]
fn a_mode_change_fails_under_one_title() {
    let (f, mut d) = doc();
    f.fail_next("set_automation_mode", model_err("boom"));
    let e = run(
        "automation.set_mode",
        &[t("strips", "1"), t("mode", "read")],
        &mut d,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "Automation Mode Failed: boom");
    f.fail_next(
        "set_automation_mode",
        busy("Release the touched control first."),
    );
    let e = run(
        "automation.set_mode",
        &[t("strips", "1"), t("mode", "read")],
        &mut d,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert_eq!(e.message, "Release the touched control first.");
    // `all` reads the mixer first: its refusal is the mode's failure.
    f.fail_next("mixer", model_err("gone"));
    let e = run(
        "automation.set_mode",
        &[t("strips", "all"), t("mode", "read")],
        &mut d,
    )
    .unwrap_err();
    assert_eq!(e.message, "Automation Mode Failed: gone");
    assert!(d.alert.is_none());
}

#[test]
fn a_mode_change_is_applied() {
    let (f, mut d) = doc();
    let before = d.snapshot().version;
    run(
        "automation.set_mode",
        &[t("strips", "1"), t("mode", "write")],
        &mut d,
    )
    .unwrap();
    assert!(d.snapshot().version > before, "the snapshot is applied");
    assert_eq!(
        f.mixer_snapshot().strips[0].automation,
        AutomationModeRow::Write
    );
}

/// A gesture begun over the channel always ends over it: the release
/// needs no value, and is passed on for a target that went meanwhile.
#[test]
fn a_channel_gesture_always_ends_over_the_channel() {
    let (f, mut d) = doc();
    for target in [
        "fader:1",
        "pan:2",
        "mute:0",
        "solo:1",
        "send:1:1",
        "param:10:4294967295",
        "fader:99",
    ] {
        let touch = run(
            "automation.gesture",
            &[t("target", target), t("phase", "touch")],
            &mut d,
        );
        assert!(touch.is_ok(), "{target}");
        let release = run(
            "automation.gesture",
            &[t("target", target), t("phase", "release")],
            &mut d,
        );
        assert!(release.is_ok(), "{target}");
        let last = f.gestures().last().cloned().unwrap();
        assert_eq!(Some(last.0), parse_automation_target(target.into()));
        assert_eq!(last.1, GesturePhaseRow::Release);
    }
    let before = f.gestures().len();
    let r = run(
        "automation.gesture",
        &[
            t("target", "fader:1"),
            t("phase", "release"),
            t("value", "0.5"),
        ],
        &mut d,
    );
    assert_eq!(code(&r), Some(ErrorCode::InvalidParams));
    assert_eq!(
        r.unwrap_err().message,
        "automation.gesture: release takes no `value`"
    );
    assert_eq!(f.gestures().len(), before);
}

#[test]
fn a_released_channel_touch_shows_its_step() {
    let (f, mut d) = doc();
    run(
        "automation.gesture",
        &[
            t("target", "fader:1"),
            t("phase", "touch"),
            t("value", "0.5"),
        ],
        &mut d,
    )
    .unwrap();
    let before = d.snapshot().version;
    run(
        "automation.gesture",
        &[t("target", "fader:1"), t("phase", "release")],
        &mut d,
    )
    .unwrap();
    assert!(d.snapshot().version > before);
    assert_eq!(d.snapshot().version, f.snap().version);
}

#[test]
fn hostile_arguments_never_reach_the_core() {
    let (f, mut d) = doc();
    for args in [
        vec![t("strips", "all")],
        vec![t("mode", "read")],
        vec![t("strips", "all"), t("mode", "loud")],
        vec![t("strips", "all"), t("mode", "READ")],
        vec![t("strips", "all"), b("mode", true)],
        vec![i("strips", 1), t("mode", "read")],
        vec![
            t("strips", &format!("{}1", "1,".repeat(50_000))),
            t("mode", "read"),
        ],
        vec![t("strips", "all,1"), t("mode", "read")],
        vec![t("strips", "1,"), t("mode", "read")],
    ] {
        let r = run("automation.set_mode", &args, &mut d);
        assert_eq!(code(&r), Some(ErrorCode::InvalidParams), "{args:?}");
    }
    let e = run("automation.set_mode", &[t("strips", "all")], &mut d).unwrap_err();
    assert_eq!(
        e.message,
        "automation.set_mode: `mode` is off, read, touch, latch, write or trim"
    );
    let e = run(
        "automation.set_mode",
        &[t("strips", "1,"), t("mode", "read")],
        &mut d,
    )
    .unwrap_err();
    assert_eq!(
        e.message,
        "automation.set_mode: `strips` is all or strip IDs such as \"1,2\""
    );
    assert!(f.mode_calls().is_empty());
    let long_value = "0".repeat(100_000);
    let long_target = "9".repeat(100_000);
    for args in [
        vec![t("phase", "touch")],
        vec![t("target", "fader:1")],
        vec![t("target", "fader:1"), t("phase", "wiggle")],
        vec![i("target", 1), t("phase", "touch")],
        vec![t("target", "fader:1"), t("phase", "value"), i("value", 0)],
        vec![t("target", &long_target), t("phase", "touch")],
        vec![
            t("target", "fader:1"),
            t("phase", "value"),
            t("value", &long_value),
        ],
        vec![
            t("target", "fader:1"),
            t("phase", "value"),
            t("value", "inf"),
        ],
        vec![t("target", "fader:1"), t("phase", "value"), t("value", ".")],
        vec![
            t("target", "fader:1"),
            t("phase", "value"),
            t("value", "0.5.1"),
        ],
        vec![t("target", "fader:1"), t("phase", "value"), t("value", "")],
        vec![
            t("target", "fader:1"),
            t("phase", "value"),
            t("value", "\u{660}.\u{665}"),
        ],
    ] {
        let r = run("automation.gesture", &args, &mut d);
        assert_eq!(code(&r), Some(ErrorCode::InvalidParams), "{:?}", args);
    }
    let messages = [
        (
            vec![t("phase", "touch")],
            "automation.gesture: `target` is fader:<strip>, pan:<strip>, mute:<strip>, solo:<strip>, send:<strip>:<n> (n from 1), param:<slot>:<param> or vca:<vca>",
        ),
        (
            vec![
                t("target", "fader:1"),
                t("phase", "value"),
                t("value", "nan"),
            ],
            "automation.gesture: `value` is a number from 0 to 1 such as \"0.5\"",
        ),
        (
            vec![t("target", "fader:1"), t("phase", "value")],
            "automation.gesture: `value` is needed for value (0 to 1)",
        ),
        (
            vec![t("target", "fader:1"), t("phase", "wiggle")],
            "automation.gesture: `phase` is touch, value or release",
        ),
    ];
    for (args, message) in messages {
        let e = run("automation.gesture", &args, &mut d).unwrap_err();
        assert_eq!(e.message, message);
    }
    assert!(f.gestures().is_empty());
    // The edges of the range are values.
    for v in ["0", "1.0", ".5"] {
        run(
            "automation.gesture",
            &[t("target", "fader:1"), t("phase", "value"), t("value", v)],
            &mut d,
        )
        .unwrap();
    }
    let values: Vec<_> = f.gestures().iter().map(|g| g.1).collect();
    assert_eq!(
        values,
        [
            GesturePhaseRow::Value { value: 0. },
            GesturePhaseRow::Value { value: 1. },
            GesturePhaseRow::Value { value: 0.5 },
        ]
    );
}

// New Bus (Swift `MixerCommandsTests`).

#[test]
fn new_bus_command_runs() {
    let (f, mut d) = doc();
    f.clear_calls();
    let r = run("mixer.new_bus", &[i("strip", 1)], &mut d);
    assert_eq!(status(&r).as_deref(), Some("Bus 2"));
    assert_eq!(d.status(), Some("Bus 2"));
    assert_eq!(f.calls(), ["new_bus_from(1,Bus 2)"]);

    f.clear_calls();
    let alone = run("mixer.new_bus", &[], &mut d);
    assert_eq!(status(&alone).as_deref(), Some("Bus 3"));
    assert_eq!(f.calls(), ["add_bus(Bus 3)"]);

    f.clear_calls();
    let named = run(
        "mixer.new_bus",
        &[i("strip", 1), t("name", "  Drums ")],
        &mut d,
    );
    assert_eq!(status(&named).as_deref(), Some("Drums"));
    assert_eq!(f.calls(), ["new_bus_from(1,Drums)"]);
    assert!(d.alert.is_none());
    assert_eq!(
        d.snapshot().version,
        f.snap().version,
        "the snapshot is applied"
    );
}

#[test]
fn new_bus_command_refuses_bad_input() {
    let (f, mut d) = doc();
    f.clear_calls();
    let cases: [(Args, ErrorCode, Option<&str>); 7] = [
        (
            vec![i("strip", 1), t("name", "a\0b")],
            ErrorCode::InvalidParams,
            Some("mixer.new_bus: `name` is text without a NUL"),
        ),
        (
            vec![t("name", "  ")],
            ErrorCode::InvalidParams,
            Some("mixer.new_bus: `name` is not blank"),
        ),
        (
            vec![i("strip", 0)],
            ErrorCode::InvalidParams,
            Some("Master has no sends."),
        ),
        (
            vec![i("strip", 999)],
            ErrorCode::UnknownId,
            Some("The mixer has no strip 999."),
        ),
        (
            vec![i("strip", -1)],
            ErrorCode::InvalidParams,
            Some("mixer.new_bus: `strip` is a strip ID (see state.get mixer)"),
        ),
        (
            vec![t("strip", "1")],
            ErrorCode::InvalidParams,
            Some("mixer.new_bus: `strip` is a strip ID (see state.get mixer)"),
        ),
        (vec![i("bus", 1)], ErrorCode::InvalidParams, None),
    ];
    for (args, want, message) in cases {
        let e = run("mixer.new_bus", &args, &mut d).unwrap_err();
        assert_eq!(e.code, want, "{args:?}: {}", e.message);
        if let Some(m) = message {
            assert_eq!(e.message, m, "{args:?}");
        }
    }
    assert!(f.calls().is_empty(), "{:?}", f.calls());
    assert!(d.alert.is_none());
}

#[test]
fn a_refused_new_bus_is_failed_or_busy_and_a_gesture_stops_it() {
    let (f, mut d) = doc();
    f.fail_next("new_bus_from", model_err("Strip 9 is not in this song."));
    let e = run("mixer.new_bus", &[i("strip", 1)], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "New Bus Failed: Strip 9 is not in this song.");
    f.fail_next("add_bus", busy("Finish the edit first."));
    let e = run("mixer.new_bus", &[], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert_eq!(e.message, "Finish the edit first.");
    assert!(d.alert.is_none());
    f.clear_calls();
    assert!(d.begin_gesture("drag", vec![]));
    let e = run("mixer.new_bus", &[], &mut d).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Finish the drag first.");
    assert!(
        !f.calls().iter().any(|c| c.contains("bus")),
        "{:?}",
        f.calls()
    );
}

/// Swift `EditorModel.openDetail`: any detail opening over a bottom pane
/// that shows no editor commits the strip rename first (a double-clicked
/// block too), and so does `ui.pane` opening an editor kind in a named
/// main view.
#[test]
fn opening_a_detail_over_the_mixer_commits_the_strip_rename() {
    let (f, mut d) = doc();
    assert!(d.ensure_mixer());
    run(
        "ui.pane",
        &[t("pane", "bottom"), t("view", "mixer")],
        &mut d,
    )
    .unwrap();
    d.with_mixer(|m, _| {
        m.begin_rename(1, MixerPlacement::Pane);
        m.rename_draft = "Vox 2".into();
    });
    let block = support::block_row(0, 61_440, &[]);
    d.open_block_detail(30, &block);
    assert_eq!(f.count("rename_strip(1,Vox 2)"), 1);
    assert!(d.mixer().unwrap().renaming().is_none());
    assert_eq!(d.layout().views.current().bottom.view, PaneView::Editor);
    // Over the editor already: nothing ends.
    d.with_mixer(|m, _| {
        m.begin_rename(1, MixerPlacement::Pane);
        m.rename_draft = "Vox 3".into();
    });
    d.open_block_detail(31, &block);
    assert!(d.mixer().unwrap().renaming().is_some());
    // The mix view's bottom pane is not on the editor: opening there ends it.
    d.update_layout(|l| l.views.mix.bottom.view = PaneView::AudioEditor);
    d.open_detail_in(
        vizdaw_app::editor::EditorTarget::Sequence { seq: 3 },
        Some(DetailEditorKind::List),
        MainView::Mix,
    );
    assert_eq!(f.count("rename_strip(1,Vox 3)"), 1);
    assert_eq!(d.layout().views.mix.bottom.view, PaneView::Editor);
}
