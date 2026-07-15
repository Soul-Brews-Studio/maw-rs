#!/usr/bin/env bun
// Regenerates differential fixtures. Usage: MAW_JS_REPO=/path/to/maw-js bun scripts/gen-routing-corpus.ts
import { writeFileSync, mkdirSync } from "node:fs"; import { join, resolve } from "node:path";
const repo = process.env.MAW_JS_REPO ?? "../maw-js";
const mod = await import(`file://${resolve(repo)}/src/core/routing.ts`);
const resolveTarget = mod.resolveTarget ?? mod.default?.resolveTarget;
if (typeof resolveTarget !== "function") throw new Error("maw-js src/core/routing.ts must export resolveTarget");
const baseConfig = { node: "selfnode", namedPeers: [{ name: "peerbox", url: "http://peerbox.local:3457" }], peers: ["http://farbox.wg:3458"], agents: { remoteagent: "peerbox", selfagent: "selfnode", nopurlagent: "ghostnode" } };
function norm(r: any): any { return r.type === "peer" ? { type: "peer", peerUrl: r.peerUrl, target: r.target, node: r.node } : r.type === "local" ? { type: "local", target: r.target } : r.type === "self-node" ? { type: "self-node", target: r.target } : { type: "error", reason: r.reason, detail: r.detail, hint: r.hint ?? null }; }
const cases: any[] = [];
for (let i = 0; i < 50; i++) {
  const sess = `${String(i).padStart(2, "0")}-maw-rs`, win = `codex-${i}`;
  cases.push({ name: `exact local ${i}`, query: win, sessions: [{ name: sess, windows: [{ index: 1, name: win, active: true }] }] });
  cases.push({ name: `explicit window ${i}`, query: `${sess}:${win}.0`, sessions: [{ name: sess, windows: [{ index: 2, name: "other", active: false }, { index: 7, name: win, active: true }] }] });
  cases.push({ name: `peer map ${i}`, query: `peerbox:agent-${i}`, sessions: [] });
  cases.push({ name: `unknown ${i}`, query: `ghost-${i}`, sessions: [] });
}
for (const c of cases) c.expected = norm(resolveTarget(c.query, baseConfig, c.sessions));
const out = "crates/maw-routing/tests/fixtures/differential/generated.json";
mkdirSync(join(out, ".."), { recursive: true }); writeFileSync(out, JSON.stringify({ baseConfig, cases }) + "\n");
console.log(`wrote ${cases.length} cases to ${out}`);
