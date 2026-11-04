// App state: what the engine sends over /ws, plus local view state (selected deck tab,
// sequence, step, zoom). Selecting a deck tab only changes the view (SPEC §7.2).

import type {
  ClipRef, ClipView, Command, DeckView, EditOp, LogRecord, LogStats, RunView, RuntimeState,
  SequenceView, StructureState, Uuid, VersionInfo,
} from './types';

export type InfoDialog = 'about' | 'licenses' | 'shortcuts' | 'quit' | 'ffgl';

const ZOOM_KEY = 'seq.uiZoom';
function loadUiZoom(): number {
  try {
    const z = Number(localStorage.getItem(ZOOM_KEY));
    return z >= 0.5 && z <= 2 ? z : 1;
  } catch {
    return 1;
  }
}
/** CSS zoom on the root (browser UI); remembered per browser. */
function applyUiZoom(z: number) {
  document.documentElement.style.zoom = z === 1 ? '' : String(z);
  try {
    localStorage.setItem(ZOOM_KEY, String(z));
  } catch {
    /* private mode: not remembered */
  }
}

/** Records the Console tab keeps (the app's ring buffer holds as many). */
export const LOG_LIMIT = 5000;

/** Step lengths are in sixteenths (file format v2); 4/4 only. */
export const SIXTEENTHS_PER_BAR = 16;
export const TICKS_PER_SIXTEENTH = 6;
/** Default resize snap: an eighth (Shift while dragging snaps to sixteenths). */
export const SNAP_SIXTEENTHS = 2;

/** Identifies a deck tab: its position (deck names need not be unique), or the name of an orphaned deck. */
export function deckKey(d: DeckView): string {
  return d.index !== null ? `#${d.index}` : `name:${d.name}`;
}

class AppState {
  structure = $state<StructureState | null>(null);
  runtime = $state<RuntimeState | null>(null);
  connected = $state(false);
  /** Running inside the desktop shell (`/api/version`): Ctrl+Q and the zoom keys go to it. */
  desktop = $state(false);
  version = $state<VersionInfo | null>(null);

  /** Top-level tab (M12 E). Log records only flow while the Console tab is open. */
  tab = $state<'sequencer' | 'console'>('sequencer');
  logs = $state.raw<LogRecord[]>([]);
  logStats = $state<LogStats>({ errors: 0, dropped: 0 });
  /** Error count when the Console tab was last visible (for the badge). */
  errorsSeen = $state(0);
  /** Preferences dialog (MIDI clock input). */
  prefsOpen = $state(false);
  /** Info dialogs of the Help menu (M11 Phase F); one at a time. */
  dialog = $state<InfoDialog | null>(null);
  /** Browser UI only: page zoom set from the View menu (the desktop app zooms its window). */
  uiZoom = $state(loadUiZoom());

  /** Deck tab being shown (see `deckKey`); `null` = follow the live deck. */
  viewDeckKey = $state<string | null>(null);
  selectedSeq = $state<Uuid | null>(null);
  selectedStep = $state<{ lane: number; index: number } | null>(null);
  /** Pixels per sixteenth in the lane grid. */
  zoom = $state(7);
  search = $state('');
  /** Waiting for a clip click in the browser to relink the selected step. */
  relinking = $state(false);
  /** Pending deck-switch confirmation (SPEC §7.2 step 1). */
  confirmSwitch = $state<{ seq: Uuid; name: string; deck: string; running: string[] } | null>(null);

  private ws: WebSocket | null = null;

  connect() {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    this.ws = ws;
    ws.onopen = () => {
      this.connected = true;
      fetch('/api/version')
        .then((r) => r.json())
        .then((v: VersionInfo) => {
          this.version = v;
          this.desktop = v.desktop === true;
          // The browser UI's zoom from an earlier visit; the desktop window (same origin) zooms itself.
          document.documentElement.style.zoom = !this.desktop && this.uiZoom !== 1 ? String(this.uiZoom) : '';
        })
        .catch(() => {});
      if (this.tab === 'console') this.send({ cmd: 'log_subscribe' });
    };
    ws.onclose = () => {
      this.connected = false;
      setTimeout(() => this.connect(), 1000);
    };
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.type === 'structure') this.structure = msg.data;
      else if (msg.type === 'runtime') this.runtime = msg.data;
      else if (msg.type === 'log_snapshot') {
        this.logs = msg.data.records.slice(-LOG_LIMIT);
        this.logStats = msg.data.stats;
      } else if (msg.type === 'log') this.addLogs(msg.data);
      else if (msg.type === 'log_stats') this.logStats = msg.data;
    };
  }

  send(cmd: Command) {
    if (this.ws?.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(cmd));
  }

  edit(op: EditOp) {
    this.send({ cmd: 'edit', op });
  }

  // ---- menus (M11 Phase F) ----

  openDialog(d: InfoDialog) {
    this.prefsOpen = false;
    this.dialog = d;
  }

  /** Project links, from Cargo metadata via `/api/version` (fallbacks until it has loaded). */
  get links() {
    const repo = this.version?.repository ?? 'https://github.com/ghostrider/clip-sequencer';
    const home = this.version?.homepage ?? 'https://ghostrider.github.io/clip-sequencer/';
    const site = home.endsWith('/') ? home : `${home}/`;
    return { repo, site, manual: `${site}manual/`, issues: `${repo}/issues/new/choose` };
  }

  /** View menu of the browser UI: `step` in 10 % steps, `null` resets. */
  zoomUi(step: number | null) {
    const z = step === null ? 1 : Math.round((this.uiZoom + step) * 10) / 10;
    this.uiZoom = Math.min(2, Math.max(0.5, z));
    applyUiZoom(this.uiZoom);
  }

  // ---- console ----

  setTab(tab: 'sequencer' | 'console') {
    if (tab === this.tab) return;
    this.errorsSeen = this.logStats.errors;
    this.tab = tab;
    this.send({ cmd: tab === 'console' ? 'log_subscribe' : 'log_unsubscribe' });
  }

  /** New errors since the Console tab was last visible (badge on the tab). */
  get newErrors(): number {
    return this.tab === 'console' ? 0 : Math.max(0, this.logStats.errors - this.errorsSeen);
  }

  private addLogs(records: LogRecord[]) {
    const last = this.logs.length ? this.logs[this.logs.length - 1].seq : -1;
    const fresh = records.filter((r) => r.seq > last); // the snapshot may overlap the first batch
    if (!fresh.length) return;
    const errors = fresh.filter((r) => r.level === 'ERROR').length;
    if (errors) this.logStats = { ...this.logStats, errors: this.logStats.errors + errors };
    this.logs = [...this.logs, ...fresh].slice(-LOG_LIMIT);
  }

  // ---- derived ----

  get decks(): DeckView[] {
    return this.structure?.decks ?? [];
  }

  get viewDeck(): DeckView | null {
    const key = this.viewDeckKey;
    return (
      (key !== null ? this.decks.find((d) => deckKey(d) === key) : undefined) ??
      this.decks.find((d) => d.live) ??
      this.decks[0] ??
      null
    );
  }

  get selected(): SequenceView | null {
    const deck = this.viewDeck;
    if (!deck) return null;
    return deck.sequences.find((s) => s.sequence.id === this.selectedSeq) ?? null;
  }

  run(seq: Uuid | null | undefined): RunView | null {
    if (!seq) return null;
    return this.runtime?.running.find((r) => r.seq === seq) ?? null;
  }

  /** Clip of the deck being viewed, by id. */
  clip(id: number | null | undefined): ClipView | null {
    if (id == null) return null;
    for (const l of this.viewDeck?.layers ?? []) {
      const c = l.clips.find((c) => c.id === id);
      if (c) return c;
    }
    return null;
  }

  thumbUrl(c: ClipView | null): string | null {
    return c?.thumb ? `/thumbs/clip/${c.id}.png?v=${encodeURIComponent(c.thumb)}` : null;
  }

  /** Layers a lane may target in the viewed deck (not the SEQ layer, unique names). */
  get laneLayers() {
    return (this.viewDeck?.layers ?? []).filter((l) => !l.is_seq_layer);
  }

  // ---- actions ----

  /** ▶ on a card or Space: start, or stop when running. Switching decks asks first. */
  toggle(view: SequenceView, deck: DeckView) {
    const id = view.sequence.id;
    if (this.run(id)) {
      this.send({ cmd: 'stop', seq: id });
      return;
    }
    const running = this.runtime?.running ?? [];
    if (!deck.live && running.length > 0) {
      this.confirmSwitch = { seq: id, name: view.sequence.name, deck: deck.name, running: running.map((r) => r.name) };
      return;
    }
    this.send({ cmd: 'start', seq: id });
  }

  uniqueName(deck: DeckView, base: string): string {
    const taken = new Set(deck.sequences.map((s) => s.sequence.name));
    if (!taken.has(base)) return base;
    for (let i = 2; ; i++) if (!taken.has(`${base} ${i}`)) return `${base} ${i}`;
  }

  /** First eligible layer not used by the given lanes. */
  freeLayer(used: string[]): string | null {
    const names = this.laneLayers.filter((l) => !l.ambiguous).map((l) => l.name);
    return names.find((n) => !used.includes(n)) ?? null;
  }

  newSequence() {
    const deck = this.viewDeck;
    if (!deck) return;
    const id = crypto.randomUUID();
    this.edit({
      op: 'create_sequence',
      deck: deck.name,
      id,
      name: this.uniqueName(deck, 'New sequence'),
      layer: this.freeLayer([]) ?? 'Layer',
    });
    this.selectedSeq = id;
    this.selectedStep = null;
  }

  clipRef(c: ClipView): ClipRef {
    const r: ClipRef = { id: c.id };
    if (c.name) r.name = c.name;
    if (c.file) r.file = c.file;
    return r;
  }
}

export const app = new AppState();

/**
 * Layer label as Resolume shows it. Unnamed layers all report the raw name "Layer #" through the
 * API; Resolume displays the "#" as the layer number. Lanes cannot target them (SPEC §4.4).
 */
export function layerLabel(l: { name: string; index: number; ambiguous: boolean }): string {
  if (l.name.includes('#')) return `${l.name.replace('#', String(l.index))} · unnamed, rename in Resolume to use it`;
  if (l.ambiguous) return `${l.name} · name used twice, rename in Resolume`;
  return l.name;
}

/** Length in sixteenths as whole bars, else eighths, else sixteenths: `2 bars`, `3/8`, `5/16`. */
export function formatLength(sixteenths: number): string {
  const bars = sixteenths / SIXTEENTHS_PER_BAR;
  if (sixteenths % SIXTEENTHS_PER_BAR === 0) return `${bars} ${bars === 1 ? 'bar' : 'bars'}`;
  if (sixteenths % 2 === 0) return `${sixteenths / 2}/8`;
  return `${sixteenths}/16`;
}

export function repeatLabel(r: SequenceView['sequence']['repeat']): string {
  return r.mode === 'once' ? 'once' : r.mode === 'infinite' ? '∞' : `×${r.count}`;
}

// The desktop shell's View > Toggle Console menu item calls this (it has no IPC into the page).
(window as unknown as { __seqToggleConsole: () => void }).__seqToggleConsole = () =>
  app.setTab(app.tab === 'console' ? 'sequencer' : 'console');

// The desktop shell's File > Preferences menu item calls this.
(window as unknown as { __seqOpenPrefs: () => void }).__seqOpenPrefs = () => (app.prefsOpen = true);

// The desktop shell's Help menu items (About, Licenses, Keyboard shortcuts) call this.
(window as unknown as { __seqOpenDialog: (d: InfoDialog) => void }).__seqOpenDialog = (d) => app.openDialog(d);
