// Read-only slice of maw-js@746df172 src/vendor/mpr-plugins/team.
import { Host, Memory } from "@extism/as-pdk";
import { length } from "@extism/as-pdk/lib/env";
import { fsRead, fsList, tmuxCommand } from "@maw-rs/wasm-sdk";

@external("extism:host/user", "maw.paths.get") declare function mawPathsGet(input: u64): u64;
export function myAbort(message: string | null, fileName: string | null, lineNumber: u32, columnNumber: u32): void {}

class Parsed { value: string = ""; next: i32 = 0; }
class Team { name: string = ""; members: i32 = 0; alive: i32 = 0; exited: i32 = 0; }

export function handle(): i32 {
  const args = extractArgs(Host.inputString());
  const sub = args.length == 0 ? "list" : args[0].toLowerCase();
  if (sub != "list" && sub != "ls") return finish(false, "", "team slice 1 implements only list/ls");
  const teams = pathGet("teams"); const vault = pathGet("vault");
  if (teams == "" || vault == "") return finish(false, "", "team inventory roots unavailable");
  return finish(true, render(teams, vault), "");
}

function render(teamsRoot: string, vaultRoot: string): string {
  const live = livePaneIds(); const tool = new Array<Team>(); const names = new Array<string>();
  const dirs = listPaths(teamsRoot, "dir");
  for (let i = 0; i < dirs.length; i++) {
    const raw = readFile(dirs[i] + "/config.json"); if (raw == "" || raw.indexOf("\"members\"") < 0) continue;
    const team = parseTool(baseName(dirs[i]), raw, live); tool.push(team); names.push(team.name);
  }
  const vault = new Array<Team>(); const vaultDirs = listPaths(vaultRoot + "/memory/mailbox/teams", "dir");
  for (let i = 0; i < vaultDirs.length; i++) {
    const name = baseName(vaultDirs[i]); if (contains(names, name)) continue;
    const raw = readFile(vaultDirs[i] + "/manifest.json"); if (raw == "" || raw.indexOf("\"members\"") < 0) continue;
    const team = new Team(); team.name = name; team.members = memberCount(raw); vault.push(team);
  }
  if (tool.length == 0 && vault.length == 0)
    return "\u001b[90mNo teams found.\u001b[0m\n\u001b[90m  looked in: ~/.claude/teams/ (tool) + ψ/memory/mailbox/teams/ (vault)\u001b[0m";
  let out = "\n  \u001b[36;1mTEAM" + spaces(26) + "STORE  MEMBERS  STATUS          ZOMBIES\u001b[0m\n";
  for (let i = 0; i < tool.length; i++) {
    const t = tool[i]; const status = t.alive > 0 ? "\u001b[32m" + t.alive.toString() + " alive\u001b[0m" : "\u001b[90mno live panes\u001b[0m";
    const dead = t.exited > 0 ? "\u001b[90m" + t.exited.toString() + " exited\u001b[0m" : "0";
    out += "  " + pad(t.name, 30) + pad("tool", 7) + pad(t.members.toString(), 9) + pad(status, 26) + dead + "\n";
  }
  for (let i = 0; i < vault.length; i++) {
    const t = vault[i]; out += "  " + pad(t.name, 30) + pad("vault", 7) + pad(t.members.toString(), 9) + pad("\u001b[90mprep-only\u001b[0m", 26) + "\u001b[90m—\u001b[0m\n";
  }
  if (vault.length > 0) out += "\n  \u001b[90m" + vault.length.toString() + " vault-only team(s) — resume via \u001b[36mmaw team resume <name>\u001b[90m or remove via \u001b[36mrm -rf ψ/memory/mailbox/teams/<name>/\u001b[0m\n";
  return out;
}

function parseTool(name: string, raw: string, live: string[]): Team {
  const t = new Team(); t.name = name; const objects = arrayObjects(raw, "members");
  for (let i = 0; i < objects.length; i++) {
    const lead = field(objects[i], "agentType") == "team-lead"; const pane = field(objects[i], "tmuxPaneId");
    if (!lead) t.members++; if (pane != "" && pane != "in-process") { if (contains(live, pane)) { if (!lead) t.alive++; } else if (!lead) t.exited++; }
  }
  return t;
}

function livePaneIds(): string[] {
  const res = tmuxCommand("{\"command\":\"list-panes\",\"args\":[\"-a\",\"-F\",\"#{pane_id}\"]}");
  const stdout = field(res, "stdout"); const out = new Array<string>(); let i = 0;
  while (i < stdout.length) { let e = stdout.indexOf("\n", i); if (e < 0) e = stdout.length; const v = stdout.slice(i, e); if (v != "") out.push(v); i = e + 1; }
  return out;
}
function listPaths(path: string, kind: string): string[] {
  const res = fsList("{\"path\":" + quote(path) + ",\"recursive\":false,\"includeDirs\":true}"); const out = new Array<string>(); let i = 0;
  while (true) { const p = res.indexOf("\"path\":", i); if (p < 0) break; let s = p; while (s > 0 && res.charAt(s) != "{") s--; let e = p; while (e < res.length && res.charAt(e) != "}") e++; const item = res.slice(s, e + 1); if (field(item, "kind") == kind) out.push(field(item, "path")); i = e + 1; }
  sort(out); return out;
}
function arrayObjects(raw: string, key: string): string[] { const out = new Array<string>(); const at = raw.indexOf("\"" + key + "\""); if (at < 0) return out; const start = raw.indexOf("[", at); if (start < 0) return out; let depth = 0; let begin = -1; let quoted = false; for (let i = start + 1; i < raw.length; i++) { const ch = raw.charAt(i); if (ch == "\"" && raw.charAt(i - 1) != "\\") quoted = !quoted; if (quoted) continue; if (ch == "{") { if (depth == 0) begin = i; depth++; } else if (ch == "}") { depth--; if (depth == 0 && begin >= 0) out.push(raw.slice(begin, i + 1)); } else if (ch == "]" && depth == 0) break; } return out; }
function memberCount(raw: string): i32 { const objects = arrayObjects(raw, "members"); let count = objects.length; const at = raw.indexOf("\"members\""); if (at < 0) return 0; const start = raw.indexOf("[", at); const end = raw.indexOf("]", start); if (start < 0 || end < 0) return count; const body = raw.slice(start + 1, end); let quoted = false; let depth = 0; for (let i = 0; i < body.length; i++) { const ch = body.charAt(i); if (ch == "{" && !quoted) depth++; else if (ch == "}" && !quoted) depth--; else if (ch == "\"" && body.charAt(i - 1) != "\\") { if (!quoted && depth == 0) count++; quoted = !quoted; } } return count; }
function readFile(path: string): string { const res = fsRead("{\"path\":" + quote(path) + ",\"encoding\":\"utf8\"}"); return res.indexOf("\"ok\":true") < 0 ? "" : field(res, "content"); }
function pathGet(name: string): string { const p = Memory.allocateString("{\"name\":" + quote(name) + "}"); const out = mawPathsGet(p.offset); const res = new Memory(out, length(out)).toString(); return res.indexOf("\"ok\":true") < 0 ? "" : field(res, "path"); }
function extractArgs(raw: string): string[] { const out = new Array<string>(); const at = raw.indexOf("\"args\":["); if (at < 0) return out; let i = at + 8; while (i < raw.length && raw.charAt(i) != "]") { if (raw.charAt(i) == "\"") { const p = readString(raw, i); out.push(p.value); i = p.next; } else i++; } return out; }
function field(raw: string, key: string): string { const at = raw.indexOf("\"" + key + "\":"); if (at < 0) return ""; let i = raw.indexOf("\"", at + key.length + 3); return i < 0 ? "" : readString(raw, i).value; }
function readString(raw: string, i: i32): Parsed { const p = new Parsed(); i++; while (i < raw.length) { const ch = raw.charAt(i); if (ch == "\"") { p.next = i + 1; return p; } if (ch == "\\" && i + 1 < raw.length) { const n = raw.charAt(++i); p.value += n == "n" ? "\n" : n; } else p.value += ch; i++; } p.next = i; return p; }
function finish(ok: bool, output: string, error: string): i32 { Host.outputString(ok ? "{\"ok\":true,\"output\":" + quote(output) + "}" : "{\"ok\":false,\"error\":" + quote(error) + "}"); return 0; }
function pad(s: string, n: i32): string { return s + spaces(n > s.length ? n - s.length : 0); } function spaces(n: i32): string { let s = ""; for (let i = 0; i < n; i++) s += " "; return s; }
function contains(a: string[], v: string): bool { for (let i = 0; i < a.length; i++) if (a[i] == v) return true; return false; }
function baseName(path: string): string { const i = path.lastIndexOf("/"); return i < 0 ? path : path.slice(i + 1); }
function sort(a: string[]): void { for (let i = 1; i < a.length; i++) { const v = a[i]; let j = i; while (j > 0 && a[j - 1] > v) { a[j] = a[j - 1]; j--; } a[j] = v; } }
function quote(s: string): string { let out = "\""; const hex = "0123456789abcdef"; for (let i = 0; i < s.length; i++) { const ch = s.charAt(i); const code = s.charCodeAt(i); if (ch == "\\") out += "\\\\"; else if (ch == "\"") out += "\\\""; else if (ch == "\n") out += "\\n"; else if (ch == "\r") out += "\\r"; else if (ch == "\t") out += "\\t"; else if (code < 32 || code > 126) out += "\\u" + hex.charAt((code >> 12) & 15) + hex.charAt((code >> 8) & 15) + hex.charAt((code >> 4) & 15) + hex.charAt(code & 15); else out += ch; } return out + "\""; }
