import { ApiFailure, apiFailure } from '../api';
import { CSRF_HEADER, loadCsrfToken } from '../session';

export interface CatalogSummary {
  library_id?: string | null;
  revision?: number;
  catalog_id: string;
  name: string;
  version: number;
  export_format_version: string | null;
  metrics: number;
  discovery_rules: number;
  metrics_with_hints: number;
  omitted_template_dashboards: number;
  diagnostics: string[];
}

export interface PackagePreview { file: string; sha256: string; size_bytes: number; entry_hashes: Record<string, string> | null; outcome: { status: 'ready' | 'invalid'; version?: 'v1' | 'v2'; kind?: 'page' | 'dashboard'; name?: string; pages?: number; widgets?: number; catalogs?: number; declared_compatibility?: { minimum: string; maximum_exclusive?: string }; registry_compatibility?: { knowledge: 'known' | 'unknown' | 'incompatible'; calculated: { minimum: string; maximum_exclusive?: string } | null; declared_outside_registry: boolean }; dashboards?: Array<{ id: string; name: string; pages: Array<{ id: string; name: string }> }> } }
export interface PackageDownload { id: string; display_name: string; sha256: string; size_bytes: number; entry_hashes: Record<string, string> }
export interface SensitiveScan { revision: number; findings: Array<{ kind: 'url' | 'email' | 'ip' | 'macro' | 'unknown' | 'metric_key' | 'mask' | 'path' | 'fqdn' | 'credential' | 'identifier' | 'free_text'; location: string; evidence: 'structural' | 'heuristic' | 'unanalysed' }>; truncated: boolean; unknown_objects: number; credential_candidates: number }
export type RedactionTarget = { kind: 'project_id' } | { kind: 'dashboard_id' } | { kind: 'page_id'; page_index: number } | { kind: 'widget_id'; page_index: number; widget_index: number } | { kind: 'dashboard_name' } | { kind: 'page_name'; page_index: number } | { kind: 'widget_name'; page_index: number; widget_index: number } | { kind: 'widget_host_mask'; page_index: number; widget_index: number; field_index: number } | { kind: 'role_alias'; role_index: number } | { kind: 'slot_alias'; role_index: number; slot_index: number } | { kind: 'role_description'; role_index: number } | { kind: 'widget_unresolved_name'; page_index: number; widget_index: number; field_index: number } | { kind: 'widget_text'; page_index: number; widget_index: number; field_index: number } | { kind: 'widget_item_pattern'; page_index: number; widget_index: number; field_index: number } | { kind: 'catalog_template_name'; catalog_index: number } | { kind: 'catalog_metric_name'; catalog_index: number; metric_index: number } | { kind: 'catalog_metric_units'; catalog_index: number; metric_index: number } | { kind: 'catalog_metric_id'; catalog_index: number; metric_index: number } | { kind: 'catalog_id'; catalog_index: number } | { kind: 'catalog_selector_aliases'; catalog_index: number } | { kind: 'catalog_clear_uuids'; catalog_index: number } | { kind: 'catalog_clear_source'; catalog_index: number } | { kind: 'clear_provenance' } | { kind: 'remove_page'; page_index: number } | { kind: 'remove_widget'; page_index: number; widget_index: number };
export interface RedactionChange { target: RedactionTarget; replacement: string; apply_to_matching?: boolean }
export interface RedactionPreview { revision: number; sha256: string; changes: Array<{ target: RedactionTarget; request_index: number; before_chars: number; after_chars: number; before_masked: string }>; remaining: Omit<SensitiveScan, 'revision'> }
export interface PackageSelection { file: string; dashboard_id: string | null; page_id: string | null }
export interface PackageImportResult { file: string; outcome: { status: 'saved' | 'failed'; local_id?: string; kind?: 'page' | 'dashboard' } }

export interface CatalogMetric {
  id: string;
  name: string;
  value_type: string;
  units: string;
  origin: string;
}

export type WidgetEvaluationProfile = '7.4' | '8.0';

export interface OfflineWidgetEvaluation {
  profile: WidgetEvaluationProfile | null;
  unrecognized: Array<{ widget_id: string; widget_type: string; unknown_type: boolean; unknown_field_count: number }>;
}

export interface OfflinePage {
  local_id: string;
  created_at_ms: number;
  updated_at_ms: number;
  revision: number;
  catalog_versions: Array<{ catalog_id: string; version: number }>;
  embedded_catalogs?: string[];
  recovery?: { may_have_unsaved_changes: boolean; last_saved_at_ms: number };
  widget_evaluation: OfflineWidgetEvaluation;
  bundle: {
    project: {
      kind: 'page';
      root: { id: string; name: string; widgets: PortablePageWidget[] };
      catalogs: Array<{ id: string; file: string }>;
      roles: Record<string, PageRole>;
    };
    catalogs: Record<string, { catalog_id: string; source: { template_name?: string; export_format_version?: string }; metrics: CatalogMetric[] }>;
  };
  composition: {
    slots_declared: boolean;
    instance_count: number;
    source_count: number;
    truncated: boolean;
    instances: Array<{
      widget_id: string;
      slot_id: string | null;
      sources: Array<{ catalog_id: string; metric_id: string; role: string; slot_id: string }>;
    }>;
  };
}

export interface OfflinePageSummary {
  local_id: string;
  name: string;
  created_at_ms: number;
  catalogs: number;
  widgets: number;
}

export interface LibraryPageSummary {
  library_id: string;
  source_local_id: string;
  saved_at_ms: number;
  name: string;
  widgets: number;
  revision?: number;
  catalog_versions?: Array<{ catalog_id: string; version: number }>;
}

export interface LibraryIndex {
  cache_write_failed?: boolean;
  catalogs: Array<{ library_id: string; revision: number; catalog_id: string; version: number; name: string; export_format_version: string | null; metrics: number; used_by: string[] }>;
  pages: Array<{ library_id: string; revision: number; source_local_id: string; saved_at_ms: number; name: string; widgets: number; catalog_versions: Array<[string, number]> }>;
  snapshots: SnapshotSummary[];
  damaged_snapshots: Array<{ library_id: string; code: string }>;
  snapshot_references_incomplete?: boolean;
}

export interface SnapshotSummary {
  library_id: string;
  connection_id: string;
  server_version: string;
  read_at_ms: number;
  used_by: string[];
}
export interface SnapshotComparison {
  identical: boolean;
  left_pages: number;
  right_pages: number;
  left_widgets: number;
  right_widgets: number;
}
export interface SnapshotDeletionPreview {
  library_id: string;
  workcopy_count: number;
  journal_count: number;
  references_unknown: boolean;
  deletable: boolean;
}

export interface CatalogDeletionPreview {
  library_id: string;
  revision: number;
  name: string;
  catalog_id: string;
  version: number;
  used_by: string[];
}

export interface OfflineDashboard {
  local_id: string;
  revision: number;
  bundle: {
    catalogs: Record<string, { source: { template_name?: string }; metrics: Array<{ name: string }> }>;
    project: {
      kind: 'dashboard';
      roles: Record<string, PageRole>;
      catalogs: Array<{ id: string; file: string }>;
      root: { id: string; name: string; pages: Array<{ id: string; name: string; widgets: PortablePageWidget[] }> };
    };
  };
}

export interface PageMetricChoice { catalog_id: string; metric_id: string }
export interface PageRole { cardinality: 'one' | 'many'; slots?: string[]; description?: string }
export interface PageHostSelection { role: string; selection: 'one' | 'all' | 'slot'; slot_id?: string }
export interface PageSeriesChoice extends PageMetricChoice { hosts: PageHostSelection; color?: string }
type PortableMetric = { catalog: string; metric: string };
export type PageLayout = { mode: 'auto'; w: number; h: number } | { mode: 'fixed'; x: number; y: number; w: number; h: number };
export type PortablePageWidget = ({ id: string; layout: PageLayout } & (
  | { type: 'zabbix'; name: string; widget_type: string; registry_state: 'known' | 'unknown'; fields: readonly unknown[] }
  | ({ title: string } & (
  | { type: 'graph'; expansion: 'series_per_host'; series: Array<{ metric: PortableMetric; hosts: PageHostSelection; color?: string }> }
  | { type: 'item_value'; expansion: 'single' | 'widget_per_host'; metric: PortableMetric; hosts: PageHostSelection }
  ))
));
export function portableWidgetTitle(widget: PortablePageWidget): string { return widget.type === 'zabbix' ? widget.name : widget.title; }
export interface NewOfflinePage {
  name: string;
  evaluation_profile?: WidgetEvaluationProfile;
  catalogs: Array<{ catalog_id: string; version: number }>;
  roles: Record<string, PageRole>;
  widgets: Array<
    | { kind: 'graph'; widget_id?: string; layout?: PageLayout; title: string; metrics: PageSeriesChoice[] }
    | { kind: 'item_value'; widget_id?: string; layout?: PageLayout; title: string; metric: PageMetricChoice; hosts: PageHostSelection; expansion: 'single' | 'widget_per_host' }
    | { kind: 'preserved'; widget_id: string; layout?: PageLayout }
  >;
}

export type FilePreview = { file: string } & (
  | { status: 'ready'; catalogs: CatalogSummary[] }
  | { status: 'rejected'; code: string; line: number | null; column: number | null }
);

export interface CatalogPreview {
  id: string | null;
  files: FilePreview[];
}

export function createCatalogClient(fetcher: typeof fetch = (...args) => fetch(...args), dashboardId?: string) {
  const base = '/api/library';
  const pageBase = dashboardId ? `${base}/offline-dashboards/${encodeURIComponent(dashboardId)}/pages` : `${base}/offline-pages`;
  let csrf: Promise<string> | null = null;
  const editorTokens = new Map<string, string>();
  const editHeaders = (id: string): Record<string, string> => {
    const token = editorTokens.get(id); return token ? { 'X-ZDashstitch-Editor': token } : {};
  };

  async function request<T>(path: string, method = 'GET', payload?: File | object, extraHeaders: Record<string, string> = {}, keepalive = false): Promise<T> {
    const headers: Record<string, string> = { Accept: 'application/json', ...extraHeaders };
    if (method === 'POST') {
      csrf ??= loadCsrfToken(fetcher).catch((error: unknown) => {
        csrf = null;
        throw error;
      });
      headers[CSRF_HEADER] = await csrf;
      headers['Content-Type'] = payload instanceof File ? 'application/octet-stream' : 'application/json';
    }
    const response = await fetcher(path, {
      method,
      headers,
      credentials: 'same-origin',
      keepalive,
      body: method === 'POST' ? payload instanceof File ? payload : JSON.stringify(payload ?? {}) : undefined
    });
    if (!response.ok) throw await apiFailure(response);
    return (await response.json()) as T;
  }

  return {
    index: () => request<LibraryIndex>(`${base}/index`),
    snapshots: () => request<{ snapshots: SnapshotSummary[]; damaged: Array<{ library_id: string; code: string }>; references_incomplete: boolean }>(`${base}/snapshots`),
    compareSnapshots: (left: string, right: string) => request<SnapshotComparison>(`${base}/snapshots/compare`, 'POST', { left, right }),
    snapshotDeletePreview: (id: string) => request<SnapshotDeletionPreview>(`${base}/snapshots/${encodeURIComponent(id)}/delete-preview`),
    deleteSnapshot: (id: string) => request<{ deleted: boolean }>(`${base}/snapshots/${encodeURIComponent(id)}/delete`, 'POST', { confirmed: true }),
    list: () => request<CatalogSummary[]>(`${base}/catalogs`),
    catalog: (id: string, version: number) => request<{ metrics: CatalogMetric[] }>(`${base}/catalogs/${encodeURIComponent(id)}/${version}`),
    pages: () => request<OfflinePageSummary[]>(`${base}/offline-pages`),
    libraryPages: () => request<LibraryPageSummary[]>(`${base}/pages`),
    duplicateLibraryPage: (id: string, expected_revision: number) => request<LibraryPageSummary>(`${base}/pages/${encodeURIComponent(id)}/duplicate`, 'POST', { expected_revision }),
    renameLibraryPage: (id: string, expected_revision: number, display_name: string) => request<LibraryPageSummary>(`${base}/pages/${encodeURIComponent(id)}/rename`, 'POST', { expected_revision, display_name }),
    deleteLibraryPage: (id: string, expected_revision: number, confirmed_name: string) => request<{}>(`${base}/pages/${encodeURIComponent(id)}/delete`, 'POST', { expected_revision, confirmed_name }),
    duplicateCatalog: (id: string, expected_revision: number) => request<CatalogSummary>(`${base}/catalogs/${encodeURIComponent(id)}/duplicate`, 'POST', { expected_revision }),
    renameCatalog: (id: string, expected_revision: number, display_name: string) => request<CatalogSummary>(`${base}/catalogs/${encodeURIComponent(id)}/rename`, 'POST', { expected_revision, display_name }),
    catalogDeletePreview: (id: string) => request<CatalogDeletionPreview>(`${base}/catalogs/${encodeURIComponent(id)}/delete-preview`),
    deleteCatalog: (id: string, expected_revision: number, confirmed_name: string) => request<{}>(`${base}/catalogs/${encodeURIComponent(id)}/delete`, 'POST', { expected_revision, confirmed_name }),
    takeLibraryPage: (id: string) => request<OfflinePage>(`${base}/pages/${encodeURIComponent(id)}/take`, 'POST'),
    savePageToLibrary: (id: string) => request<LibraryPageSummary>(`${base}/pages/from-draft/${encodeURIComponent(id)}`, 'POST'),
    saveDashboardPageToLibrary: (dashboard: string, page: string) => request<LibraryPageSummary>(`${base}/pages/from-dashboard/${encodeURIComponent(dashboard)}/${encodeURIComponent(page)}`, 'POST'),
    dashboards: () => request<Array<{ local_id: string; name: string; pages: number; widgets: number }>>(`${base}/offline-dashboards`),
    dashboard: (id: string) => request<OfflineDashboard>(`${base}/offline-dashboards/${encodeURIComponent(id)}`),
    dashboardPage: (id: string, pageId: string) => request<OfflinePage>(`${base}/offline-dashboards/${encodeURIComponent(id)}/pages/${encodeURIComponent(pageId)}`),
    createDashboard: (name: string, page_ids: string[]) => request<OfflineDashboard>(`${base}/offline-dashboards`, 'POST', { name, page_ids }),
    updateDashboard: (id: string, expected_revision: number, name: string, page_order: string[], append_page_ids: string[]) => request<OfflineDashboard>(`${base}/offline-dashboards/${encodeURIComponent(id)}`, 'POST', { expected_revision, name, page_order, append_page_ids }),
    prepareDashboardExport: (id: string, revision?: number) => request<PackageDownload>(`${base}/offline-dashboards/${encodeURIComponent(id)}/export`, 'POST', { expected_revision: revision }),
    createExportJob: () => request<{ id: string }>(`${base}/export-jobs`, 'POST', {}),
    exportJobStatus: (id: string) => request<{ phase: 'created' | 'choosing' | 'preparing' | 'scanning' | 'transforming' | 'validating' | 'packaging' | 'verifying' | 'writing' | 'committing' | 'done' | 'failed' | 'cancelled'; cancellable: boolean }>(`${base}/export-jobs/${encodeURIComponent(id)}`),
    cancelExportJob: (id: string) => request<{ cancelled: boolean }>(`${base}/export-jobs/${encodeURIComponent(id)}/cancel`, 'POST', {}),
    saveExport: (kind: 'page' | 'dashboard', id: string, expected_revision: number, yaml: boolean, job_id: string) => request<{ cancelled: boolean; path?: string; sha256?: string; size_bytes?: number; entry_hashes?: Record<string, string> }>(`${base}/${kind === 'page' ? 'offline-pages' : 'offline-dashboards'}/${encodeURIComponent(id)}/save-export`, 'POST', { expected_revision, yaml, job_id }),
    preparePageExport: (id: string, revision?: number) => request<PackageDownload>(`${base}/offline-pages/${encodeURIComponent(id)}/export`, 'POST', { expected_revision: revision }),
    prepareDashboardYamlExport: (id: string, revision?: number) => request<PackageDownload>(`${base}/offline-dashboards/${encodeURIComponent(id)}/export-yaml`, 'POST', { expected_revision: revision }),
    preparePageYamlExport: (id: string, revision?: number) => request<PackageDownload>(`${base}/offline-pages/${encodeURIComponent(id)}/export-yaml`, 'POST', { expected_revision: revision }),
    scanPackage: (kind: 'page' | 'dashboard', id: string) => request<SensitiveScan>(`${base}/${kind === 'page' ? 'offline-pages' : 'offline-dashboards'}/${encodeURIComponent(id)}/sensitive-preview`),
    previewRedaction: (kind: 'page' | 'dashboard', id: string, expected_revision: number, changes: RedactionChange[]) => request<RedactionPreview>(`${base}/${kind === 'page' ? 'offline-pages' : 'offline-dashboards'}/${encodeURIComponent(id)}/redaction-preview`, 'POST', { expected_revision, changes }),
    saveRedaction: (kind: 'page' | 'dashboard', id: string, expected_revision: number, changes: RedactionChange[], preview_sha256: string, job_id: string) => request<{ cancelled: boolean; path?: string; sha256?: string; size_bytes?: number }>(`${base}/${kind === 'page' ? 'offline-pages' : 'offline-dashboards'}/${encodeURIComponent(id)}/redaction-save`, 'POST', { expected_revision, changes, preview_sha256, acknowledged: true, job_id }),
    exportRedaction: (kind: 'page' | 'dashboard', id: string, expected_revision: number, changes: RedactionChange[], preview_sha256: string) => request<PackageDownload>(`${base}/${kind === 'page' ? 'offline-pages' : 'offline-dashboards'}/${encodeURIComponent(id)}/redaction-export`, 'POST', { expected_revision, changes, preview_sha256, acknowledged: true }),
    page: (id: string) => request<OfflinePage>(`${pageBase}/${encodeURIComponent(id)}`),
    createPage: (page: NewOfflinePage) => request<OfflinePage>(`${base}/offline-pages`, 'POST', page),
    updatePage: (id: string, revision: number, page: NewOfflinePage, confirmedRemovals: string[] = []) =>
      request<OfflinePage>(`${pageBase}/${encodeURIComponent(id)}`, 'POST', {
        expected_revision: revision, page,
        ...(confirmedRemovals.length ? { confirmed_widget_removals: confirmedRemovals } : {})
      }, editHeaders(id)),
    async acquirePage(id: string, takeOver = false): Promise<'editing' | 'read_only'> {
      const result = await request<{ state: 'editing'; token: string } | { state: 'read_only' }>(`${pageBase}/${encodeURIComponent(id)}/edit-session`, 'POST', { take_over: takeOver });
      if (result.state === 'editing') editorTokens.set(id, result.token); else editorTokens.delete(id);
      return result.state;
    },
    renewPage: (id: string) => request(`${pageBase}/${encodeURIComponent(id)}/edit-session/renew`, 'POST', {}, editHeaders(id)),
    releasePage(id: string) {
      const headers = editHeaders(id); editorTokens.delete(id);
      return request(`${pageBase}/${encodeURIComponent(id)}/edit-session/release`, 'POST', {}, headers, true);
    },
    async inspectPackages(files: readonly File[]): Promise<{ id: string; files: PackagePreview[] }> {
      const { id } = await request<{ id: string }>('/api/upload-sets', 'POST');
      const encoded = encodeURIComponent(id);
      for (const file of files) await request(`/api/uploads/${encoded}/files/${encodeURIComponent(file.name)}`, 'POST', file);
      return { id, files: await request<PackagePreview[]>(`${base}/package-import/${encoded}/inspect`, 'POST') };
    },
    importPackages: (id: string, selections: PackageSelection[]) => request<PackageImportResult[]>(`${base}/package-import/${encodeURIComponent(id)}/run`, 'POST', { selections }),
    async inspect(files: readonly File[]): Promise<CatalogPreview> {
      const { id } = await request<{ id: string }>('/api/upload-sets', 'POST');
      const encoded = encodeURIComponent(id);
      const rejected: FilePreview[] = [];
      let uploaded = 0;
      for (const file of files) {
        if (file.size > 2 * 1024 * 1024) {
          rejected.push({ file: file.name, status: 'rejected', code: 'template_export_size', line: null, column: null });
          continue;
        }
        try {
          await request(`/api/uploads/${encoded}/files/${encodeURIComponent(file.name)}`, 'POST', file);
          uploaded++;
        } catch (error) {
          const code = error instanceof ApiFailure
            ? error.response?.code ?? 'request_failed'
            : 'request_failed';
          rejected.push({ file: file.name, status: 'rejected', code, line: null, column: null });
        }
      }
      const parsed = uploaded > 0
        ? await request<FilePreview[]>(`${base}/template-import/${encoded}/inspect`, 'POST')
        : [];
      return { id: uploaded > 0 ? id : null, files: [...parsed, ...rejected] };
    },
    run: (id: string) =>
      request<CatalogSummary[]>(`${base}/template-import/${encodeURIComponent(id)}/run`, 'POST')
  };
}
