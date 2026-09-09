import { version } from "../version.js";

export const helpText = `
tunnelr ${version} - expose ports of your local machine through a cheap VPS.

How it works:

  1. On the VPS run the tunnelr server. It listens on one control port and
     prints its address and a secret key.
  2. On your machine run the tunnelr client with that address and key. It asks
     the server to open the ports you want.
  3. Anyone who connects to <vps>:<port> reaches your machine. Ports open when
     the client connects and close when it disconnects.

Usage:

  tunnelr -p <control port> [options]              start the server (on the VPS)
  tunnelr <vps host[:control port]> -p <ports>     start the client (on your machine)
  tunnelr service install|uninstall                manage the systemd service
  tunnelr token [-a <key or file>]                 print a one-time token for the HTTP API
  tunnelr help | version | update

Server (on the VPS):

  sudo tunnelr -p 8500

  As root on Linux with systemd this installs (or updates) the "tunnelr"
  service, starts it and prints the address and the key. The service starts
  again after every reboot. Run the same command again anytime to see the
  address and key. Without root or systemd the server runs in the foreground.

  -p, --port <port>      Control port (default 8500). The client connects to it.
  -a, --auth <key|file>  Key, or path to a file with the key. Default file:
                         ~/.secret/tunnelr-key. A new key is created and saved
                         there when the file is missing.
  --foreground           Do not touch systemd, run in the foreground.
  --bind <address>       Listen only on this address (default: all interfaces).

Client (on your machine):

  tunnelr my-vps.example.com -p 3000 -a ~/.secret/tunnelr-key
  tunnelr my-vps.example.com -p 3000,4000,8000
  tunnelr my-vps.example.com:2500 -p 3000
  tunnelr my-vps.example.com -p 8000:3000

  <host[:port]>          VPS host name or IP, control port defaults to 8500.
                         Use wss://host:port when a TLS proxy is in front.
  -p, --port <spec>      Port to open on the VPS. Separate many with commas or
                         repeat the flag. "remote:local" forwards VPS port
                         "remote" to local port "local" (default: the same).
  -a, --auth <key|file>  Key, or path to a file with the key.
                         Default: ~/.secret/tunnelr-key.
  --local-host <host>    Where local ports live (default: 127.0.0.1).

  The client reconnects by itself when the connection drops.

Auth:

  The key never travels over the network. Every request carries a one-time
  token signed with the key that expires after a minute, so a captured token
  cannot be reused. Both machines need a correct clock.

Service (on the VPS, needs root and systemd):

  sudo tunnelr service install [-p 8500]   same as "sudo tunnelr -p 8500"
  sudo tunnelr service uninstall           stop, disable and remove the service

  Check it with:  systemctl status tunnelr  |  journalctl -u tunnelr -f

HTTP API (on the control port):

  GET /ports          list open ports and their clients
  DELETE /ports/<n>   close port n, the client that opened it is told about it
  GET /               server name and version, no token needed

  Send a token in "Authorization: Bearer <token>" or "?token=<token>":

    curl -H "Authorization: Bearer $(tunnelr token)" http://my-vps.example.com:8500/ports
    curl -X DELETE -H "Authorization: Bearer $(tunnelr token)" http://my-vps.example.com:8500/ports/3000

  Ports are opened by the client, so there is no POST endpoint.

Other:

  --debug     Print debug logs
  -h, --help  Show this help
  -v, --version

Note: traffic between the client and the server is not encrypted by tunnelr.
Use HTTPS or SSH inside the tunnel, or put a TLS proxy in front of the server.
`;

export default {
  execute: () => console.log(helpText.trimEnd()),
  match: (args) => args.help || args._[0] === "help",
};
