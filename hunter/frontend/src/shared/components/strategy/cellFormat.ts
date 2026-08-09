// Shared strategy-table cell formatters + instruction-label parsing. THE single
// source: both strategy families (and the shared rule form / params engine) import
// from here, so a per-family copy of these formatters is always a bug.
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
