//! Apple's `DLSMusicDevice` (the system GM bank) as an
//! [`InstrumentNode`].

use core_engine::BackendError;
use core_engine::backend::{InstrumentNode, StereoBuf};
use objc2_audio_toolbox::{
    AudioComponentDescription, AudioComponentFindNext, AudioComponentInstanceDispose,
    AudioComponentInstanceNew, AudioUnit, AudioUnitInitialize, AudioUnitRender,
    AudioUnitRenderActionFlags, AudioUnitSetProperty, AudioUnitUninitialize, MusicDeviceMIDIEvent,
    MusicDeviceSysEx, kAudioUnitManufacturer_Apple, kAudioUnitProperty_MaximumFramesPerSlice,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global, kAudioUnitScope_Output,
    kAudioUnitSubType_DLSSynth, kAudioUnitType_MusicDevice,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp, AudioTimeStampFlags,
    SMPTETime, SMPTETimeFlags, SMPTETimeType, kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM,
};
use std::ffi::c_void;
use std::ptr::{self, NonNull};

/// An `AudioBufferList` with room for two buffers (the C type declares a
/// one-element trailing array).
#[repr(C)]
struct StereoBufferList {
    count: u32,
    buffers: [AudioBuffer; 2],
}

fn os(call: &'static str, status: i32) -> Result<(), BackendError> {
    if status == 0 {
        Ok(())
    } else {
        Err(BackendError::Os { call, status })
    }
}

/// The built-in synth: one `DLSMusicDevice` serving all 16 channels.
pub struct DlsSynth {
    unit: AudioUnit,
    sample_time: f64,
    max_frames: u32,
}

// SAFETY: the AudioUnit is owned by this value and used by one thread at a
// time (created on the control thread, then moved to the render thread).
unsafe impl Send for DlsSynth {}

impl DlsSynth {
    /// Instantiates and initializes the synth for stereo non-interleaved f32
    /// output at `sample_rate`, rendering at most `max_frames` per call.
    pub fn new(sample_rate: f64, max_frames: u32) -> Result<DlsSynth, BackendError> {
        let mut desc = AudioComponentDescription {
            componentType: kAudioUnitType_MusicDevice,
            componentSubType: kAudioUnitSubType_DLSSynth,
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        // SAFETY: `desc` is a valid description; a null component starts the
        // search from the beginning.
        let comp = unsafe { AudioComponentFindNext(ptr::null_mut(), NonNull::from(&mut desc)) };
        if comp.is_null() {
            return Err(BackendError::ComponentMissing);
        }
        let mut unit: AudioUnit = ptr::null_mut();
        // SAFETY: `comp` is a valid component and `unit` a valid out-pointer.
        os("AudioComponentInstanceNew", unsafe {
            AudioComponentInstanceNew(comp, NonNull::from(&mut unit))
        })?;
        let synth = DlsSynth {
            unit,
            sample_time: 0.0,
            max_frames,
        };
        let fmt = AudioStreamBasicDescription {
            mSampleRate: sample_rate,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat
                | kAudioFormatFlagIsPacked
                | kAudioFormatFlagIsNonInterleaved,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        // SAFETY: `fmt` outlives the call and the size matches its type.
        os("AudioUnitSetProperty(StreamFormat)", unsafe {
            AudioUnitSetProperty(
                synth.unit,
                kAudioUnitProperty_StreamFormat,
                kAudioUnitScope_Output,
                0,
                (&raw const fmt).cast::<c_void>(),
                size_of::<AudioStreamBasicDescription>() as u32,
            )
        })?;
        // SAFETY: as above, for a u32 property.
        os("AudioUnitSetProperty(MaximumFramesPerSlice)", unsafe {
            AudioUnitSetProperty(
                synth.unit,
                kAudioUnitProperty_MaximumFramesPerSlice,
                kAudioUnitScope_Global,
                0,
                (&raw const max_frames).cast::<c_void>(),
                size_of::<u32>() as u32,
            )
        })?;
        // SAFETY: the unit is configured and not yet initialized.
        os("AudioUnitInitialize", unsafe {
            AudioUnitInitialize(synth.unit)
        })?;
        Ok(synth)
    }
}

impl Drop for DlsSynth {
    fn drop(&mut self) {
        // SAFETY: the unit was created by `AudioComponentInstanceNew` and is
        // not used after this.
        unsafe {
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
        }
    }
}

impl InstrumentNode for DlsSynth {
    fn midi(&mut self, offset: u32, msg: &[u8]) {
        match msg {
            [0xF0, ..] => {
                let Ok(len) = u32::try_from(msg.len()) else {
                    return;
                };
                // SAFETY: `msg` is valid for `len` bytes during the call.
                // SysEx has no frame offset and lands at the block start.
                unsafe {
                    MusicDeviceSysEx(self.unit, NonNull::from(msg).cast::<u8>(), len);
                }
            }
            [status, rest @ ..] => {
                let d1 = rest.first().copied().unwrap_or(0);
                let d2 = rest.get(1).copied().unwrap_or(0);
                // SAFETY: plain values; the unit is initialized. // RT
                unsafe {
                    MusicDeviceMIDIEvent(
                        self.unit,
                        u32::from(*status),
                        u32::from(d1),
                        u32::from(d2),
                        offset,
                    );
                }
            }
            [] => {}
        }
    }

    fn render(&mut self, frames: u32, out: &mut StereoBuf) {
        let n = frames.min(self.max_frames).min(out.capacity());
        let bytes = n * 4;
        let mut list = StereoBufferList {
            count: 2,
            buffers: [
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: bytes,
                    mData: out.left.as_mut_ptr().cast::<c_void>(),
                },
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: bytes,
                    mData: out.right.as_mut_ptr().cast::<c_void>(),
                },
            ],
        };
        let mut stamp = AudioTimeStamp {
            mSampleTime: self.sample_time,
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
        };
        let mut flags = AudioUnitRenderActionFlags(0);
        // SAFETY: `list` describes two buffers of `n` f32 samples owned by
        // `out`, valid for the call; `stamp` and `flags` are valid. // RT
        let status = unsafe {
            AudioUnitRender(
                self.unit,
                &mut flags,
                NonNull::from(&mut stamp),
                0,
                n,
                NonNull::from(&mut list).cast::<AudioBufferList>(),
            )
        };
        self.sample_time += f64::from(n);
        if status != 0 {
            out.clear(frames);
            return;
        }
        // If the unit substituted its own buffers, copy from them.
        for (buf, dst) in list.buffers.iter().zip([&mut out.left, &mut out.right]) {
            let ours = dst.as_mut_ptr().cast::<c_void>();
            if buf.mData != ours && !buf.mData.is_null() {
                // SAFETY: the unit guarantees `mData` holds `n` samples until
                // the next render call. // RT
                let src =
                    unsafe { std::slice::from_raw_parts(buf.mData.cast::<f32>(), n as usize) };
                if let Some(d) = dst.get_mut(..n as usize) {
                    d.copy_from_slice(src);
                }
            }
        }
        // Frames past `max_frames` (or past the buffer) were not rendered:
        // silence them rather than leave stale samples. // RT
        let end = frames.min(out.capacity()) as usize;
        for ch in [&mut out.left, &mut out.right] {
            if let Some(rest) = ch.get_mut(n as usize..end) {
                rest.fill(0.0);
            }
        }
    }

    fn reset(&mut self) {
        for ch in 0..16u8 {
            self.midi(0, &[0xB0 | ch, 123, 0]);
            self.midi(0, &[0xB0 | ch, 121, 0]);
        }
    }
}
