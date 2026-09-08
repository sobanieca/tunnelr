import { version } from "../version.js";

export const helpText = `
tunnelr ${version} - expose ports of your local machine through a cheap VPS.

How it works:

  1. On the VPS run the tunnelr server. It listens on one control port.
  2. On your machine run the tunnelr client. It connects to the control port
     and asks the server to open the ports you want.
  3. Anyone who connects to <vps>:<port> reaches your local machine.
     Ports are opened when the client connects and closed when it disconnects.

Usage:

  tunnelr -p <control port> [options]                 start the server (on the VPS, as a service)
  tunnelr <vps host[:control port]> -p <port> [...]   start the client (on your machine)
  tunnelr service install|uninstall [options]         run the server as a systemd service
  tunnelr help | version | update

Server (on the VPS):

  tunnelr -p 2500
  tunnelr -p 2500 -a mySecretToken

  When run as root on a Linux box with systemd, this installs (or updates)
  the "tunnelr" systemd service, starts it and prints the connection info.
  The service starts again after every reboot. Run the same command again
  anytime to see the info or to change the port or token. Without root or
  systemd the server runs in the foreground.

  -p, --port <port>   Control port. The client and the HTTP API use it.
                      Default: 2500.
  --foreground        Do not touch systemd, run in the foreground.
  -a, --auth <token>  Auth token. If not set, tunnelr reads --auth-file, then
                      TUNNELR_AUTH env, then ~/.tunnelr/auth. If nothing is
                      found, it creates a random token, saves it and prints it.
  --auth-file <path>  Read the token from this file (created when missing).
  --bind <address>    Listen only on this address (default: all interfaces).

  On start the server prints its IP, the control port and the auth token.

Client (on your machine):

  tunnelr my-vps.example.com -p 8500
  tunnelr my-vps.example.com:2500 -p 8500 -p 8600 -a mySecretToken
  tunnelr my-vps.example.com -p 8500,8600
  tunnelr my-vps.example.com -p 8500:3000
  tunnelr my-vps.example.com -p 8500 --auth-file ~/.secrets/vps-token

  <host[:port]>         VPS host name or IP. Control port defaults to 2500.
                        Use wss://host:port when a TLS proxy is in front.
  -p, --port <spec>     Port to open on the VPS. Repeat the flag or separate
                        with commas. "remote:local" forwards VPS port "remote"
                        to a different local port. Default local port = remote.
  -a, --auth <token>    Auth token. Falls back to --auth-file, TUNNELR_AUTH env,
                        then ~/.tunnelr/auth.
  --auth-file <path>    Read the token from this file.
  --local-host <host>   Where local ports live (default: 127.0.0.1).

  The client reconnects by itself when the connection drops.

Service (on the VPS, needs root and systemd):

  tunnelr service install [-p 2500]        same as "tunnelr -p 2500"
  tunnelr service uninstall                stop, disable and remove the service

  Check it with:  systemctl status tunnelr  |  journalctl -u tunnelr -f

HTTP API (on the control port):

  Every request needs the auth token. Send it in a header, in the query
  string or in a JSON body:

    Authorization: Bearer <token>     or     ?auth=<token>     or     { "auth": "<token>" }

  GET /ports                 List open ports and their clients.
    curl -H "Authorization: Bearer <token>" http://my-vps.example.com:2500/ports

  DELETE /ports/<port>       Close a port. The client that opened it is told about it.
    curl -X DELETE -H "Authorization: Bearer <token>" http://my-vps.example.com:2500/ports/8500

  GET /                      Server name and version, no auth needed.

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
