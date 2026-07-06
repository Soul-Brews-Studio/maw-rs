# Release and CalVer guide

Condensed from `CLAUDE.md`; keep that file as the detailed Claude-facing release memory.

## Version scheme

maw-rs uses day-based CalVer:

```text
stable: v<YY>.<M>.<DD>
alpha:  v<YY>.<M>.<DD>-alpha.<HMM>
beta:   v<YY>.<M>.<DD>-beta.<HMM>
```

`HMM` is Bangkok wall-clock time as `hour * 100 + minute`, without a leading zero. If a
slot would not be greater than the highest existing suffix for that base/channel, advance
to the next calendar base. `maw --version` embeds the exact commit and build time.

## Promotion flow

1. Work lands by squash-merge PRs into `alpha`.
2. Release promotion moves `alpha` to `main` by a merge-commit PR.
3. Tag the promoted commit as stable (`v<YY>.<M>.<DD>`) or prerelease
   (`v<YY>.<M>.<DD>-alpha.<HMM>` / beta equivalent).
4. Publish the GitHub release.
5. Issues with `Fixes #N` in alpha PRs are closed by hand because GitHub only auto-closes
   on default-branch merges.

## macOS install note

When installing a replacement binary on macOS, remove the old file before copying the new
one. Reusing the inode can trip stale code-sign cache behavior and SIGKILL the next run:

```bash
rm -f <install-path>/maw
cp target/release/maw <install-path>/maw
```
