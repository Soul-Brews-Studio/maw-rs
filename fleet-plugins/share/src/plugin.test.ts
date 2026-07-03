import { afterEach, beforeEach, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { cmdShare } from "./plugin";

const TEST_URL = "http://127.0.0.1:9/s/TEST#deadbeef";

let tmp = "";
let observedPids: number[] = [];
const savedEnv = {
  HOME: process.env.HOME,
  MAW_HOME: process.env.MAW_HOME,
  MAW_DATA_DIR: process.env.MAW_DATA_DIR,
  MAW_SHARE_SSHX_BIN: process.env.MAW_SHARE_SSHX_BIN,
  MAW_SHARE_SERVER: process.env.MAW_SHARE_SERVER,
  TMUX: process.env.TMUX,
};

beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "maw-share-test-"));
  observedPids = [];
  process.env.HOME = join(tmp, "home");
  process.env.MAW_HOME = join(tmp, "maw-home");
  process.env.MAW_DATA_DIR = join(tmp, "maw-data");
  process.env.MAW_SHARE_SERVER = "https://ssh.clubsxai.com";
  delete process.env.TMUX;
});

afterEach(() => {
  cleanupSharePids();
  if (savedEnv.HOME === undefined) delete process.env.HOME;
  else process.env.HOME = savedEnv.HOME;
  if (savedEnv.MAW_HOME === undefined) delete process.env.MAW_HOME;
  else process.env.MAW_HOME = savedEnv.MAW_HOME;
  if (savedEnv.MAW_DATA_DIR === undefined) delete process.env.MAW_DATA_DIR;
  else process.env.MAW_DATA_DIR = savedEnv.MAW_DATA_DIR;
  if (savedEnv.MAW_SHARE_SSHX_BIN === undefined) delete process.env.MAW_SHARE_SSHX_BIN;
  else process.env.MAW_SHARE_SSHX_BIN = savedEnv.MAW_SHARE_SSHX_BIN;
  if (savedEnv.MAW_SHARE_SERVER === undefined) delete process.env.MAW_SHARE_SERVER;
  else process.env.MAW_SHARE_SERVER = savedEnv.MAW_SHARE_SERVER;
  if (savedEnv.TMUX === undefined) delete process.env.TMUX;
  else process.env.TMUX = savedEnv.TMUX;
  rmSync(tmp, { recursive: true, force: true });
});

function shareDir(): string {
  return join(process.env.HOME!, ".maw", "share");
}

function statePath(label: string): string {
  return join(shareDir(), `${label}.json`);
}

function readState(label: string): any {
  return JSON.parse(readFileSync(statePath(label), "utf-8"));
}

function fakeBin(name: string, lines: string[]): string {
  const bin = join(tmp, name);
  writeFileSync(bin, [...lines, ""].join("\n"));
  chmodSync(bin, 0o700);
  return bin;
}

function fakeSuccessSshx(): string {
  return fakeBin("fake-sshx-success", [
    "#!/bin/sh",
    `echo '${TEST_URL}'`,
    "exec sleep 60",
  ]);
}

function fakeFailingSshx(): string {
  return fakeBin("fake-sshx-fail", [
    "#!/bin/sh",
    "echo 'setup detail: retry exhausted' >&2",
    "echo 'grpc Open failed: 403 Forbidden at edge' >&2",
    "exit 42",
  ]);
}

async function runShare(args: string[]): Promise<string[]> {
  const lines: string[] = [];
  await cmdShare(args, (line = "") => lines.push(line));
  return lines;
}

function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function waitForDead(pid: number): Promise<void> {
  for (let i = 0; i < 20; i += 1) {
    if (!pidAlive(pid)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

function cleanupSharePids(): void {
  for (const pid of observedPids) {
    try {
      if (pidAlive(pid)) process.kill(pid, "SIGINT");
    } catch {
      // best effort
    }
  }
  const dir = shareDir();
  if (!existsSync(dir)) return;
  for (const file of readdirSync(dir)) {
    if (!file.endsWith(".json")) continue;
    try {
      const state = JSON.parse(readFileSync(join(dir, file), "utf-8"));
      if (Number.isInteger(state.pid) && pidAlive(state.pid)) process.kill(state.pid, "SIGINT");
    } catch {
      // best effort
    }
  }
}

test("share v0 lifecycle uses fake sshx and isolated state", async () => {
  process.env.MAW_SHARE_SSHX_BIN = fakeSuccessSshx();
  const label = "lifecycle";

  const startLines = await runShare(["start", "--name", label]);
  expect(startLines).toEqual([TEST_URL]);

  expect(existsSync(statePath(label))).toBe(true);
  const state = readState(label);
  observedPids.push(state.pid);
  expect(state.name).toBe(label);
  expect(state.url).toBe(TEST_URL);
  expect(Number.isInteger(state.pid)).toBe(true);
  expect(state.pid).toBeGreaterThan(0);
  expect(Number.isNaN(Date.parse(state.startedAt))).toBe(false);
  expect(pidAlive(state.pid)).toBe(true);

  const lsLines = await runShare(["ls"]);
  expect(lsLines.join("\n")).toContain(`${label}  running  pid=${state.pid}`);
  expect(lsLines.join("\n")).not.toContain(TEST_URL);

  await expect(runShare(["start", "--name", label]))
    .rejects
    .toThrow(`share '${label}' is already running`);

  const urlLines = await runShare(["url", label]);
  expect(urlLines).toEqual([TEST_URL]);

  const stopLines = await runShare(["stop", label]);
  expect(stopLines).toEqual([`stopped ${label} (pid ${state.pid})`]);
  expect(existsSync(statePath(label))).toBe(false);
  await waitForDead(state.pid);
  expect(pidAlive(state.pid)).toBe(false);
});

test("share labels reject slashes", async () => {
  process.env.MAW_SHARE_SSHX_BIN = fakeSuccessSshx();

  await expect(runShare(["start", "--name", "bad/name"]))
    .rejects
    .toThrow("invalid share label");
  expect(existsSync(shareDir())).toBe(false);
});

test("share start failure includes fake sshx stderr tail", async () => {
  process.env.MAW_SHARE_SSHX_BIN = fakeFailingSshx();

  await expect(runShare(["start", "--name", "failing"]))
    .rejects
    .toThrow(/sshx did not print a session URL.*grpc Open failed: 403 Forbidden at edge/s);
});
