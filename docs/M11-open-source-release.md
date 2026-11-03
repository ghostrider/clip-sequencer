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

- [x] **Copyright holder** for the LICENSE line: real name (`Ulrich Staudinger`) or artist
      name (`Sun92`). Default: `Copyright (c) 2026 Ulrich Staudinger`.
- [x] **Project name.** Must not be "Resolume …". Pattern "<Name> for Resolume". Check
      that the name is free on GitHub (and crates.io, if publishing there later).
- [x] **GitHub owner/repo** (user account or org), e.g. `github.com/<owner>/<repo>`.
- [x] **Starting version:** default `0.1.0`.
- [x] **Binary distribution:** (a) free on GitHub Releases, (b) GitHub Releases are source-only and
      prebuilt binaries are sold through a shop, or (c) both. This decides what the release workflow
      attaches (Phase I). Default: (a).
- [x] **Release approval:** should a published release need a manual click from Uli (draft release plus
      a protected `release` environment)? Default: yes.

- [x] **Manual language:** English only, or English + German. Default: English only (German can follow later).
- [x] **Site address:** the default GitHub Pages URL `https://<owner>.github.io/<repo>/`, or a custom
      domain (e.g. a subdomain of a domain Uli already owns). Default: the GitHub Pages URL.
- [ ] **Public contact address** for the site and the manual (a project e-mail address, not a personal
      inbox). It's needed for the contact / legal notice page (Phase G2).
- [x] **FFGL plugin:** its name, what it does, and where its source currently lives (it was built in a
      separate Claude Code session). Claude reads the plugin's source and README to fill in the details,
      and asks Uli only what it can't find there.
- [x] **FFGL plugin location:** (a) move it into this repo under `ffgl/`, with its history (monorepo, one
      release, one version), or (b) keep it in its own repo, and the release workflow fetches a pinned
      version. Default: (a).

Done when: all eleven are answered and recorded at the bottom of this file under "Decisions".
*(2026-11-01: ten answered; the public contact address is still open and is needed before Phase G2.)*

## Phase B — Extract into its own repository

The sequencer currently lives in `vj_stuff/resolume-sequencer/` inside the `vj_stuff` repo.

*(2026-11-01: extracted into `ghostrider/clip-sequencer` (local clone, not pushed). One
`git filter-repo --path resolume-sequencer/ --path ffgl-plugins/seq_view/ --path-rename resolume-sequencer/:
--path-rename ffgl-plugins/seq_view/:ffgl/` on a fresh single-branch clone of `claude/resolume-sequencer-m3-m10`
gave 45 commits, including the plugin's history from before its move. The FFGL SDK is now a submodule
`external/ffgl` pinned to `5b6e078` (the commit `vj_stuff` uses), and `stb_image.h` / `stb_truetype.h` are vendored
in `third_party/stb/`. The plugin rules from `vj_stuff/ffgl-plugins/AGENTS.md` are in `ffgl/AGENTS.md`.)*

- [x] Extract the subfolder **with its history** into a new local repo, using either
      `git subtree split --prefix=resolume-sequencer -b sequencer-export` or
      `git filter-repo --subdirectory-filter resolume-sequencer` on a fresh clone.
      Never run filter-repo on the original `vj_stuff` checkout.
- [ ] Verify that the new repo builds and all tests pass on its own (no paths into `../`).
      *(2026-11-01, Linux container: `npm ci`, `npm run build`, `npm run check` clean; `cargo fmt --check` and
      `cargo clippy --workspace --all-targets --no-default-features -D warnings` clean; `cargo test --workspace
      --no-default-features`: 77 passed, 1 failed (`preferences_switch_and_save_the_midi_clock_port`: the container
      has no ALSA sequencer, so `/api/midi/ports` returns 500; not an extraction issue). Every SDK/stb path in both
      `.vcxproj` files resolves. Three PNGs were Git LFS pointers (vj_stuff's `.gitattributes`); history was
      rewritten to hold the real files. Open: on Windows, `scripts\check.ps1` (with the `desktop` feature),
      `ffgl\build_seqview.ps1` and `ffgl\tests\SeqViewTests.exe` after `git submodule update --init`.)*
- [x] `CLAUDE.md` in the new repo must not reference the parent `vj_stuff/CLAUDE.md`,
      personal paths, or other projects. Move anything general that it needs from the parent file
      into it.
- [x] If Phase A chose (a) for the FFGL plugin: import it into `ffgl/` with its history, the same way
      (`git subtree split` / `git filter-repo --subdirectory-filter <plugin folder>` on a fresh clone of
      its repo, then `git subtree add --prefix=ffgl` or a merge with `--allow-unrelated-histories`).
      Uli's own plugin install script stays where it is; it isn't part of the release (see Phase G3).
- [x] Do **not** push yet (pushing happens in Phase H1).

## Phase C — Scrub content and history

Scan the **working tree and the full git history** of the new repo.

*(2026-11-01: done before the first push, reviewed by Uli. All 46 commits were scanned (every blob, every commit
message, author data). Found and rewritten with `git filter-repo` in all history:*
- *14 licensed Artlist stock clips (IDs, titles, creator names, also truncated forms in debug output) in the samples,
  the `full.seq.json` fixture, tests, `examples/soak.rs` and the API notes → `Clip 01` … `Clip 14` (one-to-one, so
  duplicates and lookups still hold); the library paths `Q:\video library\…` → `D:\media`.*
- *`docs/samples/thumbnail-clip.png`, a frame of one of those clips → a generated 320x240 gradient.*
- *a Windows user name in a temp path in `ws-05` → `C:/Users/user/…/probe/`.*
- *the venue composition name → `show-b` (docs and one commit message).*
- *the employment-check wording in this plan (Phase A and Decisions).*
- *commit authors → `Ulrich Staudinger <275349+ghostrider@users.noreply.github.com>`; `Claude-Session` trailers
  removed from commit messages.*
- *Kept: Resolume's own effect, source and preset names in the samples (product content); generic paths like
  `Q:\media\something else.mov` in unit tests.*
- *No media, `.avc`, real show files, thumbnail cache, DLL/PDB, secrets, tokens or private IPs were ever committed;
  the three PNGs that were LFS pointers are now normal files. No SDK files beyond the pinned submodule and stb.)*

- [x] `docs/samples/` (M0 probe output): replace real clip names, media file paths, user
      names and machine names with neutral sample data. Keep the JSON structure identical
      so tests still pass.
- [x] No real show files (`*.seq.json`), thumbnail cache, `.avc` compositions, media clips
      or screenshots containing third-party footage.
- [x] The FFGL plugin source and history are included in the scan: no built DLLs, PDBs, local paths in
      CMake caches or build scripts, and no copies of SDK files whose license doesn't allow redistribution.
- [x] No secrets, tokens, private IPs or hostnames in configs, logs or tests.
- [x] Search history with `git log -p` / `git grep $(git rev-list --all)` for: `C:\\Users`,
      `/Users/`, `.avc`, `.mov`, `.mp4`, `token`, `password`, `192.168.`. Report findings to Uli
      before rewriting any history.
- [x] If history has to be rewritten, do it with `git filter-repo` before the first push.
- [x] `.gitignore` covers `target/`, `web/node_modules/`, `web/dist/`, the thumbnail cache,
      local `seq.toml`, `*.seq.json` outside fixtures, and logs.

## Phase D — License files and third-party notices

- [x] `LICENSE` in the repo root: standard MIT text, with the copyright line from Phase A.
- [x] `Cargo.toml`: `license = "MIT"`, `repository`, `description`, `authors`,
      `readme`, `keywords`, `categories` in `[workspace.package]`. All crates inherit them.
      Internal crates and tools get `publish = false`.
- [x] **Rust third-party notices:** add `cargo-about` with an `about.toml` and template that
      generates `THIRD_PARTY_LICENSES.md` (or `.html`) from the dependency tree.
- [x] **Web UI third-party notices:** add `rollup-plugin-license` (Vite) so the build emits
      the npm dependency licenses. Merge them into the same third-party file, or a second section of it.
- [x] **License policy in CI:** add `cargo-deny` with `deny.toml`. Allow MIT, Apache-2.0,
      BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0 (file-level). Deny GPL/AGPL/LGPL and unknown
      licenses. Also enable the advisories check. Report any dependency that fails.
- [x] **Fonts:** stop loading IBM Plex from Google Fonts. Bundle the woff2 files in
      `web/src/assets/fonts/` (the UI must work offline at venues). Add `OFL.txt` (SIL Open
      Font License 1.1) next to them, and list IBM Plex in the third-party notices.
- [x] **FFGL SDK and plugin dependencies:** record the license of the FFGL SDK (Resolume's `ffgl`
      repository) **[verify the exact license text from the SDK repo]** and of any other library the plugin
      links or vendors (e.g. GLEW, GLM). Add them to the third-party notices, and include their license texts in
      the release. If the SDK is vendored as a git submodule, pin it to a commit.
- [ ] Optional: SPDX header `// SPDX-License-Identifier: MIT` at the top of source files.
      Ask Uli first.

*(2026-11-01, done:*
- *`LICENSE` (MIT, Ulrich Staudinger); `[workspace.package]` has authors, repository, homepage, description, readme,
  keywords, categories and `publish = false` (nothing goes to crates.io), inherited by all crates; `web/package.json`
  is `clip-sequencer-ui`, MIT.*
- *Notices: `scripts/third-party-licenses.mjs` writes `THIRD_PARTY_LICENSES.md` (zip) and `.json` (Phase F dialog)
  from `cargo about generate --format json` (`about.toml`: Windows x64 target, no dev/build dependencies, own crates
  skipped) plus `web/dist/third-party-licenses.json` (`rollup-plugin-license` in `vite.config.ts`; it works with
  Vite 8 / Rolldown for JS modules: svelte, clsx) plus fixed entries for IBM Plex, FFGL SDK, GLEW, stb. One file
  instead of merging: 326 components, 155 distinct texts. It also notes the WebView2 Runtime (used from Windows, not
  shipped: portable zip, no bootstrapper). `release.ps1` generates it; the generated files are git-ignored.*
- *`deny.toml`: the allow-list above, Windows x64 target, all features; `cargo deny check licenses bans advisories
  sources` passes (only duplicate-version warnings, normal for Tauri). No dependency fails. `check.ps1` runs it if
  installed; CI (Phase I) will run `cargo-deny-action`.*
- *Fonts: the UI already bundled IBM Plex through `@fontsource/ibm-plex-sans`/`-mono` (woff2 in `web/dist`, no Google
  Fonts request), which replaces copying woff2 files into `web/src/assets/fonts/`. The splash page gets latin-subset
  copies in `crates/seq-app/shell/fonts/` with `OFL.txt`; `OFL.txt` is also in the repo root. Only
  `docs/ui/editor-mockup.html` (a design reference, not shipped or published) still links Google Fonts.*
- *FFGL: SDK is BSD-3-Clause (Copyright 2023 FreeFrame, `external/ffgl/LICENSE.md`, verified), pinned submodule
  `5b6e078`; the plugin also links GLEW 2.1.0 statically (`glew32s.lib` from the SDK: modified BSD + MIT) and uses
  stb_image 2.30 / stb_truetype 1.26 (MIT or public domain). WinHTTP and OpenGL32 are Windows system DLLs.)*


## Phase E — Versioning and build info

- [x] **Single version source:** `version` lives only in `[workspace.package]`. All crates use
      `version.workspace = true`. `web/package.json` gets the same version, synced by a small
      script or checked in CI.
- [x] **Build info** embedded at compile time via `build.rs` (or `vergen`):
  - `version` (semver from Cargo)
  - `build_number`: the git commit count (`git rev-list --count HEAD`). It's identical in local
    builds, CI and the release workflow, and reproducible from any checkout (CI must check out with
    `fetch-depth: 0`). Falls back to `0` if git isn't available. Don't use `GITHUB_RUN_NUMBER`: each
    workflow has its own counter, so CI and release builds of the same commit would get different numbers.
  - `git_commit`: short hash, with `-dirty` if the tree has changes
  - `build_date`: UTC, ISO 8601
  - `target`: e.g. `x86_64-pc-windows-msvc`
- [x] Expose the build info:
  - [x] `seq-app --version` prints `<name> 0.1.0 (build 42, abc1234, 2026-10-27)`
  - [x] the first log line at startup
  - [x] `GET /api/version` returns JSON with all fields
  - [ ] the About screen (Phase F)
- [x] **Windows exe metadata:** embed version info (FileVersion, ProductVersion,
      CompanyName = copyright holder, LegalCopyright, ProductName) and an app icon via
      `embed-resource` or `winres`. **[Uli]** provides the icon, or Claude makes a simple
      placeholder `.ico`.
- [x] **FFGL plugin version:** the plugin gets the same version as the app (monorepo) or its own pinned
      version (separate repo). It's embedded in the DLL's Windows version resource (FileVersion, ProductVersion,
      LegalCopyright), and, where the FFGL API allows it, in the plugin's info/about fields shown in Resolume.
      The app must be able to read the version of an installed DLL (needed for "Plugin deployment", Phase G3).
- [x] Release tags follow `vMAJOR.MINOR.PATCH`. CI fails if the tag doesn't match the Cargo version.

*(2026-11-03, done; the About screen follows in Phase F:*
- *Version only in `[workspace.package]`; `scripts/check-version.mjs` checks the crates (`version.workspace`),
  `web/package.json`, `package-lock.json` and that `tauri.conf.json` sets none; `--tag [vX.Y.Z]` (or
  `GITHUB_REF_NAME`) also checks the release tag. `check.ps1` runs it; `release.yml` will run it with `--tag` (I2).*
- *`build.rs` (no `vergen`): build number = `git rev-list --count HEAD` (0 without git), commit (+`-dirty`),
  UTC build date (ISO 8601, `SOURCE_DATE_EPOCH` honoured), target, and `SEQ_LONG_VERSION`. `--version` prints
  `seq-app-cli 0.1.0 (build 52, 931e56d, 2026-11-03)` (clap uses the binary's name); the first log line adds target
  and build time; `/api/version` has `build`, `commit`, `build_date`, `target`; the engine test checks them.*
- *Exe metadata: Tauri's build step writes ProductName, CompanyName (`publisher`), LegalCopyright, FileVersion /
  ProductVersion (Cargo version) and the placeholder `§` icon; no `embed-resource`/`winres`. The build number is
  not put into the fourth FileVersion field (that would need a `TAURI_CONFIG` override; not required).
  **To verify on Windows:** the properties of `seq-app.exe` and `seq-app-cli.exe`.*
- *FFGL: `SeqView.vcxproj` reads the version from `Cargo.toml` and build number / commit / date from git (the
  per-machine `build_number.txt` is gone); `SeqView.rc` gives the DLL a VERSIONINFO (FileVersion
  `0.1.0.<build>`), which G3 reads; About text and the Build parameter show the long version. The FFGL plugin
  major/minor integers stay 1.0 (changing them might affect how Resolume matches the plugin; untested).
  **To verify on Windows:** `ffgl\build_seqview.ps1`, DLL properties, the About/Build text in Resolume.)*


## Phase F — UI: menu bar, About, License, shortcuts

Use only the existing design tokens (`web/src/tokens.css`). Every item opens an existing
function or a static info dialog. No new behaviour.

- [x] **Menu bar** at the top of the app, above or merged into the top bar, keyboard-accessible
      (Alt or F10 focuses it; arrow keys move; Esc closes):
  - **File:** Reload composition · Save now · Open data folder (only if this is trivial
    via the server; otherwise leave it out) · Quit server (with confirmation)
  - **View:** Zoom in · Zoom out · Reset zoom
  - **Help:** Keyboard shortcuts · User manual (opens the manual on the project site, Phase G2,
    in the system browser) · Report an issue (GitHub issues/new) · Licenses · About
- [x] **About dialog:** app name and icon; version, build number, commit, build date (from
      `/api/version`); `Copyright (c) 2026 <holder>`; "Released under the MIT License"
      linking to the License dialog; GitHub repository link; the disclaimer *"Not affiliated with or
      endorsed by Resolume BV. Resolume is a trademark of Resolume BV."*; *"Provided as is,
      without warranty of any kind."*; links to the project site and the user manual.
- [x] **License dialog:** the project's MIT text first, then the third-party notices,
      grouped per package (name, version, license, collapsible full text). Data is served by the app
      from the generated file embedded at build time.
- [x] **Keyboard shortcuts dialog:** a list of the shortcuts that already exist (SPEC §9.3).
- [x] Each dialog closes with Esc and the close button, traps focus, and returns focus to its menu item.

*(2026-11-03, done:*
- *Menu bar: per the M12 decision the desktop app keeps its native menu (Help now also has User manual, Report an
  issue, Licenses, About); `web/src/components/MenuBar.svelte` is shown only in browser/headless mode. File:
  Reload composition, Save now, Preferences…, Quit server… (confirmation; the headless app stops like Ctrl+C).
  "Open data folder" is left out in the browser: the server would have to open Explorer for a browser that may not
  sit at that machine. View: page zoom (CSS zoom, remembered per browser), Toggle Console. Help as in the plan.
  Alt alone or F10 enters the bar; arrows, Home/End, Enter/Space, Esc, Tab follow the ARIA menubar pattern.*
- *About / Licenses / Keyboard shortcuts are page dialogs in both modes (the native menu opens them through
  `window.__seqOpenDialog`). Licenses: `GET /api/licenses` = the MIT text + `THIRD_PARTY_LICENSES.json` embedded
  at build time (placeholder note in dev builds; `release.ps1` now generates the notices before `cargo build`).
  Shortcuts: one list, `web/src/lib/shortcuts.ts` (the copy in `desktop.rs` is gone).*
- *Shared `Dialog.svelte`: Esc and ✕ close, Tab is trapped, focus returns to the opener (the menu-bar item).
  Editor keys (Space, Del, R, …) no longer act inside dialogs or on the menu bar.*
- *Verified in Chromium against `mock-resolume` + headless `seq-app-cli` (14 checks: keyboard model, dialog
  contents, focus trap/return, filter and expanding in Licenses, zoom persistence, Quit server Cancel/Quit).
  **To verify on Windows:** the native Help items in `seq-app.exe` (dialogs open in the window, links in the
  system browser).)*


## Phase G — Repository documents and security defaults

- [x] `README.md` (aimed at developers and first-time visitors on GitHub; end users are pointed to the
      project site and manual from Phase G2, which the README links prominently at the top):
  - [x] what it does (2–3 sentences) and a screenshot (with neutral demo content, no third-party footage)
  - [x] requirements: Windows 10/11, Resolume Arena 7.26+ with the webserver and OSC input enabled,
        a MIDI clock source (e.g. a virtual MIDI port)
  - [x] install (download the release zip), run, CLI flags / `seq.toml`, the SEQ-layer convention,
        where data, cache and logs live
  - [x] SmartScreen note: the exe is unsigned, so Windows may warn ("More info → Run anyway")
  - [x] building from source
  - [x] the Resolume trademark disclaimer and a license section
- [x] `CHANGELOG.md` in the Keep a Changelog format; first entry `0.1.0` summarises the current feature set.
- [x] `CONTRIBUTING.md`: build/test commands, code style (fmt, clippy), PR expectations, and
      that the sequence file format changes only with a version bump and a migration.
- [x] `SECURITY.md`: how to report issues privately (GitHub private vulnerability reporting).
- [x] **Security default:** the UI server binds to `127.0.0.1` by default. Binding to other
      interfaces requires an explicit flag (`--ui-bind 0.0.0.0`) and logs a warning. If the app
      currently binds to all interfaces, this change is part of M11. Document it in the README.
- [x] `.github/ISSUE_TEMPLATE/bug_report.yml` (asks for app version/build, Resolume version,
      Windows version, MIDI clock source, a log excerpt) and `feature_request.yml`.
- [x] `.github/pull_request_template.md` (short checklist: tests, clippy, changelog entry, manual updated if user-facing behaviour changed).
- [ ] Optional: `.editorconfig`; `CODE_OF_CONDUCT.md` (Contributor Covenant). Ask Uli.

*(2026-11-03, done; the optional `.editorconfig` / `CODE_OF_CONDUCT.md` are waiting for Uli:*
- *README: site / manual / download / changelog links on top; screenshot `docs/images/screenshot.png` of the
  neutral demo (`docs/samples/demo-composition.json` + `demo.seq.json` in `mock-resolume`, generated abstract
  thumbnails); features; requirements with any MIDI clock source (no reference to the separate Clock Shifter
  project); install from the zip with checksum and SmartScreen notes; Resolume setup incl. the SEQ layer; run,
  options, UI port, logs, data folder (kept from before); security; building from source; license and
  disclaimer. `seq.toml` is not implemented (`config.rs`), so the README documents CLI flags only.*
- *Security default: the UI server binds to `127.0.0.1` and there is no option to bind elsewhere (stricter than
  the plan's opt-in flag; no `--ui-bind` was added, since that would be a new feature nobody needs yet).
  Documented in README and SECURITY.md.*
- *CHANGELOG (0.1.0 "Unreleased" until the release sets the date; `[Unreleased]` above it), CONTRIBUTING,
  SECURITY, issue forms (bug report with version/build, program, Resolume, Windows, clock source, log;
  feature request; `config.yml` links to the manual and private reporting) and the PR template.)*


## Phase G2 — End-user manual and project site (GitHub Pages)

The manual is for VJs who use the sequencer, not for developers. It assumes they know Resolume,
but not this tool. It's published, together with a small project site, on GitHub Pages
(configuration in H6, deployment in I5).

### Structure and tooling
- [x] Sources live in `site/`, separate from the developer docs in `docs/`:
  ```
  site/
    landing/        hand-written static landing page (index.html, CSS, images)
    manual/         mdBook project (book.toml, src/SUMMARY.md, src/*.md, src/images/)
    shared/         tokens.css and the self-hosted fonts, copied from web/ at build time
    build.ps1       assembles landing + manual into site/dist/ (manual under /manual/)
  ```
- [x] Tool: **mdBook** for the manual (Rust ecosystem, search built in, works offline as static
      files), plus a hand-written landing page. The landing page uses the app's design tokens, so
      the site and the app look like one product.
- [x] Base path: the site is served under `/<repo>/` on the default GitHub Pages URL. Set
      `site-url` in `book.toml` accordingly, and use only relative links in the landing page. If a custom
      domain is chosen in Phase A, the base path is `/`.
- [x] **No external requests:** no Google Fonts, no CDN, no analytics, no cookies, no embedded videos from
      third-party hosts. Fonts are self-hosted. (That keeps the site free of tracking and cookie-banner
      obligations under Swiss and EU data-protection law.)

### Manual content (`site/manual/src/`)
Write it in the Phase A language. Every page is task-oriented: what you want to do, then the steps.
- [x] **Introduction:** what the sequencer does, how it fits next to Resolume and the MIDI clock, requirements.
- [x] **Installation:** installer vs. portable zip, WebView2 (after M12), the SmartScreen warning, checksum
      verification, updating to a new version, uninstalling (and where the data stays).
- [ ] **Setting up Resolume:** enable the webserver and OSC input (with the exact preference paths), MIDI clock
      sync, Beat Snap **off** for clip triggers, recommended clip transport mode (BPM sync), creating the `SEQ`
      layer with `§ <name>` placeholder clips and an invisible source.
- [x] **Setting up the clock:** any MIDI clock source works; selecting the input port; using a virtual MIDI
      port (e.g. loopMIDI); a note on the separate Clock Shifter for projector latency.
- [x] **Quick start:** a step-by-step tutorial, from an empty sequence to a 2-lane, 4-bar sequence started from
      Resolume, with a screenshot per step.
- [x] **The interface:** top bar (deck tabs and LIVE, connection chips, Stop all), sequence list, editor (lanes,
      steps, rests, the 1/8-bar grid, Repeat, lane end behaviour), clip browser, step inspector, status bar,
      Console tab (after M12), menus.
- [x] **Working with decks:** sequences belong to one deck; viewing vs. starting; the deck switch on start and its
      confirmation.
- [x] **Playing live:** starting and stopping (UI and SEQ clips), edits apply at the next start, manual override and
      "Resume lane", Stop all, what happens on clock loss.
- [x] **Files and data:** the `.seq.json` file next to the composition, backups, moving a show to another machine,
      orphaned sequences after a deck rename, how clips are re-found after renames (ID → name → file) and what
      red steps mean.
- [x] **Thumbnails:** where they come from, cached thumbnails for inactive decks, "Push thumbnail to Resolume".
- [x] **Settings reference:** all CLI flags and `seq.toml` keys with defaults, UI port fallback and `--strict-port`,
      `--headless`, bind address and its security note.
- [x] **Keyboard shortcuts:** generated from, or checked against, the in-app shortcuts dialog (Phase F), so they never diverge.
- [x] **Troubleshooting:** no clock / clock not locked; Resolume not reachable; triggers "not confirmed"; red
      (unresolved) steps; port in use / app already running; SmartScreen; where the logs are and what to attach to a
      bug report.
- [x] **FAQ** and **Glossary** (sequence, lane, step, pass, deck, LIVE, yield, SEQ clip).
- [x] **About:** version history (links to the changelog), license and third-party notices, the Resolume trademark
      disclaimer, how to report bugs.

### Screenshots
- [x] Screenshots show **neutral demo content only** (generated colour clips, no third-party footage, no real show
      names). Use `tools/mock-resolume` with a demo composition.
- [ ] Optional, ask Uli: a script (Playwright) that starts the app against the mock and regenerates all manual
      screenshots, so they stay current after UI changes.

### Landing page (`site/landing/`)
- [x] One page: name and one-line pitch; a hero screenshot (or a short looping GIF/WebM, self-hosted); 4–6 key
      features; requirements; a **Download** button (latest GitHub Release, or the shop link, per the Phase A
      distribution decision); links to the manual, the source on GitHub, the changelog and issues; the license line
      and the Resolume disclaimer in the footer.
- [ ] A **Contact / legal notice** page with the copyright holder's name and the public contact address from
      Phase A. If binaries are sold (distribution (b) or (c)), this page must also meet the Swiss e-commerce
      information duties (clear identity and a contact address, Art. 3 para. 1 lit. s UWG). **[Uli]** reviews the
      wording; this isn't legal advice.
- [x] A short **Privacy** note: the site uses no cookies or tracking; GitHub hosts it (link to GitHub's privacy
      statement).
- [x] Works at phone width, passes basic accessibility checks (contrast, alt text, keyboard), and has a favicon and
      Open Graph tags (title, description, preview image) for link previews.

### Versioning
- [ ] The published manual always matches the **latest release**, not `main`. It's deployed by the release
      process (I5), so unreleased features never show up in the public manual.
- [x] Each manual page footer shows "Manual for version x.y.z". The version is injected at build time from
      `[workspace.package].version`.
- [x] Changes to user-facing behaviour must update the manual in the same PR (add this to `CONTRIBUTING.md` and
      the PR template checklist).

*(2026-11-03, done except the four open items:*
- *`site/`: `manual/` (mdBook 0.5, 16 chapters + generated shortcuts page), `landing/` (index, contact, privacy,
  style), `shared/` (generated: tokens, IBM Plex, icons), `build.mjs` instead of `build.ps1`: one Node script for
  Windows and the Linux runner (Node is needed for the UI anyway). It generates the shortcuts page from
  `web/src/lib/shortcuts.json` (now the app's single list), runs mdbook (warnings fail), injects "Manual for
  version x.y.z", fills `{{…}}` from `site/site.json`, and fails on broken internal links, missing CSS `url()`s
  and anything loaded from another host. `--release` fails without `contactEmail`.*
- *Checked in Chromium, served under `/clip-sequencer/` like GitHub Pages: no failed or external requests, IBM
  Plex loads, search works, no horizontal scroll at 390 px, one h1 per page, alt texts, skip link first in tab
  order; text contrast ≥ 6.8:1 (WCAG AA) for every colour pair used.*
- *Screenshots: 14 in `site/manual/src/images/`, from the demo show (`docs/samples/demo-*`, generated thumbnails),
  no running state (no MIDI clock in the build container).*
- *Open:* **Setting up Resolume:** *the exact preference path for MIDI clock sync and a recommended clip transport
  mode are not in the manual; Uli to confirm (the chapter says "MIDI clock sync in Resolume's MIDI settings").*
  **Contact page:** *needs `contactEmail` in `site/site.json` and Uli's review of the wording (and a postal address
  if the Swiss rules require one); the shop button appears once `shopUrl` is set.* **Latest release only:** *with
  the deployment (I5).* **Screenshot script:** *`site/screenshots.mjs` exists (it made the screenshots); keep or drop
  per Uli. The "Installing the FFGL plugin" chapter follows with G3.)*


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

_(Phase A, 2026-11-01)_

- Copyright holder: **Ulrich Staudinger**, i.e. `Copyright (c) 2026 Ulrich Staudinger` (same as the M12 splash).
- Project name: **Clip Sequencer for Resolume** (short name "Clip Sequencer", already the M12 app name).
  `clip-sequencer` is free on crates.io; no GitHub project of that name for Resolume was found.
- GitHub owner/repo: **`ghostrider/clip-sequencer`** (Uli creates it in H1).
- Starting version: **0.1.0**
- Binary distribution: **(c) both**: the portable zip and `SHA256SUMS.txt` are free on GitHub Releases, and binaries
  are also sold through a shop. Consequences: the landing page's contact / legal notice page must meet the Swiss
  e-commerce information duties (G2); release notes and the landing page link to the shop as well (shop URL: open,
  needed by G2/I2).
- Release approval required: **yes** (draft release + protected `release` environment)
- Manual language: **English only**
- Site address: **GitHub Pages URL**, `https://ghostrider.github.io/clip-sequencer/` (base path `/clip-sequencer/`)
- Public contact address: **open**. Uli creates a project address; needed before Phase G2.
- FFGL plugin: **"1000A Sequence"** (`SeqView`, plugin ID `VJSQ`), an FFGL 2.1 source plugin that shows a sequence
  (lanes, steps with thumbnails, playhead, pass counter) on the `§` placeholder clips of the SEQ layer. Source:
  `ffgl/` in this repo (moved from `vj_stuff/ffgl-plugins/seq_view/` on 2026-10-27). It currently builds through
  `SeqView.vcxproj` against `../../external/ffgl` and `../../third_party/stb` (to be vendored in Phase B) and uses
  the DLL runtime (`/MD`; G3 switches to `/MT`).
- FFGL plugin location: **(a) monorepo `ffgl/`**
- Resolume user plugin folder (verified): _(open, Phase G3)_

## Changes from M12 (2026-11-01)

M12 decided **portable zip only, no NSIS installer** (see `docs/M12-desktop-shell.md`, Decisions). This overrides
the installer items in this plan:
- G3 "Release contents": the NSIS installer component "FFGL plugin for Resolume Arena" is dropped. The plugin is
  installed from the app (File → Install FFGL plugin…) or `seq-app-cli --install-ffgl`, or by hand from the zip.
  The NSIS uninstaller item under "Uninstall" is dropped too.
- G2 manual "Installation": portable zip only (no installer vs. zip choice); WebView2 must be installed (it ships
  with Windows 11).
- E: exe metadata and the icon come from Tauri's bundler config, not `embed-resource`/`winres`.
- I2 step 4: build with `scripts/release.ps1` logic (UI, then `cargo build --release -p seq-app`, giving
  `seq-app.exe` and `seq-app-cli.exe`), not an installer. Assets: the zip (both exes, `ffgl/`, LICENSE,
  THIRD_PARTY_LICENSES, OFL.txt, README, CHANGELOG) and `SHA256SUMS.txt`.
- D: the notices also cover Tauri/wry and the Microsoft WebView2 runtime.
- F: the desktop app uses the native Tauri menu; the HTML menu bar appears only in browser/headless mode.
- `--install-ffgl` and other console-only flags live on `seq-app-cli.exe`.
