<script lang="ts">
  // Modal dialog shell (M11 Phase F): title, close button, Esc closes, focus stays inside (Tab wraps) and
  // returns to the element that had it when the dialog opened (the menu bar puts it on its menu button).
  import type { Snippet } from 'svelte';

  let {
    title,
    onclose,
    wide = false,
    children,
    actions,
  }: { title: string; onclose: () => void; wide?: boolean; children: Snippet; actions?: Snippet } = $props();

  const id = `dlg-${Math.random().toString(36).slice(2, 8)}`;
  let box: HTMLDivElement | undefined = $state();

  $effect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // First focusable control, else the dialog itself.
    queueMicrotask(() => (focusables()[0] ?? box)?.focus());
    return () => {
      if (opener?.isConnected) opener.focus();
    };
  });

  function focusables(): HTMLElement[] {
    if (!box) return [];
    return [...box.querySelectorAll<HTMLElement>(
      'a[href], button:not([disabled]), input:not([disabled]), select, textarea, summary, [tabindex]:not([tabindex="-1"])',
    )].filter((el) => el.offsetParent !== null || el === document.activeElement);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose();
      return;
    }
    if (e.key !== 'Tab') return;
    const list = focusables();
    if (!list.length) {
      e.preventDefault();
      return;
    }
    const first = list[0];
    const last = list[list.length - 1];
    if (e.shiftKey && (document.activeElement === first || document.activeElement === box)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
</script>

<div class="modal-backdrop open" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal info-dialog" class:wide role="dialog" aria-modal="true" aria-labelledby={id} tabindex="-1"
    bind:this={box} {onkeydown}>
    <div class="dialog-head">
      <h2 id={id}>{title}</h2>
      <button class="icon-btn dialog-close" aria-label="Close" title="Close (Esc)" onclick={onclose}>✕</button>
    </div>
    <div class="dialog-body">
      {@render children()}
    </div>
    {#if actions}
      <div class="actions">{@render actions()}</div>
    {/if}
  </div>
</div>

<style>
  .info-dialog { width: min(520px, 92vw); max-height: 86vh; outline: none; }
  .info-dialog.wide { width: min(820px, 94vw); }
  .dialog-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .dialog-close { width: 32px; height: 32px; flex-basis: 32px; color: var(--muted); }
  .dialog-close:hover { color: var(--text); }
  .dialog-body { overflow-y: auto; min-height: 0; display: flex; flex-direction: column; gap: 12px; }
</style>
