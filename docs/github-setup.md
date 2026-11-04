# GitHub setup checklist

Settings of `github.com/ghostrider/clip-sequencer` that only the owner can click (M11 Phase H). Work through it
top to bottom and tick each box. GitHub moves things around now and then: if a setting isn't where this says,
search for its name in the settings page.

Order matters in two places: the **required status checks** come after the first CI run (Phase I), and
**GitHub Pages** only works once the repository is public on a free account.

## H1 — Repository basics

Already done: the repository exists (private), `main` is pushed, no tags yet.

**Settings → General**

- [ ] **Default branch:** `main`.
- [ ] **Features:** Issues **on**; Wiki **off**; Projects **off**; Discussions as you like (off is fine).
- [ ] **Pull Requests:** **Allow squash merging** on; **Allow merge commits** off; **Allow rebase merging** off.
- [ ] **Automatically delete head branches:** on.
- [ ] **Releases → Enable release immutability** (or "Immutable releases"), if the option exists: published
      releases and their files can then no longer be changed.

## H2 — Actions permissions

**Settings → Actions → General**

- [ ] **Actions permissions:** "Allow ghostrider, and select non-ghostrider, actions and reusable workflows".
  - [ ] **Allow actions created by GitHub:** on (covers `actions/checkout`, `actions/setup-node`,
        `actions/upload-artifact`, `actions/download-artifact`, and the Pages actions).
  - [ ] **Allow specified actions and reusable workflows** (comma-separated):
        `dtolnay/rust-toolchain@*, Swatinem/rust-cache@*, EmbarkStudios/cargo-deny-action@*`
- [ ] **Artifact and log retention:** 30 days.
- [ ] **Approval for running fork pull request workflows from contributors:** "Require approval for first-time
      contributors".
- [ ] **Workflow permissions:** "Read repository contents and packages permissions" (read-only).
  - [ ] **Allow GitHub Actions to create and approve pull requests:** **off**.

The release workflow asks for write access itself, only in the job that publishes (Phase I).

## H3 — Branch and tag protection

**Settings → Rules → Rulesets → New ruleset → New branch ruleset**

- [ ] **Name:** `protect-main`; **Enforcement status:** Active.
- [ ] **Bypass list:** add **Repository admin** (you can still push to `main` directly while working alone).
- [ ] **Target branches:** "Include default branch".
- [ ] **Rules:** **Restrict deletions** on; **Block force pushes** on.
- [ ] Optional: **Require a pull request before merging** (with the bypass you can still push directly).
- [ ] Save. *Leave "Require status checks to pass" off for now: see "After the first CI run" below.*

**Settings → Rules → Rulesets → New ruleset → New tag ruleset**

- [ ] **Name:** `protect-release-tags`; **Enforcement status:** Active.
- [ ] **Bypass list:** **Repository admin** only.
- [ ] **Target tags:** "Add target → Include by pattern" → `v*`.
- [ ] **Rules:** **Restrict creations**, **Restrict updates**, **Restrict deletions** on.
- [ ] Save. Only you can create release tags, and a published tag can't be moved.

## H4 — Release environment

**Settings → Environments → New environment**

- [ ] **Name:** `release`.
- [ ] **Required reviewers:** add yourself (`ghostrider`). **Prevent self-review:** **off** (otherwise nobody
      can approve your own releases).
- [ ] **Deployment branches and tags:** "Selected branches and tags" → **Add deployment branch or tag rule** →
      Ref type **Tag**, pattern `v*`.
- [ ] **Environment secrets:** none. **Repository secrets:** none. The workflows use the automatic
      `GITHUB_TOKEN`.

Kept for later, *not* created now: `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (only for an
auto-updater). They would go into the `release` environment, never into the repository secrets.

## Code signing (SignPath Foundation)

The release workflow signs `seq-app.exe`, `seq-app-cli.exe` and `ffgl/SeqView.dll` through SignPath once this is set
up; until then releases stay unsigned and nothing else changes. The policy that SignPath requires is
`site/landing/code-signing.html` (https://ghostrider.github.io/clip-sequencer/code-signing.html).

1. [ ] The policy page is live (deployed with the site), and multi-factor authentication is on for GitHub.
2. [ ] Apply at https://signpath.org/ (Apply). Repository `https://github.com/ghostrider/clip-sequencer`, license MIT,
       homepage and code signing policy as above, build system GitHub Actions (`.github/workflows/release.yml`).
3. [ ] After approval, in **SignPath** (https://app.signpath.io), with multi-factor authentication on:
   - the project slug is `clip-sequencer` (otherwise set the repository variable `SIGNPATH_PROJECT_SLUG`);
   - its artifact configuration is `.signpath/artifact-configurations/release.xml` (paste it in as the default);
   - the signing policies are `release-signing` and `test-signing` (the names release.yml uses), with the
     GitHub trusted build system linked to the project;
   - create a CI user (submitter on both policies) and copy its API token; you stay the approver.
4. [ ] On GitHub, **Settings → Environments → New environment** `signing`: no reviewers (SignPath asks for the
       approval); **Deployment branches and tags** → "Selected branches and tags": branch `main` and tag `v*`;
       **Environment secrets** → `SIGNPATH_API_TOKEN` = the CI user's token.
5. [ ] **Settings → Secrets and variables → Actions → Variables** → **New repository variable**
       `SIGNPATH_ORGANIZATION_ID` = the organization ID from SignPath. This switches signing on.
6. [ ] Test: **Actions → release → Run workflow** on `main` signs with `test-signing` (no approval needed, test
       certificate). The step "Check the zip" lists the three files with their signer.
7. [ ] With the next tag, approve the signing request on SignPath (the run waits up to an hour), then the
       `release` environment as before. Update the Status line on the policy page, and the unsigned-download notes
       in the manual (`installation.md`) and the README.

## H5 — Security

**Settings → Advanced Security** (on some accounts: **Code security**)

- [ ] **Private vulnerability reporting:** Enable (`SECURITY.md` points people there).
- [ ] **Dependabot alerts:** Enable.
- [ ] **Dependabot security updates:** Enable.
- [ ] **Secret scanning:** Enable, and **Push protection:** Enable (both available for public repositories; on a
      private one they may need a paid plan; then enable them after going public).

Already in the repository: `.github/dependabot.yml` (weekly cargo, npm and Actions updates, minor and patch
grouped, at most 5 open PRs each) and `.github/CODEOWNERS` (`* @ghostrider`).

## H6 — GitHub Pages (project site and manual)

On a free account, Pages for a private repository isn't available: do this right after making the repository
public (Phase I4). On a paid plan you can do it now.

**Settings → Pages**

- [ ] **Build and deployment → Source:** **GitHub Actions** (not "Deploy from a branch").
- [ ] The site will be at `https://ghostrider.github.io/clip-sequencer/` (no custom domain, Phase A).

**Settings → Environments → `github-pages`** (appears after the first deployment)

- [ ] **Deployment branches and tags:** "Selected branches and tags": a **Tag** rule `v*` and a **Branch** rule
      `main` (for manual redeploys of the manual).

**Repository main page → About (gear icon)**

- [ ] **Website:** `https://ghostrider.github.io/clip-sequencer/`.
- [ ] **Description:** "Play multi-layer clip sequences in Resolume Arena, locked to your show's MIDI clock."
- [ ] **Topics:** `resolume`, `vj`, `midi-clock`, `sequencer`, `ffgl`, `osc`.

## After the first CI run (Phase I)

Once `ci.yml` has run on `main`, the job names exist and GitHub can offer them.

**Settings → Rules → Rulesets → `protect-main` → edit**

- [ ] **Require status checks to pass:** on; **Require branches to be up to date before merging:** on.
- [ ] **Add checks:** `rust`, `web`, `deny`, `ffgl`, `site` (pick them from the list; GitHub shows each with
      the source "GitHub Actions").
- [ ] Save.

## Done

- [ ] Everything above is ticked (except "After the first CI run" and, on a free account, H6).
- [ ] Tell Claude, so `docs/M11-open-source-release.md` Phase H can be ticked.
