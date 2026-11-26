//! `tunnelr server [-p <tunnel port>]`. As root on Linux with systemd it installs,
//! updates or just checks the service, prints the banner and exits. Inside
//! the service or with `--foreground` it runs the server in the foreground.
//! `--uninstall` removes the service.

use crate::VERSION;
use crate::args::Args;
use crate::auth::find_or_create_key;
use crate::logger;
use crate::net;
use crate::protocol::parse_port;
use crate::server::{ServerOptions, start_server};
use crate::service::{self, SERVICE_NAME, ServiceOptions, ServiceState};
use crate::signals;
use std::path::Path;

const DEFAULT_TUNNEL_PORT: u16 = 4000;

pub fn matches(args: &Args) -> bool {
    args.command() == Some("server")
}

enum Mode {
    Service(ServiceState),
    Foreground,
}

fn print_banner(port: u16, key: &str, key_path: &Path, mode: &Mode) {
    let local = net::local_addresses();
    let public = net::public_address();
    let best = public
        .or_else(|| local.first().cloned())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let others: Vec<&String> = local.iter().filter(|a| **a != best).collect();
    let host = format!("{}:{port}", net::format_host(&best));
    let mode_line = match mode {
        Mode::Service(ServiceState::Running) => {
            format!("systemd service \"{SERVICE_NAME}\" is running")
        }
        Mode::Service(ServiceState::Installed) => {
            format!("systemd service \"{SERVICE_NAME}\" installed and started")
        }
        Mode::Service(ServiceState::Updated) => {
            format!("systemd service \"{SERVICE_NAME}\" updated and restarted")
        }
        Mode::Foreground => "running in foreground (no systemd service)".to_string(),
    };
    let other_ips = if others.is_empty() {
        String::new()
    } else {
        let list: Vec<&str> = others.iter().map(|a| a.as_str()).collect();
        format!("\n  Other IPs:     {}", list.join(", "))
    };
    let footer = match mode {
        Mode::Foreground => String::new(),
        Mode::Service(_) => format!(
            "\nLogs: journalctl -u {SERVICE_NAME} -f    Remove: sudo tunnelr server --uninstall\n"
        ),
    };
    let key_file = key_path.display();
    logger::plain(format!(
        "
tunnelr server {VERSION} - {mode_line}

  Address:       {host}{other_ips}
  Tunnel port:   {port}  (tunnelr clients connect here)
  Public ports:  any other port open on this VPS, the client picks them with -p
  Key:           {key}
  Key file:      {key_file}

On your machine, save the key in ~/.tunnelr/key and run:

  tunnelr {host} -p <public port>:<app port>

Run this command again anytime to see the address and the key.
{footer}"
    ));
}

fn run_foreground(
    port: u16,
    key: &str,
    key_path: &Path,
    bind: Option<String>,
) -> Result<(), String> {
    let server = std::sync::Arc::new(start_server(ServerOptions {
        port,
        key: key.to_string(),
        bind,
    })?);
    if service::is_inside_service() {
        logger::info(format!("Listening on tunnel port {port}"));
    } else {
        print_banner(port, key, key_path, &Mode::Foreground);
    }
    let stopping = server.clone();
    signals::on_terminate(move || {
        logger::info("Shutting down...");
        stopping.shutdown();
        std::process::exit(0);
    });
    loop {
        std::thread::park();
    }
}

/// Why the service cannot be used, or `None` when it can.
fn service_blocker() -> Option<&'static str> {
    if std::env::consts::OS != "linux" {
        Some("not on Linux")
    } else if !service::has_systemd() {
        Some("systemd not found")
    } else if !service::is_root() {
        Some("not running as root (try sudo)")
    } else {
        None
    }
}

pub fn execute(args: &Args) -> Result<(), String> {
    if args.positional.len() > 1 {
        return Err("Use: tunnelr server [-p <tunnel port>] [--uninstall]".to_string());
    }
    if args.uninstall {
        return service::uninstall_service();
    }
    let given: Vec<&String> = args.port.iter().chain(&args.tunnel_port).collect();
    if given.len() > 1 {
        return Err(
            "Server needs one tunnel port, example: server -p 4000. Public ports are picked by the client"
                .to_string(),
        );
    }
    let port = match given.first() {
        Some(port) => parse_port(port).map_err(|_| format!("Invalid tunnel port: {port}"))?,
        None => DEFAULT_TUNNEL_PORT,
    };
    let (key, key_path) = find_or_create_key(args.auth.as_deref())?;

    if service::is_inside_service() || args.foreground {
        return run_foreground(port, &key, &key_path, args.bind.clone());
    }
    if let Some(blocker) = service_blocker() {
        logger::warn(format!(
            "Cannot install the systemd service ({blocker}). Running in foreground, tunnelr will not survive a reboot."
        ));
        return run_foreground(port, &key, &key_path, args.bind.clone());
    }
    signals::on_terminate(|| std::process::exit(130));
    let state = service::ensure_service(&ServiceOptions {
        port,
        key_path: &key_path,
        bind: args.bind.as_deref(),
    })?;
    print_banner(port, &key, &key_path, &Mode::Service(state));
    Ok(())
}
