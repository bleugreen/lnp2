export const name = "Lumeneer server status";
export const description =
  "Check if rsrvpnp is running on lumeneer.local and test basic connectivity.";

const SSH = "bleu@lumeneer.local";

function ssh(cmd: string, timeout = 10_000) {
  const r = Bun.spawnSync(["ssh", SSH, cmd], {
    stdout: "pipe",
    stderr: "pipe",
    timeout,
  });
  return { ok: r.exitCode === 0, stdout: r.stdout.toString().trim() };
}

export default async function () {
  const lines: string[] = [];

  // Check process
  const proc = ssh("ps aux | grep 'rsrvpnp serve' | grep -v grep");
  if (proc.ok && proc.stdout) {
    lines.push("Process: running");
    // Extract PID and CPU
    const parts = proc.stdout.split(/\s+/);
    lines.push(`  PID: ${parts[1]}, CPU: ${parts[2]}%, MEM: ${parts[3]}%`);
  } else {
    lines.push("Process: NOT RUNNING");
    return lines.join("\n");
  }

  // Check HTTP
  const http = ssh("curl -sf -o /dev/null -w '%{http_code}' http://localhost:3000/api/camera/list", 5_000);
  lines.push(`HTTP: ${http.ok ? `OK (${http.stdout})` : "unreachable"}`);

  // Camera list
  if (http.ok) {
    const cameras = ssh("curl -sf http://localhost:3000/api/camera/list", 5_000);
    if (cameras.ok) lines.push(`Cameras: ${cameras.stdout}`);
  }

  // Uptime (from log start)
  const logStart = ssh("head -1 /tmp/rsrvpnp.log | grep -oP '\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}'");
  if (logStart.ok && logStart.stdout) {
    lines.push(`Started: ${logStart.stdout}`);
  }

  return lines.join("\n");
}
