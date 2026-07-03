/**
 * Atlas-oracle repo resolver.
 */
import { existsSync } from "fs";
import { resolve } from "path";

// maw-rs runs bun-dev plugins with cwd = plugin dir. The invoking shell's $PWD is
// the caller repo/user directory and is the right base for relative Atlas state.
export function invokeDir(): string {
  return process.env.PWD || process.cwd();
}

export function resolveFromInvoke(path: string): string {
  return resolve(invokeDir(), path);
}

function isAtlasOpsRepo(root: string): boolean {
  return existsSync(`${root}/parliament/api/server.ts`) ||
    existsSync(`${root}/scripts/transcribe.py`) ||
    existsSync(`${root}/.discord`) ||
    existsSync(`${root}/fleet-registry.json`) ||
    existsSync(`${root}/ψ/inbox`);
}

export function findAtlasRepo(): string | null {
  const { execSync } = require("child_process");
  const candidates: string[] = [];

  if (process.env.ATLAS_REPO) candidates.push(process.env.ATLAS_REPO);

  try {
    const callerRepo = execSync("git rev-parse --show-toplevel", {
      cwd: invokeDir(),
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    candidates.push(callerRepo);
  } catch {}

  candidates.push(invokeDir());

  try {
    const ghqRoot = execSync("ghq root", { encoding: "utf8" }).trim();
    candidates.push(
      `${ghqRoot}/github.com/Soul-Brews-Studio/atlas-oracle`,
      `${ghqRoot}/github.com/nat-build-with-oracle/maw-atlas`,
    );
  } catch {}

  candidates.push(
    "/opt/Code/github.com/Soul-Brews-Studio/atlas-oracle",
    "/opt/Code/github.com/nat-build-with-oracle/maw-atlas",
  );

  for (const p of [...new Set(candidates)].filter(Boolean)) {
    const root = resolve(p);
    if (isAtlasOpsRepo(root)) return root;
  }

  return null;
}
