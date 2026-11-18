//! Co-Producer's mixer chores (plan 8d Ruling 3): plug-in references
//! resolved by the Plugin Manager's rules and plan 6g's Multi-Q rules, and
//! the list of plug-ins Co-Producer may use.
//!
//! Every naming of Multi-Q (`multi-q`, `clap:<MULTIQ_CLAP_ID>`,
//! `au:<MULTIQ_AU_ID>`) is the bundled CLAP that [`bundled::multiq`]
//! answers: the AU copy exists only for other apps, and Co-Producer never
//! loads it. Any other reference is the catalog's preferred entry, usable
//! only when OK or not yet validated (so not turned off, blocklisted or
//! missing) and of the kind the command needs.

use crate::bundled;
use crate::catalog::PluginKindTag;
use crate::error::VizError;
use crate::guard::ffi_guard;
use core_host::PluginKind;
use core_host::bundled::{MULTIQ_AU_ID, MULTIQ_CLAP_ID};
use core_host::catalog::{Catalog, CatalogEntry, PluginStatus};
use core_model::diff::shown_name;
use core_model::mixer::{PluginFormat, PluginRef};

/// The most rows [`co_producer_plugins`] gives.
pub const MAX_PLUGINS: usize = 5000;

/// The most parameters `Session::co_producer_params` gives.
pub const MAX_PARAMS: usize = 1000;

/// The bundled Multi-Q's reference text.
const MULTI_Q: &str = "multi-q";

/// The Built-in Synth's reference text (`track.new_plugin_instrument`).
const BUILT_IN_SYNTH: &str = "built-in-synth";

fn refuse<T>(message: String) -> Result<T, VizError> {
    Err(VizError::Model { message })
}

/// Whether `format`/`id` names Multi-Q.
fn is_multiq(format: PluginFormat, id: &str) -> bool {
    match format {
        PluginFormat::Clap => id == MULTIQ_CLAP_ID,
        PluginFormat::Au => id == MULTIQ_AU_ID,
    }
}

fn kind_text(k: PluginKind) -> &'static str {
    match k {
        PluginKind::Instrument => "an instrument",
        PluginKind::Effect => "an effect",
    }
}

/// Ruling 3. `want` is the kind the command needs.
pub fn resolve_plugin(text: &str, want: PluginKind) -> Result<PluginRef, VizError> {
    if text == BUILT_IN_SYNTH {
        // Not a plug-in: `track.new_plugin_instrument` takes it before
        // anything is resolved.
        return refuse(match want {
            PluginKind::Effect => "Built-in Synth is an instrument, not an effect.".into(),
            PluginKind::Instrument => "Built-in Synth is built in, not a plug-in.".into(),
        });
    }
    let named = if text == MULTI_Q {
        None
    } else if let Some(id) = text.strip_prefix("clap:") {
        Some((PluginFormat::Clap, id))
    } else if let Some(id) = text.strip_prefix("au:") {
        Some((PluginFormat::Au, id))
    } else {
        return refuse(format!("No installed plug-in is {}.", shown_name(text)));
    };
    match named {
        Some((format, id)) if !is_multiq(format, id) => {
            if id.is_empty() {
                return refuse(format!("No installed plug-in is {}.", shown_name(text)));
            }
            crate::catalog::with(|c| from_catalog(c, text, format, id, want))
        }
        _ => {
            if want == PluginKind::Instrument {
                return refuse("Multi-Q is an effect, not an instrument.".into());
            }
            bundled::multiq().ok_or_else(|| VizError::Model {
                message: "Multi-Q is not available in this build.".into(),
            })
        }
    }
}

/// The catalog's preferred entry of `format`/`id` when usable as `want`.
fn from_catalog(
    c: &Catalog,
    text: &str,
    format: PluginFormat,
    id: &str,
    want: PluginKind,
) -> Result<PluginRef, VizError> {
    let Some(e) = c.preferred(format, id) else {
        return refuse(format!("No installed plug-in is {}.", shown_name(text)));
    };
    let name = shown_name(&e.plugin.name);
    match c.status_of(e) {
        PluginStatus::Ok | PluginStatus::NotValidated => {}
        PluginStatus::Disabled => {
            return refuse(format!("{name} is turned off in the Plugin Manager."));
        }
        PluginStatus::Missing => return refuse(format!("{name} cannot be used: it is missing.")),
        PluginStatus::Blocklisted(reason) => {
            return refuse(format!(
                "{name} cannot be used: it is blocklisted ({}).",
                shown_name(&reason)
            ));
        }
    }
    match e.kind {
        Some(k) if k == want => Ok(e.plugin.clone()),
        Some(k) => refuse(format!(
            "{name} is {}, not {}.",
            kind_text(k),
            kind_text(want)
        )),
        None => refuse(format!(
            "{name} cannot be used: VizDAW does not know whether it is an instrument or an effect."
        )),
    }
}

/// The reference text for `p`: "multi-q" for the bundled Multi-Q, else
/// "clap:<id>" or "au:<id>".
pub fn plugin_ref_text(p: &PluginRef) -> String {
    match p.format {
        PluginFormat::Clap if p.id == MULTIQ_CLAP_ID => MULTI_Q.into(),
        PluginFormat::Clap => format!("clap:{}", p.id),
        PluginFormat::Au => format!("au:{}", p.id),
    }
}

/// One plug-in Co-Producer may use (Ruling 3).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoProducerPluginRow {
    /// The reference text to pass back: `multi-q`, `built-in-synth`,
    /// `clap:<id>` or `au:<id>`.
    pub plugin: String,
    /// Its name, safe to show.
    pub name: String,
    /// Its maker, safe to show; empty for the built-ins.
    pub maker: String,
    /// `au`, `clap` or `built_in`.
    pub format: String,
    /// Instrument or effect.
    pub kind: PluginKindTag,
    /// Its version, as the catalog has it.
    pub version: String,
}

/// The plug-ins Co-Producer may use, and whether the list stopped at
/// [`MAX_PLUGINS`].
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoProducerPluginsRow {
    /// Built-ins first, then instruments, then effects, each by name.
    pub plugins: Vec<CoProducerPluginRow>,
    /// More were usable than listed.
    pub truncated: bool,
}

/// The plug-ins Co-Producer may use (Ruling 3): the built-ins, then the
/// catalog's usable plug-ins of a known kind, instruments first, each by
/// name ignoring case, then reference; at most [`MAX_PLUGINS`]. `kind`
/// keeps only that kind. Never fails: empty if anything goes wrong.
#[uniffi::export]
pub fn co_producer_plugins(kind: Option<PluginKindTag>) -> CoProducerPluginsRow {
    ffi_guard(|| Ok(plugins(kind))).unwrap_or(CoProducerPluginsRow {
        plugins: Vec::new(),
        truncated: false,
    })
}

fn plugins(kind: Option<PluginKindTag>) -> CoProducerPluginsRow {
    let wants = |k: PluginKindTag| kind.is_none_or(|w| w == k);
    let mut rows = Vec::new();
    if wants(PluginKindTag::Effect) {
        if let Some(mq) = bundled::multiq() {
            rows.push(CoProducerPluginRow {
                plugin: MULTI_Q.into(),
                name: "Multi-Q".into(),
                maker: String::new(),
                format: "built_in".into(),
                kind: PluginKindTag::Effect,
                version: shown_name(&mq.version),
            });
        }
    }
    if wants(PluginKindTag::Instrument) {
        rows.push(CoProducerPluginRow {
            plugin: BUILT_IN_SYNTH.into(),
            name: "Built-in Synth".into(),
            maker: String::new(),
            format: "built_in".into(),
            kind: PluginKindTag::Instrument,
            version: String::new(),
        });
    }
    let mut found: Vec<CoProducerPluginRow> = crate::catalog::with(|c| {
        c.entries()
            .iter()
            .filter(|e| usable(c, e))
            .filter_map(|e| {
                let k = PluginKindTag::from(e.kind?);
                let plugin = plugin_ref_text(&e.plugin);
                // Only what the registry takes back (Ruling 3's shape).
                core_control::registry::plugin_ref(&plugin, false).ok()?;
                wants(k).then(|| CoProducerPluginRow {
                    plugin,
                    name: shown_name(&e.plugin.name),
                    maker: shown_name(&e.plugin.vendor),
                    format: match e.plugin.format {
                        PluginFormat::Au => "au".into(),
                        PluginFormat::Clap => "clap".into(),
                    },
                    kind: k,
                    version: shown_name(&e.plugin.version),
                })
            })
            .collect()
    });
    found.sort_by(|a, b| {
        let rank = |k: PluginKindTag| u8::from(k != PluginKindTag::Instrument);
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.plugin.cmp(&b.plugin))
    });
    rows.extend(found);
    let truncated = rows.len() > MAX_PLUGINS;
    rows.truncate(MAX_PLUGINS);
    CoProducerPluginsRow {
        plugins: rows,
        truncated,
    }
}

/// Whether `e` is a plug-in [`resolve_plugin`] gives: the preferred entry
/// of its ID, not Multi-Q (listed once, as a built-in), OK or not yet
/// validated, of a known kind.
fn usable(c: &Catalog, e: &CatalogEntry) -> bool {
    let p = &e.plugin;
    !p.id.is_empty()
        && e.kind.is_some()
        && !is_multiq(p.format, &p.id)
        && !(p.format == PluginFormat::Clap && p.id == MULTIQ_AU_ID)
        && !(p.format == PluginFormat::Au && p.id == MULTIQ_CLAP_ID)
        && c.preferred(p.format, &p.id)
            .is_some_and(|x| std::ptr::eq(x, e))
        && matches!(
            c.status_of(e),
            PluginStatus::Ok | PluginStatus::NotValidated
        )
}
