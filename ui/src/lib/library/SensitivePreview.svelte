<script lang="ts">
  import type { I18n, MessageId } from '../i18n';
  import { createCatalogClient, type SensitiveScan } from './client';
  let { i18n, kind, id, revision, enabled, onFinding }: { i18n: I18n; kind: 'page' | 'dashboard'; id: string; revision: number; enabled: boolean; onFinding: (location: string) => void } = $props();
  const client = createCatalogClient();
  let scan = $state<SensitiveScan | null>(null);
  let busy = $state(false);
  let failed = $state(false);
  let filter = $state<'all' | SensitiveScan['findings'][number]['kind']>('all');
  const labels = { url: 'package_scan.url', email: 'package_scan.email', ip: 'package_scan.ip', macro: 'package_scan.macro', unknown: 'package_scan.unknown', metric_key: 'package_scan.metric_key', mask: 'package_scan.mask', path: 'package_scan.path', fqdn: 'package_scan.fqdn', credential: 'package_scan.credential', identifier: 'package_scan.identifier', free_text: 'package_scan.free_text' } as const satisfies Record<SensitiveScan['findings'][number]['kind'], MessageId>;
  const evidenceLabels = { structural: 'package_scan.evidence_structural', heuristic: 'package_scan.evidence_heuristic', unanalysed: 'package_scan.evidence_unanalysed' } as const satisfies Record<SensitiveScan['findings'][number]['evidence'], MessageId>;
  let generation = 0;
  $effect(() => { revision; id; enabled; generation++; scan = null; failed = false; filter = 'all'; });
  async function check(): Promise<void> {
    if (!enabled || busy) return;
    const current = generation;
    busy = true; failed = false; scan = null;
    try { const result = await client.scanPackage(kind, id); if (generation === current) { if (result.revision === revision) scan = result; else failed = true; } }
    catch { if (generation === current) failed = true; }
    finally { busy = false; }
  }
</script>

<section>
  <p class="muted">{i18n.t('package_scan.warning')}</p>
  <button type="button" disabled={!enabled || busy} onclick={() => void check()}>{i18n.t('package_scan.action')}</button>
  {#if failed}<p role="alert">{i18n.t('package_scan.failed')}</p>{/if}
  {#if scan}
    {#if scan.findings.length > 0}
      <label for={`package-scan-filter-${kind}-${id}`}>{i18n.t('package_scan.filter')}</label>
      <select id={`package-scan-filter-${kind}-${id}`} bind:value={filter}>
        <option value="all">{i18n.t('common.filter.all')}</option>
        {#each Object.entries(labels) as [key, label] (key)}<option value={key}>{i18n.t(label)}</option>{/each}
      </select>
    {/if}
    {#if scan.findings.length === 0}<p role="status">{i18n.t('package_scan.none')}</p>{/if}
    {#if scan.truncated}<p role="alert">{i18n.t('package_scan.truncated')}</p>{/if}
    {#if scan.unknown_objects > 0}<p role="alert">{i18n.t('package_scan.unknown_count', { count: scan.unknown_objects })}</p>{/if}
    {#if scan.credential_candidates > 0}<p role="alert">{i18n.t('package_scan.credential_count', { count: scan.credential_candidates })}</p>{/if}
    {#if scan.findings.length > 0}<ul>{#each scan.findings.filter((finding) => filter === 'all' || filter === finding.kind) as finding}<li>{i18n.t(labels[finding.kind])} · {i18n.t(evidenceLabels[finding.evidence])}: <code>{finding.location}</code> <button type="button" disabled={!enabled || busy} onclick={() => onFinding(finding.location)}>{i18n.t('package_scan.prepare_copy')}</button></li>{/each}</ul>{/if}
  {/if}
</section>
