/**
 * Reverse bridge for maw atlas route.
 *
 * Codex pane output → Discord thread:
 *   1. `maw peek <pane>` via Bun.spawn
 *   2. compare against the previous snapshot
 *   3. post changed output back to the mapped Discord thread
 */
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "fs";
import { dirname, resolve } from "path";
import { execFile } from "child_process";
import { postMessage } from "./discord";
import { resolveFromInvoke } from "./repo";

type Log = (line: string) => void;

export type ReverseRouteEntry = {
  name?: string;
  pane: string;
  agent?: string;
};

export type ReverseRouteTable = Record<string, ReverseRouteEntry>;

export type ReverseBridgeSnapshot = {
  panes: Record<string, string | undefined>;
  lastPostedAt?: Record<string, string | undefined>;
};

export type ReverseBridgeOptions = {
  routingPath?: string;
  snapshotPath?: string;
  maxContentLength?: number;
  dryRun?: boolean;
  /** Post full pane content the first time a pane has no snapshot. Default: false (seed only). */
  replayInitial?: boolean;
};

export type ReverseBridgeResult = {
  checked: number;
  changed: number;
  posted: number;
};

const DEFAULT_ROUTING_TABLE = "/opt/Code/github.com/Soul-Brews-Studio/atlas-oracle/.discord/thread-routing.json";
const DEFAULT_SNAPSHOT_FILE = ".maw/atlas-route/reverse-snapshots.json";
const DISCORD_LIMIT = 2000;

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

export function loadReverseRouteTable(file = DEFAULT_ROUTING_TABLE): ReverseRouteTable {
  const raw = readJson<Record<string, any>>(file, {});
  const table: ReverseRouteTable = {};

  for (const [threadId, route] of Object.entries(raw)) {
    if (!/^\d{17,20}$/.test(threadId)) continue;
    if (!route || typeof route !== "object" || typeof route.pane !== "string" || !route.pane.trim()) continue;
    table[threadId] = {
      name: typeof route.name === "string" ? route.name : undefined,
      pane: route.pane.trim(),
      agent: typeof route.agent === "string" ? route.agent : undefined,
    };
  }

  return table;
}

function loadSnapshot(file: string): ReverseBridgeSnapshot {
  const snapshot = readJson<ReverseBridgeSnapshot>(file, { panes: {}, lastPostedAt: {} });
  return {
    panes: snapshot?.panes && typeof snapshot.panes === "object" ? snapshot.panes : {},
    lastPostedAt: snapshot?.lastPostedAt && typeof snapshot.lastPostedAt === "object" ? snapshot.lastPostedAt : {},
  };
}

async function streamToText(stream: any): Promise<string> {
  if (!stream) return "";
  return new Response(stream).text();
}

async function execPeekFallback(pane: string): Promise<string> {
  return new Promise((resolvePromise, reject) => {
    execFile("maw", ["peek", pane], { encoding: "utf8", maxBuffer: 2 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err) {
        reject(new Error([err.message, stderr, stdout].filter(Boolean).join("\n")));
        return;
      }
      resolvePromise(stdout || "");
    });
  });
}

export async function peekPane(pane: string): Promise<string> {
  const bun = (globalThis as any).Bun;
  if (!bun?.spawn) return execPeekFallback(pane);

  const proc = bun.spawn(["maw", "peek", pane], {
    stdout: "pipe",
    stderr: "pipe",
  });
  const [code, stdout, stderr] = await Promise.all([
    proc.exited,
    streamToText(proc.stdout),
    streamToText(proc.stderr),
  ]);

  if (code !== 0) throw new Error([`maw peek ${pane} exited ${code}`, stderr].filter(Boolean).join("\n"));
  return stdout;
}

function commonPrefixLength(a: string, b: string): number {
  const max = Math.min(a.length, b.length);
  let i = 0;
  while (i < max && a.charCodeAt(i) === b.charCodeAt(i)) i++;
  return i;
}

export function diffOutput(previous: string | undefined, current: string): string | null {
  if (previous === current) return null;
  if (!previous) return current.trim() || null;
  if (current.startsWith(previous)) return current.slice(previous.length).trim() || null;

  const prefix = commonPrefixLength(previous, current);
  const changed = current.slice(prefix).trim();
  return changed || current.trim() || null;
}

function truncateDiscord(content: string, maxContentLength: number): string {
  const max = Math.min(maxContentLength, DISCORD_LIMIT);
  if (content.length <= max) return content;
  const marker = `\n… truncated ${content.length - max} chars`;
  return content.slice(0, Math.max(0, max - marker.length)) + marker;
}

function formatDiscordContent(route: ReverseRouteEntry, pane: string, diff: string, maxContentLength: number): string {
  const label = route.agent || route.name || pane;
  const header = `↩️ **${label}** (${pane}) output:`;
  const fenced = `${header}\n\`\`\`\n${diff}\n\`\`\``;
  if (fenced.length <= maxContentLength && fenced.length <= DISCORD_LIMIT) return fenced;
  return truncateDiscord(`${header}\n${diff}`, maxContentLength);
}

export async function runReverseBridgeOnce(
  log: Log,
  token: string,
  table: ReverseRouteTable,
  options: ReverseBridgeOptions = {},
): Promise<ReverseBridgeResult> {
  const snapshotFile = resolveFromInvoke(options.snapshotPath || DEFAULT_SNAPSHOT_FILE);
  const maxContentLength = options.maxContentLength || 1900;
  const snapshot = loadSnapshot(snapshotFile);
  let checked = 0;
  let changed = 0;
  let posted = 0;

  for (const [threadId, route] of Object.entries(table)) {
    checked++;
    const previous = snapshot.panes[route.pane];
    const current = await peekPane(route.pane);
    const diff = diffOutput(previous, current);
    snapshot.panes[route.pane] = current;

    if (!diff) continue;
    changed++;

    if (!previous && !options.replayInitial) {
      log(`seeded: ${route.pane} → ${threadId} (initial snapshot, not posted)`);
      continue;
    }

    const content = formatDiscordContent(route, route.pane, diff, maxContentLength);
    if (options.dryRun) {
      log(`dry-run reverse post: ${route.pane} → ${threadId} (${content.length} chars)`);
    } else {
      await postMessage(token, threadId, content);
      snapshot.lastPostedAt![threadId] = new Date().toISOString();
      log(`reverse posted: ${route.pane} → ${threadId} (${content.length} chars)`);
    }
    posted++;
  }

  writeJson(snapshotFile, snapshot);
  return { checked, changed, posted };
}

export async function runReverseBridgeFromRouting(
  log: Log,
  token: string,
  options: ReverseBridgeOptions = {},
): Promise<ReverseBridgeResult> {
  const routingFile = options.routingPath ? resolveFromInvoke(options.routingPath) : DEFAULT_ROUTING_TABLE;
  if (!existsSync(routingFile)) throw new Error(`routing table not found: ${routingFile}`);
  const table = loadReverseRouteTable(routingFile);
  return runReverseBridgeOnce(log, token, table, options);
}
