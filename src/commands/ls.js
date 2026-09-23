import { getConnectionsPath, loadConnections } from "../connections.js";
import logger from "../logger.js";

/** `tunnelr ls` lists saved connections, "*" marks the last used one. */
export default {
  execute: () => {
    const { last, connections } = loadConnections();
    if (connections.length === 0) {
      logger.plain(
        "No saved connections. Add one with:\n" +
          "  tunnelr add <name> <vps host[:port]> -p <ports> [-a <key or file>]",
      );
      return;
    }
    const width = Math.max(...connections.map((c) => c.name.length));
    const lines = connections.map((c) => {
      const mark = c.name === last ? "*" : " ";
      const extra = [
        c.auth ? `-a ${c.auth}` : "",
        c.to ? `--to ${c.to}` : "",
      ].filter(Boolean).join(" ");
      return `${mark} ${c.name.padEnd(width)}  ${c.target} -p ${
        c.ports.join(",") || "(none)"
      }${extra ? "  " + extra : ""}`;
    });
    logger.plain(
      `${
        lines.join("\n")
      }\n\n* last used, "tunnelr" opens it. Saved in ${getConnectionsPath()}`,
    );
  },
  match: (args) => args._[0] === "ls",
};
