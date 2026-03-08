export const name = "Lumeneer server logs";
export const description =
  "View recent lnp2 server logs from lumeneer.local. ";

export const inputSchema = {
  type: "object",
  properties: {
    lines: {
      type: "number",
      description: "Number of log lines to show (default: 30)",
    },
    filter: {
      type: "string",
      description: "Grep filter pattern (e.g. 'ERROR', 'camera', '>>')",
    },
  },
};

export default async function ({ inputs, CWD }) {
  const lines = inputs.lines ?? 30;
  const filter = inputs.filter;

  // Use just logs but non-interactive (no -f follow)
  const cmd = filter
    ? `journalctl -u lnp2 -n ${lines} --no-pager | grep -i '${filter}'`
    : `journalctl -u lnp2 -n ${lines} --no-pager`;

  const r = Bun.spawnSync(["ssh", "bleu@lumeneer.local", cmd], {
    stdout: "pipe",
    stderr: "pipe",
    timeout: 10_000,
  });

  if (r.exitCode !== 0 && !r.stdout.toString()) {
    return `No output (exit ${r.exitCode}). Server may not be running.`;
  }

  return r.stdout.toString().replace(/\x1b\[[0-9;]*m/g, "");
}
