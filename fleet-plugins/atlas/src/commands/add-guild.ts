import { resolveInvite, getGuildChannels, filterTextChannels } from "../lib/discord";

export async function addGuild(log: (s: string) => void, token: string, args: string[]) {
  const input = args[1];
  if (!input) { log("usage: maw atlas add-guild <invite-code-or-guild-id>"); return; }

  let guildId = input;
  if (!/^\d+$/.test(input)) {
    const invite = await resolveInvite(token, input);
    guildId = invite.guild?.id;
    log(`resolved invite → ${invite.guild?.name} (${guildId})`);
  }

  const channels = await getGuildChannels(token, guildId);
  if (!Array.isArray(channels)) { log(`✗ can't access guild ${guildId}`); return; }
  const text = filterTextChannels(channels);
  log(`${text.length} text channels found`);
  for (const ch of text) log(`  💬 #${ch.name} (${ch.id})`);
}
