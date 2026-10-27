<script lang="ts">
  // Console tab (docs/M12-desktop-shell.md, Phase E): the app's log, live while this tab is open.
  // Virtualised: only the rows in view (plus a margin) are in the DOM, so 5,000 lines stay smooth.
  import { tick } from 'svelte';
  import { app } from '../lib/app.svelte';
  import type { LogRecord } from '../lib/types';

  const ROW = 20; // px, fixed row height (see .log-row)
  const MARGIN = 20; // extra rows rendered above and below the viewport

  type Level = 'ERROR' | 'WARN' | 'INFO' | 'DEBUG';
  const LEVELS: Level[] = ['ERROR', 'WARN', 'INFO', 'DEBUG'];
  let shown = $state<Record<Level, boolean>>({ ERROR: true, WARN: true, INFO: true, DEBUG: true });
  let text = $state('');
  /** "Clear view": hide everything up to this record (client-side only). */
  let clearedSeq = $state(-1);

  let list: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let height = $state(400);
  /** Follow the newest line; scrolling up pauses it. */
  let follow = $state(true);
  let pausedAt = $state(0);
  let copied = $state(false);

  const level = (r: LogRecord): Level => (r.level === 'TRACE' ? 'DEBUG' : r.level);
  let rows = $derived.by(() => {
    const needle = text.trim().toLowerCase();
    return app.logs.filter(
      (r) =>
        r.seq > clearedSeq &&
        shown[level(r)] &&
        (!needle || r.message.toLowerCase().includes(needle) || r.target.toLowerCase().includes(needle)),
    );
  });
  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - MARGIN));
  let last = $derived(Math.min(rows.length, Math.ceil((scrollTop + height) / ROW) + MARGIN));
  let newWhilePaused = $derived(follow ? 0 : Math.max(0, rows.length - pausedAt));

  // Keep the newest line in view while following.
  $effect(() => {
    rows.length;
    if (follow && list) tick().then(() => list && (list.scrollTop = list.scrollHeight));
  });

  function onScroll() {
    if (!list) return;
    scrollTop = list.scrollTop;
    const atBottom = list.scrollTop + list.clientHeight >= list.scrollHeight - ROW;
    if (atBottom && !follow) follow = true;
    else if (!atBottom && follow) {
      follow = false;
      pausedAt = rows.length;
    }
  }

  function resume() {
    follow = true;
    if (list) list.scrollTop = list.scrollHeight;
  }

  const time = (ts: string) => ts.slice(11, 23);
  const line = (r: LogRecord) => `${r.ts} ${r.level.padEnd(5)} ${r.target}: ${r.message}`;

  async function copyVisible() {
    const body = rows.map(line).join('\n');
    try {
      await navigator.clipboard.writeText(body);
    } catch {
      const t = document.createElement('textarea');
      t.value = body;
      document.body.appendChild(t);
      t.select();
      document.execCommand('copy');
      t.remove();
    }
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  let header = $derived.by(() => {
    const v = app.version;
    const parts = v ? [`${v.name} ${v.version}`, `build ${v.build}`, v.commit] : [];
    parts.push(location.origin);
    return parts.join(' · ');
  });
</script>

<section class="console" aria-label="Console">
  <div class="console-head">
    <span class="mono console-info">{header}</span>
    {#if app.logStats.dropped > 0}
      <span class="tag pending mono" title="Log records that did not reach the file or this view">dropped: {app.logStats.dropped}</span>
    {/if}
  </div>
  <div class="console-tools">
    <div class="repeat" role="group" aria-label="Levels">
      <span>Levels</span>
      {#each LEVELS as l (l)}
        <button aria-pressed={shown[l]} onclick={() => (shown[l] = !shown[l])}>{l[0] + l.slice(1).toLowerCase()}</button>
      {/each}
    </div>
    <input class="console-filter" type="search" placeholder="Filter text or target" bind:value={text} aria-label="Filter" />
    <div class="spacer"></div>
    <button class="btn" onclick={() => (clearedSeq = app.logs.length ? app.logs[app.logs.length - 1].seq : -1)}>Clear view</button>
    <button class="btn" onclick={copyVisible}>{copied ? 'Copied' : 'Copy visible'}</button>
    <a class="btn console-link" href="/api/logs/current" download>Save log…</a>
    {#if app.desktop}
      <button class="btn" onclick={() => app.send({ cmd: 'shell', action: 'open_logs' })}>Open logs folder</button>
    {/if}
  </div>
  <div class="log-list mono" bind:this={list} bind:clientHeight={height} onscroll={onScroll} role="log" aria-live="off">
    <div style:height={`${rows.length * ROW}px`} style:position="relative">
      {#each rows.slice(first, last) as r, i (r.seq)}
        <div class="log-row lvl-{level(r).toLowerCase()}" style:top={`${(first + i) * ROW}px`}>
          <span class="log-ts">{time(r.ts)}</span>
          <span class="log-lvl">{r.level}</span>
          <span class="log-target">{r.target}</span>
          <span class="log-msg" title={r.message}>{r.message}</span>
        </div>
      {/each}
    </div>
    {#if !rows.length}
      <p class="meta console-empty">No log lines{text || clearedSeq >= 0 ? ' match' : ' yet'}.</p>
    {/if}
  </div>
  {#if !follow}
    <button class="btn console-paused" onclick={resume}>⏸ paused · {newWhilePaused} new</button>
  {/if}
</section>

<style>
  .console { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; gap: 10px; padding: 14px 16px; position: relative; }
  .console-head { display: flex; align-items: center; gap: 10px; }
  .console-info { font-size: 12px; color: var(--muted); }
  .console-tools { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .console-filter { min-height: 36px; min-width: 220px; padding: 0 10px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--control); color: var(--text); }
  .console-link { display: inline-flex; align-items: center; text-decoration: none; }
  .log-list { flex: 1 1 auto; min-height: 200px; height: calc(100vh - 200px); overflow-y: auto; border: 1px solid var(--line); border-radius: var(--radius); background: var(--track); font-size: 12px; }
  .log-row { position: absolute; left: 0; right: 0; height: 20px; line-height: 20px; display: flex; gap: 10px; padding: 0 10px; white-space: nowrap; }
  .log-ts { color: var(--faint); flex: 0 0 auto; }
  .log-lvl { flex: 0 0 44px; font-weight: 600; }
  .log-target { flex: 0 0 220px; overflow: hidden; text-overflow: ellipsis; color: var(--muted); }
  .log-msg { flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; }
  .lvl-error .log-lvl, .lvl-error .log-msg { color: var(--err); }
  .lvl-warn .log-lvl, .lvl-warn .log-msg { color: var(--accent); }
  .lvl-info .log-lvl, .lvl-info .log-msg { color: var(--text-2); }
  .lvl-debug .log-lvl, .lvl-debug .log-msg { color: var(--muted); }
  .console-empty { padding: 10px; }
  .console-paused { position: absolute; right: 28px; bottom: 26px; background: var(--accent-bg); border-color: var(--accent-line); color: var(--accent-text); }
</style>
