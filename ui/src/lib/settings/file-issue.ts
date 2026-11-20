import type { LocalizedText } from '../components';

const labels = {
  invalid_yaml: 'settings.file.issue.invalid_yaml',
  invalid_format: 'settings.file.issue.invalid_format',
  invalid_storage_root: 'settings.file.issue.invalid_storage_root',
  file_too_large: 'settings.file.issue.file_too_large',
  interrupted_save: 'settings.file.issue.interrupted_save',
  unreadable: 'settings.file.unreadable'
} as const;

export interface SettingsFileIssue {
  code: keyof typeof labels;
  line: number | null;
  column: number | null;
}

export function settingsFileIssue(value: unknown): SettingsFileIssue | null {
  if (!value || typeof value !== 'object') return null;
  const { code, line, column } = value as Record<string, unknown>;
  if (typeof code !== 'string' || !Object.hasOwn(labels, code)) return null;
  const position = (part: unknown): number | null =>
    typeof part === 'number' && Number.isSafeInteger(part) && part > 0 ? part : null;
  return { code: code as keyof typeof labels, line: position(line), column: position(column) };
}

export function settingsIssueText(issue: SettingsFileIssue): LocalizedText {
  return issue.code === 'invalid_yaml' && issue.line !== null && issue.column !== null
    ? { id: 'settings.file.issue.yaml_location', params: { line: issue.line, column: issue.column } }
    : { id: labels[issue.code] };
}
