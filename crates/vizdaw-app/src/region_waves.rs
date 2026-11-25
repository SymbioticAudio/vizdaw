//! The waveforms of audio regions (Swift `MediaModel.waveform`,
//! `ArrangementView.drawRegions`; plan 8c owner ruling 2026-11-24).
//!
//! The core picks the overview level and applies the region's gain and
//! fades: a view asks `region_waveform(event, from, to, columns)` for the
//! region's visible ticks at one column per point (at most
//! [`MAX_COLUMNS`]). Swift asked on the main thread in `draw`; here
//! nothing reads on the UI thread or in render. [`RegionWaves::want`]
//! hands out a [`WaveRequest`] that the store runs on the background
//! executor, tagged with a generation; its [`WaveReply`] lands with
//! [`RegionWaves::land`], and a reply older than the rows held is dropped.
//! Render only reads what is held ([`RegionWaves::look`]).
//!
//! Rows are held per region, for its media, the zoom (ticks per point)
//! and the visible slice. They are dropped on a media change, a region
//! edit (its media, place, length, offset, gain or fades) and a zoom
//! change. `Pending` and `Coarse` rows (and failed reads) are asked again
//! once after the next `overviews_changed`, never before (no busy loop);
//! a new document snapshot asks every region again (a tempo edit moves
//! the audio under the ticks), keeping the old rows drawn meanwhile.

use std::collections::HashMap;
use std::sync::Arc;

use vizdaw_ffi::events::{EventKindRow, EventRow, FadeRow};
use vizdaw_ffi::selection::TickRange;
use vizdaw_ffi::{MediaRow, OverviewRows, OverviewStateRow, VizError};

use crate::session::DocSession;

/// The most columns one read asks for (the core's limit).
pub const MAX_COLUMNS: u32 = 8192;

/// What a region's waveform is doing, as its tree element's value says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveState {
    /// Nothing asked or held (no media, nothing visible).
    None,
    /// Asked and not answered yet, or the core is building the overview.
    Pending,
    /// Drawn from the 256-frame peaks while the samples are read.
    Coarse,
    /// Exact.
    Ready,
    /// The media is offline: hatched, never asked.
    Offline,
}

impl WaveState {
    /// The tree value: `none`, `pending`, `coarse`, `ready` or `offline`.
    pub fn as_str(self) -> &'static str {
        match self {
            WaveState::None => "none",
            WaveState::Pending => "pending",
            WaveState::Coarse => "coarse",
            WaveState::Ready => "ready",
            WaveState::Offline => "offline",
        }
    }

    fn of(s: OverviewStateRow) -> Self {
        match s {
            OverviewStateRow::Ready => WaveState::Ready,
            OverviewStateRow::Coarse => WaveState::Coarse,
            OverviewStateRow::Pending => WaveState::Pending,
            OverviewStateRow::Offline => WaveState::Offline,
        }
    }
}

/// The ticks and columns one read covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slice {
    pub from_tick: i64,
    pub to_tick: i64,
    pub columns: u32,
}

impl Slice {
    /// The read of `visible` ticks `width` points wide: one column per
    /// point rounded up, at most [`MAX_COLUMNS`]; `None` for an empty
    /// slice or width.
    pub fn of(visible: TickRange, width: f32) -> Option<Slice> {
        if visible.end <= visible.start || !width.is_finite() || width <= 0. {
            return None;
        }
        Some(Slice {
            from_tick: visible.start,
            to_tick: visible.end,
            columns: (width.ceil().min(MAX_COLUMNS as f32) as u32).max(1),
        })
    }
}

/// Rows held for a region, with the slice they cover.
#[derive(Debug, Clone, PartialEq)]
pub struct HeldWave {
    pub slice: Slice,
    pub rows: Arc<OverviewRows>,
}

/// What render draws of a region: its state and the rows held, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveLook {
    pub state: WaveState,
    pub held: Option<HeldWave>,
    /// The slices nothing needs asking for: the rows held, while they
    /// are current, and the read in flight.
    pub quiet: Vec<Slice>,
}

impl WaveLook {
    fn nothing(state: WaveState) -> Self {
        WaveLook {
            state,
            held: None,
            quiet: vec![],
        }
    }

    /// Whether a view showing `slice` should ask (offline: never). A
    /// pre-check for render; [`RegionWaves::want`] decides.
    pub fn needs(&self, slice: Slice) -> bool {
        self.state != WaveState::Offline && !self.quiet.contains(&slice)
    }
}

/// One read to run off the UI thread ([`WaveRequest::run`]).
pub struct WaveRequest {
    pub generation: u64,
    pub event: u64,
    pub media: u64,
    pub slice: Slice,
    session: Arc<dyn DocSession>,
}

impl std::fmt::Debug for WaveRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaveRequest")
            .field("generation", &self.generation)
            .field("event", &self.event)
            .field("media", &self.media)
            .field("slice", &self.slice)
            .finish()
    }
}

impl WaveRequest {
    /// Reads (a core call: never on the UI thread).
    pub fn run(self) -> WaveReply {
        let s = self.slice;
        let rows = self
            .session
            .region_waveform(self.event, s.from_tick, s.to_tick, s.columns);
        WaveReply {
            generation: self.generation,
            event: self.event,
            media: self.media,
            slice: s,
            rows,
        }
    }
}

/// A read's answer, for [`RegionWaves::land`].
#[derive(Debug, Clone)]
pub struct WaveReply {
    pub generation: u64,
    pub event: u64,
    pub media: u64,
    pub slice: Slice,
    pub rows: Result<OverviewRows, VizError>,
}

/// What of a region its rows depend on: a change drops them.
#[derive(Debug, Clone, PartialEq)]
struct Shape {
    media: u64,
    tick: i64,
    duration: i64,
    source_offset: u64,
    frames: u64,
    gain_db: f32,
    fade_in: FadeRow,
    fade_out: FadeRow,
    crossfade: bool,
}

impl Shape {
    fn of(r: &EventRow, media: u64) -> Self {
        Shape {
            media,
            tick: r.tick,
            duration: r.duration,
            source_offset: r.source_offset,
            frames: r.frames,
            gain_db: r.gain_db,
            fade_in: r.fade_in,
            fade_out: r.fade_out,
            crossfade: r.crossfade,
        }
    }
}

/// What a held read came to.
#[derive(Debug, Clone)]
enum Got {
    Rows(HeldWave),
    /// The read failed: nothing is drawn; asked again after news.
    Failed(Slice),
}

#[derive(Debug, Clone)]
struct Entry {
    shape: Shape,
    /// No reply asked before this generation is taken (the entry is
    /// newer than it).
    born: u64,
    held: Option<(u64, Got)>,
    /// The newest read in flight.
    asked: Option<(u64, Slice)>,
}

/// The regions' waveform cache (one per document).
pub struct RegionWaves {
    session: Arc<dyn DocSession>,
    /// The zoom the entries are for.
    ticks_per_point: Option<f64>,
    /// The next request's generation.
    next: u64,
    /// Rows asked before this generation are asked again (a snapshot).
    stale_before: u64,
    /// `Pending`, `Coarse` and failed rows asked before this generation
    /// are asked again (overview news).
    news_before: u64,
    entries: HashMap<u64, Entry>,
}

impl RegionWaves {
    pub fn new(session: Arc<dyn DocSession>) -> Self {
        RegionWaves {
            session,
            ticks_per_point: None,
            next: 1,
            stale_before: 0,
            news_before: 0,
            entries: HashMap::new(),
        }
    }

    /// The view's zoom: a change drops every region's rows.
    pub fn zoom(&mut self, ticks_per_point: f64) {
        if self.ticks_per_point != Some(ticks_per_point) {
            self.entries.clear();
            self.ticks_per_point = Some(ticks_per_point);
        }
    }

    /// The media rows changed: every region's rows go.
    pub fn media_changed(&mut self) {
        self.entries.clear();
    }

    /// `IdleUpdate.overviews_changed`: `Pending` and `Coarse` rows are
    /// asked again (once each).
    pub fn overviews_changed(&mut self) {
        self.news_before = self.next;
    }

    /// A new snapshot: every region is asked again; its rows stay drawn
    /// until the new ones land.
    pub fn document_changed(&mut self) {
        self.stale_before = self.next;
    }

    /// The read region `region` needs over `visible` ticks, `width`
    /// points wide (columns: the width rounded up, at most
    /// [`MAX_COLUMNS`]), at the zoom last given; `None` when nothing is
    /// to be asked: offline media (by `media`'s offline flag), no media,
    /// an empty slice or width, rows held for that slice, or a read of it
    /// in flight.
    pub fn want(
        &mut self,
        region: &EventRow,
        media: &[MediaRow],
        visible: TickRange,
        width: f32,
    ) -> Option<WaveRequest> {
        let id = region.media?;
        if region.kind != EventKindRow::AudioRegion || offline(id, media) {
            return None;
        }
        let slice = Slice::of(visible, width)?;
        let shape = Shape::of(region, id);
        let born = self.next;
        let e = self.entries.entry(region.id).or_insert_with(|| Entry {
            shape: shape.clone(),
            born,
            held: None,
            asked: None,
        });
        if e.shape != shape {
            *e = Entry {
                shape,
                born,
                held: None,
                asked: None,
            };
        }
        if quiet(e, self.stale_before, self.news_before).contains(&slice) {
            return None;
        }
        let generation = self.next;
        self.next += 1;
        e.asked = Some((generation, slice));
        Some(WaveRequest {
            generation,
            event: region.id,
            media: id,
            slice,
            session: self.session.clone(),
        })
    }

    /// A read's answer: kept unless the region changed since it was
    /// asked, or rows asked later are already held. True when it was
    /// kept (redraw).
    pub fn land(&mut self, reply: WaveReply) -> bool {
        let Some(e) = self.entries.get_mut(&reply.event) else {
            return false;
        };
        if e.asked.is_some_and(|(g, _)| g == reply.generation) {
            e.asked = None;
        }
        if reply.media != e.shape.media
            || reply.generation < e.born
            || e.held.as_ref().is_some_and(|(g, _)| *g >= reply.generation)
        {
            return false;
        }
        let got = match reply.rows {
            Ok(rows) => Got::Rows(HeldWave {
                slice: reply.slice,
                rows: Arc::new(rows),
            }),
            Err(_) => Got::Failed(reply.slice),
        };
        e.held = Some((reply.generation, got));
        true
    }

    /// What render draws of `region` at `ticks_per_point`: offline media
    /// is `Offline`; otherwise the rows held (for this region as it is,
    /// at this zoom) and their state, `Pending` while only asked, `None`
    /// before that.
    pub fn look(&self, region: &EventRow, media: &[MediaRow], ticks_per_point: f64) -> WaveLook {
        let Some(id) = region.media else {
            return WaveLook::nothing(WaveState::None);
        };
        if offline(id, media) {
            return WaveLook::nothing(WaveState::Offline);
        }
        if self.ticks_per_point != Some(ticks_per_point) {
            return WaveLook::nothing(WaveState::None);
        }
        let Some(e) = self
            .entries
            .get(&region.id)
            .filter(|e| e.shape == Shape::of(region, id))
        else {
            return WaveLook::nothing(WaveState::None);
        };
        let quiet = quiet(e, self.stale_before, self.news_before);
        let (state, held) = match &e.held {
            Some((_, Got::Rows(h))) => (WaveState::of(h.rows.state), Some(h.clone())),
            None if e.asked.is_some() => (WaveState::Pending, None),
            Some((_, Got::Failed(_))) | None => (WaveState::None, None),
        };
        WaveLook { state, held, quiet }
    }

    /// Regions with rows or reads (tests).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The slices of entry `e` nothing needs asking for: the newest read in
/// flight and the rows held, unless a snapshot since (`stale_before`) or,
/// for rows not `Ready`, overview news since (`news_before`) outdated
/// them.
fn quiet(e: &Entry, stale_before: u64, news_before: u64) -> Vec<Slice> {
    let mut out = vec![];
    if let Some((g, s)) = e.asked
        && g >= stale_before
        && g >= news_before
    {
        out.push(s);
    }
    if let Some((g, got)) = &e.held {
        let (slice, settled) = match got {
            Got::Rows(h) => (h.slice, h.rows.state == OverviewStateRow::Ready),
            Got::Failed(s) => (*s, false),
        };
        if *g >= stale_before && (settled || *g >= news_before) {
            out.push(slice);
        }
    }
    out
}

/// Whether media `id` is offline in `media`.
fn offline(id: u64, media: &[MediaRow]) -> bool {
    media.iter().any(|m| m.id == id && m.offline.is_some())
}

/// A region the view shows: its row, its visible ticks and how wide they
/// are in points.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionWant {
    pub region: EventRow,
    pub visible: TickRange,
    pub width: f32,
}

/// A document's waveforms: the cache, the media version it was filled
/// at and the reads waiting for the store to run them.
pub struct DocWaves {
    pub(crate) cache: RegionWaves,
    media_version: u64,
    pending: Vec<WaveRequest>,
}

impl DocWaves {
    pub(crate) fn new(session: Arc<dyn DocSession>) -> Self {
        DocWaves {
            cache: RegionWaves::new(session),
            media_version: 0,
            pending: vec![],
        }
    }
}

impl crate::DocumentModel {
    /// The regions' waveform cache.
    pub fn region_waves(&self) -> &RegionWaves {
        &self.waves.cache
    }

    /// What render draws of `region` at `ticks_per_point` (held rows
    /// only; no read).
    pub fn region_wave(&self, region: &EventRow, ticks_per_point: f64) -> WaveLook {
        self.region_wave_in(region, &self.media_rows(), ticks_per_point)
    }

    /// [`Self::region_wave`] with the media rows already read (one read
    /// per frame).
    pub fn region_wave_in(
        &self,
        region: &EventRow,
        media: &[MediaRow],
        ticks_per_point: f64,
    ) -> WaveLook {
        if self.waves.media_version != self.media_version {
            // The media moved: what is held is for the old rows.
            let offline = region.media.is_some_and(|id| offline(id, media));
            return WaveLook::nothing(if offline {
                WaveState::Offline
            } else {
                WaveState::None
            });
        }
        self.waves.cache.look(region, media, ticks_per_point)
    }

    /// The view shows `wants` at `ticks_per_point`: the reads they need
    /// wait for the store ([`crate::DocumentStore::want_region_waves`]
    /// runs them off the UI thread). How many reads were asked for.
    pub fn want_region_waves(&mut self, ticks_per_point: f64, wants: &[RegionWant]) -> usize {
        if self.waves.media_version != self.media_version {
            self.waves.cache.media_changed();
            self.waves.media_version = self.media_version;
        }
        self.waves.cache.zoom(ticks_per_point);
        let media = self.media_rows();
        let before = self.waves.pending.len();
        for w in wants {
            if let Some(r) = self.waves.cache.want(&w.region, &media, w.visible, w.width) {
                self.waves.pending.push(r);
            }
        }
        self.waves.pending.len() - before
    }

    /// The waveform reads waiting, taken (the store runs them).
    pub(crate) fn take_wave_reads(&mut self) -> Vec<WaveRequest> {
        std::mem::take(&mut self.waves.pending)
    }

    /// Waveform reads landed; true when any was kept (redraw).
    pub(crate) fn land_waves(&mut self, replies: Vec<WaveReply>) -> bool {
        let mut kept = false;
        for r in replies {
            kept |= self.waves.cache.land(r);
        }
        kept
    }
}
