#!/usr/bin/env node
// Generates THIRD_PARTY_LICENSES.md (for the release zip) and THIRD_PARTY_LICENSES.json (for the in-app
// License dialog, M11 Phase F) from:
//   - Rust crates of the Windows build: `cargo about generate --format json` (about.toml)
//   - Web UI: web/dist/third-party-licenses.json, written by `npm run build` (rollup-plugin-license)
//   - Fonts, FFGL plugin dependencies (FFGL SDK, GLEW, stb): fixed entries below, texts read from the repo
// Requires: cargo-about (`cargo install --locked cargo-about --features cli`), and the UI built first.
// Usage: node scripts/third-party-licenses.mjs [--out-dir <dir>]   (default: repository root)

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const outArg = process.argv.indexOf('--out-dir');
const outDir = outArg > 0 ? process.argv[outArg + 1] : root;
const read = (p) => readFileSync(join(root, p), 'utf8').replace(/\r\n/g, '\n');

function fail(msg) {
  console.error(`third-party-licenses: ${msg}`);
  process.exit(1);
}

const version = /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(read('Cargo.toml'))?.[1];
if (!version) fail('no version in [workspace.package] of Cargo.toml');

// --- Rust crates -------------------------------------------------------------------------------
let about;
try {
  about = JSON.parse(
    execFileSync('cargo', ['about', 'generate', '--format', 'json'], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'inherit'],
    }),
  );
} catch (e) {
  fail(`cargo about failed (${e.message}). Install it: cargo install --locked cargo-about --features cli`);
}

const texts = []; // distinct license texts
const textIndex = new Map();
const addText = (t) => {
  const text = t.replace(/\r\n/g, '\n').trimEnd();
  if (!textIndex.has(text)) {
    textIndex.set(text, texts.length);
    texts.push(text);
  }
  return textIndex.get(text);
};

const packages = [];
const rustByKey = new Map();
for (const group of about.licenses) {
  const t = addText(group.text);
  for (const { crate } of group.used_by) {
    const key = `${crate.name}@${crate.version}`;
    let p = rustByKey.get(key);
    if (!p) {
      p = {
        name: crate.name,
        version: crate.version,
        license: crate.license ?? '',
        part: 'Desktop app (Rust)',
        url: crate.repository ?? `https://crates.io/crates/${crate.name}`,
        texts: [],
      };
      rustByKey.set(key, p);
      packages.push(p);
    }
    if (!p.texts.includes(t)) p.texts.push(t);
  }
}

// npm "repository" may be a URL ("git+https://….git") or GitHub shorthand ("owner/repo").
const repoUrl = (r) => {
  if (!r) return undefined;
  const u = r.replace(/^git\+/, '').replace(/\.git$/, '');
  return /^[\w.-]+\/[\w.-]+$/.test(u) ? `https://github.com/${u}` : u;
};

// --- Web UI --------------------------------------------------------------------------------------
const webFile = join(root, 'web/dist/third-party-licenses.json');
if (!existsSync(webFile)) fail('web/dist/third-party-licenses.json missing: run `npm run build` in web/ first');
for (const d of JSON.parse(readFileSync(webFile, 'utf8'))) {
  if (!d.licenseText) fail(`web dependency ${d.name} ${d.version} has no license text`);
  packages.push({
    name: d.name,
    version: d.version,
    license: d.license,
    part: 'Web UI (bundled JavaScript)',
    url: repoUrl(d.repository) ?? `https://www.npmjs.com/package/${d.name}`,
    texts: [addText(d.noticeText ? `${d.licenseText}\n\n${d.noticeText}` : d.licenseText)],
  });
}

// --- Fixed entries -------------------------------------------------------------------------------
const lock = JSON.parse(read('web/package-lock.json'));
const npmVersion = (name) => lock.packages[`node_modules/${name}`]?.version ?? '';
const ofl = addText(read('OFL.txt'));
for (const [name, pkg] of [
  ['IBM Plex Sans', '@fontsource/ibm-plex-sans'],
  ['IBM Plex Mono', '@fontsource/ibm-plex-mono'],
]) {
  packages.push({
    name,
    version: `@fontsource ${npmVersion(pkg)}`,
    license: 'OFL-1.1',
    part: 'Fonts (UI and splash screen)',
    url: 'https://github.com/IBM/plex',
    texts: [ofl],
  });
}

let ffglCommit = '';
try {
  ffglCommit = execFileSync('git', ['-C', root, 'ls-tree', 'HEAD', 'external/ffgl'], { encoding: 'utf8' })
    .split(/\s+/)[2]
    .slice(0, 7);
} catch {
  /* not a git checkout: leave the version empty */
}
const stbText = (file) => {
  const src = read(file);
  const a = src.indexOf('ALTERNATIVE A - MIT License');
  const end = src.indexOf('*/', a);
  if (a < 0 || end < 0) fail(`license block not found in ${file}`);
  return src.slice(a, end).trim();
};
const stbVersion = (file) => /- v(\d+\.\d+)/.exec(read(file))?.[1] ?? '';
packages.push(
  {
    name: 'FFGL SDK',
    version: ffglCommit ? `git ${ffglCommit}` : '',
    license: 'BSD-3-Clause',
    part: 'FFGL plugin',
    url: 'https://github.com/resolume/ffgl',
    texts: [addText(read('external/ffgl/LICENSE.md'))],
  },
  {
    name: 'GLEW (The OpenGL Extension Wrangler Library)',
    version: '2.1.0',
    license: 'BSD-3-Clause AND MIT',
    part: 'FFGL plugin',
    url: 'https://github.com/nigels-com/glew',
    texts: [addText(read('external/ffgl/deps/glew-2.1.0/LICENSE.txt'))],
  },
  {
    name: 'stb_image',
    version: stbVersion('third_party/stb/stb_image.h'),
    license: 'MIT OR Unlicense',
    part: 'FFGL plugin',
    url: 'https://github.com/nothings/stb',
    texts: [addText(stbText('third_party/stb/stb_image.h'))],
  },
  {
    name: 'stb_truetype',
    version: stbVersion('third_party/stb/stb_truetype.h'),
    license: 'MIT OR Unlicense',
    part: 'FFGL plugin',
    url: 'https://github.com/nothings/stb',
    texts: [addText(stbText('third_party/stb/stb_truetype.h'))],
  },
);

// --- Output --------------------------------------------------------------------------------------
const parts = [
  'Desktop app (Rust)',
  'Web UI (bundled JavaScript)',
  'Fonts (UI and splash screen)',
  'FFGL plugin',
];
packages.sort(
  (a, b) =>
    parts.indexOf(a.part) - parts.indexOf(b.part) ||
    a.name.localeCompare(b.name, 'en') ||
    a.version.localeCompare(b.version, 'en', { numeric: true }),
);

const header = `Clip Sequencer for Resolume ${version} includes the third-party components listed below.`;
const systemNote =
  'Not included: the Microsoft Edge WebView2 Runtime, which the desktop app uses from Windows ' +
  '(part of Windows 11; licensed by Microsoft under its own terms), and Resolume Arena.';

writeFileSync(
  join(outDir, 'THIRD_PARTY_LICENSES.json'),
  JSON.stringify({ product: 'Clip Sequencer for Resolume', version, note: systemNote, packages, texts }, null, 1) +
    '\n',
);

const md = [];
md.push('# Third-party licenses', '', header, '', systemNote, '');
for (const part of parts) {
  const list = packages.filter((p) => p.part === part);
  md.push(`## ${part} (${list.length})`, '', '| Component | Version | License |', '|---|---|---|');
  for (const p of list) md.push(`| [${p.name}](${p.url}) | ${p.version} | ${p.license} |`);
  md.push('');
}
md.push('## License texts', '');
texts.forEach((text, i) => {
  const users = packages.filter((p) => p.texts.includes(i)).map((p) => `${p.name} ${p.version}`.trim());
  md.push(`### ${i + 1}. Used by: ${users.join(', ')}`, '', '```text', text.replace(/```/g, "'''"), '```', '');
});
writeFileSync(join(outDir, 'THIRD_PARTY_LICENSES.md'), md.join('\n'));

console.log(
  `third-party-licenses: ${packages.length} components, ${texts.length} distinct texts -> ` +
    `${join(outDir, 'THIRD_PARTY_LICENSES.md')} (+ .json)`,
);
