<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { I18n } from '../i18n';
  import {
    Banner,
    Button,
    EmptyState,
    Icon,
    Pagination,
    Progress,
    RadioGroup,
    Skeleton,
    Tabs,
    nextRovingIndex,
    resolveText,
    type LocalizedText
  } from '../components';
  import ComingLater from '../shell/ComingLater.svelte';
  import { pageName } from '../shell/format';
  import { draftPath, serverCopyPath, serverPreviewPath } from '../shell/router';
  import type { ImportOutcome, ServerDashboardRow } from './client';
  import RecoveryActions from './RecoveryActions.svelte';
  import { workcopyCodeText } from './messages';
  import {
    ACCESS_LABEL,
    MAX_SEARCH_CHARS,
    OWNER_LABEL,
    accessFilterOptions,
    copyNaming,
    isAccessFilter,
    mineText,
    newestCopy,
    pageCount,
    searchProblem,
    typeBreakdown,
    typeList
  } from './server';
  import type { ServerDashboardsStore } from './store.svelte';

  // «Панели в Zabbix» → «Панели» (P1-03B, mockup 03-01) for the active connection. The table is a
  // grid with column headers; selecting a row is separate from focus (P1-03H): arrows move focus,
  // Enter or Space selects and reads the composition. Every action starts with a copy; the server
  // is never changed here.
  let { i18n, store, profileName, serverVersion, modeLabel, onimported }: {
    i18n: I18n;
    store: ServerDashboardsStore;
    /** User data: the active profile name. */
    profileName: string;
    /** Technical value, e.g. `7.4.16`; `null` when not known yet. */
    serverVersion: string | null;
    modeLabel: LocalizedText;
    onimported: (outcome: ImportOutcome, draftName: string) => void;
  } = $props();

  let tab = $state('server-tab-dashboards');
  let access = $state<string>(untrack(() => store.access));
  let focusedId = $state<string | null>(null);
  let takePage = $state<string>('');
  let searchError = $state<LocalizedText | null>(null);

  const list = $derived(store.list);
  const rows = $derived(list?.rows ?? []);
  const selected = $derived(store.selectedRow);
  const preview = $derived(store.preview && store.preview.dashboardid === store.selectedId ? store.preview : null);
  const tabStop = $derived(rows.some((row) => row.dashboardid === focusedId) ? focusedId : (store.selectedId ?? rows[0]?.dashboardid ?? null));
  const filterOptions = $derived(accessFilterOptions(i18n, list));
  const breakdown = $derived(preview ? typeBreakdown(i18n, preview.pages) : null);
  // The list and the preview read the same local storage; either may be newer.
  const existing = $derived(newestCopy([...(preview?.existing_copies ?? []), ...(selected?.existing_copies ?? [])]));
  const pageOptions = $derived(
    (preview?.pages ?? []).map((page, index) => ({
      value: page.dashboard_pageid,
      label:
        page.name.trim() === ''
          ? { id: 'object.page.unnamed' as const, params: { number: index + 1 } }
          : { id: 'object.page.numbered' as const, params: { number: index + 1, name: page.name } },
      description: { id: 'object.page.widget_count' as const, params: { count: page.widget_count } }
    }))
  );

  onMount(() => {
    if (!store.list && !store.loading) void store.load();
    if (store.selectedId && !store.preview && !store.previewLoading) void store.loadPreview();
  });

  $effect(() => {
    if (isAccessFilter(access) && access !== store.access) void store.setAccess(access);
  });

  $effect(() => {
    const pages = preview?.pages ?? [];
    if (!pages.some((page) => page.dashboard_pageid === takePage)) takePage = pages[0]?.dashboard_pageid ?? '';
  });

  function rowId(dashboardid: string): string {
    return `server-row-${dashboardid}`;
  }

  function onRowKeydown(event: KeyboardEvent, index: number): void {
    const next = nextRovingIndex(index, rows.length, event.key, 'vertical');
    if (next === index || next < 0) return;
    event.preventDefault();
    const target = rows[next];
    focusedId = target.dashboardid;
    document.getElementById(rowId(target.dashboardid))?.focus();
  }

  function submitSearch(event: SubmitEvent): void {
    event.preventDefault();
    const problem = searchProblem(store.searchInput);
    searchError = problem === null ? null : { id: 'errors.workcopy.search_invalid', params: { max: MAX_SEARCH_CHARS } };
    if (problem === null) void store.applySearch();
  }

  async function takeSelectedPage(): Promise<void> {
    const pages = preview?.pages ?? [];
    const index = pages.findIndex((candidate) => candidate.dashboard_pageid === takePage);
    const page = pages[index];
    if (!page) return;
    const shown = pageName(i18n, page.name, index + 1);
    const outcome = await store.importCopy({ scope: 'page', dashboard_pageid: page.dashboard_pageid }, copyNaming(i18n, shown, index + 1));
    if (outcome) onimported(outcome, outcome.name.trim() === '' ? shown : outcome.name);
  }

  function ownerLine(row: ServerDashboardRow): string {
    return [
      i18n.t('server.details.owner', { owner: resolveText(i18n, OWNER_LABEL[row.owner]) }),
      resolveText(i18n, ACCESS_LABEL[row.access]),
      i18n.t('dashboards.summary.pages_widgets', { pages: row.page_count, widgets: row.widget_count })
    ].join(' · ');
  }
</script>

<main id="main-content" tabindex="-1">
  <div class="screen server">
    <header class="screen-head with-actions">
      <div class="screen-head">
        <h1>{i18n.t('server.dashboards.title')}</h1>
        <p class="lead">
          {i18n.t('server.subtitle', { profile: profileName, version: serverVersion ?? '—', mode: resolveText(i18n, modeLabel) })}
        </p>
      </div>
      <Button {i18n} label={{ id: 'server.action.refresh' }} busy={store.loading} onclick={() => void store.load()} />
    </header>

    <Tabs
      {i18n}
      label={{ id: 'server.dashboards.title' }}
      tabs={[
        { id: 'server-tab-dashboards', label: { id: 'server.tab.dashboards' }, panelId: 'server-panel-dashboards' },
        { id: 'server-tab-templates', label: { id: 'server.tab.templates' }, panelId: 'server-panel-templates' }
      ]}
      bind:selected={tab}
    />

    <div id="server-panel-templates" class="tab-panel" role="tabpanel" aria-labelledby="server-tab-templates" hidden={tab !== 'server-tab-templates'}>
      <EmptyState {i18n} heading={{ id: 'server.tab.templates' }} reason={{ id: 'server.templates.later' }} />
    </div>

    <div id="server-panel-dashboards" class="tab-panel" role="tabpanel" aria-labelledby="server-tab-dashboards" hidden={tab !== 'server-tab-dashboards'}>
      <div class="filters">
        <RadioGroup {i18n} name="server-access" legend={{ id: 'server.filter.legend' }} options={filterOptions} bind:value={access} />
        <form class="search-form" role="search" aria-label={i18n.t('server.search.placeholder')} onsubmit={submitSearch}>
          <div class="search-field">
            <Icon name="search" size={18} />
            <input
              id="server-search"
              type="search"
              autocomplete="off"
              aria-label={i18n.t('server.search.placeholder')}
              placeholder={i18n.t('server.search.placeholder')}
              aria-invalid={searchError ? 'true' : undefined}
              aria-describedby={searchError ? 'server-search-error' : undefined}
              bind:value={store.searchInput}
            />
          </div>
          <Button {i18n} type="submit" label={{ id: 'server.search.action' }} />
          {#if searchError}<span id="server-search-error" class="field-error">{resolveText(i18n, searchError)}</span>{/if}
        </form>
      </div>

      {#if store.listFailure}
        <Banner {i18n} heading={{ id: 'server.list.load_failed' }} body={workcopyCodeText(store.listFailure.code, store.listFailure.params)} tone="danger">
          {#snippet actions()}<RecoveryActions {i18n} code={store.listFailure?.code ?? ''} onretry={() => void store.load()} />{/snippet}
        </Banner>
      {:else if list === null}
        <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={5} />
      {:else if rows.length === 0}
        <EmptyState {i18n} heading={{ id: 'dashboards.permission.not_visible_or_denied' }} reason={{ id: 'server.visibility_note' }} />
      {:else}
        <div class="drafts-layout server-layout">
          <div class="drafts-list" aria-busy={store.loading}>
            <div class="table-scroll">
              <table>
                <caption class="visually-hidden">{i18n.t('server.dashboards.title')}</caption>
                <thead>
                  <tr>
                    <th scope="col">{i18n.t('server.column.name')}</th>
                    <th scope="col">{i18n.t('server.column.owner')}</th>
                    <th scope="col">{i18n.t('server.column.access')}</th>
                    <th scope="col" class="numeric">{i18n.t('server.column.pages')}</th>
                    <th scope="col" class="numeric">{i18n.t('server.column.widgets')}</th>
                    <th scope="col">{i18n.t('server.column.editable')}</th>
                    <th scope="col">{i18n.t('server.column.mine')}</th>
                  </tr>
                </thead>
                <tbody>
                  {#each rows as row, index (row.dashboardid)}
                    {@const isSelected = row.dashboardid === store.selectedId}
                    <tr class:selected={isSelected}>
                      <td>
                        <button
                          id={rowId(row.dashboardid)}
                          type="button"
                          class="row-select"
                          aria-pressed={isSelected}
                          aria-controls="server-details"
                          tabindex={row.dashboardid === tabStop ? 0 : -1}
                          onfocus={() => {
                            focusedId = row.dashboardid;
                          }}
                          onclick={() => void store.select(row)}
                          onkeydown={(event) => onRowKeydown(event, index)}
                        >{row.name}</button>
                      </td>
                      <td>{resolveText(i18n, OWNER_LABEL[row.owner])}</td>
                      <td>{resolveText(i18n, ACCESS_LABEL[row.access])}</td>
                      <td class="numeric">{i18n.number(row.page_count)}</td>
                      <td class="numeric">{i18n.number(row.widget_count)}</td>
                      <td>{i18n.t('server.editable_count', { editable: row.editable_widget_count, total: row.widget_count })}</td>
                      <td>{mineText(i18n, row.existing_copies, list.existing_copies_known)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
            {#if pageCount(list) > 1}
              <Pagination
                {i18n}
                label={{ id: 'server.pagination.label' }}
                previousLabel={{ id: 'server.pagination.previous' }}
                nextLabel={{ id: 'server.pagination.next' }}
                page={store.page}
                pages={pageCount(list)}
                onchange={(page) => void store.setPage(page)}
              />
            {/if}
            {#if list.truncated}
              <p class="details-note"><Icon name="alert-triangle" size={18} /><span>{i18n.t('server.list.truncated', { count: list.counts.all })}</span></p>
            {/if}
            {#if !list.existing_copies_known}
              <p class="details-note"><Icon name="alert-triangle" size={18} /><span>{i18n.t('server.list.copies_unknown')}</span></p>
            {/if}
            <p class="details-note"><Icon name="info-circle" size={18} /><span>{i18n.t('server.visibility_note')}</span></p>
          </div>

          <aside id="server-details" class="card drafts-details" aria-labelledby="server-details-title" aria-busy={store.previewLoading}>
            {#if selected}
              <h2 id="server-details-title">{selected.name}</h2>
              <p class="muted">{ownerLine(selected)}</p>

              {#if store.previewFailure}
                <Banner {i18n} heading={{ id: 'server.preview.load_failed' }} body={workcopyCodeText(store.previewFailure.code, store.previewFailure.params)} tone="danger">
                  {#snippet actions()}<RecoveryActions {i18n} code={store.previewFailure?.code ?? ''} onretry={() => void store.loadPreview()} />{/snippet}
                </Banner>
              {:else if !preview}
                <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={3} />
              {:else}
                <RadioGroup {i18n} name="server-take-page" legend={{ id: 'server.details.pages' }} options={pageOptions} bind:value={takePage} disabled={store.importing !== null} />

                {#if breakdown}
                  <section class="server-widgets" aria-labelledby="server-widgets-title">
                    <h3 id="server-widgets-title" class="eyebrow">{i18n.t('server.details.widgets')}</h3>
                    <p>
                      <span>{i18n.t('server.details.editable', { count: breakdown.editable })}</span>
                      {#if breakdown.editableTypes.length > 0}<span class="muted">{typeList(i18n, breakdown.editableTypes)}</span>{/if}
                    </p>
                    {#if breakdown.other > 0}
                      <p>
                        <span>{i18n.t('server.details.not_editable', { count: breakdown.other })}</span>
                        <span class="muted">{[typeList(i18n, breakdown.otherTypes), i18n.t('server.details.copied_as_is')].join('; ')}</span>
                      </p>
                    {/if}
                  </section>
                {/if}
              {/if}

              {#if existing}
                <p class="details-note">
                  <Icon name="info-circle" size={18} />
                  <span>{i18n.t('server.details.existing_copy', { time: existing.snapshot_at })}</span>
                </p>
                <a href={draftPath(existing.local_id)} aria-describedby="server-details-title">{i18n.t('server.action.open_copy')}</a>
              {/if}

              {#if store.importFailure}
                <Banner {i18n} heading={{ id: 'copy.result.failed' }} body={workcopyCodeText(store.importFailure.code, store.importFailure.params)} tone="danger">
                  {#snippet actions()}<RecoveryActions {i18n} code={store.importFailure?.code ?? ''} onretry={() => void takeSelectedPage()} />{/snippet}
                </Banner>
              {/if}
              {#if store.importing === 'page'}<Progress {i18n} label={{ id: 'copy.progress' }} />{/if}

              <div class="details-actions">
                <!-- P2-05E: look into the dashboard read only, without a copy. -->
                <a
                  class="button-link"
                  href={serverPreviewPath(store.connectionLocalId ?? '')}
                  aria-describedby="server-details-title"
                >{i18n.t('server.action.preview')}</a>
                <a
                  class="button-link primary"
                  href={serverCopyPath(store.connectionLocalId ?? '')}
                  aria-describedby="server-details-title"
                >{i18n.t('server.action.copy')}</a>
                <Button
                  {i18n}
                  label={{ id: 'startup.action.take_page' }}
                  describedby="server-details-title"
                  busy={store.importing === 'page'}
                  disabled={!preview || takePage === '' || (store.importing !== null && store.importing !== 'page')}
                  onclick={() => void takeSelectedPage()}
                />
                <ComingLater {i18n} id="server-details-later">
                  <Button {i18n} label={{ id: 'server.action.clone' }} disabled />
                  <Button {i18n} label={{ id: 'intent.replace_host' }} disabled />
                </ComingLater>
              </div>
              <p class="details-note"><Icon name="info-circle" size={18} /><span>{i18n.t('server.actions_start_with_copy')}</span></p>
            {:else}
              <h2 id="server-details-title" class="visually-hidden">{i18n.t('server.dashboards.title')}</h2>
              <p class="muted">{i18n.t('server.details.select_hint')}</p>
            {/if}
          </aside>
        </div>
      {/if}
      <p class="visually-hidden" role="status">
        {#if list}{resolveText(i18n, { id: 'common.filter.with_count', params: { label: i18n.t('server.dashboards.title'), count: list.total } })}{/if}
      </p>
    </div>
  </div>
</main>
