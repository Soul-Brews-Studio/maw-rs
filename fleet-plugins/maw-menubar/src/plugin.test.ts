import { expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { LABEL, lifecycle, renderPlist, type Runtime } from "./launchagent";
import handler, { LIFECYCLE_COMMANDS, handleMenubar } from "./plugin";

function run(args: string[]) {
  return handler({ source: "cli", args });
}

test("help exposes the complete lifecycle surface", async () => {
  const result = await run(["--help"]);
  expect(result).toMatchObject({ ok: true, exitCode: 0 });
  for (const command of LIFECYCLE_COMMANDS) expect(result.output).toContain(command);
});

test("dispatcher routes every lifecycle command", async () => {
  for (const command of LIFECYCLE_COMMANDS) {
    const lines: string[] = [];
    const calls: string[] = [];
    const exitCode = await handleMenubar([command], (line) => lines.push(line), async (value) => {
      calls.push(value); return 0;
    });
    expect(exitCode).toBe(0);
    expect(calls).toEqual([command]);
  }
});

test("no arguments defaults to status and unknown input shows usage", async () => {
  const calls: string[] = [];
  await handleMenubar([], () => {}, async (value) => { calls.push(value); return 0; });
  expect(calls).toEqual(["status"]);
  const unknown = await run(["bogus"]);
  expect(unknown).toMatchObject({ ok: false, exitCode: 1 });
  expect(unknown.output).toContain("Unknown subcommand: bogus");
  expect(unknown.output).toContain("Usage:");
});

test("writer mode streams output instead of returning duplicate text", async () => {
  const lines: string[] = [];
  const result = await handler({ source: "cli", args: ["--help"], writer: (line) => lines.push(line) });
  expect(result.output).toBe("");
  expect(lines.length).toBeGreaterThan(1);
});

function fakeRuntime(home: string, calls: string[][], print: string | null, api = false, plutil = 0): Runtime {
  const helper = join(home, "helper"); const maw = join(home, "maw");
  writeFileSync(helper, "helper"); writeFileSync(maw, "maw");
  return {
    home, helper, maw, uid: 501,
    spawn(argv) {
      calls.push(argv);
      if (argv[0] === "/usr/bin/plutil") return { exitCode: plutil };
      if (argv[1] === "print") return { exitCode: print === null ? 1 : 0, stdout: { toString: () => print || "" } };
      return { exitCode: 0 };
    },
    fetch: async () => ({ ok: api, json: async () => ({ ok: api }) }),
  };
}

test("install renders a valid plist and uses structural argv", async () => {
  const home = mkdtempSync(join(tmpdir(), "maw-menubar-")); const calls: string[][] = [];
  const runtime = fakeRuntime(home, calls, null);
  expect(await lifecycle("install", () => {}, runtime)).toBe(0);
  const plist = join(home, "Library", "LaunchAgents", `${LABEL}.plist`);
  expect(Bun.spawnSync(["/usr/bin/plutil", "-lint", plist]).exitCode).toBe(0);
  expect(readFileSync(plist, "utf8")).toBe(renderPlist(runtime.helper, runtime.maw, home));
  expect(calls).toContainEqual(["/usr/bin/plutil", "-lint", `${plist}.tmp-${process.pid}`]);
  expect(calls).toContainEqual(["/bin/launchctl", "bootstrap", "gui/501", plist]);
  rmSync(home, { recursive: true, force: true });
});

test("loaded is distinct from running and API-connected", async () => {
  const home = mkdtempSync(join(tmpdir(), "maw-menubar-")); const calls: string[][] = [];
  const plist = join(home, "Library", "LaunchAgents", `${LABEL}.plist`);
  mkdirSync(join(home, "Library", "LaunchAgents"), { recursive: true }); writeFileSync(plist, "installed");
  const lines: string[] = [];
  await lifecycle("status", (line) => lines.push(line), fakeRuntime(home, calls, "state = waiting"));
  expect(lines).toEqual(["plist-on-disk: yes", "launchd-loaded: yes", "process-running: no", "api-connected: no"]);
  const healthy: string[] = [];
  await lifecycle("status", (line) => healthy.push(line), fakeRuntime(home, calls, "state = running\npid = 44", true));
  expect(healthy).toEqual(["plist-on-disk: yes", "launchd-loaded: yes", "process-running: yes", "api-connected: yes"]);
  expect(await lifecycle("uninstall", () => {}, fakeRuntime(home, calls, "state = running\npid = 44"))).toBe(0);
  expect(calls).toContainEqual(["/bin/launchctl", "bootout", `gui/501/${LABEL}`]);
  expect(existsSync(plist)).toBe(false); rmSync(home, { recursive: true, force: true });
});

test("failed validation preserves the installed plist", async () => {
  const home = mkdtempSync(join(tmpdir(), "maw-menubar-")); const calls: string[][] = [];
  const plist = join(home, "Library", "LaunchAgents", `${LABEL}.plist`);
  mkdirSync(join(home, "Library", "LaunchAgents"), { recursive: true }); writeFileSync(plist, "old");
  expect(await lifecycle("install", () => {}, fakeRuntime(home, calls, null, false, 1))).toBe(1);
  expect(readFileSync(plist, "utf8")).toBe("old");
  expect(readdirSync(join(home, "Library", "LaunchAgents"))).toEqual([`${LABEL}.plist`]);
  rmSync(home, { recursive: true, force: true });
});
