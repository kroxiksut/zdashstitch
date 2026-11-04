// Machine codes of the connection backend → message IDs (P1-00F §3, P1-02D). The backend never
// sends prose; every code it can return has a message here, and unknown codes fall back to
// `errors.connection.unknown` with the code as a technical value.

import type { LocalizedText } from '../components';
import { catalogs, type MessageId } from '../i18n';
import { placeholderTypes } from '../i18n/generated';
import type { AccessMode, CheckReport, CheckStepName, CheckStepReport, ConnectionProfile, StepStatus } from './client';

type Params = Readonly<Record<string, string>>;

function count(params: Params, name: string): number {
  const value = Number(params[name]);
  return Number.isFinite(value) ? value : Number.NaN;
}

const ADDRESS_REASONS = {
  syntax: 'errors.address.syntax',
  scheme: 'errors.address.scheme',
  userinfo: 'errors.address.userinfo',
  query: 'errors.address.query',
  fragment: 'errors.address.fragment',
  host: 'errors.address.host'
} as const;

const TRUST_REASONS = {
  too_large: 'errors.trust.too_large',
  empty: 'errors.trust.empty',
  invalid_pem: 'errors.trust.invalid_pem',
  private_key: 'errors.trust.private_key',
  too_many_certificates: 'errors.trust.too_many_certificates',
  rejected: 'errors.trust.rejected',
  changed: 'errors.trust.changed'
} as const;

/** Codes without parameters that map to one message each. */
const SIMPLE_CODES = {
  dns_unresolvable: 'errors.connection.dns_unresolvable',
  connection_refused: 'errors.connection.refused',
  host_unreachable_or_timeout: 'errors.connection.unreachable_or_timeout',
  connection_failed: 'errors.connection.failed',
  wrong_base_path: 'errors.connection.api_not_found',
  redirect: 'errors.connection.api_not_found',
  not_jsonrpc: 'errors.connection.api_not_found',
  unexpected_response: 'errors.connection.unexpected_response',
  tls_untrusted: 'errors.tls.untrusted',
  tls_certificate_time_invalid: 'errors.tls.time_invalid',
  tls_name_mismatch: 'errors.tls.name_mismatch',
  auth_failed: 'errors.auth.credentials_or_directory',
  token_not_authorized: 'errors.auth.token_invalid_or_disabled',
  token_expired: 'errors.auth.token_expired',
  mfa_session_terminated: 'errors.auth.mfa_token_required',
  session_expired: 'errors.auth.session_expired',
  system_access_denied: 'errors.permission.system_access',
  server_identity_changed: 'connection.identity.changed',
  identity_first_seen: 'connection.identity.first_seen',
  trust_material_missing: 'errors.trust.missing',
  trust_upload_invalid: 'errors.trust.upload_invalid',
  profile_not_found: 'errors.profile.not_found',
  profile_store_invalid: 'errors.profile.store_invalid',
  profile_store_linked_path: 'errors.profile.store_linked_path',
  profile_store_too_large: 'errors.profile.store_too_large',
  profile_store_unavailable: 'errors.profile.store_unavailable',
  plain_http_token_storage_unconfirmed: 'errors.profile.plain_http_unconfirmed',
  not_connected: 'errors.connection.not_connected',
  secret_required: 'errors.connection.secret_required',
  secret_invalid: 'errors.connection.secret_invalid',
  writes_not_confirmed: 'errors.connection.writes_not_confirmed',
  random_source_unavailable: 'errors.connection.random_unavailable',
  request_invalid: 'errors.connection.request_invalid',
  connections_unavailable: 'errors.connection.service_unavailable',
  credential_missing: 'errors.credential.missing',
  credential_locked: 'errors.credential.locked',
  credential_unavailable: 'errors.credential.unavailable',
  backend_unavailable: 'startup.error.unavailable'
} as const satisfies Record<string, MessageId>;

/** Every code the P2-01 backend and the local server can return for connection commands. */
export const KNOWN_CODES: ReadonlyArray<string> = [
  ...Object.keys(SIMPLE_CODES),
  'invalid_address',
  'trust_material_invalid',
  'unsupported_version',
  'api_method_denied',
  'api_error',
  'server_version_changed',
  'profile_name_invalid',
  'username_invalid',
  'profile_limit_reached'
];

function isMessageId(code: string): code is MessageId {
  return Object.prototype.hasOwnProperty.call(catalogs.en, code);
}

/** User-facing text of a failure or notice code. */
export function codeText(code: string, params: Params = {}): LocalizedText {
  if (code in SIMPLE_CODES) return { id: SIMPLE_CODES[code as keyof typeof SIMPLE_CODES] } as LocalizedText;
  switch (code) {
    case 'invalid_address': {
      const reason = params.reason as keyof typeof ADDRESS_REASONS;
      return { id: ADDRESS_REASONS[reason] ?? 'errors.address.syntax' };
    }
    case 'trust_material_invalid': {
      const reason = params.reason as keyof typeof TRUST_REASONS;
      return { id: TRUST_REASONS[reason] ?? 'errors.trust.invalid_pem' };
    }
    case 'unsupported_version':
      return { id: 'errors.connection.version_unsupported', params: { version: params.version ?? '' } };
    case 'api_method_denied':
      return { id: 'errors.permission.api_method', params: { method: params.method ?? '' } };
    case 'api_error':
      return { id: 'errors.connection.api_error', params: { method: params.method ?? '' } };
    case 'server_version_changed':
      return {
        id: 'connection.identity.version_changed',
        params: { previous_version: params.previous_version ?? '', version: params.version ?? '' }
      };
    case 'profile_name_invalid':
      return { id: 'errors.profile.name_invalid', params: { max_chars: count(params, 'max_chars') } };
    case 'username_invalid':
      return { id: 'errors.profile.username_invalid', params: { max_chars: count(params, 'max_chars') } };
    case 'profile_limit_reached':
      return { id: 'errors.profile.limit_reached', params: { max: count(params, 'max') } };
    default:
      // HTTP boundary errors (`http.security.*`, `http.transfer.*`) are message IDs already.
      if (isMessageId(code) && Object.keys(placeholderTypes[code]).length === 0) {
        return { id: code } as LocalizedText;
      }
      return { id: 'errors.connection.unknown', params: { code } };
  }
}

export const STEP_ORDER: ReadonlyArray<CheckStepName> = ['endpoint', 'api', 'auth', 'identity', 'capabilities'];

export const STEP_LABEL: Readonly<Record<CheckStepName, LocalizedText>> = {
  endpoint: { id: 'connection.test.step.endpoint' },
  api: { id: 'connection.test.step.api' },
  auth: { id: 'connection.test.step.auth' },
  identity: { id: 'connection.test.step.identity' },
  capabilities: { id: 'connection.test.step.capabilities' }
};

export const STEP_STATUS_LABEL: Readonly<Record<StepStatus, LocalizedText>> = {
  passed: { id: 'connection.test.status.passed' },
  notice: { id: 'connection.test.status.notice' },
  failed: { id: 'connection.test.status.failed' },
  not_checked: { id: 'connection.test.status.not_checked' }
};

export const STEP_ICON: Readonly<Record<StepStatus, 'circle-check' | 'info-circle' | 'circle-x' | 'circle-dashed'>> = {
  passed: 'circle-check',
  notice: 'info-circle',
  failed: 'circle-x',
  not_checked: 'circle-dashed'
};

const NOT_CHECKED_REASON: Readonly<Record<CheckStepName, MessageId>> = {
  endpoint: 'connection.test.reason.endpoint',
  api: 'connection.test.reason.api',
  auth: 'connection.test.reason.auth',
  identity: 'connection.test.reason.identity',
  capabilities: 'connection.test.reason.identity'
};

/** Explanation shown for one step of a report (P1-02D table). */
export function stepText(report: CheckReport, step: CheckStepReport, translate: (id: MessageId) => string): LocalizedText {
  if (step.status === 'not_checked') {
    const failed = report.steps.find((candidate) => candidate.status === 'failed');
    return { id: 'connection.test.not_checked', params: { reason: translate(NOT_CHECKED_REASON[failed?.step ?? 'endpoint']) } };
  }
  if (step.status === 'failed') {
    if (step.step === 'auth' && step.params.during === 'sign_in') return { id: 'connection.test.during_sign_in' };
    return codeText(step.code ?? 'unknown', step.params);
  }
  if (step.status === 'notice') {
    if (step.code === 'api_method_denied') {
      return { id: 'connection.test.capabilities_denied', params: { methods: (step.params.methods ?? '').split(',').join(', ') } };
    }
    if (step.code === 'server_identity_changed') return { id: 'connection.identity.accepted' };
    return codeText(step.code ?? 'unknown', step.params);
  }
  switch (step.step) {
    case 'endpoint':
      return { id: 'connection.test.result.endpoint' };
    case 'api':
      return { id: 'connection.test.result.api', params: { version: step.params.version ?? report.server_version ?? '' } };
    case 'auth':
      return report.auth_method === 'api_token'
        ? { id: 'connection.test.result.auth_token' }
        : { id: 'connection.test.result.auth_password' };
    case 'identity':
      return { id: 'connection.test.result.identity_matches' };
    case 'capabilities':
      return { id: 'connection.test.result.capabilities' };
  }
}

export const MODE_WORD: Readonly<Record<AccessMode, MessageId>> = {
  read_only: 'connection.mode.read_only',
  read_write: 'connection.mode.read_write'
};

export type CardTone = 'success' | 'info' | 'warning' | 'danger';

/** Badge of a profile card (P1-02A table). `connecting` is a UI-only state. */
export function stateBadge(
  profile: ConnectionProfile,
  connecting: boolean,
  translate: (id: MessageId) => string
): { label: LocalizedText; tone: CardTone } {
  if (connecting) return { label: { id: 'connection.state.connecting' }, tone: 'info' };
  switch (profile.state) {
    case 'connected':
      return {
        label: { id: 'connection.state.connected_mode', params: { mode: translate(MODE_WORD[profile.mode ?? 'read_only']) } },
        tone: 'success'
      };
    case 'needs_sign_in':
      return { label: { id: 'connection.state.needs_sign_in' }, tone: 'warning' };
    case 'last_check_failed':
      return { label: { id: 'connection.state.last_check_failed' }, tone: 'warning' };
    case 'not_connected':
      return { label: { id: 'connection.state.not_connected' }, tone: 'info' };
  }
}

/** «Where the secret lives» part of the card meta line (P1-02A). */
export function authSummary(profile: Pick<ConnectionProfile, 'auth_method' | 'secret_storage'>): LocalizedText {
  if (profile.auth_method === 'password') {
    return profile.secret_storage === 'os_store'
      ? { id: 'connection.card.auth.password_os_store' }
      : { id: 'connection.card.auth.password' };
  }
  return profile.secret_storage === 'os_store'
    ? { id: 'connection.card.auth.token_os_store' }
    : { id: 'connection.card.auth.token_memory' };
}
