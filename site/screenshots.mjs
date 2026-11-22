#!/usr/bin/env node
// Regenerates the manual's screenshots (site/manual/src/images/) from the neutral demo show: mock-resolume with
// docs/samples/demo-composition.json, the demo thumbnails (docs/samples/demo-thumbs/) and docs/samples/demo.seq.json.
// No real footage ever appears. Needs Playwright with Chromium (`npm i -g playwright && npx playwright install chromium`,
// found through NODE_PATH) and the debug binaries:
//   cargo build -p mock-resolume -p seq-app --bin mock-resolume --bin seq-app-cli
//   NODE_PATH=$(npm root -g) node site/screenshots.mjs
// The MIDI clock is not simulated, so the shots show the app idle (CLOCK chip red). The last shots switch the clock
// source to Resolume's tempo (the mock's tempo controller, 120 BPM) for the clock page.

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
await page.getByRole('tab', { name: /Sequencer/ }).click();

// Clock source (SPEC §3.1): Preferences → Clock with a MIDI input and with Resolume's tempo, then the clock chip
// before and after a Resync. The MIDI port list is made up, so no port names of this machine appear.
await page.route('**/api/midi/ports', (r) =>
  r.fulfill({ json: { ports: ['loopMIDI Port', 'Clock Shifter Out', 'Arturia BeatStep Pro'], selected: 'loopMIDI Port' } }),
);
await page.locator('.statusbar .chip-btn').click();
await page.locator('.prefs').waitFor();
await sleep(300);
await shot('prefs-clock-midi', page.locator('.prefs'));
await page.locator('input[name="clock-source"][value="resolume"]').check();
await shot('prefs-clock-resolume', page.locator('.prefs'));
await page.getByRole('button', { name: 'Save' }).click();
const chip = page.locator('.statusbar .chip-btn');
await chip.filter({ hasText: 'press Resync' }).waitFor();
await shot('clock-chip-resolume', chip);
// Resync pressed in "Resolume": the mock reports it to the app like Resolume does (notes §17).
const resyncId = comp.tempocontroller.resync.id;
const mockWs = new WebSocket(`ws://127.0.0.1:${PORTS.rest}/api/v1`);
await new Promise((res, rej) => {
  mockWs.onopen = res;
  mockWs.onerror = rej;
});
mockWs.send(JSON.stringify({ action: 'set', parameter: `/parameter/by-id/${resyncId}`, value: true }));
await chip.filter({ hasText: 'synced' }).waitFor();
await shot('clock-chip-synced', chip);
mockWs.close();

// Colour LUTs (SPEC §14): a LUT made in the tab and used by the composition's LUT effect (the demo composition has
// one on the composition and one on the Front layer).
await page.keyboard.press('Control+3');
await page.getByRole('button', { name: '+ New' }).click();
await page.locator('.lut-card', { hasText: 'New LUT' }).waitFor();
await page.locator('.lut-editor h1').dblclick();
await page.locator('input.rename').fill('Stage Warm');
await page.keyboard.press('Enter');
await page.locator('.lut-card', { hasText: 'Stage Warm' }).waitFor();
const setSlider = (group, label, value) =>
  page
    .locator(group ? `.group:has(.caps:text-is("${group}")) .ctl` : '.ctl', { hasText: label })
    .first()
    .locator('input[type=range]')
    .evaluate((el, v) => {
      el.value = String(v);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    }, value);
await setSlider(null, 'Temperature', 0.45);
await sleep(100);
await setSlider(null, 'Contrast', 1.15);
await sleep(100);
await setSlider(null, 'Saturation', 1.2);
await sleep(100);
// A gentle S on the master curve.
await page.locator('.curve-box').scrollIntoViewIfNeeded();
const curve = await page.locator('.curve-box').boundingBox();
for (const [x, y, to] of [[0.25, 0.75, 0.8], [0.75, 0.25, 0.2]]) {
  await page.mouse.move(curve.x + curve.width * x, curve.y + curve.height * y);
  await page.mouse.down();
  await page.mouse.move(curve.x + curve.width * x, curve.y + curve.height * to, { steps: 4 });
  await page.mouse.up();
}
await page.locator('.lut-controls').evaluate((el) => (el.scrollTop = 0));
await page.locator('#lut-image').selectOption({ label: 'Skin tones' });
await page.locator('.fx-row', { hasText: 'Composition' }).getByRole('button', { name: /Use Stage Warm/ }).click();
await page.locator('.fx-row', { hasText: 'Composition' }).locator('.tag', { hasText: 'this LUT' }).waitFor();
await page.locator('.preview').click({ position: { x: 420, y: 200 } }); // split a little left of the middle
await sleep(800);
await shot('luts-tab');
await shot('luts-effects', page.locator('.in-resolume'));
await page.setViewportSize({ width: 1440, height: 1100 });
await sleep(300);
await shot('luts-controls', page.locator('.lut-controls'));
await page.setViewportSize({ width: 1440, height: 820 });

await browser.close();
rmSync(data, { recursive: true, force: true });
console.log(`screenshots in ${out}: ${readdirSync(out).length} files`);
process.exit(0);
