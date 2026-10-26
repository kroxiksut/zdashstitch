<script lang="ts">
  import type { I18n } from '../i18n';
  import { StatusBadge, resolveText } from '../components';
  import { CHECK_GROUPS, type DraftDetail } from './drafts';
  import { CHECK_GROUP_LABEL, findingLevelLabel } from './labels';

  // «Checks» in the P2–P3 scope (P1-04F §3.4, P1-01H §6): a summary built from common components.
  // The local check runs in the application layer; without its result the section says so.
  let { i18n, draft }: { i18n: I18n; draft: DraftDetail } = $props();

  const checks = $derived(draft.checks);
  const exportBlocked = $derived(checks ? checks.findings.filter((finding) => finding.blocks.includes('export')).length : 0);
  const publicationBlocked = $derived(
    checks ? checks.findings.filter((finding) => finding.blocks.includes('publication')).length : 0
  );
</script>

<section class="workspace-section" aria-labelledby="checks-title">
  <h2 id="checks-title">{i18n.t('draft.nav.checks')}</h2>
  <p class="muted">{i18n.t('checks.subtitle')}</p>

  <ul class="readiness">
    <li><StatusBadge {i18n} label={{ id: 'checks.summary.save_always' }} tone="success" /></li>
    {#if checks}
      <li>
        {#if exportBlocked > 0}
          <StatusBadge {i18n} label={{ id: 'checks.summary.export_blocked', params: { count: exportBlocked } }} tone="danger" />
        {:else}
          <StatusBadge {i18n} label={{ id: 'checks.summary.export_ready' }} tone="success" />
        {/if}
      </li>
      <li>
        {#if publicationBlocked > 0}
          <StatusBadge {i18n} label={{ id: 'checks.summary.publication_blocked', params: { count: publicationBlocked } }} tone="danger" />
        {:else}
          <StatusBadge {i18n} label={{ id: 'checks.summary.publication_ready' }} tone="success" />
        {/if}
      </li>
    {:else}
      <li><StatusBadge {i18n} label={{ id: 'common.state.coming_later' }} tone="info" /></li>
    {/if}
  </ul>

  {#if checks}
    {#if checks.findings.length === 0}
      <p role="status">{i18n.t('checks.empty', { count: checks.checked_objects })}</p>
    {/if}
    {#each CHECK_GROUPS as group (group)}
      {@const findings = checks.findings.filter((finding) => finding.group === group)}
      {#if findings.length > 0 || checks.passed[group] > 0}
        <section class="check-group" aria-labelledby={`checks-group-${group}`}>
          <h3 id={`checks-group-${group}`}>{resolveText(i18n, CHECK_GROUP_LABEL[group])}</h3>
          {#if findings.length > 0}
            <ul class="findings">
              {#each findings as finding (finding.id)}
                <li>
                  <StatusBadge {i18n} label={findingLevelLabel(finding.blocks)} tone={finding.blocks.length > 0 ? 'danger' : 'warning'} />
                  <span>{resolveText(i18n, finding.message)}</span>
                  {#if finding.page !== undefined && finding.widget !== undefined}
                    <span class="muted">{i18n.t('checks.object_path', { page: finding.page, widget: finding.widget })}</span>
                  {:else if finding.page !== undefined}
                    <span class="muted">{finding.page}</span>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
          {#if checks.passed[group] > 0}<p class="muted">{i18n.t('checks.group.passed', { count: checks.passed[group] })}</p>{/if}
        </section>
      {/if}
    {/each}
  {/if}

  <details class="technical-details">
    <summary>{i18n.t('checks.last_preflight.title')}</summary>
    <p>{i18n.t('checks.last_preflight.none')}</p>
  </details>
</section>
