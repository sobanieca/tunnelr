import { createToken, findKey } from "../auth.js";

/** Prints a one-time token for the HTTP API. */
export default {
  execute: async (args) => console.log(await createToken(findKey(args))),
  match: (args) => args._[0] === "token",
};
