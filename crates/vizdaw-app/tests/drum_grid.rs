//! Swift `DrumGridModelTests` (the settings) and `DrumGridViewTests` (the
//! labels and headers): the Drum Grid bar's settings, its tree labels and
//! header text, the saved layout and the `drum_lanes` seam.

use serde_json::{Value, json};
use vizdaw_app::DocSession;
use vizdaw_app::drum_grid::{
    DrumGridSettings, DrumView, caption, cell_label, header_text, lengths_including, nudge_title,
    row_label, stepped_velocity,
};
use vizdaw_app::editor::EditorLayout;
use vizdaw_app::testing::{drum_fake, four_four};
use vizdaw_ffi::VizError;
use vizdaw_ffi::drum::{DrumEdit, DrumRowRow};
use vizdaw_ffi::edit::EditCommand;

const Q: i64 = 15_360;

fn decode(json: &str) -> DrumGridSettings {
    EditorLayout::decode(Some(json)).drum_grid
}

#[test]
fn settings_decode_tolerantly() {
    let s = decode(
        r#"{"drumGrid":{"view":"timeline","step":-5,"velocity":200,"rowHeight":1e9,"spacing":"x"}}"#,
    );
    assert_eq!(s.view, DrumView::Timeline);
    assert_eq!(s.step, 3_840);
    assert_eq!(s.velocity, 127);
    assert_eq!(s.row_height, 64.);
    assert_eq!(s.spacing, DrumGridSettings::default().spacing);
    // Step, spacing and duration are what Rust paints with.
    let tiny = decode(r#"{"drumGrid":{"step":1,"spacing":-3,"duration":0}}"#);
    assert_eq!((tiny.step, tiny.spacing, tiny.duration), (480, 480, Q / 8));
    let huge = decode(r#"{"drumGrid":{"step":1000000000000,"spacing":1000000000000000}}"#);
    assert_eq!((huge.step, huge.spacing), (61_440, 16 * Q));
    assert_eq!(decode("{}"), DrumGridSettings::default());
    assert_eq!(
        decode(r#"{"drumGrid":{"view":"spiral"}}"#).view,
        DrumView::Matrix
    );
    assert_eq!(
        lengths_including(999).last(),
        Some(&("999 ticks".to_owned(), 999))
    );
    assert_eq!(lengths_including(Q).len(), 9);
}

#[test]
fn the_settings_survive_a_save() {
    let existing = json!({"drumGrid": {"future": {"x": 1}}, "other": true}).to_string();
    let mut l = EditorLayout::decode(Some(&existing));
    l.drum_grid.view = DrumView::Timeline;
    l.drum_grid.velocity = 64;
    let text = l.merge_into(Some(&existing));
    let saved: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        saved["drumGrid"]["future"],
        json!({"x": 1}),
        "an unknown key stays"
    );
    assert_eq!(saved["drumGrid"]["view"], json!("timeline"));
    assert_eq!(saved["drumGrid"]["velocity"], json!(64));
    assert_eq!(saved["other"], json!(true));
    let back = EditorLayout::decode(Some(&text)).drum_grid;
    assert_eq!(back, l.drum_grid);
}

#[test]
fn cells_and_rows_are_labelled() {
    let meter = four_four(3).meter;
    assert_eq!(
        cell_label("Kick", 0, true, 100 << 9, &meter),
        "Kick, bar 1 beat 1, on, velocity 100"
    );
    assert_eq!(
        cell_label("Snare", 7_680, false, 0, &meter),
        "Snare, bar 1 beat 1 tick 7680, off"
    );
    let lanes = drum_fake().drum_lanes(3).unwrap();
    let perc = &lanes[1];
    assert_eq!(row_label(perc, &perc.rows[0]), "Clap, D#2, Perc, Kit");
    let mut drums = lanes[0].clone();
    drums.destination = None;
    assert_eq!(header_text(&drums), "Drums · No Destination");
    assert_eq!(header_text(&lanes[0]), "Drums · Kit");
    assert_eq!(nudge_title(360), "Nudge: +360 ticks");
    assert_eq!(nudge_title(-240), "Nudge: -240 ticks");
    assert_eq!(nudge_title(0), "Nudge: 0 ticks");
}

#[test]
fn every_row_header_names_its_destination_and_dims_an_inherited_one() {
    let lanes = drum_fake().drum_lanes(3).unwrap();
    let kick = lanes[0].rows[0].clone();
    let own = DrumRowRow {
        dest: Some(7),
        destination: Some("Other".into()),
        ..kick.clone()
    };
    assert_eq!(caption(&own), ("Kick".into(), Some("Other".into()), false));
    assert_eq!(caption(&kick), ("Kick".into(), Some("Kit".into()), true));
}

#[test]
fn the_velocity_stepper_steps_by_four() {
    assert_eq!(stepped_velocity(100, true), 104);
    assert_eq!(stepped_velocity(125, true), 127);
    assert_eq!(stepped_velocity(3, false), 1);
    assert_eq!(stepped_velocity(100, false), 96);
}

#[test]
fn the_fake_reads_drum_lanes() {
    let f = drum_fake();
    let lanes = f.drum_lanes(3).unwrap();
    let names: Vec<&str> = lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Drums", "Perc"]);
    assert_eq!(f.drum_lanes_reads(), 1);
    assert!(f.calls().iter().any(|c| c == "drum_lanes"));
    f.fail_next(
        "drum_lanes",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    assert!(matches!(f.drum_lanes(3), Err(VizError::Busy { .. })));
    assert_eq!(f.drum_lanes(3).unwrap().len(), 2, "once only");
    f.set_call_error(
        "drum_lanes",
        Some(VizError::Busy {
            message: "busy".into(),
        }),
    );
    assert!(f.drum_lanes(3).is_err());
    f.set_call_error("drum_lanes", None);
    // Marking a track gives it a drum layout, as the core does.
    f.edit(EditCommand::Drum {
        edit: DrumEdit::MarkLane {
            seq: 3,
            lane: 30,
            drum: true,
        },
    })
    .unwrap();
    assert!(f.lanes_of(3).iter().any(|l| l.id == 30 && l.is_drum));
}
