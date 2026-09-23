import { removeConnection } from "../connections.js";
import logger from "../logger.js";

/** `tunnelr rm <name>` */
export default {
  execute: (args) => {
    const name = args._[1];
    if (!name) throw new Error("Use: tunnelr rm <name>");
    const removed = removeConnection(String(name));
    logger.plain(`Removed connection "${removed.name}" (${removed.target})`);
  },
  match: (args) => args._[0] === "rm",
};
