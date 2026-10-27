<script lang="ts">
  import { app } from './lib/app.svelte';
  import type { ShellAction } from './lib/types';
  import TopBar from './components/TopBar.svelte';
  import SequenceList from './components/SequenceList.svelte';
  import Editor from './components/Editor.svelte';
  import SidePanel from './components/SidePanel.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import SwitchModal from './components/SwitchModal.svelte';
  import Console from './components/Console.svelte';

  // Global shortcuts (SPEC §9.3). Inputs keep their own keys.
  function onKey(e: KeyboardEvent) {
    if (app.desktop && (e.ctrlKey || e.metaKey) && !e.altKey) {
      // Window shortcuts of the desktop shell; the native menu never sees keys typed in here.
      const keys: Record<string, ShellAction> = { '=': 'zoom_in', '+': 'zoom_in', '-': 'zoom_out', '0': 'zoom_reset', q: 'quit' };
      const action = keys[e.key.toLowerCase()];
      if (action) {
        e.preventDefault();
        app.send({ cmd: 'shell', action });
        return;
      }
    }
    if ((e.ctrlKey || e.metaKey) && !e.altKey && (e.key === '1' || e.key === '2')) {
      e.preventDefault();
      app.setTab(e.key === '1' ? 'sequencer' : 'console');
      return;
    }
    if (app.tab !== 'sequencer') return; // editor shortcuts only in the editor
    const t = e.target as HTMLElement;
    if (t.closest('input, select, textarea, [contenteditable]')) return;
    const view = app.selected;
    const deck = app.viewDeck;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
      e.preventDefault();
      app.send({ cmd: e.shiftKey ? 'redo' : 'undo' });
      return;
    }
    if (e.key === 'Escape') {
      app.selectedStep = null;
      app.relinking = false;
      return;
    }
    if (e.key === ' ' && view && deck) {
      e.preventDefault();
      app.toggle(view, deck);
      return;
    }
    const sel = app.selectedStep;
    if (!view || !sel) return;
    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      app.edit({ op: 'remove_step', seq: view.sequence.id, lane: sel.lane, index: sel.index });
      app.selectedStep = null;
    } else if (e.key.toLowerCase() === 'r') {
      app.edit({ op: 'insert_step', seq: view.sequence.id, lane: sel.lane, index: sel.index + 1, step: { clip: null, length: 8 } });
      app.selectedStep = { lane: sel.lane, index: sel.index + 1 };
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="app">
  <TopBar />
  {#if !app.connected}
    <div class="banner">Not connected to the sequencer app · retrying…</div>
  {:else if app.structure?.model_error}
    <div class="banner">Sequence file problem (read-only until fixed): {app.structure.model_error}</div>
  {/if}
  {#if app.tab === 'console'}
    <Console />
  {:else}
    <div class="body">
      <SequenceList />
      <Editor />
      <SidePanel />
    </div>
  {/if}
  <StatusBar />
</div>
<SwitchModal />
