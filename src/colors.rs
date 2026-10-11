//! ANSI colors with the semantics of `@std/fmt/colors`, disabled when
//! `NO_COLOR` is set.

use std::sync::OnceLock;

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()))
}

fn run(s: &str, open: u8, close: u8) -> String {
    if !enabled() {
        return s.to_string();
    }
    let open = format!("\x1b[{open}m");
    let close = format!("\x1b[{close}m");
    format!("{open}{}{close}", s.replace(&close, &open))
}

pub fn gray(s: &str) -> String {
    run(s, 90, 39)
}

pub fn yellow(s: &str) -> String {
    run(s, 33, 39)
}

pub fn bright_red(s: &str) -> String {
    run(s, 91, 39)
}

pub fn bright_yellow(s: &str) -> String {
    run(s, 93, 39)
}

pub fn bright_blue(s: &str) -> String {
    run(s, 94, 39)
}
