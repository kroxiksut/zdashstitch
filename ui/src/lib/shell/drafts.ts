// Typed client boundary for local drafts (P1-03H «Данные строки черновика»).
// Production uses the HTTP client of `../workcopy/client` over `/api/drafts`; tests and the DEMO
// build use the in-memory client. Nothing here invents data. Times below are epoch milliseconds
// already parsed at the client boundary: the HTTP client receives RFC 3339 UTC strings (O-035) and
// converts them with `parseTimestamp`.

import type { LocalizedText } from '../components';

export type DraftKind = 'dashboard' | 'page';
export type DraftOrigin = 'local' | 'server_copy';
export type DraftLocalState = 'saved' | 'dirty';
export type DraftFreshnessState = 'not_checked' | 'current' | 'stale' | 'conflict' | 'source_missing';

export interface DraftSource {
  /** Server data: the source dashboard name, shown as data, never translated. */
  readonly dashboard_name: string;
  readonly page_name?: string;
  /** 1-based position of the source page in its dashboard; only for page copies. */
  readonly page_number?: number;
  /** Opaque ID of the connection profile the snapshot was read through. */
  readonly connection_local_id?: string;
  /** User data: the connection profile name the snapshot was read through. */
  readonly connection_name?: string;
  /** Technical value: the Zabbix server version of that connection, e.g. `7.4.16`. */
  readonly server_version?: string;
  /** Snapshot time, epoch milliseconds. */
  readonly snapshot_at: number;
}

export interface DraftFreshness {
  readonly state: DraftFreshnessState;
  /** Epoch milliseconds of the last check; only for `current`. */
  readonly checked_at?: number;
  /** Epoch milliseconds of the change found on the server; only for `conflict`, when known. */
  readonly changed_at?: number;
}

/**
 * How a local draft came to be (P1-00F §9.2). Server copies are described by `source` instead.
 * `sources` of an assembled dashboard are data names (dashboards, files), never translated.
 */
export type LocalProvenance =
  | { readonly kind: 'created_here' }
  | { readonly kind: 'imported_file' }
  | { readonly kind: 'page_file' }
  | { readonly kind: 'legacy_file' }
  | { readonly kind: 'assembled'; readonly page_count: number; readonly sources: ReadonlyArray<string> };

/** A dashboard created on the server by publishing this draft (P1-03F, O-022). */
export interface CreatedDashboard {
  /** Server data. */
  readonly name: string;
  /** Epoch milliseconds. */
  readonly created_at: number;
  /** Opaque ID of the connection profile used for the publication. */
  readonly connection_local_id?: string;
  /** User data: the connection profile used for the publication; absent when the profile is gone. */
  readonly connection_name?: string;
  /** `verified`: read back after writing; `not_found`: missing at the last check. */
  readonly state: 'verified' | 'not_found';
}

export interface DraftSummary {
  readonly local_id: string;
  readonly kind: DraftKind;
  readonly origin: DraftOrigin;
  /** User data. */
  readonly name: string;
  readonly source?: DraftSource;
  /** Only server copies have freshness. */
  readonly freshness?: DraftFreshness;
  readonly local_state: DraftLocalState;
  /** Epoch milliseconds. */
  readonly updated_at: number;
  /** Only local drafts; absent when the storage does not know it. */
  readonly provenance?: LocalProvenance;
  /** Epoch milliseconds of the last verified publication; absent means never published. */
  readonly last_published_at?: number;
  /** Size of the draft for the list details, when the storage provides it. */
  readonly counts?: { readonly pages: number; readonly widgets: number };
  readonly created_dashboards?: ReadonlyArray<CreatedDashboard>;
}

export interface DraftWidget {
  readonly id: string;
  /** User data; may be empty. */
  readonly name: string;
  /** Zabbix widget type identifier, a technical value. */
  readonly type: string;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  /** `false` for types without a codec: «Editing not supported». */
  readonly editable: boolean;
  /** Server data: the host the widget shows, when it is one known host. */
  readonly host_name?: string;
}

export interface DraftPage {
  readonly id: string;
  /** User data; may be empty. */
  readonly name: string;
  /** Seconds; `null` means the dashboard default. */
  readonly display_period: number | null;
  readonly widgets: ReadonlyArray<DraftWidget>;
}

export type CheckGroup = 'structure' | 'widgets' | 'sources' | 'references' | 'compatibility';
export const CHECK_GROUPS: ReadonlyArray<CheckGroup> = ['structure', 'widgets', 'sources', 'references', 'compatibility'];

export interface DraftFinding {
  readonly id: string;
  readonly group: CheckGroup;
  readonly level: 'blocking' | 'warning';
  readonly blocks: ReadonlyArray<'export' | 'publication'>;
  readonly message: LocalizedText;
  /** User data: page and widget names for the object path. */
  readonly page?: string;
  readonly widget?: string;
}

export interface DraftChecks {
  readonly checked_objects: number;
  readonly passed: Readonly<Record<CheckGroup, number>>;
  readonly findings: ReadonlyArray<DraftFinding>;
}

/** Zabbix dashboard properties shown in «Structure» (P1-01B §2a); only for dashboard drafts. */
export interface DashboardProperties {
  /** Default page display period, seconds. */
  readonly display_period: number;
  /** «Start slideshow automatically». */
  readonly auto_start: boolean;
}

export interface DraftDetail extends DraftSummary {
  readonly pages: ReadonlyArray<DraftPage>;
  readonly properties?: DashboardProperties;
  /** Server data: the chosen target dashboard name, only when chosen. */
  readonly target?: { readonly dashboard_name: string };
  /** Computed compatibility (P0-09/O-029), only when computed. */
  readonly compatibility?: { readonly min_version: string };
  /** Result of the local check, only when it has run. */
  readonly checks?: DraftChecks;
}

/** Machine code and safe parameters of a failed drafts command; the UI maps the code to a message. */
export interface DraftsFailure {
  readonly code: string;
  readonly params: Readonly<Record<string, string>>;
}

/** A workcopy file that could not be read; listed so the user sees it, never modified. */
export interface DamagedDraft {
  readonly local_id: string;
  readonly code: string;
}

export type DraftListResult =
  | { readonly status: 'unavailable' }
  | { readonly status: 'failed'; readonly failure: DraftsFailure }
  | { readonly status: 'ready'; readonly drafts: ReadonlyArray<DraftSummary>; readonly damaged?: ReadonlyArray<DamagedDraft> };

export type DraftLoadResult =
  | { readonly status: 'unavailable' }
  | { readonly status: 'missing' }
  | { readonly status: 'failed'; readonly failure: DraftsFailure }
  | { readonly status: 'ready'; readonly draft: DraftDetail };

export type DraftRenameResult =
  | { readonly status: 'unavailable' }
  | { readonly status: 'failed'; readonly failure: DraftsFailure }
  | { readonly status: 'ready'; readonly draft: DraftSummary };

export interface DraftsClient {
  list(): Promise<DraftListResult>;
  load(localId: string): Promise<DraftLoadResult>;
  /** «Rename» (P1-03F): the local name only; the server is not touched. */
  rename(localId: string, name: string): Promise<DraftRenameResult>;
}

/** Client without a backend: no data, an explicit `unavailable` state. */
export const unavailableDraftsClient: DraftsClient = {
  async list() {
    return { status: 'unavailable' };
  },
  async load() {
    return { status: 'unavailable' };
  },
  async rename() {
    return { status: 'unavailable' };
  }
};

function summaryOf(draft: DraftDetail): DraftSummary {
  const {
    pages: _pages,
    properties: _properties,
    target: _target,
    compatibility: _compatibility,
    checks: _checks,
    ...summary
  } = draft;
  return summary;
}

/** Longest local draft name, in Unicode code points (`MAX_DRAFT_NAME_CHARS` of the backend). */
export const MAX_DRAFT_NAME_CHARS = 255;

/** C0 and C1 control characters, which the backend refuses in names. */
export function hasControlCharacter(value: string): boolean {
  return [...value].some((character) => {
    const code = character.codePointAt(0) ?? 0;
    return code < 0x20 || (code >= 0x7f && code <= 0x9f);
  });
}

/** In-memory client for tests and the DEMO build; never wired into the production build. */
export function createMemoryDraftsClient(drafts: ReadonlyArray<DraftDetail>): DraftsClient {
  const byId = new Map(drafts.map((draft) => [draft.local_id, draft]));
  return {
    async list() {
      return { status: 'ready', drafts: [...byId.values()].map(summaryOf) };
    },
    async load(localId) {
      const draft = byId.get(localId);
      return draft ? { status: 'ready', draft } : { status: 'missing' };
    },
    async rename(localId: string, name: string): Promise<DraftRenameResult> {
      const draft = byId.get(localId);
      if (!draft) return { status: 'failed', failure: { code: 'workcopy_not_found', params: {} } };
      const trimmed = name.trim();
      if (trimmed === '' || [...trimmed].length > MAX_DRAFT_NAME_CHARS || hasControlCharacter(trimmed)) {
        return { status: 'failed', failure: { code: 'draft_name_invalid', params: { max: String(MAX_DRAFT_NAME_CHARS) } } };
      }
      const renamed: DraftDetail = { ...draft, name: trimmed, updated_at: Math.max(Date.now(), draft.updated_at) };
      byId.set(localId, renamed);
      return { status: 'ready', draft: summaryOf(renamed) };
    }
  };
}

/** Profile names for opaque connection IDs (P1-03F «Подключение»); unknown profiles stay unnamed. */
export function withConnectionNames<T extends DraftSummary>(draft: T, nameOf: (localId: string) => string | undefined): T {
  const sourceId = draft.source?.connection_local_id;
  const source = draft.source && sourceId !== undefined ? { ...draft.source, connection_name: nameOf(sourceId) } : draft.source;
  const created = draft.created_dashboards?.map((item) =>
    item.connection_local_id === undefined ? item : { ...item, connection_name: nameOf(item.connection_local_id) }
  );
  return { ...draft, source, created_dashboards: created };
}

export type KindFilter = 'all' | DraftKind;
export type OriginFilter = 'any' | DraftOrigin;

/** Newest first, as on «Drafts» (P1-03F) and «Recent» (P1-01A). */
export function sortByUpdated<T extends Pick<DraftSummary, 'updated_at' | 'local_id'>>(drafts: ReadonlyArray<T>): T[] {
  return [...drafts].sort((left, right) => right.updated_at - left.updated_at || left.local_id.localeCompare(right.local_id));
}

export function filterDrafts<T extends Pick<DraftSummary, 'kind' | 'origin'>>(
  drafts: ReadonlyArray<T>,
  kind: KindFilter,
  origin: OriginFilter
): T[] {
  return drafts.filter((draft) => (kind === 'all' || draft.kind === kind) && (origin === 'any' || draft.origin === origin));
}

export function kindCounts(drafts: ReadonlyArray<Pick<DraftSummary, 'kind'>>): Record<KindFilter, number> {
  return {
    all: drafts.length,
    dashboard: drafts.filter((draft) => draft.kind === 'dashboard').length,
    page: drafts.filter((draft) => draft.kind === 'page').length
  };
}

/** Search key: Unicode NFC, whitespace collapsed, case folded for the UI locale. */
export function searchKey(value: string, locale: string): string {
  return value.normalize('NFC').trim().replace(/\s+/gu, ' ').toLocaleLowerCase(locale);
}

/** «Search by name» (P1-03F): a case-insensitive substring of the user name; empty query keeps all. */
export function searchDrafts<T extends Pick<DraftSummary, 'name'>>(drafts: ReadonlyArray<T>, query: string, locale: string): T[] {
  const needle = searchKey(query, locale);
  if (needle === '') return [...drafts];
  return drafts.filter((draft) => searchKey(draft.name, locale).includes(needle));
}

/** The draft shown in the details column: the chosen one while visible, otherwise the first row. */
export function selectedDraft<T extends Pick<DraftSummary, 'local_id'>>(visible: ReadonlyArray<T>, selectedId: string | null): T | undefined {
  return visible.find((draft) => draft.local_id === selectedId) ?? visible[0];
}

export const RECENT_LIMIT = 5;

export function recentDrafts<T extends Pick<DraftSummary, 'updated_at' | 'local_id'>>(drafts: ReadonlyArray<T>): T[] {
  return sortByUpdated(drafts).slice(0, RECENT_LIMIT);
}

export function widgetCount(pages: ReadonlyArray<DraftPage>): number {
  return pages.reduce((total, page) => total + page.widgets.length, 0);
}

/** ISO timestamp for `<time datetime>`, or undefined for an invalid value. */
export function isoTime(epochMs: number): string | undefined {
  return Number.isFinite(epochMs) ? new Date(epochMs).toISOString() : undefined;
}

/**
 * Names shared by two or more drafts (P2-08A), as search keys: such drafts show their source
 * connection next to the name so that copies from different servers are not confused.
 */
export function duplicateNames(drafts: ReadonlyArray<Pick<DraftSummary, 'name'>>, locale: string): ReadonlySet<string> {
  const seen = new Set<string>();
  const shared = new Set<string>();
  for (const draft of drafts) {
    const key = searchKey(draft.name, locale);
    if (seen.has(key)) shared.add(key);
    seen.add(key);
  }
  return shared;
}
