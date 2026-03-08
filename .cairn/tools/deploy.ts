export const name = "Deploy to lumeneer";
export const description =
  "Sync source, build Rust + web on lumeneer.local, and restart the server. " +
  "Use after making changes to ship them to the machine.";

export const inputSchema = {
  type: "object",
  properties: {
    rustOnly: {
      type: "boolean",
      description: "Skip web build (Rust-only changes)",
    },
    webOnly: {
      type: "boolean",
      description: "Skip Rust build (web-only changes, no server restart)",
    },
  },
};

const SSH = "bleu@lumeneer.local";
const REMOTE = "~/lnp2";

function ssh(cmd: string, timeout = 120_000) {
  const r = Bun.spawnSync(["ssh", SSH, cmd], {
    stdout: "pipe",
    stderr: "pipe",
    timeout,
  });
  return {
    ok: r.exitCode === 0,
    exitCode: r.exitCode,
    stdout: r.stdout.toString(),
    stderr: r.stderr.toString(),
  };
}

export default async function ({ inputs, CWD }) {
  const steps: string[] = [];
  const rustOnly = inputs.rustOnly ?? false;
  const webOnly = inputs.webOnly ?? false;

  // 1. Rsync source
  const sync = Bun.spawnSync(
    [
      "rsync", "-avz",
      "--exclude", "target/",
      "--exclude", "node_modules/",
      "--exclude", "web/dist/",
      "--exclude", ".git/",
      `${CWD}/`,
      `${SSH}:${REMOTE}/`,
    ],
    { stdout: "pipe", stderr: "pipe", timeout: 30_000 }
  );
  if (sync.exitCode !== 0) {
    return `rsync failed:\n${sync.stderr.toString()}`;
  }
  steps.push("Synced source to lumeneer");

  // 2. Build web
  if (!rustOnly) {
    const web = ssh(
      `cd ${REMOTE}/web && npm install --silent 2>&1 && node ./node_modules/vite/bin/vite.js build 2>&1`,
      60_000
    );
    if (!web.ok) {
      return `Web build failed:\n${web.stdout}\n${web.stderr}`;
    }
    steps.push("Built web GUI");
  }

  // 3. Build Rust
  if (!webOnly) {
    const rust = ssh(
      `cd ${REMOTE} && ~/.cargo/bin/cargo build --release 2>&1`,
      300_000
    );
    if (!rust.ok) {
      return `Rust build failed:\n${rust.stdout.slice(-1500)}`;
    }
    steps.push("Built Rust binary");

    // 4. Restart server
    ssh("pkill -9 -f 'lnp2 serve' 2>/dev/null", 5_000);
    await new Promise((r) => setTimeout(r, 2000));

    ssh(
      `nohup bash -c 'cd ${REMOTE} && exec ./target/release/lnp2 serve --config config' > /tmp/lnp2.log 2>&1 & disown; echo started`,
      8_000
    );
    await new Promise((r) => setTimeout(r, 3000));

    const log = ssh("tail -5 /tmp/lnp2.log", 5_000);
    const listening = log.stdout.includes("Listening on");
    steps.push(listening ? "Server restarted and listening" : "Server started (check log)");
  } else {
    steps.push("Skipped Rust build + restart (web-only)");
  }

  return steps.map((s) => `✓ ${s}`).join("\n");
}
