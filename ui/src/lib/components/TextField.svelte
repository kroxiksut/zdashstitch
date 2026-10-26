<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type DisplayText, type LocalizedText } from './text';

  let { i18n, id, label, value = $bindable(''), type = 'text', hint, error, required = false, readonly = false, disabled = false }: {
    i18n: I18n;
    id: string;
    label: LocalizedText;
    value?: string;
    type?: 'text' | 'password' | 'url';
    hint?: LocalizedText;
    error?: DisplayText;
    required?: boolean;
    readonly?: boolean;
    disabled?: boolean;
  } = $props();
  const hintId = $derived(`${id}-hint`);
  const errorId = $derived(`${id}-error`);
</script>

<div class="field">
  <label for={id}>{resolveText(i18n, label)}</label>
  <input {id} {type} bind:value {required} {readonly} {disabled} aria-invalid={error ? 'true' : undefined} aria-describedby={[hint ? hintId : '', error ? errorId : ''].filter(Boolean).join(' ') || undefined} />
  {#if hint}<span id={hintId} class="hint">{resolveText(i18n, hint)}</span>{/if}
  {#if error}<span id={errorId} class="field-error">{resolveText(i18n, error)}</span>{/if}
</div>
