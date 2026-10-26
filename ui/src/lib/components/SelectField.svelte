<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, id, label, options, value = $bindable(), hint, disabled = false }: {
    i18n: I18n;
    id: string;
    label: LocalizedText;
    options: ReadonlyArray<{ value: string; label: LocalizedText }>;
    value?: string;
    hint?: LocalizedText;
    disabled?: boolean;
  } = $props();
</script>

<div class="field">
  <label for={id}>{resolveText(i18n, label)}</label>
  <select {id} bind:value {disabled} aria-describedby={hint ? `${id}-hint` : undefined}>
    {#each options as option (option.value)}
      <option value={option.value}>{resolveText(i18n, option.label)}</option>
    {/each}
  </select>
  {#if hint}<span id={`${id}-hint`} class="hint">{resolveText(i18n, hint)}</span>{/if}
</div>
