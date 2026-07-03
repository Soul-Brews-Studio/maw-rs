import {
  listGuilds, getGuildChannels, postMessage,
  createThread, createThreadFromMessage, deleteThread,
  joinThread, addThreadMember, archiveThread,
} from "../lib/discord";

async function resolveChannel(token: string, input: string): Promise<string | null> {
  if (/^\d{17,20}$/.test(input)) return input;
  const clean = input.replace(/^#/, "").toLowerCase();
  const guilds = await listGuilds(token);
  if (!Array.isArray(guilds)) return null;
  for (const g of guilds) {
    const channels = await getGuildChannels(token, g.id);
    if (!Array.isArray(channels)) continue;
    const match = channels.find((c: any) =>
      c.name?.toLowerCase() === clean ||
      c.name?.toLowerCase().includes(clean)
    );
    if (match) return match.id;
  }
  return null;
}

async function listActiveThreads(token: string, guildId: string): Promise<any[]> {
  const res = await fetch(`https://discord.com/api/v10/guilds/${guildId}/threads/active`, {
    headers: { Authorization: `Bot ${token}`, "User-Agent": "maw-atlas/1.0.0" },
  });
  if (!res.ok) return [];
  const data = await res.json() as any;
  return data.threads || [];
}

async function resolveThread(token: string, nameOrId: string): Promise<string | null> {
  if (/^\d{17,20}$/.test(nameOrId)) return nameOrId;
  const clean = nameOrId.replace(/^#/, "").toLowerCase();
  const guilds = await listGuilds(token);
  if (!Array.isArray(guilds)) return null;
  for (const g of guilds) {
    const threads = await listActiveThreads(token, g.id);
    const match = threads.find((t: any) => t.name?.toLowerCase() === clean || t.name?.toLowerCase().includes(clean));
    if (match) return match.id;
  }
  return null;
}

export async function threads(log: (s: string) => void, token: string, args: string[]) {
  const sub = args[1];

  if (!sub || sub === "help") {
    log("usage:");
    log("  maw atlas threads                          list active threads");
    log("  maw atlas threads create <ch> <name>       create thread (empty)");
    log("  maw atlas threads open <ch> <name>         create thread with starter message + join");
    log("  maw atlas threads delete <name-or-id>      delete thread");
    log("  maw atlas threads archive <name-or-id>     archive thread");
    log("  maw atlas threads join <name-or-id>        bot joins thread");
    log("  maw atlas threads add <thread> <user-id>   add user to thread");
    log("  maw atlas threads --json                   JSON output");
    return;
  }

  if (sub === "create") {
    const channel = args[2];
    const name = args.slice(3).filter(a => !a.startsWith("--")).join(" ");
    if (!channel || !name) { log("usage: maw atlas threads create <channel> <thread-name>"); return; }
    const channelId = await resolveChannel(token, channel);
    if (!channelId) { log(`✗ channel not found: ${channel}`); return; }
    const thread = await createThread(token, channelId, name);
    log(`✓ #${thread.name} created (${thread.id})`);
    return;
  }

  if (sub === "open") {
    const channel = args[2];
    const name = args.slice(3).filter(a => !a.startsWith("--")).join(" ");
    if (!channel || !name) { log("usage: maw atlas threads open <channel> <thread-name>"); return; }
    const channelId = await resolveChannel(token, channel);
    if (!channelId) { log(`✗ channel not found: ${channel}`); return; }
    const msg = await postMessage(token, channelId, `🤖 **${name}** — thread workspace`);
    const thread = await createThreadFromMessage(token, channelId, msg.id, name);
    await joinThread(token, thread.id);
    log(`✓ #${thread.name} opened (${thread.id}) — with starter message + bot joined`);
    return;
  }

  if (sub === "delete" || sub === "rm") {
    const target = args[2];
    if (!target) { log("usage: maw atlas threads delete <name-or-id>"); return; }
    const threadId = await resolveThread(token, target);
    if (!threadId) { log(`✗ thread not found: ${target}`); return; }
    await deleteThread(token, threadId);
    log(`✓ thread deleted (${threadId})`);
    return;
  }

  if (sub === "archive") {
    const target = args[2];
    if (!target) { log("usage: maw atlas threads archive <name-or-id>"); return; }
    const threadId = await resolveThread(token, target);
    if (!threadId) { log(`✗ thread not found: ${target}`); return; }
    await archiveThread(token, threadId);
    log(`✓ thread archived (${threadId})`);
    return;
  }

  if (sub === "join") {
    const target = args[2];
    if (!target) { log("usage: maw atlas threads join <name-or-id>"); return; }
    const threadId = await resolveThread(token, target);
    if (!threadId) { log(`✗ thread not found: ${target}`); return; }
    const r = await joinThread(token, threadId);
    log(r.ok ? `✓ bot joined thread (${threadId})` : `✗ join failed: ${r.status}`);
    return;
  }

  if (sub === "add") {
    const target = args[2];
    const userId = args[3];
    if (!target || !userId) { log("usage: maw atlas threads add <thread> <user-id>"); return; }
    const threadId = await resolveThread(token, target);
    if (!threadId) { log(`✗ thread not found: ${target}`); return; }
    const r = await addThreadMember(token, threadId, userId);
    log(r.ok ? `✓ user ${userId} added to thread (${threadId})` : `✗ add failed: ${r.status}`);
    return;
  }

  // default: list
  const guilds = await listGuilds(token);
  if (!Array.isArray(guilds)) { log("✗ could not list guilds"); return; }

  const json = args.includes("--json");
  const allThreads: any[] = [];

  for (const g of guilds) {
    const gThreads = await listActiveThreads(token, g.id);
    if (!gThreads.length) continue;
    if (!json) log(`${g.name} — ${gThreads.length} active threads`);
    for (const t of gThreads) {
      if (json) {
        allThreads.push({ id: t.id, name: t.name, guild: g.name, parent_id: t.parent_id });
      } else {
        log(`  🧵 #${t.name} (${t.id}) ← parent: ${t.parent_id}`);
      }
    }
  }

  if (json) {
    log(JSON.stringify(allThreads, null, 2));
  } else if (allThreads.length === 0) {
    log("no active threads found");
  }
}
