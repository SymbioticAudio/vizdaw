//! The timer kind's main-thread half: it registers host timers and file
//! descriptors when told to through its unlisted parameter switches, and
//! counts the callbacks it gets.

use crate::plugin::MainThread;
use crate::{
    PARAM_FD_CALLS, PARAM_FD_FLAGS, PARAM_FD_MODIFY, PARAM_FD_REGISTER, PARAM_FD_SELF_UNREGISTER,
    PARAM_FD_UNREGISTER, PARAM_LAST_RESULT, PARAM_TIMER_FIRES, PARAM_TIMER_REGISTER,
    PARAM_TIMER_SELF_UNREGISTER, PARAM_TIMER_SPAWN, PARAM_TIMER_UNREGISTER,
};
#[cfg(unix)]
use clack_extensions::posix_fd::{FdFlags, HostPosixFd, PluginPosixFdImpl};
use clack_extensions::timer::{HostTimer, PluginTimerImpl, TimerId};
use std::collections::{HashMap, HashSet};

/// What the timer kind has seen and been told (main thread only).
#[derive(Debug, Default)]
pub struct Clock {
    /// The result of the last register or unregister switch.
    last: f64,
    /// Callbacks per timer id.
    fires: HashMap<u32, u32>,
    /// Timers that unregister themselves in their next callback.
    self_unregister: HashSet<u32>,
    /// Timers whose every callback registers a new period-0 timer.
    spawn: HashSet<u32>,
    /// Callbacks and last flags per descriptor.
    fd_calls: HashMap<i32, (u32, u32)>,
    /// Descriptors that unregister themselves in their next callback.
    fd_self_unregister: HashSet<i32>,
}

fn ok(r: bool) -> f64 {
    f64::from(u8::from(r))
}

impl MainThread<'_> {
    pub(crate) fn register_timer(&self, period_ms: u32) -> Option<u32> {
        let timer: HostTimer = self.host.get_extension()?;
        timer
            .register_timer(&self.host, period_ms)
            .ok()
            .map(|t| t.0)
    }

    pub(crate) fn unregister_timer(&self, id: u32) -> bool {
        self.host
            .get_extension::<HostTimer>()
            .is_some_and(|t| t.unregister_timer(&self.host, TimerId(id)).is_ok())
    }

    #[cfg(unix)]
    fn fd_call(&self, id: u32, value: u32) -> bool {
        let Some(ext) = self.host.get_extension::<HostPosixFd>() else {
            return false;
        };
        let fd = (value / 8) as i32;
        let flags = FdFlags::from_bits_truncate(value % 8);
        match id {
            PARAM_FD_REGISTER => ext.register_fd(&self.host, fd, flags).is_ok(),
            PARAM_FD_MODIFY => ext.modify_fd(&self.host, fd, flags).is_ok(),
            _ => ext.unregister_fd(&self.host, value as i32).is_ok(),
        }
    }

    #[cfg(not(unix))]
    fn fd_call(&self, _id: u32, _value: u32) -> bool {
        false
    }

    /// Applies one switch (main thread, from `flush` while inactive).
    pub(crate) fn command(&self, id: u32, value: f64) {
        let v = value.max(0.0).round() as u32;
        let last = match id {
            PARAM_TIMER_REGISTER => self.register_timer(v).map_or(-1.0, f64::from),
            PARAM_TIMER_UNREGISTER => ok(self.unregister_timer(v)),
            PARAM_TIMER_SELF_UNREGISTER => {
                self.clock.borrow_mut().self_unregister.insert(v);
                return;
            }
            PARAM_TIMER_SPAWN => {
                self.clock.borrow_mut().spawn.insert(v);
                return;
            }
            PARAM_FD_REGISTER | PARAM_FD_MODIFY | PARAM_FD_UNREGISTER => ok(self.fd_call(id, v)),
            PARAM_FD_SELF_UNREGISTER => {
                self.clock.borrow_mut().fd_self_unregister.insert(v as i32);
                return;
            }
            _ => match self.editor_command(id, v) {
                Some(r) => r,
                None => return,
            },
        };
        self.clock.borrow_mut().last = last;
    }

    /// The readouts: the last result, timer fires and descriptor calls.
    pub(crate) fn readout(&self, id: u32) -> Option<f64> {
        let c = self.clock.borrow();
        if id == PARAM_LAST_RESULT {
            return Some(c.last);
        }
        if let Some(v) = self.editor_readout(id) {
            return Some(v);
        }
        if id >= PARAM_FD_FLAGS {
            let fd = (id - PARAM_FD_FLAGS) as i32;
            return Some(f64::from(c.fd_calls.get(&fd).map_or(0, |x| x.1)));
        }
        if id >= PARAM_FD_CALLS {
            let fd = (id - PARAM_FD_CALLS) as i32;
            return Some(f64::from(c.fd_calls.get(&fd).map_or(0, |x| x.0)));
        }
        if id >= PARAM_TIMER_FIRES {
            let t = id - PARAM_TIMER_FIRES;
            return Some(f64::from(c.fires.get(&t).copied().unwrap_or(0)));
        }
        None
    }
}

impl PluginTimerImpl for MainThread<'_> {
    fn on_timer(&self, timer_id: TimerId) {
        let id = timer_id.0;
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            self.gui_on_timer(id);
            return;
        }
        let (unregister, spawn) = {
            let mut c = self.clock.borrow_mut();
            *c.fires.entry(id).or_default() += 1;
            (c.self_unregister.remove(&id), c.spawn.contains(&id))
        };
        if unregister {
            self.unregister_timer(id);
        }
        if spawn {
            let new = self.register_timer(0).map_or(-1.0, f64::from);
            self.clock.borrow_mut().last = new;
        }
    }
}

#[cfg(unix)]
impl PluginPosixFdImpl for MainThread<'_> {
    fn on_fd(&self, fd: i32, flags: FdFlags) {
        #[cfg(target_os = "linux")]
        if self.kind() == crate::Kind::Gui {
            self.gui_on_fd(fd);
            return;
        }
        let unregister = {
            let mut c = self.clock.borrow_mut();
            let e = c.fd_calls.entry(fd).or_default();
            e.0 += 1;
            e.1 = flags.bits();
            c.fd_self_unregister.remove(&fd)
        };
        if unregister {
            if let Some(ext) = self.host.get_extension::<HostPosixFd>() {
                let _ = ext.unregister_fd(&self.host, fd);
            }
        }
    }
}
