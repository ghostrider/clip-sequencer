// Mirrors crates/seq-app/src/engine/view.rs and the seq-core model (serde JSON shapes).
import type { CustomTheme } from './themes';

export type Uuid = string;

export interface ClipRef {
  id?: number;
  name?: string;
  file?: string;
}

/** Format v4 (SPEC §6): how a clip comes in; `length` in sixteenths (0 = cut), `blend` a Resolume blend mode. */
export interface Transition {
  length: number;
  blend?: string;
}

/** Format v4: on which passes a step plays. */
export type When =
  | { is: 'first' }
  | { is: 'not_first' }
  | { is: 'last' }
  | { is: 'not_last' }
  | { is: 'cycle'; pass: number; of: number };

export type Pick = 'random' | 'cycle';

/** One more option of a step: a clip in clip lanes, a column in slot lanes, `null` a rest. */
export type Choice = ClipRef | number | null;

/**
 * Clip lanes use `clip`, slot lanes `column` (1-based, SPEC §4.5); `null` is a rest. The format v4 fields are
 * optional; `choices` (1–3) are options 2…, `pick` only with `choices` (absent = random).
 */
export interface Step {
  clip?: ClipRef | null;
  column?: number | null;
  length: number;
  when?: When;
  choices?: Choice[];
  pick?: Pick;
  transition?: Transition;
  /** Tied to the step before: continues its clip if it still plays (SPEC §6). */
  tie?: boolean;
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
  /** Format v4: default transition of the lane's steps. */
  transition?: Transition;
  steps: Step[];
}

/** Format v4 (SPEC §3.2): when a started sequence begins. `bars` ∈ 1, 2, 4, 8, 16. */
export type Start = { mode: 'immediate' } | { mode: 'beat' } | { mode: 'bar'; bars: number };

/** Format v4: a named marker, `at` in sixteenths from the start. */
export interface Section {
  at: number;
  name: string;
}

export interface Sequence {
  id: Uuid;
  name: string;
  start: Start;
  repeat: Repeat;
  after_last_pass: AfterLastPass;
  /** Sorted by `at`; omitted when empty. */
  sections?: Section[];
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
  /** Per step, its `choices` (options 2…) resolved one by one; empty without choices. */
  choices: StepResolution[][];
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
  /** Follows its layer's transition; false: it uses its own (SPEC §7.7). */
  layer_determined: boolean;
}

export interface LayerView {
  id: number;
  index: number;
  name: string;
  ambiguous: boolean;
  is_seq_layer: boolean;
  clips: ClipView[];
  /** The layer's transition blend modes; empty if unknown (SPEC §7.7). */
  blend_options: string[];
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
  /** What "Sync SEQ layer" would do (SPEC §7.6); only for the live deck. */
  seq_sync: SeqSyncPlan | null;
}

/** seq-core/src/seq_sync.rs */
export interface SeqSyncPlan {
  layer: { state: 'exists'; index: number } | { state: 'missing' } | { state: 'ambiguous'; count: number };
  /** Sequence names without a § clip. */
  add: string[];
  /** § clips without a sequence; playing ones are never cleared. */
  orphans: { name: string; clip_id: number; column: number; connected: boolean }[];
  /** [column, clip id] of empty slots on the SEQ layer. */
  empty_slots: [number, number][];
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
  /** Resolume's LUT effects: composition, layers, live deck's clips (SPEC §14). */
  lut_effects: LutEffect[];
}

// ---- LUTs (SPEC §14; crates/lut-core, crates/seq-app/src/luts.rs) ----

/** A colour wheel value: master plus R, G, B (lift adds them, gamma and gain multiply). */
export interface Wheel {
  master: number;
  r: number;
  g: number;
  b: number;
}

/** lut-core Grade: every default changes nothing. */
export interface Grade {
  exposure: number;
  temperature: number;
  tint: number;
  lift: Wheel;
  gamma: Wheel;
  gain: Wheel;
  contrast: number;
  saturation: number;
}

/** Control points [x, y] in 0..1, joined by a monotone spline. */
export interface Curve {
  points: [number, number][];
}

export interface Curves {
  master: Curve;
  r: Curve;
  g: Curve;
  b: Curve;
}

export interface LutView {
  name: string;
  /** Bumped on every change (the preview fetches the table then). */
  rev: number;
  grade: Grade;
  curves: Curves;
  /** An imported table is the base under the controls. */
  has_base: boolean;
  /** Full path of the .cube file. */
  path: string;
}

/** `luts` message. */
export interface LutsView {
  /** Folder of the .cube files. */
  dir: string;
  luts: LutView[];
}

export type LutPlace =
  | { kind: 'composition' }
  | { kind: 'layer'; layer: number; name: string }
  | { kind: 'clip'; layer: number; column: number; name: string };

/** One of Resolume's own LUT effects. */
export interface LutEffect {
  effect_id: number;
  /** The `LUT 3D` file parameter (set to point the effect at a file). */
  file_param: number;
  /** Full path of its .cube file; empty if none. */
  path: string;
  place: LutPlace;
}

export interface RunLaneView {
  state: 'idle' | 'playing' | 'yielded' | 'resume_pending' | 'taken_over';
  /** Mute and solo (SPEC §7.8); `silent`: sends nothing now (muted, or another lane is soloed). */
  mute: boolean;
  solo: boolean;
  silent: boolean;
  /** Option the current step played (0 = its own); `null` when its pass condition kept it silent. */
  option: number | null;
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

/** Where the clock comes from (SPEC §3, §3.1). */
export type ClockSource = 'midi' | 'resolume';

export interface ClockView {
  source: ClockSource;
  /** The MIDI port (kept while the source is Resolume). */
  port: string | null;
  /** MIDI: the port is open. Resolume: connected and a tempo controller is known. */
  connected: boolean;
  running: boolean;
  bpm: number | null;
  /** Resolume source: `Play`, `Pause` or `Stop`. */
  transport: string | null;
  /** Resolume source: in phase with Resolume's beat since the last Resync. */
  synced: boolean;
  /** The bar phase for quantized starts comes from a MIDI Start or Resync (SPEC §3.2). */
  bar_synced: boolean;
}

/** A start waiting for its beat or bar (SPEC §3.2). */
export interface PendingView {
  seq: Uuid;
  name: string;
  from_seq_clip: boolean;
  /** The start mode boundary it waits for; `null` when a cue waits for `end_of_pass`. */
  mode: Start | null;
  /** A cue waiting for the end of the current pass of this running sequence (SPEC §3.3). */
  end_of_pass: Uuid | null;
  /** A cue: what plays stops when it starts. */
  cue: boolean;
  /** Ticks until it starts (24 per beat). */
  wait_ticks: number;
}

/** Cue points (SPEC §3.3). */
export type CueAt = { at: 'start_mode' } | { at: 'bars_8' } | { at: 'bars_16' } | { at: 'end_of_pass'; of: Uuid };

export interface RuntimeState {
  clock: ClockView;
  ws: boolean;
  ws_synced: boolean;
  rest: boolean;
  osc: boolean;
  /** Triggers sent but not confirmed several times in a row: Resolume's OSC input is probably off. */
  osc_unanswered: boolean;
  running: RunView[];
  /** Starts waiting for their boundary, in the order they were made. */
  pending: PendingView[];
  switching: { seq: Uuid; deck: string } | null;
  last_osc: { addr: string; send_us: number } | null;
  last_confirm: string | null;
  confirmations: number;
  notices: string[];
  saved: number;
  save_error: string | null;
  /** "Sync SEQ layer" is running (SPEC §7.6). */
  seq_sync_busy: boolean;
  /** The armed sequence (SPEC §7.9). */
  record?: RecordView | null;
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
  | { op: 'set_step_column'; seq: Uuid; lane: number; index: number; column: number | null }
  // Format v4. Option positions: 0 = the step's own clip/column, 1… = its choices.
  | { op: 'set_start'; seq: Uuid; start: Start }
  | { op: 'set_lane_transition'; seq: Uuid; lane: number; transition: Transition | null }
  | { op: 'set_step_transition'; seq: Uuid; lane: number; index: number; transition: Transition | null }
  | { op: 'set_step_tie'; seq: Uuid; lane: number; index: number; tie: boolean }
  | { op: 'set_step_when'; seq: Uuid; lane: number; index: number; when: When | null }
  | { op: 'add_choice'; seq: Uuid; lane: number; index: number; choice: Choice }
  | { op: 'remove_choice'; seq: Uuid; lane: number; index: number; option: number }
  | { op: 'move_choice'; seq: Uuid; lane: number; index: number; from: number; to: number }
  | { op: 'set_choice'; seq: Uuid; lane: number; index: number; option: number; choice: Choice }
  | { op: 'set_pick'; seq: Uuid; lane: number; index: number; pick: Pick | null }
  | { op: 'add_section'; seq: Uuid; at: number; name: string }
  | { op: 'move_section'; seq: Uuid; section: number; at: number }
  | { op: 'rename_section'; seq: Uuid; section: number; name: string }
  | { op: 'delete_section'; seq: Uuid; section: number };

export type Command =
  | { cmd: 'start'; seq: Uuid }
  | ({ cmd: 'cue'; seq: Uuid } & CueAt)
  | { cmd: 'cancel_cue' }
  | { cmd: 'set_guide_lines'; every: number | null }
  | { cmd: 'set_debug_log'; on: boolean }
  | { cmd: 'check_update' }
  | { cmd: 'update_answer'; skip: boolean }
  | { cmd: 'install_update' }
  | { cmd: 'set_update_check'; on: boolean }
  | { cmd: 'set_armed'; seq: Uuid | null; bars?: number }
  | { cmd: 'set_lane_mute'; seq: Uuid; lane: number; on: boolean }
  | { cmd: 'set_lane_solo'; seq: Uuid; lane: number; on: boolean }
  | { cmd: 'stop'; seq: Uuid }
  | { cmd: 'stop_all' }
  | { cmd: 'resume_lane'; seq: Uuid; lane: number }
  | { cmd: 'edit'; op: EditOp }
  | { cmd: 'undo' }
  | { cmd: 'redo' }
  | { cmd: 'reload_composition' }
  | { cmd: 'save_now' }
  | { cmd: 'sync_seq_layer'; remove_orphans: boolean }
  | { cmd: 'push_thumbnail'; seq: Uuid }
  | { cmd: 'shell'; action: ShellAction }
  | { cmd: 'log_subscribe' }
  | { cmd: 'log_unsubscribe' }
  | { cmd: 'set_midi_port'; port: string | null }
  | { cmd: 'set_clock_source'; source: ClockSource }
  | { cmd: 'set_theme'; id: string | null }
  | { cmd: 'save_theme'; theme: CustomTheme; select: boolean }
  | { cmd: 'delete_theme'; id: string }
  | { cmd: 'lut_create'; name: string; from?: string | null }
  | { cmd: 'lut_rename'; from: string; to: string }
  | { cmd: 'lut_delete'; name: string }
  | { cmd: 'lut_update'; name: string; grade: Grade; curves: Curves }
  | { cmd: 'lut_clear_base'; name: string }
  | { cmd: 'lut_apply'; name: string; file_param: number };

/** In-app update (SPEC §15), `update` message. */
export interface Release {
  version: string;
  notes: string;
  page: string;
  zip_name: string;
  zip_url: string;
  zip_size: number;
  sums_url: string;
}

export type CheckResult =
  | { state: 'up_to_date'; current: string }
  | { state: 'available'; current: string; release: Release }
  | { state: 'error'; current: string; message: string };

export type UpdatePhase =
  | { state: 'downloading'; done: number; total: number }
  | { state: 'restarting' }
  | { state: 'failed'; message: string };

export interface UpdateView {
  current: string;
  checking: boolean;
  result: CheckResult | null;
  /** The start-up check found a version to offer: ask Download and install / Skip this version / Later. */
  ask: boolean;
  phase: UpdatePhase | null;
  /** Why this copy can't update itself (not a release folder, folder not writable). */
  blocked: string | null;
  startup_check: boolean;
  releases_page: string;
}

/** Theme settings from the app (`appearance` message, SPEC §9.7). */
export interface Appearance {
  /** Editor guide lines every 8, 16 or 32 bars, or `null` = off (SPEC §9.3). */
  guide_lines: number | null;
  /** Preferences → Log debug messages. */
  debug_log: boolean;
  /** Selected theme id; `null` = the default. */
  theme: string | null;
  custom_themes: CustomTheme[];
}

/** Window actions for the desktop shell (ignored in a browser). */
export type ShellAction =
  | 'quit' | 'zoom_in' | 'zoom_out' | 'zoom_reset' | 'open_logs' | 'open_ffgl_dir' | 'open_data_dir' | 'open_manual' | 'open_issue';

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

/** Recording from Resolume (SPEC §7.9). */
export interface RecordView {
  seq: Uuid;
  /** Its run goes and records; else armed, waiting for its start. */
  recording: boolean;
  pass: number;
  clicks: number;
}
