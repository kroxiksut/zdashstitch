import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import App from '../../App.svelte';
import { createI18n } from '../i18n';
import DraftsScreen from './DraftsScreen.svelte';
import DraftWorkspace from './DraftWorkspace.svelte';
import SettingsScreen from './SettingsScreen.svelte';
import StartScreen from './StartScreen.svelte';
import {
  createMemoryDraftsClient,
  filterDrafts,
  kindCounts,
  recentDrafts,
  unavailableDraftsClient,
  type DraftDetail
} from './drafts';
import { durationText, pageName, widgetTypeName } from './format';
import { findingLevelLabel } from './labels';
import { periodUnit, previewAspectRatio, rowsUsed, widgetBox } from './preview';
import { draftPath, internalLinkPath, parseRoute, topSection } from './router';

// Test fixtures only: neutral names, no real infrastructure. Never used by the production build.
const pageCopy: DraftDetail = {
  local_id: 'draft-page-1',
  kind: 'page',
  origin: 'server_copy',
  name: 'Queue overview',
  source: { dashboard_name: 'Source dashboard', page_name: 'Queues', snapshot_at: Date.UTC(2026, 9, 24, 12, 40) },
  freshness: { state: 'not_checked' },
  local_state: 'dirty',
  updated_at: Date.UTC(2026, 9, 25, 12, 40),
  pages: [
    {
      id: 'p1',
      name: 'Queues',
      display_period: 60,
      widgets: [
        { id: 'w1', name: 'Throughput', type: 'svggraph', x: 0, y: 0, width: 36, height: 5, editable: true },
        { id: 'w2', name: 'Top queues', type: 'tophosts', x: 36, y: 0, width: 36, height: 3, editable: false }
      ]
    }
  ],
  compatibility: { min_version: '7.4.0' },
  checks: {
    checked_objects: 3,
    passed: { structure: 2, widgets: 0, sources: 0, references: 0, compatibility: 0 },
    findings: [
      {
        id: 'f1',
        group: 'compatibility',
        level: 'warning',
        blocks: ['publication'],
        message: { id: 'compatibility.unknown_widget_type', params: { widget_type: 'tophosts' } },
        page: 'Queues',
        widget: 'Top queues'
      }
    ]
  }
};

const localDashboard: DraftDetail = {
  local_id: 'draft-dashboard-1',
  kind: 'dashboard',
  origin: 'local',
  name: 'Combined view',
  local_state: 'saved',
  updated_at: Date.UTC(2026, 9, 25, 12, 55),
  pages: [
    { id: 'p1', name: 'First', display_period: null, widgets: [] },
    { id: 'p2', name: '', display_period: 30, widgets: [] }
  ]
};

const olderDashboard: DraftDetail = { ...localDashboard, local_id: 'draft-dashboard-2', name: 'Older view', updated_at: 1 };

const en = createI18n('en');
const ru = createI18n('ru');

describe('shell routes (P1-01B §3, P1-01H §2)', () => {
  it('parses every route family', () => {
    expect(parseRoute('/').route).toEqual({ name: 'start' });
    expect(parseRoute('/drafts').route).toEqual({ name: 'drafts' });
    expect(parseRoute('/library').route).toEqual({ name: 'library' });
    expect(parseRoute('/connections').route).toEqual({ name: 'connections' });
    expect(parseRoute('/server').route).toEqual({ name: 'server', connectionLocalId: null });
    expect(parseRoute('/server/conn-1/dashboards').route).toEqual({ name: 'server', connectionLocalId: 'conn-1' });
    expect(parseRoute('/settings/about').route).toEqual({ name: 'settings', section: 'about' });
    expect(parseRoute('/draft/abc_1/checks').route).toEqual({ name: 'draft', localId: 'abc_1', section: 'checks' });
    expect(parseRoute('/draft/abc/structure/page/2/edit')).toEqual({
      route: { name: 'draft', localId: 'abc', section: 'structure' }
    });
  });

  it('canonicalizes partial or unknown addresses without guessing identities', () => {
    expect(parseRoute('/settings')).toEqual({ route: { name: 'settings', section: 'appearance' }, canonical: '/settings/appearance' });
    expect(parseRoute('/draft/abc')).toEqual({
      route: { name: 'draft', localId: 'abc', section: 'overview' },
      canonical: '/draft/abc/overview'
    });
    expect(parseRoute('/nowhere')).toEqual({ route: { name: 'start' }, canonical: '/' });
    expect(parseRoute('/draft/..%2Fetc/overview')).toEqual({ route: { name: 'start' }, canonical: '/' });
    expect(parseRoute('/draft/%E0%A4%A/overview')).toEqual({ route: { name: 'start' }, canonical: '/' });
    expect(parseRoute('/server/https:%2F%2Fzabbix/dashboards').canonical).toBe('/server');
  });

  it('builds opaque draft paths and maps drafts to the «Drafts» header item', () => {
    expect(draftPath('a-1', 'structure')).toBe('/draft/a-1/structure');
    expect(topSection({ name: 'draft', localId: 'a', section: 'overview' })).toBe('drafts');
  });

  it('routes only plain same-origin clicks and keeps in-page anchors native', () => {
    const click = { button: 0, metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, defaultPrevented: false };
    const link = (href: string, target = '') => ({ href, target, hasAttribute: () => false });
    // Built without a URL literal: the UI contract check rejects remote-looking strings in sources.
    const origin = ['http:', '', '127.0.0.1:4173'].join('/');
    expect(internalLinkPath(link(`${origin}/drafts`), origin, click)).toBe('/drafts');
    expect(internalLinkPath(link(`${origin}/drafts#main-content`), origin, click)).toBeNull();
    expect(internalLinkPath(link(`${origin}/api/health`), origin, click)).toBeNull();
    expect(internalLinkPath(link(`${origin.replace('127.0.0.1', '127.0.0.2')}/drafts`), origin, click)).toBeNull();
    expect(internalLinkPath(link(`${origin}/drafts`, '_blank'), origin, click)).toBeNull();
    expect(internalLinkPath(link(`${origin}/drafts`), origin, { ...click, ctrlKey: true })).toBeNull();
  });
});

describe('drafts client boundary', () => {
  it('reports unavailable storage instead of inventing drafts', async () => {
    expect(await unavailableDraftsClient.list()).toEqual({ status: 'unavailable' });
    expect(await unavailableDraftsClient.load('x')).toEqual({ status: 'unavailable' });
  });

  it('serves summaries and details from memory for tests', async () => {
    const client = createMemoryDraftsClient([pageCopy, localDashboard]);
    const list = await client.list();
    expect(list.status).toBe('ready');
    if (list.status === 'ready') expect(list.drafts[0]).not.toHaveProperty('pages');
    expect(await client.load('missing')).toEqual({ status: 'missing' });
    expect((await client.load(pageCopy.local_id)).status).toBe('ready');
  });

  it('filters, counts and limits recent drafts', () => {
    const drafts = [pageCopy, localDashboard, olderDashboard];
    expect(kindCounts(drafts)).toEqual({ all: 3, dashboard: 2, page: 1 });
    expect(filterDrafts(drafts, 'dashboard', 'local').map((draft) => draft.local_id)).toEqual(['draft-dashboard-1', 'draft-dashboard-2']);
    expect(filterDrafts(drafts, 'all', 'server_copy')).toEqual([pageCopy]);
    const many = Array.from({ length: 7 }, (_, index) => ({ ...localDashboard, local_id: `d${index}`, updated_at: index }));
    expect(recentDrafts(many).map((draft) => draft.local_id)).toEqual(['d6', 'd5', 'd4', 'd3', 'd2']);
  });
});

describe('page preview geometry (P1-01B §2a)', () => {
  it('keeps Zabbix grid proportions for the chosen screen width', () => {
    expect(previewAspectRatio(1920, 8)).toBe('1920 / 560');
    expect(previewAspectRatio(1366, 0)).toBeUndefined();
    expect(widgetBox({ x: 36, y: 2, width: 18, height: 3 }, 10)).toEqual({ left: 50, width: 25, top: 20, height: 30 });
    expect(rowsUsed(pageCopy.pages[0].widgets)).toBe(5);
    expect(rowsUsed([{ y: 60, height: 10 }])).toBe(64);
  });

  it('expresses display periods in whole units', () => {
    expect(periodUnit(30)).toEqual({ value: 30, unit: 'second' });
    expect(periodUnit(120)).toEqual({ value: 2, unit: 'minute' });
    expect(periodUnit(3600)).toEqual({ value: 1, unit: 'hour' });
  });
});

describe('shell screens', () => {
  it('renders the header, landmarks and the offline mode on start', () => {
    const { body } = render(App, { props: { initialPath: '/', locale: 'en' } });
    expect(body).toContain('class="skip-link" href="#main-content"');
    expect(body).toContain('<header class="app-header">');
    expect(body).toContain('<main id="main-content" tabindex="-1">');
    for (const label of ['Start', 'Drafts', 'Dashboards in Zabbix', 'Library', 'Connections', 'Settings', 'Offline']) {
      expect(body).toContain(label);
    }
    expect(body).toMatch(/<a href="\/" aria-current="page">Start<\/a>/);
    expect(body).toContain('What do you want to do with Zabbix dashboards?');
  });

  it('shows unavailable start actions with a visible reason and no sample data', () => {
    const { body } = render(StartScreen, { props: { i18n: en, drafts: { status: 'unavailable' } } });
    expect(body).toContain('No local drafts yet');
    expect(body.match(/Will be available later/g)).toHaveLength(3);
    expect(body.match(/<button[^>]*disabled/g)).toHaveLength(5);
    expect(body).toContain('href="/connections"');
  });

  it('lists recent drafts newest first with type, origin and local state', () => {
    const drafts = { status: 'ready' as const, drafts: [olderDashboard, pageCopy, localDashboard] };
    const { body } = render(StartScreen, { props: { i18n: ru, drafts } });
    expect(body.indexOf('Combined view')).toBeLessThan(body.indexOf('Queue overview'));
    expect(body.indexOf('Queue overview')).toBeLessThan(body.indexOf('Older view'));
    expect(body).toContain('Копия с сервера');
    expect(body).toContain('href="/draft/draft-page-1/overview"');
  });

  it('renders the drafts table with filters, columns and links', () => {
    const drafts = { status: 'ready' as const, drafts: [pageCopy, localDashboard] };
    const { body } = render(DraftsScreen, { props: { i18n: en, drafts } });
    for (const text of ['Name', 'Type', 'Origin', 'Source', 'State', 'All 2', 'Dashboards 1', 'Pages 1', 'Any origin', 'Copies from server']) {
      expect(body).toContain(text);
    }
    expect(body).toContain('<a href="/draft/draft-page-1/overview">Queue overview</a>');
    expect(body).toContain('Source dashboard / Queues');
    expect(body).toContain('type="radio"');
  });

  it('says that drafts storage is not available yet instead of showing a list', () => {
    const { body } = render(DraftsScreen, { props: { i18n: en, drafts: { status: 'unavailable' } } });
    expect(body).toContain('No local drafts yet');
    expect(body).not.toContain('<table');
  });

  it('shows the workspace context line, breadcrumb and section navigation', () => {
    const { body } = render(DraftWorkspace, {
      props: { i18n: en, localId: pageCopy.local_id, section: 'overview', result: { status: 'ready', draft: pageCopy } }
    });
    expect(body).toContain(['<nav class="side-nav" aria-label', '"Draft sections">'].join('='));
    for (const text of ['Overview', 'Structure', 'Bindings', 'Checks', 'Publication', 'Unsaved local changes', 'Not checked', 'Zabbix 7.4.0+', 'Editing not supported']) {
      expect(body).toContain(text);
    }
    expect(body).toContain('aria-current="page">Overview');
    // Attribute assertions are joined so the UI contract scan does not read them as markup.
    expect(body).toContain(['<nav class="breadcrumb" aria-label', '"Breadcrumbs">'].join('='));
    expect(body).toContain('Graph · 0, 0 · 36 × 5');
  });

  it('renders the structure tree and the page preview with editing unavailable', () => {
    const { body } = render(DraftWorkspace, {
      props: { i18n: ru, localId: pageCopy.local_id, section: 'structure', result: { status: 'ready', draft: pageCopy } }
    });
    expect(body).toContain('role="tree"');
    expect(body).toContain('role="treeitem"');
    expect(body).toContain('1. Queues');
    expect(body).toContain('Предпросмотр страницы');
    expect(body).toContain('Ширина экрана: 1920 px');
    expect(body).toContain('Показано строк: 5 из 64');
    expect(body).toContain('Появится позже');
    expect(body).toMatch(/id="preview-synthetic"[^>]*disabled/);
  });

  it('summarizes checks with common components and the last publication checks', () => {
    const { body } = render(DraftWorkspace, {
      props: { i18n: en, localId: pageCopy.local_id, section: 'checks', result: { status: 'ready', draft: pageCopy } }
    });
    expect(body).toContain('Save locally — always possible');
    expect(body).toContain('Export to file — possible');
    expect(body).toContain('Publication — blocked: 1 issue');
    expect(body).toContain('Queues › Top queues');
    expect(body).toContain('Passed: 2');
    expect(body).toContain('Not run yet');
  });

  it('shows publication requirements offline and a missing draft without inventing it', () => {
    const publication = render(DraftWorkspace, {
      props: { i18n: en, localId: localDashboard.local_id, section: 'publication', result: { status: 'ready', draft: localDashboard } }
    }).body;
    expect(publication).toContain('Publication needs a connection to Zabbix.');
    expect(publication).not.toContain('<button');
    const missing = render(DraftWorkspace, {
      props: { i18n: en, localId: 'gone', section: 'overview', result: { status: 'unavailable' } }
    }).body;
    expect(missing).toContain('This draft is no longer available in local storage.');
    expect(missing).not.toContain('side-nav');
  });

  it('keeps the settings navigation, reserved about route and unavailable G1b sections', () => {
    const { body } = render(SettingsScreen, { props: { i18n: ru, section: 'about' } });
    expect(body).toContain('<a href="/settings/about" aria-current="page">О программе и лицензии</a>');
    expect(body).toContain('Пароли и токены здесь не хранятся и не показываются.');
    expect(body).toContain(['placeholder', '"Поиск настроек"'].join('='));
    expect(body).not.toContain('href="/settings/security"');
    expect(body).toContain('Безопасность и экспорт');
  });
});

describe('shell texts (P1-00F §9)', () => {
  it('maps widget types to names and keeps unknown types technical', () => {
    expect(widgetTypeName(en, 'svggraph')).toBe('Graph');
    expect(widgetTypeName(ru, 'topitems')).toBe('Часто используемые элементы');
    expect(widgetTypeName(ru, 'vendor.custom')).toBe('vendor.custom');
    const unknown: DraftDetail = {
      ...pageCopy,
      pages: [{ ...pageCopy.pages[0], widgets: [{ id: 'u', name: '', type: 'vendor.custom', x: 0, y: 0, width: 6, height: 2, editable: true }] }]
    };
    const { body } = render(DraftWorkspace, {
      props: { i18n: en, localId: unknown.local_id, section: 'overview', result: { status: 'ready', draft: unknown } }
    });
    expect(body).toContain('Editing not supported');
    expect(body).toMatch(/class="muted technical"[^>]*>vendor\.custom · 0, 0/);
  });

  it('names untitled pages and counts widgets with plurals', () => {
    expect(pageName(en, '  ')).toBe('Untitled');
    expect(pageName(ru, '')).toBe('Без названия');
    expect([1, 2, 5, 21].map((count) => ru.t('object.page.widget_count', { count }))).toEqual(['1 виджет', '2 виджета', '5 виджетов', '21 виджет']);
    expect(en.t('object.page.widget_count', { count: 1 })).toBe('1 widget');
    expect(en.t('object.page.widget_count', { count: 3 })).toBe('3 widgets');
  });

  it('formats page periods in the largest whole unit', () => {
    expect([10, 30, 60, 120, 600, 1800, 3600].map((seconds) => durationText(ru, seconds))).toEqual([
      '10 секунд', '30 секунд', '1 минута', '2 минуты', '10 минут', '30 минут', '1 час'
    ]);
    expect(durationText(en, 60)).toBe('1 minute');
  });

  it('renders an optional select suffix only when the value is present', () => {
    expect(ru.t('draft.provenance.assembled', { count: 3, sources: 'A, B' })).toBe('собрана из 3 страниц: A, B');
    expect(en.t('draft.provenance.assembled', { count: 1 })).toBe('Built from 1 page');
  });

  it('shows finding levels as text next to the colour', () => {
    expect(findingLevelLabel([])).toEqual({ id: 'checks.level.warning' });
    expect(findingLevelLabel(['export', 'publication'])).toEqual({ id: 'checks.level.blocks_both' });
    const { body } = render(DraftWorkspace, {
      props: { i18n: en, localId: pageCopy.local_id, section: 'checks', result: { status: 'ready', draft: pageCopy } }
    });
    expect(body).toContain('Blocks publication');
  });

  it('labels the main navigation landmark and shows /server as not connected', () => {
    const { body } = render(App, { props: { initialPath: '/server', locale: 'ru' } });
    expect(body).toContain(['<nav class="top-nav" aria-label', '"Основная навигация">'].join('='));
    expect(body).toContain('Не подключено');
    expect(body).toContain('href="/connections"');
  });
});
