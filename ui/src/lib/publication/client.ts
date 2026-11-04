// Typed client of the publication routes (P2-04E/F over P1-08H). The browser holds only an opaque
// plan handle and the `plan_id` it showed; it never sees a plan, a payload or Zabbix. Times arrive
// as RFC 3339 UTC strings (O-035) and are parsed here. Production uses the HTTP client; tests and
// the DEMO build use the in-memory client, which never reaches the production build.

import { ApiFailure, apiFailure } from '../api';
import { previewFromWire } from '../bindings/client';
import type { CompatibilityReport } from '../bindings/model';
import { CSRF_HEADER, loadCsrfToken } from '../session';
import { parseTimestamp } from '../timestamp';
import type {
  CheckResult,
  ConfirmRequest,
  DamagedJournal,
  JournalEntry,
  JournalList,
  Outcome,
  PlanPreview,
  PlanRequest,
  PlanResult,
  PublicationFailure,
  RefetchResult
} from './model';

export interface PublicationClient {
  /** Fresh read of the source and of the replacement; read-only. */
  check(localId: string): Promise<CheckResult>;
  /** The server builds and keeps the plan; the answer is its preview and handle. */
  plan(localId: string, request: PlanRequest): Promise<PlanResult>;
  /** The one write: exactly what the dialog showed. */
  confirm(localId: string, request: ConfirmRequest): Promise<Outcome>;
  /** «Отмена»: the plan is dropped. */
  discard(localId: string, handle: string): Promise<void>;
  /** «Скопировать заново»: a new copy with the replacement calculated again. */
  refetch(localId: string, name: string | null): Promise<RefetchResult>;
  journal(): Promise<JournalList>;
  entry(operationId: string): Promise<JournalEntry>;
  /** «Проверить результат»: reads only. */
  reconcile(operationId: string): Promise<Outcome>;
  /** «Закрыть без сверки». */
  close(operationId: string): Promise<JournalEntry>;
  /** «Убрать в карантин». */
  quarantine(operationId: string): Promise<void>;
}

// ---------------------------------------------------------------- failures

/** A rejected command of the in-memory client, shaped like an HTTP failure. */
export class PublicationMemoryFailure extends Error {
  constructor(
    readonly code: string,
    readonly params: Readonly<Record<string, string>> = {},
    readonly report?: CompatibilityReport
  ) {
    super(code);
    this.name = 'PublicationMemoryFailure';
  }
}

interface BlockedResponse {
  readonly code: string;
  readonly params?: Readonly<Record<string, string>>;
  readonly report?: CompatibilityReport;
}

/** Reduce any thrown value to a code; a network failure has no server code. */
export function publicationFailure(error: unknown): PublicationFailure {
  if (error instanceof PublicationMemoryFailure) {
    return error.report ? { code: error.code, params: error.params, status: 0, report: error.report } : { code: error.code, params: error.params, status: 0 };
  }
  if (error instanceof ApiFailure && error.response) {
    const response = error.response as BlockedResponse;
    const failure = { code: response.code, params: response.params ?? {}, status: error.status };
    return response.report ? { ...failure, report: response.report } : failure;
  }
  if (error instanceof DOMException && error.name === 'AbortError') return { code: 'aborted', params: {}, status: 0 };
  return { code: 'backend_unavailable', params: {}, status: error instanceof ApiFailure ? error.status : 0 };
}

// ---------------------------------------------------------------- wire types

type WireTime = string | null;
type WireEntry = Omit<JournalEntry, 'transitions' | 'plan_created_at' | 'confirmed_at' | 'submitted_at'> & {
  readonly transitions: ReadonlyArray<{ readonly state: JournalEntry['state']; readonly at: WireTime }>;
  readonly plan_created_at: WireTime;
  readonly confirmed_at: WireTime;
  readonly submitted_at: WireTime;
};
type WireOutcome = Omit<Outcome, 'entry' | 'base_advanced'> & {
  readonly entry: WireEntry | null;
  readonly base_advanced: { readonly revision: number; readonly base_fingerprint: string; readonly read_at: WireTime } | null;
};
type WirePlan = Omit<PlanPreview, 'created_at'> & { readonly created_at: WireTime };
type WireCheck = Omit<CheckResult, 'source' | 'preview'> & {
  readonly source: Omit<CheckResult['source'], 'checked_at'> & { readonly checked_at: WireTime };
  readonly preview: Parameters<typeof previewFromWire>[0];
};

export function entryFromWire(wire: WireEntry): JournalEntry {
  return {
    ...wire,
    transitions: wire.transitions.map((transition) => ({ state: transition.state, at: parseTimestamp(transition.at) })),
    plan_created_at: parseTimestamp(wire.plan_created_at),
    confirmed_at: parseTimestamp(wire.confirmed_at),
    submitted_at: wire.submitted_at === null ? null : parseTimestamp(wire.submitted_at)
  };
}

export function outcomeFromWire(wire: WireOutcome): Outcome {
  return {
    ...wire,
    entry: wire.entry === null ? null : entryFromWire(wire.entry),
    base_advanced: wire.base_advanced === null ? null : { ...wire.base_advanced, read_at: parseTimestamp(wire.base_advanced.read_at) }
  };
}

function planFromWire(wire: WirePlan): PlanPreview {
  return { ...wire, created_at: parseTimestamp(wire.created_at) } as PlanPreview;
}

function checkFromWire(wire: WireCheck): CheckResult {
  return {
    ...wire,
    source: { ...wire.source, checked_at: parseTimestamp(wire.source.checked_at) },
    preview: previewFromWire(wire.preview)
  };
}

/** Client over the local server. The CSRF token is read from `/api/session` and kept in memory only. */
export function createHttpPublicationClient(fetcher: typeof fetch = (...args) => fetch(...args)): PublicationClient {
  let csrf: Promise<string> | null = null;

  function ensureCsrf(): Promise<string> {
    csrf ??= loadCsrfToken(fetcher).catch((error: unknown) => {
      csrf = null;
      throw error;
    });
    return csrf;
  }

  async function send<T>(method: 'GET' | 'POST', path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { Accept: 'application/json' };
    const init: RequestInit = { credentials: 'same-origin', method, headers };
    if (method === 'POST') {
      headers['Content-Type'] = 'application/json';
      headers[CSRF_HEADER] = await ensureCsrf();
      init.body = JSON.stringify(body ?? {});
    }
    const response = await fetcher(path, init);
    if (!response.ok) throw await apiFailure(response);
    return (await response.json()) as T;
  }

  const draft = (localId: string, action: string): string => `/api/drafts/${encodeURIComponent(localId)}/publication/${action}`;
  const journal = (operationId: string, action?: string): string =>
    `/api/publication/journal/${encodeURIComponent(operationId)}${action === undefined ? '' : `/${action}`}`;

  return {
    async check(localId) {
      return checkFromWire(await send<WireCheck>('POST', draft(localId, 'check')));
    },
    async plan(localId, request) {
      const wire = await send<{ handle: string; plan: WirePlan }>('POST', draft(localId, 'plan'), request);
      return { handle: wire.handle, plan: planFromWire(wire.plan) };
    },
    async confirm(localId, request) {
      return outcomeFromWire(await send<WireOutcome>('POST', draft(localId, 'confirm'), request));
    },
    async discard(localId, handle) {
      await send<unknown>('POST', draft(localId, 'discard'), { handle });
    },
    refetch(localId, name) {
      return send<RefetchResult>('POST', draft(localId, 'refetch'), name === null ? {} : { name });
    },
    async journal() {
      const wire = await send<{ unfinished: WireEntry[]; damaged: DamagedJournal[] }>('GET', '/api/publication/journal');
      return { unfinished: wire.unfinished.map(entryFromWire), damaged: wire.damaged };
    },
    async entry(operationId) {
      return entryFromWire(await send<WireEntry>('GET', journal(operationId)));
    },
    async reconcile(operationId) {
      return outcomeFromWire(await send<WireOutcome>('POST', journal(operationId, 'reconcile')));
    },
    async close(operationId) {
      return entryFromWire(await send<WireEntry>('POST', journal(operationId, 'close'), { confirmed: true }));
    },
    async quarantine(operationId) {
      await send<unknown>('POST', journal(operationId, 'quarantine'), { confirmed: true });
    }
  };
}

// ---------------------------------------------------------------- in-memory client

/** Scripted behaviour of one draft for tests and the DEMO build. */
export interface MemoryDraft {
  readonly check: CheckResult;
  /** What «Записать в Zabbix» ends with, per action. */
  readonly outcome: Readonly<Partial<Record<PlanRequest['action'], Outcome['state']>>>;
  /** Evidence of a create outcome, e.g. `dashboard_name_taken`. */
  readonly evidence?: string;
}

export interface MemoryPublicationFixture {
  readonly drafts: Readonly<Record<string, MemoryDraft>>;
  readonly journal?: JournalList;
}

function entryOf(operationId: string, localId: string, request: PlanRequest, state: Outcome['state'], evidence: string | null, now: number): JournalEntry {
  const create = request.action === 'save_as_copy';
  const sent = state !== 'failed_before_send' && state !== 'conflict';
  const transitions: JournalEntry['transitions'] = [
    { state: 'confirmed', at: now },
    ...(sent ? [{ state: 'submitting' as const, at: now + 40 }] : []),
    { state, at: now + 160 }
  ];
  return {
    operation_id: operationId,
    kind: create ? 'create' : 'update',
    intent: create ? 'save_as_copy' : 'replace_host',
    connection_local_id: 'demo-synthetic-connection',
    server_version: '7.4.16',
    target: create ? { kind: 'new_dashboard', dashboardid: null, name: request.name } : { kind: 'existing_dashboard', dashboardid: null, name: null },
    source: { local_id: localId, revision: request.revision },
    state,
    progress: state === 'unknown_outcome' || state === 'failed_after_response' || state === 'manual_check_required' ? 'needs_reconciliation' : 'finished',
    sends: sent ? 1 : 0,
    transitions,
    plan_created_at: now - 2000,
    confirmed_at: now,
    submitted_at: sent ? now + 40 : null,
    evidence,
    created_dashboardid: null,
    reported_dashboardid: null,
    closed_by: null,
    source_fingerprint: request.local_fingerprint,
    expected_fingerprint: request.local_fingerprint,
    observed_fingerprint: null
  };
}

/**
 * In-memory client for tests and the DEMO build: scripted checks and outcomes per draft, one-shot
 * handles and a journal that reconciliation, «Закрыть без сверки» and quarantine change. It never
 * pretends to reach a server: every outcome is the fixture's.
 */
export function createMemoryPublicationClient(fixture: MemoryPublicationFixture): PublicationClient {
  const plans = new Map<string, { localId: string; request: PlanRequest; plan: PlanPreview }>();
  const unfinished = new Map<string, JournalEntry>((fixture.journal?.unfinished ?? []).map((entry) => [entry.operation_id, entry]));
  const damaged = new Map<string, DamagedJournal>((fixture.journal?.damaged ?? []).map((entry) => [entry.operation_id, entry]));
  const entries = new Map<string, JournalEntry>(unfinished);
  let sequence = 0;
  const draftOf = (localId: string): MemoryDraft => {
    const draft = fixture.drafts[localId];
    if (!draft) throw new PublicationMemoryFailure('not_connected');
    return draft;
  };
  const record = (entry: JournalEntry): void => {
    entries.set(entry.operation_id, entry);
    if (entry.progress === 'needs_reconciliation') unfinished.set(entry.operation_id, entry);
    else unfinished.delete(entry.operation_id);
  };
  return {
    async check(localId) {
      return { ...draftOf(localId).check, source: { ...draftOf(localId).check.source, checked_at: Date.now() } };
    },
    async plan(localId, request) {
      const draft = draftOf(localId);
      if (draft.check.mode !== 'read_write') throw new PublicationMemoryFailure('write_mode_required');
      if (request.action === 'update_source' && draft.check.source.state !== 'current') {
        throw new PublicationMemoryFailure('plan_invalidated', { reason: 'source_changed' });
      }
      if (request.action === 'update_source' && !draft.check.preview.report.allows_publication) {
        throw new PublicationMemoryFailure('publication_blocked', {}, draft.check.preview.report);
      }
      sequence += 1;
      const now = Date.now();
      const plan_id = `zds-demo-synthetic-${String(sequence).padStart(4, '0')}`;
      const handle = `plan-demo-synthetic-${String(sequence).padStart(4, '0')}`;
      const report: CompatibilityReport = { allows_publication: true, checks: [], findings: draft.check.preview.report.findings.filter((finding) => finding.severity === 'informational') };
      const plan: PlanPreview =
        request.action === 'update_source'
          ? {
              action: 'update_source',
              plan_id,
              write_requests: 1,
              created_at: now,
              dashboardid: '0',
              replacement_count: draft.check.preview.changes.length,
              changed_pages: [...new Set(draft.check.preview.changes.map((change) => change.key.page_index))],
              contracts: [],
              replacement_kinds: [],
              revision: request.revision,
              kept_digest: request.kept_digest,
              source_fingerprint: request.base_fingerprint,
              expected_fingerprint: request.local_fingerprint,
              report
            }
          : {
              action: 'save_as_copy',
              plan_id,
              write_requests: 1,
              created_at: now,
              target_name: request.name,
              private: true,
              page_count: draft.check.preview.diff.summary.pages.total,
              widget_count: draft.check.preview.diff.summary.widgets.total,
              unknown_widgets: 0,
              contracts: [],
              name_check: 'not_visible',
              revision: request.revision,
              local_fingerprint: request.local_fingerprint,
              content_fingerprint: request.local_fingerprint,
              report
            };
      for (const [key, stored] of plans) if (stored.localId === localId) plans.delete(key);
      plans.set(handle, { localId, request, plan });
      return { handle, plan };
    },
    async confirm(localId, request) {
      const stored = plans.get(request.handle);
      plans.delete(request.handle);
      if (!stored) throw new PublicationMemoryFailure('plan_not_found');
      if (stored.localId !== localId || stored.plan.plan_id !== request.plan_id) throw new PublicationMemoryFailure('plan_mismatch');
      const draft = draftOf(localId);
      const state = draft.outcome[stored.request.action] ?? 'verified';
      const evidence = stored.request.action === 'save_as_copy' ? (draft.evidence ?? null) : null;
      const entry = entryOf(request.plan_id, localId, stored.request, state, evidence, Date.now());
      record(entry);
      return {
        operation_id: entry.operation_id,
        kind: entry.kind,
        state,
        sent: entry.sends > 0,
        invalidated: null,
        journal_error: null,
        evidence,
        created_dashboardid: null,
        target_name: stored.request.action === 'save_as_copy' ? stored.request.name : null,
        base_advanced: state === 'verified' && stored.request.action === 'update_source' ? { revision: stored.request.revision + 1, base_fingerprint: stored.request.local_fingerprint, read_at: Date.now() } : null,
        base_advance_error: null,
        record_error: null,
        entry
      };
    },
    async discard(_localId, handle) {
      plans.delete(handle);
    },
    async refetch(localId) {
      draftOf(localId);
      throw new PublicationMemoryFailure('not_connected');
    },
    async journal() {
      return { unfinished: [...unfinished.values()], damaged: [...damaged.values()] };
    },
    async entry(operationId) {
      const entry = entries.get(operationId);
      if (!entry) throw new PublicationMemoryFailure('operation_not_found');
      return entry;
    },
    async reconcile(operationId) {
      const entry = entries.get(operationId);
      if (!entry) throw new PublicationMemoryFailure('operation_not_found');
      if (entry.progress !== 'needs_reconciliation') throw new PublicationMemoryFailure('reconcile_not_needed', { state: entry.state });
      // The demo never claims a write it cannot see: zero candidates is a manual check (OPEN_DECISIONS 28.10.2026 evening, item 1).
      const updated: JournalEntry = {
        ...entry,
        state: 'manual_check_required',
        progress: 'needs_reconciliation',
        evidence: entry.kind === 'create' ? 'reconcile_not_found' : 'other_found',
        transitions: [...entry.transitions, { state: 'manual_check_required', at: Date.now() }]
      };
      record(updated);
      return {
        operation_id: operationId,
        kind: updated.kind,
        state: updated.state,
        sent: updated.sends > 0,
        invalidated: null,
        journal_error: null,
        evidence: updated.evidence,
        created_dashboardid: null,
        target_name: updated.target.kind === 'new_dashboard' ? updated.target.name : null,
        base_advanced: null,
        base_advance_error: null,
        record_error: null,
        entry: updated
      };
    },
    async close(operationId) {
      const entry = entries.get(operationId);
      if (!entry) throw new PublicationMemoryFailure('operation_not_found');
      if (entry.progress !== 'needs_reconciliation') throw new PublicationMemoryFailure('reconcile_not_needed', { state: entry.state });
      const closed: JournalEntry = {
        ...entry,
        state: 'closed_manually',
        progress: 'finished',
        transitions: [...entry.transitions, { state: 'closed_manually', at: Date.now() }]
      };
      record(closed);
      return closed;
    },
    async quarantine(operationId) {
      if (!damaged.delete(operationId)) throw new PublicationMemoryFailure('operation_not_found');
    }
  };
}
