import { Host, Memory } from "@extism/as-pdk";
import { length } from "@extism/as-pdk/lib/env";
import { fsRead, fsList } from "@maw-rs/wasm-sdk";

@external("extism:host/user", "maw.paths.get") declare function mawPathsGet(input: u64): u64;
export function myAbort(message: string | null, fileName: string | null, lineNumber: u32, columnNumber: u32): void {}

class Item { recipient: string = ""; sender: string = "unknown"; team: string = ""; type: string = "message"; subject: string = ""; body: string = ""; path: string = ""; }
class Parsed { value: string = ""; next: i32 = 0; }

export function handle(): i32 {
  const args = extractArgs(Host.inputString());
  const recipient = flagValue(args, "--recipient").toLowerCase();
  const vault = pathGet("vault");
  return vault.length == 0 ? finish(false, "", "vault root unavailable") : finish(true, response(scan(vault, recipient)), null);
}

function scan(vault: string, recipient: string): Item[] {
  const out = new Array<Item>();
  const oracles = listPaths(vault, "dir");
  for (let i = 0; i < oracles.length; i++) {
    const files = listPaths(oracles[i] + "/inbox", "file");
    for (let j = 0; j < files.length; j++) {
      const path = files[j]; if (!path.endsWith(".md")) continue;
      const body = readContent(path); if (body.length == 0) continue;
      const item = parseItem(path, baseName(oracles[i]), body);
      if (recipient.length == 0 || item.recipient.toLowerCase() == recipient) out.push(item);
    }
  }
  return out;
}

function parseItem(path: string, oracle: string, content: string): Item {
  const item = new Item();
  item.path = path; item.recipient = pick(front(content, "recipient"), pick(front(content, "to"), oracle));
  item.sender = pick(front(content, "sender"), pick(front(content, "from"), "unknown"));
  item.team = front(content, "team"); item.type = pick(front(content, "type"), "message"); item.body = markdownBody(content);
  item.subject = pick(front(content, "subject"), pick(firstLine(item.body), baseName(path)));
  return item;
}

function response(items: Item[]): string {
  return "{\"items\":[" + itemsJson(items) + "],\"stats\":{\"totalItems\":" + items.length.toString() + ",\"byRecipient\":" + countJson(items, true) + ",\"byType\":" + countJson(items, false) + ",\"oldestAgeHours\":" + (items.length == 0 ? "null" : "0") + ",\"newestAgeHours\":" + (items.length == 0 ? "null" : "0") + "},\"errors\":[],\"schemaVersion\":1}";
}

function itemsJson(items: Item[]): string {
  let out = "";
  for (let i = 0; i < items.length; i++) {
    const it = items[i]; if (i > 0) out += ",";
    out += "{\"recipient\":" + quote(it.recipient) + ",\"sender\":" + quote(it.sender);
    if (it.team.length > 0) out += ",\"team\":" + quote(it.team);
    out += ",\"type\":" + quote(it.type) + ",\"subject\":" + quote(it.subject) + ",\"body\":" + quote(it.body) + ",\"path\":" + quote(it.path) + ",\"mtime\":0,\"ageHours\":0,\"schemaVersion\":1}";
  }
  return out;
}

function countJson(items: Item[], byRecipient: bool): string {
  const keys = new Array<string>(); const counts = new Array<i32>();
  for (let i = 0; i < items.length; i++) {
    const key = byRecipient ? items[i].recipient : items[i].type; let at = -1;
    for (let j = 0; j < keys.length; j++) if (keys[j] == key) at = j;
    if (at >= 0) counts[at] += 1; else { keys.push(key); counts.push(1); }
  }
  for (let i = 1; i < keys.length; i++) { const k = keys[i]; const c = counts[i]; let j = i; while (j > 0 && strGt(keys[j - 1], k)) { keys[j] = keys[j - 1]; counts[j] = counts[j - 1]; j--; } keys[j] = k; counts[j] = c; }
  let out = "{"; for (let i = 0; i < keys.length; i++) { if (i > 0) out += ","; out += quote(keys[i]) + ":" + counts[i].toString(); }
  return out + "}";
}

function listPaths(path: string, want: string): string[] {
  const res = fsList("{\"path\":" + quote(path) + ",\"recursive\":false,\"includeDirs\":true}");
  const out = new Array<string>(); if (res.indexOf("\"ok\":true") < 0) return out;
  let i = 0;
  while (true) {
    const p = res.indexOf("\"path\":", i); if (p < 0) break;
    let start = p; while (start > 0 && res.charAt(start) != "{") start--;
    let end = p; while (end < res.length && res.charAt(end) != "}") end++;
    const entry = res.slice(start, end + 1);
    if (jsonStringField(entry, "kind") == want) out.push(jsonStringField(entry, "path"));
    i = end + 1;
  }
  sortStrings(out); return out;
}

function pathGet(name: string): string { const input = Memory.allocateString("{\"name\":" + quote(name) + "}"); const output = mawPathsGet(input.offset); const out = new Memory(output, length(output)).toString(); return out.indexOf("\"ok\":true") < 0 ? "" : jsonStringField(out, "path"); }
function readContent(path: string): string { const out = fsRead("{\"path\":" + quote(path) + ",\"encoding\":\"utf8\"}"); return out.indexOf("\"ok\":true") < 0 ? "" : jsonStringField(out, "content"); }

function front(content: string, key: string): string {
  if (!content.startsWith("---\n")) return "";
  const end = content.indexOf("\n---", 4); if (end < 0) return "";
  const fm = content.slice(4, end); let i = 0;
  while (i < fm.length) { let e = fm.indexOf("\n", i); if (e < 0) e = fm.length; const line = trim(fm.slice(i, e)); const colon = line.indexOf(":"); if (colon > 0 && trim(line.slice(0, colon)) == key) return stripQuotes(trim(line.slice(colon + 1))); i = e + 1; }
  return "";
}

function markdownBody(content: string): string {
  if (!content.startsWith("---\n")) return trim(content);
  const end = content.indexOf("\n---", 4); if (end < 0) return trim(content);
  let start = end + 4; while (start < content.length && (content.charAt(start) == "\n" || content.charAt(start) == "\r")) start++;
  return trim(content.slice(start));
}

function firstLine(body: string): string {
  let i = 0;
  while (i < body.length) { let e = body.indexOf("\n", i); if (e < 0) e = body.length; let line = trim(body.slice(i, e)); while (line.startsWith("#")) line = trim(line.slice(1)); if (line.length > 0) return line; i = e + 1; }
  return "";
}

function extractArgs(input: string): string[] { const args = new Array<string>(); let i = 0; while (true) { const at = input.indexOf("\"", i); if (at < 0) break; const p = readJsonString(input, at); args.push(p.value); i = p.next; } return args; }
function flagValue(args: string[], flag: string): string { for (let i = 0; i < args.length; i++) { if (args[i] == flag && i + 1 < args.length) return args[i + 1]; if (args[i].startsWith(flag + "=")) return args[i].slice(flag.length + 1); } return ""; }
function finish(ok: bool, output: string, error: string | null): i32 { Host.outputString(ok ? "{\"ok\":true,\"output\":" + quote(output) + "}" : "{\"ok\":false,\"error\":" + quote(error == null ? "failed" : error!) + "}"); return ok ? 0 : 1; }
function pick(a: string, b: string): string { return a.length > 0 ? a : b; }
function baseName(path: string): string { const p = path.lastIndexOf("/"); return p >= 0 ? path.slice(p + 1) : path; }
function trim(value: string): string { let s = 0; let e = value.length; while (s < e && value.charCodeAt(s) <= 32) s++; while (e > s && value.charCodeAt(e - 1) <= 32) e--; return value.slice(s, e); }
function stripQuotes(value: string): string { return value.length >= 2 && ((value.charAt(0) == "\"" && value.charAt(value.length - 1) == "\"") || (value.charAt(0) == "'" && value.charAt(value.length - 1) == "'")) ? value.slice(1, value.length - 1) : value; }
function sortStrings(a: string[]): void { for (let i = 1; i < a.length; i++) { const x = a[i]; let j = i; while (j > 0 && strGt(a[j - 1], x)) { a[j] = a[j - 1]; j--; } a[j] = x; } }
function strGt(a: string, b: string): bool { const n = a.length < b.length ? a.length : b.length; for (let i = 0; i < n; i++) { const ca = a.charCodeAt(i); const cb = b.charCodeAt(i); if (ca != cb) return ca > cb; } return a.length > b.length; }
function stringAfter(s: string, i: i32): Parsed { while (i < s.length && s.charAt(i) != "\"") i++; return readJsonString(s, i); }
function jsonStringField(s: string, key: string): string { const at = s.indexOf("\"" + key + "\":"); return at < 0 ? "" : stringAfter(s, at + key.length + 3).value; }
function readJsonString(s: string, i: i32): Parsed { const p = new Parsed(); i++; while (i < s.length) { const ch = s.charAt(i); if (ch == "\"") { p.next = i + 1; return p; } if (ch == "\\" && i + 1 < s.length) { const n = s.charAt(i + 1); p.value += n == "n" ? "\n" : n == "r" ? "\r" : n == "t" ? "\t" : n; i += 2; } else { p.value += ch; i++; } } p.next = i; return p; }
function quote(s: string): string { let out = "\""; for (let i = 0; i < s.length; i++) { const ch = s.charAt(i); if (ch == "\\") out += "\\\\"; else if (ch == "\"") out += "\\\""; else if (ch == "\n") out += "\\n"; else if (ch == "\r") out += "\\r"; else if (ch == "\t") out += "\\t"; else out += ch; } return out + "\""; }
