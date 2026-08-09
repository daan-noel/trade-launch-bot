import { getUtcOffsetMinutes } from 'components/token-price-chart/chartTimezone';

function dtfPart(parts: Intl.DateTimeFormatPart[], type: Intl.DateTimeFormatPartTypes): string {
  return parts.find((p) => p.type === type)?.value ?? '';
}

/**
 * `Intl.DateTimeFormat` is an expensive constructor (full locale + timezone
 * resolution). These formatters are built with a tiny finite set of
 * (timezone, options) combinations. Reconstructing one per call means per table
 * cell, per render, ~40-100×/sec on the live-trade tables. Cache them at module
 * level keyed by a stable string so each combination is built once and reused.
 * Mirrors `createChartTimeFormatters` in chartTimezone.ts.
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

/** Epoch ms → `YYYY-MM-DD HH:mm:ss` in the given timezone. */
export function formatTimestampMs(ms: number, timeZone: string): string {
  const formatted = formatInstantParts(ms, timeZone, false);
  if (formatted) return `${formatted.date} ${formatted.time}`;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
}

type DateBound = 'lower' | 'upper';

const DT_LOCAL_RE = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/;

function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

/**
 * A `datetime-local` value (`"YYYY-MM-DDTHH:mm[:ss]"`) is a bare wall-clock with
 * no zone. The picker means that wall-clock **in the selected project timezone**
 * (the same zone the table displays times in), but the backend's `parse_dt`
 * reads the string as UTC. This converts the typed wall-clock from `timeZone`
 * into the equivalent UTC wall-clock and returns it as a 19-char
 * `"YYYY-MM-DDTHH:mm:ss"` string. The backend appends `Z` to any non-16-char
 * value, so the result is read as the exact UTC instant — no backend change.
 *
 * DST-aware via `getUtcOffsetMinutes` (which returns the offset *active at a
 * given instant*, never a hardcoded value). We probe the zone offset on both
 * sides of the typed day (±24h, far enough to straddle any single transition)
 * to collect every offset in effect near the wall-clock, then form a candidate
 * instant `naive - offset` for each. A candidate is a *valid* occurrence only if
 * the offset actually in effect at that instant matches the offset used.
 *
 *  - Normal time: exactly one valid candidate → exact.
 *  - Fall-back (clocks repeat 01:00–01:59): two valid candidates — the typed
 *    wall-clock occurs twice. `lower` takes the earliest, `upper` the latest, so
 *    the whole ambiguous hour stays inside a range filter.
 *  - Spring-forward gap (02:00–02:59 doesn't exist): no valid candidate; we fall
 *    back to the raw candidates' min/max, which collapse consistently to the
 *    transition boundary (lower ≤ upper always holds).
 *
 * `bound` only affects the result inside a transition; the inclusion-safe
 * tie-break guarantees a range never drops an intended token.
 *
 * Empty input → `''`. Unparseable input is passed through unchanged (defensive).
 */
export function datetimeLocalToUtcWallClock(
  value: string,
  timeZone: string,
  bound: DateBound,
): string {
  if (!value) return '';
  const m = DT_LOCAL_RE.exec(value);
  if (!m) return value;
  const [, y, mo, d, hh, mm, ss] = m;
  const naiveMs = Date.UTC(
    Number(y),
    Number(mo) - 1,
    Number(d),
    Number(hh),
    Number(mm),
    ss ? Number(ss) : 0,
  );

  const DAY_MS = 86_400_000;
  // Distinct offsets in effect near the typed wall-clock (covers both sides of a
  // transition). A Set dedupes the common single-offset case to one candidate.
  const offsets = new Set<number>([
    getUtcOffsetMinutes(timeZone, new Date(naiveMs - DAY_MS)),
    getUtcOffsetMinutes(timeZone, new Date(naiveMs)),
    getUtcOffsetMinutes(timeZone, new Date(naiveMs + DAY_MS)),
  ]);

  const allCands: number[] = [];
  const validCands: number[] = [];
  for (const off of offsets) {
    const cand = naiveMs - off * 60_000;
    allCands.push(cand);
    // Round-trip check: the wall-clock genuinely occurs at `cand` only if the
    // zone's offset *there* is the one we assumed (filters out gap-side guesses).
    if (getUtcOffsetMinutes(timeZone, new Date(cand)) === off) validCands.push(cand);
  }

  const pool = validCands.length > 0 ? validCands : allCands;
  const chosenMs = bound === 'lower' ? Math.min(...pool) : Math.max(...pool);

  const dt = new Date(chosenMs);
  return (
    `${dt.getUTCFullYear()}-${pad2(dt.getUTCMonth() + 1)}-${pad2(dt.getUTCDate())}` +
    `T${pad2(dt.getUTCHours())}:${pad2(dt.getUTCMinutes())}:${pad2(dt.getUTCSeconds())}`
  );
}

