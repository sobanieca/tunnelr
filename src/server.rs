//! The tunnelr server: one control port for the HTTP API and the WebSocket
//! of the clients, plus one listener for every port a client opened. When a
//! client asks for a port that is already open, the old tunnel is closed and
//! the new client takes over.

use crate::VERSION;
use crate::auth::TokenVerifier;
use crate::http::{self, Request};
use crate::json::Value;
use crate::logger;
use crate::mux::{Mux, Stats};
use crate::net;
use crate::protocol::{self, message, message_type, number_field};
use crate::time;
use crate::websocket::Socket;
use std::collections::{BTreeSet, HashMap};
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const PING_INTERVAL: Duration = Duration::from_secs(15);
const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CLIENT_CLOSE_DELAY: Duration = Duration::from_secs(1);
const PORT_RELEASE_WAIT: Duration = Duration::from_secs(1);

pub struct ServerOptions {
    pub port: u16,
    pub key: String,
    pub bind: Option<String>,
}

/// An open port as listed by the HTTP API.
#[derive(Debug, Clone, PartialEq)]
pub struct TunnelInfo {
    pub port: u16,
    pub client: String,
    pub since: String,
    pub active_connections: u64,
    pub total_connections: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

impl TunnelInfo {
    fn to_json(&self) -> Value {
        Value::object(vec![
            ("port", self.port.into()),
            ("client", self.client.as_str().into()),
            ("since", self.since.as_str().into()),
            ("activeConnections", self.active_connections.into()),
            ("totalConnections", self.total_connections.into()),
            ("bytesIn", self.bytes_in.into()),
            ("bytesOut", self.bytes_out.into()),
        ])
    }
}

struct Client {
    mux: Arc<Mux>,
    ports: Mutex<BTreeSet<u16>>,
    hello_seen: AtomicBool,
    ready: AtomicBool,
    dropped: AtomicBool,
    remote_addr: String,
}

struct Tunnel {
    port: u16,
    client: Arc<Client>,
    since_ms: u64,
    stats: Arc<Stats>,
    stop: Arc<AtomicBool>,
    accepting: Option<JoinHandle<()>>,
}

impl Tunnel {
    /// Stops the listener. The port is free again when this returns.
    fn stop_listener(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accepting.take() {
            let _ = handle.join();
        }
    }
}

struct State {
    port: u16,
    bind: Option<String>,
    verifier: TokenVerifier,
    tunnels: Mutex<HashMap<u16, Tunnel>>,
    clients: Mutex<Vec<Arc<Client>>>,
    stream_seq: AtomicU32,
}

pub struct Server {
    state: Arc<State>,
    stop: Arc<AtomicBool>,
    accepting: Mutex<Option<JoinHandle<()>>>,
}

pub fn start_server(options: ServerOptions) -> Result<Server, String> {
    let listener = net::listen_tcp(options.port, options.bind.as_deref())
        .map_err(|err| format!("Cannot listen on port {}: {err}", options.port))?;
    let state = Arc::new(State {
        port: options.port,
        bind: options.bind,
        verifier: TokenVerifier::new(&options.key),
        tunnels: Mutex::new(HashMap::new()),
        clients: Mutex::new(Vec::new()),
        stream_seq: AtomicU32::new(0),
    });
    let stop = Arc::new(AtomicBool::new(false));
    let accepting = {
        let state = state.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            net::accept_until(&listener, &stop, |stream, addr| {
                let state = state.clone();
                std::thread::spawn(move || handle_connection(&state, stream, addr));
            })
        })
    };
    Ok(Server {
        state,
        stop,
        accepting: Mutex::new(Some(accepting)),
    })
}

impl Server {
    #[cfg(test)]
    pub fn list_tunnels(&self) -> Vec<TunnelInfo> {
        self.state.list_tunnels()
    }

    #[cfg(test)]
    pub fn close_tunnel(&self, port: u16, reason: &str) -> bool {
        self.state.close_tunnel(port, reason)
    }

    /// Disconnects all clients, closes all ports and the control port.
    pub fn shutdown(&self) {
        let clients: Vec<Arc<Client>> = self
            .state
            .clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for client in clients {
            self.state.drop_client(&client);
            client.mux.close(1001, "server shutting down");
        }
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self
            .accepting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = handle.join();
        }
    }
}

fn handle_connection(state: &Arc<State>, stream: TcpStream, addr: SocketAddr) {
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let (request, reader) = match http::read_request(&stream) {
        Ok(read) => read,
        Err(err) => {
            logger::debug(format!("Bad request from {addr}: {err}"));
            return;
        }
    };
    let upgrade = request
        .header("upgrade")
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if request.path == "/tunnel" && upgrade {
        if http::accept_websocket(&stream, &request).is_err() {
            return;
        }
        let writer = match stream.try_clone() {
            Ok(writer) => writer,
            Err(_) => return,
        };
        let socket = Socket {
            reader,
            writer: Box::new(writer),
            tcp: stream,
            mask: false,
        };
        state.handle_tunnel(socket, net::clean_address(addr.ip()));
        return;
    }
    let (status, body) = state.handle_api(&request);
    let _ = http::respond_json(&stream, status, &body);
}

fn error_body(message: &str) -> Value {
    Value::object(vec![("error", message.into())])
}

/// Token from "Authorization: Bearer <token>", "Authorization: <token>" or
/// "?token=<token>".
fn read_token(request: &Request) -> String {
    if let Some(header) = request.header("authorization") {
        let header = header.trim();
        let token = match header.get(..7) {
            Some(prefix) if prefix.eq_ignore_ascii_case("bearer ") => &header[7..],
            _ => header,
        };
        return token.trim().to_string();
    }
    request.query_param("token").unwrap_or_default().to_string()
}

impl State {
    fn list_tunnels(&self) -> Vec<TunnelInfo> {
        let tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<&Tunnel> = tunnels.values().collect();
        list.sort_by_key(|t| (t.since_ms, t.port));
        list.into_iter()
            .map(|t| TunnelInfo {
                port: t.port,
                client: t.client.remote_addr.clone(),
                since: time::iso_string(t.since_ms),
                active_connections: t.stats.active.load(Ordering::SeqCst),
                total_connections: t.stats.connections.load(Ordering::SeqCst),
                bytes_in: t.stats.bytes_read.load(Ordering::SeqCst),
                bytes_out: t.stats.bytes_written.load(Ordering::SeqCst),
            })
            .collect()
    }

    fn handle_api(&self, request: &Request) -> (u16, Value) {
        if request.path == "/" {
            return (
                200,
                Value::object(vec![
                    ("name", "tunnelr".into()),
                    ("version", VERSION.into()),
                ]),
            );
        }
        let port_param = match request.path.strip_prefix("/ports") {
            Some("") => None,
            Some(rest) => match rest.strip_prefix('/') {
                Some(digits)
                    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
                {
                    Some(digits)
                }
                _ => return (404, error_body("not found")),
            },
            None => return (404, error_body("not found")),
        };
        if let Err(problem) = self.verifier.verify(&read_token(request)) {
            return (401, error_body(&format!("unauthorized: {problem}")));
        }
        match (request.method.as_str(), port_param) {
            ("GET", None) => {
                let ports = self
                    .list_tunnels()
                    .iter()
                    .map(TunnelInfo::to_json)
                    .collect();
                (200, Value::object(vec![("ports", Value::Array(ports))]))
            }
            ("DELETE", Some(digits)) => {
                let closed = digits
                    .parse::<u16>()
                    .is_ok_and(|port| self.close_tunnel(port, "closed by api"));
                if closed {
                    let port: f64 = digits.parse().unwrap_or_default();
                    (200, Value::object(vec![("closed", Value::Number(port))]))
                } else {
                    (404, error_body("port is not open"))
                }
            }
            ("POST", _) => (
                400,
                error_body("ports are opened by the tunnelr client, see tunnelr --help"),
            ),
            _ => (405, error_body("method not allowed")),
        }
    }

    fn close_tunnel(&self, port: u16, reason: &str) -> bool {
        let removed = self
            .tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&port);
        let Some(tunnel) = removed else {
            return false;
        };
        let client = tunnel.client.clone();
        let no_ports_left = {
            let mut ports = client.ports.lock().unwrap_or_else(|e| e.into_inner());
            ports.remove(&port);
            ports.is_empty()
        };
        client.mux.close_streams_of(&tunnel.stats);
        client.mux.send_control(&message(
            "closed",
            vec![("port", port.into()), ("reason", reason.into())],
        ));
        logger::info(format!("Port {port} closed ({reason})"));
        if no_ports_left && client.ready.load(Ordering::SeqCst) {
            // give the client a moment to read the message and close by itself
            let client = client.clone();
            std::thread::spawn(move || {
                std::thread::sleep(CLIENT_CLOSE_DELAY);
                if client
                    .ports
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_empty()
                {
                    client.mux.close(1000, "no ports left");
                }
            });
        }
        tunnel.stop_listener();
        true
    }

    fn listen(&self, port: u16) -> io::Result<TcpListener> {
        let started = Instant::now();
        loop {
            match net::listen_tcp(port, self.bind.as_deref()) {
                // a tunnel of a client that just disconnected may still be
                // releasing the port
                Err(err)
                    if err.kind() == io::ErrorKind::AddrInUse
                        && started.elapsed() < PORT_RELEASE_WAIT =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                result => return result,
            }
        }
    }

    fn open_ports(self: &Arc<Self>, client: &Arc<Client>, ports: &[u16]) -> Result<(), String> {
        let mut opened = Vec::new();
        for &port in ports {
            let taken = self
                .tunnels
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&port);
            if taken {
                self.close_tunnel(port, "taken over by a new client");
            }
            let listener = match self.listen(port) {
                Ok(listener) => listener,
                Err(err) => {
                    for port in opened {
                        self.close_tunnel(port, "rolled back");
                    }
                    return Err(format!("Cannot open port {port}: {err}"));
                }
            };
            let stats = Arc::new(Stats::default());
            let stop = Arc::new(AtomicBool::new(false));
            let accepting = {
                let state = self.clone();
                let client = client.clone();
                let stats = stats.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    net::accept_until(&listener, &stop, |conn, addr| {
                        let id = state.stream_seq.fetch_add(1, Ordering::SeqCst) + 1;
                        stats.connections.fetch_add(1, Ordering::SeqCst);
                        logger::debug(format!(
                            "Port {port}: new connection {id} from {}",
                            net::clean_address(addr.ip())
                        ));
                        client.mux.send_control(&message(
                            "connect",
                            vec![("id", id.into()), ("port", port.into())],
                        ));
                        client
                            .mux
                            .attach(id, Box::new(move || Ok(conn)), Some(stats.clone()));
                    });
                })
            };
            self.tunnels
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(
                    port,
                    Tunnel {
                        port,
                        client: client.clone(),
                        since_ms: time::now_ms(),
                        stats,
                        stop,
                        accepting: Some(accepting),
                    },
                );
            client
                .ports
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(port);
            opened.push(port);
        }
        Ok(())
    }

    fn reject(&self, client: &Client, code: u16, problem: &str) {
        logger::warn(format!("Client {} rejected: {problem}", client.remote_addr));
        client
            .mux
            .send_control(&message("error", vec![("message", problem.into())]));
        let reason: String = problem.chars().take(120).collect();
        client.mux.close(code, &reason);
    }

    fn handle_hello(self: &Arc<Self>, client: &Arc<Client>, msg: &Value) {
        let token = msg
            .get("token")
            .map(Value::to_plain_string)
            .unwrap_or_default();
        if let Err(problem) = self.verifier.verify(&token) {
            self.reject(client, 4001, &problem);
            return;
        }
        let ports = match self.requested_ports(msg) {
            Ok(ports) => ports,
            Err(problem) => {
                self.reject(client, 4002, &problem);
                return;
            }
        };
        if let Err(problem) = self.open_ports(client, &ports) {
            self.reject(client, 4002, &problem);
            return;
        }
        client.ready.store(true, Ordering::SeqCst);
        client.mux.send_control(&message(
            "ready",
            vec![(
                "ports",
                Value::Array(ports.iter().map(|p| (*p).into()).collect()),
            )],
        ));
        let list: Vec<String> = ports.iter().map(u16::to_string).collect();
        logger::info(format!(
            "Client {} opened port(s) {}",
            client.remote_addr,
            list.join(", ")
        ));
    }

    fn requested_ports(&self, msg: &Value) -> Result<Vec<u16>, String> {
        let requested = msg
            .get("ports")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if requested.is_empty() {
            return Err("no ports requested".to_string());
        }
        let mut ports: Vec<u16> = Vec::new();
        for value in requested {
            let port = protocol::parse_port(&value.to_plain_string())?;
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
        if ports.contains(&self.port) {
            return Err(format!(
                "port {} is used by the tunnelr server itself",
                self.port
            ));
        }
        Ok(ports)
    }

    fn handle_control(self: &Arc<Self>, client: &Arc<Client>, msg: Value) {
        match message_type(&msg) {
            "hello" => {
                if !client.hello_seen.swap(true, Ordering::SeqCst) {
                    self.handle_hello(client, &msg);
                }
            }
            "close" => {
                if let Some(id) = number_field(&msg, "id") {
                    client.mux.close_stream(id, false);
                }
            }
            "pong" => {}
            other => logger::debug(format!("Unknown control message: {other}")),
        }
    }

    fn drop_client(&self, client: &Arc<Client>) {
        if client.dropped.swap(true, Ordering::SeqCst) {
            return;
        }
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|c| !Arc::ptr_eq(c, client));
        let ports: Vec<u16> =
            std::mem::take(&mut *client.ports.lock().unwrap_or_else(|e| e.into_inner()))
                .into_iter()
                .collect();
        for port in ports {
            let removed = {
                let mut tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
                match tunnels.get(&port) {
                    Some(tunnel) if Arc::ptr_eq(&tunnel.client, client) => tunnels.remove(&port),
                    _ => None,
                }
            };
            if let Some(tunnel) = removed {
                tunnel.stop_listener();
                logger::info(format!("Port {port} closed (client disconnected)"));
            }
        }
        client.mux.close_all_streams();
    }

    fn handle_tunnel(self: &Arc<Self>, socket: Socket, remote_addr: String) {
        let mux = Mux::new(socket);
        mux.set_read_timeout(Some(IDLE_TIMEOUT));
        let client = Arc::new(Client {
            mux: mux.clone(),
            ports: Mutex::new(BTreeSet::new()),
            hello_seen: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            dropped: AtomicBool::new(false),
            remote_addr,
        });
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(client.clone());
        logger::debug(format!("Client {} connected", client.remote_addr));

        {
            let client = client.clone();
            std::thread::spawn(move || {
                if client.mux.wait_closed(HELLO_TIMEOUT) {
                    return;
                }
                if !client.ready.load(Ordering::SeqCst) {
                    client
                        .mux
                        .send_control(&message("error", vec![("message", "hello timeout".into())]));
                    client.mux.close(4000, "hello timeout");
                }
                let mut wait = PING_INTERVAL.saturating_sub(HELLO_TIMEOUT);
                while !client.mux.wait_closed(wait) {
                    client.mux.send_control(&message("ping", vec![]));
                    wait = PING_INTERVAL;
                }
            });
        }

        let state = self.clone();
        let handler_client = client.clone();
        let info = mux.run(move |msg| state.handle_control(&handler_client, msg));
        logger::debug(format!(
            "Client {} disconnected (code {})",
            client.remote_addr, info.code
        ));
        self.drop_client(&client);
    }
}
