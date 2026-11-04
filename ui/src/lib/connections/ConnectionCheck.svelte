<script lang="ts">
  import { tick, untrack } from 'svelte';
  import type { I18n } from '../i18n';
  import { Breadcrumb, Button, Checkbox, EmptyState, Icon, Progress, resolveText, type LocalizedText } from '../components';
  import { DATE_TIME_FORMAT, type MessageId } from '../i18n';
  import { connectionPath } from '../shell/router';
  import { parseTimestamp } from '../timestamp';
  import type { CheckStepName } from './client';
  import { STEP_ICON, STEP_LABEL, STEP_ORDER, STEP_STATUS_LABEL, stepText } from './messages';
  import { checkReportText, failedStep } from './state';
  import type { ConnectionsStore } from './store.svelte';

  // Step-by-step connection check (P1-02D, mockup 05-03). The local server runs the five steps in
  // order and returns them at once; the result is announced in a live region and focus moves to
  // the first fixing action after a failed step.
  let { i18n, store, localId, navigate }: {
    i18n: I18n;
    store: ConnectionsStore;
    localId: string;
    navigate: (path: string) => void;
  } = $props();

  const profile = $derived(store.profile(localId));
  const missing = $derived(store.list !== null && profile === null);
  const report = $derived(store.reports[localId] ?? null);
  const running = $derived(store.busy[localId] === 'test' || store.busy[localId] === 'connect');
  const failed = $derived(failedStep(report));
  const failedReport = $derived(report?.steps.find((step) => step.status === 'failed') ?? null);
  const identityChanged = $derived(failedReport?.code === 'server_identity_changed');
  let includeAddress = $state(false);
  let copyStatus = $state<LocalizedText | null>(null);
  let fixAction = $state<HTMLElement>();
  let previousReport: unknown = null;

  const translate = (id: MessageId): string => resolveText(i18n, { id } as LocalizedText);

  $effect(() => {
    const target = profile;
    if (target && store.autostart === target.local_id) {
      untrack(() => {
        store.autostart = null;
        void store.test(target);
      });
    }
  });

  $effect(() => {
    const current = report;
    if (current !== previousReport) {
      previousReport = current;
      if (current && !current.passed) void tick().then(() => fixAction?.focus());
    }
  });

  function stepIndex(step: CheckStepName): number {
    return STEP_ORDER.indexOf(step) + 1;
  }

  async function copyReport(): Promise<void> {
    if (!report) return;
    try {
      await navigator.clipboard.writeText(checkReportText(report, profile, includeAddress));
      copyStatus = { id: 'connection.test.report.copied' };
    } catch {
      copyStatus = { id: 'connection.test.report.copy_failed' };
    }
  }

  function fixPath(step: CheckStepName | null): string | null {
    if (step === 'endpoint' || step === 'api' || step === 'auth') return connectionPath(localId, 'edit');
    return null;
  }
</script>

<main id="main-content" tabindex="-1">
  <div class="screen connection-check">
    {#if missing}
      <EmptyState {i18n} level={1} heading={{ id: 'connection.list.title' }} reason={{ id: 'errors.profile.not_found' }}>
        {#snippet action()}<a class="button-link" href="/connections">{i18n.t('connection.action.back_to_list')}</a>{/snippet}
      </EmptyState>
    {:else if profile}
      <Breadcrumb
        {i18n}
        label={{ id: 'shell.a11y.breadcrumbs' }}
        items={[
          { label: { id: 'connection.list.title' }, href: '/connections' },
          { label: { value: profile.name, kind: 'user' }, href: connectionPath(localId, 'edit') },
          { label: { id: 'connection.test.crumb' } }
        ]}
      />
      <header class="screen-head">
        <h1>{i18n.t('connection.test.title', { profile: profile.name })}</h1>
        <p class="lead">{i18n.t('connection.test.subtitle')}</p>
      </header>

      <div class="check-layout">
        <section class="card steps-card" aria-labelledby="connection-steps-title">
          <h2 id="connection-steps-title" class="visually-hidden">{i18n.t('connection.a11y.steps')}</h2>
          <div class="check-status" role="status" aria-live="polite">
            {#if running}
              <Progress {i18n} label={{ id: 'connection.test.running' }} />
            {:else if report?.passed}
              <p>{i18n.t('connection.test.passed')}</p>
            {:else if failed}
              <p>{i18n.t('connection.test.failed_at', { step: resolveText(i18n, STEP_LABEL[failed]) })}</p>
            {:else if !report}
              <p>{i18n.t('connection.test.idle')}</p>
            {/if}
          </div>

          {#if report}
            <ol class="steps">
              {#each report.steps as step (step.step)}
                <li class="step {step.status}">
                  <span class="step-icon"><Icon name={STEP_ICON[step.status]} size={24} /></span>
                  <div class="step-body">
                    <h3>{i18n.t('connection.test.step_heading', { number: stepIndex(step.step), name: resolveText(i18n, STEP_LABEL[step.step]) })}</h3>
                    <p class="step-status">{resolveText(i18n, STEP_STATUS_LABEL[step.status])}</p>
                    <p>{resolveText(i18n, stepText(report, step, translate))}</p>
                    {#if step.status === 'failed'}
                      <div class="actions">
                        {#if identityChanged}
                          <button bind:this={fixAction} type="button" class="primary" onclick={() => void store.connect(profile, true)}>
                            {i18n.t('connection.action.confirm_new_server')}
                          </button>
                        {:else if fixPath(step.step)}
                          <a bind:this={fixAction} class="button-link primary" href={fixPath(step.step)}>
                            {step.step === 'auth' ? i18n.t('connection.action.fix_sign_in') : i18n.t('connection.action.fix_address')}
                          </a>
                        {/if}
                        <Button {i18n} label={{ id: 'connection.action.retry' }} disabled={running} onclick={() => void store.test(profile)} />
                      </div>
                    {/if}
                  </div>
                </li>
              {/each}
            </ol>
          {/if}

          <div class="actions">
            {#if !report || report.passed}
              <Button {i18n} label={{ id: 'connection.action.test' }} busy={running} disabled={running} onclick={() => void store.test(profile)} />
            {/if}
            {#if report?.passed && profile.state !== 'connected'}
              <Button {i18n} variant="primary" label={{ id: 'connection.action.connect' }} disabled={running} onclick={() => void store.connect(profile)} />
            {/if}
            {#if running}
              <Button {i18n} label={{ id: 'connection.action.cancel' }} onclick={() => store.cancel(localId)} />
            {/if}
            <a class="button-link" href="/connections">{i18n.t('connection.action.back_to_list')}</a>
          </div>
        </section>

        {#if report}
          <aside class="card report-card" aria-labelledby="connection-report-title">
            <h2 id="connection-report-title" class="eyebrow">{i18n.t('connection.test.details.title')}</h2>
            <dl class="details">
              <div><dt>{i18n.t('connection.test.details.code')}</dt><dd class="technical">{report.result_code ?? '—'}</dd></div>
              <div><dt>{i18n.t('connection.test.details.step')}</dt><dd class="technical">{failed ?? '—'}</dd></div>
              <div>
                <dt>{i18n.t('connection.test.details.method')}</dt>
                <dd>{report.auth_method === 'api_token' ? i18n.t('connection.form.api_token') : i18n.t('connection.form.auth_password')}</dd>
              </div>
              <div><dt>{i18n.t('connection.test.details.time')}</dt><dd>{i18n.date(parseTimestamp(report.checked_at), DATE_TIME_FORMAT)}</dd></div>
              <div><dt>{i18n.t('connection.test.details.version')}</dt><dd class="technical">{report.server_version ?? '—'}</dd></div>
            </dl>
            <Checkbox {i18n} id="connection-report-address" label={{ id: 'connection.test.report.include_address' }} bind:checked={includeAddress} />
            <Button {i18n} label={{ id: 'connection.test.report.copy' }} onclick={() => void copyReport()} />
            <p class="hint" role="status">{#if copyStatus}{resolveText(i18n, copyStatus)}{/if}</p>
            <p class="hint">{i18n.t('connection.test.report.scope')}</p>
            {#if report.auth_method === 'password'}<p class="hint">{i18n.t('connection.test.lockout_hint')}</p>{/if}
          </aside>
        {/if}
      </div>
      {#if store.announcement}
        <p class="announcement" role="status">
          <span>{resolveText(i18n, store.announcement.text)}</span>
          {#if store.announcement.detail}<span>{resolveText(i18n, store.announcement.detail)}</span>{/if}
        </p>
      {/if}
    {:else}
      <p role="status">{i18n.t('common.state.loading')}</p>
    {/if}
  </div>
</main>

<style>
  .connection-check {
    max-width: 84rem;
  }

  .check-layout {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(16rem, 1fr);
    align-items: start;
    gap: var(--space-6);
  }

  .steps {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .step {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-panel);
  }

  .step.failed {
    border-color: var(--status-danger-border);
    background: var(--status-danger-surface);
  }

  .step.passed .step-icon {
    color: var(--status-success-icon);
  }

  .step.notice .step-icon {
    color: var(--status-info-icon);
  }

  .step.failed .step-icon,
  .step.failed h3 {
    color: var(--status-danger-text);
  }

  .step.not_checked .step-icon,
  .step.not_checked .step-body {
    color: var(--text-muted);
  }

  .step-body {
    display: grid;
    gap: var(--space-1);
  }

  .step-status {
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .report-card {
    gap: var(--space-3);
  }

  .details {
    display: grid;
    gap: var(--space-2);
    margin: 0;
  }

  .details > div {
    display: grid;
    grid-template-columns: minmax(6rem, 1fr) 2fr;
    gap: var(--space-3);
  }

  .details dt {
    color: var(--text-secondary);
  }

  .details dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .announcement {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .button-link {
    gap: var(--space-2);
  }

  @media (max-width: 64rem) {
    .check-layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
