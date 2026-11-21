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
export type ReusableWidgetType = 'item' | 'svggraph';
export type ReusableOrigin = 'builtin' | 'file' | 'local';
export type ReusableKind = 'style_profile' | 'widget_blueprint';
export type ReusableCompatibility = 'compatible' | 'partial' | 'incompatible';
export interface ReusableMetadata { name: string; description?: string; tags?: string[] }
interface ReusableCommon extends ReusableMetadata {
  adapter: string;
  version: number;
  declared_compatibility?: { minimum: string; maximum_exclusive?: string };
}
export interface ReusableStyle extends ReusableCommon {
  format: 'zdashstitch.style-profile/v1';
  profile_id: string;
  widget_types: Partial<Record<ReusableWidgetType, {
    series_palette?: string[]; color_palette?: number; widget_fields?: Record<string, unknown>; [other: string]: unknown;
  }>>;
}
export interface BlueprintRole { param: string; cardinality: 'one' | 'many'; slots?: string[]; description?: string }
export interface BlueprintMetric { param: string; value_type?: string; units?: string; description?: string }
export interface ReusableBlueprint extends ReusableCommon {
  format: 'zdashstitch.widget-blueprint/v1';
  blueprint_id: string;
  widget_type: ReusableWidgetType;
  parameters: { roles: BlueprintRole[]; metrics: BlueprintMetric[] };
}
export type ReusableAsset = { kind: 'style_profile'; document: ReusableStyle }
  | { kind: 'widget_blueprint'; document: ReusableBlueprint };
export interface ReusableEntry { revision: number; origin: ReusableOrigin; asset: ReusableAsset }
export interface ReusableImport { name: string; kind: ReusableKind; adapter: string; version: number; existing: [number, ReusableOrigin] | null }
export type ReusableImportMode = { mode: 'new' | 'copy' } | { mode: 'replace'; expected_revision: number };
export type RoleAssignment = { role: string; slots: Record<string, string> };
export type MetricAssignment = { catalog: string; metric: string };
export interface ReusableAction {
  asset_id: string; asset_revision: number; expected_revision: number;
  operation: { kind: 'style'; widget_ids: string[]; mode: 'replace' | 'keep_local' | 'merge'; entire_dashboard?: boolean }
    | { kind: 'insert'; roles: Record<string, RoleAssignment>; metrics: Record<string, MetricAssignment | null> };
}
export type ReusablePreview = { kind: 'style'; widgets: Array<{ page_id: string; page_name: string; widget_id: string; widget_name: string; state: ReusableCompatibility; changes: Array<{ name: string; before: string | null; after: string }> }> }
  | { kind: 'insert'; unresolved: number };
export interface CopyStyleState {
  revision: number;
  has_style: boolean;
  undo: boolean;
  publication: { base_fingerprint: string; local_fingerprint: string; changes: CopyStyleChange[] } | null;
}
export interface CopyStyleRequest { expected_revision: number; profile_id: string; profile_revision: number; page: number; widgets?: number[] }
export interface CopyStyleChange { page: number; widget: number; field: string; before: string; after: string }
export interface CopyStylePreview { revision: number; skipped: number; changes: CopyStyleChange[] }
export type CopyBlueprintMetric = { catalog_id: string; version: number; metric_id: string };
export interface CopyBlueprintRequest {
  expected_revision: number; blueprint_id: string; blueprint_revision: number; page_id: string;
  rebind_widget?: string;
  roles: Record<string, { host: string | null; slots: Record<string, string | null> }>;
  metrics: Record<string, CopyBlueprintMetric | null>;
  x: number; y: number;
}
export interface CopyBlueprintPreview {
  revision: number;
  widget: { id: string; name: string; x: number; y: number; width: number; height: number };
  unresolved: string[];
  fingerprint: string;
}
export interface CopyBlueprintState {
  revision: number;
  has_blueprint: boolean;
  publication: { base_fingerprint: string; local_fingerprint: string; page_name: string; widget: CopyBlueprintPreview['widget'] } | null;
}
export const reusableId = (asset: ReusableAsset): string => asset.kind === 'style_profile' ? asset.document.profile_id : asset.document.blueprint_id;
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
  reusable?: { marks: Record<string, { profile_id: string; profile_version: number; profile_name: string; fields: Record<string, string>; applied_at_ms: number }>; undo: boolean; redo: boolean };
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
    copyStyleState: (id: string) => request<CopyStyleState>(`/api/drafts/${encodeURIComponent(id)}/reusable-style`),
    copyBlueprintState: (id: string) => request<CopyBlueprintState>(`/api/drafts/${encodeURIComponent(id)}/reusable-blueprint`),
    copyBlueprint: (id: string, action: CopyBlueprintRequest, approved_fingerprint?: string, allow_unbound = false) =>
      request<CopyBlueprintPreview>(`/api/drafts/${encodeURIComponent(id)}/reusable-blueprint`, 'POST', { action, approved_fingerprint, allow_unbound }),
    copyBlueprintHosts: (id: string, search: string) =>
      request<{ hosts: Array<{ hostid: string; name: string }>; truncated: boolean }>(
        `/api/drafts/${encodeURIComponent(id)}/reusable-blueprint-hosts`, 'POST', { search }),
    copyStyle: (id: string, action: CopyStyleRequest, apply: boolean) =>
      request<CopyStylePreview>(`/api/drafts/${encodeURIComponent(id)}/reusable-style`, 'POST', { action, apply }),
    undoCopyStyle: (id: string, expected_revision: number) =>
      request<CopyStyleState>(`/api/drafts/${encodeURIComponent(id)}/reusable-style-undo`, 'POST', { expected_revision }),
    reusable: () => request<ReusableEntry[]>(`${base}/reusable`),
    createReusable: (asset: ReusableAsset) => request<ReusableEntry>(`${base}/reusable`, 'POST', asset),
    renameReusable: (id: string, expected_revision: number, metadata: ReusableMetadata) =>
      request<ReusableEntry>(`${base}/reusable/${encodeURIComponent(id)}/metadata`, 'POST', { expected_revision, metadata }),
    updateReusableStyle: (document: ReusableStyle, expected_revision: number) =>
      request<ReusableEntry>(`${base}/reusable/${encodeURIComponent(document.profile_id)}/style`, 'POST', { expected_revision, document }),
    duplicateReusable: (id: string, expected_revision: number) =>
      request<ReusableEntry>(`${base}/reusable/${encodeURIComponent(id)}/duplicate`, 'POST', { expected_revision }),
    deleteReusable: (id: string, expected_revision: number) =>
      request<{}>(`${base}/reusable/${encodeURIComponent(id)}/delete`, 'POST', { expected_revision }),
    scanReusable: (id: string) => request<{ revision: number; scan: Omit<SensitiveScan, 'revision'> }>(`${base}/reusable/${encodeURIComponent(id)}/scan`),
    exportReusable: (id: string, expected_revision: number) =>
      request<{ id: string; display_name: string }>(`${base}/reusable/${encodeURIComponent(id)}/export`, 'POST', { expected_revision, acknowledged: true }),
    saveReusable: (id: string, expected_revision: number) =>
      request<{ cancelled: boolean }>(`${base}/reusable/${encodeURIComponent(id)}/save`, 'POST', { expected_revision, acknowledged: true }),
    chooseReusable: () => request<{ id: string; preview: ReusableImport } | { id: null }>(`${base}/reusable/import-choose`, 'POST'),
    async inspectReusable(file: File): Promise<{ id: string; preview: ReusableImport }> {
      const { id } = await request<{ id: string }>('/api/upload-sets', 'POST');
      const encoded = encodeURIComponent(id);
      await request(`/api/uploads/${encoded}/files/${encodeURIComponent(file.name)}`, 'POST', file);
      return request<{ id: string; preview: ReusableImport }>(`${base}/reusable/import/${encoded}/inspect`, 'POST');
    },
    runReusableImport: (id: string, mode: ReusableImportMode) =>
      request<ReusableEntry>(`${base}/reusable/import/${encodeURIComponent(id)}/run`, 'POST', mode),
    reusableAction: (id: string, action: ReusableAction, apply: boolean) =>
      request<{ preview: ReusablePreview; page?: OfflinePage }>(`${pageBase}/${encodeURIComponent(id)}/reusable`, 'POST', { action, apply }, editHeaders(id)),
    reusableHistory: (id: string, expected_revision: number, redo: boolean) =>
      request<OfflinePage>(`${pageBase}/${encodeURIComponent(id)}/reusable-history`, 'POST', { expected_revision, action: { kind: 'switch', redo } }, editHeaders(id)),
    detachReusable: (id: string, expected_revision: number, widget_id: string) =>
      request<OfflinePage>(`${pageBase}/${encodeURIComponent(id)}/reusable-history`, 'POST', { expected_revision, action: { kind: 'detach', widget_id } }, editHeaders(id)),
    extractReusable: (id: string, expected_revision: number, widget_id: string, name: string, kind: ReusableKind) =>
      request<ReusableEntry>(`${pageBase}/${encodeURIComponent(id)}/reusable-extract`, 'POST', { expected_revision, widget_id, name, kind }),
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
