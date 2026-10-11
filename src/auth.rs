//! Auth: the key is a shared secret that never travels over the wire.
//! The client sends a one-time token "<timestamp>.<nonce>.<hmac>" signed with
//! the key. The server checks the signature, rejects tokens older than
//! TOKEN_TTL_MS and remembers used nonces, so a captured token is useless.

use crate::logger;
use crate::time::now_ms;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const KEY_LENGTH: usize = 32;
const NONCE_BYTES: usize = 12;
const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
pub const TOKEN_TTL_MS: u64 = 60_000;

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .expect("system random generator");
    bytes
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn generate_key() -> String {
    random_bytes::<KEY_LENGTH>()
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect()
}

pub fn home() -> PathBuf {
    ["HOME", "USERPROFILE"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_key_path() -> PathBuf {
    home().join(".secret").join("tunnelr-key")
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/') || value.starts_with('~') || value.starts_with('.')
}

pub fn read_key_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// A key and the file it lives in (`None` when the key was given directly).
#[derive(Debug, PartialEq)]
pub struct ResolvedKey {
    pub key: String,
    pub path: Option<PathBuf>,
}

/// Resolves the -a value: a path to a key file, the key itself, or nothing
/// (then ~/.secret/tunnelr-key).
pub fn resolve_key(auth: Option<&str>) -> ResolvedKey {
    let value = auth.unwrap_or_default().trim();
    if value.is_empty() {
        let path = default_key_path();
        return ResolvedKey {
            key: read_key_file(&path),
            path: Some(path),
        };
    }
    if looks_like_path(value) {
        let path = expand_home(value);
        return ResolvedKey {
            key: read_key_file(&path),
            path: Some(path),
        };
    }
    let from_file = read_key_file(Path::new(value));
    if !from_file.is_empty() {
        return ResolvedKey {
            key: from_file,
            path: Some(PathBuf::from(value)),
        };
    }
    ResolvedKey {
        key: value.to_string(),
        path: None,
    }
}

/// Client side: the key must exist.
pub fn find_key(auth: Option<&str>) -> Result<String, String> {
    let resolved = resolve_key(auth);
    if resolved.key.is_empty() {
        let path = resolved.path.unwrap_or_else(default_key_path);
        return Err(format!(
            "Key file {} is missing or empty. Save the key printed by the server there, or pass -a <key>",
            path.display()
        ));
    }
    Ok(resolved.key)
}

/// Server side: uses the key from -a or the key file, creates and saves a new
/// one when there is none. A key given directly is saved to
/// ~/.secret/tunnelr-key. Returns the key and the file it is saved in.
pub fn find_or_create_key(auth: Option<&str>) -> Result<(String, PathBuf), String> {
    let resolved = resolve_key(auth);
    let target = resolved.path.unwrap_or_else(default_key_path);
    if !resolved.key.is_empty() && resolved.key == read_key_file(&target) {
        return Ok((resolved.key, target));
    }
    let key = if resolved.key.is_empty() {
        generate_key()
    } else {
        resolved.key
    };
    write_private_file(&target, &format!("{key}\n"))
        .map_err(|err| format!("Cannot save the key to {}: {err}", target.display()))?;
    logger::info(format!("Key saved to {}", target.display()));
    Ok((key, target))
}

/// Writes a file readable only by its owner, creating parent directories.
pub fn write_private_file(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(content.as_bytes())
}

fn sign(key: &str, text: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes());
    to_hex(hmac::sign(&key, text.as_bytes()).as_ref())
}

/// One-time token "<timestamp>.<nonce>.<hmac>" signed with the key.
pub fn create_token(key: &str) -> String {
    let ts = now_ms();
    let nonce = to_hex(&random_bytes::<NONCE_BYTES>());
    let mac = sign(key, &format!("{ts}.{nonce}"));
    format!("{ts}.{nonce}.{mac}")
}

/// Checks tokens and remembers used nonces.
pub struct TokenVerifier {
    key: String,
    ttl_ms: u64,
    /// nonce -> when it can be forgotten
    used: Mutex<HashMap<String, u64>>,
}

impl TokenVerifier {
    pub fn new(key: &str) -> Self {
        Self::with_ttl(key, TOKEN_TTL_MS)
    }

    pub fn with_ttl(key: &str, ttl_ms: u64) -> Self {
        TokenVerifier {
            key: key.to_string(),
            ttl_ms,
            used: Mutex::new(HashMap::new()),
        }
    }

    /// Returns `Err(reason)` when the token is not fine.
    pub fn verify(&self, token: &str) -> Result<(), String> {
        let mut parts = token.split('.');
        let (ts_text, nonce, mac) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        let ts = match ts_text.parse::<f64>() {
            Ok(ts) if ts.is_finite() && !nonce.is_empty() && !mac.is_empty() => ts,
            _ => return Err("malformed token".to_string()),
        };
        let now = now_ms();
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        used.retain(|_, expires| *expires > now);
        if (now as f64 - ts).abs() > self.ttl_ms as f64 {
            return Err("token expired (check the clock on both machines)".to_string());
        }
        if !safe_equal(mac, &sign(&self.key, &format!("{ts_text}.{nonce}"))) {
            return Err("wrong key".to_string());
        }
        if used.contains_key(nonce) {
            return Err("token already used".to_string());
        }
        used.insert(nonce.to_string(), ts as u64 + self.ttl_ms);
        Ok(())
    }
}

/// Compares strings in constant time.
pub fn safe_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "test-key-123";

    #[test]
    fn tokens_are_one_time_and_expire() {
        let verifier = TokenVerifier::with_ttl(KEY, 1000);
        let token = create_token(KEY);
        assert_eq!(verifier.verify(&token), Ok(()));
        assert_eq!(verifier.verify(&token), Err("token already used".into()));
        assert_eq!(verifier.verify(&create_token(KEY)), Ok(()));
        assert_eq!(
            verifier.verify(&create_token("other")),
            Err("wrong key".into())
        );
        assert_eq!(verifier.verify("garbage"), Err("malformed token".into()));

        let parts: Vec<&str> = token.split('.').collect();
        let ts: u64 = parts[0].parse().unwrap();
        let forged = format!("{}.{}.{}", ts + 1, parts[1], parts[2]);
        assert_eq!(verifier.verify(&forged), Err("wrong key".into()));

        let fresh = create_token(KEY);
        let parts: Vec<&str> = fresh.split('.').collect();
        let old = format!("{}.{}.{}", now_ms() - 5000, parts[1], parts[2]);
        assert!(
            verifier
                .verify(&old)
                .unwrap_err()
                .starts_with("token expired")
        );
    }

    #[test]
    fn compares_in_constant_time() {
        assert!(safe_equal("abc", "abc"));
        assert!(!safe_equal("abc", "abd"));
        assert!(!safe_equal("abc", "abcd"));
        assert!(!safe_equal("", "a"));
    }

    #[test]
    fn key_from_file_or_value() {
        let dir = std::env::temp_dir().join(format!("tunnelr-key-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        std::fs::write(&path, "  file-key \n").unwrap();
        let path_text = path.to_str().unwrap();

        assert_eq!(find_key(Some(path_text)), Ok("file-key".to_string()));
        assert_eq!(find_key(Some("literalKey")), Ok("literalKey".to_string()));
        assert_eq!(
            find_or_create_key(Some(path_text)),
            Ok(("file-key".to_string(), path.clone()))
        );
        let missing = dir.join("missing");
        assert!(find_key(Some(missing.to_str().unwrap())).is_err());

        let new_path = dir.join("new").join("key");
        let (key, saved) = find_or_create_key(Some(new_path.to_str().unwrap())).unwrap();
        assert_eq!(saved, new_path);
        assert_eq!(key.len(), KEY_LENGTH);
        assert_eq!(read_key_file(&new_path), key);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&new_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            find_or_create_key(Some(new_path.to_str().unwrap())),
            Ok((key, new_path))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
