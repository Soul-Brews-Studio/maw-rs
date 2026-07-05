import { afterEach, beforeEach, expect, test } from "bun:test";
import { cmdHermes } from "./plugin";

const TOP_LEVEL_VERBS = [
  "whoami",
  "send",
  "read",
  "channels",
  "threads",
  "chat",
  "sessions",
  "health",
  "line",
  "server-api",
  "api",
];

const DISCORD_BOT_CASES: Array<{ name: string; args: string[] }> = [
  { name: "whoami", args: ["whoami"] },
  { name: "send", args: ["send", "1234567890", "hello"] },
  { name: "read", args: ["read", "1234567890", "2"] },
  { name: "channels", args: ["channels"] },
  { name: "threads", args: ["threads", "list", "1234567890"] },
];

const savedEnv = {
  DISCORD_BOT_TOKEN: process.env.DISCORD_BOT_TOKEN,
  MAW_HERMES_DISABLE_PASS: process.env.MAW_HERMES_DISABLE_PASS,
};
const originalFetch = globalThis.fetch;
let fetchCalls = 0;

beforeEach(() => {
  delete process.env.DISCORD_BOT_TOKEN;
  process.env.MAW_HERMES_DISABLE_PASS = "1";
  fetchCalls = 0;
  globalThis.fetch = (async () => {
    fetchCalls += 1;
    throw new Error("test failed: Hermes attempted real HTTP");
  }) as typeof fetch;
});

afterEach(() => {
  restoreEnv("DISCORD_BOT_TOKEN", savedEnv.DISCORD_BOT_TOKEN);
  restoreEnv("MAW_HERMES_DISABLE_PASS", savedEnv.MAW_HERMES_DISABLE_PASS);
  globalThis.fetch = originalFetch;
});

test("no args prints help listing all top-level verbs", async () => {
  const out = await runHermes([]);

  expect(out).toContain("maw hermes");
  expect(out).toContain("11 commands");
  for (const verb of TOP_LEVEL_VERBS) {
    expect(out).toContain(verb);
  }
});

test("unknown verb prints the help hint", async () => {
  const out = await runHermes(["bogus"]);

  expect(out).toContain("unknown hermes verb 'bogus'");
  expect(out).toContain("run maw hermes --help");
});

for (const { name, args } of DISCORD_BOT_CASES) {
  test(`${name} fails clearly without Discord token before network`, async () => {
    const { error, output } = await runHermesError(args);

    expect(output).toBe("");
    expect(error).toMatch(/missing token\/config/i);
    expect(error).toContain("DISCORD_BOT_TOKEN");
    expect(fetchCalls).toBe(0);
  });
}

async function runHermes(args: string[]): Promise<string> {
  const lines: string[] = [];
  await cmdHermes(args, (s = "") => lines.push(s));
  return lines.join("\n");
}

async function runHermesError(args: string[]): Promise<{ error: string; output: string }> {
  const lines: string[] = [];
  try {
    await cmdHermes(args, (s = "") => lines.push(s));
  } catch (e: any) {
    return { error: e?.message || String(e), output: lines.join("\n") };
  }
  throw new Error(`expected Hermes ${args.join(" ")} to fail`);
}

function restoreEnv(key: keyof typeof savedEnv, value: string | undefined): void {
  if (value === undefined) delete process.env[key];
  else process.env[key] = value;
}
