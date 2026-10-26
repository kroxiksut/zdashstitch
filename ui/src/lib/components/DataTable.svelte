<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type DisplayText, type LocalizedText } from './text';

  let { i18n, caption, headers, rows }: {
    i18n: I18n;
    caption: LocalizedText;
    headers: ReadonlyArray<LocalizedText>;
    /** `href` turns the first cell of the row into a link that opens the row object. */
    rows: ReadonlyArray<{ id: string; cells: ReadonlyArray<DisplayText>; href?: string }>;
  } = $props();
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="table-scroll" tabindex="0" role="region" aria-label={resolveText(i18n, caption)}>
  <table>
    <caption>{resolveText(i18n, caption)}</caption>
    <thead><tr>{#each headers as header}<th scope="col">{resolveText(i18n, header)}</th>{/each}</tr></thead>
    <tbody>
      {#each rows as row (row.id)}
        <tr>{#each row.cells as cell, index}<td class:technical={'kind' in cell && cell.kind === 'technical'}>{#if index === 0 && row.href}<a href={row.href}>{resolveText(i18n, cell)}</a>{:else}{resolveText(i18n, cell)}{/if}</td>{/each}</tr>
      {/each}
    </tbody>
  </table>
</div>
