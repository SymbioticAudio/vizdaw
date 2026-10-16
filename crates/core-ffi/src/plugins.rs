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
use core_host::PluginHost;
use core_host::catalog::HostMarks;
use core_model::document::Document;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::Mutex;
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

#[derive(Default)]
struct Registry {
    hosts: BTreeMap<u64, Entry>,
    /// Hosts of closed sessions, waiting for their processors.
    closing: Vec<(PluginHost, Instant)>,
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
