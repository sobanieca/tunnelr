import logger from "../logger.js";

const INSTALL_COMMAND =
  "deno install -g --allow-all -f -r -n tunnelr jsr:@sobanieca/tunnelr";

export default {
  execute: async (args) => {
    if (args.deno) {
      logger.info("Updating tunnelr to the latest version...");
      const command = new Deno.Command("deno", {
        args: [
          "install",
          "-g",
          "--allow-all",
          "-f",
          "-r",
          "-n",
          "tunnelr",
          "jsr:@sobanieca/tunnelr",
        ],
        stdout: "inherit",
        stderr: "inherit",
      });
      const { code } = await command.output();
      if (code !== 0) {
        logger.error("Failed to update tunnelr");
        Deno.exit(1);
      }
      logger.info("tunnelr has been updated");
      return;
    }

    logger.plain(`To update tunnelr to the latest version, run:

  ${INSTALL_COMMAND}

Or let tunnelr run it for you:

  tunnelr update --deno

For standalone binaries, run the install script again:

  curl -fsSL sobanieca.github.io/tunnelr/install.sh | bash

Or download manually from:

  https://github.com/sobanieca/tunnelr/releases/latest`);
  },
  match: (args) => args._[0] === "update",
};
