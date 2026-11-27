<script lang="ts">
  // Length and blend mode of a transition (SPEC §7.7, §9.3, §9.4): the lane default chip and the inspector.
  import { FADE_LENGTHS, fadeLength } from '../lib/fade';
  import type { Transition } from '../lib/types';

  let {
    transition,
    options,
    onchange,
    compact = false,
  }: { transition: Transition; options: string[]; onchange: (t: Transition) => void; compact?: boolean } = $props();

  // A length or blend from the file that isn't in the lists stays selectable.
  let lengths = $derived(FADE_LENGTHS.includes(transition.length) ? FADE_LENGTHS : [...FADE_LENGTHS, transition.length].sort((a, b) => a - b));
  let blends = $derived(transition.blend && !options.includes(transition.blend) ? [transition.blend, ...options] : options);
</script>

<select class="pill-select mono fade-pick" class:compact aria-label="Fade length" title="Fade length: the clip fades in over this time (cut: at once)"
  value={transition.length}
  onchange={(e) => onchange({ ...transition, length: Number((e.currentTarget as HTMLSelectElement).value) })}>
  {#each lengths as n (n)}<option value={n}>{n === 0 ? 'cut' : `↗ ${fadeLength(n)}`}</option>{/each}
</select>
{#if transition.length > 0}
  <select class="pill-select fade-pick" class:compact aria-label="Blend mode" title="Blend mode of the fade; the layer's own if not set"
    value={transition.blend ?? ''}
    onchange={(e) => {
      const v = (e.currentTarget as HTMLSelectElement).value;
      onchange({ length: transition.length, ...(v ? { blend: v } : {}) });
    }}>
    <option value="">layer's blend</option>
    {#each blends as b (b)}<option value={b}>{b}</option>{/each}
  </select>
{/if}
