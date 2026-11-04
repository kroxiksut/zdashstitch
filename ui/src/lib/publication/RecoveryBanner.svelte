<script lang="ts">
  import type { I18n } from '../i18n';
  import { Banner, Button, resolveText } from '../components';
  import { draftPath } from '../shell/router';
  import { intentText, publicationCodeText, resultBody, stateLabel, STATE_LABEL } from './messages';
  import type { DamagedJournal, JournalEntry, JournalTarget } from './model';
  import RecoveryDialogs from './RecoveryDialogs.svelte';
  import type { RecoveryStore } from './store.svelte';

  // `startup.recovery.found` (P1-01G §4, P1-08G): unfinished publication operations and damaged
  // journal files, shown on «Начало», in «Черновики» and in the workspace of the affected draft
  // before any new write. Each operation names its operation, object kind, source and target in
  // separate fields; the safe actions are «Проверить результат» (reads; read-only mode is
  // enough), «Журнал операции», «Открыть черновик», «Закрыть без сверки» after a manual check or
  // when the profile is gone, and «Убрать в карантин» for a damaged file (item 4, 28.10.2026).
  let { i18n, store, localId = null, profileName, draftName, activeConnectionId }: {
    i18n: I18n;
    store: RecoveryStore;
    /** Only the operations of this draft (and every damaged file); `null` for all. */
    localId?: string | null;
    profileName: (connectionLocalId: string) => string | null;
    draftName: (localId: string) => string | null;
    activeConnectionId: string | null;
  } = $props();

  const list = $derived(localId === null ? (store.list ?? { unfinished: [], damaged: [] }) : store.forDraft(localId));
  let closing = $state<JournalEntry | null>(null);
  let quarantining = $state<DamagedJournal | null>(null);

  function targetText(target: JournalTarget | null): string {
    if (target === null) return '—';
    if (target.kind === 'new_dashboard') return i18n.t('publication.journal.target_new', { name: target.name ?? '' });
    return target.name ? i18n.t('publication.journal.target_existing', { name: target.name }) : i18n.t('publication.journal.target_existing_unnamed');
  }

  function profileText(connectionLocalId: string): string {
    return profileName(connectionLocalId) ?? i18n.t('publication.journal.profile_removed');
  }

  function sourceText(entry: JournalEntry): string {
    const name = entry.source ? draftName(entry.source.local_id) : null;
    return entry.source && name !== null ? i18n.t('publication.journal.source_draft', { name, revision: entry.source.revision }) : i18n.t('publication.journal.source_removed');
  }

  function body(entry: JournalEntry): string {
    return i18n.t('startup.recovery.body', {
      intent: resolveText(i18n, intentText(entry.intent)),
      object_kind: i18n.t('object.kind.dashboard'),
      source: sourceText(entry),
      target: targetText(entry.target),
      profile: profileText(entry.connection_local_id),
      time: entry.submitted_at ?? entry.confirmed_at
    });
  }
</script>

{#if list.unfinished.length > 0 || list.damaged.length > 0 || store.loadFailure}
  <section class="recovery" aria-label={i18n.t('startup.recovery.a11y.region')}>
    {#if store.loadFailure}
      <Banner {i18n} heading={{ id: 'publication.state.unknown_outcome' }} body={{ id: 'startup.recovery.load_failed', params: { code: store.loadFailure.code } }} tone="danger" blocking />
    {/if}
    {#if list.unfinished.length > 0}
      <div class="banner warning" role="alert">
        <h2>{i18n.t('startup.recovery.found', { count: list.unfinished.length })}</h2>
        <ul class="recovery-list">
          {#each list.unfinished as entry (entry.operation_id)}
            {@const connected = activeConnectionId === entry.connection_local_id}
            <li class="card">
              <p>{body(entry)}</p>
              <p class="muted">{i18n.t('startup.recovery.state', { state: resolveText(i18n, STATE_LABEL[entry.state] ?? stateLabel(entry.state)) })}</p>
              {#if entry.state === 'manual_check_required'}
                <p class="muted">{resolveText(i18n, resultBody(i18n, entry.state, entry.kind, entry.evidence, null))}</p>
              {/if}
              {#if !connected}
                <p class="muted">{i18n.t('startup.recovery.connect_first', { profile: profileText(entry.connection_local_id) })}</p>
              {/if}
              {#if store.commandFailure?.operationId === entry.operation_id}
                <p class="tone-danger" role="alert">{resolveText(i18n, publicationCodeText(i18n, store.commandFailure.failure))}</p>
              {/if}
              <div class="actions">
                {#if connected}
                  <Button {i18n} variant="primary" label={{ id: 'publication.action.reconcile' }} busy={store.busy === entry.operation_id} disabled={store.busy !== null} onclick={() => void store.reconcile(entry.operation_id)} />
                {:else}
                  <a class="button-link primary" href="/connections">{i18n.t('startup.recovery.action.connect')}</a>
                {/if}
                <Button {i18n} variant="quiet" label={{ id: 'publication.action.open_journal' }} onclick={() => void store.show(entry.operation_id)} />
                {#if entry.source && draftName(entry.source.local_id) !== null && localId === null}
                  <a class="button-link" href={draftPath(entry.source.local_id, 'publication')}>{i18n.t('startup.recovery.action.open_draft')}</a>
                {/if}
                {#if entry.state === 'manual_check_required' || profileName(entry.connection_local_id) === null}
                  <Button {i18n} label={{ id: 'startup.recovery.close_unverified' }} disabled={store.busy !== null} onclick={() => (closing = entry)} />
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    {#if list.damaged.length > 0}
      <div class="banner danger" role="alert">
        <h2>{i18n.t('startup.recovery.damaged.title', { count: list.damaged.length })}</h2>
        <ul class="recovery-list">
          {#each list.damaged as damaged (damaged.operation_id)}
            <li class="card">
              <p>
                {damaged.target
                  ? i18n.t('startup.recovery.damaged.body_target', { operation_id: damaged.operation_id, code: damaged.code, target: targetText(damaged.target) })
                  : i18n.t('startup.recovery.damaged.body_all', { operation_id: damaged.operation_id, code: damaged.code })}
              </p>
              {#if store.commandFailure?.operationId === damaged.operation_id}
                <p class="tone-danger" role="alert">{resolveText(i18n, publicationCodeText(i18n, store.commandFailure.failure))}</p>
              {/if}
              <div class="actions">
                <Button {i18n} label={{ id: 'startup.recovery.quarantine' }} disabled={store.busy !== null} onclick={() => (quarantining = damaged)} />
              </div>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
    <div role="status" aria-live="polite">
      {#if store.announcement}<p class="app-notice">{resolveText(i18n, store.announcement)}</p>{/if}
      {#if store.reconciled}<p class="app-notice">{resolveText(i18n, STATE_LABEL[store.reconciled.state])}</p>{/if}
    </div>
  </section>
{/if}

<RecoveryDialogs
  {i18n}
  {store}
  {closing}
  {quarantining}
  targetText={(entry) => targetText(entry.target)}
  sourceText={(entry) => sourceText(entry)}
  profileText={(entry) => profileText(entry.connection_local_id)}
  onclosed={() => {
    closing = null;
    quarantining = null;
  }}
/>
