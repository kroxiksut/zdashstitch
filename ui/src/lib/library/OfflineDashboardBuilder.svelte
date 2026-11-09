<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { I18n } from '../i18n';
  import { createCatalogClient, type OfflineDashboard, type OfflinePageSummary } from './client';
  import { dashboardPagePath, draftPath, internalLinkPath } from '../shell/router';

  let { i18n, initialDashboard = null, onchanged }: {
    i18n: I18n;
    initialDashboard?: OfflineDashboard | null;
    onchanged?: (dashboard: OfflineDashboard) => void;
  } = $props();
  const client = createCatalogClient();
  let opened = $state<OfflineDashboard | null>(untrack(() => initialDashboard));
  let name = $state(untrack(() => initialDashboard?.bundle.project.root.name ?? ''));
  let existing = $state(untrack(() => (initialDashboard?.bundle.project.root.pages ?? []).map((page) => ({ id: page.id, name: page.name, widgets: page.widgets.length }))));
  let sources = $state<OfflinePageSummary[]>([]);
  let append = $state<string[]>([]);
  let selectedSource = $state('');
  let busy = $state(false);
  let failed = $state(false);
  let exportFailed = $state(false);
  let exporting = $state(false);
  let conflict = $state(false);
  let failureCode = $state('');
  let saved = $state(false);
  let dirty = $state(false);

  onMount(() => {
    void client.pages().then((pages) => {
      sources = pages;
      selectedSource = pages[0]?.local_id ?? '';
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

  async function exportPackage(): Promise<void> {
    if (!opened || dirty || busy || exporting) return;
    exporting = true;
    exportFailed = false;
    try {
      const download = await client.prepareDashboardExport(opened.local_id);
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
</script>

<section class="card dashboard-builder" aria-labelledby="offline-dashboard-title">
  <h2 id="offline-dashboard-title">{i18n.t('offline_dashboard.title')}</h2>
  <p class="muted">{i18n.t('offline_dashboard.help')}</p>
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
    <select id="offline-dashboard-source" bind:value={selectedSource} disabled={busy || conflict || sources.length === 0}>
      {#each sources as source (source.local_id)}
        <option value={source.local_id}>{source.name}</option>
      {/each}
    </select>
    <button type="button" disabled={busy || conflict || !selectedSource} onclick={add}>{i18n.t('offline_dashboard.add')}</button>
  </div>
  {#if sources.length === 0}<p class="muted">{i18n.t('offline_dashboard.no_pages')}</p>{/if}
  {#if append.length > 0}
    <h3>{i18n.t('offline_dashboard.pending')}</h3>
    <ul>
      {#each append as id (id)}
        <li><bdi>{sources.find((page) => page.local_id === id)?.name ?? id}</bdi>
          <button type="button" disabled={busy} onclick={() => { append = append.filter((candidate) => candidate !== id); dirty = true; }}>{i18n.t('offline_dashboard.remove')}</button>
        </li>
      {/each}
    </ul>
  {/if}
  <button type="button" disabled={busy || conflict || !name.trim() || existing.length + append.length === 0} onclick={() => void save()}>{i18n.t('offline_dashboard.save')}</button>
  {#if opened}
    <button type="button" disabled={busy || exporting || dirty} onclick={() => void exportPackage()}>{i18n.t('offline_dashboard.export')}</button>
    <p class="muted">{i18n.t('offline_dashboard.export_caution')}</p>
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
