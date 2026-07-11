import { Memory } from "@extism/as-pdk";
import { length } from "@extism/as-pdk/lib/env";

@external("extism:host/user", "maw.config.get") declare function mawConfigGet(input: u64): u64;
@external("extism:host/user", "maw.time.now") declare function mawTimeNow(input: u64): u64;
@external("extism:host/user", "maw.config.set") declare function mawConfigSet(input: u64): u64;
@external("extism:host/user", "maw.consent.read") declare function mawConsentRead(input: u64): u64;
@external("extism:host/user", "maw.state.get") declare function mawStateGet(input: u64): u64;
@external("extism:host/user", "maw.state.set") declare function mawStateSet(input: u64): u64;
@external("extism:host/user", "maw.fs.read") declare function mawFsRead(input: u64): u64;
@external("extism:host/user", "maw.fs.write") declare function mawFsWrite(input: u64): u64;
@external("extism:host/user", "maw.fs.remove") declare function mawFsRemove(input: u64): u64;
@external("extism:host/user", "maw.fs.list") declare function mawFsList(input: u64): u64;
@external("extism:host/user", "maw.fs.stat") declare function mawFsStat(input: u64): u64;
@external("extism:host/user", "maw.exec.run") declare function mawExecRun(input: u64): u64;
@external("extism:host/user", "maw.http.request") declare function mawHttpRequest(input: u64): u64;
@external("extism:host/user", "maw.localserver.request") declare function mawLocalserverRequest(input: u64): u64;
@external("extism:host/user", "maw.tmux.list_sessions") declare function mawTmuxListSessions(input: u64): u64;
@external("extism:host/user", "maw.tmux.capture") declare function mawTmuxCapture(input: u64): u64;
@external("extism:host/user", "maw.tmux.send_keys") declare function mawTmuxSendKeys(input: u64): u64;
@external("extism:host/user", "maw.tmux.command") declare function mawTmuxCommand(input: u64): u64;
@external("extism:host/user", "maw.tmux.tags_read") declare function mawTmuxTagsRead(input: u64): u64;
@external("extism:host/user", "maw.tmux.tags_write") declare function mawTmuxTagsWrite(input: u64): u64;
@external("extism:host/user", "maw.http.peer_send") declare function mawHttpPeerSend(input: u64): u64;
@external("extism:host/user", "maw.http.peer_wake") declare function mawHttpPeerWake(input: u64): u64;
@external("extism:host/user", "maw.ssh.exec") declare function mawSshExec(input: u64): u64;

export class UserError extends Error {
  code: string;
  constructor(message: string, code: string = "user_error") {
    super(message);
    this.name = "UserError";
    this.code = code;
  }
}

export class EngineDef {
  name: string;
  command: string;
  constructor(name: string, command: string) {
    this.name = name;
    this.command = command;
  }
}

export const DEFAULT_ENGINES = [
  new EngineDef("codex", "codex"),
  new EngineDef("claude", "claude"),
  new EngineDef("gemini", "gemini")
];
export const TTL_MS: i64 = 86_400_000;
export const FLEET_DIR = "fleet";
export const CONFIG_DIR = "config";
export const MAW_ROOT = ".maw";
export const CONFIG_FILE = "maw.config.json";

export function C(value: string): string { return value; }
export function tlink(target: string): string { return "tmux://" + target; }
export function isUserError(error: Error): bool { return error.name == "UserError"; }

export function validateNickname(name: string): bool {
  if (name.length == 0 || name.length > 32) return false;
  for (let i = 0; i < name.length; i++) {
    const c = name.charCodeAt(i);
    if (!isAlphaNum(c) && c != 45 && c != 95) return false;
  }
  return true;
}

export function assertValidOracleName(name: string): void {
  if (!validateNickname(name)) throw new UserError("invalid oracle name: " + name, "invalid_oracle_name");
}

export function normalizeTarget(target: string): string { return target.trim().toLowerCase(); }
export function isInfrastructureChannelSessionName(name: string): bool {
  return name == "bigboy" || name == "gmtk" || name.startsWith("infra-") || name.endsWith("-vps");
}

export function defaultEngineNameForConfig(_config: string = ""): string { return DEFAULT_ENGINES[0].name; }
export function resolveEngine(name: string): EngineDef {
  for (let i = 0; i < DEFAULT_ENGINES.length; i++) if (DEFAULT_ENGINES[i].name == name) return DEFAULT_ENGINES[i];
  return new EngineDef(name, name);
}

export function buildCommand(command: string, args: string[] = []): string {
  return [command].concat(args).join(" ");
}

export function buildCommandInDir(dir: string, command: string, args: string[] = []): string {
  return "cd " + shellQuote(dir) + " && " + buildCommand(command, args);
}

export function parseFlags(args: string[]): Map<string, string> {
  const flags = new Map<string, string>();
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (!arg.startsWith("--")) continue;
    const eq = arg.indexOf("=");
    if (eq >= 0) flags.set(arg.slice(2, eq), arg.slice(eq + 1));
    else flags.set(arg.slice(2), "true");
  }
  return flags;
}

export function sparkline(values: i32[]): string {
  if (values.length == 0) return "";
  const ticks = "12345678";
  let min = values[0];
  let max = values[0];
  for (let i = 1; i < values.length; i++) { if (values[i] < min) min = values[i]; if (values[i] > max) max = values[i]; }
  let out = "";
  const span = max - min;
  for (let i = 0; i < values.length; i++) {
    const idx = span == 0 ? 0 : ((values[i] - min) * 7) / span;
    out += ticks.charAt(idx);
  }
  return out;
}

export function agentProcessNames(): string[] { return ["codex", "claude", "gemini", "node", "bun"]; }
export function matchesAgentProcessName(command: string): bool {
  const lower = command.toLowerCase();
  const names = agentProcessNames();
  for (let i = 0; i < names.length; i++) if (lower.includes(names[i])) return true;
  return false;
}
export function isAgentCommand(command: string): bool { return matchesAgentProcessName(command); }
export function engineIdlePromptPatterns(): string[] { return [">", "❯", "Human:", "User:"]; }
export function matchesEngineIdlePrompt(text: string): bool {
  const patterns = engineIdlePromptPatterns();
  for (let i = 0; i < patterns.length; i++) if (text.endsWith(patterns[i])) return true;
  return false;
}
export function extractOracleName(session: string): string { return session.split(":")[0]; }

export function definePlugin(manifestJson: string): string { return manifestJson; }

export function loadConfig(argsJson: string = "{}"): string { return call(mawConfigGet, argsJson); }
export function cfg(key: string): string { return call(mawConfigGet, "{\"key\":" + quote(key) + "}"); }
export function saveConfig(argsJson: string): string { return call(mawConfigSet, argsJson); }
export function resetConfig(argsJson: string = "{}"): string { return call(mawConfigSet, argsJson); }
export function readConsent(argsJson: string): string { return call(mawConsentRead, argsJson); }
export function getEnvVars(argsJson: string = "{}"): string { return call(mawConfigGet, argsJson); }
export function timeNow(argsJson: string = "{}"): string { return call(mawTimeNow, argsJson); }
export function loadPending(argsJson: string): string { return call(mawStateGet, argsJson); }
export function savePending(argsJson: string): string { return call(mawStateSet, argsJson); }
export function readAudit(argsJson: string): string { return call(mawStateGet, argsJson); }
export function logAudit(argsJson: string): string { return call(mawStateSet, argsJson); }
export function fsRead(argsJson: string): string { return call(mawFsRead, argsJson); }
export function fsWrite(argsJson: string): string { return call(mawFsWrite, argsJson); }
export function fsRemove(argsJson: string): string { return call(mawFsRemove, argsJson); }
export function fsList(argsJson: string): string { return call(mawFsList, argsJson); }
export function fsStat(argsJson: string): string { return call(mawFsStat, argsJson); }
export function hostExec(argsJson: string): string { return call(mawExecRun, argsJson); }
export function curlFetch(argsJson: string): string { return call(mawHttpRequest, argsJson); }
export function localserverRequest(argsJson: string): string { return call(mawLocalserverRequest, argsJson); }
export function listSessions(argsJson: string = "{}"): string { return call(mawTmuxListSessions, argsJson); }
export function capture(argsJson: string): string { return call(mawTmuxCapture, argsJson); }
export function sendKeys(argsJson: string): string { return call(mawTmuxSendKeys, argsJson); }
export function tmuxCommand(argsJson: string): string { return call(mawTmuxCommand, argsJson); }
export function readPaneTags(argsJson: string): string { return call(mawTmuxTagsRead, argsJson); }
export function tagPane(argsJson: string): string { return call(mawTmuxTagsWrite, argsJson); }
export function cmdSend(argsJson: string): string { return call(mawHttpPeerSend, argsJson); }
export function cmdWake(argsJson: string): string { return call(mawHttpPeerWake, argsJson); }
export function sshExec(argsJson: string): string { return call(mawSshExec, argsJson); }
export function attachRemoteSession(_argsJson: string): string { return "{\"ok\":false,\"code\":\"unsupported\",\"error\":\"interactive attach is unsupported in WASM\"}"; }

function call(fn: (input: u64) => u64, argsJson: string): string {
  const input = Memory.allocateString(argsJson);
  const output = fn(input.offset);
  const outputLength = length(output);
  return new Memory(output, outputLength).toString();
}

function shellQuote(value: string): string { return "'" + value.replace("'", "'\\''") + "'"; }
function quote(value: string): string { return "\"" + value.replace("\\", "\\\\").replace("\"", "\\\"") + "\""; }
function isAlphaNum(c: i32): bool { return (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122); }
