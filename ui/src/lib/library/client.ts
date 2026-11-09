import { ApiFailure, apiFailure } from '../api';
import { CSRF_HEADER, loadCsrfToken } from '../session';

export interface CatalogSummary {
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

export interface CatalogMetric {
  id: string;
  name: string;
  value_type: string;
  units: string;
  origin: string;
}

export interface OfflinePage {
  local_id: string;
  created_at_ms: number;
  updated_at_ms: number;
  revision: number;
  catalog_versions: Array<{ catalog_id: string; version: number }>;
  recovery?: { may_have_unsaved_changes: boolean; last_saved_at_ms: number };
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

export interface OfflineDashboard {
  local_id: string;
  revision: number;
  bundle: {
    project: {
      kind: 'dashboard';
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
    list: () => request<CatalogSummary[]>(`${base}/catalogs`),
    catalog: (id: string, version: number) => request<{ metrics: CatalogMetric[] }>(`${base}/catalogs/${encodeURIComponent(id)}/${version}`),
    pages: () => request<OfflinePageSummary[]>(`${base}/offline-pages`),
    dashboards: () => request<Array<{ local_id: string; name: string; pages: number; widgets: number }>>(`${base}/offline-dashboards`),
    dashboard: (id: string) => request<OfflineDashboard>(`${base}/offline-dashboards/${encodeURIComponent(id)}`),
    dashboardPage: (id: string, pageId: string) => request<OfflinePage>(`${base}/offline-dashboards/${encodeURIComponent(id)}/pages/${encodeURIComponent(pageId)}`),
    createDashboard: (name: string, page_ids: string[]) => request<OfflineDashboard>(`${base}/offline-dashboards`, 'POST', { name, page_ids }),
    updateDashboard: (id: string, expected_revision: number, name: string, page_order: string[], append_page_ids: string[]) => request<OfflineDashboard>(`${base}/offline-dashboards/${encodeURIComponent(id)}`, 'POST', { expected_revision, name, page_order, append_page_ids }),
    prepareDashboardExport: (id: string) => request<{ id: string; display_name: string }>(`${base}/offline-dashboards/${encodeURIComponent(id)}/export`, 'POST'),
    page: (id: string) => request<OfflinePage>(`${pageBase}/${encodeURIComponent(id)}`),
    createPage: (page: NewOfflinePage) => request<OfflinePage>(`${base}/offline-pages`, 'POST', page),
    updatePage: (id: string, revision: number, page: NewOfflinePage) => request<OfflinePage>(`${pageBase}/${encodeURIComponent(id)}`, 'POST', { expected_revision: revision, page }, editHeaders(id)),
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
