/* @ts-self-types="./main.d.ts" */
import args from "./src/args.js";
import logger from "./src/logger.js";
import help from "./src/commands/help.js";
import version from "./src/commands/version.js";
import update from "./src/commands/update.js";
import service from "./src/commands/service.js";
import server from "./src/commands/server.js";
import client from "./src/commands/client.js";

const commands = [
  { name: "help", engine: help },
  { name: "version", engine: version },
  { name: "update", engine: update },
  { name: "service", engine: service },
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
