/** Formatting helpers shared by the shelf, the tree, and the status line. */

/**
 * Grimoire's interface is English-only, so its dates are too.
 *
 * The OS locale was tried first and produced "2分钟前" beside an English label
 * in the Margin, which reads as a bug rather than as thoughtfulness. When there
 * is a real language setting, this should follow it.
 *
 * Numbers keep the OS locale: digit grouping is a regional convention rather
 * than a language, and "41,208" versus "41 208" never looks out of place.
 */
const relative = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
const compact = new Intl.NumberFormat(undefined);

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;

/**
 * "just now", "2 hours ago", "last week", or a date once it stops being
 * usefully relative.
 */
export function relativeTime(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return 'never';
  const at = Date.parse(iso);
  if (Number.isNaN(at)) return 'unknown';

  const elapsed = now - at;
  // Clock skew, or a file copied from a machine set ahead. Reporting "in 3
  // hours" for something already written would be more confusing than this.
  if (elapsed < 0) return 'just now';
  if (elapsed < MINUTE) return 'just now';
  if (elapsed < HOUR) return relative.format(-Math.floor(elapsed / MINUTE), 'minute');
  if (elapsed < DAY) return relative.format(-Math.floor(elapsed / HOUR), 'hour');
  if (elapsed < WEEK) return relative.format(-Math.floor(elapsed / DAY), 'day');
  if (elapsed < 4 * WEEK) return relative.format(-Math.floor(elapsed / WEEK), 'week');

  return new Date(at).toLocaleDateString('en', {
    year: 'numeric',
    month: 'short',
    day: 'numeric'
  });
}

/** A word or character count, grouped for the reader's locale. */
export function count(value: number): string {
  return compact.format(value);
}

/** "12 chapters", "1 chapter" — a count with its noun agreeing. */
export function plural(value: number, singular: string, pluralForm = `${singular}s`): string {
  return `${compact.format(value)} ${value === 1 ? singular : pluralForm}`;
}
