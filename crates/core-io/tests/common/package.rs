//! Export → package for the round-trip and hostile-file tests: the export's
//! files written as a `.dawproject` (each audio entry a 44-byte stub WAV),
//! the fake media an import of it is given, and a plug-in property-list
//! codec that converts both ways.

use core_io::IoError;
use core_io::dawproject::export::DpExport;
use core_io::dawproject::import::MediaSource;
use core_io::dawproject::package::{AudioSource, DpPackage};
use core_io::dawproject::plugins::PlistCodec;
use core_io::zip::{EntryMethod, ZipWriter};
use core_media::import::Imported;
use core_model::document::Document;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The AU codec the tests use: the XML form is the binary one inside a
/// `<plist>` element, so a state comes back byte for byte.
pub struct TestPlist;

impl PlistCodec for TestPlist {
    fn to_xml(&self, binary: &[u8]) -> Result<Vec<u8>, String> {
        let mut out = b"<plist>".to_vec();
        out.extend_from_slice(binary);
        out.extend_from_slice(b"</plist>");
        Ok(out)
    }

    fn to_binary(&self, plist: &[u8]) -> Result<Vec<u8>, String> {
        plist
            .strip_prefix(b"<plist>")
            .and_then(|p| p.strip_suffix(b"</plist>"))
            .map(<[u8]>::to_vec)
            .ok_or_else(|| "not a test plist".to_owned())
    }
}

/// A WAV header for no samples: 44 bytes.
pub fn stub_wav() -> Vec<u8> {
    let mut w = Vec::with_capacity(44);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&36u32.to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&2u16.to_le_bytes()); // channels
    w.extend_from_slice(&48_000u32.to_le_bytes());
    w.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&0u32.to_le_bytes());
    w
}

/// The export's package as bytes: the project, the song info, the record,
/// the plug-in states and a stub WAV for each audio entry.
pub fn package_bytes(x: &DpExport) -> Result<Vec<u8>, IoError> {
    package_bytes_with(x, true)
}

/// [`package_bytes`], without the record unless `record`.
pub fn package_bytes_with(x: &DpExport, record: bool) -> Result<Vec<u8>, IoError> {
    let mut z = ZipWriter::new(Vec::new());
    z.add(
        "project.xml",
        EntryMethod::Deflated,
        x.project_xml.as_bytes(),
    )?;
    z.add(
        "metadata.xml",
        EntryMethod::Deflated,
        x.metadata_xml.as_bytes(),
    )?;
    if record {
        z.add(
            "vizdaw/record.json",
            EntryMethod::Deflated,
            x.record_json.as_bytes(),
        )?;
    }
    for (entry, bytes) in &x.states {
        z.add(entry, EntryMethod::Stored, bytes)?;
    }
    let wav = stub_wav();
    for a in &x.audio {
        z.add(&a.entry, EntryMethod::Stored, &wav)?;
    }
    z.finish()
}

/// [`package_bytes`] written to `dir/<stem>.dawproject`.
pub fn write_package(dir: &Path, stem: &str, x: &DpExport) -> PathBuf {
    let path = dir.join(format!("{stem}.dawproject"));
    std::fs::write(&path, package_bytes(x).expect("package")).expect("write the package");
    path
}

/// The content hash of a copy of `hash` with `gain_db` applied.
pub fn copy_hash(hash: &str, gain_db: f32) -> String {
    if gain_db == 0.0 {
        hash.to_owned()
    } else {
        format!("{hash}@{gain_db}")
    }
}

/// What a conversion of each audio file of `pkg` (a package of `x`, an
/// export of `doc`) would make, without converting anything: the original
/// media item of `doc` at its own rate, or, for a file with gain applied,
/// its copy (with [`copy_hash`]'s hash).
pub fn media_of(pkg: &DpPackage, x: &DpExport, doc: &Document) -> BTreeMap<usize, MediaSource> {
    let mut media = BTreeMap::new();
    for a in pkg.audio() {
        let AudioSource::Entry(entry) = &a.source else {
            panic!("an exported file is outside the package")
        };
        let file = x
            .audio
            .iter()
            .find(|f| &f.entry == entry)
            .unwrap_or_else(|| panic!("no audio file {entry}"));
        let item = doc.media().get(&file.media).expect("the media item");
        let hash = item.content_hash.clone().unwrap_or_default();
        media.insert(
            a.key,
            MediaSource::Online(vec![Imported {
                path: format!("Media/{}.caf", a.name),
                name: file.name.clone(),
                original_name: item.name.clone(),
                channels: item.channels,
                sample_rate: item.sample_rate,
                length_frames: item.length_frames,
                content_hash: copy_hash(&hash, file.gain_db),
                clipped: 0,
            }]),
        );
    }
    media
}
