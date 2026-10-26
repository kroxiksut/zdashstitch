<script lang="ts">
  import type { I18n } from '../i18n';
  import { StatusBadge, resolveText } from '../components';
  import { MODE_LABEL, TOP_NAV, type ConnectionMode } from './labels';
  import { DEFAULT_SETTINGS_SECTION, settingsPath, type TopSection } from './router';

  let { i18n, current, mode }: { i18n: I18n; current: TopSection; mode: ConnectionMode } = $props();
</script>

<header class="app-header">
  <a class="brand" href="/">{i18n.t('common.product_name')}</a>
  <nav class="top-nav" aria-label={i18n.t('shell.a11y.main_nav')}>
    <ul>
      {#each TOP_NAV as entry (entry.section)}
        <li><a href={entry.href} aria-current={current === entry.section ? 'page' : undefined}>{resolveText(i18n, entry.label)}</a></li>
      {/each}
    </ul>
  </nav>
  <div class="header-end">
    <StatusBadge {i18n} label={MODE_LABEL[mode]} tone="info" />
    <a class="settings-link" href={settingsPath(DEFAULT_SETTINGS_SECTION)} aria-current={current === 'settings' ? 'page' : undefined}>{i18n.t('nav.settings')}</a>
  </div>
</header>
