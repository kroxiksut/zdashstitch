<script lang="ts">
  import type { I18n } from '../i18n';
  import { Banner, Button, Icon, Progress, StatusBadge, resolveText, type IconName, type LocalizedText } from '../components';
  import { draftPath } from '../shell/router';
  import { evidenceText, publicationCodeText, resultBody, STATE_LABEL, STATE_TONE } from './messages';
  import { NEEDS_RECONCILIATION, type JournalEntry, type Outcome } from './model';
  import type { PublicationStore } from './store.svelte';

  // Step 5 «Запись и проверка» (P1-08G, mockups 11-07/11-08): stages without percentages, the
  // outcome banner, the journal box and the only safe next actions. A result is never called
  // successful without the read-back; display is never checked automatically; an unknown outcome
  // offers only «Проверить результат» (reads) and, after a manual check, «Закрыть без сверки».
  let { i18n, store, localId, zabbixUrl, onjournal, onclosemanually }: {
    i18n: I18n;
    store: PublicationStore;
    localId: string;
    /** «Открыть в Zabbix» for a dashboard ID; `null` without a usable profile address. */
    zabbixUrl: (dashboardid: string | null) => string | null;
    onjournal: (entry: JournalEntry) => void;
    onclosemanually: (entry: JournalEntry) => void;
  } = $props();

  type StageState = 'done' | 'failed' | 'pending' | 'not_checked';
  interface Stage {
    readonly label: LocalizedText;
    readonly detail: LocalizedText;
    readonly state: StageState;
    readonly at?: number;
  }

  const STAGE_ICON: Readonly<Record<StageState, IconName>> = {
    done: 'circle-check',
    failed: 'circle-x',
    pending: 'circle-dashed',
    not_checked: 'circle-dashed'
  };

  function at(outcome: Outcome, state: string): number | undefined {
    return outcome.entry?.transitions.find((transition) => transition.state === state)?.at;
  }

  function stages(outcome: Outcome): Stage[] {
    const create = outcome.kind === 'create';
    const verified = outcome.state === 'verified' || outcome.state === 'reconciled_verified';
    const unknown = NEEDS_RECONCILIATION.has(outcome.state);
    const list: Stage[] = [
      create
        ? { label: { id: 'publication.stage.name_check' }, detail: { id: 'publication.stage.name_check_done' }, state: outcome.sent ? 'done' : 'failed', at: at(outcome, 'confirmed') }
        : { label: { id: 'publication.stage.pre_read' }, detail: outcome.state === 'conflict' ? { id: 'publication.state.conflict' } : { id: 'publication.stage.pre_read_done' }, state: outcome.state === 'conflict' ? 'failed' : outcome.sent ? 'done' : 'failed', at: at(outcome, 'confirmed') },
      {
        label: { id: 'publication.stage.write' },
        detail: !outcome.sent ? { id: 'publication.stage.not_sent' } : unknown ? { id: 'publication.stage.write_unknown' } : { id: 'publication.stage.write_done' },
        state: !outcome.sent ? 'failed' : unknown ? 'pending' : 'done',
        at: at(outcome, 'submitting')
      },
      {
        label: { id: 'publication.stage.read_back' },
        detail: verified ? { id: 'publication.stage.read_back_done' } : { id: 'publication.stage.not_done' },
        state: verified ? 'done' : outcome.sent ? 'pending' : 'failed'
      }
    ];
    if (verified) {
      list.push(
        { label: { id: 'publication.stage.structure' }, detail: { id: 'publication.stage.structure_done' }, state: 'done' },
        { label: { id: 'publication.stage.sources' }, detail: { id: 'publication.stage.sources_done' }, state: 'done' },
        { label: { id: 'publication.stage.display' }, detail: { id: 'publication.stage.display_not_checked' }, state: 'not_checked' }
      );
    }
    return list;
  }

  const outcome = $derived(store.outcome);
  const entry = $derived(outcome?.entry ?? null);
  const needsCheck = $derived(outcome !== null && NEEDS_RECONCILIATION.has(outcome.state));
  const dashboardid = $derived(outcome?.created_dashboardid ?? entry?.target.dashboardid ?? null);
  const openUrl = $derived(outcome !== null && (outcome.state === 'verified' || outcome.state === 'reconciled_verified' || needsCheck) ? zabbixUrl(dashboardid) : null);
  const finished = $derived(entry?.transitions.at(-1)?.at);
  const tone = $derived(outcome ? STATE_TONE[outcome.state] : 'info');
</script>

<section class="publication-write" aria-labelledby="publication-write-title">
  <h3 id="publication-write-title" class="visually-hidden">{i18n.t('publication.step.write')}</h3>
  {#if store.writing}
    <div class="card">
      <Progress {i18n} label={{ id: 'publication.stage.sending' }} />
      <p class="muted" role="status" aria-live="polite">{#if store.waitingLong}{i18n.t('publication.stage.waiting')}{/if}</p>
      <p class="muted">{i18n.t('publication.confirm.no_cancel_after_send')}</p>
    </div>
  {:else if outcome}
    <Banner
      {i18n}
      heading={STATE_LABEL[outcome.state]}
      body={resultBody(i18n, outcome.state, outcome.kind, outcome.evidence, outcome.invalidated)}
      {tone}
      blocking={tone === 'danger' || tone === 'warning'}
    />
    {#if outcome.journal_error}
      <p class="note"><Icon name="alert-triangle" />{i18n.t('publication.result.journal_error', { code: outcome.journal_error.code })}</p>
    {/if}
    <div class="write-layout">
      <section class="card" aria-labelledby="publication-stages-title">
        <h4 id="publication-stages-title" class="eyebrow">{i18n.t('publication.stages.title')}</h4>
        <ol class="stages">
          {#each stages(outcome) as stage, index (index)}
            <li class="stage {stage.state}">
              <Icon name={STAGE_ICON[stage.state]} size={18} />
              <span class="stage-label">{resolveText(i18n, stage.label)}</span>
              <span class="muted">
                {resolveText(i18n, stage.detail)}
                {#if stage.at !== undefined}· {i18n.t('publication.stage.at', { time: stage.at })}{/if}
              </span>
            </li>
          {/each}
        </ol>
        {#if needsCheck}
          <h4 class="eyebrow">{i18n.t('publication.reconcile.title')}</h4>
          <table class="data-table">
            <thead>
              <tr>
                <th scope="col">{i18n.t('publication.reconcile.column.found')}</th>
                <th scope="col">{i18n.t('publication.reconcile.column.result')}</th>
                <th scope="col">{i18n.t('publication.reconcile.column.next')}</th>
              </tr>
            </thead>
            <tbody>
              {#if outcome.kind === 'create'}
                <tr><td>{i18n.t('publication.reconcile.found_one')}</td><td>{i18n.t('publication.state.reconciled_verified')}</td><td>{i18n.t('publication.reconcile.next_continue')}</td></tr>
                <!-- OPEN_DECISIONS 28.10.2026 (evening), item 1: zero candidates is a manual check. -->
                <tr><td>{i18n.t('publication.reconcile.found_none_or_other')}</td><td>{i18n.t('publication.state.manual_check_required')}</td><td>{i18n.t('publication.reconcile.next_open')}</td></tr>
              {:else}
                <tr><td>{i18n.t('publication.reconcile.found_expected')}</td><td>{i18n.t('publication.state.reconciled_verified')}</td><td>{i18n.t('publication.reconcile.next_continue')}</td></tr>
                <tr><td>{i18n.t('publication.reconcile.found_base')}</td><td>{i18n.t('publication.state.reconciled_failed')}</td><td>{i18n.t('publication.reconcile.next_new_preview')}</td></tr>
                <tr><td>{i18n.t('publication.reconcile.found_other')}</td><td>{i18n.t('publication.state.manual_check_required')}</td><td>{i18n.t('publication.reconcile.next_open')}</td></tr>
              {/if}
            </tbody>
          </table>
          <p class="note"><Icon name="ban" />{i18n.t('publication.reconcile.write_blocked')}</p>
        {/if}
        {#if outcome.base_advanced}
          <p class="note"><Icon name="info-circle" />{i18n.t('publication.result.base_advanced')}</p>
        {:else if outcome.base_advance_error}
          <p class="note"><Icon name="alert-triangle" />{i18n.t('publication.result.base_not_advanced')}</p>
        {/if}
        {#if outcome.kind === 'create' && (outcome.state === 'verified' || outcome.state === 'reconciled_verified') && outcome.target_name}
          <p class="note"><Icon name="info-circle" />{i18n.t('publication.result.copy_created', { name: outcome.target_name })}</p>
        {/if}
      </section>

      <aside class="card" aria-labelledby="publication-journal-title">
        <h4 id="publication-journal-title" class="eyebrow">{i18n.t('publication.journal.title')}</h4>
        <dl class="journal-facts">
          <div><dt>{i18n.t('publication.journal.field.state')}</dt><dd><StatusBadge {i18n} label={STATE_LABEL[outcome.state]} {tone} /></dd></div>
          {#if entry}
            <div><dt>{i18n.t('publication.journal.field.started')}</dt><dd>{i18n.t('publication.stage.at', { time: entry.confirmed_at })}</dd></div>
            {#if entry.submitted_at !== null}<div><dt>{i18n.t('publication.journal.field.sent')}</dt><dd>{i18n.t('publication.stage.at', { time: entry.submitted_at })}</dd></div>{/if}
            {#if finished !== undefined && !needsCheck}<div><dt>{i18n.t('publication.journal.field.finished')}</dt><dd>{i18n.t('publication.stage.at', { time: finished })}</dd></div>{/if}
            {#if entry.evidence}<div><dt>{i18n.t('publication.journal.field.evidence')}</dt><dd>{resolveText(i18n, evidenceText(entry.evidence))}</dd></div>{/if}
          {/if}
          <div><dt>{i18n.t('publication.journal.field.repeats')}</dt><dd>{i18n.number(0)}</dd></div>
          <div><dt>{i18n.t('publication.confirm.field.plan')}</dt><dd class="technical">{outcome.operation_id}</dd></div>
        </dl>
        {#if needsCheck}<p class="muted">{i18n.t('publication.journal.disk_note')}</p>{/if}
        {#if entry}<Button {i18n} variant="quiet" label={{ id: 'publication.action.open_journal' }} onclick={() => onjournal(entry)} />{/if}
      </aside>
    </div>

    <div class="bindings-live" role="status" aria-live="polite">
      {#if store.reconciling}<p class="app-notice">{i18n.t('publication.reconcile.update_note')}</p>{/if}
    </div>
    {#if store.writeFailure}
      <Banner {i18n} heading={{ id: 'bindings.command_failed' }} body={publicationCodeText(i18n, store.writeFailure)} tone="danger" />
    {/if}
    <div class="actions">
      {#if needsCheck}
        <Button {i18n} variant="primary" label={{ id: 'publication.action.reconcile' }} busy={store.reconciling} disabled={store.reconciling} onclick={() => void store.reconcile()}>
          {#snippet leadingIcon()}<Icon name="refresh" />{/snippet}
        </Button>
        {#if outcome.state === 'manual_check_required' && entry}
          <Button {i18n} label={{ id: 'startup.recovery.close_unverified' }} onclick={() => onclosemanually(entry)} />
        {/if}
      {:else if outcome.state !== 'verified' && outcome.state !== 'reconciled_verified'}
        <Button {i18n} variant="primary" label={{ id: 'publication.action.new_preview' }} onclick={() => store.restart()} />
      {/if}
      {#if openUrl}<a class="button-link" href={openUrl} target="_blank" rel="noopener noreferrer">{i18n.t('publication.action.open_in_zabbix')}</a>{/if}
      <a class="button-link" href={draftPath(localId, 'overview')}>{i18n.t('publication.action.back_to_draft')}</a>
    </div>
    {#if needsCheck}<p class="muted">{outcome.kind === 'create' ? i18n.t('publication.reconcile.create_note') : i18n.t('publication.reconcile.update_note')}</p>{/if}
  {:else if store.writeFailure}
    <Banner {i18n} heading={{ id: 'publication.state.failed_before_send' }} body={publicationCodeText(i18n, store.writeFailure)} tone="danger" blocking />
    <div class="actions">
      <Button {i18n} variant="primary" label={{ id: 'publication.action.new_preview' }} onclick={() => store.restart()} />
    </div>
  {/if}
</section>
