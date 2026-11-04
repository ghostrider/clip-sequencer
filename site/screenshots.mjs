#!/usr/bin/env node
// Regenerates the manual's screenshots (site/manual/src/images/) from the neutral demo show: mock-resolume with
// docs/samples/demo-composition.json, the demo thumbnails (docs/samples/demo-thumbs/) and docs/samples/demo.seq.json.
// No real footage ever appears. Needs Playwright with Chromium (`npm i -g playwright && npx playwright install chromium`,
// found through NODE_PATH) and the debug binaries:
//   cargo build -p mock-resolume -p seq-app --bin mock-resolume --bin seq-app-cli
//   NODE_PATH=$(npm root -g) node site/screenshots.mjs
// The MIDI clock is not simulated, so the shots show the app idle (CLOCK chip red).

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'site/manual/src/images');
const exe = (name) => {
  const p = join(root, 'target/debug', process.platform === 'win32' ? `${name}.exe` : name);
  if (!existsSync(p)) throw new Error(`${p} missing: cargo build -p mock-resolume -p seq-app --bin mock-resolume --bin seq-app-cli`);
  return p;
};
const PORTS = { rest: 18080, osc: 17000, ui: 18090 };
const ui = `http://127.0.0.1:${PORTS.ui}`;
const rest = `http://127.0.0.1:${PORTS.rest}/api/v1`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitFor(url) {
  for (let i = 0; i < 60; i++) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {
      /* not up yet */
    }
    await sleep(250);
  }
  throw new Error(`${url} did not come up`);
}

const children = [];
process.on('exit', () => children.forEach((c) => c.kill()));

// Mock Resolume with the demo composition and its thumbnails.
const composition = join(root, 'docs/samples/demo-composition.json');
children.push(spawn(exe('mock-resolume'), ['--rest-port', PORTS.rest, '--osc-port', PORTS.osc, '--composition', composition], { stdio: 'ignore' }));
await waitFor(`${rest}/product`);
const comp = JSON.parse(readFileSync(composition, 'utf8'));
const slug = (n) => n.toLowerCase().replace(/ /g, '-');
for (const layer of comp.layers) {
  for (const c of layer.clips) {
    const png = join(root, 'docs/samples/demo-thumbs', `${slug(c.name.value)}.png`);
    if (!c.name.value || !existsSync(png)) continue;
    const form = new FormData();
    form.append('file', new Blob([readFileSync(png)], { type: 'image/png' }), 'thumb.png');
    const r = await fetch(`${rest}/composition/clips/by-id/${c.id}/thumbnail`, { method: 'POST', body: form });
    if (!r.ok) throw new Error(`thumbnail upload for ${c.name.value}: HTTP ${r.status}`);
  }
}

// The sequencer, headless, on a fresh data folder with the demo sequences.
const data = mkdtempSync(join(tmpdir(), 'seq-shots-'));
cpSync(join(root, 'docs/samples/demo.seq.json'), join(data, 'Demo.seq.json'));
children.push(spawn(exe('seq-app-cli'), ['--rest-port', PORTS.rest, '--osc-port', PORTS.osc, '--ui-port', PORTS.ui, '--strict-port', '--data-dir', data], { stdio: 'ignore' }));
await waitFor(`${ui}/api/version`);

mkdirSync(out, { recursive: true });
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 820 } });
const shot = async (name, target = page) => {
  await sleep(400); // thumbnails and mosaics settle
  await target.screenshot({ path: join(out, `${name}.png`) });
  console.log('shot', name);
};
const card = (name) => page.locator('.seqcard', { hasText: name });
const zoomGrid = async (steps) => {
  await page.locator('.grid').hover();
  for (let i = 0; i < steps; i++) {
    await page.keyboard.down('Control');
    await page.mouse.wheel(0, -100);
    await page.keyboard.up('Control');
  }
};

await page.goto(ui);
await page.waitForSelector('.seqcard');
await card('Drop A').click();
await zoomGrid(3);
await shot('overview');
await shot('top-bar', page.locator('.topbar'));
await shot('sequence-list', page.locator('.seqlist'));
await shot('editor', page.locator('.editor'));
await page.locator('.step', { hasText: 'Rings' }).click();
await shot('side-panel', page.locator('.side'));
await page.keyboard.press('Escape');

// Unresolved step (its clip is gone from the composition) and the inspector's resolution status.
await card('Old Set').click();
await page.locator('.step').nth(1).click();
await shot('unresolved-step');
await page.keyboard.press('Escape');

// A deck that no longer exists in Resolume: its sequences are orphaned.
await page.locator('.topbar .tab', { hasText: 'Act 4' }).click();
await shot('orphaned-deck');
await page.locator('.topbar .tab', { hasText: 'Act 2' }).click();

// Sequence card menu (right-click).
await card('Intro').click({ button: 'right' });
await sleep(300);
await page.screenshot({ path: join(out, 'sequence-menu.png'), clip: { x: 0, y: 90, width: 520, height: 420 } });
console.log('shot', 'sequence-menu');
await page.keyboard.press('Escape');
await page.mouse.click(700, 700);

// Quick start: a new two-lane sequence.
await page.getByRole('button', { name: '+ New' }).click();
await sleep(300);
await shot('quickstart-1-new');
const lanes = page.locator('.lane');
const dragClip = async (clip, lane) => {
  await page.locator('.side [draggable="true"]', { hasText: clip }).first().dragTo(lanes.nth(lane));
  await sleep(250);
};
await page.locator('.layer-select').first().selectOption('Background');
await dragClip('Gradient Sweep', 0);
await shot('quickstart-2-first-clip');
await dragClip('Night Sky', 0);
await page.getByRole('button', { name: '+ Lane' }).click();
await sleep(300);
await page.locator('.layer-select').nth(1).selectOption('Mid');
await dragClip('Bars', 1);
await dragClip('Rings', 1);
await dragClip('Tunnel', 1);
await shot('quickstart-3-two-lanes');
await page.getByRole('button', { name: '∞' }).click();
await shot('quickstart-4-repeat', page.locator('.editor'));

// Help menu dialogs and the Console tab.
await page.getByRole('menuitem', { name: 'Help' }).click();
await page.getByRole('menuitem', { name: 'About' }).click();
await shot('about');
await page.keyboard.press('Escape');
await page.getByRole('tab', { name: /Console/ }).click();
await sleep(800);
await shot('console');

await browser.close();
rmSync(data, { recursive: true, force: true });
console.log(`screenshots in ${out}: ${readdirSync(out).length} files`);
process.exit(0);
