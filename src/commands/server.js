import { startServer } from "../server.js";
import { findOrCreateKey } from "../auth.js";
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

const DEFAULT_CONTROL_PORT = 8500;

const printBanner = async ({ port, key, keyPath, mode }) => {
  const local = getLocalAddresses();
  const publicAddress = await getPublicAddress();
  const best = publicAddress || local[0] || "127.0.0.1";
  const others = local.filter((a) => a !== best);
  const host = `${formatHost(best)}:${port}`;

  const modeLine = {
    running: `systemd service "${SERVICE_NAME}" is running`,
    installed: `systemd service "${SERVICE_NAME}" installed and started`,
    updated: `systemd service "${SERVICE_NAME}" updated and restarted`,
    foreground: "running in foreground (no systemd service)",
  }[mode];

  logger.plain(`
tunnelr server ${version} - ${modeLine}

  Address:   ${host}${
    others.length ? `\n  Other IPs: ${others.join(", ")}` : ""
  }
  Key:       ${key}
  Key file:  ${keyPath}

Run this command again anytime to see the address and the key.
${
    mode === "foreground" ? "" : `
Logs: journalctl -u ${SERVICE_NAME} -f    Remove: sudo tunnelr service uninstall
`
  }`);
};

const runForeground = async ({ port, key, keyPath, bind }) => {
  const server = startServer({ port, key, bind });
  if (!isInsideService()) {
    await printBanner({ port, key, keyPath, mode: "foreground" });
  } else logger.info(`Listening on port ${port}`);

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
      throw new Error("Server needs one control port, example: -p 8500");
    }
    const port = parsePort(ports[0] ?? DEFAULT_CONTROL_PORT);
    const { key, path: keyPath } = findOrCreateKey(args);
    const options = { port, key, keyPath, bind: args.bind };

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
    await printBanner({ port, key, keyPath, mode });
  },
  match: (args) => args._.length === 0 && (args.port ?? []).length > 0,
};
