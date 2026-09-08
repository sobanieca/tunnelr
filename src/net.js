import logger from "./logger.js";

const PUBLIC_IP_URLS = ["https://api.ipify.org", "https://api64.ipify.org"];
const PUBLIC_IP_TIMEOUT_MS = 3000;

/**
 * Listen on all interfaces. Tries dual-stack "::" first, then IPv4 only.
 * @param {number} port
 * @param {string} [bind] force a specific address
 * @returns {Deno.TcpListener}
 */
export const listenTcp = (port, bind) => {
  const hostnames = bind ? [bind] : ["::", "0.0.0.0"];
  let lastError;
  for (const hostname of hostnames) {
    try {
      return Deno.listen({ hostname, port, transport: "tcp" });
    } catch (err) {
      lastError = err;
      if (err instanceof Deno.errors.AddrInUse) break;
    }
  }
  throw lastError;
};

const isLinkLocal = (address) =>
  address.startsWith("fe80:") || address.startsWith("169.254.");

const isLoopback = (address) => address === "::1" || address.startsWith("127.");

const isPrivateV4 = (address) =>
  address.startsWith("10.") ||
  address.startsWith("192.168.") ||
  /^172\.(1[6-9]|2\d|3[01])\./.test(address);

/**
 * Local addresses that other machines may use. Public IPv4 first.
 * @returns {string[]}
 */
export const getLocalAddresses = () => {
  const all = Deno.networkInterfaces()
    .map((i) => i.address)
    .filter((a) => !isLoopback(a) && !isLinkLocal(a));
  const score = (a) => {
    if (a.includes(":")) return 2;
    return isPrivateV4(a) ? 1 : 0;
  };
  return [...new Set(all)].sort((a, b) => score(a) - score(b));
};

/** Best effort lookup of the public IP, IPv4 first. @returns {Promise<string | undefined>} */
export const getPublicAddress = async () => {
  for (const url of PUBLIC_IP_URLS) {
    try {
      const response = await fetch(url, {
        signal: AbortSignal.timeout(PUBLIC_IP_TIMEOUT_MS),
      });
      const text = (await response.text()).trim();
      if (response.ok && text) return text;
    } catch (err) {
      logger.debug(`Public IP lookup at ${url} failed: ${err.message}`);
    }
  }
  return undefined;
};

/** Show IPv4 mapped IPv6 addresses ("::ffff:1.2.3.4") as plain IPv4. @param {string} address */
export const cleanAddress = (address) => address.replace(/^::ffff:/i, "");

/** @param {string} address */
export const formatHost = (address) =>
  address.includes(":") ? `[${address}]` : address;
