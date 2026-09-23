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

  Every port tunnelr uses (the control port and each exposed port) has to be
  one the VPS really has open. Cheap providers forward only a few, mikr.us
  gives two, like 20185 and 30185.

Usage:

  tunnelr server [-p <control port>] [options]     start the server (on the VPS)
  tunnelr <vps host[:control port]> -p <ports>     start the client (on your machine)
  tunnelr add <name> <vps host[:port]> -p <ports>  save a connection
  tunnelr [<name>] [-p <ports>]                    open a saved connection (default: last used)
  tunnelr ls | rm <name>                           list or remove saved connections
  tunnelr server --uninstall                       remove the systemd service
  tunnelr token [-a <key or file>]                 print a one-time token for the HTTP API
  tunnelr help | version | update

Server (on the VPS):

  sudo tunnelr server -p 20185

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
  --uninstall            Stop, disable and remove the systemd service.

  Check the service with:  systemctl status tunnelr  |  journalctl -u tunnelr -f

Client (on your machine):

  tunnelr my-vps.example.com:20185 -p 30185:3000 -a ~/.secret/tunnelr-key
  tunnelr my-vps.example.com -p 3000 -a ~/.secret/tunnelr-key
  tunnelr my-vps.example.com -p 3000,4000,8000
  tunnelr my-vps.example.com:2500 -p 3000

  <host[:port]>          VPS host name or IP, control port defaults to 8500.
                         Use wss://host:port when a TLS proxy is in front.
  -p, --port <spec>      Port to open on the VPS, it must be open there.
                         Separate many with commas or repeat the flag.
                         "remote:local" forwards VPS port "remote" to local
                         port "local" (default: the same).
  -a, --auth <key|file>  Key, or path to a file with the key.
                         Default: ~/.secret/tunnelr-key.
  --to <host>            Host to forward to (default: 127.0.0.1). Use it when
                         the service runs elsewhere in your LAN or in a
                         container without a published port.

  The client reconnects by itself when the connection drops.

Saved connections (on your machine):

  tunnelr add vps 203.0.113.10:20185 -p 30185:3000 -a ~/.secret/tunnelr-key
  tunnelr                  open the last used connection with its saved ports
  tunnelr -p 4000          same, but forward VPS port 30185 to local port 4000
  tunnelr vps -p 30186:80  open "vps" with other ports
  tunnelr ls               list saved connections, "*" marks the last used one
  tunnelr rm vps           remove one

  "add" takes the same flags as the client (-p, -a, --to) and replaces
  a connection with the same name. Flags given when opening a connection win
  over the saved ones. A bare port in -p keeps the saved VPS port at the same
  position and changes only the local port, "remote:local" is taken as is.
  Connections are kept in ~/.config/tunnelr/connections.json (the key file
  path is saved there, not the key).

Auth:

  The key never travels over the network. Every request carries a one-time
  token signed with the key that expires after a minute, so a captured token
  cannot be reused. Both machines need a correct clock.

HTTP API (on the control port):

  GET /ports          list open ports and their clients
  DELETE /ports/<n>   close port n, the client that opened it is told about it
  GET /               server name and version, no token needed

  Send a token in "Authorization: Bearer <token>" or "?token=<token>":

    curl -H "Authorization: Bearer $(tunnelr token)" http://my-vps.example.com:20185/ports
    curl -X DELETE -H "Authorization: Bearer $(tunnelr token)" http://my-vps.example.com:20185/ports/3000

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
