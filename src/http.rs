//! Just enough HTTP/1.1 for the control port: read a request head, answer
//! with JSON and close the connection.

use crate::json::{self, Value};
use crate::websocket::{self, Reader};
use std::io::{self, BufReader, Read, Write};
use std::net::TcpStream;

pub struct Request {
    pub method: String,
    /// Path without query and trailing slashes ("/" for the root).
    pub path: String,
    pub query: String,
    pub head: Vec<String>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        websocket::header(&self.head, name)
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key == name).then_some(value)
        })
    }
}

pub fn read_request(stream: &TcpStream) -> io::Result<(Request, Reader)> {
    let mut reader: Reader = BufReader::new(Box::new(stream.try_clone()?));
    let head = websocket::read_head(&mut reader)?;
    let mut parts = head
        .first()
        .map(String::as_str)
        .unwrap_or_default()
        .split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or("/");
    let target = target
        .find("://")
        .and_then(|i| target[i + 3..].find('/').map(|j| &target[i + 3 + j..]))
        .unwrap_or(target);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = path.trim_end_matches('/');
    let request = Request {
        method,
        path: if path.is_empty() {
            "/".to_string()
        } else {
            path.to_string()
        },
        query: query.to_string(),
        head,
    };
    let body_length = request
        .header("content-length")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if body_length > 0 {
        io::copy(
            &mut reader.by_ref().take(body_length.min(64 * 1024)),
            &mut io::sink(),
        )?;
    }
    Ok((request, reader))
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    }
}

/// `JSON.stringify(body, null, 2) + "\n"` with `Connection: close`.
pub fn respond_json(mut stream: &TcpStream, status: u16, body: &Value) -> io::Result<()> {
    let body = json::stringify_pretty(body) + "\n";
    let response = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        reason_phrase(status),
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

/// Accepts a WebSocket upgrade.
pub fn accept_websocket(mut stream: &TcpStream, request: &Request) -> io::Result<()> {
    let key = request.header("sec-websocket-key").unwrap_or_default();
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {}\r\n\r\n",
        websocket::accept_key(key)
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}
