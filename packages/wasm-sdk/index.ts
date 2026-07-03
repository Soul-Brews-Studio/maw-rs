// Root entry so `asc --path` package resolution finds @maw-rs/wasm-sdk: asc
// resolves a package to <pkg>/index.ts and does not honor package.json
// "exports"/"ascMain", so this shim re-exports the real assembly source.
export * from "./assembly/index";
