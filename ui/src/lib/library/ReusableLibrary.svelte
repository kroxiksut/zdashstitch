<script lang="ts">
  import { onMount } from 'svelte';
  import type { I18n } from '../i18n';
  import { Dialog } from '../components';
  import { createCatalogClient, reusableId, type ReusableEntry, type ReusableImport, type ReusableImportMode, type ReusableOrigin, type ReusableStyle, type SensitiveScan } from './client';
  import { displayReusableName, reusableError } from './reusable';

  let { i18n }: { i18n: I18n } = $props();
  const client = createCatalogClient();
  let entries = $state<ReusableEntry[]>([]);
  let loading = $state(true);
  let storageFailed = $state(false);
  let busy = $state(false);
  let message = $state('');
  let saved = $state(false);
  let selectedFile = $state<File | null>(null);
  let fileInput: HTMLInputElement;
  let pendingImport = $state<{ id: string; preview: ReusableImport } | null>(null);
  let search = $state('');
  let origin = $state<ReusableOrigin | 'all'>('all');
  const order = $derived(entries.filter((entry) =>
    (origin === 'all' || entry.origin === origin)
      && [displayReusableName(i18n, entry), ...(entry.asset.document.tags ?? [])]
        .some((value) => value.toLocaleLowerCase().includes(search.toLocaleLowerCase()))
  ).sort((a, b) => displayReusableName(i18n, a).localeCompare(displayReusableName(i18n, b))));

  let editing = $state<ReusableEntry | null>(null);
  let editMode = $state<'metadata' | 'style' | 'create' | null>(null);
  let name = $state('');
  let description = $state('');
  let tags = $state('');
  let adapter = $state('8.0');
  let palette = $state('');
  const colors = $derived(palette.split(/\r?\n/).map((line) => line.trim()).filter(Boolean));
  const validColors = $derived(colors.length <= 64 && colors.every((color) => /^[0-9A-F]{6}$/.test(color)));
  let deleteEntry = $state<ReusableEntry | null>(null);
  let exporting = $state<ReusableEntry | null>(null);
  let scan = $state<Omit<SensitiveScan, 'revision'> | null>(null);
  let acknowledged = $state(false);

  async function reload(): Promise<void> {
    loading = true;
    storageFailed = false;
    try { entries = await client.reusable(); }
    catch { storageFailed = true; }
    finally { loading = false; }
  }
  onMount(() => { void reload(); });

  async function perform(action: () => Promise<void>, importer = false): Promise<void> {
    if (busy) return;
    busy = true; saved = false; message = '';
    try { await action(); }
    catch (error) { message = reusableError(i18n, error, importer); }
    finally { busy = false; }
  }

  function inspect(): void {
    const file = selectedFile;
    if (!file) return;
    pendingImport = null;
    void perform(async () => { pendingImport = await client.inspectReusable(file); }, true);
  }

  function chooseNative(): void {
    pendingImport = null;
    void perform(async () => {
      const result = await client.chooseReusable();
      if (result.id !== null) pendingImport = result;
    }, true);
  }

  function saveImport(mode: ReusableImportMode): void {
    const current = pendingImport;
    if (!current) return;
    void perform(async () => {
      pendingImport = null;
      await client.runReusableImport(current.id, mode);
      if (fileInput) fileInput.value = '';
      selectedFile = null;
      await reload();
      saved = true;
    }, true);
  }

  function edit(entry: ReusableEntry | null, mode: 'metadata' | 'style' | 'create'): void {
    editing = entry; editMode = mode;
    name = entry?.asset.document.name ?? '';
    description = entry?.asset.document.description ?? '';
    tags = entry?.asset.document.tags?.join('\n') ?? '';
    adapter = entry?.asset.document.adapter ?? '8.0';
    palette = entry?.asset.kind === 'style_profile' ? entry.asset.document.widget_types.svggraph?.series_palette?.join('\n') ?? '' : '';
  }

  function saveEdit(): void {
    if (!editMode || !name.trim() || !validColors) return;
    const current = editing;
    const metadata = {
      name: name.trim(),
      ...(description.trim() ? { description: description.trim() } : {}),
      tags: tags.split(/\r?\n/).map((value) => value.trim()).filter(Boolean),
    };
    void perform(async () => {
      if (editMode === 'metadata' && current) {
        await client.renameReusable(reusableId(current.asset), current.revision, metadata);
      } else if (editMode === 'style' && current?.asset.kind === 'style_profile') {
        const original = $state.snapshot(current.asset.document);
        const svggraph = { ...original.widget_types.svggraph, series_palette: colors.slice() };
        delete svggraph.color_palette;
        await client.updateReusableStyle({ ...original, widget_types: { ...original.widget_types, svggraph } }, current.revision);
      } else {
        const document: ReusableStyle = { format: 'zdashstitch.style-profile/v1', profile_id: crypto.randomUUID(), ...metadata, version: 1, adapter, widget_types: { svggraph: { series_palette: colors.slice() } } };
        await client.createReusable({ kind: 'style_profile', document });
      }
      editMode = null; editing = null;
      await reload(); saved = true;
    });
  }

  function duplicate(entry: ReusableEntry): void {
    void perform(async () => { await client.duplicateReusable(reusableId(entry.asset), entry.revision); await reload(); saved = true; });
  }

  function confirmDelete(): void {
    const entry = deleteEntry;
    if (!entry) return;
    void perform(async () => { await client.deleteReusable(reusableId(entry.asset), entry.revision); deleteEntry = null; await reload(); });
  }

  function reviewExport(entry: ReusableEntry): void {
    exporting = null; scan = null; acknowledged = false;
    void perform(async () => {
      const result = await client.scanReusable(reusableId(entry.asset));
      if (result.revision !== entry.revision) { message = i18n.t('reusable.conflict'); return; }
      scan = result.scan;
      exporting = entry;
    });
  }

  function download(): void {
    const entry = exporting;
    if (!entry || !acknowledged || scan?.credential_candidates) return;
    void perform(async () => {
      const result = await client.exportReusable(reusableId(entry.asset), entry.revision);
      const link = document.createElement('a');
      link.href = `/api/downloads/${encodeURIComponent(result.id)}`;
      link.download = result.display_name;
      document.body.append(link); link.click(); link.remove();
      exporting = null; scan = null;
    });
  }
  function saveNative(): void {
    const entry = exporting;
    if (!entry || !acknowledged || scan?.credential_candidates) return;
    void perform(async () => {
      const result = await client.saveReusable(reusableId(entry.asset), entry.revision);
      if (!result.cancelled) { exporting = null; scan = null; }
    });
  }
</script>

<section class="card" aria-labelledby="reusable-library-title">
  <h2 id="reusable-library-title">{i18n.t('reusable.title')}</h2>
  <div class="reusable-actions">
    <button type="button" disabled={busy} onclick={chooseNative}>{i18n.t('reusable.open_file')}</button>
    <label>{i18n.t('reusable.search')} <input type="search" bind:value={search} /></label>
    <label>{i18n.t('reusable.origin')}
      <select bind:value={origin}>
        {#each ['all', 'builtin', 'file', 'local'] as source}
          <option value={source}>{i18n.t(source === 'all' ? 'reusable.all' : source === 'file' ? 'reusable.file' : source === 'local' ? 'reusable.local' : 'reusable.builtin')}</option>
        {/each}
      </select>
    </label>
    <button type="button" disabled={busy || editMode !== null} onclick={() => edit(null, 'create')}>{i18n.t('reusable.create_style')}</button>
  </div>
  <div class="reusable-actions">
    <label>{i18n.t('reusable.choose_file')}
      <input type="file" accept=".yaml,.yml" bind:this={fileInput} disabled={busy} onchange={(event) => { selectedFile = event.currentTarget.files?.[0] ?? null; pendingImport = null; }} />
    </label>
    <button type="button" disabled={busy || !selectedFile} onclick={inspect}>{i18n.t('reusable.inspect_file')}</button>
  </div>
  {#if pendingImport}
    {@const imported = pendingImport.preview}
    <div class="card">
      <strong>{imported.name}</strong>
      <p>{i18n.t(imported.kind === 'style_profile' ? 'reusable.profiles' : 'reusable.blueprints')} · {i18n.t('reusable.details', { version: imported.version, adapter: imported.adapter })}</p>
      {#if !['7.4', '8.0'].includes(imported.adapter)}<p>{i18n.t('reusable.unverified')}</p>{/if}
      <div class="reusable-actions">
        {#if imported.existing}
          <p>{i18n.t('reusable.collision')}</p>
          {#if imported.existing[1] !== 'builtin'}
            <button type="button" disabled={busy} onclick={() => saveImport({ mode: 'replace', expected_revision: imported.existing?.[0] ?? 0 })}>{i18n.t('reusable.replace')}</button>
          {/if}
          <button type="button" disabled={busy} onclick={() => saveImport({ mode: 'copy' })}>{i18n.t('reusable.save_copy')}</button>
        {:else}
          <button type="button" disabled={busy} onclick={() => saveImport({ mode: 'new' })}>{i18n.t('reusable.import')}</button>
        {/if}
        <button type="button" disabled={busy} onclick={() => { pendingImport = null; }}>{i18n.t('reusable.cancel_import')}</button>
      </div>
    </div>
  {/if}
  {#if storageFailed}<p role="alert">{i18n.t('reusable.storage_failed')}</p>{/if}
  {#if message}<p role="alert">{message}</p>{/if}
  {#if saved}<p role="status">{i18n.t('reusable.loaded')}</p>{/if}
  {#if !loading && !storageFailed}
    {#if order.length === 0}
      <p>{i18n.t('reusable.empty')}</p>
    {:else}
      {#each ['style_profile', 'widget_blueprint'] as kind}
        {@const items = order.filter((entry) => entry.asset.kind === kind)}
        {#if items.length > 0}
          <h3>{i18n.t(kind === 'style_profile' ? 'reusable.profiles' : 'reusable.blueprints')}</h3>
          <ul>
            {#each items as entry (reusableId(entry.asset))}
              <li>
                <strong>{displayReusableName(i18n, entry)}</strong>
                <p>{i18n.t('reusable.details', { version: entry.asset.document.version, adapter: entry.asset.document.adapter })} · {i18n.t(entry.origin === 'builtin' ? 'reusable.builtin' : entry.origin === 'file' ? 'reusable.file' : 'reusable.local')}</p>
                {#if !['7.4', '8.0'].includes(entry.asset.document.adapter)}<p>{i18n.t('reusable.unverified')}</p>{/if}
                {#if entry.origin !== 'builtin' && entry.asset.document.description}<p>{entry.asset.document.description}</p>{/if}
                <div class="reusable-actions">
                  {#if entry.origin !== 'builtin'}
                    <button type="button" disabled={busy} onclick={() => edit(entry, 'metadata')}>{i18n.t('reusable.rename')}</button>
                    {#if entry.asset.kind === 'style_profile' && entry.asset.document.widget_types.svggraph?.series_palette}
                      <button type="button" disabled={busy} onclick={() => edit(entry, 'style')}>{i18n.t('reusable.edit')}</button>
                    {/if}
                    <button type="button" disabled={busy} onclick={() => { deleteEntry = entry; }}>{i18n.t('reusable.delete')}</button>
                  {/if}
                  <button type="button" disabled={busy} onclick={() => duplicate(entry)}>{i18n.t('reusable.duplicate')}</button>
                  <button type="button" disabled={busy} onclick={() => reviewExport(entry)}>{i18n.t('reusable.export')}</button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      {/each}
    {/if}
  {/if}
  {#if editMode}
    <form class="card" onsubmit={(event) => { event.preventDefault(); saveEdit(); }}>
      {#if editMode !== 'style'}
        <label>{i18n.t('reusable.name')} <input bind:value={name} required maxlength={editing?.asset.kind === 'widget_blueprint' ? 255 : 120} /></label>
        <label>{i18n.t('reusable.description')} <textarea bind:value={description} maxlength="2000"></textarea></label>
        <label>{i18n.t('reusable.tags')} <textarea bind:value={tags}></textarea></label>
      {/if}
      {#if editMode === 'create'}
        <label>{i18n.t('reusable.adapter')}
          <select bind:value={adapter}><option value="8.0">8.0</option><option value="7.4">7.4</option></select>
        </label>
      {/if}
      {#if editMode !== 'metadata'}
        <label>{i18n.t('reusable.palette')} <textarea bind:value={palette} required aria-describedby="reusable-palette-hint"></textarea></label>
        <p id="reusable-palette-hint">{i18n.t('reusable.palette_hint')}</p>
      {/if}
      <div class="reusable-actions">
        <button type="submit" disabled={busy || !name.trim() || (editMode !== 'metadata' && (!validColors || colors.length === 0))}>{i18n.t('reusable.save')}</button>
        <button type="button" disabled={busy} onclick={() => { editMode = null; }}>{i18n.t('reusable.cancel')}</button>
      </div>
    </form>
  {/if}
</section>

{#if deleteEntry}
  <Dialog {i18n} id="reusable-delete" title={{ id: 'reusable.confirm_delete' }} description={{ id: 'reusable.delete_prompt', params: { name: displayReusableName(i18n, deleteEntry) } }} open={true} cancellable={!busy} onclose={() => { deleteEntry = null; }}>
    {#snippet actions()}
      <button type="button" disabled={busy} onclick={confirmDelete}>{i18n.t('reusable.delete')}</button>
      <button type="button" disabled={busy} onclick={() => { deleteEntry = null; }}>{i18n.t('reusable.cancel')}</button>
    {/snippet}
  </Dialog>
{/if}

{#if exporting && scan}
  <Dialog {i18n} id="reusable-export" title={{ id: 'reusable.export_review' }} description={{ id: 'reusable.export_info' }} open={true} cancellable={!busy} onclose={() => { exporting = null; }}>
    <p>{displayReusableName(i18n, exporting)}</p>
    <p>{i18n.t('reusable.export_findings', { count: scan.findings.length })}</p>
    {#if message}<p role="alert">{message}</p>{/if}
    {#if scan.credential_candidates > 0}
      <p role="alert">{i18n.t('reusable.export_blocked')}</p>
    {:else}
      <label><input type="checkbox" bind:checked={acknowledged} /> {i18n.t('reusable.export_ack')}</label>
    {/if}
    {#snippet actions()}
      <button type="button" disabled={busy || !acknowledged || !!scan?.credential_candidates} onclick={saveNative}>{i18n.t('reusable.save_file')}</button>
      <button type="button" disabled={busy || !acknowledged || !!scan?.credential_candidates} onclick={download}>{i18n.t('reusable.export')}</button>
      <button type="button" disabled={busy} onclick={() => { exporting = null; }}>{i18n.t('reusable.cancel')}</button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .reusable-actions { display: flex; flex-wrap: wrap; align-items: flex-end; gap: var(--space-3); }
  label { display: grid; gap: var(--space-1); }
  ul { display: grid; gap: var(--space-3); }
  li { overflow-wrap: anywhere; }
  p { margin-block: var(--space-1); }
</style>
