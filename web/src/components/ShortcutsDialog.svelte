<script lang="ts">
  // Help → Keyboard shortcuts (M11 Phase F): lib/shortcuts.ts, filtered to this mode (desktop / browser).
  import { app } from '../lib/app.svelte';
  import { SHORTCUT_GROUPS } from '../lib/shortcuts';
  import Dialog from './Dialog.svelte';

  let mode = $derived(app.desktop ? 'desktop' : 'browser');
</script>

<Dialog title="Keyboard shortcuts" onclose={() => (app.dialog = null)}>
  {#each SHORTCUT_GROUPS as g (g.title)}
    <section>
      <h3 class="caps">{g.title}</h3>
      <table>
        <tbody>
          {#each g.items.filter((s) => !s.only || s.only === mode) as s (s.keys)}
            <tr><th scope="row"><kbd>{s.keys}</kbd></th><td>{s.what}</td></tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/each}
</Dialog>

<style>
  section { display: flex; flex-direction: column; gap: 6px; }
  h3 { margin: 0; }
  table { border-collapse: collapse; width: 100%; }
  th { text-align: left; font-weight: 400; white-space: nowrap; padding: 4px 16px 4px 0; vertical-align: top; width: 1%; }
  td { padding: 4px 0; color: var(--text-2); }
  tr + tr th, tr + tr td { border-top: 1px solid var(--line-soft); }
  kbd { font: 12px var(--mono); padding: 1px 6px; border-radius: 4px; border: 1px solid var(--line-strong); background: var(--control); }
</style>
