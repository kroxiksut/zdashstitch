<script lang="ts">
  import { tick } from 'svelte';
  import type { I18n } from '../i18n';
  import { Button, Icon, StatusBadge, resolveText, type LocalizedText } from '../components';
  import ComingLater from './ComingLater.svelte';
  import RenameDialog from './RenameDialog.svelte';
  import type { DraftRenameResult, DraftSummary } from './drafts';
  import type { PortableClient, PortableConversionPreview, PortableConversionOutcome, PortableBindingSetup } from '../workcopy/client';
  import { workcopyCodeText } from '../workcopy/messages';
  import { createdDashboardLines, detailRows } from './format';
  import { KIND_LABEL, ORIGIN_LABEL } from './labels';
  import { draftPath } from './router';
  import { canSavePreviewedPage, samePagePreviewScope, type PagePreviewScope } from './page-preview-guard';

  // Details column of «Drafts» (P1-03F, mockup 02-01): what the draft is, where it came from and
  // what was published from it. «Open» and «Rename» work; the other actions are later entries and
  // stay visible but unavailable with «Will be available later» (P1-01H §6).
  let { i18n, draft, onrename, portable, onportablecreated }: {
    i18n: I18n;
    draft: DraftSummary;
    /** Absent while the storage cannot rename (e.g. without a backend). */
    onrename?: (localId: string, name: string) => Promise<DraftRenameResult>;
    portable?: PortableClient;
    onportablecreated?: (outcome: PortableConversionOutcome) => void;
  } = $props();

  const rows = $derived(detailRows(i18n, draft));
  const created = $derived(draft.created_dashboards ?? []);
  let renaming = $state(false);
  let portablePreview = $state<PortableConversionPreview | null>(null);
  let bindingSetup = $state<PortableBindingSetup | null>(null);
  let bindingInputs = $state<Array<{ selected: string; role: string; slot: string }>>([]);
  let hostInputs = $state<Array<{ role: string; slot: string }>>([]);
  let patternInputs = $state<Array<{ role: string }>>([]);
  let portableError = $state<{ readonly title: LocalizedText; readonly reason: LocalizedText } | null>(null);
  let portableBusy = $state<'preview' | 'create' | 'library' | null>(null);
  let librarySaved = $state(false);
  let pageChoices = $state<ReadonlyArray<{ readonly id: string; readonly name: string }>>([]);
  let selectedPageId = $state('');
  let pagePreview = $state<PortableConversionPreview | null>(null);
  let previewedPage = $state<PagePreviewScope | null>(null);
  let requestGeneration = 0;

  $effect(() => {
    draft.local_id;
    requestGeneration += 1;
    pageChoices = [];
    selectedPageId = '';
    pagePreview = null;
    previewedPage = null;
    portablePreview = null;
    bindingSetup = null;
    bindingInputs = [];
    hostInputs = [];
    patternInputs = [];
    portableError = null;
    portableBusy = null;
    librarySaved = false;
  });

  function currentPageScope(): PagePreviewScope {
    return { workcopyId: draft.local_id, pageId: selectedPageId, generation: requestGeneration };
  }

  function pageSelectionChanged(): void {
    requestGeneration += 1;
    pagePreview = null;
    previewedPage = null;
    librarySaved = false;
    portableBusy = null;
  }

  const portableCounts = [
    ['item_fields', 'drafts.portable.count.item_fields'],
    ['item_pattern_fields', 'drafts.portable.count.item_pattern_fields'],
    ['host_fields', 'drafts.portable.count.host_fields'],
    ['host_pattern_fields', 'drafts.portable.count.host_pattern_fields'],
    ['graph_fields', 'drafts.portable.count.graph_fields'],
    ['unknown_fields', 'drafts.portable.count.unknown_fields'],
    ['unclassified_widgets', 'drafts.portable.count.unclassified_widgets'],
    ['reference_definitions', 'drafts.portable.count.reference_definitions'],
    ['reference_consumers', 'drafts.portable.count.reference_consumers']
  ] as const;

  const portableBlockerMessages: Readonly<Record<string, LocalizedText>> = {
    portable_conversion_item_binding_required: { id: 'drafts.portable.blocker.item_binding_required' },
    portable_conversion_host_binding_required: { id: 'drafts.portable.blocker.host_binding_required' },
    portable_conversion_graph_binding_required: { id: 'drafts.portable.blocker.graph_binding_required' },
    portable_conversion_unknown_data: { id: 'drafts.portable.blocker.unknown_data' },
    portable_conversion_reference_required: { id: 'drafts.portable.blocker.reference_required' },
    portable_conversion_scope_unsupported: { id: 'drafts.portable.blocker.scope_unsupported' },
    portable_conversion_unsupported_data: { id: 'drafts.portable.blocker.unsupported_data' },
    portable_conversion_binding_required: { id: 'drafts.portable.blocker.binding_required' },
    portable_conversion_widget_unsupported: { id: 'drafts.portable.blocker.widget_unsupported' },
    portable_conversion_field_unsupported: { id: 'drafts.portable.blocker.field_unsupported' }
  };

  async function closeRename(): Promise<void> {
    if (!renaming) return;
    renaming = false;
    await tick();
    document.getElementById('drafts-details-rename')?.focus();
  }

  async function checkPortable(): Promise<void> {
    if (!portable || !onportablecreated || portableBusy !== null) return;
    const workcopyId = draft.local_id;
    const generation = ++requestGeneration;
    const current = () => draft.local_id === workcopyId && requestGeneration === generation;
    portableBusy = 'preview';
    portableError = null;
    portablePreview = null;
    bindingSetup = null;
    librarySaved = false;
    try {
      const result = await portable.preview(workcopyId);
      if (!current()) return;
      if (result.status === 'ready') portablePreview = result.value;
      else portableError = {
        title: { id: 'drafts.portable.failed_title' },
        reason: result.status === 'unavailable' ? { id: 'startup.error.unavailable' } : workcopyCodeText(result.failure.code, result.failure.params)
      };
    } catch {
      if (current()) portableError = { title: { id: 'drafts.portable.failed_title' }, reason: { id: 'startup.error.unavailable' } };
    } finally {
      if (!current()) return;
      portableBusy = null;
      await tick();
      document.getElementById('drafts-portable-result-title')?.focus();
      if (portableError) document.getElementById('drafts-portable-error')?.focus();
    }
  }

  async function loadBindings(): Promise<void> {
    if (!portable?.bindingSetup || portableBusy !== null) return;
    const id = draft.local_id;
    const generation = ++requestGeneration;
    portableBusy = 'preview'; portableError = null;
    try {
      const result = await portable.bindingSetup(id);
      if (draft.local_id !== id || generation !== requestGeneration) return;
      if (result.status === 'ready') {
        bindingSetup = result.value;
        const hosts = [...new Set([...result.value.items.map(item => item.hostid), ...(result.value.hosts ?? []).map(host => host.hostid)].filter(Boolean))];
        bindingInputs = result.value.items.map(item => ({ selected: '', role: 'hosts', slot: `host-${hosts.indexOf(item.hostid) + 1}` }));
        hostInputs = (result.value.hosts ?? []).map(host => ({ role: 'hosts', slot: `host-${hosts.indexOf(host.hostid) + 1}` }));
        patternInputs = (result.value.patterns ?? []).map(() => ({ role: 'hosts' }));
      } else portableError = { title: { id: 'drafts.portable.failed_title' }, reason: result.status === 'unavailable' ? { id: 'startup.error.unavailable' } : workcopyCodeText(result.failure.code, result.failure.params) };
    } catch { if (draft.local_id === id && generation === requestGeneration) portableError = { title: { id: 'drafts.portable.failed_title' }, reason: { id: 'startup.error.unavailable' } }; }
    finally { if (draft.local_id === id && generation === requestGeneration) portableBusy = null; }
  }

  async function createBoundPortable(): Promise<void> {
    if (!portable?.createWithBindings || !bindingSetup || !onportablecreated || portableBusy !== null) return;
    const setup = bindingSetup;
    const choices = setup.items.map((item, i) => {
      const input = bindingInputs[i];
      const selected = input?.selected ? item.candidates[Number(input.selected) - 1] : undefined;
      return selected && input.role && input.slot ? { itemid: item.itemid, catalog: selected.catalog, metric: selected.metric, role: input.role, slot: input.slot } : null;
    });
    if (choices.some(choice => choice === null)) return;
    const id = draft.local_id; const generation = ++requestGeneration;
    portableBusy = 'create'; portableError = null;
    try {
      const hosts = (setup.hosts ?? []).map((host, i) => ({hostid: host.hostid, role: hostInputs[i].role, slot: hostInputs[i].slot}));
      const patterns = (setup.patterns ?? []).map((pattern, i) => ({page_index: pattern.page_index, widget_index: pattern.widget_index, field_name: pattern.field_name, role: patternInputs[i].role}));
      const result = await portable.createWithBindings(id, setup, choices.filter(choice => choice !== null), hosts, patterns);
      if (draft.local_id !== id || generation !== requestGeneration) return;
      if (result.status === 'ready') onportablecreated(result.value);
      else portableError = { title: { id: 'drafts.portable.create_failed_title' }, reason: result.status === 'unavailable' ? { id: 'startup.error.unavailable' } : workcopyCodeText(result.failure.code, result.failure.params) };
    } catch { if (draft.local_id === id && generation === requestGeneration) portableError = { title: { id: 'drafts.portable.create_failed_title' }, reason: { id: 'startup.error.unavailable' } }; }
    finally { if (draft.local_id === id && generation === requestGeneration) portableBusy = null; }
  }

  async function savePortablePage(): Promise<void> {
    if (!portable || draft.kind !== 'page' || !portablePreview?.ready || portableBusy !== null) return;
    const workcopyId = draft.local_id;
    const generation = ++requestGeneration;
    const current = () => draft.local_id === workcopyId && requestGeneration === generation;
    portableBusy = 'library';
    portableError = null;
    librarySaved = false;
    try {
      const result = await portable.savePageToLibrary(workcopyId);
      if (!current()) return;
      if (result.status === 'ready') librarySaved = true;
      else portableError = {
        title: { id: 'page_library.save_failed' },
        reason: result.status === 'unavailable' ? { id: 'startup.error.unavailable' } : workcopyCodeText(result.failure.code, result.failure.params)
      };
    } catch {
      if (current()) portableError = { title: { id: 'page_library.save_failed' }, reason: { id: 'startup.error.unavailable' } };
    } finally {
      if (!current()) return;
      portableBusy = null;
      await tick();
      if (portableError) document.getElementById('drafts-portable-error')?.focus();
    }
  }

  async function loadPageChoices(): Promise<void> {
    if (!portable || portableBusy !== null) return;
    const workcopyId = draft.local_id;
    const generation = ++requestGeneration;
    const current = () => draft.local_id === workcopyId && requestGeneration === generation;
    portableBusy = 'preview';
    portableError = null;
    try {
      const result = await portable.pages(workcopyId);
      if (!current()) return;
      if (result.status === 'ready') pageChoices = result.value;
      else portableError = { title: { id: 'drafts.portable.failed_title' }, reason: result.status === 'unavailable' ? { id: 'startup.error.unavailable' } : workcopyCodeText(result.failure.code, result.failure.params) };
    } catch {
      if (current()) portableError = { title: { id: 'drafts.portable.failed_title' }, reason: { id: 'startup.error.unavailable' } };
    } finally { if (current()) portableBusy = null; }
  }

  async function checkSelectedPage(): Promise<void> {
    if (!portable || !selectedPageId || portableBusy !== null) return;
    const request = { workcopyId: draft.local_id, pageId: selectedPageId, generation: ++requestGeneration };
    const current = () => samePagePreviewScope(request, currentPageScope());
    portableBusy = 'preview';
    pagePreview = null;
    previewedPage = null;
    librarySaved = false;
    portableError = null;
    try {
      const result = await portable.previewPage(request.workcopyId, request.pageId);
      if (!current()) return;
      if (result.status === 'ready') { pagePreview = result.value; previewedPage = request; }
      else portableError = { title: { id: 'drafts.portable.failed_title' }, reason: result.status === 'unavailable' ? { id: 'startup.error.unavailable' } : workcopyCodeText(result.failure.code, result.failure.params) };
    } catch {
      if (current()) portableError = { title: { id: 'drafts.portable.failed_title' }, reason: { id: 'startup.error.unavailable' } };
    } finally { if (current()) portableBusy = null; }
  }

  async function saveSelectedPage(): Promise<void> {
    if (!portable || !canSavePreviewedPage(previewedPage, currentPageScope(), pagePreview?.ready === true) || portableBusy !== null) return;
    const request = currentPageScope();
    const current = () => samePagePreviewScope(request, currentPageScope());
    portableBusy = 'library';
    portableError = null;
    librarySaved = false;
    try {
      const result = await portable.saveDashboardPageToLibrary(request.workcopyId, request.pageId);
      if (!current()) return;
      if (result.status === 'ready') librarySaved = true;
      else portableError = { title: { id: 'page_library.save_failed' }, reason: result.status === 'unavailable' ? { id: 'startup.error.unavailable' } : workcopyCodeText(result.failure.code, result.failure.params) };
    } catch {
      if (current()) portableError = { title: { id: 'page_library.save_failed' }, reason: { id: 'startup.error.unavailable' } };
    } finally { if (current()) portableBusy = null; }
  }

  async function createPortable(): Promise<void> {
    if (!portable || !onportablecreated || !portablePreview?.ready || portableBusy !== null) return;
    const workcopyId = draft.local_id;
    const generation = ++requestGeneration;
    const current = () => draft.local_id === workcopyId && requestGeneration === generation;
    portableBusy = 'create';
    portableError = null;
    try {
      const result = await portable.create(workcopyId);
      if (!current()) return;
      if (result.status === 'ready') {
        onportablecreated?.(result.value);
      } else {
        portableError = {
          title: { id: 'drafts.portable.create_failed_title' },
          reason: result.status === 'unavailable' ? { id: 'startup.error.unavailable' } : workcopyCodeText(result.failure.code, result.failure.params)
        };
      }
    } catch {
      if (current()) portableError = { title: { id: 'drafts.portable.create_failed_title' }, reason: { id: 'startup.error.unavailable' } };
    } finally {
      if (!current()) return;
      portableBusy = null;
      await tick();
      if (portableError) document.getElementById('drafts-portable-error')?.focus();
    }
  }
</script>

<h2 id="drafts-details-title">{draft.name}</h2>
<p class="badges">
  <StatusBadge {i18n} label={KIND_LABEL[draft.kind]} tone="info" />
  <StatusBadge {i18n} label={ORIGIN_LABEL[draft.origin]} tone="info" />
</p>
<dl class="details-list">
  {#each rows as row (row.term)}
    <div>
      <dt>{row.term}</dt>
      <dd>{row.value}</dd>
    </div>
  {/each}
</dl>

{#if created.length > 0}
  <section class="created" aria-labelledby="drafts-created-title">
    <h3 id="drafts-created-title" class="eyebrow">
      {i18n.t(draft.origin === 'server_copy' ? 'drafts.created_from_copy' : 'drafts.created_from_draft')}
    </h3>
    <ul class="created-list">
      {#each created as item, index (index)}
        {@const lines = createdDashboardLines(i18n, item)}
        <li class:not-found={item.state === 'not_found'}>
          <Icon name={item.state === 'verified' ? 'circle-check' : 'alert-triangle'} size={18} />
          <span class="created-text">
            <span>{lines[0]}</span>
            <span class="muted">{lines[1]}</span>
          </span>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<div class="details-actions">
  <a class="button-link primary" href={draftPath(draft.local_id)} aria-describedby="drafts-details-title">{i18n.t('common.action.open')}</a>
  {#if onrename}
    <Button {i18n} id="drafts-details-rename" label={{ id: 'common.action.rename' }} describedby="drafts-details-title" onclick={() => (renaming = true)} />
  {/if}
  {#if draft.origin === 'server_copy' && portable && onportablecreated}
    <Button
      {i18n}
      variant="secondary"
      label={{ id: 'drafts.action.make_portable' }}
      describedby="drafts-details-title"
      busy={portableBusy === 'preview'}
      disabled={portableBusy !== null}
      onclick={() => void checkPortable()}
    />
    {#if draft.kind === 'dashboard'}
      <Button {i18n} label={{ id: 'page_library.choose_from_workcopy' }} disabled={portableBusy !== null} onclick={() => void loadPageChoices()} />
    {/if}
  {/if}
  <ComingLater {i18n} id="drafts-details-actions">
    <Button {i18n} label={{ id: 'common.action.duplicate' }} disabled />
    {#if !onrename}<Button {i18n} label={{ id: 'common.action.rename' }} disabled />{/if}
    {#if draft.origin === 'server_copy' && (!portable || !onportablecreated)}
      <Button {i18n} label={{ id: 'drafts.action.make_portable' }} disabled />
    {:else if draft.origin !== 'server_copy'}
      <Button {i18n} label={{ id: 'drafts.action.export_file' }} disabled />
    {/if}
    <Button {i18n} label={{ id: 'common.action.delete' }} disabled />
  </ComingLater>
</div>

<p class="details-note"><Icon name="info-circle" size={18} /><span>{i18n.t('drafts.delete.safe')}</span></p>
{#if draft.origin === 'server_copy'}
  <p class="hint">{i18n.t('drafts.server_copy_export_hint')}</p>
{/if}

{#if draft.origin === 'server_copy' && draft.kind === 'dashboard' && pageChoices.length > 0}
  <section class="portable-preview" aria-labelledby="page-library-workcopy-title">
    <h3 id="page-library-workcopy-title" class="eyebrow">{i18n.t('page_library.choose_from_workcopy')}</h3>
    <label for="page-library-workcopy-select">{i18n.t('page_library.selected_page')}</label>
    <select id="page-library-workcopy-select" bind:value={selectedPageId} disabled={portableBusy !== null} onchange={pageSelectionChanged}>
      <option value="">{i18n.t('page_library.select_page')}</option>
      {#each pageChoices as page (page.id)}<option value={page.id}>{page.name}</option>{/each}
    </select>
    <Button {i18n} label={{ id: 'page_library.check_page' }} disabled={!selectedPageId || portableBusy !== null} onclick={() => void checkSelectedPage()} />
    {#if pagePreview && previewedPage && samePagePreviewScope(previewedPage, currentPageScope())}
      <p>{i18n.t('drafts.portable.summary', { kind: i18n.t('object.kind.page'), pages: pagePreview.inventory.pages, widgets: pagePreview.inventory.widgets })}</p>
      <dl class="details-list">
        {#each portableCounts as [key, message] (key)}<div><dt>{i18n.t(message)}</dt><dd>{i18n.number(pagePreview.inventory[key])}</dd></div>{/each}
      </dl>
      {#if pagePreview.ready}
        <p role="status">{i18n.t('drafts.portable.ready')}</p>
        <Button {i18n} label={{ id: 'page_library.save_action' }} disabled={portableBusy !== null || !canSavePreviewedPage(previewedPage, currentPageScope(), pagePreview.ready)} onclick={() => void saveSelectedPage()} />
        {#if librarySaved}<p role="status">{i18n.t('page_library.saved')}</p>{/if}
      {:else}
        <ul class="blocker-list">
          {#each [...new Set([...(pagePreview.blocker ? [pagePreview.blocker] : []), ...pagePreview.blockers])] as blocker (blocker)}
            <li>{resolveText(i18n, portableBlockerMessages[blocker] ?? { id: 'errors.connection.unknown', params: { code: blocker } })}</li>
          {/each}
        </ul>
      {/if}
    {/if}
  </section>
{/if}

{#if portableError}
  <section id="drafts-portable-error" class="details-note" role="alert" tabindex="-1" aria-labelledby="drafts-portable-error-title">
    <Icon name="alert-triangle" size={18} />
    <span><strong id="drafts-portable-error-title">{resolveText(i18n, portableError.title)}</strong> {resolveText(i18n, portableError.reason)}</span>
  </section>
{/if}

{#if portableBusy !== null}
  <p role="status" aria-live="polite">{i18n.t(portableBusy === 'preview' ? 'drafts.portable.checking' : 'drafts.portable.creating')}</p>
{/if}

{#if portablePreview}
  <section class="portable-preview" aria-labelledby="drafts-portable-result-title" aria-live="polite" aria-busy={portableBusy === 'create'}>
    <h3 id="drafts-portable-result-title" class="eyebrow" tabindex="-1">{i18n.t('drafts.portable.preview_title')}</h3>
    <p>{i18n.t('drafts.portable.summary', {
      kind: i18n.t(portablePreview.inventory.kind === 'dashboard' ? 'object.kind.dashboard' : 'object.kind.page'),
      pages: portablePreview.inventory.pages,
      widgets: portablePreview.inventory.widgets
    })}</p>
    <h4 class="eyebrow">{i18n.t('drafts.portable.dependencies_title')}</h4>
    <dl class="details-list">
      {#each portableCounts as [key, message] (key)}
        <div><dt>{i18n.t(message)}</dt><dd>{i18n.number(portablePreview.inventory[key])}</dd></div>
      {/each}
    </dl>
    {#if portablePreview.ready}
      <p role="status">{i18n.t('drafts.portable.ready')}</p>
      <Button
        {i18n}
        variant="primary"
        label={{ id: 'drafts.portable.create_action' }}
        describedby="drafts-portable-result-title"
        busy={portableBusy === 'create'}
        disabled={portableBusy !== null}
        onclick={() => void createPortable()}
      />
      {#if draft.kind === 'page'}
        <Button {i18n} label={{ id: 'page_library.save_action' }} busy={portableBusy === 'library'} disabled={portableBusy !== null} onclick={() => void savePortablePage()} />
        {#if librarySaved}<p role="status">{i18n.t('page_library.saved')}</p>{/if}
      {/if}
    {:else}
      <ul class="blocker-list">
        {#each [...new Set([...(portablePreview.blocker ? [portablePreview.blocker] : []), ...portablePreview.blockers])] as blocker (blocker)}
          <li>{resolveText(i18n, portableBlockerMessages[blocker] ?? { id: 'errors.connection.unknown', params: { code: blocker } })}</li>
        {/each}
        {#if portablePreview.blocker === null && portablePreview.blockers.length === 0}
          <li>{resolveText(i18n, { id: 'errors.connection.unknown', params: { code: 'portable_conversion_blocked' } })}</li>
        {/if}
      </ul>
      {#if (portablePreview.inventory.item_fields > 0 || portablePreview.inventory.host_fields > 0 || portablePreview.inventory.graph_fields > 0 || portablePreview.inventory.host_pattern_fields > 0 || portablePreview.inventory.item_pattern_fields > 0) && portable?.bindingSetup && portable.createWithBindings}
        <Button {i18n} label={{ id: 'drafts.portable.bindings.open' }} disabled={portableBusy !== null} onclick={() => void loadBindings()} />
        {#if bindingSetup}
          <p>{i18n.t('drafts.portable.bindings.help')}</p>
          {#if (bindingSetup.unresolved_graphs ?? 0) > 0}<p>{i18n.t('drafts.portable.bindings.graphs_unresolved')}</p>{/if}
          {#if (bindingSetup.patterns?.length ?? 0) > 0}<p>{i18n.t('drafts.portable.bindings.pattern_help')}</p>{/if}
          {#each bindingSetup.items as item, i (item.itemid)}
            <fieldset>
              <legend>{item.host ?? i18n.t('drafts.portable.bindings.unavailable')} — {item.name ?? i18n.t('drafts.portable.bindings.unavailable')}</legend>
              <label>{i18n.t('drafts.portable.bindings.metric')}
                <select bind:value={bindingInputs[i].selected}>
                  <option value="">{i18n.t('drafts.portable.bindings.select')}</option>
                  {#each item.candidates as candidate, candidateIndex}
                    <option value={String(candidateIndex + 1)}>{candidate.catalog_name} — {candidate.metric_name}{candidate.lld_prototype ? ` (${i18n.t('drafts.portable.bindings.lld_instance')})` : ''}</option>
                  {/each}
                </select>
              </label>
              <label>{i18n.t('drafts.portable.bindings.role')} <input bind:value={bindingInputs[i].role} /></label>
              <label>{i18n.t('drafts.portable.bindings.slot')} <input bind:value={bindingInputs[i].slot} /></label>
            </fieldset>
          {/each}
          {#each bindingSetup.hosts ?? [] as host, i (host.hostid)}
            <fieldset>
              <legend>{host.name ?? i18n.t('drafts.portable.bindings.unavailable')}</legend>
              <label>{i18n.t('drafts.portable.bindings.role')} <input bind:value={hostInputs[i].role} /></label>
              <label>{i18n.t('drafts.portable.bindings.slot')} <input bind:value={hostInputs[i].slot} /></label>
            </fieldset>
          {/each}
          {#each bindingSetup.patterns ?? [] as pattern, i (`${pattern.page_index}:${pattern.widget_index}:${pattern.field_name}`)}
            <fieldset>
              <legend>{pattern.field_name}: {pattern.value}</legend>
              <label>{i18n.t('drafts.portable.bindings.role')} <input bind:value={patternInputs[i].role} /></label>
            </fieldset>
          {/each}
          <Button {i18n} variant="primary" label={{ id: 'drafts.portable.create_action' }} disabled={portableBusy !== null || (bindingSetup.items.length === 0 && (bindingSetup.hosts?.length ?? 0) === 0 && (bindingSetup.unresolved_graphs ?? 0) === 0 && (bindingSetup.patterns?.length ?? 0) === 0) || bindingInputs.some(input => !input.selected || !input.role || !input.slot) || hostInputs.some(input => !input.role || !input.slot) || patternInputs.some(input => !input.role)} onclick={() => void createBoundPortable()} />
        {/if}
      {/if}
    {/if}
  </section>
{/if}

{#if renaming && onrename}
  <RenameDialog {i18n} {draft} {onrename} onclose={() => void closeRename()} />
{/if}
