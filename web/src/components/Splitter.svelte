<script lang="ts">
  // A vertical drag handle that resizes the panel next to it (Sequencer tab: sequence list, clip browser).
  // `side`: which panel it resizes, the one on its left or on its right. Double-click resets the width;
  // arrow keys move it in 16 px steps when it has focus.
  let {
    width = $bindable(),
    side,
    min,
    max,
    initial,
    label,
  }: { width: number; side: 'left' | 'right'; min: number; max: number; initial: number; label: string } = $props();

  let start: { x: number; w: number } | null = null;
  const clamp = (w: number) => Math.round(Math.min(max, Math.max(min, w)));

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    start = { x: e.clientX, w: width };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }
  function move(e: PointerEvent) {
    if (!start) return;
    const dx = e.clientX - start.x;
    width = clamp(start.w + (side === 'left' ? dx : -dx));
  }
  function up() {
    start = null;
  }
  function key(e: KeyboardEvent) {
    const step = e.key === 'ArrowLeft' ? -16 : e.key === 'ArrowRight' ? 16 : 0;
    if (!step) return;
    e.preventDefault();
    width = clamp(width + (side === 'left' ? step : -step));
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div class="splitter" role="separator" aria-orientation="vertical" aria-label={label} aria-valuenow={width}
  aria-valuemin={min} aria-valuemax={max} tabindex="0" title="Drag to resize · double-click resets"
  onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up}
  ondblclick={() => (width = initial)} onkeydown={key}></div>

<style>
  .splitter { flex: 0 0 5px; margin: 0 -2px; position: relative; z-index: 5; cursor: col-resize; touch-action: none; }
  .splitter::after { content: ""; position: absolute; top: 0; bottom: 0; left: 2px; width: 1px; background: transparent; transition: background .1s; }
  .splitter:hover::after, .splitter:focus-visible::after { background: var(--accent); width: 2px; left: 1px; }
  .splitter:focus-visible { outline: none; }
</style>
