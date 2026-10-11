//! systemd service on the VPS: the unit file runs `tunnelr server` with the
//! key file (`-a <path>`, the key itself is not in the unit).

use crate::logger;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

pub const SERVICE_NAME: &str = "tunnelr";
pub const UNIT_PATH: &str = "/etc/systemd/system/tunnelr.service";
/// Set in the unit file so the server knows it runs under systemd.
pub const SERVICE_ENV: &str = "TUNNELR_SERVICE";
const START_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ServiceOptions<'a> {
    pub port: u16,
    pub key_path: &'a Path,
    pub bind: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ServiceState {
    Running,
    Installed,
    Updated,
}

struct Output {
    code: i32,
    out: String,
    err: String,
}

fn run_systemctl(args: &[&str]) -> Output {
    match Command::new("systemctl").args(args).output() {
        Ok(output) => Output {
            code: output.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            err: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        },
        Err(err) => Output {
            code: -1,
            out: String::new(),
            err: err.to_string(),
        },
    }
}

fn systemctl(args: &[&str]) -> Result<String, String> {
    let result = run_systemctl(args);
    if result.code != 0 {
        return Err(format!(
            "systemctl {} failed: {}",
            args.join(" "),
            result.err
        ));
    }
    Ok(result.out)
}

pub fn is_inside_service() -> bool {
    std::env::var(SERVICE_ENV).is_ok_and(|value| value == "1")
}

pub fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

pub fn has_systemd() -> bool {
    Path::new("/run/systemd/system").is_dir()
}

fn unit_file(options: &ServiceOptions, exe: &Path) -> String {
    let bind = options
        .bind
        .map(|bind| format!(" --bind {bind}"))
        .unwrap_or_default();
    format!(
        "[Unit]
Description=tunnelr server
After=network-online.target
Wants=network-online.target

[Service]
ExecStart={} server -p {} -a {}{bind}
Restart=always
RestartSec=3
Environment=HOME=/root
Environment={SERVICE_ENV}=1

[Install]
WantedBy=multi-user.target
",
        exe.display(),
        options.port,
        options.key_path.display()
    )
}

fn is_active() -> bool {
    run_systemctl(&["is-active", SERVICE_NAME]).out == "active"
}

fn wait_until_active() -> bool {
    let start = Instant::now();
    while start.elapsed() < START_TIMEOUT {
        if is_active() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// Makes sure the systemd service exists, matches the given options and runs.
pub fn ensure_service(options: &ServiceOptions) -> Result<ServiceState, String> {
    let exe = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|err| format!("Cannot find the tunnelr binary: {err}"))?;
    let wanted = unit_file(options, &exe);
    let current = std::fs::read_to_string(UNIT_PATH).ok();

    let state = if current.as_deref() == Some(wanted.as_str()) {
        if is_active() {
            return Ok(ServiceState::Running);
        }
        systemctl(&["enable", "--now", SERVICE_NAME])?;
        ServiceState::Running
    } else {
        crate::auth::write_private_file(Path::new(UNIT_PATH), &wanted)
            .map_err(|err| format!("Cannot write {UNIT_PATH}: {err}"))?;
        logger::info(format!(
            "{} {UNIT_PATH}",
            if current.is_some() {
                "Updated"
            } else {
                "Wrote"
            }
        ));
        systemctl(&["daemon-reload"])?;
        systemctl(&["enable", SERVICE_NAME])?;
        systemctl(&["restart", SERVICE_NAME])?;
        if current.is_some() {
            ServiceState::Updated
        } else {
            ServiceState::Installed
        }
    };

    if !wait_until_active() {
        return Err(format!(
            "Service {SERVICE_NAME} did not start. Check: journalctl -u {SERVICE_NAME} -n 30"
        ));
    }
    Ok(state)
}

pub fn uninstall_service() -> Result<(), String> {
    let result = run_systemctl(&["disable", "--now", SERVICE_NAME]);
    if result.code != 0 {
        logger::warn(result.err);
    }
    match std::fs::remove_file(UNIT_PATH) {
        Ok(()) => logger::info(format!("Removed {UNIT_PATH}")),
        Err(_) => logger::warn(format!("{UNIT_PATH} was not found")),
    }
    systemctl(&["daemon-reload"])?;
    logger::info(format!("Service {SERVICE_NAME} is removed"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_runs_the_server_command() {
        let options = ServiceOptions {
            port: 20185,
            key_path: Path::new("/root/.secret/tunnelr-key"),
            bind: Some("::"),
        };
        let unit = unit_file(&options, Path::new("/usr/local/bin/tunnelr"));
        assert!(unit.contains(
            "ExecStart=/usr/local/bin/tunnelr server -p 20185 -a /root/.secret/tunnelr-key --bind ::\n"
        ));
        assert!(unit.contains("Environment=TUNNELR_SERVICE=1\n"));
    }
}
