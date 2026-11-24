//! The VST3 processor's `unsafe` calls and COM objects (host track H1
//! Task 4): the host's `IParameterChanges` / `IParamValueQueue` and
//! `IEventList` over fixed arrays, the bus buffers handed to `process`,
//! and the `setProcessing` and `process` calls. Everything is allocated
//! in [`RtData::new`] (main thread); nothing here allocates, locks or
//! blocks afterwards. // RT
//!
//! The lists follow the SDK's `public.sdk/source/vst/hosting/eventlist.cpp`
//! and `parameterchanges.cpp`: a queue keeps its points sorted by sample
//! offset and a point at an offset that already has one replaces it.
//! They are used by one thread at a time: the audio thread fills them,
//! the plug-in reads and writes them inside `process` on that thread, and
//! the host reads what it wrote after `process` returns.

use super::ffi::sdk;
use sdk::Vst::{
    AudioBusBuffers, AudioBusBuffers__type0, Event, IAudioProcessor, IAudioProcessorTrait,
    IEventList, IEventListTrait, IParamValueQueue, IParamValueQueueTrait, IParameterChanges,
    IParameterChangesTrait, ParamID, ParamValue, ProcessContext, ProcessData, SymbolicSampleSizes_,
};
use sdk::{TBool, int32, kInvalidArgument, kResultFalse, kResultOk, tresult};
use std::cell::Cell;
use vst3::{Class, ComPtr, ComWrapper};

/// Most points one parameter's queue holds in a block; a later point that
/// does not fit replaces the last one (the latest value wins).
pub(crate) const QUEUE_POINTS: usize = 32;
/// Most parameters with changes in one block, each way: the output list
/// has this many; the input list has this many for controller edits and
/// mapped MIDI controllers, plus [`HOST_QUEUES`] kept for the host's
/// values, plus one for each value a first block carries. Controller
/// edits past their share wait for the next block.
pub(crate) const QUEUES: usize = 128;
/// Input queues kept for the host's values: as many as a block can carry
/// (`MAX_PARAM_EVENTS`), so none is ever dropped (the host counts every
/// value it handed over as delivered).
pub(crate) const HOST_QUEUES: usize = crate::process::MAX_PARAM_EVENTS;
/// Most events the plug-in may add to the output event list in a block.
pub(crate) const OUT_EVENTS: usize = 128;

/// One parameter's changes in a block (`IParamValueQueue`).
pub(crate) struct ParamQueue {
    id: Cell<ParamID>,
    len: Cell<usize>,
    points: [Cell<(int32, ParamValue)>; QUEUE_POINTS],
    /// A point came from the host (automation, a host edit or a mapped
    /// MIDI controller), not from the plug-in's own controller.
    from_host: Cell<bool>,
}

impl ParamQueue {
    fn new() -> ParamQueue {
        ParamQueue {
            id: Cell::new(0),
            len: Cell::new(0),
            points: [const { Cell::new((0, 0.0)) }; QUEUE_POINTS],
            from_host: Cell::new(false),
        }
    }

    fn reset(&self, id: ParamID) {
        self.id.set(id);
        self.len.set(0);
        self.from_host.set(false);
    }

    /// The parameter.
    pub(crate) fn id(&self) -> ParamID {
        self.id.get()
    }

    /// Whether a point came from the host.
    pub(crate) fn host_sourced(&self) -> bool {
        self.from_host.get()
    }

    /// The point at `i`.
    fn point(&self, i: usize) -> Option<(int32, ParamValue)> {
        (i < self.len.get())
            .then(|| self.points.get(i).map(Cell::get))
            .flatten()
    }

    /// The last point (the value at the end of the block).
    pub(crate) fn last(&self) -> Option<(int32, ParamValue)> {
        self.len.get().checked_sub(1).and_then(|i| self.point(i))
    }

    /// Adds a point, keeping them sorted by offset; returns its index. A
    /// point at an offset that has one replaces it; a full queue takes a
    /// point after its last one in place of that one.
    pub(crate) fn add(&self, offset: int32, value: ParamValue) -> Option<usize> {
        let len = self.len.get();
        let mut pos = len;
        while pos > 0 && self.point(pos - 1).is_some_and(|(o, _)| o > offset) {
            pos -= 1;
        }
        if let Some(prev) = pos.checked_sub(1) {
            if self.point(prev).is_some_and(|(o, _)| o == offset) {
                self.points.get(prev)?.set((offset, value));
                return Some(prev);
            }
        }
        if len == QUEUE_POINTS {
            if pos != len {
                return None;
            }
            self.points.get(len - 1)?.set((offset, value));
            return Some(len - 1);
        }
        for k in (pos..len).rev() {
            let p = self.points.get(k)?.get();
            self.points.get(k + 1)?.set(p);
        }
        self.points.get(pos)?.set((offset, value));
        self.len.set(len + 1);
        Some(pos)
    }
}

impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }

    unsafe fn getPointCount(&self) -> int32 {
        int32::try_from(self.len.get()).unwrap_or(0)
    }

    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        let Some((o, v)) = usize::try_from(index).ok().and_then(|i| self.point(i)) else {
            return kInvalidArgument;
        };
        if offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller's out values, checked non-null.
        unsafe {
            *offset = o;
            *value = v;
        }
        kResultOk
    }

    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        let Some(i) = self.add(offset, value) else {
            return kResultFalse;
        };
        if !index.is_null() {
            // SAFETY: the caller's out value, checked non-null.
            unsafe { *index = int32::try_from(i).unwrap_or(0) };
        }
        kResultOk
    }
}

/// A block's parameter changes (`IParameterChanges`): up to [`QUEUES`]
/// queues, one per parameter.
pub(crate) struct ParamChanges {
    queues: Box<[ComWrapper<ParamQueue>]>,
    used: Cell<usize>,
}

impl ParamChanges {
    fn new(queues: usize) -> ParamChanges {
        ParamChanges {
            queues: (0..queues)
                .map(|_| ComWrapper::new(ParamQueue::new()))
                .collect(),
            used: Cell::new(0),
        }
    }

    /// Whether more than `keep` queues are still free. // RT
    pub(crate) fn has_room_beyond(&self, keep: usize) -> bool {
        self.used.get().saturating_add(keep) < self.queues.len()
    }

    /// Forgets every change. // RT
    pub(crate) fn clear(&self) {
        self.used.set(0);
    }

    /// The queues in use, in the order they were added. // RT
    pub(crate) fn queues(&self) -> impl Iterator<Item = &ParamQueue> {
        self.queues.iter().take(self.used.get()).map(|w| &**w)
    }

    /// The queue of `id` and its index: the one in use, else a new one;
    /// `None` when every queue is taken. // RT
    fn queue(&self, id: ParamID) -> Option<(usize, &ComWrapper<ParamQueue>)> {
        let used = self.used.get();
        if let Some(found) = self.queues.iter().take(used).position(|q| q.id() == id) {
            return self.queues.get(found).map(|q| (found, q));
        }
        let q = self.queues.get(used)?;
        q.reset(id);
        self.used.set(used + 1);
        Some((used, q))
    }

    /// Adds a point for `id`; false when it was dropped. // RT
    pub(crate) fn add(
        &self,
        id: ParamID,
        offset: int32,
        value: ParamValue,
        from_host: bool,
    ) -> bool {
        let Some((_, q)) = self.queue(id) else {
            return false;
        };
        if from_host {
            q.from_host.set(true);
        }
        q.add(offset, value).is_some()
    }
}

/// `q` as the pointer the plug-in gets (not a new reference: the list
/// keeps it alive).
fn queue_ptr(q: &ComWrapper<ParamQueue>) -> *mut IParamValueQueue {
    q.as_com_ref::<IParamValueQueue>()
        .map_or(std::ptr::null_mut(), |r| r.as_ptr())
}

impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        int32::try_from(self.used.get()).unwrap_or(0)
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        usize::try_from(index)
            .ok()
            .filter(|i| *i < self.used.get())
            .and_then(|i| self.queues.get(i))
            .map_or(std::ptr::null_mut(), queue_ptr)
    }

    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut int32,
    ) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's ID, checked non-null.
        let id = unsafe { *id };
        let Some((i, q)) = self.queue(id) else {
            return std::ptr::null_mut();
        };
        if !index.is_null() {
            // SAFETY: the caller's out value, checked non-null.
            unsafe { *index = int32::try_from(i).unwrap_or(0) };
        }
        queue_ptr(q)
    }
}

/// A block's events (`IEventList`) over a fixed array.
pub(crate) struct EventList {
    events: Box<[Cell<Event>]>,
    len: Cell<usize>,
}

impl EventList {
    fn new(capacity: usize) -> EventList {
        // SAFETY: an all-zero `Event` is a valid plain value (a note-on
        // on bus 0 at offset 0 with every field zero).
        let zero: Event = unsafe { std::mem::zeroed() };
        EventList {
            events: (0..capacity).map(|_| Cell::new(zero)).collect(),
            len: Cell::new(0),
        }
    }

    /// Forgets every event. // RT
    pub(crate) fn clear(&self) {
        self.len.set(0);
    }

    /// Appends `e`; false when full. // RT
    pub(crate) fn push(&self, e: Event) -> bool {
        let len = self.len.get();
        match self.events.get(len) {
            Some(slot) => {
                slot.set(e);
                self.len.set(len + 1);
                true
            }
            None => false,
        }
    }
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        int32::try_from(self.len.get()).unwrap_or(0)
    }

    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        let found = usize::try_from(index)
            .ok()
            .filter(|i| *i < self.len.get())
            .and_then(|i| self.events.get(i));
        match found {
            Some(ev) if !e.is_null() => {
                // SAFETY: the caller's out value, checked non-null.
                unsafe { *e = ev.get() };
                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        if e.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the caller's event, checked non-null.
        if self.push(unsafe { *e }) {
            kResultOk
        } else {
            kResultFalse
        }
    }
}

/// One direction's audio buses as `process` takes them: an
/// `AudioBusBuffers` per bus, pointing into one array of channel
/// pointers, which the processor sets every block.
pub(crate) struct Buses {
    buffers: Box<[AudioBusBuffers]>,
    channels: Box<[*mut f32]>,
}

impl Buses {
    fn new(counts: &[u16]) -> Buses {
        let total: usize = counts.iter().map(|c| usize::from(*c)).sum();
        let mut channels: Box<[*mut f32]> = vec![std::ptr::null_mut(); total].into_boxed_slice();
        let base = channels.as_mut_ptr();
        let mut start = 0usize;
        let buffers = counts
            .iter()
            .map(|c| {
                let b = AudioBusBuffers {
                    numChannels: int32::from(*c),
                    silenceFlags: 0,
                    __field0: AudioBusBuffers__type0 {
                        // Into the boxed array, which never moves.
                        channelBuffers32: base.wrapping_add(start),
                    },
                };
                start += usize::from(*c);
                b
            })
            .collect();
        Buses { buffers, channels }
    }

    /// Points channel `k` (counted over every bus) at `buffer`, which must
    /// hold at least the block's frames until `process` returns. // RT
    pub(crate) fn set(&mut self, k: usize, buffer: &mut [f32]) {
        if let Some(c) = self.channels.get_mut(k) {
            *c = buffer.as_mut_ptr();
        }
    }
}

/// Everything a `process` call hands the plug-in, allocated once, and the
/// plug-in's processor. Moved to the render threads with the processor.
pub(crate) struct RtData {
    processor: ComPtr<IAudioProcessor>,
    /// Input and output buses.
    pub(crate) ins: Buses,
    pub(crate) outs: Buses,
    params_in: ComWrapper<ParamChanges>,
    params_out: ComWrapper<ParamChanges>,
    events_in: ComWrapper<EventList>,
    events_out: ComWrapper<EventList>,
    context: Box<ProcessContext>,
}

// SAFETY: `RtData` is used by one thread at a time (the `AudioProcessor`
// contract): the COM objects' cells and the raw channel pointers are only
// touched by the thread that holds it, and by the plug-in inside a
// `process` call that thread makes. The plug-in's processor may be called
// from any one thread at a time (the SDK's processing-thread rule).
unsafe impl Send for RtData {}

impl RtData {
    /// Allocates the lists (the input changes with room for `first`
    /// parameters beyond [`QUEUES`] and [`HOST_QUEUES`], so a first
    /// block's values and every host value all fit),
    /// the context and buses with `in_counts` and `out_counts` channels
    /// (main thread).
    pub(crate) fn new(
        processor: ComPtr<IAudioProcessor>,
        in_counts: &[u16],
        out_counts: &[u16],
        first: usize,
    ) -> RtData {
        RtData {
            processor,
            ins: Buses::new(in_counts),
            outs: Buses::new(out_counts),
            params_in: ComWrapper::new(ParamChanges::new(
                QUEUES.saturating_add(HOST_QUEUES).saturating_add(first),
            )),
            params_out: ComWrapper::new(ParamChanges::new(QUEUES)),
            events_in: ComWrapper::new(EventList::new(crate::process::MAX_EVENTS)),
            events_out: ComWrapper::new(EventList::new(OUT_EVENTS)),
            // SAFETY: an all-zero `ProcessContext` is a valid plain value
            // (no state flags set).
            context: Box::new(unsafe { std::mem::zeroed() }),
        }
    }

    /// The input parameter changes. // RT
    pub(crate) fn params_in(&self) -> &ParamChanges {
        &self.params_in
    }

    /// The plug-in's output parameter changes. // RT
    pub(crate) fn params_out(&self) -> &ParamChanges {
        &self.params_out
    }

    /// The input events. // RT
    pub(crate) fn events_in(&self) -> &EventList {
        &self.events_in
    }

    /// The transport the next block gets. // RT
    pub(crate) fn context_mut(&mut self) -> &mut ProcessContext {
        &mut self.context
    }

    /// `setProcessing`; whether the plug-in took it. // RT
    pub(crate) fn set_processing(&self, on: bool) -> bool {
        // SAFETY: a plain call; the SDK allows it on the processing thread.
        unsafe { self.processor.setProcessing(TBool::from(on)) == kResultOk }
    }

    /// Runs `process` for `frames` frames in `mode` with the buses, lists
    /// and context as set; clears the output lists first. Whether it
    /// returned `kResultOk`. // RT
    pub(crate) fn process(&mut self, mode: int32, frames: int32) -> bool {
        self.params_out.clear();
        self.events_out.clear();
        let ptr_of = |c: &ComWrapper<ParamChanges>| {
            c.as_com_ref::<IParameterChanges>()
                .map_or(std::ptr::null_mut(), |r| r.as_ptr())
        };
        let ev_of = |e: &ComWrapper<EventList>| {
            e.as_com_ref::<IEventList>()
                .map_or(std::ptr::null_mut(), |r| r.as_ptr())
        };
        let mut data = ProcessData {
            processMode: mode,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: frames,
            numInputs: int32::try_from(self.ins.buffers.len()).unwrap_or(0),
            numOutputs: int32::try_from(self.outs.buffers.len()).unwrap_or(0),
            inputs: self.ins.buffers.as_mut_ptr(),
            outputs: self.outs.buffers.as_mut_ptr(),
            inputParameterChanges: ptr_of(&self.params_in),
            outputParameterChanges: ptr_of(&self.params_out),
            inputEvents: ev_of(&self.events_in),
            outputEvents: ev_of(&self.events_out),
            processContext: &mut *self.context,
        };
        // SAFETY: every pointer in `data` is live for the call: the bus
        // arrays and the channel pointers they hold (each channel set to a
        // buffer of at least `frames` samples by the caller), the lists
        // (owned here) and the context.
        unsafe { self.processor.process(&mut data) == kResultOk }
    }
}
