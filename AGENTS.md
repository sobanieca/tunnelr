# AGENTS.md

This file provides guidance to AI coding agents (Claude Code and others) when
working with code in this repository.

## Overview

`tunnelr` is a CLI tool that exposes ports of a local machine through a remote
VPS. One binary works as the server (on the VPS) and as the client (on the local
machine). It's written in Rust and distributed as a single binary. Keep the
dependency list minimal: `ureq` (HTTP for updates and the public IP lookup),
`rustls` + `webpki-roots` (`wss://` connections), `ring` (HMAC, SHA-1, random)
and `libc`. All of them except `libc` are already used by `ureq`. JSON,
WebSocket, the HTTP API, argument parsing and colors are implemented in the
crate itself.

## Development Commands

Development tasks are cargo aliases defined in `.cargo/config.toml` (backed by
the `xtask` crate):

```bash
cargo tunnelr [args]      # Run tunnelr from source
cargo verify              # fmt --check, clippy -D warnings and all tests
cargo test                # Run all tests
TUNNELR_TEST_VERBOSE=1 cargo test   # tests with tunnelr logs
```

`cargo verify` is the pre-commit check and is what CI runs.

Try it locally in two terminals:

```bash
cargo tunnelr server -p 4000 --foreground        # server, prints the address and key
cargo tunnelr 127.0.0.1:4000 -p 3000 -a <key>    # client (or -a <key file>)
```

## Architecture

### Entry point and commands

`src/main.rs` parses args (`src/args.rs`) and runs the first matching command:
help, version, update, token, add, rm, ls, server, otherwise the client (or the
help when there is nothing to open). Errors are logged and tunnelr exits with
code 1. Commands with more code live in `src/commands/`:

- `server.rs` - `tunnelr server [-p <port>]`. As root on Linux with systemd it
  calls `ensure_service` (install, update or just check the unit), then prints
  the banner with IP, key and example command and exits. Inside the service
  (`TUNNELR_SERVICE=1` env) or with `--foreground` it runs the server in the
  foreground. `--uninstall` removes the service. Default control port is 4000.
- `client.rs` - `tunnelr <host[:port]> -p <ports>`,
  `tunnelr [<name>] [-p <ports>]` opens a saved connection (default: the last
  used one)
- `add.rs`, `rm.rs`, `ls.rs` - manage saved connections (`src/connections.rs`,
  stored in `~/.config/tunnelr/connections.json`)

`tunnelr token` prints a one-time token for the HTTP API, `tunnelr update`
(`src/update.rs`) replaces the binary with the latest GitHub release, and
tunnelr checks for a new version once a day (`TUNNELR_NO_UPDATE_CHECK=1`
disables it, it never runs inside the service).

### Core modules

Everything uses blocking sockets and threads, there is no async runtime.

- `src/server.rs` - `start_server(ServerOptions { port, key, bind })`. Accepts
  connections on the control port, handles the HTTP API (`GET /ports`,
  `DELETE /ports/:port`, `GET /`, see `src/http.rs`) and the WebSocket upgrade
  on `/tunnel`. Keeps a map of open tunnels (port -> listener thread, client,
  stats). When a client asks for a port that is already open, the old tunnel is
  closed and the new client takes over.
- `src/client.rs` - `start_client(ClientOptions { target, key, ports, ... })`.
  Connects to `ws://host:port/tunnel` (or `wss://`), sends `hello`, opens a
  local TCP connection for every `connect` message. Reconnects with exponential
  backoff. Stops for good on auth or port errors (close codes 4001/4002) or
  when the server closed all its ports.
- `src/mux.rs` - `Mux`, used by both sides. Multiplexes many TCP connections
  over one WebSocket: one writer thread for the socket, a reader and a writer
  thread per stream, backpressure when more than 4 MiB are queued, and closes
  streams when either side ends.
- `src/websocket.rs` - RFC 6455 framing, the client handshake and the TLS
  split for `wss://` (read and write halves sharing one rustls connection).
- `src/protocol.rs` - wire format description, frame encode/decode and port
  parsing (`8500`, `8500,8600`, `8500:3000`).
- `src/auth.rs` - key handling and one-time tokens. `-a` is the key itself or a
  path to a key file, default `~/.secret/tunnelr-key`. The server creates and
  saves a random key when the file is missing. The key never goes over the wire:
  `create_token(key)` makes `"<ts>.<nonce>.<hmac-sha256>"`, `TokenVerifier`
  checks the signature, a 60 s TTL and rejects reused nonces. Used by the
  WebSocket `hello` and the HTTP API.
- `src/service.rs` - systemd unit file content (runs `tunnelr server` with the
  key file `-a <path>`, the key itself is not in the unit), `ensure_service`
  (compares the wanted unit with the installed one, writes and restarts only on
  change), `uninstall_service`.
- `src/net.rs` - dual-stack listen, an accept loop that can be stopped,
  local/public IP detection.
- `src/signals.rs` - graceful stop on SIGINT/SIGTERM for the client and the
  server.
- `src/logger.rs` - small logger with timestamps, `--debug` enables debug
  output, `NO_COLOR=1` disables colors.
- `src/json.rs`, `src/time.rs`, `src/colors.rs` - JSON, clock and ANSI helpers.
- `src/help.txt` - the help text (also published at
  https://sobanieca.github.io/tunnelr/help.txt).

### Protocol

Text WebSocket frames are JSON control messages (`hello` with a one-time
`token`, `ready`, `error`, `connect`, `close`, `closed`, `ping`, `pong`). Binary
frames carry stream data: 4 bytes stream id (big endian) followed by payload.
Stream ids are assigned by the server. See the comment at the top of
`src/protocol.rs`. The protocol is the same as in the Deno versions (0.1, 0.2),
so old and new clients and servers work together.

### Tests

- Unit tests live next to the code (`#[cfg(test)]`).
- `src/tests.rs` starts real servers and clients in process on free ports and
  checks data flow (echo, large payload, HTTP, parallel connections), the API,
  key and token checks (wrong key, replay), takeover and reconnect.
- `tests/cli.rs` runs the binary in an isolated home directory: help, argument
  errors, saved connections, tokens and a server + client end-to-end run. No
  snapshots.

## Landing page

The landing page lives in `docs/index.html`. Every time you introduce a new
feature or change existing behavior, update the landing page as well, so it
always reflects the current feature set.

## Versioning and release

Bump the version in `Cargo.toml` and add a `CHANGELOG.md` entry. Every push to
`main` runs `cargo verify`, builds binaries for Linux (musl, static) and macOS
(x64/arm64) and publishes them as GitHub release `v<version>`. `install.sh` and
`tunnelr update` download these release assets, so their names
(`tunnelr-linux-x64`, ...) must not change.
