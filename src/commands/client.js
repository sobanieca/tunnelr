import { startClient } from "../client.js";
import { findToken } from "../auth.js";
import { parsePortMappings } from "../protocol.js";

export default {
  execute: async (args) => {
    const ports = parsePortMappings(args.port ?? []);
    if (ports.length === 0) {
      throw new Error("Give at least one port to open, example: -p 8500");
    }
    const auth = findToken(args);
    if (!auth) {
      throw new Error(
        "Auth token is missing. Pass -a <token> or --auth-file <path>, set TUNNELR_AUTH or create ~/.tunnelr/auth",
      );
    }
    const client = startClient({
      target: String(args._[0]),
      auth,
      ports,
      localHost: args.localHost,
    });
    const stop = () => {
      client.stop();
      Deno.exit(0);
    };
    Deno.addSignalListener("SIGINT", stop);
    Deno.addSignalListener("SIGTERM", stop);
    await client.done;
    Deno.exit(1);
  },
  match: (args) => args._.length === 1 && typeof args._[0] === "string",
};
