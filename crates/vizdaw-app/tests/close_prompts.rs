//! The close and quit questions (Swift `ClosePrompts`): while a Save made
//! during recording is still owed, the close question offers no Don't
//! Save and says the take and the changes will be saved; the quit
//! question names the songs that are saved anyway.

use vizdaw_app::close_prompts::{
    CANCEL, DONT_SAVE, SAVE, SAVE_PENDING_INFORMATIVE, close_prompt, quit_detail,
};

#[test]
fn the_close_question_offers_dont_save_only_with_no_save_owed() {
    let p = close_prompt("Song", false);
    // Swift `M/ClosePrompts.swift:23-26`.
    assert_eq!(
        p.message,
        "Do you want to save the changes you made to \u{201c}Song\u{201d}?"
    );
    assert_eq!(
        p.detail,
        "Your changes will be lost if you don't save them."
    );
    assert_eq!(p.buttons, vec![SAVE, DONT_SAVE, CANCEL]);
    let p = close_prompt("Song", true);
    assert_eq!(
        p.message,
        "Do you want to save the changes you made to \u{201c}Song\u{201d}?"
    );
    assert_eq!(p.detail, SAVE_PENDING_INFORMATIVE);
    assert_eq!(
        SAVE_PENDING_INFORMATIVE,
        "Recording is still going. The take and your changes will be saved when it stops."
    );
    assert_eq!(p.buttons, vec![SAVE, CANCEL]);
}

#[test]
fn the_quit_question_names_the_songs_saved_after_their_pass() {
    let base = "Some documents have unsaved changes.";
    assert_eq!(quit_detail(&[]), base);
    assert_eq!(
        quit_detail(&["Take".into()]),
        format!(
            "{base} If you discard changes, \u{201c}Take\u{201d} will still be saved when its recording ends."
        )
    );
    assert_eq!(
        quit_detail(&["A".into(), "B".into(), "C".into()]),
        format!(
            "{base} If you discard changes, \u{201c}A\u{201d}, \u{201c}B\u{201d} and \u{201c}C\u{201d} will still be saved when their recordings end."
        )
    );
}
