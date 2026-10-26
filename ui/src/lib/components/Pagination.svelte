<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, label, previousLabel, nextLabel, page, pages, onchange }: {
    i18n: I18n;
    label: LocalizedText;
    previousLabel: LocalizedText;
    nextLabel: LocalizedText;
    page: number;
    pages: number;
    onchange?: (page: number) => void;
  } = $props();
</script>

<nav class="pagination" aria-label={resolveText(i18n, label)}>
  <button type="button" disabled={page <= 1} onclick={() => onchange?.(page - 1)}>{resolveText(i18n, previousLabel)}</button>
  <output aria-live="polite">{i18n.number(page)} / {i18n.number(pages)}</output>
  <button type="button" disabled={page >= pages} onclick={() => onchange?.(page + 1)}>{resolveText(i18n, nextLabel)}</button>
</nav>
