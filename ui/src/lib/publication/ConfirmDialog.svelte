<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Dialog, TechnicalDetails } from '../components';
  import type { ReplaceHostPreview } from '../bindings/model';
  import type { DraftDetail } from '../shell/drafts';
  import type { PlanResult } from './model';
  import type { PublicationStore } from './store.svelte';

  // P1-08F (mockup 11-06): the one-shot plan the server keeps. The dialog shows what will be
  // written, what stays, the accepted warnings, the plan ID and time, the re-read before sending
  // and the invalidation conditions. Focus starts on «Отмена»; Esc cancels; the browser returns
  // focus to «Подтвердить запись…» when the dialog closes.
  let { i18n, store, plan, preview, draft, profile, version, oncancel }: {
    i18n: I18n;
    store: PublicationStore;
    plan: PlanResult;
    preview: ReplaceHostPreview;
    draft: DraftDetail;
    /** User data: the active profile name. */
    profile: string;
    version: string;
    oncancel: () => void;
  } = $props();

  const preview_ = $derived(plan.plan);
  const dashboardName = $derived(draft.source?.dashboard_name ?? draft.name);
  const source = $derived(preview.operation.source_host.name);
  const target = $derived(preview.operation.target_host?.name ?? '—');
  const warnings = $derived(preview_.report.findings.filter((finding) => finding.severity === 'informational' && finding.code !== 'kept_on_source').length);
  const kept = $derived(preview.kept_on_source.entries.length);
  const untouchedWidgets = $derived(preview.diff.summary.widgets.total - preview.diff.summary.widgets.changed);
  const question = $derived(
    preview_.action === 'update_source'
      ? { id: 'publication.confirm.question_update' as const, params: { count: preview_.replacement_count, dashboard_name: dashboardName } }
      : { id: 'publication.confirm.question_create' as const, params: { name: preview_.target_name } }
  );
</script>

<Dialog {i18n} id="publication-confirm" open title={{ id: 'publication.confirm.title' }} description={question} onclose={oncancel}>
  <dl class="confirm-facts">
    <div>
      <dt>{i18n.t('publication.confirm.field.connection')}</dt>
      <dd>{i18n.t('publication.confirm.connection_value', { profile, version })}</dd>
    </div>
    <div>
      <dt>{i18n.t('publication.confirm.field.dashboard')}</dt>
      <dd>
        {preview_.action === 'update_source'
          ? i18n.t('publication.confirm.dashboard_existing', { dashboard_name: dashboardName })
          : i18n.t('publication.confirm.dashboard_new', { name: preview_.target_name })}
      </dd>
    </div>
    <div>
      <dt>{i18n.t('publication.confirm.field.action')}</dt>
      <dd>{preview_.action === 'update_source' ? i18n.t('publication.confirm.api_update') : i18n.t('publication.confirm.api_create')}</dd>
    </div>
    <div>
      <dt>{i18n.t('publication.confirm.field.changes')}</dt>
      <dd>
        {preview_.action === 'update_source'
          ? i18n.t('publication.confirm.changes_update', { count: preview_.replacement_count, source, target })
          : i18n.t('publication.confirm.changes_create', { pages: preview_.page_count })}
      </dd>
    </div>
    <div>
      <dt>{i18n.t('publication.confirm.field.unchanged')}</dt>
      <dd>
        {preview_.action === 'update_source'
          ? i18n.t('publication.confirm.unchanged_update', { count: untouchedWidgets })
          : i18n.t('publication.untouched.source', { dashboard_name: dashboardName })}
      </dd>
    </div>
    {#if warnings > 0}
      <div>
        <dt>{i18n.t('publication.confirm.field.warnings')}</dt>
        <dd>{i18n.t('publication.confirm.warnings_reviewed', { count: warnings })}</dd>
      </div>
    {/if}
    {#if kept > 0}
      <div>
        <dt>{i18n.t('publication.confirm.field.kept', { host: source })}</dt>
        <dd>{i18n.t('publication.confirm.kept_confirmed', { count: kept })}</dd>
      </div>
    {/if}
    <div>
      <dt>{i18n.t('publication.confirm.field.plan')}</dt>
      <dd><span class="technical">{preview_.plan_id}</span> · {i18n.t('publication.confirm.plan_once', { time: preview_.created_at })}</dd>
    </div>
  </dl>
  {#if preview_.action === 'update_source'}
    <p>{i18n.t('publication.confirm.reread', { time: preview_.created_at })}</p>
  {:else}
    <p>{i18n.t('publication.confirm.create_name_check')}</p>
  {/if}
  <p class="muted">{i18n.t('publication.confirm.invalid_when')}</p>
  <p><strong>{i18n.t('publication.confirm.no_cancel_after_send')}</strong></p>
  <TechnicalDetails
    {i18n}
    summary={{ id: 'publication.tech.title' }}
    values={preview_.action === 'update_source'
      ? [
          { label: { id: 'publication.tech.base' }, value: { value: preview_.source_fingerprint, kind: 'technical' } },
          { label: { id: 'publication.tech.local' }, value: { value: preview_.expected_fingerprint, kind: 'technical' } }
        ]
      : [{ label: { id: 'publication.tech.local' }, value: { value: preview_.local_fingerprint, kind: 'technical' } }]}
  />
  {#snippet actions()}
    <Button {i18n} label={{ id: 'common.action.cancel' }} disabled={store.writing} onclick={oncancel} />
    <Button {i18n} variant="primary" label={{ id: 'publication.action.confirm_write' }} busy={store.writing} disabled={store.writing} onclick={() => void store.confirm()} />
  {/snippet}
</Dialog>
