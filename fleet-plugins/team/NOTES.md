# team — read-only artifact slice 1 (#72)

Reference: `maw-js@746df172:src/vendor/mpr-plugins/team` (manifest v2.0.1). The
reference's command/alias/usage surface is locked in `contract.json`.

This first ship-tier slice implements only default/`list`/`ls`. It reads tool teams,
vault-only manifests, and live pane IDs with exactly `fs:read:teams`, `fs:read:vault`,
and `tmux:read`. The artifact is invoked directly in acceptance tests; native `maw team`
still owns CLI dispatch, so there is no partial cutover.

Task/member state, writes, lifecycle tmux operations, and invitation/consent remain
deferred pending their named-root and typed-ABI design decisions.
