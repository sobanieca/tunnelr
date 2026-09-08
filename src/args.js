import { deps } from "./deps.js";

const args = deps.parse(Deno.args, {
  boolean: ["help", "version", "debug", "foreground"],
  string: ["auth", "auth-file", "local-host", "bind"],
  collect: ["port"],
  alias: {
    p: "port",
    a: "auth",
    h: "help",
    v: "version",
  },
});

const kebabToCamel = (str) =>
  str.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());

/** @type {Record<string, any>} */
const normalized = {};
for (const [key, value] of Object.entries(args)) {
  normalized[kebabToCamel(key)] = value;
}

export default normalized;
