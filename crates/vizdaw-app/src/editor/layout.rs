//! The editors' saved layout (Swift `EditorLayout.swift`,
//! `WindowLayout.swift`): stored by the session in the package's
//! `ui.json`, written on Save, never an undo step. Decoding is tolerant (a
//! missing or malformed field keeps its default); writing merges into the
//! JSON already there, so keys this app does not model (torn windows,
//! the drum grid, …) survive.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use vizdaw_ffi::blocks::BlockRule;

use super::detail::{DetailEditorKind, DetailState};
use super::snap::Snap;
use crate::arrangement::PPQ;
use crate::drum_grid::DrumGridSettings;
use crate::piano_roll::PianoRollSettings;
use crate::strip_chart::{StripChartSettings, StripLane};

/// Light drawing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LightMode {
    #[default]
    Auto,
    On,
    Off,
}

impl LightMode {
    pub fn title(self) -> &'static str {
        match self {
            LightMode::Auto => "Automatic",
            LightMode::On => "On",
            LightMode::Off => "Off",
        }
    }
}

/// The arrangement's block rule with its user-set size. Swift writes
/// `{"silenceGap":{"beats":4}}`, `{"fixedBars":{"_0":2}}`, `{"wholeLane":{}}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockRuleSetting {
    SilenceGap {
        beats: i64,
    },
    FixedBars {
        #[serde(rename = "_0")]
        bars: i64,
    },
    WholeLane {},
}

impl Default for BlockRuleSetting {
    fn default() -> Self {
        BlockRuleSetting::SilenceGap { beats: 4 }
    }
}

impl BlockRuleSetting {
    /// The core's rule.
    pub fn rule(self) -> BlockRule {
        match self {
            BlockRuleSetting::SilenceGap { beats } => BlockRule::SilenceGap {
                ticks: beats.max(1).saturating_mul(PPQ),
            },
            BlockRuleSetting::FixedBars { bars } => BlockRule::FixedBars {
                bars: u32::try_from(bars.max(1)).unwrap_or(u32::MAX),
            },
            BlockRuleSetting::WholeLane {} => BlockRule::WholeLane,
        }
    }
}

/// The window's two main views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MainView {
    #[default]
    Arrange,
    Mix,
}

/// The three panes around the centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSide {
    Left,
    Bottom,
    Right,
}

/// What a pane shows; the raw values are the control channel's names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneView {
    Strip,
    Sequences,
    Inspector,
    Editor,
    Mixer,
    Controllers,
    MediaPool,
    Plugins,
    Markers,
    AudioEditor,
}

impl MainView {
    pub const ALL: [MainView; 2] = [MainView::Arrange, MainView::Mix];

    /// The control channel's name (`arrange`, `mix`).
    pub fn name(self) -> &'static str {
        match self {
            MainView::Arrange => "arrange",
            MainView::Mix => "mix",
        }
    }

    pub fn parse(name: &str) -> Option<MainView> {
        MainView::ALL.into_iter().find(|v| v.name() == name)
    }
}

impl PaneView {
    pub const ALL: [PaneView; 10] = [
        PaneView::Strip,
        PaneView::Sequences,
        PaneView::Inspector,
        PaneView::Editor,
        PaneView::Mixer,
        PaneView::Controllers,
        PaneView::MediaPool,
        PaneView::Plugins,
        PaneView::Markers,
        PaneView::AudioEditor,
    ];

    /// The control channel's name (`strip`, `media_pool`, …).
    pub fn name(self) -> &'static str {
        match self {
            PaneView::Strip => "strip",
            PaneView::Sequences => "sequences",
            PaneView::Inspector => "inspector",
            PaneView::Editor => "editor",
            PaneView::Mixer => "mixer",
            PaneView::Controllers => "controllers",
            PaneView::MediaPool => "media_pool",
            PaneView::Plugins => "plugins",
            PaneView::Markers => "markers",
            PaneView::AudioEditor => "audio_editor",
        }
    }

    pub fn parse(name: &str) -> Option<PaneView> {
        PaneView::ALL.into_iter().find(|v| v.name() == name)
    }
}

impl PaneSide {
    pub const ALL: [PaneSide; 3] = SIDES;

    /// The control channel's name (`left`, `bottom`, `right`).
    pub fn name(self) -> &'static str {
        match self {
            PaneSide::Left => "left",
            PaneSide::Bottom => "bottom",
            PaneSide::Right => "right",
        }
    }

    pub fn parse(name: &str) -> Option<PaneSide> {
        SIDES.into_iter().find(|s| s.name() == name)
    }

    /// The views the pane can show in `main`; the first is its default.
    pub fn views(self, main: MainView) -> &'static [PaneView] {
        use PaneView::*;
        match (self, main) {
            (PaneSide::Left, _) => &[Strip, Sequences, Inspector],
            (PaneSide::Bottom, MainView::Arrange) => &[Editor, AudioEditor, Mixer],
            (PaneSide::Bottom, MainView::Mix) => &[Editor, AudioEditor],
            (PaneSide::Right, _) => &[Controllers, MediaPool, Plugins, Markers],
        }
    }

    /// The sizes a pane is clamped to.
    pub fn size_range(self) -> RangeInclusive<f64> {
        match self {
            PaneSide::Left => 160.0..=480.,
            PaneSide::Bottom => 120.0..=900.,
            PaneSide::Right => 180.0..=520.,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaneState {
    pub shown: bool,
    pub view: PaneView,
    pub size: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaneSetup {
    pub left: PaneState,
    pub bottom: PaneState,
    pub right: PaneState,
}

impl PaneSetup {
    pub fn pane(&self, side: PaneSide) -> &PaneState {
        match side {
            PaneSide::Left => &self.left,
            PaneSide::Bottom => &self.bottom,
            PaneSide::Right => &self.right,
        }
    }

    pub fn pane_mut(&mut self, side: PaneSide) -> &mut PaneState {
        match side {
            PaneSide::Left => &mut self.left,
            PaneSide::Bottom => &mut self.bottom,
            PaneSide::Right => &mut self.right,
        }
    }
}

/// Each main view's pane setup and the window frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewLayouts {
    pub main: MainView,
    pub arrange: PaneSetup,
    pub mix: PaneSetup,
    /// The document window's `[x, y, width, height]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Vec<f64>>,
}

const SIDES: [PaneSide; 3] = [PaneSide::Left, PaneSide::Bottom, PaneSide::Right];

impl Default for ViewLayouts {
    fn default() -> Self {
        let pane = |shown, view, size| PaneState { shown, view, size };
        ViewLayouts {
            main: MainView::Arrange,
            arrange: PaneSetup {
                left: pane(true, PaneView::Strip, 220.),
                bottom: pane(true, PaneView::Editor, 260.),
                right: pane(false, PaneView::Controllers, 260.),
            },
            mix: PaneSetup {
                left: pane(false, PaneView::Strip, 220.),
                bottom: pane(false, PaneView::Editor, 260.),
                right: pane(false, PaneView::Plugins, 260.),
            },
            frame: None,
        }
    }
}

impl ViewLayouts {
    pub fn setup(&self, main: MainView) -> &PaneSetup {
        match main {
            MainView::Arrange => &self.arrange,
            MainView::Mix => &self.mix,
        }
    }

    pub fn setup_mut(&mut self, main: MainView) -> &mut PaneSetup {
        match main {
            MainView::Arrange => &mut self.arrange,
            MainView::Mix => &mut self.mix,
        }
    }

    /// The current main view's setup.
    pub fn current(&self) -> &PaneSetup {
        self.setup(self.main)
    }

    pub fn current_mut(&mut self) -> &mut PaneSetup {
        self.setup_mut(self.main)
    }

    /// Sets `main`'s `side` pane: its view (refused when off the pane's
    /// list: the refusal is returned and nothing changes), shown flag and
    /// size (clamped to the pane's range; a non-finite size is ignored).
    /// Swift `EditorModel.setPane`.
    pub fn set_pane(
        &mut self,
        side: PaneSide,
        view: Option<PaneView>,
        shown: Option<bool>,
        size: Option<f64>,
        main: MainView,
    ) -> Result<(), String> {
        if let Some(v) = view
            && !side.views(main).contains(&v)
        {
            return Err(off_list(v.name(), side, main));
        }
        let p = self.setup_mut(main).pane_mut(side);
        if let Some(v) = view {
            p.view = v;
        }
        if let Some(on) = shown {
            p.shown = on;
        }
        if let Some(size) = size.filter(|s| s.is_finite()) {
            let r = side.size_range();
            p.size = size.clamp(*r.start(), *r.end());
        }
        Ok(())
    }

    /// Tolerant: each pane field that is missing or malformed keeps its
    /// default; then the whole is sanitized.
    fn decode(v: &Value) -> ViewLayouts {
        let mut out = ViewLayouts::default();
        let Some(obj) = v.as_object() else {
            return out;
        };
        out.main = field(obj, "main", out.main);
        for main in [MainView::Arrange, MainView::Mix] {
            let key = match main {
                MainView::Arrange => "arrange",
                MainView::Mix => "mix",
            };
            let Some(setup) = obj.get(key).and_then(Value::as_object) else {
                continue;
            };
            for side in SIDES {
                let name = match side {
                    PaneSide::Left => "left",
                    PaneSide::Bottom => "bottom",
                    PaneSide::Right => "right",
                };
                let Some(p) = setup.get(name).and_then(Value::as_object) else {
                    continue;
                };
                let pane = out.setup_mut(main).pane_mut(side);
                pane.shown = field(p, "shown", pane.shown);
                pane.view = field(p, "view", pane.view);
                pane.size = field(p, "size", pane.size);
            }
        }
        out.frame = field(obj, "frame", None);
        out.sanitized()
    }

    /// Off-list views go back to the default, sizes are clamped, a
    /// malformed frame goes.
    pub fn sanitized(mut self) -> ViewLayouts {
        let d = ViewLayouts::default();
        for main in [MainView::Arrange, MainView::Mix] {
            for side in SIDES {
                let dp = *d.setup(main).pane(side);
                let p = self.setup_mut(main).pane_mut(side);
                if !side.views(main).contains(&p.view) {
                    p.view = dp.view;
                }
                let r = side.size_range();
                p.size = if p.size.is_finite() {
                    p.size.clamp(*r.start(), *r.end())
                } else {
                    dp.size
                };
            }
        }
        if self
            .frame
            .as_ref()
            .is_some_and(|f| f.len() != 4 || !f.iter().all(|x| x.is_finite()))
        {
            self.frame = None;
        }
        self
    }
}

/// Why `view` (as the channel names it) cannot show in `main`'s `side`
/// pane (Swift `ViewCommands.choose`).
pub fn off_list(view: &str, side: PaneSide, main: MainView) -> String {
    format!(
        "{view} is not on the {} pane's list in the {} view.",
        side.name(),
        main.name()
    )
}

/// The transport's marks and record modes, saved with the project under
/// `transport` (Swift `TransportMarks`): ten numbered locators, the punch
/// range and the punch and overdub switches. Changing them is no undo
/// step. Decoding is tolerant: a missing or malformed field keeps its
/// default; ticks are kept within ±2^40; the punch range is kept only
/// when out is after in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportMarks {
    /// Locator `n`'s tick, or `None` when it is not set.
    pub locators: [Option<i64>; TransportMarks::LOCATOR_COUNT],
    pub punch_in: i64,
    /// After `punch_in`.
    pub punch_out: i64,
    pub punch: bool,
    /// New documents start in Overdub; a saved `overdub` keeps its mode.
    pub overdub: bool,
}

impl Default for TransportMarks {
    fn default() -> Self {
        TransportMarks {
            locators: [None; TransportMarks::LOCATOR_COUNT],
            punch_in: 0,
            punch_out: 4 * 61_440,
            punch: false,
            overdub: true,
        }
    }
}

impl TransportMarks {
    /// How many locators (keys 0–9).
    pub const LOCATOR_COUNT: usize = 10;
    /// The model's time limit, either side of 0.
    pub const TICK_LIMIT: i64 = 1 << 40;

    /// `t` when it is within the model's limit.
    fn sane(t: i64) -> Option<i64> {
        (-Self::TICK_LIMIT..=Self::TICK_LIMIT)
            .contains(&t)
            .then_some(t)
    }

    /// Locator `n` (0–9), if set.
    pub fn locator(&self, n: usize) -> Option<i64> {
        self.locators.get(n).copied().flatten()
    }

    /// Sets (a tick) or clears (`None`) locator `n`; false for a number
    /// outside 0–9 or a tick past the limit.
    pub fn set_locator(&mut self, n: usize, tick: Option<i64>) -> bool {
        if n >= Self::LOCATOR_COUNT || tick.is_some_and(|t| Self::sane(t).is_none()) {
            return false;
        }
        self.locators[n] = tick;
        true
    }

    /// Sets the punch range; false (nothing changed) unless `out > in`
    /// and both are within the limit.
    pub fn set_punch(&mut self, a: i64, b: i64) -> bool {
        if Self::sane(a).is_none() || Self::sane(b).is_none() || b <= a {
            return false;
        }
        self.punch_in = a;
        self.punch_out = b;
        true
    }

    /// Tolerant, as Swift's `init(from:)`.
    fn decode(v: &Value) -> TransportMarks {
        let mut out = TransportMarks::default();
        let Some(obj) = v.as_object() else {
            return out;
        };
        let raw: Vec<Option<i64>> = field(obj, "locators", Vec::new());
        for (slot, t) in out.locators.iter_mut().zip(raw) {
            *slot = t.and_then(Self::sane);
        }
        let a = field(obj, "punchIn", out.punch_in);
        let b = field(obj, "punchOut", out.punch_out);
        out.set_punch(a, b);
        out.punch = field(obj, "punch", out.punch);
        out.overdub = field(obj, "overdub", out.overdub);
        out
    }
}

/// The editor state saved with the project, under Swift's keys.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorLayout {
    /// The detail pane's content (`None`: empty).
    pub detail: Option<DetailState>,
    /// The editor a double-clicked block opens in.
    pub last_kind: DetailEditorKind,
    pub block_rule: BlockRuleSetting,
    pub light_mode: LightMode,
    pub snap: Snap,
    pub lane_height: f64,
    pub ticks_per_point: f64,
    /// Follow playback (page mode).
    pub follow: bool,
    /// Per-view pane setups and the window frame.
    pub views: ViewLayouts,
    /// The piano roll's view settings (`pianoRoll`).
    pub piano_roll: PianoRollSettings,
    /// The locators, punch range and record modes (`transport`).
    pub transport: TransportMarks,
    /// Every Strip Chart's bar (`stripChart`, Plan 4d Ruling 21).
    pub strip_chart: StripChartSettings,
    /// Every Drum Grid's bar (`drumGrid`, Plan 4e Ruling 11).
    pub drum_grid: DrumGridSettings,
    /// The right pane's controllers chart rows, per track (`controllers`,
    /// keyed by `controllers_key`; Plan 7a Ruling 7).
    pub controllers: BTreeMap<String, Vec<StripLane>>,
}

/// The `controllers` key of a track: `"seq:lane"`
/// (`M/EditorLayout.swift:67`).
pub fn controllers_key(seq: u64, lane: u64) -> String {
    format!("{seq}:{lane}")
}

/// The `controllers` map; an entry that fails to decode is dropped, and
/// anything but an object is empty.
fn decode_controllers(v: &Value) -> BTreeMap<String, Vec<StripLane>> {
    let Some(obj) = v.as_object() else {
        return BTreeMap::new();
    };
    obj.iter()
        .filter_map(|(k, rows)| Some((k.clone(), Vec::<StripLane>::deserialize(rows).ok()?)))
        .collect()
}

impl Default for EditorLayout {
    fn default() -> Self {
        EditorLayout {
            detail: None,
            last_kind: DetailEditorKind::PianoRoll,
            block_rule: BlockRuleSetting::default(),
            light_mode: LightMode::Auto,
            snap: Snap::default(),
            lane_height: 48.,
            ticks_per_point: 384.,
            follow: true,
            views: ViewLayouts::default(),
            piano_roll: PianoRollSettings::default(),
            transport: TransportMarks::default(),
            strip_chart: StripChartSettings::default(),
            drum_grid: DrumGridSettings::default(),
            controllers: BTreeMap::new(),
        }
    }
}

/// `obj[key]` as a `T`, or `fallback` when missing or malformed.
pub(crate) fn field<T: DeserializeOwned>(obj: &Map<String, Value>, key: &str, fallback: T) -> T {
    obj.get(key)
        .and_then(|v| T::deserialize(v).ok())
        .unwrap_or(fallback)
}

/// A modelled value as JSON (plain data: serializing cannot fail).
fn to_value<T: Serialize>(x: &T) -> Value {
    serde_json::to_value(x).unwrap_or(Value::Null)
}

/// `v` with every object's keys in sorted order (whatever map type
/// `serde_json` was built with).
fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut entries: Vec<(String, Value)> = m.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        other => other,
    }
}

impl EditorLayout {
    /// Lane heights a hand-edited or corrupt `ui.json` is clamped to.
    pub const LANE_HEIGHT_RANGE: RangeInclusive<f64> = 16.0..=400.;
    /// Zoom levels (`TimeAxis`'s limits) a decoded layout is clamped to.
    pub const TICKS_PER_POINT_RANGE: RangeInclusive<f64> = 2.0..=61_440.;

    /// `v` clamped to `range`; `fallback` when it is not a finite positive
    /// number.
    pub fn sane(v: f64, range: &RangeInclusive<f64>, fallback: f64) -> f64 {
        if !v.is_finite() || v <= 0. {
            return fallback;
        }
        v.clamp(*range.start(), *range.end())
    }

    /// The layout in `json`, or the default when it is missing or
    /// unreadable; each field that is missing or malformed keeps its
    /// default.
    pub fn decode(json: Option<&str>) -> EditorLayout {
        let d = EditorLayout::default();
        let Some(Value::Object(obj)) = json.and_then(|j| serde_json::from_str(j).ok()) else {
            return d;
        };
        EditorLayout {
            detail: field(&obj, "detail", None),
            last_kind: field(&obj, "lastKind", d.last_kind),
            block_rule: field(&obj, "blockRule", d.block_rule),
            light_mode: field(&obj, "lightMode", d.light_mode),
            snap: field(&obj, "snap", d.snap),
            lane_height: Self::sane(
                field(&obj, "laneHeight", d.lane_height),
                &Self::LANE_HEIGHT_RANGE,
                d.lane_height,
            ),
            ticks_per_point: Self::sane(
                field(&obj, "ticksPerPoint", d.ticks_per_point),
                &Self::TICKS_PER_POINT_RANGE,
                d.ticks_per_point,
            ),
            follow: field(&obj, "follow", d.follow),
            views: obj
                .get("views")
                .map_or_else(ViewLayouts::default, ViewLayouts::decode),
            piano_roll: obj
                .get("pianoRoll")
                .map_or_else(PianoRollSettings::default, PianoRollSettings::decode),
            transport: obj
                .get("transport")
                .map_or_else(TransportMarks::default, TransportMarks::decode),
            strip_chart: obj
                .get("stripChart")
                .map_or_else(StripChartSettings::default, StripChartSettings::decode),
            drum_grid: obj
                .get("drumGrid")
                .map_or_else(DrumGridSettings::default, DrumGridSettings::decode),
            controllers: obj
                .get("controllers")
                .map_or_else(BTreeMap::new, decode_controllers),
        }
    }

    /// The modelled keys; `None` values are left out (Swift omits a nil
    /// optional).
    fn entries(&self) -> Vec<(&'static str, Option<Value>)> {
        vec![
            ("detail", self.detail.as_ref().map(to_value)),
            ("lastKind", Some(to_value(&self.last_kind))),
            ("blockRule", Some(to_value(&self.block_rule))),
            ("lightMode", Some(to_value(&self.light_mode))),
            ("snap", Some(to_value(&self.snap))),
            ("laneHeight", Some(Value::from(self.lane_height))),
            ("ticksPerPoint", Some(Value::from(self.ticks_per_point))),
            ("follow", Some(Value::Bool(self.follow))),
            ("views", Some(to_value(&self.views))),
            ("pianoRoll", Some(to_value(&self.piano_roll))),
            ("transport", Some(to_value(&self.transport))),
            ("stripChart", Some(to_value(&self.strip_chart))),
            ("drumGrid", Some(to_value(&self.drum_grid))),
            ("controllers", Some(to_value(&self.controllers))),
        ]
    }

    /// This layout alone, sorted keys.
    pub fn encoded(&self) -> String {
        self.merge_into(None)
    }

    /// This layout written over `existing` (the `ui.json` the session
    /// holds): the keys this layout models are replaced (or removed when
    /// empty), every other key stays; sorted keys. A missing or damaged
    /// `existing` starts from nothing.
    pub fn merge_into(&self, existing: Option<&str>) -> String {
        let mut obj = match existing.and_then(|j| serde_json::from_str(j).ok()) {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        for (key, value) in self.entries() {
            match value {
                Some(v) => {
                    let merged = merge_key(key, obj.get(key), v);
                    obj.insert(key.to_owned(), merged);
                }
                None => {
                    obj.remove(key);
                }
            }
        }
        sorted(Value::Object(obj)).to_string()
    }
}

/// Struct-shaped objects are merged into what is there, so keys this app
/// does not model inside them survive; enum-shaped values (`blockRule`,
/// `lastKind`, `lightMode`, `detail.target`), the `controllers` map and
/// the detail itself (its unmodelled fields ride in `DetailState::extra`)
/// are replaced whole.
fn merge_key(key: &str, old: Option<&Value>, new: Value) -> Value {
    match key {
        "snap" | "pianoRoll" | "transport" | "stripChart" | "drumGrid" => {
            merge_struct(old, new, &[])
        }
        "views" => {
            let mut views = merge_struct(old, new, &["frame"]);
            for main in ["arrange", "mix"] {
                let old_setup = old.and_then(|o| o.get(main));
                let Some(setup) = views.get_mut(main) else {
                    continue;
                };
                let mut merged = merge_struct(old_setup, setup.take(), &[]);
                for side in ["left", "bottom", "right"] {
                    let old_pane = old_setup.and_then(|o| o.get(side));
                    if let Some(pane) = merged.get_mut(side) {
                        *pane = merge_struct(old_pane, pane.take(), &[]);
                    }
                }
                *setup = merged;
            }
            views
        }
        "detail" => {
            let mut detail = new;
            let old_filter = old.and_then(|o| o.get("listFilter"));
            if let Some(filter) = detail.get_mut("listFilter") {
                *filter = merge_struct(old_filter, filter.take(), &["shownDests"]);
            }
            detail
        }
        _ => new,
    }
}

/// `new`'s keys written over the object `old`: keys `new` lacks stay,
/// except the modelled optionals in `optional` (absent in `new` means
/// none). Anything that is not two objects is `new`.
fn merge_struct(old: Option<&Value>, new: Value, optional: &[&str]) -> Value {
    let (Some(Value::Object(old)), Value::Object(new)) = (old, &new) else {
        return new;
    };
    let mut out = old.clone();
    for key in optional {
        if !new.contains_key(*key) {
            out.remove(*key);
        }
    }
    for (k, v) in new {
        out.insert(k.clone(), v.clone());
    }
    Value::Object(out)
}
