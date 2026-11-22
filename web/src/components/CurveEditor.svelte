<script lang="ts">
  // RGB curves (SPEC §14): master first, then each channel. Click adds a point, drag moves it, double-click
  // removes it (at least two stay). The curve is a monotone spline (the same as lut-core), so it never folds back.
  import type { Curve, Curves } from '../lib/types';
  import { curveFn, defaultCurve } from '../lib/lut';

  let { curves, onchange }: { curves: Curves; onchange: (c: Curves) => void } = $props();

  type Ch = keyof Curves;
  const CHANNELS: { id: Ch; label: string; color: string }[] = [
    { id: 'master', label: 'Master', color: 'var(--text)' },
    { id: 'r', label: 'R', color: '#ff5a5a' },
    { id: 'g', label: 'G', color: '#55d66b' },
    { id: 'b', label: 'B', color: '#5a8cff' },
  ];
  let ch = $state<Ch>('master');
  const S = 256; // SVG units
  let svg: SVGSVGElement | undefined = $state();
  let drag = $state<number | null>(null);

  const path = (c: Curve) => {
    const f = curveFn(c);
    let d = '';
    for (let i = 0; i <= 64; i++) {
      const x = i / 64;
      d += `${i ? 'L' : 'M'}${(x * S).toFixed(1)},${((1 - f(x)) * S).toFixed(1)}`;
    }
    return d;
  };

  function at(e: PointerEvent | MouseEvent): [number, number] {
    const r = svg!.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    const y = Math.min(1, Math.max(0, 1 - (e.clientY - r.top) / r.height));
    return [x, y];
  }

  function setPoints(points: [number, number][]) {
    onchange({ ...curves, [ch]: { points } });
  }

  function sorted(): [number, number][] {
    return [...curves[ch].points].sort((a, b) => a[0] - b[0]) as [number, number][];
  }

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    const [x, y] = at(e);
    const pts = sorted();
    // Grab a point within ~8 px, else add one.
    const r = svg!.getBoundingClientRect();
    const near = pts.findIndex(([px, py]) => Math.hypot((px - x) * r.width, (py - y) * r.height) < 9);
    if (near >= 0) {
      drag = near;
    } else {
      pts.push([x, y]);
      pts.sort((a, b) => a[0] - b[0]);
      drag = pts.findIndex((p) => p[0] === x && p[1] === y);
      setPoints(pts);
    }
    svg!.setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    if (drag === null) return;
    const pts = sorted();
    const [x, y] = at(e);
    const lo = drag > 0 ? pts[drag - 1][0] + 0.01 : 0;
    const hi = drag < pts.length - 1 ? pts[drag + 1][0] - 0.01 : 1;
    pts[drag] = [Math.min(hi, Math.max(lo, x)), y];
    setPoints(pts);
  }

  function up() {
    drag = null;
  }

  function remove(e: MouseEvent) {
    const [x, y] = at(e);
    const pts = sorted();
    if (pts.length <= 2) return;
    const r = svg!.getBoundingClientRect();
    const i = pts.findIndex(([px, py]) => Math.hypot((px - x) * r.width, (py - y) * r.height) < 9);
    if (i >= 0) {
      pts.splice(i, 1);
      setPoints(pts);
    }
  }

  let active = $derived(CHANNELS.find((c) => c.id === ch)!);
</script>

<div class="curves">
  <div class="row-between">
    <div class="mode-switch" role="group" aria-label="Curve">
      {#each CHANNELS as c (c.id)}
        <button aria-pressed={ch === c.id} onclick={() => (ch = c.id)}>{c.label}</button>
      {/each}
    </div>
    <button class="x" title="Reset this curve" onclick={() => onchange({ ...curves, [ch]: defaultCurve() })}>Reset</button>
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <svg bind:this={svg} viewBox="0 0 {S} {S}" class="curve-box" role="img" aria-label="{active.label} curve"
    onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} ondblclick={remove}>
    {#each [1, 2, 3] as i (i)}
      <line x1={(i * S) / 4} y1="0" x2={(i * S) / 4} y2={S} class="grid-line" />
      <line x1="0" y1={(i * S) / 4} x2={S} y2={(i * S) / 4} class="grid-line" />
    {/each}
    <line x1="0" y1={S} x2={S} y2="0" class="diag" />
    {#each CHANNELS.filter((c) => c.id !== ch) as c (c.id)}
      <path d={path(curves[c.id])} fill="none" stroke={c.color} stroke-width="1" opacity="0.3" />
    {/each}
    <path d={path(curves[ch])} fill="none" stroke={active.color} stroke-width="2" />
    {#each curves[ch].points as [x, y], i (i)}
      <circle cx={x * S} cy={(1 - y) * S} r="5" class="pt" style:stroke={active.color} />
    {/each}
  </svg>
  <p class="meta">Click to add a point · drag to move · double-click to remove</p>
</div>

<style>
  .curves { display: flex; flex-direction: column; gap: 6px; }
  .curve-box { width: 100%; aspect-ratio: 1; background: var(--track); border: 1px solid var(--line-strong); border-radius: var(--radius); touch-action: none; cursor: crosshair; }
  .grid-line { stroke: var(--tick); stroke-width: 1; }
  .diag { stroke: var(--tick-strong); stroke-width: 1; stroke-dasharray: 3 3; }
  .pt { fill: var(--bg); stroke-width: 2; }
  .meta { margin: 0; }
</style>
