<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type DisplayText, type LocalizedText } from './text';
  let { i18n, label, items }: { i18n: I18n; label: LocalizedText; items: ReadonlyArray<{ label: DisplayText; href?: string }> } = $props();
</script>

<nav class="breadcrumb" aria-label={resolveText(i18n, label)}>
  <ol>{#each items as item, index}{#if item.href}<li><a href={item.href}>{resolveText(i18n, item.label)}</a></li>{:else}<li aria-current="page">{resolveText(i18n, item.label)}</li>{/if}{/each}</ol>
</nav>
