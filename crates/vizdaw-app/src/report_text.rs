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
