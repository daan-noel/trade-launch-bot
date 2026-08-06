export function formatDecimal(value: number, decimals: number): string {
  return value.toFixed(decimals);
}

export function formatDecimalTrim(value: number, decimals: number): string {
  const s = value.toFixed(decimals);
  if (!s.includes('.')) return s;
  const trimmed = s.replace(/0+$/, '').replace(/\.$/, '');
  return trimmed || s;
}

/**
 * Collapse f32/REAL round-trip noise on coarse SOL knobs (`bucket_size_amount`,
 * `bucket_width_sol`). Mirrors Rust `tidy_sol_decimal`: `0.10000000149011612` → `0.1`.
 * ~7 significant digits ≈ IEEE-754 binary32.
 */
export function tidySolDecimal(sol: number): number {
  if (!Number.isFinite(sol)) return sol;
  return Number(sol.toPrecision(7));
}

export function formatPrice(value: number): string {
  if (value === 0) return '0';
  const abs = Math.abs(value);
  if (abs >= 1) return formatDecimalTrim(value, 6);

  const exponent = -Math.floor(Math.log10(abs));
  let engineeringExponent: number;
  if (exponent <= 3) engineeringExponent = -3;
  else if (exponent <= 6) engineeringExponent = -6;
  else if (exponent <= 9) engineeringExponent = -9;
  else if (exponent <= 12) engineeringExponent = -12;
  else if (exponent <= 15) engineeringExponent = -15;
  else return value.toExponential(6);

  const mantissa = value / 10 ** engineeringExponent;
  return `${formatDecimalTrim(mantissa, 6)}e${engineeringExponent}`;
}

export function formatCompact(value: number, decimals: number): string {
  if (value === 0) return '0';
  const abs = Math.abs(value);
  const sign = value < 0 ? '-' : '';

  if (abs >= 1_000_000_000) return `${sign}${formatDecimalTrim(abs / 1_000_000_000, decimals)}G`;
  if (abs >= 1_000_000) return `${sign}${formatDecimalTrim(abs / 1_000_000, decimals)}M`;
  if (abs >= 1_000) return `${sign}${formatDecimalTrim(abs / 1_000, decimals)}K`;
  if (abs < 1e-6) return `${sign}${abs.toExponential(decimals)}`;
  return formatDecimalTrim(value, decimals);
}

export function truncate(s: string, maxLen: number): string {
  return s.length <= maxLen ? s : `${s.slice(0, maxLen)}…`;
}

export function formatAge(seconds: number): string {
  if (seconds < 0) return '?';
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    return m === 0 ? `${h}h` : `${h}h ${m}m`;
  }
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  return h === 0 ? `${d}d` : `${d}d ${h}h`;
}

/**
 * Full age breakdown down to the second, e.g. `30s`, `12m 30s`, `3h 12m 30s`,
 * `1D 3h 12m 30s`. Unlike {@link formatAge} no unit is collapsed — every unit
 * from the largest non-zero one down through seconds is shown. The day unit is
 * uppercase (`D`); hour/minute/second stay lowercase.
 */
export function formatAgePrecise(seconds: number): string {
  if (seconds < 0) return '?';
  const s = Math.floor(seconds);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const parts: string[] = [];
  if (d > 0) parts.push(`${d}D`);
  if (d > 0 || h > 0) parts.push(`${h}h`);
  if (d > 0 || h > 0 || m > 0) parts.push(`${m}m`);
  parts.push(`${sec}s`);
  return parts.join(' ');
}

export function formatWithCommas(n: number): string {
  return n.toLocaleString('en-US', { maximumFractionDigits: 0 });
}

/**
 * Compact duration for live countdowns/elapsed (e.g. `45s`, `2m 15s`, `1h 3m`).
 * Unlike {@link formatAge} this keeps the second-level unit under an hour, so a
 * progress ETA visibly ticks down. Sub-second / negative inputs clamp to `0s`.
 *
 * Also the ONE formatter for position Age / Held cells (Console open, attention
 * and waiting lanes, History `Held`, the position detail modals).
 */
export function formatDurationShort(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  if (s < 86400) {
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    return `${h}h ${m}m`;
  }
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  return `${d}d ${h}h`;
}

/**
 * Single-unit duration from milliseconds for chart tooltips (`1.5s`, `2.3m`,
 * `1.1h`). Picks the largest unit under which the value is < 1 and trims to one
 * decimal. `undefined` renders an em dash.
 */
export function formatDurationMs(ms: number | undefined): string {
  if (ms == null) return '—';
  const sec = ms / 1000;
  if (sec < 60) return `${formatDecimalTrim(sec, 1)}s`;
  if (sec < 3600) return `${formatDecimalTrim(sec / 60, 1)}m`;
  return `${formatDecimalTrim(sec / 3600, 1)}h`;
}

/**
 * Format a USD value: engineering-notation cents for sub-$0.01 prices,
 * comma-grouped dollars-and-cents otherwise.
 */
export function formatUsd(value: number): string {
  if (value === 0) return '$0';
  const abs = Math.abs(value);
  const sign = value < 0 ? '-' : '';
  if (abs < 0.01) return `${sign}$${formatPrice(abs)}`;
  const rounded = Math.round(abs * 100) / 100;
  const whole = Math.trunc(rounded);
  const frac = Math.round((rounded - whole) * 100);
  const wholeStr = formatWithCommas(whole);
  if (frac === 0) return `${sign}$${wholeStr}`;
  return `${sign}$${wholeStr}.${String(frac).padStart(2, '0')}`;
}

/** True when a numeric cell should show "-" (null, undefined, zero, NaN). */
export function isEmptyNum(n: number | null | undefined): boolean {
  return n == null || n === 0 || Number.isNaN(n);
}

export function ageClass(seconds: number): string {
  if (seconds < 3600) return 'text-red font-semibold';
  if (seconds < 86400) return 'text-warning font-semibold';
  if (seconds < 604800) return 'text-green';
  return 'text-[#555]';
}

export function priceClass(price: number | null | undefined): string {
  if (price == null || price === 0) return 'text-text';
  const abs = Math.abs(price);
  if (abs >= 1) return 'text-text';
  if (abs >= 1e-3) return 'text-info';
  if (abs >= 1e-6) return 'text-[#7cdbff]';
  if (abs >= 1e-9) return 'text-[#82b7ff]';
  if (abs >= 1e-12) return 'text-accent';
  if (abs >= 1e-15) return 'text-[#ff8bce]';
  return 'text-warning';
}

export function ratioClass(mult: number | null | undefined): string {
  if (mult == null) return 'text-[#555]';
  if (mult >= 100) return 'text-red font-extrabold';
  if (mult >= 30) return 'text-accent font-bold';
  if (mult >= 10) return 'text-warning font-bold';
  if (mult >= 3) return 'text-green font-semibold';
  if (mult >= 1.5) return 'text-info font-medium';
  return 'text-[#555]';
}

export function ratioVariant(mult: number | null | undefined): StatVariant {
  if (mult == null) return 'muted';
  if (mult >= 100) return 'danger';
  if (mult >= 30) return 'accent';
  if (mult >= 10) return 'warning';
  if (mult >= 3) return 'primary';
  if (mult >= 1.5) return 'info';
  return 'muted';
}

export type StatVariant = 'default' | 'primary' | 'warning' | 'danger' | 'info' | 'accent' | 'muted';

export function statVariantClass(v: StatVariant): string {
  const map: Record<StatVariant, string> = {
    default: 'text-text',
    primary: 'text-primary font-semibold',
    warning: 'text-warning font-semibold',
    danger: 'text-red font-bold',
    info: 'text-info font-semibold',
    accent: 'text-accent font-bold',
    muted: 'text-text-dim',
  };
  return map[v];
}
