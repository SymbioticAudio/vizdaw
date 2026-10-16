//! CoreAudio HAL output through an IOProc on the device's own real-time
//! thread (no AUHAL).

use core_engine::BackendError;
use core_engine::backend::{
    AudioBackend, AudioConfig, BackendEvent, BlockTime, MAX_FRAMES, StreamInfo,
};
use core_engine::render::{Faults, Renderer};
use objc2_core_audio::{
    AudioConvertHostTimeToNanos, AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID,
    AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop, AudioGetCurrentHostTime,
    AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertyScope,
    AudioObjectPropertySelector, AudioObjectRemovePropertyListener, AudioObjectSetPropertyData,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreams, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
    kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    kAudioStreamPropertyVirtualFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioFormatFlagIsFloat, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{CFRetained, CFString};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const SYSTEM: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;

fn os(call: &'static str, status: i32) -> Result<(), BackendError> {
    if status == 0 {
        Ok(())
    } else {
        Err(BackendError::Os { call, status })
    }
}

fn addr(
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// Reads a fixed-size property.
fn get<T: Copy + Default>(
    obj: AudioObjectID,
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> Result<T, BackendError> {
    let mut a = addr(selector, scope);
    let mut value = T::default();
    let mut size = size_of::<T>() as u32;
    // SAFETY: `value` is a valid buffer of `size` bytes for a property of
    // type `T`; `a` and `size` are valid pointers.
    os("AudioObjectGetPropertyData", unsafe {
        AudioObjectGetPropertyData(
            obj,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast::<c_void>(),
        )
    })?;
    Ok(value)
}

/// Writes a fixed-size property.
fn set<T: Copy>(
    obj: AudioObjectID,
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    value: T,
) -> Result<(), BackendError> {
    let mut a = addr(selector, scope);
    let mut v = value;
    // SAFETY: `v` is a valid `T` of the stated size.
    os("AudioObjectSetPropertyData", unsafe {
        AudioObjectSetPropertyData(
            obj,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            size_of::<T>() as u32,
            NonNull::from(&mut v).cast::<c_void>(),
        )
    })
}

/// Reads a variable-size property as 8-byte-aligned words.
fn get_bytes(
    obj: AudioObjectID,
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> Result<Vec<u64>, BackendError> {
    let mut a = addr(selector, scope);
    let mut size = 0u32;
    // SAFETY: valid pointers.
    os("AudioObjectGetPropertyDataSize", unsafe {
        AudioObjectGetPropertyDataSize(
            obj,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
        )
    })?;
    let mut buf = vec![0u64; (size as usize).div_ceil(8).max(1)];
    // SAFETY: `buf` holds at least `size` bytes.
    os("AudioObjectGetPropertyData", unsafe {
        AudioObjectGetPropertyData(
            obj,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(buf.as_mut_ptr().cast::<c_void>()).unwrap_or(NonNull::dangling()),
        )
    })?;
    buf.truncate((size as usize).div_ceil(8));
    Ok(buf)
}

/// Reads an array-of-`u32` property (object IDs).
fn get_ids(
    obj: AudioObjectID,
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> Result<Vec<AudioObjectID>, BackendError> {
    let words = get_bytes(obj, selector, scope)?;
    Ok(words
        .iter()
        .flat_map(|w| [*w as u32, (*w >> 32) as u32])
        .filter(|id| *id != 0)
        .collect())
}

fn name_of(obj: AudioObjectID) -> Option<String> {
    let mut a = addr(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal);
    let mut s: *const CFString = ptr::null();
    let mut size = size_of::<*const CFString>() as u32;
    // SAFETY: the name property is a +1 retained CFStringRef.
    let status = unsafe {
        AudioObjectGetPropertyData(
            obj,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut s).cast::<c_void>(),
        )
    };
    let p = NonNull::new(s.cast_mut())?;
    if status != 0 {
        return None;
    }
    // SAFETY: see above; we take over the reference.
    let s: CFRetained<CFString> = unsafe { CFRetained::from_raw(p) };
    Some(s.to_string())
}

/// Output channel counts of the device's output buffers, in order.
fn output_layout(dev: AudioObjectID) -> Result<Vec<u32>, BackendError> {
    let words = get_bytes(
        dev,
        kAudioDevicePropertyStreamConfiguration,
        kAudioObjectPropertyScopeOutput,
    )?;
    let list = words.as_ptr().cast::<AudioBufferList>();
    // SAFETY: the property is an AudioBufferList with `mNumberBuffers`
    // trailing buffers, stored in `words` (8-byte aligned).
    let n = unsafe { (*list).mNumberBuffers } as usize;
    let max = (words.len() * 8).saturating_sub(8) / size_of::<AudioBuffer>();
    // SAFETY: as above; `n` is clamped to what the buffer holds.
    let bufs = unsafe { std::slice::from_raw_parts((*list).mBuffers.as_ptr(), n.min(max)) };
    Ok(bufs.iter().map(|b| b.mNumberChannels).collect())
}

/// Every output stream must be 32-bit float linear PCM.
fn check_formats(dev: AudioObjectID) -> Result<(), BackendError> {
    for stream in get_ids(
        dev,
        kAudioDevicePropertyStreams,
        kAudioObjectPropertyScopeOutput,
    )? {
        let f: AudioStreamBasicDescription = get_asbd(stream)?;
        if f.mFormatID != kAudioFormatLinearPCM
            || f.mFormatFlags & kAudioFormatFlagIsFloat == 0
            || f.mBitsPerChannel != 32
        {
            return Err(BackendError::Format {
                detail: format!(
                    "stream {stream}: format {:#x}, flags {:#x}, {} bits",
                    f.mFormatID, f.mFormatFlags, f.mBitsPerChannel
                ),
            });
        }
    }
    Ok(())
}

fn get_asbd(stream: AudioObjectID) -> Result<AudioStreamBasicDescription, BackendError> {
    let mut a = addr(
        kAudioStreamPropertyVirtualFormat,
        kAudioObjectPropertyScopeGlobal,
    );
    let mut f = AudioStreamBasicDescription {
        mSampleRate: 0.0,
        mFormatID: 0,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 0,
        mBitsPerChannel: 0,
        mReserved: 0,
    };
    let mut size = size_of::<AudioStreamBasicDescription>() as u32;
    // SAFETY: `f` is a valid ASBD buffer.
    os("AudioObjectGetPropertyData(VirtualFormat)", unsafe {
        AudioObjectGetPropertyData(
            stream,
            NonNull::from(&mut a),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::from(&mut f).cast::<c_void>(),
        )
    })?;
    Ok(f)
}

/// Names of every device with output channels.
pub fn output_devices() -> Vec<String> {
    get_ids(
        SYSTEM,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )
    .unwrap_or_default()
    .into_iter()
    .filter(|d| output_layout(*d).is_ok_and(|l| l.iter().sum::<u32>() > 0))
    .filter_map(name_of)
    .collect()
}

/// Flags set by property listeners (non-RT CoreAudio notification thread)
/// and read by `take_events`.
#[derive(Debug, Default)]
struct Flags {
    dead: AtomicBool,
    default_changed: AtomicBool,
    rate_changed: AtomicBool,
    buffer_changed: AtomicBool,
}

unsafe extern "C-unwind" fn listener(
    _obj: AudioObjectID,
    count: u32,
    addresses: NonNull<AudioObjectPropertyAddress>,
    client: *mut c_void,
) -> i32 {
    // SAFETY: `client` is the `Arc<Flags>` pointer registered in `listen`,
    // kept alive until the listener is removed; `addresses` holds `count`
    // entries.
    let (flags, addrs) = unsafe {
        (
            &*client.cast::<Flags>(),
            std::slice::from_raw_parts(addresses.as_ptr(), count as usize),
        )
    };
    for a in addrs {
        match a.mSelector {
            s if s == kAudioDevicePropertyDeviceIsAlive => {
                flags.dead.store(true, Ordering::Relaxed)
            }
            s if s == kAudioHardwarePropertyDefaultOutputDevice => {
                flags.default_changed.store(true, Ordering::Relaxed)
            }
            s if s == kAudioDevicePropertyNominalSampleRate => {
                flags.rate_changed.store(true, Ordering::Relaxed)
            }
            s if s == kAudioDevicePropertyBufferFrameSize => {
                flags.buffer_changed.store(true, Ordering::Relaxed)
            }
            _ => {}
        }
    }
    0
}

/// State owned by the IOProc while running.
struct RtContext {
    renderer: Renderer,
    left: Box<[f32]>,
    right: Box<[f32]>,
    poisoned: bool,
}

/// Zeroes every output buffer. // RT
fn silence(out: &mut AudioBufferList) {
    let n = out.mNumberBuffers as usize;
    // SAFETY: CoreAudio hands us `mNumberBuffers` trailing buffers.
    let bufs = unsafe { std::slice::from_raw_parts_mut(out.mBuffers.as_mut_ptr(), n) };
    for b in bufs {
        if !b.mData.is_null() {
            // SAFETY: `mData` holds `mDataByteSize` bytes.
            unsafe { ptr::write_bytes(b.mData.cast::<u8>(), 0, b.mDataByteSize as usize) };
        }
    }
}

/// Renders planar stereo and writes it to the device's buffers:
/// global channel 0 = left, 1 = right, others zero. // RT
fn render(ctx: &mut RtContext, out: &mut AudioBufferList, time: &AudioTimeStamp) {
    let n = out.mNumberBuffers as usize;
    // SAFETY: CoreAudio hands us `mNumberBuffers` trailing buffers.
    let bufs = unsafe { std::slice::from_raw_parts_mut(out.mBuffers.as_mut_ptr(), n) };
    let Some(first) = bufs.first() else {
        return;
    };
    let ch0 = first.mNumberChannels.max(1);
    let frames = first.mDataByteSize / (4 * ch0);
    let host_ns = if time.mFlags.contains(AudioTimeStampFlags::HostTimeValid) {
        // SAFETY: pure conversion.
        unsafe { AudioConvertHostTimeToNanos(time.mHostTime) }
    } else {
        // SAFETY: pure queries.
        unsafe { AudioConvertHostTimeToNanos(AudioGetCurrentHostTime()) }
    };
    let sr = ctx.renderer.config().sample_rate;
    let len = (frames as usize).min(ctx.left.len());
    let (Some(l), Some(r)) = (ctx.left.get_mut(..len), ctx.right.get_mut(..len)) else {
        return;
    };
    let mut chans: [&mut [f32]; 2] = [l, r];
    ctx.renderer.process(
        &mut chans,
        frames,
        BlockTime {
            host_time_ns: host_ns,
            sample_rate: sr,
        },
    );
    let mut global = 0usize;
    for b in bufs.iter_mut() {
        let nch = b.mNumberChannels.max(1) as usize;
        let samples = b.mDataByteSize as usize / 4;
        if b.mData.is_null() {
            global += nch;
            continue;
        }
        // SAFETY: `mData` holds `mDataByteSize` bytes of f32 samples.
        let data = unsafe { std::slice::from_raw_parts_mut(b.mData.cast::<f32>(), samples) };
        for (i, x) in data.iter_mut().enumerate() {
            let frame = i / nch;
            let ch = global + i % nch;
            *x = match ch {
                0 => chans[0].get(frame).copied().unwrap_or(0.0),
                1 => chans[1].get(frame).copied().unwrap_or(0.0),
                _ => 0.0,
            };
        }
        global += nch;
    }
}

unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    _input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    mut output: NonNull<AudioBufferList>,
    output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    // SAFETY: `client` is the `RtContext` boxed in `start`, owned by this
    // IOProc until `stop` has returned from `AudioDeviceStop`; the buffer
    // list and timestamp are valid for this call. // RT
    let (ctx, out, time) = unsafe {
        (
            &mut *client.cast::<RtContext>(),
            output.as_mut(),
            output_time.as_ref(),
        )
    };
    if ctx.poisoned {
        silence(out);
        return 0;
    }
    let ok = catch_unwind(AssertUnwindSafe(|| render(ctx, out, time))).is_ok();
    if !ok {
        // Never unwind into CoreAudio: go silent for good and report.
        ctx.poisoned = true;
        ctx.renderer.raise(Faults::PANIC);
        silence(out);
    }
    0
}

struct Running {
    proc_id: AudioDeviceIOProcID,
    ctx: *mut RtContext,
}

struct Listening {
    flags: *const Flags,
    device: AudioObjectID,
    follow_default: bool,
}

/// The HAL output backend.
pub struct CoreAudioBackend {
    device: Option<AudioObjectID>,
    info: Option<StreamInfo>,
    running: Option<Running>,
    listening: Option<Listening>,
}

impl std::fmt::Debug for CoreAudioBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreAudioBackend")
            .field("device", &self.device)
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

// SAFETY: the raw pointers are owned by the backend and only touched by the
// control thread (the IOProc owns `ctx` between start and stop).
unsafe impl Send for CoreAudioBackend {}

impl Default for CoreAudioBackend {
    fn default() -> Self {
        CoreAudioBackend::new()
    }
}

const LISTENED: [(AudioObjectPropertySelector, bool); 4] = [
    (kAudioDevicePropertyDeviceIsAlive, false),
    (kAudioDevicePropertyNominalSampleRate, false),
    (kAudioDevicePropertyBufferFrameSize, false),
    (kAudioHardwarePropertyDefaultOutputDevice, true),
];

impl CoreAudioBackend {
    /// A closed backend.
    pub fn new() -> CoreAudioBackend {
        CoreAudioBackend {
            device: None,
            info: None,
            running: None,
            listening: None,
        }
    }

    /// What `open` found.
    pub fn info(&self) -> Option<&StreamInfo> {
        self.info.as_ref()
    }

    fn listen(&mut self, device: AudioObjectID, follow_default: bool) {
        let flags = Arc::into_raw(Arc::new(Flags::default()));
        for (selector, system) in LISTENED {
            if system && !follow_default {
                continue;
            }
            let obj = if system { SYSTEM } else { device };
            let mut a = addr(selector, kAudioObjectPropertyScopeGlobal);
            // SAFETY: `flags` stays alive until `unlisten` removes this
            // listener with the same arguments.
            unsafe {
                AudioObjectAddPropertyListener(
                    obj,
                    NonNull::from(&mut a),
                    Some(listener),
                    flags.cast_mut().cast::<c_void>(),
                );
            }
        }
        self.listening = Some(Listening {
            flags,
            device,
            follow_default,
        });
    }

    fn unlisten(&mut self) {
        let Some(l) = self.listening.take() else {
            return;
        };
        for (selector, system) in LISTENED {
            if system && !l.follow_default {
                continue;
            }
            let obj = if system { SYSTEM } else { l.device };
            let mut a = addr(selector, kAudioObjectPropertyScopeGlobal);
            // SAFETY: removes exactly the listener added in `listen`.
            unsafe {
                AudioObjectRemovePropertyListener(
                    obj,
                    NonNull::from(&mut a),
                    Some(listener),
                    l.flags.cast_mut().cast::<c_void>(),
                );
            }
        }
        // SAFETY: no listener refers to `flags` any more.
        drop(unsafe { Arc::from_raw(l.flags) });
    }

    /// Stops (if running) and forgets the device.
    pub fn close(&mut self) -> Option<Renderer> {
        let r = self.stop().ok();
        self.unlisten();
        self.device = None;
        self.info = None;
        r
    }
}

impl AudioBackend for CoreAudioBackend {
    fn open(&mut self, cfg: &AudioConfig) -> Result<StreamInfo, BackendError> {
        self.close();
        let device = match &cfg.device {
            None => get::<AudioObjectID>(
                SYSTEM,
                kAudioHardwarePropertyDefaultOutputDevice,
                kAudioObjectPropertyScopeGlobal,
            )?,
            Some(name) => get_ids(
                SYSTEM,
                kAudioHardwarePropertyDevices,
                kAudioObjectPropertyScopeGlobal,
            )?
            .into_iter()
            .find(|d| name_of(*d).as_deref() == Some(name.as_str()))
            .ok_or(BackendError::NoDevice)?,
        };
        if device == 0 {
            return Err(BackendError::NoDevice);
        }
        let layout = output_layout(device)?;
        let channels: u32 = layout.iter().sum();
        if channels == 0 {
            return Err(BackendError::NoDevice);
        }
        check_formats(device)?;
        if let Some(n) = cfg.buffer_frames {
            set(
                device,
                kAudioDevicePropertyBufferFrameSize,
                kAudioObjectPropertyScopeGlobal,
                n,
            )?;
        }
        let sample_rate: f64 = get(
            device,
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
        )?;
        let buffer_frames: u32 = get(
            device,
            kAudioDevicePropertyBufferFrameSize,
            kAudioObjectPropertyScopeGlobal,
        )?;
        let info = StreamInfo {
            device_name: name_of(device).unwrap_or_default(),
            sample_rate,
            buffer_frames,
            channels,
        };
        self.device = Some(device);
        self.info = Some(info.clone());
        self.listen(device, cfg.device.is_none());
        Ok(info)
    }

    fn start(&mut self, renderer: Renderer) -> Result<(), BackendError> {
        let device = self.device.ok_or(BackendError::NoDevice)?;
        if self.running.is_some() {
            return Err(BackendError::Os {
                call: "start (already running)",
                status: -1,
            });
        }
        let max = renderer.config().max_frames.max(MAX_FRAMES) as usize;
        let ctx = Box::into_raw(Box::new(RtContext {
            renderer,
            left: vec![0.0; max].into_boxed_slice(),
            right: vec![0.0; max].into_boxed_slice(),
            poisoned: false,
        }));
        let mut proc_id: AudioDeviceIOProcID = None;
        // SAFETY: `io_proc` matches the IOProc signature; `ctx` stays valid
        // until `stop` destroys the IOProc and reclaims it.
        let created = unsafe {
            AudioDeviceCreateIOProcID(
                device,
                Some(io_proc),
                ctx.cast::<c_void>(),
                NonNull::from(&mut proc_id),
            )
        };
        if let Err(e) = os("AudioDeviceCreateIOProcID", created) {
            // SAFETY: the IOProc was not created, so we still own `ctx`.
            drop(unsafe { Box::from_raw(ctx) });
            return Err(e);
        }
        // SAFETY: `proc_id` was just created on `device`.
        let started = unsafe { AudioDeviceStart(device, proc_id) };
        if let Err(e) = os("AudioDeviceStart", started) {
            // SAFETY: the device never ran the IOProc; destroy it and take
            // `ctx` back.
            unsafe {
                AudioDeviceDestroyIOProcID(device, proc_id);
                drop(Box::from_raw(ctx));
            }
            return Err(e);
        }
        self.running = Some(Running { proc_id, ctx });
        Ok(())
    }

    fn stop(&mut self) -> Result<Renderer, BackendError> {
        let device = self.device.ok_or(BackendError::NoDevice)?;
        let run = self.running.take().ok_or(BackendError::NoDevice)?;
        // SAFETY: after `AudioDeviceStop` returns the IOProc no longer runs,
        // so after destroying it we own `ctx` again.
        let ctx = unsafe {
            AudioDeviceStop(device, run.proc_id);
            AudioDeviceDestroyIOProcID(device, run.proc_id);
            Box::from_raw(run.ctx)
        };
        Ok(ctx.renderer)
    }

    fn take_events(&mut self) -> Vec<BackendEvent> {
        let mut ev = Vec::new();
        let (Some(l), Some(device)) = (self.listening.as_ref(), self.device) else {
            return ev;
        };
        // SAFETY: `flags` lives until `unlisten`.
        let flags = unsafe { &*l.flags };
        if flags.dead.swap(false, Ordering::Relaxed) {
            let alive: u32 = get(
                device,
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            )
            .unwrap_or(0);
            if alive == 0 {
                ev.push(BackendEvent::DeviceLost);
            }
        }
        if flags.default_changed.swap(false, Ordering::Relaxed) {
            let now: AudioObjectID = get(
                SYSTEM,
                kAudioHardwarePropertyDefaultOutputDevice,
                kAudioObjectPropertyScopeGlobal,
            )
            .unwrap_or(0);
            if now != device {
                ev.push(BackendEvent::DefaultDeviceChanged);
            }
        }
        if flags.rate_changed.swap(false, Ordering::Relaxed) {
            if let Ok(rate) = get::<f64>(
                device,
                kAudioDevicePropertyNominalSampleRate,
                kAudioObjectPropertyScopeGlobal,
            ) {
                if self.info.as_ref().is_some_and(|i| i.sample_rate != rate) {
                    if let Some(i) = self.info.as_mut() {
                        i.sample_rate = rate;
                    }
                    ev.push(BackendEvent::SampleRateChanged(rate));
                }
            }
        }
        if flags.buffer_changed.swap(false, Ordering::Relaxed) {
            if let Ok(n) = get::<u32>(
                device,
                kAudioDevicePropertyBufferFrameSize,
                kAudioObjectPropertyScopeGlobal,
            ) {
                if self.info.as_ref().is_some_and(|i| i.buffer_frames != n) {
                    if let Some(i) = self.info.as_mut() {
                        i.buffer_frames = n;
                    }
                    ev.push(BackendEvent::BufferSizeChanged(n));
                }
            }
        }
        ev
    }
}

impl Drop for CoreAudioBackend {
    fn drop(&mut self) {
        drop(self.close());
    }
}
