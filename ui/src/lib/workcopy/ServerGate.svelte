<script lang="ts">
  import type { I18n } from '../i18n';
  import { EmptyState, Skeleton } from '../components';

  // «Панели в Zabbix» before the list can be shown (P1-01B §3, P1-03B states): the profile list is
  // still loading, there is no connection, or the address names a connection that is not active.
  let { i18n, state }: { i18n: I18n; state: 'loading' | 'offline' | 'other_connection' } = $props();
</script>

<main id="main-content" tabindex="-1">
  <div class="screen">
    {#if state === 'loading'}
      <h1 class="visually-hidden">{i18n.t('server.dashboards.title')}</h1>
      <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={3} />
    {:else}
      <EmptyState
        {i18n}
        level={1}
        heading={{ id: 'server.dashboards.title' }}
        reason={state === 'offline' ? { id: 'server.empty.offline' } : { id: 'server.empty.other_connection' }}
      >
        {#snippet action()}<a class="button-link primary" href="/connections">{i18n.t('startup.action.connect')}</a>{/snippet}
      </EmptyState>
    {/if}
  </div>
</main>
