//! The macOS importer (sub-project 5 spec §5.1, approach B): ExtAudioFile
//! reads every format the system reads (WAV, AIFF, CAF, FLAC, MP3, AAC,
//! ALAC…) and converts to float32 at the project rate with the
//! highest-quality sample-rate converter. `core_media::import` writes the
//! CAF files. Never used on the real-time thread.

use core_media::MediaError;
use core_media::import::{MediaImporter, SourceInfo};
use objc2_audio_toolbox::{
    AudioConverterRef, AudioConverterSetProperty, ExtAudioFileDispose, ExtAudioFileGetProperty,
    ExtAudioFileOpenURL, ExtAudioFileRead, ExtAudioFileRef, ExtAudioFileSetProperty,
    kAudioConverterQuality_Max, kAudioConverterSampleRateConverterComplexity,
    kAudioConverterSampleRateConverterComplexity_Mastering,
    kAudioConverterSampleRateConverterQuality, kExtAudioFileProperty_AudioConverter,
    kExtAudioFileProperty_ClientDataFormat, kExtAudioFileProperty_ConverterConfig,
    kExtAudioFileProperty_FileDataFormat, kExtAudioFileProperty_FileLengthFrames,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription, kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsPacked, kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFURL;
use std::ffi::c_void;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

/// Frames per `ExtAudioFileRead`.
const CHUNK: u32 = 8192;

/// Imports any file the system can read (see the module docs).
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtAudioImporter;

/// An open ExtAudioFile, disposed on drop.
struct File(ExtAudioFileRef);

impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `ExtAudioFileOpenURL` and is disposed
        // exactly once, here.
        unsafe { ExtAudioFileDispose(self.0) };
    }
}

fn check(status: i32, call: &str, path: &Path) -> Result<(), MediaError> {
    if status == 0 {
        Ok(())
    } else {
        let cc = four_cc(status as u32);
        Err(MediaError::Unsupported(format!(
            "{}: {call} failed ({status}{})",
            path.display(),
            if cc.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                format!(", '{cc}'")
            } else {
                String::new()
            }
        )))
    }
}

fn four_cc(v: u32) -> String {
    v.to_be_bytes().iter().map(|b| char::from(*b)).collect()
}

fn open(path: &Path) -> Result<File, MediaError> {
    if !path.is_file() {
        return Err(MediaError::Io(format!("{}: no such file", path.display())));
    }
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `bytes` is a valid buffer of the given length.
    let url = unsafe {
        CFURL::from_file_system_representation(None, bytes.as_ptr(), bytes.len() as isize, false)
    }
    .ok_or_else(|| MediaError::Io(format!("{}: not a file URL", path.display())))?;
    let mut f: ExtAudioFileRef = std::ptr::null_mut();
    // SAFETY: `url` is a valid CFURL and `f` a valid out pointer.
    let st = unsafe { ExtAudioFileOpenURL(&url, NonNull::from(&mut f)) };
    check(st, "ExtAudioFileOpenURL", path)?;
    Ok(File(f))
}

fn get<T: Copy>(f: &File, id: u32, path: &Path) -> Result<T, MediaError> {
    // SAFETY: only used with plain-old-data property types (ASBD, i64,
    // pointers) for which all-zero is a valid value.
    let mut v: T = unsafe { std::mem::zeroed() };
    let mut size = size_of::<T>() as u32;
    // SAFETY: `v` is a buffer of `size` bytes for a property of type `T`.
    let st = unsafe {
        ExtAudioFileGetProperty(
            f.0,
            id,
            NonNull::from(&mut size),
            NonNull::from(&mut v).cast::<c_void>(),
        )
    };
    check(st, "ExtAudioFileGetProperty", path)?;
    Ok(v)
}

fn file_format(f: &File, path: &Path) -> Result<AudioStreamBasicDescription, MediaError> {
    let d: AudioStreamBasicDescription = get(f, kExtAudioFileProperty_FileDataFormat, path)?;
    if d.mChannelsPerFrame == 0 || d.mSampleRate.is_nan() || d.mSampleRate <= 0.0 {
        return Err(MediaError::Unsupported(format!(
            "{}: no channels or no rate",
            path.display()
        )));
    }
    Ok(d)
}

/// Asks the file's converter for maximum quality (best effort: a file
/// without a converter, e.g. no rate change, keeps what it has).
fn best_quality(f: &File, path: &Path) {
    let Ok(conv) = get::<AudioConverterRef>(f, kExtAudioFileProperty_AudioConverter, path) else {
        return;
    };
    if conv.is_null() {
        return;
    }
    for (prop, value) in [
        (
            kAudioConverterSampleRateConverterQuality,
            kAudioConverterQuality_Max,
        ),
        (
            kAudioConverterSampleRateConverterComplexity,
            kAudioConverterSampleRateConverterComplexity_Mastering,
        ),
    ] {
        let mut v = value;
        // SAFETY: `conv` is the file's live converter; `v` a valid u32.
        let _ = unsafe {
            AudioConverterSetProperty(
                conv,
                prop,
                size_of::<u32>() as u32,
                NonNull::from(&mut v).cast::<c_void>(),
            )
        };
    }
    // A null converter configuration makes ExtAudioFile apply the
    // converter's new properties.
    let mut none: *const c_void = std::ptr::null();
    // SAFETY: the property takes a `CFPropertyListRef`; null is allowed.
    let _ = unsafe {
        ExtAudioFileSetProperty(
            f.0,
            kExtAudioFileProperty_ConverterConfig,
            size_of::<*const c_void>() as u32,
            NonNull::from(&mut none).cast::<c_void>(),
        )
    };
}

impl MediaImporter for ExtAudioImporter {
    fn probe(&self, path: &Path) -> Result<SourceInfo, MediaError> {
        let f = open(path)?;
        let d = file_format(&f, path)?;
        let frames: i64 = get(&f, kExtAudioFileProperty_FileLengthFrames, path)?;
        Ok(SourceInfo {
            format_name: four_cc(d.mFormatID),
            sample_rate: d.mSampleRate,
            bits: d.mBitsPerChannel,
            channels: u16::try_from(d.mChannelsPerFrame).unwrap_or(u16::MAX),
            frames: u64::try_from(frames).unwrap_or(0),
        })
    }

    fn decode(
        &self,
        path: &Path,
        rate: u32,
        sink: &mut dyn FnMut(&[f32]) -> Result<(), MediaError>,
        cancel: &AtomicBool,
    ) -> Result<u64, MediaError> {
        let f = open(path)?;
        let ch = file_format(&f, path)?.mChannelsPerFrame;
        let mut client = AudioStreamBasicDescription {
            mSampleRate: f64::from(rate),
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
            mBytesPerPacket: 4 * ch,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4 * ch,
            mChannelsPerFrame: ch,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        // SAFETY: `client` is a valid ASBD of the stated size.
        let st = unsafe {
            ExtAudioFileSetProperty(
                f.0,
                kExtAudioFileProperty_ClientDataFormat,
                size_of::<AudioStreamBasicDescription>() as u32,
                NonNull::from(&mut client).cast::<c_void>(),
            )
        };
        check(st, "set client format", path)?;
        best_quality(&f, path);
        let per = ch as usize;
        let mut buf = vec![0.0f32; CHUNK as usize * per];
        let mut total = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            let mut frames = CHUNK;
            let mut abl = AudioBufferList {
                mNumberBuffers: 1,
                mBuffers: [AudioBuffer {
                    mNumberChannels: ch,
                    mDataByteSize: (buf.len() * 4) as u32,
                    mData: buf.as_mut_ptr().cast::<c_void>(),
                }],
            };
            // SAFETY: `abl` describes `buf`, which outlives the call;
            // `frames` is a valid in/out count.
            let st = unsafe {
                ExtAudioFileRead(f.0, NonNull::from(&mut frames), NonNull::from(&mut abl))
            };
            check(st, "ExtAudioFileRead", path)?;
            if frames == 0 {
                return Ok(total);
            }
            let n = (frames as usize).min(CHUNK as usize) * per;
            sink(buf.get(..n).unwrap_or(&[]))?;
            total += u64::from(frames);
        }
    }
}
