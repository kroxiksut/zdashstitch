<script lang="ts">
  import { tick } from 'svelte';
  import type { I18n } from '../i18n';
  import { Button, Dialog, TextField, type LocalizedText } from '../components';
  import { workcopyCodeText } from '../workcopy/messages';
  import type { DraftRenameResult, DraftSummary } from './drafts';

  // «Переименовать» (P1-03F): the local name of one draft. Focus starts in the field; an error
  // stays next to it and keeps the typed text. The server is never touched.
  let { i18n, draft, onrename, onclose }: {
    i18n: I18n;
    draft: DraftSummary;
    onrename: (localId: string, name: string) => Promise<DraftRenameResult>;
    onclose: (renamed: boolean) => void;
  } = $props();

  let name = $state('');
  let field = $state<HTMLInputElement>();
  let busy = $state(false);
  let error = $state<LocalizedText | undefined>(undefined);

  $effect(() => {
    name = draft.name;
    void tick().then(() => field?.select());
  });

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (busy) return;
    busy = true;
    error = undefined;
    const result = await onrename(draft.local_id, name);
    busy = false;
    if (result.status === 'ready') {
      onclose(true);
      return;
    }
    error =
      result.status === 'failed'
        ? workcopyCodeText(result.failure.code, result.failure.params)
        : { id: 'startup.error.unavailable' };
    await tick();
    field?.focus();
  }
</script>

<Dialog
  {i18n}
  id="draft-rename"
  open
  title={{ id: 'drafts.rename.title' }}
  description={{ id: 'drafts.rename.description' }}
  onclose={() => onclose(false)}
>
  <form class="rename-form" novalidate onsubmit={submit}>
    <TextField {i18n} id="draft-rename-name" label={{ id: 'drafts.column.name' }} bind:value={name} bind:element={field} autocomplete="off" required disabled={busy} {error} />
    <div class="actions">
      <Button {i18n} type="submit" variant="primary" label={{ id: 'common.action.rename' }} {busy} />
      <Button {i18n} label={{ id: 'common.action.cancel' }} onclick={() => onclose(false)} />
    </div>
  </form>
</Dialog>

<style>
  .rename-form {
    display: grid;
    gap: var(--space-3);
    margin-block-start: var(--space-3);
  }
</style>
