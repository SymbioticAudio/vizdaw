//! Co-Producer's mixer session methods (plan 8d Tasks 3–4), kept out of
//! `session.rs`: that file gains only `mod co_producer_mixer;` and the
//! one-line hooks these tasks name.

use super::Session;
use crate::error::VizError;
use crate::guard::ffi_guard;
use crate::mixer_chores::{MAX_PARAMS, plugin_ref_text};
use crate::plugins;
use crate::snapshot::ParamRow;
use core_model::ids::PluginInstanceId;

/// A loaded plug-in's parameters for Co-Producer (plan 8d Ruling 3).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CoProducerParamsRow {
    /// The slot asked about.
    pub slot: u64,
    /// Its plug-in's reference text (`multi-q`, `clap:<id>`, `au:<id>`).
    pub plugin: String,
    /// The plug-in is running, so its parameters are known.
    pub running: bool,
    /// Its parameters with their current values, without hidden ones, at
    /// most [`MAX_PARAMS`]; empty while not running.
    pub params: Vec<ParamRow>,
}

/// Co-Producer's reads of the mixer (plan 8d Task 3).
#[uniffi::export]
impl Session {
    /// Slot `slot`'s plug-in and parameters (Ruling 3): `Err` "This song
    /// has no slot <n>." for a slot this document lacks. Reads the host as
    /// [`Session::params`] does, and changes nothing.
    pub fn co_producer_params(&self, slot: u64) -> Result<CoProducerParamsRow, VizError> {
        ffi_guard(|| {
            let inner = self.lock_inner();
            let id = PluginInstanceId(slot);
            let Some(p) = inner.doc().plugin_slot(id) else {
                return Err(VizError::Model {
                    message: format!("This song has no slot {slot}."),
                });
            };
            let plugin = plugin_ref_text(&p.plugin);
            let params = plugins::with(self.id, |h| {
                let inst = h.and_then(|h| h.instance(id))?;
                let infos = inst.info().params.clone();
                let rows = infos
                    .into_iter()
                    .filter(|i| !i.hidden)
                    .take(MAX_PARAMS)
                    .map(|i| ParamRow {
                        id: i.id.0,
                        value: inst.param_value(i.id).unwrap_or(i.default),
                        name: i.name,
                        module: i.module,
                        min: i.min,
                        max: i.max,
                        default: i.default,
                        stepped: i.stepped,
                        automatable: i.automatable,
                        hidden: false,
                    })
                    .collect::<Vec<_>>();
                Some(rows)
            })?;
            Ok(CoProducerParamsRow {
                slot,
                plugin,
                running: params.is_some(),
                params: params.unwrap_or_default(),
            })
        })
    }
}
