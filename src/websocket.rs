//! WebSocket (RFC 6455) over blocking sockets: client handshake for
//! `ws://` and `wss://` (TLS with the Mozilla root certificates), the server
//! accept key, and message framing. Connections are split into a read half
//! and a write half so one thread can read while another writes.

use crate::auth::random_bytes;
use crate::net;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
const MAX_HEADER_SIZE: usize = 16 * 1024;

pub const OP_CONTINUATION: u8 = 0x0;
pub const OP_TEXT: u8 = 0x1;
pub const OP_BINARY: u8 = 0x2;
pub const OP_CLOSE: u8 = 0x8;
pub const OP_PING: u8 = 0x9;
pub const OP_PONG: u8 = 0xA;

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `Sec-WebSocket-Accept` for a `Sec-WebSocket-Key`.
pub fn accept_key(key: &str) -> String {
    let digest = ring::digest::digest(
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        format!("{}{GUID}", key.trim()).as_bytes(),
    );
    base64(digest.as_ref())
}

pub type Reader = BufReader<Box<dyn Read + Send>>;
pub type Writer = Box<dyn Write + Send>;

/// An open WebSocket: read half, write half and the TCP socket underneath
/// (for timeouts and shutdown).
pub struct Socket {
    pub reader: Reader,
    pub writer: Writer,
    pub tcp: TcpStream,
    /// Clients mask their frames, servers do not.
    pub mask: bool,
}

/// A received message.
#[derive(Debug, PartialEq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close(Option<u16>, String),
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Reads one frame: (fin, opcode, payload).
fn read_frame(reader: &mut Reader) -> io::Result<(bool, u8, Vec<u8>)> {
    let mut head = [0u8; 2];
    reader.read_exact(&mut head)?;
    let fin = head[0] & 0x80 != 0;
    let opcode = head[0] & 0x0F;
    let masked = head[1] & 0x80 != 0;
    let len = match head[1] & 0x7F {
        126 => {
            let mut ext = [0u8; 2];
            reader.read_exact(&mut ext)?;
            u64::from(u16::from_be_bytes(ext))
        }
        127 => {
            let mut ext = [0u8; 8];
            reader.read_exact(&mut ext)?;
            u64::from_be_bytes(ext)
        }
        n => u64::from(n),
    };
    if len > MAX_MESSAGE_SIZE as u64 {
        return Err(invalid("WebSocket frame too large"));
    }
    let mut key = [0u8; 4];
    if masked {
        reader.read_exact(&mut key)?;
    }
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload)?;
    if masked {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= key[i % 4];
        }
    }
    Ok((fin, opcode, payload))
}

/// Reads messages, joining fragmented ones. Control frames may arrive
/// between the fragments of a message.
pub struct MessageReader {
    reader: Reader,
    partial: Option<(u8, Vec<u8>)>,
}

impl MessageReader {
    pub fn new(reader: Reader) -> Self {
        MessageReader {
            reader,
            partial: None,
        }
    }

    pub fn read(&mut self) -> io::Result<Message> {
        loop {
            let (fin, opcode, payload) = read_frame(&mut self.reader)?;
            let (opcode, data) = match opcode {
                OP_PING => return Ok(Message::Ping(payload)),
                OP_PONG => return Ok(Message::Pong),
                OP_CLOSE => {
                    let code = payload
                        .get(..2)
                        .map(|code| u16::from_be_bytes([code[0], code[1]]));
                    let reason = String::from_utf8_lossy(payload.get(2..).unwrap_or_default());
                    return Ok(Message::Close(code, reason.into_owned()));
                }
                OP_TEXT | OP_BINARY if self.partial.is_none() => (opcode, payload),
                OP_CONTINUATION => {
                    let (opcode, mut data) = self
                        .partial
                        .take()
                        .ok_or_else(|| invalid("unexpected continuation frame"))?;
                    if data.len() + payload.len() > MAX_MESSAGE_SIZE {
                        return Err(invalid("WebSocket message too large"));
                    }
                    data.extend_from_slice(&payload);
                    (opcode, data)
                }
                _ => return Err(invalid("unexpected WebSocket opcode")),
            };
            if !fin {
                self.partial = Some((opcode, data));
                continue;
            }
            return Ok(if opcode == OP_TEXT {
                Message::Text(
                    String::from_utf8(data).map_err(|_| invalid("text frame is not UTF-8"))?,
                )
            } else {
                Message::Binary(data)
            });
        }
    }
}

/// Encodes one complete frame.
pub fn encode_frame(opcode: u8, payload: &[u8], mask: bool) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | opcode);
    let mask_bit = if mask { 0x80 } else { 0 };
    match payload.len() {
        n if n < 126 => frame.push(mask_bit | n as u8),
        n if n <= 0xFFFF => {
            frame.push(mask_bit | 126);
            frame.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            frame.push(mask_bit | 127);
            frame.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    if mask {
        let key = random_bytes::<4>();
        frame.extend_from_slice(&key);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
    } else {
        frame.extend_from_slice(payload);
    }
    frame
}

pub fn close_payload(code: u16, reason: &str) -> Vec<u8> {
    let mut payload = code.to_be_bytes().to_vec();
    let mut end = reason.len().min(123);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    payload.extend_from_slice(&reason.as_bytes()[..end]);
    payload
}

/// Reads HTTP headers (up to the empty line), returns the lines.
pub fn read_head(reader: &mut Reader) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut total = 0;
    loop {
        let mut line = String::new();
        let n = reader
            .by_ref()
            .take(MAX_HEADER_SIZE as u64)
            .read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed",
            ));
        }
        total += n;
        if total > MAX_HEADER_SIZE {
            return Err(invalid("HTTP headers too large"));
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        if line.is_empty() {
            return Ok(lines);
        }
        lines.push(line);
    }
}

pub fn header<'a>(lines: &'a [String], name: &str) -> Option<&'a str> {
    lines.iter().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

/// Connects to `ws://host:port/path` or `wss://...` and does the handshake.
pub fn connect(host: &str, port: u16, path: &str, secure: bool) -> io::Result<Socket> {
    let tcp = net::connect(host, port)?;
    tcp.set_read_timeout(Some(net::CONNECT_TIMEOUT))?;
    let (reader, writer): (Box<dyn Read + Send>, Writer) = if secure {
        tls_split(host, tcp.try_clone()?)?
    } else {
        (Box::new(tcp.try_clone()?), Box::new(tcp.try_clone()?))
    };
    let mut socket = Socket {
        reader: BufReader::new(reader),
        writer,
        tcp,
        mask: true,
    };

    let key = base64(&random_bytes::<16>());
    let default_port = if secure { 443 } else { 80 };
    let host_header = if port == default_port {
        net::format_host(host.trim_matches(['[', ']']))
    } else {
        format!("{}:{port}", net::format_host(host.trim_matches(['[', ']'])))
    };
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nUser-Agent: tunnelr/{}\r\n\r\n",
        crate::VERSION
    );
    socket.writer.write_all(request.as_bytes())?;
    socket.writer.flush()?;

    let head = read_head(&mut socket.reader)?;
    let status = head.first().map(String::as_str).unwrap_or_default();
    if status.split_whitespace().nth(1) != Some("101") {
        return Err(io::Error::other(format!(
            "server did not accept the WebSocket ({status})"
        )));
    }
    if header(&head, "sec-websocket-accept") != Some(accept_key(&key).as_str()) {
        return Err(io::Error::other("invalid WebSocket handshake response"));
    }
    socket.tcp.set_read_timeout(None)?;
    Ok(socket)
}

fn tls_split(host: &str, mut tcp: TcpStream) -> io::Result<(Box<dyn Read + Send>, Writer)> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.trim_matches(['[', ']']).to_string())
        .map_err(|_| invalid("invalid host name"))?;
    let mut conn =
        rustls::ClientConnection::new(Arc::new(config), name).map_err(io::Error::other)?;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp)?;
    }
    let shared = Arc::new(Mutex::new(conn));
    let reader = TlsReader {
        conn: shared.clone(),
        tcp: tcp.try_clone()?,
        out: tcp.try_clone()?,
        buffer: vec![0u8; 32 * 1024],
        pending: Vec::new(),
        offset: 0,
        eof: false,
    };
    let writer = TlsWriter {
        conn: shared,
        out: tcp,
    };
    Ok((Box::new(reader), Box::new(writer)))
}

/// Read half of a TLS connection: reads the socket without holding the lock,
/// then decrypts under the lock.
struct TlsReader {
    conn: Arc<Mutex<rustls::ClientConnection>>,
    tcp: TcpStream,
    out: TcpStream,
    buffer: Vec<u8>,
    pending: Vec<u8>,
    offset: usize,
    eof: bool,
}

impl Read for TlsReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.offset < self.pending.len() {
                let n = buf.len().min(self.pending.len() - self.offset);
                buf[..n].copy_from_slice(&self.pending[self.offset..self.offset + n]);
                self.offset += n;
                return Ok(n);
            }
            self.pending.clear();
            self.offset = 0;
            if self.eof {
                return Ok(0);
            }
            let n = self.tcp.read(&mut self.buffer)?;
            if n == 0 {
                self.eof = true;
                continue;
            }
            let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            let mut input = &self.buffer[..n];
            while !input.is_empty() {
                conn.read_tls(&mut input)?;
                conn.process_new_packets().map_err(io::Error::other)?;
                let mut chunk = [0u8; 16 * 1024];
                loop {
                    match conn.reader().read(&mut chunk) {
                        Ok(0) => {
                            self.eof = true;
                            break;
                        }
                        Ok(k) => self.pending.extend_from_slice(&chunk[..k]),
                        Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                        Err(err) => return Err(err),
                    }
                }
            }
            while conn.wants_write() {
                conn.write_tls(&mut self.out)?;
            }
        }
    }
}

struct TlsWriter {
    conn: Arc<Mutex<rustls::ClientConnection>>,
    out: TcpStream,
}

impl Write for TlsWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let n = conn.writer().write(buf)?;
        while conn.wants_write() {
            conn.write_tls(&mut self.out)?;
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(bytes: Vec<u8>) -> MessageReader {
        MessageReader::new(BufReader::new(Box::new(io::Cursor::new(bytes))))
    }

    #[test]
    fn encodes_base64_and_accept_key() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn frames_round_trip() {
        for size in [0, 5, 125, 126, 70_000] {
            for mask in [false, true] {
                let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
                let frame = encode_frame(OP_BINARY, &payload, mask);
                let message = reader(frame).read().unwrap();
                assert_eq!(message, Message::Binary(payload));
            }
        }
        let mut bytes = encode_frame(OP_CLOSE, &close_payload(4001, "wrong key"), true);
        bytes.extend(encode_frame(OP_PING, b"x", false));
        let mut r = reader(bytes);
        assert_eq!(
            r.read().unwrap(),
            Message::Close(Some(4001), "wrong key".into())
        );
        assert_eq!(r.read().unwrap(), Message::Ping(b"x".to_vec()));
    }

    #[test]
    fn joins_fragments() {
        let mut first = encode_frame(OP_TEXT, b"hel", false);
        first[0] &= 0x7F;
        let mut bytes = first;
        bytes.extend(encode_frame(OP_PING, b"", false));
        bytes.extend(encode_frame(OP_CONTINUATION, b"lo", false));
        let mut r = reader(bytes);
        assert_eq!(r.read().unwrap(), Message::Ping(vec![]));
        assert_eq!(r.read().unwrap(), Message::Text("hello".into()));
    }
}
