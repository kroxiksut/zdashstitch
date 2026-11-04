<script lang="ts">
  import type { I18n } from '../i18n';
  import { Banner, Button, Icon, resolveText } from '../components';
  import type { DraftDetail } from '../shell/drafts';
  import { draftPath } from '../shell/router';
  import { publicationCodeText, REMOTE_CHANGE_LABEL } from './messages';
  import type { CheckResult, RemoteChange } from './model';
  import type { PublicationStore } from './store.svelte';

  // P1-08D / P2-04E (mockup 11-05): Remote differs from Base. The plan is invalid, steps 3–5 are
  // unavailable, the draft is not changed. Exits: «Сохранить копию» (primary), «Скопировать
  // заново» (new copy, replacement calculated again), «Проверить снова»; «Обновить поверх» is
  // shown unavailable. No automatic merge (O-014).
  let { i18n, store, check, draft, copyName, onsavecopy }: {
    i18n: I18n;
    store: PublicationStore;
    check: CheckResult;
    draft: DraftDetail;
    /** «… (копия N)» for the new local copy (P2-08A). */
    copyName: string;
    onsavecopy: () => void;
  } = $props();

  const dashboardName = $derived(draft.source?.dashboard_name ?? draft.name);
  const remote = $derived(check.remote);
  const state = $derived(check.source.state);

  function objectText(change: RemoteChange): string {
    if (change.widget_type !== null) {
      return i18n.t('publication.conflict.object.widget', { name: change.widget_name || '—', widget_type: change.widget_type });
    }
    if (change.page_number !== null) return i18n.t('publication.conflict.object.page', { number: change.page_number, name: change.page_name ?? '' });
    return i18n.t('publication.conflict.object.dashboard');
  }
</script>

<section class="publication-conflict" aria-labelledby="publication-conflict-title">
  <div role="alert">
    {#if state === 'source_missing'}
      <h3 id="publication-conflict-title">{i18n.t('publication.conflict.missing_title', { dashboard_name: dashboardName })}</h3>
      <p>{i18n.t('publication.conflict.missing_body')}</p>
    {:else if state === 'stale'}
      <h3 id="publication-conflict-title">{i18n.t('publication.conflict.stale_title')}</h3>
      <p>{i18n.t('publication.conflict.stale_body')}</p>
    {:else}
      <h3 id="publication-conflict-title">{i18n.t('publication.conflict.title', { dashboard_name: dashboardName })}</h3>
      <p>{i18n.t('publication.conflict.body')}</p>
      <p class="muted">{i18n.t('publication.conflict.detected_at', { time: store.checkedAt ?? Number.NaN, snapshot: draft.source?.snapshot_at ?? Number.NaN })}</p>
    {/if}
  </div>

  <div class="conflict-layout">
    {#if remote && !remote.missing}
      <div class="card">
        <table class="data-table">
          <caption>{i18n.t('publication.conflict.caption')}</caption>
          <thead>
            <tr>
              <th scope="col">{i18n.t('publication.conflict.column.object')}</th>
              <th scope="col">{i18n.t('publication.conflict.column.server')}</th>
              <th scope="col">{i18n.t('publication.conflict.column.mine')}</th>
            </tr>
          </thead>
          <tbody>
            {#each remote.changes as change, index (index)}
              <tr class:overlap={change.overlaps}>
                <th scope="row">{objectText(change)}</th>
                <td>{resolveText(i18n, REMOTE_CHANGE_LABEL[change.kind])}</td>
                <td>
                  {#if change.overlaps}
                    <Icon name="alert-triangle" />{i18n.t('publication.conflict.mine.host_replaced')}
                  {:else}
                    {i18n.t('publication.conflict.mine.none')}
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if remote.truncated > 0}<p class="muted">{i18n.t('publication.conflict.truncated', { count: remote.truncated })}</p>{/if}
        {#if remote.overlapping_widgets > 0}
          <p class="note"><Icon name="alert-triangle" />{i18n.t('publication.conflict.overlap', { count: remote.overlapping_widgets })}</p>
        {/if}
        <p class="muted">{i18n.t('publication.conflict.no_author')}</p>
      </div>
    {/if}

    <aside class="card" aria-labelledby="publication-conflict-next">
      <h3 id="publication-conflict-next" class="eyebrow">{i18n.t('publication.conflict.next_title')}</h3>
      <div class="exit">
        <Button {i18n} variant="primary" label={{ id: 'publication.target.save_copy' }} onclick={onsavecopy} />
        <p class="muted">{i18n.t('publication.conflict.save_copy_hint')}</p>
      </div>
      {#if state !== 'source_missing'}
        <div class="exit">
          <Button {i18n} label={{ id: 'workcopy.refetch.action' }} busy={store.refetching} disabled={store.refetching} onclick={() => void store.refetch(copyName)}>
            {#snippet leadingIcon()}<Icon name="copy" />{/snippet}
          </Button>
          <p class="muted">{i18n.t('publication.conflict.refetch_hint')}</p>
        </div>
      {/if}
      <div class="exit">
        <Button {i18n} label={{ id: 'publication.conflict.overwrite' }} disabled />
        <p class="muted">{i18n.t('publication.conflict.overwrite_unavailable')}</p>
      </div>
      <div class="exit">
        <Button {i18n} label={{ id: 'publication.check.recheck' }} busy={store.checking} disabled={store.checking} onclick={() => void store.recheck()}>
          {#snippet leadingIcon()}<Icon name="refresh" />{/snippet}
        </Button>
        <p class="muted">{i18n.t('publication.conflict.recheck_hint')}</p>
      </div>
    </aside>
  </div>

  <div role="status" aria-live="polite">
    {#if store.refetched}
      <p class="app-notice">
        <span>{i18n.t('publication.conflict.refetched', { name: store.refetched.name })}</span>
        {#if !store.refetched.replayed && store.refetched.replay_code}
          <span>{i18n.t('publication.conflict.refetch_not_replayed', { code: store.refetched.replay_code })}</span>
        {/if}
        <a href={draftPath(store.refetched.local_id, store.refetched.replayed ? 'bindings' : 'overview')}>{i18n.t('publication.conflict.open_new_copy')}</a>
      </p>
    {/if}
  </div>
  {#if store.refetchFailure}
    <Banner {i18n} heading={{ id: 'bindings.command_failed' }} body={publicationCodeText(i18n, store.refetchFailure)} tone="danger" />
  {/if}
</section>
