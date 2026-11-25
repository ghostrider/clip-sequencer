// A local stand-in for GitHub's release API, for testing the in-app update (SPEC §15, docs/RELEASING.md).
//
//   node scripts/fake-update-source.mjs <release zip> [--port 8765] [--notes "text"] [--prerelease]
//
// Serves, like api.github.com and its download links:
//   GET /repos/ghostrider/clip-sequencer/releases/latest   the release (version from the zip's name)
//   GET /dl/<zip name>                                    the zip
//   GET /dl/SHA256SUMS.txt                                its sums (the zip and the exes and DLL inside)
// Then start the app with `--update-source http://127.0.0.1:8765`.

import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { basename } from 'node:path';
import { inflateRawSync } from 'node:zlib';

const args = process.argv.slice(2);
const opt = (name, def) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : def;
};
const zipPath = args.find((a, i) => !a.startsWith('--') && !(i > 0 && args[i - 1].startsWith('--') && args[i - 1] !== '--prerelease'));
if (!zipPath) {
  console.error('usage: node scripts/fake-update-source.mjs <clip-sequencer-X.Y.Z-windows-x64.zip> [--port 8765] [--notes text] [--prerelease]');
  process.exit(2);
}
const port = Number(opt('--port', '8765'));
const zipName = basename(zipPath);
const version = /clip-sequencer-(.+)-windows-x64\.zip$/.exec(zipName)?.[1];
if (!version) {
  console.error(`${zipName}: not a release zip name (clip-sequencer-<version>-windows-x64.zip)`);
  process.exit(2);
}
const zip = readFileSync(zipPath);
const sha = (b) => createHash('sha256').update(b).digest('hex');

// The files inside the zip that the release workflow lists too (central directory walk; stored or deflated).
function entries(buf) {
  const out = [];
  let eocd = buf.length - 22;
  while (eocd >= 0 && buf.readUInt32LE(eocd) !== 0x06054b50) eocd--;
  if (eocd < 0) throw new Error('not a zip');
  let p = buf.readUInt32LE(eocd + 16);
  const n = buf.readUInt16LE(eocd + 10);
  for (let i = 0; i < n; i++) {
    const method = buf.readUInt16LE(p + 10);
    const csize = buf.readUInt32LE(p + 20);
    const nlen = buf.readUInt16LE(p + 28);
    const xlen = buf.readUInt16LE(p + 30);
    const clen = buf.readUInt16LE(p + 32);
    const local = buf.readUInt32LE(p + 42);
    const name = buf.toString('utf8', p + 46, p + 46 + nlen).replace(/\\/g, '/');
    const lstart = local + 30 + buf.readUInt16LE(local + 26) + buf.readUInt16LE(local + 28);
    const raw = buf.subarray(lstart, lstart + csize);
    if (!name.endsWith('/')) out.push([name, method === 0 ? raw : inflateRawSync(raw)]);
    p += 46 + nlen + xlen + clen;
  }
  return out;
}
const listed = ['clip-sequencer/seq-app.exe', 'clip-sequencer/seq-app-cli.exe', 'clip-sequencer/ffgl/SeqView.dll'];
const lines = [`${sha(zip)}  ${zipName}`];
for (const [name, data] of entries(zip)) if (listed.includes(name)) lines.push(`${sha(data)}  ${name}`);
const sums = lines.join('\n') + '\n';

const base = `http://127.0.0.1:${port}`;
const release = {
  tag_name: `v${version}`,
  name: `Clip Sequencer for Resolume ${version}`,
  body: opt('--notes', `Test release ${version} served by scripts/fake-update-source.mjs.`),
  html_url: `${base}/page`,
  draft: false,
  prerelease: args.includes('--prerelease'),
  assets: [
    { name: zipName, size: zip.length, browser_download_url: `${base}/dl/${zipName}` },
    { name: 'SHA256SUMS.txt', size: sums.length, browser_download_url: `${base}/dl/SHA256SUMS.txt` },
  ],
};

createServer((req, res) => {
  console.log(new Date().toISOString(), req.method, req.url);
  if (req.url === '/repos/ghostrider/clip-sequencer/releases/latest') {
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify(release));
  } else if (req.url === `/dl/${zipName}`) {
    res.writeHead(200, { 'content-type': 'application/zip', 'content-length': zip.length });
    res.end(zip);
  } else if (req.url === '/dl/SHA256SUMS.txt') {
    res.writeHead(200, { 'content-type': 'text/plain' });
    res.end(sums);
  } else {
    res.writeHead(404);
    res.end();
  }
}).listen(port, '127.0.0.1', () => {
  console.log(`fake update source for ${version} on ${base} (${lines.length - 1} file(s) listed in SHA256SUMS.txt)`);
  console.log(`start the app with: --update-source ${base}`);
});
