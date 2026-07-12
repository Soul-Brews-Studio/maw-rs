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
4. Publish the GitHub release. The tag workflow uploads the macOS arm64 binary and
   checksum. For stable tags it also generates `maw.rb` from that checksum and pushes the
   formula to `Soul-Brews-Studio/homebrew-maw` using the `HOMEBREW_TAP_TOKEN` repository
   secret. Alpha and beta tags do not update the stable formula.
5. Verify the tap after the workflow finishes:

   ```bash
   brew update
   brew upgrade maw # or: brew install soul-brews-studio/maw/maw
   maw --version
   maw ls
   ```

6. Issues with `Fixes #N` in alpha PRs are closed by hand because GitHub only auto-closes
   on default-branch merges.

The tap repository layout is `Formula/maw.rb` plus its top-level `README.md`. See
`docs/install.md` for user installation and version-pinning commands. If formula
automation fails, download `maw.rb` from the stable GitHub release and commit it to the
tap; do not hand-edit its version or checksum.

## macOS install note

When installing a replacement binary on macOS, remove the old file before copying the new
one. Reusing the inode can trip stale code-sign cache behavior and SIGKILL the next run:

```bash
rm -f <install-path>/maw
cp target/release/maw <install-path>/maw
```
