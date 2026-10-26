<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';

  import { loadHealth } from './lib/api';
  import { EmptyState } from './lib/components';
  import { applyDocumentLocale, createI18n, detectLocale, type Locale } from './lib/i18n';
  import AppHeader from './lib/shell/AppHeader.svelte';
  import DraftsScreen from './lib/shell/DraftsScreen.svelte';
  import DraftWorkspace from './lib/shell/DraftWorkspace.svelte';
  import SettingsScreen from './lib/shell/SettingsScreen.svelte';
  import StartScreen from './lib/shell/StartScreen.svelte';
  import {
    unavailableDraftsClient,
    type DraftListResult,
    type DraftLoadResult,
    type DraftsClient
  } from './lib/shell/drafts';
  import type { ConnectionMode } from './lib/shell/labels';
  import { internalLinkPath, parseRoute, topSection, type Route } from './lib/shell/router';

  // Application shell (P2-08): global header, routes P1-01B §3 / P1-01H §2, draft workspace.
  // There is no drafts or connection API yet: the default client reports «unavailable» and the
  // connection mode is «Offline»; nothing on these screens is sample data.
  let { draftsClient = unavailableDraftsClient, initialPath, locale }: {
    draftsClient?: DraftsClient;
    initialPath?: string;
    locale?: Locale;
  } = $props();

  const i18n = createI18n(untrack(() => locale) ?? detectLocale(globalThis.navigator?.language));
  if (globalThis.document) applyDocumentLocale(document.documentElement, i18n.locale);

  const initial = parseRoute(untrack(() => initialPath) ?? globalThis.location?.pathname ?? '/');
  const mode: ConnectionMode = 'offline';
  let route = $state<Route>(initial.route);
  let backendUnavailable = $state(false);
  let draftList = $state<DraftListResult | null>(null);
  let draftLoad = $state<{ localId: string; result: DraftLoadResult | null } | null>(null);
  let listRequest = 0;
  let loadRequest = 0;

  async function refreshList(): Promise<void> {
    const request = ++listRequest;
    draftList = null;
    let result: DraftListResult;
    try {
      result = await draftsClient.list();
    } catch {
      result = { status: 'unavailable' };
    }
    if (request === listRequest) draftList = result;
  }

  async function loadDraft(localId: string): Promise<void> {
    const request = ++loadRequest;
    draftLoad = { localId, result: null };
    let result: DraftLoadResult;
    try {
      result = await draftsClient.load(localId);
    } catch {
      result = { status: 'unavailable' };
    }
    if (request === loadRequest) draftLoad = { localId, result };
  }

  $effect(() => {
    const name = route.name;
    if (name === 'start' || name === 'drafts') untrack(() => void refreshList());
  });

  $effect(() => {
    if (route.name !== 'draft') return;
    const localId = route.localId;
    untrack(() => {
      if (draftLoad?.localId !== localId) void loadDraft(localId);
    });
  });

  async function focusMain(): Promise<void> {
    await tick();
    document.getElementById('main-content')?.focus();
  }

  function navigate(path: string): void {
    const parsed = parseRoute(path);
    history.pushState(null, '', parsed.canonical ?? path);
    route = parsed.route;
    void focusMain();
  }

  onMount(() => {
    if (initial.canonical) history.replaceState(null, '', initial.canonical);
    const onPopState = (): void => {
      const parsed = parseRoute(location.pathname);
      if (parsed.canonical) history.replaceState(null, '', parsed.canonical);
      route = parsed.route;
      void focusMain();
    };
    const onClick = (event: MouseEvent): void => {
      const anchor = event.target instanceof Element ? event.target.closest('a') : null;
      if (!anchor) return;
      const path = internalLinkPath(anchor, location.origin, event);
      if (path === null) return;
      event.preventDefault();
      if (path !== location.pathname) navigate(path);
    };
    window.addEventListener('popstate', onPopState);
    document.addEventListener('click', onClick);
    loadHealth().catch(() => {
      backendUnavailable = true;
    });
    return () => {
      window.removeEventListener('popstate', onPopState);
      document.removeEventListener('click', onClick);
    };
  });
</script>

<svelte:head>
  <title>{i18n.t('common.product_name')}</title>
  <meta name="description" content={i18n.t('startup.meta.description')} />
</svelte:head>

<a class="skip-link" href="#main-content">{i18n.t('common.skip_to_content')}</a>
<AppHeader {i18n} current={topSection(route)} {mode} />
{#if backendUnavailable}
  <p class="app-alert" role="alert">{i18n.t('startup.error.unavailable')}</p>
{/if}

{#if route.name === 'draft'}
  <DraftWorkspace
    {i18n}
    localId={route.localId}
    section={route.section}
    result={draftLoad?.localId === route.localId ? draftLoad.result : null}
  />
{:else if route.name === 'settings'}
  <SettingsScreen {i18n} section={route.section} />
{:else}
  <main id="main-content" tabindex="-1">
    {#if route.name === 'start'}
      <StartScreen {i18n} drafts={draftList} />
    {:else if route.name === 'drafts'}
      <DraftsScreen {i18n} drafts={draftList} />
    {:else if route.name === 'server'}
      <div class="screen">
        <EmptyState {i18n} level={1} heading={{ id: 'server.dashboards.title' }} reason={{ id: 'connection.state.not_connected' }}>
          {#snippet action()}<a class="button-link primary" href="/connections">{i18n.t('startup.action.connect')}</a>{/snippet}
        </EmptyState>
      </div>
    {:else if route.name === 'library'}
      <div class="screen">
        <EmptyState {i18n} level={1} heading={{ id: 'library.title' }} reason={{ id: 'common.state.coming_later' }} />
      </div>
    {:else}
      <div class="screen">
        <EmptyState {i18n} level={1} heading={{ id: 'connection.list.title' }} reason={{ id: 'common.state.coming_later' }} />
      </div>
    {/if}
  </main>
{/if}
