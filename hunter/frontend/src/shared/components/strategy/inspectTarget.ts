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

/** Build the chart entry/exit markers for an inspect target. Shared by both the
 *  inspect modals and the per-row charts-grid overlay, so the "chart" button's
 *  inline charts mark entry/exit identically to the modal. */
export function buildEventMarkers(target: InspectTarget): ChartEventMarker[] {
  const markers: ChartEventMarker[] = [];
  if (target.entryTime != null && target.entryPrice != null) {
    markers.push({
      kind: 'entry',
      time: target.entryTime,
      priceInSol: target.entryPrice,
      txSignature: target.entryTx ?? null,
      label: 'Entry',
    });
  }
  if (target.exitTime != null && target.exitPrice != null) {
    markers.push({
      kind: 'exit',
      time: target.exitTime,
      priceInSol: target.exitPrice,
      txSignature: target.exitTx ?? null,
      label: target.exitLabel ? `Exit · ${target.exitLabel}` : 'Exit',
    });
  }
  return markers;
}

/** A charts-grid overlay hook that draws only entry/exit markers (no swing legs) —
 *  the tpsl case. Derives from row data, so it calls no hooks and is trivially safe
 *  to invoke per card. */
export function markerRowOverlay<R>(toTarget: (r: R) => InspectTarget): ChartOverlayHook<R> {
  return (row) => ({ eventMarkers: buildEventMarkers(toTarget(row)) });
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

/** Map a live/paper position row to an inspect target. */
export function inspectFromPosition(r: RulePositionRecord): InspectTarget {
  return {
    mint_address: r.mint_address,
    entryTime: r.entry_time,
    entryPrice: r.entry_price,
    entryTx: r.entry_tx,
    exitTime: r.exit_time,
    exitPrice: r.exit_price,
    exitTx: r.exit_tx,
    exitLabel: r.exit_reason ?? null,
  };
}
