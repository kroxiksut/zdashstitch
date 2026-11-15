import type { PortablePageWidget } from './client';

/** Mirrors the server allowlist; the server remains authoritative. */
export function redactableHostMaskIndices(widget: PortablePageWidget): number[] {
  if (widget.type !== 'zabbix' || widget.widget_type !== 'svggraph' || widget.registry_state !== 'known') return [];
  return widget.fields.flatMap((raw, index) => {
    if (!raw || typeof raw !== 'object') return [];
    const field = raw as { name?: unknown; field_type?: unknown; registry_state?: unknown; value?: { kind?: unknown } };
    return typeof field.name === 'string' && /^(?:(?:ds|or)\.[0-9]+\.hosts\.[0-9]+|problemhosts\.[0-9]+)$/.test(field.name)
      && field.field_type === 1 && field.registry_state === 'known' && ['string', 'dynamic_pattern'].includes(String(field.value?.kind)) ? [index] : [];
  });
}

export function redactableTextIndices(widget: PortablePageWidget): number[] {
  if (widget.type !== 'zabbix' || widget.registry_state !== 'known') return [];
  return widget.fields.flatMap((raw, index) => {
    if (!raw || typeof raw !== 'object') return [];
    const field = raw as { name?: unknown; field_type?: unknown; registry_state?: unknown; value?: { kind?: unknown } };
    if (typeof field.name !== 'string' || field.field_type !== 1 || field.registry_state !== 'known' || field.value?.kind !== 'string') return [];
    const name = field.name; const type = widget.widget_type;
    const allowed = (type === 'item' && name === 'description') || (['problems', 'problemsbysv'].includes(type) && name === 'problem') || (type === 'problems' && name === 'tag_priority') || (type === 'svggraph' && (name === 'problem_name' || /^ds\.[0-9]+\.data_set_label$/.test(name))) || (type === 'tophosts' && /^columns\.[0-9]+\.(name|text)$/.test(name)) || (['problems', 'problemsbysv', 'svggraph', 'tophosts'].includes(type) && /^tags\.[0-9]+\.(tag|value)$/.test(name));
    return allowed ? [index] : [];
  });
}

export function redactableItemPatternIndices(widget: PortablePageWidget): number[] {
  if (widget.type !== 'zabbix' || widget.registry_state !== 'known') return [];
  return widget.fields.flatMap((raw, index) => {
    if (!raw || typeof raw !== 'object') return [];
    const field = raw as { name?: unknown; field_type?: unknown; registry_state?: unknown; value?: { kind?: unknown } };
    if (typeof field.name !== 'string' || field.field_type !== 1 || field.registry_state !== 'known' || !['string', 'dynamic_pattern'].includes(String(field.value?.kind))) return [];
    return ((widget.widget_type === 'svggraph' && /^(?:ds|or)\.[0-9]+\.items\.[0-9]+$/.test(field.name)) || (widget.widget_type === 'tophosts' && /^columns\.[0-9]+\.item$/.test(field.name))) ? [index] : [];
  });
}
