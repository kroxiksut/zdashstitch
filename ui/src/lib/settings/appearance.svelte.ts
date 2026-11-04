// «Settings → Appearance» (P2-00G, P1-09C, O-038): language, theme and reduced motion. The settings
// file behind the local server is the only source of truth; the browser keeps nothing in storage.
// Every change applies at once (only the theme/motion attributes and the message catalog change:
// route, focus, selection and form data stay) and is then saved through the local server.

import { ApiFailure, apiFailure } from '../api';
import { detectLocale, type Locale } from '../i18n';
import { CSRF_HEADER, loadCsrfToken } from '../session';
import { applyMotion, createThemeController, type MotionPreference, type ThemePreference } from '../theme';

export const APPEARANCE_ENDPOINT = '/api/settings/appearance' as const;

export type LocalePreference = 'system' | Locale;
export const LOCALE_PREFERENCES: ReadonlyArray<LocalePreference> = ['system', 'en', 'ru'];

/** State of the settings file: `invalid` means the last valid values apply and the file is kept. */
export type SettingsFileState = 'valid' | 'missing' | 'invalid';

export interface AppearancePreferences {
  readonly locale: LocalePreference;
  readonly theme: ThemePreference;
  readonly reduced_motion: MotionPreference;
}

/** «Reset section» returns every value to «As in the system» (P1-09C). */
export const DEFAULT_APPEARANCE: AppearancePreferences = { locale: 'system', theme: 'system', reduced_motion: 'system' };

export type AppearanceResult =
  | { readonly status: 'ready'; readonly preferences: AppearancePreferences; readonly file_state: SettingsFileState }
  /** The settings file has errors; it is not overwritten. */
  | { readonly status: 'file_invalid' }
  /** No answer, or the server could not read or write the file. */
  | { readonly status: 'failed' };

export interface AppearanceClient {
  load(): Promise<AppearanceResult>;
  save(preferences: AppearancePreferences): Promise<AppearanceResult>;
}

const LOCALES = new Set<string>(LOCALE_PREFERENCES);
const THEMES = new Set<string>(['system', 'light', 'dark', 'high_contrast']);
const MOTIONS = new Set<string>(['system', 'reduce', 'no-preference']);
const FILE_STATES = new Set<string>(['valid', 'missing', 'invalid']);

interface WireAppearance {
  locale: string;
  theme: string;
  reduced_motion: string;
  file_state: string;
}

/** Values from the server outside the closed sets are a failed answer, not a guess. */
function fromWire(wire: WireAppearance): AppearanceResult {
  if (!LOCALES.has(wire.locale) || !THEMES.has(wire.theme) || !MOTIONS.has(wire.reduced_motion) || !FILE_STATES.has(wire.file_state)) {
    return { status: 'failed' };
  }
  return {
    status: 'ready',
    preferences: {
      locale: wire.locale as LocalePreference,
      theme: wire.theme as ThemePreference,
      reduced_motion: wire.reduced_motion as MotionPreference
    },
    file_state: wire.file_state as SettingsFileState
  };
}

/** Client over the local server. The CSRF token is read from `/api/session` and kept in memory only. */
export function createHttpAppearanceClient(fetcher: typeof fetch = (...args) => fetch(...args)): AppearanceClient {
  let csrf: Promise<string> | null = null;
  const ensureCsrf = (): Promise<string> => {
    csrf ??= loadCsrfToken(fetcher).catch((error: unknown) => {
      csrf = null;
      throw error;
    });
    return csrf;
  };

  async function send(init: RequestInit): Promise<AppearanceResult> {
    try {
      const response = await fetcher(APPEARANCE_ENDPOINT, { credentials: 'same-origin', ...init });
      if (!response.ok) throw await apiFailure(response);
      return fromWire((await response.json()) as WireAppearance);
    } catch (error) {
      if (error instanceof ApiFailure && error.response?.code === 'settings_file_invalid') return { status: 'file_invalid' };
      return { status: 'failed' };
    }
  }

  return {
    load: () => send({ method: 'GET', headers: { Accept: 'application/json' } }),
    async save(preferences) {
      let token: string;
      try {
        token = await ensureCsrf();
      } catch {
        return { status: 'failed' };
      }
      return send({
        method: 'PUT',
        headers: { Accept: 'application/json', 'Content-Type': 'application/json', [CSRF_HEADER]: token },
        body: JSON.stringify({ locale: preferences.locale, theme: preferences.theme, reduced_motion: preferences.reduced_motion })
      });
    }
  };
}

interface MediaQueryLike {
  readonly matches: boolean;
  addEventListener(type: 'change', listener: () => void): void;
  removeEventListener(type: 'change', listener: () => void): void;
}

/** The document the preferences apply to; absent in server-side rendering and unit tests. */
export interface AppearanceEnvironment {
  readonly root: { dataset: { theme?: string; motion?: string }; style: { colorScheme: string } };
  readonly prefersDark: MediaQueryLike;
  readonly prefersReducedMotion: MediaQueryLike;
  /** Locale of the browser or the system, for «As in the system». */
  readonly systemLanguage: string | undefined;
}

export function documentEnvironment(): AppearanceEnvironment | null {
  const root = globalThis.document?.documentElement;
  const matchMedia = globalThis.matchMedia;
  if (!root || typeof matchMedia !== 'function') return null;
  return {
    root,
    prefersDark: matchMedia('(prefers-color-scheme: dark)'),
    prefersReducedMotion: matchMedia('(prefers-reduced-motion: reduce)'),
    systemLanguage: globalThis.navigator?.language
  };
}

/**
 * Why the last change is not in the settings file: it applies until restart only.
 * `file_invalid` — the file has errors and is kept; `failed` — the server did not save it.
 */
export type SessionOnlyReason = 'file_invalid' | 'failed';

export class AppearanceStore {
  preferences = $state<AppearancePreferences>(DEFAULT_APPEARANCE);
  /** `null` until the first answer of the local server. */
  fileState = $state<SettingsFileState | null>(null);
  loaded = $state(false);
  saving = $state(false);
  sessionOnly = $state<SessionOnlyReason | null>(null);
  /** The last change reached the settings file; announced politely once. */
  justSaved = $state(false);
  /** The system asks for reduced motion (`prefers-reduced-motion: reduce`). */
  systemReducedMotion = $state(false);
  readonly systemLocale: Locale;

  /** Locale the interface uses now. */
  get locale(): Locale {
    return this.preferences.locale === 'system' ? this.systemLocale : this.preferences.locale;
  }

  /** The system choice of reduced motion, unless the settings choose explicitly. */
  get reducedMotion(): boolean {
    return this.preferences.reduced_motion === 'system' ? this.systemReducedMotion : this.preferences.reduced_motion === 'reduce';
  }

  #theme: ReturnType<typeof createThemeController> | null = null;
  #environment: AppearanceEnvironment | null;
  #onMotion = (): void => {
    if (this.#environment) this.systemReducedMotion = this.#environment.prefersReducedMotion.matches;
  };
  #request = 0;
  #pending: Promise<unknown> = Promise.resolve();

  constructor(
    readonly client: AppearanceClient,
    environment: AppearanceEnvironment | null = null
  ) {
    this.#environment = environment;
    this.systemLocale = detectLocale(environment?.systemLanguage ?? globalThis.navigator?.language);
    if (environment) {
      this.#theme = createThemeController(environment.root, environment.prefersDark, this.preferences.theme);
      applyMotion(environment.root, this.preferences.reduced_motion);
      environment.prefersReducedMotion.addEventListener('change', this.#onMotion);
      this.#onMotion();
    }
  }

  async load(): Promise<void> {
    const request = ++this.#request;
    const result = await this.client.load();
    if (request !== this.#request) return;
    this.loaded = true;
    if (result.status === 'ready') {
      this.fileState = result.file_state;
      this.#apply(result.preferences);
    }
  }

  /** Applies a change at once, then saves it; a refused or failed save keeps it until restart. */
  async change(patch: Partial<AppearancePreferences>): Promise<void> {
    const next: AppearancePreferences = { ...this.preferences, ...patch };
    this.#apply(next);
    const request = ++this.#request;
    this.saving = true;
    this.justSaved = false;
    // Saves go one after another, so the file always ends with the last choice.
    const save = this.#pending.then(() => this.client.save(next));
    this.#pending = save.catch(() => undefined);
    const result: AppearanceResult = await save.catch(() => ({ status: 'failed' }) as const);
    if (request !== this.#request) return;
    this.saving = false;
    this.loaded = true;
    if (result.status === 'ready') {
      this.fileState = result.file_state;
      this.sessionOnly = null;
      this.justSaved = true;
    } else {
      if (result.status === 'file_invalid') this.fileState = 'invalid';
      this.sessionOnly = result.status;
    }
  }

  reset(): Promise<void> {
    return this.change(DEFAULT_APPEARANCE);
  }

  destroy(): void {
    this.#theme?.destroy();
    this.#environment?.prefersReducedMotion.removeEventListener('change', this.#onMotion);
  }

  #apply(preferences: AppearancePreferences): void {
    this.preferences = preferences;
    this.#theme?.setPreference(preferences.theme);
    if (this.#environment) applyMotion(this.#environment.root, preferences.reduced_motion);
  }
}

/** A client without a local server: nothing is saved, every change applies until restart. */
export const offlineAppearanceClient: AppearanceClient = {
  load: () => Promise.resolve({ status: 'failed' }),
  save: () => Promise.resolve({ status: 'failed' })
};
