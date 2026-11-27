//! The mixer's plug-in picks (Swift `MixerModel.BrowserTarget`,
//! `openBrowser`, `browserModel`, `pick`, `instrumentMenu`,
//! `instrumentDestination`, `pickInstrument`; `M/MixerModel.swift:124-186`,
//! `:314-319`, `:452-562`): where the open browser's pick goes, the
//! instrument menu of an instrument slot, and the replace of a
//! destination's instrument. Each pick that goes is one session call whose
//! snapshot is applied, one undo step the core names; a refusal is the
//! Swift alert title. The rows come from the caller
//! ([`AppPlugins`]), never from a catalog of the mixer's own.

use std::collections::HashSet;

use vizdaw_ffi::mixer::{StripKindRow, StripRow};
use vizdaw_ffi::snapshot::PluginRefRow;
use vizdaw_ffi::{CatalogRowFfi, PluginKindTag, PluginStatusRow};

use super::model::{MixerModel, MixerPlacement};
use crate::document::DocumentModel;
use crate::plugin_browser::PluginBrowserModel;
use crate::plugins::AppPlugins;

pub const ADD_PLUGIN_FAILED: &str = "Add Plugin Failed";
pub const NEW_INSTRUMENT_FAILED: &str = "New Instrument Failed";
pub const INSTRUMENT_FAILED: &str = "Instrument Failed";
pub const LOAD_EQ_FAILED: &str = "Load EQ Failed";

/// Where a plug-in picked in the browser goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserTarget {
    /// The end of a strip's insert chain.
    Insert { strip: u64 },
    /// A new plug-in instrument (and its track).
    Instrument,
    /// An existing destination, which then plays the plug-in.
    Destination(u64),
    /// A strip's EQ slot (an effect, before the inserts).
    Eq { strip: u64 },
}

impl BrowserTarget {
    /// The browser lists instruments for it (effects for an insert or
    /// the EQ).
    pub fn wants_instrument(&self) -> bool {
        matches!(
            self,
            BrowserTarget::Instrument | BrowserTarget::Destination(_)
        )
    }

    /// The kind of plug-in its browser lists.
    pub fn kind(&self) -> PluginKindTag {
        if self.wants_instrument() {
            PluginKindTag::Instrument
        } else {
            PluginKindTag::Effect
        }
    }
}

/// The open browser: its target, where its popover shows, and its list.
#[derive(Debug, Clone)]
pub(super) struct OpenBrowser {
    target: BrowserTarget,
    placement: MixerPlacement,
    model: PluginBrowserModel,
}

/// One plug-in of the instrument menu.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentChoice {
    pub plugin: PluginRefRow,
    /// It is the plug-in loaded now (its item is checked).
    pub checked: bool,
}

/// One maker's submenu.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentMaker {
    pub vendor: String,
    pub choices: Vec<InstrumentChoice>,
}

/// An instrument slot's menu (`S/MixerView.swift:356-391`).
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentMenu {
    /// "Open Editor" is enabled: a plug-in is loaded (the Built-in Synth
    /// slot has none to open). The view may also turn it off for a slot
    /// with no editor.
    pub open_editor: bool,
    pub makers: Vec<InstrumentMaker>,
}

/// One item of the instrument menu, in Swift's order.
#[derive(Debug, Clone, PartialEq)]
pub enum InstrumentMenuItem {
    OpenEditor {
        enabled: bool,
    },
    Separator,
    /// No usable plug-in instrument: one disabled item.
    NoInstruments,
    Maker(InstrumentMaker),
}

impl InstrumentMenuItem {
    pub const OPEN_EDITOR: &'static str = "Open Editor";
    pub const NO_INSTRUMENTS: &'static str = "No Plugin Instruments";

    pub fn title(&self) -> &str {
        match self {
            InstrumentMenuItem::OpenEditor { .. } => Self::OPEN_EDITOR,
            InstrumentMenuItem::Separator => "",
            InstrumentMenuItem::NoInstruments => Self::NO_INSTRUMENTS,
            InstrumentMenuItem::Maker(m) => &m.vendor,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            InstrumentMenuItem::OpenEditor { enabled } => *enabled,
            InstrumentMenuItem::Separator | InstrumentMenuItem::NoInstruments => false,
            InstrumentMenuItem::Maker(_) => true,
        }
    }
}

impl InstrumentMenu {
    /// "Open Editor", a separator, then the makers or "No Plugin
    /// Instruments".
    pub fn items(&self) -> Vec<InstrumentMenuItem> {
        let mut out = vec![
            InstrumentMenuItem::OpenEditor {
                enabled: self.open_editor,
            },
            InstrumentMenuItem::Separator,
        ];
        if self.makers.is_empty() {
            out.push(InstrumentMenuItem::NoInstruments);
        }
        out.extend(self.makers.iter().cloned().map(InstrumentMenuItem::Maker));
        out
    }
}

/// The same plug-in: its format and id.
fn same(a: &PluginRefRow, b: &PluginRefRow) -> bool {
    a.format == b.format && a.id == b.id
}

impl MixerModel {
    /// Opens the browser for `target`, its popover in `placement`. Its
    /// list is read from `plugins` once per presentation: opening again
    /// while it shows a list of the same kind keeps that list (and any
    /// search the view keeps), so a scan that finishes meanwhile changes
    /// nothing until it closes (`M/MixerModel.swift:173-186`).
    pub fn open_browser(
        &mut self,
        target: BrowserTarget,
        placement: MixerPlacement,
        plugins: &AppPlugins,
    ) {
        let kind = target.kind();
        let model = match self.browser.take() {
            Some(open) if open.model.kind == kind => open.model,
            _ => PluginBrowserModel::new(kind, plugins.browser(kind)),
        };
        self.browser = Some(OpenBrowser {
            target,
            placement,
            model,
        });
    }

    /// The open browser's list.
    pub fn browser(&self) -> Option<&PluginBrowserModel> {
        self.browser.as_ref().map(|b| &b.model)
    }

    /// Where the open browser's pick goes.
    pub fn browser_target(&self) -> Option<BrowserTarget> {
        self.browser.as_ref().map(|b| b.target)
    }

    /// The open browser shows in `p`.
    pub fn shows_browser(&self, p: MixerPlacement) -> bool {
        self.browser.as_ref().is_some_and(|b| b.placement == p)
    }

    /// Closes the browser; its list goes with it.
    pub fn close_browser(&mut self) {
        self.browser = None;
    }

    /// Puts `plugin`, picked in the open browser, where it was opened, and
    /// closes the browser. Returns the slot whose window should open (the
    /// new insert, instrument or EQ); `None` when nothing went (no browser,
    /// no change, or a refusal, whose alert is up).
    pub fn pick(&mut self, doc: &mut DocumentModel, plugin: &PluginRefRow) -> Option<u64> {
        let target = self.browser.take()?.target;
        match target {
            BrowserTarget::Insert { strip } => {
                let before: HashSet<u64> = self
                    .strip(strip)
                    .map(|s| s.inserts.iter().map(|x| x.id).collect())
                    .unwrap_or_default();
                let p = plugin.clone();
                if !self.command(doc, ADD_PLUGIN_FAILED, |s| s.add_insert(strip, None, p)) {
                    return None;
                }
                self.strip(strip)?
                    .inserts
                    .iter()
                    .map(|x| x.id)
                    .find(|id| !before.contains(id))
            }
            BrowserTarget::Instrument => {
                let before: HashSet<u64> = self.instrument_slots().collect();
                let p = plugin.clone();
                if !self.command(doc, NEW_INSTRUMENT_FAILED, |s| {
                    s.add_plugin_instrument(p.name.clone(), p)
                }) {
                    return None;
                }
                self.instrument_slots().find(|id| !before.contains(id))
            }
            BrowserTarget::Destination(dest) => {
                self.pick_instrument_on_destination(doc, plugin, dest)
            }
            BrowserTarget::Eq { strip } => {
                // The plug-in already in the slot is no change, so no undo
                // step.
                let loaded = self.strip(strip).and_then(|s| s.eq.as_ref());
                if loaded.is_some_and(|x| x.plugin == *plugin) {
                    return None;
                }
                let p = plugin.clone();
                if !self.command(doc, LOAD_EQ_FAILED, |s| s.set_eq(strip, Some(p))) {
                    return None;
                }
                self.strip(strip)?.eq.as_ref().map(|x| x.id)
            }
        }
    }

    /// Every plug-in instrument's own slot.
    fn instrument_slots(&self) -> impl Iterator<Item = u64> + '_ {
        self.snapshot()
            .strips
            .iter()
            .filter_map(|s| s.instrument.as_ref().map(|x| x.id))
    }

    /// The instrument menu: the enabled, usable instruments of `rows`
    /// grouped by maker as the browser groups them, `current` checked
    /// (`M/MixerModel.swift:487-501`). "Open Editor" is enabled with a
    /// plug-in loaded.
    pub fn instrument_menu(
        rows: &[CatalogRowFfi],
        current: Option<&PluginRefRow>,
    ) -> InstrumentMenu {
        let usable: Vec<CatalogRowFfi> = rows
            .iter()
            .filter(|r| {
                r.kind == Some(PluginKindTag::Instrument)
                    && r.enabled
                    && r.status == PluginStatusRow::Ok
            })
            .cloned()
            .collect();
        let makers = PluginBrowserModel::group(&usable, "")
            .into_iter()
            .map(|(vendor, rows)| InstrumentMaker {
                vendor,
                choices: rows
                    .into_iter()
                    .map(|r| InstrumentChoice {
                        checked: current.is_some_and(|c| same(c, &r.plugin)),
                        plugin: r.plugin,
                    })
                    .collect(),
            })
            .collect();
        InstrumentMenu {
            open_editor: current.is_some(),
            makers,
        }
    }

    /// The destination an instrument strip plays: its own, or for the
    /// Built-in Synth strip (one strip for every built-in synth) the
    /// [`synth_focus`](Self::synth_focus) one if it is a built-in synth,
    /// else the first. `None` for other strips
    /// (`M/MixerModel.swift:510-524`).
    pub fn instrument_destination(&self, strip: &StripRow) -> Option<u64> {
        match strip.kind {
            StripKindRow::Instrument { dest } => Some(dest),
            StripKindRow::BuiltinSynth => {
                let synths: Vec<u64> = self
                    .destinations()
                    .iter()
                    .filter(|d| d.plugin.is_none() && !d.external)
                    .map(|d| d.id)
                    .collect();
                match self.synth_focus {
                    Some(f) if synths.contains(&f) => Some(f),
                    _ => synths.first().copied(),
                }
            }
            _ => None,
        }
    }

    /// The plug-in destination `dest` plays; `None` for the built-in
    /// synth and an unknown destination.
    pub fn instrument_plugin_of_destination(&self, dest: u64) -> Option<&PluginRefRow> {
        self.destinations()
            .iter()
            .find(|d| d.id == dest)?
            .plugin
            .as_ref()
    }

    /// Replaces the instrument of `strip` (an instrument strip) with
    /// `plugin`, picked in its menu. The plug-in already loaded, and any
    /// other strip, is no call. Returns the new instrument's slot (the
    /// window to open) when it went; `None` for no change or a refusal
    /// (the alert "Instrument Failed" is up).
    pub fn pick_instrument(
        &mut self,
        doc: &mut DocumentModel,
        plugin: &PluginRefRow,
        strip: &StripRow,
    ) -> Option<u64> {
        match strip.kind {
            StripKindRow::Instrument { dest } => {
                if strip
                    .instrument
                    .as_ref()
                    .is_some_and(|x| same(&x.plugin, plugin))
                {
                    return None;
                }
                self.replace_instrument(doc, plugin, dest)
            }
            StripKindRow::BuiltinSynth => {
                let dest = self.instrument_destination(strip)?;
                self.pick_instrument_on_destination(doc, plugin, dest)
            }
            _ => None,
        }
    }

    /// Makes destination `dest` play `plugin` (every track using it plays
    /// it). The plug-in it plays already is no call; a destination this
    /// mixer has not seen is the core's to refuse. Returns the new slot as
    /// [`pick_instrument`](Self::pick_instrument).
    pub fn pick_instrument_on_destination(
        &mut self,
        doc: &mut DocumentModel,
        plugin: &PluginRefRow,
        dest: u64,
    ) -> Option<u64> {
        if self
            .instrument_plugin_of_destination(dest)
            .is_some_and(|cur| same(cur, plugin))
        {
            return None;
        }
        self.replace_instrument(doc, plugin, dest)
    }

    fn replace_instrument(
        &mut self,
        doc: &mut DocumentModel,
        plugin: &PluginRefRow,
        dest: u64,
    ) -> Option<u64> {
        let p = plugin.clone();
        if !self.command(doc, INSTRUMENT_FAILED, |s| {
            s.set_destination_plugin(dest, p)
        }) {
            return None;
        }
        self.snapshot()
            .strips
            .iter()
            .find(|s| s.kind == StripKindRow::Instrument { dest })?
            .instrument
            .as_ref()
            .map(|x| x.id)
    }
}
