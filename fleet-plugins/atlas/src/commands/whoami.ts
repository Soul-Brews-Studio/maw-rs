import { getMe, listGuilds } from "../lib/discord";

export async function whoami(log: (s: string) => void, token: string) {
  const me = await getMe(token);
  log(`${me.username} (${me.id}) — bot: ${me.bot}`);
  const guilds = await listGuilds(token);
  log(`${guilds.length} guild(s):`);
  for (const g of guilds) log(`  ${g.name} (${g.id})`);
}
