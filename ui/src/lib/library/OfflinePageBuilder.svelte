<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import { ApiFailure } from '../api';
  import type { I18n, MessageId } from '../i18n';
  import { createCatalogClient, portableWidgetTitle, type CatalogMetric, type CatalogSummary, type NewOfflinePage, type OfflinePage, type PageMetricChoice, type PageSeriesChoice, type PageRole, type PageHostSelection, type WidgetEvaluationProfile } from './client';
  import { validNewRole, validSource, singleSource } from './offline-roles';
  import { canonicalEditorState, pageRequestFromSaved, savedCatalogChoices, saveKnownPage } from './offline-edit';
  import { draftPath, dashboardPagePath, internalLinkPath, parseRoute } from '../shell/router';
  import { EditorHistory } from './editor-history';
  import { EditorAutosave } from './editor-autosave';
  import { createEditorSettingsClient, type EditorPreferences } from '../settings/editor';
  import { settingsPath } from '../shell/router';
  import EditorCanvas from './EditorCanvas.svelte';
  import SensitivePreview from './SensitivePreview.svelte';
  import PortablePatternReview from './PortablePatternReview.svelte';
  import PortableLldReview from './PortableLldReview.svelte';
  import PortablePublication from '../publication/PortablePublication.svelte';
  import type { ActiveConnection, ConnectionProfile } from '../connections/client';
  import RedactionPreview from './RedactionPreview.svelte';
  import SaveAsButton from './SaveAsButton.svelte';
  import ReusablePageActions from './ReusablePageActions.svelte';
  import type { FixedLayout } from './editor-geometry';
  import { PageEditingSession, type EditorAccess } from './editor-session';
  import { PageClipboard, type ClipboardCode, type ClipboardContent, type ClipboardContext, type ClipboardIssue } from './editor-clipboard';
  import { emptyResolution, initialResolution, inspectResolvedPaste, applyResolvedPaste } from './editor-paste-resolution';
  import PasteResolutionForm from './PasteResolutionForm.svelte';
  import { acceptsMetricType, compositionFindings, graphRowCount } from './editor-composition';
  import { Dialog, resolveText, type LocalizedText } from '../components';
  import type { LayoutChange } from './editor-geometry';

  let { i18n, initialPage = null, onchanged, clipboard = new PageClipboard(), view = 'edit', dashboardId, active = null, profile = null }: { i18n: I18n; initialPage?: OfflinePage | null; onchanged?: (page: OfflinePage) => void; clipboard?: PageClipboard; view?: 'edit' | 'checks'; dashboardId?: string; active?: ActiveConnection | null; profile?: ConnectionProfile | null } = $props();
  const client = createCatalogClient(undefined, untrack(() => dashboardId));
  const pagePath = (pageId: string): string => dashboardId ? dashboardPagePath(dashboardId, pageId) : draftPath(pageId, 'structure');
  type EditorCatalog = Omit<CatalogSummary, 'version'> & { version: number | null };
  let catalogs = $state<EditorCatalog[]>([]);
  let chosen = $state<EditorCatalog[]>([]);
  let metrics = $state<Record<string, CatalogMetric[]>>({});
  let selected = $state<PageSeriesChoice[]>([]);
  let roles = $state<Record<string, PageRole>>({ source: { cardinality: 'one', slots: ['source-01'] } });
  let roleId = $state('');
  let cardinality = $state<'one' | 'many'>('many');
  let slotLines = $state('');
  let roleError = $state(false);
  let roleInUse = $state(false);
  let editingRole = $state<string | null>(null);
  const newRole = $derived<PageRole>({ cardinality, slots: slotLines.split(/\r?\n/).map((slot) => slot.trim()).filter(Boolean) });
  let widgets = $state<NewOfflinePage['widgets']>([]);
  let opened = $state<OfflinePage | null>(null);
  let evaluationProfile = $state<WidgetEvaluationProfile>('8.0');
  const savedEvaluation = $derived((opened ?? initialPage)?.widget_evaluation);
  let editingWidget = $state<number | null>(null);
  let baseline = $state('');
  let conflict = $state(false);
  let loadWarning = $state(false);
  let name = $state('');
  let widgetTitle = $state('');
  let busy = $state(false);
  let savingPage = $state(false);
  let error = $state(false);
  let removalDependency = $state(false);
  let removalSelection = $state<string[]>([]);
  let preservedTitles = $state<Record<string, string>>({});
  let saved = $state(false);
  let librarySaved = $state(false);
  let librarySaveFailed = $state(false);
  let librarySaving = $state(false);
  let exporting = $state(false);
  let exportFailed = $state(false);
  let exportHash = $state<string | null>(null);
  let exportEntries = $state<Record<string, string> | null>(null);
  let redactionJump = $state<{ location: string; sequence: number } | null>(null);
  let catalogInUse = $state(false);
  let memoryHistory: EditorHistory<NewOfflinePage> | null = null;
  let undoAction = $state<MessageId | null>(null);
  let redoAction = $state<MessageId | null>(null);
  let historyTruncated = $state(false);
  let autosaveEnabled = $state(true);
  let autosaveMaxInterval = $state<30 | 60 | 120>(30);
  let defaultZoom = $state<EditorPreferences['default_zoom']>('fit_width');
  let showGrid = $state(true);
  let settingsRequest = 0;
  let retrySeconds = $state<number | null>(null);
  let editAccess = $state<EditorAccess>('claiming');
  let selectedWidgetIds = $state<string[]>([]);
  let clipboardContent = $state<ClipboardContent | null>(null);
  let clipboardIssues = $state<ClipboardIssue[]>([]);
  let pasteOpen = $state(false);
  let pastePlacement = $state<'original' | 'first_free'>('original');
  let pasteResolution = $state(emptyResolution());
  let findingFocus = $state<string | null>(null);
  const localScope = `unsaved-${crypto.randomUUID()}`;
  const canEdit = $derived(opened === null || editAccess === 'editing');
  const request = (): NewOfflinePage => ({ name, catalogs: chosen.filter((item): item is CatalogSummary => item.version !== null).map(({ catalog_id, version }) => ({ catalog_id, version })), roles, widgets });
  const modelDirty = $derived(canonicalEditorState(request()) !== baseline);
  const dirty = $derived(modelDirty || widgetTitle.trim() !== '' || selected.length > 0 || roleId.trim() !== '' || slotLines.trim() !== '');
  const pendingInput = $derived(selected.length > 0 || !!widgetTitle.trim() || !!roleId.trim() || !!slotLines.trim());
  const metricsByCatalog = $derived(Object.fromEntries(chosen.map((pin) => [pin.catalog_id, metrics[keyOf(pin)] ?? []])));
  const findings = $derived(compositionFindings(request(), metricsByCatalog, savedEvaluation));
  $effect(() => {
    if (view === 'edit' && findingFocus !== null) {
      const id = findingFocus; findingFocus = null;
      void tick().then(() => { const element = document.getElementById(id ? `editor-list-widget-${id}` : 'offline-page-name'); element?.focus(); element?.scrollIntoView({ block: 'nearest' }); });
    }
  });
  const pastePlan = $derived(pasteOpen ? inspectResolvedPaste($state.snapshot(clipboardContent), clipboardContext(), pastePlacement, $state.snapshot(pasteResolution)) : { issues: [], widgets: [], removeIds: [] });
  const clipboardMessages = {
    empty: 'offline_editor.clipboard.empty', opaque: 'offline_editor.clipboard.opaque', reference: 'offline_editor.clipboard.reference',
    catalog: 'offline_editor.clipboard.catalog', role: 'offline_editor.clipboard.role', metric: 'offline_editor.clipboard.metric',
    geometry: 'offline_editor.clipboard.geometry', limit: 'offline_editor.clipboard.limit', source_changed: 'offline_editor.clipboard.source_changed',
    cross_page_cut: 'offline_editor.clipboard.cross_page_cut', ids: 'offline_editor.clipboard.ids'
  } as const satisfies Record<ClipboardCode, MessageId>;
  function clipboardMessage(issue: ClipboardIssue): LocalizedText {
    if (issue.code === 'catalog' || issue.code === 'role' || issue.code === 'metric') return { id: clipboardMessages[issue.code], params: { value: issue.value ?? '' } };
    return { id: clipboardMessages[issue.code] };
  }
  function clipboardContext(): ClipboardContext { return { sourceId: opened?.local_id ?? localScope, page: $state.snapshot(request()), metrics: $state.snapshot(metricsByCatalog) }; }
  function copySelection(cut: boolean): void {
    if (busy || pendingInput || (cut && (!canEdit || conflict))) return;
    clipboardIssues = clipboard.copy(clipboardContext(), selectedWidgetIds, cut);
    clipboardContent = clipboard.read();
  }
  function openPaste(): void {
    if (busy || !canEdit || conflict || pendingInput) return;
    clipboardContent = clipboard.read(); pasteResolution = initialResolution($state.snapshot(clipboardContent), clipboardContext()); pastePlacement = 'original'; pasteOpen = true;
  }
  async function closePaste(): Promise<void> { pasteOpen = false; await tick(); document.getElementById('editor-paste')?.focus(); }
  function paste(): void {
    if (busy || !canEdit || conflict || pendingInput) return;
    const result = applyResolvedPaste($state.snapshot(clipboardContent), clipboardContext(), pastePlacement, () => `widget-${crypto.randomUUID().replaceAll('-', '')}`, $state.snapshot(pasteResolution));
    clipboardIssues = result.issues;
    if (!result.page) return;
    widgets = result.page.widgets; selectedWidgetIds = result.ids;
    clipboard.consumeCut(); clipboardContent = clipboard.read();
    commitChange('editor.op.paste'); void closePaste();
  }
  function removeSelection(ids: string[]): void {
    if (busy || !canEdit || conflict || pendingInput) return;
    if (widgets.some((widget) => ids.includes(widget.widget_id ?? '') && widget.kind === 'preserved')) {
      removalSelection = [...ids]; return;
    }
    applyRemoval(ids);
  }
  function applyRemoval(ids: string[]): void {
    if (busy || !canEdit || conflict) return;
    const currentId = editingWidget === null ? undefined : widgets[editingWidget]?.widget_id;
    widgets = widgets.filter((widget) => !ids.includes(widget.widget_id ?? '')); selectedWidgetIds = [];
    if (currentId !== undefined) {
      const index = widgets.findIndex((widget) => widget.widget_id === currentId);
      editingWidget = index < 0 ? null : index;
      if (index < 0) { selected = []; widgetTitle = ''; }
    }
    commitChange('editor.op.remove_widget');
  }
  function confirmRemoval(): void {
    applyRemoval(removalSelection);
    removalSelection = [];
  }
  function groupGeometry(changes: LayoutChange[]): void {
    if (busy || !canEdit || conflict) return;
    const byIndex = new Map(changes.map((change) => [change.index, change.layout]));
    widgets = widgets.map((widget, index) => byIndex.has(index) ? { ...widget, layout: byIndex.get(index)! } : widget);
    commitChange('editor.op.move');
  }
  $effect(() => { if (dirty) { saved = false; exportHash = null; exportEntries = null; } });
  const autosaver = new EditorAutosave({
    dirty: () => modelDirty && !conflict && canEdit,
    save: savePage,
    retryable: (failure) => opened !== null && !conflict && canEdit && !(failure instanceof Error && failure.message === 'offline_page_invalid') && !(failure instanceof ApiFailure && ['offline_page_invalid', 'offline_page_edit_lost', 'offline_page_widget_removal_dependency'].includes(failure.response?.code ?? '')),
    failed: (failure, retry) => {
      if (failure instanceof ApiFailure && failure.response?.code === 'offline_page_edit_lost') editingSession.lost();
      conflict ||= failure instanceof ApiFailure && failure.response?.code === 'offline_page_conflict';
      error = !conflict && !removalDependency; retrySeconds = retry;
    }
  });
  const editingSession = new PageEditingSession(client, (state) => {
    editAccess = state;
    autosaver.setEnabled(autosaveEnabled && (opened === null || state === 'editing'));
    if (state === 'read_only') { memoryHistory = new EditorHistory($state.snapshot(request())); syncHistory(); retrySeconds = null; }
  });

  async function claimEditor(takeOver = false): Promise<void> {
    if (!opened || (takeOver && dirty && !window.confirm(i18n.t('offline_builder.leave_dirty')))) return;
    busy = true;
    try {
      await editingSession.open(opened.local_id, takeOver);
      if (editAccess === 'editing') { hydrate(await client.page(opened.local_id)); conflict = false; error = false; }
    } catch { error = true; }
    finally { busy = false; }
  }

  function syncHistory(): void {
    undoAction = memoryHistory?.undoAction as MessageId | null;
    redoAction = memoryHistory?.redoAction as MessageId | null;
    historyTruncated = memoryHistory?.truncated ?? false;
  }
  function commitChange(action: MessageId): void {
    if (!canEdit) return;
    memoryHistory?.commit($state.snapshot(request()), action); syncHistory();
    if (opened) autosaver.changed();
  }
  function applySnapshot(snapshot: NewOfflinePage): void {
    removalDependency = false;
    name = snapshot.name; roles = snapshot.roles; widgets = snapshot.widgets;
    chosen = [...catalogs.filter((item) => item.version === null), ...snapshot.catalogs.map((pin) => catalogs.find((item) => keyOf(item) === keyOf(pin))).filter((item): item is EditorCatalog => item !== undefined)];
    selected = []; widgetTitle = ''; editingWidget = null; roleId = ''; slotLines = ''; editingRole = null;
    selectedWidgetIds = selectedWidgetIds.filter((id) => snapshot.widgets.some((widget) => widget.widget_id === id));
    syncHistory(); if (opened) autosaver.changed();
  }
  function undo(redo = false): void {
    if (busy || conflict || !canEdit || selected.length || widgetTitle.trim() || roleId.trim() || slotLines.trim()) return;
    commitChange('editor.op.edit_field');
    const snapshot = redo ? memoryHistory?.redo() : memoryHistory?.undo();
    if (snapshot) applySnapshot(snapshot);
    else if (opened && !dirty && (redo ? opened.reusable?.redo : opened.reusable?.undo)) {
      void moveReusableHistory(opened, redo);
    }
  }
  async function moveReusableHistory(page: OfflinePage, redo: boolean): Promise<void> {
    busy = true; error = false;
    try { const updated = await client.reusableHistory(page.local_id, page.revision, redo); hydrate(updated); onchanged?.(updated); }
    catch (failure) {
      if (failure instanceof ApiFailure && failure.response?.code === 'offline_page_edit_lost') editingSession.lost();
      else if (failure instanceof ApiFailure && failure.response?.code === 'offline_page_conflict') conflict = true;
      else error = true;
    }
    finally { busy = false; }
  }
  function titleOf(widget: NewOfflinePage['widgets'][number]): string {
    if (widget.kind !== 'preserved') return widget.title;
    return preservedTitles[widget.widget_id] ?? widget.widget_id;
  }
  function geometry(index: number, layout: FixedLayout, action: 'move' | 'resize'): void {
    if (!canEdit) return;
    widgets = widgets.map((widget, at) => at === index ? { ...widget, layout } : widget);
    commitChange(action === 'move' ? 'editor.op.move' : 'editor.op.resize');
  }

  const keyOf = (catalog: Pick<EditorCatalog, 'catalog_id' | 'version'>): string => `${catalog.catalog_id}/${catalog.version ?? 'embedded'}`;
  const metricFor = (choice: PageMetricChoice): CatalogMetric | undefined => {
    const catalog = chosen.find((item) => item.catalog_id === choice.catalog_id);
    return catalog ? metrics[keyOf(catalog)]?.find((metric) => metric.id === choice.metric_id) : undefined;
  };
  const graphReady = $derived(selected.length > 0 && selected.length <= 32 && selected.every((item) => {
    const metric = metricFor(item);
    return metric !== undefined && acceptsMetricType('graph', metric.value_type) && validSource(item.hosts, roles);
  }));
  const selectedItemMetric = $derived(selected.length === 1 ? metricFor(selected[0]) : undefined);
  const itemReady = $derived(selectedItemMetric !== undefined && acceptsMetricType('item_value', selectedItemMetric.value_type) && validSource(selected[0].hosts, roles));
  const singleReady = $derived(itemReady && singleSource(selected[0].hosts, roles));
  const repeatReady = $derived(itemReady && selected[0].hosts.selection === 'all');
  const selectedGraphRows = $derived(graphRowCount(selected, roles));
  const usesCatalog = (id: string): boolean => widgets.some((widget) => widget.kind === 'graph'
    ? widget.metrics.some((metric) => metric.catalog_id === id)
    : widget.kind === 'item_value' ? widget.metric.catalog_id === id : true);

  function hydrate(page: OfflinePage): void {
    preservedTitles = Object.fromEntries(page.bundle.project.root.widgets
      .filter((widget) => widget.type === 'zabbix')
      .map((widget) => [widget.id, portableWidgetTitle(widget)]));
    removalDependency = false;
    const content = pageRequestFromSaved(page);
    const embedded: EditorCatalog[] = (page.embedded_catalogs ?? []).map((catalog_id) => {
      const catalog = page.bundle.catalogs[catalog_id];
      if (!catalog) throw new Error('offline_page_missing_catalog');
      return { catalog_id, version: null, name: catalog.source.template_name ?? catalog_id, export_format_version: catalog.source.export_format_version ?? null, metrics: catalog.metrics.length, discovery_rules: 0, metrics_with_hints: 0, omitted_template_dashboards: 0, diagnostics: [] };
    });
    const pinned: EditorCatalog[] = [...savedCatalogChoices(page), ...embedded];
    opened = page;
    name = content.name;
    roles = content.roles;
    widgets = content.widgets;
    chosen = pinned;
    catalogs = [...catalogs.filter((catalog) => !pinned.some((pin) => pin.catalog_id === catalog.catalog_id)), ...pinned];
    metrics = { ...metrics, ...Object.fromEntries(pinned.map((pin) => [keyOf(pin), page.bundle.catalogs[pin.catalog_id].metrics])) };
    selected = []; widgetTitle = ''; editingWidget = null;
    roleId = ''; slotLines = ''; editingRole = null;
    baseline = canonicalEditorState(request());
    selectedWidgetIds = [];
    memoryHistory = new EditorHistory($state.snapshot(request())); syncHistory();
  }

  function refreshEditorSettings(): void {
    const request = ++settingsRequest;
    void createEditorSettingsClient().load().then((settings) => {
      if (request !== settingsRequest) return;
      autosaveEnabled = settings.values.autosave === 'on';
      autosaveMaxInterval = settings.values.autosave_max_interval_s;
      defaultZoom = settings.values.default_zoom;
      showGrid = settings.values.show_grid;
      autosaver.setMaxInterval(autosaveMaxInterval);
      autosaver.setEnabled(autosaveEnabled && canEdit);
    }).catch(() => { /* Defaults remain active when settings cannot be read. */ });
  }
  onMount(() => {
    refreshEditorSettings();
    clipboardContent = clipboard.read();
    try { if (initialPage) hydrate(initialPage); else { baseline = canonicalEditorState(request()); memoryHistory = new EditorHistory($state.snapshot(request())); } } catch { error = true; }
    void client.list().then((items) => catalogs = [...chosen, ...items.filter((item) => !chosen.some((pin) => pin.catalog_id === item.catalog_id))]).catch(() => loadWarning = true);
    if (initialPage) void claimEditor();
    const currentPath = location.pathname;
    let bypass = false;
    const leave = async (): Promise<boolean> => {
      if (autosaveEnabled && opened && modelDirty) await autosaver.flush();
      return !dirty || window.confirm(i18n.t('offline_builder.leave_dirty'));
    };
    const beforeUnload = (event: BeforeUnloadEvent): void => {
      if (autosaveEnabled && opened && modelDirty) void autosaver.flush();
      if (dirty || busy) { event.preventDefault(); event.returnValue = ''; }
    };
    const clicked = (event: MouseEvent): void => {
      const anchor = event.target instanceof Element ? event.target.closest('a') : null;
      if (!anchor) return;
      const path = internalLinkPath(anchor, location.origin, event);
      const nextRoute = path === null ? null : parseRoute(path).route;
      if (nextRoute?.name === 'draft' && nextRoute.localId === opened?.local_id) return;
      if (bypass) { bypass = false; return; }
      if (path !== null && path !== location.pathname && (dirty || busy)) {
        event.preventDefault(); event.stopImmediatePropagation();
        void leave().then((allowed) => { if (allowed) { bypass = true; anchor.click(); } });
      }
    };
    const popped = (event: PopStateEvent): void => {
      if (bypass) { bypass = false; return; }
      const nextRoute = parseRoute(location.pathname).route;
      if (nextRoute.name === 'draft' && nextRoute.localId === opened?.local_id) return;
      if (dirty || busy) {
        const target = location.pathname; history.pushState(null, '', currentPath); event.stopImmediatePropagation();
        void leave().then((allowed) => { if (allowed) { bypass = true; history.replaceState(null, '', target); window.dispatchEvent(new PopStateEvent('popstate')); } });
      }
    };
    const hidden = (): void => {
      if (!document.hidden) { refreshEditorSettings(); if (opened) void editingSession.check(); }
      if (document.hidden && autosaveEnabled && opened && modelDirty) void autosaver.flush();
    };
    const keyboard = (event: KeyboardEvent): void => {
      if (!(event.ctrlKey || event.metaKey)) return;
      if (event.key.toLowerCase() === 's') { event.preventDefault(); void autosaver.flush(); return; }
      if (event.target instanceof Element && event.target.closest('input,textarea,[contenteditable="true"]')) return;
      if (view === 'edit' && ['c', 'x', 'v'].includes(event.key.toLowerCase())) {
        event.preventDefault(); if (event.key.toLowerCase() === 'v') openPaste(); else copySelection(event.key.toLowerCase() === 'x'); return;
      }
      if (event.key.toLowerCase() === 'z' || event.key.toLowerCase() === 'y') { event.preventDefault(); undo(event.shiftKey || event.key.toLowerCase() === 'y'); }
    };
    window.addEventListener('beforeunload', beforeUnload);
    window.addEventListener('popstate', popped, true);
    window.addEventListener('focus', refreshEditorSettings);
    document.addEventListener('click', clicked, true);
    document.addEventListener('visibilitychange', hidden);
    document.addEventListener('keydown', keyboard);
    return () => {
      settingsRequest++;
      window.removeEventListener('beforeunload', beforeUnload);
      window.removeEventListener('popstate', popped, true);
      window.removeEventListener('focus', refreshEditorSettings);
      document.removeEventListener('click', clicked, true);
      document.removeEventListener('visibilitychange', hidden); document.removeEventListener('keydown', keyboard); autosaver.dispose(); editingSession.dispose();
    };
  });

  async function toggleCatalog(catalog: EditorCatalog, checked: boolean): Promise<void> {
    if (catalog.version === null) return;
    if (busy) return;
    error = false;
    catalogInUse = false;
    if (usesCatalog(catalog.catalog_id)) { catalogInUse = true; return; }
    const previous = chosen;
    chosen = checked
      ? [...chosen.filter((item) => item.catalog_id !== catalog.catalog_id), catalog]
      : chosen.filter((item) => keyOf(item) !== keyOf(catalog));
    selected = selected.filter((item) => item.catalog_id !== catalog.catalog_id);
    if (checked && !metrics[keyOf(catalog)]) {
      busy = true;
      try {
        const detail = await client.catalog(catalog.catalog_id, catalog.version);
        metrics = { ...metrics, [keyOf(catalog)]: detail.metrics };
      } catch {
        chosen = previous;
        error = true;
      } finally { busy = false; }
    }
    commitChange('editor.op.bindings');
  }

  function toggleMetric(choice: PageMetricChoice, checked: boolean): void {
    const role = Object.keys(roles)[0];
    if (!role) return;
    selected = checked
      ? [...selected, { ...choice, hosts: { role, selection: roles[role].cardinality === 'one' ? 'one' : 'all' } }]
      : selected.filter((item) => item.catalog_id !== choice.catalog_id || item.metric_id !== choice.metric_id);
  }

  function addRole(): void {
    const id = roleId.trim();
    const others = Object.fromEntries(Object.entries(roles).filter(([key]) => key !== editingRole));
    roleError = !validNewRole(id, newRole, others);
    if (roleError) return;
    const next = { ...roles, [id]: { ...newRole, ...(roles[id]?.description ? { description: roles[id].description } : {}) } };
    roleInUse = widgets.some((widget) => widget.kind === 'graph'
      ? widget.metrics.some((metric) => !validSource(metric.hosts, next))
      : widget.kind === 'item_value' ? (widget.expansion === 'single' ? !singleSource(widget.hosts, next) : !validSource(widget.hosts, next)) : false);
    if (roleInUse) return;
    roles = next;
    roleId = '';
    slotLines = '';
    editingRole = null;
    commitChange('editor.op.bindings');
  }

  function editRole(id: string): void {
    editingRole = id; roleId = id; cardinality = roles[id].cardinality; slotLines = (roles[id].slots ?? []).join('\n');
    void tick().then(() => document.getElementById('offline-role-slots')?.focus());
  }

  function removeRole(id: string): void {
    roleInUse = widgets.some((widget) => widget.kind === 'graph'
      ? widget.metrics.some((metric) => metric.hosts.role === id) : widget.kind === 'item_value' ? widget.hosts.role === id : true);
    if (roleInUse) return;
    roles = Object.fromEntries(Object.entries(roles).filter(([key]) => key !== id));
    selected = selected.filter((metric) => metric.hosts.role !== id);
    commitChange('editor.op.bindings');
  }

  function setSource(index: number, hosts: PageHostSelection): void {
    selected = selected.map((metric, at) => at === index ? { ...metric, hosts } : metric);
  }

  function setRole(index: number, role: string): void {
    setSource(index, { role, selection: roles[role].cardinality === 'one' ? 'one' : 'all' });
  }

  function setSelection(index: number, selection: PageHostSelection['selection']): void {
    setSource(index, { role: selected[index].hosts.role, selection, ...(selection === 'slot' ? { slot_id: '' } : {}) });
  }

  function addGraph(): void {
    if (!widgetTitle.trim() || !graphReady) return;
    applyWidget({ kind: 'graph', title: widgetTitle.trim(), metrics: structuredClone($state.snapshot(selected)) });
    widgetTitle = '';
    selected = [];
  }

  function applyWidget(widget: NewOfflinePage['widgets'][number]): void {
    const previous = editingWidget === null ? null : widgets[editingWidget];
    const replacement = { ...widget, widget_id: previous?.widget_id ?? `widget-${crypto.randomUUID().replaceAll('-', '')}`,
      layout: previous?.layout ?? { mode: 'auto' as const, w: widget.kind === 'graph' ? 36 : 12, h: 5 } };
    widgets = editingWidget === null ? [...widgets, replacement] : widgets.map((value, index) => index === editingWidget ? replacement : value);
    editingWidget = null;
    commitChange(previous ? 'editor.op.edit_field' : 'editor.op.add_widget');
  }

  function editWidget(index: number): void {
    const widget = widgets[index];
    if (widget.kind === 'preserved') return;
    editingWidget = index; widgetTitle = widget.title;
    selected = structuredClone($state.snapshot(widget.kind === 'graph' ? widget.metrics : [{ ...widget.metric, hosts: widget.hosts }]));
    void tick().then(() => document.getElementById('offline-widget-title')?.focus());
  }

  function removeWidget(index: number): void {
    const widget = widgets[index];
    if (widget.kind === 'preserved') {
      if (!busy && canEdit && !conflict) removalSelection = [widget.widget_id];
      return;
    }
    widgets = widgets.filter((_, at) => at !== index);
    if (editingWidget === index) { editingWidget = null; selected = []; widgetTitle = ''; }
    else if (editingWidget !== null && editingWidget > index) editingWidget--;
    commitChange('editor.op.remove_widget');
  }

  function duplicateWidget(index: number): void {
    const widget = widgets[index]; if (widget.kind === 'preserved') return;
    const copy = structuredClone($state.snapshot(widget));
    copy.widget_id = `widget-${crypto.randomUUID().replaceAll('-', '')}`;
    copy.layout = { mode: 'auto', w: widget.layout?.w ?? 12, h: widget.layout?.h ?? 5 };
    widgets = [...widgets, copy]; commitChange('editor.op.add_widget');
  }

  function addItem(repeat: boolean): void {
    if (!widgetTitle.trim() || !(repeat ? repeatReady : singleReady)) return;
    const { catalog_id, metric_id, hosts } = selected[0];
    applyWidget({ kind: 'item_value', title: widgetTitle.trim(), metric: { catalog_id, metric_id }, hosts: { ...hosts }, expansion: repeat ? 'widget_per_host' : 'single' });
    widgetTitle = '';
    selected = [];
  }

  async function savePage(): Promise<void> {
    if (!canEdit) throw new ApiFailure(409, { code: 'offline_page_edit_lost', params: {} });
    if (!name.trim() || (!opened && widgets.length === 0) || conflict) throw new Error('offline_page_invalid');
    name = name.trim(); commitChange('editor.op.page');
    busy = true;
    savingPage = true;
    error = false;
    removalDependency = false;
    saved = false;
    try {
      const content = $state.snapshot({ ...request(), name: name.trim() });
      const before = opened;
      const page = before
        ? await saveKnownPage(client, before, content, before.bundle.project.root.widgets
          .filter((widget) => widget.type === 'zabbix' && !content.widgets.some((entry) => entry.kind === 'preserved' && entry.widget_id === widget.id))
          .map((widget) => widget.id))
        : await client.createPage({ ...content, evaluation_profile: evaluationProfile });
      opened = page; baseline = canonicalEditorState(content); retrySeconds = null;
      if (!before) { memoryHistory = new EditorHistory($state.snapshot(content)); syncHistory(); await editingSession.open(page.local_id); }
      saved = true;
      onchanged?.(page);
    } catch (failure) {
      conflict = failure instanceof ApiFailure && failure.response?.code === 'offline_page_conflict';
      removalDependency = failure instanceof ApiFailure && failure.response?.code === 'offline_page_widget_removal_dependency';
      error = !conflict && !removalDependency;
      throw failure;
    }
    finally { busy = false; savingPage = false; }
  }

  async function reloadSaved(): Promise<void> {
    if (!opened || (dirty && !window.confirm(i18n.t('offline_builder.leave_dirty')))) return;
    busy = true;
    autosaver.setEnabled(false);
    error = false;
    try { hydrate(await client.page(opened.local_id)); conflict = false; saved = false; }
    catch { error = true; }
    finally { busy = false; autosaver.setEnabled(autosaveEnabled && canEdit); }
  }

  async function exportPage(yaml = false): Promise<void> {
    if (!opened || dashboardId || dirty || busy || exporting) return;
    exporting = true; exportFailed = false; exportHash = null; exportEntries = null;
    try {
      const download = yaml ? await client.preparePageYamlExport(opened.local_id, opened.revision) : await client.preparePageExport(opened.local_id, opened.revision);
      exportHash = download.sha256;
      exportEntries = Object.keys(download.entry_hashes).length ? download.entry_hashes : null;
      const link = document.createElement('a');
      link.href = `/api/downloads/${encodeURIComponent(download.id)}`;
      link.download = download.display_name;
      document.body.append(link); link.click(); link.remove();
    } catch { exportFailed = true; }
    finally { exporting = false; }
  }
  let exportPath = $state<string | null>(null);
  async function savePageExport(yaml: boolean, jobId: string): Promise<{ cancelled: boolean }> {
    if (!opened || dashboardId || dirty || busy || exporting) return { cancelled: true };
    exporting = true; exportFailed = false; exportPath = null; exportHash = null; exportEntries = null;
    try {
      const result = await client.saveExport('page', opened.local_id, opened.revision, yaml, jobId);
      if (!result.cancelled) { exportPath = result.path ?? null; exportHash = result.sha256 ?? null; exportEntries = result.entry_hashes && Object.keys(result.entry_hashes).length ? result.entry_hashes : null; }
      return result;
    } catch (error) { exportFailed = true; throw error; }
    finally { exporting = false; }
  }
</script>

<section class="card offline-builder" aria-labelledby="offline-builder-title">
  <h2 id="offline-builder-title">{i18n.t(opened ? 'offline_builder.edit_title' : 'offline_builder.title')}</h2>
  <p class="muted">{i18n.t('offline_builder.help')}</p>
  {#if savedEvaluation}
    <p>{savedEvaluation.profile === null
      ? i18n.t('offline_builder.profile_unavailable')
      : i18n.t('offline_builder.profile_current', { version: savedEvaluation.profile })}</p>
  {:else}
    <label for="offline-evaluation-profile">{i18n.t('offline_builder.profile')}</label>
    <select id="offline-evaluation-profile" bind:value={evaluationProfile} disabled={busy}>
      <option value="8.0">{i18n.t('offline_builder.profile_choice', { version: '8.0' })}</option>
      <option value="7.4">{i18n.t('offline_builder.profile_choice', { version: '7.4' })}</option>
    </select>
  {/if}
  <p class="muted">{i18n.t('offline_builder.profile_help')}</p>
  <p class="muted">{i18n.t('offline_editor.scope')}</p>
  <p class="muted">{i18n.t('offline_editor.history_hint')}</p>
  {#if opened && !canEdit}
    <p role="status">{i18n.t(editAccess === 'claiming' ? 'offline_editor.claiming' : editAccess === 'read_only' ? 'offline_editor.open_elsewhere' : 'offline_editor.owner_unavailable')}</p>
    <button type="button" disabled={busy || editAccess === 'claiming'} onclick={() => void claimEditor(true)}>{i18n.t('offline_editor.edit_here')}</button>
    {#if dirty}<label for="editor-unsaved-copy">{i18n.t('offline_editor.unsaved_copy')}</label><textarea id="editor-unsaved-copy" readonly rows="6" value={JSON.stringify({ page: request(), widget_input: { title: widgetTitle, metrics: selected }, role_input: { id: roleId, cardinality, slots: slotLines } }, null, 2)}></textarea>{/if}
  {/if}
  {#if opened?.recovery?.may_have_unsaved_changes}<p role="alert">{i18n.t('offline_editor.recovery', { time: opened.recovery.last_saved_at_ms })}</p>{/if}
  <div class="actions">
    <button type="button" disabled={busy || !canEdit || conflict || (undoAction === null && (dirty || !opened?.reusable?.undo)) || pendingInput} onclick={() => undo()}>{i18n.t('editor.action.undo', { action: undoAction ? i18n.t(undoAction, { field: i18n.t('offline_builder.widgets') }) : i18n.t(opened?.reusable?.undo ? 'reusable.history_action' : 'offline_editor.no_history') })}</button>
    <button type="button" disabled={busy || !canEdit || conflict || (redoAction === null && (dirty || !opened?.reusable?.redo)) || pendingInput} onclick={() => undo(true)}>{i18n.t('editor.action.redo', { action: redoAction ? i18n.t(redoAction, { field: i18n.t('offline_builder.widgets') }) : i18n.t(opened?.reusable?.redo ? 'reusable.history_action' : 'offline_editor.no_history') })}</button>
  </div>
  {#if historyTruncated}<p>{i18n.t('offline_editor.history_limit')}</p>{/if}
  <p class="muted">{i18n.t('settings.autosave.title')}: {i18n.t(autosaveEnabled ? 'settings.editor.on' : 'settings.editor.off')} · <a href={settingsPath('editor')}>{i18n.t('settings.editor.title')}</a></p>
  {#if autosaveEnabled && opened}<p class="muted">{i18n.t('settings.autosave.max_interval', { seconds: autosaveMaxInterval })}</p>{/if}
  {#if autosaveEnabled && !opened}<p class="muted">{i18n.t('settings.editor.autosave_after_create')}</p>{/if}
  {#if !autosaveEnabled}<p class="muted">{i18n.t('settings.autosave.off_warning')}</p>{/if}
  {#if retrySeconds !== null}<p role="alert">{i18n.t('offline_editor.retry', { seconds: retrySeconds })}</p>{/if}
  <p role="status">{i18n.t(savingPage ? 'offline_builder.saving' : dirty ? 'offline_builder.unsaved' : opened ? 'offline_builder.saved' : 'offline_builder.new_page')}</p>
  {#if loadWarning}<p role="status">{i18n.t('offline_builder.library_unavailable')}</p>{/if}
  {#if conflict}<p role="alert">{i18n.t('offline_builder.conflict')}</p>{/if}
  {#if error}<p role="alert">{i18n.t('offline_builder.error')}</p>{/if}
  {#if removalDependency}<p role="alert">{i18n.t('offline_editor.removal_dependency')}</p>{/if}
  {#if catalogInUse}<p role="alert">{i18n.t('offline_builder.catalog_in_use')}</p>{/if}
  {#if saved}<p role="status">{i18n.t('offline_builder.saved')}</p>{/if}
  {#if opened}
    {#if !dashboardId}
      <PortablePatternReview {i18n} draft={opened} enabled={!dirty && !busy} />
      <PortableLldReview {i18n} draft={opened} enabled={!dirty && !busy} />
      <PortablePublication {i18n} draft={opened} enabled={!dirty && !busy} {active} {profile} />
      <SensitivePreview {i18n} kind="page" id={opened.local_id} revision={opened.revision} enabled={!dirty && !busy} onFinding={(location) => { redactionJump = { location, sequence: (redactionJump?.sequence ?? 0) + 1 }; }} />
      <RedactionPreview {i18n} kind="page" id={opened.local_id} revision={opened.revision} enabled={!dirty && !busy} pages={[opened.bundle.project.root]} roles={opened.bundle.project.roles} catalogs={opened.bundle.catalogs} jump={redactionJump} />
      <button type="button" disabled={busy || dirty || exporting} onclick={() => void exportPage()}>{i18n.t('offline_builder.export')}</button>
      <SaveAsButton {i18n} messageId="redaction.save_zip_as" disabled={busy || dirty || exporting} run={(job) => savePageExport(false, job)} onDone={() => {}} onError={() => { exportFailed = true; }} />
      {#if opened.bundle.project.catalogs.length === 0}<button type="button" disabled={busy || dirty || exporting} onclick={() => void exportPage(true)}>{i18n.t('package_import.export_yaml')}</button>{/if}
      {#if opened.bundle.project.catalogs.length === 0}<SaveAsButton {i18n} messageId="redaction.save_yaml_as" disabled={busy || dirty || exporting} run={(job) => savePageExport(true, job)} onDone={() => {}} onError={() => { exportFailed = true; }} />{/if}
      <p class="muted">{i18n.t('redaction.save_no_overwrite')}</p>
      {#if exportPath}<p role="status">{i18n.t('redaction.saved')}: <bdi>{exportPath}</bdi></p>{/if}
      <p class="muted">{i18n.t('offline_dashboard.export_caution')}</p>
      {#if exportHash}<p role="status">{i18n.t('package_import.export_hash')}: <code>{exportHash}</code></p>{/if}
      {#if exportEntries}<details><summary>{i18n.t('package_import.file_hashes')}</summary><ul>{#each Object.entries(exportEntries) as [name, hash] (name)}<li><bdi>{name}</bdi>: <code>{hash}</code></li>{/each}</ul></details>{/if}
      {#if exportFailed}<p role="alert">{i18n.t('offline_builder.export_failed')}</p>{/if}
    {/if}
    <button type="button" disabled={busy || dirty || librarySaving} onclick={async () => {
      if (!opened) return;
      librarySaveFailed = false;
      librarySaved = false;
      librarySaving = true;
      try {
        if (dashboardId) await client.saveDashboardPageToLibrary(dashboardId, opened.local_id);
        else await client.savePageToLibrary(opened.local_id);
        librarySaved = true;
      }
      catch { librarySaveFailed = true; }
      finally { librarySaving = false; }
    }}>{i18n.t('page_library.save_action')}</button>
    {#if librarySaved}<p role="status">{i18n.t('page_library.saved')}</p>{/if}
    {#if librarySaveFailed}<p role="alert">{i18n.t('page_library.save_failed')}</p>{/if}
  {/if}
  {#if view === 'checks'}
    <section aria-labelledby="local-checks-title">
      <h3 id="local-checks-title">{i18n.t('draft.nav.checks')}</h3>
      <p>{i18n.t('offline_editor.checks_help')}</p>
      <p role="status">{i18n.t('offline_editor.finding_count', { count: findings.length })}</p>
      {#if findings.some((finding) => finding.blocks.includes('export'))}<p>{i18n.t('checks.summary.export_blocked', { count: findings.filter((finding) => finding.blocks.includes('export')).length })}</p>{/if}
      {#if findings.some((finding) => finding.blocks.includes('publication'))}<p>{i18n.t('checks.summary.publication_blocked', { count: findings.filter((finding) => finding.blocks.includes('publication')).length })}</p>{/if}
      <ul>{#each findings as finding (finding.id)}<li><code>{finding.code}</code> · {i18n.t(finding.blocks.length === 2 ? 'checks.level.blocks_both' : finding.blocks.includes('export') ? 'checks.level.blocks_export' : finding.blocks.includes('publication') ? 'checks.level.blocks_publication' : 'checks.level.warning')} · {resolveText(i18n, finding.message)}
        {#if opened}<a href={pagePath(opened.local_id)} onclick={() => { selectedWidgetIds = [...finding.widget_ids]; findingFocus = finding.widget_ids[0] ?? ''; }}>{i18n.t('offline_editor.resolve.go_to_object')}</a>{/if}
      </li>{/each}</ul>
    </section>
  {:else}
  <div class="actions">
    <button type="button" disabled={busy || pendingInput || selectedWidgetIds.length === 0} onclick={() => copySelection(false)}>{i18n.t('offline_editor.clipboard.copy')}</button>
    <button type="button" disabled={busy || !canEdit || conflict || pendingInput || selectedWidgetIds.length === 0} onclick={() => copySelection(true)}>{i18n.t('offline_editor.clipboard.cut')}</button>
    <button type="button" id="editor-paste" disabled={busy || !canEdit || conflict || pendingInput || clipboardContent === null} onclick={openPaste}>{i18n.t('offline_editor.clipboard.paste')}</button>
    <button type="button" disabled={busy || !canEdit || conflict || pendingInput || selectedWidgetIds.length === 0} onclick={() => removeSelection(selectedWidgetIds)}>{i18n.t('offline_editor.remove_selection')}</button>
  </div>
  <p class="muted">{i18n.t('offline_editor.selection_help')}</p>
  {#if clipboardContent}<p role="status">{i18n.t(clipboardContent.cut ? 'offline_editor.clipboard.cut_pending' : 'offline_editor.clipboard.copied', { count: clipboardContent.widgets.length })}</p>{/if}
  {#each clipboardIssues as issue, index (index)}<p role="alert">{resolveText(i18n, clipboardMessage(issue))}</p>{/each}
  <label for="offline-page-name">{i18n.t('offline_builder.page_name')}</label>
  <input id="offline-page-name" maxlength="200" disabled={busy} readonly={!canEdit} bind:value={name}
    oninput={() => { void tick().then(() => { if (opened) autosaver.changed(); }); }}
    onblur={() => commitChange('editor.op.page')} onkeydown={(event) => { if (event.key === 'Enter') commitChange('editor.op.page'); }} />
  {#if opened}
    <ReusablePageActions {i18n} {client} page={opened} {selectedWidgetIds} allowDashboardScope={dashboardId !== undefined} disabled={dirty || busy || conflict || !canEdit} onpending={(pending) => { busy = pending; }} onupdated={(page) => { hydrate(page); onchanged?.(page); }} />
  {/if}
  {#if opened || widgets.length > 0}<EditorCanvas {i18n} {widgets} {defaultZoom} {showGrid} titles={widgets.map(titleOf)} {findings} selectedIds={selectedWidgetIds} onselection={(ids) => selectedWidgetIds = ids} ongroupmove={groupGeometry} ongroupremove={removeSelection} disabled={busy || conflict || !canEdit} onchange={geometry} onremove={removeWidget} onduplicate={duplicateWidget} />{/if}
  <fieldset class="editor-fields" disabled={!canEdit}>
  <fieldset disabled={busy || dashboardId !== undefined}>
    <legend>{i18n.t('offline_builder.roles')}</legend>
    <p class="muted">{i18n.t('offline_builder.roles_help')}</p>
    {#if roleError}<p role="alert">{i18n.t('offline_builder.role_invalid')}</p>{/if}
    {#if roleInUse}<p role="alert">{i18n.t('offline_builder.role_in_use')}</p>{/if}
    <ul>{#each Object.entries(roles) as [id, role] (id)}<li>
      <bdi>{id}</bdi> · {i18n.t(role.cardinality === 'one' ? 'offline_builder.role_one' : 'offline_builder.role_many')}
      <ol>{#each role.slots ?? [] as slot (slot)}<li><bdi>{slot}</bdi></li>{/each}</ol>
      <button type="button" onclick={() => editRole(id)}>{i18n.t('offline_builder.edit_role', { role: id })}</button>
      <button type="button" onclick={() => removeRole(id)}>{i18n.t('offline_builder.remove_role', { role: id })}</button>
    </li>{/each}</ul>
    <label for="offline-role-id">{i18n.t('offline_builder.role_id')}</label>
    <input id="offline-role-id" maxlength="64" disabled={editingRole !== null} bind:value={roleId} />
    <label for="offline-role-cardinality">{i18n.t('offline_builder.cardinality')}</label>
    <select id="offline-role-cardinality" bind:value={cardinality}>
      <option value="one">{i18n.t('offline_builder.role_one')}</option>
      <option value="many">{i18n.t('offline_builder.role_many')}</option>
    </select>
    <label for="offline-role-slots">{i18n.t('offline_builder.slots')}</label>
    <textarea id="offline-role-slots" rows="3" maxlength="16640" bind:value={slotLines}></textarea>
    <button type="button" onclick={addRole}>{i18n.t(editingRole ? 'offline_builder.apply_role' : 'offline_builder.add_role')}</button>
    {#if roleId || slotLines}<button type="button" onclick={() => { editingRole = null; roleId = ''; slotLines = ''; roleError = false; roleInUse = false; }}>{i18n.t('common.action.cancel')}</button>{/if}
  </fieldset>
  <fieldset>
    <legend>{i18n.t('offline_builder.choose_catalogs')}</legend>
    {#each catalogs as catalog (keyOf(catalog))}
      <label class="builder-choice"><input type="checkbox" disabled={busy || dashboardId !== undefined || catalog.version === null} checked={chosen.some((item) => keyOf(item) === keyOf(catalog))}
        onchange={(event) => void toggleCatalog(catalog, event.currentTarget.checked)} />
        <bdi>{catalog.name}</bdi> · {catalog.version === null ? i18n.t('offline_builder.embedded_catalog') : i18n.t('offline_builder.version', { version: catalog.version })}
      </label>
    {/each}
  </fieldset>
  {#if chosen.length > 0}
    <fieldset>
      <legend>{i18n.t('offline_builder.choose_metrics')}</legend>
      {#each chosen as catalog (keyOf(catalog))}
        <h3><bdi>{catalog.name}</bdi></h3>
        {#each metrics[keyOf(catalog)] ?? [] as metric (metric.id)}
          <label class="builder-choice"><input type="checkbox" disabled={busy || Object.keys(roles).length === 0}
            checked={selected.some((item) => item.catalog_id === catalog.catalog_id && item.metric_id === metric.id)}
            onchange={(event) => toggleMetric({ catalog_id: catalog.catalog_id, metric_id: metric.id }, event.currentTarget.checked)} />
            <bdi>{metric.name}</bdi> <small><bdi>{metric.id}</bdi></small>
          </label>
        {/each}
      {/each}
    </fieldset>
    {#each selected as metric, index (`${index}/${metric.catalog_id}/${metric.metric_id}`)}
      <fieldset disabled={busy}>
        <legend>{i18n.t('offline_builder.metric_source', { metric: metricFor(metric)?.name ?? metric.metric_id })}</legend>
        <label for={`offline-source-role-${index}`}>{i18n.t('offline_builder.source_role')}</label>
        <select id={`offline-source-role-${index}`} value={metric.hosts.role} onchange={(event) => setRole(index, event.currentTarget.value)}>
          {#each Object.keys(roles) as role}<option value={role}>{role}</option>{/each}
        </select>
        <label for={`offline-source-selection-${index}`}>{i18n.t('offline_builder.selection')}</label>
        <select id={`offline-source-selection-${index}`} value={metric.hosts.selection} onchange={(event) => setSelection(index, event.currentTarget.value as PageHostSelection['selection'])}>
          {#if roles[metric.hosts.role]?.cardinality === 'one'}<option value="one">{i18n.t('offline_builder.select_one')}</option>{/if}
          <option value="all">{i18n.t('offline_builder.select_all')}</option>
          <option value="slot">{i18n.t('offline_builder.select_slot')}</option>
        </select>
        {#if metric.hosts.selection === 'slot'}
          <label for={`offline-source-slot-${index}`}>{i18n.t('offline_builder.slot')}</label>
          <select id={`offline-source-slot-${index}`} value={metric.hosts.slot_id ?? ''} onchange={(event) => setSource(index, { ...metric.hosts, slot_id: event.currentTarget.value })}>
            <option value="">{i18n.t('offline_builder.choose_slot')}</option>
            {#each roles[metric.hosts.role]?.slots ?? [] as slot}<option value={slot}>{slot}</option>{/each}
          </select>
        {/if}
      </fieldset>
    {/each}
    <label for="offline-widget-title">{i18n.t('offline_builder.widget_title')}</label>
    <input id="offline-widget-title" maxlength="200" disabled={busy} bind:value={widgetTitle} />
    {#if selected.length > 0}<p role="status">{i18n.t('offline_builder.graph_rows', { count: selectedGraphRows })}</p>{/if}
    <div class="actions">
      <button type="button" disabled={busy || !widgetTitle.trim() || !graphReady}
        onclick={addGraph}>{i18n.t(editingWidget === null ? 'offline_builder.add_graph' : 'offline_builder.apply_graph')}</button>
      <button type="button" disabled={busy || !widgetTitle.trim() || !singleReady}
        onclick={() => addItem(false)}>{i18n.t(editingWidget === null ? 'offline_builder.add_item' : 'offline_builder.apply_item')}</button>
      <button type="button" disabled={busy || !widgetTitle.trim() || !repeatReady}
        onclick={() => addItem(true)}>{i18n.t(editingWidget === null ? 'offline_builder.add_repeat' : 'offline_builder.apply_repeat')}</button>
      {#if selected.length || widgetTitle}<button type="button" disabled={busy} onclick={() => { editingWidget = null; selected = []; widgetTitle = ''; }}>{i18n.t('common.action.cancel')}</button>{/if}
    </div>
  {/if}
  {#if widgets.length > 0}
    <h3>{i18n.t('offline_builder.widgets')}</h3>
    <ol>{#each widgets as widget, index}<li><bdi>{titleOf(widget)}</bdi>
      {#if widget.kind === 'graph'}<span> · {i18n.t('offline_builder.graph_rows', { count: graphRowCount(widget.metrics, roles) })}</span>{/if}
      <ul>{#each findings.filter((finding) => finding.widget_ids.includes(widget.widget_id ?? '')) as finding (finding.id)}<li><code>{finding.code}</code> · {resolveText(i18n, finding.message)}</li>{/each}</ul>
      {#if widget.kind === 'item_value' && widget.expansion === 'widget_per_host'}<span>{i18n.t('offline_builder.repeated')}</span>{/if}
      {#if widget.kind === 'preserved'}<p>{i18n.t('offline_editor.opaque')}</p>{:else}
        <button type="button" disabled={busy || conflict} onclick={() => editWidget(index)}>{i18n.t('offline_builder.edit_widget', { widget: titleOf(widget) })}</button>
        <button type="button" disabled={busy || conflict} onclick={() => duplicateWidget(index)}>{i18n.t('common.action.duplicate')}</button>
      {/if}
      <button type="button" disabled={busy || conflict} onclick={() => removeWidget(index)}>{i18n.t('offline_builder.remove')}</button>
      </li>{/each}</ol>
  {/if}
  {#if selected.length || widgetTitle.trim() || roleId.trim() || slotLines.trim()}<p>{i18n.t('offline_builder.pending_help')}</p>{/if}
  </fieldset>
  <button type="button" disabled={busy || conflict || !canEdit || !modelDirty || !name.trim() || (!opened && widgets.length === 0)} onclick={() => void autosaver.flush()}>{i18n.t('offline_builder.save')}</button>
  {#if opened}
    <button type="button" disabled={busy} onclick={() => void reloadSaved()}>{i18n.t('offline_builder.reload')}</button>
    <a href={dashboardId ? draftPath(dashboardId) : draftPath(opened.local_id)}>{i18n.t(dashboardId ? 'offline_dashboard.back' : 'offline_builder.open_draft')}</a>
  {/if}
  {#if opened}
    <div class="builder-opened">
      <h3><bdi>{opened.bundle.project.root.name}</bdi></h3>
      <p class="muted">{i18n.t('offline_builder.saved_composition')}</p>
      <p>{i18n.t('offline_builder.opened', { catalogs: opened.bundle.project.catalogs.length, widgets: opened.bundle.project.root.widgets.length })}</p>
      <p class="muted">{i18n.t('offline_builder.logical_help')}</p>
      {#if opened.composition.slots_declared}
        <p>{i18n.t('offline_builder.logical_counts', { widgets: opened.composition.instance_count, sources: opened.composition.source_count })}</p>
        {#if opened.composition.truncated}<p>{i18n.t('offline_builder.logical_truncated')}</p>{/if}
        <ul>{#each opened.composition.instances as instance (`${instance.widget_id}/${instance.slot_id ?? ''}`)}<li>
          <bdi>{(() => { const widget = opened.bundle.project.root.widgets.find((widget) => widget.id === instance.widget_id); return widget ? portableWidgetTitle(widget) : instance.widget_id; })()}</bdi>
          {#if instance.slot_id}<span> · <bdi>{instance.slot_id}</bdi></span>{/if}
          <ul>{#each instance.sources as source}<li><bdi>{source.role}</bdi> · <bdi>{source.slot_id}</bdi> · <bdi>{source.metric_id}</bdi></li>{/each}</ul>
        </li>{/each}</ul>
      {:else}<p>{i18n.t('offline_builder.slots_missing')}</p>{/if}
      <ol>{#each opened.bundle.project.root.widgets as widget}<li><bdi>{portableWidgetTitle(widget)}</bdi></li>{/each}</ol>
    </div>
  {/if}
  {/if}
  <Dialog {i18n} id="editor-remove-widgets-dialog" title={{ id: 'offline_editor.confirm_removal' }} description={{ id: 'offline_editor.confirm_removal_help' }} open={removalSelection.length > 0} onclose={() => removalSelection = []}>
    <ul>{#each widgets.filter((widget) => removalSelection.includes(widget.widget_id ?? '')) as widget}<li><bdi>{titleOf(widget)}</bdi></li>{/each}</ul>
    <button type="button" onclick={() => removalSelection = []}>{i18n.t('common.action.cancel')}</button>
    <button type="button" disabled={busy || !canEdit || conflict} onclick={confirmRemoval}>{i18n.t('offline_builder.remove')}</button>
  </Dialog>
  <Dialog {i18n} id="editor-paste-dialog" title={{ id: 'offline_editor.clipboard.paste' }} description={{ id: 'offline_editor.clipboard.resolve_help' }} open={pasteOpen} onclose={() => void closePaste()}>
    {#snippet children()}
      <fieldset><legend>{i18n.t('offline_editor.clipboard.placement')}</legend>
        <label><input type="radio" name="paste-placement" value="original" bind:group={pastePlacement} />{i18n.t('offline_editor.clipboard.original')}</label>
        <label><input type="radio" name="paste-placement" value="first_free" bind:group={pastePlacement} />{i18n.t('offline_editor.clipboard.first_free')}</label>
      </fieldset>
      <p>{i18n.t('offline_editor.clipboard.resolve_sources')}</p>
      {#if clipboardContent && !clipboardContent.cut}<PasteResolutionForm {i18n} content={clipboardContent} target={clipboardContext()} resolution={pasteResolution} onchange={(value) => pasteResolution = value} />{/if}
      <ul>{#each pastePlan.issues as issue, index (index)}<li>{resolveText(i18n, clipboardMessage(issue))}</li>{/each}</ul>
      <ol>{#each pastePlan.widgets as widget, index (index)}<li><bdi>{widget.title}</bdi>{#if widget.layout?.mode === 'fixed'} · {i18n.t('offline_editor.box', { x: widget.layout.x, y: widget.layout.y, w: widget.layout.w, h: widget.layout.h })}{/if}</li>{/each}</ol>
    {/snippet}
    {#snippet actions()}
      <button type="button" disabled={!canEdit || busy || pastePlan.issues.length > 0} onclick={paste}>{i18n.t('offline_editor.clipboard.apply')}</button>
      <button type="button" onclick={() => void closePaste()}>{i18n.t('common.action.cancel')}</button>
    {/snippet}
  </Dialog>
</section>

<style>
  .offline-builder { display: grid; gap: .9rem; }
  .offline-builder fieldset { border: 1px solid var(--border-subtle); border-radius: .5rem; max-height: 18rem; overflow: auto; }
  .offline-builder .editor-fields { border: 0; padding: 0; max-height: none; overflow: visible; display: grid; gap: .9rem; }
  .builder-choice { display: block; padding: .35rem 0; }
  .builder-choice input { margin-right: .5rem; }
  .offline-builder input:not([type="checkbox"]) { max-width: 35rem; width: 100%; }
  .offline-builder select, .offline-builder textarea { display: block; max-width: 35rem; width: 100%; margin-bottom: .5rem; }
  .builder-opened { border-top: 1px solid var(--border-subtle); padding-top: .5rem; }
</style>
