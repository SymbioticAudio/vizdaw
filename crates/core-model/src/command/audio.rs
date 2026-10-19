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
    }
    Ok(())
}
