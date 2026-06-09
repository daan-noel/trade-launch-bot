export function formatDecimal(value: number, decimals: number): string {
  return value.toFixed(decimals);
}

export function formatDecimalTrim(value: number, decimals: number): string {
  const s = value.toFixed(decimals);
  if (!s.includes('.')) return s;
  const trimmed = s.replace(/0+$/, '').replace(/\.$/, '');
  return trimmed || s;
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

export function formatWithCommas(n: number): string {
  return n.toLocaleString('en-US', { maximumFractionDigits: 0 });
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
