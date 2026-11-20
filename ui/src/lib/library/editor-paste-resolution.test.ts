import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import { createI18n } from '../i18n';
import { PageClipboard, type ClipboardContext } from './editor-clipboard';
import { applyResolvedPaste, initialResolution, inspectResolvedPaste, resolveClipboard, type PasteResolution } from './editor-paste-resolution';
import PasteResolutionForm from './PasteResolutionForm.svelte';
import { compositionFindings } from './editor-composition';
import { EditorHistory } from './editor-history';

function fixture() {
  const source: ClipboardContext = { sourceId: 'source-page', page: { name: 'Source', catalogs: [{ catalog_id: 'source-catalog', version: 1 }], roles: { source: { cardinality: 'many', slots: ['source-a', 'source-b'] } }, widgets: [
    { kind: 'graph', widget_id: 'graph-id', title: 'Graph', metrics: [{ catalog_id: 'source-catalog', metric_id: 'source-metric', hosts: { role: 'source', selection: 'all' } }], layout: { mode: 'fixed', x: 0, y: 0, w: 8, h: 4 } },
    { kind: 'item_value', widget_id: 'item-id', title: 'Value', metric: { catalog_id: 'source-catalog', metric_id: 'source-metric' }, hosts: { role: 'source', selection: 'slot', slot_id: 'source-b' }, expansion: 'single', layout: { mode: 'fixed', x: 10, y: 0, w: 8, h: 4 } }
  ] }, metrics: { 'source-catalog': [{ id: 'source-metric', name: 'Source metric', value_type: 'numeric_float', units: 'ms', origin: 'template_item' }] } };
  const target: ClipboardContext = { sourceId: 'target-page', page: { name: 'Target', catalogs: [{ catalog_id: 'target-catalog', version: 3 }], roles: { target: { cardinality: 'many', slots: ['target-x', 'target-y'] } }, widgets: [] }, metrics: { 'target-catalog': [{ id: 'target-metric', name: 'Target metric', value_type: 'numeric_float', units: 'ms', origin: 'template_item' }] } };
  const clipboard = new PageClipboard(); expect(clipboard.copy(source, ['graph-id', 'item-id'], false)).toEqual([]);
  const content = clipboard.read()!;
  const resolution: PasteResolution = { catalogs: { 'source-catalog': { ...target.page.catalogs[0] } }, metrics: { 'source-catalog/source-metric': { metric_id: 'target-metric', evidence: structuredClone(target.metrics['target-catalog'][0]) } }, roles: { source: { role: 'target', evidence: structuredClone(target.page.roles.target), slots: { 'source-a': 'target-x', 'source-b': 'target-y' } } } };
  return { source, target, content, resolution };
}

describe('explicit local Page paste resolution', () => {
  it('leaves different identities undecided and preserves exact identities without guessing', () => {
    const { source, target, content } = fixture();
    expect(initialResolution(content, target)).toEqual({ catalogs: {}, metrics: {}, roles: {} });
    expect(inspectResolvedPaste(content, target, 'original', initialResolution(content, target)).issues.map((issue) => issue.code)).toEqual(['catalog', 'metric', 'role']);
    expect(inspectResolvedPaste(content, source, 'first_free', initialResolution(content, source)).issues).toEqual([]);
  });
  it('rewrites all/slot and metrics together, keeps source immutable, and undoes the entire paste once', () => {
    const { source, target, content, resolution } = fixture(); const beforeSource = structuredClone(source); const history = new EditorHistory(target.page);
    let counter = 0; const result = applyResolvedPaste(content, target, 'original', () => `copy-${++counter}`, resolution);
    expect(result.issues).toEqual([]); expect(result.ids).toEqual(['copy-1', 'copy-2']);
    const [graph, item] = result.page!.widgets; if (graph.kind !== 'graph' || item.kind !== 'item_value') throw new Error('fixture');
    expect(graph.metrics[0]).toEqual({ catalog_id: 'target-catalog', metric_id: 'target-metric', hosts: { role: 'target', selection: 'all' } });
    expect(item.hosts).toEqual({ role: 'target', selection: 'slot', slot_id: 'target-y' });
    expect(result.page!.roles).toEqual(target.page.roles); expect(result.page!.catalogs).toEqual(target.page.catalogs);
    expect(source).toEqual(beforeSource); expect(content.widgets[0].widget_id).toBe('graph-id');
    expect(compositionFindings(result.page!, target.metrics).map((finding) => finding.code)).toEqual(['source_binding_required', 'source_binding_required']);
    history.commit(result.page!, 'paste'); expect(history.undo()).toEqual(target.page); expect(history.undo()).toBeNull();
  });
  it('rejects stale pin, metric evidence and role evidence before allocating IDs', () => {
    for (const kind of ['pin', 'metric', 'role'] as const) {
      const { target, content, resolution } = fixture();
      if (kind === 'pin') target.page.catalogs[0].version++;
      if (kind === 'metric') target.metrics['target-catalog'][0].name = 'Changed';
      if (kind === 'role') target.page.roles.target.slots!.reverse();
      const before = structuredClone(target);
      const result = applyResolvedPaste(content, target, 'original', () => { throw new Error('must preflight before IDs'); }, resolution);
      expect(result.page).toBeNull(); expect(result.issues.some((issue) => issue.code === (kind === 'pin' ? 'catalog' : kind))).toBe(true); expect(target).toEqual(before);
    }
  });
  it('rejects missing, duplicate, foreign or reordered slot choices and cardinality changes', () => {
    const cases: Array<Record<string, string>> = [{ 'source-a': 'target-x' }, { 'source-a': 'target-x', 'source-b': 'target-x' }, { 'source-a': 'foreign', 'source-b': 'target-y' }, { 'source-a': 'target-y', 'source-b': 'target-x' }];
    for (const slots of cases) {
      const { target, content, resolution } = fixture(); resolution.roles.source.slots = slots;
      expect(resolveClipboard(content, target, resolution).issues).toContainEqual({ code: 'role', value: 'source' });
    }
    const { target, content, resolution } = fixture(); target.page.roles.target.cardinality = 'one'; resolution.roles.source.evidence = structuredClone(target.page.roles.target);
    expect(resolveClipboard(content, target, resolution).issues).toContainEqual({ code: 'role', value: 'source' });
  });
  it('rejects incompatible units and value types even after the chosen evidence is refreshed', () => {
    for (const change of [{ units: 's' }, { value_type: 'numeric_unsigned' }, { value_type: 'binary' }, { value_type: 'json' }]) {
      const { target, content, resolution } = fixture(); Object.assign(target.metrics['target-catalog'][0], change); resolution.metrics['source-catalog/source-metric'].evidence = structuredClone(target.metrics['target-catalog'][0]);
      expect(resolveClipboard(content, target, resolution).issues).toContainEqual({ code: 'metric', value: 'source-metric' });
    }
  });
  it('rejects injected reference fields and still forbids cross-page cut', () => {
    const { target, content, resolution } = fixture(); const graph = content.widgets[0]; if (graph.kind !== 'graph') throw new Error('fixture');
    Object.assign(graph.metrics[0], { reference: 'external' });
    expect(resolveClipboard(content, target, resolution).issues).toEqual([{ code: 'reference' }]);
    delete (graph.metrics[0] as unknown as Record<string, unknown>).reference; content.cut = true;
    expect(inspectResolvedPaste(content, target, 'original', resolution).issues).toEqual([{ code: 'cross_page_cut' }]);
  });
  it('shows explicit candidates and slot order in the accessible form', () => {
    const { target, content, resolution } = fixture();
    const html = render(PasteResolutionForm, { props: { i18n: createI18n('en'), content, target, resolution, onchange() {} } }).body;
    expect(html).toContain('Target pinned catalog'); expect(html).toContain('Target metric'); expect(html).toContain('Target role'); expect(html).toContain('target-x'); expect(html).toContain('preserving declared order');
  });
});
