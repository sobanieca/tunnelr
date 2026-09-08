# tunnelr

Expose ports of your local machine through a cheap VPS (for example
[mikr.us](https://mikr.us), less than 10 USD per year).

```
internet  -->  my-vps.example.com:8500  ==tunnel==>  your machine:8500
```

- One binary, no config files. Run it on the VPS as the server and on your
  machine as the client.
- Ports open when the client connects and close when it disconnects. You can
  open many ports at once.
- The client reconnects by itself. The server runs as a systemd service, so it
  survives reboots.
- A small HTTP API shows which ports are open.

## Quick start

**1. On the VPS: install and start the server**

```bash
curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash
sudo tunnelr -p 2500
```

This installs a systemd service, starts it on control port 2500 and prints the
auth token. The service starts again after every reboot. Run the same command
again anytime to see the connection info. Without root or systemd the server
runs in the foreground instead.

**2. On your machine: open a tunnel**

```bash
tunnelr my-vps.example.com -p 8500 -a <token>
```

Now `my-vps.example.com:8500` reaches port 8500 on your machine. Keep the
command running. Press `Ctrl+C` to close the tunnel.

## More examples

```bash
# Many ports
tunnelr my-vps.example.com -p 8500 -p 8600
tunnelr my-vps.example.com -p 8500,8600

# VPS port 8500 -> local port 3000
tunnelr my-vps.example.com -p 8500:3000

# Server on a different control port
tunnelr my-vps.example.com:3000 -p 8500

# Keep the token out of the command line
tunnelr my-vps.example.com -p 8500 --auth-file ~/.secrets/vps-token
export TUNNELR_AUTH=<token>        # or save it to ~/.tunnelr/auth
tunnelr my-vps.example.com -p 8500
```

## HTTP API

The server answers on the control port. Send the token in the `Authorization`
header, in `?auth=` or in a JSON body `{ "auth": "..." }`.

```bash
# List open ports
curl -H "Authorization: Bearer <token>" http://my-vps.example.com:2500/ports

# Close a port (the client is told about it)
curl -X DELETE -H "Authorization: Bearer <token>" http://my-vps.example.com:2500/ports/8500
```

There is no endpoint to add ports. The client adds them when it connects.

## Service on the VPS

```bash
sudo tunnelr -p 2500              # install or update, start, show info
sudo tunnelr -p 3000              # change the control port
sudo tunnelr service uninstall    # stop and remove
tunnelr -p 2500 --foreground      # run without systemd
journalctl -u tunnelr -f          # logs
```

## Notes

- Ports must be reachable on the VPS. On mikr.us only some ports are forwarded
  from the public IPv4 address, so use those ports.
- tunnelr does not encrypt the traffic between client and server. Use HTTPS or
  SSH inside the tunnel when the data is sensitive.
- Run `tunnelr --help` for all options.

## Installation

**Binary (Linux, macOS):**

```bash
curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash
```

**Deno:**

```bash
deno install -g --allow-all -f -r -n tunnelr jsr:@sobanieca/tunnelr
```

**Update:** run the install command again, or `tunnelr update --deno`.
