//! Audio-region commands. Frame/tick conversion uses the owning sequence's
//! tempo map and the media item's sample rate.

use super::Report;
use super::event::insert_specs;
use crate::document::Document;
use crate::error::{ModelError, Result};
use crate::event::{Event, EventBody, EventSpec, Fade};
use crate::ids::{AnyId, EventId, LaneId, MediaId, SeqId};
use crate::rules::check_event_fields;
use crate::time::Tick;

/// Audio-region edits.
#[derive(Debug, Clone, PartialEq)]
pub enum AudioCmd {
    /// Insert a region into a lane's active take.
    Insert {
        /// Sequence.
        seq: SeqId,
        /// Lane.
        lane: LaneId,
        /// Start tick.
        tick: Tick,
        /// Media item.
        media: MediaId,
        /// Start within the media (frames).
        source_offset: u64,
        /// Length (frames, >= 1).
        length: u64,
        /// Gain in dB.
        gain_db: f32,
        /// Fade in.
        fade_in: Fade,
        /// Fade out.
        fade_out: Fade,
    },
    /// Trim frames from the front and back (negative values extend).
    Trim {
        /// The region.
        event: EventId,
        /// Frames removed from the start (the region start moves later).
        front_frames: i64,
        /// Frames removed from the end.
        back_frames: i64,
    },
    /// Split a region in two at `at` (strictly inside it). The second part
    /// gets a new ID, reported as created.
    Split {
        /// The region.
        event: EventId,
        /// Split position.
        at: Tick,
    },
    /// Set gain.
    SetGain {
        /// The region.
        event: EventId,
        /// Gain in dB.
        gain_db: f32,
    },
    /// Set fades.
    SetFades {
        /// The region.
        event: EventId,
        /// Fade in.
        fade_in: Fade,
        /// Fade out.
        fade_out: Fade,
    },
    /// Set each region's gain so its loudest sample reaches `peak_db`
    /// (spec §4.4; non-destructive). Each region comes with the linear
    /// peak of the frames it plays, measured from its overview (5b
    /// Ruling 5). Events that are not regions are ignored.
    Normalize {
        /// `(region, peak)`: the peak as a linear amplitude (> 0).
        regions: Vec<(EventId, f32)>,
        /// The target, −96…0 dBFS.
        peak_db: f32,
    },
    /// The fade in ends (`In`) or the fade out starts (`Out`) at `at`, in
    /// every region `at` lies strictly inside (5b Ruling 6).
    FadeTo {
        /// The events (non-regions are ignored).
        events: Vec<EventId>,
        /// The cursor.
        at: Tick,
        /// Which fade.
        edge: FadeEdge,
    },
    /// Removes both fades of every region.
    RemoveFades {
        /// The events (non-regions are ignored).
        events: Vec<EventId>,
    },
}

/// Which fade a [`AudioCmd::FadeTo`] sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeEdge {
    /// The fade in.
    In,
    /// The fade out.
    Out,
}

impl AudioCmd {
    /// Stable command name.
    pub fn name(&self) -> &'static str {
        match self {
            AudioCmd::Insert { .. } => "Insert Audio Region",
            AudioCmd::Trim { .. } => "Trim Region",
            AudioCmd::Split { .. } => "Split Region",
            AudioCmd::SetGain { .. } => "Set Region Gain",
            AudioCmd::SetFades { .. } => "Set Region Fades",
            AudioCmd::Normalize { .. } => "Normalize",
            AudioCmd::FadeTo {
                edge: FadeEdge::In, ..
            } => "Fade In to Cursor",
            AudioCmd::FadeTo {
                edge: FadeEdge::Out,
                ..
            } => "Fade Out from Cursor",
            AudioCmd::RemoveFades { .. } => "Remove Fades",
        }
    }
}

/// The region event, its owning sequence and the media sample rate.
fn region(d: &Document, id: EventId) -> Result<(Event, SeqId, f64)> {
    let (ev, loc) = d.event(id)?;
    let EventBody::AudioRegion { media, .. } = ev.body else {
        return Err(ModelError::invalid(format!("{id} is not an audio region")));
    };
    let rate = d
        .media()
        .get(&media)
        .map(|m| f64::from(m.sample_rate.max(1)))
        .ok_or(ModelError::UnknownId(AnyId::Media(media)))?;
    Ok((ev.clone(), loc.seq, rate))
}

/// Shortens a region's fades so together they fit it: the fade in first,
/// then the fade out to what is left (5a Ruling 1).
pub(crate) fn clamp_fades(ev: &mut Event) {
    if let EventBody::AudioRegion {
        length,
        fade_in,
        fade_out,
        ..
    } = &mut ev.body
    {
        fade_in.length_frames = fade_in.length_frames.min(*length);
        fade_out.length_frames = fade_out
            .length_frames
            .min(length.saturating_sub(fade_in.length_frames));
    }
}

fn store(d: &mut Document, ev: Event) -> Result<()> {
    check_event_fields(d, &ev.spec())?;
    d.replace_event(ev)?;
    Ok(())
}

pub(crate) fn apply(d: &mut Document, cmd: &AudioCmd, r: &mut Report) -> Result<()> {
    match cmd {
        AudioCmd::Insert {
            seq,
            lane,
            tick,
            media,
            source_offset,
            length,
            gain_db,
            fade_in,
            fade_out,
        } => {
            // A region plays on its lane's output path: a lane without one
            // takes the first, or a new "Audio 1" (5a Ruling 6).
            if d.lane(*seq, *lane)?.audio_out.is_none() {
                let out = match d.outputs().keys().next().copied() {
                    Some(o) => o,
                    None => super::routing::add_output(d, "Audio 1", "audio:1-2", r),
                };
                d.lane_mut(*seq, *lane)?.audio_out = Some(out);
            }
            let spec = EventSpec::new(
                *tick,
                EventBody::AudioRegion {
                    media: *media,
                    source_offset: *source_offset,
                    length: *length,
                    gain_db: *gain_db,
                    fade_in: *fade_in,
                    fade_out: *fade_out,
                },
            );
            insert_specs(d, *seq, *lane, &[spec], r)?;
        }
        AudioCmd::Trim {
            event,
            front_frames,
            back_frames,
        } => {
            let (mut ev, seq, rate) = region(d, *event)?;
            let tempo = d.sequence(seq)?.tempo.clone();
            if let EventBody::AudioRegion {
                source_offset,
                length,
                ..
            } = &mut ev.body
            {
                let new_offset = i128::from(*source_offset) + i128::from(*front_frames);
                let new_len =
                    i128::from(*length) - i128::from(*front_frames) - i128::from(*back_frames);
                *source_offset = u64::try_from(new_offset)
                    .map_err(|_| ModelError::range("region.source_offset", new_offset))?;
                *length = u64::try_from(new_len)
                    .map_err(|_| ModelError::range("region.length", new_len))?;
                let secs = tempo.tick_to_seconds(ev.tick) + *front_frames as f64 / rate;
                ev.tick = tempo.seconds_to_tick(secs);
            }
            clamp_fades(&mut ev);
            store(d, ev)?;
        }
        AudioCmd::Split { event, at } => {
            let (ev, seq, rate) = region(d, *event)?;
            let tempo = d.sequence(seq)?.tempo.clone();
            let EventBody::AudioRegion {
                media,
                source_offset,
                length,
                gain_db,
                fade_in,
                fade_out,
            } = ev.body.clone()
            else {
                return Ok(());
            };
            let secs = tempo.tick_to_seconds(*at) - tempo.tick_to_seconds(ev.tick);
            let first = (secs * rate).round();
            if *at <= ev.tick || first < 1.0 || first >= length as f64 {
                return Err(ModelError::range("split.at", at.0));
            }
            let first = first as u64;
            let mut a = ev.clone();
            a.body = EventBody::AudioRegion {
                media,
                source_offset,
                length: first,
                gain_db,
                fade_in,
                fade_out: Fade::default(),
            };
            clamp_fades(&mut a);
            store(d, a)?;
            let id = d.counters_mut().alloc_event();
            let mut b = Event {
                id,
                tick: *at,
                dest: ev.dest,
                group_channel: ev.group_channel,
                mute: ev.mute,
                body: EventBody::AudioRegion {
                    media,
                    source_offset: source_offset + first,
                    length: length - first,
                    gain_db,
                    fade_in: Fade::default(),
                    fade_out,
                },
            };
            clamp_fades(&mut b);
            check_event_fields(d, &b.spec())?;
            let loc = d.locate(*event)?;
            d.insert_event(loc.seq, loc.lane, Some(loc.take), b)?;
            r.created.push(AnyId::Event(id));
        }
        AudioCmd::SetGain { event, gain_db } => {
            let (mut ev, _, _) = region(d, *event)?;
            if let EventBody::AudioRegion { gain_db: g, .. } = &mut ev.body {
                *g = *gain_db;
            }
            store(d, ev)?;
        }
        AudioCmd::SetFades {
            event,
            fade_in,
            fade_out,
        } => {
            let (mut ev, _, _) = region(d, *event)?;
            if let EventBody::AudioRegion {
                fade_in: fi,
                fade_out: fo,
                ..
            } = &mut ev.body
            {
                *fi = *fade_in;
                *fo = *fade_out;
            }
            store(d, ev)?;
        }
        AudioCmd::Normalize { regions, peak_db } => {
            if !(peak_db.is_finite() && (-96.0..=0.0).contains(peak_db)) {
                return Err(ModelError::range("normalize.peak_db", peak_db));
            }
            let mut changed = 0usize;
            for (id, peak) in regions {
                if !is_region(d, *id)? {
                    continue;
                }
                if !(peak.is_finite() && *peak > 0.0) {
                    r.warnings.push(format!("{id} is silent: not normalized"));
                    continue;
                }
                let want = f64::from(*peak_db) - 20.0 * f64::from(*peak).log10();
                let g = want.clamp(-144.0, 24.0) as f32;
                if (want - f64::from(g)).abs() > 0.05 {
                    r.warnings.push(format!("{id}: gain limited to {g:+.1} dB"));
                }
                let (mut ev, _, _) = region(d, *id)?;
                if let EventBody::AudioRegion { gain_db, .. } = &mut ev.body {
                    *gain_db = g;
                }
                store(d, ev)?;
                changed += 1;
            }
            if changed == 0 {
                return Err(ModelError::invalid(
                    "no selected audio region has sound to normalize",
                ));
            }
        }
        AudioCmd::FadeTo { events, at, edge } => {
            let mut changed = 0usize;
            for id in events {
                if !is_region(d, *id)? {
                    continue;
                }
                let (mut ev, seq, rate) = region(d, *id)?;
                let tempo = d.sequence(seq)?.tempo.clone();
                let secs = tempo.tick_to_seconds(*at) - tempo.tick_to_seconds(ev.tick);
                let f = (secs * rate).round();
                let EventBody::AudioRegion {
                    length,
                    fade_in,
                    fade_out,
                    ..
                } = &mut ev.body
                else {
                    continue;
                };
                // Strictly inside: a fade of 0 or of the whole region is
                // not "to the cursor".
                if !(f >= 1.0 && f < *length as f64) {
                    continue;
                }
                let f = f as u64;
                match edge {
                    FadeEdge::In => {
                        fade_in.length_frames = f;
                        fade_out.length_frames = fade_out.length_frames.min(*length - f);
                    }
                    FadeEdge::Out => {
                        fade_out.length_frames = *length - f;
                        fade_in.length_frames = fade_in.length_frames.min(f);
                    }
                }
                store(d, ev)?;
                changed += 1;
            }
            if changed == 0 {
                return Err(ModelError::invalid(
                    "the cursor is not inside a selected audio region",
                ));
            }
        }
        AudioCmd::RemoveFades { events } => {
            let mut regions = 0usize;
            for id in events {
                if !is_region(d, *id)? {
                    continue;
                }
                let (mut ev, _, _) = region(d, *id)?;
                if let EventBody::AudioRegion {
                    fade_in, fade_out, ..
                } = &mut ev.body
                {
                    *fade_in = Fade::default();
                    *fade_out = Fade::default();
                }
                store(d, ev)?;
                regions += 1;
            }
            if regions == 0 {
                return Err(ModelError::invalid(
                    "select audio regions to remove their fades",
                ));
            }
        }
    }
    Ok(())
}

/// Whether event `id` is an audio region (an unknown ID is an error).
fn is_region(d: &Document, id: EventId) -> Result<bool> {
    Ok(matches!(d.event(id)?.0.body, EventBody::AudioRegion { .. }))
}
