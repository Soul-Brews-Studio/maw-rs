# `@maw-rs/wasm-sdk`

AssemblyScript bindings for writing ship-tier WASM plugins for
[maw-rs](https://github.com/Soul-Brews-Studio/maw-rs). The package exports typed
wrappers for the capability-gated `maw.*` host ABI and the shared plugin helpers.

## Install

```bash
npm install --save-dev @maw-rs/wasm-sdk@^1.0.0
```

Import the bindings from AssemblyScript:

```ts
import { fsList, timeNow } from "@maw-rs/wasm-sdk";
```

Point `maw plugin build` at the project containing the installed SDK toolchain when
no maw-rs checkout is available:

```bash
MAW_WASM_SDK_DIR="$PWD" maw plugin build .
```

The package includes its pinned AssemblyScript compiler and Extism AssemblyScript
PDK dependency, so plugin authors do not need a maw-rs checkout.

## Version and host ABI contract

The npm version tracks the **maw host ABI contract version**, not the API maturity
of this JavaScript/AssemblyScript wrapper. Plugin manifests should declare the
matching compatibility range, for example `"sdk": "^1.0.0"`.

A host ABI breaking change bumps the npm major. Additive host calls bump the minor.
A breaking wrapper API change also bumps the npm major even when the host ABI itself
has not changed; the compatibility table will continue to name the supported host
ABI explicitly.

| npm SDK | Host ABI | Notable host calls |
| --- | --- | --- |
| `1.0.0` | `1.0.0` | `maw.time.now`, `maw.tmux.command`, paginated `maw.fs.list` (`offset` / `nextOffset`) |

See [CHANGELOG.md](./CHANGELOG.md) for the host ABI additions shipped by each SDK
release.

## Package validation

```bash
npm ci
npm run build
npm pack --dry-run
```

`npm run build` compiles the public entry point as an AssemblyScript smoke test.
Publishing is handled from the maw-rs release workflow when `NPM_TOKEN` is configured.
