<script lang="ts">
  import { tick } from 'svelte';
  import { DATE_TIME_FORMAT, type I18n } from '../i18n';
  import { Banner, Button, Dialog, Icon, Progress, Skeleton, resolveText, type LocalizedText } from '../components';
  import { historyOpText, replaceHostCodeText } from './messages';
  import MappingStep from './MappingStep.svelte';
  import type { StepItem } from './model';
  import NewHostStep from './NewHostStep.svelte';
  import ScopeStep from './ScopeStep.svelte';
  import StepBar from './StepBar.svelte';
  import { WIZARD_STEPS, type ReplaceHostStore, type WizardStep } from './store.svelte';
  import SummaryStep from './SummaryStep.svelte';

  // «Привязки» of a draft for ReplaceHost (P1-05, P2-03E, mockups 09-01…09-03, 09-07 actions):
  // Scope → Dependencies → New host → Mapping → Summary. Choosing hosts needs the draft's own
  // connection (read-only reads); the stored mapping, decisions, confirmation and undo work offline.
  // Every change is an operation of the draft; the server is changed only by publication (P2-04).
  let { i18n, store, localId, serverReads }: {
    i18n: I18n;
    store: ReplaceHostStore;
    localId: string;
    /** The draft's own connection is active, so the server may be read. */
    serverReads: boolean;
  } = $props();

  let resetOpen = $state(false);
  let historyOpen = $state(false);

  const preview = $derived(store.preview);
  const calculated = $derived(preview !== null);

  const STEP_LABEL: Readonly<Record<WizardStep, LocalizedText>> = {
    scope: { id: 'bindings.step.scope' },
    dependencies: { id: 'bindings.step.dependencies' },
    new_host: { id: 'bindings.step.new_host' },
    mapping: { id: 'bindings.step.mapping' },
    summary: { id: 'bindings.step.summary' }
  };

  function available(step: WizardStep): boolean {
    if (calculated) return true;
    switch (step) {
      case 'scope':
        return true;
      case 'dependencies':
        return store.inventory !== null && store.sourceHostid !== null;
      case 'new_host':
        return store.sourceHostid !== null && store.scope !== null;
      default:
        return false;
    }
  }

  const steps = $derived<StepItem[]>(
    WIZARD_STEPS.map((step, index) => {
      const current = WIZARD_STEPS.indexOf(store.step);
      const state: StepItem['state'] =
        step === store.step ? 'current' : !available(step) ? 'unavailable' : index < current || (calculated && index < 3) ? 'done' : 'todo';
      return { id: step, label: STEP_LABEL[step], state };
    })
  );

  $effect(() => {
    if (store.localId !== localId) return;
    if (store.status === 'none' && serverReads && store.inventory === null && !store.inventoryLoading && store.inventoryFailure === null) {
      void store.loadInventory();
    }
  });

  async function go(step: WizardStep): Promise<void> {
    store.step = step;
    await tick();
    document.getElementById('bindings-step-title')?.focus();
  }

  async function next(): Promise<void> {
    switch (store.step) {
      case 'scope':
        return go('dependencies');
      case 'dependencies':
        return go('new_host');
      case 'new_host':
        if (await store.calculate()) await go('mapping');
        return;
      case 'mapping':
        return go('summary');
      default:
        return;
    }
  }

  function back(): void {
    const index = WIZARD_STEPS.indexOf(store.step);
    const previous = WIZARD_STEPS[index - 1];
    if (previous) void go(previous);
  }

  const nextLabel = $derived<LocalizedText | null>(
    store.step === 'scope'
      ? { id: 'bindings.action.next_dependencies' }
      : store.step === 'dependencies'
        ? { id: 'bindings.action.next_new_host' }
        : store.step === 'new_host' && !calculated
          ? { id: 'bindings.action.next_mapping' }
          : store.step === 'new_host'
            ? null
            : store.step === 'mapping'
              ? { id: 'bindings.action.next_summary' }
              : null
  );
  const nextDisabled = $derived(
    store.busy !== null ||
      (store.step === 'scope' && (store.sourceHostid === null || store.scope === null)) ||
      (store.step === 'dependencies' && (store.sourceHostid === null || store.scope === null)) ||
      (store.step === 'new_host' && (store.target === null || !serverReads))
  );

  const progress = $derived<LocalizedText | null>(
    store.busy === 'calculate'
      ? { id: 'bindings.progress.calculating' }
      : store.busy === 'check'
        ? { id: 'bindings.progress.checking' }
        : store.busy !== null
          ? { id: 'bindings.progress.saving' }
          : null
  );

  async function reset(): Promise<void> {
    resetOpen = false;
    if (await store.reset()) await go('scope');
  }

  function toggleHistory(event: Event): void {
    historyOpen = (event.currentTarget as HTMLDetailsElement).open;
    if (historyOpen) void store.loadHistory();
  }
</script>

<section class="workspace-section bindings" aria-labelledby="bindings-title">
  <h2 id="bindings-title">{i18n.t('draft.nav.bindings')}</h2>

  {#if store.status === 'idle' || store.status === 'loading'}
    <Skeleton {i18n} label={{ id: 'common.state.loading' }} lines={4} />
  {:else if store.status === 'failed' && store.loadFailure}
    <Banner {i18n} heading={{ id: 'bindings.load_failed' }} body={replaceHostCodeText(store.loadFailure.code, store.loadFailure.params)} tone="danger">
      {#snippet actions()}<Button {i18n} label={{ id: 'bindings.action.retry' }} onclick={() => void store.load()} />{/snippet}
    </Banner>
  {:else}
    <StepBar {i18n} label={{ id: 'bindings.a11y.steps' }} {steps} onselect={(id) => void go(id as WizardStep)} />

    {#if preview}
      <div class="bindings-toolbar">
        <p class="muted">
          {i18n.t('bindings.calculated_at', { time: preview.operation.calculated_at, version: preview.operation.server_version })}
          {#if preview.checked_at !== null}· {i18n.t('bindings.checked_at', { time: preview.checked_at })}{/if}
        </p>
        <div class="actions">
          <Button {i18n} label={{ id: 'bindings.action.check_all' }} disabled={!serverReads || store.busy !== null} busy={store.busy === 'check'} describedby={serverReads ? undefined : 'bindings-check-offline'} onclick={() => void store.revalidate()}>
            {#snippet leadingIcon()}<Icon name="refresh" />{/snippet}
          </Button>
          <Button {i18n} variant="quiet" label={{ id: 'bindings.action.reset_all' }} disabled={store.busy !== null} onclick={() => (resetOpen = true)} />
        </div>
        {#if !serverReads}<p id="bindings-check-offline" class="muted">{i18n.t('bindings.offline.check_all')}</p>{/if}
      </div>
    {/if}

    <div class="bindings-live" role="status" aria-live="polite">
      {#if progress}<Progress {i18n} label={progress} />{/if}
      {#if store.announcement && store.busy === null}<p class="app-notice">{resolveText(i18n, store.announcement)}</p>{/if}
    </div>
    {#if store.commandFailure}
      <Banner {i18n} heading={{ id: 'bindings.command_failed' }} body={replaceHostCodeText(store.commandFailure.code, store.commandFailure.params)} tone="danger" />
    {/if}

    <h3 id="bindings-step-title" class="visually-hidden" tabindex="-1">{resolveText(i18n, STEP_LABEL[store.step])}</h3>

    {#if (store.step === 'scope' || store.step === 'dependencies' || store.step === 'new_host') && preview}
      <section class="card" aria-labelledby="stored-title">
        <h3 id="stored-title">
          {i18n.t('bindings.stored.title', { source: preview.operation.source_host.name, target: preview.operation.target_host?.name ?? '—' })}
        </h3>
        <p class="muted">{i18n.t('bindings.stored.change_hint')}</p>
        <div class="actions"><Button {i18n} label={{ id: 'bindings.action.reset_all' }} disabled={store.busy !== null} onclick={() => (resetOpen = true)} /></div>
      </section>
    {:else if store.step === 'scope' || store.step === 'dependencies' || store.step === 'new_host'}
      {#if !serverReads}
        <Banner {i18n} heading={{ id: 'bindings.offline.title' }} body={{ id: 'bindings.offline.body' }} tone="info">
          {#snippet actions()}<a class="button-link" href="/connections">{i18n.t('bindings.action.connect')}</a>{/snippet}
        </Banner>
      {:else if store.inventoryLoading}
        <Skeleton {i18n} label={{ id: 'bindings.progress.inventory' }} lines={5} />
      {:else if store.inventoryFailure}
        <Banner {i18n} heading={{ id: 'bindings.load_failed' }} body={replaceHostCodeText(store.inventoryFailure.code, store.inventoryFailure.params)} tone="danger">
          {#snippet actions()}<Button {i18n} label={{ id: 'bindings.action.retry' }} onclick={() => void store.loadInventory()} />{/snippet}
        </Banner>
      {:else if store.inventory}
        {#if store.step === 'new_host'}
          <NewHostStep {i18n} {store} />
        {:else}
          <ScopeStep {i18n} {store} withDependencies={store.step === 'dependencies'} />
        {/if}
      {/if}
    {:else if preview && store.step === 'mapping'}
      <MappingStep {i18n} {store} {preview} />
    {:else if preview && store.step === 'summary'}
      <SummaryStep {i18n} {store} {preview} {localId} />
    {/if}

    <div class="wizard-nav actions">
      {#if store.step !== 'scope'}<Button {i18n} label={{ id: 'bindings.action.back' }} onclick={back} />{/if}
      {#if nextLabel}
        <Button {i18n} variant="primary" label={nextLabel} disabled={nextDisabled} busy={store.busy === 'calculate'} onclick={() => void next()} />
      {/if}
    </div>

    {#if preview}
      <details class="technical-details" ontoggle={toggleHistory}>
        <summary>{i18n.t('bindings.history.title')}</summary>
        {#if historyOpen && store.history}
          {#if store.history.length === 0}
            <p>{i18n.t('bindings.history.empty')}</p>
          {:else}
            <ol class="history">
              {#each store.history as entry (entry.seq)}
                <li>
                  <span>{resolveText(i18n, historyOpText(entry.op))}</span>
                  <time datetime={Number.isFinite(entry.recorded_at) ? new Date(entry.recorded_at).toISOString() : undefined}>{i18n.date(entry.recorded_at, DATE_TIME_FORMAT)}</time>
                </li>
              {/each}
            </ol>
          {/if}
        {/if}
      </details>
    {/if}
    <p class="muted">{i18n.t('bindings.local_only')}</p>
    <p class="muted">{i18n.t('bindings.plan_invalid_note')}</p>
  {/if}
</section>

<Dialog {i18n} id="bindings-reset" title={{ id: 'bindings.reset.title' }} description={{ id: 'bindings.reset.body' }} open={resetOpen} onclose={() => (resetOpen = false)}>
  {#snippet actions()}
    <Button {i18n} label={{ id: 'common.action.cancel' }} onclick={() => (resetOpen = false)} />
    <Button {i18n} variant="danger" label={{ id: 'bindings.action.reset_all' }} onclick={() => void reset()} />
  {/snippet}
</Dialog>
