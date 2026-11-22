//! The transport (Swift: `DocumentModel`'s transport part and
//! `TransportReadouts`). No GPUI and no timers: the caller calls `poll()`
//! every `poll_interval()`.

use std::sync::Arc;
use std::time::Duration;

use vizdaw_ffi::VizError;
use vizdaw_ffi::events::MeterEntryRow;
use vizdaw_ffi::snapshot::TransportState;

use crate::alerts::AlertItem;
use crate::document::DocumentModel;
use crate::session::DocSession;
use crate::store::DocumentStore;

/// How a poll ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollResult {
    /// The transport was read.
    Updated,
    /// The plugin host was busy: the last transport stays, silently.
    Busy,
    /// The read failed: one alert shows (not one per poll).
    Failed,
}

/// The transport shown until the first successful read.
fn stopped() -> TransportState {
    TransportState {
        playing: false,
        seq: None,
        tick: 0,
        position_bbu: "1.1.0".into(),
        seconds: 0.0,
        sample_rate: 0.0,
        device: String::new(),
        faults: vec![],
        report: vec![],
        host_time_ns: 0,
        ticks_per_second: 30_720.0,
        rate_mismatch: false,
        counting_in: false,
        click: None,
    }
}

/// The transport of one document: the last state read, the sequence it
/// plays and where playing started.
pub struct TransportModel {
    session: Arc<dyn DocSession>,
    state: TransportState,
    selected: Option<u64>,
    play_start: Option<i64>,
}

impl TransportModel {
    /// Reads the transport once. Busy (an AU v3 editor opening in a nested
    /// run loop) starts stopped at the beginning; the next poll reads the
    /// real state.
    pub fn new(doc: &DocumentModel) -> Result<Self, VizError> {
        let state = match doc.session().transport() {
            Ok(t) => t,
            Err(VizError::Busy { .. }) => stopped(),
            Err(e) => return Err(e),
        };
        Ok(TransportModel {
            session: doc.session().clone(),
            state,
            selected: None,
            play_start: None,
        })
    }

    pub fn state(&self) -> &TransportState {
        &self.state
    }

    pub fn is_playing(&self) -> bool {
        self.state.playing
    }

    /// Where the last Play started; Stop returns there.
    pub fn play_start(&self) -> Option<i64> {
        self.play_start
    }

    /// Chooses the sequence Play plays.
    pub fn select(&mut self, seq: u64) {
        self.selected = Some(seq);
    }

    /// The sequence Play plays: the chosen one while it exists, else the
    /// first top-level sequence, else the first.
    pub fn selected_seq(&self, doc: &DocumentModel) -> Option<u64> {
        let seqs = &doc.snapshot().sequences;
        match self.selected {
            Some(id) if seqs.iter().any(|s| s.id == id) => Some(id),
            _ => seqs
                .iter()
                .find(|s| s.top_level)
                .or_else(|| seqs.first())
                .map(|s| s.id),
        }
    }

    /// Whether Play does anything: a sequence is shown, or it is playing.
    pub fn can_play(&self, doc: &DocumentModel) -> bool {
        self.selected_seq(doc).is_some() || self.is_playing()
    }

    /// How often the caller polls: fast while playing, slow otherwise.
    pub fn poll_interval(&self) -> Duration {
        if self.is_playing() {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(250)
        }
    }

    /// Stops when playing, else plays the shown sequence. A failure is an
    /// alert; the transport is polled either way.
    pub fn play_stop(&mut self, doc: &DocumentModel, store: &mut DocumentStore) {
        let result = if self.is_playing() {
            self.session.stop()
        } else if self.selected_seq(doc).is_some() {
            self.start_playing(doc, store, None, None)
        } else {
            Ok(())
        };
        if let Err(e) = result {
            let title = if self.is_playing() {
                "Stop Failed"
            } else {
                "Play Failed"
            };
            store.post_alert(AlertItem::from_error(&e, title));
        }
        self.poll(store);
    }

    /// Plays the shown sequence from `from` (`None`: where the transport
    /// is), up to `to` where it stops, and remembers where it started.
    pub fn start_playing(
        &mut self,
        doc: &DocumentModel,
        store: &mut DocumentStore,
        from: Option<i64>,
        to: Option<i64>,
    ) -> Result<(), VizError> {
        let seq = self.selected_seq(doc).ok_or_else(|| VizError::Model {
            message: "Select a sequence to play.".into(),
        })?;
        // The engine restarts at bar 1 from the end, or from another
        // sequence's position.
        let length = doc
            .snapshot()
            .sequences
            .iter()
            .find(|s| s.id == seq)
            .map_or(0, |s| s.length_ticks);
        let there = self.state.seq.is_none() || self.state.seq == Some(seq);
        let start = from.unwrap_or(if there && self.state.tick < length {
            self.state.tick
        } else {
            0
        });
        match from {
            Some(f) => self.session.play_from(seq, f, to)?,
            None => self.session.play(seq)?,
        }
        self.play_start = Some(start);
        self.poll(store);
        Ok(())
    }

    /// Stops and goes back to where playing started. The error is the
    /// caller's to show (keys report a refusal in the status line).
    pub fn stop_and_return(&mut self, store: &mut DocumentStore) -> Result<(), VizError> {
        self.session.stop()?;
        self.session.locate(self.play_start.unwrap_or(0))?;
        self.poll(store);
        Ok(())
    }

    /// Reads the transport. A failure shows one alert, not one per poll;
    /// Busy keeps the last transport, silently.
    pub fn poll(&mut self, store: &mut DocumentStore) -> PollResult {
        match self.session.transport() {
            Ok(t) => {
                self.state = t;
                PollResult::Updated
            }
            Err(VizError::Busy { .. }) => PollResult::Busy,
            Err(e) => {
                if store.alert().is_none() {
                    store.post_alert(AlertItem::from_error(&e, "Transport Failed"));
                }
                PollResult::Failed
            }
        }
    }

    /// The four readout texts; `meter` is the sequence info's meter map
    /// (`None` before it is known), which the caller owns.
    pub fn readout(&self, meter: Option<&[MeterEntryRow]>) -> TransportReadout {
        TransportReadout {
            texts: TransportReadouts::texts(&self.state, meter),
            playing: self.state.playing,
        }
    }
}

/// What the transport bar shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportReadout {
    pub texts: ReadoutTexts,
    pub playing: bool,
}

/// The four readouts' texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadoutTexts {
    pub bar: String,
    pub time: String,
    pub tempo: String,
    pub signature: String,
}

impl ReadoutTexts {
    pub fn text(&self, b: ReadoutBox) -> &str {
        match b {
            ReadoutBox::Bar => &self.bar,
            ReadoutBox::Time => &self.time,
            ReadoutBox::Tempo => &self.tempo,
            ReadoutBox::Signature => &self.signature,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadoutBox {
    Bar,
    Time,
    Tempo,
    Signature,
}

impl ReadoutBox {
    pub const ALL: [ReadoutBox; 4] = [
        ReadoutBox::Bar,
        ReadoutBox::Time,
        ReadoutBox::Tempo,
        ReadoutBox::Signature,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ReadoutBox::Bar => "Bar",
            ReadoutBox::Time => "Time",
            ReadoutBox::Tempo => "Tempo",
            ReadoutBox::Signature => "Sig",
        }
    }

    pub fn accessibility_label(self) -> &'static str {
        match self {
            ReadoutBox::Bar => "Bar Readout",
            ReadoutBox::Time => "Time Readout",
            ReadoutBox::Tempo => "Tempo Readout",
            ReadoutBox::Signature => "Signature Readout",
        }
    }
}

/// The text formats of the readouts (Swift: `TransportReadouts`, `Formatting`).
pub struct TransportReadouts;

impl TransportReadouts {
    /// The dash for a value that is not there.
    pub const DASH: &'static str = "\u{2013}";
    /// Positions are clamped to ±2^50 (Swift: `TimeAxis.limit`).
    pub const TICK_LIMIT: i64 = 1 << 50;
    /// The widest text each box shows.
    pub const WIDEST: WidestTexts = WidestTexts {
        bar: "9999.4.960",
        time: "999:59.999",
        tempo: "999.99",
        signature: "255/128",
    };

    /// `ticks_per_second` as BPM to two places; a dash for 0, a negative or
    /// non-finite value, and for one above 999.99 once rounded.
    pub fn tempo_text(ticks_per_second: f64) -> String {
        if !ticks_per_second.is_finite() || ticks_per_second <= 0.0 {
            return Self::DASH.into();
        }
        let hundredths = (ticks_per_second * 60.0 / 15_360.0 * 100.0).round();
        if !hundredths.is_finite() || hundredths > 99_999.0 {
            return Self::DASH.into();
        }
        format!("{:.2}", hundredths / 100.0)
    }

    /// The meter in effect at `tick`; a dash with no sequence info.
    pub fn signature_text(tick: i64, meter: Option<&[MeterEntryRow]>) -> String {
        let Some(meter) = meter else {
            return Self::DASH.into();
        };
        let t = tick.clamp(-Self::TICK_LIMIT, Self::TICK_LIMIT);
        match meter
            .iter()
            .rfind(|m| m.tick <= t)
            .or_else(|| meter.first())
        {
            Some(m) => format!("{}/{}", m.num, m.den),
            None => "4/4".into(),
        }
    }

    pub fn texts(t: &TransportState, meter: Option<&[MeterEntryRow]>) -> ReadoutTexts {
        ReadoutTexts {
            bar: format_bbu(&t.position_bbu),
            time: format_time(t.seconds),
            tempo: Self::tempo_text(t.ticks_per_second),
            signature: Self::signature_text(t.tick, meter),
        }
    }
}

/// The widest text of each box, as plain strings.
pub struct WidestTexts {
    pub bar: &'static str,
    pub time: &'static str,
    pub tempo: &'static str,
    pub signature: &'static str,
}

/// `m:ss.fff`, with a leading `-` before zero (pickups). Non-finite input
/// gives `--:--.---`; huge values are clamped.
pub fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() {
        return "--:--.---".into();
    }
    let total_ms = (seconds.abs() * 1000.0).round().min(9e18) as i64;
    let sign = if seconds < 0.0 && total_ms > 0 {
        "-"
    } else {
        ""
    };
    format!(
        "{sign}{}:{:02}.{:03}",
        total_ms / 60_000,
        (total_ms / 1000) % 60,
        total_ms % 1000
    )
}

/// Bar.beat.unit as the core formats it; empty input shows `-.-.-`.
pub fn format_bbu(position: &str) -> String {
    if position.is_empty() {
        "-.-.-".into()
    } else {
        position.into()
    }
}
