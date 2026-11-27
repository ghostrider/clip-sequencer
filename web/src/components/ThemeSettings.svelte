<script lang="ts">
  // Preferences → Appearance (SPEC §9.7): pick a theme (shown at once as a preview), change its colours and
  // fonts, save the result as a new theme or over a saved one, delete saved themes. The dialog's Save makes
  // the selected theme the one in use; Cancel goes back to it.
  import { app } from '../lib/app.svelte';
  import {
    BUILTIN_THEMES, COLOR_GROUPS, COLOR_TOKENS, DEFAULT_THEME_ID, FONTS, FONT_ROLES, HEX, applyTheme, fromCustom,
    newThemeId, resolveTheme, type Theme,
  } from '../lib/themes';

  let { selected = $bindable(), dirty = $bindable() }: { selected: string; dirty: boolean } = $props();

  let themes = $derived<Theme[]>([...BUILTIN_THEMES, ...app.appearance.custom_themes.map(fromCustom)]);
  let current = $derived(resolveTheme(selected, app.appearance.custom_themes));
  /** Unsaved changes to the selected theme (`null` = none). */
  let draft = $state<Theme | null>(null);
  const nameFor = (t: Theme) => (t.builtin ? `${t.name} (custom)` : t.name);
  let draftName = $state(nameFor(resolveTheme(app.appearance.theme, app.appearance.custom_themes)));
  let editing = $state(false);
  let confirmDelete = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    dirty = draft !== null;
  });
  // Preview: what the dialog shows is applied to the whole app until Save or Cancel.
  $effect(() => {
    applyTheme(draft ?? current);
  });

  function pick(id: string) {
    selected = id;
    draft = null;
    draftName = nameFor(resolveTheme(id, app.appearance.custom_themes));
    confirmDelete = false;
    error = null;
  }

  function edit() {
    editing = !editing;
  }

  function change(fn: (t: Theme) => void) {
    const t: Theme = structuredClone($state.snapshot(draft ?? current)) as Theme;
    fn(t);
    draft = t;
  }

  function setColor(token: (typeof COLOR_TOKENS)[number], value: string) {
    const v = value.trim();
    if (!HEX.test(v)) return;
    change((t) => (t.colors[token] = v.toUpperCase()));
  }

  /** Native colour pickers have no alpha: keep the token's alpha when it has one. */
  function fromPicker(token: (typeof COLOR_TOKENS)[number], rgb: string) {
    const old = (draft ?? current).colors[token];
    setColor(token, rgb + (old.length === 9 ? old.slice(7) : ''));
  }

  function save(asNew: boolean) {
    const t = draft ?? current;
    const name = draftName.trim();
    if (!name) {
      error = 'Give the theme a name.';
      return;
    }
    if (name.length > 64) {
      error = 'The name is too long (64 characters at most).';
      return;
    }
    const id = asNew || t.builtin ? newThemeId() : t.id;
    const theme = { id, name, base: t.base, colors: { ...t.colors }, fonts: { ...t.fonts } };
    app.send({ cmd: 'save_theme', theme, select: true });
    // Shown at once; the app's answer brings the same list.
    const list = app.appearance.custom_themes.filter((c) => c.id !== id);
    app.appearance = { ...app.appearance, theme: id, custom_themes: [...list, theme] };
    selected = id;
    draft = null;
    error = null;
  }

  function discard() {
    draft = null;
    draftName = nameFor(current);
    error = null;
  }

  function remove() {
    if (current.builtin) return;
    const id = current.id;
    app.send({ cmd: 'delete_theme', id });
    app.appearance = {
      ...app.appearance,
      theme: app.appearance.theme === id ? null : app.appearance.theme,
      custom_themes: app.appearance.custom_themes.filter((c) => c.id !== id),
    };
    pick(DEFAULT_THEME_ID);
  }

  const swatchTokens = ['bg', 'surface', 'accent', 'info-text', 'ok', 'err'] as const;
  let shown = $derived(draft ?? current);
  const fontStack = (id: string) => FONTS.find((f) => f.id === id)?.stack ?? 'inherit';
</script>

<div class="themes" role="radiogroup" aria-label="Theme">
  {#each themes as t (t.id)}
    <button class="theme-card" role="radio" aria-checked={t.id === selected} onclick={() => pick(t.id)}
      style:--c-bg={t.colors.bg} style:--c-panel={t.colors.panel} style:--c-text={t.colors.text}
      style:--c-muted={t.colors.muted} style:--c-accent={t.colors.accent}>
      <span class="theme-sample">
        <span class="theme-aa" style:font-family={fontStack(t.fonts.head)}>Aa</span>
        <span class="theme-play"></span>
        <span class="theme-mono" style:font-family={fontStack(t.fonts.mono)}>3.3</span>
      </span>
      <span class="theme-swatches">
        {#each swatchTokens as k (k)}<i style:background={t.colors[k]}></i>{/each}
      </span>
      <span class="theme-name">
        {t.name}
        {#if t.id === DEFAULT_THEME_ID}<span class="meta">default</span>{/if}
        {#if !t.builtin}<span class="meta">saved</span>{/if}
        {#if t.id === selected && draft}<span class="theme-edited">edited</span>{/if}
      </span>
    </button>
  {/each}
</div>

<div class="theme-actions">
  <button class="btn" onclick={edit} aria-expanded={editing}>{editing ? 'Hide colours and fonts' : 'Change colours and fonts…'}</button>
  {#if !current.builtin}
    {#if confirmDelete}
      <span class="meta">Delete “{current.name}”?</span>
      <button class="btn danger" onclick={remove}>Delete</button>
      <button class="btn" onclick={() => (confirmDelete = false)}>Keep</button>
    {:else}
      <button class="btn" onclick={() => (confirmDelete = true)}>Delete theme…</button>
    {/if}
  {/if}
</div>

{#if editing}
  <div class="theme-editor">
    <p class="meta">
      Changes show at once. {current.builtin ? 'Built-in themes stay as they are: save your changes as a new theme.' : ''}
      Corners and label style come from the theme it was made from.
    </p>

    <fieldset class="theme-group">
      <legend class="caps">Fonts</legend>
      <div class="theme-fonts">
        {#each FONT_ROLES as [role, label] (role)}
          <label class="theme-field">
            <span>{label}</span>
            <select value={shown.fonts[role]} style:font-family={fontStack(shown.fonts[role])}
              onchange={(e) => { const v = e.currentTarget.value; change((t) => (t.fonts[role] = v as typeof t.fonts[typeof role])); }}>
              <optgroup label="Sans">
                {#each FONTS.filter((f) => !f.mono) as f (f.id)}<option value={f.id}>{f.name}</option>{/each}
              </optgroup>
              <optgroup label="Monospace">
                {#each FONTS.filter((f) => f.mono) as f (f.id)}<option value={f.id}>{f.name}</option>{/each}
              </optgroup>
            </select>
          </label>
        {/each}
      </div>
    </fieldset>

    {#each COLOR_GROUPS as g (g.label)}
      <fieldset class="theme-group">
        <legend class="caps">{g.label}</legend>
        <div class="theme-colors">
          {#each g.tokens as [token, label] (token)}
            <label class="theme-field color">
              <input type="color" value={shown.colors[token].slice(0, 7)} aria-label="{label} colour"
                oninput={(e) => fromPicker(token, e.currentTarget.value)} />
              <span class="theme-label">{label}</span>
              <input class="hex mono" type="text" value={shown.colors[token]} maxlength="9" spellcheck="false"
                aria-label="{label} hex value" title="#RRGGBB or #RRGGBBAA"
                onchange={(e) => setColor(token, e.currentTarget.value)} />
            </label>
          {/each}
        </div>
      </fieldset>
    {/each}

    <div class="theme-save">
      <label class="theme-field name">
        <span>Theme name</span>
        <input type="text" bind:value={draftName} maxlength="64" placeholder="e.g. Club night" />
      </label>
      {#if !current.builtin}
        <button class="btn" onclick={() => save(false)} disabled={!draft}>Save changes</button>
      {/if}
      <button class="btn primary" onclick={() => save(true)} disabled={!draft && current.builtin}>Save as new theme</button>
      <button class="btn" onclick={discard} disabled={!draft}>Discard changes</button>
    </div>
    {#if error}<p class="theme-error">{error}</p>{/if}
  </div>
{/if}

<style>
  .themes { display: grid; grid-template-columns: repeat(auto-fill, minmax(140px, 1fr)); gap: 8px; }
  .theme-card { display: flex; flex-direction: column; gap: 6px; padding: 6px; text-align: left; cursor: pointer;
    border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--control); color: var(--text); }
  .theme-card[aria-checked='true'] { border-color: var(--accent); box-shadow: inset 0 0 0 1px var(--accent); }
  .theme-card:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .theme-sample { position: relative; display: flex; align-items: center; justify-content: space-between; height: 44px;
    padding: 0 10px; background: var(--c-bg); border-left: 14px solid var(--c-panel); border-radius: var(--radius-sm); }
  .theme-aa { font-size: 20px; font-weight: 700; color: var(--c-text); }
  .theme-mono { font-size: 11px; color: var(--c-muted); }
  .theme-play { position: absolute; left: 58%; top: 0; bottom: 0; width: 2px; background: var(--c-accent); }
  .theme-swatches { display: flex; height: 8px; }
  .theme-swatches i { flex: 1; }
  .theme-name { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px; font-weight: 600; }
  .theme-edited { font-size: 11px; color: var(--accent-text); }
  .theme-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .theme-editor { display: flex; flex-direction: column; gap: 14px; padding: 12px; border: 1px solid var(--line); border-radius: var(--radius-lg); background: var(--track); }
  .theme-editor p { margin: 0; }
  .theme-group { border: 0; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .theme-fonts { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 8px 12px; }
  .theme-colors { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 4px 12px; }
  .theme-field { display: flex; flex-direction: column; gap: 3px; font-size: 12px; color: var(--text-2); }
  .theme-field.color { flex-direction: row; align-items: center; gap: 8px; }
  .theme-field select, .theme-field input[type='text'] { background: var(--control); color: var(--text); border: 1px solid var(--line-strong); border-radius: var(--radius-sm); padding: 4px 6px; }
  .theme-field input[type='color'] { width: 28px; height: 24px; padding: 0; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--control); cursor: pointer; flex: 0 0 auto; }
  .theme-label { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .hex { width: 88px; font-size: 11px; }
  .theme-save { display: flex; flex-wrap: wrap; align-items: end; gap: 8px; border-top: 1px solid var(--line); padding-top: 12px; }
  .theme-field.name { flex: 1 1 200px; }
  .theme-error { color: var(--err-text); }
</style>
