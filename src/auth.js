// Auth: the key is a shared secret that never travels over the wire.
// The client sends a one-time token "<timestamp>.<nonce>.<hmac>" signed with
// the key. The server checks the signature, rejects tokens older than
// TOKEN_TTL_MS and remembers used nonces, so a captured token is useless.

import { deps } from "./deps.js";
import logger from "./logger.js";

const KEY_LENGTH = 32;
const NONCE_BYTES = 12;
const ALPHABET =
  "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
export const TOKEN_TTL_MS = 60_000;

const encoder = new TextEncoder();

/** @param {Uint8Array} bytes */
const toHex = (bytes) =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

export const generateKey = () => {
  const bytes = crypto.getRandomValues(new Uint8Array(KEY_LENGTH));
  return Array.from(bytes, (b) => ALPHABET[b % ALPHABET.length]).join("");
};

export const getHome = () =>
  Deno.env.get("HOME") || Deno.env.get("USERPROFILE") || ".";

export const getKeyPath = () => deps.join(getHome(), ".secret", "tunnelr-key");

/** @param {string} path */
const expandHome = (path) =>
  path.startsWith("~/") ? deps.join(getHome(), path.slice(2)) : path;

/** @param {string} value */
const looksLikePath = (value) =>
  value.includes("/") || value.startsWith("~") || value.startsWith(".");

/** @param {string} path */
export const readKeyFile = (path) => {
  try {
    return Deno.readTextFileSync(path).trim();
  } catch {
    return "";
  }
};

/**
 * Resolve the -a value: a path to a key file, the key itself, or nothing
 * (then ~/.secret/tunnelr-key). `path` is set when the key lives in a file.
 * @param {unknown} auth
 * @returns {{ key: string, path: string }}
 */
export const resolveKey = (auth) => {
  const value = String(auth ?? "").trim();
  if (!value) return { key: readKeyFile(getKeyPath()), path: getKeyPath() };
  if (looksLikePath(value)) {
    const path = expandHome(value);
    return { key: readKeyFile(path), path };
  }
  const fromFile = readKeyFile(value);
  if (fromFile) return { key: fromFile, path: value };
  return { key: value, path: "" };
};

/**
 * Client side: the key must exist.
 * @param {{ auth?: string }} args
 */
export const findKey = (args) => {
  const { key, path } = resolveKey(args.auth);
  if (!key) {
    throw new Error(
      `Key file ${path} is missing or empty. Save the key printed by the server there, or pass -a <key>`,
    );
  }
  return key;
};

/**
 * Server side: use the key from -a or the key file, create and save a new one
 * when there is none. A key given directly is saved to ~/.secret/tunnelr-key.
 * @param {{ auth?: string }} args
 * @returns {{ key: string, path: string }}
 */
export const findOrCreateKey = (args) => {
  const { key, path } = resolveKey(args.auth);
  const target = path || getKeyPath();
  if (key && key === readKeyFile(target)) return { key, path: target };
  const fresh = key || generateKey();
  Deno.mkdirSync(deps.dirname(target), { recursive: true });
  Deno.writeTextFileSync(target, fresh + "\n", { mode: 0o600 });
  logger.info(`Key saved to ${target}`);
  return { key: fresh, path: target };
};

/**
 * @param {string} key
 * @param {string} text
 */
const sign = async (key, text) => {
  const cryptoKey = await crypto.subtle.importKey(
    "raw",
    encoder.encode(key),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", cryptoKey, encoder.encode(text));
  return toHex(new Uint8Array(mac));
};

/**
 * One-time token "<timestamp>.<nonce>.<hmac>" signed with the key.
 * @param {string} key
 */
export const createToken = async (key) => {
  const ts = Date.now();
  const nonce = toHex(crypto.getRandomValues(new Uint8Array(NONCE_BYTES)));
  return `${ts}.${nonce}.${await sign(key, `${ts}.${nonce}`)}`;
};

/**
 * Returns a function that checks tokens and remembers used nonces.
 * The check resolves to "" when the token is fine, or to the reason it is not.
 * @param {string} key
 * @param {number} [ttlMs]
 * @returns {(token: unknown) => Promise<string>}
 */
export const createTokenVerifier = (key, ttlMs = TOKEN_TTL_MS) => {
  /** @type {Map<string, number>} nonce -> when it can be forgotten */
  const used = new Map();
  return async (token) => {
    const [tsText, nonce, mac] = String(token ?? "").split(".");
    const ts = Number(tsText);
    if (!nonce || !mac || !Number.isFinite(ts)) return "malformed token";
    const now = Date.now();
    for (const [n, expires] of used) if (expires <= now) used.delete(n);
    if (Math.abs(now - ts) > ttlMs) {
      return "token expired (check the clock on both machines)";
    }
    if (!safeEqual(mac, await sign(key, `${tsText}.${nonce}`))) {
      return "wrong key";
    }
    if (used.has(nonce)) return "token already used";
    used.set(nonce, ts + ttlMs);
    return "";
  };
};

/**
 * Compare strings in constant time.
 * @param {string} a
 * @param {string} b
 */
export const safeEqual = (a, b) => {
  if (typeof a !== "string" || typeof b !== "string") return false;
  const bytesA = encoder.encode(a);
  const bytesB = encoder.encode(b);
  let diff = bytesA.length ^ bytesB.length;
  for (let i = 0; i < Math.max(bytesA.length, bytesB.length); i++) {
    diff |= (bytesA[i] ?? 0) ^ (bytesB[i] ?? 0);
  }
  return diff === 0;
};
