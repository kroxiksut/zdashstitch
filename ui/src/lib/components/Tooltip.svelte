<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';
  let { i18n, id, text, trigger }: { i18n: I18n; id: string; text: LocalizedText; trigger: Snippet } = $props();
  let visible = $state(false);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<span class="tooltip-anchor" role="group" aria-describedby={id} onmouseenter={() => { visible = true; }} onmouseleave={() => { visible = false; }} onfocusin={() => { visible = true; }} onfocusout={() => { visible = false; }} onkeydown={(event) => { if (event.key === 'Escape') visible = false; }}>
  {@render trigger()}
  {#if visible}<span {id} role="tooltip">{resolveText(i18n, text)}</span>{/if}
</span>
