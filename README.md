# tunnelr

Website: https://sobanieca.github.io/tunnelr/

Expose ports of your local machine to the internet through a cheap VPS.

```
internet  -->  my-vps.example.com:30185  ==tunnel==>  your machine:3000
```

## Server

1. Buy a cheap VPS, for example [mikr.us](https://mikr.us) (less than 10 USD per
   year!).

   > Note: Whatever server you decide to buy it is recommended to use Debian OS.

2. Log in to the VPS as the provider describes, then:

   a. Install `tunnelr`:

   ```bash
   curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash
   ```

   b. Check in the provider's panel which ports the VPS has open (cheap
   providers forward only a few, mikr.us gives two, like `20185` and `30185`)
   and start tunnelr on one of them:

   ```bash
   sudo tunnelr server -p 20185
   ```

   It prints the address and the key:

   ```
   tunnelr server 0.1.0 - systemd service "tunnelr" installed and started

     Address:   203.0.113.10:20185
     Key:       kD3xW9q1mZ8pR4tY7uH2cV6bN0aS5fGj
     Key file:  /root/.secret/tunnelr-key

   Run this command again anytime to see the address and the key.
   ```

Congratulations, you have your own tunnel server! It survives reboots.

## Your machine

3. Install `tunnelr`:

   ```bash
   curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash
   ```

4. Save the key printed by the server in a file, for example
   `~/.secret/tunnelr-key`.
5. Open a port of your machine to the world, using another open port of the VPS
   (use the address printed by the server):

   ```bash
   tunnelr 203.0.113.10:20185 -p 30185:3000 -a ~/.secret/tunnelr-key
   ```

Now `203.0.113.10:30185` reaches port 3000 on your machine. Keep the command
running, `Ctrl+C` closes the tunnel.

## Good to know

- `-a` takes the key itself or a path to a file with it. Without `-a` tunnelr
  reads `~/.secret/tunnelr-key`.
- The key never travels over the network. Every request carries a one-time token
  signed with the key, so a captured token cannot be reused.
- `-p 30185:3000` forwards VPS port 30185 to local port 3000. When the VPS has
  all ports open, `-p 3000,5000` opens the same ports on both sides.
  `tunnelr --help` shows all options and the HTTP API.
- Every port tunnelr uses (the control port and each exposed port) has to be one
  the VPS really has open, so two open ports mean one tunnel.
- To skip typing the address and the key every time, save the connection once,
  then `tunnelr` alone opens it again:

  ```bash
  tunnelr add vps 203.0.113.10:20185 -p 30185:3000 -a ~/.secret/tunnelr-key
  tunnelr            # opens the last used connection, here 30185:3000
  tunnelr -p 5000    # same VPS port, local port 5000 instead
  tunnelr ls         # list saved connections, "*" marks the last used one
  tunnelr rm vps     # remove one
  ```

  Connections are kept in `~/.config/tunnelr/connections.json` with the path to
  the key file, not the key.
- tunnelr does not encrypt the traffic. Use HTTPS or SSH inside the tunnel when
  the data is sensitive.

## Installation

`tunnelr` is a single binary for Linux and macOS (x64 and arm64). No runtime is
needed.

```bash
curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash
```

The script installs `tunnelr` to `/usr/local/bin`, for all users. If you can't
write to `/usr/local/bin`, the script stops with an error. Run it with `sudo`:

```bash
curl -fsSL sobanieca.github.io/tunnelr/install.sh | sudo bash
```

Or install to a custom location: set `INSTALL_DIR` to a directory that is on
your `PATH`:

```bash
curl -fsSL sobanieca.github.io/tunnelr/install.sh | INSTALL_DIR=~/.local/bin bash
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
https://sobanieca.github.io/tunnelr/help.txt (handy to give to AI assistants).
