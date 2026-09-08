# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

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
deno task run -p 2500                     # server, prints the token
deno task run 127.0.0.1:2500 -p 8500 -a <token>   # client
```

## Architecture

### Entry point and commands

`main.js` parses args (`src/args.js`) and runs the first command whose
`match(args)` returns true. Commands live in `src/commands/`:

- `help.js` - help text, also documents the HTTP API
- `version.js`, `update.js` - like in jsonr
- `service.js` - `tunnelr service install|uninstall` (install is the same as
  running the server)
- `server.js` - `tunnelr -p <port>` (no host argument). As root on Linux with
  systemd it calls `ensureService` (install, update or just check the unit),
  then prints the banner with IP, token and example commands and exits. Inside
  the service (`TUNNELR_SERVICE=1` env) or with `--foreground` it runs the
  server in the foreground.
- `client.js` - `tunnelr <host[:port]> -p <ports>`

### Core modules

- `src/server.js` - `startServer({ port, auth, bind })`. Runs `Deno.serve` on
  the control port. Handles the HTTP API (`GET /ports`, `DELETE /ports/:port`,
  `GET /`) and the WebSocket upgrade on `/tunnel`. Keeps a map of open tunnels
  (port -> listener, client, stats). When a client asks for a port that is
  already open, the old tunnel is closed and the new client takes over.
- `src/client.js` - `startClient({ target, auth, ports, ... })`. Connects to
  `ws://host:port/tunnel`, sends `hello`, opens a local TCP connection for every
  `connect` message. Reconnects with exponential backoff. Stops for good on auth
  or port errors (close codes 4001/4002) or when the server closed all its
  ports.
- `src/mux.js` - `Mux` class used by both sides. Multiplexes many TCP
  connections over one WebSocket. Keeps write order per stream, applies simple
  backpressure via `bufferedAmount`, and closes streams when either side ends.
- `src/protocol.js` - wire format description, frame encode/decode and port
  parsing (`8500`, `8500,8600`, `8500:3000`).
- `src/auth.js` - token lookup order: `-a` flag, `--auth-file` path,
  `TUNNELR_AUTH` env, `~/.tunnelr/auth` file. The server creates and saves a
  random token when none exists (to `--auth-file` when given).
- `src/service.js` - systemd unit file content, `ensureService` (compares the
  wanted unit with the installed one, writes and restarts only on change),
  `uninstallService`.
- `src/net.js` - dual-stack listen helper, local/public IP detection.
- `src/logger.js` - small logger with timestamps, `--debug` enables debug
  output, `setQuiet` is used by tests.

### Protocol

Text WebSocket frames are JSON control messages (`hello`, `ready`, `error`,
`connect`, `close`, `closed`, `ping`, `pong`). Binary frames carry stream data:
4 bytes stream id (big endian) followed by payload. Stream ids are assigned by
the server. See the comment at the top of `src/protocol.js`.

### Tests

`test/test.js` starts real servers and clients in process on free ports and
checks data flow (echo, large payload, HTTP), the API, auth, takeover and
reconnect. No snapshots.

## Versioning and release

Bump the version in both `deno.json` and `src/version.js` (CI fails when they
differ) and add a `CHANGELOG.md` entry. On push to `main` the workflow runs
`deno task check`, publishes to JSR as `@sobanieca/tunnelr` and creates a GitHub
release with binaries for Linux and macOS (x64, arm64). `install.sh` downloads
the latest release binary.
