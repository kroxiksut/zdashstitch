<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';
  let { i18n, label, children }: { i18n: I18n; label: LocalizedText; children?: Snippet } = $props();
</script>

<aside class="drawer" aria-label={resolveText(i18n, label)}>{#if children}{@render children()}{/if}</aside>
