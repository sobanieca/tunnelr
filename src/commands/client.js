import { startClient } from "../client.js";
import { findKey } from "../auth.js";
import { parsePortMappings } from "../protocol.js";

export default {
  execute: async (args) => {
    const ports = parsePortMappings(args.port ?? []);
    if (ports.length === 0) {
      throw new Error("Give at least one port to open, example: -p 8500");
    }
    const key = findKey(args);
    const client = startClient({
      target: String(args._[0]),
      key,
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
