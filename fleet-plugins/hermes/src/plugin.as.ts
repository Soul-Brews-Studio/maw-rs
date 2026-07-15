import { Host, Memory } from "@extism/as-pdk";
import { length } from "@extism/as-pdk/lib/env";
@external("extism:host/user", "maw.net.fetch")
declare function mawNetFetch(input: u64): u64;
export function myAbort(message: string | null, fileName: string | null, lineNumber: u32, columnNumber: u32): void {}
class Parsed { value: string = ""; next: i32 = 0; }
class Resp { ok: bool = false; status: i32 = 0; body: string = ""; error: string = ""; }
class ThreadItem { id: string = ""; name: string = ""; parent: string = ""; last: string = ""; count: string = "?"; archived: bool = false; }
export function handle(): i32 {
  const args = extractArgs(Host.inputString());
  const cmd = args.length > 0 ? args[0] : "";
  if (cmd == "whoami") return done(whoami());
  if (cmd == "channels") return done(channels());
  if (cmd == "read") return done(readChannel(args));
  if (cmd == "threads") return done(threads(args));
  return fail("usage: maw hermes <whoami|channels|read|threads>");
}
function whoami(): string {
  const r = get("/users/@me", ""); if (!r.ok) return errText(r.error);
  return "bot: " + str(r.body, "username") + " | id: " + str(r.body, "id") + " | bot: " + (boolField(r.body, "bot") ? "true" : "false");
}
function channels(): string {
  const r = get("/users/@me/guilds", ""); if (!r.ok) return errText(r.error);
  const rows = objects(r.body); let out = "";
  for (let i = 0; i < rows.length; i++) out += (i > 0 ? "\n" : "") + "guild: " + str(rows[i], "name") + "  (" + str(rows[i], "id") + ")";
  return out.length > 0 ? out : "(no guilds)";
}
function readChannel(args: string[]): string {
  const ch = args.length > 1 ? args[1] : ""; if (ch == "") return errText("usage: maw hermes read <channel-id> [n]");
  const limit = args.length > 2 ? args[2] : "5";
  const r = get("/channels/" + ch + "/messages", "{\"limit\":" + quote(limit) + "}"); if (!r.ok) return errText(r.error);
  const rows = objects(r.body); let out = ""; let threads = new Array<string>();
  for (let i = rows.length - 1; i >= 0; i--) {
    const m = rows[i]; const line = "[" + str(m, "username") + (boolField(m, "bot") ? " bot" : "") + "] " + msgBody(m);
    out += (out.length > 0 ? "\n" : "") + line;
    const th = obj(m, "thread"); if (th.length > 0) threads.push(str(th, "id") + " (" + str(th, "name") + ")");
  }
  if (threads.length > 0) out += "\n-> " + threads.length.toString() + " thread(s) here: " + threads.join(", ") + " - read inside with: maw hermes threads " + ch + " --read";
  return out.length > 0 ? out : "(no messages)";
}
function threads(args: string[]): string {
  if (args.length < 3) return errText("usage: maw hermes threads list/read <channel-id|guild-id> [--all]");
  const mode = args[1]; const id = args[2]; const all = has(args, "--all");
  if (mode != "list" && mode != "read") return errText("usage: maw hermes threads list/read <channel-id|guild-id> [--all]");
  const list = fetchThreads(id, all); if (list.length == 0) return "(no active threads)";
  let out = "";
  for (let i = 0; i < list.length; i++) {
    const t = list[i]; out += (out.length > 0 ? "\n" : "") + "thread " + t.name + "  (" + t.id + ")  msgs=" + t.count + (t.archived ? " [archived]" : "");
    if (mode == "read") {
      const r = get("/channels/" + t.id + "/messages", "{\"limit\":\"50\"}");
      if (!r.ok) { out += "\n     (read HTTP " + r.status.toString() + ")"; continue; }
      const ms = objects(r.body);
      for (let j = ms.length - 1; j >= 0; j--) out += "\n     [" + str(ms[j], "username") + (boolField(ms[j], "bot") ? " bot" : "") + "] " + msgBody(ms[j]);
    }
  }
  return out;
}
function fetchThreads(id: string, all: bool): ThreadItem[] {
  let gid = id; let parent = ""; const ch = get("/channels/" + id, "");
  if (ch.ok && str(ch.body, "guild_id") != "") { gid = str(ch.body, "guild_id"); parent = id; }
  const active = get("/guilds/" + gid + "/threads/active", ""); const list = new Array<ThreadItem>();
  if (!active.ok) return list; addThreads(list, arr(active.body, "threads"), parent, false);
  if (all && parent != "") { const archived = get("/channels/" + parent + "/threads/archived/public", "{\"limit\":\"50\"}"); if (archived.ok) addThreads(list, arr(archived.body, "threads"), parent, true); }
  sortThreads(list); return list;
}
function addThreads(out: ThreadItem[], rows: string[], parent: string, archived: bool): void {
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i]; if (parent != "" && str(r, "parent_id") != parent) continue;
    const id = str(r, "id"); if (seen(out, id)) continue;
    const t = new ThreadItem(); t.id = id; t.name = str(r, "name"); t.parent = str(r, "parent_id"); t.last = str(r, "last_message_id"); t.count = raw(r, "message_count"); t.archived = archived; out.push(t);
  }
}
function get(path: string, query: string): Resp {
  const req = "{\"endpoint\":\"discord-rest\",\"method\":\"GET\",\"path\":" + quote(path) + (query.length > 0 ? ",\"query\":" + query : "") + "}";
  const p = Memory.allocateString(req); const got = mawNetFetch(p.offset); const text = new Memory(got, length(got)).toString();
  const r = new Resp();
  if (text.indexOf("\"ok\":true") < 0) { r.error = str(text, "error"); if (r.error == "") r.error = "maw.net.fetch failed"; return r; }
  r.status = intField(text, "status"); r.body = str(text, "body"); r.ok = r.status >= 200 && r.status < 300; r.error = r.ok ? "" : "HTTP " + r.status.toString(); return r;
}
function extractArgs(input: string): string[] { const at = input.indexOf("\"args\""); const open = at >= 0 ? input.indexOf("[", at) : input.indexOf("["); return open >= 0 ? strings(input, open) : new Array<string>(); }
function strings(s: string, i: i32): string[] { const a = new Array<string>(); while (i < s.length && s.charAt(i) != "]") { if (s.charAt(i) == "\"") { const p = read(s, i); a.push(p.value); i = p.next; } else i++; } return a; }
function has(a: string[], v: string): bool { for (let i = 0; i < a.length; i++) if (a[i] == v) return true; return false; }
function done(output: string): i32 { return output.startsWith("!ERR!") ? fail(output.slice(5)) : ok(output); }
function errText(error: string): string { return "!ERR!" + error; }
function ok(output: string): i32 { Host.outputString("{\"ok\":true,\"output\":" + quote(output) + "}"); return 0; }
function fail(error: string): i32 { Host.outputString("{\"ok\":false,\"error\":" + quote(error) + "}"); return 1; }
function msgBody(m: string): string { const c = str(m, "content"); if (trim(c) != "") return c; if (m.indexOf("\"embeds\":[{") >= 0) return "(embed)"; if (m.indexOf("\"attachments\":[{") >= 0) return "(attachment)"; return "(no text - type " + raw(m, "type") + ")"; }
function seen(a: ThreadItem[], id: string): bool { for (let i = 0; i < a.length; i++) if (a[i].id == id) return true; return false; }
function sortThreads(a: ThreadItem[]): void { for (let i = 1; i < a.length; i++) { const x = a[i]; let j = i; while (j > 0 && newer(x.last, a[j - 1].last)) { a[j] = a[j - 1]; j--; } a[j] = x; } }
function newer(a: string, b: string): bool { if (a.length != b.length) return a.length > b.length; return a > b; }
function arr(s: string, key: string): string[] { const at = s.indexOf("\"" + key + "\""); const open = at >= 0 ? s.indexOf("[", at) : s.indexOf("["); return open < 0 ? new Array<string>() : objectsAt(s, open); }
function objects(s: string): string[] { const open = s.indexOf("["); return open < 0 ? new Array<string>() : objectsAt(s, open); }
function objectsAt(s: string, i: i32): string[] { const out = new Array<string>(); let d = 0; let st = -1; let ins = false; let esc = false; for (; i < s.length; i++) { const c = s.charAt(i); if (ins) { if (esc) esc = false; else if (c == "\\") esc = true; else if (c == "\"") ins = false; continue; } if (c == "\"") ins = true; else if (c == "{") { if (d == 0) st = i; d++; } else if (c == "}") { d--; if (d == 0 && st >= 0) out.push(s.slice(st, i + 1)); } else if (c == "]" && d == 0) break; } return out; }
function obj(s: string, key: string): string { const at = s.indexOf("\"" + key + "\""); if (at < 0) return ""; let i = s.indexOf("{", at); if (i < 0) return ""; const start = i; let d = 0; let ins = false; let esc = false; for (; i < s.length; i++) { const c = s.charAt(i); if (ins) { if (esc) esc = false; else if (c == "\\") esc = true; else if (c == "\"") ins = false; continue; } if (c == "\"") ins = true; else if (c == "{") d++; else if (c == "}") { d--; if (d == 0) return s.slice(start, i + 1); } } return ""; }
function str(s: string, key: string): string { const at = s.indexOf("\"" + key + "\""); if (at < 0) return ""; const q = s.indexOf("\"", s.indexOf(":", at) + 1); return q < 0 ? "" : read(s, q).value; }
function raw(s: string, key: string): string { const at = s.indexOf("\"" + key + "\""); if (at < 0) return ""; let i = s.indexOf(":", at) + 1; while (i < s.length && s.charCodeAt(i) <= 32) i++; if (s.charAt(i) == "\"") return read(s, i).value; let e = i; while (e < s.length && ",}]\n\r\t ".indexOf(s.charAt(e)) < 0) e++; return s.slice(i, e); }
function intField(s: string, key: string): i32 { const v = raw(s, key); let n = 0; for (let i = 0; i < v.length; i++) { const c = v.charCodeAt(i); if (c >= 48 && c <= 57) n = n * 10 + <i32>(c - 48); } return n; }
function boolField(s: string, key: string): bool { return raw(s, key) == "true"; }
function read(s: string, i: i32): Parsed { const p = new Parsed(); i++; while (i < s.length) { const c = s.charAt(i); if (c == "\"") { p.next = i + 1; return p; } if (c == "\\" && i + 1 < s.length) { const n = s.charAt(i + 1); p.value += n == "n" ? "\n" : n == "r" ? "\r" : n == "t" ? "\t" : n; i += 2; } else { p.value += c; i++; } } p.next = i; return p; }
function trim(s: string): string { let a = 0; let b = s.length; while (a < b && s.charCodeAt(a) <= 32) a++; while (b > a && s.charCodeAt(b - 1) <= 32) b--; return s.slice(a, b); }
function quote(s: string): string { let o = "\""; for (let i = 0; i < s.length; i++) { const c = s.charAt(i); if (c == "\\") o += "\\\\"; else if (c == "\"") o += "\\\""; else if (c == "\n") o += "\\n"; else if (c == "\r") o += "\\r"; else if (c == "\t") o += "\\t"; else o += c; } return o + "\""; }
