//! A CLAP plugin instance on the main thread.

use super::ffi;
use super::handlers::{AudioThread, Flags, MainThread, PluginExts, Shared, VizHost};
use super::processor::ClapProcessor;
use crate::error::HostError;
use crate::info::{AudioPortInfo, ParamInfo, PluginInfo, PluginKind, PortRole, Tail};
use crate::instance::{AudioProcessor, InstanceEvents, ParentView, PluginEditor, PluginInstance};
use clack_extensions::audio_ports::{AudioPortFlags, AudioPortInfoBuffer};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, GuiSize};
use clack_extensions::note_ports::{NoteDialects, NotePortInfoBuffer};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags};
use clack_extensions::tail::TailLength;
use clack_host::events::event_types::ParamValueEvent;
use clack_host::prelude::*;
use core_model::mixer::ParamId;
use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;

/// A loaded CLAP plugin (main-thread half).
pub struct ClapInstance {
    inst: clack_host::plugin::PluginInstance<VizHost>,
    flags: Arc<Flags>,
    info: PluginInfo,
    clap_notes: bool,
    gui_open: bool,
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
        let inst = clack_host::plugin::PluginInstance::<VizHost>::new(
            move |_| Shared::new(shared_flags, name),
            |shared| MainThread::new(shared),
            &entry,
            &cid,
            &host,
        )
        .map_err(|e| HostError::LoadFailed(format!("{id}: {e}")))?;
        let mut me = ClapInstance {
            inst,
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
        };
        me.read_info();
        Ok(me)
    }

    fn exts(&self) -> PluginExts {
        self.inst
            .access_shared_handler(|s| s.exts.get().copied().unwrap_or_default())
    }

    /// Reads ports, note ports and parameters (main thread).
    fn read_info(&mut self) {
        let exts = self.exts();
        let h = self.inst.plugin_handle();
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
        if let Some(pe) = exts.params {
            let mut buf = ParamInfoBuffer::new();
            for i in 0..pe.count(&h) {
                if let Some(p) = pe.get_info(&h, i, &mut buf) {
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
        self.clap_notes = clap_notes;
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
        if self.inst.is_active() {
            let _ = self.inst.try_deactivate();
        }
        if self.inst.is_active() {
            return Err(HostError::ActivateFailed("already active".into()));
        }
        // Ports may have changed while inactive.
        self.read_info();
        let stopped = self
            .inst
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
            .map_or(0, |l| l.get(&self.inst.plugin_handle()));
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
        let _ = self.inst.try_deactivate();
    }

    fn is_active(&self) -> bool {
        self.inst.is_active()
    }

    fn save_state(&mut self) -> Result<Vec<u8>, HostError> {
        let state = self
            .exts()
            .state
            .ok_or_else(|| HostError::StateRejected("the plugin has no state".into()))?;
        let mut out = Vec::new();
        state
            .save(&self.inst.plugin_handle(), &mut out)
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
            .load(&self.inst.plugin_handle(), &mut reader)
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
        let mut h = self.inst.inactive_plugin_handle().ok_or_else(|| {
            HostError::StateRejected(
                "parameters of an active plugin go through its processor".into(),
            )
        })?;
        params.flush(&mut h, &events.as_input(), &mut out.as_output());
        Ok(())
    }

    fn param_value(&mut self, id: ParamId) -> Option<f64> {
        let params = self.exts().params?;
        params.get_value(&self.inst.plugin_handle(), clap_id(id)?)
    }

    fn editor(&mut self) -> Option<&mut dyn PluginEditor> {
        let gui = self.exts().gui?;
        let cfg = GuiConfiguration {
            api_type: GuiApiType::default_for_current_platform()?,
            is_floating: false,
        };
        if !gui.is_api_supported(&self.inst.plugin_handle(), cfg) {
            return None;
        }
        Some(self)
    }

    fn idle(&mut self) -> InstanceEvents {
        if self.inst.is_active() {
            // Reclaims a plugin whose processor was dropped; fails
            // harmlessly while one is still out.
            let _ = self.inst.try_deactivate();
        }
        if Flags::take(&self.flags.callback) {
            self.inst.call_on_main_thread_callback();
        }
        let ports = Flags::take(&self.flags.ports);
        if ports && !self.inst.is_active() {
            self.read_info();
        }
        if Flags::take(&self.flags.gui_closed) {
            self.gui_open = false;
        }
        InstanceEvents {
            restart: Flags::take(&self.flags.restart),
            latency_changed: Flags::take(&self.flags.latency),
            params_changed: Flags::take(&self.flags.params) || ports,
            state_dirty: Flags::take(&self.flags.state_dirty),
        }
    }
}

impl PluginEditor for ClapInstance {
    fn open(&mut self, parent: ParentView) -> Result<(u32, u32), HostError> {
        let gui = self
            .exts()
            .gui
            .ok_or_else(|| HostError::EditorUnavailable("no GUI".into()))?;
        let api = GuiApiType::default_for_current_platform()
            .filter(|a| *a == GuiApiType::COCOA)
            .ok_or_else(|| HostError::EditorUnavailable("embedding needs Cocoa".into()))?;
        let h = self.inst.plugin_handle();
        if self.gui_open {
            gui.hide(&h).ok();
            gui.destroy(&h);
            self.gui_open = false;
        }
        gui.create(
            &h,
            GuiConfiguration {
                api_type: api,
                is_floating: false,
            },
        )
        .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))?;
        self.gui_open = true;
        if let Err(e) = ffi::set_cocoa_parent(&gui, &h, parent.0) {
            gui.destroy(&h);
            self.gui_open = false;
            return Err(e);
        }
        if let Err(e) = gui.show(&h) {
            gui.destroy(&h);
            self.gui_open = false;
            return Err(HostError::EditorUnavailable(format!("{e:?}")));
        }
        let size = gui.get_size(&h).unwrap_or(GuiSize {
            width: 400,
            height: 300,
        });
        Ok((size.width, size.height))
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(u32, u32), HostError> {
        let gui = self
            .exts()
            .gui
            .ok_or_else(|| HostError::EditorUnavailable("no GUI".into()))?;
        let h = self.inst.plugin_handle();
        let want = gui
            .adjust_size(&h, GuiSize { width, height })
            .unwrap_or(GuiSize { width, height });
        gui.set_size(&h, want)
            .map_err(|e| HostError::EditorUnavailable(format!("{e:?}")))?;
        Ok((want.width, want.height))
    }

    fn can_resize(&mut self) -> bool {
        match (self.exts().gui, self.gui_open) {
            (Some(gui), true) => gui.can_resize(&self.inst.plugin_handle()),
            _ => false,
        }
    }

    fn close(&mut self) {
        if !self.gui_open {
            return;
        }
        if let Some(gui) = self.exts().gui {
            let h = self.inst.plugin_handle();
            gui.hide(&h).ok();
            gui.destroy(&h);
        }
        self.gui_open = false;
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        PluginEditor::close(self);
        let _ = self.inst.try_deactivate();
    }
}
