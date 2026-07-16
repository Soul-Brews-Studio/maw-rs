# maw-js plugin invoke reference

This is a source-derived reference for porting the maw-js plugin invoke path to
maw-rs. It is based on these maw-js files:

- `/opt/Code/github.com/Soul-Brews-Studio/maw-js/src/plugin/types.ts`
- `/opt/Code/github.com/Soul-Brews-Studio/maw-js/src/plugin/registry.ts`
- `/opt/Code/github.com/Soul-Brews-Studio/maw-js/src/plugin/registry-invoke.ts`
- `/opt/Code/github.com/Soul-Brews-Studio/maw-js/src/cli/dispatch.ts`
- API dispatch helpers in `/opt/Code/github.com/Soul-Brews-Studio/maw-js/src/api/plugins.ts`
  and `/opt/Code/github.com/Soul-Brews-Studio/maw-js/src/api/index.ts`

## Shared invoke contract

`InvokeContext` is the object passed to plugin handlers. The shared type in
`src/plugin/types.ts` currently contains:

```ts
export interface InvokeContext {
  source: "cli" | "api" | "peer";
  args: string[] | Record<string, unknown>;
  matchedName?: string;
  writer?: (...args: unknown[]) => void;
  flags?: Record<string, boolean | string | number | string[]>;
}
```

Field behavior:

- `source`: `"cli"`, `"api"`, or `"peer"`. `registry-invoke.ts` only applies
  universal CLI help/version handling and stdout writer injection when
  `source === "cli"`.
- `args`: CLI invocations pass the remaining argv array after the matched
  command words are removed. API and peer invocations pass an object.
- `matchedName`: optional CLI command surface that matched, including aliases.
  In the registry dispatch path this is lower-cased because matching is done
  against `args.join(" ").toLowerCase()`. Plugins can use it for alias-aware
  behavior or deprecation messages.
- `flags`: optional parsed CLI flags, populated by `src/cli/dispatch.ts` only
  when manifest-declared CLI flags parse to at least one value. Keys have no
  leading dashes, for example `--team-id` becomes `team-id`.
- `writer`: optional streaming writer. `invokePlugin()` injects a stdout writer
  for CLI TypeScript plugins if the caller did not already provide one. API and
  peer calls leave it undefined.
- `config`: not part of the shared `InvokeContext` type in current maw-js.
  Config affects discovery indirectly through `discoverPackages()` loading
  `disabledPlugins`, but ordinary invoke contexts do not carry a `config`
  field. A vendor plugin has a separate hook-local context shape with
  `config?: MawConfig`; that is not the plugin invoke contract.

`InvokeResult` is:

```ts
export interface InvokeResult {
  ok: boolean;
  output?: string;
  error?: string;
  exitCode?: number;
}
```

Result behavior:

- `ok: true` means the invocation succeeded. `output` may be absent.
- `ok: false` means failure. CLI prints `error` when present and exits with
  `exitCode ?? 1`.
- `exitCode` is meaningful for failed CLI results; API routes do not currently
  preserve it in HTTP responses.

## Registry discovery

`src/plugin/registry.ts` is the loader and public facade. It re-exports
`invokePlugin` from `registry-invoke.ts` and exposes `discoverPackages()`.

Discovery scans configured plugin directories, loads `plugin.json` manifests,
applies SDK semver and artifact hash gates, marks plugins listed in config
`disabledPlugins`, sorts by manifest `weight` ascending, and applies the active
profile filter. The returned `LoadedPlugin` contains:

```ts
{
  manifest: PluginManifest;
  dir: string;
  wasmPath: string;
  entryPath?: string;
  kind: "wasm" | "ts";
  disabled?: boolean;
}
```

The discovery result is memoized per process unless dependency injection opts
out of the cache.

## CLI dispatch chain

Top-level CLI dispatch in `src/cli/dispatch.ts` follows this ladder:

1. `routeComm(cmd, args)` and `routeTools(cmd, args)`.
2. Top-level aliases from `./top-aliases`.
3. `matchCommand(args)` / `executeCommand(...)` for the command registry.
4. Package plugin registry via `dispatchPluginRegistry(cmd, args)`.
5. Agent-name shorthand fallback.

The package plugin registry path is the command -> plugin -> handler path that
maw-rs needs to mirror:

1. Dynamically import `discoverPackages`, `invokePlugin`, and dispatch-match
   helpers.
2. Load plugins with `discoverPackages()`.
3. Lower-case the full command line as `cmdName = args.join(" ").toLowerCase()`.
4. Call `resolvePluginMatch(plugins, cmdName)`.
5. If ambiguous, print candidates and raise `UserError`.
6. If matched, check plugin dependencies. Missing or disabled dependencies are
   reported before invocation.
7. Count words in `matchedName` and compute `remaining = args.slice(matchedWords)`.
   This preserves original argument case for plugin handlers.
8. Validate `remaining` against `manifest.cli.flags` when present.
9. Parse declared flags with `parsePluginFlags()`.
10. Invoke:

```ts
const result = await invokePlugin(dispatch.plugin, {
  source: "cli",
  args: remaining,
  matchedName: dispatch.matchedName,
  ...(Object.keys(parsedFlags).length > 0 ? { flags: parsedFlags } : {}),
});
```

11. Print `result.output` when present. If `!result.ok`, print `result.error`
    and `process.exit(result.exitCode ?? 1)`. Otherwise `process.exit(0)`.

Command matching rules from `src/cli/dispatch-match.ts`:

- Explicit `manifest.cli.command` and `manifest.cli.aliases` define CLI names.
- If `manifest.cli` is absent, a plugin can still dispatch as `manifest.name`
  only when it has an executable TS or WASM surface and no non-CLI surfaces.
- Disabled plugins are skipped unless the caller requests disabled matching for
  diagnostics.
- Resolution priority is exact command, exact alias, prefix command, then prefix
  alias. Prefix matching requires a word boundary (`name + " "`), not raw
  `startsWith(name)`.
- Multiple winners in the same priority bucket are ambiguous.

Universal plugin flags are handled inside `invokePlugin()` before the handler:

- `-v`, `--version`, and `-version` return plugin metadata and surfaces.
- `-h`, `--help`, and `-help` anywhere in CLI args return usage, aliases, flags,
  surfaces, and plugin directory.

## API dispatch

The dedicated plugins router in `src/api/plugins.ts` is mounted under `/api` and
defines:

- `GET /api/plugins`: list plugins that expose `manifest.api`.
- `GET /api/plugins/:name`: find plugin by exact `manifest.name`, require
  `manifest.api.methods` to include `"GET"`, then invoke with query params.
- `POST /api/plugins/:name`: find plugin by exact `manifest.name`, require
  `manifest.api.methods` to include `"POST"`, then invoke with request body or
  `{}`.

GET invocation shape:

```ts
const result: InvokeResult = await deps.invokePlugin(plugin, {
  source: "api",
  args: query as Record<string, unknown>,
} satisfies InvokeContext);
```

POST invocation shape:

```ts
const result: InvokeResult = await deps.invokePlugin(plugin, {
  source: "api",
  args: (body ?? {}) as Record<string, unknown>,
} satisfies InvokeContext);
```

API error mapping in this router:

- Missing plugin -> HTTP 404 and `{ ok: false, error: "plugin '<name>' not found" }`.
- Method not declared -> HTTP 405 and `{ ok: false, error: "method not allowed" }`.
- Invoke failure -> HTTP 500 and `{ ok: false, error: result.error ?? "invoke failed" }`.
- Success -> `{ ok: true, output: result.output }`.

`src/api/index.ts` also auto-mounts every discovered plugin `manifest.api.path`.
It strips a leading `/api` from the manifest path before registering it on the
`/api` router, skips collisions with direct routes, and invokes with
`{ source: "api", args: query ?? {} }` for GET or
`{ source: "api", args: body ?? {} }` for POST. This auto-mounted path returns
the raw `InvokeResult`.

## Peer dispatch

Peer plugin dispatch is used by `maw hey plugin:<name> ...` in
`src/commands/shared/comm-send.ts`. It looks up a plugin by exact
`manifest.name` and invokes:

```ts
const result = await invokePlugin(plugin, {
  source: "peer",
  args: { message, from: pluginFrom },
});
```

`pluginFrom` is either the configured local node name or the sender identity.

## TypeScript plugin invocation

For `LoadedPlugin` values where `kind === "ts"` and `entryPath` is set,
`invokePlugin()` dynamically imports the plugin entry and calls its default
export or named `handler` export.

The exact import call is:

```ts
const mod = await import(pathToFileURL(realpathSync(plugin.entryPath)).href);
```

Handler resolution and call:

```ts
const handler = mod.default || mod.handler;
if (!handler) return { ok: false, error: "TS plugin has no default export or handler" };
const result = await handler(ctxWithWriter);
```

If the handler returns an object containing an `ok` key, maw-js returns that
object as the `InvokeResult`. Any other handler return value is treated as
success with `{ ok: true }`. Thrown values are converted to
`{ ok: false, error: stackOrMessage }`.

Important runtime behavior:

- There is no sandbox for TS plugins.
- `process.exit` is not monkey-patched; a TS plugin calling it can terminate the
  host process.
- CLI TS invocations receive an injected `writer` that joins arguments with a
  space and writes a trailing newline to `process.stdout`.

## WASM invoke protocol

For WASM plugins, `invokePlugin()`:

1. Reads `plugin.wasmPath`.
2. Compiles a `WebAssembly.Module`.
3. Requires module exports named `handle` and `memory`; otherwise returns
   `{ ok: false, error: "wasm missing required handle+memory exports" }`.
4. Builds an `env` import object with host functions from `wasm-bridge.ts`.
5. Instantiates the module with that import object.
6. Captures `instance.exports.memory` and resolves allocation as exported
   `maw_alloc` if present, otherwise the bridge's fallback allocator for host
   callbacks.
7. Calls `preCacheBridge(bridge)` before invoking `handle`, best effort.
8. JSON-encodes the `InvokeContext`, UTF-8 encodes it, allocates space, writes
   the bytes into WASM linear memory, and calls `handle(ptr, len)`.
9. Interprets the returned pointer as output.
10. Races the invocation against a hard 5 second timeout.

The required entrypoint shape is:

```ts
const handle = instance.exports.handle as (ptr: number, len: number) => number;
const resultPtr = handle(argPtr, bytes.length);
```

The context write path is:

```ts
const json = JSON.stringify(ctx);
const bytes = textEncoder.encode(json);
const argPtr = (instance.exports.maw_alloc as Function)?.(bytes.length) ?? 0;
new Uint8Array(wasmMemory.buffer).set(bytes, argPtr);
```

Return protocol:

- If `handle()` returns `0` or a negative pointer, maw-js returns `{ ok: true }`.
- If `handle()` returns a positive pointer, maw-js first reads a little-endian
  `u32` length at that pointer.
- If `0 < len < 1_000_000`, maw-js reads `len` UTF-8 bytes from `resultPtr + 4`
  and returns `{ ok: true, output }` when non-empty.
- Otherwise maw-js falls back to reading a null-terminated UTF-8 string starting
  at `resultPtr`.

WASM bridge memory conventions:

- Strings passed to host functions are UTF-8 `(ptr, len)` pairs.
- Host-returned strings use `u32_le length + UTF-8 payload`.
- WASM modules should export `memory` and `maw_alloc(size) -> ptr`.
- The bridge provides host functions such as `maw_print`, `maw_print_err`,
  `maw_log`, `maw_identity`, `maw_federation`, `maw_send`, `maw_fetch`,
  `maw_async_result`, and fallback `maw_alloc`.
- The memory growth guard defaults to 256 WebAssembly pages, i.e. 16 MiB.

Porting notes:

- The host currently checks only `handle` and `memory` as required exports, but
  practical context writing expects `maw_alloc` or else writes the input JSON at
  pointer `0`.
- WASM output is always wrapped into an `InvokeResult` with `ok: true`; there is
  no structured WASM failure result in this protocol unless the host itself
  fails to read, compile, instantiate, invoke, or times out.
- The JSON context sent to WASM will omit `writer` because functions are not
  JSON-serializable.
