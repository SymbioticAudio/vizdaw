//! Co-Producer's starter projects (plan 8e Task 4; Ruling 5): one acting
//! step in a new song sets the tempo, meter and markers, and makes the
//! buses, tracks, effects, outputs and sends, from what is installed or
//! the built-ins in its place. It writes no notes, regions or automation,
//! it needs a new song, and every spec is hostile.
//!
//! Every test that builds runs in a child with a temp home: its
//! instruments and inserts load the test plug-ins. Offline only.

use core_host::bundled::MULTIQ_CLAP_ID;
use core_host::catalog::Catalog;
use core_model::document::{DestKind, Document};
use core_model::ids::{LaneId, SeqId, StripId};
use core_model::marks::lane_strip;
use core_model::meter::Meter;
use core_model::mixer::{Output, PluginFormat, PluginRef, Strip, StripKind};
use core_model::tempo::Bpm;
use core_model::time::Tick;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::bundled::{debug_clear_multiq, debug_set_multiq};
use vizdaw_ffi::control::{ControlResult, result_json};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::starter::{CoProducerStarterRow, StarterStripRow};
use vizdaw_ffi::{Session, VizError, set_default_eq, set_new_channel_eq};

static SERIAL: Mutex<()> = Mutex::new(());

const SYNTH: &str = "clap:vizdaw.test.synth4";
const GAIN: &str = "clap:vizdaw.test.gain";
const NONE: &str = "clap:vizdaw.test.none";
const STAMP: &str = "2026-11-17 10-00";
const BAR: i64 = 61_440;
const SHARED: &str = "Tracks on the Built-in Synth share its one sound for now.";

/// One test at a time; offline; a catalog holding only the test plug-ins;
/// the fake Multi-Q; "Put an EQ on new channels" on, so buses get the
/// default EQ. Test builds keep the support folder in a per-process
/// temporary folder (`recovery::support_dir`).
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
    vizdaw_ffi::catalog::with(|c| *c = Catalog::new());
    vizdaw_ffi::catalog::add_test_bundle(&test_plugins::bundle()).unwrap();
    set_new_channel_eq(true);
    set_default_eq(None);
    debug_clear_multiq();
    debug_set_multiq(Some(multi_q()));
    g
}

fn multi_q() -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: MULTIQ_CLAP_ID.into(),
        name: "Multi-Q".into(),
        vendor: String::new(),
        version: "0.10.9".into(),
    }
}

/// Runs `test` again alone in a child of this test binary whose `HOME`
/// and `CFFIXED_USER_HOME` are a temp folder; answers whether the caller
/// is that child (and so runs the body).
fn in_temp_home(test: &str) -> bool {
    const MARK: &str = "VIZDAW_TEST_TEMP_HOME";
    if let Some(home) = std::env::var_os(MARK) {
        assert_eq!(std::env::var_os("HOME"), Some(home.clone()), "HOME");
        assert_eq!(
            std::env::var_os("CFFIXED_USER_HOME"),
            Some(home),
            "CFFIXED_USER_HOME"
        );
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            test,
            "--exact",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HOME", home.path())
        .env("CFFIXED_USER_HOME", home.path())
        .env(MARK, home.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    print!("{stdout}");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{test} failed in its temp home");
    assert!(
        stdout.contains("test result: ok. 1 passed"),
        "the child did not run {test} exactly once"
    );
    false
}

fn enable(id: &str, on: bool) {
    vizdaw_ffi::catalog::with(|c| c.set_enabled(PluginFormat::Clap, id, on));
}

/// Plan 8e Task 1's synthwave example, every instrument the test synth.
fn synthwave() -> Value {
    json!({
        "title": "Synthwave",
        "tempo": "118",
        "meter": "4/4",
        "markers": [
            {"bar": 1, "name": "Intro"},
            {"bar": 9, "name": "Verse"},
            {"bar": 17, "name": "Chorus"},
            {"bar": 33, "name": "Bridge"},
            {"bar": 41, "name": "Outro"}
        ],
        "buses": [
            {"name": "Reverb", "effects": [GAIN]},
            {"name": "Delay", "output": "Reverb"}
        ],
        "tracks": [
            {"name": "Drums", "kind": "instrument", "plugin": SYNTH},
            {"name": "Bass", "kind": "instrument", "plugin": SYNTH},
            {"name": "Pad", "kind": "instrument", "plugin": SYNTH,
             "sends": [{"bus": "Reverb", "level_db": "-12"}]},
            {"name": "Lead", "kind": "instrument", "plugin": SYNTH},
            {"name": "Arp", "kind": "instrument", "plugin": SYNTH}
        ]
    })
}

/// `spec` with track `i`'s `key` set to `value`.
fn with_track(mut spec: Value, i: usize, key: &str, value: Value) -> Value {
    spec["tracks"][i][key] = value;
    spec
}

fn doc(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
}

/// The document without plug-in state blobs, which a plug-in that has
/// just started running stores on its own, and without the project ID,
/// which a request's first saved copy stamps (plan 6a).
fn shape(s: &Session) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.remove("state");
                m.remove("project_id");
                m.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v: Value = serde_json::from_str(&s.debug_document_json()).unwrap();
    strip(&mut v);
    v
}

/// A new song as File › New makes it.
fn new_song() -> Arc<Session> {
    Session::new_empty().unwrap()
}

/// `co_producer_starter` on `s` with `spec`'s text, inside the request
/// "start synthwave", as the app's call makes it.
fn start_text(s: &Session, spec: &str) -> Result<CoProducerStarterRow, VizError> {
    s.co_producer_begin("start synthwave".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    let r = s.co_producer_starter(spec.into());
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    r
}

fn start(s: &Session, spec: &Value) -> Result<CoProducerStarterRow, VizError> {
    start_text(s, &spec.to_string())
}

fn message<T: std::fmt::Debug>(r: Result<T, VizError>) -> String {
    match r {
        Err(e) => e.to_string(),
        Ok(v) => panic!("expected a refusal, got {v:?}"),
    }
}

fn strip_named<'a>(d: &'a Document, name: &str) -> &'a Strip {
    d.mixer()
        .strips()
        .values()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no strip {name}"))
}

fn the_seq(d: &Document) -> SeqId {
    let seqs: Vec<SeqId> = d.sequences().keys().copied().collect();
    assert_eq!(seqs.len(), 1, "{seqs:?}");
    seqs[0]
}

fn lane_named(d: &Document, name: &str) -> LaneId {
    d.sequence(the_seq(d))
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no track {name}"))
        .id
}

fn strip_of_track(d: &Document, name: &str) -> StripId {
    lane_strip(d, lane_named(d, name)).unwrap()
}

/// The plug-in IDs of `strip`'s inserts, in order.
fn insert_ids(d: &Document, strip: StripId) -> Vec<String> {
    d.mixer()
        .strip(strip)
        .unwrap()
        .inserts
        .iter()
        .map(|p| p.plugin.id.clone())
        .collect()
}

/// Whether track `name` plays the Built-in Synth.
fn plays_builtin(d: &Document, name: &str) -> bool {
    let seq = the_seq(d);
    let l = d.lane(seq, lane_named(d, name)).unwrap();
    let dest = d.instruments().get(&l.midi_dest.unwrap()).unwrap();
    dest.kind.is_builtin_synth()
}

fn catalog_name(id: &str) -> String {
    vizdaw_ffi::catalog::with(|c| {
        c.preferred(PluginFormat::Clap, id)
            .unwrap()
            .plugin
            .name
            .clone()
    })
}

#[test]
fn a_starter_sets_up_sounds_and_routing() {
    if !in_temp_home("a_starter_sets_up_sounds_and_routing") {
        return;
    }
    let _g = serial();
    let s = new_song();
    let fresh = shape(&s);
    let row = start(&s, &synthwave()).unwrap();
    assert!(row.lines.is_empty(), "{:?}", row.lines);
    let d = doc(&s);
    let seq = d.sequence(the_seq(&d)).unwrap();

    // Tempo, meter and markers.
    let tempo: Vec<_> = seq.tempo.points().iter().map(|p| (p.tick, p.bpm)).collect();
    assert_eq!(tempo, vec![(Tick::ZERO, Bpm(118_000))]);
    let meter: Vec<Meter> = seq.meter.entries().iter().map(|e| e.meter).collect();
    assert_eq!(meter, vec![Meter { num: 4, den: 4 }]);
    let markers: Vec<(i64, &str)> = seq
        .markers
        .iter()
        .map(|m| (m.tick.0, m.name.as_str()))
        .collect();
    assert_eq!(
        markers,
        vec![
            (0, "Intro"),
            (8 * BAR, "Verse"),
            (16 * BAR, "Chorus"),
            (32 * BAR, "Bridge"),
            (40 * BAR, "Outro"),
        ]
    );

    // The buses: Delay goes to Reverb, Reverb has the test gain, and both
    // have the default EQ.
    let reverb = strip_named(&d, "Reverb (Co-Producer)");
    let delay = strip_named(&d, "Delay (Co-Producer)");
    assert_eq!(reverb.kind, StripKind::Bus);
    assert_eq!(delay.kind, StripKind::Bus);
    assert_eq!(delay.output, Output::Bus(reverb.id));
    assert_eq!(reverb.output, Output::Master);
    assert_eq!(insert_ids(&d, reverb.id), vec!["vizdaw.test.gain"]);
    assert!(insert_ids(&d, delay.id).is_empty());
    for b in [reverb, delay] {
        assert_eq!(
            b.eq.as_ref().map(|e| e.plugin.id.as_str()),
            Some(MULTIQ_CLAP_ID)
        );
    }

    // Five instrument tracks, each playing its own test synth.
    let names: Vec<&str> = seq.lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        ["Drums", "Bass", "Pad", "Lead", "Arp"].map(|n| format!("{n} (Co-Producer)"))
    );
    let mut dests = Vec::new();
    for l in &seq.lanes {
        let dest = l.midi_dest.unwrap();
        match &d.instruments().get(&dest).unwrap().kind {
            DestKind::PluginInstrument { slot } => {
                assert_eq!(slot.plugin.id, "vizdaw.test.synth4");
                assert!(slot.co_producer, "{}", l.name);
            }
            k => panic!("{} plays {k:?}", l.name),
        }
        assert!(!dests.contains(&dest));
        dests.push(dest);
        // Its strip is its own, named after it, to master.
        let st = d.mixer().strip(lane_strip(&d, l.id).unwrap()).unwrap();
        assert_eq!(st.kind, StripKind::Instrument(dest));
        assert_eq!(st.name, l.name);
        assert_eq!(st.output, Output::Master);
    }

    // Pad sends to Reverb at −12 dB, post-fader; no other track sends.
    let pad = d
        .mixer()
        .strip(strip_of_track(&d, "Pad (Co-Producer)"))
        .unwrap();
    assert_eq!(pad.sends.len(), 1);
    let send = pad.sends[0];
    assert_eq!(send.target, reverb.id);
    assert_eq!(send.level_db, -12.0);
    assert!(!send.pre_fader);
    assert!(send.enabled);
    for n in ["Drums", "Bass", "Lead", "Arp"] {
        let st = d
            .mixer()
            .strip(strip_of_track(&d, &format!("{n} (Co-Producer)")))
            .unwrap();
        assert!(st.sends.is_empty(), "{n}");
    }

    // Everything is Co-Producer's.
    assert!(seq.lanes.iter().all(|l| l.co_producer));
    for st in d.mixer().strips().values() {
        if st.kind == StripKind::Master {
            continue;
        }
        assert!(st.co_producer, "{}", st.name);
        assert!(st.name.ends_with(" (Co-Producer)"), "{}", st.name);
        assert!(st.inserts.iter().all(|p| p.co_producer), "{}", st.name);
        assert!(st.eq.iter().all(|p| p.co_producer), "{}", st.name);
        assert!(st.sends.iter().all(|x| x.co_producer), "{}", st.name);
    }

    // One undo gives File › New's song back.
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Co-Producer: Start Synthwave")
    );
    s.undo().unwrap();
    assert_eq!(shape(&s), fresh);
}

#[test]
fn starters_write_no_notes() {
    if !in_temp_home("starters_write_no_notes") {
        return;
    }
    let _g = serial();
    let s = new_song();
    // An audio track as well, with effects, an output and a send.
    let mut spec = synthwave();
    spec["tracks"].as_array_mut().unwrap().push(json!({
        "name": "Vox", "kind": "audio", "effects": [GAIN], "output": "Delay",
        "sends": [{"bus": "Reverb"}]
    }));
    start(&s, &spec).unwrap();
    let d = doc(&s);
    let mut tracks = 0;
    for seq in d.sequences().values() {
        for l in &seq.lanes {
            tracks += 1;
            for t in &l.takes {
                assert!(t.events.is_empty(), "{} has events", l.name);
            }
        }
    }
    // Six tracks and no automation track.
    assert_eq!(tracks, 6);
    let vox = d
        .mixer()
        .strip(strip_of_track(&d, "Vox (Co-Producer)"))
        .unwrap();
    assert!(matches!(vox.kind, StripKind::AudioPath(_)));
    assert_eq!(vox.name, "Vox (Co-Producer)");
    assert_eq!(insert_ids(&d, vox.id), vec!["vizdaw.test.gain"]);
    assert_eq!(
        vox.output,
        Output::Bus(strip_named(&d, "Delay (Co-Producer)").id)
    );
    assert_eq!(vox.sends.len(), 1);
    assert_eq!(vox.sends[0].level_db, 0.0);
}

#[test]
fn starters_fall_back_to_built_ins() {
    if !in_temp_home("starters_fall_back_to_built_ins") {
        return;
    }
    let _g = serial();

    // An instrument that is not installed: the Built-in Synth.
    let s = new_song();
    let row = start(&s, &with_track(synthwave(), 1, "plugin", json!(NONE))).unwrap();
    assert_eq!(
        row.lines,
        vec![
            "Bass: No installed plug-in is clap:vizdaw.test.none.; it plays the Built-in Synth instead."
                .to_string(),
            SHARED.to_string(),
        ]
    );
    let d = doc(&s);
    assert!(plays_builtin(&d, "Bass (Co-Producer)"));
    assert!(!plays_builtin(&d, "Drums (Co-Producer)"));
    assert_eq!(row.tracks[1].plays, "Built-in Synth");

    // An effect turned off in the Plugin Manager: Multi-Q in its place,
    // on a song whose new strips get no EQ.
    set_new_channel_eq(false);
    let s = new_song();
    enable("vizdaw.test.gain", false);
    let row = start(&s, &synthwave());
    enable("vizdaw.test.gain", true);
    let row = row.unwrap();
    assert_eq!(
        row.lines,
        vec!["Reverb: Test Gain is turned off in the Plugin Manager.; Multi-Q is in its place."]
    );
    let d = doc(&s);
    let reverb = strip_named(&d, "Reverb (Co-Producer)");
    assert_eq!(insert_ids(&d, reverb.id), vec![MULTIQ_CLAP_ID]);
    assert_eq!(row.buses[0].effects, vec!["Multi-Q"]);
    assert!(reverb.eq.is_none());

    // With Multi-Q as the default EQ, the bus's EQ stands in: no second
    // Multi-Q, and the line says so.
    set_new_channel_eq(true);
    let s = new_song();
    enable("vizdaw.test.gain", false);
    let row = start(&s, &synthwave());
    enable("vizdaw.test.gain", true);
    let row = row.unwrap();
    assert_eq!(
        row.lines,
        vec![
            "Reverb: Test Gain is turned off in the Plugin Manager.; its Multi-Q EQ is in its place."
        ]
    );
    let d = doc(&s);
    let reverb = strip_named(&d, "Reverb (Co-Producer)");
    assert!(insert_ids(&d, reverb.id).is_empty());
    assert_eq!(
        reverb.eq.as_ref().map(|e| e.plugin.id.as_str()),
        Some(MULTIQ_CLAP_ID)
    );
    assert!(row.buses[0].effects.is_empty());
    set_new_channel_eq(false);

    // Two unusable effects on one strip: one Multi-Q, one left out.
    let s = new_song();
    let spec = with_track(
        synthwave(),
        3,
        "effects",
        json!([NONE, "clap:vizdaw.test.none2"]),
    );
    let row = start(&s, &spec).unwrap();
    assert_eq!(
        row.lines,
        vec![
            "Lead: No installed plug-in is clap:vizdaw.test.none.; Multi-Q is in its place.",
            "Lead: No installed plug-in is clap:vizdaw.test.none2.; left out.",
        ]
    );
    let d = doc(&s);
    assert_eq!(
        insert_ids(&d, strip_of_track(&d, "Lead (Co-Producer)")),
        vec![MULTIQ_CLAP_ID]
    );

    // With no Multi-Q in the build, the unusable effect is left out.
    let s = new_song();
    debug_set_multiq(None);
    let row = start(&s, &with_track(synthwave(), 3, "effects", json!([NONE])));
    debug_set_multiq(Some(multi_q()));
    let row = row.unwrap();
    assert_eq!(
        row.lines,
        vec!["Lead: No installed plug-in is clap:vizdaw.test.none.; left out."]
    );
    let d = doc(&s);
    assert!(insert_ids(&d, strip_of_track(&d, "Lead (Co-Producer)")).is_empty());

    // An effect given as an instrument: the Built-in Synth.
    let s = new_song();
    let row = start(&s, &with_track(synthwave(), 3, "plugin", json!(GAIN))).unwrap();
    assert_eq!(
        row.lines,
        vec![
            "Lead: Test Gain is an effect, not an instrument.; it plays the Built-in Synth instead."
                .to_string(),
            SHARED.to_string(),
        ]
    );
    assert!(plays_builtin(&doc(&s), "Lead (Co-Producer)"));

    // Two tracks on the Built-in Synth share its one strip: the first
    // sets it up, and the second's effects and sends are left out, with
    // a line, rather than stacked on the first's.
    let s = new_song();
    let spec = json!({
        "title": "Shared",
        "tempo": "120",
        "meter": "4/4",
        "buses": [{"name": "Room"}],
        "tracks": [
            {"name": "Keys", "kind": "instrument", "effects": [GAIN]},
            {"name": "Organ", "kind": "instrument", "effects": [GAIN],
             "sends": [{"bus": "Room"}]}
        ]
    });
    let row = start(&s, &spec).unwrap();
    assert_eq!(
        row.lines,
        vec![
            "Organ: shares its strip with Keys; its effects, output and sends are left out."
                .to_string(),
            SHARED.to_string(),
        ]
    );
    let d = doc(&s);
    let keys = strip_of_track(&d, "Keys (Co-Producer)");
    assert_eq!(keys, strip_of_track(&d, "Organ (Co-Producer)"));
    assert_eq!(insert_ids(&d, keys), vec!["vizdaw.test.gain"]);
    assert!(d.mixer().strip(keys).unwrap().sends.is_empty());
}

/// Owner, 2026-11-17 (plan 8e Task 6c): each audio track has its own
/// strip, so two audio tracks each keep their own effects, output and
/// sends, with no "shares its strip" line.
#[test]
fn a_starter_with_two_audio_tracks_gives_two_strips() {
    if !in_temp_home("a_starter_with_two_audio_tracks_gives_two_strips") {
        return;
    }
    let _g = serial();
    set_new_channel_eq(false);
    let s = new_song();
    let spec = json!({
        "title": "Duo",
        "tempo": "120",
        "meter": "4/4",
        "buses": [{"name": "Room"}],
        "tracks": [
            {"name": "Vox", "kind": "audio", "effects": [GAIN]},
            {"name": "Gtr", "kind": "audio", "effects": [GAIN, GAIN],
             "output": "Room"}
        ]
    });
    let row = start(&s, &spec).unwrap();
    assert!(row.lines.is_empty(), "{:?}", row.lines);
    let d = doc(&s);
    let vox = strip_of_track(&d, "Vox (Co-Producer)");
    let gtr = strip_of_track(&d, "Gtr (Co-Producer)");
    assert_ne!(vox, gtr, "each audio track has its own strip");
    let (v, g) = (d.mixer().strip(vox).unwrap(), d.mixer().strip(gtr).unwrap());
    assert!(matches!(v.kind, StripKind::AudioPath(_)));
    assert!(matches!(g.kind, StripKind::AudioPath(_)));
    assert_eq!(v.name, "Vox (Co-Producer)");
    assert_eq!(g.name, "Gtr (Co-Producer)");
    assert_eq!(insert_ids(&d, vox), vec!["vizdaw.test.gain"]);
    assert_eq!(
        insert_ids(&d, gtr),
        vec!["vizdaw.test.gain", "vizdaw.test.gain"]
    );
    assert_eq!(v.output, Output::Master);
    assert_eq!(
        g.output,
        Output::Bus(strip_named(&d, "Room (Co-Producer)").id)
    );
}

#[test]
fn starters_never_fail_over_a_plugin() {
    if !in_temp_home("starters_never_fail_over_a_plugin") {
        return;
    }
    let _g = serial();
    set_new_channel_eq(false);
    let s = new_song();
    let spec = json!({
        "title": "Nothing Fits",
        "tempo": "90",
        "meter": "3/4",
        "buses": [{"name": "Echo", "effects": [NONE, NONE]}],
        "tracks": [
            {"name": "Keys", "kind": "instrument", "plugin": NONE, "effects": [NONE]},
            {"name": "Vox", "kind": "audio", "effects": [NONE]}
        ]
    });
    let row = start(&s, &spec).unwrap();
    let why = "No installed plug-in is clap:vizdaw.test.none.";
    assert_eq!(
        row.lines,
        vec![
            format!("Echo: {why}; Multi-Q is in its place."),
            format!("Echo: {why}; left out."),
            format!("Keys: {why}; it plays the Built-in Synth instead."),
            format!("Keys: {why}; Multi-Q is in its place."),
            format!("Vox: {why}; Multi-Q is in its place."),
            SHARED.to_string(),
        ]
    );
    let d = doc(&s);
    assert!(plays_builtin(&d, "Keys (Co-Producer)"));
    let seq = d.sequence(the_seq(&d)).unwrap();
    assert_eq!(seq.meter.entries()[0].meter, Meter { num: 3, den: 4 });
    assert_eq!(seq.tempo.points()[0].bpm, Bpm(90_000));
    assert!(seq.markers.is_empty());
}

#[test]
fn a_starter_answers_what_it_made() {
    if !in_temp_home("a_starter_answers_what_it_made") {
        return;
    }
    let _g = serial();
    let s = new_song();
    let mut spec = synthwave();
    spec["tracks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "Vox", "kind": "audio"}));
    let row = start(&s, &spec).unwrap();
    assert_eq!(row.title, "Synthwave");
    assert_eq!(row.undo_label, "Co-Producer: Start Synthwave");
    assert_eq!(
        row.tracks[0],
        StarterStripRow {
            name: "Drums (Co-Producer)".into(),
            plays: catalog_name("vizdaw.test.synth4"),
            effects: Vec::new(),
        }
    );
    assert_eq!(row.tracks.len(), 6);
    assert_eq!(row.tracks[5].name, "Vox (Co-Producer)");
    assert_eq!(row.tracks[5].plays, "audio");
    let buses: Vec<(&str, Vec<String>)> = row
        .buses
        .iter()
        .map(|b| (b.name.as_str(), b.effects.clone()))
        .collect();
    assert_eq!(
        buses,
        vec![
            (
                "Reverb (Co-Producer)",
                vec![catalog_name("vizdaw.test.gain")]
            ),
            ("Delay (Co-Producer)", Vec::new()),
        ]
    );
}

#[test]
fn a_starter_needs_a_new_song() {
    if !in_temp_home("a_starter_needs_a_new_song") {
        return;
    }
    let _g = serial();
    let s = new_song();
    let seq = s.snapshot().unwrap().sequences[0].id;
    s.new_track(seq, vizdaw_ffi::TrackKindRow::Instrument, None)
        .unwrap();
    let before = shape(&s);
    let label = s.snapshot().unwrap().undo_label;
    assert_eq!(
        message(start(&s, &synthwave())),
        "A starter project needs a new song."
    );
    assert_eq!(shape(&s), before);
    assert_eq!(s.snapshot().unwrap().undo_label, label);

    // A bus of the composer's is refused too.
    let s = new_song();
    s.add_bus("Room".into()).unwrap();
    let before = shape(&s);
    assert_eq!(
        message(start(&s, &synthwave())),
        "A starter project needs a new song."
    );
    assert_eq!(shape(&s), before);
}

#[test]
fn starter_refuses_hostile_input() {
    let _g = serial();
    let s = new_song();
    let before = shape(&s);
    let object = "The starter project must be an object.";
    let long = "x".repeat(70_000);
    let cases: Vec<(String, String)> = vec![
        ("".into(), object.into()),
        ("not json".into(), object.into()),
        ("{\"title\":".into(), object.into()),
        (long.clone(), object.into()),
        ("\0".into(), object.into()),
        ("[]".into(), object.into()),
        ("null".into(), object.into()),
        (
            json!({"title": "A", "tempo": "120", "meter": "4/4", "tracks": [], "extra": 1})
                .to_string(),
            "Unknown key extra in the starter project.".into(),
        ),
        (
            json!({"title": long, "tempo": "120", "meter": "4/4",
                   "tracks": [{"name": "A", "kind": "instrument"}]})
            .to_string(),
            core_control::starter::parse(&json!({"title": long})).unwrap_err(),
        ),
        (
            json!({"title": "A\u{0}B", "tempo": "120", "meter": "4/4",
                   "tracks": [{"name": "A", "kind": "instrument"}]})
            .to_string(),
            core_control::starter::parse(&json!({"title": "A\u{0}B"})).unwrap_err(),
        ),
        (
            with_track(synthwave(), 0, "kind", json!(7)).to_string(),
            "Track Drums's kind must be instrument or audio.".into(),
        ),
        (
            with_track(synthwave(), 0, "output", json!("Nowhere")).to_string(),
            "No bus Nowhere in the starter project.".into(),
        ),
    ];
    for (text, want) in cases {
        let shown: String = text.chars().take(80).collect();
        assert_eq!(message(start_text(&s, &text)), want, "{shown}");
        assert_eq!(shape(&s), before, "{shown}");
    }
    // Every message is parse's own, for any spec parse refuses.
    let spec = with_track(synthwave(), 0, "plugin", json!("nope"));
    assert_eq!(
        message(start(&s, &spec)),
        core_control::starter::parse(&spec).unwrap_err()
    );
    assert_eq!(shape(&s), before);
    // Outside any request and call, too.
    assert_eq!(
        message(s.co_producer_starter("[1]".into())),
        object.to_string()
    );
    assert_eq!(shape(&s), before);
}

#[test]
fn starter_result_json() {
    let row = CoProducerStarterRow {
        title: "Synthwave".into(),
        undo_label: "Co-Producer: Start Synthwave".into(),
        tracks: vec![
            StarterStripRow {
                name: "Drums (Co-Producer)".into(),
                plays: "Test Synth 4-Out".into(),
                effects: vec!["Test Gain".into()],
            },
            StarterStripRow {
                name: "Vox (Co-Producer)".into(),
                plays: "audio".into(),
                effects: Vec::new(),
            },
        ],
        buses: vec![StarterStripRow {
            name: "Reverb (Co-Producer)".into(),
            plays: String::new(),
            effects: vec!["Multi-Q".into()],
        }],
        lines: vec!["Reverb: x; Multi-Q is in its place.".into()],
    };
    let j = result_json(&ControlResult::CoProducerStarter {
        doc: "D".into(),
        window: 7,
        row,
    });
    assert_eq!(
        j,
        json!({
            "doc": "D",
            "window": 7,
            "title": "Synthwave",
            "undo_label": "Co-Producer: Start Synthwave",
            "tracks": [
                {"name": "Drums (Co-Producer)", "plays": "Test Synth 4-Out"},
                {"name": "Vox (Co-Producer)", "plays": "audio"}
            ],
            "buses": [{"name": "Reverb (Co-Producer)", "effects": ["Multi-Q"]}],
            "lines": ["Reverb: x; Multi-Q is in its place."]
        })
    );
}
