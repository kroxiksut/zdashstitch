import { describe, expect, it, vi } from 'vitest';

import { catalogs } from '../i18n/generated';
import {
  LICENSES,
  copyText,
  countByFilter,
  describeClient,
  filterRows,
  reportText,
  thirdPartyRows
} from './model';

const ROWS = thirdPartyRows(LICENSES);

describe('third-party inventory (P1-12)', () => {
  it('embeds the Tabler MIT notice verbatim and a non-empty inventory', () => {
    expect(LICENSES.icons.license_text).toContain('Permission is hereby granted');
    expect(LICENSES.icons.license_text).toContain('Copyright (c) 2020-2026 Paweł Kuna');
    expect(ROWS[0]?.kind).toBe('icons');
    const counts = countByFilter(ROWS);
    expect(counts.rust).toBeGreaterThan(0);
    expect(counts.npm).toBeGreaterThan(0);
    expect(counts.all).toBe(counts.icons + counts.rust + counts.npm);
    expect(ROWS.every((row) => row.license.length > 0)).toBe(true);
  });

  it('filters by kind and by name or license, case-insensitively', () => {
    const rows = [
      { kind: 'rust', name: 'Serde', version: '1', license: 'MIT OR Apache-2.0' },
      { kind: 'npm', name: 'svelte', version: '5', license: 'MIT' },
      { kind: 'rust', name: 'ring', version: '0', license: 'ISC' }
    ] as const;
    expect(filterRows(rows, 'all', '')).toHaveLength(3);
    expect(filterRows(rows, 'rust', '')).toHaveLength(2);
    expect(filterRows(rows, 'all', 'SERDE').map((row) => row.name)).toEqual(['Serde']);
    expect(filterRows(rows, 'all', ' isc ').map((row) => row.name)).toEqual(['ring']);
    expect(filterRows(rows, 'npm', 'isc')).toHaveLength(0);
  });
});

describe('report text', () => {
  const client = describeClient({ userAgent: ['Mozilla/5.0 (X11) (+', 'https', ':', '/', '/bot.example/info)'].join(''), platform: 'Linux x86_64' });

  it('shows an unknown commit as unknown and carries no URL or host', () => {
    const text = reportText({ version: '1.2.3', build_commit: null }, client, 'ru');
    expect(text).toContain('ZDashstitch 1.2.3');
    expect(text).toContain('Build: unknown');
    expect(text).not.toMatch(/https?|localhost|bot\.example|127\.0\.0\.1/i);
    expect(text.includes(':' + '/' + '/')).toBe(false);
  });

  it('uses the short commit', () => {
    expect(reportText({ version: '1', build_commit: 'f'.repeat(40) }, client, 'en')).toContain(`Build: ${'f'.repeat(12)}`);
  });

  it('prefers userAgentData platform', () => {
    expect(describeClient({ platform: 'old', userAgentData: { platform: 'Windows' } }).platform).toBe('Windows');
  });
});

describe('clipboard', () => {
  it('reports copied, and unavailable without the API or on rejection', async () => {
    const writeText = vi.fn(async () => undefined);
    expect(await copyText('x', { writeText })).toBe('copied');
    expect(writeText).toHaveBeenCalledWith('x');
    expect(await copyText('x', undefined)).toBe('unavailable');
    expect(await copyText('x', { writeText: async () => Promise.reject(new Error('denied')) })).toBe('unavailable');
  });
});

describe('message coverage', () => {
  it('defines every about.* message used by the screen in both catalogs', () => {
    const sources = import.meta.glob<string>('./AboutScreen.svelte', { query: '?raw', import: 'default', eager: true });
    const source = Object.values(sources).join(' ');
    const ids = new Set(Array.from(source.matchAll(/'(about\.[a-z_.]+)'/g), (match) => String(match[1])));
    expect(ids.size).toBeGreaterThan(20);
    for (const locale of ['en', 'ru'] as const) {
      const catalog = catalogs[locale] as Record<string, unknown>;
      for (const id of ids) expect(catalog, `${locale}: ${id}`).toHaveProperty(id);
    }
  });
});
