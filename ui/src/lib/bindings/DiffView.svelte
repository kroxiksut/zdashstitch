<script lang="ts">
  import { tick } from 'svelte';
  import type { I18n } from '../i18n';
  import { Icon, TechnicalDetails, TextField, resolveText } from '../components';
  import { widgetTypeName } from '../shell/format';
  import { CHANGE_ICON, CHANGE_LABEL, CHANGE_TONE, reasonText } from './messages';
  import type { BindingRow, ChangeState, PageDiff, ReplaceHostPreview, WidgetDiff } from './model';
  import { pageLabel, sourceText, targetText, widgetLabel, widgetSearchText } from './view';

  // «Мои изменения: база → черновик» (P1-08B, mockup 11-04) for ReplaceHost: pages, widgets and
  // series with a Tabler marker and a word per change, filters, search and an inspector with the
  // changed fields (code, before → after). The comparison «base → Zabbix now» comes with P2-04.
  let { i18n, preview }: { i18n: I18n; preview: ReplaceHostPreview } = $props();

  type Filter = 'all' | ChangeState;
  const FILTERS: ReadonlyArray<Filter> = ['all', 'changed', 'kept_on_source', 'untouched', 'unresolved'];
  let filter = $state<Filter>('all');
  let search = $state('');
  let selected = $state<string | null>(null);

  const widgets = $derived(preview.diff.pages.flatMap((page) => page.widgets.map((widget) => ({ page, widget }))));
  const counts = $derived(
    Object.fromEntries(FILTERS.map((item) => [item, item === 'all' ? widgets.length : widgets.filter(({ widget }) => widget.state === item).length])) as Record<Filter, number>
  );
  const needle = $derived(search.trim().toLocaleLowerCase(i18n.locale));

  function matches(page: PageDiff, widget: WidgetDiff): boolean {
    if (filter !== 'all' && widget.state !== filter) return false;
    return needle === '' || widgetSearchText(i18n, preview, page, widget).includes(needle);
  }

  function idOf(page: PageDiff, widget: WidgetDiff): string {
    return `${page.page_index}:${widget.widget_index}`;
  }

  function rowsOf(page: PageDiff, widget: WidgetDiff): BindingRow[] {
    return preview.rows.filter((row) => row.key.page_index === page.page_index && row.key.widget_index === widget.widget_index);
  }

  function filterLabel(item: Filter): string {
    switch (item) {
      case 'all':
        return i18n.t('common.filter.all');
      case 'changed':
        return i18n.t('publication.diff.filter.changed');
      case 'kept_on_source':
        return i18n.t('publication.diff.filter.kept');
      case 'untouched':
        return i18n.t('publication.diff.filter.untouched');
      case 'unresolved':
        return i18n.t('publication.diff.filter.unresolved');
    }
  }

  function changeText(widget: WidgetDiff): string {
    switch (widget.state) {
      case 'changed':
        return i18n.t('publication.diff.change.field', { count: widget.fields.changed });
      case 'kept_on_source':
        return i18n.t('publication.diff.change.kept');
      case 'unresolved':
        return i18n.t('publication.diff.change.unresolved');
      case 'untouched':
        return i18n.t('publication.diff.no_changes');
    }
  }

  function unique(values: string[]): string {
    return [...new Set(values)].join(', ');
  }

  function before(rows: BindingRow[]): string {
    return rows.length === 0 ? '—' : unique(rows.map((row) => sourceText(preview, row)));
  }

  function after(rows: BindingRow[]): string {
    if (rows.length === 0) return '—';
    return unique(rows.map((row) => (row.state === 'changed' ? targetText(preview, row) : sourceText(preview, row))));
  }

  const current = $derived.by(() => {
    for (const page of preview.diff.pages) {
      for (const widget of page.widgets) if (idOf(page, widget) === selected) return { page, widget, rows: rowsOf(page, widget) };
    }
    return null;
  });

  async function open(page: PageDiff, widget: WidgetDiff, focus: boolean): Promise<void> {
    selected = idOf(page, widget);
    if (focus) {
      await tick();
      document.getElementById('diff-inspector-title')?.focus();
    }
  }
</script>

<div class="diff-layout">
  <div class="diff-main">
    <div class="filters" role="group" aria-label={i18n.t('publication.diff.filter.label')}>
      {#each FILTERS as item (item)}
        <button type="button" class="chip" aria-pressed={filter === item} onclick={() => (filter = item)}>
          {#if item !== 'all'}<Icon name={CHANGE_ICON[item]} size={16} />{/if}
          {i18n.t('common.filter.with_count', { label: filterLabel(item), count: counts[item] })}
        </button>
      {/each}
    </div>
    <TextField {i18n} id="diff-search" label={{ id: 'publication.diff.search' }} bind:value={search} />

    {#each preview.diff.pages as page (page.page_index)}
      {@const visible = page.widgets.filter((widget) => matches(page, widget))}
      {#if visible.length > 0}
        <details class="deps-page" open>
          <summary>
            <span class="deps-page-title">{pageLabel(i18n, preview, page.page_index)}</span>
            <span class="muted">{i18n.t('publication.diff.page_summary', { changed: page.widget_counts.changed, total: page.widget_counts.total })}</span>
          </summary>
          <div class="table-scroll">
            <table>
              <caption class="visually-hidden">{i18n.t('publication.diff.caption', { page: pageLabel(i18n, preview, page.page_index) })}</caption>
              <thead>
                <tr>
                  <th scope="col">{i18n.t('publication.diff.column.object')}</th>
                  <th scope="col">{i18n.t('publication.diff.column.change')}</th>
                  <th scope="col">{i18n.t('publication.diff.column.before')}</th>
                  <th scope="col">{i18n.t('publication.diff.column.after')}</th>
                </tr>
              </thead>
              <tbody>
                {#each visible as widget (widget.widget_index)}
                  {@const rows = rowsOf(page, widget)}
                  <tr class:selected={selected === idOf(page, widget)}>
                    <td>
                      <button
                        type="button"
                        class="row-select"
                        aria-pressed={selected === idOf(page, widget)}
                        onclick={() => void open(page, widget, false)}
                        onkeydown={(event) => {
                          if (event.key === 'Enter') {
                            event.preventDefault();
                            void open(page, widget, true);
                          }
                        }}
                      >{widgetLabel(i18n, widget)} · {widgetTypeName(i18n, widget.widget_type)}</button>
                      {#if !widget.in_scope}<span class="muted">{i18n.t('publication.diff.out_of_scope')}</span>{/if}
                    </td>
                    <td><span class="marker tone-{CHANGE_TONE[widget.state]}"><Icon name={CHANGE_ICON[widget.state]} size={18} />{changeText(widget)}</span></td>
                    <td class="technical">{before(rows)}</td>
                    <td class="technical">{after(rows)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </details>
      {/if}
    {/each}
    {#if widgets.every(({ page, widget }) => !matches(page, widget))}<p>{i18n.t('publication.diff.empty_filter')}</p>{/if}
    <p class="note"><Icon name="info-circle" />{i18n.t('publication.diff.matching_by_id')}</p>
  </div>

  <aside class="card inspector" aria-labelledby="diff-inspector-title">
    <h3 id="diff-inspector-title" class="eyebrow" tabindex="-1">{i18n.t('publication.diff.inspector.title')}</h3>
    {#if current === null}
      <p class="muted">{i18n.t('publication.diff.inspector.none')}</p>
    {:else}
      <p class="inspector-object">
        <strong>{widgetLabel(i18n, current.widget)}</strong>
        <span class="muted">{widgetTypeName(i18n, current.widget.widget_type)} · {pageLabel(i18n, preview, current.page.page_index)}</span>
      </p>
      <p class="marker"><Icon name={CHANGE_ICON[current.widget.state]} size={18} />{resolveText(i18n, CHANGE_LABEL[current.widget.state])}</p>
      {#if current.rows.length > 0}
        <h4 class="eyebrow">{i18n.t('publication.diff.inspector.changed_fields')}</h4>
        <ul class="field-changes">
          {#each current.rows as row (row.key.field)}
            <li>
              <span class="technical">{row.key.field}</span>
              <span class="technical">{sourceText(preview, row)} → {row.state === 'changed' ? targetText(preview, row) : sourceText(preview, row)}</span>
              <span class="marker"><Icon name={CHANGE_ICON[row.state]} size={16} />{resolveText(i18n, CHANGE_LABEL[row.state])}</span>
              {#if row.kept_reason}<span class="muted">{resolveText(i18n, reasonText(row.kept_reason))}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if current.widget.series.length > 0}
        <h4 class="eyebrow">{i18n.t('publication.diff.inspector.sources')}</h4>
        <ul class="field-changes">
          {#each current.widget.series as series (series.dataset_index)}
            <li>
              <span>{i18n.t('publication.diff.inspector.series', { index: series.dataset_index + 1 })}</span>
              <span class="marker"><Icon name={CHANGE_ICON[series.state]} size={16} />{resolveText(i18n, CHANGE_LABEL[series.state])}</span>
              {#if series.host_patterns.length > 0}<span class="technical">{series.host_patterns.join(', ')}</span>{/if}
              {#if series.item_patterns.length > 0}<span class="technical">{series.item_patterns.join(', ')}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      <h4 class="eyebrow">{i18n.t('publication.diff.inspector.unchanged')}</h4>
      <p class="muted">{i18n.t('publication.diff.inspector.unchanged_body')}</p>
      <TechnicalDetails
        {i18n}
        summary={{ id: 'publication.tech.title' }}
        values={[
          { label: { id: 'publication.tech.widgetid' }, value: { value: current.widget.widgetid ?? '—', kind: 'technical' } },
          { label: { id: 'publication.tech.pageid' }, value: { value: current.page.dashboard_pageid ?? '—', kind: 'technical' } },
          { label: { id: 'publication.tech.widget_type' }, value: { value: current.widget.widget_type, kind: 'technical' } },
          { label: { id: 'publication.tech.fields' }, value: { value: current.widget.row_fields.join(', ') || '—', kind: 'technical' } }
        ]}
      />
    {/if}
  </aside>
</div>
