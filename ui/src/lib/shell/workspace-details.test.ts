import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import { catalogs, createI18n, type MessageId } from '../i18n';
import AppHeader from './AppHeader.svelte';
import DraftDetails from './DraftDetails.svelte';
import DraftsScreen from './DraftsScreen.svelte';
import DraftWorkspace from './DraftWorkspace.svelte';
import StartScreen from './StartScreen.svelte';
import { duplicateNames, searchDrafts, selectedDraft, withConnectionNames, type DraftDetail, type DraftSummary } from './drafts';
import {
  createdDashboardLines,
  dashboardPropertiesText,
  detailRows,
  listSourceText,
  originSummaryText,
  recentOriginText,
  sourceConnectionSuffix,
  stateText
} from './format';
import { overviewSteps } from './labels';

// Test fixtures only: neutral names, no real infrastructure. Never used by the production build.
const SNAPSHOT = Date.UTC(2026, 9, 25, 11, 2);

const dashboardCopy: DraftSummary = {
  local_id: 'copy-dashboard',
  kind: 'dashboard',
  origin: 'server_copy',
  name: 'Databases: main',
  source: { dashboard_name: 'Databases: main', connection_name: 'Lab', server_version: '7.4.16', snapshot_at: SNAPSHOT },
  freshness: { state: 'conflict', changed_at: Date.UTC(2026, 9, 25, 11, 31) },
  local_state: 'dirty',
  updated_at: 50,
  counts: { pages: 3, widgets: 19 },
  created_dashboards: [
    { name: 'Databases: copy', created_at: Date.UTC(2026, 9, 24, 15, 20), connection_name: 'Lab', state: 'verified' },
    { name: 'Databases: old', created_at: Date.UTC(2026, 9, 20, 9, 0), connection_name: 'Lab', state: 'not_found' }
  ]
};

const pageCopy: DraftSummary = {
  local_id: 'copy-page',
  kind: 'page',
  origin: 'server_copy',
  name: 'Queues',
  source: { dashboard_name: 'Databases: main', page_name: 'Queues', page_number: 2, snapshot_at: SNAPSHOT },
  freshness: { state: 'not_checked' },
  local_state: 'saved',
  updated_at: 40
};

const assembled: DraftSummary = {
  local_id: 'assembled',
  kind: 'dashboard',
  origin: 'local',
  name: 'Summary',
  local_state: 'saved',
  updated_at: 30,
  provenance: { kind: 'assembled', page_count: 3, sources: ['Main', 'Pilot', 'colleague file'] }
};

const imported: DraftSummary = {
  local_id: 'imported',
  kind: 'page',
  origin: 'local',
  name: 'Keeper',
  local_state: 'saved',
  updated_at: 20,
  provenance: { kind: 'imported_file' }
};

const en = createI18n('en');
const ru = createI18n('ru');

/** Server-rendered markup without Svelte hydration markers. */
function html(body: string): string {
  return body.replace(/<!--[^]*?-->/g, '');
}

/** A disabled button whose visible text is `label` (an icon may come first). */
function disabledButton(label: string): RegExp {
  const escaped = label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`<button[^>]*disabled[^>]*>(?:(?!</button>)[^])*<span>${escaped}</span>`);
}

describe('search and selection in «Drafts» (P1-03F)', () => {
  const drafts = [dashboardCopy, pageCopy, assembled, imported];

  it('finds drafts by a case-insensitive part of the name', () => {
    expect(searchDrafts(drafts, 'que', 'en').map((draft) => draft.local_id)).toEqual(['copy-page']);
    expect(searchDrafts(drafts, '  DATABASES:   MAIN ', 'en').map((draft) => draft.local_id)).toEqual(['copy-dashboard']);
    expect(searchDrafts(drafts, '', 'en')).toHaveLength(4);
    expect(searchDrafts(drafts, 'missing', 'en')).toEqual([]);
  });

  it('compares Unicode names in one normal form and the UI locale case', () => {
    const named = [{ name: 'Сводная ЙОД' }, { name: 'Другая' }];
    // «й» typed as «и» + combining breve still matches the composed letter.
    expect(searchDrafts(named, 'йод', 'ru')).toEqual([{ name: 'Сводная ЙОД' }]);
    expect(searchDrafts(named, 'СВОДНАЯ', 'ru')).toEqual([{ name: 'Сводная ЙОД' }]);
  });

  it('keeps the chosen row while visible and falls back to the first row', () => {
    expect(selectedDraft(drafts, 'imported')?.local_id).toBe('imported');
    expect(selectedDraft(drafts, 'filtered-out')?.local_id).toBe('copy-dashboard');
    expect(selectedDraft(drafts, null)?.local_id).toBe('copy-dashboard');
    expect(selectedDraft([], 'imported')).toBeUndefined();
  });
});

describe('origin lines (P1-00F §9.2)', () => {
  it('fills the «Source» column from the provenance or the snapshot', () => {
    expect(listSourceText(ru, assembled)).toBe('собрана из 3 страниц');
    expect(listSourceText(ru, imported)).toBe('импортирована из файла');
    expect(listSourceText(ru, { ...imported, provenance: { kind: 'page_file' } })).toBe('файл страницы');
    expect(listSourceText(ru, { ...imported, provenance: { kind: 'legacy_file' } })).toBe('файл старого формата');
    expect(listSourceText(ru, { ...imported, provenance: { kind: 'created_here' } })).toBe('создана здесь');
    expect(listSourceText(ru, { ...imported, provenance: undefined })).toBe('—');
    expect(listSourceText(ru, pageCopy)).toBe('«Databases: main», стр. 2');
    expect(listSourceText(ru, dashboardCopy)).toBe(`Lab · ${ru.date(SNAPSHOT, { dateStyle: 'short', timeStyle: 'short' })}`);
  });

  it('adds sources and local state notes to «Recent» entries', () => {
    expect(recentOriginText(ru, assembled)).toBe('собрана из 3 страниц: Main, Pilot, colleague file · не публиковалась');
    expect(recentOriginText(ru, { ...pageCopy, local_state: 'dirty' })).toBe('взята из панели «Databases: main» · изменена локально');
    expect(recentOriginText(ru, imported)).toBe('импортирована из файла · не публиковалась');
    expect(recentOriginText(en, { ...imported, last_published_at: 1 })).toBe('Imported from a file');
    expect(recentOriginText(en, { ...assembled, provenance: { kind: 'assembled', page_count: 1, sources: [] }, last_published_at: 1 })).toBe('Built from 1 page');
  });

  it('shows short states in rows and the long form in details', () => {
    expect(stateText(ru, dashboardCopy)).toBe('Изменена · Изменена на сервере');
    expect(stateText(ru, pageCopy)).toBe('Сохранена · Не проверена');
    expect(stateText(en, assembled)).toBe('Saved');
  });

  it('shows a short label for every snapshot freshness state in rows', () => {
    const row = (state: 'current' | 'stale' | 'conflict' | 'source_missing' | 'not_checked') =>
      ({ local_state: 'saved', freshness: { state } }) as const;
    const expected = {
      current: ['Проверена', 'Checked'],
      stale: ['Устарела', 'Out of date'],
      conflict: ['Изменена на сервере', 'Changed on server'],
      source_missing: ['Источник не найден', 'Source missing'],
      not_checked: ['Не проверена', 'Not checked']
    } as const;
    for (const [state, [russian, english]] of Object.entries(expected)) {
      const draft = row(state as keyof typeof expected);
      expect(stateText(ru, draft)).toBe(`Сохранена · ${russian}`);
      expect(stateText(en, draft)).toBe(`Saved · ${english}`);
    }
  });

  it('summarizes the origin in the workspace side column', () => {
    expect(originSummaryText(ru, pageCopy)).toBe(`снимок от ${ru.date(SNAPSHOT)}`);
    expect(originSummaryText(ru, assembled)).toBe('собрана из 3 страниц');
    expect(originSummaryText(ru, { ...imported, provenance: undefined })).toBeNull();
  });
});

describe('details column (mockup 02-01)', () => {
  it('lists source, connection, base, freshness and local state of a server copy', () => {
    const rows = detailRows(ru, dashboardCopy);
    expect(rows.map((row) => row.term)).toEqual(['Источник', 'Подключение', 'База', 'Свежесть', 'Локально']);
    expect(rows[1].value).toBe('Lab · Zabbix 7.4.16');
    expect(rows[2].value).toBe(`снимок ${ru.date(SNAPSHOT, { dateStyle: 'short', timeStyle: 'short' })}`);
    expect(rows[3].value).toMatch(/^изменена на сервере \(/);
    expect(rows[4].value).toBe('Изменена · 3 страницы, 19 виджетов');
  });

  it('leaves out what a local draft does not have', () => {
    expect(detailRows(en, assembled).map((row) => row.term)).toEqual(['Source', 'Locally']);
    expect(detailRows(en, { ...imported, provenance: undefined }).map((row) => row.term)).toEqual(['Locally']);
  });

  it('marks dashboards created from the draft as verified or not found', () => {
    const [verified, missing] = dashboardCopy.created_dashboards ?? [];
    expect(createdDashboardLines(ru, verified)[0]).toMatch(/^«Databases: copy» — создана /);
    expect(createdDashboardLines(ru, verified)[1]).toBe('Проверено повторным чтением · Lab');
    expect(createdDashboardLines(ru, missing)[1]).toBe('Не найдена при последней проверке · Lab');
  });

  it('renders the details of a server copy with only «Open» available', () => {
    const { body } = render(DraftDetails, { props: { i18n: ru, draft: dashboardCopy } });
    expect(body).toContain('<h2 id="drafts-details-title">Databases: main</h2>');
    expect(body).toContain('href="/draft/copy-dashboard/overview"');
    expect(body).toContain('Открыть');
    for (const action of ['Дублировать', 'Переименовать', 'Сделать переносимой для экспорта…', 'Удалить']) {
      expect(html(body)).toMatch(disabledButton(action));
    }
    expect(body).toContain('Появится позже');
    expect(body).toContain('Создано из этой копии');
    expect(body).toContain('Удаление не трогает панели на сервере.');
    expect(body).toContain('Копия с сервера экспортируется только после преобразования в переносимую.');
    expect(body).toContain('class="not-found"');
  });

  it('offers export to file for a local draft instead of making it portable', () => {
    const { body } = render(DraftDetails, { props: { i18n: en, draft: { ...assembled, created_dashboards: dashboardCopy.created_dashboards } } });
    expect(body).toContain('Export to file…');
    expect(body).not.toContain('Make portable for export…');
    expect(body).toContain('Created from this draft');
    expect(body).not.toContain('A copy from server is exported only');
  });
});

describe('«Drafts» screen (mockup 02-01)', () => {
  const list = { status: 'ready' as const, drafts: [imported, assembled, pageCopy, dashboardCopy] };

  it('shows search, the unavailable «Create» action and the sort hint', () => {
    const { body } = render(DraftsScreen, { props: { i18n: ru, drafts: list } });
    expect(body).toContain(['aria-label', '"Поиск по имени"'].join('='));
    expect(body).toContain('type="search"');
    expect(html(body)).toMatch(/<button[^>]*disabled[^>]*>\s*<span class="icon" aria-hidden="true"><svg[^>]*>(?:(?!<\/button>)[^])*<\/svg><\/span>\s*<span>Создать<\/span>/);
    expect(body).toContain('Сортировка — по времени изменения');
    expect(body).toContain('Появится позже');
  });

  it('selects the newest draft, keeps one row tab stop and shows its details', () => {
    const { body } = render(DraftsScreen, { props: { i18n: en, drafts: list } });
    expect(body.match(/aria-current="true"/g)).toHaveLength(1);
    expect(body).toMatch(/id="drafts-row-copy-dashboard"[^>]*aria-current="true"[^>]*tabindex="0"/);
    expect(body.match(/tabindex="0"/g)).toHaveLength(1);
    expect(body).toContain(['aria-labelledby', '"drafts-details-title"'].join('='));
    expect(body).toContain('Changed · Changed on server');
    expect(body).toContain('“Databases: main”, page 2');
    expect(body).toContain('role="status"');
  });

  it('shows the source connection next to names that several drafts share (P2-08A)', () => {
    const SOURCE = { dashboard_name: 'Databases: main', server_version: '7.4.16', snapshot_at: SNAPSHOT };
    const names = (id: string): string | undefined => ({ 'conn-lab': 'Lab', 'conn-pilot': 'Pilot' })[id];
    const fromLab = withConnectionNames({ ...dashboardCopy, source: { ...SOURCE, connection_local_id: 'conn-lab' } }, names);
    const fromPilot = withConnectionNames(
      { ...dashboardCopy, local_id: 'copy-dashboard-2', updated_at: 45, source: { ...SOURCE, connection_local_id: 'conn-pilot' } },
      names
    );
    const unique = withConnectionNames({ ...pageCopy, source: { ...SOURCE, connection_local_id: 'conn-lab' } }, names);
    const shared = { status: 'ready' as const, drafts: [fromLab, fromPilot, unique] };
    const screen = render(DraftsScreen, { props: { i18n: en, drafts: shared } }).body;
    expect(screen).toContain('<span class="muted">(Lab)</span>');
    expect(screen).toContain('<span class="muted">(Pilot)</span>');
    expect(screen.match(/<span class="muted">\(/g)).toHaveLength(2);
    const start = render(StartScreen, { props: { i18n: ru, drafts: shared } }).body;
    expect(start).toContain('<span class="muted">(Pilot)</span>');
    expect(start.match(/<span class="muted">\(/g)).toHaveLength(2);
    expect(duplicateNames([{ name: 'Main ' }, { name: 'main' }, { name: 'Other' }], 'en')).toEqual(new Set(['main']));
    // A local draft or an unknown profile adds nothing.
    expect(sourceConnectionSuffix(en, { ...fromLab, origin: 'local' }, new Set(['databases: main']))).toBe('');
    expect(sourceConnectionSuffix(en, dashboardCopy, new Set(['databases: main']))).toBe('(Lab)');
    expect(sourceConnectionSuffix(en, { ...pageCopy, name: 'Databases: main' }, new Set(['databases: main']))).toBe('');
  });
});

describe('«Recent», «Overview» and «Structure»', () => {
  it('shows the origin line of recent drafts', () => {
    const { body } = render(StartScreen, { props: { i18n: ru, drafts: { status: 'ready', drafts: [assembled, pageCopy] } } });
    expect(body).toContain('собрана из 3 страниц: Main, Pilot, colleague file · не публиковалась');
  });

  const page: DraftDetail = {
    ...pageCopy,
    pages: [
      {
        id: 'p1',
        name: 'Queues',
        display_period: null,
        widgets: [
          { id: 'w1', name: 'Connections', type: 'svggraph', x: 0, y: 0, width: 36, height: 5, editable: true, host_name: 'host-a' },
          { id: 'w2', name: 'Disk', type: 'gauge', x: 0, y: 5, width: 24, height: 3, editable: false }
        ]
      }
    ]
  };

  it('renders source, target, composition and next steps of a page copy', () => {
    const { body } = render(DraftWorkspace, {
      props: { i18n: ru, localId: page.local_id, section: 'overview', result: { status: 'ready', draft: page } }
    });
    for (const text of [
      'Источник',
      'Снимок',
      'нужно подключение',
      'Назначение',
      'Целевая панель не выбрана',
      'Источник и назначение задаются независимо.',
      'Состав страницы',
      'График · 36 × 5',
      'узел сети: host-a',
      'Радиальный датчик · 24 × 3',
      '«Изменение не поддерживается» — ZDashstitch пока не изменяет этот тип',
      'Следующие шаги',
      '1. Выберите целевую панель',
      'Добавить страницу в панель',
      'Локальные изменения не меняют панель на сервере.',
      'Происхождение',
      'Сетка 72 колонки, занято 8 строк',
      '2 виджета'
    ]) {
      expect(body).toContain(text);
    }
    // P2-08A: steps with a section of their own link to it directly; the sections are titled.
    expect(body).toContain('2. Проверьте привязки');
    expect(body).toContain('3. Посмотрите план');
    expect(html(body)).toMatch(/<a class="button-link step-link" href="\/draft\/copy-page\/bindings">\s*Перейти в раздел «Привязки»\s*<\/a>/);
    expect(html(body)).toMatch(/<a class="button-link step-link" href="\/draft\/copy-page\/publication">\s*Перейти в раздел «Публикация»\s*<\/a>/);
    expect(body).toContain('Разделы черновика');
    expect(html(body)).toMatch(disabledButton('Добавить страницу в панель'));
  });

  it('says «Will be available later» for the source check when connected', () => {
    const { body } = render(DraftWorkspace, {
      props: { i18n: en, localId: page.local_id, section: 'overview', result: { status: 'ready', draft: page }, connected: true }
    });
    expect(body).not.toContain('needs a connection');
    expect(body).toContain('Will be available later');
  });

  it('has no target step for a dashboard', () => {
    expect(overviewSteps('dashboard').map((step) => step.label.id)).toEqual([
      'draft.overview.step.check_bindings',
      'draft.overview.step.review_plan'
    ]);
    expect(overviewSteps('page')).toHaveLength(3);
  });

  it('shows dashboard properties as in Zabbix', () => {
    expect(dashboardPropertiesText(ru, { display_period: 30, auto_start: true })).toBe(
      'Период страницы по умолчанию: 30 секунд · слайд-шоу вкл.'
    );
    expect(dashboardPropertiesText(en, { display_period: 120, auto_start: false })).toBe(
      'Default page display period: 2 minutes · slideshow off'
    );
    const dashboard: DraftDetail = {
      ...assembled,
      pages: [{ id: 'p1', name: '', display_period: null, widgets: [] }],
      properties: { display_period: 30, auto_start: true }
    };
    const { body } = render(DraftWorkspace, {
      props: { i18n: ru, localId: dashboard.local_id, section: 'structure', result: { status: 'ready', draft: dashboard } }
    });
    expect(body).toContain('Период страницы по умолчанию: 30 секунд · слайд-шоу вкл.');
    expect(html(body)).toMatch(disabledButton('Свойства панели'));
  });
});

describe('header icons (P1-01B §1)', () => {
  it('draws the Tabler settings icon next to the «Settings» text', () => {
    const { body } = render(AppHeader, { props: { i18n: ru, current: 'start', mode: 'offline' } });
    expect(html(body)).toMatch(/<a class="settings-link"[^>]*><svg class="icon"[^>]*aria-hidden="true"[^>]*>(?:(?!<\/a>)[^])*<\/svg><span>Настройки<\/span><\/a>/);
  });
});

describe('P2-08 message IDs', () => {
  // Every ID the shell uses for the items of this pass; English is the source, Russian complete.
  const ids: MessageId[] = [
    'draft.provenance.assembled',
    'draft.provenance.taken_from',
    'draft.provenance.page_of',
    'draft.provenance.imported_file',
    'draft.provenance.page_file',
    'draft.provenance.legacy_file',
    'draft.provenance.created_here',
    'draft.provenance.snapshot',
    'draft.state.modified_locally',
    'draft.state.never_published',
    'drafts.state.saved',
    'drafts.state.modified',
    'drafts.state.changed_on_server',
    'drafts.search.placeholder',
    'drafts.search.no_results',
    'drafts.action.create',
    'drafts.sort_hint',
    'drafts.details.source',
    'drafts.details.connection',
    'drafts.details.base',
    'drafts.details.base_snapshot',
    'drafts.details.freshness',
    'drafts.details.changed_on_server_at',
    'drafts.details.local',
    'drafts.details.created_item',
    'drafts.details.created_not_found',
    'drafts.created_from_copy',
    'drafts.created_from_draft',
    'drafts.action.make_portable',
    'drafts.action.export_file',
    'drafts.delete.safe',
    'drafts.server_copy_export_hint',
    'common.action.open',
    'common.action.duplicate',
    'common.action.rename',
    'common.action.delete',
    'common.state.verified',
    'draft.structure.dashboard_properties',
    'draft.structure.dashboard_properties_summary',
    'draft.structure.slideshow.on',
    'draft.structure.slideshow.off',
    'draft.overview.origin',
    'draft.overview.snapshot_from',
    'draft.overview.grid_rows_used',
    'draft.overview.local_changes_note',
    'draft.overview.source',
    'draft.overview.snapshot',
    'draft.overview.needs_connection',
    'draft.overview.target',
    'draft.overview.target.none',
    'draft.overview.target.independent',
    'draft.overview.composition.page',
    'draft.overview.composition.dashboard',
    'draft.overview.composition.help',
    'draft.overview.widget_size',
    'draft.overview.widget_host',
    'draft.overview.edit_unsupported_note',
    'draft.overview.next_steps',
    'draft.overview.step.numbered',
    'draft.overview.step.choose_target',
    'draft.overview.step.choose_target.help',
    'draft.overview.step.check_bindings',
    'draft.overview.step.check_bindings.help',
    'draft.overview.step.review_plan',
    'draft.overview.step.review_plan.help',
    'intent.append_page',
    'nav.settings'
  ];
  // Pure formats of data lists, identical in both languages by design.
  const sameInBoth = new Set<MessageId>([
    'draft.provenance.snapshot',
    'draft.overview.widget_size',
    'draft.overview.step.numbered'
  ]);

  it('exist in English and Russian, and Russian prose is translated', () => {
    for (const id of ids) {
      expect(catalogs.en[id], id).toBeTruthy();
      expect(catalogs.ru[id], id).toBeTruthy();
      if (!sameInBoth.has(id)) expect(catalogs.ru[id], id).not.toBe(catalogs.en[id]);
    }
  });
});
