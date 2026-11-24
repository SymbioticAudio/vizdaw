//! Wait-free parameter queues between the main thread and a processor: the
//! host's values go in, the plugin's gesture events come out. // RT (the
//! audio-side ends)

use crate::process::{ParamChange, ParamEvent};
use core_model::mixer::ParamId;
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Capacity of each queue.
pub const PARAM_QUEUE: usize = 1024;

/// Main-thread end of the values going to a processor. It numbers what
/// it sends, so the host can tell which values no processed block has
/// delivered yet ([`ParamSender::undelivered`]).
pub struct ParamSender {
    queue: Producer<(ParamId, f64)>,
    /// Values sent so far.
    sent: u64,
    /// The number (in `sent`'s count) of each parameter's latest value.
    latest: BTreeMap<ParamId, u64>,
    /// Values a processed block has carried (published by the receiver).
    delivered: Arc<AtomicU64>,
}
/// Audio-side end of the values going to a processor.
pub struct ParamReceiver {
    queue: Consumer<(ParamId, f64)>,
    /// Values taken so far.
    taken: u64,
    delivered: Arc<AtomicU64>,
}
/// Audio-side end of the plugin's parameter events.
pub struct GestureSender(Producer<ParamEvent>);
/// Main-thread end of the plugin's parameter events.
pub struct GestureReceiver(Consumer<ParamEvent>);

impl std::fmt::Debug for ParamSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ParamSender")
    }
}

/// Both queues of one processor: `(main-thread ends, audio-side ends)`.
pub fn param_queues() -> (
    (ParamSender, GestureReceiver),
    (ParamReceiver, GestureSender),
) {
    let (ptx, prx) = RingBuffer::new(PARAM_QUEUE);
    let (gtx, grx) = RingBuffer::new(PARAM_QUEUE);
    let delivered = Arc::new(AtomicU64::new(0));
    let sender = ParamSender {
        queue: ptx,
        sent: 0,
        latest: BTreeMap::new(),
        delivered: delivered.clone(),
    };
    let receiver = ParamReceiver {
        queue: prx,
        taken: 0,
        delivered,
    };
    (
        (sender, GestureReceiver(grx)),
        (receiver, GestureSender(gtx)),
    )
}

impl ParamSender {
    /// Queues a value; false when the queue is full (try again later).
    pub fn send(&mut self, id: ParamId, value: f64) -> bool {
        if self.queue.push((id, value)).is_err() {
            return false;
        }
        self.sent += 1;
        self.latest.insert(id, self.sent);
        true
    }

    /// Queues a value the plugin already holds (set while it was
    /// inactive): not counted by [`ParamSender::undelivered`]. False when
    /// the queue is full.
    pub fn send_held(&mut self, id: ParamId, value: f64) -> bool {
        if self.queue.push((id, value)).is_err() {
            return false;
        }
        self.sent += 1;
        self.latest.remove(&id);
        true
    }

    /// The parameters whose latest value sent here no processed block has
    /// carried yet: the plugin does not hold it, so its saved state does
    /// not either.
    pub fn undelivered(&mut self) -> Vec<ParamId> {
        let delivered = self.delivered.load(Ordering::Acquire);
        self.latest.retain(|_, n| *n > delivered);
        self.latest.keys().copied().collect()
    }
}

impl ParamReceiver {
    /// Moves queued values into `out` (at frame 0) until it is full;
    /// returns how many. // RT
    pub fn drain_into(&mut self, out: &mut [ParamChange]) -> usize {
        let mut n = 0;
        for slot in out.iter_mut() {
            let Ok((id, value)) = self.queue.pop() else {
                break;
            };
            self.taken += 1;
            *slot = ParamChange {
                frame: 0,
                id,
                value,
            };
            n += 1;
        }
        n
    }

    /// Tells the main thread that every value taken so far has reached
    /// the processor: call after a processed block. // RT
    pub fn block_done(&self) {
        self.delivered.store(self.taken, Ordering::Release);
    }
}

impl GestureSender {
    /// Queues events; returns how many did not fit. // RT
    pub fn send_all(&mut self, events: &[ParamEvent]) -> usize {
        events.iter().filter(|e| self.0.push(**e).is_err()).count()
    }
}

impl GestureReceiver {
    /// Every queued event.
    pub fn drain(&mut self) -> Vec<ParamEvent> {
        let mut v = Vec::new();
        while let Ok(e) = self.0.pop() {
            v.push(e);
        }
        v
    }
}
