// Saved connections: name -> VPS address, ports and key, kept in
// ~/.config/tunnelr/connections.json so "tunnelr" alone can open the last
// used tunnel again. The key is not copied here, only the -a value as given
// (usually a path to the key file).

import { deps } from "./deps.js";
import { getHome } from "./auth.js";
import { parsePortMappings } from "./protocol.js";
import { parseTarget } from "./client.js";

/** Names that are commands, so they cannot name a connection. */
export const RESERVED_NAMES = [
  "help",
  "version",
  "update",
  "service",
  "token",
  "server",
  "add",
  "rm",
  "ls",
];

/**
 * @typedef {object} Connection
 * @property {string} name
 * @property {string} target  host[:port] of the VPS
 * @property {string[]} ports  "remote:local" entries
 * @property {string} [auth]  -a value: key file path or the key itself
 * @property {string} [to]
 */

/** @typedef {{ last: string, connections: Connection[] }} Store */

export const getConnectionsPath = () =>
  deps.join(getHome(), ".config", "tunnelr", "connections.json");

/**
 * @param {string} [path]
 * @returns {Store}
 */
export const loadConnections = (path = getConnectionsPath()) => {
  try {
    const data = JSON.parse(Deno.readTextFileSync(path));
    return {
      last: typeof data.last === "string" ? data.last : "",
      connections: Array.isArray(data.connections) ? data.connections : [],
    };
  } catch {
    return { last: "", connections: [] };
  }
};

/**
 * @param {Store} store
 * @param {string} [path]
 */
export const saveConnections = (store, path = getConnectionsPath()) => {
  Deno.mkdirSync(deps.dirname(path), { recursive: true });
  Deno.writeTextFileSync(path, JSON.stringify(store, null, 2) + "\n", {
    mode: 0o600,
  });
};

/**
 * @param {{ remote: number, local: number }[]} mappings
 * @returns {string[]}
 */
export const formatPorts = (mappings) =>
  mappings.map((m) =>
    m.remote === m.local ? `${m.remote}` : `${m.remote}:${m.local}`
  );

/**
 * Validate and save a connection. An existing one with the same name is
 * replaced. The new connection becomes the last used one.
 * @param {Connection} connection
 * @param {string} [path]
 * @returns {Connection}
 */
export const addConnection = (connection, path = getConnectionsPath()) => {
  const name = String(connection.name ?? "").trim();
  if (!name) throw new Error("Give the connection a name");
  if (RESERVED_NAMES.includes(name)) {
    throw new Error(`"${name}" is a tunnelr command, pick another name`);
  }
  const target = String(connection.target ?? "").trim();
  if (!target) throw new Error("Give the VPS address, example: 1.2.3.4:20185");
  parseTarget(target);
  /** @type {Connection} */
  const saved = {
    name,
    target,
    ports: formatPorts(parsePortMappings(connection.ports ?? [])),
  };
  if (connection.auth) saved.auth = connection.auth;
  if (connection.to) saved.to = connection.to;

  const store = loadConnections(path);
  store.connections = store.connections.filter((c) => c.name !== name);
  store.connections.push(saved);
  store.last = name;
  saveConnections(store, path);
  return saved;
};

/**
 * @param {string} name
 * @param {string} [path]
 * @returns {Connection} the removed connection
 */
export const removeConnection = (name, path = getConnectionsPath()) => {
  const store = loadConnections(path);
  const found = store.connections.find((c) => c.name === name);
  if (!found) throw new Error(`No connection named "${name}", see: tunnelr ls`);
  store.connections = store.connections.filter((c) => c.name !== name);
  if (store.last === name) store.last = store.connections.at(-1)?.name ?? "";
  saveConnections(store, path);
  return found;
};

/**
 * @param {string} name
 * @param {string} [path]
 * @returns {Connection | undefined}
 */
export const findConnection = (name, path = getConnectionsPath()) =>
  loadConnections(path).connections.find((c) => c.name === name);

/**
 * @param {string} [path]
 * @returns {Connection | undefined}
 */
export const findLastConnection = (path = getConnectionsPath()) => {
  const store = loadConnections(path);
  return store.connections.find((c) => c.name === store.last) ??
    store.connections.at(-1);
};

/**
 * @param {string} name
 * @param {string} [path]
 */
export const markLastConnection = (name, path = getConnectionsPath()) => {
  const store = loadConnections(path);
  if (store.last === name) return;
  if (!store.connections.some((c) => c.name === name)) return;
  store.last = name;
  saveConnections(store, path);
};

/**
 * Ports to open: the saved ones, unless -p is given. A bare port in -p (no
 * "remote:local") keeps the saved VPS port at the same position and only
 * changes the local port, so with saved "30185:3000" the flag -p 4000 opens
 * 30185:4000. Extra bare ports open the same port on both sides.
 * @param {string[]} savedPorts
 * @param {unknown} override -p values
 * @returns {{ remote: number, local: number }[]}
 */
export const resolvePorts = (savedPorts, override) => {
  const saved = parsePortMappings(savedPorts);
  const list = (Array.isArray(override) ? override : [override])
    .filter((v) => v !== undefined && v !== null && v !== "")
    .flatMap((v) => String(v).split(","))
    .map((v) => v.trim())
    .filter(Boolean);
  if (list.length === 0) return saved;
  const merged = list.map((item, i) =>
    item.includes(":") || !saved[i] ? item : `${saved[i].remote}:${item}`
  );
  return parsePortMappings(merged);
};
