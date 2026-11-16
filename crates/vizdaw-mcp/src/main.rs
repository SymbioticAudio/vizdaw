//! `vizdaw-mcp`: Claude's app runs it; it speaks MCP on stdin and stdout
//! and reaches VizDAW over the Co-Producer socket. Logs go to stderr only.
//! It exits 0 at the end of stdin.
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

use std::io::{self, BufReader, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
use vizdaw_mcp::rpc::{Line, read_line_capped};
use vizdaw_mcp::{MAX_LINE, Server, support_dir_from_env};

/// What the reader thread hands over.
enum Input {
    Line(Vec<u8>),
    TooLong,
}

/// How long the main loop waits for a line before it calls `tick`.
const TICK: Duration = Duration::from_secs(1);

fn main() {
    let (tx, rx) = mpsc::channel::<Input>();
    let reader = std::thread::Builder::new()
        .name("vizdaw-mcp-stdin".into())
        .spawn(move || {
            let mut r = BufReader::with_capacity(64 * 1024, io::stdin());
            loop {
                let input = match read_line_capped(&mut r, MAX_LINE) {
                    Ok(Some(Line::Full(b))) => Input::Line(b),
                    Ok(Some(Line::TooLong)) => Input::TooLong,
                    Ok(None) | Err(_) => return,
                };
                if tx.send(input).is_err() {
                    return;
                }
            }
        });
    if let Err(e) = reader {
        eprintln!("vizdaw-mcp: cannot read stdin: {e}");
        std::process::exit(1);
    }
    let mut server = Server::new(support_dir_from_env());
    let stdout = io::stdout();
    loop {
        let input = match rx.recv_timeout(TICK) {
            Ok(i) => Some(i),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        // Server handles each message inside its own catch_unwind; this
        // one is a backstop so nothing can end the loop.
        let lines = catch_unwind(AssertUnwindSafe(|| match input {
            None => server.tick(),
            Some(Input::TooLong) => server.too_long(),
            Some(Input::Line(bytes)) => match String::from_utf8(bytes) {
                Ok(text) => server.handle_line(&text),
                Err(_) => server.not_utf8(),
            },
        }))
        .unwrap_or_default();
        let mut out = stdout.lock();
        for line in lines {
            let written = out
                .write_all(line.as_bytes())
                .and_then(|()| out.write_all(b"\n"))
                .and_then(|()| out.flush());
            if written.is_err() {
                // The client is gone.
                return;
            }
        }
    }
}
