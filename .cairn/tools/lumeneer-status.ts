export const name = "Lumeneer server status";
export const description =
  "Check if lnp2 is running on lumeneer.local and test basic connectivity.";

export default async function ({ CWD }) {
  const r = Bun.spawnSync(["just", "status"], {
    cwd: CWD,
    stdout: "pipe",
    stderr: "pipe",
    timeout: 10_000,
  });

  const output = (r.stdout.toString() + r.stderr.toString()).replace(/\x1b\[[0-9;]*m/g, "").trim();

  if (!output) {
    return "Could not reach lumeneer.local";
  }

  return output;
}
