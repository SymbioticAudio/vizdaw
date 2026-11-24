//! The transport (Swift: `DocumentModel`'s transport part and
//! `TransportReadouts`). No GPUI and no timers: the caller calls `poll()`
//! every `poll_interval()`.

use std::cell::Cell;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vizdaw_ffi::VizError;
use vizdaw_ffi::events::MeterEntryRow;
use vizdaw_ffi::snapshot::TransportState;

use core_control::{ControlError, ErrorCode};
use vizdaw_ffi::surfaces::shuttle_quarters;

use crate::alerts::AlertItem;
use crate::commands::{self, TEXT_FIELD_HAS_KEYBOARD, from_viz};
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
pub(crate) fn stopped() -> TransportState {
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
    /// The document's last good read, which every poll feeds.
    shared: Arc<Mutex<TransportState>>,
    /// The document's Disk too slow dismissal, cleared by a read with no
    /// underrun.
    disk_warning_dismissed: Arc<std::sync::atomic::AtomicBool>,
    /// Shared with the document, whose commands start playing too.
    play_start: Arc<Mutex<Option<i64>>>,
    clock: Clock,
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
            shared: doc.shared_transport(),
            disk_warning_dismissed: doc.shared_disk_warning(),
            play_start: doc.shared_play_start(),
            clock: Clock::new(),
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
        *self.play_start.lock().unwrap()
    }

    /// The playhead clock, fed by every poll.
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// The sequence Play plays: the document's shown one.
    pub fn selected_seq(&self, doc: &DocumentModel) -> Option<u64> {
        doc.selected_seq()
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
        if let Err((playing, e)) = doc.try_play_stop() {
            let title = if playing {
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
        doc.start_playing(from, to)?;
        self.poll(store);
        Ok(())
    }

    /// Stops and goes back to where playing started. The error is the
    /// caller's to show (keys report a refusal in the status line).
    pub fn stop_and_return(&mut self, store: &mut DocumentStore) -> Result<(), VizError> {
        crate::document::stop_and_return(self.session.as_ref(), &self.play_start)?;
        self.poll(store);
        Ok(())
    }

    /// Reads the transport. A failure shows one alert, not one per poll;
    /// Busy keeps the last transport, silently.
    pub fn poll(&mut self, store: &mut DocumentStore) -> PollResult {
        match self.session.transport() {
            Ok(t) => {
                self.clock
                    .sample(t.tick, t.playing, t.seq, t.ticks_per_second, Instant::now());
                crate::banners::note_transport(&self.disk_warning_dismissed, &t);
                *self.shared.lock().unwrap() = t.clone();
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

    /// The readouts of `doc`'s transport: tempo and meter come from the
    /// sequence it plays, else the shown one.
    pub fn readout_of(&self, doc: &DocumentModel) -> TransportReadout {
        let seq = self.state.seq.or(doc.selected_seq());
        let info = seq.and_then(|s| doc.sequence_info(s));
        self.readout(info.map(|i| i.meter.as_slice()))
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

/// Extrapolates the playhead between transport polls (Swift:
/// `TransportClock`): `tick + min(elapsed, poll) x ticks_per_second`.
/// While playing it never steps back by less than 0.1 s of ticks (poll
/// jitter); it snaps on bigger backward jumps (a locate, a loop wrap), on
/// start, stop and a change of sequence. Time is passed in.
pub struct Clock {
    sample: Option<Sample>,
    /// The tick drawn last.
    shown: Cell<f64>,
}

/// One transport reading, as the clock keeps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub tick: i64,
    pub playing: bool,
    pub seq: Option<u64>,
    pub ticks_per_second: f64,
    pub at: Instant,
}

impl Clock {
    /// How far the extrapolation may run past the last sample: the poll
    /// interval while playing.
    pub const POLL: Duration = Duration::from_millis(33);
    /// How far back the playhead may be read without snapping.
    pub const HOLD: Duration = Duration::from_millis(100);

    pub fn new() -> Self {
        Clock {
            sample: None,
            shown: Cell::new(0.0),
        }
    }

    /// The last reading, if any.
    pub fn last(&self) -> Option<&Sample> {
        self.sample.as_ref()
    }

    /// A new transport reading taken at `at`.
    pub fn sample(&mut self, tick: i64, playing: bool, seq: Option<u64>, tps: f64, at: Instant) {
        let new = Sample {
            tick,
            playing,
            seq,
            ticks_per_second: if tps.is_finite() { tps.max(0.0) } else { 0.0 },
            at,
        };
        let continuing = self
            .sample
            .is_some_and(|old| old.playing && new.playing && old.seq == new.seq);
        let jumped_back =
            (tick as f64) < self.shown.get() - Self::HOLD.as_secs_f64() * new.ticks_per_second;
        self.sample = Some(new);
        if !continuing || jumped_back {
            self.shown.set(tick as f64);
        }
    }

    /// The tick to draw at `at`.
    pub fn tick_at(&self, at: Instant) -> i64 {
        let Some(s) = self.sample else { return 0 };
        if !s.playing {
            self.shown.set(s.tick as f64);
            return s.tick;
        }
        let elapsed = at.saturating_duration_since(s.at).min(Self::POLL);
        let t = s.tick as f64 + elapsed.as_secs_f64() * s.ticks_per_second;
        self.shown.set(self.shown.get().max(t));
        self.shown.get().round() as i64
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

/// The bar's rewind and fast forward buttons held to wind, and the held
/// keys (Swift: `TransportShuttle`).
///
/// - A click (a release with no wind yet) steps one bar; with shift it
///   goes to the previous or next marker. A release outside the button
///   with no wind yet moves nothing.
/// - Holding winds on the surfaces' shuttle curve: nothing below 0.4 s,
///   then faster the longer it is held. The caller calls `tick` every
///   [`Shuttle::TICK_EVERY`]. The hold's end lands an open automation
///   pass once (`transport_hold_end`).
/// - A held key steps one bar per key-down and auto-repeat
///   (`transport_key_step`, no landing); its key-up lands once.
/// - Everything waits while a text field has the keyboard, and a refusal
///   goes to the status line, never an alert. Time is passed in.
#[derive(Default)]
pub struct Shuttle {
    hold: Option<Hold>,
    /// A key step went through since the last key-up.
    key_stepped: bool,
}

struct Hold {
    forward: bool,
    /// Shift goes to a marker on release and never winds.
    shift: bool,
    start: Instant,
    /// Milliseconds since the press up to which it has wound.
    last_ms: u64,
    /// A wind went through: the release ends the hold, steps no bar.
    wound: bool,
}

impl Shuttle {
    /// How often a held button winds.
    pub const TICK_EVERY: Duration = Duration::from_millis(100);

    pub fn new() -> Self {
        Self::default()
    }

    /// A button is held (pressed and not yet released or cancelled).
    pub fn holding(&self) -> bool {
        self.hold.is_some()
    }

    /// Rewind (`forward` false) or fast forward pressed at `at`.
    pub fn press(&mut self, forward: bool, shift: bool, at: Instant, doc: &mut DocumentModel) {
        if self.hold.is_some() {
            self.cancel(doc);
        }
        if !doc.transport_keys_enabled() {
            doc.set_status(Some(TEXT_FIELD_HAS_KEYBOARD.into()));
            return;
        }
        self.hold = Some(Hold {
            forward,
            shift,
            start: at,
            last_ms: 0,
            wound: false,
        });
    }

    /// The button is still held at `at`: winds by the curve since the last
    /// tick, signed, when that is above 0.
    pub fn tick(&mut self, at: Instant, doc: &mut DocumentModel) {
        let Some(h) = self.hold.as_mut() else { return };
        if h.shift {
            return;
        }
        if !doc.transport_keys_enabled() {
            // A text field took the keyboard mid-hold: the hold stops
            // where it got to.
            self.cancel(doc);
            doc.set_status(Some(TEXT_FIELD_HAS_KEYBOARD.into()));
            return;
        }
        let now = ms_since(h.start, at);
        let quarters = shuttle_quarters(h.last_ms, now);
        h.last_ms = h.last_ms.max(now);
        if !(quarters > 0.0 && quarters.is_finite()) {
            return;
        }
        let signed = if h.forward { quarters } else { -quarters };
        let wound = h.wound;
        match doc.session().transport_wind(signed) {
            Ok(_) => h.wound = true,
            Err(e) => {
                // The refusal says why once and the hold stops; a pass the
                // earlier winds moved still lands (its own error is
                // ignored).
                self.hold = None;
                doc.set_status(Some(from_viz(&e).message));
                if wound {
                    land(doc, false);
                }
            }
        }
    }

    /// The button is released at `at`. With no wind yet it is a click
    /// (`inside`) or nothing (outside); after winding it winds the
    /// remainder and ends the hold, wherever the release is.
    pub fn release(&mut self, inside: bool, at: Instant, doc: &mut DocumentModel) {
        let Some(h) = self.hold.as_ref() else { return };
        if !h.wound {
            let (forward, shift) = (h.forward, h.shift);
            self.hold = None;
            if !inside {
                return;
            }
            let id = match (shift, forward) {
                (true, true) => "transport.next_marker",
                (true, false) => "transport.previous_marker",
                (false, true) => "transport.fast_forward",
                (false, false) => "transport.rewind",
            };
            if let Err(e) = commands::run(id, &[], doc) {
                doc.set_status(Some(e.message));
            }
            return;
        }
        self.tick(at, doc);
        // A refused remainder already ended the hold and landed.
        if self.hold.take().is_some() {
            land(doc, true);
        }
    }

    /// Ends the hold with no further wind; after winding, the open pass
    /// lands where the wind got to.
    pub fn cancel(&mut self, doc: &mut DocumentModel) {
        if let Some(h) = self.hold.take()
            && h.wound
        {
            land(doc, true);
        }
    }

    /// A key-down or auto-repeat of rewind or fast forward: one bar, moved
    /// as a wind moves (no landing), gated as the command is; a refusal
    /// shows in the status line and is returned.
    pub fn key_down(&mut self, forward: bool, doc: &mut DocumentModel) -> Result<(), ControlError> {
        let result = if !doc.transport_keys_enabled() {
            Err(ControlError::new(
                ErrorCode::Unavailable,
                TEXT_FIELD_HAS_KEYBOARD,
            ))
        } else if doc.selected_seq().is_none() {
            Err(ControlError::new(
                ErrorCode::Unavailable,
                "Select a sequence first.",
            ))
        } else {
            doc.session()
                .transport_key_step(forward)
                .map(|_| self.key_stepped = true)
                .map_err(|e| from_viz(&e))
        };
        if let Err(e) = &result {
            doc.set_status(Some(e.message.clone()));
        }
        result
    }

    /// The held key is up (or the window or app lost the keyboard): one
    /// landing if a key step happened since the last key-up.
    pub fn key_up(&mut self, doc: &mut DocumentModel) {
        if std::mem::take(&mut self.key_stepped) {
            land(doc, true);
        }
    }
}

/// Milliseconds from `start` to `at`; a time before `start` is 0.
fn ms_since(start: Instant, at: Instant) -> u64 {
    // Far beyond any hold; keeps the conversion in range.
    at.saturating_duration_since(start)
        .as_millis()
        .min(1_000_000_000_000_000) as u64
}

/// One landing (`transport_hold_end`); a refusal shows if `report`.
fn land(doc: &mut DocumentModel, report: bool) {
    if let Err(e) = doc.session().transport_hold_end()
        && report
    {
        doc.set_status(Some(from_viz(&e).message));
    }
}
