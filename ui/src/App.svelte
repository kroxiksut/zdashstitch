<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';

  import { loadHealth } from './lib/api';
  import { createHttpReplaceHostClient, type ReplaceHostClient } from './lib/bindings/client';
  import { ReplaceHostStore } from './lib/bindings/store.svelte';
  import { createHttpPublicationClient, type PublicationClient } from './lib/publication/client';
  import RecoveryBanner from './lib/publication/RecoveryBanner.svelte';
  import { PublicationStore, RecoveryStore } from './lib/publication/store.svelte';
  import { resolveText, type LocalizedText } from './lib/components';
  import { applyDocumentLocale, createI18n, type Locale } from './lib/i18n';
  import { createHttpConnectionsClient, type ConnectionsClient } from './lib/connections/client';
  import ConnectionCheck from './lib/connections/ConnectionCheck.svelte';
  import ConnectionsScreen from './lib/connections/ConnectionsScreen.svelte';
  import ProfileEditor from './lib/connections/ProfileEditor.svelte';
  import DefaultPasswordDialog from './lib/connections/DefaultPasswordDialog.svelte';
  import SignInDialog from './lib/connections/SignInDialog.svelte';
  import { ConnectionsStore } from './lib/connections/store.svelte';
  import AppHeader from './lib/shell/AppHeader.svelte';
  import CatalogLibrary from './lib/library/CatalogLibrary.svelte';
  import DraftsScreen from './lib/shell/DraftsScreen.svelte';
  import DraftWorkspace from './lib/shell/DraftWorkspace.svelte';
  import { AppearanceStore, createHttpAppearanceClient, documentEnvironment, type AppearanceClient } from './lib/settings/appearance.svelte';
  import SettingsScreen from './lib/shell/SettingsScreen.svelte';
  import StartScreen from './lib/shell/StartScreen.svelte';
  import {
    withConnectionNames,
    type DraftListResult,
    type DraftLoadResult,
    type DraftRenameResult,
    type DraftsClient
  } from './lib/shell/drafts';
  import { draftPath, internalLinkPath, parseRoute, serverPath, topSection, type Route } from './lib/shell/router';
  import { createHttpWorkcopyClients, type ImportOutcome, type ServerClient } from './lib/workcopy/client';
  import CopyScreen from './lib/workcopy/CopyScreen.svelte';
  import ServerDashboardsScreen from './lib/workcopy/ServerDashboardsScreen.svelte';
  import ServerGate from './lib/workcopy/ServerGate.svelte';
  import ServerPreviewScreen from './lib/workcopy/ServerPreviewScreen.svelte';
  import { ServerDashboardsStore } from './lib/workcopy/store.svelte';

  // Application shell (P2-08): global header, routes P1-01B §3 / P1-01H §2, draft workspace.
  // Drafts and copies of server dashboards (P2-05D) use the local server routes; nothing on these
  // screens is sample data. Connections (P2-01G) use the local server routes; the header shows the
  // one active connection or «Offline». The browser never talks to Zabbix.
  let { draftsClient, serverClient, connectionsClient, replaceHostClient, publicationClient, appearanceClient, initialPath, locale, demo = false }: {
    draftsClient?: DraftsClient;
    replaceHostClient?: ReplaceHostClient;
    publicationClient?: PublicationClient;
    serverClient?: ServerClient;
    connectionsClient?: ConnectionsClient;
    appearanceClient?: AppearanceClient;
    initialPath?: string;
    locale?: Locale;
    /** Only the DEMO build (synthetic drafts) sets this; every screen then shows «Demo data». */
    demo?: boolean;
  } = $props();

  // Language, theme and reduced motion come from the settings file (P2-00G); a language change
  // swaps the catalog in place, so routes, focus and form data stay.
  const appearance = new AppearanceStore(untrack(() => appearanceClient) ?? createHttpAppearanceClient(), documentEnvironment());
  const i18n = $derived(createI18n(locale ?? appearance.locale));
  $effect(() => applyDocumentLocale(document.documentElement, i18n.locale));

  const initial = parseRoute(untrack(() => initialPath) ?? globalThis.location?.pathname ?? '/');
  const http = createHttpWorkcopyClients();
  const drafts: DraftsClient = untrack(() => draftsClient) ?? http.drafts;
  const connections = new ConnectionsStore(untrack(() => connectionsClient) ?? createHttpConnectionsClient());
  const server = new ServerDashboardsStore(untrack(() => serverClient) ?? http.server, drafts);
  const replaceHost = new ReplaceHostStore(untrack(() => replaceHostClient) ?? createHttpReplaceHostClient());
  // Publication (P2-04F) and the journal recovery banner (P1-01G §4): one journal per process.
  const publishing = untrack(() => publicationClient) ?? createHttpPublicationClient();
  const recovery = new RecoveryStore(publishing);
  const publication = new PublicationStore(publishing, () => void recovery.load());
  let route = $state<Route>(initial.route);
  let backendUnavailable = $state(false);
  let draftList = $state<DraftListResult | null>(null);
  let draftLoad = $state<{ localId: string; result: DraftLoadResult | null } | null>(null);
  /** Result of the last user command, announced politely (P1-00E §2 `success`). */
  let announcement = $state<{ text: LocalizedText; detail?: LocalizedText } | null>(null);
  let listRequest = 0;
  let loadRequest = 0;

  /** Profile names for opaque connection IDs; a removed profile stays unnamed. */
  const nameOf = (localId: string): string | undefined => connections.profile(localId)?.name;
  const namedList = $derived<DraftListResult | null>(
    draftList?.status === 'ready' ? { ...draftList, drafts: draftList.drafts.map((draft) => withConnectionNames(draft, nameOf)) } : draftList
  );
  const namedLoad = $derived<DraftLoadResult | null>(
    draftLoad?.result?.status === 'ready' ? { status: 'ready', draft: withConnectionNames(draftLoad.result.draft, nameOf) } : (draftLoad?.result ?? null)
  );
  const activeId = $derived(connections.header.localId);
  const knownDraftNames = $derived(draftList?.status === 'ready' ? draftList.drafts.map((draft) => draft.name) : []);
  const draftNameOf = (localId: string): string | null =>
    (draftList?.status === 'ready' ? draftList.drafts.find((draft) => draft.local_id === localId)?.name : undefined) ??
    (draftLoad?.localId === localId && draftLoad.result?.status === 'ready' ? draftLoad.result.draft.name : null);
  const profileNameOf = (localId: string): string | null => nameOf(localId) ?? null;
  const serverConnectionId = $derived(
    route.name === 'server' || route.name === 'server_copy' || route.name === 'server_preview' ? route.connectionLocalId : null
  );

  async function refreshList(): Promise<void> {
    const request = ++listRequest;
    draftList = null;
    let result: DraftListResult;
    try {
      result = await drafts.list();
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
      result = await drafts.load(localId);
    } catch {
      result = { status: 'unavailable' };
    }
    if (request === loadRequest) draftLoad = { localId, result };
  }

  async function renameDraft(localId: string, name: string): Promise<DraftRenameResult> {
    const result = await drafts.rename(localId, name);
    if (result.status !== 'ready') return result;
    const renamed = result.draft;
    if (draftList?.status === 'ready') {
      draftList = { ...draftList, drafts: draftList.drafts.map((draft) => (draft.local_id === localId ? { ...draft, ...renamed } : draft)) };
    }
    if (draftLoad?.localId === localId && draftLoad.result?.status === 'ready') {
      draftLoad = { localId, result: { status: 'ready', draft: { ...draftLoad.result.draft, name: renamed.name, updated_at: renamed.updated_at } } };
    }
    announcement = { text: { id: 'drafts.rename.done', params: { name: renamed.name } } };
    return result;
  }

  /** A copy was created (P1-01E E6 → E7): announce it and open the new draft. */
  function imported(outcome: ImportOutcome, draftName: string): void {
    draftList = null;
    navigate(draftPath(outcome.local_id));
    const detail: LocalizedText | undefined =
      outcome.item_metadata === 'partial'
        ? { id: 'copy.result.metadata_partial' }
        : outcome.item_metadata === 'unavailable'
          ? { id: 'copy.result.metadata_unavailable' }
          : undefined;
    announcement = detail ? { text: { id: 'copy.result.created', params: { name: draftName } }, detail } : { text: { id: 'copy.result.created', params: { name: draftName } } };
  }

  $effect(() => {
    const name = route.name;
    if (name === 'start' || name === 'drafts') untrack(() => void refreshList());
  });

  $effect(() => {
    if (route.name !== 'draft') return;
    const localId = route.localId;
    untrack(() => {
      if (draftLoad?.localId !== localId || draftLoad.result?.status !== 'ready') void loadDraft(localId);
    });
  });

  // «Панели в Zabbix» (P1-01B §3): `/server` goes to the active connection; the store follows the
  // route connection and keeps nothing of another connection.
  $effect(() => {
    const target = route.name === 'server' && route.connectionLocalId === null ? activeId : null;
    if (target) untrack(() => navigate(serverPath(target), true));
  });

  $effect(() => {
    const connectionLocalId = serverConnectionId;
    const generation = connections.list?.active?.context_generation ?? null;
    if (connectionLocalId !== null && connectionLocalId === activeId) untrack(() => server.open(connectionLocalId, generation));
  });

  // Disconnected or session ended: the server list of that connection is no longer current.
  $effect(() => {
    if (connections.list !== null && activeId === null) untrack(() => server.close());
  });

  // The copy check and the preview (P2-05E) need a dashboard chosen in this run; a reload returns
  // to the list.
  $effect(() => {
    if ((route.name !== 'server_copy' && route.name !== 'server_preview') || connections.list === null) return;
    const connectionLocalId = route.connectionLocalId;
    if (server.connectionLocalId !== connectionLocalId || server.selectedRow === null) {
      untrack(() => navigate(serverPath(connectionLocalId), true));
    }
  });

  async function focusMain(): Promise<void> {
    await tick();
    // A new screen starts at the top: focusing <main> must not scroll the app header away.
    document.getElementById('main-content')?.focus({ preventScroll: true });
    window.scrollTo(0, 0);
  }

  function navigate(path: string, replace = false): void {
    const parsed = parseRoute(path);
    if (replace) history.replaceState(null, '', parsed.canonical ?? path);
    else history.pushState(null, '', parsed.canonical ?? path);
    announcement = null;
    route = parsed.route;
    void focusMain();
  }

  onMount(() => {
    if (initial.canonical) history.replaceState(null, '', initial.canonical);
    const onPopState = (): void => {
      const parsed = parseRoute(location.pathname);
      if (parsed.canonical) history.replaceState(null, '', parsed.canonical);
      announcement = null;
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
    void connections.refresh();
    void appearance.load();
    void recovery.load();
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
<AppHeader
  {i18n}
  current={topSection(route)}
  mode={connections.header.mode}
  profileName={connections.header.profileName}
  onsetreadonly={() => {
    if (connections.header.localId) void connections.setReadOnly(connections.header.localId);
  }}
/>
{#if demo}
  <p class="demo-marker" role="note">{i18n.t('shell.demo.marker')}</p>
{/if}
{#if backendUnavailable}
  <p class="app-alert" role="alert">{i18n.t('startup.error.unavailable')}</p>
{/if}
<div class="app-status" role="status" aria-live="polite">
  {#if announcement}
    <p class="app-notice">
      <span>{resolveText(i18n, announcement.text)}</span>
      {#if announcement.detail}<span>{resolveText(i18n, announcement.detail)}</span>{/if}
    </p>
  {/if}
</div>

{#if route.name === 'draft'}
  <DraftWorkspace
    {i18n}
    localId={route.localId}
    section={route.section}
    result={draftLoad?.localId === route.localId ? namedLoad : null}
    connected={connections.header.mode !== 'offline'}
    {replaceHost}
    {publication}
    {recovery}
    profileName={profileNameOf}
    draftNames={knownDraftNames}
    activeConnectionId={activeId}
    connection={connections.header.profileName
      ? {
          name: connections.header.profileName,
          mode: connections.header.mode === 'read_write' ? { id: 'connection.mode.read_write' } : { id: 'connection.mode.read_only' },
          writable: connections.header.mode === 'read_write',
          version: activeId ? (connections.profile(activeId)?.last_check?.server_version ?? null) : null,
          endpoint: activeId ? (connections.profile(activeId)?.endpoint_display ?? null) : null,
          localId: activeId
        }
      : null}
    {demo}
  />
{:else if route.name === 'settings'}
  <SettingsScreen {i18n} section={route.section} {appearance} />
{:else if route.name === 'connections'}
  <ConnectionsScreen {i18n} store={connections} {navigate} />
{:else if route.name === 'connection_new'}
  <ProfileEditor {i18n} store={connections} localId={null} {navigate} />
{:else if route.name === 'connection_edit'}
  {#key route.localId}<ProfileEditor {i18n} store={connections} localId={route.localId} {navigate} />{/key}
{:else if route.name === 'connection_check'}
  <ConnectionCheck {i18n} store={connections} localId={route.localId} {navigate} />
{:else if (route.name === 'server' || route.name === 'server_copy' || route.name === 'server_preview') && route.connectionLocalId !== null && route.connectionLocalId === activeId && server.connectionLocalId === route.connectionLocalId}
  {#if route.name === 'server'}
    <ServerDashboardsScreen
      {i18n}
      store={server}
      profileName={connections.header.profileName ?? '—'}
      serverVersion={connections.profile(route.connectionLocalId)?.last_check?.server_version ?? null}
      modeLabel={connections.header.mode === 'read_write' ? { id: 'connection.mode.read_write' } : { id: 'connection.mode.read_only' }}
      onimported={imported}
    />
  {:else if route.name === 'server_preview'}
    <ServerPreviewScreen {i18n} store={server} connectionLocalId={route.connectionLocalId} onimported={imported} />
  {:else}
    <CopyScreen {i18n} store={server} connectionLocalId={route.connectionLocalId} onimported={imported} />
  {/if}
{:else if route.name === 'server' || route.name === 'server_copy' || route.name === 'server_preview'}
  <ServerGate
    {i18n}
    state={connections.list === null && !connections.loadFailure
      ? 'loading'
      : connections.header.mode === 'offline'
        ? 'offline'
        : serverConnectionId !== null && serverConnectionId !== activeId
          ? 'other_connection'
          : 'loading'}
  />
{:else}
  <main id="main-content" tabindex="-1">
    {#if (route.name === 'start' || route.name === 'drafts') && (recovery.count > 0 || recovery.loadFailure)}
      <div class="screen recovery-screen">
        <RecoveryBanner {i18n} store={recovery} profileName={profileNameOf} draftName={draftNameOf} activeConnectionId={activeId} />
      </div>
    {/if}
    {#if route.name === 'start'}
      <StartScreen {i18n} drafts={namedList} connection={connections.header} />
    {:else if route.name === 'drafts'}
      <DraftsScreen {i18n} drafts={namedList} onrename={renameDraft} />
    {:else if route.name === 'library'}
      <CatalogLibrary {i18n} />
    {/if}
  </main>
{/if}

{#if connections.signIn}
  {#key connections.signIn}<SignInDialog {i18n} store={connections} request={connections.signIn} />{/key}
{:else if connections.defaultPasswordWarning}
  {#key connections.defaultPasswordWarning}<DefaultPasswordDialog {i18n} store={connections} profile={connections.defaultPasswordWarning} />{/key}
{/if}
