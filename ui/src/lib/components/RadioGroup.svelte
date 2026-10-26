<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';
  let { i18n, name, legend, options, value = $bindable(), disabled = false }: {
    i18n: I18n;
    name: string;
    legend: LocalizedText;
    options: ReadonlyArray<{ value: string; label: LocalizedText; description?: LocalizedText }>;
    value?: string;
    disabled?: boolean;
  } = $props();
</script>

<fieldset {disabled}>
  <legend>{resolveText(i18n, legend)}</legend>
  {#each options as option (option.value)}
    <div class="choice">
      <input id={`${name}-${option.value}`} type="radio" {name} value={option.value} bind:group={value} aria-describedby={option.description ? `${name}-${option.value}-description` : undefined} />
      <label for={`${name}-${option.value}`}>{resolveText(i18n, option.label)}</label>
      {#if option.description}<span id={`${name}-${option.value}-description`} class="description">{resolveText(i18n, option.description)}</span>{/if}
    </div>
  {/each}
</fieldset>
