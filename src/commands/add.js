import { addConnection, getConnectionsPath } from "../connections.js";
import logger from "../logger.js";

/** `tunnelr add <name> <host[:port]> -p <ports> [-a <key|file>]` */
export default {
  execute: (args) => {
    const [, name, target] = args._;
    if (!name || !target) {
      throw new Error(
        "Use: tunnelr add <name> <vps host[:port]> -p <ports> [-a <key or file>]",
      );
    }
    const saved = addConnection({
      name: String(name),
      target: String(target),
      ports: args.port ?? [],
      auth: args.auth,
      to: args.to,
    });
    logger.plain(
      `Saved connection "${saved.name}": ${saved.target} -p ${
        saved.ports.join(",") || "(no ports, give -p when running)"
      }\n` +
        `Saved in ${getConnectionsPath()}\n\n` +
        `Open it with:  tunnelr   or   tunnelr ${saved.name} [-p <ports>]`,
    );
  },
  match: (args) => args._[0] === "add",
};
