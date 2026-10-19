//! Every `unsafe` call of the AU host: the AudioUnit C API (which also
//! drives AU v3 components through the system bridge, Plan Ruling 6),
//! CoreFoundation property lists for state, and the AU event listener for
//! UI gestures.

use block2::RcBlock;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, msg_send};
use objc2_audio_toolbox::{
    AUListenerDispose, AUParameterListenerRef, AUParameterSet, AURenderCallbackStruct,
    AudioComponent, AudioComponentCopyName, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentFlags, AudioComponentGetDescription, AudioComponentGetVersion,
    AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioComponentInstantiate,
    AudioComponentInstantiationOptions, AudioUnit, AudioUnitAddPropertyListener,
    AudioUnitCocoaViewInfo, AudioUnitEventType, AudioUnitGetParameter, AudioUnitGetProperty,
    AudioUnitGetPropertyInfo, AudioUnitInitialize, AudioUnitParameter, AudioUnitParameterInfo,
    AudioUnitParameterOptions, AudioUnitParameterUnit, AudioUnitPropertyID, AudioUnitRender,
    AudioUnitRenderActionFlags, AudioUnitReset, AudioUnitScope, AudioUnitSetParameter,
    AudioUnitSetProperty, AudioUnitUninitialize, HostCallbackInfo, MusicDeviceMIDIEvent,
    kAudioUnitProperty_ClassInfo, kAudioUnitProperty_CocoaUI, kAudioUnitProperty_ElementCount,
    kAudioUnitProperty_HostCallbacks, kAudioUnitProperty_Latency,
    kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitProperty_ParameterInfo,
    kAudioUnitProperty_ParameterList, kAudioUnitProperty_RequestViewController,
    kAudioUnitProperty_SetRenderCallback, kAudioUnitProperty_StreamFormat,
    kAudioUnitProperty_TailTime, kAudioUnitScope_Global,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    SMPTETime, SMPTETimeFlags, SMPTETimeType, kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM,
};
use objc2_core_foundation::{
    CFBundle, CFData, CFPropertyList, CFPropertyListCreateData, CFPropertyListCreateWithData,
    CFPropertyListFormat, CFRetained, CFRunLoop, CFString, CFURL, CGPoint, CGRect, CGSize,
    kCFRunLoopDefaultMode,
};
use std::ffi::CString;
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Most channels of one bus VizDAW renders (the graph is stereo).
pub(crate) const MAX_BUS_CHANNELS: usize = 2;

fn os(call: &str, status: i32) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("{call} failed with status {status}"))
    }
}

/// A component (a plugin "class"), as found by enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Component(AudioComponent);

// SAFETY: an `AudioComponent` is an immutable, process-wide registry handle;
// every function taking one is thread-safe.
unsafe impl Send for Component {}

/// What enumeration tells about a component.
#[derive(Debug, Clone)]
pub(crate) struct ComponentInfo {
    pub component: Component,
    pub kind: u32,
    pub subtype: u32,
    pub manufacturer: u32,
    pub name: String,
    pub version: u32,
    pub v3: bool,
}

fn cfstring(s: *const CFString) -> String {
    match NonNull::new(s.cast_mut()) {
        // SAFETY: the caller passes a CFString it owns (+1), released here.
        Some(p) => unsafe { CFRetained::from_raw(p) }.to_string(),
        None => String::new(),
    }
}

/// Every component whose type is one of `types`.
pub(crate) fn components(types: &[u32]) -> Vec<ComponentInfo> {
    let mut out = Vec::new();
    for &kind in types {
        let mut desc = AudioComponentDescription {
            componentType: kind,
            componentSubType: 0,
            componentManufacturer: 0,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        let mut comp: AudioComponent = ptr::null_mut();
        loop {
            // SAFETY: `desc` is a valid description; `comp` is null or a
            // component this loop got from the previous call.
            comp = unsafe { AudioComponentFindNext(comp, NonNull::from(&mut desc)) };
            if comp.is_null() {
                break;
            }
            let mut d = desc;
            let mut name: *const CFString = ptr::null();
            let mut version = 0u32;
            // SAFETY: `comp` is valid; the out-pointers live for the calls.
            let ok = unsafe {
                AudioComponentGetDescription(comp, NonNull::from(&mut d)) == 0
                    && AudioComponentCopyName(comp, NonNull::from(&mut name)) == 0
            };
            // SAFETY: as above.
            unsafe { AudioComponentGetVersion(comp, NonNull::from(&mut version)) };
            if !ok {
                continue;
            }
            out.push(ComponentInfo {
                component: Component(comp),
                kind: d.componentType,
                subtype: d.componentSubType,
                manufacturer: d.componentManufacturer,
                name: cfstring(name),
                version,
                v3: AudioComponentFlags(d.componentFlags)
                    .contains(AudioComponentFlags::IsV3AudioUnit),
            });
        }
    }
    out
}

/// The component with exactly these codes.
pub(crate) fn find(kind: u32, subtype: u32, manufacturer: u32) -> Option<ComponentInfo> {
    components(&[kind])
        .into_iter()
        .find(|c| c.subtype == subtype && c.manufacturer == manufacturer)
}

/// An open AudioUnit instance; disposed on drop.
pub(crate) struct Unit {
    unit: AudioUnit,
    /// Latency-change flag the property listener sets (its refcon).
    latency_flag: Option<Arc<AtomicBool>>,
}

// SAFETY: an AudioUnit may be used from several threads: the render thread
// renders while the main thread reads properties and parameters (the
// AudioUnit contract); `Unit` adds no unsynchronised state of its own.
unsafe impl Send for Unit {}
// SAFETY: as above.
unsafe impl Sync for Unit {}

impl Drop for Unit {
    fn drop(&mut self) {
        // SAFETY: the instance is owned here and nothing renders it any more
        // (the processor holding the other `Arc` is gone).
        unsafe {
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
        }
    }
}

/// Instantiates a v2 component synchronously (any thread, Plan Ruling 2).
pub(crate) fn instantiate_v2(c: Component) -> Result<Unit, String> {
    let mut unit: AudioUnit = ptr::null_mut();
    // SAFETY: `c` is a valid component and `unit` a valid out-pointer.
    os("AudioComponentInstanceNew", unsafe {
        AudioComponentInstanceNew(c.0, NonNull::from(&mut unit))
    })?;
    Ok(Unit {
        unit,
        latency_flag: None,
    })
}

/// Instantiates a component asynchronously (AU v3; out of process when
/// `out_of_process`) and waits up to `timeout` for it.
pub(crate) fn instantiate_async(
    c: Component,
    out_of_process: bool,
    timeout: Duration,
) -> Result<Unit, String> {
    let (tx, rx) = channel::<(usize, i32)>();
    // The completion may arrive after the wait gave up: it then owns the
    // instance and disposes it. The flag and the send happen under one lock,
    // so an instance is either received below or disposed, never lost.
    let state = Arc::new(Mutex::new(Completion {
        tx: Some(tx),
        abandoned: false,
    }));
    let completion = Arc::clone(&state);
    let done = RcBlock::new(move |unit: AudioUnit, status: i32| {
        let mut g = completion.lock().unwrap_or_else(PoisonError::into_inner);
        if g.abandoned {
            if !unit.is_null() {
                // SAFETY: the instance was just created for this request,
                // which was abandoned: nothing else refers to it.
                unsafe { AudioComponentInstanceDispose(unit) };
            }
            return;
        }
        if let Some(tx) = g.tx.take() {
            let _ = tx.send((unit as usize, status));
        }
    });
    let opts = if out_of_process {
        AudioComponentInstantiationOptions::LoadOutOfProcess
    } else {
        AudioComponentInstantiationOptions::LoadInProcess
    };
    // SAFETY: `c` is valid; the block is retained by the system until it is
    // called.
    unsafe { AudioComponentInstantiate(c.0, opts, &done) };
    let (unit, status) = match rx.recv_timeout(timeout) {
        Ok(v) => v,
        Err(_) => {
            let mut g = state.lock().unwrap_or_else(PoisonError::into_inner);
            g.abandoned = true;
            // It may have been sent between the timeout and the lock.
            rx.try_recv()
                .map_err(|_| "instantiation timed out".to_string())?
        }
    };
    if unit == 0 {
        os("AudioComponentInstantiate", status)?;
        return Err("AudioComponentInstantiate gave no instance".into());
    }
    if let Err(e) = os("AudioComponentInstantiate", status) {
        // SAFETY: an instance came with an error status; it is ours and
        // unused.
        unsafe { AudioComponentInstanceDispose(unit as AudioUnit) };
        return Err(e);
    }
    Ok(Unit {
        unit: unit as AudioUnit,
        latency_flag: None,
    })
}

/// What the v3 completion handler shares with the waiting loader.
struct Completion {
    tx: Option<Sender<(usize, i32)>>,
    abandoned: bool,
}

fn stream(sample_rate: f64, channels: u32) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: sample_rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat
            | kAudioFormatFlagIsPacked
            | kAudioFormatFlagIsNonInterleaved,
        mBytesPerPacket: 4,
        mFramesPerPacket: 1,
        mBytesPerFrame: 4,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 32,
        mReserved: 0,
    }
}

impl Unit {
    fn get<T: Copy>(
        &self,
        id: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: u32,
        zero: T,
    ) -> Option<T> {
        let mut v = zero;
        let mut size = size_of::<T>() as u32;
        // SAFETY: `v` has room for `size` bytes of the property's type, which
        // the caller names with `T`.
        let st = unsafe {
            AudioUnitGetProperty(
                self.unit,
                id,
                scope,
                element,
                NonNull::from(&mut v).cast::<c_void>(),
                NonNull::from(&mut size),
            )
        };
        (st == 0).then_some(v)
    }

    fn set<T>(
        &self,
        call: &str,
        id: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: u32,
        v: &T,
    ) -> Result<(), String> {
        // SAFETY: `v` outlives the call and its size matches the property's
        // type (named by the caller).
        os(call, unsafe {
            AudioUnitSetProperty(
                self.unit,
                id,
                scope,
                element,
                (v as *const T).cast::<c_void>(),
                size_of::<T>() as u32,
            )
        })
    }

    /// Buses in `scope` (input or output).
    pub(crate) fn element_count(&self, scope: AudioUnitScope) -> u32 {
        self.get(kAudioUnitProperty_ElementCount, scope, 0, 0u32)
            .unwrap_or(0)
    }

    /// Asks for `count` buses in `scope`; false when the unit refuses.
    pub(crate) fn set_element_count(&self, scope: AudioUnitScope, count: u32) -> bool {
        self.set(
            "AudioUnitSetProperty(ElementCount)",
            kAudioUnitProperty_ElementCount,
            scope,
            0,
            &count,
        )
        .is_ok()
    }

    /// Channels of bus `element` in `scope`, as the unit has it now.
    pub(crate) fn channels(&self, scope: AudioUnitScope, element: u32) -> u32 {
        self.get(
            kAudioUnitProperty_StreamFormat,
            scope,
            element,
            stream(0.0, 0),
        )
        .map_or(0, |f| f.mChannelsPerFrame)
    }

    /// Sets bus `element` of `scope` to non-interleaved f32.
    pub(crate) fn set_format(
        &self,
        scope: AudioUnitScope,
        element: u32,
        sample_rate: f64,
        channels: u32,
    ) -> Result<(), String> {
        self.set(
            "AudioUnitSetProperty(StreamFormat)",
            kAudioUnitProperty_StreamFormat,
            scope,
            element,
            &stream(sample_rate, channels),
        )
    }

    /// Largest block it will be asked for.
    pub(crate) fn set_max_frames(&self, frames: u32) -> Result<(), String> {
        self.set(
            "AudioUnitSetProperty(MaximumFramesPerSlice)",
            kAudioUnitProperty_MaximumFramesPerSlice,
            kAudioUnitScope_Global,
            0,
            &frames,
        )
    }

    /// Feeds input bus `element` from `callback` with `refcon`.
    pub(crate) fn set_input(&self, element: u32, refcon: *mut c_void) -> Result<(), String> {
        let cb = AURenderCallbackStruct {
            inputProc: Some(render_input),
            inputProcRefCon: refcon,
        };
        self.set(
            "AudioUnitSetProperty(SetRenderCallback)",
            kAudioUnitProperty_SetRenderCallback,
            objc2_audio_toolbox::kAudioUnitScope_Input,
            element,
            &cb,
        )
    }

    /// Allocates for rendering (activation).
    pub(crate) fn initialize(&self) -> Result<(), String> {
        // SAFETY: the unit is open and configured.
        os("AudioUnitInitialize", unsafe {
            AudioUnitInitialize(self.unit)
        })
    }

    /// Frees what `initialize` allocated (deactivation).
    pub(crate) fn uninitialize(&self) {
        // SAFETY: the unit is open; uninitializing twice is harmless.
        unsafe { AudioUnitUninitialize(self.unit) };
    }

    /// Clears its DSP state. // RT
    pub(crate) fn reset(&self) {
        // SAFETY: the unit is open. // RT
        unsafe { AudioUnitReset(self.unit, kAudioUnitScope_Global, 0) };
    }

    /// Latency in seconds.
    pub(crate) fn latency(&self) -> f64 {
        self.get(
            kAudioUnitProperty_Latency,
            kAudioUnitScope_Global,
            0,
            0.0f64,
        )
        .unwrap_or(0.0)
    }

    /// Tail in seconds.
    pub(crate) fn tail(&self) -> f64 {
        self.get(
            kAudioUnitProperty_TailTime,
            kAudioUnitScope_Global,
            0,
            0.0f64,
        )
        .unwrap_or(0.0)
    }

    /// Sets `flag` whenever the unit reports a latency change.
    pub(crate) fn watch_latency(&mut self, flag: Arc<AtomicBool>) -> Result<(), String> {
        let refcon = Arc::as_ptr(&flag).cast_mut().cast::<c_void>();
        // SAFETY: `refcon` points at the flag, which `self` keeps alive
        // (`latency_flag`) until the unit is disposed, which removes the
        // listener.
        os("AudioUnitAddPropertyListener", unsafe {
            AudioUnitAddPropertyListener(
                self.unit,
                kAudioUnitProperty_Latency,
                Some(latency_changed),
                refcon,
            )
        })?;
        self.latency_flag = Some(flag);
        Ok(())
    }

    /// Global parameter addresses.
    pub(crate) fn parameter_ids(&self) -> Vec<u32> {
        let mut size = 0u32;
        // SAFETY: `size` is a valid out-pointer; no data pointer is passed.
        let st = unsafe {
            AudioUnitGetPropertyInfo(
                self.unit,
                kAudioUnitProperty_ParameterList,
                kAudioUnitScope_Global,
                0,
                &mut size,
                ptr::null_mut(),
            )
        };
        if st != 0 || size == 0 {
            return Vec::new();
        }
        let mut ids = vec![0u32; size as usize / size_of::<u32>()];
        let mut size = (ids.len() * size_of::<u32>()) as u32;
        // SAFETY: `ids` has room for `size` bytes.
        let st = unsafe {
            AudioUnitGetProperty(
                self.unit,
                kAudioUnitProperty_ParameterList,
                kAudioUnitScope_Global,
                0,
                NonNull::new_unchecked(ids.as_mut_ptr().cast::<c_void>()),
                NonNull::from(&mut size),
            )
        };
        if st != 0 {
            return Vec::new();
        }
        ids.truncate(size as usize / size_of::<u32>());
        ids
    }

    /// `(name, min, max, default, flags, unit)` of parameter `id`.
    pub(crate) fn parameter_info(
        &self,
        id: u32,
    ) -> Option<(String, f32, f32, f32, AudioUnitParameterOptions, u32)> {
        // SAFETY: the info struct is plain data; all-zero is a valid value.
        let zero: AudioUnitParameterInfo = unsafe { std::mem::zeroed() };
        let info = self.get(
            kAudioUnitProperty_ParameterInfo,
            kAudioUnitScope_Global,
            id,
            zero,
        )?;
        let flags = info.flags;
        let name = if flags.contains(AudioUnitParameterOptions::Flag_HasCFNameString)
            && !info.cfNameString.is_null()
        {
            let s = info.cfNameString;
            if flags.contains(AudioUnitParameterOptions::Flag_CFNameRelease) {
                cfstring(s)
            } else {
                // SAFETY: a non-owned CFString valid while the unit lives.
                unsafe { &*s }.to_string()
            }
        } else {
            let bytes: Vec<u8> = info
                .name
                .iter()
                .take_while(|c| **c != 0)
                .map(|c| *c as u8)
                .collect();
            String::from_utf8_lossy(&bytes).into_owned()
        };
        if info.unit == AudioUnitParameterUnit::CustomUnit
            && flags.contains(AudioUnitParameterOptions::Flag_CFNameRelease)
        {
            // The host owns the custom unit name too; release it.
            drop(cfstring(info.unitName));
        }
        Some((
            name,
            info.minValue,
            info.maxValue,
            info.defaultValue,
            flags,
            info.unit.0,
        ))
    }

    /// Parameter `id`'s value.
    pub(crate) fn get_param(&self, id: u32) -> Option<f32> {
        let mut v = 0.0f32;
        // SAFETY: `v` is a valid out-pointer.
        let st = unsafe {
            AudioUnitGetParameter(
                self.unit,
                id,
                kAudioUnitScope_Global,
                0,
                NonNull::from(&mut v),
            )
        };
        (st == 0).then_some(v)
    }

    /// Sets parameter `id` at `offset` frames into the next render. // RT
    pub(crate) fn set_param(&self, id: u32, value: f32, offset: u32) {
        // SAFETY: plain values; the unit is open. // RT
        unsafe { AudioUnitSetParameter(self.unit, id, kAudioUnitScope_Global, 0, value, offset) };
    }

    /// Queues a MIDI 1.0 message at `offset` frames into the next render.
    /// // RT
    pub(crate) fn midi(&self, status: u8, d1: u8, d2: u8, offset: u32) {
        // SAFETY: plain values; the unit is open. // RT
        unsafe {
            MusicDeviceMIDIEvent(
                self.unit,
                u32::from(status),
                u32::from(d1),
                u32::from(d2),
                offset,
            )
        };
    }

    /// The unit's state (`ClassInfo`) as a binary property list.
    pub(crate) fn save_state(&self) -> Result<Vec<u8>, String> {
        let mut plist: *const c_void = ptr::null();
        let mut size = size_of::<*const c_void>() as u32;
        // SAFETY: `plist` receives a +1 CFPropertyListRef.
        os("AudioUnitGetProperty(ClassInfo)", unsafe {
            AudioUnitGetProperty(
                self.unit,
                kAudioUnitProperty_ClassInfo,
                kAudioUnitScope_Global,
                0,
                NonNull::from(&mut plist).cast::<c_void>(),
                NonNull::from(&mut size),
            )
        })?;
        let plist = NonNull::new(plist.cast_mut().cast::<CFPropertyList>())
            .ok_or("the unit returned no state")?;
        // SAFETY: we own the +1 reference returned above.
        let plist = unsafe { CFRetained::from_raw(plist) };
        // SAFETY: `plist` is a valid property list; null error pointer.
        let data = unsafe {
            CFPropertyListCreateData(
                None,
                Some(&plist),
                CFPropertyListFormat::BinaryFormat_v1_0,
                0,
                ptr::null_mut(),
            )
        }
        .ok_or("the state is not a property list")?;
        Ok(data.to_vec())
    }

    /// Restores a state from [`Unit::save_state`].
    pub(crate) fn load_state(&self, bytes: &[u8]) -> Result<(), String> {
        let data = CFData::from_bytes(bytes);
        // SAFETY: `data` is valid; null format and error pointers.
        let plist = unsafe {
            CFPropertyListCreateWithData(None, Some(&data), 0, ptr::null_mut(), ptr::null_mut())
        }
        .ok_or("not a property list")?;
        let raw: *const CFPropertyList = CFRetained::as_ptr(&plist).as_ptr();
        // SAFETY: the unit copies what it needs from the property list,
        // which outlives the call.
        os("AudioUnitSetProperty(ClassInfo)", unsafe {
            AudioUnitSetProperty(
                self.unit,
                kAudioUnitProperty_ClassInfo,
                kAudioUnitScope_Global,
                0,
                (&raw const raw).cast::<c_void>(),
                size_of::<*const c_void>() as u32,
            )
        })
    }

    /// Renders `frames` frames of output bus `bus` into `channels` (fewer
    /// when a channel is shorter), at `sample_time`. False when the unit
    /// failed.
    /// // RT
    pub(crate) fn render(
        &self,
        bus: u32,
        sample_time: f64,
        frames: u32,
        channels: &mut [Box<[f32]>],
    ) -> bool {
        let n = channels.len().min(MAX_BUS_CHANNELS);
        // Never let the unit write past a channel: render no more frames
        // than the shortest one holds.
        let shortest = channels
            .iter()
            .take(n)
            .map(|c| u32::try_from(c.len()).unwrap_or(u32::MAX))
            .min()
            .unwrap_or(frames);
        let frames = frames.min(shortest);
        let bytes = frames * 4;
        let empty = AudioBuffer {
            mNumberChannels: 1,
            mDataByteSize: 0,
            mData: ptr::null_mut(),
        };
        let mut list = BufferList {
            count: n as u32,
            buffers: [empty; MAX_BUS_CHANNELS],
        };
        for (b, ch) in list.buffers.iter_mut().zip(channels.iter_mut()) {
            b.mDataByteSize = bytes;
            b.mData = ch.as_mut_ptr().cast::<c_void>();
        }
        let mut stamp = timestamp(sample_time);
        let mut flags = AudioUnitRenderActionFlags(0);
        // SAFETY: `list` describes `n` buffers of `frames` f32 owned by
        // `channels` (each at least `frames` long, clamped above), valid for
        // the call; `stamp` and `flags` are valid. //
        // RT
        let st = unsafe {
            AudioUnitRender(
                self.unit,
                &mut flags,
                NonNull::from(&mut stamp),
                bus,
                frames,
                NonNull::from(&mut list).cast::<AudioBufferList>(),
            )
        };
        if st != 0 {
            return false;
        }
        // If the unit substituted its own buffers, copy from them.
        for (b, ch) in list.buffers.iter().zip(channels.iter_mut()) {
            let ours = ch.as_mut_ptr().cast::<c_void>();
            if b.mData != ours && !b.mData.is_null() {
                // SAFETY: the unit guarantees `mData` holds `frames` samples
                // until its next render. // RT
                let src =
                    unsafe { std::slice::from_raw_parts(b.mData.cast::<f32>(), frames as usize) };
                if let Some(d) = ch.get_mut(..frames as usize) {
                    d.copy_from_slice(src);
                }
            }
        }
        true
    }
}

/// An `AudioBufferList` with room for [`MAX_BUS_CHANNELS`] buffers.
#[repr(C)]
struct BufferList {
    count: u32,
    buffers: [AudioBuffer; MAX_BUS_CHANNELS],
}

fn timestamp(sample_time: f64) -> AudioTimeStamp {
    AudioTimeStamp {
        mSampleTime: sample_time,
        mHostTime: 0,
        mRateScalar: 0.0,
        mWordClockTime: 0,
        mSMPTETime: SMPTETime {
            mSubframes: 0,
            mSubframeDivisor: 0,
            mCounter: 0,
            mType: SMPTETimeType(0),
            mFlags: SMPTETimeFlags(0),
            mHours: 0,
            mMinutes: 0,
            mSeconds: 0,
            mFrames: 0,
        },
        mFlags: AudioTimeStampFlags::SampleTimeValid,
        mReserved: 0,
    }
}

/// Where the input callback reads from: set by the processor before each
/// render. Bus 0 is the main input, bus 1 the sidechain.
#[derive(Debug)]
struct InputFeed {
    /// `(pointer, channels)` per bus; null means silence.
    buses: [(*mut Box<[f32]>, usize); 2],
    /// Frames available in each channel.
    frames: usize,
    /// Silence, for buses with nothing routed (at least `frames` long).
    zeros: Box<[f32]>,
}

/// Owns an [`InputFeed`] on the heap. The unit's input callbacks get its
/// raw address (`refcon`) and only ever go through that pointer, so moving
/// the handle or writing through it never invalidates what they hold.
#[derive(Debug)]
pub(crate) struct Feed(NonNull<InputFeed>);

// SAFETY: the feed is touched by its owner (the processor) and by the render
// callback inside the owner's `AudioUnitRender` call: one thread at a time.
unsafe impl Send for Feed {}

impl Feed {
    /// A feed with silence for blocks of up to `max_frames`.
    pub(crate) fn new(max_frames: u32) -> Feed {
        let feed = Box::new(InputFeed {
            buses: [(ptr::null_mut(), 0); 2],
            frames: 0,
            zeros: vec![0.0; max_frames as usize].into_boxed_slice(),
        });
        Feed(NonNull::from(Box::leak(feed)))
    }

    /// The callbacks' `refcon`.
    pub(crate) fn refcon(&self) -> *mut c_void {
        self.0.as_ptr().cast::<c_void>()
    }

    /// Points the callbacks at `buses` (`(channels, count)` for the main
    /// input and the sidechain), `frames` long; null for silence. // RT
    pub(crate) fn set(&mut self, buses: [(*mut Box<[f32]>, usize); 2], frames: usize) {
        let p = self.0.as_ptr();
        // SAFETY: the feed is alive (owned by `self`) and no callback runs
        // now: they run only inside `AudioUnitRender`, which the owner is
        // not in while it holds `&mut self`. // RT
        unsafe {
            (*p).buses = buses;
            (*p).frames = frames;
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `Box::leak` in `new` and is freed
        // once, here; the unit is not rendered any more (its processor is
        // being dropped).
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

/// The AudioUnit input callback: hands the unit our input channels (when
/// its buffers are null) or copies them into its buffers. // RT
unsafe extern "C-unwind" fn render_input(
    refcon: NonNull<c_void>,
    _flags: NonNull<AudioUnitRenderActionFlags>,
    _stamp: NonNull<AudioTimeStamp>,
    bus: u32,
    frames: u32,
    io_data: *mut AudioBufferList,
) -> i32 {
    let feed = refcon.cast::<InputFeed>().as_ptr();
    // SAFETY: `refcon` is the processor's live `InputFeed` (see `Feed`),
    // not written while the processor is inside `AudioUnitRender`; only
    // plain fields are read, through the raw pointer.
    let (buses, avail, zeros, zeros_len) = unsafe {
        let z: &mut Box<[f32]> = &mut (*feed).zeros;
        ((*feed).buses, (*feed).frames, z.as_mut_ptr(), z.len())
    };
    let Some(list) = NonNull::new(io_data) else {
        return 0;
    };
    // SAFETY: the unit passes a valid list with `mNumberBuffers` buffers.
    let count = unsafe { (*list.as_ptr()).mNumberBuffers } as usize;
    // SAFETY: the buffers trail the header, `count` of them.
    let buffers = unsafe {
        std::slice::from_raw_parts_mut(
            (&raw mut (*list.as_ptr()).mBuffers).cast::<AudioBuffer>(),
            count,
        )
    };
    let (src, channels) = buses
        .get(bus as usize)
        .copied()
        .unwrap_or((ptr::null_mut(), 0));
    let n = (frames as usize).min(avail);
    for (k, b) in buffers.iter_mut().enumerate() {
        // A mono source feeds every channel; nothing routed is silence.
        // Either way several of the unit's buffers may be lent the same
        // memory: that is fine because a unit only reads input it was lent
        // (the AUBase contract when it passes null buffers).
        let chan: Option<&mut [f32]> = if src.is_null() || channels == 0 {
            None
        } else {
            // SAFETY: `src` points at `channels` boxed channels owned by the
            // processor's context, valid and unaliased during the render;
            // the reference ends with this iteration.
            Some(unsafe { &mut **src.add(k.min(channels - 1)) })
        };
        if b.mData.is_null() {
            // The unit wants our buffer: lend it the channel (or silence).
            let (data, len) = match chan {
                Some(c) => (c.as_mut_ptr(), c.len()),
                None => (zeros, zeros_len),
            };
            b.mData = data.cast::<c_void>();
            b.mDataByteSize = (n.min(len) * 4) as u32;
            continue;
        }
        let want = (frames as usize).min(b.mDataByteSize as usize / 4);
        // SAFETY: the unit's buffer holds `mDataByteSize` bytes. // RT
        let dst = unsafe { std::slice::from_raw_parts_mut(b.mData.cast::<f32>(), want) };
        let src_samples = chan.and_then(|c| c.get(..n.min(want)));
        let copied = match src_samples {
            Some(s) => {
                if let Some(d) = dst.get_mut(..s.len()) {
                    d.copy_from_slice(s);
                }
                s.len()
            }
            None => 0,
        };
        if let Some(rest) = dst.get_mut(copied..) {
            rest.fill(0.0);
        }
    }
    0
}

/// The latency property listener. // any thread
unsafe extern "C-unwind" fn latency_changed(
    refcon: NonNull<c_void>,
    _unit: AudioUnit,
    _id: AudioUnitPropertyID,
    _scope: AudioUnitScope,
    _element: u32,
) {
    // SAFETY: `refcon` is the `AtomicBool` the unit's owner keeps alive.
    let flag = unsafe { refcon.cast::<AtomicBool>().as_ref() };
    flag.store(true, Ordering::Release);
}

/// `AudioUnitEvent` (not in the generated bindings: it holds a union).
#[repr(C)]
#[derive(Clone, Copy)]
struct AudioUnitEvent {
    event_type: AudioUnitEventType,
    argument: AudioUnitParameter,
}

type ListenerBlock = block2::DynBlock<dyn Fn(*mut c_void, *const c_void, u64, f32)>;

unsafe extern "C-unwind" {
    fn AUEventListenerCreateWithDispatchQueue(
        out_listener: *mut AUParameterListenerRef,
        notification_interval: f32,
        value_change_granularity: f32,
        queue: *mut c_void,
        block: &ListenerBlock,
    ) -> i32;
    fn AUEventListenerAddEventType(
        listener: AUParameterListenerRef,
        object: *mut c_void,
        event: *const AudioUnitEvent,
    ) -> i32;
    fn AUEventListenerNotify(
        listener: AUParameterListenerRef,
        object: *mut c_void,
        event: *const AudioUnitEvent,
    ) -> i32;
    fn dispatch_queue_create(label: *const std::ffi::c_char, attr: *mut c_void) -> *mut c_void;
    fn dispatch_release(object: *mut c_void);
}

/// A UI parameter event: `(kind, parameter, value)` with kind 0 = value,
/// 1 = begin gesture, 2 = end gesture.
pub(crate) type UiEvent = (u32, u32, f32);

/// Listens for parameter changes and gestures made in the plugin's own UI.
pub(crate) struct GestureListener {
    listener: AUParameterListenerRef,
    /// A serial queue, so events arrive in order.
    queue: *mut c_void,
    events: Arc<Mutex<Vec<UiEvent>>>,
}

// SAFETY: the listener reference is used only by its owner; the callback
// side touches only the `Mutex`.
unsafe impl Send for GestureListener {}

impl GestureListener {
    /// Listens to every parameter in `ids` of `unit`.
    pub(crate) fn new(unit: &Unit, ids: &[u32]) -> Result<GestureListener, String> {
        let events: Arc<Mutex<Vec<UiEvent>>> = Arc::default();
        let sink = Arc::clone(&events);
        let block = RcBlock::new(
            move |_obj: *mut c_void, ev: *const c_void, _t: u64, value: f32| {
                // SAFETY: the system passes a valid `AudioUnitEvent` for the
                // call (or null, ignored).
                let Some(ev) = (unsafe { ev.cast::<AudioUnitEvent>().as_ref() }) else {
                    return;
                };
                if let Ok(mut v) = sink.lock() {
                    v.push((ev.event_type.0, ev.argument.mParameterID, value));
                }
            },
        );
        let mut listener: AUParameterListenerRef = ptr::null_mut();
        // SAFETY: a static label; null attributes make a serial queue.
        let queue =
            unsafe { dispatch_queue_create(c"org.vizdaw.au-gestures".as_ptr(), ptr::null_mut()) };
        // SAFETY: valid out-pointer and queue; the block is retained by the
        // listener.
        let st = unsafe {
            AUEventListenerCreateWithDispatchQueue(&mut listener, 0.0, 0.0, queue, &block)
        };
        if st != 0 {
            // SAFETY: the queue was created above and is not used.
            unsafe { dispatch_release(queue) };
            return Err(format!(
                "AUEventListenerCreateWithDispatchQueue failed with status {st}"
            ));
        }
        let me = GestureListener {
            listener,
            queue,
            events,
        };
        for &id in ids {
            for kind in [
                AudioUnitEventType::ParameterValueChange,
                AudioUnitEventType::BeginParameterChangeGesture,
                AudioUnitEventType::EndParameterChangeGesture,
            ] {
                let ev = event(unit, kind, id);
                // SAFETY: `ev` is valid for the call; the listener is open.
                unsafe { AUEventListenerAddEventType(me.listener, ptr::null_mut(), &ev) };
            }
        }
        Ok(me)
    }

    /// Events since the last call, in arrival order.
    pub(crate) fn take(&self) -> Vec<UiEvent> {
        self.events
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

impl Drop for GestureListener {
    fn drop(&mut self) {
        // SAFETY: the listener and queue were created by `new` and are
        // released once, the listener first.
        unsafe {
            AUListenerDispose(self.listener);
            dispatch_release(self.queue);
        }
    }
}

fn event(unit: &Unit, kind: AudioUnitEventType, id: u32) -> AudioUnitEvent {
    AudioUnitEvent {
        event_type: kind,
        argument: AudioUnitParameter {
            mAudioUnit: unit.unit,
            mParameterID: id,
            mScope: kAudioUnitScope_Global,
            mElement: 0,
        },
    }
}

/// Does what a plugin's own UI does when the user drags a control: begin
/// gesture, set the value (notifying listeners), end gesture. Tests use it
/// to stand in for a user.
pub(crate) fn ui_gesture(unit: &Unit, id: u32, value: f32) {
    let begin = event(unit, AudioUnitEventType::BeginParameterChangeGesture, id);
    let end = event(unit, AudioUnitEventType::EndParameterChangeGesture, id);
    let mut param = begin.argument;
    // SAFETY: the events and parameter are valid for each call; a null
    // sending listener notifies every listener.
    unsafe {
        AUEventListenerNotify(ptr::null_mut(), ptr::null_mut(), &begin);
        AUParameterSet(
            ptr::null_mut(),
            ptr::null_mut(),
            NonNull::from(&mut param),
            value,
            0,
        );
        AUEventListenerNotify(ptr::null_mut(), ptr::null_mut(), &end);
    }
}

// ---------------------------------------------------------------------------
// Transport for AUs (`kAudioUnitProperty_HostCallbacks`, spec §5): the
// processor stores each block's facts in atomics before rendering; the
// unit's calls read them on the render thread. // RT

/// The transport facts one unit's host callbacks report.
#[derive(Debug, Default)]
pub(crate) struct HostTransport {
    /// Quarter-note position (f64 bits).
    beat: AtomicU64,
    /// Tempo in BPM (f64 bits).
    tempo: AtomicU64,
    /// Schedule frame (f64 bits).
    sample: AtomicU64,
    /// Stream rate (f64 bits).
    rate: AtomicU64,
    playing: AtomicBool,
    /// Set when playing toggled or the position jumped; cleared when a
    /// unit reads it.
    changed: AtomicBool,
}

impl HostTransport {
    /// A transport at 120 BPM, stopped at 0, for `rate`.
    pub(crate) fn new(rate: f64) -> HostTransport {
        let t = HostTransport::default();
        t.tempo.store(120f64.to_bits(), Ordering::Relaxed);
        t.rate.store(rate.to_bits(), Ordering::Relaxed);
        t
    }

    /// The stream rate changed (activation). Main thread.
    pub(crate) fn set_rate(&self, rate: f64) {
        self.rate.store(rate.to_bits(), Ordering::Relaxed);
    }

    /// Stores one block's facts (`expected` is the frame the previous
    /// block's end predicted: anything else is a jump). // RT
    pub(crate) fn set(&self, playing: bool, frame: u64, beats: f64, tempo: f64, expected: u64) {
        let was = self.playing.swap(playing, Ordering::Relaxed);
        if was != playing || frame != expected {
            self.changed.store(true, Ordering::Relaxed);
        }
        self.beat.store(beats.to_bits(), Ordering::Relaxed);
        let tempo = if tempo.is_finite() && tempo > 0.0 {
            tempo
        } else {
            120.0
        };
        self.tempo.store(tempo.to_bits(), Ordering::Relaxed);
        self.sample
            .store((frame as f64).to_bits(), Ordering::Relaxed);
    }

    fn f(a: &AtomicU64) -> f64 {
        f64::from_bits(a.load(Ordering::Relaxed))
    }

    /// What `beatAndTempoProc` reports: beat, tempo. // RT
    pub(crate) fn beat_and_tempo(&self) -> (f64, f64) {
        (Self::f(&self.beat), Self::f(&self.tempo))
    }

    /// What `musicalTimeLocationProc` reports: frames to the next whole
    /// beat, the 4/4 time signature (3b Plan Ruling 11) and the downbeat of
    /// the current bar. // RT
    pub(crate) fn musical_time(&self) -> (u32, f32, u32, f64) {
        let beat = Self::f(&self.beat);
        let frames_per_beat = 60.0 / Self::f(&self.tempo) * Self::f(&self.rate);
        let to_next = (beat.ceil() - beat) * frames_per_beat;
        let downbeat = (beat / 4.0).floor() * 4.0;
        (to_next.round().max(0.0) as u32, 4.0, 4, downbeat)
    }

    /// What `transportStateProc2` reports: playing, changed (cleared by the
    /// read), sample position. // RT
    pub(crate) fn state(&self) -> (bool, bool, f64) {
        (
            self.playing.load(Ordering::Relaxed),
            self.changed.swap(false, Ordering::Relaxed),
            Self::f(&self.sample),
        )
    }
}

/// `refcon` is the `HostTransport` the callbacks were installed with.
unsafe fn transport<'a>(refcon: *mut c_void) -> Option<&'a HostTransport> {
    // SAFETY: the callbacks are installed with a pointer to a
    // `HostTransport` that the instance and its processor keep alive (in an
    // `Arc`) for as long as the unit exists.
    unsafe { refcon.cast::<HostTransport>().as_ref() }
}

unsafe extern "C-unwind" fn beat_and_tempo_proc(
    refcon: *mut c_void,
    beat: *mut f64,
    tempo: *mut f64,
) -> i32 {
    // SAFETY: see `transport`.
    let Some(t) = (unsafe { transport(refcon) }) else {
        return -10863; // kAudioUnitErr_CannotDoInCurrentContext
    };
    let (b, bpm) = t.beat_and_tempo();
    // SAFETY: the unit passes valid out-pointers or null for values it
    // does not want.
    unsafe {
        if let Some(p) = beat.as_mut() {
            *p = b;
        }
        if let Some(p) = tempo.as_mut() {
            *p = bpm;
        }
    }
    0
}

unsafe extern "C-unwind" fn musical_time_proc(
    refcon: *mut c_void,
    to_next_beat: *mut u32,
    numerator: *mut f32,
    denominator: *mut u32,
    downbeat: *mut f64,
) -> i32 {
    // SAFETY: see `transport`.
    let Some(t) = (unsafe { transport(refcon) }) else {
        return -10863;
    };
    let (next, num, den, down) = t.musical_time();
    // SAFETY: valid out-pointers or null, as above.
    unsafe {
        if let Some(p) = to_next_beat.as_mut() {
            *p = next;
        }
        if let Some(p) = numerator.as_mut() {
            *p = num;
        }
        if let Some(p) = denominator.as_mut() {
            *p = den;
        }
        if let Some(p) = downbeat.as_mut() {
            *p = down;
        }
    }
    0
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C-unwind" fn transport_state_proc2(
    refcon: *mut c_void,
    playing: *mut u8,
    recording: *mut u8,
    changed: *mut u8,
    sample: *mut f64,
    cycling: *mut u8,
    cycle_start: *mut f64,
    cycle_end: *mut f64,
) -> i32 {
    // SAFETY: see `transport`.
    let Some(t) = (unsafe { transport(refcon) }) else {
        return -10863;
    };
    let (play, moved, at) = t.state();
    // SAFETY: valid out-pointers or null, as above.
    unsafe {
        if let Some(p) = playing.as_mut() {
            *p = u8::from(play);
        }
        if let Some(p) = recording.as_mut() {
            *p = 0;
        }
        if let Some(p) = changed.as_mut() {
            *p = u8::from(moved);
        }
        if let Some(p) = sample.as_mut() {
            *p = at;
        }
        if let Some(p) = cycling.as_mut() {
            *p = 0;
        }
        if let Some(p) = cycle_start.as_mut() {
            *p = 0.0;
        }
        if let Some(p) = cycle_end.as_mut() {
            *p = 0.0;
        }
    }
    0
}

unsafe extern "C-unwind" fn transport_state_proc(
    refcon: *mut c_void,
    playing: *mut u8,
    changed: *mut u8,
    sample: *mut f64,
    cycling: *mut u8,
    cycle_start: *mut f64,
    cycle_end: *mut f64,
) -> i32 {
    // SAFETY: forwards the same pointers (recording not asked).
    unsafe {
        transport_state_proc2(
            refcon,
            playing,
            ptr::null_mut(),
            changed,
            sample,
            cycling,
            cycle_start,
            cycle_end,
        )
    }
}

fn callbacks(t: &HostTransport) -> HostCallbackInfo {
    HostCallbackInfo {
        hostUserData: ptr::from_ref(t).cast_mut().cast::<c_void>(),
        beatAndTempoProc: Some(beat_and_tempo_proc),
        musicalTimeLocationProc: Some(musical_time_proc),
        transportStateProc: Some(transport_state_proc),
        transportStateProc2: Some(transport_state_proc2),
    }
}

impl Unit {
    /// Installs the host callbacks reading `t` (which must outlive the
    /// unit: callers keep it in the same `Arc`s as the unit).
    pub(crate) fn set_host_callbacks(&self, t: &Arc<HostTransport>) -> Result<(), String> {
        let info = callbacks(t);
        self.set(
            "set HostCallbacks",
            kAudioUnitProperty_HostCallbacks,
            kAudioUnitScope_Global,
            0,
            &info,
        )
    }
}

/// What the callbacks installed for `t` report, called the way a unit
/// calls them (tests): beat and tempo; frames to the next beat, numerator,
/// denominator, downbeat; playing, changed, sample.
#[allow(clippy::type_complexity)]
pub(crate) fn call_host_callbacks(
    t: &HostTransport,
) -> ((f64, f64), (u32, f32, u32, f64), (bool, bool, f64)) {
    let info = callbacks(t);
    let (mut b, mut bpm) = (0.0, 0.0);
    let (mut next, mut num, mut den, mut down) = (0u32, 0f32, 0u32, 0.0);
    let (mut play, mut rec, mut moved, mut at, mut cyc, mut cs, mut ce) =
        (0u8, 0u8, 0u8, 0.0, 0u8, 0.0, 0.0);
    // SAFETY: the callbacks get the pointer they were made for and valid
    // out-pointers, exactly as a unit would pass them.
    unsafe {
        if let Some(f) = info.beatAndTempoProc {
            f(info.hostUserData, &mut b, &mut bpm);
        }
        if let Some(f) = info.musicalTimeLocationProc {
            f(info.hostUserData, &mut next, &mut num, &mut den, &mut down);
        }
        if let Some(f) = info.transportStateProc2 {
            f(
                info.hostUserData,
                &mut play,
                &mut rec,
                &mut moved,
                &mut at,
                &mut cyc,
                &mut cs,
                &mut ce,
            );
        }
    }
    (
        (b, bpm),
        (next, num, den, down),
        (play != 0, moved != 0, at),
    )
}

// ---------------------------------------------------------------------------
// Editors (spec §9; 3b Plan Ruling 7 left them to 3c): the unit's own Cocoa
// view (AU v2, `kAudioUnitProperty_CocoaUI`), its view controller's view
// (AU v3, `kAudioUnitProperty_RequestViewController`), or CoreAudioKit's
// generic view. AppKit: main thread only.

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "CoreAudioKit", kind = "framework")]
unsafe extern "C" {}

/// Where an editor's view came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewKind {
    /// The unit's Cocoa view factory (AU v2).
    Cocoa,
    /// The unit's view controller (AU v3).
    Controller,
    /// CoreAudioKit's `AUGenericView` (a unit without a UI of its own).
    Generic,
}

/// A plugin's `NSView`, shown inside a parent view the app owns. Removed
/// from its parent on drop (before the unit can go: `AuInstance` drops its
/// editor first).
pub(crate) struct View {
    view: Retained<AnyObject>,
    /// A v3 view controller owns its view: kept while the view is shown.
    _controller: Option<Retained<AnyObject>>,
    pub(crate) kind: ViewKind,
}

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View").field("kind", &self.kind).finish()
    }
}

/// The size an editor is asked for when the unit has no preference.
const PREFERRED: CGSize = CGSize {
    width: 400.0,
    height: 300.0,
};

impl Unit {
    /// The unit's editor view: its Cocoa view, else its v3 view controller
    /// (waited for up to `wait` while the main run loop runs), else the
    /// generic view. Main thread only.
    pub(crate) fn editor_view(&self, wait: Duration) -> Result<View, String> {
        if MainThreadMarker::new().is_none() {
            return Err("plugin editors open on the main thread only".into());
        }
        self.cocoa_view()
            .or_else(|_| self.controller_view(wait))
            .or_else(|_| self.generic_view())
    }

    fn cocoa_view(&self) -> Result<View, String> {
        let mut size = 0u32;
        // SAFETY: valid unit and out-pointer; no data is written.
        os("CocoaUI info", unsafe {
            AudioUnitGetPropertyInfo(
                self.unit,
                kAudioUnitProperty_CocoaUI,
                kAudioUnitScope_Global,
                0,
                &mut size,
                ptr::null_mut(),
            )
        })?;
        let n = size as usize;
        if n < size_of::<AudioUnitCocoaViewInfo>() {
            return Err("no Cocoa view".into());
        }
        // One URL followed by at least one class name, each a CF object.
        let mut raw = vec![ptr::null::<c_void>(); n.div_ceil(size_of::<*const c_void>())];
        // SAFETY: `raw` has room for `size` bytes.
        os("CocoaUI", unsafe {
            AudioUnitGetProperty(
                self.unit,
                kAudioUnitProperty_CocoaUI,
                kAudioUnitScope_Global,
                0,
                NonNull::new_unchecked(raw.as_mut_ptr().cast::<c_void>()),
                NonNull::from(&mut size),
            )
        })?;
        // SAFETY: the property hands the host one reference to each CF
        // object (the URL, then every class name); each non-null entry is
        // adopted exactly once here, so every path below releases them.
        let (url, mut names) = unsafe {
            let mut it = raw.iter().take(n / size_of::<*const c_void>());
            let url = it
                .next()
                .and_then(|u| NonNull::new(u.cast_mut().cast::<CFURL>()))
                .map(|u| CFRetained::from_raw(u));
            let names: Vec<CFRetained<CFString>> = it
                .filter_map(|c| NonNull::new(c.cast_mut().cast::<CFString>()))
                .map(|c| CFRetained::from_raw(c))
                .collect();
            (url, names)
        };
        if names.is_empty() {
            return Err("empty Cocoa view info".into());
        }
        let url = url.ok_or("empty Cocoa view info")?;
        let class = names.swap_remove(0);
        let bundle = CFBundle::new(None, Some(&url)).ok_or("cannot open the view bundle")?;
        // SAFETY: loading the plugin's own view bundle runs its
        // initializers; the plugin is trusted in-process code by then.
        if !unsafe { bundle.load_executable() } {
            return Err("cannot load the view bundle".into());
        }
        let name = CString::new(class.to_string()).map_err(|e| e.to_string())?;
        let cls = AnyClass::get(&name).ok_or("no view factory class")?;
        // SAFETY: the class implements `AUCocoaUIBase` (that is what the
        // property names); `new` returns an owned factory and
        // `uiViewForAudioUnit:withSize:` an autoreleased `NSView` for a live
        // unit.
        let view: Option<Retained<AnyObject>> = unsafe {
            let factory: Retained<AnyObject> = msg_send![cls, new];
            msg_send![&factory, uiViewForAudioUnit: self.unit, withSize: PREFERRED]
        };
        Ok(View {
            view: view.ok_or("the view factory made no view")?,
            _controller: None,
            kind: ViewKind::Cocoa,
        })
    }

    fn controller_view(&self, wait: Duration) -> Result<View, String> {
        /// What the completion handler reported.
        #[derive(Default)]
        struct Reply {
            /// The handler ran (with a controller or with nil).
            answered: bool,
            /// A retained controller (`Retained::into_raw`), if given.
            vc: Option<usize>,
            /// The wait is over: a late controller is not kept.
            abandoned: bool,
        }
        let slot: Arc<Mutex<Reply>> = Arc::new(Mutex::new(Reply::default()));
        let fill = Arc::clone(&slot);
        let block = RcBlock::new(move |vc: *mut AnyObject| {
            let mut r = fill.lock().unwrap_or_else(PoisonError::into_inner);
            r.answered = true;
            if r.abandoned {
                return;
            }
            // SAFETY: the unit hands over a valid view controller (or
            // null); one reference is taken here and adopted below.
            if let Some(vc) = unsafe { Retained::retain(vc) } {
                r.vc = Some(Retained::into_raw(vc) as usize);
            }
        });
        let ptr_to_block: *const block2::Block<dyn Fn(*mut AnyObject)> = &*block;
        self.set(
            "RequestViewController",
            kAudioUnitProperty_RequestViewController,
            kAudioUnitScope_Global,
            0,
            &ptr_to_block,
        )?;
        let until = std::time::Instant::now() + wait;
        let raw = loop {
            {
                let mut r = slot.lock().unwrap_or_else(PoisonError::into_inner);
                if r.answered {
                    // A nil answer: no controller, no more waiting.
                    break r.vc.take().ok_or("no view controller")?;
                }
                if std::time::Instant::now() >= until {
                    r.abandoned = true;
                    return Err("no view controller".into());
                }
            }
            // SAFETY: the default mode is a valid static; the main run loop
            // delivers the unit's completion.
            let mode = unsafe { kCFRunLoopDefaultMode };
            CFRunLoop::run_in_mode(mode, 0.02, true);
        };
        // SAFETY: `raw` is the reference taken in the block.
        let vc =
            unsafe { Retained::from_raw(raw as *mut AnyObject) }.ok_or("no view controller")?;
        // SAFETY: an `NSViewController`'s `view` is a retained-by-it NSView.
        let view: Option<Retained<AnyObject>> = unsafe { msg_send![&vc, view] };
        Ok(View {
            view: view.ok_or("the view controller has no view")?,
            _controller: Some(vc),
            kind: ViewKind::Controller,
        })
    }

    fn generic_view(&self) -> Result<View, String> {
        let cls = AnyClass::get(c"AUGenericView").ok_or("CoreAudioKit is missing")?;
        // SAFETY: `AUGenericView` is an NSView subclass whose
        // `initWithAudioUnit:` takes a live unit.
        let view: Option<Retained<AnyObject>> = unsafe {
            let obj: Allocated<AnyObject> = msg_send![cls, alloc];
            msg_send![obj, initWithAudioUnit: self.unit]
        };
        Ok(View {
            view: view.ok_or("no generic view")?,
            _controller: None,
            kind: ViewKind::Generic,
        })
    }
}

impl View {
    /// Adds the view to `parent` (an `NSView *` the caller keeps alive while
    /// it is shown) at the origin (the bottom-left of an unflipped parent); returns its size in points.
    pub(crate) fn attach(&self, parent: *mut c_void) -> Result<(u32, u32), String> {
        if MainThreadMarker::new().is_none() {
            return Err("plugin editors open on the main thread only".into());
        }
        let parent = parent.cast::<AnyObject>();
        // SAFETY: `parent` is a live `NSView` (the `ParentView` contract);
        // the view is ours.
        unsafe {
            let parent = parent.as_ref().ok_or("no parent view")?;
            let _: () = msg_send![parent, addSubview: &*self.view];
            let _: () = msg_send![&*self.view, setFrameOrigin: CGPoint { x: 0.0, y: 0.0 }];
        }
        Ok(self.size())
    }

    /// The view's size in points.
    pub(crate) fn size(&self) -> (u32, u32) {
        // SAFETY: `frame` of a live view.
        let f: CGRect = unsafe { msg_send![&*self.view, frame] };
        (
            f.size.width.round().max(1.0) as u32,
            f.size.height.round().max(1.0) as u32,
        )
    }
}

impl Drop for View {
    fn drop(&mut self) {
        if MainThreadMarker::new().is_some() {
            // SAFETY: removing a live view from its parent (a no-op when it
            // has none).
            unsafe {
                let _: () = msg_send![&*self.view, removeFromSuperview];
            }
        }
    }
}
