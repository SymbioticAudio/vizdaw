//! Real-time thread support on macOS (spec §7): a Mach-semaphore waker,
//! `THREAD_TIME_CONSTRAINT_POLICY` promotion, the device's `os_workgroup`,
//! and the performance-core count. All `unsafe` here calls libSystem.

use core_engine::pool::{RtPlatform, RtWaker, RtWorkgroup, StdPlatform, WorkgroupToken};
use std::ffi::{CStr, c_char, c_void};

type KernReturn = i32;
type MachPort = u32;

#[repr(C)]
struct TimebaseInfo {
    numer: u32,
    denom: u32,
}

/// `struct thread_time_constraint_policy` (`mach/thread_policy.h`).
#[repr(C)]
struct TimeConstraintPolicy {
    period: u32,
    computation: u32,
    constraint: u32,
    preemptible: i32,
}

const THREAD_TIME_CONSTRAINT_POLICY: u32 = 2;
const THREAD_TIME_CONSTRAINT_POLICY_COUNT: u32 = 4;
const SYNC_POLICY_FIFO: i32 = 0;
/// `__OS_WORKGROUP_JOIN_TOKEN_SIZE__` on LP64 (`os/workgroup_base.h`).
const JOIN_TOKEN_SIZE: usize = 36;

/// `struct os_workgroup_join_token_opaque_s`.
#[repr(C)]
struct JoinToken {
    sig: u32,
    opaque: [u8; JOIN_TOKEN_SIZE],
}

unsafe extern "C" {
    static mach_task_self_: MachPort;
    fn semaphore_create(
        task: MachPort,
        semaphore: *mut MachPort,
        policy: i32,
        value: i32,
    ) -> KernReturn;
    fn semaphore_destroy(task: MachPort, semaphore: MachPort) -> KernReturn;
    fn semaphore_signal(semaphore: MachPort) -> KernReturn;
    fn semaphore_wait(semaphore: MachPort) -> KernReturn;
    fn mach_timebase_info(info: *mut TimebaseInfo) -> KernReturn;
    fn pthread_self() -> *mut c_void;
    fn pthread_mach_thread_np(thread: *mut c_void) -> MachPort;
    fn thread_policy_set(thread: MachPort, flavor: u32, policy: *mut i32, count: u32)
    -> KernReturn;
    fn os_workgroup_join(wg: *mut c_void, token: *mut JoinToken) -> i32;
    fn os_workgroup_leave(wg: *mut c_void, token: *mut JoinToken);
    fn os_release(object: *mut c_void);
    fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> i32;
}

/// An `RtWaker` on one Mach semaphore (one per worker).
#[derive(Debug)]
pub struct MachWaker {
    sem: MachPort,
}

impl MachWaker {
    /// A semaphore with count 0.
    pub fn new() -> Result<MachWaker, String> {
        let mut sem: MachPort = 0;
        // SAFETY: `sem` is a valid out-pointer; `mach_task_self_` is this
        // task's port, set up by libSystem before `main`.
        let kr = unsafe { semaphore_create(mach_task_self_, &mut sem, SYNC_POLICY_FIFO, 0) };
        if kr == 0 {
            Ok(MachWaker { sem })
        } else {
            Err(format!("semaphore_create failed ({kr})"))
        }
    }
}

impl RtWaker for MachWaker {
    fn wake(&self, n: u32) {
        for _ in 0..n {
            // SAFETY: `sem` is a live semaphore owned by `self`;
            // `semaphore_signal` is a real-time-safe Mach trap.
            unsafe {
                semaphore_signal(self.sem);
            }
        }
    }

    fn wait(&self) {
        // SAFETY: as above; an interrupted wait returns early, which the
        // waker contract allows.
        unsafe {
            semaphore_wait(self.sem);
        }
    }
}

impl Drop for MachWaker {
    fn drop(&mut self) {
        // SAFETY: the semaphore was created by `new` on this task and is
        // destroyed once.
        unsafe {
            semaphore_destroy(mach_task_self_, self.sem);
        }
    }
}

/// Gives the calling thread `THREAD_TIME_CONSTRAINT_POLICY` for blocks of
/// `period_ns`: period and constraint one block, computation half a block
/// (clamped to 50 µs … 50 ms), preemptible.
pub fn promote_current_thread(period_ns: u64) -> Result<(), String> {
    let mut tb = TimebaseInfo { numer: 0, denom: 0 };
    // SAFETY: `tb` is a valid out-pointer.
    unsafe {
        mach_timebase_info(&mut tb);
    }
    if tb.numer == 0 || tb.denom == 0 {
        return Err("mach_timebase_info failed".into());
    }
    let to_abs = |ns: u64| -> u32 {
        u32::try_from(u128::from(ns) * u128::from(tb.denom) / u128::from(tb.numer))
            .unwrap_or(u32::MAX)
    };
    let period = to_abs(period_ns);
    let mut policy = TimeConstraintPolicy {
        period,
        computation: to_abs((period_ns / 2).clamp(50_000, 50_000_000)).min(period),
        constraint: period,
        preemptible: 1,
    };
    // SAFETY: `policy` is a `thread_time_constraint_policy` of exactly
    // `THREAD_TIME_CONSTRAINT_POLICY_COUNT` integers; the port names the
    // calling thread (`pthread_mach_thread_np` needs no deallocation).
    let kr = unsafe {
        thread_policy_set(
            pthread_mach_thread_np(pthread_self()),
            THREAD_TIME_CONSTRAINT_POLICY,
            (&mut policy as *mut TimeConstraintPolicy).cast::<i32>(),
            THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        )
    };
    if kr == 0 {
        Ok(())
    } else {
        Err(format!("thread_policy_set failed ({kr})"))
    }
}

/// An audio device's IO-thread workgroup (one retained reference).
#[derive(Debug)]
pub struct OsWorkgroup {
    wg: *mut c_void,
}

// SAFETY: `os_workgroup_t` objects are reference counted and thread safe;
// join/leave may be called from any thread (each on its own token).
unsafe impl Send for OsWorkgroup {}
// SAFETY: as above.
unsafe impl Sync for OsWorkgroup {}

impl OsWorkgroup {
    /// Takes ownership of one reference to `wg` (released on drop).
    ///
    /// # Safety
    /// `wg` is null or an `os_workgroup_t` the caller owns one reference to.
    pub unsafe fn from_owned(wg: *mut c_void) -> Option<OsWorkgroup> {
        (!wg.is_null()).then_some(OsWorkgroup { wg })
    }
}

impl RtWorkgroup for OsWorkgroup {
    fn join(&self) -> Result<WorkgroupToken, String> {
        let mut token = Box::new(JoinToken {
            sig: 0,
            opaque: [0; JOIN_TOKEN_SIZE],
        });
        // SAFETY: `wg` is alive (we hold a reference) and `token` is a
        // writable, correctly sized join token.
        let rc = unsafe { os_workgroup_join(self.wg, &mut *token) };
        if rc == 0 {
            Ok(WorkgroupToken(token))
        } else {
            Err(format!("os_workgroup_join failed ({rc})"))
        }
    }

    fn leave(&self, token: WorkgroupToken) {
        if let Ok(mut t) = token.0.downcast::<JoinToken>() {
            // SAFETY: `t` was filled by `os_workgroup_join` on this
            // workgroup; the pool leaves on the thread that joined.
            unsafe { os_workgroup_leave(self.wg, &mut *t) };
        }
    }
}

impl Drop for OsWorkgroup {
    fn drop(&mut self) {
        // SAFETY: releases the one reference taken in `from_owned`.
        unsafe { os_release(self.wg) };
    }
}

/// The macOS platform for render workers.
#[derive(Debug, Clone, Copy, Default)]
pub struct MacRtPlatform;

impl RtPlatform for MacRtPlatform {
    fn waker(&self) -> Box<dyn RtWaker> {
        match MachWaker::new() {
            Ok(w) => Box::new(w),
            Err(_) => StdPlatform.waker(),
        }
    }

    fn promote(&self, period_ns: u64) -> Result<(), String> {
        promote_current_thread(period_ns)
    }
}

fn sysctl_i32(name: &CStr) -> Option<i32> {
    let mut v: i32 = 0;
    let mut len = size_of::<i32>();
    // SAFETY: `v` is a writable i32 of `len` bytes; `name` is
    // NUL-terminated; no new value is set.
    let rc = unsafe {
        sysctlbyname(
            name.as_ptr(),
            (&mut v as *mut i32).cast::<c_void>(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && len == size_of::<i32>()).then_some(v)
}

/// Performance cores (`hw.perflevel0.physicalcpu`, else `hw.physicalcpu`;
/// at least 1).
pub fn performance_cores() -> usize {
    [c"hw.perflevel0.physicalcpu", c"hw.physicalcpu"]
        .into_iter()
        .find_map(|n| sysctl_i32(n).filter(|v| *v > 0))
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(1)
}

/// The spec's default worker count: `min(performance cores − 1, 7)`.
pub fn default_workers() -> usize {
    performance_cores().saturating_sub(1).min(7)
}
