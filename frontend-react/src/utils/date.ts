export function formatIso(iso: string): string {
  const { date, time } = formatIsoLines(iso);
  return time ? `${date} ${time}` : date;
}

/** UTC date + time (with seconds) for stacked table cells. */
export function formatIsoLines(iso: string): { date: string; time: string } {
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return { date: iso, time: '' };
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return {
    date: `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`,
    time: `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`,
  };
}

/** Compact table display: MM/DD HH:mm (UTC). */
export function formatIsoCompact(iso: string): string {
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return iso;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getUTCMonth() + 1)}/${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
}

export function isoHoursAgo(s: string): number | null {
  const ms = Date.parse(s);
  if (Number.isNaN(ms)) return null;
  return Math.max(0, Date.now() - ms) / 3_600_000;
}
