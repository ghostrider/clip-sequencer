<script lang="ts">
  // LUT tab (SPEC §14): the LUT library, a before/after preview and the controls. Every change goes to the app at
  // once; it rewrites the LUT's .cube, and Resolume's LUT effects that use that file reload it by themselves.
  import { app } from '../lib/app.svelte';
  import type { Curves, Grade, LutView, Wheel } from '../lib/types';
  import {
    SAMPLE_IMAGES, KODAK_IMAGES, LutPreview, defaultGrade, fetchTable, fileName, freeLutName, lutOfEffect, placeLabel,
  } from '../lib/lut';
  import CurveEditor from './CurveEditor.svelte';

  let luts = $derived(app.luts?.luts ?? []);
  let sel = $derived<LutView | null>(luts.find((l) => l.name === app.selectedLut) ?? luts[0] ?? null);
  let effects = $derived(app.structure?.lut_effects ?? []);
  let resolumeUp = $derived(!!app.runtime?.ws);
  const usedBy = (name: string) => effects.filter((e) => lutOfEffect(e, app.luts) === name).length;

  // ---- the controls: a local copy, sent while dragging; the app's echo doesn't move a control being dragged ----
  let draft = $state<{ grade: Grade; curves: Curves } | null>(null);
  let draftName = '';
  let lastLocal = 0;
  const QUIET_MS = 800;
  $effect(() => {
    const s = sel;
    if (!s) {
      draft = null;
      draftName = '';
      return;
    }
    if (s.name !== draftName || performance.now() - lastLocal > QUIET_MS) {
      draft = { grade: $state.snapshot(s.grade) as Grade, curves: $state.snapshot(s.curves) as Curves };
      draftName = s.name;
    }
  });

  let sendTimer: ReturnType<typeof setTimeout> | null = null;
  const SEND_MS = 40;
  function changed() {
    lastLocal = performance.now();
    if (sendTimer) return;
    sendTimer = setTimeout(() => {
      sendTimer = null;
      if (draft && draftName) {
        app.send({ cmd: 'lut_update', name: draftName, grade: $state.snapshot(draft.grade) as Grade, curves: $state.snapshot(draft.curves) as Curves });
      }
    }, SEND_MS);
  }
  function setGrade(f: (g: Grade) => void) {
    if (!draft) return;
    f(draft.grade);
    changed();
  }
  function setCurves(c: Curves) {
    if (!draft) return;
    draft.curves = c;
    changed();
  }

  // ---- preview ----
  let canvas: HTMLCanvasElement | undefined = $state();
  /** Bound to `canvas`; made again when the canvas is (it only exists while a LUT is selected). */
  let preview = $state.raw<LutPreview | null>(null);
  let previewCanvas: HTMLCanvasElement | null = null;
  let glError = $state<string | null>(null);
  let image = $state('sample:ramps');
  let liveClips = $derived(
    (app.decks.find((d) => d.live)?.layers ?? []).filter((l) => !l.is_seq_layer).flatMap((l) => l.clips.filter((c) => c.thumb)),
  );

  $effect(() => {
    const c = canvas ?? null;
    if (c === previewCanvas) return;
    previewCanvas = c;
    tableFor = '';
    tableRev = -1;
    if (!c) {
      preview = null;
      return;
    }
    try {
      preview = new LutPreview(c);
      glError = null;
    } catch (e) {
      preview = null;
      glError = String(e);
    }
  });

  // Image source.
  $effect(() => {
    const src = image;
    const p = preview;
    if (!p || !canvas) return;
    const s = src.startsWith('sample:')
      ? ([...SAMPLE_IMAGES, ...KODAK_IMAGES].find((x) => `sample:${x.id}` === src) ?? SAMPLE_IMAGES[0])
      : null;
    if (s && 'draw' in s) {
      const off = document.createElement('canvas');
      off.width = 960;
      off.height = 540;
      s.draw(off.getContext('2d')!, off.width, off.height);
      p.setImage(off);
      p.draw();
    } else {
      const clip = s ? null : (app.clip(Number(src.slice(5))) ?? liveClips.find((c) => `clip:${c.id}` === src) ?? null);
      const url = s ? s.url : clip ? app.thumbUrl(clip) : null;
      if (!url) return;
      const img = new Image();
      img.onload = () => {
        if (image !== src || preview !== p) return;
        p.setImage(img);
        p.draw();
      };
      img.src = url;
    }
  });

  // The baked table: fetched when the selected LUT changes (one request at a time, the newest wins).
  let tableFor = '';
  let tableRev = -1;
  let fetching = false;
  async function loadTable() {
    const s = sel;
    if (!s || !preview) return;
    if (fetching) return;
    if (s.name === tableFor && s.rev === tableRev) return;
    fetching = true;
    try {
      const t = await fetchTable(s.name);
      if (t && preview) {
        preview.setTable(t.size, t.data);
        tableFor = s.name;
        tableRev = t.rev;
        preview.draw();
      }
    } catch {
      /* next change retries */
    } finally {
      fetching = false;
    }
    const now = sel;
    if (now && (now.name !== tableFor || now.rev !== tableRev)) loadTable();
  }
  $effect(() => {
    sel?.rev;
    sel?.name;
    if (preview) loadTable();
  });

  // ---- library actions ----
  let renaming = $state(false);
  let renameText = $state('');
  let confirmDelete = $state(false);
  let importError = $state<string | null>(null);
  let fileInput: HTMLInputElement | undefined = $state();

  function select(name: string) {
    app.selectedLut = name;
    renaming = false;
    confirmDelete = false;
  }
  function create() {
    const name = freeLutName(app.luts, 'New LUT');
    app.send({ cmd: 'lut_create', name });
    select(name);
  }
  function duplicate() {
    if (!sel) return;
    const name = freeLutName(app.luts, sel.name);
    app.send({ cmd: 'lut_create', name, from: sel.name });
    select(name);
  }
  function startRename() {
    if (!sel) return;
    renameText = sel.name;
    renaming = true;
  }
  function finishRename(commit: boolean) {
    if (!renaming) return;
    renaming = false;
    const to = renameText.trim();
    if (!commit || !sel || !to || to === sel.name) return;
    app.send({ cmd: 'lut_rename', from: sel.name, to });
    app.selectedLut = to;
  }
  function remove() {
    if (!sel) return;
    app.send({ cmd: 'lut_delete', name: sel.name });
    confirmDelete = false;
    app.selectedLut = luts.find((l) => l.name !== sel?.name)?.name ?? null;
  }
  async function importFiles(files: FileList | null) {
    importError = null;
    let last: string | null = null;
    const taken = { luts: [...luts] } as { luts: LutView[] };
    for (const f of files ?? []) {
      const name = freeLutName({ dir: '', luts: taken.luts }, f.name.replace(/\.cube$/i, ''));
      try {
        const r = await fetch(`/api/luts/import?name=${encodeURIComponent(name)}`, { method: 'POST', body: await f.text() });
        const res = await r.json();
        if (!res.ok) importError = `${f.name}: ${res.error}`;
        else {
          last = name;
          taken.luts.push({ name } as LutView);
        }
      } catch (e) {
        importError = `${f.name}: ${e}`;
      }
    }
    if (last) select(last);
    if (fileInput) fileInput.value = '';
  }

  // ---- control definitions ----
  type Basic = { key: 'exposure' | 'contrast' | 'saturation' | 'temperature' | 'tint'; label: string; min: number; max: number; def: number; hint: string };
  const BASIC: Basic[] = [
    { key: 'exposure', label: 'Exposure', min: -4, max: 4, def: 0, hint: 'stops' },
    { key: 'contrast', label: 'Contrast', min: 0, max: 3, def: 1, hint: 'around mid grey' },
    { key: 'saturation', label: 'Saturation', min: 0, max: 3, def: 1, hint: '0 = grey' },
    { key: 'temperature', label: 'Temperature', min: -1, max: 1, def: 0, hint: 'cool … warm' },
    { key: 'tint', label: 'Tint', min: -1, max: 1, def: 0, hint: 'green … magenta' },
  ];
  type WheelKey = 'lift' | 'gamma' | 'gain';
  const WHEELS: { key: WheelKey; label: string; min: number; max: number; def: number; hint: string }[] = [
    { key: 'lift', label: 'Lift', min: -0.5, max: 0.5, def: 0, hint: 'shadows' },
    { key: 'gamma', label: 'Gamma', min: 0.2, max: 3, def: 1, hint: 'midtones' },
    { key: 'gain', label: 'Gain', min: 0, max: 2.5, def: 1, hint: 'highlights' },
  ];
  const PARTS: { key: keyof Wheel; label: string }[] = [
    { key: 'master', label: 'All' },
    { key: 'r', label: 'R' },
    { key: 'g', label: 'G' },
    { key: 'b', label: 'B' },
  ];
</script>

{#snippet slider(label: string, value: number, min: number, max: number, def: number, set: (v: number) => void, title = '')}
  <label class="ctl" {title}>
    <span class="ctl-label">{label}</span>
    <input type="range" {min} {max} step={(max - min) / 400} {value}
      oninput={(e) => set(Number((e.currentTarget as HTMLInputElement).value))}
      ondblclick={() => set(def)} />
    <output class="mono" class:changed={Math.abs(value - def) > 1e-6}>{value.toFixed(2)}</output>
  </label>
{/snippet}

<div class="body luts-body">
  <aside class="seqlist lut-list" aria-label="LUT library">
    <div class="row-between">
      <span class="caps">LUTs</span>
      <div class="lib-actions">
        <button class="btn ghost" onclick={create}>+ New</button>
        <button class="btn ghost" onclick={() => fileInput?.click()} title="Import .cube files">Import…</button>
      </div>
    </div>
    <input bind:this={fileInput} type="file" accept=".cube" multiple hidden onchange={(e) => importFiles((e.currentTarget as HTMLInputElement).files)} />
    {#if importError}<p class="meta bad">{importError}</p>{/if}
    {#each luts as l (l.name)}
      {@const n = usedBy(l.name)}
      <button class="seqcard lut-card" class:selected={sel?.name === l.name} onclick={() => select(l.name)} ondblclick={startRename}>
        <span class="seqinfo">
          <span class="seqname">{l.name}</span>
          <span class="meta">
            {#if n > 0}<span class="bind">used by {n} LUT effect{n === 1 ? '' : 's'}</span>{:else}not used in Resolume{/if}{#if l.has_base} · imported{/if}
          </span>
        </span>
      </button>
    {:else}
      <p class="meta">No LUTs yet. <b>+ New</b> starts from a neutral LUT, <b>Import…</b> loads .cube files.</p>
    {/each}
  </aside>

  <section class="editor lut-editor" aria-label="LUT">
    {#if sel && draft}
      <div class="seqhead">
        {#if renaming}
          <!-- svelte-ignore a11y_autofocus -->
          <input class="rename title" bind:value={renameText} autofocus aria-label="LUT name"
            onkeydown={(e) => { if (e.key === 'Enter') finishRename(true); else if (e.key === 'Escape') finishRename(false); }}
            onblur={() => finishRename(true)} />
        {:else}
          <h1 title="Double-click to rename" ondblclick={startRename}>{sel.name}</h1>
        {/if}
        {#if sel.has_base}
          <span class="tag info" title="The imported table is the base; the controls apply on top">imported base</span>
          <button class="x" onclick={() => app.send({ cmd: 'lut_clear_base', name: sel!.name })} title="Drop the imported table">remove base</button>
        {/if}
        <div class="spacer"></div>
        <button class="btn" onclick={startRename}>Rename</button>
        <button class="btn" onclick={duplicate}>Duplicate</button>
        <a class="btn link-btn" href="/api/luts/cube?name={encodeURIComponent(sel.name)}" download="{sel.name}.cube">Export .cube</a>
        {#if confirmDelete}
          <span class="meta">{usedBy(sel.name) ? `Used by ${usedBy(sel.name)} LUT effect(s) in Resolume. ` : ''}Delete?</span>
          <button class="btn danger" onclick={remove}>Delete</button>
          <button class="btn" onclick={() => (confirmDelete = false)}>Keep</button>
        {:else}
          <button class="btn danger" onclick={() => (confirmDelete = true)}>Delete…</button>
        {/if}
      </div>

      <div class="preview-tools">
        <label class="caps" for="lut-image">Image</label>
        <select id="lut-image" class="pill-select image-select" bind:value={image}>
          <optgroup label="Test images">
            {#each SAMPLE_IMAGES as s (s.id)}<option value="sample:{s.id}">{s.label}</option>{/each}
          </optgroup>
          <optgroup label="Kodak test photos">
            {#each KODAK_IMAGES as s (s.id)}<option value="sample:{s.id}">{s.label}</option>{/each}
          </optgroup>
          {#if liveClips.length}
            <optgroup label="Clips of the live deck">
              {#each liveClips as c (c.id)}<option value="clip:{c.id}">{c.name || `Clip ${c.column}`}</option>{/each}
            </optgroup>
          {/if}
        </select>
      </div>
      <div class="preview-box">
        <div class="preview-captions"><span class="caps">Original</span><span class="caps">With {sel.name}</span></div>
        <canvas bind:this={canvas} width="1932" height="540" class="preview" aria-label="Preview: original and with the LUT"></canvas>
        {#if glError}<p class="meta bad">Preview not available: {glError}</p>{/if}
      </div>

      <div class="in-resolume">
        <div class="row-between">
          <span class="caps">LUT effects in Resolume</span>
          <span class="meta mono ellipsis" title={sel.path}>{fileName(sel.path)}</span>
        </div>
        {#if !resolumeUp}
          <p class="meta">Not connected to Resolume.</p>
        {:else if !effects.length}
          <p class="meta">
            No LUT effect in the composition. In Resolume, add the <b>LUT</b> effect (Effects → Color) to the composition,
            a layer or a clip; it shows up here. Or choose this LUT's file in the effect's <b>LUT 3D</b> field yourself.
          </p>
        {:else}
          <div class="fx-list">
            {#each effects as e (e.effect_id)}
              {@const uses = lutOfEffect(e, app.luts)}
              <div class="fx-row">
                <span class="fx-place">{placeLabel(e)}</span>
                <span class="fx-file mono ellipsis" class:ours={!!uses} title={e.path || 'no file'}>
                  {uses ?? (e.path ? fileName(e.path) : 'no file')}
                </span>
                {#if uses === sel.name}
                  <span class="tag info">this LUT</span>
                {:else}
                  <button class="btn ghost" onclick={() => app.send({ cmd: 'lut_apply', name: sel!.name, file_param: e.file_param })}>
                    Use {sel.name}
                  </button>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <p class="hint">Create a LUT with <b>+ New</b>, or import a .cube file.</p>
    {/if}
  </section>

  <aside class="side lut-controls" aria-label="LUT controls">
    {#if draft}
      <div class="group">
        <div class="row-between">
          <span class="caps">Basic</span>
          <button class="x" title="Reset all controls (curves stay)" onclick={() => setGrade((g) => Object.assign(g, defaultGrade()))}>Reset</button>
        </div>
        {#each BASIC as b (b.key)}
          {@render slider(b.label, draft.grade[b.key], b.min, b.max, b.def, (v) => setGrade((g) => (g[b.key] = v)), `${b.hint} · double-click resets`)}
        {/each}
      </div>
      {#each WHEELS as w (w.key)}
        <div class="group">
          <span class="caps">{w.label} <span class="meta">· {w.hint}</span></span>
          {#each PARTS as p (p.key)}
            {@render slider(p.label, draft.grade[w.key][p.key], w.min, w.max, w.def, (v) => setGrade((g) => (g[w.key][p.key] = v)), 'double-click resets')}
          {/each}
        </div>
      {/each}
      <div class="group">
        <span class="caps">Curves</span>
        <CurveEditor curves={draft.curves} onchange={setCurves} />
      </div>
    {/if}
  </aside>
</div>

<style>
  .luts-body { min-height: 0; }
  .lut-list { overflow-y: auto; }
  .lib-actions { display: flex; gap: 4px; }
  .lut-card { width: 100%; text-align: left; color: inherit; font: inherit; }
  .lut-editor { gap: 12px; }
  .seqhead h1 { cursor: text; }
  .link-btn { display: inline-flex; align-items: center; text-decoration: none; }
  .preview-tools { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .image-select { max-width: 260px; }
  .preview-box { position: relative; width: 100%; max-width: 1600px; display: flex; flex-direction: column; gap: 4px; }
  .preview-captions { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .preview { display: block; width: 100%; height: auto; aspect-ratio: 1932 / 540; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--track); }
  .in-resolume { display: flex; flex-direction: column; gap: 8px; max-width: 960px; border-top: 1px solid var(--line); padding-top: 12px; }
  .fx-list { display: flex; flex-direction: column; gap: 4px; }
  .fx-row { display: grid; grid-template-columns: minmax(140px, 1.2fr) minmax(0, 1fr) auto; align-items: center; gap: 10px; padding: 6px 8px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface); }
  .fx-place { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fx-file { font-size: 11px; color: var(--muted); }
  .fx-file.ours { color: var(--info-text); }
  .lut-controls { overflow-y: auto; max-height: calc(100vh - 120px); }
  .group { display: flex; flex-direction: column; gap: 4px; padding-bottom: 10px; border-bottom: 1px solid var(--line); }
  .ctl { display: grid; grid-template-columns: 84px minmax(0, 1fr) 44px; align-items: center; gap: 8px; font-size: 12px; }
  .ctl-label { color: var(--text-2); }
  .ctl input[type=range] { width: 100%; accent-color: var(--accent); }
  .ctl output { font-size: 11px; color: var(--muted); text-align: right; }
  .ctl output.changed { color: var(--accent-text); }
  .bad { color: var(--err-text); }
</style>
