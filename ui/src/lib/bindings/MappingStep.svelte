<script lang="ts">
  import { tick } from 'svelte';
  import type { I18n } from '../i18n';
  import { Icon, resolveText } from '../components';
  import { widgetTypeName } from '../shell/format';
  import { CHANGE_ICON, CHANGE_LABEL, MATCH_ICON, MATCH_SHORT, MATCH_TONE } from './messages';
  import { MATCH_STATUSES, keyId, matchCounts, type BindingRow, type MatchStatus, type ReplaceHostPreview } from './model';
  import RowInspector from './RowInspector.svelte';
  import type { ReplaceHostStore } from './store.svelte';
  import { pageLabel, sourceText, targetText, widgetLabel, widgetOf } from './view';

  // «Соответствия» (P1-05D, mockup 09-03): source → target per reference with the match as icon and
  // word, filters by status, keyboard navigation (arrows between rows, Enter opens the row) and
  // the inspector with candidates, evidence and decisions.
  let { i18n, store, preview }: {
    i18n: I18n;
    store: ReplaceHostStore;
    preview: ReplaceHostPreview;
  } = $props();

  type Filter = 'all' | MatchStatus | 'kept_on_source';
  let filter = $state<Filter>('all');
  let table = $state<HTMLTableElement>();

  const counts = $derived(matchCounts(preview.rows));
  const source = $derived(preview.operation.source_host.name);
  const target = $derived(preview.operation.target_host?.name ?? null);
  const filters = $derived(
    (['all', ...MATCH_STATUSES, 'kept_on_source'] as Filter[]).filter((item) => item === 'all' || counts[item] > 0)
  );
  const visible = $derived(
    preview.rows.filter((row) => {
      if (filter === 'all') return true;
      if (filter === 'kept_on_source') return row.state === 'kept_on_source';
      return row.state !== 'kept_on_source' && (row.match ?? 'denied') === filter;
    })
  );
  const selected = $derived(preview.rows.find((row) => keyId(row.key) === store.selectedRow) ?? null);

  function filterLabel(item: Filter) {
    if (item === 'all') return i18n.t('common.filter.all');
    if (item === 'kept_on_source') return i18n.t('publication.diff.filter.kept');
    return resolveText(i18n, MATCH_SHORT[item]);
  }

  async function select(row: BindingRow, focusInspector = false): Promise<void> {
    store.selectedRow = keyId(row.key);
    if (focusInspector) {
      await tick();
      document.getElementById('row-inspector-title')?.focus();
    }
  }

  function keydown(event: KeyboardEvent, index: number): void {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp' && event.key !== 'Home' && event.key !== 'End') return;
    event.preventDefault();
    const last = visible.length - 1;
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? last : Math.min(last, Math.max(0, index + (event.key === 'ArrowDown' ? 1 : -1)));
    const buttons = table?.querySelectorAll<HTMLButtonElement>('button.row-select');
    buttons?.[next]?.focus();
    const row = visible[next];
    if (row) store.selectedRow = keyId(row.key);
  }
</script>

<div class="mapping-layout">
  <section class="card mapping" aria-labelledby="mapping-title">
    <h3 id="mapping-title">{i18n.t('bindings.mapping.title', { source, target: target ?? '—' })}</h3>
    {#if target === null}<p class="note warning-note"><Icon name="circle-x" />{i18n.t('bindings.mapping.target_missing')}</p>{/if}

    <div class="filters" role="group" aria-label={i18n.t('bindings.mapping.filter')}>
      {#each filters as item (item)}
        <button type="button" class="chip" aria-pressed={filter === item} onclick={() => (filter = item)}>
          {i18n.t('common.filter.with_count', { label: filterLabel(item), count: item === 'all' ? counts.all : counts[item] })}
        </button>
      {/each}
    </div>
    <p class="muted">{i18n.t('bindings.mapping.keyboard_hint')}</p>

    {#if preview.rows.length === 0}
      <p>{i18n.t('bindings.mapping.no_rows')}</p>
    {:else if visible.length === 0}
      <p>{i18n.t('bindings.mapping.empty_filter')}</p>
    {:else}
      <div class="table-scroll">
        <table bind:this={table} class="mapping-table">
          <caption class="visually-hidden">{i18n.t('bindings.mapping.caption')}</caption>
          <thead>
            <tr>
              <th scope="col">{i18n.t('bindings.mapping.column.source', { host: source })}</th>
              <th scope="col">{i18n.t('bindings.mapping.column.target', { host: target ?? '—' })}</th>
              <th scope="col">{i18n.t('bindings.mapping.column.match')}</th>
              <th scope="col">{i18n.t('bindings.mapping.column.widget')}</th>
            </tr>
          </thead>
          <tbody>
            {#each visible as row, index (keyId(row.key))}
              {@const widget = widgetOf(preview, row.key.page_index, row.key.widget_index)}
              <tr class:selected={store.selectedRow === keyId(row.key)}>
                <td>
                  <button
                    type="button"
                    class="row-select technical"
                    aria-pressed={store.selectedRow === keyId(row.key)}
                    onclick={() => void select(row)}
                    onkeydown={(event) => {
                      if (event.key === 'Enter') {
                        event.preventDefault();
                        void select(row, true);
                      } else keydown(event, index);
                    }}
                  >{sourceText(preview, row)}</button>
                </td>
                <td>
                  {#if row.state === 'changed'}
                    <span class="technical">{targetText(preview, row)}</span>
                  {:else if row.match === 'ambiguous' || row.match === 'unresolved_lld'}
                    {#if row.candidates.length > 0}{i18n.t('bindings.mapping.candidates_count', { count: row.candidates.length })}{:else}{i18n.t('bindings.mapping.no_target')}{/if}
                  {:else}
                    {i18n.t('bindings.mapping.no_target')}
                  {/if}
                </td>
                <td>
                  {#if row.state === 'kept_on_source'}
                    <span class="marker tone-warning"><Icon name={CHANGE_ICON.kept_on_source} size={18} />{resolveText(i18n, CHANGE_LABEL.kept_on_source)}</span>
                  {:else if row.match}
                    <span class="marker tone-{MATCH_TONE[row.match]}"><Icon name={MATCH_ICON[row.match]} size={18} />{resolveText(i18n, MATCH_SHORT[row.match])}</span>
                  {:else}
                    <span class="marker tone-danger"><Icon name="circle-dashed" size={18} />{i18n.t('bindings.match_short.unverified')}</span>
                  {/if}
                  {#if row.decision_state === 'applied'}<span class="muted">{i18n.t('bindings.kept.by_decision')}</span>{/if}
                  {#if row.check !== 'checked'}<span class="marker tone-danger"><Icon name="circle-dashed" size={18} />{i18n.t(row.check === 'stale' ? 'bindings.state.stale' : 'bindings.state.invalid')}</span>{/if}
                </td>
                <td>
                  <span>{widgetLabel(i18n, widget, row.widget_type)}</span>
                  <span class="muted">{pageLabel(i18n, preview, row.key.page_index)} · {widgetTypeName(i18n, row.widget_type)}</span>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
    <p class="note"><Icon name="info-circle" />{i18n.t('bindings.mapping.similar_name_not_proof')}</p>
  </section>

  <RowInspector {i18n} {store} {preview} row={selected} />
</div>
