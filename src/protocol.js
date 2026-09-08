// Wire format used between tunnelr client and server over one WebSocket.
//
// Text frames carry JSON control messages:
//   client -> server: { type: "hello", auth, ports: [8500, 8600], version }
//   server -> client: { type: "ready", ports: [8500, 8600] }
//   server -> client: { type: "error", message }
//   server -> client: { type: "connect", id, port }   new visitor on a port
//   both directions:  { type: "close", id }            stream ended
//   server -> client: { type: "closed", port, reason } port was removed
//   server -> client: { type: "ping" }, client -> server: { type: "pong" }
//
// Binary frames carry stream data: 4 bytes stream id (big endian) + payload.

const HEADER_SIZE = 4;

/**
 * @param {number} id
 * @param {Uint8Array} data
 * @returns {Uint8Array}
 */
export const encodeFrame = (id, data) => {
  const frame = new Uint8Array(HEADER_SIZE + data.length);
  new DataView(frame.buffer).setUint32(0, id);
  frame.set(data, HEADER_SIZE);
  return frame;
};

/**
 * @param {Uint8Array} frame
 * @returns {{ id: number, data: Uint8Array }}
 */
export const decodeFrame = (frame) => {
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  return { id: view.getUint32(0), data: frame.subarray(HEADER_SIZE) };
};

const MIN_PORT = 1;
const MAX_PORT = 65535;

/**
 * @param {unknown} value
 * @returns {number}
 */
export const parsePort = (value) => {
  const port = Number(value);
  if (!Number.isInteger(port) || port < MIN_PORT || port > MAX_PORT) {
    throw new Error(`Invalid port: ${value}`);
  }
  return port;
};

/**
 * Parse -p values like "8500", "8500,8600" or "8500:3000" (remote:local).
 * @param {unknown} values
 * @returns {{ remote: number, local: number }[]}
 */
export const parsePortMappings = (values) => {
  const list = Array.isArray(values) ? values : [values];
  const mappings = [];
  for (const raw of list) {
    if (raw === undefined || raw === null || raw === "") continue;
    for (const item of String(raw).split(",")) {
      const part = item.trim();
      if (!part) continue;
      const [remote, local] = part.split(":");
      mappings.push({
        remote: parsePort(remote),
        local: parsePort(local ?? remote),
      });
    }
  }
  const seen = new Set();
  for (const mapping of mappings) {
    if (seen.has(mapping.remote)) {
      throw new Error(`Port ${mapping.remote} was given more than once`);
    }
    seen.add(mapping.remote);
  }
  return mappings;
};
