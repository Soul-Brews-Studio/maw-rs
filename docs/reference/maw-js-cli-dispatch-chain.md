# maw-js CLI Dispatch Chain

This is the maw-js routing model the maw-rs port should preserve. Source
anchors are from `/opt/Code/github.com/Soul-Brews-Studio/maw-js`.

## Entry Point Flow

`src/cli.ts` is the executable entry point:

1. Sets `MAW_CLI=1`.
2. Applies `--as <name>` before other imports through `applyInstancePreset()`.
   This can mutate `MAW_HOME` for `maw serve --as <name>` before path/config
   modules evaluate.
3. Reads `process.argv.slice(2)`, strips global verbosity flags
   (`--quiet`, `-q`, `--silent`, `-s`), and lowercases only `cmd = args[0]`
   for route selection. The original-cased `args` array continues downstream.
4. Logs audit for the command.
5. Handles `version`, `update`, and `upgrade` directly before plugin setup.
6. Resolves the plugin directory with `MAW_PLUGINS_DIR || mawDataPath("plugins")`.
7. Runs bootstrap for bundled/source plugins. During first-install source
   bootstrap, `runBootstrap()` loads config for `pluginSources`.
8. Calls `scanCommands(pluginDir, "user")`. This direct-file command registry
   loads config for `disabledPlugins`, then registers `.ts`, `.js`, and `.wasm`
   command files found directly under the plugin directory.
9. Runs `maybeAutoRestore(cmd)`.
10. Shows usage for no command or top-level help.
11. Calls `dispatchCommand(cmd, args)`.

Relevant source: `src/cli.ts:1-64`, `src/cli/plugin-bootstrap.ts:207-213`,
`src/cli/command-registry.ts:26-44`.

## Dispatch Ladder

`dispatchCommand()` is the central ladder:

1. `routeComm(cmd, args)` for core communication verbs such as `hey`, `send`,
   `notify`, and `peek`.
2. `routeTools(cmd, args)` for native/core tools such as `plugin`, `plugins`,
   `artifacts`, `agents`, `audit`, `tmux`, and `serve`.
3. Top-level aliases from `top-aliases.ts`.
4. Direct-file command registry: `matchCommand(args)` and `executeCommand(...)`.
5. Manifest plugin registry: `dispatchPluginRegistry(cmd, args)`.
6. Unknown-command handling, unique-prefix retry, fuzzy suggestions, and finally
   agent-name shorthand (`maw <agent> <message>` or `maw <agent>`).

Relevant source: `src/cli/dispatch.ts:17-53`, `src/cli/dispatch.ts:154-270`.

## Top Aliases

Top aliases run after `routeComm`/`routeTools` and before both plugin
registries. They are one-shot aliases: the alias target is not recursively
expanded as another alias.

There are two forms:

- Argv rewrite: replace the leading verb and continue normal dispatch.
  Example: `maw a neo` becomes `maw attach neo` because `a: ["attach"]`.
- Direct handler: call a statically imported function and stop dispatch.
  Examples include `maw ls`, `maw wake`, `maw awake`, `maw new`,
  `maw preflight`, and `maw wtf`.

Important consequences:

- `maw a <name>` enters plugin resolution as `attach <name>`.
- `maw attach <name>` is not itself an alias; the alias check returns null.
- `maw ls` does not reach plugin resolution because it is a direct handler.

Relevant source: `src/cli/top-aliases.ts:1-21`,
`src/cli/top-aliases.ts:66-123`.

## Native Commands vs Plugin Commands

Native/core commands are handled before plugin dispatch. If `routeComm` or
`routeTools` returns true, dispatch stops and no plugin registry can claim that
argv.

Examples:

- `maw hey ...` and `maw send ...` are core communication routes.
- `maw plugin ...`, `maw plugins ...`, `maw tmux ...`, `maw serve ...`,
  `maw agents ...`, and `maw audit ...` are native tool routes.
- `maw plugin install`, `maw plugin build`, and related lifecycle subcommands
  are caught by the native `plugin` route, then explicitly forwarded to the
  plugin-lifecycle package.
- `maw plugin ls` is caught by the native `plugin` route and forwarded to the
  legacy plural `cmdPlugins` handler. It does not reach general plugin
  dispatch.

Plugin commands are considered only after those native routes and aliases. A
plugin cannot override an earlier native route in the normal dispatch ladder.

Relevant source: `src/cli/dispatch.ts:25-29`,
`src/cli/route-tools.ts:223-330`.

## Direct-File Command Registry

This is the older/beta command registry loaded by `scanCommands(pluginDir,
"user")`. It scans only `.ts`, `.js`, and `.wasm` files directly under the
plugin directory. It does not discover package directories that contain
`plugin.json`.

Matching is by longest argv prefix against `command.name` from the loaded
module. `executeCommand()` imports TS/JS handlers or invokes WASM command
modules.

Relevant source: `src/cli/command-registry.ts:26-44`,
`src/cli/command-registry-match.ts:10-39`,
`src/cli/command-registry-execute.ts:22-104`.

## Manifest Plugin Registry

Package plugins are discovered by `discoverPackages()` from plugin directories
with `plugin.json`. The manifest dispatcher uses the effective CLI names:

- If `manifest.cli` exists, use `manifest.cli.command` plus
  `manifest.cli.aliases`.
- If `manifest.cli` is absent, default to `manifest.name` only when the plugin
  is an implicit legacy CLI plugin with an executable TS/WASM surface and no
  non-CLI surface.
- Headless/API/module/hook/cron/transport plugins do not participate in CLI
  dispatch.

`resolvePluginMatch()` does a two-pass ordered match:

1. Exact command: `cmdName === manifest.cli.command`.
2. Exact alias.
3. Prefix command with word boundary:
   `cmdName.startsWith(command + " ")`.
4. Prefix alias with word boundary.

Multiple winners in the active pass are ambiguous. A single winner is invoked
with:

- `source: "cli"`
- `args: remaining`, where `remaining` is the argv tail after the matched
  command words
- `matchedName`
- parsed manifest flags when declared

The dispatcher validates manifest-declared CLI flags before invocation and
checks missing/disabled plugin dependencies. `invokePlugin()` then imports a TS
plugin handler or runs a WASM plugin.

Relevant source: `src/cli/dispatch.ts:65-116`,
`src/cli/dispatch-match.ts:42-147`,
`src/plugin/registry.ts:135-220`,
`src/plugin/registry-invoke.ts:38-143`.

## `maw attach <name>` Chain

For `maw attach neo`:

1. `cli.ts` receives raw argv `["attach", "neo"]`.
2. Verbosity flags are stripped, leaving `args = ["attach", "neo"]`.
3. `cmd = "attach"`.
4. `version`/`update` direct handling does not match.
5. Bootstrap ensures bundled plugins, including the vendored `attach` package,
   are linked into the plugin directory when missing.
6. `scanCommands(pluginDir, "user")` runs, but the bundled `attach` package is
   a package directory with `plugin.json`, not a direct `.ts/.js/.wasm` command
   file, so this step is not the normal owner of `attach`.
7. `dispatchCommand("attach", args)` starts.
8. `routeComm("attach", args)` returns false.
9. `routeTools("attach", args)` returns false.
10. `resolveTopAlias(args)` returns null because only `a` is aliased to
    `attach`; `attach` is canonical.
11. `matchCommand(args)` checks the direct-file registry. In the standard
    bundled-package path, it does not match `attach`.
12. `dispatchPluginRegistry()` calls `discoverPackages()`.
13. `resolvePluginMatch(plugins, "attach neo")` sees the attach package
    manifest:

    ```json
    {
      "name": "attach",
      "cli": {
        "command": "attach"
      }
    }
    ```

14. The manifest command prefix matches because
    `"attach neo".startsWith("attach ")`.
15. The matched command has one word, so `remaining = ["neo"]`.
16. Declared plugin CLI flags are validated if present. The attach manifest
    does not declare `cli.flags`, so validation is permissive.
17. `invokePlugin(attach, { source: "cli", args: ["neo"], matchedName:
    "attach" })` imports `src/vendor/mpr-plugins/attach/index.ts`.
18. The attach plugin parses its own flags, extracts `name = "neo"`, and calls
    `cmdAttach(name, opts)`.
19. On success, output is printed and the process exits `0`; on plugin failure,
    error output is printed and the process exits with the plugin exit code or
    `1`.

For `maw a neo`, steps 1-9 differ only at the alias step:
`resolveTopAlias(["a", "neo"])` rewrites argv to `["attach", "neo"]`, then the
same manifest plugin path runs.

Relevant source: `src/cli.ts:21-61`, `src/cli/top-aliases.ts:66-123`,
`src/vendor/mpr-plugins/attach/plugin.json:1-15`,
`src/vendor/mpr-plugins/attach/index.ts:1-58`.

## `maw plugin ls` Chain

For `maw plugin ls`:

1. `cli.ts` receives `args = ["plugin", "ls"]` and `cmd = "plugin"`.
2. `dispatchCommand("plugin", args)` starts.
3. `routeComm("plugin", args)` returns false.
4. `routeTools("plugin", args)` matches the native `plugin` route.
5. `sub = args[1]?.toLowerCase()` is `"ls"`.
6. The lifecycle forwarding set does not include `ls`.
7. The legacy-management set does include `ls`, so `routeTools` loads
   `cmdPlugins` and `parseFlags`.
8. It parses plugin-list flags with skip index `2`.
9. It calls `cmdPlugins("ls", args.slice(2), flags)`.
10. `cmdPlugins` dispatches `ls`/`list` to `doLs(...)`.
11. `routeTools` returns true, so no top alias, direct-file command registry,
    or manifest plugin registry can run.

This means `maw plugin ls` is a native/core management command, not a normal
plugin command. The similarly named plugin-lifecycle package handles
subcommands such as `init`, `build`, `dev`, `install`, `search`, `registry`,
`pin`, and `unpin`, but `ls` intentionally stays on the legacy plural
`plugins` handler path.

Relevant source: `src/cli/route-tools.ts:248-315`,
`src/commands/shared/plugins.ts:56-103`.
