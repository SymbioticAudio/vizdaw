//! Limits on hostile input (spec §6.5). Every reader takes a [`Limits`];
//! a package reader also takes a [`PackageLimits`], an AAF reader an
//! [`AafLimits`].

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
/// Most bytes the parse of `project.xml` holds at once: its text plus its
/// tree, so the tree may use at most
/// `min(MAX_XML_TREE_BYTES, MAX_PARSE_BYTES − text length)` (final review:
/// a text near [`MAX_PROJECT_XML_BYTES`] is mostly white space or comments,
/// which carry nothing).
pub const MAX_PARSE_BYTES: usize = 256 * 1024 * 1024;
/// Largest `metadata.xml`, unpacked, in bytes (§6.5).
pub const MAX_METADATA_XML_BYTES: u64 = 1024 * 1024;
/// Largest `vizdaw/record.json`, unpacked, in bytes (§6.5).
pub const MAX_RECORD_BYTES: u64 = 256 * 1024 * 1024;
/// Largest snapshot (`Interchange/<export id>.json`) a merge reads, in
/// bytes (plan 6e Ruling 17).
pub const MAX_SNAPSHOT_BYTES: usize = 256 << 20;
/// Most list entries in one snapshot, counted while parsing (plan 6e
/// Task 7): a projection and its parallel identity, each as long as an
/// XML tree may be, so a snapshot of tiny entries cannot grow ~8× in
/// memory.
pub const MAX_SNAPSHOT_ENTRIES: usize = 2 * MAX_XML_NODES;
/// Largest plug-in state entry, unpacked, in bytes (§6.5). Shared with
/// `MixerCmd::SetPluginState`'s cap (plan 6e Task 3).
pub const MAX_STATE_BYTES: u64 = core_model::state::MAX_STATE_BYTES;
/// Most bytes of plug-in state one import holds at once, all states
/// together (states shared by several plug-ins count once).
pub const MAX_STATE_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
/// Deepest XML element nesting (§6.5).
pub const MAX_XML_DEPTH: usize = 256;
/// Most nodes in one XML tree (§6.5).
pub const MAX_XML_NODES: usize = 4_000_000;
/// Most bytes one XML tree holds, arena and nodes, counted as they grow (§6.5).
pub const MAX_XML_TREE_BYTES: usize = 128 * 1024 * 1024;
/// Most bytes one DAWproject read holds at once in all, counted as it
/// grows: the XML tree plus the typed model built from it, then (the tree
/// dropped) the model plus the ID check. The XML text is outside it, but is
/// dropped before the model is built, and while it is held it counts
/// against [`MAX_PARSE_BYTES`] with the tree, so a read stays within about
/// 256 MiB whatever the text's size.
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

/// Most note and audio tracks a DAWproject may hold; more is refused. The
/// model applies a command in time linear in the project, so an import that
/// builds n tracks takes time quadratic in n: 1 000 tracks apply in well
/// under a second, 100 000 would take hours.
pub const MAX_IMPORT_TRACKS: usize = 1_000;
/// Most buses (folders, effect and submix channels) a DAWproject may hold;
/// more is refused, for the same reason as [`MAX_IMPORT_TRACKS`] (a bus costs
/// more to add than a track).
pub const MAX_IMPORT_BUSES: usize = 256;

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
    /// Most bytes of plug-in state an import holds, all states together.
    pub max_state_total: u64,
    /// Most an XML entry may unpack to, as a multiple of its packed size.
    pub max_ratio: u64,
    /// Deepest XML element nesting.
    pub max_depth: usize,
    /// Most nodes in one XML tree.
    pub max_nodes: usize,
    /// Most bytes one XML tree holds.
    pub max_tree_bytes: usize,
    /// Most note and audio tracks the project may hold.
    pub max_import_tracks: usize,
    /// Most buses the project may hold.
    pub max_import_buses: usize,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_entries: MAX_ZIP_ENTRIES,
            max_project_xml: MAX_PROJECT_XML_BYTES,
            max_metadata_xml: MAX_METADATA_XML_BYTES,
            max_record: MAX_RECORD_BYTES,
            max_state: MAX_STATE_BYTES,
            max_state_total: MAX_STATE_TOTAL_BYTES,
            max_ratio: MAX_XML_RATIO,
            max_depth: MAX_XML_DEPTH,
            max_nodes: MAX_XML_NODES,
            max_tree_bytes: MAX_XML_TREE_BYTES,
            max_import_tracks: MAX_IMPORT_TRACKS,
            max_import_buses: MAX_IMPORT_BUSES,
        }
    }
}

/// Most objects one AAF file may hold; more is refused (plan 6f Ruling 13).
pub const MAX_AAF_OBJECTS: usize = 1_000_000;
/// Deepest strong-reference nesting below the AAF root object; deeper is
/// refused (plan 6f Ruling 13).
pub const MAX_REF_DEPTH: usize = 64;
/// Largest `properties` stream of one AAF object, in bytes; a larger one
/// leaves its object out (plan 6f Ruling 13).
pub const MAX_PROPERTIES_BYTES: u64 = 1024 * 1024;
/// Most entries in one AAF index stream; more leaves the property out
/// (plan 6f Ruling 13).
pub const MAX_INDEX_ENTRIES: usize = 1_000_000;
/// Longest chain of AAF mobs one clip may follow, walked with a visited set
/// (plan 6f Ruling 13).
pub const MAX_MOB_CHAIN: usize = 16;
/// Most control points in one AAF curve (plan 6f Ruling 13).
pub const MAX_CONTROL_POINTS: usize = 100_000;

/// Most bytes one AAF read holds, counted as it reads: `cfb`'s FAT,
/// directory and MiniFAT, then the object store (plan 6f Ruling 13, as the
/// owner ruled after Task 7's review). Its own budget, not
/// [`MAX_MODEL_BYTES`]: a large post-production session runs to 250 000
/// objects and more, about 1.2 KB each as read.
pub const AAF_READ_BUDGET: usize = 768 * 1024 * 1024;

/// The limits an AAF reader applies (plan 6f Ruling 13); tests lower them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AafLimits {
    /// Most objects in the file.
    pub max_objects: usize,
    /// Deepest strong-reference nesting below the root object.
    pub max_depth: usize,
    /// Largest `properties` stream of one object, in bytes.
    pub max_properties_bytes: u64,
    /// Most entries in one index stream.
    pub max_index_entries: usize,
    /// Longest mob chain one clip may follow.
    pub max_mob_chain: usize,
    /// Most control points in one curve.
    pub max_control_points: usize,
    /// Longest single text, in bytes.
    pub max_text_bytes: usize,
    /// Most audio tracks the file may give.
    pub max_import_tracks: usize,
    /// Most bytes the object store (and the read that builds it) holds,
    /// counted as objects are read.
    pub max_model_bytes: usize,
}

impl Default for AafLimits {
    fn default() -> Self {
        Self {
            max_objects: MAX_AAF_OBJECTS,
            max_depth: MAX_REF_DEPTH,
            max_properties_bytes: MAX_PROPERTIES_BYTES,
            max_index_entries: MAX_INDEX_ENTRIES,
            max_mob_chain: MAX_MOB_CHAIN,
            max_control_points: MAX_CONTROL_POINTS,
            max_text_bytes: MAX_TEXT_BYTES,
            max_import_tracks: MAX_IMPORT_TRACKS,
            max_model_bytes: AAF_READ_BUDGET,
        }
    }
}
