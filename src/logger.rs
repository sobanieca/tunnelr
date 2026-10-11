//! Console logger: every line starts with a gray `[HH:MM:SS]`, info is bright
//! blue, warnings bright yellow, errors bright red. Debug output only with
//! `--debug`. Unit tests are quiet unless `TUNNELR_TEST_VERBOSE=1` is set.

use crate::colors;
use crate::time;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG: AtomicBool = AtomicBool::new(false);

pub fn init(debug: bool) {
    DEBUG.store(debug, Ordering::Relaxed);
}

fn quiet() -> bool {
    cfg!(test) && std::env::var_os("TUNNELR_TEST_VERBOSE").is_none_or(|v| v.is_empty())
}

/// Writes a line to stdout, ignoring errors such as a closed pipe.
pub fn print_line(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
    let _ = stdout.flush();
}

fn write(color: fn(&str) -> String, message: &str) {
    if quiet() {
        return;
    }
    let stamp = colors::gray(&format!("[{}]", time::local_clock()));
    print_line(&format!("{stamp} {}", color(message)));
}

pub fn debug_enabled() -> bool {
    DEBUG.load(Ordering::Relaxed) || (cfg!(test) && !quiet())
}

pub fn debug(message: impl AsRef<str>) {
    if debug_enabled() {
        write(colors::gray, message.as_ref());
    }
}

pub fn info(message: impl AsRef<str>) {
    write(colors::bright_blue, message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    write(colors::bright_yellow, message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write(colors::bright_red, message.as_ref());
}

/// Prints text as is, without timestamp and color.
pub fn plain(text: impl AsRef<str>) {
    if !quiet() {
        print_line(text.as_ref());
    }
}
