<script lang="ts">
  import type { I18n } from '../i18n';
  import { Icon, StatusBadge } from '../components';
  import { draftPath } from '../shell/router';
  import KeptList from './KeptList.svelte';
  import { blockingFindings, type ReplaceHostPreview } from './model';
  import type { ReplaceHostStore } from './store.svelte';

  // «Итог» of ReplaceHost (P1-05E): one replacement of the old host in every supported reference of
  // the scope; what changes, what does not, what stays on the old host (with «Подтвердить список»)
  // and what is unresolved. «К публикации» opens the preview (P2-03D); the server is not changed.
  let { i18n, store, preview, localId }: {
    i18n: I18n;
    store: ReplaceHostStore;
    preview: ReplaceHostPreview;
    localId: string;
  } = $props();

  const summary = $derived(preview.diff.summary);
  const blockers = $derived(blockingFindings(preview.report).length);
</script>

<section class="card summary" aria-labelledby="summary-title">
  <h3 id="summary-title">{i18n.t('bindings.summary.title')}</h3>
  <ul class="summary-list">
    <li class="marker"><Icon name="pencil" size={18} />{i18n.t('bindings.summary.changes', { fields: summary.fields.changed, widgets: summary.widgets.changed })}</li>
    <li class="marker"><Icon name="minus" size={18} />{i18n.t('bindings.summary.untouched', { widgets: summary.widgets.untouched })}</li>
    <li class="marker">
      <Icon name="ban" size={18} />{i18n.t('bindings.summary.kept', { count: preview.kept_on_source.entries.length })}
      {#if preview.kept_on_source.entries.length > 0}
        <StatusBadge {i18n} label={preview.kept_on_source.confirmed ? { id: 'bindings.kept.confirmed' } : { id: 'bindings.kept.unconfirmed' }} tone={preview.kept_on_source.confirmed ? 'success' : 'warning'} />
      {/if}
    </li>
    <li class="marker" class:tone-danger={summary.fields.unresolved > 0}><Icon name={summary.fields.unresolved > 0 ? 'circle-x' : 'circle-check'} size={18} />{i18n.t('bindings.summary.unresolved', { count: summary.fields.unresolved })}</li>
  </ul>
  <p class="muted">{i18n.t('bindings.summary.only_changes')}</p>

  <KeptList {i18n} {store} {preview} idPrefix="summary" />

  {#if blockers > 0}
    <p class="marker tone-danger"><Icon name="circle-x" size={18} />{i18n.t('bindings.summary.blocked', { count: blockers })}</p>
  {/if}
  <div class="actions">
    <a class="button-link primary" href={draftPath(localId, 'publication')}>{i18n.t('bindings.action.to_publication')}</a>
  </div>
  <p class="note"><Icon name="info-circle" />{i18n.t('bindings.draft_only')}</p>
</section>
