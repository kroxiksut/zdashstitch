// Typed client of the local draft and server-dashboard routes (P2-05D over P1-03H). The browser
// never talks to Zabbix: the local server reads the active connection read-only and answers with
// data, codes and safe parameters. Times arrive as RFC 3339 UTC strings (O-035) and are parsed
// here with `parseTimestamp`; Zabbix IDs (`dashboardid`, `dashboard_pageid`) are used only in API
// calls, never in interface routes.

import { ApiFailure, apiFailure } from '../api';
import { failureOf, type CommandFailure } from '../connections/client';
import { CSRF_HEADER, loadCsrfToken } from '../session';
import type {
  CreatedDashboard,
  DamagedDraft,
  DraftDetail,
  DraftFreshnessState,
  DraftKind,
  DraftLocalState,
  DraftOrigin,
  DraftsClient,
  DraftSummary
} from '../shell/drafts';
import { parseTimestamp } from '../timestamp';

// ---------------------------------------------------------------- wire types

interface WireSource {
  readonly dashboard_name: string;
  readonly page_name: string | null;
  readonly page_number: number | null;
  readonly connection_local_id: string;
  readonly server_version: string;
  readonly snapshot_at: string | null;
}

interface WireSummary {
  readonly local_id: string;
  readonly kind: DraftKind;
  readonly origin: DraftOrigin;
  readonly name: string;
  readonly source: WireSource;
  readonly freshness: { readonly state: DraftFreshnessState; readonly checked_at: string | null };
  readonly local_state: DraftLocalState;
  readonly updated_at: string | null;
  readonly counts: { readonly pages: number; readonly widgets: number };
  readonly created_dashboards: ReadonlyArray<{
    readonly connection_local_id: string;
    readonly name: string;
    readonly created_at: string | null;
  }>;
}

interface WireDetail extends WireSummary {
  readonly pages: ReadonlyArray<{
    readonly id: string;
    readonly name: string;
    readonly display_period: number | null;
    readonly widgets: ReadonlyArray<{
      readonly id: string;
      readonly name: string;
      readonly type: string;
      readonly x: number;
      readonly y: number;
      readonly width: number;
      readonly height: number;
      readonly editable: boolean;
    }>;
  }>;
  readonly properties: { readonly display_period: number; readonly auto_start: boolean } | null;
}

interface WireCopy {
  readonly local_id: string;
  readonly kind: DraftKind;
  readonly snapshot_at: string | null;
}

interface WireRow extends Omit<ServerDashboardRow, 'existing_copies'> {
  readonly existing_copies: ReadonlyArray<WireCopy>;
}

interface WireList extends Omit<ServerDashboardList, 'rows'> {
  readonly rows: ReadonlyArray<WireRow>;
}

interface WirePreview extends Omit<ImportPreview, 'existing_copies'> {
  readonly existing_copies: ReadonlyArray<WireCopy>;
}

// ---------------------------------------------------------------- UI model

export type AccessFilter = 'all' | 'mine' | 'shared';
export type DashboardOwner = 'me' | 'other';
export type DashboardAccess = 'private' | 'shared';

/** A local copy of a server dashboard («У меня»). */
export interface ExistingCopy {
  readonly local_id: string;
  readonly kind: DraftKind;
  /** Epoch milliseconds of the snapshot. */
  readonly snapshot_at: number;
}

export interface ServerDashboardRow {
  /** Zabbix ID: API calls only, never a route or visible text. */
  readonly dashboardid: string;
  /** Server data. */
  readonly name: string;
  readonly owner: DashboardOwner;
  readonly access: DashboardAccess;
  readonly page_count: number;
  readonly widget_count: number;
  readonly editable_widget_count: number;
  /** Newest first. */
  readonly existing_copies: ReadonlyArray<ExistingCopy>;
}

export interface ServerDashboardList {
  readonly rows: ReadonlyArray<ServerDashboardRow>;
  /** Matching dashboards after the access filter, before paging. */
  readonly total: number;
  /** Matching dashboards of the search per access filter. */
  readonly counts: Readonly<Record<AccessFilter, number>>;
  readonly offset: number;
  readonly limit: number;
  /** More dashboards matched on the server than one read returns. */
  readonly truncated: boolean;
  /** `not_visible_or_denied` when nothing matched: Zabbix does not tell the two apart. */
  readonly empty_reason: 'not_visible_or_denied' | null;
  /** `false`: local copies could not be checked, so «no copy» must not be shown. */
  readonly existing_copies_known: boolean;
}

export interface PreviewWidget {
  /** Zabbix ID: a key of the read-only preview only, never shown. */
  readonly id: string;
  /** Server data; may be empty. */
  readonly name: string;
  /** Technical widget type ID. */
  readonly type: string;
  /** Position and size in dashboard grid cells (P2-05E «Предпросмотр»). */
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly editable: boolean;
}

export interface PreviewPage {
  /** Zabbix ID: used in the page import body only. */
  readonly dashboard_pageid: string;
  readonly name: string;
  /** Seconds; `null` means the dashboard default. */
  readonly display_period: number | null;
  readonly widget_count: number;
  readonly editable_widget_count: number;
  readonly widgets: ReadonlyArray<PreviewWidget>;
}

export interface PreviewDependencies {
  readonly hosts: number;
  readonly host_patterns: number;
  readonly items: number;
  readonly item_patterns: number;
  readonly graphs: number;
  readonly widget_references: number;
  readonly unknown: number;
}

/** «Проверка перед копированием» (P1-03C): one read; nothing is stored. */
export interface ImportPreview {
  readonly dashboardid: string;
  readonly name: string;
  readonly owner: DashboardOwner;
  readonly access: DashboardAccess;
  readonly pages: ReadonlyArray<PreviewPage>;
  /** Dashboard properties as in Zabbix, for the read-only preview (P2-05E). */
  readonly properties: { readonly display_period: number; readonly auto_start: boolean };
  readonly dependencies: PreviewDependencies;
  readonly unknown_widget_types: ReadonlyArray<string>;
  readonly unknown_field_count: number;
  readonly existing_copies: ReadonlyArray<ExistingCopy>;
}

export type ImportScope = { readonly scope: 'dashboard' } | { readonly scope: 'page'; readonly dashboard_pageid: string };

export interface ImportOutcome {
  readonly local_id: string;
  readonly kind: DraftKind;
  /** User data: the stored draft name. */
  readonly name: string;
  readonly attempts: number;
  /** Technical detail. */
  readonly snapshot_fingerprint: string;
  readonly snapshot_reused: boolean;
  readonly item_metadata: 'not_needed' | 'complete' | 'partial' | 'unavailable';
}

export interface ServerQuery {
  /** Name substring; empty means all. */
  readonly search: string;
  readonly access: AccessFilter;
  readonly offset: number;
  readonly limit: number;
}

export interface ServerClient {
  list(connectionLocalId: string, query: ServerQuery, signal?: AbortSignal): Promise<ServerDashboardList>;
  preview(connectionLocalId: string, dashboardid: string, signal?: AbortSignal): Promise<ImportPreview>;
  /** `name`: the local draft name (P2-08A «… (copy N)»); absent keeps the source name. */
  importDashboard(connectionLocalId: string, dashboardid: string, scope: ImportScope, name?: string): Promise<ImportOutcome>;
}

// ---------------------------------------------------------------- mapping

function copyOf(copy: WireCopy): ExistingCopy {
  return { local_id: copy.local_id, kind: copy.kind, snapshot_at: parseTimestamp(copy.snapshot_at) };
}

/** Wire summary → UI model. The profile name is filled later from the profile list. */
export function summaryFromWire(wire: WireSummary): DraftSummary {
  const source = wire.source;
  const created: CreatedDashboard[] = wire.created_dashboards.map((item) => ({
    name: item.name,
    created_at: parseTimestamp(item.created_at),
    connection_local_id: item.connection_local_id,
    // The application records a created dashboard only after a verified read-back (O-022).
    state: 'verified'
  }));
  const freshness =
    wire.freshness.state === 'current'
      ? { state: wire.freshness.state, checked_at: parseTimestamp(wire.freshness.checked_at) }
      : { state: wire.freshness.state };
  return {
    local_id: wire.local_id,
    kind: wire.kind,
    origin: wire.origin,
    name: wire.name,
    source: {
      dashboard_name: source.dashboard_name,
      ...(source.page_name === null ? {} : { page_name: source.page_name }),
      ...(source.page_number === null ? {} : { page_number: source.page_number }),
      connection_local_id: source.connection_local_id,
      server_version: source.server_version,
      snapshot_at: parseTimestamp(source.snapshot_at)
    },
    freshness,
    local_state: wire.local_state,
    updated_at: parseTimestamp(wire.updated_at),
    counts: { pages: wire.counts.pages, widgets: wire.counts.widgets },
    created_dashboards: created
  };
}

export function detailFromWire(wire: WireDetail): DraftDetail {
  return {
    ...summaryFromWire(wire),
    pages: wire.pages.map((page) => ({
      id: page.id,
      name: page.name,
      display_period: page.display_period,
      widgets: page.widgets.map((widget) => ({ ...widget }))
    })),
    ...(wire.properties === null ? {} : { properties: { ...wire.properties } })
  };
}

// ---------------------------------------------------------------- transport

function failure(error: unknown): CommandFailure {
  return failureOf(error);
}

/** No server answer at all (network error, not JSON): the backend is unavailable. */
function unreachable(error: unknown): boolean {
  return !(error instanceof ApiFailure && error.response);
}

/** Clients over the local server. The CSRF token is read from `/api/session` and kept in memory only. */
export function createHttpWorkcopyClients(fetcher: typeof fetch = (...args) => fetch(...args)): {
  drafts: DraftsClient;
  server: ServerClient;
} {
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

  const draftPath = (localId: string, action?: string): string =>
    `/api/drafts/${encodeURIComponent(localId)}${action === undefined ? '' : `/${action}`}`;
  const serverPath = (connectionLocalId: string, dashboardid?: string, action?: string): string => {
    let path = `/api/server/${encodeURIComponent(connectionLocalId)}/dashboards`;
    if (dashboardid !== undefined) path += `/${encodeURIComponent(dashboardid)}`;
    if (action !== undefined) path += `/${action}`;
    return path;
  };

  const drafts: DraftsClient = {
    async list() {
      try {
        const wire = await send<{ drafts: WireSummary[]; damaged: DamagedDraft[] }>('GET', '/api/drafts');
        return { status: 'ready', drafts: wire.drafts.map(summaryFromWire), damaged: wire.damaged };
      } catch (error) {
        return unreachable(error) ? { status: 'unavailable' } : { status: 'failed', failure: failure(error) };
      }
    },
    async load(localId) {
      try {
        return { status: 'ready', draft: detailFromWire(await send<WireDetail>('GET', draftPath(localId))) };
      } catch (error) {
        if (unreachable(error)) return { status: 'unavailable' };
        const result = failure(error);
        return result.code === 'workcopy_not_found' ? { status: 'missing' } : { status: 'failed', failure: result };
      }
    },
    async rename(localId, name) {
      try {
        return { status: 'ready', draft: summaryFromWire(await send<WireSummary>('POST', draftPath(localId, 'rename'), { name })) };
      } catch (error) {
        return unreachable(error) ? { status: 'unavailable' } : { status: 'failed', failure: failure(error) };
      }
    }
  };

  const server: ServerClient = {
    async list(connectionLocalId, query, signal) {
      const parameters = new URLSearchParams();
      if (query.search.trim() !== '') parameters.set('search', query.search.trim());
      if (query.access !== 'all') parameters.set('access', query.access);
      if (query.offset > 0) parameters.set('offset', String(query.offset));
      parameters.set('limit', String(query.limit));
      const wire = await send<WireList>('GET', `${serverPath(connectionLocalId)}?${parameters.toString()}`, undefined, signal);
      return { ...wire, rows: wire.rows.map((row) => ({ ...row, existing_copies: row.existing_copies.map(copyOf) })) };
    },
    async preview(connectionLocalId, dashboardid, signal) {
      const wire = await send<WirePreview>('GET', serverPath(connectionLocalId, dashboardid), undefined, signal);
      return { ...wire, existing_copies: wire.existing_copies.map(copyOf) };
    },
    importDashboard(connectionLocalId, dashboardid, scope, name) {
      return send<ImportOutcome>('POST', serverPath(connectionLocalId, dashboardid, 'import'), name === undefined ? scope : { ...scope, name });
    }
  };

  return { drafts, server };
}
