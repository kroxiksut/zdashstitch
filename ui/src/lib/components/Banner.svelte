<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type DisplayText, type LocalizedText } from './text';

  let { i18n, heading, body, tone = 'info', blocking = false, actions }: {
    i18n: I18n;
    heading: LocalizedText;
    body: DisplayText;
    tone?: 'success' | 'info' | 'warning' | 'danger';
    blocking?: boolean;
    actions?: Snippet;
  } = $props();
</script>

<section class="banner {tone}" role={blocking ? 'alert' : 'status'} aria-live={blocking ? 'assertive' : 'polite'}>
  <h2>{resolveText(i18n, heading)}</h2>
  <p>{resolveText(i18n, body)}</p>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</section>
