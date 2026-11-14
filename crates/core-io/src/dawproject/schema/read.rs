//! Reading a DAWproject tree into the model.
//!
//! The model is built under a byte budget
//! ([`MAX_MODEL_BYTES`](crate::limits::MAX_MODEL_BYTES)): every item
//! is charged its size (doubled, for a `Vec` may hold twice its length) and
//! its texts. Past the budget the reader stops adding and the file is refused
//! as having too many parts.

use super::model::*;
use crate::IoError;
use crate::limits::{Limits, MAX_DETAILS, MAX_TIMELINE_NESTING};
use crate::report::{ItemKind, Report};
use crate::text::sanitize;
use crate::xml::{self, NodeId, Tree};
use crate::zip::{NOT_DAWPROJECT, damaged};

/// The report item for an element or attribute the model lacks.
pub const UNKNOWN_PARTS: &str = "Parts of the file VizDAW doesn't know";
/// The report item for an element left out because it could not be read.
pub const UNREADABLE_PARTS: &str = "Parts of the file that could not be read";
/// The report item for an optional value that could not be read.
pub const UNREADABLE_VALUES: &str = "Values in the file that could not be read";

/// Most settings kept as text for one built-in device.
const MAX_SETTINGS: usize = 64;
/// Longest one setting's text, in bytes.
const MAX_SETTING_BYTES: usize = 512;
/// Deepest element of a setting that is summarised.
const SETTING_DEPTH: usize = 3;
/// Longest ID or reference, in bytes; a longer one refuses the file.
pub const MAX_ID_BYTES: usize = 256;

const NAMED: [&str; 4] = ["id", "name", "color", "comment"];
const NAMEABLE: [&str; 3] = ["name", "color", "comment"];
const HEAD: [&str; 6] = ["id", "name", "color", "comment", "timeUnit", "track"];

/// `s` with control characters and bidirectional overrides dropped, and
/// whether it was cut at the text limit.
fn clean(s: &str) -> (String, bool) {
    sanitize(s.as_bytes(), &Limits::default())
}

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
    cap: Cap,
    /// What the tree held when the count began.
    tree_bytes: usize,
    /// A refusal found on the way; the work stops at the next loop.
    stop: Option<IoError>,
}

impl<'a> Rd<'a> {
    fn new(t: &'a Tree, rep: &'a mut Report) -> Self {
        let tree_bytes = t.bytes();
        Rd {
            t,
            rep,
            // The tree is alive while the model is built from it.
            cap: Cap::starting_at(tree_bytes),
            tree_bytes,
            stop: None,
        }
    }

    /// Whether the work should stop: the file is refused.
    fn halted(&self) -> bool {
        self.cap.over || self.stop.is_some()
    }

    /// The refusal, if the work was cut short.
    fn refusal(&mut self) -> Option<IoError> {
        self.stop
            .take()
            .or_else(|| self.cap.over.then(|| damaged("it has too many parts")))
    }

    /// Counts one occurrence under `kind`; the detail is only made while the
    /// item still keeps details.
    fn add(&mut self, kind: ItemKind, what: &str, detail: impl FnOnce() -> String) {
        let full = self
            .rep
            .items
            .iter()
            .any(|i| i.kind == kind && i.what == what && i.details.len() >= MAX_DETAILS);
        let detail = if full { None } else { Some(detail()) };
        self.rep.add(kind, what, detail);
    }

    fn unknown(&mut self, detail: impl FnOnce() -> String) {
        self.add(ItemKind::Note, UNKNOWN_PARTS, detail);
    }

    fn unknown_element(&mut self, n: NodeId) {
        let t = self.t;
        self.unknown(|| clean(t.name(n)).0);
    }

    /// Notes every attribute of `n` that is in none of `known`.
    fn check_attrs(&mut self, n: NodeId, known: &[&[&str]]) {
        let t = self.t;
        for (k, _) in t.attrs(n) {
            if k.starts_with("xmlns") || known.iter().any(|l| l.contains(&k)) {
                continue;
            }
            self.unknown(|| format!("{}@{}", clean(t.name(n)).0, clean(k).0));
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

    /// An element with the attributes `known` and no children.
    fn leaf(&mut self, n: NodeId, known: &[&[&str]]) {
        self.check_attrs(n, known);
        self.check_children(n, &[]);
    }

    fn left_out(&mut self, n: NodeId) {
        let t = self.t;
        self.add(ItemKind::LeftOut, UNREADABLE_PARTS, || clean(t.name(n)).0);
    }

    fn bad_value(&mut self, n: NodeId, attr: &str) {
        let t = self.t;
        self.add(ItemKind::LeftOut, UNREADABLE_VALUES, || {
            format!("{}@{attr}", clean(t.name(n)).0)
        });
    }

    /// An attribute's text, cleaned; one cut at the text limit is reported.
    fn string(&mut self, n: NodeId, a: &str) -> Option<String> {
        let v = self.t.attr(n, a)?;
        let (s, cut) = clean(v);
        if cut {
            self.bad_value(n, a);
        }
        let s = {
            let mut s = s;
            s.shrink_to_fit();
            s
        };
        self.cap.text(s.len());
        Some(s)
    }

    /// An ID or a reference: one over [`MAX_ID_BYTES`] refuses the file.
    fn id_string(&mut self, n: NodeId, a: &str) -> Option<String> {
        if self.t.attr(n, a).is_some_and(|v| v.len() > MAX_ID_BYTES) {
            self.stop.get_or_insert_with(|| {
                damaged(format!("an ID is longer than {MAX_ID_BYTES} bytes"))
            });
            return None;
        }
        self.string(n, a)
    }

    /// An optional attribute read by `parse`; one that is there but
    /// unreadable is reported.
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

    fn named(&mut self, n: NodeId, id: bool) -> DpNamed {
        DpNamed {
            id: if id { self.id_string(n, "id") } else { None },
            name: self.string(n, "name"),
            color: self.string(n, "color"),
            comment: self.string(n, "comment"),
        }
    }

    fn head(&mut self, n: NodeId) -> DpHead {
        DpHead {
            named: self.named(n, true),
            time_unit: self.opt(n, "timeUnit", DpTimeUnit::parse),
            track: self.id_string(n, "track"),
        }
    }

    // ---- parameters

    /// A real parameter; `None` (nothing reported) if its unit is missing.
    fn real_quiet(&mut self, n: NodeId) -> Option<DpRealParam> {
        let unit = self.req(n, "unit", DpUnit::parse)?;
        self.cap.item::<DpRealParam>(0);
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

    fn real(&mut self, n: NodeId) -> Option<DpRealParam> {
        let p = self.real_quiet(n);
        if p.is_none() {
            self.left_out(n);
        }
        p
    }

    fn bool_param(&mut self, n: NodeId) -> DpBoolParam {
        self.cap.item::<DpBoolParam>(0);
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
        self.cap.item::<DpTrack>(0);
        self.check_attrs(n, &[&NAMED, &["contentType", "loaded"]]);
        let mut content = Vec::new();
        for word in t
            .attr(n, "contentType")
            .unwrap_or_default()
            .split_whitespace()
        {
            match DpContentType::parse(word) {
                Some(c) => content.push(c),
                None => self.unknown(|| format!("Track@contentType {}", clean(word).0)),
            }
        }
        self.cap.text(content.capacity());
        let mut track = DpTrack {
            named: self.named(n, true),
            content,
            loaded: self.boolean(n, "loaded"),
            channel: None,
            children: Vec::new(),
        };
        for c in t.children(n) {
            if self.halted() {
                break;
            }
            match t.name(c) {
                "Channel" if track.channel.is_none() => {
                    track.channel = Some(Box::new(self.channel(c)));
                }
                "Track" => track.children.push(self.track(c)),
                _ => self.unknown_element(c),
            }
        }
        track
    }

    fn channel(&mut self, n: NodeId) -> DpChannel {
        let t = self.t;
        self.cap.item::<DpChannel>(0);
        self.check_attrs(
            n,
            &[&NAMED, &["role", "audioChannels", "destination", "solo"]],
        );
        let mut ch = DpChannel {
            named: self.named(n, true),
            role: self.opt(n, "role", DpMixerRole::parse),
            audio_channels: self.int(n, "audioChannels"),
            destination: self.id_string(n, "destination"),
            solo: self.boolean(n, "solo"),
            devices: Vec::new(),
            mute: None,
            pan: None,
            sends: Vec::new(),
            volume: None,
        };
        let mut seen = [false; 5];
        for c in t.children(n) {
            if self.halted() {
                break;
            }
            let slot = match t.name(c) {
                "Devices" => 0,
                "Mute" => 1,
                "Pan" => 2,
                "Sends" => 3,
                "Volume" => 4,
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
                0 => self.devices(c, &mut ch.devices),
                1 => ch.mute = Some(Box::new(self.bool_param(c))),
                2 => ch.pan = self.real(c).map(Box::new),
                3 => self.sends(c, &mut ch.sends),
                _ => ch.volume = self.real(c).map(Box::new),
            }
        }
        ch
    }

    fn devices(&mut self, n: NodeId, out: &mut Vec<DpDevice>) {
        let t = self.t;
        self.check_attrs(n, &[]);
        for c in t.children(n) {
            if self.halted() {
                break;
            }
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
        self.cap.item::<DpDevice>(0);
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
            bands: Vec::new(),
            other_ids: Vec::new(),
        };
        let mut seen = [false; 3];
        for c in t.children(n) {
            if self.halted() {
                break;
            }
            let slot = match t.name(c) {
                "Parameters" => 0,
                "Enabled" => 1,
                "State" => 2,
                name if kind.settings_children().contains(&name) => {
                    if d.settings.len() < MAX_SETTINGS {
                        let setting = self.setting(c);
                        self.cap.item::<String>(0);
                        self.cap.text(setting.len());
                        d.settings.push(setting);
                        if kind == DpDeviceKind::Equalizer && name == "Band" {
                            let band = self.eq_band(c);
                            d.bands.push(band);
                        }
                    } else {
                        self.unknown_element(c);
                    }
                    self.collect_ids(c, &mut d.other_ids);
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
                0 => self.parameters(c, &mut d),
                1 => d.enabled = Some(self.bool_param(c)),
                _ => d.state = self.file_ref(c),
            }
        }
        Some(d)
    }

    /// The IDs in `n` and below, for parts the model does not keep.
    fn collect_ids(&mut self, n: NodeId, out: &mut Vec<String>) {
        let t = self.t;
        if self.halted() {
            return;
        }
        if t.attr(n, "id").is_some() {
            if let Some(id) = self.id_string(n, "id") {
                self.cap.item::<String>(0);
                self.cap.text(id.len());
                out.push(id);
            }
        }
        for c in t.children(n) {
            self.collect_ids(c, out);
        }
    }

    fn parameters(&mut self, n: NodeId, d: &mut DpDevice) {
        let t = self.t;
        self.check_attrs(n, &[]);
        for c in t.children(n) {
            if self.halted() {
                break;
            }
            if t.name(c) == "RealParameter" {
                d.parameters.extend(self.real(c));
            } else {
                self.unknown_element(c);
                self.collect_ids(c, &mut d.other_ids);
            }
        }
    }

    /// An `Equalizer`'s `Band`, read quietly: its text is already in the
    /// device's settings, and the mapping lists what it cannot use.
    fn eq_band(&mut self, n: NodeId) -> DpEqBand {
        let t = self.t;
        self.cap.item::<DpEqBand>(0);
        // An element whose unit cannot be read is kept with no value, so the
        // mapping leaves the band out rather than use a default.
        let real = |c: NodeId| {
            let unit = t.attr(c, "unit").and_then(DpUnit::parse);
            Some(DpRealParam {
                named: DpNamed::default(),
                parameter_id: None,
                unit: unit.unwrap_or(DpUnit::Normalized),
                value: unit.and_then(|_| t.attr(c, "value").and_then(xml::double_or_inf)),
                min: t.attr(c, "min").and_then(xml::double_or_inf),
                max: t.attr(c, "max").and_then(xml::double_or_inf),
            })
        };
        let mut b = DpEqBand {
            kind: t.attr(n, "type").and_then(DpEqBandType::parse),
            order: t.attr(n, "order").and_then(xml::int),
            freq: None,
            gain: None,
            q: None,
            enabled: None,
        };
        let (mut freq, mut gain, mut q, mut enabled) = (false, false, false, false);
        for c in t.children(n) {
            match t.name(c) {
                "Freq" if !freq => {
                    freq = true;
                    b.freq = real(c);
                }
                "Gain" if !gain => {
                    gain = true;
                    b.gain = real(c);
                }
                "Q" if !q => {
                    q = true;
                    b.q = real(c);
                }
                "Enabled" if !enabled => {
                    enabled = true;
                    b.enabled = t.attr(c, "value").and_then(xml::boolean);
                }
                _ => {}
            }
        }
        b
    }

    /// A built-in device's element as one line of text.
    fn setting(&self, n: NodeId) -> String {
        fn go(t: &Tree, n: NodeId, depth: usize, out: &mut String) {
            out.push_str(t.name(n));
            for (k, v) in t.attrs(n) {
                if out.len() > MAX_SETTING_BYTES {
                    return;
                }
                out.push(' ');
                out.push_str(k);
                out.push('=');
                out.push_str(v);
            }
            if depth > 1 {
                let mut first = true;
                for c in t.children(n) {
                    if out.len() > MAX_SETTING_BYTES {
                        return;
                    }
                    out.push_str(if first { ": " } else { ", " });
                    first = false;
                    go(t, c, depth - 1, out);
                }
            }
        }
        let mut raw = String::new();
        go(self.t, n, SETTING_DEPTH, &mut raw);
        let mut cleaned = clean(&raw).0;
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
            if self.halted() {
                break;
            }
            if t.name(c) == "Send" {
                out.extend(self.send(c));
            } else {
                self.unknown_element(c);
            }
        }
    }

    fn send(&mut self, n: NodeId) -> Option<DpSend> {
        let t = self.t;
        self.cap.item::<DpSend>(0);
        let mut enable = None;
        let mut pan = None;
        let mut volume = None;
        let (mut enable_seen, mut pan_seen, mut volume_seen) = (false, false, false);
        for c in t.children(n) {
            if self.halted() {
                break;
            }
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
                    volume = self.real_quiet(c);
                }
                _ => self.unknown_element(c),
            }
        }
        let Some(volume) = volume else {
            self.left_out(n);
            return None;
        };
        self.check_attrs(n, &[&NAMED, &["destination", "type"]]);
        Some(DpSend {
            named: self.named(n, true),
            destination: self.id_string(n, "destination"),
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
            markers_head: DpHead::default(),
            markers: Vec::new(),
            tempo_automation: None,
            time_signature_automation: None,
        };
        let mut seen = [false; 4];
        for c in t.children(n) {
            if self.halted() {
                break;
            }
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
                1 => (a.markers_head, a.markers) = self.markers(c),
                2 => a.tempo_automation = self.points(c),
                _ => a.time_signature_automation = self.points(c),
            }
        }
        a
    }

    /// A timeline element at nesting `level` (1 is the outermost); `None`
    /// when it was left out.
    fn timeline(&mut self, n: NodeId, level: usize) -> Option<DpTimeline> {
        let t = self.t;
        let name = t.name(n);
        self.cap.item::<DpTimeline>(0);
        if !is_timeline(name) {
            self.unknown_element(n);
            let name = clean(name).0;
            self.cap.text(name.len());
            return Some(DpTimeline::Unknown(name));
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
                self.check_attrs(n, &[&HEAD]);
                let head = self.head(n);
                let mut clip = None;
                for c in t.children(n) {
                    if self.halted() {
                        break;
                    }
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
            if self.halted() {
                break;
            }
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
            if self.halted() {
                break;
            }
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
            if self.halted() {
                break;
            }
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
        self.cap.item::<DpClip>(0);
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
            reference: self.id_string(n, "reference"),
            content: self.content(n, level),
        })
    }

    fn notes(&mut self, n: NodeId, level: usize) -> DpNotes {
        let t = self.t;
        self.check_attrs(n, &[&HEAD]);
        let head = self.head(n);
        let mut notes = Vec::new();
        for c in t.children(n) {
            if self.halted() {
                break;
            }
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
        self.cap.item::<DpNote>(0);
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
        let mut target = None;
        let mut points = Vec::new();
        // The target is required; look for it before reading anything.
        if !t.children(n).any(|c| t.name(c) == "Target") {
            self.left_out(n);
            return None;
        }
        self.check_attrs(n, &[&HEAD, &["unit"]]);
        let head = self.head(n);
        let unit = self.opt(n, "unit", DpUnit::parse);
        for c in t.children(n) {
            if self.halted() {
                break;
            }
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
            parameter: self.id_string(n, "parameter"),
            expression: self.opt(n, "expression", DpExpression::parse),
            channel: self.int(n, "channel"),
            key: self.int(n, "key"),
            controller: self.int(n, "controller"),
        }
    }

    fn point(&mut self, n: NodeId) -> Option<DpPoint> {
        let time = self.req(n, "time", xml::double);
        let (value, known): (_, &[&str]) = match self.t.name(n) {
            "RealPoint" => (
                self.req(n, "value", xml::double).map(|v| {
                    let interpolation = self.opt(n, "interpolation", DpInterpolation::parse);
                    DpPointValue::Real(v, interpolation)
                }),
                &["time", "value", "interpolation"],
            ),
            "BoolPoint" => (
                self.req(n, "value", xml::boolean).map(DpPointValue::Bool),
                &["time", "value"],
            ),
            "IntegerPoint" => (
                self.req(n, "value", xml::int).map(DpPointValue::Integer),
                &["time", "value"],
            ),
            "EnumPoint" => (
                self.req(n, "value", xml::int).map(DpPointValue::Enum),
                &["time", "value"],
            ),
            _ => (
                self.req(n, "numerator", xml::int)
                    .zip(self.req(n, "denominator", xml::int))
                    .map(|(a, b)| DpPointValue::TimeSig(a, b)),
                &["time", "numerator", "denominator"],
            ),
        };
        let (Some(time), Some(value)) = (time, value) else {
            self.left_out(n);
            return None;
        };
        self.cap.item::<DpPoint>(0);
        self.leaf(n, &[known]);
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
            if self.halted() {
                break;
            }
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
            if self.halted() {
                break;
            }
            if t.name(c) == "Warp" {
                let pair =
                    self.req(c, "time", xml::double)
                        .zip(self.req(c, "contentTime", xml::double));
                match pair {
                    Some(p) => {
                        self.leaf(c, &[&["time", "contentTime"]]);
                        self.cap.item::<(f64, f64)>(0);
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
            if self.halted() {
                break;
            }
            if t.name(c) == "Marker" {
                if let Some(time) = self.req(c, "time", xml::double) {
                    self.cap.item::<DpMarker>(0);
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
        self.cap.item::<DpScene>(0);
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
            if self.halted() {
                break;
            }
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
                        if self.halted() {
                            break;
                        }
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
        if let Some(e) = self.refusal() {
            return Err(e);
        }
        p.dropped_text = t.dropped_text();
        p.model_bytes = self.cap.used().saturating_sub(self.tree_bytes);
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
            if self.halted() {
                break;
            }
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
            if self.halted() {
                break;
            }
            self.cap.item::<DpStructureItem>(0);
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
/// no `version`, a version whose major number is not 1, an ID longer than
/// [`MAX_ID_BYTES`] and a model that would pass
/// [`MAX_MODEL_BYTES`](crate::limits::MAX_MODEL_BYTES) ("it has too many
/// parts"). What it leaves out or does not know is counted in `report` (see
/// the module documentation). The IDs are not checked: that is
/// [`check_ids`](super::check_ids).
pub fn read_project(t: &Tree, report: &mut Report) -> Result<DpProject, IoError> {
    Rd::new(t, report).project()
}

/// Reads `t` as a DAWproject `metadata.xml`. A root that is not `MetaData`
/// gives an empty result and is counted in `report` as unknown, as are
/// elements it lacks.
pub fn read_metadata(t: &Tree, report: &mut Report) -> DpMetaData {
    let mut rd = Rd::new(t, report);
    let mut m = DpMetaData::default();
    let root = t.root();
    if t.name(root) != "MetaData" {
        rd.unknown_element(root);
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
            *f = Some(clean(t.text(c)).0);
        }
        if let Some(cut) = m.cut.get_mut(i) {
            *cut = t.text_cut(c);
        }
    }
    m
}
