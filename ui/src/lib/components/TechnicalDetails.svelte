<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type DisplayText, type LocalizedText } from './text';

  let { i18n, summary, values }: {
    i18n: I18n;
    summary: LocalizedText;
    values: ReadonlyArray<{ label: LocalizedText; value: DisplayText }>;
  } = $props();
</script>

<details class="technical-details">
  <summary>{resolveText(i18n, summary)}</summary>
  <dl>
    {#each values as item}
      <div><dt>{resolveText(i18n, item.label)}</dt><dd class:technical={'kind' in item.value && item.value.kind === 'technical'}>{resolveText(i18n, item.value)}</dd></div>
    {/each}
  </dl>
</details>
