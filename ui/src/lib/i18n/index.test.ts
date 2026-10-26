import { describe, expect, it } from 'vitest';

import {
  applyDocumentLocale,
  catalogs,
  createI18n,
  detectLocale,
  formatTemplate
} from './index';

describe('localization runtime', () => {
  it('keeps English and Russian key sets identical', () => {
    expect(Object.keys(catalogs.ru).sort()).toEqual(Object.keys(catalogs.en).sort());
  });

  it('interpolates typed parameters without interpreting markup', () => {
    const i18n = createI18n('en');
    expect(
      i18n.t('compatibility.unknown_widget_type', { widget_type: '<script>alert(1)</script>' })
    ).toContain('<script>alert(1)</script>');
  });

  it('renders invalid dates and non-finite numbers as a dash instead of throwing', () => {
    const i18n = createI18n('ru');
    expect(i18n.date(new Date(Number.NaN))).toBe('—');
    expect(i18n.date(Number.NaN)).toBe('—');
    expect(i18n.number(Number.NaN)).toBe('—');
    expect(i18n.number(Number.POSITIVE_INFINITY)).toBe('—');
    expect(
      formatTemplate('{when} / {count}', { when: new Date('invalid'), count: Number.NaN }, { when: 'date', count: 'number' }, 'en')
    ).toBe('— / —');
    expect(
      formatTemplate('{count, plural, one {# item} other {# items}}', { count: Number.NaN }, { count: 'number' }, 'en')
    ).toBe('—');
  });

  it('formats values and plural categories for the selected locale', () => {
    const english = createI18n('en');
    const russian = createI18n('ru');
    expect(english.number(1234.5)).not.toEqual(russian.number(1234.5));
    expect(english.plural(2)).toBe('other');
    expect(russian.plural(2)).toBe('few');
  });

  it('formats typed placeholders and ICU plural branches for the selected locale', () => {
    const english = createI18n('en');
    const russian = createI18n('ru');
    expect([0, 1, 2, 5, 21, 101].map((count) => russian.t('common.count.items', { count })))
      .toEqual([
        '0 элементов',
        '1 элемент',
        '2 элемента',
        '5 элементов',
        '21 элемент',
        '101 элемент'
      ]);
    expect(english.t('common.count.items', { count: 0 })).toBe('0 items');
    expect(english.t('common.count.items', { count: 1 })).toBe('1 item');

    const timestamp = Date.UTC(2026, 9, 25, 14, 30);
    const en = formatTemplate(
      '{count} / {when}',
      { count: 1234.5, when: timestamp },
      { count: 'number', when: 'date' },
      'en'
    );
    const ru = formatTemplate(
      '{count} / {when}',
      { count: 1234.5, when: timestamp },
      { count: 'number', when: 'date' },
      'ru'
    );
    expect(en).not.toEqual(ru);
    expect(en).toContain(english.number(1234.5));
    expect(ru).toContain(russian.number(1234.5));
  });

  it('renders an em dash for an absent optional bound', () => {
    expect(
      createI18n('en').t('compatibility.calculated_range', {
        minimum: '7.4.0'
      })
    ).toContain('—');
  });

  it('selects Russian explicitly and otherwise falls back to English', () => {
    expect(detectLocale('ru-RU')).toBe('ru');
    expect(detectLocale('en-US')).toBe('en');
    expect(detectLocale('de-DE')).toBe('en');
    expect(detectLocale(undefined)).toBe('en');
  });

  it('applies the selected language to the document root', () => {
    const root = { lang: 'en' };
    applyDocumentLocale(root, 'ru');
    expect(root.lang).toBe('ru');
  });
});
