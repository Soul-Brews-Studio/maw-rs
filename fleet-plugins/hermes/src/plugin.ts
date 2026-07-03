// maw hermes (maw-rs bun-dev tier) - Discord REST bot verbs + API-server user turns.
//
// Ported from laris-co/hermes-oracle/.maw/plugins/hermes. maw-rs runs bun-dev
// plugins with cwd = the plugin directory; if Hermes ever needs the caller cwd,
// use $PWD from the invoking shell rather than process.cwd().

declare const Bun: any;
declare const process: any;

type Log = (s?: string) => void;
type HttpResult = { status: number; ok: boolean; data: any };
type ServerCtx = {
  log: Log;
  pos: string[];
  flag: (f: string) => boolean;
  opt: (f: string) => string | undefined;
  json: boolean;
};
type ThreadCtx = { log: Log; pos: string[]; flag: (f: string) => boolean };

const DISCORD_API = "https://discord.com/api/v10";
const DISCORD_TOKEN_PASS = "discord/hermes-nous-gateway-token";
const API_KEY_PASS = "hermes/api-server-key";
const WEBHOOK_RELAY_TOKEN_PASS = "webhook-relay/api-token";
const API_SERVER = `http://127.0.0.1:${(process.env.API_SERVER_PORT || "8642").trim()}`;
const RELAY_SIG = (process.env.MAW_HERMES_RELAY_SIG || "Hermes Oracle (Claude - AI)").trim();
const ARRA = "/opt/Code/github.com/laris-co/arra/src/arthur-cli.ts";

function hermesHome(): string {
  return (process.env.HERMES_HOME || `${process.env.HOME}/.hermes`).trim();
}

function seenPath(): string {
  return `${hermesHome()}/maw-hermes-seen.json`;
}

function passShow(name: string): string {
  const p = Bun.spawnSync(["pass", "show", name]);
  const out = p.stdout.toString().trim();
  if (!out) {
    const err = p.stderr?.toString?.().trim?.() || "";
    throw new Error(`pass show ${name} returned empty${err ? `: ${err}` : ""}`);
  }
  return out;
}

function discordToken(): string {
  const env = (process.env.DISCORD_BOT_TOKEN || "").trim();
  return env || passShow(DISCORD_TOKEN_PASS);
}

function apiServerKey(): string {
  const env = (process.env.API_SERVER_KEY || "").trim();
  return env || passShow(API_KEY_PASS);
}

async function httpJson(url: string, init: RequestInit = {}, headers: Record<string, string> = {}): Promise<HttpResult> {
  const res = await fetch(url, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...headers,
      ...(init.headers || {}),
    },
  });
  const txt = await res.text();
  let data: any;
  try {
    data = JSON.parse(txt);
  } catch {
    data = txt;
  }
  return { status: res.status, ok: res.ok, data };
}

async function discordApi(path: string, init: RequestInit = {}): Promise<HttpResult> {
  return httpJson(DISCORD_API + path, init, { Authorization: `Bot ${discordToken()}` });
}

async function apiServer(path: string, init: RequestInit = {}): Promise<HttpResult> {
  return httpJson(API_SERVER + path, init, { Authorization: `Bearer ${apiServerKey()}` });
}

function need(r: HttpResult): void {
  if (!r.ok) {
    throw new Error(`HTTP ${r.status}: ${typeof r.data === "string" ? r.data : JSON.stringify(r.data)}`);
  }
}

function dump(log: Log, r: HttpResult): void {
  log(JSON.stringify(r.data, null, 2));
}

export async function whoami(log: Log): Promise<void> {
  const r = await discordApi("/users/@me");
  if (!r.ok) throw new Error(`whoami HTTP ${r.status}`);
  log(`bot: ${r.data.username} | id: ${r.data.id} | bot: ${r.data.bot}`);
}

export async function send(log: Log, args: string[]): Promise<void> {
  const ch = args[1];
  const text = args.slice(2).join(" ");
  if (!ch || !text) throw new Error("usage: maw hermes send <channel-id> <text...>");
  const r = await discordApi(`/channels/${ch}/messages`, {
    method: "POST",
    body: JSON.stringify({ content: text }),
  });
  if (!r.ok) throw new Error(`send HTTP ${r.status}: ${JSON.stringify(r.data)}`);
  log(`\x1b[32msent\x1b[0m -> discord:${ch} (msg ${r.data.id}): ${text}`);
}

export async function read(log: Log, args: string[]): Promise<void> {
  const ch = args[1];
  const n = Number(args[2]) || 5;
  if (!ch) throw new Error("usage: maw hermes read <channel-id> [n]");
  const r = await discordApi(`/channels/${ch}/messages?limit=${n}`);
  if (!r.ok) throw new Error(`read HTTP ${r.status}`);
  const rows = (r.data as any[]).slice().reverse();
  for (const m of rows) log(`[${m.author.username}${m.author.bot ? " bot" : ""}] ${m.content}`);
  const threadIds = rows.filter((m: any) => m.thread?.id).map((m: any) => `${m.thread.id} (${m.thread.name})`);
  if (threadIds.length) {
    log(`\x1b[33m-> ${threadIds.length} thread(s) here:\x1b[0m ${threadIds.join(", ")} - read inside with: maw hermes threads ${ch} --read`);
  }
}

export async function channels(log: Log): Promise<void> {
  const r = await discordApi("/users/@me/guilds");
  if (!r.ok) throw new Error(`channels HTTP ${r.status}`);
  for (const g of r.data as any[]) log(`guild: ${g.name}  (${g.id})`);
}

export function sessionOrigin(sid: string): any | null {
  try {
    const path = `${hermesHome()}/sessions/sessions.json`;
    const raw = Bun.spawnSync(["cat", path]).stdout.toString();
    const data = JSON.parse(raw);
    const entries = data?.entries ? data.entries : data;
    for (const v of Object.values(entries as Record<string, any>)) {
      if ((v as any)?.session_id === sid) return (v as any)?.origin ?? null;
    }
  } catch {
    // fall through
  }
  return null;
}

export function originNote(sid: string): string | undefined {
  const origin = sessionOrigin(sid);
  if (!origin) return undefined;
  const label = origin.chat_name || origin.chat_id || origin.platform || "unknown";
  return `[Session origin: ${origin.platform || "discord"} / ${label}`
    + ` (chat_type=${origin.chat_type || "?"}). This turn arrives via the API server, not a`
    + ` live platform connection - platform-specific actions are unavailable, but this IS`
    + ` where the session lives. Do not guess the channel from memory.]`;
}

export async function chat(log: Log, args: string[]): Promise<void> {
  const flags = new Set(args.filter((a) => a.startsWith("--")));
  const rest = args.slice(1).filter((a) => !a.startsWith("--"));
  const sid = rest[0];
  const text = rest.slice(1).join(" ");
  if (!sid || !text) throw new Error("usage: maw hermes chat <session-id> <text...> [--discord]");

  const origin = sessionOrigin(sid);
  const body: any = { message: text };
  const note = originNote(sid);
  if (note) body.system_message = note;

  const r = await apiServer(`/api/sessions/${sid}/chat`, { method: "POST", body: JSON.stringify(body) });
  if (!r.ok) throw new Error(`chat HTTP ${r.status}: ${JSON.stringify(r.data)}`);
  const reply = r.data?.message?.content ?? JSON.stringify(r.data);
  log(`\x1b[36mHermes\x1b[0m (session ${r.data?.session_id ?? sid}):`);
  log(reply);

  if (flags.has("--discord")) {
    const ch = origin?.chat_id ?? origin?.thread_id;
    if (!ch) {
      log("\x1b[33m(--discord: no Discord channel bound to this session in sessions.json)\x1b[0m");
      return;
    }
    await discordApi(`/channels/${ch}/messages`, {
      method: "POST",
      body: JSON.stringify({ content: `[local relay] ${text}\n- relayed by ${RELAY_SIG}` }),
    });
    await discordApi(`/channels/${ch}/messages`, { method: "POST", body: JSON.stringify({ content: reply }) });
    log(`\x1b[32m(forwarded to discord:${ch})\x1b[0m`);
  }
}

function loadSeen(): Record<string, string> {
  try {
    const txt = Bun.spawnSync(["cat", seenPath()]).stdout.toString().trim();
    return txt ? JSON.parse(txt) : {};
  } catch {
    return {};
  }
}

async function saveSeen(seen: Record<string, string>): Promise<void> {
  try {
    await Bun.write(seenPath(), JSON.stringify(seen, null, 2));
  } catch {
    // best effort
  }
}

function newer(a?: string, b?: string): boolean {
  if (!a) return false;
  if (!b) return true;
  try {
    return BigInt(a) > BigInt(b);
  } catch {
    return a > b;
  }
}

function msgBody(m: any): string {
  if (m?.content && m.content.trim()) return m.content;
  if (m?.embeds?.length) return "(embed)";
  if (m?.attachments?.length) return `(${m.attachments.length} attachment)`;
  return `(no text - type ${m?.type})`;
}

async function fetchThreads(id: string, inclArchived: boolean) {
  let gid: string | null = null;
  let parentFilter: string | null = null;
  const ch = await discordApi(`/channels/${id}`);
  if (ch.ok && ch.data?.guild_id) {
    gid = ch.data.guild_id;
    parentFilter = id;
  } else {
    gid = id;
    parentFilter = null;
  }

  const act = await discordApi(`/guilds/${gid}/threads/active`);
  if (!act.ok) throw new Error(`threads/active HTTP ${act.status}: ${JSON.stringify(act.data)}`);
  let list: any[] = (act.data?.threads ?? []).filter((t: any) => !parentFilter || t.parent_id === parentFilter);

  if (inclArchived && parentFilter) {
    const arc = await discordApi(`/channels/${parentFilter}/threads/archived/public?limit=50`);
    if (arc.ok) list = list.concat((arc.data?.threads ?? []).map((t: any) => ({ ...t, _archived: true })));
  }
  const byId: Record<string, any> = {};
  for (const t of list) byId[t.id] = byId[t.id] || t;
  return {
    gid,
    parentFilter,
    list: Object.values(byId).sort((a: any, b: any) => newer(a.last_message_id, b.last_message_id) ? -1 : 1),
  };
}

const threadVerbs: Record<string, (c: ThreadCtx) => Promise<void>> = {
  list: async ({ log, pos, flag }) => {
    const id = pos[1];
    if (!id) throw new Error("usage: maw hermes threads list <channel-id|guild-id> [--all]");
    const { gid, parentFilter, list } = await fetchThreads(id, flag("--all"));
    const seen = loadSeen();
    if (!list.length) {
      log(parentFilter ? `(no threads under channel ${parentFilter})` : `(no active threads in guild ${gid})`);
      return;
    }
    for (const t of list) {
      const tag = newer(t.last_message_id, seen[t.id]) ? "\x1b[32mNEW\x1b[0m " : "    ";
      log(`${tag}thread \x1b[36m${t.name}\x1b[0m  (${t.id})  msgs=${t.message_count ?? "?"}${t._archived ? " [archived]" : ""}`);
    }
  },

  read: async ({ log, pos, flag }) => {
    const id = pos[1];
    if (!id) throw new Error("usage: maw hermes threads read <channel-id|guild-id> [--all]");
    const { gid, parentFilter, list } = await fetchThreads(id, flag("--all"));
    const seen = loadSeen();
    if (!list.length) {
      log(parentFilter ? `(no threads under channel ${parentFilter})` : `(no active threads in guild ${gid})`);
      return;
    }
    for (const t of list) {
      const tag = newer(t.last_message_id, seen[t.id]) ? "\x1b[32mNEW\x1b[0m " : "    ";
      log(`${tag}thread \x1b[36m${t.name}\x1b[0m  (${t.id})  msgs=${t.message_count ?? "?"}${t._archived ? " [archived]" : ""}`);
      const msgs = await discordApi(`/channels/${t.id}/messages?limit=50`);
      if (!msgs.ok) {
        log(`     (read HTTP ${msgs.status})`);
        continue;
      }
      for (const m of (msgs.data as any[]).slice().reverse()) {
        const mtag = newer(m.id, seen[t.id]) ? "\x1b[32mNEW\x1b[0m" : "   ";
        log(`     ${mtag} [${m.author.username}${m.author.bot ? " bot" : ""}] ${msgBody(m)}`);
      }
      const newest = (msgs.data as any[])[0]?.id;
      if (newer(newest, seen[t.id])) seen[t.id] = newest;
    }
    await saveSeen(seen);
  },

  create: async ({ log, pos }) => {
    const ch = pos[1];
    const name = pos.slice(2).join(" ");
    if (!ch || !name) throw new Error("usage: maw hermes threads create <channel-id> <name...>");
    const r = await discordApi(`/channels/${ch}/threads`, {
      method: "POST",
      body: JSON.stringify({ name, type: 11, auto_archive_duration: 1440 }),
    });
    if (!r.ok) throw new Error(`create HTTP ${r.status}: ${JSON.stringify(r.data)}`);
    log(`\x1b[32mcreated thread\x1b[0m \x1b[36m${r.data.name}\x1b[0m (${r.data.id}) under channel ${ch}`);
  },
};

export async function threads(log: Log, args: string[]): Promise<void> {
  const rest = args.slice(1);
  const flag = (f: string) => rest.includes(f);
  const pos = rest.filter((x) => !x.startsWith("--"));
  const verb = pos[0];

  if (verb && threadVerbs[verb]) {
    await threadVerbs[verb]({ log, pos, flag });
    return;
  }
  if (verb) {
    const fn = flag("--read") ? threadVerbs.read : threadVerbs.list;
    await fn({ log, pos: ["_", ...pos], flag });
    return;
  }

  log(`maw hermes threads - ${Object.keys(threadVerbs).length} verbs: ${Object.keys(threadVerbs).join(", ")}`);
  log("  list <ch|guild> [--all]      list threads (NEW = unseen)");
  log("  read <ch|guild> [--all]      list + read inside, advance cursor");
  log("  create <ch> <name...>        create a new public thread");
  log("  (legacy: threads <ch> [--read] [--all] still works)");
}

async function arra(args: string[]): Promise<string> {
  const tok = passShow(WEBHOOK_RELAY_TOKEN_PASS);
  const p = Bun.spawn(["bun", ARRA, ...args], {
    stdout: "pipe",
    stderr: "pipe",
    env: { ...process.env, WEBHOOK_RELAY_TOKEN: tok },
  });
  const out = await new Response(p.stdout).text();
  await p.exited;
  return out.trim();
}

async function lineChats(log: Log): Promise<void> {
  const out = await arra(["line", "chats"]);
  log(out || "no LINE chats found");
}

async function lineRead(log: Log, args: string[]): Promise<void> {
  const chatName = args[2] || "";
  const n = args[3] || "20";
  const a = ["line", "digest", "-d", "today", "-l", n];
  if (chatName) a.push("-g", chatName);
  a.push("--full");
  log(await arra(a) || "no LINE messages found");
}

async function lineSearch(log: Log, args: string[]): Promise<void> {
  const q = args.slice(2).join(" ");
  if (!q) {
    log("usage: maw hermes line search <keyword>");
    return;
  }
  log(await arra(["line", "history", "-q", q]) || `no LINE messages matching "${q}"`);
}

async function lineToday(log: Log, args: string[]): Promise<void> {
  const n = args[2] || "20";
  log(await arra(["hits", "-d", "today", "-l", n]) || "no webhook hits today");
}

async function lineHits(log: Log, args: string[]): Promise<void> {
  const date = args[2] || "today";
  const n = args[3] || "20";
  log(await arra(["hits", "-d", date, "-l", n]) || `no webhook hits for ${date}`);
}

async function lineGroups(log: Log, args: string[]): Promise<void> {
  const date = args[2] || "today";
  log(await arra(["line", "groups", "-d", date]) || `no active groups for ${date}`);
}

const lineVerbs: Record<string, (log: Log, args: string[]) => Promise<void>> = {
  chats: (log) => lineChats(log),
  read: (log, a) => lineRead(log, a),
  search: (log, a) => lineSearch(log, a),
  today: (log, a) => lineToday(log, a),
  hits: (log, a) => lineHits(log, a),
  groups: (log, a) => lineGroups(log, a),
};

export async function line(log: Log, args: string[]): Promise<void> {
  const cmd = args[1];
  const fn = cmd ? lineVerbs[cmd] : undefined;
  if (fn) return fn(log, args);
  log("maw hermes line - LINE messages via webhook-relay");
  log("  maw hermes line chats             - list LINE chats/groups");
  log("  maw hermes line read [chat] [n]   - read recent messages");
  log("  maw hermes line search <keyword>  - search messages");
  log("  maw hermes line today [n]         - real-time webhook hits (today)");
  log("  maw hermes line hits [date] [n]   - raw webhook hits by date");
  log("  maw hermes line groups [date]     - active LINE groups");
}

const serverApiVerbs: Record<string, (c: ServerCtx) => Promise<void>> = {
  health: async ({ log, flag, json }) => {
    const detailed = flag("--detailed");
    const r = await apiServer(detailed ? "/health/detailed" : "/health");
    if (json) return dump(log, r);
    log(r.ok
      ? `\x1b[32mapi server up\x1b[0m -> ${API_SERVER}${detailed ? "\n" + JSON.stringify(r.data, null, 2) : ""}`
      : `api server down (HTTP ${r.status})`);
  },

  sessions: async ({ log, json }) => {
    const r = await apiServer("/api/sessions");
    need(r);
    if (json) return dump(log, r);
    const list: any[] = Array.isArray(r.data) ? r.data : (r.data?.sessions ?? r.data?.data ?? []);
    for (const s of list) log(`${s.id ?? s.session_id}  ${s.source ?? ""}  ${s.title ?? ""}`);
  },

  create: async ({ log, opt, json }) => {
    const body: any = {};
    const id = opt("--id"); if (id) body.id = id;
    const title = opt("--title"); if (title) body.title = title;
    const model = opt("--model"); if (model) body.model = model;
    const r = await apiServer("/api/sessions", { method: "POST", body: JSON.stringify(body) });
    need(r);
    if (json) return dump(log, r);
    const s = r.data?.session ?? r.data;
    log(`\x1b[32mcreated\x1b[0m ${s?.id ?? "?"}  ${s?.title ?? ""}`);
  },

  get: async ({ log, pos }) => {
    const sid = pos[1];
    if (!sid) throw new Error("usage: maw hermes server-api get <session-id>");
    const r = await apiServer(`/api/sessions/${sid}`);
    need(r);
    log(JSON.stringify(r.data?.session ?? r.data, null, 2));
  },

  messages: async ({ log, pos, json }) => {
    const sid = pos[1];
    if (!sid) throw new Error("usage: maw hermes server-api messages <session-id>");
    const r = await apiServer(`/api/sessions/${sid}/messages`);
    need(r);
    if (json) return dump(log, r);
    const msgs: any[] = Array.isArray(r.data) ? r.data : (r.data?.messages ?? r.data?.data ?? []);
    for (const m of msgs) log(`\x1b[36m${m.role}\x1b[0m: ${typeof m.content === "string" ? m.content : JSON.stringify(m.content)}`);
  },

  chat: async ({ log, pos, flag, json }) => {
    const sid = pos[1];
    const text = pos.slice(2).join(" ");
    if (!sid || !text) throw new Error("usage: maw hermes server-api chat <session-id> <text...> [--no-origin]");
    const body: any = { message: text };
    if (!flag("--no-origin")) {
      const note = originNote(sid);
      if (note) body.system_message = note;
    }
    const r = await apiServer(`/api/sessions/${sid}/chat`, { method: "POST", body: JSON.stringify(body) });
    need(r);
    if (json) return dump(log, r);
    log(`\x1b[36mHermes\x1b[0m (session ${r.data?.session_id ?? sid}):`);
    log(r.data?.message?.content ?? JSON.stringify(r.data));
  },

  fork: async ({ log, pos }) => {
    const sid = pos[1];
    if (!sid) throw new Error("usage: maw hermes server-api fork <session-id>");
    const r = await apiServer(`/api/sessions/${sid}/fork`, { method: "POST" });
    need(r);
    log(`\x1b[32mforked\x1b[0m -> ${(r.data?.session ?? r.data)?.id ?? "?"}`);
  },

  patch: async ({ log, pos, opt }) => {
    const sid = pos[1];
    if (!sid) throw new Error("usage: maw hermes server-api patch <session-id> [--title <t>] [--end <reason>]");
    const body: any = {};
    const title = opt("--title"); if (title !== undefined) body.title = title;
    const end = opt("--end"); if (end !== undefined) body.end_reason = end;
    const r = await apiServer(`/api/sessions/${sid}`, { method: "PATCH", body: JSON.stringify(body) });
    need(r);
    log(`\x1b[32mpatched\x1b[0m ${sid}`);
  },

  delete: async ({ log, pos, flag }) => {
    const sid = pos[1];
    if (!sid) throw new Error("usage: maw hermes server-api delete <session-id> --yes");
    if (!flag("--yes")) throw new Error("refusing to delete without --yes (Nothing is Deleted - confirm intent)");
    const r = await apiServer(`/api/sessions/${sid}`, { method: "DELETE" });
    need(r);
    log(`\x1b[33mdeleted\x1b[0m ${sid}`);
  },

  raw: async ({ log, pos }) => {
    const method = (pos[1] || "GET").toUpperCase();
    const path = pos[2];
    const rawBody = pos.slice(3).join(" ") || undefined;
    if (!path) throw new Error("usage: maw hermes server-api raw <METHOD> <path> [json-body]");
    const init: any = { method };
    if (rawBody !== undefined) init.body = rawBody;
    const r = await apiServer(path, init);
    log(JSON.stringify(r.data, null, 2));
  },
};

export async function runServerApi(log: Log, a: string[]): Promise<void> {
  const flag = (f: string) => a.includes(f);
  const opt = (f: string) => {
    const i = a.indexOf(f);
    return i >= 0 ? a[i + 1] : undefined;
  };
  const pos = a.filter((x) => !x.startsWith("--"));
  const json = flag("--json");
  const verb = pos[0];
  const fn = verb ? serverApiVerbs[verb] : undefined;
  if (!fn) {
    log("maw hermes server-api - full :8642 API-server wrapper");
    log(`  ${Object.keys(serverApiVerbs).length} verbs: ${Object.keys(serverApiVerbs).join(", ")}`);
    log("  health [--detailed] | sessions | create [--id --title --model]");
    log("  get <id> | messages <id> | chat <id> <text> [--no-origin] | fork <id>");
    log("  patch <id> [--title --end] | delete <id> --yes | raw <METHOD> <path> [body]");
    log("  (append --json to dump the raw response body)");
    return;
  }
  await fn({ log, pos, flag, opt, json });
}

const commands: Record<string, (log: Log, args: string[]) => Promise<void>> = {
  whoami: (log) => whoami(log),
  send: (log, a) => send(log, a),
  read: (log, a) => read(log, a),
  channels: (log) => channels(log),
  threads: (log, a) => threads(log, a),
  chat: (log, a) => chat(log, a),
  sessions: (log) => runServerApi(log, ["sessions"]),
  health: (log) => runServerApi(log, ["health"]),
  line: (log, a) => line(log, a),
  "server-api": (log, a) => runServerApi(log, a.slice(1)),
  api: (log, a) => runServerApi(log, a.slice(1)),
};

export const command = {
  name: "hermes",
  description: "Hermes bridge - Discord REST bot verbs plus API-server user turns.",
};

export async function cmdHermes(args: string[], log: Log = console.log): Promise<void> {
  const sub = args[0];
  const fn = sub ? commands[sub] : undefined;
  if (fn) await fn(log, args);
  else help(log);
}

export default async function handler(ctx: any) {
  const buf: string[] = [];
  const log: Log = (s = "") => (ctx?.writer ? ctx.writer(s) : buf.push(s));
  try {
    const args: string[] = Array.isArray(ctx?.args) ? ctx.args : [];
    await cmdHermes(args, log);
    return { ok: true, output: buf.join("\n") || undefined };
  } catch (e: any) {
    return { ok: false, error: e?.message || String(e) };
  }
}

function help(log: Log): void {
  log("maw hermes - Discord REST (bot) + API-server user turns + threads");
  log(`  ${Object.keys(commands).length} commands: ${Object.keys(commands).join(", ")}`);
  log("  # bot level - no LLM, agent does NOT see");
  log("  whoami | send <ch> <text> | read <ch> [n] | channels | threads <ch|guild> [--read] [--all]");
  log("  # user turn - Hermes runs + replies with context");
  log("  chat <sid> <text> [--discord] | sessions | health");
  log("  # nested full :8642 wrapper");
  log(`  server-api <verb>   (${Object.keys(serverApiVerbs).length} verbs: ${Object.keys(serverApiVerbs).join(", ")})`);
  log("  # LINE read-only helper");
  log("  line <chats|read|search|today|hits|groups>");
}

if (import.meta.main) {
  try {
    await cmdHermes(process.argv.slice(2));
  } catch (e: any) {
    console.error(e?.message || String(e));
    process.exit(1);
  }
}
