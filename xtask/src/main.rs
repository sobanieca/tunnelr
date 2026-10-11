//! Development tasks, run through the cargo aliases in `.cargo/config.toml`:
//!
//! - `cargo verify`: formatting, lints and tests (run before committing)

use std::process::{Command, ExitCode};

fn cargo(args: &[&str]) -> bool {
    println!("> cargo {}", args.join(" "));
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    Command::new(cargo)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_default();
    let ok = match task.as_str() {
        "verify" => {
            cargo(&["fmt", "--all", "--check"])
                && cargo(&[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ])
                && cargo(&["test", "--workspace"])
        }
        _ => {
            eprintln!("Usage: cargo xtask verify");
            false
        }
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
