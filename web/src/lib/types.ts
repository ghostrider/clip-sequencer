// Mirrors crates/seq-app/src/engine/view.rs and the seq-core model (serde JSON shapes).

export type Uuid = string;

export interface ClipRef {
  id?: number;
  name?: string;
  file?: string;
}

export interface Step {
  clip: ClipRef | null;
  length: number;
}

export type LaneEnd = 'loop_to_end' | 'hold' | 'clear';
export type AfterLastPass = 'clear' | 'hold';
export type Repeat = { mode: 'once' } | { mode: 'count'; count: number } | { mode: 'infinite' };

export interface Lane {
  layer: string;
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
  state: 'rest' | 'ok' | 'unresolved';
  clip_id: number | null;
  by: 'id' | 'name' | 'file' | null;
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
  pass_eighths: number;
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
  clips_source: 'live' | 'cached' | 'none';
  layers: LayerView[];
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
  | { op: 'insert_step'; seq: Uuid; lane: number; index: number; step: Step }
  | { op: 'remove_step'; seq: Uuid; lane: number; index: number }
  | { op: 'set_step_length'; seq: Uuid; lane: number; index: number; length: number }
  | { op: 'move_step'; seq: Uuid; lane: number; from: number; to: number }
  | { op: 'set_step_clip'; seq: Uuid; lane: number; index: number; clip: ClipRef | null };

export type Command =
  | { cmd: 'start'; seq: Uuid }
  | { cmd: 'stop'; seq: Uuid }
  | { cmd: 'stop_all' }
  | { cmd: 'resume_lane'; seq: Uuid; lane: number }
  | { cmd: 'edit'; op: EditOp }
  | { cmd: 'undo' }
  | { cmd: 'redo' }
  | { cmd: 'push_thumbnail'; seq: Uuid };
