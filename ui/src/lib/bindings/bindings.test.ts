import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import { DEMO_REPLACE_HOST, createDemoReplaceHostClient } from '../../demo/replace-host';
import { resolveText } from '../components';
import { catalogs, createI18n, type MessageId } from '../i18n';
import BindingsSection from './BindingsSection.svelte';
import { createHttpReplaceHostClient, createMemoryReplaceHostClient, previewFromWire } from './client';
import {
  CHECK_REASON_CODES,
  FINDING_CODES,
  PLAN_REASON_CODES,
  REASON_CODES,
  REPLACE_HOST_ERROR_CODES,
  checkReasonText,
  findingText,
  planInvalidText,
  reasonText,
  replaceHostCodeText
} from './messages';
import { matchCounts, referencesInScope } from './model';
import PublicationSection from './PublicationSection.svelte';
import { ReplaceHostStore } from './store.svelte';

// Fixtures are the synthetic DEMO data; no real infrastructure.
const i18n = createI18n('ru');
const en = createI18n('en');
const { preview, inventory, search } = DEMO_REPLACE_HOST;

function idOf(text: { id: string }): MessageId {
  return text.id as MessageId;
}

describe('ReplaceHost codes → message IDs', () => {
  it('maps every resolver reason, check reason and finding to a real message in EN and RU', () => {
    for (const code of REASON_CODES) {
      const text = reasonText(code);
      expect(text.id, code).not.toBe('bindings.reason.unknown');
      expect(catalogs.en[idOf(text)], code).toBeTypeOf('string');
      expect(catalogs.ru[idOf(text)], code).toBeTypeOf('string');
    }
    for (const code of CHECK_REASON_CODES) expect(checkReasonText(code).id, code).not.toBe('bindings.reason.unknown');
    for (const code of FINDING_CODES) {
      const finding = { code, severity: 'blocks_publication' as const, group: 'bindings' as const, subject: null, reason: null, params: { count: '2', contract: 'update_several_pages', differences: 'units,origin', base_version: '7.4.16' } };
      const text = findingText(i18n, finding, 'host-a');
      expect(text.id, code).not.toBe('publication.finding.unknown');
      expect(resolveText(i18n, text), code).not.toContain('{');
    }
    for (const code of REPLACE_HOST_ERROR_CODES) {
      const text = replaceHostCodeText(code);
      expect(text.id, code).not.toBe('errors.connection.unknown');
    }
  });

  it('names every plan invalidation reason and the resend contract', () => {
    expect(PLAN_REASON_CODES).toHaveLength(7);
    for (const code of PLAN_REASON_CODES) {
      for (const locale of [i18n, en]) {
        const text = resolveText(locale, planInvalidText(locale, code));
        expect(text, code).not.toContain('{');
        expect(text, code).not.toContain(code);
      }
    }
    expect(resolveText(i18n, planInvalidText(i18n, 'session_ended'))).toBe(
      'План больше не действует: локальный сеанс завершён. Создайте новый предпросмотр.'
    );
    expect(resolveText(en, planInvalidText(en, 'future_reason'))).toContain('future_reason');
    const resend = findingText(
      en,
      { code: 'write_contract_unverified', severity: 'blocks_publication', group: 'widgets', subject: null, reason: null, params: { contract: 'resend_widget_type', widget_type: 'gauge' } },
      'a'
    );
    expect(resolveText(en, resend)).toBe('Not verified by a laboratory contract: resending an unchanged gauge widget on a changed page');
  });

  it('keeps unknown codes visible instead of empty', () => {
    expect(reasonText('future_reason')).toEqual({ id: 'bindings.reason.unknown', params: { code: 'future_reason' } });
    expect(replaceHostCodeText('something_new')).toEqual({ id: 'errors.connection.unknown', params: { code: 'something_new' } });
    expect(en.t('publication.finding.write_contract_unverified', { contract: 'x' })).toContain('x');
  });

  it('localizes difference lists and contracts', () => {
    const text = findingText(i18n, { code: 'match_differences', severity: 'informational', group: 'bindings', subject: null, reason: null, params: { differences: 'units,origin' } }, 'a');
    expect(resolveText(i18n, text)).toContain('единицы измерения');
    const contract = findingText(en, { code: 'write_contract_unverified', severity: 'blocks_publication', group: 'connection', subject: null, reason: null, params: { contract: 'update_several_pages' } }, 'a');
    expect(resolveText(en, contract)).toContain('several pages');
  });
});

describe('HTTP client of the ReplaceHost routes', () => {
  it('parses RFC 3339 times and sends CSRF only with POST', async () => {
    const calls: Array<{ path: string; init: RequestInit }> = [];
    const wire = {
      ...preview,
      operation: { ...preview.operation, recorded_at: '2026-10-26T08:30:00Z', calculated_at: '2026-10-26T08:30:00.500Z' },
      checked_at: null
    };
    const fetcher = (async (path: string, init: RequestInit) => {
      calls.push({ path, init });
      if (path === '/api/session') return new Response(JSON.stringify({ csrf_token: 'csrf-1' }), { status: 200 });
      return new Response(JSON.stringify(wire), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }) as typeof fetch;
    const client = createHttpReplaceHostClient(fetcher);
    const loaded = await client.preview('wc-1');
    expect(loaded.operation.calculated_at).toBe(Date.UTC(2026, 9, 26, 8, 30, 0, 500));
    expect(loaded.checked_at).toBeNull();
    await client.decide('wc-1', { page_index: 0, widget_index: 1, field: 'itemid.0' }, { kind: 'keep_on_source' });
    expect(calls[0]?.path).toBe('/api/drafts/wc-1/replace-host');
    expect((calls[0]?.init.headers as Record<string, string>)['X-ZDashstitch-CSRF']).toBeUndefined();
    const post = calls.find((call) => call.path.endsWith('/decide'));
    expect((post?.init.headers as Record<string, string>)['X-ZDashstitch-CSRF']).toBe('csrf-1');
    expect(JSON.parse(String(post?.init.body))).toEqual({ key: { page_index: 0, widget_index: 1, field: 'itemid.0' }, decision: { kind: 'keep_on_source' } });
    await client.searchHosts('wc-1', '10', ' pg ');
    expect(calls.at(-1)?.path).toBe('/api/drafts/wc-1/replace-host/hosts?source_hostid=10&search=pg');
  });

  it('converts a server-side checked time', () => {
    const parsed = previewFromWire({ ...preview, operation: { ...preview.operation, recorded_at: null, calculated_at: null }, checked_at: '2026-10-26T09:00:00Z' } as never);
    expect(parsed.checked_at).toBe(Date.UTC(2026, 9, 26, 9));
    expect(Number.isNaN(parsed.operation.calculated_at)).toBe(true);
  });
});

describe('model helpers', () => {
  it('splits references inside and outside a partial scope', () => {
    const host = inventory.hosts[0]!;
    expect(referencesInScope(host, inventory.pages, { kind: 'dashboard' }).outside).toBe(0);
    const widgets = referencesInScope(host, inventory.pages, { kind: 'widgets', widgetids: ['70011'] });
    expect(widgets.inside.length).toBe(2);
    expect(widgets.outside).toBe(2);
  });

  it('counts rows by match with kept rows apart', () => {
    const counts = matchCounts(preview.rows);
    expect(counts.all).toBe(preview.rows.length);
    expect(counts.kept_on_source).toBe(2);
    expect(counts.exact + counts.compatible + counts.ambiguous + counts.missing + counts.unresolved_lld + counts.denied + counts.kept_on_source).toBe(counts.all);
  });
});

describe('ReplaceHost store over the in-memory client', () => {
  it('loads, decides, undoes, confirms and resets a stored replacement', async () => {
    const store = new ReplaceHostStore(createDemoReplaceHostClient());
    store.open('demo-synthetic-01');
    await store.load();
    expect(store.status).toBe('ready');
    expect(store.step).toBe('mapping');
    const ambiguous = store.preview?.rows.find((row) => row.match === 'ambiguous');
    expect(ambiguous).toBeDefined();
    expect(await store.decide(ambiguous!.key, { kind: 'choose_candidate', itemid: '60002' })).toBe(true);
    const decided = store.preview?.rows.find((row) => row.key.field === ambiguous!.key.field);
    expect(decided?.state).toBe('changed');
    expect(store.announcement).toEqual({ id: 'bindings.decision.done' });
    expect(await store.undo(decided!.decision_seq!)).toBe(true);
    expect(store.preview?.rows.find((row) => row.key.field === ambiguous!.key.field)?.state).toBe('unresolved');
    expect(await store.confirmKept()).toBe(true);
    expect(store.preview?.kept_on_source.confirmed).toBe(true);
    expect(await store.reset()).toBe(true);
    expect(store.status).toBe('none');
    expect(store.preview).toBeNull();
    expect(store.step).toBe('scope');
  });

  it('walks the wizard: inventory, target search, calculation', async () => {
    const store = new ReplaceHostStore(createDemoReplaceHostClient());
    store.open('demo-synthetic-02');
    await store.load();
    expect(store.status).toBe('none');
    await store.loadInventory();
    expect(store.sourceHostid).toBe('20101');
    store.scopeKind = 'widgets';
    expect(store.scope).toBeNull();
    store.scopeWidgets = ['70011'];
    expect(store.scope).toEqual({ kind: 'widgets', widgetids: ['70011'] });
    store.searchInput = 'demo host';
    await store.searchTargets();
    expect(store.searchResult?.hosts.length).toBe(2);
    store.chooseTarget('20201');
    expect(await store.calculate()).toBe(true);
    expect(store.step).toBe('mapping');
    expect(store.preview?.operation.target_host?.name).toBe('Demo host E');
  });

  it('shows a decision error as a code, not prose', async () => {
    const store = new ReplaceHostStore(createMemoryReplaceHostClient({ d: { preview, calculated: true } }));
    store.open('d');
    await store.load();
    const kept = store.preview!.rows.find((row) => row.state === 'kept_on_source')!;
    expect(await store.decide(kept.key, { kind: 'keep_on_source' })).toBe(false);
    expect(store.commandFailure?.code).toBe('decision_not_applicable');
  });
});

describe('ReplaceHost screens (09-03, 11-03)', () => {
  async function loaded(localId = 'demo-synthetic-01'): Promise<ReplaceHostStore> {
    const store = new ReplaceHostStore(createDemoReplaceHostClient());
    store.open(localId);
    await store.load();
    return store;
  }

  it('shows the mapping with match words and icons, never colour only', async () => {
    const store = await loaded();
    const { body } = render(BindingsSection, { props: { i18n, store, localId: 'demo-synthetic-01', serverReads: false } });
    expect(body).toContain('aria-current="step"');
    expect(body).toContain('Заменить узел сети: Демо-узел А → Демо-узел Б');
    expect(body).toContain('Неоднозначно');
    expect(body).toContain('Не найдено (LLD)');
    expect(body).toContain('С отличиями');
    expect(body).toContain('Останется на старом');
    expect(body).toContain('Для «Проверить все» нужно подключение.');
    expect(body).not.toMatch(/\{[a-z_]+\}/);
  });

  it('shows the summary with the list to confirm', async () => {
    const store = await loaded();
    store.step = 'summary';
    const { body } = render(BindingsSection, { props: { i18n, store, localId: 'demo-synthetic-01', serverReads: true } });
    expect(body).toContain('Итог замены');
    expect(body).toContain('Подтвердить список (2)');
    expect(body).toContain('/draft/demo-synthetic-01/publication');
  });

  it('asks for a connection before choosing hosts', async () => {
    const store = await loaded('demo-synthetic-02');
    const { body } = render(BindingsSection, { props: { i18n, store, localId: 'demo-synthetic-02', serverReads: false } });
    expect(body).toContain('Для подбора узлов нужно подключение');
  });

  it('previews publication findings offline and keeps write steps unavailable', async () => {
    const store = await loaded();
    const draft = {
      local_id: 'demo-synthetic-01',
      kind: 'dashboard' as const,
      origin: 'server_copy' as const,
      name: 'Demo',
      source: { dashboard_name: 'Демо: базы данных', snapshot_at: Date.UTC(2026, 9, 24) },
      local_state: 'saved' as const,
      updated_at: 0,
      pages: []
    };
    const { body } = render(PublicationSection, {
      props: { i18n, store, draft, localId: 'demo-synthetic-01', serverReads: false, connection: null }
    });
    expect(body).toContain('Публикация заблокирована');
    expect(body).toContain('Блокирует');
    expect(body).toContain('Предупреждение');
    expect(body).toContain('Подтвердите список виджетов, которые останутся на старом узле');
    expect(body).toContain('Автономно');
    expect((body.match(/disabled/g) ?? []).length).toBeGreaterThanOrEqual(3);
    expect(body).not.toMatch(/\{[a-z_]+\}/);
  });

  it('uses only synthetic demo values', () => {
    const text = JSON.stringify(DEMO_REPLACE_HOST);
    expect(text).not.toMatch(/\b\d{1,3}(?:\.\d{1,3}){3}\b/);
    expect(text).not.toMatch(/[a-z]+:[/]{2}/i);
    expect(search.hosts.every((host) => host.name.startsWith('Demo'))).toBe(true);
  });
});
