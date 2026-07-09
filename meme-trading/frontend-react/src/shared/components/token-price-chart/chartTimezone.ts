import type { Time } from 'lightweight-charts';

export function getDefaultChartTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    return 'UTC';
  }
}

export function isValidTimezone(tz: string): boolean {
  if (!tz) return false;
  try {
    Intl.DateTimeFormat(undefined, { timeZone: tz });
    return true;
  } catch {
    return false;
  }
}

let cachedTimezones: readonly string[] | null = null;

/** Full IANA timezone list (sorted), when the runtime supports it. */
export function getIanaTimezones(): readonly string[] {
  if (cachedTimezones) return cachedTimezones;
  try {
    if (typeof Intl.supportedValuesOf === 'function') {
      cachedTimezones = Intl.supportedValuesOf('timeZone').slice().sort();
      return cachedTimezones;
    }
  } catch {
    /* ignore */
  }
  cachedTimezones = ['UTC', getDefaultChartTimezone()].sort();
  return cachedTimezones;
}

/** City/region label from an IANA id (last path segment, underscores → spaces). */
export function formatTimezoneCityName(timeZone: string): string {
  if (timeZone === 'UTC' || timeZone === 'Etc/UTC' || timeZone === 'Etc/GMT') {
    return 'UTC';
  }
  const slash = timeZone.lastIndexOf('/');
  const segment = slash >= 0 ? timeZone.slice(slash + 1) : timeZone;
  return segment.replace(/_/g, ' ');
}

/** Current UTC offset in minutes (positive = east of UTC). */
export function getUtcOffsetMinutes(timeZone: string, at: Date = new Date()): number {
  try {
    const part = new Intl.DateTimeFormat('en-US', {
      timeZone,
      timeZoneName: 'longOffset',
    })
      .formatToParts(at)
      .find((p) => p.type === 'timeZoneName')?.value;
    if (part) {
      const match = part.match(/([+-])(\d{1,2})(?::(\d{2}))?/);
      if (match) {
        const sign = match[1] === '+' ? 1 : -1;
        const hours = Number.parseInt(match[2], 10);
        const mins = match[3] ? Number.parseInt(match[3], 10) : 0;
        return sign * (hours * 60 + mins);
      }
      if (part === 'GMT' || part === 'UTC') return 0;
    }
  } catch {
    /* ignore */
  }
  return 0;
}

export function formatUtcOffsetLabel(offsetMinutes: number): string {
  if (offsetMinutes === 0) return 'UTC';
  const sign = offsetMinutes > 0 ? '+' : '-';
  const abs = Math.abs(offsetMinutes);
  const h = Math.floor(abs / 60);
  const m = abs % 60;
  if (m === 0) return `UTC${sign}${h}`;
  return `UTC${sign}${h}:${String(m).padStart(2, '0')}`;
}

/** e.g. `UTC+8 Shanghai` */
export function formatTimezoneOptionLabel(timeZone: string, at: Date = new Date()): string {
  const offset = formatUtcOffsetLabel(getUtcOffsetMinutes(timeZone, at));
  const city = formatTimezoneCityName(timeZone);
  return `${offset} ${city}`;
}

export type TimezoneSelectOption = { id: string; label: string; offsetMinutes: number };

let cachedSelectOptions: readonly TimezoneSelectOption[] | null = null;

export function getTimezoneSelectOptions(extraId?: string): readonly TimezoneSelectOption[] {
  const zones = new Set(getIanaTimezones());
  if (extraId) zones.add(extraId);

  const at = new Date();
  const options: TimezoneSelectOption[] = [...zones].map((id) => ({
    id,
    label: formatTimezoneOptionLabel(id, at),
    offsetMinutes: getUtcOffsetMinutes(id, at),
  }));
  options.sort(
    (a, b) => a.offsetMinutes - b.offsetMinutes || a.label.localeCompare(b.label),
  );

  if (!extraId && cachedSelectOptions) return cachedSelectOptions;
  if (!extraId) cachedSelectOptions = options;
  return options;
}

function timeToSeconds(time: Time): number | null {
  if (typeof time === 'number') return time;
  if (typeof time === 'object' && time !== null && 'year' in time) {
    return Math.floor(Date.UTC(time.year, time.month - 1, time.day) / 1000);
  }
  return null;
}

export function createChartTimeFormatters(timezone: string) {
  const crosshairFmt = new Intl.DateTimeFormat(undefined, {
    timeZone: timezone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
  const tickFmt = new Intl.DateTimeFormat(undefined, {
    timeZone: timezone,
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });

  const format = (time: Time, dtf: Intl.DateTimeFormat) => {
    const sec = timeToSeconds(time);
    if (sec == null) return '';
    return dtf.format(sec * 1000);
  };

  return {
    timeFormatter: (time: Time) => format(time, crosshairFmt),
    tickMarkFormatter: (time: Time) => format(time, tickFmt),
  };
}
