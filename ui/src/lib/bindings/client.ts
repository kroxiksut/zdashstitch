// Typed client of the ReplaceHost routes (P2-03D/E over P1-05H, P1-08H). The browser never talks
// to Zabbix: the local server reads the draft's own connection read-only and answers with data,
// codes and safe parameters. Times arrive as RFC 3339 UTC strings (O-035) and are parsed here.
// Production uses the HTTP client; tests and the DEMO build use the in-memory client, which never
// reaches the production build.

import { apiFailure } from '../api';
import { CSRF_HEADER, loadCsrfToken } from '../session';
import { parseTimestamp } from '../timestamp';
import type {
  BindingRow,
  CalculateRequest,
  FieldKey,
  OperationRecord,
  RemoveResult,
  ReplaceHostInventory,
  ReplaceHostPreview,
  RowDecision,
  TargetHostSearch
} from './model';

export interface ReplaceHostClient {
  /** The stored operation, offline; rejects with `replace_host_not_calculated` when there is none. */
  preview(localId: string): Promise<ReplaceHostPreview>;
  inventory(localId: string, signal?: AbortSignal): Promise<ReplaceHostInventory>;
  searchHosts(localId: string, sourceHostid: string, search: string, signal?: AbortSignal): Promise<TargetHostSearch>;
  calculate(localId: string, request: CalculateRequest): Promise<ReplaceHostPreview>;
  revalidate(localId: string): Promise<ReplaceHostPreview>;
  decide(localId: string, key: FieldKey, decision: RowDecision): Promise<ReplaceHostPreview>;
  confirmKept(localId: string, digest: string): Promise<ReplaceHostPreview>;
  removeOperation(localId: string, seq: number): Promise<RemoveResult>;
  history(localId: string): Promise<ReadonlyArray<OperationRecord>>;
}

// ---------------------------------------------------------------- wire types

type WirePreview = Omit<ReplaceHostPreview, 'operation' | 'checked_at'> & {
  readonly operation: Omit<ReplaceHostPreview['operation'], 'recorded_at' | 'calculated_at'> & {
    readonly recorded_at: string | null;
    readonly calculated_at: string | null;
  };
  readonly checked_at: string | null;
};

type WireRecord = Omit<OperationRecord, 'recorded_at'> & { readonly recorded_at: string | null };

export function previewFromWire(wire: WirePreview): ReplaceHostPreview {
  return {
    ...wire,
    operation: {
      ...wire.operation,
      recorded_at: parseTimestamp(wire.operation.recorded_at),
      calculated_at: parseTimestamp(wire.operation.calculated_at)
    },
    checked_at: wire.checked_at === null ? null : parseTimestamp(wire.checked_at)
  };
}

/** Clients over the local server. The CSRF token is read from `/api/session` and kept in memory only. */
export function createHttpReplaceHostClient(fetcher: typeof fetch = (...args) => fetch(...args)): ReplaceHostClient {
  let csrf: Promise<string> | null = null;

  function ensureCsrf(): Promise<string> {
    csrf ??= loadCsrfToken(fetcher).catch((error: unknown) => {
      csrf = null;
      throw error;
    });
    return csrf;
  }

  async function send<T>(method: 'GET' | 'POST', path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
    const headers: Record<string, string> = { Accept: 'application/json' };
    const init: RequestInit = { credentials: 'same-origin', method, headers, signal };
    if (method === 'POST') {
      headers['Content-Type'] = 'application/json';
      headers[CSRF_HEADER] = await ensureCsrf();
      init.body = JSON.stringify(body ?? {});
    }
    const response = await fetcher(path, init);
    if (!response.ok) throw await apiFailure(response);
    return (await response.json()) as T;
  }

  const path = (localId: string, action?: string): string =>
    `/api/drafts/${encodeURIComponent(localId)}/replace-host${action === undefined ? '' : `/${action}`}`;

  return {
    async preview(localId) {
      return previewFromWire(await send<WirePreview>('GET', path(localId)));
    },
    inventory(localId, signal) {
      return send<ReplaceHostInventory>('GET', path(localId, 'inventory'), undefined, signal);
    },
    searchHosts(localId, sourceHostid, search, signal) {
      const parameters = new URLSearchParams({ source_hostid: sourceHostid });
      if (search.trim() !== '') parameters.set('search', search.trim());
      return send<TargetHostSearch>('GET', `${path(localId, 'hosts')}?${parameters.toString()}`, undefined, signal);
    },
    async calculate(localId, request) {
      return previewFromWire(await send<WirePreview>('POST', path(localId, 'calculate'), request));
    },
    async revalidate(localId) {
      return previewFromWire(await send<WirePreview>('POST', path(localId, 'revalidate')));
    },
    async decide(localId, key, decision) {
      return previewFromWire(await send<WirePreview>('POST', path(localId, 'decide'), { key, decision }));
    },
    async confirmKept(localId, digest) {
      return previewFromWire(await send<WirePreview>('POST', path(localId, 'confirm-kept'), { digest }));
    },
    async removeOperation(localId, seq) {
      const wire = await send<{ removed: number[]; preview: WirePreview | null }>(
        'POST',
        path(localId, `operations/${encodeURIComponent(String(seq))}/remove`)
      );
      return { removed: wire.removed, preview: wire.preview === null ? null : previewFromWire(wire.preview) };
    },
    async history(localId) {
      const wire = await send<{ operations: WireRecord[] }>('GET', path(localId, 'history'));
      return wire.operations.map((record) => ({ ...record, recorded_at: parseTimestamp(record.recorded_at) }));
    }
  };
}

// ---------------------------------------------------------------- in-memory client

/** A rejected command of the in-memory client, shaped like an HTTP failure. */
export class MemoryFailure extends Error {
  constructor(
    readonly code: string,
    readonly params: Readonly<Record<string, string>> = {}
  ) {
    super(code);
    this.name = 'MemoryFailure';
  }
}

export interface MemoryFixture {
  readonly inventory?: ReplaceHostInventory;
  readonly search?: TargetHostSearch;
  /** The preview a calculation returns (and the stored one when `calculated`). */
  readonly preview: ReplaceHostPreview;
  readonly calculated?: boolean;
}

function withRow(preview: ReplaceHostPreview, key: FieldKey, change: (row: BindingRow) => BindingRow): ReplaceHostPreview {
  return {
    ...preview,
    rows: preview.rows.map((row) =>
      row.key.page_index === key.page_index && row.key.widget_index === key.widget_index && row.key.field === key.field ? change(row) : row
    )
  };
}

/**
 * In-memory client for tests and the DEMO build: a fixed preview with local decisions, keep on
 * source, confirmation and undo. It changes rows only, not the diff or the findings, and never
 * pretends to read a server.
 */
export function createMemoryReplaceHostClient(fixtures: Readonly<Record<string, MemoryFixture>>): ReplaceHostClient {
  const stored = new Map<string, ReplaceHostPreview>();
  const original = new Map<string, Map<string, BindingRow>>();
  const history = new Map<string, OperationRecord[]>();
  let seq = 100;
  for (const [localId, fixture] of Object.entries(fixtures)) {
    if (fixture.calculated) stored.set(localId, fixture.preview);
  }
  const fixtureOf = (localId: string): MemoryFixture => {
    const fixture = fixtures[localId];
    if (!fixture) throw new MemoryFailure('workcopy_not_found');
    return fixture;
  };
  const current = (localId: string): ReplaceHostPreview => {
    fixtureOf(localId);
    const preview = stored.get(localId);
    if (!preview) throw new MemoryFailure('replace_host_not_calculated');
    return preview;
  };
  const record = (localId: string, op: string, replaceSeq: number | null): number => {
    seq += 1;
    const entries = history.get(localId) ?? [];
    entries.push({ seq, op, op_version: 1, recorded_at: Date.now(), replace_seq: replaceSeq });
    history.set(localId, entries);
    return seq;
  };
  return {
    async preview(localId) {
      return current(localId);
    },
    async inventory(localId) {
      const inventory = fixtureOf(localId).inventory;
      if (!inventory) throw new MemoryFailure('not_connected');
      return inventory;
    },
    async searchHosts(localId, _source, search) {
      const found = fixtureOf(localId).search;
      if (!found) throw new MemoryFailure('not_connected');
      const needle = search.trim().toLocaleLowerCase();
      const hosts = found.hosts.filter((host) => needle === '' || host.name.toLocaleLowerCase().includes(needle));
      return { ...found, hosts, empty_reason: hosts.length === 0 ? 'not_visible_or_denied' : null };
    },
    async calculate(localId) {
      const preview = fixtureOf(localId).preview;
      const operationSeq = record(localId, 'replace_host', null);
      const calculated = { ...preview, operation: { ...preview.operation, seq: operationSeq } };
      stored.set(localId, calculated);
      return calculated;
    },
    async revalidate(localId) {
      const preview = { ...current(localId), checked_at: Date.now() };
      stored.set(localId, preview);
      return preview;
    },
    async decide(localId, key, decision) {
      const preview = current(localId);
      const row = preview.rows.find((candidate) => candidate.key.page_index === key.page_index && candidate.key.widget_index === key.widget_index && candidate.key.field === key.field);
      if (!row) throw new MemoryFailure('binding_not_found');
      if (!row.allowed_decisions.includes(decision.kind)) throw new MemoryFailure('decision_not_applicable');
      const rows = original.get(localId) ?? new Map<string, BindingRow>();
      const id = `${key.page_index}:${key.widget_index}:${key.field}`;
      if (!rows.has(id)) rows.set(id, row);
      original.set(localId, rows);
      const decisionSeq = record(localId, 'replace_host_decision', preview.operation.seq);
      const updated = withRow(preview, key, (base) => {
        if (decision.kind === 'keep_on_source') {
          return { ...base, state: 'kept_on_source', target: null, decision, decision_seq: decisionSeq, decision_state: 'applied', kept_reason: null };
        }
        const chosen = base.candidates.find((candidate) => candidate.itemid === decision.itemid);
        if (!chosen) throw new MemoryFailure('candidate_not_visible');
        return {
          ...base,
          state: 'changed',
          target: { kind: 'item', itemid: chosen.itemid },
          target_item: chosen,
          decision,
          decision_seq: decisionSeq,
          decision_state: 'applied'
        };
      });
      stored.set(localId, updated);
      return updated;
    },
    async confirmKept(localId, digest) {
      const preview = current(localId);
      if (preview.kept_on_source.entries.length === 0) throw new MemoryFailure('kept_on_source_empty');
      if (preview.kept_on_source.digest !== digest) throw new MemoryFailure('kept_on_source_list_changed');
      const confirmationSeq = record(localId, 'replace_host_confirm_kept', preview.operation.seq);
      const updated: ReplaceHostPreview = {
        ...preview,
        kept_on_source: { ...preview.kept_on_source, confirmed: true, confirmation_seq: confirmationSeq },
        report: {
          ...preview.report,
          findings: preview.report.findings.filter((finding) => finding.code !== 'kept_on_source_unconfirmed')
        }
      };
      stored.set(localId, updated);
      return updated;
    },
    async removeOperation(localId, removedSeq) {
      const preview = current(localId);
      if (removedSeq === preview.operation.seq) {
        stored.delete(localId);
        original.delete(localId);
        const removed = (history.get(localId) ?? []).map((entry) => entry.seq);
        history.delete(localId);
        return { removed: removed.length > 0 ? removed : [removedSeq], preview: null };
      }
      const row = preview.rows.find((candidate) => candidate.decision_seq === removedSeq);
      if (!row) throw new MemoryFailure('operation_not_found', { seq: String(removedSeq) });
      const restored = original.get(localId)?.get(`${row.key.page_index}:${row.key.widget_index}:${row.key.field}`) ?? row;
      const updated = withRow(preview, row.key, () => restored);
      stored.set(localId, updated);
      history.set(localId, (history.get(localId) ?? []).filter((entry) => entry.seq !== removedSeq));
      return { removed: [removedSeq], preview: updated };
    },
    async history(localId) {
      current(localId);
      return [...(history.get(localId) ?? [])];
    }
  };
}
