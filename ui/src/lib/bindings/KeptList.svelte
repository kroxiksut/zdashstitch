<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Icon, StatusBadge, resolveText } from '../components';
  import { widgetTypeName } from '../shell/format';
  import { reasonText } from './messages';
  import type { ReplaceHostPreview } from './model';
  import type { ReplaceHostStore } from './store.svelte';
  import { pageLabel, widgetLabel, widgetOf } from './view';

  // «Останутся на старом узле» (O-003, P1-05B/E, P1-08C): widgets of unsupported types, masks and
  // «keep on source» decisions, with the one action «Подтвердить список». The confirmation is bound
  // to exactly this list; a later change of the list asks for it again.
  let { i18n, store, preview, idPrefix }: {
    i18n: I18n;
    store: ReplaceHostStore;
    preview: ReplaceHostPreview;
    idPrefix: string;
  } = $props();

  const kept = $derived(preview.kept_on_source);
  const host = $derived(preview.operation.source_host.name);
  const widgets = $derived(new Set(kept.entries.map((entry) => `${entry.page_index}:${entry.widget_index}`)).size);
</script>

{#if kept.entries.length > 0}
  <section class="kept-box" aria-labelledby={`${idPrefix}-kept-title`}>
    <h4 id={`${idPrefix}-kept-title`} class="kept-title"><Icon name="ban" />{i18n.t('bindings.kept.title', { count: widgets, host })}</h4>
    <ul class="kept-list">
      {#each kept.entries as entry (`${entry.page_index}:${entry.widget_index}:${entry.field}`)}
        {@const widget = widgetOf(preview, entry.page_index, entry.widget_index)}
        <li>
          <span class="widget-name">{widgetLabel(i18n, widget, entry.widget_type)}</span>
          <span class="muted">{pageLabel(i18n, preview, entry.page_index)} · {widgetTypeName(i18n, entry.widget_type)}</span>
          {#if entry.reason}<span>{resolveText(i18n, reasonText(entry.reason))}</span>{/if}
          {#if entry.by_decision}<span class="muted">{i18n.t('bindings.kept.by_decision')}</span>{/if}
        </li>
      {/each}
    </ul>
    <p>{i18n.t('bindings.kept.not_blocking')}</p>
    {#if kept.confirmed}
      <StatusBadge {i18n} label={{ id: 'bindings.kept.confirmed' }} tone="success">
        {#snippet icon()}<Icon name="circle-check" size={16} />{/snippet}
      </StatusBadge>
    {:else}
      <div class="actions">
        <Button
          {i18n}
          variant="primary"
          label={{ id: 'publication.check.action.confirm_kept', params: { count: kept.entries.length } }}
          describedby={`${idPrefix}-kept-title`}
          disabled={store.busy !== null}
          busy={store.busy === 'confirm'}
          onclick={() => void store.confirmKept()}
        />
      </div>
    {/if}
  </section>
{/if}
