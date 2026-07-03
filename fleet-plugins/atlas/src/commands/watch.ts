/**
 * maw atlas watch — Tier 2 channel watcher / auto-spawn core.
 *
 * Polls a Discord forum/text channel for newly active threads. For each new
 * thread it applies the watch guard layer, asks for `go`, then wakes a worker
 * and records thread → pane routing.
 */
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, writeFileSync } from "fs";
import { dirname, resolve } from "path";
import { execFile } from "child_process";
import { getChannel, getGuildChannels, getMessages, listGuilds, postMessage } from "../lib/discord";
import { findAtlasRepo, invokeDir, resolveFromInvoke } from "../lib/repo";
import * as watchGuards from "../lib/watch-guards";

type Log = (s: string) => void;

type ThreadInfo = {
  id: string;
  name: string;
  parent_id?: string;
  guild_id?: string;
  owner_id?: string;
  thread_metadata?: { archived?: boolean };
};

type WatchState = {
  knownThreads: Record<string, { name: string; seenAt: string }>;
  pending: Record<string, { confirmationId?: string; requestedAt: string; workerName: string; branchName: string }>;
  spawned: Record<string, { workerName: string; pane: string; spawnedAt: string }>;
};

type GuardResult = boolean | { ok?: boolean; allowed?: boolean; reason?: string; warning?: string };

type WatchGuards = typeof watchGuards;

const DEFAULT_INTERVAL_MS = 10_000;
const DEFAULT_CONFIRM_TIMEOUT_MS = 10 * 60_000;
const DEFAULT_ROUTING_TABLE = "/opt/Code/github.com/Soul-Brews-Studio/atlas-oracle/.discord/thread-routing.json";
const DEFAULT_ACCESS_FILE = "/opt/Code/github.com/Soul-Brews-Studio/atlas-oracle/.discord/access.json";
const DEFAULT_STATE_FILE = ".maw/atlas-watch/state.json";
const DEFAULT_NOTIFY_TARGET = "01-atlas:1";

function argValue(args: string[], name: string): string | undefined {
  const exact = args.find(a => a.startsWith(`${name}=`));
  if (exact) return exact.slice(name.length + 1);
  const idx = args.indexOf(name);
  return idx >= 0 ? args[idx + 1] : undefined;
}

function intArg(args: string[], name: string, fallback: number): number {
  const raw = argValue(args, name);
  const parsed = raw ? Number.parseInt(raw, 10) : NaN;
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function hasFlag(args: string[], name: string): boolean {
  return args.includes(name);
}

function sleep(ms: number) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

function readJson<T>(file: string, fallback: T): T {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch { return fallback; }
}

function writeJson(file: string, value: any) {
  mkdirSync(dirname(file), { recursive: true });
  const tmp = `${file}.tmp-${process.pid}`;
  writeFileSync(tmp, `${JSON.stringify(value, null, 2)}\n`);
  renameSync(tmp, file);
}

function atlasRepo(): string | null {
  return findAtlasRepo() || (existsSync(DEFAULT_ROUTING_TABLE) ? "/opt/Code/github.com/Soul-Brews-Studio/atlas-oracle" : null);
}

function routingPath(args: string[]): string {
  const explicit = argValue(args, "--routing") || argValue(args, "--config") || process.env.DISCORD_THREAD_ROUTING;
  if (explicit) return resolveFromInvoke(explicit);
  const repo = atlasRepo();
  return repo ? resolve(repo, ".discord/thread-routing.json") : DEFAULT_ROUTING_TABLE;
}

function accessPath(args: string[]): string {
  const explicit = argValue(args, "--access") || process.env.ATLAS_ACCESS_JSON;
  if (explicit) return resolveFromInvoke(explicit);
  const repo = atlasRepo();
  return repo ? resolve(repo, ".discord/access.json") : DEFAULT_ACCESS_FILE;
}

function statePath(args: string[]): string {
  return resolveFromInvoke(argValue(args, "--state") || process.env.ATLAS_WATCH_STATE || DEFAULT_STATE_FILE);
}

function initialState(): WatchState {
  return { knownThreads: {}, pending: {}, spawned: {} };
}

function loadState(args: string[]): WatchState {
  const state = readJson<WatchState>(statePath(args), initialState());
  return {
    knownThreads: state.knownThreads || {},
    pending: state.pending || {},
    spawned: state.spawned || {},
  };
}

function gateOk(result: GuardResult): { ok: boolean; reason?: string; warning?: string } {
  if (typeof result === "boolean") return { ok: result };
  return {
    ok: result.ok ?? result.allowed ?? false,
    reason: result.reason,
    warning: result.warning,
  };
}

function fallbackAllowFrom(userId: string | undefined, channelId: string, file: string): { ok: boolean; reason?: string } {
  if (!userId) return { ok: false, reason: "thread owner_id missing" };
  const access = readJson<any>(file, {});
  const globalAllow = Array.isArray(access.allowFrom) ? access.allowFrom : [];
  const groupAllow = Array.isArray(access.groups?.[channelId]?.allowFrom) ? access.groups[channelId].allowFrom : [];
  const allowed = new Set([...globalAllow, ...groupAllow].map(String));
  return allowed.has(userId) ? { ok: true } : { ok: false, reason: `user ${userId} not in allowFrom` };
}

async function allowFromGate(guards: WatchGuards, thread: ThreadInfo, channelId: string, args: string[]) {
  const file = accessPath(args);
  const fn = guards.allowFromGate || guards.checkAllowFrom;
  if (!fn) return fallbackAllowFrom(thread.owner_id, channelId, file);
  return gateOk(await fn({ userId: thread.owner_id, thread, channelId, accessPath: file }, thread.owner_id, channelId, file));
}

function sanitizeThreadName(guards: WatchGuards, name: string): string {
  const fn = guards.sanitizeThreadName || guards.sanitizeInput;
  const cleaned = fn ? fn(name, 50) : name.replace(/[\u0000-\u001f\u007f-\u009f]/g, "").trim().slice(0, 50);
  return cleaned || "atlas-thread";
}

function sanitizeBranchName(guards: WatchGuards, name: string): string {
  if (guards.sanitizeBranchName) return guards.sanitizeBranchName(name);
  const clean = sanitizeThreadName(guards, name).toLowerCase()
    .replace(/[^a-z0-9._/-]+/g, "-")
    .replace(/^[-/.]+|[-/.]+$/g, "")
    .replace(/[-/]{2,}/g, "-")
    .slice(0, 50);
  return clean || `thread-${Date.now()}`;
}

function activeWorkerCount(state: WatchState): number {
  return Object.keys(state.spawned || {}).length;
}

async function maxWorktreesGate(guards: WatchGuards, state: WatchState, args: string[]) {
  const max = intArg(args, "--max-worktrees", 10);
  const count = activeWorkerCount(state);
  const fn = guards.maxWorktreesGate || guards.checkMaxWorktrees;
  if (fn) return gateOk(await fn({ activeWorkers: count, maxWorktrees: max, state }, count, max));
  return count >= max ? { ok: false, reason: `active workers ${count} >= maxWorktrees ${max}` } : { ok: true };
}

async function emitBudgetAlert(guards: WatchGuards, state: WatchState, args: string[], log: Log) {
  const threshold = intArg(args, "--budget-threshold", 8);
  const count = activeWorkerCount(state);
  if (guards.budgetAlert) {
    const warning = await guards.budgetAlert({ activeWorkers: count, threshold, state }, count, threshold);
    if (warning) log(String(warning));
    return;
  }
  if (count > threshold) log(`warning: active workers ${count} > budget threshold ${threshold}`);
}

async function resolveChannel(token: string, input: string): Promise<{ id: string; guildId: string; name?: string } | null> {
  if (/^\d{17,20}$/.test(input)) {
    const ch = await getChannel(token, input);
    return ch?.id && ch?.guild_id ? { id: ch.id, guildId: ch.guild_id, name: ch.name } : null;
  }

  const clean = input.replace(/^#/, "").toLowerCase();
  const guilds = await listGuilds(token);
  if (!Array.isArray(guilds)) return null;

  for (const guild of guilds) {
    const channels = await getGuildChannels(token, guild.id);
    if (!Array.isArray(channels)) continue;
    const match = channels.find((c: any) => c.name?.toLowerCase() === clean || c.name?.toLowerCase().includes(clean));
    if (match) return { id: match.id, guildId: guild.id, name: match.name };
  }
  return null;
}

async function listActiveThreads(token: string, guildId: string, parentChannelId: string): Promise<ThreadInfo[]> {
  const res = await fetch(`https://discord.com/api/v10/guilds/${guildId}/threads/active`, {
    headers: { Authorization: `Bot ${token}`, "User-Agent": "maw-atlas/1.0.0" },
  });
  if (!res.ok) throw new Error(`Discord ${res.status} GET active threads`);
  const data = await res.json() as any;
  return (Array.isArray(data.threads) ? data.threads : [])
    .filter((thread: ThreadInfo) => thread.parent_id === parentChannelId)
    .map((thread: ThreadInfo) => ({ ...thread, guild_id: guildId }));
}

function snowflakeCompare(a: string, b: string): number {
  try {
    const aa = BigInt(a);
    const bb = BigInt(b);
    return aa < bb ? -1 : aa > bb ? 1 : 0;
  } catch {
    return a.localeCompare(b);
  }
}

async function waitForGo(token: string, threadId: string, afterId: string | undefined, timeoutMs: number, intervalMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const messages = await getMessages(token, threadId, 50);
    const hit = Array.isArray(messages) && messages.some((message: any) => {
      if (afterId && snowflakeCompare(message.id, afterId) <= 0) return false;
      if (message.author?.bot) return false;
      return String(message.content || "").trim().toLowerCase() === "go";
    });
    if (hit) return true;
    await sleep(intervalMs);
  }
  return false;
}

function shellQuote(argv: string[]): string {
  return argv.map(a => /[^A-Za-z0-9_./:=@-]/.test(a) ? JSON.stringify(a) : a).join(" ");
}

async function execText(argv: string[]): Promise<{ code: number; stdout: string; stderr: string }> {
  const bun = (globalThis as any).Bun;
  if (bun?.spawn) {
    const proc = bun.spawn(argv, { stdout: "pipe", stderr: "pipe" });
    const [code, stdout, stderr] = await Promise.all([
      proc.exited,
      proc.stdout ? new Response(proc.stdout).text() : Promise.resolve(""),
      proc.stderr ? new Response(proc.stderr).text() : Promise.resolve(""),
    ]);
    return { code, stdout, stderr };
  }

  return new Promise(resolvePromise => {
    execFile(argv[0], argv.slice(1), { encoding: "utf8", maxBuffer: 1024 * 1024 }, (err, stdout, stderr) => {
      resolvePromise({ code: err && typeof (err as any).code === "number" ? (err as any).code : (err ? 1 : 0), stdout, stderr });
    });
  });
}

function parsePane(output: string, fallback: string): string {
  const match = output.match(/(?:pane|target|delivered\s*→)\s*[:=]?\s*([A-Za-z0-9_.-]+:\d+|%\d+)/i)
    || output.match(/([A-Za-z0-9_.-]+:\d+|%\d+)/);
  return match?.[1] || fallback;
}

async function notify(target: string | null, message: string, log: Log) {
  if (!target) return;
  const result = await execText(["maw", "hey", target, message]);
  if (result.code !== 0) log(`notify failed: ${result.stderr || result.stdout}`.trim());
}

function updateRoutingTable(file: string, thread: ThreadInfo, pane: string, workerName: string) {
  const table = readJson<Record<string, any>>(file, {});
  table[thread.id] = { name: thread.name, pane, agent: workerName };
  writeJson(file, table);
}

function countWorktrees(root = invokeDir()): number {
  try {
    const entries = readdirSync(resolve(root, "agents"));
    return entries.filter(name => name.includes("codex") || name.includes("atlas")).length;
  } catch {
    return 0;
  }
}

async function handleNewThread(
  log: Log,
  token: string,
  thread: ThreadInfo,
  channelId: string,
  state: WatchState,
  guards: WatchGuards,
  args: string[],
) {
  const allowed = await allowFromGate(guards, thread, channelId, args);
  if (!allowed.ok) {
    log(`reject thread ${thread.name} (${thread.id}): ${allowed.reason || "allowFrom denied"}`);
    return;
  }

  const cap = await maxWorktreesGate(guards, state, args);
  if (!cap.ok) {
    log(`reject thread ${thread.name} (${thread.id}): ${cap.reason || "maxWorktrees cap"}`);
    await postMessage(token, thread.id, `cannot spawn codex: ${cap.reason || "max worktrees reached"}`);
    return;
  }

  const safeName = sanitizeThreadName(guards, thread.name);
  const branchName = sanitizeBranchName(guards, safeName);
  const workerName = argValue(args, "--worker-name") || `atlas-${branchName}`;
  const confirmation = await postMessage(token, thread.id, "spawn codex? reply go");
  state.pending[thread.id] = { confirmationId: confirmation?.id, requestedAt: new Date().toISOString(), workerName, branchName };
  writeJson(statePath(args), state);
  log(`confirmation posted: #${thread.name} (${thread.id}) waiting for go`);

  const go = await waitForGo(
    token,
    thread.id,
    confirmation?.id,
    intArg(args, "--confirm-timeout", DEFAULT_CONFIRM_TIMEOUT_MS),
    intArg(args, "--confirm-interval", 5_000),
  );
  if (!go) {
    log(`timeout waiting for go: #${thread.name} (${thread.id})`);
    return;
  }

  const wakeArgs = ["maw", "wake", workerName, "--branch", branchName, "--thread", thread.id];
  log(`go received; exec ${shellQuote(wakeArgs)}`);
  const wake = hasFlag(args, "--dry-run") ? { code: 0, stdout: `dry-run pane ${workerName}`, stderr: "" } : await execText(wakeArgs);
  if (wake.code !== 0) throw new Error(`maw wake failed: ${wake.stderr || wake.stdout}`.trim());

  const pane = parsePane(wake.stdout || wake.stderr, workerName);
  updateRoutingTable(routingPath(args), thread, pane, workerName);
  state.spawned[thread.id] = { workerName, pane, spawnedAt: new Date().toISOString() };
  delete state.pending[thread.id];
  writeJson(statePath(args), state);
  await postMessage(token, thread.id, `codex spawned: ${workerName} → ${pane}`);
  log(`spawned: ${thread.name} (${thread.id}) → ${workerName} / ${pane}`);
}

async function watchOnce(log: Log, token: string, channel: { id: string; guildId: string }, state: WatchState, guards: WatchGuards, args: string[]) {
  await emitBudgetAlert(guards, state, args, log);
  if (countWorktrees() > intArg(args, "--max-worktrees", 10)) {
    log("warning: worktree count exceeds maxWorktrees fallback cap");
  }

  const threads = await listActiveThreads(token, channel.guildId, channel.id);
  const newThreads = threads.filter(thread => !state.knownThreads[thread.id]);
  for (const thread of threads) {
    state.knownThreads[thread.id] = state.knownThreads[thread.id] || { name: thread.name, seenAt: new Date().toISOString() };
  }
  writeJson(statePath(args), state);

  for (const thread of newThreads) {
    await handleNewThread(log, token, thread, channel.id, state, guards, args);
  }

  return { active: threads.length, newThreads: newThreads.length };
}

function usage(log: Log) {
  log("usage:");
  log("  maw atlas watch <channel> [--once] [--dry-run]");
  log("");
  log("polls active threads every 10s; new thread → allowFrom gate → confirmation go → maw wake → routing update");
  log("options:");
  log("  --interval=N            poll interval ms (default 10000)");
  log("  --confirm-timeout=N     wait for go ms (default 600000)");
  log("  --access=path           access.json path");
  log("  --routing=path          thread-routing.json path");
  log("  --state=path            watch state path");
  log("  --max-worktrees=N       cap active workers (default 10)");
}

export async function watch(log: Log, token: string, args: string[]) {
  const channelInput = args[1];
  if (!channelInput || channelInput === "help" || channelInput === "-h" || channelInput === "--help") {
    usage(log);
    return;
  }
  if (!token) {
    log("✗ no DISCORD_BOT_TOKEN — set env or `pass insert discord/atlas-oracle-token`");
    return;
  }

  const channel = await resolveChannel(token, channelInput);
  if (!channel) {
    log(`✗ channel not found: ${channelInput}`);
    return;
  }

  const guards = watchGuards;
  const state = loadState(args);
  const notifyTarget = hasFlag(args, "--no-notify") ? null : (argValue(args, "--notify") || DEFAULT_NOTIFY_TARGET);
  log(`watching #${channel.name || channel.id} (${channel.id}) every ${intArg(args, "--interval", DEFAULT_INTERVAL_MS)}ms`);

  while (true) {
    try {
      const result = await watchOnce(log, token, channel, state, guards, args);
      log(`poll: ${result.active} active thread(s), ${result.newThreads} new`);
      if (result.newThreads) await notify(notifyTarget, `[m5:atlas] watch saw ${result.newThreads} new thread(s) in ${channel.name || channel.id}`, log);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      log(`watch error: ${msg}`);
      await notify(notifyTarget, `[m5:atlas] watch error in ${channel.name || channel.id}: ${msg}`, log);
    }

    if (hasFlag(args, "--once")) return;
    await sleep(intArg(args, "--interval", DEFAULT_INTERVAL_MS));
  }
}
