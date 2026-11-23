//! VizDAW's CLAP host callbacks (clack `HostHandlers`): the plugin's
//! requests land in atomics that `ClapInstance::idle` reads on the main
//! thread; thread-check answers from the main thread's ID and a
//! thread-local "inside an audio call" flag. Timers and file descriptors go
//! into the instance's run-loop table ([`crate::run_loop`]).

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
#[cfg(unix)]
use clack_extensions::posix_fd::{FdFlags, HostPosixFdImpl, PluginPosixFd};
use clack_extensions::state::{HostState, HostStateImpl, PluginState};
use clack_extensions::tail::{HostTail, HostTailImpl, PluginTail};
use clack_extensions::thread_check::{HostThreadCheck, HostThreadCheckImpl};
use clack_extensions::timer::{HostTimerImpl, PluginTimer, TimerId};
use clack_host::prelude::*;
use std::cell::{Cell, RefCell};
#[cfg(unix)]
use std::os::unix::io::RawFd;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::ThreadId;

use crate::run_loop::Registrations;

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
    /// The editor is open and said it can resize: `request_resize` is
    /// accepted (and checked again on the main thread).
    pub gui_resizable: AtomicBool,
    /// Its resize hints changed: `can_resize` is read again.
    pub gui_hints: AtomicBool,
    /// The last accepted `request_resize`, in the API's units
    /// ([`GuiSize::pack_to_u64`]; 0: none).
    pub gui_resize: AtomicU64,
}

impl Flags {
    pub(crate) fn take(flag: &AtomicBool) -> bool {
        flag.swap(false, Ordering::AcqRel)
    }

    /// Takes the pending resize request, if any.
    pub(crate) fn take_resize(&self) -> Option<GuiSize> {
        match self.gui_resize.swap(0, Ordering::AcqRel) {
            0 => None,
            v => Some(GuiSize::unpack_from_u64(v)),
        }
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
    pub render: Option<clack_extensions::render::PluginRender>,
    pub timer: Option<PluginTimer>,
    #[cfg(unix)]
    pub posix_fd: Option<PluginPosixFd>,
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
            render: instance.get_extension(),
            timer: instance.get_extension(),
            #[cfg(unix)]
            posix_fd: instance.get_extension(),
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
    fn resize_hints_changed(&self) {
        self.flags.gui_hints.store(true, Ordering::Release);
    }

    /// Accepted only while a resizable editor is open; the main thread
    /// clamps it through `adjust_size`, sets it and reports it at the next
    /// idle (`InstanceEvents::editor_resize`). The last request wins.
    fn request_resize(&self, size: GuiSize) -> Result<(), HostError> {
        if size.width == 0 || size.height == 0 {
            return Err(HostError::Message("an empty size"));
        }
        if !self.flags.gui_resizable.load(Ordering::Acquire) {
            return Err(HostError::Message(
                "the editor is not open or not resizable",
            ));
        }
        self.flags
            .gui_resize
            .store(size.pack_to_u64(), Ordering::Release);
        Ok(())
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
    /// The instance's timers and descriptors, shared with the run loop.
    registrations: Rc<RefCell<Registrations>>,
}

impl<'a> MainThread<'a> {
    pub(crate) fn new(
        shared: &'a Shared,
        registrations: Rc<RefCell<Registrations>>,
    ) -> MainThread<'a> {
        MainThread {
            shared,
            registrations,
        }
    }

    /// Runs `f` on the run-loop table, refusing a call from another thread
    /// (the table is main-thread only) or from inside the loop's own
    /// borrow.
    fn table<T>(&self, f: impl FnOnce(&mut Registrations) -> Option<T>) -> Result<T, HostError> {
        if !self.shared.is_main_thread() {
            return Err(HostError::Message("not on the main thread"));
        }
        self.registrations
            .try_borrow_mut()
            .ok()
            .and_then(|mut t| f(&mut t))
            .ok_or(HostError::Message("refused"))
    }
}

impl HostTimerImpl for MainThread<'_> {
    fn register_timer(&self, period_ms: u32) -> Result<TimerId, HostError> {
        self.table(|t| t.register_timer(period_ms)).map(TimerId)
    }

    fn unregister_timer(&self, timer_id: TimerId) -> Result<(), HostError> {
        self.table(|t| t.unregister_timer(timer_id.0).then_some(()))
    }
}

#[cfg(unix)]
impl HostPosixFdImpl for MainThread<'_> {
    fn register_fd(&self, fd: RawFd, flags: FdFlags) -> Result<(), HostError> {
        self.table(|t| t.register_fd(fd, flags).then_some(()))
    }

    fn modify_fd(&self, fd: RawFd, flags: FdFlags) -> Result<(), HostError> {
        self.table(|t| t.modify_fd(fd, flags).then_some(()))
    }

    fn unregister_fd(&self, fd: RawFd) -> Result<(), HostError> {
        self.table(|t| t.unregister_fd(fd).then_some(()))
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
        // Timers and descriptors only off macOS for now: nothing drives the
        // run loop there yet, and a plug-in that chose the host timer would
        // never tick (H0 ruling; H2 decides once the app drives the loop).
        #[cfg(not(target_os = "macos"))]
        builder.register::<clack_extensions::timer::HostTimer>();
        #[cfg(all(unix, not(target_os = "macos")))]
        builder.register::<clack_extensions::posix_fd::HostPosixFd>();
    }
}
