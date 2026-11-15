<script lang="ts">
  import type { I18n, MessageId } from '../i18n';
  import { untrack } from 'svelte';
  import { ApiFailure } from '../api';
  import { createCatalogClient, type PageRole, type PortablePageWidget, type RedactionChange, type RedactionPreview, type RedactionTarget, type SensitiveScan } from './client';
  import { redactableHostMaskIndices, redactableTextIndices, redactableItemPatternIndices } from './redaction-targets';
  import SaveAsButton from './SaveAsButton.svelte';
  import { canSelectFinding, findingTarget } from './finding-target';
  let { i18n, kind, id, revision, enabled, pages, roles, catalogs, jump }: { i18n: I18n; kind: 'page' | 'dashboard'; id: string; revision: number; enabled: boolean; pages: Array<{ name: string; widgets: PortablePageWidget[] }>; roles: Record<string, PageRole>; catalogs: Record<string, { source: { template_name?: string }; metrics: Array<{ name: string }> }>; jump: { location: string; sequence: number } | null } = $props();
  const client = createCatalogClient();
  let selected = $state('');
  let replacement = $state('');
  let applyToMatching = $state(false);
  let changes = $state<RedactionChange[]>([]);
  let preview = $state<RedactionPreview | null>(null);
  let acknowledged = $state(false);
  let busy = $state(false);
  let failed = $state(false);
  let protectedFailure = $state(false);
  let saved = $state<{ path?: string; sha256?: string } | null>(null);
  let generation = 0;
  let processedJump = 0;
  const labels = { url: 'package_scan.url', email: 'package_scan.email', ip: 'package_scan.ip', macro: 'package_scan.macro', unknown: 'package_scan.unknown', metric_key: 'package_scan.metric_key', mask: 'package_scan.mask', path: 'package_scan.path', fqdn: 'package_scan.fqdn', credential: 'package_scan.credential', identifier: 'package_scan.identifier', free_text: 'package_scan.free_text' } as const satisfies Record<SensitiveScan['findings'][number]['kind'], MessageId>;
  const evidenceLabels = { structural: 'package_scan.evidence_structural', heuristic: 'package_scan.evidence_heuristic', unanalysed: 'package_scan.evidence_unanalysed' } as const satisfies Record<SensitiveScan['findings'][number]['evidence'], MessageId>;
  function named(target: string, value: string): string { return value ? i18n.t('redaction.with_value', { target, value }) : target; }
  function hostMaskChoices(widget: PortablePageWidget, page_index: number, widget_index: number): Array<{ value: string; label: string }> {
    return redactableHostMaskIndices(widget).map((field_index) => ({ value: JSON.stringify({ kind: 'widget_host_mask', page_index, widget_index, field_index }), label: i18n.t('redaction.host_mask', { page: page_index + 1, widget: widget_index + 1, field: field_index + 1 }) }));
  }
  const options = $derived([
    { value: JSON.stringify({ kind: 'project_id' }), label: i18n.t('redaction.project_id') },
    { value: JSON.stringify({ kind: 'clear_provenance' }), label: i18n.t('redaction.clear_provenance') },
    ...(kind === 'dashboard' ? [{ value: JSON.stringify({ kind: 'dashboard_id' }), label: i18n.t('redaction.dashboard_id') }] : []),
    ...(kind === 'dashboard' ? [{ value: JSON.stringify({ kind: 'dashboard_name' }), label: i18n.t('redaction.dashboard') }] : []),
    ...Object.keys(catalogs).sort().flatMap((key, catalog_index) => [
      { value: JSON.stringify({ kind: 'catalog_id', catalog_index }), label: named(i18n.t('redaction.catalog_id', { catalog: catalog_index + 1 }), catalogs[key].source.template_name ?? key) },
      { value: JSON.stringify({ kind: 'catalog_selector_aliases', catalog_index }), label: i18n.t('redaction.selector_aliases', { catalog: catalog_index + 1 }) },
      { value: JSON.stringify({ kind: 'catalog_clear_uuids', catalog_index }), label: i18n.t('redaction.clear_uuids', { catalog: catalog_index + 1 }) },
      { value: JSON.stringify({ kind: 'catalog_clear_source', catalog_index }), label: i18n.t('redaction.clear_catalog_source', { catalog: catalog_index + 1 }) },
      ...(catalogs[key].source.template_name ? [{ value: JSON.stringify({ kind: 'catalog_template_name', catalog_index }), label: i18n.t('redaction.catalog_template', { catalog: catalog_index + 1 }) }] : []),
      ...catalogs[key].metrics.flatMap((metric, metric_index) => [
        { value: JSON.stringify({ kind: 'catalog_metric_name', catalog_index, metric_index }), label: named(i18n.t('redaction.catalog_metric', { catalog: catalog_index + 1, metric: metric_index + 1 }), metric.name) },
        { value: JSON.stringify({ kind: 'catalog_metric_units', catalog_index, metric_index }), label: named(i18n.t('redaction.catalog_metric_units', { catalog: catalog_index + 1, metric: metric_index + 1 }), metric.name) },
        { value: JSON.stringify({ kind: 'catalog_metric_id', catalog_index, metric_index }), label: named(i18n.t('redaction.catalog_metric_id', { catalog: catalog_index + 1, metric: metric_index + 1 }), metric.name) }
      ])
    ]),
    ...Object.keys(roles).sort().flatMap((role, role_index) => [
      { value: JSON.stringify({ kind: 'role_alias', role_index }), label: named(i18n.t('redaction.role', { role: role_index + 1 }), role) },
      ...(roles[role].description ? [{ value: JSON.stringify({ kind: 'role_description', role_index }), label: i18n.t('redaction.role_description', { role: role_index + 1 }) }] : []),
      ...(roles[role].slots ?? []).map((slot, slot_index) => ({ value: JSON.stringify({ kind: 'slot_alias', role_index, slot_index }), label: named(i18n.t('redaction.slot', { role: role_index + 1, slot: slot_index + 1 }), slot) }))
    ]),
    ...pages.flatMap((page, page_index) => [
      ...(kind === 'dashboard' ? [{ value: JSON.stringify({ kind: 'remove_page', page_index }), label: named(i18n.t('redaction.remove_page', { page: page_index + 1 }), page.name) }] : []),
      { value: JSON.stringify({ kind: 'page_id', page_index }), label: named(i18n.t('redaction.page_id', { page: page_index + 1 }), page.name) },
      { value: JSON.stringify({ kind: 'page_name', page_index }), label: named(i18n.t('redaction.page', { page: page_index + 1 }), page.name) },
      ...page.widgets.flatMap((widget, widget_index) => [
        { value: JSON.stringify({ kind: 'widget_id', page_index, widget_index }), label: named(i18n.t('redaction.widget_id', { page: page_index + 1, widget: widget_index + 1 }), widget.type === 'zabbix' ? widget.name : widget.title) },
        { value: JSON.stringify({ kind: 'remove_widget', page_index, widget_index }), label: named(i18n.t('redaction.remove_widget', { page: page_index + 1, widget: widget_index + 1 }), widget.type === 'zabbix' ? widget.name : widget.title) },
        ...(widget.type === 'zabbix' && widget.registry_state === 'unknown' ? [] : [{ value: JSON.stringify({ kind: 'widget_name', page_index, widget_index }), label: i18n.t('redaction.widget', { page: page_index + 1, widget: widget_index + 1 }) }, ...hostMaskChoices(widget, page_index, widget_index)]),
        ...(widget.type === 'zabbix' && widget.registry_state === 'known' ? widget.fields.flatMap((raw, field_index) => { const field = raw as { registry_state?: string; value?: { kind?: string } }; return field?.registry_state === 'known' && field.value?.kind === 'unresolved' ? [{ value: JSON.stringify({ kind: 'widget_unresolved_name', page_index, widget_index, field_index }), label: i18n.t('redaction.unresolved_name', { page: page_index + 1, widget: widget_index + 1, field: field_index + 1 }) }] : []; }) : [])
        , ...redactableTextIndices(widget).map((field_index) => ({ value: JSON.stringify({ kind: 'widget_text', page_index, widget_index, field_index }), label: i18n.t('redaction.widget_text', { page: page_index + 1, widget: widget_index + 1, field: field_index + 1 }) }))
        , ...redactableItemPatternIndices(widget).map((field_index) => ({ value: JSON.stringify({ kind: 'widget_item_pattern', page_index, widget_index, field_index }), label: i18n.t('redaction.item_pattern', { page: page_index + 1, widget: widget_index + 1, field: field_index + 1 }) }))
      ])
    ])
  ]);
  $effect(() => { revision; id; enabled; generation++; changes = []; preview = null; acknowledged = false; failed = false; protectedFailure = false; saved = null; selected = ''; replacement = ''; applyToMatching = false; });
  $effect(() => {
    const request = jump;
    if (!request || !enabled || request.sequence === processedJump) return;
    processedJump = request.sequence;
    untrack(() => {
      const target = findingTarget(request.location, kind, options.map((option) => option.value));
      if (canSelectFinding(target, selected, replacement, applyToMatching, changes.map((change) => JSON.stringify(change.target)))) selected = target;
      document.getElementById(`redaction-title-${kind}-${id}`)?.scrollIntoView({ block: 'start' });
      document.getElementById(`redaction-target-${kind}-${id}`)?.focus();
    });
  });
  function invalidate(): void { preview = null; acknowledged = false; failed = false; protectedFailure = false; saved = null; }
  const removing = $derived(['remove_page', 'remove_widget', 'catalog_selector_aliases', 'catalog_clear_uuids', 'catalog_clear_source', 'clear_provenance'].some((kind) => selected.includes(`"${kind}"`)));
  const mayBeEmpty = $derived(selected.includes('"catalog_metric_units"'));
  const groupable = $derived(!selected || ['page_name', 'widget_name', 'widget_host_mask', 'dashboard_name'].some((kind) => selected.includes(`"${kind}"`)));
  function add(): void {
    if (!enabled || !selected || (!removing && !mayBeEmpty && !replacement.trim()) || changes.some((item) => JSON.stringify(item.target) === selected)) return;
    changes = [...changes, { target: JSON.parse(selected) as RedactionTarget, replacement: removing ? '' : replacement, apply_to_matching: applyToMatching }];
    selected = ''; replacement = ''; applyToMatching = false; invalidate();
  }
  async function review(): Promise<void> {
    if (!enabled || !changes.length || busy || selected || replacement.trim() || applyToMatching) return;
    const current = generation; busy = true; invalidate();
    try { const result = await client.previewRedaction(kind, id, revision, $state.snapshot(changes)); if (generation === current && result.revision === revision) preview = result; }
    catch (error) { if (generation === current) { failed = true; protectedFailure = error instanceof ApiFailure && error.response?.code === 'redaction_protected'; } }
    finally { busy = false; }
  }
  async function download(): Promise<void> {
    if (!enabled || !preview || !acknowledged || busy || selected || replacement.trim() || applyToMatching) return;
    const current = generation; busy = true; failed = false;
    try {
      const result = await client.exportRedaction(kind, id, revision, $state.snapshot(changes), preview.sha256);
      if (generation !== current) return;
      const link = document.createElement('a'); link.href = `/api/downloads/${encodeURIComponent(result.id)}`; link.download = result.display_name; document.body.append(link); link.click(); link.remove();
      acknowledged = false;
    } catch (error) { if (generation === current) { failed = true; protectedFailure = error instanceof ApiFailure && error.response?.code === 'redaction_protected'; } }
    finally { busy = false; }
  }
  async function save(jobId: string): Promise<{ cancelled: boolean }> {
    if (!enabled || !preview || !acknowledged || busy || selected || replacement.trim() || applyToMatching) return { cancelled: true };
    const current = generation; busy = true; failed = false; saved = null;
    try {
      const result = await client.saveRedaction(kind, id, revision, $state.snapshot(changes), preview.sha256, jobId);
      if (generation === current && !result.cancelled) { saved = result; acknowledged = false; }
      return result;
    } catch (error) { if (generation === current) { failed = true; protectedFailure = error instanceof ApiFailure && error.response?.code === 'redaction_protected'; } throw error; }
    finally { busy = false; }
  }
  function label(target: RedactionTarget): string { return options.find((item) => item.value === JSON.stringify(target))?.label ?? ''; }
</script>

<section aria-labelledby={`redaction-title-${kind}-${id}`}>
  <h3 id={`redaction-title-${kind}-${id}`}>{i18n.t('redaction.title')}</h3>
  <p>{i18n.t('redaction.help')}</p>
  <p class="muted">{i18n.t('redaction.limit')}</p>
  <label for={`redaction-target-${kind}-${id}`}>{i18n.t('redaction.target')}</label>
  <select id={`redaction-target-${kind}-${id}`} bind:value={selected} onchange={() => { applyToMatching = false; replacement = ''; }} disabled={!enabled || busy}>
    <option value="">{i18n.t('package_import.choose')}</option>
    {#each options as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
  </select>
  <label for={`redaction-value-${kind}-${id}`}>{i18n.t('redaction.replacement')}</label>
  <input id={`redaction-value-${kind}-${id}`} maxlength="200" bind:value={replacement} disabled={!enabled || busy || removing} />
  <label><input type="checkbox" bind:checked={applyToMatching} disabled={!enabled || busy || !groupable} /> {i18n.t('redaction.matching')}</label>
  <button type="button" onclick={add} disabled={!enabled || busy || !selected || (!removing && !mayBeEmpty && !replacement.trim()) || changes.length >= 128 || changes.some((item) => JSON.stringify(item.target) === selected)}>{i18n.t('redaction.add')}</button>
  {#if changes.length > 0}<ul>{#each changes as change, index (JSON.stringify(change.target))}<li>{label(change.target)}: <bdi>{change.replacement}</bdi> <button type="button" disabled={busy} onclick={() => { changes = changes.filter((_, current) => current !== index); invalidate(); }}>{i18n.t('redaction.remove')}</button></li>{/each}</ul>{/if}
  {#if changes.some((change) => change.target.kind === 'widget_host_mask') || preview?.changes.some((change) => change.target.kind === 'widget_host_mask')}<p class="muted">{i18n.t('redaction.mask_caution')}</p>{/if}
  {#if changes.some((change) => change.target.kind === 'widget_item_pattern')}<p class="muted">{i18n.t('redaction.item_pattern_caution')}</p>{/if}
  {#if changes.some((change) => change.target.kind === 'remove_widget' || change.target.kind === 'remove_page')}<p class="muted">{i18n.t('redaction.remove_caution')}</p>{/if}
  {#if changes.some((change) => ['project_id', 'dashboard_id', 'page_id', 'widget_id', 'catalog_id', 'catalog_metric_id'].includes(change.target.kind))}<p class="muted">{i18n.t('redaction.id_caution')}</p>{/if}
  {#if changes.some((change) => change.target.kind === 'catalog_selector_aliases')}<p class="muted">{i18n.t('redaction.alias_caution')}</p>{/if}
  {#if changes.some((change) => change.target.kind === 'catalog_clear_source')}<p class="muted">{i18n.t('redaction.clear_catalog_source_caution')}</p>{/if}
  {#if changes.some((change) => change.target.kind === 'catalog_metric_units') || mayBeEmpty}<p class="muted">{i18n.t('redaction.units_caution')}</p>{/if}
  <button type="button" disabled={!enabled || busy || changes.length === 0 || !!selected || !!replacement.trim() || applyToMatching} onclick={() => void review()}>{i18n.t('redaction.preview')}</button>
  {#if failed}<p role="alert">{i18n.t(protectedFailure ? 'redaction.protected' : 'redaction.failed')}</p>{/if}
  {#if preview}
    <ul>{#each preview.changes as change}<li>{label(change.target)}: <bdi>{change.before_masked}</bdi> → <bdi>{changes[change.request_index]?.replacement ?? ''}</bdi>. {i18n.t('redaction.length', { before: change.before_chars, after: change.after_chars })}</li>{/each}</ul>
    <p>{i18n.t('redaction.remaining', { count: preview.remaining.findings.length })}</p>
    {#if preview.remaining.truncated}<p role="alert">{i18n.t('package_scan.truncated')}</p>{/if}
    {#if preview.remaining.unknown_objects > 0}<p role="alert">{i18n.t('package_scan.unknown_count', { count: preview.remaining.unknown_objects })}</p>{/if}
    {#if preview.remaining.credential_candidates > 0}<p role="alert">{i18n.t('redaction.credentials_block')}</p>{/if}
    {#if preview.remaining.findings.length > 0}<ul>{#each preview.remaining.findings as finding}<li>{i18n.t(labels[finding.kind])} · {i18n.t(evidenceLabels[finding.evidence])}: <code>{finding.location}</code></li>{/each}</ul>{/if}
    <p>{i18n.t('package_import.sha256')}: <code>{preview.sha256}</code></p>
    <label><input type="checkbox" bind:checked={acknowledged} disabled={!enabled || busy} /> {i18n.t('redaction.acknowledge')}</label>
    <button type="button" disabled={!enabled || busy || !acknowledged || !!selected || !!replacement.trim() || applyToMatching || preview.remaining.credential_candidates > 0} onclick={() => void download()}>{i18n.t('redaction.export')}</button>
    <SaveAsButton {i18n} messageId="redaction.save_as" disabled={!enabled || busy || !acknowledged || !!selected || !!replacement.trim() || applyToMatching || preview.remaining.credential_candidates > 0} run={save} onDone={() => {}} onError={() => { failed = true; }} />
    <p class="muted">{i18n.t('redaction.save_as_hint')}</p>
    {#if saved}<p role="status">{i18n.t('redaction.saved')}: <bdi>{saved.path}</bdi> · {i18n.t('package_import.sha256')}: <code>{saved.sha256}</code></p>{/if}
  {/if}
</section>
