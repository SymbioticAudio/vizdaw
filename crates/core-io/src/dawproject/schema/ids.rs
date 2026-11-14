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

/// The IDs of a project and what they name. It borrows the IDs from the
/// project it was made from.
#[derive(Debug, Clone, Default)]
pub struct Ids<'a> {
    map: HashMap<&'a str, IdKind>,
}

impl Ids<'_> {
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

/// What a hash table of `capacity` entries of `entry` bytes holds, with its
/// control bytes and the headroom of its power-of-two size.
fn table_bytes(capacity: usize, entry: usize) -> usize {
    let buckets = capacity
        .saturating_mul(8)
        .div_ceil(7)
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX);
    buckets.saturating_mul(entry.saturating_add(1))
}

/// One walk over the model: its IDs, references and the edges that could
/// make clips play each other. Everything it grows is counted in `cap`
/// (which the model's own bytes started), and the walk stops once over.
struct Scan<'a> {
    ids: HashMap<&'a str, IdKind>,
    ids_seen: usize,
    duplicate: Option<&'a str>,
    refs: Vec<Reference<'a>>,
    refs_seen: usize,
    /// The IDs of the timelines around the part being walked.
    around: Vec<&'a str>,
    around_seen: usize,
    /// (from, to): `from` is a timeline ID, `to` the ID of a timeline inside
    /// it (with an ID of its own) or one its clips (directly inside it)
    /// refer to.
    edges: Vec<(&'a str, &'a str)>,
    edges_seen: usize,
    cap: Cap,
}

impl<'a> Scan<'a> {
    fn new(start: usize) -> Self {
        Scan {
            ids: HashMap::new(),
            ids_seen: 0,
            duplicate: None,
            refs: Vec::new(),
            refs_seen: 0,
            around: Vec::new(),
            around_seen: 0,
            edges: Vec::new(),
            edges_seen: 0,
            cap: Cap::starting_at(start),
        }
    }

    fn register(&mut self, id: &'a str, kind: IdKind) {
        if self.cap.over {
            return;
        }
        if self.ids.insert(id, kind).is_some() && self.duplicate.is_none() {
            self.duplicate = Some(id);
        }
        let bytes = table_bytes(self.ids.capacity(), size_of::<(&str, IdKind)>());
        self.cap.grew(&mut self.ids_seen, bytes);
    }

    fn id(&mut self, n: &'a DpNamed, kind: IdKind) {
        if let Some(id) = &n.id {
            self.register(id, kind);
        }
    }

    fn reference(&mut self, id: &'a Option<String>, want: IdKind, what: &'static str) {
        if self.cap.over {
            return;
        }
        if let Some(id) = id {
            self.refs.push(Reference { id, want, what });
            let bytes = self
                .refs
                .capacity()
                .saturating_mul(size_of::<Reference<'_>>());
            self.cap.grew(&mut self.refs_seen, bytes);
        }
    }

    fn edge(&mut self, from: &'a str, to: &'a str) {
        if self.cap.over {
            return;
        }
        self.edges.push((from, to));
        let bytes = self
            .edges
            .capacity()
            .saturating_mul(size_of::<(&str, &str)>());
        self.cap.grew(&mut self.edges_seen, bytes);
    }

    fn real(&mut self, p: &'a DpRealParam) {
        self.id(&p.named, IdKind::Parameter);
    }

    fn bool_param(&mut self, p: &'a DpBoolParam) {
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
                self.register(id.as_str(), IdKind::Parameter);
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
        let bytes = self.around.capacity().saturating_mul(size_of::<&str>());
        self.cap.grew(&mut self.around_seen, bytes);
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

    /// Whether the edges hold a loop; `Err` if the check passed the budget.
    fn has_loop(&mut self) -> Result<bool, IoError> {
        self.edges.sort_unstable_by(|a, b| a.0.cmp(b.0));
        let edges = std::mem::take(&mut self.edges);
        let range = |node: &str| {
            (
                edges.partition_point(|e| e.0 < node),
                edges.partition_point(|e| e.0 <= node),
            )
        };
        // false: on the path being walked; true: finished.
        let mut state: HashMap<&str, bool> = HashMap::new();
        let (mut state_seen, mut path_seen) = (0, 0);
        let mut path: Vec<(&str, usize, usize)> = Vec::new();
        for &(start, _) in &edges {
            if state.contains_key(start) {
                continue;
            }
            state.insert(start, false);
            let (from, to) = range(start);
            path.push((start, from, to));
            while let Some(top) = path.last_mut() {
                if self.cap.over {
                    return Err(damaged("it has too many parts"));
                }
                let (node, at, end) = *top;
                if at >= end {
                    state.insert(node, true);
                    path.pop();
                    continue;
                }
                top.1 = at.saturating_add(1);
                let Some(&(_, next)) = edges.get(at) else {
                    path.pop();
                    continue;
                };
                match state.get(next) {
                    Some(false) => return Ok(true),
                    Some(true) => {}
                    None => {
                        state.insert(next, false);
                        let (from, to) = range(next);
                        path.push((next, from, to));
                    }
                }
                let bytes = table_bytes(state.capacity(), size_of::<(&str, bool)>());
                self.cap.grew(&mut state_seen, bytes);
                let bytes = path
                    .capacity()
                    .saturating_mul(size_of::<(&str, usize, usize)>());
                self.cap.grew(&mut path_seen, bytes);
            }
        }
        Ok(false)
    }
}

/// Checks `p`'s IDs: every one must name one part (else the file is refused),
/// clips must not refer to each other in a loop (else refused), and a
/// reference to nothing, or to the wrong kind of part, is counted in `report`
/// as left out ([`MISSING_REFERENCES`]) while it stays in the model. Clips,
/// markers, notes and points have no IDs, so their `named.id` is ignored.
/// The check continues the budget the reader counted
/// ([`DpProject::model_bytes`], against
/// [`MAX_MODEL_BYTES`](crate::limits::MAX_MODEL_BYTES)) and so assumes the
/// tree the project was read from has been dropped; past it the file is
/// refused as having too many parts.
pub fn check_ids<'a>(p: &'a DpProject, report: &mut Report) -> Result<Ids<'a>, IoError> {
    let mut s = Scan::new(p.model_bytes);
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
    if let Some(id) = s.duplicate {
        return Err(damaged(format!("two parts share the ID '{}'", shown(id))));
    }
    if s.has_loop()? {
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
