//! A ZIP written byte by byte, so a test can lie about any field: local
//! headers, the central directory and the end record.

/// One entry as the archive will claim it.
#[derive(Debug, Clone)]
pub struct RawEntry {
    /// The name bytes, written as they are.
    pub name: Vec<u8>,
    /// The entry's bytes as stored (already compressed for method 8).
    pub data: Vec<u8>,
    /// The compression method field.
    pub method: u16,
    /// The general-purpose flags field.
    pub flags: u16,
    /// The uncompressed size field.
    pub declared_size: u32,
    /// The CRC field; `None` is the CRC of `data`.
    pub crc: Option<u32>,
    /// A Unix mode in the external attributes, with a Unix "version made by".
    pub unix_mode: Option<u32>,
    /// Sizes become `0xFFFF_FFFF`, with a ZIP64 extra claiming this
    /// uncompressed size (and the true compressed size).
    pub zip64_size: Option<u64>,
}

impl RawEntry {
    /// An honest Stored entry.
    pub fn stored(name: &str, data: &[u8]) -> Self {
        Self {
            name: name.as_bytes().to_vec(),
            data: data.to_vec(),
            method: 0,
            flags: 0,
            declared_size: u32::try_from(data.len()).unwrap(),
            crc: None,
            unix_mode: None,
            zip64_size: None,
        }
    }

    /// An honest Deflated entry of `plain`.
    pub fn deflated(name: &str, plain: &[u8]) -> Self {
        use std::io::Write;
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(plain).unwrap();
        Self {
            name: name.as_bytes().to_vec(),
            data: enc.finish().unwrap(),
            method: 8,
            flags: 0,
            declared_size: u32::try_from(plain.len()).unwrap(),
            crc: Some(rawzip::crc32(plain)),
            unix_mode: None,
            zip64_size: None,
        }
    }
}

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// The entry's (crc, compressed field, uncompressed field, extra).
fn fields(e: &RawEntry) -> (u32, u32, u32, Vec<u8>) {
    let crc = e.crc.unwrap_or_else(|| rawzip::crc32(&e.data));
    match e.zip64_size {
        Some(size) => {
            let mut extra = Vec::new();
            u16le(&mut extra, 0x0001);
            u16le(&mut extra, 16);
            extra.extend_from_slice(&size.to_le_bytes());
            extra.extend_from_slice(&(e.data.len() as u64).to_le_bytes());
            (crc, u32::MAX, u32::MAX, extra)
        }
        None => (
            crc,
            u32::try_from(e.data.len()).unwrap(),
            e.declared_size,
            Vec::new(),
        ),
    }
}

/// A ZIP of `entries` whose end record claims their true count.
pub fn raw_zip(entries: &[RawEntry]) -> Vec<u8> {
    raw_zip_claiming(entries, u16::try_from(entries.len()).unwrap())
}

/// A ZIP of `entries` whose end record claims `claimed` entries.
pub fn raw_zip_claiming(entries: &[RawEntry], claimed: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let mut offsets = Vec::new();
    for e in entries {
        let (crc, csize, usize_, extra) = fields(e);
        offsets.push(u32::try_from(out.len()).unwrap());
        u32le(&mut out, 0x0403_4b50);
        u16le(&mut out, if e.zip64_size.is_some() { 45 } else { 20 });
        u16le(&mut out, e.flags);
        u16le(&mut out, e.method);
        u16le(&mut out, 0); // time
        u16le(&mut out, 0); // date
        u32le(&mut out, crc);
        u32le(&mut out, csize);
        u32le(&mut out, usize_);
        u16le(&mut out, u16::try_from(e.name.len()).unwrap());
        u16le(&mut out, u16::try_from(extra.len()).unwrap());
        out.extend_from_slice(&e.name);
        out.extend_from_slice(&extra);
        out.extend_from_slice(&e.data);
    }
    let cd_start = out.len();
    for (e, offset) in entries.iter().zip(&offsets) {
        let (crc, csize, usize_, extra) = fields(e);
        u32le(&mut out, 0x0201_4b50);
        let needed = if e.zip64_size.is_some() { 45 } else { 20 };
        u16le(
            &mut out,
            if e.unix_mode.is_some() {
                (3 << 8) | needed
            } else {
                needed
            },
        );
        u16le(&mut out, needed);
        u16le(&mut out, e.flags);
        u16le(&mut out, e.method);
        u16le(&mut out, 0);
        u16le(&mut out, 0);
        u32le(&mut out, crc);
        u32le(&mut out, csize);
        u32le(&mut out, usize_);
        u16le(&mut out, u16::try_from(e.name.len()).unwrap());
        u16le(&mut out, u16::try_from(extra.len()).unwrap());
        u16le(&mut out, 0); // comment
        u16le(&mut out, 0); // disk
        u16le(&mut out, 0); // internal attributes
        u32le(&mut out, e.unix_mode.map_or(0, |m| m << 16));
        u32le(&mut out, *offset);
        out.extend_from_slice(&e.name);
        out.extend_from_slice(&extra);
    }
    let cd_size = out.len() - cd_start;
    u32le(&mut out, 0x0605_4b50);
    u16le(&mut out, 0);
    u16le(&mut out, 0);
    u16le(&mut out, claimed);
    u16le(&mut out, claimed);
    u32le(&mut out, u32::try_from(cd_size).unwrap());
    u32le(&mut out, u32::try_from(cd_start).unwrap());
    u16le(&mut out, 0);
    out
}
