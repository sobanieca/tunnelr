# 1.0.0

- First release
- Server mode (`tunnelr -p 2500`) with HTTP API to list and close ports
- Client mode (`tunnelr <vps> -p 8500`) that opens ports on the server
  dynamically, supports many ports and `remote:local` mapping, and reconnects by
  itself
- `--auth-file <path>` to read the token from any file
- `tunnelr -p 2500` on a VPS installs and starts a systemd service, so the
  server survives reboots; `--foreground` skips systemd
