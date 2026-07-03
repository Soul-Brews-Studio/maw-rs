import { getMe, setBotAvatar } from "../lib/discord";
import { readFileSync, existsSync } from "fs";
import { resolveFromInvoke } from "../lib/repo";

function toDataUri(filePath: string): string {
  const abs = resolveFromInvoke(filePath);
  if (!existsSync(abs)) throw new Error(`file not found: ${abs}`);
  const buf = readFileSync(abs);
  const ext = abs.split(".").pop()?.toLowerCase();
  const mime = ext === "png" ? "image/png"
    : ext === "jpg" || ext === "jpeg" ? "image/jpeg"
    : ext === "gif" ? "image/gif"
    : ext === "webp" ? "image/webp"
    : "image/png";
  return `data:${mime};base64,${buf.toString("base64")}`;
}

export async function avatar(log: (s: string) => void, token: string, args: string[]) {
  const sub = args[1];

  if (sub === "set") {
    const filePath = args[2];
    if (!filePath) { log("usage: maw atlas avatar set <path-to-image>"); return; }
    const dataUri = toDataUri(filePath);
    const sizeKB = Math.round(Buffer.from(dataUri).length / 1024);
    log(`uploading avatar (${sizeKB}KB)...`);
    const result = await setBotAvatar(token, dataUri);
    log(`✓ avatar updated: ${result.username} (${result.id})`);
    if (result.avatar) {
      log(`  https://cdn.discordapp.com/avatars/${result.id}/${result.avatar}.png`);
    }
    return;
  }

  const me = await getMe(token);
  if (me.avatar) {
    log(`current avatar: https://cdn.discordapp.com/avatars/${me.id}/${me.avatar}.png`);
  } else {
    log("no avatar set");
  }
  log("");
  log("usage: maw atlas avatar set <path-to-image>");
  log("  supports: PNG, JPG, GIF, WebP");
}
