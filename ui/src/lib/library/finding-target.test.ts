import { expect, it } from 'vitest';
import { canSelectFinding, findingTarget } from './finding-target';

it('maps recognized fields and falls back to excluding opaque widgets', () => {
  const pattern = JSON.stringify({ kind: 'widget_item_pattern', page_index: 0, widget_index: 2, field_index: 3 });
  const removal = JSON.stringify({ kind: 'remove_widget', page_index: 0, widget_index: 2 });
  expect(findingTarget('/project/root/widgets/2/fields/3/value/value', 'page', [pattern, removal])).toBe(pattern);
  expect(findingTarget('/project/root/widgets/2/fields/3/value/value', 'page', [removal])).toBe(removal);
  expect(findingTarget('/project/root/widgets/2/fields/3/value/value', 'page', [])).toBeNull();
  expect(findingTarget('/project/root/widgets/not-an-index', 'page', [removal])).toBeNull();
});

it('keeps unfinished or already-added changes when navigating a finding', () => {
  expect(canSelectFinding('target', '', '', false, [])).toBe(true);
  expect(canSelectFinding('target', 'other', '', false, [])).toBe(false);
  expect(canSelectFinding('target', '', 'typed value', false, [])).toBe(false);
  expect(canSelectFinding('target', '', '', true, [])).toBe(false);
  expect(canSelectFinding('target', '', '', false, ['target'])).toBe(false);
  expect(canSelectFinding(null, '', '', false, [])).toBe(false);
});

it('maps dashboard pages without using untrusted names as indices', () => {
  const page = JSON.stringify({ kind: 'page_name', page_index: 4 });
  const removal = JSON.stringify({ kind: 'remove_widget', page_index: 4, widget_index: 2 });
  expect(findingTarget('/project/root/pages/4/name', 'dashboard', [page])).toBe(page);
  expect(findingTarget('/project/root/pages/4/widgets/2', 'dashboard', [removal])).toBe(removal);
  expect(findingTarget('/catalogs/*/metrics/0/name', 'dashboard', [page])).toBeNull();
});

it('maps sanitized catalog positions to the corresponding action', () => {
  const name = JSON.stringify({ kind: 'catalog_metric_name', catalog_index: 1, metric_index: 3 });
  const alias = JSON.stringify({ kind: 'catalog_selector_aliases', catalog_index: 1 });
  const source = JSON.stringify({ kind: 'catalog_clear_source', catalog_index: 1 });
  expect(findingTarget('/catalogs/1/metrics/3/name', 'page', [name, alias, source])).toBe(name);
  expect(findingTarget('/catalogs/1/metrics/3/selector/value', 'page', [name, alias, source])).toBe(alias);
  expect(findingTarget('/catalogs/1/source/source_server_version', 'page', [name, alias, source])).toBe(source);
  expect(findingTarget('/catalogs/private-name/metrics/3/name', 'page', [name])).toBeNull();
});

it('maps role and slot positions without exposing their names', () => {
  const role = JSON.stringify({ kind: 'role_alias', role_index: 1 });
  const slot = JSON.stringify({ kind: 'slot_alias', role_index: 1, slot_index: 2 });
  expect(findingTarget('/project/roles/1/id', 'page', [role, slot])).toBe(role);
  expect(findingTarget('/project/roles/1/slots/2', 'page', [role, slot])).toBe(slot);
});
