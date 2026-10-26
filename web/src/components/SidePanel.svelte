<script lang="ts">
  import { app, formatLength } from '../lib/app.svelte';
  import type { ClipView, LayerView } from '../lib/types';

  let deck = $derived(app.viewDeck);
  let view = $derived(app.selected);
  let sel = $derived(app.selectedStep);
  let step = $derived(view && sel ? view.sequence.lanes[sel.lane]?.steps[sel.index] ?? null : null);
  let res = $derived(view && sel ? view.resolution?.lanes[sel.lane]?.steps[sel.index] ?? null : null);
  let clip = $derived(app.clip(res?.clip_id));

  let q = $derived(app.search.trim().toLowerCase());
  function matches(l: LayerView, c: ClipView) {
    return !q || c.name.toLowerCase().includes(q) || l.name.toLowerCase().includes(q) || (c.file ?? '').toLowerCase().includes(q);
  }
  let rows = $derived(
    (deck?.layers ?? []).map((l) => ({ layer: l, clips: l.clips.filter((c) => matches(l, c)) })).filter((r) => r.clips.length > 0 || !q),
  );

  function dragStart(e: DragEvent, c: ClipView) {
    e.dataTransfer?.setData('application/x-seq-clip', JSON.stringify(app.clipRef(c)));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy';
  }

  function pick(c: ClipView) {
    if (!app.relinking || !view || !sel) return;
    app.edit({ op: 'set_step_clip', seq: view.sequence.id, lane: sel.lane, index: sel.index, clip: app.clipRef(c) });
    app.relinking = false;
  }

  function setLength(n: number) {
    if (view && sel && n >= 1) app.edit({ op: 'set_step_length', seq: view.sequence.id, lane: sel.lane, index: sel.index, length: n });
  }

  function mark(v: boolean | null | undefined) {
    return v == null ? '—' : v ? '✓' : '✕';
  }
</script>

<aside class="side">
  <div style="display:flex;flex-direction:column;gap:8px">
    <div class="caps">Clip browser · {deck?.name ?? '—'}</div>
    <div class="meta">
      {#if deck?.clips_source === 'live'}Live thumbnails · inactive decks show cached ones
      {:else if deck?.clips_source === 'cached'}Cached from the last time this deck was live
      {:else}This deck has not been live yet: make it live once in Resolume to load its clips{/if}
    </div>
    {#if app.relinking}
      <div class="tag pending">Click a clip to relink the selected step · Esc cancels</div>
    {/if}
    <label class="meta" style="display:flex;flex-direction:column;gap:4px">Search clips
      <input type="text" placeholder="name, layer or file" bind:value={app.search} />
    </label>
    <div class="browser">
      {#each rows as { layer, clips } (layer.id)}
        <div class="layer-row">
          <div class="mono" style="font-size:11px;color:var(--text-2)">
            {layer.name}{layer.ambiguous ? ' · ambiguous name' : ''}{layer.is_seq_layer ? ' · placeholders' : ''}
          </div>
          <div class="clips">
            {#each clips as c (c.id)}
              {@const url = app.thumbUrl(c)}
              <div class="clip" class:pickable={app.relinking} role="button" tabindex="0" title={`${c.name}${c.file ? '\n' + c.file : ''}\ncolumn ${c.column}`}
                draggable={!layer.is_seq_layer} ondragstart={(e) => dragStart(e, c)} onclick={() => pick(c)}
                onkeydown={(e) => e.key === 'Enter' && pick(c)}>
                <div class="thumb" style:background-image={url ? `url(${url})` : null}></div>
                <span>{c.name || '—'}</span>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>

  <div class="inspector">
    {#if view && sel && step}
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
        <button class="btn" style="text-align:left" onclick={() => (app.relinking = !app.relinking)}>{app.relinking ? 'Cancel relink' : 'Relink to clip…'}</button>
        {#if step.clip}
          <button class="btn" onclick={() => view && sel && app.edit({ op: 'set_step_clip', seq: view.sequence.id, lane: sel.lane, index: sel.index, clip: null })}>Make rest</button>
        {/if}
      </div>
      <div style="display:flex;flex-direction:column;gap:4px">
        <span class="meta">Length (1/8 bar)</span>
        <div class="stepper">
          <button class="icon-btn" aria-label="Shorter" disabled={step.length <= 1} onclick={() => setLength(step.length - 1)}>−</button>
          <output class="mono">{step.length} · {formatLength(step.length)}</output>
          <button class="icon-btn" aria-label="Longer" onclick={() => setLength(step.length + 1)}>+</button>
        </div>
      </div>
    {:else}
      <div class="caps">Step inspector</div>
      <div class="hint">Select a step in the lane grid.</div>
    {/if}
  </div>
</aside>
