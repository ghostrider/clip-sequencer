<script lang="ts">
  import { app, SIXTEENTHS_PER_BAR, SNAP_SIXTEENTHS, TICKS_PER_SIXTEENTH, formatLength, layerLabel, isRest, isSlotLane, whenLabel, whenWarning } from '../lib/app.svelte';
  import type { DeckView, Lane, SequenceView, Step, LaneEnd, StepResolution } from '../lib/types';
  import { fadeLabel, fadeLength, fadeWarnings, stepFade } from '../lib/fade';

  let { view, deck }: { view: SequenceView; deck: DeckView } = $props();

  let seq = $derived(view.sequence);
  let run = $derived(app.run(seq.id));
  let zoom = $derived(app.zoom);

  // Local previews while dragging; the edit is sent on release.
  let resize = $state<{ lane: number; index: number; startX: number; startLen: number; len: number } | null>(null);
  let drag = $state<{ lane: number; from: number; to: number; startX: number; active: boolean; alt: boolean } | null>(null);
  let dropAt = $state<{ lane: number; index: number } | null>(null);

  function len(lane: number, index: number, step: Step) {
    return resize && resize.lane === lane && resize.index === index ? resize.len : step.length;
  }

  let totalSixteenths = $derived.by(() => {
    const longest = Math.max(0, ...seq.lanes.map((l, li) => l.steps.reduce((a, s, i) => a + len(li, i, s), 0)));
    // Sections beyond the end stay visible (SPEC §6: changing lanes never moves or deletes them).
    const lastSection = Math.max(0, ...(seq.sections ?? []).map((s) => s.at + 1));
    const bars = Math.max(4, Math.ceil(Math.max(longest, lastSection) / SIXTEENTHS_PER_BAR) + 1);
    return bars * SIXTEENTHS_PER_BAR;
  });

  // ---- section markers (SPEC §6 sections, §9.3) ----
  let sections = $derived(seq.sections ?? []);
  let guideEvery = $derived(app.appearance.guide_lines);
  let secDrag = $state<{ index: number; startX: number; startAt: number; at: number; active: boolean } | null>(null);
  let secRename = $state<{ index: number; text: string } | null>(null);
  let rulerMenu = $state<{ x: number; y: number; at: number; index: number | null } | null>(null);

  function sectionAt(index: number): number {
    return secDrag?.index === index ? secDrag.at : sections[index].at;
  }

  /** Grid position under clientX in `el` (the ruler or the tracks), snapped to eighths or, with Shift, sixteenths. */
  function gridAt(el: HTMLElement, clientX: number, fine: boolean): number {
    const raw = (clientX - el.getBoundingClientRect().left) / zoom;
    const snap = fine ? 1 : SNAP_SIXTEENTHS;
    return Math.max(0, Math.round(raw / snap) * snap);
  }

  function flagDown(e: PointerEvent, index: number) {
    if (e.button !== 0 || secRename) return;
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    secDrag = { index, startX: e.clientX, startAt: sections[index].at, at: sections[index].at, active: false };
  }
  function flagMove(e: PointerEvent) {
    if (!secDrag) return;
    if (!secDrag.active && Math.abs(e.clientX - secDrag.startX) < 4) return;
    secDrag.active = true;
    const snap = e.shiftKey ? 1 : SNAP_SIXTEENTHS;
    const raw = secDrag.startAt + (e.clientX - secDrag.startX) / zoom;
    secDrag.at = Math.max(0, Math.round(raw / snap) * snap);
  }
  function flagUp(index: number) {
    const d = secDrag;
    secDrag = null;
    app.selectedStep = null;
    if (!d || !d.active || d.at === d.startAt) {
      app.selectedSection = index;
      return;
    }
    app.edit({ op: 'move_section', seq: seq.id, section: index, at: d.at });
    // Its place in the sorted list after the move.
    app.selectedSection = sections.filter((s, i) => i !== index && s.at < d.at).length;
  }
  function commitRename() {
    const r = secRename;
    secRename = null;
    if (!r) return;
    const name = r.text.trim();
    if (name && name !== sections[r.index]?.name) app.edit({ op: 'rename_section', seq: seq.id, section: r.index, name });
  }
  function rulerContext(e: MouseEvent, index: number | null) {
    e.preventDefault();
    e.stopPropagation();
    const ruler = (e.currentTarget as HTMLElement).closest('.tracks')?.querySelector('.ruler') as HTMLElement;
    rulerMenu = { x: e.clientX, y: e.clientY, at: gridAt(ruler, e.clientX, e.shiftKey), index };
  }
  function autofocus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }
  let bars = $derived(totalSixteenths / SIXTEENTHS_PER_BAR);
  // Sixteenth ticks on the ruler once they are far enough apart to read.
  let fineRuler = $derived(zoom >= 6);

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
    app.selectedSection = null;
  }

  // ---- resize by the right edge ----
  function resizeDown(e: PointerEvent, lane: number, index: number, step: Step) {
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    resize = { lane, index, startX: e.clientX, startLen: step.length, len: step.length };
  }
  // Snaps the length to eighths, or to sixteenths while Shift is held (SPEC §9.3).
  function resizeMove(e: PointerEvent) {
    if (!resize) return;
    const snap = e.shiftKey ? 1 : SNAP_SIXTEENTHS;
    const raw = resize.startLen + (e.clientX - resize.startX) / zoom;
    resize.len = Math.max(snap, Math.round(raw / snap) * snap);
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
    app.selectedSection = null;
    app.selectedLane = null;
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

  function laneState(li: number): string {
    return run?.lanes[li]?.state ?? 'idle';
  }

  /** The lane's settings in one line (`full`: spelled out, for the tooltip). */
  function laneSummary(lane: Lane, full: boolean): string {
    const mode = isSlotLane(lane) ? 'slot' : 'clip';
    const end = ends.find((o) => o.value === lane.end)?.label ?? lane.end;
    const fade = lane.transition
      ? lane.transition.length === 0 ? 'cut' : `↗ ${fadeLength(lane.transition.length)}`
      : lane.steps.some((s) => s.transition) ? (full ? "steps' own fades" : '↗ steps') : '';
    if (full) return `${mode} lane · ${end}${fade ? ` · ${fade}` : ''} · click for the lane settings`;
    return [mode, end.split(' ')[0].toLowerCase(), fade].filter(Boolean).join(' · ');
  }

  let bpm = $derived(app.runtime?.clock.bpm ?? 120);
  // This sequence records from Resolume (SPEC §7.9): its yielded lanes are the ones you play.
  let recording = $derived(app.runtime?.record?.seq === seq.id && !!app.runtime?.record?.recording);
  let rec = $derived(recording ? app.runtime?.record ?? null : null);

  /** Start (sixteenths) of each step of a lane. */
  function stepStarts(lane: Lane): number[] {
    const out: number[] = [];
    let at = 0;
    for (const st of lane.steps) {
      out.push(at);
      at += st.length;
    }
    return out;
  }
  /**
   * Whether a tie can ever continue anything (SPEC §6 `tie`): `false` when the step before (the lane's last for
   * the first step) surely plays another clip; unknown with choices.
   */
  function tieWorks(li: number, si: number): boolean {
    const lane = seq.lanes[li];
    const steps = lane.steps;
    const prev = si > 0 ? si - 1 : steps.length - 1;
    if (prev === si || steps[si].choices?.length || steps[prev].choices?.length) return true;
    const res = view.resolution?.lanes[li]?.steps;
    const a = res?.[prev];
    const b = res?.[si];
    if (isSlotLane(lane)) return steps[prev].column != null && steps[prev].column === steps[si].column;
    if (!steps[prev].clip || !steps[si].clip) return false;
    if (a?.clip_id != null && b?.clip_id != null) return a.clip_id === b.clip_id;
    return true;
  }

  /** Recording (SPEC §7.9): this step was recorded in the current pass / by its newest click. */
  function takeMark(layer: string, start: number): 'new' | 'take' | null {
    if (!rec) return null;
    let i = -1;
    rec.take.forEach((m, k) => {
      if (m.layer === layer && m.at === start) i = k;
    });
    if (i < 0) return null;
    return i === rec.take.length - 1 ? 'new' : 'take';
  }
  /** Below this block width the fade label is just `↗ 1` (SPEC §9.3). */
  const NARROW_PX = 120;

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
        {@const rlane = run?.lanes[li]}
        <!-- The lane head (SPEC §9.3): layer, state, M/S while running and a summary; the rest is in the
             lane inspector (click the head). -->
        <div class="lanehead" class:bad={res && res.layer !== 'ok'} class:selected={app.selectedLane === li && !app.selectedStep}
          role="button" tabindex="-1" title="Click: lane settings in the inspector"
          onclick={() => { app.selectedLane = li; app.selectedStep = null; app.selectedSection = null; }}
          onkeydown={(e) => e.key === 'Enter' && (app.selectedLane = li)}>
          <div class="row-between">
            <select class="layer-select mono" value={lane.layer} title={res?.layer === 'ok' ? 'layer' : `layer ${res?.layer}`}
              onclick={(e) => e.stopPropagation()}
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
            {#if run && rlane && state !== 'idle' && state !== 'taken_over'}
              <span class="mode-switch ms" role="group" aria-label="Mute and solo">
                <button aria-pressed={rlane.mute} title="Mute: clear the layer, keep counting; back at the next step"
                  onclick={(e) => { e.stopPropagation(); app.send({ cmd: 'set_lane_mute', seq: seq.id, lane: li, on: !rlane.mute }); }}>M</button>
                <button aria-pressed={rlane.solo} title="Solo: mute the other lanes of this sequence"
                  onclick={(e) => { e.stopPropagation(); app.send({ cmd: 'set_lane_solo', seq: seq.id, lane: li, on: !rlane.solo }); }}>S</button>
              </span>
            {/if}
          </div>
          <span class="lane-info mono" title={laneSummary(lane, true)}>{laneSummary(lane, false)}</span>
          <div class="lane-sum">
            {#if rlane?.silent}
              <span class="pill muted" title={rlane.mute ? 'Muted: counts, sends nothing' : 'Another lane is soloed'}>muted</span>
            {:else if state === 'yielded' && recording}
              <span class="pill rec" title="Recording: you play this layer in Resolume">rec</span>
            {:else if state === 'yielded'}
              <button class="pill yielded" title="Played by hand in Resolume. Click: Resume lane (plays again from its next step)"
                onclick={(e) => { e.stopPropagation(); app.send({ cmd: 'resume_lane', seq: seq.id, lane: li }); }}>yielded ↺</button>
            {:else}
              <span class="pill" class:playing={state === 'playing'} class:yielded={state === 'resume_pending'}>
                {state === 'resume_pending' ? 'resuming' : state === 'taken_over' ? 'taken over' : state}
              </span>
            {/if}
          </div>
        </div>
      {/each}
    </div>

    <div class="tracks">
      <div class="ruler" role="presentation" oncontextmenu={(e) => rulerContext(e, null)}
        title="Right-click: add a section here · M adds one at the playhead or the selected step">
        {#each Array(bars) as _, b (b)}
          <div class="bar" style:flex={`0 0 ${SIXTEENTHS_PER_BAR * zoom}px`}>
            <div class="num">{b + 1}</div>
            <div class="ticks" class:fine={fineRuler}>{#each Array(fineRuler ? SIXTEENTHS_PER_BAR : SIXTEENTHS_PER_BAR / 2) as _, t (t)}<i></i>{/each}</div>
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
            {@const start = stepStarts(lane)[si]}
            {@const mark = takeMark(lane.layer, start)}
            {@const holding = !!rec && rec.held.includes(lane.layer) && run != null && start * TICKS_PER_SIXTEENTH <= run.pos && run.pos < (start + step.length) * TICKS_PER_SIXTEENTH}
            {@const fade = stepFade(lane, step)}
            {@const blockPx = len(li, si, step) * zoom}
            {@const fadeWarns = fade ? fadeWarnings(fade.t, app.laneLayers.find((l) => l.name === lane.layer), clip, bpm) : []}
            <div class="slot" role="listitem" style:width={`${len(li, si, step) * zoom}px`}
              class:drop-before={(dropAt?.lane === li && dropAt.index === si) || (drag?.active && drag.lane === li && drag.to === si && drag.from !== si && (drag.alt || drag.to < drag.from))}
              class:drop-after={drag?.active && !drag.alt && drag.lane === li && drag.to === si && drag.to > drag.from}>
              {#if step.tie}
                {@const works = tieWorks(li, si)}
                <!-- The tie spans the gap to the step before (SPEC §6 tie). -->
                <span class="tie-arc" class:idle={!works} aria-hidden="true"
                  title={works ? 'Tied: plays on the clip of the step before if it still plays (T toggles)' : 'Tied, but the step before plays another clip: this tie has no effect'}>⌒</span>
              {/if}
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
                class:tied={!!step.tie}
                class:take={mark != null}
                style:background-image={thumb ? `linear-gradient(color-mix(in srgb, var(--bg) 55%, transparent), color-mix(in srgb, var(--bg) 55%, transparent)), url(${thumb})` : null}
                title={`${stepLabel(step, r)}${step.column != null && r?.state === 'empty' ? ' · plays as a rest' : ''}${r?.ambiguous ? ' · ambiguous match' : ''}${r?.other_layer ? ' · clip is on another layer' : ''}${notConfirmed ? ' · not confirmed by Resolume' : ''}`}
                role="button"
                tabindex="-1"
                onpointerdown={(e) => stepDown(e, li, si)}
                onpointermove={stepMove}
                onpointerup={() => stepUp(li, si)}
                onpointercancel={() => (drag = null)}
              >
                {#if fade && !rest && fade.t.length === 0 && fade.own}
                  <div class="fade-cut" aria-hidden="true"></div>
                {:else if fade && !rest && fade.t.length > 0}
                  {@const fadePx = fade.t.length * zoom}
                  {@const x = Math.min(fadePx, blockPx)}
                  {@const y = 100 - (100 * x) / fadePx}
                  <svg class="fade-wedge" class:inherited={!fade.own} viewBox={`0 0 ${blockPx} 100`} preserveAspectRatio="none" aria-hidden="true">
                    <polygon points={`0,100 ${x},${y} ${x},100`} />
                    <line x1="0" y1="100" x2={x} y2={y} vector-effect="non-scaling-stroke" />
                  </svg>
                  {#if fadePx > blockPx}<span class="fade-more" title="The fade still runs when the next step starts">›</span>{/if}
                {/if}
                {#if mark === 'new'}{#key rec?.clicks}<div class="take-flash" aria-hidden="true"></div>{/key}{/if}
                {#if holding && run}
                  <!-- Held down (Piano): what lies after the playhead is cut at the release. -->
                  <div class="holding" aria-hidden="true" title="Held: ends where you let go"
                    style:left={`${(run.pos / TICKS_PER_SIXTEENTH - start) * zoom}px`}></div>
                {/if}
                <div class="name">{stepLabel(step, r)}</div>
                <!-- Badges share the length row (the length gives way first), so they never cover the clip name. -->
                <div class="step-foot">
                  <div class="len">{step.tie ? '⌒ ' : ''}{unresolved ? 'unresolved · ' : ''}{formatLength(len(li, si, step))}{#if fade && !rest && (fade.t.length > 0 || fade.own)}
                      {' · '}<span class="fade-label" class:bad={fadeWarns.length > 0}
                        title={fadeWarns.length ? fadeWarns.join(' · ') :`${fade.own ? 'Its own transition' : 'The lane default'}: fades in over ${fadeLength(fade.t.length)}${fade.t.blend ? ` with ${fade.t.blend}` : ''}`}
                      >{fadeWarns.length ? '⚠ ' : ''}{fadeLabel(fade.t, blockPx < NARROW_PX)}</span>{/if}</div>
                  {#if step.when || step.choices?.length}
                    {@const warnWhen = whenWarning(step.when, seq.repeat)}
                    {@const n = 1 + (step.choices?.length ?? 0)}
                    {@const choiceUnresolved = (res?.choices?.[si] ?? []).some((c) => c.state === 'unresolved')}
                    <div class="badges">
                      {#if step.when}
                        <span class="badge" class:bad={warnWhen} title={warnWhen ?? 'Plays only on some passes'}>{warnWhen ? '⚠ ' : ''}{whenLabel(step.when)}</span>
                      {/if}
                      {#if n > 1}
                        <span class="badge" class:bad={choiceUnresolved}
                          title={`${n} options, ${step.pick === 'cycle' ? 'in turn' : 'picked at random'}${choiceUnresolved ? ' · a choice is unresolved' : ''}`}
                        >{playing && rl?.option != null ? rl.option + 1 : 1}/{n} {step.pick === 'cycle' ? '↻' : '⤨'}</span>
                      {/if}
                    </div>
                  {/if}
                </div>
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
      {#if guideEvery}
        {#each Array(Math.floor(totalSixteenths / (guideEvery * SIXTEENTHS_PER_BAR))) as _, g (g)}
          {#if g > 0}<div class="guide-line" style:left={`${g * guideEvery * SIXTEENTHS_PER_BAR * zoom}px`}></div>{/if}
        {/each}
      {/if}
      {#each sections as sec, i (i)}
        {@const x = sectionAt(i) * zoom}
        {@const beyond = sectionAt(i) >= view.pass_sixteenths}
        <div class="section-line" class:beyond class:selected={app.selectedSection === i} style:left={`${x}px`}></div>
        <div class="section-flag" class:beyond class:selected={app.selectedSection === i} style:left={`${x}px`}
          role="button" tabindex="-1" title={`${sec.name} · bar ${Math.floor(sectionAt(i) / SIXTEENTHS_PER_BAR) + 1}${beyond ? ' · beyond the end of the sequence' : ''} · drag to move, double-click to rename, Del deletes`}
          onpointerdown={(e) => flagDown(e, i)} onpointermove={flagMove} onpointerup={() => flagUp(i)} onpointercancel={() => (secDrag = null)}
          ondblclick={() => (secRename = { index: i, text: sec.name })} oncontextmenu={(e) => rulerContext(e, i)}>
          {#if secRename?.index === i}
            <input class="rename" maxlength="64" bind:value={secRename.text} use:autofocus onblur={commitRename}
              onpointerdown={(e) => e.stopPropagation()}
              onkeydown={(e) => { e.stopPropagation(); if (e.key === 'Enter') commitRename(); if (e.key === 'Escape') secRename = null; }} />
          {:else}{sec.name}{/if}
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

<svelte:window onclick={() => (rulerMenu = null)} />
{#if rulerMenu}
  {@const m = rulerMenu}
  <div class="menu" style:left={`${m.x}px`} style:top={`${m.y}px`} role="menu">
    {#if m.index == null}
      <button role="menuitem" onclick={() => { app.addSection(view, m.at); rulerMenu = null; }}>
        Add section at bar {Math.floor(m.at / SIXTEENTHS_PER_BAR) + 1}{m.at % SIXTEENTHS_PER_BAR ? ` + ${formatLength(m.at % SIXTEENTHS_PER_BAR)}` : ''}
      </button>
    {:else}
      {@const idx = m.index}
      <button role="menuitem" onclick={() => { secRename = { index: idx, text: sections[idx].name }; rulerMenu = null; }}>Rename</button>
      <button role="menuitem" class="danger" onclick={() => { app.edit({ op: 'delete_section', seq: seq.id, section: idx }); app.selectedSection = null; rulerMenu = null; }}>Delete</button>
    {/if}
  </div>
{/if}
