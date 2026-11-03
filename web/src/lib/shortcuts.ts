// Keyboard shortcuts (SPEC §9.3 plus the app's own): the one list behind Help → Keyboard shortcuts, in the
// browser UI and, through the page, in the desktop app's native Help menu.

export interface Shortcut {
  keys: string;
  what: string;
  /** Only in the desktop app (`desktop`) or only in the browser UI (`browser`). */
  only?: 'desktop' | 'browser';
}

export const SHORTCUT_GROUPS: { title: string; items: Shortcut[] }[] = [
  {
    title: 'Sequence editor',
    items: [
      { keys: 'Space', what: 'Start / stop the selected sequence' },
      { keys: 'Del', what: 'Remove the selected step' },
      { keys: 'R', what: 'Insert a one-bar rest after the selected step' },
      { keys: 'Esc', what: 'Deselect' },
      { keys: 'Ctrl+Z', what: 'Undo' },
      { keys: 'Ctrl+Shift+Z', what: 'Redo' },
      { keys: 'Drag edge', what: 'Resize a step, snapping to 1/8' },
      { keys: 'Shift+drag edge', what: 'Resize a step, snapping to 1/16' },
      { keys: 'Alt+drag', what: 'Duplicate a step' },
      { keys: 'Ctrl+wheel', what: 'Zoom the lane grid' },
    ],
  },
  {
    title: 'App',
    items: [
      { keys: 'Ctrl+1 / Ctrl+2', what: 'Sequencer / Console tab' },
      { keys: 'Ctrl+,', what: 'Preferences (MIDI clock input)' },
      { keys: 'Alt or F10', what: 'Open the menu bar (arrow keys move, Esc closes)', only: 'browser' },
      { keys: 'Ctrl+= / Ctrl+- / Ctrl+0', what: 'Zoom the window in / out / reset', only: 'desktop' },
      { keys: 'Ctrl+Q', what: 'Quit', only: 'desktop' },
    ],
  },
];
