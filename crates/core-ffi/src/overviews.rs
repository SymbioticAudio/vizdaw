//! Waveform overviews in a session (sub-project 5 spec §10.1; plan 5b
//! Rulings 2–4, 17). One worker thread per session builds, verifies and
//! loads the overview of every online media item and reads exact sample
//! bins for views zoomed in below 256 frames per column. It opens files
//! with its own `CafReader`: it never touches the engine, the streamer or
//! its RAM cache, so nothing here is near the real-time path. Reads on the
//! FFI are memory lookups; what is not loaded yet answers `Pending` (or
//! `Coarse`) and the next `IdleUpdate.overviews_changed` says to ask again.

use crate::error::VizError;
use core_media::caf::CafReader;
use core_media::overview::{Bins, Overview, Stamp};
use core_model::event::Fade;
use core_model::ids::MediaId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

/// How a waveform read came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OverviewStateRow {
    /// The columns are exact (from the overview or from the samples).
    Ready,
    /// Zoomed in below 256 frames per column: these columns come from the
    /// 256-frame peaks while the samples are read; ask again after the
    /// next `IdleUpdate.overviews_changed`.
    Coarse,
    /// The overview is being built or loaded: the columns are silent; ask
    /// again after the next `IdleUpdate.overviews_changed`.
    Pending,
    /// The media is offline: nothing to draw.
    Offline,
}

/// Waveform columns: per channel, then per column (`channel * columns +
/// column`), the lowest and highest sample, linear (±1 at full scale,
/// more with gain).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct OverviewRows {
    /// How they came out.
    pub state: OverviewStateRow,
    /// 1 or 2.
    pub channels: u16,
    /// Columns per channel.
    pub columns: u32,
    /// Lowest sample per column.
    pub min: Vec<f32>,
    /// Highest sample per column.
    pub max: Vec<f32>,
}

impl OverviewRows {
    /// All-zero columns. `channels` comes from the document, which may
    /// claim any count: it is clamped to what a media file can hold, and
    /// `columns` to [`MAX_COLUMNS`] (review I2).
    pub fn silent(state: OverviewStateRow, channels: u16, columns: u32) -> OverviewRows {
        let channels = channels.clamp(1, core_media::caf::MAX_CHANNELS);
        let columns = columns.min(MAX_COLUMNS);
        let n = usize::from(channels) * columns as usize;
        OverviewRows {
            state,
            channels,
            columns,
            min: vec![0.0; n],
            max: vec![0.0; n],
        }
    }
}

/// The most columns one read returns.
pub const MAX_COLUMNS: u32 = 8192;
/// Below this many frames per column, columns come from the samples.
const SAMPLE_LEVEL: f64 = 256.0;
/// Exact sample windows kept.
const WINDOWS: usize = 32;

/// `Model` error unless `columns` is 1–[`MAX_COLUMNS`].
pub fn check_columns(columns: u32) -> Result<(), VizError> {
    if (1..=MAX_COLUMNS).contains(&columns) {
        Ok(())
    } else {
        Err(VizError::Model {
            message: format!("columns must be 1–{MAX_COLUMNS}, not {columns}"),
        })
    }
}

/// A media item the session wants an overview of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    /// The item.
    pub media: MediaId,
    /// Its package-relative path (as the document says).
    pub path: String,
    /// Its file.
    pub file: PathBuf,
    /// The content hash the document expects (`None`: unknown).
    pub hash: Option<String>,
    /// The package whose `Media/.overviews` caches it.
    pub package: PathBuf,
}

/// One verification the worker made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// The file no longer holds the audio expected.
    Mismatch {
        /// The item.
        media: MediaId,
        /// The path checked.
        path: String,
        /// The hash expected.
        hash: Option<String>,
    },
    /// The file was read (or its cache matched) and holds the audio
    /// expected.
    Verified {
        /// The item.
        media: MediaId,
        /// The path checked.
        path: String,
    },
}

/// What the worker found since the last [`Overviews::take_news`].
#[derive(Debug, Default)]
pub struct News {
    /// Some overview or sample window became available: redraw.
    pub changed: bool,
    /// Verifications in the order they were made, so a later finding for
    /// an item outranks an earlier one (review m1).
    pub found: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WindowKey {
    media: MediaId,
    /// The item's window generation when asked: a window of an older one
    /// is never served.
    generation: u64,
    file: PathBuf,
    from: u64,
    to: u64,
    step: u64,
}

#[derive(Debug)]
enum Task {
    Ensure(Want),
    Window(WindowKey),
}

#[derive(Default)]
struct State {
    queue: VecDeque<Task>,
    wanted: BTreeMap<MediaId, Want>,
    /// Items queued or done for their current want.
    handled: BTreeSet<MediaId>,
    by_hash: BTreeMap<String, Arc<Overview>>,
    /// Bumped whenever an item's key is dropped: its windows go with it.
    generations: BTreeMap<MediaId, u64>,
    /// Each item's overview hash (the expected one, or the one a build
    /// computed for an item without a hash).
    keys: BTreeMap<MediaId, String>,
    windows: VecDeque<(WindowKey, Arc<Bins>)>,
    asked: BTreeSet<WindowKey>,
    news: News,
    busy: bool,
    held: bool,
    stop: bool,
    /// The next task panics (test hook, preflight M10).
    #[cfg(feature = "test-hooks")]
    panic_next: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    cancel: AtomicBool,
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl State {
    /// Forgets `media`'s overview key and every sample window read or
    /// asked for it (review m2).
    fn drop_key(&mut self, media: MediaId) {
        self.keys.remove(&media);
        let g = self.generations.entry(media).or_default();
        *g = g.wrapping_add(1);
        self.windows.retain(|(k, _)| k.media != media);
        self.asked.retain(|k| k.media != media);
        self.queue
            .retain(|t| !matches!(t, Task::Window(k) if k.media == media));
    }
}

/// True when two wants name the same file and audio: the package (Save
/// As) does not count.
fn same_audio(a: &Want, b: &Want) -> bool {
    a.path == b.path && a.hash == b.hash
}

/// A session's overviews and their worker.
pub struct Overviews {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Default for Overviews {
    fn default() -> Overviews {
        Overviews {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
                cancel: AtomicBool::new(false),
            }),
            thread: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for Overviews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overviews").finish_non_exhaustive()
    }
}

impl Drop for Overviews {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Overviews {
    /// The items to keep overviews for. With `again` every item is checked
    /// again (a recheck); otherwise only items that are new or whose path
    /// or hash changed.
    pub fn ensure(&self, wants: Vec<Want>, again: bool) {
        let mut s = lock(&self.shared.state);
        let next: BTreeMap<MediaId, Want> = wants.into_iter().map(|w| (w.media, w)).collect();
        let prev = std::mem::take(&mut s.wanted);
        s.handled
            .retain(|m| !again && prev.contains_key(m) && prev.get(m) == next.get(m));
        let handled = s.handled.clone();
        // An item naming another file or hash answers `Pending` until its
        // new overview is loaded: never the previous file's (preflight I1).
        // An item with a content hash whose path and hash are unchanged
        // keeps its overview (content-addressed) while it is checked again
        // (a recheck, a Save As); one without a hash keeps it only while
        // its want is untouched.
        let gone: Vec<MediaId> = s
            .keys
            .keys()
            .copied()
            .filter(|m| {
                let keep = match (prev.get(m), next.get(m)) {
                    (Some(p), Some(n)) if n.hash.is_some() => same_audio(p, n),
                    (Some(_), Some(_)) => handled.contains(m),
                    _ => false,
                };
                !keep
            })
            .collect();
        for m in gone {
            s.drop_key(m);
        }
        s.queue.retain(|t| match t {
            Task::Ensure(w) => handled.contains(&w.media),
            Task::Window(_) => true,
        });
        for w in next.values() {
            if s.handled.insert(w.media) {
                s.queue.push_back(Task::Ensure(w.clone()));
            }
        }
        s.wanted = next;
        let work = !s.queue.is_empty();
        drop(s);
        if work {
            self.start();
            self.shared.wake.notify_all();
        }
    }

    /// The loaded overview of `media`, if any.
    pub fn get(&self, media: MediaId) -> Option<Arc<Overview>> {
        let s = lock(&self.shared.state);
        s.keys.get(&media).and_then(|h| s.by_hash.get(h)).cloned()
    }

    /// Exact sample bins of `file` over `[from, to)` in `step`-frame bins,
    /// if read; otherwise asks the worker (replacing an older request for
    /// the same item) and answers `None`.
    pub fn window(
        &self,
        media: MediaId,
        file: &Path,
        from: u64,
        to: u64,
        step: u64,
    ) -> Option<Arc<Bins>> {
        let mut s = lock(&self.shared.state);
        let generation = s.generations.get(&media).copied().unwrap_or(0);
        if let Some((_, b)) = s.windows.iter().find(|(k, _)| {
            k.media == media
                && k.generation == generation
                && k.file.as_path() == file
                && k.step == step
                && k.from <= from
                && to <= k.to
        }) {
            return Some(Arc::clone(b));
        }
        let key = WindowKey {
            media,
            generation,
            file: file.to_path_buf(),
            from,
            to,
            step,
        };
        if s.asked.contains(&key) {
            return None;
        }
        // Scrolling asks again and again: only the newest request per item
        // is worth reading.
        let stale: Vec<WindowKey> = s
            .queue
            .iter()
            .filter_map(|t| match t {
                Task::Window(k) if k.media == media => Some(k.clone()),
                _ => None,
            })
            .collect();
        s.queue
            .retain(|t| !matches!(t, Task::Window(k) if k.media == media));
        for k in stale {
            s.asked.remove(&k);
        }
        s.asked.insert(key.clone());
        s.queue.push_front(Task::Window(key));
        drop(s);
        self.start();
        self.shared.wake.notify_all();
        None
    }

    /// What the worker found since the last call.
    pub fn take_news(&self) -> News {
        std::mem::take(&mut lock(&self.shared.state).news)
    }

    /// True when the worker has nothing queued and is not working (test
    /// hook).
    pub fn idle(&self) -> bool {
        let s = lock(&self.shared.state);
        (s.held || s.queue.is_empty()) && !s.busy
    }

    /// Drops every loaded overview and sample window and asks for every
    /// wanted item again (test hook: what a fresh session holds before its
    /// worker ran).
    #[cfg(feature = "test-hooks")]
    pub fn forget_all(&self) {
        let mut s = lock(&self.shared.state);
        let loaded: Vec<MediaId> = s.keys.keys().copied().collect();
        for m in loaded {
            s.drop_key(m);
        }
        s.by_hash.clear();
        s.windows.clear();
        s.asked.clear();
        s.queue.clear();
        s.handled.clear();
        let wanted: Vec<Want> = s.wanted.values().cloned().collect();
        for w in wanted {
            s.handled.insert(w.media);
            s.queue.push_back(Task::Ensure(w));
        }
        drop(s);
        self.start();
        self.shared.wake.notify_all();
    }

    /// Holds the worker before its next request (`true`), or lets it go
    /// (test hook).
    pub fn set_held(&self, on: bool) {
        lock(&self.shared.state).held = on;
        self.shared.wake.notify_all();
    }

    /// Makes the worker's next task panic (test hook, preflight M10).
    #[cfg(feature = "test-hooks")]
    pub fn panic_next(&self) {
        lock(&self.shared.state).panic_next = true;
    }

    /// Stops the worker and waits for it (the session closes).
    pub fn shutdown(&self) {
        lock(&self.shared.state).stop = true;
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        let t = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(t) = t {
            let _ = t.join();
        }
    }

    fn start(&self) {
        let mut t = self.thread.lock().unwrap_or_else(PoisonError::into_inner);
        if t.is_some() || lock(&self.shared.state).stop {
            return;
        }
        let sh = Arc::clone(&self.shared);
        // A failed spawn leaves requests pending (drawn as Pending).
        *t = std::thread::Builder::new()
            .name("vizdaw-overviews".into())
            .spawn(move || run(&sh))
            .ok();
    }
}

enum Ensured {
    Loaded(Arc<Overview>),
    Mismatch,
    Failed,
    /// The file changed while it was read: nothing found is trusted.
    Again,
}

/// The worker's loop: windows first (they are at the front), then
/// ensures, until stopped. A task that panics is dropped (its item is
/// asked again by a recheck or a new want) and the worker goes on
/// (preflight M10).
fn run(sh: &Shared) {
    loop {
        let task = {
            let mut s = lock(&sh.state);
            loop {
                if s.stop {
                    return;
                }
                if !s.held {
                    if let Some(t) = s.queue.pop_front() {
                        s.busy = true;
                        break t;
                    }
                }
                s = sh.wake.wait(s).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let window = match &task {
            Task::Window(k) => Some(k.clone()),
            Task::Ensure(_) => None,
        };
        let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(sh, task)));
        if done.is_err() {
            let mut s = lock(&sh.state);
            s.busy = false;
            if let Some(k) = window {
                s.asked.remove(&k);
            }
        }
    }
}

/// Does one task and records what it found.
fn work(sh: &Shared, task: Task) {
    #[cfg(feature = "test-hooks")]
    if std::mem::take(&mut lock(&sh.state).panic_next) {
        #[allow(clippy::panic)]
        {
            panic!("injected overview task panic (test hook)");
        }
    }
    match task {
        Task::Ensure(w) => {
            let out = ensure_one(sh, &w);
            let mut s = lock(&sh.state);
            s.busy = false;
            let current = s.wanted.get(&w.media) == Some(&w);
            match out {
                Ensured::Loaded(ov) => {
                    let h = ov.hash.clone();
                    s.by_hash.insert(h.clone(), ov);
                    if current {
                        s.keys.insert(w.media, h);
                        s.news.found.push(Finding::Verified {
                            media: w.media,
                            path: w.path.clone(),
                        });
                        s.news.changed = true;
                    }
                }
                Ensured::Mismatch => {
                    if current {
                        s.drop_key(w.media);
                        s.news.found.push(Finding::Mismatch {
                            media: w.media,
                            path: w.path.clone(),
                            hash: w.hash.clone(),
                        });
                    }
                }
                Ensured::Again => {
                    if current {
                        s.queue.push_back(Task::Ensure(w));
                    }
                }
                Ensured::Failed => {}
            }
        }
        Task::Window(k) => {
            let r = CafReader::open(&k.file).and_then(|r| Bins::read(&r, k.from, k.to, k.step));
            let mut s = lock(&sh.state);
            s.busy = false;
            s.asked.remove(&k);
            let current = s.generations.get(&k.media).copied().unwrap_or(0) == k.generation;
            if let (Ok(b), true) = (r, current) {
                s.windows.push_back((k, Arc::new(b)));
                while s.windows.len() > WINDOWS {
                    s.windows.pop_front();
                }
                s.news.changed = true;
            }
        }
    }
}

/// Loads `w`'s overview when a cached one (in memory or on disk) has the
/// expected hash and a stamp of the file as it is now; otherwise builds it
/// (reading the whole file, which checks its hash) and caches it. A file
/// whose stamp moved during the build is read again later (`Again`); an
/// item without a content hash is kept in memory only (preflight M6).
fn ensure_one(sh: &Shared, w: &Want) -> Ensured {
    let stamp = Stamp::of(&w.file).ok();
    if let (Some(h), Some(st)) = (w.hash.as_deref(), stamp) {
        let held = lock(&sh.state).by_hash.get(h).cloned();
        if let Some(ov) = held.filter(|o| o.has_stamp(&st)) {
            return Ensured::Loaded(ov);
        }
        if let Some(p) = Overview::path(&w.package, h) {
            if let Ok(ov) = Overview::read(&p) {
                if ov.hash == h && ov.has_stamp(&st) {
                    return Ensured::Loaded(Arc::new(ov));
                }
            }
        }
    }
    let built = core_media::overview::build(&w.file, &sh.cancel);
    if sh.cancel.load(Ordering::Relaxed) {
        return Ensured::Failed;
    }
    // `build` reads the file as it is while it reads: a write meanwhile
    // makes the result (and a mismatch) meaningless.
    if Stamp::of(&w.file).ok() != stamp {
        return Ensured::Again;
    }
    let mut ov = match built {
        Ok(o) => o,
        Err(_) => return Ensured::Failed,
    };
    let Some(h) = &w.hash else {
        if let Some(st) = stamp {
            ov = ov.with_stamp(st);
        }
        return Ensured::Loaded(Arc::new(ov));
    };
    if ov.hash != *h {
        return Ensured::Mismatch;
    }
    if let Some(p) = Overview::path(&w.package, &ov.hash) {
        // Keep the stamps of other files with the same audio.
        if let Ok(old) = Overview::read(&p) {
            if old.hash == ov.hash {
                for s in old.stamps.iter().copied() {
                    ov = ov.with_stamp(s);
                }
            }
        }
        if let Some(st) = stamp {
            ov = ov.with_stamp(st);
        }
        // A want gone meanwhile (a Save As moved the package away, the
        // item was relinked) writes nothing: its package may be removed.
        if lock(&sh.state).wanted.get(&w.media) == Some(w) {
            let _ = ov.write(&p);
        }
    } else if let Some(st) = stamp {
        ov = ov.with_stamp(st);
    }
    Ensured::Loaded(Arc::new(ov))
}

/// Waveform columns of `media` (file `file`; `channels` while nothing is
/// loaded): column `c` covers media frames `spans[c]` (`None`: silence),
/// scaled by `gain(c)`. Below 256 frames per column the columns come from
/// exact sample bins, or (`Coarse`) the overview until those are read.
pub fn columns(
    store: &Overviews,
    media: MediaId,
    file: &Path,
    channels: u16,
    spans: &[Option<(u64, u64)>],
    gain: &dyn Fn(usize) -> f32,
) -> OverviewRows {
    let n = spans.len();
    let Some(ov) = store.get(media) else {
        return OverviewRows::silent(OverviewStateRow::Pending, channels, n as u32);
    };
    let (lo, hi) = spans
        .iter()
        .flatten()
        .fold((u64::MAX, 0u64), |(a, b), (x, y)| (a.min(*x), b.max(*y)));
    // Nothing past the file's end is read (review I1).
    let hi = hi.min(ov.frames);
    let mut state = OverviewStateRow::Ready;
    let mut bins = None;
    if lo < hi && n > 0 {
        let fpc = (hi - lo) as f64 / n as f64;
        if fpc < SAMPLE_LEVEL {
            let step = (fpc.floor() as u64).max(1);
            bins = store.window(media, file, lo, hi, step);
            if bins.is_none() {
                state = OverviewStateRow::Coarse;
            }
        }
    }
    let mut out = OverviewRows::silent(state, ov.channels, n as u32);
    for (c, span) in spans.iter().enumerate() {
        let Some((a, b)) = *span else { continue };
        let g = gain(c);
        for k in 0..ov.channels {
            let r = match &bins {
                Some(bn) => bn.range(k, a, b),
                None => ov.range(k, a, b),
            };
            if let Some((x, y)) = r {
                let i = usize::from(k) * n + c;
                if let Some(v) = out.min.get_mut(i) {
                    *v = (x * g).min(y * g);
                }
                if let Some(v) = out.max.get_mut(i) {
                    *v = (x * g).max(y * g);
                }
            }
        }
    }
    out
}

/// The fade gain at region frame `r` of a region of `length` frames (the
/// engine's `Segment::gain_at`, without the region gain).
pub fn fade_gain(r: u64, length: u64, fade_in: Fade, fade_out: Fade) -> f32 {
    use core_engine::schedule::fade_curve;
    let mut g = 1.0;
    let fi = fade_in.length_frames;
    if fi > 0 && r < fi {
        g *= fade_curve(fade_in.shape, r as f32 / fi as f32);
    }
    let fo = fade_out.length_frames;
    let left = length.saturating_sub(r);
    if fo > 0 && left <= fo {
        g *= fade_curve(fade_out.shape, left as f32 / fo as f32);
    }
    g
}

/// Copies the overview of every hash in `media` (path → hash, the
/// history's `referenced_media_anywhere`) from package `from` to `to`
/// when `to` lacks it (Save As, Ruling 17). Best effort: a missing one is
/// rebuilt later.
pub fn copy_cache(media: &BTreeMap<String, Option<String>>, from: &Path, to: &Path) {
    if from == to {
        return;
    }
    for h in media.values().flatten() {
        let (Some(src), Some(dst)) = (Overview::path(from, h), Overview::path(to, h)) else {
            continue;
        };
        if dst.exists() || !src.is_file() {
            continue;
        }
        let Some(dir) = dst.parent() else { continue };
        if std::fs::create_dir_all(dir).is_err() {
            continue;
        }
        // Through a temp name and a rename, as `Overview::write` does, so
        // a failed copy never leaves a partial overview under its name.
        let name = dst
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = dir.join(format!(".{name}.{}-copy.tmp", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let placed = std::fs::hard_link(&src, &tmp)
            .or_else(|_| std::fs::copy(&src, &tmp).map(|_| ()))
            .and_then(|()| std::fs::rename(&tmp, &dst));
        if placed.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}
