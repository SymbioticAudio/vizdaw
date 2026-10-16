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
use std::cell::RefCell;
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
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
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
    let (rate, max) = config();
    let mut host = PluginHost::new(rate, max).map_err(|e| VizError::Engine {
        message: e.to_string(),
    })?;
    #[cfg(target_os = "macos")]
    host.add_format(Box::new(platform_macos::au::AuFormat::new()));
    let mut marks = HostMarks::default();
    catalog::apply(&mut host, &mut marks);
    REGISTRY.with(|r| r.borrow_mut().hosts.insert(id, Entry { host, marks }));
    Ok(())
}

/// Runs `f` with session `id`'s plugin host (`None` on another thread, or
/// after [`close`]). `f` must not call `with` again.
pub fn with<T>(id: u64, f: impl FnOnce(Option<&mut PluginHost>) -> T) -> T {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        f(r.hosts.get_mut(&id).map(|e| &mut e.host))
    })
}

/// Tells every host on this thread what the catalog knows now.
pub fn apply_catalog() {
    REGISTRY.with(|r| {
        for e in r.borrow_mut().hosts.values_mut() {
            catalog::apply(&mut e.host, &mut e.marks);
        }
    });
}

/// Whether session `id` has plugins loaded or processors still out.
pub fn has_plugins(id: u64) -> bool {
    with(id, |p| p.is_some_and(|p| p.live_count() + p.retiring() > 0))
}

/// Closes session `id`'s host: every slot is dropped (shelved processors
/// are deactivated now); a host whose processors are still in the engine
/// waits in the closing list.
pub fn close(id: u64) {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
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
/// that waited [`CLOSING_WAIT`]).
pub fn tick_closing() {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
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
