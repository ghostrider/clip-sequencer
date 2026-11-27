<script lang="ts">
  import { app, formatLength, layerLabel, isSlotLane, whenWarning } from '../lib/app.svelte';
  import type { Choice, ClipView, LayerView, StepResolution, Transition, When } from '../lib/types';
  import { fadeLength, fadeWarnings, stepFade } from '../lib/fade';
  import FadePicker from './FadePicker.svelte';

  let deck = $derived(app.viewDeck);
  let view = $derived(app.selected);
  let sel = $derived(app.selectedStep);
  let step = $derived(view && sel ? view.sequence.lanes[sel.lane]?.steps[sel.index] ?? null : null);
  let res = $derived(view && sel ? view.resolution?.lanes[sel.lane]?.steps[sel.index] ?? null : null);
  let clip = $derived(app.clip(res?.clip_id));
  let slotLane = $derived(view && sel ? isSlotLane(view.sequence.lanes[sel.lane]) : false);

  let q = $derived(app.search.trim().toLowerCase());
  function matches(l: LayerView, c: ClipView) {
    return !q || c.name.toLowerCase().includes(q) || l.name.toLowerCase().includes(q) || (c.file ?? '').toLowerCase().includes(q);
  }
  let rows = $derived(
    (deck?.layers ?? []).map((l) => ({ layer: l, clips: l.clips.filter((c) => matches(l, c)) })).filter((r) => r.clips.length > 0 || !q),
  );

  function dragStart(e: DragEvent, c: ClipView) {
    e.dataTransfer?.setData('application/x-seq-clip', JSON.stringify(app.clipRef(c)));
    e.dataTransfer?.setData('application/x-seq-column', String(c.column));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy';
  }

  function pick(c: ClipView) {
    if (!app.relinking || !view || !sel) return;
    const at = { seq: view.sequence.id, lane: sel.lane, index: sel.index };
    const choice: Choice = slotLane ? c.column : app.clipRef(c);
    const target = app.relinkTarget;
    if (target === 'add') app.edit({ op: 'add_choice', ...at, choice });
    else if (typeof target === 'number') app.edit({ op: 'set_choice', ...at, option: target, choice });
    else if (slotLane) app.edit({ op: 'set_step_column', ...at, column: c.column });
    else app.edit({ op: 'set_step_clip', ...at, clip: app.clipRef(c) });
    app.relinking = false;
    app.relinkTarget = 'step';
  }

  /** Start (or cancel) picking a clip in the browser for `target`. */
  function relink(target: 'step' | 'add' | number) {
    const same = app.relinking && app.relinkTarget === target;
    app.relinkTarget = target;
    app.relinking = !same;
  }

  // ---- format v4: pass condition and choices (SPEC §6, §9.4) ----

  type PlayOn = 'always' | When['is'];
  const playOnLabels: [PlayOn, string][] = [
    ['always', 'every pass'],
    ['first', 'first pass'],
    ['not_first', 'all but the first'],
    ['last', 'last pass'],
    ['not_last', 'all but the last'],
    ['cycle', 'pass A of every B'],
  ];

  function setWhen(when: When | null) {
    if (view && sel) app.edit({ op: 'set_step_when', seq: view.sequence.id, lane: sel.lane, index: sel.index, when });
  }

  function setPlayOn(v: PlayOn) {
    if (v === 'always') setWhen(null);
    else if (v === 'cycle') setWhen({ is: 'cycle', pass: 1, of: 2 });
    else setWhen({ is: v });
  }

  function setCycle(pass: number, of: number) {
    of = Math.min(16, Math.max(2, Math.round(of) || 2));
    pass = Math.min(of, Math.max(1, Math.round(pass) || 1));
    setWhen({ is: 'cycle', pass, of });
  }

  function choiceEdit(op: 'remove_choice' | 'set_pick', extra: Record<string, unknown>) {
    if (view && sel) app.edit({ op, seq: view.sequence.id, lane: sel.lane, index: sel.index, ...extra } as never);
  }

  function moveChoice(from: number, to: number) {
    if (view && sel) app.edit({ op: 'move_choice', seq: view.sequence.id, lane: sel.lane, index: sel.index, from, to });
  }

  function addRestChoice() {
    if (view && sel) app.edit({ op: 'add_choice', seq: view.sequence.id, lane: sel.lane, index: sel.index, choice: null });
  }

  /** Dropping a clip from the browser onto the Choices box adds it as a choice. */
  function dropChoice(e: DragEvent) {
    if (!view || !sel) return;
    const data = e.dataTransfer?.getData('application/x-seq-clip');
    if (!data) return;
    e.preventDefault();
    const choice: Choice = slotLane ? Number(e.dataTransfer?.getData('application/x-seq-column')) : JSON.parse(data);
    app.edit({ op: 'add_choice', seq: view.sequence.id, lane: sel.lane, index: sel.index, choice });
  }

  function choiceLabel(c: Choice, r: StepResolution | undefined): string {
    if (c == null) return 'Rest';
    if (typeof c === 'number') {
      if (r?.state === 'empty') return `col ${c} · empty`;
      return `col ${c} · ${app.clip(r?.clip_id)?.name || 'clip'}`;
    }
    return app.clip(r?.clip_id)?.name || c.name || c.file || 'clip';
  }

  let choices = $derived(step?.choices ?? []);
  let choiceRes = $derived(view && sel ? view.resolution?.lanes[sel.lane]?.choices?.[sel.index] ?? [] : []);

  function setLength(n: number) {
    if (view && sel && n >= 1) app.edit({ op: 'set_step_length', seq: view.sequence.id, lane: sel.lane, index: sel.index, length: n });
  }

  function setColumn(n: number | null) {
    if (view && sel && (n === null || n >= 1)) {
      app.edit({ op: 'set_step_column', seq: view.sequence.id, lane: sel.lane, index: sel.index, column: n });
    }
  }

  // ---- transition (SPEC §7.7, §9.4) ----

  function setTransition(transition: Transition | null) {
    if (view && sel) app.edit({ op: 'set_step_transition', seq: view.sequence.id, lane: sel.lane, index: sel.index, transition });
  }

  function mark(v: boolean | null | undefined) {
    return v == null ? '—' : v ? '✓' : '✕';
  }
</script>

{#snippet v4Fields()}
  {#if view && sel && step}
    {@const when = step.when}
    {@const warn = whenWarning(when, view.sequence.repeat)}
    {@const lane = view.sequence.lanes[sel.lane]}
    {@const layer = app.laneLayers.find((l) => l.name === lane.layer)}
    {@const fade = stepFade(lane, step)}
    {@const fadeWarns = fade ? fadeWarnings(fade.t, layer, clip, app.runtime?.clock.bpm ?? 120) : []}
    {@const anyFade = !!lane.transition || lane.steps.some((s) => s.transition)}
    <label class="meta tie-toggle" title="If the clip of the step before still plays (also across the loop), this step lets it play on instead of starting it again">
      <input type="checkbox" checked={!!step.tie}
        onchange={(e) => view && sel && app.edit({ op: 'set_step_tie', seq: view.sequence.id, lane: sel.lane, index: sel.index, tie: (e.currentTarget as HTMLInputElement).checked })} />
      ⌒ Tie to the step before: continue its clip, don't restart it
    </label>
    <div style="display:flex;flex-direction:column;gap:4px">
      <span class="meta">Transition · how the clip comes in</span>
      <div style="display:flex;gap:6px;align-items:center;flex-wrap:wrap">
        <select class="pill-select" style="max-width:none" aria-label="Transition" value={step.transition ? 'own' : 'inherit'}
          onchange={(e) => setTransition((e.currentTarget as HTMLSelectElement).value === 'own' ? { ...(lane.transition ?? { length: 4 }) } : null)}>
          <option value="inherit">{lane.transition ? `lane default (${fadeLength(lane.transition.length)})` : anyFade ? "the layer's original" : "Resolume's own"}</option>
          <option value="own">its own</option>
        </select>
        {#if step.transition}
          <FadePicker transition={step.transition} options={layer?.blend_options ?? []} onchange={setTransition} />
        {/if}
      </div>
      {#each fadeWarns as w (w)}<span style="font-size:11px;color:var(--err-text)">⚠ {w}</span>{/each}
    </div>

    <div style="display:flex;flex-direction:column;gap:4px">
      <span class="meta">Play on</span>
      <div style="display:flex;gap:6px;align-items:center;flex-wrap:wrap">
        <select class="pill-select" style="max-width:none" value={when?.is ?? 'always'}
          onchange={(e) => setPlayOn((e.currentTarget as HTMLSelectElement).value as PlayOn)}>
          {#each playOnLabels as [v, label] (v)}<option value={v}>{label}</option>{/each}
        </select>
        {#if when?.is === 'cycle'}
          <label class="meta">pass <input class="num-input mono" type="number" min="1" max={when.of} value={when.pass}
            onchange={(e) => setCycle(Number((e.currentTarget as HTMLInputElement).value), when.of)} /></label>
          <label class="meta">of <input class="num-input mono" type="number" min="2" max="16" value={when.of}
            onchange={(e) => setCycle(when.pass, Number((e.currentTarget as HTMLInputElement).value))} /></label>
        {/if}
      </div>
      {#if warn}<span style="font-size:11px;color:var(--err-text)">⚠ {warn}</span>{/if}
      {#if when}<span class="meta">On other passes the step sends nothing: the clip before it plays on.</span>{/if}
    </div>

    <div class="choices" role="group" aria-label="Choices" ondragover={(e) => e.dataTransfer?.types.includes('application/x-seq-clip') && e.preventDefault()}
      ondrop={dropChoice}>
      <div style="display:flex;align-items:center;gap:6px">
        <span class="meta" style="flex:1">Choices · one option is picked each time the step plays</span>
        {#if choices.length > 0}
          <div class="mode-switch" role="group" aria-label="Pick">
            <button aria-pressed={step.pick !== 'cycle'} title="Any option each time"
              onclick={() => choiceEdit('set_pick', { pick: null })}>⤨ random</button>
            <button aria-pressed={step.pick === 'cycle'} title="Option 1, 2, 3 … in turn; each start begins at 1"
              onclick={() => choiceEdit('set_pick', { pick: 'cycle' })}>↻ cycle</button>
          </div>
        {/if}
      </div>
      <div class="choice-row"><span class="mono">1</span><span class="ellipsis" style="flex:1">this step</span></div>
      {#each choices as c, ci (ci)}
        {@const r = choiceRes[ci]}
        {@const cl = app.clip(r?.clip_id)}
        <div class="choice-row" class:bad={r?.state === 'unresolved'}>
          <span class="mono">{ci + 2}</span>
          <div class="thumb" style="width:32px;flex:none" class:rest={c == null || r?.state === 'empty'} class:unres={r?.state === 'unresolved'}
            style:background-image={app.thumbUrl(cl) ? `url(${app.thumbUrl(cl)})` : null}></div>
          <span class="ellipsis" style="flex:1" title={r?.state === 'unresolved' ? 'Unresolved: plays nothing when picked' : ''}>
            {choiceLabel(c, r)}{r?.state === 'unresolved' ? ' · unresolved' : ''}
          </span>
          <button class="icon-btn" aria-label="Move up" title="Move up (to the top: becomes the step's own)" onclick={() => moveChoice(ci + 1, ci)}>↑</button>
          <button class="icon-btn" aria-label="Move down" disabled={ci === choices.length - 1} onclick={() => moveChoice(ci + 1, ci + 2)}>↓</button>
          <button class="icon-btn" aria-label="Relink choice" title={slotLane ? 'Pick its column from a clip' : 'Relink to a clip'}
            aria-pressed={app.relinking && app.relinkTarget === ci + 1} onclick={() => relink(ci + 1)}>⇄</button>
          <button class="icon-btn" aria-label="Remove choice" onclick={() => choiceEdit('remove_choice', { option: ci + 1 })}>×</button>
        </div>
      {/each}
      {#if choices.length < 3}
        <div style="display:flex;gap:6px;flex-wrap:wrap">
          <button class="btn" aria-pressed={app.relinking && app.relinkTarget === 'add'} onclick={() => relink('add')}>
            {app.relinking && app.relinkTarget === 'add' ? 'Cancel' : '+ Choice from clip…'}</button>
          <button class="btn" onclick={addRestChoice}>+ Rest</button>
        </div>
        <span class="meta">or drop a clip from the browser here (up to 3 choices)</span>
      {/if}
    </div>
  {/if}
{/snippet}

<aside class="side">
  <div style="display:flex;flex-direction:column;gap:8px">
    <div class="caps">Clip browser · {deck?.name ?? '—'}</div>
    <!-- While picking, the hint takes the place of the source line (and floats over the search box if it is
         taller), so the clips don't move under the pointer. -->
    <div class="source-line">
      <div class="meta" class:hidden-keep={app.relinking}>
        {#if deck?.clips_source === 'live'}Live thumbnails · inactive decks show cached ones
        {:else if deck?.clips_source === 'cached'}Cached from the last time this deck was live
        {:else}This deck has not been live yet: make it live once in Resolume to load its clips{/if}
      </div>
      {#if app.relinking}
        <div class="tag pending pick-hint">
          {app.relinkTarget === 'add' ? 'Click a clip to add it as a choice' : typeof app.relinkTarget === 'number' ? `Click a clip for choice ${app.relinkTarget + 1}` : slotLane ? 'Click a clip to use its column for the selected step' : 'Click a clip to relink the selected step'} · Esc cancels
        </div>
      {/if}
    </div>
    <label class="meta" style="display:flex;flex-direction:column;gap:4px">Search clips
      <input type="text" placeholder="name, layer or file" bind:value={app.search} />
    </label>
    <div class="browser">
      {#each rows as { layer, clips } (layer.id)}
        <div class="layer-row">
          <div class="mono" style="font-size:11px;color:var(--text-2)">
            {layerLabel(layer)}{layer.is_seq_layer ? ' · placeholders' : ''}
          </div>
          <div class="clips">
            {#each clips as c (c.id)}
              {@const url = app.thumbUrl(c)}
              <div class="clip" class:pickable={app.relinking} role="button" tabindex="0" title={`${c.name}${c.file ? '\n' + c.file : ''}\ncolumn ${c.column}`}
                draggable={!layer.is_seq_layer} ondragstart={(e) => dragStart(e, c)} onclick={() => pick(c)}
                onkeydown={(e) => e.key === 'Enter' && pick(c)}>
                <div class="thumb" style:background-image={url ? `url(${url})` : null}>
                  <span class="col-badge mono" aria-label={`column ${c.column}`}>{c.column}</span>
                </div>
                <span>{c.name || '—'}</span>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>

  <div class="inspector">
    {#if view && sel && step && slotLane}
      <!-- Slot lane step (SPEC §4.5, §9.4): a column, played with whatever it holds. -->
      <div class="caps">Step inspector · {view.sequence.lanes[sel.lane].layer} step {sel.index + 1} · slot</div>
      <div style="display:flex;gap:10px;align-items:center">
        <div class="thumb" style="width:64px" class:rest={step.column == null || res?.state === 'empty'}
          style:background-image={app.thumbUrl(clip) ? `url(${app.thumbUrl(clip)})` : null}></div>
        <div style="display:flex;flex-direction:column;gap:2px;min-width:0">
          {#if step.column == null}
            <strong>Rest</strong>
            <span class="meta">Clears the layer at its start</span>
          {:else if res?.state === 'empty'}
            <strong>Column {step.column} · empty</strong>
            <span class="meta">Empty slot → rest (clears the layer)</span>
          {:else}
            <strong class="ellipsis">Column {step.column} · {clip?.name || 'clip'}</strong>
            <span style="font-size:11px;color:var(--info-text)">Plays whatever is in this slot when the step starts</span>
          {/if}
        </div>
      </div>
      <div style="display:flex;flex-direction:column;gap:4px">
        <span class="meta">Column{deck?.columns ? ` (1–${deck.columns})` : ''}</span>
        <div class="stepper">
          <button class="icon-btn" aria-label="Previous column" disabled={step.column == null || step.column <= 1}
            onclick={() => setColumn((step.column ?? 2) - 1)}>−</button>
          <output class="mono">{step.column ?? '—'}</output>
          <button class="icon-btn" aria-label="Next column" disabled={deck != null && deck.columns > 0 && (step.column ?? 0) >= deck.columns}
            onclick={() => setColumn((step.column ?? 0) + 1)}>+</button>
        </div>
      </div>
      <div style="display:flex;gap:6px;flex-wrap:wrap">
        <button class="btn" style="text-align:left" onclick={() => relink('step')}>{app.relinking && app.relinkTarget === 'step' ? 'Cancel' : 'Pick column from clip…'}</button>
        {#if step.column != null}
          <button class="btn" onclick={() => setColumn(null)}>Make rest</button>
        {/if}
      </div>
      <div style="display:flex;flex-direction:column;gap:4px">
        <span class="meta">Length (1/16 bar)</span>
        <div class="stepper">
          <button class="icon-btn" aria-label="Shorter" disabled={step.length <= 1} onclick={() => setLength(step.length - 1)}>−</button>
          <output class="mono">{step.length} · {formatLength(step.length)}</output>
          <button class="icon-btn" aria-label="Longer" onclick={() => setLength(step.length + 1)}>+</button>
        </div>
      </div>
      {@render v4Fields()}
    {:else if view && sel && step}
      <div class="caps">Step inspector · {view.sequence.lanes[sel.lane].layer} step {sel.index + 1}</div>
      <div style="display:flex;gap:10px;align-items:center">
        <div class="thumb" style="width:64px" class:unres={res?.state === 'unresolved'} class:rest={!step.clip}
          style:background-image={app.thumbUrl(clip) ? `url(${app.thumbUrl(clip)})` : null}></div>
        <div style="display:flex;flex-direction:column;gap:2px;min-width:0">
          <strong class="ellipsis">{step.clip ? clip?.name || step.clip.name || step.clip.file || 'clip' : 'Rest'}</strong>
          {#if !step.clip}
            <span class="meta">Clears the layer at its start</span>
          {:else if res?.state === 'unresolved'}
            <span style="font-size:11px;color:var(--err-text)">Unresolved · treated as rest at runtime</span>
          {:else if res}
            <span style="font-size:11px;color:var(--info-text)">
              Resolved by {res.by}{res.ambiguous ? ' · ambiguous' : ''}{res.other_layer ? ' · plays on its own layer' : ''}
            </span>
          {/if}
        </div>
      </div>
      {#if step.clip}
        <div style="display:flex;flex-direction:column;gap:4px">
          <div class="kv"><span class="k">1 · clip ID</span><span class="v" class:ok={res?.methods.id} class:bad={res?.methods.id === false}>{step.clip.id ?? '—'} {mark(res?.methods.id)}</span></div>
          <div class="kv"><span class="k">2 · name in deck</span><span class="v ellipsis" class:ok={res?.methods.name} class:bad={res?.methods.name === false}>{step.clip.name ?? '—'} {mark(res?.methods.name)}</span></div>
          <div class="kv"><span class="k">3 · media file</span><span class="v ellipsis" class:ok={res?.methods.file} class:bad={res?.methods.file === false}>{step.clip.file ?? '—'} {mark(res?.methods.file)}</span></div>
        </div>
      {/if}
      <div style="display:flex;gap:6px;flex-wrap:wrap">
        <button class="btn" style="text-align:left" onclick={() => relink('step')}>{app.relinking && app.relinkTarget === 'step' ? 'Cancel relink' : 'Relink to clip…'}</button>
        {#if step.clip}
          <button class="btn" onclick={() => view && sel && app.edit({ op: 'set_step_clip', seq: view.sequence.id, lane: sel.lane, index: sel.index, clip: null })}>Make rest</button>
        {/if}
      </div>
      <div style="display:flex;flex-direction:column;gap:4px">
        <span class="meta">Length (1/16 bar)</span>
        <div class="stepper">
          <button class="icon-btn" aria-label="Shorter" disabled={step.length <= 1} onclick={() => setLength(step.length - 1)}>−</button>
          <output class="mono">{step.length} · {formatLength(step.length)}</output>
          <button class="icon-btn" aria-label="Longer" onclick={() => setLength(step.length + 1)}>+</button>
        </div>
      </div>
      {@render v4Fields()}
    {:else}
      <div class="caps">Step inspector</div>
      <div class="hint">Select a step in the lane grid.</div>
    {/if}
  </div>
</aside>
