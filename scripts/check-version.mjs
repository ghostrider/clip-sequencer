#!/usr/bin/env node
// Checks that the version has one source (M11 Phase E): [workspace.package] in Cargo.toml.
//   - every crate inherits it (`version.workspace = true`)
//   - web/package.json and web/package-lock.json carry the same version
//   - crates/seq-app/tauri.conf.json sets no version (Tauri then takes Cargo's)
//   - with --tag vX.Y.Z (or a tag in GITHUB_REF_NAME when --tag is given without a value): the tag matches
// The FFGL plugin reads the version from Cargo.toml itself (ffgl/SeqView.vcxproj), so it cannot drift.
// Usage: node scripts/check-version.mjs [--tag [vX.Y.Z]] [--github-output]
//   --github-output  also appends "version=X.Y.Z" to $GITHUB_OUTPUT (GitHub Actions step output)
// Exit code 0 = consistent, 1 = mismatch (with a message per problem).

import { appendFileSync, readFileSync, readdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => readFileSync(join(root, p), 'utf8');
const problems = [];

const version = /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(read('Cargo.toml').replace(/\r\n/g, '\n'))?.[1];
if (!version) {
  console.error('check-version: no version in [workspace.package] of Cargo.toml');
  process.exit(1);
}
if (!/^\d+\.\d+\.\d+$/.test(version)) problems.push(`Cargo.toml: version "${version}" is not MAJOR.MINOR.PATCH`);

for (const dir of ['crates', 'tools']) {
  for (const name of readdirSync(join(root, dir))) {
    const manifest = join(dir, name, 'Cargo.toml');
    if (!existsSync(join(root, manifest))) continue;
    const pkg = /\[package\]([^[]*)/.exec(read(manifest))?.[1] ?? '';
    if (!/^version\.workspace\s*=\s*true\s*$/m.test(pkg)) {
      problems.push(`${manifest}: must use "version.workspace = true"`);
    }
  }
}

const pkg = JSON.parse(read('web/package.json'));
if (pkg.version !== version) problems.push(`web/package.json: version ${pkg.version}, Cargo.toml ${version}`);
const lock = JSON.parse(read('web/package-lock.json'));
if (lock.version !== version || lock.packages?.['']?.version !== version) {
  problems.push(`web/package-lock.json: version ${lock.version} / ${lock.packages?.['']?.version}, Cargo.toml ${version}`);
}

const tauri = JSON.parse(read('crates/seq-app/tauri.conf.json'));
if ('version' in tauri) problems.push('crates/seq-app/tauri.conf.json: must not set "version"');

const tagArg = process.argv.indexOf('--tag');
if (tagArg > 0) {
  const next = process.argv[tagArg + 1];
  const tag = next && !next.startsWith('--') ? next : process.env.GITHUB_REF_NAME;
  if (!tag) problems.push('--tag: no tag given and GITHUB_REF_NAME is not set');
  else if (tag !== `v${version}`) problems.push(`tag ${tag} does not match the version: expected v${version}`);
}

if (problems.length) {
  for (const p of problems) console.error(`check-version: ${p}`);
  process.exit(1);
}
console.log(`check-version: ${version} everywhere${tagArg > 0 ? ' (tag matches)' : ''}`);
if (process.argv.includes('--github-output')) {
  if (!process.env.GITHUB_OUTPUT) {
    console.error('check-version: --github-output needs GITHUB_OUTPUT');
    process.exit(1);
  }
  appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\n`);
}
