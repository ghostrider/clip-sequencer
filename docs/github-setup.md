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

Kept for later, *not* created now: `WINDOWS_CERTIFICATE` / `WINDOWS_CERTIFICATE_PASSWORD` (if the executables are
code-signed one day), `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (only for an auto-updater).
They would go into the `release` environment, never into the repository secrets.

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
