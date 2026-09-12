# Contract: Generic Engine Provider

## Boundary

Native wake/attach own target resolution, tmux/session mutation, cold-start proof, worktree routing,
launch confirmation, and attach/switch behavior. A provider owns vendor-specific command planning,
resume/profile/account policy, and compatibility classification.

The first provider is Codex, but the interface cannot contain Codex-named fields.

An accepted manifest contains a provider descriptor separate from CLI routes:

```json
{
  "provider": {
    "id": "provider.codex",
    "export": "provider_plan",
    "executableId": "codex",
    "operations": ["launch", "resume", "profile", "maintenance"],
    "safeEnv": [{"name":"CODEX_HOME","valueSchema":"providerRootId"}]
  }
}
```

The descriptor is bounded, versioned, hash-covered, and validated during discovery. Executable and
environment entries are identifiers/basenames from host policy, never arbitrary paths or secrets.

## Plan request

```json
{
  "schemaVersion": 1,
  "operation": "launch",
  "providerId": "provider.codex",
  "engine": "provider-selected-public-id",
  "executableId": "codex",
  "cwd": "/validated/worktree",
  "compositionMode": "provider-default",
  "resume": null,
  "channels": [],
  "providerRoots": [
    {"id":"provider-root-1","slot":1,"state":"busy"},
    {"id":"provider-root-2","slot":2,"state":"free"}
  ]
}
```

The native host supplies already-validated, non-secret inputs. Raw environment and config files are
not included. Before worktree or audit mutation, native code resolves the target branch/commit and
builds a read-only configuration snapshot for the prospective final cwd, including the frozen
directory-layer precedence that would apply after checkout. The request is bound to that
branch/commit/config digest; native code revalidates it before worktree creation and refuses a race
or stale snapshot rather than planning twice. Provider-root/account candidates are opaque
invocation-issued ids plus bounded facts gathered before the plan-only instance starts; they contain
no resolved path or secret. The provider cannot call occupancy from plan mode and may reference only
an id present in this request.

Native generic composition remains authoritative around the provider plan. From the same target
config snapshot it first freezes the current complete-command precedence, including explicit
`--engine-cmd`, directory-layer `commands.<engine>`, and the higher-priority complete
`commands.<engine>-resume` override. `compositionMode` tells the provider only whether its default/
family transform is needed; operator-authored complete commands are never sent to or rewritten by
the provider. After selecting the complete override or validating provider argv, native code appends
the exact original `--prompt` or resolved `wake.prompt` bytes once, in the frozen position. The
provider neither receives nor can drop, duplicate, or alter that prompt.
For an operator-complete composition, the provider is still presence/refusal-checked and called once
for the bound provider decision, but response args/env must be empty and cannot affect the native
complete command.

## Plan response

```json
{
  "schemaVersion": 1,
  "executableId": "codex",
  "args": [],
  "env": [{"name":"CODEX_HOME","valueRef":"provider-root-2"}],
  "displayEngine": "codex",
  "resumeMode": "none"
}
```

Rules:

- `providerId` identifies the accepted descriptor; it is never used as executable authority.
  `executableId` MUST equal that descriptor's separate `executableId` supplied by native code. Native code resolves
  it to the configured executable; the guest cannot return a program/path/shell fragment.
- Args are an argv vector validated against the operation-specific schema: allowed flags, cardinality,
  byte limits, control rejection, and mutually exclusive options are native-enforced.
- Every environment entry is allowlisted with a value schema in the accepted provider descriptor.
  Scalar enum/boolean values are bounded and control-free; path-valued settings use only opaque
  host-issued provider-root/account ids that native code resolves after validation. Literal absolute,
  relative, traversal, or unissued paths are rejected. Authentication token values are never accepted
  from or returned by a guest.
- Native wake first computes the final prospective worktree/cwd and branch-config snapshot without
  creating it, invokes and validates the provider exactly once for that final cwd, caches the
  accepted plan, revalidates the snapshot, and only then writes wake phase/state or performs
  filesystem/worktree/tmux mutation. It never replans after worktree creation.
- Unknown schema/version/field, malformed plan, missing provider, or refused artifact fails before
  mutation. Explicit provider selection never falls back silently.
- The final command preserves the frozen generic complete-command/resume-override precedence and
  exact prompt append described above. Provider args are vendor policy only; they never replace an
  operator-authored complete command or carry instruction text.

## Compatibility classification

A provider may classify bounded command/title metadata already gathered by native tmux/process
adapters. It does not receive process environments or arbitrary process access. Failure to classify
returns `unknown`; native generic safety policy decides how to fail closed.

## Provider discovery

Provider id resolves to one accepted external artifact and exact manifest capability. A small
host-owned known-provider catalog maps explicit engine/provider ids to package/source/install repair
metadata even when the artifact is absent; it contains no launch argv, profile, or vendor command
policy. Discovery uses the same hash/SDK/refusal rules as CLI plugins. Native wake/attach do not
depend on any provider when another engine is selected.

Provider planning is host-to-guest. Native code opens the accepted exact artifact in a fresh instance,
invokes only the descriptor's provider export with `InvokeSource::Provider`, and validates the reply
before returning it to wake or a typed maintenance operation. Provider mode grants no CLI-route host
capabilities: ABI-compatible imports exist only as refusal stubs, including filesystem, pane,
lifecycle, layout, batch, worktree, maintenance, occupancy, and consent. A typed stack may perform CLI
`wave` -> native lifecycle -> fresh provider plan, but any provider-to-host-operation/provider or
same-surface recursion is refused.

## Maintenance and health

`maintenance` plans are bound to the descriptor executable and an allowlisted operation; the native
adapter executes them without exposing raw process authority to the guest. Generic health returns
only typed `{available,authState,trustState,isolationState,detailCode}` facts. The host may inspect
config/auth/process state internally, but token/config bytes, raw environment, and secret paths never
cross the boundary. Probe failure is `unknown`, not healthy/free.

## Compatibility tests

- Existing Codex launch and resume fixture rows produce the same final argv through provider plan.
- CLI prompt, directory `wake.prompt`, `commands.<engine>`, `commands.<engine>-resume`, and charter
  engine-command/resume precedence rows preserve exact argv and append prompt exactly once.
- Provider-present Codex rows preserve frozen behavior. Explicit missing/refused Codex selection is
  the only provider-cutover error change and fails before mutation; all non-Codex and unrelated-
  plugin wake/attach rows remain byte-identical.
- Non-Codex native wake/attach rows are byte-identical with no Codex artifact installed.
- Missing/refused Codex provider fails before tmux/session mutation with a repair hint.
- Existing dispatcher behavior still appends exactly one bounded CLI request/failure audit record;
  that observability record is not a successful wake phase. Every provider selection/plan error
  leaves wake phase/state, worktree, workflow filesystem, tmux, and lifecycle state unchanged beyond that exact dispatcher audit append, and a one-call
  counter proves no post-mutation replan exists.
- A not-yet-checked-out existing branch whose directory-layer config differs from the base checkout
  is planned from the target commit snapshot; a changed branch/config digest is refused before
  mutation.
- Provider output cannot inject shell control, arbitrary environment names, or secret values.
- No provider operation can invoke wake/attach recursively.
- Provider maintenance cannot select a program or operation absent from its accepted descriptor.
- A provider export attempting lifecycle/provider re-entry is refused without mutation.
