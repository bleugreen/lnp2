export const name = "Deploy to lumeneer";
export const description =
  "Sync source, build Rust + web on lumeneer.local, and restart the server. ";

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

export default async function ({ inputs, CWD }) {
  const rustOnly = inputs.rustOnly ?? false;
  const webOnly = inputs.webOnly ?? false;

  let recipe: string;
  if (webOnly) {
    recipe = "build-web";
  } else if (rustOnly) {
    recipe = "deploy-rust";
  } else {
    recipe = "deploy";
  }

  const r = Bun.spawnSync(["just", recipe], {
    cwd: CWD,
    stdout: "pipe",
    stderr: "pipe",
    timeout: 600_000,
  });

  const output = r.stdout.toString() + r.stderr.toString();

  if (r.exitCode !== 0) {
    return `Deploy failed (exit ${r.exitCode}):\n${output.slice(-2000)}`;
  }

  // For web-only, also copy dist to production
  if (webOnly) {
    const cp = Bun.spawnSync(
      ["ssh", "bleu@lumeneer.local",
       "rsync -a --delete /home/bleu/lnp2/builds/$(git -C /home/bleu/lnp2/builds/ ls 2>/dev/null | head -1)/web/dist/ /home/bleu/lnp2/production/web/dist/ 2>/dev/null; " +
       "rsync -a --delete /home/bleu/lnp2/builds/agent-LNP2-3-builder-1/web/dist/ /home/bleu/lnp2/production/web/dist/"],
      { stdout: "pipe", stderr: "pipe", timeout: 15_000 }
    );
    if (cp.exitCode !== 0) {
      return `Web build succeeded but deploy to production failed:\n${cp.stderr.toString()}`;
    }
  }

  return output.replace(/\x1b\[[0-9;]*m/g, "").trim();
}
