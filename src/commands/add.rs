//! `tunnelr add <name> <host[:port]> -p <ports> [-a <key|file>] [--to <host>]`

use crate::args::Args;
use crate::connections::{self, Connection};
use crate::logger;

pub fn matches(args: &Args) -> bool {
    args.command() == Some("add")
}

pub fn execute(args: &Args) -> Result<(), String> {
    let (Some(name), Some(target)) = (args.arg(1), args.arg(2)) else {
        return Err(
            "Use: tunnelr add <name> <vps host[:port]> -p <ports> [-a <key or file>]".to_string(),
        );
    };
    let path = connections::connections_path();
    let saved = connections::add(
        Connection {
            name: name.to_string(),
            target: target.to_string(),
            ports: args.port.clone(),
            auth: args.auth.clone(),
            to: args.to.clone(),
        },
        &path,
    )?;
    let ports = if saved.ports.is_empty() {
        "(no ports, give -p when running)".to_string()
    } else {
        saved.ports.join(",")
    };
    logger::plain(format!(
        "Saved connection \"{}\": {} -p {ports}\nSaved in {}\n\nOpen it with:  tunnelr   or   tunnelr {} [-p <ports>]",
        saved.name,
        saved.target,
        path.display(),
        saved.name
    ));
    Ok(())
}
