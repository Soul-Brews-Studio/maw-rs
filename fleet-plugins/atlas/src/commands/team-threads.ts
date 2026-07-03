import { listGuilds, getGuildChannels, createThread } from "../lib/discord";
import { invokeDir } from "../lib/repo";

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

async function joinThread(token: string, threadId: string) {
  await fetch(`https://discord.com/api/v10/channels/${threadId}/thread-members/@me`, {
    method: "PUT",
    headers: { Authorization: `Bot ${token}`, "User-Agent": "maw-atlas/1.0.0" },
  });
}

async function postMessage(token: string, channelId: string, content: string) {
  await fetch(`https://discord.com/api/v10/channels/${channelId}/messages`, {
    method: "POST",
    headers: {
      Authorization: `Bot ${token}`,
      "Content-Type": "application/json",
      "User-Agent": "maw-atlas/1.0.0",
    },
    body: JSON.stringify({ content }),
  });
}

export async function teamThreads(log: (s: string) => void, token: string, args: string[]) {
  const sub = args[1];
  const channel = args[2] || "102-atlas-oracle";

  if (!sub || sub === "help") {
    log("usage:");
    log("  maw atlas team-threads sync [channel]    create threads for each worktree agent");
    log("  maw atlas team-threads list [channel]    list agent threads");
    log("  maw atlas team-threads clean [channel]   archive empty agent threads");
    log("");
    log("Reads .maw/teams/*.yaml to find team members, creates matching Discord threads.");
    log("Default channel: 102-atlas-oracle");
    return;
  }

  const channelId = await resolveChannel(token, channel);
  if (!channelId) { log(`✗ channel not found: ${channel}`); return; }

  const guilds = await listGuilds(token);
  let allThreads: any[] = [];
  for (const g of guilds) {
    const threads = await listActiveThreads(token, g.id);
    allThreads = allThreads.concat(threads.filter((t: any) => t.parent_id === channelId));
  }

  if (sub === "list") {
    const agentThreads = allThreads.filter((t: any) => t.name.startsWith("codex-") || t.name === "codex" || t.name.startsWith("atlas-"));
    if (agentThreads.length === 0) { log("no agent threads found"); return; }
    for (const t of agentThreads) {
      log(`  🧵 #${t.name} (${t.id})`);
    }
    log(`\n${agentThreads.length} agent threads`);
    return;
  }

  if (sub === "sync") {
    const { readFileSync, existsSync } = require("fs");
    const { resolve } = require("path");

    const charterPath = resolve(invokeDir(), ".maw/teams/atlas-m5.yaml");
    if (!existsSync(charterPath)) {
      log("✗ .maw/teams/atlas-m5.yaml not found"); return;
    }

    const yaml = readFileSync(charterPath, "utf8");
    const members = [...yaml.matchAll(/name:\s*(\S+)/g)].map(m => m[1]);
    const agents = members.filter(n => n !== "atlas-oracle");

    const existingNames = new Set(allThreads.map((t: any) => t.name));

    let created = 0;
    for (const agent of agents) {
      const threadName = agent.replace("atlas-", "");
      if (existingNames.has(threadName)) {
        log(`  ✓ #${threadName} exists`);
        continue;
      }
      const thread = await createThread(token, channelId, threadName);
      await joinThread(token, thread.id);
      await postMessage(token, thread.id, `🌍 ${threadName} thread — worktree \`agents/1-${agent}/\`\n\n— [m5:atlas]`);
      log(`  + #${threadName} created (${thread.id})`);
      created++;
    }
    log(`\n${created} created, ${agents.length - created} already existed`);
    return;
  }

  log(`unknown: ${sub} — run 'maw atlas team-threads help'`);
}
