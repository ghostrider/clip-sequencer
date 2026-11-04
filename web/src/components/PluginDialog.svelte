<script lang="ts">
  // File → Install FFGL plugin… (SPEC §12.1, M11 Phase G3): installs, updates or removes the bundled
  // "1000A Sequence" plugin in Resolume's plugin folder. The app does the file work (GET/POST /api/ffgl…).
  import { app } from '../lib/app.svelte';
  import type { DllVersion, FfglResult, FfglStatus } from '../lib/types';
  import Dialog from './Dialog.svelte';

  let status = $state<FfglStatus | null>(null);
  let busy = $state(false);
  let message = $state<string | null>(null);
  let error = $state<string | null>(null);
  let dirText = $state('');
  let editingDir = $state(false);
  let confirmDowngrade = $state(false);

  const ver = (v: DllVersion | null | undefined) => (v ? `${v.major}.${v.minor}.${v.patch} (build ${v.build})` : 'unknown version');

  function show(s: FfglStatus) {
    status = s;
    if (!editingDir) dirText = s.dir ?? '';
    if (s.action !== 'downgrade') confirmDowngrade = false;
  }

  async function load() {
    error = null;
    try {
      const r = await fetch('/api/ffgl');
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      show(await r.json());
    } catch (e) {
      error = `Could not read the plugin status: ${e}`;
    }
  }

  async function post(path: string, body: unknown) {
    busy = true;
    message = null;
    error = null;
    try {
      const r = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      const v: FfglResult = await r.json();
      show(v.status);
      if (v.ok) message = v.message ?? 'Done.';
      else error = v.error ?? 'Failed.';
    } catch (e) {
      error = `Request failed: ${e}`;
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    load();
    // Closing the dialog refreshes the status-bar hint (it disappears once the plugin is installed).
    return () => app.checkFfgl();
  });

  function primary() {
    if (!status?.action) return;
    if (status.action === 'downgrade' && !confirmDowngrade) {
      confirmDowngrade = true;
      return;
    }
    post('/api/ffgl/install', { allow_downgrade: status.action === 'downgrade' });
    confirmDowngrade = false;
  }

  function saveDir(dir: string | null) {
    editingDir = false;
    post('/api/ffgl/dir', { dir });
  }

  let label = $derived.by(() => {
    switch (status?.action) {
      case 'install': return 'Install';
      case 'update': return 'Update';
      case 'reinstall': return 'Reinstall';
      case 'downgrade': return confirmDowngrade ? 'Downgrade anyway' : 'Downgrade…';
      default: return 'Install';
    }
  });
  let installed = $derived(status?.installed ?? null);
  let blocked = $derived(!!status?.resolume_running);
</script>

<Dialog title="Install FFGL plugin" onclose={() => (app.dialog = null)}>
  {#if !status}
    <p class="meta">{error ?? 'Reading…'}</p>
  {:else}
    <p class="ffgl-intro">
      <strong>{status.plugin_name}</strong> shows a sequence's timeline on its <span class="mono">§</span> clip in
      Resolume (optional). The app copies it into Resolume's plugin folder.
    </p>

    <dl class="ffgl-facts">
      <dt>In this app</dt>
      <dd>{status.bundled_version ? ver(status.bundled_version) : 'not included in this build'}</dd>
      <dt>Plugin folder</dt>
      <dd>
        {#if editingDir}
          <input class="ffgl-dir mono" type="text" bind:value={dirText} aria-label="Plugin folder"
            onkeydown={(e) => { if (e.key === 'Enter') saveDir(dirText); if (e.key === 'Escape') { e.stopPropagation(); editingDir = false; dirText = status?.dir ?? ''; } }} />
          <span class="ffgl-row">
            <button class="btn" onclick={() => saveDir(dirText)} disabled={busy}>Save</button>
            <button class="btn" onclick={() => saveDir(null)} disabled={busy}>Use default</button>
            <button class="btn" onclick={() => { editingDir = false; dirText = status?.dir ?? ''; }}>Cancel</button>
          </span>
        {:else}
          <span class="mono ffgl-path">{status.dir ?? 'unknown (no Documents folder)'}</span>
          <span class="meta">
            {status.dir_source === 'command_line' ? '(--ffgl-dir)' : status.dir_is_default ? '(default)' : '(chosen)'}
            {#if status.dir && !status.dir_exists} · does not exist yet; it is created on install{/if}
          </span>
          <span class="ffgl-row">
            <button class="btn" onclick={() => (editingDir = true)} disabled={busy}>Change…</button>
            {#if app.desktop && status.dir_exists}
              <button class="btn" onclick={() => app.send({ cmd: 'shell', action: 'open_ffgl_dir' })}>Open folder</button>
            {/if}
          </span>
        {/if}
      </dd>
      <dt>Installed</dt>
      <dd>
        {#if !installed}not installed
        {:else if !installed.ours}<span class="ffgl-warn">another SeqView.dll is there; it is left alone</span>
        {:else}{ver(installed.version)}{/if}
      </dd>
    </dl>

    {#if blocked}
      <p class="ffgl-warn" role="status">Quit Resolume Arena to install the plugin: Resolume locks loaded plugins.
        <button class="btn" onclick={load} disabled={busy}>Check again</button></p>
    {/if}
    {#if status.action === 'downgrade'}
      <p class="ffgl-warn">The installed plugin is newer than the one in this app. Installing replaces it with the older version.</p>
    {/if}
    {#if message}<p class="ffgl-ok" role="status">{message}</p>{/if}
    {#if error}<p class="ffgl-warn" role="alert">{error}</p>{/if}
    {#if !status.can_detect_resolume}
      <p class="meta">This system can't tell whether Resolume runs: make sure it is closed.</p>
    {/if}
  {/if}

  {#snippet actions()}
    {#if installed?.ours}
      <button class="btn" onclick={() => post('/api/ffgl/remove', {})} disabled={busy || blocked}>Remove</button>
    {/if}
    <div class="ffgl-spacer"></div>
    <button class="btn" onclick={() => (app.dialog = null)}>Close</button>
    <button class="btn primary" onclick={primary}
      disabled={busy || blocked || !status?.action || (installed !== null && !installed.ours)}>{busy ? 'Working…' : label}</button>
  {/snippet}
</Dialog>

<style>
  .ffgl-intro { margin: 0; color: var(--text-2); }
  .ffgl-facts { display: grid; grid-template-columns: max-content 1fr; gap: 8px 14px; margin: 0; }
  .ffgl-facts dt { color: var(--muted); }
  .ffgl-facts dd { margin: 0; display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .ffgl-path { word-break: break-all; }
  .ffgl-row { display: flex; gap: 8px; flex-wrap: wrap; }
  .ffgl-dir { width: 100%; background: var(--control); color: var(--text); border: 1px solid var(--line-strong); border-radius: var(--radius); padding: 6px 8px; }
  .ffgl-warn { color: var(--err-text); margin: 0; }
  .ffgl-ok { color: var(--ok-text); margin: 0; }
  .ffgl-spacer { flex: 1; }
</style>
