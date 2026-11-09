import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import { createI18n } from '../i18n';
import EditorCanvas from './EditorCanvas.svelte';
import { selectWidget, widgetOrder } from './editor-selection';
import { moveSelection } from './editor-geometry';
import { PageClipboard, applyPaste, inspectPaste, type ClipboardContext } from './editor-clipboard';
import { compositionFindings } from './editor-composition';
import { EditorHistory } from './editor-history';
import type { NewOfflinePage } from './client';

function context(): ClipboardContext {
  const graph = (id: string, x: number): NewOfflinePage['widgets'][number] => ({ kind: 'graph', widget_id: id, title: id, layout: { mode: 'fixed', x, y: 0, w: 8, h: 4 }, metrics: [{ catalog_id: 'catalog', metric_id: 'metric', hosts: { role: 'source', selection: 'one' } }] });
  return { sourceId: 'page-a', page: { name: 'Page', catalogs: [{ catalog_id: 'catalog', version: 1 }], roles: { source: { cardinality: 'one', slots: ['source-01'] } }, widgets: [graph('first', 0), graph('second', 10), graph('neighbor', 40)] }, metrics: { catalog: [{ id: 'metric', name: 'Metric', value_type: 'numeric_float', units: '', origin: 'item' }] } };
}

describe('local page selection and group geometry', () => {
  it('toggles, ranges from the anchor and uses geometric order before Auto', () => {
    const ctx = context(); ctx.page.widgets.reverse();
    expect(widgetOrder(ctx.page.widgets)).toEqual(['first', 'second', 'neighbor']);
    expect(selectWidget(['first'], 'second', ['first', 'second', 'neighbor'], 'first', true, false).ids).toEqual(['first', 'second']);
    expect(selectWidget(['first', 'second'], 'first', [], null, true, false).ids).toEqual(['second']);
    expect(selectWidget([], 'neighbor', ['first', 'second', 'neighbor'], 'first', false, true).ids).toEqual(['first', 'second', 'neighbor']);
  });
  it('rejects the whole group at bounds or a neighbor and leaves input unchanged', () => {
    const ctx = context(); const before = structuredClone(ctx.page.widgets);
    expect(moveSelection(ctx.page.widgets, ['first', 'second'], -1, 0)).toBeNull();
    expect(moveSelection(ctx.page.widgets, ['first', 'second'], 30, 0)).toBeNull();
    expect(moveSelection(ctx.page.widgets, ['first', 'second'], 0, 5)?.map((change) => change.layout.y)).toEqual([5, 5]);
    expect(ctx.page.widgets).toEqual(before);
  });
});

describe('memory-only clipboard transaction', () => {
  it('allocates new IDs, keeps group offsets and neighbors, and undoes as one step', () => {
    const ctx = context(); const clipboard = new PageClipboard();
    const source = ctx.page.widgets[0]; if (source.kind !== 'graph') throw new Error('fixture');
    const color = '#' + (0x102030).toString(16);
    source.metrics[0].color = color;
    const before = structuredClone(ctx.page);
    const transactionHistory = new EditorHistory(before);
    expect(clipboard.copy(ctx, ['first', 'second'], false)).toEqual([]);
    expect(inspectPaste(clipboard.read(), ctx, 'original').issues).toEqual([{ code: 'geometry' }]);
    let counter = 0; const result = applyPaste(clipboard.read(), ctx, 'first_free', () => `copy-${++counter}`);
    expect(result.issues).toEqual([]); expect(result.ids).toEqual(['copy-1', 'copy-2']);
    expect(result.page?.widgets.slice(0, 3)).toEqual(ctx.page.widgets);
    const pasted = result.page!.widgets[3]; if (pasted.kind !== 'graph') throw new Error('fixture');
    expect(pasted.metrics[0].color).toBe(color);
    const boxes = result.page!.widgets.slice(3).map((widget) => widget.layout);
    expect(boxes).toEqual([{ mode: 'fixed', x: 18, y: 0, w: 8, h: 4 }, { mode: 'fixed', x: 28, y: 0, w: 8, h: 4 }]);
    transactionHistory.commit(result.page!, 'paste'); expect(transactionHistory.undo()).toEqual(ctx.page); expect(transactionHistory.undo()).toBeNull();
    expect(clipboard.read()?.widgets[0].widget_id).toBe('first');
  });
  it('stages cut without mutation, preserves IDs on same-page paste and blocks changed or foreign sources', () => {
    const ctx = context(); const before = structuredClone(ctx.page); const clipboard = new PageClipboard();
    clipboard.copy(ctx, ['first', 'second'], true); expect(ctx.page).toEqual(before);
    expect(inspectPaste(clipboard.read(), { ...ctx, sourceId: 'other' }, 'first_free').issues).toEqual([{ code: 'cross_page_cut' }]);
    ctx.page.widgets[0].layout = { mode: 'fixed', x: 1, y: 0, w: 8, h: 4 };
    expect(inspectPaste(clipboard.read(), ctx, 'original').issues).toEqual([{ code: 'source_changed' }]);
    ctx.page = before;
    const result = applyPaste(clipboard.read(), ctx, 'original', () => { throw new Error('cut must preserve IDs'); });
    expect(result.ids).toEqual(['first', 'second']); expect(result.page?.widgets).toHaveLength(3);
    clipboard.consumeCut(); expect(clipboard.read()).toBeNull();
  });
  it('copies across local pages only with matching pins, ordered roles and metric evidence', () => {
    const ctx = context(); const clipboard = new PageClipboard(); clipboard.copy(ctx, ['first'], false);
    const target = context(); target.sourceId = 'page-b'; target.page.widgets = [];
    expect(inspectPaste(clipboard.read(), target, 'original').issues).toEqual([]);
    target.page.catalogs[0].version = 2;
    expect(inspectPaste(clipboard.read(), target, 'original').issues).toContainEqual({ code: 'catalog', value: 'catalog' });
    target.page.catalogs[0].version = 1; target.page.roles.source.slots = ['changed'];
    expect(inspectPaste(clipboard.read(), target, 'original').issues).toContainEqual({ code: 'role', value: 'source' });
    target.page.roles.source.slots = ['source-01']; target.metrics.catalog[0].units = 'ms';
    expect(inspectPaste(clipboard.read(), target, 'original').issues).toContainEqual({ code: 'metric', value: 'metric' });
  });
  it('fails closed for an opaque member or nested reference without replacing the previous clipboard', () => {
    const ctx = context(); const clipboard = new PageClipboard(); clipboard.copy(ctx, ['first'], false); const old = clipboard.read();
    ctx.page.widgets.push({ kind: 'preserved', widget_id: 'opaque' });
    expect(clipboard.copy(ctx, ['first', 'opaque'], false)).toEqual([{ code: 'opaque' }]); expect(clipboard.read()).toEqual(old);
    const widget = ctx.page.widgets[0]; if (widget.kind !== 'graph') throw new Error('fixture');
    Object.assign(widget.metrics[0].hosts, { reference: 'producer' });
    expect(clipboard.copy(ctx, ['first'], false)).toEqual([{ code: 'reference' }]); expect(clipboard.read()).toEqual(old);
  });
  it('rejects ID collisions and a group that has no free area atomically', () => {
    const ctx = context(); const clipboard = new PageClipboard(); clipboard.copy(ctx, ['first'], false);
    expect(applyPaste(clipboard.read(), ctx, 'first_free', () => 'neighbor').issues).toEqual([{ code: 'ids' }]);
    ctx.page.widgets = [{ ...ctx.page.widgets[2], layout: { mode: 'fixed', x: 0, y: 0, w: 72, h: 64 } }];
    const before = structuredClone(ctx.page);
    expect(applyPaste(clipboard.read(), ctx, 'first_free', () => 'copy').issues).toEqual([{ code: 'geometry' }]);
    expect(ctx.page).toEqual(before);
  });
});

describe('shared local composition findings', () => {
  it('attaches one overlap finding to both widgets and projects it onto the canvas', () => {
    const ctx = context(); ctx.page.widgets[1].layout = ctx.page.widgets[0].layout;
    const findings = compositionFindings(ctx.page, ctx.metrics); const overlaps = findings.filter((finding) => finding.code === 'layout_overlap');
    expect(overlaps).toHaveLength(1); expect(overlaps[0].widget_ids).toEqual(['first', 'second']);
    const html = render(EditorCanvas, { props: { i18n: createI18n('en'), widgets: ctx.page.widgets, titles: ['First', 'Second', 'Neighbor'], selectedIds: ['first', 'second'], findings, onchange() {}, onremove() {}, onduplicate() {} } }).body;
    expect(html.match(/Widgets overlap\./g)).toHaveLength(2); expect(html).toContain('Selected widgets: 2');
    expect(findings.filter((finding) => finding.code === 'source_binding_required').every((finding) => finding.blocks.join() === 'publication')).toBe(true);
  });
});
