<script lang="ts">
  import { untrack } from 'svelte';
  import type { I18n } from '../i18n';
  import { Banner, Button, EmptyState, Icon, Skeleton, StatusBadge, Tabs, TechnicalDetails, resolveText, type LocalizedText } from '../components';
  import type { DraftDetail } from '../shell/drafts';
  import { draftPath } from '../shell/router';
  import ConfirmDialog from '../publication/ConfirmDialog.svelte';
  import ConflictPanel from '../publication/ConflictPanel.svelte';
  import { publicationCodeText } from '../publication/messages';
  import { zabbixDashboardUrl, type JournalEntry } from '../publication/model';
  import RecoveryBanner from '../publication/RecoveryBanner.svelte';
  import RecoveryDialogs from '../publication/RecoveryDialogs.svelte';
  import type { PublicationStep, PublicationStore, RecoveryStore } from '../publication/store.svelte';
  import TargetStep from '../publication/TargetStep.svelte';
  import WriteStep from '../publication/WriteStep.svelte';
  import DiffView from './DiffView.svelte';
  import FindingsList from './FindingsList.svelte';
  import KeptList from './KeptList.svelte';
  import { replaceHostCodeText } from './messages';
  import { blockingFindings, informationalFindings, type ReplaceHostPreview, type StepItem } from './model';
  import StepBar from './StepBar.svelte';
  import type { ReplaceHostStore } from './store.svelte';

  // «Публикация» for ReplaceHost (P1-08A–G, P2-03D/P2-04E/F; mockups 11-02…11-08): 1 «Назначение»
  // (explicit action), 2 «Проверки» (fresh read of the source and of the replacement; a conflict
  // opens P1-08D), 3 «Изменения» (base → draft, base → Zabbix now), 4 «Подтверждение» (the server
  // builds a one-shot plan; the dialog confirms exactly it) and 5 «Запись и проверка». The browser
  // never writes to Zabbix and never holds a plan: only an opaque handle. Without the draft's own
  // connection in read-write mode the steps show the requirements and nothing is written.
  let { i18n, store, draft, localId, serverReads, connection, publication, recovery, profileName, draftName, copyName = '' }: {
    i18n: I18n;
    store: ReplaceHostStore;
    draft: DraftDetail;
    localId: string;
    serverReads: boolean;
    /** Connection cell of the context card: profile, mode and, when known, version and address. */
    connection: {
      name: string;
      mode: LocalizedText;
      writable?: boolean;
      version?: string | null;
      endpoint?: string | null;
      localId?: string | null;
    } | null;
    publication?: PublicationStore;
    recovery?: RecoveryStore;
    profileName?: (connectionLocalId: string) => string | null;
    draftName?: (localId: string) => string | null;
    /** «… (копия N)» for «Скопировать заново» (P2-08A). */
    copyName?: string;
  } = $props();

  let diffTab = $state('diff-local');
  let journalClosing = $state<JournalEntry | null>(null);

  $effect(() => {
    if (!publication) return;
    const id = localId;
    untrack(() => publication.open(id));
  });

  // Checks run when the step opens (P1-08C) through the draft's own connection; read-only.
  $effect(() => {
    if (!publication || !serverReads || publication.step !== 'checks') return;
    untrack(() => {
      if (publication.check === null && !publication.checking && publication.checkFailure === null) void publication.recheck();
    });
  });

  const view = $derived<PublicationStep>(publication?.step ?? 'checks');
  const check = $derived(publication?.check ?? null);
  const preview = $derived<ReplaceHostPreview | null>(check?.preview ?? store.preview);
  const conflict = $derived(check !== null && check.source.state !== 'current');
  const action = $derived(publication?.action ?? null);
  const writable = $derived(serverReads && (check?.mode === 'read_write' || (check === null && connection?.writable === true)));

  // For a copy only the blockers of the replacement itself apply (unresolved references, the
  // unconfirmed kept list); the update contracts of the source dashboard do not (P2-04E).
  const COPY_BLOCKING = new Set([
    'target_host_not_visible',
    'binding_unresolved',
    'source_unverified',
    'decision_invalid',
    'binding_stale',
    'binding_invalid',
    'kept_on_source_unconfirmed',
    'kept_on_source_confirmation_stale',
    'scope_link_crossing'
  ]);
  const actionBlockers = $derived(
    preview === null
      ? 1
      : action === 'save_as_copy'
        ? blockingFindings(preview.report).filter((finding) => COPY_BLOCKING.has(finding.code)).length
        : blockingFindings(preview.report).length
  );
  const blockers = $derived(preview ? blockingFindings(preview.report).length : 0);
  const warnings = $derived(preview ? informationalFindings(preview.report).filter((finding) => finding.code !== 'kept_on_source').length : 0);
  const checksPass = $derived(
    check !== null && check.journal_block === null && actionBlockers === 0 && (action === 'save_as_copy' || !conflict)
  );
  const canConfirm = $derived(publication !== undefined && action !== null && writable && checksPass && !(action === 'save_as_copy' && (publication?.copyName.trim() ?? '') === ''));

  const steps = $derived<StepItem[]>([
    { id: 'target', label: { id: 'publication.step.target' }, state: !publication ? 'unavailable' : view === 'target' ? 'current' : action !== null ? 'done' : 'todo' },
    {
      id: 'checks',
      label: { id: 'publication.step.checks' },
      state: view === 'checks' ? 'current' : conflict && action !== 'save_as_copy' ? 'error' : checksPass ? 'done' : 'todo'
    },
    { id: 'changes', label: { id: 'publication.step.changes' }, state: view === 'changes' ? 'current' : conflict && action !== 'save_as_copy' ? 'unavailable' : 'todo' },
    { id: 'confirm', label: { id: 'publication.step.confirm' }, state: !publication || !checksPass ? 'unavailable' : view === 'confirm' ? 'current' : 'todo' },
    { id: 'write', label: { id: 'publication.step.write' }, state: view === 'write' ? 'current' : publication?.outcome ? 'done' : 'unavailable' }
  ]);

  function select(id: string): void {
    if (!publication) return;
    if (id === 'target' || id === 'checks' || id === 'changes' || id === 'confirm' || id === 'write') publication.goto(id);
  }

  const targetSummary = $derived<string>(
    action === 'update_source'
      ? i18n.t('publication.context.target_same')
      : action === 'save_as_copy' && publication && publication.copyName.trim() !== ''
        ? i18n.t('publication.context.target_new', { name: publication.copyName.trim() })
        : i18n.t('publication.context.target_not_chosen')
  );
  const expected = $derived<string>(
    preview === null
      ? ''
      : action === 'save_as_copy'
        ? i18n.t('publication.expected.save_as_copy', { pages: preview.diff.summary.pages.total, name: publication?.copyName.trim() || '—' })
        : action === 'update_source'
          ? i18n.t('publication.expected.replace_host', {
              count: preview.diff.summary.widgets.changed,
              source: preview.operation.source_host.name,
              target: preview.operation.target_host?.name ?? '—'
            })
          : i18n.t('publication.expected.choose_action')
  );

  const zabbixUrl = (dashboardid: string | null): string | null => zabbixDashboardUrl(connection?.endpoint ?? null, dashboardid);
  const nameOfProfile = (id: string): string | null => profileName?.(id) ?? null;
  const nameOfDraft = (id: string): string | null => draftName?.(id) ?? (id === localId ? draft.name : null);
</script>

<section class="workspace-section publication" aria-labelledby="publication-title">
  <h2 id="publication-title">{i18n.t('publication.title')}</h2>

  {#if recovery}
    <RecoveryBanner {i18n} store={recovery} {localId} profileName={nameOfProfile} draftName={nameOfDraft} activeConnectionId={connection?.localId ?? null} />
  {/if}

  {#if store.status === 'idle' || store.status === 'loading'}
    <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={4} />
  {:else if store.status === 'failed' && store.loadFailure}
    <Banner {i18n} heading={{ id: 'bindings.load_failed' }} body={replaceHostCodeText(store.loadFailure.code, store.loadFailure.params)} tone="danger">
      {#snippet actions()}<Button {i18n} label={{ id: 'bindings.action.retry' }} onclick={() => void store.load()} />{/snippet}
    </Banner>
  {:else if !preview}
    <EmptyState {i18n} heading={{ id: 'publication.none.title' }} reason={{ id: 'publication.none.body' }}>
      {#snippet action()}<a href={draftPath(localId, 'bindings')}>{i18n.t('draft.nav.bindings')}</a>{/snippet}
    </EmptyState>
  {:else}
    <StepBar {i18n} label={{ id: 'publication.a11y.steps' }} {steps} onselect={select} />

    <dl class="context-card card">
      <div>
        <dt class="eyebrow">{i18n.t('publication.context.operation')}</dt>
        <dd>{i18n.t('intent.replace_host')}</dd>
        <dd class="muted">{preview.operation.source_host.name} → {preview.operation.target_host?.name ?? '—'}</dd>
      </div>
      <div>
        <dt class="eyebrow">{i18n.t('publication.context.source')}</dt>
        <dd>{draft.source?.dashboard_name ?? '—'}</dd>
        {#if draft.source}<dd class="muted">{i18n.t('draft.overview.snapshot_from', { date: draft.source.snapshot_at })}</dd>{/if}
      </div>
      <div>
        <dt class="eyebrow">{i18n.t('publication.context.target')}</dt>
        <dd>{targetSummary}</dd>
        {#if action !== null}<dd class="muted">{i18n.t('publication.context.target_hint')}</dd>{/if}
      </div>
      <div>
        <dt class="eyebrow">{i18n.t('publication.context.connection')}</dt>
        {#if connection && serverReads}
          <dd>{connection.name}</dd>
          <dd class="muted">{resolveText(i18n, connection.mode)}</dd>
        {:else}
          <dd>{i18n.t('common.state.offline')}</dd>
        {/if}
      </div>
      <div>
        <dt class="eyebrow">{i18n.t('publication.context.scope')}</dt>
        <dd>{i18n.t('publication.context.scope_value', { changed: preview.diff.summary.widgets.changed, total: preview.diff.summary.widgets.total })}</dd>
        <dd class="muted">{i18n.t('publication.context.pages_value', { count: preview.diff.summary.pages.total })}</dd>
      </div>
    </dl>
    <p class="note"><Icon name="info-circle" />{i18n.t('publication.expected.title', { summary: expected })}</p>

    <div class="bindings-live" role="status" aria-live="polite">
      {#if store.announcement && store.busy === null}<p class="app-notice">{resolveText(i18n, store.announcement)}</p>{/if}
      {#if publication?.checking}<p class="app-notice">{i18n.t('publication.checks.checking')}</p>{/if}
    </div>
    {#if store.commandFailure}
      <Banner {i18n} heading={{ id: 'bindings.command_failed' }} body={replaceHostCodeText(store.commandFailure.code, store.commandFailure.params)} tone="danger" />
    {/if}
    {#if publication?.checkFailure}
      <Banner {i18n} heading={{ id: 'bindings.command_failed' }} body={publicationCodeText(i18n, publication.checkFailure)} tone="danger" />
    {/if}
    {#if check?.journal_block}
      <Banner
        {i18n}
        heading={{ id: 'publication.reconcile.write_blocked' }}
        body={check.journal_block.code === 'journal_damaged' ? { id: 'publication.journal_block_damaged' } : { id: 'publication.journal_block' }}
        tone="warning"
        blocking
      />
    {/if}

    {#if view === 'target' && publication}
      <TargetStep {i18n} store={publication} {preview} {draft} onnext={() => publication.goto('checks')} />
    {:else if view === 'checks' && conflict && check && publication && action !== 'save_as_copy'}
      <ConflictPanel
        {i18n}
        store={publication}
        {check}
        {draft}
        {copyName}
        onsavecopy={() => {
          publication.setAction('save_as_copy');
          publication.goto('target');
        }}
      />
    {:else if view === 'checks'}
      <p class="note"><Icon name="info-circle" />{i18n.t('publication.preview.read_only_note')}</p>
      <div class="checks-layout">
        <section class="card" aria-labelledby="preflight-title">
          <div class="section-head">
            <h3 id="preflight-title">{i18n.t('publication.preflight.title')}</h3>
            {#if preview.checked_at !== null}<span class="muted">{i18n.t('bindings.checked_at', { time: preview.checked_at })}</span>{/if}
          </div>
          {#if publication}
            <p class="marker">
              {#if check?.source.state === 'current'}
                <Icon name="circle-check" size={18} />{i18n.t('publication.checks.source_current', { time: check.source.checked_at })}
              {:else}
                <Icon name="circle-dashed" size={18} />{i18n.t('publication.checks.source_not_checked')}
              {/if}
            </p>
          {/if}
          <FindingsList {i18n} {preview} />
          <KeptList {i18n} {store} {preview} idPrefix="publication" />
          <div class="actions">
            <a class="button-link" href={draftPath(localId, 'bindings')}>{i18n.t('publication.check.action.open_bindings')}</a>
          </div>
        </section>
        <aside class="card readiness-card" aria-labelledby="readiness-title">
          <h3 id="readiness-title" class="eyebrow">{i18n.t('bindings.step.summary')}</h3>
          {#if preview.report.allows_publication || (action === 'save_as_copy' && actionBlockers === 0)}
            <p class="marker"><Icon name="circle-check" size={18} />{i18n.t('publication.check.summary.ready')}</p>
          {:else}
            <p class="marker tone-danger" role="alert"><Icon name="circle-x" size={18} />{i18n.t('publication.preflight.blocked')}</p>
          {/if}
          <ul class="readiness">
            <li><StatusBadge {i18n} label={{ id: 'publication.check.count.blocking', params: { count: action === 'save_as_copy' ? actionBlockers : blockers } }} tone={(action === 'save_as_copy' ? actionBlockers : blockers) > 0 ? 'danger' : 'success'} /></li>
            <li><StatusBadge {i18n} label={{ id: 'publication.check.count.warnings', params: { count: warnings } }} tone={warnings > 0 ? 'warning' : 'success'} /></li>
          </ul>
          {#if actionBlockers > 0}<p class="muted">{i18n.t('publication.check.summary.plan_not_built')}</p>{/if}
          <Button
            {i18n}
            label={{ id: 'publication.check.recheck' }}
            disabled={!serverReads || store.busy !== null || (publication?.checking ?? false)}
            busy={store.busy === 'check' || (publication?.checking ?? false)}
            onclick={() => void (publication ? publication.recheck() : store.revalidate())}
          >
            {#snippet leadingIcon()}<Icon name="refresh" />{/snippet}
          </Button>
          {#if !serverReads}<p class="muted">{i18n.t('bindings.offline.check_all')}</p>{/if}
        </aside>
      </div>
      {#if publication}
        <div class="actions">
          <Button {i18n} label={{ id: 'publication.action.back_target' }} onclick={() => publication.goto('target')} />
          <Button {i18n} variant="primary" label={{ id: 'publication.action.next_changes' }} onclick={() => publication.goto('changes')} />
        </div>
      {/if}
    {:else if view === 'changes'}
      <Tabs
        {i18n}
        label={{ id: 'publication.step.changes' }}
        bind:selected={diffTab}
        tabs={[
          { id: 'diff-local', label: { id: 'publication.diff.tab.local' }, panelId: 'diff-local-panel' },
          { id: 'diff-remote', label: { id: 'publication.diff.tab.remote' }, panelId: 'diff-remote-panel' }
        ]}
      />
      {#if diffTab === 'diff-local'}
        <div id="diff-local-panel" role="tabpanel" aria-labelledby="diff-local">
          <DiffView {i18n} {preview} />
        </div>
      {:else}
        <div id="diff-remote-panel" role="tabpanel" aria-labelledby="diff-remote">
          {#if check?.source.state === 'current'}
            <p><Icon name="circle-check" />{i18n.t('publication.diff.remote_current')}</p>
          {:else if check?.remote && publication}
            <ConflictPanel {i18n} store={publication} {check} {draft} {copyName} onsavecopy={() => { publication.setAction('save_as_copy'); publication.goto('target'); }} />
          {:else}
            <p>{i18n.t('publication.diff.remote_later')}</p>
          {/if}
        </div>
      {/if}
      {#if publication}
        <div class="actions">
          <Button {i18n} label={{ id: 'publication.action.back_checks' }} onclick={() => publication.goto('checks')} />
          <Button {i18n} variant="primary" label={{ id: 'publication.action.next_confirm' }} disabled={!checksPass} onclick={() => publication.goto('confirm')} />
        </div>
      {/if}
    {:else if view === 'confirm' && publication}
      <section class="card" aria-labelledby="publication-confirm-step">
        <h3 id="publication-confirm-step" class="eyebrow">{i18n.t('publication.confirm.summary_title')}</h3>
        <p>{i18n.t('publication.confirm.target_line', { target: targetSummary })}</p>
        <p>{i18n.t('publication.expected.title', { summary: expected })}</p>
        {#if !writable}<p class="muted">{i18n.t('publication.confirm.offline')}</p>{/if}
        {#if !canConfirm}<p class="muted">{i18n.t('publication.confirm.requirements')}</p>{/if}
        {#if publication.planning}<p class="muted" role="status">{i18n.t('publication.confirm.building')}</p>{/if}
        {#if publication.planFailure}
          <Banner {i18n} heading={{ id: 'publication.preflight.blocked' }} body={publicationCodeText(i18n, publication.planFailure)} tone="danger" blocking />
        {/if}
        <div class="actions">
          <Button {i18n} label={{ id: 'publication.action.back_changes' }} onclick={() => publication.goto('changes')} />
          <Button
            {i18n}
            id="publication-confirm-open"
            variant="primary"
            label={{ id: 'publication.action.confirm_dialog' }}
            disabled={!canConfirm || publication.planning}
            busy={publication.planning}
            onclick={() => void publication.buildPlan(preview)}
          />
        </div>
      </section>
      {#if publication.plan}
        <ConfirmDialog
          {i18n}
          store={publication}
          plan={publication.plan}
          {preview}
          {draft}
          profile={connection?.name ?? '—'}
          version={connection?.version ?? preview.operation.server_version}
          oncancel={() => void publication.discardPlan()}
        />
      {/if}
    {:else if view === 'write' && publication}
      <WriteStep {i18n} store={publication} {localId} {zabbixUrl} onjournal={(entry) => void recovery?.show(entry.operation_id)} onclosemanually={(entry) => (journalClosing = entry)} />
    {/if}

    <TechnicalDetails
      {i18n}
      summary={{ id: 'publication.tech.title' }}
      values={[
        { label: { id: 'publication.tech.base' }, value: { value: preview.base_fingerprint, kind: 'technical' } },
        { label: { id: 'publication.tech.local' }, value: { value: preview.local_fingerprint, kind: 'technical' } },
        { label: { id: 'publication.tech.revision' }, value: { value: String(preview.revision), kind: 'technical' } }
      ]}
    />
  {/if}

  {#if recovery}
    <RecoveryDialogs
      {i18n}
      store={recovery}
      closing={journalClosing}
      quarantining={null}
      targetText={(entry) => (entry.target.name ?? '—')}
      sourceText={() => draft.name}
      profileText={(entry) => nameOfProfile(entry.connection_local_id) ?? i18n.t('publication.journal.profile_removed')}
      journal={false}
      onclosed={() => {
        journalClosing = null;
        const closed = recovery?.lastClosed;
        const outcome = publication?.outcome;
        if (publication && outcome && closed && closed.operation_id === outcome.operation_id) {
          publication.outcome = { ...outcome, state: closed.state, entry: closed };
        }
      }}
    />
  {/if}
</section>
