<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { apiFailure } from '../api';
  import { Button, Dialog, resolveText } from '../components';
  import type { I18n } from '../i18n';
  import type { MessageId } from '../i18n/generated';
  import { createHttpPublicationClient, outcomeFromWire, publicationFailure } from '../publication/client';
  import { publicationCodeText, resultBody } from '../publication/messages';
  import type { Outcome, PublicationFailure } from '../publication/model';
  import { CSRF_HEADER, loadCsrfToken } from '../session';
  import { valueTypeText } from './messages';
  import CloneComparison from './CloneComparison.svelte';
  import type { CloneComparisonData } from './clone-comparison';

  let { i18n, localId, connected, writable, onjournal }: {
    i18n: I18n;
    localId: string;
    connected: boolean;
    writable: boolean;
    onjournal?: (operationId: string) => void;
  } = $props();

  interface Host { hostid: string; name: string }
  interface FieldAddress {
    widget: { page_index: number; widget_index: number; dashboard_pageid: string | null; widgetid: string | null };
    field_name: string;
  }
  interface LldItem {
    itemid: string; name: string; key: string; value_type: number; units: string;
  }
  interface LldRow {
    field: FieldAddress; slot_id: string; source: LldItem; source_host: string; target_host: string;
    prototype_key: string | null; candidates: LldItem[]; fingerprint: string;
  }
  interface PatternRequirement {
    field: FieldAddress; kind: 'host' | 'item'; source_pattern: string;
  }
  interface PatternPreview {
    fingerprint: string; match_count: number; matches: { host: string; name: string }[]; truncated: boolean;
  }
  interface PatternEdit {
    action: '' | 'keep' | 'replace';
    value: string;
    review?: { signature: string; value: string; result: PatternPreview };
  }
  type PatternChoice = { field: FieldAddress; decision:
    { action: 'keep' } | { action: 'replace'; target_pattern: string; fingerprint: string } };
  interface SeriesLabel { host: string; item: string; key: string }
  interface SeriesOption {
    field: FieldAddress;
    hostid: string;
    widget_name: string;
    dataset_index: number;
    position: number;
    source: SeriesLabel;
  }
  interface GrowthPreview { fingerprint: string; before: SeriesLabel[]; after: SeriesLabel[] }
  interface Setup {
    revision: number;
    source_fingerprint: string;
    hosts: Host[];
    patterns: PatternRequirement[];
    unattributed: number;
    growth_samples: SeriesOption[];
  }
  interface Saved {
    clone_id: string;
    name: string;
    pages: number;
    widgets: number;
    changed: number;
    targets: string[];
    kept_fields: string[];
    kept_digest: string | null;
    missing_contracts: string[];
    blocked: string | null;
    last_outcome: Parameters<typeof outcomeFromWire>[0] | null;
  }
  interface Plan { handle: string; plan_id: string; name: string; pages: number; widgets: number }

  const publication = createHttpPublicationClient();
  const path = $derived(`/api/drafts/${encodeURIComponent(localId)}/clone`);
  let setup = $state<Setup | null>(null);
  let saved = $state<Saved | null>(null);
  let plan = $state<Plan | null>(null);
  let outcome = $state<Outcome | null>(null);
  let failure = $state<PublicationFailure | null>(null);
  let hosts = $state<Host[]>([]);
  let choices = $state<Record<string, string>>({});
  let name = $state('');
  let search = $state('');
  let truncated = $state(false);
  let patternEdits = $state<Record<string, PatternEdit>>({});
  let sampleIndex = $state('');
  let extraHost = $state('');
  let growthReview = $state<{ signature: string; preview: GrowthPreview } | null>(null);
  let bindingReview = $state<{ signature: string; rows: LldRow[] } | null>(null);
  let lldSelections = $state<Record<string, string>>({});
  let kept = $state(false);
  let busy = $state(false);
  let visible = $state(false);
  let comparisonOpen = $state(false);
  let nameInput = $state<HTMLInputElement>();
  let savedHeading = $state<HTMLHeadingElement>();
  const setupErrors: Partial<Record<string, MessageId>> = {
    clone_intent_source_changed: 'clone.error.stale_source',
    clone_mapping_invalid: 'clone.error.target_selection',
    clone_target_not_visible: 'clone.error.target_selection',
    clone_target_unresolved: 'clone.error.unresolved',
    clone_mapping_incomplete: 'clone.error.unresolved',
    clone_intent_target_name_invalid: 'clone.error.name',
    clone_growth_selection_invalid: 'clone.error.growth_selection',
    clone_growth_single_host_widget: 'clone.error.growth_single_host',
    clone_growth_other_references: 'clone.error.growth_other_references',
    clone_growth_metric_incompatible: 'clone.error.growth_metric',
    clone_growth_preview_required: 'clone.error.growth_preview',
    clone_template_color_missing: 'clone.error.growth_color',
    clone_lld_selection_required: 'clone.error.lld_required',
    clone_lld_selection_stale: 'clone.error.lld_stale',
    clone_lld_candidate_not_visible: 'clone.error.lld_stale',
    clone_lld_candidate_prototype_mismatch: 'clone.error.lld_stale',
    clone_pattern_decision_required: 'clone.error.pattern_required',
    clone_pattern_decision_invalid: 'clone.error.pattern_required',
    clone_pattern_invalid: 'clone.error.pattern_invalid',
    clone_pattern_preview_stale: 'clone.error.pattern_stale',
    clone_pattern_target_not_visible: 'clone.error.target_selection',
    clone_pattern_preview_truncated: 'clone.error.pattern_unverified',
    clone_pattern_preview_read_failed: 'clone.error.pattern_unverified'
  };

  async function request<T>(url: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { Accept: 'application/json' };
    const init: RequestInit = { credentials: 'same-origin', headers };
    if (body !== undefined) {
      headers['Content-Type'] = 'application/json';
      headers[CSRF_HEADER] = await loadCsrfToken(fetch);
      init.method = 'POST';
      init.body = JSON.stringify(body);
    }
    const response = await fetch(url, init);
    if (!response.ok) throw await apiFailure(response);
    return (await response.json()) as T;
  }

  async function run(action: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    failure = null;
    try { await action(); }
    catch (error) {
      failure = publicationFailure(error);
      if (failure.code.startsWith('clone_lld_')) bindingReview = null;
      if (failure.code === 'clone_pattern_preview_stale') {
        for (const edit of Object.values(patternEdits)) edit.review = undefined;
      }
    }
    finally { busy = false; }
  }

  function remember(cloneId: string | null): void {
    const url = new URL(window.location.href);
    if (cloneId) url.searchParams.set('clone', cloneId);
    else url.searchParams.delete('clone');
    window.history.replaceState(window.history.state, '', url);
  }

  onMount(() => {
    const cloneId = new URL(window.location.href).searchParams.get('clone');
    if (!cloneId) return;
    visible = true;
    void run(async () => {
      saved = await request<Saved>(`${path}/${encodeURIComponent(cloneId)}`);
      name = saved.name;
      outcome = saved.last_outcome === null ? null : outcomeFromWire(saved.last_outcome);
    });
  });

  async function findHosts(): Promise<void> {
    const found = await request<{ hosts: Host[]; truncated: boolean }>(
      `${path}/hosts?${new URLSearchParams({ search: search.trim() })}`
    );
    const selected = new Set([...Object.values(choices), extraHost]);
    const seen = new Set(found.hosts.map((host) => host.hostid));
    hosts = [...found.hosts, ...hosts.filter((host) => selected.has(host.hostid) && !seen.has(host.hostid))];
    truncated = found.truncated;
  }

  function begin(): void {
    visible = true;
    if (!connected) return;
    void run(async () => {
      const latest = await request<Setup>(path);
      if (setup?.revision !== latest.revision || setup.source_fingerprint !== latest.source_fingerprint ||
          JSON.stringify(setup.growth_samples) !== JSON.stringify(latest.growth_samples)) {
        sampleIndex = '';
        extraHost = '';
        growthReview = null;
        bindingReview = null;
        lldSelections = {};
        patternEdits = Object.fromEntries(latest.patterns.map((row) => [fieldKey(row.field), { action: '', value: '' }]));
      }
      setup = latest;
      await findHosts();
    }).then(async () => {
      await tick();
      if (document.activeElement === document.body) nameInput?.focus();
    });
  }

  const sample = $derived(sampleIndex === '' ? undefined : setup?.growth_samples[Number(sampleIndex)]);
  const growth = $derived(sample && extraHost ? { sample: sample.field, target_hostid: extraHost } : null);
  const canCheckBindings = $derived(setup !== null && connected && setup.unattributed === 0 &&
    setup.hosts.every((host) => !!choices[host.hostid]) &&
    setup.hosts.some((host) => choices[host.hostid] !== 'keep') &&
    (sampleIndex === '' || (growth !== null && sample !== undefined &&
      extraHost !== sample.hostid && !Object.values(choices).includes(extraHost))));
  const bindingSignature = $derived(JSON.stringify({ choices, growth }));
  const lldRows = $derived(bindingReview?.signature === bindingSignature ? bindingReview.rows : null);
  const lldChoices = $derived((lldRows ?? []).flatMap((row) => {
    const itemid = lldSelections[row.fingerprint];
    return itemid ? [{ field: row.field, slot_id: row.slot_id, fingerprint: row.fingerprint, itemid }] : [];
  }));
  const patternChoices = $derived.by(() => {
    const choices: PatternChoice[] = [];
    for (const row of setup?.patterns ?? []) {
      const edit = patternEdits[fieldKey(row.field)];
      if (edit?.action === 'keep') choices.push({ field: row.field, decision: { action: 'keep' } });
      if (edit?.action === 'replace') {
        const preview = currentPatternReview(edit);
        if (preview) choices.push({ field: row.field, decision: {
          action: 'replace', target_pattern: edit.value, fingerprint: preview.fingerprint
        } });
      }
    }
    return choices;
  });
  const canPrepare = $derived(canCheckBindings && name.trim() !== '' && lldRows !== null &&
    lldRows.length === lldChoices.length && setup?.patterns.length === patternChoices.length);
  const canPreview = $derived(canPrepare && growth !== null);
  const signature = $derived(JSON.stringify({ name: name.trim(), bindingSignature, patternChoices, lldChoices }));
  const currentReview = $derived(growth !== null && growthReview?.signature === signature ? growthReview.preview : null);
  const canSave = $derived(canPrepare && (sampleIndex === '' || (canPreview && currentReview !== null)));

  function setupBody(input: Setup) {
    return {
      revision: input.revision, source_fingerprint: input.source_fingerprint, name: name.trim(),
      hosts: Object.fromEntries(input.hosts.map((host) => [
        host.hostid, choices[host.hostid] === 'keep' ? null : choices[host.hostid]
      ])),
      pattern_choices: patternChoices,
      growth,
      lld_choices: lldChoices
    };
  }

  function fieldKey(field: FieldAddress): string {
    return JSON.stringify([field.widget.page_index, field.widget.widget_index, field.field_name]);
  }

  function currentPatternReview(edit: PatternEdit): PatternPreview | null {
    return edit.review?.signature === bindingSignature && edit.review.value === edit.value ? edit.review.result : null;
  }

  function previewPattern(row: PatternRequirement): void {
    if (!setup || !canCheckBindings) return;
    const edit = patternEdits[fieldKey(row.field)];
    if (!edit?.value.trim() || edit.action !== 'replace') return;
    const value = edit.value;
    const signature = bindingSignature;
    const body = { setup: setupBody(setup), field: row.field, target_pattern: value };
    void run(async () => {
      edit.review = undefined;
      const result = await request<PatternPreview>(`${path}/preview-pattern`, body);
      edit.review = { signature, value, result };
    });
  }

  function reviewBindings(): void {
    if (!setup || !canCheckBindings) return;
    const body = setupBody(setup);
    const reviewed = bindingSignature;
    void run(async () => {
      bindingReview = null;
      const result = await request<{ lld_rows: LldRow[] }>(`${path}/review-bindings`, body);
      lldSelections = Object.fromEntries(result.lld_rows.flatMap((row) => {
        const itemid = lldSelections[row.fingerprint];
        return itemid && row.candidates.some((item) => item.itemid === itemid) ? [[row.fingerprint, itemid]] : [];
      }));
      bindingReview = { signature: reviewed, rows: result.lld_rows };
    });
  }

  function previewGrowth(): void {
    if (!setup || !canPreview) return;
    const body = setupBody(setup);
    const reviewedSignature = signature;
    void run(async () => {
      growthReview = null;
      const preview = await request<GrowthPreview>(`${path}/preview-growth`, body);
      growthReview = { signature: reviewedSignature, preview };
    });
  }

  function save(): void {
    if (!setup || !canSave) return;
    const body = { ...setupBody(setup), growth_fingerprint: currentReview?.fingerprint ?? null };
    void run(async () => {
      saved = await request<Saved>(path, body);
      kept = false;
      outcome = null;
      remember(saved.clone_id);
      await tick();
      if (document.activeElement === document.body) savedHeading?.focus();
    });
  }

  function makePlan(): void {
    if (!saved || !writable || saved.blocked || saved.missing_contracts.length > 0 || (saved.kept_digest !== null && !kept)) return;
    const draft = saved;
    void run(async () => {
      plan = await request<Plan>(`${path}/${encodeURIComponent(draft.clone_id)}/plan`, {
        kept_digest: kept ? draft.kept_digest : null
      });
    });
  }

  function cancelPlan(): void {
    const current = plan;
    plan = null;
    if (current) {
      void run(async () => publication.discard(localId, current.handle)).then(async () => {
        await tick();
        if (document.activeElement === document.body) document.getElementById('clone-plan-trigger')?.focus();
      });
    }
  }

  function confirm(): void {
    if (!plan || !saved || busy) return;
    const current = plan;
    const keptDigest = kept ? saved.kept_digest : null;
    plan = null;
    void run(async () => {
      outcome = await publication.confirm(localId, {
        handle: current.handle, plan_id: current.plan_id, name: current.name,
        kept_digest: keptDigest, confirmed: true
      });
    });
  }

  function edit(): void {
    if (!connected) return;
    // Keep the saved branch and URL until the replacement draft is saved.
    saved = null;
    outcome = null;
    begin();
  }

  function reconcile(): void {
    if (!outcome || !connected) return;
    const id = outcome.operation_id;
    void run(async () => { outcome = await publication.reconcile(id); });
  }
</script>

<section aria-labelledby="clone-title">
  <h2 id="clone-title">{i18n.t('clone.title')}</h2>
  {#if !visible}
    <Button {i18n} label={{ id: 'clone.open' }} onclick={begin} disabled={!connected} />
  {:else if saved}
    <h3 bind:this={savedHeading} tabindex="-1">{saved.name}</h3>
    <p>{i18n.t('clone.summary', { pages: saved.pages, widgets: saved.widgets, changes: saved.changed })}</p>
    <p>{i18n.t('clone.private_copy')}</p>
    <ul aria-label={i18n.t('clone.targets')}>{#each saved.targets as target}<li>{target}</li>{/each}</ul>
    {#if saved.kept_fields.length}
      <h4>{i18n.t('clone.kept')}</h4>
      <ul>{#each saved.kept_fields as field}<li><code>{field}</code></li>{/each}</ul>
    {/if}
    {#if saved.kept_digest !== null}
      <label><input type="checkbox" bind:checked={kept} disabled={busy || plan !== null} /> {i18n.t('clone.confirm_kept')}</label>
    {/if}
    {#if saved.blocked || saved.missing_contracts.length}
      <p role="status">{i18n.t('clone.blocked')}</p>
    {:else if !writable}
      <p>{i18n.t('clone.read_only')}</p>
    {/if}
    <div class="actions">
      <Button {i18n} id="clone-edit-trigger" label={{ id: 'clone.edit' }} onclick={edit} disabled={busy || !connected || plan !== null} />
      <Button {i18n} id="clone-plan-trigger" variant="primary" label={{ id: 'clone.plan' }} onclick={makePlan}
        disabled={busy || plan !== null || !writable || !!saved.blocked || saved.missing_contracts.length > 0 || (saved.kept_digest !== null && !kept) || outcome?.sent === true} />
    </div>
    {@const comparisonPath = `${path}/${encodeURIComponent(saved.clone_id)}/comparison`}
    <details class="clone-comparison" bind:open={comparisonOpen}>
      <summary>{i18n.t('clone.compare.title')}</summary>
      {#if comparisonOpen}
        {#key `${comparisonPath}:${outcome?.operation_id ?? ''}:${outcome?.state ?? ''}`}
          <CloneComparison {i18n} {onjournal} load={() => request<CloneComparisonData>(comparisonPath)} />
        {/key}
      {/if}
    </details>
  {:else if setup}
    <label class="mapping">{i18n.t('clone.name')} <input bind:this={nameInput} bind:value={name} maxlength="255" disabled={busy} /></label>
    <form onsubmit={(event) => { event.preventDefault(); void run(findHosts); }}>
      <label class="mapping">{i18n.t('clone.search')} <input bind:value={search} maxlength="128" disabled={busy} /></label>
      <button type="submit" disabled={busy || !connected}>{i18n.t('clone.find')}</button>
    </form>
    {#if truncated}<p>{i18n.t('clone.narrow_search')}</p>{/if}
    {#each setup.hosts as source (source.hostid)}
      <label class="mapping">
        {i18n.t('clone.target_for', { host: source.name })}
        <select bind:value={choices[source.hostid]} disabled={busy}>
          <option value="">{i18n.t('clone.choose')}</option>
          <option value="keep">{i18n.t('clone.keep_source')}</option>
          {#each hosts.filter((host) => host.hostid !== source.hostid) as host (host.hostid)}
            <option value={host.hostid} disabled={setup.hosts.some((other) => other.hostid !== source.hostid && choices[other.hostid] === host.hostid)}>{host.name}</option>
          {/each}
        </select>
      </label>
    {/each}
    {#if setup.growth_samples.length}
      <fieldset class="mapping">
        <legend>{i18n.t('clone.growth.title')}</legend>
        <label class="mapping">{i18n.t('clone.growth.sample')}
          <select bind:value={sampleIndex} disabled={busy}>
            <option value="">{i18n.t('clone.growth.none')}</option>
            {#each setup.growth_samples as row, index}
              <option value={String(index)}>{i18n.t('clone.growth.option', {
                page: row.field.widget.page_index + 1, widget: row.field.widget.widget_index + 1,
                dataset: row.dataset_index + 1, row: row.position + 1,
                host: row.source.host, item: row.source.item
              })}{row.widget_name ? ` · ${row.widget_name}` : ''}</option>
            {/each}
          </select>
        </label>
        {#if sample}
          <label class="mapping">{i18n.t('clone.growth.host')}
            <select bind:value={extraHost} disabled={busy}>
              <option value="">{i18n.t('clone.choose')}</option>
              {#each hosts as host (host.hostid)}
                <option value={host.hostid} disabled={host.hostid === sample.hostid || Object.values(choices).includes(host.hostid)}>{host.name}</option>
              {/each}
            </select>
          </label>
        {/if}
        <div aria-live="polite">
          {#if currentReview}
            <h4>{i18n.t('clone.growth.before')}</h4>
            <ol>{#each currentReview.before as row}<li>{row.host} · {row.item} <code>{row.key}</code></li>{/each}</ol>
            <h4>{i18n.t('clone.growth.after')}</h4>
            <ol>{#each currentReview.after as row, index}
              <li>{row.host} · {row.item} <code>{row.key}</code>
                {#if index >= currentReview.before.length}<strong>{i18n.t('clone.growth.added')}</strong>{/if}
              </li>
            {/each}</ol>
          {/if}
        </div>
      </fieldset>
    {/if}
    <Button {i18n} label={{ id: 'clone.lld.review' }} onclick={reviewBindings} disabled={busy || !canCheckBindings} />
    <div aria-live="polite">
      {#if lldRows?.length === 0}<p>{i18n.t('clone.lld.none_required')}</p>{/if}
    </div>
    {#each lldRows ?? [] as row (row.fingerprint)}
      <fieldset class="mapping">
        <legend>{i18n.t('clone.lld.row', { page: row.field.widget.page_index + 1, widget: row.field.widget.widget_index + 1, host: row.target_host })}</legend>
        <p>{row.source_host} · {row.source.name} <code>{row.source.key}</code></p>
        {#if row.prototype_key}<p>{i18n.t('clone.lld.prototype')} <code>{row.prototype_key}</code></p>{/if}
        {#each row.candidates as candidate (candidate.itemid)}
          <label class="lld-choice">
            <input type="radio" name={`clone-lld-${row.fingerprint}`} value={candidate.itemid}
              checked={lldSelections[row.fingerprint] === candidate.itemid}
              onchange={() => { lldSelections[row.fingerprint] = candidate.itemid; }}
              disabled={busy || candidate.value_type !== row.source.value_type} />
            <span>{candidate.name} · <code>{candidate.key}</code><br />
              {valueTypeText(i18n, candidate.value_type)}{candidate.units ? ` · ${candidate.units}` : ''}</span>
          </label>
        {:else}
          <p>{i18n.t('bindings.candidates.none')}</p>
        {/each}
      </fieldset>
    {/each}
    {#if setup.patterns.length}
      <h3>{i18n.t('clone.pattern.title')}</h3>
      {#each setup.patterns as row (fieldKey(row.field))}
        {@const edit = patternEdits[fieldKey(row.field)]}
        {@const preview = currentPatternReview(edit)}
        <fieldset class="mapping">
          <legend>{i18n.t(row.kind === 'host' ? 'clone.pattern.host_row' : 'clone.pattern.item_row', {
            page: row.field.widget.page_index + 1, widget: row.field.widget.widget_index + 1
          })}</legend>
          <p><code>{row.field.field_name}</code> · <code>{row.source_pattern}</code></p>
          <label class="mapping">{i18n.t('clone.pattern.decision')}
            <select bind:value={edit.action} disabled={busy}>
              <option value="">{i18n.t('clone.pattern.choose')}</option>
              <option value="keep">{i18n.t('clone.pattern.keep')}</option>
              <option value="replace">{i18n.t('clone.pattern.replace')}</option>
            </select>
          </label>
          {#if edit.action === 'keep'}
            <p>{i18n.t('clone.keep_patterns')}</p>
          {:else if edit.action === 'replace'}
            <label class="mapping">{i18n.t('clone.pattern.value')}
              <input type="text" maxlength="255" bind:value={edit.value} disabled={busy} />
            </label>
            <p>{i18n.t('clone.pattern.scope')}</p>
            <Button {i18n} label={{ id: 'clone.pattern.preview' }} onclick={() => previewPattern(row)}
              disabled={busy || !canCheckBindings || !edit.value.trim()} />
            <div aria-live="polite">
              {#if preview}
                <p>{i18n.t('clone.pattern.count', { count: preview.match_count })}</p>
                {#if preview.matches.length}
                  <ul>{#each preview.matches as item}<li>{item.host === item.name ? item.name : `${item.host} · ${item.name}`}</li>{/each}</ul>
                {/if}
                {#if preview.truncated}<p>{i18n.t('clone.pattern.truncated')}</p>{/if}
              {/if}
            </div>
          {/if}
        </fieldset>
      {/each}
    {/if}
    {#if setup.unattributed > 0}<p role="status">{i18n.t('clone.blocked')}</p>{/if}
    {#if sample}<Button {i18n} label={{ id: 'clone.growth.preview' }} onclick={previewGrowth} disabled={busy || !canPreview} />{/if}
    <Button {i18n} variant="primary" label={{ id: 'clone.save' }} onclick={save} disabled={busy || !canSave} />
  {:else if !busy}
    <Button {i18n} label={{ id: 'clone.open' }} onclick={begin} disabled={!connected} />
  {/if}
  {#if busy}<p role="status">{i18n.t('clone.working')}</p>{/if}
  {#if failure}
    {@const message = setupErrors[failure.code]}
    <p role="alert">{message ? i18n.t(message) : resolveText(i18n, publicationCodeText(i18n, failure))}</p>
  {/if}
  {#if outcome}
    <p role="status">{resolveText(i18n, resultBody(i18n, outcome.state, outcome.kind, outcome.evidence, outcome.invalidated))}</p>
    {#if outcome.entry?.progress === 'needs_reconciliation'}
      <Button {i18n} label={{ id: 'clone.reconcile' }} onclick={reconcile} disabled={busy || !connected} />
    {/if}
  {/if}
</section>

<Dialog {i18n} id="clone-confirm" title={{ id: 'clone.confirm_title' }}
  description={{ id: 'clone.private_copy' }} open={plan !== null} onclose={cancelPlan}>
  {#if plan}<p>{plan.name}</p>{/if}
  {#snippet actions()}
    <Button {i18n} label={{ id: 'clone.cancel' }} onclick={cancelPlan} />
    <Button {i18n} variant="primary" label={{ id: 'clone.write' }} onclick={confirm} />
  {/snippet}
</Dialog>

<style>
  section {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    justify-items: start;
    gap: var(--space-3, .75rem);
    overflow-wrap: anywhere;
  }
  section > *, form > label { min-inline-size: 0; max-inline-size: 100%; }
  .actions, form { display: flex; flex-wrap: wrap; gap: var(--space-2, .5rem); align-items: end; }
  .mapping { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--space-1, .25rem); }
  fieldset { min-inline-size: 0; max-inline-size: 100%; margin-inline: 0; }
  input:not([type='checkbox']), select, button { min-block-size: 32px; min-inline-size: 0; max-inline-size: 100%; }
  label:has(input[type='checkbox']) { min-block-size: 32px; display: inline-flex; align-items: center; gap: .5rem; }
  .lld-choice { min-block-size: 32px; display: flex; align-items: center; gap: .5rem; }
  .clone-comparison { inline-size: 100%; }
  .clone-comparison summary { min-block-size: 32px; }
</style>
