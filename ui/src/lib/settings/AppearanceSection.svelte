<script lang="ts">
  import { Banner, Button, resolveText, type LocalizedText } from '../components';
  import { DATE_TIME_FORMAT, type I18n } from '../i18n';
  import { THEME_PREFERENCES, type ThemePreference } from '../theme';
  import { LOCALE_PREFERENCES, type AppearanceStore, type LocalePreference } from './appearance.svelte';

  // «Settings → Appearance» (P2-00G, P1-09C, accepted mockup 06-01; fourth theme — O-038).
  // Language and theme are radio groups (arrow keys move and choose); every change applies at once
  // and is saved to the settings file through the local server.
  let { i18n, store }: { i18n: I18n; store: AppearanceStore } = $props();

  const LOCALE_LABEL: Readonly<Record<LocalePreference, LocalizedText>> = {
    system: { id: 'settings.language.system' },
    en: { id: 'settings.language.option.en' },
    ru: { id: 'settings.language.option.ru' }
  };
  const THEME_LABEL: Readonly<Record<ThemePreference, LocalizedText>> = {
    system: { id: 'settings.theme.system' },
    light: { id: 'settings.theme.light' },
    dark: { id: 'settings.theme.dark' },
    high_contrast: { id: 'settings.theme.high_contrast' }
  };

  // Fixed sample values of the format preview (mockup 06-01).
  const SAMPLE_TIME = new Date(2026, 9, 25, 14, 30);
  const SAMPLE_NUMBER = 1234567.8;

  const systemLanguage = $derived(
    i18n.t(store.systemLocale === 'ru' ? 'settings.language.name.ru' : 'settings.language.name.en')
  );
  const systemMotionState = $derived(i18n.t(store.systemReducedMotion ? 'settings.motion.state.on' : 'settings.motion.state.off'));
</script>

<section class="appearance" aria-labelledby="appearance-title" aria-busy={!store.loaded}>
  <header class="screen-head with-actions">
    <div class="screen-head">
      <h1 id="appearance-title">{i18n.t('settings.appearance.title')}</h1>
      <p class="hint">{i18n.t('settings.appearance.subtitle')}</p>
    </div>
    <input
      class="settings-search"
      type="search"
      aria-label={i18n.t('settings.search.placeholder')}
      placeholder={i18n.t('settings.search.placeholder')}
      disabled
    />
  </header>

  {#if store.sessionOnly === 'file_invalid' || store.fileState === 'invalid'}
    <Banner {i18n} tone="warning" heading={{ id: 'settings.file.invalid.title' }} body={{ id: 'settings.appearance.invalid_body' }} />
  {:else if store.sessionOnly === 'failed'}
    <Banner {i18n} tone="warning" heading={{ id: 'settings.appearance.not_saved.title' }} body={{ id: 'settings.appearance.not_saved.body' }} />
  {/if}

  <div class="appearance-grid">
    <section class="card appearance-group" aria-labelledby="appearance-locale-title">
      <h2 id="appearance-locale-title" class="eyebrow">{i18n.t('settings.language.title')}</h2>
      <div class="choices" role="radiogroup" aria-labelledby="appearance-locale-title">
        {#each LOCALE_PREFERENCES as option (option)}
          <div class="choice">
            <input
              id={`appearance-locale-${option}`}
              type="radio"
              name="appearance-locale"
              value={option}
              checked={store.preferences.locale === option}
              aria-describedby={option === 'system' ? 'appearance-locale-system-description' : undefined}
              onchange={() => void store.change({ locale: option })}
            />
            <label for={`appearance-locale-${option}`} lang={option === 'system' ? undefined : option}>{resolveText(i18n, LOCALE_LABEL[option])}</label>
            {#if option === 'system'}
              <span id="appearance-locale-system-description" class="description">
                {i18n.t('settings.language.current', { language: systemLanguage })}
              </span>
            {/if}
          </div>
        {/each}
      </div>
      <p class="hint">{i18n.t('settings.language.after_restart')}</p>
      <p class="hint">{i18n.t('settings.data_not_translated')}</p>
    </section>

    <section class="card appearance-group" aria-labelledby="appearance-theme-title">
      <h2 class="eyebrow group-head">
        <span id="appearance-theme-title">{i18n.t('settings.theme.title')}</span>
        {#if store.preferences.theme !== 'system'}<span class="changed-badge">{i18n.t('settings.changed')}</span>{/if}
      </h2>
      <div class="theme-options" role="radiogroup" aria-labelledby="appearance-theme-title">
        {#each THEME_PREFERENCES as option (option)}
          <div class="theme-option" class:selected={store.preferences.theme === option}>
            <label class="theme-preview {option}" for={`appearance-theme-${option}`} aria-hidden="true">
              {#if option === 'system'}
                <span class="half light"><span class="panel"></span></span>
                <span class="half dark"><span class="panel"></span></span>
              {:else}
                <span class="panel"></span><span class="panel"></span>
              {/if}
            </label>
            <div class="choice">
              <input
                id={`appearance-theme-${option}`}
                type="radio"
                name="appearance-theme"
                value={option}
                checked={store.preferences.theme === option}
                aria-describedby={option === 'high_contrast' ? 'appearance-theme-high-contrast-description' : undefined}
                onchange={() => void store.change({ theme: option })}
              />
              <label for={`appearance-theme-${option}`}>{resolveText(i18n, THEME_LABEL[option])}</label>
            </div>
          </div>
        {/each}
      </div>
      <p id="appearance-theme-high-contrast-description" class="hint">{i18n.t('settings.theme.high_contrast.hint')}</p>
      <p class="hint">{i18n.t('settings.theme.not_widgets')}</p>
    </section>

    <section class="card appearance-group" aria-labelledby="appearance-motion-title">
      <h2 id="appearance-motion-title" class="eyebrow">{i18n.t('settings.motion.title')}</h2>
      <div class="choice">
        <input
          id="appearance-reduce-motion"
          type="checkbox"
          checked={store.reducedMotion}
          aria-describedby="appearance-reduce-motion-description"
          onchange={(event) => void store.change({ reduced_motion: event.currentTarget.checked ? 'reduce' : 'no-preference' })}
        />
        <label for="appearance-reduce-motion">{i18n.t('settings.motion.reduce')}</label>
        <span id="appearance-reduce-motion-description" class="description">
          {store.preferences.reduced_motion === 'system'
            ? i18n.t('settings.motion.system_state', { state: systemMotionState })
            : i18n.t('settings.motion.explicit_state', { state: systemMotionState })}
          {i18n.t('settings.motion.no_meaning')}
        </span>
      </div>
      <h3>{i18n.t('settings.contrast.title')}</h3>
      <p class="hint">{i18n.t('settings.contrast.note')}</p>
    </section>

    <section class="card appearance-group" aria-labelledby="appearance-formats-title">
      <h2 id="appearance-formats-title" class="eyebrow">{i18n.t('settings.formats.title')}</h2>
      <dl class="facts">
        <dt>{i18n.t('settings.formats.datetime')}</dt>
        <dd>{i18n.date(SAMPLE_TIME, DATE_TIME_FORMAT)}</dd>
        <dt>{i18n.t('settings.formats.number')}</dt>
        <dd>{i18n.number(SAMPLE_NUMBER)}</dd>
        <dt>{i18n.t('settings.formats.count')}</dt>
        <dd>{i18n.t('settings.formats.count_sample', { widgets: 5, pages: 1, more: 22 })}</dd>
      </dl>
    </section>

    <section class="card appearance-group appearance-file" aria-labelledby="appearance-file-title">
      <div>
        <h2 id="appearance-file-title" class="eyebrow">{i18n.t('settings.file.title')}</h2>
        <p class="hint">{i18n.t('settings.file.hint')}</p>
      </div>
      <Button {i18n} label={{ id: 'settings.reset.section' }} busy={store.saving} onclick={() => void store.reset()} />
    </section>
  </div>

  <p class="visually-hidden" role="status" aria-live="polite">
    {#if store.justSaved}{i18n.t('settings.appearance.saved')}{/if}
  </p>
</section>
