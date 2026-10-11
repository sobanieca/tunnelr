//! Runs the `tunnelr` binary in an isolated home directory.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_tunnelr");

fn temp_home(name: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("tunnelr-cli-{name}-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn command(home: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env("HOME", home)
        .env("NO_COLOR", "1")
        .env("TUNNELR_NO_UPDATE_CHECK", "1")
        .env_remove("TUNNELR_SERVICE")
        .env_remove("XDG_CACHE_HOME");
    command
}

fn run(home: &Path, args: &[&str]) -> (i32, String) {
    let Output { status, stdout, .. } = command(home, args).output().unwrap();
    (
        status.code().unwrap_or(-1),
        String::from_utf8_lossy(&stdout).into_owned(),
    )
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn help_and_version() {
    let home = temp_home("help");
    let (code, out) = run(&home, &["--help"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("tunnelr - expose ports"), "{out}");
    assert!(out.contains("HTTP API"));
    assert_eq!(run(&home, &["help"]).1, out);
    assert_eq!(run(&home, &[]).1, out, "no saved connection shows the help");

    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(run(&home, &["version"]), (0, format!("{version}\n")));
    assert_eq!(run(&home, &["-v"]), (0, format!("{version}\n")));
}

#[test]
fn invalid_arguments() {
    let home = temp_home("invalid");
    let (code, out) = run(&home, &["--nope"]);
    assert_eq!(code, 1);
    assert!(
        out.contains("Unknown option: --nope. See: tunnelr --help"),
        "{out}"
    );

    let (code, out) = run(&home, &["1.2.3.4"]);
    assert_eq!(code, 1);
    assert!(out.contains("Give at least one port to open"), "{out}");

    let (code, out) = run(&home, &["1.2.3.4", "-p", "70000"]);
    assert_eq!(code, 1);
    assert!(out.contains("Invalid port: 70000"), "{out}");

    let (code, out) = run(&home, &["server", "-p", "1", "-p", "2"]);
    assert_eq!(code, 1);
    assert!(out.contains("Server needs one control port"), "{out}");
}

#[test]
fn saved_connections() {
    let home = temp_home("connections");
    let (code, out) = run(&home, &["ls"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("No saved connections."), "{out}");

    let (code, out) = run(
        &home,
        &[
            "add",
            "vps",
            "203.0.113.10:20185",
            "-p",
            "30185:3000",
            "-a",
            "~/.secret/k",
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("Saved connection \"vps\": 203.0.113.10:20185 -p 30185:3000\n"));
    run(
        &home,
        &[
            "add",
            "home-lab",
            "10.0.0.5",
            "--to",
            "192.168.1.20",
            "-p",
            "80,443",
        ],
    );

    let (_, out) = run(&home, &["ls"]);
    let path = home.join(".config/tunnelr/connections.json");
    assert_eq!(
        out,
        format!(
            "  vps       203.0.113.10:20185 -p 30185:3000  -a ~/.secret/k\n\
             * home-lab  10.0.0.5 -p 80,443  --to 192.168.1.20\n\n\
             * last used, \"tunnelr\" opens it. Saved in {}\n",
            path.display()
        )
    );

    let (code, out) = run(&home, &["add", "ls", "x"]);
    assert_eq!(code, 1);
    assert!(
        out.contains("\"ls\" is a tunnelr command, pick another name"),
        "{out}"
    );

    assert_eq!(
        run(&home, &["rm", "home-lab"]),
        (
            0,
            "Removed connection \"home-lab\" (10.0.0.5)\n".to_string()
        )
    );
    let (code, out) = run(&home, &["rm", "home-lab"]);
    assert_eq!(code, 1);
    assert!(out.contains("No connection named \"home-lab\""), "{out}");
    assert!(run(&home, &["ls"]).1.starts_with("* vps "));
}

#[test]
fn token() {
    let home = temp_home("token");
    let (code, out) = run(&home, &["token"]);
    assert_eq!(code, 1);
    assert!(out.contains("tunnelr-key is missing or empty"), "{out}");

    let (code, out) = run(&home, &["token", "-a", "some-key"]);
    assert_eq!(code, 0);
    let parts: Vec<&str> = out.trim().split('.').collect();
    assert_eq!(parts.len(), 3, "{out}");
    assert!(parts[0].parse::<u64>().is_ok());
    assert_eq!(parts[1].len(), 24);
    assert_eq!(parts[2].len(), 64);
}

/// Kills the process when dropped.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_port(port: u16) {
    let start = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "port {port} did not open"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn terminate(process: &mut Process) -> Option<i32> {
    // SAFETY: sends SIGTERM to a child process we own.
    unsafe { libc_kill(process.0.id() as i32, 15) };
    let start = Instant::now();
    loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            return status.code();
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "process did not stop"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

unsafe extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, signal: i32) -> i32;
}

#[test]
fn server_and_client_processes() {
    let home = temp_home("e2e");
    let (control, local, remote) = (free_port(), free_port(), free_port());

    let echo = TcpListener::bind(("127.0.0.1", local)).unwrap();
    std::thread::spawn(move || {
        for mut conn in echo.incoming().flatten() {
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                while let Ok(n) = conn.read(&mut buffer) {
                    if n == 0 || conn.write_all(&buffer[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });

    let control_arg = control.to_string();
    let mut server = Process(
        command(
            &home,
            &[
                "server",
                "-p",
                &control_arg,
                "--foreground",
                "--bind",
                "127.0.0.1",
            ],
        )
        .stdout(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_for_port(control);
    let key = std::fs::read_to_string(home.join(".secret/tunnelr-key")).unwrap();
    assert_eq!(key.trim().len(), 32);

    let target = format!("127.0.0.1:{control}");
    let ports = format!("{remote}:{local}");
    let (code, _) = run(&home, &["add", "test", &target, "-p", &ports]);
    assert_eq!(code, 0);
    let mut client = Process(command(&home, &[]).stdout(Stdio::null()).spawn().unwrap());
    wait_for_port(remote);

    let mut conn = TcpStream::connect(("127.0.0.1", remote)).unwrap();
    conn.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    conn.write_all(b"hello from a visitor").unwrap();
    let mut reply = [0u8; 20];
    conn.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"hello from a visitor");

    let token = run(&home, &["token"]).1;
    let mut api = TcpStream::connect(("127.0.0.1", control)).unwrap();
    write!(
        api,
        "GET /ports HTTP/1.1\r\nauthorization: Bearer {}\r\n\r\n",
        token.trim()
    )
    .unwrap();
    let mut response = String::new();
    api.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(
        response.contains(&format!("\"port\": {remote},")),
        "{response}"
    );

    assert_eq!(terminate(&mut client), Some(0));
    assert_eq!(terminate(&mut server), Some(0));
}
