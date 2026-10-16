//! Wait-free parameter queues between the main thread and a processor: the
//! host's values go in, the plugin's gesture events come out. // RT (the
//! audio-side ends)

use crate::process::{ParamChange, ParamEvent};
use core_model::mixer::ParamId;
use rtrb::{Consumer, Producer, RingBuffer};

/// Capacity of each queue.
pub const PARAM_QUEUE: usize = 1024;

/// Main-thread end of the values going to a processor.
pub struct ParamSender(Producer<(ParamId, f64)>);
/// Audio-side end of the values going to a processor.
pub struct ParamReceiver(Consumer<(ParamId, f64)>);
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
    (
        (ParamSender(ptx), GestureReceiver(grx)),
        (ParamReceiver(prx), GestureSender(gtx)),
    )
}

impl ParamSender {
    /// Queues a value; false when the queue is full (try again later).
    pub fn send(&mut self, id: ParamId, value: f64) -> bool {
        self.0.push((id, value)).is_ok()
    }
}

impl ParamReceiver {
    /// Moves queued values into `out` (at frame 0) until it is full;
    /// returns how many. // RT
    pub fn drain_into(&mut self, out: &mut [ParamChange]) -> usize {
        let mut n = 0;
        for slot in out.iter_mut() {
            let Ok((id, value)) = self.0.pop() else {
                break;
            };
            *slot = ParamChange {
                frame: 0,
                id,
                value,
            };
            n += 1;
        }
        n
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
