//! `tunnelr ls` lists saved connections, "*" marks the last used one.

use crate::args::Args;
use crate::connections;
use crate::logger;

pub fn matches(args: &Args) -> bool {
    args.command() == Some("ls")
}

pub fn execute() {
    let path = connections::connections_path();
    let store = connections::load(&path);
    if store.connections.is_empty() {
        logger::plain(
            "No saved connections. Add one with:\n  tunnelr add <name> <vps host[:port]> -p <ports> [-a <key or file>]",
        );
        return;
    }
    let width = store
        .connections
        .iter()
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(0);
    let lines: Vec<String> = store
        .connections
        .iter()
        .map(|c| {
            let mark = if c.name == store.last { "*" } else { " " };
            let extra: Vec<String> = [
                c.auth.as_ref().map(|auth| format!("-a {auth}")),
                c.to.as_ref().map(|to| format!("--to {to}")),
            ]
            .into_iter()
            .flatten()
            .collect();
            let ports = if c.ports.is_empty() {
                "(none)".to_string()
            } else {
                c.ports.join(",")
            };
            let extra = if extra.is_empty() {
                String::new()
            } else {
                format!("  {}", extra.join(" "))
            };
            format!("{mark} {:width$}  {} -p {ports}{extra}", c.name, c.target)
        })
        .collect();
    logger::plain(format!(
        "{}\n\n* last used, \"tunnelr\" opens it. Saved in {}",
        lines.join("\n"),
        path.display()
    ));
}
