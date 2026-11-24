//! The report's wording (Swift `ReportText`), shared by the export sheets'
//! "What won't transfer" and the report sheet. Reports say "track", never
//! "lane".

use vizdaw_ffi::{ReportItemRow, ReportKindRow, ReportRow};

/// The groups in the order "Conflicts" (a merge's), "Changed", "Left
/// out", "Notes"; empty ones are omitted.
pub fn groups(r: &ReportRow) -> Vec<(&'static str, Vec<&ReportItemRow>)> {
    const ORDER: [(&str, ReportKindRow); 4] = [
        ("Conflicts", ReportKindRow::Conflict),
        ("Changed", ReportKindRow::Changed),
        ("Left out", ReportKindRow::LeftOut),
        ("Notes", ReportKindRow::Note),
    ];
    ORDER
        .iter()
        .filter_map(|(title, kind)| {
            let items: Vec<&ReportItemRow> = r.items.iter().filter(|i| i.kind == *kind).collect();
            (!items.is_empty()).then_some((*title, items))
        })
        .collect()
}

/// "<what> (<count>)" when it happened more than once.
pub fn line(i: &ReportItemRow) -> String {
    if i.count > 1 {
        format!("{} ({})", i.what, i.count)
    } else {
        i.what.clone()
    }
}

/// The places given, then "and N more" when there were more. An item
/// kept without places shows only its count (as the core's
/// `Report::plain`).
pub fn details(i: &ReportItemRow) -> Vec<String> {
    let mut out = i.details.clone();
    let given = i.details.len() as u64;
    if given > 0 && i.count > given {
        out.push(format!("and {} more", i.count - given));
    }
    out
}

/// The summary, then each group's title and lines with indented details
/// (Copy and Save as Text…).
pub fn plain(r: &ReportRow) -> String {
    let groups = groups(r);
    if groups.is_empty() {
        return format!("{}\n{EVERYTHING_TRANSFERRED}", r.summary);
    }
    let mut lines = vec![r.summary.clone()];
    for (title, items) in groups {
        lines.push(title.to_owned());
        for item in items {
            lines.push(format!("  {}", line(item)));
            lines.extend(details(item).into_iter().map(|d| format!("    {d}")));
        }
    }
    lines.join("\n")
}

/// A group's count: its items' counts added up.
pub fn group_count(items: &[&ReportItemRow]) -> u64 {
    items.iter().map(|i| i.count).sum()
}

/// A group's heading on the sheet, `<title> (<count>)`.
pub fn group_heading(title: &str, items: &[&ReportItemRow]) -> String {
    format!("{title} ({})", group_count(items))
}

/// A group's accessibility label, `<title>, <count>`.
pub fn group_label(title: &str, items: &[&ReportItemRow]) -> String {
    format!("{title}, {}", group_count(items))
}

// ---------------------------------------------------------------------
// The report sheet (Swift `ReportSheet`)

/// The sheet's titles, by [`title`].
pub const EXPORT_REPORT: &str = "Export Report";
pub const MERGE_REPORT: &str = "Merge Report";
pub const IMPORT_REPORT: &str = "Import Report";
pub const REPORT: &str = "Report";
/// Every title the sheet can have.
pub const TITLES: [&str; 4] = [EXPORT_REPORT, MERGE_REPORT, IMPORT_REPORT, REPORT];

/// A report with no group.
pub const EVERYTHING_TRANSFERRED: &str = "Everything transferred.";
pub const COPY: &str = "Copy";
pub const COPY_LABEL: &str = "Copy the report";
pub const SAVE_AS_TEXT: &str = "Save as Text\u{2026}";
pub const SAVE_LABEL: &str = "Save the report as text";
/// Shown only when the export wrote files. Linux says "Show in Folder"
/// (owner ruling, slice 7c).
#[cfg(target_os = "macos")]
pub const SHOW_FILES: &str = "Show in Finder";
#[cfg(target_os = "macos")]
pub const SHOW_FILES_LABEL: &str = "Show the exported files in Finder";
#[cfg(not(target_os = "macos"))]
pub const SHOW_FILES: &str = "Show in Folder";
#[cfg(not(target_os = "macos"))]
pub const SHOW_FILES_LABEL: &str = "Show the exported files in their folder";
/// Esc presses it.
pub const DONE: &str = "Done";
/// File › Last Import or Export Report… with none (the status line).
pub const NO_REPORT_YET: &str = "There is no import or export report yet.";

/// The sheet's title (Swift `ReportSheet.title`): the core's summary
/// starts "Imported", "Exported" or "Merged", or is "Nothing to merge…";
/// a report that wrote files is an export's.
pub fn title(r: &ReportRow, written: &[String]) -> &'static str {
    let s = r.summary.as_str();
    if !written.is_empty() || s.starts_with("Exported") {
        EXPORT_REPORT
    } else if s.starts_with("Merged") || s.starts_with("Nothing to merge") {
        MERGE_REPORT
    } else if s.starts_with("Imported") {
        IMPORT_REPORT
    } else {
        REPORT
    }
}

/// The alert of a report Save as Text… could not write to `path` (Swift
/// `NSAlert(error:)`'s title).
pub fn save_failed_title(path: &std::path::Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("The file \u{201c}{name}\u{201d} couldn\u{2019}t be saved.")
}

/// What Show in Finder reveals of `written` (Swift selects every file in
/// one file manager window; GPUI's `reveal_path` shows one): each folder
/// written into once, with its first file, in order.
pub fn reveal_paths(written: &[String]) -> Vec<&str> {
    let mut folders: Vec<&std::path::Path> = vec![];
    let mut out = vec![];
    for w in written {
        let folder = std::path::Path::new(w)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        if !folders.contains(&folder) {
            folders.push(folder);
            out.push(w.as_str());
        }
    }
    out
}
