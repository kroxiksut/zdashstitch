<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, label, value, maximum = 100 }: { i18n: I18n; label: LocalizedText; value?: number; maximum?: number } = $props();
</script>

<div class="progress" role="status" aria-live="polite">
  <span>{resolveText(i18n, label)}</span>
  {#if value === undefined}
    <progress aria-label={resolveText(i18n, label)}></progress>
  {:else}
    <progress aria-label={resolveText(i18n, label)} value={value} max={maximum}></progress>
  {/if}
</div>
