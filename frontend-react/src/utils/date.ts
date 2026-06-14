function dtfPart(parts: Intl.DateTimeFormatPart[], type: Intl.DateTimeFormatPartTypes): string {
  return parts.find((p) => p.type === type)?.value ?? '';
}

/**
 * `Intl.DateTimeFormat` is an expensive constructor (full locale + timezone
 * resolution). These formatters are built with a tiny finite set of
 * (timezone, options) combinations but were previously reconstructed on every
 * call — i.e. per table cell, per render, ~40-100×/sec on the live-trade tables.
 * Cache them at module level keyed by a stable string so each combination is
 * built once and reused. Mirrors `createChartTimeFormatters` in chartTimezone.ts.
 */
const dtfCache = new Map<string, Intl.DateTimeFormat>();
function getDtf(key: string, build: () => Intl.DateTimeFormat): Intl.DateTimeFormat {
  let f = dtfCache.get(key);
  if (!f) {
    f = build();
    dtfCache.set(key, f);
  }
  return f;
}

function formatInstantParts(
  ms: number,
  timeZone: string,
  withFractionalSeconds: boolean,
): { date: string; time: string } | null {
  try {
    const dtf = getDtf(
      `parts|${timeZone}|${withFractionalSeconds}`,
      () =>
        new Intl.DateTimeFormat('en-US', {
          timeZone,
          year: 'numeric',
          month: '2-digit',
          day: '2-digit',
          hour: '2-digit',
          minute: '2-digit',
          second: '2-digit',
          ...(withFractionalSeconds ? { fractionalSecondDigits: 3 as const } : {}),
          hour12: false,
        }),
    );
    const parts = dtf.formatToParts(new Date(ms));
    const y = dtfPart(parts, 'year');
    const mo = dtfPart(parts, 'month');
    const da = dtfPart(parts, 'day');
    const h = dtfPart(parts, 'hour');
    const mi = dtfPart(parts, 'minute');
    const s = dtfPart(parts, 'second');
    const frac = withFractionalSeconds
      ? (dtfPart(parts, 'fractionalSecond') || '000').padStart(3, '0')
      : '';
    return {
      date: `${y}-${mo}-${da}`,
      time: withFractionalSeconds ? `${h}:${mi}:${s}.${frac}` : `${h}:${mi}:${s}`,
    };
  } catch {
    return null;
  }
}

export function formatIso(iso: string, timeZone: string): string {
  const { date, time } = formatIsoLines(iso, timeZone);
  return time ? `${date} ${time}` : date;
}

/** Date + time (with seconds and ms) for stacked table cells. */
export function formatIsoLines(
  iso: string,
  timeZone: string,
): { date: string; time: string } {
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return { date: iso, time: '' };
  const formatted = formatInstantParts(ms, timeZone, true);
  if (formatted) return formatted;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  const padMs = (n: number) => String(n).padStart(3, '0');
  return {
    date: `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`,
    time: `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}.${padMs(d.getUTCMilliseconds())}`,
  };
}

/** Compact table display: MM/DD HH:mm. */
export function formatIsoCompact(iso: string, timeZone: string): string {
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return iso;
  try {
    const parts = getDtf(
      `compact|${timeZone}`,
      () =>
        new Intl.DateTimeFormat('en-US', {
          timeZone,
          month: '2-digit',
          day: '2-digit',
          hour: '2-digit',
          minute: '2-digit',
          hour12: false,
        }),
    ).formatToParts(new Date(ms));
    const mo = dtfPart(parts, 'month');
    const da = dtfPart(parts, 'day');
    const h = dtfPart(parts, 'hour');
    const mi = dtfPart(parts, 'minute');
    return `${mo}/${da} ${h}:${mi}`;
  } catch {
    const d = new Date(ms);
    const pad = (n: number) => String(n).padStart(2, '0');
    return `${pad(d.getUTCMonth() + 1)}/${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
  }
}

/** Epoch ms → `YYYY-MM-DD HH:mm:ss` in the given timezone. */
export function formatTimestampMs(ms: number, timeZone: string): string {
  const formatted = formatInstantParts(ms, timeZone, false);
  if (formatted) return `${formatted.date} ${formatted.time}`;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
}

/** Epoch ms → `MM/DD HH:mm:ss` in the given timezone. */
export function formatTimestampMsCompact(ms: number, timeZone: string): string {
  if (!Number.isFinite(ms)) return String(ms);
  try {
    const parts = getDtf(
      `tscompact|${timeZone}`,
      () =>
        new Intl.DateTimeFormat('en-US', {
          timeZone,
          month: '2-digit',
          day: '2-digit',
          hour: '2-digit',
          minute: '2-digit',
          second: '2-digit',
          hour12: false,
        }),
    ).formatToParts(new Date(ms));
    const h = dtfPart(parts, 'hour');
    const mi = dtfPart(parts, 'minute');
    const s = dtfPart(parts, 'second');
    return `${h}:${mi}:${s}`;
  } catch {
    const d = new Date(ms);
    const pad = (n: number) => String(n).padStart(2, '0');
    // return `${pad(d.getUTCMonth() + 1)}/${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
    return `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
  }
}

export function isoHoursAgo(s: string): number | null {
  const ms = Date.parse(s);
  if (Number.isNaN(ms)) return null;
  return Math.max(0, Date.now() - ms) / 3_600_000;
}
