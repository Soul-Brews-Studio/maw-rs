/**
 * maw atlas spawn-session — Tier 1 shortcut for starting an Atlas team session.
 *
 * Sequentially runs:
 *   1. maw team up <charter>
 *   2. maw atlas team-threads sync
 *   3. maw atlas route start
 */

type Log = (s: string) => void;

type Step = {
  label: string;
  argv: string[];
};

const DEFAULT_NOTIFY_TARGET = "01-atlas:1";

function hasFlag(args: string[], name: string): boolean {
  return args.includes(name);
}

function argValue(args: string[], name: string): string | undefined {
  const exact = args.find(a => a.startsWith(`${name}=`));
  if (exact) return exact.slice(name.length + 1);
  const idx = args.indexOf(name);
  return idx >= 0 ? args[idx + 1] : undefined;
}

function shellQuote(argv: string[]): string {
  return argv.map(a => /[^A-Za-z0-9_./:=@-]/.test(a) ? JSON.stringify(a) : a).join(" ");
}

async function spawnText(argv: string[]): Promise<{ code: number; stdout: string; stderr: string }> {
  const bun = (globalThis as any).Bun;
  if (!bun?.spawn) throw new Error("Bun.spawn is required for maw atlas spawn-session");

  const proc = bun.spawn(argv, { stdout: "pipe", stderr: "pipe" });
  const [code, stdout, stderr] = await Promise.all([
    proc.exited,
    proc.stdout ? new Response(proc.stdout).text() : Promise.resolve(""),
    proc.stderr ? new Response(proc.stderr).text() : Promise.resolve(""),
  ]);
  return { code, stdout, stderr };
}

async function notify(target: string | null, message: string, dryRun: boolean, log: Log) {
  if (!target) return;
  const argv = ["maw", "hey", target, message];
  if (dryRun) {
    log(`dry-run notify: ${shellQuote(argv)}`);
    return;
  }

  const result = await spawnText(argv);
  if (result.code !== 0) {
    log(`notify failed (${result.code}): ${result.stderr || result.stdout}`.trim());
  }
}

async function runStep(step: Step, dryRun: boolean, notifyTarget: string | null, log: Log) {
  log(`→ ${step.label}: ${shellQuote(step.argv)}`);
  if (dryRun) {
    log(`dry-run: skipped ${step.label}`);
    await notify(notifyTarget, `[m5:atlas] spawn-session dry-run step ${step.label}: ${shellQuote(step.argv)}`, dryRun, log);
    return;
  }

  const result = await spawnText(step.argv);
  if (result.stdout.trim()) log(result.stdout.trim());
  if (result.stderr.trim()) log(result.stderr.trim());
  if (result.code !== 0) {
    await notify(notifyTarget, `[m5:atlas] spawn-session step failed: ${step.label} exited ${result.code}`, false, log);
    throw new Error(`${step.label} failed with exit code ${result.code}`);
  }

  log(`✓ ${step.label} complete`);
  await notify(notifyTarget, `[m5:atlas] spawn-session step complete: ${step.label}`, false, log);
}

function usage(log: Log) {
  log("usage:");
  log("  maw atlas spawn-session <charter> [--dry-run] [--notify=01-atlas:1|--no-notify]");
  log("");
  log("runs sequentially:");
  log("  maw team up <charter>");
  log("  maw atlas team-threads sync");
  log("  maw atlas route start");
}

export async function spawnSession(log: Log, args: string[]) {
  const charter = args[1];
  if (!charter || charter === "help" || charter === "-h" || charter === "--help") {
    usage(log);
    return;
  }

  const dryRun = hasFlag(args, "--dry-run");
  const notifyTarget = hasFlag(args, "--no-notify") ? null : (argValue(args, "--notify") || DEFAULT_NOTIFY_TARGET);
  const steps: Step[] = [
    { label: "team up", argv: ["maw", "team", "up", charter] },
    { label: "team threads sync", argv: ["maw", "atlas", "team-threads", "sync"] },
    { label: "route start", argv: ["maw", "atlas", "route", "start"] },
  ];

  log(`maw atlas spawn-session ${charter}${dryRun ? " (dry-run)" : ""}`);
  for (const step of steps) {
    await runStep(step, dryRun, notifyTarget, log);
  }
  log("✓ spawn-session complete");
  await notify(notifyTarget, `[m5:atlas] spawn-session complete: ${charter}`, dryRun, log);
}
