/**
 * discord-db — dedicated sqlite store for polled Discord messages.
 *
 * Separate from ~/.maw/message-ledger.sqlite (that is the maw-federation ledger,
 * with federation-specific enums/identities and no read-cursor). This table is
 * the "load Discord data into our DB" half of the polling router.
 *
 * Idempotency: the Discord snowflake `message_id` is the PRIMARY KEY, so
 * `INSERT OR IGNORE` makes overlapping polls a no-op — no content hash needed.
 * The read-cursor itself stays in .maw/atlas-route/last-seen.json (single-writer
 * map, no query need) — this table is only the queryable message archive.
 */
import { Database } from "bun:sqlite";
import { mkdirSync } from "fs";
import { dirname } from "path";

export interface DiscordMsgRow {
  message_id: string;
  channel_id: string;
  thread_id: string | null;
  guild_id: string | null;
  author_id: string;
  author_name: string | null;
  author_is_bot: number;
  content: string | null;
  attachments_json: string | null;
  ts: string;
  created_at: string;
}

export interface MessageStore {
  insert: (row: DiscordMsgRow) => void;
  markRouted: (messageId: string, routedTo: string, routedAt: string) => void;
  count: () => number;
  close: () => void;
}

const SCHEMA = `
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS discord_messages (
  message_id       TEXT PRIMARY KEY,
  channel_id       TEXT NOT NULL,
  thread_id        TEXT,
  guild_id         TEXT,
  author_id        TEXT NOT NULL,
  author_name      TEXT,
  author_is_bot    INTEGER NOT NULL DEFAULT 0,
  content          TEXT,
  attachments_json TEXT,
  ts               TEXT NOT NULL,
  routed_to        TEXT,
  routed_at        TEXT,
  created_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dm_channel_ts ON discord_messages(channel_id, ts);
CREATE INDEX IF NOT EXISTS idx_dm_thread     ON discord_messages(thread_id);
CREATE INDEX IF NOT EXISTS idx_dm_author     ON discord_messages(author_id);
`;

/** Open (or create) the message store. The returned object owns the single DB handle. */
export function openMessageStore(dbPath: string): MessageStore {
  mkdirSync(dirname(dbPath), { recursive: true });
  const db = new Database(dbPath);
  db.exec(SCHEMA);

  const insertStmt = db.prepare(
    `INSERT OR IGNORE INTO discord_messages
       (message_id, channel_id, thread_id, guild_id, author_id, author_name,
        author_is_bot, content, attachments_json, ts, routed_to, routed_at, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?)`,
  );
  const routedStmt = db.prepare(
    `UPDATE discord_messages SET routed_to = ?, routed_at = ? WHERE message_id = ?`,
  );
  const countStmt = db.query("SELECT count(*) AS c FROM discord_messages");

  return {
    insert: (r) =>
      insertStmt.run(
        r.message_id, r.channel_id, r.thread_id, r.guild_id, r.author_id,
        r.author_name, r.author_is_bot, r.content, r.attachments_json, r.ts, r.created_at,
      ),
    markRouted: (id, to, at) => routedStmt.run(to, at, id),
    count: () => (countStmt.get() as { c: number }).c,
    close: () => db.close(),
  };
}
