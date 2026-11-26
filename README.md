# tunnelr

Website: https://adam.sobaniec.com/tunnelr/

Expose ports of your local machine to the internet through a cheap VPS.

```
internet  -->  my-vps.example.com:30185  ==tunnel==>  your machine:3000
                 public port                          app port
```

## Ports

tunnelr uses three kinds of ports. Every message, flag and help text uses
these names:

| Port | Where | Who connects to it | Example | Set with |
|---|---|---|---|---|
| **Tunnel port** | VPS, one per server | the tunnelr client (and the HTTP API) | `20185` | `tunnelr server -p 20185`, client: `<vps>:20185` |
| **Public port** | VPS, one per forwarded app | visitors from the internet | `30185` | client: `-p 30185:3000` (left side) |
| **App port** | your machine (or the `--to` host) | tunnelr, to reach your app | `3000` | client: `-p 30185:3000` (right side) |

```
tunnelr 203.0.113.10:20185 -p 30185:3000
                     │        │     └── app port: where your app listens
                     │        └──────── public port: what visitors open
                     └───────────────── tunnel port: where the server listens
```

The tunnel port and every public port must be open on the VPS. With two open
ports (as on mikr.us) you get one tunnel port and one public port.

## Server

1. Buy a cheap VPS, for example [mikr.us](https://mikr.us) (less than 10 USD per
   year!).

   > Note: Whatever server you decide to buy it is recommended to use Debian OS.

2. Log in to the VPS as the provider describes, then:

   a. Install `tunnelr` to `/usr/local/bin`, as root (the service runs as root,
   and `sudo` does not search the folders of your user):

   ```bash
   curl -fsSL adam.sobaniec.com/tunnelr/install.sh | sudo bash
   ```

   b. Check in the provider's panel which ports the VPS has open (cheap
   providers forward only a few, mikr.us gives two, like `20185` and `30185`).
   Pick one as the **tunnel port** and start tunnelr on it, keep the other for
   the public port:

   ```bash
   sudo tunnelr server -p 20185
   ```

   It prints the address and the key:

   ```
   tunnelr server 0.1.0 - systemd service "tunnelr" installed and started

     Address:       203.0.113.10:20185
     Tunnel port:   20185  (tunnelr clients connect here)
     Public ports:  any other port open on this VPS, the client picks them with -p
     Key:           kD3xW9q1mZ8pR4tY7uH2cV6bN0aS5fGj
     Key file:      /root/.tunnelr/key

   On your machine, save the key in ~/.tunnelr/key and run:

     tunnelr 203.0.113.10:20185 -p <public port>:<app port>

   Run this command again anytime to see the address and the key.
   ```

Congratulations, you have your own tunnel server! It survives reboots.

## Your machine

3. Install `tunnelr`:

   ```bash
   curl -fsSL adam.sobaniec.com/tunnelr/install.sh | bash
   ```

4. Save the key printed by the server in a file, for example
   `~/.tunnelr/key`.
5. Open your app to the world: give the server address (with the tunnel port),
   then `-p <public port>:<app port>`, where the public port is the other open
   port of the VPS and the app port is where your app listens:

   ```bash
   tunnelr 203.0.113.10:20185 -p 30185:3000 -a ~/.tunnelr/key
   ```

   It prints:

   ```
   Tunnel is open (tunnel port 20185 on 203.0.113.10):
     203.0.113.10:30185 -> 127.0.0.1:3000   (public port 30185 -> app port 3000)
   ```

Now `203.0.113.10:30185` reaches port 3000 on your machine. Keep the command
running, `Ctrl+C` closes the tunnel.

## Good to know

- `-a` takes the key itself or a path to a file with it. Without `-a` tunnelr
  reads `~/.tunnelr/key`.
- The key never travels over the network. Every request carries a one-time token
  signed with the key, so a captured token cannot be reused.
- `-p 30185:3000` forwards public port 30185 to app port 3000. When the VPS has
  all ports open, `-p 3000,5000` uses the same number as public and app port.
  `tunnelr --help` shows all options and the HTTP API.
- On the server `-p` (or `--tunnel-port`) is the tunnel port. On the client the
  tunnel port goes in the address (`<vps>:20185`, default 4000) and `-p` is
  for public and app ports.
- To skip typing the address and the key every time, save the connection once,
  then `tunnelr` alone opens it again:

  ```bash
  tunnelr add vps 203.0.113.10:20185 -p 30185:3000 -a ~/.tunnelr/key
  tunnelr            # opens the last used connection, here 30185:3000
  tunnelr -p 5000    # same public port, app port 5000 instead
  tunnelr ls         # list saved connections, "*" marks the last used one
  tunnelr rm vps     # remove one
  ```

  Connections are kept in `~/.tunnelr/connections.json` with the path to
  the key file, not the key.
- tunnelr does not encrypt the traffic. Use HTTPS or SSH inside the tunnel when
  the data is sensitive.

## Installation

`tunnelr` is a single binary for Linux and macOS (x64 and arm64). No runtime is
needed.

```bash
curl -fsSL adam.sobaniec.com/tunnelr/install.sh | bash
```

The script installs `tunnelr` to `~/.tunnelr/bin`, with no `sudo` needed. If
that folder is not on your `PATH`, the script asks to add it in your shell's
startup file (`~/.bashrc`, `~/.zshrc`, fish's `conf.d` or `~/.profile`). Set
`TUNNELR_NO_MODIFY_PATH=1` to leave the file unchanged.

On the VPS, install as root, so that `sudo tunnelr server` finds the binary and
the root service runs a binary that only root can change. Run as root, the
script installs to `/usr/local/bin`:

```bash
curl -fsSL adam.sobaniec.com/tunnelr/install.sh | sudo bash
```

Or install to another folder: set `INSTALL_DIR`:

```bash
curl -fsSL adam.sobaniec.com/tunnelr/install.sh | INSTALL_DIR=~/.local/bin bash
```

Or download a binary (`tunnelr-linux-x64`, `tunnelr-linux-arm64`,
`tunnelr-macos-x64`, `tunnelr-macos-arm64`) from the
[releases page](https://github.com/sobanieca/tunnelr/releases/latest).

## Updating

```bash
tunnelr update
```

This downloads the latest release and replaces the binary. If `tunnelr` is
installed in a system directory (like `/usr/local/bin`) run
`sudo tunnelr update`. On the VPS restart the service afterwards:
`sudo systemctl restart tunnelr`. `tunnelr` checks for new versions once a day
and prints a notice when one is available (set `TUNNELR_NO_UPDATE_CHECK=1` to
disable it).

### Migrating from the Deno version (0.1, 0.2)

Versions up to 0.2 were distributed for Deno (`jsr:@sobanieca/tunnelr`). Remove
the Deno installation with `deno uninstall -g tunnelr` and install the binary as
described above. On the VPS run `sudo tunnelr server -p <port>` again, it
updates the systemd service to the new binary. Keys, key files and saved
connections work unchanged, and old clients and servers can talk to new ones.

The full help text is available with `tunnelr --help` and at
https://adam.sobaniec.com/tunnelr/help.txt (handy to give to AI assistants).
