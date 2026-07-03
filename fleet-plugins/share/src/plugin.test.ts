import { afterEach, beforeEach, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { cmdShare } from "./plugin";

let tmp = "";
const savedEnv = {
  HOME: process.env.HOME,
  MAW_SHARE_SSHX_BIN: process.env.MAW_SHARE_SSHX_BIN,
  MAW_SHARE_SERVER: process.env.MAW_SHARE_SERVER,
};

beforeEach(() => {
  tmp = mkdtempSync(join(tmpdir(), "maw-share-test-"));
  process.env.HOME = join(tmp, "home");
  process.env.MAW_SHARE_SERVER = "https://ssh.clubsxai.com";
});

afterEach(() => {
  if (savedEnv.HOME === undefined) delete process.env.HOME;
  else process.env.HOME = savedEnv.HOME;
  if (savedEnv.MAW_SHARE_SSHX_BIN === undefined) delete process.env.MAW_SHARE_SSHX_BIN;
  else process.env.MAW_SHARE_SSHX_BIN = savedEnv.MAW_SHARE_SSHX_BIN;
  if (savedEnv.MAW_SHARE_SERVER === undefined) delete process.env.MAW_SHARE_SERVER;
  else process.env.MAW_SHARE_SERVER = savedEnv.MAW_SHARE_SERVER;
  rmSync(tmp, { recursive: true, force: true });
});

test("share start failure includes fake sshx stderr tail", async () => {
  const bin = join(tmp, "fake-sshx");
  writeFileSync(bin, [
    "#!/bin/sh",
    "echo 'setup detail: retry exhausted' >&2",
    "echo 'grpc Open failed: 403 Forbidden at edge' >&2",
    "exit 42",
    "",
  ].join("\n"));
  chmodSync(bin, 0o700);
  process.env.MAW_SHARE_SSHX_BIN = bin;

  await expect(cmdShare(["start", "--name", "failing"], () => {}))
    .rejects
    .toThrow(/sshx did not print a session URL.*grpc Open failed: 403 Forbidden at edge/s);
});
