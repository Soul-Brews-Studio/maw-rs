// Host-fn probe: imports one maw host function from the pinned @maw-rs/wasm-sdk
// (no per-plugin node_modules) and calls it. Compiling this module produces an
// `extism:host/user` "maw.exec.run" import — exactly what the MVP WASM runtime
// used to reject. Representative of squad, which derives its team by shelling out.
import { hostExec } from "@maw-rs/wasm-sdk";
import { Host } from "@extism/as-pdk";

export function myAbort(message: string | null, fileName: string | null, lineNumber: u32, columnNumber: u32): void {}

export function handle(): i32 {
  const response = hostExec("{\"command\":\"true\",\"args\":[]}");
  Host.outputString("{\"ok\":true,\"probe\":\"hostfn\",\"response\":" + response + "}");
  return 0;
}
