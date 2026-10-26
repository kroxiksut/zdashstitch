<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';

  let { i18n, id, label, accept, multiple = false, onchange }: {
    i18n: I18n;
    id: string;
    label: LocalizedText;
    accept?: string;
    multiple?: boolean;
    onchange?: (files: FileList) => void;
  } = $props();
</script>

<div class="drop-zone">
  <label for={id}>{resolveText(i18n, label)}</label>
  <input {id} type="file" {accept} {multiple} onchange={(event) => { const files = event.currentTarget.files; if (files) onchange?.(files); }} />
</div>
