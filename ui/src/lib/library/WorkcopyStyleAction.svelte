<script lang="ts">
  import { onMount } from 'svelte';
  import { Dialog } from '../components';
  import type { I18n } from '../i18n';
  import { createCatalogClient, reusableId, type CopyStylePreview, type CopyStyleRequest, type CopyStyleState, type ReusableEntry } from './client';
  import { displayReusableName, reusableError } from './reusable';

  let { i18n, id, serverVersion, page, widget, widgetNames }: {
    i18n: I18n; id: string; serverVersion: string; page: number; widget?: number; widgetNames: string[];
  } = $props();
  const client = createCatalogClient();
  let styleState = $state<CopyStyleState | null>(null);
  let entries = $state<ReusableEntry[]>([]);
  let open = $state(false);
  let busy = $state(false);
  let chosen = $state('');
  let message = $state('');
  let checked = $state<{ request: CopyStyleRequest; preview: CopyStylePreview } | null>(null);
  const supported = $derived(['7.4.16', '8.0.0'].includes(serverVersion));

  onMount(() => {
    if (supported) void client.copyStyleState(id).then((result) => styleState = result).catch((error) => message = reusableError(i18n, error));
  });

  async function show(): Promise<void> {
    if (busy || !supported) return;
    open = true; chosen = ''; checked = null; message = ''; busy = true;
    try {
      const [stored, candidates] = await Promise.all([client.copyStyleState(id), client.reusable()]);
      styleState = stored;
      entries = candidates.filter((entry) => entry.asset.kind === 'style_profile'
        && entry.asset.document.adapter === serverVersion.split('.').slice(0, 2).join('.')
        && (entry.asset.document.widget_types.svggraph?.series_palette?.length
          || (serverVersion === '8.0.0' && typeof entry.asset.document.widget_types.item?.widget_fields?.bg_color === 'string')));
    } catch (error) { message = reusableError(i18n, error); }
    finally { busy = false; }
  }

  async function review(): Promise<void> {
    const asset = entries.find((entry) => reusableId(entry.asset) === chosen);
    if (!asset || !styleState || busy) return;
    busy = true; message = '';
    const request: CopyStyleRequest = { expected_revision: styleState.revision, profile_id: chosen, profile_revision: asset.revision, page, ...(widget === undefined ? {} : { widgets: [widget] }) };
    try { checked = { request, preview: await client.copyStyle(id, request, false) }; }
    catch (error) { message = reusableError(i18n, error); }
    finally { busy = false; }
  }

  async function apply(): Promise<void> {
    const candidate = checked;
    if (!candidate?.preview.changes.length || busy) return;
    busy = true; message = '';
    try {
      await client.copyStyle(id, $state.snapshot(candidate.request), true);
      styleState = await client.copyStyleState(id);
      open = false; checked = null;
    } catch (error) { message = reusableError(i18n, error); checked = null; }
    finally { busy = false; }
  }

  async function undo(): Promise<void> {
    if (busy || !styleState?.undo) return;
    busy = true; message = '';
    try { styleState = await client.undoCopyStyle(id, styleState.revision); }
    catch (error) { message = reusableError(i18n, error); }
    finally { busy = false; }
  }
  function close(): void { if (!busy) { open = false; checked = null; } }
</script>

{#if supported}
  <button type="button" disabled={busy} onclick={() => void show()}>{i18n.t('reusable.style_action')}</button>
  {#if styleState?.has_style}<p>{i18n.t(styleState.publication && serverVersion === '8.0.0' ? 'reusable.copy_style_review_publish' : 'reusable.copy_style_publish')}</p>{/if}
  {#if styleState?.undo}<button type="button" disabled={busy} onclick={() => void undo()}>{i18n.t('editor.action.undo', { action: i18n.t('reusable.history_action') })}</button>{/if}
  {#if message && !open}<p role="alert">{message}</p>{/if}
{/if}
{#if open}
  <Dialog {i18n} id="workcopy-style" title={{ id: 'reusable.style_action' }} description={{ id: 'reusable.copy_style_review_publish' }} open={true} cancellable={!busy} onclose={close}>
    {#if entries.length}
      <label>{i18n.t('reusable.choose')} <select bind:value={chosen} disabled={busy} onchange={() => { checked = null; }}>
        <option value="">{i18n.t('reusable.choose_value')}</option>
        {#each entries as entry (reusableId(entry.asset))}<option value={reusableId(entry.asset)}>{displayReusableName(i18n, entry)}</option>{/each}
      </select></label>
    {:else if !busy && !message}<p>{i18n.t('reusable.no_objects')}</p>{/if}
    {#if message}<p role="alert">{message}</p>{/if}
    {#if checked}
      {#if checked.preview.skipped}<p>{i18n.t('reusable.copy_style_skipped', { count: checked.preview.skipped })}</p>{/if}
      {#if !checked.preview.changes.length}<p>{i18n.t('reusable.no_changes')}</p>{/if}
      <ul>{#each checked.preview.changes as change (`${change.widget}/${change.field}`)}
        <li>{widgetNames[change.widget]} · <code>{change.field}</code> · {change.before || i18n.t('reusable.before_default')} → {change.after || i18n.t('reusable.before_default')}</li>
      {/each}</ul>
    {/if}
    {#snippet actions()}
      {#if checked}
        <button type="button" disabled={busy || !checked.preview.changes.length} onclick={() => void apply()}>{i18n.t('reusable.apply')}</button>
      {:else}
        <button type="button" disabled={busy || !chosen} onclick={() => void review()}>{i18n.t('reusable.review')}</button>
      {/if}
      <button type="button" disabled={busy} onclick={close}>{i18n.t('reusable.cancel')}</button>
    {/snippet}
  </Dialog>
{/if}
