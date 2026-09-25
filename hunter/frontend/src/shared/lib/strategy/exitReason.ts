/**
 * Exit-reason display + filter vocabulary (SSOT).
 *
 * The engine books `TakeProfit`, `StopLoss`, `Dead`, `Manual`, `Migrated`, or the
 * label of the rule line that sold. A line with no label of its own sells as its first
 * condition, `m_flow.buy_sol @!volume [10s] >= 2`; a labelled one as its label
 * (`spike`). Rows from before the v2 metric system hold `untagged_buy(2s) >= 0.9`,
 * compact `stall>` or bare `Metrics`, and are shown as stored.
 */

import { formatMetricThreshold, type WindowSpec } from './windowSpec';

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

/** A v2 auto label: the read's full label, then `op value`
 *  (`m_flow.buy_sol @!volume [10s] >= 2`, `m_state.age_sec < 20`). */
const LINE_AUTO_LABEL = new RegExp(
  String.raw`^(m_[a-z]+\.[a-z0-9_]+(?: @!?[a-z0-9_]+)?(?: \[[^\]]+\])?) (>=|<=|!=|>|<|=) ([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)$`,
);

/** The names the engine books for exits that are not a rule line. */
const NAMED_EXITS = new Set([
  'TakeProfit',
  'StopLoss',
  'Dead',
  'Manual',
  'ManualClose',
  'Migrated',
  'ExitFailed',
  'LiquidityExit',
  'TrailingStop',
  'Stall',
  'TimeStop',
  'NoEntry',
  'Open',
]);

/** Spaced `name[(window)] op value`: a pre-v2 row (`stall > 3`, `untagged_buy(2s) >= 0.9`). */
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
  const v2 = LINE_AUTO_LABEL.exec(reason.trim());
  if (v2) return { name: v2[1], op: v2[2], value: v2[3] };
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

/** True when a rule line sold: any reason that is not one of the engine's named
 *  exits (a line's own label, an auto label, or a pre-v2 metric label / `Metrics`). */
export function isMetricExitReason(reason: string | null | undefined): boolean {
  const r = reason?.trim();
  if (!r) return false;
  return !NAMED_EXITS.has(r);
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
 * Accepts every stored form: `untagged_buy(2s) >= 0.9` and `buy_count(30sl@1) >= 3`
 * (current — the window qualifier is what separates a dynamic group from its
 * lifetime twin), the bare `untagged_buy >= 0.9` still on older rows, and legacy
 * compact `stall>`. Returns
 * `null` for a non-metric reason (`TakeProfit`, `Dead`, …), which names no
 * condition to draw.
 */
export function parseMetricExitTarget(
  reason: string | null | undefined,
): MetricExitTarget | null {
  if (!reason) return null;
  // A v2 auto label names the whole read, tag and span included.
  const v2 = LINE_AUTO_LABEL.exec(reason.trim());
  if (v2) return { metric: v2[1], window: null };
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

export { formatMetricThreshold };

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
