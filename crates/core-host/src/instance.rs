//! The plugin abstraction (spec §5, Plan Ruling 1): a main-thread
//! [`PluginInstance`] that activates into a real-time [`AudioProcessor`],
//! and the [`PluginFormat`] that makes instances (AU on macOS, CLAP
//! everywhere).

use crate::error::HostError;
use crate::info::PluginInfo;
use crate::process::ProcessCtx;
use core_engine::graph::ProcessStatus;
use core_model::mixer::{ParamId, PluginRef};
use std::any::Any;
use std::ffi::c_void;
use std::path::PathBuf;

/// The real-time half of an active plugin. Moved to the render threads;
/// used by one thread at a time.
pub trait AudioProcessor: Send {
    /// Processes one block. `Error` counts as a faulty block (spec §7).
    /// // RT
    fn process(&mut self, ctx: &mut ProcessCtx<'_>) -> ProcessStatus;
    /// Clears voices, delay memory and envelopes. // RT
    fn reset(&mut self);
}

/// What an instance asked for since the last [`PluginInstance::idle`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstanceEvents {
    /// The plugin wants to be deactivated and activated again (CLAP
    /// `request_restart`, e.g. after a latency change).
    pub restart: bool,
    /// Its latency changed (read again after the next activation).
    pub latency_changed: bool,
    /// Its parameter list or values changed outside a gesture.
    pub params_changed: bool,
    /// Its state changed in a way the host should save (CLAP
    /// `state.mark_dirty`).
    pub state_dirty: bool,
}

/// The native view an editor attaches to (an `NSView *` on macOS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentView(pub *mut c_void);

/// A plugin's own editor. Main thread only.
pub trait PluginEditor {
    /// Attaches the editor to `parent` and shows it; returns its size in
    /// points.
    fn open(&mut self, parent: ParentView) -> Result<(u32, u32), HostError>;
    /// Asks for a new size; returns the size the plugin accepted.
    fn resize(&mut self, width: u32, height: u32) -> Result<(u32, u32), HostError>;
    /// Whether the user may resize it.
    fn can_resize(&self) -> bool;
    /// Hides and detaches it (also on drop).
    fn close(&mut self);
}

/// The main-thread half of a plugin. Not `Send`: CLAP and AppKit tie it to
/// the thread that made it (Plan Ruling 1).
pub trait PluginInstance {
    /// Ports, parameters, latency and tail as of the last activation.
    fn info(&self) -> &PluginInfo;
    /// Allocates for `sample_rate` and blocks of up to `max_frames` and
    /// hands out the processor. Fails while a processor is out.
    fn activate(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn AudioProcessor>, HostError>;
    /// Takes the processor back and frees what `activate` allocated.
    fn deactivate(&mut self, processor: Box<dyn AudioProcessor>);
    /// Whether a processor is out.
    fn is_active(&self) -> bool;
    /// The plugin's full state.
    fn save_state(&mut self) -> Result<Vec<u8>, HostError>;
    /// Restores a state from [`PluginInstance::save_state`].
    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError>;
    /// Sets parameters while inactive (load: state first, then the
    /// overlay, spec §4). Active instances get values through the
    /// processor instead.
    fn set_params(&mut self, values: &[(ParamId, f64)]) -> Result<(), HostError>;
    /// A parameter's current value, when the plugin reports one.
    fn param_value(&mut self, id: ParamId) -> Option<f64>;
    /// The plugin's editor, if it has one.
    fn editor(&mut self) -> Option<Box<dyn PluginEditor + '_>>;
    /// Main-thread servicing (`host_idle`, 60 Hz): runs callbacks the
    /// plugin asked for and reports what it requested.
    fn idle(&mut self) -> InstanceEvents;
}

/// What to load.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadRequest {
    /// The plugin.
    pub plugin: PluginRef,
    /// Where it lives (a CLAP bundle), from the catalog.
    pub path: Option<PathBuf>,
    /// Rate it will be activated at.
    pub sample_rate: f64,
    /// Largest block it will be activated for.
    pub max_frames: u32,
}

/// What a format's loader step made, handed to [`PluginFormat::finish`] on
/// the main thread.
pub struct Prepared(pub Box<dyn Any + Send>);

/// A format's loader step: runs on the loader thread.
pub type PrepareJob = Box<dyn FnOnce() -> Result<Prepared, HostError> + Send>;

/// A plugin format: AU (macOS) or CLAP.
pub trait PluginFormat {
    /// Which format.
    fn format(&self) -> core_model::mixer::PluginFormat;
    /// The part of loading that may run off the main thread (AU v2:
    /// instantiate and allocate; AU v3: the asynchronous instantiation;
    /// CLAP: nothing, Plan Ruling 2).
    fn prepare(&self, req: &LoadRequest) -> PrepareJob;
    /// Finishes on the main thread.
    fn finish(
        &mut self,
        req: &LoadRequest,
        prepared: Prepared,
    ) -> Result<Box<dyn PluginInstance>, HostError>;
}
