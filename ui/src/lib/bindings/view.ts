// Display helpers of the ReplaceHost screens: names of pages and widgets from the preview, and
// field values as server data («было → станет»). Server data is shown as is, never translated.

import type { I18n } from '../i18n';
import { displayName, numberedPageName, widgetTypeName } from '../shell/format';
import type { BindingRow, FieldValue, ItemFacts, PageDiff, ReplaceHostPreview, WidgetDiff } from './model';

export function pageOf(preview: ReplaceHostPreview, pageIndex: number): PageDiff | undefined {
  return preview.diff.pages.find((page) => page.page_index === pageIndex);
}

export function widgetOf(preview: ReplaceHostPreview, pageIndex: number, widgetIndex: number): WidgetDiff | undefined {
  return pageOf(preview, pageIndex)?.widgets.find((widget) => widget.widget_index === widgetIndex);
}

/** «2. Базы данных» — page position (1-based in the draft) and name. */
export function pageLabel(i18n: I18n, preview: ReplaceHostPreview, pageIndex: number): string {
  const position = preview.diff.pages.findIndex((page) => page.page_index === pageIndex);
  const page = pageOf(preview, pageIndex);
  return numberedPageName(i18n, page?.name ?? '', position + 1);
}

/** Widget name, or its type name when the name is empty. */
export function widgetLabel(i18n: I18n, widget: Pick<WidgetDiff, 'name' | 'widget_type'> | undefined, fallbackType = ''): string {
  const type = widget?.widget_type ?? fallbackType;
  return displayName(widget?.name ?? '', widgetTypeName(i18n, type));
}

function itemText(item: ItemFacts | null, itemid: string): string {
  return item ? item.key : itemid;
}

/** A field value as server data: item key, host name, host pattern or a technical ID. */
export function valueText(preview: ReplaceHostPreview, value: FieldValue | null, item: ItemFacts | null = null): string {
  if (value === null) return '—';
  switch (value.kind) {
    case 'item':
    case 'item_prototype':
      return itemText(item, value.itemid);
    case 'host': {
      const operation = preview.operation;
      if (value.hostid === operation.source_host.hostid) return operation.source_host.name;
      if (operation.target_host && value.hostid === operation.target_host.hostid) return operation.target_host.name;
      return value.hostid;
    }
    case 'host_pattern':
      return value.pattern;
    case 'graph':
    case 'graph_prototype':
      return value.graphid;
  }
}

export function sourceText(preview: ReplaceHostPreview, row: BindingRow): string {
  return valueText(preview, row.source, row.source_item);
}

export function targetText(preview: ReplaceHostPreview, row: BindingRow): string {
  return valueText(preview, row.target, row.target_item);
}

export function sourceHostName(preview: ReplaceHostPreview): string {
  return preview.operation.source_host.name;
}

export function targetHostName(preview: ReplaceHostPreview): string | null {
  return preview.operation.target_host?.name ?? null;
}

/** Search key for the diff: widget name, type, field codes and values. */
export function widgetSearchText(i18n: I18n, preview: ReplaceHostPreview, page: PageDiff, widget: WidgetDiff): string {
  const rows = preview.rows.filter((row) => row.key.page_index === page.page_index && row.key.widget_index === widget.widget_index);
  return [
    widget.name,
    widget.widget_type,
    widgetTypeName(i18n, widget.widget_type),
    ...rows.flatMap((row) => [row.key.field, sourceText(preview, row), targetText(preview, row)])
  ]
    .join(' ')
    .toLocaleLowerCase(i18n.locale);
}
