<script lang="ts">
  import { app } from '../lib/app.svelte';

  let rt = $derived(app.runtime);
  let clockOk = $derived(!!rt?.clock.connected && !!rt?.clock.running);
  let clockText = $derived.by(() => {
    if (!rt) return 'CLOCK';
    if (!rt.clock.connected) return 'CLOCK · no port';
    if (!rt.clock.running) return 'CLOCK · no clock';
    return `CLOCK ${rt.clock.bpm?.toFixed(1) ?? '—'} BPM · locked`;
  });
</script>

<header class="topbar">
  <div class="logo">§ SEQ</div>
  <div>
    <span style="color:var(--muted)">Composition</span>
    <span class="mono">{app.structure?.composition ?? '—'}</span>
  </div>
  <div class="tabs" role="tablist" aria-label="Decks">
    {#each app.decks as d (d.name)}
      <button
        class="tab"
        class:orphaned={d.orphaned}
        role="tab"
        aria-selected={app.viewDeck?.name === d.name}
        title={d.orphaned ? 'Not in Resolume: sequences are orphaned' : d.clips_source === 'cached' ? 'Clips cached from the last time this deck was live' : ''}
        onclick={() => {
          app.viewDeckName = d.name;
          app.selectedStep = null;
          if (!d.sequences.some((s) => s.sequence.id === app.selectedSeq)) app.selectedSeq = d.sequences[0]?.sequence.id ?? null;
        }}
      >
        {d.name}{#if d.live}<span class="live-badge">LIVE</span>{/if}
      </button>
    {/each}
  </div>
  <div class="spacer"></div>
  <div class="chips">
    <span class="chip" class:bad={!clockOk} title={rt?.clock.port ?? 'no MIDI port configured'}>{clockText}</span>
    <span class="chip" class:bad={!rt?.rest}>REST</span>
    <span class="chip" class:bad={!rt?.ws}>WS</span>
    <span class="chip" class:bad={!rt?.osc}>OSC</span>
  </div>
  <button class="btn danger" onclick={() => app.send({ cmd: 'stop_all' })}>Stop all</button>
</header>
