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
