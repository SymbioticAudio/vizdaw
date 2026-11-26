//! Prints the command registry as Markdown checklist rows
//! (`| id | menu | title | shortcut | menu item | Mac | Linux |`), so the
//! parity checklist can be regenerated:
//! `cargo run -p core-control --example dump_commands`.

use core_control::registry::COMMANDS;

fn main() {
    for c in COMMANDS {
        println!(
            "| {} | {} | {} | {} | {} | | |",
            c.id,
            c.menu,
            c.title,
            c.shortcut.unwrap_or(""),
            c.menu_item
        );
    }
}
