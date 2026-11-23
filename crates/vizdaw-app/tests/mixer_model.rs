//! The mixer model: Swift `MixerModelTests` (the parts this slice ports),
//! `StoreMixerTests` and the mixer-on-demand tests.

mod support;

use std::sync::Arc;
use std::time::Instant;

use support::{FakeSession, dest_row, mixer_fake, strip_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::mixer::{MIXER_UNAVAILABLE, MixerModel, MixerPlacement};
use vizdaw_app::store::DocumentStore;
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::AutomationModeRow;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::{StripKindRow, SwatchRow, VcaRow};

fn make_with(f: FakeSession) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(f);
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert!(d.ensure_mixer());
    (f, d)
}

fn make() -> (Arc<FakeSession>, DocumentModel) {
    make_with(mixer_fake())
}

fn mx<R>(d: &mut DocumentModel, f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R) -> R {
    d.with_mixer(f).expect("a mixer")
}

fn names(d: &DocumentModel) -> Vec<String> {
    d.mixer()
        .unwrap()
        .strips()
        .map(|s| s.name.clone())
        .collect()
}

fn model_err(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn busy() -> VizError {
    VizError::Busy {
        message: "editor opening".into(),
    }
}

#[test]
fn master_is_separate_and_outputs_are_named() {
    let f = mixer_fake();
    f.set_destinations(vec![dest_row(7, "Piano")]);
    f.edit_mixer(|m| m.strips[0].output = Some(2));
    let (_f, d) = make_with(f);
    let m = d.mixer().unwrap();
    assert_eq!(names(&d), ["Synth", "Bus 1"]);
    assert_eq!(m.master().unwrap().name, "Master");
    let strips: Vec<_> = m.strips().collect();
    assert_eq!(m.output_name(strips[0]), "Bus 1");
    assert_eq!(m.output_name(strips[1]), "Master");
    assert!(
        m.targets(strips[1]).is_empty(),
        "a bus does not send to itself"
    );
    assert_eq!(m.targets(strips[0]).len(), 1);
    assert_eq!(m.destinations()[0].name, "Piano");
}

#[test]
fn columns_put_midi_strips_before_the_first_bus_and_vcas_last() {
    let f = mixer_fake();
    f.edit_mixer(|m| {
        m.strips = vec![
            strip_row(5, "Audio 1", StripKindRow::AudioPath { path: 1 }),
            strip_row(2, "Bus 1", StripKindRow::Bus),
            strip_row(6, "Audio 2", StripKindRow::AudioPath { path: 2 }),
            strip_row(0, "Master", StripKindRow::Master),
        ];
        m.vcas = vec![VcaRow {
            id: 1,
            name: "VCA 1".into(),
            colour: SwatchRow::Blue,
            members: vec![5],
            gain_db: 0.0,
            mute: false,
            solo: false,
            automation: AutomationModeRow::Read,
        }];
    });
    let mut ext = dest_row(7, "Ext Keys");
    ext.external = true;
    f.set_destinations(vec![dest_row(4, "Built-in Synth"), ext]);
    let (f, mut d) = make_with(f);
    let ids: Vec<String> = d
        .mixer()
        .unwrap()
        .columns()
        .iter()
        .map(|c| c.id())
        .collect();
    assert_eq!(ids, ["s5", "d7", "s2", "s6", "v1"]);
    // No bus: the MIDI strips go at the end, before the VCAs.
    f.edit_mixer(|m| m.strips.retain(|s| s.kind != StripKindRow::Bus));
    mx(&mut d, |m, d| m.set_output(d, 5, None));
    let ids: Vec<String> = d
        .mixer()
        .unwrap()
        .columns()
        .iter()
        .map(|c| c.id())
        .collect();
    assert_eq!(ids, ["s5", "s6", "d7", "v1"]);
}

#[test]
fn the_mixer_is_refetched_only_when_the_document_changes() {
    let (f, mut d) = make();
    let before = f.mixer_reads();
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(f.mixer_reads(), before);
    d.rename_track(30, "Kit");
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(f.mixer_reads(), before + 1);
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(f.mixer_reads(), before + 1);
}

#[test]
fn buses_get_free_names_and_edits_reach_the_session() {
    let (f, mut d) = make();
    let now = Instant::now();
    mx(&mut d, |m, d| {
        m.add_bus(d);
        m.toggle_mute(d, 1, now);
        m.set_output(d, 1, Some(2));
        m.set_colour(d, 1, Some(SwatchRow::Teal));
        m.set_colour(d, 1, Some(SwatchRow::Teal));
    });
    assert!(
        f.gestures()
            .iter()
            .all(|g| g.0 == T::StripMute { strip: 1 }),
        "mute is a gesture"
    );
    assert_eq!(
        f.calls(),
        [
            "add_bus(Bus 2)",
            "set_output(1,Some(2))",
            "set_strip_colour(1,Some(Teal))"
        ],
        "a colour the strip has sends nothing"
    );
    assert_eq!(
        names(&d),
        ["Synth", "Bus 1", "Bus 2"],
        "the mixer is read again"
    );
    assert!(d.alert.is_none());
}

#[test]
fn new_bus_uses_the_add_bus_name() {
    // Only buses count: a track strip named "Bus 1" does not.
    let other = mixer_fake();
    other.edit_mixer(|m| {
        m.strips = vec![
            strip_row(1, "Bus 1", StripKindRow::BuiltinSynth),
            strip_row(0, "Master", StripKindRow::Master),
        ];
        m.buses = vec![];
    });
    let (o, mut n) = make_with(other);
    mx(&mut n, |m, d| m.add_bus(d));
    assert_eq!(o.calls(), ["add_bus(Bus 1)"]);

    let (f, mut d) = make();
    mx(&mut d, |m, d| m.new_bus(d, 1));
    assert_eq!(f.calls(), ["new_bus_from(1,Bus 2)"]);
    assert!(d.alert.is_none());
}

#[test]
fn a_refused_new_bus_alerts_once() {
    let (f, mut d) = make();
    f.set_call_error("new_bus_from", Some(model_err("Master has no sends.")));
    let before = f.mixer_reads();
    mx(&mut d, |m, d| m.new_bus(d, 0));
    let alert = d.alert.clone().unwrap();
    assert_eq!(alert.title, "New Bus Failed");
    assert_eq!(alert.message, "Master has no sends.");
    mx(&mut d, |m, d| m.new_bus(d, 0));
    assert_eq!(
        d.alert.as_ref().unwrap().id,
        alert.id,
        "the same alert is not raised twice"
    );
    assert!(f.mixer_reads() > before, "the mixer is refetched");
    assert!(f.calls().is_empty());
}

#[test]
fn renaming_and_errors() {
    let (f, mut d) = make();
    mx(&mut d, |m, d| {
        m.begin_rename(2, MixerPlacement::Window);
        assert_eq!(m.rename_draft, "Bus 1");
        m.rename_draft = "  Verb ".into();
        m.commit_rename(d);
        m.begin_rename(2, MixerPlacement::Window);
        m.commit_rename(d); // unchanged: no command
        m.begin_rename(2, MixerPlacement::Window);
        m.rename_draft = "   ".into();
        m.commit_rename(d); // blank: no command
    });
    assert_eq!(f.calls(), ["rename_strip(2,Verb)"]);
    f.set_call_error("set_output", Some(model_err("routing cycle")));
    mx(&mut d, |m, d| m.set_output(d, 2, Some(1)));
    assert_eq!(d.alert.as_ref().unwrap().title, "Output Failed");
    assert_eq!(d.alert.as_ref().unwrap().message, "routing cycle");
    f.set_call_error("rename_strip", Some(model_err("taken")));
    d.alert = None;
    mx(&mut d, |m, d| {
        m.begin_rename(2, MixerPlacement::View);
        m.rename_draft = "Drums".into();
        m.commit_rename(d);
    });
    assert_eq!(d.alert.as_ref().unwrap().title, "Rename Failed");
}

#[test]
fn a_rename_shows_in_one_placement() {
    let (_f, mut d) = make();
    mx(&mut d, |m, _| m.begin_rename(2, MixerPlacement::View));
    let m = d.mixer().unwrap();
    assert!(m.is_renaming(2, MixerPlacement::View));
    assert!(!m.is_renaming(2, MixerPlacement::Window));
    assert!(!m.is_renaming(1, MixerPlacement::View));
    mx(&mut d, |m, _| m.cancel_rename());
    assert_eq!(d.mixer().unwrap().renaming(), None);
}

#[test]
fn a_busy_session_skips_the_refresh_silently_and_retries() {
    let (f, mut d) = make();
    let rows = names(&d);
    f.edit_mixer(|m| m.strips.insert(2, strip_row(3, "New", StripKindRow::Bus)));
    f.set_call_error("mixer", Some(busy()));
    d.rename_track(30, "Kit");
    mx(&mut d, |m, d| {
        m.refresh_if_needed(d);
        m.poll_levels(d, Instant::now());
    });
    assert!(d.alert.is_none());
    assert_eq!(names(&d), rows, "the previous rows stay");
    f.set_call_error("mixer", None);
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(
        names(&d),
        [rows[0].as_str(), rows[1].as_str(), "New"],
        "retried on the next tick"
    );
    assert!(d.alert.is_none());
}

#[test]
fn a_busy_refetch_is_owed_even_without_a_version_change() {
    let (f, mut d) = make();
    f.set_call_error("mixer", Some(busy()));
    f.set_call_error("set_output", Some(model_err("no")));
    let version = d.snapshot().version;
    mx(&mut d, |m, d| m.set_output(d, 1, Some(2)));
    assert_eq!(
        d.snapshot().version,
        version,
        "a refused edit moves nothing"
    );
    f.set_call_error("mixer", None);
    f.edit_mixer(|m| m.strips[0].name = "Lead".into());
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(names(&d)[0], "Lead");
}

#[test]
fn identical_command_alerts_are_not_re_set() {
    let (f, mut d) = make();
    f.set_call_error("set_output", Some(model_err("no")));
    mx(&mut d, |m, d| m.set_output(d, 1, Some(2)));
    let first = d.alert.clone().unwrap();
    mx(&mut d, |m, d| m.set_output(d, 1, None));
    assert_eq!(d.alert.as_ref().unwrap().id, first.id);
}

#[test]
fn a_failing_refetch_is_mixer_unavailable() {
    let (f, mut d) = make();
    f.set_call_error("mixer", Some(model_err("gone")));
    d.rename_track(30, "Kit");
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(d.alert.as_ref().unwrap().title, MIXER_UNAVAILABLE);
    let reads = f.mixer_reads();
    mx(&mut d, |m, d| m.refresh_if_needed(d));
    assert_eq!(
        f.mixer_reads(),
        reads,
        "asked again only when the document moves"
    );
}

#[test]
fn one_mixer_per_document_gone_with_it() {
    let mut store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(mixer_fake()))),
        Box::new(|| Ok(Arc::new(mixer_fake()))),
        Box::new(|_| Ok(Arc::new(mixer_fake()))),
    );
    let id = store.new_empty().unwrap();
    let a = store.mixer(id).unwrap() as *const MixerModel;
    let b = store.mixer(id).unwrap() as *const MixerModel;
    assert_eq!(a, b, "one mixer");
    let c = store.document(id).unwrap().mixer().unwrap() as *const MixerModel;
    assert_eq!(a, c, "the document holds it");
    store.close(id);
    assert!(store.mixer(id).is_none());
    assert!(store.mixer(vizdaw_app::DocId(99)).is_none());
}

#[test]
fn a_busy_first_ask_is_retried_quietly_until_the_mixer_arrives() {
    let f = Arc::new(mixer_fake());
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert!(d.mixer().is_none(), "nothing asked for the mixer");
    f.set_call_error("mixer", Some(busy()));
    assert!(!d.ensure_mixer());
    assert!(!d.ensure_mixer());
    assert!(d.alert.is_none(), "Busy is quiet");
    assert_eq!(f.mixer_reads(), 2, "asked again next time");
    f.set_call_error("mixer", None);
    assert!(d.ensure_mixer());
    assert!(d.with_mixer(|m, _| m.fetches()).is_some());
}

#[test]
fn a_failed_mixer_is_asked_for_once_per_version() {
    let f = Arc::new(mixer_fake());
    let mut d = DocumentModel::new(f.clone()).unwrap();
    f.set_call_error("mixer", Some(model_err("broken")));
    assert!(!d.ensure_mixer());
    assert!(d.with_mixer(|_, _| ()).is_none());
    assert_eq!(f.mixer_reads(), 1, "drawing does not retry it");
    assert_eq!(d.alert.as_ref().unwrap().title, MIXER_UNAVAILABLE);
    d.alert = None;
    d.rename_track(30, "Kit");
    assert!(!d.ensure_mixer());
    assert_eq!(f.mixer_reads(), 2, "asked again once the document moved");
    assert!(d.alert.is_none(), "the retry does not alert again");
    f.set_call_error("mixer", None);
    d.rename_track(30, "Kick");
    assert!(d.ensure_mixer());
}

/// Audio 1 (5) plays path 1, which Groove (30) and Chords (31) play
/// through; Chords is armed. Synth (1) has no input track.
fn armed_fake() -> FakeSession {
    use vizdaw_ffi::edit::MonitorModeRow;
    let f = mixer_fake();
    let mut groove = support::lane_row(30, "Groove", false);
    groove.audio_out = Some(1);
    let mut chords = support::lane_row(31, "Chords", false);
    chords.audio_out = Some(1);
    chords.arm = true;
    chords.monitor = MonitorModeRow::Input;
    f.set_lanes(3, vec![groove, chords]);
    f.edit_mixer(|m| {
        m.strips.insert(
            1,
            strip_row(5, "Audio 1", StripKindRow::AudioPath { path: 1 }),
        )
    });
    f
}

#[test]
fn the_input_track_is_the_first_armed_one_through_the_path() {
    let (_f, d) = make_with(armed_fake());
    let m = d.mixer().unwrap();
    let audio = m.strip(5).unwrap();
    assert_eq!(m.input_lane(&d, audio).map(|l| l.id), Some(31));
    assert!(m.can_arm(&d, audio));
    let synth = m.strip(1).unwrap();
    assert!(m.input_lane(&d, synth).is_none());
    assert!(!m.can_arm(&d, synth), "R and I are for audio tracks");
    assert!(!m.can_arm(&d, m.master().unwrap()));
}

#[test]
fn record_arm_toggles_the_input_track_as_one_edit() {
    use vizdaw_ffi::edit::{EditCommand, LaneEdit};
    let (f, mut d) = make_with(armed_fake());
    mx(&mut d, |m, d| m.toggle_arm(d, 5));
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: 3,
                lane: 31,
                mute: None,
                solo: None,
                arm: Some(false),
            }
        })
    );
    let n = f.edits().len();
    mx(&mut d, |m, d| m.toggle_arm(d, 1));
    assert_eq!(
        f.edits().len(),
        n,
        "a strip with no input track sends nothing"
    );
}

#[test]
fn the_monitor_cycles_auto_input_off() {
    use vizdaw_app::mixer::{monitor_help, next_monitor};
    use vizdaw_ffi::edit::{EditCommand, LaneEdit, MonitorModeRow as M};
    assert_eq!(next_monitor(M::Auto), M::Input);
    assert_eq!(next_monitor(M::Input), M::Off);
    assert_eq!(next_monitor(M::Off), M::Auto);
    assert_eq!(monitor_help(M::Off), "Input monitoring: off");
    assert_eq!(monitor_help(M::Input), "Input monitoring: always the input");
    let (f, mut d) = make_with(armed_fake());
    mx(&mut d, |m, d| m.cycle_monitor(d, 5));
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetMonitor {
                seq: 3,
                lane: 31,
                monitor: M::Off,
            }
        })
    );
    let n = f.edits().len();
    mx(&mut d, |m, d| m.cycle_monitor(d, 0));
    assert_eq!(f.edits().len(), n, "master has no input track");
}
