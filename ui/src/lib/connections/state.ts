// Screen logic of «Connections» (P1-02A–E) without DOM: card actions, the profile form model,
// error placement, the sign-in lockout, and the sanitized check report. Kept pure for tests.

import type { LocalizedText } from '../components';
import { parseTimestamp } from '../timestamp';
import type {
  AuthMethod,
  CheckReport,
  CheckStepName,
  CommandFailure,
  ConnectionList,
  ConnectionProfile,
  ProfileInput
} from './client';
import { codeText } from './messages';

export const MAX_PROFILE_NAME_CHARS = 128;
/** Zabbix blocks password sign-in for 30 s after five consecutive failures (P1-02C1). */
export const SIGN_IN_ATTEMPTS = 5;
export const SIGN_IN_BLOCK_SECONDS = 30;
/** After this delay the sign-in dialog explains the wait (LDAP can be slow, P1-02C1). */
export const SIGN_IN_WAITING_AFTER_MS = 4000;

export type CardAction = 'connect' | 'sign_in' | 'disconnect' | 'check' | 'edit' | 'cancel' | 'enable_writes' | 'set_read_only';

/** Actions of a profile card per state (P1-02A table; P1-02E for the mode switch). */
export function cardActions(profile: ConnectionProfile, connecting: boolean): CardAction[] {
  if (connecting) return ['cancel'];
  switch (profile.state) {
    case 'connected':
      return [profile.mode === 'read_write' ? 'set_read_only' : 'enable_writes', 'edit', 'disconnect'];
    case 'needs_sign_in':
      return ['edit', 'sign_in'];
    case 'last_check_failed':
      return ['edit', 'check'];
    case 'not_connected':
      return ['edit', 'connect'];
  }
}

export interface HeaderConnection {
  readonly mode: 'offline' | 'read_only' | 'read_write';
  /** User data: the active profile name. */
  readonly profileName: string | null;
  readonly localId: string | null;
}

export const OFFLINE: HeaderConnection = { mode: 'offline', profileName: null, localId: null };

/** Header label source: the one active profile, or «Offline». */
export function headerConnection(list: ConnectionList | null): HeaderConnection {
  const active = list?.active;
  if (!active) return OFFLINE;
  const profile = list.profiles.find((candidate) => candidate.local_id === active.local_id);
  return { mode: active.mode, profileName: profile?.name ?? null, localId: active.local_id };
}

/** Replace one profile in a list, keeping the active marker consistent with the profile state. */
export function withProfile(list: ConnectionList, profile: ConnectionProfile): ConnectionList {
  const profiles = list.profiles.some((candidate) => candidate.local_id === profile.local_id)
    ? list.profiles.map((candidate) => (candidate.local_id === profile.local_id ? profile : candidate))
    : [...list.profiles, profile];
  let active = list.active;
  if (profile.state === 'connected' && profile.mode) {
    active = { local_id: profile.local_id, mode: profile.mode, context_generation: profile.context_generation };
  } else if (active?.local_id === profile.local_id) {
    active = null;
  }
  // Only one profile is connected at a time: connecting another one closed the previous session.
  const others = profiles.map((candidate) =>
    active && candidate.local_id !== active.local_id && candidate.state === 'connected'
      ? { ...candidate, state: 'not_connected' as const, mode: null }
      : candidate
  );
  return { profiles: others, active };
}

export function withoutProfile(list: ConnectionList, localId: string): ConnectionList {
  return {
    profiles: list.profiles.filter((candidate) => candidate.local_id !== localId),
    active: list.active?.local_id === localId ? null : list.active
  };
}

// ---------------------------------------------------------------- profile form

export interface ProfileForm {
  name: string;
  address: string;
  method: AuthMethod;
  storeToken: boolean;
  /** Keep the password in the system credential store (O-037). */
  storePassword: boolean;
  confirmPlainHttp: boolean;
  username: string;
  trust: 'system' | 'custom_ca';
  caRef: string | null;
  caFingerprints: ReadonlyArray<string>;
}

export function emptyForm(): ProfileForm {
  return {
    name: '',
    address: '',
    method: 'api_token',
    storeToken: false,
    storePassword: false,
    confirmPlainHttp: false,
    username: '',
    trust: 'system',
    caRef: null,
    caFingerprints: []
  };
}

export function formFromProfile(profile: ConnectionProfile): ProfileForm {
  return {
    name: profile.name,
    address: profile.endpoint_display,
    method: profile.auth_method,
    storeToken: profile.auth_method === 'api_token' && profile.secret_storage === 'os_store',
    storePassword: profile.auth_method === 'password' && profile.secret_storage === 'os_store',
    confirmPlainHttp: profile.plain_http && profile.secret_storage === 'os_store',
    username: profile.username ?? '',
    trust: profile.trust,
    caRef: profile.ca_ref,
    caFingerprints: profile.ca_fingerprints
  };
}

export function inputFromForm(form: ProfileForm): ProfileInput {
  const caRef = form.caRef;
  return {
    name: form.name.trim(),
    address: form.address.trim(),
    auth:
      form.method === 'password'
        ? { method: 'password', username: form.username, store_password: form.storePassword }
        : { method: 'api_token', store_token: form.storeToken },
    trust: form.trust === 'custom_ca' && caRef ? { kind: 'custom_ca', ca_ref: caRef } : { kind: 'system' },
    confirm_plain_http_token_storage: form.confirmPlainHttp
  };
}

export type FormField = 'name' | 'address' | 'username' | 'confirm_http' | 'trust';
export type FormErrors = Partial<Record<FormField | 'form', LocalizedText>>;

/** The chosen sign-in method keeps its secret in the system credential store (O-037). */
export function storesSecret(form: ProfileForm): boolean {
  return form.method === 'password' ? form.storePassword : form.storeToken;
}

/** The public account of a new Zabbix installation, offered by «Fill in the default account». */
export const ZABBIX_DEFAULT_ACCOUNT = { username: 'Admin', password: 'zabbix' } as const;

/** Checks the browser can do before sending; the server repeats every check. */
export function validateForm(form: ProfileForm, plainHttp: boolean): FormErrors {
  const errors: FormErrors = {};
  const name = form.name.trim();
  if (name === '' || [...name].length > MAX_PROFILE_NAME_CHARS) {
    errors.name = codeText('profile_name_invalid', { max_chars: String(MAX_PROFILE_NAME_CHARS) });
  }
  if (form.address.trim() === '') errors.address = codeText('invalid_address', { reason: 'syntax' });
  if (form.method === 'password' && form.username === '') errors.username = codeText('username_invalid', { max_chars: '255' });
  if (storesSecret(form) && plainHttp && !form.confirmPlainHttp) {
    errors.confirm_http = codeText('plain_http_token_storage_unconfirmed');
  }
  if (form.trust === 'custom_ca' && !form.caRef) errors.trust = { id: 'connection.form.custom_ca_required' };
  return errors;
}

/** Field that owns a server failure; `form` for everything else (shown in the form summary). */
export function fieldOfCode(code: string): FormField | 'form' {
  switch (code) {
    case 'profile_name_invalid':
      return 'name';
    case 'invalid_address':
      return 'address';
    case 'username_invalid':
      return 'username';
    case 'plain_http_token_storage_unconfirmed':
      return 'confirm_http';
    case 'trust_material_invalid':
    case 'trust_material_missing':
    case 'trust_upload_invalid':
      return 'trust';
    default:
      return 'form';
  }
}

export const FIELD_ORDER: ReadonlyArray<FormField> = ['name', 'address', 'username', 'confirm_http', 'trust'];

export function firstInvalidField(errors: FormErrors): FormField | 'form' | null {
  return FIELD_ORDER.find((field) => errors[field] !== undefined) ?? (errors.form ? 'form' : null);
}

/** «Это другой сервер» (P1-02B): only for an existing profile whose normalized address changes. */
export function serverChanged(original: ConnectionProfile | null, previewBaseUrl: string | null): boolean {
  return original !== null && previewBaseUrl !== null && previewBaseUrl !== original.endpoint_display;
}

export function duplicateName(source: string, format: (name: string) => string): string {
  const name = format(source);
  const characters = [...name];
  return characters.length <= MAX_PROFILE_NAME_CHARS ? name : characters.slice(0, MAX_PROFILE_NAME_CHARS).join('');
}

// ---------------------------------------------------------------- sign-in

/**
 * «Connect» and «Check» need the sign-in dialog first: no secret is stored in the system credential
 * store and none is kept in the local process memory (O-037).
 */
export function asksSecretFirst(profile: ConnectionProfile): boolean {
  return profile.secret_storage === 'none' && !profile.secret_in_memory;
}

/** The connect or test command needs a secret typed by the user (P1-02C1). */
export function needsSecret(failure: CommandFailure): boolean {
  return failure.code === 'secret_required' || failure.code === 'credential_missing';
}

export function consecutiveFailures(report: CheckReport | null): number {
  const auth = report?.steps.find((step) => step.step === 'auth');
  const value = Number(auth?.params.consecutive_failures ?? 0);
  return Number.isFinite(value) ? value : 0;
}

/** Seconds of the Zabbix sign-in block that apply after this report, 0 when sign-in is not blocked. */
export function lockoutSeconds(report: CheckReport | null): number {
  return consecutiveFailures(report) >= SIGN_IN_ATTEMPTS ? SIGN_IN_BLOCK_SECONDS : 0;
}

export function failedStep(report: CheckReport | null): CheckStepName | null {
  return report?.steps.find((step) => step.status === 'failed')?.step ?? null;
}

/** Sign-in failure codes that belong under the secret field of the dialog. */
export function signInFailure(report: CheckReport): { code: string; params: Readonly<Record<string, string>> } | null {
  const failed = report.steps.find((step) => step.status === 'failed');
  if (!failed) return null;
  return { code: failed.code ?? 'unknown', params: failed.params };
}

// ---------------------------------------------------------------- report

/** Normalized UTC form of the server timestamp for the technical report; `unknown` when invalid. */
function isoTime(value: string | null): string {
  const time = parseTimestamp(value);
  return Number.isFinite(time) ? new Date(time).toISOString() : 'unknown';
}

function paramText(params: Readonly<Record<string, string>>): string {
  const entries = Object.entries(params);
  return entries.length === 0 ? '' : ` (${entries.map(([name, value]) => `${name}=${value}`).join(', ')})`;
}

/**
 * Sanitized diagnostic report (P1-02D «Копировать отчёт»): codes, steps, method and time in
 * English technical form. It never contains a password, token, server response or file path; the
 * server address is added only when the user ticks «Include the server address».
 */
export function checkReportText(report: CheckReport, profile: ConnectionProfile | null, includeAddress: boolean): string {
  const lines = [
    'ZDashstitch connection check',
    `checked_at: ${isoTime(report.checked_at)}`,
    `auth_method: ${report.auth_method}`,
    `server_version: ${report.server_version ?? 'unknown'}`,
    `result: ${report.passed ? 'passed' : `failed ${report.result_code ?? 'unknown'}`}`
  ];
  const failed = failedStep(report);
  if (failed) lines.push(`failed_step: ${failed}`);
  for (const step of report.steps) {
    lines.push(`step ${step.step}: ${step.status}${step.code ? ` ${step.code}` : ''}${paramText(step.params)}`);
  }
  if (includeAddress && profile) lines.push(`address: ${profile.endpoint_display}`);
  return `${lines.join('\n')}\n`;
}
