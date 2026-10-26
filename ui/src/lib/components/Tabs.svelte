<script lang="ts">
  import type { I18n } from '../i18n';
  import { nextRovingIndex } from './keyboard';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, label, tabs, selected = $bindable(), onselect }: {
    i18n: I18n;
    label: LocalizedText;
    tabs: ReadonlyArray<{ id: string; label: LocalizedText; panelId: string }>;
    selected?: string;
    onselect?: (id: string) => void;
  } = $props();

  function select(index: number): void {
    const tab = tabs[index];
    if (!tab) return;
    selected = tab.id;
    onselect?.(tab.id);
    document.getElementById(tab.id)?.focus();
  }

  function keydown(event: KeyboardEvent, index: number): void {
    const next = nextRovingIndex(index, tabs.length, event.key, 'horizontal');
    if (next !== index) {
      event.preventDefault();
      select(next);
    }
  }
</script>

<div role="tablist" aria-label={resolveText(i18n, label)}>
  {#each tabs as tab, index (tab.id)}
    <button id={tab.id} type="button" role="tab" aria-selected={selected === tab.id} aria-controls={tab.panelId} tabindex={selected === tab.id ? 0 : -1} onclick={() => select(index)} onkeydown={(event) => keydown(event, index)}>
      {resolveText(i18n, tab.label)}
    </button>
  {/each}
</div>
