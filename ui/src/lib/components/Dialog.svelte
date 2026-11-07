<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, id, title, description, open = false, cancellable = true, children, actions, onclose }: {
    i18n: I18n;
    id: string;
    title: LocalizedText;
    description: LocalizedText;
    open?: boolean;
    cancellable?: boolean;
    children?: Snippet;
    actions?: Snippet;
    onclose?: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  const titleId = $derived(`${id}-title`);
  const descriptionId = $derived(`${id}-description`);

  function wrapTab(event: KeyboardEvent): void {
    if (event.key !== 'Tab' || !dialog.open) return;
    const controls = Array.from(dialog.querySelectorAll<HTMLElement>(
      'a[href], button, input, select, textarea, [tabindex]'
    )).filter((control) => control.tabIndex >= 0 && !control.matches(':disabled') &&
      !control.closest('[inert]') && control.getClientRects().length > 0 &&
      getComputedStyle(control).visibility === 'visible');
    const first = controls[0];
    const last = controls.at(-1);
    if (!first || !last || document.activeElement !== (event.shiftKey ? first : last)) return;
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
  }

  onMount(() => {
    if (open && !dialog.open) dialog.showModal();
    return () => {
      if (dialog.open) dialog.close();
    };
  });

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog bind:this={dialog} aria-labelledby={titleId} aria-describedby={descriptionId}
  onkeydown={wrapTab} oncancel={(event) => { if (!cancellable) event.preventDefault(); }} onclose={onclose}>
  <h2 id={titleId}>{resolveText(i18n, title)}</h2>
  <p id={descriptionId}>{resolveText(i18n, description)}</p>
  {#if children}{@render children()}{/if}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</dialog>
