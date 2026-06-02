import { formatDecimalTrim } from '../../utils/format';

export function dashNum(val: number | null | undefined, zero = 0): string {
  if (val == null || val === zero) return '-';
  return String(val);
}

export function dashF(val: number, precision: number): string {
  if (val === 0) return '-';
  return formatDecimalTrim(val, precision);
}

export function dashPercent(val: number): string {
  if (val === 0) return '-';
  return `${formatDecimalTrim(val, 1)}%`;
}

export function parseIxLabels(raw: string): unknown[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];
  if (trimmed.startsWith('[')) {
    try {
      const parsed = JSON.parse(trimmed) as unknown;
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }
  return trimmed
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
}

export function fmtTime(iso: string | null | undefined): string {
  if (!iso) return '—';
  return iso.slice(0, 16).replace('T', ' ');
}

export const EXAMPLE_IX_LABELS =
  '["Compute Budget: SetComputeUnitLimit", "Compute Budget: SetComputeUnitPrice", "Pump.Fun: Create_v2", "Associated Token: CreateIdempotent", "Pump.Fun: Buy", "System Program: Transfer"]';
