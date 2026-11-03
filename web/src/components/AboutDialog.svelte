<script lang="ts">
  // Help → About (M11 Phase F): version and build from /api/version, license, links, disclaimers.
  import { app } from '../lib/app.svelte';
  import Dialog from './Dialog.svelte';

  let v = $derived(app.version);
  let links = $derived(app.links);
  let date = $derived(v?.build_date ? v.build_date.slice(0, 10) : '');
</script>

<Dialog title="About" onclose={() => (app.dialog = null)}>
  <div class="about-head">
    <img src="/icon.png" alt="" width="64" height="64" />
    <div>
      <div class="about-name">{v?.name ?? 'Clip Sequencer'} <span class="about-for">for Resolume</span></div>
      <div class="mono about-version">
        {#if v}
          Version {v.version} · build {v.build} · {v.commit} · {date}
        {:else}
          Version unknown (not connected)
        {/if}
      </div>
    </div>
  </div>
  <p>{v?.copyright ?? 'Copyright (c) 2026 Ulrich Staudinger'}</p>
  <p>
    Released under the
    <button class="linkish" onclick={() => app.openDialog('licenses')}>MIT License</button>.
    Provided as is, without warranty of any kind.
  </p>
  <ul class="about-links">
    <li><a href={links.site} target="_blank" rel="noopener">Project site</a></li>
    <li><a href={links.manual} target="_blank" rel="noopener">User manual</a></li>
    <li><a href={links.repo} target="_blank" rel="noopener">Source code on GitHub</a></li>
    <li><a href={links.issues} target="_blank" rel="noopener">Report an issue</a></li>
  </ul>
  <p class="meta">
    Not affiliated with or endorsed by Resolume BV. Resolume is a trademark of Resolume BV.
  </p>
  {#if v}
    <p class="meta mono">{v.target} · built {v.build_date}</p>
  {/if}
</Dialog>

<style>
  .about-head { display: flex; gap: 14px; align-items: center; }
  .about-head img { border-radius: 12px; flex: 0 0 64px; }
  .about-name { font-size: 18px; font-weight: 600; }
  .about-for { font-weight: 400; color: var(--muted); }
  .about-version { font-size: 12px; color: var(--text-2); margin-top: 4px; }
  p { margin: 0; color: var(--text-2); }
  .about-links { margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 4px; }
  a, .linkish { color: var(--accent-text); }
  .linkish { background: none; border: 0; padding: 0; font: inherit; text-decoration: underline; cursor: pointer; }
</style>
