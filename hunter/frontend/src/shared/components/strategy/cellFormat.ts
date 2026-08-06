// Shared strategy-table cell formatters + instruction-label parsing. Single source
// for what used to be byte-identical `tpsl1/utils.ts` and `tpsl2/utils.ts` copies —
// both strategy families (and the shared rule form / params engine) import from here.
import { formatDecimalTrim } from 'utils/format';

export function dashPercent(val: number | null | undefined): string {
  if (val == null || val === 0 || Number.isNaN(val)) return '-';
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
