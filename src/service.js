import logger from "./logger.js";

export const SERVICE_NAME = "tunnelr";
export const UNIT_PATH = `/etc/systemd/system/${SERVICE_NAME}.service`;
/** Set in the unit file so the server knows it runs under systemd. */
export const SERVICE_ENV = "TUNNELR_SERVICE";
const START_TIMEOUT_MS = 5000;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const runSystemctl = async (...cmdArgs) => {
  try {
    const { code, stdout, stderr } = await new Deno.Command("systemctl", {
      args: cmdArgs,
      stdout: "piped",
      stderr: "piped",
    }).output();
    return {
      code,
      out: new TextDecoder().decode(stdout).trim(),
      err: new TextDecoder().decode(stderr).trim(),
    };
  } catch (error) {
    return { code: -1, out: "", err: error.message };
  }
};

const systemctl = async (...cmdArgs) => {
  const result = await runSystemctl(...cmdArgs);
  if (result.code !== 0) {
    throw new Error(`systemctl ${cmdArgs.join(" ")} failed: ${result.err}`);
  }
  return result.out;
};

export const isInsideService = () => Deno.env.get(SERVICE_ENV) === "1";

export const isRoot = () => Deno.uid() === 0;

export const hasSystemd = () => {
  try {
    return Deno.statSync("/run/systemd/system").isDirectory;
  } catch {
    return false;
  }
};

/** The command that starts this very program. */
const selfCommand = () => {
  const exec = Deno.execPath();
  const isDenoRuntime = /(^|\/)deno(\.exe)?$/.test(exec);
  if (!isDenoRuntime) return exec;
  return `${exec} run --allow-all ${Deno.mainModule}`;
};

/** @param {{ port: number, auth: string, bind?: string }} options */
const unitFile = ({ port, auth, bind }) =>
  `[Unit]
Description=tunnelr server
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=${selfCommand()} -p ${port} -a ${auth}${bind ? ` --bind ${bind}` : ""}
Restart=always
RestartSec=3
Environment=HOME=/root
Environment=${SERVICE_ENV}=1

[Install]
WantedBy=multi-user.target
`;

const readUnit = async () => {
  try {
    return await Deno.readTextFile(UNIT_PATH);
  } catch {
    return null;
  }
};

export const isActive = async () =>
  (await runSystemctl("is-active", SERVICE_NAME)).out === "active";

const waitUntilActive = async () => {
  const start = Date.now();
  while (Date.now() - start < START_TIMEOUT_MS) {
    if (await isActive()) return true;
    await sleep(200);
  }
  return false;
};

/**
 * Make sure the systemd service exists, matches the given options and runs.
 * @param {{ port: number, auth: string, bind?: string }} options
 * @returns {Promise<"running" | "installed" | "updated">}
 */
export const ensureService = async (options) => {
  const wanted = unitFile(options);
  const current = await readUnit();
  let state;

  if (current === wanted) {
    if (await isActive()) return "running";
    await systemctl("enable", "--now", SERVICE_NAME);
    state = "running";
  } else {
    await Deno.writeTextFile(UNIT_PATH, wanted, { mode: 0o600 });
    logger.info(`${current ? "Updated" : "Wrote"} ${UNIT_PATH}`);
    await systemctl("daemon-reload");
    await systemctl("enable", SERVICE_NAME);
    await systemctl("restart", SERVICE_NAME);
    state = current ? "updated" : "installed";
  }

  if (!(await waitUntilActive())) {
    throw new Error(
      `Service ${SERVICE_NAME} did not start. Check: journalctl -u ${SERVICE_NAME} -n 30`,
    );
  }
  return state;
};

export const uninstallService = async () => {
  const result = await runSystemctl("disable", "--now", SERVICE_NAME);
  if (result.code !== 0) logger.warn(result.err);
  try {
    await Deno.remove(UNIT_PATH);
    logger.info(`Removed ${UNIT_PATH}`);
  } catch {
    logger.warn(`${UNIT_PATH} was not found`);
  }
  await systemctl("daemon-reload");
  logger.info(`Service ${SERVICE_NAME} is removed`);
};
