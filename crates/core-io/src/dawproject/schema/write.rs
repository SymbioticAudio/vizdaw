//! Writing the model as DAWproject XML, in the XSD's order.

use super::model::*;
use crate::report::{ItemKind, Report};
use crate::xml::{Attr, XmlWriter};

// ---------------------------------------------------------------- writing

/// The report item for a part the DAWproject file cannot hold.
pub const UNWRITABLE_PARTS: &str = "Parts the DAWproject file cannot hold";

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

fn write_clip(w: &mut XmlWriter, rep: &mut Report, c: &DpClip) {
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
        write_timeline(w, rep, t);
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

/// Whether [`write_timeline`] writes anything for `t`: not a video (its media
/// file is not modelled), an unknown element, an empty `markers` (the XSD
/// wants one marker) or warps with no warp or nothing to warp.
fn writable(t: &DpTimeline) -> bool {
    match t {
        DpTimeline::Video(_) | DpTimeline::Unknown(_) => false,
        DpTimeline::Markers(_, m) => !m.is_empty(),
        DpTimeline::Warps(x) => !x.warps.is_empty() && writable(&x.content),
        _ => true,
    }
}

/// Reports a timeline [`write_timeline`] leaves out.
fn omitted(rep: &mut Report, t: &DpTimeline) {
    let name = match t {
        DpTimeline::Video(_) => "Video".to_owned(),
        DpTimeline::Unknown(name) => name.clone(),
        DpTimeline::Markers(..) => "markers".to_owned(),
        DpTimeline::Warps(_) => "Warps".to_owned(),
        _ => return,
    };
    rep.add(ItemKind::LeftOut, UNWRITABLE_PARTS, Some(name));
}

fn write_timeline(w: &mut XmlWriter, rep: &mut Report, t: &DpTimeline) {
    if !writable(t) {
        omitted(rep, t);
        return;
    }
    match t {
        DpTimeline::Lanes(l) => write_lanes(w, rep, l),
        DpTimeline::Clips(c) => {
            let mut a = Attrs::new();
            put_head(&mut a, &c.head);
            start(w, "Clips", &a, !c.clips.is_empty());
            for clip in &c.clips {
                write_clip(w, rep, clip);
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
                    write_timeline(w, rep, c);
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
            write_timeline(w, rep, &x.content);
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
                write_clip(w, rep, c);
            }
            end(w, c.is_some());
        }
        DpTimeline::Video(_) | DpTimeline::Unknown(_) => {}
    }
}

fn write_lanes(w: &mut XmlWriter, rep: &mut Report, l: &DpLanes) {
    let mut a = Attrs::new();
    put_head(&mut a, &l.head);
    let has = l.items.iter().any(writable);
    start(w, "Lanes", &a, has);
    for item in &l.items {
        write_timeline(w, rep, item);
    }
    end(w, has);
}

/// `p` as the text of a `project.xml`. Deterministic: the same model always
/// gives the same bytes. See the module documentation for what is not
/// written.
pub fn write_project(p: &DpProject) -> String {
    write_project_reporting(p, &mut Report::default())
}

/// [`write_project`], counting in `rep` what it cannot write validly and so
/// leaves out ([`UNWRITABLE_PARTS`]): videos, unknown timelines, empty
/// `markers`, warps with no warp or no writable content, and scenes with no
/// writable content. A project with no application is written with VizDAW's.
pub fn write_project_reporting(p: &DpProject, rep: &mut Report) -> String {
    let mut w = XmlWriter::new();
    let version: Attrs<'_> = vec![("version", Attr::Str(&p.version))];
    w.open("Project", &version);
    let (name, version) = p
        .application
        .as_ref()
        .map_or(("VizDAW", env!("CARGO_PKG_VERSION")), |(n, v)| (n, v));
    let a: Attrs<'_> = vec![("name", Attr::Str(name)), ("version", Attr::Str(version))];
    w.empty("Application", &a);
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
            write_lanes(&mut w, rep, l);
        }
        write_markers(&mut w, "Markers", &arr.markers_head, &arr.markers);
        if let Some(t) = &arr.tempo_automation {
            write_points(&mut w, "TempoAutomation", t);
        }
        if let Some(t) = &arr.time_signature_automation {
            write_points(&mut w, "TimeSignatureAutomation", t);
        }
        end(&mut w, has);
    }
    let mut scenes = Vec::new();
    for s in &p.scenes {
        if s.content.as_ref().is_some_and(writable) {
            scenes.push(s);
        } else {
            rep.add(
                ItemKind::LeftOut,
                UNWRITABLE_PARTS,
                Some("Scene".to_owned()),
            );
        }
    }
    if !scenes.is_empty() {
        w.open("Scenes", &[]);
        for s in scenes {
            let mut a = Attrs::new();
            put_named(&mut a, &s.named, false);
            w.open("Scene", &a);
            if let Some(c) = &s.content {
                write_timeline(&mut w, rep, c);
            }
            w.close();
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
