//! Command-line arguments.

use clap::{Parser, ValueEnum};
use core_engine::schedule::BuiltinRoute;
use std::path::PathBuf;

/// Where `builtin:synth` destinations play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum To {
    /// Apple's built-in DLS synth.
    Builtin,
    /// The "VizDAW Out" virtual MIDI source.
    Virtual,
    /// Both.
    Both,
}

impl From<To> for BuiltinRoute {
    fn from(t: To) -> BuiltinRoute {
        match t {
            To::Builtin => BuiltinRoute::Builtin,
            To::Virtual => BuiltinRoute::Virtual,
            To::Both => BuiltinRoute::Both,
        }
    }
}

/// Plays a VizDAW project sequence through the built-in synth.
#[derive(Debug, Parser)]
#[command(name = "vizdaw-play", version)]
pub struct Args {
    /// The `.vizdaw` package to play.
    #[arg(required_unless_present = "write_demo")]
    pub project: Option<PathBuf>,
    /// Sequence to play (default: the only top-level sequence).
    #[arg(long = "seq", value_name = "NAME")]
    pub seq: Option<String>,
    /// Loop from bar 1 to the end until Ctrl-C.
    #[arg(long = "loop")]
    pub looped: bool,
    /// Output device name (default: follow the system default output).
    #[arg(long, value_name = "NAME")]
    pub device: Option<String>,
    /// IO buffer size in frames.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(32..=2048))]
    pub buffer: Option<u32>,
    /// Where `builtin:synth` destinations play.
    #[arg(long, value_enum, default_value_t = To::Builtin)]
    pub to: To,
    /// Shift MIDI output timestamps (milliseconds, may be negative).
    #[arg(
        long = "midi-offset-ms",
        value_name = "N",
        default_value_t = 0.0,
        allow_negative_numbers = true
    )]
    pub midi_offset_ms: f64,
    /// Render worker threads besides the audio thread (0–15). Default:
    /// performance cores − 1, at most 7, when playing live; 0 offline.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(0..=15))]
    pub workers: Option<u32>,
    /// Render offline (48 kHz, 512-frame blocks) and write the event log.
    #[arg(long, value_name = "OUT.txt")]
    pub offline: Option<PathBuf>,
    /// List sequences, output devices and MIDI destinations, then exit.
    #[arg(long)]
    pub list: bool,
    /// Write the demo project to this package path, then exit.
    #[arg(long = "write-demo", value_name = "DIR/demo.vizdaw", conflicts_with_all = ["project", "offline", "list"])]
    pub write_demo: Option<PathBuf>,
}
