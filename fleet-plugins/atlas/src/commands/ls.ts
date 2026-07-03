import { listGuilds, getGuildChannels, filterTextChannels, filterVoiceChannels } from "../lib/discord";

export async function ls(log: (s: string) => void, token: string) {
  const guilds = await listGuilds(token);
  for (const g of guilds) {
    const channels = await getGuildChannels(token, g.id);
    if (!Array.isArray(channels)) { log(`  ✗ ${g.name}: access denied`); continue; }
    const text = filterTextChannels(channels);
    const voice = filterVoiceChannels(channels);
    log(`${g.name} — ${text.length} text, ${voice.length} voice`);
    for (const ch of text) log(`  💬 #${ch.name} (${ch.id})`);
    for (const ch of voice) log(`  🔊 ${ch.name} (${ch.id})`);
    log("");
  }
}
