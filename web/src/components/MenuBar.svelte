<script lang="ts">
  // Menu bar of the browser UI (M11 Phase F; the desktop app has the native menu instead, M12 decision).
  // Keyboard: Alt (pressed alone) or F10 enters it, ←/→ move between menus, ↓/Enter/Space open one,
  // ↑/↓/Home/End move inside it, Enter/Space activate, Esc closes (and leaves the bar), Tab leaves it.
  import { app } from '../lib/app.svelte';
  import Dialog from './Dialog.svelte';

  type Item = { label: string; hint?: string; run: () => void } | 'sep';
  interface Menu {
    label: string;
    items: Item[];
  }

  const external = (url: string) => () => window.open(url, '_blank', 'noopener');

  let menus = $derived<Menu[]>([
    {
      label: 'File',
      items: [
        { label: 'Reload composition', run: () => app.send({ cmd: 'reload_composition' }) },
        { label: 'Save now', run: () => app.send({ cmd: 'save_now' }) },
        { label: 'Preferences…', hint: 'Ctrl+,', run: () => (app.prefsOpen = true) },
        'sep',
        { label: 'Quit server…', run: () => app.openDialog('quit') },
      ],
    },
    {
      label: 'View',
      items: [
        { label: 'Zoom in', run: () => app.zoomUi(0.1) },
        { label: 'Zoom out', run: () => app.zoomUi(-0.1) },
        { label: 'Reset zoom', run: () => app.zoomUi(null) },
        'sep',
        {
          label: 'Toggle Console',
          hint: 'Ctrl+1 / Ctrl+2',
          run: () => app.setTab(app.tab === 'console' ? 'sequencer' : 'console'),
        },
      ],
    },
    {
      label: 'Help',
      items: [
        { label: 'Keyboard shortcuts', run: () => app.openDialog('shortcuts') },
        { label: 'User manual ↗', run: external(app.links.manual) },
        { label: 'Report an issue ↗', run: external(app.links.issues) },
        'sep',
        { label: 'Licenses', run: () => app.openDialog('licenses') },
        { label: 'About', run: () => app.openDialog('about') },
      ],
    },
  ]);

  let openIndex = $state<number | null>(null);
  let tops: HTMLButtonElement[] = $state([]);
  let itemEls: HTMLButtonElement[] = $state([]);
  let bar: HTMLDivElement | undefined = $state();
  /** Where focus was before Alt / F10 entered the bar (Esc returns there). */
  let before: HTMLElement | null = null;
  /** Alt went down with no other key or click since (Windows convention: Alt alone opens the menu bar). */
  let altAlone = false;

  function enter() {
    before = document.activeElement instanceof HTMLElement && !bar?.contains(document.activeElement) ? document.activeElement : null;
    tops[0]?.focus();
  }

  function leave() {
    openIndex = null;
    const target = before;
    before = null;
    if (target && target !== document.body && target.isConnected) target.focus();
    // Nothing focusable before (focus was on the page itself): just leave the bar.
    if (bar?.contains(document.activeElement)) (document.activeElement as HTMLElement).blur();
  }

  function openMenu(i: number, focus: 'first' | 'last' | 'none' = 'first') {
    openIndex = i;
    tops[i]?.focus();
    if (focus === 'none') return;
    queueMicrotask(() => {
      const list = itemEls.filter(Boolean);
      (focus === 'first' ? list[0] : list[list.length - 1])?.focus();
    });
  }

  function activate(item: Exclude<Item, 'sep'>, menu: number) {
    openIndex = null;
    tops[menu]?.focus(); // dialogs opened by the item return focus here
    before = null;
    item.run();
  }

  function onTopKey(e: KeyboardEvent, i: number) {
    const n = menus.length;
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const j = (i + (e.key === 'ArrowRight' ? 1 : n - 1)) % n;
      if (openIndex !== null) openMenu(j);
      else tops[j]?.focus();
    } else if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      openMenu(i);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      openMenu(i, 'last');
    } else if (e.key === 'Escape') {
      e.preventDefault();
      leave();
    } else if (e.key === 'Tab') {
      openIndex = null;
    }
  }

  function onItemKey(e: KeyboardEvent, menu: number) {
    const list = itemEls.filter(Boolean);
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    const n = menus.length;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const d = e.key === 'ArrowDown' ? 1 : list.length - 1;
      list[(at + d) % list.length]?.focus();
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      (e.key === 'Home' ? list[0] : list[list.length - 1])?.focus();
    } else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      openMenu((menu + (e.key === 'ArrowRight' ? 1 : n - 1)) % n);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      openIndex = null;
      tops[menu]?.focus();
    } else if (e.key === 'Tab') {
      openIndex = null;
    }
  }

  function onWindowKeyDown(e: KeyboardEvent) {
    if (e.key === 'Alt') {
      altAlone = !e.repeat;
      return;
    }
    altAlone = false;
    if (e.key === 'F10' && !e.shiftKey && !e.ctrlKey && !e.altKey && !app.dialog && !app.prefsOpen) {
      e.preventDefault();
      if (bar?.contains(document.activeElement)) leave();
      else enter();
    }
  }

  function onWindowKeyUp(e: KeyboardEvent) {
    if (e.key !== 'Alt' || !altAlone) return;
    altAlone = false;
    if (app.dialog || app.prefsOpen) return;
    e.preventDefault();
    if (bar?.contains(document.activeElement)) leave();
    else enter();
  }

  function onWindowPointerDown(e: PointerEvent) {
    altAlone = false; // Alt+drag duplicates a step: not a menu gesture
    if (openIndex !== null && !bar?.contains(e.target as Node)) openIndex = null;
  }

  function quit() {
    app.send({ cmd: 'shell', action: 'quit' });
    app.dialog = null;
  }
</script>

<svelte:window onkeydown={onWindowKeyDown} onkeyup={onWindowKeyUp} onpointerdown={onWindowPointerDown} onblur={() => (altAlone = false)} />

<div class="mb-bar" role="menubar" aria-label="Main menu" tabindex="-1" bind:this={bar}>
  {#each menus as m, i (m.label)}
    <div class="mb-wrap">
      <button class="mb-top" role="menuitem" aria-haspopup="menu" aria-expanded={openIndex === i}
        tabindex={i === 0 ? 0 : -1} bind:this={tops[i]}
        onclick={() => (openIndex === i ? (openIndex = null) : openMenu(i, 'none'))}
        onpointerenter={() => openIndex !== null && openIndex !== i && openMenu(i, 'none')}
        onkeydown={(e) => onTopKey(e, i)}>{m.label}</button>
      {#if openIndex === i}
        <div class="mb-pop" role="menu" aria-label={m.label}>
          {#each m.items as it, k (k)}
            {#if it === 'sep'}
              <div class="mb-sep" role="separator"></div>
            {:else}
              <button class="mb-item" role="menuitem" tabindex="-1"
                bind:this={itemEls[m.items.slice(0, k).filter((x) => x !== 'sep').length]}
                onclick={() => activate(it, i)} onkeydown={(e) => onItemKey(e, i)}>
                <span>{it.label}</span>{#if it.hint}<span class="mb-hint">{it.hint}</span>{/if}
              </button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</div>

{#if app.dialog === 'quit'}
  <Dialog title="Quit the sequencer server?" onclose={() => (app.dialog = null)}>
    <p class="quit-text">
      Running sequences end as set in each sequence, pending edits are saved, and this page loses its connection.
      Start the app again to continue.
    </p>
    {#snippet actions()}
      <button class="btn" onclick={() => (app.dialog = null)}>Cancel</button>
      <button class="btn danger" onclick={quit}>Quit server</button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .mb-bar { display: flex; gap: 2px; padding: 2px 8px; background: var(--panel); border-bottom: 1px solid var(--line-soft); font-size: 12px; }
  .mb-wrap { position: relative; }
  .mb-top { padding: 4px 10px; border: 0; border-radius: 4px; background: transparent; color: var(--text-2); font: inherit; cursor: pointer; }
  .mb-top:hover, .mb-top[aria-expanded="true"], .mb-top:focus-visible { background: var(--control); color: var(--text); }
  .mb-top:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .mb-pop { position: absolute; top: calc(100% + 2px); left: 0; z-index: 20; min-width: 220px; padding: 4px; display: flex; flex-direction: column;
    background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); box-shadow: 0 8px 24px rgba(0,0,0,.45); }
  .mb-item { display: flex; justify-content: space-between; gap: 24px; padding: 6px 10px; border: 0; border-radius: 4px; background: transparent;
    color: var(--text); font: inherit; text-align: left; cursor: pointer; }
  .mb-item:hover, .mb-item:focus-visible { background: var(--accent-bg); color: var(--accent-text); outline: none; }
  .mb-hint { color: var(--muted); font-family: var(--mono); font-size: 11px; }
  .mb-sep { height: 1px; margin: 4px 6px; background: var(--line); }
  .quit-text { margin: 0; color: var(--text-2); }
</style>
