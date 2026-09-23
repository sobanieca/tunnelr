/* @ts-self-types="./main.d.ts" */
import args from "./src/args.js";
import logger from "./src/logger.js";
import help from "./src/commands/help.js";
import version from "./src/commands/version.js";
import update from "./src/commands/update.js";
import token from "./src/commands/token.js";
import server from "./src/commands/server.js";
import client from "./src/commands/client.js";
import add from "./src/commands/add.js";
import rm from "./src/commands/rm.js";
import ls from "./src/commands/ls.js";

const commands = [
  { name: "help", engine: help },
  { name: "version", engine: version },
  { name: "update", engine: update },
  { name: "token", engine: token },
  { name: "add", engine: add },
  { name: "rm", engine: rm },
  { name: "ls", engine: ls },
  { name: "server", engine: server },
  { name: "client", engine: client },
];

logger.debug("Args provided:");
logger.debug(args);

const command = commands.find((c) => c.engine.match(args)) ??
  commands[0];
logger.debug(`Executing command ${command.name}`);

try {
  await command.engine.execute(args);
} catch (err) {
  logger.error(err instanceof Error ? err.message : String(err));
  logger.debug(err);
  Deno.exit(1);
}
