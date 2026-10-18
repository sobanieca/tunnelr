//! The tunnelr client: connects to `ws://host:port/tunnel`, sends `hello`,
//! opens a local TCP connection for every `connect` message and reconnects
//! with exponential backoff. Stops for good on auth or port errors (close
//! codes 4001/4002) or when the server closed all its ports.

use crate::VERSION;
use crate::auth::create_token;
use crate::json::Value;
use crate::logger;
use crate::mux::Mux;
use crate::net;
use crate::protocol::{Mapping, message, message_type, number_field};
use crate::websocket;
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const DEFAULT_CONTROL_PORT: u16 = 4000;
const WATCHDOG: Duration = Duration::from_secs(45);
const MIN_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);

#[derive(Debug, PartialEq)]
pub struct Target {
    /// Host name or IP, IPv6 in brackets.
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub url: String,
    pub label: String,
}

/// Turns "host", "host:2500", "http://host:2500" or "wss://host" into the
/// WebSocket address of the server.
pub fn parse_target(target: &str) -> Result<Target, String> {
    let invalid = || format!("Invalid address: {target}");
    let mut text = target.trim();
    let mut secure = false;
    if let Some((scheme, rest)) = text.split_once("://")
        && !scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        secure = ["https", "wss"].contains(&scheme.to_ascii_lowercase().as_str());
        text = rest;
    }
    let text = text.split('/').next().unwrap_or_default();
    let text = text.rsplit_once('@').map_or(text, |(_, host)| host);
    let (host, port) = if text.starts_with('[') {
        let end = text.find(']').ok_or_else(invalid)?;
        let host = &text[..=end];
        match &text[end + 1..] {
            "" => (host, None),
            rest => (host, Some(rest.strip_prefix(':').ok_or_else(invalid)?)),
        }
    } else {
        match text.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (text, None),
        }
    };
    if host.is_empty() || host.contains(':') && !host.starts_with('[') {
        return Err(invalid());
    }
    let port = match port {
        None | Some("") => DEFAULT_CONTROL_PORT,
        Some(port) => port.parse::<u16>().map_err(|_| invalid())?,
    };
    let host = host.to_ascii_lowercase();
    Ok(Target {
        url: format!(
            "{}://{host}:{port}/tunnel",
            if secure { "wss" } else { "ws" }
        ),
        label: format!("{host}:{port}"),
        host,
        port,
        secure,
    })
}

pub struct ClientOptions {
    pub target: String,
    pub key: String,
    pub ports: Vec<Mapping>,
    pub to: Option<String>,
    pub on_ready: Option<Box<dyn Fn(Vec<u16>) + Send + Sync>>,
}

struct Shared {
    stopped: Mutex<bool>,
    wake: Condvar,
    current: Mutex<Option<Arc<Mux>>>,
}

impl Shared {
    fn is_stopped(&self) -> bool {
        *self.stopped.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_stopped(&self) {
        *self.stopped.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.notify_all();
    }

    /// Sleeps for `duration` unless the client is stopped meanwhile.
    fn sleep(&self, duration: Duration) {
        let stopped = self.stopped.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self
            .wake
            .wait_timeout_while(stopped, duration, |stopped| !*stopped);
    }
}

pub struct ClientHandle {
    shared: Arc<Shared>,
    thread: JoinHandle<String>,
}

/// Stops a client from another thread.
pub struct Stopper(Arc<Shared>);

impl Stopper {
    pub fn stop(&self) {
        self.0.set_stopped();
        if let Some(mux) = self
            .0
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            mux.close(1000, "client stopped");
        }
    }
}

impl ClientHandle {
    pub fn stopper(&self) -> Stopper {
        Stopper(self.shared.clone())
    }

    #[cfg(test)]
    pub fn stop(&self) {
        self.stopper().stop();
    }

    /// Waits until the client stopped for good and returns why: "stopped"
    /// or the reason the server rejected the tunnel.
    pub fn wait(self) -> String {
        self.thread.join().unwrap_or_else(|_| "crashed".to_string())
    }
}

struct SessionResult {
    code: u16,
    reason: String,
}

pub fn start_client(options: ClientOptions) -> Result<ClientHandle, String> {
    let target = parse_target(&options.target)?;
    let shared = Arc::new(Shared {
        stopped: Mutex::new(false),
        wake: Condvar::new(),
        current: Mutex::new(None),
    });
    let thread = {
        let shared = shared.clone();
        std::thread::spawn(move || run(&target, &options, &shared))
    };
    Ok(ClientHandle { shared, thread })
}

fn run(target: &Target, options: &ClientOptions, shared: &Arc<Shared>) -> String {
    logger::info(format!("Connecting to {}...", target.label));
    let mut retry = MIN_RETRY;
    while !shared.is_stopped() {
        let result = session(target, options, shared, &mut retry);
        if shared.is_stopped() {
            break;
        }
        if result.code == 4001 || result.code == 4002 {
            // the server rejected us for a reason that will not change by retrying
            logger::error(format!("Server rejected the tunnel: {}", result.reason));
            shared.set_stopped();
            return result.reason;
        }
        logger::warn(format!(
            "Disconnected ({}). Reconnecting in {}s...",
            result.reason,
            retry.as_secs()
        ));
        shared.sleep(retry);
        retry = (retry * 2).min(MAX_RETRY);
    }
    "stopped".to_string()
}

fn session(
    target: &Target,
    options: &ClientOptions,
    shared: &Arc<Shared>,
    retry: &mut Duration,
) -> SessionResult {
    let socket = match websocket::connect(
        target.host.trim_matches(['[', ']']),
        target.port,
        "/tunnel",
        target.secure,
    ) {
        Ok(socket) => socket,
        Err(err) => {
            return SessionResult {
                code: crate::mux::CODE_ABNORMAL,
                reason: format!("connection failed: {err}"),
            };
        }
    };
    let mux = Mux::new(socket);
    *shared.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(mux.clone());
    if shared.is_stopped() {
        mux.close(1000, "client stopped");
    }
    mux.set_read_timeout(Some(WATCHDOG));

    let to = options
        .to
        .clone()
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let mut mappings: BTreeMap<u16, u16> =
        options.ports.iter().map(|m| (m.remote, m.local)).collect();
    let remote_ports: Vec<Value> = mappings.keys().map(|p| (*p).into()).collect();
    mux.send_control(&message(
        "hello",
        vec![
            ("token", create_token(&options.key).into()),
            ("ports", Value::Array(remote_ports)),
            ("version", VERSION.into()),
        ],
    ));

    let mut close_reason = String::new();
    let handler_mux = mux.clone();
    let info = mux.run(|msg| match message_type(&msg) {
        "ready" => {
            *retry = MIN_RETRY;
            let describe: Vec<String> = mappings
                .iter()
                .map(|(remote, local)| format!("{}:{remote} -> {to}:{local}", target.host))
                .collect();
            logger::info(format!("Tunnel is open: {}", describe.join(", ")));
            if let Some(on_ready) = &options.on_ready {
                let ports = msg
                    .get("ports")
                    .and_then(Value::as_array)
                    .map(|ports| {
                        ports
                            .iter()
                            .filter_map(|p| p.as_f64())
                            .map(|p| p as u16)
                            .collect()
                    })
                    .unwrap_or_default();
                on_ready(ports);
            }
        }
        "error" => {
            close_reason = msg
                .get("message")
                .map(Value::to_plain_string)
                .unwrap_or_default();
        }
        "connect" => {
            let (Some(id), Some(port)) = (number_field(&msg, "id"), number_field(&msg, "port"))
            else {
                return;
            };
            let Some(&local) = u16::try_from(port)
                .ok()
                .and_then(|port| mappings.get(&port))
            else {
                handler_mux.send_control(&message("close", vec![("id", id.into())]));
                return;
            };
            logger::debug(format!("Visitor {id} on port {port}"));
            let to = to.clone();
            handler_mux.attach(
                id,
                Box::new(move || {
                    net::connect(&to, local).inspect_err(|err| {
                        logger::warn(format!("Cannot connect to {to}:{local}: {err}"));
                    })
                }),
                None,
            );
        }
        "close" => {
            if let Some(id) = number_field(&msg, "id") {
                handler_mux.close_stream(id, false);
            }
        }
        "closed" => {
            let port = number_field(&msg, "port").unwrap_or_default();
            if let Ok(port) = u16::try_from(port) {
                mappings.remove(&port);
            }
            let reason = msg
                .get("reason")
                .map(Value::to_plain_string)
                .unwrap_or_default();
            logger::warn(format!("Port {port} was closed by server: {reason}"));
            if mappings.is_empty() {
                close_reason = "all ports were closed by server".to_string();
                shared.set_stopped();
                handler_mux.close(1000, "");
            }
        }
        "ping" => handler_mux.send_control(&message("pong", vec![])),
        other => logger::debug(format!("Unknown control message: {other}")),
    });

    mux.close_all_streams();
    *shared.current.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let reason = if !close_reason.is_empty() {
        close_reason
    } else if info.timed_out {
        "no response from server".to_string()
    } else if !info.reason.is_empty() {
        info.reason
    } else {
        format!("code {}", info.code)
    };
    SessionResult {
        code: info.code,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_targets() {
        assert_eq!(
            parse_target("my-vps.com").unwrap().url,
            "ws://my-vps.com:4000/tunnel"
        );
        assert_eq!(parse_target("my-vps.com:3000").unwrap().port, 3000);
        assert_eq!(
            parse_target("wss://my-vps.com").unwrap().url,
            "wss://my-vps.com:4000/tunnel"
        );
        assert_eq!(
            parse_target("http://My-VPS.com:2500/").unwrap().label,
            "my-vps.com:2500"
        );
        let v6 = parse_target("[::1]:2500").unwrap();
        assert_eq!((v6.host.as_str(), v6.port), ("[::1]", 2500));
        assert_eq!(parse_target("[2001:db8::1]").unwrap().port, 4000);
        assert!(parse_target("").is_err());
        assert!(parse_target("host:99999").is_err());
        assert!(parse_target("::1").is_err());
    }
}
