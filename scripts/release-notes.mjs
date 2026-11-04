#!/usr/bin/env node
// Extracts the CHANGELOG.md section of a version into release-notes.md (M11 Phase I2) and fails if there is none.
//   node scripts/release-notes.mjs [--version x.y.z] [--release] [--out release-notes.md]
// --version: default [workspace.package] version. --release (tagged releases): the section must carry a date,
// "## [x.y.z] - YYYY-MM-DD", not "Unreleased" (docs/RELEASING.md step 2).

import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const arg = (name) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? process.argv[i + 1] : undefined;
};
const fail = (msg) => {
  console.error(`release-notes: ${msg}`);
  process.exit(1);
};

const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8').replace(/\r\n/g, '\n');
const version = arg('--version') ?? /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(cargo)?.[1];
if (!version) fail('no version');
const out = arg('--out') ?? join(root, 'release-notes.md');

const changelog = readFileSync(join(root, 'CHANGELOG.md'), 'utf8').replace(/\r\n/g, '\n');
const lines = changelog.split('\n');
const escaped = version.replace(/\./g, '\\.');
const start = lines.findIndex((l) => new RegExp(`^## \\[${escaped}\\]`).test(l));
if (start < 0) fail(`CHANGELOG.md has no "## [${version}]" section`);
const header = lines[start];
if (process.argv.includes('--release') && !/^## \[[^\]]+\] - \d{4}-\d{2}-\d{2}\s*$/.test(header)) {
  fail(`"${header}" needs a release date ("## [${version}] - YYYY-MM-DD"), see docs/RELEASING.md`);
}
let end = lines.findIndex((l, i) => i > start && /^## \[/.test(l));
if (end < 0) end = lines.length;
// Without the link references at the end of the file.
const body = lines
  .slice(start + 1, end)
  .filter((l) => !/^\[[^\]]+\]: https?:\/\//.test(l))
  .join('\n')
  .trim();
if (!body) fail(`the "## [${version}]" section of CHANGELOG.md is empty`);

const notes = `${body}\n\n---\n\nDownload \`clip-sequencer-${version}-windows-x64.zip\`, check it against \`SHA256SUMS.txt\`, unzip and ` +
  `run \`seq-app.exe\`. The exe is not code-signed: Windows SmartScreen may ask, choose "More info → Run anyway". ` +
  `Manual: https://ghostrider.github.io/clip-sequencer/manual/\n`;
writeFileSync(out, notes);
console.log(`release-notes: ${header.replace(/^## /, '')} -> ${out}`);
