<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';
  let { i18n, label, lines = 3 }: { i18n: I18n; label: LocalizedText; lines?: number } = $props();
</script>

<div class="skeleton-wrap">
  <span role="status">{resolveText(i18n, label)}</span>
  <div class="skeleton" aria-hidden="true">{#each Array(lines) as _}<span></span>{/each}</div>
</div>
