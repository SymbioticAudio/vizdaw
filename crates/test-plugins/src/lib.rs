//! In-repo CLAP plugins for host and scanner tests (spec §13), built with
//! `clack-plugin`. One binary, `VizDAW Test.clap`, exposes every kind:
//!
//! | id | what it does |
//! |---|---|
//! | `vizdaw.test.gain` | stereo gain (param 0) with a configurable latency (param 1, restarts); param 2 "Wiggle" makes it perform a UI-style gesture setting Gain; state round-trips both values |
//! | `vizdaw.test.ducker` | main × (1 − min(1, peak(sidechain))) per block; sidechain on input port 1 |
//! | `vizdaw.test.synth4` | instrument with 4 stereo outputs: notes on MIDI channel k sound on output k |
//! | `vizdaw.test.nan` | writes NaN into every block |
//! | `vizdaw.test.bank` | passes audio through; 24 automatable parameters (ids 100–123) that only store their values |
//! | `vizdaw.test.crasher` | crashes (SIGSEGV) when activated |
//! | `vizdaw.test.hanger` | never returns from activate |
//!
//! [`bundle`] builds the `.clap` bundle for tests.

mod bundle;
mod plugin;

pub use bundle::{bundle, scanner};

use clack_plugin::entry::prelude::*;
use clack_plugin::prelude::*;
use plugin::TestPlugin;
use std::ffi::CStr;

/// What a test plugin does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Gain + latency + state + gestures.
    Gain,
    /// Sidechain ducker.
    Ducker,
    /// Four-output synth.
    Synth4,
    /// NaN emitter.
    Nan,
    /// Crashes on activate.
    Crasher,
    /// Hangs on activate.
    Hanger,
    /// Pass-through with [`BANK_PARAMS`] automatable parameters.
    Bank,
}

impl Kind {
    /// Every kind, in factory order.
    pub const ALL: [Kind; 7] = [
        Kind::Gain,
        Kind::Ducker,
        Kind::Synth4,
        Kind::Nan,
        Kind::Crasher,
        Kind::Hanger,
        Kind::Bank,
    ];

    /// The CLAP plugin id.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Gain => "vizdaw.test.gain",
            Kind::Ducker => "vizdaw.test.ducker",
            Kind::Synth4 => "vizdaw.test.synth4",
            Kind::Nan => "vizdaw.test.nan",
            Kind::Crasher => "vizdaw.test.crasher",
            Kind::Hanger => "vizdaw.test.hanger",
            Kind::Bank => "vizdaw.test.bank",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Gain => "Test Gain",
            Kind::Ducker => "Test Ducker",
            Kind::Synth4 => "Test Synth 4-Out",
            Kind::Nan => "Test NaN",
            Kind::Crasher => "Test Crasher",
            Kind::Hanger => "Test Hanger",
            Kind::Bank => "Test Bank",
        }
    }

    fn descriptor(self) -> PluginDescriptor {
        use clack_plugin::plugin::features::{AUDIO_EFFECT, INSTRUMENT, STEREO, SYNTHESIZER};
        let d = PluginDescriptor::new(self.id(), self.name())
            .with_vendor("VizDAW")
            .with_version("1.0.0");
        match self {
            Kind::Synth4 => d.with_features([INSTRUMENT, SYNTHESIZER, STEREO]),
            _ => d.with_features([AUDIO_EFFECT, STEREO]),
        }
    }
}

/// Gain parameter id (0 … 2, default 1).
pub const PARAM_GAIN: u32 = 0;
/// Latency parameter id (0 … 48 000 frames, stepped, default 0).
pub const PARAM_LATENCY: u32 = 1;
/// "Wiggle": setting it to `v` makes the plugin perform a gesture that sets
/// Gain to `v`, as if a user dragged its knob.
pub const PARAM_WIGGLE: u32 = 2;
/// The bank plugin's first parameter id (0 … 1, default 0).
pub const PARAM_BANK_FIRST: u32 = 100;
/// How many parameters the bank plugin has (ids `PARAM_BANK_FIRST..+24`).
pub const BANK_PARAMS: u32 = 24;

/// The entry: one factory exposing every [`Kind`].
pub struct TestEntry {
    factory: PluginFactoryWrapper<TestFactory>,
}

impl Entry for TestEntry {
    fn new(_bundle_path: Option<&CStr>) -> Result<Self, EntryLoadError> {
        Ok(TestEntry {
            factory: PluginFactoryWrapper::new(TestFactory {
                descriptors: Kind::ALL.map(Kind::descriptor),
            }),
        })
    }

    fn declare_factories<'a>(&'a self, builder: &mut EntryFactories<'a>) {
        builder.register_factory(&self.factory);
    }
}

struct TestFactory {
    descriptors: [PluginDescriptor; 7],
}

impl PluginFactoryImpl for TestFactory {
    fn plugin_count(&self) -> u32 {
        Kind::ALL.len() as u32
    }

    fn plugin_descriptor(&self, index: u32) -> Option<&PluginDescriptor> {
        self.descriptors.get(index as usize)
    }

    fn create_plugin<'a>(
        &'a self,
        host_info: HostInfo<'a>,
        plugin_id: &CStr,
    ) -> Option<PluginInstance<'a>> {
        let i = Kind::ALL
            .iter()
            .position(|k| k.id().as_bytes() == plugin_id.to_bytes())?;
        let kind = *Kind::ALL.get(i)?;
        let desc = self.descriptors.get(i)?;
        Some(PluginInstance::new::<TestPlugin>(
            host_info,
            desc,
            move |host| Ok(plugin::Shared::new(kind, host)),
            |host, shared| Ok(plugin::MainThread::new(host, shared)),
        ))
    }
}

clack_export_entry!(TestEntry);
