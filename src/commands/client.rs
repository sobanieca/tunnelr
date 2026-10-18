//! `tunnelr <host[:port]> -p <ports>`, `tunnelr <name> [-p <ports>]` opens a
//! saved connection and `tunnelr [-p <ports>]` opens the last used one.

use crate::args::Args;
use crate::auth::find_key;
use crate::client::{ClientOptions, start_client};
use crate::connections;
use crate::logger;
use crate::protocol::parse_port_mappings;
use crate::signals;

/// No arguments: open the last saved connection. "tunnelr" without any flags
/// and without a saved connection shows the help instead.
pub fn matches(args: &Args) -> bool {
    match args.positional.len() {
        1 => true,
        0 => {
            !args.port.is_empty()
                || connections::find_last(&connections::connections_path()).is_some()
        }
        _ => false,
    }
}

pub fn execute(args: &Args) -> Result<(), String> {
    let path = connections::connections_path();
    let name = args.arg(0);
    let saved = match name {
        None => connections::find_last(&path),
        Some(name) => connections::find(name, &path),
    };
    if name.is_none() && saved.is_none() {
        return Err(
            "No saved connection to open. Use: tunnelr <vps host[:port]> -p <ports>, tunnelr add <name> <vps host[:port]> -p <ports>, or tunnelr server -p <port>"
                .to_string(),
        );
    }

    let mut auth = args.auth.clone();
    let mut to = args.to.clone();
    let (target, ports) = match &saved {
        Some(saved) => {
            logger::info(format!(
                "Using saved connection \"{}\" ({})",
                saved.name, saved.target
            ));
            auth = auth.or_else(|| saved.auth.clone());
            to = to.or_else(|| saved.to.clone());
            connections::mark_last(&saved.name, &path)?;
            (
                saved.target.clone(),
                connections::resolve_ports(&saved.ports, &args.port)?,
            )
        }
        None => (
            name.unwrap_or_default().to_string(),
            parse_port_mappings(&args.port)?,
        ),
    };
    if ports.is_empty() {
        return Err("Give at least one port to open, example: -p 3000".to_string());
    }
    let key = find_key(auth.as_deref())?;
    let client = start_client(ClientOptions {
        target,
        key,
        ports,
        to,
        on_ready: None,
    })?;
    let stopper = client.stopper();
    signals::on_terminate(move || {
        stopper.stop();
        std::process::exit(0);
    });
    client.wait();
    std::process::exit(1);
}
