import { deps } from "./deps.js";
import logger from "./logger.js";

const TOKEN_LENGTH = 24;
const ALPHABET =
  "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

export const generateToken = () => {
  const bytes = crypto.getRandomValues(new Uint8Array(TOKEN_LENGTH));
  return Array.from(bytes, (b) => ALPHABET[b % ALPHABET.length]).join("");
};

export const getHome = () =>
  Deno.env.get("HOME") || Deno.env.get("USERPROFILE") || ".";

export const getTokenPath = () => deps.join(getHome(), ".tunnelr", "auth");

/** @param {string} path */
const readTokenFile = (path) => {
  try {
    return Deno.readTextFileSync(path).trim();
  } catch {
    return "";
  }
};

/**
 * Token comes from: -a flag, --auth-file <path>, TUNNELR_AUTH env,
 * then the ~/.tunnelr/auth file.
 * @param {{ auth?: string, authFile?: string }} args
 * @returns {string | undefined}
 */
export const findToken = (args) => {
  if (args.auth) return String(args.auth);
  if (args.authFile) {
    const token = readTokenFile(args.authFile);
    if (!token) {
      throw new Error(`Auth file ${args.authFile} is missing or empty`);
    }
    return token;
  }
  const fromEnv = Deno.env.get("TUNNELR_AUTH");
  if (fromEnv) return fromEnv;
  return readTokenFile(getTokenPath()) || undefined;
};

/**
 * Same as findToken, but creates and stores a new token when none exists.
 * With --auth-file the token is written to that file instead of ~/.tunnelr/auth.
 * @param {{ auth?: string, authFile?: string }} args
 * @returns {string}
 */
export const findOrCreateToken = (args) => {
  if (args.authFile && !args.auth && !readTokenFile(args.authFile)) {
    return createToken(args.authFile);
  }
  return findToken(args) ?? createToken(getTokenPath());
};

/** @param {string} path */
const createToken = (path) => {
  const token = generateToken();
  Deno.mkdirSync(deps.dirname(path), { recursive: true });
  Deno.writeTextFileSync(path, token + "\n", { mode: 0o600 });
  logger.info(`New auth token saved to ${path}`);
  return token;
};

/**
 * Compare tokens in constant time.
 * @param {string} a
 * @param {string} b
 */
export const tokensMatch = (a, b) => {
  if (typeof a !== "string" || typeof b !== "string") return false;
  const bytesA = new TextEncoder().encode(a);
  const bytesB = new TextEncoder().encode(b);
  let diff = bytesA.length ^ bytesB.length;
  for (let i = 0; i < Math.max(bytesA.length, bytesB.length); i++) {
    diff |= (bytesA[i] ?? 0) ^ (bytesB[i] ?? 0);
  }
  return diff === 0;
};
