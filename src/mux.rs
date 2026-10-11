//! Multiplexes many TCP connections over one WebSocket. Both client and
//! server use it.
//!
//! One thread writes to the WebSocket (messages are queued in order), the
//! owner of the mux reads it with `run`. Every stream has a thread reading
//! the TCP connection and a thread writing to it, so a slow connection never
//! blocks the others. Sending stream data waits while more than
//! MAX_BUFFERED_BYTES are queued (backpressure), control messages never wait.

use crate::json::{self, Value};
use crate::logger;
use crate::protocol::{self, decode_frame, encode_frame};
use crate::websocket::{
    self, Message, MessageReader, OP_BINARY, OP_CLOSE, OP_PONG, OP_TEXT, Socket,
};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const MAX_BUFFERED_BYTES: usize = 4 * 1024 * 1024;
const READ_BUFFER_SIZE: usize = 32 * 1024;
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// Close code used when the connection ended without a close frame.
pub const CODE_ABNORMAL: u16 = 1006;

/// Connection counters of one tunnel.
#[derive(Default)]
pub struct Stats {
    pub connections: AtomicU64,
    pub active: AtomicU64,
    pub bytes_read: AtomicU64,
    pub bytes_written: AtomicU64,
}

enum Outgoing {
    Frame(u8, Vec<u8>, usize),
    Shutdown,
}

struct Stream {
    data: mpsc::Sender<Vec<u8>>,
    stats: Option<Arc<Stats>>,
}

/// How the WebSocket ended.
#[derive(Debug)]
pub struct CloseInfo {
    pub code: u16,
    pub reason: String,
    pub timed_out: bool,
}

pub struct Mux {
    outgoing: mpsc::Sender<Outgoing>,
    buffered: Mutex<usize>,
    drained: Condvar,
    open: AtomicBool,
    closed: Mutex<bool>,
    closed_signal: Condvar,
    close_sent: AtomicBool,
    streams: Mutex<HashMap<u32, Stream>>,
    reader: Mutex<Option<MessageReader>>,
    tcp: TcpStream,
    mask: bool,
}

/// Opens a TCP connection for a stream (client side) or hands over an
/// accepted one (server side).
pub type Connector = Box<dyn FnOnce() -> io::Result<TcpStream> + Send>;

impl Mux {
    pub fn new(socket: Socket) -> Arc<Mux> {
        let (sender, receiver) = mpsc::channel();
        let mux = Arc::new(Mux {
            outgoing: sender,
            buffered: Mutex::new(0),
            drained: Condvar::new(),
            open: AtomicBool::new(true),
            closed: Mutex::new(false),
            closed_signal: Condvar::new(),
            close_sent: AtomicBool::new(false),
            streams: Mutex::new(HashMap::new()),
            reader: Mutex::new(Some(MessageReader::new(socket.reader))),
            tcp: socket.tcp,
            mask: socket.mask,
        });
        let writer_mux = mux.clone();
        let writer = socket.writer;
        std::thread::spawn(move || writer_mux.write_loop(writer, receiver));
        mux
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::SeqCst)
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) {
        let _ = self.tcp.set_read_timeout(timeout);
    }

    fn write_loop(
        self: Arc<Self>,
        mut writer: websocket::Writer,
        receiver: mpsc::Receiver<Outgoing>,
    ) {
        for outgoing in receiver {
            let Outgoing::Frame(opcode, payload, counted) = outgoing else {
                break;
            };
            let frame = websocket::encode_frame(opcode, &payload, self.mask);
            let result = writer.write_all(&frame).and_then(|_| writer.flush());
            if counted > 0 {
                let mut buffered = self.buffered.lock().unwrap_or_else(|e| e.into_inner());
                *buffered -= counted;
                self.drained.notify_all();
            }
            if let Err(err) = result {
                logger::debug(format!("WebSocket write failed: {err}"));
                break;
            }
            if opcode == OP_CLOSE {
                let tcp = self.tcp.try_clone();
                std::thread::spawn(move || {
                    std::thread::sleep(CLOSE_GRACE);
                    if let Ok(tcp) = tcp {
                        let _ = tcp.shutdown(Shutdown::Both);
                    }
                });
            }
        }
        self.mark_closed();
        let _ = self.tcp.shutdown(Shutdown::Both);
    }

    fn mark_closed(&self) {
        self.open.store(false, Ordering::SeqCst);
        let buffered = self.buffered.lock().unwrap_or_else(|e| e.into_inner());
        self.drained.notify_all();
        drop(buffered);
        let mut closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
        *closed = true;
        self.closed_signal.notify_all();
    }

    /// Waits up to `timeout` for the WebSocket to end. True when it ended.
    pub fn wait_closed(&self, timeout: Duration) -> bool {
        let closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
        let (closed, _) = self
            .closed_signal
            .wait_timeout_while(closed, timeout, |closed| !*closed)
            .unwrap_or_else(|e| e.into_inner());
        *closed
    }

    pub fn send_control(&self, msg: &Value) {
        if self.is_open() {
            let text = json::stringify(msg);
            let _ = self
                .outgoing
                .send(Outgoing::Frame(OP_TEXT, text.into_bytes(), 0));
        }
    }

    fn send_data(&self, id: u32, data: &[u8]) -> Result<(), ()> {
        let frame = encode_frame(id, data);
        let size = frame.len();
        let mut buffered = self.buffered.lock().unwrap_or_else(|e| e.into_inner());
        while *buffered > MAX_BUFFERED_BYTES && self.is_open() {
            buffered = self
                .drained
                .wait(buffered)
                .unwrap_or_else(|e| e.into_inner());
        }
        if !self.is_open() {
            return Err(());
        }
        *buffered += size;
        drop(buffered);
        self.outgoing
            .send(Outgoing::Frame(OP_BINARY, frame, size))
            .map_err(|_| ())
    }

    /// Starts the closing handshake. Nothing is sent after it.
    pub fn close(&self, code: u16, reason: &str) {
        if self.close_sent.swap(true, Ordering::SeqCst) {
            return;
        }
        self.open.store(false, Ordering::SeqCst);
        let _ = self.outgoing.send(Outgoing::Frame(
            OP_CLOSE,
            websocket::close_payload(code, reason),
            0,
        ));
    }

    /// Reads the WebSocket until it ends, calls `on_control` for every JSON
    /// message and forwards stream data. Returns how the socket ended.
    pub fn run(self: &Arc<Self>, mut on_control: impl FnMut(Value)) -> CloseInfo {
        let Some(mut reader) = self.reader.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return CloseInfo {
                code: CODE_ABNORMAL,
                reason: String::new(),
                timed_out: false,
            };
        };
        let info = loop {
            match reader.read() {
                Ok(Message::Text(text)) => match json::parse(&text) {
                    Ok(msg) => on_control(msg),
                    Err(_) => logger::debug("Ignoring malformed control message"),
                },
                Ok(Message::Binary(frame)) => {
                    if let Some((id, data)) = decode_frame(&frame) {
                        let streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(stream) = streams.get(&id) {
                            let _ = stream.data.send(data.to_vec());
                        }
                    }
                }
                Ok(Message::Ping(payload)) => {
                    if !self.close_sent.load(Ordering::SeqCst) {
                        let _ = self.outgoing.send(Outgoing::Frame(OP_PONG, payload, 0));
                    }
                }
                Ok(Message::Pong) => {}
                Ok(Message::Close(code, reason)) => {
                    if !self.close_sent.swap(true, Ordering::SeqCst) {
                        self.open.store(false, Ordering::SeqCst);
                        let payload = code
                            .map(|code| websocket::close_payload(code, ""))
                            .unwrap_or_default();
                        let _ = self.outgoing.send(Outgoing::Frame(OP_CLOSE, payload, 0));
                    }
                    let _ = self.outgoing.send(Outgoing::Shutdown);
                    break CloseInfo {
                        code: code.unwrap_or(1005),
                        reason,
                        timed_out: false,
                    };
                }
                Err(err) => {
                    let timed_out = matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    );
                    logger::debug(format!("WebSocket read ended: {err}"));
                    break CloseInfo {
                        code: CODE_ABNORMAL,
                        reason: String::new(),
                        timed_out,
                    };
                }
            }
        };
        self.open.store(false, Ordering::SeqCst);
        let _ = self.outgoing.send(Outgoing::Shutdown);
        if info.code == CODE_ABNORMAL {
            let _ = self.tcp.shutdown(Shutdown::Both);
        }
        self.mark_closed();
        info
    }

    pub fn has_stream(&self, id: u32) -> bool {
        self.streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&id)
    }

    /// Binds a stream id to a TCP connection and starts forwarding its data.
    pub fn attach(self: &Arc<Self>, id: u32, connector: Connector, stats: Option<Arc<Stats>>) {
        let (sender, receiver) = mpsc::channel();
        if let Some(stats) = &stats {
            stats.active.fetch_add(1, Ordering::SeqCst);
        }
        self.streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id,
                Stream {
                    data: sender,
                    stats: stats.clone(),
                },
            );
        let mux = self.clone();
        std::thread::spawn(move || mux.stream_loop(id, connector, receiver, stats));
    }

    fn stream_loop(
        self: Arc<Self>,
        id: u32,
        connector: Connector,
        receiver: mpsc::Receiver<Vec<u8>>,
        stats: Option<Arc<Stats>>,
    ) {
        let conn = match connector() {
            Ok(conn) => conn,
            Err(err) => {
                logger::debug(format!("Stream {id} could not connect: {err}"));
                self.close_stream(id, true);
                return;
            }
        };
        if let Ok(mut writer) = conn.try_clone() {
            let mux = self.clone();
            let stats = stats.clone();
            std::thread::spawn(move || {
                for data in receiver {
                    if let Err(err) = writer.write_all(&data) {
                        logger::debug(format!("Stream {id} write failed: {err}"));
                        mux.close_stream(id, true);
                        break;
                    }
                    if let Some(stats) = &stats {
                        stats
                            .bytes_written
                            .fetch_add(data.len() as u64, Ordering::SeqCst);
                    }
                }
                let _ = writer.shutdown(Shutdown::Both);
            });
        } else {
            let _ = conn.shutdown(Shutdown::Both);
        }

        let mut conn = conn;
        let mut buffer = vec![0u8; READ_BUFFER_SIZE];
        while self.has_stream(id) {
            match conn.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if let Some(stats) = &stats {
                        stats.bytes_read.fetch_add(n as u64, Ordering::SeqCst);
                    }
                    if self.send_data(id, &buffer[..n]).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    logger::debug(format!("Stream {id} read ended: {err}"));
                    break;
                }
            }
        }
        self.close_stream(id, true);
    }

    /// Closes a stream. Pending writes are flushed first. `notify` tells the
    /// other side about it.
    pub fn close_stream(&self, id: u32, notify: bool) {
        let removed = self
            .streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        let Some(stream) = removed else { return };
        if let Some(stats) = &stream.stats {
            stats.active.fetch_sub(1, Ordering::SeqCst);
        }
        if notify {
            self.send_control(&protocol::message("close", vec![("id", id.into())]));
        }
    }

    /// Closes the streams counted in `stats` (the streams of one tunnel).
    pub fn close_streams_of(&self, stats: &Arc<Stats>) {
        let ids: Vec<u32> = self
            .streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, stream)| stream.stats.as_ref().is_some_and(|s| Arc::ptr_eq(s, stats)))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.close_stream(id, true);
        }
    }

    pub fn close_all_streams(&self) {
        let ids: Vec<u32> = self
            .streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .copied()
            .collect();
        for id in ids {
            self.close_stream(id, false);
        }
    }
}
