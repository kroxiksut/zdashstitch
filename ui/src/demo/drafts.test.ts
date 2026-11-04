import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import App from '../App.svelte';
import { createI18n } from '../lib/i18n';
import { widgetTypeLabel } from '../lib/shell/labels';
import { createDemoDraftsClient, DEMO_DRAFTS } from './drafts';

describe('DEMO build data (manual accessibility checks)', () => {
  it('marks every synthetic draft with the prefix the dist check looks for', () => {
    const ids = DEMO_DRAFTS.map((draft) => draft.local_id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(id).toMatch(/^demo-synthetic-\d{2}$/);
  });

  it('contains no addresses, URLs or IP-like values', () => {
    const text = JSON.stringify(DEMO_DRAFTS);
    expect(text).not.toMatch(/\b\d{1,3}(?:\.\d{1,3}){3}\b/);
    expect(text).not.toMatch(/[a-z]+:[/]{2}/i);
    expect(text).not.toMatch(/\.(?:com|net|org|ru|local|lan|corp|internal)\b/i);
  });

  it('covers the variants the manual check needs', () => {
    const kinds = new Set(DEMO_DRAFTS.map((draft) => `${draft.kind}/${draft.origin}`));
    expect(kinds).toEqual(new Set(['dashboard/server_copy', 'page/server_copy', 'dashboard/local', 'page/local']));
    const freshness = new Set(DEMO_DRAFTS.map((draft) => draft.freshness?.state).filter(Boolean));
    for (const state of ['conflict', 'stale', 'current', 'source_missing']) expect(freshness).toContain(state);
    expect(DEMO_DRAFTS.some((draft) => draft.name.length > 100)).toBe(true);
    expect(DEMO_DRAFTS.some((draft) => /[А-Яа-яЁё]/.test(draft.name))).toBe(true);
    expect(DEMO_DRAFTS.some((draft) => /^[\x20-\x7e]+$/.test(draft.name))).toBe(true);
    expect(DEMO_DRAFTS.some((draft) => draft.pages.length > 2)).toBe(true);
    const types = DEMO_DRAFTS.flatMap((draft) => draft.pages.flatMap((page) => page.widgets.map((widget) => widget.type)));
    expect(types.some((type) => widgetTypeLabel(type) === null)).toBe(true);
    expect(new Set(types.filter((type) => widgetTypeLabel(type) !== null)).size).toBeGreaterThan(5);
  });

  it('serves the drafts through the in-memory client', async () => {
    const client = createDemoDraftsClient();
    const list = await client.list();
    expect(list.status === 'ready' ? list.drafts.length : 0).toBe(DEMO_DRAFTS.length);
    expect((await client.load('demo-synthetic-01')).status).toBe('ready');
  });

  it('shows a localized «Demo data» marker only in demo mode', () => {
    for (const [locale, marker] of [['en', 'Demo data'], ['ru', 'Демонстрационные данные']] as const) {
      expect(createI18n(locale).t('shell.demo.marker')).toBe(marker);
      const demo = render(App, { props: { initialPath: '/drafts', locale, demo: true } }).body;
      expect(demo).toContain(`<p class="demo-marker" role="note">${marker}</p>`);
      const normal = render(App, { props: { initialPath: '/drafts', locale } }).body;
      expect(normal).not.toContain('demo-marker');
    }
  });
});
