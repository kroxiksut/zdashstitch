// Connection state of the running UI (P1-01C axis D): the profile list, the one active
// connection shown in the header, in-flight commands and the announcement of their result.
// Secrets pass through `submitSecret` only and are never kept in this store.

import type { LocalizedText } from '../components';
import {
  failureOf,
  type CheckReport,
  type CommandFailure,
  type ConnectionList,
  type ConnectionProfile,
  type ConnectionsClient
} from './client';
import { codeText } from './messages';
import { asksSecretFirst, headerConnection, needsSecret, withProfile, withoutProfile, type HeaderConnection } from './state';

export type SignInPurpose = 'connect' | 'test';

export interface SignInRequest {
  readonly profile: ConnectionProfile;
  readonly purpose: SignInPurpose;
  readonly acceptNewIdentity: boolean;
}

/** Result of one submitted secret, as the sign-in dialog needs it. */
export type SecretOutcome =
  | { readonly kind: 'done'; readonly report: CheckReport }
  | { readonly kind: 'rejected'; readonly report: CheckReport }
  | { readonly kind: 'failed'; readonly failure: CommandFailure }
  | { readonly kind: 'aborted' };

export interface Announcement {
  readonly text: LocalizedText;
  readonly detail?: LocalizedText;
  readonly tone: 'success' | 'info' | 'warning' | 'danger';
}

/** A sign-in rejected at step 3 keeps the dialog open; other results close it (P1-02C1). */
function rejectedAtSignIn(report: CheckReport): boolean {
  return report.steps.some((step) => step.step === 'auth' && step.status === 'failed');
}

export class ConnectionsStore {
  list = $state<ConnectionList | null>(null);
  loadFailure = $state<CommandFailure | null>(null);
  /** Profile IDs with a running connect/test command. */
  busy = $state<Record<string, 'connect' | 'test' | 'other'>>({});
  /** Last check or connect report per profile, for this run of the UI only. */
  reports = $state<Record<string, CheckReport>>({});
  announcement = $state<Announcement | null>(null);
  signIn = $state<SignInRequest | null>(null);
  /** Profile whose check view starts the check on open («Check connection» in the editor). */
  autostart = $state<string | null>(null);
  /** Profile that just signed in with the default Zabbix account (O-037); shown once per run. */
  defaultPasswordWarning = $state<ConnectionProfile | null>(null);
  readonly header: HeaderConnection = $derived(headerConnection(this.list));

  #controllers = new Map<string, AbortController>();
  #defaultPasswordShown = new Set<string>();

  constructor(readonly client: ConnectionsClient) {}

  profile(localId: string): ConnectionProfile | null {
    return this.list?.profiles.find((candidate) => candidate.local_id === localId) ?? null;
  }

  async refresh(): Promise<void> {
    try {
      this.list = await this.client.list();
      this.loadFailure = null;
    } catch (error) {
      this.loadFailure = failureOf(error);
    }
  }

  apply(profile: ConnectionProfile | null): void {
    if (profile && this.list) this.list = withProfile(this.list, profile);
  }

  forget(localId: string): void {
    if (this.list) this.list = withoutProfile(this.list, localId);
    delete this.reports[localId];
  }

  announce(announcement: Announcement | null): void {
    this.announcement = announcement;
  }

  /**
   * «Connect»: a password or token stored in the system credential store or kept in the local
   * process memory connects without a dialog (O-037). The dialog opens when there is none, when the
   * stored entry is missing, or when the server rejected the kept secret (it is dropped then).
   */
  async connect(profile: ConnectionProfile, acceptNewIdentity = false): Promise<void> {
    if (asksSecretFirst(profile)) {
      this.signIn = { profile, purpose: 'connect', acceptNewIdentity };
      return;
    }
    const outcome = await this.run(profile, 'connect', undefined, acceptNewIdentity);
    if ((outcome.kind === 'failed' && needsSecret(outcome.failure)) || outcome.kind === 'rejected') {
      this.signIn = { profile: this.profile(profile.local_id) ?? profile, purpose: 'connect', acceptNewIdentity };
    }
  }

  /** «Check»: same secret rules as connect; the session is closed after the five steps. */
  async test(profile: ConnectionProfile): Promise<void> {
    if (asksSecretFirst(profile)) {
      this.signIn = { profile, purpose: 'test', acceptNewIdentity: false };
      return;
    }
    const outcome = await this.run(profile, 'test');
    if ((outcome.kind === 'failed' && needsSecret(outcome.failure)) || outcome.kind === 'rejected') {
      this.signIn = { profile: this.profile(profile.local_id) ?? profile, purpose: 'test', acceptNewIdentity: false };
    }
  }

  /** Called by the sign-in dialog with the typed secret; the dialog clears its field first. */
  async submitSecret(request: SignInRequest, secret: string): Promise<SecretOutcome> {
    const outcome = await this.run(request.profile, request.purpose, secret, request.acceptNewIdentity);
    if (outcome.kind === 'rejected' && request.purpose === 'connect') return outcome;
    if (outcome.kind !== 'aborted' && !(outcome.kind === 'failed' && needsSecret(outcome.failure))) {
      this.signIn = null;
    }
    return outcome;
  }

  /**
   * Closes the default-password warning; `hide` stores «Do not show again for this connection» in
   * the profile (a non-secret flag).
   */
  async dismissDefaultPasswordWarning(hide: boolean): Promise<void> {
    const profile = this.defaultPasswordWarning;
    this.defaultPasswordWarning = null;
    if (!profile || !hide) return;
    try {
      this.apply(await this.client.hideDefaultPasswordWarning(profile.local_id));
    } catch (error) {
      this.announceFailure(error);
    }
  }

  closeSignIn(): void {
    if (this.signIn) this.cancel(this.signIn.profile.local_id);
    this.signIn = null;
  }

  cancel(localId: string): void {
    this.#controllers.get(localId)?.abort();
  }

  async disconnect(profile: ConnectionProfile): Promise<void> {
    this.busy = { ...this.busy, [profile.local_id]: 'other' };
    try {
      const outcome = await this.client.disconnect(profile.local_id);
      this.apply(outcome.profile);
      this.announce({
        text: { id: 'connection.disconnected', params: { profile: profile.name } },
        detail: outcome.logout ? { id: 'connection.logout_failed' } : undefined,
        tone: 'info'
      });
    } catch (error) {
      this.announceFailure(error);
      await this.refresh();
    } finally {
      this.clearBusy(profile.local_id);
    }
  }

  async enableWrites(profile: ConnectionProfile): Promise<boolean> {
    try {
      const outcome = await this.client.enableWrites(profile.local_id);
      this.apply(outcome.profile);
      this.announce({ text: { id: 'connection.enable_writes.enabled', params: { profile: profile.name } }, tone: 'warning' });
      return true;
    } catch (error) {
      this.announceFailure(error);
      await this.refresh();
      return false;
    }
  }

  async setReadOnly(localId: string): Promise<void> {
    try {
      const outcome = await this.client.setReadOnly(localId);
      this.apply(outcome.profile);
      this.announce({ text: { id: 'connection.read_only_restored' }, tone: 'info' });
    } catch (error) {
      this.announceFailure(error);
      await this.refresh();
    }
  }

  announceFailure(error: unknown): void {
    const failure = failureOf(error);
    if (failure.code === 'aborted') return;
    this.announce({ text: codeText(failure.code, failure.params), tone: 'danger' });
  }

  /** Raises the default-password warning once per run of the UI for this profile. */
  private noteDefaultPassword(profile: ConnectionProfile, report: CheckReport): void {
    if (!report.default_password_warning || this.#defaultPasswordShown.has(profile.local_id)) return;
    this.#defaultPasswordShown.add(profile.local_id);
    this.defaultPasswordWarning = profile;
  }

  private clearBusy(localId: string): void {
    const next = { ...this.busy };
    delete next[localId];
    this.busy = next;
  }

  private async run(
    profile: ConnectionProfile,
    purpose: SignInPurpose,
    secret?: string,
    acceptNewIdentity = false
  ): Promise<SecretOutcome> {
    const localId = profile.local_id;
    this.#controllers.get(localId)?.abort();
    const controller = new AbortController();
    this.#controllers.set(localId, controller);
    this.busy = { ...this.busy, [localId]: purpose };
    this.announce(null);
    try {
      const options = { secret, acceptNewIdentity, signal: controller.signal };
      if (purpose === 'test') {
        const outcome = await this.client.test(localId, options);
        this.reports = { ...this.reports, [localId]: outcome.report };
        this.apply(outcome.profile);
        this.noteDefaultPassword(outcome.profile ?? profile, outcome.report);
        return rejectedAtSignIn(outcome.report)
          ? { kind: 'rejected', report: outcome.report }
          : { kind: 'done', report: outcome.report };
      }
      const outcome = await this.client.connect(localId, options);
      this.reports = { ...this.reports, [localId]: outcome.report };
      if (outcome.connected) {
        // Connecting replaced any other active connection on the server.
        await this.refresh();
        this.apply(outcome.profile);
        this.announce({
          text: { id: 'connection.connected', params: { profile: profile.name } },
          detail: outcome.credential_save ? { id: 'connection.credential_save_failed' } : undefined,
          tone: 'success'
        });
        this.noteDefaultPassword(outcome.profile ?? profile, outcome.report);
        return { kind: 'done', report: outcome.report };
      }
      this.apply(outcome.profile);
      const failed = outcome.report.steps.find((step) => step.status === 'failed');
      this.announce({
        text: { id: 'connection.connect_failed', params: { profile: profile.name } },
        detail: failed ? codeText(failed.code ?? 'unknown', failed.params) : undefined,
        tone: 'danger'
      });
      return rejectedAtSignIn(outcome.report)
        ? { kind: 'rejected', report: outcome.report }
        : { kind: 'done', report: outcome.report };
    } catch (error) {
      const failure = failureOf(error);
      if (failure.code === 'aborted') return { kind: 'aborted' };
      if (!needsSecret(failure)) {
        this.announce({ text: codeText(failure.code, failure.params), tone: 'danger' });
      }
      return { kind: 'failed', failure };
    } finally {
      if (this.#controllers.get(localId) === controller) this.#controllers.delete(localId);
      this.clearBusy(localId);
    }
  }
}
