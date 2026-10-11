//! Wire format used between tunnelr client and server over one WebSocket.
//!
//! Text frames carry JSON control messages:
//!   client -> server: { type: "hello", token, ports: [8500, 8600], version }
//!                     token is a one-time token signed with the key, see auth.rs
//!   server -> client: { type: "ready", ports: [8500, 8600] }
//!   server -> client: { type: "error", message }
//!   server -> client: { type: "connect", id, port }   new visitor on a port
//!   both directions:  { type: "close", id }            stream ended
//!   server -> client: { type: "closed", port, reason } port was removed
//!   server -> client: { type: "ping" }, client -> server: { type: "pong" }
//!
//! Binary frames carry stream data: 4 bytes stream id (big endian) + payload.

use crate::json::Value;

const HEADER_SIZE: usize = 4;

pub fn encode_frame(id: u32, data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER_SIZE + data.len());
    frame.extend_from_slice(&id.to_be_bytes());
    frame.extend_from_slice(data);
    frame
}

pub fn decode_frame(frame: &[u8]) -> Option<(u32, &[u8])> {
    let header: [u8; HEADER_SIZE] = frame.get(..HEADER_SIZE)?.try_into().ok()?;
    Some((u32::from_be_bytes(header), &frame[HEADER_SIZE..]))
}

/// A control message `{ type, ...fields }`.
pub fn message(kind: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut entries = vec![("type", Value::from(kind))];
    entries.extend(fields);
    Value::object(entries)
}

pub fn message_type(msg: &Value) -> &str {
    msg.get("type").and_then(Value::as_str).unwrap_or_default()
}

/// Stream id or port number from a control message field.
pub fn number_field(msg: &Value, name: &str) -> Option<u32> {
    let n = msg.get(name)?.as_f64()?;
    (n.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(&n)).then_some(n as u32)
}

pub fn parse_port(value: &str) -> Result<u16, String> {
    let port = value.trim().parse::<u32>().unwrap_or(0);
    if !(1..=65535).contains(&port) {
        return Err(format!("Invalid port: {value}"));
    }
    Ok(port as u16)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapping {
    pub remote: u16,
    pub local: u16,
}

/// Splits `-p` values on commas: `["8500,8600", "9000"]` -> 3 items.
pub fn split_port_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|value| value.split(','))
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Parses `-p` values like "8500", "8500,8600" or "8500:3000" (remote:local).
pub fn parse_port_mappings(values: &[String]) -> Result<Vec<Mapping>, String> {
    let mut mappings: Vec<Mapping> = Vec::new();
    for item in split_port_list(values) {
        let mut parts = item.split(':');
        let remote = parse_port(parts.next().unwrap_or_default())?;
        let local = match parts.next() {
            Some(local) => parse_port(local)?,
            None => remote,
        };
        if mappings.iter().any(|m| m.remote == remote) {
            return Err(format!("Port {remote} was given more than once"));
        }
        mappings.push(Mapping { remote, local });
    }
    Ok(mappings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn m(remote: u16, local: u16) -> Mapping {
        Mapping { remote, local }
    }

    #[test]
    fn frames() {
        let frame = encode_frame(0x01020304, b"hi");
        assert_eq!(frame, vec![1, 2, 3, 4, b'h', b'i']);
        assert_eq!(decode_frame(&frame), Some((0x01020304, &b"hi"[..])));
        assert_eq!(decode_frame(&[1, 2]), None);
    }

    #[test]
    fn port_parsing() {
        assert_eq!(
            parse_port_mappings(&strings(&["8500"])),
            Ok(vec![m(8500, 8500)])
        );
        assert_eq!(
            parse_port_mappings(&strings(&["8500,8600"])),
            Ok(vec![m(8500, 8500), m(8600, 8600)])
        );
        assert_eq!(
            parse_port_mappings(&strings(&["8500:3000", "9000"])),
            Ok(vec![m(8500, 3000), m(9000, 9000)])
        );
        assert!(parse_port_mappings(&strings(&["70000"])).is_err());
        assert!(parse_port_mappings(&strings(&["0"])).is_err());
        assert!(parse_port_mappings(&strings(&["abc"])).is_err());
        assert_eq!(
            parse_port_mappings(&strings(&["80,80:3000"])),
            Err("Port 80 was given more than once".to_string())
        );
    }
}
