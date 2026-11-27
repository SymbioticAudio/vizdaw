//! The app's plug-in settings (Swift `PluginSettings` and
//! `PluginSettingsCommands`): "Put an EQ on new channels", which EQ that
//! is, and "Install Multi-Q for other apps". They belong to the machine,
//! so they live in the preferences (`newChannelEq` and
//! `newChannelEqPlugin`), are pushed to the core at launch, and on every
//! change go to the core first and are saved after. Decoding is tolerant:
//! a damaged choice is Multi-Q and its key is removed. A stored VST3
//! choice is kept (Swift dropped it).

use std::path::Path;

use core_control::registry::ArgValue;
use core_control::{ControlError, ErrorCode};
use serde_json::{Value, json};
use vizdaw_ffi::VizError;
use vizdaw_ffi::bundled::DefaultEqRow;
use vizdaw_ffi::catalog::{CatalogRowFfi, PluginKindTag, PluginStatusRow};
use vizdaw_ffi::snapshot::PluginFormatTag;

use crate::alerts::error_message;
use crate::commands::Ran;
use crate::prefs::Prefs;
use crate::session::CoreCalls;

/// The preferences key of the switch (Swift "VizDAW.newChannelEq").
pub const NEW_CHANNEL_EQ_KEY: &str = "newChannelEq";
/// The preferences key of the chosen EQ: `{"format","id","name"}`; absent
/// is Multi-Q.
pub const EQ_KEY: &str = "newChannelEqPlugin";
/// Multi-Q's own CLAP ID, never offered as a second item.
pub const MULTIQ_CLAP_ID: &str = "com.dusk-audio.multi-q";
/// The line shown, and the install's answer, when the build has no EQ.
pub const NO_BUNDLED_EQ: &str = "This build of VizDAW has no bundled EQ.";
/// The core's sentence for a build without the AU.
pub const NOT_IN_THIS_BUILD: &str = "Multi-Q is not in this build of VizDAW.";
/// The commands `run_plugins` handles.
pub const PLUGIN_COMMANDS: &[&str] = &[
    "plugins.new_channel_eq",
    "plugins.install_multiq",
    "plugins.default_eq",
];

/// What [`PluginPrefs::read`] found: the installed effects and the
/// core's note.
#[derive(Debug, Clone, Default)]
pub struct EqRead {
    pub choices: Vec<DefaultEqRow>,
    pub note: Option<String>,
}

/// What the Install button's call came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Installed; the bundle's path.
    Installed(String),
    /// Something is in the way: ask before replacing it.
    AskReplace,
    /// Why it failed (also the `status`).
    Failed(String),
}

/// The plug-in settings the app keeps.
#[derive(Debug, Clone)]
pub struct PluginPrefs {
    new_channel_eq: bool,
    default_eq: Option<DefaultEqRow>,
    eq_choices: Vec<DefaultEqRow>,
    note: Option<String>,
    status: Option<String>,
    bundled_eq_name: Option<String>,
    bundled_au_present: bool,
    install_destination: Option<String>,
    problem: Option<String>,
}

/// The format's name in the stored choice and the command's prefix.
pub fn format_name(f: PluginFormatTag) -> &'static str {
    match f {
        PluginFormatTag::Au => "au",
        PluginFormatTag::Clap => "clap",
        PluginFormatTag::Vst3 => "vst3",
    }
}

fn format_of(name: &str) -> Option<PluginFormatTag> {
    match name {
        "au" => Some(PluginFormatTag::Au),
        "clap" => Some(PluginFormatTag::Clap),
        "vst3" => Some(PluginFormatTag::Vst3),
        _ => None,
    }
}

/// The stored choice in `v`; any other shape is `None`.
fn read_eq(v: &Value) -> Option<DefaultEqRow> {
    let text = |k: &str| v.get(k)?.as_str().map(str::to_owned);
    let (id, name) = (text("id")?, text("name")?);
    if id.is_empty() || id.contains('\0') || name.contains('\0') {
        return None;
    }
    Some(DefaultEqRow {
        format: format_of(&text("format")?)?,
        id,
        name,
    })
}

/// The picker's items from the catalog's rows: usable (ok or not yet
/// validated), enabled effects with an ID, Multi-Q's CLAP left out.
pub fn eq_choices_from(rows: &[CatalogRowFfi]) -> Vec<DefaultEqRow> {
    rows.iter()
        .filter(|r| {
            r.kind == Some(PluginKindTag::Effect)
                && r.enabled
                && !r.plugin.id.is_empty()
                && matches!(
                    r.status,
                    PluginStatusRow::Ok | PluginStatusRow::NotValidated
                )
                && !(r.plugin.format == PluginFormatTag::Clap && r.plugin.id == MULTIQ_CLAP_ID)
        })
        .map(|r| DefaultEqRow {
            format: r.plugin.format,
            id: r.plugin.id.clone(),
            name: r.plugin.name.clone(),
        })
        .collect()
}

impl Default for PluginPrefs {
    /// Before the preferences are read: the switch on, Multi-Q, nothing
    /// pushed.
    fn default() -> Self {
        PluginPrefs {
            new_channel_eq: true,
            default_eq: None,
            eq_choices: Vec::new(),
            note: None,
            status: None,
            bundled_eq_name: None,
            bundled_au_present: false,
            install_destination: None,
            problem: None,
        }
    }
}

impl PluginPrefs {
    /// The settings in `prefs`, pushed to the core now (call it before any
    /// document opens). A damaged choice is Multi-Q and is removed from
    /// `prefs`; a failed removal leaves it for the next launch.
    pub fn load(prefs: &mut Prefs, core: &dyn CoreCalls) -> PluginPrefs {
        let new_channel_eq = prefs
            .raw(NEW_CHANNEL_EQ_KEY)
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let default_eq = match prefs.raw(EQ_KEY) {
            None => None,
            Some(v) => {
                let row = read_eq(v);
                if row.is_none() {
                    let _ = prefs.remove_raw(EQ_KEY);
                }
                row
            }
        };
        core.set_new_channel_eq(new_channel_eq);
        core.set_default_eq(default_eq.clone());
        PluginPrefs {
            new_channel_eq,
            default_eq,
            eq_choices: Vec::new(),
            note: None,
            status: None,
            bundled_eq_name: core.bundled_eq_name(),
            bundled_au_present: core.bundled_au_present(),
            install_destination: None,
            problem: None,
        }
    }

    /// Where installs go (`None`, the app's: the user's Components
    /// folder); tests give a temporary folder.
    pub fn set_install_destination(&mut self, dest: Option<String>) {
        self.install_destination = dest;
    }

    /// Why the last save failed (the core has the change for this run).
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn new_channel_eq(&self) -> bool {
        self.new_channel_eq
    }

    pub fn default_eq(&self) -> Option<&DefaultEqRow> {
        self.default_eq.as_ref()
    }

    /// The name the caption gives the EQ new channels get.
    pub fn default_eq_name(&self) -> &str {
        self.default_eq.as_ref().map_or("Multi-Q", |c| &c.name)
    }

    /// The installed effects the picker offers, as last read by `refresh`.
    pub fn eq_choices(&self) -> &[DefaultEqRow] {
        &self.eq_choices
    }

    /// The choice is Multi-Q, or one of the `eq_choices` (by format and ID).
    pub fn default_eq_available(&self) -> bool {
        self.default_eq.as_ref().is_none_or(|c| {
            self.eq_choices
                .iter()
                .any(|r| r.format == c.format && r.id == c.id)
        })
    }

    /// The core's line under the picker (the fallback), as last read.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// "Installed in <folder>." or why the install failed.
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn bundled_eq_name(&self) -> Option<&str> {
        self.bundled_eq_name.as_deref()
    }

    pub fn bundled_au_present(&self) -> bool {
        self.bundled_au_present
    }

    /// Turns the switch on or off: the core takes it, it is shown and the
    /// core's note is read again; then it is saved (a failed write is the
    /// error, and the core keeps the value for this run).
    pub fn set_new_channel_eq(
        &mut self,
        on: bool,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        core.set_new_channel_eq(on);
        self.new_channel_eq = on;
        self.note = core.default_eq_note();
        let saved = prefs.set_raw(NEW_CHANNEL_EQ_KEY, Value::Bool(on));
        self.problem = saved.clone().err();
        saved
    }

    /// Chooses the EQ new channels get (`None`: Multi-Q, which stores
    /// nothing): the core takes it, then it is saved.
    pub fn set_default_eq(
        &mut self,
        choice: Option<DefaultEqRow>,
        prefs: &mut Prefs,
        core: &dyn CoreCalls,
    ) -> Result<(), String> {
        core.set_default_eq(choice.clone());
        self.note = core.default_eq_note();
        let saved = match &choice {
            Some(c) => prefs.set_raw(
                EQ_KEY,
                json!({"format": format_name(c.format), "id": c.id, "name": c.name}),
            ),
            None => prefs.remove_raw(EQ_KEY),
        };
        self.default_eq = choice;
        self.problem = saved.clone().err();
        saved
    }

    /// Reads the installed effects and the core's note again (the tab
    /// appearing, a command about to match against them). It reads the
    /// catalogue: a UI thread calls [`PluginPrefs::read`] elsewhere and
    /// [`PluginPrefs::apply_read`] itself.
    pub fn refresh(&mut self, core: &dyn CoreCalls) {
        self.apply_read(Self::read(core));
    }

    /// The read half of [`PluginPrefs::refresh`]; it may block.
    pub fn read(core: &dyn CoreCalls) -> EqRead {
        EqRead {
            choices: core.eq_choices(),
            note: core.default_eq_note(),
        }
    }

    /// The other half: takes what [`PluginPrefs::read`] found.
    pub fn apply_read(&mut self, read: EqRead) {
        self.eq_choices = read.choices;
        self.note = read.note;
    }

    /// Where installs go (`None`: the user's Components folder).
    pub fn install_destination(&self) -> Option<&str> {
        self.install_destination.as_deref()
    }

    /// Whether the Install button can do anything: the build has the
    /// bundled EQ and its AU.
    pub fn can_install(&self) -> bool {
        self.bundled_eq_name.is_some() && self.bundled_au_present
    }

    /// The end of an install the core call of which ran elsewhere (it
    /// copies a bundle, so it may block): sets `status` as
    /// [`PluginPrefs::install`] does.
    pub fn finish_install(&mut self, result: Result<String, VizError>) -> InstallOutcome {
        match self.record_install(result) {
            Ok(path) => InstallOutcome::Installed(path),
            Err(VizError::Exists { .. }) => {
                self.status = None;
                InstallOutcome::AskReplace
            }
            Err(e) => {
                let m = error_message(&e);
                self.status = Some(m.clone());
                InstallOutcome::Failed(m)
            }
        }
    }

    fn record_install(&mut self, result: Result<String, VizError>) -> Result<String, VizError> {
        let path = result?;
        let folder = Path::new(&path)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.status = Some(format!("Installed in {folder}."));
        Ok(path)
    }

    /// The Install button (and the Replace answer): something in the way
    /// is `AskReplace`; any other failure lands in `status`.
    pub fn install(&mut self, replace: bool, core: &dyn CoreCalls) -> InstallOutcome {
        if self.bundled_eq_name.is_none() {
            self.status = Some(NO_BUNDLED_EQ.into());
            return InstallOutcome::Failed(NO_BUNDLED_EQ.into());
        }
        let result = core.install_multiq(self.install_destination.clone(), replace);
        self.finish_install(result)
    }

    /// Installs into the destination and sets `status`; a failure is
    /// returned and asks nothing. Ok: the installed bundle's path.
    pub fn install_quietly(
        &mut self,
        replace: bool,
        core: &dyn CoreCalls,
    ) -> Result<String, VizError> {
        let result = core.install_multiq(self.install_destination.clone(), replace);
        self.record_install(result)
    }
}

const DEFAULT_EQ_USAGE: &str = "`plugin` is multi-q, clap:<id> or au:<type/subtype/manufacturer>";

/// Runs `plugins.new_channel_eq`, `plugins.install_multiq` or
/// `plugins.default_eq`; they need no document and open nothing. An
/// install over something in the way is `dialog_required` "Replace"
/// unless `replace` is set; a build without Multi-Q is `unavailable`.
pub fn run_plugins(
    id: &str,
    args: &[(String, ArgValue)],
    model: &mut PluginPrefs,
    prefs: &mut Prefs,
    core: &dyn CoreCalls,
) -> Result<Ran, ControlError> {
    run_plugins_with(id, args, model, prefs, core, Step::Whole)
}

/// How much of a command's slow work has been done elsewhere.
pub enum Step {
    /// None: `run_plugins_with` does it all.
    Whole,
    /// `plugins.default_eq`'s catalogue read, done.
    Read(EqRead),
    /// `plugins.install_multiq`'s copy, done.
    Installed(Result<String, VizError>),
}

/// The `replace` flag of `plugins.install_multiq`, and the refusal when the
/// build cannot install: what a caller checks before it starts the copy.
pub fn install_request(
    args: &[(String, ArgValue)],
    model: &PluginPrefs,
) -> Result<bool, ControlError> {
    if !model.can_install() {
        return Err(ControlError::new(ErrorCode::Unavailable, NOT_IN_THIS_BUILD));
    }
    Ok(args
        .iter()
        .any(|(n, v)| n == "replace" && matches!(v, ArgValue::Flag(true))))
}

/// [`run_plugins`] with its slow work (the catalogue read of
/// `plugins.default_eq`, the copy of `plugins.install_multiq`) done
/// already, so a caller can run it off its UI thread.
pub fn run_plugins_with(
    id: &str,
    args: &[(String, ArgValue)],
    model: &mut PluginPrefs,
    prefs: &mut Prefs,
    core: &dyn CoreCalls,
    step: Step,
) -> Result<Ran, ControlError> {
    let find = |name: &str| args.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    let flag = |name: &str| match find(name) {
        Some(ArgValue::Flag(f)) => Some(*f),
        _ => None,
    };
    let ran = |status: String| Ran {
        undo: None,
        status: Some(status),
        focus: None,
        open_mixer_window: false,
        open_modify_panel: false,
    };
    let write = |r: Result<(), String>| r.map_err(|m| ControlError::new(ErrorCode::Failed, m));
    match id {
        "plugins.new_channel_eq" => {
            let on = flag("on").ok_or_else(|| ControlError::params(id, "`on` is required"))?;
            write(model.set_new_channel_eq(on, prefs, core))?;
            Ok(ran(format!(
                "new-channel EQ {}",
                if on { "on" } else { "off" }
            )))
        }
        "plugins.install_multiq" => {
            let replace = install_request(args, model)?;
            let result = match step {
                Step::Installed(r) => r,
                _ => core.install_multiq(model.install_destination.clone(), replace),
            };
            match model.record_install(result) {
                Ok(path) => Ok(ran(path)),
                Err(VizError::Exists { .. }) => Err(ControlError::dialog("Replace", &["replace"])),
                Err(VizError::Model { message }) if message == NOT_IN_THIS_BUILD => {
                    Err(ControlError::new(ErrorCode::Unavailable, message))
                }
                Err(e) => Err(ControlError::new(
                    ErrorCode::Failed,
                    format!("Install Failed: {}", error_message(&e)),
                )),
            }
        }
        "plugins.default_eq" => {
            let Some(ArgValue::Text(plugin)) = find("plugin") else {
                return Err(ControlError::params(id, "`plugin` is required"));
            };
            if plugin == "multi-q" {
                write(model.set_default_eq(None, prefs, core))?;
                return Ok(ran("default EQ Multi-Q".into()));
            }
            let bad = || ControlError::params(id, DEFAULT_EQ_USAGE);
            let (prefix, plugin_id) = plugin.split_once(':').ok_or_else(bad)?;
            let format = format_of(prefix).ok_or_else(bad)?;
            if plugin_id.is_empty() || plugin.contains('\0') {
                return Err(bad());
            }
            match step {
                Step::Read(r) => model.apply_read(r),
                _ => model.refresh(core),
            }
            let row = model
                .eq_choices()
                .iter()
                .find(|r| r.format == format && r.id == plugin_id)
                .cloned()
                .ok_or_else(|| {
                    ControlError::new(
                        ErrorCode::UnknownId,
                        format!("No installed effect is {plugin}."),
                    )
                })?;
            let name = row.name.clone();
            write(model.set_default_eq(Some(row), prefs, core))?;
            Ok(ran(format!("default EQ {name}")))
        }
        _ => Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("There is no command {id}."),
        )),
    }
}
