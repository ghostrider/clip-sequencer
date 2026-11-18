<script lang="ts">
  // Preferences: which MIDI input carries the clock, and the colour theme (SPEC §9.7). Saved by the app in
  // <data-dir>/settings.json; `--midi-port` on the command line still wins for the run it is given in.
  import { app } from '../lib/app.svelte';
  import { DEFAULT_THEME_ID } from '../lib/themes';
  import ThemeSettings from './ThemeSettings.svelte';

  let ports = $state<string[]>([]);
  let choice = $state<string | null>(null);
  /** The port as loaded: the clock input is only switched (and restarted) when this changes. */
  let loadedChoice: string | null | undefined = undefined;
  let theme = $state(DEFAULT_THEME_ID);
  let themeDirty = $state(false);
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
      loadedChoice = choice;
    } catch (e) {
      error = `Could not list the MIDI inputs: ${e}`;
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (app.prefsOpen) {
      loadedChoice = undefined;
      theme = app.appearance.theme ?? DEFAULT_THEME_ID;
      app.themePreview = true;
      load();
      queueMicrotask(() => dialog?.focus());
    } else if (app.themePreview) {
      // Closed without Save (Cancel, Esc, another dialog): back to the saved theme.
      app.themePreview = false;
      app.applySavedTheme();
    }
  });

  function close() {
    app.prefsOpen = false;
  }

  function save() {
    if (loadedChoice !== undefined && choice !== loadedChoice) app.send({ cmd: 'set_midi_port', port: choice });
    if (theme !== (app.appearance.theme ?? DEFAULT_THEME_ID)) {
      app.send({ cmd: 'set_theme', id: theme === DEFAULT_THEME_ID ? null : theme });
      app.appearance = { ...app.appearance, theme: theme === DEFAULT_THEME_ID ? null : theme };
    }
    app.themePreview = false;
    app.applySavedTheme();
    close();
  }

  let current = $derived(app.runtime?.clock);
</script>

{#if app.prefsOpen}
  <div class="modal-backdrop open" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
    <div class="modal prefs" role="dialog" aria-modal="true" aria-labelledby="prefs-title" tabindex="-1" bind:this={dialog}
      onkeydown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); } }}>
      <h2 id="prefs-title">Preferences</h2>
      <div class="tabs" role="tablist" aria-label="Preferences">
        <button class="tab" role="tab" aria-selected={app.prefsTab === 'clock'} aria-controls="prefs-clock"
          onclick={() => (app.prefsTab = 'clock')}>MIDI clock</button>
        <button class="tab" role="tab" aria-selected={app.prefsTab === 'appearance'} aria-controls="prefs-appearance"
          onclick={() => (app.prefsTab = 'appearance')}>Appearance</button>
      </div>
      <div class="prefs-body">
      <div class="prefs-group" id="prefs-clock" role="tabpanel" aria-label="MIDI clock" hidden={app.prefsTab !== 'clock'}>
        <div class="caps">MIDI clock input</div>
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
      </div>
      <div class="prefs-group" id="prefs-appearance" role="tabpanel" aria-label="Appearance" hidden={app.prefsTab !== 'appearance'}>
        <p class="meta">The theme shows at once; Save keeps it, Cancel goes back. Saved in the app's settings, so the
          desktop window and every browser show the same theme.</p>
        <ThemeSettings bind:selected={theme} bind:dirty={themeDirty} />
      </div>
      </div>
      <div class="actions">
        {#if app.prefsTab === 'clock'}
          <button class="btn" onclick={load} disabled={loading}>Refresh MIDI inputs</button>
        {/if}
        {#if themeDirty}<span class="meta">Theme changes not saved yet: save them as a theme, or discard them.</span>{/if}
        <div class="spacer"></div>
        <button class="btn" onclick={close}>Cancel</button>
        <button class="btn primary" onclick={save} disabled={loading || themeDirty}>Save</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .prefs { width: min(780px, 94vw); max-height: 90vh; outline: none; }
  .prefs-body { overflow-y: auto; min-height: 0; flex: 1 1 auto; padding-right: 4px; }
  .prefs-group[hidden] { display: none; }
  .prefs-group { border: 0; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .prefs-group p { margin: 0; }
  .prefs-ports { display: flex; flex-direction: column; gap: 2px; max-height: 260px; overflow-y: auto; padding: 4px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--track); }
  .prefs-port { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: var(--radius-sm); cursor: pointer; }
  .prefs-port:hover { background: var(--control); }
  .prefs-port input { accent-color: var(--accent); }
  .prefs-error { color: var(--err-text); }
  .actions { align-items: center; flex-wrap: wrap; }
  .actions .spacer { flex: 1; }
</style>
