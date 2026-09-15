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
