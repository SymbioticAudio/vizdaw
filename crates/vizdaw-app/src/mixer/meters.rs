//! Meter state and ballistics (Swift `MixerModel.pollLevels`): each poll
//! holds `max(peak, old × 0.85)` per 30 Hz period, the decay scaled by the
//! time that passed; a strip missing from a take falls to silence. No clip
//! latch and no peak-hold line (Swift has none).

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use vizdaw_ffi::mixer::MixerLevels;

use super::math::MixerMath;

/// The period the 0.85 decay is per (30 Hz).
pub const METER_PERIOD: Duration = Duration::from_nanos(33_333_333);
/// The least time between two reads of the levels (Swift `tickInterval`).
pub const POLL_INTERVAL: Duration = Duration::from_millis(30);
/// Below this a held peak is silence (−60 dBFS, the meter's bottom).
pub const SILENCE: f32 = 0.001;
/// Below this a MIDI destination's activity is gone (Swift's 0.005).
pub const MIDI_SILENCE: f64 = 0.005;

/// A meter's two channels as shown (peaks with a falling hold).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MeterLevel {
    pub left: f32,
    pub right: f32,
}

impl MeterLevel {
    /// No signal.
    pub const SILENT: MeterLevel = MeterLevel {
        left: 0.0,
        right: 0.0,
    };

    fn scaled(self, k: f32) -> MeterLevel {
        MeterLevel {
            left: self.left * k,
            right: self.right * k,
        }
    }

    fn is_silent(self) -> bool {
        self.left < SILENCE && self.right < SILENCE
    }
}

/// How much a held peak keeps after `dt`: 0.85 per [`METER_PERIOD`].
pub fn decay(dt: Duration) -> f32 {
    let periods = dt.as_secs_f64() / METER_PERIOD.as_secs_f64();
    if periods == 1.0 {
        return MixerMath::METER_DECAY;
    }
    f64::from(MixerMath::METER_DECAY).powf(periods) as f32
}

/// What the mixer last read of the levels, and when.
#[derive(Debug, Clone, Default)]
pub struct Meters {
    levels: HashMap<u64, MeterLevel>,
    /// When `levels` were set (`None`: never).
    at: Option<Instant>,
    pub dsp_load: f32,
    pub dsp_peak: f32,
    pub latency_frames: u64,
    /// Plugins the engine bypassed for faults (NaN output, errors).
    pub faulted: BTreeSet<u64>,
    /// Each MIDI destination's note activity, 0…1, falling.
    midi: HashMap<u64, f64>,
}

/// A load as the load text shows it ("12").
fn percent(x: f32) -> String {
    format!("{:.0}", f64::from(x) * 100.0)
}

/// Two reads show the same when everything but their time is the same,
/// the loads compared as the load text shows them (a jitter inside one
/// percent is no change).
impl PartialEq for Meters {
    fn eq(&self, o: &Self) -> bool {
        self.levels == o.levels
            && percent(self.dsp_load) == percent(o.dsp_load)
            && percent(self.dsp_peak) == percent(o.dsp_peak)
            && self.latency_frames == o.latency_frames
            && self.faulted == o.faulted
            && self.midi == o.midi
    }
}

impl Meters {
    /// Takes one read of the levels at `now`. An empty read (a session
    /// that does not hold the engine) is silence falling, not an error.
    pub fn take(&mut self, l: &MixerLevels, now: Instant) {
        let k = match self.at {
            Some(at) => decay(now.saturating_duration_since(at)),
            None => 0.0,
        };
        let mut next: HashMap<u64, MeterLevel> = HashMap::new();
        // A strip missing from the take falls toward silence.
        for (strip, old) in &self.levels {
            let held = old.scaled(k);
            if !held.is_silent() {
                next.insert(*strip, held);
            }
        }
        for m in &l.meters {
            let held = next.get(&m.strip).copied().unwrap_or_default();
            next.insert(
                m.strip,
                MeterLevel {
                    left: MixerMath::sanitized_peak(m.peak_l).max(held.left),
                    right: MixerMath::sanitized_peak(m.peak_r).max(held.right),
                },
            );
        }
        self.levels = next;
        self.at = Some(now);
        self.dsp_load = l.dsp_load;
        self.dsp_peak = l.dsp_peak;
        self.latency_frames = l.latency_frames;
        self.faulted = l.faulted.iter().copied().collect();
        let k = f64::from(k);
        let mut activity: HashMap<u64, f64> = self
            .midi
            .iter()
            .map(|(d, v)| (*d, v * k))
            .filter(|(_, v)| *v >= MIDI_SILENCE)
            .collect();
        for m in &l.midi {
            let v = f64::from(m.velocity.min(127)) / 127.0;
            let e = activity.entry(m.dest).or_default();
            *e = e.max(v);
        }
        self.midi = activity;
    }

    /// Strip `strip`'s meter as last read.
    pub fn level(&self, strip: u64) -> MeterLevel {
        self.levels.get(&strip).copied().unwrap_or_default()
    }

    /// Strip `strip`'s meter as drawn at `now`: the last read, fallen by
    /// the time since it (a frame between two reads).
    pub fn level_at(&self, strip: u64, now: Instant) -> MeterLevel {
        let l = self.level(strip);
        match self.at {
            Some(at) => l.scaled(decay(now.saturating_duration_since(at))),
            None => l,
        }
    }

    /// The strips with a meter above silence.
    pub fn strips(&self) -> impl Iterator<Item = u64> + '_ {
        self.levels.keys().copied()
    }

    /// Destination `dest`'s note activity, 0…1.
    pub fn midi_activity(&self, dest: u64) -> f64 {
        self.midi.get(&dest).copied().unwrap_or(0.0)
    }

    /// "DSP 12% · peak 30% · PDC 10.7 ms" (no PDC before a sample rate).
    pub fn load_text(&self, sample_rate: f64) -> String {
        let pdc = if sample_rate > 0.0 {
            format!(
                " \u{00B7} PDC {:.1} ms",
                self.latency_frames as f64 / sample_rate * 1000.0
            )
        } else {
            String::new()
        };
        format!(
            "DSP {}% \u{00B7} peak {}%{pdc}",
            percent(self.dsp_load),
            percent(self.dsp_peak)
        )
    }
}
