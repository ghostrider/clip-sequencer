<script lang="ts">
  // Preferences: which MIDI input carries the clock. Saved by the app in <data-dir>/settings.json;
  // `--midi-port` on the command line still wins for the run it is given in.
  import { app } from '../lib/app.svelte';

  let ports = $state<string[]>([]);
  let choice = $state<string | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let dialog: HTMLDivElement | undefined = $state();

  async function load() {
    loading = true;
    error = null;
    try {
      const r = await fetch('/api/midi/ports');
      if (!r.ok) throw new Error(await r.text());
      const v: { ports: string[]; selected: string | null } = await r.json();
      ports = v.ports;
      // The saved value may be a part of the full port name (like --midi-port).
      const sel = v.selected;
      choice = sel === null ? null : (v.ports.find((p) => p === sel) ?? v.ports.find((p) => p.toLowerCase().includes(sel.toLowerCase())) ?? sel);
    } catch (e) {
      error = `Could not list the MIDI inputs: ${e}`;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (app.prefsOpen) {
      load();
      queueMicrotask(() => dialog?.focus());
    }
  });

  function close() {
    app.prefsOpen = false;
  }

  function save() {
    app.send({ cmd: 'set_midi_port', port: choice });
    close();
  }

  let current = $derived(app.runtime?.clock);
</script>

{#if app.prefsOpen}
  <div class="modal-backdrop open" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
    <div class="modal prefs" role="dialog" aria-modal="true" aria-labelledby="prefs-title" tabindex="-1" bind:this={dialog}
      onkeydown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); } }}>
      <h2 id="prefs-title">Preferences</h2>
      <fieldset class="prefs-group">
        <legend class="caps">MIDI clock input</legend>
        <p class="meta">
          The sequencer follows the MIDI clock on this input (e.g. the Clock Shifter's virtual port).
          {#if current?.port}
            Now: <span class="mono">{current.port}</span> · {current.connected ? (current.running ? 'clock running' : 'connected, no clock') : 'not found'}
          {/if}
        </p>
        {#if error}
          <p class="prefs-error">{error}</p>
        {:else if loading && !ports.length}
          <p class="meta">Looking for MIDI inputs…</p>
        {:else}
          <div class="prefs-ports" role="radiogroup" aria-label="MIDI input">
            {#each ports as p (p)}
              <label class="prefs-port">
                <input type="radio" name="midi-port" value={p} bind:group={choice} />
                <span class="mono">{p}</span>
              </label>
            {/each}
            {#if choice !== null && !ports.includes(choice)}
              <label class="prefs-port">
                <input type="radio" name="midi-port" value={choice} bind:group={choice} />
                <span class="mono">{choice}</span> <span class="meta">(not connected now)</span>
              </label>
            {/if}
            <label class="prefs-port">
              <input type="radio" name="midi-port" value={null} bind:group={choice} />
              <span>No MIDI clock</span>
            </label>
            {#if !ports.length}
              <p class="meta">No MIDI inputs found. Start the Clock Shifter (or loopMIDI), then refresh.</p>
            {/if}
          </div>
        {/if}
      </fieldset>
      <div class="actions">
        <button class="btn" onclick={load} disabled={loading}>Refresh</button>
        <div class="spacer"></div>
        <button class="btn" onclick={close}>Cancel</button>
        <button class="btn primary" onclick={save} disabled={loading || !!error}>Save</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .prefs { width: min(520px, 92vw); outline: none; }
  .prefs-group { border: 0; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .prefs-group legend { margin-bottom: 6px; }
  .prefs-group p { margin: 0; }
  .prefs-ports { display: flex; flex-direction: column; gap: 2px; max-height: 260px; overflow-y: auto; padding: 4px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--track); }
  .prefs-port { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 4px; cursor: pointer; }
  .prefs-port:hover { background: var(--control); }
  .prefs-port input { accent-color: var(--accent); }
  .prefs-error { color: var(--err-text); }
  .actions .spacer { flex: 1; }
</style>
