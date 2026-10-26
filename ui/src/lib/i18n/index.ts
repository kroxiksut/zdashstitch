import {
  catalogs,
  placeholderTypes,
  type Locale,
  type MessageId,
  type MessageParamsById
} from './generated';

type EmptyParams = Record<never, never>;
type ParamsFor<I extends MessageId> = MessageParamsById[I];
type TranslationArgs<I extends MessageId> = keyof ParamsFor<I> extends never
  ? [params?: EmptyParams]
  : [params: ParamsFor<I>];
type PlaceholderType =
  | 'technical'
  | 'technical_optional'
  | 'text'
  | 'text_optional'
  | 'number'
  | 'date'
  | 'datetime';

export interface I18n {
  readonly locale: Locale;
  t<I extends MessageId>(id: I, ...args: TranslationArgs<I>): string;
  number(value: number, options?: Intl.NumberFormatOptions): string;
  date(value: Date | number, options?: Intl.DateTimeFormatOptions): string;
  plural(value: number): Intl.LDMLPluralRule;
}

function matchingBrace(source: string, opening: number): number {
  let depth = 0;
  for (let index = opening; index < source.length; index += 1) {
    if (source[index] === '{') depth += 1;
    if (source[index] === '}') {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  throw new Error('unbalanced localized message braces');
}

function branchesOf(source: string, keyPattern: RegExp): Readonly<Record<string, string>> {
  const branches: Record<string, string> = {};
  let position = 0;
  while (position < source.length) {
    while (/\s/.test(source[position] ?? '')) position += 1;
    if (position >= source.length) break;
    const match = keyPattern.exec(source.slice(position));
    if (!match) throw new Error('invalid localized branch key');
    const key = match[0];
    position += key.length;
    while (/\s/.test(source[position] ?? '')) position += 1;
    if (source[position] !== '{') throw new Error('localized branch is missing');
    const closing = matchingBrace(source, position);
    branches[key] = source.slice(position + 1, closing);
    position = closing + 1;
  }
  if (!('other' in branches)) throw new Error('localized plural or select requires other');
  return branches;
}

function pluralBranches(source: string): Readonly<Record<string, string>> {
  return branchesOf(source, /^(?:zero|one|two|few|many|other|=[0-9]+)/);
}

function selectBranches(source: string): Readonly<Record<string, string>> {
  return branchesOf(source, /^[a-z][a-z0-9_]*/);
}

const MISSING_VALUE = '—';

function formatNumber(value: number, locale: Locale, options?: Intl.NumberFormatOptions): string {
  return Number.isFinite(value) ? new Intl.NumberFormat(locale, options).format(value) : MISSING_VALUE;
}

function formatDate(value: Date | number, locale: Locale, options?: Intl.DateTimeFormatOptions): string {
  const time = value instanceof Date ? value.getTime() : value;
  return Number.isFinite(time) ? new Intl.DateTimeFormat(locale, options).format(time) : MISSING_VALUE;
}

/** Format of `datetime` placeholders: short date and time in the selected locale. */
export const DATE_TIME_FORMAT: Intl.DateTimeFormatOptions = { dateStyle: 'short', timeStyle: 'short' };

function formatValue(value: unknown, type: PlaceholderType | undefined, locale: Locale): string {
  if (value === undefined) return MISSING_VALUE;
  if (type === 'number' && typeof value === 'number') return formatNumber(value, locale);
  if (type === 'date' && (value instanceof Date || typeof value === 'number')) return formatDate(value, locale);
  if (type === 'datetime' && (value instanceof Date || typeof value === 'number')) {
    return formatDate(value, locale, DATE_TIME_FORMAT);
  }
  return String(value);
}

export function formatTemplate(
  template: string,
  params: Readonly<Record<string, unknown>>,
  types: Readonly<Record<string, PlaceholderType>>,
  locale: Locale,
  pluralValue?: number
): string {
  let output = '';
  let position = 0;
  while (position < template.length) {
    if (template[position] === '#' && pluralValue !== undefined) {
      output += formatNumber(pluralValue, locale);
      position += 1;
      continue;
    }
    if (template[position] !== '{') {
      output += template[position];
      position += 1;
      continue;
    }
    const closing = matchingBrace(template, position);
    const expression = template.slice(position + 1, closing).trim();
    const plural = /^([a-z][a-z0-9_]*)\s*,\s*plural\s*,([\s\S]*)$/.exec(expression);
    if (plural) {
      const name = plural[1];
      const value = params[name];
      if (typeof value !== 'number' || !Number.isFinite(value)) {
        output += MISSING_VALUE;
      } else {
        const branches = pluralBranches(plural[2]);
        const exact = branches[`=${value}`];
        const category = new Intl.PluralRules(locale).select(value);
        output += formatTemplate(
          exact ?? branches[category] ?? branches.other,
          params,
          types,
          locale,
          value
        );
      }
    } else if (/^[a-z][a-z0-9_]*\s*,\s*select\s*,/.test(expression)) {
      // An absent optional select argument renders nothing: it spells an optional suffix.
      const select = /^([a-z][a-z0-9_]*)\s*,\s*select\s*,([\s\S]*)$/.exec(expression);
      const value = select ? params[select[1]] : undefined;
      if (select && value !== undefined && value !== '') {
        const branches = selectBranches(select[2]);
        output += formatTemplate(branches[String(value)] ?? branches.other, params, types, locale, pluralValue);
      }
    } else if (/^[a-z][a-z0-9_]*$/.test(expression)) {
      output += formatValue(params[expression], types[expression], locale);
    } else {
      throw new Error('unsupported localized message expression');
    }
    position = closing + 1;
  }
  return output;
}

export function createI18n(locale: Locale): I18n {
  const selected = catalogs[locale] ?? catalogs.en;
  return {
    locale,
    t<I extends MessageId>(id: I, ...args: TranslationArgs<I>): string {
      const template = selected[id] ?? catalogs.en[id];
      return formatTemplate(
        template,
        (args[0] ?? {}) as Readonly<Record<string, unknown>>,
        placeholderTypes[id] as Readonly<Record<string, PlaceholderType>>,
        locale
      );
    },
    number(value, options) {
      return formatNumber(value, locale, options);
    },
    date(value, options) {
      return formatDate(value, locale, options);
    },
    plural(value) {
      return new Intl.PluralRules(locale).select(value);
    }
  };
}

export function detectLocale(language: string | undefined): Locale {
  return language?.toLowerCase().startsWith('ru') ? 'ru' : 'en';
}

export function applyDocumentLocale(root: Pick<HTMLElement, 'lang'>, locale: Locale): void {
  root.lang = locale;
}

export { catalogs } from './generated';
export type { Locale, MessageId, MessageParamsById } from './generated';
