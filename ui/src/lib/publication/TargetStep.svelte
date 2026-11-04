<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Icon, RadioGroup, TextField } from '../components';
  import type { ReplaceHostPreview } from '../bindings/model';
  import type { DraftDetail } from '../shell/drafts';
  import type { PublicationAction } from './model';
  import type { PublicationStore } from './store.svelte';

  // Step 1 «Назначение» for ReplaceHost (P1-08E/I, mockup 11-02): the action is chosen explicitly
  // (no default), the copy needs a name, «Не изменяется» lists what stays, and owner/sharing are
  // not written (P5-08 contract pending: a copy is private, an update does not send them).
  let { i18n, store, preview, draft, onnext }: {
    i18n: I18n;
    store: PublicationStore;
    preview: ReplaceHostPreview;
    draft: DraftDetail;
    onnext: () => void;
  } = $props();

  const dashboardName = $derived(draft.source?.dashboard_name ?? draft.name);
  const sourceHost = $derived(preview.operation.source_host.name);
  const untouched = $derived(preview.diff.summary.widgets.total - preview.diff.summary.widgets.changed);
  const getAction = (): string | undefined => store.action ?? undefined;
  const setAction = (value: string | undefined): void => {
    if (value === 'update_source' || value === 'save_as_copy') store.setAction(value as PublicationAction);
    // A suggestion the user can change; the server makes the final uniqueness check.
    if (value === 'save_as_copy' && store.copyName.trim() === '') store.copyName = i18n.t('drafts.copy_name', { name: dashboardName, number: 2 });
  };
  const nameMissing = $derived(store.action === 'save_as_copy' && store.copyName.trim() === '');
</script>

<div class="publication-target">
  <section class="card" aria-labelledby="publication-action-title">
    <h3 id="publication-action-title" class="eyebrow">{i18n.t('publication.target.title')}</h3>
    <p class="muted">{i18n.t('publication.target.choose_explicitly')}</p>
    <RadioGroup
      {i18n}
      name="publication-action"
      legend={{ id: 'publication.target.title' }}
      bind:value={getAction, setAction}
      options={[
        { value: 'save_as_copy', label: { id: 'publication.target.save_copy' }, description: { id: 'publication.target.save_copy_hint' } },
        {
          value: 'update_source',
          label: { id: 'publication.target.update_source', params: { dashboard_name: dashboardName } },
          description: { id: 'publication.target.update_source_hint' }
        }
      ]}
    />
    {#if store.action === 'update_source'}
      <p class="note"><Icon name="alert-triangle" />{i18n.t('publication.target.update_visible_to_all')}</p>
    {/if}
    {#if store.action === 'save_as_copy'}
      <TextField
        {i18n}
        id="publication-copy-name"
        label={{ id: 'publication.target.name' }}
        bind:value={store.copyName}
        required
        hint={{ id: 'publication.target.name_server_check' }}
        error={nameMissing ? { id: 'publication.target.name_required' } : undefined}
      />
    {/if}
    <p class="muted">{i18n.t('publication.target.no_retarget')}</p>
  </section>

  <section class="card" aria-labelledby="publication-untouched-title">
    <h3 id="publication-untouched-title" class="eyebrow">{i18n.t('publication.untouched.title')}</h3>
    <ul class="plain-list">
      {#if store.action === 'save_as_copy'}
        <li>{i18n.t('publication.untouched.source', { dashboard_name: dashboardName })}</li>
      {:else}
        <li>{i18n.t('publication.untouched.widgets', { count: untouched, host: sourceHost })}</li>
        <li>{i18n.t('publication.untouched.pages')}</li>
        <li>{i18n.t('publication.untouched.access')}</li>
      {/if}
      <li>{i18n.t('publication.untouched.zabbix_objects')}</li>
    </ul>
  </section>

  <section class="card" aria-labelledby="publication-access-title">
    <h3 id="publication-access-title" class="eyebrow">{i18n.t('publication.access.title')}</h3>
    <p>{store.action === 'save_as_copy' ? i18n.t('publication.access.copy_private') : i18n.t('publication.access.update_not_sent')}</p>
  </section>

  <div class="actions">
    <Button {i18n} variant="primary" label={{ id: 'publication.action.next_checks' }} disabled={store.action === null || nameMissing} onclick={onnext} />
  </div>
</div>
