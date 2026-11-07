<script lang="ts">
  import { onMount } from 'svelte';
  import type { I18n } from '../i18n';
  import type { MessageId } from '../i18n/generated';
  import { Button } from '../components';
  import type { CloneComparisonData } from './clone-comparison';

  let { i18n, load, onjournal }: {
    i18n: I18n;
    load: () => Promise<CloneComparisonData>;
    onjournal?: (operationId: string) => void;
  } = $props();

  const states = {
    pending: 'clone.compare.pending',
    unavailable: 'clone.compare.unavailable',
    verified_read_back: 'clone.compare.verified',
    changed_since_read_back: 'clone.compare.drift',
    unverified: 'clone.compare.unverified'
  } satisfies Record<CloneComparisonData['observation'], MessageId>;
  const categories = {
    all: 'clone.compare.all',
    changed: 'clone.compare.changed',
    added: 'clone.compare.added',
    untouched: 'clone.compare.untouched',
    unresolved: 'clone.compare.unresolved'
  } satisfies Record<string, MessageId>;
  const kinds = {
    dashboard: 'clone.compare.dashboard',
    page: 'clone.compare.page',
    widget: 'clone.compare.widget',
    field: 'clone.compare.field'
  } satisfies Record<string, MessageId>;
  const sides = ['clone.compare.source', 'clone.compare.expected', 'clone.compare.observed'] as const;

  let data = $state<CloneComparisonData | null>(null);
  let filter = $state<keyof typeof categories>('all');
  let openPages = $state(new Set([1]));
  let loading = $state(false);
  let failed = $state(false);
  const rows = $derived((data?.rows ?? []).filter((row) =>
    filter === 'all' || (filter === 'unresolved' ? row.unresolved : row.change === filter)));
  const groups = $derived.by(() => {
    const pages = new Map<number, typeof rows>();
    for (const row of rows) {
      const index = row.page ?? 0;
      const page = pages.get(index) ?? [];
      page.push(row);
      pages.set(index, page);
    }
    return [...pages.entries()].sort(([left], [right]) => left - right);
  });

  async function refresh(): Promise<void> {
    if (loading) return;
    loading = true;
    failed = false;
    try { data = await load(); }
    catch { data = null; failed = true; }
    finally { loading = false; }
  }

  function togglePage(page: number, open: boolean): void {
    const next = new Set(openPages);
    if (open) next.add(page);
    else next.delete(page);
    openPages = next;
  }

  onMount(() => { void refresh(); });
</script>

<section class="comparison" aria-label={i18n.t('clone.compare.title')} aria-busy={loading}>
  <div class="controls">
    <Button {i18n} label={{ id: 'clone.compare.refresh' }} onclick={refresh} disabled={loading} />
    <a href="#clone-edit-trigger">{i18n.t('clone.compare.mapping')}</a>
    {#if data?.operation_id && onjournal}
      {@const id = data.operation_id}
      <Button {i18n} label={{ id: 'publication.action.open_journal' }} onclick={() => onjournal(id)} />
    {/if}
  </div>
  <div role="status">
    {#if loading}<p>{i18n.t('clone.working')}</p>
    {:else if data}
      <p>{i18n.t(states[data.observation])}</p>
      {#if data.verdict && data.verdict !== 'matches'}
        <p>{i18n.t('clone.compare.verdict')} <code>{data.verdict}</code></p>
      {/if}
    {/if}
  </div>
  {#if failed}<p role="alert">{i18n.t('clone.compare.failed')}</p>{/if}
  {#if data && !loading}
    <h4>{i18n.t('clone.compare.access')}</h4>
    <dl class="sides">
      {#each data.access as entry, index}
        <div>
          <dt>{i18n.t(sides[index])}</dt>
          <dd>
            {#if entry}
              <ul>
                {#if entry.private !== null}<li>{i18n.t(entry.private ? 'clone.compare.private' : 'clone.compare.public')}</li>{/if}
                {#if entry.shared !== null}<li>{i18n.t(entry.shared ? 'clone.compare.shared' : 'clone.compare.unshared')}</li>{/if}
                {#if entry.owner_matches !== null}<li>{i18n.t(entry.owner_matches ? 'clone.compare.owner' : 'clone.compare.owner_differs')}</li>{/if}
              </ul>
            {:else}{i18n.t('clone.compare.no_observation')}{/if}
          </dd>
        </div>
      {/each}
    </dl>
    <label class="filter">
      {i18n.t('clone.compare.filter')}
      <select bind:value={filter}>
        {#each Object.entries(categories) as [value, label]}
          <option {value}>{i18n.t(label)}</option>
        {/each}
      </select>
    </label>
    <p role="status">{i18n.t('clone.compare.count', { count: rows.length })}</p>
    {#each groups as [page, group] (page)}
      {#if page === 0}
        {@render rowsOf(group)}
      {:else}
        <details open={openPages.has(page)} ontoggle={(event) => togglePage(page, event.currentTarget.open)}>
          <summary>{i18n.t('clone.compare.page_group', { page, count: group.length })}</summary>
          {#if openPages.has(page)}{@render rowsOf(group)}{/if}
        </details>
      {/if}
    {/each}
  {/if}
</section>

{#snippet rowsOf(group: CloneComparisonData['rows'])}
  <ul class="rows">
    {#each group as row}
      <li>
        <h5>
          {i18n.t(kinds[row.kind])}
          {#if row.at}<code>({row.at[0]}, {row.at[1]})</code>{/if}
          {#if row.field}<code>{row.field}</code>{/if}
        </h5>
        <p>
          {i18n.t(categories[row.change])}
          {#if row.kept} · {i18n.t('clone.compare.kept')}{/if}
          {#if row.observed_differs} · {i18n.t('clone.compare.different')}{:else if row.unresolved} · {i18n.t('clone.compare.unresolved')}{/if}
        </p>
        <dl class="sides">
          {#each [row.source, row.expected, row.observed] as cell, index}
            <div>
              <dt>{i18n.t(sides[index])}</dt>
              <dd>
                {#if cell}
                  {cell.label || (row.kind === 'field' ? i18n.t('clone.compare.value_hidden') : i18n.t('clone.compare.unnamed'))}
                  {#if cell.widget_type}<code>{cell.widget_type}</code>{/if}
                  {#if cell.geometry}<p>{i18n.t('clone.compare.geometry', {
                    x: cell.geometry[0], y: cell.geometry[1], width: cell.geometry[2], height: cell.geometry[3]
                  })}</p>{/if}
                {:else if index === 2 && (data?.observation === 'pending' || data?.observation === 'unavailable')}
                  {i18n.t('clone.compare.no_observation')}
                {:else}{i18n.t('clone.compare.absent')}{/if}
              </dd>
            </div>
          {/each}
        </dl>
      </li>
    {/each}
  </ul>
{/snippet}

<style>
  .comparison { display: grid; gap: var(--space-2); min-inline-size: 0; overflow-wrap: anywhere; }
  .controls { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
  .controls a { display: inline-flex; min-block-size: 32px; align-items: center; }
  .filter { display: grid; gap: var(--space-1); justify-self: start; }
  select, summary { min-block-size: 32px; max-inline-size: 100%; }
  .rows { list-style: none; padding: 0; }
  .rows > li { border-block-start: 1px solid var(--border-subtle); padding-block: var(--space-2); }
  h5, dd, p { margin: 0; }
  .sides { display: grid; gap: var(--space-2); grid-template-columns: repeat(3, minmax(0, 1fr)); }
  dt { font-weight: 600; }
  @media (max-width: 42rem) { .sides { grid-template-columns: minmax(0, 1fr); } }
</style>
