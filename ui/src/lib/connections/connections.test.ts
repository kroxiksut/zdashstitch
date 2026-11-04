import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import connectionsCatalog from '../../../../lang/en/connections.json';
import App from '../../App.svelte';
import { catalogs, createI18n, type MessageId } from '../i18n';
import { resolveText, type LocalizedText } from '../components';
import { parseRoute, topSection } from '../shell/router';
import type {
  CheckReport,
  ConnectOutcome,
  ConnectionList,
  ConnectionProfile,
  ConnectionsClient,
  TestOutcome
} from './client';
import ConnectionCheck from './ConnectionCheck.svelte';
import ConnectionsScreen from './ConnectionsScreen.svelte';
import DefaultPasswordDialog from './DefaultPasswordDialog.svelte';
import { KNOWN_CODES, codeText, stateBadge, stepText } from './messages';
import ProfileEditor from './ProfileEditor.svelte';
import SignInDialog from './SignInDialog.svelte';
import {
  cardActions,
  checkReportText,
  duplicateName,
  emptyForm,
  fieldOfCode,
  formFromProfile,
  headerConnection,
  inputFromForm,
  lockoutSeconds,
  serverChanged,
  validateForm,
  withProfile
} from './state';
import { ConnectionsStore } from './store.svelte';

// Test fixtures only: reserved documentation names, no scheme (the UI source must not contain
// URLs), no real infrastructure. Never used by the production build.
const SECRET = 'fixture-secret-value-7d1e';

function profile(overrides: Partial<ConnectionProfile> = {}): ConnectionProfile {
  return {
    local_id: 'conn-0123456789abcdef0123456789abcdef',
    name: 'Lab 7.4',
    endpoint_display: 'zabbix-lab.example.invalid/zabbix/',
    api_url_display: 'zabbix-lab.example.invalid/zabbix/api',
    plain_http: false,
    auth_method: 'api_token',
    secret_storage: 'os_store',
    username: null,
    trust: 'system',
    ca_ref: null,
    ca_fingerprints: [],
    last_check: { at: '2026-10-26T07:33:20Z', server_version: '7.4.16', result_code: null },
    state: 'not_connected',
    mode: null,
    context_generation: 0,
    default_password_warning_hidden: false,
    secret_in_memory: false,
    ...overrides
  };
}

const password = profile({
  local_id: 'conn-fedcba9876543210fedcba9876543210',
  name: 'Prod',
  auth_method: 'password',
  secret_storage: 'none',
  username: 'dash-editor'
});

function report(failedAt: 'auth' | null, params: Record<string, string> = {}): CheckReport {
  const steps: CheckReport['steps'] = [
    { step: 'endpoint', status: 'passed', code: null, params: {} },
    { step: 'api', status: 'passed', code: null, params: { version: '7.4.16' } },
    failedAt === 'auth'
      ? { step: 'auth', status: 'failed', code: 'auth_failed', params: { auth_method: 'password', ...params } }
      : { step: 'auth', status: 'passed', code: null, params: { auth_method: 'password' } },
    failedAt === 'auth'
      ? { step: 'identity', status: 'not_checked', code: 'previous_step_failed', params: {} }
      : { step: 'identity', status: 'notice', code: 'identity_first_seen', params: {} },
    failedAt === 'auth'
      ? { step: 'capabilities', status: 'not_checked', code: 'previous_step_failed', params: {} }
      : { step: 'capabilities', status: 'notice', code: 'api_method_denied', params: { methods: 'user.get,usergroup.get' } }
  ];
  return {
    steps,
    auth_method: 'password',
    server_version: '7.4.16',
    identity: failedAt ? null : 'identity_first_seen',
    read_access: [],
    checked_at: '2026-10-26T07:35:00Z',
    result_code: failedAt ? 'auth_failed' : null,
    passed: failedAt === null,
    default_password_warning: false
  };
}

/** In-memory client that records what it was sent; it never echoes a secret. */
function fakeClient(
  initial: ConnectionList,
  behaviour: { connect: (secret?: string) => ConnectOutcome | Error; test?: (secret?: string) => CheckReport }
) {
  const sent: unknown[] = [];
  const hidden: string[] = [];
  let list = initial;
  const client: ConnectionsClient = {
    async list() {
      return list;
    },
    async profile(localId) {
      const found = list.profiles.find((candidate) => candidate.local_id === localId);
      if (!found) throw new Error('missing');
      return found;
    },
    async previewAddress(address) {
      return { base_url: `${address}/`, api_url: `${address}/api`, plain_http: false };
    },
    async create(input, secret) {
      sent.push({ ...input, with_secret: secret !== undefined });
      return profile({ name: input.name });
    },
    async update(_localId, input, secret) {
      sent.push({ ...input, with_secret: secret !== undefined });
      return { profile: profile({ name: input.name }), context_changed: false };
    },
    async duplicate(_localId, name) {
      return profile({ name });
    },
    async remove() {
      return { local_credential: null };
    },
    async test(_localId, options): Promise<TestOutcome> {
      sent.push({ test: options?.secret !== undefined });
      return { report: behaviour.test?.(options?.secret) ?? report(null), profile: null };
    },
    async connect(_localId, options) {
      sent.push({ connect: options?.secret !== undefined });
      const outcome = behaviour.connect(options?.secret);
      if (outcome instanceof Error) throw outcome;
      if (outcome.connected && outcome.profile) {
        list = withProfile(list, outcome.profile);
      }
      return outcome;
    },
    async disconnect() {
      return { logout: null, profile: null };
    },
    async enableWrites(localId) {
      return { mode: 'read_write', profile: { ...list.profiles[0], local_id: localId, state: 'connected', mode: 'read_write' } };
    },
    async setReadOnly(localId) {
      return { mode: 'read_only', profile: { ...list.profiles[0], local_id: localId, state: 'connected', mode: 'read_only' } };
    },
    async forgetSecret() {},
    async hideDefaultPasswordWarning(localId) {
      hidden.push(localId);
      const found = list.profiles.find((candidate) => candidate.local_id === localId) ?? profile();
      return { ...found, default_password_warning_hidden: true };
    },
    async importTrustedCa() {
      return { ca_ref: `sha256-${'0'.repeat(64)}`, certificate_fingerprints: ['ab'] };
    }
  };
  return { client, sent, hidden };
}

const en = createI18n('en');
const ru = createI18n('ru');

function text(i18n: ReturnType<typeof createI18n>, value: LocalizedText): string {
  return resolveText(i18n, value);
}

describe('connection routes (P2-01G)', () => {
  it('adds editor and check routes with opaque IDs only', () => {
    expect(parseRoute('/connections/new').route).toEqual({ name: 'connection_new' });
    expect(parseRoute('/connections/conn-1/edit').route).toEqual({ name: 'connection_edit', localId: 'conn-1' });
    expect(parseRoute('/connections/conn-1/check').route).toEqual({ name: 'connection_check', localId: 'conn-1' });
    expect(parseRoute('/connections/conn-1').canonical).toBe('/connections');
    expect(parseRoute('/connections/zabbix.example.invalid%2Fx/edit').canonical).toBe('/connections');
    expect(topSection({ name: 'connection_check', localId: 'conn-1' })).toBe('connections');
  });
});

describe('connection codes and messages (P1-00F §3)', () => {
  // Every code the P2-01 backend (zabbix taxonomy, application connections, local server
  // routes) can return to the connection screens.
  const BACKEND_CODES = [
    'invalid_address', 'trust_material_invalid', 'dns_unresolvable', 'connection_refused',
    'host_unreachable_or_timeout', 'tls_untrusted', 'tls_certificate_time_invalid', 'tls_name_mismatch',
    'connection_failed', 'wrong_base_path', 'redirect', 'not_jsonrpc', 'unsupported_version', 'auth_failed',
    'token_not_authorized', 'token_expired', 'mfa_session_terminated', 'session_expired', 'system_access_denied',
    'api_method_denied', 'api_error', 'unexpected_response', 'identity_first_seen', 'server_version_changed',
    'server_identity_changed', 'not_connected', 'plain_http_token_storage_unconfirmed', 'profile_limit_reached',
    'profile_name_invalid', 'profile_not_found', 'profile_store_invalid', 'profile_store_linked_path',
    'profile_store_too_large', 'profile_store_unavailable', 'random_source_unavailable', 'secret_required',
    'trust_material_missing', 'username_invalid', 'writes_not_confirmed', 'credential_missing',
    'credential_locked', 'credential_unavailable', 'request_invalid', 'secret_invalid', 'connections_unavailable',
    'trust_upload_invalid'
  ];

  it('maps every backend code to its own message, never to the generic fallback', () => {
    for (const code of BACKEND_CODES) {
      expect(KNOWN_CODES, code).toContain(code);
      const message = codeText(code, { reason: 'syntax', version: '7.0.0', method: 'user.get', max_chars: '128', max: '256' });
      expect(message.id, code).not.toBe('errors.connection.unknown');
      for (const i18n of [en, ru]) {
        const rendered = text(i18n, message);
        expect(rendered, code).not.toMatch(/[{}]|—|undefined|NaN/);
      }
    }
    expect(codeText('something_new').id).toBe('errors.connection.unknown');
    expect(codeText('http.security.csrf_invalid').id).toBe('http.security.csrf_invalid');
  });

  it('covers every address and trust reason separately', () => {
    for (const reason of ['syntax', 'scheme', 'userinfo', 'query', 'fragment', 'host']) {
      expect(codeText('invalid_address', { reason }).id).toBe(`errors.address.${reason}`);
    }
    for (const reason of ['too_large', 'empty', 'invalid_pem', 'private_key', 'too_many_certificates', 'rejected', 'changed']) {
      expect(codeText('trust_material_invalid', { reason }).id).toBe(`errors.trust.${reason}`);
    }
  });

  it('uses every message of the connections catalog from the UI', () => {
    const sources = import.meta.glob<string>(['./*.ts', './*.svelte', '../shell/*.svelte', '../../App.svelte', '!./*.test.ts'], {
      query: '?raw',
      import: 'default',
      eager: true
    });
    const used = Object.values(sources).join(' ');
    const unused = Object.keys(connectionsCatalog).filter((id) => !used.includes(`'${id}'`));
    expect(unused).toEqual([]);
    for (const id of Object.keys(connectionsCatalog)) expect(catalogs.ru, id).toHaveProperty([id]);
  });

  it('explains every step of a failed and a passed check', () => {
    const failed = report('auth', { consecutive_failures: '2' });
    const translate = (id: MessageId): string => text(en, { id } as LocalizedText);
    expect(text(en, stepText(failed, failed.steps[2], translate))).toBe(
      'Sign-in failed. Check the credentials. For LDAP accounts, the directory may also be unavailable.'
    );
    expect(text(ru, stepText(failed, failed.steps[3], (id) => text(ru, { id } as LocalizedText)))).toBe(
      'Не проверялось: нужен успешный вход'
    );
    const passed = report(null);
    expect(text(en, stepText(passed, passed.steps[1], translate))).toBe('Zabbix API 7.4.16 found.');
    expect(text(en, stepText(passed, passed.steps[4], translate))).toContain('user.get, usergroup.get');
  });
});

describe('connection screen logic (P1-02A–E)', () => {
  it('offers the actions of the P1-02A table per state', () => {
    expect(cardActions(profile(), false)).toEqual(['edit', 'connect']);
    expect(cardActions(profile({ state: 'last_check_failed' }), false)).toEqual(['edit', 'check']);
    expect(cardActions(profile({ state: 'needs_sign_in' }), false)).toEqual(['edit', 'sign_in']);
    expect(cardActions(profile({ state: 'connected', mode: 'read_only' }), false)).toEqual(['enable_writes', 'edit', 'disconnect']);
    expect(cardActions(profile({ state: 'connected', mode: 'read_write' }), false)).toEqual(['set_read_only', 'edit', 'disconnect']);
    expect(cardActions(profile(), true)).toEqual(['cancel']);
  });

  it('keeps one active connection and derives the header label', () => {
    const list: ConnectionList = { profiles: [profile(), password], active: null };
    expect(headerConnection(list).mode).toBe('offline');
    const first = withProfile(list, { ...profile(), state: 'connected', mode: 'read_only' });
    expect(headerConnection(first)).toEqual({ mode: 'read_only', profileName: 'Lab 7.4', localId: profile().local_id });
    const second = withProfile(first, { ...password, state: 'connected', mode: 'read_only' });
    expect(second.profiles.filter((candidate) => candidate.state === 'connected')).toHaveLength(1);
    expect(headerConnection(second).profileName).toBe('Prod');
    const badge = stateBadge({ ...password, state: 'connected', mode: 'read_only' }, false, (id) => text(ru, { id } as LocalizedText));
    expect(text(ru, badge.label)).toBe('Подключено · только чтение');
  });

  it('validates the form, places server errors on fields and never sends a secret field', () => {
    const form = { ...emptyForm(), name: ' ', address: '', storeToken: true };
    const errors = validateForm(form, true);
    expect(Object.keys(errors).sort()).toEqual(['address', 'confirm_http', 'name']);
    expect(validateForm({ ...emptyForm(), name: 'Lab', address: 'x', trust: 'custom_ca' }, false).trust).toEqual({
      id: 'connection.form.custom_ca_required'
    });
    expect(fieldOfCode('invalid_address')).toBe('address');
    expect(fieldOfCode('trust_material_invalid')).toBe('trust');
    expect(fieldOfCode('profile_store_unavailable')).toBe('form');
    const input = inputFromForm({ ...emptyForm(), name: ' Lab ', address: ' zabbix.example.invalid ', method: 'password', username: 'editor' });
    expect(input).toEqual({
      name: 'Lab',
      address: 'zabbix.example.invalid',
      auth: { method: 'password', username: 'editor', store_password: false },
      trust: { kind: 'system' },
      confirm_plain_http_token_storage: false
    });
    expect(JSON.stringify(input)).not.toMatch(/"(secret|password|token)":/);
  });

  it('keeps the storage choice per sign-in method (O-037)', () => {
    const passwordForm = { ...emptyForm(), name: 'Lab', address: 'x', method: 'password' as const, username: 'editor' };
    // The token choice does not leak into a password profile and the other way round.
    expect(inputFromForm({ ...passwordForm, storeToken: true }).auth).toEqual({
      method: 'password',
      username: 'editor',
      store_password: false
    });
    // Storing a password for plain http needs the same confirmation as a token.
    expect(validateForm({ ...passwordForm, storePassword: true }, true).confirm_http).toBeDefined();
    expect(validateForm({ ...passwordForm, storePassword: true, confirmPlainHttp: true }, true).confirm_http).toBeUndefined();
    const stored = formFromProfile({ ...password, secret_storage: 'os_store' });
    expect(stored.storePassword).toBe(true);
    expect(stored.storeToken).toBe(false);
    expect(inputFromForm(stored).auth).toEqual({ method: 'password', username: 'dash-editor', store_password: true });
  });

  it('detects a server change only for an existing profile', () => {
    expect(serverChanged(null, 'a/')).toBe(false);
    expect(serverChanged(profile(), profile().endpoint_display)).toBe(false);
    expect(serverChanged(profile(), 'zabbix-new.example.invalid/')).toBe(true);
  });

  it('blocks sign-in for 30 s after the fifth consecutive failure', () => {
    expect(lockoutSeconds(report('auth', { consecutive_failures: '4' }))).toBe(0);
    expect(lockoutSeconds(report('auth', { consecutive_failures: '5' }))).toBe(30);
    expect(lockoutSeconds(null)).toBe(0);
  });

  it('copies a sanitized report with the address only on request', () => {
    const failed = report('auth', { consecutive_failures: '1' });
    const without = checkReportText(failed, password, false);
    expect(without).toContain('step auth: failed auth_failed');
    expect(without).toContain('failed_step: auth');
    expect(without).toContain('checked_at: 2026-10-26T07:35:00.000Z');
    expect(checkReportText({ ...failed, checked_at: 'garbage' }, password, false)).toContain('checked_at: unknown');
    expect(without).not.toContain(password.endpoint_display);
    expect(checkReportText(failed, password, true)).toContain(`address: ${password.endpoint_display}`);
  });

  it('truncates a duplicate name to the profile limit', () => {
    expect(duplicateName('Prod', (name) => en.t('connection.duplicate.name', { profile: name }))).toBe('Prod (copy)');
    expect([...duplicateName('x'.repeat(128), (name) => `${name} (copy)`)]).toHaveLength(128);
  });
});

describe('connection store: sign-in flow (P1-02C1)', () => {
  it('opens the dialog for a missing stored token, keeps it open on a rejected secret and holds no secret', async () => {
    const list: ConnectionList = { profiles: [profile()], active: null };
    let attempts = 0;
    const { client, sent } = fakeClient(list, {
      connect: (secret) => {
        attempts += 1;
        if (attempts === 1) {
          return { report: report('auth', { consecutive_failures: '1' }), connected: false, context_changed: false, credential_save: null, profile: null };
        }
        return {
          report: report(null),
          connected: true,
          context_changed: false,
          credential_save: null,
          profile: { ...profile(), state: 'connected', mode: 'read_only' }
        };
      }
    });
    // The stored token is missing: the server answers with the `credential_missing` code.
    const original = client.connect;
    client.connect = async (localId, options) => {
      if (options?.secret === undefined) {
        const { ApiFailure } = await import('../api');
        throw new ApiFailure(409, { code: 'credential_missing', params: {} });
      }
      return original(localId, options);
    };
    const store = new ConnectionsStore(client);
    await store.refresh();
    await store.connect(store.list!.profiles[0]);
    expect(store.signIn?.purpose).toBe('connect');

    const rejected = await store.submitSecret(store.signIn!, SECRET);
    expect(rejected.kind).toBe('rejected');
    expect(store.signIn).not.toBeNull();

    const accepted = await store.submitSecret(store.signIn!, SECRET);
    expect(accepted.kind).toBe('done');
    expect(store.signIn).toBeNull();
    expect(store.header.mode).toBe('read_only');
    expect(store.header.profileName).toBe('Lab 7.4');
    expect(JSON.stringify([store.list, store.reports, store.announcement, sent])).not.toContain(SECRET);
  });

  it('connects a profile with a stored password without the dialog (O-037)', async () => {
    const stored = { ...password, secret_storage: 'os_store' as const };
    const { client, sent } = fakeClient(
      { profiles: [stored], active: null },
      {
        connect: () => ({
          report: report(null),
          connected: true,
          context_changed: false,
          credential_save: null,
          profile: { ...stored, state: 'connected', mode: 'read_only' }
        })
      }
    );
    const store = new ConnectionsStore(client);
    await store.refresh();
    await store.connect(stored);
    expect(store.signIn).toBeNull();
    expect(sent).toEqual([{ connect: false }]);
    expect(store.header.profileName).toBe('Prod');
  });

  it('uses a secret kept in memory without the dialog and falls back to it on rejection', async () => {
    const remembered = { ...password, secret_in_memory: true };
    let attempts = 0;
    const { client, sent } = fakeClient(
      { profiles: [remembered], active: null },
      {
        connect: () => new Error('unused'),
        test: () => {
          attempts += 1;
          return attempts === 1 ? report(null) : report('auth', { consecutive_failures: '1' });
        }
      }
    );
    const store = new ConnectionsStore(client);
    await store.refresh();
    await store.test(remembered);
    expect(store.signIn).toBeNull();
    expect(sent).toEqual([{ test: false }]);
    // The server rejected the kept secret (and dropped it): the dialog asks again.
    await store.test(remembered);
    expect(store.signIn?.purpose).toBe('test');
    // Without any kept or stored secret the dialog opens before any request.
    store.closeSignIn();
    await store.test(password);
    expect(store.signIn?.purpose).toBe('test');
    expect(sent).toHaveLength(2);
  });

  it('warns once per run about the default password and hides it per profile on request', async () => {
    const flagged = { ...report(null), default_password_warning: true };
    const { client, hidden } = fakeClient(
      { profiles: [password], active: null },
      { connect: () => new Error('unused'), test: () => flagged }
    );
    const store = new ConnectionsStore(client);
    await store.refresh();
    const remembered = { ...password, secret_in_memory: true };
    await store.test(remembered);
    expect(store.defaultPasswordWarning?.local_id).toBe(password.local_id);
    await store.dismissDefaultPasswordWarning(false);
    expect(store.defaultPasswordWarning).toBeNull();
    expect(hidden).toEqual([]);
    await store.test(remembered);
    expect(store.defaultPasswordWarning).toBeNull();

    const second = new ConnectionsStore(client);
    await second.refresh();
    await second.test(remembered);
    await second.dismissDefaultPasswordWarning(true);
    expect(hidden).toEqual([password.local_id]);
    expect(second.profile(password.local_id)?.default_password_warning_hidden).toBe(true);
  });

  it('asks a password profile for its password before any request', async () => {
    const { client, sent } = fakeClient({ profiles: [password], active: null }, { connect: () => new Error('unused') });
    const store = new ConnectionsStore(client);
    await store.refresh();
    await store.connect(password);
    expect(store.signIn).toEqual({ profile: password, purpose: 'connect', acceptNewIdentity: false });
    expect(sent).toEqual([]);
  });
});

describe('connection screens render (SSR)', () => {
  function storeWith(list: ConnectionList | null, reports: Record<string, CheckReport> = {}): ConnectionsStore {
    const { client } = fakeClient(list ?? { profiles: [], active: null }, { connect: () => new Error('unused') });
    const store = new ConnectionsStore(client);
    store.list = list;
    store.reports = reports;
    return store;
  }

  it('lists profiles with state, address, meta line and HTTP warning, without secrets', () => {
    const list: ConnectionList = {
      profiles: [
        { ...profile(), state: 'connected', mode: 'read_only' },
        password,
        profile({ local_id: 'conn-11111111111111111111111111111111', name: 'Pilot', plain_http: true, state: 'last_check_failed', last_check: { at: '2026-10-26T07:33:20Z', server_version: null, result_code: 'host_unreachable_or_timeout' } })
      ],
      active: { local_id: profile().local_id, mode: 'read_only', context_generation: 0 }
    };
    const { body } = render(ConnectionsScreen, { props: { i18n: ru, store: storeWith(list), navigate: () => {} } });
    expect(body).toContain('<h1>Подключения</h1>');
    expect(body).toContain('Подключено · только чтение');
    expect(body).toContain('Последняя проверка: ошибка');
    expect(body).toContain('Сервер недоступен или не ответил вовремя.');
    expect(body).toContain('HTTP без шифрования');
    expect(body).toContain('API-токен в системном хранилище');
    expect(body).toContain('имя пользователя и пароль, пароль только в памяти');
    expect(body).not.toContain('имя пользователя и пароль в системном хранилище');
    expect(body).toContain('Разрешить запись панелей');
    expect(body).toContain('aria-describedby="connection-conn-fedcba9876543210fedcba9876543210-title"');
    expect(body).toContain('role="status"');
    expect(body).not.toContain(SECRET);
  });

  it('shows the empty state with offline continuation', () => {
    const { body } = render(ConnectionsScreen, { props: { i18n: en, store: storeWith({ profiles: [], active: null }), navigate: () => {} } });
    expect(body).toContain('No connections yet');
    expect(body).toContain('Continue without a connection');
    expect(body).toContain('href="/connections/new"');
  });

  it('renders the editor with address rules, sign-in method and trust, no TLS bypass', () => {
    const { body } = render(ProfileEditor, { props: { i18n: ru, store: storeWith({ profiles: [], active: null }), localId: null, navigate: () => {} } });
    expect(body).toContain('Новое подключение');
    expect(body).toContain('Адрес Zabbix');
    expect(body).toContain('Как в адресной строке браузера, с портом и путём');
    expect(body).toContain('Сохранить токен в системном хранилище');
    expect(body).toContain('Отключить проверку сертификата нельзя.');
    // O-037: the token is typed in the form, in a masked field the browser must not store.
    expect(body).toMatch(/id="connection-token"[^>]*type="password"[^>]*autocomplete="off"/);
    expect(body).toContain('Без сохранения токен хранится только в памяти программы до отключения или завершения сеанса');
    expect(body).toMatch(/id="connection-token"[^>]*value=""/);
  });

  it('renders the password under the user name with storage, default account and forget (O-037)', () => {
    const stored = { ...password, secret_storage: 'os_store' as const };
    const { body } = render(ProfileEditor, {
      props: { i18n: ru, store: storeWith({ profiles: [stored], active: null }), localId: stored.local_id, navigate: () => {} }
    });
    const username = body.indexOf('id="connection-username"');
    const passwordField = body.indexOf('id="connection-password"');
    expect(username).toBeGreaterThan(0);
    expect(passwordField).toBeGreaterThan(username);
    expect(body).toMatch(/id="connection-password"[^>]*type="password"[^>]*autocomplete="off"/);
    expect(body).toContain('Подставить учётную запись по умолчанию');
    expect(body).toContain('Сохранить пароль в системном хранилище');
    expect(body).toContain('API-токен безопаснее');
    expect(body).toContain('Пароль хранится в системном хранилище. Оставьте поле пустым, чтобы не менять его.');
    expect(body).toContain('Забыть сохранённый пароль');
    expect(body).not.toContain('Сохранить токен в системном хранилище');
  });

  it('renders the default-password warning with «do not show again»', () => {
    const store = storeWith({ profiles: [password], active: null });
    const { body } = render(DefaultPasswordDialog, { props: { i18n: ru, store, profile: password } });
    expect(body).toContain('Используется пароль по умолчанию');
    expect(body).toContain('Подключение «Prod» выполнило вход с паролем Zabbix по умолчанию. Смените его на рабочем сервере.');
    expect(body).toContain('Больше не показывать для этого подключения');
    expect(body).not.toContain('zabbix<');
  });

  it('renders step results, technical details and the copy action', () => {
    const failed = report('auth', { consecutive_failures: '1' });
    const store = storeWith({ profiles: [password], active: null }, { [password.local_id]: failed });
    const { body } = render(ConnectionCheck, { props: { i18n: ru, store, localId: password.local_id, navigate: () => {} } });
    expect(body).toContain('Проверка подключения «Prod»');
    expect(body).toContain('1. Адрес и TLS');
    expect(body).toContain('Проверка остановлена на шаге «Вход».');
    expect(body).toContain('Изменить данные входа');
    expect(body).toContain('Не проверялось: нужен успешный вход');
    expect(body).toContain('auth_failed');
    expect(body).toContain('Копировать отчёт');
    expect(body).toContain('Включить адрес сервера в отчёт');
  });

  it('renders the sign-in dialog with a masked field that the browser must not store', () => {
    const store = storeWith({ profiles: [password], active: null });
    const { body } = render(SignInDialog, { props: { i18n: ru, store, request: { profile: password, purpose: 'connect', acceptNewIdentity: false } } });
    expect(body).toContain('Вход в «Prod»');
    expect(body).toContain('Имя пользователя: dash-editor');
    expect(body).toMatch(/type="password"[^>]*autocomplete="off"|autocomplete="off"[^>]*type="password"/);
    expect(body).toContain('Войти');
  });

  it('wires the routes into the shell', () => {
    const { client } = fakeClient({ profiles: [], active: null }, { connect: () => new Error('unused') });
    for (const path of ['/connections', '/connections/new']) {
      const { body } = render(App, { props: { initialPath: path, locale: 'en', connectionsClient: client } });
      expect(body).toContain('Offline');
      expect(body).toContain('aria-current="page"');
    }
  });
});
