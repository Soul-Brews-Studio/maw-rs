import { lifecycle } from "./launchagent";

type Log = (line: string) => void;

declare const process: { argv: string[]; exit(code: number): never };

type InvokeContext = {
  source?: string;
  args?: string[];
  writer?: (line: string) => void;
};

type InvokeResult = {
  ok: boolean;
  output: string;
  error?: string;
  exitCode: number;
};

export const command = {
  name: "menubar",
  description: "Native macOS menu bar companion for local maw fleet status.",
};

export const LIFECYCLE_COMMANDS = [
  "install",
  "start",
  "stop",
  "status",
  "uninstall",
  "run",
] as const;

export type LifecycleCommand = typeof LIFECYCLE_COMMANDS[number];

function usage(log: Log): void {
  log("maw menubar - native macOS fleet status companion");
  log("");
  log("Usage:");
  log("  maw menubar install");
  log("  maw menubar start|stop|status|uninstall");
  log("  maw menubar run  # private LaunchAgent entrypoint");
}

function isLifecycleCommand(value: string): value is LifecycleCommand {
  return (LIFECYCLE_COMMANDS as readonly string[]).includes(value);
}

export async function handleMenubar(
  args: string[], log: Log, runLifecycle = lifecycle,
): Promise<number> {
  const subcommand = args[0] || "status";
  if (["help", "-h", "--help"].includes(subcommand)) {
    usage(log);
    return 0;
  }
  if (!isLifecycleCommand(subcommand)) {
    log(`Unknown subcommand: ${subcommand}`);
    usage(log);
    return 1;
  }
  if (args.length > 1) {
    log(`maw menubar ${subcommand}: unexpected arguments`);
    return 1;
  }
  return runLifecycle(subcommand, log);
}

export default async function handler(ctx: InvokeContext): Promise<InvokeResult> {
  const lines: string[] = [];
  const log = (line: string) => (ctx.writer ? ctx.writer(line) : lines.push(line));
  const args = ctx.source === "cli" || !ctx.source ? (ctx.args || []) : [];
  const exitCode = await handleMenubar(args, log);
  const output = ctx.writer ? "" : lines.join("\n");
  return { ok: exitCode === 0, output, error: exitCode === 0 ? undefined : output, exitCode };
}

if ((import.meta as ImportMeta & { main?: boolean }).main) {
  const lines: string[] = [];
  const exitCode = await handleMenubar(process.argv.slice(2), (line) => lines.push(line));
  if (lines.length) console.log(lines.join("\n"));
  process.exit(exitCode);
}
