//! Minimal JSON: parse, compact and pretty stringify. Object keys keep their
//! insertion order, like in JavaScript.

use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn object(entries: Vec<(&str, Value)>) -> Value {
        Value::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// `String(value)` for strings and numbers, used where JavaScript
    /// converted values loosely.
    pub fn to_plain_string(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Number(n) => format_number(*n),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
            other => stringify(other),
        }
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_string())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Value::Number(value as f64)
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Number(value as f64)
    }
}

impl From<u16> for Value {
    fn from(value: u16) -> Self {
        Value::Number(value as f64)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

fn format_number(n: f64) -> String {
    if !n.is_finite() {
        "null".to_string()
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_value(out: &mut String, value: &Value, indent: Option<usize>, depth: usize) {
    let newline = |out: &mut String, depth: usize| {
        if let Some(width) = indent {
            out.push('\n');
            out.push_str(&" ".repeat(width * depth));
        }
    };
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&format_number(*n)),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_value(out, item, indent, depth + 1);
            }
            newline(out, depth);
            out.push(']');
        }
        Value::Object(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (key, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_string(out, key);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, item, indent, depth + 1);
            }
            newline(out, depth);
            out.push('}');
        }
    }
}

/// `JSON.stringify(value)`
pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None, 0);
    out
}

/// `JSON.stringify(value, null, 2)`
pub fn stringify_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(2), 0);
    out
}

pub fn parse(text: &str) -> Result<Value, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.pos != parser.bytes.len() {
        return Err(parser.error("unexpected data after JSON value"));
    }
    Ok(value)
}

const MAX_DEPTH: usize = 128;

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> String {
        format!("Invalid JSON at position {}: {message}", self.pos)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, literal: &str, value: Value) -> Result<Value, String> {
        if self.bytes[self.pos..].starts_with(literal.as_bytes()) {
            self.pos += literal.len();
            Ok(value)
        } else {
            Err(self.error("unexpected token"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        match self.bytes.get(self.pos) {
            None => Err(self.error("unexpected end")),
            Some(b'n') => self.expect("null", Value::Null),
            Some(b't') => self.expect("true", Value::Bool(true)),
            Some(b'f') => self.expect("false", Value::Bool(false)),
            Some(b'"') => self.string().map(Value::String),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected token")),
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        while matches!(
            self.bytes.get(self.pos),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.pos += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .map(Value::Number)
            .ok_or_else(|| self.error("invalid number"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u32::from_str_radix(d, 16).ok())
            .ok_or_else(|| self.error("invalid unicode escape"))?;
        self.pos += 4;
        Ok(digits)
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while !matches!(self.bytes.get(self.pos), None | Some(b'"' | b'\\')) {
                self.pos += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| self.error("invalid UTF-8"))?,
            );
            match self.bytes.get(self.pos) {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                _ => {
                    self.pos += 1;
                    let escape = *self
                        .bytes
                        .get(self.pos)
                        .ok_or_else(|| self.error("unterminated string"))?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code)
                                && self.bytes[self.pos..].starts_with(b"\\u")
                            {
                                self.pos += 2;
                                let low = self.hex4()?;
                                code = 0x10000
                                    + ((code - 0xD800) << 10)
                                    + (low.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(self.error("invalid escape")),
                    }
                }
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, String> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, String> {
        self.pos += 1;
        let mut entries: Vec<(String, Value)> = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(Value::Object(entries));
        }
        loop {
            self.skip_whitespace();
            if self.bytes.get(self.pos) != Some(&b'"') {
                return Err(self.error("expected a key"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.bytes.get(self.pos) != Some(&b':') {
                return Err(self.error("expected ':'"));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key, value)),
            }
            self.skip_whitespace();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(entries));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let text = r#"{"type":"hello","ports":[8500,8600],"ok":true,"x":null,"s":"a\"b\n\u00e9"}"#;
        let value = parse(text).unwrap();
        assert_eq!(value.get("type").and_then(Value::as_str), Some("hello"));
        assert_eq!(
            stringify(&value),
            "{\"type\":\"hello\",\"ports\":[8500,8600],\"ok\":true,\"x\":null,\"s\":\"a\\\"b\\né\"}"
        );
    }

    #[test]
    fn pretty() {
        let value = Value::object(vec![
            ("ports", Value::Array(vec![])),
            ("list", Value::Array(vec![1u16.into(), "a".into()])),
        ]);
        assert_eq!(
            stringify_pretty(&value),
            "{\n  \"ports\": [],\n  \"list\": [\n    1,\n    \"a\"\n  ]\n}"
        );
    }

    #[test]
    fn rejects_invalid() {
        for text in ["", "{", "[1,]", "{\"a\" 1}", "tru", "1 2", "\"\\x\""] {
            assert!(parse(text).is_err(), "{text}");
        }
        assert_eq!(parse(" 1.5e2 ").unwrap(), Value::Number(150.0));
        assert_eq!(
            parse("\"\\ud83d\\ude00\"").unwrap(),
            Value::String("😀".into())
        );
    }
}
