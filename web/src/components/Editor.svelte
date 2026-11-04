<script lang="ts">
  import { app, formatLength } from '../lib/app.svelte';
  import type { Repeat } from '../lib/types';
  import LaneGrid from './LaneGrid.svelte';

  let view = $derived(app.selected);
  let deck = $derived(app.viewDeck);
  let run = $derived(app.run(view?.sequence.id));
  let editingName = $state(false);
  let nameText = $state('');
  let countText = $state('4');

  $effect(() => {
    const r = view?.sequence.repeat;
    if (r?.mode === 'count') countText = String(r.count);
  });

  function setRepeat(repeat: Repeat) {
    if (view) app.edit({ op: 'set_repeat', seq: view.sequence.id, repeat });
  }

  function commitCount() {
    const n = Math.max(2, Math.round(Number(countText) || 2));
    countText = String(n);
    setRepeat({ mode: 'count', count: n });
  }

  function commitName() {
    const name = nameText.trim();
    if (view && name && name !== view.sequence.name) app.edit({ op: 'rename_sequence', seq: view.sequence.id, name });
    editingName = false;
  }

  function addLane() {
    if (!view) return;
    const layer = app.freeLayer(view.sequence.lanes.map((l) => l.layer));
    if (!layer) return;
    app.edit({ op: 'add_lane', seq: view.sequence.id, layer });
  }

  function autofocus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }
</script>

<main class="editor">
  {#if view && deck}
    {@const s = view.sequence}
    {@const prefix = app.structure?.seq_prefix ?? '§ '}
    <div class="seqhead">
      {#if editingName}
        <input class="rename title" bind:value={nameText} use:autofocus onblur={commitName}
          onkeydown={(e) => { if (e.key === 'Enter') commitName(); if (e.key === 'Escape') editingName = false; }} />
      {:else}
        <h1 title="Double-click to rename" ondblclick={() => { nameText = s.name; editingName = true; }}>{s.name}</h1>
      {/if}
      {#if view.binding}
        <span class="tag info">bound to {prefix}{s.name} · {app.structure?.seq_layer} layer · col {view.binding.column}</span>
      {:else if deck.clips_source !== 'none'}
        <span class="tag bad">no {prefix}{s.name} clip on the {app.structure?.seq_layer} layer</span>
      {/if}
      <span class="hint">
        Start: immediate · {formatLength(view.pass_sixteenths)} · grid 1/8 (Shift 1/16) · after last pass:
        <button class="linkish" title="Toggle what happens after the last pass"
          onclick={() => app.edit({ op: 'set_after_last_pass', seq: s.id, value: s.after_last_pass === 'clear' ? 'hold' : 'clear' })}
        >{s.after_last_pass}</button>
      </span>
      {#if run?.edited}<span class="tag pending">● edited · applies at next start</span>{/if}
      <div class="spacer"></div>
      <div class="repeat" role="group" aria-label="Repeat">
        <span>Repeat</span>
        <button aria-pressed={s.repeat.mode === 'once'} onclick={() => setRepeat({ mode: 'once' })}>Once</button>
        <button aria-pressed={s.repeat.mode === 'count'} onclick={() => s.repeat.mode !== 'count' && commitCount()}>
          × <input type="number" min="2" aria-label="Repeat count" bind:value={countText}
            onclick={(e) => e.stopPropagation()} onchange={commitCount}
            onkeydown={(e) => e.key === 'Enter' && (e.currentTarget as HTMLInputElement).blur()} />
        </button>
        <button aria-pressed={s.repeat.mode === 'infinite'} aria-label="Infinite" onclick={() => setRepeat({ mode: 'infinite' })}>∞</button>
      </div>
      {#if run}<span class="mono">pass {run.pass} / {run.passes ?? '∞'}</span>{/if}
      <button class="btn" onclick={addLane} disabled={!app.freeLayer(s.lanes.map((l) => l.layer))}>+ Lane</button>
      <button class="btn" disabled={!deck.live || !view.binding} title={!deck.live ? 'Only for the live deck' : !view.binding ? 'Needs a § clip on the SEQ layer' : ''}
        onclick={() => app.send({ cmd: 'push_thumbnail', seq: s.id })}>Push thumbnail to Resolume</button>
      <button class="btn primary" disabled={deck.orphaned || app.runtime?.switching?.seq === s.id} onclick={() => app.toggle(view, deck)}>
        {#if app.runtime?.switching?.seq === s.id}switching…{:else if run}■ Stop{:else if deck.live}▶ Start{:else}▶ Start on {deck.name}{/if}
      </button>
    </div>

    <LaneGrid {view} {deck} />

    <div class="hint">
      Drag clips from the browser onto a lane · drag a block's right edge to resize in 1/8-bar steps (hold Shift for 1/16) · drag to reorder, Alt-drag to
      duplicate · Del removes a step · R inserts a rest · Space starts/stops · Ctrl+wheel zooms · Ctrl+Z / Ctrl+Shift+Z undo/redo
    </div>
  {:else}
    <div class="hint" style="margin:auto">Select a sequence on the left, or create one with <strong>+ New</strong>.</div>
  {/if}
</main>
