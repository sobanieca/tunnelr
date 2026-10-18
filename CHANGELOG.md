# 0.4.0

- Breaking change: the default control port is 4000 instead of 8500, for
  `tunnelr server` and for a client target without a port. An installed
  service keeps its port in the unit file until `sudo tunnelr server` runs
  again without `-p`. To keep the old port, run `tunnelr server -p 8500` and
  connect to `<host>:8500`

# 0.3.0

- tunnelr is rewritten in Rust and distributed only as a standalone binary
  (static on Linux, about 3 MB). The Deno/JSR package (`jsr:@sobanieca/tunnelr`)
  is no longer published. Install with
  `curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash`. The wire protocol,
  key files and saved connections are unchanged, so 0.2 clients and servers
  work with 0.3 ones
- `tunnelr update` downloads the latest release and replaces the binary
  (`--deno` option removed). tunnelr checks for a new version once a day and
  prints a notice when one is available (`TUNNELR_NO_UPDATE_CHECK=1` disables
  it, the server inside the systemd service never checks)
- Fix: the systemd service started by `sudo tunnelr server` now runs
  `tunnelr server ...` (0.2.0 wrote a unit without the `server` command, so the
  service did not start the server). Run `sudo tunnelr server -p <port>` once
  to update the unit
- Unknown options are reported as errors instead of being ignored
- The help text is published at https://sobanieca.github.io/tunnelr/help.txt

# 0.2.0

- Breaking: the server starts with `tunnelr server -p <port>` (was
  `tunnelr -p <port>`) and `tunnelr server --uninstall` removes the systemd
  service (was `tunnelr service uninstall`, `service install` is gone)
- Breaking: `--local-host` is now `--to <host>`, the host the client forwards to
  (default 127.0.0.1)
- Saved connections: `tunnelr add <name> <vps> -p <ports>` remembers the
  address, ports and key file, `tunnelr ls` and `tunnelr rm <name>` manage them
- `tunnelr` alone opens the last used connection, `tunnelr <name>` a chosen one,
  `-p <port>` changes the local port while keeping the saved VPS port

# 0.1.0

- First prerelease
- Server mode (`sudo tunnelr -p 20185`) installs and starts a systemd service on
  the VPS, so the server survives reboots; `--foreground` skips systemd
- Client mode (`tunnelr <vps> -p 3000,4000`) opens ports on the server
  dynamically, supports `remote:local` mapping and reconnects by itself
- Key is kept in `~/.secret/tunnelr-key` on both machines; `-a` takes the key or
  a path to a key file
- Every request carries a one-time token signed with the key (60 s TTL), so the
  key never travels over the network and a captured token cannot be reused
- HTTP API to list and close ports, `tunnelr token` prints a token for it
