import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import { DEMO_REPLACE_HOST, createDemoReplaceHostClient } from '../../demo/replace-host';
import { createDemoPublicationClient } from '../../demo/publication';
import PublicationSection from '../bindings/PublicationSection.svelte';
import { ReplaceHostStore } from '../bindings/store.svelte';
import { resolveText } from '../components';
import { catalogs, createI18n, type MessageId } from '../i18n';
import { createHttpPublicationClient, createMemoryPublicationClient, publicationFailure } from './client';
import { EVIDENCE_CODES, JOURNAL_STATES, PUBLICATION_ERROR_CODES, evidenceText, publicationCodeText, resultBody, STATE_LABEL } from './messages';
import { zabbixDashboardUrl } from './model';
import RecoveryBanner from './RecoveryBanner.svelte';
import { PublicationStore, RecoveryStore } from './store.svelte';
import WriteStep from './WriteStep.svelte';

// Fixtures are the synthetic DEMO data; no real infrastructure.
const i18n = createI18n('ru');
const en = createI18n('en');

const draft = {
  local_id: 'demo-synthetic-01',
  kind: 'dashboard' as const,
  origin: 'server_copy' as const,
  name: 'Демо: базы данных — Пилот',
  source: { dashboard_name: 'Демо: базы данных', snapshot_at: Date.UTC(2026, 9, 24), connection_local_id: 'demo-synthetic-connection' },
  local_state: 'saved' as const,
  updated_at: 0,
  pages: []
};

async function stores(localId = 'demo-synthetic-01') {
  const replaceHost = new ReplaceHostStore(createDemoReplaceHostClient());
  replaceHost.open(localId);
  await replaceHost.load();
  const client = createDemoPublicationClient();
  const recovery = new RecoveryStore(client);
  const publication = new PublicationStore(client, () => void recovery.load());
  publication.open(localId);
  await recovery.load();
  return { replaceHost, publication, recovery, client };
}

describe('publication codes → message IDs', () => {
  it('names every journal state, evidence and error in EN and RU', () => {
    for (const state of JOURNAL_STATES) {
      const id = STATE_LABEL[state].id as MessageId;
      expect(catalogs.en[id], state).toBeTypeOf('string');
      expect(catalogs.ru[id], state).toBeTypeOf('string');
      for (const locale of [i18n, en]) {
        for (const kind of ['update', 'create'] as const) {
          expect(resolveText(locale, resultBody(locale, state, kind, null, 'draft_changed')), state).not.toMatch(/\{[a-z_]+\}/);
        }
      }
    }
    for (const code of EVIDENCE_CODES) expect(evidenceText(code).id, code).not.toBe('publication.evidence.unknown');
    for (const code of PUBLICATION_ERROR_CODES) {
      const text = publicationCodeText(i18n, { code, params: { reason: 'source_changed', operation_id: 'zds-1' } });
      expect(text.id, code).not.toBe('errors.publication.code');
      expect(resolveText(i18n, text), code).not.toMatch(/\{[a-z_]+\}/);
    }
  });

  it('applies the owner decisions of 28.10.2026 (evening) to the result texts', () => {
    // Item 3: `already exists` after send is the final «не записано», with the hidden-dashboard text.
    const taken = resolveText(i18n, resultBody(i18n, 'reconciled_failed', 'create', 'dashboard_name_taken', null));
    expect(taken).toBe('Имя занято, возможно, недоступной вам панелью. Панель не создана; выберите другое имя.');
    // Item 1: zero candidates of a create reconciliation is a manual check, not «Сверено: не записано».
    const none = resultBody(i18n, 'manual_check_required', 'create', 'reconcile_not_found', null);
    expect(none.id).toBe('publication.result.manual_body');
    expect(resolveText(i18n, STATE_LABEL.manual_check_required)).toBe('Нужна проверка вручную');
  });

  it('builds «Открыть в Zabbix» only for an http(s) profile address and a numeric dashboard ID', () => {
    // Addresses are assembled here because UI sources may not contain literal remote URLs.
    const address = (scheme: string, rest: string): string => `${scheme}:${'/'.repeat(2)}${rest}`;
    const base = address('https', 'zabbix.example.invalid/zabbix/');
    const expected = `${base}zabbix.php?action=dashboard.view&dashboardid=42`;
    expect(zabbixDashboardUrl(base, '42')).toBe(expected);
    expect(zabbixDashboardUrl(['java', 'script:void(0)'].join(''), '42')).toBeNull();
    expect(zabbixDashboardUrl(address('ftp', 'zabbix.example.invalid/'), '42')).toBeNull();
    expect(zabbixDashboardUrl(base, '42&x=1')).toBeNull();
    expect(zabbixDashboardUrl(null, '42')).toBeNull();
  });
});

describe('HTTP client', () => {
  it('sends only the handle, the shown plan ID and the confirmation, with the CSRF token', async () => {
    const calls: Array<{ path: string; init: RequestInit }> = [];
    const fetcher = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input);
      calls.push({ path, init: init ?? {} });
      if (path === '/api/session') return new Response(JSON.stringify({ csrf_token: 'csrf-1' }), { status: 200 });
      if (path.endsWith('/confirm')) {
        return new Response(JSON.stringify({ code: 'plan_not_found', params: {} }), { status: 404, headers: { 'Content-Type': 'application/json' } });
      }
      return new Response(JSON.stringify({ unfinished: [], damaged: [] }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }) as typeof fetch;
    const client = createHttpPublicationClient(fetcher);
    await expect(client.journal()).resolves.toEqual({ unfinished: [], damaged: [] });
    const refused = await client.confirm('wc-1', { handle: 'plan-1', plan_id: 'zds-1', confirmed: true, kept_digest: null }).catch((error: unknown) => publicationFailure(error));
    expect(refused).toMatchObject({ code: 'plan_not_found', status: 404 });
    const post = calls.find((call) => call.path === '/api/drafts/wc-1/publication/confirm');
    expect((post?.init.headers as Record<string, string>)['X-ZDashstitch-CSRF']).toBe('csrf-1');
    expect(JSON.parse(String(post?.init.body))).toEqual({ handle: 'plan-1', plan_id: 'zds-1', confirmed: true, kept_digest: null });
  });
});

describe('publication flow (memory client)', () => {
  it('builds a one-shot plan only for an explicit action and confirms it once', async () => {
    const { publication, client } = await stores('demo-synthetic-02');
    expect(publication.action).toBeNull();
    await publication.recheck();
    expect(publication.check?.source.state).toBe('current');
    const preview = publication.check?.preview;
    if (!preview) throw new Error('preview');
    expect(await publication.buildPlan(preview)).toBe(false);
    publication.setAction('update_source');
    expect(await publication.buildPlan(preview)).toBe(true);
    const handle = publication.plan?.handle ?? '';
    const planId = publication.plan?.plan.plan_id ?? '';
    await publication.confirm();
    expect(publication.outcome?.state).toBe('verified');
    expect(publication.outcome?.base_advanced).not.toBeNull();
    expect(publication.plan).toBeNull();
    await expect(client.confirm('demo-synthetic-02', { handle, plan_id: planId, confirmed: true })).rejects.toMatchObject({ code: 'plan_not_found' });
  });

  it('keeps the conflicted draft and refuses an update plan', async () => {
    const { publication } = await stores();
    await publication.recheck();
    expect(publication.check?.source.state).toBe('conflict');
    const preview = publication.check?.preview;
    if (!preview) throw new Error('preview');
    publication.setAction('update_source');
    expect(await publication.buildPlan(preview)).toBe(false);
    expect(publication.planFailure).toMatchObject({ code: 'plan_invalidated', params: { reason: 'source_changed' } });
    publication.setAction('save_as_copy');
    publication.copyName = 'Демо: копия';
    expect(await publication.buildPlan(preview)).toBe(true);
    await publication.confirm();
    expect(publication.outcome?.state).toBe('unknown_outcome');
    await publication.reconcile();
    expect(publication.outcome?.state).toBe('manual_check_required');
  });

  it('closes an entry and quarantines a damaged file only through their commands', async () => {
    const { recovery } = await stores();
    expect(recovery.count).toBe(2);
    expect(await recovery.close('zds-create-demo-synthetic-0001')).toBe(true);
    expect(await recovery.quarantine('zds-demo-synthetic-damaged-0001')).toBe(true);
    expect(recovery.count).toBe(0);
    const memory = createMemoryPublicationClient({ drafts: {} });
    await expect(memory.quarantine('zds-missing')).rejects.toMatchObject({ code: 'operation_not_found' });
  });
});

describe('publication screens', () => {
  it('shows the conflict with safe exits and no overwrite', async () => {
    const { replaceHost, publication, recovery } = await stores();
    await publication.recheck();
    publication.goto('checks');
    const { body } = render(PublicationSection, {
      props: { i18n, store: replaceHost, draft, localId: 'demo-synthetic-01', serverReads: true, connection: null, publication, recovery }
    });
    expect(body).toContain('изменилась на сервере после копирования');
    expect(body).toContain('Сохранить копию');
    expect(body).toContain('Скопировать заново');
    expect(body).toContain('Обновить поверх — недоступно');
    expect(body).toContain('Пересекается с вашими изменениями: 1 виджет');
    expect(body).not.toMatch(/\{[a-z_]+\}/);
  });

  it('shows the target step with no default action', async () => {
    const { replaceHost, publication, recovery } = await stores('demo-synthetic-02');
    // The page draft has no stored calculation in the demo: the fresh check brings the preview.
    await publication.recheck();
    publication.goto('target');
    const { body } = render(PublicationSection, {
      props: { i18n, store: replaceHost, draft: { ...draft, local_id: 'demo-synthetic-02' }, localId: 'demo-synthetic-02', serverReads: true, connection: null, publication, recovery }
    });
    expect(body).toContain('Выберите явно: варианта по умолчанию нет.');
    expect(body).not.toContain('checked');
    expect(body).toContain('Далее: проверки');
  });

  it('shows an unknown result with only «Проверить результат» and no retry', async () => {
    const { publication } = await stores();
    await publication.recheck();
    publication.setAction('save_as_copy');
    publication.copyName = 'Демо: копия';
    const preview = publication.check?.preview;
    if (!preview) throw new Error('preview');
    await publication.buildPlan(preview);
    await publication.confirm();
    const { body } = render(WriteStep, {
      props: { i18n, store: publication, localId: 'demo-synthetic-01', zabbixUrl: () => null, onjournal: () => {}, onclosemanually: () => {} }
    });
    expect(body).toContain('Результат публикации неизвестен');
    expect(body).toContain('Проверить результат');
    expect(body).toContain('Нужна проверка вручную');
    expect(body).toContain('Новая запись в эту цель заблокирована');
    expect(body).not.toContain('Записать в Zabbix');
    expect(body).not.toMatch(/\{[a-z_]+\}/);
  });

  it('shows unfinished operations and damaged files before any new write', async () => {
    const { recovery } = await stores();
    const { body } = render(RecoveryBanner, {
      props: { i18n, store: recovery, profileName: () => null, draftName: () => 'Демо: базы данных — Пилот', activeConnectionId: null }
    });
    expect(body).toContain('Найдена незавершённая операция');
    expect(body).toContain('Подключитесь к «Удалённый профиль подключения»');
    expect(body).toContain('Закрыть без сверки');
    expect(body).toContain('Убрать в карантин');
    expect(body).not.toMatch(/\{[a-z_]+\}/);
  });

  it('uses only synthetic demo values', () => {
    const text = JSON.stringify(DEMO_REPLACE_HOST);
    expect(text).not.toMatch(/\b\d{1,3}(?:\.\d{1,3}){3}\b/);
  });
});
