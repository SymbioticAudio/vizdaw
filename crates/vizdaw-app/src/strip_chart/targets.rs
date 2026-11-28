//! The mixer's automation targets: the row menu's groups and the names
//! rows and learned maps are titled with (Plan 4d Ruling 21,
//! `M/StripChartModel.swift:5-60`).

use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::StripKindRow;

use super::{StripLane, StripNames};
use crate::mixer::MixerModel;
use crate::session::DocSession;

/// A group of the row menu's automation choices
/// (`M/StripChartModel.swift:5-14`).
#[derive(Debug, Clone, PartialEq)]
pub struct StripMenu {
    pub title: String,
    pub lanes: Vec<StripLane>,
}

/// A document's automation targets: their names and the menu groups.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutomationTargets {
    pub names: StripNames,
    pub menus: Vec<StripMenu>,
}

/// A read: `None` on an ordinary error, Busy passed on
/// (`M/StripChartModel.swift:56-59`).
fn read<V>(r: Result<V, VizError>) -> Result<Option<V>, VizError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e @ VizError::Busy { .. }) => Err(e),
        Err(_) => Ok(None),
    }
}

impl AutomationTargets {
    /// Per strip its fader, pan (not master), mute, solo and sends; per
    /// plugin slot its automatable, visible parameters ("‹plugin› ·
    /// ‹strip›"); one velocity scale per destination; one fader per VCA
    /// (`M/StripChartModel.swift:23-54`). A Busy part is an `Err`, so the
    /// caller keeps what it had and retries; any other error reads that
    /// part as empty.
    pub fn load(s: &dyn DocSession) -> Result<AutomationTargets, VizError> {
        let mut n = StripNames::default();
        let mut menus = vec![];
        let mixer = read(s.mixer())?;
        let strips = mixer.as_ref().map_or(&[][..], |m| &m.strips[..]);
        for st in strips {
            n.strips.insert(st.id, st.name.clone());
        }
        for st in strips {
            let sends = st
                .sends
                .iter()
                .map(|x| {
                    n.strips
                        .get(&x.target)
                        .cloned()
                        .unwrap_or_else(|| format!("Bus {}", x.target))
                })
                .collect();
            n.sends.insert(st.id, sends);
            let strip = st.id;
            let mut lanes = vec![StripLane::Automation(T::StripFader { strip })];
            if !matches!(st.kind, StripKindRow::Master) {
                lanes.push(StripLane::Automation(T::StripPan { strip }));
            }
            lanes.push(StripLane::Automation(T::StripMute { strip }));
            lanes.push(StripLane::Automation(T::StripSolo { strip }));
            lanes.extend((0..st.sends.len()).map(|i| {
                StripLane::Automation(T::SendLevel {
                    strip,
                    send: u16::try_from(i).unwrap_or(u16::MAX),
                })
            }));
            menus.push(StripMenu {
                title: st.name.clone(),
                lanes,
            });
            for slot in MixerModel::slots(st) {
                n.slots.insert(slot.id, slot.plugin.name.clone());
                let params: Vec<_> = read(s.params(slot.id))?
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|p| p.automatable && !p.hidden)
                    .collect();
                if !params.is_empty() {
                    menus.push(StripMenu {
                        title: format!("{} \u{b7} {}", slot.plugin.name, st.name),
                        lanes: params
                            .iter()
                            .map(|p| {
                                StripLane::Automation(T::PluginParam {
                                    slot: slot.id,
                                    param: p.id,
                                })
                            })
                            .collect(),
                    });
                }
                n.params.insert(slot.id, params);
            }
        }
        let dests = read(s.destinations())?.unwrap_or_default();
        for d in &dests {
            n.dests.insert(d.id, d.name.clone());
        }
        if !dests.is_empty() {
            menus.push(StripMenu {
                title: "Velocity Scale".into(),
                lanes: dests
                    .iter()
                    .map(|d| StripLane::Automation(T::VelocityScale { dest: d.id }))
                    .collect(),
            });
        }
        let vcas = mixer.as_ref().map_or(&[][..], |m| &m.vcas[..]);
        for v in vcas {
            n.vcas.insert(v.id, v.name.clone());
        }
        if !vcas.is_empty() {
            menus.push(StripMenu {
                title: "VCAs".into(),
                lanes: vcas
                    .iter()
                    .map(|v| StripLane::Automation(T::VcaFader { vca: v.id }))
                    .collect(),
            });
        }
        Ok(AutomationTargets { names: n, menus })
    }
}
