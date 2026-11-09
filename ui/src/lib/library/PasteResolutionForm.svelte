<script lang="ts">
  import type { I18n } from '../i18n';
  import type { ClipboardContent, ClipboardContext } from './editor-clipboard';
  import { compatibleMetric, compatibleRole, metricRequirements, type PasteResolution } from './editor-paste-resolution';
  let { i18n, content, target, resolution, onchange }: { i18n: I18n; content: ClipboardContent; target: ClipboardContext; resolution: PasteResolution; onchange: (value: PasteResolution) => void } = $props();
  const requirements = $derived(metricRequirements(content));
  function catalog(source: string, id: string): void {
    const next = $state.snapshot(resolution); const pin = target.page.catalogs.find((pin) => pin.catalog_id === id);
    if (pin) next.catalogs[source] = { ...pin }; else delete next.catalogs[source];
    for (const entry of requirements.filter((entry) => entry.catalog === source)) delete next.metrics[entry.key];
    onchange(next);
  }
  function metric(key: string, sourceCatalog: string, id: string): void {
    const next = $state.snapshot(resolution); const pin = next.catalogs[sourceCatalog];
    const evidence = pin && target.metrics[pin.catalog_id]?.find((metric) => metric.id === id);
    if (evidence) next.metrics[key] = { metric_id: id, evidence: $state.snapshot(evidence) }; else delete next.metrics[key];
    onchange(next);
  }
  function role(source: string, id: string): void {
    const next = $state.snapshot(resolution); const evidence = target.page.roles[id];
    if (evidence) next.roles[source] = { role: id, evidence: $state.snapshot(evidence), slots: {} }; else delete next.roles[source];
    onchange(next);
  }
  function slot(source: string, sourceSlot: string, id: string): void {
    const next = $state.snapshot(resolution); if (!next.roles[source]) return;
    next.roles[source].slots[sourceSlot] = id; onchange(next);
  }
</script>

<section aria-labelledby="paste-resolution-title">
  <h3 id="paste-resolution-title">{i18n.t('offline_editor.resolve.title')}</h3>
  <p>{i18n.t('offline_editor.resolve.help')}</p>
  {#each content.catalogs as pin, index (pin.catalog_id)}
    <fieldset><legend>{i18n.t('offline_editor.resolve.catalog', { catalog: pin.catalog_id, version: pin.version })}</legend>
      <label for={`paste-catalog-${index}`}>{i18n.t('offline_editor.resolve.target_catalog')}</label>
      <select id={`paste-catalog-${index}`} value={resolution.catalogs[pin.catalog_id]?.catalog_id ?? ''} onchange={(event) => catalog(pin.catalog_id, event.currentTarget.value)}>
        <option value="">{i18n.t('offline_editor.resolve.choose')}</option>
        {#each target.page.catalogs as candidate (candidate.catalog_id)}<option value={candidate.catalog_id}>{candidate.catalog_id} · {i18n.t('offline_builder.version', { version: candidate.version })}</option>{/each}
      </select>
      {#each requirements.filter((entry) => entry.catalog === pin.catalog_id) as entry, metricIndex (entry.key)}
        {@const targetCatalog = resolution.catalogs[pin.catalog_id]?.catalog_id}
        {@const candidates = (target.metrics[targetCatalog ?? ''] ?? []).filter((candidate) => compatibleMetric(entry.metric, candidate))}
        <p><bdi>{entry.metric.name}</bdi> · <bdi>{entry.metric.id}</bdi> · <bdi>{entry.metric.value_type}</bdi> · <bdi>{entry.metric.units}</bdi></p>
        <label for={`paste-metric-${index}-${metricIndex}`}>{i18n.t('offline_editor.resolve.target_metric')}</label>
        <select id={`paste-metric-${index}-${metricIndex}`} disabled={!targetCatalog} value={resolution.metrics[entry.key]?.metric_id ?? ''} onchange={(event) => metric(entry.key, pin.catalog_id, event.currentTarget.value)}>
          <option value="">{i18n.t('offline_editor.resolve.choose')}</option>
          {#each candidates as candidate (candidate.id)}<option value={candidate.id}>{candidate.name} · {candidate.id} · {candidate.value_type} · {candidate.units}</option>{/each}
        </select>
        {#if targetCatalog && candidates.length === 0}<p>{i18n.t('offline_editor.resolve.no_metrics')}</p>{/if}
      {/each}
    </fieldset>
  {/each}
  {#each Object.entries(content.roles) as [id, sourceRole], index (id)}
    {@const candidates = Object.entries(target.page.roles).filter(([, candidate]) => compatibleRole(sourceRole, candidate))}
    <fieldset><legend>{i18n.t('offline_editor.resolve.role', { role: id, count: sourceRole.slots?.length ?? 0 })}</legend>
      <label for={`paste-role-${index}`}>{i18n.t('offline_editor.resolve.target_role')}</label>
      <select id={`paste-role-${index}`} value={resolution.roles[id]?.role ?? ''} onchange={(event) => role(id, event.currentTarget.value)}>
        <option value="">{i18n.t('offline_editor.resolve.choose')}</option>
        {#each candidates as [roleId, candidate] (roleId)}<option value={roleId}>{roleId} · {i18n.t(candidate.cardinality === 'one' ? 'offline_builder.role_one' : 'offline_builder.role_many')}</option>{/each}
      </select>
      {#if candidates.length === 0}<p>{i18n.t('offline_editor.resolve.no_roles')}</p>{/if}
      <p>{i18n.t('offline_editor.resolve.slot_order')}</p>
      {#each sourceRole.slots ?? [] as sourceSlot, slotIndex (sourceSlot)}
        <label for={`paste-slot-${index}-${slotIndex}`}>{i18n.t('offline_editor.resolve.slot', { slot: sourceSlot, position: slotIndex + 1 })}</label>
        <select id={`paste-slot-${index}-${slotIndex}`} disabled={!resolution.roles[id]} value={resolution.roles[id]?.slots[sourceSlot] ?? ''} onchange={(event) => slot(id, sourceSlot, event.currentTarget.value)}>
          <option value="">{i18n.t('offline_editor.resolve.choose')}</option>
          {#each target.page.roles[resolution.roles[id]?.role ?? '']?.slots ?? [] as targetSlot, targetIndex (targetSlot)}<option value={targetSlot}>{targetIndex + 1} · {targetSlot}</option>{/each}
        </select>
      {/each}
    </fieldset>
  {/each}
</section>
