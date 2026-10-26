<script lang="ts">
  import type { I18n } from '../i18n';
  import { Breadcrumb, EmptyState, Skeleton, StatusBadge, resolveText } from '../components';
  import ChecksSection from './ChecksSection.svelte';
  import OverviewSection from './OverviewSection.svelte';
  import StructureSection from './StructureSection.svelte';
  import { widgetCount, type DraftLoadResult } from './drafts';
  import { sourceText } from './format';
  import {
    DRAFT_SECTION_LABEL,
    KIND_LABEL,
    LOCAL_STATE_LABEL,
    LOCAL_STATE_TONE,
    ORIGIN_LABEL,
    freshnessLabel,
    freshnessTone
  } from './labels';
  import { DRAFT_SECTIONS, draftPath, type DraftSection } from './router';

  // Draft workspace (P1-01B §2): side column with sections, context line, section content.
  // `result` is null while the draft is loading. «Publication» is visible offline and shows
  // requirements without any write action (P1-01B §2).
  let { i18n, localId, section, result }: {
    i18n: I18n;
    localId: string;
    section: DraftSection;
    result: DraftLoadResult | null;
  } = $props();

  const draft = $derived(result?.status === 'ready' ? result.draft : null);
</script>

<div class="workspace">
  {#if draft}
    <nav class="side-nav" aria-label={i18n.t('draft.a11y.sections')}>
      <p id="workspace-nav-title" class="eyebrow">{resolveText(i18n, KIND_LABEL[draft.kind])}</p>
      <ul>
        {#each DRAFT_SECTIONS as item (item)}
          <li><a href={draftPath(localId, item)} aria-current={item === section ? 'page' : undefined}>{resolveText(i18n, DRAFT_SECTION_LABEL[item])}</a></li>
        {/each}
      </ul>
      <div class="side-summary">
        <StatusBadge {i18n} label={ORIGIN_LABEL[draft.origin]} tone="info" />
        <p class="muted">{i18n.t('dashboards.summary.pages_widgets', { pages: draft.pages.length, widgets: widgetCount(draft.pages) })}</p>
      </div>
    </nav>
  {/if}

  <main id="main-content" tabindex="-1">
    {#if result === null}
      <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={4} />
    {:else if draft === null}
      <EmptyState {i18n} level={1} heading={{ id: 'drafts.title' }} reason={{ id: 'startup.draft.missing' }}>
        {#snippet action()}<a href="/drafts">{i18n.t('startup.action.all_drafts')}</a>{/snippet}
      </EmptyState>
    {:else}
      <Breadcrumb
        {i18n}
        label={{ id: 'shell.a11y.breadcrumbs' }}
        items={[
          { label: { id: 'drafts.title' }, href: '/drafts' },
          { label: { value: draft.name, kind: 'user' }, href: draftPath(localId, 'overview') },
          { label: DRAFT_SECTION_LABEL[section] }
        ]}
      />
      <header class="context-line">
        <h1>{draft.name}</h1>
        <div class="badges">
          <StatusBadge {i18n} label={KIND_LABEL[draft.kind]} tone="info" />
          <StatusBadge {i18n} label={ORIGIN_LABEL[draft.origin]} tone="info" />
          <StatusBadge {i18n} label={LOCAL_STATE_LABEL[draft.local_state]} tone={LOCAL_STATE_TONE[draft.local_state]} />
          {#if draft.freshness}<StatusBadge {i18n} label={freshnessLabel(draft.freshness)} tone={freshnessTone(draft.freshness)} />{/if}
          {#if draft.compatibility}<StatusBadge {i18n} label={{ id: 'draft.compat.label', params: { min_version: draft.compatibility.min_version } }} tone="info" />{/if}
        </div>
        {#if draft.source}<p class="muted">{i18n.t('draft.context.source', { source: sourceText(i18n, draft.source) })}</p>{/if}
        {#if draft.target}<p class="muted">{i18n.t('draft.context.target', { target: draft.target.dashboard_name })}</p>{/if}
      </header>

      {#if section === 'overview'}
        <OverviewSection {i18n} {draft} />
      {:else if section === 'structure'}
        <StructureSection {i18n} {draft} />
      {:else if section === 'bindings'}
        <EmptyState {i18n} heading={{ id: 'draft.nav.bindings' }} reason={{ id: 'common.state.coming_later' }} />
      {:else if section === 'checks'}
        <ChecksSection {i18n} {draft} />
      {:else}
        <EmptyState {i18n} heading={{ id: 'draft.nav.publication' }} reason={{ id: 'publication.requirement.no_connection' }} />
      {/if}
    {/if}
  </main>
</div>
