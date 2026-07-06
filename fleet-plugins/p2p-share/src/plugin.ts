// maw p2p-share (maw-rs bun-dev tier) - WebRTC P2P terminal sharing.
//
// Ported from ~/.maw/plugins/p2p-share. maw-rs runs bun-dev plugins with cwd =
// the plugin directory; this plugin intentionally serves the committed viewer
// asset from the plugin directory and streams a tmux pane via subprocess calls.

import { readFileSync, unlinkSync } from "fs";
import { join } from "path";
import { Buffer } from "buffer";

declare const Bun: any;
declare const process: any;

type Log = (msg: string) => void;
type Dims = { cols: number; rows: number };
type PtyStream = { stop: () => void };
type SpawnSync = (cmd: string[]) => { exitCode?: number; stderr?: { toString(): string } };
type Werift = {
  RTCPeerConnection: any;
  RTCSessionDescription: any;
  RTCIceCandidate: any;
};

export const VIEWER_PORT = 7742;
export const DEFAULT_SIGNAL_URL = "wss://phd-signaling.laris.workers.dev/ws";

export const command = {
  name: "p2p-share",
  description: "Share tmux panes via WebRTC P2P - no server, no tunnel, no port forwarding.",
};

function usage(log: Log): void {
  log("maw p2p-share - WebRTC P2P terminal sharing");
  log("  No server needed. No tunnel. No port forwarding.");
  log("  Built by PhD Oracle using the phd-dropbox WebRTC stack.");
  log("");
  log("Usage:");
  log("  maw p2p-share share <pane> [--signal <url>] [--name <name>] [--port <port>]");
  log("  maw p2p-share status");
}

export function getFlag(args: string[], flag: string): string {
  const exact = args.find((arg) => arg.startsWith(`${flag}=`));
  if (exact) return exact.slice(flag.length + 1);
  const idx = args.indexOf(flag);
  if (idx === -1 || idx + 1 >= args.length) return "";
  return args[idx + 1];
}

function sanitizePeerName(value: string): string {
  return value.replace(/[:.]/g, "-");
}

export function parseShareOptions(args: string[]): {
  target: string;
  signalUrl: string;
  peerName: string;
  port: number;
} {
  const target = args[1] || "";
  const signalUrl = getFlag(args, "--signal") || DEFAULT_SIGNAL_URL;
  const peerName = getFlag(args, "--name") || `share-${sanitizePeerName(target)}`;
  const parsedPort = Number.parseInt(getFlag(args, "--port") || String(VIEWER_PORT), 10);
  const port = Number.isFinite(parsedPort) && parsedPort > 0 ? parsedPort : VIEWER_PORT;
  return { target, signalUrl, peerName, port };
}

function sanitizeTmpSuffix(value: string): string {
  return value.replace(/[^A-Za-z0-9_-]/g, "-").replace(/-+/g, "-").slice(0, 80) || "pane";
}

function getPaneDimensions(target: string): Dims {
  const r = Bun.spawnSync(["tmux", "display-message", "-t", target, "-p", "#{pane_width} #{pane_height}"]);
  if (r.exitCode !== 0) return { cols: 80, rows: 24 };
  const [cols, rows] = r.stdout.toString().trim().split(" ").map(Number);
  return { cols: cols || 80, rows: rows || 24 };
}

function captureSnapshot(target: string): string {
  const r = Bun.spawnSync(["tmux", "capture-pane", "-t", target, "-e", "-p"]);
  if (r.exitCode !== 0) return "";
  return r.stdout.toString().replace(/\n/g, "\r\n");
}

function dataChannelPayloadToText(data: unknown): string {
  if (typeof data === "string") return data;
  if (data instanceof Buffer) return data.toString("utf8");
  if (data instanceof ArrayBuffer) return Buffer.from(data).toString("utf8");
  if (ArrayBuffer.isView(data)) return Buffer.from(data.buffer, data.byteOffset, data.byteLength).toString("utf8");
  return "";
}

function runTmuxInput(target: string, args: string[], action: string, log: Log, spawnSync: SpawnSync): void {
  const r = spawnSync(["tmux", "send-keys", "-t", target, ...args]);
  if (r.exitCode !== 0) log(`tmux ${action} failed: ${r.stderr?.toString() || "unknown error"}`);
}

export function sendDataChannelTextToPane(
  target: string,
  data: unknown,
  log: Log = () => {},
  spawnSync: SpawnSync = Bun.spawnSync,
): void {
  const text = dataChannelPayloadToText(data);
  if (!text) return;

  let literal = "";
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (ch !== "\n" && ch !== "\r") {
      literal += ch;
      continue;
    }

    if (literal) runTmuxInput(target, ["-l", "--", literal], "literal input", log, spawnSync);
    literal = "";
    runTmuxInput(target, ["Enter"], "enter", log, spawnSync);
    if (ch === "\r" && text[i + 1] === "\n") i += 1;
  }

  if (literal) runTmuxInput(target, ["-l", "--", literal], "literal input", log, spawnSync);
}

function startPtyStream(
  target: string,
  onData: (chunk: Buffer) => void,
  onDims: (dims: Dims) => void,
  log: Log,
): PtyStream {
  let running = true;
  const dims = getPaneDimensions(target);
  onDims(dims);

  const snapshot = captureSnapshot(target);
  if (snapshot) onData(Buffer.from("\x1b[2J\x1b[H" + snapshot));
  log(`Dims ${dims.cols}x${dims.rows} + snapshot ${snapshot.length} bytes (CRLF fixed)`);

  const fifoPath = `/tmp/maw-p2p-pty-${sanitizeTmpSuffix(target)}-${Date.now()}.fifo`;
  const mkfifo = Bun.spawnSync(["mkfifo", fifoPath]);
  if (mkfifo.exitCode !== 0) {
    log(`mkfifo failed: ${mkfifo.stderr.toString()}`);
    throw new Error("mkfifo failed");
  }

  const reader = Bun.spawn(["cat", fifoPath], { stdout: "pipe" });
  Bun.spawnSync(["tmux", "pipe-pane", "-O", "-o", "-t", target, `cat > ${fifoPath}`]);
  log("Live stream: tmux pipe-pane -O -o -> FIFO -> cat reader");

  (async () => {
    try {
      for await (const chunk of reader.stdout) {
        if (!running) break;
        onData(Buffer.from(chunk));
      }
    } catch (err) {
      if (running) log(`FIFO reader error: ${err}`);
    }
  })();

  return {
    stop() {
      running = false;
      reader.kill();
      Bun.spawnSync(["tmux", "pipe-pane", "-t", target]);
      try { unlinkSync(fifoPath); } catch {}
    },
  };
}

export async function loadWerift(
  importer: (specifier: string) => Promise<unknown> = (specifier) => import(specifier),
): Promise<Werift> {
  try {
    return await importer("werift") as Werift;
  } catch (err) {
    throw new Error(
      `missing dependency 'werift' (${err instanceof Error ? err.message : String(err)}). ` +
      "Run `bun install` in fleet-plugins/p2p-share before starting a share.",
    );
  }
}

async function startSharePeer(opts: {
  target: string;
  signalUrl: string;
  peerName: string;
  log: Log;
}): Promise<void> {
  const { target, signalUrl, peerName, log } = opts;
  const { RTCPeerConnection, RTCSessionDescription, RTCIceCandidate } = await loadWerift();

  const peerConns = new Map<string, any>();
  const viewers = new Map<string, { pc: any; dc: any }>();
  const pendingIce = new Map<string, any[]>();
  const remoteReady = new Set<string>();
  let ptyStream: PtyStream | null = null;
  let ws: WebSocket;

  function broadcastToViewers(data: Buffer): void {
    for (const viewer of viewers.values()) {
      try {
        if (viewer.dc.readyState === "open") viewer.dc.send(data);
      } catch {}
    }
  }

  function broadcastDims(dims: Dims): void {
    const msg = JSON.stringify({ type: "dims", cols: dims.cols, rows: dims.rows });
    for (const viewer of viewers.values()) {
      try {
        if (viewer.dc.readyState === "open") viewer.dc.send(Buffer.from(msg));
      } catch {}
    }
  }

  function stopViewer(id: string): void {
    const pc = peerConns.get(id);
    if (pc) {
      try { pc.close(); } catch {}
      peerConns.delete(id);
    }
    viewers.delete(id);
    pendingIce.delete(id);
    remoteReady.delete(id);
    if (viewers.size === 0 && ptyStream) {
      ptyStream.stop();
      ptyStream = null;
      log("No viewers - PTY stream paused");
    }
  }

  async function handleOffer(msg: { from: string; sdp: { sdp: string; type: string } }): Promise<void> {
    log(`Offer from viewer ${msg.from}`);

    const pc = new RTCPeerConnection({
      iceServers: [
        { urls: "stun:stun.l.google.com:19302" },
        { urls: "stun:stun1.l.google.com:19302" },
      ],
    });
    peerConns.set(msg.from, pc);

    pc.onIceCandidate.subscribe((candidate: any) => {
      ws.send(JSON.stringify({
        type: "ice-candidate",
        target: msg.from,
        candidate: candidate.toJSON(),
      }));
    });

    const dc = pc.createDataChannel("pty-stream", { ordered: true });
    dc.onmessage = (event: { data?: unknown }) => {
      sendDataChannelTextToPane(target, event?.data, log);
    };
    dc.stateChanged.subscribe?.((state: string) => {
      log(`DataChannel state: ${state} (viewer ${msg.from})`);
      if (state === "open") {
        viewers.set(msg.from, { pc, dc });
        log(`Viewers: ${viewers.size}`);
        if (!ptyStream) {
          log(`Starting PTY stream for ${target}`);
          try {
            ptyStream = startPtyStream(target, broadcastToViewers, broadcastDims, log);
            log("PTY streaming via tmux pipe-pane");
          } catch (err) {
            log(`PTY stream failed: ${err}`);
          }
        }
      }
      if (state === "closed") stopViewer(msg.from);
    });

    await pc.setRemoteDescription(new RTCSessionDescription(msg.sdp.sdp, msg.sdp.type));
    remoteReady.add(msg.from);

    const queued = pendingIce.get(msg.from) || [];
    for (const candidate of queued) {
      try { await pc.addIceCandidate(new RTCIceCandidate(candidate)); }
      catch (err) { log(`flush ICE: ${err}`); }
    }
    pendingIce.delete(msg.from);

    const answer = await pc.createAnswer();
    await pc.setLocalDescription(answer);
    ws.send(JSON.stringify({
      type: "answer",
      target: msg.from,
      sdp: { type: answer.type, sdp: answer.sdp },
    }));
    log(`Answer sent to ${msg.from}`);
  }

  async function handleIce(msg: { from: string; candidate: any }): Promise<void> {
    const pc = peerConns.get(msg.from);
    if (!pc || !msg.candidate) return;
    if (!remoteReady.has(msg.from)) {
      const arr = pendingIce.get(msg.from) || [];
      arr.push(msg.candidate);
      pendingIce.set(msg.from, arr);
      return;
    }
    try { await pc.addIceCandidate(new RTCIceCandidate(msg.candidate)); }
    catch (err) { log(`addIce: ${err}`); }
  }

  function connectSignaling(): void {
    const authKey = process.env.P2P_SHARE_KEY || process.env.AUTH_KEY || "";
    const url = authKey ? `${signalUrl}?key=${encodeURIComponent(authKey)}` : signalUrl;
    ws = new WebSocket(url);

    ws.onopen = () => {
      log("Connected to signaling server");
      ws.send(JSON.stringify({ type: "identify", name: peerName }));
    };

    ws.onmessage = async (event) => {
      const msg = JSON.parse(String(event.data));
      switch (msg.type) {
        case "ping":
          ws.send(JSON.stringify({ type: "pong" }));
          break;
        case "welcome":
          log(`Registered as ${peerName} - waiting for viewers...`);
          break;
        case "peer-identified":
          log(`Peer joined: ${msg.name} (${msg.id})`);
          break;
        case "peer-left":
          log(`Peer left: ${msg.id}`);
          stopViewer(msg.id);
          break;
        case "offer":
          await handleOffer(msg);
          break;
        case "ice-candidate":
          await handleIce(msg);
          break;
      }
    };

    ws.onclose = () => {
      log("Signaling disconnected - reconnecting in 5s");
      setTimeout(connectSignaling, 5000);
    };

    ws.onerror = (err) => {
      log(`Signaling error: ${err}`);
    };
  }

  log(`Sharing pane: ${target}`);
  connectSignaling();
  await new Promise(() => {});
}

async function handleP2pShare(args: string[], log: Log): Promise<number> {
  const subcommand = args[0] || "status";

  if (subcommand === "status" || subcommand === "help" || subcommand === "-h" || subcommand === "--help") {
    usage(log);
    return 0;
  }

  if (subcommand !== "share") {
    log(`Unknown subcommand: ${subcommand}`);
    log("Run: maw p2p-share status");
    return 1;
  }

  const target = args[1];
  if (!target) {
    log("Usage: maw p2p-share share <pane>");
    log("  e.g. maw p2p-share share mawjs-oracle:0.0");
    return 1;
  }

  const { signalUrl, peerName, port } = parseShareOptions(args);
  const authKey = process.env.P2P_SHARE_KEY || process.env.AUTH_KEY || "";

  log("P2P Share starting...");
  log(`  Pane:   ${target}`);
  log(`  Peer:   ${peerName}`);
  log(`  Signal: ${signalUrl}`);
  log(`  Viewer: http://localhost:${port}`);
  log("");

  const viewerHtml = readFileSync(join(import.meta.dir, "..", "viewer.html"), "utf8");
  const server = Bun.serve({
    port,
    fetch(req: Request) {
      const url = new URL(req.url);
      if (url.pathname === "/" || url.pathname === "/viewer") {
        return new Response(viewerHtml, { headers: { "content-type": "text/html; charset=utf-8" } });
      }
      if (url.pathname === "/config") {
        return Response.json({ signalUrl, peerName, target, authKey });
      }
      return new Response("Not found", { status: 404 });
    },
  });

  log("Viewer ready:");
  log(`  Local: http://localhost:${port}`);
  log(`  Share: http://localhost:${port}/?peer=${encodeURIComponent(peerName)}`);
  log("  Anyone with this link + signaling access can view and type into the shared pane");
  log("");

  try {
    await startSharePeer({ target, signalUrl, peerName, log });
    return 0;
  } catch (err) {
    log(`P2P Share failed: ${err}`);
    server.stop();
    return 1;
  }
}

type InvokeContext = {
  source?: string;
  args?: string[];
  writer?: (line: string) => void;
};

type InvokeResult = {
  ok: boolean;
  output: string;
  error?: string;
  exitCode: number;
};

export default async function handler(ctx: InvokeContext): Promise<InvokeResult> {
  const out: string[] = [];
  const log = (line: string) => (ctx.writer ? ctx.writer(line) : out.push(line));
  const args = ctx.source === "cli" || !ctx.source ? (ctx.args || []) : [];
  const exitCode = await handleP2pShare(args, log);
  const ok = exitCode === 0;
  return { ok, output: ctx.writer ? "" : out.join("\n"), error: ok ? undefined : out.join("\n"), exitCode };
}

if (import.meta.main) {
  const out: string[] = [];
  const code = await handleP2pShare(process.argv.slice(2), (line) => out.push(line));
  if (out.length) console.log(out.join("\n"));
  process.exit(code);
}
