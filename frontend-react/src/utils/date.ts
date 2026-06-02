export function formatIso(iso: string): string {
  const ms = Date.parse(iso);
  if (Number.isNaN(ms)) return iso;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}:${pad(d.getUTCSeconds())}`;
}

export function isoHoursAgo(s: string): number | null {
  const ms = Date.parse(s);
  if (Number.isNaN(ms)) return null;
  return Math.max(0, Date.now() - ms) / 3_600_000;
}
