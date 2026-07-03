// maw share (maw-rs bun-dev tier) - minimal sshx terminal-share bridge.
//
// maw-rs runs bun-dev plugins with cwd = the plugin directory. Share does not need
// the caller cwd in v0; labels default to the current tmux session or "default".

import { chmodSync, existsSync, mkdirSync, readFileSync, readdirSync, unlinkSync, writeFileSync } from "fs";
import { basename, join } from "path";
import { homedir } from "os";

declare const Bun: any;
declare const process: any;

type Log = (s?: string) => void;
type ShareState = {
  name: string;
  url: string;
  pid: number;
  startedAt: string;
};

const DEFAULT_SERVER = "https://ssh.clubsxai.com";
const DEFAULT_BIN = "sshx";
const LABEL_RE = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$/;

function shareServer(): string {
  return (process.env.MAW_SHARE_SERVER || DEFAULT_SERVER).trim();
}

function sshxBin(): string {
  return (process.env.MAW_SHARE_SSHX_BIN || DEFAULT_BIN).trim();
}

function stateDir(): string {
  const home = process.env.HOME || homedir();
  return join(home, ".maw", "share");
}

function ensureStateDir(): string {
  const dir = stateDir();
  mkdirSync(dir, { recursive: true });
  chmodSync(dir, 0o700);
  return dir;
}

function statePath(label: string): string {
  return join(ensureStateDir(), `${label}.json`);
}

function isAlive(pid: unknown): boolean {
  if (!Number.isInteger(pid) || Number(pid) <= 0) return false;
  try {
    process.kill(Number(pid), 0);
    return true;
  } catch {
    return false;
  }
}

function readState(label: string): ShareState | undefined {
  const path = statePath(label);
  if (!existsSync(path)) return undefined;
  return JSON.parse(readFileSync(path, "utf-8")) as ShareState;
}

function readRequiredState(label: string): ShareState {
  const state = readState(label);
  if (!state) throw new Error(`no share state for '${label}'`);
  return state;
}

function writeState(state: ShareState): void {
  const path = statePath(state.name);
  writeFileSync(path, JSON.stringify(state, null, 2) + "\n", { mode: 0o600 });
  chmodSync(path, 0o600);
}

function removeState(label: string): void {
  const path = statePath(label);
  if (existsSync(path)) unlinkSync(path);
}

function currentTmuxSession(): string | undefined {
  if (!process.env.TMUX) return undefined;
  const p = Bun.spawnSync(["tmux", "display-message", "-p", "#S"], {
    stdout: "pipe",
    stderr: "ignore",
  });
  const name = p.stdout.toString().trim();
  return name || undefined;
}

function defaultLabel(): string {
  const tmux = currentTmuxSession();
  return tmux && LABEL_RE.test(tmux) ? tmux : "default";
}

function normalizeLabel(raw?: string): string {
  const label = (raw || "").trim() || defaultLabel();
  if (!LABEL_RE.test(label)) {
    throw new Error("invalid share label; use letters, digits, dot, dash, or underscore, with no slashes");
  }
  return label;
}

function parseNameFlag(args: string[]): string | undefined {
  for (let i = 1; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "--name") {
      const label = args[i + 1];
      if (!label) throw new Error("usage: maw share start [--name <label>]");
      return label;
    }
    if (arg.startsWith("--name=")) return arg.slice("--name=".length);
  }
  return undefined;
}

function binaryExists(bin: string): boolean {
  if (!bin) return false;
  if (bin.includes("/")) {
    return Bun.spawnSync(["test", "-x", bin]).exitCode === 0;
  }
  return Bun.spawnSync(["sh", "-c", "command -v \"$1\" >/dev/null 2>&1", "sh", bin]).exitCode === 0;
}

function requireSshxBinary(): string {
  const bin = sshxBin();
  if (!binaryExists(bin)) {
    throw new Error(`sshx binary not found: ${bin}. Install sshx or set MAW_SHARE_SSHX_BIN=/path/to/sshx.`);
  }
  return bin;
}

async function readOneLine(stream: ReadableStream<Uint8Array>, timeoutMs = 15000): Promise<string> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error("timed out waiting for sshx session URL on stdout")), timeoutMs);
  });
  const read = (async () => {
    while (!buffer.includes("\n")) {
      const chunk = await reader.read();
      if (chunk.done) break;
      buffer += decoder.decode(chunk.value, { stream: true });
    }
    reader.releaseLock();
    return buffer.split(/\r?\n/, 1)[0].trim();
  })();
  try {
    return await Promise.race([read, timeout]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

function parseSessionUrl(line: string): string {
  if (!line) throw new Error("sshx did not print a session URL");
  let parsed: URL;
  try {
    parsed = new URL(line);
  } catch {
    throw new Error("sshx printed an invalid session URL");
  }
  if (!parsed.hash) throw new Error("sshx session URL is missing its #fragment secret");
  return line;
}

function stateFiles(): string[] {
  const dir = ensureStateDir();
  return readdirSync(dir)
    .filter((file: string) => file.endsWith(".json"))
    .sort();
}

export async function start(log: Log, args: string[]): Promise<void> {
  const label = normalizeLabel(parseNameFlag(args));
  const existing = readState(label);
  if (existing && isAlive(existing.pid)) {
    throw new Error(`share '${label}' is already running (pid ${existing.pid}); run: maw share url ${label}`);
  }
  if (existing) removeState(label);

  const bin = requireSshxBinary();
  const server = shareServer();
  const child = Bun.spawn([bin, "--quiet", "--server", server], {
    stdout: "pipe",
    stderr: "ignore",
    stdin: "ignore",
    detached: true,
  });

  let url: string;
  try {
    url = parseSessionUrl(await readOneLine(child.stdout));
  } catch (error: any) {
    try {
      child.kill("SIGINT");
    } catch {
      // best effort startup cleanup
    }
    throw new Error(error?.message || String(error));
  }

  child.unref();
  writeState({ name: label, url, pid: child.pid, startedAt: new Date().toISOString() });
  log(url);
}

export async function ls(log: Log): Promise<void> {
  const files = stateFiles();
  if (!files.length) {
    log("(no shares)");
    return;
  }
  for (const file of files) {
    const label = basename(file, ".json");
    try {
      const state = readRequiredState(label);
      const status = isAlive(state.pid) ? "running" : "stopped";
      log(`${state.name}  ${status}  pid=${state.pid}  started=${state.startedAt}`);
    } catch (error: any) {
      log(`${label}  unreadable  ${error?.message || String(error)}`);
    }
  }
}

export async function url(log: Log, args: string[]): Promise<void> {
  const label = normalizeLabel(args[1]);
  const state = readRequiredState(label);
  log(state.url);
}

export async function stop(log: Log, args: string[]): Promise<void> {
  const label = normalizeLabel(args[1]);
  const state = readRequiredState(label);
  if (isAlive(state.pid)) {
    try {
      process.kill(state.pid, "SIGINT");
      log(`stopped ${state.name} (pid ${state.pid})`);
    } catch (error: any) {
      if (error?.code !== "ESRCH") throw error;
      log(`removed stale ${state.name} (pid ${state.pid} not alive)`);
    }
  } else {
    log(`removed stale ${state.name} (pid ${state.pid} not alive)`);
  }
  removeState(label);
}

const commands: Record<string, (log: Log, args: string[]) => Promise<void>> = {
  start,
  ls: (log) => ls(log),
  url,
  stop,
};

export const command = {
  name: "share",
  description: "Minimal maw terminal-share bridge for sshx sessions.",
};

export async function cmdShare(args: string[], log: Log = console.log): Promise<void> {
  const sub = args[0];
  const fn = sub ? commands[sub] : undefined;
  if (fn) await fn(log, args);
  else help(log);
}

export default async function handler(ctx: any) {
  const buf: string[] = [];
  const log: Log = (s = "") => (ctx?.writer ? ctx.writer(s) : buf.push(s));
  try {
    const args: string[] = Array.isArray(ctx?.args) ? ctx.args : [];
    await cmdShare(args, log);
    return { ok: true, output: buf.join("\n") || undefined };
  } catch (error: any) {
    return { ok: false, error: error?.message || String(error) };
  }
}

function help(log: Log): void {
  log("maw share - minimal terminal-share bridge");
  log("  start [--name <label>]  start sshx and print the session URL");
  log("  ls                      list local share state without printing URLs");
  log("  url [label]             print a stored share URL");
  log("  stop [label]            SIGINT the share pid and remove state");
}

if (import.meta.main) {
  try {
    await cmdShare(process.argv.slice(2));
  } catch (error: any) {
    console.error(error?.message || String(error));
    process.exit(1);
  }
}
