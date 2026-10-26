<script lang="ts">
  import { app } from '../lib/app.svelte';

  let c = $derived(app.confirmSwitch);
  function go() {
    if (c) app.send({ cmd: 'start', seq: c.seq });
    app.confirmSwitch = null;
  }
</script>

{#if c}
  <div class="modal-backdrop open" role="presentation" onclick={(e) => e.target === e.currentTarget && (app.confirmSwitch = null)}>
    <div class="modal" role="dialog" aria-labelledby="dm-title">
      <h2 id="dm-title">Switch deck and start?</h2>
      <p style="margin:0;color:var(--text-2)">
        <strong>{c.running.join(', ')}</strong> {c.running.length === 1 ? 'is' : 'are'} running. Starting
        <strong>{c.name}</strong> stops {c.running.length === 1 ? 'it' : 'them'} and switches Resolume to
        <strong>{c.deck}</strong>.
      </p>
      <div class="actions">
        <button class="btn" onclick={() => (app.confirmSwitch = null)}>Cancel</button>
        <button class="btn primary" onclick={go}>Switch to {c.deck} &amp; start</button>
      </div>
    </div>
  </div>
{/if}
