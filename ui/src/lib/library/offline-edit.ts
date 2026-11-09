import type { NewOfflinePage, OfflinePage, CatalogSummary } from './client';
import { ApiFailure } from '../api';

/** Reconcile a lost local HTTP response by reading, never by a blind second write. */
export async function saveKnownPage(client: {
  updatePage: (id: string, revision: number, content: NewOfflinePage) => Promise<OfflinePage>;
  page: (id: string) => Promise<OfflinePage>;
}, before: OfflinePage, content: NewOfflinePage): Promise<OfflinePage> {
  try { return await client.updatePage(before.local_id, before.revision, content); }
  catch (failure) {
    if (failure instanceof ApiFailure) throw failure;
    const latest = await client.page(before.local_id);
    if (latest.revision === before.revision + 1 && canonicalEditorState(pageRequestFromSaved(latest)) === canonicalEditorState(content)) return latest;
    if (latest.revision !== before.revision) throw new ApiFailure(409, { code: 'offline_page_conflict', params: {} });
    throw failure;
  }
}

export function canonicalEditorState(value: unknown): string {
  return JSON.stringify(value, (_key, item: unknown) => item !== null && typeof item === 'object' && !Array.isArray(item)
    ? Object.fromEntries(Object.entries(item).sort(([left], [right]) => left.localeCompare(right))) : item);
}

/** Opaque fields stay on the server; an explicit geometry edit never rewrites them. */
export function pageRequestFromSaved(page: OfflinePage): NewOfflinePage {
  return {
    name: page.bundle.project.root.name,
    catalogs: page.catalog_versions.map((pin) => ({ ...pin })),
    roles: Object.fromEntries(Object.entries(page.bundle.project.roles).map(([id, role]) => [id, { ...role, ...(role.slots ? { slots: [...role.slots] } : {}) }])),
    widgets: page.bundle.project.root.widgets.map((widget) => {
      if (widget.type === 'graph') return {
        kind: 'graph', widget_id: widget.id, layout: { ...widget.layout }, title: widget.title,
        metrics: widget.series.map((series) => ({
          catalog_id: series.metric.catalog, metric_id: series.metric.metric,
          hosts: { ...series.hosts }, ...(series.color === undefined ? {} : { color: series.color })
        }))
      };
      if (widget.type === 'item_value') return {
        kind: 'item_value', widget_id: widget.id, layout: { ...widget.layout }, title: widget.title,
        metric: { catalog_id: widget.metric.catalog, metric_id: widget.metric.metric },
        hosts: { ...widget.hosts }, expansion: widget.expansion
      };
      return { kind: 'preserved', widget_id: widget.id, layout: { ...widget.layout } };
    })
  };
}

/** Pinned catalog copies keep the form useful even when the library is unavailable. */
export function savedCatalogChoices(page: OfflinePage): CatalogSummary[] {
  return page.catalog_versions.map((pin) => {
    const catalog = page.bundle.catalogs[pin.catalog_id];
    if (!catalog) throw new Error('offline_page_missing_catalog');
    return {
      ...pin, name: catalog.source.template_name ?? pin.catalog_id,
      export_format_version: catalog.source.export_format_version ?? null,
      metrics: catalog.metrics.length, discovery_rules: 0, metrics_with_hints: 0,
      omitted_template_dashboards: 0, diagnostics: []
    };
  });
}
