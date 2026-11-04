<script lang="ts">
  import { onMount } from 'svelte';
  import type { I18n } from '../i18n';
  import { Banner, Breadcrumb, Button, Icon, Progress, Skeleton, StatusBadge } from '../components';
  import { displayName, numberedPageName, widgetTypeName } from '../shell/format';
  import { draftPath, serverPath } from '../shell/router';
  import type { ImportOutcome } from './client';
  import RecoveryActions from './RecoveryActions.svelte';
  import { workcopyCodeText } from './messages';
  import { copyNaming, newestCopy } from './server';
  import type { ServerDashboardsStore } from './store.svelte';

  // «Проверка перед копированием» (P1-03C, mockup 03-03): composition, dependencies, what is saved
  // and what is not, then one consistent import (P1-03D). With an existing copy the primary
  // action is «Open my copy»; «New copy» creates a separate one and leaves the existing one as is.
  let { i18n, store, connectionLocalId, onimported }: {
    i18n: I18n;
    store: ServerDashboardsStore;
    connectionLocalId: string;
    onimported: (outcome: ImportOutcome, draftName: string) => void;
  } = $props();

  const row = $derived(store.selectedRow);
  const preview = $derived(store.preview && store.preview.dashboardid === store.selectedId ? store.preview : null);
  const name = $derived(preview?.name ?? row?.name ?? '');
  const existing = $derived(newestCopy([...(preview?.existing_copies ?? []), ...(row?.existing_copies ?? [])]));
  const importing = $derived(store.importing !== null);

  onMount(() => {
    if (store.selectedId && !store.preview && !store.previewLoading) void store.loadPreview();
  });

  async function copy(): Promise<void> {
    const outcome = await store.importCopy({ scope: 'dashboard' }, copyNaming(i18n, name));
    if (outcome) onimported(outcome, outcome.name.trim() === '' ? name : outcome.name);
  }
</script>

<main id="main-content" tabindex="-1">
  <div class="screen copy-check">
    <Breadcrumb
      {i18n}
      label={{ id: 'shell.a11y.breadcrumbs' }}
      items={[
        { label: { id: 'server.dashboards.title' }, href: serverPath(connectionLocalId) },
        { label: { value: name, kind: 'user' }, href: serverPath(connectionLocalId) },
        { label: { id: 'copy.breadcrumb' } }
      ]}
    />
    <header class="screen-head">
      <h1>{i18n.t('copy.title', { dashboard_name: name })}</h1>
      {#if preview}
        <p class="lead">
          {i18n.t('copy.scope.whole', {
            summary: i18n.t('dashboards.summary.pages_widgets', {
              pages: preview.pages.length,
              widgets: preview.pages.reduce((total, page) => total + page.widget_count, 0)
            })
          })}
        </p>
      {/if}
    </header>

    {#if store.previewFailure}
      <Banner {i18n} heading={{ id: 'server.preview.load_failed' }} body={workcopyCodeText(store.previewFailure.code, store.previewFailure.params)} tone="danger">
        {#snippet actions()}<RecoveryActions {i18n} code={store.previewFailure?.code ?? ''} onretry={() => void store.loadPreview()} />{/snippet}
      </Banner>
    {:else if !preview}
      <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={6} />
    {:else}
      <div class="overview-layout">
        <div class="overview-main">
          <section class="card" aria-labelledby="copy-composition-title">
            <h2 id="copy-composition-title" class="eyebrow">{i18n.t('copy.section.composition')}</h2>
            {#each preview.pages as page, index (page.dashboard_pageid)}
              <details class="copy-page" open={index === 0}>
                <summary>
                  <span class="copy-page-title">{numberedPageName(i18n, page.name, index + 1)}</span>
                  <span class="muted">
                    {i18n.t('copy.page_summary', {
                      widgets: i18n.t('object.page.widget_count', { count: page.widget_count }),
                      editable: page.editable_widget_count,
                      other: page.widget_count - page.editable_widget_count
                    })}
                  </span>
                </summary>
                {#if page.widgets.length > 0}
                  <div class="table-scroll">
                    <table>
                      <caption class="visually-hidden">{numberedPageName(i18n, page.name, index + 1)}</caption>
                      <thead>
                        <tr>
                          <th scope="col">{i18n.t('drafts.column.name')}</th>
                          <th scope="col">{i18n.t('drafts.column.kind')}</th>
                          <th scope="col">{i18n.t('server.column.editable')}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {#each page.widgets as widget, widgetIndex (widgetIndex)}
                          <tr>
                            <td>{displayName(widget.name, widgetTypeName(i18n, widget.type))}</td>
                            <td>{widgetTypeName(i18n, widget.type)}</td>
                            <td>
                              {#if widget.editable}
                                {i18n.t('copy.widget.editable')}
                              {:else}
                                <StatusBadge {i18n} label={{ id: 'copy.widget.as_is' }} tone="warning" />
                              {/if}
                            </td>
                          </tr>
                        {/each}
                      </tbody>
                    </table>
                  </div>
                {/if}
              </details>
            {/each}
          </section>

          <section class="card" aria-labelledby="copy-dependencies-title">
            <h2 id="copy-dependencies-title" class="eyebrow">{i18n.t('copy.section.dependencies')}</h2>
            <dl class="details-list">
              <div>
                <dt>{i18n.t('copy.dep.hosts')}</dt>
                <dd>
                  {i18n.t('copy.dep.by_id_or_pattern', {
                    total: preview.dependencies.hosts + preview.dependencies.host_patterns,
                    by_id: preview.dependencies.hosts,
                    by_pattern: preview.dependencies.host_patterns
                  })}
                </dd>
              </div>
              <div>
                <dt>{i18n.t('copy.dep.items')}</dt>
                <dd>
                  {i18n.t('copy.dep.by_id_or_pattern', {
                    total: preview.dependencies.items + preview.dependencies.item_patterns,
                    by_id: preview.dependencies.items,
                    by_pattern: preview.dependencies.item_patterns
                  })}
                </dd>
              </div>
              <div>
                <dt>{i18n.t('copy.dep.graphs')}</dt>
                <dd>{i18n.number(preview.dependencies.graphs)}</dd>
              </div>
              <div>
                <dt>{i18n.t('copy.dep.references')}</dt>
                <dd>{i18n.t('copy.dep.references_value', { count: preview.dependencies.widget_references })}</dd>
              </div>
              <div>
                <dt>{i18n.t('copy.dep.unknown_fields')}</dt>
                <dd>{i18n.t('copy.dep.unknown_fields_value', { count: preview.unknown_field_count })}</dd>
              </div>
              {#if preview.dependencies.unknown > 0}
                <div>
                  <dt>{i18n.t('copy.dep.other')}</dt>
                  <dd>{i18n.number(preview.dependencies.unknown)}</dd>
                </div>
              {/if}
            </dl>
            <p class="muted">{i18n.t('copy.dep.not_copied')}</p>
          </section>
        </div>

        <section class="card copy-saved" aria-labelledby="copy-saved-title">
          <h2 id="copy-saved-title" class="eyebrow">{i18n.t('copy.section.saved')}</h2>
          <ul class="copy-list">
            <li>{i18n.t('copy.saved.snapshot')}</li>
            <li>{i18n.t('copy.saved.draft')}</li>
            <li>{i18n.t('copy.saved.access')}</li>
          </ul>
          <h2 id="copy-not-saved-title" class="eyebrow">{i18n.t('copy.section.not_saved')}</h2>
          <p>{i18n.t('copy.not_saved')}</p>
          <p class="details-note"><Icon name="info-circle" size={18} /><span>{i18n.t('copy.read_twice')}</span></p>
          {#if existing}
            <p class="details-note">
              <Icon name="info-circle" size={18} />
              <span>{i18n.t('copy.existing', { time: existing.snapshot_at })} {i18n.t('copy.existing.refetch_hint')}</span>
            </p>
          {/if}
        </section>
      </div>
    {/if}

    {#if store.importFailure}
      <Banner {i18n} heading={{ id: 'copy.result.failed' }} body={workcopyCodeText(store.importFailure.code, store.importFailure.params)} tone="danger">
        {#snippet actions()}<RecoveryActions {i18n} code={store.importFailure?.code ?? ''} onretry={() => void copy()} />{/snippet}
      </Banner>
    {/if}
    {#if importing}<Progress {i18n} label={{ id: 'copy.progress' }} />{/if}

    <div class="actions copy-actions">
      <a class="button-link" href={serverPath(connectionLocalId)}>{i18n.t('common.action.cancel')}</a>
      {#if existing}
        <Button {i18n} label={{ id: 'copy.action.new_copy' }} busy={importing} disabled={!preview} onclick={() => void copy()} />
        <a class="button-link primary" href={draftPath(existing.local_id)}>{i18n.t('copy.action.open_mine')}</a>
      {:else}
        <Button {i18n} variant="primary" label={{ id: 'server.action.copy' }} busy={importing} disabled={!preview} onclick={() => void copy()} />
      {/if}
    </div>
  </div>
</main>
