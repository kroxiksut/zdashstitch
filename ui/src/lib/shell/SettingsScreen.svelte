<script lang="ts">
  import type { I18n } from '../i18n';
  import AboutScreen from '../about/AboutScreen.svelte';
  import { EmptyState, resolveText } from '../components';
  import AppearanceSection from '../settings/AppearanceSection.svelte';
  import { AppearanceStore, offlineAppearanceClient } from '../settings/appearance.svelte';
  import { SETTINGS_ENTRIES } from './labels';
  import { settingsPath, type SettingsSection } from './router';

  // «Settings» navigation (P1-09, mockup 06-01). «Appearance» works (P2-00G); other section
  // screens arrive with P4-06;
  // G1b sections are visible but unavailable (P1-01H §6).
  // «About and licenses» (P1-12) is the last navigation entry; it opens `/settings/about`.
  // TODO(O-032): End session — goes to «Security and export» (P1-12); the command is
  // `endLocalSession` in `../session` (P2-SEC-01E), the screen is not built yet.
  // «Appearance» (P2-00G) works over the shared store of the application; without one (unit tests)
  // it shows the defaults and saves nothing.
  let { i18n, section, appearance }: { i18n: I18n; section: SettingsSection; appearance?: AppearanceStore } = $props();
  const appearanceStore = $derived(appearance ?? new AppearanceStore(offlineAppearanceClient));

  const current = $derived(SETTINGS_ENTRIES.find((entry) => entry.section === section) ?? SETTINGS_ENTRIES[0]);
</script>

<div class="workspace settings">
  <nav class="side-nav" aria-labelledby="settings-nav-title">
    <p id="settings-nav-title" class="eyebrow">{i18n.t('settings.title')}</p>
    <ul>
      {#each SETTINGS_ENTRIES as entry (entry.section)}
        <li>
          {#if entry.available}
            <a href={settingsPath(entry.section)} aria-current={entry.section === section ? 'page' : undefined}>{resolveText(i18n, entry.label)}</a>
          {:else}
            <span class="nav-unavailable">
              <span>{resolveText(i18n, entry.label)}</span>
              <span class="hint">{i18n.t('common.state.coming_later')}</span>
            </span>
          {/if}
        </li>
      {/each}
    </ul>
    <p class="hint">{i18n.t('settings.nav.no_secrets')}</p>
  </nav>

  <main id="main-content" tabindex="-1">
    {#if section === 'about'}
      <AboutScreen {i18n} />
    {:else if section === 'appearance'}
      <AppearanceSection {i18n} store={appearanceStore} />
    {:else}
      <header class="screen-head">
        <h1>{i18n.t('settings.title')}</h1>
        <input
          class="settings-search"
          type="search"
          aria-label={i18n.t('settings.search.placeholder')}
          placeholder={i18n.t('settings.search.placeholder')}
          disabled
        />
      </header>
      <EmptyState {i18n} heading={current.label} reason={{ id: 'common.state.coming_later' }} />
    {/if}
  </main>
</div>
