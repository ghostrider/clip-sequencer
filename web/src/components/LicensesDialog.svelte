<script lang="ts">
  // Help → Licenses (M11 Phase F): the project's MIT license, then the third-party notices grouped by part
  // (name, version, license; the full text on expanding). Data: GET /api/licenses (embedded at build time).
  import { app } from '../lib/app.svelte';
  import type { LicensesInfo, NoticePackage } from '../lib/types';
  import Dialog from './Dialog.svelte';

  let info = $state<LicensesInfo | null>(null);
  let error = $state<string | null>(null);
  let filter = $state('');
  /** Expanded packages (their texts are rendered only then: all texts together are ~300 KB). */
  let open = $state<Record<string, boolean>>({});

  $effect(() => {
    fetch('/api/licenses')
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then((v: LicensesInfo) => (info = v))
      .catch((e) => (error = `Could not load the licenses: ${e.message ?? e}`));
  });

  const key = (p: NoticePackage) => `${p.part}|${p.name}|${p.version}`;

  let groups = $derived.by(() => {
    if (!info) return [];
    const q = filter.trim().toLowerCase();
    const list = info.notices.packages.filter(
      (p) => !q || p.name.toLowerCase().includes(q) || p.license.toLowerCase().includes(q),
    );
    const parts: string[] = [];
    for (const p of list) if (!parts.includes(p.part)) parts.push(p.part);
    return parts.map((part) => ({ part, packages: list.filter((p) => p.part === part) }));
  });
</script>

<Dialog title="Licenses" wide onclose={() => (app.dialog = null)}>
  {#if error}
    <p class="lic-error">{error}</p>
  {:else if !info}
    <p class="meta">Loading…</p>
  {:else}
    <section>
      <h3 class="caps">Clip Sequencer for Resolume</h3>
      <pre class="lic-text">{info.license}</pre>
    </section>
    <section>
      <h3 class="caps">Third-party components</h3>
      {#if info.notices.note}<p class="meta">{info.notices.note}</p>{/if}
      {#if info.notices.packages.length}
        <input class="lic-filter" type="search" placeholder="Filter by name or license" aria-label="Filter components"
          bind:value={filter} />
        {#each groups as g (g.part)}
          <h4>{g.part} <span class="meta">({g.packages.length})</span></h4>
          <ul class="lic-list">
            {#each g.packages as p (key(p))}
              <li>
                <details ontoggle={(e) => (open[key(p)] = (e.currentTarget as HTMLDetailsElement).open)}>
                  <summary>
                    <span class="lic-name">{p.name}</span>
                    <span class="mono meta">{p.version}</span>
                    <span class="mono lic-id">{p.license}</span>
                  </summary>
                  {#if open[key(p)]}
                    <p class="meta"><a href={p.url} target="_blank" rel="noopener">{p.url}</a></p>
                    {#each p.texts as t (t)}
                      <pre class="lic-text">{info.notices.texts[t]}</pre>
                    {/each}
                  {/if}
                </details>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="meta">No component matches “{filter}”.</p>
        {/each}
      {/if}
    </section>
  {/if}
</Dialog>

<style>
  section { display: flex; flex-direction: column; gap: 8px; }
  h3, h4 { margin: 0; }
  h4 { font-size: 13px; margin-top: 6px; }
  .lic-text { margin: 0; padding: 10px; max-height: 260px; overflow: auto; white-space: pre-wrap; font: 11px/1.45 var(--mono);
    color: var(--text-2); background: var(--track); border: 1px solid var(--line); border-radius: var(--radius); }
  .lic-filter { padding: 6px 10px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--control);
    color: var(--text); font: inherit; }
  .lic-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .lic-list li { border-bottom: 1px solid var(--line-soft); }
  summary { display: flex; gap: 10px; align-items: baseline; padding: 5px 2px; cursor: pointer; }
  summary:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .lic-name { flex: 1; }
  .lic-id { font-size: 11px; color: var(--info-text); }
  details[open] { padding-bottom: 8px; }
  details > :not(summary) { margin-top: 6px; }
  a { color: var(--accent-text); word-break: break-all; }
  .lic-error { color: var(--err-text); margin: 0; }
</style>
