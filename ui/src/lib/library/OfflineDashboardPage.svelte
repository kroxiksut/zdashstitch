<script lang="ts">
  import { onMount } from 'svelte';
  import type { I18n } from '../i18n';
  import { draftPath } from '../shell/router';
  import type { PageClipboard } from './editor-clipboard';
  import { createCatalogClient, type OfflinePage, type OfflineDashboard } from './client';
  import OfflinePageBuilder from './OfflinePageBuilder.svelte';

  let { i18n, dashboardId, pageId, clipboard, onchanged }: {
    i18n: I18n;
    dashboardId: string;
    pageId: string;
    clipboard: PageClipboard;
    onchanged?: (dashboard: OfflineDashboard) => void;
  } = $props();
  const client = createCatalogClient();
  let page = $state<OfflinePage | null>(null);
  let failed = $state(false);
  onMount(() => {
    let active = true;
    void client.dashboardPage(dashboardId, pageId).then((loaded) => {
      if (active) page = loaded;
    }).catch(() => { if (active) failed = true; });
    return () => { active = false; };
  });
  function saved(): void {
    void client.dashboard(dashboardId).then((dashboard) => onchanged?.(dashboard)).catch(() => {});
  }
</script>

<div class="screen">
  <a href={draftPath(dashboardId)}>{i18n.t('offline_dashboard.back')}</a>
  <p class="muted">{i18n.t('offline_dashboard.sources_fixed')}</p>
  {#if failed}<p role="alert">{i18n.t('offline_dashboard.page_load_failed')}</p>
  {:else if page}
    {#key `${dashboardId}/${pageId}`}
      <OfflinePageBuilder {i18n} {clipboard} initialPage={page} {dashboardId} onchanged={saved} />
    {/key}
  {:else}<p>{i18n.t('common.state.loading')}</p>{/if}
</div>
