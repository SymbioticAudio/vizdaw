//! The macOS [`SinkFactory`]: the DLS synth for `BuiltinSynth`, CoreMIDI
//! for the virtual source and named destinations.

use crate::dls::DlsSynth;
use crate::midi::{MidiSystem, list_destinations};
use core_engine::BackendError;
use core_engine::backend::{MidiBackend, MidiPortInfo};
use core_engine::engine::SinkFactory;
use core_engine::render::Sink;
use core_engine::schedule::{TargetIdx, TargetKind};

/// Name of VizDAW's virtual source.
pub const VIRTUAL_SOURCE_NAME: &str = "VizDAW Out";

/// Creates platform sinks; the MIDI client is created on first use.
#[derive(Debug, Default)]
pub struct MacSinks {
    midi: Option<MidiSystem>,
}

impl MacSinks {
    /// No MIDI client yet.
    pub fn new() -> MacSinks {
        MacSinks::default()
    }

    fn midi(&mut self) -> Result<&mut MidiSystem, BackendError> {
        if self.midi.is_none() {
            self.midi = Some(MidiSystem::new("VizDAW")?);
        }
        self.midi.as_mut().ok_or(BackendError::NoDevice)
    }
}

impl SinkFactory for MacSinks {
    fn ports(&self) -> Vec<MidiPortInfo> {
        list_destinations()
    }

    fn make(
        &mut self,
        _target: TargetIdx,
        kind: &TargetKind,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Sink, BackendError> {
        Ok(match kind {
            TargetKind::BuiltinSynth => {
                Sink::Instrument(Box::new(DlsSynth::new(sample_rate, max_frames)?))
            }
            TargetKind::MidiVirtualSource => {
                Sink::Midi(self.midi()?.open_virtual_source(VIRTUAL_SOURCE_NAME)?)
            }
            TargetKind::MidiDestination(port) => Sink::Midi(self.midi()?.open_destination(port)?),
        })
    }
}
