<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let {
    i18n,
    label,
    variant = 'secondary',
    type = 'button',
    busy = false,
    disabled = false,
    leadingIcon,
    onclick
  }: {
    i18n: I18n;
    label: LocalizedText;
    variant?: 'primary' | 'secondary' | 'quiet' | 'danger';
    type?: 'button' | 'submit' | 'reset';
    busy?: boolean;
    disabled?: boolean;
    leadingIcon?: Snippet;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button
  {type}
  class:primary={variant === 'primary'}
  class:secondary={variant === 'secondary'}
  class:quiet={variant === 'quiet'}
  class:danger={variant === 'danger'}
  {disabled}
  aria-busy={busy}
  onclick={(event) => {
    if (!busy) onclick?.(event);
  }}
>
  {#if leadingIcon}
    <span class="icon" aria-hidden="true">{@render leadingIcon()}</span>
  {/if}
  <span>{resolveText(i18n, label)}</span>
  {#if busy}<span class="busy" aria-hidden="true"></span>{/if}
</button>
