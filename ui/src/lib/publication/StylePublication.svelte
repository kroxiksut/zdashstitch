<script lang="ts">
  import { onMount } from 'svelte';
  import { Button, Dialog, resolveText } from '../components';
  import type { I18n } from '../i18n';
  import { createCatalogClient, type CopyBlueprintState, type CopyStyleState } from '../library/client';
  import type { DraftDetail } from '../shell/drafts';
  import { createHttpPublicationClient, publicationFailure } from './client';
  import { publicationCodeText, resultBody } from './messages';
  import type { Outcome, PlanPreview, PublicationFailure } from './model';

  let { i18n, draft, writable, blueprint = false, onchanged }: { i18n: I18n; draft: DraftDetail; writable: boolean; blueprint?: boolean; onchanged?: () => void } = $props();
  const library = createCatalogClient();
  const client = createHttpPublicationClient();
  let changes = $state<CopyStyleState | CopyBlueprintState | null>(null);
  let prepared = $state<{ handle: string; plan: Extract<PlanPreview, { action: 'update_source' }> } | null>(null);
  let outcome = $state<Outcome | null>(null);
  let failure = $state<PublicationFailure | null>(null);
  let busy = $state(false);
  const dashboardName = $derived(draft.source?.dashboard_name ?? draft.name);
  const hasChanges = $derived(changes && ('has_style' in changes ? changes.has_style : changes.has_blueprint));

  async function refresh(): Promise<void> {
    changes = await (blueprint ? library.copyBlueprintState(draft.local_id) : library.copyStyleState(draft.local_id));
  }

  onMount(() => { void run(refresh); });

  async function run(operation: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    failure = null;
    try { await operation(); }
    catch (error) { failure = publicationFailure(error); }
    finally { busy = false; }
  }

  function prepare(): void {
    const current = changes;
    const diff = current?.publication;
    if (!writable || !current || !diff || prepared) return;
    outcome = null;
    void run(async () => {
      const next = await client.plan(draft.local_id, {
        action: blueprint ? 'update_blueprint' : 'update_style', revision: current.revision,
        base_fingerprint: diff.base_fingerprint, local_fingerprint: diff.local_fingerprint
      });
      if (next.plan.action === 'update_source') prepared = { handle: next.handle, plan: next.plan };
    });
  }

  function cancel(): void {
    const current = prepared;
    prepared = null;
    if (current) void run(async () => client.discard(draft.local_id, current.handle));
  }

  function confirm(): void {
    const current = prepared;
    if (!current || !writable || busy) return;
    prepared = null;
    void run(async () => {
      outcome = await client.confirm(draft.local_id, {
        handle: current.handle, plan_id: current.plan.plan_id, confirmed: true
      });
      if (outcome.base_advanced) onchanged?.();
      await refresh();
    });
  }
</script>

{#if hasChanges || failure || outcome}
  <section class="workspace-section">
    <h2>{i18n.t(blueprint ? 'reusable.publish_blueprint' : 'reusable.publish_style')}</h2>
    {#if changes?.publication}
      <Button {i18n} label={{ id: 'publication.action.confirm_dialog' }} variant="primary"
        disabled={!writable || busy || prepared !== null} onclick={prepare} />
    {:else if hasChanges}
      <p>{i18n.t(blueprint ? 'reusable.copy_blueprint_publish' : 'reusable.copy_style_publish')}</p>
    {/if}
    {#if failure}<p role="alert">{resolveText(i18n, publicationCodeText(i18n, failure))}</p>{/if}
    {#if outcome}<p role="status">{resolveText(i18n, resultBody(i18n, outcome.state, outcome.kind, outcome.evidence, outcome.invalidated))}</p>{/if}
  </section>
{/if}

{#if prepared && changes?.publication}
  {@const diff = changes.publication}
  {@const confirmationText = 'widget' in diff
    ? { id: 'reusable.confirm_publish_blueprint' as const, params: { widget: diff.widget.name, page: diff.page_name, dashboard: dashboardName } }
    : { id: 'publication.confirm.question_update' as const, params: { count: prepared.plan.replacement_count, dashboard_name: dashboardName } }}
  <Dialog {i18n} id={blueprint ? 'workcopy-blueprint-confirm' : 'workcopy-style-confirm'} open title={{ id: 'publication.confirm.title' }}
    description={confirmationText}
    onclose={cancel}>
    {#if 'changes' in diff}<ul>
      {#each diff.changes as change (`${change.page}/${change.widget}/${change.field}`)}
        {@const page = draft.kind === 'page' ? draft.pages[0] : draft.pages[change.page]}
        <li>{page?.name} · {page?.widgets[change.widget]?.name} · <code>{change.field}</code> · {change.before || i18n.t('reusable.before_default')} → {change.after || i18n.t('reusable.before_default')}</li>
      {/each}
    </ul>{/if}
    <p>{i18n.t('publication.confirm.api_update')}</p>
    <p>{i18n.t('publication.confirm.reread', { time: prepared.plan.created_at })}</p>
    {#snippet actions()}
      <Button {i18n} label={{ id: 'clone.cancel' }} onclick={cancel} />
      <Button {i18n} label={{ id: 'clone.write' }} variant="primary" onclick={confirm} disabled={!writable || busy} />
    {/snippet}
  </Dialog>
{/if}
