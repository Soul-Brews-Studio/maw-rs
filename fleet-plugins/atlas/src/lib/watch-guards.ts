/** Security and cleanup guards for `maw atlas watch`. */
import { existsSync, readFileSync, readdirSync, rmSync } from "fs";
import { basename, resolve } from "path";
import { execFile } from "child_process";
import { invokeDir } from "./repo";

export type GuardDecision = {
  ok: boolean;
  reason?: string;
  warning?: string;
};

export type AllowFromInput = {
  userId?: string;
  channelId?: string;
  thread?: { id?: string; owner_id?: string; parent_id?: string };
  accessPath: string;
};

export type MaxWorktreesInput = {
  activeWorkers?: number;
  maxWorktrees?: number;
  worktreeRoot?: string;
  state?: { spawned?: Record<string, unknown> };
};

export type CleanupRoute = {
  threadId: string;
  workerName?: string;
  pane?: string;
  worktreePath?: string;
};

export type CleanupInput = {
  routes: CleanupRoute[];
  activeThreads: Array<{ id: string; thread_metadata?: { archived?: boolean }; archived?: boolean }>;
  dryRun?: boolean;
  worktreeRoot?: string;
  log?: (s: string) => void;
};

export type CleanupResult = {
  checked: number;
  cleaned: number;
  skipped: number;
  errors: Array<{ route: CleanupRoute; error: string }>;
};

function readJson<T>(file: string, fallback: T): T {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch { return fallback; }
}

function decision(ok: boolean, reason?: string, warning?: string): GuardDecision {
  return { ok, reason, warning };
}

export function sanitizeInput(value: string, maxLength = 50): string {
  return String(value || "")
    .replace(/[\u0000-\u001f\u007f-\u009f]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, maxLength);
}

export function sanitizeThreadName(name: string, maxLength = 50): string {
  return sanitizeInput(name, maxLength) || "atlas-thread";
}

export function sanitizeBranchName(name: string): string {
  const safe = sanitizeThreadName(name, 50)
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[^a-z0-9._/-]+/g, "-")
    .replace(/\.\.+/g, ".")
    .replace(/^[-/.]+|[-/.]+$/g, "")
    .replace(/[-/]{2,}/g, "-")
    .slice(0, 50);
  return safe || `thread-${Date.now()}`;
}

function allowedUsers(access: any, channelId?: string): Set<string> {
  const allow = new Set<string>();
  if (Array.isArray(access.allowFrom)) for (const id of access.allowFrom) allow.add(String(id));
  if (channelId && Array.isArray(access.groups?.[channelId]?.allowFrom)) {
    for (const id of access.groups[channelId].allowFrom) allow.add(String(id));
  }
  return allow;
}

export function allowFromGate(input: AllowFromInput): GuardDecision {
  const userId = input.userId || input.thread?.owner_id;
  if (!userId) return decision(false, "thread creator user ID missing");
  const access = readJson<any>(input.accessPath, {});
  const allowed = allowedUsers(access, input.channelId || input.thread?.parent_id);
  if (allowed.size === 0) return decision(false, "allowFrom is empty");
  return allowed.has(String(userId)) ? decision(true) : decision(false, `user ${userId} not in allowFrom`);
}

export function checkAllowFrom(input: AllowFromInput): GuardDecision {
  return allowFromGate(input);
}

function countWorktreeDirs(root: string): number {
  try {
    const agentsDir = resolve(root, "agents");
    return readdirSync(agentsDir, { withFileTypes: true })
      .filter(entry => entry.isDirectory())
      .filter(entry => /codex|atlas/i.test(entry.name))
      .length;
  } catch {
    return 0;
  }
}

export function maxWorktreesGate(input: MaxWorktreesInput = {}): GuardDecision {
  const max = input.maxWorktrees ?? 10;
  const stateCount = input.state?.spawned ? Object.keys(input.state.spawned).length : 0;
  const activeWorkers = input.activeWorkers ?? Math.max(stateCount, countWorktreeDirs(input.worktreeRoot || invokeDir()));
  if (activeWorkers >= max) return decision(false, `active workers ${activeWorkers} >= maxWorktrees ${max}`);
  return decision(true);
}

export function checkMaxWorktrees(input: MaxWorktreesInput = {}): GuardDecision {
  return maxWorktreesGate(input);
}

export function budgetAlert(input: { activeWorkers?: number; threshold?: number; state?: { spawned?: Record<string, unknown> } } = {}): string | void {
  const threshold = input.threshold ?? 8;
  const activeWorkers = input.activeWorkers ?? (input.state?.spawned ? Object.keys(input.state.spawned).length : 0);
  if (activeWorkers > threshold) return `warning: active workers ${activeWorkers} > budget threshold ${threshold}`;
}

async function execText(argv: string[], dryRun: boolean): Promise<{ code: number; stdout: string; stderr: string }> {
  if (dryRun) return { code: 0, stdout: `dry-run: ${argv.join(" ")}`, stderr: "" };
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

function routeWorktreePath(route: CleanupRoute, root: string): string | null {
  if (route.worktreePath) return route.worktreePath;
  if (route.workerName) return resolve(root, "agents", `1-${sanitizeBranchName(route.workerName)}`);
  if (route.pane && !route.pane.includes(":")) return resolve(root, "agents", `1-${sanitizeBranchName(route.pane)}`);
  return null;
}

export async function cleanupArchivedThreads(input: CleanupInput): Promise<CleanupResult> {
  const active = new Map(input.activeThreads.map(thread => [thread.id, thread]));
  const result: CleanupResult = { checked: input.routes.length, cleaned: 0, skipped: 0, errors: [] };
  const root = input.worktreeRoot || invokeDir();

  for (const route of input.routes) {
    const thread = active.get(route.threadId);
    const archived = !thread || thread.archived || thread.thread_metadata?.archived;
    if (!archived) { result.skipped++; continue; }

    try {
      const target = route.workerName || route.pane;
      if (target) {
        const kill = await execText(["maw", "kill", target], !!input.dryRun);
        if (kill.code !== 0) throw new Error(kill.stderr || kill.stdout || `maw kill ${target} failed`);
        input.log?.(`cleanup kill: ${target}`);
      }

      const worktree = routeWorktreePath(route, root);
      if (worktree && (input.dryRun || existsSync(worktree))) {
        const rm = await execText(["git", "worktree", "remove", "--force", worktree], !!input.dryRun);
        if (rm.code !== 0) throw new Error(rm.stderr || rm.stdout || `git worktree remove ${worktree} failed`);
        input.log?.(`cleanup worktree: ${basename(worktree)}`);
      }
      result.cleaned++;
    } catch (e) {
      result.errors.push({ route, error: e instanceof Error ? e.message : String(e) });
    }
  }

  return result;
}

export async function autoCleanup(input: CleanupInput): Promise<CleanupResult> {
  return cleanupArchivedThreads(input);
}
