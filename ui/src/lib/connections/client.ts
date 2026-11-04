// Typed client of the local connection routes (P2-01G over P1-02H). The browser never talks to
// Zabbix: every command goes to the local server, which answers with codes and safe parameters.
// A secret (password or API token) is sent once in a POST/PUT body (sign-in, check, or a profile
// that stores it, O-037) and is never returned, stored in browser storage, put into a URL or logged.

import { ApiFailure, apiFailure } from '../api';
import { CSRF_HEADER, loadCsrfToken } from '../session';

export type AuthMethod = 'api_token' | 'password';
export type SecretStorage = 'none' | 'os_store';
export type TrustKind = 'system' | 'custom_ca';
export type ProfileState = 'not_connected' | 'connected' | 'last_check_failed' | 'needs_sign_in';
export type AccessMode = 'read_only' | 'read_write';
export type CheckStepName = 'endpoint' | 'api' | 'auth' | 'identity' | 'capabilities';
export type StepStatus = 'passed' | 'notice' | 'failed' | 'not_checked';

export interface LastCheck {
  /** RFC 3339 UTC (O-035); parse with `parseTimestamp`. */
  readonly at: string | null;
  readonly server_version: string | null;
  readonly result_code: string | null;
}

/** Non-secret profile data (P1-02H). */
export interface ConnectionProfile {
  readonly local_id: string;
  /** User data. */
  readonly name: string;
  readonly endpoint_display: string;
  readonly api_url_display: string;
  readonly plain_http: boolean;
  readonly auth_method: AuthMethod;
  readonly secret_storage: SecretStorage;
  /** User data; only for the password method. */
  readonly username: string | null;
  readonly trust: TrustKind;
  /** Reference of the stored CA bundle (`sha256-…`), not a secret. */
  readonly ca_ref: string | null;
  readonly ca_fingerprints: ReadonlyArray<string>;
  readonly last_check: LastCheck | null;
  readonly state: ProfileState;
  readonly mode: AccessMode | null;
  readonly context_generation: number;
  /** «Do not show again for this connection» of the default-password warning (O-037). */
  readonly default_password_warning_hidden: boolean;
  /** A secret of a profile that does not store it is kept in the local process memory (O-037). */
  readonly secret_in_memory: boolean;
}

export interface ActiveConnection {
  readonly local_id: string;
  readonly mode: AccessMode;
  readonly context_generation: number;
}

export interface ConnectionList {
  readonly profiles: ReadonlyArray<ConnectionProfile>;
  readonly active: ActiveConnection | null;
}

export interface CheckStepReport {
  readonly step: CheckStepName;
  readonly status: StepStatus;
  /** Failure or notice code; for `not_checked` the reason code. */
  readonly code: string | null;
  readonly params: Readonly<Record<string, string>>;
}

export interface CheckReport {
  readonly steps: ReadonlyArray<CheckStepReport>;
  readonly auth_method: AuthMethod;
  readonly server_version: string | null;
  readonly identity: string | null;
  readonly read_access: ReadonlyArray<{ readonly method: string; readonly access: 'available' | 'denied' }>;
  /** RFC 3339 UTC (O-035); parse with `parseTimestamp`. */
  readonly checked_at: string | null;
  readonly result_code: string | null;
  readonly passed: boolean;
  /** Sign-in passed with the public Zabbix default account (O-037); the values never travel back. */
  readonly default_password_warning: boolean;
}

export interface ConnectOutcome {
  readonly report: CheckReport;
  readonly connected: boolean;
  readonly context_changed: boolean;
  readonly credential_save: string | null;
  readonly profile: ConnectionProfile | null;
}

export interface TestOutcome {
  readonly report: CheckReport;
  readonly profile: ConnectionProfile | null;
}

export interface ProfileInput {
  readonly name: string;
  readonly address: string;
  readonly auth:
    | { readonly method: 'api_token'; readonly store_token: boolean }
    | { readonly method: 'password'; readonly username: string; readonly store_password: boolean };
  readonly trust: { readonly kind: 'system' } | { readonly kind: 'custom_ca'; readonly ca_ref: string };
  readonly confirm_plain_http_token_storage: boolean;
}

export interface AddressPreview {
  readonly base_url: string;
  readonly api_url: string;
  readonly plain_http: boolean;
}

export interface TrustedCa {
  readonly ca_ref: string;
  readonly certificate_fingerprints: ReadonlyArray<string>;
}

export interface ConnectOptions {
  /** Password or API token typed in the sign-in dialog; omitted when a token is stored. */
  readonly secret?: string;
  readonly acceptNewIdentity?: boolean;
  readonly signal?: AbortSignal;
}

export interface ConnectionsClient {
  list(): Promise<ConnectionList>;
  profile(localId: string): Promise<ConnectionProfile>;
  previewAddress(address: string, signal?: AbortSignal): Promise<AddressPreview>;
  /**
   * `secret`: the password or token typed in the form. The local process stores it in the system
   * credential store when the profile says so, otherwise keeps it only in memory (O-037).
   */
  create(input: ProfileInput, secret?: string): Promise<ConnectionProfile>;
  update(
    localId: string,
    input: ProfileInput,
    secret?: string
  ): Promise<{ profile: ConnectionProfile; context_changed: boolean }>;
  duplicate(localId: string, name: string): Promise<ConnectionProfile>;
  remove(localId: string): Promise<{ local_credential: string | null }>;
  test(localId: string, options?: ConnectOptions): Promise<TestOutcome>;
  connect(localId: string, options?: ConnectOptions): Promise<ConnectOutcome>;
  disconnect(localId: string): Promise<{ logout: string | null; profile: ConnectionProfile | null }>;
  enableWrites(localId: string): Promise<{ mode: AccessMode; profile: ConnectionProfile | null }>;
  setReadOnly(localId: string): Promise<{ mode: AccessMode; profile: ConnectionProfile | null }>;
  /** Removes the stored password or token from this computer (`credential.forget`). */
  forgetSecret(localId: string): Promise<void>;
  hideDefaultPasswordWarning(localId: string): Promise<ConnectionProfile>;
  importTrustedCa(file: File): Promise<TrustedCa>;
}

const BASE = '/api/connections';

function profilePath(localId: string, action?: string): string {
  const path = `${BASE}/${encodeURIComponent(localId)}`;
  return action === undefined ? path : `${path}/${action}`;
}

/** Client over the local server. The CSRF token is read from `/api/session` and kept in memory only. */
export function createHttpConnectionsClient(fetcher: typeof fetch = (...args) => fetch(...args)): ConnectionsClient {
  let csrf: Promise<string> | null = null;

  /** The cached CSRF token; a failed load is not cached, so the next command loads it again. */
  function ensureCsrf(): Promise<string> {
    csrf ??= loadCsrfToken(fetcher).catch((error: unknown) => {
      csrf = null;
      throw error;
    });
    return csrf;
  }

  async function send<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
    const headers: Record<string, string> = { Accept: 'application/json' };
    const init: RequestInit = { credentials: 'same-origin', method, headers, signal };
    if (method !== 'GET') {
      headers['Content-Type'] = 'application/json';
      headers[CSRF_HEADER] = await ensureCsrf();
      init.body = JSON.stringify(body ?? {});
    }
    const response = await fetcher(path, init);
    if (!response.ok) throw await apiFailure(response);
    return (await response.json()) as T;
  }

  async function uploadPem(file: File): Promise<string> {
    const token = await ensureCsrf();
    const set = await send<{ id: string }>('POST', '/api/upload-sets');
    const response = await fetcher(`/api/uploads/${encodeURIComponent(set.id)}/files/certificate.pem`, {
      body: file,
      credentials: 'same-origin',
      headers: { Accept: 'application/json', 'Content-Type': 'application/octet-stream', [CSRF_HEADER]: token },
      method: 'POST'
    });
    if (!response.ok) throw await apiFailure(response);
    return set.id;
  }

  return {
    list: () => send('GET', BASE),
    profile: (localId) => send('GET', profilePath(localId)),
    previewAddress: (address, signal) => send('POST', `${BASE}/address-preview`, { address }, signal),
    create: (input, secret) => send('POST', BASE, { ...input, ...secretBody(secret) }),
    update: (localId, input, secret) => send('PUT', profilePath(localId), { ...input, ...secretBody(secret) }),
    duplicate: (localId, name) => send('POST', profilePath(localId, 'duplicate'), { name }),
    remove: (localId) => send('DELETE', profilePath(localId)),
    test: (localId, options = {}) =>
      send('POST', profilePath(localId, 'test'), secretBody(options.secret), options.signal),
    connect: (localId, options = {}) =>
      send(
        'POST',
        profilePath(localId, 'connect'),
        { ...secretBody(options.secret), accept_new_identity: options.acceptNewIdentity === true },
        options.signal
      ),
    disconnect: (localId) => send('POST', profilePath(localId, 'disconnect')),
    enableWrites: (localId) => send('POST', profilePath(localId, 'enable-writes'), { confirmed: true }),
    setReadOnly: (localId) => send('POST', profilePath(localId, 'read-only')),
    forgetSecret: async (localId) => {
      await send('POST', profilePath(localId, 'forget-secret'));
    },
    hideDefaultPasswordWarning: (localId) => send('POST', profilePath(localId, 'hide-default-password-warning')),
    importTrustedCa: async (file) => {
      const uploadSetId = await uploadPem(file);
      return send('POST', `${BASE}/trusted-ca`, { upload_set_id: uploadSetId });
    }
  };
}

function secretBody(secret: string | undefined): { secret?: string } {
  return secret === undefined || secret === '' ? {} : { secret };
}

/** Machine code and safe parameters of a failed command. */
export interface CommandFailure {
  readonly code: string;
  readonly params: Readonly<Record<string, string>>;
  readonly status: number;
}

/** Reduce any thrown value to a code; a network failure or abort has no server code. */
export function failureOf(error: unknown): CommandFailure {
  if (error instanceof ApiFailure && error.response) {
    return { code: error.response.code, params: error.response.params ?? {}, status: error.status };
  }
  if (error instanceof DOMException && error.name === 'AbortError') {
    return { code: 'aborted', params: {}, status: 0 };
  }
  if (error instanceof ApiFailure) return { code: 'backend_unavailable', params: {}, status: error.status };
  return { code: 'backend_unavailable', params: {}, status: 0 };
}
