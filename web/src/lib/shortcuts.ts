// Keyboard shortcuts (SPEC §9.3 plus the app's own). The data lives in shortcuts.json: the one list behind
// Help → Keyboard shortcuts (browser UI and, through the page, the desktop app's native Help menu) and the
// manual's "Keyboard shortcuts" page, which site/build.mjs generates from the same file.

import groups from './shortcuts.json';

export interface Shortcut {
  keys: string;
  what: string;
  /** Only in the desktop app (`desktop`) or only in the browser UI (`browser`). */
  only?: 'desktop' | 'browser';
}

export const SHORTCUT_GROUPS = groups as { title: string; items: Shortcut[] }[];
