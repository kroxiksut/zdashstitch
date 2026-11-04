<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Icon, resolveText } from '../components';
  import { widgetTypeName } from '../shell/format';
  import {
    CHANGE_ICON,
    CHANGE_LABEL,
    FITNESS_LABEL,
    MATCH_ICON,
    MATCH_LONG,
    checkReasonText,
    differencesText,
    reasonText,
    sourceKindText,
    valueTypeText
  } from './messages';
  import type { BindingRow, ItemFacts, ReplaceHostPreview } from './model';
  import type { ReplaceHostStore } from './store.svelte';
  import { pageLabel, sourceText, targetText, widgetLabel, widgetOf } from './view';

  // Inspector of one 09-03 row (P1-05D «Кандидаты» / «Основания»): what refers where, the match
  // and its reasons, item evidence (key, type of information, units, origin), candidates as a
  // radio group with «Выбрать», «Оставить на исходном» and undo. Every decision is a local
  // operation of the draft; nothing is chosen automatically and the server is not changed.
  let { i18n, store, preview, row }: {
    i18n: I18n;
    store: ReplaceHostStore;
    preview: ReplaceHostPreview;
    row: BindingRow | null;
  } = $props();

  let candidate = $state<string | null>(null);

  const chooseKind = $derived(
    row?.allowed_decisions.includes('choose_candidate')
      ? 'choose_candidate'
      : row?.allowed_decisions.includes('choose_lld_instance')
        ? 'choose_lld_instance'
        : null
  );
  const canKeep = $derived(row?.allowed_decisions.includes('keep_on_source') ?? false);
  const busy = $derived(store.busy !== null);
  const host = $derived(preview.operation.source_host.name);
  const target = $derived(preview.operation.target_host?.name ?? '—');

  $effect(() => {
    // A new row starts without a pre-selected candidate: nothing is chosen automatically.
    void row?.key;
    candidate = null;
  });

  function originText(item: ItemFacts): string {
    if (item.prototype_key) return i18n.t('bindings.evidence.origin.lld', { key: item.prototype_key });
    if (item.template_itemid) return i18n.t('bindings.evidence.origin.template');
    return i18n.t('bindings.evidence.origin.own');
  }

  function choose(): void {
    if (!row || !chooseKind || candidate === null) return;
    void store.decide(row.key, { kind: chooseKind, itemid: candidate });
  }
</script>

{#snippet evidence(item: ItemFacts)}
  <ul class="evidence">
    <li>{i18n.t('bindings.evidence.key', { key: item.key })}</li>
    <li>{i18n.t('bindings.evidence.value_type', { type: valueTypeText(i18n, item.value_type) })}</li>
    <li>{item.units === '' ? i18n.t('bindings.evidence.no_units') : i18n.t('bindings.evidence.units', { units: item.units })}</li>
    <li>{originText(item)}</li>
  </ul>
{/snippet}

<aside class="card inspector" aria-labelledby="row-inspector-title">
  <h3 id="row-inspector-title" class="eyebrow" tabindex="-1">{i18n.t('bindings.inspector.title')}</h3>
  {#if row === null}
    <p class="muted">{i18n.t('bindings.inspector.none')}</p>
  {:else}
    {@const widget = widgetOf(preview, row.key.page_index, row.key.widget_index)}
    <p class="inspector-object">
      <strong>{widgetLabel(i18n, widget, row.widget_type)}</strong>
      <span class="muted">{pageLabel(i18n, preview, row.key.page_index)} · {widgetTypeName(i18n, row.widget_type)}</span>
      <span class="muted technical">{i18n.t('bindings.inspector.field', { field: row.key.field })}</span>
      <span class="muted">{resolveText(i18n, sourceKindText(row.source.kind))}</span>
    </p>
    <p class="technical">{sourceText(preview, row)} → {row.state === 'changed' ? targetText(preview, row) : '—'}</p>

    <p class="marker">
      <Icon name={CHANGE_ICON[row.state]} size={18} />{resolveText(i18n, CHANGE_LABEL[row.state])}
    </p>
    {#if row.match}
      <p class="marker"><Icon name={MATCH_ICON[row.match]} size={18} />{resolveText(i18n, MATCH_LONG[row.match])}</p>
    {/if}
    {#if row.applicability}<p class="muted">{resolveText(i18n, FITNESS_LABEL[row.applicability])}</p>{/if}
    {#if row.check !== 'checked'}
      <p class="marker tone-danger">
        <Icon name="circle-dashed" size={18} />{i18n.t(row.check === 'stale' ? 'bindings.state.stale' : 'bindings.state.invalid')}
        {#if row.check_reason}<span>{resolveText(i18n, checkReasonText(row.check_reason))}</span>{/if}
      </p>
    {/if}

    <h4 class="eyebrow">{i18n.t('bindings.source_evidence')}</h4>
    <ul class="reasons">
      {#if row.kept_reason}<li>{resolveText(i18n, reasonText(row.kept_reason))}</li>{/if}
      {#each row.reasons.filter((reason) => reason !== row.kept_reason) as reason (reason)}
        <li>{resolveText(i18n, reasonText(reason))}</li>
      {/each}
      {#if row.differences.length > 0}<li>{i18n.t('bindings.evidence.differences', { differences: differencesText(i18n, row.differences) })}</li>{/if}
      {#each row.pattern_coverage as coverage (coverage.pattern)}
        <li>{i18n.t('bindings.evidence.coverage', { pattern: coverage.pattern, source: coverage.source_matches, target: coverage.target_matches })}</li>
      {/each}
    </ul>
    {#if row.source_item}
      <h4 class="eyebrow">{i18n.t('bindings.evidence.source_item')}</h4>
      {@render evidence(row.source_item)}
    {/if}
    {#if row.target_item && row.state === 'changed'}
      <h4 class="eyebrow">{i18n.t('bindings.evidence.target_item')}</h4>
      {@render evidence(row.target_item)}
    {/if}

    {#if row.decision_state === 'applied' && row.decision}
      <p class="marker" role="note">
        <Icon name="circle-check" size={18} />
        {row.decision.kind === 'keep_on_source'
          ? i18n.t('bindings.decision.applied_keep')
          : i18n.t('bindings.decision.applied_candidate', { key: row.target_item?.key ?? row.decision.itemid })}
      </p>
    {:else if row.decision_state === 'invalid'}
      <p class="marker tone-danger" role="note"><Icon name="circle-x" size={18} />{i18n.t('bindings.decision.invalid')}</p>
    {/if}

    {#if chooseKind && row.decision_state !== 'applied'}
      <fieldset class="choice-group candidates">
        <legend>{i18n.t('bindings.candidates.legend', { host: target })}</legend>
        {#each row.candidates as option (option.itemid)}
          <label class="choice">
            <input type="radio" name="row-candidate" value={option.itemid} checked={candidate === option.itemid} onchange={() => (candidate = option.itemid)} disabled={busy} />
            <span class="technical">{option.key}</span>
            <span class="description">
              {option.name} · {valueTypeText(i18n, option.value_type)}{option.units === '' ? '' : ` · ${option.units}`} · {originText(option)}
            </span>
          </label>
        {/each}
      </fieldset>
      <div class="actions">
        <Button {i18n} variant="primary" label={chooseKind === 'choose_lld_instance' ? { id: 'bindings.mapping.choose_lld_instance' } : { id: 'bindings.decision.choose' }} disabled={candidate === null || busy} busy={store.busy === 'decide'} onclick={choose} />
      </div>
    {:else if (row.match === 'ambiguous' || row.match === 'unresolved_lld') && row.candidates.length === 0 && row.decision_state !== 'applied'}
      <p class="muted">{i18n.t('bindings.candidates.none')}</p>
    {/if}

    {#if row.state === 'unresolved' && row.decision_state !== 'applied'}<p class="muted">{i18n.t('bindings.decision.unresolved')}</p>{/if}

    <div class="actions">
      {#if canKeep && row.decision?.kind !== 'keep_on_source'}
        <Button {i18n} label={{ id: 'bindings.decision.keep_on_source' }} disabled={busy} busy={store.busy === 'decide'} onclick={() => row && void store.decide(row.key, { kind: 'keep_on_source' })} />
      {/if}
      {#if row.decision_seq !== null}
        <Button {i18n} variant="quiet" label={{ id: 'bindings.decision.undo' }} disabled={busy} busy={store.busy === 'undo'} onclick={() => row?.decision_seq != null && void store.undo(row.decision_seq)} />
      {/if}
    </div>
    {#if canKeep && row.decision?.kind !== 'keep_on_source'}<p class="muted">{i18n.t('bindings.decision.keep_hint', { host })}</p>{/if}
  {/if}
</aside>
