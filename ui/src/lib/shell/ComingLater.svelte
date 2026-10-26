<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from '../components';

  // A group of visible but unavailable actions with one visible reason (P1-01H §6).
  let { i18n, id, hint = { id: 'common.state.coming_later' }, children }: {
    i18n: I18n;
    id: string;
    hint?: LocalizedText;
    children: Snippet;
  } = $props();
</script>

<div class="coming-later" role="group" aria-describedby={`${id}-hint`}>
  <div class="actions">{@render children()}</div>
  <p id={`${id}-hint`} class="hint">{resolveText(i18n, hint)}</p>
</div>
