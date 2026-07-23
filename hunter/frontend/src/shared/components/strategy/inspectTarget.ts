// Canonical `InspectTarget` shape + the record→target mappers shared by every
// strategy page (tpsl1 / tpsl2 / swing1, live + lab). Previously the interface was
// duplicated in both TokenInspectModal forks and the mappers were copy-pasted into
// all five page files; this is the single source.
import type { ChartEventMarker } from 'components/token-price-chart';
import type { ChartOverlayHook } from 'components/tokens/TokenChartsGrid';
import type { RulePositionRecord, SimulatedTokenResult } from 'types';

/** What the token-inspect chart modal needs to draw entry/exit markers for a run. */
export interface InspectTarget {
  mint_address: string;
  symbol?: string | null;
  /** Trigger-trade (signal) snapshot — the trade that armed this position, distinct
   *  from the actual entry fill. Drawn as a `signal` marker so the gap to the entry
   *  fill (slippage between "signal fired" and "we filled") is visible. Omit/null
   *  when there's no separate trigger (e.g. sim results without a target). */
  targetTime?: string | null;
  targetPrice?: number | null;
  targetTx?: string | null;
  entryTime: string | null;
  /** null for an armed position whose entry hasn't filled yet. */
  entryPrice: number | null;
  entryTx?: string | null;
  exitTime: string | null;
  exitPrice: number | null;
  exitTx?: string | null;
  /** Exit reason / position status (e.g. "TakeProfit"); appended to the exit label. */
  exitLabel?: string | null;
}

/** Build the chart **fill** entry/exit markers for an inspect target (backend
 *  run/position result). Shared by inspect modals and the per-row charts-grid
 *  overlay. Metric-pane condition fires use `role: 'signal'` separately. */
export function buildEventMarkers(target: InspectTarget): ChartEventMarker[] {
  const markers: ChartEventMarker[] = [];
  // Trigger (signal) point — only when it's a distinct trade from the entry fill.
  // If we filled on the very trade that armed us (same tx), the gap is zero and a
  // second pointer at the same bar is pure noise, so skip it.
  const sameAsEntry =
    target.targetTx != null &&
    target.entryTx != null &&
    target.targetTx === target.entryTx;
  if (target.targetTime != null && target.targetPrice != null && !sameAsEntry) {
    markers.push({
      kind: 'entry',
      role: 'signal',
      time: target.targetTime,
      priceInSol: target.targetPrice,
      txSignature: target.targetTx ?? null,
      label: 'Signal',
    });
  }
  if (target.entryTime != null && target.entryPrice != null) {
    markers.push({
      kind: 'entry',
      role: 'fill',
      time: target.entryTime,
      priceInSol: target.entryPrice,
      txSignature: target.entryTx ?? null,
      label: 'Entry',
    });
  }
  if (target.exitTime != null && target.exitPrice != null) {
    markers.push({
      kind: 'exit',
      role: 'fill',
      time: target.exitTime,
      priceInSol: target.exitPrice,
      txSignature: target.exitTx ?? null,
      label: target.exitLabel ? `Exit · ${target.exitLabel}` : 'Exit',
    });
  }
  return markers;
}

/** Build the chart markers for **all re-entry episodes** of one token — the union
 *  of every episode's signal/entry/exit markers on a single chart, so a token a
 *  rule re-entered N times shows N entry arrows + N exit arrows together. Episodes
 *  are ordered by entry time and, when there's more than one, each marker's label is
 *  suffixed with its episode number (`Entry 1` … `Exit 3`) so overlapping arrows stay
 *  legible. A single-episode token is unchanged (no suffix). Pass every fired
 *  `InspectTarget` for the mint; unentered/`NoEntry` targets contribute nothing. */
export function buildEventMarkersForEpisodes(targets: InspectTarget[]): ChartEventMarker[] {
  const ordered = [...targets].sort((a, b) => {
    const ta = a.entryTime ? Date.parse(a.entryTime) : 0;
    const tb = b.entryTime ? Date.parse(b.entryTime) : 0;
    return ta - tb;
  });
  const numbered = ordered.length > 1;
  return ordered.flatMap((t, i) => {
    const markers = buildEventMarkers(t);
    if (!numbered) return markers;
    const n = i + 1;
    return markers.map((m) => ({ ...m, label: `${m.label} ${n}` }));
  });
}

/** A charts-grid overlay hook that draws only entry/exit markers (no swing legs) —
 *  the tpsl case. Derives from row data, so it calls no hooks and is trivially safe
 *  to invoke per card. */
export function markerRowOverlay<R>(toTarget: (r: R) => InspectTarget): ChartOverlayHook<R> {
  return (row) => ({ eventMarkers: buildEventMarkers(toTarget(row)) });
}

/** Mint-only inspect (no fill markers) — Creation Stats drill-down, universe browse. */
export function inspectFromMint(
  mint_address: string,
  symbol?: string | null,
): InspectTarget {
  return {
    mint_address,
    symbol: symbol ?? null,
    entryTime: null,
    entryPrice: null,
    exitTime: null,
    exitPrice: null,
  };
}

/** Map a backtest/simulate result row to an inspect target. */
export function inspectFromSim(r: SimulatedTokenResult): InspectTarget {
  const fired = r.fired !== false && r.exit_reason !== 'NoEntry';
  return {
    mint_address: r.mint_address,
    symbol: r.symbol,
    entryTime: r.entry_time,
    entryPrice: r.entry_price,
    entryTx: r.entry_tx,
    exitTime: r.exit_time,
    exitPrice: r.exit_price,
    exitTx: r.exit_tx,
    exitLabel:
      fired && r.exit_reason && r.exit_reason !== 'Open' ? r.exit_reason : null,
  };
}

/** Stable per-row identity for engine-run result rows that carry no DB `id`
 *  (`SimulatedTokenResult` / `ComboTokenResult`). Re-entry (cooldown + episode
 *  cap) lets a rule/combo enter the same mint more than once in a single run, so
 *  `mint_address` alone is no longer unique — `entry_time` is (one fill per
 *  episode). A never-entered `NoEntry` row has no `entry_time` and is unique by
 *  mint alone (a matched-but-unentered token can't loop). Use as the table
 *  `rowKey` / charts-grid `rowKey` / inspect-selection key for any row source
 *  that can re-enter. `RulePositionRecord` doesn't need this — it has a real
 *  DB `id`. */
export function episodeRowKey(r: { mint_address: string; entry_time?: string | null }): string {
  return r.entry_time ? `${r.mint_address}::${r.entry_time}` : r.mint_address;
}

/** Map a live/paper position row to an inspect target. */
export function inspectFromPosition(r: RulePositionRecord): InspectTarget {
  return {
    mint_address: r.mint_address,
    targetTime: r.target_time,
    targetPrice: r.target_price,
    targetTx: r.target_tx,
    entryTime: r.entry_time,
    entryPrice: r.entry_price,
    entryTx: r.entry_tx,
    exitTime: r.exit_time,
    exitPrice: r.exit_price,
    exitTx: r.exit_tx,
    exitLabel: r.exit_reason ?? null,
  };
}
