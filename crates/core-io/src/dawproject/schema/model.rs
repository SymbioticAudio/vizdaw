//! The types of the DAWproject model.

macro_rules! string_enum {
    ($(#[$meta:meta])* pub enum $name:ident { $($var:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $(#[doc = concat!("`", $text, "`.")] $var,)+
        }

        impl $name {
            pub(super) fn parse(s: &str) -> Option<Self> {
                match s.trim_matches([' ', '\t', '\n', '\r']) {
                    $($text => Some(Self::$var),)+
                    _ => None,
                }
            }

            pub(super) fn text(self) -> &'static str {
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
    /// child elements ([`Tree::dropped_text`](crate::xml::Tree::dropped_text)); for the mappers to report.
    pub dropped_text: usize,
    /// The bytes the reader counted for this model (not the tree's), so that
    /// [`check_ids`](super::check_ids) continues the same budget. Zero for a
    /// project built in code.
    pub model_bytes: usize,
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
    pub channel: Option<Box<DpChannel>>,
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
    pub mute: Option<Box<DpBoolParam>>,
    /// Its pan.
    pub pan: Option<Box<DpRealParam>>,
    /// Its sends.
    pub sends: Vec<DpSend>,
    /// Its volume.
    pub volume: Option<Box<DpRealParam>>,
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
    pub(super) const ALL: [Self; 10] = [
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

    pub(super) fn element(self) -> &'static str {
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

    pub(super) fn of(element: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.element() == element)
    }

    /// Whether the XSD gives it `pluginVersion`.
    pub(super) fn is_plugin(self) -> bool {
        matches!(self, Self::Au | Self::Clap | Self::Vst2 | Self::Vst3)
    }

    /// The names of the children the XSD gives it after the common ones.
    pub(super) fn settings_children(self) -> &'static [&'static str] {
        match self {
            Self::Equalizer => &["Band", "InputGain", "OutputGain"],
            Self::Compressor => &[
                "Attack",
                "AutoMakeup",
                "InputGain",
                "OutputGain",
                "Ratio",
                "Release",
                "Threshold",
            ],
            Self::NoiseGate => &["Attack", "Range", "Ratio", "Release", "Threshold"],
            Self::Limiter => &["Attack", "InputGain", "OutputGain", "Release", "Threshold"],
            _ => &[],
        }
    }
}

string_enum! {
    /// The type of an `Equalizer` band (the XSD's `eqBandType`).
    pub enum DpEqBandType {
        HighPass = "highPass",
        LowPass = "lowPass",
        BandPass = "bandPass",
        HighShelf = "highShelf",
        LowShelf = "lowShelf",
        Bell = "bell",
        Notch = "notch",
    }
}

impl DpEqBandType {
    /// The XSD's spelling, e.g. `highPass`.
    pub fn name(self) -> &'static str {
        self.text()
    }
}

/// One `Band` of an `Equalizer`. Every part is optional here: what the
/// mapping cannot use is listed there, not refused by the reader.
#[derive(Debug, Clone, PartialEq)]
pub struct DpEqBand {
    /// Its `type`; `None` when missing or unknown.
    pub kind: Option<DpEqBandType>,
    /// Its `order`.
    pub order: Option<i32>,
    /// Its `Freq`.
    pub freq: Option<DpRealParam>,
    /// Its `Gain`.
    pub gain: Option<DpRealParam>,
    /// Its `Q`.
    pub q: Option<DpRealParam>,
    /// Its `Enabled` value.
    pub enabled: Option<bool>,
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
    /// An `Equalizer`'s bands, read structurally beside [`DpDevice::settings`]
    /// (at most as many as settings are kept; empty for other kinds). Not
    /// written.
    pub bands: Vec<DpEqBand>,
    /// The IDs of parameters read but not kept (other than real ones, and a
    /// built-in device's settings), so that references to them still resolve.
    pub other_ids: Vec<String>,
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
    /// The head of its `Markers` timeline (id, time unit, track).
    pub markers_head: DpHead,
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
    /// Which texts the tree cut at its limit ([`Tree::text_cut`](crate::xml::Tree::text_cut)); for the
    /// mappers to report.
    pub cut: [bool; 13],
    /// How many runs of text the tree dropped for being in an element with
    /// child elements; for the mappers to report.
    pub dropped_text: usize,
}

/// A running total of the bytes a read holds, against
/// [`MAX_MODEL_BYTES`](crate::limits::MAX_MODEL_BYTES). Past the cap `over`
/// is set and the work stops adding.
#[derive(Debug, Default)]
pub(super) struct Cap {
    used: usize,
    pub(super) over: bool,
}

impl Cap {
    /// A total that already holds `used` bytes.
    pub(super) fn starting_at(used: usize) -> Self {
        let mut cap = Cap::default();
        cap.charge(used);
        cap
    }

    /// The bytes counted so far.
    pub(super) fn used(&self) -> usize {
        self.used
    }

    /// Counts `bytes` more.
    pub(super) fn charge(&mut self, bytes: usize) {
        self.used = self.used.saturating_add(bytes);
        if self.used > crate::limits::MAX_MODEL_BYTES {
            self.over = true;
        }
    }

    /// Counts a `T` held in a `Vec` (which may be up to twice as large as
    /// its length) and `extra` bytes.
    pub(super) fn item<T>(&mut self, extra: usize) {
        self.charge(size_of::<T>().saturating_mul(2).saturating_add(extra));
    }

    /// Counts a heap text of `len` bytes: rounded up, with the allocator's
    /// own header. Nothing for an empty one.
    pub(super) fn text(&mut self, len: usize) {
        if len > 0 {
            self.charge(len.next_multiple_of(16).saturating_add(16));
        }
    }

    /// Counts the growth of a collection to `bytes` (what it holds now, on
    /// top of `seen`, what was counted before): the old allocation is kept
    /// counted, as it lives while the new one is made.
    pub(super) fn grew(&mut self, seen: &mut usize, bytes: usize) {
        if bytes > *seen {
            self.charge(bytes);
            *seen = bytes;
        }
    }
}
