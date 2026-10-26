<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  // `level` 1 is for a screen whose only content is this empty state, so it keeps one main heading.
  let { i18n, heading, reason, action, level = 2 }: { i18n: I18n; heading: LocalizedText; reason: LocalizedText; action?: Snippet; level?: 1 | 2 } = $props();
</script>

<section class="empty-state">
  <svelte:element this={level === 1 ? 'h1' : 'h2'}>{resolveText(i18n, heading)}</svelte:element>
  <p>{resolveText(i18n, reason)}</p>
  {#if action}{@render action()}{/if}
</section>
