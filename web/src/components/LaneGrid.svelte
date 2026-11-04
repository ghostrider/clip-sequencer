<script lang="ts">
  import { app, SIXTEENTHS_PER_BAR, TICKS_PER_SIXTEENTH, LENGTH_STEP, formatLength, layerLabel, isRest, isSlotLane } from '../lib/app.svelte';
  import type { DeckView, SequenceView, Step, LaneEnd, LaneMode, StepResolution } from '../lib/types';

  let { view, deck }: { view: SequenceView; deck: DeckView } = $props();

  let seq = $derived(view.sequence);
  let run = $derived(app.run(seq.id));
  let zoom = $derived(app.zoom);

  // Local previews while dragging; the edit is sent on release.
  // `at`: the step's start in sixteenths, so its end (not its length) snaps to the grid.
  let resize = $state<{ lane: number; index: number; at: number; startX: number; startLen: number; len: number } | null>(null);
  let drag = $state<{ lane: number; from: number; to: number; startX: number; active: boolean; alt: boolean } | null>(null);
  let dropAt = $state<{ lane: number; index: number } | null>(null);

  function len(lane: number, index: number, step: Step) {
    return resize && resize.lane === lane && resize.index === index ? resize.len : step.length;
  }

  let totalSixteenths = $derived.by(() => {
    const longest = Math.max(0, ...seq.lanes.map((l, li) => l.steps.reduce((a, s, i) => a + len(li, i, s), 0)));
    const bars = Math.max(4, Math.ceil(longest / SIXTEENTHS_PER_BAR) + 1);
    return bars * SIXTEENTHS_PER_BAR;
  });
  let bars = $derived(totalSixteenths / SIXTEENTHS_PER_BAR);

  function stepLabel(step: Step, r: StepResolution | undefined): string {
    const c = app.clip(r?.clip_id);
    if (step.column != null) {
      // Slot lane step (SPEC §4.5): the column and what it holds now.
      if (r?.state === 'empty') return `col ${step.column} · empty`;
      return c?.name ? `col ${step.column} · ${c.name}` : `col ${step.column}`;
    }
    if (!step.clip) return 'rest';
    return c?.name || step.clip.name || step.clip.file || `clip ${step.clip.id}`;
  }

  /** Index at which a drop/move at clientX lands in a lane. */
  function indexAt(laneEl: HTMLElement, clientX: number, steps: Step[], lane: number): number {
    const x = clientX - laneEl.getBoundingClientRect().left;
    let acc = 0;
    for (let i = 0; i < steps.length; i++) {
      const w = len(lane, i, steps[i]) * zoom;
      if (x < acc + w / 2) return i;
      acc += w;
    }
    return steps.length;
  }

  // ---- drop from the clip browser ----
  function onDragOver(e: DragEvent, lane: number) {
    if (!e.dataTransfer?.types.includes('application/x-seq-clip')) return;
    // Slot lanes take the clip's column (SPEC §9.3); generator/placeholder rows set it too.
    if (isSlotLane(seq.lanes[lane]) && !e.dataTransfer.types.includes('application/x-seq-column')) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
    dropAt = { lane, index: indexAt(e.currentTarget as HTMLElement, e.clientX, seq.lanes[lane].steps, lane) };
  }
  function onDrop(e: DragEvent, lane: number) {
    const data = e.dataTransfer?.getData('application/x-seq-clip');
    dropAt = null;
    if (!data) return;
    e.preventDefault();
    const index = indexAt(e.currentTarget as HTMLElement, e.clientX, seq.lanes[lane].steps, lane);
    const step: Step = isSlotLane(seq.lanes[lane])
      ? { column: Number(e.dataTransfer?.getData('application/x-seq-column')), length: SIXTEENTHS_PER_BAR }
      : { clip: JSON.parse(data), length: SIXTEENTHS_PER_BAR };
    app.edit({ op: 'insert_step', seq: seq.id, lane, index, step });
    app.selectedStep = { lane, index };
  }

  // ---- resize by the right edge ----
  function resizeDown(e: PointerEvent, lane: number, index: number, step: Step) {
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const at = seq.lanes[lane].steps.slice(0, index).reduce((a, s) => a + s.length, 0);
    resize = { lane, index, at, startX: e.clientX, startLen: step.length, len: step.length };
  }
  function resizeMove(e: PointerEvent) {
    if (!resize) return;
    // The step's end snaps to the 1/8 grid, or to 1/16 with Shift held (SPEC §9.3).
    const unit = e.shiftKey ? 1 : LENGTH_STEP;
    const end = resize.at + resize.startLen + (e.clientX - resize.startX) / zoom;
    const snapped = Math.round(end / unit) * unit;
    resize.len = Math.max(1, snapped - resize.at);
  }
  function resizeUp() {
    if (!resize) return;
    const r = resize;
    resize = null;
    if (r.len !== r.startLen) app.edit({ op: 'set_step_length', seq: seq.id, lane: r.lane, index: r.index, length: r.len });
  }

  // ---- reorder / Alt-duplicate ----
  function stepDown(e: PointerEvent, lane: number, index: number) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { lane, from: index, to: index, startX: e.clientX, active: false, alt: e.altKey };
  }
  function stepMove(e: PointerEvent) {
    if (!drag) return;
    if (!drag.active && Math.abs(e.clientX - drag.startX) < 4) return;
    drag.active = true;
    drag.alt = e.altKey;
    const laneEl = (e.currentTarget as HTMLElement).closest('.lane') as HTMLElement;
    const steps = seq.lanes[drag.lane].steps;
    let i = indexAt(laneEl, e.clientX, steps, drag.lane);
    // Moving right: the gap index counts the dragged step itself.
    if (!drag.alt && i > drag.from) i -= 1;
    drag.to = Math.min(i, drag.alt ? steps.length : steps.length - 1);
  }
  function stepUp(lane: number, index: number) {
    const d = drag;
    drag = null;
    if (!d || !d.active) {
      app.selectedStep = { lane, index };
      return;
    }
    const steps = seq.lanes[d.lane].steps;
    if (d.alt) {
      app.edit({ op: 'insert_step', seq: seq.id, lane: d.lane, index: d.to, step: structuredClone($state.snapshot(steps[d.from])) });
      app.selectedStep = { lane: d.lane, index: d.to };
    } else if (d.to !== d.from) {
      app.edit({ op: 'move_step', seq: seq.id, lane: d.lane, from: d.from, to: d.to });
      app.selectedStep = { lane: d.lane, index: d.to };
    }
  }

  function onWheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    e.preventDefault();
    app.zoom = Math.min(30, Math.max(2, Math.round(app.zoom * (e.deltaY < 0 ? 1.15 : 1 / 1.15))));
  }

  function moveLane(from: number, to: number) {
    app.edit({ op: 'move_lane', seq: seq.id, from, to });
    // Keep the selected step on the lane it belongs to.
    const sel = app.selectedStep;
    if (sel?.lane === from) app.selectedStep = { lane: to, index: sel.index };
    else if (sel?.lane === to) app.selectedStep = { lane: from, index: sel.index };
  }

  function laneState(li: number): string {
    return run?.lanes[li]?.state ?? 'idle';
  }

  function setMode(lane: number, mode: LaneMode) {
    if ((seq.lanes[lane].mode ?? 'clip') !== mode) app.edit({ op: 'set_lane_mode', seq: seq.id, lane, mode });
  }

  const ends: { value: LaneEnd; label: string }[] = [
    { value: 'loop_to_end', label: 'Loop to end' },
    { value: 'hold', label: 'Hold' },
    { value: 'clear', label: 'Clear' },
  ];
</script>

<div class="grid" onwheel={onWheel}>
  <div class="grid-inner" style:min-width={`calc(var(--lane-head-w) + ${totalSixteenths * zoom}px)`}>
    <div class="lanehead-col">
      <div class="ruler-spacer">bar.beat</div>
      {#each seq.lanes as lane, li (li)}
        {@const res = view.resolution?.lanes[li]}
        {@const state = laneState(li)}
        <div class="lanehead" class:bad={res && res.layer !== 'ok'}>
          <div class="row-between">
            <select class="layer-select mono" value={lane.layer} title={res?.layer === 'ok' ? 'layer' : `layer ${res?.layer}`}
              onchange={(e) => app.edit({ op: 'set_lane_layer', seq: seq.id, lane: li, layer: (e.currentTarget as HTMLSelectElement).value })}>
              {#if !app.laneLayers.some((l) => l.name === lane.layer)}
                <option value={lane.layer}>{lane.layer} (missing)</option>
              {/if}
              {#each app.laneLayers as l (l.id)}
                <option value={l.name} disabled={l.ambiguous && l.name !== lane.layer}>
                  {l.ambiguous || l.name.includes('#') ? layerLabel(l) : l.name}
                </option>
              {/each}
            </select>
            <span class="lane-tools">
              <button class="x" aria-label="Move lane up" title="Move lane up" disabled={li === 0}
                onclick={() => moveLane(li, li - 1)}>▲</button>
              <button class="x" aria-label="Move lane down" title="Move lane down" disabled={li === seq.lanes.length - 1}
                onclick={() => moveLane(li, li + 1)}>▼</button>
            <button class="x" aria-label="Delete lane" title={seq.lanes.length === 1 ? 'A sequence needs at least one lane' : 'Delete lane'}
              disabled={seq.lanes.length === 1} onclick={() => app.edit({ op: 'remove_lane', seq: seq.id, lane: li })}>✕</button>
            </span>
          </div>
          <div style="display:flex;gap:4px;flex-wrap:wrap;align-items:center">
            <div class="mode-switch" role="group" aria-label="Lane mode"
              title="Clip: steps follow their clips. Slot: steps play whatever is in a column of this layer.">
              <button aria-pressed={!isSlotLane(lane)} onclick={() => setMode(li, 'clip')}>Clip</button>
              <button aria-pressed={isSlotLane(lane)} onclick={() => setMode(li, 'slot')}>Slot</button>
            </div>
            <select class="pill-select" value={lane.end}
              onchange={(e) => app.edit({ op: 'set_lane_end', seq: seq.id, lane: li, end: (e.currentTarget as HTMLSelectElement).value as LaneEnd })}>
              {#each ends as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
            </select>
            <span class="pill" class:playing={state === 'playing'} class:yielded={state === 'yielded' || state === 'resume_pending'}>
              {state === 'resume_pending' ? 'resuming' : state === 'taken_over' ? 'taken over' : state}
            </span>
            {#if state === 'yielded'}
              <button class="btn ghost" onclick={() => app.send({ cmd: 'resume_lane', seq: seq.id, lane: li })}>Resume lane</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>

    <div class="tracks">
      <div class="ruler">
        {#each Array(bars) as _, b (b)}
          <div class="bar" style:flex={`0 0 ${SIXTEENTHS_PER_BAR * zoom}px`}>
            <div class="num">{b + 1}</div>
            <div class="ticks">{#each Array(SIXTEENTHS_PER_BAR / LENGTH_STEP) as _, t (t)}<i></i>{/each}</div>
          </div>
        {/each}
      </div>
      {#each seq.lanes as lane, li (li)}
        {@const res = view.resolution?.lanes[li]}
        {@const rl = run?.lanes[li]}
        <div class="lane" role="list" ondragover={(e) => onDragOver(e, li)} ondragleave={() => (dropAt = null)} ondrop={(e) => onDrop(e, li)}>
          {#each lane.steps as step, si (si)}
            {@const r = res?.steps[si]}
            {@const clip = app.clip(r?.clip_id)}
            {@const thumb = app.thumbUrl(clip)}
            {@const rest = isRest(step) || r?.state === 'empty'}
            {@const unresolved = r?.state === 'unresolved'}
            {@const playing = rl?.state === 'playing' && rl.step === si}
            {@const notConfirmed = rl?.not_confirmed.includes(si)}
            {@const selected = app.selectedStep?.lane === li && app.selectedStep?.index === si}
            {@const dragged = drag?.active && drag.lane === li && drag.from === si}
            <div class="slot" role="listitem" style:width={`${len(li, si, step) * zoom}px`}
              class:drop-before={(dropAt?.lane === li && dropAt.index === si) || (drag?.active && drag.lane === li && drag.to === si && drag.from !== si && (drag.alt || drag.to < drag.from))}
              class:drop-after={drag?.active && !drag.alt && drag.lane === li && drag.to === si && drag.to > drag.from}>
              <div
                class="step"
                class:rest
                class:slot-step={step.column != null}
                class:unresolved
                class:selected
                class:playing
                class:not-confirmed={notConfirmed}
                class:dragged
                class:warn={r?.ambiguous || r?.other_layer}
                style:background-image={thumb ? `linear-gradient(color-mix(in srgb, var(--bg) 55%, transparent), color-mix(in srgb, var(--bg) 55%, transparent)), url(${thumb})` : null}
                title={`${stepLabel(step, r)}${step.column != null && r?.state === 'empty' ? ' · plays as a rest' : ''}${r?.ambiguous ? ' · ambiguous match' : ''}${r?.other_layer ? ' · clip is on another layer' : ''}${notConfirmed ? ' · not confirmed by Resolume' : ''}`}
                role="button"
                tabindex="-1"
                onpointerdown={(e) => stepDown(e, li, si)}
                onpointermove={stepMove}
                onpointerup={() => stepUp(li, si)}
                onpointercancel={() => (drag = null)}
              >
                <div class="name">{stepLabel(step, r)}</div>
                <div class="len">{unresolved ? 'unresolved · ' : ''}{formatLength(len(li, si, step))}</div>
                <div class="resize" role="separator" aria-label="Resize step"
                  onpointerdown={(e) => resizeDown(e, li, si, step)} onpointermove={resizeMove} onpointerup={resizeUp}></div>
              </div>
            </div>
          {/each}
          {#if dropAt?.lane === li && dropAt.index === lane.steps.length}<div class="slot drop-before" style:width="8px"></div>{/if}
          {#if lane.steps.length === 0 && dropAt?.lane !== li}
            <div class="hint" style="align-self:center;padding:0 10px">
              {isSlotLane(lane) ? 'drop clips here: each step plays its column' : 'drop clips here'}
            </div>
          {/if}
        </div>
      {/each}
      {#if run}
        {@const x = (run.pos / TICKS_PER_SIXTEENTH) * zoom}
        <div class="playhead" style:left={`${x}px`}></div>
        <div class="playhead-label" style:left={`${x}px`}>{run.bar_beat}</div>
      {/if}
    </div>
  </div>
</div>
