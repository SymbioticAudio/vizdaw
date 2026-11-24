//! In-repo CLAP plugins for host and scanner tests (spec §13), built with
//! `clack-plugin`. One binary, `VizDAW Test.clap`, exposes every kind:
//!
//! | id | what it does |
//! |---|---|
//! | `vizdaw.test.gain` | stereo gain (param 0) with a configurable latency (param 1, restarts); param 2 "Wiggle" makes it perform a UI-style gesture setting Gain; state round-trips both values; CLAP `render`, with unlisted test switches 3 (reads the offline mode) and 4 (declares a hard real-time requirement) |
//! | `vizdaw.test.ducker` | main × (1 − min(1, peak(sidechain))) per block; sidechain on input port 1 |
//! | `vizdaw.test.synth4` | instrument with 4 stereo outputs: notes on MIDI channel k sound on output k |
//! | `vizdaw.test.nan` | writes NaN into every block |
//! | `vizdaw.test.bank` | passes audio through; 24 automatable parameters (ids 100–123) that only store their values |
//! | `vizdaw.test.crasher` | crashes (SIGSEGV) when activated |
//! | `vizdaw.test.hanger` | never returns from activate |
//! | `vizdaw.test.timer` | passes audio through; registers host timers and file descriptors (CLAP `timer-support`, `posix-fd-support`) on command: the unlisted `PARAM_TIMER_*`/`PARAM_FD_*` switches; it counts its callbacks, read back as parameter values. It has a headless embedded editor (CLAP `gui`, Cocoa or X11) that draws nothing and reports what the host told it (`PARAM_GUI_*`) |
//! | `vizdaw.test.gui` | passes audio through; on Linux, a real embedded X11 editor (CLAP `gui`): in `gui.create` it connects to the X server and registers a host timer and its connection's descriptor; it draws a [`GUI_COLOR`] child window under the host's parent, and from its first timer callback after `show` asks the host to grow it by [`GUI_GROW`] points (`request_resize`). What it saw reads back as `PARAM_GUI_*` values. Elsewhere it has no editor |
//!
//! Creating a CLAP plug-in gives the creating thread a thread-exit
//! destructor in this library, as the VST3 side does (see
//! [`Vst3Probe`], which also reads the CLAP entry's `init` and `deinit`
//! counts through [`Vst3Probe::open_clap`]).
//!
//! The same library is also a VST3 module (`GetPluginFactory`, see [`VST3_GAIN_ID`]); [`bundle`] builds
//! the `.clap` bundle for tests and [`vst3_bundle`] the `.vst3` one.

mod bundle;
mod editor;
#[cfg(target_os = "linux")]
mod gui;
mod plugin;
mod timer;
mod vst3_module;

pub use bundle::{bundle, copy_bundle, copy_vst3_bundle, scanner, vst3_bundle};
pub use editor::{BASE_SIZE, MAX_SIZE, MIN_SIZE};
pub use vst3_module::{
    VST3_BANK_BASE, VST3_BANK_ID, VST3_BANK_SIZE, VST3_COMPONENT_GAIN, VST3_CONTROLLER_ID,
    VST3_CRASHER_ID, VST3_GAIN_ID, VST3_HANGER_ID, VST3_NAN_ID, VST3_PARAM_BEND, VST3_PARAM_GAIN,
    VST3_PARAM_METER, VST3_PARAM_MOD, VST3_PARAM_MODE, VST3_PARAM_PRESSURE, VST3_PARAM_SECRET,
    VST3_PARAMS, VST3_PROCESS_CRASHER_ID, VST3_SYNTH_ID, Vst3Counters, Vst3Edit, Vst3Mode,
    Vst3ParamDef, Vst3Probe, vst3_restart,
};

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
    /// Pass-through that registers host timers and file descriptors.
    Timer,
    /// Pass-through with a real X11 editor (Linux).
    Gui,
}

impl Kind {
    /// Every kind, in factory order.
    pub const ALL: [Kind; 9] = [
        Kind::Gain,
        Kind::Ducker,
        Kind::Synth4,
        Kind::Nan,
        Kind::Crasher,
        Kind::Hanger,
        Kind::Bank,
        Kind::Timer,
        Kind::Gui,
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
            Kind::Timer => "vizdaw.test.timer",
            Kind::Gui => "vizdaw.test.gui",
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
            Kind::Timer => "Test Timer",
            Kind::Gui => "Test GUI",
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
/// Test switch, not in the gain plugin's parameter list: reads 1 while the
/// host has it rendering offline (CLAP `render`), else 0.
pub const PARAM_RENDER_OFFLINE: u32 = 3;
/// Test switch, not in the gain plugin's parameter list: set it to 1 (while
/// inactive) and the gain plugin declares a hard real-time requirement.
pub const PARAM_HARD_REALTIME: u32 = 4;
/// The bank plugin's first parameter id (0 … 1, default 0).
pub const PARAM_BANK_FIRST: u32 = 100;
/// How many parameters the bank plugin has (ids `PARAM_BANK_FIRST..+24`).
pub const BANK_PARAMS: u32 = 24;

/// Timer kind, set while inactive: registers a host timer with this period
/// (ms); [`PARAM_LAST_RESULT`] then reads its id, or −1 when refused.
pub const PARAM_TIMER_REGISTER: u32 = 200;
/// Timer kind: unregisters timer `value`; [`PARAM_LAST_RESULT`] reads 1 or 0.
pub const PARAM_TIMER_UNREGISTER: u32 = 201;
/// Timer kind: timer `value` unregisters itself inside its next callback.
pub const PARAM_TIMER_SELF_UNREGISTER: u32 = 202;
/// Timer kind: every callback of timer `value` registers a new period-0
/// timer (a flood); [`PARAM_LAST_RESULT`] then reads the newest one's id.
pub const PARAM_TIMER_SPAWN: u32 = 203;
/// Timer kind: registers file descriptor `value / 8` for the CLAP fd flags
/// `value % 8` (read 1, write 2, error 4); [`PARAM_LAST_RESULT`] reads 1 or 0.
pub const PARAM_FD_REGISTER: u32 = 204;
/// Timer kind: changes a registered descriptor's flags, encoded as for
/// [`PARAM_FD_REGISTER`].
pub const PARAM_FD_MODIFY: u32 = 205;
/// Timer kind: unregisters descriptor `value`.
pub const PARAM_FD_UNREGISTER: u32 = 206;
/// Timer kind: descriptor `value` unregisters itself inside its next
/// callback.
pub const PARAM_FD_SELF_UNREGISTER: u32 = 207;
/// Timer kind: the result of the last register or unregister switch.
pub const PARAM_LAST_RESULT: u32 = 210;
/// Timer kind's editor: 1 makes it resizable (`can_resize`, `adjust_size`
/// clamping to [`MIN_SIZE`]–[`MAX_SIZE`], `set_size`), 0 fixed (the
/// default).
pub const PARAM_GUI_RESIZABLE: u32 = 220;
/// Timer kind's editor: asks the host to resize it to `value >> 16` by
/// `value & 0xFFFF` (the API's units, CLAP `request_resize`);
/// [`PARAM_LAST_RESULT`] reads 1 when the host accepted, else 0. Accepted,
/// it takes that size itself at once (as editors that grow a panel do),
/// resizable or not.
pub const PARAM_GUI_REQUEST_RESIZE: u32 = 221;
/// Timer kind: unregisters timer `value` inside the next `gui.destroy`
/// and appends `gui.destroy unregister <id>: <1|0>` to `<bundle>.log`.
pub const PARAM_GUI_DESTROY_UNREGISTER: u32 = 222;
/// Timer kind: unregisters timer `value` while the plug-in is destroyed
/// and appends `destroy unregister <id>: <1|0>` to `<bundle>.log`.
pub const PARAM_DESTROY_UNREGISTER: u32 = 223;
/// Timer kind's editor: reads the last `set_scale` (0 when none came).
pub const PARAM_GUI_SCALE: u32 = 230;
/// Timer kind's editor: reads 1 if `set_scale` came before the last
/// `set_parent` (since its `create`), 0 if not, −1 before any `set_parent`.
pub const PARAM_GUI_SCALED_FIRST: u32 = 231;
/// Timer kind's editor: reads its width in the API's units (physical
/// pixels on X11, points on Cocoa); [`BASE_SIZE`] times the scale at first.
pub const PARAM_GUI_WIDTH: u32 = 232;
/// Timer kind's editor: reads its height, as [`PARAM_GUI_WIDTH`].
pub const PARAM_GUI_HEIGHT: u32 = 233;
/// Timer kind's editor: reads 1 while created, else 0.
pub const PARAM_GUI_OPEN: u32 = 234;
/// Timer kind's editor: 1 makes its next `set_size` fail (once).
pub const PARAM_GUI_REFUSE_SET_SIZE: u32 = 224;
/// Timer kind's editor: reads how many `set_size` calls it got (accepted
/// or not).
pub const PARAM_GUI_SET_SIZES: u32 = 235;
/// GUI kind: the timer `gui.create` registered (its id), or −1 when the
/// host refused it; −2 before any `create`.
pub const PARAM_GUI_CREATE_TIMER: u32 = 240;
/// GUI kind: 1 if the host accepted the X connection's descriptor in
/// `gui.create`, 0 if not; −2 before any `create`.
pub const PARAM_GUI_CREATE_FD: u32 = 241;
/// GUI kind: its X11 window's id (0 while it has none).
pub const PARAM_GUI_WINDOW: u32 = 242;
/// GUI kind: the X11 parent window the host gave it (0 before any).
pub const PARAM_GUI_PARENT: u32 = 243;
/// GUI kind: how many times it filled its window with [`GUI_COLOR`].
pub const PARAM_GUI_DRAWS: u32 = 244;
/// GUI kind: how many callbacks its timer had.
pub const PARAM_GUI_TIMER_CALLS: u32 = 245;
/// GUI kind: how many callbacks its X connection's descriptor had.
pub const PARAM_GUI_FD_CALLS: u32 = 246;
/// GUI kind: its own resize request: −1 not asked yet, 1 accepted (it
/// took the size), 0 refused.
pub const PARAM_GUI_RESIZE_ASKED: u32 = 247;
/// GUI kind: the colour it fills its window with (`0xRRGGBB`, a
/// TrueColor pixel value).
pub const GUI_COLOR: u32 = 0x33_66_CC;
/// GUI kind: how much it asks to grow, in points (times the scale in
/// pixels).
pub const GUI_GROW: (u32, u32) = (40, 20);
/// GUI kind: its timer's period (ms).
pub const GUI_TIMER_MS: u32 = 30;
/// Timer kind: reads how many times timer `id` fired, at `PARAM_TIMER_FIRES + id`.
pub const PARAM_TIMER_FIRES: u32 = 1000;
/// Timer kind: reads how many callbacks descriptor `fd` had, at `PARAM_FD_CALLS + fd`.
pub const PARAM_FD_CALLS: u32 = 2000;
/// Timer kind: reads the flags of descriptor `fd`'s last callback, at
/// `PARAM_FD_FLAGS + fd`.
pub const PARAM_FD_FLAGS: u32 = 3000;

/// The entry: one factory exposing every [`Kind`].
pub struct TestEntry {
    factory: PluginFactoryWrapper<TestFactory>,
}

impl Drop for TestEntry {
    fn drop(&mut self) {
        vst3_module::clap_deinit();
    }
}

impl Entry for TestEntry {
    fn new(bundle_path: Option<&CStr>) -> Result<Self, EntryLoadError> {
        vst3_module::clap_init();
        Ok(TestEntry {
            factory: PluginFactoryWrapper::new(TestFactory {
                descriptors: Kind::ALL.map(Kind::descriptor),
                bundle: bundle_path
                    .and_then(|p| p.to_str().ok())
                    .map(std::path::PathBuf::from),
            }),
        })
    }

    fn declare_factories<'a>(&'a self, builder: &mut EntryFactories<'a>) {
        builder.register_factory(&self.factory);
    }
}

struct TestFactory {
    descriptors: [PluginDescriptor; 9],
    /// Where the bundle was loaded from (the timer kind logs beside it).
    bundle: Option<std::path::PathBuf>,
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
        vst3_module::mark_thread();
        Some(PluginInstance::new::<TestPlugin>(
            host_info,
            desc,
            move |host| Ok(plugin::Shared::new(kind, host, self.bundle.as_deref())),
            |host, shared| Ok(plugin::MainThread::new(host, shared)),
        ))
    }
}

clack_export_entry!(TestEntry);
