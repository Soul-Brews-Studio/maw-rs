import { Host, Memory } from "@extism/as-pdk";
import { length } from "@extism/as-pdk/lib/env";
@external("extism:host/user", "maw.net.fetch") declare function mawNetFetch(input: u64): u64;
export function myAbort(message: string | null, fileName: string | null, lineNumber: u32, columnNumber: u32): void {}
class Parsed { value: string = ""; next: i32 = 0; }
class Resp { ok: bool = false; status: i32 = 0; body: string = ""; error: string = ""; }
export function handle(): i32 {
  const args = extractArgs(Host.inputString()); const cmd = args.length > 0 ? args[0] : "";
  if (cmd == "whoami") return done(whoami());
  if (cmd == "ls") return done(listState());
  if (cmd == "read") return done(readMessages(args));
  if (cmd == "threads") return done(listThreads(args));
  return fail("usage: maw atlas <whoami|ls|read|threads>");
}
function whoami(): string {
  const me = get("/users/@me", ""); if (!me.ok) return errText(me.error);
  const gs = get("/users/@me/guilds", ""); if (!gs.ok) return errText(gs.error);
  const guilds = objects(gs.body); let out = str(me.body, "username") + " (" + str(me.body, "id") + ") — bot: " + (boolField(me.body, "bot") ? "true" : "false");
  out += "\n" + guilds.length.toString() + " guild(s):";
  for (let i = 0; i < guilds.length; i++) out += "\n  " + str(guilds[i], "name") + " (" + str(guilds[i], "id") + ")";
  return out;
}
function listState(): string {
  const gs = get("/users/@me/guilds", ""); if (!gs.ok) return errText(gs.error);
  const guilds = objects(gs.body); let out = "";
  for (let i = 0; i < guilds.length; i++) {
    const g = guilds[i]; const cs = get("/guilds/" + str(g, "id") + "/channels", "");
    if (!cs.ok) { out += (out.length > 0 ? "\n" : "") + "  ✗ " + str(g, "name") + ": access denied"; continue; }
    const channels = objects(cs.body); let text = 0; let voice = 0; let rows = "";
    for (let j = 0; j < channels.length; j++) { const c = channels[j]; const kind = raw(c, "type"); if (kind == "0") { text++; rows += "\n  💬 #" + str(c, "name") + " (" + str(c, "id") + ")"; } else if (kind == "2") { voice++; rows += "\n  🔊 " + str(c, "name") + " (" + str(c, "id") + ")"; } }
    out += (out.length > 0 ? "\n\n" : "") + str(g, "name") + " — " + text.toString() + " text, " + voice.toString() + " voice" + rows;
  }
  return out.length > 0 ? out : "(no guilds)";
}
function readMessages(args: string[]): string {
  if (args.length < 2) return errText("usage: maw atlas read <channel> [--limit=N] [--json]");
  const id = resolveChannel(args[1]); if (id == "") return errText("channel not found: " + args[1]);
  const limit = flag(args, "--limit=", "5"); const r = get("/channels/" + id + "/messages", "{\"limit\":" + quote(limit) + "}"); if (!r.ok) return errText(r.error);
  const rows = objects(r.body); const json = has(args, "--json"); let out = json ? "[" : "";
  for (let i = rows.length - 1; i >= 0; i--) { const m = rows[i]; if (json) out += (out.length > 1 ? "," : "") + "{\"id\":" + quote(str(m, "id")) + ",\"author\":" + quote(str(m, "username")) + ",\"bot\":" + (boolField(m, "bot") ? "true" : "false") + ",\"content\":" + quote(str(m, "content")) + ",\"timestamp\":" + quote(str(m, "timestamp").slice(0, 19)) + "}"; else out += (out.length > 0 ? "\n" : "") + str(m, "timestamp").slice(0, 16) + " " + str(m, "username") + (boolField(m, "bot") ? " 🤖" : "") + ": " + message(m); }
  if (json) return out + "]"; return out + "\n\n" + rows.length.toString() + " messages";
}
function listThreads(args: string[]): string {
  const gs = get("/users/@me/guilds", ""); if (!gs.ok) return errText(gs.error);
  const guilds = objects(gs.body); const json = has(args, "--json"); let out = json ? "[" : ""; let count = 0;
  for (let i = 0; i < guilds.length; i++) { const g = guilds[i]; const r = get("/guilds/" + str(g, "id") + "/threads/active", ""); if (!r.ok) continue; const ts = arr(r.body, "threads"); if (!json && ts.length > 0) out += (out.length > 0 ? "\n" : "") + str(g, "name") + " — " + ts.length.toString() + " active threads"; for (let j = 0; j < ts.length; j++) { const t = ts[j]; if (json) out += (count > 0 ? "," : "") + "{\"id\":" + quote(str(t, "id")) + ",\"name\":" + quote(str(t, "name")) + ",\"guild\":" + quote(str(g, "name")) + ",\"parent_id\":" + quote(str(t, "parent_id")) + "}"; else out += "\n  🧵 #" + str(t, "name") + " (" + str(t, "id") + ") ← parent: " + str(t, "parent_id"); count++; } }
  return json ? out + "]" : count > 0 ? out : "no active threads found";
}
function resolveChannel(input: string): string {
  let numeric = input.length >= 17 && input.length <= 20; for (let i = 0; i < input.length; i++) if (input.charCodeAt(i) < 48 || input.charCodeAt(i) > 57) numeric = false; if (numeric) return input;
  const clean = (input.startsWith("#") ? input.slice(1) : input).toLowerCase(); const gs = get("/users/@me/guilds", ""); if (!gs.ok) return ""; const guilds = objects(gs.body);
  for (let i = 0; i < guilds.length; i++) { const cs = get("/guilds/" + str(guilds[i], "id") + "/channels", ""); if (!cs.ok) continue; const rows = objects(cs.body); for (let j = 0; j < rows.length; j++) { const name = str(rows[j], "name").toLowerCase(); if (name == clean || name.indexOf(clean) >= 0) return str(rows[j], "id"); } } return "";
}
function get(path: string, query: string): Resp { const req = "{\"endpoint\":\"discord-rest\",\"method\":\"GET\",\"path\":" + quote(path) + (query.length > 0 ? ",\"query\":" + query : "") + "}"; const p = Memory.allocateString(req); const got = mawNetFetch(p.offset); const text = new Memory(got, length(got)).toString(); const r = new Resp(); if (text.indexOf("\"ok\":true") < 0) { r.error = str(text, "error"); if (r.error == "") r.error = "maw.net.fetch failed"; return r; } r.status = intField(text, "status"); r.body = str(text, "body"); r.ok = r.status >= 200 && r.status < 300; r.error = r.ok ? "" : "HTTP " + r.status.toString(); return r; }
function extractArgs(input: string): string[] { const at = input.indexOf("\"args\""); const open = at >= 0 ? input.indexOf("[", at) : input.indexOf("["); return open >= 0 ? strings(input, open) : new Array<string>(); }
function strings(s: string, i: i32): string[] { const a = new Array<string>(); while (i < s.length && s.charAt(i) != "]") { if (s.charAt(i) == "\"") { const p = read(s, i); a.push(p.value); i = p.next; } else i++; } return a; }
function has(a: string[], v: string): bool { for (let i = 0; i < a.length; i++) if (a[i] == v) return true; return false; }
function flag(a: string[], prefix: string, fallback: string): string { for (let i = 0; i < a.length; i++) if (a[i].startsWith(prefix)) return a[i].slice(prefix.length); return fallback; }
function done(output: string): i32 { return output.startsWith("!ERR!") ? fail(output.slice(5)) : ok(output); }
function errText(error: string): string { return "!ERR!" + error; }
function ok(output: string): i32 { Host.outputString("{\"ok\":true,\"output\":" + quote(output) + "}"); return 0; }
function fail(error: string): i32 { Host.outputString("{\"ok\":false,\"error\":" + quote(error) + "}"); return 1; }
function message(m: string): string { const c = str(m, "content"); return c.length > 0 ? c.slice(0, 200) : "(no content)"; }
function arr(s: string, key: string): string[] { const at = s.indexOf("\"" + key + "\""); const open = at >= 0 ? s.indexOf("[", at) : s.indexOf("["); return open < 0 ? new Array<string>() : objectsAt(s, open); }
function objects(s: string): string[] { const open = s.indexOf("["); return open < 0 ? new Array<string>() : objectsAt(s, open); }
function objectsAt(s: string, i: i32): string[] { const out = new Array<string>(); let d = 0; let st = -1; let ins = false; let esc = false; for (; i < s.length; i++) { const c = s.charAt(i); if (ins) { if (esc) esc = false; else if (c == "\\") esc = true; else if (c == "\"") ins = false; continue; } if (c == "\"") ins = true; else if (c == "{") { if (d == 0) st = i; d++; } else if (c == "}") { d--; if (d == 0 && st >= 0) out.push(s.slice(st, i + 1)); } else if (c == "]" && d == 0) break; } return out; }
function str(s: string, key: string): string { const at = s.indexOf("\"" + key + "\""); if (at < 0) return ""; const q = s.indexOf("\"", s.indexOf(":", at) + 1); return q < 0 ? "" : read(s, q).value; }
function raw(s: string, key: string): string { const at = s.indexOf("\"" + key + "\""); if (at < 0) return ""; let i = s.indexOf(":", at) + 1; while (i < s.length && s.charCodeAt(i) <= 32) i++; if (s.charAt(i) == "\"") return read(s, i).value; let e = i; while (e < s.length && ",}]\n\r\t ".indexOf(s.charAt(e)) < 0) e++; return s.slice(i, e); }
function intField(s: string, key: string): i32 { const v = raw(s, key); let n = 0; for (let i = 0; i < v.length; i++) { const c = v.charCodeAt(i); if (c >= 48 && c <= 57) n = n * 10 + <i32>(c - 48); } return n; }
function boolField(s: string, key: string): bool { return raw(s, key) == "true"; }
function read(s: string, i: i32): Parsed { const p = new Parsed(); i++; while (i < s.length) { const c = s.charAt(i); if (c == "\"") { p.next = i + 1; return p; } if (c == "\\" && i + 1 < s.length) { const n = s.charAt(i + 1); p.value += n == "n" ? "\n" : n == "r" ? "\r" : n == "t" ? "\t" : n; i += 2; } else { p.value += c; i++; } } p.next = i; return p; }
function quote(s: string): string { let o = "\""; for (let i = 0; i < s.length; i++) { const c = s.charAt(i); const n = s.charCodeAt(i); if (c == "\\") o += "\\\\"; else if (c == "\"") o += "\\\""; else if (c == "\n") o += "\\n"; else if (c == "\r") o += "\\r"; else if (c == "\t") o += "\\t"; else if (n > 126) o += "\\u" + hex4(n); else o += c; } return o + "\""; }
function hex4(n: i32): string { const h = "0123456789abcdef"; return h.charAt((n >> 12) & 15) + h.charAt((n >> 8) & 15) + h.charAt((n >> 4) & 15) + h.charAt(n & 15); }
