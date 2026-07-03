import { expect, test } from "bun:test";
import handler, { DEFAULT_SIGNAL_URL, VIEWER_PORT, getFlag, loadWerift, parseShareOptions } from "./plugin";

async function run(args: string[]) {
  return handler({ source: "cli", args });
}

function expectUsageBanner(output: string): void {
  expect(output).toContain("maw p2p-share - WebRTC P2P terminal sharing");
  expect(output).toContain("maw p2p-share share <pane>");
}

test("no args, status, and help print usage banner", async () => {
  for (const args of [[], ["status"], ["help"]]) {
    const result = await run(args);
    expect(result.ok).toBe(true);
    expect(result.exitCode).toBe(0);
    expectUsageBanner(result.output);
  }
});

test("share without pane prints pane usage error", async () => {
  const result = await run(["share"]);

  expect(result.ok).toBe(false);
  expect(result.exitCode).toBe(1);
  expect(result.output).toContain("Usage: maw p2p-share share <pane>");
  expect(result.error).toContain("Usage: maw p2p-share share <pane>");
});

test("flag and share option parsing returns explicit values and defaults", () => {
  const explicitArgs = [
    "share",
    "mawjs-oracle:0.0",
    "--signal",
    "wss://signal.local/ws",
    "--name=custom-peer",
    "--port",
    "9090",
  ];

  expect(getFlag(explicitArgs, "--signal")).toBe("wss://signal.local/ws");
  expect(getFlag(explicitArgs, "--name")).toBe("custom-peer");
  expect(getFlag(explicitArgs, "--port")).toBe("9090");
  expect(parseShareOptions(explicitArgs)).toEqual({
    target: "mawjs-oracle:0.0",
    signalUrl: "wss://signal.local/ws",
    peerName: "custom-peer",
    port: 9090,
  });

  expect(parseShareOptions(["share", "mawjs-oracle:0.0"])).toEqual({
    target: "mawjs-oracle:0.0",
    signalUrl: DEFAULT_SIGNAL_URL,
    peerName: "share-mawjs-oracle-0-0",
    port: VIEWER_PORT,
  });

  expect(parseShareOptions(["share", "pane", "--port", "not-a-port"]).port).toBe(VIEWER_PORT);
});

test("loadWerift reports missing dependency with install hint", async () => {
  await expect(loadWerift(async () => {
    throw new Error("Cannot find package 'werift'");
  })).rejects.toThrow(/missing dependency 'werift'.*Run `bun install` in fleet-plugins\/p2p-share/s);
});
