/**
 * Exit-reason display + filter vocabulary (SSOT).
 *
 * Persisted strings come from the engine (`TakeProfit`, `stall > 3`, `Open`, …).
 * Metric-condition exits store spaced `name op value`; legacy rows may still
 * hold bare `Metrics` or compact `stall>`. Badge rendering separates the three
 * tokens visually when the spaced form is present.
 */

import type { WindowSpec } from './windowSpec';

export interface MetricExitParts {
  name: string;
  op: string;
  /** Threshold string as stored (may be empty for legacy compact `stall>`). */
  value: string;
}

/** The window qualifier a dynamic metric's name carries: `(2s)`, `(30sl)`,
 *  `(30sl@1)`. Mirrors the Rust `event::format_metric_exit_name` — the whole span,
 *  because two reqs differing only in unit or lag read different tape and must not
 *  print identically. A static metric carries no qualifier at all. */
const WINDOW_QUALIFIER = String.raw`(?:\((\d*\.?\d+)(s|sl)(?:@(\d*\.?\d+))?\))?`;

/** Spaced `name[(window)] op value` (e.g. `stall > 3`, `nonvol_buy(2s) >= 0.9`). */
const METRIC_EXIT_SPACED = new RegExp(
  String.raw`^([a-z][a-z0-9_]*)${WINDOW_QUALIFIER} (>=|<=|!=|>|<|=) ([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)$`,
);

/** Legacy compact `{name}{op}` (e.g. `stall>`). */
const METRIC_EXIT_COMPACT = /^([a-z][a-z0-9_]*)(>=|<=|!=|>|<|=)$/;

/** Parse a metric-exit label into display parts (spaced or compact). */
export function parseMetricExitParts(
  reason: string | null | undefined,
): MetricExitParts | null {
  if (!reason) return null;
  const spaced = METRIC_EXIT_SPACED.exec(reason.trim());
  if (spaced) {
    // The qualifier stays ON the name: it is what separates a dynamic group's read
    // from its identically-named lifetime twin, so a badge that dropped it would
    // show two different exits under one label.
    const window = spaced[2] ? `(${spaced[2]}${spaced[3]}${spaced[4] ? `@${spaced[4]}` : ''})` : '';
    return { name: `${spaced[1]}${window}`, op: spaced[5], value: spaced[6] };
  }
  const compact = METRIC_EXIT_COMPACT.exec(reason.trim());
  if (compact) {
    return { name: compact[1], op: compact[2], value: '' };
  }
  return null;
}

/** True for bare legacy `Metrics` or a metric detail label. */
export function isMetricExitReason(reason: string | null | undefined): boolean {
  if (!reason) return false;
  return reason === 'Metrics' || parseMetricExitParts(reason) != null;
}

/** Which authored condition a metric exit reason names. */
export interface MetricExitTarget {
  metric: string;
  /** The trailing window the label qualifies itself with, `null` when it carries
   *  none — and a bare name is genuinely ambiguous whenever the rule authors both
   *  a windowed condition and its identically-named lifetime twin.
   *
   *  The WHOLE span, not a size: `(30s)` and `(30sl)` name different windows, and
   *  so do `(30sl)` and `(30sl@1)`. */
  window: WindowSpec | null;
}

/**
 * The condition a persisted exit reason points at, for surfaces that draw *that*
 * condition (the chart's value lane).
 *
 * Accepts every stored form: `nonvol_buy(2s) >= 0.9` and `buy_count(30sl@1) >= 3`
 * (current — the window qualifier is what separates a dynamic group from its
 * lifetime twin), the bare `nonvol_buy >= 0.9` still on older rows, and legacy
 * compact `stall>`. Returns
 * `null` for a non-metric reason (`TakeProfit`, `Dead`, …), which names no
 * condition to draw.
 */
export function parseMetricExitTarget(
  reason: string | null | undefined,
): MetricExitTarget | null {
  if (!reason) return null;
  const m = new RegExp(
    String.raw`^\s*([a-z][a-z0-9_]*)${WINDOW_QUALIFIER}\s*(?:>=|<=|!=|>|<|=)`,
    'i',
  ).exec(reason);
  if (!m) return null;
  const window: WindowSpec | null = m[2]
    ? { size: Number(m[2]), lag: m[4] ? Number(m[4]) : 0, unit: m[3] === 'sl' ? 'slot' : 'sec' }
    : null;
  return { metric: m[1], window };
}

/** Format a metric fire as spaced `name op value` (frontend signal markers). */
export function formatMetricExitLabel(
  name: string,
  op: string,
  value: number,
): string {
  return `${name} ${op} ${formatMetricThreshold(value)}`;
}

/** Compact threshold for labels — mirrors the engine's format_metric_threshold. */
export function formatMetricThreshold(v: number): string {
  if (!Number.isFinite(v)) return String(v);
  if (v === 0) return '0';
  if (Number.isInteger(v) && Math.abs(v) < 1e15) return String(v);
  const s = v.toFixed(6);
  return s.replace(/\.?0+$/, '');
}

/** Compact badge label for a persisted `exit_reason`. Still-open / null →
 *  `"Open"`. Metric detail forms render as stored (spaced). Legacy bare
 *  `Metrics` still splits win/loss via `pnlSol`. */
export function exitReasonLabel(
  reason: string | null | undefined,
  pnlSol?: number | null,
): string {
  switch (reason) {
    case 'LiquidityExit':
      return 'LIQ';
    case 'TakeProfit':
      return 'TP';
    case 'StopLoss':
      return 'SL';
    case 'TrailingStop':
      return 'TRAIL';
    case 'Stall':
      return 'STALL';
    case 'TimeStop':
      return 'TIME';
    case 'ExitFailed':
      return 'FAIL';
    case 'Manual':
    case 'ManualClose':
      return 'MANUAL';
    case 'Dead':
      return 'DEAD';
    case 'Metrics':
      return metricsExitLabel(pnlSol);
    case 'Migrated':
      return 'MIG';
    case 'NoEntry':
      return 'No entry';
    case 'Open':
    case null:
    case undefined:
    case '':
      return 'Open';
    default:
      return reason;
  }
}

/** Legacy bare `Metrics` → `METRIC+` / `METRIC-` / `METRIC` from PnL sign. */
export function metricsExitLabel(pnlSol?: number | null): string {
  if (pnlSol == null || !Number.isFinite(pnlSol) || pnlSol === 0) return 'METRIC';
  return pnlSol > 0 ? 'METRIC+' : 'METRIC-';
}

/** Per-column filter/search haystack. */
export function exitReasonSearchText(
  reason: string | null | undefined,
  pnlSol?: number | null,
): string {
  const stored = reason?.trim() ? reason.trim() : 'Open';
  const label = exitReasonLabel(reason, pnlSol);
  if (isMetricExitReason(reason)) {
    const parts = new Set([stored, 'METRIC', label]);
    const parsed = parseMetricExitParts(reason);
    if (parsed) {
      parts.add(parsed.name);
      parts.add(parsed.op);
      if (parsed.value) parts.add(parsed.value);
    }
    return [...parts].join(' ');
  }
  return label === stored ? stored : `${stored} ${label}`;
}

/** Badge / loose labels → persisted `exit_reason` for server-side contains. */
const EXIT_REASON_FILTER_ALIASES: Readonly<Record<string, string>> = {
  tp: 'TakeProfit',
  takeprofit: 'TakeProfit',
  'take profit': 'TakeProfit',
  sl: 'StopLoss',
  stoploss: 'StopLoss',
  'stop loss': 'StopLoss',
  trail: 'TrailingStop',
  trailing: 'TrailingStop',
  trailingstop: 'TrailingStop',
  stall: 'Stall',
  time: 'TimeStop',
  timestop: 'TimeStop',
  liq: 'LiquidityExit',
  liquidity: 'LiquidityExit',
  liquidityexit: 'LiquidityExit',
  fail: 'ExitFailed',
  exitfailed: 'ExitFailed',
  manual: 'Manual',
  manualclose: 'Manual',
  dead: 'Dead',
  metric: 'Metrics',
  metrics: 'Metrics',
  'metric+': 'Metrics',
  'metrics+': 'Metrics',
  'metric-': 'Metrics',
  'metrics-': 'Metrics',
  mig: 'Migrated',
  migrated: 'Migrated',
  open: 'Open',
  noentry: 'NoEntry',
  'no entry': 'NoEntry',
  'not fired': 'NoEntry',
};

export function normalizeExitReasonFilter(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return trimmed;
  return EXIT_REASON_FILTER_ALIASES[trimmed.toLowerCase()] ?? trimmed;
}

export function isExitReasonFilterKey(key: string): boolean {
  return key === 'reason' || key === 'exit_reason';
}
