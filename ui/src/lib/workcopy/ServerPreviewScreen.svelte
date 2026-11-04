<script lang="ts">
  import { onMount } from 'svelte';
  import type { I18n } from '../i18n';
  import { Banner, Breadcrumb, Button, Icon, Progress, SelectField, Skeleton, StatusBadge, resolveText } from '../components';
  import StructureSection from '../shell/StructureSection.svelte';
  import { pageName } from '../shell/format';
  import { draftPath, serverCopyPath, serverPath } from '../shell/router';
  import type { ImportOutcome } from './client';
  import RecoveryActions from './RecoveryActions.svelte';
  import { workcopyCodeText } from './messages';
  import { ACCESS_LABEL, OWNER_LABEL, copyNaming, newestCopy, previewStructure } from './server';
  import type { ServerDashboardsStore } from './store.svelte';

  // «Предпросмотр» of a server dashboard (P2-05E, owner decision 28.10.2026): pages, widgets and
  // the grid as in «Structure», read only and without a copy. The composition comes from the one
  // read-only `dashboard.get` of the selection (the same read as the copy check); nothing is
  // stored on this computer. «Copy from server» goes to the copy check, «Take a page» is the
  // existing page import.
  let { i18n, store, connectionLocalId, onimported }: {
    i18n: I18n;
    store: ServerDashboardsStore;
    connectionLocalId: string;
    onimported: (outcome: ImportOutcome, draftName: string) => void;
  } = $props();

  const row = $derived(store.selectedRow);
  const preview = $derived(store.preview && store.preview.dashboardid === store.selectedId ? store.preview : null);
  const name = $derived(preview?.name ?? row?.name ?? '');
  const structure = $derived(preview ? previewStructure(preview) : null);
  const existing = $derived(newestCopy([...(preview?.existing_copies ?? []), ...(row?.existing_copies ?? [])]));
  let takePage = $state<string>('');
  const pageOptions = $derived(
    (preview?.pages ?? []).map((page, index) => ({
      value: page.dashboard_pageid,
      label:
        page.name.trim() === ''
          ? { id: 'object.page.unnamed' as const, params: { number: index + 1 } }
          : { id: 'object.page.numbered' as const, params: { number: index + 1, name: page.name } }
    }))
  );

  onMount(() => {
    if (store.selectedId && !store.preview && !store.previewLoading) void store.loadPreview();
  });

  $effect(() => {
    const pages = preview?.pages ?? [];
    if (!pages.some((page) => page.dashboard_pageid === takePage)) takePage = pages[0]?.dashboard_pageid ?? '';
  });

  async function takeSelectedPage(): Promise<void> {
    const pages = preview?.pages ?? [];
    const index = pages.findIndex((candidate) => candidate.dashboard_pageid === takePage);
    const page = pages[index];
    if (!page) return;
    const shown = pageName(i18n, page.name, index + 1);
    const outcome = await store.importCopy(
      { scope: 'page', dashboard_pageid: page.dashboard_pageid },
      copyNaming(i18n, shown, index + 1)
    );
    if (outcome) onimported(outcome, outcome.name.trim() === '' ? shown : outcome.name);
  }
</script>

<main id="main-content" tabindex="-1">
  <div class="screen server-preview">
    <Breadcrumb
      {i18n}
      label={{ id: 'shell.a11y.breadcrumbs' }}
      items={[
        { label: { id: 'server.dashboards.title' }, href: serverPath(connectionLocalId) },
        { label: { value: name, kind: 'user' }, href: serverPath(connectionLocalId) },
        { label: { id: 'server.preview.breadcrumb' } }
      ]}
    />
    <header class="screen-head">
      <h1>{name}</h1>
      <p class="badges">
        <StatusBadge {i18n} label={{ id: 'server.preview.badge' }} tone="info" />
        {#if row}
          <StatusBadge {i18n} label={OWNER_LABEL[row.owner]} tone="info" />
          <StatusBadge {i18n} label={ACCESS_LABEL[row.access]} tone="info" />
        {/if}
      </p>
      <p class="details-note"><Icon name="info-circle" size={18} /><span>{i18n.t('server.preview.note')}</span></p>
      {#if row}
        <p class="muted">{i18n.t('server.details.owner', { owner: resolveText(i18n, OWNER_LABEL[row.owner]) })}</p>
      {/if}
    </header>

    {#if store.previewFailure}
      <Banner {i18n} heading={{ id: 'server.preview.load_failed' }} body={workcopyCodeText(store.previewFailure.code, store.previewFailure.params)} tone="danger">
        {#snippet actions()}<RecoveryActions {i18n} code={store.previewFailure?.code ?? ''} onretry={() => void store.loadPreview()} />{/snippet}
      </Banner>
    {:else if !preview || !structure}
      <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={6} />
    {:else}
      <div class="card">
        {#if existing}
          <p class="details-note">
            <Icon name="info-circle" size={18} />
            <span>{i18n.t('server.details.existing_copy', { time: existing.snapshot_at })}</span>
          </p>
        {/if}
        {#if store.importFailure}
          <Banner {i18n} heading={{ id: 'copy.result.failed' }} body={workcopyCodeText(store.importFailure.code, store.importFailure.params)} tone="danger">
            {#snippet actions()}<RecoveryActions {i18n} code={store.importFailure?.code ?? ''} onretry={() => void takeSelectedPage()} />{/snippet}
          </Banner>
        {/if}
        {#if store.importing === 'page'}<Progress {i18n} label={{ id: 'copy.progress' }} />{/if}
        <div class="server-preview-actions">
          <a class="button-link primary" href={serverCopyPath(connectionLocalId)}>{i18n.t('server.action.copy')}</a>
          {#if existing}
            <a class="button-link" href={draftPath(existing.local_id)}>{i18n.t('server.action.open_copy')}</a>
          {/if}
          <SelectField
            {i18n}
            id="server-preview-page"
            label={{ id: 'server.preview.page_to_take' }}
            options={pageOptions}
            bind:value={takePage}
            disabled={store.importing !== null}
          />
          <Button
            {i18n}
            label={{ id: 'startup.action.take_page' }}
            busy={store.importing === 'page'}
            disabled={takePage === '' || (store.importing !== null && store.importing !== 'page')}
            onclick={() => void takeSelectedPage()}
          />
        </div>
      </div>

      <StructureSection {i18n} draft={structure} readonly />
    {/if}
  </div>
</main>
