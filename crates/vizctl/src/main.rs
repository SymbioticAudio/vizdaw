//! The `vizctl` command (see `vizctl help`).
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(vizctl::cli::run(&args));
}
