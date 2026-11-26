//! A VST3 state through a DAWproject package (host track H1 Task 5, Plan
//! Ruling 2): the blob a VST3 instance saves is a `.vstpreset`, which the
//! export writes as is to `plugins/NNN.vstpreset` and the import reads
//! back as the new slot's state; a new instance loads it and has both
//! streams back (the component's gain, the controller's own value).

use core_engine::backend::BlockInfo;
use core_engine::fixtures::Builder;
use core_host::vst3::Vst3Instance;
use core_host::{AudioProcessor, ParamOut, PluginInstance, ProcessCtx};
use core_io::dawproject::export::{
    ClipRule, ExportEnv, ExportOptions, LeftOutAudio, RegionGain, export,
};
use core_io::dawproject::import::{ImportEnv, apply, map};
use core_io::dawproject::package::open;
use core_io::dawproject::plugins::NoPlist;
use core_io::limits::PackageLimits;
use core_io::report::Report;
use core_io::zip::{EntryMethod, ZipWriter};
use core_model::command::{Command, LaneCmd, SequenceCmd};
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{AnyId, SeqId};
use core_model::mixer::{ParamId, PluginFormat, PluginRef, StripKind};
use core_model::selection::ViewFilter;
use core_model::time::Tick;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use test_plugins::{VST3_GAIN_ID, VST3_PARAM_GAIN, Vst3Probe};

const GAIN: ParamId = ParamId(VST3_PARAM_GAIN as u64);

/// Runs one quiet block (the first block carries the values set before).
fn one_block(p: &mut dyn AudioProcessor) {
    let chans = |n: usize| -> Vec<Box<[f32]>> { (0..n).map(|_| vec![0.0f32; 64].into()).collect() };
    let (mut input, mut sidechain, mut outputs) = (chans(2), chans(2), chans(2));
    let mut out = ParamOut::default();
    p.process(&mut ProcessCtx {
        frames: 64,
        input: &mut input,
        sidechain: &mut sidechain,
        outputs: &mut outputs,
        events: &[],
        params_in: &[],
        params_out: &mut out,
        transport: &BlockInfo::default(),
    });
}

/// A document like a new project: the master and one sequence with no
/// tracks (what an import goes into).
fn new_project() -> (Document, SeqId) {
    let mut h = History::new(Document::new());
    let r = h
        .execute(
            &Command::Sequence(SequenceCmd::New {
                name: "Sequence 1".into(),
            }),
            &ViewFilter::default(),
        )
        .unwrap();
    let seq = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        })
        .unwrap();
    let lane = h.doc().sequence(seq).unwrap().lanes.front().unwrap().id;
    h.execute(
        &Command::Lane(LaneCmd::Remove { seq, lane }),
        &ViewFilter::default(),
    )
    .unwrap();
    (h.doc().clone(), seq)
}

#[test]
fn a_vst3_state_round_trips_through_a_dawproject_package() {
    let d = tempfile::tempdir().unwrap();
    let copy = d.path().join("Private.vst3");
    test_plugins::copy_vst3_bundle(&copy);
    let probe = Vst3Probe::open(&copy);

    // The plug-in's state: -6 dB in the component (sent with a first
    // block), memo 41 in the controller.
    probe.set_memo(41);
    let mut a = Vst3Instance::load(&copy, VST3_GAIN_ID).unwrap();
    a.set_params(&[(GAIN, -6.0)]).unwrap();
    let mut p = a.activate(48_000.0, 64).unwrap();
    one_block(p.as_mut());
    a.deactivate(p);
    let blob = a.save_state().unwrap();

    // A song with it on the master, exported.
    let mut b = Builder::new();
    let (seq, _lane) = b.sequence("Song", "Keys", None).unwrap();
    let master = b.strip_of(StripKind::Master).unwrap();
    let slot = b
        .insert(master, a.info().plugin.clone())
        .expect("an insert");
    let (doc, _) = core_model::state::capture(b.doc(), slot, &blob).unwrap();
    let x = export(
        &doc,
        &ExportOptions {
            seq,
            region_gain: RegionGain::Apply,
            clips: ClipRule::SilenceGap(Tick(3840)),
            left_out: LeftOutAudio::default(),
        },
        &ExportEnv {
            plist: &NoPlist,
            app_version: "1.0",
            project_id: "00000000-0000-4000-8000-000000000001",
            export_id: "E1",
            exported_at: 1_700_000_000,
        },
        &AtomicBool::new(false),
    )
    .expect("exports");
    assert_eq!(
        x.states,
        vec![("plugins/001.vstpreset".to_string(), blob.clone())]
    );

    // Written as a package, opened and imported into a new project.
    let mut z = ZipWriter::new(Vec::new());
    z.add(
        "project.xml",
        EntryMethod::Deflated,
        x.project_xml.as_bytes(),
    )
    .unwrap();
    z.add(
        "metadata.xml",
        EntryMethod::Deflated,
        x.metadata_xml.as_bytes(),
    )
    .unwrap();
    for (entry, bytes) in &x.states {
        z.add(entry, EntryMethod::Stored, bytes).unwrap();
    }
    let path = d.path().join("Song.dawproject");
    std::fs::write(&path, z.finish().unwrap()).unwrap();
    let pkg = open(&path, &PackageLimits::default(), &AtomicBool::new(false)).expect("opens");
    let installed = |f: PluginFormat, id: &str| f == PluginFormat::Vst3 && id == VST3_GAIN_ID;
    let imp = map(
        &pkg,
        &BTreeMap::new(),
        &ImportEnv {
            installed: &installed,
            plist: &NoPlist,
            rate: 48_000,
            ids: None,
            eq_plugin: None,
        },
        &AtomicBool::new(false),
    )
    .expect("maps");
    let (new, seq) = new_project();
    let mut h = History::new(new);
    let mut report = Report::default();
    h.begin("Import DAWproject").unwrap();
    let states = apply(&mut h, &imp, seq, &mut report).expect("applies");
    h.commit().unwrap();
    let m = h.doc().mixer();
    let ins = &m.strip(m.master()).unwrap().inserts;
    assert_eq!(ins.len(), 1, "{report:?}");
    let want: PluginRef = ins[0].plugin.clone();
    assert_eq!(
        (want.format, want.id.as_str()),
        (PluginFormat::Vst3, VST3_GAIN_ID)
    );
    let (_, bytes) = states
        .iter()
        .find(|(s, _)| *s == ins[0].id)
        .expect("the slot's state");
    assert_eq!(&bytes[..], &blob[..], "carried as is");

    // A new instance (another memo) takes it: both streams are back.
    probe.set_memo(0);
    let mut b = Vst3Instance::load(&copy, &want.id).unwrap();
    b.load_state(bytes).unwrap();
    assert_eq!(probe.counters().last_memo, 41);
    let g = b.param_value(GAIN).unwrap();
    assert!((g + 6.0).abs() < 1e-6, "{g}");
    assert_eq!(b.save_state().unwrap(), blob);
}
