import { assert, assertEquals, assertNotEquals } from "jsr:@std/assert@1.0.13";
import { startServer } from "../src/server.js";
import { parseTarget, startClient } from "../src/client.js";
import { parsePortMappings } from "../src/protocol.js";
import {
  createToken,
  createTokenVerifier,
  findKey,
  findOrCreateKey,
} from "../src/auth.js";
import logger from "../src/logger.js";

logger.setQuiet(!Deno.args.includes("--verbose"));

const KEY = "test-key-123";
const encoder = new TextEncoder();
const decoder = new TextDecoder();

const freePort = () => {
  const listener = Deno.listen({ port: 0, hostname: "127.0.0.1" });
  const port = /** @type {Deno.NetAddr} */ (listener.addr).port;
  listener.close();
  return port;
};

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const waitFor = async (check, timeoutMs = 5000) => {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (await check()) return;
    await sleep(20);
  }
  throw new Error("Timed out waiting for condition");
};

/** Local TCP echo server that sends back everything it gets. */
const startEcho = (port) => {
  const listener = Deno.listen({ port, hostname: "127.0.0.1" });
  (async () => {
    try {
      for await (const conn of listener) {
        (async () => {
          const buf = new Uint8Array(64 * 1024);
          try {
            while (true) {
              const n = await conn.read(buf);
              if (n === null) break;
              let written = 0;
              while (written < n) {
                written += await conn.write(buf.subarray(written, n));
              }
            }
          } catch {
            // closed
          }
          try {
            conn.close();
          } catch {
            // closed
          }
        })();
      }
    } catch {
      // listener closed
    }
  })();
  return listener;
};

const readExact = async (conn, size) => {
  const out = new Uint8Array(size);
  let got = 0;
  while (got < size) {
    const n = await conn.read(out.subarray(got));
    if (n === null) throw new Error(`Connection ended after ${got} bytes`);
    got += n;
  }
  return out;
};

const connectClient = (controlPort, ports, extra = {}) =>
  new Promise((resolve) => {
    const client = startClient({
      target: `127.0.0.1:${controlPort}`,
      key: KEY,
      ports,
      ...extra,
      onReady: (openedPorts) => {
        extra.onReady?.(openedPorts);
        resolve(client);
      },
    });
  });

const api = async (controlPort, path, init = {}) =>
  fetch(`http://127.0.0.1:${controlPort}${path}`, {
    headers: { authorization: `Bearer ${await createToken(KEY)}` },
    ...init,
  });

Deno.test("echo data goes through the tunnel", async () => {
  const controlPort = freePort();
  const localPort = freePort();
  const remotePort = freePort();
  const server = startServer({ port: controlPort, key: KEY });
  const echo = startEcho(localPort);
  const client = await connectClient(controlPort, [
    { remote: remotePort, local: localPort },
  ]);

  const conn = await Deno.connect({ hostname: "127.0.0.1", port: remotePort });
  await conn.write(encoder.encode("hello tunnel"));
  const reply = await readExact(conn, "hello tunnel".length);
  assertEquals(decoder.decode(reply), "hello tunnel");
  conn.close();

  client.stop();
  await client.done;
  echo.close();
  await server.shutdown();
});

Deno.test("large payload keeps order and size", async () => {
  const controlPort = freePort();
  const localPort = freePort();
  const remotePort = freePort();
  const server = startServer({ port: controlPort, key: KEY });
  const echo = startEcho(localPort);
  const client = await connectClient(controlPort, [
    { remote: remotePort, local: localPort },
  ]);

  const size = 3 * 1024 * 1024;
  const payload = new Uint8Array(size);
  for (let i = 0; i < size; i++) payload[i] = (i * 7) & 0xff;

  const conn = await Deno.connect({ hostname: "127.0.0.1", port: remotePort });
  const writing = (async () => {
    let written = 0;
    while (written < size) {
      written += await conn.write(payload.subarray(written));
    }
  })();
  const reply = await readExact(conn, size);
  await writing;
  assertEquals(reply, payload);
  conn.close();

  client.stop();
  await client.done;
  echo.close();
  await server.shutdown();
});

Deno.test("http server works through the tunnel", async () => {
  const controlPort = freePort();
  const localPort = freePort();
  const remotePort = freePort();
  const server = startServer({ port: controlPort, key: KEY });
  const local = Deno.serve(
    { port: localPort, hostname: "127.0.0.1", onListen: () => {} },
    (req) => new Response(`you asked for ${new URL(req.url).pathname}`),
  );
  const client = await connectClient(controlPort, [
    { remote: remotePort, local: localPort },
  ]);

  const response = await fetch(`http://127.0.0.1:${remotePort}/some/path`);
  assertEquals(await response.text(), "you asked for /some/path");

  client.stop();
  await client.done;
  await local.shutdown();
  await server.shutdown();
});

Deno.test("multiple ports, api list and delete", async () => {
  const controlPort = freePort();
  const localA = freePort();
  const localB = freePort();
  const remoteA = freePort();
  const remoteB = freePort();
  const server = startServer({ port: controlPort, key: KEY });
  const echoA = startEcho(localA);
  const echoB = startEcho(localB);
  let stopReason = "";
  const client = await connectClient(controlPort, [
    { remote: remoteA, local: localA },
    { remote: remoteB, local: localB },
  ], { onStop: (reason) => stopReason = reason });

  const listed = await (await api(controlPort, "/ports")).json();
  assertEquals(
    listed.ports.map((p) => p.port).sort(),
    [remoteA, remoteB].sort(),
  );

  for (const remotePort of [remoteA, remoteB]) {
    const conn = await Deno.connect({
      hostname: "127.0.0.1",
      port: remotePort,
    });
    await conn.write(encoder.encode("ping"));
    assertEquals(decoder.decode(await readExact(conn, 4)), "ping");
    conn.close();
  }

  const unauthorized = await fetch(`http://127.0.0.1:${controlPort}/ports`);
  assertEquals(unauthorized.status, 401);
  await unauthorized.body?.cancel();

  const token = await createToken(KEY);
  const viaQuery = await fetch(
    `http://127.0.0.1:${controlPort}/ports/${remoteA}?token=${token}`,
    { method: "DELETE" },
  );
  assertEquals((await viaQuery.json()).closed, remoteA);

  const replay = await fetch(`http://127.0.0.1:${controlPort}/ports`, {
    headers: { authorization: `Bearer ${token}` },
  });
  assertEquals(replay.status, 401);
  await replay.body?.cancel();

  const afterDelete = await (await api(controlPort, "/ports")).json();
  assertEquals(afterDelete.ports.map((p) => p.port), [remoteB]);

  const missing = await api(controlPort, `/ports/${remoteA}`, {
    method: "DELETE",
  });
  assertEquals(missing.status, 404);
  await missing.body?.cancel();

  await api(controlPort, `/ports/${remoteB}`, { method: "DELETE" }).then((r) =>
    r.body?.cancel()
  );
  await client.done;
  assertEquals(stopReason, "stopped");
  assertEquals(server.listTunnels(), []);

  echoA.close();
  echoB.close();
  await server.shutdown();
});

Deno.test("wrong key is rejected", async () => {
  const controlPort = freePort();
  const server = startServer({ port: controlPort, key: KEY });
  let stopReason = "";
  const client = startClient({
    target: `127.0.0.1:${controlPort}`,
    key: "wrong",
    ports: [{ remote: freePort(), local: 1 }],
    onStop: (reason) => stopReason = reason,
  });
  await client.done;
  assertEquals(stopReason, "wrong key");
  await server.shutdown();
});

Deno.test("tokens are one-time and expire", async () => {
  const verify = createTokenVerifier(KEY, 1000);
  const token = await createToken(KEY);
  assertEquals(await verify(token), "");
  assertEquals(await verify(token), "token already used");
  assertEquals(await verify(await createToken(KEY)), "");
  assertEquals(await verify(await createToken("other")), "wrong key");
  assertEquals(await verify("garbage"), "malformed token");

  const [ts, nonce, mac] = token.split(".");
  // A different timestamp breaks the signature. Derive it from the token's own
  // timestamp, not from Date.now(): on a fast machine the clock can still be in
  // the same millisecond, the mac would match and the nonce check would win.
  assertEquals(await verify(`${Number(ts) + 1}.${nonce}.${mac}`), "wrong key");
  const [, oldNonce, oldMac] = (await createToken(KEY)).split(".");
  const expired = await verify(`${Date.now() - 5000}.${oldNonce}.${oldMac}`);
  assert(expired.startsWith("token expired"));
});

Deno.test("new client takes over a port", async () => {
  const controlPort = freePort();
  const localA = freePort();
  const localB = freePort();
  const remotePort = freePort();
  const server = startServer({ port: controlPort, key: KEY });
  const localServerA = Deno.serve(
    { port: localA, hostname: "127.0.0.1", onListen: () => {} },
    () => new Response("A"),
  );
  const localServerB = Deno.serve(
    { port: localB, hostname: "127.0.0.1", onListen: () => {} },
    () => new Response("B"),
  );

  const first = await connectClient(controlPort, [
    { remote: remotePort, local: localA },
  ]);
  assertEquals(
    await (await fetch(`http://127.0.0.1:${remotePort}/`)).text(),
    "A",
  );

  const second = await connectClient(controlPort, [
    { remote: remotePort, local: localB },
  ]);
  await first.done;
  assertEquals(
    await (await fetch(`http://127.0.0.1:${remotePort}/`)).text(),
    "B",
  );

  second.stop();
  await second.done;
  await localServerA.shutdown();
  await localServerB.shutdown();
  await server.shutdown();
});

Deno.test("client reconnects after server restart", async () => {
  const controlPort = freePort();
  const localPort = freePort();
  const remotePort = freePort();
  let server = startServer({ port: controlPort, key: KEY });
  const echo = startEcho(localPort);
  let readyCount = 0;
  const client = await connectClient(controlPort, [
    { remote: remotePort, local: localPort },
  ], { onReady: () => readyCount++ });

  await server.shutdown();
  await waitFor(() => server.listTunnels().length === 0);
  await sleep(200);
  server = startServer({ port: controlPort, key: KEY });
  await waitFor(() => readyCount >= 1, 10000);
  await waitFor(() => server.listTunnels().length === 1, 10000);

  const conn = await Deno.connect({ hostname: "127.0.0.1", port: remotePort });
  await conn.write(encoder.encode("back"));
  assertEquals(decoder.decode(await readExact(conn, 4)), "back");
  conn.close();

  client.stop();
  await client.done;
  echo.close();
  await server.shutdown();
});

Deno.test("local port down closes the visitor connection", async () => {
  const controlPort = freePort();
  const remotePort = freePort();
  const server = startServer({ port: controlPort, key: KEY });
  const client = await connectClient(controlPort, [
    { remote: remotePort, local: freePort() },
  ]);

  const conn = await Deno.connect({ hostname: "127.0.0.1", port: remotePort });
  const n = await conn.read(new Uint8Array(16));
  assertEquals(n, null);
  conn.close();

  client.stop();
  await client.done;
  await server.shutdown();
});

Deno.test("port parsing", () => {
  assertEquals(parsePortMappings(["8500"]), [{ remote: 8500, local: 8500 }]);
  assertEquals(parsePortMappings(["8500,8600"]), [
    { remote: 8500, local: 8500 },
    { remote: 8600, local: 8600 },
  ]);
  assertEquals(parsePortMappings(["8500:3000", "9000"]), [
    { remote: 8500, local: 3000 },
    { remote: 9000, local: 9000 },
  ]);
  let failed = false;
  try {
    parsePortMappings(["70000"]);
  } catch {
    failed = true;
  }
  assert(failed);

  assertEquals(parseTarget("my-vps.com").url, "ws://my-vps.com:8500/tunnel");
  assertEquals(parseTarget("my-vps.com:3000").port, 3000);
  assertEquals(
    parseTarget("wss://my-vps.com").url,
    "wss://my-vps.com:8500/tunnel",
  );
  assertEquals(parseTarget("http://my-vps.com:2500/").label, "my-vps.com:2500");
  assertNotEquals(parseTarget("[::1]:2500").host, "");
});

Deno.test("key from -a: file path or key itself", async () => {
  const dir = await Deno.makeTempDir();
  const path = `${dir}/key`;
  await Deno.writeTextFile(path, "  file-key \n");
  assertEquals(findKey({ auth: path }), "file-key");
  assertEquals(findKey({ auth: "literalKey" }), "literalKey");
  assertEquals(findOrCreateKey({ auth: path }), { key: "file-key", path });

  let failed = false;
  try {
    findKey({ auth: `${dir}/missing` });
  } catch {
    failed = true;
  }
  assert(failed);

  const created = findOrCreateKey({ auth: `${dir}/new/key` });
  assertEquals(created.path, `${dir}/new/key`);
  assertEquals((await Deno.readTextFile(created.path)).trim(), created.key);
  assertEquals(findOrCreateKey({ auth: created.path }), created);
  await Deno.remove(dir, { recursive: true });
});
