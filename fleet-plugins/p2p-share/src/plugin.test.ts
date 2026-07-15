import { expect, test } from "bun:test";
import handler, {
  DEFAULT_SIGNAL_URL,
  UNAUTHENTICATED_RISK_FLAG,
  VIEWER_PORT,
  captureSnapshot,
  createViewerFetchHandler,
  createViewerServerOptions,
  getFlag,
  handleP2pShare,
  initializeOpenViewer,
  loadWerift,
  parseShareOptions,
  requiresUnauthenticatedRiskAcknowledgement,
  renderViewerHtml,
  sendDataChannelTextToPane,
  shareTargetError,
} from "./plugin";
import { readFileSync } from "node:fs";

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

test("unauthenticated share requires explicit risk acknowledgement", async () => {
  const output: string[] = [];
  const exitCode = await handleP2pShare(["share", "session:0.0"], (line) => output.push(line), "");

  expect(exitCode).toBe(1);
  expect(output.join("\n")).toContain("SECURITY BLOCK");
  expect(output.join("\n")).toContain("Remote viewers can send keystrokes");
  expect(output.join("\n")).toContain(UNAUTHENTICATED_RISK_FLAG);
  expect(requiresUnauthenticatedRiskAcknowledgement(["share", "pane"], "secret")).toBe(false);
  expect(requiresUnauthenticatedRiskAcknowledgement(
    ["share", "pane", UNAUTHENTICATED_RISK_FLAG],
    "",
  )).toBe(false);
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

test("DataChannel text is sent literally to tmux and newlines become Enter", () => {
  const calls: string[][] = [];
  const spawnSync = (cmd: string[]) => {
    calls.push(cmd);
    return { exitCode: 0, stderr: { toString: () => "" } };
  };

  sendDataChannelTextToPane("session:window.0", "echo FLEETPAD_E2E_123\r\npwd", () => {}, spawnSync);

  expect(calls).toEqual([
    ["tmux", "send-keys", "-t", "session:window.0", "-l", "--", "echo FLEETPAD_E2E_123"],
    ["tmux", "send-keys", "-t", "session:window.0", "Enter"],
    ["tmux", "send-keys", "-t", "session:window.0", "-l", "--", "pwd"],
  ]);
});

test("DataChannel binary payload is decoded before tmux input", () => {
  const calls: string[][] = [];
  const payload = new TextEncoder().encode("hello\n").buffer;

  sendDataChannelTextToPane("pane", payload, () => {}, (cmd: string[]) => {
    calls.push(cmd);
    return { exitCode: 0, stderr: { toString: () => "" } };
  });

  expect(calls).toEqual([
    ["tmux", "send-keys", "-t", "pane", "-l", "--", "hello"],
    ["tmux", "send-keys", "-t", "pane", "Enter"],
  ]);
});

test("viewer config is injected safely and is not exposed by /config", async () => {
  const secret = "signal-secret</script>";
  const template = readFileSync(new URL("../viewer.html", import.meta.url), "utf8");
  const html = renderViewerHtml(template, {
    signalUrl: "wss://signal.local/ws",
    peerName: "share-test",
    target: "session:0.0",
    authKey: secret,
  });
  const fetchViewer = createViewerFetchHandler(html);

  expect(createViewerServerOptions(7742, html).hostname).toBe("127.0.0.1");
  expect(html).not.toContain("</script></script>");
  expect(html).toContain('"authKey":"signal-secret\\u003c/script>"');
  expect(await (await fetchViewer(new Request("http://127.0.0.1/config"))).text()).not.toContain(secret);
  expect((await fetchViewer(new Request("http://127.0.0.1/config"))).status).toBe(404);
  expect(await (await fetchViewer(new Request("http://127.0.0.1/"))).text()).toBe(html);
});

test("each concurrent viewer receives dims when its own DataChannel opens", () => {
  const viewers = new Map<string, { pc: unknown; dc: unknown }>();
  const firstMessages: Buffer[] = [];
  const secondMessages: Buffer[] = [];
  const channel = (messages: Buffer[]) => ({
    readyState: "open",
    send(data: Buffer) { messages.push(data); },
  });

  initializeOpenViewer(viewers, "viewer-1", {}, channel(firstMessages), { cols: 120, rows: 40 }, () => {});
  initializeOpenViewer(viewers, "viewer-2", {}, channel(secondMessages), { cols: 120, rows: 40 }, () => {});

  const expected = Buffer.from(JSON.stringify({ type: "dims", cols: 120, rows: 40 }));
  expect(viewers.size).toBe(2);
  expect(firstMessages).toEqual([expected]);
  expect(secondMessages).toEqual([expected]);
});

test("served viewer sends terminal input and keeps rendering received bytes", async () => {
  const template = readFileSync(new URL("../viewer.html", import.meta.url), "utf8");
  const html = renderViewerHtml(template, {
    signalUrl: "wss://signal.local/ws",
    peerName: "share-test",
    target: "session:0.0",
    authKey: "secret",
  });
  const response = createViewerFetchHandler(html)(new Request("http://127.0.0.1/"));
  const servedViewer = await response.text();

  expect(servedViewer).toContain("disableStdin: false");
  expect(servedViewer).toMatch(/term\.onData\(\(data\).*dc\.send\(data\)/s);
  expect(servedViewer).toContain("term.write(bytes)");
  expect(servedViewer).toContain("term.write(arr)");
});

test("capture-pane validation rejects a target before the viewer server or signaling starts", async () => {
  expect(() => captureSnapshot("missing:9.9", () => ({
    exitCode: 1,
    stderr: { toString: () => "can't find pane: missing:9.9" },
  }))).toThrow("tmux capture-pane failed for 'missing:9.9'");

  const calls: string[][] = [];
  const output: string[] = [];
  const exitCode = await handleP2pShare(["share", ":0.0"], (line) => output.push(line), "secret", (cmd) => {
    calls.push(cmd);
    return cmd[1] === "capture-pane" ? {
      exitCode: 1,
      stderr: { toString: () => "can't find pane: :0.0" },
    } : { exitCode: 0, stdout: { toString: () => "0" } };
  });

  expect(exitCode).toBe(1);
  expect(output.join("\n")).toContain("Target pane ':0.0' cannot be captured");
  expect(output.join("\n")).not.toContain("P2P Share starting");
  expect(calls).toEqual([
    ["tmux", "capture-pane", "-t", ":0.0", "-e", "-p"],
  ]);
});

test("pre-existing tmux pipe is rejected instead of silently streaming zero bytes", async () => {
  const calls: string[][] = [];
  const spawnSync = (cmd: string[]) => {
    calls.push(cmd);
    return {
      exitCode: 0,
      stdout: { toString: () => cmd[1] === "display-message" ? "1\n" : "snapshot" },
    };
  };

  expect(shareTargetError("live:0.0", spawnSync)).toContain("already has a tmux pipe");
  expect(calls).toEqual([
    ["tmux", "capture-pane", "-t", "live:0.0", "-e", "-p"],
    ["tmux", "display-message", "-t", "live:0.0", "-p", "#{pane_pipe}"],
  ]);
  const output: string[] = [];
  const exitCode = await handleP2pShare(
    ["share", "live:0.0"],
    (line) => output.push(line),
    "secret",
    spawnSync,
  );

  expect(exitCode).toBe(1);
  expect(output.join("\n")).toContain("stop it before sharing");
  expect(output.join("\n")).not.toContain("P2P Share starting");
});
