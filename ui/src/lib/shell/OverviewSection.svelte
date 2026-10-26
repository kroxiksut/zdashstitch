<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, StatusBadge } from '../components';
  import ComingLater from './ComingLater.svelte';
  import { widgetCount, type DraftDetail } from './drafts';
  import { displayName, pageName, widgetGeometry, widgetTypeName } from './format';
  import { ORIGIN_LABEL, widgetTypeLabel } from './labels';

  // «Overview» (P1-01B, mockup 07-01): origin, composition in grid cells, available next steps.
  let { i18n, draft }: { i18n: I18n; draft: DraftDetail } = $props();
</script>

<section class="workspace-section" aria-labelledby="overview-title">
  <h2 id="overview-title">{i18n.t('draft.nav.overview')}</h2>
  <p class="badges">
    <StatusBadge {i18n} label={ORIGIN_LABEL[draft.origin]} tone="info" />
    <span>{i18n.t('dashboards.summary.pages_widgets', { pages: draft.pages.length, widgets: widgetCount(draft.pages) })}</span>
  </p>
  {#if draft.origin === 'server_copy'}
    <ComingLater {i18n} id="overview-source-actions" hint={{ id: 'connection.state.not_connected' }}>
      <Button {i18n} label={{ id: 'draft.action.check_source' }} disabled />
    </ComingLater>
  {/if}
  <p class="muted">{i18n.t('draft.structure.grid_help')}</p>
  {#each draft.pages as page, pageIndex (page.id)}
    <section class="composition" aria-labelledby={`overview-page-${pageIndex}`}>
      <h3 id={`overview-page-${pageIndex}`}>{pageName(i18n, page.name)}</h3>
      <ul class="widget-list">
        {#each page.widgets as widget (widget.id)}
          <li>
            <span class="widget-name">{displayName(widget.name, widgetTypeName(i18n, widget.type))}</span>
            <span class="muted" class:technical={!widgetTypeLabel(widget.type)}>{widgetGeometry(i18n, widget)}</span>
            {#if !widget.editable || !widgetTypeLabel(widget.type)}<StatusBadge {i18n} label={{ id: 'common.state.edit_unsupported' }} tone="warning" />{/if}
          </li>
        {/each}
      </ul>
    </section>
  {/each}
</section>
