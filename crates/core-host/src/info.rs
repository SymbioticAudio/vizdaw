//! What a plugin is: ports, parameters, latency and tail (spec §5). The
//! scanner prints it as JSON and the catalog stores it, so it is serde.

use core_model::mixer::{ParamId, PluginRef};
use serde::{Deserialize, Serialize};

/// Instrument or effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// Takes notes, makes audio (a plugin instrument destination).
    Instrument,
    /// Processes audio (an insert).
    Effect,
}

/// What an audio port is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortRole {
    /// The main input or output.
    Main,
    /// A sidechain input (CLAP: the first non-main input; AU: input bus 1).
    Sidechain,
    /// Any other port (an instrument's extra outputs).
    Aux,
}

/// One audio port (a channel group).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioPortInfo {
    /// Display name.
    pub name: String,
    /// Channels (1 or 2 in VizDAW; the graph is stereo, Plan Ruling 9).
    pub channels: u16,
    /// Role.
    pub role: PortRole,
}

/// One parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamInfo {
    /// Stable ID (CLAP `clap_id`, AU parameter address).
    pub id: ParamId,
    /// Display name.
    pub name: String,
    /// Group path, `/`-separated (may be empty).
    pub module: String,
    /// Lowest plain value.
    pub min: f64,
    /// Highest plain value.
    pub max: f64,
    /// Default plain value.
    pub default: f64,
    /// Takes whole values only.
    pub stepped: bool,
    /// Automatable (sub-project 4 lists only these).
    pub automatable: bool,
    /// Hidden from the user.
    pub hidden: bool,
}

/// How long a plugin keeps sounding after its input stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tail {
    /// No tail (or not reported).
    #[default]
    None,
    /// This many frames.
    Frames(u32),
    /// Forever (a reverb that never decays to silence).
    Infinite,
}

/// Everything VizDAW knows about a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginInfo {
    /// Format, id, name, vendor, version.
    pub plugin: PluginRef,
    /// Instrument or effect.
    pub kind: PluginKind,
    /// Audio inputs, main first.
    pub inputs: Vec<AudioPortInfo>,
    /// Audio outputs, main first.
    pub outputs: Vec<AudioPortInfo>,
    /// Takes notes.
    pub note_input: bool,
    /// Parameters, in the plugin's order.
    pub params: Vec<ParamInfo>,
    /// Latency in frames (at the last activation).
    pub latency: u32,
    /// Tail.
    pub tail: Tail,
}

impl PluginInfo {
    /// Index of the sidechain input port, if any (a plugin without one
    /// shows no sidechain selector, spec §5).
    pub fn sidechain_input(&self) -> Option<usize> {
        self.inputs
            .iter()
            .position(|p| p.role == PortRole::Sidechain)
    }

    /// Index of the main input port, if any.
    pub fn main_input(&self) -> Option<usize> {
        self.inputs.iter().position(|p| p.role == PortRole::Main)
    }

    /// Total output channels over every port.
    pub fn output_channels(&self) -> usize {
        self.outputs.iter().map(|p| usize::from(p.channels)).sum()
    }

    /// A parameter by ID.
    pub fn param(&self, id: ParamId) -> Option<&ParamInfo> {
        self.params.iter().find(|p| p.id == id)
    }
}
