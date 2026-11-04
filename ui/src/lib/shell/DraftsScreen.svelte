<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, EmptyState, Icon, RadioGroup, Skeleton, nextRovingIndex, resolveText } from '../components';
  import { workcopyCodeText } from '../workcopy/messages';
  import ComingLater from './ComingLater.svelte';
  import DraftDetails from './DraftDetails.svelte';
  import {
    duplicateNames,
    filterDrafts,
    kindCounts,
    searchDrafts,
    selectedDraft,
    sortByUpdated,
    type DraftListResult,
    type DraftRenameResult,
    type KindFilter,
    type OriginFilter
  } from './drafts';
  import { listSourceText, sourceConnectionSuffix, stateText } from './format';
  import { KIND_LABEL, ORIGIN_LABEL } from './labels';
  import { draftPath } from './router';

  // «Drafts» (P1-03F, mockup 02-01): filters, search by name, the list and the details column.
  // `drafts` is null while the list is loading. The name is a link that opens the draft (P2-08A);
  // a click elsewhere in the row selects it and shows the details. Selecting is separate from
  // focus (P1-03H): arrows move focus between the name links, Enter opens, Space selects.
  let { i18n, drafts, onrename }: {
    i18n: I18n;
    drafts: DraftListResult | null;
    onrename?: (localId: string, name: string) => Promise<DraftRenameResult>;
  } = $props();

  // RadioGroup binds plain strings; only known filter values are offered.
  let kind = $state<string>('all');
  let origin = $state<string>('any');
  let query = $state('');
  let selectedId = $state<string | null>(null);
  let focusedId = $state<string | null>(null);

  const all = $derived(drafts?.status === 'ready' ? sortByUpdated(drafts.drafts) : []);
  const damaged = $derived(drafts?.status === 'ready' ? (drafts.damaged ?? []) : []);
  const counts = $derived(kindCounts(all));
  // P2-08A: drafts with the same name show their source connection next to the name.
  const duplicates = $derived(duplicateNames(all, i18n.locale));
  const visible = $derived(searchDrafts(filterDrafts(all, kind as KindFilter, origin as OriginFilter), query, i18n.locale));
  const selected = $derived(selectedDraft(visible, selectedId));
  // One tab stop for the rows: the last focused row while visible, otherwise the selected one.
  const tabStop = $derived(visible.some((draft) => draft.local_id === focusedId) ? focusedId : (selected?.local_id ?? null));
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

  function rowId(localId: string): string {
    return `drafts-row-${localId}`;
  }

  function onRowKeydown(event: KeyboardEvent, index: number): void {
    if (event.key === ' ' || event.key === 'Spacebar') {
      event.preventDefault();
      selectedId = visible[index].local_id;
      return;
    }
    const next = nextRovingIndex(index, visible.length, event.key, 'vertical');
    if (next === index || next < 0) return;
    event.preventDefault();
    const target = visible[next];
    focusedId = target.local_id;
    document.getElementById(rowId(target.local_id))?.focus();
  }
</script>

<div class="screen drafts">
  <header class="screen-head with-actions">
    <div>
      <h1>{i18n.t('drafts.title')}</h1>
      <p class="lead">{i18n.t('drafts.subtitle')}</p>
    </div>
    <ComingLater {i18n} id="drafts-create-actions">
      <Button {i18n} variant="primary" label={{ id: 'drafts.action.create' }} disabled>
        {#snippet leadingIcon()}<Icon name="plus" size={18} />{/snippet}
      </Button>
      <Button {i18n} label={{ id: 'startup.action.import_files' }} disabled />
    </ComingLater>
  </header>

  {#if drafts === null}
    <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={4} />
  {:else if drafts.status === 'unavailable'}
    <EmptyState {i18n} heading={{ id: 'drafts.load_failed' }} reason={{ id: 'startup.error.unavailable' }} />
  {:else if drafts.status === 'failed'}
    <EmptyState {i18n} heading={{ id: 'drafts.load_failed' }} reason={workcopyCodeText(drafts.failure.code, drafts.failure.params)} />
  {:else if all.length === 0}
    {#if damaged.length > 0}
      <p class="details-note"><Icon name="alert-triangle" size={18} /><span>{i18n.t('drafts.damaged', { count: damaged.length })}</span></p>
    {/if}
    <EmptyState {i18n} heading={{ id: 'startup.recent.empty' }} reason={{ id: 'startup.local.help' }} />
  {:else}
    <div class="filters">
      <RadioGroup {i18n} name="drafts-kind" legend={{ id: 'drafts.column.kind' }} options={kindOptions} bind:value={kind} />
      <RadioGroup {i18n} name="drafts-origin" legend={{ id: 'drafts.column.origin' }} options={originOptions} bind:value={origin} />
      <div class="search-field">
        <Icon name="search" size={18} />
        <input
          id="drafts-search"
          type="search"
          autocomplete="off"
          aria-label={i18n.t('drafts.search.placeholder')}
          placeholder={i18n.t('drafts.search.placeholder')}
          bind:value={query}
        />
      </div>
    </div>

    {#if damaged.length > 0}
      <p class="details-note"><Icon name="alert-triangle" size={18} /><span>{i18n.t('drafts.damaged', { count: damaged.length })}</span></p>
    {/if}
    {#if visible.length === 0 || !selected}
      <p class="muted">{i18n.t('drafts.search.no_results')}</p>
    {:else}
      <div class="drafts-layout">
        <div class="drafts-list">
          <div class="table-scroll">
            <table>
              <caption class="visually-hidden">{i18n.t('drafts.title')}</caption>
              <thead>
                <tr>
                  <th scope="col">{i18n.t('drafts.column.name')}</th>
                  <th scope="col">{i18n.t('drafts.column.kind')}</th>
                  <th scope="col">{i18n.t('drafts.column.origin')}</th>
                  <th scope="col">{i18n.t('drafts.column.source')}</th>
                  <th scope="col">{i18n.t('drafts.column.state')}</th>
                </tr>
              </thead>
              <tbody>
                {#each visible as draft, index (draft.local_id)}
                  {@const isSelected = draft.local_id === selected?.local_id}
                  {@const suffix = sourceConnectionSuffix(i18n, draft, duplicates)}
                  <!-- The mouse shortcut of the row; the keyboard selects with Space on the name link. -->
                  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
                  <tr
                    class="selectable-row"
                    class:selected={isSelected}
                    onclick={() => {
                      selectedId = draft.local_id;
                    }}
                  >
                    <td>
                      <span class="row-name">
                        <a
                          id={rowId(draft.local_id)}
                          class="row-open"
                          href={draftPath(draft.local_id)}
                          aria-current={isSelected ? 'true' : undefined}
                          aria-describedby="drafts-list-hint"
                          tabindex={draft.local_id === tabStop ? 0 : -1}
                          onfocus={() => {
                            focusedId = draft.local_id;
                          }}
                          onkeydown={(event) => onRowKeydown(event, index)}
                        >{draft.name}</a>
                        {#if suffix}<span class="muted">{suffix}</span>{/if}
                      </span>
                    </td>
                    <td>{resolveText(i18n, KIND_LABEL[draft.kind])}</td>
                    <td>{resolveText(i18n, ORIGIN_LABEL[draft.origin])}</td>
                    <td>{listSourceText(i18n, draft)}</td>
                    <td>{stateText(i18n, draft)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          <p id="drafts-list-hint" class="hint">{i18n.t('drafts.list.open_hint')}</p>
          <p class="hint">{i18n.t('drafts.sort_hint')}</p>
        </div>
        <aside id="drafts-details" class="card drafts-details" aria-labelledby="drafts-details-title">
          <DraftDetails {i18n} draft={selected} {onrename} />
        </aside>
      </div>
    {/if}
    <p class="visually-hidden" role="status">{resolveText(i18n, { id: 'common.filter.with_count', params: { label: i18n.t('drafts.title'), count: visible.length } })}</p>
  {/if}
</div>
