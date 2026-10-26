<script lang="ts">
  import type { I18n } from '../i18n';
  import { nextRovingIndex } from './keyboard';
  import { resolveText, type LocalizedText, type UserText } from './text';
  let { i18n, id, label, options, value = $bindable(''), expanded = $bindable(false), onselect }: {
    i18n: I18n;
    id: string;
    label: LocalizedText;
    options: ReadonlyArray<{ id: string; label: UserText }>;
    value?: string;
    expanded?: boolean;
    onselect?: (id: string) => void;
  } = $props();
  let active = $state(0);

  function choose(index: number): void {
    const option = options[index];
    if (!option) return;
    value = option.label.value;
    expanded = false;
    onselect?.(option.id);
  }

  function keydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') { expanded = false; return; }
    if (event.key === 'Enter' && expanded) { event.preventDefault(); choose(active); return; }
    const next = nextRovingIndex(active, options.length, event.key, 'vertical');
    if (next !== active) { event.preventDefault(); active = next; expanded = true; }
  }
</script>

<div class="field combobox">
  <label for={id}>{resolveText(i18n, label)}</label>
  <input {id} role="combobox" autocomplete="off" aria-autocomplete="list" aria-expanded={expanded} aria-controls={`${id}-listbox`} aria-activedescendant={expanded ? options[active]?.id : undefined} bind:value onfocus={() => { expanded = true; }} onkeydown={keydown} />
  {#if expanded}
    <ul id={`${id}-listbox`} role="listbox">
      {#each options as option, index (option.id)}
        <!-- Focus remains on the combobox input per the APG pattern. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li id={option.id} role="option" aria-selected={index === active} onclick={() => choose(index)}>{option.label.value}</li>
      {/each}
    </ul>
  {/if}
</div>
