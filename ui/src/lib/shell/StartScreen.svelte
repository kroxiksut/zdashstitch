<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Skeleton, StatusBadge } from '../components';
  import ComingLater from './ComingLater.svelte';
  import { duplicateNames, isoTime, recentDrafts, type DraftListResult } from './drafts';
  import { recentOriginText, sourceConnectionSuffix } from './format';
  import { KIND_LABEL, ORIGIN_LABEL } from './labels';
  import { draftPath, serverPath } from './router';
  import type { HeaderConnection } from '../connections/state';

  // «Start» (P1-01A, mockup 01-01). `drafts` is null while the list is loading. The server card
  // shows the one active connection; without it, «Connect to Zabbix».
  let { i18n, drafts, connection = null }: {
    i18n: I18n;
    drafts: DraftListResult | null;
    connection?: HeaderConnection | null;
  } = $props();

  const active = $derived(connection && connection.mode !== 'offline' && connection.localId ? connection : null);

  const recent = $derived(drafts?.status === 'ready' ? recentDrafts(drafts.drafts) : []);
  // P2-08A: the same name among all drafts shows the source connection next to the name.
  const duplicates = $derived(drafts?.status === 'ready' ? duplicateNames(drafts.drafts, i18n.locale) : new Set<string>());
  const dateFormat: Intl.DateTimeFormatOptions = { dateStyle: 'short', timeStyle: 'short' };
</script>

<div class="screen start">
  <header class="screen-head">
    <p class="eyebrow">{i18n.t('startup.kicker')}</p>
    <h1>{i18n.t('startup.question')}</h1>
    <p class="lead">{i18n.t('startup.intro')}</p>
  </header>

  <div class="start-cards">
    <section class="card" aria-labelledby="start-server-title">
      <h2 id="start-server-title">{i18n.t('startup.server.title')}</h2>
      <p>{i18n.t('startup.server.help')}</p>
      {#if active}
        <p>
          <StatusBadge
            {i18n}
            label={active.mode === 'read_write'
              ? { id: 'connection.state.read_write_profile', params: { profile: active.profileName ?? '—' } }
              : { id: 'connection.state.read_only_profile', params: { profile: active.profileName ?? '—' } }}
            tone="success"
          />
        </p>
        <div class="actions">
          <a class="button-link primary" href={serverPath(active.localId)}>{i18n.t('startup.action.open_server')}</a>
        </div>
      {:else}
        <p><StatusBadge {i18n} label={{ id: 'connection.state.not_connected' }} tone="info" /></p>
        <div class="actions">
          <a class="button-link primary" href="/connections">{i18n.t('startup.action.connect')}</a>
        </div>
      {/if}
    </section>

    <section class="card" aria-labelledby="start-local-title">
      <h2 id="start-local-title">{i18n.t('startup.local.title')}</h2>
      <p><StatusBadge {i18n} label={{ id: 'startup.local.badge' }} tone="success" /></p>
      <p>{i18n.t('startup.local.help')}</p>
      <div class="tiles">
        <section class="tile" aria-labelledby="start-tile-dashboard">
          <h3 id="start-tile-dashboard">{i18n.t('object.kind.dashboard')}</h3>
          <p>{i18n.t('startup.local.dashboard_help')}</p>
          <ComingLater {i18n} id="start-dashboard-actions">
            <Button {i18n} label={{ id: 'startup.action.new_dashboard' }} disabled />
            <Button {i18n} label={{ id: 'startup.action.assemble_dashboard' }} disabled />
          </ComingLater>
        </section>
        <section class="tile" aria-labelledby="start-tile-page">
          <h3 id="start-tile-page">{i18n.t('object.kind.page')}</h3>
          <p>{i18n.t('startup.local.page_help')}</p>
          <ComingLater {i18n} id="start-page-actions">
            <Button {i18n} label={{ id: 'startup.action.new_page' }} disabled />
            <Button {i18n} label={{ id: 'startup.action.take_page' }} disabled />
          </ComingLater>
        </section>
      </div>
      <ComingLater {i18n} id="start-import-actions">
        <Button {i18n} label={{ id: 'startup.action.import_files' }} disabled />
      </ComingLater>
      <p class="hint">{i18n.t('startup.import.help')}</p>
    </section>
  </div>

  <section class="card recent" aria-labelledby="start-recent-title">
    <div class="section-head">
      <h2 id="start-recent-title">{i18n.t('startup.recent.title')}</h2>
      <a href="/drafts">{i18n.t('startup.action.all_drafts')}</a>
    </div>
    {#if drafts === null}
      <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={2} />
    {:else if recent.length === 0}
      <p class="muted">{i18n.t('startup.recent.empty')}</p>
    {:else}
      <ul class="recent-list">
        {#each recent as draft (draft.local_id)}
          {@const origin = recentOriginText(i18n, draft)}
          {@const suffix = sourceConnectionSuffix(i18n, draft, duplicates)}
          <li>
            <span class="row-name">
              <a href={draftPath(draft.local_id)}>{draft.name}</a>
              {#if suffix}<span class="muted">{suffix}</span>{/if}
            </span>
            <span class="badges">
              <StatusBadge {i18n} label={KIND_LABEL[draft.kind]} tone="info" />
              <StatusBadge {i18n} label={ORIGIN_LABEL[draft.origin]} tone="info" />
            </span>
            {#if origin !== ''}<span class="origin-line muted">{origin}</span>{/if}
            <time class="muted" datetime={isoTime(draft.updated_at)}>{i18n.date(draft.updated_at, dateFormat)}</time>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>
