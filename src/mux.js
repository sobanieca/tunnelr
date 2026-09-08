import { decodeFrame, encodeFrame } from "./protocol.js";
import logger from "./logger.js";

const MAX_BUFFERED_BYTES = 4 * 1024 * 1024;
const READ_BUFFER_SIZE = 32 * 1024;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * @param {Deno.Conn} conn
 * @param {Uint8Array} data
 */
const writeAll = async (conn, data) => {
  let written = 0;
  while (written < data.length) {
    written += await conn.write(data.subarray(written));
  }
};

/**
 * Multiplexes many TCP connections over one WebSocket.
 * Both client and server use the same class.
 */
export class Mux {
  /**
   * @param {WebSocket} ws
   * @param {(msg: any) => void} onControl called for every JSON message
   */
  constructor(ws, onControl) {
    this.ws = ws;
    this.onControl = onControl;
    /** @type {Map<number, any>} */
    this.streams = new Map();
    ws.binaryType = "arraybuffer";
    ws.addEventListener("message", (event) => this.handleMessage(event));
  }

  get isOpen() {
    return this.ws.readyState === WebSocket.OPEN;
  }

  /** @param {MessageEvent} event */
  handleMessage(event) {
    if (typeof event.data === "string") {
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch {
        logger.debug("Ignoring malformed control message");
        return;
      }
      this.onControl(msg);
      return;
    }

    const { id, data } = decodeFrame(new Uint8Array(event.data));
    const stream = this.streams.get(id);
    if (!stream) return;

    stream.writes = stream.writes
      .then(async () => {
        const conn = await stream.ready;
        await writeAll(conn, data);
        if (stream.stats) stream.stats.bytesWritten += data.length;
      })
      .catch(() => this.close(id));
  }

  /**
   * Bind a stream id to a TCP connection and start forwarding its data.
   * @param {number} id
   * @param {Deno.Conn | Promise<Deno.Conn>} ready
   * @param {{ bytesRead: number, bytesWritten: number, active: number } | null} stats
   */
  attach(id, ready, stats = null) {
    const stream = {
      id,
      ready: Promise.resolve(ready),
      writes: Promise.resolve(),
      stats,
    };
    stream.ready.catch(() => {});
    this.streams.set(id, stream);
    if (stats) stats.active++;
    this.readLoop(stream);
  }

  async readLoop(stream) {
    let conn;
    try {
      conn = await stream.ready;
    } catch (err) {
      logger.debug(`Stream ${stream.id} could not connect: ${err.message}`);
      this.close(stream.id);
      return;
    }

    const buffer = new Uint8Array(READ_BUFFER_SIZE);
    try {
      while (this.streams.has(stream.id)) {
        const n = await conn.read(buffer);
        if (n === null) break;
        if (stream.stats) stream.stats.bytesRead += n;
        await this.sendFrame(stream.id, buffer.subarray(0, n));
      }
    } catch (err) {
      logger.debug(`Stream ${stream.id} read ended: ${err.message}`);
    }
    this.close(stream.id);
  }

  /**
   * @param {number} id
   * @param {Uint8Array} data
   */
  async sendFrame(id, data) {
    while (this.ws.bufferedAmount > MAX_BUFFERED_BYTES) {
      if (!this.isOpen) throw new Error("socket closed");
      await sleep(5);
    }
    if (!this.isOpen) throw new Error("socket closed");
    this.ws.send(encodeFrame(id, data));
  }

  /** @param {object} msg */
  sendControl(msg) {
    if (this.isOpen) this.ws.send(JSON.stringify(msg));
  }

  /**
   * Close a stream. Pending writes are flushed first.
   * @param {number} id
   * @param {boolean} notify tell the other side about it
   */
  close(id, notify = true) {
    const stream = this.streams.get(id);
    if (!stream) return;
    this.streams.delete(id);
    if (stream.stats) stream.stats.active--;
    if (notify) this.sendControl({ type: "close", id });
    stream.writes.finally(async () => {
      try {
        (await stream.ready).close();
      } catch {
        // already closed
      }
    });
  }

  /** @param {(stream: any) => boolean} predicate */
  closeWhere(predicate) {
    for (const stream of [...this.streams.values()]) {
      if (predicate(stream)) this.close(stream.id, true);
    }
  }

  closeAll() {
    for (const id of [...this.streams.keys()]) this.close(id, false);
  }
}
