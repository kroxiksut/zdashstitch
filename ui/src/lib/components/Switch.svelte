<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, id, label, onLabel, offLabel, checked = $bindable(false), disabled = false }: {
    i18n: I18n;
    id: string;
    label: LocalizedText;
    onLabel: LocalizedText;
    offLabel: LocalizedText;
    checked?: boolean;
    disabled?: boolean;
  } = $props();
</script>

<div class="choice switch">
  <input {id} type="checkbox" role="switch" bind:checked {disabled} />
  <label for={id}>{resolveText(i18n, label)}</label>
  <span class="state">{resolveText(i18n, checked ? onLabel : offLabel)}</span>
</div>
