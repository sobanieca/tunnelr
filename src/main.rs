mod args;
mod auth;
mod client;
mod colors;
mod commands;
mod connections;
mod http;
mod json;
mod logger;
mod mux;
mod net;
mod protocol;
mod server;
mod service;
mod signals;
mod time;
mod update;
mod websocket;

#[cfg(test)]
mod tests;

use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const HELP: &str = include_str!("help.txt");

enum Command {
    Help,
    Version,
    Update,
    Token,
    Add,
    Rm,
    Ls,
    Server,
    Client,
}

fn pick(args: &args::Args) -> Command {
    let command = args.command();
    if args.help || command == Some("help") {
        Command::Help
    } else if args.version || command == Some("version") {
        Command::Version
    } else if command == Some("update") {
        Command::Update
    } else if command == Some("token") {
        Command::Token
    } else if commands::add::matches(args) {
        Command::Add
    } else if commands::rm::matches(args) {
        Command::Rm
    } else if commands::ls::matches(args) {
        Command::Ls
    } else if commands::server::matches(args) {
        Command::Server
    } else if commands::client::matches(args) {
        Command::Client
    } else {
        Command::Help
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    logger::init(raw.iter().any(|arg| arg == "--debug"));
    let args = match args::parse(&raw) {
        Ok(args) => args,
        Err(message) => {
            logger::error(format!("{message}. See: tunnelr --help"));
            std::process::exit(1);
        }
    };
    logger::debug(format!("Args provided: {args:?}"));

    let command = pick(&args);
    let long_running = matches!(command, Command::Client | Command::Server);
    if long_running {
        signals::block();
    }
    let update_check = match command {
        Command::Version | Command::Update | Command::Token => None,
        _ if service::is_inside_service() => None,
        _ => update::start_background_check(),
    };
    let update_check = if long_running {
        std::thread::spawn(move || update::notify(update_check, Duration::from_secs(10)));
        None
    } else {
        update_check
    };

    let result = match command {
        Command::Help => {
            logger::print_line(HELP.trim_end());
            Ok(())
        }
        Command::Version => {
            logger::print_line(VERSION);
            Ok(())
        }
        Command::Update => update::execute(),
        Command::Token => auth::find_key(args.auth.as_deref()).map(|key| {
            logger::print_line(&auth::create_token(&key));
        }),
        Command::Add => commands::add::execute(&args),
        Command::Rm => commands::rm::execute(&args),
        Command::Ls => {
            commands::ls::execute();
            Ok(())
        }
        Command::Server => commands::server::execute(&args),
        Command::Client => commands::client::execute(&args),
    };

    let code = match result {
        Ok(()) => 0,
        Err(message) => {
            logger::error(message);
            1
        }
    };
    update::notify(update_check, Duration::from_millis(500));
    std::process::exit(code);
}
