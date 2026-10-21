//! CoreAudio HAL output and input through an IOProc on the device's own
//! real-time thread (no AUHAL). Input (sub-project 5 spec §6.1) comes from
//! the output device's own IOProc (one duplex stream) or, from another
//! device, through a private aggregate device with the output device as
//! clock and drift correction on the input; it is opened only when asked.

use core_engine::BackendError;
use core_engine::backend::{
    AudioBackend, AudioConfig, BackendEvent, BlockTime, InputBlock, MAX_FRAMES, MAX_INPUT_CHANNELS,
    StreamInfo,
};
use core_engine::pool::RtWorkgroup;
use core_engine::render::{Faults, Renderer};
use objc2_core_audio::{
    AudioConvertHostTimeToNanos, AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID,
    AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop, AudioGetCurrentHostTime,
    AudioHardwareCreateAggregateDevice, AudioHardwareDestroyAggregateDevice,
    AudioObjectAddPropertyListener, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize,
    AudioObjectID, AudioObjectPropertyAddress, AudioObjectPropertyElement,
    AudioObjectPropertyScope, AudioObjectPropertySelector, AudioObjectRemovePropertyListener,
    AudioObjectSetPropertyData, kAudioAggregateDeviceIsPrivateKey,
    kAudioAggregateDeviceMainSubDeviceKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceSubDeviceListKey, kAudioAggregateDeviceUIDKey,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyDeviceIsAlive,
    kAudioDevicePropertyDeviceUID, kAudioDevicePropertyIOThreadOSWorkgroup,
    kAudioDevicePropertyLatency, kAudioDevicePropertyNominalSampleRate,
    kAudioDevicePropertySafetyOffset, kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreams, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyElementName, kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    kAudioStreamPropertyLatency, kAudioStreamPropertyVirtualFormat,
    kAudioSubDeviceDriftCompensationKey, kAudioSubDeviceUIDKey,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    kAudioFormatFlagIsFloat, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType};
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
    addr_at(selector, scope, kAudioObjectPropertyElementMain)
}

fn addr_at(
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    element: AudioObjectPropertyElement,
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: element,
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
    string_prop(
        obj,
        addr(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal),
    )
}

/// A device's persistent UID (what an aggregate device names it by).
fn uid_of(obj: AudioObjectID) -> Option<String> {
    string_prop(
        obj,
        addr(
            kAudioDevicePropertyDeviceUID,
            kAudioObjectPropertyScopeGlobal,
        ),
    )
}

/// A CFString property.
fn string_prop(obj: AudioObjectID, mut a: AudioObjectPropertyAddress) -> Option<String> {
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
    layout(dev, kAudioObjectPropertyScopeOutput)
}

/// Channel counts of the device's buffers in `scope`, in order.
fn layout(dev: AudioObjectID, scope: AudioObjectPropertyScope) -> Result<Vec<u32>, BackendError> {
    let words = get_bytes(dev, kAudioDevicePropertyStreamConfiguration, scope)?;
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
    check_scope_formats(dev, kAudioObjectPropertyScopeOutput)
}

/// Every stream of `scope` must be 32-bit float linear PCM.
fn check_scope_formats(
    dev: AudioObjectID,
    scope: AudioObjectPropertyScope,
) -> Result<(), BackendError> {
    for stream in get_ids(dev, kAudioDevicePropertyStreams, scope)? {
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

/// The latency of `dev` in `scope`, in frames: device latency + safety
/// offset + the first stream's latency (spec §6.1). Unknown parts count
/// 0.
fn latency(dev: AudioObjectID, scope: AudioObjectPropertyScope) -> u32 {
    let device: u32 = get(dev, kAudioDevicePropertyLatency, scope).unwrap_or(0);
    let safety: u32 = get(dev, kAudioDevicePropertySafetyOffset, scope).unwrap_or(0);
    let stream: u32 = get_ids(dev, kAudioDevicePropertyStreams, scope)
        .ok()
        .and_then(|v| v.first().copied())
        .and_then(|st| {
            get(
                st,
                kAudioStreamPropertyLatency,
                kAudioObjectPropertyScopeGlobal,
            )
            .ok()
        })
        .unwrap_or(0);
    device.saturating_add(safety).saturating_add(stream)
}

/// The names of `dev`'s first `n` input channels (the device's element
/// names; "Input k" where it has none).
fn input_names(dev: AudioObjectID, n: u32) -> Vec<String> {
    (1..=n)
        .map(|k| {
            string_prop(
                dev,
                addr_at(
                    kAudioObjectPropertyElementName,
                    kAudioObjectPropertyScopeInput,
                    k,
                ),
            )
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Input {k}"))
        })
        .collect()
}

/// A device by name.
fn find_device(name: &str) -> Result<AudioObjectID, BackendError> {
    get_ids(
        SYSTEM,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )?
    .into_iter()
    .find(|d| name_of(*d).as_deref() == Some(name))
    .ok_or(BackendError::NoDevice)
}

/// The description of a private aggregate device (spec §6.1) named
/// `uid`: `output` first and as its clock, `input` drift-corrected.
pub fn aggregate_description(
    uid: &str,
    output: &str,
    input: &str,
) -> CFRetained<CFDictionary<CFString, CFType>> {
    let key = |k: &std::ffi::CStr| CFString::from_str(&k.to_string_lossy());
    let sub = |device: &str, drift: bool| {
        let keys = [
            key(kAudioSubDeviceUIDKey),
            key(kAudioSubDeviceDriftCompensationKey),
        ];
        let uid = CFString::from_str(device);
        let drift = CFNumber::new_i32(i32::from(drift));
        let values: [&CFType; 2] = [uid.as_ref(), drift.as_ref()];
        let keys: [&CFString; 2] = [&keys[0], &keys[1]];
        CFDictionary::<CFString, CFType>::from_slices(&keys, &values)
    };
    let (out, inp) = (sub(output, false), sub(input, true));
    let list = CFArray::<CFType>::from_objects(&[(*out).as_ref(), (*inp).as_ref()]);
    let keys = [
        key(kAudioAggregateDeviceUIDKey),
        key(kAudioAggregateDeviceNameKey),
        key(kAudioAggregateDeviceIsPrivateKey),
        key(kAudioAggregateDeviceMainSubDeviceKey),
        key(kAudioAggregateDeviceSubDeviceListKey),
    ];
    let (u, name, private, main) = (
        CFString::from_str(uid),
        CFString::from_str("VizDAW Input"),
        CFNumber::new_i32(1),
        CFString::from_str(output),
    );
    let values: [&CFType; 5] = [
        u.as_ref(),
        name.as_ref(),
        private.as_ref(),
        main.as_ref(),
        (*list).as_ref(),
    ];
    let keys: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
    CFDictionary::<CFString, CFType>::from_slices(&keys, &values)
}

/// Creates the private aggregate device of `output` and `input`.
fn create_aggregate(
    output: AudioObjectID,
    input: AudioObjectID,
) -> Result<AudioObjectID, BackendError> {
    let (Some(out_uid), Some(in_uid)) = (uid_of(output), uid_of(input)) else {
        return Err(BackendError::NoDevice);
    };
    let uid = format!("org.vizdaw.input.{}", std::process::id());
    let desc = aggregate_description(&uid, &out_uid, &in_uid);
    let mut id: AudioObjectID = 0;
    // SAFETY: `desc` is a valid dictionary of CFString keys for the call
    // and `id` a valid out-pointer.
    os("AudioHardwareCreateAggregateDevice", unsafe {
        AudioHardwareCreateAggregateDevice(desc.as_opaque(), NonNull::from(&mut id))
    })?;
    if id == 0 {
        return Err(BackendError::NoDevice);
    }
    Ok(id)
}

/// When a block's first frame is heard: the output time CoreAudio gives
/// plus the output latency (spec §6.3), so MIDI scheduled against it
/// leaves at audible time.
pub fn audible_ns(out_ns: u64, output_latency: u32, rate: f64) -> u64 {
    let lat = (f64::from(output_latency) * 1e9 / rate.max(1.0)).round() as u64;
    out_ns.saturating_add(lat)
}

/// How much earlier than the output buffer's first frame is heard the
/// input buffer's first frame was captured (spec §6.3): their sample-time
/// difference plus both latencies.
pub fn input_delay(
    out_sample: f64,
    in_sample: f64,
    output_latency: u32,
    input_latency: u32,
) -> i64 {
    let gap = if out_sample.is_finite() && in_sample.is_finite() {
        (out_sample - in_sample).round() as i64
    } else {
        0
    };
    gap.saturating_add(i64::from(output_latency))
        .saturating_add(i64::from(input_latency))
}

/// Copies input channels `offset..offset + inputs.len()` (counted across
/// every buffer of `list`) into the planar `inputs`, `frames` frames; the
/// rest of each is left alone. // RT
///
/// # Safety
///
/// As for [`buffers`].
unsafe fn deinterleave(
    list: NonNull<AudioBufferList>,
    offset: usize,
    inputs: &mut [Box<[f32]>],
    frames: usize,
) {
    // SAFETY: forwarded from the caller.
    let bufs = unsafe { buffers(list) };
    let mut global = 0usize;
    for b in bufs.iter() {
        let nch = b.mNumberChannels.max(1) as usize;
        if b.mData.is_null() {
            global += nch;
            continue;
        }
        let samples = b.mDataByteSize as usize / 4;
        // SAFETY: `mData` holds `mDataByteSize` bytes of f32 samples.
        let data = unsafe { std::slice::from_raw_parts(b.mData.cast::<f32>(), samples) };
        for c in 0..nch {
            let Some(dst) = (global + c)
                .checked_sub(offset)
                .and_then(|k| inputs.get_mut(k))
            else {
                continue;
            };
            for (f, d) in dst.iter_mut().take(frames).enumerate() {
                *d = data.get(f * nch + c).copied().unwrap_or(0.0);
            }
        }
        global += nch;
    }
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

/// Names of every device with input channels.
pub fn input_devices() -> Vec<String> {
    get_ids(
        SYSTEM,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
    )
    .unwrap_or_default()
    .into_iter()
    .filter(|d| layout(*d, kAudioObjectPropertyScopeInput).is_ok_and(|l| l.iter().sum::<u32>() > 0))
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
    // kept alive for the backend's lifetime (`retired`); `addresses` holds
    // `count` entries.
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
    /// Planar input, one buffer per input channel (none: input closed).
    inputs: Box<[Box<[f32]>]>,
    /// The first device input channel this stream's input carries (an
    /// aggregate puts the output device's inputs first).
    input_offset: usize,
    output_latency: u32,
    input_latency: u32,
}

/// The `mNumberBuffers` buffers of `list`. // RT
///
/// # Safety
///
/// `list` must point at a valid `AudioBufferList` followed by its
/// `mNumberBuffers` buffers, all valid and not otherwise accessed for `'a`.
unsafe fn buffers<'a>(list: NonNull<AudioBufferList>) -> &'a mut [AudioBuffer] {
    let l = list.as_ptr();
    // SAFETY: the caller guarantees `list` is valid. The buffers trail the
    // header beyond the declared one-element `mBuffers` array, so the slice
    // pointer is derived from `list` itself with `addr_of_mut!` (raw-pointer
    // provenance over the whole list) rather than through a
    // `&mut AudioBufferList`, whose provenance would cover only the first
    // buffer.
    unsafe {
        let n = ptr::read(ptr::addr_of!((*l).mNumberBuffers)) as usize;
        let first = ptr::addr_of_mut!((*l).mBuffers).cast::<AudioBuffer>();
        std::slice::from_raw_parts_mut(first, n)
    }
}

/// Zeroes every output buffer. // RT
///
/// # Safety
///
/// As for [`buffers`].
unsafe fn silence(out: NonNull<AudioBufferList>) {
    // SAFETY: forwarded from the caller.
    let bufs = unsafe { buffers(out) };
    for b in bufs {
        if !b.mData.is_null() {
            // SAFETY: `mData` holds `mDataByteSize` bytes.
            unsafe { ptr::write_bytes(b.mData.cast::<u8>(), 0, b.mDataByteSize as usize) };
        }
    }
}

/// Renders planar stereo and writes it to the device's buffers:
/// global channel 0 = left, 1 = right, others zero. // RT
///
/// # Safety
///
/// As for [`buffers`].
unsafe fn render(
    ctx: &mut RtContext,
    out: NonNull<AudioBufferList>,
    time: &AudioTimeStamp,
    input: NonNull<AudioBufferList>,
    input_time: &AudioTimeStamp,
) {
    // SAFETY: forwarded from the caller (CoreAudio hands the IOProc
    // `mNumberBuffers` trailing buffers, valid for the call).
    let bufs = unsafe { buffers(out) };
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
    let valid = AudioTimeStampFlags::SampleTimeValid;
    let delay = if time.mFlags.contains(valid) && input_time.mFlags.contains(valid) {
        input_delay(
            time.mSampleTime,
            input_time.mSampleTime,
            ctx.output_latency,
            ctx.input_latency,
        )
    } else {
        i64::from(ctx.output_latency) + i64::from(ctx.input_latency)
    };
    if !ctx.inputs.is_empty() {
        // SAFETY: CoreAudio's input list is valid for this call.
        unsafe { deinterleave(input, ctx.input_offset, &mut ctx.inputs, len) };
    }
    let mut ins: [&[f32]; MAX_INPUT_CHANNELS] = [&[]; MAX_INPUT_CHANNELS];
    for (slot, buf) in ins.iter_mut().zip(ctx.inputs.iter()) {
        *slot = buf.get(..len).unwrap_or(&[]);
    }
    let n_in = ctx.inputs.len().min(MAX_INPUT_CHANNELS);
    ctx.renderer.process_input(
        &mut chans,
        frames,
        BlockTime {
            // MIDI leaves at the time this block is heard (spec §6.3).
            host_time_ns: audible_ns(host_ns, ctx.output_latency, sr),
            sample_rate: sr,
        },
        InputBlock {
            channels: ins.get(..n_in).unwrap_or(&[]),
            delay_frames: delay,
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
    input: NonNull<AudioBufferList>,
    input_time: NonNull<AudioTimeStamp>,
    output: NonNull<AudioBufferList>,
    output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    // SAFETY: `client` is the `RtContext` boxed in `start`, owned by this
    // IOProc until `stop` has returned from `AudioDeviceStop`; the buffer
    // lists and timestamps are valid for this call. The buffer lists stay
    // raw pointers (see `buffers`). // RT
    let (ctx, time, in_time) = unsafe {
        (
            &mut *client.cast::<RtContext>(),
            output_time.as_ref(),
            input_time.as_ref(),
        )
    };
    if ctx.poisoned {
        // SAFETY: CoreAudio's output list is valid for this call.
        unsafe { silence(output) };
        return 0;
    }
    // SAFETY: as above; `render` is the only user of the lists meanwhile.
    let ok = catch_unwind(AssertUnwindSafe(|| unsafe {
        render(ctx, output, time, input, in_time)
    }))
    .is_ok();
    if !ok {
        // Never unwind into CoreAudio: go silent for good and report.
        ctx.poisoned = true;
        ctx.renderer.raise(Faults::PANIC);
        // SAFETY: as above; the panicked `render` no longer uses the list.
        unsafe { silence(output) };
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
    /// The private aggregate device made for a separate input device
    /// (destroyed on close).
    aggregate: Option<AudioObjectID>,
    /// The input device inside it, also listened to (unplugging it ends a
    /// recording).
    input_device: Option<AudioObjectID>,
    /// The first device input channel the stream carries.
    input_offset: usize,
    /// The output device (the aggregate's clock when there is one): what
    /// following the system default compares.
    output: Option<AudioObjectID>,
    /// Flags of removed listeners. CoreAudio may still be running a
    /// listener call on its notification thread while
    /// `AudioObjectRemovePropertyListener` returns, so the flags live as
    /// long as the backend.
    retired: Vec<Arc<Flags>>,
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
            aggregate: None,
            input_device: None,
            input_offset: 0,
            output: None,
            retired: Vec::new(),
        }
    }

    /// What `open` found.
    pub fn info(&self) -> Option<&StreamInfo> {
        self.info.as_ref()
    }

    /// The open device's IO-thread workgroup, for render workers to join
    /// (spec §7); `None` when closed or unsupported.
    pub fn workgroup(&self) -> Option<Arc<dyn RtWorkgroup>> {
        let device = self.device?;
        let raw: usize = get(
            device,
            kAudioDevicePropertyIOThreadOSWorkgroup,
            kAudioObjectPropertyScopeGlobal,
        )
        .ok()?;
        // SAFETY: the HAL returns a retained `os_workgroup_t` that "the
        // caller is responsible for releasing" (AudioHardware.h);
        // `OsWorkgroup` releases it on drop.
        let wg = unsafe { crate::rt::OsWorkgroup::from_owned(raw as *mut c_void) }?;
        Some(Arc::new(wg))
    }

    fn listen(&mut self, device: AudioObjectID, follow_default: bool) {
        let flags = Arc::into_raw(Arc::new(Flags::default()));
        if let Some(input) = self.input_device {
            let mut a = addr(
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            );
            // SAFETY: as below; removed in `unlisten`.
            unsafe {
                AudioObjectAddPropertyListener(
                    input,
                    NonNull::from(&mut a),
                    Some(listener),
                    flags.cast_mut().cast::<c_void>(),
                );
            }
        }
        for (selector, system) in LISTENED {
            if system && !follow_default {
                continue;
            }
            let obj = if system { SYSTEM } else { device };
            let mut a = addr(selector, kAudioObjectPropertyScopeGlobal);
            // SAFETY: `flags` stays alive for the backend's lifetime (see
            // `retired`).
            let status = unsafe {
                AudioObjectAddPropertyListener(
                    obj,
                    NonNull::from(&mut a),
                    Some(listener),
                    flags.cast_mut().cast::<c_void>(),
                )
            };
            if status != 0 {
                eprintln!(
                    "warning: AudioObjectAddPropertyListener({selector:#x}) failed ({status}); \
                     device changes may go unnoticed"
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
        if let Some(input) = self.input_device {
            let mut a = addr(
                kAudioDevicePropertyDeviceIsAlive,
                kAudioObjectPropertyScopeGlobal,
            );
            // SAFETY: removes exactly the listener added in `listen`.
            unsafe {
                AudioObjectRemovePropertyListener(
                    input,
                    NonNull::from(&mut a),
                    Some(listener),
                    l.flags.cast_mut().cast::<c_void>(),
                );
            }
        }
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
        // SAFETY: `l.flags` came from `Arc::into_raw` in `listen` and is
        // taken back exactly once; the Arc is kept (not freed) in case a
        // listener call is still in flight.
        self.retired.push(unsafe { Arc::from_raw(l.flags) });
    }

    /// Stops (if running) and forgets the device; an aggregate device made
    /// for input is destroyed.
    pub fn close(&mut self) -> Option<Renderer> {
        let r = self.stop().ok();
        self.unlisten();
        if let Some(agg) = self.aggregate.take() {
            // SAFETY: the aggregate was made by this backend and nothing
            // runs on it any more.
            unsafe {
                AudioHardwareDestroyAggregateDevice(agg);
            }
        }
        self.input_device = None;
        self.input_offset = 0;
        self.output = None;
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
        let outputs = output_layout(device)?;
        let channels: u32 = outputs.iter().sum();
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
        if let Some(want) = cfg.sample_rate {
            let now: f64 = get(
                device,
                kAudioDevicePropertyNominalSampleRate,
                kAudioObjectPropertyScopeGlobal,
            )?;
            if (now - want).abs() > 0.5 {
                // The device may refuse or take a moment: ask, then wait up
                // to 1 s for it to report the new rate (5a Ruling 20). A
                // refused request is not waited for (Task 11 Minor 1): the
                // rate read below reports the mismatch at once.
                let asked = set(
                    device,
                    kAudioDevicePropertyNominalSampleRate,
                    kAudioObjectPropertyScopeGlobal,
                    want,
                )
                .is_ok();
                settle_rate(asked, want, std::time::Duration::from_secs(1), || {
                    get::<f64>(
                        device,
                        kAudioDevicePropertyNominalSampleRate,
                        kAudioObjectPropertyScopeGlobal,
                    )
                })?;
            }
        }
        // Input (spec §6.1), once the output runs at its rate: the output
        // device's own, or another device's through a private aggregate
        // (the output device first, as its clock), whose inputs list the
        // output device's before the input device's.
        let output = device;
        let mut device = device;
        let mut input_source = None;
        if cfg.input_enabled {
            match &cfg.input_device {
                Some(name) if name_of(output).as_deref() != Some(name.as_str()) => {
                    let input = find_device(name)?;
                    // Pre-flight M15: the input device runs at the
                    // output's rate (the project's, unless the output
                    // refused it) before the aggregate joins them, or
                    // input does not open.
                    let rate: f64 = get(
                        output,
                        kAudioDevicePropertyNominalSampleRate,
                        kAudioObjectPropertyScopeGlobal,
                    )?;
                    match_rate(
                        &mut HalRate(input),
                        name,
                        rate,
                        std::time::Duration::from_secs(1),
                    )?;
                    let agg = create_aggregate(output, input)?;
                    self.input_offset = layout(output, kAudioObjectPropertyScopeInput)
                        .map(|l| l.iter().sum::<u32>() as usize)
                        .unwrap_or(0);
                    self.aggregate = Some(agg);
                    self.input_device = Some(input);
                    input_source = Some(input);
                    device = agg;
                }
                _ => input_source = Some(output),
            }
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
        let input_channels = match input_source {
            Some(src) => {
                let n = layout(src, kAudioObjectPropertyScopeInput)
                    .map(|l| l.iter().sum::<u32>())
                    .unwrap_or(0)
                    .min(MAX_INPUT_CHANNELS as u32);
                if n > 0 {
                    check_scope_formats(device, kAudioObjectPropertyScopeInput)?;
                }
                n
            }
            None => 0,
        };
        let info = StreamInfo {
            device_name: name_of(output).unwrap_or_default(),
            sample_rate,
            buffer_frames,
            channels,
            input_channels,
            input_channel_names: input_source
                .map(|src| input_names(src, input_channels))
                .unwrap_or_default(),
            input_latency_frames: if input_channels > 0 {
                latency(device, kAudioObjectPropertyScopeInput)
            } else {
                0
            },
            output_latency_frames: latency(device, kAudioObjectPropertyScopeOutput),
        };
        self.device = Some(device);
        self.output = Some(output);
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
        let info = self.info.clone();
        let inputs = info.as_ref().map_or(0, |i| i.input_channels as usize);
        let ctx = Box::into_raw(Box::new(RtContext {
            renderer,
            left: vec![0.0; max].into_boxed_slice(),
            right: vec![0.0; max].into_boxed_slice(),
            poisoned: false,
            inputs: (0..inputs)
                .map(|_| vec![0.0; max].into_boxed_slice())
                .collect(),
            input_offset: self.input_offset,
            output_latency: info.as_ref().map_or(0, |i| i.output_latency_frames),
            input_latency: info.as_ref().map_or(0, |i| i.input_latency_frames),
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
            // SAFETY: the device never ran the IOProc; once it is destroyed
            // we own `ctx` again. If destroying fails, `ctx` is leaked.
            unsafe {
                if AudioDeviceDestroyIOProcID(device, proc_id) == 0 {
                    drop(Box::from_raw(ctx));
                }
            }
            return Err(e);
        }
        self.running = Some(Running { proc_id, ctx });
        Ok(())
    }

    fn stop(&mut self) -> Result<Renderer, BackendError> {
        let device = self.device.ok_or(BackendError::NoDevice)?;
        let run = self.running.take().ok_or(BackendError::NoDevice)?;
        // SAFETY: the IOProc ID belongs to `device`.
        let destroyed = unsafe {
            AudioDeviceStop(device, run.proc_id);
            AudioDeviceDestroyIOProcID(device, run.proc_id)
        };
        // If the IOProc could not be destroyed it may still reference
        // `ctx`: leak it (and the renderer) rather than free it.
        os("AudioDeviceDestroyIOProcID", destroyed)?;
        // SAFETY: the IOProc is destroyed, so we own `ctx` again.
        let ctx = unsafe { Box::from_raw(run.ctx) };
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
            // The input device inside an aggregate counts too: the
            // aggregate may outlive it.
            let alive = |d: AudioObjectID| {
                get::<u32>(
                    d,
                    kAudioDevicePropertyDeviceIsAlive,
                    kAudioObjectPropertyScopeGlobal,
                )
                .unwrap_or(0)
                    != 0
            };
            if !alive(device) || self.input_device.is_some_and(|d| !alive(d)) {
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
            if now != self.output.unwrap_or(device) {
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

/// A device's nominal rate, read and asked for (the seam the rate
/// decision of [`match_rate`] is tested through).
trait NominalRate {
    /// The rate it runs at now.
    fn rate(&mut self) -> Result<f64, BackendError>;
    /// Asks it for `want`; false when it refused at once.
    fn ask(&mut self, want: f64) -> bool;
}

/// A HAL device's nominal rate.
struct HalRate(AudioObjectID);

impl NominalRate for HalRate {
    fn rate(&mut self) -> Result<f64, BackendError> {
        get(
            self.0,
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
        )
    }

    fn ask(&mut self, want: f64) -> bool {
        set(
            self.0,
            kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal,
            want,
        )
        .is_ok()
    }
}

/// Brings input device `name` to `want` Hz (pre-flight M15): asked only
/// when it runs at another rate, then waited for up to `timeout` (not at
/// all when it refused); an error when it still runs at another rate.
fn match_rate(
    dev: &mut impl NominalRate,
    name: &str,
    want: f64,
    timeout: std::time::Duration,
) -> Result<(), BackendError> {
    let same = |r: f64| (r - want).abs() <= 0.5;
    if same(dev.rate()?) {
        return Ok(());
    }
    let asked = dev.ask(want);
    settle_rate(asked, want, timeout, || dev.rate())?;
    let now = dev.rate()?;
    if same(now) {
        return Ok(());
    }
    Err(BackendError::Format {
        detail: format!(
            "the input device {name} runs at {now} Hz and did not change to the project's {want} Hz"
        ),
    })
}

/// After asking a device for rate `want`: when it accepted (`asked`),
/// polls `read` every 20 ms, up to `timeout`, until it reports `want`;
/// when it refused, returns at once (nothing will change).
fn settle_rate<E>(
    asked: bool,
    want: f64,
    timeout: std::time::Duration,
    mut read: impl FnMut() -> Result<f64, E>,
) -> Result<(), E> {
    if !asked {
        return Ok(());
    }
    let until = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < until {
        if (read()? - want).abs() <= 0.5 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `AudioBufferList` with two trailing buffers, laid out as
    /// CoreAudio does.
    #[repr(C)]
    struct TwoBuffers {
        n: u32,
        bufs: [AudioBuffer; 2],
    }

    #[test]
    fn silence_reaches_every_trailing_buffer() {
        let mut a = [1.0f32; 8];
        let mut b = [1.0f32; 8];
        let buf = |d: &mut [f32; 8]| AudioBuffer {
            mNumberChannels: 1,
            mDataByteSize: 32,
            mData: d.as_mut_ptr().cast(),
        };
        let mut list = TwoBuffers {
            n: 2,
            bufs: [buf(&mut a), buf(&mut b)],
        };
        let p = NonNull::from(&mut list).cast::<AudioBufferList>();
        // SAFETY: `list` is a valid two-buffer list for the call.
        unsafe { silence(p) };
        assert_eq!(a, [0.0; 8]);
        assert_eq!(b, [0.0; 8]);
    }

    /// Plan 5c Task 11 (spec §6.3): MIDI leaves at the time a block is
    /// heard, the output latency after CoreAudio's output time.
    #[test]
    fn a_block_is_heard_one_output_latency_later() {
        assert_eq!(audible_ns(1_000_000, 0, 48_000.0), 1_000_000);
        assert_eq!(audible_ns(1_000_000, 480, 48_000.0), 11_000_000);
        assert_eq!(audible_ns(u64::MAX - 1, 480, 48_000.0), u64::MAX);
    }

    /// Spec §6.3: the input buffer's first frame was captured the
    /// sample-time difference plus both latencies before the output
    /// buffer's first frame is heard.
    #[test]
    fn the_input_delay_adds_both_latencies_to_the_buffer_gap() {
        assert_eq!(input_delay(1000.0, 744.0, 100, 50), 256 + 150);
        assert_eq!(input_delay(500.0, 500.0, 0, 0), 0);
        assert_eq!(input_delay(f64::NAN, 1.0, 10, 20), 30);
    }

    /// The input channels after `offset`, across every buffer, land in
    /// the planar buffers.
    #[test]
    fn input_buffers_are_split_into_channels_from_the_offset() {
        // Buffer 0: two interleaved channels; buffer 1: one channel.
        let mut a = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut b = [10.0f32, 20.0, 30.0];
        let list = TwoBuffers {
            n: 2,
            bufs: [
                AudioBuffer {
                    mNumberChannels: 2,
                    mDataByteSize: 24,
                    mData: a.as_mut_ptr().cast(),
                },
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: 12,
                    mData: b.as_mut_ptr().cast(),
                },
            ],
        };
        let mut list = list;
        let p = NonNull::from(&mut list).cast::<AudioBufferList>();
        let mut inputs: Box<[Box<[f32]>]> =
            (0..2).map(|_| vec![0.0; 3].into_boxed_slice()).collect();
        // The aggregate's first input (buffer 0, channel 0) belongs to the
        // output device: offset 1.
        // SAFETY: `list` is a valid two-buffer list for the call.
        unsafe { deinterleave(p, 1, &mut inputs, 3) };
        assert_eq!(&*inputs[0], &[2.0, 4.0, 6.0]);
        assert_eq!(&*inputs[1], &[10.0, 20.0, 30.0]);
    }

    /// The aggregate's description names both devices, is private and
    /// clocked by the output.
    #[test]
    fn the_aggregate_is_private_and_clocked_by_the_output() {
        let d = aggregate_description("org.vizdaw.input.1", "OUT-UID", "IN-UID");
        assert_eq!(d.len(), 5);
        let get = |k: &str| d.get(&CFString::from_str(k)).unwrap();
        let private = get("private");
        assert_eq!(
            private.downcast_ref::<CFNumber>().unwrap().as_i64(),
            Some(1)
        );
        let main = get("master");
        assert_eq!(
            main.downcast_ref::<CFString>().unwrap().to_string(),
            "OUT-UID"
        );
        let subs = get("subdevices");
        assert_eq!(subs.downcast_ref::<CFArray>().unwrap().len(), 2);
    }

    /// Task 11 Minor 1: a device that refused the rate is not polled (no
    /// 1 s wait); one that accepted it is polled until it reports it.
    #[test]
    fn a_refused_rate_is_not_waited_for() {
        let mut reads = 0;
        let t0 = std::time::Instant::now();
        let r = settle_rate(false, 48_000.0, std::time::Duration::from_secs(1), || {
            reads += 1;
            Ok::<f64, ()>(44_100.0)
        });
        assert_eq!((r, reads), (Ok(()), 0));
        assert!(t0.elapsed() < std::time::Duration::from_millis(100));
        let mut now = 44_100.0;
        let r = settle_rate(true, 48_000.0, std::time::Duration::from_secs(1), || {
            reads += 1;
            let r = now;
            now = 48_000.0;
            Ok::<f64, ()>(r)
        });
        assert_eq!((r, reads), (Ok(()), 2));
    }

    /// A device standing in for the input device's nominal rate: `accept`
    /// says whether a request is taken, `lag` how many reads it takes to
    /// report the new rate.
    struct FakeRate {
        now: f64,
        accept: bool,
        lag: u32,
        pending: Option<f64>,
        asked: Vec<f64>,
    }

    impl FakeRate {
        fn new(now: f64, accept: bool, lag: u32) -> FakeRate {
            FakeRate {
                now,
                accept,
                lag,
                pending: None,
                asked: Vec::new(),
            }
        }
    }

    impl NominalRate for FakeRate {
        fn rate(&mut self) -> Result<f64, BackendError> {
            if let Some(p) = self.pending {
                if self.lag == 0 {
                    self.now = p;
                    self.pending = None;
                } else {
                    self.lag -= 1;
                }
            }
            Ok(self.now)
        }

        fn ask(&mut self, want: f64) -> bool {
            self.asked.push(want);
            if self.accept {
                self.pending = Some(want);
            }
            self.accept
        }
    }

    /// Pre-flight M15: an input device already at the rate is not asked;
    /// one at another rate is asked once and waited for; one that refuses
    /// (or never changes) fails, naming both rates.
    #[test]
    fn the_input_device_is_brought_to_the_rate_or_refused() {
        let t = std::time::Duration::from_millis(200);
        let mut same = FakeRate::new(48_000.0, true, 0);
        assert!(match_rate(&mut same, "Mic", 48_000.0, t).is_ok());
        assert!(same.asked.is_empty(), "a device at the rate is not asked");

        let mut slow = FakeRate::new(44_100.0, true, 2);
        assert!(match_rate(&mut slow, "Mic", 48_000.0, t).is_ok());
        assert_eq!(slow.asked, [48_000.0]);
        assert_eq!(slow.now, 48_000.0);

        let mut refuses = FakeRate::new(44_100.0, false, 0);
        let t0 = std::time::Instant::now();
        let e = match_rate(
            &mut refuses,
            "Mic",
            48_000.0,
            std::time::Duration::from_secs(5),
        )
        .unwrap_err()
        .to_string();
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(500),
            "a refusal is not waited for"
        );
        assert_eq!(refuses.asked, [48_000.0]);
        assert!(
            e.contains("Mic") && e.contains("44100") && e.contains("48000"),
            "{e}"
        );

        // Accepted, but never reports the rate within the wait.
        let mut stuck = FakeRate::new(44_100.0, true, u32::MAX);
        assert!(match_rate(&mut stuck, "Mic", 48_000.0, t).is_err());
    }
}
