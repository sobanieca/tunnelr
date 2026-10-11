//! `tunnelr rm <name>`

use crate::args::Args;
use crate::connections;
use crate::logger;

pub fn matches(args: &Args) -> bool {
    args.command() == Some("rm")
}

pub fn execute(args: &Args) -> Result<(), String> {
    let name = args.arg(1).ok_or("Use: tunnelr rm <name>")?;
    let removed = connections::remove(name, &connections::connections_path())?;
    logger::plain(format!(
        "Removed connection \"{}\" ({})",
        removed.name, removed.target
    ));
    Ok(())
}
