<script lang="ts">
  // "Sync SEQ layer" (SPEC §7.6): shows what the sync would do in the live deck, then asks the app to do it.
  import { app } from '../lib/app.svelte';
  import type { DeckView } from '../lib/types';
  import Dialog from './Dialog.svelte';

  let { deck, onclose }: { deck: DeckView; onclose: () => void } = $props();

  let removeOrphans = $state(false);
  let plan = $derived(deck.seq_sync);
  let layer = $derived(app.structure?.seq_layer ?? 'SEQ');
  let prefix = $derived(app.structure?.seq_prefix ?? '§ ');
  let clearable = $derived(plan?.orphans.filter((o) => !o.connected) ?? []);
  let columnsNeeded = $derived(
    plan ? Math.max(0, plan.add.length - plan.empty_slots.length - (removeOrphans ? clearable.length : 0)) : 0,
  );
  let nothingToDo = $derived(
    !!plan && plan.layer.state === 'exists' && plan.add.length === 0 && (!removeOrphans || clearable.length === 0),
  );

  function sync() {
    app.send({ cmd: 'sync_seq_layer', remove_orphans: removeOrphans });
    onclose();
  }
</script>

<Dialog title={`Sync ${layer} layer · ${deck.name}`} {onclose}>
  {#if !plan}
    <p class="hint">No composition from Resolume yet.</p>
  {:else}
    <p class="lead">Gives every sequence of this deck a <code>{prefix}name</code> clip on the {layer} layer in Resolume, so it can be started from there.</p>
    {#if plan.layer.state === 'ambiguous'}
      <p class="tag bad">{plan.layer.count} layers are named {layer}. Rename all but one in Resolume, then sync again.</p>
    {:else}
      <ul class="plan">
        {#if plan.layer.state === 'missing'}
          <li>Create the <strong>{layer}</strong> layer on top, with opacity 0.</li>
        {/if}
        {#if plan.add.length > 0}
          <li>
            Add {plan.add.length} {plan.add.length === 1 ? 'clip' : 'clips'}:
            <span class="names">{plan.add.map((n) => prefix + n).join(', ')}</span>
          </li>
        {/if}
        {#if columnsNeeded > 0}
          <li>Add {columnsNeeded} {columnsNeeded === 1 ? 'column' : 'columns'} at the end (the layer has no free slot).</li>
        {/if}
        {#if plan.layer.state === 'exists' && plan.add.length === 0}
          <li>Every sequence already has its clip.</li>
        {/if}
      </ul>
      {#if plan.orphans.length > 0}
        <label class="orphans">
          <input type="checkbox" bind:checked={removeOrphans} disabled={clearable.length === 0} />
          <span>
            Remove {prefix}clips without a sequence:
            <span class="names">{plan.orphans.map((o) => o.name + (o.connected ? ' (playing, kept)' : '')).join(', ')}</span>
          </span>
        </label>
      {/if}
      <p class="hint">New clips hold the "1000A Sequence" plugin, or Resolume's Blank source if the plugin isn't installed. Other clips on the layer stay as they are.</p>
    {/if}
  {/if}
  {#snippet actions()}
    <button class="btn" onclick={onclose}>Cancel</button>
    <button
      class="btn primary"
      disabled={!plan || plan.layer.state === 'ambiguous' || nothingToDo || !!app.runtime?.seq_sync_busy}
      onclick={sync}>Sync</button
    >
  {/snippet}
</Dialog>

<style>
  .lead { margin: 0; color: var(--text-2); }
  .plan { margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 6px; }
  .names { font-family: var(--mono); font-size: 12px; }
  .orphans { display: flex; gap: 8px; align-items: flex-start; }
  code { font-family: var(--mono); }
</style>
