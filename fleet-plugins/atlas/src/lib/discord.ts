/**
 * Discord REST API client — low-level methods split per endpoint.
 */

const API = "https://discord.com/api/v10";
const UA = "maw-atlas/1.0.0";

export function getToken(): string | null {
  if (process.env.DISCORD_BOT_TOKEN) return process.env.DISCORD_BOT_TOKEN;
  try {
    const { execSync } = require("child_process");
    return execSync("pass show discord/atlas-oracle-token 2>/dev/null", { encoding: "utf8" }).trim() || null;
  } catch { return null; }
}

async function request(path: string, token: string, method = "GET", body?: any): Promise<any> {
  const res = await fetch(`${API}${path}`, {
    method,
    headers: {
      Authorization: `Bot ${token}`,
      "Content-Type": "application/json",
      "User-Agent": UA,
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error(`Discord ${res.status} ${method} ${path}`);
  const txt = await res.text();
  try { return JSON.parse(txt); } catch { return { raw: txt }; }
}

// ── Identity ──

export async function getMe(token: string) {
  return request("/users/@me", token);
}

// ── Guilds ──

export async function listGuilds(token: string) {
  return request("/users/@me/guilds", token);
}

export async function getGuildChannels(token: string, guildId: string) {
  return request(`/guilds/${guildId}/channels`, token);
}

// ── Channels ──

export async function getChannel(token: string, channelId: string) {
  return request(`/channels/${channelId}`, token);
}

export async function createChannel(token: string, guildId: string, name: string, type = 0, parentId?: string) {
  return request(`/guilds/${guildId}/channels`, token, "POST", {
    name, type, ...(parentId ? { parent_id: parentId } : {}),
  });
}

export async function deleteChannel(token: string, channelId: string) {
  return request(`/channels/${channelId}`, token, "DELETE");
}

export async function moveChannel(token: string, channelId: string, parentId: string) {
  return request(`/channels/${channelId}`, token, "PATCH", { parent_id: parentId });
}

// ── Messages ──

export async function getMessages(token: string, channelId: string, limit = 100, before?: string) {
  let path = `/channels/${channelId}/messages?limit=${Math.min(limit, 100)}`;
  if (before) path += `&before=${before}`;
  return request(path, token);
}

export async function postMessage(token: string, channelId: string, content: string) {
  return request(`/channels/${channelId}/messages`, token, "POST", { content });
}

// ── Threads ──

export async function createThread(token: string, channelId: string, name: string, autoArchiveDuration = 10080) {
  return request(`/channels/${channelId}/threads`, token, "POST", {
    name, type: 11, auto_archive_duration: autoArchiveDuration,
  });
}

export async function createThreadFromMessage(token: string, channelId: string, messageId: string, name: string, autoArchiveDuration = 10080) {
  return request(`/channels/${channelId}/messages/${messageId}/threads`, token, "POST", {
    name, auto_archive_duration: autoArchiveDuration,
  });
}

export async function deleteThread(token: string, threadId: string) {
  return request(`/channels/${threadId}`, token, "DELETE");
}

export async function joinThread(token: string, threadId: string) {
  const res = await fetch(`https://discord.com/api/v10/channels/${threadId}/thread-members/@me`, {
    method: "PUT", headers: { Authorization: `Bot ${token}`, "User-Agent": "maw-atlas/1.0.0" },
  });
  return { ok: res.ok, status: res.status };
}

export async function addThreadMember(token: string, threadId: string, userId: string) {
  const res = await fetch(`https://discord.com/api/v10/channels/${threadId}/thread-members/${userId}`, {
    method: "PUT", headers: { Authorization: `Bot ${token}`, "User-Agent": "maw-atlas/1.0.0" },
  });
  return { ok: res.ok, status: res.status };
}

export async function archiveThread(token: string, threadId: string) {
  return request(`/channels/${threadId}`, token, "PATCH", { archived: true });
}

// ── Slash Commands (Application Commands) ──

export async function listSlashCommands(token: string, appId: string, guildId?: string) {
  const path = guildId
    ? `/applications/${appId}/guilds/${guildId}/commands`
    : `/applications/${appId}/commands`;
  return request(path, token);
}

export async function registerSlashCommands(token: string, appId: string, commands: any[], guildId?: string) {
  const path = guildId
    ? `/applications/${appId}/guilds/${guildId}/commands`
    : `/applications/${appId}/commands`;
  return request(path, token, "PUT", commands);
}

export async function deleteSlashCommand(token: string, appId: string, commandId: string, guildId?: string) {
  const path = guildId
    ? `/applications/${appId}/guilds/${guildId}/commands/${commandId}`
    : `/applications/${appId}/commands/${commandId}`;
  return request(path, token, "DELETE");
}

// ── Invites ──

export async function resolveInvite(token: string, code: string) {
  const clean = code.replace(/.*discord\.gg\//, "").replace(/.*invite\//, "");
  return request(`/invites/${clean}`, token);
}

// ── Application Settings ──

export async function getApplication(token: string, appId: string) {
  return request(`/applications/${appId}`, token);
}

export async function updateApplication(token: string, appId: string, data: any) {
  return request(`/applications/${appId}`, token, "PATCH", data);
}

// ── Bot Avatar ──

export async function setBotAvatar(token: string, avatarBase64: string) {
  return request("/users/@me", token, "PATCH", { avatar: avatarBase64 });
}

// ── Helpers ──

export function filterTextChannels(channels: any[]): any[] {
  return Array.isArray(channels) ? channels.filter(c => c.type === 0) : [];
}

export function filterVoiceChannels(channels: any[]): any[] {
  return Array.isArray(channels) ? channels.filter(c => c.type === 2) : [];
}
