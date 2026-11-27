//! The strip commands over the channel and the menus (Swift
//! `MixerCommandsTests` `mixerEditsRunThroughOneCall`,
//! `aRefusedMixerEditIsFailedOrBusy`, `aGestureStopsEveryMixerEdit`;
//! `ControlCommands.runStrip`'s `mixer.set_colour`; `AutomationCommandsTests`
//! for `automation.mode.*` and `automation.suspend`).

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;

use core_control::registry::{self, ArgValue};
use core_control::{ControlError, ErrorCode};
use support::{FakeSession, mixer_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{self, APP_COMMANDS, MIXER_COMMANDS, Ran, perform_from_menu, run};
use vizdaw_app::prefs::Prefs;
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{AutomationModeRow, GesturePhaseRow};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::events::AutomationTargetRow as T;

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

fn doc() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(mixer_fake());
    let d = DocumentModel::new(f.clone()).unwrap();
    (f, d)
}

fn model_err(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn busy(m: &str) -> VizError {
    VizError::Busy { message: m.into() }
}

fn err(r: Result<Ran, ControlError>) -> ControlError {
    r.expect_err("refused")
}

fn control_args(a: &Args) -> Vec<ControlArg> {
    a.iter()
        .map(|(n, v)| ControlArg {
            name: n.clone(),
            value: match v {
                ArgValue::Text(value) => ControlValue::Text {
                    value: value.clone(),
                },
                ArgValue::Flag(value) => ControlValue::Flag { value: *value },
                ArgValue::Int(value) => ControlValue::Int { value: *value },
            },
        })
        .collect()
}

/// The strip edits, as the channel passes them, and their titles.
fn edits() -> Vec<(&'static str, Args, &'static str)> {
    vec![
        (
            "mixer.set_strip",
            vec![i("strip", 1), t("gain_db", "-3"), b("mute", true)],
            "Set Strip",
        ),
        (
            "mixer.set_output",
            vec![i("strip", 1), i("bus", 2)],
            "Set Strip Output",
        ),
        (
            "mixer.add_send",
            vec![i("strip", 1), i("bus", 2), t("level_db", "-6")],
            "Add Send",
        ),
        (
            "mixer.set_send",
            vec![i("strip", 1), i("index", 1), t("level_db", "-6")],
            "Set Send",
        ),
        (
            "mixer.remove_send",
            vec![i("strip", 1), i("index", 1)],
            "Remove Send",
        ),
        ("mixer.remove_bus", vec![i("strip", 2)], "Remove Bus"),
    ]
}

#[test]
fn strip_edits_run_through_one_call_and_answer_their_title() {
    for (id, args, title) in edits() {
        let (f, mut d) = doc();
        assert!(MIXER_COMMANDS.contains(&id), "{id}");
        assert!(commands::enabled(id, &d), "{id}");
        let before = d.snapshot().version;
        let ran = run(id, &args, &mut d).unwrap_or_else(|e| panic!("{id}: {e:?}"));
        assert_eq!(ran.status.as_deref(), Some(title), "{id}");
        assert_eq!(registry::find(id).unwrap().title, title, "{id}");
        assert_eq!(
            f.run_mixers(),
            [(id.to_owned(), control_args(&args))],
            "{id}: the arguments unchanged"
        );
        assert!(
            d.snapshot().version > before,
            "{id}: the snapshot is applied"
        );
        assert!(d.alert.is_none(), "{id}");
    }
}

#[test]
fn a_refused_strip_edit_is_failed_in_the_sessions_words_or_busy() {
    for (id, args, _) in edits() {
        let (f, mut d) = doc();
        f.set_call_error("run_mixer", Some(model_err("Strip 9 is not in this song.")));
        let e = err(run(id, &args, &mut d));
        assert_eq!(e.code, ErrorCode::Failed, "{id}");
        assert_eq!(e.message, "Strip 9 is not in this song.", "{id}");
        f.set_call_error("run_mixer", Some(busy("Finish the edit first.")));
        let e = err(run(id, &args, &mut d));
        assert_eq!(e.code, ErrorCode::Busy, "{id}");
        assert_eq!(e.message, "Finish the edit first.", "{id}");
        assert!(d.alert.is_none(), "{id}: no dialog");
    }
}

#[test]
fn a_gesture_stops_every_strip_edit() {
    for (id, args, _) in edits().into_iter().chain([(
        "mixer.set_colour",
        vec![i("strip", 1), t("colour", "red")],
        "",
    )]) {
        let (f, mut d) = doc();
        assert!(d.begin_gesture("Move", vec![]));
        let e = err(run(id, &args, &mut d));
        assert_eq!(e.code, ErrorCode::Unavailable, "{id}");
        assert_eq!(e.message, "Finish the drag first.", "{id}");
        assert!(f.run_mixers().is_empty(), "{id}");
        assert!(
            !f.calls().iter().any(|c| c.starts_with("set_strip_colour")),
            "{id}"
        );
    }
}

/// The channel counts sends from 1 and the core's `run_mixer` takes them
/// so; the gesture target `send:<strip>:<n>` is 1-based too and reaches the
/// session as send n − 1.
#[test]
fn the_channel_counts_sends_from_one() {
    let (f, mut d) = doc();
    run(
        "mixer.set_send",
        &[i("strip", 1), i("index", 1), b("pre_fader", true)],
        &mut d,
    )
    .unwrap();
    run("mixer.remove_send", &[i("strip", 1), i("index", 2)], &mut d).unwrap();
    let runs = f.run_mixers();
    let index = |n: usize| {
        runs[n]
            .1
            .iter()
            .find(|a| a.name == "index")
            .map(|a| a.value.clone())
    };
    assert_eq!(index(0), Some(ControlValue::Int { value: 1 }));
    assert_eq!(index(1), Some(ControlValue::Int { value: 2 }));
    run(
        "automation.gesture",
        &[t("target", "send:1:1"), t("phase", "touch")],
        &mut d,
    )
    .unwrap();
    assert_eq!(
        f.gestures().last().map(|g| g.0),
        Some(T::SendLevel { strip: 1, send: 0 }),
        "the first send is send 0 at the session"
    );
    assert_eq!(
        f.gestures().last().map(|g| g.1),
        Some(GesturePhaseRow::Touch { value: None })
    );
}

/// A null `bus` (master) arrives as the registry's `none` text and goes
/// to the core as it came.
#[test]
fn set_output_to_master_passes_the_null() {
    let (f, mut d) = doc();
    let args = vec![i("strip", 1), t("bus", registry::NULL_TEXT)];
    run("mixer.set_output", &args, &mut d).unwrap();
    assert_eq!(
        f.run_mixers(),
        [("mixer.set_output".into(), control_args(&args))]
    );
}

// `mixer.set_colour` (Swift `ControlCommands.runStrip`).

#[test]
fn set_colour_sets_a_swatch_or_clears_it() {
    let (f, mut d) = doc();
    let ran = run(
        "mixer.set_colour",
        &[i("strip", 1), t("colour", "teal")],
        &mut d,
    )
    .unwrap();
    assert_eq!(ran.status, None);
    run(
        "mixer.set_colour",
        &[i("strip", 1), t("colour", "none")],
        &mut d,
    )
    .unwrap();
    assert_eq!(
        f.calls(),
        ["set_strip_colour(1,Some(Teal))", "set_strip_colour(1,None)"]
    );
    assert!(MIXER_COMMANDS.contains(&"mixer.set_colour"));
}

#[test]
fn set_colour_checks_its_arguments_first() {
    let (f, mut d) = doc();
    const STRIP: &str = "mixer.set_colour: `strip` is a strip ID (see state.get mixer)";
    const COLOUR: &str = "mixer.set_colour: `colour` is none, red, orange, yellow, green, teal, \
                          blue, purple or pink";
    for (args, message) in [
        (vec![t("colour", "red")], STRIP),
        (vec![t("strip", "1"), t("colour", "red")], STRIP),
        (vec![i("strip", -1), t("colour", "red")], STRIP),
        (vec![i("strip", 1)], COLOUR),
        (vec![i("strip", 1), t("colour", "Red")], COLOUR),
        (vec![i("strip", 1), t("colour", "cyan")], COLOUR),
        (vec![i("strip", 1), b("colour", true)], COLOUR),
    ] {
        let e = err(run("mixer.set_colour", &args, &mut d));
        assert_eq!(e.code, ErrorCode::InvalidParams, "{args:?}");
        assert_eq!(e.message, message, "{args:?}");
    }
    assert!(f.calls().is_empty());
}

#[test]
fn a_refused_colour_is_failed_in_the_sessions_words_or_busy() {
    let (f, mut d) = doc();
    let args = [i("strip", 9), t("colour", "red")];
    f.set_call_error(
        "set_strip_colour",
        Some(model_err("Strip 9 is not in this song.")),
    );
    let e = err(run("mixer.set_colour", &args, &mut d));
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "Strip 9 is not in this song.");
    f.set_call_error(
        "set_strip_colour",
        Some(busy("Release the touched control first.")),
    );
    let e = err(run("mixer.set_colour", &args, &mut d));
    assert_eq!(e.code, ErrorCode::Busy);
    assert!(d.alert.is_none());
}

/// Make Mine is Co-Producer's: not handled in this slice.
#[test]
fn make_mine_is_not_handled_yet() {
    let (f, mut d) = doc();
    assert!(!MIXER_COMMANDS.contains(&"mixer.make_mine"));
    let e = err(run("mixer.make_mine", &[i("strip", 1)], &mut d));
    assert_eq!(e.code, ErrorCode::UnknownId);
    assert!(f.calls().is_empty());
}

// `automation.mode.*` (Swift `AutomationCommands.runMode`).

const MODES: [(&str, AutomationModeRow); 6] = [
    ("automation.mode.off", AutomationModeRow::Off),
    ("automation.mode.read", AutomationModeRow::Read),
    ("automation.mode.touch", AutomationModeRow::Touch),
    ("automation.mode.latch", AutomationModeRow::Latch),
    ("automation.mode.write", AutomationModeRow::Write),
    ("automation.mode.trim", AutomationModeRow::Trim),
];

fn select(d: &mut DocumentModel, strips: &[u64]) {
    d.set_selected_strips(strips.iter().copied().collect());
}

#[test]
fn mode_commands_set_the_selected_strips() {
    for (id, mode) in MODES {
        let (f, mut d) = doc();
        assert!(MIXER_COMMANDS.contains(&id), "{id}");
        assert!(!commands::enabled(id, &d), "{id}: dimmed with no selection");
        let e = err(perform_from_menu(id, &mut d));
        assert_eq!(e.code, ErrorCode::Unavailable);
        assert_eq!(e.message, "Select strips in the mixer first.");
        assert_eq!(d.status(), Some("Select strips in the mixer first."));
        assert_eq!(code(run(id, &[], &mut d)), Some(ErrorCode::Unavailable));
        select(&mut d, &[2, 0]);
        assert!(commands::enabled(id, &d), "{id}");
        perform_from_menu(id, &mut d).unwrap();
        assert_eq!(f.mode_calls().last(), Some(&(vec![0, 2], mode)), "{id}");
        run(id, &[], &mut d).unwrap();
        assert_eq!(f.mode_calls().len(), 2, "{id}");
    }
}

fn code(r: Result<Ran, ControlError>) -> Option<ErrorCode> {
    r.err().map(|e| e.code)
}

#[test]
fn the_channel_runs_mode_commands_and_ignores_stray_arguments() {
    let (f, mut d) = doc();
    select(&mut d, &[2, 1]);
    let hostile = [
        t("strips", "all"),
        t("mode", "off"),
        b("strips", true),
        b("mode", false),
    ];
    run("automation.mode.trim", &hostile, &mut d).unwrap();
    assert_eq!(
        f.mode_calls(),
        [(vec![1, 2], AutomationModeRow::Trim)],
        "the selection and the ID decide"
    );
}

#[test]
fn a_mode_command_with_a_drag_open_is_unavailable_like_set_mode() {
    let (f, mut d) = doc();
    select(&mut d, &[1]);
    assert!(d.begin_gesture("Move", vec![]));
    let menu = err(perform_from_menu("automation.mode.read", &mut d));
    let channel = err(run("automation.mode.read", &[], &mut d));
    let set_mode = err(run(
        "automation.set_mode",
        &[t("strips", "1"), t("mode", "read")],
        &mut d,
    ));
    for e in [&menu, &channel, &set_mode] {
        assert_eq!(e.code, ErrorCode::Unavailable);
        assert_eq!(e.message, "Finish the drag first.");
    }
    assert!(f.mode_calls().is_empty());
}

#[test]
fn mode_commands_skip_strips_that_went_with_the_mixer_closed() {
    let (f, mut d) = doc();
    select(&mut d, &[1, 2]);
    // Undo of Add Bus with no mixer shown: no refetch prunes the selection.
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 2));
    run("automation.mode.write", &[], &mut d).unwrap();
    assert_eq!(f.mode_calls().last().unwrap().0, [1]);
    assert_eq!(d.selected_strips(), &BTreeSet::from([1]), "pruned");
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 1));
    let n = f.mode_calls().len();
    select(&mut d, &[1, 2]);
    let e = err(perform_from_menu("automation.mode.read", &mut d));
    assert_eq!(e.message, "Select strips in the mixer first.");
    assert!(d.selected_strips().is_empty());
    assert_eq!(f.mode_calls().len(), n, "the core is not called");
}

#[test]
fn the_selection_is_pruned_when_the_document_moves() {
    let (f, mut d) = doc();
    select(&mut d, &[1, 2]);
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 2));
    // Any edit moves the document (here: a colour).
    run(
        "mixer.set_colour",
        &[i("strip", 1), t("colour", "red")],
        &mut d,
    )
    .unwrap();
    assert_eq!(d.selected_strips(), &BTreeSet::from([1]));
    f.edit_mixer(|m| m.strips.retain(|s| s.id != 1));
    run(
        "mixer.set_colour",
        &[i("strip", 1), t("colour", "none")],
        &mut d,
    )
    .unwrap();
    assert!(d.selected_strips().is_empty());
    assert!(!commands::enabled("automation.mode.read", &d));
}

#[test]
fn a_busy_or_failed_mode_command_answers_like_set_mode() {
    let (f, mut d) = doc();
    select(&mut d, &[1]);
    f.set_call_error(
        "set_automation_mode",
        Some(busy("Release the touched control first.")),
    );
    let e = err(run("automation.mode.read", &[], &mut d));
    assert_eq!(e.code, ErrorCode::Busy);
    assert_eq!(e.message, "Release the touched control first.");
    f.set_call_error("set_automation_mode", Some(model_err("boom")));
    let channel = err(run("automation.mode.read", &[], &mut d));
    let set_mode = err(run(
        "automation.set_mode",
        &[t("strips", "1"), t("mode", "read")],
        &mut d,
    ));
    assert_eq!(channel.code, ErrorCode::Failed);
    assert_eq!(channel.message, "Automation Mode Failed: boom");
    assert_eq!(set_mode.message, channel.message);
    assert!(d.alert.is_none());
}

// `automation.suspend`: process-wide, so one test owns the switch.

#[test]
fn suspend_toggles_the_global_switch_from_the_channel_and_the_menu() {
    use vizdaw_ffi::automation::{automation_suspended, set_automation_suspended};
    let (_f, mut d) = doc();
    let mut prefs = Prefs::in_memory();
    set_automation_suspended(false);
    assert!(APP_COMMANDS.contains(&"automation.suspend"));
    assert!(commands::enabled("automation.suspend", &d));
    assert!(!commands::is_on("automation.suspend", &d));
    commands::run_app("automation.suspend", &[], &mut prefs).unwrap();
    assert!(automation_suspended(), "absent toggles");
    assert!(commands::is_on("automation.suspend", &d));
    commands::run_app("automation.suspend", &[b("on", true)], &mut prefs).unwrap();
    assert!(automation_suspended());
    let e = commands::run_app("automation.suspend", &[t("on", "yes")], &mut prefs).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(e.message, "automation.suspend: `on` is true or false");
    assert!(automation_suspended(), "a refused call does not toggle");
    commands::run_app("automation.suspend", &[], &mut prefs).unwrap();
    assert!(!automation_suspended());
    // The menu item toggles too.
    perform_from_menu("automation.suspend", &mut d).unwrap();
    assert!(automation_suspended());
    perform_from_menu("automation.suspend", &mut d).unwrap();
    assert!(!automation_suspended());
    assert!(!commands::is_on("automation.mode.read", &d));
    set_automation_suspended(false);
}

// The plug-in verbs (slice 10a Task 7; Swift `MixerCommands.edit`,
// `M/MixerCommands.swift:103-141`).

/// The six plug-in edits, as the channel passes them, and their titles.
fn plugin_edits() -> Vec<(&'static str, Args, &'static str)> {
    vec![
        (
            "mixer.add_insert",
            vec![i("strip", 1), t("plugin", "multi-q")],
            "Add Insert",
        ),
        ("mixer.remove_insert", vec![i("slot", 7)], "Remove Insert"),
        (
            "mixer.move_insert",
            vec![i("slot", 7), i("index", 1)],
            "Move Insert",
        ),
        (
            "mixer.set_bypass",
            vec![i("slot", 7), b("bypass", true)],
            "Bypass Insert",
        ),
        (
            "mixer.set_eq",
            vec![i("strip", 1), t("plugin", "multi-q")],
            "Set EQ",
        ),
        (
            "mixer.set_param",
            vec![i("slot", 7), i("param", 0), t("value", "0.5")],
            "Set Plug-in Parameter",
        ),
    ]
}

#[test]
fn add_insert_over_the_channel_runs_the_core_mixer_edit() {
    let (f, mut d) = doc();
    let args = vec![i("strip", 1), t("plugin", "multi-q"), i("index", 1)];
    assert!(MIXER_COMMANDS.contains(&"mixer.add_insert"));
    assert!(commands::enabled("mixer.add_insert", &d));
    let before = d.snapshot().version;
    let ran = run("mixer.add_insert", &args, &mut d).expect("ran");
    assert_eq!(ran.status.as_deref(), Some("Add Insert"));
    assert_eq!(
        f.run_mixers(),
        [("mixer.add_insert".to_owned(), control_args(&args))],
        "one run_mixer call, the arguments as given"
    );
    assert!(d.snapshot().version > before, "the snapshot is applied");
    assert!(d.alert.is_none());
}

#[test]
fn each_plugin_verb_replies_with_its_registry_title() {
    for (id, args, title) in plugin_edits() {
        let (f, mut d) = doc();
        assert!(MIXER_COMMANDS.contains(&id), "{id}");
        assert_eq!(registry::find(id).unwrap().title, title, "{id}");
        let ran = run(id, &args, &mut d).unwrap_or_else(|e| panic!("{id}: {e:?}"));
        assert_eq!(ran.status.as_deref(), Some(title), "{id}");
        assert_eq!(
            f.run_mixers(),
            [(id.to_owned(), control_args(&args))],
            "{id}"
        );
        // An open gesture is `unavailable`, and nothing reaches the core.
        let (f, mut d) = doc();
        assert!(d.begin_gesture("Move", vec![]));
        let e = err(run(id, &args, &mut d));
        assert_eq!(e.code, ErrorCode::Unavailable, "{id}");
        assert_eq!(e.message, "Finish the drag first.", "{id}");
        assert!(f.run_mixers().is_empty(), "{id}");
    }
}

#[test]
fn an_unknown_plugin_is_the_cores_refusal() {
    const NONE: &str = "No installed plug-in is clap:vizdaw.e2e.none.";
    for (id, args, _) in plugin_edits().into_iter().chain([(
        "track.new_plugin_instrument",
        vec![t("plugin", "clap:vizdaw.e2e.none")],
        "",
    )]) {
        let (f, mut d) = doc();
        f.set_call_error("run_mixer", Some(model_err(NONE)));
        let e = err(run(id, &args, &mut d));
        assert_eq!(e.code, ErrorCode::Failed, "{id}");
        assert_eq!(e.message, NONE, "{id}: the core's words, verbatim");
        f.set_call_error("run_mixer", Some(busy("Finish the edit first.")));
        let e = err(run(id, &args, &mut d));
        assert_eq!(e.code, ErrorCode::Busy, "{id}");
        assert_eq!(e.message, "Finish the edit first.", "{id}");
        assert!(d.alert.is_none(), "{id}: no dialog");
    }
}

/// `track.new_plugin_instrument` without `seq` and `after` gets the
/// selected sequence and track, as New Track does; the new track is
/// selected and named in the status (Swift
/// `aPluginInstrumentTrackGoesBelowTheSelection`,
/// `MixerCommandsTests.swift:164`).
#[test]
fn a_new_plugin_instrument_track_goes_below_the_selection() {
    const ID: &str = "track.new_plugin_instrument";
    let (f, mut d) = doc();
    assert!(MIXER_COMMANDS.contains(&ID));
    let seq = d.selected_seq().expect("a sequence");
    let lane = d.lanes()[0].id;
    d.select_track(lane, false);
    let plugin = t("plugin", "built-in-synth");
    let ran = run(ID, std::slice::from_ref(&plugin), &mut d).expect("ran");
    let count = d.lanes().len();
    assert_eq!(ran.status, Some(format!("Inst {count}")));
    assert_eq!(
        f.run_mixers(),
        [(
            ID.to_owned(),
            control_args(&vec![
                plugin.clone(),
                i("seq", seq as i64),
                i("after", lane as i64)
            ])
        )]
    );
    let made = d
        .lanes()
        .iter()
        .find(|l| l.name == format!("Inst {count}"))
        .expect("the new track")
        .id;
    assert_eq!(d.lanes()[1].id, made, "below the selected track");
    assert_eq!(d.selected_lane(), Some(made), "the new track is selected");

    // A `seq` given is kept, and no track of another sequence is `after`.
    let other_seq = d
        .snapshot()
        .sequences
        .iter()
        .map(|q| q.id)
        .find(|q| *q != seq)
        .expect("another sequence");
    let before = f.run_mixers().len();
    let other = run(ID, &[plugin.clone(), i("seq", other_seq as i64)], &mut d).expect("ran");
    let n = f.lanes_of(other_seq).len();
    assert_eq!(other.status, Some(format!("Inst {n}")));
    assert_eq!(
        f.run_mixers()[before..],
        [(
            ID.to_owned(),
            control_args(&vec![plugin, i("seq", other_seq as i64)])
        )]
    );
    assert_eq!(d.selected_lane(), Some(made), "the selection stays");
}
