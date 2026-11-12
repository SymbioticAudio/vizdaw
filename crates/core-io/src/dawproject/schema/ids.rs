//! Checking a project's IDs and the references to them.

use super::model::*;
use crate::IoError;
use crate::report::{ItemKind, Report};
use crate::zip::damaged;
use std::collections::HashMap;

/// The report item for a reference to a part that is not there.
pub const MISSING_REFERENCES: &str = "References to parts that are not in the file";

/// Most characters of an ID a message shows.
const SHOWN_ID_CHARS: usize = 64;

/// `id` as a message may show it.
fn shown(id: &str) -> String {
    match id.char_indices().nth(SHOWN_ID_CHARS) {
        Some((end, _)) => format!("{}...", id.get(..end).unwrap_or_default()),
        None => id.to_owned(),
    }
}

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

/// One walk over the model: its IDs, references and the edges that could
/// make clips play each other.
#[derive(Default)]
struct Scan<'a> {
    ids: HashMap<String, IdKind>,
    duplicate: Option<String>,
    refs: Vec<Reference<'a>>,
    /// The IDs of the timelines around the part being walked.
    around: Vec<&'a str>,
    /// Timeline ID to the IDs of the timelines inside it with an ID, and of
    /// those its clips (directly inside it) refer to.
    edges: HashMap<&'a str, Vec<&'a str>>,
    cap: Cap,
}

impl<'a> Scan<'a> {
    fn register(&mut self, id: &str, kind: IdKind) {
        self.cap.charge(id.len().saturating_add(64));
        if self.ids.insert(id.to_owned(), kind).is_some() && self.duplicate.is_none() {
            self.duplicate = Some(id.to_owned());
        }
    }

    fn id(&mut self, n: &DpNamed, kind: IdKind) {
        if let Some(id) = &n.id {
            self.register(id, kind);
        }
    }

    fn reference(&mut self, id: &'a Option<String>, want: IdKind, what: &'static str) {
        if let Some(id) = id {
            self.cap
                .charge(size_of::<Reference<'_>>().saturating_mul(2));
            self.refs.push(Reference { id, want, what });
        }
    }

    fn edge(&mut self, from: &'a str, to: &'a str) {
        self.cap.charge(size_of::<&str>().saturating_mul(4));
        self.edges.entry(from).or_default().push(to);
    }

    fn real(&mut self, p: &DpRealParam) {
        self.id(&p.named, IdKind::Parameter);
    }

    fn bool_param(&mut self, p: &DpBoolParam) {
        self.id(&p.named, IdKind::Parameter);
    }

    fn track(&mut self, t: &'a DpTrack) {
        if self.cap.over {
            return;
        }
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
            for id in &d.other_ids {
                self.register(id, IdKind::Parameter);
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
        let Some(id) = &h.named.id else {
            return false;
        };
        if let Some(parent) = self.around.last().copied() {
            self.edge(parent, id);
        }
        self.around.push(id);
        true
    }

    fn timeline(&mut self, t: &'a DpTimeline) {
        if self.cap.over {
            return;
        }
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
            if let Some(from) = self.around.last().copied() {
                self.edge(from, r);
            }
        }
        if let Some(t) = &c.content {
            self.timeline(t);
        }
    }

    /// Whether the edges hold a loop.
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
/// The check is itself bounded by
/// [`MAX_MODEL_BYTES`](crate::limits::MAX_MODEL_BYTES).
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
        if !a.markers.is_empty() || a.markers_head.named.id.is_some() {
            let pushed = s.head(&a.markers_head);
            s.leave(pushed);
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
    if s.cap.over {
        return Err(damaged("it has too many parts"));
    }
    if let Some(id) = &s.duplicate {
        return Err(damaged(format!("two parts share the ID '{}'", shown(id))));
    }
    if s.has_loop() {
        return Err(damaged("clips refer to each other in a loop"));
    }
    for r in &s.refs {
        if s.ids.get(r.id) != Some(&r.want) {
            report.add(
                ItemKind::LeftOut,
                MISSING_REFERENCES,
                Some(format!("{} '{}'", r.what, shown(r.id))),
            );
        }
    }
    Ok(Ids { map: s.ids })
}
