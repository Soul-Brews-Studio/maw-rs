// squad (maw-rs dev tier) — form a team of real oracles, the basic way. Loud & clear.
//
// Ported verbatim from athena-oracle/.maw/plugins/squad/impl.ts (the locked model),
// with one dev-tier adaptation: maw-rs runs bun-dev plugins with cwd = the plugin
// directory, so `process.cwd()` no longer points at the lead repo. We derive the
// invoking directory from $PWD (inherited from the maw process, which the shell set
// to the lead's repo). The M2 WASM ship port uses `maw.paths.get cwd` instead — the
// InvokeContext-supplied cwd — so the ship tier never depends on $PWD.
//
//   The lead IS the team. No wrapper entity:
//     run `maw squad start` FROM the lead oracle's repo → team = repo name minus "-oracle"
//     (athena-oracle → team "athena", lead = athena). join/say/ls are implicit to that team.
//
//   A team = one folder:  ~/.claude/teams/<team>/  { config.json + inboxes/*.json }
//   join   = run the oracle's claude with --team-name <team>, from its OWN repo (= identity)
//   say    = append JSON into inboxes/<member>.json ; the member polls it between turns
//
import { existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { spawnSync } from "child_process";

const TEAMS = join(homedir(), ".claude", "teams");
// invalid --agent-color makes the spawn fail SILENTLY (learned the hard way: odin/orange)
const COLORS = ["red", "green", "yellow", "blue", "purple", "cyan", "magenta", "white"];
// names become file paths + tmux session names — no dots, slashes, or ../ traversal
const NAME_RE = /^[a-z0-9][a-z0-9_-]*$/i;
const dirOf = (team: string) => join(TEAMS, team);
const cfgOf = (team: string) => join(dirOf(team), "config.json");

// dev-tier cwd: maw-rs chdir's bun into the plugin dir, so $PWD (inherited from the
// invoking shell) is the lead repo, not process.cwd(). Fall back to cwd if unset.
const invokeDir = (): string => process.env.PWD || process.cwd();

function sh(cmd: string, args: string[]): string {
  const r = spawnSync(cmd, args, { encoding: "utf-8" });
  return (r.stdout || "").trim();
}

// team = this repo's name minus "-oracle". The repo you stand in IS the lead.
function hereTeam(): { team: string; repo: string } {
  const cwd = invokeDir();
  const repo = sh("git", ["-C", cwd, "rev-parse", "--show-toplevel"]) || cwd;
  const team = (repo.split("/").pop() || "").replace(/-oracle$/, "");
  if (!team) throw new Error("can't derive a team name from this directory");
  return { team, repo };
}

// maw squad start — start THIS repo's squad. Adopts an existing team folder, never clobbers.
function cmdStart() {
  const { team, repo } = hereTeam();
  mkdirSync(join(dirOf(team), "inboxes"), { recursive: true });
  const existed = existsSync(cfgOf(team));
  const cfg: any = existed
    ? JSON.parse(readFileSync(cfgOf(team), "utf-8"))
    : { name: team, members: [], createdAt: Date.now() };
  if (!cfg.leadSessionId)
    cfg.leadSessionId = sh("maw", ["team-agent", "uuid", "--bare"]).split("\n")[0] || String(Date.now());
  cfg.leadRepo = repo;
  writeFileSync(cfgOf(team), JSON.stringify(cfg, null, 2) + "\n");
  // the teams runtime delivers member→lead messages to team-lead.json, not lead.json
  const leadIbx = join(dirOf(team), "inboxes", "team-lead.json");
  if (!existsSync(leadIbx)) writeFileSync(leadIbx, "[]\n");
  console.log(`⚡ squad '${team}' ${existed ? "adopted (already existed)" : "started"} → ${dirOf(team)}`);
  console.log(`   lead: ${repo.split("/").pop()} (this repo)   lead session: ${cfg.leadSessionId}`);
  console.log(`   replies arrive in: inboxes/team-lead.json`);
  console.log(`   next: maw squad join digger   ·   maw squad say digger "<text>"   ·   maw squad ls`);
}

// maw squad join <oracle> [color] — spawn <oracle> into THIS repo's squad, from its own repo
function cmdJoin(role: string, color = "cyan") {
  if (!role) throw new Error("usage: maw squad join <oracle> [color]");
  if (!NAME_RE.test(role)) throw new Error(`invalid oracle name '${role}' (letters/digits/-/_ only)`);
  if (!COLORS.includes(color))
    throw new Error(`invalid color '${color}' — spawn would fail SILENTLY. valid: ${COLORS.join(" ")}`);
  const { team } = hereTeam();
  if (!existsSync(cfgOf(team)))
    throw new Error(`squad '${team}' not started — run: maw squad start (from the lead repo)`);
  // ONE oracle, ONE session: the session IS the oracle (named plain `digger`, not `athena-digger`)
  // — team membership is a boot flag, not a new identity. Also: maw new exits 0 WITHOUT
  // respawning when a session already exists (even as a dead shell), so "✓ joined" would lie.
  const clash = sh("tmux", ["ls", "-F", "#S"]).split("\n")
    .filter((s) => s === role || s.endsWith(`-${role}`));
  if (clash.length)
    throw new Error(
      `${role} already has a live session: ${clash.join(", ")} — one oracle, one session.` +
      `   attach: maw a ${role}   or kill first: tmux kill-session -t ${clash[0]}`);
  const loc = sh("maw", ["locate", role]);
  const repo = (loc.match(/repo:\s*(\S+)/) || [])[1];
  if (!repo) throw new Error(`can't locate repo for '${role}' — try: maw locate ${role}`);
  const cfg = JSON.parse(readFileSync(cfgOf(team), "utf-8"));
  const claudeCmd = [
    "direnv exec . env CLAUDECODE=1 CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 claude",
    `--agent-id ${role}@${team}`, `--agent-name ${role}`, `--team-name ${team}`,
    `--agent-color ${color}`, `--parent-session-id ${cfg.leadSessionId}`, "--dangerously-skip-permissions",
  ].join(" ");
  console.log(`⚡ ${role} → squad '${team}'   (repo: ${repo})`);
  // post maw-rs cutover: `maw new` is now the PLUGIN SCAFFOLD verb (--rust/--as) —
  // session-create lives only in mawjs until maw-rs grows a native equivalent
  const r = spawnSync("mawjs", ["new", role, "--path", repo, "--no-attach", "--print", "--cmd", claudeCmd],
    { stdio: ["inherit", "pipe", "inherit"], encoding: "utf-8" });
  if (r.status !== 0) throw new Error(`maw new failed for ${role}`);
  cfg.members = (cfg.members || []).filter((m: any) => m.name !== role);
  cfg.members.push({ agentId: `${role}@${team}`, name: role, color, repo, joinedAt: Date.now() });
  writeFileSync(cfgOf(team), JSON.stringify(cfg, null, 2) + "\n");
  const ibx = join(dirOf(team), "inboxes", `${role}.json`);
  if (!existsSync(ibx)) writeFileSync(ibx, "[]\n");
  console.log(`  ✓ ${role} joined. say: maw squad say ${role} "<text>"   view: maw a ${role}`);
}

// maw squad say <member> <text...> — append a message to a member's inbox (never clobbers)
function cmdSay(member: string, text: string) {
  if (!member || !text) throw new Error("usage: maw squad say <member> <text>");
  if (!NAME_RE.test(member)) throw new Error(`invalid member name '${member}' (letters/digits/-/_ only)`);
  const { team } = hereTeam();
  if (!existsSync(cfgOf(team)))
    throw new Error(`squad '${team}' not started — run: maw squad start (from the lead repo)`);
  // only roster members poll an inbox — saying to anyone else is silent message loss
  const cfg = JSON.parse(readFileSync(cfgOf(team), "utf-8"));
  const names = ((cfg.members || []) as any[]).map((m) => m.name);
  if (!names.includes(member))
    throw new Error(
      `'${member}' is not in squad '${team}' — members: ${names.join(", ") || "(none)"}.` +
      `   join first: maw squad join ${member}`);
  const p = join(dirOf(team), "inboxes", `${member}.json`);
  const msgs = existsSync(p) ? JSON.parse(readFileSync(p, "utf-8")) : [];
  msgs.push({
    from: "team-lead", text, timestamp: new Date().toISOString(),
    color: "cyan", type: "message", read: false,
  });
  writeFileSync(p, JSON.stringify(msgs, null, 2) + "\n");
  console.log(`✓ said to ${member}@${team}: ${text}`);
}

// maw squad ls — show THIS repo's squad: members + inboxes + live tmux
function cmdLs() {
  const { team } = hereTeam();
  console.log(`squad: ${team}`);
  if (existsSync(cfgOf(team))) {
    const cfg = JSON.parse(readFileSync(cfgOf(team), "utf-8"));
    console.log(`  lead: ${(cfg.leadRepo || "?").split("/").pop()}   session: ${cfg.leadSessionId || "?"}`);
    const members = (cfg.members || []) as any[];
    if (members.length) for (const m of members) console.log(`  member: ${m.name} (${m.color})  ${m.repo || ""}`);
    else console.log("  members: (none yet — maw squad join <oracle>)");
  } else console.log("  (not started — maw squad start)");
  const ibx = join(dirOf(team), "inboxes");
  if (existsSync(ibx)) {
    const parts = readdirSync(ibx).filter((f: string) => f.endsWith(".json")).map((f: string) => {
      try {
        const unread = JSON.parse(readFileSync(join(ibx, f), "utf-8")).filter((m: any) => !m.read).length;
        return f.replace(/\.json$/, "") + (unread ? ` (${unread} unread)` : "");
      } catch { return f + " (unreadable)"; }
    });
    console.log(`  inboxes: ${parts.join(", ")}`);
  }
  // a member's session is named after the oracle itself (one oracle, one session)
  const live = existsSync(cfgOf(team))
    ? (JSON.parse(readFileSync(cfgOf(team), "utf-8")).members || [])
        .map((m: any) => m.name)
        .filter((n: string) => spawnSync("tmux", ["has-session", "-t", `=${n}`]).status === 0)
    : [];
  console.log(`  live tmux: ${live.length ? live.join(", ") : "(none)"}`);
}

export function cmdSquad(args: string[]) {
  const [sub, a2, ...rest] = args;
  switch (sub) {
    case "start": return cmdStart();
    case "join": return cmdJoin(a2, rest[0]);
    case "say": return cmdSay(a2, rest.join(" "));
    case "ls": return cmdLs();
    default: printUsage();
  }
}

function printUsage() {
  console.log("maw squad — the lead IS the team. Run from the lead oracle's repo; team = repo name.");
  console.log("  maw squad start                  start this repo's squad (athena-oracle → 'athena')");
  console.log("  maw squad join <oracle> [color]  spawn <oracle> into this squad (own repo = identity)");
  console.log("  maw squad say  <member> <text>   append a message to a member's inbox");
  console.log("  maw squad ls                     show this squad: members + inboxes + live tmux");
}

// bun-dev entry: maw-rs dispatches `bun impl.ts <args...>`. Run loud, fail loud —
// a thrown guard prints to stderr and exits non-zero so the caller sees it.
if (import.meta.main) {
  try {
    cmdSquad(process.argv.slice(2));
  } catch (e: any) {
    console.error(e?.message || String(e));
    process.exit(1);
  }
}
