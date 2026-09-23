import { startClient } from "../client.js";
import { findKey } from "../auth.js";
import { parsePortMappings } from "../protocol.js";
import {
  findConnection,
  findLastConnection,
  markLastConnection,
} from "../connections.js";
import { resolvePorts } from "../connections.js";
import logger from "../logger.js";

/**
 * `tunnelr <host[:port]> -p <ports>`, `tunnelr <name> [-p <ports>]` opens a
 * saved connection and `tunnelr [-p <ports>]` opens the last used one.
 */
export default {
  execute: async (args) => {
    const name = args._[0] === undefined ? undefined : String(args._[0]);
    const saved = name === undefined
      ? findLastConnection()
      : findConnection(name);
    if (name === undefined && !saved) {
      throw new Error(
        "No saved connection to open. Use: tunnelr <vps host[:port]> -p <ports>, " +
          "tunnelr add <name> <vps host[:port]> -p <ports>, or tunnelr server -p <port>",
      );
    }

    let target = name;
    let ports;
    let auth = args.auth;
    let to = args.to;
    if (saved) {
      logger.info(`Using saved connection "${saved.name}" (${saved.target})`);
      target = saved.target;
      ports = resolvePorts(saved.ports, args.port);
      auth ??= saved.auth;
      to ??= saved.to;
      markLastConnection(saved.name);
    } else {
      ports = parsePortMappings(args.port ?? []);
    }
    if (ports.length === 0) {
      throw new Error("Give at least one port to open, example: -p 8500");
    }
    const key = findKey({ auth });
    const client = startClient({ target, key, ports, to });
    const stop = () => {
      client.stop();
      Deno.exit(0);
    };
    Deno.addSignalListener("SIGINT", stop);
    Deno.addSignalListener("SIGTERM", stop);
    await client.done;
    Deno.exit(1);
  },
  // no arguments: open the last saved connection. "tunnelr" without any
  // flags and without a saved connection shows the help instead
  match: (args) =>
    (args._.length === 1 && typeof args._[0] === "string") ||
    (args._.length === 0 &&
      (findLastConnection() !== undefined || (args.port ?? []).length > 0)),
};
