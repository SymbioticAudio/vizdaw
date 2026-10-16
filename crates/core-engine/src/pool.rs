//! Render workers (spec §7) and the platform seams they use: `RtWaker`
//! (wait/wake), `RtWorkgroup` (join/leave the device's workgroup) and
//! `RtPlatform` (wakers and real-time promotion). `platform-macos`
//! implements them with Mach semaphores, `THREAD_TIME_CONSTRAINT_POLICY`
//! and `os_workgroup`; `StdPlatform` is the portable fallback.

use crate::error::EngineError;
use crate::graph::exec::Job;
use crate::render::{Faults, Shared};
use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

/// How long an idle worker spins before waiting (spec §7: ≈ 20 µs).
pub const SPIN: Duration = Duration::from_micros(20);

/// Wakes waiting workers.
pub trait RtWaker: Send + Sync {
    /// Lets up to `n` waiting workers run. // RT
    fn wake(&self, n: u32);
    /// Blocks the calling worker until woken; spurious returns are allowed.
    fn wait(&self);
    /// The worker threads, once spawned (for wakers that wake threads
    /// directly).
    fn set_threads(&self, threads: Vec<Thread>) {
        let _ = threads;
    }
}

/// A workgroup membership, returned by `join` and handed back to `leave`
/// on the same thread.
pub struct WorkgroupToken(pub Box<dyn Any + Send>);

impl std::fmt::Debug for WorkgroupToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkgroupToken")
    }
}

/// The audio device's thread workgroup.
pub trait RtWorkgroup: Send + Sync {
    /// Joins the calling thread.
    fn join(&self) -> Result<WorkgroupToken, String>;
    /// Removes the calling thread (the one that joined).
    fn leave(&self, token: WorkgroupToken);
}

/// Platform services for render threads.
pub trait RtPlatform: Send + Sync {
    /// A waker for one worker thread (each worker has its own, so wakes can
    /// be targeted).
    fn waker(&self) -> Box<dyn RtWaker>;
    /// Gives the calling thread real-time scheduling for blocks of
    /// `period_ns`.
    fn promote(&self, period_ns: u64) -> Result<(), String>;
}

/// What workers apply between jobs (Plan Ruling 22).
#[derive(Clone, Default)]
pub struct RtEnv {
    /// Block duration in ns (0: leave scheduling as is).
    pub period_ns: u64,
    /// The device's workgroup, when it has one.
    pub workgroup: Option<Arc<dyn RtWorkgroup>>,
}

impl std::fmt::Debug for RtEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RtEnv")
            .field("period_ns", &self.period_ns)
            .field("workgroup", &self.workgroup.is_some())
            .finish()
    }
}

/// The portable platform: a `park`/`unpark` waker, no promotion.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdPlatform;

impl RtPlatform for StdPlatform {
    fn waker(&self) -> Box<dyn RtWaker> {
        Box::new(StdWaker {
            tokens: AtomicU32::new(0),
            cap: 1,
            threads: OnceLock::new(),
        })
    }

    fn promote(&self, _: u64) -> Result<(), String> {
        Ok(())
    }
}

/// A token count capped at `cap`, plus `unpark` (Plan Ruling 21).
struct StdWaker {
    tokens: AtomicU32,
    cap: u32,
    threads: OnceLock<Vec<Thread>>,
}

impl RtWaker for StdWaker {
    fn wake(&self, n: u32) {
        if n == 0 {
            return;
        }
        let mut t = self.tokens.load(Ordering::Acquire);
        loop {
            let next = t.saturating_add(n).min(self.cap);
            match self
                .tokens
                .compare_exchange_weak(t, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(now) => t = now,
            }
        }
        if let Some(ts) = self.threads.get() {
            for t in ts {
                t.unpark();
            }
        }
    }

    fn wait(&self) {
        loop {
            let t = self.tokens.load(Ordering::Acquire);
            if t > 0 {
                if self
                    .tokens
                    .compare_exchange(t, t - 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return;
                }
                continue;
            }
            std::thread::park();
        }
    }

    fn set_threads(&self, threads: Vec<Thread>) {
        let _ = self.threads.set(threads);
    }
}

/// What the executor and the workers share.
pub struct PoolShared {
    job: AtomicPtr<Job>,
    in_job: AtomicU32,
    /// One waker per worker.
    wakers: Box<[Box<dyn RtWaker>]>,
    /// Round-robin cursor for job wakes.
    next: AtomicUsize,
    workers: usize,
    stop: AtomicBool,
    helps: AtomicBool,
    env: Mutex<RtEnv>,
    epoch: AtomicU64,
    /// Environment applications so far (each worker adds one per epoch it
    /// applies).
    acks: AtomicU64,
}

impl std::fmt::Debug for PoolShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PoolShared")
            .field("workers", &self.workers)
            .finish_non_exhaustive()
    }
}

impl PoolShared {
    /// Number of worker threads.
    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Whether the callback thread renders nodes too.
    pub fn callback_helps(&self) -> bool {
        self.helps.load(Ordering::Relaxed)
    }

    /// Sets whether the callback thread renders nodes too (Plan Ruling 19).
    pub fn set_callback_helps(&self, on: bool) {
        self.helps.store(on, Ordering::Relaxed);
    }

    pub(crate) fn active(&self) -> bool {
        self.workers > 0 && !self.stop.load(Ordering::Acquire)
    }

    /// Publishes a job and wakes workers for its `ready` source nodes. // RT
    pub(crate) fn publish(&self, job: *const Job, ready: usize) {
        self.job.store(job.cast_mut(), Ordering::SeqCst);
        self.wake(u32::try_from(ready).unwrap_or(u32::MAX));
    }

    /// Wakes up to `n` workers, round robin (a busy one finds the job
    /// done or takes the next node). // RT
    pub(crate) fn wake(&self, n: u32) {
        let w = self.wakers.len();
        if w == 0 {
            return;
        }
        for _ in 0..(n as usize).min(w) {
            let i = self.next.fetch_add(1, Ordering::Relaxed) % w;
            if let Some(k) = self.wakers.get(i) {
                k.wake(1);
            }
        }
    }

    fn wake_all(&self) {
        for k in self.wakers.iter() {
            k.wake(1);
        }
    }

    /// Unpublishes the job and spins until every worker has left it, or
    /// until `until` passes (false: one is still inside). // RT
    pub(crate) fn retire_job(&self, until: Option<Instant>) -> bool {
        self.job.store(std::ptr::null_mut(), Ordering::SeqCst);
        while self.in_job.load(Ordering::SeqCst) != 0 {
            if until.is_some_and(|u| Instant::now() >= u) {
                return false;
            }
            std::hint::spin_loop();
        }
        true
    }

    /// Publishes `env`, wakes every worker and waits (up to 1 s, control
    /// thread only) until each one applied it.
    fn set_env(&self, env: RtEnv) {
        if let Ok(mut g) = self.env.lock() {
            *g = env;
        }
        let workers = u64::try_from(self.workers).unwrap_or(u64::MAX);
        let target = self.acks.load(Ordering::Acquire).saturating_add(workers);
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.wake_all();
        let start = Instant::now();
        while self.acks.load(Ordering::Acquire) < target && start.elapsed() < Duration::from_secs(1)
        {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// One worker's share of the published job, if any. // RT
    fn help(&self) {
        self.in_job.fetch_add(1, Ordering::SeqCst);
        let p = self.job.load(Ordering::SeqCst);
        if !p.is_null() {
            // SAFETY: `in_job` was raised (SeqCst) before `job` was loaded
            // (SeqCst), and `retire_job` nulls `job` (SeqCst) before waiting
            // for `in_job == 0`; in the single SeqCst order either this load
            // follows the null store (and saw null or a later job) or
            // `retire_job` sees this thread counted and waits. So a non-null
            // job, and the graph, sinks and shared state it points to, stay
            // valid and are not touched mutably by the owner until the
            // `fetch_sub` below. The job's contents were written before the
            // SeqCst store that published it, so they are visible here.
            let job = unsafe { &*p };
            let mut idle: Option<Instant> = None;
            loop {
                // SAFETY: this worker saw the job published and counts in
                // `in_job` (the contract of `Job::step`, argued above).
                if unsafe { job.step() } {
                    idle = None;
                    continue;
                }
                // SAFETY: as for `step` just above.
                if unsafe { job.remaining() } == 0 {
                    break;
                }
                let now = Instant::now();
                match idle {
                    None => idle = Some(now),
                    Some(t) if now.duration_since(t) >= SPIN => break,
                    Some(_) => std::hint::spin_loop(),
                }
            }
        }
        self.in_job.fetch_sub(1, Ordering::SeqCst);
    }
}

fn worker(s: &PoolShared, me: usize, platform: &dyn RtPlatform, faults: &Shared) {
    let mut epoch = 0u64;
    let mut joined: Option<(Arc<dyn RtWorkgroup>, WorkgroupToken)> = None;
    let Some(waker) = s.wakers.get(me) else {
        return;
    };
    loop {
        waker.wait();
        if s.stop.load(Ordering::Acquire) {
            break;
        }
        let e = s.epoch.load(Ordering::Acquire);
        if e != epoch {
            epoch = e;
            let env = s.env.lock().map(|g| (*g).clone()).unwrap_or_default();
            if let Some((wg, token)) = joined.take() {
                wg.leave(token);
            }
            if env.period_ns > 0 && platform.promote(env.period_ns).is_err() {
                faults.raise(Faults::THREAD_POLICY_FAILED);
            }
            if let Some(wg) = env.workgroup {
                match wg.join() {
                    Ok(t) => joined = Some((wg, t)),
                    Err(_) => faults.raise(Faults::WORKGROUP_JOIN_FAILED),
                }
            }
            s.acks.fetch_add(1, Ordering::AcqRel);
        }
        let arm = crate::rt_check::arm_workers();
        if arm {
            crate::rt_check::arm();
        }
        s.help();
        if arm {
            crate::rt_check::disarm();
        }
    }
    if let Some((wg, token)) = joined.take() {
        wg.leave(token);
    }
}

/// The worker threads. Dropping the pool stops and joins them.
pub struct Pool {
    shared: Arc<PoolShared>,
    handles: Vec<JoinHandle<()>>,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool")
            .field("workers", &self.handles.len())
            .finish()
    }
}

impl Pool {
    /// Starts `workers` threads named `vizdaw-render-N`; they raise faults
    /// in `faults`.
    pub fn new(
        workers: usize,
        platform: Arc<dyn RtPlatform>,
        faults: Arc<Shared>,
    ) -> Result<Pool, EngineError> {
        let shared = Arc::new(PoolShared {
            job: AtomicPtr::new(std::ptr::null_mut()),
            in_job: AtomicU32::new(0),
            wakers: (0..workers).map(|_| platform.waker()).collect(),
            next: AtomicUsize::new(0),
            workers,
            stop: AtomicBool::new(false),
            helps: AtomicBool::new(true),
            env: Mutex::new(RtEnv::default()),
            epoch: AtomicU64::new(0),
            acks: AtomicU64::new(0),
        });
        let mut pool = Pool {
            shared,
            handles: Vec::with_capacity(workers),
        };
        for i in 0..workers {
            let (s, p, f) = (
                Arc::clone(&pool.shared),
                Arc::clone(&platform),
                Arc::clone(&faults),
            );
            let h = std::thread::Builder::new()
                .name(format!("vizdaw-render-{i}"))
                .spawn(move || worker(&s, i, &*p, &f))
                .map_err(|e| EngineError::Thread {
                    name: "vizdaw-render",
                    reason: e.to_string(),
                })?;
            pool.handles.push(h);
        }
        pool.register_threads();
        Ok(pool)
    }

    fn register_threads(&self) {
        for (k, h) in self.shared.wakers.iter().zip(&self.handles) {
            k.set_threads(vec![h.thread().clone()]);
        }
    }

    /// The handle executors publish jobs through.
    pub fn handle(&self) -> Arc<PoolShared> {
        Arc::clone(&self.shared)
    }

    /// Number of worker threads.
    pub fn workers(&self) -> usize {
        self.handles.len()
    }

    /// Hands the workers a new real-time environment (applied between
    /// jobs); returns once every worker applied it, or after 1 s. Not RT.
    pub fn set_env(&self, env: RtEnv) {
        self.shared.set_env(env);
    }

    /// Whether the callback thread renders nodes too.
    pub fn set_callback_helps(&self, on: bool) {
        self.shared.set_callback_helps(on);
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        // A failed `new` may not have registered the threads yet.
        self.register_threads();
        self.shared.wake_all();
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}
