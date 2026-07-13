# maw-js → maw-rs parity matrix (issue #76)

Generated from source inspection on 2026-06-25 UTC+7. maw-js source of truth: live fleet install `/home/agent/github.com/Soul-Brews-Studio/maw-js`, version `26.6.13-alpha.1921`, commit `5560732f`. This is the finish-line checklist for full maw-js → maw-rs parity under epic #25; doc-only gaps become follow-up implementation issues.

## Summary

- Total rows: **131**
- native ✅: **21**
- WASM ✅: **15**
- stub ⚠️: **14**
- NOT-PORTED ❌: **81**

Legend: **native ✅** = Rust dispatcher/implementation exists; **WASM ✅** = wasm parity harness covers at least the listed source path/argv; **stub ⚠️** = verb or helper exists but flags/output/subcommands are incomplete; **NOT-PORTED ❌** = no maw-rs native/WASM parity found or intentionally no-code/won't-do.

## Source evidence used

- maw-js dispatcher/routing read directly: `/home/agent/github.com/Soul-Brews-Studio/maw-js/src/cli/dispatch.ts`, `dispatch-match.ts`, `dispatch-flag-parse.ts`, `top-aliases.ts`, `route-comm.ts`, `route-tools.ts`.
- maw-js command surfaces read/enumerated from all **99** dirs under `src/vendor/mpr-plugins/*/` plus `src/commands/plugins/**` and `src/commands/shared/**`.
- maw-rs dispatcher read from `crates/maw-cli/src/core_impl/part01.rs` (`DISPATCHER_ENTRIES`) and implementation parts under `crates/maw-cli/src/core_impl/part*.rs`.
- maw-rs WASM parity read from `crates/maw-plugin-manifest/tests/wasm_parity_harness.rs` and manifest tests.
- Accuracy cautions: issue #55 ported only `scope/find/token`; `peers` (~1798 LOC), `activity` (~624 LOC), and `follow` (~343 LOC) remain unported. Issue #56 merged only init + tmux-interactive-subset + attach-ssh + stream; full tmux (~2028 LOC), attach (~897 LOC), view (~641 LOC), and split (~437 LOC) remain partial. Issue #67 option-injection sweep must be verified from source before closing all exec/ssh paths.

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
| `ping` | [peer] | maw-js source | WASM ✅ | WASM batch3 parity for ping [] and [alpha]; maw-rs has no native dispatcher entry. |
| `contacts` | add/remove/rm; --inbox --maw --notes --repo --thread | maw-js source | NOT-PORTED ❌ | No native entry; no WASM parity fixture found. |
| `broadcast` | --fleet --session --team | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `send-text` | raw pane text; no flags | maw-js source | NOT-PORTED ❌ | No native entry; not covered by send native, which sends envelope+Enter. |
| `send-enter` | --n/--N | maw-js source | native ✅ | Rust native subset exists for pane enter; verify exact source behavior before all-green. |
| `talk-to` | --force | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `run` | peer/local run | maw-js source | stub ⚠️ | Rust native run exists but source shows small handler; full maw-js plugin behavior not proven. |
| `forward-error` | --last --to | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `transport` | diagnostics | maw-js source | native ✅ | Rust native plan in maw-transport via dispatcher; not a direct maw-js vendor command but built-in transport exists. |
| `federation` | status/sync; --apply --check --dry-run --force --json --peers --port --probe --prune --user --verify | maw-js source | WASM ✅ | WASM parity covers federation status and sync --json only; native has federation-* plan commands but not full maw-js federation surface. |
| `serve` | [port]\|status\|stop; --gateway --as --force-takeover --quiet --verbose | maw-js source | native ✅ | Rust async serve exists; maw-js routeTools has more gateway/status options, exact parity not proven. |
| `serve-agents` | server API surface | maw-js source | NOT-PORTED ❌ | Headless/API plugin; no direct native parity row found. |
| `serve-debug` | server debug API surface | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-federation` | server federation API surface | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-identity` | identity server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-triggers` | trigger server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-triggers-mutate` | trigger mutation API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-views` | views server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-worktrees` | worktree server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `serve-ws` | websocket server API | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `zenoh-scout` | --advertise --all --force --json --limit --locator --no-advertise --status --timeout --transport | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
## Tmux / session / workspace

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `tmux` | ls\|peek\|split\|attach plus maw-js full tmux plugin: attach/break/close/kill/layout/ls/open/pipe/sync/etc.; many flags | maw-js source | stub ⚠️ | Issue #56 merged only interactive subset; Rust part33 supports ls/list, peek, split, attach. Full 2028 LOC maw-js tmux plugin remains partial. |
| `attach / a` | --dry-run --help --no-split --shell --split --yes plus target resolution | maw-js source | stub ⚠️ | Rust attach supports --print/--readonly/--plan-json/--dry-run/--yes/--ssh-alias/--alive; maw-js attach is 897 LOC with shell/split/no-split behavior still not matched. |
| `attach-ssh` | remote ssh attach flow | maw-js source | native ✅ | Rust native 4c subset with dry-run/plan-json tests; verify full ssh path option-injection coverage before marking closed. |
| `view` | --clean --kill --no-wake --read-only/--readonly --split --wake --zombie-agents | maw-js source | stub ⚠️ | Rust view is attach+--readonly+--print shim; maw-js view plugin is 641 LOC. Most wake/split/cleanup semantics not ported. |
| `split` | --bottom --claude-pane-policy --horizontal --no-attach --pct --right --vertical | maw-js source | stub ⚠️ | Rust split only handles target, -v/--vertical, --pct, --cmd, --dry-run. maw-js 437 LOC split flags/output remain partial. |
| `stream` | --help --into --name --unlink | maw-js source | native ✅ | Rust native stream covers link/unlink plans; recheck byte-level output before final all-green. |
| `capture` | --full --lines --pane | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `kill` | --all --force --index --pane --peer | maw-js source | NOT-PORTED ❌ | maw-js top alias routes kill to tmux kill; Rust tmux subset lacks kill subcommand. |
| `panes` | --all --pid | maw-js source | NOT-PORTED ❌ | maw-js alias routes to tmux ls --all --verbose; Rust tmux ls exists but no panes top alias/flag parity. |
| `tab` | --force --talk | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `tag` | --meta --pane --title | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `take` | no notable flags | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `zoom` | --pane | maw-js source | NOT-PORTED ❌ | maw-js alias routes tmux zoom; Rust tmux subset lacks zoom. |
| `tile` | --cmd --engine --force --layout --path --porcelain --shell --wt... | maw-js source | NOT-PORTED ❌ | Built-in maw-js tile plugin; no Rust dispatcher entry. |
| `pane` | swap panes | maw-js source | NOT-PORTED ❌ | Built-in maw-js pane command; no Rust dispatcher entry. |
| `session` | --json --short | maw-js source | NOT-PORTED ❌ | Alias for whoami in maw-js builtins; no Rust session top-level entry. |
| `whoami` | --json --short | maw-js source | NOT-PORTED ❌ | No Rust dispatcher entry. |
| `workon` | --layout | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `workspace` | agents\|create\|invite\|join\|leave\|list/ls\|share\|status\|unshare; --hub/--workspace/--ws | maw-js source | WASM ✅ | WASM parity covers ls/list/default only; mutating workspace verbs remain gap. |
| `bg` | --all --dry-run --follow --json --lines --name --older-than | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `park` | ls and park note flow | maw-js source | WASM ✅ | WASM batch3 covers ls and note flow with git host calls. |
| `sleep` | --all-done | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `soul-sync` | agents\|pull; --from --git-common-dir --project --show-toplevel | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `stop` | no notable flags | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `resume` | no notable flags | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `reunion` | --git-common-dir | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `shellenv` | bash\|fish\|zsh | maw-js source | WASM ✅ | WASM batch1 parity covers shellenv bash/fish/zsh/default. |
## Discord

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `discord` | access\|bind\|channels\|check\|guilds\|inventory\|ls\|members\|pair\|route\|serve\|status\|tokens\|version; --apply --check --force --json --redact --restart --session --version | maw-js source | stub ⚠️ | Rust native discord exists and #74 merged REST subset (version, inventory/access list, members safety). Full maw-js discord command surface remains partial. |
## Consent / auth / policy

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `auth` | sign/verify/hash/hmac/from/loopback/constants parser plans | maw-js + maw-rs source | stub ⚠️ | Rust has native auth plan/test matrix; maw-js auth surface not in vendor list here, so keep partial until source-level exact command mapping is reconciled. |
| `consent` | approve\|reject\|list\|list-trust\|trust\|untrust; --help | maw-js + maw-rs source | WASM ✅ | WASM parity covers read-only list/list-trust only; Rust has low-level consent-* plan commands, not top-level maw consent approvals. |
| `pair` | generate; --at --expires | maw-js + maw-rs source | stub ⚠️ | Rust pair-code/pair-api low-level entries exist; maw-js top-level pair generate surface not directly matched. |
| `trust` | add\|remove/rm/delete\|list/ls; --yes | maw-js + maw-rs source | NOT-PORTED ❌ | No top-level Rust trust command; low-level consent-trust-* only. |
| `scope` | create/delete/info/list/ls/new/remove/rm/show; --lead --members --ttl --yes | maw-js #642 contract + #303 tests | native ✅ | Scope files, symmetric scope-trust.json, pending approval queue, and peer-send ACL gate are live. Cross-scope untrusted peer sends queue; corrupt ACL/trust fails open with loud warning. |
| `auto-pair-proof` | low-level proof helper | maw-js + maw-rs source | native ✅ | Rust native plan helper; not a maw-js top-level vendor command. |
| `recent-hello` | low-level pairing helper | maw-js + maw-rs source | native ✅ | Rust native plan helper; not direct maw-js top-level vendor command. |
| `pair-code / pair-code-store / pair-api / pair-api-auto` | low-level pairing helpers | maw-js + maw-rs source | native ✅ | Rust native plan helpers; do not count as top-level pair parity. |
| `policy / plugin-policy / split-policy` | policy plan helpers | maw-js + maw-rs source | native ✅ | Rust native plan helpers; no equivalent maw-js top-level plugin row found. |
## Plugin host / built-ins

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `plugin` | init\|build\|dev\|install\|create\|ls\|info\|remove\|enable\|disable; many lifecycle flags | maw-js + maw-rs source | stub ⚠️ | Rust plugin/plugin-scaffold/plugin-manifest cover manifests/scaffold and Rust-WASM build/dev. The ship-tier host does not yet provide a reviewed JS/TS-to-WASM build path, so that path fails closed; Bun/JS fleet-plugin and dev-tier surfaces remain first-class. Full maw-js lifecycle install/search/lock remains partial. |
| `plugins` | ls\|info\|remove\|lean\|standard\|full\|nuke\|enable\|disable; --json --all -v filters | maw-js + maw-rs source | NOT-PORTED ❌ | maw-js core route; no Rust dispatcher entry named plugins. |
| `plugin-manifest` | parse\|load\|discover\|import-symbol\|invoke; --scan-dir --plugin --source --arg --disabled --runtime-version --plan-json | maw-js + maw-rs source | native ✅ | Rust-native manifest/registry/WASM host CLI exists; supports test fixtures and import/invoke plan output. |
| `plugin-scaffold` | scaffold plugin dirs | maw-js + maw-rs source | native ✅ | Rust-native plugin scaffold exists. |
| `completions` | bash\|fish\|zsh\|commands; --help | maw-js + maw-rs source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `oracle-skills` | --help | maw-js + maw-rs source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `oracle-workon` | --all --dry-run --engine --force --no-attach --prompt --split --task --tiled --with --work | maw-js + maw-rs source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `artifact-manager` | init\|create\|write\|attach\|list/ls\|show\|get; --json --team | maw-js + maw-rs source | NOT-PORTED ❌ | Vendor plugin; no Rust dispatcher entry/WASM fixture. |
| `artifacts / artifact` | ls\|get [team] [task-id] --json | maw-js + maw-rs source | NOT-PORTED ❌ | maw-js core route; no Rust dispatcher entry. |
| `agents / agent` | --json --all --node | maw-js + maw-rs source | NOT-PORTED ❌ | maw-js core route; no Rust dispatcher entry. |
| `audit` | [limit] | maw-js + maw-rs source | NOT-PORTED ❌ | maw-js core route; no Rust dispatcher entry. |
| `config` | set/get-ish config; --json | maw-js + maw-rs source | WASM ✅ | WASM parity covers config set node and set port --json; secret-like set is host-gated. Full config surface remains limited. |
| `channel` | add/remove/list/setup; channel setup flags | maw-js + maw-rs source | NOT-PORTED ❌ | Built-in maw-js command; no Rust dispatcher entry. |
| `discover` | --awake --json --peers --tree | maw-js + maw-rs source | native ✅ | Rust native discover plan exists; compare exact built-in output before closing. |
| `fuzzy / resolve / identity / normalize / calver / xdg / bind-host / hub / auto-wake / route / worktree-window` | Rust helper commands | maw-js + maw-rs source | native ✅ | Native Rust support commands; mostly not direct maw-js plugin rows but needed for parity internals. |
| `ffi Tier-2` | FFI plugin host | maw-js + maw-rs source | stub ⚠️ | Won't-do/full Tier-2 deferred per issue #70; keep as stub reason rather than blank. |
## Fleet / orchestration / misc plugins

| command | subcommand(s) / notable flags | maw-js | maw-rs status | notes |
| --- | --- | --- | --- | --- |
| `team` | add\|assign\|bring\|check\|create\|delete\|done\|down\|enter\|history\|invite\|list/ls\|members\|msg\|plan\|preflight\|prune\|reassign\|remove/rm\|resume\|send\|send-enter\|shutdown\|spawn\|status\|task/tasks\|up; many flags | maw-js source | NOT-PORTED ❌ | Large maw-js orchestration plugin; no Rust dispatcher entry. |
| `swarm` | --count --parent --session-id --split --tiled --worktree/--wt | maw-js source | NOT-PORTED ❌ | Built-in maw-js command; no Rust dispatcher entry. |
| `mega` | ls\|status\|stop\|kill\|tree; team-lead variants | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `avengers` | all\|best\|health\|status\|traffic; --help | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `assign` | --oracle | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `peers` | accept\|add\|forget\|info\|list/ls\|probe\|probe-all\|remove/rm\|tofu-bootstrap; --alias --all --allow-unreachable --discovered --json --limit --node --ssh --timeout --user | maw-js source | NOT-PORTED ❌ | Explicit #55 gap: maw-js peers is ~1798 LOC and not ported; Rust only has peer-sources/peer-probe helpers. |
| `peer-sources / peer-probe` | source/probe helpers | maw-js source | native ✅ | Rust-native helpers; not full top-level peers parity. |
| `activity` | --all --json --sampler --samples --stuck-only --watch --window | maw-js source | NOT-PORTED ❌ | Explicit #55 gap: ~624 LOC maw-js plugin not ported. |
| `follow` | --grep --json --quit-on-idle --since | maw-js source | NOT-PORTED ❌ | Explicit #55 gap: ~343 LOC maw-js plugin not ported. |
| `pulse` | active\|add\|clean\|cleanup\|list/ls\|orphan\|stale; --dry-run --oracle --priority --sync --worktree/--wt | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `inbox` | approve\|drain\|pending\|read\|reject\|show\|status\|write; --all --dry-run --from --json --last --max --older-than-hours --safe --unread | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `cross-team-queue` | headless queue surface | maw-js source | WASM ✅ | WASM batch1 parity fixture covers no-arg output; source dispatcher treats non-CLI surfaces specially. |
| `fleet` | doctor/init/health/consolidate/resume/sync/wake; --json --dry-run --fix --reboot etc. | maw-js source | NOT-PORTED ❌ | Built-in maw-js fleet command; no Rust top-level fleet dispatcher entry. |
| `oracle` | about\|fleet\|list\|nickname\|prune\|register\|scan\|search\|stale; --json etc. | maw-js source | NOT-PORTED ❌ | Built-in maw-js oracle command; no Rust top-level oracle dispatcher entry. |
| `bud` | agents/fleet/gh; --blank --dry-run --fast --force --from --issue --repo --root --scaffold-only --split --tiny etc. | maw-js source | NOT-PORTED ❌ | No Rust dispatcher entry. |
| `awaken` | --blank --dry-run --fast --force --from --issue --repo --root --seed --split --sync-peers --track-vault --trigger --yes | maw-js source | NOT-PORTED ❌ | No Rust dispatcher entry. |
| `incubate` | --blank --contribute --dry-run --fast --flash --force --from --issue --repo --root --split --trigger | maw-js source | NOT-PORTED ❌ | No Rust dispatcher entry. |
| `wake` | --all --all-local --attach --dry-run --fresh --from-snapshot --incubate --issue --kill --layout --list --main --new --no-attach --parent --peer --pick --pr --repo --resume --snapshot --solo --split --task --wt | maw-js source | stub ⚠️ | Rust async wake exists but falls back for some paths; full maw-js wake/worktree behavior and flags not fully native. |
| `bring / b` | alias for wake --split; --to/--pick/engine inherited | maw-js source | native ✅ | Rust native bring plan exists; still compare output against maw-js alias. |
| `work / awake / scaffold / new / promote / preflight / snapshots` | top aliases | maw-js source | NOT-PORTED ❌ | maw-js top aliases; no direct Rust entries except bring/b, wake and plugin-scaffold equivalents. |
| `project` | find\|incubate\|learn\|list\|search; --contribute --flash --offload | maw-js source | WASM ✅ | WASM batch1 covers project subcommands for no-host output. |
| `learn` | --deep --fast --mode | maw-js source | WASM ✅ | WASM batch1 covers learned args including unknown --turbo behavior. |
| `dream` | --all --between --date --format --gain --json --limit --oneline --pain --plan --porcelain --project --repo --since --speculate --state | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `costs` | --daily --days --json | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `signals` | --days --json --root | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `done` | --all --clean-branch --dry-run --force | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `pr` | --body --show-current --title | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `archive` | --dry-run --yes | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `absorb` | --dry-run --into | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `cleanup` | --ask --dry-run --json --prune-stale --repo --scope --worktrees --yes --zombie-agents/--zombies | maw-js source | WASM ✅ | WASM batch3 covers only --worktrees [--yes] --json; rest remains partial. |
| `forget` | --all --dry-run --force --json --yes | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `restart` | --no-update --ref --version | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `setup` | auto-wake; --dry-run --only --repo --user | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `user-setup` | --dry-run --json --porcelain | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `doctor` | --allow-drift --backend --capture --dry-run --errors --fix-sessions --fix-stale --fix-xdg --forward --gateway --json --manifest-path --migrate --no-prompt --plan --port --release --smoke --version | maw-js source | NOT-PORTED ❌ | No Rust top-level doctor entry. Do not confuse with OMX doctor. |
| `check` | tools/version; --version | maw-js source | WASM ✅ | WASM batch3 covers check tools with exec host transcript. |
| `demo` | --daily --fast | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `about` | no notable flags | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `overview` | --color --kill | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `profile` | active/current/info/list/ls/set/show/use | maw-js source | WASM ✅ | WASM parity covers current/list/show/use. |
| `triggers` | no notable flags | maw-js source | WASM ✅ | WASM parity covers no-arg output. |
| `on` | --once --timeout | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `token / tokens` | current\|list/ls/tokens\|load\|save\|scan\|use; --force --no-team | maw-js source | stub ⚠️ | Issue #55 native primitive: list/current implemented; use/save/load/scan return deferred stub. |
| `find` | --oracle | maw-js source | stub ⚠️ | Issue #55 ported native, but verify full maw-js output/source search parity before green. |
| `locate` | --json --path | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `rename` | no notable flags | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `ui` | --3d --dev --install --source --tunnel --version | maw-js source | NOT-PORTED ❌ | No native entry/WASM fixture. |
| `mqtt` | closed/won't-do | maw-js source | NOT-PORTED ❌ | Intentionally no-code/won't-do per issue #12; leave as reasoned not-ported. |
| `batch2 closed set` | closed/won't-do | maw-js source | NOT-PORTED ❌ | Issue #13 batch2 closed as no-code/won't-do where applicable; keep future rows explicit if source resurfaces names. |

## Follow-up issue seeds

- Split #55 remainder into separate issues: `peers`, `activity`, `follow`.
- Split #56 remainder into separate issues: full `tmux`, full `attach`, full `view`, full `split`, with flag/output golden tests. Keep existing Rust subset tests as regression coverage.
- Audit #67 option-injection coverage across every Rust exec/ssh boundary before marking attach-ssh/stream/tmux paths final-green.
- Promote WASM rows from “covered argv subset” to true parity only after every source subcommand/flag in this table has a golden output test or an explicit won't-do note.
