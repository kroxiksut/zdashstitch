<script lang="ts">
  import { Button, Dialog, resolveText } from '../components';
  import type { I18n } from '../i18n';
  import { createHttpPublicationClient, publicationFailure, type AccessChoices } from './client';
  import DashboardAccessSummary from './DashboardAccessSummary.svelte';
  import { publicationCodeText, resultBody } from './messages';
  import type { Outcome, PlanPreview, PublicationFailure } from './model';

  let { i18n, localId, sourceName, writable, connected }: {
    i18n: I18n; localId: string; sourceName: string; writable: boolean; connected: boolean;
  } = $props();

  const client = createHttpPublicationClient();
  let name = $state('');
  let restore = $state(false);
  let publicAcknowledged = $state(false);
  let choices = $state<AccessChoices | null>(null);
  let prepared = $state<{ handle: string; plan: Extract<PlanPreview, { action: 'save_as_copy' }> } | null>(null);
  let outcome = $state<Outcome | null>(null);
  let failure = $state<PublicationFailure | null>(null);
  let busy = $state(false);

  async function run(operation: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    failure = null;
    try { await operation(); }
    catch (error) { failure = publicationFailure(error); }
    finally { busy = false; }
  }

  function prepare(): void {
    if (!writable || name.trim() === '' || prepared) return;
    const target = name.trim();
    const restoreAccess = restore;
    void run(async () => {
      const source = await client.copySource(localId);
      const next = await client.plan(localId, {
        action: 'save_as_copy', ...source, name: target, restore_source_access: restoreAccess
      });
      if (next.plan.action !== 'save_as_copy') return;
      choices = next.plan.access ? await client.accessChoices().catch(() => null) : null;
      publicAcknowledged = false;
      prepared = { handle: next.handle, plan: next.plan };
    });
  }

  function cancel(): void {
    const current = prepared;
    prepared = null;
    if (current) void run(async () => client.discard(localId, current.handle));
  }

  function confirm(): void {
    const current = prepared;
    if (!current || busy || (!current.plan.private && !publicAcknowledged)) return;
    prepared = null;
    void run(async () => {
      outcome = await client.confirm(localId, {
        handle: current.handle, plan_id: current.plan.plan_id, name: current.plan.target_name, confirmed: true,
        ...(!current.plan.private ? { acknowledge_public_access: true } : {})
      });
    });
  }

  function reconcile(): void {
    const id = outcome?.operation_id;
    if (connected && id) void run(async () => { outcome = await client.reconcile(id); });
  }
</script>

<section class="workspace-section" aria-labelledby="save-copy-title">
  <h2 id="save-copy-title">{i18n.t('publication.target.save_copy')}</h2>
  <label>
    {i18n.t('publication.target.name')}
    <input bind:value={name} placeholder={i18n.t('drafts.copy_name', { name: sourceName, number: 2 })}
      disabled={!writable || busy || prepared !== null} />
  </label>
  <label>
    <input type="checkbox" bind:checked={restore} disabled={!writable || busy || prepared !== null} />
    {i18n.t('publication.access.restore_source')}
  </label>
  {#if !writable}<p>{i18n.t('publication.confirm.offline')}</p>{/if}
  <Button {i18n} label={{ id: 'publication.action.confirm_dialog' }} variant="primary" onclick={prepare}
    disabled={!writable || busy || prepared !== null || name.trim() === ''} />
  {#if busy}<p role="status">{i18n.t('publication.confirm.building')}</p>{/if}
  {#if failure}<p role="alert">{resolveText(i18n, publicationCodeText(i18n, failure))}</p>{/if}
  {#if outcome}
    <p role="status">{resolveText(i18n, resultBody(i18n, outcome.state, outcome.kind, outcome.evidence, outcome.invalidated))}</p>
    {#if outcome.entry?.progress === 'needs_reconciliation'}
      <Button {i18n} label={{ id: 'clone.reconcile' }} onclick={reconcile} disabled={busy || !connected} />
    {/if}
  {/if}
</section>

<Dialog {i18n} id="save-copy-confirm" title={{ id: 'publication.confirm.title' }}
  description={{ id: 'publication.confirm.question_create', params: { name: prepared?.plan.target_name ?? '' } }}
  open={prepared !== null} onclose={cancel}>
  {#if prepared}
    {#if prepared.plan.access}
      <DashboardAccessSummary {i18n} access={prepared.plan.access} {choices} label={i18n.t('publication.access.after')} />
      {#if prepared.plan.access_visible_only}<p>{i18n.t('clone.copied_access_visible_part')}</p>{/if}
      {#if !prepared.plan.private}
        <label><input type="checkbox" bind:checked={publicAcknowledged} /> {i18n.t('publication.access.public_warning')}</label>
      {/if}
    {:else}
      <p>{i18n.t('publication.access.copy_private')}</p>
    {/if}
  {/if}
  {#snippet actions()}
    <Button {i18n} label={{ id: 'clone.cancel' }} onclick={cancel} />
    <Button {i18n} label={{ id: 'clone.write' }} variant="primary" onclick={confirm}
      disabled={prepared?.plan.private === false && !publicAcknowledged} />
  {/snippet}
</Dialog>

<style>
  section { display: grid; gap: var(--space-3, .75rem); justify-items: start; }
  label { display: flex; gap: var(--space-2, .5rem); align-items: center; min-block-size: 32px; }
</style>
