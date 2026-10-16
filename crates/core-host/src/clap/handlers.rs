//! VizDAW's CLAP host callbacks (clack `HostHandlers`): the plugin's
//! requests land in atomics that `ClapInstance::idle` reads on the main
//! thread; thread-check answers from the main thread's ID and a
//! thread-local "inside an audio call" flag.

use clack_extensions::audio_ports::{
    AudioPortRescanFlags, HostAudioPorts, HostAudioPortsImpl, PluginAudioPorts,
};
use clack_extensions::gui::{GuiSize, HostGui, HostGuiImpl, PluginGui};
use clack_extensions::latency::{HostLatency, HostLatencyImpl, PluginLatency};
use clack_extensions::log::{HostLog, HostLogImpl, LogSeverity};
use clack_extensions::note_ports::{
    HostNotePorts, HostNotePortsImpl, NoteDialects, NotePortRescanFlags, PluginNotePorts,
};
use clack_extensions::params::{
    HostParams, HostParamsImplMainThread, HostParamsImplShared, ParamClearFlags, ParamRescanFlags,
    PluginParams,
};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_extensions::tail::{HostTail, HostTailImpl, PluginTail};
use clack_extensions::thread_check::{HostThreadCheck, HostThreadCheckImpl};
use clack_host::prelude::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::ThreadId;

thread_local! {
    static IN_AUDIO: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` as an `[audio-thread]` call: thread-check reports this thread
/// as the audio thread (and not the main thread) meanwhile. // RT
pub(crate) fn audio_call<T>(f: impl FnOnce() -> T) -> T {
    let was = IN_AUDIO.try_with(|c| c.replace(true)).unwrap_or(false);
    let r = f();
    let _ = IN_AUDIO.try_with(|c| c.set(was));
    r
}

fn in_audio() -> bool {
    IN_AUDIO.try_with(Cell::get).unwrap_or(false)
}

/// What the plugin asked for, read and cleared on the main thread.
#[derive(Debug, Default)]
pub(crate) struct Flags {
    pub restart: AtomicBool,
    pub callback: AtomicBool,
    pub latency: AtomicBool,
    pub params: AtomicBool,
    pub state_dirty: AtomicBool,
    pub ports: AtomicBool,
    pub gui_closed: AtomicBool,
}

impl Flags {
    pub(crate) fn take(flag: &AtomicBool) -> bool {
        flag.swap(false, Ordering::AcqRel)
    }
}

/// The plugin extensions VizDAW uses, queried once while it initializes.
#[derive(Clone, Copy, Default)]
pub(crate) struct PluginExts {
    pub audio_ports: Option<PluginAudioPorts>,
    pub note_ports: Option<PluginNotePorts>,
    pub params: Option<PluginParams>,
    pub state: Option<PluginState>,
    pub latency: Option<PluginLatency>,
    pub tail: Option<PluginTail>,
    pub gui: Option<PluginGui>,
}

/// `[thread-safe]` host data.
pub(crate) struct Shared {
    pub flags: Arc<Flags>,
    pub exts: OnceLock<PluginExts>,
    main: ThreadId,
    name: String,
}

impl Shared {
    pub(crate) fn new(flags: Arc<Flags>, name: String) -> Shared {
        Shared {
            flags,
            exts: OnceLock::new(),
            main: std::thread::current().id(),
            name,
        }
    }
}

impl<'a> SharedHandler<'a> for Shared {
    fn initializing(&self, instance: InitializingPluginHandle<'a>) {
        let _ = self.exts.set(PluginExts {
            audio_ports: instance.get_extension(),
            note_ports: instance.get_extension(),
            params: instance.get_extension(),
            state: instance.get_extension(),
            latency: instance.get_extension(),
            tail: instance.get_extension(),
            gui: instance.get_extension(),
        });
    }

    fn request_restart(&self) {
        self.flags.restart.store(true, Ordering::Release);
    }

    fn request_process(&self) {
        // VizDAW processes every active instance every block.
    }

    fn request_callback(&self) {
        self.flags.callback.store(true, Ordering::Release);
    }
}

impl HostLogImpl for Shared {
    fn log(&self, severity: LogSeverity, message: &str) {
        // Never do I/O inside an audio call (spec §2): drop those.
        if !in_audio() {
            eprintln!("[{}] {severity}: {message}", self.name);
        }
    }
}

impl HostThreadCheckImpl for Shared {
    fn is_main_thread(&self) -> bool {
        !in_audio() && std::thread::current().id() == self.main
    }

    fn is_audio_thread(&self) -> bool {
        in_audio()
    }
}

impl HostParamsImplShared for Shared {
    fn request_flush(&self) {
        // Parameters reach active plugins through `process`, which runs
        // every block.
    }
}

impl HostGuiImpl for Shared {
    fn resize_hints_changed(&self) {}

    fn request_resize(&self, _size: GuiSize) -> Result<(), HostError> {
        Err(HostError::Message("the host window decides its size"))
    }

    fn request_show(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostError> {
        Ok(())
    }

    fn closed(&self, _was_destroyed: bool) {
        self.flags.gui_closed.store(true, Ordering::Release);
    }
}

/// `[main-thread]` host data.
pub(crate) struct MainThread<'a> {
    shared: &'a Shared,
}

impl<'a> MainThread<'a> {
    pub(crate) fn new(shared: &'a Shared) -> MainThread<'a> {
        MainThread { shared }
    }
}

impl<'a> MainThreadHandler<'a> for MainThread<'a> {}

impl HostLatencyImpl for MainThread<'_> {
    fn changed(&self) {
        self.shared.flags.latency.store(true, Ordering::Release);
    }
}

impl HostParamsImplMainThread for MainThread<'_> {
    fn rescan(&self, _flags: ParamRescanFlags) {
        self.shared.flags.params.store(true, Ordering::Release);
    }

    fn clear(&self, _id: ClapId, _flags: ParamClearFlags) {}
}

impl HostStateImpl for MainThread<'_> {
    fn mark_dirty(&self) {
        self.shared.flags.state_dirty.store(true, Ordering::Release);
    }
}

impl HostAudioPortsImpl for MainThread<'_> {
    fn is_rescan_flag_supported(&self, _flag: AudioPortRescanFlags) -> bool {
        true
    }

    fn rescan(&self, _flags: AudioPortRescanFlags) {
        self.shared.flags.ports.store(true, Ordering::Release);
    }
}

impl HostNotePortsImpl for MainThread<'_> {
    fn supported_dialects(&self) -> NoteDialects {
        NoteDialects::CLAP | NoteDialects::MIDI
    }

    fn rescan(&self, _flags: NotePortRescanFlags) {
        self.shared.flags.ports.store(true, Ordering::Release);
    }
}

/// `[audio-thread]` host data.
pub(crate) struct AudioThread;

impl AudioProcessorHandler<'_> for AudioThread {}

impl HostTailImpl for AudioThread {
    fn changed(&mut self) {
        // Read again at the next activation; nothing to do on the audio
        // thread.
    }
}

/// The handler set.
pub(crate) struct VizHost;

impl HostHandlers for VizHost {
    type Shared<'a> = Shared;
    type MainThread<'a> = MainThread<'a>;
    type AudioProcessor<'a> = AudioThread;

    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Shared) {
        builder
            .register::<HostLog>()
            .register::<HostThreadCheck>()
            .register::<HostLatency>()
            .register::<HostParams>()
            .register::<HostState>()
            .register::<HostAudioPorts>()
            .register::<HostNotePorts>()
            .register::<HostTail>()
            .register::<HostGui>();
    }
}
