//! VST3 helpers: the in-repo Rust test VST3 (a private copy per test, so
//! its probe sees only that test's image) and the VST3 SDK's sample
//! plug-ins, which `scripts/build-vst3-samples.sh` builds into
//! `target/vst3sdk/out` (or `$VIZDAW_VST3_SAMPLES`).
//!
//! A test that needs a sample skips, with a message, when it is not built;
//! CI sets `VIZDAW_REQUIRE_VST3_SAMPLES=1`, which makes a missing sample a
//! failure instead.

use core_host::vst3::Vst3Format;
use core_host::{LoadRequest, PluginFormat as _, PluginInstance};
use core_model::mixer::{PluginFormat, PluginRef};
use std::path::{Path, PathBuf};
use test_plugins::Vst3Probe;

/// The reference of VST3 class `id` named `name`.
pub fn vst3_ref(id: &str, name: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Vst3,
        id: id.into(),
        name: name.into(),
        vendor: "VizDAW".into(),
        version: "1.0.0".into(),
    }
}

/// A private copy of the Rust test VST3 and a probe on its image.
pub struct Private {
    pub _dir: tempfile::TempDir,
    pub path: PathBuf,
    pub probe: Vst3Probe,
}

pub fn private() -> Private {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Private.vst3");
    test_plugins::copy_vst3_bundle(&path);
    let probe = Vst3Probe::open(&path);
    Private {
        _dir: dir,
        path,
        probe,
    }
}

/// Loads class `id` from the bundle at `path` through the format, as the
/// plug-in host does (prepare off the main thread, finish on it).
pub fn load_vst3(path: &Path, id: &str) -> Box<dyn PluginInstance> {
    let mut f = Vst3Format::new();
    let req = LoadRequest {
        plugin: vst3_ref(id, id),
        path: Some(path.to_path_buf()),
        sample_rate: 48_000.0,
        max_frames: 4096,
    };
    let prepared = (f.prepare(&req))().unwrap();
    f.finish(&req, prepared).unwrap()
}

/// Where the SDK samples are.
fn samples_dir() -> PathBuf {
    match std::env::var_os("VIZDAW_VST3_SAMPLES") {
        Some(d) => PathBuf::from(d),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/vst3sdk/out"),
    }
}

/// The SDK sample bundle `name` (e.g. `again.vst3`), or `None` (after a
/// message) when it is not built, unless CI requires it.
pub fn sample(name: &str) -> Option<PathBuf> {
    let p = samples_dir().join(name);
    if p.is_dir() {
        return Some(p);
    }
    let required = std::env::var("VIZDAW_REQUIRE_VST3_SAMPLES").is_ok_and(|v| v == "1");
    assert!(
        !required,
        "the VST3 SDK sample {} is missing (run scripts/build-vst3-samples.sh)",
        p.display()
    );
    eprintln!(
        "skipped: the VST3 SDK sample {} is not built (scripts/build-vst3-samples.sh builds it)",
        p.display()
    );
    None
}

/// Class IDs of the SDK samples the tests use (from each bundle's
/// `moduleinfo.json`).
pub mod sdk {
    /// `again.vst3`: a stereo gain (Gain 0, VuPPM 1 read-only, Bypass 2).
    pub const AGAIN: &str = "84E8DE5F92554F5396FAE4133C935A18";
    /// `again.vst3`: the gain with a mono sidechain input.
    pub const AGAIN_SIDECHAIN: &str = "41347FD6FED64094AFBB12B7DBA1D441";
    /// `again-sample-accurate.vst3`: a gain that ramps sample by sample
    /// (Bypass 0, Gain 1).
    pub const AGAIN_SAMPLE_ACCURATE: &str = "C18D3C1E719E4E29924D3ECAA5E4DA18";
    /// `adelay.vst3`: a delay (Delay 100 in seconds 0-1, Bypass 101).
    pub const ADELAY: &str = "0CDBB66985D548A9BFD8371909D24BB3";
    /// `note-expression-synth.vst3`: the instrument without an editor.
    pub const NOTE_EXPRESSION_SYNTH: &str = "6EE65CD1B83A4AF480AA7929AEA6B8A0";
    /// `host-checker.vst3`: checks the host's calls and reports problems
    /// through read-only output parameters.
    pub const HOST_CHECKER: &str = "23FC190E02DD4499A8D2230E50617DA3";
    /// `panner.vst3`: mono in, stereo out (Bypass 100, Pan 102).
    pub const PANNER: &str = "A2EAF7DB320640F48EDE380DDF89562C";
}

/// The buffers of one block for a plug-in with `info`'s ports: its main
/// input, its sidechain and every output channel.
pub struct Block {
    pub input: Vec<Box<[f32]>>,
    pub sidechain: Vec<Box<[f32]>>,
    pub outputs: Vec<Box<[f32]>>,
    pub out: core_host::ParamOut,
}

impl Block {
    pub fn new(info: &core_host::PluginInfo, frames: usize) -> Block {
        let chans = |n: usize| -> Vec<Box<[f32]>> {
            (0..n)
                .map(|_| vec![0.0f32; frames].into_boxed_slice())
                .collect()
        };
        let port = |i: Option<usize>| {
            i.and_then(|i| info.inputs.get(i))
                .map_or(0, |p| usize::from(p.channels))
        };
        Block {
            input: chans(port(info.main_input())),
            sidechain: chans(port(info.sidechain_input())),
            outputs: chans(info.output_channels()),
            out: core_host::ParamOut::default(),
        }
    }

    /// Fills the main input with `input` and the sidechain with `side`.
    pub fn fill(&mut self, input: f32, side: f32) {
        for c in &mut self.input {
            c.fill(input);
        }
        for c in &mut self.sidechain {
            c.fill(side);
        }
    }

    /// Runs `frames` frames through `p`.
    pub fn run(
        &mut self,
        p: &mut dyn core_host::AudioProcessor,
        frames: u32,
        events: &[core_host::MidiEvent],
        params: &[core_host::ParamChange],
        transport: &core_engine::backend::BlockInfo,
    ) -> core_engine::graph::ProcessStatus {
        self.out.clear();
        let mut ctx = core_host::ProcessCtx {
            frames,
            input: &mut self.input,
            sidechain: &mut self.sidechain,
            outputs: &mut self.outputs,
            events,
            params_in: params,
            params_out: &mut self.out,
            transport,
        };
        p.process(&mut ctx)
    }

    /// Runs `frames` frames with no events or parameter changes.
    pub fn quiet(
        &mut self,
        p: &mut dyn core_host::AudioProcessor,
        frames: u32,
    ) -> core_engine::graph::ProcessStatus {
        self.run(
            p,
            frames,
            &[],
            &[],
            &core_engine::backend::BlockInfo::default(),
        )
    }
}
