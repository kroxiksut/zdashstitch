import type { LocalizedText } from '../components';
import type { CatalogMetric, NewOfflinePage } from './client';
import { overlaps, validBox } from './editor-geometry';
import { singleSource, validSource } from './offline-roles';

export interface EditorFinding {
  id: string;
  code: string;
  widget_ids: string[];
  level: 'blocking' | 'warning';
  blocks: Array<'export' | 'publication'>;
  message: LocalizedText;
}

/** Counts logical graph rows without resolving slots to Zabbix hosts or items. */
export function graphRowCount(sources: Array<{ hosts: { role: string; selection: string } }>, roles: NewOfflinePage['roles']): number {
  return sources.reduce((count, source) => count + (source.hosts.selection === 'all'
    ? (roles[source.hosts.role]?.slots?.length ?? 0) : 1), 0);
}

/** One UI projection for canvas, outline and Checks; the server revalidates saves. */
export function compositionFindings(page: NewOfflinePage, metrics: Record<string, CatalogMetric[]>): EditorFinding[] {
  const findings: EditorFinding[] = [];
  const add = (code: string, ids: string[], message: LocalizedText, blocks: EditorFinding['blocks'] = ['export', 'publication']): void => {
    const id = `${code}/${ids.join('/')}`;
    if (!findings.some((finding) => finding.id === id)) findings.push({ id, code, widget_ids: ids, level: blocks.length ? 'blocking' : 'warning', blocks, message });
  };
  if (!page.name.trim()) add('name_empty', [], { id: 'offline_editor.check.name' });
  if (!page.widgets.length) add('page_empty', [], { id: 'offline_editor.check.empty' }, []);
  const ids = new Set<string>();
  for (const [index, widget] of page.widgets.entries()) {
    const id = widget.widget_id ?? `widget-index-${index}`;
    if (ids.has(id)) add('duplicate_widget', [id], { id: 'offline_editor.check.ids' });
    ids.add(id);
    const box = widget.layout;
    if (box?.mode === 'fixed') {
      if (!validBox(box, [], -1)) add('layout_bounds', [id], { id: 'offline_editor.check.bounds' });
      for (let prior = 0; prior < index; prior++) {
        const other = page.widgets[prior];
        if (other.layout?.mode === 'fixed' && overlaps(box, other.layout)) add('layout_overlap', [other.widget_id ?? `widget-index-${prior}`, id], { id: 'offline_editor.check.overlap' });
      }
    }
    if (widget.kind === 'preserved') { add('opaque_read_only', [id], { id: 'offline_editor.opaque' }, []); continue; }
    const sources = widget.kind === 'graph' ? widget.metrics : [{ ...widget.metric, hosts: widget.hosts }];
    if (!sources.length) add('widget_incomplete', [id], { id: 'offline_editor.check.incomplete' });
    if (widget.kind === 'graph') {
      const units = new Set(sources.map((source) => metrics[source.catalog_id]?.find((metric) => metric.id === source.metric_id)?.units).filter((unit): unit is string => unit !== undefined));
      if (units.size > 1) add('graph_mixed_units', [id], { id: 'offline_editor.check.mixed_units' }, []);
    }
    for (const source of sources) {
      if (!page.catalogs.some((pin) => pin.catalog_id === source.catalog_id)) add('catalog_missing', [id], { id: 'offline_editor.check.catalog', params: { catalog: source.catalog_id } });
      const metric = metrics[source.catalog_id]?.find((metric) => metric.id === source.metric_id);
      if (!metric) add('unknown_metric', [id], { id: 'offline_editor.check.metric', params: { metric: source.metric_id } });
      else if ((widget.kind === 'graph' && !['numeric_float', 'numeric_unsigned'].includes(metric.value_type)) || metric.value_type === 'binary') add('metric_type', [id], { id: 'offline_editor.check.metric_type' });
      if (!validSource(source.hosts, page.roles) || (widget.kind === 'item_value' && (widget.expansion === 'single' ? !singleSource(source.hosts, page.roles) : source.hosts.selection !== 'all'))) add('selector_invalid', [id], { id: 'offline_editor.check.role', params: { role: source.hosts.role } });
    }
    if (sources.length) add('source_binding_required', [id], { id: 'offline_editor.check.binding' }, ['publication']);
  }
  return findings;
}
