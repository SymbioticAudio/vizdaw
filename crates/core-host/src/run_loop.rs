//! The main-thread run loop behind CLAP `timer-support` and
//! `posix-fd-support`. Plug-ins register timers and file descriptors
//! through their host (`clap::handlers`), each instance into its own
//! table; the app drives the loop from its main-thread event loop: it waits
//! until one of [`run_loop_fds`] is ready or a few milliseconds pass, then
//! calls [`run_loop_tick`], which runs the plug-ins' callbacks. The loop has
//! no thread of its own, so every callback runs on the main thread.
//!
//! The loop belongs to the thread that creates the instances (the main
//! thread; CLAP instances are not `Send`) and is held thread-locally, so it
//! needs no lock. Called from any other thread it sees nothing.
//!
//! A tick's work is bounded: each timer fires at most once (a period-0 or
//! late timer included; missed periods are dropped, not caught up), each
//! ready descriptor gets at most one callback per instance, and timers
//! registered during the tick wait for the next one.

pub use clack_extensions::posix_fd::FdFlags;
use std::cell::{Cell, RefCell};
use std::os::unix::io::RawFd;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// One registered timer.
#[derive(Debug)]
struct Timer {
    id: u32,
    period: Duration,
    /// When it fires next; `None` until the first tick after registration
    /// arms it (registration has no clock).
    due: Option<Instant>,
}

/// One instance's timers and descriptors (main thread).
#[derive(Debug, Default)]
pub(crate) struct Registrations {
    next_timer: u32,
    timers: Vec<Timer>,
    fds: Vec<(RawFd, FdFlags)>,
}

impl Registrations {
    /// A new timer's id, unique within the instance; `None` once the ids
    /// run out (`u32::MAX` is CLAP's invalid id).
    pub(crate) fn register_timer(&mut self, period_ms: u32) -> Option<u32> {
        let id = self.next_timer;
        if id == u32::MAX {
            return None;
        }
        self.next_timer += 1;
        self.timers.push(Timer {
            id,
            period: Duration::from_millis(u64::from(period_ms)),
            due: None,
        });
        Some(id)
    }

    /// Removes timer `id`; false if it is not registered.
    pub(crate) fn unregister_timer(&mut self, id: u32) -> bool {
        let before = self.timers.len();
        self.timers.retain(|t| t.id != id);
        self.timers.len() != before
    }

    /// Adds descriptor `fd`; false if it is invalid or already registered
    /// (a change of flags goes through [`Registrations::modify_fd`]).
    pub(crate) fn register_fd(&mut self, fd: RawFd, flags: FdFlags) -> bool {
        if fd < 0 || self.fd_flags(fd).is_some() {
            return false;
        }
        self.fds.push((fd, flags));
        true
    }

    /// Changes the flags of descriptor `fd`; false if it is not registered.
    pub(crate) fn modify_fd(&mut self, fd: RawFd, flags: FdFlags) -> bool {
        match self.fds.iter_mut().find(|(f, _)| *f == fd) {
            Some(entry) => {
                entry.1 = flags;
                true
            }
            None => false,
        }
    }

    /// Removes descriptor `fd`; false if it is not registered.
    pub(crate) fn unregister_fd(&mut self, fd: RawFd) -> bool {
        let before = self.fds.len();
        self.fds.retain(|(f, _)| *f != fd);
        self.fds.len() != before
    }

    fn fd_flags(&self, fd: RawFd) -> Option<FdFlags> {
        self.fds.iter().find(|(f, _)| *f == fd).map(|(_, fl)| *fl)
    }

    fn has_timer(&self, id: u32) -> bool {
        self.timers.iter().any(|t| t.id == id)
    }

    /// Arms new timers and moves the ones due at `now` into `out`, each
    /// rescheduled a period later (or a period after `now`, when late).
    fn take_due(&mut self, now: Instant, out: &mut Vec<u32>) {
        for t in &mut self.timers {
            let due = *t.due.get_or_insert(now + t.period);
            if due <= now {
                out.push(t.id);
                let next = due + t.period;
                t.due = Some(if next <= now { now + t.period } else { next });
            }
        }
    }
}

/// The plug-in side of one instance: runs its callbacks. A target whose
/// instance is gone, or busy (the tick re-entered it), does nothing.
pub(crate) trait Target {
    /// Calls the plug-in's `on_timer`.
    fn on_timer(&self, id: u32);
    /// Calls the plug-in's `on_fd`.
    fn on_fd(&self, fd: RawFd, flags: FdFlags);
}

struct Entry {
    key: u64,
    table: Rc<RefCell<Registrations>>,
    target: Rc<dyn Target>,
}

thread_local! {
    static LOOP: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static NEXT_KEY: Cell<u64> = const { Cell::new(0) };
    static TICKING: Cell<bool> = const { Cell::new(false) };
}

/// An instance's place in the run loop; dropping it removes the instance
/// and everything it registered.
#[derive(Debug)]
pub(crate) struct Member(u64);

/// Adds an instance's `table` and `target` to this thread's run loop.
pub(crate) fn join(table: Rc<RefCell<Registrations>>, target: Rc<dyn Target>) -> Member {
    let key = NEXT_KEY
        .try_with(|k| {
            let key = k.get();
            k.set(key.wrapping_add(1));
            key
        })
        .unwrap_or(u64::MAX);
    let _ = LOOP.try_with(|l| l.borrow_mut().push(Entry { key, table, target }));
    Member(key)
}

impl Drop for Member {
    fn drop(&mut self) {
        let removed = LOOP
            .try_with(|l| {
                let mut l = l.borrow_mut();
                let at = l.iter().position(|e| e.key == self.0)?;
                Some(l.remove(at))
            })
            .ok()
            .flatten();
        // Leftovers go with it, even while a tick still holds the table.
        if let Some(e) = removed {
            if let Ok(mut t) = e.table.try_borrow_mut() {
                *t = Registrations::default();
            }
        }
    }
}

type Snapshot = Vec<(Rc<RefCell<Registrations>>, Rc<dyn Target>)>;

fn entries() -> Snapshot {
    LOOP.try_with(|l| {
        l.borrow()
            .iter()
            .map(|e| (Rc::clone(&e.table), Rc::clone(&e.target)))
            .collect()
    })
    .unwrap_or_default()
}

/// Every descriptor the plug-ins of this thread registered, with the events
/// they wait for (merged when two instances share one), sorted by
/// descriptor: what the app polls before the next [`run_loop_tick`].
pub fn run_loop_fds() -> Vec<(RawFd, FdFlags)> {
    let mut out: Vec<(RawFd, FdFlags)> = Vec::new();
    for (table, _) in entries() {
        let Ok(t) = table.try_borrow() else {
            continue;
        };
        for (fd, flags) in &t.fds {
            match out.iter_mut().find(|(f, _)| f == fd) {
                Some(entry) => entry.1 |= *flags,
                None => out.push((*fd, *flags)),
            }
        }
    }
    out.sort_by_key(|(fd, _)| *fd);
    out
}

/// One pass of the run loop at `now` (main thread): fires the timers due,
/// then hands each instance the `ready` descriptors it registered, with
/// the ready events it asked for (errors always). Bounded work; see the
/// module docs. A nested call (from inside a callback) does nothing.
pub fn run_loop_tick(now: Instant, ready: &[(RawFd, FdFlags)]) {
    if TICKING.try_with(|t| t.replace(true)).unwrap_or(true) {
        return;
    }
    let _ticking = Ticking;
    let mut due = Vec::new();
    for (table, target) in entries() {
        due.clear();
        if let Ok(mut t) = table.try_borrow_mut() {
            t.take_due(now, &mut due);
        }
        for id in &due {
            // An earlier callback may have unregistered it.
            if table.try_borrow().is_ok_and(|t| t.has_timer(*id)) {
                target.on_timer(*id);
            }
        }
        for (fd, flags) in ready {
            let Some(want) = table.try_borrow().ok().and_then(|t| t.fd_flags(*fd)) else {
                continue;
            };
            let got = *flags & (want | FdFlags::ERROR);
            if !got.is_empty() {
                target.on_fd(*fd, got);
            }
        }
    }
}

/// Ends a tick, also when a callback unwinds.
struct Ticking;

impl Drop for Ticking {
    fn drop(&mut self) {
        let _ = TICKING.try_with(|t| t.set(false));
    }
}
