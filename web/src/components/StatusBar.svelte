<script lang="ts">
  import { app } from '../lib/app.svelte';

  let rt = $derived(app.runtime);
  let current = $derived(app.run(app.selectedSeq) ?? rt?.running[0] ?? null);
  let notices = $derived((rt?.notices ?? []).slice(-3));
  function isProblem(n: string) {
    return /yielded|failed|rejected|not confirmed|no clock|lost|problem|cannot|matches no|disconnected/i.test(n);
  }
</script>

<footer class="statusbar">
  {#if current}
    <span style="color:var(--text)">{current.name} · {current.bar_beat} · tick {current.pos}</span>
  {:else}
    <span>idle</span>
  {/if}
  {#if rt?.switching}
    <span style="color:var(--accent-text)">switching to {rt.switching.deck}…</span>
  {/if}
  {#if rt?.last_osc}
    <span>last OSC → {rt.last_osc.addr} · {(rt.last_osc.send_us / 1000).toFixed(1)} ms</span>
  {/if}
  {#if rt?.last_confirm}
    <span style="color:var(--info-text)">WS {rt.last_confirm}</span>
  {/if}
  {#if rt?.save_error}
    <span style="color:var(--err-text)">save failed: {rt.save_error}</span>
  {/if}
  {#if app.ffglHint}
    <button class="statusbar-hint" onclick={() => app.openDialog('ffgl')}
      title="File → Install FFGL plugin…">
      {app.ffglHint === 'missing' ? 'FFGL plugin missing · Install…' : 'FFGL plugin update available · Update…'}
    </button>
  {/if}
  {#each notices as n, i (i + n)}
    <span style:color={isProblem(n) ? 'var(--err-text)' : 'var(--muted)'}>{n}</span>
  {/each}
</footer>

<style>
  .statusbar-hint { background: none; border: 0; padding: 0; font: inherit; color: var(--accent-text); text-decoration: underline;
    cursor: pointer; }
  .statusbar-hint:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 2px; }
</style>
