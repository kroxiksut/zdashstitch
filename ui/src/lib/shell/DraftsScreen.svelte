<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, DataTable, EmptyState, RadioGroup, Skeleton, resolveText } from '../components';
  import ComingLater from './ComingLater.svelte';
  import {
    filterDrafts,
    kindCounts,
    sortByUpdated,
    type DraftListResult,
    type KindFilter,
    type OriginFilter
  } from './drafts';
  import { sourceText, stateText } from './format';
  import { KIND_LABEL, ORIGIN_LABEL } from './labels';
  import { draftPath } from './router';

  // «Drafts» (P1-03F, mockup 02-01). `drafts` is null while the list is loading.
  let { i18n, drafts }: { i18n: I18n; drafts: DraftListResult | null } = $props();

  // RadioGroup binds plain strings; only known filter values are offered.
  let kind = $state<string>('all');
  let origin = $state<string>('any');

  const all = $derived(drafts?.status === 'ready' ? sortByUpdated(drafts.drafts) : []);
  const counts = $derived(kindCounts(all));
  const visible = $derived(filterDrafts(all, kind as KindFilter, origin as OriginFilter));
  const rows = $derived(
    visible.map((draft) => ({
      id: draft.local_id,
      href: draftPath(draft.local_id),
      cells: [
        { value: draft.name, kind: 'user' as const },
        KIND_LABEL[draft.kind],
        ORIGIN_LABEL[draft.origin],
        { value: draft.source ? sourceText(i18n, draft.source) : '—', kind: 'user' as const },
        { value: stateText(i18n, draft), kind: 'user' as const }
      ]
    }))
  );
  const kindOptions = $derived([
    { value: 'all', label: { id: 'common.filter.with_count' as const, params: { label: i18n.t('common.filter.all'), count: counts.all } } },
    { value: 'dashboard', label: { id: 'common.filter.with_count' as const, params: { label: i18n.t('drafts.filter.dashboards'), count: counts.dashboard } } },
    { value: 'page', label: { id: 'common.filter.with_count' as const, params: { label: i18n.t('drafts.filter.pages'), count: counts.page } } }
  ]);
  const originOptions = [
    { value: 'any', label: { id: 'drafts.filter.any_origin' as const } },
    { value: 'local', label: { id: 'drafts.filter.local' as const } },
    { value: 'server_copy', label: { id: 'drafts.filter.server_copies' as const } }
  ];
</script>

<div class="screen drafts">
  <header class="screen-head with-actions">
    <div>
      <h1>{i18n.t('drafts.title')}</h1>
      <p class="lead">{i18n.t('drafts.subtitle')}</p>
    </div>
    <ComingLater {i18n} id="drafts-create-actions">
      <Button {i18n} label={{ id: 'startup.action.new_dashboard' }} disabled />
      <Button {i18n} label={{ id: 'startup.action.new_page' }} disabled />
      <Button {i18n} label={{ id: 'startup.action.import_files' }} disabled />
    </ComingLater>
  </header>

  {#if drafts === null}
    <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={4} />
  {:else if drafts.status === 'unavailable'}
    <EmptyState {i18n} heading={{ id: 'startup.recent.empty' }} reason={{ id: 'common.state.coming_later' }} />
  {:else if all.length === 0}
    <EmptyState {i18n} heading={{ id: 'startup.recent.empty' }} reason={{ id: 'startup.local.help' }} />
  {:else}
    <div class="filters">
      <RadioGroup {i18n} name="drafts-kind" legend={{ id: 'drafts.column.kind' }} options={kindOptions} bind:value={kind} />
      <RadioGroup {i18n} name="drafts-origin" legend={{ id: 'drafts.column.origin' }} options={originOptions} bind:value={origin} />
    </div>
    <DataTable
      {i18n}
      caption={{ id: 'drafts.title' }}
      headers={[
        { id: 'drafts.column.name' },
        { id: 'drafts.column.kind' },
        { id: 'drafts.column.origin' },
        { id: 'drafts.column.source' },
        { id: 'drafts.column.state' }
      ]}
      {rows}
    />
    <p class="visually-hidden" role="status">{resolveText(i18n, { id: 'common.filter.with_count', params: { label: i18n.t('drafts.title'), count: visible.length } })}</p>
  {/if}
</div>
