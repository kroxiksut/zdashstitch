<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, label, tone = 'info', icon, count }: {
    i18n: I18n;
    label: LocalizedText;
    tone?: 'success' | 'info' | 'warning' | 'danger';
    icon?: Snippet;
    count?: number;
  } = $props();
</script>

<span class="status-badge" class:success={tone === 'success'} class:info={tone === 'info'} class:warning={tone === 'warning'} class:danger={tone === 'danger'}>
  {#if icon}<span aria-hidden="true">{@render icon()}</span>{/if}
  <span>{resolveText(i18n, label)}</span>
  {#if count !== undefined}<span>({i18n.number(count)})</span>{/if}
</span>
