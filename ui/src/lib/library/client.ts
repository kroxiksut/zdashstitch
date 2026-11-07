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

export type FilePreview = { file: string } & (
  | { status: 'ready'; catalogs: CatalogSummary[] }
  | { status: 'rejected'; code: string; line: number | null; column: number | null }
);

export interface CatalogPreview {
  id: string | null;
  files: FilePreview[];
}

export function createCatalogClient(fetcher: typeof fetch = (...args) => fetch(...args)) {
  const base = '/api/library';
  let csrf: Promise<string> | null = null;

  async function request<T>(path: string, method = 'GET', file?: File): Promise<T> {
    const headers: Record<string, string> = { Accept: 'application/json' };
    if (method === 'POST') {
      csrf ??= loadCsrfToken(fetcher).catch((error: unknown) => {
        csrf = null;
        throw error;
      });
      headers[CSRF_HEADER] = await csrf;
      headers['Content-Type'] = file ? 'application/octet-stream' : 'application/json';
    }
    const response = await fetcher(path, {
      method,
      headers,
      credentials: 'same-origin',
      body: method === 'POST' ? file ?? '{}' : undefined
    });
    if (!response.ok) throw await apiFailure(response);
    return (await response.json()) as T;
  }

  return {
    list: () => request<CatalogSummary[]>(`${base}/catalogs`),
    choose: () => request<CatalogPreview>(`${base}/template-import/choose`, 'POST'),
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
