<script lang="ts">
  import { app, deckKey } from '../lib/app.svelte';

  let rt = $derived(app.runtime);
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
    return `CLOCK ${c.bpm?.toFixed(1) ?? '—'} BPM · locked`;
  });
  let clockTitle = $derived(
    rt?.clock.source === 'resolume'
      ? `Following Resolume's tempo${rt.clock.synced ? '' : '; press Resync in Resolume to line up with its beat'} · click to change (Ctrl+,)`
      : (rt?.clock.port ?? 'no MIDI port selected') + ' · click to choose (Ctrl+,)',
  );
</script>

<header class="topbar">
  <div class="logo" aria-label="§ SEQ"><span class="mark">§</span>SEQ</div>
  <div class="tabs view-tabs" role="tablist" aria-label="View">
    <button class="tab" role="tab" aria-selected={app.tab === 'sequencer'} title="Ctrl+1"
      onclick={() => app.setTab('sequencer')}>Sequencer</button>
    <button class="tab" role="tab" aria-selected={app.tab === 'console'} title="Ctrl+2"
      onclick={() => app.setTab('console')}>
      Console{#if app.newErrors > 0}<span class="error-badge" aria-label="{app.newErrors} new errors">{app.newErrors}</span>{/if}
    </button>
  </div>
  <div>
    <span style="color:var(--muted)">Composition</span>
    <span class="mono">{app.structure?.composition ?? '—'}</span>
  </div>
  <div class="tabs" role="tablist" aria-label="Decks" hidden={app.tab !== 'sequencer'}>
    {#each app.decks as d (deckKey(d))}
      <button
        class="tab"
        class:orphaned={d.orphaned}
        class:ambiguous={d.ambiguous}
        role="tab"
        aria-selected={app.viewDeck === d}
        title={d.orphaned ? 'Not in Resolume: sequences are orphaned' : d.ambiguous ? `Several decks are named "${d.name}" and share their sequences. Rename them in Resolume.` : d.clips_source === 'cached' ? 'Clips cached from the last time this deck was live' : ''}
        onclick={() => {
          app.viewDeckKey = deckKey(d);
          app.selectedStep = null;
          if (!d.sequences.some((s) => s.sequence.id === app.selectedSeq)) app.selectedSeq = d.sequences[0]?.sequence.id ?? null;
        }}
      >
        {d.name}{#if d.ambiguous}<span class="ambiguous-badge">same name</span>{/if}{#if d.live}<span class="live-badge">LIVE</span>{/if}
      </button>
    {/each}
  </div>
  <div class="spacer"></div>
  <div class="chips">
    <button class="chip chip-btn" class:bad={!clockOk} title={clockTitle}
      onclick={() => { app.prefsTab = 'clock'; app.prefsOpen = true; }}>{clockText}</button>
    <span class="chip" class:bad={!rt?.rest}>REST</span>
    <span class="chip" class:bad={!rt?.ws}>WS</span>
    <span class="chip" class:bad={!rt?.osc}>OSC</span>
  </div>
  <button class="btn danger" onclick={() => app.send({ cmd: 'stop_all' })}>Stop all</button>
</header>
