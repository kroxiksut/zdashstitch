<script lang="ts">
  import type { I18n } from '../i18n';
  import { Icon, resolveText } from '../components';
  import { widgetTypeName } from '../shell/format';
  import { GROUP_FIX_SECTION, GROUP_LABEL, findingText, reasonText } from './messages';
  import { FINDING_GROUPS, type Finding, type ReplaceHostPreview } from './model';
  import { pageLabel, widgetLabel, widgetOf } from './view';

  // Publication findings of the preview (P1-08C) grouped by where they are fixed. «Блокирует» and
  // «Предупреждение» are an icon plus a word, never colour only. The list never builds a plan.
  let { i18n, preview }: { i18n: I18n; preview: ReplaceHostPreview } = $props();

  const host = $derived(preview.operation.source_host.name);
  // Kept references are listed once in «Останутся на старом узле», not once per field here.
  const findings = $derived(preview.report.findings.filter((finding) => finding.code !== 'kept_on_source'));

  function subjectText(finding: Finding): string | null {
    const subject = finding.subject;
    if (!subject) return null;
    const widget = widgetOf(preview, subject.page_index, subject.widget_index);
    return `${pageLabel(i18n, preview, subject.page_index)} · ${widgetLabel(i18n, widget)} · ${widgetTypeName(i18n, widget?.widget_type ?? '')}`;
  }
</script>

{#if findings.length === 0}
  <p>{i18n.t('publication.check.none')}</p>
{/if}
{#each FINDING_GROUPS as group (group)}
  {@const items = findings.filter((finding) => finding.group === group)}
  {#if items.length > 0}
    <section class="check-group" aria-labelledby={`findings-${group}`}>
      <div class="section-head">
        <h4 id={`findings-${group}`}>{resolveText(i18n, GROUP_LABEL[group])}</h4>
        <span class="muted">{i18n.t('publication.check.fixed_in', { section: resolveText(i18n, GROUP_FIX_SECTION[group]) })}</span>
      </div>
      <ul class="findings">
        {#each items as finding, index (`${finding.code}:${index}`)}
          {@const subject = subjectText(finding)}
          <li>
            {#if finding.severity === 'blocks_publication'}
              <span class="marker tone-danger"><Icon name="circle-x" size={18} />{i18n.t('publication.check.status.blocks')}</span>
            {:else}
              <span class="marker tone-warning"><Icon name="alert-triangle" size={18} />{i18n.t('publication.check.status.warning')}</span>
            {/if}
            <span>{resolveText(i18n, findingText(i18n, finding, host))}</span>
            {#if finding.reason}<span class="muted">{resolveText(i18n, reasonText(finding.reason))}</span>{/if}
            {#if subject}<span class="muted">{subject}</span>{/if}
            {#if finding.subject?.field}<span class="muted technical">{finding.subject.field}</span>{/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}
{/each}
