//! Export → package for the round-trip and hostile-file tests: the export's
//! files written as a `.dawproject` (each audio entry a 44-byte stub WAV),
//! and a plug-in property-list codec that converts both ways.

use core_io::IoError;
use core_io::dawproject::export::DpExport;
use core_io::dawproject::plugins::PlistCodec;
use core_io::zip::{EntryMethod, ZipWriter};
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
    z.add(
        "vizdaw/record.json",
        EntryMethod::Deflated,
        x.record_json.as_bytes(),
    )?;
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
