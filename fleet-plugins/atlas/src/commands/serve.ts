import { findAtlasRepo } from "../lib/repo";

export async function serve(log: (s: string) => void, args: string[]) {
  const repo = findAtlasRepo();
  if (!repo) { log("✗ atlas-oracle repo not found (need parliament/api/server.ts)"); return; }

  const port = args.find(a => a.startsWith("--port="))?.split("=")[1] || "4567";
  const password = process.env.DASHBOARD_PASSWORD || "catlab";
  const autoBuild = args.includes("--build");
  const { execSync } = require("child_process");
  const { existsSync } = require("fs");

  const distPath = `${repo}/parliament/app/dist/index.html`;
  if (autoBuild || !existsSync(distPath)) {
    log(`building PARLIAMENT UI (${repo}/parliament/app)...`);
    try {
      execSync(`cd ${repo}/parliament/app && bun run build`, { stdio: "inherit" });
      log("✓ build complete");
    } catch (e: any) {
      log(`⚠ build failed: ${e.message || e} — serving API only`);
    }
  }

  log(`starting PARLIAMENT on :${port}`);
  log(`  API: ${repo}/parliament/api/server.ts`);
  log(`  UI:  ${existsSync(distPath) ? "parliament/app/dist/ (built)" : "not built — API only"}`);
  try {
    execSync(`bun ${repo}/parliament/api/server.ts`, {
      stdio: "inherit",
      env: { ...process.env, DASHBOARD_PASSWORD: password },
    });
  } catch (e: any) {
    log(`server exited: ${e.message || e}`);
  }
}
