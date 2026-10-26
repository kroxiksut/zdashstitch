<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';
  let { i18n, message, dismissLabel, ondismiss }: { i18n: I18n; message: LocalizedText; dismissLabel: LocalizedText; ondismiss?: () => void } = $props();
</script>

<div class="toast" role="status" aria-live="polite">
  <span>{resolveText(i18n, message)}</span>
  <button class="quiet" type="button" onclick={ondismiss}>{resolveText(i18n, dismissLabel)}</button>
</div>
