import { ApiFailure } from '../api';
import type { I18n } from '../i18n';
import { reusableId, type ReusableEntry } from './client';

export function displayReusableName(i18n: I18n, entry: ReusableEntry): string {
  if (entry.origin === 'builtin') {
    switch (reusableId(entry.asset)) {
      case '7a2f3c10-5b7e-4c8a-9d21-0f6e4b1a2c3d': return i18n.t('reusable.builtin_bars');
      case '45d10c01-1a20-47f7-98f2-4bdad3475de3': return i18n.t('reusable.builtin_line');
    }
  }
  return entry.asset.document.name;
}

export function reusableError(i18n: I18n, error: unknown, importer = false): string {
  const code = error instanceof ApiFailure ? error.response?.code ?? 'unknown' : 'unknown';
  const messages = {
    format_newer: 'reusable.format_newer',
    reusable_conflict: 'reusable.conflict',
    offline_page_conflict: 'reusable.conflict',
    workcopy_revision_conflict: 'reusable.conflict',
    workcopy_changed_concurrently: 'reusable.conflict',
    reusable_workcopy_publish_unsupported: 'reusable.copy_style_publish',
    reusable_adapter_unsupported: 'reusable.unsupported_adapter',
    reusable_blueprint_shape_unsupported: 'reusable.unsupported_blueprint',
    reusable_blueprint_placement: 'reusable.blueprint_placement',
    reusable_blueprint_inventory_incomplete: 'reusable.blueprint_inventory_incomplete',
    source_binding_required: 'reusable.blueprint_source_missing',
    catalog_missing: 'reusable.blueprint_catalog_missing',
    reusable_assignments_invalid: 'reusable.assignment_invalid',
    reusable_style_source_unsupported: 'reusable.style_source_unsupported',
    redaction_credentials_present: 'reusable.export_blocked',
  } as const;
  const message = messages[code as keyof typeof messages];
  return message
    ? i18n.t(message)
    : i18n.t(importer ? 'reusable.import_invalid' : 'reusable.operation_failed', { code });
}
