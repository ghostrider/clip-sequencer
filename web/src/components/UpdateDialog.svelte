<script lang="ts">
  // In-app update (SPEC §15): the result of Help → Check for updates…, the question after the start-up check
  // (Download and install / Skip this version / Later) and the install's progress. State from the app (`update`).
  import { app } from '../lib/app.svelte';
  import Dialog from './Dialog.svelte';

  let u = $derived(app.update);
  let result = $derived(u?.result ?? null);
  let phase = $derived(u?.phase ?? null);
  let release = $derived(result?.state === 'available' ? result.release : null);
  let ask = $derived(!!u?.ask);
  // Sequences running or waiting block an install (the app refuses it too).
  let busy = $derived((app.runtime?.running.length ?? 0) > 0 || (app.runtime?.pending?.length ?? 0) > 0);
  let blockedWhy = $derived(sentence(u?.blocked) ?? (busy ? 'Stop all sequences first.' : null));
  let working = $derived(phase?.state === 'downloading' || phase?.state === 'restarting');

  // The app's reasons are written to follow "The update did not work: "; shown on their own they start a sentence.
  function sentence(s: string | null | undefined) {
    if (!s) return null;
    return s[0].toUpperCase() + s.slice(1) + (/[.!?]$/.test(s) ? '' : '.');
  }

  function mb(n: number) {
    return (n / 1_048_576).toFixed(1);
  }

  function close() {
    if (working) return;
    if (ask) app.send({ cmd: 'update_answer', skip: false }); // closing the question = Later
    app.updateOpen = false;
  }
  function later() {
    app.send({ cmd: 'update_answer', skip: false });
    app.updateOpen = false;
  }
  function skip() {
    app.send({ cmd: 'update_answer', skip: true });
    app.updateOpen = false;
  }
</script>

<Dialog title="Update" onclose={close}>
  {#if phase?.state === 'downloading'}
    {@const pct = phase.total ? Math.min(100, (phase.done / phase.total) * 100) : 0}
    <p>
      {#if phase.total && phase.done >= phase.total}Verifying…{:else}Downloading {mb(phase.done)}{phase.total ? ` / ${mb(phase.total)}` : ''} MB{/if}
    </p>
    <div class="update-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(pct)}>
      <div style:width="{pct}%"></div>
    </div>
  {:else if phase?.state === 'restarting'}
    <p>Restarting with the new version…</p>
  {:else if phase?.state === 'failed'}
    <p class="bad">The update did not work: {phase.message}</p>
    <p class="meta">You can download the release yourself and unpack it over this folder.</p>
  {:else if u?.checking && !release}
    <p>Checking for updates…</p>
  {:else if result?.state === 'up_to_date'}
    <p>Clip Sequencer {result.current} is up to date.</p>
  {:else if result?.state === 'error'}
    <p class="bad">The update check failed: {result.message}</p>
  {:else if release && result?.state === 'available'}
    <p><strong>Version {release.version} is available</strong> (you have {result.current}).</p>
    {#if release.notes}<pre class="update-notes">{release.notes}</pre>{/if}
    {#if blockedWhy}<p class="meta bad">{blockedWhy}</p>{/if}
    <p class="meta">The app downloads the release, checks it, quits and starts the new version; your sequences and
      settings stay as they are. If the new version doesn't start, the old one comes back.</p>
  {/if}

  {#snippet actions()}
    {#if release && !working && phase?.state !== 'failed'}
      {#if ask}
        <button class="btn" onclick={skip}>Skip this version</button>
        <button class="btn" onclick={later}>Later</button>
      {/if}
      <a class="btn link-btn" href={release.page} target="_blank" rel="noopener">Release page</a>
      <div class="spacer"></div>
      <button class="btn primary" disabled={!!blockedWhy} title={blockedWhy ?? ''}
        onclick={() => app.send({ cmd: 'install_update' })}>Download and install</button>
    {:else}
      {#if result?.state === 'error' || phase?.state === 'failed'}
        <a class="btn link-btn" href={u?.releases_page} target="_blank" rel="noopener">Release page</a>
      {/if}
      <div class="spacer"></div>
      {#if !working}<button class="btn" onclick={close}>Close</button>{/if}
    {/if}
  {/snippet}
</Dialog>

<style>
  p, pre { margin: 0; }
  .update-notes { max-height: 240px; overflow: auto; white-space: pre-wrap; font: 12px/1.5 var(--mono); background: var(--control); border: 1px solid var(--line); border-radius: var(--radius); padding: 8px 10px; }
  .update-bar { height: 6px; background: var(--control); border-radius: 3px; overflow: hidden; }
  .update-bar > div { height: 100%; background: var(--accent); transition: width 160ms ease-out; }
  .bad { color: var(--err-text); }
  .link-btn { display: inline-flex; align-items: center; text-decoration: none; }
</style>
