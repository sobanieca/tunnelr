//! Networking helpers: dual-stack listening, an accept loop that can be
//! stopped, local and public address detection.

use crate::logger;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const PUBLIC_IP_URLS: [&str; 2] = ["https://api.ipify.org", "https://api64.ipify.org"];
const PUBLIC_IP_TIMEOUT: Duration = Duration::from_secs(3);
const ACCEPT_POLL_MS: i32 = 200;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Listens on all interfaces: dual-stack "::" first, then IPv4 only.
/// `bind` forces a specific address.
pub fn listen_tcp(port: u16, bind: Option<&str>) -> io::Result<TcpListener> {
    let hosts: Vec<&str> = match bind {
        Some(bind) => vec![bind],
        None => vec!["::", "0.0.0.0"],
    };
    let mut last_error = io::Error::other(format!("cannot listen on port {port}"));
    for host in hosts {
        match TcpListener::bind((host.trim_matches(['[', ']']), port)) {
            Ok(listener) => return Ok(listener),
            Err(err) if err.kind() == io::ErrorKind::AddrInUse => return Err(err),
            Err(err) => {
                logger::debug(format!("Cannot listen on {host}:{port}: {err}"));
                last_error = err;
            }
        }
    }
    Err(last_error)
}

/// Accepts connections until `stop` is set. The listener is checked every
/// 200 ms, so the port is free shortly after `stop` is set.
pub fn accept_until(
    listener: &TcpListener,
    stop: &AtomicBool,
    mut on_connection: impl FnMut(TcpStream, SocketAddr),
) {
    let mut poll = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    while !stop.load(Ordering::SeqCst) {
        // SAFETY: one valid pollfd that lives for the duration of the call.
        let ready = unsafe { libc::poll(&mut poll, 1, ACCEPT_POLL_MS) };
        if ready <= 0 || stop.load(Ordering::SeqCst) {
            continue;
        }
        match listener.accept() {
            Ok((stream, addr)) => {
                let _ = stream.set_nodelay(true);
                on_connection(stream, addr)
            }
            Err(err) => {
                logger::debug(format!("Accept failed: {err}"));
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Connects to `host:port`, trying every address the name resolves to.
pub fn connect(host: &str, port: u16) -> io::Result<TcpStream> {
    let mut last_error = io::Error::other(format!("cannot resolve {host}"));
    for addr in (host.trim_matches(['[', ']']), port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(err) => last_error = err,
        }
    }
    Err(last_error)
}

/// Shows IPv4 mapped IPv6 addresses ("::ffff:1.2.3.4") as plain IPv4.
pub fn clean_address(ip: IpAddr) -> String {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => v6.to_string(),
        },
        IpAddr::V4(v4) => v4.to_string(),
    }
}

pub fn format_host(address: &str) -> String {
    if address.contains(':') {
        format!("[{address}]")
    } else {
        address.to_string()
    }
}

fn is_private_v4(address: &str) -> bool {
    address
        .parse::<std::net::Ipv4Addr>()
        .is_ok_and(|ip| ip.is_private())
}

/// Interface addresses other machines may use, public IPv4 first.
pub fn local_addresses() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills a linked list that is freed with freeifaddrs;
    // sockaddr pointers are cast according to their address family.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return found;
        }
        let mut current = list;
        while !current.is_null() {
            let addr = (*current).ifa_addr;
            if !addr.is_null() {
                let ip = match i32::from((*addr).sa_family) {
                    libc::AF_INET => {
                        let v4 = &*(addr as *const libc::sockaddr_in);
                        Some(IpAddr::from(v4.sin_addr.s_addr.to_ne_bytes()))
                    }
                    libc::AF_INET6 => {
                        let v6 = &*(addr as *const libc::sockaddr_in6);
                        Some(IpAddr::from(v6.sin6_addr.s6_addr))
                    }
                    _ => None,
                };
                if let Some(ip) = ip {
                    let link_local = match ip {
                        IpAddr::V4(v4) => v4.is_link_local(),
                        IpAddr::V6(v6) => v6.is_unicast_link_local(),
                    };
                    let text = ip.to_string();
                    if !ip.is_loopback() && !link_local && !found.contains(&text) {
                        found.push(text);
                    }
                }
            }
            current = (*current).ifa_next;
        }
        libc::freeifaddrs(list);
    }
    let score = |a: &String| {
        if a.contains(':') {
            2
        } else if is_private_v4(a) {
            1
        } else {
            0
        }
    };
    found.sort_by_key(score);
    found
}

/// Best effort lookup of the public IP, IPv4 first.
pub fn public_address() -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(PUBLIC_IP_TIMEOUT))
        .build()
        .into();
    for url in PUBLIC_IP_URLS {
        match agent
            .get(url)
            .call()
            .and_then(|mut response| response.body_mut().read_to_string())
        {
            Ok(text) if !text.trim().is_empty() => return Some(text.trim().to_string()),
            Ok(_) => {}
            Err(err) => logger::debug(format!("Public IP lookup at {url} failed: {err}")),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_addresses() {
        assert_eq!(clean_address("::ffff:1.2.3.4".parse().unwrap()), "1.2.3.4");
        assert_eq!(clean_address("::1".parse().unwrap()), "::1");
        assert_eq!(format_host("::1"), "[::1]");
        assert_eq!(format_host("1.2.3.4"), "1.2.3.4");
    }
}
