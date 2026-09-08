import { uninstallService } from "../service.js";
import serverCommand from "./server.js";

export default {
  execute: async (args) => {
    const action = args._[1];
    if (action === "install") {
      // same as running the server: it installs and starts the service
      return await serverCommand.execute({ ...args, _: [] });
    }
    if (action === "uninstall") return await uninstallService();
    throw new Error(
      "Use: tunnelr service install [-p 2500] [-a token]  or  tunnelr service uninstall",
    );
  },
  match: (args) => args._[0] === "service",
};
