//! The main-thread run loop behind CLAP `timer-support` and
//! `posix-fd-support` and the VST3 `Linux::IRunLoop`. Plug-ins register
//! timers and file descriptors through their host (`clap::handlers`; a
//! VST3 view through its frame, `vst3::editor`), each instance (each open
//! VST3 view) into its own table; the app drives the loop from its
//! main-thread event loop: it waits until one of [`run_loop_fds`] is ready
//! or a few milliseconds pass, then calls [`run_loop_tick`], which runs the
//! plug-ins' callbacks. The loop has no thread of its own, so every
//! callback runs on the main thread.
//!
//! The loop belongs to the thread that creates the instances (the main
//! thread; CLAP and VST3 instances are not `Send`) and is held
//! thread-locally, so it needs no lock. Called from any other thread it
//! sees nothing.
//!
//! A tick's work is bounded: each timer fires at most once (a period-0 or
//! late timer included; missed periods are dropped, not caught up), each
//! ready descriptor gets at most one callback per instance, and timers
//! registered during the tick wait for the next one.
//!
//! The CLAP host offers timers everywhere and descriptors on every unix
//! (`clap::handlers`); `Linux::IRunLoop` is a Linux interface. Descriptors
//! exist only on unix; elsewhere the loop has timers alone and
//! [`run_loop_tick`] takes no ready list. An instance holds at most
//! [`MAX_TIMERS`] timers. A descriptor that [`run_loop_pump`] finds closed
//! (`POLLNVAL`) gets one `ERROR` callback and is then left out of
//! [`run_loop_fds`] and of later ticks until the plug-in modifies or
//! registers it again (a plug-in that closes it before unregistering would
//! otherwise get an error on every pump). [`run_loop_calls`] counts the
//! callbacks made, for tests and diagnostics.
//!
//! Who pumps: every binary that loads plug-ins on a thread it keeps
//! running. The GPUI app runs one app-wide main-thread loop every 10 ms
//! (`vizdaw-gpui`, outside the 50 ms document idle: plug-in timers are
//! often 16-30 ms); `vizdaw-play` pumps in its live loop (100 ms
//! resolution, documented there). `vizctl`, `vizdaw-scan` and `vizdaw-mcp`
//! need none: they load no plug-ins in process, or only for a one-block
//! scan that needs no timers.

#[cfg(unix)]
pub use clack_extensions::posix_fd::FdFlags;
use std::cell::{Cell, RefCell};
#[cfg(unix)]
use std::os::unix::io::RawFd;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The most timers one instance (one VST3 run-loop host) may hold at once;
/// a further registration is refused. A tick visits every timer, so a
/// plug-in that leaks them would slow every tick.
pub const MAX_TIMERS: usize = 256;

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
    #[cfg(unix)]
    fds: Vec<Fd>,
}

/// One registered descriptor.
#[cfg(unix)]
#[derive(Debug)]
struct Fd {
    fd: RawFd,
    flags: FdFlags,
    /// Found closed (`POLLNVAL`) and told once: left out until modified or
    /// registered again.
    invalid: bool,
}

impl Registrations {
    /// A new timer's id, unique within the instance; `None` once the ids
    /// run out (`u32::MAX` is CLAP's invalid id) or the instance already
    /// holds [`MAX_TIMERS`].
    pub(crate) fn register_timer(&mut self, period_ms: u32) -> Option<u32> {
        let id = self.next_timer;
        if id == u32::MAX || self.timers.len() >= MAX_TIMERS {
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

    #[cfg(unix)]
    /// Adds descriptor `fd`; false if it is invalid or already registered
    /// (a change of flags goes through [`Registrations::modify_fd`]). One
    /// found closed (`POLLNVAL`) may be registered again: it takes the new
    /// flags and is watched again.
    pub(crate) fn register_fd(&mut self, fd: RawFd, flags: FdFlags) -> bool {
        if fd < 0 {
            return false;
        }
        match self.fds.iter_mut().find(|e| e.fd == fd) {
            Some(e) if e.invalid => {
                e.flags = flags;
                e.invalid = false;
                true
            }
            Some(_) => false,
            None => {
                self.fds.push(Fd {
                    fd,
                    flags,
                    invalid: false,
                });
                true
            }
        }
    }

    #[cfg(unix)]
    /// Changes the flags of descriptor `fd` (and watches it again if it
    /// was found closed); false if it is not registered.
    pub(crate) fn modify_fd(&mut self, fd: RawFd, flags: FdFlags) -> bool {
        match self.fds.iter_mut().find(|e| e.fd == fd) {
            Some(e) => {
                e.flags = flags;
                e.invalid = false;
                true
            }
            None => false,
        }
    }

    #[cfg(unix)]
    /// Watches descriptor `fd` again if it was found closed (a VST3 handler
    /// registered on a descriptor another handler already watches).
    pub(crate) fn rewatch_fd(&mut self, fd: RawFd) {
        if let Some(e) = self.fds.iter_mut().find(|e| e.fd == fd) {
            e.invalid = false;
        }
    }

    #[cfg(unix)]
    /// Removes descriptor `fd`; false if it is not registered.
    pub(crate) fn unregister_fd(&mut self, fd: RawFd) -> bool {
        let before = self.fds.len();
        self.fds.retain(|e| e.fd != fd);
        self.fds.len() != before
    }

    #[cfg(unix)]
    /// The flags `fd` waits for; `None` when it is not registered or was
    /// found closed.
    fn fd_flags(&self, fd: RawFd) -> Option<FdFlags> {
        self.fds
            .iter()
            .find(|e| e.fd == fd && !e.invalid)
            .map(|e| e.flags)
    }

    #[cfg(unix)]
    fn mark_invalid(&mut self, fd: RawFd) {
        if let Some(e) = self.fds.iter_mut().find(|e| e.fd == fd) {
            e.invalid = true;
        }
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
    #[cfg(unix)]
    fn on_fd(&self, fd: RawFd, flags: FdFlags);
}

struct Entry {
    key: u64,
    table: Rc<RefCell<Registrations>>,
    target: Rc<dyn Target>,
}

thread_local! {
    static CALLS: Cell<RunLoopCalls> = const { Cell::new(RunLoopCalls { timers: 0, fds: 0 }) };
    static LOOP: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    static NEXT_KEY: Cell<u64> = const { Cell::new(0) };
    static TICKING: Cell<bool> = const { Cell::new(false) };
}

/// How many plug-in callbacks this thread's run loop has made.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunLoopCalls {
    /// Timer callbacks.
    pub timers: u64,
    /// Descriptor callbacks.
    pub fds: u64,
}

/// The callbacks this thread's run loop has made so far.
pub fn run_loop_calls() -> RunLoopCalls {
    CALLS.try_with(Cell::get).unwrap_or_default()
}

fn count(f: impl FnOnce(&mut RunLoopCalls)) {
    let _ = CALLS.try_with(|c| {
        let mut n = c.get();
        f(&mut n);
        c.set(n);
    });
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

#[cfg(unix)]
/// Every descriptor the plug-ins of this thread registered, with the events
/// they wait for (merged when two instances share one), sorted by
/// descriptor: what the app polls before the next [`run_loop_tick`].
pub fn run_loop_fds() -> Vec<(RawFd, FdFlags)> {
    let mut out: Vec<(RawFd, FdFlags)> = Vec::new();
    for (table, _) in entries() {
        let Ok(t) = table.try_borrow() else {
            continue;
        };
        for e in t.fds.iter().filter(|e| !e.invalid) {
            match out.iter_mut().find(|(f, _)| *f == e.fd) {
                Some(entry) => entry.1 |= e.flags,
                None => out.push((e.fd, e.flags)),
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
#[cfg(unix)]
pub fn run_loop_tick(now: Instant, ready: &[(RawFd, FdFlags)]) {
    tick_fds(now, ready, &[]);
}

/// [`run_loop_tick`], where the descriptors in `invalid` (all in `ready`)
/// were found closed: after this callback they are left out.
#[cfg(unix)]
fn tick_fds(now: Instant, ready: &[(RawFd, FdFlags)], invalid: &[RawFd]) {
    tick(now, |table, target| {
        for (fd, flags) in ready {
            let Some(want) = table.try_borrow().ok().and_then(|t| t.fd_flags(*fd)) else {
                continue;
            };
            let got = *flags & (want | FdFlags::ERROR);
            if got.is_empty() {
                continue;
            }
            if invalid.contains(fd) {
                // Before the callback: it may register the number again.
                if let Ok(mut t) = table.try_borrow_mut() {
                    t.mark_invalid(*fd);
                }
            }
            count(|c| c.fds += 1);
            target.on_fd(*fd, got);
        }
    });
}

/// One pass of the run loop without waiting (main thread): polls the
/// registered descriptors ([`run_loop_fds`]) with a zero timeout, then
/// ticks at the current time with the ready ones (due timers fire). A
/// descriptor the poll finds closed (`POLLNVAL`) gets one `ERROR` callback
/// and is left out until modified or registered again. An app that can
/// wait on descriptors polls [`run_loop_fds`] itself and calls
/// [`run_loop_tick`] instead.
#[cfg(unix)]
pub fn run_loop_pump() {
    let fds = run_loop_fds();
    let mut polled: Vec<libc::pollfd> = fds
        .iter()
        .map(|(fd, flags)| libc::pollfd {
            fd: *fd,
            events: poll_events(*flags),
            revents: 0,
        })
        .collect();
    let mut ready = Vec::new();
    let mut invalid = Vec::new();
    if !polled.is_empty() {
        let n = libc::nfds_t::try_from(polled.len()).unwrap_or(libc::nfds_t::MAX);
        // SAFETY: `polled` is a live array of at least `n` records, and a
        // zero timeout never blocks.
        let r = unsafe { libc::poll(polled.as_mut_ptr(), n, 0) };
        if r > 0 {
            for p in &polled {
                let f = ready_flags(p.revents);
                if !f.is_empty() {
                    ready.push((p.fd, f));
                }
                if p.revents & libc::POLLNVAL != 0 {
                    invalid.push(p.fd);
                }
            }
        }
    }
    tick_fds(Instant::now(), &ready, &invalid);
}

/// One pass of the run loop (main thread): due timers fire.
#[cfg(not(unix))]
pub fn run_loop_pump() {
    run_loop_tick(Instant::now());
}

#[cfg(unix)]
fn poll_events(flags: FdFlags) -> std::ffi::c_short {
    let mut e = 0;
    if flags.contains(FdFlags::READ) {
        e |= libc::POLLIN;
    }
    if flags.contains(FdFlags::WRITE) {
        e |= libc::POLLOUT;
    }
    e
}

#[cfg(unix)]
fn ready_flags(revents: std::ffi::c_short) -> FdFlags {
    let mut f = FdFlags::empty();
    if revents & libc::POLLIN != 0 {
        f |= FdFlags::READ;
    }
    if revents & libc::POLLOUT != 0 {
        f |= FdFlags::WRITE;
    }
    if revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
        f |= FdFlags::ERROR;
    }
    f
}

/// One pass of the run loop at `now` (main thread): fires the timers due.
/// Without unix descriptors there is no ready list.
#[cfg(not(unix))]
pub fn run_loop_tick(now: Instant) {
    tick(now, |_, _| {});
}

/// Fires each instance's due timers, then runs `fds` for it.
fn tick(now: Instant, mut fds: impl FnMut(&RefCell<Registrations>, &dyn Target)) {
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
                count(|c| c.timers += 1);
                target.on_timer(*id);
            }
        }
        fds(&table, target.as_ref());
    }
}

/// Ends a tick, also when a callback unwinds.
struct Ticking;

impl Drop for Ticking {
    fn drop(&mut self) {
        let _ = TICKING.try_with(|t| t.set(false));
    }
}

#[cfg(test)]
mod tests {
    //! The loop with a fake plug-in side, so it is covered on every
    //! platform (the plug-in-driven tests are in `tests/run_loop.rs`).
    use super::*;

    /// Records the callbacks it gets; optionally unregisters a timer from
    /// inside its callback.
    #[derive(Default)]
    struct Fake {
        table: RefCell<Option<Rc<RefCell<Registrations>>>>,
        timers: RefCell<Vec<u32>>,
        unregister_on_fire: Cell<Option<u32>>,
        #[cfg(unix)]
        fds: RefCell<Vec<(RawFd, FdFlags)>>,
    }

    impl Target for Fake {
        fn on_timer(&self, id: u32) {
            self.timers.borrow_mut().push(id);
            if let (Some(victim), Some(t)) =
                (self.unregister_on_fire.get(), self.table.borrow().as_ref())
            {
                t.borrow_mut().unregister_timer(victim);
            }
        }

        #[cfg(unix)]
        fn on_fd(&self, fd: RawFd, flags: FdFlags) {
            self.fds.borrow_mut().push((fd, flags));
        }
    }

    fn joined() -> (Rc<RefCell<Registrations>>, Rc<Fake>, Member) {
        let table = Rc::new(RefCell::new(Registrations::default()));
        let fake = Rc::new(Fake::default());
        *fake.table.borrow_mut() = Some(Rc::clone(&table));
        let target: Rc<dyn Target> = fake.clone();
        let member = join(Rc::clone(&table), target);
        (table, fake, member)
    }

    #[cfg(unix)]
    fn tick(now: Instant) {
        run_loop_tick(now, &[]);
    }

    #[cfg(not(unix))]
    fn tick(now: Instant) {
        run_loop_tick(now);
    }

    #[test]
    fn timers_fire_once_per_period_and_once_per_tick() {
        let (table, fake, _m) = joined();
        let ten = table.borrow_mut().register_timer(10).unwrap();
        let zero = table.borrow_mut().register_timer(0).unwrap();
        let t0 = Instant::now();
        tick(t0);
        tick(t0);
        assert_eq!(*fake.timers.borrow(), [zero, zero]);
        fake.timers.borrow_mut().clear();
        tick(t0 + Duration::from_millis(10_000));
        assert_eq!(*fake.timers.borrow(), [ten, zero], "late: once each");
        assert!(table.borrow_mut().unregister_timer(ten));
        assert!(!table.borrow_mut().unregister_timer(ten));
    }

    #[test]
    fn callbacks_are_counted() {
        let (table, _fake, _m) = joined();
        let before = run_loop_calls();
        table.borrow_mut().register_timer(0).unwrap();
        tick(Instant::now());
        tick(Instant::now());
        assert_eq!(run_loop_calls().timers, before.timers + 2);
        assert_eq!(run_loop_calls().fds, before.fds);
    }

    #[test]
    fn a_timer_unregistered_by_an_earlier_callback_does_not_fire() {
        let (table, fake, _m) = joined();
        let a = table.borrow_mut().register_timer(0).unwrap();
        let b = table.borrow_mut().register_timer(0).unwrap();
        fake.unregister_on_fire.set(Some(b));
        tick(Instant::now());
        assert_eq!(*fake.timers.borrow(), [a]);
    }

    #[test]
    fn leaving_the_loop_clears_the_table_and_stops_callbacks() {
        let (table, fake, member) = joined();
        table.borrow_mut().register_timer(0).unwrap();
        drop(member);
        assert!(table.borrow().timers.is_empty());
        tick(Instant::now());
        assert!(fake.timers.borrow().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ready_descriptors_get_the_events_they_asked_for() {
        let (table, fake, _m) = joined();
        assert!(table.borrow_mut().register_fd(3, FdFlags::READ));
        assert!(!table.borrow_mut().register_fd(-1, FdFlags::READ));
        assert_eq!(run_loop_fds(), vec![(3, FdFlags::READ)]);
        let all = FdFlags::READ | FdFlags::WRITE | FdFlags::ERROR;
        run_loop_tick(Instant::now(), &[(3, all), (4, all)]);
        assert_eq!(*fake.fds.borrow(), [(3, FdFlags::READ | FdFlags::ERROR)]);
    }

    #[test]
    fn an_instance_holds_at_most_max_timers() {
        let mut t = Registrations::default();
        let ids: Vec<u32> = (0..MAX_TIMERS)
            .map(|_| t.register_timer(10).unwrap())
            .collect();
        assert_eq!(t.register_timer(10), None);
        assert!(t.unregister_timer(ids[0]));
        assert!(t.register_timer(10).is_some());
        assert_eq!(t.register_timer(10), None);
    }

    #[cfg(unix)]
    #[test]
    fn poll_results_map_to_clap_flags() {
        assert_eq!(poll_events(FdFlags::READ), libc::POLLIN);
        assert_eq!(
            poll_events(FdFlags::READ | FdFlags::WRITE | FdFlags::ERROR),
            libc::POLLIN | libc::POLLOUT
        );
        assert_eq!(ready_flags(0), FdFlags::empty());
        assert_eq!(ready_flags(libc::POLLIN), FdFlags::READ);
        assert_eq!(
            ready_flags(libc::POLLOUT | libc::POLLHUP),
            FdFlags::WRITE | FdFlags::ERROR
        );
        assert_eq!(ready_flags(libc::POLLNVAL), FdFlags::ERROR);
    }

    #[cfg(unix)]
    #[test]
    fn a_descriptor_found_closed_is_told_once() {
        let (table, fake, _m) = joined();
        assert!(table.borrow_mut().register_fd(950, FdFlags::READ));
        // Nothing else has 950 open: POLLNVAL.
        run_loop_pump();
        run_loop_pump();
        assert_eq!(*fake.fds.borrow(), [(950, FdFlags::ERROR)]);
        assert!(run_loop_fds().is_empty());
        table.borrow_mut().rewatch_fd(950);
        assert_eq!(run_loop_fds()[0], (950, FdFlags::READ));
    }
}
