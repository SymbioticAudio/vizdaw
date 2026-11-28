#![allow(dead_code)]

#[allow(unused_imports)]
pub use vizdaw_app::testing::{
    FakeSession, block_row, demo_fake, dest_row, drum_fake, editor_fake, ext_dest, four_four,
    group_row, groups_fake, lane_row, mixer_fake, note_row, roll_fake, slot_row, strip_row,
    vca_row,
};

/// The session calls of a drag: begins, gesture edits, ends and aborts.
pub fn edit_calls(f: &FakeSession) -> Vec<String> {
    f.calls()
        .into_iter()
        .filter(|c| c.starts_with("begin_edit") || c == "edit_gesture" || c.ends_with("_edit"))
        .collect()
}
