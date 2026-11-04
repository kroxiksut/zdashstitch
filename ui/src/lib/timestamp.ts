// Timestamps from application APIs (decision O-035): the backend sends RFC 3339 UTC strings,
// the UI parses them here and formats the result with `DATE_TIME_FORMAT` in the user's locale
// and local time zone. The server never sends a formatted date.

const RFC3339 = /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(?:([Zz])|([+-])(\d{2}):(\d{2}))$/;

function daysInMonth(year: number, month: number): number {
  const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  return [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1] ?? 0;
}

/**
 * Epoch milliseconds of an RFC 3339 timestamp with an explicit offset, or `NaN` when the value is
 * absent or not a valid timestamp. `NaN` renders as the missing-value marker in `datetime`
 * placeholders and `I18n.date`, never as «Invalid Date».
 */
export function parseTimestamp(value: string | null | undefined): number {
  if (typeof value !== 'string') return Number.NaN;
  const match = RFC3339.exec(value);
  if (!match) return Number.NaN;
  const [year, month, day, hour, minute, second] = match.slice(1, 7).map(Number);
  const fraction = match[7] ?? '';
  const offsetSign = match[9] === '-' ? -1 : 1;
  const offsetHours = match[9] ? Number(match[10]) : 0;
  const offsetMinutes = match[9] ? Number(match[11]) : 0;
  if (
    month < 1 ||
    month > 12 ||
    day < 1 ||
    day > daysInMonth(year, month) ||
    hour > 23 ||
    minute > 59 ||
    second > 59 ||
    offsetHours > 23 ||
    offsetMinutes > 59
  ) {
    return Number.NaN;
  }
  const milliseconds = fraction === '' ? 0 : Math.floor(Number(`0.${fraction}`) * 1000);
  const date = new Date(Date.UTC(2000, month - 1, day, hour, minute, second, milliseconds));
  // Date.UTC maps years 0..99 to 1900..1999; set the full year explicitly.
  date.setUTCFullYear(year, month - 1, day);
  return date.getTime() - offsetSign * (offsetHours * 60 + offsetMinutes) * 60_000;
}
