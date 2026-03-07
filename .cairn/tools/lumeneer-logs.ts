export const name = "Lumeneer server logs";
export const description =
  "View recent rsrvpnp server logs from lumeneer.local. " +
  "Useful for debugging after deploy or checking runtime errors.";

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

const SSH = "bleu@lumeneer.local";

export default async function ({ inputs }) {
  const lines = inputs.lines ?? 30;
  const filter = inputs.filter;

  let cmd = `tail -${lines} /tmp/rsrvpnp.log`;
  if (filter) {
    cmd += ` | grep -i '${filter}'`;
  }

  const r = Bun.spawnSync(["ssh", SSH, cmd], {
    stdout: "pipe",
    stderr: "pipe",
    timeout: 10_000,
  });

  if (r.exitCode !== 0 && !r.stdout.toString()) {
    return `No output (exit ${r.exitCode}). Server may not be running.`;
  }

  // Strip ANSI color codes for readability
  return r.stdout.toString().replace(/\x1b\[[0-9;]*m/g, "");
}
