<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, id, label, description, checked = $bindable(false), disabled = false }: {
    i18n: I18n;
    id: string;
    label: LocalizedText;
    description?: LocalizedText;
    checked?: boolean;
    disabled?: boolean;
  } = $props();
</script>

<div class="choice">
  <input {id} type="checkbox" bind:checked {disabled} aria-describedby={description ? `${id}-description` : undefined} />
  <label for={id}>{resolveText(i18n, label)}</label>
  {#if description}<span id={`${id}-description`} class="description">{resolveText(i18n, description)}</span>{/if}
</div>
