<script lang="ts">
  import { app } from '../lib/app.svelte';

  let rt = $derived(app.runtime);
  // Connection indicators (SPEC §9.5): the clock (click opens Preferences → Clock), REST, WS, OSC.
  let clockOk = $derived(!!rt?.clock.connected && !!rt?.clock.running);
  let clockText = $derived.by(() => {
    if (!rt) return 'CLOCK';
    const c = rt.clock;
    if (c.source === 'resolume') {
      // SPEC §3.1: ticks made at Resolume's tempo; "synced" once a Resync lined them up.
      if (!c.connected) return 'RESOLUME TEMPO · not connected';
      const bpm = `${c.bpm?.toFixed(1) ?? '—'} BPM`;
      if (c.transport === 'Pause') return `RESOLUME ${bpm} · paused`;
      if (c.transport === 'Stop') return `RESOLUME ${bpm} · stopped`;
      return `RESOLUME ${bpm} · ${c.synced ? 'synced' : 'press Resync'}`;
    }
    if (!c.connected) return 'CLOCK · no port';
    if (!c.running) return 'CLOCK · no clock';
    // SPEC §3.2: bars count from the first tick until a MIDI Start sets the bar phase.
    return `CLOCK ${c.bpm?.toFixed(1) ?? '—'} BPM · locked${c.bar_synced ? '' : ' · bar phase not synced'}`;
  });
  let clockTitle = $derived(
    rt?.clock.source === 'resolume'
      ? `Following Resolume's tempo${rt.clock.synced ? '' : '; press Resync in Resolume to line up with its beat'} · click to change (Ctrl+,)`
      : (rt?.clock.port ?? 'no MIDI port selected') +
          (rt?.clock.bar_synced === false ? ' · bars count from the first tick until the clock sends MIDI Start' : '') +
          ' · click to choose (Ctrl+,)',
  );
  let current = $derived(app.run(app.selectedSeq) ?? rt?.running[0] ?? null);
  let notices = $derived((rt?.notices ?? []).slice(-3));
  function isProblem(n: string) {
    return /yielded|failed|rejected|not confirmed|no clock|lost|problem|cannot|matches no|disconnected|doesn't react/i.test(n);
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
  <span class="status-spacer"></span>
  <div class="chips sm">
    <button class="chip chip-btn" class:bad={!clockOk} title={clockTitle}
      onclick={() => { app.prefsTab = 'clock'; app.prefsOpen = true; }}>{clockText}</button>
    <span class="chip" class:bad={!rt?.rest} title="Resolume REST API">REST</span>
    <span class="chip" class:bad={!rt?.ws} title="Resolume WebSocket">WS</span>
    <!-- Red when sending fails here, or when Resolume ignores the triggers (UDP: sending alone can't tell). -->
    <span class="chip" class:bad={!rt?.osc || rt?.osc_unanswered}
      title={!rt?.osc ? 'OSC to Resolume: sending fails'
        : rt?.osc_unanswered ? "OSC to Resolume: sent, but Resolume doesn't react. Is OSC Input on in Resolume (Preferences → OSC), on the port the sequencer sends to?"
        : 'OSC to Resolume'}>OSC</span>
  </div>
</footer>

<style>
  .status-spacer { flex: 1 1 auto; }
  .chips.sm { gap: 4px; font-size: 10px; align-items: center; }
  .chips.sm .chip { padding: 1px 6px; gap: 4px; }
  .chips.sm .chip::before { width: 6px; height: 6px; }
  .statusbar-hint { background: none; border: 0; padding: 0; font: inherit; color: var(--accent-text); text-decoration: underline;
    cursor: pointer; }
  .statusbar-hint:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: var(--radius-sm); }
</style>
