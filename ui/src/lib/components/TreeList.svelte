<script lang="ts">
  import type { I18n } from '../i18n';
  import { nextRovingIndex } from './keyboard';
  import { resolveText, type DisplayText, type LocalizedText, type UserText } from './text';
  let { i18n, label, items, selected = $bindable(), onselect }: {
    i18n: I18n;
    label: LocalizedText;
    /** `detail` is a secondary line; `note` is a state label such as «Editing not supported». */
    items: ReadonlyArray<{ id: string; label: UserText; level: number; expanded?: boolean; detail?: DisplayText; note?: LocalizedText }>;
    selected?: string;
    onselect?: (id: string) => void;
  } = $props();
  // The roving tab stop follows keyboard focus, then the selected item, then the first item.
  let focused = $state(-1);
  const tabStop = $derived.by(() => {
    if (focused >= 0 && focused < items.length) return focused;
    return Math.max(0, items.findIndex((item) => item.id === selected));
  });
  function focus(index: number): void {
    focused = index;
    document.getElementById(items[index]?.id ?? '')?.focus();
  }
</script>

<div role="tree" aria-label={resolveText(i18n, label)}>
  {#each items as item, index (item.id)}
    <button id={item.id} class="tree-row quiet" type="button" role="treeitem" aria-level={item.level} aria-expanded={item.expanded} aria-selected={selected === item.id} tabindex={tabStop === index ? 0 : -1} onclick={() => { focused = index; selected = item.id; onselect?.(item.id); }} onkeydown={(event) => { const next = nextRovingIndex(index, items.length, event.key, 'vertical'); if (next !== index) { event.preventDefault(); focus(next); } }}>
      <span class="tree-text">
        <span class="tree-label">{item.label.value}</span>
        {#if item.detail}<span class="tree-detail" class:technical={'kind' in item.detail && item.detail.kind === 'technical'}>{resolveText(i18n, item.detail)}</span>{/if}
      </span>
      {#if item.note}<span class="tree-note">{resolveText(i18n, item.note)}</span>{/if}
    </button>
  {/each}
</div>
