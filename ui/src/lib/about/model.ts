// Logic of «About and licenses» (P1-12): third-party inventory filtering and the report text.
// Pure functions only; the screen renders their results as plain text (no raw HTML).

import { shortBuildCommit, type AboutInfo } from '../about';
import licenseData from './licenses.json';

export type ComponentKind = 'icons' | 'rust' | 'npm';
export type ComponentFilter = 'all' | ComponentKind;

export interface ThirdPartyRow {
  readonly kind: ComponentKind;
  readonly name: string;
  readonly version: string;
  readonly license: string;
}

export interface LicenseData {
  readonly schema_version: number;
  readonly product_licenses: { readonly mit: string; readonly apache: string; readonly docs: string };
  readonly brand_provenance: string;
  readonly icons: {
    readonly name: string;
    readonly version: string;
    readonly commit: string;
    readonly license: string;
    readonly license_text: string;
    readonly icon_names: ReadonlyArray<string>;
  };
  readonly components: ReadonlyArray<{
    readonly kind: 'rust' | 'npm';
    readonly name: string;
    readonly version: string;
    readonly license: string;
    readonly source: string;
  }>;
}

/** Generated at build time by `tools/build_licenses.py`; the application never fetches it. */
export const LICENSES = licenseData as LicenseData;

/** All rows in display order: the icon set first, then Rust crates and npm packages (already sorted). */
export function thirdPartyRows(data: LicenseData = LICENSES): ReadonlyArray<ThirdPartyRow> {
  return [
    { kind: 'icons', name: data.icons.name, version: data.icons.version, license: data.icons.license },
    ...data.components.map(({ kind, name, version, license }) => ({ kind, name, version, license }))
  ];
}

export function countByFilter(rows: ReadonlyArray<ThirdPartyRow>): Readonly<Record<ComponentFilter, number>> {
  return {
    all: rows.length,
    icons: rows.filter((row) => row.kind === 'icons').length,
    rust: rows.filter((row) => row.kind === 'rust').length,
    npm: rows.filter((row) => row.kind === 'npm').length
  };
}

/** Filter by kind and a case-insensitive substring of the name or the license expression. */
export function filterRows(
  rows: ReadonlyArray<ThirdPartyRow>,
  filter: ComponentFilter,
  query: string
): ReadonlyArray<ThirdPartyRow> {
  const needle = query.trim().toLowerCase();
  return rows.filter(
    (row) =>
      (filter === 'all' || row.kind === filter) &&
      (needle === '' || row.name.toLowerCase().includes(needle) || row.license.toLowerCase().includes(needle))
  );
}

export interface ClientInfo {
  readonly userAgent: string;
  readonly platform: string;
}

/** Operating system and browser as the browser reports them; nothing about the server. */
export function describeClient(nav: {
  readonly userAgent?: string;
  readonly platform?: string;
  readonly userAgentData?: { readonly platform?: string };
}): ClientInfo {
  return {
    userAgent: nav.userAgent ?? '',
    platform: nav.userAgentData?.platform || nav.platform || ''
  };
}

const URL_PATTERN = /[a-z][a-z0-9+.-]*:\/\/\S*/gi;

/**
 * Plain-text diagnostics for a bug report (English: diagnostic data, like application logs).
 * Contains no server addresses, profile names, accounts, secrets or paths; any URL-like text
 * in a client string is dropped.
 */
export function reportText(info: AboutInfo, client: ClientInfo, locale: string): string {
  const commit = shortBuildCommit(info) ?? 'unknown';
  const clean = (value: string): string => value.replace(URL_PATTERN, '').replace(/\s+/g, ' ').trim() || 'unknown';
  return [
    `ZDashstitch ${info.version}`,
    `Build: ${commit}`,
    `Platform: ${clean(client.platform)}`,
    `Browser: ${clean(client.userAgent)}`,
    `Interface language: ${locale}`
  ].join('\n');
}

export type CopyResult = 'copied' | 'unavailable';

/** Writes to the clipboard; any failure (no API, permission) falls back to a manual-copy hint. */
export async function copyText(
  text: string,
  clipboard: { writeText?: (value: string) => Promise<void> } | undefined
): Promise<CopyResult> {
  if (!clipboard?.writeText) return 'unavailable';
  try {
    await clipboard.writeText(text);
    return 'copied';
  } catch {
    return 'unavailable';
  }
}
