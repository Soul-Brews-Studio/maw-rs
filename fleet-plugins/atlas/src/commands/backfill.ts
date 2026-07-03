/**
 * maw atlas backfill — guild-aware Discord channel message backfill.
 *
 * Wraps the backfill script in atlas-oracle repo, or runs standalone
 * Discord REST fetch if the script isn't available.
 *
 * Output splitting (keeps dumps manageable, mirrors scripts/backfill-channels.ts):
 *   default      → <channel>.json (single file)
 *   --chunk=N    → <channel>.partNN.json + <channel>.index.json (split-style, by count)
 *   --by-date    → <channel>.YYYY-MM-DD.json + index (csplit-style, Bangkok day)
 * --by-date wins if both are given.
 */
import { findAtlasRepo, resolveFromInvoke } from "../lib/repo";
import { listGuilds, getGuildChannels, getMessages, filterTextChannels } from "../lib/discord";
import { writeFileSync, mkdirSync, existsSync } from "fs";
import { join } from "path";

const DISCORD_EPOCH = 1420070400000n;
// Discord snowflake → Bangkok (GMT+7) calendar day "YYYY-MM-DD"
const snowflakeToBkkDay = (id: string) =>
  new Date(Number((BigInt(id) >> 22n) + DISCORD_EPOCH) + 7 * 3600 * 1000).toISOString().slice(0, 10);
const sanitize = (s: string) => s.replace(/[^a-zA-Z0-9฀-๿_-]/g, "_").slice(0, 60);

/** Write a channel's messages, optionally split. Returns a short log label. */
function writeChannel(outDir: string, chName: string, messages: any[], chunkSize: number, byDate: boolean): string {
  const base = sanitize(chName);
  const writeJson = (f: string, d: any) => writeFileSync(join(outDir, f), JSON.stringify(d, null, 2) + "\n");
  const writeIndex = (mode: string, parts: { file: string; count: number }[], extra: any = {}) =>
    writeJson(`${base}.index.json`, { channel: chName, mode, total: messages.length, parts, ...extra });

  if (byDate && messages.length) {
    const groups = new Map<string, any[]>();
    for (const m of messages) {
      const day = snowflakeToBkkDay(m.id);
      let arr = groups.get(day);
      if (!arr) { arr = []; groups.set(day, arr); }
      arr.push(m);
    }
    const parts: { file: string; count: number }[] = [];
    for (const [day, msgs] of [...groups.entries()].sort()) {
      const f = `${base}.${day}.json`;
      writeJson(f, msgs);
      parts.push({ file: f, count: msgs.length });
    }
    writeIndex("by-date", parts, { tz: "Asia/Bangkok" });
    return `${parts.length} day-files`;
  }

  if (chunkSize > 0 && messages.length > chunkSize) {
    const parts: { file: string; count: number }[] = [];
    for (let i = 0; i < messages.length; i += chunkSize) {
      const slice = messages.slice(i, i + chunkSize);
      const f = `${base}.part${String(parts.length + 1).padStart(2, "0")}.json`;
      writeJson(f, slice);
      parts.push({ file: f, count: slice.length });
    }
    writeIndex("chunk", parts, { chunkSize });
    return `${parts.length} parts ×≤${chunkSize}`;
  }

  writeJson(`${base}.json`, messages);
  return `${base}.json`;
}

export async function backfill(log: (s: string) => void, token: string, args: string[]) {
  const repo = findAtlasRepo();
  const scriptPath = repo ? join(repo, "scripts/backfill-channels.ts") : null;

  // if atlas-oracle repo has the full script, delegate to it
  if (scriptPath && existsSync(scriptPath)) {
    const { execFileSync } = require("child_process");
    const rest = args.slice(1); // keep as an arg array — never shell-join (arg-injection)
    log(`delegating to ${scriptPath}`);
    try {
      // token + state-dir go through env, NOT the command line (no ps/argv leak);
      // script + args are passed as an argv array via execFileSync (no shell = no injection).
      execFileSync("bun", [scriptPath, ...rest], {
        stdio: "inherit",
        env: {
          ...process.env,
          DISCORD_STATE_DIR: `${repo}/.discord`,
          DISCORD_BOT_TOKEN: token,
        },
      });
    } catch (e: any) {
      log(`backfill exited: ${e.message || e}`);
    }
    return;
  }

  // standalone mode — fetch directly
  const fetchAll = args.includes("--all");
  const listOnly = args.includes("--list");
  const guildFilter = args.find(a => a.startsWith("--guild="))?.split("=")[1];
  const limit = fetchAll ? 10000 : parseInt(args.find(a => a.startsWith("--limit="))?.split("=")[1] || "100");
  const chunkSize = parseInt(args.find(a => a.startsWith("--chunk="))?.split("=")[1] || "0");
  const byDate = args.includes("--by-date");

  const guilds = await listGuilds(token);
  log(`${guilds.length} guild(s)`);

  for (const g of guilds) {
    if (guildFilter && !g.id.includes(guildFilter) && !g.name.toLowerCase().includes(guildFilter.toLowerCase())) continue;

    const channels = await getGuildChannels(token, g.id);
    if (!Array.isArray(channels)) { log(`  ✗ ${g.name}: access denied`); continue; }
    const text = filterTextChannels(channels);
    log(`\n${g.name} — ${text.length} text channels`);

    if (listOnly) {
      for (const ch of text) log(`  💬 #${ch.name} (${ch.id})`);
      continue;
    }

    const outDir = repo ? join(repo, ".discord/backfill", sanitize(g.name)) : resolveFromInvoke(join("backfill", sanitize(g.name)));
    mkdirSync(outDir, { recursive: true });

    let guildTotal = 0;
    for (const ch of text) {
      const all: any[] = [];
      let before: string | undefined;

      while (all.length < limit) {
        const batch = await getMessages(token, ch.id, 100, before);
        if (!batch.length) break;
        all.push(...batch);
        before = batch[batch.length - 1].id;
        if (batch.length < 100) break;
        await new Promise(r => setTimeout(r, 500));
      }

      all.sort((a: any, b: any) => a.id.localeCompare(b.id));
      const written = writeChannel(outDir, ch.name, all, chunkSize, byDate);
      log(`  ✓ #${ch.name}: ${all.length} msgs → ${written}`);
      guildTotal += all.length;

      await new Promise(r => setTimeout(r, 1000));
    }
    log(`  total: ${guildTotal} msgs`);
  }
}
