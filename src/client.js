import { Mux } from "./mux.js";
import { createToken } from "./auth.js";
import { version } from "./version.js";
import logger from "./logger.js";

const DEFAULT_CONTROL_PORT = 8500;
const WATCHDOG_MS = 45_000;
const MIN_RETRY_MS = 1000;
const MAX_RETRY_MS = 30_000;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Turn "host", "host:2500", "http://host:2500" or "wss://host" into a ws url.
 * @param {string} target
 */
export const parseTarget = (target) => {
  let text = String(target).trim();
  let secure = false;
  const scheme = text.match(/^(\w+):\/\//);
  if (scheme) {
    secure = ["https", "wss"].includes(scheme[1].toLowerCase());
    text = text.slice(scheme[0].length);
  }
  text = text.replace(/\/.*$/, "");
  const url = new URL(`http://${text}`);
  const port = url.port ? Number(url.port) : DEFAULT_CONTROL_PORT;
  const host = url.hostname;
  return {
    host,
    port,
    url: `${secure ? "wss" : "ws"}://${host}:${port}/tunnel`,
    label: `${host}:${port}`,
  };
};

/**
 * @param {{
 *   target: string,
 *   key: string,
 *   ports: { remote: number, local: number }[],
 *   to?: string,
 *   onReady?: (ports: number[]) => void,
 *   onStop?: (reason: string) => void,
 * }} options
 */
export const startClient = (options) => {
  const target = parseTarget(options.target);
  const to = options.to || "127.0.0.1";
  const mappings = new Map(options.ports.map((m) => [m.remote, m.local]));
  let stopped = false;
  let retryDelay = MIN_RETRY_MS;
  /** @type {WebSocket | null} */
  let currentWs = null;

  const describe = () =>
    [...mappings]
      .map(([remote, local]) => `${target.host}:${remote} -> ${to}:${local}`)
      .join(", ");

  const session = () =>
    new Promise((resolve) => {
      const ws = new WebSocket(target.url);
      currentWs = ws;
      let ready = false;
      let closeReason = "";
      let watchdog = null;

      const kick = () => {
        clearTimeout(watchdog);
        watchdog = setTimeout(() => {
          closeReason = "no response from server";
          ws.close();
        }, WATCHDOG_MS);
      };

      const mux = new Mux(ws, (msg) => {
        kick();
        switch (msg?.type) {
          case "ready":
            ready = true;
            retryDelay = MIN_RETRY_MS;
            logger.info(`Tunnel is open: ${describe()}`);
            options.onReady?.(msg.ports);
            break;
          case "error":
            closeReason = msg.message;
            break;
          case "connect": {
            const local = mappings.get(msg.port);
            if (!local) {
              mux.sendControl({ type: "close", id: msg.id });
              break;
            }
            logger.debug(`Visitor ${msg.id} on port ${msg.port}`);
            mux.attach(
              msg.id,
              Deno.connect({ hostname: to, port: local }).catch(
                (err) => {
                  logger.warn(
                    `Cannot connect to ${to}:${local}: ${err.message}`,
                  );
                  throw err;
                },
              ),
            );
            break;
          }
          case "close":
            mux.close(msg.id, false);
            break;
          case "closed":
            mappings.delete(msg.port);
            logger.warn(`Port ${msg.port} was closed by server: ${msg.reason}`);
            if (mappings.size === 0) {
              closeReason = "all ports were closed by server";
              stopped = true;
              ws.close();
            }
            break;
          case "ping":
            mux.sendControl({ type: "pong" });
            break;
          default:
            logger.debug(`Unknown control message: ${msg?.type}`);
        }
      });

      ws.addEventListener("open", async () => {
        kick();
        mux.sendControl({
          type: "hello",
          token: await createToken(options.key),
          ports: [...mappings.keys()],
          version,
        });
      });
      ws.addEventListener("error", () => {
        if (!closeReason) closeReason = "connection failed";
      });
      ws.addEventListener("close", (event) => {
        clearTimeout(watchdog);
        mux.closeAll();
        currentWs = null;
        resolve({
          ready,
          reason: closeReason || event.reason || `code ${event.code}`,
          code: event.code,
        });
      });
    });

  const run = async () => {
    logger.info(`Connecting to ${target.label}...`);
    while (!stopped) {
      const result = await session();
      if (stopped) break;
      if (result.code === 4001 || result.code === 4002) {
        // server rejected us for a reason that will not change by retrying
        logger.error(`Server rejected the tunnel: ${result.reason}`);
        stopped = true;
        options.onStop?.(result.reason);
        return;
      }
      logger.warn(
        `Disconnected (${result.reason}). Reconnecting in ${
          retryDelay / 1000
        }s...`,
      );
      await sleep(retryDelay);
      retryDelay = Math.min(retryDelay * 2, MAX_RETRY_MS);
    }
    options.onStop?.("stopped");
  };

  const done = run();

  return {
    done,
    stop: () => {
      stopped = true;
      currentWs?.close(1000, "client stopped");
    },
  };
};
