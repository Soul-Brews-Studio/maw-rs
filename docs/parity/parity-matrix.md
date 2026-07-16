# maw-js → maw-rs parity matrix (issue #76)

Generated from source inspection on 2026-06-25 UTC+7. maw-js source of truth: live fleet install `/home/agent/github.com/Soul-Brews-Studio/maw-js`, version `26.6.13-alpha.1921`, commit `5560732f`. This is the finish-line checklist for full maw-js → maw-rs parity under epic #25; doc-only gaps become follow-up implementation issues.

**Wave-3 refresh (2026-07-15):** corrected `ping` and `consent` from `WASM ✅` to `native ✅` (both carry a top-level native `DispatcherEntry` — `ping.rs` / `consent.rs`, `Handler::Sync`; their `wasm-parity` artifacts are in-tree parity fixtures, not `maw-plugins` extractions). Flipped `stream` and `hub` from `native ✅` to `WASM ✅` and added a `layout` row (`WASM ✅`): all three are ship-tier plugins extracted to `Soul-Brews-Studio/maw-plugins` (packages/20-stream, 20-hub, 20-layout) with **no** native `DispatcherEntry`. Verified by running the built binary — a real invoke on a default (no-feature) build errors `plugin '<verb>' is a ship-tier WASM plugin … built without the 'wasm-host' feature` (and `stream`'s unlink golden is `#[cfg(feature="wasm-host")]` in `native_attach_view_stream_split.rs`). The parallel-native reading (a `DISPATCH_114`/`DISPATCH_300` claim) was refuted at runtime.

**Repo split phase 2 (2026-07-16):** the committed WASM test fixtures cited as
evidence below (`crates/maw-plugin-manifest/tests/fixtures/wasm-parity/*`,
`crates/maw-cli/tests/fixtures/native-*/<name>-plugin/`, `hostfn-probe`,
`epic55/follow-plugin`, `wasm-dispatch`) and the `examples/wasm-parity/`
sources were extracted to
[Soul-Brews-Studio/maw-fixtures](https://github.com/Soul-Brews-Studio/maw-fixtures)
(@aecf20b6). The fixture-welded tests referenced in the Evidence column were
removed/gutted in the same split; test rework is tracked in #546. Fixture
paths below are preserved as historical evidence pointers — resolve them
against maw-fixtures (same relative layout) or maw-rs history @5cbd148e.

## Summary

- Total rows: **133**
- native ✅: **81**
- WASM ✅: **29**
- stub ⚠️: **13**
- NOT-PORTED ❌: **10**

Counts sum to the total: 81 + 29 + 13 + 10 = 133.

> **wasm-host gate (wave-3):** `WASM ✅` verbs that are ship-tier plugins extracted to the `Soul-Brews-Studio/maw-plugins` monorepo (e.g. `layout`, packages/20-layout) run **only** on a maw binary built with `--features wasm-host`; default builds omit the Extism runtime and the verb errors loudly. In-tree `WASM ✅` rows (covered by a committed parity fixture or CLI integration test) are a separate, test-only mechanism and are not gated on that feature.

Legend: **native ✅** = Rust dispatcher/implementation exists; **WASM ✅** = a committed WASM fixture covers at least the listed source path/argv through the parity harness or CLI integration tests, **or** the verb is a ship-tier plugin extracted to `Soul-Brews-Studio/maw-plugins` that runs only on a `--features wasm-host` build (see the wasm-host gate note above); **stub ⚠️** = verb or helper exists but flags/output/subcommands are incomplete; **NOT-PORTED ❌** = no maw-rs native/WASM parity found or intentionally no-code/won't-do.

## Source evidence used

- maw-js dispatcher/routing read directly: `/home/agent/github.com/Soul-Brews-Studio/maw-js/src/cli/dispatch.ts`, `dispatch-match.ts`, `dispatch-flag-parse.ts`, `top-aliases.ts`, `route-comm.ts`, `route-tools.ts`.
- maw-js command surfaces read/enumerated from all **99** dirs under `src/vendor/mpr-plugins/*/` plus `src/commands/plugins/**` and `src/commands/shared/**`.
- maw-rs dispatcher read from exact `DispatcherEntry` registrations under `crates/maw-cli/src/core_impl/*.rs`.
- maw-rs WASM parity read from `crates/maw-plugin-manifest/tests/wasm_parity_harness.rs`, manifest tests, and CLI integration fixtures under `crates/maw-cli/tests/fixtures/`.
- 2026-07-14 NOT-PORTED census: all 80 rows were checked against exact `DispatcherEntry` command registrations and committed CLI-exercised WASM fixtures; corrected-row evidence is recorded inline.
- Accuracy cautions: a native dispatcher or WASM fixture proves a live Rust-binary route, not full flag/output parity. Full tmux, attach, view, and split surfaces remain partial where their rows say `stub ⚠️`; issue #67 option-injection coverage must still be verified before closing all exec/ssh paths.

## Messaging / transport / server

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `hey` | --from, --inbox, --approve, --trust, --no-verify-submit | maw-js source | native ✅ | Rust async transport native; source path differs from maw-js routeComm but top-level delivery exists. |
| `send` | top-level alias of hey; raw plugin command also exists | maw-js source | native ✅ | Top-level maw-rs send is native hey-style delivery. Raw send-text semantics are separate. |
| `notify` | --from, --approve, --trust; inbox-only | maw-js source + #303 tests | native ✅ | Native notify is inbox-only; peer path is covered by the live scope ACL gate and queues untrusted cross-scope sends. |
| `peek` | top-level federation-aware peek | maw-js source | WASM ✅ | Covered by WASM parity fixture for peek seeded host; raw tmux peek is native subset. |
| `messages` | serve/status/stop; --detach --direction --engine --from --json --limit --port --q --state --to | maw-js source | native ✅ | Rust async message service/client exists, but flag/output parity should be rechecked against full plugin before final green. |
| `reply / rp` | --list; reply to last/listed message | maw-js source | native ✅ | Rust async reply entry exists; mark as native but needs byte-level output audit. |
| `health` | no notable flags | maw-js source | native ✅ | Rust async health entry exists; compare text output before closing parity. |
| `ls` | --active --all --federation --fix --fleet-only --json --no-teams --node --recent --verify | maw-js source | native ✅ | Rust native ls exists; maw-js direct alias has rich flags. Treat remaining exact output parity as follow-up. |
| `ping` | [peer] | maw-js source | native ✅ | Native top-level `DispatcherEntry`: `crates/maw-cli/src/core_impl/ping.rs:1` (`Handler::Sync`). The `wasm-parity/ping/` artifact is an in-tree parity fixture, not a `maw-plugins` extraction; primary status is native. |
| `contacts` | add/remove/rm; --inbox --maw --notes --repo --thread | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-contacts/contacts-plugin/plugin.json:15` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_contacts_plugin.rs:63`. |
| `broadcast` | --fleet --session --team | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-broadcast/broadcast-plugin/plugin.json:13` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_broadcast_plugin.rs:105`. |
| `send-text` | raw pane text; no flags | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/send_text.rs:2`. |
| `send-enter` | --n/--N | maw-js source | native ✅ | Rust native subset exists for pane enter; verify exact source behavior before all-green. |
| `talk-to` | --force | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/talk_to.rs:4`. |
| `run` | peer/local run | maw-js source | stub ⚠️ | Rust native run exists but source shows small handler; full maw-js plugin behavior not proven. |
| `forward-error` | --last --to | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/forward_error.rs:2`. |
| `transport` | diagnostics | maw-js source | native ✅ | Rust native plan in maw-transport via dispatcher; not a direct maw-js vendor command but built-in transport exists. |
| `federation` | status/sync; --apply --check --dry-run --force --json --peers --port --probe --prune --user --verify | maw-js source | WASM ✅ | WASM parity covers federation status and sync --json only; native has federation-* plan commands but not full maw-js federation surface. |
| `serve` | [port]\|status\|stop; --gateway --as --force-takeover --quiet --verbose | maw-js source | native ✅ | Rust async serve exists; maw-js routeTools has more gateway/status options, exact parity not proven. |
| `serve-agents` | server API surface | maw-js source | NOT-PORTED ❌ | Headless/API plugin; no direct native parity row found. |
| `serve-debug` | server debug API surface | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-federation` | server federation API surface | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-identity` | identity server API | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/serve_identity.rs:2`. |
| `serve-triggers` | trigger server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-triggers-mutate` | trigger mutation API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-views` | views server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-worktrees` | worktree server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-ws` | websocket server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `zenoh-scout` | --advertise --all --force --json --limit --locator --no-advertise --status --timeout --transport | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/zenoh_scout.rs:3`. |
## Tmux / session / workspace

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `tmux` | ls\|peek\|split\|attach plus maw-js full tmux plugin: attach/break/close/kill/layout/ls/open/pipe/sync/etc.; many flags | maw-js source | stub ⚠️ | Issue #56 merged only interactive subset; Rust part33 supports ls/list, peek, split, attach. Full 2028 LOC maw-js tmux plugin remains partial. |
| `attach / a` | --dry-run --help --no-split --shell --split --yes plus target resolution | maw-js source | stub ⚠️ | Rust attach supports --print/--readonly/--plan-json/--dry-run/--yes/--ssh-alias/--alive; maw-js attach is 897 LOC with shell/split/no-split behavior still not matched. |
| `attach-ssh` | remote ssh attach flow | maw-js source | native ✅ | Rust native 4c subset with dry-run/plan-json tests; verify full ssh path option-injection coverage before marking closed. |
| `view` | --clean --kill --no-wake --read-only/--readonly --split --wake --zombie-agents | maw-js source | stub ⚠️ | Rust view is attach+--readonly+--print shim; maw-js view plugin is 641 LOC. Most wake/split/cleanup semantics not ported. |
| `split` | --bottom --claude-pane-policy --horizontal --no-attach --pct --right --vertical | maw-js source | stub ⚠️ | Rust split only handles target, -v/--vertical, --pct, --cmd, --dry-run. maw-js 437 LOC split flags/output remain partial. |
| `stream` | --help --into --name --unlink | maw-js source | WASM ✅ | Ship-tier plugin extracted to `Soul-Brews-Studio/maw-plugins` (packages/20-stream); no native maw-rs `DispatcherEntry`. Real invoke on a default build errors "ship-tier WASM plugin … --features wasm-host"; the unlink golden is `#[cfg(feature="wasm-host")]` in `native_attach_view_stream_split.rs`. Runs only under `--features wasm-host`. |
| `layout` | tmux layout apply/save/list | maw-js source | WASM ✅ | Ship-tier plugin extracted to `Soul-Brews-Studio/maw-plugins` (packages/20-layout); no native maw-rs `DispatcherEntry`. Runs only on a maw binary built with `--features wasm-host` (see the wasm-host gate). |
| `capture` | --full --lines --pane | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/capture.rs:2`. |
| `kill` | --all --force --index --pane --peer | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/kill.rs:2`. |
| `panes` | --all --pid | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/tmux_panes.rs:1`. |
| `tab` | --force --talk | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/tmux_tab.rs:2`. |
| `tag` | --meta --pane --title | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/tmux_tag.rs:2`. |
| `take` | no notable flags | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/tmux_handover.rs:2`. |
| `zoom` | --pane | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/tmux_zoom.rs:2`. |
| `tile` | --cmd --engine --force --layout --path --porcelain --shell --wt... | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-tile/tile-plugin/plugin.json:19` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_tile_plugin.rs:59`. |
| `pane` | swap panes | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/pane_swap.rs:1`. |
| `session` | --json --short | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/session.rs:2`. |
| `whoami` | --json --short | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/whoami.rs:2`. |
| `workon` | --layout | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/workon.rs:2`. |
| `workspace` | agents\|create\|invite\|join\|leave\|list/ls\|share\|status\|unshare; --hub/--workspace/--ws | maw-js source | WASM ✅ | WASM parity covers ls/list/default only; mutating workspace verbs remain gap. |
| `bg` | --all --dry-run --follow --json --lines --name --older-than | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/background_jobs.rs:2`. |
| `park` | ls and park note flow | maw-js source | WASM ✅ | WASM batch3 covers ls and note flow with git host calls. |
| `sleep` | --all-done | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/sleep.rs:2`. |
| `soul-sync` | agents\|pull; --from --git-common-dir --project --show-toplevel | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-soul-sync/soul-sync-plugin/plugin.json:21` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_soul_sync.rs:106`. |
| `stop` | no notable flags | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/fleet_stop.rs:3`. |
| `resume` | no notable flags | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/resume.rs:2`. |
| `reunion` | --git-common-dir | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/reunion.rs:2`. |
| `shellenv` | bash\|fish\|zsh | maw-js source | WASM ✅ | WASM batch1 parity covers shellenv bash/fish/zsh/default. |
## Discord

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `discord` | access\|bind\|channels\|check\|guilds\|inventory\|ls\|members\|pair\|route\|serve\|status\|tokens\|version; --apply --check --force --json --redact --restart --session --version | maw-js source | stub ⚠️ | Rust native discord exists and #74 merged REST subset (version, inventory/access list, members safety). Full maw-js discord command surface remains partial. |
## Consent / auth / policy

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `auth` | sign/verify/hash/hmac/from/loopback/constants parser plans | maw-js + maw-rs source | stub ⚠️ | Rust has native auth plan/test matrix; maw-js auth surface not in vendor list here, so keep partial until source-level exact command mapping is reconciled. |
| `consent` | approve\|reject\|list\|list-trust\|trust\|untrust; --help | maw-js + maw-rs source | native ✅ | Native top-level `DispatcherEntry`: `crates/maw-cli/src/core_impl/consent.rs:1` (`Handler::Sync`), plus low-level consent-* plan commands. The `wasm-parity/consent/` artifact is an in-tree parity fixture, not a `maw-plugins` extraction. |
| `pair` | generate; --at --expires | maw-js + maw-rs source | stub ⚠️ | Rust pair-code/pair-api low-level entries exist; maw-js top-level pair generate surface not directly matched. |
| `trust` | add\|remove/rm/delete\|list/ls; --yes | maw-js + maw-rs source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/trust.rs:3`. |
| `scope` | create/delete/info/list/ls/new/remove/rm/show; --lead --members --ttl --yes | maw-js #642 contract + #303 tests | native ✅ | Scope files, symmetric scope-trust.json, pending approval queue, and peer-send ACL gate are live. Cross-scope untrusted peer sends queue; corrupt ACL/trust fails open with loud warning. |
| `auto-pair-proof` | low-level proof helper | maw-js + maw-rs source | native ✅ | Rust native plan helper; not a maw-js top-level vendor command. |
| `recent-hello` | low-level pairing helper | maw-js + maw-rs source | native ✅ | Rust native plan helper; not direct maw-js top-level vendor command. |
| `pair-code / pair-code-store / pair-api / pair-api-auto` | low-level pairing helpers | maw-js + maw-rs source | native ✅ | Rust native plan helpers; do not count as top-level pair parity. |
| `policy / plugin-policy / split-policy` | policy plan helpers | maw-js + maw-rs source | native ✅ | Rust native plan helpers; no equivalent maw-js top-level plugin row found. |
## Plugin host / built-ins

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `plugin` | init\|build\|dev\|install\|create\|ls\|info\|remove\|enable\|disable; many lifecycle flags | maw-js + maw-rs source | stub ⚠️ | Rust plugin/plugin-scaffold/plugin-manifest cover manifests/scaffold and Rust-WASM build/dev. The ship-tier host does not yet provide a reviewed JS/TS-to-WASM build path, so that path fails closed; Bun/JS fleet-plugin and dev-tier surfaces remain first-class. Full maw-js lifecycle install/search/lock remains partial. |
| `plugins` | ls\|info\|remove\|lean\|standard\|full\|nuke\|enable\|disable; --json --all -v filters | maw-js + maw-rs source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/plugins.rs:2`. |
| `plugin-manifest` | parse\|load\|discover\|import-symbol\|invoke; --scan-dir --plugin --source --arg --disabled --runtime-version --plan-json | maw-js + maw-rs source | native ✅ | Rust-native manifest/registry/WASM host CLI exists; supports test fixtures and import/invoke plan output. |
| `plugin-scaffold` | scaffold plugin dirs | maw-js + maw-rs source | native ✅ | Rust-native plugin scaffold exists. |
| `completions` | bash\|fish\|zsh\|commands; --help | maw-js + maw-rs source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/completions.rs:2`. |
| `oracle-skills` | --help | maw-js + maw-rs source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/oracle_skills.rs:2`. |
| `oracle-workon` | --all --dry-run --engine --force --no-attach --prompt --split --task --tiled --with --work | maw-js + maw-rs source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/oracle_workon.rs:2`. |
| `artifact-manager` | init\|create\|write\|attach\|list/ls\|show\|get; --json --team | maw-js + maw-rs source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-artifact-manager/artifact-manager-plugin/plugin.json:19` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_artifact_manager.rs:62`. |
| `artifacts / artifact` | ls\|get [team] [task-id] --json | maw-js + maw-rs source | native ✅ | Native DispatcherEntries: `crates/maw-cli/src/core_impl/artifacts.rs:2`, `crates/maw-cli/src/core_impl/artifacts.rs:3`. |
| `agents / agent` | --json --all --node | maw-js + maw-rs source | native ✅ | Native DispatcherEntries: `crates/maw-cli/src/core_impl/agents.rs:3`, `crates/maw-cli/src/core_impl/agents.rs:7`. |
| `audit` | [limit] | maw-js + maw-rs source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/audit.rs:1`. |
| `config` | set/get-ish config; --json | maw-js + maw-rs source | WASM ✅ | WASM parity covers config set node and set port --json; secret-like set is host-gated. Full config surface remains limited. |
| `channel` | add/remove/list/setup; channel setup flags | maw-js + maw-rs source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/channel.rs:1`. |
| `discover` | --awake --json --peers --tree | maw-js + maw-rs source | native ✅ | Rust native discover plan exists; compare exact built-in output before closing. |
| `fuzzy / resolve / identity / normalize / calver / xdg / bind-host / auto-wake / route / worktree-window` | Rust helper commands | maw-js + maw-rs source | native ✅ | Native Rust support commands; mostly not direct maw-js plugin rows but needed for parity internals. |
| `hub` | fleet hub surface | maw-js + maw-rs source | WASM ✅ | Ship-tier plugin extracted to `Soul-Brews-Studio/maw-plugins` (packages/20-hub); no native maw-rs `DispatcherEntry` (`hub_xdg_plan.rs` is the unrelated hub-xdg helper). Real invoke on a default build errors "ship-tier WASM plugin … --features wasm-host". Runs only under `--features wasm-host`. |
| `ffi Tier-2` | FFI plugin host | maw-js + maw-rs source | stub ⚠️ | Won't-do/full Tier-2 deferred per issue #70; keep as stub reason rather than blank. |
## Fleet / orchestration / misc plugins

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `team` | add\|assign\|bring\|check\|create\|delete\|done\|down\|enter\|history\|invite\|list/ls\|members\|msg\|plan\|preflight\|prune\|reassign\|remove/rm\|resume\|send\|send-enter\|shutdown\|spawn\|status\|task/tasks\|up; many flags | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/team_enter.rs:2`. |
| `swarm` | --count --parent --session-id --split --tiled --worktree/--wt | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/swarm.rs:1`. |
| `mega` | ls\|status\|stop\|kill\|tree; team-lead variants | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-mega/mega-plugin/plugin.json:15` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_mega_plugin.rs:59`. |
| `avengers` | all\|best\|health\|status\|traffic; --help | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-avengers/avengers-plugin/plugin.json:12` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_avengers.rs:105`. |
| `assign` | --oracle | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/assign.rs:1`. |
| `peers` | accept\|add\|forget\|info\|list/ls\|probe\|probe-all\|remove/rm\|tofu-bootstrap; --alias --all --allow-unreachable --discovered --json --limit --node --ssh --timeout --user | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/peers.rs:2`. |
| `peer-sources / peer-probe` | source/probe helpers | maw-js source | native ✅ | Rust-native helpers; not full top-level peers parity. |
| `activity` | --all --json --sampler --samples --stuck-only --watch --window | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/activity.rs:2`. |
| `follow` | --grep --json --quit-on-idle --since | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/epic55/follow-plugin/plugin.json:7` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_activity_follow_dispatch.rs:61`. |
| `pulse` | active\|add\|clean\|cleanup\|list/ls\|orphan\|stale; --dry-run --oracle --priority --sync --worktree/--wt | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-pulse/pulse-plugin/plugin.json:17` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_pulse_plugin.rs:89`. |
| `inbox` | approve\|drain\|pending\|read\|reject\|show\|status\|write; --all --dry-run --from --json --last --max --older-than-hours --safe --unread | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/inbox.rs:2`. |
| `cross-team-queue` | headless queue surface | maw-js source | WASM ✅ | WASM batch1 parity fixture covers no-arg output; source dispatcher treats non-CLI surfaces specially. |
| `fleet` | doctor/init/health/consolidate/resume/sync/wake; --json --dry-run --fix --reboot etc. | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/fleet.rs:2`. |
| `oracle` | about\|fleet\|list\|nickname\|prune\|register\|scan\|search\|stale; --json etc. | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/oracle.rs:2`. |
| `bud` | agents/fleet/gh; --blank --dry-run --fast --force --from --issue --repo --root --scaffold-only --split --tiny etc. | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/buddy_workspace.rs:2`. |
| `awaken` | --blank --dry-run --fast --force --from --issue --repo --root --seed --split --sync-peers --track-vault --trigger --yes | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/awaken.rs:2`. |
| `incubate` | --blank --contribute --dry-run --fast --flash --force --from --issue --repo --root --split --trigger | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-incubate/incubate-plugin/plugin.json:13` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_incubate_plugin.rs:142`. |
| `wake` | --all --all-local --attach --dry-run --fresh --from-snapshot --incubate --issue --kill --layout --list --main --new --no-attach --parent --peer --pick --pr --repo --resume --snapshot --solo --split --task --wt | maw-js source | stub ⚠️ | Rust async wake exists but falls back for some paths; full maw-js wake/worktree behavior and flags not fully native. |
| `bring / b` | alias for wake --split; --to/--pick/engine inherited | maw-js source | native ✅ | Rust native bring plan exists; still compare output against maw-js alias. |
| `work / awake / scaffold / new / promote / preflight / snapshots` | top aliases | maw-js source | native ✅ | Native DispatcherEntries: `crates/maw-cli/src/core_impl/workspace_scaffold_commands.rs:2`, `crates/maw-cli/src/core_impl/workspace_scaffold_commands.rs:3`, `crates/maw-cli/src/core_impl/workspace_scaffold_commands.rs:4`, `crates/maw-cli/src/core_impl/workspace_scaffold_commands.rs:5`, `crates/maw-cli/src/core_impl/workspace_scaffold_commands.rs:6`, `crates/maw-cli/src/core_impl/workspace_scaffold_commands.rs:7`, `crates/maw-cli/src/core_impl/workspace_scaffold_commands.rs:8`. |
| `project` | find\|incubate\|learn\|list\|search; --contribute --flash --offload | maw-js source | WASM ✅ | WASM batch1 covers project subcommands for no-host output. |
| `learn` | --deep --fast --mode | maw-js source | WASM ✅ | WASM batch1 covers learned args including unknown --turbo behavior. |
| `dream` | --all --between --date --format --gain --json --limit --oneline --pain --plan --porcelain --project --repo --since --speculate --state | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-dream/dream-plugin/plugin.json:10` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_dream_plugin.rs:139`. |
| `costs` | --daily --days --json | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-costs/costs-plugin/plugin.json:11` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_costs.rs:101`. |
| `signals` | --days --json --root | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/signals.rs:2`. |
| `done` | --all --clean-branch --dry-run --force | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/worktree_finish.rs:2`. |
| `pr` | --body --show-current --title | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/github_pull_request.rs:2`. |
| `archive` | --dry-run --yes | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/archive.rs:2`. |
| `absorb` | --dry-run --into | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/absorb.rs:1`. |
| `cleanup` | --ask --dry-run --json --prune-stale --repo --scope --worktrees --yes --zombie-agents/--zombies | maw-js source | WASM ✅ | WASM batch3 covers only --worktrees [--yes] --json; rest remains partial. |
| `forget` | --all --dry-run --force --json --yes | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/forget.rs:1`. |
| `restart` | --no-update --ref --version | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/restart.rs:3`. |
| `setup` | auto-wake; --dry-run --only --repo --user | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/setup.rs:2`. |
| `user-setup` | --dry-run --json --porcelain | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/user_setup.rs:1`. |
| `doctor` | --allow-drift --backend --capture --dry-run --errors --fix-sessions --fix-stale --fix-xdg --forward --gateway --json --manifest-path --migrate --no-prompt --plan --port --release --smoke --version | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/doctor.rs:1`. |
| `check` | tools/version; --version | maw-js source | WASM ✅ | WASM batch3 covers check tools with exec host transcript. |
| `demo` | --daily --fast | maw-js source | WASM ✅ | WASM CLI fixture: `crates/maw-cli/tests/fixtures/native-demo/demo-plugin/plugin.json:13` (sibling `plugin.wasm`), exercised through the binary in `crates/maw-cli/tests/native_demo_plugin.rs:90`. |
| `about` | no notable flags | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/about.rs:4`. |
| `overview` | --color --kill | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/overview.rs:2`. |
| `profile` | active/current/info/list/ls/set/show/use | maw-js source | WASM ✅ | WASM parity covers current/list/show/use. |
| `triggers` | no notable flags | maw-js source | WASM ✅ | WASM parity covers no-arg output. |
| `on` | --once --timeout | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/trigger_registration.rs:2`. |
| `token / tokens` | current\|list/ls/tokens\|load\|save\|scan\|use; --force --no-team | maw-js source | stub ⚠️ | Issue #55 native primitive: list/current implemented; use/save/load/scan return deferred stub. |
| `find` | --oracle | maw-js source | stub ⚠️ | Issue #55 ported native, but verify full maw-js output/source search parity before green. |
| `locate` | --json --path | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/locate.rs:2`. |
| `rename` | no notable flags | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/rename.rs:2`. |
| `ui` | --3d --dev --install --source --tunnel --version | maw-js source | native ✅ | Native DispatcherEntry: `crates/maw-cli/src/core_impl/maw_ui.rs:2`. |
| `mqtt` | closed/won't-do | maw-js source | NOT-PORTED ❌ | Intentionally no-code/won't-do per issue #12; leave as reasoned not-ported. |
| `batch2 closed set` | closed/won't-do | maw-js source | NOT-PORTED ❌ | Issue #13 batch2 closed as no-code/won't-do where applicable; keep future rows explicit if source resurfaces names. |

## Follow-up issue seeds

- Split #55 remainder into separate issues: `peers`, `activity`, `follow`.
- Split #56 remainder into separate issues: full `tmux`, full `attach`, full `view`, full `split`, with flag/output golden tests. Keep existing Rust subset tests as regression coverage.
- Audit #67 option-injection coverage across every Rust exec/ssh boundary before marking attach-ssh/stream/tmux paths final-green.
- Promote WASM rows from “covered argv subset” to true parity only after every source subcommand/flag in this table has a golden output test or an explicit won't-do note.
