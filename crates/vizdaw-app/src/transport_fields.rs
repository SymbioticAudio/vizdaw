//! The transport bar's Position and Edit In/Out fields (Swift:
//! `TransportFields.swift`): what each shows and what typed text does.
//! Punch In and Out come with recording.

use vizdaw_ffi::events::MeterEntryRow;
use vizdaw_ffi::selection::TickRange;

use crate::alerts::error_message;
use crate::document::DocumentModel;
use crate::position::{Bbu, bar_ticks, parse_position};

/// The text-editing owner while a field is typed in.
pub const TEXT_OWNER: &str = "transport";
/// The farthest position a field takes, either side of 0.
pub const TICK_LIMIT: i64 = 1 << 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportField {
    Position,
    EditIn,
    EditOut,
}

impl TransportField {
    pub const ALL: [TransportField; 3] = [Self::Position, Self::EditIn, Self::EditOut];

    /// Its label (and its accessibility title).
    pub fn title(self) -> &'static str {
        match self {
            Self::Position => "Position",
            Self::EditIn => "Edit In",
            Self::EditOut => "Edit Out",
        }
    }
}

/// The shown sequence's meter (empty: 4/4 from bar 1).
fn shown_meter(doc: &DocumentModel) -> Vec<MeterEntryRow> {
    doc.selected_seq()
        .and_then(|s| doc.sequence_info(s))
        .map(|i| i.meter.clone())
        .unwrap_or_default()
}

/// The tick a field holds; `None` for an edit range that is not set.
pub fn field_tick(doc: &DocumentModel, f: TransportField) -> Option<i64> {
    match f {
        TransportField::Position => Some(doc.last_transport().tick),
        TransportField::EditIn => doc.selection().time_range.map(|r| r.start),
        TransportField::EditOut => doc.selection().time_range.map(|r| r.end),
    }
}

/// A field's text: bar.beat.unit, empty when it holds nothing.
pub fn field_text(doc: &DocumentModel, f: TransportField) -> String {
    let meter = shown_meter(doc);
    field_tick(doc, f)
        .map(|t| Bbu::of(t, &meter).text())
        .unwrap_or_default()
}

/// The tick typed `text` names, read against the field's position (an
/// empty part keeps that part); ":", "," and spaces separate like dots.
pub fn field_input(doc: &DocumentModel, f: TransportField, text: &str) -> Option<i64> {
    let meter = shown_meter(doc);
    let typed: String = text
        .trim()
        .chars()
        .map(|c| if ":, ".contains(c) { '.' } else { c })
        .collect();
    let current = Bbu::of(field_tick(doc, f).unwrap_or(0), &meter);
    parse_position(&typed, current).map(|b| b.tick(&meter))
}

/// Commits typed text to a field. False, with the status line saying why,
/// for unreadable text or an impossible range.
pub fn commit_field(doc: &mut DocumentModel, f: TransportField, text: &str) -> bool {
    let Some(t) = field_input(doc, f, text).filter(|t| t.abs() <= TICK_LIMIT) else {
        doc.set_status(Some(format!(
            "\u{201c}{text}\u{201d} is not a position (bar.beat.unit)."
        )));
        return false;
    };
    let range = doc.selection().time_range;
    match f {
        TransportField::Position => {
            if let Err(e) = doc.move_to(t) {
                doc.set_status(Some(error_message(&e)));
                return false;
            }
        }
        TransportField::EditIn => {
            let bar = bar_ticks(t, &shown_meter(doc));
            let length = range.map_or(bar, |r| r.end - r.start);
            let end = match range {
                Some(r) if r.end > t => r.end,
                _ => t.saturating_add(length),
            };
            doc.set_time_range(TickRange { start: t, end });
        }
        TransportField::EditOut => {
            let Some(r) = range else {
                doc.set_status(Some("Set Edit In first.".into()));
                return false;
            };
            if t <= r.start {
                doc.set_status(Some("Edit Out must be after Edit In.".into()));
                return false;
            }
            doc.set_time_range(TickRange {
                start: r.start,
                end: t,
            });
        }
    }
    true
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::testing::demo_fake;

    const BAR: i64 = 61_440;
    const BEAT: i64 = 15_360;

    #[test]
    fn fields_show_and_take_bar_beat_unit() {
        let fake = Arc::new(demo_fake());
        let mut m = DocumentModel::new(fake.clone()).unwrap();
        fake.set_transport(|t| t.tick = BAR + BEAT);
        m.poll_transport().unwrap();
        assert_eq!(field_text(&m, TransportField::Position), "2.2.0");
        assert_eq!(field_text(&m, TransportField::EditIn), "");
        assert!(commit_field(&mut m, TransportField::Position, "3"));
        assert_eq!(m.poll_transport().unwrap().tick, 2 * BAR);
        assert!(commit_field(&mut m, TransportField::Position, "4:2:480"));
        assert_eq!(m.poll_transport().unwrap().tick, 3 * BAR + BEAT + BEAT / 2);
        assert!(commit_field(&mut m, TransportField::Position, ".3"));
        assert_eq!(
            m.poll_transport().unwrap().tick,
            3 * BAR + 2 * BEAT + BEAT / 2
        );
        assert!(!commit_field(&mut m, TransportField::Position, "x.y"));
        assert_eq!(
            m.status(),
            Some("\u{201c}x.y\u{201d} is not a position (bar.beat.unit).")
        );
        // A bar beyond the typed range is refused, not clamped.
        assert!(!commit_field(&mut m, TransportField::Position, "2000000"));
    }

    #[test]
    fn edit_in_and_out_set_the_selections_range() {
        let mut m = DocumentModel::new(Arc::new(demo_fake())).unwrap();
        let range = |m: &DocumentModel| m.selection().time_range.map(|r| (r.start, r.end));
        assert!(!commit_field(&mut m, TransportField::EditOut, "3"));
        assert_eq!(m.status(), Some("Set Edit In first."));
        assert!(commit_field(&mut m, TransportField::EditIn, "2"));
        assert_eq!(range(&m), Some((BAR, 2 * BAR)), "a bar long");
        assert!(commit_field(&mut m, TransportField::EditOut, "4"));
        assert_eq!(range(&m), Some((BAR, 3 * BAR)));
        assert!(!commit_field(&mut m, TransportField::EditOut, "1"));
        assert_eq!(m.status(), Some("Edit Out must be after Edit In."));
        assert!(commit_field(&mut m, TransportField::EditIn, "5"));
        assert_eq!(
            range(&m),
            Some((4 * BAR, 6 * BAR)),
            "the length moves along"
        );
        assert_eq!(field_text(&m, TransportField::EditIn), "5.1.0");
        assert_eq!(field_text(&m, TransportField::EditOut), "7.1.0");
    }
}
