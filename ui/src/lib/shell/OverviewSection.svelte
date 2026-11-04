<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, StatusBadge, resolveText } from '../components';
  import ComingLater from './ComingLater.svelte';
  import { isoTime, type DraftDetail, type DraftWidget } from './drafts';
  import { SHORT_DATE_TIME, displayName, localProvenanceText, numberedPageName, pageName, widgetSize, widgetTypeName } from './format';
  import { DRAFT_SECTION_LABEL, overviewSteps, widgetTypeLabel } from './labels';
  import { draftPath } from './router';

  // «Overview» (P1-01B, mockup 07-01): source, target, composition in grid cells and next steps.
  // Actions that need later screens (source check, target choice, «Add page to dashboard») stay
  // visible but unavailable with a visible reason (P1-01H §6).
  let { i18n, draft, connected = false }: { i18n: I18n; draft: DraftDetail; connected?: boolean } = $props();

  function unsupported(widget: DraftWidget): boolean {
    return !widget.editable || widgetTypeLabel(widget.type) === null;
  }

  const anyUnsupported = $derived(draft.pages.some((page) => page.widgets.some(unsupported)));
  const steps = $derived(overviewSteps(draft.kind));
</script>

<section class="workspace-section" aria-labelledby="overview-title">
  <h2 id="overview-title">{i18n.t('draft.nav.overview')}</h2>

  <div class="overview-layout">
    <div class="overview-main">
      <div class="overview-cards">
        <section class="card" aria-labelledby="overview-source-title">
          <h3 id="overview-source-title" class="eyebrow">{i18n.t('draft.overview.source')}</h3>
          {#if draft.origin === 'server_copy' && draft.source}
            <dl class="details-list">
              <div>
                <dt>{i18n.t('object.kind.dashboard')}</dt>
                <dd>{draft.source.dashboard_name}</dd>
              </div>
              {#if draft.source.page_name !== undefined}
                <div>
                  <dt>{i18n.t('object.kind.page')}</dt>
                  <dd>{pageName(i18n, draft.source.page_name, draft.source.page_number)}</dd>
                </div>
              {/if}
              <div>
                <dt>{i18n.t('draft.overview.snapshot')}</dt>
                <dd><time datetime={isoTime(draft.source.snapshot_at)}>{i18n.date(draft.source.snapshot_at, SHORT_DATE_TIME)}</time></dd>
              </div>
            </dl>
            <ComingLater
              {i18n}
              id="overview-source-actions"
              hint={connected ? { id: 'common.state.coming_later' } : { id: 'draft.overview.needs_connection' }}
            >
              <Button {i18n} label={{ id: 'draft.action.check_source' }} disabled />
            </ComingLater>
          {:else if draft.provenance}
            <p>{localProvenanceText(i18n, draft.provenance, true)}</p>
          {:else}
            <p>{resolveText(i18n, { id: 'object.origin.local' })}</p>
          {/if}
        </section>

        <section class="card" aria-labelledby="overview-target-title">
          <h3 id="overview-target-title" class="eyebrow">{i18n.t('draft.overview.target')}</h3>
          <p>{draft.target ? draft.target.dashboard_name : i18n.t('draft.overview.target.none')}</p>
          <p class="muted">{i18n.t('draft.overview.target.independent')}</p>
          <ComingLater {i18n} id="overview-target-actions">
            <Button {i18n} label={{ id: 'startup.action.choose_dashboard' }} disabled />
          </ComingLater>
        </section>
      </div>

      <section class="composition" aria-labelledby="overview-composition-title">
        <h3 id="overview-composition-title" class="eyebrow">
          {i18n.t(draft.kind === 'page' ? 'draft.overview.composition.page' : 'draft.overview.composition.dashboard')}
        </h3>
        <p class="muted">{i18n.t('draft.overview.composition.help')}</p>
        {#each draft.pages as page, pageIndex (page.id)}
          {#if draft.kind === 'dashboard'}
            <h4 id={`overview-page-${pageIndex}`}>
              {numberedPageName(i18n, page.name, pageIndex + 1)}
              <span class="muted">{i18n.t('object.page.widget_count', { count: page.widgets.length })}</span>
            </h4>
          {/if}
          <ul class="widget-cards" aria-labelledby={draft.kind === 'dashboard' ? `overview-page-${pageIndex}` : 'overview-composition-title'}>
            {#each page.widgets as widget (widget.id)}
              <li class="widget-card">
                <span class="widget-name">{displayName(widget.name, widgetTypeName(i18n, widget.type))}</span>
                <span class="muted" class:technical={!widgetTypeLabel(widget.type)}>{widgetSize(i18n, widget)}</span>
                {#if widget.host_name}<span class="muted">{i18n.t('draft.overview.widget_host', { host: widget.host_name })}</span>{/if}
                {#if unsupported(widget)}<StatusBadge {i18n} label={{ id: 'common.state.edit_unsupported' }} tone="warning" />{/if}
              </li>
            {/each}
          </ul>
        {/each}
        {#if anyUnsupported}<p class="muted">{i18n.t('draft.overview.edit_unsupported_note')}</p>{/if}
      </section>
    </div>

    <section class="card overview-steps" aria-labelledby="overview-steps-title">
      <h3 id="overview-steps-title" class="eyebrow">{i18n.t('draft.overview.next_steps')}</h3>
      <ol class="steps">
        {#each steps as step, index (step.label.id)}
          {@const title = i18n.t('draft.overview.step.numbered', { number: index + 1, step: resolveText(i18n, step.label) })}
          <li>
            <span class="step-title">{title}</span>
            <span class="muted">{resolveText(i18n, step.help)}</span>
            <!-- P2-08A: a step with its own section opens it directly. -->
            {#if step.section}
              <a class="button-link step-link" href={draftPath(draft.local_id, step.section)}>
                {i18n.t('draft.overview.step.open_section', { section: resolveText(i18n, DRAFT_SECTION_LABEL[step.section]) })}
              </a>
            {/if}
          </li>
        {/each}
      </ol>
      {#if draft.kind === 'page'}
        <ComingLater {i18n} id="overview-append-actions">
          <Button {i18n} variant="primary" label={{ id: 'intent.append_page' }} disabled />
        </ComingLater>
      {/if}
    </section>
  </div>
</section>
