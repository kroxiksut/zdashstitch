<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { I18n } from '../i18n';
  import type { ActiveConnection, ConnectionProfile } from '../connections/client';
  import PortablePublication from '../publication/PortablePublication.svelte';
  import { createCatalogClient, type OfflineDashboard, type OfflinePageSummary, type LibraryPageSummary } from './client';
  import { dashboardPagePath, draftPath, internalLinkPath } from '../shell/router';
  import { choosePageSource } from './page-source-selection';
  import SensitivePreview from './SensitivePreview.svelte';
  import PortablePatternReview from './PortablePatternReview.svelte';
  import PortableLldReview from './PortableLldReview.svelte';
  import RedactionPreview from './RedactionPreview.svelte';
  import SaveAsButton from './SaveAsButton.svelte';

  let { i18n, initialDashboard = null, active = null, profile = null, onchanged }: {
    i18n: I18n;
    initialDashboard?: OfflineDashboard | null;
    active?: ActiveConnection | null;
    profile?: ConnectionProfile | null;
    onchanged?: (dashboard: OfflineDashboard) => void;
  } = $props();
  const client = createCatalogClient();
  let opened = $state<OfflineDashboard | null>(untrack(() => initialDashboard));
  let name = $state(untrack(() => initialDashboard?.bundle.project.root.name ?? ''));
  let existing = $state(untrack(() => (initialDashboard?.bundle.project.root.pages ?? []).map((page) => ({ id: page.id, name: page.name, widgets: page.widgets.length }))));
  let sources = $state<OfflinePageSummary[]>([]);
  let librarySources = $state<LibraryPageSummary[]>([]);
  let append = $state<string[]>([]);
  let selectedSource = $state('');
  let busy = $state(false);
  let failed = $state(false);
  let exportFailed = $state(false);
  let exportHash = $state<string | null>(null);
  let exportEntries = $state<Record<string, string> | null>(null);
  let redactionJump = $state<{ location: string; sequence: number } | null>(null);
  let exporting = $state(false);
  let conflict = $state(false);
  let failureCode = $state('');
  let saved = $state(false);
  let dirty = $state(false);
  $effect(() => { if (dirty) { exportHash = null; exportEntries = null; } });

  onMount(() => {
    void client.pages().then((pages) => {
      sources = pages;
      selectedSource = choosePageSource(selectedSource, pages.map((page) => page.local_id));
    }).catch(() => { failed = true; });
    void client.libraryPages().then((pages) => {
      librarySources = pages;
      selectedSource = choosePageSource(selectedSource, pages.map((page) => page.library_id));
    }).catch(() => { failed = true; });
    const currentPath = location.pathname;
    let bypass = false;
    const beforeUnload = (event: BeforeUnloadEvent): void => {
      if (dirty || busy) { event.preventDefault(); event.returnValue = ''; }
    };
    const clicked = (event: MouseEvent): void => {
      const anchor = event.target instanceof Element ? event.target.closest('a') : null;
      if (!anchor) return;
      const path = internalLinkPath(anchor, location.origin, event);
      if (path === null || path === location.pathname || bypass || (!dirty && !busy)) return;
      event.preventDefault(); event.stopImmediatePropagation();
      if (window.confirm(i18n.t('offline_dashboard.leave_dirty'))) { bypass = true; anchor.click(); }
    };
    const popped = (event: PopStateEvent): void => {
      if (bypass) { bypass = false; return; }
      if (!dirty && !busy) return;
      const target = location.pathname;
      history.pushState(null, '', currentPath);
      event.stopImmediatePropagation();
      if (window.confirm(i18n.t('offline_dashboard.leave_dirty'))) {
        bypass = true; history.replaceState(null, '', target); window.dispatchEvent(new PopStateEvent('popstate'));
      }
    };
    window.addEventListener('beforeunload', beforeUnload);
    window.addEventListener('popstate', popped, true);
    document.addEventListener('click', clicked, true);
    return () => {
      window.removeEventListener('beforeunload', beforeUnload);
      window.removeEventListener('popstate', popped, true);
      document.removeEventListener('click', clicked, true);
    };
  });

  function move(index: number, offset: number): void {
    const next = index + offset;
    if (next < 0 || next >= existing.length) return;
    const copy = [...existing];
    [copy[index], copy[next]] = [copy[next], copy[index]];
    existing = copy;
    dirty = true;
    saved = false;
  }

  function add(): void {
    if (!selectedSource || append.includes(selectedSource) || existing.some((page) => page.id === selectedSource)) return;
    append = [...append, selectedSource];
    dirty = true;
    saved = false;
  }

  async function save(): Promise<void> {
    if (busy || conflict || !name.trim() || existing.length + append.length === 0) return;
    busy = true;
    failed = false;
    exportFailed = false;
    exportHash = null;
    exportEntries = null;
    failureCode = '';
    saved = false;
    try {
      const dashboard = opened
        ? await client.updateDashboard(opened.local_id, opened.revision, name, existing.map((page) => page.id), append)
        : await client.createDashboard(name, append);
      if (!opened) {
        dirty = false;
        busy = false;
        window.location.assign(draftPath(dashboard.local_id));
        return;
      }
      opened = dashboard;
      existing = dashboard.bundle.project.root.pages.map((page) => ({ id: page.id, name: page.name, widgets: page.widgets.length }));
      append = [];
      dirty = false;
      saved = true;
      onchanged?.(dashboard);
    } catch (error) {
      failureCode = error instanceof Error && 'response' in error
        ? (error as { response?: { code?: string } }).response?.code ?? ''
        : '';
      conflict = failureCode === 'offline_dashboard_conflict';
      failed = true;
    } finally {
      busy = false;
    }
  }

  async function exportPackage(yaml = false): Promise<void> {
    if (!opened || dirty || busy || exporting) return;
    exporting = true;
    exportFailed = false;
    exportHash = null;
    exportEntries = null;
    try {
      const download = yaml ? await client.prepareDashboardYamlExport(opened.local_id, opened.revision) : await client.prepareDashboardExport(opened.local_id, opened.revision);
      exportHash = download.sha256;
      exportEntries = Object.keys(download.entry_hashes).length ? download.entry_hashes : null;
      const link = document.createElement('a');
      link.href = `/api/downloads/${encodeURIComponent(download.id)}`;
      link.download = download.display_name;
      document.body.append(link);
      link.click();
      link.remove();
    } catch {
      exportFailed = true;
    } finally {
      exporting = false;
    }
  }
  let exportPath = $state<string | null>(null);
  async function savePackage(yaml: boolean, jobId: string): Promise<{ cancelled: boolean }> {
    if (!opened || dirty || busy || exporting) return { cancelled: true };
    exporting = true; exportFailed = false; exportPath = null; exportHash = null; exportEntries = null;
    try {
      const result = await client.saveExport('dashboard', opened.local_id, opened.revision, yaml, jobId);
      if (!result.cancelled) { exportPath = result.path ?? null; exportHash = result.sha256 ?? null; exportEntries = result.entry_hashes && Object.keys(result.entry_hashes).length ? result.entry_hashes : null; }
      return result;
    } catch (error) { exportFailed = true; throw error; }
    finally { exporting = false; }
  }
</script>

<section class="card dashboard-builder" aria-labelledby="offline-dashboard-title">
  <h2 id="offline-dashboard-title">{i18n.t('offline_dashboard.title')}</h2>
  <p class="muted">{i18n.t('offline_dashboard.help')}</p>
  {#if opened}<PortablePatternReview {i18n} draft={opened} enabled={!dirty && !busy} />{/if}
  {#if opened}<PortableLldReview {i18n} draft={opened} enabled={!dirty && !busy} />{/if}
  {#if opened}<PortablePublication {i18n} draft={opened} enabled={!dirty && !busy} {active} {profile} />{/if}
  <label for="offline-dashboard-name">{i18n.t('offline_dashboard.name')}</label>
  <input id="offline-dashboard-name" maxlength="120" bind:value={name} oninput={() => { dirty = true; saved = false; }} disabled={busy || conflict} />

  {#if existing.length > 0}
    <h3>{i18n.t('offline_dashboard.pages')}</h3>
    <ol class="page-list">
      {#each existing as page, index (page.id)}
        <li>
          {#if opened}<a href={dashboardPagePath(opened.local_id, page.id)}><bdi>{page.name}</bdi></a>{:else}<bdi>{page.name}</bdi>{/if}
          <span class="muted">{i18n.t('offline_dashboard.widget_count', { count: page.widgets })}</span>
          <button type="button" disabled={busy || conflict || index === 0} onclick={() => move(index, -1)} aria-label={i18n.t('offline_dashboard.move_up')}>{i18n.t('offline_dashboard.move_up')}</button>
          <button type="button" disabled={busy || conflict || index === existing.length - 1} onclick={() => move(index, 1)} aria-label={i18n.t('offline_dashboard.move_down')}>{i18n.t('offline_dashboard.move_down')}</button>
          <button type="button" disabled={busy || conflict} onclick={() => { existing = existing.filter((entry) => entry.id !== page.id); dirty = true; saved = false; }}>{i18n.t('offline_dashboard.remove')}</button>
        </li>
      {/each}
    </ol>
  {/if}

  <label for="offline-dashboard-source">{i18n.t('offline_dashboard.add_page')}</label>
  <div class="source-row">
    <select id="offline-dashboard-source" bind:value={selectedSource} disabled={busy || conflict || sources.length + librarySources.length === 0}>
      {#each sources as source (source.local_id)}
        <option value={source.local_id}>{i18n.t('page_library.draft_source')}: {source.name}</option>
      {/each}
      {#each librarySources as source (source.library_id)}
        <option value={source.library_id}>{i18n.t('page_library.library_source')}: {source.name}</option>
      {/each}
    </select>
    <button type="button" disabled={busy || conflict || !selectedSource} onclick={add}>{i18n.t('offline_dashboard.add')}</button>
  </div>
  {#if sources.length + librarySources.length === 0}<p class="muted">{i18n.t('offline_dashboard.no_pages')}</p>{/if}
  {#if append.length > 0}
    <h3>{i18n.t('offline_dashboard.pending')}</h3>
    <ul>
      {#each append as id (id)}
        <li><bdi>{sources.find((page) => page.local_id === id)?.name ?? librarySources.find((page) => page.library_id === id)?.name ?? id}</bdi>
          <button type="button" disabled={busy} onclick={() => { append = append.filter((candidate) => candidate !== id); dirty = true; }}>{i18n.t('offline_dashboard.remove')}</button>
        </li>
      {/each}
    </ul>
  {/if}
  <button type="button" disabled={busy || conflict || !name.trim() || existing.length + append.length === 0} onclick={() => void save()}>{i18n.t('offline_dashboard.save')}</button>
  {#if opened}
    <SensitivePreview {i18n} kind="dashboard" id={opened.local_id} revision={opened.revision} enabled={!dirty && !busy} onFinding={(location) => { redactionJump = { location, sequence: (redactionJump?.sequence ?? 0) + 1 }; }} />
    <RedactionPreview {i18n} kind="dashboard" id={opened.local_id} revision={opened.revision} enabled={!dirty && !busy} pages={opened.bundle.project.root.pages} roles={opened.bundle.project.roles} catalogs={opened.bundle.catalogs} jump={redactionJump} />
    <button type="button" disabled={busy || exporting || dirty} onclick={() => void exportPackage()}>{i18n.t('offline_dashboard.export')}</button>
    <SaveAsButton {i18n} messageId="redaction.save_zip_as" disabled={busy || dirty || exporting} run={(job) => savePackage(false, job)} onDone={() => {}} onError={() => { exportFailed = true; }} />
    {#if opened.bundle.project.catalogs.length === 0}<button type="button" disabled={busy || exporting || dirty} onclick={() => void exportPackage(true)}>{i18n.t('package_import.export_yaml')}</button>{/if}
    {#if opened.bundle.project.catalogs.length === 0}<SaveAsButton {i18n} messageId="redaction.save_yaml_as" disabled={busy || exporting || dirty} run={(job) => savePackage(true, job)} onDone={() => {}} onError={() => { exportFailed = true; }} />{/if}
    <p class="muted">{i18n.t('redaction.save_no_overwrite')}</p>
    {#if exportPath}<p role="status">{i18n.t('redaction.saved')}: <bdi>{exportPath}</bdi></p>{/if}
    <p class="muted">{i18n.t('offline_dashboard.export_caution')}</p>
    {#if exportHash}<p role="status">{i18n.t('package_import.export_hash')}: <code>{exportHash}</code></p>{/if}
    {#if exportEntries}<details><summary>{i18n.t('package_import.file_hashes')}</summary><ul>{#each Object.entries(exportEntries) as [name, hash] (name)}<li><bdi>{name}</bdi>: <code>{hash}</code></li>{/each}</ul></details>{/if}
    {#if dirty}<p class="muted">{i18n.t('offline_dashboard.save_before_export')}</p>{/if}
  {/if}
  <div aria-live="polite">
    {#if saved}<p>{i18n.t('offline_dashboard.saved')}</p>{/if}
    {#if failed}<p role="alert">{i18n.t(
      conflict ? 'offline_dashboard.conflict'
        : failureCode === 'offline_dashboard_reference_conflict' ? 'offline_dashboard.reference_conflict'
          : failureCode === 'offline_dashboard_reference_unresolved' ? 'offline_dashboard.reference_unresolved'
            : 'offline_dashboard.failed'
    )}</p>{/if}
    {#if exportFailed}<p role="alert">{i18n.t('offline_dashboard.export_failed')}</p>{/if}
  </div>
</section>

<style>
  .dashboard-builder { display: grid; gap: var(--space-3); max-inline-size: 52rem; }
  .dashboard-builder h2, .dashboard-builder h3, .dashboard-builder p { margin: 0; }
  .source-row, .page-list li { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
  .page-list { display: grid; gap: var(--space-2); }
</style>
