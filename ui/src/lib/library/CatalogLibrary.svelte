<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiFailure } from '../api';
  import { Button } from '../components';
  import type { I18n } from '../i18n';
  import ReusableLibrary from './ReusableLibrary.svelte';
  import { createCatalogClient, type CatalogDeletionPreview, type CatalogPreview, type CatalogSummary, type LibraryIndex, type LibraryPageSummary, type OfflinePage, type SnapshotSummary, type SnapshotComparison, type SnapshotDeletionPreview } from './client';

  let { i18n, ontaken }: { i18n: I18n; ontaken?: (page: OfflinePage) => void } = $props();
  const client = createCatalogClient();
  let catalogs = $state<CatalogSummary[] | null>(null);
  let pages = $state<LibraryPageSummary[] | null>(null);
  let index = $state<LibraryIndex | null>(null);
  let snapshots = $state<SnapshotSummary[] | null>(null);
  let damagedSnapshots = $state<Array<{ library_id: string; code: string }>>([]);
  let snapshotReferencesIncomplete = $state(false);
  let snapshotListError = $state(false);
  let compareLeft = $state('');
  let compareRight = $state('');
  let comparison = $state<SnapshotComparison | null>(null);
  let compareError = $state(false);
  let snapshotDeletion = $state<SnapshotDeletionPreview | null>(null);
  let snapshotActionError = $state(false);
  let indexError = $state(false);
  let cacheWarning = $state(false);
  let query = $state('');
  let kind = $state<'all' | 'catalog' | 'page' | 'snapshot'>('all');
  let order = $state<'newest' | 'name'>('newest');
  let filteredCatalogs = $derived((catalogs ?? []).filter((item) => (kind === 'all' || kind === 'catalog') && item.name.toLocaleLowerCase().includes(query.toLocaleLowerCase())).sort((a, b) => order === 'name' ? a.name.localeCompare(b.name) : b.version - a.version));
  let filteredPages = $derived((pages ?? []).filter((item) => (kind === 'all' || kind === 'page') && item.name.toLocaleLowerCase().includes(query.toLocaleLowerCase())).sort((a, b) => order === 'name' ? a.name.localeCompare(b.name) : b.saved_at_ms - a.saved_at_ms));
  let filteredSnapshots = $derived((snapshots ?? []).filter((item) => (kind === 'all' || kind === 'snapshot') && item.connection_id.toLocaleLowerCase().includes(query.toLocaleLowerCase())).sort((a, b) => order === 'name' ? a.connection_id.localeCompare(b.connection_id) : b.read_at_ms - a.read_at_ms));
  let catalogListError = $state(false);
  let pageListError = $state(false);
  let files = $state<File[]>([]);
  let preview = $state<CatalogPreview | null>(null);
  let busy = $state(false);
  let importError = $state(false);
  let savedCount = $state<number | null>(null);
  let takeError = $state(false);
  let actionError = $state(false);
  let editing = $state<string | null>(null);
  let editName = $state('');
  let deleting = $state<string | null>(null);
  let deleteConfirmation = $state('');
  let catalogDeletion = $state<CatalogDeletionPreview | null>(null);
  let input: HTMLInputElement;

  async function reload(): Promise<void> {
    compareLeft = '';
    compareRight = '';
    comparison = null;
    compareError = false;
    snapshotDeletion = null;
    catalogListError = false;
    pageListError = false;
    indexError = false;
    snapshotListError = false;
    cacheWarning = false;
    await Promise.all([
      client.list().then((items) => { catalogs = items; }).catch(() => { catalogs = null; catalogListError = true; }),
      client.libraryPages().then((items) => { pages = items; }).catch(() => { pages = null; pageListError = true; }),
      client.snapshots().then((items) => { snapshots = items.snapshots; damagedSnapshots = items.damaged; snapshotReferencesIncomplete = items.references_incomplete; }).catch(() => { snapshots = null; damagedSnapshots = []; snapshotReferencesIncomplete = false; snapshotListError = true; }),
      client.index().then((items) => { index = items; cacheWarning = items.cache_write_failed === true; }).catch(() => { index = null; indexError = true; })
    ]);
  }

  onMount(() => { void reload(); });

  function select(selection: FileList | null): void {
    if (busy || !selection) return;
    files = Array.from(selection);
    preview = null;
    importError = false;
    savedCount = null;
  }

  async function inspect(): Promise<void> {
    if (busy || files.length === 0) return;
    busy = true;
    preview = null;
    savedCount = null;
    importError = false;
    try {
      preview = await client.inspect(files);
    } catch {
      importError = true;
    } finally {
      busy = false;
    }
  }

  async function save(): Promise<void> {
    if (busy || !preview?.id || !preview.files.some((file) => file.status === 'ready')) return;
    busy = true;
    importError = false;
    try {
      const saved = await client.run(preview.id);
      savedCount = saved.length;
      preview = null;
      files = [];
      input.value = '';
      await reload();
    } catch (error) {
      importError = true;
      preview = null;
      // If the local server handled the request, its upload set is already spent.
      if (!(error instanceof ApiFailure)) await reload();
    } finally {
      busy = false;
    }
  }

  async function take(id: string): Promise<void> {
    if (busy) return;
    busy = true;
    takeError = false;
    try {
      const page = await client.takeLibraryPage(id);
      ontaken?.(page);
    } catch {
      takeError = true;
    } finally {
      busy = false;
    }
  }

  async function perform(action: () => Promise<unknown>): Promise<void> {
    if (busy) return;
    busy = true;
    actionError = false;
    try { await action(); editing = null; deleting = null; catalogDeletion = null; deleteConfirmation = ''; await reload(); }
    catch { actionError = true; await reload(); }
    finally { busy = false; }
  }

  async function inspectCatalogDeletion(id: string): Promise<void> {
    if (busy) return;
    busy = true;
    actionError = false;
    catalogDeletion = null;
    deleteConfirmation = '';
    try { catalogDeletion = await client.catalogDeletePreview(id); editing = null; deleting = null; }
    catch { actionError = true; }
    finally { busy = false; }
  }

  async function compareStoredSnapshots(): Promise<void> {
    if (busy || !compareLeft || !compareRight) return;
    busy = true; compareError = false; comparison = null;
    try { comparison = await client.compareSnapshots(compareLeft, compareRight); }
    catch { compareError = true; }
    finally { busy = false; }
  }

  async function inspectSnapshotDeletion(id: string): Promise<void> {
    if (busy) return;
    busy = true; snapshotActionError = false; snapshotDeletion = null;
    try { snapshotDeletion = await client.snapshotDeletePreview(id); }
    catch { snapshotActionError = true; }
    finally { busy = false; }
  }

  async function deleteStoredSnapshot(id: string): Promise<void> {
    if (busy || snapshotDeletion?.library_id !== id || !snapshotDeletion.deletable) return;
    busy = true; snapshotActionError = false;
    try { await client.deleteSnapshot(id); snapshotDeletion = null; await reload(); }
    catch { snapshotActionError = true; snapshotDeletion = null; await reload(); }
    finally { busy = false; }
  }
</script>

<div class="screen library-screen">
  <header class="screen-head">
    <h1>{i18n.t('library.title')}</h1>
  </header>
  <section class="card import-card" aria-labelledby="catalog-import-title" id="template-import">
    <h2 id="catalog-import-title">{i18n.t('startup.action.import_files')}</h2>
    <p id="catalog-import-help">{i18n.t('template_import.help')}</p>
    <p class="muted">{i18n.t('template_import.source_steps')}</p>
    <p class="muted">{i18n.t('template_import.server_future')}</p>
    <div class="actions">
      <label class="button-link browser-select" for="template-import-file"
        ondragover={(event) => event.preventDefault()}
        ondrop={(event) => { event.preventDefault(); select(event.dataTransfer?.files ?? null); }}
      >{i18n.t('template_import.choose')}</label>
      <input class="file-input" id="template-import-file" type="file" multiple accept=".yaml,.yml,.json,.xml"
        aria-describedby="catalog-import-help" disabled={busy} bind:this={input}
        onchange={(event) => select(event.currentTarget.files)} />
      {#if files.length > 0}<span class="muted">{i18n.t('template_import.selected', { count: files.length })}</span>{/if}
      <Button {i18n} label={{ id: 'template_import.inspect' }} disabled={busy || files.length === 0}
        onclick={() => void inspect()} />
    </div>
    <div aria-live="polite">
      {#if busy}<p>{i18n.t('common.state.loading')}</p>{/if}
      {#if importError}<p role="alert">{i18n.t('template_import.action_failed')}</p>{/if}
      {#if savedCount !== null}<p>{i18n.t('template_import.saved', { count: savedCount })}</p>{/if}
      {#if preview}
        <ul class="preview">
          {#each preview.files as file}
            <li>
              <bdi>{file.file}</bdi>
              {#if file.status === 'ready'}
                <p>{i18n.t('template_import.becomes', { count: file.catalogs.length })}</p>
                <ul>
                  {#each file.catalogs as catalog}
                    <li>
                      <bdi>{catalog.name}</bdi> {i18n.t('template_import.preview_counts', {
                        metrics: catalog.metrics, rules: catalog.discovery_rules, hints: catalog.metrics_with_hints
                      })}
                      {#if catalog.version > 1}<p>{i18n.t('template_import.new_version')}</p>{/if}
                      {#if catalog.diagnostics.includes('source_partial') || catalog.omitted_template_dashboards > 0}
                        <p>{i18n.t('template_import.partial')}</p>
                      {/if}
                      {#if catalog.diagnostics.includes('missing_parent_template')}
                        <p>{i18n.t('template_import.missing_parent')}</p>
                      {/if}
                      {#if catalog.diagnostics.includes('unresolved_macros')}
                        <p>{i18n.t('template_import.unresolved_macros')}</p>
                      {/if}
                      {#if catalog.diagnostics.includes('lld_requires_instance')}
                        <p>{i18n.t('template_import.lld_requires_instance')}</p>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {:else}
                <p>{i18n.t('template_import.rejected', { code: file.code })}
                  {#if file.line !== null && file.column !== null}
                    {i18n.t('template_import.position', { line: file.line, column: file.column })}
                  {/if}
                </p>
              {/if}
            </li>
          {/each}
        </ul>
        {#if preview.id && preview.files.some((file) => file.status === 'ready')}
          <Button {i18n} variant="primary" label={{ id: 'template_import.save' }} disabled={busy}
            onclick={() => void save()} />
        {/if}
      {/if}
    </div>
  </section>
  <section class="card catalog-card" aria-label={i18n.t('page_library.filters')}>
    <label>{i18n.t('page_library.search')} <input type="search" bind:value={query} /></label>
    <label>{i18n.t('page_library.kind')} <select bind:value={kind}>
      <option value="all">{i18n.t('page_library.all')}</option>
      <option value="catalog">{i18n.t('template_import.catalogs')}</option>
      <option value="page">{i18n.t('page_library.title')}</option>
      <option value="snapshot">{i18n.t('snapshot_library.title')}</option>
    </select></label>
    <label>{i18n.t('page_library.sort')} <select bind:value={order}>
      <option value="newest">{i18n.t('page_library.newest')}</option>
      <option value="name">{i18n.t('page_library.name')}</option>
    </select></label>
  </section>
  {#if indexError}<p role="status">{i18n.t('page_library.index_unavailable')}</p>{/if}
  {#if cacheWarning}<p role="status">{i18n.t('page_library.cache_write_failed')}</p>{/if}
  {#if actionError}<p role="alert">{i18n.t('page_library.action_failed')}</p>{/if}
  <section class="card catalog-card" aria-labelledby="library-snapshots-title">
    <h2 id="library-snapshots-title">{i18n.t('snapshot_library.title')}</h2>
    <p class="muted">{i18n.t('snapshot_library.local_only')}</p>
    {#if snapshotActionError}<p role="alert">{i18n.t('snapshot_library.delete_failed')}</p>{/if}
    {#if snapshotListError}<p role="alert">{i18n.t('snapshot_library.list_failed')}</p>
    {:else if snapshots === null}<p aria-live="polite">{i18n.t('common.state.loading')}</p>
    {:else if snapshots.length === 0 && damagedSnapshots.length === 0}<p>{i18n.t('snapshot_library.empty')}</p>
    {:else}
      {#if damagedSnapshots.length > 0}<p role="alert">{i18n.t('snapshot_library.damaged', { count: damagedSnapshots.length })}</p>{/if}
      {#if snapshotReferencesIncomplete}<p role="status">{i18n.t('snapshot_library.references_incomplete')}</p>{/if}
      <ul class="catalogs">
        {#each filteredSnapshots as snapshot (snapshot.library_id)}
          <li>
            <strong>{i18n.t('snapshot_library.item')}</strong>
            <p>{i18n.t('snapshot_library.source', { connection: snapshot.connection_id, version: snapshot.server_version })}</p>
            <p>{i18n.t('snapshot_library.read_at', { time: new Date(snapshot.read_at_ms) })}</p>
            {#if snapshotReferencesIncomplete}<p>{i18n.t('snapshot_library.usage_unknown')}</p>
            {:else}<p>{i18n.t('snapshot_library.used_by', { count: snapshot.used_by.length })}</p>{/if}
            {#each snapshot.used_by as copy (copy)}<a href={`/draft/${encodeURIComponent(copy)}/overview`}>{i18n.t('snapshot_library.open_copy')}</a>{/each}
            <Button {i18n} label={{ id: 'snapshot_library.delete_action' }} disabled={busy} onclick={() => void inspectSnapshotDeletion(snapshot.library_id)} />
            {#if snapshotDeletion?.library_id === snapshot.library_id}
              <div>
                <p>{i18n.t('snapshot_library.delete_effect')}</p>
                <p>{i18n.t('snapshot_library.delete_refs', { copies: snapshotDeletion.workcopy_count, journal: snapshotDeletion.journal_count })}</p>
                {#if snapshotDeletion.references_unknown}<p role="alert">{i18n.t('snapshot_library.delete_unknown')}</p>{/if}
                {#if snapshotDeletion.deletable}<Button {i18n} variant="danger" label={{ id: 'snapshot_library.confirm_delete' }} disabled={busy} onclick={() => void deleteStoredSnapshot(snapshot.library_id)} />{/if}
                <Button {i18n} label={{ id: 'page_library.cancel' }} disabled={busy} onclick={() => { snapshotDeletion = null; }} />
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      {#if snapshots.length > 1}
        <form onsubmit={(event) => { event.preventDefault(); void compareStoredSnapshots(); }}>
          <h3>{i18n.t('snapshot_library.compare_title')}</h3>
          <label>{i18n.t('snapshot_library.first')} <select bind:value={compareLeft} onchange={() => { comparison = null; compareError = false; }} required>
            <option value="">{i18n.t('snapshot_library.choose')}</option>
            {#each snapshots as snapshot (snapshot.library_id)}<option value={snapshot.library_id}>{snapshot.connection_id} · {i18n.date(snapshot.read_at_ms)}</option>{/each}
          </select></label>
          <label>{i18n.t('snapshot_library.second')} <select bind:value={compareRight} onchange={() => { comparison = null; compareError = false; }} required>
            <option value="">{i18n.t('snapshot_library.choose')}</option>
            {#each snapshots as snapshot (snapshot.library_id)}<option value={snapshot.library_id}>{snapshot.connection_id} · {i18n.date(snapshot.read_at_ms)}</option>{/each}
          </select></label>
          <Button {i18n} type="submit" label={{ id: 'snapshot_library.compare_action' }} disabled={busy || !compareLeft || !compareRight} />
          {#if compareError}<p role="alert">{i18n.t('snapshot_library.compare_failed')}</p>{/if}
          {#if comparison}<p role="status">{i18n.t(comparison.identical ? 'snapshot_library.identical' : 'snapshot_library.different')}</p>
            <p>{i18n.t('snapshot_library.counts', { left_pages: comparison.left_pages, right_pages: comparison.right_pages, left_widgets: comparison.left_widgets, right_widgets: comparison.right_widgets })}</p>{/if}
        </form>
      {/if}
    {/if}
  </section>
  <section class="card catalog-card" aria-labelledby="catalog-list-title">
    <h2 id="catalog-list-title">{i18n.t('template_import.catalogs')}</h2>
    <p class="muted">{i18n.t('template_import.catalog_use_later')}</p>
    {#if catalogs && catalogs.length > 0}<p><a href="/drafts#offline-builder-title">{i18n.t('offline_builder.go_to_drafts')}</a></p>{/if}
    {#if catalogListError}
      <p role="alert">{i18n.t('template_import.list_failed')}</p>
    {:else if catalogs === null}
      <p aria-live="polite">{i18n.t('common.state.loading')}</p>
    {:else if catalogs.length === 0}
      <p>{i18n.t('template_import.empty')}</p>
    {:else}
      <ul class="catalogs">
        {#each filteredCatalogs as catalog (`${catalog.catalog_id}/${catalog.version}`)}
          {@const indexed = index?.catalogs.find((entry) => entry.catalog_id === catalog.catalog_id && entry.version === catalog.version)}
          <li>
            <bdi>{catalog.name}</bdi>
            <p>{i18n.t('template_import.version', { version: catalog.version, export_version: catalog.export_format_version ?? '—' })}</p>
            <p>{i18n.t('template_import.preview_counts', {
              metrics: catalog.metrics, rules: catalog.discovery_rules, hints: catalog.metrics_with_hints
            })}</p>
            {#if catalog.diagnostics.includes('source_partial') || catalog.omitted_template_dashboards > 0}
              <p>{i18n.t('template_import.partial')}</p>
            {/if}
            {#if catalog.diagnostics.includes('missing_parent_template')}
              <p>{i18n.t('template_import.missing_parent')}</p>
            {/if}
            {#if catalog.diagnostics.includes('unresolved_macros')}
              <p>{i18n.t('template_import.unresolved_macros')}</p>
            {/if}
            {#if catalog.diagnostics.includes('lld_requires_instance')}
              <p>{i18n.t('template_import.lld_requires_instance')}</p>
            {/if}
            {#if indexed}<p>{i18n.t('page_library.used_by', { count: indexed.used_by.length })}</p>
            {:else}<p>{i18n.t('page_library.usage_unknown')}</p>{/if}
            {#if catalog.library_id}
              <div class="actions">
                <Button {i18n} label={{ id: 'page_library.duplicate' }} disabled={busy} onclick={() => void perform(() => client.duplicateCatalog(catalog.library_id!, catalog.revision ?? 1))} />
                <Button {i18n} label={{ id: 'page_library.rename' }} disabled={busy} onclick={() => { editing = catalog.library_id!; editName = catalog.name; deleting = null; }} />
                <Button {i18n} label={{ id: 'page_library.delete' }} disabled={busy} onclick={() => void inspectCatalogDeletion(catalog.library_id!)} />
              </div>
              {#if editing === catalog.library_id}
                <form onsubmit={(event) => { event.preventDefault(); void perform(() => client.renameCatalog(catalog.library_id!, catalog.revision ?? 1, editName)); }}>
                  <label>{i18n.t('page_library.display_name')} <input bind:value={editName} maxlength="160" required /></label>
                  <Button {i18n} type="submit" label={{ id: 'page_library.save_name' }} disabled={busy} />
                  <Button {i18n} label={{ id: 'page_library.cancel' }} disabled={busy} onclick={() => { editing = null; }} />
                </form>
              {/if}
              {#if catalogDeletion?.library_id === catalog.library_id}
                <div class="card">
                  <p>{i18n.t('page_library.catalog_delete_effect')}</p>
                  <p>{i18n.t('template_import.version', { version: catalogDeletion.version, export_version: catalog.export_format_version ?? '—' })}</p>
                  {#if catalogDeletion.used_by.length > 0}
                    <p>{i18n.t('page_library.catalog_in_use', { count: catalogDeletion.used_by.length })}</p>
                    <ul>{#each catalogDeletion.used_by as consumer}<li><bdi>{consumer}</bdi></li>{/each}</ul>
                  {:else}
                    <form onsubmit={(event) => { event.preventDefault(); void perform(() => client.deleteCatalog(catalog.library_id!, catalogDeletion!.revision, deleteConfirmation)); }}>
                      <label>{i18n.t('page_library.confirm_name', { name: catalogDeletion.name })} <input bind:value={deleteConfirmation} required /></label>
                      <Button {i18n} type="submit" variant="danger" label={{ id: 'page_library.confirm_catalog_delete' }} disabled={busy || deleteConfirmation !== catalogDeletion.name} />
                    </form>
                  {/if}
                  <Button {i18n} label={{ id: 'page_library.cancel' }} disabled={busy} onclick={() => { catalogDeletion = null; }} />
                </div>
              {/if}
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
  <section class="card catalog-card" aria-labelledby="library-pages-title">
    <h2 id="library-pages-title">{i18n.t('page_library.title')}</h2>
    {#if takeError}<p role="alert">{i18n.t('page_library.take_failed')}</p>{/if}
    {#if pageListError}<p role="alert">{i18n.t('page_library.list_failed')}</p>
    {:else if pages === null}<p>{i18n.t('common.state.loading')}</p>
    {:else if pages.length === 0}<p>{i18n.t('page_library.empty')}</p>
    {:else}<ul>
      {#each filteredPages as page (page.library_id)}
        {@const indexed = index?.pages.find((entry) => entry.library_id === page.library_id)}
        <li>
          <bdi>{page.name}</bdi> · {i18n.t('offline_dashboard.widget_count', { count: page.widgets })} · {new Date(page.saved_at_ms).toLocaleString(i18n.locale)} · {i18n.t('page_library.catalog_count', { count: page.catalog_versions?.length ?? 0 })} · {i18n.t('page_library.source', { id: page.source_local_id })}
          <div class="actions">
            <Button {i18n} label={{ id: 'page_library.take_action' }} disabled={busy} onclick={() => void take(page.library_id)} />
            <Button {i18n} label={{ id: 'page_library.duplicate' }} disabled={busy} onclick={() => void perform(() => client.duplicateLibraryPage(page.library_id, page.revision ?? 1))} />
            <Button {i18n} label={{ id: 'page_library.rename' }} disabled={busy} onclick={() => { editing = page.library_id; editName = page.name; deleting = null; }} />
            <Button {i18n} label={{ id: 'page_library.delete' }} disabled={busy} onclick={() => { deleting = page.library_id; deleteConfirmation = ''; editing = null; }} />
          </div>
          {#if editing === page.library_id}
            <form onsubmit={(event) => { event.preventDefault(); void perform(() => client.renameLibraryPage(page.library_id, page.revision ?? 1, editName)); }}>
              <label>{i18n.t('page_library.display_name')} <input bind:value={editName} maxlength="160" required /></label>
              <Button {i18n} type="submit" label={{ id: 'page_library.save_name' }} disabled={busy} />
              <Button {i18n} label={{ id: 'page_library.cancel' }} disabled={busy} onclick={() => { editing = null; }} />
            </form>
          {/if}
          {#if deleting === page.library_id}
            <div class="card">
              <p>{i18n.t('page_library.delete_effect')}</p>
              <p>{i18n.t('page_library.source', { id: page.source_local_id })} · {i18n.t('page_library.catalog_count', { count: page.catalog_versions?.length ?? 0 })}</p>
              <form onsubmit={(event) => { event.preventDefault(); void perform(() => client.deleteLibraryPage(page.library_id, page.revision ?? 1, deleteConfirmation)); }}>
                <label>{i18n.t('page_library.confirm_name', { name: page.name })} <input bind:value={deleteConfirmation} required /></label>
                <Button {i18n} type="submit" variant="danger" label={{ id: 'page_library.confirm_delete' }} disabled={busy || deleteConfirmation !== page.name} />
                <Button {i18n} label={{ id: 'page_library.cancel' }} disabled={busy} onclick={() => { deleting = null; }} />
              </form>
            </div>
          {/if}
        </li>
      {/each}
    </ul>{/if}
  </section>
  <ReusableLibrary {i18n} />
</div>

<style>
  .library-screen { display: grid; gap: var(--space-6); }
  .card { align-content: start; }
  .card h2, .card p { margin: 0; }
  .import-card { max-inline-size: 52rem; }
  .catalog-card { max-inline-size: 52rem; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-3); margin-block: var(--space-3); }
  .browser-select { cursor: pointer; }
  .file-input { position: absolute; inline-size: 1px; block-size: 1px; padding: 0; border: 0; clip-path: inset(50%); overflow: hidden; }
  .browser-select:has(+ .file-input:focus-visible) { outline: 2px solid currentColor; outline-offset: 2px; }
  .preview, .catalogs { margin-block: var(--space-3); }
  li { overflow-wrap: anywhere; }
</style>
