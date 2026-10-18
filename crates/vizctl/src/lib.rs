//! `vizctl`: drives VizDAW through its control channel from a shell
//! (control spec §4, §10). [`cli`] turns arguments into requests;
//! [`launch`] starts VizDAW with the channel on in a throwaway home.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod cli;
pub mod launch;
