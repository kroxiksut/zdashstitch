<script lang="ts">
  import type { I18n } from '../i18n';
  import { Banner, Button, EmptyState, Icon, Skeleton, StatusBadge, resolveText } from '../components';
  import { connectionPath } from '../shell/router';
  import { parseTimestamp } from '../timestamp';
  import type { LocalizedText } from '../components';
  import type { ConnectionProfile } from './client';
  import EnableWritesDialog from './EnableWritesDialog.svelte';
  import { authSummary, codeText, stateBadge } from './messages';
  import { cardActions, type CardAction } from './state';
  import type { ConnectionsStore } from './store.svelte';

  // «Connections» list (P1-02A, mockup 05-01). Profiles carry no secret; the card shows the
  // normalized address, the last confirmed version and time, and where the secret lives.
  let { i18n, store, navigate }: { i18n: I18n; store: ConnectionsStore; navigate: (path: string) => void } = $props();

  let writesFor = $state<ConnectionProfile | null>(null);
  let writesBusy = $state(false);
  const list = $derived(store.list);

  const ACTION_LABEL = {
    connect: { id: 'connection.action.connect' },
    sign_in: { id: 'connection.action.sign_in' },
    disconnect: { id: 'connection.action.disconnect' },
    check: { id: 'connection.action.check' },
    edit: { id: 'connection.action.edit' },
    cancel: { id: 'connection.action.cancel' },
    enable_writes: { id: 'connection.action.enable_writes' },
    set_read_only: { id: 'connection.action.set_read_only' }
  } as const;

  const PRIMARY: ReadonlyArray<CardAction> = ['connect', 'sign_in'];

  function act(profile: ConnectionProfile, action: CardAction): void {
    switch (action) {
      case 'connect':
      case 'sign_in':
        void store.connect(profile);
        break;
      case 'disconnect':
        void store.disconnect(profile);
        break;
      case 'check':
        navigate(connectionPath(profile.local_id, 'check'));
        break;
      case 'edit':
        navigate(connectionPath(profile.local_id, 'edit'));
        break;
      case 'cancel':
        store.cancel(profile.local_id);
        break;
      case 'enable_writes':
        writesFor = profile;
        break;
      case 'set_read_only':
        void store.setReadOnly(profile.local_id);
        break;
    }
  }

  async function confirmWrites(): Promise<void> {
    if (!writesFor) return;
    writesBusy = true;
    await store.enableWrites(writesFor);
    writesBusy = false;
    writesFor = null;
  }
</script>

<main id="main-content" tabindex="-1">
  <div class="screen connections">
    <header class="screen-head with-actions">
      <div class="screen-head">
        <h1>{i18n.t('connection.list.title')}</h1>
        <p class="lead">{i18n.t('connection.list.subtitle')}</p>
      </div>
      <a class="button-link primary" href="/connections/new"><Icon name="plus" /><span>{i18n.t('connection.action.new')}</span></a>
    </header>

    <div class="announcement" role="status" aria-live="polite">
      {#if store.announcement}
        <p class="note {store.announcement.tone}">
          <span>{resolveText(i18n, store.announcement.text)}</span>
          {#if store.announcement.detail}<span>{resolveText(i18n, store.announcement.detail)}</span>{/if}
        </p>
      {/if}
    </div>

    {#if store.loadFailure && list === null}
      <Banner {i18n} heading={{ id: 'connection.list.load_failed' }} body={codeText(store.loadFailure.code, store.loadFailure.params)} tone="danger">
        {#snippet actions()}<Button {i18n} label={{ id: 'connection.action.retry' }} onclick={() => void store.refresh()} />{/snippet}
      </Banner>
    {:else if list === null}
      <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={3} />
    {:else if list.profiles.length === 0}
      <EmptyState {i18n} heading={{ id: 'connection.list.empty' }} reason={{ id: 'connection.list.empty_help' }}>
        {#snippet action()}
          <div class="actions centered">
            <a class="button-link primary" href="/connections/new"><Icon name="plus" /><span>{i18n.t('connection.action.new')}</span></a>
            <a class="button-link" href="/">{i18n.t('connection.action.continue_offline')}</a>
          </div>
        {/snippet}
      </EmptyState>
    {:else}
      <ul class="connection-cards" aria-label={i18n.t('connection.a11y.profiles')}>
        {#each list.profiles as profile (profile.local_id)}
          {@const busy = store.busy[profile.local_id]}
          {@const connecting = busy === 'connect'}
          {@const badge = stateBadge(profile, connecting, (id) => resolveText(i18n, { id } as LocalizedText))}
          {@const titleId = `connection-${profile.local_id}-title`}
          <li>
            <article class="connection-card" class:active={profile.state === 'connected'} aria-labelledby={titleId}>
              <div class="card-head">
                <h2 id={titleId}>{profile.name}</h2>
                <StatusBadge {i18n} label={badge.label} tone={badge.tone} />
              </div>
              <p class="technical address">{profile.endpoint_display}</p>
              <p class="meta muted">
                {#if profile.last_check?.server_version}
                  <span>{i18n.t('connection.result.version', { version: profile.last_check.server_version })}</span>
                {/if}
                {#if profile.last_check}
                  <span>{i18n.t('connection.card.checked_at', { time: parseTimestamp(profile.last_check.at) })}</span>
                {:else}
                  <span>{i18n.t('connection.card.never_checked')}</span>
                {/if}
                <span>{resolveText(i18n, authSummary(profile))}</span>
              </p>
              {#if profile.state === 'last_check_failed' && profile.last_check?.result_code}
                <p class="field-error">{resolveText(i18n, codeText(profile.last_check.result_code))}</p>
              {/if}
              {#if profile.state === 'connected'}
                <p class="hint">
                  {profile.mode === 'read_write' ? i18n.t('connection.mode.read_write_help') : i18n.t('connection.mode.read_only_help')}
                </p>
              {/if}
              {#if profile.plain_http}
                <p class="note warning"><Icon name="alert-triangle" /><span>{i18n.t('connection.warning.http')}</span></p>
              {/if}
              <div class="actions card-actions">
                {#each cardActions(profile, connecting) as action (action)}
                  <Button
                    {i18n}
                    label={ACTION_LABEL[action]}
                    variant={PRIMARY.includes(action) ? 'primary' : 'secondary'}
                    describedby={titleId}
                    busy={busy === 'other' && action === 'disconnect'}
                    disabled={busy !== undefined && action !== 'cancel'}
                    onclick={() => act(profile, action)}
                  />
                {/each}
              </div>
            </article>
          </li>
        {/each}
      </ul>
      <p><a href="/">{i18n.t('connection.action.continue_offline')}</a></p>
    {/if}
    <p class="hint">{i18n.t('connection.list.no_secrets')}</p>
  </div>
</main>

{#if writesFor}
  <EnableWritesDialog {i18n} profile={writesFor} busy={writesBusy} onconfirm={() => void confirmWrites()} oncancel={() => (writesFor = null)} />
{/if}

<style>
  .connections {
    max-width: 72rem;
  }

  .connection-cards {
    display: grid;
    gap: var(--space-4);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .connection-card {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-5) var(--space-6);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-panel);
    background: var(--surface-raised);
  }

  .connection-card.active {
    border: 2px solid var(--border-selected);
  }

  .card-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }

  .card-head h2 {
    font-size: 1.0625rem;
    overflow-wrap: anywhere;
  }

  .address {
    color: var(--text-code);
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0 var(--space-2);
    font-size: 0.875rem;
  }

  .meta span + span::before {
    content: '·';
    margin-inline-end: var(--space-2);
  }

  .card-actions {
    justify-content: flex-end;
  }

  .centered {
    justify-content: center;
  }

  .note {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid;
    border-radius: var(--radius-panel);
  }

  .button-link {
    gap: var(--space-2);
  }
</style>
