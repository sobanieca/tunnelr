import { deps } from "./deps.js";

const debugEnabled = Deno.args.includes("--debug");
let quiet = false;

const stamp = () => {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return deps.colors.gray(
    `[${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}]`,
  );
};

const format = (value) =>
  typeof value === "string" ? value : Deno.inspect(value, { depth: 4 });

const write = (color, value) => {
  if (quiet) return;
  console.log(`${stamp()} ${color(format(value))}`);
};

const logger = {
  /** @param {unknown} value */
  info: (value) => write(deps.colors.brightBlue, value),
  /** @param {unknown} value */
  warn: (value) => write(deps.colors.brightYellow, value),
  /** @param {unknown} value */
  error: (value) => write(deps.colors.brightRed, value),
  /** @param {unknown} value */
  debug: (value) => {
    if (debugEnabled) write(deps.colors.gray, value);
  },
  /** Print text as is, without timestamp and color. @param {string} text */
  plain: (text) => {
    if (!quiet) console.log(text);
  },
  /** Turn all output off (used by tests). @param {boolean} value */
  setQuiet: (value) => {
    quiet = value;
  },
};

export default logger;
