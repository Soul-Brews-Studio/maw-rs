import { getMe, getApplication, updateApplication } from "../lib/discord";

export async function app(log: (s: string) => void, token: string, args: string[]) {
  const me = await getMe(token);
  const appId = me.id;
  const sub = args[1];

  if (!sub || sub === "help") {
    log("usage:");
    log("  maw atlas app                          show app settings");
    log("  maw atlas app interactions <url>        set Interactions Endpoint URL");
    log("  maw atlas app interactions --clear      remove Interactions Endpoint URL");
    return;
  }

  if (sub === "interactions") {
    const url = args[2];
    if (!url) {
      const info = await getApplication(token, appId);
      log(`current: ${info.interactions_endpoint_url || "(not set)"}`);
      return;
    }
    if (url === "--clear") {
      await updateApplication(token, appId, { interactions_endpoint_url: "" });
      log("✓ Interactions Endpoint URL cleared");
      return;
    }
    await updateApplication(token, appId, { interactions_endpoint_url: url });
    log(`✓ Interactions Endpoint URL set: ${url}`);
    return;
  }

  const info = await getApplication(token, appId);
  log(`${info.name} (${info.id})`);
  log(`  description: ${info.description || "(none)"}`);
  log(`  interactions_endpoint_url: ${info.interactions_endpoint_url || "(not set)"}`);
  log(`  public_key: ${info.verify_key}`);
  log(`  bot_public: ${info.bot_public}`);
  log(`  flags: ${info.flags}`);
}
