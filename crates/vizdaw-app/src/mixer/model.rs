//! One document's mixer (Swift `MixerModel`): the strips, meters,
//! gestures, typed values, modes, selection and the strip edits. Fader,
//! pan, send-level, mute and solo moves are gestures on the core (touch,
//! value, release), which makes each one undo step and names it; the app
//! never names them.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_model::automation::{fader_db, fader_norm, pan_norm, pan_of};
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::{AutomationModeRow, GestureHolderRow, GesturePhaseRow};
use vizdaw_ffi::edit::{EditCommand, LaneEdit, MonitorModeRow};
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::mixer::{
    BusRow, DestRow, MixerSnapshot, StripKindRow, StripRow, SwatchRow, VcaRow,
};
use vizdaw_ffi::snapshot::{DocSnapshot, LaneRow};

use super::gesture::{ControlGesture, MIN_DRAG_DISTANCE};
use super::math::{AutomationModes, MixerMath, StripEntry};
use super::meters::{Meters, POLL_INTERVAL};
use crate::alerts::AlertItem;
use crate::document::DocumentModel;
use crate::session::DocSession;

type Target = AutomationTargetRow;

/// How long a release that meets Busy is retried before it is given up
/// with an alert. Time-based, as the poll's rate changes.
pub const RELEASE_WINDOW: Duration = Duration::from_secs(2);
/// The most targets one `heard_values` call takes.
pub const MAX_HEARD_TARGETS: usize = 4096;
/// The alert when the mixer cannot be read.
pub const MIXER_UNAVAILABLE: &str = "Mixer Unavailable";

/// Where a mixer shows (Swift `MixerPlacement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MixerPlacement {
    /// The Mixer window.
    Window,
    /// The bottom pane.
    Pane,
    /// The Mix view's centre.
    View,
    /// The left pane's Channel Strip.
    Strip,
}

/// One column of the mixer: an audio strip, an external destination's
/// MIDI strip, or a VCA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MixerColumn<'a> {
    Audio(&'a StripRow),
    Midi(&'a DestRow),
    Vca(&'a VcaRow),
}

impl MixerColumn<'_> {
    /// "s" + strip ID, "d" + destination ID or "v" + VCA ID, so they never
    /// collide.
    pub fn id(&self) -> String {
        match self {
            MixerColumn::Audio(s) => format!("s{}", s.id),
            MixerColumn::Midi(d) => format!("d{}", d.id),
            MixerColumn::Vca(v) => format!("v{}", v.id),
        }
    }
}

/// The control the mouse went down on in one placement.
#[derive(Debug, Clone, Copy)]
struct Press {
    target: Target,
    /// ⌥ was held at the press on a strip fader: its group stays.
    alone: bool,
}

/// A release that met Busy, retried until `deadline`.
#[derive(Debug, Clone, Copy)]
struct PendingRelease {
    deadline: Instant,
    alone: bool,
}

pub struct MixerModel {
    snapshot: MixerSnapshot,
    destinations: Vec<DestRow>,
    /// The document version the mixer was last read at.
    seen: u64,
    /// A refetch hit Busy and is still owed.
    refetch_owed: bool,
    fetches: u64,
    meters: Meters,
    last_poll: Option<Instant>,
    placements: BTreeSet<MixerPlacement>,
    /// The strip whose name is being edited, and where.
    renaming: Option<(u64, MixerPlacement)>,
    pub rename_draft: String,
    /// Shown values (normalised) of targets touched here, once moved.
    live: HashMap<Target, f64>,
    /// The values heard while playing, in the mixer's units (dB, −1…1).
    heard: HashMap<Target, f64>,
    /// Targets touched here whose touch the core took.
    touches: HashSet<Target>,
    /// Targets whose editing span is open (press to release), the core's
    /// touch taken or not: a value inside one is part of the drag, a value
    /// outside one is a click.
    editing: HashSet<Target>,
    /// Targets the core lists as touched.
    core_touched: HashSet<Target>,
    /// The core's values of the targets it holds and this view does not
    /// (a group drag's followers).
    core_values: HashMap<Target, f64>,
    /// Targets touched here alone (⌥-drag of a fader).
    alone: HashSet<Target>,
    /// Releases that met Busy.
    pending: HashMap<Target, PendingRelease>,
    presses: HashMap<MixerPlacement, Press>,
    gestures: HashMap<(MixerPlacement, Target), ControlGesture>,
    /// Moves when a tick changed what the placements show.
    shown_version: u64,
}

/// What a tick can change of what the placements show.
#[derive(PartialEq)]
struct Shown {
    meters: Meters,
    heard: HashMap<Target, f64>,
    core_touched: HashSet<Target>,
    core_values: HashMap<Target, f64>,
    fetches: u64,
    touches: HashSet<Target>,
    pending: usize,
}

impl MixerModel {
    /// Reads the mixer of `doc`'s session.
    pub fn new(doc: &DocumentModel) -> Result<Self, VizError> {
        let session = doc.session();
        let snapshot = session.mixer()?;
        let destinations = session.destinations().unwrap_or_default();
        Ok(MixerModel {
            snapshot,
            destinations,
            seen: doc.snapshot().version,
            refetch_owed: false,
            fetches: 1,
            meters: Meters::default(),
            last_poll: None,
            placements: BTreeSet::new(),
            renaming: None,
            rename_draft: String::new(),
            live: HashMap::new(),
            heard: HashMap::new(),
            touches: HashSet::new(),
            editing: HashSet::new(),
            core_touched: HashSet::new(),
            core_values: HashMap::new(),
            alone: HashSet::new(),
            pending: HashMap::new(),
            presses: HashMap::new(),
            gestures: HashMap::new(),
            shown_version: 0,
        })
    }

    // Derived.

    pub fn snapshot(&self) -> &MixerSnapshot {
        &self.snapshot
    }

    /// Every strip but master, in display order.
    pub fn strips(&self) -> impl Iterator<Item = &StripRow> {
        self.snapshot
            .strips
            .iter()
            .filter(|s| s.kind != StripKindRow::Master)
    }

    pub fn master(&self) -> Option<&StripRow> {
        self.snapshot
            .strips
            .iter()
            .find(|s| s.kind == StripKindRow::Master)
    }

    pub fn strip(&self, id: u64) -> Option<&StripRow> {
        self.snapshot.strips.iter().find(|s| s.id == id)
    }

    pub fn buses(&self) -> &[BusRow] {
        &self.snapshot.buses
    }

    pub fn vcas(&self) -> &[VcaRow] {
        &self.snapshot.vcas
    }

    /// The instrument list (lanes play through these).
    pub fn destinations(&self) -> &[DestRow] {
        &self.destinations
    }

    /// The columns, left to right: the strips in the document's order,
    /// with every external destination's MIDI strip inserted before the
    /// first bus (at the end when there is none), then the VCAs. Master is
    /// not one: it is drawn after a divider.
    pub fn columns(&self) -> Vec<MixerColumn<'_>> {
        let mut out: Vec<MixerColumn<'_>> = self.strips().map(MixerColumn::Audio).collect();
        let at = self
            .strips()
            .position(|s| s.kind == StripKindRow::Bus)
            .unwrap_or(out.len());
        let midi = self
            .destinations
            .iter()
            .filter(|d| d.external)
            .map(MixerColumn::Midi);
        out.splice(at..at, midi);
        out.extend(self.snapshot.vcas.iter().map(MixerColumn::Vca));
        out
    }

    /// "Master" or the bus's name.
    pub fn output_name(&self, strip: &StripRow) -> String {
        match strip.output {
            None => "Master".into(),
            Some(b) => self
                .snapshot
                .buses
                .iter()
                .find(|x| x.id == b)
                .map_or_else(|| format!("Bus {b}"), |x| x.name.clone()),
        }
    }

    /// Buses `strip` may send or output to (not itself).
    pub fn targets(&self, strip: &StripRow) -> Vec<&BusRow> {
        self.snapshot
            .buses
            .iter()
            .filter(|b| b.id != strip.id)
            .collect()
    }

    pub fn strip_name(&self, id: u64) -> String {
        self.strip(id)
            .map_or_else(|| format!("Strip {id}"), |s| s.name.clone())
    }

    /// Times the mixer was read (tests check the refetch rules).
    pub fn fetches(&self) -> u64 {
        self.fetches
    }

    pub fn meters(&self) -> &Meters {
        &self.meters
    }

    /// "DSP 12% · peak 30% · PDC 10.7 ms".
    pub fn load_text(&self, sample_rate: f64) -> String {
        self.meters.load_text(sample_rate)
    }

    /// Takes `l` as a read of the levels at `now`, on top of what was
    /// read (a measurement build's synthetic signal; not for the UI).
    #[doc(hidden)]
    pub fn take_levels(&mut self, l: &vizdaw_ffi::mixer::MixerLevels, now: Instant) {
        self.meters.take(l, now);
        self.shown_version += 1;
    }

    /// The placements showing this mixer now.
    pub fn placements(&self) -> &BTreeSet<MixerPlacement> {
        &self.placements
    }

    // Shown values.

    /// The fader dB as shown: live while touched here, else the core's
    /// value while it holds the fader (a group drag's follower), else heard
    /// while playing, else the snapshot's.
    pub fn gain_db(&self, s: &StripRow) -> f64 {
        self.shown_db(Target::StripFader { strip: s.id }, s.gain_db)
    }

    /// A VCA's fader dB as shown, by the same rule.
    pub fn vca_gain_db(&self, v: &VcaRow) -> f64 {
        self.shown_db(Target::VcaFader { vca: v.id }, v.gain_db)
    }

    fn shown_db(&self, t: Target, rest: f64) -> f64 {
        if let Some(v) = self.live.get(&t) {
            return fader_db(*v);
        }
        if let Some(v) = self.core_values.get(&t) {
            return fader_db(*v);
        }
        self.heard.get(&t).copied().unwrap_or(rest)
    }

    /// The pan as shown (live while touched, else heard while playing).
    pub fn pan(&self, s: &StripRow) -> f64 {
        let t = Target::StripPan { strip: s.id };
        if let Some(v) = self.live.get(&t) {
            return pan_of(*v);
        }
        self.heard.get(&t).copied().unwrap_or(s.pan)
    }

    /// Send `index`'s level as shown (live while touched, else heard while
    /// playing).
    pub fn send_level_db(&self, s: &StripRow, index: usize) -> f64 {
        let t = Target::SendLevel {
            strip: s.id,
            send: send_index(index),
        };
        if let Some(v) = self.live.get(&t) {
            return fader_db(*v);
        }
        if let Some(v) = self.heard.get(&t) {
            return *v;
        }
        s.sends
            .get(index)
            .map_or(MixerMath::FLOOR_DB, |x| x.level_db)
    }

    /// Touched here or in the core (a channel or plugin touch), or a
    /// release still waiting: the ring.
    pub fn is_touched(&self, t: Target) -> bool {
        self.touches.contains(&t) || self.core_touched.contains(&t) || self.pending.contains_key(&t)
    }

    /// A release met Busy and waits for a retry.
    pub fn release_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// A drag is under way in placement `p`.
    pub fn is_dragging(&self, p: MixerPlacement) -> bool {
        self.presses
            .get(&p)
            .and_then(|press| self.gestures.get(&(p, press.target)))
            .is_some_and(ControlGesture::is_dragging)
    }

    /// The control the mouse is down on in placement `p` (dragged or
    /// not), until its mouse-up or a cancel.
    pub fn pressed(&self, p: MixerPlacement) -> Option<Target> {
        self.presses.get(&p).map(|press| press.target)
    }

    /// The control position (0…1) of `t` as shown: the fader law for a
    /// fader, VCA or send, (pan + 1) / 2 for a pan.
    pub fn position(&self, t: Target) -> f64 {
        match t {
            Target::StripFader { strip } => self
                .strip(strip)
                .map_or(0.0, |s| MixerMath::fader_position(self.gain_db(s))),
            Target::VcaFader { vca } => self
                .snapshot
                .vcas
                .iter()
                .find(|v| v.id == vca)
                .map_or(0.0, |v| MixerMath::fader_position(self.vca_gain_db(v))),
            Target::SendLevel { strip, send } => self.strip(strip).map_or(0.0, |s| {
                MixerMath::fader_position(self.send_level_db(s, usize::from(send)))
            }),
            Target::StripPan { strip } => self.strip(strip).map_or(0.5, |s| pan_norm(self.pan(s))),
            _ => 0.0,
        }
    }

    // Placements.

    pub fn appeared(&mut self, p: MixerPlacement) {
        self.placements.insert(p);
    }

    /// Placement `p` went: its drag ends; the last placement gone ends
    /// every open touch.
    pub fn disappeared(&mut self, doc: &mut DocumentModel, p: MixerPlacement, now: Instant) {
        self.placements.remove(&p);
        self.cancel_drags(doc, p, now);
        if self.placements.is_empty() {
            self.end_open_drag(doc, now);
        }
    }

    pub fn is_renaming(&self, strip: u64, p: MixerPlacement) -> bool {
        self.renaming == Some((strip, p))
    }

    /// The strip whose name is being edited, and where.
    pub fn renaming(&self) -> Option<(u64, MixerPlacement)> {
        self.renaming
    }

    // Polling.

    /// One poll, however many placements call it per frame: at most every
    /// [`POLL_INTERVAL`] it refetches when needed, reads the levels and
    /// the automation state (and the heard values while playing). A
    /// release waiting on Busy is retried on every call. True when it
    /// polled.
    pub fn tick(&mut self, doc: &mut DocumentModel, now: Instant) -> bool {
        let before = self.shown();
        self.retry_releases(doc, now);
        if let Some(last) = self.last_poll
            && now >= last
            && now - last < POLL_INTERVAL
        {
            self.note_shown(before);
            return false;
        }
        self.last_poll = Some(now);
        self.refresh_if_needed(doc);
        self.poll_levels(doc, now);
        self.poll_automation(doc);
        self.note_shown(before);
        true
    }

    /// Moves whenever a tick changed what the placements show (meters,
    /// load, heard values, the core's touches, a refetch): every
    /// placement redraws then, whichever one ticked.
    pub fn shown_version(&self) -> u64 {
        self.shown_version
    }

    fn shown(&self) -> Shown {
        Shown {
            meters: self.meters.clone(),
            heard: self.heard.clone(),
            core_touched: self.core_touched.clone(),
            core_values: self.core_values.clone(),
            fetches: self.fetches,
            touches: self.touches.clone(),
            pending: self.pending.len(),
        }
    }

    fn note_shown(&mut self, before: Shown) {
        if before != self.shown() {
            self.shown_version += 1;
        }
    }

    /// The document moved since the mixer was read, or a refetch is owed
    /// (see [`MixerModel::refresh_if_needed`]).
    pub fn needs_refresh(&self, doc: &DocumentModel) -> bool {
        doc.snapshot().version != self.seen || self.refetch_owed
    }

    /// Refetches the mixer when the document changed or a refetch is owed.
    pub fn refresh_if_needed(&mut self, doc: &mut DocumentModel) {
        if self.needs_refresh(doc) {
            self.refetch(doc);
        }
    }

    /// Reads meters, load and faults. A refusal keeps the last.
    pub fn poll_levels(&mut self, doc: &DocumentModel, now: Instant) {
        if let Ok(l) = doc.session().levels() {
            self.meters.take(&l, now);
        }
    }

    /// Reads the core's touched targets and their values (`Busy` and
    /// errors keep the last); while playing, the heard values too.
    pub fn poll_automation(&mut self, doc: &DocumentModel) {
        self.poll_heard(doc);
        let Ok(a) = doc.session().automation_state() else {
            return;
        };
        self.core_touched = a.touched.iter().copied().collect();
        self.core_values = a
            .touched
            .iter()
            .zip(&a.touched_values)
            .filter(|(t, v)| !self.touches.contains(t) && v.is_finite())
            .map(|(t, v)| (*t, *v))
            .collect();
    }

    /// Every strip's fader, pan and send levels, then every VCA's fader.
    fn heard_targets(&self) -> Vec<Target> {
        let mut out = vec![];
        for s in &self.snapshot.strips {
            out.push(Target::StripFader { strip: s.id });
            out.push(Target::StripPan { strip: s.id });
            out.extend((0..s.sends.len()).map(|i| Target::SendLevel {
                strip: s.id,
                send: send_index(i),
            }));
        }
        out.extend(
            self.snapshot
                .vcas
                .iter()
                .map(|v| Target::VcaFader { vca: v.id }),
        );
        out.truncate(MAX_HEARD_TARGETS);
        out
    }

    fn poll_heard(&mut self, doc: &DocumentModel) {
        if !doc.is_playing() {
            self.heard.clear();
            return;
        }
        let targets = self.heard_targets();
        let Ok(values) = doc.session().heard_values(targets.clone()) else {
            return;
        };
        self.heard = targets
            .into_iter()
            .zip(values)
            .filter_map(|(t, v)| v.map(|v| (t, v)))
            .collect();
    }

    // Gestures.

    /// Mouse-down: `touch(value: None)` on the core (the value heard now).
    /// A touch the core refuses is not kept, so no value or release
    /// follows it. `alone` (⌥ at the press) moves this target without its
    /// group until its release.
    pub fn touch(&mut self, doc: &mut DocumentModel, t: Target, alone: bool) {
        self.editing.insert(t);
        if self.touches.contains(&t) {
            return;
        }
        // Its release has not reached the core yet: the core still holds
        // it, and this drag carries on with it.
        if let Some(p) = self.pending.remove(&t) {
            self.touches.insert(t);
            if p.alone {
                self.alone.insert(t);
            }
            return;
        }
        let session = doc.session().clone();
        match send(&session, t, GesturePhaseRow::Touch { value: None }, alone) {
            Ok(()) => {
                self.touches.insert(t);
                if alone {
                    self.alone.insert(t);
                }
            }
            Err(e) => raise(doc, AlertItem::from_error(&e, failure_title(t))),
        }
    }

    /// A move of a touched target to normalised `v`: heard at once and
    /// shown from now on. Inside a drag whose touch was refused it sends
    /// nothing; a refused value releases the touch at once. A value with
    /// no drag open (a key) is a click at `v`.
    pub fn move_to(&mut self, doc: &mut DocumentModel, t: Target, v: f64, now: Instant) {
        if !self.touches.contains(&t) {
            if !self.editing.contains(&t) {
                self.click(doc, t, v, now);
            }
            return;
        }
        self.live.insert(t, v);
        let alone = self.alone.contains(&t);
        let session = doc.session().clone();
        if let Err(e) = send(&session, t, GesturePhaseRow::Value { value: v }, alone) {
            raise(doc, AlertItem::from_error(&e, failure_title(t)));
            self.touches.remove(&t);
            self.live.remove(&t);
            self.core_touched.remove(&t);
            self.alone.remove(&t);
            self.send_release(doc, t, alone, now);
        }
    }

    /// Mouse-up: the core closes the gesture; the control shows the
    /// snapshot's value again. A release that meets Busy is retried.
    pub fn release(&mut self, doc: &mut DocumentModel, t: Target, now: Instant) {
        self.editing.remove(&t);
        if !self.touches.remove(&t) {
            return;
        }
        self.live.remove(&t);
        self.core_touched.remove(&t);
        let alone = self.alone.remove(&t);
        if self.send_release(doc, t, alone, now) {
            self.gesture_landed(doc);
        }
    }

    /// Touch at `v`, value and release at once (mute, solo, a reset, a
    /// typed value). Ignored while the target is touched here. A refused
    /// touch sends nothing else; a later failure still releases it.
    pub fn click(&mut self, doc: &mut DocumentModel, t: Target, v: f64, now: Instant) {
        if self.touches.contains(&t) {
            return;
        }
        // This click's own release closes a waiting one.
        self.pending.remove(&t);
        let session = doc.session().clone();
        if let Err(e) = session.gesture(t, GesturePhaseRow::Touch { value: Some(v) }) {
            raise(doc, AlertItem::from_error(&e, failure_title(t)));
            return;
        }
        match session.gesture(t, GesturePhaseRow::Value { value: v }) {
            Ok(()) => {
                self.send_release(doc, t, false, now);
            }
            Err(e) => {
                // The touch was taken: close it, and report the value's
                // failure.
                if let Err(VizError::Busy { .. }) = session.gesture(t, GesturePhaseRow::Release) {
                    self.pend(t, false, now);
                }
                raise(doc, AlertItem::from_error(&e, failure_title(t)));
            }
        }
        self.gesture_landed(doc);
    }

    /// Releases every touch still open here (the last placement or the
    /// document closing must not leave a gesture open), each once.
    pub fn end_open_drag(&mut self, doc: &mut DocumentModel, now: Instant) {
        self.editing.clear();
        self.presses.clear();
        self.gestures.clear();
        if self.touches.is_empty() {
            return;
        }
        let mut open: Vec<Target> = self.touches.drain().collect();
        open.sort_by_key(|t| format!("{t:?}"));
        let mut landed = false;
        for t in open {
            self.live.remove(&t);
            self.core_touched.remove(&t);
            let alone = self.alone.remove(&t);
            landed |= self.send_release(doc, t, alone, now);
        }
        self.alone.clear();
        if landed {
            self.gesture_landed(doc);
        }
    }

    /// Retries every release that met Busy; one past its window, or one
    /// refused otherwise, is an alert.
    pub fn retry_releases(&mut self, doc: &mut DocumentModel, now: Instant) {
        if self.pending.is_empty() {
            return;
        }
        let session = doc.session().clone();
        let mut landed = false;
        let mut waiting: Vec<(Target, PendingRelease)> = self.pending.drain().collect();
        waiting.sort_by_key(|(t, _)| format!("{t:?}"));
        for (t, p) in waiting {
            match send(&session, t, GesturePhaseRow::Release, p.alone) {
                Ok(()) => landed = true,
                Err(VizError::Busy { .. }) if now < p.deadline => {
                    self.pending.insert(t, p);
                }
                Err(e) => raise(doc, AlertItem::from_error(&e, failure_title(t))),
            }
        }
        if landed {
            self.gesture_landed(doc);
        }
    }

    /// Sends a release; true when it went. Busy waits for a retry; another
    /// refusal is an alert.
    fn send_release(
        &mut self,
        doc: &mut DocumentModel,
        t: Target,
        alone: bool,
        now: Instant,
    ) -> bool {
        let session = doc.session().clone();
        match send(&session, t, GesturePhaseRow::Release, alone) {
            Ok(()) => true,
            Err(VizError::Busy { .. }) => {
                self.pend(t, alone, now);
                false
            }
            Err(e) => {
                raise(doc, AlertItem::from_error(&e, failure_title(t)));
                true
            }
        }
    }

    fn pend(&mut self, t: Target, alone: bool, now: Instant) {
        self.pending.insert(
            t,
            PendingRelease {
                deadline: now + RELEASE_WINDOW,
                alone,
            },
        );
    }

    // The drag tracker (driven by the view's mouse events).

    /// Mouse-down on the control of `t` in placement `p`. Sends nothing: a
    /// press without movement is no touch. `travel` is the points for the
    /// whole range (the fader's throw; [`super::gesture::DRAG_TRAVEL`] for
    /// a knob). ⌥ held at the press moves a strip fader alone.
    pub fn press(
        &mut self,
        doc: &mut DocumentModel,
        p: MixerPlacement,
        t: Target,
        travel: f64,
        option: bool,
        now: Instant,
    ) {
        // A press whose mouse-up never came ends first.
        self.lift(doc, p, now);
        let alone = option && matches!(t, Target::StripFader { .. });
        self.presses.insert(p, Press { target: t, alone });
        self.gestures
            .entry((p, t))
            .or_insert_with(|| ControlGesture::new(travel))
            .travel = travel;
    }

    /// The mouse moved by (`dx`, `dy`) points from the press (down
    /// positive); `fine` is ⌥ now. The first move of at least
    /// [`MIN_DRAG_DISTANCE`] touches the target.
    pub fn drag(
        &mut self,
        doc: &mut DocumentModel,
        p: MixerPlacement,
        dx: f64,
        dy: f64,
        fine: bool,
        now: Instant,
    ) {
        let Some(press) = self.presses.get(&p).copied() else {
            return;
        };
        let t = press.target;
        let start = self.position(t);
        let Some(g) = self.gestures.get_mut(&(p, t)) else {
            return;
        };
        if !g.is_dragging() && dx.hypot(dy) < MIN_DRAG_DISTANCE {
            return;
        }
        let Some(change) = g.changed(dy, fine, || start) else {
            return;
        };
        if change.began {
            self.touch(doc, t, press.alone);
        }
        self.move_to(doc, t, norm_of(t, change.value), now);
    }

    /// Mouse-up in placement `p`: a drag under way is released.
    pub fn lift(&mut self, doc: &mut DocumentModel, p: MixerPlacement, now: Instant) {
        let Some(press) = self.presses.remove(&p) else {
            return;
        };
        let ended = self
            .gestures
            .get_mut(&(p, press.target))
            .is_some_and(|g| g.ended(now));
        if ended {
            self.release(doc, press.target, now);
        }
    }

    /// The drag in `p` was cancelled (the window deactivated, the control
    /// went): released once.
    pub fn cancel_drags(&mut self, doc: &mut DocumentModel, p: MixerPlacement, now: Instant) {
        self.lift(doc, p, now);
    }

    /// A double-click on the control of `t`: the reset (0 dB, centre),
    /// unless a drag is under way or ended less than
    /// [`super::gesture::SETTLE`] ago.
    pub fn double_click(
        &mut self,
        doc: &mut DocumentModel,
        p: MixerPlacement,
        t: Target,
        now: Instant,
    ) {
        let reset = self
            .gestures
            .get(&(p, t))
            .is_none_or(|g| g.double_tapped(now));
        if reset {
            self.click(doc, t, reset_value(t), now);
        }
    }

    /// A key step of `delta` (0.01, ×10 with ⇧) of position: touch, value
    /// and release.
    pub fn nudge(&mut self, doc: &mut DocumentModel, t: Target, delta: f64, now: Instant) {
        let v = (self.position(t) + delta).clamp(0.0, 1.0);
        self.touch(doc, t, false);
        self.move_to(doc, t, norm_of(t, v), now);
        self.release(doc, t, now);
    }

    // Intents.

    pub fn toggle_mute(&mut self, doc: &mut DocumentModel, strip: u64, now: Instant) {
        let Some(on) = self.strip(strip).map(|s| s.mute) else {
            return;
        };
        self.click(
            doc,
            Target::StripMute { strip },
            if on { 0.0 } else { 1.0 },
            now,
        );
    }

    pub fn toggle_solo(&mut self, doc: &mut DocumentModel, strip: u64, now: Instant) {
        let Some(on) = self.strip(strip).map(|s| s.solo) else {
            return;
        };
        self.click(
            doc,
            Target::StripSolo { strip },
            if on { 0.0 } else { 1.0 },
            now,
        );
    }

    // Record arm and monitor (audio-path strips only).

    /// The track an audio-path strip's R and I edit: of the shown
    /// sequence's tracks that play through the strip's path, the first
    /// armed one, else the first; `None` for other strips.
    pub fn input_lane<'d>(&self, doc: &'d DocumentModel, s: &StripRow) -> Option<&'d LaneRow> {
        let StripKindRow::AudioPath { path } = s.kind else {
            return None;
        };
        doc.input_lane(path)
    }

    /// R and I work on a strip with an input track (an audio path); the
    /// instrument strips and master leave them disabled.
    pub fn can_arm(&self, doc: &DocumentModel, s: &StripRow) -> bool {
        self.input_lane(doc, s).is_some()
    }

    /// Toggles the arm of the strip's input track (one undo step), as the
    /// track header does.
    pub fn toggle_arm(&mut self, doc: &mut DocumentModel, strip: u64) {
        let Some(s) = self.strip(strip) else { return };
        let Some((lane, arm)) = self.input_lane(doc, s).map(|l| (l.id, l.arm)) else {
            return;
        };
        let Some(seq) = doc.selected_seq() else {
            return;
        };
        doc.edit(EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq,
                lane,
                mute: None,
                solo: None,
                arm: Some(!arm),
            },
        });
    }

    /// The monitor button: Auto, Input, then Off (one undo step).
    pub fn cycle_monitor(&mut self, doc: &mut DocumentModel, strip: u64) {
        let Some(s) = self.strip(strip) else { return };
        let Some((lane, mode)) = self.input_lane(doc, s).map(|l| (l.id, l.monitor)) else {
            return;
        };
        let Some(seq) = doc.selected_seq() else {
            return;
        };
        doc.edit(EditCommand::Lane {
            edit: LaneEdit::SetMonitor {
                seq,
                lane,
                monitor: next_monitor(mode),
            },
        });
    }

    /// Applies a typed level as one click. False, with the status line
    /// set, when the text is refused or the target is touched.
    fn type_value(
        &mut self,
        doc: &mut DocumentModel,
        t: Target,
        norm: Option<f64>,
        hint: &str,
        now: Instant,
    ) -> bool {
        let Some(norm) = norm else {
            doc.set_status(Some(hint.into()));
            return false;
        };
        if self.is_touched(t) {
            doc.set_status(Some(StripEntry::TOUCHED_HINT.into()));
            return false;
        }
        self.click(doc, t, norm, now);
        true
    }

    pub fn type_level(
        &mut self,
        doc: &mut DocumentModel,
        strip: u64,
        text: &str,
        now: Instant,
    ) -> bool {
        let norm = StripEntry::level(text).map(fader_norm);
        self.type_value(
            doc,
            Target::StripFader { strip },
            norm,
            StripEntry::LEVEL_HINT,
            now,
        )
    }

    pub fn type_pan(
        &mut self,
        doc: &mut DocumentModel,
        strip: u64,
        text: &str,
        now: Instant,
    ) -> bool {
        let norm = StripEntry::pan(text).map(pan_norm);
        self.type_value(
            doc,
            Target::StripPan { strip },
            norm,
            StripEntry::PAN_HINT,
            now,
        )
    }

    pub fn type_send(
        &mut self,
        doc: &mut DocumentModel,
        strip: u64,
        index: usize,
        text: &str,
        now: Instant,
    ) -> bool {
        if self.strip(strip).is_none_or(|s| index >= s.sends.len()) {
            return false;
        }
        let t = Target::SendLevel {
            strip,
            send: send_index(index),
        };
        let norm = StripEntry::level(text).map(fader_norm);
        self.type_value(doc, t, norm, StripEntry::LEVEL_HINT, now)
    }

    pub fn set_mode(&mut self, doc: &mut DocumentModel, strip: u64, mode: AutomationModeRow) {
        self.command(doc, AutomationModes::FAILED, |s| {
            s.set_automation_mode(vec![strip], mode)
        });
    }

    /// Every strip, master included, in the snapshot's order.
    pub fn set_all_modes(&mut self, doc: &mut DocumentModel, mode: AutomationModeRow) {
        let all: Vec<u64> = self.snapshot.strips.iter().map(|s| s.id).collect();
        self.command(doc, AutomationModes::FAILED, |s| {
            s.set_automation_mode(all, mode)
        });
    }

    /// A click selects `strip` alone; ⌘-click (`adding`) adds or removes
    /// it. The core hears the selection (the surfaces show it).
    pub fn select(&mut self, doc: &mut DocumentModel, strip: u64, adding: bool) {
        let mut set = doc.selected_strips().clone();
        if !adding {
            set = BTreeSet::from([strip]);
        } else if !set.remove(&strip) {
            set.insert(strip);
        }
        doc.set_selected_strips(set);
    }

    pub fn add_bus(&mut self, doc: &mut DocumentModel) {
        let name = MixerMath::next_bus_name(&self.snapshot.buses);
        self.command(doc, "Add Bus Failed", |s| s.add_bus(&name));
    }

    /// A new bus fed by a post-fader send from `strip`, one undo step "New
    /// Bus".
    pub fn new_bus(&mut self, doc: &mut DocumentModel, from: u64) {
        let name = MixerMath::next_bus_name(&self.snapshot.buses);
        self.command(doc, "New Bus Failed", |s| s.new_bus_from(from, &name));
    }

    pub fn remove_bus(&mut self, doc: &mut DocumentModel, strip: u64) {
        self.command(doc, "Remove Bus Failed", |s| s.remove_bus(strip));
    }

    /// Routes `strip` to `bus` (`None`: master).
    pub fn set_output(&mut self, doc: &mut DocumentModel, strip: u64, bus: Option<u64>) {
        self.command(doc, "Output Failed", |s| s.set_output(strip, bus));
    }

    /// Sets `strip`'s own colour; the one it has already sends nothing.
    pub fn set_colour(&mut self, doc: &mut DocumentModel, strip: u64, colour: Option<SwatchRow>) {
        if self.strip(strip).is_some_and(|s| s.colour == colour) {
            return;
        }
        self.command(doc, "Strip Colour Failed", |s| {
            s.set_strip_colour(strip, colour)
        });
    }

    pub fn begin_rename(&mut self, strip: u64, p: MixerPlacement) {
        self.rename_draft = self.strip_name(strip);
        self.renaming = Some((strip, p));
    }

    /// Renames to the draft; a blank or unchanged name sends nothing.
    pub fn commit_rename(&mut self, doc: &mut DocumentModel) {
        let Some((id, _)) = self.renaming.take() else {
            return;
        };
        let name = self.rename_draft.trim().to_owned();
        if name.is_empty() || name == self.strip_name(id) {
            return;
        }
        self.command(doc, "Rename Failed", |s| s.rename_strip(id, &name));
    }

    pub fn cancel_rename(&mut self) {
        self.renaming = None;
    }

    // Plumbing.

    /// A gesture closed: the document and the mixer show its step now.
    fn gesture_landed(&mut self, doc: &mut DocumentModel) {
        if let Ok(s) = doc.session().snapshot() {
            doc.apply(s);
        }
        self.refetch(doc);
    }

    /// Runs an edit: its snapshot is applied, a failure is the alert
    /// `title`; the mixer is read again either way. True when it went.
    pub(super) fn command(
        &mut self,
        doc: &mut DocumentModel,
        title: &str,
        body: impl FnOnce(&dyn DocSession) -> Result<DocSnapshot, VizError>,
    ) -> bool {
        let session = doc.session().clone();
        let ok = match body(session.as_ref()) {
            Ok(s) => {
                doc.apply(s);
                true
            }
            Err(e) => {
                raise(doc, AlertItem::from_error(&e, title));
                false
            }
        };
        self.refetch(doc);
        ok
    }

    /// Reads the mixer again. `Busy` skips silently: the previous state
    /// stays and the next tick tries again. Another refusal is the alert
    /// "Mixer Unavailable" (unless one shows).
    fn refetch(&mut self, doc: &mut DocumentModel) {
        let session = doc.session().clone();
        let read = session
            .mixer()
            .and_then(|m| Ok((m, session.destinations()?)));
        match read {
            Ok((snap, dests)) => {
                self.snapshot = snap;
                self.destinations = dests;
                self.fetches += 1;
                self.forget_gone(doc);
            }
            Err(VizError::Busy { .. }) => {
                self.refetch_owed = true;
                return;
            }
            Err(e) => {
                if doc.alert.is_none() {
                    doc.alert = Some(AlertItem::from_error(&e, MIXER_UNAVAILABLE));
                }
            }
        }
        self.refetch_owed = false;
        self.seen = doc.snapshot().version;
    }

    /// Releases (once, quietly: the control went) the touches whose
    /// control the mixer no longer has, ends their drags, and drops gone
    /// strips from the selection.
    fn forget_gone(&mut self, doc: &mut DocumentModel) {
        let snap = &self.snapshot;
        self.editing.retain(|t| exists(*t, snap));
        self.pending.retain(|t, _| exists(*t, snap));
        self.presses.retain(|_, p| exists(p.target, snap));
        self.gestures.retain(|(_, t), _| exists(*t, snap));
        let gone: Vec<Target> = self
            .touches
            .iter()
            .copied()
            .filter(|t| !exists(*t, snap))
            .collect();
        let session: Arc<dyn DocSession> = doc.session().clone();
        for t in gone {
            self.touches.remove(&t);
            self.live.remove(&t);
            let alone = self.alone.remove(&t);
            let _ = send(&session, t, GesturePhaseRow::Release, alone);
        }
        if let Some((id, _)) = self.renaming
            && self.strip(id).is_none()
        {
            self.renaming = None;
        }
        let kept: BTreeSet<u64> = doc
            .selected_strips()
            .iter()
            .copied()
            .filter(|id| self.strip(*id).is_some())
            .collect();
        if &kept != doc.selected_strips() {
            doc.set_selected_strips(kept);
        }
    }
}

/// The input monitor's next mode: Auto, Input, Off, Auto.
pub fn next_monitor(m: MonitorModeRow) -> MonitorModeRow {
    match m {
        MonitorModeRow::Auto => MonitorModeRow::Input,
        MonitorModeRow::Input => MonitorModeRow::Off,
        MonitorModeRow::Off => MonitorModeRow::Auto,
    }
}

/// The input monitor's help for mode `m` (Swift `LaneInputs.help`).
pub fn monitor_help(m: MonitorModeRow) -> &'static str {
    match m {
        MonitorModeRow::Auto => {
            "Input monitoring: Auto (heard while armed and stopped, or recording in range)"
        }
        MonitorModeRow::Input => "Input monitoring: always the input",
        MonitorModeRow::Off => "Input monitoring: off",
    }
}

/// A send's position as the automation target spells it (0-based).
pub fn send_index(index: usize) -> u16 {
    u16::try_from(index).unwrap_or(u16::MAX)
}

/// The normalised value of control position `p` of `t`.
pub fn norm_of(t: Target, p: f64) -> f64 {
    match t {
        Target::StripFader { .. } | Target::VcaFader { .. } | Target::SendLevel { .. } => {
            fader_norm(MixerMath::fader_db(p))
        }
        _ => p,
    }
}

/// What a double-click resets `t` to: 0 dB, or the centre.
pub fn reset_value(t: Target) -> f64 {
    match t {
        Target::StripPan { .. } => 0.5,
        Target::StripFader { .. } | Target::VcaFader { .. } | Target::SendLevel { .. } => {
            fader_norm(0.0)
        }
        _ => 0.0,
    }
}

/// The alert title of a refused gesture on `t`.
pub fn failure_title(t: Target) -> &'static str {
    match t {
        Target::StripFader { .. } => "Fader Failed",
        Target::StripPan { .. } => "Pan Failed",
        Target::SendLevel { .. } => "Send Failed",
        Target::StripMute { .. } => "Mute Failed",
        Target::StripSolo { .. } => "Solo Failed",
        Target::PluginParam { .. } => "Parameter Failed",
        Target::VelocityScale { .. } => "Velocity Failed",
        Target::VcaFader { .. } => "VCA Failed",
    }
}

/// Whether `t` still names a control of `snap`; targets the mixer does not
/// touch count as present.
fn exists(t: Target, snap: &MixerSnapshot) -> bool {
    let strip = |id: u64| snap.strips.iter().find(|s| s.id == id);
    match t {
        Target::StripFader { strip: s }
        | Target::StripPan { strip: s }
        | Target::StripMute { strip: s }
        | Target::StripSolo { strip: s } => strip(s).is_some(),
        Target::SendLevel { strip: s, send } => {
            strip(s).is_some_and(|x| usize::from(send) < x.sends.len())
        }
        Target::VcaFader { vca } => snap.vcas.iter().any(|v| v.id == vca),
        Target::PluginParam { .. } | Target::VelocityScale { .. } => true,
    }
}

/// One gesture phase from the mouse; `alone` keeps the group still.
fn send(
    session: &Arc<dyn DocSession>,
    t: Target,
    phase: GesturePhaseRow,
    alone: bool,
) -> Result<(), VizError> {
    if alone {
        session.gesture_as(t, phase, GestureHolderRow::Mouse, true)
    } else {
        session.gesture(t, phase)
    }
}

/// Sets `item` unless the same alert is already showing.
fn raise(doc: &mut DocumentModel, item: AlertItem) {
    if doc
        .alert
        .as_ref()
        .is_some_and(|a| a.title == item.title && a.message == item.message)
    {
        return;
    }
    doc.alert = Some(item);
}
