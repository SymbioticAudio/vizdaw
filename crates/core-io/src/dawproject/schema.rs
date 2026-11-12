//! The typed model of a DAWproject file (`project.xml` and `metadata.xml`,
//! format version 1; the XSD at `tests/data/dawproject/Project.xsd` is the
//! reference), reading it from a [`Tree`], writing it in the XSD's order and
//! checking its IDs.
//!
//! Every type is prefixed `Dp`, so a glob import never shadows a name in
//! `core_model`.
//!
//! **Reading** is lenient where it can be and reports what it leaves out:
//!
//! - an element or attribute the model lacks is skipped and counted
//!   ([`UNKNOWN_PARTS`]; a namespace declaration is not one);
//! - an element with a required attribute (or child) missing or unreadable is
//!   left out with its contents ([`UNREADABLE_PARTS`], the element's name as
//!   the detail), as is a timeline nested deeper than
//!   [`MAX_TIMELINE_NESTING`](crate::limits::MAX_TIMELINE_NESTING);
//! - an optional attribute that cannot be read is dropped
//!   ([`UNREADABLE_VALUES`], `Element@attribute` as the detail);
//! - every string is cleaned by [`text::sanitize`](crate::text::sanitize);
//! - references are kept as written: [`check_ids`] reports the ones that name
//!   nothing or the wrong kind of part, and the mappers treat them as absent;
//! - text the tree cut or dropped is not reported here: it is kept in
//!   [`DpMetaData::cut`] and `dropped_text`, for the import mappers to
//!   report.
//!
//! **Writing** keeps the XSD's element order, writes `id` only on the types
//! the XSD makes referenceable and which have one here (never on clips,
//! markers, notes, points and scenes: nothing refers to them), and omits
//! what the XSD cannot hold from the model: [`DpTimeline::Video`] (its
//! media file is not modelled), [`DpTimeline::Unknown`], an empty `markers`
//! timeline and a built-in device's [`DpDevice::settings`] (text for the
//! report).

use crate::IoError;
use crate::limits::{Limits, MAX_TIMELINE_NESTING};
use crate::report::{ItemKind, Report};
use crate::text::sanitize;
use crate::xml::{self, Attr, NodeId, Tree, XmlWriter};
use crate::zip::{NOT_DAWPROJECT, damaged};
use std::collections::HashMap;

/// The report item for an element or attribute the model lacks.
pub const UNKNOWN_PARTS: &str = "Parts of the file VizDAW doesn't know";
/// The report item for an element left out because it could not be read.
pub const UNREADABLE_PARTS: &str = "Parts of the file that could not be read";
/// The report item for an optional value that could not be read.
pub const UNREADABLE_VALUES: &str = "Values in the file that could not be read";
/// The report item for a reference to a part that is not there.
pub const MISSING_REFERENCES: &str = "References to parts that are not in the file";

/// Most settings kept as text for one built-in device.
const MAX_SETTINGS: usize = 64;
/// Longest one setting's text, in bytes.
const MAX_SETTING_BYTES: usize = 512;
/// Deepest element of a setting that is summarised.
const SETTING_DEPTH: usize = 3;

/// A fieldless enum written as XML attribute text.
macro_rules! string_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($var:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $(#[doc = concat!("`", $text, "`.")] $var,)+
        }

        impl $name {
            fn parse(s: &str) -> Option<Self> {
                match s.trim_matches([' ', '\t', '\n', '\r']) {
                    $($text => Some(Self::$var),)+
                    _ => None,
                }
            }

            fn text(self) -> &'static str {
                match self {
                    $(Self::$var => $text,)+
                }
            }
        }
    };
}

string_enum! {
    /// A parameter's unit.
    pub enum DpUnit {
        Linear = "linear",
        Normalized = "normalized",
        Percent = "percent",
        Decibel = "decibel",
        Hertz = "hertz",
        Semitones = "semitones",
        Seconds = "seconds",
        Beats = "beats",
        Bpm = "bpm",
    }
}

string_enum! {
    /// What a time is measured in.
    pub enum DpTimeUnit {
        Beats = "beats",
        Seconds = "seconds",
    }
}

string_enum! {
    /// What a track holds.
    pub enum DpContentType {
        Audio = "audio",
        Automation = "automation",
        Notes = "notes",
        Video = "video",
        Markers = "markers",
        Tracks = "tracks",
    }
}

string_enum! {
    /// What a channel does in the mixer.
    pub enum DpMixerRole {
        Regular = "regular",
        Master = "master",
        Effect = "effect",
        Submix = "submix",
        Vca = "vca",
    }
}

string_enum! {
    /// What a device does.
    pub enum DpDeviceRole {
        Instrument = "instrument",
        NoteFx = "noteFX",
        AudioFx = "audioFX",
        Analyzer = "analyzer",
    }
}

string_enum! {
    /// Where a send taps the channel.
    pub enum DpSendType {
        Pre = "pre",
        Post = "post",
    }
}

string_enum! {
    /// What an automation target controls other than a parameter.
    pub enum DpExpression {
        Gain = "gain",
        Pan = "pan",
        Transpose = "transpose",
        Timbre = "timbre",
        Formant = "formant",
        Pressure = "pressure",
        ChannelController = "channelController",
        ChannelPressure = "channelPressure",
        PolyPressure = "polyPressure",
        PitchBend = "pitchBend",
        ProgramChange = "programChange",
    }
}

string_enum! {
    /// How a real point's value continues to the next point.
    pub enum DpInterpolation {
        Hold = "hold",
        Linear = "linear",
    }
}

/// The attributes every part with a name has; `id` only where the XSD makes
/// the part referenceable.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpNamed {
    /// The `id` other parts refer to.
    pub id: Option<String>,
    /// The `name`.
    pub name: Option<String>,
    /// The `color`, `#rrggbb`.
    pub color: Option<String>,
    /// The `comment`.
    pub comment: Option<String>,
}

/// A real-valued parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct DpRealParam {
    /// Name and id.
    pub named: DpNamed,
    /// The plug-in's own parameter number.
    pub parameter_id: Option<i32>,
    /// What `value`, `min` and `max` are measured in.
    pub unit: DpUnit,
    /// The value (may be infinite).
    pub value: Option<f64>,
    /// The least value (may be infinite).
    pub min: Option<f64>,
    /// The greatest value (may be infinite).
    pub max: Option<f64>,
}

/// An on/off parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct DpBoolParam {
    /// Name and id.
    pub named: DpNamed,
    /// The plug-in's own parameter number.
    pub parameter_id: Option<i32>,
    /// The value.
    pub value: Option<bool>,
}

/// A time signature parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct DpTimeSigParam {
    /// Name and id.
    pub named: DpNamed,
    /// Beats per bar.
    pub numerator: i32,
    /// The note value of a beat.
    pub denominator: i32,
}

/// A whole project file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpProject {
    /// `Project@version`, as written.
    pub version: String,
    /// The application's name and version.
    pub application: Option<(String, String)>,
    /// The transport's tempo.
    pub tempo: Option<DpRealParam>,
    /// The transport's time signature.
    pub time_signature: Option<DpTimeSigParam>,
    /// Tracks and bare channels, in order.
    pub structure: Vec<DpStructureItem>,
    /// The arrangement.
    pub arrangement: Option<DpArrangement>,
    /// The scenes.
    pub scenes: Vec<DpScene>,
    /// How many runs of text the tree dropped for being in an element with
    /// child elements ([`Tree::dropped_text`]); for the mappers to report.
    pub dropped_text: usize,
}

/// One entry of the structure.
#[derive(Debug, Clone, PartialEq)]
pub enum DpStructureItem {
    /// A track.
    Track(DpTrack),
    /// A channel with no track.
    Channel(DpChannel),
}

/// A track.
#[derive(Debug, Clone, PartialEq)]
pub struct DpTrack {
    /// Name and id.
    pub named: DpNamed,
    /// What it holds.
    pub content: Vec<DpContentType>,
    /// Whether it is loaded.
    pub loaded: Option<bool>,
    /// Its mixer channel.
    pub channel: Option<DpChannel>,
    /// The tracks inside it.
    pub children: Vec<DpTrack>,
}

/// A mixer channel.
#[derive(Debug, Clone, PartialEq)]
pub struct DpChannel {
    /// Name and id.
    pub named: DpNamed,
    /// Its role.
    pub role: Option<DpMixerRole>,
    /// How many audio channels it carries.
    pub audio_channels: Option<i32>,
    /// The id of the channel it plays into.
    pub destination: Option<String>,
    /// Whether it is soloed.
    pub solo: Option<bool>,
    /// Its devices, in order.
    pub devices: Vec<DpDevice>,
    /// Its mute.
    pub mute: Option<DpBoolParam>,
    /// Its pan.
    pub pan: Option<DpRealParam>,
    /// Its sends.
    pub sends: Vec<DpSend>,
    /// Its volume.
    pub volume: Option<DpRealParam>,
}

/// Which element a device was or is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DpDeviceKind {
    /// `AuPlugin`.
    Au,
    /// `ClapPlugin`.
    Clap,
    /// `Vst2Plugin`.
    Vst2,
    /// `Vst3Plugin`.
    Vst3,
    /// `BuiltinDevice`.
    Builtin,
    /// `Equalizer`.
    Equalizer,
    /// `Compressor`.
    Compressor,
    /// `NoiseGate`.
    NoiseGate,
    /// `Limiter`.
    Limiter,
    /// `Device`.
    Other,
}

impl DpDeviceKind {
    const ALL: [Self; 10] = [
        Self::Au,
        Self::Clap,
        Self::Vst2,
        Self::Vst3,
        Self::Builtin,
        Self::Equalizer,
        Self::Compressor,
        Self::NoiseGate,
        Self::Limiter,
        Self::Other,
    ];

    fn element(self) -> &'static str {
        match self {
            Self::Au => "AuPlugin",
            Self::Clap => "ClapPlugin",
            Self::Vst2 => "Vst2Plugin",
            Self::Vst3 => "Vst3Plugin",
            Self::Builtin => "BuiltinDevice",
            Self::Equalizer => "Equalizer",
            Self::Compressor => "Compressor",
            Self::NoiseGate => "NoiseGate",
            Self::Limiter => "Limiter",
            Self::Other => "Device",
        }
    }

    fn of(element: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.element() == element)
    }

    /// Whether the XSD gives it `pluginVersion`.
    fn is_plugin(self) -> bool {
        matches!(self, Self::Au | Self::Clap | Self::Vst2 | Self::Vst3)
    }

    /// Whether the XSD gives it children of its own after the common ones.
    fn has_settings(self) -> bool {
        matches!(
            self,
            Self::Equalizer | Self::Compressor | Self::NoiseGate | Self::Limiter
        )
    }
}

/// A file inside the package, or outside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpFileRef {
    /// The path, as written.
    pub path: String,
    /// Whether it is outside the package.
    pub external: bool,
}

/// A device: a plug-in or a built-in effect.
#[derive(Debug, Clone, PartialEq)]
pub struct DpDevice {
    /// Which element it is.
    pub kind: DpDeviceKind,
    /// Name and id.
    pub named: DpNamed,
    /// The plug-in's own ID.
    pub device_id: Option<String>,
    /// Its name.
    pub device_name: String,
    /// Its role.
    pub role: DpDeviceRole,
    /// Its vendor.
    pub vendor: Option<String>,
    /// A plug-in's version (`pluginVersion`).
    pub version: Option<String>,
    /// Whether it is loaded.
    pub loaded: Option<bool>,
    /// Its real parameters.
    pub parameters: Vec<DpRealParam>,
    /// Its on/off.
    pub enabled: Option<DpBoolParam>,
    /// Its saved state.
    pub state: Option<DpFileRef>,
    /// A built-in device's bands and other settings, as text for the report.
    /// Not written.
    pub settings: Vec<String>,
}

/// A send.
#[derive(Debug, Clone, PartialEq)]
pub struct DpSend {
    /// Name and id.
    pub named: DpNamed,
    /// The id of the channel it plays into.
    pub destination: Option<String>,
    /// Where it taps the channel.
    pub kind: Option<DpSendType>,
    /// Its on/off.
    pub enable: Option<DpBoolParam>,
    /// Its pan.
    pub pan: Option<DpRealParam>,
    /// Its level.
    pub volume: DpRealParam,
}

/// The arrangement.
#[derive(Debug, Clone, PartialEq)]
pub struct DpArrangement {
    /// Name and id.
    pub named: DpNamed,
    /// Its timelines.
    pub lanes: Option<DpLanes>,
    /// Its markers.
    pub markers: Vec<DpMarker>,
    /// The tempo automation.
    pub tempo_automation: Option<DpPoints>,
    /// The time signature automation.
    pub time_signature_automation: Option<DpPoints>,
}

/// What every timeline has.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpHead {
    /// Name and id.
    pub named: DpNamed,
    /// What its times are measured in (inherited when absent).
    pub time_unit: Option<DpTimeUnit>,
    /// The id of the track it belongs to.
    pub track: Option<String>,
}

/// A timeline.
#[derive(Debug, Clone, PartialEq)]
pub enum DpTimeline {
    /// Timelines side by side.
    Lanes(DpLanes),
    /// Clips.
    Clips(DpClips),
    /// Notes.
    Notes(DpNotes),
    /// Automation points.
    Points(DpPoints),
    /// An audio file.
    Audio(DpAudio),
    /// An audio file's warps.
    Warps(DpWarps),
    /// Markers.
    Markers(DpHead, Vec<DpMarker>),
    /// A clip slot with at most one clip.
    ClipSlot(DpHead, Option<Box<DpClip>>),
    /// A video (only its head is kept).
    Video(DpHead),
    /// An element this model lacks, in a place for a timeline; its name.
    Unknown(String),
}

/// Timelines side by side.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpLanes {
    /// Its head.
    pub head: DpHead,
    /// The timelines in it.
    pub items: Vec<DpTimeline>,
}

/// Clips.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpClips {
    /// Its head.
    pub head: DpHead,
    /// The clips in it.
    pub clips: Vec<DpClip>,
}

/// A clip. `named.id` is never read or written.
#[derive(Debug, Clone, PartialEq)]
pub struct DpClip {
    /// Its name (and a placeholder id).
    pub named: DpNamed,
    /// Where it starts.
    pub time: f64,
    /// How long it is.
    pub duration: Option<f64>,
    /// What the times inside its content are measured in.
    pub content_time_unit: Option<DpTimeUnit>,
    /// Where in its content it starts playing.
    pub play_start: Option<f64>,
    /// Where in its content it stops playing.
    pub play_stop: Option<f64>,
    /// Where in its content its loop starts.
    pub loop_start: Option<f64>,
    /// Where in its content its loop ends.
    pub loop_end: Option<f64>,
    /// What the fade times are measured in.
    pub fade_time_unit: Option<DpTimeUnit>,
    /// The fade-in time (negative: a crossfade).
    pub fade_in: Option<f64>,
    /// The fade-out time.
    pub fade_out: Option<f64>,
    /// Whether it plays.
    pub enable: Option<bool>,
    /// The id of the timeline it plays instead of holding one.
    pub reference: Option<String>,
    /// The timeline it holds.
    pub content: Option<Box<DpTimeline>>,
}

/// Notes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpNotes {
    /// Its head.
    pub head: DpHead,
    /// The notes in it.
    pub notes: Vec<DpNote>,
}

/// A note.
#[derive(Debug, Clone, PartialEq)]
pub struct DpNote {
    /// Where it starts.
    pub time: f64,
    /// How long it is.
    pub duration: f64,
    /// Its MIDI channel.
    pub channel: i32,
    /// Its key.
    pub key: i32,
    /// Its velocity, 0 to 1.
    pub vel: Option<f64>,
    /// Its release velocity, 0 to 1.
    pub rel: Option<f64>,
    /// Its expression automation.
    pub content: Option<Box<DpTimeline>>,
}

/// What a points timeline automates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpTarget {
    /// The id of a parameter.
    pub parameter: Option<String>,
    /// Or an expression.
    pub expression: Option<DpExpression>,
    /// The MIDI channel it applies to.
    pub channel: Option<i32>,
    /// The key it applies to.
    pub key: Option<i32>,
    /// The controller number.
    pub controller: Option<i32>,
}

/// A point's value.
#[derive(Debug, Clone, PartialEq)]
pub enum DpPointValue {
    /// A real value and how it continues.
    Real(f64, Option<DpInterpolation>),
    /// An on/off value.
    Bool(bool),
    /// An integer value.
    Integer(i32),
    /// An enumeration value.
    Enum(i32),
    /// A time signature: numerator and denominator.
    TimeSig(i32, i32),
}

/// An automation point.
#[derive(Debug, Clone, PartialEq)]
pub struct DpPoint {
    /// Where it is.
    pub time: f64,
    /// Its value.
    pub value: DpPointValue,
}

/// Automation points.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpPoints {
    /// Its head.
    pub head: DpHead,
    /// What the values are measured in.
    pub unit: Option<DpUnit>,
    /// What it automates.
    pub target: DpTarget,
    /// The points, in order.
    pub points: Vec<DpPoint>,
}

/// An audio file.
#[derive(Debug, Clone, PartialEq)]
pub struct DpAudio {
    /// Its head.
    pub head: DpHead,
    /// Its length, in seconds.
    pub duration: f64,
    /// Its channels.
    pub channels: i32,
    /// Its sample rate.
    pub sample_rate: i32,
    /// The stretch algorithm.
    pub algorithm: Option<String>,
    /// The file.
    pub file: DpFileRef,
}

/// A timeline and the warps that map its time.
#[derive(Debug, Clone, PartialEq)]
pub struct DpWarps {
    /// Its head.
    pub head: DpHead,
    /// What the content's times are measured in.
    pub content_time_unit: DpTimeUnit,
    /// The warped timeline.
    pub content: Box<DpTimeline>,
    /// Pairs of time and content time.
    pub warps: Vec<(f64, f64)>,
}

/// A marker. `named.id` is never read or written.
#[derive(Debug, Clone, PartialEq)]
pub struct DpMarker {
    /// Its name (and a placeholder id).
    pub named: DpNamed,
    /// Where it is.
    pub time: f64,
}

/// A scene. `named.id` is written never; it is read as written.
#[derive(Debug, Clone, PartialEq)]
pub struct DpScene {
    /// Its name (and a read id).
    pub named: DpNamed,
    /// What it plays.
    pub content: Option<DpTimeline>,
}

/// The names of `metadata.xml`'s fields, in the XSD's order, which is
/// `SongInfo::fields`' order.
pub const METADATA_FIELDS: [&str; 13] = [
    "Title",
    "Artist",
    "Album",
    "OriginalArtist",
    "Composer",
    "Songwriter",
    "Producer",
    "Arranger",
    "Year",
    "Genre",
    "Copyright",
    "Website",
    "Comment",
];

/// A whole metadata file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DpMetaData {
    /// The fields' texts, in [`METADATA_FIELDS`]' order.
    pub fields: [Option<String>; 13],
    /// Which texts the tree cut at its limit ([`Tree::text_cut`]); for the
    /// mappers to report.
    pub cut: [bool; 13],
    /// How many runs of text the tree dropped for being in an element with
    /// child elements; for the mappers to report.
    pub dropped_text: usize,
}

// ---------------------------------------------------------------- reading

/// `s` with control characters and bidirectional overrides dropped.
fn clean(s: &str) -> String {
    sanitize(s.as_bytes(), &Limits::default()).0
}

const NAMED: [&str; 4] = ["id", "name", "color", "comment"];
const NAMEABLE: [&str; 3] = ["name", "color", "comment"];
const HEAD: [&str; 6] = ["id", "name", "color", "comment", "timeUnit", "track"];

/// Element names that are timelines (`markers` is the XSD's spelling inside
/// a timeline, `Markers` the arrangement's).
fn is_timeline(name: &str) -> bool {
    matches!(
        name,
        "Lanes"
            | "Clips"
            | "Notes"
            | "Points"
            | "Audio"
            | "Warps"
            | "Markers"
            | "markers"
            | "ClipSlot"
            | "Video"
    )
}

struct Rd<'a> {
    t: &'a Tree,
    rep: &'a mut Report,
}

impl<'a> Rd<'a> {
    fn unknown(&mut self, detail: String) {
        self.rep.add(ItemKind::Note, UNKNOWN_PARTS, Some(detail));
    }

    fn unknown_element(&mut self, n: NodeId) {
        self.unknown(clean(self.t.name(n)));
    }

    /// Notes every attribute of `n` that is in none of `known`.
    fn check_attrs(&mut self, n: NodeId, known: &[&[&str]]) {
        let t = self.t;
        for (k, _) in t.attrs(n) {
            if k.starts_with("xmlns") || known.iter().any(|l| l.contains(&k)) {
                continue;
            }
            self.unknown(format!("{}@{}", clean(t.name(n)), clean(k)));
        }
    }

    /// Notes every child element of `n` that is not named in `known`.
    fn check_children(&mut self, n: NodeId, known: &[&str]) {
        let t = self.t;
        for c in t.children(n) {
            if !known.contains(&t.name(c)) {
                self.unknown_element(c);
            }
        }
    }

    /// An element with no attributes and children of its own.
    fn leaf(&mut self, n: NodeId, known: &[&[&str]]) {
        self.check_attrs(n, known);
        self.check_children(n, &[]);
    }

    fn left_out(&mut self, n: NodeId) {
        let name = clean(self.t.name(n));
        self.rep
            .add(ItemKind::LeftOut, UNREADABLE_PARTS, Some(name));
    }

    fn bad_value(&mut self, n: NodeId, attr: &str) {
        let at = format!("{}@{attr}", clean(self.t.name(n)));
        self.rep.add(ItemKind::LeftOut, UNREADABLE_VALUES, Some(at));
    }

    fn string(&self, n: NodeId, a: &str) -> Option<String> {
        self.t.attr(n, a).map(clean)
    }

    /// An optional attribute read by `parse`; one that is there but unreadable
    /// is reported.
    fn opt<T>(&mut self, n: NodeId, a: &str, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
        let v = self.t.attr(n, a)?;
        let got = parse(v);
        if got.is_none() {
            self.bad_value(n, a);
        }
        got
    }

    fn double(&mut self, n: NodeId, a: &str) -> Option<f64> {
        self.opt(n, a, xml::double)
    }

    fn int(&mut self, n: NodeId, a: &str) -> Option<i32> {
        self.opt(n, a, xml::int)
    }

    fn boolean(&mut self, n: NodeId, a: &str) -> Option<bool> {
        self.opt(n, a, xml::boolean)
    }

    fn req<T>(&self, n: NodeId, a: &str, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
        self.t.attr(n, a).and_then(parse)
    }

    fn named(&self, n: NodeId, id: bool) -> DpNamed {
        DpNamed {
            id: if id { self.string(n, "id") } else { None },
            name: self.string(n, "name"),
            color: self.string(n, "color"),
            comment: self.string(n, "comment"),
        }
    }

    fn head(&mut self, n: NodeId) -> DpHead {
        DpHead {
            named: self.named(n, true),
            time_unit: self.opt(n, "timeUnit", DpTimeUnit::parse),
            track: self.string(n, "track"),
        }
    }

    // ---- parameters

    fn real(&mut self, n: NodeId) -> Option<DpRealParam> {
        let Some(unit) = self.req(n, "unit", DpUnit::parse) else {
            self.left_out(n);
            return None;
        };
        self.leaf(
            n,
            &[&NAMED, &["parameterID", "unit", "value", "min", "max"]],
        );
        Some(DpRealParam {
            named: self.named(n, true),
            parameter_id: self.int(n, "parameterID"),
            unit,
            value: self.opt(n, "value", xml::double_or_inf),
            min: self.opt(n, "min", xml::double_or_inf),
            max: self.opt(n, "max", xml::double_or_inf),
        })
    }

    fn bool_param(&mut self, n: NodeId) -> DpBoolParam {
        self.leaf(n, &[&NAMED, &["parameterID", "value"]]);
        DpBoolParam {
            named: self.named(n, true),
            parameter_id: self.int(n, "parameterID"),
            value: self.boolean(n, "value"),
        }
    }

    fn time_sig(&mut self, n: NodeId) -> Option<DpTimeSigParam> {
        let (Some(numerator), Some(denominator)) = (
            self.req(n, "numerator", xml::int),
            self.req(n, "denominator", xml::int),
        ) else {
            self.left_out(n);
            return None;
        };
        self.leaf(n, &[&NAMED, &["numerator", "denominator"]]);
        Some(DpTimeSigParam {
            named: self.named(n, true),
            numerator,
            denominator,
        })
    }

    fn file_ref(&mut self, n: NodeId) -> Option<DpFileRef> {
        let Some(path) = self.string(n, "path") else {
            self.left_out(n);
            return None;
        };
        self.leaf(n, &[&["path", "external"]]);
        Some(DpFileRef {
            path,
            external: self.boolean(n, "external").unwrap_or(false),
        })
    }

    // ---- structure

    fn track(&mut self, n: NodeId) -> DpTrack {
        let t = self.t;
        self.check_attrs(n, &[&NAMED, &["contentType", "loaded"]]);
        let mut content = Vec::new();
        for word in t
            .attr(n, "contentType")
            .unwrap_or_default()
            .split_whitespace()
        {
            match DpContentType::parse(word) {
                Some(c) => content.push(c),
                None => self.unknown(format!("Track@contentType {}", clean(word))),
            }
        }
        let mut track = DpTrack {
            named: self.named(n, true),
            content,
            loaded: self.boolean(n, "loaded"),
            channel: None,
            children: Vec::new(),
        };
        for c in t.children(n) {
            match t.name(c) {
                "Channel" if track.channel.is_none() => track.channel = Some(self.channel(c)),
                "Track" => track.children.push(self.track(c)),
                _ => self.unknown_element(c),
            }
        }
        track
    }

    fn channel(&mut self, n: NodeId) -> DpChannel {
        let t = self.t;
        self.check_attrs(
            n,
            &[&NAMED, &["role", "audioChannels", "destination", "solo"]],
        );
        let mut ch = DpChannel {
            named: self.named(n, true),
            role: self.opt(n, "role", DpMixerRole::parse),
            audio_channels: self.int(n, "audioChannels"),
            destination: self.string(n, "destination"),
            solo: self.boolean(n, "solo"),
            devices: Vec::new(),
            mute: None,
            pan: None,
            sends: Vec::new(),
            volume: None,
        };
        let mut seen = [false; 5];
        for c in t.children(n) {
            let (slot, name) = match t.name(c) {
                "Devices" => (0, "Devices"),
                "Mute" => (1, "Mute"),
                "Pan" => (2, "Pan"),
                "Sends" => (3, "Sends"),
                "Volume" => (4, "Volume"),
                _ => {
                    self.unknown_element(c);
                    continue;
                }
            };
            if seen.get(slot).copied().unwrap_or(true) {
                self.unknown_element(c);
                continue;
            }
            if let Some(s) = seen.get_mut(slot) {
                *s = true;
            }
            match name {
                "Devices" => self.devices(c, &mut ch.devices),
                "Mute" => ch.mute = Some(self.bool_param(c)),
                "Pan" => ch.pan = self.real(c),
                "Sends" => self.sends(c, &mut ch.sends),
                _ => ch.volume = self.real(c),
            }
        }
        ch
    }

    fn devices(&mut self, n: NodeId, out: &mut Vec<DpDevice>) {
        let t = self.t;
        self.check_attrs(n, &[]);
        for c in t.children(n) {
            match DpDeviceKind::of(t.name(c)) {
                Some(kind) => out.extend(self.device(c, kind)),
                None => self.unknown_element(c),
            }
        }
    }

    fn device(&mut self, n: NodeId, kind: DpDeviceKind) -> Option<DpDevice> {
        let t = self.t;
        let (Some(device_name), Some(role)) = (
            self.string(n, "deviceName"),
            self.req(n, "deviceRole", DpDeviceRole::parse),
        ) else {
            self.left_out(n);
            return None;
        };
        let version_attr: &[&str] = if kind.is_plugin() {
            &["pluginVersion"]
        } else {
            &[]
        };
        self.check_attrs(
            n,
            &[
                &NAMED,
                &[
                    "deviceID",
                    "deviceName",
                    "deviceRole",
                    "deviceVendor",
                    "loaded",
                ],
                version_attr,
            ],
        );
        let mut d = DpDevice {
            kind,
            named: self.named(n, true),
            device_id: self.string(n, "deviceID"),
            device_name,
            role,
            vendor: self.string(n, "deviceVendor"),
            version: if kind.is_plugin() {
                self.string(n, "pluginVersion")
            } else {
                None
            },
            loaded: self.boolean(n, "loaded"),
            parameters: Vec::new(),
            enabled: None,
            state: None,
            settings: Vec::new(),
        };
        let mut seen = [false; 3];
        for c in t.children(n) {
            let slot = match t.name(c) {
                "Parameters" => 0,
                "Enabled" => 1,
                "State" => 2,
                _ if kind.has_settings() => {
                    if d.settings.len() < MAX_SETTINGS {
                        d.settings.push(self.setting(c));
                    } else {
                        self.unknown_element(c);
                    }
                    continue;
                }
                _ => {
                    self.unknown_element(c);
                    continue;
                }
            };
            if seen.get(slot).copied().unwrap_or(true) {
                self.unknown_element(c);
                continue;
            }
            if let Some(s) = seen.get_mut(slot) {
                *s = true;
            }
            match slot {
                0 => self.parameters(c, &mut d.parameters),
                1 => d.enabled = Some(self.bool_param(c)),
                _ => d.state = self.file_ref(c),
            }
        }
        Some(d)
    }

    fn parameters(&mut self, n: NodeId, out: &mut Vec<DpRealParam>) {
        let t = self.t;
        self.check_attrs(n, &[]);
        for c in t.children(n) {
            if t.name(c) == "RealParameter" {
                out.extend(self.real(c));
            } else {
                self.unknown_element(c);
            }
        }
    }

    /// A built-in device's element as one line of text.
    fn setting(&self, n: NodeId) -> String {
        fn go(t: &Tree, n: NodeId, depth: usize, out: &mut String) {
            out.push_str(t.name(n));
            for (k, v) in t.attrs(n) {
                out.push(' ');
                out.push_str(k);
                out.push('=');
                out.push_str(v);
            }
            if depth > 1 {
                let mut first = true;
                for c in t.children(n) {
                    out.push_str(if first { ": " } else { ", " });
                    first = false;
                    go(t, c, depth - 1, out);
                }
            }
        }
        let mut raw = String::new();
        go(self.t, n, SETTING_DEPTH, &mut raw);
        let mut cleaned = clean(&raw);
        if cleaned.len() > MAX_SETTING_BYTES {
            let mut cut = MAX_SETTING_BYTES;
            while !cleaned.is_char_boundary(cut) {
                cut -= 1;
            }
            cleaned.truncate(cut);
        }
        cleaned
    }

    fn sends(&mut self, n: NodeId, out: &mut Vec<DpSend>) {
        let t = self.t;
        self.check_attrs(n, &[]);
        for c in t.children(n) {
            if t.name(c) == "Send" {
                out.extend(self.send(c));
            } else {
                self.unknown_element(c);
            }
        }
    }

    fn send(&mut self, n: NodeId) -> Option<DpSend> {
        let t = self.t;
        // The level is required; read the children first.
        let mut enable = None;
        let mut pan = None;
        let mut volume = None;
        let mut volume_seen = false;
        let mut pan_seen = false;
        let mut enable_seen = false;
        let mut unreadable = false;
        for c in t.children(n) {
            match t.name(c) {
                "Enable" if !enable_seen => {
                    enable_seen = true;
                    enable = Some(self.bool_param(c));
                }
                "Pan" if !pan_seen => {
                    pan_seen = true;
                    pan = self.real(c);
                }
                "Volume" if !volume_seen => {
                    volume_seen = true;
                    volume = self.real(c);
                    unreadable |= volume.is_none();
                }
                _ => self.unknown_element(c),
            }
        }
        let Some(volume) = volume else {
            // A volume that failed was reported itself.
            if !unreadable {
                self.left_out(n);
            }
            return None;
        };
        self.check_attrs(n, &[&NAMED, &["destination", "type"]]);
        Some(DpSend {
            named: self.named(n, true),
            destination: self.string(n, "destination"),
            kind: self.opt(n, "type", DpSendType::parse),
            enable,
            pan,
            volume,
        })
    }

    // ---- arrangement and timelines

    fn arrangement(&mut self, n: NodeId) -> DpArrangement {
        let t = self.t;
        self.check_attrs(n, &[&NAMED]);
        let mut a = DpArrangement {
            named: self.named(n, true),
            lanes: None,
            markers: Vec::new(),
            tempo_automation: None,
            time_signature_automation: None,
        };
        let mut seen = [false; 4];
        for c in t.children(n) {
            let slot = match t.name(c) {
                "Lanes" => 0,
                "Markers" => 1,
                "TempoAutomation" => 2,
                "TimeSignatureAutomation" => 3,
                _ => {
                    self.unknown_element(c);
                    continue;
                }
            };
            if seen.get(slot).copied().unwrap_or(true) {
                self.unknown_element(c);
                continue;
            }
            if let Some(s) = seen.get_mut(slot) {
                *s = true;
            }
            match slot {
                0 => a.lanes = self.lanes(c, 1),
                1 => a.markers = self.markers(c).1,
                2 => a.tempo_automation = self.points(c),
                _ => a.time_signature_automation = self.points(c),
            }
        }
        a
    }

    /// A timeline element at nesting `level` (1 is the outermost); `None`
    /// when it was left out.
    fn timeline(&mut self, n: NodeId, level: usize) -> Option<DpTimeline> {
        let name = self.t.name(n);
        if !is_timeline(name) {
            self.unknown_element(n);
            return Some(DpTimeline::Unknown(clean(name)));
        }
        if level > MAX_TIMELINE_NESTING {
            self.left_out(n);
            return None;
        }
        Some(match name {
            "Lanes" => DpTimeline::Lanes(self.lanes(n, level)?),
            "Clips" => DpTimeline::Clips(self.clips(n, level)?),
            "Notes" => DpTimeline::Notes(self.notes(n, level)),
            "Points" => DpTimeline::Points(self.points(n)?),
            "Audio" => DpTimeline::Audio(self.audio(n)?),
            "Warps" => DpTimeline::Warps(self.warps(n, level)?),
            "ClipSlot" => {
                let t = self.t;
                self.check_attrs(n, &[&HEAD]);
                let head = self.head(n);
                let mut clip = None;
                for c in t.children(n) {
                    if t.name(c) == "Clip" && clip.is_none() {
                        clip = self.clip(c, level + 1).map(Box::new);
                    } else {
                        self.unknown_element(c);
                    }
                }
                DpTimeline::ClipSlot(head, clip)
            }
            "Video" => {
                // Its media file is not modelled; the mappers report a video.
                self.check_attrs(
                    n,
                    &[&HEAD, &["duration", "algorithm", "channels", "sampleRate"]],
                );
                self.check_children(n, &["File"]);
                DpTimeline::Video(self.head(n))
            }
            _ => {
                let (head, markers) = self.markers(n);
                DpTimeline::Markers(head, markers)
            }
        })
    }

    /// The first child of `n` as a timeline at `level`; later children are
    /// unknown.
    fn content(&mut self, n: NodeId, level: usize) -> Option<Box<DpTimeline>> {
        let t = self.t;
        let mut content = None;
        for (i, c) in t.children(n).enumerate() {
            if i == 0 {
                content = self.timeline(c, level).map(Box::new);
            } else {
                self.unknown_element(c);
            }
        }
        content
    }

    fn lanes(&mut self, n: NodeId, level: usize) -> Option<DpLanes> {
        let t = self.t;
        self.check_attrs(n, &[&HEAD]);
        let head = self.head(n);
        let mut items = Vec::new();
        for c in t.children(n) {
            items.extend(self.timeline(c, level + 1));
        }
        Some(DpLanes { head, items })
    }

    fn clips(&mut self, n: NodeId, level: usize) -> Option<DpClips> {
        let t = self.t;
        self.check_attrs(n, &[&HEAD]);
        let head = self.head(n);
        let mut clips = Vec::new();
        for c in t.children(n) {
            if t.name(c) == "Clip" {
                clips.extend(self.clip(c, level + 1));
            } else {
                self.unknown_element(c);
            }
        }
        Some(DpClips { head, clips })
    }

    /// A clip whose content would be at `level`.
    fn clip(&mut self, n: NodeId, level: usize) -> Option<DpClip> {
        let Some(time) = self.req(n, "time", xml::double) else {
            self.left_out(n);
            return None;
        };
        self.check_attrs(
            n,
            &[
                &NAMEABLE,
                &[
                    "time",
                    "duration",
                    "contentTimeUnit",
                    "playStart",
                    "playStop",
                    "loopStart",
                    "loopEnd",
                    "fadeTimeUnit",
                    "fadeInTime",
                    "fadeOutTime",
                    "enable",
                    "reference",
                ],
            ],
        );
        Some(DpClip {
            named: self.named(n, false),
            time,
            duration: self.double(n, "duration"),
            content_time_unit: self.opt(n, "contentTimeUnit", DpTimeUnit::parse),
            play_start: self.double(n, "playStart"),
            play_stop: self.double(n, "playStop"),
            loop_start: self.double(n, "loopStart"),
            loop_end: self.double(n, "loopEnd"),
            fade_time_unit: self.opt(n, "fadeTimeUnit", DpTimeUnit::parse),
            fade_in: self.double(n, "fadeInTime"),
            fade_out: self.double(n, "fadeOutTime"),
            enable: self.boolean(n, "enable"),
            reference: self.string(n, "reference"),
            content: self.content(n, level),
        })
    }

    fn notes(&mut self, n: NodeId, level: usize) -> DpNotes {
        let t = self.t;
        self.check_attrs(n, &[&HEAD]);
        let head = self.head(n);
        let mut notes = Vec::new();
        for c in t.children(n) {
            if t.name(c) == "Note" {
                notes.extend(self.note(c, level + 1));
            } else {
                self.unknown_element(c);
            }
        }
        DpNotes { head, notes }
    }

    fn note(&mut self, n: NodeId, level: usize) -> Option<DpNote> {
        let (Some(time), Some(duration), Some(channel), Some(key)) = (
            self.req(n, "time", xml::double),
            self.req(n, "duration", xml::double),
            self.req(n, "channel", xml::int),
            self.req(n, "key", xml::int),
        ) else {
            self.left_out(n);
            return None;
        };
        self.check_attrs(n, &[&["time", "duration", "channel", "key", "vel", "rel"]]);
        Some(DpNote {
            time,
            duration,
            channel,
            key,
            vel: self.double(n, "vel"),
            rel: self.double(n, "rel"),
            content: self.content(n, level),
        })
    }

    fn points(&mut self, n: NodeId) -> Option<DpPoints> {
        let t = self.t;
        self.check_attrs(n, &[&HEAD, &["unit"]]);
        let head = self.head(n);
        let unit = self.opt(n, "unit", DpUnit::parse);
        let mut target = None;
        let mut points = Vec::new();
        for c in t.children(n) {
            match t.name(c) {
                "Target" if target.is_none() => target = Some(self.target(c)),
                "RealPoint" | "BoolPoint" | "IntegerPoint" | "EnumPoint" | "TimeSignaturePoint" => {
                    points.extend(self.point(c));
                }
                _ => self.unknown_element(c),
            }
        }
        Some(DpPoints {
            head,
            unit,
            target: target.unwrap_or_default(),
            points,
        })
    }

    fn target(&mut self, n: NodeId) -> DpTarget {
        self.leaf(
            n,
            &[&["parameter", "expression", "channel", "key", "controller"]],
        );
        DpTarget {
            parameter: self.string(n, "parameter"),
            expression: self.opt(n, "expression", DpExpression::parse),
            channel: self.int(n, "channel"),
            key: self.int(n, "key"),
            controller: self.int(n, "controller"),
        }
    }

    fn point(&mut self, n: NodeId) -> Option<DpPoint> {
        let time = self.req(n, "time", xml::double);
        let value = match self.t.name(n) {
            "RealPoint" => self.req(n, "value", xml::double).map(|v| {
                let interpolation = self.opt(n, "interpolation", DpInterpolation::parse);
                DpPointValue::Real(v, interpolation)
            }),
            "BoolPoint" => self.req(n, "value", xml::boolean).map(DpPointValue::Bool),
            "IntegerPoint" => self.req(n, "value", xml::int).map(DpPointValue::Integer),
            "EnumPoint" => self.req(n, "value", xml::int).map(DpPointValue::Enum),
            _ => self
                .req(n, "numerator", xml::int)
                .zip(self.req(n, "denominator", xml::int))
                .map(|(a, b)| DpPointValue::TimeSig(a, b)),
        };
        let (Some(time), Some(value)) = (time, value) else {
            self.left_out(n);
            return None;
        };
        self.leaf(
            n,
            &[&["time", "value", "interpolation", "numerator", "denominator"]],
        );
        Some(DpPoint { time, value })
    }

    fn audio(&mut self, n: NodeId) -> Option<DpAudio> {
        let t = self.t;
        let (Some(duration), Some(channels), Some(sample_rate)) = (
            self.req(n, "duration", xml::double),
            self.req(n, "channels", xml::int),
            self.req(n, "sampleRate", xml::int),
        ) else {
            self.left_out(n);
            return None;
        };
        let mut file = None;
        for c in t.children(n) {
            if t.name(c) == "File" && file.is_none() {
                file = self.file_ref(c);
            } else {
                self.unknown_element(c);
            }
        }
        let Some(file) = file else {
            self.left_out(n);
            return None;
        };
        self.check_attrs(
            n,
            &[&HEAD, &["duration", "channels", "sampleRate", "algorithm"]],
        );
        Some(DpAudio {
            head: self.head(n),
            duration,
            channels,
            sample_rate,
            algorithm: self.string(n, "algorithm"),
            file,
        })
    }

    fn warps(&mut self, n: NodeId, level: usize) -> Option<DpWarps> {
        let t = self.t;
        let Some(content_time_unit) = self.req(n, "contentTimeUnit", DpTimeUnit::parse) else {
            self.left_out(n);
            return None;
        };
        let mut content = None;
        let mut content_seen = false;
        let mut warps = Vec::new();
        for c in t.children(n) {
            if t.name(c) == "Warp" {
                let pair =
                    self.req(c, "time", xml::double)
                        .zip(self.req(c, "contentTime", xml::double));
                match pair {
                    Some(p) => {
                        self.leaf(c, &[&["time", "contentTime"]]);
                        warps.push(p);
                    }
                    None => self.left_out(c),
                }
            } else if content_seen {
                self.unknown_element(c);
            } else {
                content_seen = true;
                content = self.timeline(c, level + 1).map(Box::new);
            }
        }
        let Some(content) = content else {
            if !content_seen {
                self.left_out(n);
            }
            return None;
        };
        self.check_attrs(n, &[&HEAD, &["contentTimeUnit"]]);
        Some(DpWarps {
            head: self.head(n),
            content_time_unit,
            content,
            warps,
        })
    }

    fn markers(&mut self, n: NodeId) -> (DpHead, Vec<DpMarker>) {
        let t = self.t;
        self.check_attrs(n, &[&HEAD]);
        let mut markers = Vec::new();
        for c in t.children(n) {
            if t.name(c) == "Marker" {
                if let Some(time) = self.req(c, "time", xml::double) {
                    self.leaf(c, &[&NAMEABLE, &["time"]]);
                    markers.push(DpMarker {
                        named: self.named(c, false),
                        time,
                    });
                } else {
                    self.left_out(c);
                }
            } else {
                self.unknown_element(c);
            }
        }
        (self.head(n), markers)
    }

    fn scene(&mut self, n: NodeId) -> DpScene {
        self.check_attrs(n, &[&NAMED]);
        DpScene {
            named: self.named(n, true),
            content: self.content(n, 1).map(|b| *b),
        }
    }

    // ---- the project

    fn project(&mut self) -> Result<DpProject, IoError> {
        let t = self.t;
        let root = t.root();
        if t.name(root) != "Project" {
            return Err(IoError::Refused(NOT_DAWPROJECT.to_owned()));
        }
        let version = self
            .string(root, "version")
            .ok_or_else(|| damaged("the project has no version"))?;
        let major = version.split('.').next().and_then(xml::int);
        if major != Some(1) {
            return Err(IoError::Refused(format!(
                "This DAWproject file is version {version}; VizDAW reads version 1."
            )));
        }
        self.check_attrs(root, &[&["version"]]);
        let mut p = DpProject {
            version,
            ..DpProject::default()
        };
        let (mut app, mut transport, mut structure, mut arrangement, mut scenes) =
            (false, false, false, false, false);
        for c in t.children(root) {
            match t.name(c) {
                "Application" if !app => {
                    app = true;
                    p.application = self.application(c);
                }
                "Transport" if !transport => {
                    transport = true;
                    self.transport(c, &mut p);
                }
                "Structure" if !structure => {
                    structure = true;
                    self.structure(c, &mut p.structure);
                }
                "Arrangement" if !arrangement => {
                    arrangement = true;
                    p.arrangement = Some(self.arrangement(c));
                }
                "Scenes" if !scenes => {
                    scenes = true;
                    self.check_attrs(c, &[]);
                    for s in t.children(c) {
                        if t.name(s) == "Scene" {
                            p.scenes.push(self.scene(s));
                        } else {
                            self.unknown_element(s);
                        }
                    }
                }
                _ => self.unknown_element(c),
            }
        }
        p.dropped_text = t.dropped_text();
        Ok(p)
    }

    fn application(&mut self, n: NodeId) -> Option<(String, String)> {
        match (self.string(n, "name"), self.string(n, "version")) {
            (Some(name), Some(version)) => {
                self.leaf(n, &[&["name", "version"]]);
                Some((name, version))
            }
            _ => {
                self.left_out(n);
                None
            }
        }
    }

    fn transport(&mut self, n: NodeId, p: &mut DpProject) {
        let t = self.t;
        self.check_attrs(n, &[]);
        for c in t.children(n) {
            match t.name(c) {
                "Tempo" if p.tempo.is_none() => p.tempo = self.real(c),
                "TimeSignature" if p.time_signature.is_none() => {
                    p.time_signature = self.time_sig(c);
                }
                _ => self.unknown_element(c),
            }
        }
    }

    fn structure(&mut self, n: NodeId, out: &mut Vec<DpStructureItem>) {
        let t = self.t;
        self.check_attrs(n, &[]);
        for c in t.children(n) {
            match t.name(c) {
                "Track" => out.push(DpStructureItem::Track(self.track(c))),
                "Channel" => out.push(DpStructureItem::Channel(self.channel(c))),
                _ => self.unknown_element(c),
            }
        }
    }
}

/// Reads `t` as a DAWproject `project.xml`.
///
/// Refuses a root that is not `Project` ([`NOT_DAWPROJECT`]), a project with
/// no `version` and a version whose major number is not 1. What it leaves out
/// or does not know is counted in `report` (see the module documentation).
/// The IDs are not checked: that is [`check_ids`].
pub fn read_project(t: &Tree, report: &mut Report) -> Result<DpProject, IoError> {
    Rd { t, rep: report }.project()
}

/// Reads `t` as a DAWproject `metadata.xml`. A root that is not `MetaData`
/// gives an empty result; elements it lacks are counted in `report`.
pub fn read_metadata(t: &Tree, report: &mut Report) -> DpMetaData {
    let mut rd = Rd { t, rep: report };
    let mut m = DpMetaData::default();
    let root = t.root();
    if t.name(root) != "MetaData" {
        return m;
    }
    rd.check_attrs(root, &[]);
    m.dropped_text = t.dropped_text();
    for c in t.children(root) {
        let slot = METADATA_FIELDS.iter().position(|f| *f == t.name(c));
        let free = slot
            .and_then(|i| m.fields.get(i))
            .is_some_and(Option::is_none);
        let (Some(i), true) = (slot, free) else {
            rd.unknown_element(c);
            continue;
        };
        rd.leaf(c, &[]);
        if let Some(f) = m.fields.get_mut(i) {
            *f = Some(clean(t.text(c)));
        }
        if let Some(cut) = m.cut.get_mut(i) {
            *cut = t.text_cut(c);
        }
    }
    m
}

// ---------------------------------------------------------------- writing

type Attrs<'a> = Vec<(&'static str, Attr<'a>)>;

/// `v` as a finite number (a caller's NaN or infinity is written as 0).
fn finite(v: f64) -> f64 {
    if v.is_finite() { v } else { 0.0 }
}

/// A parameter's number: `inf` and `-inf` as the format spells them.
fn real_attr(v: f64) -> Attr<'static> {
    if v == f64::INFINITY {
        Attr::Str("inf")
    } else if v == f64::NEG_INFINITY {
        Attr::Str("-inf")
    } else {
        Attr::F64(finite(v))
    }
}

fn put_str<'a>(a: &mut Attrs<'a>, key: &'static str, v: &'a Option<String>) {
    if let Some(v) = v {
        a.push((key, Attr::Str(v)));
    }
}

fn put_f64(a: &mut Attrs<'_>, key: &'static str, v: Option<f64>) {
    if let Some(v) = v {
        a.push((key, Attr::F64(finite(v))));
    }
}

fn put_int(a: &mut Attrs<'_>, key: &'static str, v: Option<i32>) {
    if let Some(v) = v {
        a.push((key, Attr::I64(i64::from(v))));
    }
}

fn put_bool(a: &mut Attrs<'_>, key: &'static str, v: Option<bool>) {
    if let Some(v) = v {
        a.push((key, Attr::Bool(v)));
    }
}

fn put_named<'a>(a: &mut Attrs<'a>, n: &'a DpNamed, id: bool) {
    if id {
        put_str(a, "id", &n.id);
    }
    put_str(a, "name", &n.name);
    put_str(a, "color", &n.color);
    put_str(a, "comment", &n.comment);
}

fn put_head<'a>(a: &mut Attrs<'a>, h: &'a DpHead) {
    put_named(a, &h.named, true);
    if let Some(u) = h.time_unit {
        a.push(("timeUnit", Attr::Str(u.text())));
    }
    put_str(a, "track", &h.track);
}

fn start(w: &mut XmlWriter, name: &str, attrs: &Attrs<'_>, has_children: bool) {
    if has_children {
        w.open(name, attrs);
    } else {
        w.empty(name, attrs);
    }
}

fn end(w: &mut XmlWriter, has_children: bool) {
    if has_children {
        w.close();
    }
}

fn write_real(w: &mut XmlWriter, name: &str, p: &DpRealParam) {
    let mut a = Attrs::new();
    put_named(&mut a, &p.named, true);
    put_int(&mut a, "parameterID", p.parameter_id);
    a.push(("unit", Attr::Str(p.unit.text())));
    for (key, v) in [("value", p.value), ("min", p.min), ("max", p.max)] {
        if let Some(v) = v {
            a.push((key, real_attr(v)));
        }
    }
    w.empty(name, &a);
}

fn write_bool(w: &mut XmlWriter, name: &str, p: &DpBoolParam) {
    let mut a = Attrs::new();
    put_named(&mut a, &p.named, true);
    put_int(&mut a, "parameterID", p.parameter_id);
    put_bool(&mut a, "value", p.value);
    w.empty(name, &a);
}

fn write_file(w: &mut XmlWriter, name: &str, f: &DpFileRef) {
    let mut a: Attrs<'_> = vec![("path", Attr::Str(&f.path))];
    if f.external {
        a.push(("external", Attr::Bool(true)));
    }
    w.empty(name, &a);
}

fn write_device(w: &mut XmlWriter, d: &DpDevice) {
    let mut a = Attrs::new();
    put_named(&mut a, &d.named, true);
    put_str(&mut a, "deviceID", &d.device_id);
    a.push(("deviceName", Attr::Str(&d.device_name)));
    a.push(("deviceRole", Attr::Str(d.role.text())));
    put_str(&mut a, "deviceVendor", &d.vendor);
    if d.kind.is_plugin() {
        put_str(&mut a, "pluginVersion", &d.version);
    }
    put_bool(&mut a, "loaded", d.loaded);
    let has = !d.parameters.is_empty() || d.enabled.is_some() || d.state.is_some();
    start(w, d.kind.element(), &a, has);
    if !d.parameters.is_empty() {
        w.open("Parameters", &[]);
        for p in &d.parameters {
            write_real(w, "RealParameter", p);
        }
        w.close();
    }
    if let Some(e) = &d.enabled {
        write_bool(w, "Enabled", e);
    }
    if let Some(s) = &d.state {
        write_file(w, "State", s);
    }
    end(w, has);
}

fn write_send(w: &mut XmlWriter, s: &DpSend) {
    let mut a = Attrs::new();
    put_named(&mut a, &s.named, true);
    put_str(&mut a, "destination", &s.destination);
    if let Some(k) = s.kind {
        a.push(("type", Attr::Str(k.text())));
    }
    w.open("Send", &a);
    if let Some(e) = &s.enable {
        write_bool(w, "Enable", e);
    }
    if let Some(p) = &s.pan {
        write_real(w, "Pan", p);
    }
    write_real(w, "Volume", &s.volume);
    w.close();
}

fn write_channel(w: &mut XmlWriter, c: &DpChannel) {
    let mut a = Attrs::new();
    put_named(&mut a, &c.named, true);
    if let Some(r) = c.role {
        a.push(("role", Attr::Str(r.text())));
    }
    put_int(&mut a, "audioChannels", c.audio_channels);
    put_str(&mut a, "destination", &c.destination);
    put_bool(&mut a, "solo", c.solo);
    let has = !c.devices.is_empty()
        || c.mute.is_some()
        || c.pan.is_some()
        || !c.sends.is_empty()
        || c.volume.is_some();
    start(w, "Channel", &a, has);
    if !c.devices.is_empty() {
        w.open("Devices", &[]);
        for d in &c.devices {
            write_device(w, d);
        }
        w.close();
    }
    if let Some(m) = &c.mute {
        write_bool(w, "Mute", m);
    }
    if let Some(p) = &c.pan {
        write_real(w, "Pan", p);
    }
    if !c.sends.is_empty() {
        w.open("Sends", &[]);
        for s in &c.sends {
            write_send(w, s);
        }
        w.close();
    }
    if let Some(v) = &c.volume {
        write_real(w, "Volume", v);
    }
    end(w, has);
}

fn write_track(w: &mut XmlWriter, t: &DpTrack) {
    let mut a = Attrs::new();
    put_named(&mut a, &t.named, true);
    let content = t
        .content
        .iter()
        .map(|c| c.text())
        .collect::<Vec<_>>()
        .join(" ");
    if !t.content.is_empty() {
        a.push(("contentType", Attr::Str(&content)));
    }
    put_bool(&mut a, "loaded", t.loaded);
    let has = t.channel.is_some() || !t.children.is_empty();
    start(w, "Track", &a, has);
    if let Some(c) = &t.channel {
        write_channel(w, c);
    }
    for c in &t.children {
        write_track(w, c);
    }
    end(w, has);
}

fn write_marker(w: &mut XmlWriter, m: &DpMarker) {
    let mut a = Attrs::new();
    put_named(&mut a, &m.named, false);
    a.push(("time", Attr::F64(finite(m.time))));
    w.empty("Marker", &a);
}

fn write_markers(w: &mut XmlWriter, name: &str, head: &DpHead, markers: &[DpMarker]) {
    if markers.is_empty() {
        return;
    }
    let mut a = Attrs::new();
    put_head(&mut a, head);
    w.open(name, &a);
    for m in markers {
        write_marker(w, m);
    }
    w.close();
}

fn write_clip(w: &mut XmlWriter, c: &DpClip) {
    let mut a = Attrs::new();
    put_named(&mut a, &c.named, false);
    a.push(("time", Attr::F64(finite(c.time))));
    put_f64(&mut a, "duration", c.duration);
    if let Some(u) = c.content_time_unit {
        a.push(("contentTimeUnit", Attr::Str(u.text())));
    }
    put_f64(&mut a, "playStart", c.play_start);
    put_f64(&mut a, "playStop", c.play_stop);
    put_f64(&mut a, "loopStart", c.loop_start);
    put_f64(&mut a, "loopEnd", c.loop_end);
    if let Some(u) = c.fade_time_unit {
        a.push(("fadeTimeUnit", Attr::Str(u.text())));
    }
    put_f64(&mut a, "fadeInTime", c.fade_in);
    put_f64(&mut a, "fadeOutTime", c.fade_out);
    put_bool(&mut a, "enable", c.enable);
    put_str(&mut a, "reference", &c.reference);
    let has = c.content.as_deref().is_some_and(writable);
    start(w, "Clip", &a, has);
    if let Some(t) = &c.content {
        write_timeline(w, t);
    }
    end(w, has);
}

fn write_point(w: &mut XmlWriter, p: &DpPoint) {
    let mut a = Attrs::new();
    a.push(("time", Attr::F64(finite(p.time))));
    let name = match &p.value {
        DpPointValue::Real(v, i) => {
            a.push(("value", Attr::F64(finite(*v))));
            if let Some(i) = i {
                a.push(("interpolation", Attr::Str(i.text())));
            }
            "RealPoint"
        }
        DpPointValue::Bool(v) => {
            a.push(("value", Attr::Bool(*v)));
            "BoolPoint"
        }
        DpPointValue::Integer(v) => {
            a.push(("value", Attr::I64(i64::from(*v))));
            "IntegerPoint"
        }
        DpPointValue::Enum(v) => {
            a.push(("value", Attr::I64(i64::from(*v))));
            "EnumPoint"
        }
        DpPointValue::TimeSig(n, d) => {
            a.push(("numerator", Attr::I64(i64::from(*n))));
            a.push(("denominator", Attr::I64(i64::from(*d))));
            "TimeSignaturePoint"
        }
    };
    w.empty(name, &a);
}

fn write_points(w: &mut XmlWriter, name: &str, p: &DpPoints) {
    let mut a = Attrs::new();
    put_head(&mut a, &p.head);
    if let Some(u) = p.unit {
        a.push(("unit", Attr::Str(u.text())));
    }
    w.open(name, &a);
    let mut t = Attrs::new();
    put_str(&mut t, "parameter", &p.target.parameter);
    if let Some(e) = p.target.expression {
        t.push(("expression", Attr::Str(e.text())));
    }
    put_int(&mut t, "channel", p.target.channel);
    put_int(&mut t, "key", p.target.key);
    put_int(&mut t, "controller", p.target.controller);
    w.empty("Target", &t);
    for point in &p.points {
        write_point(w, point);
    }
    w.close();
}

/// Whether [`write_timeline`] writes anything for `t`.
fn writable(t: &DpTimeline) -> bool {
    match t {
        DpTimeline::Video(_) | DpTimeline::Unknown(_) => false,
        DpTimeline::Markers(_, m) => !m.is_empty(),
        _ => true,
    }
}

fn write_timeline(w: &mut XmlWriter, t: &DpTimeline) {
    match t {
        DpTimeline::Lanes(l) => write_lanes(w, l),
        DpTimeline::Clips(c) => {
            let mut a = Attrs::new();
            put_head(&mut a, &c.head);
            start(w, "Clips", &a, !c.clips.is_empty());
            for clip in &c.clips {
                write_clip(w, clip);
            }
            end(w, !c.clips.is_empty());
        }
        DpTimeline::Notes(n) => {
            let mut a = Attrs::new();
            put_head(&mut a, &n.head);
            start(w, "Notes", &a, !n.notes.is_empty());
            for note in &n.notes {
                let mut a: Attrs<'_> = vec![
                    ("time", Attr::F64(finite(note.time))),
                    ("duration", Attr::F64(finite(note.duration))),
                    ("channel", Attr::I64(i64::from(note.channel))),
                    ("key", Attr::I64(i64::from(note.key))),
                ];
                put_f64(&mut a, "vel", note.vel);
                put_f64(&mut a, "rel", note.rel);
                let has = note.content.as_deref().is_some_and(writable);
                start(w, "Note", &a, has);
                if let Some(c) = &note.content {
                    write_timeline(w, c);
                }
                end(w, has);
            }
            end(w, !n.notes.is_empty());
        }
        DpTimeline::Points(p) => write_points(w, "Points", p),
        DpTimeline::Audio(x) => {
            let mut a = Attrs::new();
            put_head(&mut a, &x.head);
            a.push(("duration", Attr::F64(finite(x.duration))));
            a.push(("channels", Attr::I64(i64::from(x.channels))));
            a.push(("sampleRate", Attr::I64(i64::from(x.sample_rate))));
            put_str(&mut a, "algorithm", &x.algorithm);
            w.open("Audio", &a);
            write_file(w, "File", &x.file);
            w.close();
        }
        DpTimeline::Warps(x) => {
            let mut a = Attrs::new();
            put_head(&mut a, &x.head);
            a.push(("contentTimeUnit", Attr::Str(x.content_time_unit.text())));
            w.open("Warps", &a);
            write_timeline(w, &x.content);
            for (time, content) in &x.warps {
                let a: Attrs<'_> = vec![
                    ("time", Attr::F64(finite(*time))),
                    ("contentTime", Attr::F64(finite(*content))),
                ];
                w.empty("Warp", &a);
            }
            w.close();
        }
        DpTimeline::Markers(h, m) => write_markers(w, "markers", h, m),
        DpTimeline::ClipSlot(h, c) => {
            let mut a = Attrs::new();
            put_head(&mut a, h);
            start(w, "ClipSlot", &a, c.is_some());
            if let Some(c) = c {
                write_clip(w, c);
            }
            end(w, c.is_some());
        }
        DpTimeline::Video(_) | DpTimeline::Unknown(_) => {}
    }
}

fn write_lanes(w: &mut XmlWriter, l: &DpLanes) {
    let mut a = Attrs::new();
    put_head(&mut a, &l.head);
    let has = l.items.iter().any(writable);
    start(w, "Lanes", &a, has);
    for item in &l.items {
        write_timeline(w, item);
    }
    end(w, has);
}

/// `p` as the text of a `project.xml`. Deterministic: the same model always
/// gives the same bytes. See the module documentation for what is not
/// written.
pub fn write_project(p: &DpProject) -> String {
    let mut w = XmlWriter::new();
    let version: Attrs<'_> = vec![("version", Attr::Str(&p.version))];
    w.open("Project", &version);
    if let Some((name, version)) = &p.application {
        let a: Attrs<'_> = vec![("name", Attr::Str(name)), ("version", Attr::Str(version))];
        w.empty("Application", &a);
    }
    if p.tempo.is_some() || p.time_signature.is_some() {
        w.open("Transport", &[]);
        if let Some(t) = &p.tempo {
            write_real(&mut w, "Tempo", t);
        }
        if let Some(s) = &p.time_signature {
            let mut a = Attrs::new();
            put_named(&mut a, &s.named, true);
            a.push(("numerator", Attr::I64(i64::from(s.numerator))));
            a.push(("denominator", Attr::I64(i64::from(s.denominator))));
            w.empty("TimeSignature", &a);
        }
        w.close();
    }
    if !p.structure.is_empty() {
        w.open("Structure", &[]);
        for item in &p.structure {
            match item {
                DpStructureItem::Track(t) => write_track(&mut w, t),
                DpStructureItem::Channel(c) => write_channel(&mut w, c),
            }
        }
        w.close();
    }
    if let Some(arr) = &p.arrangement {
        let mut a = Attrs::new();
        put_named(&mut a, &arr.named, true);
        let has = arr.lanes.is_some()
            || !arr.markers.is_empty()
            || arr.tempo_automation.is_some()
            || arr.time_signature_automation.is_some();
        start(&mut w, "Arrangement", &a, has);
        if let Some(l) = &arr.lanes {
            write_lanes(&mut w, l);
        }
        write_markers(&mut w, "Markers", &DpHead::default(), &arr.markers);
        if let Some(t) = &arr.tempo_automation {
            write_points(&mut w, "TempoAutomation", t);
        }
        if let Some(t) = &arr.time_signature_automation {
            write_points(&mut w, "TimeSignatureAutomation", t);
        }
        end(&mut w, has);
    }
    if !p.scenes.is_empty() {
        w.open("Scenes", &[]);
        for s in &p.scenes {
            let mut a = Attrs::new();
            put_named(&mut a, &s.named, false);
            let has = s.content.as_ref().is_some_and(writable);
            start(&mut w, "Scene", &a, has);
            if let Some(c) = &s.content {
                write_timeline(&mut w, c);
            }
            end(&mut w, has);
        }
        w.close();
    }
    w.close();
    w.finish()
}

/// `m` as the text of a `metadata.xml`: the fields that are set, in the
/// XSD's order.
pub fn write_metadata(m: &DpMetaData) -> String {
    let mut w = XmlWriter::new();
    if m.fields.iter().all(Option::is_none) {
        w.empty("MetaData", &[]);
    } else {
        w.open("MetaData", &[]);
        for (name, text) in METADATA_FIELDS.iter().zip(&m.fields) {
            if let Some(text) = text {
                w.text_element(name, text);
            }
        }
        w.close();
    }
    w.finish()
}

// -------------------------------------------------------------------- IDs

/// What an ID names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdKind {
    /// A track.
    Track,
    /// A channel.
    Channel,
    /// A parameter.
    Parameter,
    /// A timeline.
    Timeline,
    /// A device.
    Device,
    /// A send.
    Send,
    /// The arrangement or a scene.
    Other,
}

/// The IDs of a project and what they name.
#[derive(Debug, Clone, Default)]
pub struct Ids {
    map: HashMap<String, IdKind>,
}

impl Ids {
    /// What `id` names, if it names anything.
    pub fn kind(&self, id: &str) -> Option<IdKind> {
        self.map.get(id).copied()
    }
}

/// A reference found in the model, to check against the IDs.
struct Reference<'a> {
    id: &'a str,
    want: IdKind,
    what: &'static str,
}

/// One walk over the model: its IDs, references and clip-reference edges.
#[derive(Default)]
struct Scan<'a> {
    ids: HashMap<&'a str, IdKind>,
    duplicate: Option<&'a str>,
    refs: Vec<Reference<'a>>,
    /// The IDs of the timelines around the part being walked.
    around: Vec<&'a str>,
    /// Timeline ID to the IDs its clips refer to.
    edges: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Scan<'a> {
    fn id(&mut self, n: &'a DpNamed, kind: IdKind) {
        if let Some(id) = &n.id
            && self.ids.insert(id, kind).is_some()
            && self.duplicate.is_none()
        {
            self.duplicate = Some(id);
        }
    }

    fn reference(&mut self, id: &'a Option<String>, want: IdKind, what: &'static str) {
        if let Some(id) = id {
            self.refs.push(Reference { id, want, what });
        }
    }

    fn real(&mut self, p: &'a DpRealParam) {
        self.id(&p.named, IdKind::Parameter);
    }

    fn bool_param(&mut self, p: &'a DpBoolParam) {
        self.id(&p.named, IdKind::Parameter);
    }

    fn track(&mut self, t: &'a DpTrack) {
        self.id(&t.named, IdKind::Track);
        if let Some(c) = &t.channel {
            self.channel(c);
        }
        for c in &t.children {
            self.track(c);
        }
    }

    fn channel(&mut self, c: &'a DpChannel) {
        self.id(&c.named, IdKind::Channel);
        self.reference(&c.destination, IdKind::Channel, "Channel destination");
        for d in &c.devices {
            self.id(&d.named, IdKind::Device);
            d.parameters.iter().for_each(|p| self.real(p));
            if let Some(e) = &d.enabled {
                self.bool_param(e);
            }
        }
        if let Some(m) = &c.mute {
            self.bool_param(m);
        }
        if let Some(p) = &c.pan {
            self.real(p);
        }
        for s in &c.sends {
            self.id(&s.named, IdKind::Send);
            self.reference(&s.destination, IdKind::Channel, "Send destination");
            if let Some(e) = &s.enable {
                self.bool_param(e);
            }
            if let Some(p) = &s.pan {
                self.real(p);
            }
            self.real(&s.volume);
        }
        if let Some(v) = &c.volume {
            self.real(v);
        }
    }

    /// Records a timeline's head; true if its ID was pushed onto `around`.
    fn head(&mut self, h: &'a DpHead) -> bool {
        self.id(&h.named, IdKind::Timeline);
        self.reference(&h.track, IdKind::Track, "Timeline track");
        match &h.named.id {
            Some(id) => {
                self.around.push(id);
                true
            }
            None => false,
        }
    }

    fn timeline(&mut self, t: &'a DpTimeline) {
        match t {
            DpTimeline::Lanes(x) => self.lanes(x),
            DpTimeline::Points(x) => self.points(x),
            DpTimeline::Clips(x) => {
                let pushed = self.head(&x.head);
                x.clips.iter().for_each(|c| self.clip(c));
                self.leave(pushed);
            }
            DpTimeline::Notes(x) => {
                let pushed = self.head(&x.head);
                for n in &x.notes {
                    if let Some(c) = &n.content {
                        self.timeline(c);
                    }
                }
                self.leave(pushed);
            }
            DpTimeline::Warps(x) => {
                let pushed = self.head(&x.head);
                self.timeline(&x.content);
                self.leave(pushed);
            }
            DpTimeline::ClipSlot(h, c) => {
                let pushed = self.head(h);
                if let Some(c) = c {
                    self.clip(c);
                }
                self.leave(pushed);
            }
            DpTimeline::Audio(DpAudio { head, .. })
            | DpTimeline::Markers(head, _)
            | DpTimeline::Video(head) => {
                let pushed = self.head(head);
                self.leave(pushed);
            }
            DpTimeline::Unknown(_) => {}
        }
    }

    fn leave(&mut self, pushed: bool) {
        if pushed {
            self.around.pop();
        }
    }

    fn lanes(&mut self, x: &'a DpLanes) {
        let pushed = self.head(&x.head);
        x.items.iter().for_each(|i| self.timeline(i));
        self.leave(pushed);
    }

    fn points(&mut self, x: &'a DpPoints) {
        let pushed = self.head(&x.head);
        self.reference(&x.target.parameter, IdKind::Parameter, "Automation target");
        self.leave(pushed);
    }

    fn clip(&mut self, c: &'a DpClip) {
        if let Some(r) = &c.reference {
            self.reference(&c.reference, IdKind::Timeline, "Clip reference");
            for a in &self.around {
                self.edges.entry(a).or_default().push(r);
            }
        }
        if let Some(t) = &c.content {
            self.timeline(t);
        }
    }

    /// Whether the clip-reference edges hold a loop.
    fn has_loop(&self) -> bool {
        // false: on the path being walked; true: finished.
        let mut state: HashMap<&str, bool> = HashMap::new();
        for &start in self.edges.keys() {
            if state.contains_key(start) {
                continue;
            }
            state.insert(start, false);
            let mut path: Vec<(&str, usize)> = vec![(start, 0)];
            while let Some(top) = path.last_mut() {
                let node = top.0;
                let next = self.edges.get(node).and_then(|v| v.get(top.1)).copied();
                let Some(next) = next else {
                    state.insert(node, true);
                    path.pop();
                    continue;
                };
                top.1 = top.1.saturating_add(1);
                match state.get(next) {
                    Some(false) => return true,
                    Some(true) => {}
                    None => {
                        state.insert(next, false);
                        path.push((next, 0));
                    }
                }
            }
        }
        false
    }
}

/// Checks `p`'s IDs: every one must name one part (else the file is refused),
/// clips must not refer to each other in a loop (else refused), and a
/// reference to nothing, or to the wrong kind of part, is counted in `report`
/// as left out ([`MISSING_REFERENCES`]) while it stays in the model. Clips,
/// markers, notes and points have no IDs, so their `named.id` is ignored.
pub fn check_ids(p: &DpProject, report: &mut Report) -> Result<Ids, IoError> {
    let mut s = Scan::default();
    if let Some(t) = &p.tempo {
        s.real(t);
    }
    if let Some(t) = &p.time_signature {
        s.id(&t.named, IdKind::Parameter);
    }
    for item in &p.structure {
        match item {
            DpStructureItem::Track(t) => s.track(t),
            DpStructureItem::Channel(c) => s.channel(c),
        }
    }
    if let Some(a) = &p.arrangement {
        s.id(&a.named, IdKind::Other);
        if let Some(l) = &a.lanes {
            s.lanes(l);
        }
        for points in [&a.tempo_automation, &a.time_signature_automation]
            .into_iter()
            .flatten()
        {
            s.points(points);
        }
    }
    for scene in &p.scenes {
        s.id(&scene.named, IdKind::Other);
        if let Some(t) = &scene.content {
            s.timeline(t);
        }
    }
    if let Some(id) = s.duplicate {
        return Err(damaged(format!("two parts share the ID '{id}'")));
    }
    if s.has_loop() {
        return Err(damaged("clips refer to each other in a loop"));
    }
    for r in &s.refs {
        if s.ids.get(r.id) != Some(&r.want) {
            report.add(
                ItemKind::LeftOut,
                MISSING_REFERENCES,
                Some(format!("{} '{}'", r.what, r.id)),
            );
        }
    }
    let map = s.ids.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    Ok(Ids { map })
}
