<script lang="ts">
  import { tick } from 'svelte';
  import type { I18n } from '../i18n';
  import { Button, Icon, StatusBadge } from '../components';
  import ComingLater from './ComingLater.svelte';
  import RenameDialog from './RenameDialog.svelte';
  import type { DraftRenameResult, DraftSummary } from './drafts';
  import { createdDashboardLines, detailRows } from './format';
  import { KIND_LABEL, ORIGIN_LABEL } from './labels';
  import { draftPath } from './router';

  // Details column of «Drafts» (P1-03F, mockup 02-01): what the draft is, where it came from and
  // what was published from it. «Open» and «Rename» work; the other actions are later entries and
  // stay visible but unavailable with «Will be available later» (P1-01H §6).
  let { i18n, draft, onrename }: {
    i18n: I18n;
    draft: DraftSummary;
    /** Absent while the storage cannot rename (e.g. without a backend). */
    onrename?: (localId: string, name: string) => Promise<DraftRenameResult>;
  } = $props();

  const rows = $derived(detailRows(i18n, draft));
  const created = $derived(draft.created_dashboards ?? []);
  let renaming = $state(false);

  async function closeRename(): Promise<void> {
    if (!renaming) return;
    renaming = false;
    await tick();
    document.getElementById('drafts-details-rename')?.focus();
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
  <ComingLater {i18n} id="drafts-details-actions">
    <Button {i18n} label={{ id: 'common.action.duplicate' }} disabled />
    {#if !onrename}<Button {i18n} label={{ id: 'common.action.rename' }} disabled />{/if}
    {#if draft.origin === 'server_copy'}
      <Button {i18n} label={{ id: 'drafts.action.make_portable' }} disabled />
    {:else}
      <Button {i18n} label={{ id: 'drafts.action.export_file' }} disabled />
    {/if}
    <Button {i18n} label={{ id: 'common.action.delete' }} disabled />
  </ComingLater>
</div>

<p class="details-note"><Icon name="info-circle" size={18} /><span>{i18n.t('drafts.delete.safe')}</span></p>
{#if draft.origin === 'server_copy'}
  <p class="hint">{i18n.t('drafts.server_copy_export_hint')}</p>
{/if}

{#if renaming && onrename}
  <RenameDialog {i18n} {draft} {onrename} onclose={() => void closeRename()} />
{/if}
