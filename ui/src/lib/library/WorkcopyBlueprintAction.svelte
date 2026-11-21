<script lang="ts">
  import { Dialog } from '../components';
  import type { I18n } from '../i18n';
  import type { DraftWidget } from '../shell/drafts';
  import {
    createCatalogClient, reusableId, type CatalogMetric, type CatalogSummary,
    type CopyBlueprintMetric, type CopyBlueprintPreview, type CopyBlueprintRequest, type ReusableEntry
  } from './client';
  import { displayReusableName, reusableError } from './reusable';

  let { i18n, id, serverVersion, pageId, connected, rebind, onchanged }: {
    i18n: I18n; id: string; serverVersion: string; pageId: string; connected: boolean; rebind?: DraftWidget; onchanged: () => void;
  } = $props();
  const client = createCatalogClient();
  let open = $state(false);
  let busy = $state(false);
  let entries = $state<ReusableEntry[]>([]);
  let catalogs = $state<CatalogSummary[]>([]);
  let inventory = $state<Record<number, CatalogMetric[]>>({});
  let hosts = $state<Array<{ hostid: string; name: string }>>([]);
  let search = $state('');
  let truncated = $state(false);
  let chosen = $state('');
  let roles = $state<CopyBlueprintRequest['roles']>({});
  let choices = $state<Record<string, { catalog: string; metric: string }>>({});
  let x = $state<number | undefined>(0);
  let y = $state<number | undefined>(0);
  let revision = $state(0);
  let preview = $state<{ request: CopyBlueprintRequest; result: CopyBlueprintPreview } | null>(null);
  let allowUnbound = $state(false);
  let message = $state('');
  const entry = $derived(entries.find((candidate) => reusableId(candidate.asset) === chosen));
  const blueprint = $derived(entry?.asset.kind === 'widget_blueprint' ? entry.asset.document : null);
  const title = $derived(rebind ? 'reusable.blueprint_rebind' : 'reusable.blueprint_action');

  async function run(action: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    message = '';
    try { await action(); }
    catch (error) { message = reusableError(i18n, error); }
    finally { busy = false; }
  }

  function show(): void {
    open = true; chosen = ''; preview = null; inventory = {}; hosts = []; search = ''; truncated = false;
    x = rebind?.x ?? 0;
    y = rebind?.y ?? 0;
    void run(async () => {
      const [objects, library, draft] = await Promise.all([client.reusable(), client.list(), client.copyStyleState(id)]);
      entries = objects.filter((item) => item.asset.kind === 'widget_blueprint'
        && item.asset.document.adapter === serverVersion.split('.').slice(0, 2).join('.')
        && (!rebind || item.asset.document.blueprint_id === rebind.blueprint_id));
      catalogs = library;
      revision = draft.revision;
      if (rebind && entries[0]) {
        chosen = reusableId(entries[0].asset);
        choose();
      }
    });
  }

  function choose(): void {
    preview = null;
    roles = Object.fromEntries((blueprint?.parameters.roles ?? []).map((role) => [
      role.param, { host: null, slots: Object.fromEntries((role.slots ?? []).map((slot) => [slot, null])) }
    ]));
    choices = Object.fromEntries((blueprint?.parameters.metrics ?? []).map((metric) => [metric.param, { catalog: '', metric: '' }]));
  }

  function selectCatalog(param: string): void {
    preview = null;
    choices[param].metric = '';
    if (!choices[param].catalog) return;
    const index = Number(choices[param].catalog);
    const catalog = catalogs[index];
    if (inventory[index] || !catalog) return;
    void run(async () => { inventory[index] = (await client.catalog(catalog.catalog_id, catalog.version)).metrics; });
  }

  function findHosts(): void {
    if (!connected) return;
    void run(async () => {
      const result = await client.copyBlueprintHosts(id, search);
      hosts = [...new Map([...hosts, ...result.hosts].map((host) => [host.hostid, host])).values()];
      truncated = result.truncated;
    });
  }

  function review(): void {
    if (!blueprint || !entry || x === undefined || y === undefined) return;
    allowUnbound = false;
    const metrics: Record<string, CopyBlueprintMetric | null> = {};
    for (const [param, choice] of Object.entries(choices)) {
      const catalog = catalogs[Number(choice.catalog)];
      metrics[param] = choice.metric && catalog ? { catalog_id: catalog.catalog_id, version: catalog.version, metric_id: choice.metric } : null;
    }
    const request = { expected_revision: revision, blueprint_id: blueprint.blueprint_id,
      blueprint_revision: entry.revision, page_id: pageId, rebind_widget: rebind?.id,
      roles: $state.snapshot(roles), metrics, x, y };
    void run(async () => { preview = { request, result: await client.copyBlueprint(id, request) }; });
  }

  function insert(): void {
    const current = preview;
    if (!current || (current.result.unresolved.length > 0 && !allowUnbound)) return;
    void run(async () => {
      await client.copyBlueprint(id, $state.snapshot(current.request), current.result.fingerprint, allowUnbound);
      onchanged();
      close();
    });
  }

  function close(): void { open = false; preview = null; }
</script>

<button type="button" disabled={busy} onclick={show}>{i18n.t(title)}</button>
{#if open}
  <Dialog {i18n} id="workcopy-blueprint" title={{ id: title }}
    description={{ id: rebind ? 'reusable.blueprint_rebind_hint' : 'reusable.blueprint_copy_hint' }} open cancellable={!busy} onclose={close}>
    {#if entries.length}
      <label>{i18n.t('reusable.choose')}
        <select bind:value={chosen} disabled={busy || !!rebind} onchange={choose}>
          <option value="">{i18n.t('reusable.choose_value')}</option>
          {#each entries as item (reusableId(item.asset))}
            <option value={reusableId(item.asset)}>{displayReusableName(i18n, item)}</option>
          {/each}
        </select>
      </label>
    {:else if !busy && !message}<p>{i18n.t('reusable.no_objects')}</p>{/if}
    {#if blueprint}
      {#if connected && blueprint.parameters.roles.length}
        <label>{i18n.t('reusable.blueprint_host_search')} <input bind:value={search} maxlength="255" disabled={busy} /></label>
        <button type="button" disabled={busy} onclick={findHosts}>{i18n.t('reusable.blueprint_host_search')}</button>
        {#if truncated}<p>{i18n.t('reusable.blueprint_host_truncated')}</p>{/if}
      {/if}
      {#each blueprint.parameters.roles as role (role.param)}
        {#if roles[role.param]}
          <fieldset disabled={busy}>
            <legend>{i18n.t('reusable.role', { param: role.param })}</legend>
            {#if role.cardinality === 'one'}
              <label>{i18n.t('reusable.blueprint_host')}
                <select bind:value={roles[role.param].host} onchange={() => preview = null}>
                  <option value={null}>{i18n.t('reusable.unbound')}</option>
                  {#each hosts as host (host.hostid)}<option value={host.hostid}>{host.name}</option>{/each}
                </select>
              </label>
            {:else}
              {#each role.slots ?? [] as slot (slot)}
                <label>{i18n.t('reusable.slot', { slot })}
                  <select bind:value={roles[role.param].slots[slot]} onchange={() => preview = null}>
                    <option value={null}>{i18n.t('reusable.unbound')}</option>
                    {#each hosts as host (host.hostid)}<option value={host.hostid}>{host.name}</option>{/each}
                  </select>
                </label>
              {/each}
            {/if}
          </fieldset>
        {/if}
      {/each}
      {#each blueprint.parameters.metrics as metric (metric.param)}
        {#if choices[metric.param]}
          <fieldset disabled={busy}>
            <legend>{i18n.t('reusable.metric', { param: metric.param })}</legend>
            <label>{i18n.t('reusable.blueprint_catalog')}
              <select bind:value={choices[metric.param].catalog} onchange={() => selectCatalog(metric.param)}>
                <option value="">{i18n.t('reusable.choose_value')}</option>
                {#each catalogs as catalog, index (catalog.catalog_id + '/' + catalog.version)}
                  <option value={String(index)}>{catalog.name} · {catalog.version}</option>
                {/each}
              </select>
            </label>
            {#if choices[metric.param].catalog}
              <label>{i18n.t('reusable.metric', { param: metric.param })}
                <select bind:value={choices[metric.param].metric} onchange={() => preview = null}>
                  <option value="">{i18n.t('reusable.unbound')}</option>
                  {#each (inventory[Number(choices[metric.param].catalog)] ?? [])
                    .filter((option) => ['numeric_float', 'numeric_unsigned'].includes(option.value_type)
                      && ['template_item', 'host_item'].includes(option.origin)
                      && (metric.value_type === undefined || metric.value_type === option.value_type)
                      && (metric.units === undefined || metric.units === option.units)) as option (option.id)}
                    <option value={option.id}>{option.name}</option>
                  {/each}
                </select>
              </label>
            {/if}
          </fieldset>
        {/if}
      {/each}
      {#if !rebind}
        <label>{i18n.t('reusable.blueprint_x')} <input type="number" min="0" max="71" bind:value={x} disabled={busy} oninput={() => preview = null} /></label>
        <label>{i18n.t('reusable.blueprint_y')} <input type="number" min="0" max="63" bind:value={y} disabled={busy} oninput={() => preview = null} /></label>
      {/if}
    {/if}
    {#if preview}
      <p>{preview.result.widget.name}</p>
      {#if preview.result.unresolved.length}
        <p>{i18n.t('reusable.unresolved', { count: preview.result.unresolved.length })}</p>
        <label><input type="checkbox" bind:checked={allowUnbound} disabled={busy} /> {i18n.t('reusable.blueprint_keep_unbound')}</label>
      {:else}
        <p>{i18n.t('reusable.insert_ready')}</p>
      {/if}
    {/if}
    {#if message}<p role="alert">{message}</p>{/if}
    {#snippet actions()}
      {#if preview}
        <button type="button" disabled={busy || (preview.result.unresolved.length > 0 && !allowUnbound)} onclick={insert}>{i18n.t(rebind ? 'reusable.apply' : 'reusable.insert')}</button>
      {:else}
        <button type="button" disabled={busy || !blueprint || x === undefined || y === undefined} onclick={review}>{i18n.t('reusable.review')}</button>
      {/if}
      <button type="button" disabled={busy} onclick={close}>{i18n.t('reusable.cancel')}</button>
    {/snippet}
  </Dialog>
{/if}
