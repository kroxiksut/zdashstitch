import { describe, expect, it } from 'vitest';
import { createI18n, DATE_TIME_FORMAT } from './i18n';
import { parseTimestamp } from './timestamp';

describe('RFC 3339 timestamps from application APIs (O-035)', () => {
  it('parses UTC and offset timestamps to epoch milliseconds', () => {
    expect(parseTimestamp('2026-10-26T07:33:20Z')).toBe(Date.UTC(2026, 9, 26, 7, 33, 20));
    expect(parseTimestamp('2026-10-26t07:33:20z')).toBe(Date.UTC(2026, 9, 26, 7, 33, 20));
    expect(parseTimestamp('2026-10-26T10:33:20+03:00')).toBe(Date.UTC(2026, 9, 26, 7, 33, 20));
    expect(parseTimestamp('2026-10-26T02:03:20-05:30')).toBe(Date.UTC(2026, 9, 26, 7, 33, 20));
    expect(parseTimestamp('2026-10-26T07:33:20.123456Z')).toBe(Date.UTC(2026, 9, 26, 7, 33, 20, 123));
    expect(parseTimestamp('2028-02-29T00:00:00Z')).toBe(Date.UTC(2028, 1, 29));
    expect(parseTimestamp('0050-01-01T00:00:00Z')).toBe(new Date('0050-01-01T00:00:00Z').getTime());
  });

  it.each([
    null,
    undefined,
    '',
    'garbage',
    '1793000000',
    '2026-10-26',
    '2026-10-26T07:33:20',
    '2026-10-26 07:33:20Z',
    '2026-02-29T00:00:00Z',
    '2026-04-31T00:00:00Z',
    '2026-13-01T00:00:00Z',
    '2026-10-26T24:00:00Z',
    '2026-10-26T07:60:00Z',
    '2026-10-26T07:33:60Z',
    '2026-10-26T07:33:20+24:00',
    ' 2026-10-26T07:33:20Z'
  ])('rejects %j as NaN', (value) => {
    expect(parseTimestamp(value)).toBeNaN();
  });

  it('formats a parsed value in the locale and an invalid one as the missing-value marker', () => {
    const time = parseTimestamp('2026-10-26T07:33:20Z');
    for (const locale of ['en', 'ru'] as const) {
      const i18n = createI18n(locale);
      const expected = new Intl.DateTimeFormat(locale, DATE_TIME_FORMAT).format(time);
      expect(i18n.t('connection.card.checked_at', { time })).toContain(expected);
      expect(i18n.date(time, DATE_TIME_FORMAT)).toBe(expected);
      const invalid = parseTimestamp('not a timestamp');
      expect(i18n.date(invalid, DATE_TIME_FORMAT)).toBe('—');
      expect(i18n.t('connection.card.checked_at', { time: invalid })).toContain('—');
      expect(i18n.t('connection.card.checked_at', { time: invalid })).not.toContain('Invalid');
    }
  });
});
