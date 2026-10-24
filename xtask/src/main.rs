//! Development tasks, run through the cargo aliases in `.cargo/config.toml`:
//!
//! - `cargo verify`: formatting, lints and tests (run before committing)

use std::process::{Command, ExitCode};

fn is_package_variable_of_xtask(name: &str) -> bool {
    name.starts_with("CARGO_PKG_")
        || name.starts_with("CARGO_MANIFEST_")
        || matches!(
            name,
            "CARGO_CRATE_NAME" | "CARGO_BIN_NAME" | "CARGO_PRIMARY_PACKAGE"
        )
}

fn cargo(args: &[&str]) -> bool {
    println!("> cargo {}", args.join(" "));
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(cargo);
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(is_package_variable_of_xtask) {
            command.env_remove(name);
        }
    }
    command
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
