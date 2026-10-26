<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Checkbox, SelectField, TreeList, type DisplayText, type LocalizedText, type UserText } from '../components';
  import ComingLater from './ComingLater.svelte';
  import { widgetCount, type DraftDetail } from './drafts';
  import { displayName, pageName, periodText, widgetGeometry, widgetTypeName } from './format';
  import {
    DEFAULT_SCREEN_WIDTH,
    SCREEN_WIDTHS,
    previewAspectRatio,
    rowsUsed,
    widgetBox
  } from './preview';

  // «Structure» for viewing (P1-01B §2a, mockup 08-01): tree as in Zabbix and a page preview
  // in grid proportions. Structure operations and the editor arrive later (P4); they stay visible
  // but unavailable (P1-01H §6). Synthetic data is off and unavailable until P1-04I is built.
  let { i18n, draft }: { i18n: I18n; draft: DraftDetail } = $props();

  type NodeRef =
    | { readonly kind: 'dashboard' }
    | { readonly kind: 'page'; readonly page: number }
    | { readonly kind: 'widget'; readonly page: number; readonly widget: number };

  interface TreeItem {
    id: string;
    label: UserText;
    level: number;
    expanded?: boolean;
    detail?: DisplayText;
    note?: LocalizedText;
  }

  const tree = $derived.by(() => {
    const items: TreeItem[] = [];
    const refs = new Map<string, NodeRef>();
    const base = draft.kind === 'dashboard' ? 1 : 0;
    if (draft.kind === 'dashboard') {
      items.push({
        id: 'structure-dashboard',
        label: { value: draft.name, kind: 'user' },
        level: 1,
        expanded: true,
        detail: { id: 'dashboards.summary.pages_widgets', params: { pages: draft.pages.length, widgets: widgetCount(draft.pages) } }
      });
      refs.set('structure-dashboard', { kind: 'dashboard' });
    }
    draft.pages.forEach((page, pageIndex) => {
      const pageId = `structure-page-${pageIndex}`;
      items.push({
        id: pageId,
        label: { value: i18n.t('object.page.numbered', { number: pageIndex + 1, name: pageName(i18n, page.name) }), kind: 'user' },
        level: base + 1,
        expanded: true,
        detail: {
          id: 'draft.structure.page_summary',
          params: {
            widgets: i18n.t('object.page.widget_count', { count: page.widgets.length }),
            period: periodText(i18n, page.display_period)
          }
        }
      });
      refs.set(pageId, { kind: 'page', page: pageIndex });
      page.widgets.forEach((widget, widgetIndex) => {
        const widgetId = `structure-widget-${pageIndex}-${widgetIndex}`;
        items.push({
          id: widgetId,
          label: { value: displayName(widget.name, widgetTypeName(i18n, widget.type)), kind: 'user' },
          level: base + 2,
          detail: { value: widgetGeometry(i18n, widget), kind: 'user' },
          note: widget.editable ? undefined : { id: 'common.state.edit_unsupported' }
        });
        refs.set(widgetId, { kind: 'widget', page: pageIndex, widget: widgetIndex });
      });
    });
    return { items, refs };
  });

  let selectedNode = $state<string | undefined>(undefined);
  let screenWidth = $state<string>(String(DEFAULT_SCREEN_WIDTH));
  let synthetic = $state(false);

  const selectedRef = $derived(selectedNode === undefined ? undefined : tree.refs.get(selectedNode));
  const pageIndex = $derived(selectedRef && selectedRef.kind !== 'dashboard' ? selectedRef.page : 0);
  const page = $derived(draft.pages[pageIndex]);
  const selectedWidget = $derived(
    selectedRef?.kind === 'widget' ? draft.pages[selectedRef.page]?.widgets[selectedRef.widget] : undefined
  );
  const rows = $derived(page ? rowsUsed(page.widgets) : 0);
  const aspectRatio = $derived(previewAspectRatio(Number(screenWidth), rows));
  const screenOptions = SCREEN_WIDTHS.map((width) => ({
    value: String(width),
    label: { id: 'draft.preview.screen_width' as const, params: { width: String(width) } }
  }));
</script>

<section class="workspace-section" aria-labelledby="structure-title">
  <h2 id="structure-title">{i18n.t('draft.nav.structure')}</h2>
  <p class="muted">{i18n.t('draft.structure.grid_help')}</p>

  <div class="structure-layout">
    <div class="structure-tree">
      <TreeList {i18n} label={{ id: 'draft.nav.structure' }} items={tree.items} bind:selected={selectedNode} />
      <ComingLater {i18n} id="structure-page-actions">
        <Button {i18n} label={{ id: 'draft.structure.action.add_page' }} disabled />
        <Button {i18n} label={{ id: 'common.action.copy' }} disabled />
        <Button {i18n} label={{ id: 'draft.structure.action.paste_page' }} disabled />
      </ComingLater>
      {#if selectedWidget}
        <section class="selected-widget" aria-labelledby="structure-selected-title">
          <h3 id="structure-selected-title">{displayName(selectedWidget.name, widgetTypeName(i18n, selectedWidget.type))}</h3>
          <p class="muted">{widgetGeometry(i18n, selectedWidget)}</p>
          <ComingLater {i18n} id="structure-widget-edit" hint={{ id: 'editor.unavailable_yet' }}>
            <Button {i18n} label={{ id: 'draft.structure.action.edit_widget' }} disabled />
          </ComingLater>
          <ComingLater {i18n} id="structure-widget-move">
            <Button {i18n} label={{ id: 'draft.structure.action.move_widget' }} disabled />
          </ComingLater>
        </section>
      {/if}
    </div>

    <section class="preview-panel" aria-labelledby="preview-title">
      <h3 id="preview-title">{i18n.t('draft.preview.title')}</h3>
      <div class="preview-controls">
        <SelectField
          {i18n}
          id="preview-screen-width"
          label={{ id: 'draft.preview.screen_width', params: { width: screenWidth } }}
          options={screenOptions}
          bind:value={screenWidth}
        />
        <Checkbox
          {i18n}
          id="preview-synthetic"
          label={{ id: 'draft.preview.synthetic.toggle' }}
          description={{ id: 'common.state.coming_later' }}
          bind:checked={synthetic}
          disabled
        />
      </div>
      {#if page}
        <div class="preview-canvas" style:aspect-ratio={aspectRatio}>
          {#each page.widgets as widget, widgetIndex (widget.id)}
            {@const box = widgetBox(widget, rows)}
            {@const nodeId = `structure-widget-${pageIndex}-${widgetIndex}`}
            <button
              type="button"
              class="preview-widget"
              aria-pressed={selectedNode === nodeId}
              style:left={`${box.left}%`}
              style:width={`${box.width}%`}
              style:top={`${box.top}%`}
              style:height={`${box.height}%`}
              onclick={() => {
                selectedNode = nodeId;
              }}
            >
              <span class="widget-name">{displayName(widget.name, widgetTypeName(i18n, widget.type))}</span>
              <span class="technical">{widget.type}</span>
            </button>
          {/each}
        </div>
        <p class="muted">{i18n.t('draft.preview.rows_shown', { shown: rows })}</p>
      {/if}
      {#if draft.compatibility}
        <p class="muted">{i18n.t('draft.compat.label', { min_version: draft.compatibility.min_version })}</p>
      {/if}
    </section>
  </div>
</section>
