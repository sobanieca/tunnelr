import { startServer } from "../server.js";
import { findOrCreateToken } from "../auth.js";
import { parsePort } from "../protocol.js";
import { formatHost, getLocalAddresses, getPublicAddress } from "../net.js";
import {
  ensureService,
  hasSystemd,
  isInsideService,
  isRoot,
  SERVICE_NAME,
} from "../service.js";
import { version } from "../version.js";
import logger from "../logger.js";

const DEFAULT_CONTROL_PORT = 2500;

const printBanner = async ({ port, auth, mode }) => {
  const local = getLocalAddresses();
  const publicAddress = await getPublicAddress();
  const best = publicAddress || local[0] || "127.0.0.1";
  const others = local.filter((a) => a !== best);
  const host = `${formatHost(best)}:${port}`;

  const modeLine = {
    running: `systemd service "${SERVICE_NAME}" is already running`,
    installed: `systemd service "${SERVICE_NAME}" installed and started`,
    updated: `systemd service "${SERVICE_NAME}" updated and restarted`,
    foreground: "running in foreground (no systemd service)",
  }[mode];

  logger.plain(`
tunnelr server ${version} - ${modeLine}

  Address:      http://${host}${
    others.length ? `\n  Other IPs:    ${others.join(", ")}` : ""
  }
  Auth token:   ${auth}

Open a tunnel from your machine (VPS port 8500 -> local port 8500):

  tunnelr ${host} -a ${auth} -p 8500

HTTP API:

  curl -H "Authorization: Bearer ${auth}" http://${host}/ports
  curl -X DELETE -H "Authorization: Bearer ${auth}" http://${host}/ports/8500
${
    mode === "foreground" ? "" : `
Service:

  journalctl -u ${SERVICE_NAME} -f      logs
  systemctl status ${SERVICE_NAME}      status
  tunnelr service uninstall         stop and remove
`
  }`);
};

const runForeground = async ({ port, auth, bind }) => {
  const server = startServer({ port, auth, bind });
  if (!isInsideService()) await printBanner({ port, auth, mode: "foreground" });
  else logger.info(`Listening on port ${port}`);

  const stop = async () => {
    logger.info("Shutting down...");
    await server.shutdown();
    Deno.exit(0);
  };
  Deno.addSignalListener("SIGINT", stop);
  Deno.addSignalListener("SIGTERM", stop);
  await new Promise(() => {});
};

/** Why the service cannot be used, or empty when it can. */
const serviceBlocker = () => {
  if (Deno.build.os !== "linux") return "not on Linux";
  if (!hasSystemd()) return "systemd not found";
  if (!isRoot()) return "not running as root (try sudo)";
  return "";
};

export default {
  execute: async (args) => {
    const ports = args.port ?? [];
    if (ports.length > 1) {
      throw new Error("Server needs one control port, example: -p 2500");
    }
    const port = parsePort(ports[0] ?? DEFAULT_CONTROL_PORT);
    const auth = findOrCreateToken(args);
    const options = { port, auth, bind: args.bind };

    if (isInsideService() || args.foreground) {
      return await runForeground(options);
    }

    const blocker = serviceBlocker();
    if (blocker) {
      logger.warn(
        `Cannot install the systemd service (${blocker}). Running in foreground, tunnelr will not survive a reboot.`,
      );
      return await runForeground(options);
    }

    const mode = await ensureService(options);
    await printBanner({ port, auth, mode });
  },
  match: (args) => args._.length === 0 && (args.port ?? []).length > 0,
};
