import { existsSync, mkdirSync, renameSync, rmSync, writeFileSync } from "fs";
import { dirname, isAbsolute, join } from "path";

declare const Bun: { which(name: string): string | null; spawnSync(argv: string[]): SpawnResult };
declare const process: { env: Record<string, string | undefined>; cwd(): string; getuid(): number; pid: number };

export const LABEL = "com.maw.menubar";
const API_URL = "http://127.0.0.1:3456";
type Log = (line: string) => void;
export type SpawnResult = { exitCode: number; stdout?: { toString(): string }; stderr?: { toString(): string } };
export type Runtime = {
  home: string; helper: string; maw: string; uid: number;
  spawn(argv: string[]): SpawnResult;
  fetch(url: string): Promise<{ ok: boolean; json(): Promise<unknown> }>;
};

export function defaultRuntime(): Runtime {
  const root = process.cwd();
  return {
    home: process.env.HOME || "", helper: join(root, "bin", "maw-menubar"),
    maw: process.env.MAW_BIN || Bun.which("maw") || "", uid: process.getuid(),
    spawn: (argv) => Bun.spawnSync(argv), fetch: (url) => fetch(url, { signal: AbortSignal.timeout(2_000) }),
  };
}

function xml(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

export function renderPlist(helper: string, maw: string, home: string): string {
  const logRoot = join(home, ".maw", "state", "logs");
  const strings = [helper, "--maw", maw, "--api", API_URL].map((item) => `    <string>${xml(item)}</string>`).join("\n");
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>${LABEL}</string>
  <key>ProgramArguments</key><array>
${strings}
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
  <key>ProcessType</key><string>Interactive</string>
  <key>EnvironmentVariables</key><dict><key>PATH</key><string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin</string></dict>
  <key>StandardOutPath</key><string>${xml(join(logRoot, "maw-menubar.out.log"))}</string>
  <key>StandardErrorPath</key><string>${xml(join(logRoot, "maw-menubar.err.log"))}</string>
</dict></plist>
`;
}

function paths(runtime: Runtime) {
  return {
    plist: join(runtime.home, "Library", "LaunchAgents", `${LABEL}.plist`),
    domain: `gui/${runtime.uid}`, service: `gui/${runtime.uid}/${LABEL}`,
  };
}

function loaded(runtime: Runtime, service: string): SpawnResult {
  return runtime.spawn(["/bin/launchctl", "print", service]);
}

async function status(runtime: Runtime, log: Log): Promise<number> {
  const { plist, service } = paths(runtime); const state = loaded(runtime, service);
  let apiConnected = false;
  try {
    const response = await runtime.fetch(`${API_URL}/api/health`);
    apiConnected = response.ok && (await response.json() as { ok?: boolean }).ok === true;
  } catch {}
  const output = state.stdout?.toString() || "";
  log(`plist-on-disk: ${existsSync(plist) ? "yes" : "no"}`);
  log(`launchd-loaded: ${state.exitCode === 0 ? "yes" : "no"}`);
  log(`process-running: ${state.exitCode === 0 && /state\s*=\s*running|pid\s*=\s*\d+/.test(output) ? "yes" : "no"}`);
  log(`api-connected: ${apiConnected ? "yes" : "no"}`);
  return 0;
}

export async function lifecycle(command: string, log: Log, runtime = defaultRuntime()): Promise<number> {
  const { plist, domain, service } = paths(runtime);
  if (command === "status") return status(runtime, log);
  if (command === "run") return runtime.spawn([runtime.helper, "--maw", runtime.maw, "--api", API_URL]).exitCode;
  const current = loaded(runtime, service);
  if (command === "stop" || command === "uninstall") {
    if (current.exitCode === 0 && runtime.spawn(["/bin/launchctl", "bootout", service]).exitCode !== 0) {
      log("launchctl bootout failed"); return 1;
    }
    if (command === "uninstall") rmSync(plist, { force: true });
    log(`maw menubar ${command}: ok`); return 0;
  }
  if (command === "start") {
    if (!existsSync(plist)) { log("maw menubar start: install first"); return 1; }
    if (current.exitCode !== 0 && runtime.spawn(["/bin/launchctl", "bootstrap", domain, plist]).exitCode !== 0) {
      log("launchctl bootstrap failed"); return 1;
    }
    log("maw menubar start: ok"); return 0;
  }
  if (!runtime.home || !isAbsolute(runtime.helper) || !isAbsolute(runtime.maw) || !existsSync(runtime.helper) || !existsSync(runtime.maw)) {
    log("maw menubar install: absolute helper and maw binaries must exist"); return 1;
  }
  mkdirSync(dirname(plist), { recursive: true, mode: 0o700 });
  mkdirSync(join(runtime.home, ".maw", "state", "logs"), { recursive: true, mode: 0o700 });
  const temporary = `${plist}.tmp-${process.pid}`;
  writeFileSync(temporary, renderPlist(runtime.helper, runtime.maw, runtime.home), { mode: 0o600 });
  const valid = runtime.spawn(["/usr/bin/plutil", "-lint", temporary]).exitCode === 0;
  if (!valid) { rmSync(temporary, { force: true }); log("plutil validation failed"); return 1; }
  if (current.exitCode === 0 && runtime.spawn(["/bin/launchctl", "bootout", service]).exitCode !== 0) return 1;
  renameSync(temporary, plist);
  if (runtime.spawn(["/bin/launchctl", "bootstrap", domain, plist]).exitCode !== 0) {
    log("launchctl bootstrap failed"); return 1;
  }
  log(`maw menubar install: wrote ${plist}`); return 0;
}
