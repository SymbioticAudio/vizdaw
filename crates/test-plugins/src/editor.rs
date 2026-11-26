//! The timer kind's headless editor (CLAP `gui`, embedded Cocoa or X11): it
//! draws nothing and never touches the parent window, so host tests run
//! without a display. It records the calls it gets (the scale, whether the
//! scale came before the parent, its size in the API's units), can ask the
//! host to resize it, and can unregister a timer while its editor or the
//! plug-in itself is destroyed. What happens during destroy is appended to
//! `<bundle path>.log`, since nothing can be read back from a destroyed
//! plug-in.
//!
//! The GUI kind's real X11 editor is in `gui.rs`; on Linux each callback
//! below hands over to it by kind.

use crate::plugin::MainThread;
use crate::{
    PARAM_DESTROY_UNREGISTER, PARAM_GUI_DESTROY_UNREGISTER, PARAM_GUI_HEIGHT, PARAM_GUI_OPEN,
    PARAM_GUI_REFUSE_SET_SIZE, PARAM_GUI_REQUEST_RESIZE, PARAM_GUI_RESIZABLE, PARAM_GUI_SCALE,
    PARAM_GUI_SCALED_FIRST, PARAM_GUI_SET_SIZES, PARAM_GUI_WIDTH,
};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, HostGui, PluginGuiImpl, Window,
};
use clack_plugin::prelude::PluginError;
use std::io::Write as _;
use std::path::Path;

/// The editor's size at scale 1, in points.
pub const BASE_SIZE: (u32, u32) = (300, 200);
/// The smallest size `adjust_size` allows, in the API's units.
pub const MIN_SIZE: (u32, u32) = (100, 100);
/// The largest size `adjust_size` allows, in the API's units.
pub const MAX_SIZE: (u32, u32) = (2000, 1600);

/// What the editor has seen (main thread only).
#[derive(Debug)]
pub struct Editor {
    created: bool,
    resizable: bool,
    /// The last `set_scale`, or 0 when none came.
    scale: f64,
    /// A `set_scale` came since the last `create`.
    scaled: bool,
    /// At the last `set_parent`: 1 if a scale had come first, 0 if not;
    /// −1 before any.
    scaled_first: f64,
    size: GuiSize,
    /// Its next `set_size` fails.
    refuse_set_size: bool,
    /// `set_size` calls so far.
    set_sizes: u32,
    /// Timers to unregister in `gui.destroy` and in the plug-in's destroy.
    gui_destroy_unregister: Option<u32>,
    destroy_unregister: Option<u32>,
}

impl Default for Editor {
    fn default() -> Editor {
        Editor {
            created: false,
            resizable: false,
            scale: 0.0,
            scaled: false,
            scaled_first: -1.0,
            size: GuiSize {
                width: BASE_SIZE.0,
                height: BASE_SIZE.1,
            },
            refuse_set_size: false,
            set_sizes: 0,
            gui_destroy_unregister: None,
            destroy_unregister: None,
        }
    }
}

fn supported(cfg: &GuiConfiguration) -> bool {
    !cfg.is_floating && (cfg.api_type == GuiApiType::COCOA || cfg.api_type == GuiApiType::X11)
}

/// Appends `line` to `<bundle>.log`, when the bundle path is known.
pub(crate) fn log(bundle: Option<&Path>, line: &str) {
    let Some(bundle) = bundle else {
        return;
    };
    let mut path = bundle.as_os_str().to_owned();
    path.push(".log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

impl MainThread<'_> {
    /// Applies an editor switch; returns its result for the readout, or
    /// `None` for switches that have none.
    pub(crate) fn editor_command(&self, id: u32, v: u32) -> Option<f64> {
        let mut e = self.editor.borrow_mut();
        match id {
            PARAM_GUI_RESIZABLE => e.resizable = v != 0,
            PARAM_GUI_REFUSE_SET_SIZE => e.refuse_set_size = v != 0,
            PARAM_GUI_DESTROY_UNREGISTER => e.gui_destroy_unregister = Some(v),
            PARAM_DESTROY_UNREGISTER => e.destroy_unregister = Some(v),
            PARAM_GUI_REQUEST_RESIZE => {
                drop(e);
                let (width, height) = (v >> 16, v & 0xFFFF);
                let ok = self
                    .host
                    .get_extension::<HostGui>()
                    .is_some_and(|g| g.request_resize(&self.host, width, height).is_ok());
                if ok {
                    self.editor.borrow_mut().size = GuiSize { width, height };
                }
                return Some(f64::from(u8::from(ok)));
            }
            _ => {}
        }
        None
    }

    /// The editor readouts.
    pub(crate) fn editor_readout(&self, id: u32) -> Option<f64> {
        let e = self.editor.borrow();
        Some(match id {
            PARAM_GUI_SCALE => e.scale,
            PARAM_GUI_SCALED_FIRST => e.scaled_first,
            PARAM_GUI_WIDTH => f64::from(e.size.width),
            PARAM_GUI_HEIGHT => f64::from(e.size.height),
            PARAM_GUI_OPEN => f64::from(u8::from(e.created)),
            PARAM_GUI_SET_SIZES => f64::from(e.set_sizes),
            _ => return None,
        })
    }

    /// The plug-in is being destroyed: unregisters the timer it was told
    /// to and logs the result.
    pub(crate) fn editor_destroying(&self) {
        let id = self.editor.borrow_mut().destroy_unregister.take();
        if let Some(id) = id {
            let ok = self.unregister_timer(id);
            log(
                self.bundle(),
                &format!("destroy unregister {id}: {}", u8::from(ok)),
            );
        }
    }
}

impl PluginGuiImpl for MainThread<'_> {
    fn is_api_supported(&self, configuration: GuiConfiguration) -> bool {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return self.gui_supported(&configuration);
        }
        supported(&configuration)
    }

    fn get_preferred_api(&self) -> Option<GuiConfiguration<'_>> {
        Some(GuiConfiguration {
            // X11 on Linux, so the GUI kind's choice is right too.
            api_type: GuiApiType::default_for_current_platform()?,
            is_floating: false,
        })
    }

    fn create(&self, configuration: GuiConfiguration) -> Result<(), PluginError> {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return self.gui_create(&configuration);
        }
        if !supported(&configuration) {
            return Err(PluginError::Message("unsupported window API"));
        }
        let mut e = self.editor.borrow_mut();
        e.created = true;
        e.scaled = false;
        Ok(())
    }

    fn destroy(&self) {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return self.gui_destroy();
        }
        let id = {
            let mut e = self.editor.borrow_mut();
            e.created = false;
            e.gui_destroy_unregister.take()
        };
        if let Some(id) = id {
            let ok = self.unregister_timer(id);
            log(
                self.bundle(),
                &format!("gui.destroy unregister {id}: {}", u8::from(ok)),
            );
        }
    }

    fn set_scale(&self, scale: f64) -> Result<(), PluginError> {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return self.gui_set_scale(scale);
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(PluginError::Message("bad scale"));
        }
        let mut e = self.editor.borrow_mut();
        let old = if e.scale > 0.0 { e.scale } else { 1.0 };
        // Physical pixels follow the scale, as a real editor's would.
        e.size = GuiSize {
            width: (f64::from(e.size.width) * scale / old).round() as u32,
            height: (f64::from(e.size.height) * scale / old).round() as u32,
        };
        e.scale = scale;
        e.scaled = true;
        Ok(())
    }

    fn get_size(&self) -> Option<GuiSize> {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return self.gui_get_size();
        }
        Some(self.editor.borrow().size)
    }

    fn can_resize(&self) -> bool {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return false;
        }
        self.editor.borrow().resizable
    }

    fn adjust_size(&self, size: GuiSize) -> Option<GuiSize> {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return None;
        }
        if !self.editor.borrow().resizable {
            return None;
        }
        Some(GuiSize {
            width: size.width.clamp(MIN_SIZE.0, MAX_SIZE.0),
            height: size.height.clamp(MIN_SIZE.1, MAX_SIZE.1),
        })
    }

    fn set_size(&self, size: GuiSize) -> Result<(), PluginError> {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return Err(PluginError::Message("fixed size"));
        }
        let mut e = self.editor.borrow_mut();
        e.set_sizes += 1;
        if std::mem::take(&mut e.refuse_set_size) {
            return Err(PluginError::Message("refused"));
        }
        if !e.resizable {
            return Err(PluginError::Message("fixed size"));
        }
        e.size = size;
        Ok(())
    }

    fn set_parent(&self, _window: Window) -> Result<(), PluginError> {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return self.gui_set_parent(_window);
        }
        // Headless: the handle is never used.
        let mut e = self.editor.borrow_mut();
        e.scaled_first = f64::from(u8::from(e.scaled));
        Ok(())
    }

    fn set_transient(&self, _window: Window) -> Result<(), PluginError> {
        Err(PluginError::Message("embedded only"))
    }

    fn show(&self) -> Result<(), PluginError> {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return self.gui_show(true);
        }
        Ok(())
    }

    fn hide(&self) -> Result<(), PluginError> {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            return self.gui_show(false);
        }
        Ok(())
    }
}
