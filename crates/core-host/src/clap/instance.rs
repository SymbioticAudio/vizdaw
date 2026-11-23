//! A CLAP plugin instance on the main thread.

use super::ffi;
use super::handlers::{AudioThread, Flags, MainThread, PluginExts, Shared, VizHost};
use super::processor::ClapProcessor;
use crate::error::HostError;
use crate::info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
use crate::instance::{
    AudioProcessor, InstanceEvents, OfflineRender, ParentView, PluginEditor, PluginInstance,
};
#[cfg(unix)]
use crate::run_loop::FdFlags;
use crate::run_loop::{self, Member, Registrations, Target};
use clack_extensions::audio_ports::{AudioPortFlags, AudioPortInfoBuffer};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize};
use clack_extensions::note_ports::{NoteDialects, NotePortInfoBuffer};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags};
use clack_extensions::render::RenderMode;
use clack_extensions::tail::TailLength;
use clack_extensions::timer::TimerId;
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use core_model::mixer::ParamId;
use std::cell::RefCell;
use std::ffi::CString;
#[cfg(unix)]
use std::os::unix::io::RawFd;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::Ordering;

type Inst = clack_host::plugin::PluginInstance<VizHost>;

/// A loaded CLAP plugin (main-thread half).
pub struct ClapInstance {
    /// Shared (weakly) with the run loop, which calls the plug-in's timer
    /// and descriptor callbacks between the host's own calls. Methods here
    /// `borrow_mut` it for one call; that cannot re-enter (see
    /// `RunLoopTarget::with`).
    ///
    /// Declared before `_member`, so it drops first: the plug-in is
    /// destroyed while it is still in the run loop, and its own
    /// unregister calls from `gui.destroy` and `destroy` are accepted.
    inst: Rc<RefCell<Inst>>,
    /// The instance's place in the run loop, left (and its leftover
    /// registrations dropped) after the plug-in is destroyed.
    _member: Member,
    /// The extensions it declared (read once, while it initialized).
    exts: PluginExts,
    flags: Arc<Flags>,
    info: PluginInfo,
    clap_notes: bool,
    gui_open: bool,
    /// The window scale for the next editor open (physical pixels per
    /// point, [`PluginEditor::set_scale`]).
    scale: f64,
    /// Physical pixels per point of the open editor's sizes: its scale on
    /// a physical-pixel window API (X11), 1 on a logical one (Cocoa).
    gui_scale: f64,
    /// The parameters set a step away and back before a second activation
    /// (plan 6g Task 5): the bundled Multi-Q's continuous, writable ones;
    /// empty for every other plug-in.
    refresh: Vec<ParamId>,
}

impl std::fmt::Debug for ClapInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClapInstance")
            .field("plugin", &self.info.plugin.id)
            .finish_non_exhaustive()
    }
}

fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or(&[])).into_owned()
}

impl ClapInstance {
    /// Creates plugin `id` from the bundle at `path` (main thread).
    pub fn load(path: &Path, id: &str) -> Result<ClapInstance, HostError> {
        let entry = ffi::load_entry(path)?;
        let listed = super::list_bundle(path)?;
        let (plugin, kind) = listed
            .into_iter()
            .find(|(p, _)| p.id == id)
            .ok_or_else(|| HostError::NotFound(format!("{id} in {}", path.display())))?;
        let cid = CString::new(id).map_err(|e| HostError::LoadFailed(e.to_string()))?;
        let host = HostInfo::new(
            "VizDAW",
            "VizDAW",
            "https://github.com/vizdaw",
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|e| HostError::LoadFailed(e.to_string()))?;
        let flags = Arc::new(Flags::default());
        let shared_flags = Arc::clone(&flags);
        let name = plugin.name.clone();
        // The table exists before the plug-in does: some register timers
        // while they initialize or create their editor.
        let table = Rc::new(RefCell::new(Registrations::default()));
        let handler_table = Rc::clone(&table);
        let inst = Inst::new(
            move |_| Shared::new(shared_flags, name),
            move |shared| MainThread::new(shared, handler_table),
            &entry,
            &cid,
            &host,
        )
        .map_err(|e| HostError::LoadFailed(format!("{id}: {e}")))?;
        let exts = inst.access_shared_handler(|s| s.exts.get().copied().unwrap_or_default());
        let inst = Rc::new(RefCell::new(inst));
        let target = Rc::new(RunLoopTarget {
            inst: Rc::downgrade(&inst),
            exts,
        });
        let member = run_loop::join(table, target);
        let mut me = ClapInstance {
            inst,
            _member: member,
            exts,
            flags,
            info: PluginInfo {
                plugin,
                kind,
                inputs: Vec::new(),
                outputs: Vec::new(),
                note_input: false,
                params: Vec::new(),
                latency: 0,
                tail: Tail::None,
            },
            clap_notes: false,
            gui_open: false,
            scale: 1.0,
            gui_scale: 1.0,
            refresh: Vec::new(),
        };
        me.read_info();
        Ok(me)
    }

    fn exts(&self) -> PluginExts {
        self.exts
    }

    /// Reads ports, note ports and parameters (main thread).
    fn read_info(&mut self) {
        let exts = self.exts();
        let mut inst = self.inst.borrow_mut();
        let h = inst.plugin_handle();
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        if let Some(ap) = exts.audio_ports {
            let mut buf = AudioPortInfoBuffer::new();
            for is_input in [true, false] {
                let mut sidechain_taken = false;
                for i in 0..ap.count(&h, is_input) {
                    let Some(p) = ap.get(&h, i, is_input, &mut buf) else {
                        continue;
                    };
                    let role = if p.flags.contains(AudioPortFlags::IS_MAIN) {
                        PortRole::Main
                    } else if is_input && !sidechain_taken {
                        sidechain_taken = true;
                        PortRole::Sidechain
                    } else {
                        PortRole::Aux
                    };
                    let port = AudioPortInfo {
                        name: text(p.name),
                        channels: u16::try_from(p.channel_count).unwrap_or(u16::MAX),
                        role,
                    };
                    if is_input {
                        inputs.push(port);
                    } else {
                        outputs.push(port);
                    }
                }
            }
        }
        let mut note_input = false;
        let mut clap_notes = false;
        if let Some(np) = exts.note_ports {
            if np.count(&h, true) > 0 {
                note_input = true;
                let mut buf = NotePortInfoBuffer::new();
                clap_notes = np
                    .get(&h, 0, true, &mut buf)
                    .is_some_and(|p| p.supported_dialects.contains(NoteDialects::CLAP));
            }
        }
        let mut params = Vec::new();
        let mut refresh = Vec::new();
        let multiq = self.info.plugin.id == crate::bundled::MULTIQ_CLAP_ID;
        if let Some(pe) = exts.params {
            let mut buf = ParamInfoBuffer::new();
            for i in 0..pe.count(&h) {
                if let Some(p) = pe.get_info(&h, i, &mut buf) {
                    if multiq
                        && !p.flags.contains(ParamInfoFlags::IS_STEPPED)
                        && !p.flags.contains(ParamInfoFlags::IS_READONLY)
                        && p.max_value > p.min_value
                    {
                        refresh.push(ParamId(u64::from(p.id.get())));
                    }
                    params.push(ParamInfo {
                        id: ParamId(u64::from(p.id.get())),
                        name: text(p.name),
                        module: text(p.module),
                        min: p.min_value,
                        max: p.max_value,
                        default: p.default_value,
                        stepped: p.flags.contains(ParamInfoFlags::IS_STEPPED),
                        automatable: p.flags.contains(ParamInfoFlags::IS_AUTOMATABLE),
                        hidden: p.flags.contains(ParamInfoFlags::IS_HIDDEN),
                        read_only: p.flags.contains(ParamInfoFlags::IS_READONLY),
                    });
                }
            }
        }
        if note_input && inputs.is_empty() {
            self.info.kind = PluginKind::Instrument;
        }
        self.info.inputs = inputs;
        self.info.outputs = outputs;
        self.info.note_input = note_input;
        self.info.params = params;
        self.refresh = refresh;
        self.clap_notes = clap_notes;
    }
}

/// Runs an instance's timer and descriptor callbacks for the run loop.
struct RunLoopTarget {
    inst: Weak<RefCell<Inst>>,
    exts: PluginExts,
}

impl RunLoopTarget {
    /// Calls `f` with the plug-in, unless it is gone or in a host call.
    ///
    /// No re-entry: the instance is borrowed mutably only by `ClapInstance`'s
    /// own methods and here. A plug-in callback (from either) reaches only
    /// the host handlers, which touch atomics and the registrations table
    /// (`try_borrow_mut`), never this cell; and the app never calls
    /// `run_loop_tick` from inside a `ClapInstance` method. A nested tick is a
    /// no-op, and `try_borrow_mut` skips a busy instance in any case.
    fn with(&self, f: impl FnOnce(&mut Inst)) {
        if let Some(inst) = self.inst.upgrade() {
            if let Ok(mut inst) = inst.try_borrow_mut() {
                f(&mut inst);
            }
        }
    }
}

impl Target for RunLoopTarget {
    fn on_timer(&self, id: u32) {
        if let Some(timer) = self.exts.timer {
            self.with(|inst| timer.on_timer(&inst.plugin_handle(), TimerId(id)));
        }
    }

    #[cfg(unix)]
    fn on_fd(&self, fd: RawFd, flags: FdFlags) {
        if let Some(posix_fd) = self.exts.posix_fd {
            self.with(|inst| posix_fd.on_fd(&inst.plugin_handle(), fd, flags));
        }
    }
}

fn clap_id(id: ParamId) -> Option<ClapId> {
    u32::try_from(id.0).ok().and_then(ClapId::from_raw)
}

impl PluginInstance for ClapInstance {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    fn activate(
        &mut self,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<Box<dyn AudioProcessor>, HostError> {
        // A processor dropped without `deactivate` leaves the plugin active;
        // this succeeds exactly when no processor handle remains.
        if self.inst.borrow().is_active() {
            let _ = self.inst.borrow_mut().try_deactivate();
        }
        if self.inst.borrow().is_active() {
            return Err(HostError::ActivateFailed("already active".into()));
        }
        // Ports may have changed while inactive.
        self.read_info();
        let stopped = self
            .inst
            .borrow_mut()
            .activate(
                |_, _| AudioThread,
                PluginAudioConfiguration {
                    sample_rate,
                    min_frames_count: 1,
                    max_frames_count: max_frames.max(1),
                },
            )
            .map_err(|e| HostError::ActivateFailed(e.to_string()))?;
        let exts = self.exts();
        self.info.latency = exts
            .latency
            .map_or(0, |l| l.get(&self.inst.borrow_mut().plugin_handle()));
        let mut proc = PluginAudioProcessor::from(stopped);
        self.info.tail = match exts.tail.map(|t| t.get(&proc.plugin_handle())) {
            Some(TailLength::Finite(n)) if n > 0 => Tail::Frames(n),
            Some(TailLength::Infinite) => Tail::Infinite,
            _ => Tail::None,
        };
        Ok(Box::new(ClapProcessor::new(
            proc,
            &self.info,
            max_frames.max(1),
            self.clap_notes,
        )))
    }

    fn deactivate(&mut self, processor: Box<dyn AudioProcessor>) {
        // Dropping the processor stops processing (as an audio-thread
        // call); then the plugin is deactivated here.
        drop(processor);
        let _ = self.inst.borrow_mut().try_deactivate();
    }

    fn is_active(&self) -> bool {
        self.inst.borrow().is_active()
    }

    fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        let state = self
            .exts()
            .state
            .ok_or_else(|| HostError::StateRejected("the plugin has no state".into()))?;
        let mut out = Vec::new();
        state
            .save(&self.inst.borrow_mut().plugin_handle(), &mut out)
            .map_err(|e| HostError::StateRejected(e.to_string()))?;
        Ok(out)
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), HostError> {
        let state = self
            .exts()
            .state
            .ok_or_else(|| HostError::StateRejected("the plugin has no state".into()))?;
        let mut reader = data;
        state
            .load(&self.inst.borrow_mut().plugin_handle(), &mut reader)
            .map_err(|e| HostError::StateRejected(e.to_string()))
    }

    fn set_params(&mut self, values: &[(ParamId, f64)]) -> Result<(), HostError> {
        let Some(params) = self.exts().params else {
            return Ok(());
        };
        let mut events = EventBuffer::with_capacity(values.len());
        for (id, v) in values {
            if let Some(cid) = clap_id(*id) {
                events.push(&ParamValueEvent::new(0, cid, Pckn::match_all(), *v));
            }
        }
        let mut out = EventBuffer::new();
        let mut inst = self.inst.borrow_mut();
        let mut h = inst.inactive_plugin_handle().ok_or_else(|| {
            HostError::StateRejected(
                "parameters of an active plugin go through its processor".into(),
            )
        })?;
        params.flush(&mut h, &events.as_input(), &mut out.as_output());
        Ok(())
    }

    fn values_to_refresh(&self) -> Vec<ParamId> {
        self.refresh.clone()
    }

    fn param_value(&mut self, id: ParamId) -> Option<f64> {
        let params = self.exts().params?;
        params.get_value(&self.inst.borrow_mut().plugin_handle(), clap_id(id)?)
    }

    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        let gui = self.exts().gui?;
        let cfg = GuiConfiguration {
            api_type: GuiApiType::default_for_current_platform()?,
            is_floating: false,
        };
        if !gui.is_api_supported(&self.inst.borrow_mut().plugin_handle(), cfg) {
            return None;
        }
        Some(self)
    }

    fn idle(&mut self) -> InstanceEvents {
        if self.inst.borrow().is_active() {
            // Reclaims a plugin whose processor was dropped; fails
            // harmlessly while one is still out.
            let _ = self.inst.borrow_mut().try_deactivate();
        }
        if Flags::take(&self.flags.callback) {
            self.inst.borrow_mut().call_on_main_thread_callback();
        }
        let ports = Flags::take(&self.flags.ports);
        if ports && !self.inst.borrow().is_active() {
            self.read_info();
        }
        if Flags::take(&self.flags.gui_closed) {
            self.gui_open = false;
            self.flags.gui_resizable.store(false, Ordering::Release);
        }
        let editor_resize = self.editor_resize_request();
        InstanceEvents {
            restart: Flags::take(&self.flags.restart),
            latency_changed: Flags::take(&self.flags.latency),
            params_changed: Flags::take(&self.flags.params) || ports,
            state_dirty: Flags::take(&self.flags.state_dirty),
            editor_resize,
        }
    }

    fn set_offline(&mut self, on: bool) -> OfflineRender {
        let Some(render) = self.exts().render else {
            return OfflineRender::NoOfflineMode;
        };
        let mut inst = self.inst.borrow_mut();
        let h = inst.plugin_handle();
        // A plugin with a hard real-time requirement (a hardware proxy)
        // renders in real time whatever it is told (pre-flight M3).
        if on && render.has_realtime_requirement(&h) {
            return OfflineRender::RealtimeRequired;
        }
        let mode = if on {
            RenderMode::Offline
        } else {
            RenderMode::Realtime
        };
        match render.set(&h, mode) {
            Ok(()) => OfflineRender::Taken,
            Err(_) => OfflineRender::Refused,
        }
    }
}

/// The window API CLAP editors embed with here: Cocoa on macOS, X11 on
/// Linux (under Wayland through XWayland; CLAP has no embedded Wayland
/// API). `None` elsewhere.
pub fn editor_api() -> Option<GuiApiType<'static>> {
    if cfg!(target_os = "macos") {
        Some(GuiApiType::COCOA)
    } else if cfg!(target_os = "linux") {
        Some(GuiApiType::X11)
    } else {
        None
    }
}

/// A window scale, or 1 when it is not a usable one.
fn usable_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// An editor size in the API's units as logical points, rounded up so the
/// window never clips it.
fn to_points(size: GuiSize, scale: f64) -> (u32, u32) {
    let f = |v: u32| ((f64::from(v) / scale).ceil() as u32).max(1);
    (f(size.width), f(size.height))
}

/// Logical points as a size in the API's units.
fn from_points(width: u32, height: u32, scale: f64) -> GuiSize {
    let f = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
    GuiSize {
        width: f(width),
        height: f(height),
    }
}

impl ClapInstance {
    /// Applies the editor's last `request_resize` (main thread, from
    /// `idle`): only while it is open and can resize, clamped through its
    /// `adjust_size`, then set; returns the size it took in points. A
    /// refused request changes nothing.
    fn editor_resize_request(&mut self) -> Option<(u32, u32)> {
        let hints = Flags::take(&self.flags.gui_hints);
        let want = self.flags.take_resize();
        if !self.gui_open || (want.is_none() && !hints) {
            return None;
        }
        let gui = self.exts().gui?;
        let mut inst = self.inst.borrow_mut();
        let h = inst.plugin_handle();
        let resizable = gui.can_resize(&h);
        self.flags.gui_resizable.store(resizable, Ordering::Release);
        let want = want.filter(|_| resizable)?;
        let size = gui.adjust_size(&h, want).unwrap_or(want);
        gui.set_size(&h, size).ok()?;
        Some(to_points(size, self.gui_scale))
    }

    /// Hides and destroys the open editor.
    fn destroy_gui(&mut self, gui: &clack_extensions::gui::PluginGui, h: &PluginMainThreadHandle) {
        gui.hide(h).ok();
        gui.destroy(h);
        self.gui_open = false;
        self.flags.gui_resizable.store(false, Ordering::Release);
        self.flags.gui_resize.store(0, Ordering::Release);
    }
}

impl PluginEditor for ClapInstance {
    fn set_scale(&mut self, scale: f64) {
        self.scale = usable_scale(scale);
    }

    fn open(&mut self, parent: ParentView) -> Result<(u32, u32), HostError> {
        let gui = self
            .exts()
            .gui
            .ok_or_else(|| HostError::EditorUnavailable("no GUI".into()))?;
        let api = editor_api()
            .ok_or_else(|| HostError::EditorUnavailable("no embedding API here".into()))?;
        let inst = Rc::clone(&self.inst);
        let mut inst = inst.borrow_mut();
        let h = inst.plugin_handle();
        if self.gui_open {
            self.destroy_gui(&gui, &h);
        }
        // Nothing from an earlier editor carries over.
        self.flags.gui_resize.store(0, Ordering::Release);
        self.flags.gui_hints.store(false, Ordering::Release);
        gui.create(
            &h,
            GuiConfiguration {
                api_type: api,
                is_floating: false,
            },
        )
        .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))?;
        self.gui_open = true;
        // CLAP gui order: create, set_scale, can_resize, size, set_parent,
        // show. A window API in points (Cocoa) gets no scale; a plug-in
        // that measures the screen itself may decline it, and its pixels
        // are still the window's physical pixels.
        self.gui_scale = if api.uses_logical_size() {
            1.0
        } else {
            let _ = gui.set_scale(&h, self.scale);
            self.scale
        };
        self.flags
            .gui_resizable
            .store(gui.can_resize(&h), Ordering::Release);
        if let Err(e) = ffi::set_native_parent(&gui, &h, parent.0) {
            self.destroy_gui(&gui, &h);
            return Err(e);
        }
        if let Err(e) = gui.show(&h) {
            self.destroy_gui(&gui, &h);
            return Err(HostError::EditorUnavailable(format!("{e:?}")));
        }
        let size = gui
            .get_size(&h)
            .map_or((400, 300), |s| to_points(s, self.gui_scale));
        Ok(size)
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(u32, u32), HostError> {
        let gui = self
            .exts()
            .gui
            .ok_or_else(|| HostError::EditorUnavailable("no GUI".into()))?;
        let mut inst = self.inst.borrow_mut();
        let h = inst.plugin_handle();
        let asked = from_points(width, height, self.gui_scale);
        let want = gui.adjust_size(&h, asked).unwrap_or(asked);
        gui.set_size(&h, want)
            .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))?;
        Ok(to_points(want, self.gui_scale))
    }

    fn can_resize(&mut self) -> bool {
        match (self.exts().gui, self.gui_open) {
            (Some(gui), true) => gui.can_resize(&self.inst.borrow_mut().plugin_handle()),
            _ => false,
        }
    }

    fn close(&mut self) {
        if !self.gui_open {
            return;
        }
        match self.exts().gui {
            Some(gui) => {
                let inst = Rc::clone(&self.inst);
                let mut inst = inst.borrow_mut();
                self.destroy_gui(&gui, &inst.plugin_handle());
            }
            None => self.gui_open = false,
        }
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        // Editor closed and plug-in deactivated while it is still in the
        // run loop; then the fields drop: `inst` (the plug-in's destroy,
        // whose unregister calls are still accepted) before `_member`
        // (leaving the loop, which drops any registrations it kept).
        PluginEditor::close(self);
        let _ = self.inst.borrow_mut().try_deactivate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_convert_between_physical_pixels_and_points() {
        let px = |width, height| GuiSize { width, height };
        assert_eq!(to_points(px(600, 400), 2.0), (300, 200));
        assert_eq!(to_points(px(601, 401), 2.0), (301, 201), "rounded up");
        assert_eq!(to_points(px(450, 300), 1.5), (300, 200));
        assert_eq!(to_points(px(1, 1), 4.0), (1, 1), "never empty");
        assert_eq!(to_points(px(640, 480), 1.0), (640, 480));
        assert_eq!(from_points(300, 200, 2.0), px(600, 400));
        assert_eq!(from_points(301, 201, 1.25), px(376, 251));
        assert_eq!(from_points(0, 0, 2.0), px(1, 1));
    }

    #[test]
    fn unusable_scales_count_as_one() {
        assert_eq!(usable_scale(2.0), 2.0);
        assert_eq!(usable_scale(1.25), 1.25);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(usable_scale(bad), 1.0, "{bad}");
        }
    }
}
