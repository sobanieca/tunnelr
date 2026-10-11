//! Command line arguments: positionals plus the few options tunnelr knows.
//! `--name value`, `--name=value`, `-p value`, `-p=value` and `-p3000` are
//! accepted, `-p` can be repeated, `--` ends the options.

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub positional: Vec<String>,
    pub help: bool,
    pub version: bool,
    pub debug: bool,
    pub foreground: bool,
    pub uninstall: bool,
    pub auth: Option<String>,
    pub to: Option<String>,
    pub bind: Option<String>,
    pub port: Vec<String>,
}

impl Args {
    /// Positional argument at `index`.
    pub fn arg(&self, index: usize) -> Option<&str> {
        self.positional.get(index).map(String::as_str)
    }

    pub fn command(&self) -> Option<&str> {
        self.arg(0)
    }
}

enum Kind {
    Flag(fn(&mut Args)),
    Value(fn(&mut Args, String)),
}

fn option(name: &str) -> Option<(&'static str, Kind)> {
    let found = match name {
        "help" | "h" => ("help", Kind::Flag(|a| a.help = true)),
        "version" | "v" => ("version", Kind::Flag(|a| a.version = true)),
        "debug" => ("debug", Kind::Flag(|a| a.debug = true)),
        "foreground" => ("foreground", Kind::Flag(|a| a.foreground = true)),
        "uninstall" => ("uninstall", Kind::Flag(|a| a.uninstall = true)),
        "auth" | "a" => ("auth", Kind::Value(|a, v| a.auth = Some(v))),
        "to" => ("to", Kind::Value(|a, v| a.to = Some(v))),
        "bind" => ("bind", Kind::Value(|a, v| a.bind = Some(v))),
        "port" | "p" => ("port", Kind::Value(|a, v| a.port.push(v))),
        _ => return None,
    };
    Some(found)
}

pub fn parse(raw: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut iter = raw.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            args.positional.extend(iter.by_ref().cloned());
            break;
        }
        let (name, inline) = if let Some(long) = arg.strip_prefix("--") {
            match long.split_once('=') {
                Some((name, value)) => (name.to_string(), Some(value.to_string())),
                None => (long.to_string(), None),
            }
        } else if arg.len() > 1 && arg.starts_with('-') {
            let short = &arg[1..];
            let split = short.char_indices().nth(1).map_or(short.len(), |(i, _)| i);
            let (name, rest) = short.split_at(split);
            let rest = rest.strip_prefix('=').unwrap_or(rest);
            (
                name.to_string(),
                (!rest.is_empty()).then(|| rest.to_string()),
            )
        } else {
            args.positional.push(arg.clone());
            continue;
        };
        let (long, kind) = option(&name).ok_or_else(|| format!("Unknown option: {arg}"))?;
        match kind {
            Kind::Flag(set) => {
                if inline.is_some() {
                    return Err(format!("Option --{long} does not take a value"));
                }
                set(&mut args);
            }
            Kind::Value(set) => {
                let value = match inline {
                    Some(value) => value,
                    None => match iter.next_if(|next| !next.starts_with('-') || *next == "-") {
                        Some(value) => value.clone(),
                        None => return Err(format!("Option --{long} needs a value")),
                    },
                };
                set(&mut args, value);
            }
        }
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(line: &str) -> Result<Args, String> {
        let raw: Vec<String> = line.split_whitespace().map(String::from).collect();
        parse(&raw)
    }

    #[test]
    fn parses_options_and_positionals() {
        let args =
            parse_str("vps:2500 -p 3000 --port=4000 -p5000 -a ~/k --to=10.0.0.2 --debug").unwrap();
        assert_eq!(args.positional, vec!["vps:2500"]);
        assert_eq!(args.port, vec!["3000", "4000", "5000"]);
        assert_eq!(args.auth.as_deref(), Some("~/k"));
        assert_eq!(args.to.as_deref(), Some("10.0.0.2"));
        assert!(args.debug);

        let args = parse_str("server -h -v --foreground --uninstall --bind ::1").unwrap();
        assert!(args.help && args.version && args.foreground && args.uninstall);
        assert_eq!(args.bind.as_deref(), Some("::1"));
        assert_eq!(args.command(), Some("server"));

        let args = parse_str("add -- -weird").unwrap();
        assert_eq!(args.positional, vec!["add", "-weird"]);
    }

    #[test]
    fn rejects_bad_options() {
        assert_eq!(parse_str("--nope").unwrap_err(), "Unknown option: --nope");
        assert_eq!(parse_str("-x").unwrap_err(), "Unknown option: -x");
        assert_eq!(parse_str("-p").unwrap_err(), "Option --port needs a value");
        assert_eq!(
            parse_str("-a --debug").unwrap_err(),
            "Option --auth needs a value"
        );
        assert_eq!(
            parse_str("--debug=1").unwrap_err(),
            "Option --debug does not take a value"
        );
    }
}
