import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';

import { createI18n } from '../i18n';
import { CSRF_HEADER } from '../session';
import AppearanceSection from './AppearanceSection.svelte';
import {
  APPEARANCE_ENDPOINT,
  AppearanceStore,
  DEFAULT_APPEARANCE,
  createHttpAppearanceClient,
  type AppearanceClient,
  type AppearanceEnvironment,
  type AppearancePreferences,
  type AppearanceResult
} from './appearance.svelte';

const en = createI18n('en');
const ru = createI18n('ru');

function media(matches = false) {
  const listeners = new Set<() => void>();
  return {
    matches,
    addEventListener: (_type: 'change', listener: () => void) => void listeners.add(listener),
    removeEventListener: (_type: 'change', listener: () => void) => void listeners.delete(listener),
    fire(next: boolean) {
      this.matches = next;
      for (const listener of listeners) listener();
    },
    listeners
  };
}

function environment(language = 'ru-RU') {
  const root: AppearanceEnvironment['root'] = { dataset: {}, style: { colorScheme: '' } };
  const prefersDark = media(false);
  const prefersReducedMotion = media(false);
  return { root, prefersDark, prefersReducedMotion, systemLanguage: language };
}

function memoryClient(initial: AppearanceResult = { status: 'ready', preferences: DEFAULT_APPEARANCE, file_state: 'missing' }) {
  const saved: AppearancePreferences[] = [];
  let next: AppearanceResult | null = null;
  const client: AppearanceClient = {
    load: () => Promise.resolve(initial),
    save: (preferences) => {
      saved.push(preferences);
      return Promise.resolve(next ?? { status: 'ready', preferences, file_state: 'valid' });
    }
  };
  return { client, saved, failNext: (result: AppearanceResult) => (next = result) };
}

describe('appearance store', () => {
  it('applies the stored preferences from the settings file on load', async () => {
    const env = environment();
    const { client } = memoryClient({
      status: 'ready',
      preferences: { locale: 'en', theme: 'high_contrast', reduced_motion: 'reduce' },
      file_state: 'valid'
    });
    const store = new AppearanceStore(client, env);
    expect(env.root.dataset.theme).toBe('system');
    await store.load();
    expect(env.root.dataset).toEqual({ theme: 'high_contrast', motion: 'reduce' });
    expect(env.root.style.colorScheme).toBe('light');
    expect(store.locale).toBe('en');
    expect(store.reducedMotion).toBe(true);
    expect(store.fileState).toBe('valid');
  });

  it('applies a change at once and saves all three values', async () => {
    const env = environment();
    const { client, saved } = memoryClient();
    const store = new AppearanceStore(client, env);
    await store.load();
    const pending = store.change({ theme: 'dark' });
    expect(env.root.dataset.theme).toBe('dark');
    expect(env.root.style.colorScheme).toBe('dark');
    await pending;
    expect(saved).toEqual([{ locale: 'system', theme: 'dark', reduced_motion: 'system' }]);
    expect(store.justSaved).toBe(true);
    expect(store.sessionOnly).toBeNull();

    await store.change({ locale: 'en' });
    expect(store.locale).toBe('en');
    await store.reset();
    expect(saved.at(-1)).toEqual(DEFAULT_APPEARANCE);
    expect(store.locale).toBe('ru');
    expect('motion' in env.root.dataset).toBe(false);
  });

  it('keeps a change until restart when the file is invalid or the save fails', async () => {
    const env = environment();
    const memory = memoryClient();
    const store = new AppearanceStore(memory.client, env);
    memory.failNext({ status: 'file_invalid' });
    await store.change({ theme: 'high_contrast' });
    expect(env.root.dataset.theme).toBe('high_contrast');
    expect(store.sessionOnly).toBe('file_invalid');
    expect(store.fileState).toBe('invalid');
    memory.failNext({ status: 'failed' });
    await store.change({ theme: 'light' });
    expect(env.root.dataset.theme).toBe('light');
    expect(store.sessionOnly).toBe('failed');
    expect(store.justSaved).toBe(false);
  });

  it('sends saves one after another so the file ends with the last choice', async () => {
    const order: string[] = [];
    let releaseFirst: () => void = () => {};
    const client: AppearanceClient = {
      load: () => Promise.resolve({ status: 'failed' }),
      save: (preferences) => {
        order.push(`start:${preferences.theme}`);
        const done = { status: 'ready', preferences, file_state: 'valid' } as const;
        if (preferences.theme === 'dark') {
          return new Promise((resolve) => {
            releaseFirst = () => {
              order.push('end:dark');
              resolve(done);
            };
          });
        }
        order.push(`end:${preferences.theme}`);
        return Promise.resolve(done);
      }
    };
    const store = new AppearanceStore(client, environment());
    const first = store.change({ theme: 'dark' });
    const second = store.change({ theme: 'light' });
    await Promise.resolve();
    expect(order).toEqual(['start:dark']);
    releaseFirst();
    await Promise.all([first, second]);
    expect(order).toEqual(['start:dark', 'end:dark', 'start:light', 'end:light']);
    expect(store.preferences.theme).toBe('light');
    expect(store.saving).toBe(false);
  });

  it('follows the system motion preference only in «system» mode', async () => {
    const env = environment();
    const store = new AppearanceStore(memoryClient().client, env);
    expect(store.reducedMotion).toBe(false);
    env.prefersReducedMotion.fire(true);
    expect(store.systemReducedMotion).toBe(true);
    expect(store.reducedMotion).toBe(true);
    await store.change({ reduced_motion: 'no-preference' });
    expect(store.reducedMotion).toBe(false);
    expect(env.root.dataset.motion).toBe('no-preference');
    store.destroy();
    expect(env.prefersReducedMotion.listeners.size).toBe(0);
  });
});

describe('appearance HTTP client', () => {
  it('reads and saves through the protected local route with the CSRF header', async () => {
    const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url === '/api/session') return Response.json({ csrf_token: 'csrf-1' });
      if (url === APPEARANCE_ENDPOINT && init?.method === 'GET') {
        return Response.json({ locale: 'ru', theme: 'high_contrast', reduced_motion: 'system', file_state: 'valid' });
      }
      if (url === APPEARANCE_ENDPOINT && init?.method === 'PUT') {
        const body = JSON.parse(String(init.body)) as Record<string, string>;
        return Response.json({ ...body, file_state: 'valid' });
      }
      return new Response(null, { status: 404 });
    });
    const client = createHttpAppearanceClient(fetcher as typeof fetch);
    expect(await client.load()).toEqual({
      status: 'ready',
      preferences: { locale: 'ru', theme: 'high_contrast', reduced_motion: 'system' },
      file_state: 'valid'
    });
    const saved = await client.save({ locale: 'en', theme: 'dark', reduced_motion: 'reduce' });
    expect(saved.status).toBe('ready');
    const put = fetcher.mock.calls.find(([, init]) => init?.method === 'PUT');
    expect((put?.[1]?.headers as Record<string, string>)[CSRF_HEADER]).toBe('csrf-1');
    expect(JSON.parse(String(put?.[1]?.body))).toEqual({ locale: 'en', theme: 'dark', reduced_motion: 'reduce' });
  });

  it('maps an invalid settings file, failures and unknown values to data-only results', async () => {
    const answers: Response[] = [
      Response.json({ csrf_token: 'csrf-1' }),
      Response.json({ code: 'settings_file_invalid', params: {} }, { status: 409 }),
      Response.json({ code: 'settings_write_failed', params: {} }, { status: 500 }),
      Response.json({ locale: 'en', theme: 'sepia', reduced_motion: 'system', file_state: 'valid' })
    ];
    const client = createHttpAppearanceClient((() => Promise.resolve(answers.shift() ?? new Response(null, { status: 500 }))) as typeof fetch);
    expect(await client.save(DEFAULT_APPEARANCE)).toEqual({ status: 'file_invalid' });
    expect(await client.save(DEFAULT_APPEARANCE)).toEqual({ status: 'failed' });
    expect(await client.load()).toEqual({ status: 'failed' });
    const offline = createHttpAppearanceClient((() => Promise.reject(new TypeError('offline'))) as typeof fetch);
    expect(await offline.load()).toEqual({ status: 'failed' });
  });
});

describe('«Appearance» section (mockup 06-01)', () => {
  it('shows language, four themes, motion, contrast and the format preview in Russian', () => {
    const store = new AppearanceStore(memoryClient().client, environment('ru-RU'));
    const { body } = render(AppearanceSection, { props: { i18n: ru, store } });
    for (const text of [
      '<h1 id="appearance-title">Внешний вид</h1>',
      'Изменения применяются сразу и сохраняются в файл настроек.',
      'Язык интерфейса',
      'сейчас: русский',
      'Светлая',
      'Тёмная',
      'Высокая контрастность',
      'Уменьшить анимацию',
      'Как в системе: выключено.',
      'Тема интерфейса не меняет оформление виджетов на панели.',
      '5 виджетов · 1 страница · 22 виджета',
      'Сбросить раздел'
    ]) {
      expect(body).toContain(text);
    }
    expect(body.match(/name="appearance-theme"/g)).toHaveLength(4);
    expect(body.match(/name="appearance-locale"/g)).toHaveLength(3);
    expect(body).toMatch(/1\s234\s567,8/u);
    expect(body).toContain('role="radiogroup"');
    expect(body).not.toContain('изменено');
  });

  it('marks a changed theme with text and labels endonyms with their language', async () => {
    const store = new AppearanceStore(memoryClient().client, environment('en-US'));
    await store.change({ theme: 'high_contrast' });
    const { body } = render(AppearanceSection, { props: { i18n: en, store } });
    expect(body).toContain('changed');
    expect(body).toContain('High contrast');
    expect(body).toContain('now: English');
    expect(body).toContain('lang="ru"');
    expect(body).toContain('5 widgets · 1 page · 22 widgets');
    expect(body).toMatch(/id="appearance-theme-high_contrast"[^>]*checked/);
  });

  it('shows why a change applies until restart', async () => {
    const memory = memoryClient();
    const store = new AppearanceStore(memory.client, environment());
    memory.failNext({ status: 'file_invalid' });
    await store.change({ theme: 'dark' });
    const { body } = render(AppearanceSection, { props: { i18n: ru, store } });
    expect(body).toContain('Файл настроек содержит ошибки и не применён');
    expect(body).toContain('изменения здесь действуют до перезапуска');
  });
});
