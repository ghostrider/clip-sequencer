// Transitions per step in the editor (SPEC §7.7, §9.3): which transition a step gets, its label and
// its warnings. Lengths are sixteenths; the UI shows them in beats and bars.

import type { ClipView, Lane, LayerView, Step, Transition } from './types';

/** Resolume's longest layer transition, in seconds. */
export const MAX_FADE_SECONDS = 10;

/** Lengths offered in the inspector and the lane chip (sixteenths): ¼ beat … 4 bars. */
export const FADE_LENGTHS = [0, 1, 2, 4, 8, 12, 16, 32, 48, 64];

/** The transition a step shows: its own, the lane's (inherited), or none (original / Resolume's own). */
export interface StepFade {
  t: Transition;
  own: boolean;
}

export function stepFade(lane: Lane, step: Step): StepFade | null {
  if (step.transition) return { t: step.transition, own: true };
  if (lane.transition) return { t: lane.transition, own: false };
  return null;
}

/** `¼ beat`, `1 beat`, `2 beats`, `1 bar`, `6 bars`; `cut` for 0. `short`: just the number (`↗ 1`). */
export function fadeLength(sixteenths: number, short = false): string {
  if (sixteenths === 0) return 'cut';
  if (sixteenths % 16 === 0 && sixteenths >= 16) {
    const bars = sixteenths / 16;
    return short ? `${bars} bar${bars === 1 ? '' : 's'}` : `${bars} ${bars === 1 ? 'bar' : 'bars'}`;
  }
  const beats = sixteenths / 4;
  const whole = Math.floor(beats);
  const frac = ['', '¼', '½', '¾'][sixteenths % 4];
  const n = whole === 0 ? frac : `${whole}${frac}`;
  if (short) return n;
  return `${n} ${beats > 1 ? 'beats' : 'beat'}`;
}

/** The label in a step's length line: `↗ 1 beat · Alpha`, `| cut`, in narrow blocks `↗ 1`. */
export function fadeLabel(t: Transition, narrow: boolean): string {
  if (t.length === 0) return '| cut';
  if (narrow) return `↗ ${fadeLength(t.length, true)}`;
  return `↗ ${fadeLength(t.length)}${t.blend ? ` · ${t.blend}` : ''}`;
}

/** The fade's length in seconds at `bpm`. */
export function fadeSeconds(sixteenths: number, bpm: number): number {
  return bpm > 0 ? (sixteenths * 15) / bpm : 0;
}

/**
 * What is wrong with a step's transition (SPEC §7.7): capped at 10 s at the current tempo, a blend mode
 * the layer doesn't have, a clip with its own transition in Resolume. Empty when nothing is.
 */
export function fadeWarnings(t: Transition, layer: LayerView | undefined, clip: ClipView | null, bpm: number | null): string[] {
  const out: string[] = [];
  if (bpm && t.length > 0) {
    const s = fadeSeconds(t.length, bpm);
    if (s > MAX_FADE_SECONDS) out.push(`${s.toFixed(1)} s at ${Math.round(bpm)} BPM: Resolume fades at most 10 s, so it is capped`);
  }
  if (t.blend && layer && layer.blend_options.length > 0 && !layer.blend_options.includes(t.blend)) {
    out.push(`Resolume has no blend mode "${t.blend}" on this layer: only the length is set`);
  }
  if (clip && !clip.layer_determined) {
    out.push('this clip has its own transition in Resolume (not "Layer determined"): it ignores the layer\'s');
  }
  return out;
}
