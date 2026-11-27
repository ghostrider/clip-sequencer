# Releasing

How to publish a new version of Clip Sequencer for Resolume (M11 Phase I3). Releases are built by GitHub Actions
(`.github/workflows/release.yml`) from a tag; nothing is uploaded by hand.

## 1. Prepare `main`

- `main` is up to date and **CI is green** for its latest commit.
- Decide the version (semver): `x.y.(z+1)` for fixes, `x.(y+1).0` for new features, and a bump of the sequence
  file format (`docs/SPEC.md` §6) always gets at least a minor version.

## 2. Set the version and the changelog

1. `Cargo.toml` → `[workspace.package]` → `version = "x.y.z"`. Between releases it carries the next version with
   `-dev` (`0.5.0-dev`, so test builds can't pass for the release); the release drops the suffix, and after it
   the next `-dev` version goes in (all three places, step 2.2).
2. `web/package.json` → `"version": "x.y.z"`, and the two `"version"` fields at the top of `web/package-lock.json`.
3. Check: `node scripts/check-version.mjs` → "x.y.z everywhere".
4. `CHANGELOG.md`: rename `## [Unreleased]` entries into a new section `## [x.y.z] - YYYY-MM-DD` (today's date),
   leave an empty `## [Unreleased]` above it, and add the link line at the bottom:
   `[x.y.z]: https://github.com/ghostrider/clip-sequencer/releases/tag/vx.y.z` (and update the `[Unreleased]`
   compare link to `vx.y.z...HEAD`).
5. Check: `node scripts/release-notes.mjs --release` → prints the section; these become the release notes.

## 3. Commit and push

```powershell
git commit -am "Release x.y.z"
git push origin main
```

Wait until CI is green for this commit.

## 4. Optional: dry run

On GitHub: **Actions → release → Run workflow** (branch `main`). It builds everything exactly like a release and
uploads the result as the workflow artifact `release-x.y.z` (the zip, `SHA256SUMS.txt`, the notes), but creates no
release. Download it, unzip, check the contents (one folder `clip-sequencer` with `seq-app.exe`,
`seq-app-cli.exe`, `ffgl\SeqView.dll`, licenses), and try the app.

## 5. Tag

```powershell
git tag -a vx.y.z -m "x.y.z"
git push origin vx.y.z
```

Only you can push `v*` tags (ruleset `protect-release-tags`). The tag starts the release workflow. Its build job
fails if the tag doesn't match the version, or if `CHANGELOG.md` has no dated section for it.

## 6. Approve

If code signing is on (`docs/github-setup.md`, "Code signing"): the build job sends the zip to SignPath and waits up
to an hour. Approve the signing request on https://app.signpath.io (you get an e-mail; the run's log links to it).
The build then checks the signatures and goes on.

The `publish` job waits for the `release` environment: **Actions → release → the run → Review deployments →
release → Approve and deploy**. It then creates a **draft** release with the zip and `SHA256SUMS.txt`.

## 7. Review and publish

**Releases → the draft**: check the title, the notes, both files (download the zip once and compare its hash with
`SHA256SUMS.txt`), then **Publish release**.

Publishing starts `.github/workflows/pages.yml`: the site and the manual are rebuilt from the tag ("Manual for
version x.y.z" in each page footer). Open https://ghostrider.github.io/clip-sequencer/ and check it.

The plugin's debug symbols (`symbols-x.y.z` artifact of the release run) are kept for 30 days; download and keep
them if you want to read crash dumps of this version later.

## Testing the in-app update (SPEC §15)

The app asks `https://api.github.com/repos/ghostrider/clip-sequencer/releases/latest`; the hidden option
`--update-source <url>` points it at a local stand-in instead, so a real release zip can be tested end to end
before it is published:

1. Build two versions with `scripts\release.ps1`: the one to update *from* (say 0.5.0) and a newer one (say 0.5.1,
   version bumped only locally). Unpack the older zip somewhere writable, e.g. `D:\test\clip-sequencer`.
2. Serve the newer zip: `node scripts/fake-update-source.mjs release\clip-sequencer-0.5.1-windows-x64.zip`
   (port 8765; `--notes "…"` for the release notes, `--prerelease` to check that one is ignored).
3. Start the older version with `seq-app.exe --update-source http://127.0.0.1:8765` (or `seq-app-cli.exe`;
   `seq-app-cli --check-update --update-source …` only prints the result: exit 0 up to date, 2 available, 1 error).
4. Check: the question at start-up (Download and install / Skip / Later), Skip (not asked again at the next start,
   still offered by Help → Check for updates), the install while a sequence runs (refused), the install itself
   (the new version starts and says *Updated to 0.5.1*; `update-<date>.log` in the log folder), no server running
   (only a log line at start-up, an error by hand), a read-only folder, Resolume running with the plugin loaded.
5. Rollback: serve a zip whose `seq-app.exe` doesn't start (for example one that exits at once); the old version must
   come back and say *Update to … failed*.

The real endpoint can be checked read-only any time: `seq-app-cli --check-update`.

## If something is wrong

- **Before publishing:** delete the draft release and the tag (`git push --delete origin vx.y.z`,
  `git tag -d vx.y.z`; you may need the admin bypass of the tag ruleset), fix it on `main`, and tag again.
- **After publishing:** never move or reuse a published tag. Fix it on `main` and release `x.y.(z+1)`.
- **A mistake only in the manual or the site** (typo, wrong screenshot) between releases: merge the fix to `main`,
  then **Actions → pages → Run workflow** with "Build from main". The version footer still shows the latest
  release; the next release deploys from its tag again.
