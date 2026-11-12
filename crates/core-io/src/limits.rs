//! Limits on hostile input (spec §6.5). Every reader takes a [`Limits`];
//! a package reader also takes a [`PackageLimits`].

/// Largest MIDI file read, in bytes (§6.5).
pub const MAX_MIDI_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Longest variable-length quantity, in bytes (§6.5).
pub const MAX_VLQ_BYTES: usize = 4;
/// Most events in one file, all tracks together (§6.5).
pub const MAX_EVENTS: usize = 10_000_000;
/// Largest system-exclusive message, in bytes (§6.5).
pub const MAX_SYSEX_BYTES: usize = 1024 * 1024;
/// Most MTrk chunks read; more is refused (§6.5).
pub const MAX_TRACKS: usize = 4096;
/// Longest single meta or Flex Data text, in bytes (§6.5).
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Most details a report keeps per item (§6.5).
pub const MAX_DETAILS: usize = 50;

/// The limits a reader applies; tests lower them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most events in one file, all tracks together.
    pub max_events: usize,
    /// Largest system-exclusive message, in bytes.
    pub max_sysex_bytes: usize,
    /// Most MTrk chunks read.
    pub max_tracks: usize,
    /// Longest single text, in bytes.
    pub max_text_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_events: MAX_EVENTS,
            max_sysex_bytes: MAX_SYSEX_BYTES,
            max_tracks: MAX_TRACKS,
            max_text_bytes: MAX_TEXT_BYTES,
        }
    }
}

/// Most entries in a package, directories included (§6.5).
pub const MAX_ZIP_ENTRIES: usize = 10_000;
/// Longest entry name in a package, in bytes (§6.5).
pub const MAX_ENTRY_NAME_BYTES: usize = 1024;
/// Most an XML entry may unpack to, as a multiple of its packed size, by its
/// declared and by its actual size (§6.5).
pub const MAX_XML_RATIO: u64 = 200;
/// Largest `project.xml`, unpacked, in bytes (§6.5).
pub const MAX_PROJECT_XML_BYTES: u64 = 256 * 1024 * 1024;
/// Largest `metadata.xml`, unpacked, in bytes (§6.5).
pub const MAX_METADATA_XML_BYTES: u64 = 1024 * 1024;
/// Largest `vizdaw/record.json`, unpacked, in bytes (§6.5).
pub const MAX_RECORD_BYTES: u64 = 256 * 1024 * 1024;
/// Largest plug-in state entry, unpacked, in bytes (§6.5).
pub const MAX_STATE_BYTES: u64 = 64 * 1024 * 1024;
/// Deepest XML element nesting (§6.5).
pub const MAX_XML_DEPTH: usize = 256;
/// Most nodes in one XML tree (§6.5).
pub const MAX_XML_NODES: usize = 4_000_000;
/// Most bytes one XML tree holds, arena and nodes, counted as they grow (§6.5).
pub const MAX_XML_TREE_BYTES: usize = 128 * 1024 * 1024;
/// Most bytes one DAWproject read holds at once in all, counted as it
/// grows: the XML tree plus the typed model built from it, then (the tree
/// dropped) the model plus the ID check. With the XML text, which is outside
/// it, a read stays within 256 MiB for XML under 64 MiB.
pub const MAX_MODEL_BYTES: usize = 192 * 1024 * 1024;
/// Most attributes on one XML element (§6.5).
pub const MAX_XML_ATTRS: usize = 64;
/// Longest XML element or attribute name, in bytes; longer is broken XML
/// (no DAWproject name comes close).
pub const MAX_XML_NAME_BYTES: usize = 1024;
/// Longest number in a file, in characters (§6.5).
pub const MAX_NUMBER_CHARS: usize = 64;
/// Deepest timeline nesting: clips in clips, warps, note children and
/// reference chains (§6.5).
pub const MAX_TIMELINE_NESTING: usize = 16;

/// The limits a package reader applies; tests lower them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageLimits {
    /// Most entries in the package.
    pub max_entries: usize,
    /// Largest `project.xml`, unpacked, in bytes.
    pub max_project_xml: u64,
    /// Largest `metadata.xml`, unpacked, in bytes.
    pub max_metadata_xml: u64,
    /// Largest `vizdaw/record.json`, unpacked, in bytes.
    pub max_record: u64,
    /// Largest plug-in state entry, unpacked, in bytes.
    pub max_state: u64,
    /// Most an XML entry may unpack to, as a multiple of its packed size.
    pub max_ratio: u64,
    /// Deepest XML element nesting.
    pub max_depth: usize,
    /// Most nodes in one XML tree.
    pub max_nodes: usize,
    /// Most bytes one XML tree holds.
    pub max_tree_bytes: usize,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_entries: MAX_ZIP_ENTRIES,
            max_project_xml: MAX_PROJECT_XML_BYTES,
            max_metadata_xml: MAX_METADATA_XML_BYTES,
            max_record: MAX_RECORD_BYTES,
            max_state: MAX_STATE_BYTES,
            max_ratio: MAX_XML_RATIO,
            max_depth: MAX_XML_DEPTH,
            max_nodes: MAX_XML_NODES,
            max_tree_bytes: MAX_XML_TREE_BYTES,
        }
    }
}
