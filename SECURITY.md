# Security policy

## Reporting a vulnerability

Please don't open a public issue for a security problem. Report it privately through GitHub:
**[Security → Report a vulnerability](https://github.com/ghostrider/clip-sequencer/security/advisories/new)**
on this repository. Include what you found, how to reproduce it, the app version (Help → About, or
`seq-app-cli.exe --version`), and your Windows and Resolume versions.

This is a project maintained in spare time. You'll get an answer as soon as possible, usually within a
week. Fixes are released as a new patch version and credited in the advisory unless you prefer otherwise.

## Supported versions

Only the latest release gets security fixes.

## Scope and design

- The UI server listens on `127.0.0.1` only (there is no option to open it to the network). It has no login:
  any program on the same computer can use the UI and its API, as with Resolume's own webserver. Reports about
  that are in scope only if they let something reach the app from another computer.
- The app talks to Resolume over its REST API, WebSocket and OSC (by default on `127.0.0.1`), and reads and
  writes sequence files, settings, caches and logs in its data folder (`%APPDATA%\clip-sequencer`).
- The FFGL plugin only connects to the sequencer's UI server (`127.0.0.1:8090` by default).
- Release binaries are built by GitHub Actions from the tagged source; each release lists SHA-256 checksums.
- Update check and install (SPEC §15): the app asks `api.github.com` for the latest release once at each start
  (unless turned off) and when the user chooses Check for updates. An install downloads the release zip and
  `SHA256SUMS.txt` over HTTPS, checks the zip and the exes and plugin in it against those sums, and swaps the files
  of the program folder with a helper that keeps the old ones until the new version has started (else it puts them
  back). The sums come from the same release as the zip, so they prove an intact download, not its origin; signed
  releases will add a signature check. It never touches the data folder.
