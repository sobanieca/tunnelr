import { Mux } from "./mux.js";
import { parsePort } from "./protocol.js";
import { tokensMatch } from "./auth.js";
import { cleanAddress, listenTcp } from "./net.js";
import { version } from "./version.js";
import logger from "./logger.js";

const HELLO_TIMEOUT_MS = 10_000;
const PING_INTERVAL_MS = 15_000;
const IDLE_TIMEOUT_S = 30;
const CLIENT_CLOSE_DELAY_MS = 1000;

const json = (body, status = 200) =>
  new Response(JSON.stringify(body, null, 2) + "\n", {
    status,
    headers: { "content-type": "application/json" },
  });

/**
 * Accepts the token from "Authorization: Bearer <token>", "Authorization: <token>",
 * "?auth=<token>" or a JSON body { "auth": "<token>" }.
 * @param {Request} req
 */
const readToken = async (req) => {
  const header = req.headers.get("authorization");
  if (header) return header.replace(/^Bearer\s+/i, "").trim();
  const query = new URL(req.url).searchParams.get("auth");
  if (query) return query;
  if (req.method !== "GET" && req.method !== "HEAD") {
    try {
      const body = await req.json();
      if (body && typeof body.auth === "string") return body.auth;
    } catch {
      // no json body
    }
  }
  return "";
};

/**
 * @param {{ port: number, auth: string, bind?: string }} options
 */
export const startServer = ({ port, auth, bind }) => {
  /** @type {Map<number, any>} port -> tunnel */
  const tunnels = new Map();
  /** @type {Set<any>} */
  const clients = new Set();
  let clientSeq = 0;
  let streamSeq = 0;

  const listTunnels = () =>
    [...tunnels.values()].map((t) => ({
      port: t.port,
      client: t.client.remoteAddr,
      since: t.since.toISOString(),
      activeConnections: t.stats.active,
      totalConnections: t.stats.connections,
      bytesIn: t.stats.bytesRead,
      bytesOut: t.stats.bytesWritten,
    }));

  const stopListener = async (tunnel) => {
    try {
      tunnel.listener.close();
    } catch {
      // already closed
    }
    // the port is free again only when the accept loop has ended
    await tunnel.accepting;
  };

  const closeTunnel = async (tunnelPort, reason) => {
    const tunnel = tunnels.get(tunnelPort);
    if (!tunnel) return false;
    tunnels.delete(tunnelPort);
    const client = tunnel.client;
    client.ports.delete(tunnelPort);
    client.mux.closeWhere((stream) => stream.stats === tunnel.stats);
    client.mux.sendControl({ type: "closed", port: tunnelPort, reason });
    logger.info(`Port ${tunnelPort} closed (${reason})`);
    if (client.ports.size === 0 && client.ready) {
      // give the client a moment to read the message and close by itself
      setTimeout(() => {
        if (client.ports.size === 0) client.ws.close(1000, "no ports left");
      }, CLIENT_CLOSE_DELAY_MS);
    }
    await stopListener(tunnel);
    return true;
  };

  const acceptLoop = async (tunnel) => {
    try {
      for await (const conn of tunnel.listener) {
        const id = ++streamSeq;
        tunnel.stats.connections++;
        logger.debug(
          `Port ${tunnel.port}: new connection ${id} from ${
            cleanAddress(/** @type {Deno.NetAddr} */ (conn.remoteAddr).hostname)
          }`,
        );
        tunnel.client.mux.sendControl({
          type: "connect",
          id,
          port: tunnel.port,
        });
        tunnel.client.mux.attach(id, conn, tunnel.stats);
      }
    } catch (err) {
      logger.debug(`Port ${tunnel.port} accept loop ended: ${err.message}`);
    }
  };

  const openPorts = async (client, ports) => {
    const opened = [];
    for (const tunnelPort of ports) {
      if (tunnels.has(tunnelPort)) {
        await closeTunnel(tunnelPort, "taken over by a new client");
      }
      try {
        const listener = listenTcp(tunnelPort, bind);
        const tunnel = {
          port: tunnelPort,
          listener,
          client,
          since: new Date(),
          stats: { connections: 0, active: 0, bytesRead: 0, bytesWritten: 0 },
          accepting: Promise.resolve(),
        };
        tunnels.set(tunnelPort, tunnel);
        client.ports.add(tunnelPort);
        opened.push(tunnel);
      } catch (err) {
        for (const tunnel of opened) {
          await closeTunnel(tunnel.port, "rolled back");
        }
        throw new Error(`Cannot open port ${tunnelPort}: ${err.message}`);
      }
    }
    for (const tunnel of opened) tunnel.accepting = acceptLoop(tunnel);
  };

  const handleHello = async (client, msg) => {
    if (!tokensMatch(String(msg.auth ?? ""), auth)) {
      logger.warn(`Client ${client.remoteAddr} sent a wrong auth token`);
      client.mux.sendControl({ type: "error", message: "wrong auth token" });
      client.ws.close(4001, "wrong auth token");
      return;
    }
    let ports;
    try {
      if (!Array.isArray(msg.ports) || msg.ports.length === 0) {
        throw new Error("no ports requested");
      }
      ports = [...new Set(msg.ports.map(parsePort))];
      if (ports.includes(port)) {
        throw new Error(`port ${port} is used by the tunnelr server itself`);
      }
      await openPorts(client, ports);
    } catch (err) {
      logger.warn(`Client ${client.remoteAddr} rejected: ${err.message}`);
      client.mux.sendControl({ type: "error", message: err.message });
      client.ws.close(4002, err.message.slice(0, 120));
      return;
    }
    client.ready = true;
    client.mux.sendControl({ type: "ready", ports });
    logger.info(
      `Client ${client.remoteAddr} opened port(s) ${ports.join(", ")}`,
    );
  };

  const handleControl = (client, msg) => {
    switch (msg?.type) {
      case "hello":
        if (!client.ready) handleHello(client, msg);
        break;
      case "close":
        client.mux.close(msg.id, false);
        break;
      case "pong":
        break;
      default:
        logger.debug(`Unknown control message: ${msg?.type}`);
    }
  };

  const dropClient = (client) => {
    if (!clients.has(client)) return;
    clients.delete(client);
    clearTimeout(client.helloTimer);
    clearInterval(client.pingTimer);
    for (const tunnelPort of [...client.ports]) {
      const tunnel = tunnels.get(tunnelPort);
      if (tunnel && tunnel.client === client) {
        tunnels.delete(tunnelPort);
        stopListener(tunnel);
        logger.info(`Port ${tunnelPort} closed (client disconnected)`);
      }
    }
    client.ports.clear();
    client.mux.closeAll();
  };

  /** @param {Request} req @param {Deno.ServeHandlerInfo} info */
  const handleTunnel = (req, info) => {
    const remoteAddr = cleanAddress(
      /** @type {Deno.NetAddr} */ (info.remoteAddr).hostname,
    );
    const { socket, response } = Deno.upgradeWebSocket(req, {
      idleTimeout: IDLE_TIMEOUT_S,
    });
    const client = {
      id: ++clientSeq,
      ws: socket,
      /** @type {Mux} */
      mux: null,
      ports: new Set(),
      ready: false,
      remoteAddr,
      helloTimer: null,
      pingTimer: null,
    };
    client.mux = new Mux(socket, (msg) => handleControl(client, msg));
    clients.add(client);
    logger.debug(`Client ${client.remoteAddr} connected`);

    client.helloTimer = setTimeout(() => {
      if (!client.ready) {
        client.mux.sendControl({ type: "error", message: "hello timeout" });
        socket.close(4000, "hello timeout");
      }
    }, HELLO_TIMEOUT_MS);
    client.pingTimer = setInterval(
      () => client.mux.sendControl({ type: "ping" }),
      PING_INTERVAL_MS,
    );
    socket.addEventListener("close", () => {
      logger.debug(`Client ${client.remoteAddr} disconnected`);
      dropClient(client);
    });
    socket.addEventListener("error", (event) => {
      logger.debug(
        `Client ${client.remoteAddr} socket error: ${
          /** @type {ErrorEvent} */ (event).message ?? "unknown"
        }`,
      );
    });
    return response;
  };

  /** @param {Request} req @param {Deno.ServeHandlerInfo} info */
  const handler = async (req, info) => {
    const url = new URL(req.url);
    const path = url.pathname.replace(/\/+$/, "") || "/";

    if (path === "/tunnel" && req.headers.get("upgrade") === "websocket") {
      return handleTunnel(req, info);
    }

    if (path === "/") {
      return json({ name: "tunnelr", version });
    }

    const match = path.match(/^\/ports(?:\/(\d+))?$/);
    if (!match) return json({ error: "not found" }, 404);

    const token = await readToken(req);
    if (!tokensMatch(token, auth)) {
      return json({ error: "unauthorized" }, 401);
    }

    if (req.method === "GET" && !match[1]) {
      return json({ ports: listTunnels() });
    }
    if (req.method === "DELETE" && match[1]) {
      const closed = await closeTunnel(Number(match[1]), "closed by api");
      return closed
        ? json({ closed: Number(match[1]) })
        : json({ error: "port is not open" }, 404);
    }
    if (req.method === "POST") {
      return json({
        error: "ports are opened by the tunnelr client, see tunnelr --help",
      }, 400);
    }
    return json({ error: "method not allowed" }, 405);
  };

  const hostnames = bind ? [bind] : ["::", "0.0.0.0"];
  /** @type {Deno.HttpServer} */
  let httpServer;
  let hostname;
  for (const candidate of hostnames) {
    try {
      httpServer = Deno.serve(
        { port, hostname: candidate, onListen: () => {} },
        handler,
      );
      hostname = candidate;
      break;
    } catch (err) {
      if (err instanceof Deno.errors.AddrInUse) throw err;
      logger.debug(`Cannot listen on ${candidate}: ${err.message}`);
    }
  }
  if (!httpServer) throw new Error(`Cannot listen on port ${port}`);

  const shutdown = async () => {
    for (const client of [...clients]) {
      dropClient(client);
      try {
        client.ws.close(1001, "server shutting down");
      } catch {
        // ignore
      }
    }
    await httpServer.shutdown();
  };

  return { port, hostname, listTunnels, closeTunnel, shutdown };
};
