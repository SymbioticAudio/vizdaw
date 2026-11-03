//! Plugin hosts, one per session (spec §5, §11). A `PluginHost` is not
//! `Send` (CLAP and AppKit tie plugins to the thread that made them), so
//! each lives in a registry on the thread that created its session: the
//! app's main thread (tests: the test's thread). Sessions reach theirs
//! with [`with`]; a session used from another thread sees no plugin host
//! (Plan Ruling 1).

use crate::catalog;
use crate::error::VizError;
use crate::guard::lock;
use core_engine::backend::MAX_FRAMES;
use core_host::OfflineRender;
use core_host::PluginHost;
use core_host::catalog::HostMarks;
use core_host::shelf::ShelfFactory;
use core_model::document::Document;
use core_model::ids::PluginInstanceId;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a closed session's host waits for its processors to come back
/// from the engine before it is dropped anyway.
pub const CLOSING_WAIT: Duration = Duration::from_secs(5);

/// Rate and block size new hosts activate plugins for: the last engine's,
/// else 48 kHz and the engine's default block (Plan Ruling 3).
static CONFIG: Mutex<(f64, u32)> = Mutex::new((48_000.0, MAX_FRAMES));

struct Entry {
    host: PluginHost,
    marks: HostMarks,
}

/// A bounce's plugin copies (plan 5f-2 Ruling 6): a second host per
/// bounce, rendering offline, made and serviced on this thread.
struct BounceEntry {
    /// The session that bounces.
    session: u64,
    host: PluginHost,
    /// Set once nothing is loading.
    ready: Arc<AtomicBool>,
    /// The copies that do not render offline, filled when `ready` is set.
    problems: Arc<Mutex<Vec<String>>>,
    /// Every slot's plugin name, for the problems.
    names: BTreeMap<PluginInstanceId, String>,
}

#[derive(Default)]
struct Registry {
    hosts: BTreeMap<u64, Entry>,
    /// Hosts of closed sessions, waiting for their processors.
    closing: Vec<(PluginHost, Instant)>,
    /// Bounce copies by token.
    bounces: BTreeMap<u64, BounceEntry>,
    /// The last bounce token given.
    next_bounce: u64,
}

/// What a bounce gets of its plugin copies (plan 5f-2 Ruling 6).
pub struct BounceCopies {
    /// Ends them ([`end_bounce`]).
    pub token: u64,
    /// The copies' insert factory, for the bounce's engine.
    pub factory: ShelfFactory,
    /// Set by [`tick_bounces`] once nothing is loading.
    pub ready: Arc<AtomicBool>,
    /// The copies that do not render offline (filled before `ready`).
    pub problems: Arc<Mutex<Vec<String>>>,
}

thread_local! {
    /// Re-entrancy contract: a nested run loop (an AU editor waiting, a
    /// modal plugin UI) can call back into the FFI from inside [`with`]:
    /// the 60 Hz ticker, other calls, a `Session` drop. Every registry
    /// access therefore uses `try_borrow*` and answers "busy" instead of
    /// panicking. Closes that cannot run at once are queued in `PENDING`
    /// (its own cell, never borrowed across a callout) and run by the next
    /// successful registry access, engine first.
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
    static PENDING: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// Bounce copies whose end the busy registry put off.
    static PENDING_ENDS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    /// The catalog changed while the registry was busy.
    static CATALOG_DIRTY: Cell<bool> = const { Cell::new(false) };
}

fn busy() -> VizError {
    VizError::Busy {
        message: "plugin host is busy".into(),
    }
}

/// Whether the registry is borrowed (a plugin call is on the stack).
pub fn is_busy() -> bool {
    REGISTRY.with(|r| r.try_borrow_mut().is_err())
}

/// The configuration new hosts start with.
pub fn config() -> (f64, u32) {
    *lock(&CONFIG)
}

/// Remembers the engine's configuration for hosts made later.
pub fn set_config(sample_rate: f64, max_frames: u32) {
    *lock(&CONFIG) = (sample_rate, max_frames);
}

/// Makes session `id`'s plugin host on this thread (AU and CLAP), told
/// what the catalog knows.
pub fn create(id: u64) -> Result<(), VizError> {
    drain();
    let (rate, max) = config();
    let mut host = PluginHost::new(rate, max).map_err(|e| VizError::Engine {
        message: e.to_string(),
    })?;
    #[cfg(target_os = "macos")]
    host.add_format(Box::new(platform_macos::au::AuFormat::new()));
    let mut marks = HostMarks::default();
    catalog::apply(&mut host, &mut marks);
    REGISTRY.with(|r| {
        r.try_borrow_mut()
            .map_err(|_| busy())?
            .hosts
            .insert(id, Entry { host, marks });
        Ok(())
    })
}

/// Runs `f` with session `id`'s plugin host (`None` on another thread, or
/// after [`close`]); `Err(Busy)` when a plugin call is already running on
/// this thread. `f` must not call `with` again.
pub fn with<T>(id: u64, f: impl FnOnce(Option<&mut PluginHost>) -> T) -> Result<T, VizError> {
    drain();
    REGISTRY.with(|r| {
        let mut r = r.try_borrow_mut().map_err(|_| busy())?;
        Ok(f(r.hosts.get_mut(&id).map(|e| &mut e.host)))
    })
}

/// Tells every host on this thread what the catalog knows now (later, if
/// the registry is busy).
pub fn apply_catalog() {
    CATALOG_DIRTY.with(|d| d.set(true));
    drain();
}

/// Runs queued work: catalog updates, then closes of dropped sessions.
fn drain() {
    if is_busy() {
        return;
    }
    if CATALOG_DIRTY.with(|d| d.replace(false)) {
        REGISTRY.with(|r| {
            if let Ok(mut r) = r.try_borrow_mut() {
                for e in r.hosts.values_mut() {
                    catalog::apply(&mut e.host, &mut e.marks);
                }
            }
        });
    }
    let ends = PENDING_ENDS.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for token in ends {
        end_bounce(token);
    }
    let pending = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for id in pending {
        close_session(id);
    }
}

/// Whether session `id` has plugins loaded or processors still out.
pub fn has_plugins(id: u64) -> Result<bool, VizError> {
    with(id, |p| p.is_some_and(|p| p.live_count() + p.retiring() > 0))
}

/// Ends session `id`'s hosting (a dropped session). The engine goes first
/// when this session's host feeds it, since it gives the processors back;
/// then the plugin host closes. When the registry is busy the close is
/// queued and runs on the next registry access.
pub fn close_session(id: u64) {
    // Its bounce's plugin copies first (plan 5f-2, pre-flight I2).
    end_bounces_of(id);
    match has_plugins(id) {
        Ok(has) => {
            if has && crate::host::factory_owner() == Some(id) {
                crate::host::reset();
            } else {
                crate::host::release(id);
            }
            close(id);
        }
        Err(_) => PENDING.with(|p| p.borrow_mut().push(id)),
    }
}

/// Closes session `id`'s host: every slot is dropped (shelved processors
/// are deactivated now); a host whose processors are still in the engine
/// waits in the closing list. Does nothing while the registry is busy.
pub fn close(id: u64) {
    REGISTRY.with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else {
            PENDING.with(|p| p.borrow_mut().push(id));
            return;
        };
        if let Some(mut e) = r.hosts.remove(&id) {
            e.host.sync(&Document::new());
            e.host.idle();
            if e.host.retiring() > 0 {
                r.closing.push((e.host, Instant::now()));
            }
        }
    });
    tick_closing();
}

/// Services closed hosts; drops those whose processors all came back (or
/// that waited [`CLOSING_WAIT`]). Skipped while the registry is busy.
pub fn tick_closing() {
    REGISTRY.with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else {
            return;
        };
        for (h, _) in &mut r.closing {
            h.idle();
        }
        r.closing
            .retain(|(h, t)| h.retiring() > 0 && t.elapsed() < CLOSING_WAIT);
    });
}

/// Makes a bounce's plugin copies for session `session` (plan 5f-2 Ruling
/// 6): a new host at `rate` and `max` frames, told what the catalog knows,
/// rendering offline, synced to `doc` (the live states as Save captures
/// them). `Model` when this thread has no host for the session (a session
/// used off its plugin thread), `Busy` while a plugin call is running.
pub fn start_bounce(
    session: u64,
    doc: &Document,
    rate: f64,
    max: u32,
) -> Result<BounceCopies, VizError> {
    drain();
    let known = REGISTRY.with(|r| {
        r.try_borrow()
            .map(|r| r.hosts.contains_key(&session))
            .map_err(|_| busy())
    })?;
    if !known {
        return Err(VizError::Model {
            message: "bounce plugins from the window's thread".into(),
        });
    }
    let mut host = PluginHost::new(rate, max).map_err(|e| VizError::Engine {
        message: e.to_string(),
    })?;
    #[cfg(target_os = "macos")]
    host.add_format(Box::new(platform_macos::au::AuFormat::new()));
    let mut marks = HostMarks::default();
    catalog::apply(&mut host, &mut marks);
    host.set_offline_render(true);
    host.sync(doc);
    let mut names = BTreeMap::new();
    for d in doc.instruments().values() {
        if let core_model::document::DestKind::PluginInstrument { slot } = &d.kind {
            names.insert(slot.id, slot.plugin.name.clone());
        }
    }
    for (_, _, slot) in doc.mixer().inserts() {
        names.insert(slot.id, slot.plugin.name.clone());
    }
    let ready = Arc::new(AtomicBool::new(false));
    let problems = Arc::new(Mutex::new(Vec::new()));
    let factory = host.factory();
    REGISTRY.with(|r| {
        let mut r = r.try_borrow_mut().map_err(|_| busy())?;
        r.next_bounce += 1;
        let token = r.next_bounce;
        r.bounces.insert(
            token,
            BounceEntry {
                session,
                host,
                ready: Arc::clone(&ready),
                problems: Arc::clone(&problems),
                names,
            },
        );
        Ok(BounceCopies {
            token,
            factory,
            ready,
            problems,
        })
    })
}

/// Services the bounce copies (`Session::idle`): their loads finish here;
/// once nothing is loading, the copies that do not render offline go to
/// the bounce's problems and `ready` is set (Rulings 6, T3-1, T3-2).
/// Skipped while the registry is busy.
pub fn tick_bounces() {
    REGISTRY.with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else {
            return;
        };
        for b in r.bounces.values_mut() {
            b.host.idle();
            if b.ready.load(Ordering::SeqCst) || b.host.pending() > 0 {
                continue;
            }
            let mut lines = Vec::new();
            for (id, name) in &b.names {
                match b.host.offline_state(*id) {
                    Some(OfflineRender::RealtimeRequired) => {
                        lines.push(format!("{name} renders in real time only"));
                    }
                    Some(OfflineRender::Refused) => {
                        lines.push(format!("{name} has an offline mode and is not in it"));
                    }
                    Some(OfflineRender::Taken | OfflineRender::NoOfflineMode) | None => {}
                }
            }
            lock(&b.problems).extend(lines);
            b.ready.store(true, Ordering::SeqCst);
        }
    });
}

/// Ends bounce `token`'s copies: every slot is dropped, and a host whose
/// processors are still out waits in the closing list. Queued while the
/// registry is busy.
pub fn end_bounce(token: u64) {
    REGISTRY.with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else {
            PENDING_ENDS.with(|p| p.borrow_mut().push(token));
            return;
        };
        if let Some(mut b) = r.bounces.remove(&token) {
            b.host.sync(&Document::new());
            b.host.idle();
            if b.host.retiring() > 0 {
                r.closing.push((b.host, Instant::now()));
            }
        }
    });
}

/// Ends every bounce of session `session` (it was dropped). While the
/// registry is busy nothing happens here: `close_session` then queues the
/// whole close, which calls this again.
fn end_bounces_of(session: u64) {
    let tokens: Vec<u64> = REGISTRY.with(|r| {
        r.try_borrow()
            .map(|r| {
                r.bounces
                    .iter()
                    .filter(|(_, b)| b.session == session)
                    .map(|(t, _)| *t)
                    .collect()
            })
            .unwrap_or_default()
    });
    for t in tokens {
        end_bounce(t);
    }
}

/// Bounce copy hosts not yet ended (test hook; pre-flight M4).
#[cfg(feature = "test-hooks")]
pub fn bounce_hosts() -> usize {
    REGISTRY.with(|r| r.borrow().bounces.len())
}

/// Closed hosts still waiting (test hook).
#[cfg(feature = "test-hooks")]
pub fn closing_hosts() -> usize {
    REGISTRY.with(|r| r.borrow().closing.len())
}

/// Closes queued by a busy registry, not yet run (test hook).
#[cfg(feature = "test-hooks")]
pub fn pending_closes() -> usize {
    PENDING.with(|p| p.borrow().len())
}

/// Session hosts on this thread (test hook).
#[cfg(feature = "test-hooks")]
pub fn host_count() -> usize {
    REGISTRY.with(|r| r.borrow().hosts.len())
}

#[cfg(feature = "test-hooks")]
thread_local! {
    /// Fake presets by slot id (test hook; the test CLAP plug-ins have none).
    static FAKE_PRESETS: RefCell<BTreeMap<u64, core_host::Presets>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// `slot`'s presets in session `id`'s host; `None` without a running
/// instance (plan 5f-4 Ruling 7).
pub fn presets(id: u64, slot: PluginInstanceId) -> Option<core_host::Presets> {
    #[cfg(feature = "test-hooks")]
    if let Some(p) = FAKE_PRESETS.with(|f| f.borrow().get(&slot.0).cloned()) {
        return Some(p);
    }
    with(id, |h| {
        h.and_then(|h| h.instance(slot)).map(|i| i.presets())
    })
    .ok()
    .flatten()
}

/// Loads preset `index` of `slot`.
pub fn load_preset(id: u64, slot: PluginInstanceId, index: usize) -> Result<(), VizError> {
    #[cfg(feature = "test-hooks")]
    {
        let fake = FAKE_PRESETS.with(|f| {
            f.borrow_mut().get_mut(&slot.0).map(|p| {
                if index < p.names.len() {
                    p.current = Some(index);
                    true
                } else {
                    false
                }
            })
        });
        match fake {
            Some(true) => return Ok(()),
            Some(false) => {
                return Err(VizError::Model {
                    message: format!("no preset {index}"),
                });
            }
            None => {}
        }
    }
    with(id, |h| {
        h.and_then(|h| h.instance(slot))
            .map(|i| i.load_preset(index))
    })?
    .ok_or_else(|| VizError::Model {
        message: "the plug-in is not running".into(),
    })?
    .map_err(|e| VizError::Model {
        message: e.to_string(),
    })
}

/// Makes `slot` answer `names` and `current` as its presets (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_set_presets(slot: u64, names: Vec<String>, current: Option<usize>) {
    FAKE_PRESETS.with(|f| {
        f.borrow_mut()
            .insert(slot, core_host::Presets { names, current });
    });
}

/// The preset last loaded into `slot`'s fake list (test hook).
#[cfg(feature = "test-hooks")]
pub fn debug_current_preset(slot: u64) -> Option<usize> {
    FAKE_PRESETS.with(|f| f.borrow().get(&slot).and_then(|p| p.current))
}

/// The row of `slot` (an instrument's own slot when `instrument`), with
/// what `host` knows about its instance.
pub fn slot_row(
    slot: &core_model::mixer::PluginSlot,
    instrument: bool,
    host: Option<&mut PluginHost>,
) -> crate::snapshot::SlotRow {
    use crate::snapshot::{SlotRow, SlotStatus};
    use core_host::SlotState;
    let mut row = SlotRow {
        id: slot.id.0,
        plugin: (&slot.plugin).into(),
        instrument,
        bypass: slot.bypass,
        sidechain: slot.sidechain.map(|s| s.0),
        status: SlotStatus::Placeholder {
            reason: "no plugin host on this thread".into(),
        },
        latency: 0,
        has_sidechain_input: false,
        outputs: 0,
        has_editor: false,
    };
    let Some(host) = host else {
        return row;
    };
    row.status = match host.state(slot.id) {
        None | Some(SlotState::Loading) => SlotStatus::Loading,
        Some(SlotState::Live) => SlotStatus::Live,
        Some(SlotState::Failed(reason)) => SlotStatus::Placeholder { reason },
    };
    if let Some(inst) = host.instance(slot.id) {
        let info = inst.info();
        row.latency = info.latency;
        row.has_sidechain_input = info.sidechain_input().is_some();
        row.outputs = u32::try_from(info.outputs.len()).unwrap_or(u32::MAX);
        row.has_editor = inst.editor().is_some();
    }
    row
}

/// Every plugin slot of `doc`: instruments' own slots (by destination),
/// then inserts (by strip, in chain order).
pub fn slot_rows(
    doc: &Document,
    mut host: Option<&mut PluginHost>,
) -> Vec<crate::snapshot::SlotRow> {
    use core_model::document::DestKind;
    let mut rows = Vec::new();
    for d in doc.instruments().values() {
        if let DestKind::PluginInstrument { slot } = &d.kind {
            rows.push(slot_row(slot, true, host.as_deref_mut()));
        }
    }
    for (_, _, slot) in doc.mixer().inserts() {
        rows.push(slot_row(slot, false, host.as_deref_mut()));
    }
    rows
}
