//! Swift `TransportFieldsTests`' punch rows and `TransportRowTests`'
//! field keys: the Punch In and Out fields, the Tab order and where P, E
//! and U take the keyboard.

mod support;

use std::sync::Arc;

use support::{demo_fake, four_four};
use vizdaw_app::DocumentModel;
use vizdaw_app::transport_bar::{Size, TransportItem};
use vizdaw_app::transport_fields::{
    FieldKeyTarget, PUNCH_LIMIT, TransportField, commit_field, field_key, field_text, item_holding,
    tab_target,
};
use vizdaw_ffi::events::MeterEntryRow;

const BAR: i64 = 61_440;

fn model() -> DocumentModel {
    DocumentModel::new(Arc::new(demo_fake())).unwrap()
}

#[test]
fn the_fields_are_in_tab_order_with_swifts_titles() {
    use TransportField::*;
    assert_eq!(
        TransportField::ALL,
        [Position, EditIn, EditOut, PunchIn, PunchOut]
    );
    let titles: Vec<_> = TransportField::ALL.iter().map(|f| f.title()).collect();
    assert_eq!(
        titles,
        ["Position", "Edit In", "Edit Out", "Punch In", "Punch Out"]
    );
}

#[test]
fn punch_fields_keep_their_order() {
    let mut m = model();
    let marks = |m: &DocumentModel| {
        (
            m.layout().transport.punch_in,
            m.layout().transport.punch_out,
        )
    };
    assert_eq!(marks(&m), (0, 4 * BAR));
    assert_eq!(field_text(&m, TransportField::PunchIn), "1.1.0");
    assert_eq!(field_text(&m, TransportField::PunchOut), "5.1.0");
    assert!(commit_field(&mut m, TransportField::PunchIn, "2"));
    assert_eq!(marks(&m), (BAR, 4 * BAR));
    assert!(
        commit_field(&mut m, TransportField::PunchIn, "9"),
        "past Punch Out: it moves a bar after"
    );
    assert_eq!(marks(&m), (8 * BAR, 9 * BAR));
    assert!(!commit_field(&mut m, TransportField::PunchOut, "8"));
    assert_eq!(m.status(), Some("Punch Out must be after Punch In."));
    assert!(commit_field(&mut m, TransportField::PunchOut, "12"));
    assert_eq!(marks(&m), (8 * BAR, 11 * BAR));
    assert!(!commit_field(&mut m, TransportField::PunchIn, "x"));
    assert_eq!(
        m.status(),
        Some("\u{201c}x\u{201d} is not a position (bar.beat.unit).")
    );
    assert!(!m.can_undo(), "the marks are no undo step");
}

#[test]
fn a_punch_range_past_the_limit_is_refused() {
    // 255/1 bars are 15 667 200 ticks: bar 70 180 starts within the
    // limit, and a bar after it is past it.
    let f = demo_fake();
    let mut info = four_four(3);
    info.meter = vec![MeterEntryRow {
        tick: 0,
        num: 255,
        den: 1,
        sharps: 0,
        minor: false,
    }];
    f.set_info(3, info);
    let mut m = DocumentModel::new(Arc::new(f)).unwrap();
    assert_eq!(m.selected_seq(), Some(3));
    let before = m.layout().transport.clone();
    assert!(
        !commit_field(&mut m, TransportField::PunchIn, "70180"),
        "Punch Out would pass the limit"
    );
    assert_eq!(m.layout().transport, before);
    assert_eq!(m.status(), Some(PUNCH_LIMIT));
    assert_eq!(
        PUNCH_LIMIT,
        "The punch range must end by the last position."
    );
}

#[test]
fn tab_moves_through_the_shown_fields_with_punch() {
    use TransportField::*;
    let all = TransportField::ALL;
    assert_eq!(
        tab_target(&all, EditIn, false),
        Some(EditOut),
        "Tab: Edit Out"
    );
    assert_eq!(tab_target(&all, EditOut, true), Some(EditIn), "Shift-Tab");
    assert_eq!(
        tab_target(&all, PunchIn, false),
        Some(PunchOut),
        "Tab: Punch Out"
    );
    assert_eq!(tab_target(&all, EditOut, false), Some(PunchIn));
    assert_eq!(tab_target(&all, PunchOut, false), Some(Position), "round");
    assert_eq!(tab_target(&all, Position, true), Some(PunchOut));
    // An absent field's own two go round each other.
    let punch = [PunchIn, PunchOut];
    assert_eq!(tab_target(&punch, PunchOut, false), Some(PunchIn));
    assert_eq!(tab_target(&[Position], Position, false), None);
    assert_eq!(tab_target(&punch, Position, false), None);
}

#[test]
fn the_field_keys_open_their_field() {
    use TransportField::*;
    let keys = [
        (Position, TransportItem::PositionField),
        (EditIn, TransportItem::EditFields),
        (PunchIn, TransportItem::PunchFields),
    ];
    for (field, item) in keys {
        assert_eq!(item_holding(field), item);
        let all = vec![TransportItem::Cluster, item.clone(), TransportItem::Loop];
        assert_eq!(
            field_key(field, &all, &[]),
            Some(FieldKeyTarget::OnRow),
            "{field:?} on the row"
        );
        assert_eq!(
            field_key(field, &all, &[item.clone(), TransportItem::Loop]),
            Some(FieldKeyTarget::Folded(item.clone())),
            "{field:?} folded"
        );
        let without: Vec<_> = all.iter().filter(|i| **i != item).cloned().collect();
        assert_eq!(
            field_key(field, &without, &without),
            None,
            "{field:?} absent"
        );
    }
    assert_eq!(item_holding(EditOut), TransportItem::EditFields);
    assert_eq!(item_holding(PunchOut), TransportItem::PunchFields);
    // The standard bar has no Punch fields.
    let standard = vizdaw_app::transport_bar::TransportLayout::standard().items;
    assert_eq!(field_key(PunchIn, &standard, &[]), None);
    assert_eq!(
        field_key(Position, &standard, &[TransportItem::Bar(Size::Large)]),
        Some(FieldKeyTarget::OnRow)
    );
}
