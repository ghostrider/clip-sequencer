<script lang="ts">
  import { app, SIXTEENTHS_PER_BAR, repeatLabel } from '../lib/app.svelte';
  import type { SequenceView } from '../lib/types';

  let deck = $derived(app.viewDeck);
  let menu = $state<{ x: number; y: number; view: SequenceView } | null>(null);
  let renaming = $state<string | null>(null);
  let renameText = $state('');
  let confirmDelete = $state<SequenceView | null>(null);
  let resolumeDecks = $derived([...new Set(app.decks.filter((d) => !d.orphaned).map((d) => d.name))]);

  function bars(v: SequenceView) {
    const b = v.pass_sixteenths / SIXTEENTHS_PER_BAR;
    return Number.isInteger(b) ? `${b}` : b.toFixed(2);
  }

  function bindingText(v: SequenceView): { text: string; bad: boolean } {
    const s = app.structure;
    if (!s || !deck) return { text: '', bad: false };
    if (deck.clips_source === 'none') return { text: 'deck clips unknown', bad: false };
    if (v.binding) return { text: `${s.seq_prefix}${v.sequence.name} · col ${v.binding.column}`, bad: false };
    return { text: `no ${s.seq_layer} clip in deck`, bad: true };
  }

  function select(v: SequenceView) {
    if (app.selectedSeq !== v.sequence.id) app.selectedStep = null;
    app.selectedSeq = v.sequence.id;
  }

  function startRename(v: SequenceView) {
    renaming = v.sequence.id;
    renameText = v.sequence.name;
    menu = null;
  }

  function commitRename(v: SequenceView) {
    const name = renameText.trim();
    if (name && name !== v.sequence.name) app.edit({ op: 'rename_sequence', seq: v.sequence.id, name });
    renaming = null;
  }

  function duplicate(v: SequenceView) {
    if (!deck) return;
    const id = crypto.randomUUID();
    app.edit({ op: 'duplicate_sequence', seq: v.sequence.id, id, name: app.uniqueName(deck, `${v.sequence.name} copy`) });
    app.selectedSeq = id;
    menu = null;
  }

  function remove(v: SequenceView) {
    if (app.run(v.sequence.id)) app.send({ cmd: 'stop', seq: v.sequence.id });
    app.edit({ op: 'delete_sequence', seq: v.sequence.id });
    if (app.selectedSeq === v.sequence.id) app.selectedSeq = null;
    confirmDelete = null;
  }

  function autofocus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }
</script>

<svelte:window onclick={() => (menu = null)} />

<aside class="seqlist">
  <div class="row-between">
    <div class="caps">Sequences · {deck?.name ?? '—'}</div>
    <button class="btn ghost" disabled={!deck || !!app.structure?.model_error} onclick={() => app.newSequence()}>+ New</button>
  </div>
  {#if deck?.orphaned}
    <div class="hint" style="color:var(--err-text)">Deck "{deck.name}" no longer exists in Resolume. Move its sequences to another deck (right-click).</div>
  {/if}
  <div style="display:flex;flex-direction:column;gap:10px">
    {#each deck?.sequences ?? [] as v (v.sequence.id)}
      {@const running = !!app.run(v.sequence.id)}
      {@const b = bindingText(v)}
      <div
        class="seqcard"
        class:selected={app.selectedSeq === v.sequence.id}
        role="button"
        tabindex="0"
        onclick={() => select(v)}
        onkeydown={(e) => e.key === 'Enter' && select(v)}
        oncontextmenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          select(v);
          menu = { x: e.clientX, y: e.clientY, view: v };
        }}
      >
        <img class="mosaic" alt="" src={`/thumbs/seq/${v.sequence.id}.png?v=${v.mosaic_key}`} />
        <div class="seqinfo">
          <div style="display:flex;align-items:center;gap:6px">
            {#if renaming === v.sequence.id}
              <input
                class="rename"
                bind:value={renameText}
                use:autofocus
                onclick={(e) => e.stopPropagation()}
                onkeydown={(e) => {
                  e.stopPropagation();
                  if (e.key === 'Enter') commitRename(v);
                  if (e.key === 'Escape') renaming = null;
                }}
                onblur={() => commitRename(v)}
              />
            {:else}
              <span class="seqname" role="presentation" ondblclick={() => startRename(v)}>{v.sequence.name}</span>
            {/if}
            {#if running}<span class="run-badge">RUN</span>{/if}
          </div>
          <div class="meta">
            {v.sequence.lanes.length} {v.sequence.lanes.length === 1 ? 'lane' : 'lanes'} · {bars(v)} {bars(v) === '1' ? 'bar' : 'bars'} · {repeatLabel(v.sequence.repeat)}
          </div>
          <div class="bind" class:bad={b.bad}>{b.text}</div>
        </div>
        <div class="spacer"></div>
        {#if deck && !deck.orphaned}
          <button
            class={deck.live || running ? 'icon-btn' : 'btn'}
            class:running
            aria-label={`${running ? 'Stop' : 'Start'} ${v.sequence.name}`}
            disabled={app.runtime?.switching?.seq === v.sequence.id}
            onclick={(e) => {
              e.stopPropagation();
              app.toggle(v, deck);
            }}
          >
            {#if app.runtime?.switching?.seq === v.sequence.id}switching…{:else if running}■{:else if deck.live}▶{:else}▶ {deck.name}{/if}
          </button>
        {/if}
      </div>
    {:else}
      <div class="hint">No sequences in this deck. Click <strong>+ New</strong>.</div>
    {/each}
  </div>
  {#if deck && deck.unmatched_seq_clips.length > 0}
    <div class="hint" style="color:var(--err-text)">
      {app.structure?.seq_prefix}clips without a sequence: {deck.unmatched_seq_clips.join(', ')}
    </div>
  {/if}
</aside>

{#if menu}
  {@const v = menu.view}
  <div class="menu" style:left={`${menu.x}px`} style:top={`${menu.y}px`} role="menu">
    <button role="menuitem" onclick={() => startRename(v)}>Rename</button>
    <button role="menuitem" onclick={() => duplicate(v)}>Duplicate</button>
    <button role="menuitem" class="danger" onclick={() => { confirmDelete = v; menu = null; }}>Delete…</button>
    {#if deck?.orphaned}
      <div class="menu-sep">Move to deck</div>
      {#each resolumeDecks as d (d)}
        <button role="menuitem" onclick={() => { app.edit({ op: 'move_sequence', seq: v.sequence.id, deck: d }); menu = null; }}>{d}</button>
      {/each}
    {/if}
  </div>
{/if}

{#if confirmDelete}
  {@const v = confirmDelete}
  <div class="modal-backdrop open" role="presentation" onclick={(e) => e.target === e.currentTarget && (confirmDelete = null)}>
    <div class="modal" role="dialog" aria-labelledby="del-title">
      <h2 id="del-title">Delete "{v.sequence.name}"?</h2>
      <p style="margin:0;color:var(--text-2)">This can be undone with Ctrl+Z during this session.</p>
      <div class="actions">
        <button class="btn" onclick={() => (confirmDelete = null)}>Cancel</button>
        <button class="btn danger" onclick={() => remove(v)}>Delete</button>
      </div>
    </div>
  </div>
{/if}
