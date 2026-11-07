<script lang="ts">
  import { untrack } from 'svelte';
  import type { I18n } from '../i18n';
  import BindingsSection from '../bindings/BindingsSection.svelte';
  import CloneSection from '../bindings/CloneSection.svelte';
  import PublicationSection from '../bindings/PublicationSection.svelte';
  import type { ReplaceHostStore } from '../bindings/store.svelte';
  import type { PublicationStore, RecoveryStore } from '../publication/store.svelte';
  import RecoveryBanner from '../publication/RecoveryBanner.svelte';
  import { copyDraftName, copyNaming } from '../workcopy/server';
  import { Breadcrumb, EmptyState, Skeleton, StatusBadge, resolveText, type LocalizedText } from '../components';
  import ChecksSection from './ChecksSection.svelte';
  import OverviewSection from './OverviewSection.svelte';
  import StructureSection from './StructureSection.svelte';
  import { widgetCount, type DraftLoadResult } from './drafts';
  import { originSummaryText, sourceText } from './format';
  import { rowsUsed } from './preview';
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
  import { workcopyCodeText } from '../workcopy/messages';

  // Draft workspace (P1-01B §2): side column with sections, context line, section content.
  // `result` is null while the draft is loading. «Publication» is visible offline and shows
  // requirements without any write action (P1-01B §2).
  let {
    i18n,
    localId,
    section,
    result,
    connected = false,
    replaceHost,
    activeConnectionId = null,
    connection = null,
    demo = false,
    publication,
    recovery,
    profileName = () => null,
    draftNames = []
  }: {
    i18n: I18n;
    localId: string;
    section: DraftSection;
    result: DraftLoadResult | null;
    /** A connection to Zabbix is active (read-only or read-write). */
    connected?: boolean;
    /** ReplaceHost of the draft: «Привязки» (P2-03E) and the preview of «Публикация» (P2-03D). */
    replaceHost?: ReplaceHostStore;
    /** Opaque ID of the active connection profile, if any. */
    activeConnectionId?: string | null;
    /** Profile name, mode, version and address of the active connection, for publication. */
    connection?: { name: string; mode: LocalizedText; writable?: boolean; version?: string | null; endpoint?: string | null; localId?: string | null } | null;
    /** DEMO build: synthetic data stands in for the server reads. */
    demo?: boolean;
    /** Steps 1, 4 and 5 of «Публикация» (P2-04F). */
    publication?: PublicationStore;
    /** Unfinished journal operations (P1-01G §4): shown in the workspace of the affected draft. */
    recovery?: RecoveryStore;
    profileName?: (connectionLocalId: string) => string | null;
    /** Names of the known local drafts, for «… (копия N)». */
    draftNames?: ReadonlyArray<string>;
  } = $props();


  $effect(() => {
    if (!replaceHost || (section !== 'bindings' && section !== 'publication')) return;
    const id = localId;
    untrack(() => replaceHost.open(id));
  });

  const draft = $derived(result?.status === 'ready' ? result.draft : null);
  // Server reads for bindings go only through the connection the copy was made with.
  const serverReads = $derived(
    demo || (activeConnectionId !== null && draft?.source?.connection_local_id !== undefined && activeConnectionId === draft.source.connection_local_id)
  );
  const originSummary = $derived(draft ? originSummaryText(i18n, draft) : null);
  // «Скопировать заново» names the new local copy «… (копия N)» (P2-08A).
  const refetchName = $derived(
    draft ? (copyDraftName(copyNaming(i18n, draft.source?.page_name ?? draft.source?.dashboard_name ?? draft.name), true, [...draftNames, draft.name]) ?? '') : ''
  );
  const draftName = (id: string): string | null => (draft && id === draft.local_id ? draft.name : null);
</script>

<div class="workspace">
  {#if draft}
    <!-- P2-08A: the sections are titled and drawn as a list of links so that they are noticed. -->
    <nav class="side-nav" aria-label={i18n.t('draft.a11y.sections')}>
      <p id="workspace-nav-title" class="side-nav-title">{i18n.t('draft.nav.title')}</p>
      <ul class="section-links">
        {#each DRAFT_SECTIONS as item (item)}
          <li><a href={draftPath(localId, item)} aria-current={item === section ? 'page' : undefined}>{resolveText(i18n, DRAFT_SECTION_LABEL[item])}</a></li>
        {/each}
      </ul>
      <section class="side-summary" aria-labelledby="workspace-origin-title">
        <p id="workspace-origin-title" class="eyebrow">{i18n.t('draft.overview.origin')}</p>
        <p>{resolveText(i18n, ORIGIN_LABEL[draft.origin])}</p>
        {#if originSummary}<p class="muted">{originSummary}</p>{/if}
        <p class="muted">{i18n.t('object.page.widget_count', { count: widgetCount(draft.pages) })}</p>
        {#if draft.kind === 'page' && draft.pages[0]}
          <p class="muted">{i18n.t('draft.overview.grid_rows_used', { rows: rowsUsed(draft.pages[0].widgets) })}</p>
        {/if}
      </section>
    </nav>
  {/if}

  <main id="main-content" tabindex="-1">
    {#if result === null}
      <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={4} />
    {:else if draft === null}
      <EmptyState
        {i18n}
        level={1}
        heading={{ id: 'drafts.title' }}
        reason={result.status === 'failed' ? workcopyCodeText(result.failure.code, result.failure.params) : { id: 'startup.draft.missing' }}
      >
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
        {#if draft.origin === 'server_copy'}<p class="muted">{i18n.t('draft.overview.local_changes_note')}</p>{/if}
        {#if draft.source}<p class="muted">{i18n.t('draft.context.source', { source: sourceText(i18n, draft.source) })}</p>{/if}
        {#if draft.target}<p class="muted">{i18n.t('draft.context.target', { target: draft.target.dashboard_name })}</p>{/if}
      </header>

      {#if recovery && section !== 'publication'}
        <RecoveryBanner {i18n} store={recovery} {localId} {profileName} {draftName} activeConnectionId={activeConnectionId} />
      {/if}

      {#if section === 'overview'}
        <OverviewSection {i18n} {draft} {connected} />
      {:else if section === 'structure'}
        <StructureSection {i18n} {draft} />
      {:else if section === 'bindings' && replaceHost}
        {#if !demo && draft.kind === 'dashboard' && draft.origin === 'server_copy'}
          {#key localId}
            <CloneSection {i18n} {localId} connected={serverReads} writable={serverReads && connection?.writable === true}
              onjournal={recovery ? (id) => { void recovery?.show(id); } : undefined} />
          {/key}
        {/if}
        <BindingsSection {i18n} store={replaceHost} {localId} {serverReads} />
      {:else if section === 'bindings'}
        <EmptyState {i18n} heading={{ id: 'draft.nav.bindings' }} reason={{ id: 'common.state.coming_later' }} />
      {:else if section === 'checks'}
        <ChecksSection {i18n} {draft} />
      {:else if replaceHost}
        <PublicationSection {i18n} store={replaceHost} {draft} {localId} {serverReads} {connection} {publication} {recovery} {profileName} {draftName} copyName={refetchName} />
      {:else}
        <EmptyState {i18n} heading={{ id: 'draft.nav.publication' }} reason={{ id: 'publication.requirement.no_connection' }} />
      {/if}
    {/if}
  </main>
</div>
