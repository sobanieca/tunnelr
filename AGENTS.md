# AGENTS.md

This file provides guidance to AI coding agents (Claude Code and others) when
working with code in this repository.

## Overview

`tunnelr` is a CLI tool that exposes ports of a local machine through a remote
VPS. One binary works as the server (on the VPS) and as the client (on the local
machine). It is built with Deno and plain JavaScript with JSDoc type annotations
(no TypeScript files except `main.d.ts`).

## Development Commands

```bash
deno task run [args]      # run main.js with all permissions
deno task check           # fmt check, lint, type check, tests (run before commit)
deno fmt                  # format
deno lint                 # lint
deno check main.js        # type check entry point
cd test && deno task test # run tests only
cd test && deno test -A test.js -- --verbose   # tests with tunnelr logs
```

Try it locally in two terminals:

```bash
deno task run server -p 8500 --foreground # server, prints the address and key
deno task run 127.0.0.1:8500 -p 3000 -a <key>   # client (or -a <key file>)
```

## Architecture

### Entry point and commands

`main.js` parses args (`src/args.js`) and runs the first command whose
`match(args)` returns true. Commands live in `src/commands/`:

- `help.js` - help text, also documents the HTTP API
- `version.js`, `update.js` - like in jsonr
- `server.js` - `tunnelr server [-p <port>]`. As root on Linux with systemd it
  calls `ensureService` (install, update or just check the unit), then prints
  the banner with IP, key and example command and exits. Inside the service
  (`TUNNELR_SERVICE=1` env) or with `--foreground` it runs the server in the
  foreground. `--uninstall` removes the service. Default control port is 8500.
- `client.js` - `tunnelr <host[:port]> -p <ports>`,
  `tunnelr [<name>] [-p <ports>]` opens a saved connection (default: the last
  used one)
- `add.js`, `rm.js`, `ls.js` - manage saved connections (`src/connections.js`,
  stored in `~/.config/tunnelr/connections.json`)
- `token.js` - `tunnelr token` prints a one-time token for the HTTP API

### Core modules

- `src/server.js` - `startServer({ port, key, bind })`. Runs `Deno.serve` on the
  control port. Handles the HTTP API (`GET /ports`, `DELETE /ports/:port`,
  `GET /`) and the WebSocket upgrade on `/tunnel`. Keeps a map of open tunnels
  (port -> listener, client, stats). When a client asks for a port that is
  already open, the old tunnel is closed and the new client takes over.
- `src/client.js` - `startClient({ target, key, ports, ... })`. Connects to
  `ws://host:port/tunnel`, sends `hello`, opens a local TCP connection for every
  `connect` message. Reconnects with exponential backoff. Stops for good on auth
  or port errors (close codes 4001/4002) or when the server closed all its
  ports.
- `src/mux.js` - `Mux` class used by both sides. Multiplexes many TCP
  connections over one WebSocket. Keeps write order per stream, applies simple
  backpressure via `bufferedAmount`, and closes streams when either side ends.
- `src/protocol.js` - wire format description, frame encode/decode and port
  parsing (`8500`, `8500,8600`, `8500:3000`).
- `src/auth.js` - key handling and one-time tokens. `-a` is the key itself or a
  path to a key file, default `~/.secret/tunnelr-key`. The server creates and
  saves a random key when the file is missing. The key never goes over the wire:
  `createToken(key)` makes `"<ts>.<nonce>.<hmac-sha256>"`,
  `createTokenVerifier(key)` checks the signature, a 60 s TTL and rejects reused
  nonces. Used by the WebSocket `hello` and the HTTP API.
- `src/service.js` - systemd unit file content (points to the key file with
  `-a <path>`, the key itself is not in the unit), `ensureService` (compares the
  wanted unit with the installed one, writes and restarts only on change),
  `uninstallService`.
- `src/net.js` - dual-stack listen helper, local/public IP detection.
- `src/logger.js` - small logger with timestamps, `--debug` enables debug
  output, `setQuiet` is used by tests.

### Protocol

Text WebSocket frames are JSON control messages (`hello` with a one-time
`token`, `ready`, `error`, `connect`, `close`, `closed`, `ping`, `pong`). Binary
frames carry stream data: 4 bytes stream id (big endian) followed by payload.
Stream ids are assigned by the server. See the comment at the top of
`src/protocol.js`.

### Tests

`test/test.js` starts real servers and clients in process on free ports and
checks data flow (echo, large payload, HTTP), the API, key and token checks
(wrong key, replay, expiry), takeover and reconnect. No snapshots.

## Versioning and release

Bump the version in `deno.json` (`src/version.js` reads it from there) and add a
`CHANGELOG.md` entry. On push to `main` the workflow runs `deno task check`,
publishes to JSR as `@sobanieca/tunnelr` and creates a GitHub release with
binaries for Linux and macOS (x64, arm64). `install.sh` downloads the latest
release binary.
