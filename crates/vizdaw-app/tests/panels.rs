//! The file panel specs against Swift's panels (inventory §2.1 rows 1-17,
//! `AppCommands.swift:578-625`, `ProjectPanels.swift:77-276`), the
//! extension a save gets and the replace question the app asks itself
//! (inventory §7.3).

use std::path::{Path, PathBuf};

use vizdaw_app::panels::{
    self, ALL, FileType, PanelKind, PanelSpec, REPLACE_ANSWERS, SaveTarget, StartFolder,
    replace_question, save_target,
};

/// (row, kind, prompt, message, multiple, can create folders, default name)
type Row = (
    &'static str,
    PanelKind,
    &'static str,
    Option<&'static str>,
    bool,
    bool,
    Option<&'static str>,
);

const SWIFT: &[Row] = &[
    (
        "1",
        PanelKind::Open,
        "Open",
        Some("Choose a VizDAW project, a MIDI file, a DAWproject file or an AAF file"),
        false,
        false,
        None,
    ),
    (
        "2",
        PanelKind::Open,
        "Open Copy",
        Some("Choose a saved Co-Producer copy: it opens as a new untitled project."),
        false,
        false,
        None,
    ),
    (
        "3",
        PanelKind::Open,
        "Import",
        Some("Choose a MIDI file: its tracks are added at the playhead."),
        false,
        false,
        None,
    ),
    (
        "4",
        PanelKind::Save,
        "Save",
        None,
        false,
        true,
        Some("{name}.vizdaw"),
    ),
    (
        "5",
        PanelKind::Open,
        "Relink",
        Some("Choose the audio file for \u{201c}{name}\u{201d}."),
        false,
        false,
        None,
    ),
    (
        "6",
        PanelKind::Folder,
        "Search",
        Some("Choose a folder to search for the missing audio files."),
        false,
        false,
        None,
    ),
    (
        "7",
        PanelKind::Open,
        "Import",
        Some("Choose audio files: they are converted into the project's format."),
        true,
        false,
        None,
    ),
    (
        "8",
        PanelKind::Save,
        "Bounce",
        None,
        false,
        true,
        Some("Bounce.{ext}"),
    ),
    (
        "9a",
        PanelKind::Folder,
        "Bounce",
        Some("Choose a folder for the stems."),
        false,
        true,
        None,
    ),
    (
        "9b",
        PanelKind::Folder,
        "Export",
        Some("Choose a folder: each track becomes one MIDI Clip File in it."),
        false,
        true,
        None,
    ),
    (
        "9c",
        PanelKind::Folder,
        "Export Here",
        Some("Choose where to put the folder for another DAW."),
        false,
        true,
        None,
    ),
    (
        "9d",
        PanelKind::Folder,
        "Export Here",
        Some("Choose where to put the folder \u{201c}{name}\u{201d}."),
        false,
        true,
        None,
    ),
    (
        "10",
        PanelKind::Save,
        "Export",
        None,
        false,
        true,
        Some("{name}.{ext}"),
    ),
    (
        "11",
        PanelKind::Open,
        "Import",
        Some("Choose a DAWproject file: it opens as a new project."),
        false,
        false,
        None,
    ),
    (
        "12",
        PanelKind::Open,
        "Merge",
        Some("Choose the VizDAW project this DAWproject came from."),
        false,
        false,
        None,
    ),
    (
        "13",
        PanelKind::Save,
        "Export",
        None,
        false,
        true,
        Some("{name}"),
    ),
    (
        "14",
        PanelKind::Open,
        "Import",
        Some("Choose an AAF file."),
        false,
        false,
        None,
    ),
    (
        "15",
        PanelKind::Open,
        "Choose",
        Some("Choose the MIDI file that goes with the AAF file."),
        false,
        false,
        None,
    ),
    (
        "16",
        PanelKind::Folder,
        "Choose",
        Some("Choose a folder to look for the missing audio files in."),
        false,
        false,
        None,
    ),
    (
        "17",
        PanelKind::Save,
        "Save",
        None,
        false,
        true,
        Some("MIDI Report.txt"),
    ),
];

#[test]
fn every_swift_panel_has_its_spec_with_swifts_strings() {
    assert_eq!(ALL.len(), SWIFT.len());
    for (spec, row) in ALL.iter().zip(SWIFT) {
        let got: Row = (
            spec.row,
            spec.kind,
            spec.prompt,
            spec.message,
            spec.multiple,
            spec.can_create_folders,
            spec.default_name,
        );
        assert_eq!(&got, row, "row {}", row.0);
    }
}

#[test]
fn the_specs_allow_swifts_types() {
    let types = |s: &PanelSpec| s.types.to_vec();
    use FileType::*;
    assert_eq!(
        types(&panels::OPEN_PROJECT),
        vec![Project, Folder, Midi, Dawproject, Aaf]
    );
    assert_eq!(types(&panels::OPEN_COPY), vec![Project]);
    assert_eq!(types(&panels::IMPORT_MIDI), vec![Midi]);
    assert_eq!(types(&panels::SAVE_PROJECT), vec![]);
    assert_eq!(types(&panels::RELINK), vec![Audio]);
    assert_eq!(types(&panels::IMPORT_AUDIO), vec![Audio]);
    assert_eq!(types(&panels::BOUNCE_FILE), vec![GivenExtension]);
    assert_eq!(types(&panels::MIDI_EXPORT_FILE), vec![GivenExtension]);
    assert_eq!(types(&panels::IMPORT_DAWPROJECT), vec![Dawproject]);
    assert_eq!(types(&panels::MERGE_PROJECT), vec![Project, Folder]);
    assert_eq!(types(&panels::DAWPROJECT_EXPORT_FILE), vec![Dawproject]);
    assert_eq!(types(&panels::IMPORT_AAF), vec![Aaf]);
    assert_eq!(types(&panels::MIDI_BESIDE_AAF), vec![Midi]);
    assert_eq!(types(&panels::REPORT_TEXT), vec![PlainText]);
    for folder in ALL.iter().filter(|s| s.kind == PanelKind::Folder) {
        assert!(folder.types.is_empty(), "row {}", folder.row);
    }
    assert_eq!(Midi.extensions(), &["mid", "midi", "smf", "midi2"]);
    assert_eq!(Project.identifiers(), &["org.vizdaw.project"]);
}

#[test]
fn files_and_folders_are_chosen_as_swift_chooses_them() {
    // canChooseFiles / canChooseDirectories.
    let choose = |s: &PanelSpec| (s.chooses_files(), s.chooses_folders());
    assert_eq!(choose(&panels::OPEN_PROJECT), (true, true));
    assert_eq!(choose(&panels::OPEN_COPY), (true, true));
    assert_eq!(choose(&panels::MERGE_PROJECT), (true, true));
    assert_eq!(choose(&panels::IMPORT_MIDI), (true, false));
    assert_eq!(choose(&panels::IMPORT_AUDIO), (true, false));
    assert_eq!(choose(&panels::SEARCH_FOLDER), (false, true));
    assert_eq!(choose(&panels::BOUNCE_FOLDER), (false, true));
    assert_eq!(choose(&panels::SAVE_PROJECT), (false, false));
}

#[test]
fn names_messages_and_start_folders_fill_in() {
    assert_eq!(
        panels::RELINK.message_for("Kick").as_deref(),
        Some("Choose the audio file for \u{201c}Kick\u{201d}.")
    );
    assert_eq!(
        panels::AAF_EXPORT_FOLDER.message_for("Song AAF").as_deref(),
        Some("Choose where to put the folder \u{201c}Song AAF\u{201d}.")
    );
    assert_eq!(
        panels::SAVE_PROJECT.default_name_for("Song", "").as_deref(),
        Some("Song.vizdaw")
    );
    assert_eq!(
        panels::BOUNCE_FILE
            .default_name_for("Song", "aif")
            .as_deref(),
        Some("Bounce.aif")
    );
    assert_eq!(
        panels::MIDI_EXPORT_FILE
            .default_name_for("Bass", "midi2")
            .as_deref(),
        Some("Bass.midi2")
    );
    assert_eq!(
        panels::DAWPROJECT_EXPORT_FILE
            .default_name_for("Song.dawproject", "")
            .as_deref(),
        Some("Song.dawproject")
    );
    assert_eq!(
        panels::REPORT_TEXT
            .default_name_for("AAF Report", "")
            .as_deref(),
        Some("MIDI Report.txt")
    );
    assert_eq!(panels::OPEN_COPY.start, StartFolder::CoProducerCopies);
    assert_eq!(panels::SAVE_PROJECT.start, StartFolder::Given);
    assert_eq!(
        panels::co_producer_copies(Path::new("/m/Song.vizdaw")),
        PathBuf::from("/m/Song.vizdaw/Co-Producer/Copies")
    );
}

fn target(spec: &PanelSpec, chosen: &str, ext: &str, taken: &[&str]) -> SaveTarget {
    save_target(spec, Path::new(chosen), ext, |p| {
        taken.iter().any(|t| Path::new(t) == p)
    })
}

#[test]
fn a_save_gets_its_extension_and_asks_only_when_it_added_one_over_a_file() {
    let t = |path: &str, ask| SaveTarget {
        path: PathBuf::from(path),
        ask_replace: ask,
    };
    let p = &panels::SAVE_PROJECT;
    assert_eq!(target(p, "/m/Song", "", &[]), t("/m/Song.vizdaw", false));
    assert_eq!(
        target(p, "/m/Song", "", &["/m/Song.vizdaw"]),
        t("/m/Song.vizdaw", true)
    );
    // Typed with the extension: the panel asked already.
    assert_eq!(
        target(p, "/m/Song.vizdaw", "", &["/m/Song.vizdaw"]),
        t("/m/Song.vizdaw", false)
    );
    let b = &panels::BOUNCE_FILE;
    assert_eq!(target(b, "/m/Mix", "wav", &[]), t("/m/Mix.wav", false));
    assert_eq!(
        target(b, "/m/Mix.aiff", "aif", &[]),
        t("/m/Mix.aiff", false)
    );
    assert_eq!(
        target(b, "/m/Mix", "caf", &["/m/Mix.caf"]),
        t("/m/Mix.caf", true)
    );
    let m = &panels::MIDI_EXPORT_FILE;
    assert_eq!(
        target(m, "/m/Bass", "midi2", &[]),
        t("/m/Bass.midi2", false)
    );
    assert_eq!(
        target(m, "/m/Song.MID", "mid", &[]),
        t("/m/Song.MID", false)
    );
    let d = &panels::DAWPROJECT_EXPORT_FILE;
    assert_eq!(
        target(d, "/m/Song v2", "", &["/m/Song v2.dawproject"]),
        t("/m/Song v2.dawproject", true)
    );
    let r = &panels::REPORT_TEXT;
    assert_eq!(
        target(r, "/m/MIDI Report", "", &[]),
        t("/m/MIDI Report.txt", false)
    );
    // An open panel's path is as chosen.
    assert_eq!(
        target(&panels::OPEN_PROJECT, "/m/Song", "", &["/m/Song"]),
        t("/m/Song", false)
    );
}

#[test]
fn the_replace_question_names_the_file() {
    assert_eq!(
        replace_question(&panels::SAVE_PROJECT, Path::new("/m/Song.vizdaw")),
        (
            "\u{201c}Song.vizdaw\u{201d} already exists. Do you want to replace it?".to_owned(),
            "Replacing it overwrites the project that is there.".to_owned()
        )
    );
    assert_eq!(
        replace_question(&panels::BOUNCE_FILE, Path::new("/m/Mixes/Bounce.wav")),
        (
            "\u{201c}Bounce.wav\u{201d} already exists. Do you want to replace it?".to_owned(),
            "A file or folder with the same name already exists in the folder Mixes. \
             Replacing it will overwrite its current contents."
                .to_owned()
        )
    );
    // Cancel first: the default (Return) and Esc; Replace must be clicked.
    assert_eq!(REPLACE_ANSWERS, ["Cancel", "Replace"]);
}

#[test]
fn only_the_project_save_gets_the_project_wording() {
    // Another save that also ends in `.vizdaw` is not the project's panel.
    let other = PanelSpec {
        row: "x",
        ..panels::SAVE_PROJECT
    };
    let (_, detail) = replace_question(&other, Path::new("/m/Song.vizdaw"));
    assert!(detail.starts_with("A file or folder"), "{detail}");
}
