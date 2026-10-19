//! The ExtAudioFile importer (plan 5a Task 5): WAV, AIFF and FLAC
//! fixtures written here, imported into CAF in a project format.
#![cfg(target_os = "macos")]
#![allow(unsafe_code)]

use core_media::MediaError;
use core_media::caf::CafReader;
use core_media::import::{MediaImporter, import_file};
use core_model::format::{AudioFormat, BitDepth};
use objc2_audio_toolbox::{
    AudioFileFlags, ExtAudioFileCreateWithURL, ExtAudioFileDispose, ExtAudioFileRef,
    ExtAudioFileSetProperty, ExtAudioFileWrite, kAudioFileAIFFType, kAudioFileFLACType,
    kAudioFileWAVEType, kExtAudioFileProperty_ClientDataFormat,
};
use objc2_core_audio_types::{
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription,
    kAppleLosslessFormatFlag_16BitSourceData, kAudioFormatFLAC, kAudioFormatFlagIsBigEndian,
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatFlagIsSignedInteger,
    kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFURL;
use platform_macos::media::ExtAudioImporter;
use std::ffi::c_void;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy)]
enum Kind {
    Wav,
    Aiff,
    Flac,
}

fn pcm(rate: f64, ch: u32, flags: u32) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: flags | kAudioFormatFlagIsPacked,
        mBytesPerPacket: 2 * ch,
        mFramesPerPacket: 1,
        mBytesPerFrame: 2 * ch,
        mChannelsPerFrame: ch,
        mBitsPerChannel: 16,
        mReserved: 0,
    }
}

/// Writes `frames` frames of a 441 Hz sine at amplitude 0.5 (channel k
/// phase-shifted by k) as a 16-bit `kind` file.
fn fixture(path: &Path, kind: Kind, rate: f64, ch: u32, frames: usize) {
    let (file_type, mut desc) = match kind {
        Kind::Wav => (
            kAudioFileWAVEType,
            pcm(rate, ch, kAudioFormatFlagIsSignedInteger),
        ),
        Kind::Aiff => (
            kAudioFileAIFFType,
            pcm(
                rate,
                ch,
                kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsBigEndian,
            ),
        ),
        Kind::Flac => (
            kAudioFileFLACType,
            AudioStreamBasicDescription {
                mSampleRate: rate,
                mFormatID: kAudioFormatFLAC,
                mFormatFlags: kAppleLosslessFormatFlag_16BitSourceData,
                mBytesPerPacket: 0,
                mFramesPerPacket: 0,
                mBytesPerFrame: 0,
                mChannelsPerFrame: ch,
                mBitsPerChannel: 0,
                mReserved: 0,
            },
        ),
    };
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: `bytes` is a valid buffer of the given length.
    let url = unsafe {
        CFURL::from_file_system_representation(None, bytes.as_ptr(), bytes.len() as isize, false)
    }
    .unwrap();
    let mut f: ExtAudioFileRef = std::ptr::null_mut();
    // SAFETY: valid pointers for the duration of the call.
    let st = unsafe {
        ExtAudioFileCreateWithURL(
            &url,
            file_type,
            NonNull::from(&mut desc),
            std::ptr::null(),
            AudioFileFlags::EraseFile.0,
            NonNull::from(&mut f),
        )
    };
    assert_eq!(st, 0, "create {path:?}");
    let mut client = AudioStreamBasicDescription {
        mSampleRate: rate,
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
            f,
            kExtAudioFileProperty_ClientDataFormat,
            size_of::<AudioStreamBasicDescription>() as u32,
            NonNull::from(&mut client).cast::<c_void>(),
        )
    };
    assert_eq!(st, 0);
    let mut data: Vec<f32> = (0..frames * ch as usize)
        .map(|i| {
            let (fr, k) = (i / ch as usize, i % ch as usize);
            0.5 * (2.0 * std::f32::consts::PI * 441.0 * fr as f32 / rate as f32 + k as f32).sin()
        })
        .collect();
    let mut abl = AudioBufferList {
        mNumberBuffers: 1,
        mBuffers: [AudioBuffer {
            mNumberChannels: ch,
            mDataByteSize: (data.len() * 4) as u32,
            mData: data.as_mut_ptr().cast::<c_void>(),
        }],
    };
    // SAFETY: `abl` describes `data`, alive for the call.
    let st = unsafe { ExtAudioFileWrite(f, frames as u32, NonNull::from(&mut abl)) };
    assert_eq!(st, 0, "write {path:?}");
    // SAFETY: `f` was created above and is disposed once.
    unsafe { ExtAudioFileDispose(f) };
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn import(
    src: &Path,
    pkg: &Path,
    rate: u32,
    depth: BitDepth,
) -> Result<Vec<core_media::import::Imported>, MediaError> {
    import_file(
        &ExtAudioImporter,
        src,
        pkg,
        AudioFormat {
            sample_rate: rate,
            depth,
        },
        &mut |_| {},
        &AtomicBool::new(false),
    )
}

#[test]
fn probe_reads_the_source_format() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.wav");
    fixture(&p, Kind::Wav, 44_100.0, 2, 44_100);
    let i = ExtAudioImporter.probe(&p).unwrap();
    assert_eq!(
        (i.sample_rate, i.channels, i.bits, i.frames),
        (44_100.0, 2, 16, 44_100)
    );
    assert_eq!(i.format_name, "lpcm");
}

#[test]
fn importing_44k1_into_a_48k_project_converts() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("in.wav");
    fixture(&src, Kind::Wav, 44_100.0, 2, 44_100);
    let pkg = dir.path().join("p.vizdaw");
    let got = import(&src, &pkg, 48_000, BitDepth::Int24).unwrap();
    assert_eq!(got.len(), 1);
    let i = &got[0];
    assert!(i.length_frames.abs_diff(48_000) <= 1, "{}", i.length_frames);
    let r = CafReader::open(&pkg.join(&i.path)).unwrap();
    assert_eq!(
        (r.info().sample_rate, r.info().channels, r.info().depth),
        (48_000, 2, BitDepth::Int24)
    );
    let mut out = vec![0.0; r.info().frames as usize * 2];
    r.read(0, &mut out, &mut Vec::new()).unwrap();
    // A 0.5 sine has an RMS of 0.3536; the converter keeps it.
    let left: Vec<f32> = out
        .iter()
        .step_by(2)
        .skip(1000)
        .take(40_000)
        .copied()
        .collect();
    assert!((rms(&left) - 0.3536).abs() < 0.005, "{}", rms(&left));
}

#[test]
fn wav_aiff_and_flac_import_sample_for_sample() {
    let dir = tempfile::tempdir().unwrap();
    for (kind, name) in [
        (Kind::Wav, "a.wav"),
        (Kind::Aiff, "a.aiff"),
        (Kind::Flac, "a.flac"),
    ] {
        let src = dir.path().join(name);
        fixture(&src, kind, 48_000.0, 1, 48_000);
        let pkg = dir.path().join(format!("{name}.vizdaw"));
        let got = import(&src, &pkg, 48_000, BitDepth::Float32).unwrap();
        assert_eq!(got[0].length_frames, 48_000, "{name}");
        let r = CafReader::open(&pkg.join(&got[0].path)).unwrap();
        let mut out = vec![0.0; 48_000];
        r.read(0, &mut out, &mut Vec::new()).unwrap();
        for (f, x) in out.iter().enumerate().step_by(997) {
            let want = 0.5 * (2.0 * std::f32::consts::PI * 441.0 * f as f32 / 48_000.0).sin();
            // 16-bit source: within two steps.
            assert!(
                (x - want).abs() <= 2.0 / 32_768.0,
                "{name} frame {f}: {x} vs {want}"
            );
        }
    }
}

#[test]
fn garbage_is_an_error_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bad.wav");
    std::fs::write(&p, "RIFF but not really").unwrap();
    assert!(ExtAudioImporter.probe(&p).is_err());
    assert!(import(&p, &dir.path().join("p.vizdaw"), 48_000, BitDepth::Int24).is_err());
    assert!(
        ExtAudioImporter
            .probe(&dir.path().join("missing.wav"))
            .is_err()
    );
}

#[test]
fn decode_stops_when_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("long.wav");
    fixture(&src, Kind::Wav, 48_000.0, 2, 480_000);
    let cancel = AtomicBool::new(true);
    let r = ExtAudioImporter.decode(&src, 48_000, &mut |_| Ok(()), &cancel);
    assert_eq!(r, Err(MediaError::Cancelled));
}
