<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';
  let { i18n, separatorLabel, primary, detail, size = $bindable(60), onresize }: {
    i18n: I18n;
    separatorLabel: LocalizedText;
    primary: Snippet;
    detail: Snippet;
    size?: number;
    onresize?: (size: number) => void;
  } = $props();
  function resize(delta: number): void {
    size = Math.min(80, Math.max(20, size + delta));
    onresize?.(size);
  }
</script>

<div class="split-pane" style={`--primary-size:${size}%`}>
  <div class="split-primary">{@render primary()}</div>
  <!-- The ARIA separator is intentionally keyboard-adjustable. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="separator" role="separator" aria-label={resolveText(i18n, separatorLabel)} aria-orientation="vertical" aria-valuemin="20" aria-valuemax="80" aria-valuenow={size} tabindex="0" onkeydown={(event) => { if (event.key === 'ArrowLeft') resize(-5); if (event.key === 'ArrowRight') resize(5); }}></div>
  <div class="split-detail">{@render detail()}</div>
</div>
