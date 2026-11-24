//! The file panels the app shows (slice 7b Task 1; inventory §2.1 rows
//! 1-17, Swift `Panels` in `AppCommands.swift` and `ProjectPanels.swift`):
//! one [`PanelSpec`] per Swift panel with its exact strings, and the pure
//! checks every chosen path goes through (the extension a save gets, the
//! replace question the app asks itself). The GPUI crate turns a spec into
//! a macOS `NSOpenPanel`/`NSSavePanel` or a Linux portal request.

use std::path::{Path, PathBuf};

use crate::formats::with_extension;

/// What a panel chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    /// Files (and, on macOS, the folders [`PanelSpec::types`] allows).
    Open,
    /// A new file's path.
    Save,
    /// Folders only.
    Folder,
}

/// A type a panel allows (Swift's `allowedContentTypes` entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    /// A `.vizdaw` project (`org.vizdaw.project`; a folder until the
    /// bundle declares the type).
    Project,
    /// Any folder (`UTType.folder`).
    Folder,
    /// A Standard MIDI File or MIDI Clip File (`UTType.midi` and
    /// mid/midi/smf/midi2).
    Midi,
    /// A DAWproject file (`org.vizdaw.dawproject` and `dawproject`).
    Dawproject,
    /// An AAF file (`org.vizdaw.aaf` and `aaf`).
    Aaf,
    /// Any audio file the system reads (`UTType.audio`).
    Audio,
    /// Plain text (`UTType.plainText`).
    PlainText,
    /// The type of the extension the caller gives (Swift
    /// `UTType(filenameExtension: ext)`: Bounce and MIDI exports).
    GivenExtension,
}

impl FileType {
    /// The file extensions of this type (without the dot).
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileType::Project => &["vizdaw"],
            FileType::Midi => &["mid", "midi", "smf", "midi2"],
            FileType::Dawproject => &["dawproject"],
            FileType::Aaf => &["aaf"],
            FileType::Audio => &[
                "wav", "wave", "aif", "aiff", "aifc", "caf", "mp3", "m4a", "aac", "flac",
            ],
            FileType::PlainText => &["txt"],
            FileType::Folder | FileType::GivenExtension => &[],
        }
    }

    /// The macOS type identifiers of this type, before its extensions
    /// (Swift lists the declared type and the extension's type both).
    pub fn identifiers(self) -> &'static [&'static str] {
        match self {
            FileType::Project => &["org.vizdaw.project"],
            FileType::Folder => &["public.folder"],
            FileType::Midi => &["public.midi-audio"],
            FileType::Dawproject => &["org.vizdaw.dawproject"],
            FileType::Aaf => &["org.vizdaw.aaf"],
            FileType::Audio => &["public.audio"],
            FileType::PlainText => &["public.plain-text"],
            FileType::GivenExtension => &[],
        }
    }

    /// Words naming this type in a Linux file filter (the portal shows
    /// the filter's label; macOS shows none).
    pub fn label(self) -> &'static str {
        match self {
            FileType::Project => "VizDAW project",
            FileType::Folder => "folder",
            FileType::Midi => "MIDI file",
            FileType::Dawproject => "DAWproject file",
            FileType::Aaf => "AAF file",
            FileType::Audio => "audio file",
            FileType::PlainText => "text file",
            FileType::GivenExtension => "file",
        }
    }
}

/// Where a panel starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartFolder {
    /// Wherever the system starts (Swift sets no `directoryURL`).
    System,
    /// The folder the caller gives (Save As: the project's folder, else
    /// the home folder, as slice 1 does).
    Given,
    /// `<package>/Co-Producer/Copies` of the caller's project package.
    CoProducerCopies,
}

/// The extension a save panel's file must end in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveExtension {
    /// Always this one (with the other spellings it accepts as typed).
    Fixed(&'static str, &'static [&'static str]),
    /// The one the caller gives (Bounce `wav|aif|caf`, MIDI `mid|midi2`).
    Given,
}

/// One Swift panel (inventory §2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelSpec {
    /// The §2.1 row (9a-9d are rows 9 with their own strings).
    pub row: &'static str,
    pub kind: PanelKind,
    /// The default button's title (`panel.prompt`; Save's default `Save`).
    pub prompt: &'static str,
    /// `panel.message`; `{name}` stands for the caller's name.
    pub message: Option<&'static str>,
    pub types: &'static [FileType],
    pub multiple: bool,
    /// `canCreateDirectories`.
    pub can_create_folders: bool,
    pub start: StartFolder,
    /// The suggested name; `{name}` and `{ext}` stand for the caller's.
    pub default_name: Option<&'static str>,
    /// Save panels: the extension the chosen path must end in.
    pub save_extension: Option<SaveExtension>,
}

impl PanelSpec {
    /// The message with `name` in place of `{name}`.
    pub fn message_for(&self, name: &str) -> Option<String> {
        self.message.map(|m| m.replace("{name}", name))
    }

    /// The suggested file name with `name` and `ext` filled in.
    pub fn default_name_for(&self, name: &str, ext: &str) -> Option<String> {
        self.default_name
            .map(|n| n.replace("{name}", name).replace("{ext}", ext))
    }

    /// Whether the panel chooses files (Swift `canChooseFiles`).
    pub fn chooses_files(&self) -> bool {
        self.kind == PanelKind::Open
    }

    /// Whether the panel chooses folders: a folder panel, or an open panel
    /// that allows projects or folders (`canChooseDirectories`).
    pub fn chooses_folders(&self) -> bool {
        match self.kind {
            PanelKind::Folder => true,
            PanelKind::Save => false,
            PanelKind::Open => self
                .types
                .iter()
                .any(|t| matches!(t, FileType::Project | FileType::Folder)),
        }
    }

    /// The extension a save's chosen path must end in and the spellings
    /// accepted as typed; `given` is the caller's (`SaveExtension::Given`).
    pub fn save_extension_for(&self, given: &str) -> Option<(String, Vec<String>)> {
        let (ext, accepted) = match self.save_extension? {
            SaveExtension::Fixed(ext, accepted) => (ext, accepted.to_vec()),
            SaveExtension::Given => (given, accepted_spellings(given)),
        };
        Some((
            ext.to_owned(),
            accepted.into_iter().map(str::to_owned).collect(),
        ))
    }
}

/// The spellings a typed name may end in for `ext` (Bounce's `aif`
/// container also takes `aiff`, `BounceCommands.swift:69`).
fn accepted_spellings(ext: &str) -> Vec<&str> {
    if ext.eq_ignore_ascii_case("aif") {
        vec![ext, "aiff"]
    } else {
        vec![ext]
    }
}

const PROJECT_OPEN_TYPES: &[FileType] = &[
    FileType::Project,
    FileType::Folder,
    FileType::Midi,
    FileType::Dawproject,
    FileType::Aaf,
];

const fn open(row: &'static str, prompt: &'static str, message: &'static str) -> PanelSpec {
    PanelSpec {
        row,
        kind: PanelKind::Open,
        prompt,
        message: Some(message),
        types: &[],
        multiple: false,
        can_create_folders: false,
        start: StartFolder::System,
        default_name: None,
        save_extension: None,
    }
}

const fn folder(
    row: &'static str,
    prompt: &'static str,
    message: &'static str,
    can_create_folders: bool,
) -> PanelSpec {
    PanelSpec {
        kind: PanelKind::Folder,
        can_create_folders,
        ..open(row, prompt, message)
    }
}

const fn save(
    row: &'static str,
    prompt: &'static str,
    types: &'static [FileType],
    default_name: &'static str,
    save_extension: SaveExtension,
) -> PanelSpec {
    PanelSpec {
        row,
        kind: PanelKind::Save,
        prompt,
        message: None,
        types,
        multiple: false,
        can_create_folders: true,
        start: StartFolder::System,
        default_name: Some(default_name),
        save_extension: Some(save_extension),
    }
}

/// Row 1: File › Open… (`chooseProject`; the message has no full stop).
pub const OPEN_PROJECT: PanelSpec = PanelSpec {
    types: PROJECT_OPEN_TYPES,
    ..open(
        "1",
        "Open",
        "Choose a VizDAW project, a MIDI file, a DAWproject file or an AAF file",
    )
};
/// Row 2: File › Open Co-Producer Copy… (`chooseCoProducerCopy(in:)`).
pub const OPEN_COPY: PanelSpec = PanelSpec {
    types: &[FileType::Project],
    start: StartFolder::CoProducerCopies,
    ..open(
        "2",
        "Open Copy",
        "Choose a saved Co-Producer copy: it opens as a new untitled project.",
    )
};
/// Row 3: File › Import MIDI File… (`chooseMidi`).
pub const IMPORT_MIDI: PanelSpec = PanelSpec {
    types: &[FileType::Midi],
    ..open(
        "3",
        "Import",
        "Choose a MIDI file: its tracks are added at the playhead.",
    )
};
/// Row 4: Save / Save As (`chooseSaveLocation(title:)`; no types: the app adds
/// `.vizdaw`).
pub const SAVE_PROJECT: PanelSpec = PanelSpec {
    start: StartFolder::Given,
    ..save(
        "4",
        "Save",
        &[],
        "{name}.vizdaw",
        SaveExtension::Fixed("vizdaw", &["vizdaw"]),
    )
};
/// Row 5: Relink's Locate… (`chooseRelink(name:)`).
pub const RELINK: PanelSpec = PanelSpec {
    types: &[FileType::Audio],
    ..open(
        "5",
        "Relink",
        "Choose the audio file for \u{201c}{name}\u{201d}.",
    )
};
/// Row 6: Relink's Search Folder… (`chooseFolder()`; cannot create folders).
pub const SEARCH_FOLDER: PanelSpec = folder(
    "6",
    "Search",
    "Choose a folder to search for the missing audio files.",
    false,
);
/// Row 7: Import Audio (`chooseAudio`; several files).
pub const IMPORT_AUDIO: PanelSpec = PanelSpec {
    types: &[FileType::Audio],
    multiple: true,
    ..open(
        "7",
        "Import",
        "Choose audio files: they are converted into the project's format.",
    )
};
/// Row 8: Bounce to a file (`chooseBounceFile(name:ext:)`; always `Bounce.<ext>`).
pub const BOUNCE_FILE: PanelSpec = save(
    "8",
    "Bounce",
    &[FileType::GivenExtension],
    "Bounce.{ext}",
    SaveExtension::Given,
);
/// Row 9a: Bounce stems (`chooseBounceFolder`).
pub const BOUNCE_FOLDER: PanelSpec = folder("9a", "Bounce", "Choose a folder for the stems.", true);
/// Row 9b: Export MIDI Clip Files (`chooseMidiExportFolder`).
pub const MIDI_EXPORT_FOLDER: PanelSpec = folder(
    "9b",
    "Export",
    "Choose a folder: each track becomes one MIDI Clip File in it.",
    true,
);
/// Row 9c: Export for Another DAW (`chooseExportFolder`).
pub const FOR_DAW_FOLDER: PanelSpec = folder(
    "9c",
    "Export Here",
    "Choose where to put the folder for another DAW.",
    true,
);
/// Row 9d: Export AAF (`chooseAafExportFolder(name:)`).
pub const AAF_EXPORT_FOLDER: PanelSpec = folder(
    "9d",
    "Export Here",
    "Choose where to put the folder \u{201c}{name}\u{201d}.",
    true,
);
/// Row 10: Export a MIDI file (`chooseMidiExportFile(name:ext:)`; `mid`/`midi2`).
pub const MIDI_EXPORT_FILE: PanelSpec = save(
    "10",
    "Export",
    &[FileType::GivenExtension],
    "{name}.{ext}",
    SaveExtension::Given,
);
/// Row 11: Import Project › DAWproject… (`chooseDawprojectToOpen`).
pub const IMPORT_DAWPROJECT: PanelSpec = PanelSpec {
    types: &[FileType::Dawproject],
    ..open(
        "11",
        "Import",
        "Choose a DAWproject file: it opens as a new project.",
    )
};
/// Row 12: The return prompt's Merge With Existing Project… (`chooseProjectToMerge`).
pub const MERGE_PROJECT: PanelSpec = PanelSpec {
    types: &[FileType::Project, FileType::Folder],
    ..open(
        "12",
        "Merge",
        "Choose the VizDAW project this DAWproject came from.",
    )
};
/// Row 13: Export DAWproject (`chooseDawprojectFile(name:)`; the core's name).
pub const DAWPROJECT_EXPORT_FILE: PanelSpec = save(
    "13",
    "Export",
    &[FileType::Dawproject],
    "{name}",
    SaveExtension::Fixed("dawproject", &["dawproject"]),
);
/// Row 14: Import Project › AAF… (`chooseAafToOpen`).
pub const IMPORT_AAF: PanelSpec = PanelSpec {
    types: &[FileType::Aaf],
    ..open("14", "Import", "Choose an AAF file.")
};
/// Row 15: The AAF import sheet's MIDI file Choose… (`chooseMidiBesideAaf`).
pub const MIDI_BESIDE_AAF: PanelSpec = PanelSpec {
    types: &[FileType::Midi],
    ..open(
        "15",
        "Choose",
        "Choose the MIDI file that goes with the AAF file.",
    )
};
/// Row 16: The AAF import sheet's missing-audio folder (`chooseAafMediaFolder`).
pub const AAF_MEDIA_FOLDER: PanelSpec = folder(
    "16",
    "Choose",
    "Choose a folder to look for the missing audio files in.",
    false,
);
/// Row 17: The report's Save as Text… (`chooseReportText(name:)`; always
/// `MIDI Report.txt`).
pub const REPORT_TEXT: PanelSpec = save(
    "17",
    "Save",
    &[FileType::PlainText],
    "MIDI Report.txt",
    SaveExtension::Fixed("txt", &["txt"]),
);

/// Every panel of inventory §2.1 rows 1-17, in row order.
pub const ALL: &[PanelSpec] = &[
    OPEN_PROJECT,
    OPEN_COPY,
    IMPORT_MIDI,
    SAVE_PROJECT,
    RELINK,
    SEARCH_FOLDER,
    IMPORT_AUDIO,
    BOUNCE_FILE,
    BOUNCE_FOLDER,
    MIDI_EXPORT_FOLDER,
    FOR_DAW_FOLDER,
    AAF_EXPORT_FOLDER,
    MIDI_EXPORT_FILE,
    IMPORT_DAWPROJECT,
    MERGE_PROJECT,
    DAWPROJECT_EXPORT_FILE,
    IMPORT_AAF,
    MIDI_BESIDE_AAF,
    AAF_MEDIA_FOLDER,
    REPORT_TEXT,
];

/// The folder an [`StartFolder::CoProducerCopies`] panel starts in.
pub fn co_producer_copies(package: &Path) -> PathBuf {
    package.join("Co-Producer").join("Copies")
}

/// A save panel's answer after the app's own checks: the path to write
/// and whether the app must first ask about replacing what is there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveTarget {
    pub path: PathBuf,
    /// The extension was added (the panel never saw this name) and a file
    /// or folder is there: ask [`replace_question`] first.
    pub ask_replace: bool,
}

/// The path a save panel's `chosen` name stands for under `spec`, with
/// `ext` for a [`SaveExtension::Given`] spec; `exists` says whether a
/// path is taken (the file system in the app).
pub fn save_target(
    spec: &PanelSpec,
    chosen: &Path,
    ext: &str,
    exists: impl Fn(&Path) -> bool,
) -> SaveTarget {
    let Some((ext, accepted)) = spec.save_extension_for(ext) else {
        return SaveTarget {
            path: chosen.to_path_buf(),
            ask_replace: false,
        };
    };
    let accepted: Vec<&str> = accepted.iter().map(String::as_str).collect();
    let (path, added) = with_extension(&chosen.to_string_lossy(), &ext, &accepted);
    let path = PathBuf::from(path);
    let ask_replace = added && exists(&path);
    SaveTarget { path, ask_replace }
}

/// The replace question the app asks for `path` (title, detail; buttons
/// `Replace`, `Cancel`). A project keeps slice 1's wording; other files
/// get the wording the Swift save panel asked with (macOS's own).
pub fn replace_question(spec: &PanelSpec, path: &Path) -> (String, String) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = format!("\u{201c}{name}\u{201d} already exists. Do you want to replace it?");
    let detail = if spec.save_extension == SAVE_PROJECT.save_extension {
        "Replacing it overwrites the project that is there.".to_owned()
    } else {
        let folder = path
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!(
            "A file or folder with the same name already exists in the folder {folder}. \
             Replacing it will overwrite its current contents."
        )
    };
    (title, detail)
}

/// The replace question's buttons, the default first.
pub const REPLACE_ANSWERS: [&str; 2] = ["Replace", "Cancel"];
