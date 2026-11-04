// Mirrors crates/seq-app/src/engine/view.rs and the seq-core model (serde JSON shapes).

export type Uuid = string;

export interface ClipRef {
  id?: number;
  name?: string;
  file?: string;
}

/** Clip lanes use `clip`, slot lanes `column` (1-based, SPEC §4.5); `null` is a rest. */
export interface Step {
  clip?: ClipRef | null;
  column?: number | null;
  length: number;
}

export type LaneMode = 'clip' | 'slot';

export type LaneEnd = 'loop_to_end' | 'hold' | 'clear';
export type AfterLastPass = 'clear' | 'hold';
export type Repeat = { mode: 'once' } | { mode: 'count'; count: number } | { mode: 'infinite' };

export interface Lane {
  layer: string;
  /** Omitted for clip lanes. */
  mode?: LaneMode;
  end: LaneEnd;
  steps: Step[];
}

export interface Sequence {
  id: Uuid;
  name: string;
  start: 'immediate';
  repeat: Repeat;
  after_last_pass: AfterLastPass;
  lanes: Lane[];
}

export interface Methods {
  id: boolean | null;
  name: boolean | null;
  file: boolean | null;
}

export interface StepResolution {
  /** Slot steps: `slot` (the slot holds `clip_id`) or `empty` (plays as a rest). */
  state: 'rest' | 'ok' | 'unresolved' | 'slot' | 'empty';
  clip_id: number | null;
  by: 'id' | 'name' | 'file' | 'slot' | null;
  ambiguous: boolean;
  other_layer: boolean;
  methods: Methods;
}

export interface LaneResolution {
  layer: 'ok' | 'missing' | 'ambiguous' | 'seq_layer';
  steps: StepResolution[];
}

export interface ResolutionView {
  complete: boolean;
  lanes: LaneResolution[];
}

export interface SequenceView {
  sequence: Sequence;
  /** Length of one pass in sixteenths. */
  pass_sixteenths: number;
  binding: { column: number; clip_id: number } | null;
  resolution: ResolutionView | null;
  mosaic_key: string;
}

export interface ClipView {
  id: number;
  column: number;
  name: string;
  file: string | null;
  thumb: string | null;
}

export interface LayerView {
  id: number;
  index: number;
  name: string;
  ambiguous: boolean;
  is_seq_layer: boolean;
  clips: ClipView[];
}

export interface DeckView {
  name: string;
  index: number | null;
  live: boolean;
  orphaned: boolean;
  /** Several Resolume decks have this name; they share one set of sequences. */
  ambiguous: boolean;
  clips_source: 'live' | 'cached' | 'none';
  layers: LayerView[];
  /** Columns in the deck's clip data (0 without clip data). */
  columns: number;
  sequences: SequenceView[];
  unmatched_seq_clips: string[];
}

export interface StructureState {
  composition: string | null;
  live_deck: string | null;
  decks: DeckView[];
  model_error: string | null;
  can_undo: boolean;
  can_redo: boolean;
  seq_layer: string;
  seq_prefix: string;
}

export interface RunLaneView {
  state: 'idle' | 'playing' | 'yielded' | 'resume_pending' | 'taken_over';
  step: number | null;
  not_confirmed: number[];
}

export interface RunView {
  seq: Uuid;
  name: string;
  deck: string | null;
  from_seq_clip: boolean;
  pass: number;
  passes: number | null;
  pos: number;
  pass_ticks: number;
  bar_beat: string;
  lanes: RunLaneView[];
  edited: boolean;
}

export interface RuntimeState {
  clock: { port: string | null; connected: boolean; running: boolean; bpm: number | null };
  ws: boolean;
  ws_synced: boolean;
  rest: boolean;
  osc: boolean;
  running: RunView[];
  switching: { seq: Uuid; deck: string } | null;
  last_osc: { addr: string; send_us: number } | null;
  last_confirm: string | null;
  confirmations: number;
  notices: string[];
  saved: number;
  save_error: string | null;
}

// Edit operations (seq-core/src/edit.rs, tagged by "op").
export type EditOp =
  | { op: 'create_sequence'; deck: string; id: Uuid; name: string; layer: string }
  | { op: 'rename_sequence'; seq: Uuid; name: string }
  | { op: 'duplicate_sequence'; seq: Uuid; id: Uuid; name: string }
  | { op: 'delete_sequence'; seq: Uuid }
  | { op: 'move_sequence'; seq: Uuid; deck: string }
  | { op: 'set_repeat'; seq: Uuid; repeat: Repeat }
  | { op: 'set_after_last_pass'; seq: Uuid; value: AfterLastPass }
  | { op: 'add_lane'; seq: Uuid; layer: string }
  | { op: 'remove_lane'; seq: Uuid; lane: number }
  | { op: 'move_lane'; seq: Uuid; from: number; to: number }
  | { op: 'set_lane_layer'; seq: Uuid; lane: number; layer: string }
  | { op: 'set_lane_end'; seq: Uuid; lane: number; end: LaneEnd }
  | { op: 'set_lane_mode'; seq: Uuid; lane: number; mode: LaneMode }
  | { op: 'insert_step'; seq: Uuid; lane: number; index: number; step: Step }
  | { op: 'remove_step'; seq: Uuid; lane: number; index: number }
  | { op: 'set_step_length'; seq: Uuid; lane: number; index: number; length: number }
  | { op: 'move_step'; seq: Uuid; lane: number; from: number; to: number }
  | { op: 'set_step_clip'; seq: Uuid; lane: number; index: number; clip: ClipRef | null }
  | { op: 'set_step_column'; seq: Uuid; lane: number; index: number; column: number | null };

export type Command =
  | { cmd: 'start'; seq: Uuid }
  | { cmd: 'stop'; seq: Uuid }
  | { cmd: 'stop_all' }
  | { cmd: 'resume_lane'; seq: Uuid; lane: number }
  | { cmd: 'edit'; op: EditOp }
  | { cmd: 'undo' }
  | { cmd: 'redo' }
  | { cmd: 'reload_composition' }
  | { cmd: 'save_now' }
  | { cmd: 'push_thumbnail'; seq: Uuid }
  | { cmd: 'shell'; action: ShellAction }
  | { cmd: 'log_subscribe' }
  | { cmd: 'log_unsubscribe' }
  | { cmd: 'set_midi_port'; port: string | null };

/** Window actions for the desktop shell (ignored in a browser). */
export type ShellAction = 'quit' | 'zoom_in' | 'zoom_out' | 'zoom_reset' | 'open_logs' | 'open_ffgl_dir';

/** One log record (Console tab, M12 E). */
export interface LogRecord {
  seq: number;
  ts: string;
  level: 'ERROR' | 'WARN' | 'INFO' | 'DEBUG' | 'TRACE';
  target: string;
  message: string;
}

export interface LogStats {
  errors: number;
  dropped: number;
}

/** `GET /api/version`. */
export interface VersionInfo {
  app: string;
  name: string;
  version: string;
  build: string;
  commit: string;
  /** UTC build time, ISO 8601. */
  build_date: string;
  /** Target triple, e.g. `x86_64-pc-windows-msvc`. */
  target: string;
  /** `Copyright (c) 2026 …` (same as LICENSE). */
  copyright: string;
  /** GitHub repository URL (Cargo `repository`). */
  repository: string;
  /** Project site URL (Cargo `homepage`); the manual is under `manual/`. */
  homepage: string;
  ui_port: number;
  desktop: boolean;
}

/** One third-party component in the notices (`THIRD_PARTY_LICENSES.json`, scripts/third-party-licenses.mjs). */
export interface NoticePackage {
  name: string;
  version: string;
  license: string;
  part: string;
  url: string;
  /** Indexes into `Notices.texts`. */
  texts: number[];
}

export interface Notices {
  /** Set when the notices were not generated for this build (dev builds). */
  placeholder?: boolean;
  note?: string;
  packages: NoticePackage[];
  texts: string[];
}

/** `GET /api/licenses`. */
export interface LicensesInfo {
  /** The project's MIT license text. */
  license: string;
  notices: Notices;
}

/** `FILEVERSION` of a plugin DLL. */
export interface DllVersion {
  major: number;
  minor: number;
  patch: number;
  build: number;
}

/** `GET /api/ffgl`: the bundled FFGL plugin vs. the one in Resolume's plugin folder (SPEC §12.1). */
export interface FfglStatus {
  plugin_name: string;
  bundled_path: string | null;
  bundled_version: DllVersion | null;
  dir: string | null;
  dir_is_default: boolean;
  dir_exists: boolean;
  dir_source: 'command_line' | 'saved' | 'default';
  installed: { version: DllVersion | null; ours: boolean } | null;
  action: 'install' | 'update' | 'reinstall' | 'downgrade' | null;
  resolume_running: boolean;
  can_detect_resolume: boolean;
}

/** Result of `POST /api/ffgl/install`, `/remove`, `/dir`. */
export interface FfglResult {
  ok: boolean;
  message?: string;
  error?: string;
  status: FfglStatus;
}
