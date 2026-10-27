# M11 — Open-source release (MIT, GitHub)

Goal: publish the sequencer as its own GitHub repository under the MIT License, with
versioned, reproducible Windows releases. The release contains the sequencer app **and the FFGL
plugin** built for it (Phase G3). **No new product features**, with one exception requested by Uli:
a way to deploy the FFGL plugin into Resolume's plugin folder (Phase G3, "Plugin deployment").
Everything else here is legal minimum, release hygiene, or UI for information that already exists.

Work through the phases in order. Tick checkboxes as you go. Stop after each phase,
summarise, and wait for Uli before continuing. Items marked **[Uli]** need a decision or an
action from Uli. Ask, don't guess.

---

## Phase A — Decisions and checks [Uli]

- [ ] **Copyright holder** for the LICENSE line: real name (`Ulrich Staudinger`) or artist
      name (`Sun92`). Default: `Copyright (c) 2026 Ulrich Staudinger`.
- [ ] **Project name.** Must not be "Resolume …". Pattern "<Name> for Resolume". Check
      that the name is free on GitHub (and crates.io, if publishing there later).
- [ ] **GitHub owner/repo** (user account or org), e.g. `github.com/<owner>/<repo>`.
- [ ] **Starting version:** default `0.1.0`.
- [ ] **Binary distribution:** (a) free on GitHub Releases, (b) GitHub Releases are source-only and
      prebuilt binaries are sold through a shop, or (c) both. This decides what the release workflow
      attaches (Phase I). Default: (a).
- [ ] **Release approval:** should a published release need a manual click from Uli (draft release plus
      a protected `release` environment)? Default: yes.

- [ ] **Manual language:** English only, or English + German. Default: English only (German can follow later).
- [ ] **Site address:** the default GitHub Pages URL `https://<owner>.github.io/<repo>/`, or a custom
      domain (e.g. a subdomain of a domain Uli already owns). Default: the GitHub Pages URL.
- [ ] **Public contact address** for the site and the manual (a project e-mail address, not a personal
      inbox). It's needed for the contact / legal notice page (Phase G2).
- [ ] **FFGL plugin:** its name, what it does, and where its source currently lives (it was built in a
      separate Claude Code session). Claude reads the plugin's source and README to fill in the details,
      and asks Uli only what it can't find there.
- [ ] **FFGL plugin location:** (a) move it into this repo under `ffgl/`, with its history (monorepo, one
      release, one version), or (b) keep it in its own repo, and the release workflow fetches a pinned
      version. Default: (a).

Done when: all eleven are answered and recorded at the bottom of this file under "Decisions".

## Phase B — Extract into its own repository

The sequencer currently lives in `vj_stuff/resolume-sequencer/` inside the `vj_stuff` repo.

- [ ] Extract the subfolder **with its history** into a new local repo, using either
      `git subtree split --prefix=resolume-sequencer -b sequencer-export` or
      `git filter-repo --subdirectory-filter resolume-sequencer` on a fresh clone.
      Never run filter-repo on the original `vj_stuff` checkout.
- [ ] Verify that the new repo builds and all tests pass on its own (no paths into `../`).
- [ ] `CLAUDE.md` in the new repo must not reference the parent `vj_stuff/CLAUDE.md`,
      personal paths, or other projects. Move anything general that it needs from the parent file
      into it.
- [ ] If Phase A chose (a) for the FFGL plugin: import it into `ffgl/` with its history, the same way
      (`git subtree split` / `git filter-repo --subdirectory-filter <plugin folder>` on a fresh clone of
      its repo, then `git subtree add --prefix=ffgl` or a merge with `--allow-unrelated-histories`).
      Uli's own plugin install script stays where it is; it isn't part of the release (see Phase G3).
- [ ] Do **not** push yet (pushing happens in Phase H1).

## Phase C — Scrub content and history

Scan the **working tree and the full git history** of the new repo.

- [ ] `docs/samples/` (M0 probe output): replace real clip names, media file paths, user
      names and machine names with neutral sample data. Keep the JSON structure identical
      so tests still pass.
- [ ] No real show files (`*.seq.json`), thumbnail cache, `.avc` compositions, media clips
      or screenshots containing third-party footage.
- [ ] The FFGL plugin source and history are included in the scan: no built DLLs, PDBs, local paths in
      CMake caches or build scripts, and no copies of SDK files whose license doesn't allow redistribution.
- [ ] No secrets, tokens, private IPs or hostnames in configs, logs or tests.
- [ ] Search history with `git log -p` / `git grep $(git rev-list --all)` for: `C:\\Users`,
      `/Users/`, `.avc`, `.mov`, `.mp4`, `token`, `password`, `192.168.`. Report findings to Uli
      before rewriting any history.
- [ ] If history has to be rewritten, do it with `git filter-repo` before the first push.
- [ ] `.gitignore` covers `target/`, `web/node_modules/`, `web/dist/`, the thumbnail cache,
      local `seq.toml`, `*.seq.json` outside fixtures, and logs.

## Phase D — License files and third-party notices

- [ ] `LICENSE` in the repo root: standard MIT text, with the copyright line from Phase A.
- [ ] `Cargo.toml`: `license = "MIT"`, `repository`, `description`, `authors`,
      `readme`, `keywords`, `categories` in `[workspace.package]`. All crates inherit them.
      Internal crates and tools get `publish = false`.
- [ ] **Rust third-party notices:** add `cargo-about` with an `about.toml` and template that
      generates `THIRD_PARTY_LICENSES.md` (or `.html`) from the dependency tree.
- [ ] **Web UI third-party notices:** add `rollup-plugin-license` (Vite) so the build emits
      the npm dependency licenses. Merge them into the same third-party file, or a second section of it.
- [ ] **License policy in CI:** add `cargo-deny` with `deny.toml`. Allow MIT, Apache-2.0,
      BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0 (file-level). Deny GPL/AGPL/LGPL and unknown
      licenses. Also enable the advisories check. Report any dependency that fails.
- [ ] **Fonts:** stop loading IBM Plex from Google Fonts. Bundle the woff2 files in
      `web/src/assets/fonts/` (the UI must work offline at venues). Add `OFL.txt` (SIL Open
      Font License 1.1) next to them, and list IBM Plex in the third-party notices.
- [ ] **FFGL SDK and plugin dependencies:** record the license of the FFGL SDK (Resolume's `ffgl`
      repository) **[verify the exact license text from the SDK repo]** and of any other library the plugin
      links or vendors (e.g. GLEW, GLM). Add them to the third-party notices, and include their license texts in
      the release. If the SDK is vendored as a git submodule, pin it to a commit.
- [ ] Optional: SPDX header `// SPDX-License-Identifier: MIT` at the top of source files.
      Ask Uli first.

## Phase E — Versioning and build info

- [ ] **Single version source:** `version` lives only in `[workspace.package]`. All crates use
      `version.workspace = true`. `web/package.json` gets the same version, synced by a small
      script or checked in CI.
- [ ] **Build info** embedded at compile time via `build.rs` (or `vergen`):
  - `version` (semver from Cargo)
  - `build_number`: the git commit count (`git rev-list --count HEAD`). It's identical in local
    builds, CI and the release workflow, and reproducible from any checkout (CI must check out with
    `fetch-depth: 0`). Falls back to `0` if git isn't available. Don't use `GITHUB_RUN_NUMBER`: each
    workflow has its own counter, so CI and release builds of the same commit would get different numbers.
  - `git_commit`: short hash, with `-dirty` if the tree has changes
  - `build_date`: UTC, ISO 8601
  - `target`: e.g. `x86_64-pc-windows-msvc`
- [ ] Expose the build info:
  - [ ] `seq-app --version` prints `<name> 0.1.0 (build 42, abc1234, 2026-10-27)`
  - [ ] the first log line at startup
  - [ ] `GET /api/version` returns JSON with all fields
  - [ ] the About screen (Phase F)
- [ ] **Windows exe metadata:** embed version info (FileVersion, ProductVersion,
      CompanyName = copyright holder, LegalCopyright, ProductName) and an app icon via
      `embed-resource` or `winres`. **[Uli]** provides the icon, or Claude makes a simple
      placeholder `.ico`.
- [ ] **FFGL plugin version:** the plugin gets the same version as the app (monorepo) or its own pinned
      version (separate repo). It's embedded in the DLL's Windows version resource (FileVersion, ProductVersion,
      LegalCopyright), and, where the FFGL API allows it, in the plugin's info/about fields shown in Resolume.
      The app must be able to read the version of an installed DLL (needed for "Plugin deployment", Phase G3).
- [ ] Release tags follow `vMAJOR.MINOR.PATCH`. CI fails if the tag doesn't match the Cargo version.

## Phase F — UI: menu bar, About, License, shortcuts

Use only the existing design tokens (`web/src/tokens.css`). Every item opens an existing
function or a static info dialog. No new behaviour.

- [ ] **Menu bar** at the top of the app, above or merged into the top bar, keyboard-accessible
      (Alt or F10 focuses it; arrow keys move; Esc closes):
  - **File:** Reload composition · Save now · Open data folder (only if this is trivial
    via the server; otherwise leave it out) · Quit server (with confirmation)
  - **View:** Zoom in · Zoom out · Reset zoom
  - **Help:** Keyboard shortcuts · User manual (opens the manual on the project site, Phase G2,
    in the system browser) · Report an issue (GitHub issues/new) · Licenses · About
- [ ] **About dialog:** app name and icon; version, build number, commit, build date (from
      `/api/version`); `Copyright (c) 2026 <holder>`; "Released under the MIT License"
      linking to the License dialog; GitHub repository link; the disclaimer *"Not affiliated with or
      endorsed by Resolume BV. Resolume is a trademark of Resolume BV."*; *"Provided as is,
      without warranty of any kind."*; links to the project site and the user manual.
- [ ] **License dialog:** the project's MIT text first, then the third-party notices,
      grouped per package (name, version, license, collapsible full text). Data is served by the app
      from the generated file embedded at build time.
- [ ] **Keyboard shortcuts dialog:** a list of the shortcuts that already exist (SPEC §9.3).
- [ ] Each dialog closes with Esc and the close button, traps focus, and returns focus to its menu item.

## Phase G — Repository documents and security defaults

- [ ] `README.md` (aimed at developers and first-time visitors on GitHub; end users are pointed to the
      project site and manual from Phase G2, which the README links prominently at the top):
  - [ ] what it does (2–3 sentences) and a screenshot (with neutral demo content, no third-party footage)
  - [ ] requirements: Windows 10/11, Resolume Arena 7.26+ with the webserver and OSC input enabled,
        a MIDI clock source (e.g. a virtual MIDI port)
  - [ ] install (download the release zip), run, CLI flags / `seq.toml`, the SEQ-layer convention,
        where data, cache and logs live
  - [ ] SmartScreen note: the exe is unsigned, so Windows may warn ("More info → Run anyway")
  - [ ] building from source
  - [ ] the Resolume trademark disclaimer and a license section
- [ ] `CHANGELOG.md` in the Keep a Changelog format; first entry `0.1.0` summarises the current feature set.
- [ ] `CONTRIBUTING.md`: build/test commands, code style (fmt, clippy), PR expectations, and
      that the sequence file format changes only with a version bump and a migration.
- [ ] `SECURITY.md`: how to report issues privately (GitHub private vulnerability reporting).
- [ ] **Security default:** the UI server binds to `127.0.0.1` by default. Binding to other
      interfaces requires an explicit flag (`--ui-bind 0.0.0.0`) and logs a warning. If the app
      currently binds to all interfaces, this change is part of M11. Document it in the README.
- [ ] `.github/ISSUE_TEMPLATE/bug_report.yml` (asks for app version/build, Resolume version,
      Windows version, MIDI clock source, a log excerpt) and `feature_request.yml`.
- [ ] `.github/pull_request_template.md` (short checklist: tests, clippy, changelog entry, manual updated if user-facing behaviour changed).
- [ ] Optional: `.editorconfig`; `CODE_OF_CONDUCT.md` (Contributor Covenant). Ask Uli.

## Phase G2 — End-user manual and project site (GitHub Pages)

The manual is for VJs who use the sequencer, not for developers. It assumes they know Resolume,
but not this tool. It's published, together with a small project site, on GitHub Pages
(configuration in H6, deployment in I5).

### Structure and tooling
- [ ] Sources live in `site/`, separate from the developer docs in `docs/`:
  ```
  site/
    landing/        hand-written static landing page (index.html, CSS, images)
    manual/         mdBook project (book.toml, src/SUMMARY.md, src/*.md, src/images/)
    shared/         tokens.css and the self-hosted fonts, copied from web/ at build time
    build.ps1       assembles landing + manual into site/dist/ (manual under /manual/)
  ```
- [ ] Tool: **mdBook** for the manual (Rust ecosystem, search built in, works offline as static
      files), plus a hand-written landing page. The landing page uses the app's design tokens, so
      the site and the app look like one product.
- [ ] Base path: the site is served under `/<repo>/` on the default GitHub Pages URL. Set
      `site-url` in `book.toml` accordingly, and use only relative links in the landing page. If a custom
      domain is chosen in Phase A, the base path is `/`.
- [ ] **No external requests:** no Google Fonts, no CDN, no analytics, no cookies, no embedded videos from
      third-party hosts. Fonts are self-hosted. (That keeps the site free of tracking and cookie-banner
      obligations under Swiss and EU data-protection law.)

### Manual content (`site/manual/src/`)
Write it in the Phase A language. Every page is task-oriented: what you want to do, then the steps.
- [ ] **Introduction:** what the sequencer does, how it fits next to Resolume and the MIDI clock, requirements.
- [ ] **Installation:** installer vs. portable zip, WebView2 (after M12), the SmartScreen warning, checksum
      verification, updating to a new version, uninstalling (and where the data stays).
- [ ] **Setting up Resolume:** enable the webserver and OSC input (with the exact preference paths), MIDI clock
      sync, Beat Snap **off** for clip triggers, recommended clip transport mode (BPM sync), creating the `SEQ`
      layer with `§ <name>` placeholder clips and an invisible source.
- [ ] **Setting up the clock:** any MIDI clock source works; selecting the input port; using a virtual MIDI
      port (e.g. loopMIDI); a note on the separate Clock Shifter for projector latency.
- [ ] **Quick start:** a step-by-step tutorial, from an empty sequence to a 2-lane, 4-bar sequence started from
      Resolume, with a screenshot per step.
- [ ] **The interface:** top bar (deck tabs and LIVE, connection chips, Stop all), sequence list, editor (lanes,
      steps, rests, the 1/8-bar grid, Repeat, lane end behaviour), clip browser, step inspector, status bar,
      Console tab (after M12), menus.
- [ ] **Working with decks:** sequences belong to one deck; viewing vs. starting; the deck switch on start and its
      confirmation.
- [ ] **Playing live:** starting and stopping (UI and SEQ clips), edits apply at the next start, manual override and
      "Resume lane", Stop all, what happens on clock loss.
- [ ] **Files and data:** the `.seq.json` file next to the composition, backups, moving a show to another machine,
      orphaned sequences after a deck rename, how clips are re-found after renames (ID → name → file) and what
      red steps mean.
- [ ] **Thumbnails:** where they come from, cached thumbnails for inactive decks, "Push thumbnail to Resolume".
- [ ] **Settings reference:** all CLI flags and `seq.toml` keys with defaults, UI port fallback and `--strict-port`,
      `--headless`, bind address and its security note.
- [ ] **Keyboard shortcuts:** generated from, or checked against, the in-app shortcuts dialog (Phase F), so they never diverge.
- [ ] **Troubleshooting:** no clock / clock not locked; Resolume not reachable; triggers "not confirmed"; red
      (unresolved) steps; port in use / app already running; SmartScreen; where the logs are and what to attach to a
      bug report.
- [ ] **FAQ** and **Glossary** (sequence, lane, step, pass, deck, LIVE, yield, SEQ clip).
- [ ] **About:** version history (links to the changelog), license and third-party notices, the Resolume trademark
      disclaimer, how to report bugs.

### Screenshots
- [ ] Screenshots show **neutral demo content only** (generated colour clips, no third-party footage, no real show
      names). Use `tools/mock-resolume` with a demo composition.
- [ ] Optional, ask Uli: a script (Playwright) that starts the app against the mock and regenerates all manual
      screenshots, so they stay current after UI changes.

### Landing page (`site/landing/`)
- [ ] One page: name and one-line pitch; a hero screenshot (or a short looping GIF/WebM, self-hosted); 4–6 key
      features; requirements; a **Download** button (latest GitHub Release, or the shop link, per the Phase A
      distribution decision); links to the manual, the source on GitHub, the changelog and issues; the license line
      and the Resolume disclaimer in the footer.
- [ ] A **Contact / legal notice** page with the copyright holder's name and the public contact address from
      Phase A. If binaries are sold (distribution (b) or (c)), this page must also meet the Swiss e-commerce
      information duties (clear identity and a contact address, Art. 3 para. 1 lit. s UWG). **[Uli]** reviews the
      wording; this isn't legal advice.
- [ ] A short **Privacy** note: the site uses no cookies or tracking; GitHub hosts it (link to GitHub's privacy
      statement).
- [ ] Works at phone width, passes basic accessibility checks (contrast, alt text, keyboard), and has a favicon and
      Open Graph tags (title, description, preview image) for link previews.

### Versioning
- [ ] The published manual always matches the **latest release**, not `main`. It's deployed by the release
      process (I5), so unreleased features never show up in the public manual.
- [ ] Each manual page footer shows "Manual for version x.y.z". The version is injected at build time from
      `[workspace.package].version`.
- [ ] Changes to user-facing behaviour must update the manual in the same PR (add this to `CONTRIBUTING.md` and
      the PR template checklist).

## Phase G3 — FFGL plugin in the release

### Build
- [ ] The plugin builds from the repo with one command (CMake + MSVC, x64, Release), documented in
      `CONTRIBUTING.md`. If it currently only builds through a Visual Studio solution, add a CMake build or a
      scripted `msbuild` call that works on a CI runner.
- [ ] Link the MSVC runtime **statically** (`/MT`), so the DLL doesn't need the Visual C++ Redistributable on
      the user's machine. Verify with `dumpbin /dependents`: only system DLLs and `opengl32.dll`.
- [ ] Built outputs (`.dll`, `.pdb`, build folders) are git-ignored. The release ships the `.dll` only; `.pdb`
      files are kept as a workflow artifact for debugging crash reports.
- [ ] Uli's existing personal install script is **not** used by CI or the release. The deployment feature below
      replaces it for end users. Claude may read the script to reuse its logic (target folder, overwrite handling).

### Release contents
- [ ] The portable zip contains `ffgl/<PluginName>.dll` and `ffgl/README.txt` (what it is, where it goes, the
      version, a pointer to the manual).
- [ ] After M12: the NSIS installer gets an optional component, "FFGL plugin for Resolume Arena" (checked by
      default), which installs the DLL into the Resolume plugin folder (see below), using the same logic as the
      in-app deployment.
- [ ] `SHA256SUMS.txt` (Phase I) includes the DLL.

### Plugin deployment ("Install FFGL plugin") — action item
New, small feature requested by Uli: the user can install or update the bundled FFGL plugin into Resolume
Arena's plugin folder from the app, without copying files by hand. Add it to `docs/SPEC.md` (new section)
before implementing.

- [ ] **[verify] Target folder:** confirm with Uli's Resolume 7.26 setup where Resolume looks for user FFGL
      plugins. Expected: the per-user folder `Documents\Resolume Arena\Extra Effects` and any extra folders
      listed under Preferences → Video → plugin folders. Never write into `C:\Program Files\…` (that would need
      admin rights). Record the result in `docs/resolume-api-notes.md`.
- [ ] **Detection:** resolve the user's Documents folder through the Windows Known Folder API (it may be
      redirected, e.g. to OneDrive), not a hard-coded `C:\Users\…\Documents`. If the folder doesn't exist, offer
      to create it. A setting / CLI flag `--ffgl-dir` overrides the detected folder.
- [ ] **UI:** a menu item **File → Install FFGL plugin…** (native menu after M12, HTML menu otherwise), which
      opens a dialog showing:
  - the bundled plugin name and version
  - the target folder (with "Change…" and "Open folder")
  - the installed version found there, or "not installed"
  - one primary button: **Install**, **Update** (bundled is newer), **Reinstall** (same version) or
    **Downgrade** (bundled is older, with a warning)
- [ ] **Resolume running:** Resolume locks loaded plugin DLLs. Before copying, check whether `Arena.exe` is running.
      If it is, show "Quit Resolume Arena to install the plugin", with a Retry button. Never kill the process.
- [ ] **Safe copy:** copy to a temp file in the target folder, then rename over the old DLL. Keep the previous
      version as `<PluginName>.dll.bak` until the copy succeeds. On failure, restore it and show the error.
- [ ] **After install:** "Restart Resolume Arena to load the plugin" (Resolume scans plugins at startup).
- [ ] **CLI:** `seq-app --install-ffgl [--ffgl-dir <path>]` does the same without the UI (exit code 0 = done,
      non-zero = error with a message), for scripting and for the NSIS installer to reuse.
- [ ] **Startup hint (optional, ask Uli):** if the installed plugin is missing or older than the bundled one,
      show a one-line hint in the status bar ("FFGL plugin update available"). No automatic installs.
- [ ] **Uninstall:** a "Remove" button in the same dialog (deletes only the DLL the app installed). The NSIS
      uninstaller removes the DLL only if it installed it.
- [ ] **Logging:** every install, update or removal is logged with the source version, target path and result.
- [ ] **Tests:** unit tests for version comparison and folder resolution (with an injected Documents path);
      an integration test that installs into a temp folder, updates, simulates a locked file, and restores from `.bak`.

### Manual and site
- [ ] Manual (Phase G2): a chapter **"Installing the FFGL plugin"**: what the plugin does, installing via the app
      or the installer, manual install from the zip, restarting Resolume, where to find the plugin in Resolume,
      updating, removing, and troubleshooting (plugin not showing up, "file in use").
- [ ] Landing page: list the plugin among the features.

## Phase H — GitHub repository configuration

Most of these are settings in the GitHub web UI that only Uli can click. For each **[Uli]** item,
Claude writes the exact path and value into `docs/github-setup.md` (a checklist Uli works through),
then waits for confirmation. The workflow files themselves are written by Claude in Phase I.

### H1 — Create the repository [Uli]
- [ ] Create an **empty** repo `<owner>/<repo>` (no README, LICENSE or .gitignore; they come from the
      local repo). Visibility: **private** for now, switched to public in Phase I after the first green run.
- [ ] Settings → General: default branch `main`; Features: Issues on, Discussions optional,
      Wiki off, Projects off; Pull Requests: allow **squash merging** only, "Automatically delete head branches" on.
- [ ] Claude adds the remote and pushes `main`. No tags yet.

### H2 — Actions permissions [Uli]
- [ ] Settings → Actions → General → **Actions permissions:** "Allow \<owner\>, and select non-\<owner\>,
      actions and reusable workflows", with "Allow actions created by GitHub" on, and the allow-list:
      `dtolnay/rust-toolchain@*`, `Swatinem/rust-cache@*`, `EmbarkStudios/cargo-deny-action@*`.
      (Release creation uses the preinstalled `gh` CLI, so no third-party release action is needed.)
- [ ] Settings → Actions → General → **Workflow permissions:** "Read repository contents and packages
      permissions" (read-only default). Leave "Allow GitHub Actions to create and approve pull requests" **off**.
      The release workflow requests `contents: write` itself, at job level.
- [ ] Settings → Actions → General → **Fork pull request workflows:** "Require approval for first-time contributors".
- [ ] Settings → Actions → General → **Artifact and log retention:** 30 days.

### H3 — Branch and tag protection (rulesets) [Uli]
- [ ] Settings → Rules → Rulesets → New branch ruleset **`protect-main`**, target `main`, enforcement *Active*:
      restrict deletions; block force pushes; require status checks to pass (add the CI job names
      from Phase I: `ci / rust`, `ci / web`, `ci / deny`, `ci / site`, `ci / ffgl`), with "require branches to be up to date" on.
      Bypass list: repository admin (so Uli can still push directly while working solo). Optional:
      require a pull request before merging.
- [ ] New tag ruleset **`protect-release-tags`**, target pattern `v*`, enforcement *Active*:
      restrict creations, updates and deletions; bypass list: repository admin only. This means only Uli
      can create release tags, and a published tag can't be moved.
- [ ] Settings → General → **Releases:** enable "Immutable releases" if the option is available
      (published releases and their assets can then no longer be changed).

### H4 — Release environment [Uli]
- [ ] Settings → Environments → New environment **`release`**:
  - Deployment branches and tags: "Selected branches and tags", rule: tag pattern `v*`
  - Required reviewers: Uli (if "Release approval" = yes in Phase A); "Prevent self-review" **off**
    (otherwise a solo maintainer can't approve)
  - Environment secrets: **none for now**. Placeholders, documented in `docs/github-setup.md` but not
    created: `WINDOWS_CERTIFICATE` / `WINDOWS_CERTIFICATE_PASSWORD` (future code signing),
    `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (only if an updater is added later).
- [ ] No repository secrets are needed: the workflows use the automatic `GITHUB_TOKEN`.

### H5 — Security and dependency updates [Uli + Claude]
- [ ] **[Uli]** Settings → Advanced Security (or Security): enable **Private vulnerability reporting**,
      **Dependabot alerts**, **Dependabot security updates**, and **Secret scanning** with **push protection**.
- [ ] **Claude** adds `.github/dependabot.yml`: ecosystems `cargo` (`/`), `npm` (`/web`),
      `github-actions` (`/`); weekly schedule; grouped minor/patch updates per ecosystem; max 5 open PRs.
- [ ] **Claude** adds `.github/CODEOWNERS` with `* @<owner>`.

### H6 — GitHub Pages [Uli]
- [ ] Settings → Pages → **Build and deployment → Source: GitHub Actions** (not "Deploy from a branch").
- [ ] Note: Pages on a **private** repo needs a paid GitHub plan. On a free account, Pages only works after the
      repo is switched to public (I4). The first site deployment therefore happens after that switch.
- [ ] Settings → Environments → **`github-pages`** (created automatically on first deploy): restrict deployments to
      tags `v*` and the `main` branch (for manual redeploys).
- [ ] If a custom domain is chosen (Phase A):
  - set the DNS record at the domain provider: a `CNAME` from the subdomain to `<owner>.github.io`
  - Settings → Pages → Custom domain: enter the domain, wait for the DNS check, then enable **Enforce HTTPS**
  - verify the domain for the account (Settings of the user/org → Pages → Verified domains), to prevent takeover
  - no `CNAME` file is needed in the repo when deploying via Actions
- [ ] The allow-list from H2 needs no change: the Pages actions (`actions/configure-pages`,
      `actions/upload-pages-artifact`, `actions/deploy-pages`) are created by GitHub. mdBook is installed from its
      official release binary in the workflow, not through a third-party action.
- [ ] Settings → General → repo **Website** field (About box on the repo page): the site URL.

Done when: `docs/github-setup.md` is fully ticked by Uli, and `main` is pushed.

## Phase I — CI/CD workflows and first release

All workflows follow these rules:
- Pin third-party actions to a **full commit SHA**, with the version as a comment
  (`uses: Swatinem/rust-cache@<sha> # v2.x`). Dependabot keeps them updated.
- Set `permissions:` explicitly in every workflow (default `contents: read`).
- `concurrency:` per workflow and ref, with `cancel-in-progress: true` for CI only (never for release).
- Runner: `windows-latest`. Rust via `dtolnay/rust-toolchain` (stable, with `clippy` and `rustfmt`);
  cache via `Swatinem/rust-cache`; Node via `actions/setup-node` (LTS, `cache: npm`,
  `cache-dependency-path: web/package-lock.json`).
- `actions/checkout` with `fetch-depth: 0` wherever the build number is computed (Phase E).

### I1 — `ci.yml`
- [ ] Triggers: `push` to `main`, `pull_request`, `workflow_dispatch`.
- [ ] Jobs (names must match the required status checks from H3):
  - `rust`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo test --workspace`
  - `web`: `npm ci`, `npm run build` (plus lint/tests if present), in `web/`
  - `deny`: `cargo-deny-action` with `check licenses bans advisories sources`
  - `ffgl`: configure and build the FFGL plugin (CMake + MSVC, x64, Release), then check its dependencies
    with `dumpbin /dependents` (no MSVC runtime DLLs). Initialise submodules if the SDK is vendored that way.
  - `site`: install mdBook, run `site/build.ps1`, and fail on broken internal links (mdBook's build
    warnings treated as errors, plus a simple link check over `site/dist/`). Nothing is deployed from CI.
- [ ] The `rust` job builds the web UI first if the embedded assets are required to compile.

### I2 — `release.yml`
- [ ] Triggers: `push` of tags `v*`, and `workflow_dispatch` with input `dry_run` (default `true`):
      a dry run builds and uploads everything as a **workflow artifact**, but creates no release.
- [ ] Job `build` (`permissions: contents: read`):
  1. checkout with `fetch-depth: 0`
  2. **verify version:** the tag (minus `v`) must equal `[workspace.package].version` and the
     `web/package.json` version; otherwise fail with a clear message. A dry run checks only Cargo vs. npm.
  3. **verify changelog:** `CHANGELOG.md` must contain a `## [<version>]` section; extract it to
     `release-notes.md`
  4. build the web UI, then the release binary (`cargo build --release -p seq-app`; after M12,
     `cargo tauri build` instead), and the FFGL plugin (Release, x64). The plugin DLL must be built **before**
     the app/installer, because both bundle it. Check that the DLL's embedded version matches the release version.
  5. generate the third-party notices (cargo-about, plus the web notices from the UI build)
  6. package `<name>-<version>-windows-x64.zip` with `seq-app.exe`, `ffgl/<PluginName>.dll`,
     `ffgl/README.txt`, `LICENSE`, `THIRD_PARTY_LICENSES.*` (including the FFGL SDK notice), `OFL.txt`,
     `README.md`, `CHANGELOG.md` (after M12: also the NSIS installer, with the plugin component, as a separate
     asset). Upload the plugin `.pdb` as a separate workflow artifact only (not attached to the release).
  7. compute SHA-256 for each asset in PowerShell (`Get-FileHash -Algorithm SHA256`) and write a
     combined `SHA256SUMS.txt`
  8. upload everything, plus `release-notes.md`, with `actions/upload-artifact` (retention 30 days)
- [ ] Job `publish` (`needs: build`; skipped for dry runs; `environment: release`;
      `permissions: contents: write`):
  1. download the artifact
  2. create a **draft** release with the preinstalled `gh` CLI:
     `gh release create "$TAG" --draft --verify-tag --title "<name> $VERSION" --notes-file release-notes.md`
  3. attach assets according to the Phase A "Binary distribution" decision:
     (a) or (c): the zip, installer and `SHA256SUMS.txt`;
     (b): no binaries (GitHub's automatic source archives only), and the release notes link to the shop
  4. Uli reviews the draft on GitHub and clicks **Publish** (the final manual step)
- [ ] Optional, ask Uli: **build provenance** with `actions/attest-build-provenance` in the `build` job
      (needs `id-token: write` and `attestations: write`), so users can verify a binary with `gh attestation verify`.

### I3 — Release procedure (`docs/RELEASING.md`)
- [ ] Claude writes `docs/RELEASING.md`, the steps Uli follows for every release:
  1. on an up-to-date `main` with green CI: set the new version in `Cargo.toml` (`[workspace.package]`)
     and `web/package.json`, and run the version-sync check
  2. move the `## [Unreleased]` entries in `CHANGELOG.md` to `## [x.y.z] - YYYY-MM-DD`
  3. commit `Release x.y.z` and push; wait for green CI
  4. optional: run `release.yml` manually with `dry_run: true` and check the artifact
  5. `git tag -a vx.y.z -m "x.y.z"` and `git push origin vx.y.z`
  6. approve the `release` environment in the Actions run (if enabled)
  7. review the draft release (notes, assets, checksums), then **Publish**. Publishing triggers the site and
     manual deployment (I5); check the live site afterwards.
  8. if something is wrong *before* publishing: delete the draft and the tag (admin bypass), fix it, and
     re-tag. *After* publishing: never move a tag. Release `x.y.(z+1)` instead.
  9. a manual-only fix (typo, wrong screenshot) between releases: merge it to `main`, then run `pages.yml`
     manually (I5). The version footer stays at the latest release.

### I5 — `pages.yml` (project site and manual)
- [ ] Triggers: `release` with type `published` (so the site updates exactly when a release goes public), and
      `workflow_dispatch` (manual redeploy, e.g. for manual fixes).
- [ ] Permissions: `contents: read`, `pages: write`, `id-token: write`. `concurrency: group: pages`,
      `cancel-in-progress: false`.
- [ ] Job `build` (`ubuntu-latest` is fine here):
  1. checkout: the release tag for `release` events; for `workflow_dispatch`, the latest release tag by default,
     with an optional input to build from `main` instead
  2. install mdBook from its official GitHub release binary (version pinned in the workflow), and verify the
     download's checksum
  3. run the site build (a POSIX equivalent of `site/build.ps1`, or `pwsh`, which is preinstalled on the runner);
     inject the version for the "Manual for version x.y.z" footer
  4. `actions/configure-pages`, then `actions/upload-pages-artifact` with `site/dist/`
- [ ] Job `deploy` (`needs: build`, `environment: github-pages` with `url: ${{ steps.deployment.outputs.page_url }}`):
      `actions/deploy-pages`.
- [ ] After the first deployment: open the site, click through the landing page, the manual navigation, search, the
      download button and the contact page; check it at phone width.

### I4 — First release
- [ ] Push all workflow files; the CI run on `main` is green; add the five job names as required checks (H3).
- [ ] Run `release.yml` with `dry_run: true`; download the artifact, unzip it, and check the contents and checksums.
- [ ] **[Uli]** switch the repo visibility to **public** (Settings → General → Danger Zone). Re-check that the
      Phase C scrub is complete *before* this step; it's the point of no return.
- [ ] **[Uli]** enable GitHub Pages (H6) now that the repo is public.
- [ ] Tag `v0.1.0` per `docs/RELEASING.md`; approve; review the draft; publish. Then confirm that `pages.yml` ran,
      and that the site and manual are live with "Manual for version 0.1.0".
- [ ] Download the published zip on a clean machine (or in a Windows Sandbox), verify the checksum, and run it
      once against Resolume.
- [ ] On that machine: install the FFGL plugin through File → Install FFGL plugin… (with Resolume closed), start
      Resolume, and confirm the plugin appears and works. Repeat once with Resolume running, to see the
      "Quit Resolume" message.

---

## Definition of done

- The repo is public under MIT, with no personal or third-party content in its tree or history.
- `LICENSE`, `THIRD_PARTY_LICENSES`, `OFL.txt` are in the repo **and** in the release zip.
- `--version`, `/api/version`, the log header, the About dialog and the exe properties all show the same
  version, build number and commit.
- The menu bar, About, License and Shortcuts dialogs work with mouse and keyboard.
- The UI works fully offline (fonts bundled).
- The UI server binds to localhost by default.
- CI is green and required on `main`; release tags `v*` are protected; releases go through a draft (and the
  `release` environment approval, if chosen).
- `docs/github-setup.md` and `docs/RELEASING.md` exist, and `v0.1.0` was released by following them.
- `v0.1.0` exists as a GitHub Release with assets and `SHA256SUMS.txt` according to the distribution decision.
- The project site and the end-user manual for `0.1.0` are live on GitHub Pages, make no external requests, and
  are linked from the README, the repo's Website field, the About dialog and Help → User manual.
- Publishing a release redeploys the site and manual automatically; manual fixes can be redeployed by hand.
- The FFGL plugin is built in CI, ships in the release (zip and, after M12, the installer component), has its SDK
  license in the notices, and can be installed, updated and removed from the app (UI and `--install-ffgl`) without
  admin rights.

## Decisions

_(filled in during Phase A)_

- Copyright holder:
- Project name:
- GitHub owner/repo:
- Starting version:
- Binary distribution: (a) / (b) / (c)
- Release approval required: yes / no
- Manual language:
- Site address (GitHub Pages URL / custom domain):
- Public contact address:
- FFGL plugin (name, purpose, current source location):
- FFGL plugin location: (a) monorepo `ffgl/` / (b) separate repo, pinned version
- Resolume user plugin folder (verified):
