import { Host } from "@extism/as-pdk";

export function myAbort(message: string | null, fileName: string | null, lineNumber: u32, columnNumber: u32): void {}

export function handle(): i32 {
  Host.outputString("{\"ok\":true,\"output\":\"{\\\"items\\\":[],\\\"stats\\\":{\\\"totalItems\\\":0,\\\"byRecipient\\\":{},\\\"byType\\\":{},\\\"oldestAgeHours\\\":null,\\\"newestAgeHours\\\":null},\\\"errors\\\":[],\\\"schemaVersion\\\":1}\"}");
  return 0;
}
