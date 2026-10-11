//! End-to-end tests: real servers and clients in process on free ports.
//! `TUNNELR_TEST_VERBOSE=1 cargo test` shows the tunnelr logs.

use crate::auth::create_token;
use crate::client::{ClientHandle, ClientOptions, start_client};
use crate::json::{self, Value};
use crate::net;
use crate::protocol::Mapping;
use crate::server::{Server, ServerOptions, start_server};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const KEY: &str = "test-key-123";

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for(timeout: Duration, check: impl Fn() -> bool) {
    let start = Instant::now();
    while !check() {
        assert!(start.elapsed() < timeout, "Timed out waiting for condition");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn server(port: u16) -> Server {
    start_server(ServerOptions {
        port,
        key: KEY.to_string(),
        bind: None,
    })
    .unwrap()
}

/// A local TCP server, stopped when dropped.
struct Local(Arc<AtomicBool>);

impl Drop for Local {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn start_local(port: u16, handle: impl Fn(TcpStream) + Send + Sync + 'static) -> Local {
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let handle = Arc::new(handle);
    let stopper = stop.clone();
    std::thread::spawn(move || {
        net::accept_until(&listener, &stopper, |conn, _| {
            let handle = handle.clone();
            std::thread::spawn(move || handle(conn));
        })
    });
    Local(stop)
}

/// Local TCP echo server that sends back everything it gets.
fn start_echo(port: u16) -> Local {
    start_local(port, |mut conn| {
        let mut buffer = vec![0u8; 64 * 1024];
        while let Ok(n) = conn.read(&mut buffer) {
            if n == 0 || conn.write_all(&buffer[..n]).is_err() {
                break;
            }
        }
    })
}

/// Local HTTP server answering every request with `body(path)`.
fn start_http(port: u16, body: impl Fn(&str) -> String + Send + Sync + 'static) -> Local {
    start_local(port, move |mut conn| {
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match conn.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(n) => request.extend_from_slice(&buffer[..n]),
            }
        }
        let text = String::from_utf8_lossy(&request);
        let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
        let body = body(&path);
        let _ = write!(
            conn,
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
    })
}

/// Sends a raw HTTP request, returns (status, body).
fn http(port: u16, method: &str, path: &str, token: Option<&str>) -> (u16, String) {
    let mut conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let auth = token
        .map(|token| format!("authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    write!(
        conn,
        "{method} {path} HTTP/1.1\r\nhost: 127.0.0.1\r\n{auth}connection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    conn.read_to_string(&mut response).unwrap();
    let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = response.split_once("\r\n\r\n").unwrap().1.to_string();
    (status, body)
}

fn api(port: u16, method: &str, path: &str) -> (u16, Value) {
    let (status, body) = http(port, method, path, Some(&create_token(KEY)));
    (status, json::parse(&body).unwrap())
}

fn listed_ports(control_port: u16) -> Vec<u16> {
    let (_, body) = api(control_port, "GET", "/ports");
    let mut ports: Vec<u16> = body
        .get("ports")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|p| p.get("port").and_then(Value::as_f64).unwrap() as u16)
        .collect();
    ports.sort();
    ports
}

fn client_with(
    control_port: u16,
    key: &str,
    ports: &[(u16, u16)],
) -> (ClientHandle, mpsc::Receiver<Vec<u16>>) {
    let (sender, receiver) = mpsc::channel();
    let sender = std::sync::Mutex::new(sender);
    let client = start_client(ClientOptions {
        target: format!("127.0.0.1:{control_port}"),
        key: key.to_string(),
        ports: ports
            .iter()
            .map(|&(remote, local)| Mapping { remote, local })
            .collect(),
        to: None,
        on_ready: Some(Box::new(move |ports| {
            let _ = sender.lock().unwrap().send(ports);
        })),
    })
    .unwrap();
    (client, receiver)
}

fn connect_client(
    control_port: u16,
    ports: &[(u16, u16)],
) -> (ClientHandle, mpsc::Receiver<Vec<u16>>) {
    let (client, ready) = client_with(control_port, KEY, ports);
    ready
        .recv_timeout(Duration::from_secs(10))
        .expect("client ready");
    (client, ready)
}

fn visit(port: u16) -> TcpStream {
    let conn = TcpStream::connect(("127.0.0.1", port)).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    conn
}

fn read_exact(conn: &mut TcpStream, size: usize) -> Vec<u8> {
    let mut out = vec![0u8; size];
    conn.read_exact(&mut out).unwrap();
    out
}

#[test]
fn echo_data_goes_through_the_tunnel() {
    let (control, local, remote) = (free_port(), free_port(), free_port());
    let server = server(control);
    let _echo = start_echo(local);
    let (client, _) = connect_client(control, &[(remote, local)]);

    let mut conn = visit(remote);
    conn.write_all(b"hello tunnel").unwrap();
    assert_eq!(read_exact(&mut conn, 12), b"hello tunnel");
    drop(conn);

    client.stop();
    assert_eq!(client.wait(), "stopped");
    server.shutdown();
}

#[test]
fn large_payload_keeps_order_and_size() {
    let (control, local, remote) = (free_port(), free_port(), free_port());
    let server = server(control);
    let _echo = start_echo(local);
    let (client, _) = connect_client(control, &[(remote, local)]);

    let size = 3 * 1024 * 1024;
    let payload: Vec<u8> = (0..size).map(|i| (i * 7) as u8).collect();
    let mut conn = visit(remote);
    let mut writer = conn.try_clone().unwrap();
    let sent = payload.clone();
    let writing = std::thread::spawn(move || writer.write_all(&sent).unwrap());
    let reply = read_exact(&mut conn, size);
    writing.join().unwrap();
    assert!(reply == payload, "payload changed on the way");

    client.stop();
    client.wait();
    server.shutdown();
}

#[test]
fn http_server_works_through_the_tunnel() {
    let (control, local, remote) = (free_port(), free_port(), free_port());
    let server = server(control);
    let _http = start_http(local, |path| format!("you asked for {path}"));
    let (client, _) = connect_client(control, &[(remote, local)]);

    assert_eq!(
        http(remote, "GET", "/some/path", None),
        (200, "you asked for /some/path".to_string())
    );

    client.stop();
    client.wait();
    server.shutdown();
}

#[test]
fn multiple_ports_api_list_and_delete() {
    let control = free_port();
    let (local_a, local_b, remote_a, remote_b) =
        (free_port(), free_port(), free_port(), free_port());
    let server = server(control);
    let _echo_a = start_echo(local_a);
    let _echo_b = start_echo(local_b);
    let (client, _) = connect_client(control, &[(remote_a, local_a), (remote_b, local_b)]);

    let mut expected = vec![remote_a, remote_b];
    expected.sort();
    assert_eq!(listed_ports(control), expected);

    for remote in [remote_a, remote_b] {
        let mut conn = visit(remote);
        conn.write_all(b"ping").unwrap();
        assert_eq!(read_exact(&mut conn, 4), b"ping");
    }

    let (_, root) = http(control, "GET", "/", None);
    assert_eq!(
        json::parse(&root)
            .unwrap()
            .get("name")
            .and_then(Value::as_str),
        Some("tunnelr")
    );
    assert_eq!(http(control, "GET", "/ports", None).0, 401);
    assert_eq!(http(control, "GET", "/nope", None).0, 404);

    let token = create_token(KEY);
    let (status, body) = http(
        control,
        "DELETE",
        &format!("/ports/{remote_a}?token={token}"),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(
        json::parse(&body).unwrap().get("closed"),
        Some(&Value::Number(f64::from(remote_a)))
    );
    assert_eq!(
        http(control, "GET", "/ports", Some(&token)).0,
        401,
        "replay"
    );

    assert_eq!(listed_ports(control), vec![remote_b]);
    assert_eq!(api(control, "DELETE", &format!("/ports/{remote_a}")).0, 404);
    assert_eq!(api(control, "POST", "/ports").0, 400);

    let (_, listed) = api(control, "GET", "/ports");
    let entry = &listed.get("ports").and_then(Value::as_array).unwrap()[0];
    assert_eq!(entry.get("totalConnections"), Some(&Value::Number(1.0)));
    assert_eq!(entry.get("bytesIn"), Some(&Value::Number(4.0)));
    assert_eq!(
        entry.get("client").and_then(Value::as_str),
        Some("127.0.0.1")
    );

    assert_eq!(api(control, "DELETE", &format!("/ports/{remote_b}")).0, 200);
    assert_eq!(client.wait(), "stopped");
    assert!(server.list_tunnels().is_empty());
    server.shutdown();
}

#[test]
fn wrong_key_is_rejected() {
    let control = free_port();
    let server = server(control);
    let (client, _) = client_with(control, "wrong", &[(free_port(), 1)]);
    assert_eq!(client.wait(), "wrong key");
    server.shutdown();
}

#[test]
fn control_port_cannot_be_opened() {
    let control = free_port();
    let server = server(control);
    let (client, _) = client_with(control, KEY, &[(control, 1)]);
    assert_eq!(
        client.wait(),
        format!("port {control} is used by the tunnelr server itself")
    );
    server.shutdown();
}

#[test]
fn new_client_takes_over_a_port() {
    let (control, local_a, local_b, remote) = (free_port(), free_port(), free_port(), free_port());
    let server = server(control);
    let _a = start_http(local_a, |_| "A".to_string());
    let _b = start_http(local_b, |_| "B".to_string());

    let (first, _) = connect_client(control, &[(remote, local_a)]);
    assert_eq!(http(remote, "GET", "/", None).1, "A");

    let (second, _) = connect_client(control, &[(remote, local_b)]);
    assert_eq!(first.wait(), "stopped");
    assert_eq!(http(remote, "GET", "/", None).1, "B");

    second.stop();
    second.wait();
    server.shutdown();
}

#[test]
fn client_reconnects_after_server_restart() {
    let (control, local, remote) = (free_port(), free_port(), free_port());
    let first_server = server(control);
    let _echo = start_echo(local);
    let ready_count = Arc::new(AtomicUsize::new(0));
    let (client, ready) = connect_client(control, &[(remote, local)]);
    let counter = ready_count.clone();
    std::thread::spawn(move || {
        while ready.recv().is_ok() {
            counter.fetch_add(1, Ordering::SeqCst);
        }
    });

    first_server.shutdown();
    assert!(first_server.list_tunnels().is_empty());
    std::thread::sleep(Duration::from_millis(200));
    let second_server = server(control);
    wait_for(Duration::from_secs(10), || {
        ready_count.load(Ordering::SeqCst) >= 1
    });
    assert_eq!(second_server.list_tunnels().len(), 1);

    let mut conn = visit(remote);
    conn.write_all(b"back").unwrap();
    assert_eq!(read_exact(&mut conn, 4), b"back");

    client.stop();
    client.wait();
    second_server.shutdown();
}

#[test]
fn local_port_down_closes_the_visitor_connection() {
    let (control, remote) = (free_port(), free_port());
    let server = server(control);
    let (client, _) = connect_client(control, &[(remote, free_port())]);

    let mut conn = visit(remote);
    let mut buffer = [0u8; 16];
    assert_eq!(conn.read(&mut buffer).unwrap(), 0);

    client.stop();
    client.wait();
    server.shutdown();
}

#[test]
fn many_parallel_connections() {
    let (control, local, remote) = (free_port(), free_port(), free_port());
    let server = server(control);
    let _echo = start_echo(local);
    let (client, _) = connect_client(control, &[(remote, local)]);

    let visitors: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                let mut conn = visit(remote);
                let message: Vec<u8> = format!("visitor {i} ").repeat(1000).into_bytes();
                conn.write_all(&message).unwrap();
                assert!(read_exact(&mut conn, message.len()) == message);
            })
        })
        .collect();
    for visitor in visitors {
        visitor.join().unwrap();
    }
    wait_for(Duration::from_secs(5), || {
        server.list_tunnels()[0].active_connections == 0
    });
    assert_eq!(server.list_tunnels()[0].total_connections, 20);

    client.stop();
    client.wait();
    server.shutdown();
}

#[test]
fn closing_a_tunnel_from_the_server_stops_the_client() {
    let (control, remote) = (free_port(), free_port());
    let server = server(control);
    let (client, _) = connect_client(control, &[(remote, free_port())]);
    assert!(server.close_tunnel(remote, "test"));
    assert!(!server.close_tunnel(remote, "test"));
    assert_eq!(client.wait(), "stopped");
    server.shutdown();
}
