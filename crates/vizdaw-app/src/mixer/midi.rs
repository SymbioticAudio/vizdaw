//! External MIDI destinations' strips (Swift `MidiStripMath`,
//! `MixerModel` MIDI strip extension, `MidiStripCells`): volume (CC 7) and
//! pan (CC 10), both 0…127, and mute. A drag previews the controllers
//! without touching the document, at most every 20 ms, and its release
//! commits once (`set_midi_mix`, one undo step "Set MIDI Mix"); a click,
//! a reset, a key step and typed text commit at once. The fader is linear
//! in the value.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::mixer::MidiMixRow;
use vizdaw_ffi::mpe::{MpeRow, MpeZoneRow};
use vizdaw_ffi::snapshot::LaneRow;
use vizdaw_ffi::tracks::TrackKindRow;

use super::gesture::{ControlGesture, MIN_DRAG_DISTANCE};
use super::model::{MixerModel, MixerPlacement};
use crate::document::DocumentModel;

/// The alert title of a refused MIDI mix.
pub const SET_MIDI_MIX_FAILED: &str = "Set MIDI Mix Failed";
/// The least time between two previews of one drag.
pub const MIDI_PREVIEW_INTERVAL: Duration = Duration::from_millis(20);

/// The MIDI strip's value arithmetic and strings (Swift `MidiStripMath`).
pub struct MidiStripMath;

impl MidiStripMath {
    /// What an unset volume shows (and a double-click sets).
    pub const DEFAULT_VOLUME: u8 = 100;
    pub const PAN_CENTRE: u8 = 64;
    /// The left pane's header for a MIDI track without a destination.
    pub const NO_DESTINATION: &'static str = "No Destination";
    /// How much an unset value (or a control that is off) is dimmed.
    pub const DIMMED: f64 = 0.45;
    /// The note-activity meter's width beside the fader.
    pub const METER_WIDTH: f64 = 6.0;
    /// The status line after refused typed text.
    pub const VOLUME_HINT: &'static str = "Type a volume from 0 to 127.";
    pub const PAN_HINT: &'static str = "Type a pan like L 24, C or R 10, or 0 to 127.";
    /// The fader's 0…127 scale.
    pub const FADER_MARKS: [u8; 5] = [127, 100, 64, 32, 0];
    /// A key step of a MIDI control (×10 with ⇧): one value.
    pub const STEP: f64 = 1.0 / 127.0;

    /// "100" when unset, else the number.
    pub fn volume_label(v: Option<u8>) -> String {
        v.unwrap_or(Self::DEFAULT_VOLUME).to_string()
    }

    /// "C", "L 64" … "R 63"; unset reads as the centre.
    pub fn pan_label(p: Option<u8>) -> String {
        match p {
            None => "C".into(),
            Some(p) if p == Self::PAN_CENTRE => "C".into(),
            Some(p) if p < Self::PAN_CENTRE => format!("L {}", Self::PAN_CENTRE - p),
            Some(p) => format!("R {}", p - Self::PAN_CENTRE),
        }
    }

    /// A whole number 0…127 (surrounding white space ignored).
    pub fn parse_volume(s: &str) -> Option<u8> {
        number(s.trim())
    }

    /// "C", "L n" (1…64), "R n" (1…63), any case, or a plain 0…127.
    pub fn parse_pan(s: &str) -> Option<u8> {
        let t = s.trim().to_uppercase();
        if t == "C" {
            return Some(Self::PAN_CENTRE);
        }
        if let Some(rest) = t.strip_prefix('L') {
            let n = number(rest.trim())?;
            return (1..=64).contains(&n).then(|| Self::PAN_CENTRE - n);
        }
        if let Some(rest) = t.strip_prefix('R') {
            let n = number(rest.trim())?;
            return (1..=63).contains(&n).then(|| Self::PAN_CENTRE + n);
        }
        number(&t)
    }

    /// Fader position 0…1 of a volume (an unset one sits at 100).
    pub fn position(v: Option<u8>) -> f64 {
        f64::from(v.unwrap_or(Self::DEFAULT_VOLUME)) / 127.0
    }

    /// The value at control position `p`, clamped (NaN is 0).
    pub fn value_at(p: f64) -> u8 {
        if !p.is_finite() {
            return 0;
        }
        (p.clamp(0.0, 1.0) * 127.0).round() as u8
    }

    /// The pan knob's position 0…1 (an unset pan sits at the centre).
    pub fn pan_position(p: Option<u8>) -> f64 {
        f64::from(p.unwrap_or(Self::PAN_CENTRE)) / 127.0
    }

    /// The fader's scale: (position, label) from the top.
    pub fn fader_marks() -> Vec<(f64, String)> {
        Self::FADER_MARKS
            .iter()
            .map(|v| (f64::from(*v) / 127.0, v.to_string()))
            .collect()
    }

    /// The strip's kind line: "MIDI · Ch 1" (the channel is 0…15), "MIDI"
    /// without one, or "MIDI · MPE Lower" / "MIDI · MPE Upper".
    pub fn kind_label(channel: Option<u8>, mpe: Option<&MpeRow>) -> String {
        if let Some(m) = mpe {
            let zone = match m.zone {
                MpeZoneRow::Lower => "Lower",
                MpeZoneRow::Upper => "Upper",
            };
            return format!("MIDI \u{00B7} MPE {zone}");
        }
        match channel {
            Some(c) => format!("MIDI \u{00B7} Ch {}", u16::from(c) + 1),
            None => "MIDI".into(),
        }
    }
}

/// ASCII digits only, at most three, 0…127.
fn number(t: &str) -> Option<u8> {
    if t.is_empty() || t.len() > 3 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<u8>().ok().filter(|n| *n <= 127)
}

/// The MIDI strip's labels and helps (Swift `MidiStripCells`).
pub struct MidiStripLabels;

impl MidiStripLabels {
    pub const PAN_HELP: &'static str = "Pan: double-click for the centre";
    pub const FADER_HELP: &'static str = "Double-click: 100";
    pub const VOLUME_HELP: &'static str = "Type a volume from 0 to 127";
    pub const MUTE_HELP: &'static str = "Mute";
    pub const NO_DEST_HELP: &'static str = "Choose a destination first";
    pub const ARM_HELP: &'static str = "Record arm";
    pub const NO_TRACK_HELP: &'static str = "No track in this sequence plays this destination.";

    pub fn fader(name: &str) -> String {
        format!("Fader of {name}")
    }
    pub fn volume_box(name: &str) -> String {
        format!("Volume of {name}")
    }
    pub fn pan_knob(name: &str) -> String {
        format!("Pan knob of {name}")
    }
    pub fn pan_box(name: &str) -> String {
        format!("Pan of {name}")
    }
    pub fn mute(name: &str) -> String {
        format!("Mute {name}")
    }
    /// R names the track it arms (the destination without one).
    pub fn arm(name: &str) -> String {
        format!("Record arm {name}")
    }
}

/// A MIDI strip's dragged control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MidiControl {
    Volume,
    Pan,
}

impl MidiControl {
    /// A double-click's value: 100, or the centre.
    pub fn reset_value(self) -> u8 {
        match self {
            MidiControl::Volume => MidiStripMath::DEFAULT_VOLUME,
            MidiControl::Pan => MidiStripMath::PAN_CENTRE,
        }
    }

    fn of(self, v: u8) -> (Option<u8>, Option<u8>) {
        match self {
            MidiControl::Volume => (Some(v), None),
            MidiControl::Pan => (None, Some(v)),
        }
    }
}

/// The MIDI control the mouse went down on in one placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MidiPress {
    pub dest: u64,
    pub control: MidiControl,
}

/// The MIDI strips' drags.
#[derive(Debug, Default)]
pub(super) struct MidiDrags {
    /// Each dragged destination's mix so far.
    pub draft: HashMap<u64, MidiMixRow>,
    /// The committed mix each draft started from.
    pub base: HashMap<u64, MidiMixRow>,
    /// When each drag last previewed.
    pub previewed_at: HashMap<u64, Instant>,
    pub presses: HashMap<MixerPlacement, MidiPress>,
    pub gestures: HashMap<(MixerPlacement, u64, MidiControl), ControlGesture>,
}

impl MixerModel {
    /// The mix `dest`'s strip shows: a drag's draft, else the committed
    /// one; `None` for a destination the mixer does not have.
    pub fn midi_mix(&self, dest: u64) -> Option<MidiMixRow> {
        self.midi
            .draft
            .get(&dest)
            .cloned()
            .or_else(|| self.committed_mix(dest))
    }

    fn committed_mix(&self, dest: u64) -> Option<MidiMixRow> {
        self.destinations()
            .iter()
            .find(|d| d.id == dest)
            .map(|d| d.midi_mix.clone())
    }

    /// Destination `dest`'s note activity, 0…1, falling.
    pub fn midi_activity(&self, dest: u64) -> f64 {
        self.meters().midi_activity(dest)
    }

    /// The control position (0…1) of `control` on `dest`'s strip as shown.
    pub fn midi_position(&self, dest: u64, control: MidiControl) -> f64 {
        let mix = self.midi_mix(dest).unwrap_or_default();
        match control {
            MidiControl::Volume => MidiStripMath::position(mix.volume),
            MidiControl::Pan => MidiStripMath::pan_position(mix.pan),
        }
    }

    /// A drag moved a control: the controllers go out without touching
    /// the document, at most once per [`MIDI_PREVIEW_INTERVAL`]; `None`
    /// leaves that control as it was.
    pub fn midi_drag_to(
        &mut self,
        doc: &DocumentModel,
        dest: u64,
        volume: Option<u8>,
        pan: Option<u8>,
        now: Instant,
    ) {
        if volume.is_none() && pan.is_none() {
            return;
        }
        let Some(mut mix) = self.midi_mix(dest) else {
            return;
        };
        if !self.midi.draft.contains_key(&dest)
            && let Some(c) = self.committed_mix(dest)
        {
            self.midi.base.insert(dest, c);
        }
        if let Some(v) = volume {
            mix.volume = Some(v.min(127));
        }
        if let Some(p) = pan {
            mix.pan = Some(p.min(127));
        }
        self.midi.draft.insert(dest, mix);
        if let Some(last) = self.midi.previewed_at.get(&dest)
            && now >= *last
            && now - *last < MIDI_PREVIEW_INTERVAL
        {
            return;
        }
        self.midi.previewed_at.insert(dest, now);
        // A preview failing is not worth an alert: the release commits.
        let _ = doc.session().preview_midi_mix(dest, volume, pan);
    }

    /// The drag ended: commits what it last moved to, once. A drag that
    /// ended where it began is no undo step.
    pub fn midi_release(&mut self, doc: &mut DocumentModel, dest: u64) {
        self.midi.previewed_at.remove(&dest);
        self.midi.base.remove(&dest);
        let Some(mix) = self.midi.draft.remove(&dest) else {
            return;
        };
        if Some(&mix) == self.committed_mix(dest).as_ref() {
            return;
        }
        self.commit_midi_mix(doc, dest, mix);
    }

    /// The drag was abandoned: its draft goes without a commit, and the
    /// committed volume and pan go out once when either is set, so the
    /// synth matches the strip again.
    pub fn midi_cancel(&mut self, doc: &DocumentModel, dest: u64) {
        self.midi.previewed_at.remove(&dest);
        self.midi.base.remove(&dest);
        if self.midi.draft.remove(&dest).is_none() {
            return;
        }
        let Some(c) = self.committed_mix(dest) else {
            return;
        };
        if c.volume.is_none() && c.pan.is_none() {
            return;
        }
        let _ = doc.session().preview_midi_mix(dest, c.volume, c.pan);
    }

    /// Drops the drafts of destinations that went, are no longer external
    /// or whose committed mix changed under them (an undo, the channel).
    pub(super) fn prune_midi_drafts(&mut self, doc: &DocumentModel) {
        let mut stale: Vec<u64> = self
            .midi
            .draft
            .keys()
            .copied()
            .filter(|dest| {
                let external = self
                    .destinations()
                    .iter()
                    .any(|d| d.id == *dest && d.external);
                !external || self.committed_mix(*dest) != self.midi.base.get(dest).cloned()
            })
            .collect();
        stale.sort_unstable();
        for dest in stale {
            self.midi_cancel(doc, dest);
        }
        self.midi
            .presses
            .retain(|_, p| self.destinations.iter().any(|d| d.id == p.dest));
    }

    /// Reads the destinations again, so a commit builds on what the
    /// document holds now.
    fn fresh_mix(&mut self, doc: &DocumentModel, dest: u64) -> Option<MidiMixRow> {
        if let Ok(d) = doc.session().destinations() {
            self.destinations = d;
            self.prune_midi_drafts(doc);
        }
        self.midi_mix(dest)
    }

    pub fn set_midi_volume(&mut self, doc: &mut DocumentModel, dest: u64, v: u8) {
        let Some(mut mix) = self.fresh_mix(doc, dest) else {
            return;
        };
        mix.volume = Some(v.min(127));
        self.commit_midi_mix(doc, dest, mix);
    }

    pub fn set_midi_pan(&mut self, doc: &mut DocumentModel, dest: u64, p: u8) {
        let Some(mut mix) = self.fresh_mix(doc, dest) else {
            return;
        };
        mix.pan = Some(p.min(127));
        self.commit_midi_mix(doc, dest, mix);
    }

    pub fn toggle_midi_mute(&mut self, doc: &mut DocumentModel, dest: u64) {
        let Some(mut mix) = self.fresh_mix(doc, dest) else {
            return;
        };
        mix.mute = !mix.mute;
        self.commit_midi_mix(doc, dest, mix);
    }

    /// Commits a typed volume. False, with the hint in the status line,
    /// when it does not parse or the destination is unknown.
    pub fn type_midi_volume(&mut self, doc: &mut DocumentModel, dest: u64, text: &str) -> bool {
        match MidiStripMath::parse_volume(text) {
            Some(v) if self.committed_mix(dest).is_some() => {
                self.set_midi_volume(doc, dest, v);
                true
            }
            _ => {
                doc.set_status(Some(MidiStripMath::VOLUME_HINT.into()));
                false
            }
        }
    }

    /// Commits a typed pan, as [`MixerModel::type_midi_volume`].
    pub fn type_midi_pan(&mut self, doc: &mut DocumentModel, dest: u64, text: &str) -> bool {
        match MidiStripMath::parse_pan(text) {
            Some(p) if self.committed_mix(dest).is_some() => {
                self.set_midi_pan(doc, dest, p);
                true
            }
            _ => {
                doc.set_status(Some(MidiStripMath::PAN_HINT.into()));
                false
            }
        }
    }

    fn commit_midi_mix(&mut self, doc: &mut DocumentModel, dest: u64, mix: MidiMixRow) {
        self.midi.draft.remove(&dest);
        self.command(doc, SET_MIDI_MIX_FAILED, |s| s.set_midi_mix(dest, mix));
    }

    // The track R arms.

    /// The track a MIDI strip's R arms in placement `p` (Swift
    /// `midiLane(for:in:)`): in the left pane, the focused track when it
    /// plays `dest`; in every other placement, of the shown sequence's
    /// MIDI and instrument tracks that play `dest`, the first armed one,
    /// else the first.
    pub fn midi_lane<'d>(
        &self,
        doc: &'d DocumentModel,
        dest: u64,
        p: MixerPlacement,
    ) -> Option<&'d LaneRow> {
        let plays = |l: &LaneRow| l.kind != TrackKindRow::Audio && l.midi_dest == Some(dest);
        if p == MixerPlacement::Strip {
            return doc.focus_lanes().first().copied().filter(|l| plays(l));
        }
        let mut through = doc.lanes().iter().filter(|l| plays(l));
        let first = through.clone().next();
        through.find(|l| l.arm).or(first)
    }

    /// Whether `dest`'s strip shows as selected in the mixer (Swift
    /// `isMidiSelected`): a focused track (the selection, else the armed
    /// tracks) plays it.
    pub fn is_midi_selected(&self, doc: &DocumentModel, dest: u64) -> bool {
        doc.focus_lanes().iter().any(|l| l.midi_dest == Some(dest))
    }

    /// The MIDI strip's R: toggles `lane`'s arm (one undo step), as the
    /// track header does.
    pub fn toggle_midi_arm(&mut self, doc: &mut DocumentModel, lane: u64) {
        let Some(arm) = doc.lanes().iter().find(|l| l.id == lane).map(|l| l.arm) else {
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

    // The drag tracker (driven by the view's mouse events).

    /// Mouse-down on `control` of `dest`'s strip in placement `p`. Sends
    /// nothing. `travel` is the points for the whole range (the fader's
    /// throw; [`super::gesture::DRAG_TRAVEL`] for the knob).
    pub fn midi_press(
        &mut self,
        doc: &mut DocumentModel,
        p: MixerPlacement,
        dest: u64,
        control: MidiControl,
        travel: f64,
        now: Instant,
    ) {
        // A press whose mouse-up never came ends first.
        self.lift(doc, p, now);
        self.midi_lift(doc, p, now);
        self.midi.presses.insert(p, MidiPress { dest, control });
        self.midi
            .gestures
            .entry((p, dest, control))
            .or_insert_with(|| ControlGesture::new(travel))
            .travel = travel;
    }

    /// The MIDI control the mouse is down on in `p`.
    pub fn midi_pressed(&self, p: MixerPlacement) -> Option<(u64, MidiControl)> {
        self.midi.presses.get(&p).map(|x| (x.dest, x.control))
    }

    /// A MIDI drag is under way in `p`.
    pub fn is_midi_dragging(&self, p: MixerPlacement) -> bool {
        self.midi
            .presses
            .get(&p)
            .and_then(|x| self.midi.gestures.get(&(p, x.dest, x.control)))
            .is_some_and(ControlGesture::is_dragging)
    }

    /// The mouse moved by (`dx`, `dy`) points from the press (down
    /// positive); `fine` is ⌥ now. From the first move of at least
    /// [`MIN_DRAG_DISTANCE`] every move previews.
    pub fn midi_drag(
        &mut self,
        doc: &DocumentModel,
        p: MixerPlacement,
        dx: f64,
        dy: f64,
        fine: bool,
        now: Instant,
    ) {
        let Some(press) = self.midi.presses.get(&p).copied() else {
            return;
        };
        let start = self.midi_position(press.dest, press.control);
        let Some(g) = self.midi.gestures.get_mut(&(p, press.dest, press.control)) else {
            return;
        };
        if !g.is_dragging() && dx.hypot(dy) < MIN_DRAG_DISTANCE {
            return;
        }
        let Some(change) = g.changed(dy, fine, || start) else {
            return;
        };
        let (volume, pan) = press.control.of(MidiStripMath::value_at(change.value));
        self.midi_drag_to(doc, press.dest, volume, pan, now);
    }

    /// Mouse-up in `p` (or the drag cancelled: the window deactivated, the
    /// control went): a drag under way commits once.
    pub fn midi_lift(&mut self, doc: &mut DocumentModel, p: MixerPlacement, now: Instant) {
        let Some(press) = self.midi.presses.remove(&p) else {
            return;
        };
        let ended = self
            .midi
            .gestures
            .get_mut(&(p, press.dest, press.control))
            .is_some_and(|g| g.ended(now));
        if ended {
            self.midi_release(doc, press.dest);
        }
    }

    /// A double-click on `control`: 100 or the centre, unless a drag is
    /// under way or ended less than [`super::gesture::SETTLE`] ago.
    pub fn midi_double_click(
        &mut self,
        doc: &mut DocumentModel,
        p: MixerPlacement,
        dest: u64,
        control: MidiControl,
        now: Instant,
    ) {
        let reset = self
            .midi
            .gestures
            .get(&(p, dest, control))
            .is_none_or(|g| g.double_tapped(now));
        if !reset {
            return;
        }
        match control {
            MidiControl::Volume => self.set_midi_volume(doc, dest, control.reset_value()),
            MidiControl::Pan => self.set_midi_pan(doc, dest, control.reset_value()),
        }
    }

    /// A key step of `delta` of position ([`MidiStripMath::STEP`], ×10
    /// with ⇧): a preview and its commit.
    pub fn midi_nudge(
        &mut self,
        doc: &mut DocumentModel,
        dest: u64,
        control: MidiControl,
        delta: f64,
        now: Instant,
    ) {
        let p = (self.midi_position(dest, control) + delta).clamp(0.0, 1.0);
        let (volume, pan) = control.of(MidiStripMath::value_at(p));
        self.midi_drag_to(doc, dest, volume, pan, now);
        self.midi_release(doc, dest);
    }

    /// Lifts every MIDI press (the last placement went, the document
    /// closes).
    pub(super) fn lift_midi_presses(&mut self, doc: &mut DocumentModel, now: Instant) {
        let mut open: Vec<MixerPlacement> = self.midi.presses.keys().copied().collect();
        open.sort();
        for p in open {
            self.midi_lift(doc, p, now);
        }
        self.midi.gestures.clear();
    }
}
