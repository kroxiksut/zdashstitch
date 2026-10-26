<script lang="ts">
  import type { I18n } from '../i18n';
  import { resolveText, type LocalizedText } from './text';
  let { i18n, label, href, external = false }: { i18n: I18n; label: LocalizedText; href: string; external?: boolean } = $props();
</script>

<a {href} rel={external ? 'noopener noreferrer' : undefined}>{resolveText(i18n, label)}</a>
