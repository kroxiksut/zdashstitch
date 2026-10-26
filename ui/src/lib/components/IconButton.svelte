<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, label, icon, disabled = false, pressed, onclick }: {
    i18n: I18n;
    label: LocalizedText;
    icon: Snippet;
    disabled?: boolean;
    pressed?: boolean;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button class="icon-button quiet" type="button" aria-label={resolveText(i18n, label)} aria-pressed={pressed} {disabled} {onclick}>
  <span aria-hidden="true">{@render icon()}</span>
</button>
