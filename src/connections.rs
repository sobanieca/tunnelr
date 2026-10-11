//! Saved connections: name -> VPS address, ports and key, kept in
//! ~/.config/tunnelr/connections.json so "tunnelr" alone can open the last
//! used tunnel again. The key is not copied here, only the -a value as given
//! (usually a path to the key file).

use crate::auth::{home, write_private_file};
use crate::client::parse_target;
use crate::json::{self, Value};
use crate::protocol::{Mapping, parse_port_mappings, split_port_list};
use std::path::{Path, PathBuf};

/// Names that are commands, so they cannot name a connection.
pub const RESERVED_NAMES: [&str; 9] = [
    "help", "version", "update", "service", "token", "server", "add", "rm", "ls",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Connection {
    pub name: String,
    /// host[:port] of the VPS
    pub target: String,
    /// "remote:local" entries
    pub ports: Vec<String>,
    /// -a value: key file path or the key itself
    pub auth: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Store {
    pub last: String,
    pub connections: Vec<Connection>,
}

pub fn connections_path() -> PathBuf {
    home()
        .join(".config")
        .join("tunnelr")
        .join("connections.json")
}

fn string_field(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_string)
}

fn connection_from_json(value: &Value) -> Option<Connection> {
    Some(Connection {
        name: string_field(value, "name")?,
        target: string_field(value, "target").unwrap_or_default(),
        ports: value
            .get("ports")
            .and_then(Value::as_array)
            .map(|ports| ports.iter().map(Value::to_plain_string).collect())
            .unwrap_or_default(),
        auth: string_field(value, "auth"),
        to: string_field(value, "to"),
    })
}

fn connection_to_json(connection: &Connection) -> Value {
    let mut entries = vec![
        ("name", connection.name.as_str().into()),
        ("target", connection.target.as_str().into()),
        (
            "ports",
            Value::Array(connection.ports.iter().map(|p| p.as_str().into()).collect()),
        ),
    ];
    if let Some(auth) = &connection.auth {
        entries.push(("auth", auth.as_str().into()));
    }
    if let Some(to) = &connection.to {
        entries.push(("to", to.as_str().into()));
    }
    Value::object(entries)
}

pub fn load(path: &Path) -> Store {
    let Ok(data) = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|text| json::parse(&text))
    else {
        return Store::default();
    };
    Store {
        last: string_field(&data, "last").unwrap_or_default(),
        connections: data
            .get("connections")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(connection_from_json).collect())
            .unwrap_or_default(),
    }
}

pub fn save(store: &Store, path: &Path) -> Result<(), String> {
    let data = Value::object(vec![
        ("last", store.last.as_str().into()),
        (
            "connections",
            Value::Array(store.connections.iter().map(connection_to_json).collect()),
        ),
    ]);
    write_private_file(path, &(json::stringify_pretty(&data) + "\n"))
        .map_err(|err| format!("Cannot save {}: {err}", path.display()))
}

pub fn format_ports(mappings: &[Mapping]) -> Vec<String> {
    mappings
        .iter()
        .map(|m| {
            if m.remote == m.local {
                m.remote.to_string()
            } else {
                format!("{}:{}", m.remote, m.local)
            }
        })
        .collect()
}

/// Validates and saves a connection. An existing one with the same name is
/// replaced. The new connection becomes the last used one.
pub fn add(connection: Connection, path: &Path) -> Result<Connection, String> {
    let name = connection.name.trim().to_string();
    if name.is_empty() {
        return Err("Give the connection a name".to_string());
    }
    if RESERVED_NAMES.contains(&name.as_str()) {
        return Err(format!(
            "\"{name}\" is a tunnelr command, pick another name"
        ));
    }
    let target = connection.target.trim().to_string();
    if target.is_empty() {
        return Err("Give the VPS address, example: 1.2.3.4:20185".to_string());
    }
    parse_target(&target)?;
    let saved = Connection {
        name: name.clone(),
        target,
        ports: format_ports(&parse_port_mappings(&connection.ports)?),
        auth: connection.auth.filter(|auth| !auth.is_empty()),
        to: connection.to.filter(|to| !to.is_empty()),
    };
    let mut store = load(path);
    store.connections.retain(|c| c.name != name);
    store.connections.push(saved.clone());
    store.last = name;
    save(&store, path)?;
    Ok(saved)
}

/// Removes a connection and returns it.
pub fn remove(name: &str, path: &Path) -> Result<Connection, String> {
    let mut store = load(path);
    let index = store
        .connections
        .iter()
        .position(|c| c.name == name)
        .ok_or_else(|| format!("No connection named \"{name}\", see: tunnelr ls"))?;
    let removed = store.connections.remove(index);
    if store.last == name {
        store.last = store
            .connections
            .last()
            .map(|c| c.name.clone())
            .unwrap_or_default();
    }
    save(&store, path)?;
    Ok(removed)
}

pub fn find(name: &str, path: &Path) -> Option<Connection> {
    load(path).connections.into_iter().find(|c| c.name == name)
}

pub fn find_last(path: &Path) -> Option<Connection> {
    let store = load(path);
    let last = store
        .connections
        .iter()
        .find(|c| c.name == store.last)
        .cloned();
    last.or_else(|| store.connections.last().cloned())
}

pub fn mark_last(name: &str, path: &Path) -> Result<(), String> {
    let mut store = load(path);
    if store.last == name || !store.connections.iter().any(|c| c.name == name) {
        return Ok(());
    }
    store.last = name.to_string();
    save(&store, path)
}

/// Ports to open: the saved ones, unless -p is given. A bare port in -p (no
/// "remote:local") keeps the saved VPS port at the same position and only
/// changes the local port, so with saved "30185:3000" the flag -p 4000 opens
/// 30185:4000. Extra bare ports open the same port on both sides.
pub fn resolve_ports(saved_ports: &[String], overrides: &[String]) -> Result<Vec<Mapping>, String> {
    let saved = parse_port_mappings(saved_ports)?;
    let list = split_port_list(overrides);
    if list.is_empty() {
        return Ok(saved);
    }
    let merged: Vec<String> = list
        .iter()
        .enumerate()
        .map(|(i, item)| match saved.get(i) {
            Some(saved) if !item.contains(':') => format!("{}:{item}", saved.remote),
            _ => item.clone(),
        })
        .collect();
    parse_port_mappings(&merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn conn(name: &str, target: &str, ports: &[&str]) -> Connection {
        Connection {
            name: name.into(),
            target: target.into(),
            ports: strings(ports),
            ..Default::default()
        }
    }

    #[test]
    fn add_list_last_remove() {
        let dir = std::env::temp_dir().join(format!("tunnelr-conn-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("sub").join("connections.json");
        assert_eq!(load(&path), Store::default());
        assert_eq!(find_last(&path), None);

        let vps = add(conn("vps", "1.2.3.4:20185", &["30185:3000", "4000"]), &path).unwrap();
        assert_eq!(vps, conn("vps", "1.2.3.4:20185", &["30185:3000", "4000"]));
        let home = Connection {
            auth: Some("~/k".into()),
            ..conn("home", "home.example.com", &[])
        };
        add(home, &path).unwrap();
        let names: Vec<String> = load(&path)
            .connections
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, vec!["vps", "home"]);
        assert_eq!(find_last(&path).unwrap().name, "home");
        assert_eq!(find("home", &path).unwrap().auth.as_deref(), Some("~/k"));
        assert_eq!(find("nope", &path), None);

        mark_last("vps", &path).unwrap();
        assert_eq!(find_last(&path).unwrap().name, "vps");
        mark_last("nope", &path).unwrap();
        assert_eq!(find_last(&path).unwrap().name, "vps");

        add(conn("home", "h2", &["80"]), &path).unwrap();
        assert_eq!(load(&path).connections.len(), 2);
        assert_eq!(find("home", &path).unwrap().target, "h2");
        assert_eq!(find_last(&path).unwrap().name, "home");

        assert_eq!(remove("home", &path).unwrap().target, "h2");
        assert_eq!(find_last(&path).unwrap().name, "vps");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        for bad in [
            conn("ls", "x", &[]),
            conn("", "x", &[]),
            conn("a", "", &[]),
            conn("a", "x", &["70000"]),
        ] {
            assert!(add(bad.clone(), &path).is_err(), "should reject {bad:?}");
        }
        assert!(remove("nope", &path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn override_keeps_the_vps_port() {
        let m = |remote, local| Mapping { remote, local };
        let saved = strings(&["30185:3000", "30186"]);
        assert_eq!(
            resolve_ports(&saved, &[]),
            Ok(vec![m(30185, 3000), m(30186, 30186)])
        );
        assert_eq!(
            resolve_ports(&saved, &strings(&["4000"])),
            Ok(vec![m(30185, 4000)])
        );
        assert_eq!(
            resolve_ports(&saved, &strings(&["4000,5000", "6000"])),
            Ok(vec![m(30185, 4000), m(30186, 5000), m(6000, 6000)])
        );
        assert_eq!(
            resolve_ports(&saved, &strings(&["30190:80"])),
            Ok(vec![m(30190, 80)])
        );
        assert_eq!(
            resolve_ports(&[], &strings(&["3000"])),
            Ok(vec![m(3000, 3000)])
        );
    }
}
