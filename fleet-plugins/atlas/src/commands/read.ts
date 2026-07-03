import { getMessages, listGuilds, getGuildChannels, filterTextChannels } from "../lib/discord";

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

export async function read(log: (s: string) => void, token: string, args: string[]) {
  const channel = args[1];
  const limit = parseInt(args.find(a => a.startsWith("--limit="))?.split("=")[1] || "5");
  const json = args.includes("--json");
  const tree = args.includes("--tree") || (!channel && !json);

  if (tree) {
    await readTree(log, token, limit, json);
    return;
  }

  if (!channel) { log("usage: maw atlas read <channel> [--limit=N] [--json] [--tree]"); return; }

  const channelId = await resolveChannel(token, channel);
  if (!channelId) { log(`✗ channel not found: ${channel}`); return; }

  const msgs = await getMessages(token, channelId, limit);
  if (!Array.isArray(msgs)) { log(`✗ ${JSON.stringify(msgs)}`); return; }
  msgs.reverse();

  if (json) {
    const out = msgs.map(m => ({
      id: m.id,
      author: m.author?.username,
      bot: !!m.author?.bot,
      content: m.content || "",
      timestamp: m.timestamp?.slice(0, 19),
      attachments: m.attachments?.length || 0,
    }));
    log(JSON.stringify(out, null, 2));
    return;
  }

  for (const m of msgs) {
    const ts = m.timestamp?.slice(0, 16) || "?";
    const bot = m.author?.bot ? " 🤖" : "";
    log(`${ts} ${m.author?.username}${bot}: ${m.content?.slice(0, 200) || "(no content)"}`);
  }
  log(`\n${msgs.length} messages`);
}

async function readTree(log: (s: string) => void, token: string, limit: number, json: boolean) {
  const guilds = await listGuilds(token);
  if (!Array.isArray(guilds)) { log("✗ could not list guilds"); return; }

  if (json) {
    const result: any[] = [];
    for (const g of guilds) {
      const channels = await getGuildChannels(token, g.id);
      if (!Array.isArray(channels)) continue;
      const text = filterTextChannels(channels);
      const threads = await listActiveThreads(token, g.id);
      const guildData: any = { guild: g.name, channels: [] };
      for (const ch of text) {
        try {
          const msgs = await getMessages(token, ch.id, limit);
          if (!Array.isArray(msgs) || msgs.length === 0) continue;
          guildData.channels.push({
            name: ch.name, id: ch.id,
            messages: msgs.reverse().map((m: any) => ({
              id: m.id, author: m.author?.username, bot: !!m.author?.bot,
              content: m.content || "", timestamp: m.timestamp?.slice(0, 19),
            })),
            threads: threads.filter((t: any) => t.parent_id === ch.id).map((t: any) => ({ id: t.id, name: t.name })),
          });
        } catch { /* no access */ }
      }
      if (guildData.channels.length) result.push(guildData);
    }
    log(JSON.stringify(result, null, 2));
    return;
  }

  for (const g of guilds) {
    const channels = await getGuildChannels(token, g.id);
    if (!Array.isArray(channels)) continue;
    const text = filterTextChannels(channels);
    const threads = await listActiveThreads(token, g.id);
    const threadsByParent = new Map<string, any[]>();
    for (const t of threads) {
      const arr = threadsByParent.get(t.parent_id) || [];
      arr.push(t);
      threadsByParent.set(t.parent_id, arr);
    }

    const activeChannels: any[] = [];
    for (const ch of text) {
      try {
        const msgs = await getMessages(token, ch.id, limit);
        if (Array.isArray(msgs) && msgs.length > 0) {
          activeChannels.push({ ch, msgs: msgs.reverse(), threads: threadsByParent.get(ch.id) || [] });
        } else if (threadsByParent.has(ch.id)) {
          activeChannels.push({ ch, msgs: [], threads: threadsByParent.get(ch.id) || [] });
        }
      } catch { /* no access */ }
    }

    if (activeChannels.length === 0) continue;
    log(`${g.name}`);

    for (let ci = 0; ci < activeChannels.length; ci++) {
      const { ch, msgs, threads: chThreads } = activeChannels[ci];
      const isLastCh = ci === activeChannels.length - 1;
      const cPrefix = isLastCh ? "└── " : "├── ";
      const cCont = isLastCh ? "    " : "│   ";

      log(`${cPrefix}#${ch.name}`);

      for (let mi = 0; mi < msgs.length; mi++) {
        const m = msgs[mi];
        const bot = m.author?.bot ? " 🤖" : "";
        const snippet = (m.content || "(no content)").slice(0, 80).replace(/\n/g, " ");
        const isLastMsg = mi === msgs.length - 1 && chThreads.length === 0;
        const mPrefix = isLastMsg ? "└── " : "├── ";
        log(`${cCont}${mPrefix}${m.author?.username}${bot}: ${snippet}`);
      }

      for (let ti = 0; ti < chThreads.length; ti++) {
        const t = chThreads[ti];
        const isLastThread = ti === chThreads.length - 1;
        const tPrefix = isLastThread ? "└── " : "├── ";
        log(`${cCont}${tPrefix}🧵 ${t.name}`);
      }
    }
    log("");
  }
}
