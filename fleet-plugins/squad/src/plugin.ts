// squad (maw-rs ship tier) — AssemblyScript → WASM port of the locked squad model.
//
// Source of truth: laris-co/athena-oracle/.maw/plugins/squad/impl.ts. This is the
// AS-subset rewrite that `maw plugin build` compiles to plugin.wasm (Extism ship
// tier). It talks to the host only through capability-gated host functions:
//   fs:read:teams / fs:write:teams  → ~/.claude/teams   (roster + inboxes)
//   tmux:read                       → live-session check for `ls`
//   proc:exec:date                  → wall clock (WASM has no clock; see notes below)
//
// The lead IS the team: team = basename(cwd) minus "-oracle", where cwd/home come
// from the InvokeContext the CLI dispatch injects into the guest input. join stays
// on the sanctioned mawjs interim (session-create is not a maw-rs native verb yet),
// so it prints a loud pointer rather than a broken half-spawn.
//
// Guards preserved verbatim from the locked model (adversarial-review canon):
//   * start adopts an existing team folder, never clobbers
//   * say validates member ∈ roster and fails loud with NO orphan inbox write
//   * name guard blocks path traversal (../, /, .)
//   * only the 8 valid --agent-color values are accepted
//   * inbox append never clobbers existing message bytes
import { Host } from "@extism/as-pdk";
import { fsRead, fsWrite, fsList, listSessions, hostExec } from "@maw-rs/wasm-sdk";

export function myAbort(message: string | null, fileName: string | null, lineNumber: u32, columnNumber: u32): void {}

const COLORS: string[] = ["red", "green", "yellow", "blue", "purple", "cyan", "magenta", "white"];

export function handle(): i32 {
  const input = Host.inputString();
  const args = extractArgs(input);
  const cwd = jsonStringField(input, "cwd");
  const home = jsonStringField(input, "home");
  const sub = args.length > 0 ? args[0] : "";

  if (sub == "join") return cmdJoin(deriveTeam(cwd), args); // no home/roster needed for the pointer
  if (sub != "start" && sub != "say" && sub != "ls") return usage();

  if (home == "") return finish(false, null, "no home directory in invoke context");
  const team = deriveTeam(cwd);
  if (team == "") return finish(false, null, "can't derive a team name from this directory");
  const teams = home + "/.claude/teams";

  if (sub == "start") return cmdStart(teams, team, cwd);
  if (sub == "say") return cmdSay(teams, team, args);
  return cmdLs(teams, team);
}

// team = this repo's dir name minus "-oracle". The repo you stand in IS the lead.
function deriveTeam(cwd: string): string {
  let base = cwd;
  const slash = cwd.lastIndexOf("/");
  if (slash >= 0) base = cwd.slice(slash + 1);
  if (base.endsWith("-oracle")) base = base.slice(0, base.length - 7);
  return base;
}

function dirOf(teams: string, team: string): string { return teams + "/" + team; }
function cfgOf(teams: string, team: string): string { return dirOf(teams, team) + "/config.json"; }
function inboxOf(teams: string, team: string, name: string): string {
  return dirOf(teams, team) + "/inboxes/" + name + ".json";
}
function baseName(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash >= 0 ? path.slice(slash + 1) : path;
}

// ── start ────────────────────────────────────────────────────────────────────
// maw squad start — start THIS repo's squad. Adopts an existing folder, never clobbers.
//
// Adopt path (config.json exists) is a SURGICAL raw-text update, never a typed
// round-trip: the reference impl (athena-oracle/.maw/plugins/squad/impl.ts cmdStart)
// parses config into a plain JS object, fills leadSessionId when missing, sets
// leadRepo, and re-stringifies — so any field it doesn't model (a top-level
// "description", a member "isActive", etc.) SURVIVES. A typed round-trip through
// Config/Member here would silently drop every out-of-schema field and reorder keys
// (field-verified regression). So on adopt we edit only leadSessionId + leadRepo in
// place and leave every other byte intact. The create path (no existing config) keeps
// the typed serialize — there are no unknown fields to preserve.
function cmdStart(teams: string, team: string, cwd: string): i32 {
  const existing = readFile(cfgOf(teams, team));
  const existed = existing.length > 0;
  let content: string;
  let leadSessionId: string;
  if (existed) {
    const priorSid = jsonStringField(existing, "leadSessionId");
    leadSessionId = priorSid == "" ? nowMillis() : priorSid; // uuid host verb is out-of-sandbox; timestamp id per reference fallback
    let updated = ensureStringField(existing, "leadSessionId", leadSessionId); // insert/fill only when missing or empty
    updated = setStringField(updated, "leadRepo", cwd); // replace value in place, or append if absent
    content = updated;
  } else {
    const cfg = newConfig(team);
    cfg.leadSessionId = nowMillis();
    cfg.leadRepo = cwd;
    leadSessionId = cfg.leadSessionId;
    content = serializeConfig(cfg);
  }
  const wrote = writeFile(cfgOf(teams, team), content);
  if (wrote != "") return finish(false, null, wrote);

  // teams runtime delivers member→lead replies to team-lead.json, not lead.json
  const leadIbx = inboxOf(teams, team, "team-lead");
  if (readFile(leadIbx) == "") {
    const w = writeFile(leadIbx, "[]\n");
    if (w != "") return finish(false, null, w);
  }

  let out = "⚡ squad '" + team + "' " + (existed ? "adopted (already existed)" : "started") + " → " + dirOf(teams, team) + "\n";
  out += "   lead: " + baseName(cwd) + " (this repo)   lead session: " + leadSessionId + "\n";
  out += "   replies arrive in: inboxes/team-lead.json\n";
  out += "   next: maw squad join digger   ·   maw squad say digger \"<text>\"   ·   maw squad ls";
  return finish(true, out, null);
}

// ── join (loud pointer) ────────────────────────────────────────────────────────
// Session-create is not a maw-rs native verb yet; per canon join runs via mawjs.
function cmdJoin(team: string, args: string[]): i32 {
  const role = args.length > 1 ? args[1] : "";
  const color = args.length > 2 ? args[2] : "cyan";
  if (role == "") return finish(false, null, "usage: maw squad join <oracle> [color]");
  if (!nameOk(role)) return finish(false, null, "invalid oracle name '" + role + "' (letters/digits/-/_ only)");
  if (!colorOk(color))
    return finish(false, null, "invalid color '" + color + "' — spawn would fail SILENTLY. valid: " + COLORS.join(" "));
  let out = "⚡ join runs via mawjs until maw-rs grows a native session-create verb.\n";
  out += "   run: mawjs squad join " + role + " " + color + "\n";
  out += "   (spawns " + role + " into squad '" + team + "' from its own repo, then say: maw squad say " + role + " \"<text>\")";
  return finish(true, out, null);
}

// ── say ──────────────────────────────────────────────────────────────────────
// maw squad say <member> <text...> — append to a member's inbox (never clobbers).
function cmdSay(teams: string, team: string, args: string[]): i32 {
  const member = args.length > 1 ? args[1] : "";
  const text = args.length > 2 ? args.slice(2).join(" ") : "";
  if (member == "" || text == "") return finish(false, null, "usage: maw squad say <member> <text>");
  if (!nameOk(member)) return finish(false, null, "invalid member name '" + member + "' (letters/digits/-/_ only)");

  const cfgContent = readFile(cfgOf(teams, team));
  if (cfgContent == "")
    return finish(false, null, "squad '" + team + "' not started — run: maw squad start (from the lead repo)");

  // only roster members poll an inbox — saying to anyone else is silent message loss
  const members = parseMembers(cfgContent);
  if (!hasMember(members, member)) {
    const names = memberNames(members);
    return finish(false, null,
      "'" + member + "' is not in squad '" + team + "' — members: " + (names == "" ? "(none)" : names) +
      ".   join first: maw squad join " + member);
  }

  const path = inboxOf(teams, team, member);
  const existing = readFile(path);
  const msg = messageJson("team-lead", text, isoNow());
  const w = writeFile(path, appendToArray(existing, msg));
  if (w != "") return finish(false, null, w);
  return finish(true, "✓ said to " + member + "@" + team + ": " + text, null);
}

// ── ls ───────────────────────────────────────────────────────────────────────
// maw squad ls — show THIS repo's squad: members + inboxes + live tmux.
function cmdLs(teams: string, team: string): i32 {
  let out = "squad: " + team + "\n";
  const cfgContent = readFile(cfgOf(teams, team));
  let members = new Array<Member>();
  if (cfgContent == "") {
    out += "  (not started — maw squad start)\n";
  } else {
    const cfg = parseConfig(cfgContent);
    out += "  lead: " + baseName(cfg.leadRepo) + "   session: " + cfg.leadSessionId + "\n";
    members = cfg.members;
    if (members.length > 0) {
      for (let i = 0; i < members.length; i++) {
        const m = members[i];
        out += "  member: " + m.name + " (" + m.color + ")  " + m.repo + "\n";
      }
    } else {
      out += "  members: (none yet — maw squad join <oracle>)\n";
    }
  }

  const inboxes = listInboxes(teams, team);
  if (inboxes != "") out += "  inboxes: " + inboxes + "\n";

  out += "  live tmux: " + liveMembers(members) + "\n";
  return finish(true, rtrimNewline(out), null);
}

// list inbox files as "name (N unread)" from the fs.list of inboxes/
function listInboxes(teams: string, team: string): string {
  const res = fsList("{\"path\":" + quote(dirOf(teams, team) + "/inboxes") + "}");
  if (res.indexOf("\"ok\":true") < 0) return "";
  const parts = new Array<string>();
  let i = 0;
  const marker = "\"path\":";
  while (true) {
    const at = res.indexOf(marker, i);
    if (at < 0) break;
    let j = at + marker.length;
    while (j < res.length && res.charAt(j) != "\"") j++;
    if (j >= res.length) break;
    const parsed = readJsonString(res, j);
    i = parsed.next;
    const file = baseName(parsed.value);
    if (!file.endsWith(".json")) continue;
    const name = file.slice(0, file.length - 5);
    const unread = countUnread(readFile(parsed.value));
    parts.push(unread > 0 ? name + " (" + unread.toString() + " unread)" : name);
  }
  return parts.join(", ");
}

function countUnread(content: string): i32 {
  if (content == "") return 0;
  let count = 0;
  let i = 0;
  const marker = "\"read\": false";
  while (true) {
    const at = content.indexOf(marker, i);
    if (at < 0) break;
    count++;
    i = at + marker.length;
  }
  return count;
}

// a member's session is named after the oracle itself (one oracle, one session)
function liveMembers(members: Member[]): string {
  if (members.length == 0) return "(none)";
  const res = listSessions("{}");
  const live = new Array<string>();
  for (let i = 0; i < members.length; i++) {
    const needle = "\"name\":" + quote(members[i].name) + ",\"windows\"";
    if (res.indexOf(needle) >= 0) live.push(members[i].name);
  }
  return live.length > 0 ? live.join(", ") : "(none)";
}

function usage(): i32 {
  let out = "maw squad — the lead IS the team. Run from the lead oracle's repo; team = repo name.\n";
  out += "  maw squad start                  start this repo's squad (athena-oracle → 'athena')\n";
  out += "  maw squad join <oracle> [color]  spawn <oracle> into this squad (own repo = identity)\n";
  out += "  maw squad say  <member> <text>   append a message to a member's inbox\n";
  out += "  maw squad ls                     show this squad: members + inboxes + live tmux";
  return finish(true, out, null);
}

// ── config model ───────────────────────────────────────────────────────────────
class Member {
  agentId: string; name: string; color: string; repo: string; joinedAt: string;
  constructor(agentId: string, name: string, color: string, repo: string, joinedAt: string) {
    this.agentId = agentId; this.name = name; this.color = color; this.repo = repo; this.joinedAt = joinedAt;
  }
}
class Config {
  name: string; members: Member[]; createdAt: string; leadSessionId: string; leadRepo: string;
  constructor(name: string, members: Member[], createdAt: string, leadSessionId: string, leadRepo: string) {
    this.name = name; this.members = members; this.createdAt = createdAt; this.leadSessionId = leadSessionId; this.leadRepo = leadRepo;
  }
}

function newConfig(team: string): Config {
  return new Config(team, new Array<Member>(), nowMillis(), "", "");
}

function parseConfig(json: string): Config {
  const createdAt = jsonNumberField(json, "createdAt");
  return new Config(
    jsonStringField(json, "name"),
    parseMembers(json),
    createdAt == "" ? "0" : createdAt,
    jsonStringField(json, "leadSessionId"),
    jsonStringField(json, "leadRepo"),
  );
}

function parseMembers(json: string): Member[] {
  const members = new Array<Member>();
  const marker = "\"members\":";
  const mi = json.indexOf(marker);
  if (mi < 0) return members;
  let i = mi + marker.length;
  while (i < json.length && json.charAt(i) != "[") i++;
  if (i >= json.length) return members;
  const arrEnd = matchDelim(json, i, "[", "]");
  if (arrEnd <= i) return members;
  const body = json.slice(i, arrEnd + 1);
  let j = 0;
  while (j < body.length) {
    if (body.charAt(j) == "{") {
      const end = matchDelim(body, j, "{", "}");
      if (end <= j) break;
      const obj = body.slice(j, end + 1);
      const joined = jsonNumberField(obj, "joinedAt");
      members.push(new Member(
        jsonStringField(obj, "agentId"),
        jsonStringField(obj, "name"),
        jsonStringField(obj, "color"),
        jsonStringField(obj, "repo"),
        joined == "" ? "0" : joined,
      ));
      j = end + 1;
    } else {
      j++;
    }
  }
  return members;
}

function serializeConfig(cfg: Config): string {
  let out = "{\n";
  out += "  \"name\": " + quote(cfg.name) + ",\n";
  out += "  \"members\": " + serializeMembers(cfg.members) + ",\n";
  out += "  \"createdAt\": " + (cfg.createdAt == "" ? "0" : cfg.createdAt) + ",\n";
  out += "  \"leadSessionId\": " + quote(cfg.leadSessionId) + ",\n";
  out += "  \"leadRepo\": " + quote(cfg.leadRepo) + "\n";
  out += "}\n";
  return out;
}

function serializeMembers(members: Member[]): string {
  if (members.length == 0) return "[]";
  let out = "[\n";
  for (let i = 0; i < members.length; i++) {
    const m = members[i];
    out += "    {\n";
    out += "      \"agentId\": " + quote(m.agentId) + ",\n";
    out += "      \"name\": " + quote(m.name) + ",\n";
    out += "      \"color\": " + quote(m.color) + ",\n";
    out += "      \"repo\": " + quote(m.repo) + ",\n";
    out += "      \"joinedAt\": " + (m.joinedAt == "" ? "0" : m.joinedAt) + "\n";
    out += "    }";
    if (i + 1 < members.length) out += ",";
    out += "\n";
  }
  out += "  ]";
  return out;
}

function hasMember(members: Member[], name: string): bool {
  for (let i = 0; i < members.length; i++) if (members[i].name == name) return true;
  return false;
}
function memberNames(members: Member[]): string {
  const names = new Array<string>();
  for (let i = 0; i < members.length; i++) names.push(members[i].name);
  return names.join(", ");
}

// one inbox message object, formatted as a JSON.stringify(_, null, 2) array element
function messageJson(from: string, text: string, timestamp: string): string {
  let out = "{\n";
  out += "    \"from\": " + quote(from) + ",\n";
  out += "    \"text\": " + quote(text) + ",\n";
  out += "    \"timestamp\": " + quote(timestamp) + ",\n";
  out += "    \"color\": \"cyan\",\n";
  out += "    \"type\": \"message\",\n";
  out += "    \"read\": false\n";
  out += "  }";
  return out;
}

// append a pre-formatted element to a JSON array string, keeping existing bytes intact
function appendToArray(existing: string, element: string): string {
  const trimmed = rtrimWs(existing);
  if (trimmed == "" || trimmed == "[]" || trimmed == "[\n]") return "[\n  " + element + "\n]\n";
  const close = trimmed.lastIndexOf("]");
  if (close < 0) return "[\n  " + element + "\n]\n";
  let body = rtrimWs(trimmed.slice(0, close));
  if (body.endsWith("[")) return body + "\n  " + element + "\n]\n";
  return body + ",\n  " + element + "\n]\n";
}

// ── name / color guards ─────────────────────────────────────────────────────────
function nameOk(s: string): bool {
  if (s.length == 0) return false;
  if (!isAlnum(s.charCodeAt(0))) return false;
  for (let i = 1; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (!isAlnum(c) && c != 45 && c != 95) return false; // '-' '_'
  }
  return true;
}
function colorOk(color: string): bool {
  for (let i = 0; i < COLORS.length; i++) if (COLORS[i] == color) return true;
  return false;
}
function isAlnum(c: i32): bool {
  return (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122);
}

// ── host fs helpers ────────────────────────────────────────────────────────────
function readFile(path: string): string {
  const out = fsRead("{\"path\":" + quote(path) + ",\"encoding\":\"utf8\"}");
  if (out.indexOf("\"ok\":true") < 0) return "";
  return jsonStringField(out, "content");
}
// returns "" on success, else an error message
function writeFile(path: string, content: string): string {
  const out = fsWrite("{\"path\":" + quote(path) + ",\"content\":" + quote(content) + ",\"mode\":\"overwrite\",\"mkdirp\":true}");
  if (out.indexOf("\"ok\":true") >= 0) return "";
  const err = jsonStringField(out, "error");
  return err == "" ? "write failed: " + path : err;
}

// ── wall clock via proc:exec:date (WASM host exposes no time host fn) ─────────────
function execStdout(argsJson: string): string {
  const out = hostExec(argsJson);
  if (out.indexOf("\"ok\":true") < 0) return "";
  return jsonStringField(out, "stdout").trim();
}
function nowMillis(): string {
  const secs = execStdout("{\"cmd\":\"date\",\"args\":[\"+%s\"]}");
  return secs == "" ? "0" : secs + "000";
}
function isoNow(): string {
  const iso = execStdout("{\"cmd\":\"date\",\"args\":[\"-u\",\"+%Y-%m-%dT%H:%M:%S.000Z\"]}");
  return iso == "" ? "1970-01-01T00:00:00.000Z" : iso;
}

// ── JSON toolkit (from the wasm-parity contacts/send examples) ────────────────────
function extractArgs(json: string): string[] {
  const marker = "\"args\":[";
  const start = json.indexOf(marker);
  if (start < 0) return [];
  let i = start + marker.length;
  const out = new Array<string>();
  while (i < json.length && json.charAt(i) != "]") {
    if (json.charAt(i) == "\"") {
      const parsed = readJsonString(json, i);
      out.push(parsed.value);
      i = parsed.next;
    } else {
      i++;
    }
  }
  return out;
}

class ParsedString {
  value: string; next: i32;
  constructor(value: string, next: i32) { this.value = value; this.next = next; }
}
function readJsonString(s: string, start: i32): ParsedString {
  let out = "";
  let i = start + 1;
  while (i < s.length) {
    const ch = s.charAt(i);
    if (ch == "\\") {
      i++;
      if (i >= s.length) break;
      const e = s.charAt(i);
      if (e == "n") out += "\n";
      else if (e == "r") out += "\r";
      else if (e == "t") out += "\t";
      else out += e;
    } else if (ch == "\"") {
      return new ParsedString(out, i + 1);
    } else {
      out += ch;
    }
    i++;
  }
  return new ParsedString(out, i);
}
function jsonStringField(json: string, key: string): string {
  const marker = "\"" + key + "\":";
  const start = json.indexOf(marker);
  if (start < 0) return "";
  let i = start + marker.length;
  while (i < json.length && isSpace(json.charCodeAt(i))) i++;
  if (i >= json.length || json.charAt(i) != "\"") return "";
  return readJsonString(json, i).value;
}
function jsonNumberField(json: string, key: string): string {
  const marker = "\"" + key + "\":";
  const start = json.indexOf(marker);
  if (start < 0) return "";
  let i = start + marker.length;
  while (i < json.length && isSpace(json.charCodeAt(i))) i++;
  let out = "";
  while (i < json.length) {
    const c = json.charCodeAt(i);
    if ((c >= 48 && c <= 57) || c == 45) { out += json.charAt(i); i++; } else break;
  }
  return out;
}
function matchDelim(s: string, start: i32, open: string, close: string): i32 {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = start; i < s.length; i++) {
    const ch = s.charAt(i);
    if (inString) {
      if (escaped) escaped = false;
      else if (ch == "\\") escaped = true;
      else if (ch == "\"") inString = false;
    } else if (ch == "\"") inString = true;
    else if (ch == open) depth++;
    else if (ch == close) {
      depth--;
      if (depth == 0) return i;
    }
  }
  return -1;
}

// ── surgical top-level field editing (adopt path) ────────────────────────────────
// Edit one field of an object's raw JSON text without a lossy typed round-trip, so
// unknown/out-of-schema fields and key order survive byte-for-byte. All lookups are
// scoped to the ROOT object's direct members (brace-depth 1): a nested member's
// "name"/"repo" never shadows the top-level keys we touch.
class FieldSpan {
  found: bool; valStart: i32; valEnd: i32;
  constructor(found: bool, valStart: i32, valEnd: i32) {
    this.found = found; this.valStart = valStart; this.valEnd = valEnd;
  }
}

// End index (exclusive) of the JSON value that begins at `v` (a non-space char).
function valueEnd(json: string, v: i32): i32 {
  if (v >= json.length) return json.length;
  const ch = json.charAt(v);
  if (ch == "\"") return readJsonString(json, v).next;
  if (ch == "{") { const e = matchDelim(json, v, "{", "}"); return e < 0 ? json.length : e + 1; }
  if (ch == "[") { const e = matchDelim(json, v, "[", "]"); return e < 0 ? json.length : e + 1; }
  // number / true / false / null — run to the next structural delimiter, then rtrim
  let i = v;
  while (i < json.length) {
    const c = json.charAt(i);
    if (c == "," || c == "}" || c == "]") break;
    i++;
  }
  while (i > v && isSpace(json.charCodeAt(i - 1))) i--;
  return i;
}

// Locate a direct member `key` of the root object; return the span of its value.
function findTopLevelKey(json: string, key: string): FieldSpan {
  let open = 0;
  while (open < json.length && json.charAt(open) != "{") open++;
  if (open >= json.length) return new FieldSpan(false, -1, -1);
  let i = open + 1;
  let depth = 1;
  while (i < json.length) {
    const ch = json.charAt(i);
    if (ch == "\"") {
      const parsed = readJsonString(json, i);
      if (depth == 1) {
        let j = parsed.next;
        while (j < json.length && isSpace(json.charCodeAt(j))) j++;
        if (j < json.length && json.charAt(j) == ":" && parsed.value == key) {
          let v = j + 1;
          while (v < json.length && isSpace(json.charCodeAt(v))) v++;
          return new FieldSpan(true, v, valueEnd(json, v));
        }
      }
      i = parsed.next;
      continue;
    }
    if (ch == "{" || ch == "[") { depth++; i++; continue; }
    if (ch == "}" || ch == "]") { depth--; i++; if (depth == 0) break; continue; }
    i++;
  }
  return new FieldSpan(false, -1, -1);
}

// Append `"key": <rawValue>` as the last member of the root object, matching the
// 2-space canonical layout the reference's JSON.stringify(_, null, 2) produces.
function appendTopLevelField(json: string, key: string, rawValue: string): string {
  let open = 0;
  while (open < json.length && json.charAt(open) != "{") open++;
  if (open >= json.length) return json;
  const close = matchDelim(json, open, "{", "}");
  if (close < 0) return json;
  const prop = "\"" + key + "\": " + rawValue;
  let last = close - 1;
  while (last > open && isSpace(json.charCodeAt(last))) last--;
  if (last == open) {
    // empty object "{}" (only whitespace inside): open a fresh 2-space body
    return json.slice(0, open + 1) + "\n  " + prop + "\n" + json.slice(close);
  }
  return json.slice(0, last + 1) + ",\n  " + prop + json.slice(last + 1);
}

// Set a top-level string field to `value`: replace its value in place if present,
// else append it. Preserves every other byte.
function setStringField(json: string, key: string, value: string): string {
  const span = findTopLevelKey(json, key);
  if (span.found) return json.slice(0, span.valStart) + quote(value) + json.slice(span.valEnd);
  return appendTopLevelField(json, key, quote(value));
}

// Fill a top-level string field only when it is absent or empty/null (reference:
// `if (!cfg.leadSessionId) …`). A present, non-empty value is left byte-for-byte.
function ensureStringField(json: string, key: string, value: string): string {
  const span = findTopLevelKey(json, key);
  if (!span.found) return appendTopLevelField(json, key, quote(value));
  const cur = json.slice(span.valStart, span.valEnd);
  if (cur == "\"\"" || cur == "null") return json.slice(0, span.valStart) + quote(value) + json.slice(span.valEnd);
  return json;
}

function finish(ok: bool, output: string | null, error: string | null): i32 {
  Host.outputString(resultJson(ok, output, error));
  return 0;
}
function resultJson(ok: bool, output: string | null, error: string | null): string {
  let json = ok ? "{\"ok\":true" : "{\"ok\":false";
  if (output !== null) json += ",\"output\":" + quote(output);
  if (!ok && error !== null) json += ",\"error\":" + quote(error);
  return json + "}";
}
function quote(value: string): string {
  let out = "\"";
  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    const ch = value.charAt(i);
    if (ch == "\\") out += "\\\\";
    else if (ch == "\"") out += "\\\"";
    else if (ch == "\n") out += "\\n";
    else if (ch == "\r") out += "\\r";
    else if (ch == "\t") out += "\\t";
    else if (code < 32 || code > 126) out += "\\u" + hex4(code); // keep output JSON pure-ASCII (emoji/→/— via \uXXXX)
    else out += ch;
  }
  return out + "\"";
}
function isSpace(c: i32): bool { return c == 32 || c == 9 || c == 10 || c == 13; }
function rtrimWs(s: string): string {
  let end = s.length;
  while (end > 0 && isSpace(s.charCodeAt(end - 1))) end--;
  return s.slice(0, end);
}
function rtrimNewline(s: string): string {
  let end = s.length;
  while (end > 0 && (s.charCodeAt(end - 1) == 10 || s.charCodeAt(end - 1) == 13)) end--;
  return s.slice(0, end);
}
function hex4(code: i32): string {
  const digits = "0123456789abcdef";
  return digits.charAt((code >> 12) & 15) + digits.charAt((code >> 8) & 15) + digits.charAt((code >> 4) & 15) + digits.charAt(code & 15);
}
