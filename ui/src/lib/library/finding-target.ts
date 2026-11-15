/** Map a sanitized scan location to an existing, server-validated copy action. */
export function findingTarget(location: string, kind: 'page' | 'dashboard', options: readonly string[]): string | null {
  const candidates: object[] = [];
  const match = kind === 'page'
    ? /^\/project\/root\/widgets\/(\d+)(?:\/fields\/(\d+))?(?:\/|$)/.exec(location)
    : /^\/project\/root\/pages\/(\d+)\/widgets\/(\d+)(?:\/fields\/(\d+))?(?:\/|$)/.exec(location);
  if (match) {
    const page_index = kind === 'page' ? 0 : Number(match[1]);
    const widget_index = Number(match[kind === 'page' ? 1 : 2]);
    const field = match[kind === 'page' ? 2 : 3];
    if (field !== undefined) for (const target of ['widget_host_mask', 'widget_item_pattern', 'widget_unresolved_name', 'widget_text']) candidates.push({ kind: target, page_index, widget_index, field_index: Number(field) });
    if (location.endsWith('/name') || location.endsWith('/title')) candidates.push({ kind: 'widget_name', page_index, widget_index });
    if (location.endsWith('/id')) candidates.push({ kind: 'widget_id', page_index, widget_index });
    candidates.push({ kind: 'remove_widget', page_index, widget_index });
  } else {
    const role = /^\/project\/roles\/(\d+)\/(id|description|slots\/\d+)(?:\/.*)?$/.exec(location);
    if (role) {
      const role_index = Number(role[1]);
      if (role[2] === 'id') candidates.push({ kind: 'role_alias', role_index });
      else if (role[2] === 'description') candidates.push({ kind: 'role_description', role_index });
      else candidates.push({ kind: 'slot_alias', role_index, slot_index: Number(role[2].slice('slots/'.length)) });
    }
    const catalog = /^\/catalogs\/(\d+)(?:\/metrics\/(\d+)\/(name|id|units|uuid|selector)(?:\/.*)?|\/source\/(template_name|template_uuid|export_format_version|source_server_version)|\/(catalog_id))$/.exec(location);
    if (catalog) {
      const catalog_index = Number(catalog[1]);
      const metric_index = catalog[2] === undefined ? null : Number(catalog[2]);
      const field = catalog[3] ?? catalog[4] ?? catalog[5];
      if (metric_index !== null && field === 'name') candidates.push({ kind: 'catalog_metric_name', catalog_index, metric_index });
      if (metric_index !== null && field === 'id') candidates.push({ kind: 'catalog_metric_id', catalog_index, metric_index });
      if (metric_index !== null && field === 'units') candidates.push({ kind: 'catalog_metric_units', catalog_index, metric_index });
      if (field === 'selector') candidates.push({ kind: 'catalog_selector_aliases', catalog_index });
      if (field === 'uuid' || field === 'template_uuid') candidates.push({ kind: 'catalog_clear_uuids', catalog_index });
      if (field === 'template_name') candidates.push({ kind: 'catalog_template_name', catalog_index });
      if (field === 'export_format_version' || field === 'source_server_version') candidates.push({ kind: 'catalog_clear_source', catalog_index });
      if (field === 'catalog_id') candidates.push({ kind: 'catalog_id', catalog_index });
    }
    const page = /^\/project\/root\/pages\/(\d+)\/(name|id)$/.exec(location);
    if (kind === 'dashboard' && page) candidates.push({ kind: page[2] === 'id' ? 'page_id' : 'page_name', page_index: Number(page[1]) });
    if (location === '/project/root/name') candidates.push(kind === 'page' ? { kind: 'page_name', page_index: 0 } : { kind: 'dashboard_name' });
    if (location === '/project/root/id') candidates.push(kind === 'page' ? { kind: 'page_id', page_index: 0 } : { kind: 'dashboard_id' });
    if (location === '/project/project_id') candidates.push({ kind: 'project_id' });
  }
  return candidates.map((candidate) => JSON.stringify(candidate)).find((candidate) => options.includes(candidate)) ?? null;
}

/** Never replace a target or value the user is currently entering. */
export function canSelectFinding(target: string | null, selected: string, replacement: string, grouped: boolean, existingTargets: readonly string[]): target is string {
  return target !== null && !selected && !replacement.trim() && !grouped && !existingTargets.includes(target);
}
