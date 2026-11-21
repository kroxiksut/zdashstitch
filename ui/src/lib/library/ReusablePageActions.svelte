<script lang="ts">
  import { onMount } from 'svelte';
  import type { I18n } from '../i18n';
  import { Dialog } from '../components';
  import { portableWidgetTitle, reusableId, type BlueprintMetric, type BlueprintRole, type MetricAssignment, type OfflinePage, type ReusableAction, type ReusableEntry, type ReusableKind, type ReusablePreview, type RoleAssignment } from './client';
  import { displayReusableName, reusableError } from './reusable';

  type EditorClient = Pick<ReturnType<typeof import('./client').createCatalogClient>, 'reusable' | 'reusableAction' | 'extractReusable' | 'detachReusable'>;
  let { i18n, client, page, disabled, selectedWidgetIds, allowDashboardScope, onpending, onupdated }: {
    i18n: I18n;
    client: EditorClient;
    page: OfflinePage;
    disabled: boolean;
    selectedWidgetIds: string[];
    allowDashboardScope: boolean;
    onpending: (pending: boolean) => void;
    onupdated: (updated: OfflinePage) => void;
  } = $props();

  let kind = $state<ReusableKind | null>(null);
  let entries = $state<ReusableEntry[]>([]);
  let chosenId = $state('');
  let busy = $state(false);
  let message = $state('');
  let scope = $state<'page' | 'selected' | 'dashboard'>('page');
  let mode = $state<'replace' | 'keep_local' | 'merge'>('replace');
  let assignedRoles = $state<Record<string, RoleAssignment>>({});
  let assignedMetrics = $state<Record<string, string>>({});
  let checked = $state<{ action: ReusableAction; preview: ReusablePreview } | null>(null);
  let updated = $state(false);
  let extractionName = $state('');
  let extractionKind = $state<ReusableKind>('style_profile');
  let extractionMessage = $state('');
  let extractionSaved = $state(false);
  let extracting = $state(false);
  const matching = $derived(entries.filter((entry) => entry.asset.kind === kind && entry.asset.document.adapter === page.widget_evaluation?.profile));
  const selected = $derived(matching.find((entry) => reusableId(entry.asset) === chosenId));
  const blueprint = $derived(selected?.asset.kind === 'widget_blueprint' ? selected.asset.document : null);
  const roleOptions = $derived(Object.entries(page.bundle.project.roles));
  const pageWidgets = $derived(page.bundle.project.root.widgets);
  const mark = $derived(selectedWidgetIds.length === 1 ? page.reusable?.marks[selectedWidgetIds[0]] : undefined);
  const markAsset = $derived(mark && entries.find((entry) => reusableId(entry.asset) === mark.profile_id));
  const catalogOptions = $derived(Object.values(page.bundle.catalogs));
  const canReview = $derived(selected && !disabled && (kind === 'style_profile'
    ? (scope === 'dashboard' || (scope === 'page' ? pageWidgets.length : selectedWidgetIds.length) > 0)
    : (blueprint?.parameters.roles ?? []).every((role) => assignedRoles[role.param]?.role && (role.slots ?? []).every((slot) => assignedRoles[role.param].slots[slot]))
      && (blueprint?.parameters.metrics ?? []).every((metric) => assignedMetrics[metric.param])));
  const canApply = $derived(checked && (checked.preview.kind === 'insert' || checked.preview.widgets.some((widget) => widget.changes.length)));
  let detachMessage = $state('');
  onMount(() => { void client.reusable().then((assets) => entries = assets).catch(() => { /* A failed initial lookup does not invalidate saved widget provenance. */ }); });

  async function open(next: ReusableKind, fromMark = false): Promise<void> {
    if (busy || disabled) return;
    kind = next;
    chosenId = ''; checked = null; message = ''; updated = false;
    assignedRoles = {}; assignedMetrics = {}; mode = 'replace';
    scope = selectedWidgetIds.length ? 'selected' : 'page';
    busy = true;
    try {
      entries = await client.reusable();
      if (fromMark && mark) {
        if (matching.some((entry) => reusableId(entry.asset) === mark.profile_id)) {
          chosenId = mark.profile_id; mode = 'keep_local';
        } else { message = i18n.t('reusable.profile_deleted'); }
      }
    }
    catch (error) { message = reusableError(i18n, error); }
    finally { busy = false; }
  }

  function choose(id: string): void {
    chosenId = id; assignedRoles = {}; assignedMetrics = {}; checked = null; message = '';
  }
  function setRole(param: string, role: string): void {
    assignedRoles = { ...assignedRoles, [param]: { role, slots: {} } };
    checked = null;
  }
  function setSlot(param: string, slot: string, value: string): void {
    const previous = assignedRoles[param];
    if (!previous?.role) return;
    assignedRoles = { ...assignedRoles, [param]: { ...previous, slots: { ...previous.slots, [slot]: value } } };
    checked = null;
  }
  function setMetric(param: string, value: string): void {
    assignedMetrics = { ...assignedMetrics, [param]: value };
    checked = null;
  }
  function choiceFor(role: BlueprintRole): Array<[string, OfflinePage['bundle']['project']['roles'][string]]> {
    return roleOptions.filter(([, target]) => target.cardinality === role.cardinality && (target.slots?.length ?? 0) >= (role.slots?.length ?? 0));
  }
  function metricsFor(param: BlueprintMetric) {
    return catalogOptions.flatMap((catalog) => catalog.metrics
      .filter((metric) => ['numeric_float', 'numeric_unsigned'].includes(metric.value_type))
      .filter((metric) => (!param.value_type || metric.value_type === param.value_type) && (param.units === undefined || metric.units === param.units))
      .map((metric) => ({ catalog: catalog.source.template_name ?? catalog.catalog_id, metric,
        assignment: JSON.stringify({ catalog: catalog.catalog_id, metric: metric.id } satisfies MetricAssignment) })));
  }

  async function review(): Promise<void> {
    const entry = selected;
    if (!entry || !canReview || busy) return;
    let operation: ReusableAction['operation'];
    if (kind === 'style_profile') {
      operation = { kind: 'style', widget_ids: scope === 'dashboard' ? [] : scope === 'page' ? pageWidgets.map((widget) => widget.id) : selectedWidgetIds.slice(), mode, entire_dashboard: scope === 'dashboard' };
    } else {
      operation = { kind: 'insert', roles: $state.snapshot(assignedRoles), metrics: Object.fromEntries(Object.entries(assignedMetrics).map(([param, value]) =>
        [param, value === 'unbound' ? null : JSON.parse(value) as MetricAssignment])) };
    }
    const action: ReusableAction = { asset_id: chosenId, asset_revision: entry.revision, expected_revision: page.revision, operation };
    message = ''; checked = null; busy = true;
    try {
      const result = await client.reusableAction(page.local_id, action, false);
      checked = { action, preview: result.preview };
    } catch (error) { message = reusableError(i18n, error); }
    finally { busy = false; }
  }

  async function apply(): Promise<void> {
    const current = checked;
    if (!current || !canApply || busy || disabled) return;
    busy = true; message = ''; onpending(true);
    try {
      const result = await client.reusableAction(page.local_id, $state.snapshot(current.action), true);
      if (result.page) {
        onupdated(result.page);
        kind = null; updated = true;
      }
    } catch (error) { message = reusableError(i18n, error); checked = null; }
    finally { busy = false; onpending(false); }
  }
  function close(): void { if (!busy) { kind = null; checked = null; } }
  async function detach(): Promise<void> {
    if (!mark || disabled || busy) return;
    busy = true; detachMessage = ''; onpending(true);
    try { onupdated(await client.detachReusable(page.local_id, page.revision, selectedWidgetIds[0])); }
    catch (error) { detachMessage = reusableError(i18n, error); }
    finally { busy = false; onpending(false); }
  }
  async function extract(): Promise<void> {
    if (!extractionName.trim() || selectedWidgetIds.length !== 1 || disabled || extracting) return;
    extracting = true; extractionSaved = false; extractionMessage = '';
    try {
      await client.extractReusable(page.local_id, page.revision, selectedWidgetIds[0], extractionName.trim(), extractionKind);
      extractionSaved = true; extractionName = '';
    } catch (error) { extractionMessage = reusableError(i18n, error); }
    finally { extracting = false; }
  }
  function title(id: string): string {
    const widget = pageWidgets.find((item) => item.id === id);
    return widget ? portableWidgetTitle(widget) : id;
  }
</script>

<div class="asset-actions">
  <button type="button" disabled={disabled || busy || (!allowDashboardScope && pageWidgets.length === 0)} onclick={() => void open('style_profile')}>{i18n.t('reusable.style_action')}</button>
  <button type="button" disabled={disabled || busy} onclick={() => void open('widget_blueprint')}>{i18n.t('reusable.blueprint_action')}</button>
  {#if disabled}<span>{i18n.t('reusable.request_save')}</span>{/if}
  {#if updated}<span role="status">{i18n.t('reusable.changed')}</span>{/if}
</div>
{#if mark}
  <p>{i18n.t('reusable.from_profile', { name: markAsset ? displayReusableName(i18n, markAsset) : mark.profile_name, version: mark.profile_version })}
    <button type="button" disabled={disabled || busy} onclick={() => void open('style_profile', true)}>{i18n.t('reusable.update_profile')}</button>
    <button type="button" disabled={disabled || busy} onclick={() => void detach()}>{i18n.t('reusable.detach')}</button>
  </p>
  {#if markAsset && markAsset.asset.document.version > mark.profile_version}<p role="status">{i18n.t('reusable.profile_outdated', { version: markAsset.asset.document.version })}</p>{/if}
{/if}
{#if detachMessage}<p role="alert">{detachMessage}</p>{/if}

{#if selectedWidgetIds.length === 1 && !disabled}
  <details>
    <summary>{i18n.t('reusable.extract')}</summary>
    <form onsubmit={(event) => { event.preventDefault(); void extract(); }}>
      <label>{i18n.t('reusable.choose')}
        <select bind:value={extractionKind}>
          <option value="style_profile">{i18n.t('reusable.profiles')}</option>
          <option value="widget_blueprint">{i18n.t('reusable.blueprints')}</option>
        </select>
      </label>
      <label>{i18n.t('reusable.name')} <input bind:value={extractionName} maxlength={extractionKind === 'style_profile' ? 120 : 255} required /></label>
      <p>{i18n.t(extractionKind === 'style_profile' ? 'reusable.extract_style_help' : 'reusable.extract_blueprint_help')}</p>
      <button type="submit" disabled={extracting || !extractionName.trim()}>{i18n.t('reusable.save')}</button>
      {#if extractionMessage}<p role="alert">{extractionMessage}</p>{/if}
      {#if extractionSaved}<p role="status">{i18n.t('reusable.loaded')}</p>{/if}
    </form>
  </details>
{/if}

{#if kind}
  <Dialog {i18n} id="reusable-apply" title={{ id: kind === 'style_profile' ? 'reusable.style_action' : 'reusable.blueprint_action' }} description={{ id: 'reusable.choose' }} open={true} cancellable={!busy} onclose={close}>
    {#if !busy && matching.length === 0 && !message}<p>{i18n.t('reusable.no_objects')}</p>{/if}
    {#if matching.length}
      <label>{i18n.t('reusable.choose')}
        <select value={chosenId} disabled={busy} onchange={(event) => choose(event.currentTarget.value)}>
          <option value="">{i18n.t('reusable.choose_value')}</option>
          {#each matching as entry (reusableId(entry.asset))}
            <option value={reusableId(entry.asset)}>{displayReusableName(i18n, entry)}</option>
          {/each}
        </select>
      </label>
    {/if}
    {#if selected && kind === 'style_profile'}
      <label>{i18n.t('reusable.scope')}
        <select bind:value={scope} disabled={busy} onchange={() => { checked = null; }}>
          <option value="page">{i18n.t('reusable.scope_page')}</option>
          <option value="selected" disabled={selectedWidgetIds.length === 0}>{i18n.t('reusable.scope_selection', { count: selectedWidgetIds.length })}</option>
          {#if allowDashboardScope}<option value="dashboard">{i18n.t('reusable.scope_dashboard')}</option>{/if}
        </select>
      </label>
      <label>{i18n.t('reusable.mode')}
        <select bind:value={mode} disabled={busy} onchange={() => { checked = null; }}>
          <option value="replace">{i18n.t('reusable.mode_replace')}</option>
          <option value="keep_local">{i18n.t('reusable.mode_keep')}</option>
          <option value="merge">{i18n.t('reusable.mode_merge')}</option>
        </select>
      </label>
    {:else if blueprint}
      {#each blueprint.parameters.roles as role (role.param)}
        <label>{i18n.t('reusable.role', { param: role.param })}
          <select value={assignedRoles[role.param]?.role ?? ''} disabled={busy} onchange={(event) => setRole(role.param, event.currentTarget.value)}>
            <option value="">{i18n.t('reusable.choose_value')}</option>
            {#each choiceFor(role) as [id] (id)}<option value={id}>{id}</option>{/each}
          </select>
        </label>
        {#if assignedRoles[role.param]?.role}
          {#each role.slots ?? [] as slot (slot)}
            <label>{i18n.t('reusable.slot', { slot })}
              <select value={assignedRoles[role.param]?.slots[slot] ?? ''} disabled={busy} onchange={(event) => setSlot(role.param, slot, event.currentTarget.value)}>
                <option value="">{i18n.t('reusable.choose_value')}</option>
                {#each page.bundle.project.roles[assignedRoles[role.param].role].slots ?? [] as id (id)}<option value={id}>{id}</option>{/each}
              </select>
            </label>
          {/each}
        {/if}
      {/each}
      {#each blueprint.parameters.metrics as metric (metric.param)}
        <label>{i18n.t('reusable.metric', { param: metric.param })}
          <select value={assignedMetrics[metric.param] ?? ''} disabled={busy} onchange={(event) => setMetric(metric.param, event.currentTarget.value)}>
            <option value="">{i18n.t('reusable.choose_value')}</option>
            <option value="unbound">{i18n.t('reusable.unbound')}</option>
            {#each metricsFor(metric) as candidate (candidate.assignment)}
              <option value={candidate.assignment}>{candidate.catalog} · {candidate.metric.name}</option>
            {/each}
          </select>
        </label>
      {/each}
    {/if}
    {#if message}<p role="alert">{message}</p>{/if}
    {#if checked?.preview.kind === 'style'}
      <ul>
        {#each checked.preview.widgets as widget (`${widget.page_id}/${widget.widget_id}`)}
          <li><strong>{scope === 'dashboard' ? `${widget.page_name} · ${widget.widget_name || widget.widget_id}` : title(widget.widget_id)}</strong> · {i18n.t(widget.state === 'compatible' ? 'reusable.compatible' : widget.state === 'partial' ? 'reusable.partial' : 'reusable.incompatible')}
            {#if widget.changes.length}
              <ul>{#each widget.changes as change (change.name)}<li><code>{change.name}</code> · {change.before ?? i18n.t('reusable.before_default')} → {change.after}</li>{/each}</ul>
            {/if}
          </li>
        {/each}
      </ul>
      {#if !canApply}<p role="status">{i18n.t('reusable.no_changes')}</p>{/if}
    {:else if checked?.preview.kind === 'insert'}
      <p role="status">{checked.preview.unresolved ? i18n.t('reusable.unresolved', { count: checked.preview.unresolved }) : i18n.t('reusable.insert_ready')}</p>
    {/if}
    {#snippet actions()}
      {#if checked}
        <button type="button" disabled={!canApply || busy || disabled} onclick={() => void apply()}>{i18n.t(kind === 'style_profile' ? 'reusable.apply' : 'reusable.insert')}</button>
      {:else}
        <button type="button" disabled={!canReview || busy} onclick={() => void review()}>{i18n.t('reusable.review')}</button>
      {/if}
      <button type="button" disabled={busy} onclick={close}>{i18n.t('reusable.cancel')}</button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .asset-actions { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-3); }
  label { display: grid; gap: var(--space-1); margin-block: var(--space-2); }
  select { max-inline-size: 100%; }
  li { overflow-wrap: anywhere; }
</style>
