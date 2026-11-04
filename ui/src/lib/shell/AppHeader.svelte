<script lang="ts">
  import type { I18n } from '../i18n';
  import { Button, Icon, StatusBadge, resolveText, type LocalizedText } from '../components';
  import { MODE_LABEL, TOP_NAV, type ConnectionMode } from './labels';
  import { DEFAULT_SETTINGS_SECTION, settingsPath, type TopSection } from './router';

  // Connection mode label of the header (P1-01B §1, P1-02E): «Offline», «Read-only · {profile}»
  // or «Read-write · {profile}»; in read-write mode «Return to read-only» is one click away.
  let { i18n, current, mode, profileName = null, onsetreadonly }: {
    i18n: I18n;
    current: TopSection;
    mode: ConnectionMode;
    /** User data: the active connection profile. */
    profileName?: string | null;
    onsetreadonly?: () => void;
  } = $props();

  const label: LocalizedText = $derived.by(() => {
    if (profileName === null || mode === 'offline') return MODE_LABEL[mode];
    return mode === 'read_write'
      ? { id: 'connection.state.read_write_profile', params: { profile: profileName } }
      : { id: 'connection.state.read_only_profile', params: { profile: profileName } };
  });
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
    <StatusBadge {i18n} {label} tone={mode === 'read_write' ? 'warning' : 'info'} />
    {#if mode === 'read_write' && onsetreadonly}
      <Button {i18n} variant="quiet" label={{ id: 'connection.action.set_read_only' }} onclick={onsetreadonly} />
    {/if}
    <a class="settings-link" href={settingsPath(DEFAULT_SETTINGS_SECTION)} aria-current={current === 'settings' ? 'page' : undefined}><Icon name="settings" size={18} /><span>{i18n.t('nav.settings')}</span></a>
  </div>
</header>
