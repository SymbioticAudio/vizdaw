//! `vizdaw-play`: play a VizDAW project sequence through the built-in synth.

mod args;
#[cfg(target_os = "macos")]
mod live;
mod offline;
mod project;
#[cfg(target_os = "macos")]
mod signal;

use args::Args;
use clap::Parser;
use project::Choice;
use std::process::ExitCode;

#[cfg(feature = "rt-check")]
#[global_allocator]
static ALLOC: core_engine::rt_check::CountingAlloc = core_engine::rt_check::CountingAlloc;

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: &Args) -> Result<ExitCode, String> {
    if let Some(path) = &args.write_demo {
        let doc = core_engine::fixtures::demo_document().map_err(|e| e.to_string())?;
        core_model::file::save(&doc, path).map_err(|e| e.to_string())?;
        println!("wrote {}", path.display());
        return Ok(ExitCode::SUCCESS);
    }
    let Some(path) = &args.project else {
        return Err("no project given".into());
    };
    let doc = project::load(path)?;
    if args.list {
        list(&doc);
        return Ok(ExitCode::SUCCESS);
    }
    let seq = match project::choose(&doc, args.seq.as_deref()) {
        Choice::Play(s) => s,
        Choice::Unknown(name) => return Err(format!("no sequence named {name:?}")),
        Choice::Ambiguous(names) => {
            eprintln!("choose a sequence with --seq:");
            for n in names {
                eprintln!("  {n}");
            }
            return Ok(ExitCode::from(2));
        }
    };
    if let Some(out) = &args.offline {
        let (report, text) = offline::render(&doc, seq, args.to.into(), args.looped)?;
        for w in &report.warnings {
            eprintln!("warning: {w}");
        }
        std::fs::write(out, text).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
        eprintln!("wrote {} ({} event(s))", out.display(), report.events);
        return Ok(ExitCode::SUCCESS);
    }
    play(args, &doc, seq)?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(target_os = "macos")]
fn play(
    args: &Args,
    doc: &core_model::document::Document,
    seq: core_model::ids::SeqId,
) -> Result<(), String> {
    live::run(args, doc, seq)?;
    #[cfg(feature = "rt-check")]
    eprintln!(
        "rt-check: {} allocation(s) on the render path",
        core_engine::rt_check::count()
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn play(
    _: &Args,
    _: &core_model::document::Document,
    _: core_model::ids::SeqId,
) -> Result<(), String> {
    Err("live playback needs macOS; use --offline".into())
}

fn list(doc: &core_model::document::Document) {
    println!("sequences:");
    for s in project::describe_sequences(doc) {
        println!("  {s}");
    }
    #[cfg(target_os = "macos")]
    {
        println!("audio outputs:");
        for d in platform_macos::hal::output_devices() {
            println!("  {d}");
        }
        println!("MIDI destinations:");
        for p in platform_macos::midi::list_destinations() {
            println!("  {}", p.name);
        }
    }
}
